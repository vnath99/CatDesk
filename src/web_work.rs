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

#[path = "web_review_work.rs"]
mod review;
pub(crate) use review::action as review_action;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AssignmentRequest {
    project_id: String,
    request_id: String,
    source_event_id: String,
    expected_target_generation: u64,
    expected_target_sha256: String,
    assignment_sha256: String,
}

impl AssignmentRequest {
    fn parse(args: Value) -> Result<Self, String> {
        let request: Self =
            serde_json::from_value(args).map_err(|_| "WEB_WORK_ARGUMENTS_INVALID")?;
        WorkRequest::parse(
            json!({"projectId":request.project_id,"requestId":request.request_id,
            "expectedTargetGeneration":request.expected_target_generation,
            "expectedTargetSha256":request.expected_target_sha256,"taskText":"Assignment admission"}),
        )?;
        if request.source_event_id
            != format!(
                "web-work-{}",
                protocol::digest(&format!("{}\0{}", request.project_id, request.request_id))
            )
            || request.assignment_sha256.len() != 64
            || !request
                .assignment_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("WEB_WORK_ARGUMENTS_INVALID".into());
        }
        Ok(request)
    }
}

/// Operator-only association of an explicitly opted-in, exact local assignment.
/// This neither publishes work nor starts its endpoint or isolated writer turn.
pub(crate) fn associate(
    workspace: &Path,
    args: Value,
    assignment_file: &Path,
) -> Result<Value, String> {
    assigned_action(
        workspace,
        args,
        AssignmentAction::Associate(assignment_file),
    )
}

pub(crate) fn resume_prepare(workspace: &Path, args: Value) -> Result<Value, String> {
    assigned_action(workspace, args, AssignmentAction::Prepare)
}

pub(crate) fn execution_permission(
    workspace: &Path,
    args: Value,
    revoke: bool,
) -> Result<Value, String> {
    assigned_action(workspace, args, AssignmentAction::Permission(revoke))
}

pub(crate) fn authorized_assignment(
    workspace: &Path,
    args: Value,
    key: &str,
) -> Result<Value, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let result = assigned_action(workspace, args.clone(), AssignmentAction::Authorize(key));
        if matches!(
            result.as_ref().map_err(String::as_str),
            Err("WEB_CONTEXT_REGISTRATION_BUSY" | "WEB_TARGET_WAKE_CONFIG_BUSY")
        ) && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        } else {
            return result;
        }
    }
}

