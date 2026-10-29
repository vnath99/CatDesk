//! Finite foreground serving of the already trusted installed image.
//! No package selection, promotion, native-daemon startup or automatic retry.
use crate::reviewed_candidate_owner::OwnedCandidateChild;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const ROUTE: &str = "/catdesk-qualified-stopped-v2/mcp";
const LOCAL: &str = "http://127.0.0.1:3200/catdesk-qualified-stopped-v2/mcp";
const READ_TOOLS: [&str; 10] = [
    "catdesk_instruction",
    "catdesk_transport_status",
    "ollama_model_probe",
    "read",
    "search",
    "project_memory_read",
    "plan_read",
    "task_queue_read",
    "prompt_templates_list",
    "prompt_template_read",
];
const PROJECT_ROUTE: &str = "/project/mcp";
#[path = "operator_installed_project.rs"]
mod project;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectOptions {
    pub control: PathBuf,
    pub selection: crate::project_mcp::Selection,
    pub maximum_seconds: u64,
}

pub(super) fn parse_project_options(args: &[String]) -> Result<ProjectOptions, String> {
    let usage = "installed-backend run-project requires --control-workspace <existing-root> --project-id <id> --project-target-generation <n> --project-target-sha256 <digest> --max-runtime-seconds <1..180>";
    if args.len() != 13
        || args[0] != "operator"
        || args[1] != "installed-backend"
        || args[2] != "run-project"
        || args[3] != "--control-workspace"
        || args[5] != "--project-id"
        || args[7] != "--project-target-generation"
        || args[9] != "--project-target-sha256"
        || args[11] != "--max-runtime-seconds"
        || args[4].len() > 4096
        || !Path::new(&args[4]).is_absolute()
        || !catdesk_wake::protocol::valid_id(&args[6])
        || args[10].len() != 64
        || !args[10]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(usage.into());
    }
    let target_generation = args[8]
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or(usage)?;
    let maximum_seconds = args[12]
        .parse::<u64>()
        .ok()
        .filter(|value| (1..=180).contains(value))
        .ok_or(usage)?;
    Ok(ProjectOptions {
        control: PathBuf::from(&args[4]),
        selection: crate::project_mcp::Selection {
            project_id: args[6].clone(),
            target_generation,
            target_sha256: args[10].clone(),
        },
        maximum_seconds,
    })
}

pub(super) async fn run_project(
    workspace: &Path,
    options: ProjectOptions,
) -> Result<Value, String> {
    let token =
        std::env::var("CATDESK_MCP_AUTH_TOKEN").map_err(|_| "INSTALLED_BACKEND_AUTH_REQUIRED")?;
    if !valid_token(&token) || !(1..=180).contains(&options.maximum_seconds) {
        return Err("INSTALLED_BACKEND_AUTH_REQUIRED".into());
    }
    let workspace = workspace.to_path_buf();
    tokio::task::spawn_blocking(move || project::run_owned(&workspace, &options, &token))
        .await
        .map_err(|_| "INSTALLED_BACKEND_OWNER_FAILED")?
}

