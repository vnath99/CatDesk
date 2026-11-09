//! Receipt-bound, no-send observation of an existing project response.
//!
//! This deliberately adds no submission, trace-click or reconciliation action.

use std::{collections::BTreeMap, path::Path};

use serde_json::{Value, json};

use crate::{
    web_conversation_context::ConversationKey,
    web_work_state::{WorkLedger, WorkRecord},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ResponseKind {
    Sent,
    Historical,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExistingResponseAction {
    project_id: String,
    event_id: String,
    kind: ResponseKind,
}

/// Leave every existing Web verb to its original parser. The two added verbs
/// accept only a registered project identity and an existing delivery identity.
pub(super) fn parse(args: &[String]) -> Result<Option<ExistingResponseAction>, String> {
    if args.first().map(String::as_str) != Some("operator")
        || args.get(1).map(String::as_str) != Some("web")
        || args.get(2).map(String::as_str) != Some("review")
    {
        return Ok(None);
    }
    let kind = match args.get(3).map(String::as_str) {
        Some("inspect-response") => ResponseKind::Sent,
        Some("inspect-history-response") => ResponseKind::Historical,
        _ => return Ok(None),
    };
    if !(args.len() - 4).is_multiple_of(2) {
        return Err("WEB_USAGE: each option requires one value".into());
    }
    let mut options = BTreeMap::new();
    for pair in args[4..].chunks_exact(2) {
        if options.insert(pair[0].as_str(), pair[1].as_str()).is_some() {
            return Err("WEB_USAGE: duplicate option".into());
        }
    }
    let project_id = options
        .remove("--project-id")
        .ok_or("WEB_USAGE: --project-id is required")?;
    let event_id = options
        .remove("--event-id")
        .ok_or("WEB_USAGE: --event-id is required")?;
    if project_id.is_empty()
        || project_id.len() > 128
        || !project_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        || !catdesk_wake::protocol::valid_id(event_id)
        || !options.is_empty()
    {
        return Err("WEB_USAGE: invalid response identity or unexpected option".into());
    }
    Ok(Some(ExistingResponseAction {
        project_id: project_id.into(),
        event_id: event_id.into(),
        kind,
    }))
}

fn paired_key(workspace: &Path, project: &str) -> Result<ConversationKey, String> {
    let target = crate::mcp::operator_read_project_web_target_diagnosed(workspace, project)
        .map_err(|failure| failure.code.to_string())?
        .ok_or("WEB_TARGET_UNBOUND")?;
    Ok(ConversationKey {
        project_id: project.into(),
        conversation_url: target.url,
        target_generation: target.generation,
    })
}

fn validate_attribution(
    kind: ResponseKind,
    record: Option<&WorkRecord>,
    target: &catdesk_wake::protocol::Target,
) -> Result<(), String> {
    if kind == ResponseKind::Historical
        && record.is_none_or(|record| record.task_assignment.is_none())
    {
        return Err("WEB_REVIEW_TASK_BINDING_REQUIRED".into());
    }
    if record.is_some_and(|record| record.target != *target) {
        return Err("WEB_REVIEW_TASK_BINDING_CHANGED".into());
    }
    Ok(())
}

fn task_attribution(project: &str, record: Option<WorkRecord>) -> Value {
    json!(
        record.and_then(|record| record.task_assignment.map(|assignment| {
            json!({"projectId":project,"requestId":record.request_id,
            "sourceEventId":record.event_id,"taskId":assignment.task_id,
            "assignmentSha256":assignment.assignment_sha256,
            "scopeSha256":assignment.scope_sha256,"executionVerified":false})
        }))
    )
}

pub(super) fn execute(action: ExistingResponseAction, workspace: &Path) -> Result<Value, String> {
    let ExistingResponseAction {
        project_id: project,
        event_id,
        kind,
    } = action;
    // Reuse the existing per-project rollover/work ownership lease. No browser
    // observation can reinterpret a quarantined or changed project binding.
    let key = paired_key(workspace, &project)?;
    let lease = super::acquire_web_work_lease(workspace, &project)?;
    let prepared =
        crate::delegated::autonomy_projects::PreparedProjectScopeV1::resolve(workspace, &project)
            .map_err(|_| "WEB_REVIEW_PROJECT_CHANGED")?;
    let wake = catdesk_wake::store::Store::open(
        &crate::wake_protocol_client::wake_root_for_workspace(workspace)?,
    )?;
    let target =
        catdesk_wake::protocol::Target::new(key.target_generation, key.conversation_url.clone())?;
    let delivery = wake
        .delivery(&event_id)?
        .ok_or("WEB_REVIEW_EVENT_MISSING")?;
    let source_event = delivery
        .event
        .wake_metadata()?
        .map(|metadata| metadata.source_event_id)
        .unwrap_or_else(|| delivery.event.event_id.clone());
    let task_record = WorkLedger::record_for_event(workspace, &project, &source_event)?;
    validate_attribution(kind, task_record.as_ref(), &target)?;
    let guard = || {
        prepared
            .revalidate_registration()
            .map_err(|_| "WEB_REVIEW_PROJECT_CHANGED")?;
        if paired_key(workspace, &project)? != key {
            return Err("WEB_REVIEW_TARGET_CHANGED".into());
        }
        if WorkLedger::record_for_event(workspace, &project, &source_event)? != task_record {
            return Err("WEB_REVIEW_TASK_BINDING_CHANGED".into());
        }
        lease.check_target(&target)
    };
    guard()?;
    let mut value = match kind {
        ResponseKind::Sent => catdesk_wake::runtime::inspect_sent_task_response_diagnostics(
            &wake, &project, &target, &event_id, &guard,
        )?,
        ResponseKind::Historical => catdesk_wake::runtime::inspect_historical_task_response(
            &wake, &project, &target, &event_id, &guard,
        )?,
    };
    guard()?;
    value["action"] = json!(match kind {
        ResponseKind::Sent => "web_review_response_inspect",
        ResponseKind::Historical => "web_task_historical_response_inspect",
    });
    value["taskAttribution"] = task_attribution(&project, task_record);
    value["eventVerified"] = json!(false);
    value["reviewAuthorityGranted"] = json!(false);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_work_state::WorkTaskAssignment;

    fn args(verb: &str) -> Vec<String> {
        [
            "operator",
            "web",
            "review",
            verb,
            "--project-id",
            "alpha",
            "--event-id",
            "event-one",
        ]
        .map(String::from)
        .into()
    }

    #[test]
    fn only_exact_no_send_observation_verbs_are_intercepted() {
        assert_eq!(
            parse(&args("inspect-response")).unwrap().unwrap().kind,
            ResponseKind::Sent
        );
        assert_eq!(
            parse(&args("inspect-history-response"))
                .unwrap()
                .unwrap()
                .kind,
            ResponseKind::Historical
        );
        for verb in ["read", "reobserve", "reconcile", "inspect-trace", "send"] {
            assert!(parse(&args(verb)).unwrap().is_none());
        }
        for extra in [
            vec!["--event-id", "other"],
            vec!["--conversation-url", "https://chatgpt.com/c/other"],
            vec!["--retry"],
        ] {
            let mut invalid = args("inspect-response");
            invalid.extend(extra.into_iter().map(String::from));
            assert!(parse(&invalid).is_err());
        }
        let mut invalid = args("inspect-response");
        invalid[5] = "../other".into();
        assert!(parse(&invalid).is_err());
        invalid = args("inspect-response");
        invalid[7] = "../event".into();
        assert!(parse(&invalid).is_err());
    }

    fn record() -> WorkRecord {
        WorkRecord {
            request_id: "request-one".into(),
            event_id: "source-one".into(),
            message_sha256: "a".repeat(64),
            target: catdesk_wake::protocol::Target::new(1, "https://chatgpt.com/c/fixture".into())
                .unwrap(),
            phase: "OPEN".into(),
            outcome: None,
            summary_sha256: None,
            task_assignment: Some(WorkTaskAssignment {
                task_id: "task-one".into(),
                assignment_sha256: "b".repeat(64),
                scope_sha256: "c".repeat(64),
            }),
            reported_status: None,
        }
    }

    #[test]
    fn historical_observation_requires_exact_current_assignment_and_generation() {
        let mut original = record();
        assert!(
            validate_attribution(ResponseKind::Historical, Some(&original), &original.target)
                .is_ok()
        );
        assert_eq!(
            validate_attribution(ResponseKind::Historical, None, &original.target).unwrap_err(),
            "WEB_REVIEW_TASK_BINDING_REQUIRED"
        );
        let changed = catdesk_wake::protocol::Target::new(2, original.target.url.clone()).unwrap();
        assert_eq!(
            validate_attribution(ResponseKind::Historical, Some(&original), &changed).unwrap_err(),
            "WEB_REVIEW_TASK_BINDING_CHANGED"
        );
        original.task_assignment = None;
        assert_eq!(
            validate_attribution(ResponseKind::Historical, Some(&original), &original.target)
                .unwrap_err(),
            "WEB_REVIEW_TASK_BINDING_REQUIRED"
        );
        assert!(validate_attribution(ResponseKind::Sent, None, &original.target).is_ok());
    }

    #[test]
    fn attribution_never_exports_message_or_execution_authority() {
        let attribution = task_attribution("alpha", Some(record()));
        assert_eq!(attribution["taskId"], "task-one");
        assert_eq!(attribution["sourceEventId"], "source-one");
        assert_eq!(attribution["executionVerified"], false);
        assert!(attribution.get("messageSha256").is_none());
        assert!(attribution.get("target").is_none());
        assert!(attribution.get("reportedStatus").is_none());
        assert!(task_attribution("alpha", None).is_null());
    }
}
