//! Closed, local-only operator actions that must finish before CatDesk starts
//! its TUI, daemon, or MCP listener.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::command::{self, CommandResult};
use crate::mcp;

const REPAIR_TIMEOUT_MS: u64 = 10 * 60 * 1000;
const MAX_REPAIR_SCRIPT_BYTES: u64 = 128 * 1024;
const REPAIR_SUCCESS_MARKER: &str = "WAKE_BRIDGE_ENV_REPAIRED";
const MAX_REPAIR_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperatorAction {
    SetWakeTarget {
        conversation_url: String,
        expected_current_target_sha256: Option<String>,
    },
    SetDesignatedChatTarget {
        conversation_url: String,
        expected_current_target_sha256: String,
    },
    ReviewedBuild(ReviewedBuildOperatorAction),
    RepairWakeBridgeEnvironment,
    CoreHostAcceptancePreflight,
    Supervisor(crate::supervisor_lifecycle::SupervisorOperatorActionV1),
    ProjectControl {
        operation: &'static str,
        arguments: serde_json::Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReviewedBuildOperatorAction {
    Prepare { review_record_id: String },
    Confirm { confirmation_token: String },
    Result,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepairInvocation {
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<String>,
    pub(crate) workspace: PathBuf,
}

pub(crate) fn parse_operator_action(
    args: impl IntoIterator<Item = String>,
) -> Result<Option<OperatorAction>, String> {
    let args = args.into_iter().collect::<Vec<_>>();
    if args.first().map(String::as_str) != Some("operator") {
        return Ok(None);
    }
    match args.get(1).map(String::as_str) {
        Some("supervisor") => crate::supervisor_lifecycle::parse_supervisor_operator_action(&args)
            .map(OperatorAction::Supervisor)
            .map(Some),
        Some("core-acceptance") => parse_core_host_acceptance_action(&args),
        Some("wake-target") => parse_wake_target_action(&args),
        Some("designated-chat-target") => parse_designated_chat_target_action(&args),
        Some("reviewed-build") => parse_reviewed_build_action(&args),
        Some("repair-wake-bridge-environment") if args.len() == 2 => {
            Ok(Some(OperatorAction::RepairWakeBridgeEnvironment))
        }
        Some("repair-wake-bridge-environment") => {
            Err("operator repair wake bridge environment accepts no arguments".into())
        }
        Some("project") => parse_project_control_action(&args),
        _ => Err("unknown operator action".into()),
    }
}

fn parse_reviewed_build_action(args: &[String]) -> Result<Option<OperatorAction>, String> {
    let action = args
        .get(2)
        .map(String::as_str)
        .ok_or_else(|| "operator reviewed build requires a fixed action".to_string())?;
    let parsed = match action {
        "prepare" => ReviewedBuildOperatorAction::Prepare {
            review_record_id: parse_reviewed_build_single_value(args, "--review-record-id")?,
        },
        "confirm" => ReviewedBuildOperatorAction::Confirm {
            confirmation_token: parse_reviewed_build_single_value(args, "--confirmation-token")?,
        },
        "result" if args.len() == 3 => ReviewedBuildOperatorAction::Result,
        "result" => return Err("operator reviewed build result accepts no arguments".into()),
        _ => return Err("operator reviewed build accepts only fixed actions".into()),
    };
    Ok(Some(OperatorAction::ReviewedBuild(parsed)))
}

fn parse_reviewed_build_single_value(
    args: &[String],
    expected_flag: &str,
) -> Result<String, String> {
    let value = (args.len() == 5 && args.get(3).map(String::as_str) == Some(expected_flag))
        .then(|| args[4].clone())
        .filter(|value| valid_reviewed_build_operator_value(value))
        .ok_or_else(|| "operator reviewed build arguments are invalid".to_string())?;
    Ok(value)
}

fn valid_reviewed_build_operator_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn reviewed_build_request(action: &ReviewedBuildOperatorAction) -> serde_json::Value {
    match action {
        ReviewedBuildOperatorAction::Prepare { review_record_id } => {
            json!({"action":"PREFLIGHT","recordId":review_record_id})
        }
        ReviewedBuildOperatorAction::Confirm { confirmation_token } => {
            json!({"action":"CONFIRM","confirmationToken":confirmation_token})
        }
        ReviewedBuildOperatorAction::Result => json!({"action":"RESULT"}),
    }
}

/// The core acceptance reader has no options.  In particular it cannot be
/// turned into a browser, target, supervisor, or host-lifecycle operation by
/// caller-supplied text.
fn parse_core_host_acceptance_action(args: &[String]) -> Result<Option<OperatorAction>, String> {
    if args.len() == 3
        && args.first().map(String::as_str) == Some("operator")
        && args.get(1).map(String::as_str) == Some("core-acceptance")
        && args.get(2).map(String::as_str) == Some("preflight")
    {
        Ok(Some(OperatorAction::CoreHostAcceptancePreflight))
    } else {
        Err("operator core-acceptance accepts only the fixed preflight action".into())
    }
}

fn parse_project_control_action(args: &[String]) -> Result<Option<OperatorAction>, String> {
    let command = args.get(2).map(String::as_str);
    let action = args.get(3).map(String::as_str);
    let (operation, allowed): (&str, &[&str]) = match (command, action) {
        (Some("chat-target"), Some("bind")) => (
            "autonomy_project_registry_chat_target_bind",
            &[
                "--project-id",
                "--conversation-url",
                "--expected-current-target-sha256",
            ],
        ),
        (Some("registration"), Some("preflight")) => (
            "autonomy_project_registry_preflight",
            &[
                "--project-id",
                "--workspace",
                "--git-identity",
                "--verification-profile",
            ],
        ),
        (Some("registration"), Some("confirm")) => (
            "autonomy_project_registry_confirm",
            &["--confirmation-token"],
        ),
        (Some("thread-adoption"), Some("preflight")) => (
            "autonomy_project_thread_adoption_preflight",
            &["--project-id", "--workspace"],
        ),
        (Some("thread-adoption"), Some("confirm")) => (
            "autonomy_project_thread_adoption_confirm",
            &["--confirmation-token", "--candidate-handle"],
        ),
        _ => return Err("operator project accepts only reviewed fixed actions".into()),
    };
    let mut values = std::collections::BTreeMap::new();
    let mut index = 4;
    while index < args.len() {
        let flag = args
            .get(index)
            .ok_or_else(|| "operator project arguments are invalid".to_string())?;
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("{flag} requires a value"))?;
        index += 2;
        if !allowed.contains(&flag.as_str())
            || values.insert(flag.as_str(), value.clone()).is_some()
            || value.is_empty()
            || value.len() > 4096
        {
            return Err("operator project arguments are invalid".into());
        }
    }
    let required = match operation {
        "autonomy_project_registry_chat_target_bind" => {
            ["--project-id", "--conversation-url"].as_slice()
        }
        "autonomy_project_registry_preflight" => [
            "--project-id",
            "--workspace",
            "--git-identity",
            "--verification-profile",
        ]
        .as_slice(),
        "autonomy_project_registry_confirm" => ["--confirmation-token"].as_slice(),
        "autonomy_project_thread_adoption_preflight" => ["--project-id", "--workspace"].as_slice(),
        _ => ["--confirmation-token", "--candidate-handle"].as_slice(),
    };
    if !required.iter().all(|flag| values.contains_key(flag)) {
        return Err("operator project arguments are incomplete".into());
    }
    let arguments = json!({
        "projectId": values.get("--project-id"), "conversationUrl": values.get("--conversation-url"),
        "expectedCurrentTargetSha256": values.get("--expected-current-target-sha256"), "workspace": values.get("--workspace"),
        "gitIdentity": values.get("--git-identity"), "verificationProfile": values.get("--verification-profile"),
        "confirmationToken": values.get("--confirmation-token"), "candidateHandle": values.get("--candidate-handle"),
    });
    Ok(Some(OperatorAction::ProjectControl {
        operation,
        arguments,
    }))
}

fn parse_wake_target_action(args: &[String]) -> Result<Option<OperatorAction>, String> {
    if args.get(2).map(String::as_str) != Some("set") {
        return Err("operator wake target requires the fixed `set` action".into());
    }
    let mut conversation_url = None;
    let mut expected_current_target_sha256 = None;
    let mut index = 3;
    while index < args.len() {
        let option = &args[index];
        index += 1;
        let value = args
            .get(index)
            .ok_or_else(|| format!("{option} requires a value"))?
            .clone();
        index += 1;
        match option.as_str() {
            "--conversation-url" if conversation_url.is_none() => conversation_url = Some(value),
            "--expected-current-target-sha256" if expected_current_target_sha256.is_none() => {
                expected_current_target_sha256 = Some(value)
            }
            "--conversation-url" | "--expected-current-target-sha256" => {
                return Err(format!("{option} may only be supplied once"));
            }
            _ => return Err("operator wake target accepts only fixed options".into()),
        }
    }
    let conversation_url = conversation_url
        .ok_or_else(|| "operator wake target requires --conversation-url".to_string())?;
    Ok(Some(OperatorAction::SetWakeTarget {
        conversation_url,
        expected_current_target_sha256,
    }))
}

fn parse_designated_chat_target_action(args: &[String]) -> Result<Option<OperatorAction>, String> {
    if args.get(2).map(String::as_str) != Some("set") {
        return Err("operator designated chat target requires the fixed `set` action".into());
    }
    let mut conversation_url = None;
    let mut expected_current_target_sha256 = None;
    let mut index = 3;
    while index < args.len() {
        let option = &args[index];
        index += 1;
        let value = args
            .get(index)
            .ok_or_else(|| format!("{option} requires a value"))?
            .clone();
        index += 1;
        match option.as_str() {
            "--conversation-url" if conversation_url.is_none() => conversation_url = Some(value),
            "--expected-current-target-sha256" if expected_current_target_sha256.is_none() => {
                expected_current_target_sha256 = Some(value)
            }
            "--conversation-url" | "--expected-current-target-sha256" => {
                return Err(format!("{option} may only be supplied once"));
            }
            _ => return Err("operator designated chat target accepts only fixed options".into()),
        }
    }
    Ok(Some(OperatorAction::SetDesignatedChatTarget {
        conversation_url: conversation_url.ok_or_else(|| {
            "operator designated chat target requires --conversation-url".to_string()
        })?,
        expected_current_target_sha256: expected_current_target_sha256.ok_or_else(|| {
            "operator designated chat target requires --expected-current-target-sha256".to_string()
        })?,
    }))
}

pub(crate) async fn execute_operator_action(
    action: OperatorAction,
    workspace: &Path,
) -> Result<serde_json::Value, String> {
    match action {
        OperatorAction::Supervisor(action) => {
            let _ = workspace;
            Ok(crate::supervisor_lifecycle::execute_supervisor_operator_action(action))
        }
        OperatorAction::CoreHostAcceptancePreflight => Ok(
            crate::core_host_acceptance_preflight::read_fixed_core_host_acceptance_preflight(
                workspace,
            )
            .as_json(),
        ),
        OperatorAction::SetWakeTarget {
            conversation_url,
            expected_current_target_sha256,
        } => {
            let workspace = workspace
                .canonicalize()
                .map_err(|_| "operator workspace is unavailable".to_string())?;
            let result = mcp::operator_set_wake_target(
                &workspace,
                &conversation_url,
                expected_current_target_sha256.as_deref(),
            )?;
            Ok(json!({
                "action": "wake_target_set",
                "status": "success",
                "targetSha256": result.target_sha256,
                "hostPathIdentity": result.host_path_identity,
            }))
        }
        OperatorAction::SetDesignatedChatTarget {
            conversation_url,
            expected_current_target_sha256,
        } => {
            let workspace = workspace
                .canonicalize()
                .map_err(|_| "operator workspace is unavailable".to_string())?;
            let result = mcp::operator_update_designated_chat_target(
                &workspace,
                &conversation_url,
                &expected_current_target_sha256,
            )
            .map_err(|_| "designated ChatGPT target update was rejected".to_string())?;
            Ok(json!({
                "action": "designated_chat_target_set",
                "status": "success",
                "targetSha256": result.sha256,
                "targetUrl": result.url,
            }))
        }
        OperatorAction::ReviewedBuild(action) => {
            let workspace = workspace
                .canonicalize()
                .map_err(|_| "operator workspace is unavailable".to_string())?;
            let request = reviewed_build_request(&action);
            let result = crate::delegated::autonomy_supervisor::handle_tool(
                "catdesk_reviewed_build",
                request,
                &workspace,
            )
            .map_err(|_| "reviewed build operation was rejected".to_string())?;
            match action {
                ReviewedBuildOperatorAction::Prepare { .. } => Ok(json!({
                    "action":"reviewed_build_prepare",
                    "state":result.get("state").cloned(),
                    "confirmationToken":result.get("confirmationToken").cloned(),
                })),
                ReviewedBuildOperatorAction::Confirm { .. } => Ok(json!({
                    "action":"reviewed_build_confirm",
                    "state":result.get("state").cloned(),
                })),
                ReviewedBuildOperatorAction::Result => Ok(json!({
                    "action":"reviewed_build_result",
                    "state":result.get("state").cloned(),
                })),
            }
        }
        OperatorAction::RepairWakeBridgeEnvironment => {
            let workspace = workspace
                .canonicalize()
                .map_err(|_| "operator workspace is unavailable".to_string())?;
            let invocation = repair_wake_bridge_invocation(&workspace)?;
            let result = run_wake_repair_process(invocation).await?;
            if !valid_repair_result(&result) {
                return Err("wake bridge environment repair requires operator attention".into());
            }
            Ok(json!({
                "action": "repair_wake_bridge_environment",
                "status": "success",
            }))
        }
        OperatorAction::ProjectControl {
            operation,
            arguments,
        } => {
            let workspace = workspace
                .canonicalize()
                .map_err(|_| "operator workspace is unavailable".to_string())?;
            let result = crate::delegated::autonomy_supervisor::handle_tool(
                operation, arguments, &workspace,
            )
            .map_err(|_| "reviewed project operation requires operator attention".to_string())?;
            Ok(redacted_project_control_result(operation, result))
        }
    }
}

fn redacted_project_control_result(
    operation: &str,
    result: serde_json::Value,
) -> serde_json::Value {
    let field = |name: &str| result.get(name).cloned();
    match operation {
        "autonomy_project_registry_chat_target_bind" => {
            json!({"action":"project_chat_target_bind","status":"success","projectId":field("projectId"),"targetSha256":field("chatgptTargetSha256")})
        }
        "autonomy_project_registry_preflight" => {
            json!({"action":"project_registration_preflight","status":"success","confirmationToken":field("confirmationToken"),"expiresAtUnix":field("expiresAtUnix"),"confirmationFingerprint":field("confirmationFingerprint")})
        }
        "autonomy_project_registry_confirm" => {
            json!({"action":"project_registration_confirm","status":"success","projectId":field("projectId")})
        }
        "autonomy_project_thread_adoption_preflight" => {
            json!({"action":"project_thread_adoption_preflight","status":"success","confirmationToken":field("confirmationToken"),"expiresAtUnix":field("expiresAtUnix"),"candidates":field("candidates")})
        }
        _ => {
            json!({"action":"project_thread_adoption_confirm","status":"success","projectId":field("projectId")})
        }
    }
}

fn path_is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

fn checked_regular_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| "fixed wake repair boundary is unavailable".to_string())?;
    if !metadata.is_dir() || path_is_reparse_or_symlink(&metadata) {
        return Err("fixed wake repair boundary is unsafe".into());
    }
    Ok(())
}

