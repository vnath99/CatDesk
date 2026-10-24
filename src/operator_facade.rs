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

#[path = "operator_web_cli.rs"]
mod web;

pub(crate) use web::WebWorkLease;

#[cfg(test)]
pub(crate) fn read_web_status(workspace: &Path) -> Result<serde_json::Value, String> {
    web::read_status(workspace)
}

pub(crate) fn read_web_summary(workspace: &Path) -> Result<serde_json::Value, String> {
    web::read_summary(workspace)
}

pub(crate) fn start_web_auto_scheduler(state: crate::state::SharedState) {
    web::start_auto_scheduler(state);
}

pub(crate) fn start_scoped_web_auto_scheduler(
    state: crate::state::SharedState,
    projects: std::collections::BTreeSet<String>,
) {
    web::start_scoped_auto_scheduler(state, projects);
}

pub(crate) async fn stop_web_auto_scheduler(state: crate::state::SharedState) {
    web::stop_auto_scheduler(state).await;
}

pub(crate) async fn join_web_auto_schedulers(tasks: Vec<tokio::task::JoinHandle<()>>) {
    web::join_auto_schedulers(tasks).await;
}

pub(crate) fn registry_rollover_status(workspace: &Path, project: &str) -> serde_json::Value {
    web::registry_rollover_status(workspace, project)
}