#[derive(Clone, Copy)]
enum AssignmentAction<'a> {
    Associate(&'a Path),
    Prepare,
    Permission(bool),
    Authorize(&'a str),
}

fn assigned_action(
    workspace: &Path,
    args: Value,
    action: AssignmentAction<'_>,
) -> Result<Value, String> {
    let request = AssignmentRequest::parse(args)?;
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "WEB_WORK_CONTROL_UNAVAILABLE")?;
    resolve_registered_project_from_control_root(&workspace, &request.project_id)
        .map_err(|_| "WEB_WORK_PROJECT_UNAVAILABLE")?;
    let lease = crate::operator_facade::acquire_web_work_lease(&workspace, &request.project_id)?;
    mcp::with_current_project_web_target(
        &workspace,
        &request.project_id,
        request.expected_target_generation,
        &request.expected_target_sha256,
        |workspace, paired| {
            let current = protocol::Target::new(paired.generation, paired.url.clone())?;
            let scope =
                resolve_registered_project_from_control_root(workspace, &request.project_id)
                    .map_err(|_| "WEB_WORK_PROJECT_UNAVAILABLE")?;
            let ledger = WorkLedger::open(workspace, &request.project_id)?;
            let record = ledger
                .record(&request.request_id)?
                .ok_or("WEB_WORK_REQUEST_MISSING")?;
            if record.phase != "OPEN" || record.event_id != request.source_event_id {
                return Err("WEB_WORK_ASSIGNMENT_SOURCE_CHANGED".into());
            }
            lease.check_source_task(&record.target, &current, &record.event_id)?;
            require_assignment_source(
                workspace,
                &request.project_id,
                &record,
                !matches!(action, AssignmentAction::Associate(_)) && record.target == current,
            )?;
            if let AssignmentAction::Associate(path) = action {
                let link = crate::project_mcp_task::archive_work_assignment(
                    &scope,
                    path,
                    &request.assignment_sha256,
                    &request.request_id,
                    &request.source_event_id,
                )?;
                ledger.associate(&request.request_id, &request.source_event_id, &link)?;
                return Ok(json!({"status":"ASSOCIATED_ONLY","launchAuthorized":false,
                "projectId":request.project_id,"requestId":request.request_id,"sourceEventId":record.event_id,
                "taskAssignment":link,"nativeSessionAction":"NONE"}));
            }
            if record
                .task_assignment
                .as_ref()
                .is_none_or(|link| link.assignment_sha256 != request.assignment_sha256)
            {
                return Err("WEB_WORK_ASSIGNMENT_CONFLICT".into());
            }
            match action {
                AssignmentAction::Prepare => {
                    crate::project_mcp_task::prepare_work_resume(&scope, &record, &current)
                }
                AssignmentAction::Permission(revoke) => {
                    crate::project_mcp_task::grant_web_execution(&scope, &record, &current, revoke)
                }
                AssignmentAction::Authorize(key) => {
                    let archive =
                        crate::project_mcp_task::authorized_web_assignment(&scope, &record, key)?;
                    // Do not acquire the task writer during authorization: stale or
                    // mismatched caller identity must refuse before task startup.
                    Ok(
                        json!({"assignmentFile":archive,"taskId":record.task_assignment.as_ref().expect("checked").task_id}),
                    )
                }
                AssignmentAction::Associate(_) => unreachable!(),
            }
        },
    )
}

pub(crate) fn require_assignment_source(
    workspace: &Path,
    project: &str,
    record: &crate::web_work_state::WorkRecord,
    require_sent: bool,
) -> Result<(), String> {
    use catdesk_wake::store::{Phase, Store};
    let wake = Store::open_read_only(&wake_protocol_client::wake_root_for_workspace(workspace)?)?;
    let matches = |event: &protocol::Event| {
        event.event_id == record.event_id
            && event.project_id == project
            && event.event_type == "engineering_attention"
            && event.target_generation == record.target.generation
            && protocol::digest(&event.message) == record.message_sha256
    };
    if let Some(delivery) = wake.delivery(&record.event_id)? {
        if delivery.target != record.target
            || delivery.message_digest != record.message_sha256
            || !matches(&delivery.event)
        {
            return Err("WEB_WORK_SOURCE_PUBLICATION_REQUIRED".into());
        }
        if delivery.phase == Phase::Sent
            && delivery.target == record.target
            && delivery.message_digest == record.message_sha256
            && matches(&delivery.event)
            && delivery.receipt.as_ref().is_some_and(|receipt| {
                receipt.event_id == record.event_id
                    && receipt.target_generation == record.target.generation
                    && receipt.target_digest == record.target.digest
                    && receipt.message_digest == record.message_sha256
            })
        {
            return Ok(());
        }
    }
    if !require_sent && wake.events()?.iter().any(matches) {
        return Ok(());
    }
    Err(if require_sent {
        "WEB_WORK_SOURCE_SENT_REQUIRED"
    } else {
        "WEB_WORK_SOURCE_PUBLICATION_REQUIRED"
    }
    .into())
}

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
        ledger.require_ordinary_request(&request.request_id)?;
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
    #[serde(default)]
    reason_code: Option<String>,
}

