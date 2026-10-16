//! Current-user Codex CLI access for an explicitly registered project.
//!
//! This local operator route is separate from CatDesk's autonomous worker and
//! independent WakeHost. It never reads Codex credentials or launches through
//! a shell. The native CLI owns its normal session and approval behavior.

mod queue;
mod recovery;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::delegated::autonomy_projects::{
    AutonomousProjectV1, resolve_registered_project_from_control_root,
};
use crate::delegated::autonomy_runtime::{
    ProjectWriterGuard, acquire_project_writer_lock, operator_codex_home, resolve_codex_executable,
};
use crate::delegated::autonomy_state::{
    AutonomousQueueTaskStateV1, AutonomousSessionStateV1, AutonomousStateStoreV1,
};

const MAX_TASK_BYTES: usize = 24 * 1024;
const MAX_PROFILE_BYTES: usize = 128;
const CODEX_PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const CODEX_PROMPT_TIMEOUT: Duration = Duration::from_secs(30);
const CODEX_KILL_TIMEOUT: Duration = Duration::from_secs(20);
// Direct `run` is noninteractive. Leave the native, user-driven `resume` TUI
// open until its user exits it, but bound a stalled `exec resume -` turn.
const CODEX_RUN_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexScope {
    project_id: String,
    workspace: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperatorCodexAction {
    Queue(queue::QueueAction),
    Status(CodexScope),
    Recovery {
        scope: CodexScope,
        prepare: bool,
    },
    Run {
        scope: CodexScope,
        task: String,
        model: Option<String>,
        effort: Option<String>,
    },
    Resume {
        scope: CodexScope,
        model: Option<String>,
        effort: Option<String>,
    },
    /// Explicit native picker across the current user's sessions. This has
    /// no project claim or CatDesk workspace-writer lock.
    GlobalResumeAll,
}

/// `operator codex` is a local entry point. Project-scoped actions name both
/// the registered project and its approved workspace. The sole exception is
/// the explicit, manual `resume --all` native picker; it claims no project.
pub(crate) fn parse_action(args: &[String]) -> Result<OperatorCodexAction, String> {
    let verb = args.get(2).map(String::as_str).ok_or_else(|| {
        "CODEX_USAGE: expected status, run, resume, or recovery with --project-id and --workspace".to_string()
    })?;
    if verb == "queue" {
        return queue::parse_action(args).map(OperatorCodexAction::Queue);
    }
    if !matches!(verb, "status" | "run" | "resume" | "recovery") {
        return Err("CODEX_USAGE: expected status, run, resume, or recovery".into());
    }
    let recovery_prepare = if verb == "recovery" {
        match args.get(3).map(String::as_str) {
            Some("status") => Some(false),
            Some("prepare") => Some(true),
            _ => return Err("CODEX_USAGE: recovery accepts status or prepare".into()),
        }
    } else {
        None
    };
    let mut project_id = None;
    let mut workspace = None;
    let mut task = None;
    let mut model = None;
    let mut effort = None;
    let mut all = false;
    let mut index = if recovery_prepare.is_some() { 4 } else { 3 };
    while index < args.len() {
        let flag = args[index].as_str();
        if flag == "--all" {
            if all {
                return Err("CODEX_USAGE: --all may only be supplied once".into());
            }
            all = true;
            index += 1;
            continue;
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| "CODEX_USAGE: an option requires a value".to_string())?;
        if value.is_empty() || value.len() > MAX_TASK_BYTES {
            return Err("CODEX_USAGE: an option value is empty or too large".into());
        }
        let slot = match flag {
            "--project-id" => &mut project_id,
            "--workspace" => &mut workspace,
            "--task" => &mut task,
            "--model" => &mut model,
            "--effort" => &mut effort,
            _ => return Err("CODEX_USAGE: unsupported option".into()),
        };
        if slot.replace(value.clone()).is_some() {
            return Err(format!("CODEX_USAGE: {flag} may only be supplied once"));
        }
        index += 2;
    }
    if verb == "resume" && all {
        return if project_id.is_none()
            && workspace.is_none()
            && task.is_none()
            && model.is_none()
            && effort.is_none()
        {
            Ok(OperatorCodexAction::GlobalResumeAll)
        } else {
            Err("CODEX_USAGE: resume --all is a global manual picker and accepts no project, workspace, task, or profile options".into())
        };
    }
    let scope = CodexScope {
        project_id: project_id.ok_or("CODEX_USAGE: --project-id is required")?,
        workspace: PathBuf::from(workspace.ok_or("CODEX_USAGE: --workspace is required")?),
    };
    if !scope.workspace.is_absolute() {
        return Err("CODEX_SCOPE: --workspace must be an absolute path".into());
    }
    if model.as_deref().is_some_and(|value| !valid_model(value)) {
        return Err("CODEX_USAGE: --model has an invalid identifier".into());
    }
    if effort.as_deref().is_some_and(|value| !valid_effort(value)) {
        return Err(
            "CODEX_USAGE: --effort must be minimal, low, medium, high, xhigh, max, or ultra".into(),
        );
    }
    match verb {
        "recovery" if task.is_none() && model.is_none() && effort.is_none() && !all => {
            Ok(OperatorCodexAction::Recovery {
                scope,
                prepare: recovery_prepare.expect("recovery verb has an action"),
            })
        }
        "status" if task.is_none() && model.is_none() && effort.is_none() && !all => {
            Ok(OperatorCodexAction::Status(scope))
        }
        "run" if !all => {
            let task = task.ok_or("CODEX_USAGE: run requires --task")?;
            if task.trim().is_empty() || task.len() > MAX_TASK_BYTES || task.contains('\0') {
                return Err("CODEX_USAGE: --task is empty or invalid".into());
            }
            Ok(OperatorCodexAction::Run {
                scope,
                task,
                model,
                effort,
            })
        }
        "resume" if task.is_none() => Ok(OperatorCodexAction::Resume {
            scope,
            model,
            effort,
        }),
        _ => Err("CODEX_USAGE: options do not match the requested action".into()),
    }
}

fn valid_model(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PROFILE_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_effort(value: &str) -> bool {
    matches!(
        value,
        "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
    )
}

fn registered_project(
    catdesk_workspace: &Path,
    scope: &CodexScope,
) -> Result<AutonomousProjectV1, String> {
    let selected = scope
        .workspace
        .canonicalize()
        .map_err(|_| "CODEX_SCOPE: selected project workspace is unavailable".to_string())?;
    let resolved = resolve_registered_project_from_control_root(
        catdesk_workspace,
        &scope.project_id,
    )
    .map_err(|_| {
        "CODEX_SCOPE: CatDesk control Git identity or selected project registration/Git origin is unavailable"
            .to_string()
    })?;
    if selected != resolved.project.workspace {
        return Err("CODEX_SCOPE: selected workspace differs from the registered project".into());
    }
    Ok(resolved.project)
}

fn ensure_same_project_binding(
    selected: &AutonomousProjectV1,
    observed: &AutonomousProjectV1,
) -> Result<(), String> {
    if selected != observed {
        return Err(
            "CODEX_SCOPE: project registration or bound thread changed before native launch".into(),
        );
    }
    Ok(())
}

fn project_binding_sha256(project: &AutonomousProjectV1) -> Result<String, String> {
    let encoded = serde_json::to_vec(project)
        .map_err(|_| "CODEX_SCOPE: project binding cannot be encoded".to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(b"catdesk-operator-project-binding-v1\0");
    hasher.update(encoded);
    Ok(format!("{:x}", hasher.finalize()))
}

fn redirected_path(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    false
}

fn checked_autonomy_entry(path: &Path, directory: bool) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err("CODEX_LOCK: managed writer state is unreadable".into()),
    };
    if redirected_path(&metadata) || metadata.is_dir() != directory {
        return Err("CODEX_LOCK: managed writer state is redirected or unsafe".into());
    }
    Ok(true)
}

fn exact_thread(project: &AutonomousProjectV1) -> Result<&str, String> {
    let thread = project.codex_thread_id.as_deref().ok_or_else(|| {
        "CODEX_THREAD: no thread is bound; use the existing project thread adoption action"
            .to_string()
    })?;
    if !uuid::Uuid::parse_str(thread).is_ok_and(|id| id.to_string() == thread) {
        return Err("CODEX_THREAD: bound thread ID is not a canonical UUID".into());
    }
    Ok(thread)
}

fn ensure_no_interrupted_managed_writer(project: &AutonomousProjectV1) -> Result<(), String> {
    let catdesk_root = project.workspace.join(".catdesk");
    if !checked_autonomy_entry(&catdesk_root, true)? {
        return Ok(());
    }
    let state_root = catdesk_root.join("autonomy");
    if !checked_autonomy_entry(&state_root, true)? {
        return Ok(());
    }
    let observed_root = state_root
        .canonicalize()
        .map_err(|_| "CODEX_LOCK: managed writer state is unreadable".to_string())?;
    let project_workspace = project
        .workspace
        .canonicalize()
        .map_err(|_| "CODEX_LOCK: project workspace is unavailable".to_string())?;
    if !observed_root.starts_with(&project_workspace) {
        return Err("CODEX_LOCK: managed writer state escaped the project workspace".into());
    }
    for entry in fs::read_dir(&state_root)
        .map_err(|_| "CODEX_LOCK: managed writer state is unreadable".to_string())?
    {
        let entry =
            entry.map_err(|_| "CODEX_LOCK: managed writer state is unreadable".to_string())?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| "CODEX_LOCK: managed writer state is unreadable".to_string())?;
        if redirected_path(&metadata) {
            return Err("CODEX_LOCK: managed writer state contains a redirected path".into());
        }
        if metadata.is_dir()
            && !matches!(
                entry.file_name().to_string_lossy().as_ref(),
                "diagnostics" | "qwen_tools"
            )
        {
            for name in ["state.json", "queue.json"] {
                if !checked_autonomy_entry(&entry.path().join(name), false)? {
                    return Err("CODEX_LOCK: managed writer state is incomplete".into());
                }
            }
        }
    }
    let store = AutonomousStateStoreV1::open(state_root)
        .map_err(|_| "CODEX_LOCK: managed writer state is unavailable".to_string())?;
    let sessions = store
        .list_sessions_strict()
        .map_err(|_| "CODEX_LOCK: managed writer state is unreadable".to_string())?;
    for session in sessions {
        if matches!(
            session.state,
            AutonomousSessionStateV1::Running | AutonomousSessionStateV1::Verifying
        ) {
            return Err(
                "CODEX_LOCK: a managed turn or verification is unfinished; use recovery before direct Codex"
                    .into(),
            );
        }
        // A terminal snapshot alone does not prove that its provider child
        // exited: cancellation can leave the queue task and span open.
        let queue = store
            .load_queue(&session.session_id)
            .map_err(|_| "CODEX_LOCK: managed task ownership is unreadable".to_string())?;
        if queue.tasks.iter().any(|task| {
            matches!(
                task.state,
                AutonomousQueueTaskStateV1::WorkerRunning
                    | AutonomousQueueTaskStateV1::Running
                    | AutonomousQueueTaskStateV1::Verifying
                    | AutonomousQueueTaskStateV1::Repairing
            )
        }) {
            return Err(
                "CODEX_LOCK: a durable managed task still claims provider ownership; use recovery before direct Codex"
                    .into(),
            );
        }
    }
    // The queue can look quiescent while a historical accounting record still
    // has an unended task or an open activity span. Use the same bounded,
    // no-follow readback as the operator recovery view before launching.
    let recovery = recovery::report(project, false).map_err(|_| {
        "CODEX_LOCK: managed recovery evidence is unavailable or unsafe".to_string()
    })?;
    if recovery["status"] != "NO_RECORDED_BLOCKER_OBSERVED" {
        return Err(
            "CODEX_LOCK: managed ownership or accounting remains unresolved; use recovery before direct Codex"
                .into(),
        );
    }
    Ok(())
}

