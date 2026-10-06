//! Contract-governed autonomous provider and verification controller.

use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::Path;

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::reviewed_source_snapshot::{
    ReviewedSourceBaselineObservationV1, ReviewedSourceCurrentOutputV1,
    ReviewedSourceSnapshotExpectedV1, create_or_validate_reviewed_source_snapshot,
};

use super::autonomous_contract::AutonomousPolicyEngineV1;
use super::autonomous_qwen_tools::AutonomousQwenToolLoopV1;
use super::autonomy_accounting::{ActivityActorV1, ActivityEvidenceV1, ActivitySpanStatusV1};
use super::autonomy_state::{
    AUTONOMY_STATE_SCHEMA_VERSION, AutonomousPassedVerificationCheckpointV1, AutonomousPlanQueueV1,
    AutonomousProviderHandoffV1, AutonomousProviderRouteV1, AutonomousQueueTaskStateV1,
    AutonomousQueueV1, AutonomousSessionStateV1, AutonomousStateStoreV1,
    AutonomousTaskOutputBaselineAuthorityV1, AutonomousTaskOutputBaselineV1,
    AutonomousTaskOutputObservationV1, validate_exact_task_graph_materialization,
};
use super::codex_app_server::{
    CATDESK_REQUIRED_CODEX_MODEL_V1, CATDESK_REQUIRED_CODEX_REASONING_EFFORT_V1,
    CodexRoutingTelemetryV1,
};
use super::codex_cli::{
    CodexAvailabilityFailureV1, classify_codex_availability_failure,
    parse_codex_credits_reset_after, trusted_host_local_clock,
};
use super::contracts::{RunId, TurnId, WorkerSessionId};
use super::coordinator::{VerificationStatusV1, VerificationSummaryV1};
use super::runtime::{
    NormalizedProviderEventKind, ProviderMessageV1, ProviderTurnRequestV1, RuntimeError,
};
use super::worker_provider::{
    LocalQwenFallbackProviderV1, ProviderEventBatchV1, ProviderIdV1, ProviderTurnHandleV1,
    WorkerProviderTurnRequestV1, WorkerProviderV1,
};

pub trait AutonomousVerifierV1: Send {
    fn verify(&mut self) -> Result<(VerificationSummaryV1, String), RuntimeError>;

    /// Produces a bounded, independent completion artifact. A provider claim
    /// alone can never stand in for this review.
    fn final_review(
        &mut self,
        _verification: &VerificationSummaryV1,
        _authoritative_diff: &str,
    ) -> Result<String, RuntimeError> {
        Err(RuntimeError::Validation(
            "autonomous verifier did not provide a final review artifact".into(),
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutonomousControllerOutcomeV1 {
    pub state: AutonomousSessionStateV1,
    pub provider_events: usize,
    pub verification: Option<VerificationSummaryV1>,
    pub authoritative_diff: Option<String>,
    pub final_review: Option<String>,
}

pub struct AutonomousControllerV1<P, V> {
    policy: AutonomousPolicyEngineV1,
    store: AutonomousStateStoreV1,
    provider: P,
    verifier: V,
    session_id: String,
    handles: BTreeMap<String, ProviderTurnHandleV1>,
    last_handle: Option<ProviderTurnHandleV1>,
    repair_attempts: u32,
    cancel_requested: bool,
    qwen_tools: Option<AutonomousQwenToolLoopV1>,
}

pub(crate) const MAX_TASK_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
const DIRECT_CHATGPT_EXECUTOR_ID: &str = "chatgpt-direct";

/// Durable restart decision shared by the runtime preflight and controller.
///
/// A persisted provider-handle id is not itself a liveness proof: after a
/// process restart it is deliberately retained for same-thread continuity.
/// Only the bounded execution-accounting lifecycle may prove that the former
/// provider owner was interrupted and that no reviewer or mutation outcome is
/// still open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RestartTaskReconciliationV1 {
    NotApplicable,
    AlreadyReady,
    Requeued,
    Pending,
}

/// Reconciles exactly one stale `WORKER_RUNNING` current task from durable
/// CatDesk state. This does not mint a provider turn or touch immutable task
/// output attribution/baseline state. Absence, ambiguity, live ownership, and
/// potential mutation outcomes deliberately remain pending.
pub(crate) fn reconcile_restart_task_state(
    store: &AutonomousStateStoreV1,
    session_id: &str,
) -> Result<RestartTaskReconciliationV1, RuntimeError> {
    let _lock = store
        .acquire_lock(session_id, "restart-reconciliation")
        .map_err(RuntimeError::from)?;
    let snapshot = store.load_session(session_id).map_err(RuntimeError::from)?;
    if snapshot.state != AutonomousSessionStateV1::Queued {
        return Ok(RestartTaskReconciliationV1::NotApplicable);
    }
    let Some(task_id) = snapshot.current_task_id.as_deref() else {
        return Ok(RestartTaskReconciliationV1::NotApplicable);
    };
    let mut queue = store.load_queue(session_id).map_err(RuntimeError::from)?;
    let Some(task) = queue.tasks.iter_mut().find(|task| task.task_id == task_id) else {
        return Ok(RestartTaskReconciliationV1::Pending);
    };
    if task.state == AutonomousQueueTaskStateV1::Ready {
        return Ok(RestartTaskReconciliationV1::AlreadyReady);
    }
    if task.state != AutonomousQueueTaskStateV1::WorkerRunning {
        return Ok(RestartTaskReconciliationV1::NotApplicable);
    }

    let accounting = store
        .execution_accounting_store()
        .map_err(RuntimeError::from)?;
    let records = accounting
        .list(Some(session_id))?
        .into_iter()
        .filter(|record| record.task_id == task_id)
        .collect::<Vec<_>>();
    let Some(record) = records.first() else {
        return Ok(RestartTaskReconciliationV1::Pending);
    };
    if records.len() != 1
        || record.provider_turns == 0
        || record.ended_at_unix_millis.is_some()
        || record.activity_evidence_truncated
        // A tool call may have mutated a workspace or external state. Without
        // an authoritative terminal result it is outcome-unknown evidence.
        || record.catdesk_tool_calls != 0
        || snapshot.expected_codex_thread_id.as_deref().is_some_and(|expected| {
            snapshot.provider_thread_id.as_deref() != Some(expected)
        })
        || record.provider_session_id.as_deref().is_none_or(|recorded| {
            snapshot.provider_thread_id.as_deref() != Some(recorded)
        })
    {
        return Ok(RestartTaskReconciliationV1::Pending);
    }

    let provider_interrupted = record.activity_spans.iter().any(|span| {
        span.actor == ActivityActorV1::CodexProviderActive
            && span.evidence == ActivityEvidenceV1::ProviderLifecycle
            && span.status == ActivitySpanStatusV1::Interrupted
    });
    // A completed verifier span is historical evidence from a prior failed
    // verification/repair cycle, not a currently-live reviewer owner. An
    // open verifier is live and an interrupted verifier has an ambiguous
    // outcome, so both remain fail-closed.
    let live_or_ambiguous_ownership = record.activity_spans.iter().any(|span| {
        span.status == ActivitySpanStatusV1::Open
            || (span.actor == ActivityActorV1::CatdeskVerificationReviewActive
                && span.status == ActivitySpanStatusV1::Interrupted)
    });
    if !provider_interrupted || live_or_ambiguous_ownership {
        return Ok(RestartTaskReconciliationV1::Pending);
    }

    task.state = AutonomousQueueTaskStateV1::Ready;
    store
        .save_queue(session_id, &queue)
        .map_err(RuntimeError::from)?;
    store
        .append_event(
            session_id,
            "stale_worker_running_reconciled",
            "interrupted provider ownership requeued without changing task attribution",
        )
        .map_err(RuntimeError::from)?;
    Ok(RestartTaskReconciliationV1::Requeued)
}

impl<P: WorkerProviderV1 + LocalQwenFallbackProviderV1, V: AutonomousVerifierV1>
    AutonomousControllerV1<P, V>
{
    pub fn new(
        policy: AutonomousPolicyEngineV1,
        store: AutonomousStateStoreV1,
        provider: P,
        verifier: V,
        session_id: String,
    ) -> Self {
        Self {
            policy,
            store,
            provider,
            verifier,
            session_id,
            handles: BTreeMap::new(),
            last_handle: None,
            repair_attempts: 0,
            cancel_requested: false,
            qwen_tools: None,
        }
    }

    /// Called by the supported app-server integration after a read-only
    /// snapshot or rate-limit notification. It persists only bounded routing
    /// telemetry; this controller owns no billing or account mutation path.
    pub fn record_codex_observability(
        &self,
        telemetry: CodexRoutingTelemetryV1,
    ) -> Result<(), RuntimeError> {
        self.store
            .record_codex_routing_telemetry(&self.session_id, telemetry.clone(), 0)
            .map_err(RuntimeError::from)?;
        self.store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .record_observability(&self.session_id, telemetry, controller_now_millis())
    }

    pub async fn run_once(
        &mut self,
        now_unix: u64,
    ) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
        if self.cancel_requested {
            return self.stop(
                AutonomousSessionStateV1::Cancelled,
                "cancellation_requested",
            );
        }
        let mut snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        self.cancel_requested = snapshot.cancellation_requested;
        self.repair_attempts = snapshot.repair_attempts;
        if self.cancel_requested {
            return self.stop(
                AutonomousSessionStateV1::Cancelled,
                "cancellation_requested",
            );
        }
        if snapshot.state.is_terminal() {
            return Ok(AutonomousControllerOutcomeV1 {
                state: snapshot.state,
                provider_events: 0,
                verification: None,
                authoritative_diff: None,
                final_review: None,
            });
        }
        // A passed-verification checkpoint is deliberately narrower than a
        // provider lease: it permits only the already-executed task's local,
        // deterministic completion finalization. It never reaches task
        // selection, baseline capture, provider launch, or repair handling.
        if matches!(
            snapshot.state,
            AutonomousSessionStateV1::Verifying
                | AutonomousSessionStateV1::RecoveringAfterRestart
                | AutonomousSessionStateV1::Queued
                | AutonomousSessionStateV1::WaitingForChatgpt
                | AutonomousSessionStateV1::WaitingForUser
        ) {
            match self
                .store
                .load_passed_verification_checkpoint(&self.session_id)
            {
                Ok(Some(checkpoint)) => {
                    let executor_id = if checkpoint.provider_id == DIRECT_CHATGPT_EXECUTOR_ID {
                        DIRECT_CHATGPT_EXECUTOR_ID
                    } else {
                        self.provider.provider_id().as_str()
                    };
                    return self.finalize_passed_verification(checkpoint, now_unix, 0, executor_id);
                }
                Ok(None) => {}
                Err(_) => {
                    return self.post_verification_finalization_failure(
                        "passed_verification_checkpoint_unavailable",
                    );
                }
            }
        }
        if snapshot.state == AutonomousSessionStateV1::WaitingForChatgpt {
            // Only the locked, supported reply path may requeue a waiting
            // session. Gate artifacts are never themselves a launch permit.
            return Ok(AutonomousControllerOutcomeV1 {
                state: AutonomousSessionStateV1::WaitingForChatgpt,
                provider_events: 0,
                verification: None,
                authoritative_diff: None,
                final_review: None,
            });
        }
        if snapshot.state == AutonomousSessionStateV1::RecoveringAfterRestart
            && snapshot.provider_thread_id.is_none()
        {
            return self.escalate("restart_session_continuity_missing");
        }
        // Do this before stale-worker recovery or any other task progress
        // mutation. A non-empty graph has one immutable approval source: the
        // approved contract. Durable queue/plan metadata must still exactly
        // materialize it before this controller touches the task lifecycle.
        if !self.policy.contract().task_graph.is_empty()
            && self.load_exact_graph_materialization().is_err()
        {
            return self.escalate("approved_graph_materialization_unavailable_or_mismatched");
        }
        // A planner reply may have moved a terminally escalated session back
        // to QUEUED while an older daemon left its task as WORKER_RUNNING.
        // Reconciliation is shared with the host preflight and only accepts
        // positively interrupted provider ownership. It preserves the
        // captured provider thread for same-thread resume and never runs
        // while this controller still owns a live handle.
        if snapshot.state == AutonomousSessionStateV1::Queued && self.last_handle.is_none() {
            if reconcile_restart_task_state(&self.store, &self.session_id)?
                == RestartTaskReconciliationV1::Pending
            {
                return Err(RuntimeError::Validation(
                    "restart task reconciliation is pending authoritative ownership evidence"
                        .into(),
                ));
            }
        }
        if snapshot.provider_route == AutonomousProviderRouteV1::QwenFallbackActive
            && self.provider.provider_route() != AutonomousProviderRouteV1::QwenFallbackActive
            && !self
                .provider
                .activate_local_qwen(&self.policy.contract().provider_policy.routine_model)
                .await?
        {
            self.set_provider_route(AutonomousProviderRouteV1::QwenUnavailable)?;
            return self.escalate("qwen_unavailable_after_restart");
        }
        if snapshot.provider_route == AutonomousProviderRouteV1::QwenFallbackActive
            && snapshot.state == AutonomousSessionStateV1::Queued
            && snapshot.provider_handle_id.is_none()
            && snapshot
                .codex_eligible_after_unix
                .is_some_and(|eligible_at| now_unix >= eligible_at)
        {
            snapshot = self.restore_codex_at_safe_boundary(snapshot)?;
        }
        if !self.policy.contract().autonomy_lease.valid_at(now_unix) {
            return self.stop(AutonomousSessionStateV1::LeaseExpired, "lease_expired");
        }
        if snapshot.state == AutonomousSessionStateV1::RateLimited {
            if snapshot.rate_limited_since_unix.is_some_and(|started| {
                now_unix.saturating_sub(started)
                    > self
                        .policy
                        .contract()
                        .rate_limit_policy
                        .maximum_rate_limited_seconds
            }) {
                return self.escalate("rate_limit_duration_exhausted");
            }
            if snapshot
                .retry_not_before_unix
                .is_some_and(|retry_at| now_unix < retry_at)
            {
                return Ok(AutonomousControllerOutcomeV1 {
                    state: AutonomousSessionStateV1::RateLimited,
                    provider_events: 0,
                    verification: None,
                    authoritative_diff: None,
                    final_review: None,
                });
            }
        }
        if snapshot.state == AutonomousSessionStateV1::Running {
            if let Some(handle) = self.last_handle.clone() {
                let batch = self
                    .provider
                    .poll_events(&handle, snapshot.provider_event_cursor)
                    .await?;
                return self.finish_turn(batch, now_unix).await;
            }
        }
        // Re-load and re-check immediately before task selection. The first
        // check above protects stale recovery; this one protects the launch
        // boundary and supplies only contract-approved task instructions.
        let graph_materialization = if self.policy.contract().task_graph.is_empty() {
            None
        } else {
            match self.load_exact_graph_materialization() {
                Ok(materialization) => Some(materialization),
                Err(_) => {
                    return self
                        .escalate("approved_graph_materialization_unavailable_or_mismatched");
                }
            }
        };
        let queue = match graph_materialization.as_ref() {
            Some((queue, _)) => queue.clone(),
            None => self
                .store
                .load_queue(&self.session_id)
                .map_err(RuntimeError::from)?,
        };
        let task = queue.next_ready_task().ok_or_else(|| {
            RuntimeError::Validation("no dependency-satisfied autonomous task is ready".into())
        })?;
        let task_acceptance_criteria = if graph_materialization.is_some() {
            self.policy
                .contract()
                .task_graph
                .iter()
                .find(|spec| spec.task_id == task.task_id)
                .map(|spec| spec.acceptance_criteria.clone())
                .ok_or_else(|| {
                    RuntimeError::Validation(
                        "validated graph task is absent from the approved contract".into(),
                    )
                })?
        } else {
            match self.store.load_plan_queue(&self.session_id) {
                Ok(plan) => plan
                    .tasks
                    .into_iter()
                    .find(|planned| planned.task_id == task.task_id)
                    .map(|planned| planned.acceptance_criteria)
                    .unwrap_or_else(|| self.policy.contract().ordered_steps.clone()),
                Err(_) => self.policy.contract().ordered_steps.clone(),
            }
        };
        let selected_new_task = snapshot.current_task_id.as_deref() != Some(task.task_id.as_str());
        if selected_new_task {
            self.repair_attempts = 0;
            snapshot.repair_attempts = 0;
        }
        if self
            .policy
            .contract()
            .task_graph
            .iter()
            .find(|spec| spec.task_id == task.task_id)
            .and_then(|spec| spec.planner_gate.as_ref())
            .is_some()
            && !self
                .store
                .planner_gate_satisfied(&self.session_id, &task.task_id)
                .map_err(RuntimeError::from)?
        {
            snapshot.current_task_id = Some(task.task_id.clone());
            snapshot.active = true;
            self.store
                .save_session(&snapshot)
                .map_err(RuntimeError::from)?;
            return self.escalate("planned_architecture_decision_required");
        }
        if self.provider.provider_id() == ProviderIdV1::CodexCli
            && !has_authoritative_terra_high_gate(&snapshot)
        {
            return self.escalate("codex_terra_high_gate_unavailable_or_mismatched");
        }
        let worker = WorkerSessionId::new(format!("autonomy-{}", self.session_id))
            .map_err(RuntimeError::Validation)?;
        let active_model = if self.provider.provider_id() == ProviderIdV1::Ollama {
            self.policy.contract().provider_policy.routine_model.clone()
        } else {
            self.policy.contract().provider_policy.primary_model.clone()
        };
        let mut session = self.provider.create_session(&active_model, &worker);
        if self.provider.provider_id() != ProviderIdV1::Ollama {
            if let Some(provider_thread_id) = &snapshot.provider_thread_id {
                session.provider_session_id = provider_thread_id.clone();
            }
        }
        let qwen_turn = self.provider.provider_id() == ProviderIdV1::Ollama
            && snapshot.provider_route == AutonomousProviderRouteV1::QwenFallbackActive;
        let tool_definitions = if qwen_turn {
            self.qwen_tools_for(&task.task_id)?.tool_definitions()
        } else {
            // Codex CLI owns its tool protocol and must never receive CatDesk
            // definitions through this provider adapter.
            Vec::new()
        };
        let turn = ProviderTurnRequestV1 {
            run_id: RunId::new(self.session_id.clone()).map_err(RuntimeError::Validation)?,
            worker_session_id: worker,
            turn_id: TurnId::new(format!(
                "{}-turn-{}",
                task.task_id,
                snapshot.provider_turn_count + 1
            ))
            .map_err(RuntimeError::Validation)?,
            model_id: active_model,
            context_json: json!({"contractHash": self.policy.contract_hash(), "taskId": task.task_id}),
            tool_definitions,
            max_output_bytes: 16 * 1024,
        };
        // Task classification deliberately does not select Codex transport.
        // A new DAG task remains a new task for repair accounting even when
        // the host has already bound this session to the canonical project
        // thread.
        let is_repair = !selected_new_task;
        let continue_canonical_codex_thread = self.provider.provider_id() == ProviderIdV1::CodexCli
            && snapshot.provider_thread_id.is_some();
        if snapshot.provider_turn_count
            >= self.policy.contract().autonomy_lease.maximum_provider_turns
        {
            return self.escalate("provider_turn_budget_exhausted");
        }
        if is_repair
            && self.repair_attempts > self.policy.contract().verification_policy.max_repair_cycles
        {
            return self.escalate("repair_budget_exhausted");
        }
        let provider_session_id = session.provider_session_id.clone();
        let planner_reply = self
            .store
            .load_planner_reply(&self.session_id)
            .ok()
            .filter(|reply| !reply.consumed);
        let worker_instruction = if let Some(reply) = &planner_reply {
            format!(
                "A persisted planner decision applies to this turn. Decision: {}\nConstraints: {}\nResume only the existing approved task and preserve CatDesk verification requirements.",
                reply.decision,
                reply.constraints.join("; ")
            )
        } else if self.provider.provider_id() == ProviderIdV1::Ollama
            && snapshot.provider_route == AutonomousProviderRouteV1::QwenFallbackActive
        {
            if self.policy.contract().starts_on_routine_provider() {
                format!(
                    "This approved CatDesk task explicitly selected the local routine provider. Approved objective:\n{}\n\nOrdered work:\n{}\n\nExecute only task {} inside the approved workspace. Do not change Git branches, publish Git changes, access credentials, or modify files outside the contract. CatDesk independently verifies completion.",
                    bounded_contract_text(&self.policy.contract().objective, 4_096),
                    self.policy.contract().ordered_steps.join("\n"),
                    task.task_id
                )
            } else {
                let handoff = self
                    .store
                    .load_provider_handoff(&self.session_id)
                    .map_err(RuntimeError::from)?;
                format!(
                    "Continue the existing approved CatDesk task after Codex plan-credit exhaustion. Do not repeat completed work. CatDesk verification remains authoritative. Completed task IDs: {}. Continue from the first incomplete ordered step:\n{}",
                    handoff.completed_task_ids.join(", "),
                    handoff.ordered_steps.join("\n")
                )
            }
        } else if is_repair {
            format!(
                "Independent verification did not pass. Bounded verifier evidence:\n{}\nRepair only the unmet criteria, then wait for CatDesk verification and diff capture.",
                snapshot
                    .last_verification_summary
                    .as_deref()
                    .unwrap_or("no bounded verifier detail was persisted")
            )
        } else {
            format!(
                "Approved objective:\n{}\n\nOrdered work:\n{}\n\nExecute only task {} inside the approved workspace. Do not change Git branches, publish Git changes, access credentials, or modify files outside the contract. CatDesk independently verifies completion.",
                bounded_contract_text(&self.policy.contract().objective, 4_096),
                self.policy
                    .contract()
                    .ordered_steps
                    .iter()
                    .take(20)
                    .enumerate()
                    .map(|(index, step)| format!(
                        "{}. {}",
                        index + 1,
                        bounded_contract_text(step, 512)
                    ))
                    .collect::<Vec<_>>()
                    .join("\n"),
                task.task_id,
            )
        };
        let worker_instruction = format!(
            "Current approved task: {}\nTask-specific acceptance criteria:\n{}\n\n{}",
            task.task_id,
            task_acceptance_criteria
                .iter()
                .take(32)
                .enumerate()
                .map(|(index, criterion)| format!(
                    "{}. {}",
                    index + 1,
                    bounded_contract_text(criterion, 512)
                ))
                .collect::<Vec<_>>()
                .join("\n"),
            worker_instruction,
        );
        let mut history = if qwen_turn {
            self.qwen_tools_for(&task.task_id)?.history()
        } else {
            Vec::new()
        };
        history.push(ProviderMessageV1 {
            role: "user".into(),
            content: worker_instruction,
            tool_call_id: None,
            tool_name: None,
        });
        let request = WorkerProviderTurnRequestV1 {
            provider_session: session,
            turn: turn.clone(),
            history,
            json_envelope_recovery: false,
        };
        // Capture immutable approval-bound output evidence at the first
        // launch of this logical task. Repairs and restart recovery reuse it.
        self.capture_task_output_baseline(&task.task_id)?;
        self.prepare_turn(
            &task.task_id,
            &provider_session_id,
            continue_canonical_codex_thread.then_some(provider_session_id.as_str()),
        )?;
        if planner_reply.is_some() {
            self.store
                .mark_planner_reply_consumed(&self.session_id)
                .map_err(RuntimeError::from)?;
        }
        self.transition(
            AutonomousSessionStateV1::Running,
            if is_repair {
                "provider_repair_resumed"
            } else {
                "provider_turn_started"
            },
        )?;
        let continue_provider_turn = continue_canonical_codex_thread
            || (is_repair && self.provider.provider_id() != ProviderIdV1::Ollama);
        let handle = if continue_provider_turn {
            self.provider.resume_turn(request).await?
        } else {
            self.provider.start_turn(request).await?
        };
        if self.codex_thread_identity_mismatch(&handle.provider_session_id)? {
            return self.fail_closed_codex_thread_mismatch();
        }
        // A provider-active span begins only after the provider returned a
        // real turn handle. Preparing a queue item or sleeping for backoff is
        // not provider execution evidence.
        self.store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .activity_started(
                &self.session_id,
                ActivityActorV1::CodexProviderActive,
                ActivityEvidenceV1::ProviderLifecycle,
                controller_now_millis(),
            )?;
        self.handles
            .insert(handle.handle_id.clone(), handle.clone());
        self.last_handle = Some(handle.clone());
        self.persist_handle(&handle, 0)?;
        let batch = self.provider.poll_events(&handle, 0).await?;
        self.finish_turn(batch, now_unix).await
    }

    async fn finish_turn(
        &mut self,
        batch: ProviderEventBatchV1,
        now_unix: u64,
    ) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
        self.store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .record_events(
                &self.session_id,
                batch.events.len() as u32,
                super::autonomy_accounting::normalized_tool_call_count(&batch.events),
            )?;
        if batch.terminal {
            self.store
                .execution_accounting_store()
                .map_err(RuntimeError::from)?
                .activity_finished(
                    &self.session_id,
                    ActivityActorV1::CodexProviderActive,
                    controller_now_millis(),
                )?;
        }
        if self.cancel_requested {
            return self.stop(
                AutonomousSessionStateV1::Cancelled,
                "cancellation_requested",
            );
        }
        if let Some(provider_session_id) = &batch.provider_session_id {
            if self.codex_thread_identity_mismatch(provider_session_id)? {
                return self.fail_closed_codex_thread_mismatch();
            }
            if let Some(handle) = &mut self.last_handle {
                handle.provider_session_id = provider_session_id.clone();
            }
            if let Some(handle) = self.last_handle.clone() {
                self.persist_handle(&handle, batch.next_cursor)?;
            }
        }
        if !batch.terminal {
            if let Some(handle) = &self.last_handle {
                self.persist_handle(handle, batch.next_cursor)?;
            }
            return Ok(AutonomousControllerOutcomeV1 {
                state: AutonomousSessionStateV1::Running,
                provider_events: batch.events.len(),
                verification: None,
                authoritative_diff: None,
                final_review: None,
            });
        }
        if let Some(exhaustion_diagnostic) =
            codex_credit_exhaustion_diagnostic_from_events(&batch.events)
        {
            let reset_boundary = trusted_host_local_clock(now_unix)
                .and_then(|clock| parse_codex_credits_reset_after(exhaustion_diagnostic, clock));
            return self
                .handoff_to_qwen(now_unix, batch.events.len(), reset_boundary)
                .await;
        }
        if batch.events.iter().any(is_rate_limit_event) {
            return self.pause_for_rate_limit(now_unix, batch.events.len());
        }
        let qwen_turn = self.provider.provider_id() == ProviderIdV1::Ollama
            && self
                .store
                .load_session(&self.session_id)
                .map_err(RuntimeError::from)?
                .provider_route
                == AutonomousProviderRouteV1::QwenFallbackActive;
        if qwen_turn {
            let task_id = self
                .store
                .load_session(&self.session_id)
                .map_err(RuntimeError::from)?
                .current_task_id
                .ok_or_else(|| RuntimeError::Validation("Qwen turn has no active task".into()))?;
            let tools = self.qwen_tools_for(&task_id)?;
            let mut executed = false;
            let mut completion_claim = None;
            for event in &batch.events {
                match event.kind {
                    NormalizedProviderEventKind::ToolCall => {
                        let call = event.tool_call.as_ref().ok_or_else(|| {
                            RuntimeError::Validation(
                                "Qwen emitted a tool event without a normalized call".into(),
                            )
                        })?;
                        tools.execute(call).await?;
                        executed = true;
                    }
                    NormalizedProviderEventKind::TextDelta => {
                        tools.record_text(event.text.clone().unwrap_or_default())?;
                    }
                    NormalizedProviderEventKind::CompletionClaim => {
                        completion_claim = event.text.as_deref()
                    }
                    NormalizedProviderEventKind::MalformedResponse => {
                        return self.escalate("qwen_malformed_tool_response");
                    }
                    NormalizedProviderEventKind::CancelAck => {
                        return self.stop(AutonomousSessionStateV1::Cancelled, "qwen_cancelled");
                    }
                    NormalizedProviderEventKind::TerminalError => {}
                }
            }
            if executed {
                self.set_task_state(
                    &task_id,
                    super::autonomy_state::AutonomousQueueTaskStateV1::Ready,
                )?;
                self.transition(
                    AutonomousSessionStateV1::Queued,
                    "qwen_tool_calls_executed_continuation_required",
                )?;
                return Ok(AutonomousControllerOutcomeV1 {
                    state: AutonomousSessionStateV1::Queued,
                    provider_events: batch.events.len(),
                    verification: None,
                    authoritative_diff: None,
                    final_review: None,
                });
            }
            if let Some(claim) = completion_claim {
                if tools.completion_ready(claim).is_err() {
                    self.store
                        .append_event(
                            &self.session_id,
                            "qwen_completion_rejected",
                            "completion claim lacked required bounded tool evidence",
                        )
                        .map_err(RuntimeError::from)?;
                    return self.stop(
                        AutonomousSessionStateV1::Queued,
                        "qwen_completion_before_required_tools",
                    );
                }
            } else {
                return self.escalate("qwen_terminal_without_completion_or_tool_call");
            }
        }
        if batch
            .events
            .iter()
            .any(|event| event.kind == NormalizedProviderEventKind::TerminalError)
        {
            let diagnostic = bounded_provider_diagnostic(&batch.events);
            self.store
                .append_event(
                    &self.session_id,
                    "provider_terminal_diagnostic",
                    &diagnostic,
                )
                .map_err(RuntimeError::from)?;
            // A provider-owned turn is terminal at this boundary. Keep the
            // logical task resumable while the session waits for ChatGPT so a
            // bounded planner reply can continue the same captured provider
            // thread instead of stranding the queue in WORKER_RUNNING.
            self.set_current_task_state(super::autonomy_state::AutonomousQueueTaskStateV1::Ready)?;
            return self.escalate("provider_terminal_error");
        }
        self.transition(
            AutonomousSessionStateV1::Verifying,
            "independent_verification_started",
        )?;
        self.set_current_task_state(super::autonomy_state::AutonomousQueueTaskStateV1::Verifying)?;
        self.store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .record_verification_started(&self.session_id, controller_now_millis())?;
        self.store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .activity_started(
                &self.session_id,
                ActivityActorV1::CatdeskVerificationReviewActive,
                ActivityEvidenceV1::VerifierLifecycle,
                controller_now_millis(),
            )?;
        let (verification, diff) = self.verifier.verify()?;
        self.store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .record_verification_finished(
                &self.session_id,
                match verification.status {
                    VerificationStatusV1::Passed => "PASSED",
                    _ => "FAILED",
                },
                &verification.summary,
                controller_now_millis(),
            )?;
        self.store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .activity_finished(
                &self.session_id,
                ActivityActorV1::CatdeskVerificationReviewActive,
                controller_now_millis(),
            )?;
        if verification.status != VerificationStatusV1::Passed || diff.trim().is_empty() {
            self.repair_attempts = self.repair_attempts.saturating_add(1);
            self.store
                .execution_accounting_store()
                .map_err(RuntimeError::from)?
                .record_repair(&self.session_id)?;
            self.mark_task_ready_for_repair(&verification.summary)?;
            return self.stop(
                AutonomousSessionStateV1::Queued,
                "verification_or_diff_incomplete",
            );
        }
        if self.verify_task_output_attribution().is_err() {
            self.repair_attempts = self.repair_attempts.saturating_add(1);
            self.store
                .append_event(
                    &self.session_id,
                    "task_output_attribution_failed",
                    "REQUIRED_OUTPUT_UNCHANGED_OR_MISSING",
                )
                .map_err(RuntimeError::from)?;
            self.store
                .execution_accounting_store()
                .map_err(RuntimeError::from)?
                .record_repair(&self.session_id)?;
            self.mark_task_ready_for_repair("required task outputs were unchanged or missing")?;
            return self.stop(
                AutonomousSessionStateV1::Queued,
                "task_output_attribution_incomplete",
            );
        }
        // A structured-output completion must commit byte authority before a
        // passed checkpoint exists. A lease-expired recovery will therefore
        // never resample mutable workspace inputs or create a new snapshot.
        let expected_snapshot = match self.reviewed_source_snapshot_expected() {
            Ok(expected) => expected,
            Err(_) => {
                return self.post_verification_finalization_failure(
                    "reviewed_source_snapshot_unavailable",
                );
            }
        };
        let checkpoint = match self.passed_verification_checkpoint(
            self.provider.provider_id().as_str(),
            verification,
            diff,
            expected_snapshot.as_ref(),
        ) {
            Ok(checkpoint) => checkpoint,
            Err(_) => {
                return self.post_verification_finalization_failure(
                    "passed_verification_checkpoint_unavailable",
                );
            }
        };
        if self
            .store
            .write_passed_verification_checkpoint(&self.session_id, &checkpoint)
            .is_err()
        {
            return self.post_verification_finalization_failure(
                "passed_verification_checkpoint_unavailable",
            );
        }
        self.finalize_passed_verification(
            checkpoint,
            now_unix,
            batch.events.len(),
            self.provider.provider_id().as_str(),
        )
    }

    /// Claim one approved dependency-ready task for direct ChatGPT execution
    /// without launching a provider. The immutable task-output baseline and
    /// execution-accounting record are persisted before the task is exposed
    /// as direct work, so later finalization cannot attribute pre-existing
    /// workspace output to ChatGPT.
    pub fn claim_direct_chatgpt_work(
        &mut self,
        now_unix: u64,
    ) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
        let mut snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        if snapshot.state != AutonomousSessionStateV1::Queued
            || !snapshot.active
            || snapshot.current_task_id.is_some()
            || snapshot.provider_turn_count != 0
            || snapshot.approved_contract_hash.as_deref() != Some(self.policy.contract_hash())
        {
            return Err(RuntimeError::Validation(
                "direct ChatGPT claim requires one approved, never-started QUEUED session".into(),
            ));
        }
        if !self.policy.contract().autonomy_lease.valid_at(now_unix) {
            return Err(RuntimeError::Validation(
                "direct ChatGPT claim requires a valid autonomy lease".into(),
            ));
        }
        let graph_materialization = if self.policy.contract().task_graph.is_empty() {
            None
        } else {
            Some(self.load_exact_graph_materialization()?)
        };
        let queue = match graph_materialization.as_ref() {
            Some((queue, _)) => queue.clone(),
            None => self
                .store
                .load_queue(&self.session_id)
                .map_err(RuntimeError::from)?,
        };
        let task = queue.next_ready_task().ok_or_else(|| {
            RuntimeError::Validation("no dependency-satisfied autonomous task is ready".into())
        })?;
        if self
            .policy
            .contract()
            .task_graph
            .iter()
            .find(|spec| spec.task_id == task.task_id)
            .and_then(|spec| spec.planner_gate.as_ref())
            .is_some()
            && !self
                .store
                .planner_gate_satisfied(&self.session_id, &task.task_id)
                .map_err(RuntimeError::from)?
        {
            return Err(RuntimeError::Validation(
                "direct ChatGPT claim cannot bypass an unsatisfied planner gate".into(),
            ));
        }

        // This must precede every direct-work ownership mutation. A crash
        // after the immutable baseline is durable is recoverable; a missing
        // baseline after ownership begins fails closed in attribution checks.
        self.capture_task_output_baseline(&task.task_id)?;

        snapshot.current_task_id = Some(task.task_id.clone());
        snapshot.provider_thread_id = None;
        snapshot.provider_handle_id = None;
        snapshot.provider_event_cursor = 0;
        snapshot.retry_not_before_unix = None;
        snapshot.rate_limited_since_unix = None;
        snapshot.provider_route = AutonomousProviderRouteV1::WaitingForChatgpt;
        snapshot.repair_attempts = 0;
        snapshot.provider_turn_count = snapshot.provider_turn_count.saturating_add(1);
        snapshot.state = AutonomousSessionStateV1::WaitingForChatgpt;
        snapshot.active = true;
        self.store
            .save_session(&snapshot)
            .map_err(RuntimeError::from)?;
        self.store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .record_task_started(
                &self.policy.contract().project_id,
                &task.task_id,
                &self.session_id,
                None,
                DIRECT_CHATGPT_EXECUTOR_ID,
                None,
                None,
                controller_now_millis(),
            )?;
        self.store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .activity_started(
                &self.session_id,
                ActivityActorV1::ChatgptWebActive,
                ActivityEvidenceV1::ChatgptExactTargetGeneratingObservation,
                controller_now_millis(),
            )?;
        self.set_task_state(
            &task.task_id,
            super::autonomy_state::AutonomousQueueTaskStateV1::WorkerRunning,
        )?;
        self.store
            .append_event(
                &self.session_id,
                "direct_chatgpt_work_claimed",
                "approved task baseline captured before direct ChatGPT execution",
            )
            .map_err(RuntimeError::from)?;
        Ok(AutonomousControllerOutcomeV1 {
            state: AutonomousSessionStateV1::WaitingForChatgpt,
            provider_events: 0,
            verification: None,
            authoritative_diff: None,
            final_review: None,
        })
    }