pub(crate) fn resolve(workspace: &Path, args: Value) -> Result<Value, String> {
    use catdesk_wake::store::{Phase, ResponseState, Store};
    let resolution: Resolution =
        serde_json::from_value(args).map_err(|_| "WEB_WORK_ARGUMENTS_INVALID")?;
    // Share the same exact scope, digest and bounded text validation.
    let validated = WorkRequest::parse(json!({"projectId":resolution.project_id,
        "requestId":resolution.request_id,"expectedTargetGeneration":resolution.expected_target_generation,
        "expectedTargetSha256":resolution.expected_target_sha256,"taskText":resolution.summary}))?;
    let report = matches!(resolution.outcome.as_str(), "blocked" | "in_progress");
    if (!report && !matches!(resolution.outcome.as_str(), "completed" | "retired"))
        || (report
            && resolution
                .reason_code
                .as_deref()
                .is_none_or(|code| code.len() > 128 || !protocol::valid_id(code)))
        || (!report && resolution.reason_code.is_some())
    {
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
        if report && record.phase == "CLOSED" {
            return Err("WEB_WORK_REQUEST_CLOSED".into());
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
        let wake = if report {
            Store::open_read_only(&root)?
        } else {
            Store::open(&root)?
        };
        rollover_lease.check_target(
            wake.config()?
                .targets
                .get(&validated.project_id)
                .ok_or("WEB_WORK_PAIRED_TARGET_REQUIRED")?,
        )?;
        if report {
            // Explicit coordinator metadata may describe a pre-delivery blocker.
            // It cannot close ownership, retire a queue item, execute or send work.
            return ledger.report(
                &validated.request_id,
                &record.target,
                &resolution.outcome,
                resolution
                    .reason_code
                    .as_deref()
                    .expect("validated report reason"),
                &validated.task_text,
            );
        }
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
                || !(delivery.event.event_type == "engineering_attention"
                    || (delivery.event.event_type == "source_review"
                        && ledger
                            .review_dispatch_recorded(&record, &delivery.event.audit_references)?))
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
pub(crate) mod tests {
    use super::*;

    #[cfg(windows)]
    pub(crate) fn assignment_fixture_for(
        id: &str,
    ) -> (
        std::path::PathBuf,
        std::path::PathBuf,
        catdesk_wake::store::Store,
        Value,
        protocol::Event,
    ) {
        use crate::delegated::autonomy_projects::{
            AutonomousProjectRegistryV1, AutonomousProjectV1,
        };
        use std::{fs, process::Command};
        let (scope, _, assignment) = crate::project_mcp_task::tests::fixture(id);
        let original_registry = fs::read(
            scope
                .control_workspace
                .join(".catdesk/projects/projects.json"),
        )
        .unwrap();
        let control = assignment.parent().unwrap().join("runtime-control");
        fs::create_dir(&control).unwrap();
        let git = |root: &Path, args: &[&str]| {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(root)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        };
        git(
            &scope.project.workspace,
            &["remote", "add", "origin", &scope.project.git_identity],
        );
        let parent = assignment.parent().unwrap();
        let package = parent.join("package");
        let python = parent.join("python");
        fs::create_dir(&package).unwrap();
        fs::create_dir(&python).unwrap();
        for (name, bytes) in [
            ("CatDeskWakeHost.exe", b"inert fixture host".as_slice()),
            ("CatDeskBinagotchy.exe", b"inert fixture GUI".as_slice()),
            (
                "adapter.py",
                include_bytes!("../wake/adapter.py").as_slice(),
            ),
            (
                "wake_bridge.py",
                include_bytes!("../scripts/wake_bridge.py").as_slice(),
            ),
        ] {
            fs::write(package.join(name), bytes).unwrap();
        }
        fs::write(python.join("python.exe"), b"inert fixture Python").unwrap();
        fs::write(python.join("__init__.py"), b"").unwrap();
        catdesk_wake::development_runtime::prepare(&control, &package, &python).unwrap();
        git(&control, &["init", "-q"]);
        git(
            &control,
            &[
                "remote",
                "add",
                "origin",
                "https://example.test/catdesk.git",
            ],
        );
        let control = control.canonicalize().unwrap();
        let wake = catdesk_wake::store::Store::open(
            &wake_protocol_client::wake_root_for_workspace(&control).unwrap(),
        )
        .unwrap();
        let target = wake
            .set_target(id, 0, &format!("https://chatgpt.com/c/assignment-{id}"))
            .unwrap();
        let registry_path = control.join(".catdesk/projects/projects.json");
        fs::create_dir_all(registry_path.parent().unwrap()).unwrap();
        let mut registry: AutonomousProjectRegistryV1 =
            serde_json::from_slice(&original_registry).unwrap();
        registry.projects[0].chatgpt_target_url = Some(target.url.clone());
        registry.projects[0].chatgpt_target_sha256 = Some(target.digest.clone());
        registry.projects.push(AutonomousProjectV1 {
            project_id: "catdesk".into(),
            workspace: control.clone(),
            git_identity: "https://example.test/catdesk.git".into(),
            verification_profile: "rust_full".into(),
            codex_thread_id: None,
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
            chatgpt_target_history: vec![],
        });
        fs::write(registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();
        let event_id = format!("web-work-{}", protocol::digest(&format!("{id}\0request-a")));
        let mut value: Value = serde_json::from_slice(&fs::read(&assignment).unwrap()).unwrap();
        value["webWork"] = json!({"requestId":"request-a","eventId":event_id});
        fs::write(&assignment, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        let event = protocol::Event {
            schema_version: 1,
            event_id: event_id.clone(),
            project_id: id.into(),
            event_type: "engineering_attention".into(),
            created_utc: now(),
            message: "Controlled assigned-work source".into(),
            target_generation: target.generation,
            audit_references: vec![],
        };
        WorkLedger::open(&control, id)
            .unwrap()
            .prepare(
                "request-a",
                &event_id,
                &protocol::digest(&event.message),
                &target,
            )
            .unwrap();
        let args = json!({"projectId":id,"requestId":"request-a","sourceEventId":event_id,"expectedTargetGeneration":target.generation,"expectedTargetSha256":target.digest,"assignmentSha256":protocol::digest(&fs::read_to_string(&assignment).unwrap())});
        (control, assignment, wake, args, event)
    }

    #[cfg(windows)]
    fn assignment_fixture() -> (
        std::path::PathBuf,
        std::path::PathBuf,
        catdesk_wake::store::Store,
        Value,
        protocol::Event,
    ) {
        assignment_fixture_for("alpha")
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn operator_assignment_requires_publication_then_exact_sent_source_and_never_launches() {
        use catdesk_wake::store::{Phase, Receipt};
        let (control, assignment, wake, args, event) = assignment_fixture();
        assert_eq!(
            associate(&control, args.clone(), &assignment).unwrap_err(),
            "WEB_WORK_SOURCE_PUBLICATION_REQUIRED"
        );
        assert!(!control.join(".catdesk/project-mcp-tasks").exists());
        wake.publish(&event).unwrap();
        let associated = associate(&control, args.clone(), &assignment).unwrap();
        assert_eq!(associated["status"], "ASSOCIATED_ONLY");
        assert_eq!(associated["launchAuthorized"], false);
        assert_eq!(
            resume_prepare(&control, args.clone()).unwrap_err(),
            "WEB_WORK_SOURCE_SENT_REQUIRED"
        );
        let host = wake.host_lock().unwrap();
        let claimed = wake.claim(&event, &host).unwrap();
        wake.transition(
            &event.event_id,
            &claimed.owner,
            Phase::Submitting,
            None,
            None,
        )
        .unwrap();
        let target = wake.config().unwrap().targets["alpha"].clone();
        let receipt = Receipt {
            schema_version: 1,
            event_id: event.event_id.clone(),
            target_generation: target.generation,
            target_digest: target.digest,
            message_digest: protocol::digest(&event.message),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        wake.record_exact_receipt_timer(&event.event_id, &receipt)
            .unwrap();
        wake.transition(
            &event.event_id,
            &claimed.owner,
            Phase::Sent,
            Some(receipt),
            None,
        )
        .unwrap();
        drop(host);
        let prepared = resume_prepare(&control, args.clone()).unwrap();
        assert_eq!(prepared["status"], "PREPARED_ONLY");
        assert_eq!(prepared["launchAuthorized"], false);
        let scope = resolve_registered_project_from_control_root(&control, "alpha").unwrap();
        let task = crate::project_mcp_task::ProjectTask::open_with_budget(&scope, &assignment, 60)
            .unwrap();
        assert_eq!(
            resume_prepare(&control, args.clone()).unwrap_err(),
            "WEB_ROLLOVER_PROJECT_BUSY"
        );
        let read = json!({"taskId":"task-fixture","callId":"read-once","tool":"read","arguments":{"path":"src/lib.rs"}});
        let registry_path = control.join(".catdesk/projects/projects.json");
        let registration = std::fs::read(&registry_path).unwrap();
        for (key, value) in [
            ("codexThreadId", json!("other-native-owner")),
            ("verificationProfile", json!("web_only_v1")),
        ] {
            let mut changed: Value = serde_json::from_slice(&registration).unwrap();
            changed["projects"][0][key] = value;
            std::fs::write(&registry_path, serde_json::to_vec(&changed).unwrap()).unwrap();
            assert_eq!(
                task.execute(read.clone()).await.unwrap_err(),
                "PROJECT_TASK_REGISTRATION_CHANGED"
            );
            std::fs::write(&registry_path, &registration).unwrap();
        }
        task.execute(read.clone()).await.unwrap();
        task.close().await.unwrap();
        let prepared_again = resume_prepare(&control, args).unwrap();
        assert_eq!(
            prepared_again["assignmentSha256"],
            prepared["assignmentSha256"]
        );
        assert_eq!(prepared_again["journaledCallCount"], 1);
        let reopened =
            crate::project_mcp_task::ProjectTask::open_with_budget(&scope, &assignment, 60)
                .unwrap();
        assert_eq!(
            reopened.execute(read.clone()).await.unwrap_err(),
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        let old = wake.config().unwrap().targets["alpha"].clone();
        // Same-URL set_target is intentionally idempotent. A supported URL
        // round trip produces a genuinely newer generation with that same URL.
        let intermediate = wake
            .set_target(
                "alpha",
                old.generation,
                "https://chatgpt.com/c/assignment-intermediate",
            )
            .unwrap();
        wake.set_target("alpha", intermediate.generation, &old.url)
            .unwrap();
        assert_eq!(
            reopened.execute(read).await.unwrap_err(),
            "WEB_WORK_CONTINUATION_SOURCE_REQUIRED"
        );
        reopened.close().await.unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn same_url_generation_change_and_native_owner_change_refuse_without_rebinding_work() {
        use std::fs;
        let (control, assignment, wake, args, event) = assignment_fixture();
        wake.publish(&event).unwrap();
        associate(&control, args.clone(), &assignment).unwrap();
        let ledger = WorkLedger::open(&control, "alpha").unwrap();
        let original = ledger.record("request-a").unwrap().unwrap();
        let old = wake.config().unwrap().targets["alpha"].clone();
        let intermediate = wake
            .set_target(
                "alpha",
                old.generation,
                "https://chatgpt.com/c/assignment-intermediate",
            )
            .unwrap();
        wake.set_target("alpha", intermediate.generation, &old.url)
            .unwrap();
        assert_eq!(
            resume_prepare(&control, args.clone()).unwrap_err(),
            "WEB_CONTEXT_TARGET_CHANGED"
        );
        assert_eq!(ledger.record("request-a").unwrap().unwrap(), original);
        let mut refreshed = args;
        refreshed["expectedTargetGeneration"] = json!(intermediate.generation + 1);
        assert_eq!(
            resume_prepare(&control, refreshed).unwrap_err(),
            "WEB_WORK_CONTINUATION_SOURCE_REQUIRED"
        );
        let path = control.join(".catdesk/projects/projects.json");
        let mut registry: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        registry["projects"][0]["codexThreadId"] = json!("new-native-owner");
        fs::write(path, serde_json::to_vec(&registry).unwrap()).unwrap();
        assert_eq!(ledger.record("request-a").unwrap().unwrap(), original);
    }

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