fn resolve_native_cli() -> Result<(PathBuf, Option<PathBuf>), String> {
    let executable = resolve_codex_executable().map_err(|_| {
        "CODEX_UNAVAILABLE: no direct Codex executable found; check PATH or CATDESK_CODEX_CLI_EXECUTABLE"
            .to_string()
    })?;
    let codex_home = operator_codex_home().map_err(|_| {
        "CODEX_HOME_UNAVAILABLE: CATDESK_CODEX_HOME must name an existing directory".to_string()
    })?;
    Ok((executable, codex_home))
}

fn codex_command(executable: &Path, workspace: &Path, codex_home: Option<&Path>) -> Command {
    let mut command = Command::new(executable);
    command.current_dir(workspace);
    // CatDesk passes the user's authenticated Codex home by reference. It
    // neither opens auth files nor passes API keys to the child.
    command
        .env_remove("CODEX_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("CONTROL_PLANE_API_KEY");
    if let Some(home) = codex_home {
        command.env("CODEX_HOME", home);
    }
    command
}

fn invocation_args(
    action: &OperatorCodexAction,
    project: &AutonomousProjectV1,
) -> Result<Vec<String>, String> {
    // Noninteractive `exec` reports an effective approval policy of `never`
    // on the installed CLI. Make that explicit; workspace-write still bounds
    // this operator-approved task. Interactive resume can request approvals.
    let approval = if matches!(action, OperatorCodexAction::Run { .. }) {
        "never"
    } else {
        "on-request"
    };
    let mut args = vec![
        "--cd".into(),
        project.workspace.to_string_lossy().into_owned(),
        "--sandbox".into(),
        "workspace-write".into(),
        "--ask-for-approval".into(),
        approval.into(),
    ];
    match action {
        OperatorCodexAction::Queue(_) => return Ok(Vec::new()),
        OperatorCodexAction::Status(_) => return Ok(Vec::new()),
        OperatorCodexAction::Recovery { .. } => return Ok(Vec::new()),
        OperatorCodexAction::Run { model, effort, .. } => {
            args.extend(["exec".into(), "resume".into()]);
            // An exec resume turn does not inherit the preceding turn's
            // sandbox. The global flag and resume-local override both assert
            // this exact workspace policy.
            args.extend(["-c".into(), "sandbox_mode=\"workspace-write\"".into()]);
            append_profile(&mut args, model.as_deref(), effort.as_deref());
            args.push(exact_thread(project)?.into());
            args.push("-".into());
        }
        OperatorCodexAction::Resume { model, effort, .. } => {
            args.push("resume".into());
            args.extend(["-c".into(), "sandbox_mode=\"workspace-write\"".into()]);
            append_profile(&mut args, model.as_deref(), effort.as_deref());
            args.push(exact_thread(project)?.into());
        }
        OperatorCodexAction::GlobalResumeAll => {
            return Err("CODEX_SCOPE: global picker has no project-scoped invocation".into());
        }
    }
    Ok(args)
}

fn append_profile(args: &mut Vec<String>, model: Option<&str>, effort: Option<&str>) {
    if let Some(model) = model {
        args.extend(["--model".into(), model.into()]);
    }
    if let Some(effort) = effort {
        args.extend(["-c".into(), format!("model_reasoning_effort={effort}")]);
    }
}

async fn bounded_probe(mut command: Command, args: &[&str]) -> Result<(bool, String), String> {
    command.args(args).stdin(Stdio::null()).kill_on_drop(true);
    let output = tokio::time::timeout(CODEX_PROBE_TIMEOUT, command.output())
        .await
        .map_err(|_| "CODEX_UNAVAILABLE: native Codex status timed out".to_string())?
        .map_err(|_| "CODEX_UNAVAILABLE: native Codex status could not start".to_string())?;
    let body = String::from_utf8_lossy(&output.stdout);
    let version = if args == ["--version"] {
        body.trim().chars().take(128).collect()
    } else {
        String::new()
    };
    Ok((output.status.success(), version))
}

fn unavailable_recorded_turn_gate(reason: &str) -> Value {
    json!({
        "recordedState":"EVIDENCE_UNAVAILABLE", "reason":reason,
        "sessionBlockerCount":null, "queueBlockerCount":null,
        "accountingReviewRowCount":null, "pendingWriterMarkerPresent":null,
        "readOnly":true, "transactional":false,
        "launchAuthorized":false, "writerLockChecked":false,
    })
}

fn recorded_turn_gate_summary(project: &AutonomousProjectV1) -> Value {
    let report = match recovery::report(project, false) {
        Ok(report) => report,
        Err(reason) => return unavailable_recorded_turn_gate(&reason),
    };
    let Some(recorded_state) = report["status"].as_str() else {
        return unavailable_recorded_turn_gate("CODEX_RECOVERY: report status is unavailable");
    };
    let (Some(sessions), Some(queues), Some(accounting), Some(marker)) = (
        report["sessionBlockers"].as_array(),
        report["queueBlockers"].as_array(),
        report["accountingReviewRows"].as_array(),
        report["pendingWriterMarker"]["present"].as_bool(),
    ) else {
        return unavailable_recorded_turn_gate("CODEX_RECOVERY: report structure is unavailable");
    };
    let blockers_present =
        !sessions.is_empty() || !queues.is_empty() || !accounting.is_empty() || marker;
    if report["readOnly"] != true
        || report["launchAuthorized"] != false
        || !matches!(
            (recorded_state, blockers_present),
            ("NEEDS_OWNER_RECOVERY", true) | ("NO_RECORDED_BLOCKER_OBSERVED", false)
        )
    {
        return unavailable_recorded_turn_gate("CODEX_RECOVERY: report state is inconsistent");
    }
    json!({
        "recordedState":recorded_state,
        "sessionBlockerCount":sessions.len(), "queueBlockerCount":queues.len(),
        "accountingReviewRowCount":accounting.len(), "pendingWriterMarkerPresent":marker,
        "readOnly":true, "transactional":false,
        "launchAuthorized":false, "writerLockChecked":false,
    })
}

fn status_response(
    project: &AutonomousProjectV1,
    executable: &Path,
    version: &str,
    authenticated: bool,
) -> Result<Value, String> {
    let binding_sha256 = project_binding_sha256(project)?;
    Ok(json!({
        "action":"codex_status", "status":"success", "projectId":project.project_id,
        "workspace":project.workspace, "threadId":project.codex_thread_id,
        "verificationProfile":project.verification_profile,
        "bindingSha256":binding_sha256,
        "executable":executable, "version":version, "authenticated":authenticated,
        "route":"current_user_native_cli",
        "recordedTurnGate":recorded_turn_gate_summary(project),
    }))
}

async fn write_native_prompt(
    child: &mut tokio::process::Child,
    task: &str,
    limit: Duration,
) -> Result<(), &'static str> {
    let Some(mut stdin) = child.stdin.take() else {
        return Err("CODEX_IO: native Codex prompt pipe is unavailable; writer marker retained");
    };
    let result = tokio::time::timeout(limit, async {
        stdin.write_all(task.as_bytes()).await?;
        stdin.flush().await
    })
    .await;
    // ChildStdin::shutdown does not close the native pipe on Windows. The
    // `exec -` reader needs the handle dropped before it can observe EOF.
    drop(stdin);
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => {
            Err("CODEX_IO: native Codex did not accept the task prompt; writer marker retained")
        }
        Err(_) => Err("CODEX_IO: native Codex prompt write timed out; writer marker retained"),
    }
}

