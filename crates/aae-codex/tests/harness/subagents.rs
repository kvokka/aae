//! The subagent targets of a session as agent roles of its thread, on the real codex and the fake Responses API
//! (docs/design/contracts.md 4.6): a spawn of a target's type runs a child with the role's instructions, model and
//! skills.

use crate::common::{Live, sourced};
use aae_contract::config::Skill;
use aae_testkit::openai::{Provider, Reply};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

/// A skill directory in `dir` whose description carries `marker`.
fn skill(dir: &std::path::Path, name: &str, marker: &str) -> Skill {
    let path = dir.join("skills").join(name);
    std::fs::create_dir_all(&path).unwrap();
    let text = format!("---\nname: {name}\ndescription: Does {marker} work.\n---\nDo it.\n");
    std::fs::write(path.join("SKILL.md"), text).unwrap();
    Skill {
        enabled: true,
        path: Some(path),
    }
}

/// The last developer message of `request` that lists the skills, as text.
fn skills_message(request: &Value) -> String {
    let input = request["input"].as_array().into_iter().flatten().rev();
    let developer = input.filter(|i| i["role"] == "developer").map(Value::to_string);
    developer
        .into_iter()
        .find(|t| t.contains("skills_instructions"))
        .unwrap_or_default()
}

async fn the_role_reaches_the_child(v2: bool) {
    let dir = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent_type": "kit.qa", "message": "Check it.", "task_name": "qa" });
    let plan = vec![Reply::Turn(vec![
        Reply::Call("spawn_agent".into(), spawn),
        Reply::Text("DONE".into()),
    ])];
    let provider = Arc::new(Provider::start(plan).await);
    let (own, qa) = (skill(dir.path(), "own", "zq-own"), skill(dir.path(), "qa", "zq-qa"));
    let live = Live::open(provider, dir.path(), 0, "never", None, |c| {
        c.skills.insert("own".into(), sourced(own));
        if v2 {
            let native = &mut c.native.as_mut().unwrap().value;
            native["features"] = json!({ "multi_agent_v2": true });
        }
        let mut target = c.clone();
        target.description = Some(sourced("Checks the zq-desc work.".into()));
        target.system_prompt = Some(sourced("ROLE-QA-zz".into()));
        target.model = Some(sourced("gpt-role".into()));
        target.skills = [("qa".to_owned(), sourced(qa))].into();
        c.targets.insert("kit.qa".into(), target);
    })
    .await
    .unwrap();
    // The child's events come on the session's channel too: the child's request is what tells it ran.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let child = loop {
        let requests = live.provider.requests();
        if let Some(r) = requests.iter().find(|r| r.body["model"] == "gpt-role") {
            break r.body.clone();
        }
        assert!(tokio::time::Instant::now() < deadline, "no child: {requests:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let parent = live.provider.requests()[0].body.to_string();
    for seen in ["kit.qa", "zq-desc", "zq-own", "zq-qa"] {
        assert!(parent.contains(seen), "the session sees {seen}");
    }
    assert!(child.to_string().contains("ROLE-QA-zz"), "{child}");
    let skills = skills_message(&child);
    assert!(skills.contains("zq-qa") && !skills.contains("zq-own"), "{skills}");
    live.stop().await;
}

#[tokio::test]
async fn a_spawn_of_a_target_runs_its_role_in_multi_agent_v1() {
    the_role_reaches_the_child(false).await;
}

#[tokio::test]
async fn a_spawn_of_a_target_runs_its_role_in_multi_agent_v2() {
    the_role_reaches_the_child(true).await;
}

/// A resume writes the role files its targets name: a target changed since the start has a role file the start did
/// not write, and its spawn runs with the new instructions.
#[tokio::test]
async fn a_resume_writes_the_role_files_of_its_targets() {
    let dir = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent_type": "kit.qa", "message": "Check it.", "task_name": "qa" });
    let plan = vec![
        Reply::Text("STARTED".into()),
        Reply::Turn(vec![
            Reply::Call("spawn_agent".into(), spawn),
            Reply::Text("DONE".into()),
        ]),
    ];
    let provider = Arc::new(Provider::start(plan).await);
    let role = |prompt: &'static str| {
        move |c: &mut aae_contract::config::EffectiveConfig| {
            let mut target = c.clone();
            target.description = Some(sourced("Checks.".into()));
            target.system_prompt = Some(sourced(prompt.into()));
            target.model = Some(sourced("gpt-role".into()));
            c.targets.insert("kit.qa".into(), target);
        }
    };
    let mut first = Live::open(provider.clone(), dir.path(), 0, "never", None, role("ROLE-OLD-zz"))
        .await
        .unwrap();
    first.ended().await;
    let session = first.session.clone();
    first.stop().await;
    let live = Live::open(provider, dir.path(), 0, "never", Some(session), role("ROLE-NEW-zz"))
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let child = loop {
        let requests = live.provider.requests();
        if let Some(r) = requests.iter().find(|r| r.body["model"] == "gpt-role") {
            break r.body.to_string();
        }
        assert!(tokio::time::Instant::now() < deadline, "no child: {requests:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(
        child.contains("ROLE-NEW-zz") && !child.contains("ROLE-OLD-zz"),
        "{child}"
    );
    live.stop().await;
}