    /// Finalize work performed directly by ChatGPT after CatDesk has escalated
    /// an already-started logical task. This path never launches or resumes a
    /// provider. It reuses the original task/output baseline and converges on
    /// the exact same verification, checkpoint, final-review, queue, and
    /// review-inbox machinery as Codex/Qwen completion.
    pub fn finalize_direct_chatgpt_work(
        &mut self,
        now_unix: u64,
    ) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
        let snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        if snapshot.state != AutonomousSessionStateV1::WaitingForChatgpt
            || snapshot.current_task_id.is_none()
            || snapshot.provider_turn_count == 0
        {
            return Err(RuntimeError::Validation(
                "direct ChatGPT finalization requires one already-started WAITING_FOR_CHATGPT task"
                    .into(),
            ));
        }

        // A first-class direct claim opens this actor span before ChatGPT
        // edits. Takeover of a previously provider-started task has no such
        // span, and closing a non-existent actor is intentionally a no-op.
        self.store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .activity_finished(
                &self.session_id,
                ActivityActorV1::ChatgptWebActive,
                controller_now_millis(),
            )?;

        if let Some(checkpoint) = self
            .store
            .load_passed_verification_checkpoint(&self.session_id)
            .map_err(RuntimeError::from)?
        {
            if checkpoint.provider_id != DIRECT_CHATGPT_EXECUTOR_ID {
                return Err(RuntimeError::Validation(
                    "direct ChatGPT finalization found a checkpoint owned by another executor"
                        .into(),
                ));
            }
            return self.finalize_passed_verification(
                checkpoint,
                now_unix,
                0,
                DIRECT_CHATGPT_EXECUTOR_ID,
            );
        }

        self.transition(
            AutonomousSessionStateV1::Verifying,
            "direct_chatgpt_verification_started",
        )?;
        self.set_current_task_state(super::autonomy_state::AutonomousQueueTaskStateV1::Verifying)?;
        let accounting = self
            .store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?;
        accounting.record_verification_started(&self.session_id, controller_now_millis())?;
        accounting.activity_started(
            &self.session_id,
            ActivityActorV1::CatdeskVerificationReviewActive,
            ActivityEvidenceV1::VerifierLifecycle,
            controller_now_millis(),
        )?;
        let (verification, diff) = self.verifier.verify()?;
        accounting.record_verification_finished(
            &self.session_id,
            match verification.status {
                VerificationStatusV1::Passed => "PASSED",
                _ => "FAILED",
            },
            &verification.summary,
            controller_now_millis(),
        )?;
        accounting.activity_finished(
            &self.session_id,
            ActivityActorV1::CatdeskVerificationReviewActive,
            controller_now_millis(),
        )?;
        if verification.status != VerificationStatusV1::Passed || diff.trim().is_empty() {
            self.set_current_task_state(super::autonomy_state::AutonomousQueueTaskStateV1::Ready)?;
            return self.stop(
                AutonomousSessionStateV1::WaitingForChatgpt,
                "direct_chatgpt_verification_incomplete",
            );
        }
        if self.verify_task_output_attribution().is_err() {
            self.set_current_task_state(super::autonomy_state::AutonomousQueueTaskStateV1::Ready)?;
            return self.stop(
                AutonomousSessionStateV1::WaitingForChatgpt,
                "direct_chatgpt_output_attribution_incomplete",
            );
        }