pub(crate) fn capture_web_context_expected_guarded(
    workspace: &Path,
    project: &str,
    expected: (u64, &str),
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<serde_json::Value, String> {
    web::capture_context_expected_guarded(workspace, project, expected, keep_running)
}

pub(crate) fn read_web_review_expected_guarded(
    workspace: &Path,
    project: &str,
    event_id: &str,
    expected: (u64, &str),
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<serde_json::Value, String> {
    web::read_review_guarded(workspace, project, event_id, Some(expected), keep_running)
}

#[cfg(test)]
pub(crate) fn capture_web_context_with_snapshot<F>(
    workspace: &Path,
    project: &str,
    expected: (u64, &str),
    keep_running: &dyn Fn() -> Result<(), String>,
    snapshot: F,
) -> Result<serde_json::Value, String>
where
    F: FnOnce(
        &catdesk_wake::store::Store,
        &str,
        &catdesk_wake::protocol::Target,
        &dyn Fn() -> Result<(), String>,
    ) -> Result<serde_json::Value, String>,
{
    web::capture_context_with_snapshot(workspace, project, Some(expected), keep_running, snapshot)
}

pub(crate) fn acquire_web_work_lease(
    workspace: &Path,
    project: &str,
) -> Result<web::WebWorkLease, String> {
    web::acquire_web_work_lease(workspace, project)
}

const REPAIR_TIMEOUT_MS: u64 = 10 * 60 * 1000;
const MAX_REPAIR_SCRIPT_BYTES: u64 = 128 * 1024;
const REPAIR_SUCCESS_MARKER: &str = "WAKE_BRIDGE_ENV_REPAIRED";
const MAX_REPAIR_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperatorAction {
    Codex(crate::operator_codex_cli::OperatorCodexAction),
    Web(web::WebAction),
    WebDevelopment(WebDevelopmentAction),
    SetWakeTarget {
        conversation_url: String,
        expected_current_target_sha256: Option<String>,
    },
    SetDesignatedChatTarget {
        conversation_url: String,
        expected_current_target_sha256: String,
    },
    ReviewedBuild(ReviewedBuildOperatorAction),
    ReleasePair(ReleasePairOperatorAction),
    RepairWakeBridgeEnvironment,
    CoreHostAcceptancePreflight,
    Supervisor(crate::supervisor_lifecycle::SupervisorOperatorActionV1),
    ProjectControl {
        operation: &'static str,
        arguments: serde_json::Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WebDevelopmentAction {
    Prepare {
        package: PathBuf,
        python_runtime: PathBuf,
    },
    UpdatePackage {
        package: PathBuf,
    },
    RecoverPackage,
    RecordVerification {
        input_file: PathBuf,
    },
    Enqueue {
        project_id: String,
        event_id: String,
        message_file: PathBuf,
    },
    Retry {
        project_id: String,
        event_id: String,
    },
    Login {
        project_id: String,
    },
    Status,
    Start,
    Stop,
    Run,
    RunBounded {
        maximum_seconds: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReviewedBuildOperatorAction {
    Prepare { review_record_id: String },
    Confirm { confirmation_token: String },
    Result,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReleasePairOperatorAction {
    Prepare {
        review_record_id: String,
        build_path: PathBuf,
        expected_sha256: String,
    },
    Confirm {
        confirmation_token: String,
    },
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
    if args.first().map(String::as_str) == Some("codex") {
        let mut canonical = vec!["operator".to_string()];
        canonical.extend(args);
        return crate::operator_codex_cli::parse_action(&canonical)
            .map(OperatorAction::Codex)
            .map(Some);
    }
    if args.first().map(String::as_str) != Some("operator") {
        return Ok(None);
    }
    match args.get(1).map(String::as_str) {
        Some("web-dev-runtime") => parse_web_development_action(&args)
            .map(OperatorAction::WebDevelopment)
            .map(Some),
        Some("web") => web::parse(&args).map(OperatorAction::Web).map(Some),
        Some("codex") => crate::operator_codex_cli::parse_action(&args)
            .map(OperatorAction::Codex)
            .map(Some),
        Some("supervisor") => crate::supervisor_lifecycle::parse_supervisor_operator_action(&args)
            .map(OperatorAction::Supervisor)
            .map(Some),
        Some("core-acceptance") => parse_core_host_acceptance_action(&args),
        Some("wake-target") => parse_wake_target_action(&args),
        Some("designated-chat-target") => parse_designated_chat_target_action(&args),
        Some("reviewed-build") => parse_reviewed_build_action(&args),
        Some("release-pair") => parse_release_pair_action(&args),
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

fn parse_web_development_action(args: &[String]) -> Result<WebDevelopmentAction, String> {
    match args.get(2).map(String::as_str) {
        Some("login") if args.len() == 5
            && args[3] == "--project-id"
            && catdesk_wake::protocol::valid_id(&args[4]) => Ok(WebDevelopmentAction::Login {
                project_id: args[4].clone(),
            }),
        Some("retry") if args.len() == 7
            && args[3] == "--project-id" && args[5] == "--event-id"
            && catdesk_wake::protocol::valid_id(&args[4])
            && catdesk_wake::protocol::valid_id(&args[6])
            && args[6].starts_with("dev-") => Ok(WebDevelopmentAction::Retry {
                project_id: args[4].clone(), event_id: args[6].clone(),
            }),
        Some("enqueue") if args.len() == 9
            && args[3] == "--project-id" && args[5] == "--event-id" && args[7] == "--message-file"
            && catdesk_wake::protocol::valid_id(&args[4])
            && catdesk_wake::protocol::valid_id(&args[6])
            && args[6].starts_with("dev-")
            && !args[8].is_empty() && args[8].len() <= 4096
            && !args[8].chars().any(char::is_control) => Ok(WebDevelopmentAction::Enqueue {
                project_id: args[4].clone(), event_id: args[6].clone(), message_file: PathBuf::from(&args[8]),
            }),
        Some("prepare")
            if args.len() == 7
                && args[3] == "--package"
                && args[5] == "--python-runtime"
                && [&args[4], &args[6]].iter().all(|value| {
                    !value.is_empty() && value.len() <= 4096
                        && !value.chars().any(char::is_control)
                }) => Ok(WebDevelopmentAction::Prepare {
                    package: PathBuf::from(&args[4]),
                    python_runtime: PathBuf::from(&args[6]),
                }),
        Some("update-package") if args.len() == 5
            && args[3] == "--package"
            && !args[4].is_empty() && args[4].len() <= 4096
            && !args[4].chars().any(char::is_control) => Ok(WebDevelopmentAction::UpdatePackage {
                package: PathBuf::from(&args[4]),
            }),
        Some("status") if args.len() == 3 => Ok(WebDevelopmentAction::Status),
        Some("recover-package") if args.len() == 3 => Ok(WebDevelopmentAction::RecoverPackage),
        Some("record-verification") if args.len() == 5
            && args[3] == "--input-file"
            && !args[4].is_empty() && args[4].len() <= 4096
            && !args[4].chars().any(char::is_control) => Ok(WebDevelopmentAction::RecordVerification {
                input_file: PathBuf::from(&args[4]),
            }),
        Some("start") if args.len() == 3 => Ok(WebDevelopmentAction::Start),
        Some("stop") if args.len() == 3 => Ok(WebDevelopmentAction::Stop),
        Some("run") if args.len() == 3 => Ok(WebDevelopmentAction::Run),
        Some("run") if args.len() == 5 && args[3] == "--max-runtime-seconds" => {
            let maximum_seconds = args[4].parse::<u64>().ok()
                .filter(|seconds| (1..=900).contains(seconds))
                .ok_or("WEB_DEVELOPMENT_FOREGROUND_BUDGET_INVALID")?;
            Ok(WebDevelopmentAction::RunBounded { maximum_seconds })
        }
        _ => Err("WEB_DEVELOPMENT_ARGUMENTS_INVALID: use prepare --package <directory> --python-runtime <directory>, update-package --package <directory>, recover-package, record-verification --input-file <file>, login --project-id <id>, enqueue --project-id <id> --event-id <id> --message-file <file>, retry --project-id <id> --event-id dev-<id>, status, start, stop, or run".into()),
    }
}

fn login_web_development(workspace: &Path, project_id: &str) -> Result<serde_json::Value, String> {
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "DEV_WORKSPACE_UNAVAILABLE")?;
    let root = catdesk_wake::development_runtime::root_for_workspace(&workspace)?
        .ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    let target = mcp::operator_read_project_web_target(&workspace, project_id)
        .map_err(|_| "DEV_RUNTIME_PAIRED_TARGET_REQUIRED")?
        .ok_or("DEV_RUNTIME_PAIRED_TARGET_REQUIRED")?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        &workspace,
        &root,
        &target.url,
    )?;
    catdesk_wake::development_runtime::login(&workspace, &target.url)
}

/// Explicit controlled work for live development tests. It uses the same
/// paired target and durable queue as normal delivery, but never manufactures
/// a natural review record or a delivery receipt.
fn enqueue_web_development(
    workspace: &Path,
    project: &str,
    event: &str,
    message_file: &Path,
) -> Result<serde_json::Value, String> {
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "DEV_WORKSPACE_UNAVAILABLE")?;
    let root = catdesk_wake::development_runtime::root_for_workspace(&workspace)?
        .ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    let target = mcp::operator_read_project_web_target(&workspace, project)
        .map_err(|_| "DEV_RUNTIME_PAIRED_TARGET_REQUIRED")?
        .ok_or("DEV_RUNTIME_PAIRED_TARGET_REQUIRED")?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        &workspace,
        &root,
        &target.url,
    )?;
    let input = if message_file.is_absolute() {
        message_file.to_path_buf()
    } else {
        workspace.join(message_file)
    };
    let metadata = fs::symlink_metadata(&input).map_err(|_| "DEV_MESSAGE_UNAVAILABLE")?;
    if !metadata.is_file() || path_is_reparse_or_symlink(&metadata) || metadata.len() > 32 * 1024 {
        return Err("DEV_MESSAGE_INVALID".into());
    }
    let input = input
        .canonicalize()
        .map_err(|_| "DEV_MESSAGE_UNAVAILABLE")?;
    if !input.starts_with(&workspace) {
        return Err("DEV_MESSAGE_OUTSIDE_WORKSPACE".into());
    }
    if input.starts_with(workspace.join(".catdesk")) || input.starts_with(workspace.join(".git")) {
        return Err("DEV_MESSAGE_PRIVATE_STATE_REFUSED".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(&input)
        .map_err(|_| "DEV_MESSAGE_UNAVAILABLE")?
        .take(32 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "DEV_MESSAGE_UNAVAILABLE")?;
    if bytes.is_empty() || bytes.len() > 32 * 1024 {
        return Err("DEV_MESSAGE_INVALID".into());
    }
    let body = String::from_utf8(bytes).map_err(|_| "DEV_MESSAGE_INVALID")?;
    let body = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if body.is_empty() {
        return Err("DEV_MESSAGE_INVALID".into());
    }
    let message = format!(
        "DEVELOPMENT CONTROLLED WORK — project {project} — event {event}. {body} This is an isolated development event, not evidence of installed release or natural review acceptance."
    );
    if message.len() > 2048 {
        return Err("DEV_MESSAGE_TOO_LONG".into());
    }
    let store = catdesk_wake::store::Store::open(&root)?;
    let expected = catdesk_wake::protocol::Target::new(target.generation, target.url)?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        &workspace,
        store.root(),
        &expected.url,
    )?;
    let produced = store.produce_expected(event, project, "test", &message, &expected)?;
    Ok(
        json!({"mode":"DEVELOPMENT_UNPROTECTED","status":"QUEUED","eventId":produced.event_id,
        "projectId":project,"targetGeneration":expected.generation,"targetDigest":expected.digest,
        "deliveryProved":false,"naturalReviewProved":false}),
    )
}

fn retry_pre_submit_checked(
    store: &catdesk_wake::store::Store,
    project: &str,
    event: &str,
    expected: &catdesk_wake::protocol::Target,
) -> Result<(), String> {
    use catdesk_wake::store::Phase;
    if !catdesk_wake::protocol::valid_id(project)
        || !catdesk_wake::protocol::valid_id(event)
        || !event.starts_with("dev-")
    {
        return Err("DEV_RETRY_EVENT_INVALID".into());
    }
    if store.config()?.targets.get(project) != Some(expected) {
        return Err("DEV_RETRY_TARGET_MISMATCH".into());
    }
    let delivery = store.delivery(event)?.ok_or("DEV_RETRY_DELIVERY_MISSING")?;
    if delivery.event.event_id != event || delivery.event.project_id != project {
        return Err("DEV_RETRY_EVENT_PROJECT_MISMATCH".into());
    }
    let prefix = format!("DEVELOPMENT CONTROLLED WORK — project {project} — event {event}. ");
    if delivery.event.event_type != "test" || !delivery.event.message.starts_with(&prefix) {
        return Err("DEV_RETRY_NOT_CONTROLLED_EVENT".into());
    }
    if delivery.target != *expected || delivery.event.target_generation != expected.generation {
        return Err("DEV_RETRY_TARGET_MISMATCH".into());
    }
    if delivery.phase != Phase::Attention || delivery.receipt.is_some() {
        return Err("SUBMISSION_RETRY_REFUSED".into());
    }
    if !store
        .events()?
        .iter()
        .any(|queued| queued == &delivery.event)
    {
        return Err("DEV_RETRY_QUEUE_EVENT_MISSING".into());
    }
    // Wake repeats the phase, receipt and exact current-target checks under
    // its transaction lock. No event is cloned, retargeted or resubmitted here.
    store.retry_pre_submit(event)
}

fn retry_web_development(
    workspace: &Path,
    project: &str,
    event: &str,
) -> Result<serde_json::Value, String> {
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "DEV_WORKSPACE_UNAVAILABLE")?;
    let root = catdesk_wake::development_runtime::root_for_workspace(&workspace)?
        .ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    let target = mcp::operator_read_project_web_target(&workspace, project)
        .map_err(|_| "DEV_RUNTIME_PAIRED_TARGET_REQUIRED")?
        .ok_or("DEV_RUNTIME_PAIRED_TARGET_REQUIRED")?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        &workspace,
        &root,
        &target.url,
    )?;
    let store = catdesk_wake::store::Store::open(&root)?;
    let expected = catdesk_wake::protocol::Target::new(target.generation, target.url)?;
    if expected.digest != target.sha256 {
        return Err("DEV_RETRY_TARGET_MISMATCH".into());
    }
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        &workspace,
        store.root(),
        &expected.url,
    )?;
    retry_pre_submit_checked(&store, project, event, &expected)?;
    Ok(
        json!({"mode":"DEVELOPMENT_UNPROTECTED","status":"RETRY_COMMITTED",
        "projectId":project,"eventId":event,"targetGeneration":expected.generation,
        "targetDigest":expected.digest,"deliveryProved":false}),
    )
}

fn parse_release_pair_action(args: &[String]) -> Result<Option<OperatorAction>, String> {
    let action = match args.get(2).map(String::as_str) {
        Some("prepare")
            if args.len() == 9
                && args[3] == "--review-record-id"
                && args[5] == "--build-path"
                && args[7] == "--expected-sha256"
                && valid_reviewed_build_operator_value(&args[4])
                && args[6].len() <= 4096
                && args[8].len() == 64
                && args[8].bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            ReleasePairOperatorAction::Prepare {
                review_record_id: args[4].clone(),
                build_path: PathBuf::from(&args[6]),
                expected_sha256: args[8].to_ascii_lowercase(),
            }
        }
        Some("confirm")
            if args.len() == 5
                && args[3] == "--confirmation-token"
                && valid_reviewed_build_operator_value(&args[4]) =>
        {
            ReleasePairOperatorAction::Confirm {
                confirmation_token: args[4].clone(),
            }
        }
        Some("result") if args.len() == 3 => ReleasePairOperatorAction::Result,
        _ => return Err("operator release-pair accepts only fixed actions".into()),
    };
    Ok(Some(OperatorAction::ReleasePair(action)))
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
                "--model-id",
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
        (Some("thread-rotation"), Some("preflight")) => (
            "autonomy_project_thread_rotation_preflight",
            &["--project-id", "--workspace"],
        ),
        (Some("thread-rotation"), Some("confirm")) => (
            "autonomy_project_thread_rotation_confirm",
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
        "autonomy_project_thread_adoption_preflight"
        | "autonomy_project_thread_rotation_preflight" => {
            ["--project-id", "--workspace"].as_slice()
        }
        _ => ["--confirmation-token", "--candidate-handle"].as_slice(),
    };
    if !required.iter().all(|flag| values.contains_key(flag)) {
        return Err("operator project arguments are incomplete".into());
    }
    let arguments = json!({
        "projectId": values.get("--project-id"), "conversationUrl": values.get("--conversation-url"),
        "expectedCurrentTargetSha256": values.get("--expected-current-target-sha256"), "workspace": values.get("--workspace"),
        "gitIdentity": values.get("--git-identity"), "verificationProfile": values.get("--verification-profile"),
        "modelId": values.get("--model-id"),
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
    // A development control workspace cannot accidentally operate the
    // installed supervisor, release slots, or legacy global Wake controls.
    if !matches!(&action, OperatorAction::WebDevelopment(_))
        && catdesk_wake::development_runtime::root_for_workspace(workspace)?.is_some()
        && !matches!(
            &action,
            OperatorAction::Web(_)
                | OperatorAction::Codex(_)
                | OperatorAction::ProjectControl { .. }
        )
    {
        return Err("WEB_DEVELOPMENT_INSTALLED_OPERATION_UNAVAILABLE".into());
    }
    match action {
        OperatorAction::WebDevelopment(action) => {
            use catdesk_wake::development_runtime;
            match action {
                WebDevelopmentAction::Prepare {
                    package,
                    python_runtime,
                } => development_runtime::prepare(workspace, &package, &python_runtime),
                WebDevelopmentAction::UpdatePackage { package } => {
                    development_runtime::update_package(workspace, &package)
                }
                WebDevelopmentAction::RecoverPackage => {
                    development_runtime::recover_package(workspace)
                }
                WebDevelopmentAction::RecordVerification { input_file } => {
                    development_runtime::record_release_verification(workspace, &input_file)
                }
                WebDevelopmentAction::Enqueue {
                    project_id,
                    event_id,
                    message_file,
                } => enqueue_web_development(workspace, &project_id, &event_id, &message_file),
                WebDevelopmentAction::Retry {
                    project_id,
                    event_id,
                } => retry_web_development(workspace, &project_id, &event_id),
                WebDevelopmentAction::Login { project_id } => {
                    login_web_development(workspace, &project_id)
                }
                WebDevelopmentAction::Status => development_runtime::status(workspace),
                WebDevelopmentAction::Start => development_runtime::start(workspace),
                WebDevelopmentAction::Stop => development_runtime::stop(workspace),
                WebDevelopmentAction::Run => development_runtime::run_foreground(workspace),
                WebDevelopmentAction::RunBounded { maximum_seconds } => {
                    development_runtime::run_foreground_bounded(workspace, maximum_seconds)
                }
            }
        }
        OperatorAction::Web(action) => web::execute(action, workspace).await,
        OperatorAction::Codex(action) => {
            crate::operator_codex_cli::execute_action(action, workspace).await
        }
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
        OperatorAction::ReleasePair(action) => {
            let workspace = workspace
                .canonicalize()
                .map_err(|_| "operator workspace is unavailable".to_string())?;
            match action {
                ReleasePairOperatorAction::Prepare {
                    review_record_id,
                    build_path,
                    expected_sha256,
                } => {
                    let request = json!({"action":"PREFLIGHT","recordId":review_record_id,"buildPath":build_path,"expectedSha256":expected_sha256});
                    crate::delegated::autonomy_supervisor::handle_tool(
                        "catdesk_reviewed_build_promotion",
                        request,
                        &workspace,
                    )
                    .map_err(|_| "independent reviewed build preflight was rejected".to_string())?;
                    let preflight =
                        crate::daemon_reload::prepare_release_pair_reconcile(&workspace)?;
                    Ok(
                        json!({"action":"release_pair_prepare","state":"PREFLIGHT_READY","confirmationToken":preflight.confirmation_token,"expiresAtUnix":preflight.expires_at_unix,"candidateSha256":preflight.authorization.candidate_sha256,"tunnelAction":"none"}),
                    )
                }
                ReleasePairOperatorAction::Confirm { confirmation_token } => {
                    let result = crate::daemon_reload::confirm_release_pair_reconcile(
                        &workspace,
                        &confirmation_token,
                    )?;
                    Ok(
                        json!({"action":"release_pair_confirm","state":result.state,"candidateSha256":result.candidate_sha256,"tunnelAction":"none"}),
                    )
                }
                ReleasePairOperatorAction::Result => {
                    let result = crate::daemon_reload::release_pair_reconcile_result(&workspace)?;
                    let state = match result.as_ref() {
                        Some(result) => result.state.as_str(),
                        None if crate::daemon_reload::release_pair_reconcile_claimed(
                            &workspace,
                        )? =>
                        {
                            "CLAIMED_OUTCOME_UNPROVEN_OPERATOR_ATTENTION"
                        }
                        None => "RESULT_UNAVAILABLE",
                    };
                    Ok(
                        json!({"action":"release_pair_result","state":state,"candidateSha256":result.as_ref().map(|result| result.candidate_sha256.as_str()),"tunnelAction":"none"}),
                    )
                }
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
        "autonomy_project_thread_rotation_preflight" => {
            json!({"action":"project_thread_rotation_preflight","status":"success","confirmationToken":field("confirmationToken"),"expiresAtUnix":field("expiresAtUnix"),"previousThreadFingerprint":field("previousThreadFingerprint"),"candidates":field("candidates")})
        }
        "autonomy_project_thread_rotation_confirm" => {
            json!({"action":"project_thread_rotation_confirm","status":"success","projectId":field("projectId"),"model":field("model"),"reasoningEffort":field("reasoningEffort")})
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReleasePairPhase {
    Empty,
    Stage,
    Probe,
    Commit,
    Rollback,
}

/// The detached native worker supplies only fixed phase/identity values. Each
/// invocation finishes before a daemon is launched or released from its Job.
pub(crate) fn release_pair_invocation(
    workspace: &Path,
    phase: ReleasePairPhase,
    authorization_id: &str,
    transaction_id: &str,
    owner_pid: Option<u32>,
    owner_started_at_utc: Option<&str>,
) -> Result<RepairInvocation, String> {
    if authorization_id.len() != 32
        || transaction_id.len() != 32
        || !authorization_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || !transaction_id.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("release-pair worker identity is invalid".into());
    }
    if matches!(phase, ReleasePairPhase::Probe | ReleasePairPhase::Commit) != owner_pid.is_some()
        || owner_started_at_utc
            .is_some_and(|value| value.len() > 64 || value.bytes().any(|b| !b.is_ascii_graphic()))
        || (phase == ReleasePairPhase::Commit && owner_started_at_utc.is_none())
        || (matches!(
            phase,
            ReleasePairPhase::Empty | ReleasePairPhase::Stage | ReleasePairPhase::Rollback
        ) && (owner_pid.is_some() || owner_started_at_utc.is_some()))
    {
        return Err("release-pair phase arguments are invalid".into());
    }
    let workspace = workspace
        .canonicalize()
        .map(command::normalize_windows_verbatim_path)
        .map_err(|_| "release-pair workspace is unavailable".to_string())?;
    checked_regular_directory(&workspace)?;
    let script = workspace.join("scripts/reconcile-reviewed-catdesk-release-pair.ps1");
    let metadata = fs::symlink_metadata(&script)
        .map_err(|_| "fixed release-pair script is unavailable".to_string())?;
    if !metadata.is_file()
        || path_is_reparse_or_symlink(&metadata)
        || metadata.len() == 0
        || metadata.len() > MAX_REPAIR_SCRIPT_BYTES
    {
        return Err("fixed release-pair script is unsafe".into());
    }
    let script = script
        .canonicalize()
        .map(command::normalize_windows_verbatim_path)
        .map_err(|_| "fixed release-pair script is unavailable".to_string())?;
    if !script.starts_with(&workspace) {
        return Err("fixed release-pair script is unsafe".into());
    }
    let mut args = vec![
        "-NoLogo".into(),
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-File".into(),
        script.to_string_lossy().into_owned(),
        "-Workspace".into(),
        workspace.to_string_lossy().into_owned(),
        "-Phase".into(),
        match phase {
            ReleasePairPhase::Empty => "Empty",
            ReleasePairPhase::Stage => "Stage",
            ReleasePairPhase::Probe => "Probe",
            ReleasePairPhase::Commit => "Commit",
            ReleasePairPhase::Rollback => "Rollback",
        }
        .into(),
        "-AuthorizationId".into(),
        authorization_id.into(),
        "-TransactionId".into(),
        transaction_id.into(),
    ];
    if let Some(pid) = owner_pid {
        args.extend(["-OwnerPid".into(), pid.to_string()]);
    }
    if let Some(start) = owner_started_at_utc {
        args.extend(["-OwnerStartedAtUtc".into(), start.into()]);
    }
    Ok(RepairInvocation {
        program: trusted_windows_powershell(&workspace)?,
        args,
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

    #[test]
    fn web_development_requires_explicit_fixed_actions() {
        let parsed = parse_operator_action(
            [
                "operator",
                "web-dev-runtime",
                "prepare",
                "--package",
                "C:/dev/package",
                "--python-runtime",
                "C:/dev/python",
            ]
            .map(str::to_string),
        )
        .expect("parse prepare");
        assert!(matches!(
            parsed,
            Some(OperatorAction::WebDevelopment(
                WebDevelopmentAction::Prepare { .. }
            ))
        ));
        assert!(matches!(
            parse_operator_action(
                ["operator", "web-dev-runtime", "update-package", "--package", "C:/dev/new-package"]
                    .map(str::to_string)
            ),
            Ok(Some(OperatorAction::WebDevelopment(
                WebDevelopmentAction::UpdatePackage { package }
            ))) if package == Path::new("C:/dev/new-package")
        ));
        for invalid in [
            vec!["operator", "web-dev-runtime", "update-package"],
            vec![
                "operator",
                "web-dev-runtime",
                "update-package",
                "--package",
                "",
            ],
            vec![
                "operator",
                "web-dev-runtime",
                "update-package",
                "--package",
                "C:/dev/package",
                "--root",
                "C:/production",
            ],
            vec![
                "operator",
                "web-dev-runtime",
                "update-package",
                "--python-runtime",
                "C:/dev/python",
            ],
        ] {
            assert!(parse_operator_action(invalid.into_iter().map(str::to_string)).is_err());
        }
        assert!(matches!(
            parse_operator_action(
                ["operator", "web-dev-runtime", "recover-package"].map(str::to_string)
            ),
            Ok(Some(OperatorAction::WebDevelopment(
                WebDevelopmentAction::RecoverPackage
            )))
        ));
        for action in ["status", "start", "stop", "run", "recover-package"] {
            assert!(
                parse_operator_action(["operator", "web-dev-runtime", action].map(str::to_string))
                    .expect("fixed action")
                    .is_some()
            );
            assert!(
                parse_operator_action(
                    [
                        "operator",
                        "web-dev-runtime",
                        action,
                        "--root",
                        "C:/production"
                    ]
                    .map(str::to_string)
                )
                .is_err()
            );
        }
        for value in ["1", "900"] {
            assert!(matches!(
                parse_operator_action(
                    [
                        "operator",
                        "web-dev-runtime",
                        "run",
                        "--max-runtime-seconds",
                        value
                    ]
                    .map(str::to_string)
                )
                .unwrap(),
                Some(OperatorAction::WebDevelopment(
                    WebDevelopmentAction::RunBounded { .. }
                ))
            ));
        }
        for value in ["0", "901", "-1", "abc", "18446744073709551616"] {
            assert!(
                parse_operator_action(
                    [
                        "operator",
                        "web-dev-runtime",
                        "run",
                        "--max-runtime-seconds",
                        value
                    ]
                    .map(str::to_string)
                )
                .is_err()
            );
        }
        for action in ["install", "resume", "publish-reviewed-install"] {
            assert!(
                parse_operator_action(["operator", "web-dev-runtime", action].map(str::to_string))
                    .is_err()
            );
        }
        assert!(
            parse_operator_action(
                [
                    "operator",
                    "web-dev-runtime",
                    "prepare",
                    "--package",
                    "",
                    "--python-runtime",
                    "C:/python"
                ]
                .map(str::to_string)
            )
            .is_err()
        );
        assert!(matches!(
            parse_operator_action(
                ["operator", "web-dev-runtime", "login", "--project-id", "alpha"]
                    .map(str::to_string)
            ),
            Ok(Some(OperatorAction::WebDevelopment(
                WebDevelopmentAction::Login { project_id }
            ))) if project_id == "alpha"
        ));
        assert!(matches!(
            parse_operator_action(
                ["operator", "web-dev-runtime", "retry", "--project-id", "alpha", "--event-id", "dev-alpha-1"]
                    .map(str::to_string)
            ),
            Ok(Some(OperatorAction::WebDevelopment(
                WebDevelopmentAction::Retry { project_id, event_id }
            ))) if project_id == "alpha" && event_id == "dev-alpha-1"
        ));
        for invalid in [
            vec![
                "operator",
                "web-dev-runtime",
                "retry",
                "--project-id",
                "alpha",
            ],
            vec![
                "operator",
                "web-dev-runtime",
                "retry",
                "--project-id",
                "alpha",
                "--event-id",
                "ordinary-1",
            ],
            vec![
                "operator",
                "web-dev-runtime",
                "retry",
                "--project-id",
                "../alpha",
                "--event-id",
                "dev-alpha",
            ],
            vec![
                "operator",
                "web-dev-runtime",
                "retry",
                "--project-id",
                "alpha",
                "--event-id",
                "dev-one",
                "--event-id",
                "dev-two",
            ],
            vec![
                "operator",
                "web-dev-runtime",
                "retry",
                "--project-id",
                "alpha",
                "--conversation-url",
                "https://chatgpt.com/c/other",
            ],
        ] {
            assert!(parse_operator_action(invalid.into_iter().map(str::to_string)).is_err());
        }
        for invalid in [
            vec!["operator", "web-dev-runtime", "login"],
            vec![
                "operator",
                "web-dev-runtime",
                "login",
                "--project-id",
                "../alpha",
            ],
            vec![
                "operator",
                "web-dev-runtime",
                "login",
                "--conversation-url",
                "https://chatgpt.com/c/other",
            ],
            vec![
                "operator",
                "web-dev-runtime",
                "login",
                "--project-id",
                "alpha",
                "--root",
                "C:/installed",
            ],
            vec![
                "operator",
                "web-dev-runtime",
                "login",
                "--project-id",
                "alpha",
                "--project-id",
                "beta",
            ],
        ] {
            assert!(parse_operator_action(invalid.into_iter().map(str::to_string)).is_err());
        }
    }

    #[test]
    fn web_development_verification_requires_one_explicit_local_input() {
        assert_eq!(
            parse_operator_action(
                [
                    "operator",
                    "web-dev-runtime",
                    "record-verification",
                    "--input-file",
                    "evidence.json"
                ]
                .map(str::to_string)
            ),
            Ok(Some(OperatorAction::WebDevelopment(
                WebDevelopmentAction::RecordVerification {
                    input_file: PathBuf::from("evidence.json"),
                }
            )))
        );
        for invalid in [
            vec!["operator", "web-dev-runtime", "record-verification"],
            vec![
                "operator",
                "web-dev-runtime",
                "record-verification",
                "--input-file",
                "",
            ],
            vec![
                "operator",
                "web-dev-runtime",
                "record-verification",
                "--input-file",
                "bad\nfile",
            ],
            vec![
                "operator",
                "web-dev-runtime",
                "record-verification",
                "--workspace",
                "other",
            ],
            vec![
                "operator",
                "web-dev-runtime",
                "record-verification",
                "--input-file",
                "evidence.json",
                "--force",
            ],
        ] {
            assert!(parse_operator_action(invalid.into_iter().map(str::to_string)).is_err());
        }
    }

    #[test]
    fn web_development_login_requires_prepared_root_before_any_browser_action() {
        let workspace =
            std::env::temp_dir().join(format!("catdesk-login-unprepared-{}", Uuid::new_v4()));
        fs::create_dir(&workspace).unwrap();
        assert_eq!(
            login_web_development(&workspace, "alpha").unwrap_err(),
            "DEV_RUNTIME_NOT_PREPARED"
        );
        fs::remove_dir(workspace).unwrap();
    }

    #[test]
    fn web_development_retry_only_reclaims_exact_pre_submit_project_event() {
        use catdesk_wake::{
            protocol::Target,
            store::{Phase, Store},
        };
        let root = std::env::temp_dir().join(format!("catdesk-dev-retry-{}", Uuid::new_v4()));
        let store = Store::open(&root).unwrap();
        store.initialize().unwrap();
        let alpha = store
            .set_target("alpha", 0, "https://chatgpt.com/c/dev-alpha")
            .unwrap();
        let beta = store
            .set_target("beta", 0, "https://chatgpt.com/c/dev-beta")
            .unwrap();
        let lease = store.host_lock().unwrap();
        for (project, id) in [("alpha", "dev-alpha-1"), ("beta", "dev-beta-1")] {
            let target = if project == "alpha" { &alpha } else { &beta };
            let message = format!(
                "DEVELOPMENT CONTROLLED WORK — project {project} — event {id}. Isolated fixture."
            );
            let event = store
                .produce_expected(id, project, "test", &message, target)
                .unwrap();
            store.claim(&event, &lease).unwrap();
            store
                .transition(
                    id,
                    lease.owner(),
                    Phase::Attention,
                    None,
                    Some("READINESS_ATTENTION".into()),
                )
                .unwrap();
        }
        drop(lease);
        assert_eq!(
            retry_pre_submit_checked(&store, "alpha", "dev-beta-1", &alpha).unwrap_err(),
            "DEV_RETRY_EVENT_PROJECT_MISMATCH"
        );
        assert_eq!(
            retry_pre_submit_checked(
                &store,
                "alpha",
                "dev-alpha-1",
                &Target::new(1, "https://chatgpt.com/c/wrong".into()).unwrap()
            )
            .unwrap_err(),
            "DEV_RETRY_TARGET_MISMATCH"
        );
        assert_eq!(
            store.delivery("dev-alpha-1").unwrap().unwrap().phase,
            Phase::Attention
        );
        assert_eq!(
            store.delivery("dev-beta-1").unwrap().unwrap().phase,
            Phase::Attention
        );
        retry_pre_submit_checked(&store, "alpha", "dev-alpha-1", &alpha).unwrap();
        assert_eq!(
            store.delivery("dev-alpha-1").unwrap().unwrap().phase,
            Phase::Claimed
        );
        assert_eq!(
            store.delivery("dev-beta-1").unwrap().unwrap().phase,
            Phase::Attention
        );
        assert_eq!(
            retry_pre_submit_checked(&store, "alpha", "dev-alpha-1", &alpha).unwrap_err(),
            "SUBMISSION_RETRY_REFUSED"
        );
        assert_eq!(
            retry_pre_submit_checked(&store, "beta", "ordinary-1", &beta).unwrap_err(),
            "DEV_RETRY_EVENT_INVALID"
        );
        let canonical = root.canonicalize().unwrap();
        assert_eq!(
            canonical.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        assert!(
            canonical
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("catdesk-dev-retry-")
        );
        fs::remove_dir_all(canonical).unwrap();
    }

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
        assert!(matches!(
            parse_operator_action(
                [
                    "codex",
                    "status",
                    "--project-id",
                    "catdesk",
                    "--workspace",
                    "C:/work/catdesk",
                ]
                .map(str::to_string)
            ),
            Ok(Some(OperatorAction::Codex(
                crate::operator_codex_cli::OperatorCodexAction::Status(_)
            )))
        ));
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
        let rotation = parse_operator_action(
            [
                "operator",
                "project",
                "thread-rotation",
                "preflight",
                "--project-id",
                "catdesk",
                "--workspace",
                "C:/work/catdesk",
            ]
            .map(str::to_string),
        )
        .expect("parse rotation")
        .expect("rotation action");
        assert!(matches!(
            rotation,
            OperatorAction::ProjectControl {
                operation: "autonomy_project_thread_rotation_preflight",
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