pub(crate) fn repair_wake_bridge_invocation(workspace: &Path) -> Result<RepairInvocation, String> {
    let workspace = workspace
        .canonicalize()
        .map(command::normalize_windows_verbatim_path)
        .map_err(|_| "fixed wake repair boundary is unavailable".to_string())?;
    checked_regular_directory(&workspace)?;
    let scripts = workspace.join("scripts");
    checked_regular_directory(&scripts)?;
    let expected = scripts.join("repair_wake_bridge_environment.ps1");
    let metadata = fs::symlink_metadata(&expected)
        .map_err(|_| "fixed wake repair script is unavailable".to_string())?;
    if !metadata.is_file()
        || path_is_reparse_or_symlink(&metadata)
        || metadata.len() == 0
        || metadata.len() > MAX_REPAIR_SCRIPT_BYTES
    {
        return Err("fixed wake repair script is unsafe".into());
    }
    let canonical = expected
        .canonicalize()
        .map(command::normalize_windows_verbatim_path)
        .map_err(|_| "fixed wake repair script is unavailable".to_string())?;
    if canonical != expected {
        return Err("fixed wake repair script is unsafe".into());
    }
    Ok(RepairInvocation {
        program: trusted_windows_powershell(&workspace)?,
        args: vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            expected.to_string_lossy().into_owned(),
        ],
        workspace,
    })
}