struct DeadlineGuard(mpsc::Sender<()>);
impl DeadlineGuard {
    fn arm(seconds: u64) -> Self {
        let (cancel, wait) = mpsc::channel();
        let _watchdog = thread::spawn(move || {
            if matches!(
                wait.recv_timeout(Duration::from_secs(seconds)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                // Admission has already attached this dedicated CLI to its
                // lifetime Job. OS teardown closes that Job and all children.
                std::process::exit(3);
            }
        });
        Self(cancel)
    }
}
impl Drop for DeadlineGuard {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[cfg(windows)]
struct HolderWatch(std::sync::Arc<std::sync::atomic::AtomicBool>);
#[cfg(windows)]
impl HolderWatch {
    fn start(holder: &mut OwnedCandidateChild) -> Result<Self, String> {
        use std::os::windows::io::{AsHandle, AsRawHandle};
        let handle = holder
            .child_mut()
            .as_handle()
            .try_clone_to_owned()
            .map_err(|_| "INSTALLED_BACKEND_LEASE_WATCH_UNAVAILABLE")?;
        let armed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let flag = armed.clone();
        let _watch = thread::spawn(move || {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
            }
            let result = unsafe { WaitForSingleObject(handle.as_raw_handle().cast(), u32::MAX) };
            if flag.load(std::sync::atomic::Ordering::SeqCst) {
                // Losing the lease holder ends this dedicated controller;
                // its armed lifetime Job immediately stops every child.
                let _ = result;
                std::process::exit(4);
            }
        });
        Ok(Self(armed))
    }
}
#[cfg(windows)]
impl Drop for HolderWatch {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

pub(super) async fn run(workspace: &Path, seconds: u64) -> Result<Value, String> {
    if !(1..=180).contains(&seconds) {
        return Err("INSTALLED_BACKEND_BUDGET_INVALID".into());
    }
    let token =
        std::env::var("CATDESK_MCP_AUTH_TOKEN").map_err(|_| "INSTALLED_BACKEND_AUTH_REQUIRED")?;
    if !valid_token(&token) {
        return Err("INSTALLED_BACKEND_AUTH_REQUIRED".into());
    }
    let workspace = workspace.to_path_buf();
    tokio::task::spawn_blocking(move || run_owned(&workspace, seconds, &token))
        .await
        .map_err(|_| "INSTALLED_BACKEND_OWNER_FAILED")?
}

fn valid_token(token: &str) -> bool {
    (24..=4096).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_graphic())
}
fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}
fn hash(path: &Path) -> Result<String, String> {
    let mut f = fs::File::open(path).map_err(|_| "INSTALLED_BACKEND_FILE_UNAVAILABLE")?;
    let mut sha = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = f
            .read(&mut buffer)
            .map_err(|_| "INSTALLED_BACKEND_FILE_UNAVAILABLE")?;
        if n == 0 {
            break;
        }
        sha.update(&buffer[..n]);
    }
    Ok(format!("{:x}", sha.finalize()))
}
fn regular(root: &Path, relative: &str, maximum: u64) -> Result<PathBuf, String> {
    let path = root.join(relative);
    let m = fs::symlink_metadata(&path).map_err(|_| "INSTALLED_BACKEND_FILE_UNAVAILABLE")?;
    #[cfg(windows)]
    let redirected = {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let redirected = m.file_type().is_symlink();
    let actual = path
        .canonicalize()
        .map(crate::command::normalize_windows_verbatim_path)
        .map_err(|_| "INSTALLED_BACKEND_FILE_UNAVAILABLE")?;
    if redirected
        || !m.is_file()
        || m.len() == 0
        || m.len() > maximum
        || actual != path
        || !actual.starts_with(root)
    {
        return Err("INSTALLED_BACKEND_FILE_UNSAFE".into());
    }
    Ok(actual)
}
fn save(path: &Path, value: &Value) -> Result<(), String> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "INSTALLED_BACKEND_RECORD_UNAVAILABLE")?;
    f.write_all(
        serde_json::to_string_pretty(value)
            .map_err(|_| "INSTALLED_BACKEND_RECORD_INVALID")?
            .as_bytes(),
    )
    .and_then(|()| f.sync_all())
    .map_err(|_| "INSTALLED_BACKEND_RECORD_UNAVAILABLE".into())
}
fn run_directory(root: &Path, id: &str) -> Result<PathBuf, String> {
    let mut path = root.to_path_buf();
    for part in [".catdesk", "recovery-evidence", "installed-backend"] {
        path.push(part);
        if !path.exists() {
            fs::create_dir(&path).map_err(|_| "INSTALLED_BACKEND_RECORD_UNAVAILABLE")?;
        }
        let m = fs::symlink_metadata(&path).map_err(|_| "INSTALLED_BACKEND_RECORD_UNAVAILABLE")?;
        #[cfg(windows)]
        let redirected = {
            use std::os::windows::fs::MetadataExt;
            m.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let redirected = m.file_type().is_symlink();
        if !m.is_dir()
            || redirected
            || path
                .canonicalize()
                .map(crate::command::normalize_windows_verbatim_path)
                .ok()
                .as_ref()
                != Some(&path)
        {
            return Err("INSTALLED_BACKEND_RECORD_UNSAFE".into());
        }
    }
    path.push(id);
    fs::create_dir(&path).map_err(|_| "INSTALLED_BACKEND_RECORD_UNAVAILABLE")?;
    Ok(path)
}
struct Reader {
    first: mpsc::Receiver<Result<Vec<u8>, String>>,
    tail: thread::JoinHandle<super::BoundedOutput>,
    error: thread::JoinHandle<super::BoundedOutput>,
}
fn readers(owner: &mut OwnedCandidateChild) -> Result<Reader, String> {
    let output = owner
        .child_mut()
        .stdout
        .take()
        .ok_or("INSTALLED_BACKEND_OUTPUT_UNAVAILABLE")?;
    let error = owner
        .child_mut()
        .stderr
        .take()
        .ok_or("INSTALLED_BACKEND_OUTPUT_UNAVAILABLE")?;
    let (tx, first) = mpsc::sync_channel(1);
    let tail = thread::spawn(move || {
        let mut input = BufReader::new(output);
        let mut line = Vec::new();
        let result = Read::by_ref(&mut input)
            .take(65537)
            .read_until(b'\n', &mut line)
            .map(|_| line)
            .map_err(|_| "INSTALLED_BACKEND_OUTPUT_UNAVAILABLE".into());
        let _ = tx.send(result);
        super::bounded_reader(input)
    });
    Ok(Reader {
        first,
        tail,
        error: thread::spawn(move || super::bounded_reader(error)),
    })
}
fn first(reader: &Reader) -> Result<Value, String> {
    let line = reader
        .first
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "INSTALLED_BACKEND_READY_TIMEOUT")??;
    if line.is_empty() || line.len() > 65536 {
        return Err("INSTALLED_BACKEND_READY_INVALID".into());
    }
    serde_json::from_slice(&line).map_err(|_| "INSTALLED_BACKEND_READY_INVALID".into())
}
fn finish_readers(reader: Reader, token: &str) -> Result<(), String> {
    let output = reader
        .tail
        .join()
        .map_err(|_| "INSTALLED_BACKEND_OUTPUT_UNAVAILABLE")?;
    let error = reader
        .error
        .join()
        .map_err(|_| "INSTALLED_BACKEND_OUTPUT_UNAVAILABLE")?;
    if output.overflow
        || error.overflow
        || output.read_failed
        || error.read_failed
        || (!token.is_empty() && (output.text.contains(token) || error.text.contains(token)))
    {
        return Err("INSTALLED_BACKEND_OUTPUT_INVALID".into());
    }
    Ok(())
}
fn ps_command(root: &Path, text: String) -> Result<Command, String> {
    let mut c = Command::new(super::trusted_windows_powershell(root)?);
    c.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(text)
        .current_dir(root)
        .env_remove("CATDESK_MCP_AUTH_TOKEN")
        .env_remove("CONTROL_PLANE_API_KEY")
        .env_remove("CATDESK_OPENAI_TUNNEL_ID")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(c)
}
fn prefix(root: &Path, helper: &Path) -> String {
    format!(
        "$ErrorActionPreference='Stop';. {}; $root={};",
        quote(helper),
        quote(root)
    )
}
fn trust_command(root: &Path, helper: &Path) -> String {
    format!(
        "{} $lease=Enter-CatDeskStoppedLifecycleLease $root;try {{ if(-not(Test-CatDeskStoppedReconciliationStartupReady -Root $root -HeldLifecycleLease $lease)){{throw 'startup authority'}};{};[Console]::Out.Flush();if([Console]::In.ReadLine() -cne 'STOP'){{throw 'lease channel closed'}} }}finally{{$lease.Dispose()}}",
        prefix(root, helper),
        selected_release_proof_script()
    )
}

