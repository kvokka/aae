//! Criterion 20, without a model: demo 2's pack, word for word, on the real `codex` against fake Responses APIs, laid
//! out as the demo lays itself out. Round 1: the planner files one bead for the frontend, the coder writes a README
//! rumdl cannot fix, `format` fails, `qa` passes, and the run goes back to `plan`. Round 2: the planner sees rumdl's
//! findings in its prompt and files a fix, the drain runs only the new bead, `format` passes and so does `qa`, which
//! gets the project's conditions.

use crate::{
    Env,
    checks::root,
    daemon::{Daemon, tree, write},
};
use loop_manager_testkit::openai::{Provider, Reply};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command, sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(120);

/// The first planner files a README; the second, after `format` failed, files its fix, each a bead of `frontend` by
/// its prefix `f`. `{dir}` keeps the state.
const PLAN: &str = "set -e\nbead=$({loop_manager} --json run show \"$LOOP_MANAGER_RUN\" | jq -r .bead)\n\
    if [ -e {dir}/planned ]; then title='Fix the README'; else title='Write a README'; touch {dir}/planned; fi\n\
    BD_ISSUE_PREFIX=f br create \"$title\" --deps \"parent-child:$bead\" -l agent:coder --description \"$title.\" --silent \
    >> {dir}/beads\n{loop_manager} finish pass --summary \"$title\"\n";

/// In its rig, the first coder writes a reference without a definition, which rumdl cannot fix (MD052); the second
/// fixes it.
const CODE: &str = "set -e\n\
    if [ -e README.md ]; then printf '# Frontend\\n\\nOpen index.html.\\n' > README.md; \
    else printf '# Frontend\\n\\nSee [the docs][missing].\\n' > README.md; fi\n\
    {loop_manager} finish pass --summary README\n";

/// One command, then "Done.", for each of two rounds by the order of requests: the second round of the planner and
/// of qa resumes the thread of the first, so a reply by the number of tool outputs would not tell the rounds apart.
async fn fake(command: String) -> Provider {
    let turn = [Reply::Command(command), Reply::Text("Done.".into())];
    Provider::start(turn.iter().chain(&turn).cloned().collect()).await
}

/// The demo's directory without the author's run in `root/demo`: its pack, its project and its empty rigs, in a git
/// repository that ignores the rigs, with a br tracker in `root/`; each agent of the pack on its own fake.
fn demo(root: &Path, fakes: [(&str, &Provider); 3]) {
    let src = self::root().join("demos/2-multirepo-qa");
    let demo = root.join("demo");
    fs::create_dir_all(demo.join("root")).unwrap();
    let cp = Command::new("cp").arg("-R").arg(src.join("pack")).arg(&demo).status();
    assert!(cp.is_ok_and(|s| s.success()), "cp the pack");
    for file in [
        ".gitignore",
        "root/loop-manager.toml",
        "root/.loop-manager/agents/qa/checks.md",
    ] {
        fs::create_dir_all(demo.join(file).parent().unwrap()).unwrap();
        fs::copy(src.join(file), demo.join(file)).unwrap();
    }
    for rig in ["frontend", "backend"] {
        write(demo.join(rig).join(".gitkeep"), "");
    }
    for (agent, fake) in fakes {
        let path = demo.join(format!("pack/agents/{agent}/agent.toml"));
        // The host forbids user namespaces, so the codex sandbox cannot run here (docs/design/experiments.md).
        let native = format!(
            "\n[native.codex]\nmodel_provider = \"fake\"\nsandbox_mode = \"danger-full-access\"\n\
             [native.codex.model_providers.fake]\nname = \"fake\"\nbase_url = \"{}\"\n\
             wire_api = \"responses\"\nrequest_max_retries = 0\nstream_max_retries = 0\n",
            fake.base_url()
        );
        write(&path, fs::read_to_string(&path).unwrap() + &native);
    }
    let config = format!("projects = [\"{}\"]\n[harness.codex]\n", demo.join("root").display());
    write(root.join("config/loop-manager/config.toml"), config);
    for (dir, args) in [
        (demo.clone(), ["git", "init", "-q"]),
        (demo.join("root"), ["br", "init", "-q"]),
    ] {
        let status = Command::new(args[0]).args(&args[1..]).current_dir(dir).status();
        assert!(status.is_ok_and(|s| s.success()), "{args:?}");
    }
}

/// For each model request of `fake`, whether its body carries `text`.
fn carries(fake: &Provider, text: &str) -> Vec<bool> {
    fake.requests()
        .iter()
        .map(|r| r.body.to_string().contains(text))
        .collect()
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let loop_manager = env.loop_manager.display().to_string();
    let state = tree();
    let dir = state.path().display().to_string();
    for (name, text) in [("plan.sh", PLAN), ("code.sh", CODE)] {
        write(
            state.path().join(name),
            text.replace("{loop_manager}", &loop_manager).replace("{dir}", &dir),
        );
    }
    let planner = fake(format!("sh {dir}/plan.sh")).await;
    let coder = fake(format!("sh {dir}/code.sh")).await;
    let qa = fake(format!("{loop_manager} finish pass --summary checked")).await;
    let d = Daemon::start(&env, |root| {
        demo(root, [("planner", &planner), ("coder", &coder), ("qa", &qa)]);
    })
    .await;

    let body = json!({ "project": "multirepo-qa", "formula": "dev.deliver",
        "inputs": { "task": "A README for the frontend" } });
    let reply = d.post("/v1/runs", body).await.unwrap();
    let run = reply["run"].as_str().expect("a run").to_owned();
    let view = d.until_state(&run, "passed", WITHIN).await;
    assert_eq!(view["round"], 2, "{view}");

    let readme = fs::read_to_string(d.path("demo/frontend/README.md")).unwrap();
    assert_eq!(readme, "# Frontend\n\nOpen index.html.\n", "{view}");
    // The prompt of the first planner has empty remarks; the second one's carries rumdl's findings.
    let remarks = carries(&planner, "MD052");
    assert!(!remarks[0] && remarks.iter().any(|&r| r), "{remarks:?}");
    let conditions = "keeps high scores across a restart";
    assert!(carries(&qa, conditions).iter().any(|&c| c), "qa saw no conditions");

    let beads = fs::read_to_string(state.path().join("beads")).unwrap();
    let beads: Vec<&str> = beads.lines().collect();
    assert_eq!(beads.len(), 2, "{beads:?}");
    let out = d.cli(&["--json", "run", "list", "--project", "multirepo-qa"]);
    let runs: Value = serde_json::from_slice(&out.stdout).expect("run list");
    let on = |bead: &str| runs.as_array().unwrap().iter().filter(|r| r["bead"] == bead).count();
    assert_eq!((on(beads[0]), on(beads[1])), (1, 1), "{runs}");
    format!(
        "round 1: {} failed format with MD052, back to plan; round 2: the planner saw the remarks, the drain ran only \
         {}, format and qa passed",
        beads[0], beads[1]
    )
}