pub(crate) fn trusted_windows_powershell(workspace: &Path) -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        let system_root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .ok_or_else(|| "trusted Windows PowerShell is unavailable".to_string())?;
        resolve_trusted_windows_powershell(&system_root, workspace)
    }
    #[cfg(not(windows))]
    {
        let _ = workspace;
        Err("trusted Windows PowerShell is unavailable".into())
    }
}

/// Fixed reviewed-promotion invocation. The caller supplies only a canonical
/// workspace candidate and an opaque Rust-issued authorization ID; no shell,
/// script, executable, credential, or tunnel selection is accepted.
pub(crate) fn reviewed_promotion_invocation(
    workspace: &Path,
    candidate: &Path,
    authorization_id: &str,
    transaction_id: &str,
) -> Result<RepairInvocation, String> {
    if authorization_id.len() != 32
        || !authorization_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("reviewed promotion authorization is invalid".into());
    }
    if transaction_id.len() != 32 || !transaction_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("reviewed promotion transaction is invalid".into());
    }
    let workspace = workspace
        .canonicalize()
        .map(command::normalize_windows_verbatim_path)
        .map_err(|_| "reviewed promotion workspace is unavailable".to_string())?;
    checked_regular_directory(&workspace)?;
    let script = workspace
        .join("scripts")
        .join("promote-reviewed-catdesk-build.ps1");
    let script_meta = fs::symlink_metadata(&script)
        .map_err(|_| "reviewed promotion script is unavailable".to_string())?;
    if !script_meta.is_file()
        || path_is_reparse_or_symlink(&script_meta)
        || script_meta.len() == 0
        || script_meta.len() > MAX_REPAIR_SCRIPT_BYTES
    {
        return Err("reviewed promotion script is unsafe".into());
    }
    let script = script
        .canonicalize()
        .map(command::normalize_windows_verbatim_path)
        .map_err(|_| "reviewed promotion script is unavailable".to_string())?;
    if !script.starts_with(&workspace) {
        return Err("reviewed promotion script is unsafe".into());
    }
    let candidate = candidate
        .canonicalize()
        .map(command::normalize_windows_verbatim_path)
        .map_err(|_| "reviewed promotion candidate is unavailable".to_string())?;
    let candidate_meta = fs::symlink_metadata(&candidate)
        .map_err(|_| "reviewed promotion candidate is unavailable".to_string())?;
    if !candidate.starts_with(&workspace)
        || !candidate_meta.is_file()
        || path_is_reparse_or_symlink(&candidate_meta)
    {
        return Err("reviewed promotion candidate is unsafe".into());
    }
    Ok(RepairInvocation {
        program: trusted_windows_powershell(&workspace)?,
        args: vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            script.to_string_lossy().into_owned(),
            "-BuildPath".into(),
            candidate.to_string_lossy().into_owned(),
            "-Workspace".into(),
            workspace.to_string_lossy().into_owned(),
            "-Execute".into(),
            "-AuthorizationToken".into(),
            authorization_id.into(),
            "-TransactionId".into(),
            transaction_id.into(),
        ],
        workspace,
    })
}