/// Reuse the selected slot's existing authority rather than requiring every
/// future release to have the initial stopped migration's hash/generation.
/// This fragment only reads; it cannot repair, promote or mint authority.
fn selected_release_proof_script() -> &'static str {
    "if(Test-Path -LiteralPath (Join-Path $root '.catdesk/promotion-recovery/promotion-transaction.json')){throw 'pending promotion'};$p=Read-CatDeskLkgPointer $root;if($null -eq $p){throw 'selected pointer missing'};$s=Read-CatDeskLkgSlot $root $p.Slot;$c=Get-CatDeskCanonicalPairEvidence $root;if($null -eq $s -or $p.Hash -cne $s.Hash -or $p.Generation -ne $s.Generation -or $p.Slot -cne $s.Slot -or $c.Hash -cne $s.Hash){throw 'selected release'};$m=ConvertFrom-CatDeskLiteralJson ([IO.File]::ReadAllText($s.Metadata));switch -CaseSensitive ($m.source){'reviewed_promotion'{$r=Get-CatDeskReviewedPromotionAuthority -Root $root -Canonical $c};'reviewed_qualified_stopped'{$r=Get-CatDeskQualifiedStoppedAuthority -Root $root -ExpectedHash $s.Hash};'reviewed_reconciliation'{$r=Get-CatDeskReviewedReconciliationAuthority -Root $root -ExpectedHash $s.Hash};default{throw 'selected authority unsupported'}};[pscustomobject]@{schemaVersion=2;state='TRUSTED_INSTALLED';sha256=$s.Hash;slot=$s.Slot;generation=$s.Generation;authorityKind=$m.source;receiptTransaction=$r.transactionId}|ConvertTo-Json -Compress"
}