async fn wait_native_run(
    child: &mut tokio::process::Child,
    writer: &mut ProjectWriterGuard,
    limit: Duration,
) -> Result<ExitStatus, String> {
    match tokio::time::timeout(limit, child.wait()).await {
        Ok(Ok(status)) => {
            writer.finish_writer_turn().map_err(|_| {
                "CODEX_LOCK: native Codex writer marker could not be retired".to_string()
            })?;
            Ok(status)
        }
        Ok(Err(_)) => {
            let _ = tokio::time::timeout(CODEX_KILL_TIMEOUT, child.kill()).await;
            Err("CODEX_IO: native Codex run exit status is unavailable; writer marker retained for recovery".into())
        }
        Err(_) => {
            let _ = tokio::time::timeout(CODEX_KILL_TIMEOUT, child.kill()).await;
            // A killed parent cannot establish that its descendants exited.
            // Retain the durable marker even if `kill` observes parent exit.
            Err(format!(
                "CODEX_TIMEOUT: native Codex run exceeded {} seconds after prompt delivery; writer marker retained for recovery",
                limit.as_secs()
            ))
        }
    }
}

pub(crate) async fn execute_action(
    action: OperatorCodexAction,
    catdesk_workspace: &Path,
) -> Result<Value, String> {
    let action = match action {
        OperatorCodexAction::Queue(queue_action) => {
            return queue::execute_action(queue_action, catdesk_workspace).await;
        }
        action => action,
    };
    if matches!(action, OperatorCodexAction::GlobalResumeAll) {
        return execute_global_resume_picker(catdesk_workspace).await;
    }
    let scope = match &action {
        OperatorCodexAction::Queue(_) => unreachable!(),
        OperatorCodexAction::Status(scope)
        | OperatorCodexAction::Recovery { scope, .. }
        | OperatorCodexAction::Run { scope, .. }
        | OperatorCodexAction::Resume { scope, .. } => scope,
        OperatorCodexAction::GlobalResumeAll => unreachable!(),
    };
    // Validate the registration before creating any current-user lock state.
    // Re-read the exact binding after acquisition to close the rotation race.
    let selected = registered_project(catdesk_workspace, scope)?;
    if let OperatorCodexAction::Recovery { prepare, .. } = &action {
        return recovery::report(&selected, *prepare);
    }
    let (executable, codex_home) = resolve_native_cli()?;
    if let OperatorCodexAction::Status(_) = &action {
        let (version_ok, version) = bounded_probe(
            codex_command(&executable, &selected.workspace, codex_home.as_deref()),
            &["--version"],
        )
        .await?;
        if !version_ok || !version.starts_with("codex-cli ") {
            return Err(
                "CODEX_UNAVAILABLE: native executable did not report a Codex CLI version".into(),
            );
        }
        let (authenticated, _) = bounded_probe(
            codex_command(&executable, &selected.workspace, codex_home.as_deref()),
            &["login", "status"],
        )
        .await?;
        return status_response(&selected, &executable, &version, authenticated);
    }
    let mut writer = acquire_project_writer_lock(&selected.workspace).map_err(|_| {
        "CODEX_LOCK: project workspace has an active or unresolved writer; inspect recovery before direct Codex"
            .to_string()
    })?;
    let project = registered_project(catdesk_workspace, scope)?;
    ensure_same_project_binding(&selected, &project)?;
    ensure_no_interrupted_managed_writer(&project)?;
    let args = invocation_args(&action, &project)?;
    let mut command = codex_command(&executable, &project.workspace, codex_home.as_deref());
    command
        .args(&args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // If the CatDesk operator action is cancelled, dropping this child must
    // terminate the direct Codex CLI process before its project lock drops.
    command.kill_on_drop(true);
    if matches!(action, OperatorCodexAction::Run { .. }) {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::inherit());
    }
    writer
        .begin_writer_turn()
        .map_err(|_| "CODEX_LOCK: native Codex writer marker is unavailable".to_string())?;
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            writer.finish_writer_turn().map_err(|_| {
                "CODEX_LOCK: native Codex writer marker could not be retired".to_string()
            })?;
            return Err("CODEX_UNAVAILABLE: native Codex process could not start".into());
        }
    };
    if let OperatorCodexAction::Run { task, .. } = &action {
        if let Err(error) = write_native_prompt(&mut child, task, CODEX_PROMPT_TIMEOUT).await {
            // Killing the parent does not prove that its descendants stopped.
            // Keep the durable writer marker for proof-based recovery.
            let _ = tokio::time::timeout(CODEX_KILL_TIMEOUT, child.kill()).await;
            return Err(error.into());
        }
    }
    let status = if matches!(action, OperatorCodexAction::Run { .. }) {
        wait_native_run(&mut child, &mut writer, CODEX_RUN_TIMEOUT).await?
    } else {
        // `resume` is an interactive native session, not a timed model turn.
        let status = child.wait().await.map_err(|_| {
            "CODEX_IO: native Codex resume exit status is unavailable; writer marker retained"
                .to_string()
        })?;
        writer.finish_writer_turn().map_err(|_| {
            "CODEX_LOCK: native Codex writer marker could not be retired".to_string()
        })?;
        status
    };
    if !status.success() {
        return Err(format!(
            "CODEX_EXIT: native Codex returned {}; inspect its terminal output for auth, quota, approval, or task errors",
            status
                .code()
                .map_or("signal".into(), |code| code.to_string())
        ));
    }
    let (verb, model, effort) = match &action {
        OperatorCodexAction::Queue(_) => unreachable!(),
        OperatorCodexAction::Run { model, effort, .. } => ("run", model, effort),
        OperatorCodexAction::Resume { model, effort, .. } => ("resume", model, effort),
        OperatorCodexAction::Status(_) => unreachable!(),
        OperatorCodexAction::Recovery { .. } => unreachable!(),
        OperatorCodexAction::GlobalResumeAll => unreachable!(),
    };
    Ok(json!({
        "action":format!("codex_{verb}"), "status":"native_cli_exited_zero", "exitCode":0,
        "projectId":project.project_id, "workspace":project.workspace,
        "threadId":project.codex_thread_id.as_deref(),
        "requestedModel":model, "requestedEffort":effort,
        "requestedSandbox":"workspace-write",
        "requestedApprovalPolicy":if verb == "run" { "never" } else { "on-request" },
        "executable":executable,
    }))
}

