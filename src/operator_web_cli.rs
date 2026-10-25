//! Local operator controls for independently bound ChatGPT Web projects.
//! A paired target is routing authority, not a browser delivery receipt.

use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::delegated::autonomy_projects::AutonomousProjectRegistryStoreV1;
use crate::mcp::{self, DesignatedChatTargetErrorV1};
use crate::web_conversation_context::{
    ContextPressurePolicy, ConversationKey, Observation, VisibleSourceKind, WebContextStore,
};

#[path = "operator_web_auto.rs"]
mod auto;
#[path = "operator_web_observe.rs"]
mod observe;
#[path = "operator_web_retirement.rs"]
mod retirement;
#[path = "operator_web_review.rs"]
mod review;
#[path = "operator_web_rollover.rs"]
mod rollover;

pub(crate) fn start_auto_scheduler(state: crate::state::SharedState) {
    auto::start_scheduler(state);
}

pub(crate) fn start_scoped_auto_scheduler(
    state: crate::state::SharedState,
    projects: std::collections::BTreeSet<String>,
) {
    auto::start_scheduler_scoped(state, Some(projects));
}

pub(crate) async fn stop_auto_scheduler(state: crate::state::SharedState) {
    let tasks = {
        let mut app = state.lock().await;
        app.server_running = false;
        app.cancel_web_auto_scheduler()
    };
    join_auto_schedulers(tasks).await;
}

pub(crate) async fn join_auto_schedulers(tasks: Vec<tokio::task::JoinHandle<()>>) {
    for task in tasks {
        let _ = task.await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WebAction {
    Status,
    RegisterProject {
        project_id: String,
        workspace: PathBuf,
        git_identity: String,
    },
    RetireProject {
        project_id: String,
        retirement_id: String,
        expected_generation: u64,
        expected_digest: Option<String>,
    },
    ReadTarget {
        project_id: String,
    },
    BindTarget {
        project_id: String,
        conversation_url: String,
        expected_sha256: Option<String>,
        expected_generation: u64,
    },
    ContextStatus {
        project_id: String,
    },
    ContextCapture {
        project_id: String,
    },
    ReviewRead {
        project_id: String,
        event_id: String,
    },
    ReviewReobserve {
        project_id: String,
        event_id: String,
    },
    ReviewReconcile {
        project_id: String,
        event_id: String,
    },
    ReviewDiagnose {
        project_id: String,
        event_id: String,
    },
    ReviewAuthorization {
        project_id: String,
        operation: String,
        input_file: PathBuf,
    },
    WorkRequest {
        project_id: String,
        input_file: PathBuf,
    },
    WorkResolve {
        project_id: String,
        input_file: PathBuf,
    },
    WorkAssociate {
        project_id: String,
        input_file: PathBuf,
        assignment_file: PathBuf,
    },
    WorkResumePrepare {
        project_id: String,
        input_file: PathBuf,
    },
    WorkExecutionPermission {
        project_id: String,
        input_file: PathBuf,
        revoke: bool,
    },
    ContextConfigure {
        project_id: String,
        input_file: PathBuf,
    },
    ContextObserve {
        project_id: String,
        input_file: PathBuf,
    },
    RolloverStatus {
        project_id: String,
    },
    RolloverPrepare {
        project_id: String,
        handoff_file: PathBuf,
    },
    RolloverHandoffRequest {
        project_id: String,
        source_task_id: String,
    },
    RolloverHandoffCollect {
        project_id: String,
    },
    RolloverExecute {
        project_id: String,
    },
    RolloverInspectDraft {
        project_id: String,
        expected_intent_id: String,
    },
    RolloverInspectQuarantine {
        project_id: String,
        expected_intent_id: String,
    },
    RolloverArchiveDraft {
        project_id: String,
        expected_intent_id: String,
    },
    RolloverRepairDraft {
        project_id: String,
        expected_intent_id: String,
        expected_raw_digest: String,
    },
    RolloverQuarantine {
        project_id: String,
        expected_intent_id: String,
        observed_conversation_url: String,
    },
    RolloverDispositionPrepare {
        project_id: String,
        expected_intent_id: String,
        resolution_id: String,
        acknowledgement: String,
    },
    RolloverDispositionConfirm {
        project_id: String,
        expected_intent_id: String,
        resolution_id: String,
        acknowledgement: String,
        confirmation_token: String,
    },
    AutoTick {
        project_id: String,
    },
}

pub(crate) fn parse(args: &[String]) -> Result<WebAction, String> {
    if args.get(0).map(String::as_str) != Some("operator")
        || args.get(1).map(String::as_str) != Some("web")
    {
        return Err("WEB_USAGE: expected operator web".into());
    }
    if args.get(2).map(String::as_str) == Some("status") && args.len() == 3 {
        return Ok(WebAction::Status);
    }
    let group = args
        .get(2)
        .map(String::as_str)
        .ok_or("WEB_USAGE: missing action group")?;
    let verb = args
        .get(3)
        .map(String::as_str)
        .ok_or("WEB_USAGE: missing target action")?;
    let mut options = BTreeMap::new();
    let tail = &args[4..];
    if !tail.len().is_multiple_of(2) {
        return Err("WEB_USAGE: each option requires one value".into());
    }
    for pair in tail.chunks_exact(2) {
        if options.insert(pair[0].as_str(), pair[1].as_str()).is_some() {
            return Err("WEB_USAGE: duplicate option".into());
        }
    }
    let project_id = options
        .remove("--project-id")
        .ok_or("WEB_USAGE: --project-id is required")?;
    if project_id.is_empty()
        || project_id.len() > 128
        || !project_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err("WEB_USAGE: invalid project ID".into());
    }
    match (group, verb) {
        ("project", "retire") => {
            let retirement_id = options
                .remove("--retirement-id")
                .filter(|id| catdesk_wake::protocol::valid_id(id))
                .ok_or("WEB_USAGE: --retirement-id is required")?;
            let generation = options
                .remove("--expected-wake-generation")
                .ok_or("WEB_USAGE: --expected-wake-generation is required")?;
            let expected_generation = generation
                .parse::<u64>()
                .map_err(|_| "WEB_USAGE: invalid generation")?;
            let digest = options
                .remove("--expected-target-sha256")
                .ok_or("WEB_USAGE: --expected-target-sha256 is required")?;
            let expected_digest = if digest == "none" && expected_generation == 0 {
                None
            } else if expected_generation > 0
                && digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                Some(digest.to_owned())
            } else {
                return Err("WEB_USAGE: invalid retirement target witness".into());
            };
            if generation != expected_generation.to_string() || !options.is_empty() {
                return Err("WEB_USAGE: invalid retirement option".into());
            }
            Ok(WebAction::RetireProject {
                project_id: project_id.into(),
                retirement_id: retirement_id.into(),
                expected_generation,
                expected_digest,
            })
        }
        (
            "work",
            "request" | "resolve" | "associate" | "resume-prepare" | "execution-grant"
            | "execution-revoke",
        ) => {
            let input_file = options
                .remove("--input-file")
                .filter(|path| !path.is_empty())
                .ok_or("WEB_USAGE: --input-file is required")?;
            let assignment_file = if verb == "associate" {
                Some(
                    options
                        .remove("--assignment-file")
                        .filter(|path| !path.is_empty())
                        .ok_or("WEB_USAGE: --assignment-file is required")?,
                )
            } else {
                None
            };
            if !options.is_empty() {
                return Err("WEB_USAGE: unexpected work option".into());
            }
            if verb == "request" {
                Ok(WebAction::WorkRequest {
                    project_id: project_id.into(),
                    input_file: input_file.into(),
                })
            } else if verb == "resolve" {
                Ok(WebAction::WorkResolve {
                    project_id: project_id.into(),
                    input_file: input_file.into(),
                })
            } else if matches!(verb, "execution-grant" | "execution-revoke") {
                Ok(WebAction::WorkExecutionPermission {
                    project_id: project_id.into(),
                    input_file: input_file.into(),
                    revoke: verb == "execution-revoke",
                })
            } else if let Some(assignment_file) = assignment_file {
                Ok(WebAction::WorkAssociate {
                    project_id: project_id.into(),
                    input_file: input_file.into(),
                    assignment_file: assignment_file.into(),
                })
            } else {
                Ok(WebAction::WorkResumePrepare {
                    project_id: project_id.into(),
                    input_file: input_file.into(),
                })
            }
        }
        ("review", "authorize" | "revoke" | "preflight" | "run" | "resume") => {
            let input_file = options
                .remove("--input-file")
                .ok_or("WEB_USAGE: --input-file is required")?;
            if input_file.is_empty() || !options.is_empty() {
                return Err("WEB_USAGE: unexpected review authorization option".into());
            }
            Ok(WebAction::ReviewAuthorization {
                project_id: project_id.into(),
                operation: verb.into(),
                input_file: input_file.into(),
            })
        }
        ("project", "register") => {
            let candidate = options
                .remove("--workspace")
                .ok_or("WEB_USAGE: --workspace is required")?;
            let git_identity = options
                .remove("--git-identity")
                .ok_or("WEB_USAGE: --git-identity is required")?;
            if candidate.is_empty()
                || candidate.len() > 4096
                || candidate.chars().any(char::is_control)
                || git_identity.is_empty()
                || git_identity.len() > 1024
                || git_identity.chars().any(char::is_control)
                || !options.is_empty()
            {
                return Err("WEB_USAGE: invalid project registration option".into());
            }
            Ok(WebAction::RegisterProject {
                project_id: project_id.into(),
                workspace: PathBuf::from(candidate),
                git_identity: git_identity.into(),
            })
        }
        ("target", "read") if options.is_empty() => Ok(WebAction::ReadTarget {
            project_id: project_id.into(),
        }),
        ("target", "bind") => {
            let conversation_url = options
                .remove("--conversation-url")
                .ok_or("WEB_USAGE: --conversation-url is required")?;
            let expected = options
                .remove("--expected-current-target-sha256")
                .ok_or("WEB_USAGE: explicit current digest or none is required")?;
            let expected_sha256 = if expected == "none" {
                None
            } else if expected.len() == 64
                && expected
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                Some(expected.to_owned())
            } else {
                return Err("WEB_USAGE: expected digest must be lowercase SHA-256 or none".into());
            };
            let generation = options
                .remove("--expected-wake-generation")
                .ok_or("WEB_USAGE: --expected-wake-generation is required")?;
            let expected_generation = generation
                .parse::<u64>()
                .map_err(|_| "WEB_USAGE: invalid generation")?;
            if expected_generation.to_string() != generation || !options.is_empty() {
                return Err("WEB_USAGE: invalid generation or unexpected option".into());
            }
            Ok(WebAction::BindTarget {
                project_id: project_id.into(),
                conversation_url: conversation_url.into(),
                expected_sha256,
                expected_generation,
            })
        }
        ("context", "status") if options.is_empty() => Ok(WebAction::ContextStatus {
            project_id: project_id.into(),
        }),
        ("context", "capture") if options.is_empty() => Ok(WebAction::ContextCapture {
            project_id: project_id.into(),
        }),
        ("review", "read" | "reobserve" | "reconcile" | "diagnose") => {
            let event_id = options
                .remove("--event-id")
                .ok_or("WEB_USAGE: --event-id is required")?;
            if !catdesk_wake::protocol::valid_id(event_id) || !options.is_empty() {
                return Err("WEB_USAGE: invalid review event or unexpected option".into());
            }
            if verb == "diagnose" {
                Ok(WebAction::ReviewDiagnose {
                    project_id: project_id.into(),
                    event_id: event_id.into(),
                })
            } else if verb == "reconcile" {
                Ok(WebAction::ReviewReconcile {
                    project_id: project_id.into(),
                    event_id: event_id.into(),
                })
            } else if verb == "reobserve" {
                Ok(WebAction::ReviewReobserve {
                    project_id: project_id.into(),
                    event_id: event_id.into(),
                })
            } else {
                Ok(WebAction::ReviewRead {
                    project_id: project_id.into(),
                    event_id: event_id.into(),
                })
            }
        }
        ("context", "configure" | "observe") => {
            let input_file = PathBuf::from(
                options
                    .remove("--input-file")
                    .ok_or("WEB_USAGE: --input-file is required")?,
            );
            if !options.is_empty() {
                return Err("WEB_USAGE: unexpected option".into());
            }
            if verb == "configure" {
                Ok(WebAction::ContextConfigure {
                    project_id: project_id.into(),
                    input_file,
                })
            } else {
                Ok(WebAction::ContextObserve {
                    project_id: project_id.into(),
                    input_file,
                })
            }
        }
        ("rollover", "status") if options.is_empty() => Ok(WebAction::RolloverStatus {
            project_id: project_id.into(),
        }),
        ("rollover", "handoff-collect") if options.is_empty() => {
            Ok(WebAction::RolloverHandoffCollect {
                project_id: project_id.into(),
            })
        }
        ("rollover", "handoff-request") => {
            let source_task_id = options
                .remove("--source-task-id")
                .ok_or("WEB_USAGE: --source-task-id is required")?;
            if !catdesk_wake::protocol::valid_id(source_task_id) || !options.is_empty() {
                return Err("WEB_USAGE: invalid source task or unexpected option".into());
            }
            Ok(WebAction::RolloverHandoffRequest {
                project_id: project_id.into(),
                source_task_id: source_task_id.into(),
            })
        }
        ("rollover", "execute") if options.is_empty() => Ok(WebAction::RolloverExecute {
            project_id: project_id.into(),
        }),
        ("rollover", "repair-draft") => {
            let expected_intent_id = options
                .remove("--expected-intent-id")
                .ok_or("WEB_USAGE: --expected-intent-id is required")?;
            let expected_raw_digest = options
                .remove("--expected-raw-digest")
                .ok_or("WEB_USAGE: --expected-raw-digest is required")?;
            if !catdesk_wake::protocol::valid_id(expected_intent_id)
                || expected_raw_digest.len() != 64
                || !expected_raw_digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                || !options.is_empty()
            {
                return Err("WEB_USAGE: invalid draft repair witness or unexpected option".into());
            }
            Ok(WebAction::RolloverRepairDraft {
                project_id: project_id.into(),
                expected_intent_id: expected_intent_id.into(),
                expected_raw_digest: expected_raw_digest.into(),
            })
        }
        ("rollover", "inspect-draft" | "archive-draft" | "inspect-quarantine") => {
            let expected_intent_id = options
                .remove("--expected-intent-id")
                .ok_or("WEB_USAGE: --expected-intent-id is required")?;
            if !catdesk_wake::protocol::valid_id(expected_intent_id) || !options.is_empty() {
                return Err("WEB_USAGE: invalid draft intent or unexpected option".into());
            }
            if verb == "inspect-quarantine" {
                Ok(WebAction::RolloverInspectQuarantine {
                    project_id: project_id.into(),
                    expected_intent_id: expected_intent_id.into(),
                })
            } else if verb == "inspect-draft" {
                Ok(WebAction::RolloverInspectDraft {
                    project_id: project_id.into(),
                    expected_intent_id: expected_intent_id.into(),
                })
            } else {
                Ok(WebAction::RolloverArchiveDraft {
                    project_id: project_id.into(),
                    expected_intent_id: expected_intent_id.into(),
                })
            }
        }
        ("rollover", "disposition-prepare" | "disposition-confirm") => {
            let intent = options
                .remove("--expected-intent-id")
                .ok_or("WEB_USAGE: --expected-intent-id is required")?;
            let resolution = options
                .remove("--resolution-id")
                .ok_or("WEB_USAGE: --resolution-id is required")?;
            let acknowledgement = options
                .remove("--acknowledge-unknown")
                .ok_or("WEB_USAGE: --acknowledge-unknown is required")?;
            let token = if verb == "disposition-confirm" {
                Some(
                    options
                        .remove("--confirmation-token")
                        .ok_or("WEB_USAGE: --confirmation-token is required")?,
                )
            } else {
                None
            };
            if !catdesk_wake::protocol::valid_id(intent)
                || !catdesk_wake::protocol::valid_id(resolution)
                || acknowledgement != "ABANDONED_UNKNOWN_NO_REPLAY"
                || token.is_some_and(|value| !catdesk_wake::protocol::valid_id(value))
                || !options.is_empty()
            {
                return Err(
                    "WEB_USAGE: exact quarantine disposition acknowledgement and identity required"
                        .into(),
                );
            }
            if let Some(token) = token {
                Ok(WebAction::RolloverDispositionConfirm {
                    project_id: project_id.into(),
                    expected_intent_id: intent.into(),
                    resolution_id: resolution.into(),
                    acknowledgement: acknowledgement.into(),
                    confirmation_token: token.into(),
                })
            } else {
                Ok(WebAction::RolloverDispositionPrepare {
                    project_id: project_id.into(),
                    expected_intent_id: intent.into(),
                    resolution_id: resolution.into(),
                    acknowledgement: acknowledgement.into(),
                })
            }
        }
        ("rollover", "quarantine") => {
            let expected_intent_id = options
                .remove("--expected-intent-id")
                .ok_or("WEB_USAGE: --expected-intent-id is required")?;
            let observed_conversation_url = options
                .remove("--observed-conversation-url")
                .ok_or("WEB_USAGE: --observed-conversation-url is required")?;
            if !catdesk_wake::protocol::valid_id(expected_intent_id)
                || catdesk_wake::protocol::Target::new(1, observed_conversation_url.into()).is_err()
                || !options.is_empty()
            {
                return Err("WEB_USAGE: invalid quarantine witness or unexpected option".into());
            }
            Ok(WebAction::RolloverQuarantine {
                project_id: project_id.into(),
                expected_intent_id: expected_intent_id.into(),
                observed_conversation_url: observed_conversation_url.into(),
            })
        }
        ("auto", "tick") if options.is_empty() => Ok(WebAction::AutoTick {
            project_id: project_id.into(),
        }),
        ("rollover", "prepare") => {
            let handoff_file = PathBuf::from(
                options
                    .remove("--handoff-file")
                    .ok_or("WEB_USAGE: --handoff-file is required")?,
            );
            if !options.is_empty() {
                return Err("WEB_USAGE: unexpected option".into());
            }
            Ok(WebAction::RolloverPrepare {
                project_id: project_id.into(),
                handoff_file,
            })
        }
        _ => Err("WEB_USAGE: invalid target action or options".into()),
    }
}

