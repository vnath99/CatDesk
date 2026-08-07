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
use uuid::Uuid;

use super::autonomous_contract::AutonomousDevelopmentContractV1;
use super::coordinator::VerificationSummaryV1;
use super::runtime::RuntimeError;

pub const AUTONOMY_STATE_SCHEMA_VERSION: u32 = 1;

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
    Running,
    Verifying,
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
    #[serde(default)]
    pub cancellation_requested: bool,
    #[serde(default)]
    pub approved_contract_hash: Option<String>,
    #[serde(default)]
    pub consumed_idempotency_keys: BTreeMap<String, String>,
    pub last_event_sequence: u64,
    pub active: bool,
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
            for dependency in &task.depends_on {
                validate_slug(dependency, "autonomous queue dependency")?;
                if !ids.contains(dependency.as_str()) {
                    return Err(RuntimeError::Validation(
                        "autonomous queue dependency does not exist".into(),
                    ));
                }
            }
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
            provider_handle_id: None,
            provider_event_cursor: 0,
            repair_attempts: 0,
            last_verification_summary: None,
            provider_turn_count: 0,
            retry_not_before_unix: None,
            rate_limited_since_unix: None,
            cancellation_requested: false,
            approved_contract_hash: None,
            consumed_idempotency_keys: BTreeMap::new(),
            last_event_sequence: 0,
            active: true,
        };
        write_json_atomic(&session_dir.join("queue.json"), &queue)?;
        write_json_atomic(&session_dir.join("state.json"), &snapshot)?;
        Ok(snapshot)
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
        write_json_atomic(
            &self
                .session_dir(session_id)?
                .join("artifacts")
                .join("completion.json"),
            &artifacts,
        )
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
    Ok(())
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
}
