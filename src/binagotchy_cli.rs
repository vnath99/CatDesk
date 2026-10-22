//! Console Binagotchy companion for CatDesk.
//!
//! This process mode is deliberately presentation/control only. It reads the
//! same durable autonomy and WakeHost state as the product, mutates the
//! designated ChatGPT conversation only through the existing paired CAS
//! authority, and controls only the independently installed WakeHost. It never
//! constructs `AppState`, binds MCP, owns the secure tunnel, or starts a second
//! CatDesk daemon.

use crate::core_host_acceptance_preflight::read_fixed_core_host_acceptance_preflight;
use crate::delegated::autonomy_observability::read_snapshot as read_autonomy_snapshot;
use crate::delegated::autonomy_projects::{
    AutonomousProjectRegistryStoreV1, AutonomousProjectRegistryV1, AutonomousProjectV1,
    AutonomousWorkspaceLeaseV1, resolve_registered_project_from_control_root,
};
use crate::delegated::autonomy_state::{
    AUTONOMY_STATE_SCHEMA_VERSION, AutonomousQueueV1, AutonomousSessionStateV1,
};
use crate::mcp::{DesignatedChatTargetErrorV1, DesignatedChatTargetV1};
use crate::state::load_app_config;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const BINAGOTCHY_CLI_MODE_FLAG: &str = "--catdesk-binagotchy-cli";
static MANUAL_EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn parse_binagotchy_cli_mode(args: &[String]) -> Result<bool, String> {
    let count = args
        .iter()
        .filter(|arg| arg.as_str() == BINAGOTCHY_CLI_MODE_FLAG)
        .count();
    if count > 1 {
        return Err(format!(
            "{BINAGOTCHY_CLI_MODE_FLAG} may only be supplied once"
        ));
    }
    if count == 1 && args.len() != 1 {
        return Err(format!(
            "{BINAGOTCHY_CLI_MODE_FLAG} does not accept additional command-line arguments"
        ));
    }
    Ok(count == 1)
}

#[cfg(windows)]
const CLI_MUTEX_NAME: &str = "Local\\CatDesk.BinagotchyCli.V1";

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(iter::once(0))
        .collect()
}

/// Start the normal Binagotchy companion in a visible, independent console.
/// Recovery and daemon startup can call this repeatedly; an already-running
/// shell is detected before a second console is created.
pub fn launch_associated(workspace: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        type Handle = *mut std::ffi::c_void;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenMutexW(access: u32, inherit: i32, name: *const u16) -> Handle;
            fn CloseHandle(handle: Handle) -> i32;
        }
        #[link(name = "user32")]
        unsafe extern "system" {
            fn OpenInputDesktop(flags: u32, inherit: i32, access: u32) -> Handle;
            fn CloseDesktop(handle: Handle) -> i32;
        }

        let desktop = unsafe { OpenInputDesktop(0, 0, 1) };
        if desktop.is_null() {
            return;
        }
        unsafe {
            CloseDesktop(desktop);
        }

        let name = wide(CLI_MUTEX_NAME);
        let existing = unsafe { OpenMutexW(0x0010_0000, 0, name.as_ptr()) };
        if !existing.is_null() {
            unsafe {
                CloseHandle(existing);
            }
            return;
        }

        if let Ok(executable) = std::env::current_exe() {
            // CREATE_NEW_CONSOLE. The companion must remain visible even when
            // launched by the hidden canonical daemon/recovery process.
            let _ = std::process::Command::new(executable)
                .arg(BINAGOTCHY_CLI_MODE_FLAG)
                .current_dir(workspace)
                .creation_flags(0x0000_0010)
                .spawn();
        }
    }
    #[cfg(not(windows))]
    let _ = workspace;
}

fn independent_wake_store() -> Result<catdesk_wake::store::Store, String> {
    let store = catdesk_wake::store::Store::open(&catdesk_wake::runtime::default_root()?)?;
    store.initialize()?;
    Ok(store)
}

fn redacted_transport_status() -> String {
    let Ok(config) = load_app_config() else {
        return "CONFIG_UNAVAILABLE".into();
    };
    format!(
        "{} / local MCP observed by CatDesk",
        config.tunnel.mode.as_str()
    )
}

fn read_target(workspace: &Path) -> Result<DesignatedChatTargetV1, DesignatedChatTargetErrorV1> {
    crate::mcp::operator_read_designated_chat_target(workspace)
}

fn validated_wake_predecessor_digest(url: &str, digest: &str) -> Result<String, String> {
    use crate::delegated::autonomy_projects::{
        canonical_project_chat_target, project_chat_target_digest,
    };
    if canonical_project_chat_target(url).ok().as_deref() != Some(url)
        || project_chat_target_digest(url) != digest
    {
        return Err("TARGET_PROTECTED_STATE_MISMATCH".into());
    }
    Ok(digest.to_owned())
}

fn update_target(workspace: &Path, url: &str) -> Result<DesignatedChatTargetV1, String> {
    let expected_sha256 = match read_target(workspace) {
        Ok(current) => current.sha256,
        Err(DesignatedChatTargetErrorV1::ProtectedStateMismatch) => {
            // The guarded paired update can repair an invalid registry digest
            // only when the existing independent Wake target still agrees on
            // the canonical predecessor URL. Obtain the old CAS identity from
            // Wake, not from the corrupt registry. The paired operation will
            // remeasure both authorities under its lock and fail closed on any
            // different kind of divergence before advancing a generation.
            let store = independent_wake_store()?;
            let wake = store
                .config()?
                .targets
                .get("catdesk")
                .cloned()
                .ok_or_else(|| "TARGET_AUTHORITY_UNAVAILABLE".to_string())?;
            validated_wake_predecessor_digest(&wake.url, &wake.digest)?
        }
        Err(error) => return Err(target_error(error).into()),
    };
    crate::mcp::operator_update_designated_chat_target(workspace, url, &expected_sha256)
        .map_err(|error| target_error(error).to_string())
}

const fn target_error(error: DesignatedChatTargetErrorV1) -> &'static str {
    match error {
        DesignatedChatTargetErrorV1::InvalidUrl => "TARGET_INVALID_URL",
        DesignatedChatTargetErrorV1::Stale => "TARGET_STALE",
        DesignatedChatTargetErrorV1::SourceProgress => "TARGET_SOURCE_PROGRESS",
        DesignatedChatTargetErrorV1::ProtectedStateMismatch => "TARGET_PROTECTED_STATE_MISMATCH",
        DesignatedChatTargetErrorV1::SynchronizationFailure => "TARGET_SYNCHRONIZATION_FAILED",
        DesignatedChatTargetErrorV1::Unavailable => "TARGET_AUTHORITY_UNAVAILABLE",
    }
}

fn normalized_manual_message() -> String {
    "MANUAL WAKE DEBUG - CatDesk diagnostic test - NOT natural acceptance.".to_string()
}

fn manual_event_id() -> String {
    let now = catdesk_wake::store::now();
    let sequence = MANUAL_EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("manual-binagotchy-{now}-{}-{sequence}", std::process::id())
}

fn print_wake_status(out: &mut impl Write) -> Result<(), String> {
    let store = independent_wake_store()?;
    let status = catdesk_wake::runtime::status(&store)?;
    let target = status.targets.get("catdesk");
    writeln!(out, "WakeHost {}", status.version).map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  host:       {} (pid {})", status.host, status.pid)
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  target:     {}",
        target.map_or("none", |value| value.url.as_str())
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  generation: {}",
        target.map_or(0, |value| value.generation)
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  queue:      {}", status.queue_depth).map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  historical stale: {} (forensic; not current queue health)",
        status.stale_count
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  browser:    {}", status.browser).map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  login:      {}", status.login).map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  submission: {}", status.submission).map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  attention:  {}",
        status.attention.as_deref().unwrap_or("none")
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  host recoveries: {}", status.host_recovery_count)
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    if let Some(error) = status.last_host_error.as_deref() {
        writeln!(out, "  last host error: {error}").map_err(|_| "CLI_OUTPUT_FAILED")?;
        if let Some(observed) = status.last_host_error_utc {
            writeln!(out, "  host error utc: {observed}").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
    }
    if let Some(event_id) = status.last_event_id.as_deref() {
        writeln!(out, "  last event: {event_id}").map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    Ok(())
}

fn print_status(workspace: &Path, out: &mut impl Write) -> Result<(), String> {
    let transport = redacted_transport_status();
    let snapshot = read_autonomy_snapshot(workspace, &transport);
    writeln!(out, "CatDesk Binagotchy").map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  autonomy:   {} / actor {} / provider {}",
        snapshot.overall.label(),
        snapshot.actor.label(),
        snapshot.provider
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  project:    {} / {} / {}",
        snapshot.project_id, snapshot.task_id, snapshot.session_id
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  model:      {} / {}",
        snapshot.model, snapshot.reasoning
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  last step:  {}", snapshot.last_step).map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  next:       {}", snapshot.next_action).map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  review:     {} unread", snapshot.unread_review_count)
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    match read_target(workspace) {
        Ok(target) => {
            writeln!(out, "  target:     {}", target.url).map_err(|_| "CLI_OUTPUT_FAILED")?
        }
        Err(error) => writeln!(out, "  target:     {}", target_error(error))
            .map_err(|_| "CLI_OUTPUT_FAILED")?,
    }
    match print_wake_status(out) {
        Ok(()) => Ok(()),
        Err(error) if error == "CLI_OUTPUT_FAILED" => Err(error),
        Err(_) => writeln!(
            out,
            "WakeHost unavailable (independent status read failed)."
        )
        .map_err(|_| "CLI_OUTPUT_FAILED".into()),
    }
}

