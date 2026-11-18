//! Criterion 20, without a model: demo 1's pack, word for word, laid out as the demo lays itself out, on the real
//! `claude` and `codex` against fake APIs. The planner files a bead for each rig with the rig's prefix, the coder of
//! `frontend` runs on claude and that of `backend` on codex, and `report` finds every bead under the request closed.

use crate::{
    Env,
    checks::root,
    daemon::{Daemon, tree, write},
};
use loop_manager_testkit::{anthropic, openai, without_outer_session};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command, sync::Arc, time::Duration};

const WITHIN: Duration = Duration::from_secs(120);

/// A bead per rig under the request: `frontend` by its derived prefix `f`, `backend` by its `api`; `{dir}` keeps
/// their ids.
const PLAN: &str = "set -e\nbead=$({loop_manager} --json run show \"$LOOP_MANAGER_RUN\" | jq -r .bead)\n\
    new() { BD_ISSUE_PREFIX=$1 br create \"$2\" --deps \"parent-child:$bead\" -l \"agent:coder,harness:$3\" \
    --description \"$2.\" --silent >> {dir}/beads; }\n\
    new f 'Write the page' claude\nnew api 'Write the server' codex\n{loop_manager} finish pass --summary planned\n";

const CODE: &str = "set -e\necho done > DONE.md\n{loop_manager} finish pass --summary done\n";

/// The demo's directory without the author's run: its pack, its project and its empty rigs, in a git repository,
/// with a br tracker in `root/`. Each claude agent gets its fake by `[env]`; the coder's `[native.codex]` takes
/// effect only on `backend`, where the project runs it on codex.
fn demo(root: &Path, claude: [(&str, String); 2], codex: String) {
    let src = self::root().join("demos/1-planner-executor");
    let demo = root.join("demo");
    fs::create_dir_all(demo.join("root")).unwrap();
    let cp = Command::new("cp").arg("-R").arg(src.join("pack")).arg(&demo).status();
    assert!(cp.is_ok_and(|s| s.success()), "cp the pack");
    for file in [".gitignore", "root/loop-manager.toml"] {
        fs::copy(src.join(file), demo.join(file)).unwrap();
    }
    for rig in ["frontend", "backend"] {
        write(demo.join(rig).join(".gitkeep"), "");
    }
    for (agent, url) in claude {
        let path = demo.join(format!("pack/agents/{agent}/agent.toml"));
        let mut text = fs::read_to_string(&path).unwrap()
            + &format!(
                "\n[env]\nANTHROPIC_BASE_URL = \"{url}\"\nANTHROPIC_API_KEY = \"fake\"\n\
                 CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = \"1\"\nCLAUDE_CODE_MAX_RETRIES = \"0\"\n"
            );
        if agent == "coder" {
            // The host forbids user namespaces, so the codex sandbox cannot run here (docs/design/experiments.md).
            text += &format!(
                "[native.codex]\nmodel_provider = \"fake\"\nsandbox_mode = \"danger-full-access\"\n\
                 [native.codex.model_providers.fake]\nname = \"fake\"\nbase_url = \"{codex}\"\n\
                 wire_api = \"responses\"\nrequest_max_retries = 0\nstream_max_retries = 0\n"
            );
        }
        write(&path, text);
    }
    let config = format!(
        "projects = [\"{}\"]\n[harness.claude]\n[harness.codex]\n",
        demo.join("root").display()
    );
    write(root.join("config/loop-manager/config.toml"), config);
    for (dir, args) in [
        (demo.clone(), ["git", "init", "-q"]),
        (demo.join("root"), ["br", "init", "-q"]),
    ] {
        let status = without_outer_session(&mut Command::new(args[0]))
            .args(&args[1..])
            .current_dir(dir)
            .status();
        assert!(status.is_ok_and(|s| s.success()), "{args:?}");
    }
}

pub(crate) async fn run(env: Arc<Env>) -> String {
    let loop_manager = env.loop_manager.display().to_string();
    let state = tree();
    let dir = state.path().display().to_string();
    for (name, text) in [("plan.sh", PLAN), ("code.sh", CODE)] {
        let text = text.replace("{loop_manager}", &loop_manager).replace("{dir}", &dir);
        write(state.path().join(name), text);
    }
    let claude = |cmd: String| {
        use anthropic::Reply;
        anthropic::Provider::start(vec![Reply::Turn(vec![Reply::Bash(cmd), Reply::Text("Done.".into())])])
    };
    let planner = claude(format!("sh {dir}/plan.sh")).await;
    let coder = claude(format!("sh {dir}/code.sh")).await;
    let codex = openai::Provider::start(vec![openai::Reply::Task(format!("sh {dir}/code.sh"))]).await;
    let d = Daemon::start(&env, |root| {
        let claude = [("planner", planner.base_url()), ("coder", coder.base_url())];
        demo(root, claude, codex.base_url());
    })
    .await;

    let body = json!({ "project": "planner-executor", "formula": "dev.deliver",
        "inputs": { "task": "A page and its server" } });
    let reply = d.post("/v1/runs", body).await.unwrap();
    let run = reply["run"].as_str().expect("a run").to_owned();
    let view = d.until_state(&run, "passed", WITHIN).await;

    let beads = fs::read_to_string(state.path().join("beads")).unwrap();
    let beads: Vec<&str> = beads.lines().collect();
    let [page, server] = beads[..] else { panic!("{beads:?}") };
    assert!(page.starts_with("f-") && server.starts_with("api-"), "{beads:?}");
    // The last step, `report`, shows the tree of the request with both beads closed.
    let tree = view["outcome"]["data"]["stdout"].as_str().unwrap_or_default();
    for (id, title) in [(page, "Write the page"), (server, "Write the server")] {
        assert!(tree.contains(&format!("{id}: {title} [P2] [closed]")), "{view}");
    }
    for rig in ["frontend", "backend"] {
        assert!(d.path(&format!("demo/{rig}/DONE.md")).exists(), "{rig}: {view}");
    }
    assert!(
        !coder.requests().is_empty() && !codex.requests().is_empty(),
        "a coder off its harness"
    );
    let out = d.cli(&["--json", "run", "list", "--project", "planner-executor"]);
    let runs: Value = serde_json::from_slice(&out.stdout).expect("run list");
    let on = |bead: &str| runs.as_array().unwrap().iter().filter(|r| r["bead"] == bead).count();
    assert_eq!((on(page), on(server)), (1, 1), "{runs}");
    format!("{page} on claude and {server} on codex, filed by the planner with the rig prefixes, closed; report passed")
}
