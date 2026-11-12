//! Stand-in for the finisher CLI: reports what it sees to the daemon socket named in `AAE_SOCKET`.

use std::io::Write;

fn main() {
    let var = |k| std::env::var(k).unwrap_or_default();
    let mut s = std::os::unix::net::UnixStream::connect(var("AAE_SOCKET")).expect("connect to the daemon socket");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let line = format!(
        "attempt={:?} proc={:?} args={args:?}",
        var("AAE_ATTEMPT_TOKEN"),
        var("AAE_PROC_TOKEN")
    );
    s.write_all(line.as_bytes()).expect("write");
}