fn print_acceptance(workspace: &Path, out: &mut impl Write) -> Result<(), String> {
    let presentation = read_fixed_core_host_acceptance_preflight(workspace).presentation();
    writeln!(
        out,
        "Core acceptance: {} ({})",
        presentation.classification, presentation.reason
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  next live gate: {}",
        presentation.next_live_gate.unwrap_or("NONE")
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    for gate in presentation.gates {
        writeln!(
            out,
            "  {}: {} ({})",
            gate.gate,
            if gate.accepted {
                "ACCEPTED"
            } else {
                "NOT_ACCEPTED"
            },
            gate.reason
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    writeln!(out, "  Display is read-only; it cannot mint acceptance.")
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    Ok(())
}

fn print_queue(out: &mut impl Write) -> Result<(), String> {
    let store = independent_wake_store()?;
    let events = store.events()?;
    if events.is_empty() {
        writeln!(out, "Wake queue is empty.").map_err(|_| "CLI_OUTPUT_FAILED")?;
        return Ok(());
    }
    writeln!(out, "Wake queue ({} event(s)):", events.len()).map_err(|_| "CLI_OUTPUT_FAILED")?;
    for event in events {
        writeln!(
            out,
            "  {}  type={}  generation={}  created={}",
            event.event_id, event.event_type, event.target_generation, event.created_utc
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    Ok(())
}

fn registered_projects(workspace: &Path) -> Result<AutonomousProjectRegistryV1, String> {
    let catdesk_root = workspace.join(".catdesk");
    let registry_root = catdesk_root.join("projects");
    if !safe_directory(workspace)
        || !safe_directory(&catdesk_root)
        || !safe_directory(&registry_root)
    {
        return Err("PROJECT_REGISTRY_UNAVAILABLE".into());
    }
    AutonomousProjectRegistryStoreV1::open_read_only(registry_root)
        .and_then(|store| store.load_registry())
        .map_err(|_| "PROJECT_REGISTRY_UNAVAILABLE".into())
}

fn print_projects(workspace: &Path, out: &mut impl Write) -> Result<(), String> {
    let registry = registered_projects(workspace)?;
    writeln!(out, "Registered projects ({}):", registry.projects.len())
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    for project in registry.projects {
        let verified = resolve_registered_project_from_control_root(workspace, &project.project_id)
            .is_ok_and(|scope| scope.project == project);
        writeln!(
            out,
            "  {}  identity={}  workspace={}  Codex thread recorded={}  conversation recorded={}",
            project.project_id,
            if verified { "VERIFIED" } else { "UNVERIFIED" },
            project.workspace.display(),
            project.codex_thread_id.is_some(),
            project.chatgpt_target_url.is_some(),
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    writeln!(
        out,
        "  Registry bindings are not live worker or Wake receipts."
    )
    .map_err(|_| "CLI_OUTPUT_FAILED".into())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SchedulerCursorView {
    schema_version: u32,
    last_granted_project_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionView {
    schema_version: u32,
    session_id: String,
    state: AutonomousSessionStateV1,
    current_task_id: Option<String>,
    active: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractProjectView {
    project_id: String,
    workspace: PathBuf,
    expected_origin: String,
}

fn regular_file_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "PROJECT_STATE_UNAVAILABLE")?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || reparse
        || metadata.len() > 1024 * 1024
    {
        return Err("PROJECT_STATE_UNAVAILABLE".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "PROJECT_STATE_UNAVAILABLE")?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "PROJECT_STATE_UNAVAILABLE")?;
    if bytes.len() > 1024 * 1024 {
        return Err("PROJECT_STATE_UNAVAILABLE".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "PROJECT_STATE_UNAVAILABLE".into())
}

fn conservative_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn repl_project_id(value: &str) -> bool {
    conservative_id(value)
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn safe_directory(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    metadata.is_dir() && !metadata.file_type().is_symlink() && !reparse
}

fn scheduler_cursor(
    root: &Path,
    registry: &AutonomousProjectRegistryV1,
) -> Result<Option<String>, String> {
    let path = root.join("scheduler-cursor.json");
    if !path
        .try_exists()
        .map_err(|_| "SCHEDULER_CURSOR_UNAVAILABLE")?
    {
        return Ok(None);
    }
    let cursor: SchedulerCursorView = regular_file_json(&path)?;
    if cursor.schema_version != 1
        || !registry
            .projects
            .iter()
            .any(|project| project.project_id == cursor.last_granted_project_id)
    {
        return Err("SCHEDULER_CURSOR_UNAVAILABLE".into());
    }
    Ok(Some(cursor.last_granted_project_id))
}

fn scheduler_leases(
    root: &Path,
    registry: &AutonomousProjectRegistryV1,
) -> Result<Vec<AutonomousWorkspaceLeaseV1>, String> {
    let directory = root.join("leases");
    if !directory
        .try_exists()
        .map_err(|_| "SCHEDULER_LEASES_UNAVAILABLE")?
    {
        return Ok(Vec::new());
    }
    if !safe_directory(&directory) {
        return Err("SCHEDULER_LEASES_UNAVAILABLE".into());
    }
    let mut leases = Vec::new();
    for entry in fs::read_dir(directory).map_err(|_| "SCHEDULER_LEASES_UNAVAILABLE")? {
        let entry = entry.map_err(|_| "SCHEDULER_LEASES_UNAVAILABLE")?;
        if leases.len() >= 128 {
            return Err("SCHEDULER_LEASES_UNAVAILABLE".into());
        }
        let lease: AutonomousWorkspaceLeaseV1 = regular_file_json(&entry.path())?;
        let registered = registry.projects.iter().any(|project| {
            project.project_id == lease.project_id && project.workspace == lease.workspace
        });
        let expected_filename = format!(
            "{:x}.json",
            Sha256::digest(lease.workspace.to_string_lossy().as_bytes())
        );
        if !registered
            || entry.file_name() != expected_filename.as_str()
            || !conservative_id(&lease.owner_id)
        {
            return Err("SCHEDULER_LEASES_UNAVAILABLE".into());
        }
        leases.push(lease);
    }
    leases.sort_by(|left, right| left.project_id.cmp(&right.project_id));
    Ok(leases)
}

fn project_task_rows(project: &AutonomousProjectV1) -> Result<(Vec<String>, usize, usize), String> {
    if !safe_directory(&project.workspace) {
        return Err("PROJECT_QUEUE_UNAVAILABLE".into());
    }
    let catdesk_root = project.workspace.join(".catdesk");
    if !catdesk_root
        .try_exists()
        .map_err(|_| "PROJECT_QUEUE_UNAVAILABLE")?
    {
        return Ok((Vec::new(), 0, 0));
    }
    if !safe_directory(&catdesk_root) {
        return Err("PROJECT_QUEUE_UNAVAILABLE".into());
    }
    let root = catdesk_root.join("autonomy");
    if !root.try_exists().map_err(|_| "PROJECT_QUEUE_UNAVAILABLE")? {
        return Ok((Vec::new(), 0, 0));
    }
    if !safe_directory(&root) {
        return Err("PROJECT_QUEUE_UNAVAILABLE".into());
    }
    let mut rows = Vec::new();
    let mut omitted = 0;
    let mut scope_rejected = 0;
    let mut entries = fs::read_dir(root)
        .map_err(|_| "PROJECT_QUEUE_UNAVAILABLE")?
        .take(4097)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "PROJECT_QUEUE_UNAVAILABLE")?;
    if entries.len() > 4096 {
        return Err("PROJECT_QUEUE_UNAVAILABLE".into());
    }
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let session_id = entry.file_name().to_string_lossy().into_owned();
        if !conservative_id(&session_id) || !safe_directory(&entry.path()) {
            continue;
        }
        let state_path = entry.path().join("state.json");
        if !state_path
            .try_exists()
            .map_err(|_| "PROJECT_QUEUE_UNAVAILABLE")?
        {
            continue;
        }
        let session: SessionView = regular_file_json(&state_path)?;
        if session.schema_version != AUTONOMY_STATE_SCHEMA_VERSION
            || session.session_id != session_id
            || session.active == session.state.is_terminal()
            || session
                .current_task_id
                .as_deref()
                .is_some_and(|id| !conservative_id(id))
        {
            return Err("PROJECT_QUEUE_UNAVAILABLE".into());
        }
        if !session.active {
            continue;
        }
        let contract_path = entry.path().join("contract.json");
        if !contract_path
            .try_exists()
            .map_err(|_| "PROJECT_QUEUE_UNAVAILABLE")?
        {
            omitted += 1;
            continue;
        }
        let contract: ContractProjectView = match regular_file_json(&contract_path) {
            Ok(contract) => contract,
            Err(_) => {
                scope_rejected += 1;
                continue;
            }
        };
        if contract.project_id != project.project_id
            || contract.expected_origin != project.git_identity
            || contract.workspace.canonicalize().ok().as_deref()
                != Some(project.workspace.as_path())
        {
            scope_rejected += 1;
            continue;
        }
        let queue: AutonomousQueueV1 = regular_file_json(&entry.path().join("queue.json"))?;
        queue.validate().map_err(|_| "PROJECT_QUEUE_UNAVAILABLE")?;
        for task in queue.tasks {
            if rows.len() < 64 {
                rows.push(format!(
                    "{} / {}  task={}  state={:?}",
                    project.project_id, session_id, task.task_id, task.state
                ));
            } else {
                omitted += 1;
            }
        }
    }
    rows.sort();
    Ok((rows, omitted, scope_rejected))
}

fn print_project_queue(workspace: &Path, out: &mut impl Write) -> Result<(), String> {
    let registry = registered_projects(workspace)?;
    let root = workspace.join(".catdesk/projects");
    let cursor = scheduler_cursor(&root, &registry)?;
    let leases = scheduler_leases(&root, &registry)?;
    writeln!(out, "Project queue (read-only durable state):").map_err(|_| "CLI_OUTPUT_FAILED")?;
    // The explicit operator Codex queue is separate from historical managed
    // session state. Read both so current tasks are visible without claiming a
    // lease or dispatching a worker.
    match read_operator_codex_queue(workspace) {
        Ok(report) => render_operator_codex_queue(workspace, &registry, &report, out)?,
        Err(_) => writeln!(
            out,
            "  operator Codex queue: unavailable (read-only status failed)"
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?,
    }
    writeln!(
        out,
        "  fair selector last grant: {}",
        cursor.as_deref().unwrap_or("none")
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  workspace leases: {}", leases.len()).map_err(|_| "CLI_OUTPUT_FAILED")?;
    for lease in &leases {
        writeln!(
            out,
            "    {}  provider={:?}  expires={}",
            lease.project_id, lease.provider, lease.expires_at_unix
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    for project in registry.projects {
        let scope =
            match resolve_registered_project_from_control_root(workspace, &project.project_id) {
                Ok(scope) => scope,
                Err(_) => {
                    writeln!(
                        out,
                        "  {}: PROJECT_IDENTITY_UNAVAILABLE",
                        project.project_id
                    )
                    .map_err(|_| "CLI_OUTPUT_FAILED")?;
                    continue;
                }
            };
        let (rows, omitted, scope_rejected) = match project_task_rows(&scope.project) {
            Ok(tasks) => tasks,
            Err(error) => {
                writeln!(out, "  {}: {error}", project.project_id)
                    .map_err(|_| "CLI_OUTPUT_FAILED")?;
                continue;
            }
        };
        writeln!(
            out,
            "  {}: {} task(s) shown, {} omitted, {} scope-rejected session(s)",
            project.project_id,
            rows.len(),
            omitted,
            scope_rejected
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
        for row in rows {
            writeln!(out, "    {row}").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
    }
    writeln!(
        out,
        "  Durable state is not proof that a worker is running."
    )
    .map_err(|_| "CLI_OUTPUT_FAILED".into())
}

fn read_operator_codex_action(workspace: &Path, args: Vec<String>) -> Result<Value, String> {
    let action = crate::operator_codex_cli::parse_action(&args)?;
    let control = workspace.to_path_buf();
    // The REPL runs on CatDesk's Tokio thread. A short worker thread gives the
    // existing async operator entry point its own runtime without nesting one.
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "CODEX_OPERATOR: report runtime unavailable".to_string())?;
        runtime.block_on(crate::operator_codex_cli::execute_action(action, &control))
    })
    .join()
    .map_err(|_| "CODEX_OPERATOR: report worker failed".to_string())?
}

fn read_operator_codex_queue(workspace: &Path) -> Result<Value, String> {
    read_operator_codex_action(
        workspace,
        vec![
            "operator".into(),
            "codex".into(),
            "queue".into(),
            "status".into(),
        ],
    )
}

fn render_operator_codex_queue(
    workspace: &Path,
    registry: &AutonomousProjectRegistryV1,
    report: &Value,
    out: &mut impl Write,
) -> Result<(), String> {
    if report["action"] != "codex_queue_status" {
        return Err("CODEX_QUEUE: operator report action is invalid".into());
    }
    let jobs = report["jobs"]
        .as_array()
        .filter(|jobs| jobs.len() <= 512)
        .ok_or("CODEX_QUEUE: operator report is invalid or oversized")?;
    writeln!(out, "  operator Codex queue: {} job(s)", jobs.len())
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    for job in jobs.iter().take(24) {
        let project_id = safe_report_token(&job["projectId"]);
        let task_id = safe_report_token(&job["taskId"]);
        let state = safe_report_token(&job["state"]);
        let verified = registry
            .projects
            .iter()
            .find(|project| project.project_id == project_id)
            .is_some_and(|project| {
                operator_job_matches_project_binding(project, job)
                    && resolve_registered_project_from_control_root(workspace, project_id)
                        .is_ok_and(|scope| scope.project == *project)
            });
        writeln!(
            out,
            "    {project_id} / {task_id}  state={state}  identity={}",
            if verified { "VERIFIED" } else { "UNVERIFIED" }
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    if jobs.len() > 24 {
        writeln!(
            out,
            "    {} additional operator job(s) omitted.",
            jobs.len() - 24
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    Ok(())
}

fn operator_job_matches_project_binding(project: &AutonomousProjectV1, job: &Value) -> bool {
    let Some(thread_id) = project.codex_thread_id.as_deref() else {
        return false;
    };
    let Some(workspace) = project.workspace.to_str() else {
        return false;
    };
    let target_matches = match (
        project.chatgpt_target_sha256.as_deref(),
        job.get("targetSha256"),
    ) {
        (Some(expected), Some(Value::String(observed))) => observed == expected,
        (None, Some(Value::Null)) => true,
        _ => false,
    };
    let git_identity_sha256 = format!("{:x}", Sha256::digest(project.git_identity.as_bytes()));
    job["projectId"].as_str() == Some(project.project_id.as_str())
        && job["workspace"].as_str() == Some(workspace)
        && job["threadId"].as_str() == Some(thread_id)
        && target_matches
        && job["gitIdentitySha256"].as_str() == Some(git_identity_sha256.as_str())
        && job["verificationProfile"].as_str() == Some(project.verification_profile.as_str())
}

#[derive(Clone, Copy)]
enum ScopedCodexVerb {
    Resume,
    RecoveryStatus,
    RecoveryPrepare,
}

fn verified_codex_project(
    workspace: &Path,
    project_id: &str,
) -> Result<AutonomousProjectV1, String> {
    if !repl_project_id(project_id) {
        return Err("CODEX_SCOPE: project ID is invalid".into());
    }
    let registry = registered_projects(workspace)?;
    let registered = registry
        .projects
        .iter()
        .find(|project| project.project_id == project_id)
        .ok_or("CODEX_SCOPE: project ID is not registered")?;
    let resolved = resolve_registered_project_from_control_root(workspace, project_id)
        .map_err(|_| "CODEX_SCOPE: control or project Git identity is unavailable".to_string())?;
    if &resolved.project != registered {
        return Err("CODEX_SCOPE: project binding changed during verification".into());
    }
    Ok(resolved.project)
}

fn scoped_codex_args(
    project: &AutonomousProjectV1,
    verb: ScopedCodexVerb,
) -> Result<Vec<String>, String> {
    let workspace = project
        .workspace
        .to_str()
        .ok_or("CODEX_SCOPE: project path is not Unicode")?;
    let mut args = vec!["operator".into(), "codex".into()];
    match verb {
        ScopedCodexVerb::Resume => args.push("resume".into()),
        ScopedCodexVerb::RecoveryStatus | ScopedCodexVerb::RecoveryPrepare => {
            args.push("recovery".into());
            args.push(
                if matches!(verb, ScopedCodexVerb::RecoveryPrepare) {
                    "prepare"
                } else {
                    "status"
                }
                .into(),
            );
        }
    }
    args.extend([
        "--project-id".into(),
        project.project_id.clone(),
        "--workspace".into(),
        workspace.into(),
    ]);
    Ok(args)
}

fn resume_codex_project(
    workspace: &Path,
    project_id: &str,
    out: &mut impl Write,
) -> Result<(), String> {
    let project = verified_codex_project(workspace, project_id)?;
    let thread = project
        .codex_thread_id
        .as_deref()
        .ok_or("CODEX_THREAD: project has no bound Codex thread")?;
    if !uuid::Uuid::parse_str(thread).is_ok_and(|id| id.to_string() == thread) {
        return Err("CODEX_THREAD: bound thread ID is invalid".into());
    }
    let args = scoped_codex_args(&project, ScopedCodexVerb::Resume)?;
    let executable = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|_| "CODEX_UNAVAILABLE: CatDesk executable is unavailable".to_string())?;
    if !executable.is_absolute() {
        return Err("CODEX_UNAVAILABLE: CatDesk executable is not absolute".into());
    }
    writeln!(
        out,
        "Opening the bound native Codex thread for project {project_id}."
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "Binagotchy waits here until the native session exits.")
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    out.flush().map_err(|_| "CLI_OUTPUT_FAILED")?;
    // The child rechecks the registration and writer guard. Passing arguments
    // directly to this exact executable preserves the normal native CLI and
    // approval behavior without a shell or a new authority surface.
    let status = std::process::Command::new(executable)
        .args(args)
        .current_dir(workspace)
        .status()
        .map_err(|_| "CODEX_UNAVAILABLE: scoped CatDesk operator could not start".to_string())?;
    if !status.success() {
        return Err(format!(
            "CODEX_EXIT: scoped operator returned {}; inspect the native session output and `recovery {project_id}`",
            status
                .code()
                .map_or("signal".into(), |code| code.to_string())
        ));
    }
    writeln!(
        out,
        "Scoped native Codex session exited zero for {project_id}."
    )
    .map_err(|_| "CLI_OUTPUT_FAILED".into())
}

fn read_codex_recovery(
    workspace: &Path,
    project: &AutonomousProjectV1,
    prepare: bool,
) -> Result<Value, String> {
    let args = scoped_codex_args(
        project,
        if prepare {
            ScopedCodexVerb::RecoveryPrepare
        } else {
            ScopedCodexVerb::RecoveryStatus
        },
    )?;
    read_operator_codex_action(workspace, args)
}

fn safe_report_token(value: &Value) -> &str {
    value
        .as_str()
        .filter(|token| conservative_id(token))
        .unwrap_or("[redacted]")
}

fn print_web_projects(workspace: &Path, out: &mut impl Write) -> Result<(), String> {
    let report = crate::operator_facade::read_web_status(workspace)?;
    render_web_projects(&report, out)
}

/// Display only bounded metadata from the shared paired Web reader. Saved
/// scheduler/rollover observations never establish that a worker is running.
pub(crate) fn render_web_projects(report: &Value, out: &mut impl Write) -> Result<(), String> {
    let central = &report["webProjects"];
    if report["action"] != "web_status"
        || report["readOnly"] != true
        || central["status"] != "SOURCE_STATUS_ONLY"
        || central["readOnly"] != true
    {
        return Err("WEB_STATUS_UNAVAILABLE".into());
    }
    let projects = central["projects"]
        .as_array()
        .ok_or("WEB_STATUS_UNAVAILABLE")?;
    writeln!(
        out,
        "Web projects: {} | {} | read-only saved state",
        projects.len(),
        safe_report_token(&report["runtimeMode"])
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "Context is advisory; UNKNOWN coverage has no reliable capacity percentage. Live workers, delivery and installed acceptance require separate proof.")
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    for project in projects.iter().take(24) {
        let wake = &project["wake"];
        let target = &wake["target"];
        let generation = target["generation"].as_u64().filter(|n| *n > 0);
        let paired = wake["status"] == "PAIRED" && generation.is_some();
        let generation_label = generation
            .filter(|_| paired)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "UNKNOWN".into());
        let digest = target["sha256"].as_str().filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        });
        let digest_label = digest
            .filter(|_| paired)
            .map(|value| &value[..12])
            .unwrap_or("UNKNOWN");
        let history = project["conversationHistory"]
            .as_array()
            .map(Vec::len)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "UNKNOWN".into());
        writeln!(
            out,
            "  {} | Wake {} | generation {} | digest {} | history {}",
            safe_report_token(&project["projectId"]),
            safe_report_token(&wake["status"]),
            generation_label,
            digest_label,
            history
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
        let context = &project["context"];
        let pressure = &context["currentPressure"];
        let current = paired
            && digest.is_some()
            && context["status"] == "CURRENT_ADVISORY"
            && context["advisory"] == true
            && pressure["advisory"] == true
            && pressure["isCurrent"] == true
            && pressure["key"]["projectId"] == project["projectId"]
            && pressure["key"]["projectId"] == target["projectId"]
            && pressure["key"]["conversationUrl"] == target["url"]
            && target["url"].is_string()
            && pressure["key"]["targetGeneration"].as_u64() == generation
            && pressure["targetSha256"].as_str() == digest;
        if let Some(tokens) = pressure["observedEstimateTokens"]
            .as_u64()
            .filter(|_| current)
        {
            let coverage = if pressure["coverageKnown"] == true {
                "KNOWN"
            } else {
                "UNKNOWN"
            };
            let trigger = pressure["triggerTokens"]
                .as_u64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "UNKNOWN".into());
            writeln!(out, "    context: observed {tokens} tokens (advisory), coverage {coverage}, configured trigger {trigger}")
                .map_err(|_| "CLI_OUTPUT_FAILED")?;
        } else {
            writeln!(
                out,
                "    context: unavailable for current paired conversation"
            )
            .map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        let rollover = &project["rollover"];
        let automation = &rollover["automation"];
        let scope = if paired && automation["targetGeneration"].as_u64() == generation {
            "SAME_GENERATION_SNAPSHOT"
        } else {
            "STALE_OR_UNVERIFIED_SNAPSHOT"
        };
        let tick = automation["tickUtc"]
            .as_u64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "UNKNOWN".into());
        let queued = project["queue"]["queuedCount"]
            .as_u64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "UNKNOWN".into());
        writeln!(
            out,
            "    saved rollover: {} | scheduler: {} ({scope}, tick {tick}) | queued {queued}",
            safe_report_token(&rollover["phase"]),
            safe_report_token(&automation["stage"])
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    if projects.len() > 24 {
        writeln!(
            out,
            "  {} additional Web project(s) omitted",
            projects.len() - 24
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    Ok(())
}

fn print_codex_recovery(
    workspace: &Path,
    project_id: &str,
    prepare: bool,
    out: &mut impl Write,
) -> Result<(), String> {
    let project = verified_codex_project(workspace, project_id)?;
    let report = read_codex_recovery(workspace, &project, prepare)?;
    render_codex_recovery_report(&project, &report, prepare, out)
}

fn render_codex_recovery_report(
    project: &AutonomousProjectV1,
    report: &Value,
    prepare: bool,
    out: &mut impl Write,
) -> Result<(), String> {
    let project_id = &project.project_id;
    if report["projectId"] != project.project_id
        || report["readOnly"] != true
        || report["launchAuthorized"] != false
    {
        return Err("CODEX_RECOVERY: report scope or authority is invalid".into());
    }
    let status = report["status"]
        .as_str()
        .filter(|status| {
            matches!(
                *status,
                "NO_RECORDED_BLOCKER_OBSERVED" | "NEEDS_OWNER_RECOVERY"
            )
        })
        .ok_or("CODEX_RECOVERY: report status is unavailable")?;
    let sessions = report["sessionBlockers"]
        .as_array()
        .ok_or("CODEX_RECOVERY: session report is unavailable")?;
    let queue = report["queueBlockers"]
        .as_array()
        .ok_or("CODEX_RECOVERY: queue report is unavailable")?;
    let accounting = report["accountingReviewRows"]
        .as_array()
        .ok_or("CODEX_RECOVERY: accounting report is unavailable")?;
    let marker = report["pendingWriterMarker"]["present"]
        .as_bool()
        .ok_or("CODEX_RECOVERY: writer marker report is unavailable")?;
    let observed_sessions = report["observedSessionCount"]
        .as_u64()
        .filter(|count| *count <= 1_024)
        .ok_or("CODEX_RECOVERY: observed session count is invalid")?;
    if sessions.len() + queue.len() + accounting.len() > 4_096 {
        return Err("CODEX_RECOVERY: report row count is invalid".into());
    }
    writeln!(out, "Codex recovery for {project_id}: {status}").map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  observed sessions: {}  session blockers: {}  queue blockers: {}  accounting review: {}  writer marker: {}",
        observed_sessions,
        sessions.len(),
        queue.len(),
        accounting.len(),
        if marker { "present" } else { "absent" }
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    let mut displayed = 0usize;
    for (label, rows) in [
        ("session", sessions),
        ("queue", queue),
        ("accounting", accounting),
    ] {
        for row in rows {
            if displayed == 8 {
                break;
            }
            writeln!(
                out,
                "  {label}: session={} task={} state={}",
                safe_report_token(&row["sessionId"]),
                safe_report_token(&row["taskId"]),
                if row["taskState"].is_string() {
                    safe_report_token(&row["taskState"])
                } else if row["sessionState"].is_string() {
                    safe_report_token(&row["sessionState"])
                } else {
                    "review"
                },
            )
            .map_err(|_| "CLI_OUTPUT_FAILED")?;
            displayed += 1;
        }
    }
    let omitted = sessions.len() + queue.len() + accounting.len() - displayed;
    if omitted > 0 {
        writeln!(
            out,
            "  {omitted} additional row(s) omitted from this console view."
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    writeln!(
        out,
        "  Read-only observation; no writer state was cleared and no launch was authorized."
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    if prepare {
        if let Some(required) = report["requiredEvidence"].as_array() {
            for item in required.iter().take(4) {
                if let Some(step) = item
                    .as_str()
                    .filter(|step| step.len() <= 240 && !step.chars().any(char::is_control))
                {
                    writeln!(out, "  required: {step}").map_err(|_| "CLI_OUTPUT_FAILED")?;
                }
            }
        }
    } else {
        writeln!(
            out,
            "  Type `recovery prepare {project_id}` for owner handoff steps."
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    Ok(())
}

fn send_manual_test(out: &mut impl Write) -> Result<(), String> {
    let store = independent_wake_store()?;
    let message = normalized_manual_message();
    let event = store.produce(&manual_event_id(), "catdesk", "test", &message)?;
    writeln!(out, "Queued MANUAL wake test: {}", event.event_id)
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "This explicit test does NOT count as natural automatic-wake acceptance."
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    Ok(())
}

fn print_help(out: &mut impl Write) -> Result<(), String> {
    for line in [
        "Commands:",
        "  status                         CatDesk + independent WakeHost status",
        "  projects                       Show registered project paths and binding presence",
        "  web projects | web status      Show paired Web targets, context and saved progress",
        "  queue                          Show fair selector, leases and scoped task IDs",
        "  resume <project-id>            Resume that project's bound native Codex thread",
        "  recovery <project-id>          Read scoped Codex blocker report",
        "  recovery prepare <project-id>  Show owner recovery evidence steps",
        "  target                         Show authoritative designated ChatGPT URL",
        "  target <url>                   Set designated URL through paired CAS authority",
        "  target set <url>               Same as above",
        "  wake status                    Show WakeHost status",
        "  wake start|pause|resume|stop   Control independent WakeHost",
        "  wake queue                     Show current independent wake queue",
        "  wake send | wake test          Queue a fixed MANUAL diagnostic wake",
        "  wake retire <event-id>         Archive one exact safe pre-submit stale event",
        "  acceptance                     Show read-only core acceptance readiness",
        "  clear                          Clear this console",
        "  help                           Show commands",
        "  exit | quit | q                Close Binagotchy only (WakeHost keeps running)",
    ] {
        writeln!(out, "{line}").map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum ParsedCommand<'a> {
    Status,
    Projects,
    WebProjects,
    ProjectQueue,
    ResumeUsage,
    Resume(&'a str),
    RecoveryUsage,
    Recovery { project_id: &'a str, prepare: bool },
    TargetShow,
    TargetSet(&'a str),
    WakeStatus,
    WakeStart,
    WakePause,
    WakeResume,
    WakeStop,
    WakeQueue,
    WakeSend,
    WakeRetire(&'a str),
    Acceptance,
    Help,
    Clear,
    Exit,
    Empty,
    Unknown,
}

fn parse_command(input: &str) -> ParsedCommand<'_> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return ParsedCommand::Empty;
    }
    match trimmed {
        "status" => return ParsedCommand::Status,
        "projects" => return ParsedCommand::Projects,
        "web projects" | "web status" => return ParsedCommand::WebProjects,
        "queue" => return ParsedCommand::ProjectQueue,
        "resume" => return ParsedCommand::ResumeUsage,
        "recovery" => return ParsedCommand::RecoveryUsage,
        "target" => return ParsedCommand::TargetShow,
        "acceptance" => return ParsedCommand::Acceptance,
        "help" | "?" => return ParsedCommand::Help,
        "clear" | "cls" => return ParsedCommand::Clear,
        "exit" | "quit" | "q" => return ParsedCommand::Exit,
        "wake" | "wake status" => return ParsedCommand::WakeStatus,
        "wake start" => return ParsedCommand::WakeStart,
        "wake pause" => return ParsedCommand::WakePause,
        "wake resume" => return ParsedCommand::WakeResume,
        "wake stop" => return ParsedCommand::WakeStop,
        "wake queue" => return ParsedCommand::WakeQueue,
        _ => {}
    }
    if let Some(project_id) = trimmed.strip_prefix("resume ").map(str::trim) {
        return if repl_project_id(project_id) {
            ParsedCommand::Resume(project_id)
        } else {
            ParsedCommand::Unknown
        };
    }
    if let Some(project_id) = trimmed.strip_prefix("recovery prepare ").map(str::trim) {
        return if repl_project_id(project_id) {
            ParsedCommand::Recovery {
                project_id,
                prepare: true,
            }
        } else {
            ParsedCommand::Unknown
        };
    }
    if let Some(project_id) = trimmed.strip_prefix("recovery ").map(str::trim) {
        return if repl_project_id(project_id) {
            ParsedCommand::Recovery {
                project_id,
                prepare: false,
            }
        } else {
            ParsedCommand::Unknown
        };
    }
    if let Some(url) = trimmed.strip_prefix("target set ").map(str::trim) {
        return if url.is_empty() {
            ParsedCommand::Unknown
        } else {
            ParsedCommand::TargetSet(url)
        };
    }
    if let Some(url) = trimmed.strip_prefix("target ").map(str::trim) {
        return if url.is_empty() {
            ParsedCommand::Unknown
        } else {
            ParsedCommand::TargetSet(url)
        };
    }
    if matches!(trimmed, "wake send" | "wake test") {
        return ParsedCommand::WakeSend;
    }
    if let Some(event_id) = trimmed.strip_prefix("wake retire ").map(str::trim) {
        return if event_id.is_empty() || event_id.split_whitespace().count() != 1 {
            ParsedCommand::Unknown
        } else {
            ParsedCommand::WakeRetire(event_id)
        };
    }
    ParsedCommand::Unknown
}

fn execute_command(
    workspace: &Path,
    command: ParsedCommand<'_>,
    out: &mut impl Write,
) -> Result<bool, String> {
    match command {
        ParsedCommand::Status => print_status(workspace, out)?,
        ParsedCommand::Projects => print_projects(workspace, out)?,
        ParsedCommand::WebProjects => print_web_projects(workspace, out)?,
        ParsedCommand::ProjectQueue => print_project_queue(workspace, out)?,
        ParsedCommand::ResumeUsage => {
            writeln!(
                out,
                "Usage: resume <project-id>. Type `projects` for verified IDs."
            )
            .map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::Resume(project_id) => resume_codex_project(workspace, project_id, out)?,
        ParsedCommand::RecoveryUsage => {
            writeln!(
                out,
                "Usage: recovery <project-id>. Type `projects` for verified IDs."
            )
            .map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::Recovery {
            project_id,
            prepare,
        } => print_codex_recovery(workspace, project_id, prepare, out)?,
        ParsedCommand::TargetShow => match read_target(workspace) {
            Ok(target) => {
                writeln!(out, "{}", target.url).map_err(|_| "CLI_OUTPUT_FAILED")?;
                writeln!(out, "generation digest: {}", target.sha256)
                    .map_err(|_| "CLI_OUTPUT_FAILED")?;
            }
            Err(error) => return Err(target_error(error).into()),
        },
        ParsedCommand::TargetSet(url) => {
            let target = update_target(workspace, url)?;
            writeln!(out, "Designated Chat URL updated: {}", target.url)
                .map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::WakeStatus => print_wake_status(out)?,
        ParsedCommand::WakeStart => {
            let store = independent_wake_store()?;
            catdesk_wake::runtime::start_installed(&store)?;
            writeln!(out, "WakeHost start requested.").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::WakePause => {
            catdesk_wake::runtime::control(&independent_wake_store()?, "PAUSED")?;
            writeln!(out, "WakeHost paused.").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::WakeResume => {
            let store = independent_wake_store()?;
            if store.config()?.targets.is_empty() {
                return Err("TARGET_NOT_CONFIGURED".into());
            }
            catdesk_wake::runtime::control(&store, "RUNNING")?;
            catdesk_wake::runtime::start_installed(&store)?;
            writeln!(out, "WakeHost resumed.").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::WakeStop => {
            catdesk_wake::runtime::control(&independent_wake_store()?, "STOPPED")?;
            writeln!(out, "WakeHost stop requested.").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::WakeQueue => print_queue(out)?,
        ParsedCommand::WakeSend => send_manual_test(out)?,
        ParsedCommand::WakeRetire(event_id) => {
            independent_wake_store()?.retire_stale(event_id)?;
            writeln!(out, "Retired stale wake event: {event_id}")
                .map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::Acceptance => print_acceptance(workspace, out)?,
        ParsedCommand::Help => print_help(out)?,
        ParsedCommand::Clear => {
            write!(out, "\x1b[2J\x1b[H").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::Exit => return Ok(false),
        ParsedCommand::Empty => {}
        ParsedCommand::Unknown => {
            writeln!(out, "Unknown command. Type `help`.").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
    }
    Ok(true)
}

#[cfg(windows)]
struct CliMutex(*mut std::ffi::c_void);
#[cfg(windows)]
impl Drop for CliMutex {
    fn drop(&mut self) {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn acquire_cli_mutex() -> Result<Option<CliMutex>, String> {
    type Handle = *mut std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(attributes: *mut std::ffi::c_void, owner: i32, name: *const u16) -> Handle;
        fn GetLastError() -> u32;
        fn SetConsoleTitleW(title: *const u16) -> i32;
    }
    let name = wide(CLI_MUTEX_NAME);
    let mutex = unsafe { CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr()) };
    if mutex.is_null() {
        return Err("BINAGOTCHY_CLI_SINGLE_INSTANCE_UNAVAILABLE".into());
    }
    if unsafe { GetLastError() } == 183 {
        unsafe {
            let title = wide("CatDesk Binagotchy - already running");
            SetConsoleTitleW(title.as_ptr());
        }
        unsafe {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn CloseHandle(handle: Handle) -> i32;
            }
            CloseHandle(mutex);
        }
        return Ok(None);
    }
    unsafe {
        let title = wide("CatDesk Binagotchy");
        SetConsoleTitleW(title.as_ptr());
    }
    Ok(Some(CliMutex(mutex)))
}

pub fn run_cli(workspace: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let _mutex = match acquire_cli_mutex()? {
        Some(mutex) => mutex,
        None => return Ok(()),
    };

    let mut stdout = io::stdout().lock();
    writeln!(stdout, "CatDesk Binagotchy - command companion").map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        stdout,
        "Independent WakeHost control; this shell does not start a second CatDesk daemon."
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(stdout, "Type `help` for commands.\n").map_err(|_| "CLI_OUTPUT_FAILED")?;

    // This console is a presentation/explicit-control boundary, not a
    // background supervisor. Never start WakeHost merely by opening the
    // console, even if a persisted desired state says RUNNING. This makes
    // target readback and a guarded paired target update safe to perform
    // while the independent browser host is deliberately stopped.
    // Only explicit WakeStart/WakeResume commands may launch WakeHost.
    print_status(workspace, &mut stdout)?;
    writeln!(stdout).map_err(|_| "CLI_OUTPUT_FAILED")?;
    stdout.flush().map_err(|_| "CLI_OUTPUT_FAILED")?;

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        write!(stdout, "binagotchy> ").map_err(|_| "CLI_OUTPUT_FAILED")?;
        stdout.flush().map_err(|_| "CLI_OUTPUT_FAILED")?;
        let Some(line) = lines.next() else {
            break;
        };
        let line = line.map_err(|_| "CLI_INPUT_FAILED")?;
        match execute_command(workspace, parse_command(&line), &mut stdout) {
            Ok(true) => {}
            Ok(false) => break,
            Err(error) => {
                writeln!(stdout, "ERROR: {error}").map_err(|_| "CLI_OUTPUT_FAILED")?;
            }
        }
        stdout.flush().map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BINAGOTCHY_CLI_MODE_FLAG, ParsedCommand, ScopedCodexVerb, normalized_manual_message,
        parse_binagotchy_cli_mode, parse_command, print_projects, project_task_rows,
        render_codex_recovery_report, render_operator_codex_queue, scheduler_cursor,
        scheduler_leases, scoped_codex_args, validated_wake_predecessor_digest,
    };
    use crate::delegated::autonomy_projects::{
        AutonomousProjectRegistryV1, AutonomousProjectV1, PROJECT_REGISTRY_SCHEMA_VERSION,
        project_chat_target_digest,
    };
    use sha2::Digest;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use uuid::Uuid;

    fn web_report() -> serde_json::Value {
        let digest = "a".repeat(64);
        let projects = ["alpha", "beta", "gamma"].map(|id| serde_json::json!({
            "projectId":id, "conversationHistory":["private-predecessor"],
            "wake":{"status":"PAIRED","target":{"projectId":id,"generation":2,
                "sha256":digest,"url":"https://chatgpt.com/c/private-current"}},
            "context":{"status":"CURRENT_ADVISORY","advisory":true,"currentPressure":{
                "key":{"projectId":id,"conversationUrl":"https://chatgpt.com/c/private-current","targetGeneration":2},
                "targetSha256":digest,"isCurrent":true,"advisory":true,
                "observedEstimateTokens":1234,"coverageKnown":false,"triggerTokens":50000}},
            "queue":{"queuedCount":1},
            "rollover":{"phase":if id == "beta" {"QUARANTINED"} else {"NONE"},
                "automation":{"stage":"WAKE_STOPPED","targetGeneration":2,"tickUtc":123}}
        }));
        serde_json::json!({"action":"web_status","readOnly":true,
            "runtimeMode":"DEVELOPMENT_UNPROTECTED",
            "nativeThreadId":"private-native-thread","workspace":"private-workspace",
            "webProjects":{"status":"SOURCE_STATUS_ONLY","readOnly":true,"projects":projects}})
    }

    #[test]
    fn web_commands_are_read_only_exact_matches() {
        for command in ["web projects", "web status", "  web status  "] {
            assert_eq!(parse_command(command), ParsedCommand::WebProjects);
        }
        for command in [
            "web",
            "web start",
            "web status alpha",
            "web projects; wake start",
        ] {
            assert_eq!(parse_command(command), ParsedCommand::Unknown);
        }
    }

    #[test]
    fn web_view_keeps_waiting_projects_visible_and_hides_private_content() {
        let mut report = web_report();
        report["webProjects"]["projects"][2]["projectId"] = serde_json::json!("bad\u{001b}[31m");
        let mut out = Vec::new();
        super::render_web_projects(&report, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        for expected in [
            "Web projects: 3",
            "alpha | Wake PAIRED",
            "beta | Wake PAIRED",
            "saved rollover: QUARANTINED",
            "coverage UNKNOWN",
            "observed 1234 tokens (advisory)",
            "SAME_GENERATION_SNAPSHOT",
            "tick 123",
            "[redacted]",
        ] {
            assert!(text.contains(expected), "{expected}: {text}");
        }
        for private in ["https://", "private-", "\u{001b}", "%"] {
            assert!(!text.contains(private), "{text}");
        }
    }

    #[test]
    fn web_view_never_attributes_stale_context_or_scheduler_to_current_chat() {
        let baseline = web_report();
        for (pointer, replacement) in [
            ("/wake/status", serde_json::json!("ATTENTION")),
            ("/wake/target/generation", serde_json::json!(3)),
            ("/wake/target/sha256", serde_json::json!("b".repeat(64))),
            (
                "/context/currentPressure/key/projectId",
                serde_json::json!("other"),
            ),
            (
                "/context/currentPressure/key/conversationUrl",
                serde_json::json!("other"),
            ),
            (
                "/context/currentPressure/isCurrent",
                serde_json::json!(false),
            ),
            (
                "/context/currentPressure/advisory",
                serde_json::json!(false),
            ),
        ] {
            let mut report = baseline.clone();
            *report["webProjects"]["projects"][0]
                .pointer_mut(pointer)
                .unwrap() = replacement;
            report["webProjects"]["projects"]
                .as_array_mut()
                .unwrap()
                .truncate(1);
            let mut out = Vec::new();
            super::render_web_projects(&report, &mut out).unwrap();
            let text = String::from_utf8(out).unwrap();
            assert!(text.contains("context: unavailable"), "{pointer}: {text}");
            assert!(!text.contains("observed 1234"));
        }
        let mut report = baseline;
        report["webProjects"]["projects"][0]["rollover"]["automation"]["targetGeneration"] =
            serde_json::json!(1);
        let mut out = Vec::new();
        super::render_web_projects(&report, &mut out).unwrap();
        assert!(
            String::from_utf8(out)
                .unwrap()
                .contains("STALE_OR_UNVERIFIED_SNAPSHOT")
        );
    }

    #[test]
    fn web_view_bounds_output_and_refuses_invalid_reports_before_output() {
        let mut report = web_report();
        let project = report["webProjects"]["projects"][0].clone();
        report["webProjects"]["projects"] = serde_json::json!(vec![project; 30]);
        let mut out = Vec::new();
        super::render_web_projects(&report, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("6 additional Web project(s) omitted"));
        assert_eq!(text.matches("observed 1234").count(), 24);
        for pointer in [
            "/action",
            "/readOnly",
            "/webProjects/status",
            "/webProjects/projects",
        ] {
            let mut invalid = report.clone();
            *invalid.pointer_mut(pointer).unwrap() = serde_json::Value::Null;
            let mut out = Vec::new();
            assert!(super::render_web_projects(&invalid, &mut out).is_err());
            assert!(out.is_empty());
        }
    }

    fn remove_fixture_tree(path: &Path) {
        let target = path
            .canonicalize()
            .expect("canonical fixture cleanup target");
        let temp = std::env::temp_dir()
            .canonicalize()
            .expect("canonical temporary directory");
        assert!(target.starts_with(&temp) && target != temp);
        assert!(
            target
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("catdesk-binagotchy-"))
        );
        fs::remove_dir_all(target).expect("cleanup fixture");
    }

    #[test]
    fn cli_mode_is_closed_and_zero_parameter() {
        assert_eq!(
            parse_binagotchy_cli_mode(&[BINAGOTCHY_CLI_MODE_FLAG.to_string()]),
            Ok(true)
        );
        assert_eq!(parse_binagotchy_cli_mode(&[]), Ok(false));
        assert!(
            parse_binagotchy_cli_mode(&[
                BINAGOTCHY_CLI_MODE_FLAG.to_string(),
                "attacker-input".into()
            ])
            .is_err()
        );
        assert!(
            parse_binagotchy_cli_mode(&[
                BINAGOTCHY_CLI_MODE_FLAG.to_string(),
                BINAGOTCHY_CLI_MODE_FLAG.to_string()
            ])
            .is_err()
        );
    }

    #[test]
    fn command_parser_keeps_mutation_surfaces_explicit() {
        assert_eq!(parse_command("status"), ParsedCommand::Status);
        assert_eq!(parse_command("projects"), ParsedCommand::Projects);
        assert_eq!(parse_command("queue"), ParsedCommand::ProjectQueue);
        assert_eq!(parse_command("queue all"), ParsedCommand::Unknown);
        assert_eq!(parse_command("resume"), ParsedCommand::ResumeUsage);
        assert_eq!(
            parse_command("resume alpha"),
            ParsedCommand::Resume("alpha")
        );
        assert_eq!(parse_command("recovery"), ParsedCommand::RecoveryUsage);
        assert_eq!(
            parse_command("recovery alpha"),
            ParsedCommand::Recovery {
                project_id: "alpha",
                prepare: false
            }
        );
        assert_eq!(
            parse_command("recovery prepare alpha"),
            ParsedCommand::Recovery {
                project_id: "alpha",
                prepare: true
            }
        );
        for invalid in [
            "resume alpha beta",
            "resume alpha;rm",
            "resume --all",
            "resume ../alpha",
            "recovery alpha beta",
            "recovery prepare alpha;rm",
            "recovery ../alpha",
        ] {
            assert_eq!(parse_command(invalid), ParsedCommand::Unknown);
        }
        assert_eq!(parse_command("target"), ParsedCommand::TargetShow);
        assert_eq!(
            parse_command("target set https://chatgpt.com/c/example"),
            ParsedCommand::TargetSet("https://chatgpt.com/c/example")
        );
        assert_eq!(parse_command("wake start"), ParsedCommand::WakeStart);
        assert_eq!(parse_command("wake queue"), ParsedCommand::WakeQueue);
        assert_eq!(parse_command("wake send"), ParsedCommand::WakeSend);
        assert_eq!(
            parse_command("wake send hello world"),
            ParsedCommand::Unknown
        );
        assert_eq!(
            parse_command("wake retire old-event-1"),
            ParsedCommand::WakeRetire("old-event-1")
        );
        assert_eq!(parse_command("rm -rf ."), ParsedCommand::Unknown);
    }

    #[test]
    fn codex_delegate_args_are_fixed_and_scoped() {
        let project = AutonomousProjectV1 {
            project_id: "alpha".into(),
            workspace: std::env::temp_dir().join("catdesk-binagotchy-alpha space"),
            git_identity: "fixture".into(),
            verification_profile: "cargo-test".into(),
            codex_thread_id: Some(Uuid::new_v4().to_string()),
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
            chatgpt_target_history: Vec::new(),
        };
        let workspace = project.workspace.to_str().expect("Unicode fixture");
        assert_eq!(
            scoped_codex_args(&project, ScopedCodexVerb::Resume).unwrap(),
            [
                "operator",
                "codex",
                "resume",
                "--project-id",
                "alpha",
                "--workspace",
                workspace
            ]
        );
        assert_eq!(
            scoped_codex_args(&project, ScopedCodexVerb::RecoveryStatus).unwrap(),
            [
                "operator",
                "codex",
                "recovery",
                "status",
                "--project-id",
                "alpha",
                "--workspace",
                workspace
            ]
        );
        assert_eq!(
            scoped_codex_args(&project, ScopedCodexVerb::RecoveryPrepare).unwrap(),
            [
                "operator",
                "codex",
                "recovery",
                "prepare",
                "--project-id",
                "alpha",
                "--workspace",
                workspace
            ]
        );
    }

    #[test]
    fn recovery_console_caps_rows_and_rejects_cross_project_or_launch_authority() {
        let project = AutonomousProjectV1 {
            project_id: "alpha".into(),
            workspace: std::env::temp_dir().join("catdesk-binagotchy-report"),
            git_identity: "fixture".into(),
            verification_profile: "cargo-test".into(),
            codex_thread_id: None,
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
            chatgpt_target_history: Vec::new(),
        };
        let blockers = (0..20)
            .map(|index| {
                serde_json::json!({
                    "sessionId":format!("session_{index}"),
                    "taskId":format!("task_{index}"),
                    "taskState":"WORKER_RUNNING"
                })
            })
            .collect::<Vec<_>>();
        let mut report = serde_json::json!({
            "projectId":"alpha", "readOnly":true, "launchAuthorized":false,
            "status":"NEEDS_OWNER_RECOVERY", "observedSessionCount":20,
            "sessionBlockers":[], "queueBlockers":blockers,
            "accountingReviewRows":[], "pendingWriterMarker":{"present":false},
            "requiredEvidence":["Inspect owner process."]
        });
        let mut out = Vec::new();
        render_codex_recovery_report(&project, &report, true, &mut out).unwrap();
        let rendered = String::from_utf8(out).unwrap();
        assert!(rendered.contains("queue blockers: 20"));
        assert!(rendered.contains("12 additional row(s) omitted"));
        assert!(rendered.contains("required: Inspect owner process."));
        assert!(rendered.contains("no launch was authorized"));
        assert!(!rendered.contains("task_8"));
        report["queueBlockers"][0]["sessionId"] = serde_json::json!("bad\u{001b}[31m");
        let mut out = Vec::new();
        render_codex_recovery_report(&project, &report, false, &mut out).unwrap();
        let rendered = String::from_utf8(out).unwrap();
        assert!(rendered.contains("session=[redacted]"));
        assert!(!rendered.contains('\u{001b}'));
        report["projectId"] = serde_json::json!("beta");
        assert!(render_codex_recovery_report(&project, &report, false, &mut Vec::new()).is_err());
        report["projectId"] = serde_json::json!("alpha");
        report["launchAuthorized"] = serde_json::json!(true);
        assert!(render_codex_recovery_report(&project, &report, false, &mut Vec::new()).is_err());
    }

    #[test]
    fn operator_queue_console_is_bounded_and_hides_prompt_and_unsafe_ids() {
        let root = std::env::temp_dir().join("catdesk-binagotchy-operator-view");
        let registry = AutonomousProjectRegistryV1 {
            schema_version: PROJECT_REGISTRY_SCHEMA_VERSION,
            maximum_codex_workers: 1,
            maximum_qwen_workers: 1,
            projects: Vec::new(),
        };
        let jobs = (0..30)
            .map(|index| {
                serde_json::json!({
                    "projectId":"alpha", "taskId":format!("task_{index}"),
                    "workspace":root, "threadId":"secret-thread",
                    "targetSha256":null, "state":"READY",
                    "prompt":"secret prompt must never appear"
                })
            })
            .collect::<Vec<_>>();
        let mut report = serde_json::json!({"action":"codex_queue_status", "jobs":jobs});
        report["jobs"][0]["taskId"] = serde_json::json!("bad\u{001b}[31m");
        let mut out = Vec::new();
        render_operator_codex_queue(&root, &registry, &report, &mut out).unwrap();
        let rendered = String::from_utf8(out).unwrap();
        assert!(rendered.contains("operator Codex queue: 30 job(s)"));
        assert!(rendered.contains("6 additional operator job(s) omitted"));
        assert!(rendered.contains("alpha / [redacted]"));
        assert!(!rendered.contains("task_24"));
        assert!(!rendered.contains("secret prompt"));
        assert!(!rendered.contains('\u{001b}'));
        report["action"] = serde_json::json!("codex_queue_dispatch");
        assert!(render_operator_codex_queue(&root, &registry, &report, &mut Vec::new()).is_err());
    }

    #[test]
    fn operator_queue_view_requires_full_current_binding_without_exposing_origin() {
        let fixture =
            std::env::temp_dir().join(format!("catdesk-binagotchy-binding-{}", Uuid::new_v4()));
        let control = fixture.join("control");
        let alpha = fixture.join("alpha");
        for (root, origin) in [
            (&control, "https://example.test/control.git"),
            (&alpha, "https://example.test/alpha.git"),
        ] {
            fs::create_dir_all(root).expect("fixture Git root");
            for args in [
                &["init", "-q"][..],
                &["remote", "add", "origin", origin][..],
            ] {
                assert!(
                    Command::new("git")
                        .arg("-C")
                        .arg(root)
                        .args(args)
                        .status()
                        .expect("fixture Git")
                        .success()
                );
            }
        }
        let control = control.canonicalize().expect("control root");
        let alpha = alpha.canonicalize().expect("alpha root");
        let target = "https://chatgpt.com/c/6ac6cbe8-6f0c-83e9-9f7d-13489d4d87f5";
        let project = AutonomousProjectV1 {
            project_id: "alpha".into(),
            workspace: alpha.clone(),
            git_identity: "https://example.test/alpha.git".into(),
            verification_profile: "python_uv_group_dev_v1".into(),
            codex_thread_id: Some(Uuid::new_v4().to_string()),
            chatgpt_target_url: Some(target.into()),
            chatgpt_target_sha256: Some(project_chat_target_digest(target)),
            chatgpt_target_history: Vec::new(),
        };
        let registry = AutonomousProjectRegistryV1 {
            schema_version: PROJECT_REGISTRY_SCHEMA_VERSION,
            maximum_codex_workers: 1,
            maximum_qwen_workers: 1,
            projects: vec![
                AutonomousProjectV1 {
                    project_id: "catdesk".into(),
                    workspace: control.clone(),
                    git_identity: "https://example.test/control.git".into(),
                    verification_profile: "rust_full".into(),
                    codex_thread_id: Some(Uuid::new_v4().to_string()),
                    chatgpt_target_url: None,
                    chatgpt_target_sha256: None,
                    chatgpt_target_history: Vec::new(),
                },
                project.clone(),
            ],
        };
        let registry_root = control.join(".catdesk/projects");
        fs::create_dir_all(&registry_root).expect("registry directory");
        let registry_bytes = serde_json::to_vec(&registry).expect("registry JSON");
        fs::write(registry_root.join("projects.json"), &registry_bytes).expect("registry fixture");
        let original_job = serde_json::json!({
            "projectId":"alpha", "taskId":"one", "state":"READY",
            "workspace":project.workspace, "threadId":project.codex_thread_id,
            "targetSha256":project.chatgpt_target_sha256,
            "verificationProfile":project.verification_profile,
            "gitIdentitySha256":format!("{:x}", sha2::Sha256::digest(project.git_identity.as_bytes())),
        });
        let mut report = serde_json::json!({"action":"codex_queue_status", "jobs":[original_job]});
        assert!(super::operator_job_matches_project_binding(
            &project,
            &report["jobs"][0]
        ));
        assert_eq!(
            super::resolve_registered_project_from_control_root(&control, "alpha")
                .expect("registered fixture scope")
                .project,
            project
        );
        let mut out = Vec::new();
        render_operator_codex_queue(&control, &registry, &report, &mut out).expect("bound report");
        let rendered = String::from_utf8(out).expect("UTF-8 output");
        assert!(
            rendered.contains("alpha / one  state=READY  identity=VERIFIED"),
            "{rendered}"
        );
        assert!(!rendered.contains(&project.git_identity));

        for (field, replacement) in [
            ("verificationProfile", serde_json::json!("rust_full")),
            ("gitIdentitySha256", serde_json::json!("0".repeat(64))),
            (
                "verificationProfile",
                serde_json::json!(["python_uv_group_dev_v1"]),
            ),
            ("gitIdentitySha256", serde_json::Value::Null),
            ("threadId", serde_json::Value::Null),
            ("workspace", serde_json::json!([])),
            ("targetSha256", serde_json::Value::Null),
        ] {
            let saved = report["jobs"][0][field].clone();
            report["jobs"][0][field] = replacement;
            let mut out = Vec::new();
            render_operator_codex_queue(&control, &registry, &report, &mut out)
                .expect("drifted report");
            assert!(
                String::from_utf8(out)
                    .expect("UTF-8 output")
                    .contains("alpha / one  state=READY  identity=UNVERIFIED")
            );
            report["jobs"][0][field] = saved;
        }
        for field in [
            "projectId",
            "workspace",
            "threadId",
            "targetSha256",
            "verificationProfile",
            "gitIdentitySha256",
        ] {
            let mut missing = report.clone();
            missing["jobs"][0]
                .as_object_mut()
                .expect("job object")
                .remove(field);
            let mut out = Vec::new();
            render_operator_codex_queue(&control, &registry, &missing, &mut out)
                .expect("missing binding field");
            assert!(
                String::from_utf8(out)
                    .expect("UTF-8 output")
                    .contains("identity=UNVERIFIED")
            );
        }
        assert_eq!(
            fs::read(registry_root.join("projects.json")).expect("unchanged registry"),
            registry_bytes
        );
        assert!(!alpha.join(".catdesk").exists());
        remove_fixture_tree(&fixture);
    }

    #[test]
    fn codex_commands_refuse_unverified_registry_before_launch_or_report() {
        let root =
            std::env::temp_dir().join(format!("catdesk-binagotchy-unverified-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join(".catdesk/projects")).unwrap();
        let project = AutonomousProjectV1 {
            project_id: "alpha".into(),
            workspace: root.clone(),
            git_identity: "unknown-git-identity".into(),
            verification_profile: "cargo-test".into(),
            codex_thread_id: Some(Uuid::new_v4().to_string()),
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
            chatgpt_target_history: Vec::new(),
        };
        let registry = AutonomousProjectRegistryV1 {
            schema_version: PROJECT_REGISTRY_SCHEMA_VERSION,
            maximum_codex_workers: 1,
            maximum_qwen_workers: 1,
            projects: vec![project],
        };
        fs::write(
            root.join(".catdesk/projects/projects.json"),
            serde_json::to_vec(&registry).unwrap(),
        )
        .unwrap();
        assert!(super::resume_codex_project(&root, "alpha", &mut Vec::new()).is_err());
        assert!(super::print_codex_recovery(&root, "alpha", false, &mut Vec::new()).is_err());
        assert!(!root.join(".catdesk/autonomy").exists());
        remove_fixture_tree(&root);
    }

    #[test]
    fn projects_view_redacts_targets_threads_and_git_identity() {
        let root =
            std::env::temp_dir().join(format!("catdesk-binagotchy-projects-{}", Uuid::new_v4()));
        let registry_root = root.join(".catdesk/projects");
        fs::create_dir_all(&registry_root).expect("registry directory");
        let target = "https://chatgpt.com/c/6ac6cbe8-6f0c-83e9-9f7d-13489d4d87f5";
        let registry = AutonomousProjectRegistryV1 {
            schema_version: PROJECT_REGISTRY_SCHEMA_VERSION,
            maximum_codex_workers: 1,
            maximum_qwen_workers: 1,
            projects: vec![AutonomousProjectV1 {
                project_id: "catdesk".into(),
                workspace: root.clone(),
                git_identity: "secret-git-identity".into(),
                verification_profile: "cargo-test".into(),
                codex_thread_id: Some("secret-thread-id".into()),
                chatgpt_target_url: Some(target.into()),
                chatgpt_target_sha256: Some(project_chat_target_digest(target)),
                chatgpt_target_history: Vec::new(),
            }],
        };
        fs::write(
            registry_root.join("projects.json"),
            serde_json::to_vec(&registry).expect("encode"),
        )
        .expect("registry fixture");
        let mut out = Vec::new();
        print_projects(&root, &mut out).expect("projects readback");
        let rendered = String::from_utf8(out).expect("utf8");
        assert!(rendered.contains("catdesk  identity=UNVERIFIED  workspace="));
        assert!(rendered.contains("Codex thread recorded=true"));
        assert!(!rendered.contains("secret-git-identity"));
        assert!(!rendered.contains("secret-thread-id"));
        assert!(!rendered.contains(target));
        remove_fixture_tree(&root);
    }

    #[test]
    fn queue_reader_keeps_task_ids_scoped_and_detects_wrong_contract() {
        let root =
            std::env::temp_dir().join(format!("catdesk-binagotchy-queue-{}", Uuid::new_v4()));
        let session = root.join(".catdesk/autonomy/session-one");
        fs::create_dir_all(&session).expect("session directory");
        let project = AutonomousProjectV1 {
            project_id: "catdesk".into(),
            workspace: root.canonicalize().expect("canonical workspace"),
            git_identity: "https://example.test/catdesk.git".into(),
            verification_profile: "cargo-test".into(),
            codex_thread_id: None,
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
            chatgpt_target_history: Vec::new(),
        };
        fs::write(session.join("state.json"), br#"{"schemaVersion":1,"sessionId":"session-one","state":"QUEUED","currentTaskId":"task-one","active":true}"#)
            .expect("state fixture");
        let valid_contract = serde_json::json!({
            "projectId": "catdesk",
            "workspace": root,
            "expectedOrigin": project.git_identity,
        });
        fs::write(session.join("contract.json"), valid_contract.to_string())
            .expect("contract fixture");
        fs::write(session.join("queue.json"), br#"{"schemaVersion":1,"tasks":[{"taskId":"task-one","priority":1,"dependsOn":[],"state":"READY"}]}"#)
            .expect("queue fixture");
        let (rows, omitted, rejected) = project_task_rows(&project).expect("scoped queue");
        assert_eq!(omitted, 0);
        assert_eq!(rejected, 0);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains("catdesk / session-one  task=task-one"));
        let wrong_project = serde_json::json!({
            "projectId": "another-project", "workspace": root,
            "expectedOrigin": project.git_identity,
        });
        fs::write(session.join("contract.json"), wrong_project.to_string()).expect("wrong project");
        let (rows, _, rejected) = project_task_rows(&project).expect("wrong project skipped");
        assert!(rows.is_empty());
        assert_eq!(rejected, 1);
        let wrong_workspace = serde_json::json!({
            "projectId": "catdesk", "workspace": root.join("sibling"),
            "expectedOrigin": project.git_identity,
        });
        fs::create_dir_all(root.join("sibling")).expect("sibling fixture");
        fs::write(session.join("contract.json"), wrong_workspace.to_string())
            .expect("wrong workspace");
        let (rows, _, rejected) = project_task_rows(&project).expect("wrong workspace skipped");
        assert!(rows.is_empty());
        assert_eq!(rejected, 1);
        let wrong_origin = serde_json::json!({
            "projectId": "catdesk", "workspace": root,
            "expectedOrigin": "https://example.test/sibling.git",
        });
        fs::write(session.join("contract.json"), wrong_origin.to_string()).expect("wrong origin");
        let (rows, _, rejected) = project_task_rows(&project).expect("wrong origin skipped");
        assert!(rows.is_empty());
        assert_eq!(rejected, 1);
        remove_fixture_tree(&root);
    }

    #[test]
    fn project_readers_reject_redirected_or_nondirectory_catdesk_parent() {
        let root =
            std::env::temp_dir().join(format!("catdesk-binagotchy-parent-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("workspace fixture");
        let project = AutonomousProjectV1 {
            project_id: "catdesk".into(),
            workspace: root.canonicalize().expect("canonical workspace"),
            git_identity: "https://example.test/catdesk.git".into(),
            verification_profile: "cargo-test".into(),
            codex_thread_id: None,
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
            chatgpt_target_history: Vec::new(),
        };
        fs::write(root.join(".catdesk"), b"not a directory").expect("nondirectory parent");
        assert_eq!(
            project_task_rows(&project).unwrap_err(),
            "PROJECT_QUEUE_UNAVAILABLE"
        );
        assert_eq!(
            super::registered_projects(&root).unwrap_err(),
            "PROJECT_REGISTRY_UNAVAILABLE"
        );
        fs::remove_file(root.join(".catdesk")).expect("remove file fixture");

        let outside =
            std::env::temp_dir().join(format!("catdesk-binagotchy-outside-{}", Uuid::new_v4()));
        fs::create_dir_all(&outside).expect("outside fixture");
        #[cfg(windows)]
        let link = std::os::windows::fs::symlink_dir(&outside, root.join(".catdesk"));
        #[cfg(not(windows))]
        let link = std::os::unix::fs::symlink(&outside, root.join(".catdesk"));
        if link.is_ok() {
            assert_eq!(
                project_task_rows(&project).unwrap_err(),
                "PROJECT_QUEUE_UNAVAILABLE"
            );
            assert_eq!(
                super::registered_projects(&root).unwrap_err(),
                "PROJECT_REGISTRY_UNAVAILABLE"
            );
            #[cfg(windows)]
            fs::remove_dir(root.join(".catdesk")).expect("remove link fixture");
            #[cfg(not(windows))]
            fs::remove_file(root.join(".catdesk")).expect("remove link fixture");
        }
        remove_fixture_tree(&root);
        remove_fixture_tree(&outside);
    }

    #[test]
    fn scheduler_view_rejects_unregistered_cursor_without_writing_state() {
        let root =
            std::env::temp_dir().join(format!("catdesk-binagotchy-scheduler-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("scheduler directory");
        let registry = AutonomousProjectRegistryV1 {
            schema_version: PROJECT_REGISTRY_SCHEMA_VERSION,
            maximum_codex_workers: 1,
            maximum_qwen_workers: 1,
            projects: vec![AutonomousProjectV1 {
                project_id: "catdesk".into(),
                workspace: root.clone(),
                git_identity: "fixture-git".into(),
                verification_profile: "cargo-test".into(),
                codex_thread_id: None,
                chatgpt_target_url: None,
                chatgpt_target_sha256: None,
                chatgpt_target_history: Vec::new(),
            }],
        };
        assert_eq!(
            scheduler_cursor(&root, &registry).expect("empty cursor"),
            None
        );
        assert!(
            scheduler_leases(&root, &registry)
                .expect("absent leases directory means zero leases")
                .is_empty()
        );
        fs::write(
            root.join("scheduler-cursor.json"),
            br#"{"schemaVersion":1,"lastGrantedProjectId":"sibling"}"#,
        )
        .expect("cursor fixture");
        assert_eq!(
            scheduler_cursor(&root, &registry).unwrap_err(),
            "SCHEDULER_CURSOR_UNAVAILABLE"
        );
        remove_fixture_tree(&root);
    }

    #[test]
    fn presentation_launch_cannot_implicitly_restart_an_operator_stopped_wakehost() {
        // Limit source assertions to product code. Literals in this test
        // must not accidentally satisfy or invalidate their own assertions.
        let source = include_str!("binagotchy_cli.rs");
        let production = source.split("mod tests {").next().expect("product source");
        // The explicit command handlers are the sole host-start owners.
        // Merely opening a console or issuing 'target set' must not start
        // independent WakeHost, even if stored desired state is RUNNING.
        let startup = production
            .split("pub fn run_cli(")
            .nth(1)
            .expect("CLI entry point");
        assert!(!startup.contains("start_installed("));
        assert!(!startup.contains("start_installed_only_if_desired_running("));
        assert!(production.contains("ParsedCommand::WakeStart"));
        assert!(production.contains("ParsedCommand::WakeResume"));
        assert!(production.contains("catdesk_wake::runtime::start_installed(&store)?"));
    }

    #[test]
    fn manual_test_message_is_unambiguously_not_natural_acceptance() {
        let message = normalized_manual_message();
        assert_eq!(
            message,
            "MANUAL WAKE DEBUG - CatDesk diagnostic test - NOT natural acceptance."
        );
        assert!(message.contains("NOT natural acceptance"));
    }

    #[test]
    fn rollover_fallback_only_accepts_canonical_integrity_verified_wake_identity() {
        let old = "https://chatgpt.com/c/6ac6cbe8-6f0c-83e9-9f7d-13489d4d87f5";
        let digest = crate::delegated::autonomy_projects::project_chat_target_digest(old);
        assert_eq!(
            validated_wake_predecessor_digest(old, &digest),
            Ok(digest.clone())
        );
        assert!(validated_wake_predecessor_digest(old, &"8".repeat(64)).is_err());
        assert!(
            validated_wake_predecessor_digest(
                "https://example.com/c/6ac6cbe8-6f0c-83e9-9f7d-13489d4d87f5",
                &digest
            )
            .is_err()
        );
    }

    #[test]
    fn cli_source_never_constructs_daemon_or_tunnel_state() {
        let source = include_str!("binagotchy_cli.rs");
        for forbidden in [
            ["AppState", "::new("].concat(),
            ["start", "_services("].concat(),
            ["run_native", "_daemon("].concat(),
            ["ngrok::", "start_transport("].concat(),
            ["openai", "_tunnel"].concat(),
        ] {
            assert!(
                !source.contains(&forbidden),
                "unexpected ownership: {forbidden}"
            );
        }
    }
}
