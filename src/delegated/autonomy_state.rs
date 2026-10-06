//! Durable, provider-neutral autonomy session state.
//!
//! T-0028D deliberately persists no provider credentials or executable paths.
//! A restart moves in-flight work to `RECOVERING_AFTER_RESTART`; a later
//! controller must validate continuity and policy before it takes any action.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::autonomous_contract::AutonomousDevelopmentContractV1;
use super::autonomy_accounting::ExecutionAccountingStoreV1;
use super::codex_app_server::{CodexRoutingTelemetryV1, validate_telemetry};
use super::coordinator::VerificationSummaryV1;
use super::github_publication::GithubPublicationJournalV1;
use super::runtime::RuntimeError;
use crate::reviewed_source_snapshot::ReviewedSourceSnapshotExpectedV1;

pub const AUTONOMY_STATE_SCHEMA_VERSION: u32 = 1;
pub const AUTONOMY_WAKE_POLICY_SCHEMA_VERSION: u32 = 1;
pub const AUTONOMY_REVIEW_INBOX_RECORD_LIMIT: usize = 4096;
// The live inbox is a bounded notification/authorization window, not the sole audit log.
// Retire only oldest acknowledged records; unread actionable work is never compacted away.
const AUTONOMY_REVIEW_INBOX_COMPACT_TRIGGER: usize = 3584;
const AUTONOMY_REVIEW_INBOX_COMPACT_TARGET: usize = 3072;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutonomousWakeModeV1 {
    ManualOff,
    #[default]
    Indefinite,
    ThroughTask,
    UntilProviderExhausted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousWakePolicyV1 {
    pub schema_version: u32,
    pub generation: u64,
    pub mode: AutonomousWakeModeV1,
    pub terminal_task_id: Option<String>,
    pub set_at_unix: u64,
    pub stopped_reason: Option<String>,
}

impl Default for AutonomousWakePolicyV1 {
    fn default() -> Self {
        Self {
            schema_version: AUTONOMY_WAKE_POLICY_SCHEMA_VERSION,
            generation: 0,
            mode: AutonomousWakeModeV1::Indefinite,
            terminal_task_id: None,
            set_at_unix: 0,
            stopped_reason: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutonomousSessionStateV1 {
    Draft,
    Queued,
    Running,
    Verifying,
    WaitingForChatgpt,
    WaitingForUser,
    RateLimited,
    Paused,
    CompletedVerified,
    Blocked,
    Failed,
    Cancelled,
    LeaseExpired,
    CreditBudgetExhausted,
    RecoveringAfterRestart,
}

/// Durable worker selection separate from the logical session lifecycle.
/// Keeping this independently persisted makes a Codex->Qwen handoff safe to
/// recover without treating usage exhaustion as task completion.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutonomousProviderRouteV1 {
    #[default]
    CodexPreferred,
    CodexTransientRateLimited,
    CodexCreditsExhausted,
    QwenFallbackActive,
    QwenUnavailable,
    WaitingForChatgpt,
}

impl AutonomousSessionStateV1 {
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::CompletedVerified
                | Self::Blocked
                | Self::Failed
                | Self::Cancelled
                | Self::LeaseExpired
                | Self::CreditBudgetExhausted
        )
    }

    const fn needs_restart_recovery(&self) -> bool {
        matches!(self, Self::Running | Self::Verifying)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutonomousQueueTaskStateV1 {
    Planned,
    Ready,
    /// Explicit durable lifecycle label for a provider-owned turn.
    WorkerRunning,
    Running,
    Verifying,
    Repairing,
    WaitingForChatgpt,
    WaitingForUser,
    RateLimited,
    Paused,
    CompletedVerified,
    Failed,
    Cancelled,
}

impl AutonomousQueueTaskStateV1 {
    const fn satisfies_dependencies(&self) -> bool {
        matches!(self, Self::CompletedVerified)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousSessionSnapshotV1 {
    pub schema_version: u32,
    pub session_id: String,
    pub state: AutonomousSessionStateV1,
    pub current_task_id: Option<String>,
    pub provider_thread_id: Option<String>,
    /// The exact canonical Codex thread expected to report for the active
    /// provider turn. This is separate from task selection because a newly
    /// selected DAG task may continue the project's one Codex thread.
    #[serde(default)]
    pub expected_codex_thread_id: Option<String>,
    #[serde(default)]
    pub provider_route: AutonomousProviderRouteV1,
    #[serde(default)]
    pub provider_handle_id: Option<String>,
    #[serde(default)]
    pub provider_event_cursor: u64,
    #[serde(default)]
    pub repair_attempts: u32,
    #[serde(default)]
    pub last_verification_summary: Option<String>,
    #[serde(default)]
    pub provider_turn_count: u32,
    #[serde(default)]
    pub retry_not_before_unix: Option<u64>,
    #[serde(default)]
    pub rate_limited_since_unix: Option<u64>,
    /// A confirmed app-server reset time. While a healthy Qwen task is in
    /// flight the controller will not interrupt it or reprobe Codex early.
    #[serde(default)]
    pub codex_eligible_after_unix: Option<u64>,
    #[serde(default)]
    pub codex_routing_telemetry: Option<CodexRoutingTelemetryV1>,
    /// Host-only, bounded proof for the two harmless acceptance turns. It is
    /// intentionally separate from provider-turn budgeting for substantive
    /// autonomous work.
    #[serde(default)]
    pub codex_continuity: Option<CodexContinuityEvidenceV1>,
    #[serde(default)]
    pub cancellation_requested: bool,
    #[serde(default)]
    pub approved_contract_hash: Option<String>,
    #[serde(default)]
    pub consumed_idempotency_keys: BTreeMap<String, String>,
    /// A private, durable journal for the future remote-publication executor.
    /// Current contracts retain no publication executor; persisting the
    /// journal here makes replay/uncertainty fail closed across restart.
    #[serde(default)]
    pub(crate) github_publication_journal: GithubPublicationJournalV1,
    pub last_event_sequence: u64,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexContinuityEvidenceV1 {
    pub thread_id: String,
    pub first_turn_id: String,
    pub first_completed: bool,
    pub second_turn_id: Option<String>,
    pub second_completed: bool,
    pub observed_at_unix: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousQueueTaskV1 {
    pub task_id: String,
    pub priority: u32,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub state: AutonomousQueueTaskStateV1,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousQueueV1 {
    pub schema_version: u32,
    #[serde(default)]
    pub tasks: Vec<AutonomousQueueTaskV1>,
}

/// Stable planner metadata is deliberately separate from runtime queue state.
/// This allows a planner to persist scope/acceptance provenance without a
/// worker being able to rewrite its approved queue record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousPlannedTaskV1 {
    pub task_id: String,
    pub depends_on: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub allowed_paths: Vec<String>,
    pub verification_profile: String,
    pub preferred_worker_policy: String,
    pub escalation_conditions: Vec<String>,
    #[serde(default)]
    pub completion_artifact_ids: Vec<String>,
    #[serde(default)]
    pub provider_provenance: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousPlanQueueV1 {
    pub schema_version: u32,
    pub tasks: Vec<AutonomousPlannedTaskV1>,
}

/// Verifies that the durable queue and task-plan are an exact materialization
/// of an approved non-empty task graph.  Runtime task state is deliberately
/// not compared here: it is the controller-owned progress record.  All task
/// instructions and execution constraints are compared exactly so a durable
/// plan cannot become a second, mutable approval source after contract
/// approval.
pub fn validate_exact_task_graph_materialization(
    contract: &AutonomousDevelopmentContractV1,
    queue: &AutonomousQueueV1,
    plan: &AutonomousPlanQueueV1,
) -> Result<(), RuntimeError> {
    contract
        .validate()
        .map_err(|_| RuntimeError::Validation("approved task graph contract is invalid".into()))?;
    if contract.task_graph.is_empty() {
        return Ok(());
    }
    queue.validate()?;
    validate_plan_queue(plan).map_err(RuntimeError::from)?;

    let graph = &contract.task_graph;
    if queue.tasks.len() != graph.len() || plan.tasks.len() != graph.len() {
        return Err(RuntimeError::Validation(
            "approved graph materialization task count is not exact".into(),
        ));
    }
    let graph_ids = graph
        .iter()
        .map(|task| task.task_id.as_str())
        .collect::<BTreeSet<_>>();
    let queue_ids = queue
        .tasks
        .iter()
        .map(|task| task.task_id.as_str())
        .collect::<BTreeSet<_>>();
    let plan_ids = plan
        .tasks
        .iter()
        .map(|task| task.task_id.as_str())
        .collect::<BTreeSet<_>>();
    if graph_ids.len() != graph.len() || graph_ids != queue_ids || graph_ids != plan_ids {
        return Err(RuntimeError::Validation(
            "approved graph materialization task set is not exact".into(),
        ));
    }

    let allowed_paths = contract
        .allowed_paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    for approved in graph {
        let queue_task = queue
            .tasks
            .iter()
            .find(|task| task.task_id == approved.task_id)
            .ok_or_else(|| {
                RuntimeError::Validation("approved graph queue task is missing".into())
            })?;
        let plan_task = plan
            .tasks
            .iter()
            .find(|task| task.task_id == approved.task_id)
            .ok_or_else(|| {
                RuntimeError::Validation("approved graph plan task is missing".into())
            })?;
        if queue_task.priority != approved.priority
            || queue_task.depends_on != approved.depends_on
            || plan_task.depends_on != approved.depends_on
            || plan_task.acceptance_criteria != approved.acceptance_criteria
            || plan_task.completion_artifact_ids != approved.completion_artifact_ids
            || plan_task.allowed_paths != allowed_paths
            || plan_task.verification_profile != contract.verification_policy.profile
        {
            return Err(RuntimeError::Validation(
                "approved graph materialization does not match its contract".into(),
            ));
        }
    }
    Ok(())
}

impl AutonomousQueueV1 {
    pub fn new(tasks: Vec<AutonomousQueueTaskV1>) -> Result<Self, RuntimeError> {
        let queue = Self {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            tasks,
        };
        queue.validate()?;
        Ok(queue)
    }

    pub fn validate(&self) -> Result<(), RuntimeError> {
        if self.schema_version != AUTONOMY_STATE_SCHEMA_VERSION {
            return Err(RuntimeError::Validation(
                "unsupported autonomous queue schema version".into(),
            ));
        }
        let ids = self
            .tasks
            .iter()
            .map(|task| task.task_id.as_str())
            .collect::<BTreeSet<_>>();
        if ids.len() != self.tasks.len() {
            return Err(RuntimeError::Validation(
                "autonomous queue task ids must be unique".into(),
            ));
        }
        for task in &self.tasks {
            validate_slug(&task.task_id, "autonomous queue task id")?;
            if task
                .depends_on
                .iter()
                .any(|dependency| dependency == &task.task_id)
            {
                return Err(RuntimeError::Validation(
                    "autonomous queue task cannot depend on itself".into(),
                ));
            }
            if task.depends_on.iter().collect::<BTreeSet<_>>().len() != task.depends_on.len() {
                return Err(RuntimeError::Validation(
                    "autonomous queue dependencies must be unique".into(),
                ));
            }
            for dependency in &task.depends_on {
                validate_slug(dependency, "autonomous queue dependency")?;
                if !ids.contains(dependency.as_str()) {
                    return Err(RuntimeError::Validation(
                        "autonomous queue dependency does not exist".into(),
                    ));
                }
            }
        }
        let mut remaining = self
            .tasks
            .iter()
            .map(|task| {
                (
                    task.task_id.as_str(),
                    task.depends_on
                        .iter()
                        .map(String::as_str)
                        .collect::<BTreeSet<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut complete = BTreeSet::new();
        loop {
            let ready = remaining
                .iter()
                .filter(|(_, dependencies)| {
                    dependencies
                        .iter()
                        .all(|dependency| complete.contains(dependency))
                })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            if ready.is_empty() {
                break;
            }
            for id in ready {
                remaining.remove(id);
                complete.insert(id);
            }
        }
        if !remaining.is_empty() {
            return Err(RuntimeError::Validation(
                "autonomous queue dependencies must be acyclic".into(),
            ));
        }
        Ok(())
    }

    pub fn next_ready_task(&self) -> Option<&AutonomousQueueTaskV1> {
        self.tasks
            .iter()
            .filter(|task| task.state == AutonomousQueueTaskStateV1::Ready)
            .filter(|task| {
                task.depends_on.iter().all(|dependency| {
                    self.tasks
                        .iter()
                        .find(|candidate| &candidate.task_id == dependency)
                        .is_some_and(|candidate| candidate.state.satisfies_dependencies())
                })
            })
            .min_by_key(|task| (Reverse(task.priority), &task.task_id))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousEventV1 {
    pub schema_version: u32,
    pub session_id: String,
    pub sequence: u64,
    pub kind: String,
    pub summary: String,
}

/// Bounded, operator-visible decision request. It contains no provider
/// transcript, credential, endpoint, or unbounded diagnostics.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousEscalationPacketV1 {
    pub schema_version: u32,
    pub session_id: String,
    pub escalation_id: String,
    pub state: AutonomousSessionStateV1,
    pub reason: String,
    pub repair_attempts: u32,
}

/// A bounded answer to a persisted escalation. The controller consumes the
/// answer exactly once before it resumes the existing provider session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousPlannerReplyV1 {
    pub schema_version: u32,
    pub session_id: String,
    pub escalation_id: String,
    pub decision_hash: String,
    pub decision: String,
    pub constraints: Vec<String>,
    pub consumed: bool,
}

/// CatDesk-owned evidence required before verified completion. It is written
/// before the terminal transition and exposed only through bounded readers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousCompletionArtifactsV1 {
    pub schema_version: u32,
    pub session_id: String,
    pub verification: VerificationSummaryV1,
    pub authoritative_diff: String,
    pub final_review: String,
}

/// Durable evidence that provider execution and independent verification have
/// already completed. It is deliberately a finalization checkpoint, not a
/// provider-launch, mutation, or lease-extension authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousPassedVerificationCheckpointV1 {
    pub schema_version: u32,
    pub session_id: String,
    pub project_id: String,
    pub approved_contract_hash: String,
    pub logical_task_id: String,
    pub provider_id: String,
    pub provider_turn_count: u32,
    pub verification_profile: String,
    pub verification: VerificationSummaryV1,
    pub authoritative_diff: String,
    pub authoritative_diff_sha256: String,
    pub attribution_digest: String,
    /// The immutable task-output observations which were attributed before
    /// this checkpoint was committed.  Recovery may only compare current
    /// objects to these observations; it must never create a new expectation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_source_expectation: Option<ReviewedSourceSnapshotExpectedV1>,
}

/// Immutable first-launch evidence for approval-bound task outputs. The
/// record contains only approved relative IDs and SHA-256 values, never file
/// contents or provider material.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousTaskOutputBaselineV1 {
    pub schema_version: u32,
    pub session_id: String,
    pub task_id: String,
    pub approved_contract_hash: String,
    pub completion_artifact_ids: Vec<String>,
    pub observations: Vec<AutonomousTaskOutputObservationV1>,
}

/// Separate durable evidence that a task-output baseline was required before
/// a provider launch.  It intentionally contains no output observations: its
/// only purpose is to make loss or corruption of the companion immutable
/// baseline non-runnable rather than recapturable after a turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousTaskOutputBaselineAuthorityV1 {
    pub schema_version: u32,
    pub session_id: String,
    pub task_id: String,
    pub approved_contract_hash: String,
    pub completion_artifact_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousTaskOutputObservationV1 {
    pub artifact_id: String,
    /// `ABSENT` or `PRESENT_SHA256`; the latter is always an exact SHA-256.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutonomousTaskOutputBaselineStoreV1 {
    schema_version: u32,
    session_id: String,
    baselines: Vec<AutonomousTaskOutputBaselineV1>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutonomousTaskOutputBaselineAuthorityStoreV1 {
    schema_version: u32,
    session_id: String,
    authorities: Vec<AutonomousTaskOutputBaselineAuthorityV1>,
}

/// A compact, durable notification for the existing polling bridge. CatDesk
/// deliberately stores this locally instead of claiming it can wake ChatGPT
/// Web unsolicited.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousReviewInboxRecordV1 {
    pub schema_version: u32,
    pub record_id: String,
    pub project_id: String,
    pub session_id: String,
    pub state: AutonomousSessionStateV1,
    pub next_action: String,
    pub reference: String,
    pub created_at_unix: u64,
    pub unread: bool,
}

/// Explicit, non-secret replacement evidence for audit reporting.  Session
/// chronology and similar task names are never treated as supersession.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousSessionSupersessionV1 {
    pub schema_version: u32,
    pub superseded_session_id: String,
    pub superseding_session_id: String,
    pub recorded_at_unix: u64,
}

/// A versioned, bounded and deliberately non-secret continuation record.
/// It is written before local fallback begins and never contains provider
/// auth state, endpoint credentials, raw environment values, or transcripts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousProviderHandoffV1 {
    pub schema_version: u32,
    pub session_id: String,
    pub task_id: String,
    pub from_provider_id: String,
    pub to_provider_id: String,
    pub reason: String,
    pub created_at_unix: u64,
    pub objective: String,
    pub ordered_steps: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub allowed_paths: Vec<String>,
    pub forbidden_paths: Vec<String>,
    pub base_branch: String,
    pub feature_branch: String,
    pub base_commit: String,
    pub expected_origin: String,
    pub codex_thread_id: Option<String>,
    pub last_verification_summary: Option<String>,
    pub completed_task_ids: Vec<String>,
    pub pending_task_id: String,
    pub remaining_provider_turns: u32,
    pub remaining_repair_cycles: u32,
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub git_status_porcelain: Vec<String>,
    pub authoritative_diff_hash: Option<String>,
    #[serde(default)]
    pub completed_tool_calls: Vec<AutonomousToolCallProvenanceV1>,
    #[serde(default)]
    pub pending_tool_calls: Vec<AutonomousToolCallProvenanceV1>,
}

/// Redacted replay evidence copied from the CatDesk-owned journal, never a
/// raw provider transcript or tool argument payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomousToolCallProvenanceV1 {
    pub tool_call_id: String,
    pub tool_name: String,
    pub mutation_kind: String,
    pub status: String,
    pub request_hash: String,
    pub result_hash: Option<String>,
    pub outcome_summary: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AutonomyStateError {
    Io(String),
    Serde(String),
    Validation(String),
    SessionExists(String),
    SessionNotFound(String),
    LockHeld(String),
    LockOwnership(String),
}

impl From<AutonomyStateError> for RuntimeError {
    fn from(value: AutonomyStateError) -> Self {
        RuntimeError::Provider(format!("autonomy state error: {value:?}"))
    }
}

#[derive(Clone, Debug)]
pub struct AutonomousStateStoreV1 {
    root: PathBuf,
}

#[derive(Debug)]
pub struct AutonomousSessionLockV1 {
    path: PathBuf,
    owner_id: String,
    released: bool,
}

impl AutonomousStateStoreV1 {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, AutonomyStateError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(io_error)?;
        Ok(Self { root })
    }

    pub fn create_session(
        &self,
        session_id: &str,
        queue: AutonomousQueueV1,
    ) -> Result<AutonomousSessionSnapshotV1, AutonomyStateError> {
        validate_slug(session_id, "autonomous session id").map_err(validation_error)?;
        queue.validate().map_err(runtime_validation_error)?;
        let session_dir = self.session_dir(session_id)?;
        if session_dir.join("state.json").exists() {
            return Err(AutonomyStateError::SessionExists(session_id.into()));
        }
        fs::create_dir_all(session_dir.join("locks")).map_err(io_error)?;
        fs::create_dir_all(session_dir.join("provider")).map_err(io_error)?;
        fs::create_dir_all(session_dir.join("verification")).map_err(io_error)?;
        fs::create_dir_all(session_dir.join("artifacts")).map_err(io_error)?;
        let snapshot = AutonomousSessionSnapshotV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            session_id: session_id.into(),
            state: AutonomousSessionStateV1::Draft,
            current_task_id: None,
            provider_thread_id: None,
            expected_codex_thread_id: None,
            provider_route: AutonomousProviderRouteV1::CodexPreferred,
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
            github_publication_journal: GithubPublicationJournalV1::default(),
            last_event_sequence: 0,
            active: true,
        };
        write_json_atomic(&session_dir.join("queue.json"), &queue)?;
        write_json_atomic(&session_dir.join("state.json"), &snapshot)?;
        Ok(snapshot)
    }

    /// Creation rollback is allowed only before approval and only for DRAFT.
    pub fn remove_unapproved_session(&self, session_id: &str) -> Result<(), AutonomyStateError> {
        let snapshot = self.load_session(session_id)?;
        if snapshot.state != AutonomousSessionStateV1::Draft
            || snapshot.approved_contract_hash.is_some()
        {
            return Err(AutonomyStateError::Validation(
                "only an unapproved draft session may be removed".into(),
            ));
        }
        fs::remove_dir_all(self.session_dir(session_id)?).map_err(io_error)
    }

    pub fn load_wake_policy(&self) -> Result<AutonomousWakePolicyV1, AutonomyStateError> {
        let path = self.root.join("wake-policy.json");
        if !path.exists() {
            return Ok(AutonomousWakePolicyV1::default());
        }
        let policy: AutonomousWakePolicyV1 = read_json(&path)?;
        validate_wake_policy(&policy)?;
        Ok(policy)
    }

    pub fn set_wake_policy(
        &self,
        expected_generation: u64,
        mode: AutonomousWakeModeV1,
        terminal_task_id: Option<&str>,
        set_at_unix: u64,
    ) -> Result<AutonomousWakePolicyV1, AutonomyStateError> {
        let current = self.load_wake_policy()?;
        if current.generation != expected_generation || set_at_unix == 0 {
            return Err(AutonomyStateError::Validation(
                "autonomous wake policy generation is stale or invalid".into(),
            ));
        }
        let terminal_task_id = terminal_task_id.map(str::to_owned);
        let policy = AutonomousWakePolicyV1 {
            schema_version: AUTONOMY_WAKE_POLICY_SCHEMA_VERSION,
            generation: current.generation.saturating_add(1),
            terminal_task_id,
            mode,
            set_at_unix,
            stopped_reason: None,
        };
        validate_wake_policy(&policy)?;
        write_json_atomic(&self.root.join("wake-policy.json"), &policy)?;
        Ok(policy)
    }

    pub fn stop_wake_policy(
        &self,
        expected_generation: u64,
        reason: &str,
        set_at_unix: u64,
    ) -> Result<AutonomousWakePolicyV1, AutonomyStateError> {
        validate_slug(reason, "autonomous wake stopped reason").map_err(validation_error)?;
        let current = self.load_wake_policy()?;
        if current.generation != expected_generation || set_at_unix == 0 {
            return Err(AutonomyStateError::Validation(
                "autonomous wake policy generation is stale or invalid".into(),
            ));
        }
        let policy = AutonomousWakePolicyV1 {
            schema_version: AUTONOMY_WAKE_POLICY_SCHEMA_VERSION,
            generation: current.generation.saturating_add(1),
            mode: AutonomousWakeModeV1::ManualOff,
            terminal_task_id: None,
            set_at_unix,
            stopped_reason: Some(reason.into()),
        };
        write_json_atomic(&self.root.join("wake-policy.json"), &policy)?;
        Ok(policy)
    }

    pub fn load_session(
        &self,
        session_id: &str,
    ) -> Result<AutonomousSessionSnapshotV1, AutonomyStateError> {
        let session_dir = self.session_dir(session_id)?;
        if !session_dir.join("state.json").is_file() {
            return Err(AutonomyStateError::SessionNotFound(session_id.into()));
        }
        let snapshot: AutonomousSessionSnapshotV1 = read_json(&session_dir.join("state.json"))?;
        validate_snapshot(&snapshot, session_id)?;
        Ok(snapshot)
    }

    pub fn save_session(
        &self,
        snapshot: &AutonomousSessionSnapshotV1,
    ) -> Result<(), AutonomyStateError> {
        validate_snapshot(snapshot, &snapshot.session_id)?;
        self.ensure_session_exists(&snapshot.session_id)?;
        write_json_atomic(
            &self.session_dir(&snapshot.session_id)?.join("state.json"),
            snapshot,
        )
    }

    pub fn load_queue(&self, session_id: &str) -> Result<AutonomousQueueV1, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        let queue: AutonomousQueueV1 =
            read_json(&self.session_dir(session_id)?.join("queue.json"))?;
        queue.validate().map_err(runtime_validation_error)?;
        Ok(queue)
    }

    pub fn save_contract(
        &self,
        session_id: &str,
        contract: &AutonomousDevelopmentContractV1,
    ) -> Result<(), AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        contract
            .validate()
            .map_err(|_| AutonomyStateError::Validation("autonomous contract is invalid".into()))?;
        write_json_atomic(
            &self.session_dir(session_id)?.join("contract.json"),
            contract,
        )
    }

    pub fn load_contract(
        &self,
        session_id: &str,
    ) -> Result<AutonomousDevelopmentContractV1, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        let contract: AutonomousDevelopmentContractV1 =
            read_json(&self.session_dir(session_id)?.join("contract.json"))?;
        contract
            .validate()
            .map_err(|_| AutonomyStateError::Validation("autonomous contract is invalid".into()))?;
        Ok(contract)
    }

    pub fn save_queue(
        &self,
        session_id: &str,
        queue: &AutonomousQueueV1,
    ) -> Result<(), AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        queue.validate().map_err(runtime_validation_error)?;
        write_json_atomic(&self.session_dir(session_id)?.join("queue.json"), queue)
    }

    pub fn write_plan_queue(
        &self,
        session_id: &str,
        plan: &AutonomousPlanQueueV1,
    ) -> Result<(), AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        validate_plan_queue(plan)?;
        let queue = self.load_queue(session_id)?;
        let queue_ids = queue
            .tasks
            .iter()
            .map(|task| &task.task_id)
            .collect::<BTreeSet<_>>();
        let plan_ids = plan
            .tasks
            .iter()
            .map(|task| &task.task_id)
            .collect::<BTreeSet<_>>();
        if queue_ids != plan_ids {
            return Err(AutonomyStateError::Validation(
                "planned task metadata must exactly cover the durable execution queue".into(),
            ));
        }
        write_json_atomic(&self.session_dir(session_id)?.join("plan.json"), plan)
    }

    pub fn load_plan_queue(
        &self,
        session_id: &str,
    ) -> Result<AutonomousPlanQueueV1, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        let plan: AutonomousPlanQueueV1 =
            read_json(&self.session_dir(session_id)?.join("plan.json"))?;
        validate_plan_queue(&plan)?;
        Ok(plan)
    }

    /// Planner-gate satisfaction is a tiny durable artifact rather than an
    /// inference from chat history. Keys are exact approved task IDs only.
    pub fn planner_gate_satisfied(
        &self,
        session_id: &str,
        task_id: &str,
    ) -> Result<bool, AutonomyStateError> {
        validate_slug(task_id, "planner gate task id").map_err(validation_error)?;
        let path = self
            .session_dir(session_id)?
            .join("artifacts")
            .join("planner-gates.json");
        let values: BTreeSet<String> = if path.exists() {
            read_json(&path)?
        } else {
            BTreeSet::new()
        };
        Ok(values.contains(task_id))
    }

    pub fn satisfy_planner_gate(
        &self,
        session_id: &str,
        task_id: &str,
    ) -> Result<(), AutonomyStateError> {
        validate_slug(task_id, "planner gate task id").map_err(validation_error)?;
        let path = self
            .session_dir(session_id)?
            .join("artifacts")
            .join("planner-gates.json");
        let mut values: BTreeSet<String> = if path.exists() {
            read_json(&path)?
        } else {
            BTreeSet::new()
        };
        values.insert(task_id.into());
        write_json_atomic(&path, &values)
    }

    pub fn record_codex_routing_telemetry(
        &self,
        session_id: &str,
        telemetry: CodexRoutingTelemetryV1,
        _conservative_cooldown_seconds: u64,
    ) -> Result<(), AutonomyStateError> {
        validate_telemetry(&telemetry).map_err(runtime_validation_error)?;
        let mut snapshot = self.load_session(session_id)?;
        // Only an independently provider-attested reset becomes an
        // eligibility boundary.  A missing value is deliberately sticky and
        // cannot be converted into a guessed cooldown by state persistence.
        snapshot.codex_eligible_after_unix = telemetry
            .reached_limit
            .then_some(telemetry.codex_eligible_after_unix)
            .flatten();
        snapshot.codex_routing_telemetry = Some(telemetry);
        self.save_session(&snapshot)
    }

    /// The accounting ledger is adjacent to (but independent from) session
    /// state so it remains readable after a task reaches a terminal state.
    pub fn execution_accounting_store(
        &self,
    ) -> Result<ExecutionAccountingStoreV1, AutonomyStateError> {
        ExecutionAccountingStoreV1::open(&self.root).map_err(|error| {
            AutonomyStateError::Validation(format!(
                "execution accounting store unavailable: {error:?}"
            ))
        })
    }

    pub fn append_event(
        &self,
        session_id: &str,
        kind: &str,
        summary: &str,
    ) -> Result<AutonomousEventV1, AutonomyStateError> {
        validate_slug(kind, "autonomous event kind").map_err(validation_error)?;
        let summary = bounded_summary(summary)?;
        let mut snapshot = self.load_session(session_id)?;
        let events_path = self.session_dir(session_id)?.join("events.jsonl");
        let events = read_jsonl_repair_tail::<AutonomousEventV1>(&events_path)?;
        let last_sequence = events.last().map(|event| event.sequence).unwrap_or(0);
        if snapshot.last_event_sequence > last_sequence {
            return Err(AutonomyStateError::Validation(
                "autonomous state sequence is ahead of its event journal".into(),
            ));
        }
        if snapshot.last_event_sequence != last_sequence {
            snapshot.last_event_sequence = last_sequence;
            write_json_atomic(&self.session_dir(session_id)?.join("state.json"), &snapshot)?;
        }
        let event = AutonomousEventV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            session_id: session_id.into(),
            sequence: last_sequence.saturating_add(1),
            kind: kind.into(),
            summary,
        };
        append_jsonl(&events_path, &event)?;
        snapshot.last_event_sequence = event.sequence;
        write_json_atomic(&self.session_dir(session_id)?.join("state.json"), &snapshot)?;
        Ok(event)
    }

    pub fn poll_events(
        &self,
        session_id: &str,
        after_sequence: u64,
    ) -> Result<Vec<AutonomousEventV1>, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        let events = read_jsonl_repair_tail(&self.session_dir(session_id)?.join("events.jsonl"))?;
        Ok(events
            .into_iter()
            .filter(|event: &AutonomousEventV1| event.sequence > after_sequence)
            .collect())
    }

    pub fn write_escalation(
        &self,
        session_id: &str,
        state: AutonomousSessionStateV1,
        reason: &str,
    ) -> Result<AutonomousEscalationPacketV1, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        validate_slug(reason, "autonomous escalation reason").map_err(validation_error)?;
        let snapshot = self.load_session(session_id)?;
        let packet = AutonomousEscalationPacketV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            session_id: session_id.into(),
            escalation_id: format!("esc-{}", snapshot.last_event_sequence.saturating_add(1)),
            state,
            reason: reason.into(),
            repair_attempts: snapshot.repair_attempts,
        };
        write_json_atomic(
            &self
                .session_dir(session_id)?
                .join("artifacts")
                .join("escalation.json"),
            &packet,
        )?;
        Ok(packet)
    }

    pub fn load_escalation(
        &self,
        session_id: &str,
    ) -> Result<AutonomousEscalationPacketV1, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        let packet: AutonomousEscalationPacketV1 = read_json(
            &self
                .session_dir(session_id)?
                .join("artifacts")
                .join("escalation.json"),
        )?;
        validate_slug(&packet.reason, "autonomous escalation reason").map_err(validation_error)?;
        if packet.schema_version != AUTONOMY_STATE_SCHEMA_VERSION
            || packet.session_id != session_id
            || validate_slug(&packet.escalation_id, "autonomous escalation id").is_err()
        {
            return Err(AutonomyStateError::Validation(
                "autonomous escalation packet is invalid for this session".into(),
            ));
        }
        Ok(packet)
    }

    pub fn write_provider_handoff(
        &self,
        session_id: &str,
        handoff: &AutonomousProviderHandoffV1,
    ) -> Result<(), AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        validate_provider_handoff(handoff, session_id)?;
        write_json_atomic(
            &self
                .session_dir(session_id)?
                .join("provider")
                .join("handoff.json"),
            handoff,
        )
    }

    pub fn load_provider_handoff(
        &self,
        session_id: &str,
    ) -> Result<AutonomousProviderHandoffV1, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        let handoff: AutonomousProviderHandoffV1 = read_json(
            &self
                .session_dir(session_id)?
                .join("provider")
                .join("handoff.json"),
        )?;
        validate_provider_handoff(&handoff, session_id)?;
        Ok(handoff)
    }

    pub fn write_planner_reply(
        &self,
        session_id: &str,
        escalation_id: &str,
        decision_hash: &str,
        decision: &str,
        constraints: &[String],
    ) -> Result<AutonomousPlannerReplyV1, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        validate_slug(escalation_id, "autonomous escalation id").map_err(validation_error)?;
        if decision_hash.trim().is_empty()
            || decision.trim().is_empty()
            || decision.len() > 4096
            || constraints.len() > 20
            || constraints
                .iter()
                .any(|constraint| constraint.trim().is_empty() || constraint.len() > 512)
        {
            return Err(AutonomyStateError::Validation(
                "autonomous planner reply is outside bounded policy".into(),
            ));
        }
        let packet = self.load_escalation(session_id)?;
        if packet.escalation_id != escalation_id {
            return Err(AutonomyStateError::Validation(
                "autonomous planner reply does not match the current escalation".into(),
            ));
        }
        let reply = AutonomousPlannerReplyV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            session_id: session_id.into(),
            escalation_id: escalation_id.into(),
            decision_hash: decision_hash.into(),
            decision: decision.into(),
            constraints: constraints.to_vec(),
            consumed: false,
        };
        write_json_atomic(
            &self
                .session_dir(session_id)?
                .join("artifacts")
                .join("planner-reply.json"),
            &reply,
        )?;
        Ok(reply)
    }

    pub fn load_planner_reply(
        &self,
        session_id: &str,
    ) -> Result<AutonomousPlannerReplyV1, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        let reply: AutonomousPlannerReplyV1 = read_json(
            &self
                .session_dir(session_id)?
                .join("artifacts")
                .join("planner-reply.json"),
        )?;
        if reply.schema_version != AUTONOMY_STATE_SCHEMA_VERSION
            || reply.session_id != session_id
            || validate_slug(&reply.escalation_id, "autonomous escalation id").is_err()
            || reply.decision_hash.trim().is_empty()
            || reply.decision.trim().is_empty()
            || reply.decision.len() > 4096
            || reply.constraints.len() > 20
            || reply
                .constraints
                .iter()
                .any(|constraint| constraint.trim().is_empty() || constraint.len() > 512)
        {
            return Err(AutonomyStateError::Validation(
                "autonomous planner reply is invalid for this session".into(),
            ));
        }
        Ok(reply)
    }

    pub fn mark_planner_reply_consumed(&self, session_id: &str) -> Result<(), AutonomyStateError> {
        let mut reply = self.load_planner_reply(session_id)?;
        reply.consumed = true;
        write_json_atomic(
            &self
                .session_dir(session_id)?
                .join("artifacts")
                .join("planner-reply.json"),
            &reply,
        )
    }

    pub fn load_task_output_baseline(
        &self,
        session_id: &str,
        task_id: &str,
    ) -> Result<Option<AutonomousTaskOutputBaselineV1>, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        validate_slug(task_id, "task output baseline task id").map_err(validation_error)?;
        let path = self
            .session_dir(session_id)?
            .join("artifacts")
            .join("task-output-baselines.json");
        if !path.exists() {
            return Ok(None);
        }
        let store: AutonomousTaskOutputBaselineStoreV1 = read_json(&path)?;
        if store.schema_version != AUTONOMY_STATE_SCHEMA_VERSION || store.session_id != session_id {
            return Err(AutonomyStateError::Validation(
                "task output baseline store is invalid".into(),
            ));
        }
        let baseline = store
            .baselines
            .into_iter()
            .find(|baseline| baseline.task_id == task_id);
        if let Some(baseline) = &baseline {
            validate_task_output_baseline(baseline, session_id)?;
        }
        Ok(baseline)
    }

    pub fn save_task_output_baseline(
        &self,
        session_id: &str,
        baseline: &AutonomousTaskOutputBaselineV1,
    ) -> Result<(), AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        validate_task_output_baseline(baseline, session_id)?;
        let path = self
            .session_dir(session_id)?
            .join("artifacts")
            .join("task-output-baselines.json");
        let mut store: AutonomousTaskOutputBaselineStoreV1 = if path.exists() {
            read_json(&path)?
        } else {
            AutonomousTaskOutputBaselineStoreV1 {
                schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
                session_id: session_id.into(),
                baselines: Vec::new(),
            }
        };
        if store.schema_version != AUTONOMY_STATE_SCHEMA_VERSION || store.session_id != session_id {
            return Err(AutonomyStateError::Validation(
                "task output baseline store is invalid".into(),
            ));
        }
        if let Some(existing) = store
            .baselines
            .iter()
            .find(|existing| existing.task_id == baseline.task_id)
        {
            if existing != baseline {
                return Err(AutonomyStateError::Validation(
                    "task output baseline is immutable for a logical task".into(),
                ));
            }
            return Ok(());
        }
        if store.baselines.len() >= 128 {
            return Err(AutonomyStateError::Validation(
                "task output baseline store is oversized".into(),
            ));
        }
        store.baselines.push(baseline.clone());
        write_json_atomic(&path, &store)
    }

    pub fn load_task_output_baseline_authority(
        &self,
        session_id: &str,
        task_id: &str,
    ) -> Result<Option<AutonomousTaskOutputBaselineAuthorityV1>, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        validate_slug(task_id, "task output baseline authority task id")
            .map_err(validation_error)?;
        let path = self
            .session_dir(session_id)?
            .join("artifacts")
            .join("task-output-baseline-authorities.json");
        if !path.exists() {
            return Ok(None);
        }
        let store: AutonomousTaskOutputBaselineAuthorityStoreV1 = read_json(&path)?;
        if store.schema_version != AUTONOMY_STATE_SCHEMA_VERSION || store.session_id != session_id {
            return Err(AutonomyStateError::Validation(
                "task output baseline authority store is invalid".into(),
            ));
        }
        let authority = store
            .authorities
            .into_iter()
            .find(|authority| authority.task_id == task_id);
        if let Some(authority) = &authority {
            validate_task_output_baseline_authority(authority, session_id)?;
        }
        Ok(authority)
    }

    pub fn save_task_output_baseline_authority(
        &self,
        session_id: &str,
        authority: &AutonomousTaskOutputBaselineAuthorityV1,
    ) -> Result<(), AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        validate_task_output_baseline_authority(authority, session_id)?;
        let path = self
            .session_dir(session_id)?
            .join("artifacts")
            .join("task-output-baseline-authorities.json");
        let mut store: AutonomousTaskOutputBaselineAuthorityStoreV1 = if path.exists() {
            read_json(&path)?
        } else {
            AutonomousTaskOutputBaselineAuthorityStoreV1 {
                schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
                session_id: session_id.into(),
                authorities: Vec::new(),
            }
        };
        if store.schema_version != AUTONOMY_STATE_SCHEMA_VERSION || store.session_id != session_id {
            return Err(AutonomyStateError::Validation(
                "task output baseline authority store is invalid".into(),
            ));
        }
        if let Some(existing) = store
            .authorities
            .iter()
            .find(|existing| existing.task_id == authority.task_id)
        {
            if existing != authority {
                return Err(AutonomyStateError::Validation(
                    "task output baseline authority is immutable for a logical task".into(),
                ));
            }
            return Ok(());
        }
        if store.authorities.len() >= 128 {
            return Err(AutonomyStateError::Validation(
                "task output baseline authority store is oversized".into(),
            ));
        }
        store.authorities.push(authority.clone());
        write_json_atomic(&path, &store)
    }

    pub fn write_completion_artifacts(
        &self,
        session_id: &str,
        verification: &VerificationSummaryV1,
        authoritative_diff: &str,
        final_review: &str,
    ) -> Result<(), AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        if authoritative_diff.trim().is_empty()
            || final_review.trim().is_empty()
            || authoritative_diff.len() > 512 * 1024
            || final_review.len() > 24 * 1024
        {
            return Err(AutonomyStateError::Validation(
                "autonomous completion artifact exceeds bounded storage".into(),
            ));
        }
        let artifacts = AutonomousCompletionArtifactsV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            session_id: session_id.into(),
            verification: verification.clone(),
            authoritative_diff: authoritative_diff.into(),
            final_review: final_review.into(),
        };
        let path = self
            .session_dir(session_id)?
            .join("artifacts")
            .join("completion.json");
        if path.exists() {
            let existing = self.load_completion_artifacts(session_id)?;
            return if existing == artifacts {
                Ok(())
            } else {
                Err(AutonomyStateError::Validation(
                    "autonomous completion artifact conflicts with durable evidence".into(),
                ))
            };
        }
        write_json_atomic(&path, &artifacts)
    }

    pub fn write_passed_verification_checkpoint(
        &self,
        session_id: &str,
        checkpoint: &AutonomousPassedVerificationCheckpointV1,
    ) -> Result<(), AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        validate_passed_verification_checkpoint(checkpoint, session_id)?;
        let path = self
            .session_dir(session_id)?
            .join("verification")
            .join("passed-finalization.json");
        if path.exists() {
            let existing = self
                .load_passed_verification_checkpoint(session_id)?
                .ok_or_else(|| {
                    AutonomyStateError::Validation(
                        "passed verification checkpoint is unavailable".into(),
                    )
                })?;
            return if existing == *checkpoint {
                Ok(())
            } else {
                Err(AutonomyStateError::Validation(
                    "passed verification checkpoint conflicts with durable evidence".into(),
                ))
            };
        }
        write_json_atomic(&path, checkpoint)
    }

    pub fn load_passed_verification_checkpoint(
        &self,
        session_id: &str,
    ) -> Result<Option<AutonomousPassedVerificationCheckpointV1>, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        let path = self
            .session_dir(session_id)?
            .join("verification")
            .join("passed-finalization.json");
        if !path.exists() {
            return Ok(None);
        }
        let checkpoint: AutonomousPassedVerificationCheckpointV1 = read_json(&path)?;
        validate_passed_verification_checkpoint(&checkpoint, session_id)?;
        Ok(Some(checkpoint))
    }

    /// A checkpoint belongs only to the current logical task. Once that task
    /// is durably complete and another approved task is ready, it cannot
    /// authorize the next task's finalization.
    pub fn clear_passed_verification_checkpoint(
        &self,
        session_id: &str,
    ) -> Result<(), AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        let path = self
            .session_dir(session_id)?
            .join("verification")
            .join("passed-finalization.json");
        if path.exists() {
            fs::remove_file(path).map_err(io_error)?;
        }
        Ok(())
    }

    pub fn load_completion_artifacts(
        &self,
        session_id: &str,
    ) -> Result<AutonomousCompletionArtifactsV1, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        let artifacts: AutonomousCompletionArtifactsV1 = read_json(
            &self
                .session_dir(session_id)?
                .join("artifacts")
                .join("completion.json"),
        )?;
        if artifacts.schema_version != AUTONOMY_STATE_SCHEMA_VERSION
            || artifacts.session_id != session_id
            || artifacts.authoritative_diff.trim().is_empty()
            || artifacts.final_review.trim().is_empty()
            || artifacts.authoritative_diff.len() > 512 * 1024
            || artifacts.final_review.len() > 24 * 1024
        {
            return Err(AutonomyStateError::Validation(
                "autonomous completion artifact is invalid for this session".into(),
            ));
        }
        Ok(artifacts)
    }

    /// Idempotently adds an unread review item. The stable record id is
    /// derived from the durable state version, so controller retries and
    /// restart recovery cannot create duplicate notices.
    pub fn emit_review_inbox_record(
        &self,
        session_id: &str,
        project_id: &str,
        state: AutonomousSessionStateV1,
        next_action: &str,
        reference: &str,
        created_at_unix: u64,
    ) -> Result<AutonomousReviewInboxRecordV1, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        validate_slug(project_id, "review inbox project id").map_err(validation_error)?;
        validate_slug(next_action, "review inbox next action").map_err(validation_error)?;
        if reference.trim().is_empty()
            || reference.len() > 1024
            || contains_secret_marker(reference)
        {
            return Err(AutonomyStateError::Validation(
                "review inbox reference is invalid or unsafe".into(),
            ));
        }
        let snapshot = self.load_session(session_id)?;
        let record_id = format!(
            "review-{}-{}-{}",
            session_id, snapshot.last_event_sequence, next_action
        );
        let path = self.root.join("review-inbox.json");
        let mut records: Vec<AutonomousReviewInboxRecordV1> = if path.exists() {
            read_json(&path)?
        } else {
            Vec::new()
        };
        if let Some(record) = records.iter().find(|record| record.record_id == record_id) {
            return Ok(record.clone());
        }
        compact_review_inbox_for_capacity(&mut records);
        if records.len() >= AUTONOMY_REVIEW_INBOX_RECORD_LIMIT {
            return Err(AutonomyStateError::Validation(
                "review inbox reached its bounded record limit".into(),
            ));
        }
        let record = AutonomousReviewInboxRecordV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            record_id,
            project_id: project_id.into(),
            session_id: session_id.into(),
            state,
            next_action: next_action.into(),
            reference: reference.into(),
            created_at_unix,
            unread: true,
        };
        validate_review_record(&record)?;
        records.push(record.clone());
        write_json_atomic(&path, &records)?;
        Ok(record)
    }

    /// Idempotently persists a delegated worker review without fabricating an
    /// autonomous session. The delegated run id occupies the legacy
    /// `session_id` field so the existing bounded inbox/bridge consumer stays
    /// wire-compatible while project identity remains independently bound.
    pub fn emit_delegated_review_inbox_record(
        &self,
        run_id: &str,
        project_id: &str,
        reference: &str,
        created_at_unix: u64,
    ) -> Result<AutonomousReviewInboxRecordV1, AutonomyStateError> {
        if run_id.is_empty()
            || run_id.len() > 120
            || !run_id.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
        {
            return Err(AutonomyStateError::Validation(
                "delegated review run id is invalid".into(),
            ));
        }
        validate_slug(project_id, "review inbox project id").map_err(validation_error)?;
        let compatibility_session_id = run_id.replace(['.', ':'], "_");
        if reference.trim().is_empty()
            || reference.len() > 1024
            || contains_secret_marker(reference)
            || created_at_unix == 0
        {
            return Err(AutonomyStateError::Validation(
                "delegated review reference is invalid or unsafe".into(),
            ));
        }
        let next_action = "independent_final_review";
        let record_id = format!("review-{compatibility_session_id}");
        let path = self.root.join("review-inbox.json");
        let mut records: Vec<AutonomousReviewInboxRecordV1> = if path.exists() {
            read_json(&path)?
        } else {
            Vec::new()
        };
        if let Some(existing) = records.iter().find(|record| record.record_id == record_id) {
            let expected = AutonomousReviewInboxRecordV1 {
                schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
                record_id,
                project_id: project_id.into(),
                session_id: compatibility_session_id.clone(),
                state: AutonomousSessionStateV1::CompletedVerified,
                next_action: next_action.into(),
                reference: reference.into(),
                created_at_unix: existing.created_at_unix,
                unread: existing.unread,
            };
            if *existing != expected {
                return Err(AutonomyStateError::Validation(
                    "delegated review replay conflicts with durable inbox evidence".into(),
                ));
            }
            return Ok(existing.clone());
        }
        compact_review_inbox_for_capacity(&mut records);
        if records.len() >= AUTONOMY_REVIEW_INBOX_RECORD_LIMIT {
            return Err(AutonomyStateError::Validation(
                "review inbox reached its bounded record limit".into(),
            ));
        }
        let record = AutonomousReviewInboxRecordV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            record_id,
            project_id: project_id.into(),
            session_id: compatibility_session_id.clone(),
            state: AutonomousSessionStateV1::CompletedVerified,
            next_action: next_action.into(),
            reference: reference.into(),
            created_at_unix,
            unread: true,
        };
        validate_review_record(&record)?;
        records.push(record.clone());
        write_json_atomic(&path, &records)?;
        Ok(record)
    }

    pub fn list_review_inbox(
        &self,
        project_id: Option<&str>,
        after_record_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<AutonomousReviewInboxRecordV1>, AutonomyStateError> {
        if let Some(project_id) = project_id {
            validate_slug(project_id, "review inbox project id").map_err(validation_error)?;
        }
        let path = self.root.join("review-inbox.json");
        let records: Vec<AutonomousReviewInboxRecordV1> = if path.exists() {
            read_json(&path)?
        } else {
            Vec::new()
        };
        let start = after_record_id
            .and_then(|id| records.iter().position(|record| record.record_id == id))
            .map_or(0, |index| index.saturating_add(1));
        records
            .into_iter()
            .skip(start)
            .filter(|record| record.unread)
            .filter(|record| project_id.is_none_or(|project_id| record.project_id == project_id))
            .take(limit.min(100))
            .map(|record| {
                validate_review_record(&record)?;
                Ok(record)
            })
            .collect()
    }

    /// Audit-only reader includes acknowledged records so review state is
    /// evidence rather than a presentation filter.
    pub fn all_review_inbox(
        &self,
    ) -> Result<Vec<AutonomousReviewInboxRecordV1>, AutonomyStateError> {
        let path = self.root.join("review-inbox.json");
        let records: Vec<AutonomousReviewInboxRecordV1> = if path.exists() {
            read_json(&path)?
        } else {
            Vec::new()
        };
        records
            .into_iter()
            .map(|record| {
                validate_review_record(&record)?;
                Ok(record)
            })
            .collect()
    }

    /// Persists one explicit replacement relation.  The audit consumes only
    /// this file; it does not infer supersession from task IDs or dates.
    pub fn record_session_supersession(
        &self,
        superseded_session_id: &str,
        superseding_session_id: &str,
        recorded_at_unix: u64,
    ) -> Result<AutonomousSessionSupersessionV1, AutonomyStateError> {
        self.ensure_session_exists(superseded_session_id)?;
        self.ensure_session_exists(superseding_session_id)?;
        if superseded_session_id == superseding_session_id || recorded_at_unix == 0 {
            return Err(AutonomyStateError::Validation(
                "autonomous supersession relation is invalid".into(),
            ));
        }
        let path = self.root.join("supersessions.json");
        let mut relations: Vec<AutonomousSessionSupersessionV1> = if path.exists() {
            read_json(&path)?
        } else {
            Vec::new()
        };
        if let Some(existing) = relations
            .iter()
            .find(|relation| relation.superseded_session_id == superseded_session_id)
        {
            if existing.superseding_session_id == superseding_session_id {
                return Ok(existing.clone());
            }
            return Err(AutonomyStateError::Validation(
                "autonomous session already has explicit supersession evidence".into(),
            ));
        }
        if relations.len() >= 512 {
            return Err(AutonomyStateError::Validation(
                "autonomous supersession relation limit reached".into(),
            ));
        }
        let relation = AutonomousSessionSupersessionV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            superseded_session_id: superseded_session_id.into(),
            superseding_session_id: superseding_session_id.into(),
            recorded_at_unix,
        };
        relations.push(relation.clone());
        write_json_atomic(&path, &relations)?;
        Ok(relation)
    }

    pub fn list_session_supersessions(
        &self,
    ) -> Result<Vec<AutonomousSessionSupersessionV1>, AutonomyStateError> {
        let path = self.root.join("supersessions.json");
        let relations: Vec<AutonomousSessionSupersessionV1> = if path.exists() {
            read_json(&path)?
        } else {
            Vec::new()
        };
        if relations.len() > 512
            || relations.iter().any(|relation| {
                relation.schema_version != AUTONOMY_STATE_SCHEMA_VERSION
                    || relation.recorded_at_unix == 0
                    || validate_slug(
                        &relation.superseded_session_id,
                        "autonomous superseded session id",
                    )
                    .is_err()
                    || validate_slug(
                        &relation.superseding_session_id,
                        "autonomous superseding session id",
                    )
                    .is_err()
                    || relation.superseded_session_id == relation.superseding_session_id
            })
        {
            return Err(AutonomyStateError::Validation(
                "autonomous supersession evidence is invalid".into(),
            ));
        }
        Ok(relations)
    }

    /// Return the newest unread review event that hands one exact session back
    /// to ChatGPT. This scans the bounded durable inbox directly so dispatch is
    /// independent of the public list API's presentation limit.
    pub fn latest_actionable_review_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<AutonomousReviewInboxRecordV1>, AutonomyStateError> {
        validate_slug(session_id, "review inbox session id").map_err(validation_error)?;
        let path = self.root.join("review-inbox.json");
        let records: Vec<AutonomousReviewInboxRecordV1> = if path.exists() {
            read_json(&path)?
        } else {
            Vec::new()
        };
        for record in records.into_iter().rev() {
            validate_review_record(&record)?;
            if record.unread
                && record.session_id == session_id
                && ((record.state == AutonomousSessionStateV1::CompletedVerified
                    && record.next_action == "independent_final_review")
                    || (record.state == AutonomousSessionStateV1::WaitingForChatgpt
                        && record.next_action == "chatgpt_decision_required"))
            {
                return Ok(Some(record));
            }
        }
        Ok(None)
    }

    /// Acknowledgement is idempotent: a polling bridge may safely retry it
    /// after a process crash without re-notifying or losing the record.
    pub fn acknowledge_review_inbox(
        &self,
        record_id: &str,
    ) -> Result<AutonomousReviewInboxRecordV1, AutonomyStateError> {
        validate_slug(record_id, "review inbox record id").map_err(validation_error)?;
        let path = self.root.join("review-inbox.json");
        let mut records: Vec<AutonomousReviewInboxRecordV1> = read_json(&path)?;
        let record = records
            .iter_mut()
            .find(|record| record.record_id == record_id)
            .ok_or_else(|| {
                AutonomyStateError::Validation("review inbox record was not found".into())
            })?;
        validate_review_record(record)?;
        record.unread = false;
        let result = record.clone();
        write_json_atomic(&path, &records)?;
        Ok(result)
    }

    pub fn acquire_lock(
        &self,
        session_id: &str,
        owner_id: &str,
    ) -> Result<AutonomousSessionLockV1, AutonomyStateError> {
        self.ensure_session_exists(session_id)?;
        validate_slug(owner_id, "autonomous lock owner id").map_err(validation_error)?;
        let path = self
            .session_dir(session_id)?
            .join("locks")
            .join("session.lock");
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = options.open(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                AutonomyStateError::LockHeld(session_id.into())
            } else {
                io_error(error)
            }
        })?;
        file.write_all(owner_id.as_bytes()).map_err(io_error)?;
        file.write_all(b"\n").map_err(io_error)?;
        file.sync_data().map_err(io_error)?;
        Ok(AutonomousSessionLockV1 {
            path,
            owner_id: owner_id.into(),
            released: false,
        })
    }

    pub fn recover_interrupted_sessions(
        &self,
    ) -> Result<Vec<AutonomousSessionSnapshotV1>, AutonomyStateError> {
        let mut recovered = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            if !entry.file_type().map_err(io_error)?.is_dir() {
                continue;
            }
            let session_id = entry.file_name().to_string_lossy().into_owned();
            let mut snapshot = match self.load_session(&session_id) {
                Ok(snapshot) => snapshot,
                Err(AutonomyStateError::SessionNotFound(_)) => continue,
                Err(error) => return Err(error),
            };
            if snapshot.active && snapshot.state.needs_restart_recovery() {
                snapshot.state = AutonomousSessionStateV1::RecoveringAfterRestart;
                self.save_session(&snapshot)?;
                self.append_event(
                    &session_id,
                    "restart_recovery_required",
                    "in-flight autonomy work requires continuity and policy review after restart",
                )?;
                recovered.push(self.load_session(&session_id)?);
            }
        }
        Ok(recovered)
    }

    pub fn list_sessions(&self) -> Result<Vec<AutonomousSessionSnapshotV1>, AutonomyStateError> {
        let mut sessions = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            if !entry.file_type().map_err(io_error)?.is_dir() {
                continue;
            }
            let session_id = entry.file_name().to_string_lossy().into_owned();
            if let Ok(snapshot) = self.load_session(&session_id) {
                sessions.push(snapshot);
            }
        }
        sessions.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        Ok(sessions)
    }

    fn ensure_session_exists(&self, session_id: &str) -> Result<(), AutonomyStateError> {
        if self.session_dir(session_id)?.join("state.json").is_file() {
            Ok(())
        } else {
            Err(AutonomyStateError::SessionNotFound(session_id.into()))
        }
    }

    fn session_dir(&self, session_id: &str) -> Result<PathBuf, AutonomyStateError> {
        validate_slug(session_id, "autonomous session id").map_err(validation_error)?;
        Ok(self.root.join(session_id))
    }
}