fn resolve_trusted_windows_powershell(
    system_root: &Path,
    workspace: &Path,
) -> Result<PathBuf, String> {
    if !system_root.is_absolute() {
        return Err("trusted Windows PowerShell is unavailable".into());
    }
    let system_root = system_root
        .canonicalize()
        .map(command::normalize_windows_verbatim_path)
        .map_err(|_| "trusted Windows PowerShell is unavailable".to_string())?;
    let workspace = workspace
        .canonicalize()
        .map(command::normalize_windows_verbatim_path)
        .map_err(|_| "trusted Windows PowerShell is unavailable".to_string())?;
    if system_root.starts_with(&workspace) {
        return Err("trusted Windows PowerShell is unsafe".into());
    }
    checked_regular_directory(&system_root)?;
    let expected = system_root
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let metadata = fs::symlink_metadata(&expected)
        .map_err(|_| "trusted Windows PowerShell is unavailable".to_string())?;
    if !metadata.is_file() || path_is_reparse_or_symlink(&metadata) {
        return Err("trusted Windows PowerShell is unsafe".into());
    }
    let canonical = expected
        .canonicalize()
        .map(command::normalize_windows_verbatim_path)
        .map_err(|_| "trusted Windows PowerShell is unavailable".to_string())?;
    if canonical != expected || canonical.starts_with(&workspace) {
        return Err("trusted Windows PowerShell is unsafe".into());
    }
    Ok(canonical)
}

