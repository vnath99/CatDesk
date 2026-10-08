use crate::{
    Result, VERSION,
    protocol::Target,
    store::{HostLease, Phase, Receipt, ResponseState, Store, TurnTimer, atomic, now, read},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Control {
    pub schema_version: u32,
    pub desired: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Status {
    pub version: String,
    pub protocol_version: u32,
    pub host: String,
    pub pid: u32,
    pub updated_utc: u64,
    pub targets: BTreeMap<String, Target>,
    pub queue_depth: usize,
    pub stale_count: usize,
    pub browser: String,
    pub login: String,
    pub submission: String,
    pub last_event_id: Option<String>,
    pub last_attempt_utc: Option<u64>,
    pub last_success_utc: Option<u64>,
    pub last_receipt: Option<Receipt>,
    pub attention: Option<String>,
    #[serde(default)]
    pub host_recovery_count: u64,
    #[serde(default)]
    pub last_host_error: Option<String>,
    #[serde(default)]
    pub last_host_error_utc: Option<u64>,
    #[serde(default)]
    pub turn_timers: Vec<TurnStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadinessStatus {
    pub observed_utc: u64,
    pub event_id: Option<String>,
    pub route: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadinessJournal {
    schema_version: u32,
    entries: Vec<ReadinessStatus>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueHealth {
    pub schema_version: u32,
    pub actionable: usize,
    pub reconciliation: usize,
    pub attention: usize,
    pub stale: usize,
    pub forensic: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStatus {
    pub event_id: String,
    pub started_utc: u64,
    pub elapsed_seconds: u64,
    pub remaining_seconds: u64,
    pub soft_checkpoint_reached: bool,
    pub deadline_reached: bool,
    pub response_state: ResponseState,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedInstallStatus {
    pub compiled_version: String,
    pub current_directory: Option<String>,
    pub current_version: Option<String>,
    pub reviewed_candidate_directory: String,
    pub reviewed_candidate_version: String,
    pub already_current: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedInstallHandoff {
    pub directory: String,
    pub version: String,
    pub prior_desired: String,
    pub already_current: bool,
    pub final_host: String,
    pub final_pid: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedInstallPublication {
    pub directory: String,
    pub version: String,
    pub already_materialized: bool,
}

#[derive(Debug)]
struct ReviewedInstallLease {
    _file: File,
}

fn reviewed_install_lease(store: &Store) -> Result<ReviewedInstallLease> {
    let path = store.root().join("install.lock");
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let file = options
        .open(path)
        .map_err(|_| "HOST_INSTALL_ALREADY_RUNNING".to_string())?;
    Ok(ReviewedInstallLease { _file: file })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewedInstallTransaction {
    schema_version: u32,
    candidate_directory: String,
    prior_desired: String,
}

fn reviewed_install_transaction_path(store: &Store) -> PathBuf {
    store.root().join("reviewed-install-handoff.json")
}

fn begin_or_resume_reviewed_install_handoff(store: &Store, candidate: &str) -> Result<String> {
    let path = reviewed_install_transaction_path(store);
    if path.exists() {
        let transaction: ReviewedInstallTransaction = read(&path)?;
        if transaction.schema_version != 1
            || !matches!(
                transaction.prior_desired.as_str(),
                "RUNNING" | "PAUSED" | "STOPPED"
            )
        {
            return Err("HOST_INSTALL_HANDOFF_CONFLICT".into());
        }
        if transaction.candidate_directory == candidate {
            return Ok(transaction.prior_desired);
        }

        // A failed activation may already have switched current.json to the
        // old candidate before that candidate's host failed to start. Permit a
        // newer reviewed candidate to continue the interrupted transaction
        // only when the old candidate is still the exact current pointer and
        // no WakeHost owns the singleton lease. Preserve the original desired
        // state so a failed upgrade cannot silently turn RUNNING into STOPPED.
        let current: serde_json::Value = read(&store.root().join("current.json"))
            .map_err(|_| "HOST_INSTALL_HANDOFF_CONFLICT")?;
        if install_id(&current)? != transaction.candidate_directory {
            return Err("HOST_INSTALL_HANDOFF_CONFLICT".into());
        }
        match store.host_lock() {
            Ok(lease) => drop(lease),
            Err(_) => return Err("HOST_INSTALL_HANDOFF_CONFLICT".into()),
        }
        atomic(
            &path,
            &ReviewedInstallTransaction {
                schema_version: 1,
                candidate_directory: candidate.into(),
                prior_desired: transaction.prior_desired.clone(),
            },
        )?;
        return Ok(transaction.prior_desired);
    }
    let prior_desired = desired(store)?;
    atomic(
        &path,
        &ReviewedInstallTransaction {
            schema_version: 1,
            candidate_directory: candidate.into(),
            prior_desired: prior_desired.clone(),
        },
    )?;
    Ok(prior_desired)
}

fn complete_reviewed_install_handoff(store: &Store, candidate: &str) -> Result<()> {
    let path = reviewed_install_transaction_path(store);
    let transaction: ReviewedInstallTransaction = read(&path)?;
    if transaction.schema_version != 1 || transaction.candidate_directory != candidate {
        return Err("HOST_INSTALL_HANDOFF_CONFLICT".into());
    }
    std::fs::remove_file(&path).map_err(|_| "HOST_INSTALL_HANDOFF_CLEANUP_FAILED".into())
}

pub fn default_root() -> Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("CONFIG_LOCALAPPDATA_MISSING")?)
            .join("CatDeskWake"),
    )
}

pub fn control(store: &Store, desired: &str) -> Result<()> {
    if !matches!(desired, "RUNNING" | "PAUSED" | "STOPPED") {
        return Err("HOST_CONTROL_INVALID".into());
    }
    atomic(
        &store.root().join("control.json"),
        &Control {
            schema_version: 1,
            desired: desired.into(),
        },
    )
}

fn desired(store: &Store) -> Result<String> {
    let value: Control = read(&store.root().join("control.json"))?;
    if value.schema_version != 1
        || !matches!(value.desired.as_str(), "RUNNING" | "PAUSED" | "STOPPED")
    {
        return Err("HOST_CONTROL_INVALID".into());
    }
    Ok(value.desired)
}

pub fn status(store: &Store) -> Result<Status> {
    let config = store.config()?;
    let mut status = match read::<Status>(&store.root().join("status.json")) {
        Ok(status) => status,
        Err(_) => Status {
            version: VERSION.into(),
            protocol_version: 1,
            host: "STOPPED".into(),
            pid: 0,
            updated_utc: now(),
            targets: config.targets.clone(),
            queue_depth: 0,
            stale_count: 0,
            browser: "NOT_OBSERVED".into(),
            login: "NOT_OBSERVED".into(),
            submission: "IDLE".into(),
            last_event_id: None,
            last_attempt_utc: None,
            last_success_utc: None,
            last_receipt: None,
            attention: None,
            host_recovery_count: 0,
            last_host_error: None,
            last_host_error_utc: None,
            turn_timers: vec![],
        },
    };
    status.targets = config.targets;
    let observed_utc = now();
    refresh_live_observability(store, &mut status, observed_utc)?;
    // Browser/login/submission are live-attempt fields, not historical health.
    // Once no current-generation event remains actionable, carrying an old
    // ATTENTION/CLAIMED value forward makes a healthy idle host look broken.
    // Preserve last-event/receipt timestamps as history, but normalize the
    // transient attempt surface back to an explicit idle/not-observed state.
    let preserve_last_attempt_surface = status
        .last_event_id
        .as_deref()
        .and_then(|id| store.delivery(id).ok().flatten())
        .is_some_and(|delivery| {
            status.targets.get(&delivery.event.project_id) == Some(&delivery.target)
                && matches!(
                    delivery.phase,
                    Phase::Claimed | Phase::Submitting | Phase::Attention
                )
        });
    if status.queue_depth == 0 && !preserve_last_attempt_surface {
        status.browser = "NOT_OBSERVED".into();
        status.login = "NOT_OBSERVED".into();
        status.submission = "IDLE".into();
    }
    match store.host_lock() {
        Ok(_) => {
            status.host = "STOPPED".into();
            status.pid = 0;
        }
        Err(e) if e == "HOST_ALREADY_RUNNING" => {
            if now().saturating_sub(status.updated_utc) > 10 {
                status.host = "ERROR".into();
                status.attention = Some("HOST_HEARTBEAT_STALE".into());
            }
        }
        Err(e) => return Err(e),
    }
    Ok(status)
}

pub fn turn_timer_status(timer: TurnTimer, observed_utc: u64) -> TurnStatus {
    let terminal_utc = timer.completed_utc.unwrap_or(observed_utc);
    let elapsed = terminal_utc.saturating_sub(timer.started_utc);
    TurnStatus {
        event_id: timer.event_id,
        started_utc: timer.started_utc,
        elapsed_seconds: elapsed,
        remaining_seconds: TURN_TARGET_SECONDS.saturating_sub(elapsed),
        soft_checkpoint_reached: elapsed >= TURN_SOFT_CHECKPOINT_SECONDS,
        deadline_reached: elapsed >= TURN_TARGET_SECONDS,
        response_state: timer.response_state,
    }
}

fn refresh_live_observability(store: &Store, status: &mut Status, observed_utc: u64) -> Result<()> {
    status.turn_timers = store
        .timers()?
        .into_iter()
        .filter(|timer| {
            status.targets.get(&timer.project_id).is_some_and(|target| {
                target.generation == timer.target_generation && target.digest == timer.target_digest
            })
        })
        .map(|timer| turn_timer_status(timer, observed_utc))
        .collect();
    // Show current response observation ahead of retained forensic/completed
    // timers. Sorting is presentation only; the durable records stay intact.
    status.turn_timers.sort_by_key(|timer| {
        let priority = match timer.response_state {
            ResponseState::Observing | ResponseState::Retrying => 0,
            ResponseState::Attention => 1,
            ResponseState::Complete => 2,
        };
        (
            priority,
            std::cmp::Reverse(timer.started_utc),
            timer.event_id.clone(),
        )
    });
    status.queue_depth = 0;
    status.stale_count = 0;
    for event in store.events()? {
        let delivery = store.delivery(&event.event_id)?;
        if delivery
            .as_ref()
            .is_some_and(|value| value.phase == Phase::Sent)
        {
            continue;
        }
        if delivery
            .as_ref()
            .is_some_and(|value| value.phase == Phase::Stale)
            || status
                .targets
                .get(&event.project_id)
                .is_none_or(|target| target.generation != event.target_generation)
        {
            status.stale_count += 1;
            continue;
        }
        // queueDepth is USER-deliverable work only. SUBMITTING and ATTENTION
        // remain durable reconciliation/forensic evidence and must not look
        // like events Wake is permitted to submit again.
        if delivery
            .as_ref()
            .is_none_or(|value| value.phase == Phase::Claimed)
        {
            status.queue_depth += 1;
        }
    }
    // A terminal last attempt has durable evidence. Never associate a prior
    // quarantined event's transient fields with this last-event identity.
    if let Some(id) = status.last_event_id.as_deref()
        && let Some(delivery) = store.delivery(id)?
        && status.targets.get(&delivery.event.project_id) == Some(&delivery.target)
    {
        match delivery.phase {
            Phase::Attention => {
                status.submission = "ATTENTION".into();
                status.browser = "ATTENTION".into();
                status.attention = delivery.reason;
            }
            Phase::Sent => {
                status.submission = "SENT".into();
                status.attention = None;
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn queue_health(store: &Store) -> Result<QueueHealth> {
    let config = store.config()?;
    let mut result = QueueHealth {
        schema_version: 1,
        actionable: 0,
        reconciliation: 0,
        attention: 0,
        stale: 0,
        forensic: 0,
    };
    for event in store.events()? {
        let delivery = store.delivery(&event.event_id)?;
        if delivery
            .as_ref()
            .is_some_and(|value| value.phase == Phase::Sent)
        {
            continue;
        }
        if delivery
            .as_ref()
            .is_some_and(|value| value.phase == Phase::Stale)
            || config
                .targets
                .get(&event.project_id)
                .is_none_or(|target| target.generation != event.target_generation)
        {
            result.stale += 1;
            continue;
        }
        match delivery.as_ref().map(|value| &value.phase) {
            None | Some(Phase::Claimed) => result.actionable += 1,
            Some(Phase::Submitting) => result.reconciliation += 1,
            Some(Phase::Attention) => result.attention += 1,
            Some(Phase::Stale | Phase::Sent) => {}
        }
    }
    result.forensic = result.reconciliation + result.attention + result.stale;
    Ok(result)
}

fn persist_live_status(store: &Store, status: &mut Status) -> Result<()> {
    let observed_utc = now();
    status.updated_utc = observed_utc;
    status.targets = store.config()?.targets;
    refresh_live_observability(store, status, observed_utc)?;
    atomic(&store.root().join("status.json"), status)
}

fn hidden(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

#[cfg(windows)]
fn browser_process_root(root: &Path) -> Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    if !root.is_absolute() {
        return Err("BROWSER_PROFILE_PATH_UNSUPPORTED".into());
    }

    let units = root.as_os_str().encode_wide().collect::<Vec<_>>();
    let verbatim_prefix = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    if units.starts_with(&verbatim_prefix) {
        // Wake's root is a local %LOCALAPPDATA% directory. Convert only a
        // verbatim local-drive path (\\?\C:\...) back to the ordinary Win32
        // drive form before handing it to SeleniumBase/Chrome. Keep the
        // canonical verbatim root everywhere else for filesystem authority.
        if units.len() < 7
            || units[5] != b':' as u16
            || !matches!(units[6], value if value == b'\\' as u16 || value == b'/' as u16)
        {
            return Err("BROWSER_PROFILE_PATH_UNSUPPORTED".into());
        }
        let normal = PathBuf::from(OsString::from_wide(&units[4..]));
        if !normal.is_absolute() {
            return Err("BROWSER_PROFILE_PATH_UNSUPPORTED".into());
        }
        return Ok(normal);
    }

    Ok(root.to_path_buf())
}

#[cfg(not(windows))]
fn browser_process_root(root: &Path) -> Result<PathBuf> {
    if !root.is_absolute() {
        return Err("BROWSER_PROFILE_PATH_UNSUPPORTED".into());
    }
    Ok(root.to_path_buf())
}

#[cfg(all(test, windows))]
mod browser_process_root_tests {
    use super::*;

    #[test]
    fn verbatim_local_drive_root_is_normalized_only_for_browser_processes() {
        let canonical = Path::new(r"\\?\C:\fixture-root\CatDeskWake");
        let browser = browser_process_root(canonical).expect("browser-compatible root");
        assert_eq!(browser, PathBuf::from(r"C:\fixture-root\CatDeskWake"));
        assert!(!browser.to_string_lossy().starts_with(r"\\?\"));
    }

    #[test]
    fn ordinary_absolute_drive_root_is_preserved() {
        let ordinary = Path::new(r"C:\fixture-root\CatDeskWake");
        assert_eq!(
            browser_process_root(ordinary).expect("ordinary browser root"),
            ordinary
        );
    }

    #[test]
    fn non_local_verbatim_root_is_refused_at_browser_boundary() {
        assert_eq!(
            browser_process_root(Path::new(r"\\?\UNC\server\share\CatDeskWake")).unwrap_err(),
            "BROWSER_PROFILE_PATH_UNSUPPORTED"
        );
    }
}

pub fn start(store: &Store) -> Result<()> {
    let executable = std::env::current_exe().map_err(|_| "HOST_EXECUTABLE_UNAVAILABLE")?;
    start_executable(store, &executable)
}

fn install_id(pointer: &serde_json::Value) -> Result<&str> {
    pointer
        .get("directory")
        .and_then(|v| v.as_str())
        .filter(|v| {
            !v.is_empty()
                && v.len() <= 200
                && !v.contains("..")
                && v.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
        })
        .ok_or_else(|| "HOST_INSTALL_POINTER_INVALID".into())
}

fn sha256_file(path: &Path, unavailable: &str) -> Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).map_err(|_| unavailable.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn pointer_hash<'a>(pointer: &'a serde_json::Value, field: &str) -> Result<&'a str> {
    pointer
        .get(field)
        .and_then(|v| v.as_str())
        .filter(|value| value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| "HOST_INSTALL_POINTER_INVALID".into())
}

fn manifest_hash<'a>(manifest: &'a serde_json::Value, name: &str) -> Result<&'a str> {
    if manifest.get("schemaVersion").and_then(|v| v.as_u64()) != Some(1) {
        return Err("HOST_INSTALL_MANIFEST_INVALID".into());
    }
    manifest
        .get("artifacts")
        .and_then(|v| v.get(name))
        .and_then(|v| v.as_str())
        .filter(|value| value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| "HOST_INSTALL_MANIFEST_INVALID".into())
}

pub fn verify_installed_browser_artifacts(
    root: &Path,
    installed: &Path,
) -> Result<(PathBuf, PathBuf)> {
    let pointer: serde_json::Value = read(&root.join("current.json"))?;
    let id = install_id(&pointer)?;
    let expected_directory = root.join("versions").join(id);
    let actual_directory =
        std::fs::canonicalize(installed).map_err(|_| "HOST_INSTALL_UNAVAILABLE".to_string())?;
    let expected_directory = std::fs::canonicalize(&expected_directory)
        .map_err(|_| "HOST_INSTALL_UNAVAILABLE".to_string())?;
    if actual_directory != expected_directory {
        return Err("HOST_INSTALL_POINTER_MISMATCH".into());
    }

    let manifest: serde_json::Value = read(&expected_directory.join("manifest.json"))?;
    let checks = [
        (
            "adapter.py",
            "adapterSha256",
            "BROWSER_ADAPTER_UNAVAILABLE",
            "BROWSER_ADAPTER_HASH_MISMATCH",
        ),
        (
            "wake_bridge.py",
            "wakeBridgeSha256",
            "BROWSER_PRIMITIVES_UNAVAILABLE",
            "BROWSER_PRIMITIVES_HASH_MISMATCH",
        ),
    ];
    for (name, pointer_field, unavailable, mismatch) in checks {
        let path = expected_directory.join(name);
        let actual = sha256_file(&path, unavailable)?;
        let expected = pointer_hash(&pointer, pointer_field)?;
        let manifest_expected = manifest_hash(&manifest, name)?;
        if !actual.eq_ignore_ascii_case(expected) || !actual.eq_ignore_ascii_case(manifest_expected)
        {
            return Err(mismatch.into());
        }
    }

    Ok((
        expected_directory.join("adapter.py"),
        expected_directory.join("wake_bridge.py"),
    ))
}

pub fn start_installed(store: &Store) -> Result<()> {
    if store.config()?.targets.is_empty() {
        return Ok(());
    }
    let pointer: serde_json::Value = read(&store.root().join("current.json"))?;
    let version = install_id(&pointer)?;
    let install_directory = store.root().join("versions").join(version);
    let executable = install_directory.join("CatDeskWakeHost.exe");
    let hash = sha256_file(&executable, "HOST_EXECUTABLE_UNAVAILABLE")?;
    if !hash.eq_ignore_ascii_case(pointer_hash(&pointer, "hostSha256")?) {
        return Err("HOST_EXECUTABLE_HASH_MISMATCH".into());
    }
    verify_installed_browser_artifacts(store.root(), &install_directory)?;
    start_executable(store, &executable)
}

fn stopped_owner_requires_handoff_wait(host: &str) -> Result<bool> {
    match host {
        "RUNNING" | "PAUSED" => Ok(false),
        "STOPPED" => Ok(true),
        _ => Err("HOST_EXISTING_OWNER_STATE_INVALID".into()),
    }
}

fn start_executable(store: &Store, executable: &Path) -> Result<()> {
    if store.config()?.targets.is_empty() {
        return Ok(());
    }
    // A previous reviewed owner writes STOPPED before its process tree and
    // singleton lease are necessarily fully unwound. Treating the still-held
    // lease as a successful start can strand Wake with no running owner after
    // package handoff. Existing RUNNING/PAUSED owners still satisfy start(),
    // but a STOPPED owner gets a bounded lease-release window.
    let mut startable = false;
    for _ in 0..100 {
        match store.host_lock() {
            Ok(lease) => {
                drop(lease);
                startable = true;
                break;
            }
            Err(e) if e == "HOST_ALREADY_RUNNING" => {
                let existing: Status = read(&store.root().join("status.json"))?;
                if stopped_owner_requires_handoff_wait(&existing.host)? {
                    thread::sleep(Duration::from_millis(100));
                } else {
                    return Ok(());
                }
            }
            Err(e) => return Err(e),
        }
    }
    if !startable {
        return Err("HOST_STOPPED_OWNER_EXIT_TIMEOUT".into());
    }
    control(store, "RUNNING")?;
    let mut command = Command::new(executable);
    command
        .arg("--host")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hidden(&mut command);
    command.spawn().map_err(|_| "HOST_START_FAILED")?;
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(100));
        if status(store)?.host == "RUNNING" {
            return Ok(());
        }
    }
    Err("HOST_START_TIMEOUT".into())
}

fn valid_install_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 240
        && !value.contains("..")
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
}

fn validate_reviewed_manifest(manifest: &serde_json::Value) -> Result<String> {
    if manifest.get("schemaVersion").and_then(|v| v.as_u64()) != Some(1)
        || manifest.get("version").and_then(|v| v.as_str()) != Some(VERSION)
        || manifest.get("protocolVersion").and_then(|v| v.as_u64()) != Some(1)
        || manifest.get("acceptance").and_then(|v| v.as_str()) != Some("DEVELOPMENT_NOT_ACTIVATED")
    {
        return Err("HOST_INSTALL_MANIFEST_INVALID".into());
    }
    let host = manifest_hash(manifest, "CatDeskWakeHost.exe")?;
    let gui = manifest_hash(manifest, "CatDeskBinagotchy.exe")?;
    Ok(format!("{}-{}-{}", VERSION, &host[..12], &gui[..12]))
}

fn validate_reviewed_candidate(
    directory: &Path,
    manifest: &serde_json::Value,
) -> Result<BTreeMap<String, String>> {
    let mut actual = BTreeMap::new();
    for (name, unavailable) in [
        ("CatDeskWakeHost.exe", "HOST_EXECUTABLE_UNAVAILABLE"),
        ("CatDeskBinagotchy.exe", "BINAGOTCHY_EXECUTABLE_UNAVAILABLE"),
        ("adapter.py", "BROWSER_ADAPTER_UNAVAILABLE"),
        ("wake_bridge.py", "BROWSER_PRIMITIVES_UNAVAILABLE"),
    ] {
        let digest = sha256_file(&directory.join(name), unavailable)?;
        if !digest.eq_ignore_ascii_case(manifest_hash(manifest, name)?) {
            return Err("HOST_INSTALL_MANIFEST_HASH_MISMATCH".into());
        }
        actual.insert(name.to_string(), digest);
    }
    Ok(actual)
}

pub fn publish_reviewed_install(
    store: &Store,
    staging_name: &str,
    expected_id: &str,
) -> Result<ReviewedInstallPublication> {
    if !staging_name.starts_with(".staging-")
        || !valid_install_component(staging_name)
        || !valid_install_component(expected_id)
        || expected_id.starts_with(".staging-")
    {
        return Err("HOST_INSTALL_POINTER_INVALID".into());
    }

    let _lease = reviewed_install_lease(store)?;
    let versions = store.root().join("versions");
    let staging = versions.join(staging_name);
    if !staging.is_dir() {
        return Err("HOST_INSTALL_STAGING_UNAVAILABLE".into());
    }
    let manifest: serde_json::Value = read(&staging.join("manifest.json"))?;
    let canonical_id = validate_reviewed_manifest(&manifest)?;
    if canonical_id != expected_id {
        return Err("HOST_INSTALL_IDENTITY_MISMATCH".into());
    }
    let staging_hashes = validate_reviewed_candidate(&staging, &manifest)?;

    let final_directory = versions.join(expected_id);
    let mut already_materialized = false;
    for entry in std::fs::read_dir(&versions).map_err(|_| "HOST_INSTALL_UNAVAILABLE")? {
        let entry = entry.map_err(|_| "HOST_INSTALL_UNAVAILABLE")?;
        if !entry
            .file_type()
            .map_err(|_| "HOST_INSTALL_UNAVAILABLE")?
            .is_dir()
        {
            continue;
        }
        let name = entry
            .file_name()
            .to_str()
            .ok_or("HOST_INSTALL_POINTER_INVALID")?
            .to_string();
        if name.starts_with(".staging-") {
            continue;
        }
        let candidate_manifest: serde_json::Value = match read(&entry.path().join("manifest.json"))
        {
            Ok(value) => value,
            Err(_) => continue,
        };
        if candidate_manifest.get("version").and_then(|v| v.as_str()) != Some(VERSION) {
            continue;
        }
        if name != expected_id {
            return Err("HOST_INSTALL_VERSION_IDENTITY_CONFLICT".into());
        }
        let canonical_existing = validate_reviewed_manifest(&candidate_manifest)?;
        if canonical_existing != expected_id {
            return Err("HOST_INSTALL_IDENTITY_MISMATCH".into());
        }
        let existing_hashes = validate_reviewed_candidate(&entry.path(), &candidate_manifest)?;
        if existing_hashes != staging_hashes {
            return Err("HOST_INSTALL_ARTIFACT_IDENTITY_CONFLICT".into());
        }
        already_materialized = true;
    }

    if already_materialized {
        std::fs::remove_dir_all(&staging)
            .map_err(|_| "HOST_INSTALL_STAGING_CLEANUP_FAILED".to_string())?;
    } else {
        std::fs::rename(&staging, &final_directory)
            .map_err(|_| "HOST_INSTALL_PUBLISH_FAILED".to_string())?;
    }

    Ok(ReviewedInstallPublication {
        directory: expected_id.to_string(),
        version: VERSION.into(),
        already_materialized,
    })
}

fn reviewed_candidate(store: &Store) -> Result<(String, PathBuf, serde_json::Value)> {
    let versions = store.root().join("versions");
    let mut matches = Vec::new();
    for entry in std::fs::read_dir(&versions).map_err(|_| "HOST_INSTALL_UNAVAILABLE")? {
        let entry = entry.map_err(|_| "HOST_INSTALL_UNAVAILABLE")?;
        let directory = entry.path();
        if !entry
            .file_type()
            .map_err(|_| "HOST_INSTALL_UNAVAILABLE")?
            .is_dir()
        {
            continue;
        }
        let id = entry
            .file_name()
            .to_str()
            .filter(|value| !value.is_empty())
            .ok_or("HOST_INSTALL_POINTER_INVALID")?
            .to_string();
        // Interrupted materialization is never a reviewed candidate, even if a
        // complete manifest happened to be written before the interruption.
        if id.starts_with(".staging-") {
            continue;
        }
        if !valid_install_component(&id) {
            return Err("HOST_INSTALL_POINTER_INVALID".into());
        }
        let manifest: serde_json::Value = match read(&directory.join("manifest.json")) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if manifest.get("version").and_then(|v| v.as_str()) != Some(VERSION) {
            continue;
        }
        if validate_reviewed_manifest(&manifest)? != id {
            return Err("HOST_INSTALL_IDENTITY_MISMATCH".into());
        }
        validate_reviewed_candidate(&directory, &manifest)?;
        matches.push((id, directory, manifest));
    }
    if matches.is_empty() {
        return Err("HOST_REVIEWED_CANDIDATE_UNAVAILABLE".into());
    }
    if matches.len() != 1 {
        return Err("HOST_REVIEWED_CANDIDATE_AMBIGUOUS".into());
    }
    Ok(matches.remove(0))
}

pub fn reviewed_install_status(store: &Store) -> Result<ReviewedInstallStatus> {
    let (id, _directory, manifest) = reviewed_candidate(store)?;
    let candidate_version = manifest
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or("HOST_INSTALL_MANIFEST_INVALID")?
        .to_string();
    let current = read::<serde_json::Value>(&store.root().join("current.json")).ok();
    let current_directory = current
        .as_ref()
        .and_then(|value| install_id(value).ok())
        .map(str::to_string);
    let current_version = current_directory.as_ref().and_then(|directory| {
        read::<serde_json::Value>(
            &store
                .root()
                .join("versions")
                .join(directory)
                .join("manifest.json"),
        )
        .ok()
        .and_then(|value| {
            value
                .get("version")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
    });
    Ok(ReviewedInstallStatus {
        compiled_version: VERSION.into(),
        already_current: current_directory.as_deref() == Some(id.as_str()),
        current_directory,
        current_version,
        reviewed_candidate_directory: id,
        reviewed_candidate_version: candidate_version,
    })
}

pub fn activate_reviewed_install(store: &Store) -> Result<ReviewedInstallHandoff> {
    let _lease = reviewed_install_lease(store)?;
    let (id, directory, manifest) = reviewed_candidate(store)?;
    let host = directory.join("CatDeskWakeHost.exe");
    let gui = directory.join("CatDeskBinagotchy.exe");
    let host_hash = sha256_file(&host, "HOST_EXECUTABLE_UNAVAILABLE")?;
    let gui_hash = sha256_file(&gui, "BINAGOTCHY_EXECUTABLE_UNAVAILABLE")?;
    let adapter_hash = sha256_file(&directory.join("adapter.py"), "BROWSER_ADAPTER_UNAVAILABLE")?;
    let wake_bridge_hash = sha256_file(
        &directory.join("wake_bridge.py"),
        "BROWSER_PRIMITIVES_UNAVAILABLE",
    )?;
    for (name, actual) in [
        ("CatDeskWakeHost.exe", host_hash.as_str()),
        ("CatDeskBinagotchy.exe", gui_hash.as_str()),
        ("adapter.py", adapter_hash.as_str()),
        ("wake_bridge.py", wake_bridge_hash.as_str()),
    ] {
        if !actual.eq_ignore_ascii_case(manifest_hash(&manifest, name)?) {
            return Err("HOST_INSTALL_MANIFEST_HASH_MISMATCH".into());
        }
    }
    let path = store.root().join("current.json");
    let previous = read::<serde_json::Value>(&path).ok();
    let already_current =
        previous.as_ref().and_then(|value| install_id(value).ok()) == Some(id.as_str());
    // Persist the pre-handoff desired state before writing STOPPED.
    // If the activating process is interrupted after that write, a later exact
    // retry resumes the same candidate and restores the original RUNNING/PAUSED
    // state instead of mistaking the transient STOPPED value for operator intent.
    let prior_desired = begin_or_resume_reviewed_install_handoff(store, &id)?;
    if !already_current {
        control(store, "STOPPED")?;
        for _ in 0..100 {
            if status(store)?.host == "STOPPED" && store.host_lock().is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        if store.host_lock().is_err() {
            return Err("HOST_STOPPED_OWNER_EXIT_TIMEOUT".into());
        }
        if let Some(previous) = previous.as_ref() {
            atomic(&store.root().join("previous.json"), previous)?;
        }
        atomic(
            &path,
            &serde_json::json!({"schemaVersion":1,"directory":id,"hostSha256":host_hash,"binagotchySha256":gui_hash,"adapterSha256":adapter_hash,"wakeBridgeSha256":wake_bridge_hash}),
        )?;
    }
    if prior_desired == "RUNNING" {
        start_installed(store)?;
    } else {
        control(store, &prior_desired)?;
    }
    let final_status = status(store)?;
    complete_reviewed_install_handoff(store, &id)?;
    Ok(ReviewedInstallHandoff {
        directory: id,
        version: VERSION.into(),
        prior_desired,
        already_current,
        final_host: final_status.host,
        final_pid: final_status.pid,
    })
}

pub fn register_install(store: &Store) -> Result<()> {
    use sha2::{Digest, Sha256};
    let executable = std::env::current_exe().map_err(|_| "HOST_EXECUTABLE_UNAVAILABLE")?;
    let directory = executable.parent().ok_or("HOST_INSTALL_POINTER_INVALID")?;
    if directory
        .parent()
        .and_then(|p| std::fs::canonicalize(p).ok())
        != std::fs::canonicalize(store.root().join("versions")).ok()
    {
        return Err("HOST_NOT_INSTALLED".into());
    }
    let id = directory
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("HOST_INSTALL_POINTER_INVALID")?;
    let host_hash = format!(
        "{:x}",
        Sha256::digest(std::fs::read(&executable).map_err(|_| "HOST_EXECUTABLE_UNAVAILABLE")?)
    );
    let gui_hash = format!(
        "{:x}",
        Sha256::digest(
            std::fs::read(directory.join("CatDeskBinagotchy.exe"))
                .map_err(|_| "BINAGOTCHY_EXECUTABLE_UNAVAILABLE")?
        )
    );
    let adapter_hash = sha256_file(&directory.join("adapter.py"), "BROWSER_ADAPTER_UNAVAILABLE")?;
    let wake_bridge_hash = sha256_file(
        &directory.join("wake_bridge.py"),
        "BROWSER_PRIMITIVES_UNAVAILABLE",
    )?;
    let manifest: serde_json::Value = read(&directory.join("manifest.json"))?;
    for (name, actual) in [
        ("CatDeskWakeHost.exe", host_hash.as_str()),
        ("CatDeskBinagotchy.exe", gui_hash.as_str()),
        ("adapter.py", adapter_hash.as_str()),
        ("wake_bridge.py", wake_bridge_hash.as_str()),
    ] {
        if !actual.eq_ignore_ascii_case(manifest_hash(&manifest, name)?) {
            return Err("HOST_INSTALL_MANIFEST_HASH_MISMATCH".into());
        }
    }
    let path = store.root().join("current.json");
    if path.exists() {
        let previous: serde_json::Value = read(&path)?;
        atomic(&store.root().join("previous.json"), &previous)?;
    }
    atomic(
        &path,
        &serde_json::json!({"schemaVersion":1,"directory":id,"hostSha256":host_hash,"binagotchySha256":gui_hash,"adapterSha256":adapter_hash,"wakeBridgeSha256":wake_bridge_hash}),
    )
}

struct Adapter {
    _job: crate::process_job::Job,
    child: Child,
    input: ChildStdin,
    output: Receiver<Result<serde_json::Value>>,
}
impl Drop for Adapter {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Adapter {
    fn allow_graceful_exit(&mut self, grace: Duration) -> bool {
        let deadline = Instant::now() + grace;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        return false;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return false,
            }
        }
    }

    fn spawn(root: &Path) -> Result<Self> {
        let executable = std::env::current_exe().map_err(|_| "HOST_EXECUTABLE_UNAVAILABLE")?;
        let installed = executable.parent().ok_or("HOST_INSTALL_UNAVAILABLE")?;
        let (adapter_path, _wake_bridge_path) =
            verify_installed_browser_artifacts(root, installed)?;
        let interpreter = root.join("runtime/python.exe");
        if !interpreter.is_file() {
            return Err("BROWSER_RUNTIME_NOT_INSTALLED".into());
        }
        let browser_root = browser_process_root(root)?;
        let mut command = Command::new(interpreter);
        command
            .arg("-I")
            .arg(adapter_path)
            .arg(&browser_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(
                File::options()
                    .create(true)
                    .append(true)
                    .open(root.join("logs/adapter.log"))
                    .map_err(|_| "BROWSER_LOG_UNAVAILABLE")?,
            ));
        hidden(&mut command);
        let mut child = command
            .spawn()
            .map_err(|_| "BROWSER_ADAPTER_START_FAILED")?;
        let job = match crate::process_job::Job::assign(&child) {
            Ok(job) => job,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        let input = child.stdin.take().ok_or("BROWSER_ADAPTER_PIPE_FAILED")?;
        let output = child.stdout.take().ok_or("BROWSER_ADAPTER_PIPE_FAILED")?;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                // Bound a single response even if an adapter emits no newline.
                let result = read_line_bounded(&mut reader, &mut bytes);
                let terminal = result.is_err();
                if tx
                    .send(result.and_then(|_| {
                        serde_json::from_slice(&bytes)
                            .map_err(|_| "BROWSER_ADAPTER_RESPONSE_INVALID".into())
                    }))
                    .is_err()
                    || terminal
                {
                    break;
                }
            }
        });
        let mut adapter = Self {
            _job: job,
            child,
            input,
            output: rx,
        };
        adapter.send(serde_json::json!({"initialize":true}))?;
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if Instant::now() >= deadline {
                return Err("BROWSER_SESSION_START_TIMEOUT".into());
            }
            match adapter.output.recv_timeout(Duration::from_millis(250)) {
                Ok(Ok(value)) => match value.get("stage").and_then(|v| v.as_str()) {
                    Some("READY") => return Ok(adapter),
                    Some("ATTENTION") => {
                        let reason = value
                            .get("reason")
                            .and_then(|v| v.as_str())
                            .filter(|s| crate::protocol::valid_id(s))
                            .unwrap_or("BROWSER_SESSION_UNAVAILABLE")
                            .to_string();
                        return Err(reason);
                    }
                    _ => return Err("BROWSER_ADAPTER_RESPONSE_INVALID".into()),
                },
                Ok(Err(e)) => return Err(e),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err("BROWSER_ADAPTER_EXITED".into()),
            }
        }
    }
    fn send(&mut self, value: serde_json::Value) -> Result<()> {
        let mut bytes =
            serde_json::to_vec(&value).map_err(|_| "BROWSER_ADAPTER_REQUEST_INVALID")?;
        bytes.push(b'\n');
        self.input
            .write_all(&bytes)
            .and_then(|_| self.input.flush())
            .map_err(|_| "BROWSER_ADAPTER_PIPE_FAILED".into())
    }
}

fn retire_adapter(adapter: &mut Option<Adapter>) -> bool {
    let Some(mut current) = adapter.take() else {
        return true;
    };
    let exited_cleanly = current.allow_graceful_exit(Duration::from_secs(5));
    drop(current);
    if exited_cleanly {
        // Give Chrome/profile locks a short bounded settling interval after
        // SeleniumBase completed its own context cleanup.
        thread::sleep(Duration::from_millis(500));
    }
    exited_cleanly
}

fn read_line_bounded(reader: &mut impl BufRead, bytes: &mut Vec<u8>) -> Result<()> {
    use std::io::Read;
    let mut bounded = reader.take(8193);
    loop {
        let mut byte = [0];
        if bounded
            .read(&mut byte)
            .map_err(|_| "BROWSER_ADAPTER_PIPE_FAILED")?
            == 0
        {
            return Err("BROWSER_ADAPTER_EXITED".into());
        }
        if byte[0] == b'\n' {
            break;
        }
        bytes.push(byte[0]);
        if bytes.len() > 8192 {
            return Err("BROWSER_ADAPTER_RESPONSE_TOO_LARGE".into());
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewSource {
    pub schema_version: u32,
    pub project_id: String,
    pub workspace: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewRecord {
    schema_version: u32,
    record_id: String,
    project_id: String,
    session_id: String,
    state: String,
    next_action: String,
    reference: String,
    created_at_unix: u64,
    unread: bool,
}

pub fn set_review_source(store: &Store, workspace: &str) -> Result<()> {
    let canonical =
        std::fs::canonicalize(workspace).map_err(|_| "REVIEW_SOURCE_WORKSPACE_INVALID")?;
    if !canonical.is_dir() {
        return Err("REVIEW_SOURCE_WORKSPACE_INVALID".into());
    }
    let inbox = canonical
        .join(".catdesk")
        .join("autonomy")
        .join("review-inbox.json");
    if !inbox.parent().is_some_and(Path::is_dir) {
        return Err("REVIEW_SOURCE_INBOX_PARENT_INVALID".into());
    }
    atomic(
        &store.root().join("source.json"),
        &ReviewSource {
            schema_version: 1,
            project_id: "catdesk".into(),
            workspace: canonical.to_string_lossy().into_owned(),
        },
    )
}

fn review_source(store: &Store) -> Result<Option<ReviewSource>> {
    let path = store.root().join("source.json");
    if !path.try_exists().map_err(|_| "REVIEW_SOURCE_UNAVAILABLE")? {
        return Ok(None);
    }
    let source: ReviewSource = read(&path)?;
    if source.schema_version != 1 || source.project_id != "catdesk" {
        return Err("REVIEW_SOURCE_INVALID".into());
    }
    Ok(Some(source))
}

fn review_cutoff(store: &Store, activation: &Activation, project_id: &str) -> Result<u64> {
    let config = store.config()?;
    let target = config
        .targets
        .get(project_id)
        .ok_or("TARGET_NOT_CONFIGURED")?;
    let changed_utc = config
        .history
        .iter()
        .rev()
        .find(|change| change.project_id == project_id && change.target == *target)
        .map(|change| change.changed_utc)
        .ok_or("WAKE_TARGET_HISTORY_INVALID")?;
    Ok(activation.accept_after_utc.max(changed_utc))
}

fn discover_review_events(store: &Store, activation: &Activation) -> Result<usize> {
    let Some(source) = review_source(store)? else {
        return Ok(0);
    };
    let workspace =
        std::fs::canonicalize(&source.workspace).map_err(|_| "REVIEW_SOURCE_WORKSPACE_INVALID")?;
    let inbox = workspace
        .join(".catdesk")
        .join("autonomy")
        .join("review-inbox.json");
    let metadata = std::fs::metadata(&inbox).map_err(|_| "REVIEW_SOURCE_INBOX_UNAVAILABLE")?;
    if !metadata.is_file() || metadata.len() > 4 * 1024 * 1024 {
        return Err("REVIEW_SOURCE_INBOX_INVALID".into());
    }
    let records: Vec<ReviewRecord> = serde_json::from_slice(
        &std::fs::read(&inbox).map_err(|_| "REVIEW_SOURCE_INBOX_UNAVAILABLE")?,
    )
    .map_err(|_| "REVIEW_SOURCE_INBOX_INVALID")?;
    let cutoff = review_cutoff(store, activation, &source.project_id)?;
    let mut produced = 0usize;
    for record in records {
        if record.schema_version != 1
            || !record.unread
            || record.project_id != source.project_id
            || record.created_at_unix < cutoff
        {
            continue;
        }
        let event_type = match (record.state.as_str(), record.next_action.as_str()) {
            ("COMPLETED_VERIFIED", "independent_final_review") => "review_ready",
            ("WAITING_FOR_CHATGPT", "chatgpt_decision_required") => "engineering_attention",
            _ => continue,
        };
        let message = format!(
            "CatDesk Wake: {event_type} for CatDesk review record {}. Review the exact record and current wake/GUI repair plan through CatDesk. Ordinary autonomy remains frozen until wake acceptance completes.",
            record.record_id
        );
        store.produce(&record.record_id, &record.project_id, event_type, &message)?;
        produced += 1;
    }
    Ok(produced)
}

#[cfg(test)]
mod review_discovery_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn fixture() -> (PathBuf, PathBuf, Store, Activation, u64) {
        let base = std::env::temp_dir().join(format!(
            "catdesk-wake-review-discovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let root = base.join("wake");
        let workspace = base.join("workspace");
        // Production Store::open intentionally validates every ancestor from
        // the volume root. This feature-gated test fixture instead starts
        // beneath its already-existing canonical parent while retaining the
        // same descendant no-link/no-reparse checks.
        std::fs::create_dir_all(&base).expect("fixture parent");
        std::fs::create_dir_all(workspace.join(".catdesk/autonomy")).expect("inbox parent");
        let store = Store::open_scoped_for_test(&root, &base).expect("store");
        store.initialize().expect("initialize");
        let target = store
            .set_target("catdesk", 0, "https://chatgpt.com/c/current")
            .expect("target");
        let config = store.config().expect("config");
        let changed = config
            .history
            .iter()
            .find(|change| change.project_id == "catdesk" && change.target == target)
            .expect("history")
            .changed_utc;
        set_review_source(&store, workspace.to_str().expect("workspace text")).expect("source");
        let activation = Activation {
            schema_version: 1,
            owner: "CatDeskWake".into(),
            migration_audit: "migration.json".into(),
            accept_after_utc: 1,
        };
        (base, workspace, store, activation, changed)
    }

    #[test]
    fn standalone_discovery_queues_only_current_generation_actionable_reviews() {
        let (base, workspace, store, activation, changed) = fixture();
        let records = serde_json::json!([
            {
                "schemaVersion":1,
                "recordId":"review-old",
                "projectId":"catdesk",
                "sessionId":"session-old",
                "state":"COMPLETED_VERIFIED",
                "nextAction":"independent_final_review",
                "reference":"artifacts/completion.json",
                "createdAtUnix":changed.saturating_sub(1),
                "unread":true
            },
            {
                "schemaVersion":1,
                "recordId":"review-current",
                "projectId":"catdesk",
                "sessionId":"session-current",
                "state":"COMPLETED_VERIFIED",
                "nextAction":"independent_final_review",
                "reference":"artifacts/completion.json",
                "createdAtUnix":changed + 1,
                "unread":true
            },
            {
                "schemaVersion":1,
                "recordId":"review-acked",
                "projectId":"catdesk",
                "sessionId":"session-acked",
                "state":"COMPLETED_VERIFIED",
                "nextAction":"independent_final_review",
                "reference":"artifacts/completion.json",
                "createdAtUnix":changed + 2,
                "unread":false
            }
        ]);
        std::fs::write(
            workspace.join(".catdesk/autonomy/review-inbox.json"),
            serde_json::to_vec_pretty(&records).expect("records"),
        )
        .expect("write inbox");

        assert_eq!(
            discover_review_events(&store, &activation).expect("discover"),
            1
        );
        let events = store.events().expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, "review-current");
        assert_eq!(events[0].event_type, "review_ready");

        assert_eq!(
            discover_review_events(&store, &activation).expect("replay"),
            1
        );
        assert_eq!(store.events().expect("replayed events").len(), 1);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn startup_retries_only_receiptless_login_attention_once_back_to_claimed() {
        let (base, _workspace, store, _activation, _changed) = fixture();
        let event = store
            .produce("review-login", "catdesk", "review_ready", "fixed message")
            .expect("produce");
        {
            let lease = store.host_lock().expect("lease");
            store.claim(&event, &lease).expect("claim");
            store
                .transition(
                    &event.event_id,
                    lease.owner(),
                    Phase::Attention,
                    None,
                    Some("LOGIN_OR_PROFILE_REQUIRED".into()),
                )
                .expect("attention");
        }
        retry_safe_startup_attention(&store).expect("startup retry");
        let delivery = store
            .delivery(&event.event_id)
            .expect("delivery read")
            .expect("delivery");
        assert_eq!(delivery.phase, Phase::Claimed);
        assert!(delivery.reason.is_none());
        assert!(delivery.receipt.is_none());

        let other = store
            .produce("review-other", "catdesk", "review_ready", "fixed other")
            .expect("produce other");
        {
            let lease = store.host_lock().expect("lease other");
            store.claim(&other, &lease).expect("claim other");
            store
                .transition(
                    &other.event_id,
                    lease.owner(),
                    Phase::Attention,
                    None,
                    Some("CAPTCHA_OR_SECURITY_CHECK".into()),
                )
                .expect("other attention");
        }
        retry_safe_startup_attention(&store).expect("startup retry other");
        let other_delivery = store
            .delivery(&other.event_id)
            .expect("other delivery read")
            .expect("other delivery");
        assert_eq!(other_delivery.phase, Phase::Attention);
        assert_eq!(
            other_delivery.reason.as_deref(),
            Some("CAPTCHA_OR_SECURITY_CHECK")
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn startup_does_not_rearm_receiptless_target_drift_without_explicit_retry() {
        let (base, _workspace, store, _activation, _changed) = fixture();
        let event = store
            .produce(
                "review-target-drift",
                "catdesk",
                "review_ready",
                "fixed message",
            )
            .expect("produce");
        {
            let lease = store.host_lock().expect("lease");
            store.claim(&event, &lease).expect("claim");
            store
                .transition(
                    &event.event_id,
                    lease.owner(),
                    Phase::Attention,
                    None,
                    Some("TARGET_DRIFT".into()),
                )
                .expect("attention");
        }

        retry_safe_startup_attention(&store).expect("startup inspection");
        let delivery = store
            .delivery(&event.event_id)
            .expect("delivery read")
            .expect("delivery");
        assert_eq!(delivery.phase, Phase::Attention);
        assert_eq!(delivery.reason.as_deref(), Some("TARGET_DRIFT"));
        assert!(delivery.receipt.is_none());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn startup_retry_ignores_safe_attention_from_stale_target_generation() {
        let (base, _workspace, store, _activation, _changed) = fixture();
        let event = store
            .produce(
                "review-stale-login",
                "catdesk",
                "review_ready",
                "fixed message",
            )
            .expect("produce");
        {
            let lease = store.host_lock().expect("lease");
            store.claim(&event, &lease).expect("claim");
            store
                .transition(
                    &event.event_id,
                    lease.owner(),
                    Phase::Attention,
                    None,
                    Some("LOGIN_OR_PROFILE_REQUIRED".into()),
                )
                .expect("attention");
        }
        store
            .set_target(
                "catdesk",
                event.target_generation,
                "https://chatgpt.com/c/new-generation",
            )
            .expect("move target generation");

        retry_safe_startup_attention(&store).expect("stale attention must not abort startup");
        let delivery = store
            .delivery(&event.event_id)
            .expect("delivery read")
            .expect("delivery");
        assert_eq!(delivery.phase, Phase::Attention);
        assert_eq!(
            delivery.reason.as_deref(),
            Some("LOGIN_OR_PROFILE_REQUIRED")
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn chatgpt_not_idle_attention_rearms_after_delay_within_bounded_window() {
        let (base, _workspace, store, _activation, _changed) = fixture();
        let event = store
            .produce(
                "review-not-idle",
                "catdesk",
                "review_ready",
                "fixed message",
            )
            .expect("produce");
        {
            let lease = store.host_lock().expect("lease");
            store.claim(&event, &lease).expect("claim");
            store
                .transition(
                    &event.event_id,
                    lease.owner(),
                    Phase::Attention,
                    None,
                    Some("CHATGPT_NOT_IDLE".into()),
                )
                .expect("attention");
        }
        let attention = store
            .delivery(&event.event_id)
            .expect("delivery read")
            .expect("delivery");
        assert!(
            !maybe_retry_chatgpt_not_idle(
                &store,
                &event,
                attention.updated_utc + CHATGPT_NOT_IDLE_RETRY_DELAY_SECONDS - 1,
            )
            .expect("not yet due")
        );
        assert_eq!(
            store
                .delivery(&event.event_id)
                .expect("delivery read")
                .expect("delivery")
                .phase,
            Phase::Attention
        );

        assert!(
            maybe_retry_chatgpt_not_idle(
                &store,
                &event,
                attention.updated_utc + CHATGPT_NOT_IDLE_RETRY_DELAY_SECONDS,
            )
            .expect("due retry")
        );
        let rearmed = store
            .delivery(&event.event_id)
            .expect("delivery read")
            .expect("delivery");
        assert_eq!(rearmed.phase, Phase::Claimed);
        assert!(rearmed.reason.is_none());
        assert!(rearmed.receipt.is_none());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn chatgpt_not_idle_attention_expires_without_replay_after_bounded_window() {
        let (base, _workspace, store, _activation, _changed) = fixture();
        let event = store
            .produce(
                "review-not-idle-expired",
                "catdesk",
                "review_ready",
                "fixed message",
            )
            .expect("produce");
        {
            let lease = store.host_lock().expect("lease");
            store.claim(&event, &lease).expect("claim");
            store
                .transition(
                    &event.event_id,
                    lease.owner(),
                    Phase::Attention,
                    None,
                    Some("CHATGPT_NOT_IDLE".into()),
                )
                .expect("attention");
        }
        assert!(
            !maybe_retry_chatgpt_not_idle(
                &store,
                &event,
                event.created_utc + CHATGPT_NOT_IDLE_RETRY_WINDOW_SECONDS + 1,
            )
            .expect("expired retry")
        );
        let delivery = store
            .delivery(&event.event_id)
            .expect("delivery read")
            .expect("delivery");
        assert_eq!(delivery.phase, Phase::Attention);
        assert_eq!(delivery.reason.as_deref(), Some("CHATGPT_NOT_IDLE"));
        assert!(delivery.receipt.is_none());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn browser_session_retries_are_bounded_and_pre_submit_only() {
        // TARGET_DRIFT already exhausted the adapter's bounded exact-target
        // readiness/recovery windows. A fresh browser session would multiply
        // that budget and create visible reopen churn without new evidence.
        assert!(!retryable_pre_submit_browser_failure(
            "TARGET_DRIFT",
            &Phase::Claimed,
            1
        ));
        assert!(retryable_pre_submit_browser_failure(
            "BROWSER_NETWORK_ERROR",
            &Phase::Claimed,
            1
        ));
        assert!(retryable_pre_submit_browser_failure(
            "BROWSER_NETWORK_ERROR",
            &Phase::Claimed,
            2
        ));
        assert!(retryable_pre_submit_browser_failure(
            "BROWSER_ATTEMPT_TIMEOUT",
            &Phase::Claimed,
            1
        ));
        assert!(retryable_pre_submit_browser_failure(
            "BROWSER_ATTEMPT_TIMEOUT",
            &Phase::Claimed,
            2
        ));
        assert!(!retryable_pre_submit_browser_failure(
            "BROWSER_ATTEMPT_TIMEOUT",
            &Phase::Claimed,
            3
        ));
        assert!(!retryable_pre_submit_browser_failure(
            "LOGIN_OR_PROFILE_REQUIRED",
            &Phase::Claimed,
            1
        ));
        assert!(!retryable_pre_submit_browser_failure(
            "TARGET_DRIFT",
            &Phase::Submitting,
            1
        ));
    }

    #[test]
    fn submitting_reconciliation_retries_are_bounded_delayed_and_never_replay() {
        let (base, _workspace, store, _activation, _changed) = fixture();
        let event = store
            .produce(
                "review-submitting-reconcile-retry",
                "catdesk",
                "review_ready",
                "receipt retry fixture",
            )
            .expect("produce");
        let lease = store.host_lock().expect("lease");
        let claimed = store.claim(&event, &lease).expect("claim");
        let submitting = store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Submitting,
                None,
                None,
            )
            .expect("submitting");

        assert!(submitting_reconciliation_due(
            &submitting,
            0,
            submitting.updated_utc
        ));

        let transient = store
            .note_submitting_attention(&event.event_id, "SUBMIT_CLEARED_NO_APPEND")
            .expect("transient attention");
        assert!(!submitting_reconciliation_due(
            &transient,
            1,
            transient.updated_utc + 29
        ));
        assert!(submitting_reconciliation_due(
            &transient,
            1,
            transient.updated_utc + 30
        ));
        assert!(submitting_reconciliation_due(
            &transient,
            2,
            transient.updated_utc + 120
        ));
        assert!(submitting_reconciliation_due(
            &transient,
            3,
            transient.updated_utc + 300
        ));
        assert!(!submitting_reconciliation_due(
            &transient,
            SUBMITTING_RECONCILIATION_ATTEMPTS,
            transient.updated_utc + 3600
        ));
        assert!(!submitting_reconciliation_due(
            &transient,
            0,
            transient.event.created_utc + SUBMITTING_RECONCILIATION_RETRY_WINDOW_SECONDS + 1
        ));

        let contradictory = store
            .note_submitting_attention(&event.event_id, "SUBMIT_TARGET_DRIFT")
            .expect("contradictory attention");
        assert!(!submitting_reconciliation_due(
            &contradictory,
            1,
            contradictory.updated_utc + 3600
        ));

        assert_eq!(
            store.claim(&event, &lease).unwrap_err(),
            "SUBMISSION_RECONCILIATION_REQUIRED"
        );
        assert_eq!(
            store.retry_pre_submit(&event.event_id).unwrap_err(),
            "SUBMISSION_RETRY_REFUSED"
        );
        let persisted = store
            .delivery(&event.event_id)
            .expect("delivery read")
            .expect("delivery");
        assert_eq!(persisted.phase, Phase::Submitting);
        assert!(persisted.receipt.is_none());
        assert_eq!(persisted.message_digest, claimed.message_digest);

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn source_binding_survives_host_package_changes_and_is_not_in_install_directory() {
        let (base, workspace, store, _activation, _changed) = fixture();
        let source: ReviewSource = read(&store.root().join("source.json")).expect("source");
        assert_eq!(source.schema_version, 1);
        assert_eq!(source.project_id, "catdesk");
        assert_eq!(
            std::fs::canonicalize(source.workspace).expect("canonical source"),
            std::fs::canonicalize(workspace).expect("canonical workspace")
        );
        assert!(store.root().join("source.json").is_file());
        let _ = std::fs::remove_dir_all(base);
    }

    fn sent_timer(store: &Store, id: &str) -> crate::store::Delivery {
        let event = store
            .produce(id, "catdesk", "review_ready", "turn timer fixture")
            .expect("produce");
        let lease = store.host_lock().expect("lease");
        let claimed = store.claim(&event, &lease).expect("claim");
        store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Submitting,
                None,
                None,
            )
            .expect("submitting");
        let receipt = Receipt {
            schema_version: 1,
            event_id: event.event_id.clone(),
            target_generation: claimed.target.generation,
            target_digest: claimed.target.digest,
            message_digest: claimed.message_digest,
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        store
            .record_exact_receipt_timer(&event.event_id, &receipt)
            .expect("timer starts at user receipt");
        let delivery = store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Sent,
                Some(receipt),
                None,
            )
            .expect("sent");
        store
            .complete_timer(&event.event_id)
            .expect("timer complete");
        delivery
    }

    #[test]
    fn passive_turn_timer_reports_elapsed_thresholds_and_never_emits_work() {
        let (base, _workspace, store, _activation, _changed) = fixture();
        let sent = sent_timer(&store, "review-turn-timer");
        assert!(store.events().expect("events").is_empty());
        let receipt = sent.receipt.as_ref().expect("receipt");
        let timer = store
            .timer(&sent.event.event_id)
            .expect("timer")
            .expect("present");
        assert_eq!(timer.started_utc, receipt.sent_utc);
        assert_eq!(timer.response_state, ResponseState::Complete);

        let state = status(&store).expect("status");
        let entry = state
            .turn_timers
            .iter()
            .find(|entry| entry.event_id == sent.event.event_id)
            .expect("timer status");
        assert_eq!(entry.started_utc, receipt.sent_utc);
        assert_eq!(entry.response_state, ResponseState::Complete);
        assert!(entry.elapsed_seconds <= TURN_TARGET_SECONDS);
        assert!(!entry.deadline_reached);
        assert!(store.events().expect("events").is_empty());

        store
            .set_target("catdesk", 1, "https://chatgpt.com/c/next")
            .expect("advance target generation");
        let advanced = status(&store).expect("advanced generation status");
        assert!(
            advanced
                .turn_timers
                .iter()
                .all(|timer| timer.event_id != sent.event.event_id),
            "historical-generation timers stay durable but leave the live surface"
        );

        let active = TurnTimer {
            schema_version: 1,
            event_id: "review-clock".into(),
            project_id: "catdesk".into(),
            target_generation: 1,
            target_digest: "a".repeat(64),
            started_utc: 100,
            completed_utc: None,
            response_state: ResponseState::Observing,
        };
        let before_soft = turn_timer_status(active.clone(), 100 + 17 * 60 + 59);
        assert!(!before_soft.soft_checkpoint_reached);
        assert!(!before_soft.deadline_reached);
        assert_eq!(before_soft.remaining_seconds, 2 * 60 + 1);

        let soft = turn_timer_status(active.clone(), 100 + 18 * 60);
        assert!(soft.soft_checkpoint_reached);
        assert!(!soft.deadline_reached);
        assert_eq!(soft.remaining_seconds, 2 * 60);

        let deadline = turn_timer_status(active.clone(), 100 + 20 * 60);
        assert!(deadline.soft_checkpoint_reached);
        assert!(deadline.deadline_reached);
        assert_eq!(deadline.remaining_seconds, 0);

        let completed = turn_timer_status(
            TurnTimer {
                completed_utc: Some(100 + 7 * 60),
                response_state: ResponseState::Complete,
                ..active
            },
            100 + 60 * 60,
        );
        assert_eq!(completed.elapsed_seconds, 7 * 60);
        assert_eq!(completed.remaining_seconds, 13 * 60);
        assert_eq!(completed.response_state, ResponseState::Complete);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn active_status_persistence_refreshes_new_exact_receipt_timer() {
        let (base, _workspace, store, _activation, _changed) = fixture();
        let mut live = status(&store).expect("initial status");
        assert!(live.turn_timers.is_empty());
        sent_timer(&store, "aaa-historical-complete");

        let event = store
            .produce(
                "review-live-timer-refresh",
                "catdesk",
                "review_ready",
                "timer refresh fixture",
            )
            .expect("produce");
        let lease = store.host_lock().expect("lease");
        let claimed = store.claim(&event, &lease).expect("claim");
        store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Submitting,
                None,
                None,
            )
            .expect("submitting");
        let receipt = Receipt {
            schema_version: 1,
            event_id: event.event_id.clone(),
            target_generation: claimed.target.generation,
            target_digest: claimed.target.digest,
            message_digest: claimed.message_digest,
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        store
            .record_exact_receipt_timer(&event.event_id, &receipt)
            .expect("durable timer");

        // `live` predates the receipt, matching attempt/reconcile ownership of a
        // stale in-memory Status. Persisting it must re-read durable timers.
        persist_live_status(&store, &mut live).expect("persist refreshed status");
        let refreshed = live
            .turn_timers
            .iter()
            .find(|entry| entry.event_id == event.event_id)
            .expect("new timer visible during active attempt");
        assert_eq!(refreshed.started_utc, receipt.sent_utc);
        assert_eq!(refreshed.response_state, ResponseState::Observing);
        assert_eq!(live.turn_timers[0].event_id, event.event_id);
        assert!(
            live.turn_timers
                .iter()
                .any(|timer| timer.event_id == "aaa-historical-complete")
        );
        assert_eq!(live.queue_depth, 0);
        let persisted: Status = read(&store.root().join("status.json")).expect("persisted status");
        assert!(
            persisted
                .turn_timers
                .iter()
                .any(|entry| entry.event_id == event.event_id)
        );
        // A receipted SUBMITTING event is reconciliation-only, never USER-deliverable.
        assert_eq!(persisted.queue_depth, 0);

        store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Sent,
                Some(receipt.clone()),
                None,
            )
            .expect("sent");
        store
            .complete_timer(&event.event_id)
            .expect("timer complete");
        persist_live_status(&store, &mut live).expect("persist terminal status");
        assert_eq!(live.queue_depth, 0);
        assert_eq!(
            live.turn_timers
                .iter()
                .find(|entry| entry.event_id == event.event_id)
                .expect("completed timer")
                .response_state,
            ResponseState::Complete
        );
        let terminal: Status = read(&store.root().join("status.json")).expect("terminal status");
        assert_eq!(terminal.queue_depth, 0);
        drop(lease);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn last_failed_event_status_is_rebuilt_from_its_own_durable_delivery() {
        let (base, _workspace, store, _activation, _changed) = fixture();
        let event = store
            .produce("manual-current-failure", "catdesk", "test", "MANUAL DEBUG")
            .unwrap();
        let lease = store.host_lock().unwrap();
        store.claim(&event, &lease).unwrap();
        store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Attention,
                None,
                Some("TARGET_DRIFT".into()),
            )
            .unwrap();
        let mut stale = status(&store).unwrap();
        stale.last_event_id = Some(event.event_id.clone());
        stale.submission = "SUBMITTING".into();
        stale.attention = Some("RESPONSE_COMPLETION_UNPROVEN".into());
        atomic(&store.root().join("status.json"), &stale).unwrap();
        let observed = status(&store).unwrap();
        assert_eq!(
            observed.last_event_id.as_deref(),
            Some(event.event_id.as_str())
        );
        assert_eq!(observed.submission, "ATTENTION");
        assert_eq!(observed.attention.as_deref(), Some("TARGET_DRIFT"));
        assert_eq!(observed.queue_depth, 0);
        let health = queue_health(&store).unwrap();
        assert_eq!(health.actionable, 0);
        assert_eq!(health.attention, 1);
        assert_eq!(health.reconciliation, 0);
        assert_eq!(health.stale, 0);
        assert_eq!(health.forensic, 1);
        assert!(store.timer(&event.event_id).unwrap().is_none());
        assert!(
            store
                .delivery(&event.event_id)
                .unwrap()
                .unwrap()
                .receipt
                .is_none()
        );
        drop(lease);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn readiness_protocol_accepts_loading_and_blank_shell_but_rejects_arbitrary_content() {
        for reason in [
            "DOCUMENT_LOADING",
            "BLANK_SHELL_RECOVERY",
            "READY",
            "RECOVERY_RETURN",
        ] {
            let value = serde_json::json!({"route":"SAME_CONVERSATION", "reason":reason});
            assert_eq!(
                readiness_observation(&value).unwrap(),
                ("SAME_CONVERSATION", reason)
            );
        }
        for value in [
            serde_json::json!({"route":"SAME_CONVERSATION", "reason":"page content"}),
            serde_json::json!({"route":"https://example.test/", "reason":"READY"}),
            serde_json::json!({"route":"SAME_CONVERSATION"}),
            serde_json::json!({"route":null,"reason":"READY"}),
        ] {
            assert_eq!(
                readiness_observation(&value).unwrap_err(),
                "BROWSER_ADAPTER_RESPONSE_INVALID"
            );
        }
    }

    #[test]
    fn readiness_history_is_bounded_validated_and_separate_from_status_contract() {
        let (base, _workspace, store, _activation, _changed) = fixture();
        let mut live = status(&store).unwrap();
        live.last_event_id = Some("diagnostic-event".into());
        for index in 0..100 {
            let reason = if index % 2 == 0 {
                "NETWORK_ERR_CONNECTION_RESET"
            } else {
                "RECOVERY_BEGIN"
            };
            let value = serde_json::json!({"route":"CHROME_ERROR", "reason":reason});
            record_readiness_observation(&store, &live, &value).unwrap();
            record_readiness_observation(&store, &live, &value).unwrap();
        }
        let history = readiness_history(&store).unwrap();
        assert_eq!(history.len(), READINESS_HISTORY_LIMIT);
        assert!(
            history
                .iter()
                .all(|row| row.event_id.as_deref() == Some("diagnostic-event"))
        );
        assert!(
            record_readiness_observation(
                &store,
                &live,
                &serde_json::json!({"route":"CHROME_ERROR","reason":"NETWORK_secret"})
            )
            .is_err()
        );

        atomic(&store.root().join("status.json"), &live).unwrap();
        let raw_status = std::fs::read_to_string(store.root().join("status.json")).unwrap();
        assert!(!raw_status.contains("readinessHistory"));
        let observed = status(&store).unwrap();
        assert_eq!(observed.last_event_id.as_deref(), Some("diagnostic-event"));
        assert_eq!(
            readiness_history(&store).unwrap().last().unwrap().reason,
            "RECOVERY_BEGIN"
        );
        assert!(store.delivery("diagnostic-event").unwrap().is_none());
        assert!(store.timer("diagnostic-event").unwrap().is_none());
        let _ = std::fs::remove_dir_all(base);
    }
}

const PRE_SUBMIT_BROWSER_SESSION_ATTEMPTS: usize = 3;
const SUBMITTING_RECONCILIATION_ATTEMPTS: usize = 4;
const SUBMITTING_RECONCILIATION_RETRY_DELAYS_SECONDS: [u64; SUBMITTING_RECONCILIATION_ATTEMPTS] =
    [0, 30, 120, 300];
const SUBMITTING_RECONCILIATION_RETRY_WINDOW_SECONDS: u64 = 30 * 60;
const CHATGPT_NOT_IDLE_RETRY_DELAY_SECONDS: u64 = 30;
const CHATGPT_NOT_IDLE_RETRY_WINDOW_SECONDS: u64 = 6 * 60 * 60;
const TURN_SOFT_CHECKPOINT_SECONDS: u64 = 18 * 60;
const TURN_TARGET_SECONDS: u64 = 20 * 60;

fn retryable_submitting_reconciliation_reason(reason: Option<&str>) -> bool {
    matches!(
        reason,
        Some(
            "SUBMIT_CLICK_UNKNOWN"
                | "SUBMIT_ENTER_UNKNOWN"
                | "SUBMIT_COMPOSER_RETAINED"
                | "SUBMIT_COMPOSER_CHANGED"
                | "SUBMIT_CLEARED_NO_APPEND"
                | "SUBMIT_RECEIPT_QUERY_FAILED"
                | "SUBMIT_RECEIPT_ROUND_TRIP_FAILED"
                | "SUBMIT_RECEIPT_UNPROVEN"
                | "SUBMISSION_RECONCILIATION_TIMEOUT"
                | "BROWSER_NETWORK_ERROR"
                | "BROWSER_ADAPTER_EXITED"
        )
    )
}

fn submitting_reconciliation_due(
    delivery: &crate::store::Delivery,
    attempts: usize,
    now_utc: u64,
) -> bool {
    if delivery.phase != Phase::Submitting
        || delivery.receipt.is_some()
        || attempts >= SUBMITTING_RECONCILIATION_ATTEMPTS
        || now_utc.saturating_sub(delivery.event.created_utc)
            > SUBMITTING_RECONCILIATION_RETRY_WINDOW_SECONDS
    {
        return false;
    }
    if attempts == 0 {
        return true;
    }
    retryable_submitting_reconciliation_reason(delivery.reason.as_deref())
        && now_utc.saturating_sub(delivery.updated_utc)
            >= SUBMITTING_RECONCILIATION_RETRY_DELAYS_SECONDS[attempts]
}

fn readiness_observation(value: &serde_json::Value) -> Result<(&str, &str)> {
    let route = value
        .get("route")
        .and_then(|v| v.as_str())
        .ok_or("BROWSER_ADAPTER_RESPONSE_INVALID")?;
    let reason = value
        .get("reason")
        .and_then(|v| v.as_str())
        .ok_or("BROWSER_ADAPTER_RESPONSE_INVALID")?;
    if !matches!(
        route,
        "SAME_CONVERSATION"
            | "DIFFERENT_CONVERSATION"
            | "CHROME_ERROR"
            | "AUTH"
            | "HOME"
            | "PROJECT_OTHER"
            | "OTHER_ROUTE"
            | "OTHER_HOST"
            | "INVALID_ROUTE"
    ) || !matches!(
        reason,
        "READY"
            | "DOCUMENT_LOADING"
            | "EDITOR_SELECTOR"
            | "CAPTCHA_OR_SECURITY_CHECK"
            | "BROWSER_NETWORK_ERROR"
            | "LOGIN_OR_PROFILE_REQUIRED"
            | "TARGET_DRIFT"
            | "RECOVERY_BEGIN"
            | "RECOVERY_RETURN"
            | "BLANK_SHELL_RECOVERY"
            | "NETWORK_UNKNOWN"
            | "NETWORK_ERR_NETWORK_CHANGED"
            | "NETWORK_ERR_INTERNET_DISCONNECTED"
            | "NETWORK_ERR_NAME_NOT_RESOLVED"
            | "NETWORK_ERR_CONNECTION_RESET"
            | "NETWORK_ERR_CONNECTION_CLOSED"
            | "NETWORK_ERR_CONNECTION_REFUSED"
            | "NETWORK_ERR_CONNECTION_TIMED_OUT"
            | "NETWORK_ERR_TIMED_OUT"
            | "NETWORK_ERR_TUNNEL_CONNECTION_FAILED"
            | "NETWORK_ERR_PROXY_CONNECTION_FAILED"
            | "NETWORK_ERR_HTTP2_PROTOCOL_ERROR"
            | "NETWORK_ERR_QUIC_PROTOCOL_ERROR"
            | "NETWORK_ERR_SSL_PROTOCOL_ERROR"
            | "NETWORK_ERR_CERT_AUTHORITY_INVALID"
            | "NETWORK_ERR_BLOCKED_BY_CLIENT"
    ) {
        return Err("BROWSER_ADAPTER_RESPONSE_INVALID".into());
    }
    Ok((route, reason))
}

const READINESS_HISTORY_LIMIT: usize = 64;

fn readiness_history_path(store: &Store) -> PathBuf {
    store.root().join("readiness-history.json")
}

pub fn readiness_history(store: &Store) -> Result<Vec<ReadinessStatus>> {
    let path = readiness_history_path(store);
    if !path
        .try_exists()
        .map_err(|_| "READINESS_HISTORY_UNAVAILABLE")?
    {
        return Ok(vec![]);
    }
    let journal: ReadinessJournal = read(&path)?;
    if journal.schema_version != 1 || journal.entries.len() > READINESS_HISTORY_LIMIT {
        return Err("READINESS_HISTORY_INVALID".into());
    }
    for row in &journal.entries {
        if row.observed_utc == 0
            || row
                .event_id
                .as_ref()
                .is_some_and(|id| !crate::protocol::valid_id(id))
            || readiness_observation(&serde_json::json!({
                "route": row.route,
                "reason": row.reason,
            }))
            .is_err()
        {
            return Err("READINESS_HISTORY_INVALID".into());
        }
    }
    Ok(journal.entries)
}

fn record_readiness_observation<'a>(
    store: &Store,
    status: &Status,
    value: &'a serde_json::Value,
) -> Result<(&'a str, &'a str)> {
    let (route, reason) = readiness_observation(value)?;
    let mut entries = readiness_history(store)?;
    let duplicate = entries.last().is_some_and(|prior| {
        prior.event_id == status.last_event_id && prior.route == route && prior.reason == reason
    });
    if !duplicate {
        entries.push(ReadinessStatus {
            observed_utc: now(),
            event_id: status.last_event_id.clone(),
            route: route.into(),
            reason: reason.into(),
        });
    }
    let excess = entries.len().saturating_sub(READINESS_HISTORY_LIMIT);
    entries.drain(..excess);
    atomic(
        &readiness_history_path(store),
        &ReadinessJournal {
            schema_version: 1,
            entries,
        },
    )?;
    Ok((route, reason))
}

fn record_response_stage(store: &Store, event_id: &str, stage: &str) -> Result<()> {
    let state = match stage {
        "GENERATION_WAIT" | "GENERATION_ACTIVE" | "RESPONSE_RETRY_STARTED" => {
            ResponseState::Observing
        }
        "RESPONSE_TIMEOUT_DETECTED" | "RESPONSE_RETRYING" => ResponseState::Retrying,
        "RESPONSE_COMPLETED" | "RESPONSE_COMPLETED_AFTER_REOPEN" => ResponseState::Observing,
        _ => return Ok(()),
    };
    store.set_timer_response_state(event_id, state)
}

fn retryable_pre_submit_browser_failure(reason: &str, phase: &Phase, attempt: usize) -> bool {
    *phase == Phase::Claimed
        && attempt < PRE_SUBMIT_BROWSER_SESSION_ATTEMPTS
        && matches!(reason, "BROWSER_NETWORK_ERROR" | "BROWSER_ATTEMPT_TIMEOUT")
}

fn maybe_retry_chatgpt_not_idle(
    store: &Store,
    event: &crate::protocol::Event,
    now_utc: u64,
) -> Result<bool> {
    let Some(delivery) = store.delivery(&event.event_id)? else {
        return Ok(false);
    };
    if delivery.phase != Phase::Attention
        || delivery.receipt.is_some()
        || delivery.reason.as_deref() != Some("CHATGPT_NOT_IDLE")
        || now_utc.saturating_sub(delivery.updated_utc) < CHATGPT_NOT_IDLE_RETRY_DELAY_SECONDS
        || now_utc.saturating_sub(event.created_utc) > CHATGPT_NOT_IDLE_RETRY_WINDOW_SECONDS
    {
        return Ok(false);
    }
    store.retry_pre_submit(&event.event_id)?;
    Ok(true)
}

fn retry_safe_startup_attention(store: &Store) -> Result<()> {
    let config = store.config()?;
    for event in store.events()? {
        if config
            .targets
            .get(&event.project_id)
            .is_none_or(|target| target.generation != event.target_generation)
        {
            continue;
        }
        let Some(delivery) = store.delivery(&event.event_id)? else {
            continue;
        };
        if delivery.phase == Phase::Attention
            && delivery.receipt.is_none()
            && delivery.reason.as_deref() == Some("LOGIN_OR_PROFILE_REQUIRED")
        {
            store.retry_pre_submit(&event.event_id)?;
        }
    }
    Ok(())
}

const HOST_RECOVERY_RESET_WINDOW_SECONDS: u64 = 300;
const HOST_RECOVERY_MAX_BACKOFF_SECONDS: u64 = 60;
const HOST_RECOVERY_MAX_ATTEMPTS: u32 = 8;

fn recoverable_host_runtime_error(reason: &str) -> bool {
    matches!(
        reason,
        "CONFIG_LOCK_UNAVAILABLE"
            | "CONFIG_UNAVAILABLE"
            | "QUEUE_UNAVAILABLE"
            | "RECEIPT_UNAVAILABLE"
            | "TURN_TIMER_UNAVAILABLE"
            | "READINESS_HISTORY_UNAVAILABLE"
            | "REVIEW_SOURCE_UNAVAILABLE"
            | "REVIEW_SOURCE_INBOX_UNAVAILABLE"
            | "STATE_UNAVAILABLE"
            | "STATE_WRITE_FAILED"
            | "STATE_REPLACE_FAILED"
            | "STATE_SYNC_FAILED"
    )
}

fn host_recovery_backoff(consecutive_failures: u32) -> Duration {
    let shift = consecutive_failures.saturating_sub(1).min(6);
    Duration::from_secs((1u64 << shift).min(HOST_RECOVERY_MAX_BACKOFF_SECONDS))
}

fn host_recovery_exhausted(consecutive_failures: u32) -> bool {
    consecutive_failures > HOST_RECOVERY_MAX_ATTEMPTS
}

fn reviewed_install_handoff_active(store: &Store) -> Result<bool> {
    reviewed_install_transaction_path(store)
        .try_exists()
        .map_err(|_| "HOST_INSTALL_HANDOFF_STATE_UNAVAILABLE".into())
}

fn verify_current_host_authority(store: &Store) -> Result<()> {
    let pointer: serde_json::Value = read(&store.root().join("current.json"))?;
    let id = install_id(&pointer)?;
    let expected_directory = std::fs::canonicalize(store.root().join("versions").join(id))
        .map_err(|_| "HOST_INSTALL_UNAVAILABLE".to_string())?;
    let executable =
        std::env::current_exe().map_err(|_| "HOST_EXECUTABLE_UNAVAILABLE".to_string())?;
    let actual_directory =
        std::fs::canonicalize(executable.parent().ok_or("HOST_INSTALL_POINTER_INVALID")?)
            .map_err(|_| "HOST_INSTALL_UNAVAILABLE".to_string())?;
    if actual_directory != expected_directory {
        return Err("HOST_INSTALL_POINTER_MISMATCH".into());
    }
    let host_hash = sha256_file(&executable, "HOST_EXECUTABLE_UNAVAILABLE")?;
    if !host_hash.eq_ignore_ascii_case(pointer_hash(&pointer, "hostSha256")?) {
        return Err("HOST_EXECUTABLE_HASH_MISMATCH".into());
    }
    verify_installed_browser_artifacts(store.root(), &actual_directory)?;
    Ok(())
}

fn persist_host_runtime_failure(
    store: &Store,
    reason: &str,
    recovering: bool,
    recovery_increment: u64,
    error_utc: u64,
) -> Result<()> {
    let mut live = status(store)?;
    live.version = VERSION.into();
    live.pid = std::process::id();
    live.host = if recovering { "RUNNING" } else { "ERROR" }.into();
    live.attention = Some(
        if recovering {
            "HOST_RUNTIME_RECOVERING"
        } else {
            "HOST_RUNTIME_FATAL"
        }
        .into(),
    );
    live.host_recovery_count = live.host_recovery_count.saturating_add(recovery_increment);
    live.last_host_error = Some(reason.into());
    live.last_host_error_utc = Some(error_utc);
    persist_live_status(store, &mut live)
}

pub fn run(store: &Store) -> Result<()> {
    let lease = store.host_lock()?;
    run_with_lease(store, &lease)
}

pub fn run_resilient(store: &Store) -> Result<()> {
    let lease = store.host_lock()?;
    let mut consecutive_failures = 0u32;
    let mut last_failure_utc = None::<u64>;
    let mut pending_recovery_count = 0u64;
    let mut pending_error = None::<(String, u64)>;

    loop {
        if pending_recovery_count > 0
            && let Some((reason, error_utc)) = pending_error.as_ref()
            && persist_host_runtime_failure(store, reason, true, pending_recovery_count, *error_utc)
                .is_ok()
        {
            pending_recovery_count = 0;
            let _ = pending_error.take();
        }

        match run_with_lease(store, &lease) {
            Ok(()) => return Ok(()),
            Err(reason) => {
                let observed_utc = now();
                if last_failure_utc.is_some_and(|prior| {
                    observed_utc.saturating_sub(prior) > HOST_RECOVERY_RESET_WINDOW_SECONDS
                }) {
                    consecutive_failures = 0;
                }
                last_failure_utc = Some(observed_utc);

                let desired_now = match desired(store) {
                    Ok(value) => value,
                    Err(control_error) => {
                        let _ = persist_host_runtime_failure(
                            store,
                            &control_error,
                            false,
                            pending_recovery_count,
                            observed_utc,
                        );
                        return Err(control_error);
                    }
                };
                let handoff_active = match reviewed_install_handoff_active(store) {
                    Ok(value) => value,
                    Err(handoff_error) => {
                        let _ = persist_host_runtime_failure(
                            store,
                            &handoff_error,
                            false,
                            pending_recovery_count,
                            observed_utc,
                        );
                        return Err(handoff_error);
                    }
                };
                if desired_now != "RUNNING"
                    || handoff_active
                    || !recoverable_host_runtime_error(&reason)
                {
                    let _ = persist_host_runtime_failure(
                        store,
                        &reason,
                        false,
                        pending_recovery_count,
                        observed_utc,
                    );
                    return Err(reason);
                }
                if let Err(authority_error) = verify_current_host_authority(store) {
                    let _ = persist_host_runtime_failure(
                        store,
                        &authority_error,
                        false,
                        pending_recovery_count,
                        observed_utc,
                    );
                    return Err(authority_error);
                }

                consecutive_failures = consecutive_failures.saturating_add(1);
                if host_recovery_exhausted(consecutive_failures) {
                    let _ = persist_host_runtime_failure(
                        store,
                        &reason,
                        false,
                        pending_recovery_count,
                        observed_utc,
                    );
                    return Err("HOST_RUNTIME_RECOVERY_EXHAUSTED".into());
                }

                pending_recovery_count = pending_recovery_count.saturating_add(1);
                pending_error = Some((reason.clone(), observed_utc));
                if persist_host_runtime_failure(
                    store,
                    &reason,
                    true,
                    pending_recovery_count,
                    observed_utc,
                )
                .is_ok()
                {
                    pending_recovery_count = 0;
                    let _ = pending_error.take();
                }

                thread::sleep(host_recovery_backoff(consecutive_failures));

                if desired(store)? != "RUNNING" || reviewed_install_handoff_active(store)? {
                    return Ok(());
                }
                verify_current_host_authority(store)?;
            }
        }
    }
}

#[cfg(test)]
mod host_runtime_recovery_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    fn fixture() -> (PathBuf, Store) {
        let base = std::env::temp_dir().join(format!(
            "catdesk-wake-host-recovery-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let root = base.join("wake");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("fixture parent");
        let store = Store::open_scoped_for_test(&root, &base).expect("store");
        store.initialize().expect("initialize");
        (base, store)
    }

    #[test]
    fn recovery_allowlist_is_transient_only() {
        for reason in [
            "CONFIG_LOCK_UNAVAILABLE",
            "CONFIG_UNAVAILABLE",
            "QUEUE_UNAVAILABLE",
            "RECEIPT_UNAVAILABLE",
            "TURN_TIMER_UNAVAILABLE",
            "READINESS_HISTORY_UNAVAILABLE",
            "REVIEW_SOURCE_UNAVAILABLE",
            "REVIEW_SOURCE_INBOX_UNAVAILABLE",
            "STATE_UNAVAILABLE",
            "STATE_WRITE_FAILED",
            "STATE_REPLACE_FAILED",
            "STATE_SYNC_FAILED",
        ] {
            assert!(recoverable_host_runtime_error(reason), "{reason}");
        }
        for reason in [
            "HOST_ACTIVATION_INVALID",
            "STATE_MALFORMED",
            "STATE_REPARSE_REFUSED",
            "TARGET_GENERATION_STALE",
            "RECEIPT_BINDING_MISMATCH",
            "HOST_EXECUTABLE_HASH_MISMATCH",
            "HOST_INSTALL_POINTER_MISMATCH",
        ] {
            assert!(!recoverable_host_runtime_error(reason), "{reason}");
        }
    }

    #[test]
    fn recovery_backoff_is_bounded() {
        let observed: Vec<u64> = (1..=9)
            .map(|attempt| host_recovery_backoff(attempt).as_secs())
            .collect();
        assert_eq!(observed, vec![1, 2, 4, 8, 16, 32, 60, 60, 60]);
    }

    #[test]
    fn recovery_threshold_is_finite() {
        assert!(!host_recovery_exhausted(HOST_RECOVERY_MAX_ATTEMPTS));
        assert!(host_recovery_exhausted(
            HOST_RECOVERY_MAX_ATTEMPTS.saturating_add(1)
        ));
    }

    #[test]
    fn deferred_recovery_increment_is_merged_into_status() {
        let (base, store) = fixture();
        persist_host_runtime_failure(&store, "STATE_WRITE_FAILED", true, 3, 77)
            .expect("persist recovery telemetry");
        let live = status(&store).expect("status");
        assert_eq!(live.host_recovery_count, 3);
        assert_eq!(live.last_host_error.as_deref(), Some("STATE_WRITE_FAILED"));
        assert_eq!(live.last_host_error_utc, Some(77));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn legacy_status_defaults_recovery_telemetry() {
        let value = serde_json::json!({
            "version":"1.0.0-dev.68",
            "protocolVersion":1,
            "host":"RUNNING",
            "pid":42,
            "updatedUtc":1,
            "targets":{},
            "queueDepth":0,
            "staleCount":0,
            "browser":"NOT_OBSERVED",
            "login":"READY",
            "submission":"IDLE",
            "lastEventId":null,
            "lastAttemptUtc":null,
            "lastSuccessUtc":null,
            "lastReceipt":null,
            "attention":null,
            "turnTimers":[]
        });
        let status: Status = serde_json::from_value(value).expect("legacy status");
        assert_eq!(status.host_recovery_count, 0);
        assert!(status.last_host_error.is_none());
        assert!(status.last_host_error_utc.is_none());
    }
}

fn run_with_lease(store: &Store, lease: &HostLease) -> Result<()> {
    retry_safe_startup_attention(store)?;
    let mut live = status(store)?;
    live.version = VERSION.into();
    live.pid = std::process::id();
    live.attention = None;
    let mut adapter = None;
    let mut reconciliation_attempts = BTreeMap::<String, usize>::new();
    loop {
        let mode = desired(store)?;
        live.host = mode.clone();
        live.targets = store.config()?.targets;
        // Turn timers are durable Store state, not attempt-local browser state.
        // Refresh them on every status write, including while a blocking browser
        // attempt owns the host loop, so newly persisted exact-receipt timers are
        // visible before the assistant turn completes.
        persist_live_status(store, &mut live)?;
        if mode == "STOPPED" {
            break;
        }
        if mode == "RUNNING" {
            // Installation starts in non-dispatching mode. Migration must install
            // a reviewed sole-owner manifest before any browser can be started.
            if !store.root().join("activation.json").exists() {
                live.attention = Some("HOST_MIGRATION_NOT_ACTIVATED".into());
            } else {
                // An explicit valid activation is required, not presence alone.
                let activation: Activation = read(&store.root().join("activation.json"))?;
                if activation.schema_version != 1 || activation.owner != "CatDeskWake" {
                    return Err("HOST_ACTIVATION_INVALID".into());
                }
                discover_review_events(store, &activation)?;
                // Assistant-turn completion is browser-observed. The passive
                // turn timer below is status only; it never emits a wake or
                // consumes an assistant-authored control flag.
                for event in store.events()? {
                    let mut existing = store.delivery(&event.event_id)?;
                    if live
                        .targets
                        .get(&event.project_id)
                        .is_none_or(|t| t.generation != event.target_generation)
                    {
                        continue;
                    }
                    if desired(store)? != "RUNNING" {
                        break;
                    }

                    if maybe_retry_chatgpt_not_idle(store, &event, now())? {
                        existing = store.delivery(&event.event_id)?;
                        live.attention = None;
                        live.submission = "CLAIMED".into();
                    }

                    if let Some(delivery) = existing.as_ref() {
                        match delivery.phase {
                            Phase::Submitting => {
                                let prior_attempts = reconciliation_attempts
                                    .get(&event.event_id)
                                    .copied()
                                    .unwrap_or(0);
                                if !submitting_reconciliation_due(delivery, prior_attempts, now()) {
                                    // This exact ambiguous delivery is never replayed. Transient
                                    // receipt-proof failures may become eligible for another bounded
                                    // read-only reconciliation after their delay; later distinct
                                    // events remain free to progress in the meantime.
                                    continue;
                                }
                                reconciliation_attempts
                                    .insert(event.event_id.clone(), prior_attempts + 1);
                                if adapter.is_none() {
                                    match Adapter::spawn(store.root()) {
                                        Ok(value) => {
                                            live.browser = "READY".into();
                                            live.attention = None;
                                            adapter = Some(value);
                                        }
                                        Err(e) => {
                                            live.browser = "ATTENTION".into();
                                            live.attention = Some(e);
                                            break;
                                        }
                                    }
                                }
                                live.last_event_id = Some(event.event_id.clone());
                                live.last_attempt_utc = Some(now());
                                live.submission = "SUBMITTING".into();
                                match reconcile_submitting(
                                    store,
                                    adapter.as_mut().expect("adapter created"),
                                    delivery,
                                    &mut live,
                                ) {
                                    Ok(()) => {
                                        let graceful = retire_adapter(&mut adapter);
                                        live.browser = if graceful {
                                            "CLOSED_AFTER_SUCCESS".into()
                                        } else {
                                            "CLOSED_AFTER_SUCCESS_FORCED".into()
                                        };
                                        continue;
                                    }
                                    Err(reason) => {
                                        store
                                            .note_submitting_attention(&event.event_id, &reason)?;
                                        live.browser = "ATTENTION".into();
                                        live.attention = Some(reason);
                                        live.submission = "SUBMITTING".into();
                                        let _ = retire_adapter(&mut adapter);
                                        // This exact delivery remains quarantined as SUBMITTING,
                                        // but it must not block later distinct Wake events.
                                        continue;
                                    }
                                }
                            }
                            Phase::Claimed => {}
                            _ => continue,
                        }
                    }

                    if adapter.is_none() {
                        match Adapter::spawn(store.root()) {
                            Ok(value) => {
                                live.browser = "READY".into();
                                live.attention = None;
                                adapter = Some(value);
                            }
                            Err(e) => {
                                live.browser = "ATTENTION".into();
                                live.attention = Some(e);
                                break;
                            }
                        }
                    }
                    let delivery = store.claim(&event, lease)?;
                    live.last_event_id = Some(event.event_id.clone());
                    live.last_attempt_utc = Some(now());
                    live.submission = "CLAIMED".into();
                    let mut browser_session_attempt = 1usize;
                    let result = loop {
                        let result = attempt(
                            store,
                            lease,
                            adapter.as_mut().expect("adapter created"),
                            &delivery,
                            &mut live,
                        );
                        match result {
                            Ok(()) => break Ok(()),
                            Err(reason) => {
                                let phase = store
                                    .delivery(&event.event_id)?
                                    .ok_or("SUBMISSION_CLAIM_MISSING")?
                                    .phase;
                                if retryable_pre_submit_browser_failure(
                                    &reason,
                                    &phase,
                                    browser_session_attempt,
                                ) {
                                    browser_session_attempt += 1;
                                    let graceful = retire_adapter(&mut adapter);
                                    live.browser = if graceful {
                                        "RETRYING_AFTER_GRACEFUL_EXIT".into()
                                    } else {
                                        "RETRYING_AFTER_FORCED_EXIT".into()
                                    };
                                    live.attention = None;
                                    persist_live_status(store, &mut live)?;
                                    thread::sleep(Duration::from_secs(2));
                                    match Adapter::spawn(store.root()) {
                                        Ok(value) => {
                                            live.browser = "READY".into();
                                            adapter = Some(value);
                                            continue;
                                        }
                                        Err(spawn_reason) => break Err(spawn_reason),
                                    }
                                }
                                break Err(reason);
                            }
                        }
                    };
                    match result {
                        Err(reason) => {
                            let phase = store
                                .delivery(&event.event_id)?
                                .ok_or("SUBMISSION_CLAIM_MISSING")?
                                .phase;
                            if phase == Phase::Claimed {
                                store.transition(
                                    &event.event_id,
                                    lease.owner(),
                                    Phase::Attention,
                                    None,
                                    Some(reason.clone()),
                                )?;
                                live.attention = Some(reason);
                                live.submission = "ATTENTION".into();
                                let _ = retire_adapter(&mut adapter);
                                break;
                            }
                            // A Submitting journal is deliberately retained on
                            // any uncertainty. Persist the specific reason, never
                            // downgrade/replay it, then allow later distinct
                            // events to proceed.
                            store.note_submitting_attention(&event.event_id, &reason)?;
                            live.attention = Some(reason);
                            live.submission = "SUBMITTING".into();
                            let _ = retire_adapter(&mut adapter);
                            continue;
                        }
                        Ok(()) => {
                            let graceful = retire_adapter(&mut adapter);
                            live.browser = if graceful {
                                "CLOSED_AFTER_SUCCESS".into()
                            } else {
                                "CLOSED_AFTER_SUCCESS_FORCED".into()
                            };
                        }
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
    drop(adapter);
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Activation {
    pub schema_version: u32,
    pub owner: String,
    pub migration_audit: String,
    pub accept_after_utc: u64,
}

fn reconcile_submitting(
    store: &Store,
    adapter: &mut Adapter,
    d: &crate::store::Delivery,
    status: &mut Status,
) -> Result<()> {
    adapter.send(serde_json::json!({
        "reconcile": true,
        "event": d.event,
        "target": d.target
    }))?;
    let mut deadline = Instant::now() + Duration::from_secs(180);
    loop {
        persist_live_status(store, status)?;
        if desired(store)? != "RUNNING" {
            return Err("HOST_PAUSED_DURING_RECONCILIATION".into());
        }
        if Instant::now() >= deadline {
            return Err("SUBMISSION_RECONCILIATION_TIMEOUT".into());
        }
        match adapter.output.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(value)) => match value.get("stage").and_then(|v| v.as_str()) {
                Some("READINESS_OBSERVED") => {
                    let (route, reason) = record_readiness_observation(store, status, &value)?;
                    deadline = Instant::now() + Duration::from_secs(180);
                    status.browser = format!("READINESS_{route}_{reason}");
                    if route == "SAME_CONVERSATION" && reason == "READY" {
                        status.login = "READY".into();
                    }
                }
                Some(
                    stage @ ("CDP_REUSE_BEGIN"
                    | "CDP_REUSE_DONE"
                    | "CDP_ACTIVATE_BEGIN"
                    | "CDP_ACTIVATE_DONE"
                    | "TARGET_OPEN"
                    | "PAGE_READY"
                    | "GENERATION_WAIT"
                    | "GENERATION_ACTIVE"
                    | "RESPONSE_TIMEOUT_DETECTED"
                    | "RESPONSE_RETRYING"
                    | "RESPONSE_RETRY_STARTED"
                    | "RESPONSE_COMPLETED"
                    | "RESPONSE_COMPLETED_AFTER_REOPEN"),
                ) => {
                    deadline = Instant::now() + Duration::from_secs(180);
                    status.browser = stage.into();
                    if stage == "PAGE_READY" {
                        status.login = "READY".into();
                    }
                    if matches!(
                        stage,
                        "GENERATION_WAIT"
                            | "GENERATION_ACTIVE"
                            | "RESPONSE_TIMEOUT_DETECTED"
                            | "RESPONSE_RETRYING"
                            | "RESPONSE_RETRY_STARTED"
                            | "RESPONSE_COMPLETED"
                            | "RESPONSE_COMPLETED_AFTER_REOPEN"
                    ) {
                        record_response_stage(store, &d.event.event_id, stage)?;
                    }
                }
                Some("USER_MESSAGE_APPENDED") => {
                    deadline = Instant::now() + Duration::from_secs(180);
                    let receipt: Receipt = serde_json::from_value(
                        value.get("receipt").ok_or("RECEIPT_MISSING")?.clone(),
                    )
                    .map_err(|_| "RECEIPT_MALFORMED")?;
                    store.record_exact_receipt_timer(&d.event.event_id, &receipt)?;
                    status.last_receipt = Some(receipt);
                    status.submission = "USER_MESSAGE_APPENDED".into();
                    status.browser = "GENERATION_WAIT".into();
                    status.login = "READY".into();
                }
                Some("SENT") => {
                    let receipt: Receipt = serde_json::from_value(
                        value.get("receipt").ok_or("RECEIPT_MISSING")?.clone(),
                    )
                    .map_err(|_| "RECEIPT_MALFORMED")?;
                    let delivery =
                        store.reconcile_submitting_sent(&d.event.event_id, receipt.clone())?;
                    if delivery.phase != Phase::Sent {
                        return Err("SUBMISSION_RECONCILIATION_REFUSED".into());
                    }
                    store.complete_timer(&d.event.event_id)?;
                    status.last_success_utc = Some(now());
                    status.last_receipt = Some(receipt);
                    status.browser = "READY".into();
                    status.login = "READY".into();
                    status.submission = "SENT".into();
                    status.attention = None;
                    return Ok(());
                }
                Some("ATTENTION") => {
                    let reason = value
                        .get("reason")
                        .and_then(|v| v.as_str())
                        .filter(|s| crate::protocol::valid_id(s))
                        .unwrap_or("BROWSER_ATTENTION")
                        .to_string();
                    if reason.contains("LOGIN") {
                        status.login = "ATTENTION".into();
                    }
                    let _ =
                        store.set_timer_response_state(&d.event.event_id, ResponseState::Attention);
                    return Err(reason);
                }
                _ => return Err("BROWSER_ADAPTER_RESPONSE_INVALID".into()),
            },
            Ok(Err(e)) => return Err(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err("BROWSER_ADAPTER_EXITED".into()),
        }
    }
}

fn submission_control_reason(
    desired_state: &str,
    submit_dispatched: bool,
    receipt_recorded: bool,
) -> Option<&'static str> {
    if desired_state == "RUNNING" {
        None
    } else if !submit_dispatched {
        Some("HOST_PAUSED_BEFORE_SUBMIT")
    } else if receipt_recorded {
        Some("HOST_PAUSED_AFTER_RECEIPT")
    } else {
        // The submit command has crossed the Rust -> adapter boundary, but an
        // exact browser receipt is not durable yet. Pausing here would abandon
        // a potentially completed USER append and lose exactly-once evidence.
        // Keep draining this one bounded adapter attempt until receipt/error.
        None
    }
}

fn attempt(
    store: &Store,
    lease: &HostLease,
    adapter: &mut Adapter,
    d: &crate::store::Delivery,
    status: &mut Status,
) -> Result<()> {
    adapter.send(serde_json::json!({"event": d.event, "target": d.target}))?;
    let mut deadline = Instant::now() + Duration::from_secs(180);
    let mut submit_dispatched = false;
    let mut receipt_recorded = false;
    loop {
        persist_live_status(store, status)?;
        if let Some(reason) =
            submission_control_reason(&desired(store)?, submit_dispatched, receipt_recorded)
        {
            if receipt_recorded {
                let _ = store.set_timer_response_state(&d.event.event_id, ResponseState::Attention);
            }
            return Err(reason.into());
        }
        if Instant::now() >= deadline {
            return Err("BROWSER_ATTEMPT_TIMEOUT".into());
        }
        match adapter.output.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(value)) => match value.get("stage").and_then(|v| v.as_str()) {
                Some("READINESS_OBSERVED") => {
                    let (route, reason) = record_readiness_observation(store, status, &value)?;
                    deadline = Instant::now() + Duration::from_secs(180);
                    status.browser = format!("READINESS_{route}_{reason}");
                    if route == "SAME_CONVERSATION" && reason == "READY" {
                        status.login = "READY".into();
                    }
                }
                Some(
                    stage @ ("UC_OPEN_BEGIN"
                    | "UC_OPEN_DONE"
                    | "CDP_ACTIVATE_BEGIN"
                    | "CDP_ACTIVATE_DONE"
                    | "CDP_REUSE_BEGIN"
                    | "CDP_REUSE_DONE"
                    | "TARGET_OPEN"
                    | "PAGE_READY"
                    | "CHAT_IDLE"
                    | "PREWRITE_READY"
                    | "RECEIPT_BASELINE_READY"
                    | "PREWRITE_CONFIRMED"
                    | "GENERATION_WAIT"
                    | "GENERATION_ACTIVE"
                    | "RESPONSE_TIMEOUT_DETECTED"
                    | "RESPONSE_RETRYING"
                    | "RESPONSE_RETRY_STARTED"
                    | "RESPONSE_COMPLETED"
                    | "RESPONSE_COMPLETED_AFTER_REOPEN"),
                ) => {
                    deadline = Instant::now() + Duration::from_secs(180);
                    status.browser = stage.into();
                    if matches!(
                        stage,
                        "PAGE_READY"
                            | "CHAT_IDLE"
                            | "PREWRITE_READY"
                            | "RECEIPT_BASELINE_READY"
                            | "PREWRITE_CONFIRMED"
                    ) {
                        status.login = "READY".into();
                    }
                    if matches!(
                        stage,
                        "GENERATION_WAIT"
                            | "GENERATION_ACTIVE"
                            | "RESPONSE_TIMEOUT_DETECTED"
                            | "RESPONSE_RETRYING"
                            | "RESPONSE_RETRY_STARTED"
                            | "RESPONSE_COMPLETED"
                            | "RESPONSE_COMPLETED_AFTER_REOPEN"
                    ) {
                        record_response_stage(store, &d.event.event_id, stage)?;
                    }
                }
                Some("BEFORE_SUBMIT") => {
                    deadline = Instant::now() + Duration::from_secs(180);
                    if desired(store)? != "RUNNING" {
                        return Err("HOST_PAUSED_BEFORE_SUBMIT".into());
                    }
                    store.transition(
                        &d.event.event_id,
                        lease.owner(),
                        Phase::Submitting,
                        None,
                        None,
                    )?;
                    status.submission = "SUBMITTING".into();
                    adapter.send(serde_json::json!({"submit": true}))?;
                    submit_dispatched = true;
                }
                Some("SUBMISSION_ACCEPTED") => {
                    deadline = Instant::now() + Duration::from_secs(180);
                    store.start_submission_timer(&d.event.event_id)?;
                    status.submission = "SUBMISSION_ACCEPTED".into();
                    status.browser = "GENERATION_WAIT".into();
                    status.login = "READY".into();
                }
                Some("USER_MESSAGE_APPENDED") => {
                    deadline = Instant::now() + Duration::from_secs(180);
                    let receipt: Receipt = serde_json::from_value(
                        value.get("receipt").ok_or("RECEIPT_MISSING")?.clone(),
                    )
                    .map_err(|_| "RECEIPT_MALFORMED")?;
                    store.record_exact_receipt_timer(&d.event.event_id, &receipt)?;
                    receipt_recorded = true;
                    status.last_receipt = Some(receipt);
                    status.submission = "USER_MESSAGE_APPENDED".into();
                    status.browser = "GENERATION_WAIT".into();
                    status.login = "READY".into();
                }
                Some("SENT") => {
                    let receipt: Receipt = serde_json::from_value(
                        value.get("receipt").ok_or("RECEIPT_MISSING")?.clone(),
                    )
                    .map_err(|_| "RECEIPT_MALFORMED")?;
                    let delivery = store.transition(
                        &d.event.event_id,
                        lease.owner(),
                        Phase::Sent,
                        Some(receipt.clone()),
                        None,
                    )?;
                    if delivery.phase != Phase::Sent {
                        return Err("SUBMISSION_TRANSITION_INVALID".into());
                    }
                    store.complete_timer(&d.event.event_id)?;
                    status.last_success_utc = Some(now());
                    status.last_receipt = Some(receipt);
                    status.browser = "READY".into();
                    status.login = "READY".into();
                    status.submission = "SENT".into();
                    status.attention = None;
                    return Ok(());
                }
                Some("ATTENTION") => {
                    let reason = value
                        .get("reason")
                        .and_then(|v| v.as_str())
                        .filter(|s| crate::protocol::valid_id(s))
                        .unwrap_or("BROWSER_ATTENTION")
                        .to_string();
                    if reason.contains("LOGIN") {
                        status.login = "ATTENTION".into();
                    }
                    status.browser = "ATTENTION".into();
                    let _ =
                        store.set_timer_response_state(&d.event.event_id, ResponseState::Attention);
                    return Err(reason);
                }
                _ => return Err("BROWSER_ADAPTER_RESPONSE_INVALID".into()),
            },
            Ok(Err(e)) => return Err(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err("BROWSER_ADAPTER_EXITED".into()),
        }
    }
}

#[cfg(test)]
mod submission_control_tests {
    use super::submission_control_reason;

    #[test]
    fn pause_before_submit_aborts_without_crossing_write_boundary() {
        for desired in ["PAUSED", "STOPPED"] {
            assert_eq!(
                submission_control_reason(desired, false, false),
                Some("HOST_PAUSED_BEFORE_SUBMIT")
            );
        }
    }

    #[test]
    fn pause_after_submit_dispatch_waits_for_exact_receipt_or_error() {
        for desired in ["PAUSED", "STOPPED"] {
            assert_eq!(submission_control_reason(desired, true, false), None);
        }
    }

    #[test]
    fn pause_after_exact_receipt_can_stop_observer_without_losing_evidence() {
        for desired in ["PAUSED", "STOPPED"] {
            assert_eq!(
                submission_control_reason(desired, true, true),
                Some("HOST_PAUSED_AFTER_RECEIPT")
            );
        }
    }

    #[test]
    fn running_never_requests_pause_abort() {
        for (submit_dispatched, receipt_recorded) in [(false, false), (true, false), (true, true)] {
            assert_eq!(
                submission_control_reason("RUNNING", submit_dispatched, receipt_recorded),
                None
            );
        }
    }
}

#[cfg(test)]
mod reviewed_install_transaction_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    fn fixture() -> (PathBuf, Store) {
        let base = std::env::temp_dir().join(format!(
            "catdesk-wake-reviewed-install-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let root = base.join("wake");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&base).expect("fixture parent");
        let store = Store::open_scoped_for_test(&root, &base).expect("store");
        store.initialize().expect("initialize");
        std::fs::create_dir_all(root.join("versions")).expect("versions");
        (root, store)
    }

    fn stage_candidate(
        root: &Path,
        label: &str,
        host_body: &str,
        gui_body: &str,
        adapter_body: &str,
    ) -> (String, String) {
        let staging_name = format!(".staging-fixture-{label}");
        let staging = root.join("versions").join(&staging_name);
        std::fs::create_dir_all(&staging).expect("staging");
        for (name, body) in [
            ("CatDeskWakeHost.exe", host_body),
            ("CatDeskBinagotchy.exe", gui_body),
            ("adapter.py", adapter_body),
            ("wake_bridge.py", "bridge"),
        ] {
            std::fs::write(staging.join(name), body.as_bytes()).expect("artifact");
        }
        let host = sha256_file(&staging.join("CatDeskWakeHost.exe"), "host").expect("host sha");
        let gui = sha256_file(&staging.join("CatDeskBinagotchy.exe"), "gui").expect("gui sha");
        let mut artifacts = serde_json::Map::new();
        for name in [
            "CatDeskWakeHost.exe",
            "CatDeskBinagotchy.exe",
            "adapter.py",
            "wake_bridge.py",
        ] {
            artifacts.insert(
                name.into(),
                serde_json::Value::String(
                    sha256_file(&staging.join(name), "artifact").expect("artifact sha"),
                ),
            );
        }
        let id = format!("{}-{}-{}", VERSION, &host[..12], &gui[..12]);
        std::fs::write(
            staging.join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "version": VERSION,
                "protocolVersion": 1,
                "installedUtc": "fixture",
                "artifacts": artifacts,
                "acceptance": "DEVELOPMENT_NOT_ACTIVATED"
            }))
            .expect("manifest json"),
        )
        .expect("manifest");
        (staging_name, id)
    }

    #[test]
    fn reviewed_handoff_preserves_pre_stop_desired_state_across_retry() {
        let (root, store) = fixture();
        control(&store, "RUNNING").expect("running");
        assert_eq!(
            begin_or_resume_reviewed_install_handoff(&store, "candidate-a").expect("begin"),
            "RUNNING"
        );
        control(&store, "STOPPED").expect("transient stop");
        assert_eq!(
            begin_or_resume_reviewed_install_handoff(&store, "candidate-a").expect("resume"),
            "RUNNING"
        );
        complete_reviewed_install_handoff(&store, "candidate-a").expect("complete");
        assert!(!reviewed_install_transaction_path(&store).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reviewed_handoff_refuses_unproven_candidate_drift() {
        let (root, store) = fixture();
        control(&store, "PAUSED").expect("paused");
        begin_or_resume_reviewed_install_handoff(&store, "candidate-a").expect("begin");
        assert_eq!(
            begin_or_resume_reviewed_install_handoff(&store, "candidate-b").unwrap_err(),
            "HOST_INSTALL_HANDOFF_CONFLICT"
        );
        assert_eq!(
            begin_or_resume_reviewed_install_handoff(&store, "candidate-a").expect("resume"),
            "PAUSED"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reviewed_handoff_allows_new_reviewed_candidate_after_current_candidate_failed_to_start() {
        let (root, store) = fixture();
        control(&store, "RUNNING").expect("running");
        begin_or_resume_reviewed_install_handoff(&store, "candidate-a").expect("begin");
        atomic(
            &store.root().join("current.json"),
            &serde_json::json!({"schemaVersion":1,"directory":"candidate-a"}),
        )
        .expect("current pointer");
        control(&store, "STOPPED").expect("failed handoff stopped");

        assert_eq!(
            begin_or_resume_reviewed_install_handoff(&store, "candidate-b")
                .expect("supersede interrupted handoff"),
            "RUNNING"
        );
        let transaction: ReviewedInstallTransaction =
            read(&reviewed_install_transaction_path(&store)).expect("transaction");
        assert_eq!(transaction.candidate_directory, "candidate-b");
        assert_eq!(transaction.prior_desired, "RUNNING");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn complete_staging_directory_is_not_a_reviewed_candidate() {
        let (root, store) = fixture();
        let _ = stage_candidate(&root, "only-staging", "host-a", "gui-a", "adapter-a");
        assert_eq!(
            reviewed_candidate(&store).unwrap_err(),
            "HOST_REVIEWED_CANDIDATE_UNAVAILABLE"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reviewed_publication_is_idempotent_for_same_identity() {
        let (root, store) = fixture();
        let (first_staging, id) = stage_candidate(&root, "first", "host-a", "gui-a", "adapter-a");
        let first = publish_reviewed_install(&store, &first_staging, &id).expect("first publish");
        assert!(!first.already_materialized);
        assert!(root.join("versions").join(&id).is_dir());
        assert!(!root.join("versions").join(&first_staging).exists());

        let (second_staging, second_id) =
            stage_candidate(&root, "second", "host-a", "gui-a", "adapter-a");
        assert_eq!(id, second_id);
        let second =
            publish_reviewed_install(&store, &second_staging, &second_id).expect("second publish");
        assert!(second.already_materialized);
        assert!(!root.join("versions").join(&second_staging).exists());
        assert_eq!(reviewed_candidate(&store).expect("candidate").0, id);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reviewed_publication_refuses_same_version_different_identity() {
        let (root, store) = fixture();
        let (first_staging, first_id) =
            stage_candidate(&root, "first", "host-a", "gui-a", "adapter-a");
        publish_reviewed_install(&store, &first_staging, &first_id).expect("first publish");

        let (second_staging, second_id) =
            stage_candidate(&root, "second", "host-b", "gui-a", "adapter-a");
        assert_ne!(first_id, second_id);
        assert_eq!(
            publish_reviewed_install(&store, &second_staging, &second_id).unwrap_err(),
            "HOST_INSTALL_VERSION_IDENTITY_CONFLICT"
        );
        assert!(root.join("versions").join(&second_staging).is_dir());
        assert_eq!(reviewed_candidate(&store).expect("candidate").0, first_id);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reviewed_publication_refuses_hidden_artifact_drift_under_same_id() {
        let (root, store) = fixture();
        let (first_staging, first_id) =
            stage_candidate(&root, "first", "host-a", "gui-a", "adapter-a");
        publish_reviewed_install(&store, &first_staging, &first_id).expect("first publish");

        let (second_staging, second_id) =
            stage_candidate(&root, "second", "host-a", "gui-a", "adapter-b");
        assert_eq!(first_id, second_id);
        assert_eq!(
            publish_reviewed_install(&store, &second_staging, &second_id).unwrap_err(),
            "HOST_INSTALL_ARTIFACT_IDENTITY_CONFLICT"
        );
        assert!(root.join("versions").join(&second_staging).is_dir());
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn reviewed_install_lease_is_process_exclusive() {
        let (root, store) = fixture();
        let lease = reviewed_install_lease(&store).expect("first lease");
        assert_eq!(
            reviewed_install_lease(&store).unwrap_err(),
            "HOST_INSTALL_ALREADY_RUNNING"
        );
        drop(lease);
        reviewed_install_lease(&store).expect("lease after release");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn idle_status_does_not_report_historical_browser_attention() {
        let (root, store) = fixture();
        atomic(
            &root.join("status.json"),
            &Status {
                version: VERSION.into(),
                protocol_version: 1,
                host: "RUNNING".into(),
                pid: 42,
                updated_utc: now(),
                targets: BTreeMap::new(),
                queue_depth: 99,
                stale_count: 0,
                browser: "ATTENTION".into(),
                login: "ATTENTION".into(),
                submission: "CLAIMED".into(),
                last_event_id: Some("historical-event".into()),
                last_attempt_utc: Some(now()),
                last_success_utc: None,
                last_receipt: None,
                attention: None,
                host_recovery_count: 0,
                last_host_error: None,
                last_host_error_utc: None,
                turn_timers: vec![],
            },
        )
        .expect("seed stale status");

        let observed = status(&store).expect("status");
        assert_eq!(observed.queue_depth, 0);
        assert_eq!(observed.browser, "NOT_OBSERVED");
        assert_eq!(observed.login, "NOT_OBSERVED");
        assert_eq!(observed.submission, "IDLE");
        assert_eq!(observed.last_event_id.as_deref(), Some("historical-event"));
        let _ = std::fs::remove_dir_all(root);
    }
}