fn reason(error: DesignatedChatTargetErrorV1) -> &'static str {
    match error {
        DesignatedChatTargetErrorV1::InvalidUrl => "INVALID_CONVERSATION_URL",
        DesignatedChatTargetErrorV1::Stale => "STALE_TARGET_WITNESS",
        DesignatedChatTargetErrorV1::SourceProgress => "SOURCE_PROGRESS_AFTER_HANDOFF",
        DesignatedChatTargetErrorV1::ProtectedStateMismatch => "REGISTRY_WAKE_MISMATCH",
        DesignatedChatTargetErrorV1::SynchronizationFailure => "TARGET_SYNCHRONIZATION_REQUIRED",
        DesignatedChatTargetErrorV1::Unavailable => "TARGET_AUTHORITY_UNAVAILABLE",
    }
}

fn read_input<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "WEB_INPUT_UNAVAILABLE")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 512 * 1024 {
        return Err("WEB_INPUT_INVALID".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("WEB_INPUT_INVALID".into());
        }
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "WEB_INPUT_UNAVAILABLE")?
        .take(512 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WEB_INPUT_UNAVAILABLE")?;
    if bytes.len() > 512 * 1024 {
        return Err("WEB_INPUT_TOO_LARGE".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "WEB_INPUT_INVALID".into())
}

fn read_work_input(path: &Path, project_id: &str) -> Result<Value, String> {
    let input: Value = read_input(path)?;
    if input.get("projectId").and_then(Value::as_str) != Some(project_id) {
        return Err("WEB_WORK_PROJECT_SCOPE_MISMATCH".into());
    }
    Ok(input)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextInput {
    schema_version: u32,
    key: ConversationKey,
    events: Vec<ContextEvent>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextEvent {
    event_id: String,
    revision: u64,
    dedup_key: String,
    source_kind: VisibleSourceKind,
    visible_text: Option<String>,
    observed_at_unix: u64,
}

impl ContextEvent {
    fn observation(&self) -> Observation<'_> {
        Observation {
            event_id: &self.event_id,
            revision: self.revision,
            dedup_key: &self.dedup_key,
            source_kind: self.source_kind,
            visible_text: self.visible_text.as_deref(),
            observed_at_unix: self.observed_at_unix,
        }
    }
}

fn paired_key(workspace: &Path, project: &str) -> Result<ConversationKey, String> {
    let target = mcp::operator_read_project_web_target_diagnosed(workspace, project)
        .map_err(|failure| failure.code.to_string())?
        .ok_or("WEB_TARGET_UNBOUND")?;
    Ok(ConversationKey {
        project_id: project.into(),
        conversation_url: target.url,
        target_generation: target.generation,
    })
}

fn context_status(workspace: &Path, project: &str) -> Value {
    let root = workspace.join(".catdesk/web-context");
    if !root.exists() {
        return json!({"status":"UNOBSERVED","advisory":true});
    }
    match WebContextStore::open_read_only(root).and_then(|store| store.project_history(project)) {
        Ok(history) => json!({"status":"OBSERVED","conversations":history,"advisory":true}),
        Err(_) => json!({"status":"UNOBSERVED_OR_UNAVAILABLE","advisory":true}),
    }
}

fn capture_context_guarded(
    workspace: &Path,
    project: &str,
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<Value, String> {
    capture_context_with_snapshot(
        workspace,
        project,
        None,
        keep_running,
        catdesk_wake::runtime::snapshot_conversation_guarded,
    )
}

pub(super) fn capture_context_expected_guarded(
    workspace: &Path,
    project: &str,
    expected: (u64, &str),
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<Value, String> {
    capture_context_with_snapshot(
        workspace,
        project,
        Some(expected),
        keep_running,
        catdesk_wake::runtime::snapshot_conversation_guarded,
    )
}

pub(super) fn capture_context_with_snapshot<F>(
    workspace: &Path,
    project: &str,
    expected: Option<(u64, &str)>,
    keep_running: &dyn Fn() -> Result<(), String>,
    snapshot: F,
) -> Result<Value, String>
where
    F: FnOnce(
        &catdesk_wake::store::Store,
        &str,
        &catdesk_wake::protocol::Target,
        &dyn Fn() -> Result<(), String>,
    ) -> Result<Value, String>,
{
    keep_running()?;
    let key = paired_key(workspace, project)?;
    if expected.is_some_and(|(generation, digest)| {
        key.target_generation != generation || key.target_sha256() != digest
    }) {
        return Err("WEB_CONTEXT_TARGET_CHANGED".into());
    }
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let wake = catdesk_wake::store::Store::open(&root)?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &key.conversation_url,
    )?;
    let source =
        catdesk_wake::protocol::Target::new(key.target_generation, key.conversation_url.clone())?;
    keep_running()?;
    let snapshot = snapshot(&wake, project, &source, keep_running)?;
    keep_running()?;
    if paired_key(workspace, project)? != key {
        return Err("WEB_CONTEXT_TARGET_CHANGED".into());
    }
    let store = WebContextStore::open_nonblocking(workspace.join(".catdesk/web-context"))?;
    let previous = store.active_key(project)?;
    store.bind_observed_target(key.clone(), previous.as_ref())?;
    observe::ingest(&store, &key, snapshot, catdesk_wake::store::now(), || {
        keep_running()?;
        if paired_key(workspace, project)? == key {
            Ok(())
        } else {
            Err("WEB_CONTEXT_TARGET_CHANGED".into())
        }
    })
}

fn review_text(snapshot: &Value, expected_message_digest: &str) -> Result<String, String> {
    let messages = snapshot["messages"]
        .as_array()
        .ok_or("WEB_REVIEW_SNAPSHOT_INVALID")?;
    let user_index = messages
        .iter()
        .rposition(|message| message["role"] == "USER")
        .ok_or("WEB_REVIEW_USER_WITNESS_REQUIRED")?;
    let user = &messages[user_index];
    let user_text = user["text"]
        .as_str()
        .ok_or("WEB_REVIEW_USER_WITNESS_REQUIRED")?;
    let normalized = user_text.split_whitespace().collect::<Vec<_>>().join(" ");
    if user["textComplete"] != true
        || catdesk_wake::protocol::digest(&normalized) != expected_message_digest
    {
        return Err("WEB_REVIEW_EVENT_NOT_LATEST_OR_INCOMPLETE".into());
    }
    if user_index + 2 != messages.len() {
        return Err("WEB_REVIEW_RESPONSE_NOT_COMPLETE".into());
    }
    let assistant = &messages[user_index + 1];
    let text = assistant["text"]
        .as_str()
        .ok_or("WEB_REVIEW_RESPONSE_NOT_COMPLETE")?;
    if assistant["role"] != "ASSISTANT"
        || assistant["textComplete"] != true
        || text.trim().is_empty()
        || text.len() > 8192
        || text.to_ascii_lowercase().contains("response couldn’t load")
        || text.to_ascii_lowercase().contains("response couldn't load")
    {
        return Err("WEB_REVIEW_RESPONSE_NOT_COMPLETE".into());
    }
    Ok(text.to_owned())
}

/// Explicit local read only; private response text is never a status field.
fn read_review(workspace: &Path, project: &str, event_id: &str) -> Result<Value, String> {
    read_review_guarded(workspace, project, event_id, None, &|| Ok(()))
}

pub(super) fn read_review_guarded(
    workspace: &Path,
    project: &str,
    event_id: &str,
    expected: Option<(u64, &str)>,
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<Value, String> {
    read_review_using(
        workspace,
        project,
        event_id,
        expected,
        keep_running,
        catdesk_wake::runtime::diagnosed_sent_snapshot,
    )
}

fn read_review_using<F>(
    workspace: &Path,
    project: &str,
    event_id: &str,
    expected: Option<(u64, &str)>,
    keep_running: &dyn Fn() -> Result<(), String>,
    snapshotter: F,
) -> Result<Value, String>
where
    F: FnOnce(
        &catdesk_wake::store::Store,
        &str,
        &catdesk_wake::protocol::Target,
        &str,
        &dyn Fn() -> Result<(), String>,
    ) -> catdesk_wake::runtime::SnapshotReadReport,
{
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    let keep_running = || {
        keep_running()?;
        if std::time::Instant::now() >= deadline {
            return Err("WEB_REVIEW_READ_TIMEOUT".into());
        }
        Ok(())
    };
    keep_running()?;
    let key = paired_key(workspace, project)?;
    if expected.is_some_and(|(generation, digest)| {
        key.target_generation != generation || key.target_sha256() != digest
    }) {
        return Err("WEB_REVIEW_TARGET_CHANGED".into());
    }
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let wake = catdesk_wake::store::Store::open(&root)?;
    let target =
        catdesk_wake::protocol::Target::new(key.target_generation, key.conversation_url.clone())?;
    let delivery = wake
        .delivery(event_id)?
        .ok_or("WEB_REVIEW_DELIVERY_REQUIRED")?;
    let receipt = delivery
        .receipt
        .as_ref()
        .ok_or("WEB_REVIEW_DELIVERY_REQUIRED")?;
    if delivery.phase != catdesk_wake::store::Phase::Sent
        || delivery.event.event_id != event_id
        || delivery.event.project_id != project
        || delivery.event.target_generation != target.generation
        || delivery.target != target
        || receipt.event_id != event_id
        || receipt.target_digest != target.digest
        || receipt.target_generation != target.generation
        || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
        || receipt.message_digest != delivery.message_digest
        || receipt.message_digest != catdesk_wake::protocol::digest(&delivery.event.message)
    {
        return Err("WEB_REVIEW_DELIVERY_SCOPE_CHANGED".into());
    }
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &target.url,
    )?;
    keep_running()?;
    let report = snapshotter(&wake, project, &target, event_id, &keep_running);
    if paired_key(workspace, project)? != key {
        return Err("WEB_REVIEW_TARGET_CHANGED".into());
    }
    if let Some(reason) = report.failure {
        return Ok(json!({"action":"web_review_read", "projectId":project,
            "eventId":event_id,"target":target,"readOnly":true,"eventVerified":false,
            "status":"UNVERIFIED_EVENT_SNAPSHOT","reason":reason,
            "diagnostic":report.diagnostic,"sendAttempted":false}));
    }
    keep_running()?;
    let snapshot = report.snapshot.ok_or("WEB_REVIEW_SNAPSHOT_INVALID")?;
    match review_text(&snapshot, &receipt.message_digest) {
        Ok(text) => Ok(
            json!({"action":"web_review_read", "projectId":project,"eventId":event_id,
            "target":target,"readOnly":true,"response":text,"eventVerified":true,
            "diagnostic":report.diagnostic,"sendAttempted":false}),
        ),
        // Explicit local reads may expose this bounded private excerpt for
        // diagnosis. It is never a status endpoint or verified event response.
        Err(reason) => Ok(json!({"action":"web_review_read", "projectId":project,
            "eventId":event_id,"target":target,"readOnly":true,"eventVerified":false,
            "status":"UNVERIFIED_EVENT_SNAPSHOT","reason":reason,
            "snapshot":snapshot,"diagnostic":report.diagnostic,"sendAttempted":false})),
    }
}

fn target_status(workspace: &Path, project_id: &str, runtime_mode: &str) -> Value {
    match mcp::operator_read_project_web_target_status(workspace, project_id) {
        Ok(Some(target)) => json!({"status":"PAIRED", "target":target, "deliveryProved":false,
            "runtimeMode":runtime_mode}),
        Ok(None) => json!({"status":"UNBOUND", "deliveryProved":false,
            "runtimeMode":runtime_mode}),
        Err(error) => json!({"status":"ATTENTION", "reason":reason(error), "deliveryProved":false,
            "runtimeMode":runtime_mode}),
    }
}

fn target_status_diagnosed(workspace: &Path, project_id: &str, runtime_mode: &str) -> Value {
    match mcp::operator_read_project_web_target_diagnosed(workspace, project_id) {
        Ok(Some(target)) => json!({"status":"PAIRED", "target":target, "deliveryProved":false,
            "runtimeMode":runtime_mode}),
        Ok(None) => json!({"status":"UNBOUND", "deliveryProved":false, "runtimeMode":runtime_mode}),
        Err(failure) => json!({"status":"ATTENTION", "reason":reason(failure.error),
            "diagnosticCode":failure.code, "deliveryProved":false, "runtimeMode":runtime_mode}),
    }
}

/// The central registry read uses the operator's validated rollover reader,
/// then projects only durable progress codes. A missing project state must not
/// create directories or a lock file merely because the registry was read.
pub(crate) fn registry_rollover_status(workspace: &Path, project: &str) -> Value {
    match rollover_status_read_only(workspace, project) {
        Ok(Some(raw)) => registry_rollover_projection(&raw),
        Ok(None) => json!({"status":"UNOBSERVED","phase":"NONE",
            "automation":{"status":"UNOBSERVED"}}),
        Err(_) => json!({"status":"UNAVAILABLE_OR_BUSY","reason":"STATE_UNAVAILABLE"}),
    }
}

pub(crate) use rollover::WorkLease as WebWorkLease;

pub(crate) fn acquire_web_work_lease(
    workspace: &Path,
    project: &str,
) -> Result<WebWorkLease, String> {
    rollover::acquire_web_work_lease(workspace, project)
}

fn rollover_status_read_only(workspace: &Path, project: &str) -> Result<Option<Value>, String> {
    if !catdesk_wake::protocol::valid_id(project) {
        return Err("WEB_PROJECT_INVALID".into());
    }
    let root = workspace.join(".catdesk/web-rollovers").join(project);
    match std::fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("WEB_ROLLOVER_STATE_UNAVAILABLE".into()),
        Ok(_) => {}
    }
    rollover::status_read_only(workspace, project).map(Some)
}

fn operator_rollover_status(workspace: &Path, project: &str) -> Result<Value, String> {
    Ok(rollover_status_read_only(workspace, project)?
        .unwrap_or_else(|| json!({"projectId":project,"phase":"NONE","automation":null})))
}

fn registry_rollover_projection(raw: &Value) -> Value {
    let Some(phase) = raw.get("phase").and_then(Value::as_str) else {
        return json!({"status":"UNAVAILABLE_OR_BUSY","reason":"STATE_INVALID"});
    };
    if !matches!(
        phase,
        "NONE"
            | "PREPARED"
            | "SUBMITTING"
            | "RESPONSE_PENDING"
            | "CREATED"
            | "BOUND"
            | "CONTINUATION_QUEUED"
            | "QUARANTINED"
    ) {
        return json!({"status":"UNAVAILABLE_OR_BUSY","reason":"STATE_INVALID"});
    }
    let automation = match raw.get("automation") {
        Some(Value::Object(status)) => {
            let stage = status.get("stage").and_then(Value::as_str).unwrap_or("");
            let stage = if matches!(
                stage,
                "OBSERVATION_DISABLED"
                    | "IN_PROGRESS"
                    | "OBSERVED"
                    | "WAKE_STOPPED"
                    | "WAKE_PAUSED"
                    | "WAKE_HOST_NOT_RUNNING"
                    | "WAKE_TARGET_CHANGED"
                    | "AWAITING_BROWSER"
                    | "AWAITING_CONTEXT"
                    | "AWAITING_TARGET"
                    | "AWAITING_QUIESCENCE"
                    | "AWAITING_RESPONSE"
                    | "AWAITING_HANDOFF"
                    | "TASK_ANCHOR_REQUIRED"
                    | "HANDOFF_REQUESTED"
                    | "SOURCE_WITNESS_REQUIRED"
                    | "SUBMISSION_UNKNOWN"
                    | "QUARANTINED"
                    | "ROLLOVER_ADVANCED"
                    | "ATTENTION"
            ) {
                stage
            } else {
                "UNKNOWN_STAGE"
            };
            let reason = status
                .get("reason")
                .and_then(Value::as_str)
                .filter(|reason| {
                    !reason.is_empty()
                        && reason.len() <= 128
                        && reason
                            .bytes()
                            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                });
            let last_failure = status
                .get("lastFailure")
                .and_then(Value::as_object)
                .and_then(|failure| {
                    let reason = failure.get("reason")?.as_str()?;
                    let tick_utc = failure.get("tickUtc")?.as_u64().filter(|tick| *tick > 0)?;
                    if reason.is_empty()
                        || reason.len() > 128
                        || !reason
                            .bytes()
                            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                    {
                        return None;
                    }
                    Some(json!({"reason":reason,"tickUtc":tick_utc,
                        "targetGeneration":failure.get("targetGeneration").and_then(Value::as_u64),
                        "requiredAction":null}))
                });
            json!({"status":"RECORDED","stage":stage,"reason":reason,
                "tickUtc":status.get("tickUtc").and_then(Value::as_u64),
                "targetGeneration":status.get("targetGeneration").and_then(Value::as_u64),
                "lastFailure":last_failure})
        }
        Some(Value::Null) | None => json!({"status":"UNOBSERVED"}),
        _ => json!({"status":"UNAVAILABLE"}),
    };
    let meter = raw
        .get("contextMeterStatus")
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "PENDING" | "RECORDED" | "RETRY_REQUIRED"));
    let bootstrap_progress = raw
        .get("bootstrapProgress")
        .and_then(Value::as_str)
        .filter(|value| {
            matches!(
                *value,
                "TEXT_ENTRY_AUTHORIZED" | "DRAFT_READY" | "SEND_ATTEMPTED"
            )
        });
    let source_target = raw.get("source");
    let bound_target = raw.get("boundTarget").filter(|target| target.is_object());
    let request_target = raw
        .get("handoffRequest")
        .and_then(|request| request.get("source"));
    let observed_target = if matches!(phase, "BOUND" | "CONTINUATION_QUEUED") {
        bound_target.or(source_target)
    } else {
        source_target.or(request_target)
    };
    let target_generation = observed_target
        .and_then(|target| target.get("generation"))
        .and_then(Value::as_u64);
    let target_digest = observed_target
        .and_then(|target| target.get("digest"))
        .and_then(Value::as_str)
        .filter(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        });
    let continuation_proof = raw
        .get("continuationProofStatus")
        .and_then(Value::as_str)
        .filter(|status| {
            matches!(
                *status,
                "NOT_CHECKED" | "VERIFIED" | "PENDING_OR_UNVERIFIED" | "UNAVAILABLE_OR_CHANGED"
            )
        })
        .unwrap_or("NOT_CHECKED");
    let creation_admission = raw.get("creationAdmission").map(|admission| {
        let phase = admission
            .get("phase")
            .and_then(Value::as_str)
            .filter(|phase| {
                matches!(
                    *phase,
                    "WAITING" | "ACTIVE" | "CREATION_RECORDED" | "QUARANTINED"
                )
            });
        match (admission.get("status").and_then(Value::as_str), phase) {
            (Some("DURABLE_SNAPSHOT"), Some(phase)) => json!({"status":"DURABLE_SNAPSHOT",
                "phase":phase,"waitingAhead":admission.get("waitingAhead").and_then(Value::as_u64),
                "liveOwnerVerified":false,"creationProof":false}),
            (Some("UNOBSERVED"), _) => json!({"status":"UNOBSERVED","liveOwnerVerified":false}),
            _ => json!({"status":"UNAVAILABLE","liveOwnerVerified":false}),
        }
    });
    let mut projection = json!({
        "status":if phase == "NONE" && raw.get("handoffRequest").is_none() {"NO_ROLLOVER"} else {"DURABLE_SNAPSHOT"},
        "phase":phase,
        "targetGeneration":target_generation,
        "targetDigest":target_digest,
        "reconciliationRequired":raw.get("reconciliationRequired").and_then(Value::as_bool).unwrap_or(false),
        "contextMeterStatus":meter,
        "bootstrapProgress":bootstrap_progress,
        "handoffRequested":raw.get("handoffRequest").is_some_and(Value::is_object),
        "handoffCollected":raw.get("handoffRequest").and_then(|request| request.get("handoffCollected")).and_then(Value::as_bool),
        "continuationProved":phase == "CONTINUATION_QUEUED" && continuation_proof == "VERIFIED" && raw.get("continuationProved") == Some(&Value::Bool(true)),
        "continuationProofStatus":continuation_proof,
        "automation":automation,
        "creationAdmission":creation_admission,
    });
    if let Some(disposition) = raw.get("operatorDisposition") {
        projection["operatorDisposition"] = if disposition.get("status").and_then(Value::as_str)
            == Some("ABANDONED_UNKNOWN_NO_REPLAY")
            && disposition
                .get("resolutionId")
                .and_then(Value::as_str)
                .is_some_and(catdesk_wake::protocol::valid_id)
            && disposition.get("providerOutcome").and_then(Value::as_str) == Some("UNKNOWN")
            && disposition
                .get("complete")
                .and_then(Value::as_bool)
                .is_some()
        {
            json!({"status":"ABANDONED_UNKNOWN_NO_REPLAY","resolutionId":disposition["resolutionId"],
                "complete":disposition["complete"],"providerOutcome":"UNKNOWN","replayAllowed":false})
        } else {
            json!({"status":"UNAVAILABLE_OR_CHANGED","replayAllowed":false})
        };
    }
    projection
}