async fn execute_global_resume_picker(catdesk_workspace: &Path) -> Result<Value, String> {
    // Verify the caller is in the registered CatDesk control Git root, then
    // hand over to the user's native picker. No selected project, task thread,
    // or project lock is claimed by this manual cross-session route.
    let control = resolve_registered_project_from_control_root(catdesk_workspace, "catdesk")
        .map_err(|_| "CODEX_SCOPE: CatDesk control Git identity is unavailable".to_string())?;
    let (executable, codex_home) = resolve_native_cli()?;
    let mut command = codex_command(
        &executable,
        &control.control_workspace,
        codex_home.as_deref(),
    );
    command
        .args([
            "--sandbox",
            "workspace-write",
            "--ask-for-approval",
            "on-request",
            "resume",
            "--all",
            "--include-non-interactive",
            "-c",
            "sandbox_mode=\"workspace-write\"",
        ])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // The picker is manual and global, but cancellation still owns its direct
    // CLI child. Codex's separately owned shared daemon is untouched.
    command.kill_on_drop(true);
    let status = command
        .spawn()
        .map_err(|_| "CODEX_UNAVAILABLE: native Codex picker could not start".to_string())?
        .wait()
        .await
        .map_err(|_| "CODEX_IO: native Codex picker exit status is unavailable".to_string())?;
    if !status.success() {
        return Err(format!(
            "CODEX_EXIT: native Codex picker returned {}; inspect its terminal output",
            status
                .code()
                .map_or("signal".into(), |code| code.to_string())
        ));
    }
    Ok(json!({
        "action":"codex_global_resume_picker", "status":"native_cli_exited_zero", "exitCode":0,
        "projectClaim":false, "threadId":null, "requestedSandbox":"workspace-write",
        "requestedApprovalPolicy":"on-request", "executable":executable,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegated::autonomy_accounting::{ActivityActorV1, ActivityEvidenceV1};
    use crate::delegated::autonomy_projects::AutonomousProjectRegistryStoreV1;
    use crate::delegated::autonomy_runtime::inspect_project_writer_pending_marker;
    use crate::delegated::autonomy_state::{AutonomousQueueTaskV1, AutonomousQueueV1};
    use std::fs;
    use std::process::Command as StdCommand;

    fn parse(items: &[&str]) -> Result<OperatorCodexAction, String> {
        parse_action(&items.iter().map(|item| (*item).into()).collect::<Vec<_>>())
    }

    fn remove_operator_fixture(root: PathBuf) {
        let resolved = root.canonicalize().expect("canonical fixture root");
        let temp = std::env::temp_dir()
            .canonicalize()
            .expect("canonical temp root");
        assert!(resolved != temp && resolved.starts_with(&temp));
        fs::remove_dir_all(resolved).expect("fixture cleanup");
    }

    fn status_fixture_project(root: &Path) -> AutonomousProjectV1 {
        AutonomousProjectV1 {
            project_id: "fixture".into(),
            workspace: root.canonicalize().expect("canonical project"),
            git_identity: "fixture".into(),
            verification_profile: "rust_full".into(),
            codex_thread_id: None,
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
        }
    }

    #[test]
    fn status_reports_clean_recorded_state_without_launch_authority_or_writes() {
        let root =
            std::env::temp_dir().join(format!("catdesk-status-clean-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).expect("fixture root");
        let project = status_fixture_project(&root);
        assert!(!root.join(".catdesk").exists());
        let status = status_response(
            &project,
            Path::new("codex-fixture.exe"),
            "codex-cli fixture",
            true,
        )
        .expect("status");
        assert_eq!(status["status"], "success");
        assert_eq!(status["authenticated"], true);
        let gate = &status["recordedTurnGate"];
        assert_eq!(gate["recordedState"], "NO_RECORDED_BLOCKER_OBSERVED");
        assert_eq!(gate["sessionBlockerCount"], 0);
        assert_eq!(gate["queueBlockerCount"], 0);
        assert_eq!(gate["accountingReviewRowCount"], 0);
        assert_eq!(gate["pendingWriterMarkerPresent"], false);
        assert_eq!(gate["readOnly"], true);
        assert_eq!(gate["transactional"], false);
        assert_eq!(gate["launchAuthorized"], false);
        assert_eq!(gate["writerLockChecked"], false);
        assert!(!root.join(".catdesk").exists());
        assert_eq!(inspect_project_writer_pending_marker(&root).unwrap(), None);
        remove_operator_fixture(root);
    }

    #[test]
    fn status_reports_recorded_blockers_without_touching_evidence() {
        let (root, project, store) = terminal_accounting_fixture();
        start_accounting_record(&store);
        let mut queue = store.load_queue("historical-task").expect("queue");
        queue.tasks[0].state = AutonomousQueueTaskStateV1::WorkerRunning;
        store
            .save_queue("historical-task", &queue)
            .expect("in-flight queue");
        let ledger = root.join(".catdesk/autonomy/execution-accounting.json");
        let queue_path = root.join(".catdesk/autonomy/historical-task/queue.json");
        let state_path = root.join(".catdesk/autonomy/historical-task/state.json");
        let ledger_before = fs::read(&ledger).expect("ledger before status");
        let queue_before = fs::read(&queue_path).expect("queue before status");
        let state_before = fs::read(&state_path).expect("state before status");
        let status = status_response(
            &project,
            Path::new("codex-fixture.exe"),
            "codex-cli fixture",
            true,
        )
        .expect("status");
        let gate = &status["recordedTurnGate"];
        assert_eq!(status["authenticated"], true);
        assert_eq!(gate["recordedState"], "NEEDS_OWNER_RECOVERY");
        assert_eq!(gate["sessionBlockerCount"], 0);
        assert_eq!(gate["queueBlockerCount"], 1);
        assert_eq!(gate["accountingReviewRowCount"], 1);
        assert_eq!(gate["pendingWriterMarkerPresent"], false);
        assert_eq!(gate["transactional"], false);
        assert_eq!(gate["launchAuthorized"], false);
        assert_eq!(gate["writerLockChecked"], false);
        assert_eq!(
            fs::read(&ledger).expect("ledger after status"),
            ledger_before
        );
        assert_eq!(
            fs::read(&queue_path).expect("queue after status"),
            queue_before
        );
        assert_eq!(
            fs::read(&state_path).expect("state after status"),
            state_before
        );
        assert_eq!(inspect_project_writer_pending_marker(&root).unwrap(), None);
        remove_operator_fixture(root);
    }

    #[test]
    fn status_marks_malformed_recovery_evidence_unavailable_without_writes() {
        let root =
            std::env::temp_dir().join(format!("catdesk-status-malformed-{}", uuid::Uuid::new_v4()));
        let state = root.join(".catdesk/autonomy");
        fs::create_dir_all(&state).expect("fixture state root");
        let ledger = state.join("execution-accounting.json");
        let malformed = b"not JSON";
        fs::write(&ledger, malformed).expect("malformed fixture ledger");
        let project = status_fixture_project(&root);
        let status = status_response(
            &project,
            Path::new("codex-fixture.exe"),
            "codex-cli fixture",
            true,
        )
        .expect("status");
        let gate = &status["recordedTurnGate"];
        assert_eq!(status["status"], "success");
        assert_eq!(gate["recordedState"], "EVIDENCE_UNAVAILABLE");
        assert!(gate["sessionBlockerCount"].is_null());
        assert!(gate["queueBlockerCount"].is_null());
        assert!(gate["accountingReviewRowCount"].is_null());
        assert!(gate["pendingWriterMarkerPresent"].is_null());
        assert_eq!(gate["transactional"], false);
        assert_eq!(gate["launchAuthorized"], false);
        assert_eq!(gate["writerLockChecked"], false);
        assert_eq!(fs::read(&ledger).expect("ledger after status"), malformed);
        assert_eq!(inspect_project_writer_pending_marker(&root).unwrap(), None);
        remove_operator_fixture(root);
    }

    #[tokio::test]
    async fn native_prompt_and_direct_run_wait_are_bounded() {
        let root =
            std::env::temp_dir().join(format!("catdesk-operator-prompt-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).expect("fixture root");
        let source = root.join("prompt-child.rs");
        let executable = root.join(if cfg!(windows) {
            "prompt-child.exe"
        } else {
            "prompt-child"
        });
        fs::write(
            &source,
            r#"
                use std::io::Read;
                use std::time::Duration;
                fn main() {
                    match std::env::args().nth(1).as_deref() {
                        Some("eof") => {
                            let mut prompt = Vec::new();
                            std::io::stdin().read_to_end(&mut prompt).unwrap();
                            if prompt != b"bounded direct prompt" {
                                std::process::exit(1);
                            }
                        }
                        Some("idle") => std::thread::sleep(Duration::from_secs(30)),
                        _ => std::process::exit(2),
                    }
                }
            "#,
        )
        .expect("fixture source");
        assert!(
            StdCommand::new("rustc")
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .status()
                .expect("rustc available")
                .success()
        );

        let mut writer = acquire_project_writer_lock(&root).expect("scratch writer lock");
        writer.begin_writer_turn().expect("scratch writer marker");
        let mut eof_child = Command::new(&executable)
            .arg("eof")
            .stdin(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("EOF reader starts");
        write_native_prompt(
            &mut eof_child,
            "bounded direct prompt",
            Duration::from_secs(5),
        )
        .await
        .expect("prompt accepted");
        let status = wait_native_run(&mut eof_child, &mut writer, Duration::from_secs(5))
            .await
            .expect("EOF reader exits after prompt handle drops");
        assert!(status.success());
        assert_eq!(inspect_project_writer_pending_marker(&root).unwrap(), None);
        drop(writer);

        let mut idle_child = Command::new(&executable)
            .arg("idle")
            .stdin(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("nonreading child starts");
        let oversized_prompt = "P".repeat(4 * 1024 * 1024);
        let started = std::time::Instant::now();
        let error = write_native_prompt(
            &mut idle_child,
            &oversized_prompt,
            Duration::from_millis(100),
        )
        .await
        .expect_err("nonreading child's pipe must time out");
        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
        tokio::time::timeout(Duration::from_secs(5), idle_child.kill())
            .await
            .expect("bounded fixture kill")
            .expect("fixture child terminated");

        // The short prompt fits the pipe, but the native process never exits.
        // A timed-out run kills the known fixture parent and leaves its marker
        // for an owner to review; the production path cannot assume its own
        // native process has no descendants as this fixture does.
        let mut writer = acquire_project_writer_lock(&root).expect("scratch writer lock");
        writer.begin_writer_turn().expect("scratch writer marker");
        let marker = inspect_project_writer_pending_marker(&root)
            .expect("scratch marker readback")
            .expect("marker before native wait");
        let mut stalled_child = Command::new(&executable)
            .arg("idle")
            .stdin(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("idle native child starts");
        write_native_prompt(&mut stalled_child, "small prompt", Duration::from_secs(5))
            .await
            .expect("small prompt accepted");
        let started = std::time::Instant::now();
        let error = wait_native_run(&mut stalled_child, &mut writer, Duration::from_secs(1))
            .await
            .expect_err("stalled native run must time out");
        assert!(error.starts_with("CODEX_TIMEOUT:"), "{error}");
        assert!(error.contains("writer marker retained"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(
            stalled_child
                .try_wait()
                .expect("fixture child status")
                .is_some()
        );
        assert_eq!(
            inspect_project_writer_pending_marker(&root).expect("marker after timeout"),
            Some(marker)
        );
        // Test-only cleanup is justified by this fixture's single process,
        // whose exit was observed above. Production timeout never does this.
        writer.finish_writer_turn().expect("retire fixture marker");
        drop(writer);
        remove_operator_fixture(root);
    }

    fn terminal_accounting_fixture() -> (PathBuf, AutonomousProjectV1, AutonomousStateStoreV1) {
        let root = std::env::temp_dir().join(format!(
            "catdesk-operator-accounting-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("fixture workspace");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("autonomy store");
        store
            .create_session(
                "historical-task",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "work".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::Ready,
                }])
                .expect("queue"),
            )
            .expect("session");
        let mut snapshot = store.load_session("historical-task").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Cancelled;
        snapshot.active = false;
        store.save_session(&snapshot).expect("terminal snapshot");
        let project = AutonomousProjectV1 {
            project_id: "fixture".into(),
            workspace: root.canonicalize().expect("canonical project"),
            git_identity: "fixture".into(),
            verification_profile: "rust_full".into(),
            codex_thread_id: None,
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
        };
        (root, project, store)
    }

    fn start_accounting_record(store: &AutonomousStateStoreV1) {
        store
            .execution_accounting_store()
            .expect("accounting store")
            .record_task_started(
                "fixture",
                "work",
                "historical-task",
                Some("thread"),
                "codex-cli",
                None,
                None,
                100,
            )
            .expect("accounting record");
    }

    #[test]
    fn unended_accounting_blocks_native_after_terminal_session_and_ready_queue() {
        let (root, project, store) = terminal_accounting_fixture();
        start_accounting_record(&store);
        let error = ensure_no_interrupted_managed_writer(&project)
            .expect_err("unended accounting must block native launch");
        assert!(error.contains("accounting"), "{error}");
        remove_operator_fixture(root);
    }

    #[test]
    fn ended_accounting_with_open_span_blocks_native_after_terminal_session() {
        let (root, project, store) = terminal_accounting_fixture();
        start_accounting_record(&store);
        let accounting = store
            .execution_accounting_store()
            .expect("accounting store");
        accounting
            .activity_started(
                "historical-task",
                ActivityActorV1::CodexProviderActive,
                ActivityEvidenceV1::ProviderLifecycle,
                110,
            )
            .expect("open span");
        accounting
            .finish_task("historical-task", None, None, 120)
            .expect("ended record");
        let error = ensure_no_interrupted_managed_writer(&project)
            .expect_err("open span must block native launch");
        assert!(error.contains("accounting"), "{error}");
        remove_operator_fixture(root);
    }

    #[test]
    fn closed_accounting_permits_native_prelaunch_after_terminal_session() {
        let (root, project, store) = terminal_accounting_fixture();
        start_accounting_record(&store);
        let accounting = store
            .execution_accounting_store()
            .expect("accounting store");
        accounting
            .activity_started(
                "historical-task",
                ActivityActorV1::CodexProviderActive,
                ActivityEvidenceV1::ProviderLifecycle,
                110,
            )
            .expect("span start");
        accounting
            .activity_finished("historical-task", ActivityActorV1::CodexProviderActive, 120)
            .expect("span end");
        accounting
            .finish_task("historical-task", None, None, 130)
            .expect("ended record");
        ensure_no_interrupted_managed_writer(&project).expect("closed accounting preflight");
        remove_operator_fixture(root);
    }

    #[test]
    fn redirected_autonomy_root_blocks_native_launch() {
        let base = std::env::temp_dir().join(format!(
            "catdesk-operator-redirected-state-{}",
            uuid::Uuid::new_v4()
        ));
        let project_root = base.join("project");
        let outside = base.join("outside");
        fs::create_dir_all(project_root.join(".catdesk")).expect("project state parent");
        fs::create_dir_all(&outside).expect("outside directory");
        let redirected = project_root.join(".catdesk/autonomy");
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside, &redirected).is_ok();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, &redirected).is_ok();
        #[cfg(not(any(windows, unix)))]
        let linked = false;
        if !linked {
            fs::write(&redirected, b"unsafe state path").expect("unsafe file fixture");
        }
        let project = AutonomousProjectV1 {
            project_id: "fixture".into(),
            workspace: project_root,
            git_identity: "fixture".into(),
            verification_profile: "rust_full".into(),
            codex_thread_id: None,
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
        };
        assert!(ensure_no_interrupted_managed_writer(&project).is_err());
        if linked {
            fs::remove_dir(&redirected).expect("remove fixture redirect");
        } else {
            fs::remove_file(&redirected).expect("remove unsafe fixture file");
        }
        remove_operator_fixture(base);
    }

    #[test]
    fn inflight_task_blocks_direct_native_even_when_session_terminal() {
        let root = std::env::temp_dir().join(format!(
            "catdesk-operator-durable-writer-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("fixture workspace");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("autonomy store");
        store
            .create_session(
                "historical-task",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "work".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::WorkerRunning,
                }])
                .expect("queue"),
            )
            .expect("session");
        let mut snapshot = store.load_session("historical-task").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Queued;
        store.save_session(&snapshot).expect("queued session");
        let project = AutonomousProjectV1 {
            project_id: "fixture".into(),
            workspace: root.canonicalize().expect("canonical project"),
            git_identity: "fixture".into(),
            verification_profile: "rust_full".into(),
            codex_thread_id: None,
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
        };
        let error = ensure_no_interrupted_managed_writer(&project)
            .expect_err("nonterminal provider task must block direct Codex");
        assert!(error.contains("durable managed task"), "{error}");
        snapshot.state = AutonomousSessionStateV1::Cancelled;
        snapshot.active = false;
        store
            .save_session(&snapshot)
            .expect("durably terminal session");
        let error = ensure_no_interrupted_managed_writer(&project)
            .expect_err("terminal snapshot does not prove provider child exit");
        assert!(error.contains("durable managed task"), "{error}");
        remove_operator_fixture(root);
    }

    #[test]
    fn parser_requires_explicit_project_workspace_and_rejects_bypass_flags() {
        let base = [
            "operator",
            "codex",
            "run",
            "--project-id",
            "catdesk",
            "--workspace",
            "C:/work/catdesk",
        ];
        assert!(parse(&base).is_err());
        assert!(parse(&[&base[..], &["--task", "edit the README"]].concat()).is_ok());
        assert!(
            parse(
                &[
                    &base[..],
                    &["--task", "edit", "--sandbox", "danger-full-access"]
                ]
                .concat()
            )
            .is_err()
        );
        assert!(parse(&[&base[..], &["--task", "edit", "--task", "other"]].concat()).is_err());
        assert!(parse(&[&base[..], &["--task", "edit", "--all"]].concat()).is_err());
        assert!(
            parse(&[
                "operator",
                "codex",
                "status",
                "--project-id",
                "catdesk",
                "--workspace",
                "C:/work/catdesk",
                "--all",
            ])
            .is_err()
        );
        assert!(parse(&[&base[..], &["--task", "edit", "--model", "gpt;evil"]].concat()).is_err());
        assert!(
            parse(&[&base[..], &["--task", "edit", "--effort", "impossible"]].concat()).is_err()
        );
        assert!(
            parse(&[
                "operator",
                "codex",
                "resume",
                "--project-id",
                "catdesk",
                "--workspace",
                "relative",
                "--all"
            ])
            .is_err()
        );
    }

    #[test]
    fn recovery_parser_is_scoped_and_read_only() {
        let args = [
            "operator",
            "codex",
            "recovery",
            "prepare",
            "--project-id",
            "catdesk",
            "--workspace",
            "C:/work/catdesk",
        ];
        assert!(matches!(
            parse(&args),
            Ok(OperatorCodexAction::Recovery { prepare: true, .. })
        ));
        assert!(parse(&[&args[..], &["--task", "edit files"]].concat()).is_err());
        assert!(parse(&[&args[..], &["--all"]].concat()).is_err());
        assert!(parse(&args[..4]).is_err());
        let mut invalid = args.to_vec();
        invalid[3] = "clear";
        assert!(parse(&invalid).is_err());
    }

    #[test]
    fn run_uses_exact_bound_thread_and_keeps_task_off_native_argv() {
        let thread = "019fe33e-5c82-7540-a41c-9ae497527e57";
        let project = AutonomousProjectV1 {
            project_id: "catdesk".into(),
            workspace: PathBuf::from("C:/work/catdesk"),
            git_identity: "fixture".into(),
            verification_profile: "rust_full".into(),
            codex_thread_id: Some(thread.into()),
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
        };
        let action = parse(&[
            "operator",
            "codex",
            "run",
            "--project-id",
            "catdesk",
            "--workspace",
            "C:/work/catdesk",
            "--task",
            "private task text",
            "--model",
            "gpt-6.1-sol",
            "--effort",
            "medium",
        ])
        .expect("parse");
        let args = invocation_args(&action, &project).expect("args");
        assert_eq!(
            &args[2..8],
            &[
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "never",
                "exec",
                "resume"
            ]
        );
        assert_eq!(args[0], "--cd");
        assert_eq!(args[1], "C:/work/catdesk");
        assert!(args.contains(&"sandbox_mode=\"workspace-write\"".to_string()));
        assert_eq!(args[args.len() - 2], thread);
        assert!(args.contains(&"gpt-6.1-sol".to_string()));
        assert!(args.contains(&"model_reasoning_effort=medium".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("-"));
        assert!(!args.iter().any(|arg| arg.contains("private task")));
    }

    #[test]
    fn global_picker_has_no_project_claim_and_exact_resume_stays_bound() {
        let project = AutonomousProjectV1 {
            project_id: "catdesk".into(),
            workspace: PathBuf::from("C:/work/catdesk"),
            git_identity: "fixture".into(),
            verification_profile: "rust_full".into(),
            codex_thread_id: Some("019fe33e-5c82-7540-a41c-9ae497527e57".into()),
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
        };
        let exact = parse(&[
            "operator",
            "codex",
            "resume",
            "--project-id",
            "catdesk",
            "--workspace",
            "C:/work/catdesk",
        ])
        .expect("exact");
        let all = parse(&["operator", "codex", "resume", "--all"]).expect("all");
        assert_eq!(all, OperatorCodexAction::GlobalResumeAll);
        assert!(
            parse(&[
                "operator",
                "codex",
                "resume",
                "--all",
                "--project-id",
                "catdesk",
                "--workspace",
                "C:/work/catdesk",
            ])
            .is_err()
        );
        assert!(
            invocation_args(&exact, &project)
                .expect("args")
                .contains(&project.codex_thread_id.clone().unwrap())
        );
        assert!(invocation_args(&all, &project).is_err());
        assert!(
            !invocation_args(&exact, &project)
                .expect("args")
                .contains(&"--all".to_string())
        );
    }

    fn fixture_git_root(root: &Path, origin: &str) {
        fs::create_dir_all(root).expect("fixture root");
        for args in [
            &["init", "-q"][..],
            &["remote", "add", "origin", origin][..],
        ] {
            assert!(
                StdCommand::new("git")
                    .arg("-C")
                    .arg(root)
                    .args(args)
                    .status()
                    .expect("Git fixture command")
                    .success()
            );
        }
    }

    #[test]
    fn registered_project_cannot_be_selected_with_sibling_workspace() {
        let temp_root = std::env::temp_dir().canonicalize().expect("temp root");
        let root = temp_root.join(format!("catdesk-operator-codex-{}", uuid::Uuid::new_v4()));
        let catdesk = root.join("catdesk");
        let sibling = root.join("sibling");
        fixture_git_root(&catdesk, "https://example.test/catdesk.git");
        fixture_git_root(&sibling, "https://example.test/sibling.git");
        fs::create_dir_all(catdesk.join(".catdesk")).expect("control state");
        let store = AutonomousProjectRegistryStoreV1::open(catdesk.join(".catdesk/projects"))
            .expect("registry");
        store.initialize(2, 1).expect("initialize");
        for (id, workspace) in [("catdesk", &catdesk), ("sibling", &sibling)] {
            store
                .register_project(AutonomousProjectV1 {
                    project_id: id.into(),
                    workspace: workspace.clone(),
                    git_identity: format!("https://example.test/{id}.git"),
                    verification_profile: "rust_full".into(),
                    codex_thread_id: None,
                    chatgpt_target_url: None,
                    chatgpt_target_sha256: None,
                })
                .expect("register");
        }
        let good = CodexScope {
            project_id: "catdesk".into(),
            workspace: catdesk.clone(),
        };
        let cross = CodexScope {
            project_id: "catdesk".into(),
            workspace: sibling,
        };
        assert_eq!(
            registered_project(&catdesk, &good)
                .expect("same project")
                .project_id,
            "catdesk"
        );
        assert!(registered_project(&catdesk, &cross).is_err());
        let sibling_scope = CodexScope {
            project_id: "sibling".into(),
            workspace: cross.workspace.clone(),
        };
        assert_eq!(
            registered_project(&catdesk, &sibling_scope)
                .expect("registered sibling")
                .project_id,
            "sibling"
        );
        let selected = registered_project(&catdesk, &sibling_scope).expect("pre-lock scope");
        store
            .bind_codex_thread("sibling", "new-thread")
            .expect("rotate fixture binding");
        let changed = registered_project(&catdesk, &sibling_scope).expect("post-lock scope");
        assert!(ensure_same_project_binding(&selected, &changed).is_err());
        assert!(
            StdCommand::new("git")
                .arg("-C")
                .arg(&cross.workspace)
                .args([
                    "remote",
                    "set-url",
                    "origin",
                    "https://example.test/drift.git"
                ])
                .status()
                .expect("drift sibling origin")
                .success()
        );
        assert!(registered_project(&catdesk, &sibling_scope).is_err());
        assert!(
            StdCommand::new("git")
                .arg("-C")
                .arg(&catdesk)
                .args([
                    "remote",
                    "set-url",
                    "origin",
                    "https://example.test/drift.git"
                ])
                .status()
                .expect("drift control origin")
                .success()
        );
        assert!(registered_project(&catdesk, &good).is_err());
        let resolved = root.canonicalize().expect("fixture root");
        assert!(resolved.starts_with(&temp_root));
        assert!(
            resolved
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("catdesk-operator-codex-")
        );
        fs::remove_dir_all(resolved).expect("cleanup fixture");
    }
}
