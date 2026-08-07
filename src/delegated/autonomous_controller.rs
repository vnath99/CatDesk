//! Contract-governed autonomous provider and verification controller.

use std::collections::BTreeMap;

use serde_json::json;

use super::autonomous_contract::AutonomousPolicyEngineV1;
use super::autonomy_state::{AutonomousSessionStateV1, AutonomousStateStoreV1};
use super::contracts::{RunId, TurnId, WorkerSessionId};
use super::coordinator::{VerificationStatusV1, VerificationSummaryV1};
use super::runtime::{
    NormalizedProviderEventKind, ProviderMessageV1, ProviderTurnRequestV1, RuntimeError,
};
use super::worker_provider::{
    ProviderEventBatchV1, ProviderTurnHandleV1, WorkerProviderTurnRequestV1, WorkerProviderV1,
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
}

impl<P: WorkerProviderV1, V: AutonomousVerifierV1> AutonomousControllerV1<P, V> {
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
        }
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
        let snapshot = self
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
        if snapshot.state == AutonomousSessionStateV1::RecoveringAfterRestart
            && snapshot.provider_thread_id.is_none()
        {
            return self.escalate("restart_session_continuity_missing");
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
        let queue = self
            .store
            .load_queue(&self.session_id)
            .map_err(RuntimeError::from)?;
        let task = queue.next_ready_task().ok_or_else(|| {
            RuntimeError::Validation("no dependency-satisfied autonomous task is ready".into())
        })?;
        let worker = WorkerSessionId::new(format!("autonomy-{}", self.session_id))
            .map_err(RuntimeError::Validation)?;
        let mut session = self.provider.create_session(
            &self.policy.contract().provider_policy.primary_model,
            &worker,
        );
        if let Some(provider_thread_id) = &snapshot.provider_thread_id {
            session.provider_session_id = provider_thread_id.clone();
        }
        let turn = ProviderTurnRequestV1 {
            run_id: RunId::new(self.session_id.clone()).map_err(RuntimeError::Validation)?,
            worker_session_id: worker,
            turn_id: TurnId::new(format!("{}-turn-1", task.task_id))
                .map_err(RuntimeError::Validation)?,
            model_id: self.policy.contract().provider_policy.primary_model.clone(),
            context_json: json!({"contractHash": self.policy.contract_hash(), "taskId": task.task_id}),
            tool_definitions: Vec::new(),
            max_output_bytes: 16 * 1024,
        };
        let is_repair = snapshot.provider_thread_id.is_some();
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
        let request = WorkerProviderTurnRequestV1 {
            provider_session: session,
            turn: turn.clone(),
            history: vec![ProviderMessageV1 {
                role: "user".into(),
                content: worker_instruction,
                tool_call_id: None,
                tool_name: None,
            }],
            json_envelope_recovery: false,
        };
        self.prepare_turn(&task.task_id, &provider_session_id)?;
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
        let handle = if is_repair {
            self.provider.resume_turn(request).await?
        } else {
            self.provider.start_turn(request).await?
        };
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
        if self.cancel_requested {
            return self.stop(
                AutonomousSessionStateV1::Cancelled,
                "cancellation_requested",
            );
        }
        if let Some(provider_session_id) = &batch.provider_session_id {
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
        if batch.events.iter().any(is_rate_limit_event) {
            return self.pause_for_rate_limit(now_unix, batch.events.len());
        }
        if batch
            .events
            .iter()
            .any(|event| event.kind == NormalizedProviderEventKind::TerminalError)
        {
            return self.escalate("provider_terminal_error");
        }
        self.transition(
            AutonomousSessionStateV1::Verifying,
            "independent_verification_started",
        )?;
        let (verification, diff) = self.verifier.verify()?;
        if verification.status != VerificationStatusV1::Passed || diff.trim().is_empty() {
            self.repair_attempts = self.repair_attempts.saturating_add(1);
            self.mark_task_ready_for_repair(&verification.summary)?;
            return self.stop(
                AutonomousSessionStateV1::Queued,
                "verification_or_diff_incomplete",
            );
        }
        if self.mark_task_completed()? {
            return self.stop(
                AutonomousSessionStateV1::Queued,
                "task_verified_next_task_queued",
            );
        }
        let final_review = match self.verifier.final_review(&verification, &diff) {
            Ok(review) => review,
            Err(_) => {
                return self.escalate("final_review_unavailable");
            }
        };
        if self
            .policy
            .contract()
            .verification_policy
            .require_final_review
            && final_review.trim().is_empty()
        {
            return self.stop(
                AutonomousSessionStateV1::WaitingForChatgpt,
                "final_review_missing",
            );
        }
        self.store
            .write_completion_artifacts(&self.session_id, &verification, &diff, &final_review)
            .map_err(RuntimeError::from)?;
        self.transition(
            AutonomousSessionStateV1::CompletedVerified,
            "completed_verified",
        )?;
        Ok(AutonomousControllerOutcomeV1 {
            state: AutonomousSessionStateV1::CompletedVerified,
            provider_events: batch.events.len(),
            verification: Some(verification),
            authoritative_diff: Some(diff),
            final_review: Some(final_review),
        })
    }

    fn transition(&self, state: AutonomousSessionStateV1, event: &str) -> Result<(), RuntimeError> {
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
        self.stop(AutonomousSessionStateV1::WaitingForChatgpt, reason)
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
        )
    }

    fn prepare_turn(&self, task_id: &str, provider_session_id: &str) -> Result<(), RuntimeError> {
        let mut snapshot = self
            .store
            .load_session(&self.session_id)
            .map_err(RuntimeError::from)?;
        snapshot.current_task_id = Some(task_id.into());
        snapshot.provider_thread_id = Some(provider_session_id.into());
        snapshot.provider_handle_id = None;
        snapshot.provider_event_cursor = 0;
        snapshot.retry_not_before_unix = None;
        snapshot.rate_limited_since_unix = None;
        snapshot.repair_attempts = self.repair_attempts;
        snapshot.provider_turn_count = snapshot.provider_turn_count.saturating_add(1);
        snapshot.state = AutonomousSessionStateV1::Running;
        self.store
            .save_session(&snapshot)
            .map_err(RuntimeError::from)
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
        self.store
            .append_event(
                &self.session_id,
                "provider_rate_limited",
                "provider capacity pause persisted",
            )
            .map_err(RuntimeError::from)?;
        Ok(AutonomousControllerOutcomeV1 {
            state: AutonomousSessionStateV1::RateLimited,
            provider_events,
            verification: None,
            authoritative_diff: None,
            final_review: None,
        })
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
        let task = queue
            .tasks
            .iter_mut()
            .find(|task| task.task_id == task_id)
            .ok_or_else(|| {
                RuntimeError::Validation("autonomous task is absent from queue".into())
            })?;
        task.state = super::autonomy_state::AutonomousQueueTaskStateV1::Ready;
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
        Ok(queue.next_ready_task().is_some())
    }
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

fn bounded_contract_text(value: &str, limit: usize) -> String {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    use crate::delegated::autonomous_contract::{
        AutonomousCommandProfileV1, AutonomousDevelopmentContractV1, AutonomousGitPolicyV1,
        AutonomousHardStopV1, AutonomousProviderPolicyV1, AutonomousRateLimitPolicyV1,
        AutonomousVerificationPolicyV1, AutonomyLeaseV1,
    };
    use crate::delegated::autonomy_state::{
        AutonomousQueueTaskStateV1, AutonomousQueueTaskV1, AutonomousQueueV1,
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
                primary_model: "fake-model".into(),
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

    #[tokio::test]
    async fn expired_lease_stops_before_provider_launch() {
        let mut controller = setup(VerificationStatusV1::Passed, "diff");
        assert_eq!(
            controller.run_once(100).await.expect("stop").state,
            AutonomousSessionStateV1::LeaseExpired
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
}
