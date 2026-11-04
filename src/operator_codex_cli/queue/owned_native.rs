//! Bounded transport for the existing native invocation, not a new worker policy.
//! The private Job is never disarmed. Cancellation requests cleanup even when
//! the async caller disappears; the durable claim/marker then remain unresolved.

use super::*;
use crate::reviewed_candidate_owner::{OwnedCandidateChild, SpawnFailure};
use std::process::Command as StdCommand;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

#[derive(Debug)]
pub(super) struct Transport {
    pub outcome: &'static str,
    pub exit_code: Option<i32>,
    pub child_exit_observed: bool,
    pub tree_exit_proven: bool,
    pub no_child_created: bool,
    pub exit_observed_at: Option<SystemTime>,
    pub stdout: Option<CodexCliNativeStdoutEvidenceV1>,
}

impl Transport {
    fn unknown(outcome: &'static str) -> Self {
        Self {
            outcome,
            exit_code: None,
            child_exit_observed: false,
            tree_exit_proven: false,
            no_child_created: false,
            exit_observed_at: None,
            stdout: None,
        }
    }
}

struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(super) async fn run(
    command: StdCommand,
    prompt: String,
    evidence: CodexCliNativeStdoutEvidenceV1,
    turn_limit: Duration,
) -> Transport {
    let cancel = Arc::new(AtomicBool::new(false));
    let _cancel_on_drop = CancelOnDrop(cancel.clone());
    tokio::task::spawn_blocking(move || {
        run_blocking(
            command,
            prompt,
            evidence,
            turn_limit,
            PROMPT_TIMEOUT,
            cancel,
        )
    })
    .await
    .unwrap_or_else(|_| Transport::unknown("NATIVE_OWNER_TASK_UNOBSERVED"))
}

struct PipeWorker<T> {
    receive: mpsc::Receiver<T>,
    thread: std::thread::JoinHandle<()>,
}
impl<T: Send + 'static> PipeWorker<T> {
    fn start(work: impl FnOnce() -> T + Send + 'static) -> std::io::Result<Self> {
        let (send, receive) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("catdesk-native-pipe".into())
            .spawn(move || {
                let _ = send.send(work());
            })?;
        Ok(Self { receive, thread })
    }
    fn finish(self) -> Option<T> {
        let value = self.receive.recv_timeout(NATIVE_STDOUT_EOF_TIMEOUT).ok()?;
        self.thread.join().ok()?;
        Some(value)
    }
}

fn drain(
    mut pipe: impl Read,
    mut evidence: CodexCliNativeStdoutEvidenceV1,
) -> (CodexCliNativeStdoutEvidenceV1, bool) {
    let mut buffer = [0; 8192];
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let count = match pipe.read(&mut buffer) {
            Ok(n) => n,
            Err(_) => {
                evidence.invalidate();
                return (evidence, false);
            }
        };
        if count == 0 {
            if !line.is_empty() || oversized {
                evidence.invalidate();
            }
            evidence.mark_eof();
            return (evidence, true);
        }
        for byte in &buffer[..count] {
            if *byte == b'\n' {
                if oversized {
                    evidence.invalidate();
                } else {
                    evidence.observe_line(&line);
                }
                line.clear();
                oversized = false;
            } else if !oversized {
                if line.len() < MAX_NATIVE_STDOUT_LINE_BYTES {
                    line.push(*byte);
                } else {
                    evidence.invalidate();
                    line.clear();
                    oversized = true;
                }
            }
        }
    }
}