fn validate_trust(proof: &Value, expected: &str) -> Result<(), String> {
    let selected_version = matches!(proof["slot"].as_str(), Some("slot-a" | "slot-b"))
        && proof["generation"]
            .as_u64()
            .is_some_and(|generation| (1..=i32::MAX as u64).contains(&generation));
    let authority = match proof["authorityKind"].as_str() {
        Some("reviewed_promotion") => selected_version,
        Some("reviewed_qualified_stopped" | "reviewed_reconciliation") => {
            proof["slot"] == "slot-b" && proof["generation"] == 2
        }
        _ => false,
    };
    if expected.len() != 64
        || !expected.bytes().all(|b| b.is_ascii_hexdigit())
        || proof.as_object().is_none_or(|object| object.len() != 7)
        || proof["schemaVersion"] != 2
        || proof["state"] != "TRUSTED_INSTALLED"
        || proof["sha256"] != expected
        || !authority
        || proof["receiptTransaction"]
            .as_str()
            .is_none_or(|s| s.len() != 32 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("INSTALLED_BACKEND_AUTHORITY_UNPROVEN".into());
    }
    Ok(())
}
fn live(owner: &mut OwnedCandidateChild) -> Result<(), String> {
    if owner
        .child_mut()
        .try_wait()
        .map_err(|_| "INSTALLED_BACKEND_OWNER_UNAVAILABLE")?
        .is_some()
    {
        return Err("INSTALLED_BACKEND_OWNER_EXITED".into());
    }
    Ok(())
}
fn ps_once(root: &Path, text: String) -> Result<Value, String> {
    let mut c = ps_command(root, text)?;
    c.stdin(Stdio::null());
    let mut owner = OwnedCandidateChild::spawn(&mut c)?;
    let reader = readers(&mut owner)?;
    let outcome = first(&reader);
    let deadline = Instant::now() + Duration::from_secs(5);
    let success = loop {
        match owner.child_mut().try_wait() {
            Ok(Some(s)) => break s.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            _ => break false,
        }
    };
    owner.terminate_tree_and_confirm_quiescent()?;
    finish_readers(reader, "")?;
    if !success {
        return Err("INSTALLED_BACKEND_OWNER_PROOF_FAILED".into());
    }
    outcome
}
fn owner_proof(
    root: &Path,
    helper: &Path,
    sha: &str,
    pid: u32,
    start: Option<&str>,
) -> Result<Value, String> {
    let start = start.unwrap_or("").replace('\'', "''");
    let value = ps_once(
        root,
        format!(
            "$ErrorActionPreference='Stop';. {} -LibraryOnly;Get-ReconcileOwnerProof {} '{}' {} '{}'|ConvertTo-Json -Compress",
            quote(helper),
            quote(root),
            sha,
            pid,
            start
        ),
    )?;
    if value["pid"] != pid
        || value["imageSha256"] != sha
        || value["startedAtUtc"].as_str().is_none_or(str::is_empty)
    {
        return Err("INSTALLED_BACKEND_OWNER_PROOF_FAILED".into());
    }
    Ok(value)
}
fn empty_proof(root: &Path, helper: &Path) -> Result<(), String> {
    let value = ps_once(
        root,
        format!(
            "$ErrorActionPreference='Stop';. {} -LibraryOnly;Assert-ReconcilePortEmpty;Assert-ReconcileNoCanonicalProcess {};[pscustomobject]@{{empty=$true}}|ConvertTo-Json -Compress",
            quote(helper),
            quote(root)
        ),
    )?;
    if value["empty"] != true {
        return Err("INSTALLED_BACKEND_NOT_EMPTY".into());
    }
    Ok(())
}
fn validate_ready(value: &Value, root: &Path, config: &Path, seconds: u64) -> Result<(), String> {
    let workspace = value["workspace"]
        .as_str()
        .and_then(|s| Path::new(s).canonicalize().ok())
        .map(crate::command::normalize_windows_verbatim_path);
    if value["status"] != "ready"
        || value["mode"] != "Computer"
        || value["tool_mode"] != "read-only"
        || value["maximum_runtime_seconds"] != seconds
        || value["web_auto_scheduler_enabled"] != false
        || !value["project_scope"].is_null()
        || value["mcp_url"] != LOCAL
        || value["auth"]["scheme"] != "bearer"
        || value["auth"]["token_redacted"] != true
        || workspace.as_deref() != Some(root)
        || value["config_path"].as_str().map(Path::new) != Some(config)
    {
        return Err("INSTALLED_BACKEND_READY_INVALID".into());
    }
    Ok(())
}
pub(crate) async fn verify_mcp_at(endpoint: &str, token: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(endpoint).map_err(|_| "INSTALLED_BACKEND_HTTP_INVALID")?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("INSTALLED_BACKEND_HTTP_INVALID".into());
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| "INSTALLED_BACKEND_HTTP_UNAVAILABLE")?;
    let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});
    let response = client
        .post(endpoint)
        .json(&list)
        .send()
        .await
        .map_err(|_| "INSTALLED_BACKEND_HTTP_UNAVAILABLE")?;
    if response.status() != reqwest::StatusCode::UNAUTHORIZED {
        return Err("INSTALLED_BACKEND_AUTH_UNPROVEN".into());
    }
    for (body, initialize) in [
        (
            json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"catdesk-installed-foreground","version":"1"}}}),
            true,
        ),
        (list, false),
    ] {
        let mut response = client
            .post(endpoint)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|_| "INSTALLED_BACKEND_HTTP_UNAVAILABLE")?;
        if response.status() != reqwest::StatusCode::OK {
            return Err("INSTALLED_BACKEND_AUTH_UNPROVEN".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "INSTALLED_BACKEND_HTTP_UNAVAILABLE")?
        {
            if bytes.len() + chunk.len() > 1048576 {
                return Err("INSTALLED_BACKEND_HTTP_OVERSIZED".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "INSTALLED_BACKEND_HTTP_INVALID")?;
        if value.get("error").is_some() {
            return Err("INSTALLED_BACKEND_HTTP_INVALID".into());
        }
        if initialize {
            if value["result"]["serverInfo"]["name"]
                .as_str()
                .is_none_or(str::is_empty)
            {
                return Err("INSTALLED_BACKEND_HTTP_INVALID".into());
            }
        } else {
            validate_catalog(&value)?;
        }
    }
    Ok(())
}
fn validate_catalog(value: &Value) -> Result<(), String> {
    let names = value["result"]["tools"]
        .as_array()
        .ok_or("INSTALLED_BACKEND_CATALOG_INVALID")?
        .iter()
        .map(|v| v["name"].as_str())
        .collect::<Vec<_>>();
    if names.len() != READ_TOOLS.len()
        || READ_TOOLS
            .iter()
            .any(|name| names.iter().filter(|v| **v == Some(*name)).count() != 1)
    {
        return Err("INSTALLED_BACKEND_CATALOG_INVALID".into());
    }
    Ok(())
}

fn history_snapshot(root: &Path) -> Result<Vec<(String, String)>, String> {
    let mut pending = vec![
        root.join(".catdesk/release-pair-control"),
        root.join(".catdesk/release-recovery"),
    ];
    let mut output = Vec::new();
    let mut visited = 0;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|_| "INSTALLED_BACKEND_HISTORY_UNAVAILABLE")? {
            visited += 1;
            if visited > 256 {
                return Err("INSTALLED_BACKEND_HISTORY_OVERSIZED".into());
            }
            let path = entry
                .map_err(|_| "INSTALLED_BACKEND_HISTORY_UNAVAILABLE")?
                .path();
            let metadata =
                fs::symlink_metadata(&path).map_err(|_| "INSTALLED_BACKEND_HISTORY_UNAVAILABLE")?;
            #[cfg(windows)]
            let redirected = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let redirected = metadata.file_type().is_symlink();
            if redirected {
                return Err("INSTALLED_BACKEND_HISTORY_UNSAFE".into());
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                // The existing exclusive OS lease is not mutable history and
                // deliberately cannot be reopened while its holder is alive.
                if path == root.join(".catdesk/release-pair-control/stopped-owner.lock") {
                    continue;
                }
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| "INSTALLED_BACKEND_HISTORY_UNSAFE")?
                    .to_string_lossy()
                    .into_owned();
                output.push((relative, hash(&path)?));
            } else {
                return Err("INSTALLED_BACKEND_HISTORY_UNSAFE".into());
            }
        }
    }
    output.push((
        "target/release/catdesk.exe.sha256".into(),
        hash(&root.join("target/release/catdesk.exe.sha256"))?,
    ));
    let promotion_receipt = root.join(".catdesk/promotion-recovery/reviewed-promotion.json");
    if promotion_receipt.exists() {
        let receipt = regular(
            root,
            ".catdesk/promotion-recovery/reviewed-promotion.json",
            2048,
        )?;
        output.push((
            ".catdesk/promotion-recovery/reviewed-promotion.json".into(),
            hash(&receipt)?,
        ));
    }
    output.sort();
    Ok(output)
}
pub(crate) fn backend_command(
    root: &Path,
    binary: &Path,
    config: &Path,
    seconds: u64,
    token: &str,
) -> Command {
    let mut command = Command::new(binary);
    command
        .args([
            "--headless-mcp",
            "--host",
            "127.0.0.1",
            "--port",
            "3200",
            "--workspace",
        ])
        .arg(root)
        .arg("--config-path")
        .arg(config)
        .args([
            "--mcp-path",
            ROUTE,
            "--mode",
            "computer",
            "--tool-mode",
            "read-only",
            "--no-ngrok",
            "--no-web-auto",
            "--max-runtime-seconds",
        ])
        .arg(seconds.to_string())
        .env("WORKSPACE_ROOT", root)
        .env("CATDESK_MCP_AUTH_TOKEN", token)
        .env_remove("CONTROL_PLANE_API_KEY")
        .env_remove("CATDESK_OPENAI_TUNNEL_ID")
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[cfg(windows)]
fn run_owned(workspace: &Path, seconds: u64, token: &str) -> Result<Value, String> {
    use std::os::windows::fs::OpenOptionsExt;
    let root = workspace
        .canonicalize()
        .map(crate::command::normalize_windows_verbatim_path)
        .map_err(|_| "INSTALLED_BACKEND_ROOT_UNAVAILABLE")?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    // This rejects MCP/server/general test-host entry before files or children.
    let lifetime = OwnedCandidateChild::enter_installed_backend_lifetime(&id, &root)?;
    let _deadline_guard = DeadlineGuard::arm(seconds);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let recovery = regular(&root, "scripts/catdesk-release-recovery.ps1", 131072)?;
    if hash(&recovery)?
        != format!(
            "{:x}",
            Sha256::digest(include_bytes!("../scripts/catdesk-release-recovery.ps1"))
        )
    {
        return Err("INSTALLED_BACKEND_HELPER_UNQUALIFIED".into());
    }
    let binary = regular(&root, "target/release/catdesk.exe", 134217728)?;
    let expected = hash(&binary)?;
    let authority = regular(
        &root,
        ".catdesk/release-pair-control/authorization.json",
        12288,
    )?;
    let authority: Value = serde_json::from_slice(
        &fs::read(authority).map_err(|_| "INSTALLED_BACKEND_AUTHORITY_UNAVAILABLE")?,
    )
    .map_err(|_| "INSTALLED_BACKEND_AUTHORITY_INVALID")?;
    let reconcile = regular(
        &root,
        "scripts/reconcile-reviewed-catdesk-release-pair.ps1",
        131072,
    )?;
    let powershell = super::trusted_windows_powershell(&root)?;
    if authority["authorization"]["scriptSha256"] != hash(&reconcile)?
        || authority["authorization"]["recoveryHelperSha256"] != hash(&recovery)?
        || authority["authorization"]["powershellSha256"] != hash(&powershell)?
    {
        return Err("INSTALLED_BACKEND_HELPER_UNQUALIFIED".into());
    }
    let _pins = [&binary, &recovery, &reconcile, &powershell]
        .into_iter()
        .map(|path| {
            OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(path)
                .map_err(|_| "INSTALLED_BACKEND_INPUT_PIN_FAILED")
        })
        .collect::<Result<Vec<_>, _>>()?;
    // The handles prevent further replacement; bind the bytes they now pin
    // before any helper is executed, rather than relying on an earlier read.
    if hash(&binary)? != expected
        || authority["authorization"]["scriptSha256"] != hash(&reconcile)?
        || authority["authorization"]["recoveryHelperSha256"] != hash(&recovery)?
        || authority["authorization"]["powershellSha256"] != hash(&powershell)?
    {
        return Err("INSTALLED_BACKEND_INPUT_CHANGED".into());
    }
    let mut command = ps_command(&root, trust_command(&root, &recovery))?;
    command.stdin(Stdio::piped());
    let mut holder = OwnedCandidateChild::spawn(&mut command)?;
    let holder_reader = readers(&mut holder)?;
    let mut directory = None;
    let mut holder_watch = None;
    let mut child_exited = false;
    let outcome: Result<(), String> = (|| {
        let trust = first(&holder_reader)?;
        validate_trust(&trust, &expected)?;
        live(&mut holder)?;
        holder_watch = Some(HolderWatch::start(&mut holder)?);
        let history = history_snapshot(&root)?;
        empty_proof(&root, &reconcile)?;
        live(&mut holder)?;
        let source = regular(
            &root,
            ".catdesk/release-pair-control/qualification-config.toml",
            65536,
        )?;
        let source_hash = hash(&source)?;
        let bytes = fs::read(&source).map_err(|_| "INSTALLED_BACKEND_CONFIG_UNAVAILABLE")?;
        let path = run_directory(&root, &id)?;
        directory = Some(path.clone());
        let config = path.join("config.toml");
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&config)
            .map_err(|_| "INSTALLED_BACKEND_CONFIG_UNAVAILABLE")?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| "INSTALLED_BACKEND_CONFIG_UNAVAILABLE")?;
        drop(file);
        save(
            &path.join("owner.json"),
            &json!({"schemaVersion":1,"runId":id,"profile":"INSTALLED_HEADLESS_LOCAL","controllerPid":lifetime.pid,"controllerCreation":lifetime.creation_time,"controllerSession":lifetime.session_id,"controllerJob":lifetime.job_name,"installedSha256":expected,"sourceConfigSha256":source_hash,"maximumRuntimeSeconds":seconds}),
        )?;
        let child_seconds = deadline
            .saturating_duration_since(Instant::now())
            .as_secs()
            .checked_sub(30)
            .filter(|s| *s > 0)
            .ok_or("INSTALLED_BACKEND_INSUFFICIENT_BUDGET")?;
        let mut command = backend_command(&root, &binary, &config, child_seconds, token);
        live(&mut holder)?;
        if hash(&binary)? != expected {
            return Err("INSTALLED_BACKEND_IMAGE_CHANGED".into());
        }
        // Keep the command's final thirty seconds for proven tree teardown and
        // immutable-input verification, not additional serving or retry.
        let started = Instant::now();
        let mut child = OwnedCandidateChild::spawn(&mut command)?;
        let reader = readers(&mut child)?;
        let child_outcome: Result<(), String> = (|| {
            let ready = first(&reader)?;
            validate_ready(&ready, &root, &config, child_seconds)?;
            live(&mut child)?;
            live(&mut holder)?;
            let before = owner_proof(&root, &reconcile, &expected, child.pid(), None)?;
            let start = before["startedAtUtc"]
                .as_str()
                .ok_or("INSTALLED_BACKEND_OWNER_PROOF_FAILED")?;
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "INSTALLED_BACKEND_HTTP_UNAVAILABLE")?;
            rt.block_on(verify_mcp_at(LOCAL, token))?;
            owner_proof(&root, &reconcile, &expected, child.pid(), Some(start))?;
            let output = json!({"status":"INSTALLED_HEADLESS_READY","profile":"INSTALLED_HEADLESS_LOCAL","runId":id,"pid":child.pid(),"startedAtUtc":start,"installedSha256":expected,"mcpUrl":LOCAL,"commandMaximumSeconds":seconds,"maximumRuntimeSeconds":child_seconds,"readOnly":true,"webAutoSchedulerEnabled":false,"nativeDaemon":false,"unattended":false});
            save(&path.join("ready.json"), &output)?;
            println!("{output}");
            std::io::stdout()
                .flush()
                .map_err(|_| "INSTALLED_BACKEND_OUTPUT_UNAVAILABLE")?;
            loop {
                live(&mut holder)?;
                if let Some(status) = child
                    .child_mut()
                    .try_wait()
                    .map_err(|_| "INSTALLED_BACKEND_WAIT_FAILED")?
                {
                    if status.success() {
                        return Ok(());
                    }
                    return Err("INSTALLED_BACKEND_CHILD_FAILED".into());
                }
                if started.elapsed() > Duration::from_secs(child_seconds + 5) {
                    return Err("INSTALLED_BACKEND_OWNER_DEADLINE".into());
                }
                thread::sleep(Duration::from_millis(50));
            }
        })();
        child.terminate_tree_and_confirm_quiescent()?;
        child_exited = true;
        finish_readers(reader, token)?;
        empty_proof(&root, &reconcile)?;
        if hash(&binary)? != expected || hash(&source)? != source_hash {
            return Err("INSTALLED_BACKEND_PROTECTED_INPUT_CHANGED".into());
        }
        if history_snapshot(&root)? != history {
            return Err("INSTALLED_BACKEND_HISTORY_CHANGED".into());
        }
        child_outcome
    })();
    drop(holder_watch);
    if let Some(mut input) = holder.child_mut().stdin.take() {
        let _ = input.write_all(b"STOP\n");
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while holder.child_mut().try_wait().ok().flatten().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    let cleanup = holder
        .terminate_tree_and_confirm_quiescent()
        .and_then(|()| finish_readers(holder_reader, ""));
    let error = outcome
        .as_ref()
        .err()
        .cloned()
        .or_else(|| cleanup.as_ref().err().cloned());
    let result = json!({"status":if error.is_none(){"INSTALLED_HEADLESS_EXITED"}else{"ATTENTION"},"profile":"INSTALLED_HEADLESS_LOCAL","runId":id,"installedSha256":expected,"ownedTreeExited":child_exited && cleanup.is_ok(),"errorCode":error,"nativeDaemon":false,"unattended":false});
    if let Some(path) = directory {
        save(&path.join("result.json"), &result)?;
    }
    outcome?;
    cleanup?;
    Ok(result)
}
#[cfg(not(windows))]
fn run_owned(_: &Path, _: u64, _: &str) -> Result<Value, String> {
    Err("INSTALLED_BACKEND_WINDOWS_REQUIRED".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_backend_history_detects_drift_and_never_overwrites_records() {
        let root = std::env::temp_dir().join(format!("installed-history-{}", uuid::Uuid::new_v4()));
        for directory in [
            ".catdesk/release-pair-control",
            ".catdesk/release-recovery",
            "target/release",
        ] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        let record = root.join(".catdesk/release-pair-control/claim.json");
        fs::write(&record, b"original").unwrap();
        fs::write(root.join("target/release/catdesk.exe.sha256"), b"original").unwrap();
        let before = history_snapshot(&root).unwrap();
        assert!(save(&record, &json!({"replacement":true})).is_err());
        assert_eq!(fs::read(&record).unwrap(), b"original");
        fs::write(&record, b"changed").unwrap();
        assert_ne!(history_snapshot(&root).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn installed_backend_ready_rejects_wrong_workspace_config_and_auto_work() {
        let root = std::env::temp_dir().join(format!("installed-ready-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let root = root
            .canonicalize()
            .map(crate::command::normalize_windows_verbatim_path)
            .unwrap();
        let config = root.join("config.toml");
        let baseline = json!({"status":"ready","mode":"Computer","tool_mode":"read-only","maximum_runtime_seconds":60,"web_auto_scheduler_enabled":false,"project_scope":null,"mcp_url":LOCAL,"auth":{"scheme":"bearer","token_redacted":true},"workspace":root,"config_path":config});
        assert!(validate_ready(&baseline, &root, &config, 60).is_ok());
        for (key, value) in [
            ("web_auto_scheduler_enabled", json!(true)),
            ("tool_mode", json!("multi-tools")),
            ("config_path", json!("other")),
            ("workspace", json!("other")),
            ("project_scope", json!({"projectId":"other"})),
        ] {
            let mut changed = baseline.clone();
            changed[key] = value;
            assert!(validate_ready(&changed, &root, &config, 60).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn installed_backend_http_checks_real_auth_status_and_read_only_catalog() {
        use axum::{
            Json, Router,
            http::{HeaderMap, StatusCode},
            routing::post,
        };
        for (status, identity, catalog, accepted) in [
            (StatusCode::OK, true, true, true),
            (StatusCode::CREATED, true, true, false),
            (StatusCode::OK, false, true, false),
            (StatusCode::OK, true, false, false),
        ] {
            let router=Router::new().route("/mcp",post(move|headers:HeaderMap,Json(body):Json<Value>|async move {
                if headers.get("authorization").and_then(|v|v.to_str().ok())!=Some("Bearer installed-backend-test-token") {return (StatusCode::UNAUTHORIZED,Json(json!({"error":"unauthorized"})));}
                let value=if body["method"]=="initialize" {if identity {json!({"result":{"serverInfo":{"name":"CatDesk"}}})}else{json!({"result":{"serverInfo":{}}})}}
                    else if catalog {json!({"result":{"tools":READ_TOOLS.iter().map(|name|json!({"name":name})).collect::<Vec<_>>()}})}else{json!({"result":{"tools":[{"name":"write"}]}})};
                (status,Json(value))
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
            let result = verify_mcp_at(&endpoint, "installed-backend-test-token").await;
            server.abort();
            let _ = server.await;
            assert_eq!(result.is_ok(), accepted);
        }
        assert!(
            verify_mcp_at("http://example.invalid/mcp", "installed-backend-test-token")
                .await
                .is_err()
        );
    }
    #[test]
    fn installed_backend_auth_has_native_ascii_bounds() {
        assert!(valid_token(&"a".repeat(24)));
        for token in [
            "a".repeat(23),
            "a".repeat(4097),
            format!("{}\n", "a".repeat(24)),
            "é".repeat(24),
        ] {
            assert!(!valid_token(&token));
        }
    }
    #[test]
    fn installed_backend_catalog_rejects_write_and_missing_tools() {
        let mut tools = READ_TOOLS
            .iter()
            .map(|name| json!({"name":name}))
            .collect::<Vec<_>>();
        assert!(validate_catalog(&json!({"result":{"tools":tools}})).is_ok());
        tools.push(json!({"name":"write"}));
        assert!(validate_catalog(&json!({"result":{"tools":tools}})).is_err());
    }
    #[test]
    fn installed_backend_trust_accepts_reviewed_succession_and_keeps_legacy_identity() {
        let sha = "a".repeat(64);
        let mut p = json!({"schemaVersion":2,"state":"TRUSTED_INSTALLED","sha256":sha,"slot":"slot-b","generation":2,"authorityKind":"reviewed_qualified_stopped","receiptTransaction":"a".repeat(32)});
        assert!(validate_trust(&p, &sha).is_ok());
        p["slot"] = json!("slot-a");
        assert!(validate_trust(&p, &sha).is_err());
        p["generation"] = json!(3);
        assert!(validate_trust(&p, &sha).is_err());
        p["authorityKind"] = json!("reviewed_promotion");
        assert!(validate_trust(&p, &sha).is_ok());
        assert!(validate_trust(&p, &"b".repeat(64)).is_err());
        for (field, value) in [
            ("schemaVersion", json!(1)),
            ("slot", json!("../slot-a")),
            ("generation", json!(0)),
            ("generation", json!(3.5)),
            ("generation", json!(i32::MAX as u64 + 1)),
            ("authorityKind", json!("operational_verified")),
            ("receiptTransaction", json!("x".repeat(32))),
        ] {
            let mut changed = p.clone();
            changed[field] = value;
            assert!(validate_trust(&changed, &sha).is_err(), "{field}");
        }
        p["authorityKind"] = json!("reviewed_reconciliation");
        assert!(validate_trust(&p, &sha).is_err());
        p["slot"] = json!("slot-b");
        p["generation"] = json!(2);
        assert!(validate_trust(&p, &sha).is_ok());
    }
    #[cfg(windows)]
    #[test]
    fn selected_release_native_guards_reject_pending_and_unproven_promotion() {
        let root = std::env::temp_dir().join(format!(
            "catdesk-selected-authority-fixture-{}",
            uuid::Uuid::new_v4()
        ));
        let slot = root.join(".catdesk/release-recovery/slot-a");
        let release = root.join("target/release");
        let promotion = root.join(".catdesk/promotion-recovery");
        let helper = root.join("scripts/catdesk-release-recovery.ps1");
        for dir in [&slot, &release, &promotion, helper.parent().unwrap()] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(
            &helper,
            include_bytes!("../scripts/catdesk-release-recovery.ps1"),
        )
        .unwrap();
        // Inert bytes and synthetic receipts only, isolated from any installed
        // root. This fixture never executes an artifact or creates real trust.
        let bytes = b"non-executable-selected-release-fixture";
        let sha = format!("{:x}", Sha256::digest(bytes));
        for dir in [&slot, &release] {
            fs::write(dir.join("catdesk.exe"), bytes).unwrap();
            fs::write(dir.join("catdesk.exe.sha256"), &sha).unwrap();
        }
        let pointer = json!({"schemaVersion":1,"slot":"slot-a","generation":3,"sha256":sha});
        let manifest = json!({"schemaVersion":1,"generation":3,"source":"reviewed_promotion","sha256":sha,"capturedAtUtc":"2026-10-28T00:00:00Z"});
        let receipt = json!({"schemaVersion":1,"transactionId":"a".repeat(32),"authorizationId":"b".repeat(32),"stage":"CANONICAL_HANDOFF_PROVEN","candidateHash":sha,"canonicalRelativePath":"target\\release\\catdesk.exe","canonicalSha256":sha});
        let pointer_path = root.join(".catdesk/release-recovery/current.json");
        let manifest_path = slot.join("manifest.json");
        let receipt_path = promotion.join("reviewed-promotion.json");
        let pending_path = promotion.join("promotion-transaction.json");
        let command = format!(
            "{} {}",
            prefix(&root, &helper),
            selected_release_proof_script()
        );
        for case in 0..7 {
            let mut p = pointer.clone();
            let mut m = manifest.clone();
            let mut r = receipt.clone();
            if case == 1 {
                r["canonicalSha256"] = json!("c".repeat(64));
            }
            if case == 2 {
                r["stage"] = json!("SWAPPED_PENDING_HANDOFF");
            }
            if case == 3 {
                m["source"] = json!("operational_verified");
            }
            if case == 4 {
                p["generation"] = json!(4);
            }
            fs::write(&pointer_path, serde_json::to_vec(&p).unwrap()).unwrap();
            fs::write(&manifest_path, serde_json::to_vec(&m).unwrap()).unwrap();
            fs::write(&receipt_path, serde_json::to_vec(&r).unwrap()).unwrap();
            if case == 5 {
                fs::remove_file(&receipt_path).unwrap();
            }
            if case == 6 {
                fs::write(&pending_path, b"{}").unwrap();
            }
            let proof = ps_once(&root, command.clone());
            if case == 0 {
                validate_trust(&proof.unwrap(), &sha).unwrap();
            } else {
                assert!(proof.is_err(), "unsafe fixture case {case} was accepted");
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "opt-in read-only inspection of the actual installed selected authority"]
    fn installed_current_selected_authority_read_only() {
        let root = PathBuf::from(
            std::env::var_os("CATDESK_READONLY_INSTALLED_RELEASE_ROOT")
                .expect("explicit existing root required"),
        );
        let helper = regular(&root, "scripts/catdesk-release-recovery.ps1", 131072).unwrap();
        assert_eq!(
            hash(&helper).unwrap(),
            format!(
                "{:x}",
                Sha256::digest(include_bytes!("../scripts/catdesk-release-recovery.ps1"))
            )
        );
        let image = regular(&root, "target/release/catdesk.exe", 134217728).unwrap();
        let before = history_snapshot(&root).unwrap();
        let command = format!(
            "{} $lease=Enter-CatDeskStoppedLifecycleLease $root;try{{if(-not(Test-CatDeskStoppedReconciliationStartupReady -Root $root -HeldLifecycleLease $lease)){{throw 'startup authority'}};{}}}finally{{$lease.Dispose()}}",
            prefix(&root, &helper),
            selected_release_proof_script()
        );
        let proof = ps_once(&root, command).unwrap();
        validate_trust(&proof, &hash(&image).unwrap()).unwrap();
        assert_eq!(history_snapshot(&root).unwrap(), before);
        println!(
            "READ_ONLY_SELECTED_RELEASE_FACTS={}",
            json!({"slot":proof["slot"],"generation":proof["generation"],"authorityKind":proof["authorityKind"],"sha256":proof["sha256"],"authorityWrites":0,"backendStarts":0})
        );
    }
    #[test]
    fn installed_backend_script_holds_real_lease_without_mutating_release() {
        let s = trust_command(Path::new("C:/root'quoted"), Path::new("C:/root/helper.ps1"));
        assert!(s.contains("Read-CatDeskLkgPointer"));
        assert!(s.contains("Read-CatDeskLkgSlot"));
        assert!(s.contains("Get-CatDeskReviewedPromotionAuthority"));
        assert!(s.contains("promotion-transaction.json"));
        assert!(s.contains("-HeldLifecycleLease $lease"));
        assert!(s.contains("ReadLine()"));
        assert!(s.contains("root''quoted"));
        for denied in [
            "Restore-CatDesk",
            "WriteAll",
            "Start-Process",
            "Stop-Process",
        ] {
            assert!(!s.contains(denied));
        }
    }
    #[test]
    fn installed_backend_command_has_no_native_or_secret_arguments() {
        let token = "a".repeat(32);
        let c = backend_command(
            Path::new("C:/root"),
            Path::new("C:/root/target/release/catdesk.exe"),
            Path::new("C:/root/private/config.toml"),
            60,
            &token,
        );
        let args = c
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.iter().any(|a| a == "--headless-mcp"));
        assert!(args.iter().any(|a| a == "--no-web-auto"));
        assert!(args.iter().any(|a| a == "--no-ngrok"));
        assert!(args.iter().any(|a| a == "read-only"));
        assert!(
            !args
                .iter()
                .any(|a| a.contains(&token) || a == "--catdesk-daemon" || a == "--auth-token")
        );
    }
}
