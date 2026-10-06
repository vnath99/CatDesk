//! Disposable process fixture for the PowerShell stale-daemon recovery test.
//!
//! This is not linked into the CatDesk application binary. The test copies this
//! deliberately tiny executable into a temporary `target/release/catdesk.exe`
//! path so the production recovery code can verify real Windows process
//! path/hash/command-line evidence without touching the active daemon.

use std::{env, fs, net::TcpListener, path::PathBuf, thread, time::Duration};

fn required_env(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("missing required test environment variable {name}"))
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 1 || args[0] != "--catdesk-daemon" {
        panic!("catdesk recovery fixture accepts only --catdesk-daemon");
    }
    let port = required_env("CATDESK_RECOVERY_FIXTURE_PORT")
        .parse::<u16>()
        .expect("fixture port must be a u16");
    let marker = PathBuf::from(required_env("CATDESK_RECOVERY_FIXTURE_MARKER"));

    // The first process deliberately has no listener but remains alive. It
    // consumes the marker atomically enough for this single-process fixture;
    // the recovery-started replacement sees the marker absent and binds.
    if marker.exists() {
        fs::remove_file(&marker).expect("remove one-shot stale marker");
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    }

    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind fixture loopback listener");
    listener
        .set_nonblocking(true)
        .expect("set fixture listener nonblocking");
    loop {
        match listener.accept() {
            Ok((_stream, _peer)) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => panic!("fixture listener failed: {error}"),
        }
    }
}
