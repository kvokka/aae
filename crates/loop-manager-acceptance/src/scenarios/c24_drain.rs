//! Criterion 24 on the real br and the real `claude` against a fake Anthropic API: a task becomes the root bead of
//! a run of `ship`; its exec step `plan` creates four beads under the root with rig prefixes and agent and harness
//! labels; the drain step gives each a run of `work` (an exec step, then an ai step on the agent of the bead): A on `web`, B on
//! `api` once A is closed, F under the epic S fails, H names another harness and gets no run; the exec step `report`
//! sees what was closed, failed and refused. A `kill -9` of the daemon in each step: no step and no bead runs twice.

use super::{br, claude_agent, tracker};
use crate::{
    Env,
    daemon::{Daemon, write},
};
use loop_manager_testkit::anthropic::{Provider, Reply};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

const WITHIN: Duration = Duration::from_secs(90);

const SHIP: &str = "name = \"ship\"\n[inputs]\ntask = { type = \"task\", required = true }\n\
    [[step]]\nname = \"plan\"\nkind = \"exec\"\ncommand = [\"sh\", \"{dir}/plan.sh\"]\ntimeout = \"120s\"\n\
    [[step]]\nname = \"drain\"\nkind = \"drain\"\nformula = \"work\"\npool = 4\n\
    [[step]]\nname = \"report\"\nkind = \"exec\"\ncommand = [\"sh\", \"{dir}/report.sh\"]\ntimeout = \"120s\"\n";

/// The ai step names `ops`; the label `agent:dev` of a bead gives its run `dev`, and with it `dev`'s template.
const WORK: &str = "name = \"work\"\n[inputs]\nbead = { type = \"task\", required = true }\n\
    [[step]]\nname = \"do\"\nkind = \"exec\"\ncommand = [\"sh\", \"{dir}/work.sh\"]\ntimeout = \"120s\"\n\
    [[step]]\nname = \"sign\"\nkind = \"ai\"\nagent = \"ops\"\n";

/// The first attempt holds until the daemon dies; the second creates the beads and keeps their ids. A bead of a rig
/// is filed with the rig's prefix (`w`, `a`) and `parent-child`, as `--parent` would give it the parent's id. F is a
/// grandchild of the root, under the epic S: S keeps an open child and is never ready.
const PLAN: &str = "set -e\nD={dir}\nif [ ! -e $D/plan.held ]; then touch $D/plan.held; exec sleep 60; fi\n\
    new() { BD_ISSUE_PREFIX=$1 br create \"$2\" --deps \"parent-child:$LOOP_MANAGER_BEAD$4\" -l \"$3\" --silent; }\n\
    a=$(new w A agent:dev,harness:claude)\n\
    b=$(new a B agent:dev,harness:claude \",blocks:$a\")\n\
    s=$(br create S -t epic --parent \"$LOOP_MANAGER_BEAD\" --silent)\n\
    f=$(BD_ISSUE_PREFIX=a br create F --deps \"parent-child:$s\" -l agent:dev,harness:claude --silent)\n\
    h=$(new w H agent:dev,harness:codex)\n\
    printf '%s %s %s %s %s' \"$a\" \"$b\" \"$f\" \"$h\" \"$s\" > $D/ids\n";

/// The first attempt on A holds until the daemon dies; a command that ran to its end is in the log; F fails.
const WORK_SH: &str = "D={dir}\nset -- $(cat $D/ids)\n\
    if [ \"$LOOP_MANAGER_BEAD\" = \"$1\" ] && [ ! -e $D/a.held ]; then touch $D/a.held; exec sleep 60; fi\n\
    echo \"$LOOP_MANAGER_BEAD\" >> $D/work.log\n[ \"$LOOP_MANAGER_BEAD\" != \"$3\" ] || exit 3\n";

const REPORT: &str = "D={dir}\nif [ ! -e $D/report.held ]; then touch $D/report.held; exec sleep 60; fi\n\
    printf %s \"$LOOP_MANAGER_STEP_DRAIN_DATA\"\n";