async fn run_wake_repair_process(invocation: RepairInvocation) -> Result<CommandResult, String> {
    tokio::task::spawn_blocking(move || run_fixed_powershell_invocation_blocking(invocation))
        .await
        .map_err(|_| "wake bridge environment repair requires operator attention".to_string())?
}

pub(crate) fn run_fixed_powershell_invocation_blocking(
    invocation: RepairInvocation,
) -> Result<CommandResult, String> {
    let started = Instant::now();
    let mut child = Command::new(&invocation.program)
        .args(&invocation.args)
        .current_dir(&invocation.workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "wake bridge environment repair requires operator attention".to_string())?;
    #[cfg(windows)]
    let mut job = match WindowsKillOnCloseJob::assign(&child) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };

    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let stdout_reader = thread::spawn(move || bounded_reader(stdout));
    let stderr_reader = thread::spawn(move || bounded_reader(stderr));
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => {
                #[cfg(windows)]
                let _ = terminate_and_wait_tree(
                    || job.close_kill_tree(),
                    || {
                        child.wait().map(|_| ()).map_err(|_| {
                            "wake bridge environment repair requires operator attention".to_string()
                        })
                    },
                );
                #[cfg(not(windows))]
                {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return Err("wake bridge environment repair requires operator attention".into());
            }
        }
        if started.elapsed() >= Duration::from_millis(REPAIR_TIMEOUT_MS) {
            timed_out = true;
            #[cfg(windows)]
            terminate_and_wait_tree(
                || job.close_kill_tree(),
                || {
                    child.wait().map(|_| ()).map_err(|_| {
                        "wake bridge environment repair requires operator attention".to_string()
                    })
                },
            )?;
            #[cfg(not(windows))]
            {
                child.kill().map_err(|_| {
                    "wake bridge environment repair requires operator attention".to_string()
                })?;
                child.wait().map_err(|_| {
                    "wake bridge environment repair requires operator attention".to_string()
                })?;
            }
            break child
                .try_wait()
                .map_err(|_| {
                    "wake bridge environment repair requires operator attention".to_string()
                })?
                .ok_or_else(|| {
                    "wake bridge environment repair requires operator attention".to_string()
                })?;
        }
        thread::sleep(Duration::from_millis(20));
    };
    #[cfg(windows)]
    job.close_after_exit();
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    if stdout.overflow || stderr.overflow || stdout.read_failed || stderr.read_failed {
        return Err("wake bridge environment repair requires operator attention".into());
    }
    Ok(CommandResult {
        stdout: stdout.text,
        stderr: stderr.text,
        success: !timed_out && status.success(),
        exit_code: status.code(),
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

#[derive(Default)]
struct BoundedOutput {
    text: String,
    overflow: bool,
    read_failed: bool,
}

fn bounded_reader<R: Read>(mut reader: R) -> BoundedOutput {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Err(_) => {
                return BoundedOutput {
                    text: String::from_utf8_lossy(&bytes).into_owned(),
                    overflow: false,
                    read_failed: true,
                };
            }
            Ok(count) if bytes.len() < MAX_REPAIR_OUTPUT_BYTES => {
                let available = MAX_REPAIR_OUTPUT_BYTES - bytes.len();
                bytes.extend_from_slice(&buffer[..count.min(available)]);
                if count > available {
                    return BoundedOutput {
                        text: String::from_utf8_lossy(&bytes).into_owned(),
                        overflow: true,
                        read_failed: false,
                    };
                }
            }
            Ok(_) => {
                return BoundedOutput {
                    text: String::from_utf8_lossy(&bytes).into_owned(),
                    overflow: true,
                    read_failed: false,
                };
            }
        }
    }
    BoundedOutput {
        text: String::from_utf8_lossy(&bytes).into_owned(),
        overflow: false,
        read_failed: false,
    }
}

fn terminate_and_wait_tree<T, W>(terminate: T, wait: W) -> Result<(), String>
where
    T: FnOnce() -> Result<(), String>,
    W: FnOnce() -> Result<(), String>,
{
    // Waiting for the direct child is required even if the tree termination
    // operation reports an error.  Returning early here could leave the
    // PowerShell parent alive while we report attention to the operator.
    let termination = terminate();
    let waiting = wait();
    termination.and(waiting)
}

