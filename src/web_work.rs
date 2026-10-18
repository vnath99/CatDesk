//! Central ChatGPT work requests use the existing durable, paired Wake queue.
//! Publication neither claims a native writer nor proves task completion.

use crate::{
    delegated::autonomy_projects::{
        AutonomousProjectRegistryStoreV1, resolve_registered_project_from_control_root,
    },
    mcp, wake_protocol_client,
    web_work_state::WorkLedger,
};
use catdesk_wake::{protocol, store::now};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkRequest {
    project_id: String,
    request_id: String,
    expected_target_generation: u64,
    expected_target_sha256: String,
    task_text: String,
}

impl WorkRequest {
    fn parse(args: Value) -> Result<Self, String> {
        let mut request: Self =
            serde_json::from_value(args).map_err(|_| "WEB_WORK_ARGUMENTS_INVALID")?;
        if [&request.project_id, &request.request_id]
            .iter()
            .any(|id| id.len() > 128 || !protocol::valid_id(id))
            || request.expected_target_generation == 0
            || request.expected_target_sha256.len() != 64
            || !request
                .expected_target_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || request.task_text.is_empty()
            || request.task_text.len() > 1024
            || request
                .task_text
                .chars()
                .any(|value| value.is_control() && !matches!(value, '\n' | '\r' | '\t'))
        {
            return Err("WEB_WORK_ARGUMENTS_INVALID".into());
        }
        request.task_text = request
            .task_text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if request.task_text.is_empty() {
            return Err("WEB_WORK_ARGUMENTS_INVALID".into());
        }
        Ok(request)
    }

    fn event_id(&self) -> String {
        format!(
            "web-work-{}",
            protocol::digest(&format!("{}\0{}", self.project_id, self.request_id))
        )
    }

    fn message(&self) -> Result<String, String> {
        let message = format!(
            "CatDesk central work request {} for project {}. Requested task: {} Review current project state and existing task ownership first. Continue only work assigned to this conversation; preserve ongoing Codex sessions and avoid duplicate execution. Report completed artifacts, verification, and remaining blockers. Browser response completion alone is not project task completion.",
            self.request_id, self.project_id, self.task_text
        );
        if message.len() > 2048 {
            return Err("WEB_WORK_MESSAGE_TOO_LARGE".into());
        }
        Ok(message)
    }
}