impl AutonomousSessionLockV1 {
    pub fn release(mut self) -> Result<(), AutonomyStateError> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> Result<(), AutonomyStateError> {
        if self.released {
            return Ok(());
        }
        let owner = fs::read_to_string(&self.path).map_err(io_error)?;
        if owner.trim() != self.owner_id {
            return Err(AutonomyStateError::LockOwnership(
                "autonomous lock content did not match owner".into(),
            ));
        }
        fs::remove_file(&self.path).map_err(io_error)?;
        self.released = true;
        Ok(())
    }
}

impl Drop for AutonomousSessionLockV1 {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

fn validate_snapshot(
    snapshot: &AutonomousSessionSnapshotV1,
    expected_session_id: &str,
) -> Result<(), AutonomyStateError> {
    if snapshot.schema_version != AUTONOMY_STATE_SCHEMA_VERSION {
        return Err(AutonomyStateError::Validation(
            "unsupported autonomous state schema version".into(),
        ));
    }
    validate_slug(&snapshot.session_id, "autonomous session id").map_err(validation_error)?;
    if snapshot.session_id != expected_session_id {
        return Err(AutonomyStateError::Validation(
            "autonomous state session id does not match storage location".into(),
        ));
    }
    if snapshot.active == snapshot.state.is_terminal() {
        return Err(AutonomyStateError::Validation(
            "autonomous state active flag contradicts terminal state".into(),
        ));
    }
    if let Some(task_id) = &snapshot.current_task_id {
        validate_slug(task_id, "autonomous current task id").map_err(validation_error)?;
    }
    if let Some(telemetry) = &snapshot.codex_routing_telemetry {
        validate_telemetry(telemetry).map_err(runtime_validation_error)?;
    }
    if let Some(continuity) = &snapshot.codex_continuity {
        validate_continuity_evidence(continuity)?;
        if snapshot.provider_thread_id.as_deref() != Some(continuity.thread_id.as_str()) {
            return Err(AutonomyStateError::Validation(
                "continuity evidence thread does not match the session binding".into(),
            ));
        }
    }
    snapshot
        .github_publication_journal
        .validate()
        .map_err(|_| {
            AutonomyStateError::Validation("GitHub publication journal is invalid".into())
        })?;
    Ok(())
}

fn validate_task_output_baseline(
    baseline: &AutonomousTaskOutputBaselineV1,
    session_id: &str,
) -> Result<(), AutonomyStateError> {
    if baseline.schema_version != AUTONOMY_STATE_SCHEMA_VERSION
        || baseline.session_id != session_id
        || validate_slug(&baseline.task_id, "task output baseline task id").is_err()
        || !baseline.approved_contract_hash.starts_with("fnv1a64:")
        || baseline.approved_contract_hash.len() != "fnv1a64:".len() + 16
        || !baseline.approved_contract_hash["fnv1a64:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || baseline.completion_artifact_ids.is_empty()
        || baseline.completion_artifact_ids.len() > 64
        || baseline.observations.len() != baseline.completion_artifact_ids.len()
    {
        return Err(AutonomyStateError::Validation(
            "task output baseline has an invalid shape".into(),
        ));
    }
    let expected = baseline
        .completion_artifact_ids
        .iter()
        .collect::<BTreeSet<_>>();
    let actual = baseline
        .observations
        .iter()
        .map(|observation| &observation.artifact_id)
        .collect::<BTreeSet<_>>();
    if expected.len() != baseline.completion_artifact_ids.len() || expected != actual {
        return Err(AutonomyStateError::Validation(
            "task output baseline artifacts are not exact".into(),
        ));
    }
    for observation in &baseline.observations {
        let valid_sha = observation.sha256.as_deref().is_some_and(|value| {
            value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
        if observation.artifact_id.is_empty()
            || observation.artifact_id.len() > 512
            || !matches!(observation.state.as_str(), "ABSENT" | "PRESENT_SHA256")
            || (observation.state == "ABSENT" && observation.sha256.is_some())
            || (observation.state == "PRESENT_SHA256" && !valid_sha)
        {
            return Err(AutonomyStateError::Validation(
                "task output baseline observation is invalid".into(),
            ));
        }
    }
    Ok(())
}

fn validate_task_output_baseline_authority(
    authority: &AutonomousTaskOutputBaselineAuthorityV1,
    session_id: &str,
) -> Result<(), AutonomyStateError> {
    let baseline = AutonomousTaskOutputBaselineV1 {
        schema_version: authority.schema_version,
        session_id: authority.session_id.clone(),
        task_id: authority.task_id.clone(),
        approved_contract_hash: authority.approved_contract_hash.clone(),
        completion_artifact_ids: authority.completion_artifact_ids.clone(),
        observations: authority
            .completion_artifact_ids
            .iter()
            .map(|artifact_id| AutonomousTaskOutputObservationV1 {
                artifact_id: artifact_id.clone(),
                state: "ABSENT".into(),
                sha256: None,
            })
            .collect(),
    };
    validate_task_output_baseline(&baseline, session_id)
}

fn validate_passed_verification_checkpoint(
    checkpoint: &AutonomousPassedVerificationCheckpointV1,
    session_id: &str,
) -> Result<(), AutonomyStateError> {
    let valid_sha =
        |value: &str| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
    let mut diff_hasher = Sha256::new();
    diff_hasher.update(checkpoint.authoritative_diff.as_bytes());
    let expected_diff_sha256 = format!("{:x}", diff_hasher.finalize());
    let attribution_material = serde_json::to_vec(&(
        checkpoint.approved_contract_hash.as_str(),
        checkpoint.logical_task_id.as_str(),
        checkpoint.reviewed_source_expectation.as_ref(),
    ))
    .map_err(|_| {
        AutonomyStateError::Validation("passed verification checkpoint is invalid".into())
    })?;
    let mut attribution_hasher = Sha256::new();
    attribution_hasher.update(attribution_material);
    let expected_attribution_digest = format!("{:x}", attribution_hasher.finalize());
    if checkpoint.schema_version != AUTONOMY_STATE_SCHEMA_VERSION
        || checkpoint.session_id != session_id
        || validate_slug(&checkpoint.project_id, "passed verification project id").is_err()
        || validate_slug(&checkpoint.logical_task_id, "passed verification task id").is_err()
        || !checkpoint.approved_contract_hash.starts_with("fnv1a64:")
        || checkpoint.approved_contract_hash.len() != "fnv1a64:".len() + 16
        || !checkpoint.approved_contract_hash["fnv1a64:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || !matches!(
            checkpoint.provider_id.as_str(),
            "codex-cli" | "ollama" | "fake" | "chatgpt-direct"
        )
        || checkpoint.provider_turn_count == 0
        || checkpoint.verification_profile.is_empty()
        || checkpoint.verification_profile.len() > 128
        || checkpoint.verification.status != super::coordinator::VerificationStatusV1::Passed
        || checkpoint.verification.command.is_empty()
        || checkpoint.verification.command.len() > 2_048
        || checkpoint.verification.summary.is_empty()
        || checkpoint.verification.summary.len() > 8 * 1024
        || checkpoint.authoritative_diff.trim().is_empty()
        || checkpoint.authoritative_diff.len() > 512 * 1024
        || !valid_sha(&checkpoint.authoritative_diff_sha256)
        || !valid_sha(&checkpoint.attribution_digest)
        || checkpoint.authoritative_diff_sha256 != expected_diff_sha256
        || checkpoint.attribution_digest != expected_attribution_digest
        || checkpoint
            .reviewed_source_expectation
            .as_ref()
            .is_some_and(|expected| {
                !valid_checkpointed_reviewed_source_expectation(expected, checkpoint)
            })
    {
        return Err(AutonomyStateError::Validation(
            "passed verification checkpoint is invalid".into(),
        ));
    }
    Ok(())
}

fn valid_checkpointed_reviewed_source_expectation(
    expected: &ReviewedSourceSnapshotExpectedV1,
    checkpoint: &AutonomousPassedVerificationCheckpointV1,
) -> bool {
    let valid_sha =
        |value: &str| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
    expected.session_id == checkpoint.session_id
        && expected.project_id == checkpoint.project_id
        && expected.approved_contract_hash == checkpoint.approved_contract_hash
        && expected.logical_task_id == checkpoint.logical_task_id
        && !expected.completion_artifact_ids.is_empty()
        && expected.completion_artifact_ids.len() <= 256
        && expected.current_outputs.len() == expected.completion_artifact_ids.len()
        && expected
            .completion_artifact_ids
            .iter()
            .zip(&expected.current_outputs)
            .all(|(artifact_id, output)| {
                artifact_id == &output.artifact_id
                    && !artifact_id.is_empty()
                    && artifact_id.len() <= 512
                    && valid_sha(&output.sha256)
            })
        && expected.baseline_observations.len() == expected.completion_artifact_ids.len()
}

fn validate_wake_policy(policy: &AutonomousWakePolicyV1) -> Result<(), AutonomyStateError> {
    if policy.schema_version != AUTONOMY_WAKE_POLICY_SCHEMA_VERSION {
        return Err(AutonomyStateError::Validation(
            "unsupported autonomous wake policy schema".into(),
        ));
    }
    match policy.mode {
        AutonomousWakeModeV1::ThroughTask => {
            let Some(task_id) = policy.terminal_task_id.as_deref() else {
                return Err(AutonomyStateError::Validation(
                    "through-task wake policy requires an exact task id".into(),
                ));
            };
            validate_slug(task_id, "autonomous wake terminal task id").map_err(validation_error)?;
        }
        _ if policy.terminal_task_id.is_some() => {
            return Err(AutonomyStateError::Validation(
                "only through-task wake policy may set a terminal task id".into(),
            ));
        }
        _ => {}
    }
    if policy
        .stopped_reason
        .as_ref()
        .is_some_and(|reason| validate_slug(reason, "autonomous wake stopped reason").is_err())
    {
        return Err(AutonomyStateError::Validation(
            "autonomous wake stopped reason is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_continuity_evidence(
    evidence: &CodexContinuityEvidenceV1,
) -> Result<(), AutonomyStateError> {
    if evidence.thread_id.is_empty()
        || evidence.first_turn_id.is_empty()
        || [
            evidence.thread_id.as_str(),
            evidence.first_turn_id.as_str(),
            evidence.second_turn_id.as_deref().unwrap_or_default(),
        ]
        .into_iter()
        .any(|value| value.len() > 256 || contains_secret_marker(value))
        || (evidence.second_completed && evidence.second_turn_id.is_none())
        || (evidence.second_turn_id.is_some() && !evidence.first_completed)
    {
        return Err(AutonomyStateError::Validation(
            "Codex continuity evidence is invalid or unsafe".into(),
        ));
    }
    Ok(())
}

fn validate_plan_queue(plan: &AutonomousPlanQueueV1) -> Result<(), AutonomyStateError> {
    if plan.schema_version != AUTONOMY_STATE_SCHEMA_VERSION
        || plan.tasks.is_empty()
        || plan.tasks.len() > 128
    {
        return Err(AutonomyStateError::Validation(
            "autonomous plan queue has an unsupported shape".into(),
        ));
    }
    let ids = plan
        .tasks
        .iter()
        .map(|task| task.task_id.as_str())
        .collect::<BTreeSet<_>>();
    if ids.len() != plan.tasks.len() {
        return Err(AutonomyStateError::Validation(
            "autonomous planned task ids must be unique".into(),
        ));
    }
    for task in &plan.tasks {
        validate_slug(&task.task_id, "autonomous planned task id").map_err(validation_error)?;
        if task
            .depends_on
            .iter()
            .any(|dependency| dependency == &task.task_id || !ids.contains(dependency.as_str()))
            || task.acceptance_criteria.is_empty()
            || task.allowed_paths.is_empty()
            || task.verification_profile.trim().is_empty()
            || task.preferred_worker_policy.trim().is_empty()
            || task.acceptance_criteria.len() > 32
            || task.allowed_paths.len() > 64
            || task.escalation_conditions.len() > 32
            || task.completion_artifact_ids.len() > 64
            || task.provider_provenance.len() > 64
            || task
                .acceptance_criteria
                .iter()
                .chain(task.allowed_paths.iter())
                .chain(task.escalation_conditions.iter())
                .chain(task.completion_artifact_ids.iter())
                .chain(task.provider_provenance.iter())
                .any(|value| {
                    value.is_empty() || value.len() > 1024 || contains_secret_marker(value)
                })
        {
            return Err(AutonomyStateError::Validation(
                "autonomous planned task metadata is invalid or unsafe".into(),
            ));
        }
    }
    Ok(())
}

fn validate_provider_handoff(
    handoff: &AutonomousProviderHandoffV1,
    expected_session_id: &str,
) -> Result<(), AutonomyStateError> {
    if handoff.schema_version != AUTONOMY_STATE_SCHEMA_VERSION
        || handoff.session_id != expected_session_id
        || handoff.from_provider_id != "codex-cli"
        || handoff.to_provider_id != "ollama"
        || handoff.reason != "codex_credits_exhausted"
        || handoff.objective.trim().is_empty()
        || handoff.objective.len() > 8_000
        || handoff.ordered_steps.len() > 20
        || handoff.acceptance_criteria.len() > 20
        || handoff.allowed_paths.len() > 64
        || handoff.forbidden_paths.len() > 64
        || handoff.changed_files.len() > 256
        || handoff.git_status_porcelain.len() > 256
        || handoff.completed_tool_calls.len() > 128
        || handoff.pending_tool_calls.len() > 128
        || handoff
            .ordered_steps
            .iter()
            .chain(handoff.acceptance_criteria.iter())
            .chain(handoff.allowed_paths.iter())
            .chain(handoff.forbidden_paths.iter())
            .chain(handoff.changed_files.iter())
            .chain(handoff.git_status_porcelain.iter())
            .any(|value| value.len() > 1024 || contains_secret_marker(value))
        || [
            handoff.objective.as_str(),
            handoff.base_branch.as_str(),
            handoff.feature_branch.as_str(),
            handoff.base_commit.as_str(),
            handoff.expected_origin.as_str(),
            handoff.codex_thread_id.as_deref().unwrap_or_default(),
        ]
        .iter()
        .any(|value| value.len() > 8_000 || contains_secret_marker(value))
        || handoff
            .last_verification_summary
            .as_deref()
            .is_some_and(|value| value.len() > 2_048 || contains_secret_marker(value))
        || handoff
            .completed_tool_calls
            .iter()
            .chain(handoff.pending_tool_calls.iter())
            .any(|call| {
                call.tool_call_id.is_empty()
                    || call.tool_name.is_empty()
                    || call.mutation_kind.is_empty()
                    || call.status.is_empty()
                    || [
                        call.tool_call_id.as_str(),
                        call.tool_name.as_str(),
                        call.mutation_kind.as_str(),
                        call.status.as_str(),
                        call.request_hash.as_str(),
                        call.result_hash.as_deref().unwrap_or_default(),
                        call.outcome_summary.as_deref().unwrap_or_default(),
                    ]
                    .iter()
                    .any(|value| value.len() > 1_024 || contains_secret_marker(value))
            })
    {
        return Err(AutonomyStateError::Validation(
            "provider handoff is invalid, unbounded, or contains a secret marker".into(),
        ));
    }
    validate_slug(&handoff.task_id, "provider handoff task id").map_err(validation_error)?;
    validate_slug(&handoff.pending_task_id, "provider handoff pending task id")
        .map_err(validation_error)?;
    Ok(())
}

/// Shared wire-contract validation for consumers of the durable review inbox.
/// It has no controller, daemon, or persistence side effects.
pub(crate) fn validate_review_inbox_record(
    record: &AutonomousReviewInboxRecordV1,
) -> Result<(), AutonomyStateError> {
    if record.schema_version != AUTONOMY_STATE_SCHEMA_VERSION
        || record.reference.trim().is_empty()
        || record.reference.len() > 1024
        || contains_secret_marker(&record.reference)
    {
        return Err(AutonomyStateError::Validation(
            "review inbox record is invalid or unsafe".into(),
        ));
    }
    validate_slug(&record.record_id, "review inbox record id").map_err(validation_error)?;
    validate_slug(&record.project_id, "review inbox project id").map_err(validation_error)?;
    validate_slug(&record.session_id, "review inbox session id").map_err(validation_error)?;
    validate_slug(&record.next_action, "review inbox next action").map_err(validation_error)
}

fn validate_review_record(
    record: &AutonomousReviewInboxRecordV1,
) -> Result<(), AutonomyStateError> {
    validate_review_inbox_record(record)
}

fn compact_review_inbox_for_capacity(records: &mut Vec<AutonomousReviewInboxRecordV1>) -> usize {
    if records.len() < AUTONOMY_REVIEW_INBOX_COMPACT_TRIGGER {
        return 0;
    }

    let mut removable = records
        .len()
        .saturating_sub(AUTONOMY_REVIEW_INBOX_COMPACT_TARGET);
    if removable == 0 {
        return 0;
    }

    let before = records.len();
    records.retain(|record| {
        if removable > 0 && !record.unread {
            removable -= 1;
            false
        } else {
            true
        }
    });
    before.saturating_sub(records.len())
}

fn contains_secret_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "token=",
        "password=",
        "secret=",
        "api_key=",
        "authorization:",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn validate_slug(value: &str, label: &str) -> Result<(), RuntimeError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(RuntimeError::Validation(format!(
            "{label} must be a conservative non-empty slug"
        )));
    }
    Ok(())
}

fn bounded_summary(value: &str) -> Result<String, AutonomyStateError> {
    if value.trim().is_empty() {
        return Err(AutonomyStateError::Validation(
            "autonomous event summary must not be empty".into(),
        ));
    }
    if value.len() > 4096 {
        return Err(AutonomyStateError::Validation(
            "autonomous event summary exceeds bounded storage".into(),
        ));
    }
    Ok(value.into())
}

fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<(), AutonomyStateError> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(io_error)?;
    serde_json::to_writer(&mut file, value).map_err(serde_error)?;
    file.write_all(b"\n").map_err(io_error)?;
    file.sync_data().map_err(io_error)
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), AutonomyStateError> {
    let parent = path.parent().ok_or_else(|| {
        AutonomyStateError::Validation("autonomous state path has no parent directory".into())
    })?;
    fs::create_dir_all(parent).map_err(io_error)?;
    let tmp_path = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|v| v.to_str()).unwrap_or("state"),
        Uuid::new_v4()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .map_err(io_error)?;
        serde_json::to_writer_pretty(&mut file, value).map_err(serde_error)?;
        file.write_all(b"\n").map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        fs::rename(&tmp_path, path).map_err(io_error)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, AutonomyStateError> {
    let file = File::open(path).map_err(io_error)?;
    serde_json::from_reader(file).map_err(serde_error)
}

fn read_jsonl_repair_tail<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>, AutonomyStateError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    repair_torn_jsonl_tail(path)?;
    let file = File::open(path).map_err(io_error)?;
    BufReader::new(file)
        .lines()
        .filter(|line| line.as_ref().is_ok_and(|line| !line.trim().is_empty()))
        .map(|line| {
            line.map_err(io_error)
                .and_then(|line| serde_json::from_str(&line).map_err(serde_error))
        })
        .collect()
}

fn repair_torn_jsonl_tail(path: &Path) -> Result<(), AutonomyStateError> {
    let bytes = fs::read(path).map_err(io_error)?;
    let mut offset = 0usize;
    while offset < bytes.len() {
        let relative_newline = bytes[offset..].iter().position(|byte| *byte == b'\n');
        let (line_end, record_end) = match relative_newline {
            Some(index) => (offset + index, offset + index + 1),
            None => (bytes.len(), bytes.len()),
        };
        let line = &bytes[offset..line_end];
        if !line.is_empty() && serde_json::from_slice::<serde_json::Value>(line).is_err() {
            if record_end != bytes.len() {
                return Err(AutonomyStateError::Serde(
                    "autonomous event journal has malformed non-terminal record".into(),
                ));
            }
            let file = OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(io_error)?;
            file.set_len(offset as u64).map_err(io_error)?;
            return Ok(());
        }
        offset = record_end;
    }
    Ok(())
}

fn io_error(error: std::io::Error) -> AutonomyStateError {
    AutonomyStateError::Io(error.to_string())
}

fn serde_error(error: serde_json::Error) -> AutonomyStateError {
    AutonomyStateError::Serde(error.to_string())
}

fn validation_error(error: RuntimeError) -> AutonomyStateError {
    AutonomyStateError::Validation(format!("{error:?}"))
}

fn runtime_validation_error(error: RuntimeError) -> AutonomyStateError {
    validation_error(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> (PathBuf, AutonomousStateStoreV1) {
        let root = std::env::temp_dir().join(format!("catdesk-autonomy-{name}-{}", Uuid::new_v4()));
        let store = AutonomousStateStoreV1::open(&root).expect("store opens");
        (root, store)
    }

    fn queue() -> AutonomousQueueV1 {
        AutonomousQueueV1::new(vec![
            AutonomousQueueTaskV1 {
                task_id: "analysis".into(),
                priority: 10,
                depends_on: Vec::new(),
                state: AutonomousQueueTaskStateV1::Ready,
            },
            AutonomousQueueTaskV1 {
                task_id: "repair".into(),
                priority: 99,
                depends_on: vec!["analysis".into()],
                state: AutonomousQueueTaskStateV1::Ready,
            },
        ])
        .expect("valid queue")
    }

    #[test]
    fn queue_selects_highest_priority_ready_task_with_satisfied_dependencies() {
        let mut queue = queue();
        assert_eq!(
            queue.next_ready_task().map(|task| task.task_id.as_str()),
            Some("analysis")
        );
        queue.tasks[0].state = AutonomousQueueTaskStateV1::CompletedVerified;
        assert_eq!(
            queue.next_ready_task().map(|task| task.task_id.as_str()),
            Some("repair")
        );
    }

    #[test]
    fn dag_queue_progresses_a_then_b_c_then_d_and_rejects_cycles() {
        let mut queue = AutonomousQueueV1::new(vec![
            AutonomousQueueTaskV1 {
                task_id: "a".into(),
                priority: 1,
                depends_on: vec![],
                state: AutonomousQueueTaskStateV1::Ready,
            },
            AutonomousQueueTaskV1 {
                task_id: "b".into(),
                priority: 2,
                depends_on: vec!["a".into()],
                state: AutonomousQueueTaskStateV1::Ready,
            },
            AutonomousQueueTaskV1 {
                task_id: "c".into(),
                priority: 1,
                depends_on: vec!["a".into()],
                state: AutonomousQueueTaskStateV1::Ready,
            },
            AutonomousQueueTaskV1 {
                task_id: "d".into(),
                priority: 1,
                depends_on: vec!["b".into(), "c".into()],
                state: AutonomousQueueTaskStateV1::Ready,
            },
        ])
        .expect("valid dag");
        assert_eq!(
            queue.next_ready_task().map(|task| task.task_id.as_str()),
            Some("a")
        );
        queue.tasks[0].state = AutonomousQueueTaskStateV1::CompletedVerified;
        assert_eq!(
            queue.next_ready_task().map(|task| task.task_id.as_str()),
            Some("b")
        );
        queue.tasks[1].state = AutonomousQueueTaskStateV1::CompletedVerified;
        assert_eq!(
            queue.next_ready_task().map(|task| task.task_id.as_str()),
            Some("c")
        );
        queue.tasks[2].state = AutonomousQueueTaskStateV1::CompletedVerified;
        assert_eq!(
            queue.next_ready_task().map(|task| task.task_id.as_str()),
            Some("d")
        );
        assert!(
            AutonomousQueueV1::new(vec![
                AutonomousQueueTaskV1 {
                    task_id: "a".into(),
                    priority: 1,
                    depends_on: vec!["b".into()],
                    state: AutonomousQueueTaskStateV1::Ready
                },
                AutonomousQueueTaskV1 {
                    task_id: "b".into(),
                    priority: 1,
                    depends_on: vec!["a".into()],
                    state: AutonomousQueueTaskStateV1::Ready
                },
            ])
            .is_err()
        );
    }

    #[test]
    fn planner_gate_satisfaction_is_task_scoped_and_durable() {
        let (_root, store) = store("planner-gate");
        store
            .create_session("session-one", queue())
            .expect("session");
        assert!(
            !store
                .planner_gate_satisfied("session-one", "analysis")
                .expect("initial")
        );
        store
            .satisfy_planner_gate("session-one", "analysis")
            .expect("satisfy");
        assert!(
            store
                .planner_gate_satisfied("session-one", "analysis")
                .expect("saved")
        );
        assert!(
            !store
                .planner_gate_satisfied("session-one", "repair")
                .expect("task scoped")
        );
    }

    #[test]
    fn queue_rejects_duplicate_and_missing_dependencies() {
        let duplicate = AutonomousQueueV1::new(vec![
            AutonomousQueueTaskV1 {
                task_id: "same".into(),
                priority: 1,
                depends_on: Vec::new(),
                state: AutonomousQueueTaskStateV1::Planned,
            },
            AutonomousQueueTaskV1 {
                task_id: "same".into(),
                priority: 2,
                depends_on: Vec::new(),
                state: AutonomousQueueTaskStateV1::Planned,
            },
        ]);
        assert!(duplicate.is_err());
        let missing = AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
            task_id: "task".into(),
            priority: 1,
            depends_on: vec!["missing".into()],
            state: AutonomousQueueTaskStateV1::Planned,
        }]);
        assert!(missing.is_err());
    }

    #[test]
    fn review_inbox_is_project_scoped_idempotent_and_acknowledgeable() {
        let (_root, store) = store("review-inbox");
        store
            .create_session("session-one", queue())
            .expect("session");
        let first = store
            .emit_review_inbox_record(
                "session-one",
                "project-one",
                AutonomousSessionStateV1::WaitingForChatgpt,
                "chatgpt_decision_required",
                "artifacts/escalation.json",
                10,
            )
            .expect("first record");
        let replay = store
            .emit_review_inbox_record(
                "session-one",
                "project-one",
                AutonomousSessionStateV1::WaitingForChatgpt,
                "chatgpt_decision_required",
                "artifacts/escalation.json",
                11,
            )
            .expect("replay");
        assert_eq!(first, replay);
        assert_eq!(
            store
                .list_review_inbox(Some("project-one"), None, 10)
                .expect("list")
                .len(),
            1
        );
        assert!(
            store
                .list_review_inbox(Some("other-project"), None, 10)
                .expect("isolated list")
                .is_empty()
        );
        assert!(
            !store
                .acknowledge_review_inbox(&first.record_id)
                .expect("ack")
                .unread
        );
        assert!(
            !store
                .acknowledge_review_inbox(&first.record_id)
                .expect("idempotent ack")
                .unread
        );
    }

    #[test]
    fn review_inbox_accepts_record_beyond_legacy_512_boundary() {
        let (root, store) = store("review-inbox-capacity");
        store
            .create_session("session-capacity", queue())
            .expect("session");

        let seeded = (0..512)
            .map(|index| AutonomousReviewInboxRecordV1 {
                schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
                record_id: format!("review-seed-{index:04}"),
                project_id: "project-one".into(),
                session_id: "session-capacity".into(),
                state: AutonomousSessionStateV1::CompletedVerified,
                next_action: "independent_final_review".into(),
                reference: "artifacts/completion.json".into(),
                created_at_unix: index + 1,
                unread: false,
            })
            .collect::<Vec<_>>();
        write_json_atomic(&root.join("review-inbox.json"), &seeded).expect("seed inbox");

        let appended = store
            .emit_review_inbox_record(
                "session-capacity",
                "project-one",
                AutonomousSessionStateV1::CompletedVerified,
                "independent_final_review",
                "artifacts/completion.json",
                1_000,
            )
            .expect("513th record");

        assert_eq!(store.all_review_inbox().expect("full inbox").len(), 513);
        assert_eq!(appended.project_id, "project-one");
        assert_eq!(appended.session_id, "session-capacity");
        assert!(appended.unread);
    }

    #[test]
    fn review_inbox_auto_compacts_old_acknowledged_records_and_preserves_unread() {
        let (root, store) = store("review-inbox-auto-compact");
        store
            .create_session("session-capacity", queue())
            .expect("session");

        let acknowledged_seed = 1_024usize;
        let seeded = (0..AUTONOMY_REVIEW_INBOX_COMPACT_TRIGGER)
            .map(|index| AutonomousReviewInboxRecordV1 {
                schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
                record_id: format!("review-seed-{index:04}"),
                project_id: "project-one".into(),
                session_id: "session-capacity".into(),
                state: AutonomousSessionStateV1::CompletedVerified,
                next_action: "independent_final_review".into(),
                reference: "artifacts/completion.json".into(),
                created_at_unix: index as u64 + 1,
                unread: index >= acknowledged_seed,
            })
            .collect::<Vec<_>>();
        write_json_atomic(&root.join("review-inbox.json"), &seeded).expect("seed inbox");

        let appended = store
            .emit_review_inbox_record(
                "session-capacity",
                "project-one",
                AutonomousSessionStateV1::CompletedVerified,
                "independent_final_review",
                "artifacts/completion.json",
                10_000,
            )
            .expect("append after auto compaction");

        let records = store.all_review_inbox().expect("compacted inbox");
        assert_eq!(records.len(), AUTONOMY_REVIEW_INBOX_COMPACT_TARGET + 1);
        assert_eq!(
            records.iter().filter(|record| record.unread).count(),
            (AUTONOMY_REVIEW_INBOX_COMPACT_TRIGGER - acknowledged_seed) + 1
        );
        assert!(
            records
                .iter()
                .all(|record| record.record_id != "review-seed-0000")
        );
        assert!(
            records
                .iter()
                .any(|record| record.record_id == "review-seed-0512")
        );
        assert_eq!(records.last(), Some(&appended));
        assert!(appended.unread);
    }

    #[test]
    fn delegated_review_inbox_is_idempotent_without_autonomous_session() {
        let (_root, store) = store("delegated-review-inbox");
        let run_id = "T-0218.R6:canary";
        let reference = "delegated-run=T-0218.R6:canary;final-review-sha256=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef;diff=sha256:abc;target-sha256=def";
        let first = store
            .emit_delegated_review_inbox_record(run_id, "project-one", reference, 10)
            .expect("delegated review record");
        let replay = store
            .emit_delegated_review_inbox_record(run_id, "project-one", reference, 20)
            .expect("idempotent replay");

        assert_eq!(first, replay);
        assert_eq!(first.record_id, "review-T-0218_R6_canary");
        assert_eq!(first.session_id, "T-0218_R6_canary");
        assert_eq!(first.project_id, "project-one");
        assert_eq!(first.state, AutonomousSessionStateV1::CompletedVerified);
        assert_eq!(first.next_action, "independent_final_review");
        assert!(first.unread);
        assert_eq!(
            store
                .list_review_inbox(Some("project-one"), None, 10)
                .expect("list delegated review")
                .len(),
            1
        );
        assert!(
            store
                .list_review_inbox(Some("other-project"), None, 10)
                .expect("project isolation")
                .is_empty()
        );
        assert!(
            store
                .emit_delegated_review_inbox_record(run_id, "other-project", reference, 30,)
                .is_err(),
            "same delegated run cannot be rebound to another project"
        );
    }

    #[test]
    fn planner_metadata_and_reset_aware_telemetry_survive_restart() {
        let (_root, store) = store("plan-telemetry");
        store
            .create_session("session-one", queue())
            .expect("session");
        let plan = AutonomousPlanQueueV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            tasks: vec![
                AutonomousPlannedTaskV1 {
                    task_id: "analysis".into(),
                    depends_on: Vec::new(),
                    acceptance_criteria: vec!["review evidence".into()],
                    allowed_paths: vec!["src".into()],
                    verification_profile: "cargo".into(),
                    preferred_worker_policy: "codex-preferred".into(),
                    escalation_conditions: vec!["scope ambiguity".into()],
                    completion_artifact_ids: Vec::new(),
                    provider_provenance: vec!["codex-cli".into()],
                },
                AutonomousPlannedTaskV1 {
                    task_id: "repair".into(),
                    depends_on: vec!["analysis".into()],
                    acceptance_criteria: vec!["repair evidence".into()],
                    allowed_paths: vec!["src".into()],
                    verification_profile: "cargo".into(),
                    preferred_worker_policy: "qwen-eligible-after-reset".into(),
                    escalation_conditions: vec!["architecture ambiguity".into()],
                    completion_artifact_ids: Vec::new(),
                    provider_provenance: Vec::new(),
                },
            ],
        };
        store.write_plan_queue("session-one", &plan).expect("plan");
        assert_eq!(store.load_plan_queue("session-one").expect("reload"), plan);
        store
            .record_codex_routing_telemetry(
                "session-one",
                CodexRoutingTelemetryV1 {
                    observed_at_unix: 100,
                    source: "codex-app-server/account-rateLimits-read".into(),
                    rate_limit_windows: Vec::new(),
                    reached_limit: true,
                    codex_eligible_after_unix: None,
                    ..Default::default()
                },
                60,
            )
            .expect("telemetry");
        assert_eq!(
            store
                .load_session("session-one")
                .expect("reload")
                .codex_eligible_after_unix,
            None
        );
    }