pub(crate) fn read_status(workspace: &Path) -> Result<Value, String> {
    read_status_projection(workspace, true)
}

pub(crate) fn read_summary(workspace: &Path) -> Result<Value, String> {
    read_status_projection(workspace, false)
}

fn read_status_projection(workspace: &Path, include_legacy_details: bool) -> Result<Value, String> {
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "WEB_SCOPE: control workspace is unavailable")?;
    let runtime_mode = crate::wake_protocol_client::wake_runtime_mode_for_workspace(&workspace)?;
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
            .and_then(|store| store.load_registry())
            .map_err(|_| "WEB_SCOPE: control registry is unavailable")?;
    let web_projects =
        mcp::web_project_registry_status(&workspace, &json!({"projects":&registry.projects}));
    let retired_projects: Vec<_> = registry
        .retired_projects
        .iter()
        .map(|retired| {
            json!({
                "projectId":retired.project.project_id,
                "retirementId":retired.retirement_id,
                "retiredUtc":retired.retired_utc,
                "workspacePreserved":true,
                "historyPreserved":true
            })
        })
        .collect();
    let mut report = json!({"action":"web_status", "readOnly":true,
        "runtimeMode":runtime_mode, "webProjects":web_projects,"retiredProjects":retired_projects,
        "meaning":"Per-project registration and paired Wake readback. Browser authentication, delivery and continuation require their own receipts."});
    if !include_legacy_details {
        return Ok(report);
    }
    let projects: Vec<_> = registry
        .projects
        .iter()
        .map(|project| {
            let stored = serde_json::to_value(project).unwrap_or(Value::Null);
            json!({
                "projectId":project.project_id,
                "workspace":project.workspace,
                "nativeThreadId":project.codex_thread_id,
                "registeredConversationUrl":project.chatgpt_target_url,
                "conversationHistory":stored.get("chatgptTargetHistory"),
                "wake":target_status(&workspace, &project.project_id, runtime_mode),
                "context":context_status(&workspace,&project.project_id),
                "rollover":operator_rollover_status(&workspace,&project.project_id)
                    .unwrap_or_else(|_| json!({"status":"UNAVAILABLE_OR_BUSY"})),
            })
        })
        .collect();
    report["projects"] = json!(projects);
    Ok(report)
}

