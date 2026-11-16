use super::*;
use crate::driver::Scope;
use std::os::unix::process::ExitStatusExt;

#[tokio::test]
async fn a_panic_of_the_reader_fails_every_session_of_the_instance_and_kills_it() {
    let mut command = Command::new("sleep");
    command.arg("30");
    let (mut child, pid, start_time) = spawn(&mut command, Path::new("sleep")).unwrap();
    let instance = InstanceHandle {
        id: "i1".into(),
        pid,
        start_time,
        pgid: pid,
    };
    let (sink, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let forgot = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = forgot.clone();
    let reader = tokio::spawn(async { panic!("a line it could not take") });
    watch(reader, Events::new("i1", sink), instance, move || {
        flag.store(true, Ordering::Relaxed)
    });
    let first = rx.recv().await.unwrap();
    let Normalized::Failure(failure) = &first.event else {
        panic!("{first:?}")
    };
    assert_eq!(
        (failure.kind, failure.scope),
        (FailureKind::AdapterCrash, Scope::Instance)
    );
    assert!(
        failure.raw.ends_with("panicked: a line it could not take"),
        "{}",
        failure.raw
    );
    assert_eq!(
        (first.session, first.seq),
        (None, 1),
        "instance-wide: every session of it"
    );
    let second = rx.recv().await.unwrap();
    assert!(matches!(second.event, Normalized::Exited { .. }) && second.seq == 2);
    assert!(forgot.load(Ordering::Relaxed));
    let status = child.wait().await.unwrap();
    orphans::waited(child.id());
    assert_eq!(status.signal(), Some(Signal::KILL.as_raw()));
}