/// Waits until `path` exists: the step that creates it is running.
async fn until_file(path: &Path) {
    let deadline = Instant::now() + WITHIN;
    while !path.exists() {
        assert!(Instant::now() < deadline, "no {} in time", path.display());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The bead `id` as br shows it.
fn show(root: &Path, id: &str) -> Value {
    let shown: Value = serde_json::from_str(&br(root, &["show", id, "--json"])).unwrap();
    shown[0].clone()
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let finish = format!("{} finish pass --summary signed", env.loop_manager.display());
    // Each session of A and B calls the finisher in its turn.
    let turn = Reply::Turn(vec![Reply::Bash(finish), Reply::Text("Done.".into())]);
    let provider = Provider::start(vec![turn]).await;
    let mut d = Daemon::start(&env, |root| {
        claude_agent(root, "dev", &provider.base_url());
        claude_agent(root, "ops", &provider.base_url());
        let shop = root.join("config/loop-manager/shop");
        write(
            shop.join("loop-manager.toml"),
            "[rigs.web]\npath = \"web\"\n[rigs.api]\npath = \"api\"\n",
        );
        write(shop.join("api/.keep"), "");
        let dir = root.display().to_string();
        for (path, text) in [
            ("config/loop-manager/shop/formulas/ship.toml", SHIP),
            ("config/loop-manager/shop/formulas/work.toml", WORK),
            ("plan.sh", PLAN),
            ("work.sh", WORK_SH),
            ("report.sh", REPORT),
        ] {
            write(root.join(path), text.replace("{dir}", &dir));
        }
        tracker(root);
    })
    .await;
    let root_dir = d.dir.path().to_owned();
    let at = |name: &str| -> PathBuf { root_dir.join(name) };
    let body = json!({ "project": "shop", "rig": "web", "formula": "ship",
        "inputs": { "task": "Ship it" } });
    let reply = d.post("/v1/runs", body).await.unwrap();
    let ship = reply["run"].as_str().expect("a run").to_owned();
    for held in ["plan.held", "a.held", "report.held"] {
        until_file(&at(held)).await;
        if held == "a.held" {
            until_failed(&d, &ship, &at("ids")).await;
        }
        d.restart().await;
    }
    let view = d.until_state(&ship, "passed", WITHIN).await;
    let journal = d
        .journal_until(WITHIN, |seen| {
            seen.iter().any(|(k, e)| k == "run.passed" && e["run"] == ship.as_str())
        })
        .await;
    let ids = std::fs::read_to_string(at("ids")).unwrap();
    let [a, b, f, h, s]: [&str; 5] = ids.split(' ').collect::<Vec<_>>().try_into().unwrap();
    let prefixes = [a, b, f, h].map(|id| id.split_once('-').map_or(id, |(p, _)| p));
    assert_eq!(prefixes, ["w", "a", "a", "w"], "{ids}");
    checked(&d, &ship, &view, &journal, [a, b, f, h, s]).await;
    format!(
        "ship: plan, drain, report, each through a kill -9; {a}, {b} closed, {f} under {s} failed once, {h} refused"
    )
}

/// Waits until the run on F, the third bead of `ids`, has failed: the restart in the drain step comes after it, so a
/// drain that gave F again in its next attempt would show a second run on F.
async fn until_failed(d: &Daemon, ship: &str, ids: &Path) {
    let ids = std::fs::read_to_string(ids).unwrap();
    let f = ids.split(' ').nth(2).expect("F");
    let deadline = Instant::now() + WITHIN;
    loop {
        let kids = d.get(&format!("/v1/runs?parent={ship}")).await;
        let kids = kids.as_array().expect("runs");
        if kids.iter().any(|k| k["bead"] == f && k["state"] == "failed") {
            return;
        }
        assert!(Instant::now() < deadline, "the run on {f} has not failed: {kids:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The checks of criterion 24 on the run `ship` that passed and the beads A, B, F, H and the epic S.
async fn checked(d: &Daemon, ship: &str, view: &Value, journal: &[(String, Value)], beads: [&str; 5]) {
    let [a, b, f, h, s] = beads;
    let root = d.dir.path();
    // Two epics: the root, from the task, and S.
    let listed: Value = serde_json::from_str(&br(root, &["list", "--all", "--json", "-t", "epic"])).unwrap();
    let epics = listed["issues"].as_array().expect("issues");
    let mut titles: Vec<&str> = epics.iter().map(|e| e["title"].as_str().unwrap()).collect();
    titles.sort_unstable();
    assert_eq!(titles, ["S", "Ship it"], "{listed}");
    let epic = epics.iter().find(|e| e["title"] == "Ship it").unwrap()["id"]
        .as_str()
        .unwrap();
    assert_eq!(view["bead"].as_str(), Some(epic), "{view}");
    // Each step finished once; the restart in the drain step ended its attempt, the next attempt finished it.
    let of = |run: &str, kind: &str| -> Vec<&Value> {
        let mine = journal.iter().filter(|(k, e)| k == kind && e["run"] == run);
        mine.map(|(_, e)| e).collect()
    };
    let finished: Vec<&str> = of(ship, "run.step_finished")
        .iter()
        .map(|e| e["data"]["run"]["step"].as_str().unwrap())
        .collect();
    assert_eq!(finished, ["plan", "drain"], "{finished:?}");
    // A kill in each step ends its attempt; the policy resumes the step in a new attempt.
    let crash = Some("daemon_crash");
    let steps = [
        ("plan", crash, "exec"),
        ("plan", None, "exec"),
        ("drain", crash, "drain"),
        ("drain", None, "drain"),
    ];
    let report = [("report", crash, "exec"), ("report", None, "exec")];
    assert_eq!(tried(view), [&steps[..], &report].concat(), "{view}");
    // Three children, one per bead but H and S; each in the root's correlation one level down.
    let kids = d.get(&format!("/v1/runs?parent={ship}")).await;
    let kids = kids.as_array().expect("runs");
    let kid = |bead: &str| -> Vec<&Value> { kids.iter().filter(|k| k["bead"] == bead).collect() };
    assert_eq!(kids.len(), 3, "{kids:?}");
    assert!(kid(h).is_empty(), "{h} refused, no run: {kids:?}");
    let correlation = &of(ship, "run.queued")[0]["correlation"];
    let mut runs = Vec::new();
    for (bead, rig, state) in [(a, "web", "passed"), (b, "api", "passed"), (f, "api", "failed")] {
        let [run] = kid(bead)[..] else {
            panic!("one run on {bead}: {kids:?}")
        };
        let id = run["id"].as_str().unwrap();
        let queued = of(id, "run.queued")[0];
        assert_eq!(
            (&queued["rig"], &queued["correlation"]),
            (&json!(rig), correlation),
            "{queued}"
        );
        assert_eq!(
            (run["depth"].as_i64(), run["state"].as_str()),
            (Some(1), Some(state)),
            "{run}"
        );
        runs.push(id.to_owned());
    }
    // A ran on the agent and the harness of its bead.
    let view_a = d.get(&format!("/v1/runs/{}", runs[0])).await;
    assert_eq!(view_a["agent"], "shop.dev", "{view_a}");
    let steps = [("do", crash, "exec"), ("do", None, "exec"), ("sign", None, "claude")];
    assert_eq!(tried(&view_a), steps, "{view_a}");
    // B started once A was closed; F failed as the agent's, once.
    let index = |run: &str, kind: &str| {
        let at = journal.iter().position(|(k, e)| k == kind && e["run"] == run);
        at.unwrap_or_else(|| panic!("no {kind} of {run}"))
    };
    assert!(
        index(&runs[0], "run.passed") < index(&runs[1], "run.started"),
        "B before A closed"
    );
    let failed = of(&runs[2], "run.failed");
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert_eq!(failed[0]["data"]["class"], "agent", "{failed:?}");
    // Each command ran to its end once.
    let log = std::fs::read_to_string(root.join("work.log")).unwrap();
    let mut log: Vec<&str> = log.lines().collect();
    log.sort_unstable();
    let mut want = vec![a, b, f];
    want.sort_unstable();
    assert_eq!(log, want, "the work log");
    // The last step saw what the drain gave.
    let passed = of(ship, "run.passed")[0];
    let stdout = passed["data"]["outcome"]["data"]["stdout"].as_str().expect("stdout");
    let data: Value = serde_json::from_str(stdout).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    assert_eq!(
        (&data["closed"], &data["failed"]),
        (&json!([a, b]), &json!([f])),
        "{data}"
    );
    let why = data["refused"][h].as_str().unwrap_or_default();
    assert!(why.contains("codex") && why.contains("claude"), "{data}");
    // br: A and B closed; F, H, S and the root open with no assignee.
    for (bead, status) in [
        (a, "closed"),
        (b, "closed"),
        (f, "open"),
        (h, "open"),
        (s, "open"),
        (epic, "open"),
    ] {
        until_shown(root, bead, status).await;
    }
}

/// `(step, failure kind, harness)` of each attempt in the view of a run.
fn tried(view: &Value) -> Vec<(&str, Option<&str>, &str)> {
    let attempts = view["attempts"].as_array().expect("attempts");
    attempts
        .iter()
        .map(|t| {
            (
                t["step"].as_str().unwrap_or_default(),
                t["failure_kind"].as_str(),
                t["harness"].as_str().unwrap_or_default(),
            )
        })
        .collect()
}

/// Waits until br shows the bead `id` with `status`, and with no assignee when open: br is written after the run's
/// transaction.
async fn until_shown(root: &Path, id: &str, status: &str) {
    let deadline = Instant::now() + WITHIN;
    loop {
        let shown = show(root, id);
        if shown["status"] == status && (status == "closed" || shown["assignee"].is_null()) {
            return;
        }
        assert!(Instant::now() < deadline, "{id} is not {status}: {shown}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
