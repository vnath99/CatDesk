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

#[path = "operator_web_observe.rs"]
mod observe;
#[path = "operator_web_rollover.rs"]
mod rollover;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WebAction {
    Status,
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

fn paired_key(workspace: &Path, project: &str) -> Result<ConversationKey, String> {
    let target = mcp::operator_read_project_web_target(workspace, project)
        .map_err(|e| format!("WEB_TARGET: {}", reason(e)))?
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
    match WebContextStore::open(root).and_then(|store| store.project_history(project)) {
        Ok(history) => json!({"status":"OBSERVED","conversations":history,"advisory":true}),
        Err(_) => json!({"status":"UNOBSERVED_OR_UNAVAILABLE","advisory":true}),
    }
}

fn target_status(workspace: &Path, project_id: &str) -> Value {
    match mcp::operator_read_project_web_target(workspace, project_id) {
        Ok(Some(target)) => json!({"status":"PAIRED", "target":target, "deliveryProved":false}),
        Ok(None) => json!({"status":"UNBOUND", "deliveryProved":false}),
        Err(error) => json!({"status":"ATTENTION", "reason":reason(error), "deliveryProved":false}),
    }
}

pub(crate) fn execute(action: WebAction, workspace: &Path) -> Result<Value, String> {
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "WEB_SCOPE: control workspace is unavailable")?;
    match action {
        WebAction::Status => {
            let registry = AutonomousProjectRegistryStoreV1::open_read_only(
                workspace.join(".catdesk/projects"),
            )
            .and_then(|store| store.load_registry())
            .map_err(|_| "WEB_SCOPE: control registry is unavailable")?;
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
                        "wake":target_status(&workspace, &project.project_id),
                        "context":context_status(&workspace,&project.project_id),
                    })
                })
                .collect();
            Ok(
                json!({"action":"web_status", "readOnly":true, "projects":projects,
                "meaning":"Per-project registration and paired Wake readback. Browser authentication, delivery and continuation require their own receipts."}),
            )
        }
        WebAction::ReadTarget { project_id } => Ok(json!({
            "action":"web_target_read", "projectId":project_id,
            "readOnly":true, "wake":target_status(&workspace, &project_id),
        })),
        WebAction::BindTarget {
            project_id,
            conversation_url,
            expected_sha256,
            expected_generation,
        } => {
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
                "deliveryProved":false, "browserAction":"NONE"}),
            )
        }
        WebAction::ContextStatus { project_id } => Ok(context_status(&workspace, &project_id)),
        WebAction::ContextCapture { project_id } => {
            let key = paired_key(&workspace, &project_id)?;
            let wake = catdesk_wake::store::Store::open(&catdesk_wake::runtime::default_root()?)?;
            let source = catdesk_wake::protocol::Target::new(
                key.target_generation,
                key.conversation_url.clone(),
            )?;
            let snapshot =
                catdesk_wake::runtime::snapshot_conversation(&wake, &project_id, &source)?;
            if paired_key(&workspace, &project_id)? != key {
                return Err("WEB_CONTEXT_TARGET_CHANGED".into());
            }
            let store = WebContextStore::open(workspace.join(".catdesk/web-context"))?;
            let previous = store.active_key(&project_id)?;
            store.bind_observed_target(key.clone(), previous.as_ref())?;
            observe::ingest(&store, &key, snapshot, catdesk_wake::store::now(), || {
                if paired_key(&workspace, &project_id)? == key {
                    Ok(())
                } else {
                    Err("WEB_CONTEXT_TARGET_CHANGED".into())
                }
            })
        }
        WebAction::ContextConfigure {
            project_id,
            input_file,
        } => {
            let key = paired_key(&workspace, &project_id)?;
            let policy: ContextPressurePolicy = read_input(&input_file)?;
            let store = WebContextStore::open(workspace.join(".catdesk/web-context"))?;
            store.configure_policy(&project_id, policy)?;
            let previous = store.active_key(&project_id)?;
            store.bind_observed_target(key.clone(), previous.as_ref())?;
            Ok(json!({"action":"web_context_configure","pressure":store.pressure(&key)?}))
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
            let store = WebContextStore::open(workspace.join(".catdesk/web-context"))?;
            let previous = store.active_key(&project_id)?;
            store.bind_observed_target(key.clone(), previous.as_ref())?;
            for event in &input.events {
                // Recheck independently of the file's claimed project key.
                if paired_key(&workspace, &project_id)? != key {
                    return Err("WEB_CONTEXT_TARGET_CHANGED".into());
                }
                store.record_observation(
                    &key,
                    Observation {
                        event_id: &event.event_id,
                        revision: event.revision,
                        dedup_key: &event.dedup_key,
                        source_kind: event.source_kind,
                        visible_text: event.visible_text.as_deref(),
                        observed_at_unix: event.observed_at_unix,
                    },
                )?;
            }
            Ok(
                json!({"action":"web_context_observe","pressure":store.pressure(&key)?,"originAuthenticated":false}),
            )
        }
        WebAction::RolloverStatus { project_id } => rollover::status(&workspace, &project_id),
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).into()).collect()
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
}
