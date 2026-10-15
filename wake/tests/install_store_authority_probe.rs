#[cfg(windows)]
#[test]
fn installer_store_preflight_is_bounded_and_fail_closed() {
    use std::{
        os::windows::process::CommandExt,
        path::Path,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("install-store-authority-probe.ps1");
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .args(["-NativeHost", env!("CARGO_BIN_EXE_CatDeskWakeHost")]);
    #[cfg(feature = "test-support")]
    command.arg("-EnableOwnerProofHost");
    let mut child = command
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch isolated Windows PowerShell preflight test");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if child
            .try_wait()
            .expect("poll isolated preflight test")
            .is_some()
        {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("isolated preflight test exceeded 60 seconds");
        }
        thread::sleep(Duration::from_millis(50));
    }
    let output = child
        .wait_with_output()
        .expect("collect isolated preflight test output");
    assert!(
        output.status.success(),
        "isolated preflight test failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("PASS: root and lock object match remains read-only"),
        "isolated preflight test omitted fail-closed evidence"
    );
}
