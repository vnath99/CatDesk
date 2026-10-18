//! Small periodic driver for the existing exact-target Web operator steps.
//! One project can wait on its browser without holding up another project.

use super::{capture_context, paired_key, rollover};
use crate::{
    delegated::autonomy_projects::AutonomousProjectRegistryStoreV1,
    state::SharedState,
    web_conversation_context::{ConversationKey, PressureStatus, WebContextStore},
    web_work_state::WorkLedger,
};
use catdesk_wake::{
    protocol::{Target, valid_id},
    runtime,
    store::{Phase, ResponseState, Store, read},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

const AUTO_INTERVAL: Duration = Duration::from_secs(120);
const MAX_ACTIVE_PROJECTS: usize = 8;

fn pressure_due(pressure: &PressureStatus) -> bool {
    pressure.is_current
        && pressure.automatic_rollover_enabled
        && (pressure.near_capacity
            || (pressure.incomplete_history_rollover
                && pressure.browser_coverage.observed
                && pressure.pressure_assessment == "UNKNOWN_OR_INCOMPLETE"))
}

fn completed_task_anchor(
    wake: &Store,
    project: &str,
    source: &Target,
) -> Result<Option<String>, String> {
    let mut latest = None;
    let timers = wake.timers()?;
    if timers.len() > 10_000 {
        return Err("WEB_AUTO_TIMER_LIMIT".into());
    }
    for timer in timers {
        if timer.project_id != project
            || timer.target_generation != source.generation
            || timer.target_digest != source.digest
            || timer.response_state != ResponseState::Complete
            || timer.completed_utc.is_none()
            || timer.event_id.starts_with("web-handoff-")
            || timer.event_id.starts_with("web-continue-")
        {
            continue;
        }
        let Some(delivery) = wake.delivery(&timer.event_id)? else {
            continue;
        };
        if delivery.phase != Phase::Sent
            || delivery.target != *source
            || delivery.receipt.is_none()
            || delivery.event.event_id != timer.event_id
            || delivery.event.project_id != project
            || delivery.event.target_generation != source.generation
        {
            continue;
        }
        let candidate = (delivery.event.created_utc, delivery.event.event_id.clone());
        if latest.as_ref().is_none_or(|current| candidate > *current) {
            latest = Some(candidate);
        }
    }
    Ok(latest.map(|(_, event_id)| event_id))
}

pub(super) fn source_task_anchor(
    workspace: &Path,
    wake: &Store,
    project: &str,
    source: &Target,
    key: &ConversationKey,
    pressure: &PressureStatus,
) -> Result<Option<String>, String> {
    let work = WorkLedger::status(workspace, project);
    match work["status"].as_str() {
        Some("UNOBSERVED" | "DURABLE_SNAPSHOT") => {}
        _ => return Err("WEB_AUTO_WORK_INTENT_UNAVAILABLE".into()),
    }
    if !work["active"].is_null() {
        let event_id = work["active"]["eventId"]
            .as_str()
            .filter(|id| valid_id(id))
            .ok_or("WEB_AUTO_WORK_INTENT_INVALID")?;
        let generation = work["active"]["targetGeneration"]
            .as_u64()
            .ok_or("WEB_AUTO_WORK_INTENT_INVALID")?;
        let target_digest = work["active"]["targetSha256"]
            .as_str()
            .ok_or("WEB_AUTO_WORK_INTENT_INVALID")?;
        let message_digest = work["active"]["messageSha256"]
            .as_str()
            .ok_or("WEB_AUTO_WORK_INTENT_INVALID")?;
        if generation != source.generation || target_digest != source.digest {
            return match rollover::completed_continuation_task_anchor(
                workspace, wake, project, source,
            )? {
                Some(task_id) if task_id == event_id => Ok(Some(task_id)),
                Some(_) => Err("WEB_AUTO_WORK_INTENT_LINEAGE_CHANGED".into()),
                None => Ok(None),
            };
        }
        let Some(delivery) = wake.delivery(event_id)? else {
            return Ok(None);
        };
        let Some(timer) = wake.timer(event_id)? else {
            return Ok(None);
        };
        if delivery.phase != Phase::Sent
            || delivery.receipt.is_none()
            || delivery.event.event_id != event_id
            || delivery.event.project_id != project
            || delivery.event.event_type != "engineering_attention"
            || delivery.event.target_generation != source.generation
            || delivery.target != *source
            || delivery.message_digest != message_digest
            || timer.event_id != event_id
            || timer.project_id != project
            || timer.target_generation != source.generation
            || timer.target_digest != source.digest
            || timer.response_state != ResponseState::Complete
            || timer.completed_utc.is_none()
        {
            return Ok(None);
        }
        return Ok(Some(event_id.into()));
    }
    if let Some(intent) = &pressure.rollover_intent
        && intent.key == *key
        && valid_id(&intent.source_task_id)
    {
        return Ok(Some(intent.source_task_id.clone()));
    }
    if let Some(task_id) = completed_task_anchor(wake, project, source)? {
        return Ok(Some(task_id));
    }
    rollover::completed_continuation_task_anchor(workspace, wake, project, source)
}

fn is_pending_handoff(reason: &str) -> bool {
    matches!(
        reason,
        "WEB_HANDOFF_EXACT_USER_RECEIPT_REQUIRED" | "WEB_HANDOFF_COMPLETED_TIMER_REQUIRED"
    )
}

pub(super) fn running_wake_state(
    wake: &Store,
    project: &str,
    source: &Target,
) -> Result<&'static str, String> {
    let control: runtime::Control = read(&wake.root().join("control.json"))?;
    if control.schema_version != 1 {
        return Err("WEB_AUTO_WAKE_CONTROL_INVALID".into());
    }
    match control.desired.as_str() {
        "STOPPED" => return Ok("WAKE_STOPPED"),
        "PAUSED" => return Ok("WAKE_PAUSED"),
        "RUNNING" => {}
        _ => return Err("WEB_AUTO_WAKE_CONTROL_INVALID".into()),
    }
    let status = runtime::status(wake)?;
    if status.host != "RUNNING" || status.pid == 0 {
        return Ok("WAKE_HOST_NOT_RUNNING");
    }
    if status.targets.get(project) != Some(source) {
        return Ok("WAKE_TARGET_CHANGED");
    }
    Ok("RUNNING")
}