pub(crate) fn request(workspace: &Path, args: Value) -> Result<Value, String> {
    // Validate all caller input before any state creation or publication.
    let request = WorkRequest::parse(args)?;
    let message = request.message()?;
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "WEB_WORK_CONTROL_UNAVAILABLE")?;
    resolve_registered_project_from_control_root(&workspace, &request.project_id)
        .map_err(|_| "WEB_WORK_PROJECT_UNAVAILABLE")?;
    // Match the existing rollover -> paired target -> registry lock order,
    // retaining the project lease until publication and intent persistence end.
    let rollover_lease =
        crate::operator_facade::acquire_web_work_lease(&workspace, &request.project_id)?;
    let paired = mcp::operator_read_project_web_target(&workspace, &request.project_id)
        .map_err(|_| "WEB_WORK_PAIRED_TARGET_REQUIRED")?
        .ok_or("WEB_WORK_PAIRED_TARGET_REQUIRED")?;
    if paired.generation != request.expected_target_generation
        || paired.sha256 != request.expected_target_sha256
    {
        return Err("WEB_WORK_TARGET_CHANGED".into());
    }
    let expected = protocol::Target::new(paired.generation, paired.url)?;
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
            .map_err(|_| "WEB_WORK_REGISTRY_UNAVAILABLE")?;
    let event_id = request.event_id();
    // Read the paired target before taking the registry lock: paired binding
    // uses the global target mutex followed by this registry lock. We then
    // recheck the registration under its lock and let Wake CAS the exact target
    // and generation atomically with publication. Never reverse that lock order.
    registry.with_registration_lock(|| {
        let scope = resolve_registered_project_from_control_root(&workspace, &request.project_id)
            .map_err(|_| "WEB_WORK_PROJECT_UNAVAILABLE")?;
        if scope.project.chatgpt_target_url.as_deref() != Some(expected.url.as_str())
            || scope.project.chatgpt_target_sha256.as_deref() != Some(expected.digest.as_str())
        {
            return Err("WEB_WORK_TARGET_CHANGED".into());
        }
        rollover_lease.check_target(&expected)?;
        let ledger = WorkLedger::open(&workspace, &request.project_id)?;
        let phase = ledger.prepare(&request.request_id, &event_id, &protocol::digest(&message), &expected)?;
        if phase == "IDEMPOTENT_CLOSED" {
            return Ok(json!({"action":"web_work_request","status":"REQUEST_ALREADY_CLOSED",
                "projectId":request.project_id,"requestId":request.request_id,"eventId":event_id,
                "nativeSessionAction":"NONE","deliveryProved":false,"taskCompletionVerified":false}));
        }
        // The existing publisher verifies activation, the current generation's
        // acceptance cutoff and development/installed root isolation. The Wake
        // event is the durable transport record; the OPEN intent is saved first
        // so publication failure can only resume this same request. A distinct
        // task cannot replace it before explicit resolution.
        let event = wake_protocol_client::publish_registered_invitation(
            &workspace,
            &request.project_id,
            &event_id,
            now(),
            &message,
            &expected,
        )?;
        Ok(json!({
        "action":"web_work_request", "status":"REQUEST_RECORDED",
        "projectId":request.project_id, "requestId":request.request_id,
        "eventId":event.event_id, "createdUtc":event.created_utc,
        "targetGeneration":event.target_generation, "targetSha256":expected.digest,
        "taskTextSha256":protocol::digest(&request.task_text),
        "nativeSessionAction":"NONE", "deliveryProved":false,
        "naturalReviewProved":false, "taskCompletionVerified":false
        }))
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Resolution {
    project_id: String,
    request_id: String,
    expected_target_generation: u64,
    expected_target_sha256: String,
    outcome: String,
    summary: String,
}

pub(crate) fn resolve(workspace: &Path, args: Value) -> Result<Value, String> {
    use catdesk_wake::store::{Phase, ResponseState, Store};
    let resolution: Resolution =
        serde_json::from_value(args).map_err(|_| "WEB_WORK_ARGUMENTS_INVALID")?;
    // Share the same exact scope, digest and bounded text validation.
    let validated = WorkRequest::parse(json!({"projectId":resolution.project_id,
        "requestId":resolution.request_id,"expectedTargetGeneration":resolution.expected_target_generation,
        "expectedTargetSha256":resolution.expected_target_sha256,"taskText":resolution.summary}))?;
    if !matches!(resolution.outcome.as_str(), "completed" | "retired") {
        return Err("WEB_WORK_ARGUMENTS_INVALID".into());
    }
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "WEB_WORK_CONTROL_UNAVAILABLE")?;
    resolve_registered_project_from_control_root(&workspace, &validated.project_id)
        .map_err(|_| "WEB_WORK_PROJECT_UNAVAILABLE")?;
    let rollover_lease =
        crate::operator_facade::acquire_web_work_lease(&workspace, &validated.project_id)?;
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
            .map_err(|_| "WEB_WORK_REGISTRY_UNAVAILABLE")?;
    registry.with_registration_lock(|| {
        resolve_registered_project_from_control_root(&workspace, &validated.project_id)
            .map_err(|_| "WEB_WORK_PROJECT_UNAVAILABLE")?;
        let ledger = WorkLedger::open(&workspace, &validated.project_id)?;
        let record = ledger
            .record(&validated.request_id)?
            .ok_or("WEB_WORK_REQUEST_MISSING")?;
        if record.target.generation != validated.expected_target_generation
            || record.target.digest != validated.expected_target_sha256
        {
            return Err("WEB_WORK_TARGET_CHANGED".into());
        }
        // Replays of a durable closure never publish or reinterpret task state.
        if record.phase == "CLOSED" {
            return ledger.resolve(
                &validated.request_id,
                &record.target,
                &resolution.outcome,
                &validated.task_text,
            );
        }
        let root = wake_protocol_client::wake_root_for_workspace(&workspace)?;
        let wake = Store::open(&root)?;
        rollover_lease.check_target(
            wake.config()?
                .targets
                .get(&validated.project_id)
                .ok_or("WEB_WORK_PAIRED_TARGET_REQUIRED")?,
        )?;
        let delivery = wake.delivery(&record.event_id)?;
        if resolution.outcome == "retired"
            && delivery
                .as_ref()
                .is_none_or(|delivery| delivery.phase != Phase::Sent)
        {
            // This existing transaction archives only a known pre-submit event.
            // SUBMITTING and SENT cannot be cleared through this path.
            wake.retire_stale_expected(
                &record.event_id,
                &validated.project_id,
                &record.target,
                &record.message_sha256,
            )?;
        } else {
            let delivery = delivery.ok_or("WEB_WORK_DELIVERY_REQUIRED")?;
            let timer = wake
                .timer(&record.event_id)?
                .ok_or("WEB_WORK_COMPLETED_RESPONSE_REQUIRED")?;
            if delivery.phase != Phase::Sent
                || delivery.receipt.is_none()
                || delivery.target != record.target
                || delivery.event.project_id != validated.project_id
                || delivery.event.event_type != "engineering_attention"
                || delivery.message_digest != record.message_sha256
                || timer.event_id != record.event_id
                || timer.project_id != validated.project_id
                || timer.target_generation != record.target.generation
                || timer.target_digest != record.target.digest
                || timer.response_state != ResponseState::Complete
                || timer.completed_utc.is_none()
            {
                return Err("WEB_WORK_COMPLETED_RESPONSE_REQUIRED".into());
            }
        }
        // A completed browser turn is a transport prerequisite. The outcome and
        // summary remain an explicit coordinator report, not verified artifacts.
        ledger.resolve(
            &validated.request_id,
            &record.target,
            &resolution.outcome,
            &validated.task_text,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> Value {
        json!({"projectId":"alpha","requestId":"request-1",
            "expectedTargetGeneration":1,"expectedTargetSha256":"a".repeat(64),
            "taskText":"Review the existing progress and continue the assigned test."})
    }

    #[test]
    fn bounded_request_rejects_extra_fields_and_unsafe_scope_before_state_access() {
        for (key, value) in [
            ("workspace", json!("C:/other")),
            ("conversationUrl", json!("https://chatgpt.com/c/other")),
            ("projectId", json!("../other")),
            ("requestId", json!("")),
            ("expectedTargetGeneration", json!(0)),
            ("expectedTargetSha256", json!("A".repeat(64))),
            ("taskText", json!(" ")),
            ("taskText", json!("x\0y")),
            ("taskText", json!("é".repeat(513))),
        ] {
            let mut input = args();
            input[key] = value;
            assert_eq!(
                WorkRequest::parse(input).err().unwrap(),
                "WEB_WORK_ARGUMENTS_INVALID"
            );
        }
        let mut missing = args();
        missing.as_object_mut().unwrap().remove("requestId");
        assert!(WorkRequest::parse(missing).is_err());
    }

    #[test]
    fn durable_request_identity_is_project_scoped_and_text_stays_bounded() {
        let first = WorkRequest::parse(args()).unwrap();
        let same = WorkRequest::parse(args()).unwrap();
        assert_eq!(first.event_id(), same.event_id());
        let mut other_args = args();
        other_args["projectId"] = json!("beta");
        let other = WorkRequest::parse(other_args).unwrap();
        assert_ne!(first.event_id(), other.event_id());
        let mut largest = args();
        largest["projectId"] = json!("p".repeat(128));
        largest["requestId"] = json!("r".repeat(128));
        largest["taskText"] = json!("x".repeat(1024));
        let largest = WorkRequest::parse(largest).unwrap();
        assert!(largest.message().unwrap().len() <= 2048);
        assert!(protocol::valid_id(&largest.event_id()));
        let mut whitespace = args();
        whitespace["taskText"] = json!("Inspect\n  existing\twork.");
        assert_eq!(
            WorkRequest::parse(whitespace).unwrap().task_text,
            "Inspect existing work."
        );
    }
}
