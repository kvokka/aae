use super::*;

fn env() -> Arc<Env> {
    let at = PathBuf::new();
    let text = String::new;
    let (date, version, host) = (text(), text(), text());
    Arc::new(Env {
        loop_manager: at.clone(),
        date,
        version,
        host,
        target: at,
    })
}

fn passing(_: Arc<Env>) -> Notes {
    Box::pin(async { daemon::tree().path().display().to_string() })
}

fn failing(_: Arc<Env>) -> Notes {
    Box::pin(async {
        let tree = daemon::tree();
        fs::write(tree.path().join("daemon.log"), "why").unwrap();
        panic!("no run passed")
    })
}

#[tokio::test]
async fn a_failed_scenario_keeps_its_tree_and_names_it() {
    let (Err(why), _) = execute(env(), failing).await else {
        panic!("the scenario passed")
    };
    let (head, tree) = why.split_once("; daemon.log and journal kept in ").unwrap();
    assert_eq!(head, "no run passed");
    assert_eq!(fs::read_to_string(Path::new(tree).join("daemon.log")).unwrap(), "why");
    fs::remove_dir_all(tree).unwrap();
    let (Ok(tree), _) = execute(env(), passing).await else {
        panic!("the scenario failed")
    };
    assert!(!Path::new(&tree).exists(), "a passed scenario leaves {tree}");
}