fn run_blocking(
    mut command: StdCommand,
    prompt: String,
    evidence: CodexCliNativeStdoutEvidenceV1,
    turn_limit: Duration,
    prompt_limit: Duration,
    cancel: Arc<AtomicBool>,
) -> Transport {
    if cancel.load(Ordering::Acquire) {
        return Transport {
            no_child_created: true,
            ..Transport::unknown("NATIVE_CANCELLED_BEFORE_SPAWN")
        };
    }
    // Same prepared args, environment and directory. Suspended assignment is
    // performed by the existing owner before any CLI code executes.
    let mut owner = match OwnedCandidateChild::spawn_classified(&mut command) {
        Ok(owner) => owner,
        Err(SpawnFailure::NoChildCreated) => {
            return Transport {
                no_child_created: true,
                ..Transport::unknown("NATIVE_SPAWN_FAILED")
            };
        }
        Err(failure) => {
            return Transport {
                tree_exit_proven: failure.quiescence_proven(),
                ..Transport::unknown("NATIVE_OWNER_SETUP_FAILED")
            };
        }
    };
    let mut result = Transport::unknown("NATIVE_PIPE_UNAVAILABLE");
    let mut output = None;
    let mut input = None;
    if let (Some(stdout), Some(mut stdin)) = (
        owner.child_mut().stdout.take(),
        owner.child_mut().stdin.take(),
    ) {
        output = PipeWorker::start(move || drain(stdout, evidence)).ok();
        if output.is_some() {
            input = PipeWorker::start(move || {
                let accepted = stdin
                    .write_all(prompt.as_bytes())
                    .and_then(|_| stdin.flush())
                    .is_ok();
                drop(stdin); // Native exec - must see EOF, including on Windows.
                accepted
            })
            .ok();
        }
    }
    if let Some(writer) = input.as_ref() {
        let deadline = Instant::now() + prompt_limit;
        let accepted = loop {
            if cancel.load(Ordering::Acquire) {
                result.outcome = "NATIVE_CANCELLED";
                break false;
            }
            match writer.receive.try_recv() {
                Ok(value) => break value,
                Err(mpsc::TryRecvError::Disconnected) => break false,
                Err(mpsc::TryRecvError::Empty) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(_) => break false,
            }
        };
        if accepted {
            let deadline = Instant::now() + turn_limit;
            loop {
                if cancel.load(Ordering::Acquire) {
                    result.outcome = "NATIVE_CANCELLED";
                    break;
                }
                match owner.child_mut().try_wait() {
                    Ok(Some(status)) => {
                        result.child_exit_observed = true;
                        result.exit_code = status.code();
                        result.exit_observed_at = Some(SystemTime::now());
                        result.outcome = if status.success() {
                            "NATIVE_EXIT_ZERO"
                        } else {
                            "NATIVE_EXIT_NONZERO"
                        };
                        break;
                    }
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Ok(None) => {
                        result.outcome = "NATIVE_TIMEOUT";
                        break;
                    }
                    Err(_) => {
                        result.outcome = "NATIVE_EXIT_UNOBSERVED";
                        break;
                    }
                }
            }
        } else if result.outcome != "NATIVE_CANCELLED" {
            result.outcome = "NATIVE_PROMPT_PIPE_FAILED";
        }
    }
    // Parent exit alone is never cleanup proof. Retain this exact Job handle
    // through complete descendant termination and zero-active-process readback.
    result.tree_exit_proven = owner.terminate_tree_and_confirm_quiescent().is_ok();
    if !result.tree_exit_proven {
        result.outcome = "NATIVE_TREE_EXIT_UNPROVEN";
    }
    if let Some(writer) = input {
        // Its single result may already have been consumed by the prompt poll.
        // After Job cleanup, require actual thread exit rather than detaching it.
        let deadline = Instant::now() + NATIVE_STDOUT_EOF_TIMEOUT;
        while !writer.thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if writer.thread.is_finished() {
            if writer.thread.join().is_err() {
                result.outcome = "NATIVE_PIPE_THREAD_UNOBSERVED";
            }
        } else {
            result.outcome = "NATIVE_PIPE_THREAD_UNOBSERVED";
        }
    }
    if let Some(reader) = output {
        if let Some((evidence, true)) = reader.finish() {
            result.stdout = Some(evidence);
        } else {
            result.outcome = "NATIVE_STDOUT_EOF_UNOBSERVED";
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, timeout: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    fn fixture(root: &Path) -> PathBuf {
        let source = root.join("pipe-fixture.rs");
        let exe = root.join("pipe-fixture.exe");
        fs::write(
            &source,
            r#"
            use std::{io::{Read, Write}, process::{Command, Stdio}, time::Duration};
            fn main() {
                let a: Vec<String> = std::env::args().collect();
                if a[1] == "sleep" { std::thread::sleep(Duration::from_secs(20)); return; }
                if a[1] != "blocked" {
                    let mut prompt = String::new();
                    std::io::stdin().read_to_string(&mut prompt).unwrap();
                    assert_eq!(prompt, "exact prompt");
                }
                let _child = Command::new(std::env::current_exe().unwrap()).arg("sleep")
                    .stdin(Stdio::null()).stdout(Stdio::inherit()).spawn().unwrap();
                std::fs::write(&a[2], _child.id().to_string()).unwrap();
                // Let the observer retain a process handle before this parent exits.
                for _ in 0..500 {
                    if std::path::Path::new(&format!("{}.ack", a[2])).exists() { break; }
                    std::thread::sleep(Duration::from_millis(10));
                }
                if a[1] == "normal" {
                    println!("{{\"type\":\"thread.started\",\"thread_id\":\"{}\"}}", a[3]);
                    println!("{{\"type\":\"turn.started\"}}");
                    println!("{{\"type\":\"turn.completed\"}}");
                    std::io::stdout().flush().unwrap();
                } else if a[1] == "nonzero" { std::process::exit(7); }
                else { std::thread::sleep(Duration::from_secs(20)); }
            }
        "#,
        )
        .unwrap();
        assert!(
            StdCommand::new("rustc")
                .arg(&source)
                .arg("-o")
                .arg(&exe)
                .status()
                .unwrap()
                .success()
        );
        exe
    }

    fn command(exe: &Path, mode: &str, pid: &Path, thread: &str) -> StdCommand {
        let mut command = StdCommand::new(exe);
        command
            .args([mode, pid.to_str().unwrap(), thread])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        command
    }

    // Only read/synchronize rights, no PID-based cleanup. The transport owns
    // the only kill authority. This observer proves the retained descendant
    // was alive before the test and exited after the transport's Job cleanup.
    fn observe(pid: PathBuf, cancel: Option<Arc<AtomicBool>>) -> std::thread::JoinHandle<bool> {
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(8);
            let id = loop {
                if let Some(id) = fs::read_to_string(&pid)
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok())
                {
                    break id;
                }
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            let handle = unsafe { OpenProcess(0x0010_0000, 0, id) };
            if handle.is_null() {
                return false;
            }
            let alive = unsafe { WaitForSingleObject(handle, 0) } == 0x102;
            fs::write(format!("{}.ack", pid.display()), "ready").unwrap();
            if let Some(cancel) = cancel {
                cancel.store(true, Ordering::Release);
            }
            let exited = unsafe { WaitForSingleObject(handle, 8_000) } == 0;
            unsafe {
                CloseHandle(handle);
            }
            alive && exited
        })
    }

    #[test]
    fn native_transport_proves_descendant_exit_for_success_failure_timeout_and_blocked_prompt() {
        let root = super::super::tests::fixture_root();
        let exe = fixture(&root);
        for mode in ["normal", "nonzero", "timeout", "blocked", "cancel"] {
            let pid = root.join(format!("{mode}.pid"));
            let cancel = Arc::new(AtomicBool::new(false));
            let observer = observe(pid.clone(), (mode == "cancel").then(|| cancel.clone()));
            let thread = Uuid::new_v4().to_string();
            let prompt = if mode == "blocked" {
                "x".repeat(1024 * 1024)
            } else {
                "exact prompt".into()
            };
            let result = run_blocking(
                command(&exe, mode, &pid, &thread),
                prompt,
                CodexCliNativeStdoutEvidenceV1::new(&thread).unwrap(),
                Duration::from_millis(800),
                Duration::from_millis(800),
                cancel,
            );
            assert!(result.tree_exit_proven, "{mode}: {}", result.outcome);
            assert!(!result.no_child_created);
            assert!(
                observer.join().unwrap(),
                "{mode}: exact descendant handle exited"
            );
            assert_eq!(
                result.outcome,
                match mode {
                    "normal" => "NATIVE_EXIT_ZERO",
                    "nonzero" => "NATIVE_EXIT_NONZERO",
                    "timeout" => "NATIVE_TIMEOUT",
                    "blocked" => "NATIVE_PROMPT_PIPE_FAILED",
                    _ => "NATIVE_CANCELLED",
                }
            );
            if mode == "normal" {
                assert!(result.stdout.unwrap().completed_for(&thread));
            }
        }
        super::super::tests::cleanup(root);
    }

    #[tokio::test]
    async fn dropping_async_native_transport_requests_bounded_owned_cleanup() {
        let root = super::super::tests::fixture_root();
        let exe = fixture(&root);
        let pid = root.join("aborted.pid");
        let observer = observe(pid.clone(), None);
        let thread = Uuid::new_v4().to_string();
        let task = tokio::spawn(run(
            command(&exe, "timeout", &pid, &thread),
            "exact prompt".into(),
            CodexCliNativeStdoutEvidenceV1::new(&thread).unwrap(),
            Duration::from_secs(20),
        ));
        let deadline = Instant::now() + Duration::from_secs(8);
        while !PathBuf::from(format!("{}.ack", pid.display())).exists() && Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(
            tokio::task::spawn_blocking(move || observer.join().unwrap())
                .await
                .unwrap()
        );
        super::super::tests::cleanup(root);
    }
}
