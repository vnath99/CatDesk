//! Read-only, redacted autonomy status for the terminal UI.
//!
//! This module intentionally consumes durable CatDesk state and T-0055
//! accounting. It does not inspect browsers, processes, credentials, routes,
//! prompts, or protected wake state.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::autonomy_accounting::WorkTimeEntryV1;
use super::autonomy_state::{
    AutonomousProviderRouteV1, AutonomousReviewInboxRecordV1, AutonomousSessionSnapshotV1,
    AutonomousSessionStateV1, AutonomousStateStoreV1, AutonomousWakeModeV1, AutonomousWakePolicyV1,
};

const MAX_TEXT: usize = 160;
const REPORT_WINDOW_MILLIS: u128 = 31 * 24 * 60 * 60 * 1_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AutonomyOverallStateV1 {
    Working,
    Waiting,
    Attention,
    Idle,
    #[default]
    Unknown,
}

impl AutonomyOverallStateV1 {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Working => "WORKING",
            Self::Waiting => "WAITING",
            Self::Attention => "ATTENTION",
            Self::Idle => "IDLE",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AutonomyActorV1 {
    CodexProvider,
    QwenProvider,
    Catdesk,
    Verifier,
    WakeBridge,
    ChatgptWeb,
    Operator,
    None,
    #[default]
    Unknown,
}

impl AutonomyActorV1 {
    pub const fn label(self) -> &'static str {
        match self {
            Self::CodexProvider => "CODEX_PROVIDER",
            Self::QwenProvider => "QWEN_PROVIDER",
            Self::Catdesk => "CATDESK",
            Self::Verifier => "VERIFIER",
            Self::WakeBridge => "WAKE_BRIDGE",
            Self::ChatgptWeb => "CHATGPT_WEB",
            Self::Operator => "OPERATOR",
            Self::None => "NONE",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Positive in-process lifecycle facts. A false value means no proof; it is
/// never upgraded from a connector, browser, window, or generic process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LiveAutonomyEvidenceV1 {
    pub wake_bridge_dispatch_active: bool,
    pub verifier_active: bool,
    pub provider_turn_active: bool,
    pub catdesk_transition_active: bool,
    pub exact_target_chatgpt_generating: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutonomyTimingSnapshotV1 {
    pub wall_millis: u128,
    pub known_active_millis: u128,
    pub provider_active_millis: u128,
    pub verification_active_millis: u128,
    pub orchestration_active_millis: u128,
    pub known_waiting_millis: u128,
    pub chatgpt_web_status: String,
    pub unknown_millis: u128,
    pub evidence_completeness: String,
}

impl Default for AutonomyTimingSnapshotV1 {
    fn default() -> Self {
        Self {
            wall_millis: 0,
            known_active_millis: 0,
            provider_active_millis: 0,
            verification_active_millis: 0,
            orchestration_active_millis: 0,
            known_waiting_millis: 0,
            chatgpt_web_status: "UNKNOWN_NOT_OBSERVABLE".into(),
            unknown_millis: 0,
            evidence_completeness: "UNKNOWN".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutonomyObservabilitySnapshotV1 {
    pub overall: AutonomyOverallStateV1,
    pub actor: AutonomyActorV1,
    /// Durable provider-route classification; it is distinct from the
    /// positive-evidence actor and never promotes a route into active work.
    pub provider: String,
    pub project_id: String,
    pub task_id: String,
    pub session_id: String,
    pub session_state: String,
    pub model: String,
    pub reasoning: String,
    pub timing: AutonomyTimingSnapshotV1,
    pub last_step: String,
    pub next_action: String,
    pub wake: String,
    pub unread_review_count: usize,
    pub review_attention: String,
    pub transport: String,
    pub observed_at_unix_millis: u128,
    pub stale: bool,
}

impl AutonomyObservabilitySnapshotV1 {
    pub fn unknown(transport: &str, reason: &str) -> Self {
        Self {
            overall: AutonomyOverallStateV1::Unknown,
            actor: AutonomyActorV1::Unknown,
            provider: "UNKNOWN".into(),
            project_id: "--".into(),
            task_id: "--".into(),
            session_id: "--".into(),
            session_state: "UNKNOWN".into(),
            model: "--".into(),
            reasoning: "--".into(),
            timing: AutonomyTimingSnapshotV1::default(),
            last_step: bounded(reason),
            next_action: "refresh local autonomy state".into(),
            wake: "UNKNOWN".into(),
            unread_review_count: 0,
            review_attention: "UNKNOWN".into(),
            transport: bounded(transport),
            observed_at_unix_millis: now_millis(),
            stale: true,
        }
    }
}

/// Bounded local reader used by the TUI refresh task. It avoids MCP self-calls
/// and returns an UNKNOWN snapshot on any corrupt or unavailable authority.
pub fn read_snapshot(workspace: &Path, transport: &str) -> AutonomyObservabilitySnapshotV1 {
    let root = workspace.join(".catdesk").join("autonomy");
    if !root.is_dir() {
        return idle_snapshot(transport);
    }
    let Ok(store) = AutonomousStateStoreV1::open(&root) else {
        return AutonomyObservabilitySnapshotV1::unknown(transport, "autonomy store unavailable");
    };
    let Ok(sessions) = store.list_sessions() else {
        return AutonomyObservabilitySnapshotV1::unknown(transport, "autonomy state unreadable");
    };
    let Ok(policy) = store.load_wake_policy() else {
        return AutonomyObservabilitySnapshotV1::unknown(transport, "wake policy unreadable");
    };
    let Ok(reviews) = store.all_review_inbox() else {
        return AutonomyObservabilitySnapshotV1::unknown(transport, "review inbox unreadable");
    };
    let selected = match select_session_for_observability(&sessions, &reviews) {
        Ok(selected) => selected,
        Err(()) => {
            return AutonomyObservabilitySnapshotV1::unknown(
                transport,
                "actionable review session unresolved",
            );
        }
    };
    let Some((session, selected_review_pending)) = selected else {
        return idle_from_policy(
            transport,
            &policy,
            reviews.iter().filter(|review| review.unread).count(),
        );
    };
    let contract = match store.load_contract(&session.session_id) {
        Ok(contract) => contract,
        Err(_) => {
            return AutonomyObservabilitySnapshotV1::unknown(
                transport,
                "active contract unreadable",
            );
        }
    };
    let timing = store
        .execution_accounting_store()
        .ok()
        .and_then(|accounting| {
            let end = now_millis();
            accounting
                .work_time_report(
                    end.saturating_sub(REPORT_WINDOW_MILLIS),
                    end.max(1),
                    Some(&contract.project_id),
                    Some(&session.session_id),
                    session.current_task_id.as_deref(),
                    1,
                    end,
                )
                .ok()
                .and_then(|report| report.entries.into_iter().next())
        });
    let last_step = store
        .poll_events(
            &session.session_id,
            session.last_event_sequence.saturating_sub(1),
        )
        .ok()
        .and_then(|events| events.into_iter().last())
        .map(|event| bounded(&event.kind))
        .unwrap_or_else(|| "UNKNOWN".into());
    build_snapshot(
        session,
        &contract.project_id,
        contract.provider_policy.primary_model.as_str(),
        session
            .codex_routing_telemetry
            .as_ref()
            .and_then(|telemetry| telemetry.reasoning_effort.as_deref())
            .unwrap_or("UNKNOWN"),
        timing.as_ref(),
        &policy,
        &reviews,
        selected_review_pending,
        LiveAutonomyEvidenceV1 {
            // A durable provider handle is the controller's authoritative
            // ownership token; a bare RUNNING state is deliberately not
            // enough after reload/recovery.
            provider_turn_active: session.state == AutonomousSessionStateV1::Running
                && session.provider_handle_id.is_some(),
            // Verification is CatDesk-owned and its durable state is set at
            // the verifier lifecycle boundary.
            verifier_active: session.state == AutonomousSessionStateV1::Verifying,
            wake_bridge_dispatch_active: super::autonomy_runtime::wake_dispatch_active(
                workspace,
                &session.session_id,
            ),
            ..LiveAutonomyEvidenceV1::default()
        },
        transport,
    )
    .with_last_step(last_step)
}

impl AutonomyObservabilitySnapshotV1 {
    fn with_last_step(mut self, last_step: String) -> Self {
        self.last_step = last_step;
        self
    }
}

pub fn build_snapshot(
    session: &AutonomousSessionSnapshotV1,
    project_id: &str,
    model: &str,
    reasoning: &str,
    timing: Option<&WorkTimeEntryV1>,
    wake_policy: &AutonomousWakePolicyV1,
    reviews: &[AutonomousReviewInboxRecordV1],
    selected_review_pending: bool,
    live: LiveAutonomyEvidenceV1,
    transport: &str,
) -> AutonomyObservabilitySnapshotV1 {
    let unread = reviews.iter().filter(|review| review.unread).count();
    let operator_attention = operator_attention_state(&session.state);
    let review_waiting = selected_review_pending
        || (session.state == AutonomousSessionStateV1::WaitingForChatgpt
            && reviews.iter().any(|review| {
                review.unread
                    && review.session_id == session.session_id
                    && actionable_review(review)
            }));
    let (overall, actor) = if operator_attention {
        (AutonomyOverallStateV1::Attention, AutonomyActorV1::Operator)
    } else if live.wake_bridge_dispatch_active {
        (AutonomyOverallStateV1::Working, AutonomyActorV1::WakeBridge)
    } else if live.verifier_active {
        (AutonomyOverallStateV1::Working, AutonomyActorV1::Verifier)
    } else if live.provider_turn_active {
        (
            AutonomyOverallStateV1::Working,
            provider_actor(&session.provider_route),
        )
    } else if live.catdesk_transition_active {
        (AutonomyOverallStateV1::Working, AutonomyActorV1::Catdesk)
    } else if live.exact_target_chatgpt_generating {
        (AutonomyOverallStateV1::Working, AutonomyActorV1::ChatgptWeb)
    } else if review_waiting {
        (AutonomyOverallStateV1::Waiting, AutonomyActorV1::ChatgptWeb)
    } else if matches!(
        session.state,
        AutonomousSessionStateV1::RateLimited
            | AutonomousSessionStateV1::Paused
            | AutonomousSessionStateV1::Queued
            | AutonomousSessionStateV1::WaitingForChatgpt
    ) {
        (AutonomyOverallStateV1::Waiting, AutonomyActorV1::None)
    } else if session.state.is_terminal() || !session.active {
        (AutonomyOverallStateV1::Idle, AutonomyActorV1::None)
    } else {
        // Durable session state alone is not sufficient to claim a currently
        // working actor after reload or a failed refresh.
        (AutonomyOverallStateV1::Unknown, AutonomyActorV1::Unknown)
    };
    let timing = timing.map(timing_from_entry).unwrap_or_default();
    AutonomyObservabilitySnapshotV1 {
        overall,
        actor,
        provider: provider_label(&session.provider_route).into(),
        project_id: bounded(project_id),
        task_id: bounded(session.current_task_id.as_deref().unwrap_or("--")),
        session_id: bounded(&session.session_id),
        session_state: session_state_label(&session.state).into(),
        model: bounded(model),
        reasoning: bounded(reasoning),
        timing,
        last_step: "UNKNOWN".into(),
        next_action: next_action(session, operator_attention, review_waiting),
        wake: wake_label(wake_policy),
        unread_review_count: unread,
        review_attention: if operator_attention || review_waiting {
            "PENDING"
        } else {
            "NONE"
        }
        .into(),
        transport: bounded(transport),
        observed_at_unix_millis: now_millis(),
        stale: false,
    }
}

fn provider_actor(route: &AutonomousProviderRouteV1) -> AutonomyActorV1 {
    if *route == AutonomousProviderRouteV1::QwenFallbackActive {
        AutonomyActorV1::QwenProvider
    } else {
        AutonomyActorV1::CodexProvider
    }
}

fn provider_label(route: &AutonomousProviderRouteV1) -> &'static str {
    match route {
        AutonomousProviderRouteV1::CodexPreferred => "CODEX",
        AutonomousProviderRouteV1::CodexTransientRateLimited => "CODEX_RATE_LIMITED",
        AutonomousProviderRouteV1::CodexCreditsExhausted => "CODEX_CREDITS_EXHAUSTED",
        AutonomousProviderRouteV1::QwenFallbackActive => "QWEN",
        AutonomousProviderRouteV1::QwenUnavailable => "QWEN_UNAVAILABLE",
        AutonomousProviderRouteV1::WaitingForChatgpt => "CHATGPT_WEB",
    }
}

fn timing_from_entry(entry: &WorkTimeEntryV1) -> AutonomyTimingSnapshotV1 {
    AutonomyTimingSnapshotV1 {
        wall_millis: entry.wall_millis,
        known_active_millis: entry.total_known_active_millis,
        provider_active_millis: entry.provider_active_millis,
        verification_active_millis: entry.verification_review_active_millis,
        orchestration_active_millis: entry.orchestration_active_millis,
        known_waiting_millis: entry.known_waiting_millis,
        chatgpt_web_status: bounded(&entry.chatgpt_web_status),
        unknown_millis: entry.unobserved_idle_or_unknown_millis,
        evidence_completeness: bounded(&entry.evidence_completeness),
    }
}

fn idle_snapshot(transport: &str) -> AutonomyObservabilitySnapshotV1 {
    idle_from_policy(transport, &AutonomousWakePolicyV1::default(), 0)
}

fn idle_from_policy(
    transport: &str,
    policy: &AutonomousWakePolicyV1,
    unread_review_count: usize,
) -> AutonomyObservabilitySnapshotV1 {
    AutonomyObservabilitySnapshotV1 {
        overall: AutonomyOverallStateV1::Idle,
        actor: AutonomyActorV1::None,
        provider: "NONE".into(),
        project_id: "--".into(),
        task_id: "--".into(),
        session_id: "--".into(),
        session_state: "IDLE".into(),
        model: "--".into(),
        reasoning: "--".into(),
        timing: AutonomyTimingSnapshotV1::default(),
        last_step: "--".into(),
        next_action: "await approved work".into(),
        wake: wake_label(policy),
        unread_review_count,
        review_attention: if unread_review_count > 0 {
            "PENDING"
        } else {
            "NONE"
        }
        .into(),
        transport: bounded(transport),
        observed_at_unix_millis: now_millis(),
        stale: false,
    }
}

fn next_action(
    session: &AutonomousSessionSnapshotV1,
    attention: bool,
    review_waiting: bool,
) -> String {
    if attention {
        return "operator review or input required".into();
    }
    if review_waiting {
        return "await ChatGPT review".into();
    }
    match session.state {
        AutonomousSessionStateV1::RateLimited => "wait for provider reset".into(),
        AutonomousSessionStateV1::Paused => "resume after policy approval".into(),
        AutonomousSessionStateV1::Queued => "await controller dispatch".into(),
        AutonomousSessionStateV1::WaitingForChatgpt => "await ChatGPT review".into(),
        AutonomousSessionStateV1::Verifying => "complete independent verification".into(),
        AutonomousSessionStateV1::Running => "await authoritative provider progress".into(),
        _ => "refresh authoritative state".into(),
    }
}

fn operator_attention_state(state: &AutonomousSessionStateV1) -> bool {
    matches!(
        state,
        AutonomousSessionStateV1::WaitingForUser | AutonomousSessionStateV1::Blocked
    )
}

fn actionable_review(review: &AutonomousReviewInboxRecordV1) -> bool {
    review.next_action == "independent_final_review"
}

fn newest_actionable_unread_review(
    reviews: &[AutonomousReviewInboxRecordV1],
) -> Option<&AutonomousReviewInboxRecordV1> {
    reviews
        .iter()
        .filter(|review| review.unread && actionable_review(review))
        .max_by(|left, right| {
            left.created_at_unix
                .cmp(&right.created_at_unix)
                .then(left.record_id.cmp(&right.record_id))
        })
}

fn select_session_for_observability<'a>(
    sessions: &'a [AutonomousSessionSnapshotV1],
    reviews: &'a [AutonomousReviewInboxRecordV1],
) -> Result<Option<(&'a AutonomousSessionSnapshotV1, bool)>, ()> {
    if let Some(session) = sessions
        .iter()
        .rev()
        .find(|session| session.active && operator_attention_state(&session.state))
    {
        return Ok(Some((session, false)));
    }
    if let Some(session) = sessions
        .iter()
        .rev()
        .find(|session| session.active && !session.state.is_terminal())
    {
        return Ok(Some((session, false)));
    }
    let Some(review) = newest_actionable_unread_review(reviews) else {
        return Ok(sessions
            .iter()
            .rev()
            .find(|session| session.active)
            .map(|session| (session, false)));
    };
    sessions
        .iter()
        .find(|session| session.session_id == review.session_id)
        .map(|session| Some((session, true)))
        .ok_or(())
}

fn wake_label(policy: &AutonomousWakePolicyV1) -> String {
    match policy.mode {
        AutonomousWakeModeV1::ManualOff => "OFF".into(),
        AutonomousWakeModeV1::Indefinite => "INDEFINITE".into(),
        AutonomousWakeModeV1::ThroughTask => format!(
            "THROUGH {}",
            bounded(policy.terminal_task_id.as_deref().unwrap_or("UNKNOWN"))
        ),
        AutonomousWakeModeV1::UntilProviderExhausted => "UNTIL_PROVIDER_EXHAUSTED".into(),
    }
}

fn session_state_label(state: &AutonomousSessionStateV1) -> &'static str {
    match state {
        AutonomousSessionStateV1::Draft => "DRAFT",
        AutonomousSessionStateV1::Queued => "QUEUED",
        AutonomousSessionStateV1::Running => "RUNNING",
        AutonomousSessionStateV1::Verifying => "VERIFYING",
        AutonomousSessionStateV1::WaitingForChatgpt => "WAITING_FOR_CHATGPT",
        AutonomousSessionStateV1::WaitingForUser => "WAITING_FOR_USER",
        AutonomousSessionStateV1::RateLimited => "RATE_LIMITED",
        AutonomousSessionStateV1::Paused => "PAUSED",
        AutonomousSessionStateV1::CompletedVerified => "COMPLETED_VERIFIED",
        AutonomousSessionStateV1::Blocked => "BLOCKED",
        AutonomousSessionStateV1::Failed => "FAILED",
        AutonomousSessionStateV1::Cancelled => "CANCELLED",
        AutonomousSessionStateV1::LeaseExpired => "LEASE_EXPIRED",
        AutonomousSessionStateV1::CreditBudgetExhausted => "CREDIT_BUDGET_EXHAUSTED",
        AutonomousSessionStateV1::RecoveringAfterRestart => "RECOVERING_AFTER_RESTART",
    }
}

fn bounded(value: &str) -> String {
    value.chars().take(MAX_TEXT).collect()
}
fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn session(state: AutonomousSessionStateV1) -> AutonomousSessionSnapshotV1 {
        AutonomousSessionSnapshotV1 {
            schema_version: 1,
            session_id: "session".into(),
            state,
            current_task_id: Some("task".into()),
            provider_thread_id: None,
            expected_codex_thread_id: None,
            provider_route: Default::default(),
            provider_handle_id: None,
            provider_event_cursor: 0,
            repair_attempts: 0,
            last_verification_summary: None,
            provider_turn_count: 0,
            retry_not_before_unix: None,
            rate_limited_since_unix: None,
            codex_eligible_after_unix: None,
            codex_routing_telemetry: None,
            codex_continuity: None,
            cancellation_requested: false,
            approved_contract_hash: None,
            consumed_idempotency_keys: BTreeMap::new(),
            github_publication_journal: Default::default(),
            last_event_sequence: 0,
            active: true,
        }
    }
    fn policy(mode: AutonomousWakeModeV1) -> AutonomousWakePolicyV1 {
        AutonomousWakePolicyV1 {
            schema_version: 1,
            generation: 0,
            mode,
            terminal_task_id: Some("task".into()),
            set_at_unix: 1,
            stopped_reason: None,
        }
    }
    fn review(session_id: &str, unread: bool) -> AutonomousReviewInboxRecordV1 {
        AutonomousReviewInboxRecordV1 {
            schema_version: 1,
            record_id: format!("review-{session_id}"),
            project_id: "project".into(),
            session_id: session_id.into(),
            state: AutonomousSessionStateV1::CompletedVerified,
            next_action: "independent_final_review".into(),
            reference: "bounded-review-reference".into(),
            created_at_unix: 1,
            unread,
        }
    }
    #[test]
    fn operator_attention_has_highest_precedence() {
        let result = build_snapshot(
            &session(AutonomousSessionStateV1::WaitingForUser),
            "project",
            "gpt-5.6-terra",
            "high",
            None,
            &policy(AutonomousWakeModeV1::Indefinite),
            &[],
            false,
            LiveAutonomyEvidenceV1 {
                provider_turn_active: true,
                ..Default::default()
            },
            "READY",
        );
        assert_eq!(result.overall, AutonomyOverallStateV1::Attention);
        assert_eq!(result.actor, AutonomyActorV1::Operator);
    }
    #[test]
    fn live_actor_precedence_is_positive_evidence_only() {
        let base = session(AutonomousSessionStateV1::Running);
        let result = build_snapshot(
            &base,
            "project",
            "model",
            "high",
            None,
            &policy(AutonomousWakeModeV1::Indefinite),
            &[],
            false,
            LiveAutonomyEvidenceV1 {
                wake_bridge_dispatch_active: true,
                verifier_active: true,
                provider_turn_active: true,
                catdesk_transition_active: true,
                exact_target_chatgpt_generating: true,
            },
            "READY",
        );
        assert_eq!(result.actor, AutonomyActorV1::WakeBridge);
        let unknown = build_snapshot(
            &base,
            "project",
            "model",
            "high",
            None,
            &policy(AutonomousWakeModeV1::Indefinite),
            &[],
            false,
            LiveAutonomyEvidenceV1::default(),
            "READY",
        );
        assert_eq!(unknown.overall, AutonomyOverallStateV1::Unknown);
    }

    #[test]
    fn qwen_provider_route_is_rendered_without_claiming_activity_from_route_alone() {
        let mut qwen = session(AutonomousSessionStateV1::Running);
        qwen.provider_route = AutonomousProviderRouteV1::QwenFallbackActive;
        let active = build_snapshot(
            &qwen,
            "project",
            "qwen3.8:27b",
            "high",
            None,
            &policy(AutonomousWakeModeV1::Indefinite),
            &[],
            false,
            LiveAutonomyEvidenceV1 {
                provider_turn_active: true,
                ..Default::default()
            },
            "READY",
        );
        assert_eq!(active.actor, AutonomyActorV1::QwenProvider);
        assert_eq!(active.provider, "QWEN");

        let unproven = build_snapshot(
            &qwen,
            "project",
            "qwen3.8:27b",
            "high",
            None,
            &policy(AutonomousWakeModeV1::Indefinite),
            &[],
            false,
            LiveAutonomyEvidenceV1::default(),
            "READY",
        );
        assert_eq!(unproven.overall, AutonomyOverallStateV1::Unknown);
        assert_eq!(unproven.provider, "QWEN");
    }
    #[test]
    fn waiting_and_wake_labels_are_exact() {
        for mode in [
            AutonomousWakeModeV1::ManualOff,
            AutonomousWakeModeV1::Indefinite,
            AutonomousWakeModeV1::ThroughTask,
            AutonomousWakeModeV1::UntilProviderExhausted,
        ] {
            let result = build_snapshot(
                &session(AutonomousSessionStateV1::RateLimited),
                "project",
                "model",
                "high",
                None,
                &policy(mode),
                &[],
                false,
                LiveAutonomyEvidenceV1::default(),
                "READY",
            );
            assert_eq!(result.overall, AutonomyOverallStateV1::Waiting);
            assert!(!result.wake.is_empty());
        }
    }

    #[test]
    fn unread_review_is_waiting_not_operator_attention_or_chatgpt_working() {
        let mut completed = session(AutonomousSessionStateV1::CompletedVerified);
        completed.active = false;
        let pending_review = review("session", true);
        let result = build_snapshot(
            &completed,
            "project",
            "gpt-5.6-terra",
            "high",
            None,
            &policy(AutonomousWakeModeV1::Indefinite),
            &[pending_review],
            true,
            LiveAutonomyEvidenceV1::default(),
            "READY",
        );
        assert_eq!(result.overall, AutonomyOverallStateV1::Waiting);
        assert_eq!(result.actor, AutonomyActorV1::ChatgptWeb);
        assert_eq!(result.session_id, "session");
        assert_eq!(result.next_action, "await ChatGPT review");
    }

    #[test]
    fn waiting_for_chatgpt_review_is_not_operator_attention() {
        let review = review("session", true);
        let result = build_snapshot(
            &session(AutonomousSessionStateV1::WaitingForChatgpt),
            "project",
            "model",
            "high",
            None,
            &policy(AutonomousWakeModeV1::Indefinite),
            &[review],
            true,
            LiveAutonomyEvidenceV1::default(),
            "READY",
        );
        assert_eq!(result.overall, AutonomyOverallStateV1::Waiting);
        assert_ne!(result.actor, AutonomyActorV1::Operator);
    }

    #[test]
    fn operator_attention_still_wins_and_acknowledged_review_allows_idle() {
        let pending_review = review("session", true);
        let result = build_snapshot(
            &session(AutonomousSessionStateV1::WaitingForUser),
            "project",
            "model",
            "high",
            None,
            &policy(AutonomousWakeModeV1::Indefinite),
            &[pending_review],
            true,
            LiveAutonomyEvidenceV1::default(),
            "READY",
        );
        assert_eq!(result.overall, AutonomyOverallStateV1::Attention);
        assert_eq!(result.actor, AutonomyActorV1::Operator);

        let mut completed = session(AutonomousSessionStateV1::CompletedVerified);
        completed.active = false;
        let acknowledged = review("session", false);
        let idle = build_snapshot(
            &completed,
            "project",
            "model",
            "high",
            None,
            &policy(AutonomousWakeModeV1::Indefinite),
            &[acknowledged],
            false,
            LiveAutonomyEvidenceV1::default(),
            "READY",
        );
        assert_eq!(idle.overall, AutonomyOverallStateV1::Idle);
    }

    #[test]
    fn active_session_beats_old_review_and_unresolvable_review_is_unknown() {
        let mut active = session(AutonomousSessionStateV1::Queued);
        active.session_id = "new-session".into();
        let mut old = session(AutonomousSessionStateV1::CompletedVerified);
        old.session_id = "old-session".into();
        old.active = false;
        let old_review = review("old-session", true);
        let sessions = [old, active];
        let reviews = [old_review];
        let selected = select_session_for_observability(&sessions, &reviews)
            .expect("selection")
            .expect("active session");
        assert_eq!(selected.0.session_id, "new-session");
        assert!(!selected.1);

        let unresolved = review("missing-session", true);
        assert!(select_session_for_observability(&[], &[unresolved]).is_err());
    }
}
