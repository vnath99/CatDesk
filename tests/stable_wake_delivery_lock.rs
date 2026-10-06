use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

fn fixture(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("catdesk-delivery-lock-{name}-{nonce}"));
    fs::create_dir_all(root.join(".catdesk/wake-bridge")).expect("fixture root");
    fs::create_dir_all(root.join(".catdesk/autonomy")).expect("fixture inbox");
    fs::write(
        root.join(".catdesk/wake-bridge/config.json"),
        r#"{"conversation_url":"https://chatgpt.com/c/exact-thread","profile_dir":".catdesk/wake-bridge/browser-profile"}"#,
    )
    .expect("config");
    fs::write(
        root.join(".catdesk/autonomy/review-inbox.json"),
        r#"[{"schemaVersion":1,"recordId":"review_1","projectId":"catdesk","sessionId":"session_1","state":"COMPLETED_VERIFIED","nextAction":"independent_final_review","reference":"artifacts/completion.json","createdAtUnix":1,"unread":true}]"#,
    )
    .expect("inbox");
    root
}

fn probe(workspace: &Path, hold_ms: u64) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_catdesk-stable-wake-lock-probe"));
    command
        .arg("--workspace")
        .arg(workspace.to_str().expect("workspace utf8"))
        .arg("--hold-ms")
        .arg(hold_ms.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

fn spawn_ready(workspace: &Path) -> std::process::Child {
    let mut child = probe(workspace, 60_000).spawn().expect("spawn lock owner");
    let stdout = child.stdout.take().expect("stdout");
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .expect("ready line");
    assert_eq!(line.trim(), "READY");
    child
}

#[test]
fn hard_killed_owner_releases_kernel_lock_without_manual_cleanup() {
    let root = fixture("hard-kill");
    let stale_lock = root.join(".catdesk/wake-bridge/state.lock");
    fs::write(&stale_lock, b"stale-marker").expect("stale lock artifact");
    let mut owner = spawn_ready(&root);
    owner.kill().expect("hard kill");
    owner.wait().expect("reap killed owner");

    let successor = probe(&root, 1).output().expect("successor");
    assert!(successor.status.success(), "successor: {successor:?}");
    assert_eq!(
        fs::read(stale_lock).expect("stale artifact remains inert"),
        b"stale-marker"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn two_processes_have_one_kernel_lock_owner() {
    let root = fixture("contention");
    let mut owner = spawn_ready(&root);
    let contender = probe(&root, 1).output().expect("contender");
    assert_eq!(contender.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&contender.stdout).trim(), "BUSY");
    owner.kill().expect("hard kill");
    owner.wait().expect("reap owner");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn two_process_claims_create_exactly_one_durable_transition() {
    let root = fixture("claim-contention");
    let first = Command::new(env!("CARGO_BIN_EXE_catdesk-stable-wake-lock-probe"))
        .args([
            "--workspace",
            root.to_str().expect("workspace utf8"),
            "--claim",
            "review_1",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("first claim");
    let second = Command::new(env!("CARGO_BIN_EXE_catdesk-stable-wake-lock-probe"))
        .args([
            "--workspace",
            root.to_str().expect("workspace utf8"),
            "--claim",
            "review_1",
        ])
        .output()
        .expect("second claim");
    let first = first.wait_with_output().expect("first result");
    let outcomes = [
        String::from_utf8_lossy(&first.stdout).trim().to_owned(),
        String::from_utf8_lossy(&second.stdout).trim().to_owned(),
    ];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| *outcome == "TRANSITIONED")
            .count(),
        1,
        "outcomes: {outcomes:?}"
    );
    assert!(
        outcomes.iter().all(|outcome| {
            matches!(outcome.as_str(), "TRANSITIONED" | "NO_TRANSITION" | "BUSY")
        }),
        "outcomes: {outcomes:?}"
    );
    fs::remove_dir_all(root).expect("cleanup");
}