#[cfg(windows)]
struct WindowsKillOnCloseJob(*mut std::ffi::c_void);

#[cfg(windows)]
impl WindowsKillOnCloseJob {
    fn assign(child: &std::process::Child) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err("wake bridge environment repair requires operator attention".into());
        }
        let mut information = JobObjectExtendedLimitInformation::default();
        information.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                &mut information as *mut _ as *mut _,
                std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
            )
        };
        let assigned = configured != 0
            && unsafe { AssignProcessToJobObject(handle, child.as_raw_handle() as *mut _) } != 0;
        if !assigned {
            unsafe { CloseHandle(handle) };
            return Err("wake bridge environment repair requires operator attention".into());
        }
        Ok(Self(handle))
    }

    fn close_kill_tree(&mut self) -> Result<(), String> {
        if self.0.is_null() {
            return Err("wake bridge environment repair requires operator attention".into());
        }
        let handle = std::mem::replace(&mut self.0, std::ptr::null_mut());
        if unsafe { CloseHandle(handle) } == 0 {
            return Err("wake bridge environment repair requires operator attention".into());
        }
        Ok(())
    }

    fn close_after_exit(&mut self) {
        if !self.0.is_null() {
            let handle = std::mem::replace(&mut self.0, std::ptr::null_mut());
            unsafe { CloseHandle(handle) };
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsKillOnCloseJob {
    fn drop(&mut self) {
        self.close_after_exit();
    }
}

#[cfg(windows)]
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
#[cfg(windows)]
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct JobObjectBasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct IoCounters {
    read_operation_count: u64,
    write_operation_count: u64,
    other_operation_count: u64,
    read_transfer_count: u64,
    write_transfer_count: u64,
    other_transfer_count: u64,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct JobObjectExtendedLimitInformation {
    basic: JobObjectBasicLimitInformation,
    io: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(
        attributes: *const std::ffi::c_void,
        name: *const u16,
    ) -> *mut std::ffi::c_void;
    fn SetInformationJobObject(
        job: *mut std::ffi::c_void,
        information_class: i32,
        information: *mut std::ffi::c_void,
        information_length: u32,
    ) -> i32;
    fn AssignProcessToJobObject(job: *mut std::ffi::c_void, process: *mut std::ffi::c_void) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

pub(crate) fn valid_repair_result(result: &CommandResult) -> bool {
    result.success
        && result.stderr.trim().is_empty()
        && result.stdout.len() <= MAX_REPAIR_OUTPUT_BYTES
        && result
            .stdout
            .lines()
            .any(|line| line.trim() == REPAIR_SUCCESS_MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sha2::Digest;
    use uuid::Uuid;

    fn repair_fixture(label: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("catdesk-operator-{label}-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("scripts")).expect("fixture scripts");
        fs::write(
            root.join("scripts")
                .join("repair_wake_bridge_environment.ps1"),
            "Write-Output WAKE_BRIDGE_ENV_REPAIRED\n",
        )
        .expect("fixture repair script");
        root
    }

    fn trusted_powershell_fixture(label: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base =
            std::env::temp_dir().join(format!("catdesk-powershell-{label}-{}", Uuid::new_v4()));
        let system_root = base.join("SystemRoot");
        let workspace = base.join("workspace");
        let executable = system_root
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        fs::create_dir_all(executable.parent().expect("PowerShell parent")).expect("system root");
        fs::create_dir_all(&workspace).expect("workspace");
        fs::write(&executable, b"fixture PowerShell").expect("PowerShell fixture");
        (system_root, workspace, executable)
    }

    #[test]
    fn parser_accepts_only_closed_operator_actions() {
        assert_eq!(
            parse_operator_action(
                ["operator", "repair-wake-bridge-environment"].map(str::to_string)
            ),
            Ok(Some(OperatorAction::RepairWakeBridgeEnvironment))
        );
        let action = parse_operator_action(
            [
                "operator",
                "project",
                "thread-adoption",
                "confirm",
                "--confirmation-token",
                "adopt-0123456789abcdef0123456789abcdef",
                "--candidate-handle",
                "candidate-0123456789abcdef0123456789abcdef",
            ]
            .map(str::to_string),
        )
        .expect("parse")
        .expect("action");
        assert!(matches!(
            action,
            OperatorAction::ProjectControl {
                operation: "autonomy_project_thread_adoption_confirm",
                ..
            }
        ));
        assert_eq!(
            parse_operator_action(
                [
                    "operator",
                    "wake-target",
                    "set",
                    "--conversation-url",
                    "https://chatgpt.com/c/current-thread",
                    "--expected-current-target-sha256",
                    &"a".repeat(64),
                ]
                .map(str::to_string)
            ),
            Ok(Some(OperatorAction::SetWakeTarget {
                conversation_url: "https://chatgpt.com/c/current-thread".into(),
                expected_current_target_sha256: Some("a".repeat(64)),
            }))
        );
        assert_eq!(
            parse_operator_action(["operator", "core-acceptance", "preflight"].map(str::to_string)),
            Ok(Some(OperatorAction::CoreHostAcceptancePreflight))
        );
        assert_eq!(
            parse_operator_action(
                [
                    "operator",
                    "reviewed-build",
                    "prepare",
                    "--review-record-id",
                    "review-record-0123456789abcdef",
                ]
                .map(str::to_string)
            ),
            Ok(Some(OperatorAction::ReviewedBuild(
                ReviewedBuildOperatorAction::Prepare {
                    review_record_id: "review-record-0123456789abcdef".into(),
                }
            )))
        );
        assert_eq!(
            parse_operator_action(
                [
                    "operator",
                    "reviewed-build",
                    "confirm",
                    "--confirmation-token",
                    "0123456789abcdef0123456789abcdef",
                ]
                .map(str::to_string)
            ),
            Ok(Some(OperatorAction::ReviewedBuild(
                ReviewedBuildOperatorAction::Confirm {
                    confirmation_token: "0123456789abcdef0123456789abcdef".into(),
                }
            )))
        );
        assert_eq!(
            parse_operator_action(["operator", "reviewed-build", "result"].map(str::to_string)),
            Ok(Some(OperatorAction::ReviewedBuild(
                ReviewedBuildOperatorAction::Result
            )))
        );
        for invalid in [
            vec!["operator", "repair-wake-bridge-environment", "extra"],
            vec!["operator", "wake-target", "set"],
            vec![
                "operator",
                "wake-target",
                "set",
                "--conversation-url",
                "x",
                "--conversation-url",
                "y",
            ],
            vec!["operator", "wake-target", "set", "--workspace", "x"],
            vec![
                "operator",
                "project",
                "registration",
                "preflight",
                "--workspace",
                "x",
            ],
            vec!["operator", "project", "shell", "run", "--command", "whoami"],
            vec!["operator", "core-acceptance"],
            vec![
                "operator",
                "core-acceptance",
                "preflight",
                "--workspace",
                "x",
            ],
            vec!["operator", "core-acceptance", "activate"],
            vec!["operator", "reviewed-build", "prepare"],
            vec![
                "operator",
                "reviewed-build",
                "prepare",
                "--build-path",
                "target/release/catdesk.exe",
            ],
            vec![
                "operator",
                "reviewed-build",
                "prepare",
                "--review-record-id",
                "record;whoami",
            ],
            vec![
                "operator",
                "reviewed-build",
                "prepare",
                "--review-record-id",
                "..",
            ],
            vec![
                "operator",
                "reviewed-build",
                "prepare",
                "--review-record-id",
                "review-record",
                "--environment",
                "X=1",
            ],
            vec![
                "operator",
                "reviewed-build",
                "confirm",
                "--confirmation-token",
                "token",
                "--expected-sha256",
                "0",
            ],
            vec![
                "operator",
                "reviewed-build",
                "result",
                "--project-id",
                "catdesk",
            ],
        ] {
            assert!(parse_operator_action(invalid.into_iter().map(str::to_string)).is_err());
        }
        assert_eq!(
            parse_operator_action(["--native-daemon"].map(str::to_string)),
            Ok(None)
        );
    }

    #[test]
    fn reviewed_build_bridge_constructs_only_fixed_supervisor_requests() {
        let prepare = reviewed_build_request(&ReviewedBuildOperatorAction::Prepare {
            review_record_id: "review-record-0123456789abcdef".into(),
        });
        assert_eq!(
            prepare,
            serde_json::json!({"action":"PREFLIGHT","recordId":"review-record-0123456789abcdef"})
        );
        let confirm = reviewed_build_request(&ReviewedBuildOperatorAction::Confirm {
            confirmation_token: "0123456789abcdef0123456789abcdef".into(),
        });
        assert_eq!(
            confirm,
            serde_json::json!({"action":"CONFIRM","confirmationToken":"0123456789abcdef0123456789abcdef"})
        );
        assert_eq!(
            reviewed_build_request(&ReviewedBuildOperatorAction::Result),
            serde_json::json!({"action":"RESULT"})
        );
        for request in [
            prepare,
            confirm,
            reviewed_build_request(&ReviewedBuildOperatorAction::Result),
        ] {
            let object = request.as_object().expect("fixed request object");
            assert!(object.len() <= 2);
            assert!(!object.keys().any(|key| matches!(
                key.as_str(),
                "buildPath"
                    | "workspace"
                    | "sourcePath"
                    | "toolPath"
                    | "expectedSha256"
                    | "environment"
                    | "command"
                    | "projectId"
            )));
        }
    }

    #[test]
    fn repair_invocation_is_fixed_noninteractive_and_rejects_unsafe_path_shapes() {
        let root = repair_fixture("invocation");
        let invocation = repair_wake_bridge_invocation(&root).expect("fixed invocation");
        assert!(invocation.program.is_absolute());
        assert_eq!(
            invocation.args[..6],
            [
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File"
            ]
        );
        assert_eq!(invocation.args.len(), 7);
        assert!(
            !invocation
                .args
                .iter()
                .any(|value| value.contains("-PythonExecutable"))
        );
        fs::remove_file(
            root.join("scripts")
                .join("repair_wake_bridge_environment.ps1"),
        )
        .expect("remove fixture script");
        assert!(repair_wake_bridge_invocation(&root).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn trusted_powershell_resolution_is_absolute_and_never_uses_workspace_or_path() {
        let (system_root, workspace, expected) = trusted_powershell_fixture("trusted");
        fs::write(workspace.join("powershell.exe"), b"workspace hijack").expect("hijack fixture");
        let resolved = resolve_trusted_windows_powershell(&system_root, &workspace)
            .expect("trusted system PowerShell");
        assert_eq!(
            resolved,
            command::normalize_windows_verbatim_path(
                expected.canonicalize().expect("canonical expected")
            )
        );
        assert_ne!(resolved, workspace.join("powershell.exe"));
        assert!(resolve_trusted_windows_powershell(&workspace, &workspace).is_err());
        fs::remove_file(&expected).expect("remove trusted fixture");
        assert!(resolve_trusted_windows_powershell(&system_root, &workspace).is_err());
        let _ = fs::remove_dir_all(system_root.parent().expect("fixture base"));
    }

    #[test]
    fn repair_result_requires_clean_exit_and_exact_terminal_marker() {
        let result = |stdout: &str, stderr: &str, success: bool| CommandResult {
            stdout: stdout.into(),
            stderr: stderr.into(),
            success,
            exit_code: success.then_some(0),
            elapsed_ms: 1,
        };
        let success = result("PYTHON3_READY\nWAKE_BRIDGE_ENV_REPAIRED\n", "", true);
        assert!(valid_repair_result(&success));
        for invalid in [
            result("WAKE_BRIDGE_ENV_REPAIRED\n", "", false),
            result("WAKE_BRIDGE_ENV_REPAIRED\n", "error", true),
            result("PYTHON3_READY\n", "", true),
            result(&"x".repeat(64 * 1024 + 1), "", true),
        ] {
            assert!(!valid_repair_result(&invalid));
        }
    }

    #[test]
    fn timeout_tree_termination_always_waits_after_kill() {
        let events = std::cell::RefCell::new(Vec::new());
        terminate_and_wait_tree(
            || {
                events.borrow_mut().push("kill");
                Ok(())
            },
            || {
                events.borrow_mut().push("wait");
                Ok(())
            },
        )
        .expect("tree cleanup");
        assert_eq!(*events.borrow(), ["kill", "wait"]);

        let events = std::cell::RefCell::new(Vec::new());
        assert!(
            terminate_and_wait_tree(
                || {
                    events.borrow_mut().push("kill");
                    Ok(())
                },
                || {
                    events.borrow_mut().push("wait");
                    Err("wait failed".into())
                },
            )
            .is_err()
        );
        assert_eq!(*events.borrow(), ["kill", "wait"]);

        let events = std::cell::RefCell::new(Vec::new());
        assert!(
            terminate_and_wait_tree(
                || {
                    events.borrow_mut().push("kill");
                    Err("kill failed".into())
                },
                || {
                    events.borrow_mut().push("wait");
                    Ok(())
                },
            )
            .is_err()
        );
        assert_eq!(*events.borrow(), ["kill", "wait"]);
    }

    #[test]
    fn bounded_reader_marks_output_overflow_for_attention() {
        let captured = bounded_reader(std::io::Cursor::new(vec![
            b'x';
            MAX_REPAIR_OUTPUT_BYTES + 1
        ]));
        assert!(captured.overflow);
        assert_eq!(captured.text.len(), MAX_REPAIR_OUTPUT_BYTES);
    }

    #[test]
    fn bounded_reader_marks_io_failure_for_attention() {
        struct FailingReader;
        impl Read for FailingReader {
            fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("fixture read failure"))
            }
        }
        assert!(bounded_reader(FailingReader).read_failed);
    }

    #[tokio::test]
    async fn target_action_delegates_to_w4_without_normal_runtime_setup() {
        let root = std::env::temp_dir().join(format!("catdesk-operator-target-{}", Uuid::new_v4()));
        let wake_root = root.join(".catdesk").join("wake-bridge");
        fs::create_dir_all(&wake_root).expect("wake root");
        fs::write(
            wake_root.join("config.json"),
            serde_json::to_vec(&serde_json::json!({
                "conversation_url": "https://chatgpt.com/c/previous-thread",
                "profile_dir": ".catdesk/wake-bridge/browser-profile"
            }))
            .expect("config"),
        )
        .expect("config write");
        let previous_hash = format!(
            "{:x}",
            sha2::Sha256::digest(b"https://chatgpt.com/c/previous-thread")
        );
        let action = OperatorAction::SetWakeTarget {
            conversation_url: "https://chatgpt.com/c/current-thread".into(),
            expected_current_target_sha256: Some(previous_hash),
        };
        let result = execute_operator_action(action, &root)
            .await
            .expect("delegated target update");
        assert_eq!(result["action"], Value::String("wake_target_set".into()));
        assert!(
            !root
                .join(".catdesk")
                .join("wake-bridge")
                .join("state.json")
                .exists()
        );
        let _ = fs::remove_dir_all(root);
    }
}
