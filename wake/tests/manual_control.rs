use catdesk_wake::{runtime, store::Store};
use serde_json::Value;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[test]
#[ignore = "manual startup diagnostic"]
fn diagnose_registered_wakehost_startup() {
    let root = runtime::default_root().expect("wake root");
    let store = Store::open(&root).expect("wake store");
    runtime::control(&store, "RUNNING").expect("desired running");

    let pointer: Value =
        serde_json::from_slice(&std::fs::read(root.join("current.json")).expect("current"))
            .expect("current json");
    let directory = pointer
        .get("directory")
        .and_then(Value::as_str)
        .expect("current directory");
    let executable = root
        .join("versions")
        .join(directory)
        .join("CatDeskWakeHost.exe");
    assert!(executable.is_file(), "registered host missing");

    let mut child = Command::new(&executable)
        .arg("--host")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn registered host");

    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(exit) = child.try_wait().expect("try_wait") {
            let output = child.wait_with_output().expect("child output");
            println!("CHILD_EXIT={exit}");
            println!("CHILD_STDOUT={}", String::from_utf8_lossy(&output.stdout));
            println!("CHILD_STDERR={}", String::from_utf8_lossy(&output.stderr));
            panic!("registered WakeHost exited before RUNNING");
        }
        let status = runtime::status(&store).expect("status");
        if status.host == "RUNNING" && status.pid == child.id() {
            println!(
                "START_OK version={} pid={} queue={}",
                status.version, status.pid, status.queue_depth
            );
            runtime::control(&store, "STOPPED").expect("stop");
            let _ = child.wait();
            return;
        }
        if Instant::now() >= deadline {
            runtime::control(&store, "STOPPED").expect("stop after timeout");
            let _ = child.kill();
            let output = child.wait_with_output().expect("timeout output");
            println!("CHILD_STDOUT={}", String::from_utf8_lossy(&output.stdout));
            println!("CHILD_STDERR={}", String::from_utf8_lossy(&output.stderr));
            panic!("registered WakeHost remained alive without publishing RUNNING");
        }
        thread::sleep(Duration::from_millis(100));
    }
}