        let expected_snapshot = match self.reviewed_source_snapshot_expected() {
            Ok(expected) => expected,
            Err(_) => {
                return self.post_verification_finalization_failure(
                    "reviewed_source_snapshot_unavailable",
                );
            }
        };
        let checkpoint = match self.passed_verification_checkpoint(
            DIRECT_CHATGPT_EXECUTOR_ID,
            verification,
            diff,
            expected_snapshot.as_ref(),
        ) {
            Ok(checkpoint) => checkpoint,
            Err(_) => {
                return self.post_verification_finalization_failure(
                    "passed_verification_checkpoint_unavailable",
                );
            }
        };
        if self
            .store
            .write_passed_verification_checkpoint(&self.session_id, &checkpoint)
            .is_err()
        {
            return self.post_verification_finalization_failure(
                "passed_verification_checkpoint_unavailable",
            );
        }
        self.finalize_passed_verification(checkpoint, now_unix, 0, DIRECT_CHATGPT_EXECUTOR_ID)
    }

    fn passed_verification_checkpoint(
        &self,
        executor_id: &str,
        verification: VerificationSummaryV1,
        authoritative_diff: String,
        expected_snapshot: Option<&ReviewedSourceSnapshotExpectedV1>,
    ) -> Result<AutonomousPassedVerificationCheckpointV1, RuntimeError> {
        let snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        let logical_task_id = snapshot.current_task_id.clone().ok_or_else(|| {
            RuntimeError::Validation("passed verification has no current task binding".into())
        })?;
        if verification.status != VerificationStatusV1::Passed
            || authoritative_diff.trim().is_empty()
            || snapshot.provider_turn_count == 0
        {
            return Err(RuntimeError::Validation(
                "passed verification binding is unavailable or mismatched".into(),
            ));
        }
        let attribution_material = serde_json::to_vec(&(
            self.policy.contract_hash(),
            &logical_task_id,
            expected_snapshot,
        ))
        .map_err(|_| {
            RuntimeError::Validation("passed verification attribution is unavailable".into())
        })?;
        let mut diff_hasher = Sha256::new();
        diff_hasher.update(authoritative_diff.as_bytes());
        let mut attribution_hasher = Sha256::new();
        attribution_hasher.update(&attribution_material);
        Ok(AutonomousPassedVerificationCheckpointV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            session_id: self.session_id.clone(),
            project_id: self.policy.contract().project_id.clone(),
            approved_contract_hash: self.policy.contract_hash().into(),
            logical_task_id,
            provider_id: executor_id.into(),
            provider_turn_count: snapshot.provider_turn_count,
            verification_profile: self.policy.contract().verification_policy.profile.clone(),
            verification,
            authoritative_diff,
            authoritative_diff_sha256: format!("{:x}", diff_hasher.finalize()),
            attribution_digest: format!("{:x}", attribution_hasher.finalize()),
            reviewed_source_expectation: expected_snapshot.cloned(),
        })
    }

    fn finalize_passed_verification(
        &mut self,
        checkpoint: AutonomousPassedVerificationCheckpointV1,
        now_unix: u64,
        provider_events: usize,
        expected_executor_id: &str,
    ) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
        let snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        if checkpoint.session_id != self.session_id
            || checkpoint.project_id != self.policy.contract().project_id
            || checkpoint.approved_contract_hash != self.policy.contract_hash()
            || snapshot.current_task_id.as_deref() != Some(checkpoint.logical_task_id.as_str())
            || snapshot.provider_turn_count != checkpoint.provider_turn_count
            || checkpoint.provider_id != expected_executor_id
            || checkpoint.verification_profile != self.policy.contract().verification_policy.profile
            || checkpoint.verification.status != VerificationStatusV1::Passed
        {
            return self.post_verification_finalization_failure(
                "passed_verification_checkpoint_mismatched",
            );
        }

        if let Some(expected) = &checkpoint.reviewed_source_expectation {
            if !self.checkpointed_reviewed_source_matches(expected) {
                return self.post_verification_finalization_failure(
                    "passed_verification_output_mismatched",
                );
            }
            if create_or_validate_reviewed_source_snapshot(
                &self.policy.contract().workspace,
                expected,
            )
            .is_err()
            {
                return self.post_verification_finalization_failure(
                    "reviewed_source_snapshot_unavailable",
                );
            }
        }

        let another_task_ready = match self.mark_task_completed() {
            Ok(ready) => ready,
            Err(_) => {
                return self
                    .post_verification_finalization_failure("post_verification_queue_unavailable");
            }
        };
        if another_task_ready {
            if self
                .store
                .execution_accounting_store()
                .map_err(RuntimeError::from)?
                .finish_task(
                    &self.session_id,
                    Some("authoritative-diff: task-verified"),
                    None,
                    controller_now_millis(),
                )
                .is_err()
            {
                return self.post_verification_finalization_failure(
                    "post_verification_accounting_unavailable",
                );
            }
            if self
                .store
                .clear_passed_verification_checkpoint(&self.session_id)
                .is_err()
            {
                return self.post_verification_finalization_failure(
                    "passed_verification_checkpoint_unavailable",
                );
            }
            return self.stop(
                AutonomousSessionStateV1::Queued,
                "task_verified_next_task_queued",
            );
        }

        let final_review = match self
            .verifier
            .final_review(&checkpoint.verification, &checkpoint.authoritative_diff)
        {
            Ok(review)
                if !self
                    .policy
                    .contract()
                    .verification_policy
                    .require_final_review
                    || !review.trim().is_empty() =>
            {
                review
            }
            _ => {
                return self.post_verification_finalization_failure("final_review_unavailable");
            }
        };
        if self
            .store
            .write_completion_artifacts(
                &self.session_id,
                &checkpoint.verification,
                &checkpoint.authoritative_diff,
                &final_review,
            )
            .is_err()
        {
            return self.post_verification_finalization_failure("completion_artifacts_unavailable");
        }
        if self
            .store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .finish_task(
                &self.session_id,
                Some("artifacts/completion.json#authoritativeDiff"),
                Some("artifacts/completion.json#finalReview"),
                controller_now_millis(),
            )
            .is_err()
        {
            return self.post_verification_finalization_failure(
                "post_verification_accounting_unavailable",
            );
        }
        if self
            .store
            .emit_review_inbox_record(
                &self.session_id,
                &self.policy.contract().project_id,
                AutonomousSessionStateV1::CompletedVerified,
                "independent_final_review",
                "artifacts/completion.json",
                now_unix,
            )
            .is_err()
        {
            return self.post_verification_finalization_failure("review_inbox_unavailable");
        }
        if self
            .transition(
                AutonomousSessionStateV1::CompletedVerified,
                "completed_verified",
            )
            .is_err()
        {
            return self
                .post_verification_finalization_failure("completion_transition_unavailable");
        }
        Ok(AutonomousControllerOutcomeV1 {
            state: AutonomousSessionStateV1::CompletedVerified,
            provider_events,
            verification: Some(checkpoint.verification),
            authoritative_diff: Some(checkpoint.authoritative_diff),
            final_review: Some(final_review),
        })
    }

    fn post_verification_finalization_failure(
        &self,
        reason: &str,
    ) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
        let _ = self.store.append_event(
            &self.session_id,
            "post_verification_finalization_pending",
            reason,
        );
        let _ = self.store.write_escalation(
            &self.session_id,
            AutonomousSessionStateV1::WaitingForChatgpt,
            reason,
        );
        self.stop(AutonomousSessionStateV1::WaitingForChatgpt, reason)
    }

    fn transition(&self, state: AutonomousSessionStateV1, event: &str) -> Result<(), RuntimeError> {
        let orchestration_started = controller_now_millis();
        let mut snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        snapshot.state = state.clone();
        snapshot.active = !state.is_terminal();
        self.store
            .save_session(&snapshot)
            .map_err(RuntimeError::from)?;
        self.store
            .append_event(&self.session_id, event, "autonomous controller transition")
            .map_err(RuntimeError::from)?;
        if matches!(
            state,
            AutonomousSessionStateV1::Queued
                | AutonomousSessionStateV1::WaitingForChatgpt
                | AutonomousSessionStateV1::WaitingForUser
                | AutonomousSessionStateV1::RateLimited
                | AutonomousSessionStateV1::Paused
                | AutonomousSessionStateV1::Blocked
        ) {
            self.store
                .execution_accounting_store()
                .map_err(RuntimeError::from)?
                .waiting_started(&self.session_id, controller_now_millis())?;
        }
        let orchestration_finished = controller_now_millis();
        let accounting = self
            .store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?;
        accounting.activity_started(
            &self.session_id,
            ActivityActorV1::CatdeskOrchestrationActive,
            ActivityEvidenceV1::ControllerTransition,
            orchestration_started,
        )?;
        accounting.activity_finished(
            &self.session_id,
            ActivityActorV1::CatdeskOrchestrationActive,
            orchestration_finished,
        )?;
        Ok(())
    }

    fn qwen_tools_for(
        &mut self,
        task_id: &str,
    ) -> Result<&mut AutonomousQwenToolLoopV1, RuntimeError> {
        if self.qwen_tools.is_none() {
            self.qwen_tools = Some(AutonomousQwenToolLoopV1::recover(
                self.policy.clone(),
                &self.session_id,
                task_id,
            )?);
        }
        self.qwen_tools
            .as_mut()
            .ok_or_else(|| RuntimeError::Validation("Qwen tool loop initialization failed".into()))
    }

    fn load_exact_graph_materialization(
        &self,
    ) -> Result<(AutonomousQueueV1, AutonomousPlanQueueV1), RuntimeError> {
        let queue = self
            .store
            .load_queue(&self.session_id)
            .map_err(RuntimeError::from)?;
        let plan = self
            .store
            .load_plan_queue(&self.session_id)
            .map_err(RuntimeError::from)?;
        validate_exact_task_graph_materialization(self.policy.contract(), &queue, &plan)?;
        Ok((queue, plan))
    }

    fn approved_completion_artifact_ids(&self, task_id: &str) -> Result<Vec<String>, RuntimeError> {
        let contract = self.policy.contract();
        if contract.task_graph.is_empty() {
            if contract.completion_artifact_ids.is_empty() {
                return Ok(Vec::new());
            }
            if task_id != contract.task_id {
                return Err(RuntimeError::Validation(
                    "legacy task output attribution task does not match contract".into(),
                ));
            }
            return Ok(contract.completion_artifact_ids.clone());
        }
        contract
            .task_graph
            .iter()
            .find(|task| task.task_id == task_id)
            .map(|task| task.completion_artifact_ids.clone())
            .ok_or_else(|| {
                RuntimeError::Validation("task output attribution task is not approved".into())
            })
    }

    fn safe_task_output_hash(&self, artifact_id: &str) -> Result<Option<String>, RuntimeError> {
        safe_task_output_hash(&self.policy, artifact_id)
    }

    fn capture_task_output_baseline(&self, task_id: &str) -> Result<(), RuntimeError> {
        let artifact_ids = self.approved_completion_artifact_ids(task_id)?;
        if artifact_ids.is_empty() {
            return Ok(());
        }
        let expected_authority = AutonomousTaskOutputBaselineAuthorityV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            session_id: self.session_id.clone(),
            task_id: task_id.into(),
            approved_contract_hash: self.policy.contract_hash().into(),
            completion_artifact_ids: artifact_ids.clone(),
        };
        let authority = self
            .store
            .load_task_output_baseline_authority(&self.session_id, task_id)
            .map_err(RuntimeError::from)?;
        if authority
            .as_ref()
            .is_some_and(|authority| authority != &expected_authority)
        {
            return Err(RuntimeError::Validation(
                "task output baseline authority binding drifted".into(),
            ));
        }
        if let Some(baseline) = self
            .store
            .load_task_output_baseline(&self.session_id, task_id)
            .map_err(RuntimeError::from)?
        {
            if baseline.approved_contract_hash != self.policy.contract_hash()
                || baseline.completion_artifact_ids != artifact_ids
            {
                return Err(RuntimeError::Validation(
                    "task output baseline binding drifted".into(),
                ));
            }
            // A crash after the baseline's atomic write but before its
            // separate authority marker is recoverable: no provider launch
            // follows until this exact immutable marker is persisted.
            if authority.is_none() {
                self.store
                    .save_task_output_baseline_authority(&self.session_id, &expected_authority)
                    .map_err(RuntimeError::from)?;
            }
            return Ok(());
        }
        if authority.is_some() || self.task_was_previously_launched(task_id)? {
            return Err(RuntimeError::Validation(
                "task output baseline authority is missing after prior launch".into(),
            ));
        }
        let observations = artifact_ids
            .iter()
            .map(|artifact_id| {
                let hash = self.safe_task_output_hash(artifact_id)?;
                Ok(AutonomousTaskOutputObservationV1 {
                    artifact_id: artifact_id.clone(),
                    state: if hash.is_some() {
                        "PRESENT_SHA256".into()
                    } else {
                        "ABSENT".into()
                    },
                    sha256: hash,
                })
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        self.store
            .save_task_output_baseline(
                &self.session_id,
                &AutonomousTaskOutputBaselineV1 {
                    schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
                    session_id: self.session_id.clone(),
                    task_id: task_id.into(),
                    approved_contract_hash: self.policy.contract_hash().into(),
                    completion_artifact_ids: artifact_ids,
                    observations,
                },
            )
            .map_err(RuntimeError::from)?;
        // This marker is written after the exact baseline.  A crash before
        // this write can only be recovered by reusing that baseline; a crash
        // after it makes a later missing/corrupt baseline non-runnable.
        self.store
            .save_task_output_baseline_authority(&self.session_id, &expected_authority)
            .map_err(RuntimeError::from)
    }

    fn task_was_previously_launched(&self, task_id: &str) -> Result<bool, RuntimeError> {
        let snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        Ok(
            snapshot.current_task_id.as_deref() == Some(task_id)
                && snapshot.provider_turn_count > 0,
        )
    }

    fn reviewed_source_snapshot_expected(
        &self,
    ) -> Result<Option<ReviewedSourceSnapshotExpectedV1>, RuntimeError> {
        let session = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        let task_id = session.current_task_id.ok_or_else(|| {
            RuntimeError::Validation("reviewed source snapshot has no current task".into())
        })?;
        let artifact_ids = self.approved_completion_artifact_ids(&task_id)?;
        if artifact_ids.is_empty() {
            return Ok(None);
        }
        let baseline = self
            .store
            .load_task_output_baseline(&self.session_id, &task_id)
            .map_err(RuntimeError::from)?
            .ok_or_else(|| RuntimeError::Validation("task output baseline is missing".into()))?;
        if baseline.session_id != self.session_id
            || baseline.task_id != task_id
            || baseline.approved_contract_hash != self.policy.contract_hash()
            || baseline.completion_artifact_ids != artifact_ids
        {
            return Err(RuntimeError::Validation(
                "task output baseline binding drifted".into(),
            ));
        }
        let current_outputs = artifact_ids
            .iter()
            .map(|artifact_id| {
                self.safe_task_output_hash(artifact_id)?
                    .ok_or_else(|| {
                        RuntimeError::Validation("task output attribution is incomplete".into())
                    })
                    .map(|sha256| ReviewedSourceCurrentOutputV1 {
                        artifact_id: artifact_id.clone(),
                        sha256,
                    })
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        let baseline_observations = baseline
            .observations
            .into_iter()
            .map(|observation| ReviewedSourceBaselineObservationV1 {
                artifact_id: observation.artifact_id,
                state: observation.state,
                sha256: observation.sha256,
            })
            .collect();
        Ok(Some(ReviewedSourceSnapshotExpectedV1 {
            session_id: self.session_id.clone(),
            project_id: self.policy.contract().project_id.clone(),
            approved_contract_hash: self.policy.contract_hash().into(),
            logical_task_id: task_id,
            completion_artifact_ids: artifact_ids,
            current_outputs,
            baseline_observations,
        }))
    }

    /// Recovery reads only the output identities bound into the durable
    /// checkpoint.  It does not consult the baseline or construct a fresh
    /// expectation, so a completed provider turn cannot be replayed through
    /// mutable task output state.
    fn checkpointed_reviewed_source_matches(
        &self,
        expected: &ReviewedSourceSnapshotExpectedV1,
    ) -> bool {
        if expected.session_id != self.session_id
            || expected.project_id != self.policy.contract().project_id
            || expected.approved_contract_hash != self.policy.contract_hash()
        {
            return false;
        }
        expected.current_outputs.iter().all(|output| {
            self.safe_task_output_hash(&output.artifact_id)
                .ok()
                .flatten()
                .as_deref()
                == Some(output.sha256.as_str())
        })
    }

    fn verify_task_output_attribution(&self) -> Result<(), RuntimeError> {
        let task_id = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?
            .current_task_id
            .ok_or_else(|| {
                RuntimeError::Validation("task output attribution has no current task".into())
            })?;
        let artifact_ids = self.approved_completion_artifact_ids(&task_id)?;
        if artifact_ids.is_empty() {
            return Ok(());
        }
        let baseline = self
            .store
            .load_task_output_baseline(&self.session_id, &task_id)
            .map_err(RuntimeError::from)?
            .ok_or_else(|| RuntimeError::Validation("task output baseline is missing".into()))?;
        if baseline.approved_contract_hash != self.policy.contract_hash()
            || baseline.completion_artifact_ids != artifact_ids
        {
            return Err(RuntimeError::Validation(
                "task output baseline binding drifted".into(),
            ));
        }
        for observation in baseline.observations {
            let now = self.safe_task_output_hash(&observation.artifact_id)?;
            let attributed = match (
                observation.state.as_str(),
                observation.sha256.as_deref(),
                now,
            ) {
                ("ABSENT", None, Some(_)) => true,
                ("PRESENT_SHA256", Some(before), Some(after)) => before != after,
                _ => false,
            };
            if !attributed {
                return Err(RuntimeError::Validation(
                    "task output attribution is incomplete".into(),
                ));
            }
        }
        Ok(())
    }

    fn stop(
        &self,
        state: AutonomousSessionStateV1,
        event: &str,
    ) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
        self.transition(state.clone(), event)?;
        Ok(AutonomousControllerOutcomeV1 {
            state,
            provider_events: 0,
            verification: None,
            authoritative_diff: None,
            final_review: None,
        })
    }

    fn escalate(&self, reason: &str) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
        self.store
            .write_escalation(
                &self.session_id,
                AutonomousSessionStateV1::WaitingForChatgpt,
                reason,
            )
            .map_err(RuntimeError::from)?;
        let outcome = self.stop(AutonomousSessionStateV1::WaitingForChatgpt, reason)?;
        self.store
            .emit_review_inbox_record(
                &self.session_id,
                &self.policy.contract().project_id,
                AutonomousSessionStateV1::WaitingForChatgpt,
                "chatgpt_decision_required",
                "artifacts/escalation.json",
                controller_now_unix(),
            )
            .map_err(RuntimeError::from)?;
        Ok(outcome)
    }

    pub async fn cancel(&mut self) -> Result<(), RuntimeError> {
        self.cancel_requested = true;
        let mut snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        snapshot.cancellation_requested = true;
        self.store
            .save_session(&snapshot)
            .map_err(RuntimeError::from)?;
        if let Some(handle) = self.last_handle.clone() {
            if self.provider.capabilities().supports_cancellation {
                self.provider.cancel(&handle).await?;
            }
        }
        self.transition(
            AutonomousSessionStateV1::Cancelled,
            "cancellation_requested",
        )?;
        self.store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .activity_finished(
                &self.session_id,
                ActivityActorV1::CodexProviderActive,
                controller_now_millis(),
            )
    }

    fn prepare_turn(
        &self,
        task_id: &str,
        provider_session_id: &str,
        expected_codex_thread_id: Option<&str>,
    ) -> Result<(), RuntimeError> {
        let mut snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        snapshot.current_task_id = Some(task_id.into());
        snapshot.provider_thread_id = Some(provider_session_id.into());
        snapshot.expected_codex_thread_id = expected_codex_thread_id.map(str::to_owned);
        snapshot.provider_handle_id = None;
        snapshot.provider_event_cursor = 0;
        snapshot.retry_not_before_unix = None;
        snapshot.rate_limited_since_unix = None;
        if self.provider.provider_id() == ProviderIdV1::CodexCli {
            snapshot.provider_route = AutonomousProviderRouteV1::CodexPreferred;
        }
        snapshot.repair_attempts = self.repair_attempts;
        snapshot.provider_turn_count = snapshot.provider_turn_count.saturating_add(1);
        snapshot.state = AutonomousSessionStateV1::Running;
        self.store
            .save_session(&snapshot)
            .map_err(RuntimeError::from)?;
        self.store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .record_task_started(
                &self.policy.contract().project_id,
                task_id,
                &self.session_id,
                Some(provider_session_id),
                self.provider.provider_id().as_str(),
                if self.provider.provider_id() == ProviderIdV1::Ollama {
                    Some(&self.policy.contract().provider_policy.routine_model)
                } else {
                    Some(&self.policy.contract().provider_policy.primary_model)
                },
                snapshot.codex_routing_telemetry.clone(),
                controller_now_millis(),
            )?;
        self.set_task_state(
            task_id,
            super::autonomy_state::AutonomousQueueTaskStateV1::WorkerRunning,
        )
    }

    /// Returns true only when a canonical-bound Codex turn reports a different
    /// identity. The comparison is intentionally independent from task or
    /// repair state so a new DAG task receives the same protection.
    fn codex_thread_identity_mismatch(
        &self,
        observed_thread_id: &str,
    ) -> Result<bool, RuntimeError> {
        let snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        Ok(self.provider.provider_id() == ProviderIdV1::CodexCli
            && snapshot
                .expected_codex_thread_id
                .as_deref()
                .is_some_and(|expected| expected != observed_thread_id))
    }

    /// A resumed provider may never replace a host-preflighted project binding.
    /// Persist only a bounded, non-secret diagnostic and move to the normal
    /// ChatGPT escalation state; callers return immediately and cannot launch
    /// a second provider turn.
    fn fail_closed_codex_thread_mismatch(
        &self,
    ) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
        self.store
            .append_event(
                &self.session_id,
                "codex_thread_identity_mismatch",
                "provider-reported Codex thread differed from the expected canonical binding",
            )
            .map_err(RuntimeError::from)?;
        self.escalate("codex_provider_thread_identity_mismatch")
    }

    fn persist_handle(
        &self,
        handle: &ProviderTurnHandleV1,
        cursor: u64,
    ) -> Result<(), RuntimeError> {
        let mut snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        if handle.provider_id == ProviderIdV1::CodexCli
            && snapshot
                .expected_codex_thread_id
                .as_deref()
                .is_some_and(|expected| expected != handle.provider_session_id)
        {
            return Err(RuntimeError::Validation(
                "refusing to persist a Codex handle with a mismatched canonical thread".into(),
            ));
        }
        snapshot.provider_thread_id = Some(handle.provider_session_id.clone());
        snapshot.provider_handle_id = Some(handle.handle_id.clone());
        snapshot.provider_event_cursor = cursor;
        snapshot.state = AutonomousSessionStateV1::Running;
        snapshot.active = true;
        self.store
            .save_session(&snapshot)
            .map_err(RuntimeError::from)
    }

    fn pause_for_rate_limit(
        &self,
        now_unix: u64,
        provider_events: usize,
    ) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
        let mut snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        snapshot.state = AutonomousSessionStateV1::RateLimited;
        snapshot.rate_limited_since_unix =
            Some(snapshot.rate_limited_since_unix.unwrap_or(now_unix));
        if self.provider.provider_id() == ProviderIdV1::CodexCli {
            snapshot.provider_route = AutonomousProviderRouteV1::CodexTransientRateLimited;
        }
        snapshot.retry_not_before_unix = Some(
            now_unix.saturating_add(
                self.policy
                    .contract()
                    .rate_limit_policy
                    .initial_backoff_seconds,
            ),
        );
        self.store
            .save_session(&snapshot)
            .map_err(RuntimeError::from)?;
        self.set_current_task_state(super::autonomy_state::AutonomousQueueTaskStateV1::Ready)?;
        self.store
            .append_event(
                &self.session_id,
                "provider_rate_limited",
                "provider capacity pause persisted",
            )
            .map_err(RuntimeError::from)?;
        self.store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .activity_finished(
                &self.session_id,
                ActivityActorV1::CodexProviderActive,
                controller_now_millis(),
            )?;
        self.store
            .execution_accounting_store()
            .map_err(RuntimeError::from)?
            .waiting_started(&self.session_id, controller_now_millis())?;
        Ok(AutonomousControllerOutcomeV1 {
            state: AutonomousSessionStateV1::RateLimited,
            provider_events,
            verification: None,
            authoritative_diff: None,
            final_review: None,
        })
    }

    async fn handoff_to_qwen(
        &mut self,
        now_unix: u64,
        provider_events: usize,
        reset_boundary: Option<u64>,
    ) -> Result<AutonomousControllerOutcomeV1, RuntimeError> {
        // The handoff is written before attempting local health.  A restart
        // can therefore safely resume the same task or escalate, but never
        // relaunch the failed Codex turn.
        let snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        let task_id = snapshot.current_task_id.clone().ok_or_else(|| {
            RuntimeError::Validation("credit exhaustion occurred without a durable task id".into())
        })?;
        let queue = self
            .store
            .load_queue(&self.session_id)
            .map_err(RuntimeError::from)?;
        let completed_task_ids = queue
            .tasks
            .iter()
            .filter(|task| {
                task.state == super::autonomy_state::AutonomousQueueTaskStateV1::CompletedVerified
            })
            .map(|task| task.task_id.clone())
            .collect();
        let git = capture_authoritative_git_evidence(&self.policy.contract().workspace)?;
        let handoff = AutonomousProviderHandoffV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            session_id: self.session_id.clone(),
            task_id: task_id.clone(),
            from_provider_id: "codex-cli".into(),
            to_provider_id: "ollama".into(),
            reason: "codex_credits_exhausted".into(),
            created_at_unix: now_unix,
            codex_thread_id: snapshot.provider_thread_id.clone(),
            objective: bounded_contract_text(&self.policy.contract().objective, 8_000),
            ordered_steps: self
                .policy
                .contract()
                .ordered_steps
                .iter()
                .take(20)
                .map(|step| bounded_contract_text(step, 512))
                .collect(),
            acceptance_criteria: self
                .policy
                .contract()
                .ordered_steps
                .iter()
                .take(20)
                .map(|step| bounded_contract_text(step, 512))
                .collect(),
            allowed_paths: self
                .policy
                .contract()
                .allowed_paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            forbidden_paths: self
                .policy
                .contract()
                .forbidden_paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            base_branch: bounded_contract_text(&self.policy.contract().base_branch, 256),
            feature_branch: bounded_contract_text(&self.policy.contract().feature_branch, 256),
            base_commit: bounded_contract_text(&self.policy.contract().base_commit, 128),
            expected_origin: bounded_contract_text(&self.policy.contract().expected_origin, 1_024),
            changed_files: git.changed_files,
            git_status_porcelain: git.status_porcelain,
            last_verification_summary: snapshot
                .last_verification_summary
                .clone()
                .map(|value| bounded_contract_text(&value, 2_048)),
            completed_task_ids,
            pending_task_id: task_id.clone(),
            remaining_provider_turns: self
                .policy
                .contract()
                .autonomy_lease
                .maximum_provider_turns
                .saturating_sub(snapshot.provider_turn_count),
            remaining_repair_cycles: self
                .policy
                .contract()
                .verification_policy
                .max_repair_cycles
                .saturating_sub(snapshot.repair_attempts),
            authoritative_diff_hash: Some(git.diff_hash),
            completed_tool_calls: Vec::new(),
            pending_tool_calls: Vec::new(),
        };
        self.store
            .write_provider_handoff(&self.session_id, &handoff)
            .map_err(RuntimeError::from)?;
        let mut routing = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        let observed_reset = routing
            .codex_routing_telemetry
            .as_ref()
            .filter(|telemetry| telemetry.reached_limit)
            .and_then(|telemetry| telemetry.codex_eligible_after_unix);
        // Do not invent a Codex re-eligibility boundary. A provider-attested
        // account telemetry reset is durable across recovery; if it is absent,
        // Qwen remains the only safe route and Codex is never reprobed.
        routing.codex_eligible_after_unix = reset_boundary
            .or(routing.codex_eligible_after_unix)
            .or(observed_reset);
        routing.provider_route = AutonomousProviderRouteV1::CodexCreditsExhausted;
        self.store
            .save_session(&routing)
            .map_err(RuntimeError::from)?;
        self.store
            .append_event(
                &self.session_id,
                "codex_credits_exhausted",
                "bounded local-provider handoff checkpoint persisted",
            )
            .map_err(RuntimeError::from)?;

        if !self
            .provider
            .activate_local_qwen(&self.policy.contract().provider_policy.routine_model)
            .await?
        {
            self.set_provider_route(AutonomousProviderRouteV1::QwenUnavailable)?;
            self.store
                .append_event(
                    &self.session_id,
                    "qwen_unavailable",
                    "local Qwen model unavailable; no cloud fallback selected",
                )
                .map_err(RuntimeError::from)?;
            return self.escalate("qwen_unavailable_after_codex_credit_exhaustion");
        }
        let mut continued = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        continued.provider_route = AutonomousProviderRouteV1::QwenFallbackActive;
        // The active local-Qwen turn has no Codex thread, but the original
        // canonical thread remains durable for one post-reset, task-boundary
        // restoration.  It is never reconstructed from a caller value.
        continued.provider_thread_id = None;
        continued.expected_codex_thread_id = handoff.codex_thread_id.clone();
        continued.provider_handle_id = None;
        continued.provider_event_cursor = 0;
        continued.retry_not_before_unix = None;
        continued.rate_limited_since_unix = None;
        continued.state = AutonomousSessionStateV1::Queued;
        continued.active = true;
        self.store
            .save_session(&continued)
            .map_err(RuntimeError::from)?;
        self.set_task_state(
            &task_id,
            super::autonomy_state::AutonomousQueueTaskStateV1::Ready,
        )?;
        self.last_handle = None;
        self.store
            .append_event(
                &self.session_id,
                "qwen_fallback_activated",
                "same logical task queued for local Qwen continuation",
            )
            .map_err(RuntimeError::from)?;
        Ok(AutonomousControllerOutcomeV1 {
            state: AutonomousSessionStateV1::Queued,
            provider_events,
            verification: None,
            authoritative_diff: None,
            final_review: None,
        })
    }

    fn set_provider_route(&self, route: AutonomousProviderRouteV1) -> Result<(), RuntimeError> {
        let mut snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        snapshot.provider_route = route;
        self.store
            .save_session(&snapshot)
            .map_err(RuntimeError::from)
    }

    fn mark_task_ready_for_repair(&self, verification_summary: &str) -> Result<(), RuntimeError> {
        let mut queue = self
            .store
            .load_queue(&self.session_id)
            .map_err(RuntimeError::from)?;
        let task_id = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?
            .current_task_id
            .ok_or_else(|| RuntimeError::Validation("autonomous task was not persisted".into()))?;
        let task_index = queue
            .tasks
            .iter()
            .position(|task| task.task_id == task_id)
            .ok_or_else(|| {
                RuntimeError::Validation("autonomous task is absent from queue".into())
            })?;
        queue.tasks[task_index].state =
            super::autonomy_state::AutonomousQueueTaskStateV1::Repairing;
        self.store
            .save_queue(&self.session_id, &queue)
            .map_err(RuntimeError::from)?;
        queue.tasks[task_index].state = super::autonomy_state::AutonomousQueueTaskStateV1::Ready;
        self.store
            .save_queue(&self.session_id, &queue)
            .map_err(RuntimeError::from)?;
        let mut snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        snapshot.repair_attempts = self.repair_attempts;
        snapshot.last_verification_summary =
            Some(bounded_contract_text(verification_summary, 2_048));
        self.store
            .save_session(&snapshot)
            .map_err(RuntimeError::from)
    }

    /// Returns true when another dependency-satisfied task remains after the
    /// current task is durably marked verified.
    fn mark_task_completed(&self) -> Result<bool, RuntimeError> {
        let mut queue = self
            .store
            .load_queue(&self.session_id)
            .map_err(RuntimeError::from)?;
        let task_id = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?
            .current_task_id
            .ok_or_else(|| RuntimeError::Validation("autonomous task was not persisted".into()))?;
        let task = queue
            .tasks
            .iter_mut()
            .find(|task| task.task_id == task_id)
            .ok_or_else(|| {
                RuntimeError::Validation("autonomous task is absent from queue".into())
            })?;
        task.state = super::autonomy_state::AutonomousQueueTaskStateV1::CompletedVerified;
        self.store
            .save_queue(&self.session_id, &queue)
            .map_err(RuntimeError::from)?;
        if queue.tasks.iter().all(|candidate| {
            candidate.state == super::autonomy_state::AutonomousQueueTaskStateV1::CompletedVerified
        }) {
            Ok(false)
        } else if queue.next_ready_task().is_some() {
            Ok(true)
        } else {
            Err(RuntimeError::Validation(
                "unfinished autonomous queue has no dependency-satisfied task".into(),
            ))
        }
    }

    fn set_current_task_state(
        &self,
        state: super::autonomy_state::AutonomousQueueTaskStateV1,
    ) -> Result<(), RuntimeError> {
        let task_id = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?
            .current_task_id
            .ok_or_else(|| RuntimeError::Validation("autonomous task was not persisted".into()))?;
        self.set_task_state(&task_id, state)
    }

    fn set_task_state(
        &self,
        task_id: &str,
        state: super::autonomy_state::AutonomousQueueTaskStateV1,
    ) -> Result<(), RuntimeError> {
        let mut queue = self
            .store
            .load_queue(&self.session_id)
            .map_err(RuntimeError::from)?;
        let task = queue
            .tasks
            .iter_mut()
            .find(|task| task.task_id == task_id)
            .ok_or_else(|| {
                RuntimeError::Validation("autonomous task is absent from queue".into())
            })?;
        task.state = state;
        self.store
            .save_queue(&self.session_id, &queue)
            .map_err(RuntimeError::from)
    }

    fn restore_codex_at_safe_boundary(
        &mut self,
        mut snapshot: super::autonomy_state::AutonomousSessionSnapshotV1,
    ) -> Result<super::autonomy_state::AutonomousSessionSnapshotV1, RuntimeError> {
        let handoff = self
            .store
            .load_provider_handoff(&self.session_id)
            .map_err(RuntimeError::from)?;
        let codex_thread_id = handoff
            .codex_thread_id
            .filter(|thread| !thread.is_empty())
            .ok_or_else(|| {
                RuntimeError::Validation(
                    "eligible Codex restoration lacks the preserved canonical thread".into(),
                )
            })?;
        if handoff.from_provider_id != ProviderIdV1::CodexCli.as_str()
            || handoff.to_provider_id != ProviderIdV1::Ollama.as_str()
            || snapshot.current_task_id.as_deref() != Some(handoff.pending_task_id.as_str())
        {
            return Err(RuntimeError::Validation(
                "eligible Codex restoration handoff continuity is mismatched".into(),
            ));
        }
        if !self.provider.prefer_codex_at_task_boundary() {
            // The provider has not changed route, so retaining Qwen is safe
            // and cannot create a Codex probe before a future safe boundary.
            return Ok(snapshot);
        }
        snapshot.provider_route = AutonomousProviderRouteV1::CodexPreferred;
        snapshot.provider_thread_id = Some(codex_thread_id.clone());
        snapshot.expected_codex_thread_id = Some(codex_thread_id);
        snapshot.provider_handle_id = None;
        snapshot.provider_event_cursor = 0;
        snapshot.codex_eligible_after_unix = None;
        self.store
            .save_session(&snapshot)
            .map_err(RuntimeError::from)?;
        self.store
            .append_event(
                &self.session_id,
                "codex_reset_restored",
                "provider-attested reset reached; canonical Codex thread restored at task boundary",
            )
            .map_err(RuntimeError::from)?;
        Ok(snapshot)
    }
}

