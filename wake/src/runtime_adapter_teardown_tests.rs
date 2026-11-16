use super::*;
use std::os::windows::{
    io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::CommandExt,
};

fn fixture_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "catdesk-adapter-exit-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    root
}

// The helper cannot spawn its descendant until the parent has assigned its
// private Job and constructed the actual production Adapter. No browser,
// runtime profile, project registration or installed artifact is involved.
fn adapter_with_descendant(graceful: bool) -> (Adapter, OwnedHandle, PathBuf) {
    let root = fixture_root();
    let lock = browser_session_lock(&root, Duration::ZERO).unwrap();
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
$null = [Console]::In.ReadLine()
$start = [Diagnostics.ProcessStartInfo]::new()
$start.FileName = 'powershell.exe'
$start.Arguments = '-NoProfile -NonInteractive -Command "[Threading.Thread]::Sleep(30000)"'
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$descendant = [Diagnostics.Process]::Start($start)
[Console]::Out.WriteLine('{{"stage":"READY","descendantPid":' + $descendant.Id + '}}')
[Console]::Out.Flush()
{tail}
"#,
        tail = if graceful {
            ""
        } else {
            "$null = [Console]::In.ReadLine()"
        },
    );
    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .unwrap();
    let job = match crate::process_job::Job::assign(&child) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("private fixture Job assignment: {error}");
        }
    };
    let input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(output)
            .read_line(&mut line)
            .map_err(|_| "FIXTURE_READ_FAILED".to_string())
            .and_then(|_| serde_json::from_str(&line).map_err(|_| "FIXTURE_FRAME_INVALID".into()));
        let _ = tx.send(result);
    });
    let mut adapter = Adapter {
        job,
        child,
        input,
        output: rx,
        browser_session_lock: Some(lock),
        exit_proven: false,
    };
    adapter
        .send(serde_json::json!({"initialize":true}))
        .unwrap();
    let frame = adapter
        .output
        .recv_timeout(Duration::from_secs(10))
        .expect("bounded fixture initialization")
        .expect("fixture frame");
    reader.join().unwrap();
    let pid = u32::try_from(frame["descendantPid"].as_u64().unwrap()).unwrap();
    // Retain this exact new descendant's kernel handle before Adapter drop;
    // PID absence/reuse is not our terminal-state proof.
    let raw = unsafe { OpenProcess(0x0010_0000, 0, pid) };
    assert!(!raw.is_null(), "fixture descendant must be observable");
    let descendant = unsafe { OwnedHandle::from_raw_handle(raw) };
    assert_eq!(
        unsafe { WaitForSingleObject(descendant.as_raw_handle(), 0) },
        258
    );
    assert!(browser_session_lock(&root, Duration::ZERO).is_err());
    (adapter, descendant, root)
}

fn assert_retirement(graceful: bool) {
    let (mut adapter, descendant, root) = adapter_with_descendant(graceful);
    if graceful {
        assert!(adapter.allow_graceful_exit(Duration::from_secs(5)));
        assert!(adapter.job.active_processes().unwrap() >= 1);
    } else {
        assert!(adapter.job.active_processes().unwrap() >= 2);
    }
    let mut current = Some(adapter);
    let started = Instant::now();
    let mut live: Status = serde_json::from_value(serde_json::json!({
        "version":VERSION,"protocolVersion":1,"host":"TEST","pid":0,"updatedUtc":0,
        "targets":{},"queueDepth":0,"staleCount":0,"browser":"READY","login":"UNKNOWN",
        "submission":"IDLE","lastEventId":null,"lastAttemptUtc":null,"lastSuccessUtc":null,
        "lastReceipt":null,"attention":null
    }))
    .unwrap();
    assert_eq!(retire_adapter(&mut current, &mut live), Some(graceful));
    assert!(live.attention.is_none());
    assert!(current.is_none());
    assert!(started.elapsed() < Duration::from_secs(20));
    assert_eq!(
        unsafe { WaitForSingleObject(descendant.as_raw_handle(), 0) },
        0,
        "a newly admitted owner must never race a surviving owned descendant"
    );
    let next_owner = browser_session_lock(&root, Duration::ZERO).unwrap();
    drop(next_owner);
    drop(descendant);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn forced_retirement_proves_descendant_exit_before_next_browser_owner() {
    assert_retirement(false);
}

#[test]
fn clean_helper_exit_does_not_substitute_for_descendant_exit() {
    assert_retirement(true);
}

#[test]
fn unproven_exit_retains_real_browser_lock_until_owner_handle_closes() {
    let root = fixture_root();
    let mut lock = Some(browser_session_lock(&root, Duration::ZERO).unwrap());
    let raw = lock.as_ref().unwrap().as_raw_handle();
    retain_unproven_browser_lock(&mut lock);
    assert!(lock.is_none());
    // Reclaim only our intentionally retained disposable fixture handle so a
    // failing assertion cannot leak it in the test process. Production never
    // releases an unproved lock this way.
    let retained = unsafe { File::from_raw_handle(raw) };
    assert!(browser_session_lock(&root, Duration::ZERO).is_err());
    drop(retained);
    let next_owner = browser_session_lock(&root, Duration::ZERO).unwrap();
    drop(next_owner);
    std::fs::remove_dir_all(root).unwrap();
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
}
