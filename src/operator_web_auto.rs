//! Small periodic driver for the existing exact-target Web operator steps.
//! One project can wait on its browser without holding up another project.

use super::{paired_key, rollover};
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
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const AUTO_INTERVAL: Duration = Duration::from_secs(120);
const MAX_ACTIVE_PROJECTS: usize = 8;

struct CancelOnExit(Arc<AtomicBool>);
impl Drop for CancelOnExit {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

fn require_scheduler_running(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("WEB_AUTO_OWNER_STOPPED".into())
    } else {
        Ok(())
    }
}

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
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<(&'static str, Value), String> {
    keep_running()?;
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let wake = Store::open(&root)?;
    execute_after_live_recheck_with(&wake, project, source, || {
        rollover::execute_automatic(workspace, project, keep_running)
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
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<(&'static str, Value), String> {
    keep_running()?;
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
        rollover::AutoStage::Execute => {
            execute_after_live_recheck(workspace, project, source, keep_running)
        }
        rollover::AutoStage::CollectHandoff => {
            // Crash recovery may find the request saved before queue
            // publication. Republishing the same event ID is idempotent and
            // cannot resend a submitted browser turn.
            rollover::resume_saved_handoff_request_guarded(workspace, project, keep_running)?;
            match rollover::collect_handoff_guarded(workspace, project, keep_running) {
                Ok(_) => execute_after_live_recheck(workspace, project, source, keep_running),
                Err(reason) if is_pending_handoff(&reason) => {
                    Ok(("AWAITING_HANDOFF", json!({"reason":reason})))
                }
                Err(reason) => Err(reason),
            }
        }
        rollover::AutoStage::Observe => {
            let observed = super::capture_context_guarded(workspace, project, keep_running)?;
            keep_running()?;
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
            let request =
                rollover::request_handoff_guarded(workspace, project, &task_id, keep_running)?;
            Ok(("HANDOFF_REQUESTED", request))
        }
    }
}

fn tick_project_mode(
    workspace: &Path,
    project: &str,
    background: bool,
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<Value, String> {
    keep_running()?;
    let (key, source) = match paired_key(workspace, project) {
        Ok(key) => {
            let source = Target::new(key.target_generation, key.conversation_url.clone())?;
            (key, source)
        }
        Err(reason) => match rollover::automatic_recovery_authority(workspace, project) {
            Ok(Some(authority)) => authority,
            Ok(None) | Err(_) => {
                keep_running()?;
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
            if keep_running().is_ok() {
                let _ = rollover::record_auto_status(
                    workspace,
                    project,
                    "ATTENTION",
                    Some(source.generation),
                    Some(reason),
                );
            }
        })?;
    keep_running()?;
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
        gated_browser_step(wake_state, || {
            run_step(workspace, project, &key, &source, keep_running)
        })
    })();
    // A replaced/stopped owner must not overwrite the next run's status.
    keep_running()?;
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
    tick_project_mode(workspace, project, false, &|| Ok(()))
}

fn tick_background_project(
    workspace: &Path,
    project: &str,
    cancel: &AtomicBool,
) -> Result<Value, String> {
    tick_project_mode(workspace, project, true, &|| {
        require_scheduler_running(cancel)
    })
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

async fn drain_project_tasks(
    cancel: &AtomicBool,
    active: BTreeMap<String, tokio::task::JoinHandle<()>>,
) {
    cancel.store(true, Ordering::Release);
    for (_, task) in active {
        let _ = task.await;
    }
}

pub(crate) fn start_scheduler(state: SharedState) {
    tokio::spawn(async move {
        let mut app = state.lock().await;
        if !app.server_running || app.server_handle.as_ref().is_some_and(|h| h.is_finished()) {
            app.server_running = false;
            let tasks = app.cancel_web_auto_scheduler();
            drop(app);
            super::join_auto_schedulers(tasks).await;
            return;
        }
        if let Some(previous) = app.web_auto_cancel.take() {
            previous.store(true, Ordering::Release);
        }
        app.web_auto_tasks.retain(|task| !task.is_finished());
        let cancel = Arc::new(AtomicBool::new(false));
        app.web_auto_cancel = Some(cancel.clone());
        let workspace = PathBuf::from(&app.workspace_root);
        let driver_state = state.clone();
        let driver = tokio::spawn(async move {
            let _lifetime = CancelOnExit(cancel.clone());
            let mut watch = tokio::time::interval(AUTO_INTERVAL);
            watch.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            watch.tick().await; // Allow service startup to settle before browser reads.
            let mut lifecycle = tokio::time::interval(Duration::from_millis(250));
            let mut active = BTreeMap::<String, tokio::task::JoinHandle<()>>::new();
            let mut cursor = 0usize;
            loop {
                let tick_due =
                    tokio::select! { _ = watch.tick() => true, _ = lifecycle.tick() => false };
                let listener_live = {
                    let mut app = driver_state.lock().await;
                    let live = app.server_running
                        && app.server_handle.as_ref().is_none_or(|h| !h.is_finished());
                    if !live
                        && app
                            .web_auto_cancel
                            .as_ref()
                            .is_some_and(|current| Arc::ptr_eq(current, &cancel))
                    {
                        app.server_running = false;
                    }
                    live
                };
                if require_scheduler_running(&cancel).is_err() || !listener_live {
                    cancel.store(true, Ordering::Release);
                    break;
                }
                active.retain(|_, handle| !handle.is_finished());
                if !tick_due {
                    continue;
                }
                let Ok(projects) = registered_projects(&workspace) else {
                    continue;
                };
                let running: BTreeSet<_> = active.keys().cloned().collect();
                let capacity = MAX_ACTIVE_PROJECTS.saturating_sub(active.len());
                for project in startable_projects(&projects, &running, capacity, cursor) {
                    let workspace = workspace.clone();
                    let project_for_task = project.clone();
                    let task_cancel = cancel.clone();
                    let handle = tokio::task::spawn_blocking(move || {
                        let _ =
                            tick_background_project(&workspace, &project_for_task, &task_cancel);
                    });
                    active.insert(project, handle);
                }
                if !projects.is_empty() {
                    cursor = (cursor + 1) % projects.len();
                }
            }
            // Dropping a blocking JoinHandle does not cancel its work. Retain each
            // handle until guarded browser I/O unwinds and its private Job closes.
            drain_project_tasks(&cancel, active).await;
        });
        app.web_auto_tasks.push(driver);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use catdesk_wake::{protocol::digest, store::Receipt};

    fn lifecycle_fixture() -> SharedState {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("web-owner-{}", uuid::Uuid::new_v4()));
        let mut app = crate::state::AppState::new_isolated_fixture(
            0,
            root.display().to_string(),
            root.join("config.toml"),
        )
        .unwrap();
        app.server_running = true;
        Arc::new(tokio::sync::Mutex::new(app))
    }

    async fn owner_after_start(
        state: &SharedState,
        previous: Option<&Arc<AtomicBool>>,
    ) -> Arc<AtomicBool> {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Some(owner) = state.lock().await.web_auto_cancel.clone()
                    && previous.is_none_or(|old| !Arc::ptr_eq(old, &owner))
                {
                    return owner;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("scheduler owner registration")
    }

    #[tokio::test]
    async fn stopped_or_replaced_scheduler_never_revives_after_restart() {
        let state = lifecycle_fixture();
        start_scheduler(state.clone());
        let first = owner_after_start(&state, None).await;
        start_scheduler(state.clone());
        let second = owner_after_start(&state, Some(&first)).await;
        assert!(require_scheduler_running(&first).is_err());
        assert!(require_scheduler_running(&second).is_ok());
        tokio::time::timeout(
            Duration::from_secs(2),
            super::super::stop_auto_scheduler(state.clone()),
        )
        .await
        .unwrap();
        assert!(require_scheduler_running(&second).is_err());
        assert!(state.lock().await.web_auto_tasks.is_empty());
        state.lock().await.server_running = true;
        start_scheduler(state.clone());
        let third = owner_after_start(&state, None).await;
        assert!(!Arc::ptr_eq(&second, &third));
        assert!(require_scheduler_running(&second).is_err());
        super::super::stop_auto_scheduler(state).await;
        assert!(require_scheduler_running(&third).is_err());
    }

    #[tokio::test]
    async fn finished_listener_retires_owner_without_waiting_for_next_browser_tick() {
        let state = lifecycle_fixture();
        let (finish, wait) = tokio::sync::oneshot::channel::<()>();
        state.lock().await.server_handle = Some(tokio::spawn(async move {
            let _ = wait.await;
        }));
        start_scheduler(state.clone());
        let owner = owner_after_start(&state, None).await;
        finish.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while require_scheduler_running(&owner).is_ok() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            !state.lock().await.server_running,
            "finished listener must also correct the stale UI flag"
        );
        super::super::stop_auto_scheduler(state).await;
    }

    #[tokio::test]
    async fn listener_that_exits_during_startup_never_registers_a_scheduler() {
        let state = lifecycle_fixture();
        let listener = tokio::spawn(async {});
        while !listener.is_finished() {
            tokio::task::yield_now().await;
        }
        state.lock().await.server_handle = Some(listener);
        start_scheduler(state.clone());
        tokio::time::timeout(Duration::from_secs(2), async {
            while state.lock().await.server_running {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let app = state.lock().await;
        assert!(app.web_auto_cancel.is_none());
        assert!(app.web_auto_tasks.is_empty());
    }

    #[tokio::test]
    async fn shutdown_drains_every_owned_blocking_task() {
        let cancel = Arc::new(AtomicBool::new(false));
        let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut active = BTreeMap::new();
        for project in ["alpha", "beta"] {
            let cancelled = cancel.clone();
            let finished = completed.clone();
            active.insert(
                project.into(),
                tokio::task::spawn_blocking(move || {
                    while require_scheduler_running(&cancelled).is_ok() {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    finished.fetch_add(1, Ordering::Release);
                }),
            );
        }
        tokio::time::timeout(Duration::from_secs(2), drain_project_tasks(&cancel, active))
            .await
            .unwrap();
        assert_eq!(completed.load(Ordering::Acquire), 2);
    }

    #[test]
    fn cancelled_background_owner_refuses_before_filesystem_or_browser_work() {
        let root =
            std::env::temp_dir().join(format!("web-cancel-no-create-{}", uuid::Uuid::new_v4()));
        assert_eq!(
            tick_background_project(&root, "alpha", &AtomicBool::new(true)).unwrap_err(),
            "WEB_AUTO_OWNER_STOPPED"
        );
        assert!(!root.exists());
    }

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