/// Hashes one approval-bound completion artifact using the same fail-closed
/// identity rules at baseline capture and later authority revalidation.
/// `NotFound` is the only absence state; links, reparse points, special
/// files, and metadata ambiguity are never interpreted as absence.
pub(crate) fn safe_task_output_hash(
    policy: &AutonomousPolicyEngineV1,
    artifact_id: &str,
) -> Result<Option<String>, RuntimeError> {
    let workspace = &policy.contract().workspace;
    let relative = Path::new(artifact_id);
    let target = workspace.join(relative);
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty() {
        return Err(RuntimeError::Validation(
            "task output path is not normalized".into(),
        ));
    }
    let mut cursor = workspace.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let std::path::Component::Normal(part) = component else {
            return Err(RuntimeError::Validation(
                "task output path is not normalized".into(),
            ));
        };
        cursor.push(part);
        let metadata = match fs::symlink_metadata(&cursor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                policy.permit_new_path(&target).map_err(|_| {
                    RuntimeError::Validation("task output path is outside approval".into())
                })?;
                return Ok(None);
            }
            Err(_) => {
                return Err(RuntimeError::Validation(
                    "task output metadata is unavailable".into(),
                ));
            }
        };
        if output_component_is_unsafe(&metadata, index + 1 == components.len()) {
            return Err(RuntimeError::Validation(
                "task output path is unsafe".into(),
            ));
        }
        if index + 1 == components.len() {
            policy.permit_path(&target).map_err(|_| {
                RuntimeError::Validation("task output path is outside approval".into())
            })?;
            if !metadata.file_type().is_file() || metadata.len() > MAX_TASK_OUTPUT_BYTES {
                return Err(RuntimeError::Validation(
                    "task output file is unsafe".into(),
                ));
            }
            let mut file = fs::File::open(&target)
                .map_err(|_| RuntimeError::Validation("task output file is unavailable".into()))?;
            let mut hasher = Sha256::new();
            let mut buffer = [0_u8; 8192];
            let mut total = 0_u64;
            loop {
                let count = file.read(&mut buffer).map_err(|_| {
                    RuntimeError::Validation("task output file cannot be read".into())
                })?;
                if count == 0 {
                    break;
                }
                total = total.saturating_add(count as u64);
                if total > MAX_TASK_OUTPUT_BYTES {
                    return Err(RuntimeError::Validation(
                        "task output file is oversized".into(),
                    ));
                }
                hasher.update(&buffer[..count]);
            }
            return Ok(Some(format!("{:x}", hasher.finalize())));
        }
    }
    Err(RuntimeError::Validation(
        "task output path is not normalized".into(),
    ))
}

fn output_component_is_unsafe(metadata: &fs::Metadata, is_terminal: bool) -> bool {
    output_component_flags_are_unsafe(
        metadata.file_type().is_symlink(),
        {
            #[cfg(windows)]
            {
                metadata.file_attributes() & 0x400 != 0
            }
            #[cfg(not(windows))]
            {
                false
            }
        },
        metadata.file_type().is_dir(),
        is_terminal,
    )
}

const fn output_component_flags_are_unsafe(
    is_symlink: bool,
    is_reparse: bool,
    is_directory: bool,
    is_terminal: bool,
) -> bool {
    is_symlink || is_reparse || (!is_terminal && !is_directory)
}

fn has_authoritative_terra_high_gate(
    snapshot: &super::autonomy_state::AutonomousSessionSnapshotV1,
) -> bool {
    snapshot
        .codex_routing_telemetry
        .as_ref()
        .is_some_and(|telemetry| {
            matches!(
                telemetry.source.as_str(),
                "codex-app-server/account-rateLimits-read"
                    | "codex-app-server/account-rateLimits-unavailable"
            ) && telemetry.selected_model.as_deref() == Some(CATDESK_REQUIRED_CODEX_MODEL_V1)
                && telemetry.reasoning_effort.as_deref()
                    == Some(CATDESK_REQUIRED_CODEX_REASONING_EFFORT_V1)
        })
}

fn is_rate_limit_event(event: &super::runtime::NormalizedProviderEventV1) -> bool {
    let text = event
        .text
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    event.kind == NormalizedProviderEventKind::TerminalError
        && (text.contains("rate limit")
            || text.contains("too many requests")
            || text.contains("http 429")
            || text.contains("status 429"))
}

struct AuthoritativeGitEvidenceV1 {
    status_porcelain: Vec<String>,
    changed_files: Vec<String>,
    diff_hash: String,
}

/// Captures git's own status and complete working-tree diff at the handoff
/// boundary. The durable handoff stores only bounded status/path metadata and
/// a SHA-256 digest, never the diff body or unredacted tool transcript.
fn capture_authoritative_git_evidence(
    workspace: &std::path::Path,
) -> Result<AuthoritativeGitEvidenceV1, RuntimeError> {
    let status = Command::new("git")
        .args(["status", "--porcelain=v1", "-z"])
        .current_dir(workspace)
        .output()
        .map_err(|error| {
            RuntimeError::Provider(format!("git status unavailable for handoff: {error}"))
        })?;
    // Test fixtures and explicitly non-git workspaces still retain a durable
    // "unavailable" marker; real repositories always use Git's output.
    if !status.status.success() {
        return Ok(AuthoritativeGitEvidenceV1 {
            status_porcelain: vec!["git-status-unavailable".into()],
            changed_files: Vec::new(),
            diff_hash: "sha256:git-unavailable".into(),
        });
    }
    let status_porcelain = status
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| {
            String::from_utf8_lossy(record)
                .chars()
                .take(1024)
                .collect::<String>()
        })
        .take(256)
        .collect::<Vec<_>>();
    let changed_files = status_porcelain
        .iter()
        .filter_map(|record| record.get(3..))
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
        .take(256)
        .collect();
    let diff = Command::new("git")
        .args(["diff", "--binary", "--no-ext-diff", "HEAD"])
        .current_dir(workspace)
        .output()
        .map_err(|error| {
            RuntimeError::Provider(format!("git diff unavailable for handoff: {error}"))
        })?;
    if !diff.status.success() {
        return Ok(AuthoritativeGitEvidenceV1 {
            status_porcelain,
            changed_files,
            diff_hash: "sha256:git-diff-unavailable".into(),
        });
    }
    let mut hash = Sha256::new();
    hash.update(&diff.stdout);
    Ok(AuthoritativeGitEvidenceV1 {
        status_porcelain,
        changed_files,
        diff_hash: format!("sha256:{:x}", hash.finalize()),
    })
}

/// Finds one explicit Codex credit-exhaustion diagnostic in a terminal batch.
///
/// The app-server can emit the provider-attested quota diagnostic as a
/// preceding text/diagnostic event and then terminate the turn with a generic
/// error.  Requiring both a Codex terminal error and exactly one independently
/// classified exhaustion text preserves the terminal boundary without
/// mistaking unrelated terminal errors for credit exhaustion.
fn codex_credit_exhaustion_diagnostic_from_events(
    events: &[super::runtime::NormalizedProviderEventV1],
) -> Option<&str> {
    let has_codex_terminal_error = events.iter().any(|event| {
        event.provider_id == ProviderIdV1::CodexCli.as_str()
            && event.kind == NormalizedProviderEventKind::TerminalError
    });
    if !has_codex_terminal_error {
        return None;
    }
    let mut diagnostics = events
        .iter()
        .filter(|event| event.provider_id == ProviderIdV1::CodexCli.as_str())
        .filter_map(|event| event.text.as_deref())
        .filter(|text| {
            classify_codex_availability_failure(text)
                == Some(CodexAvailabilityFailureV1::CreditsExhausted)
        });
    let diagnostic = diagnostics.next()?;
    diagnostics.next().is_none().then_some(diagnostic)
}

