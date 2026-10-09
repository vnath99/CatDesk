#![cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    io::Write,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

#[test]
fn fixture() {
    let Ok(path) = std::env::var("WAKE_JOB_FIXTURE") else {
        return;
    };
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "descendant", "--nocapture"])
        .env("WAKE_JOB_DESCENDANT", "1")
        .creation_flags(0x08000000)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    std::fs::write(path, child.id().to_string()).unwrap();
    thread::sleep(Duration::from_secs(30));
    let _ = child.wait();
}
#[test]
fn descendant() {
    if std::env::var_os("WAKE_JOB_DESCENDANT").is_some() {
        thread::sleep(Duration::from_secs(30));
    }
}

#[test]
fn job_closure_terminates_entire_owned_process_tree() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, id: u32) -> *mut std::ffi::c_void;
        fn WaitForSingleObject(handle: *mut std::ffi::c_void, ms: u32) -> u32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    let path = std::env::temp_dir().join(format!("wake-job-test-{}.txt", std::process::id()));
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fixture", "--nocapture"])
        .env("WAKE_JOB_FIXTURE", &path)
        .creation_flags(0x08000000)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let job = catdesk_wake::process_job::Job::assign(&child).unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"contained\n")
        .unwrap();
    for _ in 0..100 {
        if path.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let grandchild: u32 = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let handle = unsafe { OpenProcess(0x00100000, 0, grandchild) };
    assert!(!handle.is_null());
    drop(job);
    assert_eq!(unsafe { WaitForSingleObject(handle, 5000) }, 0);
    unsafe {
        CloseHandle(handle);
    }
    let stopped = std::time::Instant::now();
    child.wait().unwrap();
    assert!(stopped.elapsed() < Duration::from_secs(2));
    std::fs::remove_file(path).unwrap();
}