    #[test]
    fn state_events_and_cursor_round_trip_durably() {
        let (_root, store) = store("events");
        store
            .create_session("session-one", queue())
            .expect("session");
        let first = store
            .append_event("session-one", "session_created", "session created")
            .expect("first event");
        let second = store
            .append_event("session-one", "state_queued", "session queued")
            .expect("second event");
        assert_eq!((first.sequence, second.sequence), (1, 2));
        let events = store.poll_events("session-one", 1).expect("poll");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, 2);
        assert_eq!(
            store
                .load_session("session-one")
                .expect("load")
                .last_event_sequence,
            2
        );
    }

    #[test]
    fn escalation_packet_is_bounded_and_round_trips() {
        let (_root, store) = store("escalation");
        store
            .create_session("session-one", queue())
            .expect("session");
        let packet = store
            .write_escalation(
                "session-one",
                AutonomousSessionStateV1::WaitingForChatgpt,
                "provider_terminal_error",
            )
            .expect("packet");
        assert_eq!(packet.reason, "provider_terminal_error");
        assert_eq!(store.load_escalation("session-one").expect("read"), packet);
        assert!(
            store
                .write_escalation(
                    "session-one",
                    AutonomousSessionStateV1::WaitingForChatgpt,
                    "contains spaces",
                )
                .is_err()
        );
    }

    #[test]
    fn planner_reply_and_completion_artifacts_round_trip_durably() {
        let (_root, store) = store("artifacts");
        store
            .create_session("session-one", queue())
            .expect("session");
        let escalation = store
            .write_escalation(
                "session-one",
                AutonomousSessionStateV1::WaitingForChatgpt,
                "provider_terminal_error",
            )
            .expect("escalation");
        let constraints = vec!["preserve policy".into()];
        store
            .write_planner_reply(
                "session-one",
                &escalation.escalation_id,
                "decision-hash",
                "select the bounded repair",
                &constraints,
            )
            .expect("planner reply");
        assert!(
            !store
                .load_planner_reply("session-one")
                .expect("reply")
                .consumed
        );
        store
            .mark_planner_reply_consumed("session-one")
            .expect("consume reply");
        assert!(
            store
                .load_planner_reply("session-one")
                .expect("consumed reply")
                .consumed
        );

        let verification = VerificationSummaryV1 {
            status: super::super::coordinator::VerificationStatusV1::Passed,
            command: "cargo test".into(),
            summary: "passed".into(),
        };
        store
            .write_completion_artifacts(
                "session-one",
                &verification,
                "diff --git a/src/lib.rs b/src/lib.rs",
                "independent final review passed",
            )
            .expect("completion artifacts");
        let artifacts = store
            .load_completion_artifacts("session-one")
            .expect("load completion artifacts");
        assert_eq!(artifacts.verification, verification);
        assert!(artifacts.authoritative_diff.starts_with("diff --git"));
    }

    #[test]
    fn task_output_baseline_is_durable_and_immutable_per_task() {
        let (_root, store) = store("task-output-baseline");
        store
            .create_session("session-one", queue())
            .expect("session");
        let baseline = AutonomousTaskOutputBaselineV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            session_id: "session-one".into(),
            task_id: "analysis".into(),
            approved_contract_hash: "fnv1a64:0123456789abcdef".into(),
            completion_artifact_ids: vec!["src/output.txt".into()],
            observations: vec![AutonomousTaskOutputObservationV1 {
                artifact_id: "src/output.txt".into(),
                state: "ABSENT".into(),
                sha256: None,
            }],
        };
        store
            .save_task_output_baseline("session-one", &baseline)
            .expect("save baseline");
        assert_eq!(
            store
                .load_task_output_baseline("session-one", "analysis")
                .expect("reload")
                .expect("baseline"),
            baseline
        );
        let fresh_task_baseline = AutonomousTaskOutputBaselineV1 {
            task_id: "repair".into(),
            completion_artifact_ids: vec!["src/repair-output.txt".into()],
            observations: vec![AutonomousTaskOutputObservationV1 {
                artifact_id: "src/repair-output.txt".into(),
                state: "ABSENT".into(),
                sha256: None,
            }],
            ..baseline.clone()
        };
        store
            .save_task_output_baseline("session-one", &fresh_task_baseline)
            .expect("fresh DAG task baseline");
        assert_eq!(
            store
                .load_task_output_baseline("session-one", "repair")
                .expect("reload fresh task")
                .expect("fresh baseline"),
            fresh_task_baseline
        );
        let mut drifted = baseline;
        drifted.observations[0].state = "PRESENT_SHA256".into();
        drifted.observations[0].sha256 = Some("a".repeat(64));
        assert!(
            store
                .save_task_output_baseline("session-one", &drifted)
                .is_err()
        );
    }

    #[test]
    fn provider_handoff_round_trips_and_rejects_secret_markers() {
        let (_root, store) = store("provider-handoff");
        store
            .create_session("session-one", queue())
            .expect("session");
        let handoff = AutonomousProviderHandoffV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            session_id: "session-one".into(),
            task_id: "analysis".into(),
            from_provider_id: "codex-cli".into(),
            to_provider_id: "ollama".into(),
            reason: "codex_credits_exhausted".into(),
            created_at_unix: 10,
            objective: "continue approved work".into(),
            ordered_steps: vec!["inspect".into()],
            acceptance_criteria: vec!["tests pass".into()],
            allowed_paths: vec!["src".into()],
            forbidden_paths: vec![".git".into()],
            base_branch: "main".into(),
            feature_branch: "task-1".into(),
            base_commit: "1234567".into(),
            expected_origin: "https://example.invalid/repo.git".into(),
            codex_thread_id: Some("opaque-thread".into()),
            last_verification_summary: None,
            completed_task_ids: vec![],
            pending_task_id: "analysis".into(),
            remaining_provider_turns: 2,
            remaining_repair_cycles: 1,
            changed_files: vec!["src/lib.rs".into()],
            git_status_porcelain: vec![" M src/lib.rs".into()],
            authoritative_diff_hash: None,
            completed_tool_calls: Vec::new(),
            pending_tool_calls: Vec::new(),
        };
        store
            .write_provider_handoff("session-one", &handoff)
            .expect("write");
        assert_eq!(
            store.load_provider_handoff("session-one").expect("load"),
            handoff
        );
        let mut unsafe_handoff = handoff;
        unsafe_handoff.changed_files = vec!["token=must-not-persist".into()];
        assert!(
            store
                .write_provider_handoff("session-one", &unsafe_handoff)
                .is_err()
        );
    }

    #[test]
    fn handoff_persists_completed_mutation_without_replayable_arguments() {
        let (_root, store) = store("handoff-provenance");
        store
            .create_session("session-one", queue())
            .expect("session");
        let handoff = AutonomousProviderHandoffV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            session_id: "session-one".into(),
            task_id: "analysis".into(),
            from_provider_id: "codex-cli".into(),
            to_provider_id: "ollama".into(),
            reason: "codex_credits_exhausted".into(),
            created_at_unix: 1,
            objective: "continue".into(),
            ordered_steps: vec!["verify".into()],
            acceptance_criteria: vec!["pass".into()],
            allowed_paths: vec!["src".into()],
            forbidden_paths: vec![".git".into()],
            base_branch: "main".into(),
            feature_branch: "feature".into(),
            base_commit: "abc".into(),
            expected_origin: "https://example.invalid/repo".into(),
            codex_thread_id: None,
            last_verification_summary: None,
            completed_task_ids: Vec::new(),
            pending_task_id: "analysis".into(),
            remaining_provider_turns: 1,
            remaining_repair_cycles: 1,
            changed_files: vec!["src/lib.rs".into()],
            git_status_porcelain: vec![" M src/lib.rs".into()],
            authoritative_diff_hash: Some("sha256:abc".into()),
            completed_tool_calls: vec![AutonomousToolCallProvenanceV1 {
                tool_call_id: "apply-1".into(),
                tool_name: "patch.apply".into(),
                mutation_kind: "MUTATING".into(),
                status: "COMPLETED".into(),
                request_hash: "fnv1a64:request".into(),
                result_hash: Some("fnv1a64:result".into()),
                outcome_summary: Some("patch applied".into()),
            }],
            pending_tool_calls: vec![AutonomousToolCallProvenanceV1 {
                tool_call_id: "read-2".into(),
                tool_name: "read".into(),
                mutation_kind: "READ_ONLY".into(),
                status: "EXECUTING".into(),
                request_hash: "fnv1a64:read".into(),
                result_hash: None,
                outcome_summary: None,
            }],
        };
        store
            .write_provider_handoff("session-one", &handoff)
            .expect("persist");
        let reloaded = store.load_provider_handoff("session-one").expect("reload");
        assert_eq!(reloaded.completed_tool_calls[0].tool_call_id, "apply-1");
        assert_eq!(reloaded.pending_tool_calls[0].tool_call_id, "read-2");
        assert!(
            !serde_json::to_string(&reloaded)
                .expect("json")
                .contains("arguments")
        );
    }

    #[test]
    fn provider_handoff_is_bounded_redacted_and_survives_reload() {
        let (_root, store) = store("handoff");
        store
            .create_session("session-one", queue())
            .expect("session");
        let handoff = AutonomousProviderHandoffV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            session_id: "session-one".into(),
            task_id: "analysis".into(),
            from_provider_id: "codex-cli".into(),
            to_provider_id: "ollama".into(),
            reason: "codex_credits_exhausted".into(),
            created_at_unix: 12,
            objective: "continue the approved task".into(),
            ordered_steps: vec!["implement continuation".into()],
            acceptance_criteria: vec!["do not replay complete work".into()],
            allowed_paths: vec!["src".into()],
            forbidden_paths: vec![".git".into()],
            base_branch: "main".into(),
            feature_branch: "feature".into(),
            base_commit: "1234567".into(),
            expected_origin: "https://example.invalid/repo.git".into(),
            codex_thread_id: Some("opaque-thread".into()),
            last_verification_summary: None,
            completed_task_ids: vec!["repair".into()],
            pending_task_id: "analysis".into(),
            remaining_provider_turns: 2,
            remaining_repair_cycles: 1,
            changed_files: vec!["src/lib.rs".into()],
            git_status_porcelain: vec![" M src/lib.rs".into()],
            authoritative_diff_hash: Some("sha256:abc".into()),
            completed_tool_calls: Vec::new(),
            pending_tool_calls: Vec::new(),
        };
        store
            .write_provider_handoff("session-one", &handoff)
            .expect("handoff writes");
        assert_eq!(
            store
                .load_provider_handoff("session-one")
                .expect("handoff reloads"),
            handoff
        );
        let mut secret = handoff;
        secret.last_verification_summary = Some("token=must-not-persist".into());
        assert!(
            store
                .write_provider_handoff("session-one", &secret)
                .is_err()
        );
    }

    #[test]
    fn torn_terminal_event_is_repaired_without_losing_prior_events() {
        let (_root, store) = store("torn-tail");
        store
            .create_session("session-one", queue())
            .expect("session");
        store
            .append_event("session-one", "session_created", "session created")
            .expect("event");
        let events_path = store
            .session_dir("session-one")
            .expect("path")
            .join("events.jsonl");
        let mut file = OpenOptions::new()
            .append(true)
            .open(&events_path)
            .expect("events open");
        file.write_all(b"{not-json").expect("torn tail");
        drop(file);
        let events = store.poll_events("session-one", 0).expect("repaired poll");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn lock_prevents_second_owner_and_release_is_owner_scoped() {
        let (_root, store) = store("locks");
        store
            .create_session("session-one", queue())
            .expect("session");
        let lock = store.acquire_lock("session-one", "owner-a").expect("lock");
        assert!(matches!(
            store.acquire_lock("session-one", "owner-b"),
            Err(AutonomyStateError::LockHeld(_))
        ));
        lock.release().expect("release");
        store
            .acquire_lock("session-one", "owner-b")
            .expect("second owner");
    }

    #[test]
    fn restart_recovery_never_relaunches_inflight_work() {
        let (_root, store) = store("restart");
        let mut snapshot = store
            .create_session("session-one", queue())
            .expect("session");
        snapshot.state = AutonomousSessionStateV1::Running;
        snapshot.current_task_id = Some("analysis".into());
        store.save_session(&snapshot).expect("save running");
        let recovered = store.recover_interrupted_sessions().expect("recover");
        assert_eq!(recovered.len(), 1);
        assert_eq!(
            recovered[0].state,
            AutonomousSessionStateV1::RecoveringAfterRestart
        );
        assert_eq!(
            store.poll_events("session-one", 0).expect("events")[0].kind,
            "restart_recovery_required"
        );
    }

    #[test]
    fn terminal_state_and_active_flag_must_agree() {
        let (_root, store) = store("state-validation");
        let mut snapshot = store
            .create_session("session-one", queue())
            .expect("session");
        snapshot.state = AutonomousSessionStateV1::CompletedVerified;
        assert!(store.save_session(&snapshot).is_err());
        snapshot.active = false;
        store.save_session(&snapshot).expect("terminal state saves");
    }

    #[test]
    fn same_thread_continuity_evidence_is_bounded_and_bound_to_session_thread() {
        let (_root, store) = store("continuity-evidence");
        let mut snapshot = store
            .create_session("session-one", queue())
            .expect("session");
        snapshot.provider_thread_id = Some("thread-one".into());
        snapshot.codex_continuity = Some(CodexContinuityEvidenceV1 {
            thread_id: "thread-one".into(),
            first_turn_id: "turn-one".into(),
            first_completed: true,
            second_turn_id: Some("turn-two".into()),
            second_completed: false,
            observed_at_unix: 1,
        });
        store.save_session(&snapshot).expect("persist evidence");
        snapshot
            .codex_continuity
            .as_mut()
            .expect("evidence")
            .thread_id = "other-thread".into();
        assert!(store.save_session(&snapshot).is_err());
    }

    #[test]
    fn canonical_thread_and_continuity_history_survive_durable_session_reload() {
        let (root, store) = store("gui-cli-continuity-reload");
        let mut snapshot = store
            .create_session("session-one", queue())
            .expect("session");
        snapshot.provider_thread_id = Some("existing-gui-visible-thread".into());
        snapshot.expected_codex_thread_id = Some("existing-gui-visible-thread".into());
        snapshot.codex_continuity = Some(CodexContinuityEvidenceV1 {
            thread_id: "existing-gui-visible-thread".into(),
            first_turn_id: "continuity-turn-one".into(),
            first_completed: true,
            second_turn_id: Some("continuity-turn-two".into()),
            second_completed: true,
            observed_at_unix: 42,
        });
        store.save_session(&snapshot).expect("durable binding");

        let reloaded_store = AutonomousStateStoreV1::open(root).expect("reopen state store");
        let reloaded = reloaded_store
            .load_session("session-one")
            .expect("reload session");
        assert_eq!(
            reloaded.provider_thread_id.as_deref(),
            Some("existing-gui-visible-thread")
        );
        assert_eq!(
            reloaded.expected_codex_thread_id.as_deref(),
            Some("existing-gui-visible-thread")
        );
        assert_eq!(reloaded.codex_continuity, snapshot.codex_continuity);
        let history = serde_json::to_value(&reloaded).expect("bounded history serialization");
        assert_eq!(history["providerThreadId"], "existing-gui-visible-thread");
        assert_eq!(
            history["expectedCodexThreadId"],
            "existing-gui-visible-thread"
        );
        assert_eq!(
            history["codexContinuity"]["firstTurnId"],
            "continuity-turn-one"
        );
        assert_eq!(
            history["codexContinuity"]["secondTurnId"],
            "continuity-turn-two"
        );
    }
}