fn bounded_contract_text(value: &str, limit: usize) -> String {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn controller_now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn controller_now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn bounded_provider_diagnostic(events: &[super::runtime::NormalizedProviderEventV1]) -> String {
    let detail = events
        .iter()
        .filter(|event| event.kind == NormalizedProviderEventKind::TerminalError)
        .filter_map(|event| event.text.as_deref())
        .next()
        .unwrap_or("provider reported a terminal error without bounded detail");
    let lower = detail.to_ascii_lowercase();
    if [
        "token",
        "password",
        "secret",
        "authorization",
        "api_key",
        "apikey",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        "provider terminal error detail redacted".into()
    } else {
        bounded_contract_text(detail, 2_048)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    use crate::delegated::autonomous_contract::{
        AutonomousCommandProfileV1, AutonomousDevelopmentContractV1, AutonomousGitPolicyV1,
        AutonomousHardStopV1, AutonomousProviderPolicyV1, AutonomousRateLimitPolicyV1,
        AutonomousTaskSpecV1, AutonomousVerificationPolicyV1, AutonomyLeaseV1,
    };
    use crate::delegated::autonomy_state::{
        AutonomousPlanQueueV1, AutonomousPlannedTaskV1, AutonomousQueueTaskStateV1,
        AutonomousQueueTaskV1, AutonomousQueueV1,
    };
    use crate::delegated::runtime::{
        FakeProvider, FakeProviderTurn, NormalizedProviderEventV1, ProviderCapabilitiesV1,
        ProviderHealthStatus, ProviderHealthV1, ProviderSessionV1, ProviderType,
    };
    use crate::delegated::worker_provider::{FakeWorkerProviderV1, ProviderFuture, ProviderIdV1};

    struct RateLimitedProvider {
        next_handle: u64,
        return_rate_limit: bool,
    }

    impl RateLimitedProvider {
        fn new() -> Self {
            Self {
                next_handle: 1,
                return_rate_limit: true,
            }
        }

        fn handle(&mut self, request: &WorkerProviderTurnRequestV1) -> ProviderTurnHandleV1 {
            let handle = ProviderTurnHandleV1 {
                provider_id: ProviderIdV1::Fake,
                handle_id: format!("rate-limit-turn-{}", self.next_handle),
                provider_session_id: request.provider_session.provider_session_id.clone(),
                worker_session_id: request.turn.worker_session_id.clone(),
                turn_id: request.turn.turn_id.clone(),
            };
            self.next_handle += 1;
            handle
        }
    }

    impl WorkerProviderV1 for RateLimitedProvider {
        fn provider_id(&self) -> ProviderIdV1 {
            ProviderIdV1::Fake
        }

        fn capabilities(&self) -> ProviderCapabilitiesV1 {
            ProviderCapabilitiesV1 {
                provider_id: "fake".into(),
                provider_type: ProviderType::Fake,
                supports_streaming: true,
                supports_native_tool_calls: true,
                supports_session_continuity: true,
                supports_cancellation: true,
                context_limit: 1024,
                output_limit: 1024,
            }
        }

        fn create_session(
            &self,
            model_id: &str,
            _worker_session_id: &WorkerSessionId,
        ) -> ProviderSessionV1 {
            ProviderSessionV1 {
                provider_id: "fake".into(),
                provider_session_id: "rate-limit-session".into(),
                model_id: model_id.into(),
                keep_alive: None,
            }
        }

        fn start_turn<'a>(
            &'a mut self,
            request: WorkerProviderTurnRequestV1,
        ) -> ProviderFuture<'a, ProviderTurnHandleV1> {
            Box::pin(async move { Ok(self.handle(&request)) })
        }

        fn resume_turn<'a>(
            &'a mut self,
            request: WorkerProviderTurnRequestV1,
        ) -> ProviderFuture<'a, ProviderTurnHandleV1> {
            self.start_turn(request)
        }

        fn poll_events<'a>(
            &'a mut self,
            handle: &'a ProviderTurnHandleV1,
            _after_cursor: u64,
        ) -> ProviderFuture<'a, ProviderEventBatchV1> {
            Box::pin(async move {
                if self.return_rate_limit {
                    self.return_rate_limit = false;
                    Ok(ProviderEventBatchV1 {
                        events: vec![NormalizedProviderEventV1 {
                            provider_id: "fake".into(),
                            turn_id: handle.turn_id.clone(),
                            kind: NormalizedProviderEventKind::TerminalError,
                            text: Some("HTTP 429 rate limit".into()),
                            tool_call: None,
                        }],
                        next_cursor: 1,
                        terminal: true,
                        provider_session_id: Some(handle.provider_session_id.clone()),
                    })
                } else {
                    Ok(ProviderEventBatchV1 {
                        events: vec![NormalizedProviderEventV1 {
                            provider_id: "fake".into(),
                            turn_id: handle.turn_id.clone(),
                            kind: NormalizedProviderEventKind::CompletionClaim,
                            text: Some("resumed after rate limit".into()),
                            tool_call: None,
                        }],
                        next_cursor: 1,
                        terminal: true,
                        provider_session_id: Some(handle.provider_session_id.clone()),
                    })
                }
            })
        }

        fn cancel<'a>(&'a mut self, _handle: &'a ProviderTurnHandleV1) -> ProviderFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn status<'a>(&'a self) -> ProviderFuture<'a, ProviderHealthV1> {
            Box::pin(async {
                Ok(ProviderHealthV1 {
                    provider_id: "fake".into(),
                    status: ProviderHealthStatus::Available,
                    available_models: vec!["fake-model".into()],
                    detail: "test".into(),
                })
            })
        }
    }

    impl LocalQwenFallbackProviderV1 for RateLimitedProvider {
        fn provider_route(&self) -> AutonomousProviderRouteV1 {
            AutonomousProviderRouteV1::CodexPreferred
        }

        fn activate_local_qwen<'a>(&'a mut self, _model_id: &'a str) -> ProviderFuture<'a, bool> {
            Box::pin(async { Ok(false) })
        }

        fn prefer_codex_at_task_boundary(&mut self) -> bool {
            false
        }
    }

    struct CodexRouteTestProvider {
        route: AutonomousProviderRouteV1,
        credits_exhausted: bool,
        qwen_available: bool,
        initial_emitted: bool,
        fallback_activations: u32,
        launches: Vec<ProviderIdV1>,
        tool_sets: Vec<(ProviderIdV1, Vec<String>)>,
        next_handle: u64,
    }

    impl CodexRouteTestProvider {
        fn new(credits_exhausted: bool, qwen_available: bool) -> Self {
            Self {
                route: AutonomousProviderRouteV1::CodexPreferred,
                credits_exhausted,
                qwen_available,
                initial_emitted: false,
                fallback_activations: 0,
                launches: Vec::new(),
                tool_sets: Vec::new(),
                next_handle: 1,
            }
        }
    }

    impl WorkerProviderV1 for CodexRouteTestProvider {
        fn provider_id(&self) -> ProviderIdV1 {
            if self.route == AutonomousProviderRouteV1::QwenFallbackActive {
                ProviderIdV1::Ollama
            } else {
                ProviderIdV1::CodexCli
            }
        }

        fn capabilities(&self) -> ProviderCapabilitiesV1 {
            ProviderCapabilitiesV1 {
                provider_id: self.provider_id().as_str().into(),
                provider_type: ProviderType::LocalApi,
                supports_streaming: false,
                supports_native_tool_calls: false,
                supports_session_continuity: true,
                supports_cancellation: false,
                context_limit: 1024,
                output_limit: 1024,
            }
        }

        fn create_session(&self, model_id: &str, _worker: &WorkerSessionId) -> ProviderSessionV1 {
            ProviderSessionV1 {
                provider_id: self.provider_id().as_str().into(),
                provider_session_id: format!("{}-session", self.provider_id().as_str()),
                model_id: model_id.into(),
                keep_alive: None,
            }
        }

        fn start_turn<'a>(
            &'a mut self,
            request: WorkerProviderTurnRequestV1,
        ) -> ProviderFuture<'a, ProviderTurnHandleV1> {
            Box::pin(async move {
                let provider_id = self.provider_id();
                self.launches.push(provider_id);
                self.tool_sets.push((
                    provider_id,
                    request
                        .turn
                        .tool_definitions
                        .iter()
                        .map(|tool| tool.name.clone())
                        .collect(),
                ));
                let handle = ProviderTurnHandleV1 {
                    provider_id,
                    handle_id: format!("route-test-{}", self.next_handle),
                    provider_session_id: request.provider_session.provider_session_id,
                    worker_session_id: request.turn.worker_session_id,
                    turn_id: request.turn.turn_id,
                };
                self.next_handle += 1;
                Ok(handle)
            })
        }

        fn resume_turn<'a>(
            &'a mut self,
            request: WorkerProviderTurnRequestV1,
        ) -> ProviderFuture<'a, ProviderTurnHandleV1> {
            self.start_turn(request)
        }

        fn poll_events<'a>(
            &'a mut self,
            handle: &'a ProviderTurnHandleV1,
            _cursor: u64,
        ) -> ProviderFuture<'a, ProviderEventBatchV1> {
            Box::pin(async move {
                let text = if handle.provider_id == ProviderIdV1::CodexCli && !self.initial_emitted
                {
                    self.initial_emitted = true;
                    if self.credits_exhausted {
                        "usage limit reached"
                    } else {
                        "HTTP 429 rate limit"
                    }
                } else {
                    "completed"
                };
                Ok(ProviderEventBatchV1 {
                    events: vec![NormalizedProviderEventV1 {
                        provider_id: handle.provider_id.as_str().into(),
                        turn_id: handle.turn_id.clone(),
                        kind: if text == "completed" {
                            NormalizedProviderEventKind::CompletionClaim
                        } else {
                            NormalizedProviderEventKind::TerminalError
                        },
                        text: Some(text.into()),
                        tool_call: None,
                    }],
                    next_cursor: 1,
                    terminal: true,
                    provider_session_id: Some(handle.provider_session_id.clone()),
                })
            })
        }

        fn cancel<'a>(&'a mut self, _handle: &'a ProviderTurnHandleV1) -> ProviderFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn status<'a>(&'a self) -> ProviderFuture<'a, ProviderHealthV1> {
            Box::pin(async move {
                Ok(ProviderHealthV1 {
                    provider_id: self.provider_id().as_str().into(),
                    status: ProviderHealthStatus::Available,
                    available_models: vec!["qwen3.6:35b-a3b".into()],
                    detail: "test".into(),
                })
            })
        }
    }

    impl LocalQwenFallbackProviderV1 for CodexRouteTestProvider {
        fn provider_route(&self) -> AutonomousProviderRouteV1 {
            self.route.clone()
        }

        fn activate_local_qwen<'a>(&'a mut self, _model: &'a str) -> ProviderFuture<'a, bool> {
            Box::pin(async move {
                self.fallback_activations += 1;
                if self.qwen_available {
                    self.route = AutonomousProviderRouteV1::QwenFallbackActive;
                }
                Ok(self.qwen_available)
            })
        }

        fn prefer_codex_at_task_boundary(&mut self) -> bool {
            self.route = AutonomousProviderRouteV1::CodexPreferred;
            true
        }
    }

    struct CreditRouteProvider {
        qwen_healthy: bool,
        qwen_active: bool,
        launches: u32,
        allow_codex_restore: bool,
        diagnostic_then_generic_terminal: bool,
    }

    impl CreditRouteProvider {
        fn new(qwen_healthy: bool) -> Self {
            Self {
                qwen_healthy,
                qwen_active: false,
                launches: 0,
                allow_codex_restore: false,
                diagnostic_then_generic_terminal: false,
            }
        }

        fn with_codex_restore(mut self) -> Self {
            self.allow_codex_restore = true;
            self
        }

        fn with_diagnostic_then_generic_terminal(mut self) -> Self {
            self.diagnostic_then_generic_terminal = true;
            self
        }
    }

    impl WorkerProviderV1 for CreditRouteProvider {
        fn provider_id(&self) -> ProviderIdV1 {
            if self.qwen_active {
                ProviderIdV1::Ollama
            } else {
                ProviderIdV1::CodexCli
            }
        }

        fn capabilities(&self) -> ProviderCapabilitiesV1 {
            ProviderCapabilitiesV1 {
                provider_id: self.provider_id().as_str().into(),
                provider_type: if self.qwen_active {
                    ProviderType::LocalApi
                } else {
                    ProviderType::Fake
                },
                supports_streaming: false,
                supports_native_tool_calls: false,
                supports_session_continuity: true,
                supports_cancellation: false,
                context_limit: 1024,
                output_limit: 1024,
            }
        }

        fn create_session(&self, model_id: &str, _worker: &WorkerSessionId) -> ProviderSessionV1 {
            ProviderSessionV1 {
                provider_id: self.provider_id().as_str().into(),
                provider_session_id: format!("{}-session", self.provider_id().as_str()),
                model_id: model_id.into(),
                keep_alive: None,
            }
        }

        fn start_turn<'a>(
            &'a mut self,
            request: WorkerProviderTurnRequestV1,
        ) -> ProviderFuture<'a, ProviderTurnHandleV1> {
            Box::pin(async move {
                self.launches += 1;
                Ok(ProviderTurnHandleV1 {
                    provider_id: self.provider_id(),
                    handle_id: format!("credit-route-{}", self.launches),
                    provider_session_id: request.provider_session.provider_session_id,
                    worker_session_id: request.turn.worker_session_id,
                    turn_id: request.turn.turn_id,
                })
            })
        }

        fn resume_turn<'a>(
            &'a mut self,
            request: WorkerProviderTurnRequestV1,
        ) -> ProviderFuture<'a, ProviderTurnHandleV1> {
            self.start_turn(request)
        }

        fn poll_events<'a>(
            &'a mut self,
            handle: &'a ProviderTurnHandleV1,
            _cursor: u64,
        ) -> ProviderFuture<'a, ProviderEventBatchV1> {
            Box::pin(async move {
                let events = if handle.provider_id == ProviderIdV1::CodexCli {
                    if self.diagnostic_then_generic_terminal {
                        vec![
                            NormalizedProviderEventV1 {
                                provider_id: "codex-cli".into(),
                                turn_id: handle.turn_id.clone(),
                                kind: NormalizedProviderEventKind::TextDelta,
                                text: Some(
                                    "You've hit your usage limit. Try again at Sep 2nd, 2026 12:50 AM."
                                        .into(),
                                ),
                                tool_call: None,
                            },
                            NormalizedProviderEventV1 {
                                provider_id: "codex-cli".into(),
                                turn_id: handle.turn_id.clone(),
                                kind: NormalizedProviderEventKind::TerminalError,
                                text: Some("Codex CLI turn failed".into()),
                                tool_call: None,
                            },
                        ]
                    } else {
                        vec![NormalizedProviderEventV1 {
                            provider_id: "codex-cli".into(),
                            turn_id: handle.turn_id.clone(),
                            kind: NormalizedProviderEventKind::TerminalError,
                            text: Some("usage limit reached".into()),
                            tool_call: None,
                        }]
                    }
                } else {
                    vec![NormalizedProviderEventV1 {
                        provider_id: "ollama".into(),
                        turn_id: handle.turn_id.clone(),
                        kind: NormalizedProviderEventKind::CompletionClaim,
                        text: Some("local continuation complete".into()),
                        tool_call: None,
                    }]
                };
                Ok(ProviderEventBatchV1 {
                    events,
                    next_cursor: 1,
                    terminal: true,
                    provider_session_id: Some(handle.provider_session_id.clone()),
                })
            })
        }

        fn cancel<'a>(&'a mut self, _handle: &'a ProviderTurnHandleV1) -> ProviderFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn status<'a>(&'a self) -> ProviderFuture<'a, ProviderHealthV1> {
            Box::pin(async move {
                Ok(ProviderHealthV1 {
                    provider_id: self.provider_id().as_str().into(),
                    status: ProviderHealthStatus::Available,
                    available_models: vec!["qwen3.6:35b-a3b".into()],
                    detail: "deterministic".into(),
                })
            })
        }
    }

    impl LocalQwenFallbackProviderV1 for CreditRouteProvider {
        fn provider_route(&self) -> AutonomousProviderRouteV1 {
            if self.qwen_active {
                AutonomousProviderRouteV1::QwenFallbackActive
            } else {
                AutonomousProviderRouteV1::CodexPreferred
            }
        }

        fn activate_local_qwen<'a>(&'a mut self, _model_id: &'a str) -> ProviderFuture<'a, bool> {
            Box::pin(async move {
                self.qwen_active = self.qwen_healthy;
                Ok(self.qwen_healthy)
            })
        }

        fn prefer_codex_at_task_boundary(&mut self) -> bool {
            if self.allow_codex_restore {
                self.qwen_active = false;
                true
            } else {
                false
            }
        }
    }

    /// Records the transport method separately from task lifecycle so the
    /// controller tests can prove a host-bound Codex turn never starts a fresh
    /// thread merely because the selected DAG task changed.
    struct CanonicalCodexProvider {
        reported_thread_id: String,
        poll_reported_thread_id: String,
        start_calls: u32,
        resume_sessions: Vec<String>,
        next_handle: u32,
    }

    impl CanonicalCodexProvider {
        fn new(reported_thread_id: &str) -> Self {
            Self {
                reported_thread_id: reported_thread_id.into(),
                poll_reported_thread_id: reported_thread_id.into(),
                start_calls: 0,
                resume_sessions: Vec::new(),
                next_handle: 1,
            }
        }

        fn with_poll_thread_id(handle_thread_id: &str, poll_thread_id: &str) -> Self {
            Self {
                reported_thread_id: handle_thread_id.into(),
                poll_reported_thread_id: poll_thread_id.into(),
                start_calls: 0,
                resume_sessions: Vec::new(),
                next_handle: 1,
            }
        }

        fn handle(&mut self, request: WorkerProviderTurnRequestV1) -> ProviderTurnHandleV1 {
            let handle = ProviderTurnHandleV1 {
                provider_id: ProviderIdV1::CodexCli,
                handle_id: format!("canonical-codex-{}", self.next_handle),
                provider_session_id: self.reported_thread_id.clone(),
                worker_session_id: request.turn.worker_session_id,
                turn_id: request.turn.turn_id,
            };
            self.next_handle += 1;
            handle
        }
    }

    impl WorkerProviderV1 for CanonicalCodexProvider {
        fn provider_id(&self) -> ProviderIdV1 {
            ProviderIdV1::CodexCli
        }

        fn capabilities(&self) -> ProviderCapabilitiesV1 {
            ProviderCapabilitiesV1 {
                provider_id: "codex-cli".into(),
                provider_type: ProviderType::LocalApi,
                supports_streaming: true,
                supports_native_tool_calls: false,
                supports_session_continuity: true,
                supports_cancellation: false,
                context_limit: 1024,
                output_limit: 1024,
            }
        }

        fn create_session(&self, model_id: &str, worker: &WorkerSessionId) -> ProviderSessionV1 {
            ProviderSessionV1 {
                provider_id: "codex-cli".into(),
                provider_session_id: format!("codex-unbound-{}", worker.as_str()),
                model_id: model_id.into(),
                keep_alive: None,
            }
        }

        fn start_turn<'a>(
            &'a mut self,
            request: WorkerProviderTurnRequestV1,
        ) -> ProviderFuture<'a, ProviderTurnHandleV1> {
            Box::pin(async move {
                self.start_calls += 1;
                Ok(self.handle(request))
            })
        }

        fn resume_turn<'a>(
            &'a mut self,
            request: WorkerProviderTurnRequestV1,
        ) -> ProviderFuture<'a, ProviderTurnHandleV1> {
            Box::pin(async move {
                self.resume_sessions
                    .push(request.provider_session.provider_session_id.clone());
                Ok(self.handle(request))
            })
        }

        fn poll_events<'a>(
            &'a mut self,
            handle: &'a ProviderTurnHandleV1,
            _cursor: u64,
        ) -> ProviderFuture<'a, ProviderEventBatchV1> {
            Box::pin(async move {
                Ok(ProviderEventBatchV1 {
                    events: vec![NormalizedProviderEventV1 {
                        provider_id: "codex-cli".into(),
                        turn_id: handle.turn_id.clone(),
                        kind: NormalizedProviderEventKind::CompletionClaim,
                        text: Some("completed".into()),
                        tool_call: None,
                    }],
                    next_cursor: 1,
                    terminal: true,
                    provider_session_id: Some(self.poll_reported_thread_id.clone()),
                })
            })
        }

        fn cancel<'a>(&'a mut self, _handle: &'a ProviderTurnHandleV1) -> ProviderFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn status<'a>(&'a self) -> ProviderFuture<'a, ProviderHealthV1> {
            Box::pin(async {
                Ok(ProviderHealthV1 {
                    provider_id: "codex-cli".into(),
                    status: ProviderHealthStatus::Available,
                    available_models: vec![CATDESK_REQUIRED_CODEX_MODEL_V1.into()],
                    detail: "deterministic".into(),
                })
            })
        }
    }

    impl LocalQwenFallbackProviderV1 for CanonicalCodexProvider {
        fn provider_route(&self) -> AutonomousProviderRouteV1 {
            AutonomousProviderRouteV1::CodexPreferred
        }

        fn activate_local_qwen<'a>(&'a mut self, _model_id: &'a str) -> ProviderFuture<'a, bool> {
            Box::pin(async { Ok(false) })
        }

        fn prefer_codex_at_task_boundary(&mut self) -> bool {
            true
        }
    }

    struct Verifier {
        statuses: Vec<VerificationStatusV1>,
        diff: String,
        final_review: bool,
    }
    impl AutonomousVerifierV1 for Verifier {
        fn verify(&mut self) -> Result<(VerificationSummaryV1, String), RuntimeError> {
            let status = if self.statuses.len() > 1 {
                self.statuses.remove(0)
            } else {
                self.statuses
                    .first()
                    .cloned()
                    .unwrap_or(VerificationStatusV1::Failed)
            };
            Ok((
                VerificationSummaryV1 {
                    status,
                    command: "deterministic".into(),
                    summary: "test".into(),
                },
                self.diff.clone(),
            ))
        }

        fn final_review(
            &mut self,
            _verification: &VerificationSummaryV1,
            _authoritative_diff: &str,
        ) -> Result<String, RuntimeError> {
            if self.final_review {
                Ok("deterministic final review".into())
            } else {
                Err(RuntimeError::Validation(
                    "deterministic final review intentionally absent".into(),
                ))
            }
        }
    }

    fn setup(
        status: VerificationStatusV1,
        diff: &str,
    ) -> AutonomousControllerV1<FakeWorkerProviderV1, Verifier> {
        let root = std::env::temp_dir().join(format!("catdesk-controller-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        let contract = AutonomousDevelopmentContractV1 {
            schema_version: 1,
            contract_id: "adc-1".into(),
            task_id: "task-1".into(),
            project_id: "catdesk".into(),
            mode: "chatgpt_web_codex_autonomous".into(),
            objective: "test".into(),
            workspace: root.clone(),
            base_branch: "main".into(),
            feature_branch: "feature-1".into(),
            base_commit: "1234567".into(),
            expected_origin: "https://example.invalid/repo.git".into(),
            ordered_steps: vec!["run".into()],
            allowed_paths: vec![root.join("src")],
            forbidden_paths: Vec::new(),
            allowed_command_profiles: vec![AutonomousCommandProfileV1::CargoTest],
            git_policy: AutonomousGitPolicyV1 {
                create_branch: true,
                create_worktree: true,
                local_commits: false,
                push_feature_branch: false,
                open_pull_request: false,
                merge: false,
                force_push: false,
                modify_main: false,
            },
            provider_policy: AutonomousProviderPolicyV1 {
                primary_provider: "codex-cli".into(),
                primary_model: "gpt-5.6-terra".into(),
                routine_provider: "ollama".into(),
                routine_model: "qwen3.6:35b-a3b".into(),
                allow_paid_fallback: false,
                allow_cloud_fallback: false,
            },
            verification_policy: AutonomousVerificationPolicyV1 {
                profile: "test".into(),
                required_commands: vec![],
                max_repair_cycles: 1,
                require_authoritative_diff: true,
                require_final_review: true,
            },
            rate_limit_policy: AutonomousRateLimitPolicyV1 {
                automatic_pause: true,
                automatic_resume: true,
                initial_backoff_seconds: 5,
                maximum_backoff_seconds: 30,
                maximum_rate_limited_seconds: 60,
                honor_provider_retry_after: true,
            },
            autonomy_lease: AutonomyLeaseV1 {
                start_approval_required: false,
                renewal_allowed: true,
                issued_at_unix: 1,
                expires_at_unix: 100,
                maximum_total_elapsed_seconds: 99,
                maximum_provider_turns: 2,
                maximum_tool_calls: 2,
                maximum_consecutive_failures: 1,
                maximum_repair_cycles: 1,
            },
            hard_stop_conditions: vec![AutonomousHardStopV1::CancellationRequested],
            completion_artifact_ids: Vec::new(),
            task_graph: Vec::new(),
        };
        let policy = AutonomousPolicyEngineV1::new(contract).expect("policy");
        let store = AutonomousStateStoreV1::open(root.join("state")).expect("store");
        store
            .create_session(
                "session-1",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "work".into(),
                    priority: 1,
                    depends_on: vec![],
                    state: AutonomousQueueTaskStateV1::Ready,
                }])
                .expect("queue"),
            )
            .expect("session");
        store
            .record_codex_routing_telemetry(
                "session-1",
                CodexRoutingTelemetryV1 {
                    observed_at_unix: 10,
                    source: "codex-app-server/account-rateLimits-read".into(),
                    selected_model: Some(CATDESK_REQUIRED_CODEX_MODEL_V1.into()),
                    reasoning_effort: Some(CATDESK_REQUIRED_CODEX_REASONING_EFFORT_V1.into()),
                    ..Default::default()
                },
                900,
            )
            .expect("host gate telemetry");
        AutonomousControllerV1::new(
            policy,
            store,
            FakeWorkerProviderV1::new(FakeProvider::new(vec![
                FakeProviderTurn::Complete("first".into()),
                FakeProviderTurn::Complete("repair".into()),
            ])),
            Verifier {
                statuses: vec![status],
                diff: diff.into(),
                final_review: true,
            },
            "session-1".into(),
        )
    }

    fn graph_controller() -> AutonomousControllerV1<FakeWorkerProviderV1, Verifier> {
        let mut controller = setup(VerificationStatusV1::Passed, "diff --git");
        let mut contract = controller.policy.contract().clone();
        contract.autonomy_lease.maximum_provider_turns = 8;
        contract.task_graph = vec![
            AutonomousTaskSpecV1 {
                task_id: "a".into(),
                priority: 1,
                depends_on: vec![],
                acceptance_criteria: vec!["approved A".into()],
                planner_gate: None,
                completion_artifact_ids: Vec::new(),
            },
            AutonomousTaskSpecV1 {
                task_id: "b".into(),
                priority: 1,
                depends_on: vec!["a".into()],
                acceptance_criteria: vec!["approved B".into()],
                planner_gate: None,
                completion_artifact_ids: Vec::new(),
            },
            AutonomousTaskSpecV1 {
                task_id: "c".into(),
                priority: 1,
                depends_on: vec!["a".into()],
                acceptance_criteria: vec!["approved C".into()],
                planner_gate: None,
                completion_artifact_ids: Vec::new(),
            },
            AutonomousTaskSpecV1 {
                task_id: "d".into(),
                priority: 1,
                depends_on: vec!["b".into(), "c".into()],
                acceptance_criteria: vec!["approved D".into()],
                planner_gate: None,
                completion_artifact_ids: Vec::new(),
            },
        ];
        let allowed_paths = contract
            .allowed_paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let queue = AutonomousQueueV1::new(
            contract
                .task_graph
                .iter()
                .map(|task| AutonomousQueueTaskV1 {
                    task_id: task.task_id.clone(),
                    priority: task.priority,
                    depends_on: task.depends_on.clone(),
                    state: AutonomousQueueTaskStateV1::Ready,
                })
                .collect(),
        )
        .expect("graph queue");
        let plan = AutonomousPlanQueueV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            tasks: contract
                .task_graph
                .iter()
                .map(|task| AutonomousPlannedTaskV1 {
                    task_id: task.task_id.clone(),
                    depends_on: task.depends_on.clone(),
                    acceptance_criteria: task.acceptance_criteria.clone(),
                    allowed_paths: allowed_paths.clone(),
                    verification_profile: contract.verification_policy.profile.clone(),
                    preferred_worker_policy: "codex-preferred".into(),
                    escalation_conditions: vec!["scope ambiguity".into()],
                    completion_artifact_ids: task.completion_artifact_ids.clone(),
                    provider_provenance: vec![],
                })
                .collect(),
        };
        controller
            .store
            .save_queue("session-1", &queue)
            .expect("graph queue saved");
        controller
            .store
            .write_plan_queue("session-1", &plan)
            .expect("graph plan saved");
        controller
            .store
            .save_contract("session-1", &contract)
            .expect("graph contract saved");
        controller.policy = AutonomousPolicyEngineV1::new(contract).expect("graph policy");
        let mut snapshot = controller.store.load_session("session-1").expect("state");
        snapshot.state = AutonomousSessionStateV1::Queued;
        snapshot.active = true;
        snapshot.approved_contract_hash = Some(controller.policy.contract_hash().into());
        controller
            .store
            .save_session(&snapshot)
            .expect("started state");
        controller.provider = FakeWorkerProviderV1::new(FakeProvider::new(vec![
            FakeProviderTurn::Complete("A".into()),
            FakeProviderTurn::Complete("B".into()),
            FakeProviderTurn::Complete("C".into()),
            FakeProviderTurn::Complete("D".into()),
        ]));
        controller
    }

    fn canonical_codex_controller(
        provider: CanonicalCodexProvider,
        statuses: Vec<VerificationStatusV1>,
    ) -> AutonomousControllerV1<CanonicalCodexProvider, Verifier> {
        let initial = setup(VerificationStatusV1::Passed, "diff --git");
        let policy = initial.policy;
        let store = initial.store;
        let session_id = initial.session_id;
        let mut snapshot = store.load_session(&session_id).expect("session");
        snapshot.state = AutonomousSessionStateV1::Queued;
        snapshot.provider_thread_id = Some("canonical-project-thread".into());
        store.save_session(&snapshot).expect("canonical binding");
        AutonomousControllerV1::new(
            policy,
            store,
            provider,
            Verifier {
                statuses,
                diff: "diff --git".into(),
                final_review: true,
            },
            session_id,
        )
    }

    #[tokio::test]
    async fn canonical_bound_new_dag_tasks_resume_the_exact_thread_without_repair_budget() {
        let mut controller = canonical_codex_controller(
            CanonicalCodexProvider::new("canonical-project-thread"),
            vec![VerificationStatusV1::Passed],
        );
        let mut queue = controller.store.load_queue("session-1").expect("queue");
        queue.tasks = vec![
            AutonomousQueueTaskV1 {
                task_id: "a".into(),
                priority: 1,
                depends_on: vec![],
                state: AutonomousQueueTaskStateV1::Ready,
            },
            AutonomousQueueTaskV1 {
                task_id: "b".into(),
                priority: 1,
                depends_on: vec!["a".into()],
                state: AutonomousQueueTaskStateV1::Ready,
            },
        ];
        controller
            .store
            .save_queue("session-1", &queue)
            .expect("queue");

        assert_eq!(
            controller.run_once(10).await.expect("A").state,
            AutonomousSessionStateV1::Queued
        );
        assert_eq!(
            controller.run_once(11).await.expect("B").state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(controller.provider.start_calls, 0);
        assert_eq!(
            controller.provider.resume_sessions,
            vec!["canonical-project-thread", "canonical-project-thread"]
        );
        let snapshot = controller.store.load_session("session-1").expect("session");
        assert_eq!(
            snapshot.provider_thread_id.as_deref(),
            Some("canonical-project-thread")
        );
        assert_eq!(snapshot.repair_attempts, 0);
    }

    #[tokio::test]
    async fn same_task_repair_keeps_repair_accounting_while_resuming_canonical_thread() {
        let mut controller = canonical_codex_controller(
            CanonicalCodexProvider::new("canonical-project-thread"),
            vec![VerificationStatusV1::Failed, VerificationStatusV1::Passed],
        );
        assert_eq!(
            controller.run_once(10).await.expect("initial").state,
            AutonomousSessionStateV1::Queued
        );
        assert_eq!(
            controller.run_once(11).await.expect("repair").state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(controller.provider.start_calls, 0);
        assert_eq!(controller.provider.resume_sessions.len(), 2);
        assert_eq!(controller.repair_attempts, 1);
    }

    #[tokio::test]
    async fn mismatched_resumed_codex_handle_fails_closed_without_overwriting_binding() {
        let mut controller = canonical_codex_controller(
            CanonicalCodexProvider::new("unexpected-provider-thread"),
            vec![VerificationStatusV1::Passed],
        );
        assert_eq!(
            controller.run_once(10).await.expect("fail closed").state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        assert_eq!(controller.provider.start_calls, 0);
        assert_eq!(
            controller.provider.resume_sessions,
            vec!["canonical-project-thread"]
        );
        let snapshot = controller.store.load_session("session-1").expect("session");
        assert_eq!(
            snapshot.provider_thread_id.as_deref(),
            Some("canonical-project-thread")
        );
        assert_eq!(
            snapshot.expected_codex_thread_id.as_deref(),
            Some("canonical-project-thread")
        );
        assert_eq!(
            controller
                .store
                .load_escalation("session-1")
                .expect("escalation")
                .reason,
            "codex_provider_thread_identity_mismatch"
        );
    }

    #[tokio::test]
    async fn mismatched_resumed_codex_poll_fails_closed_without_overwriting_binding() {
        let mut controller = canonical_codex_controller(
            CanonicalCodexProvider::with_poll_thread_id(
                "canonical-project-thread",
                "unexpected-provider-thread",
            ),
            vec![VerificationStatusV1::Passed],
        );
        assert_eq!(
            controller.run_once(10).await.expect("fail closed").state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        assert_eq!(controller.provider.start_calls, 0);
        let snapshot = controller.store.load_session("session-1").expect("session");
        assert_eq!(
            snapshot.provider_thread_id.as_deref(),
            Some("canonical-project-thread")
        );
        assert_eq!(
            snapshot.expected_codex_thread_id.as_deref(),
            Some("canonical-project-thread")
        );
    }

    #[tokio::test]
    async fn graph_materialization_corruption_escalates_before_provider_launch() {
        for corruption in [
            "acceptance-criteria",
            "allowed-paths",
            "verification-profile",
            "completion-artifacts",
            "plan-dependencies",
            "queue-priority",
            "queue-dependencies",
            "extra-task",
            "missing-task",
        ] {
            let mut controller = graph_controller();
            let mut queue = controller.store.load_queue("session-1").expect("queue");
            let mut plan = controller.store.load_plan_queue("session-1").expect("plan");
            match corruption {
                "acceptance-criteria" => plan.tasks[0].acceptance_criteria = vec!["mutated".into()],
                "allowed-paths" => plan.tasks[0].allowed_paths = vec!["mutated".into()],
                "verification-profile" => plan.tasks[0].verification_profile = "mutated".into(),
                "completion-artifacts" => {
                    plan.tasks[0].completion_artifact_ids = vec!["src/mutated.txt".into()]
                }
                "plan-dependencies" => plan.tasks[1].depends_on.clear(),
                "queue-priority" => queue.tasks[0].priority = 99,
                "queue-dependencies" => queue.tasks[1].depends_on.clear(),
                "extra-task" => queue.tasks.push(AutonomousQueueTaskV1 {
                    task_id: "extra".into(),
                    priority: 1,
                    depends_on: vec![],
                    state: AutonomousQueueTaskStateV1::Ready,
                }),
                "missing-task" => {
                    queue.tasks.pop();
                }
                _ => unreachable!("fixed corruption list"),
            }
            controller
                .store
                .save_queue("session-1", &queue)
                .expect("corrupted queue saved");
            if !matches!(corruption, "extra-task" | "missing-task") {
                controller
                    .store
                    .write_plan_queue("session-1", &plan)
                    .expect("corrupted plan saved");
            }
            let outcome = controller.run_once(10).await.expect("fail closed");
            assert_eq!(outcome.state, AutonomousSessionStateV1::WaitingForChatgpt);
            assert!(controller.handles.is_empty(), "{corruption}");
            assert_eq!(
                controller
                    .store
                    .load_session("session-1")
                    .expect("state")
                    .provider_turn_count,
                0,
                "{corruption}"
            );
            assert_eq!(
                controller
                    .store
                    .load_escalation("session-1")
                    .expect("escalation")
                    .reason,
                "approved_graph_materialization_unavailable_or_mismatched"
            );
        }
    }

    #[tokio::test]
    async fn accepted_planner_reply_rearms_the_gated_task_once() {
        let mut controller = graph_controller();
        let mut contract = controller.policy.contract().clone();
        contract.task_graph[0].planner_gate = Some("choose the bounded design".into());
        controller
            .store
            .save_contract("session-1", &contract)
            .expect("gated contract");
        controller.policy = AutonomousPolicyEngineV1::new(contract).expect("gated policy");

        assert_eq!(
            controller
                .run_once(10)
                .await
                .expect("gate escalation")
                .state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        assert_eq!(
            controller
                .store
                .load_session("session-1")
                .expect("waiting state")
                .provider_turn_count,
            0
        );
        let escalation = controller
            .store
            .load_escalation("session-1")
            .expect("planner escalation");
        assert_eq!(escalation.reason, "planned_architecture_decision_required");

        controller
            .store
            .write_planner_reply(
                "session-1",
                &escalation.escalation_id,
                "decision-hash",
                "use the approved bounded design",
                &["preserve R2/R3".into()],
            )
            .expect("durable reply");
        controller
            .store
            .satisfy_planner_gate("session-1", "a")
            .expect("durable gate");
        let mut snapshot = controller.store.load_session("session-1").expect("state");
        snapshot.state = AutonomousSessionStateV1::Queued;
        snapshot.active = true;
        controller
            .store
            .save_session(&snapshot)
            .expect("requeued state");

        assert_eq!(
            controller.run_once(11).await.expect("rearmed task").state,
            AutonomousSessionStateV1::Queued
        );
        assert_eq!(
            controller
                .store
                .load_session("session-1")
                .expect("rearmed state")
                .provider_turn_count,
            1
        );
        assert!(
            controller
                .store
                .load_planner_reply("session-1")
                .expect("reply")
                .consumed
        );
    }

    #[tokio::test]
    async fn exact_a_to_b_and_c_to_d_graph_progresses_in_dependency_order() {
        let mut controller = graph_controller();
        assert_eq!(
            controller.run_once(10).await.expect("A").state,
            AutonomousSessionStateV1::Queued
        );
        let queue = controller
            .store
            .load_queue("session-1")
            .expect("queue after A");
        assert_eq!(
            queue.tasks[0].state,
            AutonomousQueueTaskStateV1::CompletedVerified
        );
        assert!(
            queue
                .tasks
                .iter()
                .filter(|task| task.task_id == "b" || task.task_id == "c")
                .all(|task| task.state == AutonomousQueueTaskStateV1::Ready)
        );
        assert_eq!(queue.next_ready_task().expect("B ready").task_id, "b");
        assert_eq!(
            controller.run_once(11).await.expect("B").state,
            AutonomousSessionStateV1::Queued
        );
        assert_eq!(
            controller.run_once(12).await.expect("C").state,
            AutonomousSessionStateV1::Queued
        );
        assert_eq!(
            controller.run_once(13).await.expect("D").state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(
            controller
                .store
                .load_session("session-1")
                .expect("final state")
                .provider_turn_count,
            4
        );
    }

    #[tokio::test]
    async fn waiting_for_chatgpt_never_selects_or_launches_a_provider_turn() {
        let mut controller = setup(VerificationStatusV1::Passed, "diff");
        let mut snapshot = controller.store.load_session("session-1").expect("state");
        snapshot.state = AutonomousSessionStateV1::WaitingForChatgpt;
        snapshot.current_task_id = Some("work".into());
        controller
            .store
            .save_session(&snapshot)
            .expect("waiting state");
        let outcome = controller.run_once(10).await.expect("waiting tick");
        assert_eq!(outcome.state, AutonomousSessionStateV1::WaitingForChatgpt);
        assert_eq!(
            controller
                .store
                .load_session("session-1")
                .expect("state")
                .provider_turn_count,
            0
        );
    }

    #[test]
    fn direct_chatgpt_claim_captures_baseline_before_edits() {
        let mut controller = setup(VerificationStatusV1::Passed, "diff --git direct-output");
        let mut contract = controller.policy.contract().clone();
        contract.task_id = "work".into();
        contract.completion_artifact_ids = vec!["src/direct-output.txt".into()];
        controller.policy = AutonomousPolicyEngineV1::new(contract).expect("direct policy");
        let contract_hash = controller.policy.contract_hash().to_string();
        let mut snapshot = controller.store.load_session("session-1").expect("state");
        snapshot.state = AutonomousSessionStateV1::Queued;
        snapshot.active = true;
        snapshot.approved_contract_hash = Some(contract_hash);
        controller
            .store
            .save_session(&snapshot)
            .expect("approved queued state");

        let claim = controller
            .claim_direct_chatgpt_work(10)
            .expect("direct claim");
        assert_eq!(claim.state, AutonomousSessionStateV1::WaitingForChatgpt);
        let claimed = controller.store.load_session("session-1").expect("claimed");
        assert_eq!(claimed.current_task_id.as_deref(), Some("work"));
        assert_eq!(claimed.provider_turn_count, 1);
        assert_eq!(
            claimed.provider_route,
            AutonomousProviderRouteV1::WaitingForChatgpt
        );
        let baseline = controller
            .store
            .load_task_output_baseline("session-1", "work")
            .expect("baseline")
            .expect("direct baseline");
        assert_eq!(baseline.observations.len(), 1);
        assert_eq!(
            baseline.observations[0].artifact_id,
            "src/direct-output.txt"
        );
        assert_eq!(baseline.observations[0].state, "ABSENT");
        assert_eq!(
            controller
                .store
                .load_queue("session-1")
                .expect("queue")
                .tasks[0]
                .state,
            AutonomousQueueTaskStateV1::WorkerRunning
        );
        let accounting = controller
            .store
            .execution_accounting_store()
            .expect("accounting")
            .list(Some("session-1"))
            .expect("records");
        assert_eq!(accounting.len(), 1);
        assert_eq!(accounting[0].provider_id, DIRECT_CHATGPT_EXECUTOR_ID);
        assert!(
            accounting[0].activity_spans.iter().any(|span| {
                span.actor == ActivityActorV1::ChatgptWebActive
                    && span.status == ActivitySpanStatusV1::Open
            }),
            "direct claim must open explicit ChatGPT activity before edits"
        );

        std::fs::write(
            controller
                .policy
                .contract()
                .workspace
                .join("src/direct-output.txt"),
            "claimed direct output",
        )
        .expect("direct output");
        controller
            .verify_task_output_attribution()
            .expect("post-claim output attribution");
    }

    #[test]
    fn direct_chatgpt_claim_finalizes_through_normal_review_event() {
        let mut controller = setup(VerificationStatusV1::Passed, "diff --git direct-work");
        let contract_hash = controller.policy.contract_hash().to_string();
        let mut snapshot = controller.store.load_session("session-1").expect("state");
        snapshot.state = AutonomousSessionStateV1::Queued;
        snapshot.active = true;
        snapshot.approved_contract_hash = Some(contract_hash);
        controller
            .store
            .save_session(&snapshot)
            .expect("approved queued state");

        controller
            .claim_direct_chatgpt_work(10)
            .expect("direct claim");
        let outcome = controller
            .finalize_direct_chatgpt_work(11)
            .expect("direct finalization");
        assert_eq!(
            outcome.state,
            AutonomousSessionStateV1::CompletedVerified,
            "direct claim finalization escalation: {:?}; events: {:?}",
            controller.store.load_escalation("session-1").ok(),
            controller.store.poll_events("session-1", 0).ok()
        );
        let reviews = controller.store.all_review_inbox().expect("review inbox");
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].next_action, "independent_final_review");
        let accounting = controller
            .store
            .execution_accounting_store()
            .expect("accounting")
            .list(Some("session-1"))
            .expect("records");
        assert!(
            accounting[0].activity_spans.iter().any(|span| {
                span.actor == ActivityActorV1::ChatgptWebActive
                    && span.status == ActivitySpanStatusV1::Completed
            }),
            "direct finalization must close the ChatGPT activity boundary"
        );
    }

    #[test]
    fn direct_chatgpt_claim_refuses_existing_or_unapproved_execution_ownership() {
        let mut controller = setup(VerificationStatusV1::Passed, "diff --git");
        let mut snapshot = controller.store.load_session("session-1").expect("state");
        snapshot.state = AutonomousSessionStateV1::Queued;
        snapshot.active = true;
        snapshot.approved_contract_hash = None;
        controller
            .store
            .save_session(&snapshot)
            .expect("unapproved");
        assert!(controller.claim_direct_chatgpt_work(10).is_err());

        snapshot.approved_contract_hash = Some(controller.policy.contract_hash().into());
        snapshot.current_task_id = Some("work".into());
        snapshot.provider_turn_count = 1;
        controller
            .store
            .save_session(&snapshot)
            .expect("already owned");
        assert!(controller.claim_direct_chatgpt_work(10).is_err());
        assert!(
            controller
                .store
                .execution_accounting_store()
                .expect("accounting")
                .list(Some("session-1"))
                .expect("records")
                .is_empty()
        );
    }

    #[test]
    fn direct_chatgpt_work_converges_on_the_same_completed_review_event() {
        let mut controller = setup(VerificationStatusV1::Passed, "diff --git");
        controller
            .prepare_turn("work", "prior-provider-thread", None)
            .expect("prior provider boundary");
        controller
            .set_current_task_state(AutonomousQueueTaskStateV1::Ready)
            .expect("direct takeover task");
        let mut snapshot = controller.store.load_session("session-1").expect("state");
        snapshot.state = AutonomousSessionStateV1::WaitingForChatgpt;
        snapshot.active = true;
        controller
            .store
            .save_session(&snapshot)
            .expect("waiting direct state");

        let outcome = controller
            .finalize_direct_chatgpt_work(10)
            .expect("direct finalization");
        assert_eq!(
            outcome.state,
            AutonomousSessionStateV1::CompletedVerified,
            "direct escalation: {:?}; events: {:?}",
            controller.store.load_escalation("session-1").ok(),
            controller.store.poll_events("session-1", 0).ok()
        );
        let reviews = controller.store.all_review_inbox().expect("review inbox");
        assert_eq!(reviews.len(), 1);
        assert_eq!(
            reviews[0].state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(reviews[0].next_action, "independent_final_review");
        assert_eq!(reviews[0].reference, "artifacts/completion.json");
        assert!(
            controller
                .store
                .load_completion_artifacts("session-1")
                .is_ok()
        );
    }

    #[test]
    fn direct_chatgpt_work_rejects_a_task_that_never_crossed_a_provider_boundary() {
        let mut controller = setup(VerificationStatusV1::Passed, "diff --git");
        let mut snapshot = controller.store.load_session("session-1").expect("state");
        snapshot.state = AutonomousSessionStateV1::WaitingForChatgpt;
        snapshot.active = true;
        snapshot.current_task_id = Some("work".into());
        snapshot.provider_turn_count = 0;
        controller
            .store
            .save_session(&snapshot)
            .expect("waiting direct state");

        assert!(controller.finalize_direct_chatgpt_work(10).is_err());
        assert!(
            controller
                .store
                .all_review_inbox()
                .expect("review inbox")
                .is_empty()
        );
        assert!(
            controller
                .store
                .load_completion_artifacts("session-1")
                .is_err()
        );
    }

    #[tokio::test]
    async fn direct_chatgpt_checkpoint_recovers_without_launching_another_provider() {
        let mut initial = setup(VerificationStatusV1::Passed, "diff --git");
        initial
            .prepare_turn("work", "prior-provider-thread", None)
            .expect("prior provider boundary");
        initial
            .set_current_task_state(AutonomousQueueTaskStateV1::Ready)
            .expect("direct takeover task");
        let mut snapshot = initial.store.load_session("session-1").expect("state");
        snapshot.state = AutonomousSessionStateV1::WaitingForChatgpt;
        snapshot.active = true;
        initial
            .store
            .save_session(&snapshot)
            .expect("waiting direct state");
        initial.verifier.final_review = false;

        assert_eq!(
            initial
                .finalize_direct_chatgpt_work(10)
                .expect("checkpointed direct finalization")
                .state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        let checkpoint = initial
            .store
            .load_passed_verification_checkpoint("session-1")
            .expect("checkpoint read")
            .expect("direct checkpoint");
        assert_eq!(checkpoint.provider_id, DIRECT_CHATGPT_EXECUTOR_ID);
        let turns_before = initial
            .store
            .load_session("session-1")
            .expect("state")
            .provider_turn_count;

        let policy = initial.policy.clone();
        let store = initial.store.clone();
        let session_id = initial.session_id.clone();
        drop(initial);
        let mut resumed = AutonomousControllerV1::new(
            policy,
            store,
            FakeWorkerProviderV1::new(FakeProvider::new(vec![FakeProviderTurn::Complete(
                "must-not-run".into(),
            )])),
            Verifier {
                statuses: vec![VerificationStatusV1::Failed],
                diff: "must-not-run".into(),
                final_review: true,
            },
            session_id,
        );
        assert_eq!(
            resumed
                .run_once(11)
                .await
                .expect("direct checkpoint recovery")
                .state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert!(
            resumed.handles.is_empty(),
            "direct checkpoint recovery must not launch a provider"
        );
        assert_eq!(
            resumed
                .store
                .load_session("session-1")
                .expect("completed")
                .provider_turn_count,
            turns_before
        );
        let reviews = resumed.store.all_review_inbox().expect("review inbox");
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].next_action, "independent_final_review");
    }

    #[tokio::test]
    async fn completion_requires_independent_verification_and_diff() {
        let mut success = setup(VerificationStatusV1::Passed, "diff --git");
        assert_eq!(
            success.run_once(10).await.expect("run").state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(
            success
                .store
                .load_completion_artifacts("session-1")
                .expect("completion evidence")
                .authoritative_diff,
            "diff --git"
        );
        let mut failed = setup(VerificationStatusV1::Failed, "");
        assert_eq!(
            failed.run_once(10).await.expect("run").state,
            AutonomousSessionStateV1::Queued
        );
    }

    fn structured_output_controller() -> AutonomousControllerV1<FakeWorkerProviderV1, Verifier> {
        let mut controller = setup(
            VerificationStatusV1::Passed,
            "diff --git unrelated-preexisting-change",
        );
        let mut contract = controller.policy.contract().clone();
        contract.task_id = "work".into();
        contract.completion_artifact_ids = vec!["src/task-output.txt".into()];
        controller.policy = AutonomousPolicyEngineV1::new(contract).expect("structured policy");
        let mut snapshot = controller.store.load_session("session-1").expect("session");
        snapshot.current_task_id = Some("work".into());
        controller
            .store
            .save_session(&snapshot)
            .expect("persist task");
        controller
    }

    #[tokio::test]
    async fn structured_output_blocks_noop_provider_despite_unrelated_dirty_diff() {
        let mut controller = structured_output_controller();
        assert_eq!(
            controller.run_once(10).await.expect("no-op provider").state,
            AutonomousSessionStateV1::Queued
        );
        assert!(
            controller
                .store
                .load_completion_artifacts("session-1")
                .is_err()
        );
        assert_eq!(
            controller
                .store
                .load_queue("session-1")
                .expect("queue")
                .tasks[0]
                .state,
            AutonomousQueueTaskStateV1::Ready
        );
        let baseline = controller
            .store
            .load_task_output_baseline("session-1", "work")
            .expect("baseline")
            .expect("captured before provider");
        assert_eq!(baseline.observations[0].state, "ABSENT");
    }

    #[tokio::test]
    async fn lost_prior_launch_baseline_fails_closed_without_a_second_provider_turn() {
        let mut controller = structured_output_controller();
        assert_eq!(
            controller
                .run_once(10)
                .await
                .expect("first no-op turn")
                .state,
            AutonomousSessionStateV1::Queued
        );
        let baseline_path = controller
            .policy
            .contract()
            .workspace
            .join("state/session-1/artifacts/task-output-baselines.json");
        fs::remove_file(&baseline_path).expect("simulate lost baseline authority");
        assert!(
            controller.run_once(11).await.is_err(),
            "the durable launch authority prohibits post-turn baseline recapture"
        );
        let snapshot = controller.store.load_session("session-1").expect("state");
        assert_eq!(
            snapshot.provider_turn_count, 1,
            "no duplicate provider launch"
        );
        assert_eq!(
            controller
                .store
                .load_queue("session-1")
                .expect("queue")
                .tasks[0]
                .state,
            AutonomousQueueTaskStateV1::Ready
        );
        assert!(
            controller
                .store
                .load_completion_artifacts("session-1")
                .is_err()
        );
    }

    #[test]
    fn baseline_write_then_authority_marker_crash_recovers_only_the_original_baseline() {
        let controller = structured_output_controller();
        controller
            .capture_task_output_baseline("work")
            .expect("baseline before simulated marker crash");
        let authority_path = controller
            .policy
            .contract()
            .workspace
            .join("state/session-1/artifacts/task-output-baseline-authorities.json");
        fs::remove_file(&authority_path).expect("simulate marker write interruption");
        let original = controller
            .store
            .load_task_output_baseline("session-1", "work")
            .expect("baseline")
            .expect("baseline present");
        controller
            .capture_task_output_baseline("work")
            .expect("recover exact baseline authority");
        assert_eq!(
            controller
                .store
                .load_task_output_baseline("session-1", "work")
                .expect("reload")
                .expect("baseline"),
            original
        );
        assert!(
            controller
                .store
                .load_task_output_baseline_authority("session-1", "work")
                .expect("authority")
                .is_some()
        );
    }

    #[test]
    fn required_output_classification_accepts_only_exact_absence_or_regular_files() {
        let controller = structured_output_controller();
        let target = controller
            .policy
            .contract()
            .workspace
            .join("src/task-output.txt");
        assert_eq!(
            controller
                .safe_task_output_hash("src/task-output.txt")
                .expect("absence"),
            None
        );
        fs::create_dir(&target).expect("special directory fixture");
        assert!(
            controller
                .safe_task_output_hash("src/task-output.txt")
                .is_err()
        );
        fs::remove_dir(&target).expect("remove directory fixture");

        #[cfg(windows)]
        {
            match std::os::windows::fs::symlink_file("missing-target.txt", &target) {
                Ok(()) => {
                    assert!(
                        controller
                            .safe_task_output_hash("src/task-output.txt")
                            .is_err()
                    );
                    fs::remove_file(&target).expect("remove dangling link fixture");
                }
                Err(error) if error.raw_os_error() == Some(1314) => {
                    assert!(output_component_flags_are_unsafe(true, true, false, true));
                }
                Err(error) => panic!("Windows reparse/dangling symlink fixture failed: {error}"),
            }

            let intermediate = controller.policy.contract().workspace.join("src/link-dir");
            match std::os::windows::fs::symlink_dir("missing-directory", &intermediate) {
                Ok(()) => {
                    assert!(
                        controller
                            .safe_task_output_hash("src/link-dir/output.txt")
                            .is_err()
                    );
                    fs::remove_file(&intermediate).expect("remove intermediate link fixture");
                }
                Err(error) if error.raw_os_error() == Some(1314) => {
                    assert!(output_component_flags_are_unsafe(true, true, true, false));
                }
                Err(error) => panic!("Windows intermediate reparse fixture failed: {error}"),
            }
        }
    }

    #[test]
    fn task_output_baselines_require_every_declared_artifact_to_change() {
        let controller = structured_output_controller();
        controller
            .capture_task_output_baseline("work")
            .expect("baseline");
        assert!(
            controller.verify_task_output_attribution().is_err(),
            "missing output is not attributed"
        );
        let output = controller
            .policy
            .contract()
            .workspace
            .join("src/task-output.txt");
        fs::write(&output, b"task attributable output").expect("create output");
        assert!(
            controller.verify_task_output_attribution().is_ok(),
            "absent output is attributed only after creation"
        );

        let mut multiple = controller.policy.contract().clone();
        multiple.completion_artifact_ids =
            vec!["src/task-output.txt".into(), "src/second.txt".into()];
        let mut controller = AutonomousControllerV1::new(
            AutonomousPolicyEngineV1::new(multiple).expect("multi policy"),
            controller.store.clone(),
            FakeWorkerProviderV1::new(FakeProvider::new(Vec::new())),
            Verifier {
                statuses: vec![],
                diff: "diff".into(),
                final_review: true,
            },
            "session-1".into(),
        );
        // A different approved artifact set for the same logical task cannot
        // replace the immutable baseline.
        assert!(controller.capture_task_output_baseline("work").is_err());
        controller.policy = AutonomousPolicyEngineV1::new({
            let mut contract = controller.policy.contract().clone();
            contract.completion_artifact_ids = vec!["src/task-output.txt".into()];
            contract
        })
        .expect("restore policy");
        let existing = controller
            .policy
            .contract()
            .workspace
            .join("src/task-output.txt");
        controller
            .capture_task_output_baseline("work")
            .expect("repair baseline retained");
        fs::write(&existing, b"modified attributable output").expect("modify output");
        assert!(
            controller.verify_task_output_attribution().is_ok(),
            "existing output must change hash"
        );

        let existing_controller = structured_output_controller();
        let existing_output = existing_controller
            .policy
            .contract()
            .workspace
            .join("src/task-output.txt");
        fs::write(&existing_output, b"pre-existing output").expect("seed output");
        existing_controller
            .capture_task_output_baseline("work")
            .expect("existing baseline");
        assert!(
            existing_controller
                .verify_task_output_attribution()
                .is_err(),
            "pre-existing unchanged output is not attributed"
        );
        fs::write(&existing_output, b"provider modified output").expect("modify output");
        assert!(
            existing_controller.verify_task_output_attribution().is_ok(),
            "existing output is attributed only after hash change"
        );

        let mut multiple_controller = structured_output_controller();
        let mut multiple_contract = multiple_controller.policy.contract().clone();
        multiple_contract.completion_artifact_ids =
            vec!["src/task-output.txt".into(), "src/second-output.txt".into()];
        multiple_controller.policy =
            AutonomousPolicyEngineV1::new(multiple_contract).expect("two-output policy");
        multiple_controller
            .capture_task_output_baseline("work")
            .expect("two-output baseline");
        fs::write(
            multiple_controller
                .policy
                .contract()
                .workspace
                .join("src/task-output.txt"),
            b"only one output changed",
        )
        .expect("create one output");
        assert!(
            multiple_controller
                .verify_task_output_attribution()
                .is_err(),
            "all declared outputs must be attributed"
        );
    }

    #[tokio::test]
    async fn terminal_provider_error_leaves_task_ready_for_planner_resume() {
        let mut controller = setup(VerificationStatusV1::Passed, "diff --git");
        controller
            .prepare_turn("work", "captured-provider-thread", None)
            .expect("prepare provider turn");
        let turn_id = TurnId::new("terminal-turn").expect("turn");
        let outcome = controller
            .finish_turn(
                ProviderEventBatchV1 {
                    events: vec![NormalizedProviderEventV1 {
                        provider_id: "fake".into(),
                        turn_id,
                        kind: NormalizedProviderEventKind::TerminalError,
                        text: Some("bounded provider failure".into()),
                        tool_call: None,
                    }],
                    next_cursor: 1,
                    terminal: true,
                    provider_session_id: Some("captured-provider-thread".into()),
                },
                10,
            )
            .await
            .expect("terminal provider escalation");
        assert_eq!(outcome.state, AutonomousSessionStateV1::WaitingForChatgpt);
        let queue = controller.store.load_queue("session-1").expect("queue");
        assert_eq!(queue.tasks[0].state, AutonomousQueueTaskStateV1::Ready);
        assert_eq!(
            controller
                .store
                .load_session("session-1")
                .expect("state")
                .provider_thread_id
                .as_deref(),
            Some("captured-provider-thread")
        );
    }

    #[test]
    fn terminal_provider_diagnostic_is_bounded_and_redacts_sensitive_text() {
        let event = NormalizedProviderEventV1 {
            provider_id: "fake".into(),
            turn_id: TurnId::new("turn-1").expect("turn"),
            kind: NormalizedProviderEventKind::TerminalError,
            text: Some("provider failure token=synthetic".into()),
            tool_call: None,
        };
        assert_eq!(
            bounded_provider_diagnostic(&[event]),
            "provider terminal error detail redacted"
        );
    }

    #[tokio::test]
    async fn expired_lease_stops_before_provider_launch() {
        let mut controller = setup(VerificationStatusV1::Passed, "diff");
        assert_eq!(
            controller.run_once(100).await.expect("stop").state,
            AutonomousSessionStateV1::LeaseExpired
        );
    }

    #[tokio::test]
    async fn passed_verification_checkpoint_finalizes_after_expiry_without_a_second_turn() {
        let mut initial = setup(VerificationStatusV1::Passed, "diff --git");
        initial.verifier.final_review = false;
        assert_eq!(
            initial.run_once(10).await.expect("checkpointed").state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        assert!(
            initial
                .store
                .load_passed_verification_checkpoint("session-1")
                .expect("checkpoint read")
                .is_some()
        );
        let turns_before_restart = initial
            .store
            .load_session("session-1")
            .expect("checkpoint state")
            .provider_turn_count;
        let policy = initial.policy.clone();
        let store = initial.store.clone();
        let session_id = initial.session_id.clone();
        drop(initial);

        let mut resumed = AutonomousControllerV1::new(
            policy,
            store,
            FakeWorkerProviderV1::new(FakeProvider::new(vec![FakeProviderTurn::Complete(
                "must-not-run".into(),
            )])),
            Verifier {
                statuses: vec![VerificationStatusV1::Failed],
                diff: "must-not-run".into(),
                final_review: true,
            },
            session_id,
        );
        assert_eq!(
            resumed
                .run_once(100)
                .await
                .expect("expired finalization")
                .state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(
            resumed
                .store
                .load_session("session-1")
                .expect("completed state")
                .provider_turn_count,
            turns_before_restart,
            "the expired checkpoint path must not relaunch a provider"
        );
        assert!(resumed.store.load_completion_artifacts("session-1").is_ok());
        assert_eq!(
            resumed
                .store
                .all_review_inbox()
                .expect("review inbox")
                .len(),
            1
        );
        assert_eq!(
            resumed
                .run_once(101)
                .await
                .expect("idempotent terminal")
                .state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(
            resumed
                .store
                .all_review_inbox()
                .expect("replayed inbox")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn queued_passed_verification_checkpoint_finalizes_locally_without_provider_launch() {
        let mut initial = setup(VerificationStatusV1::Passed, "diff --git");
        initial.verifier.final_review = false;
        assert_eq!(
            initial.run_once(10).await.expect("checkpointed").state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        assert!(
            initial
                .store
                .load_passed_verification_checkpoint("session-1")
                .expect("checkpoint read")
                .is_some()
        );
        let turns_before_restart = initial
            .store
            .load_session("session-1")
            .expect("checkpoint state")
            .provider_turn_count;
        let policy = initial.policy.clone();
        let store = initial.store.clone();
        let session_id = initial.session_id.clone();
        let mut queued = store.load_session(&session_id).expect("session state");
        queued.state = AutonomousSessionStateV1::Queued;
        queued.active = true;
        store
            .save_session(&queued)
            .expect("queued checkpoint state");
        drop(initial);

        let mut resumed = AutonomousControllerV1::new(
            policy,
            store,
            FakeWorkerProviderV1::new(FakeProvider::new(vec![FakeProviderTurn::Complete(
                "must-not-run".into(),
            )])),
            Verifier {
                statuses: vec![VerificationStatusV1::Failed],
                diff: "must-not-run".into(),
                final_review: true,
            },
            session_id,
        );
        assert_eq!(
            resumed
                .run_once(100)
                .await
                .expect("queued local finalization")
                .state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(
            resumed
                .store
                .load_session("session-1")
                .expect("completed state")
                .provider_turn_count,
            turns_before_restart,
            "a queued checkpoint must not relaunch a provider"
        );
        assert!(
            resumed.handles.is_empty(),
            "no provider handle may be launched"
        );
        assert!(resumed.store.load_completion_artifacts("session-1").is_ok());
        assert_eq!(
            resumed
                .run_once(101)
                .await
                .expect("idempotent terminal")
                .state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(
            resumed
                .store
                .all_review_inbox()
                .expect("review inbox")
                .len(),
            1,
            "the queued checkpoint may emit exactly one review record"
        );
    }

    #[tokio::test]
    async fn queued_without_checkpoint_retains_normal_provider_execution() {
        let mut controller = setup(VerificationStatusV1::Passed, "diff --git");
        let mut queued = controller
            .store
            .load_session("session-1")
            .expect("initial state");
        queued.state = AutonomousSessionStateV1::Queued;
        queued.active = true;
        controller
            .store
            .save_session(&queued)
            .expect("queued ordinary state");
        assert_eq!(
            controller
                .store
                .load_passed_verification_checkpoint("session-1")
                .expect("checkpoint read"),
            None
        );
        assert_eq!(
            controller
                .store
                .load_session("session-1")
                .expect("queued state")
                .state,
            AutonomousSessionStateV1::Queued
        );
        assert_eq!(
            controller.run_once(10).await.expect("provider turn").state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(
            controller.handles.len(),
            1,
            "ordinary queued work launches once"
        );
        assert_eq!(
            controller
                .store
                .load_session("session-1")
                .expect("completed state")
                .provider_turn_count,
            1
        );
    }

    #[tokio::test]
    async fn failed_verification_resumes_same_provider_session_for_bounded_repair() {
        let mut controller = setup(VerificationStatusV1::Failed, "diff");
        controller
            .verifier
            .statuses
            .push(VerificationStatusV1::Passed);
        assert_eq!(
            controller.run_once(10).await.expect("initial").state,
            AutonomousSessionStateV1::Queued
        );
        assert_eq!(
            controller.run_once(11).await.expect("repair").state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(controller.repair_attempts, 1);
        assert_eq!(controller.handles.len(), 2);
    }

    #[tokio::test]
    async fn cancellation_persists_and_prevents_a_repair_turn() {
        let mut controller = setup(VerificationStatusV1::Failed, "diff");
        assert_eq!(
            controller.run_once(10).await.expect("initial").state,
            AutonomousSessionStateV1::Queued
        );
        controller.cancel().await.expect("cancel");
        assert_eq!(
            controller.run_once(11).await.expect("cancelled").state,
            AutonomousSessionStateV1::Cancelled
        );
        assert_eq!(controller.handles.len(), 1);
        assert!(
            controller
                .store
                .load_session("session-1")
                .expect("state")
                .cancellation_requested
        );
    }

    #[tokio::test]
    async fn completion_requires_a_final_review_artifact() {
        let mut controller = setup(VerificationStatusV1::Passed, "diff --git");
        controller.verifier.final_review = false;
        assert_eq!(
            controller.run_once(10).await.expect("escalation").state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        assert_ne!(
            controller
                .store
                .load_session("session-1")
                .expect("state")
                .state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(
            controller
                .store
                .load_escalation("session-1")
                .expect("escalation")
                .reason,
            "final_review_unavailable"
        );
    }

    #[tokio::test]
    async fn rate_limit_is_persisted_then_resumed_after_backoff() {
        let initial = setup(VerificationStatusV1::Passed, "diff --git");
        let policy = initial.policy.clone();
        let store = initial.store.clone();
        let verifier = initial.verifier;
        let session_id = initial.session_id.clone();
        let mut controller = AutonomousControllerV1::new(
            policy,
            store,
            RateLimitedProvider::new(),
            verifier,
            session_id,
        );
        assert_eq!(
            controller.run_once(10).await.expect("rate limit").state,
            AutonomousSessionStateV1::RateLimited
        );
        assert_eq!(
            controller.run_once(14).await.expect("backoff").state,
            AutonomousSessionStateV1::RateLimited
        );
        assert_eq!(
            controller.run_once(15).await.expect("resume").state,
            AutonomousSessionStateV1::CompletedVerified
        );
    }

    #[tokio::test]
    async fn checkpoint_precedes_snapshot_failure_and_expired_restart_finalizes_once() {
        let mut initial = structured_output_controller();
        let workspace = initial.policy.contract().workspace.clone();
        initial
            .capture_task_output_baseline("work")
            .expect("absent output baseline");
        fs::write(workspace.join("src/task-output.txt"), b"attributed output")
            .expect("provider-style output");

        // The source inputs are otherwise valid. A regular file where the
        // snapshot root must be makes snapshot creation fail only after the
        // verifier, attribution, and checkpoint have completed.
        fs::write(
            workspace.join("Cargo.toml"),
            b"[package]\nname = \"snapshot-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("manifest");
        fs::write(workspace.join("Cargo.lock"), b"# fixture\n").expect("lockfile");
        fs::write(workspace.join("src/lib.rs"), b"pub fn fixture() {}\n").expect("source");
        fs::create_dir_all(workspace.join("wake/src")).expect("wake source directory");
        fs::write(
            workspace.join("wake/Cargo.toml"),
            b"[package]\nname = \"catdesk-wake\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
        )
        .expect("wake manifest");
        fs::write(
            workspace.join("wake/src/lib.rs"),
            b"pub const WAKE: u8 = 1;\n",
        )
        .expect("wake source");
        fs::write(workspace.join(".catdesk"), b"snapshot root blocker").expect("snapshot blocker");

        assert_eq!(
            initial
                .run_once(10)
                .await
                .expect("checkpointed failure")
                .state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        let checkpoint = initial
            .store
            .load_passed_verification_checkpoint("session-1")
            .expect("checkpoint read")
            .expect("checkpoint persisted before snapshot");
        assert_eq!(
            checkpoint
                .reviewed_source_expectation
                .as_ref()
                .expect("persisted output expectation")
                .current_outputs[0]
                .artifact_id,
            "src/task-output.txt"
        );
        let turns = initial
            .store
            .load_session("session-1")
            .expect("state")
            .provider_turn_count;
        assert!(
            initial
                .store
                .load_completion_artifacts("session-1")
                .is_err()
        );

        fs::remove_file(workspace.join(".catdesk")).expect("remove only snapshot blocker");
        fs::create_dir(workspace.join(".catdesk")).expect("snapshot root");
        let policy = initial.policy.clone();
        let store = initial.store.clone();
        let session_id = initial.session_id.clone();
        drop(initial);

        let mut resumed = AutonomousControllerV1::new(
            policy,
            store,
            FakeWorkerProviderV1::new(FakeProvider::new(vec![FakeProviderTurn::Complete(
                "must-not-run".into(),
            )])),
            Verifier {
                statuses: vec![VerificationStatusV1::Failed],
                diff: "must-not-run".into(),
                final_review: true,
            },
            session_id,
        );
        assert_eq!(
            resumed
                .run_once(100)
                .await
                .expect("expired local finalization")
                .state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(
            resumed
                .store
                .load_session("session-1")
                .expect("completed")
                .provider_turn_count,
            turns,
            "recovery must not launch a second provider turn"
        );
        assert_eq!(
            resumed
                .store
                .all_review_inbox()
                .expect("review inbox")
                .len(),
            1
        );
        assert_eq!(
            resumed
                .run_once(101)
                .await
                .expect("idempotent terminal tick")
                .state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(
            resumed
                .store
                .all_review_inbox()
                .expect("review inbox")
                .len(),
            1,
            "the same passed checkpoint emits one inbox record"
        );
    }

    #[tokio::test]
    async fn checkpointed_output_hash_drift_fails_closed_without_a_second_turn() {
        let mut controller = structured_output_controller();
        let workspace = controller.policy.contract().workspace.clone();
        controller
            .capture_task_output_baseline("work")
            .expect("absent output baseline");
        fs::write(
            workspace.join("src/task-output.txt"),
            b"original attributed output",
        )
        .expect("provider-style output");
        fs::write(
            workspace.join("Cargo.toml"),
            b"[package]\nname = \"snapshot-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("manifest");
        fs::write(workspace.join("Cargo.lock"), b"# fixture\n").expect("lockfile");
        fs::write(workspace.join("src/lib.rs"), b"pub fn fixture() {}\n").expect("source");
        fs::create_dir_all(workspace.join("wake/src")).expect("wake source directory");
        fs::write(
            workspace.join("wake/Cargo.toml"),
            b"[package]\nname = \"catdesk-wake\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
        )
        .expect("wake manifest");
        fs::write(
            workspace.join("wake/src/lib.rs"),
            b"pub const WAKE: u8 = 1;\n",
        )
        .expect("wake source");
        fs::write(workspace.join(".catdesk"), b"snapshot root blocker").expect("snapshot blocker");

        assert_eq!(
            controller
                .run_once(10)
                .await
                .expect("checkpointed failure")
                .state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        let turns = controller
            .store
            .load_session("session-1")
            .expect("state")
            .provider_turn_count;
        let mut queued = controller.store.load_session("session-1").expect("state");
        queued.state = AutonomousSessionStateV1::Queued;
        queued.active = true;
        controller
            .store
            .save_session(&queued)
            .expect("queued checkpoint state");
        fs::write(
            workspace.join("src/task-output.txt"),
            b"changed after checkpoint",
        )
        .expect("hostile drift");
        fs::remove_file(workspace.join(".catdesk")).expect("remove snapshot blocker");
        fs::create_dir(workspace.join(".catdesk")).expect("snapshot root");

        assert_eq!(
            controller.run_once(100).await.expect("fail closed").state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        assert_eq!(
            controller
                .store
                .load_session("session-1")
                .expect("state")
                .provider_turn_count,
            turns
        );
        assert!(
            controller
                .store
                .load_completion_artifacts("session-1")
                .is_err()
        );
        assert!(
            controller
                .store
                .all_review_inbox()
                .expect("review inbox")
                .is_empty()
        );
    }

    fn route_controller(
        provider: CodexRouteTestProvider,
    ) -> AutonomousControllerV1<CodexRouteTestProvider, Verifier> {
        let initial = setup(VerificationStatusV1::Passed, "diff --git");
        AutonomousControllerV1::new(
            initial.policy,
            initial.store,
            provider,
            initial.verifier,
            initial.session_id,
        )
    }

    #[tokio::test]
    async fn explicit_routine_mode_starts_on_qwen_without_credit_handoff() {
        let initial = setup(VerificationStatusV1::Passed, "diff --git");
        let mut contract = initial.policy.contract().clone();
        contract.mode = "chatgpt_web_qwen_autonomous".into();
        let policy = AutonomousPolicyEngineV1::new(contract.clone()).expect("routine policy");
        let store = initial.store;
        let verifier = initial.verifier;
        let session_id = initial.session_id;

        store
            .save_contract(&session_id, &contract)
            .expect("routine contract");
        let mut snapshot = store.load_session(&session_id).expect("session");
        snapshot.state = AutonomousSessionStateV1::Queued;
        snapshot.provider_route = AutonomousProviderRouteV1::QwenFallbackActive;
        snapshot.provider_thread_id = None;
        snapshot.expected_codex_thread_id = None;
        snapshot.codex_eligible_after_unix = None;
        snapshot.approved_contract_hash = Some(policy.contract_hash().to_string());
        store.save_session(&snapshot).expect("routine route");
        assert!(
            store.load_provider_handoff(&session_id).is_err(),
            "initial routine-provider mode must not require a synthetic Codex exhaustion handoff"
        );

        let mut controller = AutonomousControllerV1::new(
            policy,
            store,
            CodexRouteTestProvider::new(false, true),
            verifier,
            session_id,
        );
        assert_eq!(
            controller
                .run_once(10)
                .await
                .expect("routine Qwen turn")
                .state,
            AutonomousSessionStateV1::Queued
        );
        assert_eq!(
            controller
                .store
                .load_session("session-1")
                .expect("post-Qwen state")
                .provider_turn_count,
            1
        );
        assert_eq!(controller.provider.fallback_activations, 1);
        assert_eq!(controller.provider.launches, vec![ProviderIdV1::Ollama]);
        assert!(!controller.provider.tool_sets[0].1.is_empty());
        assert!(
            controller.store.load_provider_handoff("session-1").is_err(),
            "initial Qwen execution must remain distinguishable from credit-exhaustion fallback"
        );
    }

    #[tokio::test]
    async fn transient_codex_429_does_not_activate_local_fallback() {
        let mut controller = route_controller(CodexRouteTestProvider::new(false, true));
        assert_eq!(
            controller.run_once(10).await.expect("rate limit").state,
            AutonomousSessionStateV1::RateLimited
        );
        assert_eq!(controller.provider.fallback_activations, 0);
        assert_eq!(
            controller.run_once(15).await.expect("resume").state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(
            controller.provider.launches,
            vec![ProviderIdV1::CodexCli, ProviderIdV1::CodexCli]
        );
    }

    #[tokio::test]
    async fn codex_mutation_is_fail_closed_without_authoritative_terra_high_metadata() {
        let mut controller = route_controller(CodexRouteTestProvider::new(false, true));
        let mut snapshot = controller.store.load_session("session-1").expect("session");
        snapshot.codex_routing_telemetry = None;
        controller
            .store
            .save_session(&snapshot)
            .expect("remove gate");
        assert_eq!(
            controller.run_once(10).await.expect("safe stop").state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        assert!(controller.provider.launches.is_empty());
        assert_eq!(
            controller
                .store
                .load_escalation("session-1")
                .expect("escalation")
                .reason,
            "codex_terra_high_gate_unavailable_or_mismatched"
        );
    }

    #[tokio::test]
    async fn unavailable_account_telemetry_still_requires_and_accepts_authoritative_terra_high() {
        let mut accepted = route_controller(CodexRouteTestProvider::new(false, true));
        let mut snapshot = accepted.store.load_session("session-1").expect("session");
        let telemetry = snapshot
            .codex_routing_telemetry
            .as_mut()
            .expect("host telemetry");
        telemetry.source = "codex-app-server/account-rateLimits-unavailable".into();
        accepted.store.save_session(&snapshot).expect("persist");
        assert_eq!(
            accepted
                .run_once(10)
                .await
                .expect("authoritative gate")
                .state,
            AutonomousSessionStateV1::RateLimited
        );
        assert_eq!(accepted.provider.launches, vec![ProviderIdV1::CodexCli]);

        let mut rejected = route_controller(CodexRouteTestProvider::new(false, true));
        let mut snapshot = rejected.store.load_session("session-1").expect("session");
        let telemetry = snapshot
            .codex_routing_telemetry
            .as_mut()
            .expect("host telemetry");
        telemetry.source = "codex-app-server/account-rateLimits-unavailable".into();
        telemetry.reasoning_effort = Some("medium".into());
        rejected.store.save_session(&snapshot).expect("persist");
        assert_eq!(
            rejected.run_once(10).await.expect("fail closed").state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        assert!(rejected.provider.launches.is_empty());
    }

    #[tokio::test]
    async fn credit_exhaustion_hands_same_task_to_qwen_without_replaying_codex() {
        let mut controller = route_controller(CodexRouteTestProvider::new(true, true));
        assert_eq!(
            controller.run_once(10).await.expect("handoff").state,
            AutonomousSessionStateV1::Queued
        );
        let handoff = controller
            .store
            .load_provider_handoff("session-1")
            .expect("handoff persisted");
        assert_eq!(handoff.pending_task_id, "work");
        assert_eq!(handoff.from_provider_id, "codex-cli");
        assert_eq!(handoff.to_provider_id, "ollama");
        assert_eq!(controller.provider.fallback_activations, 1);
        assert_eq!(
            controller
                .run_once(11)
                .await
                .expect("qwen continuation")
                .state,
            AutonomousSessionStateV1::Queued
        );
        assert_eq!(
            controller
                .store
                .load_session("session-1")
                .expect("persisted rejection")
                .state,
            AutonomousSessionStateV1::Queued
        );
        assert_eq!(
            controller.provider.launches,
            vec![ProviderIdV1::CodexCli, ProviderIdV1::Ollama]
        );
        assert!(controller.provider.tool_sets[0].1.is_empty());
        assert_eq!(
            controller.provider.tool_sets[1].1,
            vec![
                "read",
                "search",
                "patch.preview",
                "patch.apply",
                "patch.compare",
                "diff.actual",
                "verify.run"
            ]
        );
        assert_eq!(controller.handles.len(), 2);
    }

    #[tokio::test]
    async fn unavailable_qwen_escalates_without_cloud_or_api_fallback() {
        let mut controller = route_controller(CodexRouteTestProvider::new(true, false));
        assert_eq!(
            controller.run_once(10).await.expect("escalate").state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        assert_eq!(controller.provider.launches, vec![ProviderIdV1::CodexCli]);
        assert_eq!(controller.provider.fallback_activations, 1);
        assert_eq!(
            controller
                .store
                .load_session("session-1")
                .expect("state")
                .provider_route,
            AutonomousProviderRouteV1::QwenUnavailable
        );
    }

    #[tokio::test]
    async fn queued_stale_worker_running_recovers_and_resumes_persisted_thread() {
        let initial = setup(VerificationStatusV1::Passed, "diff --git");
        let policy = initial.policy.clone();
        let store = initial.store.clone();
        let verifier = initial.verifier;
        let session_id = initial.session_id.clone();
        let mut snapshot = store.load_session(&session_id).expect("state");
        snapshot.state = AutonomousSessionStateV1::Queued;
        snapshot.current_task_id = Some("work".into());
        snapshot.provider_thread_id = Some("prior-provider-session".into());
        snapshot.expected_codex_thread_id = Some("prior-provider-session".into());
        snapshot.provider_handle_id = Some("stale-terminal-handle".into());
        snapshot.provider_turn_count = 1;
        store.save_session(&snapshot).expect("persist state");
        let mut queue = store.load_queue(&session_id).expect("queue");
        queue.tasks[0].state = AutonomousQueueTaskStateV1::WorkerRunning;
        store
            .save_queue(&session_id, &queue)
            .expect("persist queue");
        let accounting = store.execution_accounting_store().expect("accounting");
        accounting
            .record_task_started(
                "catdesk",
                "work",
                &session_id,
                Some("prior-provider-session"),
                "codex-cli",
                Some("model"),
                None,
                1,
            )
            .expect("turn accounting");
        accounting
            .activity_started(
                &session_id,
                ActivityActorV1::CodexProviderActive,
                ActivityEvidenceV1::ProviderLifecycle,
                2,
            )
            .expect("provider active");
        accounting
            .interrupt_open_spans(&session_id)
            .expect("interrupted provider");

        let mut controller = AutonomousControllerV1::new(
            policy,
            store,
            FakeWorkerProviderV1::new(FakeProvider::new(vec![FakeProviderTurn::Complete(
                "resumed".into(),
            )])),
            verifier,
            session_id,
        );
        assert_eq!(
            controller
                .run_once(10)
                .await
                .expect("recover and resume")
                .state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(
            controller
                .last_handle
                .as_ref()
                .expect("handle")
                .provider_session_id,
            "prior-provider-session"
        );
    }

    #[test]
    fn restart_reconciliation_requires_interrupted_owner_and_preserves_task_attribution() {
        let mut initial = setup(VerificationStatusV1::Passed, "diff --git");
        std::fs::write(
            initial
                .policy
                .contract()
                .workspace
                .join("src")
                .join("completion.bin"),
            b"approval-bound-output",
        )
        .expect("completion output");
        let mut contract = initial.policy.contract().clone();
        contract.task_id = "work".into();
        contract.completion_artifact_ids = vec!["src/completion.bin".into()];
        initial.policy = AutonomousPolicyEngineV1::new(contract).expect("policy");
        initial
            .capture_task_output_baseline("work")
            .expect("baseline captured before restart");
        let store = initial.store.clone();
        let session_id = initial.session_id.clone();
        let baseline_before = store
            .load_task_output_baseline(&session_id, "work")
            .expect("baseline")
            .expect("baseline present");
        let mut snapshot = store.load_session(&session_id).expect("state");
        snapshot.state = AutonomousSessionStateV1::Queued;
        snapshot.current_task_id = Some("work".into());
        snapshot.provider_thread_id = Some("canonical-thread".into());
        snapshot.expected_codex_thread_id = Some("canonical-thread".into());
        snapshot.provider_handle_id = Some("durably-interrupted-handle".into());
        snapshot.provider_turn_count = 3;
        store.save_session(&snapshot).expect("state persisted");
        let mut queue = store.load_queue(&session_id).expect("queue");
        queue.tasks[0].state = AutonomousQueueTaskStateV1::WorkerRunning;
        store
            .save_queue(&session_id, &queue)
            .expect("queue persisted");
        let accounting = store.execution_accounting_store().expect("accounting");
        accounting
            .record_task_started(
                "project",
                "work",
                &session_id,
                Some("canonical-thread"),
                "codex-cli",
                Some("model"),
                None,
                1,
            )
            .expect("record");

        // Earlier failed verification is historical after its completed span;
        // the later repair provider span is the only ownership being
        // reconciled.
        accounting
            .activity_started(
                &session_id,
                ActivityActorV1::CatdeskVerificationReviewActive,
                ActivityEvidenceV1::VerifierLifecycle,
                2,
            )
            .expect("historical review");
        accounting
            .activity_finished(
                &session_id,
                ActivityActorV1::CatdeskVerificationReviewActive,
                3,
            )
            .expect("historical review completed");

        // A live span is never inferred to be dead merely because the outer
        // session is queued or a handle id looks stale.
        accounting
            .activity_started(
                &session_id,
                ActivityActorV1::CodexProviderActive,
                ActivityEvidenceV1::ProviderLifecycle,
                4,
            )
            .expect("open span");
        assert_eq!(
            reconcile_restart_task_state(&store, &session_id).expect("classify live"),
            RestartTaskReconciliationV1::Pending
        );
        assert_eq!(
            store.load_queue(&session_id).expect("queue").tasks[0].state,
            AutonomousQueueTaskStateV1::WorkerRunning
        );

        accounting
            .interrupt_open_spans(&session_id)
            .expect("durable interruption");
        assert_eq!(
            reconcile_restart_task_state(&store, &session_id).expect("requeue"),
            RestartTaskReconciliationV1::Requeued
        );
        assert_eq!(
            reconcile_restart_task_state(&store, &session_id).expect("idempotent replay"),
            RestartTaskReconciliationV1::AlreadyReady
        );
        assert_eq!(
            store
                .load_task_output_baseline(&session_id, "work")
                .expect("baseline")
                .expect("baseline present"),
            baseline_before,
            "restart reconciliation never recaptures or rewrites task-output attribution"
        );
        let after = store.load_session(&session_id).expect("state after");
        assert_eq!(after.provider_turn_count, 3);
        assert_eq!(
            after.provider_thread_id.as_deref(),
            Some("canonical-thread")
        );
        assert_eq!(
            after.expected_codex_thread_id.as_deref(),
            Some("canonical-thread")
        );
    }

    #[test]
    fn restart_reconciliation_rejects_outcome_unknown_tool_evidence() {
        let initial = setup(VerificationStatusV1::Passed, "diff --git");
        let store = initial.store.clone();
        let session_id = initial.session_id.clone();
        let mut snapshot = store.load_session(&session_id).expect("state");
        snapshot.state = AutonomousSessionStateV1::Queued;
        snapshot.current_task_id = Some("work".into());
        snapshot.provider_thread_id = Some("canonical-thread".into());
        snapshot.expected_codex_thread_id = Some("canonical-thread".into());
        snapshot.provider_turn_count = 1;
        store.save_session(&snapshot).expect("state persisted");
        let mut queue = store.load_queue(&session_id).expect("queue");
        queue.tasks[0].state = AutonomousQueueTaskStateV1::WorkerRunning;
        store
            .save_queue(&session_id, &queue)
            .expect("queue persisted");
        let accounting = store.execution_accounting_store().expect("accounting");
        accounting
            .record_task_started(
                "project",
                "work",
                &session_id,
                Some("canonical-thread"),
                "codex-cli",
                Some("model"),
                None,
                1,
            )
            .expect("record");
        accounting
            .activity_started(
                &session_id,
                ActivityActorV1::CodexProviderActive,
                ActivityEvidenceV1::ProviderLifecycle,
                2,
            )
            .expect("span");
        accounting
            .interrupt_open_spans(&session_id)
            .expect("interrupted");
        accounting
            .record_events(&session_id, 1, 1)
            .expect("outcome-unknown tool evidence");
        assert_eq!(
            reconcile_restart_task_state(&store, &session_id).expect("classify"),
            RestartTaskReconciliationV1::Pending
        );
        assert_eq!(
            store.load_queue(&session_id).expect("queue").tasks[0].state,
            AutonomousQueueTaskStateV1::WorkerRunning
        );
    }

    #[test]
    fn restart_reconciliation_rejects_open_or_interrupted_reviewer_ownership() {
        let prepare_interrupted_provider = || {
            let initial = setup(VerificationStatusV1::Passed, "diff --git");
            let store = initial.store.clone();
            let session_id = initial.session_id.clone();
            let mut snapshot = store.load_session(&session_id).expect("state");
            snapshot.state = AutonomousSessionStateV1::Queued;
            snapshot.current_task_id = Some("work".into());
            snapshot.provider_thread_id = Some("canonical-thread".into());
            snapshot.expected_codex_thread_id = Some("canonical-thread".into());
            snapshot.provider_turn_count = 2;
            store.save_session(&snapshot).expect("state persisted");
            let mut queue = store.load_queue(&session_id).expect("queue");
            queue.tasks[0].state = AutonomousQueueTaskStateV1::WorkerRunning;
            store
                .save_queue(&session_id, &queue)
                .expect("queue persisted");
            let accounting = store.execution_accounting_store().expect("accounting");
            accounting
                .record_task_started(
                    "project",
                    "work",
                    &session_id,
                    Some("canonical-thread"),
                    "codex-cli",
                    Some("model"),
                    None,
                    1,
                )
                .expect("record");
            accounting
                .activity_started(
                    &session_id,
                    ActivityActorV1::CodexProviderActive,
                    ActivityEvidenceV1::ProviderLifecycle,
                    2,
                )
                .expect("provider span");
            accounting
                .interrupt_open_spans(&session_id)
                .expect("provider interrupted");
            (store, session_id, accounting)
        };

        let (store, session_id, accounting) = prepare_interrupted_provider();
        accounting
            .activity_started(
                &session_id,
                ActivityActorV1::CatdeskVerificationReviewActive,
                ActivityEvidenceV1::VerifierLifecycle,
                3,
            )
            .expect("reviewer open");
        assert_eq!(
            reconcile_restart_task_state(&store, &session_id).expect("open reviewer"),
            RestartTaskReconciliationV1::Pending
        );
        assert_eq!(
            store.load_queue(&session_id).expect("queue").tasks[0].state,
            AutonomousQueueTaskStateV1::WorkerRunning
        );

        let (store, session_id, accounting) = prepare_interrupted_provider();
        accounting
            .activity_started(
                &session_id,
                ActivityActorV1::CatdeskVerificationReviewActive,
                ActivityEvidenceV1::VerifierLifecycle,
                3,
            )
            .expect("reviewer span");
        accounting
            .interrupt_open_spans(&session_id)
            .expect("reviewer interruption");
        assert_eq!(
            reconcile_restart_task_state(&store, &session_id).expect("interrupted reviewer"),
            RestartTaskReconciliationV1::Pending
        );
        assert_eq!(
            store.load_queue(&session_id).expect("queue").tasks[0].state,
            AutonomousQueueTaskStateV1::WorkerRunning
        );
    }

    #[tokio::test]
    async fn restart_recovery_resumes_the_persisted_provider_session() {
        let initial = setup(VerificationStatusV1::Passed, "diff --git");
        let policy = initial.policy.clone();
        let store = initial.store.clone();
        let verifier = initial.verifier;
        let session_id = initial.session_id.clone();
        let mut snapshot = store.load_session(&session_id).expect("state");
        snapshot.state = AutonomousSessionStateV1::RecoveringAfterRestart;
        snapshot.current_task_id = Some("work".into());
        snapshot.provider_thread_id = Some("prior-provider-session".into());
        store.save_session(&snapshot).expect("persist recovery");
        let mut controller = AutonomousControllerV1::new(
            policy,
            store,
            FakeWorkerProviderV1::new(FakeProvider::new(vec![FakeProviderTurn::Complete(
                "resumed".into(),
            )])),
            verifier,
            session_id,
        );
        assert_eq!(
            controller.run_once(10).await.expect("resume").state,
            AutonomousSessionStateV1::CompletedVerified
        );
        assert_eq!(
            controller
                .last_handle
                .as_ref()
                .expect("handle")
                .provider_session_id,
            "prior-provider-session"
        );
    }

    #[tokio::test]
    async fn active_turn_is_polled_without_starting_another_turn() {
        let mut controller = setup(VerificationStatusV1::Passed, "diff --git");
        controller.provider = FakeWorkerProviderV1::new(FakeProvider::new(vec![
            FakeProviderTurn::Text("still running".into()),
            FakeProviderTurn::Complete("must not start yet".into()),
        ]));
        assert_eq!(
            controller.run_once(10).await.expect("start").state,
            AutonomousSessionStateV1::Running
        );
        assert_eq!(
            controller.run_once(11).await.expect("poll").state,
            AutonomousSessionStateV1::Running
        );
        assert_eq!(controller.handles.len(), 1);
    }

    #[tokio::test]
    async fn verified_task_does_not_complete_a_queue_with_more_ready_work() {
        let mut controller = setup(VerificationStatusV1::Passed, "diff --git");
        let mut queue = controller.store.load_queue("session-1").expect("queue");
        queue.tasks.push(AutonomousQueueTaskV1 {
            task_id: "follow-up".into(),
            priority: 1,
            depends_on: vec!["work".into()],
            state: AutonomousQueueTaskStateV1::Ready,
        });
        controller
            .store
            .save_queue("session-1", &queue)
            .expect("persist queue");
        assert_eq!(
            controller.run_once(10).await.expect("first task").state,
            AutonomousSessionStateV1::Queued
        );
        assert_eq!(
            controller
                .store
                .load_queue("session-1")
                .expect("queue")
                .next_ready_task()
                .expect("follow-up")
                .task_id,
            "follow-up"
        );
    }

    #[tokio::test]
    async fn persisted_provider_turn_budget_blocks_a_new_launch() {
        let mut controller = setup(VerificationStatusV1::Passed, "diff --git");
        let mut snapshot = controller.store.load_session("session-1").expect("state");
        snapshot.provider_turn_count = controller
            .policy
            .contract()
            .autonomy_lease
            .maximum_provider_turns;
        controller.store.save_session(&snapshot).expect("persist");
        assert_eq!(
            controller.run_once(10).await.expect("budget").state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        assert_eq!(controller.handles.len(), 0);
        assert_eq!(
            controller
                .store
                .load_escalation("session-1")
                .expect("escalation")
                .reason,
            "provider_turn_budget_exhausted"
        );
    }

    #[tokio::test]
    async fn confirmed_codex_credit_exhaustion_hands_off_once_to_local_qwen() {
        let initial = setup(VerificationStatusV1::Passed, "diff --git");
        let policy = initial.policy.clone();
        let store = initial.store.clone();
        let verifier = initial.verifier;
        let session_id = initial.session_id.clone();
        let mut controller = AutonomousControllerV1::new(
            policy,
            store,
            CreditRouteProvider::new(true),
            verifier,
            session_id,
        );
        assert_eq!(
            controller.run_once(10).await.expect("credit handoff").state,
            AutonomousSessionStateV1::Queued
        );
        let persisted = controller.store.load_session("session-1").expect("state");
        assert_eq!(
            persisted.provider_route,
            AutonomousProviderRouteV1::QwenFallbackActive
        );
        let handoff = controller
            .store
            .load_provider_handoff("session-1")
            .expect("handoff");
        assert_eq!(handoff.from_provider_id, "codex-cli");
        assert_eq!(handoff.to_provider_id, "ollama");
        assert_eq!(handoff.pending_task_id, "work");
        assert_eq!(controller.provider.launches, 1);
        assert_eq!(
            controller
                .run_once(11)
                .await
                .expect("local continuation")
                .state,
            AutonomousSessionStateV1::Queued
        );
        assert_eq!(controller.provider.launches, 2);
    }

    #[test]
    fn reset_boundary_is_extracted_only_from_one_positive_exhaustion_diagnostic() {
        let clock = super::super::codex_cli::CodexTrustedLocalClockV1 {
            observed_at_unix: 100,
            year: 2026,
            month: 8,
            day: 28,
            hour: 10,
            minute: 53,
            second: 0,
        };
        let exhausted = |kind, text: &str| NormalizedProviderEventV1 {
            provider_id: "codex-cli".into(),
            turn_id: TurnId::new("turn-reset").expect("turn"),
            kind,
            text: Some(text.into()),
            tool_call: None,
        };
        let live_shape = [
            exhausted(
                NormalizedProviderEventKind::TextDelta,
                "You've hit your usage limit. Try again at Sep 2nd, 2026 12:50 AM.",
            ),
            exhausted(
                NormalizedProviderEventKind::TerminalError,
                "Codex CLI turn failed",
            ),
        ];
        assert_eq!(
            codex_credit_exhaustion_diagnostic_from_events(&live_shape),
            Some("You've hit your usage limit. Try again at Sep 2nd, 2026 12:50 AM.")
        );
        assert!(
            parse_codex_credits_reset_after(
                codex_credit_exhaustion_diagnostic_from_events(&live_shape)
                    .expect("one attested diagnostic"),
                clock,
            )
            .is_some(),
            "one future explicit provider date yields one durable boundary"
        );
        assert_eq!(
            codex_credit_exhaustion_diagnostic_from_events(&[exhausted(
                NormalizedProviderEventKind::TerminalError,
                "ordinary provider error"
            ),]),
            None,
            "generic terminal errors cannot become credit exhaustion"
        );
        let transient = exhausted(
            NormalizedProviderEventKind::TerminalError,
            "HTTP 429 too many requests",
        );
        assert!(is_rate_limit_event(&transient));
        assert_eq!(
            codex_credit_exhaustion_diagnostic_from_events(&[transient]),
            None,
            "a transient rate limit remains on the Codex backoff path"
        );
    }

    #[tokio::test]
    async fn live_usage_diagnostic_then_generic_terminal_hands_off_once_to_qwen() {
        let initial = setup(VerificationStatusV1::Passed, "diff --git");
        let policy = initial.policy.clone();
        let store = initial.store.clone();
        let verifier = initial.verifier;
        let session_id = initial.session_id.clone();
        let mut controller = AutonomousControllerV1::new(
            policy,
            store,
            CreditRouteProvider::new(true).with_diagnostic_then_generic_terminal(),
            verifier,
            session_id,
        );
        assert_eq!(
            controller
                .run_once(10)
                .await
                .expect("live-shape handoff")
                .state,
            AutonomousSessionStateV1::Queued
        );
        let snapshot = controller.store.load_session("session-1").expect("state");
        assert_eq!(
            snapshot.provider_route,
            AutonomousProviderRouteV1::QwenFallbackActive
        );
        assert_eq!(
            controller
                .store
                .load_provider_handoff("session-1")
                .expect("handoff")
                .codex_thread_id
                .as_deref(),
            Some("codex-cli-session"),
            "the canonical Codex thread is retained for safe-boundary restoration"
        );
        assert_eq!(controller.provider.launches, 1, "no second Codex launch");
        assert_eq!(
            controller
                .run_once(11)
                .await
                .expect("Qwen continuation")
                .state,
            AutonomousSessionStateV1::Queued
        );
        assert_eq!(controller.provider.launches, 2);
    }

    #[tokio::test]
    async fn persisted_reset_keeps_qwen_sticky_then_restores_exact_codex_thread_at_boundary() {
        let initial = setup(VerificationStatusV1::Passed, "diff --git");
        let policy = initial.policy.clone();
        let store = initial.store.clone();
        let verifier = initial.verifier;
        let session_id = initial.session_id.clone();
        let mut controller = AutonomousControllerV1::new(
            policy,
            store,
            CreditRouteProvider::new(true).with_codex_restore(),
            verifier,
            session_id,
        );
        controller.run_once(10).await.expect("handoff");
        let mut snapshot = controller.store.load_session("session-1").expect("state");
        snapshot.codex_eligible_after_unix = Some(100);
        snapshot.state = AutonomousSessionStateV1::Queued;
        controller.store.save_session(&snapshot).expect("boundary");

        controller.run_once(99).await.expect("sticky qwen");
        assert_eq!(
            controller.provider.launches, 2,
            "no Codex probe before reset"
        );
        assert!(controller.provider.qwen_active);

        let mut boundary = controller.store.load_session("session-1").expect("state");
        boundary.state = AutonomousSessionStateV1::Queued;
        boundary.provider_handle_id = None;
        controller
            .store
            .save_session(&boundary)
            .expect("safe boundary");
        let mut queue = controller.store.load_queue("session-1").expect("queue");
        queue.tasks[0].state = AutonomousQueueTaskStateV1::Ready;
        controller
            .store
            .save_queue("session-1", &queue)
            .expect("queue");
        controller.last_handle = None;
        assert_eq!(boundary.codex_eligible_after_unix, Some(100));
        assert_eq!(boundary.current_task_id.as_deref(), Some("work"));
        assert_eq!(
            boundary.provider_route,
            AutonomousProviderRouteV1::QwenFallbackActive
        );
        controller.run_once(100).await.expect("restore");
        let restored = controller
            .store
            .load_session("session-1")
            .expect("restored state");
        assert_eq!(
            restored.provider_route,
            AutonomousProviderRouteV1::CodexPreferred
        );
        assert_eq!(
            restored.provider_thread_id.as_deref(),
            Some("codex-cli-session"),
            "the stored handoff thread, not a replacement session, is restored"
        );
        assert_eq!(restored.codex_eligible_after_unix, None);
        assert!(!controller.provider.qwen_active);
    }

    #[tokio::test]
    async fn a_still_exhausted_codex_turn_refreshes_the_persisted_qwen_boundary() {
        let initial = setup(VerificationStatusV1::Passed, "diff --git");
        let policy = initial.policy.clone();
        let store = initial.store.clone();
        let verifier = initial.verifier;
        let session_id = initial.session_id.clone();
        let mut controller = AutonomousControllerV1::new(
            policy,
            store,
            CreditRouteProvider::new(true),
            verifier,
            session_id,
        );
        controller.run_once(10).await.expect("initial handoff");
        controller
            .handoff_to_qwen(11, 1, Some(200))
            .await
            .expect("refreshed handoff");
        assert_eq!(
            controller
                .store
                .load_session("session-1")
                .expect("persisted")
                .codex_eligible_after_unix,
            Some(200)
        );
    }

    #[tokio::test]
    async fn unavailable_qwen_escalates_without_selecting_another_provider() {
        let initial = setup(VerificationStatusV1::Passed, "diff --git");
        let policy = initial.policy.clone();
        let store = initial.store.clone();
        let verifier = initial.verifier;
        let session_id = initial.session_id.clone();
        let mut controller = AutonomousControllerV1::new(
            policy,
            store,
            CreditRouteProvider::new(false),
            verifier,
            session_id,
        );
        assert_eq!(
            controller.run_once(10).await.expect("escalation").state,
            AutonomousSessionStateV1::WaitingForChatgpt
        );
        assert_eq!(controller.provider.launches, 1);
        assert_eq!(
            controller
                .store
                .load_session("session-1")
                .expect("state")
                .provider_route,
            AutonomousProviderRouteV1::QwenUnavailable
        );
        assert_eq!(
            controller
                .store
                .load_escalation("session-1")
                .expect("escalation")
                .reason,
            "qwen_unavailable_after_codex_credit_exhaustion"
        );
    }
}
