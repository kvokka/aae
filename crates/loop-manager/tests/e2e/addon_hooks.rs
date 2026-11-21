//! The environment of an addon's orchestrator hooks on a real daemon (docs/design/contracts.md 3.1): a hook finds its
//! addon's data directory, the one `${LOOP_MANAGER_ADDON_DATA}` names in the addon's config.

use crate::common::{Daemon, WAIT};
use std::{
    fs,
    time::{Duration, Instant},
};

#[tokio::test]
async fn an_orchestrator_hook_gets_its_addon_data_directory() {
    let d = Daemon::start(|dir| {
        let shop = dir.join("config/loop-manager/shop");
        let files = [
            (
                shop.join("loop-manager.toml"),
                "[[addon]]\npath = \"notes\"\n[settings]\naddons = [{ name = \"notes\" }]\n[rigs.web]\npath = \"web\"\n",
            ),
            (
                shop.join("notes/addon.toml"),
                "name = \"notes\"\nversion = \"0.1.0\"\n[[hook]]\non = \"run.passed\"\ncommand = [\"sh\", \"seen.sh\"]\n",
            ),
            (
                shop.join("notes/seen.sh"),
                "printf %s \"$LOOP_MANAGER_ADDON_DATA\" >\"$LOOP_MANAGER_ADDON_DIR/seen\"\n",
            ),
        ];
        fs::create_dir(shop.join("notes")).unwrap();
        for (path, text) in files {
            fs::write(path, text).unwrap();
        }
    })
    .await;
    let run = d.create("check", None).await;
    d.until_state(&run, "passed").await;
    let seen = d.dir.path().join("config/loop-manager/shop/notes/seen");
    let deadline = Instant::now() + WAIT;
    while !seen.exists() {
        assert!(Instant::now() < deadline, "the hook did not run");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let want = d.dir.path().join("state/loop-manager/addons/notes");
    assert_eq!(fs::read_to_string(&seen).unwrap(), want.to_string_lossy());
}