fn background_observation_enabled(workspace: &Path, key: &ConversationKey) -> Result<bool, String> {
    let root = workspace.join(".catdesk/web-context");
    if !root.exists() {
        return Ok(false);
    }
    let meter = WebContextStore::open(root)?;
    if meter.active_key(&key.project_id)?.as_ref() != Some(key) {
        return Ok(false);
    }
    let pressure = meter.pressure(key)?;
    Ok(pressure.automatic_observation_enabled || pressure.automatic_rollover_enabled)
}

fn gated_browser_step<F>(
    wake_state: &'static str,
    action: F,
) -> Result<(&'static str, Value), String>
where
    F: FnOnce() -> Result<(&'static str, Value), String>,
{
    if wake_state != "RUNNING" {
        return Ok((wake_state, json!({"browserAction":"NONE"})));
    }
    action()
}

fn execute_after_live_recheck_with(
    wake: &Store,
    project: &str,
    source: &Target,
    execute: impl FnOnce() -> Result<Value, String>,
) -> Result<(&'static str, Value), String> {
    let state = running_wake_state(wake, project, source)?;
    if state != "RUNNING" {
        return Ok((
            state,
            json!({"reason":"WAKE_CONTROL_CHANGED","browserAction":"NONE"}),
        ));
    }
    let result = execute()?;
    let stage = if result["phase"] == "RESPONSE_PENDING" {
        "AWAITING_RESPONSE"
    } else {
        "ROLLOVER_ADVANCED"
    };
    Ok((stage, result))
}

fn execute_after_live_recheck(
    workspace: &Path,
    project: &str,
    source: &Target,
) -> Result<(&'static str, Value), String> {
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let wake = Store::open(&root)?;
    execute_after_live_recheck_with(&wake, project, source, || {
        rollover::execute_automatic(workspace, project)
    })
}

fn error_stage(reason: &str) -> &'static str {
    if reason == "WAKE_STOPPED" {
        "WAKE_STOPPED"
    } else if reason == "WAKE_PAUSED" {
        "WAKE_PAUSED"
    } else if reason == "WAKE_HOST_NOT_RUNNING" {
        "WAKE_HOST_NOT_RUNNING"
    } else if reason == "WAKE_TARGET_CHANGED" {
        "WAKE_TARGET_CHANGED"
    } else if reason == "BROWSER_SESSION_BUSY" {
        "AWAITING_BROWSER"
    } else if matches!(
        reason,
        "SOURCE_SNAPSHOT_SCOPE_INVALID"
            | "BOOTSTRAP_SOURCE_QUEUE_PENDING"
            | "BOOTSTRAP_SOURCE_TURN_PENDING"
            | "CHATGPT_NOT_IDLE"
    ) {
        "AWAITING_QUIESCENCE"
    } else {
        "ATTENTION"
    }
}

fn run_step(
    workspace: &Path,
    project: &str,
    key: &ConversationKey,
    source: &Target,
) -> Result<(&'static str, Value), String> {
    match rollover::auto_stage(workspace, project, source)? {
        rollover::AutoStage::SourceWitnessRequired => Ok((
            "SOURCE_WITNESS_REQUIRED",
            json!({"reason":"WEB_AUTO_SOURCE_WITNESS_REQUIRED"}),
        )),
        rollover::AutoStage::SubmissionUnknown => Ok((
            "SUBMISSION_UNKNOWN",
            json!({"reason":"WEB_BOOTSTRAP_OUTCOME_UNKNOWN_NO_RESEND"}),
        )),
        rollover::AutoStage::Quarantined => Ok((
            "QUARANTINED",
            json!({"reason":"WEB_ROLLOVER_QUARANTINED_NO_REPLAY"}),
        )),
        rollover::AutoStage::Execute => execute_after_live_recheck(workspace, project, source),
        rollover::AutoStage::CollectHandoff => {
            // Crash recovery may find the request saved before queue
            // publication. Republishing the same event ID is idempotent and
            // cannot resend a submitted browser turn.
            rollover::resume_saved_handoff_request(workspace, project)?;
            match rollover::collect_handoff(workspace, project) {
                Ok(_) => execute_after_live_recheck(workspace, project, source),
                Err(reason) if is_pending_handoff(&reason) => {
                    Ok(("AWAITING_HANDOFF", json!({"reason":reason})))
                }
                Err(reason) => Err(reason),
            }
        }
        rollover::AutoStage::Observe => {
            let observed = capture_context(workspace, project)?;
            let meter = WebContextStore::open(workspace.join(".catdesk/web-context"))?;
            let pressure = meter.pressure(key)?;
            if !pressure_due(&pressure) {
                return Ok(("OBSERVED", observed));
            }
            let wake = Store::open(&crate::wake_protocol_client::wake_root_for_workspace(
                workspace,
            )?)?;
            if running_wake_state(&wake, project, source)? != "RUNNING" {
                return Ok(("WAKE_STOPPED", json!({"reason":"WAKE_CONTROL_CHANGED"})));
            }
            let Some(task_id) =
                source_task_anchor(workspace, &wake, project, source, key, &pressure)?
            else {
                return Ok((
                    "TASK_ANCHOR_REQUIRED",
                    json!({"reason":"WEB_AUTO_COMPLETED_WAKE_TASK_REQUIRED"}),
                ));
            };
            let request = rollover::request_handoff(workspace, project, &task_id)?;
            Ok(("HANDOFF_REQUESTED", request))
        }
    }
}

fn tick_project_mode(workspace: &Path, project: &str, background: bool) -> Result<Value, String> {
    let (key, source) = match paired_key(workspace, project) {
        Ok(key) => {
            let source = Target::new(key.target_generation, key.conversation_url.clone())?;
            (key, source)
        }
        Err(reason) => match rollover::automatic_recovery_authority(workspace, project) {
            Ok(Some(authority)) => authority,
            Ok(None) | Err(_) => {
                let _ = rollover::record_auto_status(
                    workspace,
                    project,
                    "ATTENTION",
                    None,
                    Some("WEB_AUTO_PAIRED_TARGET_REQUIRED"),
                );
                return Err(reason);
            }
        },
    };
    let selected_stage =
        rollover::auto_stage(workspace, project, &source).inspect_err(|reason| {
            let _ = rollover::record_auto_status(
                workspace,
                project,
                "ATTENTION",
                Some(source.generation),
                Some(reason),
            );
        })?;
    if background
        && selected_stage == rollover::AutoStage::Observe
        && !background_observation_enabled(workspace, &key)?
    {
        rollover::record_auto_status(
            workspace,
            project,
            "OBSERVATION_DISABLED",
            Some(source.generation),
            None,
        )?;
        return Ok(
            json!({"action":"web_auto_tick","projectId":project,"stage":"OBSERVATION_DISABLED"}),
        );
    }
    rollover::record_auto_status(
        workspace,
        project,
        "IN_PROGRESS",
        Some(source.generation),
        None,
    )?;
    let result = (|| {
        let wake = Store::open(&crate::wake_protocol_client::wake_root_for_workspace(
            workspace,
        )?)?;
        let wake_state = running_wake_state(&wake, project, &source)?;
        if wake_state == "RUNNING" {
            catdesk_wake::development_runtime::validate_selected_target_isolation(
                workspace,
                wake.root(),
                &source.url,
            )?;
        }
        gated_browser_step(wake_state, || run_step(workspace, project, &key, &source))
    })();
    match result {
        Ok((stage, value)) => {
            rollover::record_auto_status(workspace, project, stage, Some(source.generation), None)?;
            Ok(json!({"action":"web_auto_tick","projectId":project,"stage":stage,"result":value}))
        }
        Err(reason) => {
            let stage = error_stage(&reason);
            let _ = rollover::record_auto_status(
                workspace,
                project,
                stage,
                Some(source.generation),
                Some(&reason),
            );
            Err(reason)
        }
    }
}

pub(super) fn tick_project(workspace: &Path, project: &str) -> Result<Value, String> {
    tick_project_mode(workspace, project, false)
}

fn tick_background_project(workspace: &Path, project: &str) -> Result<Value, String> {
    tick_project_mode(workspace, project, true)
}

fn registered_projects(workspace: &Path) -> Result<Vec<String>, String> {
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
            .and_then(|store| store.load_registry())
            .map_err(|_| "WEB_AUTO_REGISTRY_UNAVAILABLE")?;
    Ok(registry
        .projects
        .into_iter()
        .filter(|project| project.chatgpt_target_url.is_some())
        .map(|project| project.project_id)
        .collect())
}

fn startable_projects(
    projects: &[String],
    active: &BTreeSet<String>,
    capacity: usize,
    cursor: usize,
) -> Vec<String> {
    if projects.is_empty() || capacity == 0 {
        return Vec::new();
    }
    (0..projects.len())
        .map(|offset| &projects[(cursor + offset) % projects.len()])
        .filter(|project| !active.contains(*project))
        .take(capacity)
        .cloned()
        .collect()
}

pub(crate) fn start_scheduler(state: SharedState) {
    tokio::spawn(async move {
        let workspace = {
            let app = state.lock().await;
            PathBuf::from(&app.workspace_root)
        };
        let mut watch = tokio::time::interval(AUTO_INTERVAL);
        watch.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        watch.tick().await; // Allow service startup to settle before browser reads.
        let mut active = BTreeMap::<String, tokio::task::JoinHandle<()>>::new();
        let mut cursor = 0usize;
        loop {
            watch.tick().await;
            if !state.lock().await.server_running {
                break;
            }
            active.retain(|_, handle| !handle.is_finished());
            let Ok(projects) = registered_projects(&workspace) else {
                continue;
            };
            let running: BTreeSet<_> = active.keys().cloned().collect();
            let capacity = MAX_ACTIVE_PROJECTS.saturating_sub(active.len());
            for project in startable_projects(&projects, &running, capacity, cursor) {
                let workspace = workspace.clone();
                let project_for_task = project.clone();
                let handle = tokio::task::spawn_blocking(move || {
                    let _ = tick_background_project(&workspace, &project_for_task);
                });
                active.insert(project, handle);
            }
            if !projects.is_empty() {
                cursor = (cursor + 1) % projects.len();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use catdesk_wake::{protocol::digest, store::Receipt};

    #[test]
    fn scheduler_keeps_other_projects_startable_when_one_is_busy() {
        let projects = vec!["alpha".into(), "beta".into(), "gamma".into()];
        let active = BTreeSet::from(["alpha".into()]);
        assert_eq!(
            startable_projects(&projects, &active, 2, 0),
            vec!["beta", "gamma"]
        );
        assert_eq!(startable_projects(&projects, &active, 1, 2), vec!["gamma"]);
    }

    #[test]
    fn opted_out_policy_still_allows_observation_but_not_request() {
        let mut pressure = serde_json::from_value::<PressureStatus>(json!({
            "key":{"projectId":"alpha","conversationUrl":"https://chatgpt.com/c/old","targetGeneration":1},
            "targetSha256":"x","isCurrent":true,"recordedEvents":1,
            "visibleUniqueTokens":90000,"observedEstimateTokens":90000,
            "unknownSourceCount":0,"estimatedUpperTokens":90000,
            "observedToolArgumentTokens":0,"observedToolResultTokens":0,
            "configuredBudgetTokens":96000,"safetyHeadroomTokens":24000,"triggerTokens":72000,
            "nearCapacity":true,"coverageKnown":true,"browserCoverage":{"observed":true,"initialHistoryGap":false,"identityGap":false,"textGap":false,"overflowGap":false},
            "pressureAssessment":"OBSERVED_AT_OR_ABOVE_TRIGGER",
            "automaticRolloverEnabled":false,"incompleteHistoryRollover":false,
            "rolloverIntent":null,"estimateMethod":"test","advisory":true,"originAuthenticated":false
        })).unwrap();
        assert!(!pressure_due(&pressure));
        pressure.automatic_rollover_enabled = true;
        assert!(pressure_due(&pressure));
    }

    #[test]
    fn stopped_wake_never_enters_browser_step() {
        let root = std::env::temp_dir().join(format!("web-auto-stopped-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let wake = Store::open_scoped_for_test(&root.join("wake"), &root).unwrap();
        wake.initialize().unwrap();
        let source = wake
            .set_target("alpha", 0, "https://chatgpt.com/c/old")
            .unwrap();
        runtime::control(&wake, "STOPPED").unwrap();
        let state = running_wake_state(&wake, "alpha", &source).unwrap();
        assert_eq!(state, "WAKE_STOPPED");
        let result = gated_browser_step(state, || -> Result<(&'static str, Value), String> {
            panic!("stopped host must never call the browser action")
        })
        .unwrap();
        assert_eq!(result.0, "WAKE_STOPPED");
        assert_eq!(result.1["browserAction"], "NONE");
    }

    #[test]
    fn stop_after_handoff_collection_refuses_automatic_execute() {
        let root =
            std::env::temp_dir().join(format!("web-auto-collected-stop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let wake = Store::open_scoped_for_test(&root.join("wake"), &root).unwrap();
        wake.initialize().unwrap();
        let source = wake
            .set_target("alpha", 0, "https://chatgpt.com/c/old")
            .unwrap();
        runtime::control(&wake, "STOPPED").unwrap();
        let (stage, result) = execute_after_live_recheck_with(&wake, "alpha", &source, || {
            panic!("STOPPED after collection must not invoke rollover execution")
        })
        .unwrap();
        assert_eq!(stage, "WAKE_STOPPED");
        assert_eq!(result["browserAction"], "NONE");
    }

    #[test]
    fn busy_shared_browser_is_retryable_per_project() {
        assert_eq!(error_stage("WAKE_STOPPED"), "WAKE_STOPPED");
        assert_eq!(error_stage("WAKE_TARGET_CHANGED"), "WAKE_TARGET_CHANGED");
        assert_eq!(error_stage("BROWSER_SESSION_BUSY"), "AWAITING_BROWSER");
        assert_eq!(
            error_stage("BOOTSTRAP_SOURCE_QUEUE_PENDING"),
            "AWAITING_QUIESCENCE"
        );
        assert_eq!(
            error_stage("COOPERATIVE_HOST_PROTOCOL_REQUIRED"),
            "ATTENTION"
        );
    }

    #[test]
    fn background_observation_requires_explicit_project_policy() {
        let root = std::env::temp_dir().join(format!("web-auto-policy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let key = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: "https://chatgpt.com/c/old".into(),
            target_generation: 1,
        };
        assert!(!background_observation_enabled(&root, &key).unwrap());
        let meter = WebContextStore::open(root.join(".catdesk/web-context")).unwrap();
        meter.bind_observed_target(key.clone(), None).unwrap();
        assert!(!background_observation_enabled(&root, &key).unwrap());
        let policy = crate::web_conversation_context::ContextPressurePolicy {
            automatic_observation_enabled: true,
            ..Default::default()
        };
        meter.configure_policy("alpha", policy).unwrap();
        assert!(background_observation_enabled(&root, &key).unwrap());
    }

    #[test]
    fn web_work_pending_overrides_older_completed_anchor_and_completed_work_wins() {
        let root = std::env::temp_dir().join(format!("web-auto-anchor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let wake = Store::open_scoped_for_test(&root.join("wake"), &root).unwrap();
        wake.initialize().unwrap();
        let source = wake
            .set_target("alpha", 0, "https://chatgpt.com/c/old")
            .unwrap();
        let lease = wake.host_lock().unwrap();
        for id in ["task-review", "web-continue-lineage"] {
            let event = wake
                .produce(id, "alpha", "review_ready", "review work")
                .unwrap();
            let delivery = wake.claim(&event, &lease).unwrap();
            wake.transition(id, lease.owner(), Phase::Submitting, None, None)
                .unwrap();
            let receipt = Receipt {
                schema_version: 1,
                event_id: id.into(),
                target_generation: source.generation,
                target_digest: source.digest.clone(),
                message_digest: digest(&event.message),
                sent_utc: catdesk_wake::store::now(),
                evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
            };
            wake.record_exact_receipt_timer(id, &receipt).unwrap();
            wake.transition(
                id,
                delivery.owner.as_str(),
                Phase::Sent,
                Some(receipt),
                None,
            )
            .unwrap();
            wake.complete_timer(id).unwrap();
        }
        assert_eq!(
            completed_task_anchor(&wake, "alpha", &source).unwrap(),
            Some("task-review".into())
        );
        assert_eq!(completed_task_anchor(&wake, "beta", &source).unwrap(), None);
        std::fs::create_dir(root.join(".catdesk")).unwrap();
        let key = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: source.url.clone(),
            target_generation: source.generation,
        };
        let meter = WebContextStore::open(root.join(".catdesk/web-context")).unwrap();
        meter.bind_observed_target(key.clone(), None).unwrap();
        let pressure = meter.pressure(&key).unwrap();
        assert_eq!(
            source_task_anchor(&root, &wake, "alpha", &source, &key, &pressure).unwrap(),
            Some("task-review".into())
        );

        let request_id = "central-task";
        let event_id = format!("web-work-{}", digest("alpha\0central-task"));
        let message = "Continue the registered project task";
        let ledger = WorkLedger::open(&root, "alpha").unwrap();
        ledger
            .prepare(request_id, &event_id, &digest(message), &source)
            .unwrap();
        let event = wake
            .produce_expected(
                &event_id,
                "alpha",
                "engineering_attention",
                message,
                &source,
            )
            .unwrap();
        assert_eq!(
            source_task_anchor(&root, &wake, "alpha", &source, &key, &pressure).unwrap(),
            None,
            "pending Web work must not inherit an older completed task anchor"
        );
        let delivery = wake.claim(&event, &lease).unwrap();
        wake.transition(&event_id, lease.owner(), Phase::Submitting, None, None)
            .unwrap();
        let receipt = Receipt {
            schema_version: 1,
            event_id: event_id.clone(),
            target_generation: source.generation,
            target_digest: source.digest.clone(),
            message_digest: digest(message),
            sent_utc: catdesk_wake::store::now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        wake.record_exact_receipt_timer(&event_id, &receipt)
            .unwrap();
        wake.transition(
            &event_id,
            delivery.owner.as_str(),
            Phase::Sent,
            Some(receipt),
            None,
        )
        .unwrap();
        wake.complete_timer(&event_id).unwrap();
        assert_eq!(
            source_task_anchor(&root, &wake, "alpha", &source, &key, &pressure).unwrap(),
            Some(event_id),
            "the active completed work request must replace older anchors"
        );
    }
}