pub(crate) async fn execute(action: WebAction, workspace: &Path) -> Result<Value, String> {
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "WEB_SCOPE: control workspace is unavailable")?;
    match action {
        WebAction::RegisterProject {
            project_id,
            workspace: project_workspace,
            git_identity,
        } => {
            let runtime_mode =
                crate::wake_protocol_client::wake_runtime_mode_for_workspace(&workspace)?;
            let registry =
                AutonomousProjectRegistryStoreV1::open(workspace.join(".catdesk/projects"))
                    .map_err(|_| "WEB_PROJECT_REGISTRY_UNAVAILABLE")?;
            let project = registry
                .register_web_only_project(
                    &workspace,
                    &project_id,
                    &project_workspace,
                    &git_identity,
                )
                .map_err(|_| "WEB_PROJECT_REGISTER_REFUSED")?;
            Ok(json!({
                "action":"web_project_register", "status":"REGISTERED_WEB_ONLY",
                "projectId":project.project_id, "workspace":project.workspace,
                "gitIdentity":project.git_identity,
                "verificationProfile":project.verification_profile,
                "nativeThreadId":project.codex_thread_id,
                "targetUnbound":true, "runtimeMode":runtime_mode,
                "nativeSessionAction":"NONE", "wakeAction":"NONE",
            }))
        }
        WebAction::Status => read_status(&workspace),
        WebAction::RetireProject {
            project_id,
            retirement_id,
            expected_generation,
            expected_digest,
        } => retirement::retire(
            &workspace,
            &project_id,
            &retirement_id,
            expected_generation,
            expected_digest.as_deref(),
        ),
        WebAction::ReadTarget { project_id } => {
            let runtime_mode =
                crate::wake_protocol_client::wake_runtime_mode_for_workspace(&workspace)?;
            Ok(json!({
                "action":"web_target_read", "projectId":project_id,
                "readOnly":true, "runtimeMode":runtime_mode,
                "wake":target_status_diagnosed(&workspace, &project_id, runtime_mode),
            }))
        }
        WebAction::BindTarget {
            project_id,
            conversation_url,
            expected_sha256,
            expected_generation,
        } => {
            let runtime_mode =
                crate::wake_protocol_client::wake_runtime_mode_for_workspace(&workspace)?;
            let target = mcp::operator_bind_project_web_target(
                &workspace,
                &project_id,
                &conversation_url,
                expected_sha256.as_deref(),
                expected_generation,
            )
            .map_err(|error| format!("WEB_TARGET: {}", reason(error)))?;
            Ok(
                json!({"action":"web_target_bind", "status":"PAIRED", "target":target,
                "runtimeMode":runtime_mode, "deliveryProved":false, "browserAction":"NONE"}),
            )
        }
        WebAction::ContextStatus { project_id } => Ok(context_status(&workspace, &project_id)),
        WebAction::ContextCapture { project_id } => {
            crate::web_context_control::capture_current(&workspace, &project_id).await
        }
        WebAction::ReviewRead {
            project_id,
            event_id,
        } => read_review(&workspace, &project_id, &event_id),
        WebAction::ReviewReobserve {
            project_id,
            event_id,
        } => review::reobserve(&workspace, &project_id, &event_id),
        WebAction::ReviewReconcile {
            project_id,
            event_id,
        } => review::reconcile(&workspace, &project_id, &event_id),
        WebAction::ReviewDiagnose {
            project_id,
            event_id,
        } => review::diagnose(&workspace, &project_id, &event_id),
        WebAction::ReviewAuthorization {
            project_id,
            operation,
            input_file,
        } => crate::web_work::review_action(
            &workspace,
            &operation,
            read_work_input(&input_file, &project_id)?,
        ),
        WebAction::WorkRequest {
            project_id,
            input_file,
        } => crate::web_work::request(&workspace, read_work_input(&input_file, &project_id)?),
        WebAction::WorkResolve {
            project_id,
            input_file,
        } => crate::web_work::resolve(&workspace, read_work_input(&input_file, &project_id)?),
        WebAction::WorkAssociate {
            project_id,
            input_file,
            assignment_file,
        } => crate::web_work::associate(
            &workspace,
            read_work_input(&input_file, &project_id)?,
            &assignment_file,
        ),
        WebAction::WorkResumePrepare {
            project_id,
            input_file,
        } => {
            crate::web_work::resume_prepare(&workspace, read_work_input(&input_file, &project_id)?)
        }
        WebAction::WorkExecutionPermission {
            project_id,
            input_file,
            revoke,
        } => crate::web_work::execution_permission(
            &workspace,
            read_work_input(&input_file, &project_id)?,
            revoke,
        ),
        WebAction::ContextConfigure {
            project_id,
            input_file,
        } => {
            let key = paired_key(&workspace, &project_id)?;
            let policy: ContextPressurePolicy = read_input(&input_file)?;
            crate::web_context_control::configure(
                &workspace,
                json!({"projectId":project_id,"expectedTargetGeneration":key.target_generation,
                    "expectedTargetSha256":key.target_sha256(),"policy":policy}),
            )
        }
        WebAction::ContextObserve {
            project_id,
            input_file,
        } => {
            let input: ContextInput = read_input(&input_file)?;
            let key = paired_key(&workspace, &project_id)?;
            if input.schema_version != 1
                || input.key != key
                || input.events.is_empty()
                || input.events.len() > 512
            {
                return Err("WEB_CONTEXT_INPUT_SCOPE_INVALID".into());
            }
            for event in &input.events {
                crate::web_conversation_context::validate_observation(event.observation())?;
            }
            mcp::with_current_project_web_target(
                &workspace,
                &project_id,
                key.target_generation,
                &key.target_sha256(),
                |control, _| {
                    let store =
                        WebContextStore::open_nonblocking(control.join(".catdesk/web-context"))?;
                    let previous = store.active_key(&project_id)?;
                    store.bind_observed_target(key.clone(), previous.as_ref())?;
                    let pressure = store.record_observation_batch(
                        &key,
                        input.events.iter().map(ContextEvent::observation),
                    )?;
                    Ok(json!({"action":"web_context_observe","pressure":pressure,
                        "originAuthenticated":false,"pairedTargetVerified":true,
                        "rawTextPersisted":false,"browserAction":"NONE","hostStarted":false}))
                },
            )
        }
        WebAction::RolloverStatus { project_id } => {
            operator_rollover_status(&workspace, &project_id)
        }
        WebAction::RolloverHandoffRequest {
            project_id,
            source_task_id,
        } => rollover::request_handoff(&workspace, &project_id, &source_task_id),
        WebAction::RolloverHandoffCollect { project_id } => {
            rollover::collect_handoff(&workspace, &project_id)
        }
        WebAction::RolloverPrepare {
            project_id,
            handoff_file,
        } => rollover::prepare(
            &workspace,
            &project_id,
            rollover::read_handoff(&handoff_file)?,
        ),
        WebAction::RolloverExecute { project_id } => rollover::execute(&workspace, &project_id),
        WebAction::RolloverInspectDraft {
            project_id,
            expected_intent_id,
        } => rollover::inspect_draft(&workspace, &project_id, &expected_intent_id),
        WebAction::RolloverInspectQuarantine {
            project_id,
            expected_intent_id,
        } => rollover::inspect_quarantine(&workspace, &project_id, &expected_intent_id),
        WebAction::RolloverArchiveDraft {
            project_id,
            expected_intent_id,
        } => rollover::archive_draft(&workspace, &project_id, &expected_intent_id),
        WebAction::RolloverRepairDraft {
            project_id,
            expected_intent_id,
            expected_raw_digest,
        } => rollover::repair_draft(
            &workspace,
            &project_id,
            &expected_intent_id,
            &expected_raw_digest,
        ),
        WebAction::RolloverQuarantine {
            project_id,
            expected_intent_id,
            observed_conversation_url,
        } => rollover::quarantine(
            &workspace,
            &project_id,
            &expected_intent_id,
            &observed_conversation_url,
        ),
        WebAction::RolloverDispositionPrepare {
            project_id,
            expected_intent_id,
            resolution_id,
            acknowledgement,
        } => rollover::prepare_quarantine_disposition(
            &workspace,
            &project_id,
            &expected_intent_id,
            &resolution_id,
            &acknowledgement,
        ),
        WebAction::RolloverDispositionConfirm {
            project_id,
            expected_intent_id,
            resolution_id,
            acknowledgement,
            confirmation_token,
        } => rollover::confirm_quarantine_disposition(
            &workspace,
            &project_id,
            &expected_intent_id,
            &resolution_id,
            &confirmation_token,
            &acknowledgement,
        ),
        WebAction::AutoTick { project_id } => auto::tick_project(&workspace, &project_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).into()).collect()
    }

    #[cfg(windows)]
    #[test]
    fn diagnosed_review_read_preserves_sent_evidence_and_never_promotes_timeout_text() {
        use catdesk_wake::{
            runtime::{SnapshotDiagnostic, SnapshotReadPhase, SnapshotReadReport},
            store::{Phase, Receipt, now},
        };
        let (control, _, wake, _, event) = crate::web_work::tests::assignment_fixture_for("alpha");
        wake.publish(&event).unwrap();
        let owner = wake.host_lock().unwrap();
        let delivery = wake.claim(&event, &owner).unwrap();
        wake.transition(
            &event.event_id,
            owner.owner(),
            Phase::Submitting,
            None,
            None,
        )
        .unwrap();
        let receipt = Receipt {
            schema_version: 1,
            event_id: event.event_id.clone(),
            target_generation: delivery.target.generation,
            target_digest: delivery.target.digest.clone(),
            message_digest: delivery.message_digest.clone(),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        wake.record_exact_receipt_timer(&event.event_id, &receipt)
            .unwrap();
        wake.transition(
            &event.event_id,
            owner.owner(),
            Phase::Sent,
            Some(receipt),
            None,
        )
        .unwrap();
        drop(owner);
        let paths = [
            wake.root().join("config.json"),
            wake.root().join("control.json"),
            wake.root()
                .join(format!("deliveries/{}.json", event.event_id)),
            wake.root().join(format!("turns/{}.json", event.event_id)),
            control.join(".catdesk/projects/projects.json"),
            control.join(".catdesk/web-work/alpha.json"),
        ];
        let before: Vec<_> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
        let result = read_review_using(
            &control,
            "alpha",
            &event.event_id,
            None,
            &|| Ok(()),
            |_, project, target, id, guard| {
                assert_eq!(project, "alpha");
                assert_eq!(id, event.event_id);
                assert_eq!(target, &delivery.target);
                guard().unwrap();
                SnapshotReadReport {
                    snapshot: Some(json!({"private":"never forward failure text"})),
                    failure: Some("SOURCE_SNAPSHOT_TIMEOUT".into()),
                    diagnostic: SnapshotDiagnostic {
                        phase: SnapshotReadPhase::AdapterReadyWait,
                        elapsed_millis: 90000,
                        budget_millis: 90000,
                    },
                }
            },
        )
        .unwrap();
        assert_eq!(result["eventVerified"], false);
        assert_eq!(result["reason"], "SOURCE_SNAPSHOT_TIMEOUT");
        assert_eq!(result["diagnostic"]["phase"], "ADAPTER_READY_WAIT");
        assert!(result.get("response").is_none() && result.get("snapshot").is_none());
        assert!(!result.to_string().contains("never forward"));
        let snapshot = json!({"messages":[
            {"role":"USER","text":event.message,"textComplete":true},
            {"role":"ASSISTANT","text":"Verified review body.","textComplete":true}
        ]});
        let success = read_review_using(
            &control,
            "alpha",
            &event.event_id,
            None,
            &|| Ok(()),
            |_, _, _, _, _| SnapshotReadReport {
                snapshot: Some(snapshot),
                failure: None,
                diagnostic: SnapshotDiagnostic {
                    phase: SnapshotReadPhase::Complete,
                    elapsed_millis: 10,
                    budget_millis: 90000,
                },
            },
        )
        .unwrap();
        assert_eq!(success["eventVerified"], true);
        assert_eq!(success["response"], "Verified review body.");
        assert!(
            read_review_using(
                &control,
                "alpha",
                &event.event_id,
                Some((delivery.target.generation + 1, &delivery.target.digest)),
                &|| Ok(()),
                |_, _, _, _, _| panic!("wrong target must refuse before any reader")
            )
            .is_err()
        );
        for (path, original) in paths.iter().zip(before) {
            assert_eq!(std::fs::read(path).unwrap(), original);
        }
    }

    #[test]
    fn quarantine_disposition_cli_requires_explicit_unknown_ack_and_exact_identity() {
        let prepare = args(&[
            "operator",
            "web",
            "rollover",
            "disposition-prepare",
            "--project-id",
            "alpha",
            "--expected-intent-id",
            "intent-a",
            "--resolution-id",
            "resolution-a",
            "--acknowledge-unknown",
            "ABANDONED_UNKNOWN_NO_REPLAY",
        ]);
        assert!(matches!(
            parse(&prepare).unwrap(),
            WebAction::RolloverDispositionPrepare { .. }
        ));
        let mut missing = prepare.clone();
        missing.truncate(missing.len() - 2);
        assert!(parse(&missing).is_err());
        let mut wrong = prepare.clone();
        *wrong.last_mut().unwrap() = "FAILED".into();
        assert!(parse(&wrong).is_err());
        let mut confirm = prepare;
        confirm[3] = "disposition-confirm".into();
        assert!(parse(&confirm).is_err());
        confirm.extend(args(&["--confirmation-token", "confirmed-token"]));
        assert!(matches!(
            parse(&confirm).unwrap(),
            WebAction::RolloverDispositionConfirm { .. }
        ));
        confirm.extend(args(&["--force", "true"]));
        assert!(parse(&confirm).is_err());
    }

    #[test]
    fn quarantine_disposition_status_keeps_unknown_and_redacts_private_authority() {
        let raw = json!({"phase":"QUARANTINED","operatorDisposition":{
            "status":"ABANDONED_UNKNOWN_NO_REPLAY","resolutionId":"resolution-a",
            "complete":true,"providerOutcome":"UNKNOWN","confirmationToken":"private-token",
            "originalJournalPreserved":true}});
        let status = registry_rollover_projection(&raw);
        assert_eq!(status["phase"], "QUARANTINED");
        assert_eq!(
            status["operatorDisposition"]["status"],
            "ABANDONED_UNKNOWN_NO_REPLAY"
        );
        assert_eq!(status["operatorDisposition"]["complete"], true);
        assert_eq!(status["operatorDisposition"]["providerOutcome"], "UNKNOWN");
        assert!(!status.to_string().contains("private-token"));
        assert_eq!(status["continuationProved"], false);
    }

    #[test]
    fn retirement_requires_stable_identity_and_exact_target_without_overrides() {
        for (generation, digest) in [("0", "none".to_owned()), ("3", "a".repeat(64))] {
            let command = args(&[
                "operator",
                "web",
                "project",
                "retire",
                "--project-id",
                "alpha",
                "--retirement-id",
                "retirement-one",
                "--expected-wake-generation",
                generation,
                "--expected-target-sha256",
                &digest,
            ]);
            assert!(matches!(
                parse(&command),
                Ok(WebAction::RetireProject { .. })
            ));
            for length in [6, 8, 10] {
                assert!(parse(&command[..length]).is_err());
            }
            for (option, value) in [
                ("--force", "true"),
                ("--workspace", "elsewhere"),
                ("--project-id", "beta"),
                ("--retirement-id", "changed"),
            ] {
                let mut invalid = command.clone();
                invalid.extend(args(&[option, value]));
                assert!(parse(&invalid).is_err());
            }
            for (index, value) in [(7, "../other"), (9, "03"), (11, "bad-digest")] {
                let mut invalid = command.clone();
                invalid[index] = value.into();
                assert!(parse(&invalid).is_err());
            }
        }
        for (generation, digest) in [("0", "a".repeat(64)), ("1", "none".to_owned())] {
            assert!(
                parse(&args(&[
                    "operator",
                    "web",
                    "project",
                    "retire",
                    "--project-id",
                    "alpha",
                    "--retirement-id",
                    "retirement-one",
                    "--expected-wake-generation",
                    generation,
                    "--expected-target-sha256",
                    &digest
                ]))
                .is_err()
            );
        }
    }

    #[test]
    fn work_commands_require_exact_project_and_input_without_scope_overrides() {
        for verb in ["request", "resolve"] {
            let command = args(&[
                "operator",
                "web",
                "work",
                verb,
                "--project-id",
                "alpha",
                "--input-file",
                "work.json",
            ]);
            let parsed = parse(&command).unwrap();
            assert_eq!(
                parsed,
                if verb == "request" {
                    WebAction::WorkRequest {
                        project_id: "alpha".into(),
                        input_file: "work.json".into(),
                    }
                } else {
                    WebAction::WorkResolve {
                        project_id: "alpha".into(),
                        input_file: "work.json".into(),
                    }
                }
            );
            assert!(parse(&command[..6]).is_err());
            for (option, value) in [
                ("--project-id", "beta"),
                ("--input-file", "other.json"),
                ("--workspace", "private-workspace"),
                ("--conversation-url", "private-target"),
                ("--force", "true"),
            ] {
                let mut invalid = command.clone();
                invalid.extend(args(&[option, value]));
                assert!(parse(&invalid).is_err());
            }
            let mut empty = command;
            empty.last_mut().unwrap().clear();
            assert!(parse(&empty).is_err());
        }
    }

    #[test]
    fn review_read_requires_exact_latest_complete_delivered_pair() {
        let message_digest = catdesk_wake::protocol::digest("approved review request");
        let mut snapshot = json!({"messages":[
            {"role":"USER","text":"approved review\nrequest","textComplete":true},
            {"role":"ASSISTANT","text":"Review findings.","textComplete":true}
        ]});
        assert_eq!(
            review_text(&snapshot, &message_digest).unwrap(),
            "Review findings."
        );
        assert!(review_text(&snapshot, &"0".repeat(64)).is_err());
        snapshot["messages"][1]["textComplete"] = json!(false);
        assert!(review_text(&snapshot, &message_digest).is_err());
        snapshot["messages"][1]["textComplete"] = json!(true);
        snapshot["messages"][1]["text"] = json!("This response couldn’t load");
        assert!(review_text(&snapshot, &message_digest).is_err());
        snapshot["messages"].as_array_mut().unwrap().push(json!({
            "role":"USER","text":"different task","textComplete":true
        }));
        assert!(review_text(&snapshot, &message_digest).is_err());
        let valid = args(&[
            "operator",
            "web",
            "review",
            "read",
            "--project-id",
            "catdesk",
            "--event-id",
            "review-one",
        ]);
        assert!(matches!(parse(&valid), Ok(WebAction::ReviewRead { .. })));
        assert!(parse(&valid[..6]).is_err());
        let mut invalid = valid;
        invalid.extend(args(&["--submit", "true"]));
        assert!(parse(&invalid).is_err());
    }

    #[test]
    fn review_reobserve_accepts_only_one_project_and_existing_event() {
        let valid = args(&[
            "operator",
            "web",
            "review",
            "reobserve",
            "--project-id",
            "alpha",
            "--event-id",
            "review-one",
        ]);
        assert!(matches!(
            parse(&valid),
            Ok(WebAction::ReviewReobserve { .. })
        ));
        for extra in ["--url", "--message", "--send", "--max-runtime-seconds"] {
            let mut invalid = valid.clone();
            invalid.extend(args(&[extra, "override"]));
            assert!(parse(&invalid).is_err());
        }
        let mut invalid = valid;
        *invalid.last_mut().unwrap() = "../event".into();
        assert!(parse(&invalid).is_err());
    }

    #[test]
    fn review_reconcile_has_only_existing_project_event_inputs() {
        let valid = args(&[
            "operator",
            "web",
            "review",
            "reconcile",
            "--project-id",
            "alpha",
            "--event-id",
            "review-one",
        ]);
        assert!(matches!(
            parse(&valid),
            Ok(WebAction::ReviewReconcile { .. })
        ));
        assert!(parse(&valid[..6]).is_err());
        for flag in [
            "--url",
            "--message",
            "--submit",
            "--force",
            "--max-runtime-seconds",
        ] {
            let mut invalid = valid.clone();
            invalid.extend(args(&[flag, "override"]));
            assert!(parse(&invalid).is_err());
        }
    }

    #[test]
    fn review_diagnose_has_no_sender_or_destination_override() {
        let valid = args(&[
            "operator",
            "web",
            "review",
            "diagnose",
            "--project-id",
            "alpha",
            "--event-id",
            "review-one",
        ]);
        assert!(matches!(
            parse(&valid),
            Ok(WebAction::ReviewDiagnose { .. })
        ));
        for flag in [
            "--url",
            "--message",
            "--submit",
            "--force",
            "--max-runtime-seconds",
        ] {
            let mut invalid = valid.clone();
            invalid.extend(args(&[flag, "override"]));
            assert!(parse(&invalid).is_err());
        }
    }

    #[test]
    fn scoped_review_commands_never_accept_destination_or_execution_overrides() {
        for verb in ["authorize", "revoke", "preflight", "run", "resume"] {
            let valid = args(&[
                "operator",
                "web",
                "review",
                verb,
                "--project-id",
                "alpha",
                "--input-file",
                "frozen.json",
            ]);
            assert!(matches!(
                parse(&valid),
                Ok(WebAction::ReviewAuthorization { .. })
            ));
            for override_flag in [
                "--url",
                "--workspace",
                "--message",
                "--force",
                "--max-runtime-seconds",
            ] {
                let mut invalid = valid.clone();
                invalid.extend(args(&[override_flag, "override"]));
                assert!(parse(&invalid).is_err());
            }
            assert!(parse(&valid[..6]).is_err());
        }
    }

    #[test]
    fn assignment_commands_are_operator_only_explicit_and_prepare_never_accepts_launch_options() {
        let common = [
            "operator",
            "web",
            "work",
            "resume-prepare",
            "--project-id",
            "alpha",
            "--input-file",
            "pins.json",
        ];
        assert_eq!(
            parse(&args(&common)).unwrap(),
            WebAction::WorkResumePrepare {
                project_id: "alpha".into(),
                input_file: "pins.json".into()
            }
        );
        let mut associate = args(&common);
        associate[3] = "associate".into();
        assert!(parse(&associate).is_err());
        associate.extend(args(&["--assignment-file", "assignment.json"]));
        assert_eq!(
            parse(&associate).unwrap(),
            WebAction::WorkAssociate {
                project_id: "alpha".into(),
                input_file: "pins.json".into(),
                assignment_file: "assignment.json".into()
            }
        );
        for option in [
            "--assignment-file",
            "--launch",
            "--force",
            "--port",
            "--token",
        ] {
            let mut invalid = args(&common);
            invalid.extend(args(&[option, "value"]));
            assert!(parse(&invalid).is_err());
        }
    }

    #[test]
    fn target_binding_requires_explicit_previous_authority() {
        let base = [
            "operator",
            "web",
            "target",
            "bind",
            "--project-id",
            "alpha",
            "--conversation-url",
            "https://chatgpt.com/c/new",
        ];
        assert!(parse(&args(&base)).is_err());
        let mut complete = args(&base);
        complete.extend(args(&[
            "--expected-current-target-sha256",
            "none",
            "--expected-wake-generation",
            "0",
        ]));
        assert!(matches!(
            parse(&complete),
            Ok(WebAction::BindTarget {
                expected_sha256: None,
                expected_generation: 0,
                ..
            })
        ));
    }

    #[test]
    fn execution_permission_is_local_and_accepts_no_remote_key_or_path_options() {
        for (verb, revoke) in [("execution-grant", false), ("execution-revoke", true)] {
            let base = args(&[
                "operator",
                "web",
                "work",
                verb,
                "--project-id",
                "alpha",
                "--input-file",
                "pins.json",
            ]);
            assert_eq!(
                parse(&base).unwrap(),
                WebAction::WorkExecutionPermission {
                    project_id: "alpha".into(),
                    input_file: "pins.json".into(),
                    revoke
                }
            );
            for option in [
                "--execution-key",
                "--workspace",
                "--assignment-file",
                "--force",
            ] {
                let mut invalid = base.clone();
                invalid.extend(args(&[option, "value"]));
                assert!(parse(&invalid).is_err());
            }
        }
    }

    #[test]
    fn web_only_registration_requires_exact_project_git_scope() {
        let complete = args(&[
            "operator",
            "web",
            "project",
            "register",
            "--project-id",
            "alpha",
            "--workspace",
            "C:\\Projects\\alpha",
            "--git-identity",
            "https://example.test/alpha.git",
        ]);
        assert!(matches!(
            parse(&complete),
            Ok(WebAction::RegisterProject {
                project_id,
                git_identity,
                ..
            }) if project_id == "alpha" && git_identity == "https://example.test/alpha.git"
        ));
        for invalid in [
            &complete[..complete.len() - 2],
            &complete[..complete.len() - 4],
        ] {
            assert!(parse(invalid).is_err());
        }
        let mut unexpected = complete;
        unexpected.extend(args(&["--codex-thread-id", "some-thread"]));
        assert!(parse(&unexpected).is_err());
    }

    #[test]
    fn duplicate_scope_or_extra_mutation_options_are_rejected() {
        for tail in [
            vec!["--project-id", "alpha", "--project-id", "beta"],
            vec!["--project-id", "alpha", "--force", "true"],
            vec!["--project-id", "../alpha"],
        ] {
            let mut command = args(&["operator", "web", "target", "read"]);
            command.extend(args(&tail));
            assert!(parse(&command).is_err());
        }
    }

    #[test]
    fn draft_inspection_requires_one_exact_intent_without_mutation_options() {
        let base = args(&[
            "operator",
            "web",
            "rollover",
            "inspect-draft",
            "--project-id",
            "alpha",
        ]);
        assert!(parse(&base).is_err());
        let mut complete = base.clone();
        complete.extend(args(&["--expected-intent-id", "intent-a"]));
        assert!(
            matches!(parse(&complete), Ok(WebAction::RolloverInspectDraft {
            project_id, expected_intent_id
        }) if project_id == "alpha" && expected_intent_id == "intent-a")
        );
        for tail in [
            vec!["--expected-intent-id", "../other"],
            vec!["--expected-intent-id", "intent-a", "--clear", "true"],
            vec!["--expected-intent-id", "intent-a", "--submit", "true"],
        ] {
            let mut invalid = base.clone();
            invalid.extend(args(&tail));
            assert!(parse(&invalid).is_err());
        }
    }

    #[test]
    fn draft_archival_requires_exact_scope_and_has_no_force_or_send_option() {
        let mut complete = args(&[
            "operator",
            "web",
            "rollover",
            "archive-draft",
            "--project-id",
            "alpha",
            "--expected-intent-id",
            "intent-a",
        ]);
        assert!(
            matches!(parse(&complete), Ok(WebAction::RolloverArchiveDraft {
            project_id, expected_intent_id
        }) if project_id == "alpha" && expected_intent_id == "intent-a")
        );
        assert!(parse(&complete[..6]).is_err());
        for flag in ["--force", "--submit", "--clear", "--draft-file"] {
            let mut invalid = complete.clone();
            invalid.extend(args(&[flag, "true"]));
            assert!(parse(&invalid).is_err());
        }
        complete[7] = "../other".into();
        assert!(parse(&complete).is_err());
    }

    #[test]
    fn draft_repair_requires_exact_raw_witness_without_send_options() {
        let mut complete = args(&[
            "operator",
            "web",
            "rollover",
            "repair-draft",
            "--project-id",
            "alpha",
            "--expected-intent-id",
            "intent-a",
            "--expected-raw-digest",
            &"a".repeat(64),
        ]);
        assert!(
            matches!(parse(&complete), Ok(WebAction::RolloverRepairDraft {
            project_id, expected_intent_id, expected_raw_digest
        }) if project_id == "alpha" && expected_intent_id == "intent-a"
            && expected_raw_digest == "a".repeat(64))
        );
        assert!(parse(&complete[..8]).is_err());
        for flag in ["--force", "--submit", "--clear", "--draft-file"] {
            let mut invalid = complete.clone();
            invalid.extend(args(&[flag, "true"]));
            assert!(parse(&invalid).is_err());
        }
        complete[9] = "a".repeat(63);
        assert!(parse(&complete).is_err());
        complete[9] = "G".repeat(64);
        assert!(parse(&complete).is_err());
    }

    #[test]
    fn quarantine_requires_exact_intent_and_observed_conversation() {
        let complete = args(&[
            "operator",
            "web",
            "rollover",
            "quarantine",
            "--project-id",
            "alpha",
            "--expected-intent-id",
            "intent-a",
            "--observed-conversation-url",
            "https://chatgpt.com/c/fragmented-orphan",
        ]);
        assert!(
            matches!(parse(&complete), Ok(WebAction::RolloverQuarantine { project_id, expected_intent_id, .. }) if project_id == "alpha" && expected_intent_id == "intent-a")
        );
        assert!(parse(&complete[..8]).is_err());
        let inspection = args(&[
            "operator",
            "web",
            "rollover",
            "inspect-quarantine",
            "--project-id",
            "alpha",
            "--expected-intent-id",
            "intent-a",
        ]);
        assert!(
            matches!(parse(&inspection), Ok(WebAction::RolloverInspectQuarantine {
            project_id, expected_intent_id }) if project_id == "alpha" && expected_intent_id == "intent-a")
        );
        assert!(parse(&inspection[..6]).is_err());
        let mut redirect = inspection;
        redirect.extend(args(&["--candidate-url", "https://chatgpt.com/c/other"]));
        assert!(parse(&redirect).is_err());
        let mut invalid = complete.clone();
        *invalid.last_mut().unwrap() = "https://example.com/c/untrusted".into();
        assert!(parse(&invalid).is_err());
        let mut extra = complete;
        extra.extend(args(&["--force", "true"]));
        assert!(parse(&extra).is_err());
    }

    #[test]
    fn automatic_handoff_commands_require_exact_project_and_task_scope() {
        let request = args(&[
            "operator",
            "web",
            "rollover",
            "handoff-request",
            "--project-id",
            "alpha",
            "--source-task-id",
            "task-a",
        ]);
        assert!(matches!(
            parse(&request),
            Ok(WebAction::RolloverHandoffRequest {
                project_id,
                source_task_id
            }) if project_id == "alpha" && source_task_id == "task-a"
        ));
        let collect = args(&[
            "operator",
            "web",
            "rollover",
            "handoff-collect",
            "--project-id",
            "alpha",
        ]);
        assert!(matches!(
            parse(&collect),
            Ok(WebAction::RolloverHandoffCollect { project_id }) if project_id == "alpha"
        ));
        for invalid in [
            args(&[
                "operator",
                "web",
                "rollover",
                "handoff-request",
                "--project-id",
                "alpha",
            ]),
            args(&[
                "operator",
                "web",
                "rollover",
                "handoff-request",
                "--project-id",
                "alpha",
                "--source-task-id",
                "../other",
            ]),
            args(&[
                "operator",
                "web",
                "rollover",
                "handoff-collect",
                "--project-id",
                "alpha",
                "--source-task-id",
                "task-a",
            ]),
        ] {
            assert!(parse(&invalid).is_err());
        }
    }

    #[test]
    fn auto_tick_requires_one_exact_project_scope() {
        let command = args(&["operator", "web", "auto", "tick", "--project-id", "alpha"]);
        assert!(matches!(
            parse(&command),
            Ok(WebAction::AutoTick { project_id }) if project_id == "alpha"
        ));
        let extra = args(&[
            "operator",
            "web",
            "auto",
            "tick",
            "--project-id",
            "alpha",
            "--force",
            "true",
        ]);
        assert!(parse(&extra).is_err());
    }

    #[test]
    fn creation_admission_projection_is_durable_metadata_not_delivery_or_owner_proof() {
        let projected = registry_rollover_projection(&json!({"phase":"PREPARED",
            "creationAdmission":{"status":"DURABLE_SNAPSHOT","phase":"WAITING","waitingAhead":2,
                "liveOwnerVerified":true,"creationProof":true,"handoff":"private text"}}));
        assert_eq!(projected["creationAdmission"]["phase"], "WAITING");
        assert_eq!(projected["creationAdmission"]["waitingAhead"], 2);
        assert_eq!(projected["creationAdmission"]["liveOwnerVerified"], false);
        assert_eq!(projected["creationAdmission"]["creationProof"], false);
        assert!(!projected.to_string().contains("private text"));
    }

    #[test]
    fn registry_rollover_projection_exposes_progress_without_private_handoff() {
        let raw = json!({
            "phase":"RESPONSE_PENDING",
            "source":{"url":"https://chatgpt.com/c/private-source"},
            "receipt":{"assistantText":"private answer"},
            "handoffRequest":{"handoffCollected":true,"message":"private handoff"},
            "reconciliationRequired":false,
            "contextMeterStatus":"PENDING",
            "bootstrapProgress":"SEND_ATTEMPTED",
            "automation":{"stage":"WAKE_STOPPED","reason":"WAKE_CONTROL_CHANGED",
                "tickUtc":123,"targetGeneration":7}
        });
        let summary = registry_rollover_projection(&raw);
        assert_eq!(summary["status"], "DURABLE_SNAPSHOT");
        assert_eq!(summary["phase"], "RESPONSE_PENDING");
        assert_eq!(summary["automation"]["stage"], "WAKE_STOPPED");
        assert_eq!(summary["automation"]["targetGeneration"], 7);
        assert!(summary["automation"]["lastFailure"].is_null());
        assert_eq!(summary["handoffCollected"], true);
        assert_eq!(summary["bootstrapProgress"], "SEND_ATTEMPTED");
        assert_eq!(summary["continuationProved"], false);
        assert_eq!(summary["continuationProofStatus"], "NOT_CHECKED");
        for (phase, proof, value, expected) in [
            ("CONTINUATION_QUEUED", "VERIFIED", true, true),
            ("CONTINUATION_QUEUED", "VERIFIED", false, false),
            ("RESPONSE_PENDING", "VERIFIED", true, false),
            ("CONTINUATION_QUEUED", "private-path", true, false),
        ] {
            let projected = registry_rollover_projection(&json!({"phase":phase,
                "continuationProofStatus":proof,"continuationProved":value}));
            assert_eq!(projected["continuationProved"], expected);
            assert!(!projected.to_string().contains("private-path"));
        }
        let encoded = summary.to_string();
        for private in ["private-source", "private answer", "private handoff"] {
            assert!(!encoded.contains(private));
        }
        let bad = registry_rollover_projection(&json!({
            "phase":"NONE","automation":{"stage":"C:\\private\\secret.txt",
                "reason":"C:\\private\\secret.txt"}
        }));
        assert_eq!(bad["automation"]["stage"], "UNKNOWN_STAGE");
        assert!(bad["automation"]["reason"].is_null());
        assert!(!bad.to_string().contains("secret.txt"));
        let target_wait = registry_rollover_projection(&json!({
            "phase":"NONE","automation":{"stage":"AWAITING_TARGET",
                "reason":"WEB_TARGET_WAKE_CONFIG_BUSY","targetGeneration":null}
        }));
        assert_eq!(target_wait["automation"]["stage"], "AWAITING_TARGET");
        assert_eq!(
            target_wait["automation"]["reason"],
            "WEB_TARGET_WAKE_CONFIG_BUSY"
        );
        assert!(target_wait["automation"]["targetGeneration"].is_null());
        assert!(target_wait["automation"]["lastFailure"].is_null());
        for progress in ["TEXT_ENTRY_AUTHORIZED", "DRAFT_READY", "SEND_ATTEMPTED"] {
            let projected = registry_rollover_projection(&json!({
                "phase":"SUBMITTING", "bootstrapProgress":progress
            }));
            assert_eq!(projected["bootstrapProgress"], progress);
        }
        let invalid_progress = registry_rollover_projection(&json!({
            "phase":"SUBMITTING", "bootstrapProgress":"private draft content"
        }));
        assert!(invalid_progress["bootstrapProgress"].is_null());
        assert!(
            !invalid_progress
                .to_string()
                .contains("private draft content")
        );
        assert!(
            registry_rollover_projection(&json!({"phase":"SUBMITTING"}))["bootstrapProgress"]
                .is_null()
        );
        let failed = registry_rollover_projection(&json!({
            "phase":"PREPARED", "source":{"generation":7,"digest":"a".repeat(64)},
            "automation":{"stage":"ATTENTION","reason":"WEB_ROLLOVER_NOT_DUE_OR_DISABLED",
                "targetGeneration":7,"lastFailure":{"reason":"EXISTING_DRAFT",
                    "tickUtc":42,"targetGeneration":7}}
        }));
        assert_eq!(
            failed["automation"]["reason"],
            "WEB_ROLLOVER_NOT_DUE_OR_DISABLED"
        );
        assert_eq!(
            failed["automation"]["lastFailure"]["reason"],
            "EXISTING_DRAFT"
        );
        assert!(failed["automation"]["lastFailure"]["requiredAction"].is_null());
        let malformed_failure = registry_rollover_projection(&json!({
            "phase":"PREPARED", "automation":{"stage":"ATTENTION",
                "lastFailure":{"reason":"C:\\private\\secret.txt", "tickUtc":42,
                    "targetGeneration":7}}
        }));
        assert!(malformed_failure["automation"]["lastFailure"].is_null());
        assert!(!malformed_failure.to_string().contains("secret.txt"));
        let requested = registry_rollover_projection(&json!({
            "phase":"NONE","handoffRequest":{"source":{"generation":4},
                "handoffCollected":false},"automation":null
        }));
        assert_eq!(requested["status"], "DURABLE_SNAPSHOT");
        assert_eq!(requested["phase"], "NONE");
        assert_eq!(requested["targetGeneration"], 4);
        assert_eq!(requested["handoffRequested"], true);
        let quarantined = registry_rollover_projection(&json!({
            "phase":"QUARANTINED",
            "quarantine":{"observedConversationUrl":"https://chatgpt.com/c/private-orphan"},
            "automation":{"stage":"QUARANTINED","reason":"WEB_ROLLOVER_QUARANTINED_NO_REPLAY"}
        }));
        assert_eq!(quarantined["phase"], "QUARANTINED");
        assert_eq!(quarantined["automation"]["stage"], "QUARANTINED");
        assert!(!quarantined.to_string().contains("private-orphan"));
    }

    #[test]
    fn registry_rollover_status_missing_and_malformed_state_is_local_and_read_only() {
        let workspace = std::env::temp_dir().join(format!(
            "catdesk-web-registry-status-{}",
            uuid::Uuid::new_v4()
        ));
        let missing = registry_rollover_status(&workspace, "alpha");
        assert_eq!(missing["status"], "UNOBSERVED");
        assert!(
            !workspace.exists(),
            "read did not create an absent workspace"
        );
        let partial_root = workspace.join(".catdesk/web-rollovers/delta");
        std::fs::create_dir_all(&partial_root).expect("partial project root");
        assert!(rollover_status_read_only(&workspace, "delta").is_err());
        assert!(
            !partial_root.join("project.lock").exists(),
            "read did not create a lock"
        );
        let project_root = workspace.join(".catdesk/web-rollovers/beta");
        std::fs::create_dir_all(&project_root).expect("fixture project root");
        std::fs::write(project_root.join("project.lock"), b"").expect("existing lock");
        std::fs::write(project_root.join("active.json"), b"not-json").expect("malformed pointer");
        let malformed = registry_rollover_status(&workspace, "beta");
        assert_eq!(malformed["status"], "UNAVAILABLE_OR_BUSY");
        assert_eq!(
            registry_rollover_status(&workspace, "alpha")["status"],
            "UNOBSERVED"
        );
        let healthy_root = workspace.join(".catdesk/web-rollovers/gamma");
        std::fs::create_dir_all(&healthy_root).expect("healthy project root");
        std::fs::write(healthy_root.join("project.lock"), b"").expect("healthy lock");
        std::fs::write(
            healthy_root.join("auto-status.json"),
            serde_json::to_vec(&json!({"schemaVersion":1,"projectId":"gamma",
                "tickUtc":42,"stage":"WAKE_STOPPED","targetGeneration":3,
                "reason":"WAKE_CONTROL_CHANGED"}))
            .unwrap(),
        )
        .expect("healthy automation status");
        let healthy = registry_rollover_status(&workspace, "gamma");
        assert_eq!(healthy["status"], "NO_ROLLOVER");
        assert_eq!(healthy["phase"], "NONE");
        assert_eq!(healthy["automation"]["stage"], "WAKE_STOPPED");
        assert_eq!(
            registry_rollover_status(&workspace, "beta")["status"],
            "UNAVAILABLE_OR_BUSY"
        );
        std::fs::remove_dir_all(workspace).expect("fixture cleanup");
    }
}
