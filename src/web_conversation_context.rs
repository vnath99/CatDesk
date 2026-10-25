//! Advisory, project-scoped ChatGPT Web context observations.
//!
//! The caller supplies browser-visible message/tool text and a separately
//! read-back project target. This store cannot authenticate a ChatGPT caller,
//! know provider-managed context, or authorize a browser/Wake action. It keeps
//! hashes and token counts, never transcripts. A replacement conversation is
//! bound here only after the caller verifies its real URL and Wake authority.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use tiktoken_rs::o200k_base_singleton;
use uuid::Uuid;

use crate::delegated::autonomy_projects::canonical_project_chat_target;

const SCHEMA_VERSION: u32 = 1;
const MAX_PROJECT_BYTES: usize = 64 * 1024;
const MAX_CONVERSATION_BYTES: usize = 2 * 1024 * 1024;
const MAX_VISIBLE_TEXT_BYTES: usize = 256 * 1024;
const MAX_EVENTS: usize = 4096;
const MAX_HISTORY: usize = 64;

/// A local operator setting, not a claim about the provider's context window.
/// The 24k reserve leaves space for hidden prompts, tools, and uncertainty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextPressurePolicy {
    pub configured_budget_tokens: u64,
    pub safety_headroom_tokens: u64,
    pub unknown_source_reserve_tokens: u64,
    /// Early advisory warning, independently configurable below the rollover
    /// trigger. Omission preserves legacy policy bytes and uses 80% of trigger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning_threshold_tokens: Option<u64>,
    pub automatic_rollover_enabled: bool,
    /// Explicit consent for periodic browser reads when rollover is disabled.
    #[serde(default)]
    pub automatic_observation_enabled: bool,
    /// Allow an advisory rollover request after an incomplete browser capture.
    /// This remains opt-in even when automatic rollover is enabled.
    #[serde(default)]
    pub incomplete_history_rollover: bool,
}

impl Default for ContextPressurePolicy {
    fn default() -> Self {
        Self {
            configured_budget_tokens: 96_000,
            safety_headroom_tokens: 24_000,
            unknown_source_reserve_tokens: 8_000,
            warning_threshold_tokens: None,
            automatic_rollover_enabled: false,
            automatic_observation_enabled: false,
            incomplete_history_rollover: false,
        }
    }
}

impl ContextPressurePolicy {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.configured_budget_tokens == 0
            || self.configured_budget_tokens > 2_000_000
            || self.safety_headroom_tokens >= self.configured_budget_tokens
            || self.unknown_source_reserve_tokens > self.configured_budget_tokens
            || self
                .warning_threshold_tokens
                .is_some_and(|tokens| tokens == 0 || tokens > self.trigger_tokens())
        {
            return Err("WEB_CONTEXT_POLICY_INVALID".into());
        }
        Ok(())
    }

    pub fn trigger_tokens(&self) -> u64 {
        self.configured_budget_tokens - self.safety_headroom_tokens
    }

    pub fn warning_tokens(&self) -> u64 {
        self.warning_threshold_tokens
            .unwrap_or_else(|| (self.trigger_tokens() * 4 / 5).max(1))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationKey {
    pub project_id: String,
    pub conversation_url: String,
    pub target_generation: u64,
}

impl ConversationKey {
    fn validate(&self) -> Result<(), String> {
        valid_identifier(&self.project_id)?;
        if self.target_generation == 0
            || canonical_project_chat_target(&self.conversation_url)
                .ok()
                .as_deref()
                != Some(self.conversation_url.as_str())
        {
            return Err("WEB_CONTEXT_TARGET_INVALID".into());
        }
        Ok(())
    }

    pub fn target_sha256(&self) -> String {
        sha256(self.conversation_url.as_bytes())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VisibleSourceKind {
    UserMessage,
    AssistantMessage,
    ToolResult,
    ToolArguments,
    WorkerMessage,
    Handoff,
    UnknownAttachment,
}

impl VisibleSourceKind {
    fn counts_in_context_pressure(self) -> bool {
        matches!(
            self,
            Self::UserMessage
                | Self::AssistantMessage
                | Self::ToolResult
                | Self::ToolArguments
                | Self::WorkerMessage
                | Self::Handoff
        )
    }
}

/// `event_id` identifies one stable message/tool call; a larger revision
/// replaces a streaming snapshot. The caller may give two representations of
/// the same provider-visible content one `dedup_key`; equal text in distinct
/// messages must use distinct keys. Provenance is caller-observed, not trusted.
pub struct Observation<'a> {
    pub event_id: &'a str,
    pub revision: u64,
    pub dedup_key: &'a str,
    pub source_kind: VisibleSourceKind,
    pub visible_text: Option<&'a str>,
    pub observed_at_unix: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EventMetadata {
    event_id: String,
    revision: u64,
    dedup_key: String,
    source_kind: VisibleSourceKind,
    content_sha256: Option<String>,
    visible_bytes: Option<u64>,
    estimated_tokens: Option<u64>,
    observed_at_unix: u64,
    /// Only the counted excerpt was observed, not the full message body.
    /// Missing on older complete-text records; false stays byte-compatible.
    #[serde(default, skip_serializing_if = "is_false")]
    excerpt_only: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RolloverIntent {
    pub intent_id: String,
    pub key: ConversationKey,
    pub target_sha256: String,
    pub source_task_id: String,
    pub estimated_upper_tokens: u64,
    pub trigger_tokens: u64,
    pub requested_at_unix: u64,
    /// The caller reported a quiescent point. The browser controller must
    /// independently recheck idle/ownership before any USER write.
    pub quiescence_evidence: String,
    pub status: String,
    #[serde(default = "observed_pressure_reason")]
    pub trigger_reason: String,
}

fn observed_pressure_reason() -> String {
    "OBSERVED_PRESSURE".into()
}

/// Persistent uncertainty from bounded browser snapshots. A rendered DOM
/// excerpt cannot prove absence of virtualized or older conversation turns.
/// No current browser adapter supplies evidence capable of clearing a gap.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserCoverageState {
    pub observed: bool,
    /// Latest validated browser capture, even when deduplication adds no tokens.
    /// This timestamp does not prove complete history or provider context size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_observed_at_unix: Option<u64>,
    pub initial_history_gap: bool,
    pub identity_gap: bool,
    pub text_gap: bool,
    pub overflow_gap: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct BrowserCoverageObservation {
    pub observed_at_unix: u64,
    pub initial_history_gap: bool,
    pub identity_gap: bool,
    pub text_gap: bool,
    pub overflow_gap: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationLedger {
    schema_version: u32,
    key: ConversationKey,
    events: Vec<EventMetadata>,
    rollover_intent: Option<RolloverIntent>,
    #[serde(default)]
    browser_coverage: BrowserCoverageState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectState {
    schema_version: u32,
    project_id: String,
    policy: ContextPressurePolicy,
    active: Option<ConversationKey>,
    history: Vec<ConversationKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PressureStatus {
    pub key: ConversationKey,
    pub target_sha256: String,
    pub is_current: bool,
    pub recorded_events: usize,
    /// Local metadata capacity, not evidence of provider token consumption.
    /// Derived from retained events; older/unsaturated status shapes stay valid.
    #[serde(default, skip_serializing_if = "is_false")]
    pub observation_limit_reached: bool,
    /// Latest retained event observation timestamp, not a poll time, worker
    /// heartbeat or verified wall clock. Exact-content retries keep their
    /// original timestamp; each conversation is derived independently.
    #[serde(default)]
    pub last_observed_event_at_unix: Option<u64>,
    #[serde(default)]
    pub excerpt_only_messages: usize,
    /// Deduplicated observed model-visible content, including structured tool
    /// arguments and worker messages. The field name remains wire-compatible.
    pub visible_unique_tokens: u64,
    /// Observed content only. Older unseen history has no finite bound.
    #[serde(default)]
    pub observed_estimate_tokens: u64,
    pub unknown_source_count: usize,
    pub estimated_upper_tokens: u64,
    pub observed_tool_argument_tokens: u64,
    pub observed_tool_result_tokens: u64,
    pub configured_budget_tokens: u64,
    pub safety_headroom_tokens: u64,
    pub trigger_tokens: u64,
    pub near_capacity: bool,
    #[serde(default)]
    pub warning_tokens: u64,
    /// A threshold on observed content, not a claim about hidden context.
    #[serde(default)]
    pub warning_reached: bool,
    #[serde(default)]
    pub coverage_known: bool,
    #[serde(default)]
    pub browser_coverage: BrowserCoverageState,
    #[serde(default)]
    pub pressure_assessment: String,
    pub automatic_rollover_enabled: bool,
    #[serde(default)]
    pub automatic_observation_enabled: bool,
    #[serde(default)]
    pub incomplete_history_rollover: bool,
    pub rollover_intent: Option<RolloverIntent>,
    pub estimate_method: String,
    pub advisory: bool,
    pub origin_authenticated: bool,
}

#[derive(Clone, Debug)]
pub struct WebContextStore {
    root: PathBuf,
    read_only: bool,
    nonblocking: bool,
}

impl WebContextStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, String> {
        let root = root.as_ref();
        checked_directory(root, true)?;
        let root = root.canonicalize().map_err(io_error)?;
        for child in ["projects", "conversations"] {
            let path = root.join(child);
            checked_directory(&path, true)?;
        }
        Ok(Self {
            root,
            read_only: false,
            nonblocking: false,
        })
    }

    /// Online requests and cancellable workers must not wait for a separate
    /// ledger writer. Refuse contention so the caller can retry its stable
    /// event later and the worker can unwind without abandoning its lease.
    pub fn open_nonblocking(root: impl AsRef<Path>) -> Result<Self, String> {
        let mut store = Self::open(root)?;
        store.nonblocking = true;
        Ok(store)
    }

    pub fn open_read_only(root: impl AsRef<Path>) -> Result<Self, String> {
        let root = root.as_ref();
        checked_directory(root, false)?;
        let root = root.canonicalize().map_err(io_error)?;
        for child in ["projects", "conversations"] {
            checked_directory(&root.join(child), false)?;
        }
        Ok(Self {
            root,
            read_only: true,
            nonblocking: true,
        })
    }

    pub fn configure_policy(
        &self,
        project_id: &str,
        policy: ContextPressurePolicy,
    ) -> Result<(), String> {
        valid_identifier(project_id)?;
        policy.validate()?;
        let _lock = self.lock()?;
        let mut project = self.read_project_or_default(project_id)?;
        project.policy = policy;
        self.write_project(&project)
    }

    /// Mirrors a separately verified registry/Wake target. This does not
    /// itself update the authoritative project binding or grant delivery.
    pub fn bind_observed_target(
        &self,
        key: ConversationKey,
        expected_previous: Option<&ConversationKey>,
    ) -> Result<(), String> {
        key.validate()?;
        let _lock = self.lock()?;
        let mut project = self.read_project_or_default(&key.project_id)?;
        if project.active.as_ref() == Some(&key) {
            self.read_ledger(&key)?;
            return Ok(());
        }
        if project.active.as_ref() != expected_previous
            || project
                .active
                .as_ref()
                .is_some_and(|old| key.target_generation <= old.target_generation)
            || project.history.len() >= MAX_HISTORY
        {
            return Err("WEB_CONTEXT_TARGET_CAS_CONFLICT".into());
        }
        self.reject_cross_project_url(&key)?;
        let ledger = ConversationLedger {
            schema_version: SCHEMA_VERSION,
            key: key.clone(),
            events: Vec::new(),
            rollover_intent: None,
            browser_coverage: BrowserCoverageState::default(),
        };
        if self.ledger_path(&key).exists() {
            let existing = self.read_ledger(&key)?;
            if !existing.events.is_empty()
                || existing.rollover_intent.is_some()
                || existing.browser_coverage.observed
            {
                return Err("WEB_CONTEXT_TARGET_ALREADY_RECORDED".into());
            }
        } else {
            self.write_ledger(&ledger)?;
        }
        project.history.push(key.clone());
        project.active = Some(key);
        self.write_project(&project)
    }

    /// Records only model-visible text supplied by the caller, including
    /// observed tool arguments. Transport totals are not silently attributed.
    pub fn record_observation(
        &self,
        key: &ConversationKey,
        observation: Observation<'_>,
    ) -> Result<PressureStatus, String> {
        key.validate()?;
        let event = observation_metadata(observation)?;
        let _lock = self.lock()?;
        let project = self.read_project(&key.project_id)?;
        if project.active.as_ref() != Some(key) {
            return Err("WEB_CONTEXT_OLD_GENERATION".into());
        }
        let mut ledger = self.read_ledger(key)?;
        self.record_event_locked(&project, &mut ledger, event)
    }

    /// Validate and apply a bounded import in memory, then replace its one
    /// conversation ledger once. A late invalid or conflicting revision cannot
    /// leave an earlier event counted in a rejected batch.
    pub(crate) fn record_observation_batch<'a>(
        &self,
        key: &ConversationKey,
        observations: impl IntoIterator<Item = Observation<'a>>,
    ) -> Result<PressureStatus, String> {
        self.record_batch(key, observations, false)
    }

    /// Finish accounting for an already admitted project-connection call on
    /// its original conversation, even if rollover completed during the call.
    /// This cannot create, bind or reactivate a conversation. Ordinary caller
    /// observations still require the active generation above.
    pub(crate) fn record_existing_conversation_batch<'a>(
        &self,
        key: &ConversationKey,
        observations: impl IntoIterator<Item = Observation<'a>>,
    ) -> Result<PressureStatus, String> {
        self.record_batch(key, observations, true)
    }

    fn record_batch<'a>(
        &self,
        key: &ConversationKey,
        observations: impl IntoIterator<Item = Observation<'a>>,
        allow_known_history: bool,
    ) -> Result<PressureStatus, String> {
        key.validate()?;
        let events = observations
            .into_iter()
            .take(513)
            .map(observation_metadata)
            .collect::<Result<Vec<_>, _>>()?;
        if events.is_empty() || events.len() > 512 {
            return Err("WEB_CONTEXT_INPUT_SCOPE_INVALID".into());
        }
        let _lock = self.lock()?;
        let project = self.read_project(&key.project_id)?;
        if project.active.as_ref() != Some(key)
            && !(allow_known_history && project.history.contains(key))
        {
            return Err("WEB_CONTEXT_OLD_GENERATION".into());
        }
        let mut ledger = self.read_ledger(key)?;
        let mut changed = false;
        for event in events {
            changed |= apply_event(&mut ledger, event)?;
        }
        if changed {
            self.write_ledger(&ledger)?;
        }
        Ok(pressure_status(&project, &ledger))
    }

    /// Ingests a browser-visible snapshot with a stable event identity. The
    /// content hash determines whether the existing revision is replayed or
    /// advanced. This remains an advisory estimate; the caller must separately
    /// prove which browser conversation supplied the observation.
    pub fn record_browser_observation(
        &self,
        key: &ConversationKey,
        event_id: &str,
        source_kind: VisibleSourceKind,
        visible_text: Option<&str>,
        observed_at_unix: u64,
    ) -> Result<PressureStatus, String> {
        self.record_browser_text(
            key,
            event_id,
            source_kind,
            visible_text,
            observed_at_unix,
            false,
        )
    }

    /// Count actual text from a clipped, identified message body. The largest
    /// excerpt proxy replaces smaller clipped samples of that same message;
    /// samples are never added together. A later complete body supersedes it.
    pub(crate) fn record_browser_excerpt_observation(
        &self,
        key: &ConversationKey,
        event_id: &str,
        source_kind: VisibleSourceKind,
        visible_text: &str,
        observed_at_unix: u64,
    ) -> Result<PressureStatus, String> {
        if !matches!(
            source_kind,
            VisibleSourceKind::UserMessage | VisibleSourceKind::AssistantMessage
        ) || visible_text.trim().is_empty()
        {
            return Err("WEB_CONTEXT_EVENT_CONTENT_INVALID".into());
        }
        self.record_browser_text(
            key,
            event_id,
            source_kind,
            Some(visible_text),
            observed_at_unix,
            true,
        )
    }

    fn record_browser_text(
        &self,
        key: &ConversationKey,
        event_id: &str,
        source_kind: VisibleSourceKind,
        visible_text: Option<&str>,
        observed_at_unix: u64,
        excerpt_only: bool,
    ) -> Result<PressureStatus, String> {
        key.validate()?;
        let mut event = observation_metadata(Observation {
            event_id,
            revision: 1,
            dedup_key: event_id,
            source_kind,
            visible_text,
            observed_at_unix,
        })?;
        event.excerpt_only = excerpt_only;
        let _lock = self.lock()?;
        let project = self.read_project(&key.project_id)?;
        if project.active.as_ref() != Some(key) {
            return Err("WEB_CONTEXT_OLD_GENERATION".into());
        }
        let mut ledger = self.read_ledger(key)?;
        if let Some(previous) = ledger
            .events
            .iter()
            .find(|previous| previous.event_id == event_id)
        {
            if previous.source_kind != source_kind || previous.dedup_key != event_id {
                return Err("WEB_CONTEXT_EVENT_REVISION_CONFLICT".into());
            }
            if excerpt_only && event.estimated_tokens <= previous.estimated_tokens {
                return Ok(pressure_status(&project, &ledger));
            }
            event.revision = if previous.content_sha256 == event.content_sha256
                && previous.visible_bytes == event.visible_bytes
                && previous.excerpt_only == event.excerpt_only
            {
                previous.revision
            } else {
                previous
                    .revision
                    .checked_add(1)
                    .ok_or("WEB_CONTEXT_EVENT_REVISION_CONFLICT")?
            };
        }
        self.record_event_locked(&project, &mut ledger, event)
    }

    /// Merge uncertainty from a bounded browser excerpt. Until the browser
    /// supplies an independently verified full-history scan, gaps are sticky:
    /// a later virtualized DOM window is not evidence that old history ended.
    pub fn record_browser_coverage(
        &self,
        key: &ConversationKey,
        observation: BrowserCoverageObservation,
    ) -> Result<PressureStatus, String> {
        key.validate()?;
        if !observation.initial_history_gap || observation.observed_at_unix == 0 {
            return Err("WEB_CONTEXT_COVERAGE_UNPROVEN".into());
        }
        let _lock = self.lock()?;
        let project = self.read_project(&key.project_id)?;
        if project.active.as_ref() != Some(key) {
            return Err("WEB_CONTEXT_OLD_GENERATION".into());
        }
        let mut ledger = self.read_ledger(key)?;
        let coverage = &mut ledger.browser_coverage;
        let previous = coverage.clone();
        coverage.observed = true;
        coverage.last_observed_at_unix = Some(
            coverage
                .last_observed_at_unix
                .unwrap_or(0)
                .max(observation.observed_at_unix),
        );
        coverage.initial_history_gap = true;
        coverage.identity_gap |= observation.identity_gap;
        coverage.text_gap |= observation.text_gap;
        coverage.overflow_gap |= observation.overflow_gap;
        if *coverage != previous {
            self.write_ledger(&ledger)?;
        }
        Ok(pressure_status(&project, &ledger))
    }

    /// The caller holds the ledger lock across reading the previous event,
    /// deriving its revision, and writing the replacement.
    fn record_event_locked(
        &self,
        project: &ProjectState,
        ledger: &mut ConversationLedger,
        event: EventMetadata,
    ) -> Result<PressureStatus, String> {
        if apply_event(ledger, event)? {
            self.write_ledger(ledger)?;
        }
        Ok(pressure_status(project, ledger))
    }

    pub fn pressure(&self, key: &ConversationKey) -> Result<PressureStatus, String> {
        key.validate()?;
        let _lock = self.read_lock()?;
        let project = self.read_project(&key.project_id)?;
        if !project.history.contains(key) {
            return Err("WEB_CONTEXT_TARGET_NOT_RECORDED".into());
        }
        Ok(pressure_status(&project, &self.read_ledger(key)?))
    }

    /// Read pressure and the durable one-time history guard together. This
    /// advisory decision neither creates an intent nor proves quiescence;
    /// request_rollover_if_due rechecks it before recording a request.
    pub fn rollover_readiness(
        &self,
        key: &ConversationKey,
    ) -> Result<(PressureStatus, bool), String> {
        key.validate()?;
        let _lock = self.read_lock()?;
        let project = self.read_project(&key.project_id)?;
        if !project.history.contains(key) {
            return Err("WEB_CONTEXT_TARGET_NOT_RECORDED".into());
        }
        let status = pressure_status(&project, &self.read_ledger(key)?);
        let due = self.rollover_due_locked(&project, &status)?;
        Ok((status, due))
    }

    pub fn project_history(&self, project_id: &str) -> Result<Vec<PressureStatus>, String> {
        valid_identifier(project_id)?;
        let _lock = self.read_lock()?;
        let project = self.read_project(project_id)?;
        project
            .history
            .iter()
            .map(|key| {
                self.read_ledger(key)
                    .map(|ledger| pressure_status(&project, &ledger))
            })
            .collect()
    }

    /// Read-only mirror of the target last bound after outside authority
    /// readback. `None` means the meter has no target, not that Wake is absent.
    pub fn active_key(&self, project_id: &str) -> Result<Option<ConversationKey>, String> {
        valid_identifier(project_id)?;
        let _lock = self.read_lock()?;
        Ok(self.read_project_or_default(project_id)?.active)
    }

    /// Read the active key and its pressure under one meter lock. A caller
    /// deciding whether a rollover intent reserves the current conversation
    /// must not combine two independently sampled generations.
    pub fn active_pressure(&self, project_id: &str) -> Result<Option<PressureStatus>, String> {
        valid_identifier(project_id)?;
        let _lock = self.read_lock()?;
        let project = self.read_project_or_default(project_id)?;
        let Some(key) = project.active.as_ref() else {
            return Ok(None);
        };
        Ok(Some(pressure_status(&project, &self.read_ledger(key)?)))
    }

    /// Creates at most one advisory request for this exact conversation. The
    /// caller-reported quiescent point is a prerequisite, not trusted proof.
    pub fn request_rollover_if_due(
        &self,
        key: &ConversationKey,
        observed_target_sha256: &str,
        source_task_id: &str,
        quiescent: bool,
        now_unix: u64,
    ) -> Result<Option<RolloverIntent>, String> {
        key.validate()?;
        valid_identifier(source_task_id)?;
        if now_unix == 0 || observed_target_sha256 != key.target_sha256() {
            return Err("WEB_CONTEXT_ROLLOVER_TARGET_MISMATCH".into());
        }
        let _lock = self.lock()?;
        let project = self.read_project(&key.project_id)?;
        if project.active.as_ref() != Some(key) {
            return Err("WEB_CONTEXT_OLD_GENERATION".into());
        }
        let mut ledger = self.read_ledger(key)?;
        let status = pressure_status(&project, &ledger);
        if !self.rollover_due_locked(&project, &status)? || !quiescent {
            return Ok(None);
        }
        if let Some(existing) = &ledger.rollover_intent {
            return Ok(Some(existing.clone()));
        }
        let intent = RolloverIntent {
            intent_id: sha256(
                format!(
                    "WEB_CONTEXT_ROLLOVER_V1\0{}\0{}\0{}",
                    key.project_id, key.conversation_url, key.target_generation
                )
                .as_bytes(),
            ),
            key: key.clone(),
            target_sha256: key.target_sha256(),
            source_task_id: source_task_id.into(),
            estimated_upper_tokens: status.estimated_upper_tokens,
            trigger_tokens: status.trigger_tokens,
            requested_at_unix: now_unix,
            quiescence_evidence: "CALLER_OBSERVED_ADVISORY".into(),
            status: "REQUESTED".into(),
            trigger_reason: if status.near_capacity {
                "OBSERVED_PRESSURE".into()
            } else {
                "INCOMPLETE_HISTORY_GUARD".into()
            },
        };
        ledger.rollover_intent = Some(intent.clone());
        self.write_ledger(&ledger)?;
        Ok(Some(intent))
    }

    /// The rollover owner calls this only after validating a completed durable
    /// disposition and a fresh completed task. The old REQUESTED ledger stays
    /// readable by prior releases until this actual successor is requested.
    /// Original intent bytes remain in the immutable disposition evidence.
    pub(crate) fn request_successor_after_disposition(
        &self,
        key: &ConversationKey,
        expected: &RolloverIntent,
        resolution_id: &str,
        source_task_id: &str,
        quiescent: bool,
        now_unix: u64,
    ) -> Result<Option<RolloverIntent>, String> {
        key.validate()?;
        valid_identifier(resolution_id)?;
        valid_identifier(source_task_id)?;
        if expected.key != *key
            || expected.status != "REQUESTED"
            || expected.target_sha256 != key.target_sha256()
            || now_unix == 0
        {
            return Err("WEB_CONTEXT_ROLLOVER_DISPOSITION_INVALID".into());
        }
        if source_task_id == expected.source_task_id {
            return Err("WEB_CONTEXT_ABANDONED_TASK_REPLAY_FORBIDDEN".into());
        }
        let _lock = self.lock()?;
        let project = self.read_project(&key.project_id)?;
        if project.active.as_ref() != Some(key) {
            return Err("WEB_CONTEXT_TARGET_CAS_CONFLICT".into());
        }
        let mut ledger = self.read_ledger(key)?;
        let status = pressure_status(&project, &ledger);
        if !self.rollover_due_locked(&project, &status)? || !quiescent {
            return Ok(None);
        }
        let next_id = sha256(
            format!(
                "WEB_CONTEXT_ROLLOVER_AFTER_DISPOSITION_V1\0{}\0{}\0{}\0{}\0{}",
                key.project_id,
                key.conversation_url,
                key.target_generation,
                expected.intent_id,
                resolution_id
            )
            .as_bytes(),
        );
        if let Some(existing) = &ledger.rollover_intent
            && existing.intent_id == next_id
            && existing.source_task_id == source_task_id
        {
            return Ok(Some(existing.clone()));
        }
        if ledger.rollover_intent.as_ref() != Some(expected) {
            return Err("WEB_CONTEXT_ROLLOVER_DISPOSITION_CHANGED".into());
        }
        let next = RolloverIntent {
            intent_id: next_id,
            key: key.clone(),
            target_sha256: key.target_sha256(),
            source_task_id: source_task_id.into(),
            estimated_upper_tokens: status.estimated_upper_tokens,
            trigger_tokens: status.trigger_tokens,
            requested_at_unix: now_unix,
            quiescence_evidence: "CALLER_OBSERVED_ADVISORY".into(),
            status: "REQUESTED".into(),
            trigger_reason: if status.near_capacity {
                "OBSERVED_PRESSURE"
            } else {
                "INCOMPLETE_HISTORY_GUARD"
            }
            .into(),
        };
        ledger.rollover_intent = Some(next.clone());
        self.write_ledger(&ledger)?;
        Ok(Some(next))
    }

    fn rollover_due_locked(
        &self,
        project: &ProjectState,
        status: &PressureStatus,
    ) -> Result<bool, String> {
        if !status.is_current {
            return Ok(false);
        }
        let incomplete_history_guard_candidate = project.policy.incomplete_history_rollover
            && status.browser_coverage.observed
            && status.pressure_assessment == "UNKNOWN_OR_INCOMPLETE";
        // One uncertainty-only rollover is enough to put the project on a
        // fresh conversation. Repeating it on that fresh chat's first bounded
        // excerpt would otherwise create an automatic rollover loop.
        let incomplete_history_guard = incomplete_history_guard_candidate
            && !self.predecessor_used_incomplete_history_guard(project, &status.key)?;
        Ok(project.policy.automatic_rollover_enabled
            && (status.near_capacity || incomplete_history_guard))
    }

    fn predecessor_used_incomplete_history_guard(
        &self,
        project: &ProjectState,
        current: &ConversationKey,
    ) -> Result<bool, String> {
        for predecessor in project.history.iter().filter(|key| *key != current) {
            if self
                .read_ledger(predecessor)?
                .rollover_intent
                .as_ref()
                .is_some_and(|intent| intent.trigger_reason == "INCOMPLETE_HISTORY_GUARD")
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn lock(&self) -> Result<File, String> {
        if self.read_only {
            return Err("WEB_CONTEXT_READ_ONLY".into());
        }
        let path = self.root.join("ledger.lock");
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if !metadata.is_file() || redirected(&metadata) {
                return Err("WEB_CONTEXT_LOCK_UNSAFE".into());
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(io_error)?;
        let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
        if !metadata.is_file() || redirected(&metadata) {
            return Err("WEB_CONTEXT_LOCK_UNSAFE".into());
        }
        if self.nonblocking {
            file.try_lock().map_err(|error| match error {
                std::fs::TryLockError::WouldBlock => "WEB_CONTEXT_BUSY".to_string(),
                std::fs::TryLockError::Error(error) => io_error(error),
            })?;
        } else {
            file.lock().map_err(io_error)?;
        }
        Ok(file)
    }

    fn read_lock(&self) -> Result<File, String> {
        if !self.read_only {
            return self.lock();
        }
        let path = self.root.join("ledger.lock");
        let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
        if !metadata.is_file() || redirected(&metadata) {
            return Err("WEB_CONTEXT_LOCK_UNSAFE".into());
        }
        let file = File::open(&path).map_err(io_error)?;
        let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
        if !metadata.is_file() || redirected(&metadata) {
            return Err("WEB_CONTEXT_LOCK_UNSAFE".into());
        }
        file.try_lock_shared().map_err(|_| "WEB_CONTEXT_BUSY")?;
        Ok(file)
    }

    fn project_path(&self, project_id: &str) -> PathBuf {
        self.root
            .join("projects")
            .join(format!("{}.json", sha256(project_id.as_bytes())))
    }

    fn ledger_path(&self, key: &ConversationKey) -> PathBuf {
        self.root.join("conversations").join(format!(
            "{}.json",
            sha256(
                format!(
                    "{}\0{}\0{}",
                    key.project_id, key.conversation_url, key.target_generation
                )
                .as_bytes()
            )
        ))
    }

    fn read_project_or_default(&self, project_id: &str) -> Result<ProjectState, String> {
        let path = self.project_path(project_id);
        if !path.exists() {
            return Ok(ProjectState {
                schema_version: SCHEMA_VERSION,
                project_id: project_id.into(),
                policy: ContextPressurePolicy::default(),
                active: None,
                history: Vec::new(),
            });
        }
        self.read_project(project_id)
    }

    fn read_project(&self, project_id: &str) -> Result<ProjectState, String> {
        let project: ProjectState =
            read_bounded(&self.project_path(project_id), MAX_PROJECT_BYTES)?;
        if project.schema_version != SCHEMA_VERSION
            || project.project_id != project_id
            || project.history.len() > MAX_HISTORY
            || project.policy.validate().is_err()
            || project
                .history
                .iter()
                .any(|key| key.project_id != project_id || key.validate().is_err())
            || project
                .active
                .as_ref()
                .is_some_and(|active| project.history.last() != Some(active))
        {
            return Err("WEB_CONTEXT_PROJECT_INVALID".into());
        }
        Ok(project)
    }

    fn write_project(&self, project: &ProjectState) -> Result<(), String> {
        write_atomic(
            &self.project_path(&project.project_id),
            project,
            MAX_PROJECT_BYTES,
        )
    }

    fn read_ledger(&self, key: &ConversationKey) -> Result<ConversationLedger, String> {
        let ledger: ConversationLedger =
            read_bounded(&self.ledger_path(key), MAX_CONVERSATION_BYTES)?;
        if ledger.schema_version != SCHEMA_VERSION
            || ledger.key != *key
            || ledger.events.len() > MAX_EVENTS
            || ledger
                .events
                .iter()
                .map(|event| &event.event_id)
                .collect::<BTreeSet<_>>()
                .len()
                != ledger.events.len()
            || ledger
                .events
                .iter()
                .any(|event| !valid_event_metadata(event))
            || (ledger.browser_coverage.observed && !ledger.browser_coverage.initial_history_gap)
            || ledger.browser_coverage.last_observed_at_unix == Some(0)
            || (!ledger.browser_coverage.observed
                && (ledger.browser_coverage.initial_history_gap
                    || ledger.browser_coverage.last_observed_at_unix.is_some()
                    || ledger.browser_coverage.identity_gap
                    || ledger.browser_coverage.text_gap
                    || ledger.browser_coverage.overflow_gap))
            || ledger.rollover_intent.as_ref().is_some_and(|intent| {
                intent.key != *key
                    || intent.target_sha256 != key.target_sha256()
                    || intent.status != "REQUESTED"
                    || !matches!(
                        intent.trigger_reason.as_str(),
                        "OBSERVED_PRESSURE" | "INCOMPLETE_HISTORY_GUARD"
                    )
            })
        {
            return Err("WEB_CONTEXT_LEDGER_INVALID".into());
        }
        Ok(ledger)
    }

    fn write_ledger(&self, ledger: &ConversationLedger) -> Result<(), String> {
        write_atomic(
            &self.ledger_path(&ledger.key),
            ledger,
            MAX_CONVERSATION_BYTES,
        )
    }

    fn reject_cross_project_url(&self, key: &ConversationKey) -> Result<(), String> {
        for entry in fs::read_dir(self.root.join("projects")).map_err(io_error)? {
            let path = entry.map_err(io_error)?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let project: ProjectState = read_bounded(&path, MAX_PROJECT_BYTES)?;
            if project.project_id != key.project_id
                && project
                    .history
                    .iter()
                    .any(|prior| prior.conversation_url == key.conversation_url)
            {
                return Err("WEB_CONTEXT_CROSS_PROJECT_TARGET".into());
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_observation(observation: Observation<'_>) -> Result<(), String> {
    observation_metadata(observation).map(|_| ())
}

fn apply_event(ledger: &mut ConversationLedger, event: EventMetadata) -> Result<bool, String> {
    if let Some(previous) = ledger
        .events
        .iter_mut()
        .find(|previous| previous.event_id == event.event_id)
    {
        if event.revision < previous.revision
            || event.source_kind != previous.source_kind
            || event.dedup_key != previous.dedup_key
        {
            return Err("WEB_CONTEXT_EVENT_REVISION_CONFLICT".into());
        }
        if event.revision == previous.revision {
            if event.content_sha256 != previous.content_sha256
                || event.visible_bytes != previous.visible_bytes
                || event.excerpt_only != previous.excerpt_only
            {
                return Err("WEB_CONTEXT_EVENT_REVISION_CONFLICT".into());
            }
            // Tokenizer implementation changes must not turn an exact-content
            // replay into a conflicting revision or rewrite legacy counts.
            // The content digest/bytes and all identity fields still match.
            return Ok(false);
        }
        *previous = event;
    } else {
        if ledger.events.len() >= MAX_EVENTS {
            return Err("WEB_CONTEXT_EVENT_LIMIT".into());
        }
        ledger.events.push(event);
    }
    Ok(true)
}

fn observation_metadata(observation: Observation<'_>) -> Result<EventMetadata, String> {
    valid_identifier(observation.event_id)?;
    valid_identifier(observation.dedup_key)?;
    if observation.revision == 0 || observation.observed_at_unix == 0 {
        return Err("WEB_CONTEXT_EVENT_INVALID".into());
    }
    let (content_sha256, visible_bytes, estimated_tokens) =
        match (observation.source_kind, observation.visible_text) {
            (VisibleSourceKind::UnknownAttachment, None) => (None, None, None),
            (VisibleSourceKind::UnknownAttachment, Some(_)) | (_, None) => {
                return Err("WEB_CONTEXT_EVENT_CONTENT_INVALID".into());
            }
            (_, Some(text)) if text.len() <= MAX_VISIBLE_TEXT_BYTES => (
                Some(sha256(text.as_bytes())),
                Some(text.len() as u64),
                Some(estimate_tokens(text)),
            ),
            _ => return Err("WEB_CONTEXT_EVENT_CONTENT_OVERSIZED".into()),
        };
    Ok(EventMetadata {
        event_id: observation.event_id.into(),
        revision: observation.revision,
        dedup_key: observation.dedup_key.into(),
        source_kind: observation.source_kind,
        content_sha256,
        visible_bytes,
        estimated_tokens,
        observed_at_unix: observation.observed_at_unix,
        excerpt_only: false,
    })
}

fn pressure_status(project: &ProjectState, ledger: &ConversationLedger) -> PressureStatus {
    let mut visible = BTreeMap::<&str, u64>::new();
    let mut arguments = BTreeMap::<&str, u64>::new();
    let mut results = BTreeMap::<&str, u64>::new();
    let mut unknown = BTreeSet::<&str>::new();
    for event in &ledger.events {
        if event.source_kind == VisibleSourceKind::UnknownAttachment {
            unknown.insert(&event.dedup_key);
            continue;
        }
        let tokens = event.estimated_tokens.unwrap_or(0);
        if event.source_kind.counts_in_context_pressure() {
            let entry = visible.entry(&event.dedup_key).or_default();
            *entry = (*entry).max(tokens);
        }
        let traffic = match event.source_kind {
            VisibleSourceKind::ToolArguments => Some(&mut arguments),
            VisibleSourceKind::ToolResult => Some(&mut results),
            _ => None,
        };
        if let Some(traffic) = traffic {
            let entry = traffic.entry(&event.dedup_key).or_default();
            *entry = (*entry).max(tokens);
        }
    }
    let visible_unique_tokens = visible
        .values()
        .fold(0u64, |sum, value| sum.saturating_add(*value));
    let estimated_upper_tokens = visible_unique_tokens.saturating_add(
        (unknown.len() as u64).saturating_mul(project.policy.unknown_source_reserve_tokens),
    );
    let coverage_known = ledger.browser_coverage.observed
        && !ledger.browser_coverage.initial_history_gap
        && !ledger.browser_coverage.identity_gap
        && !ledger.browser_coverage.text_gap
        && !ledger.browser_coverage.overflow_gap;
    let pressure_assessment = if !coverage_known || !unknown.is_empty() {
        "UNKNOWN_OR_INCOMPLETE"
    } else if visible_unique_tokens >= project.policy.trigger_tokens() {
        "OBSERVED_AT_OR_ABOVE_TRIGGER"
    } else {
        "OBSERVED_BELOW_TRIGGER"
    };
    PressureStatus {
        key: ledger.key.clone(),
        target_sha256: ledger.key.target_sha256(),
        is_current: project.active.as_ref() == Some(&ledger.key),
        recorded_events: ledger.events.len(),
        observation_limit_reached: ledger.events.len() >= MAX_EVENTS,
        last_observed_event_at_unix: ledger
            .events
            .iter()
            .map(|event| event.observed_at_unix)
            .max(),
        excerpt_only_messages: ledger
            .events
            .iter()
            .filter(|event| event.excerpt_only)
            .count(),
        visible_unique_tokens,
        observed_estimate_tokens: visible_unique_tokens,
        unknown_source_count: unknown.len(),
        estimated_upper_tokens,
        observed_tool_argument_tokens: arguments
            .values()
            .fold(0u64, |sum, value| sum.saturating_add(*value)),
        observed_tool_result_tokens: results
            .values()
            .fold(0u64, |sum, value| sum.saturating_add(*value)),
        configured_budget_tokens: project.policy.configured_budget_tokens,
        safety_headroom_tokens: project.policy.safety_headroom_tokens,
        trigger_tokens: project.policy.trigger_tokens(),
        // A configured reserve is a proxy, not a bound on unseen history.
        near_capacity: visible_unique_tokens >= project.policy.trigger_tokens(),
        warning_tokens: project.policy.warning_tokens(),
        warning_reached: visible_unique_tokens >= project.policy.warning_tokens(),
        coverage_known,
        browser_coverage: ledger.browser_coverage.clone(),
        pressure_assessment: pressure_assessment.into(),
        automatic_rollover_enabled: project.policy.automatic_rollover_enabled,
        automatic_observation_enabled: project.policy.automatic_observation_enabled,
        incomplete_history_rollover: project.policy.incomplete_history_rollover,
        rollover_intent: ledger.rollover_intent.clone(),
        estimate_method: "O200K_OBSERVED_CONTEXT_PROXY_WITH_CONFIGURED_UNKNOWN_RESERVE".into(),
        advisory: true,
        origin_authenticated: false,
    }
}

fn valid_identifier(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 200
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
    {
        return Err("WEB_CONTEXT_IDENTIFIER_INVALID".into());
    }
    Ok(())
}

fn valid_event_metadata(event: &EventMetadata) -> bool {
    valid_identifier(&event.event_id).is_ok()
        && valid_identifier(&event.dedup_key).is_ok()
        && event.revision > 0
        && event.observed_at_unix > 0
        && (!event.excerpt_only
            || matches!(
                event.source_kind,
                VisibleSourceKind::UserMessage | VisibleSourceKind::AssistantMessage
            ))
        && match event.source_kind {
            VisibleSourceKind::UnknownAttachment => {
                event.content_sha256.is_none()
                    && event.visible_bytes.is_none()
                    && event.estimated_tokens.is_none()
            }
            _ => {
                event.content_sha256.as_ref().is_some_and(|hash| {
                    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                }) && event
                    .visible_bytes
                    .is_some_and(|bytes| bytes <= MAX_VISIBLE_TEXT_BYTES as u64)
                    && event.estimated_tokens.is_some()
            }
        }
}

fn estimate_tokens(text: &str) -> u64 {
    // BPE on a long, unbroken run can be quadratic. Keep each work unit small
    // independently of the persisted event size. Summed pieces are an advisory
    // proxy, not an exact provider count or a bound on hidden context.
    const WORK_BYTES: usize = 256;
    let tokenizer = o200k_base_singleton();
    let mut remaining = text;
    let mut tokens = 0u64;
    while !remaining.is_empty() {
        let mut length = remaining.len().min(WORK_BYTES);
        while !remaining.is_char_boundary(length) {
            length -= 1;
        }
        tokens = tokens.saturating_add(
            tokenizer
                .encode_with_special_tokens(&remaining[..length])
                .len()
                .try_into()
                .unwrap_or(u64::MAX),
        );
        remaining = &remaining[length..];
    }
    tokens
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn io_error(error: std::io::Error) -> String {
    format!("WEB_CONTEXT_IO: {error}")
}

fn plain_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if !metadata.is_dir() || redirected(&metadata) {
        return Err("WEB_CONTEXT_DIRECTORY_UNSAFE".into());
    }
    Ok(())
}

fn redirected(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Check each ancestor before creating a missing directory. A single
/// create_dir_all would otherwise traverse a redirected parent first.
fn checked_directory(path: &Path, create: bool) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty() && *parent != path)
    {
        checked_directory(parent, create)?;
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !redirected(&metadata) => {}
        Ok(_) => return Err("WEB_CONTEXT_DIRECTORY_UNSAFE".into()),
        Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
            match fs::create_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io_error(error)),
            }
            plain_directory(path)?;
        }
        Err(error) => return Err(io_error(error)),
    }
    Ok(())
}

fn read_bounded<T: DeserializeOwned>(path: &Path, maximum: usize) -> Result<T, String> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if !metadata.is_file() || redirected(&metadata) || metadata.len() > maximum as u64 {
        return Err("WEB_CONTEXT_RECORD_UNSAFE".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io_error)?
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > maximum {
        return Err("WEB_CONTEXT_RECORD_OVERSIZED".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "WEB_CONTEXT_RECORD_INVALID".into())
}

fn write_atomic<T: Serialize>(path: &Path, value: &T, maximum: usize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "WEB_CONTEXT_RECORD_INVALID")?;
    if bytes.len() + 1 > maximum {
        return Err("WEB_CONTEXT_RECORD_OVERSIZED".into());
    }
    let parent = path.parent().ok_or("WEB_CONTEXT_RECORD_UNSAFE")?;
    plain_directory(parent)?;
    let tmp = parent.join(format!(".{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(io_error)?;
        file.write_all(&bytes).map_err(io_error)?;
        file.write_all(b"\n").map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        fs::rename(&tmp, path).map_err(io_error)
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_capture_freshness_is_durable_scoped_and_legacy_unknown() {
        let root = std::env::temp_dir().join(format!("web-freshness-{}", Uuid::new_v4()));
        let store = WebContextStore::open(&root).unwrap();
        let key = |project: &str, generation| ConversationKey {
            project_id: project.into(),
            conversation_url: format!("https://chatgpt.com/c/{project}-{generation}"),
            target_generation: generation,
        };
        let alpha = key("alpha", 1);
        let beta = key("beta", 1);
        store.bind_observed_target(alpha.clone(), None).unwrap();
        store.bind_observed_target(beta.clone(), None).unwrap();
        let capture = |observed_at_unix| BrowserCoverageObservation {
            observed_at_unix,
            initial_history_gap: true,
            identity_gap: false,
            text_gap: false,
            overflow_gap: false,
        };
        store.record_browser_coverage(&alpha, capture(20)).unwrap();
        store.record_browser_coverage(&alpha, capture(10)).unwrap();
        let reader = WebContextStore::open_read_only(&root).unwrap();
        assert_eq!(
            reader
                .pressure(&alpha)
                .unwrap()
                .browser_coverage
                .last_observed_at_unix,
            Some(20)
        );
        assert_eq!(
            reader
                .pressure(&beta)
                .unwrap()
                .browser_coverage
                .last_observed_at_unix,
            None
        );
        let next = key("alpha", 2);
        store
            .bind_observed_target(next.clone(), Some(&alpha))
            .unwrap();
        assert_eq!(
            reader
                .pressure(&next)
                .unwrap()
                .browser_coverage
                .last_observed_at_unix,
            None
        );
        assert_eq!(
            store
                .record_browser_coverage(&alpha, capture(99))
                .unwrap_err(),
            "WEB_CONTEXT_OLD_GENERATION"
        );
        assert_eq!(
            reader
                .pressure(&alpha)
                .unwrap()
                .browser_coverage
                .last_observed_at_unix,
            Some(20)
        );
        // A pre-upgrade ledger retains unknown freshness; no timestamp is invented.
        let path = store.ledger_path(&alpha);
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        legacy["browserCoverage"]
            .as_object_mut()
            .unwrap()
            .remove("lastObservedAtUnix");
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let historical = reader.pressure(&alpha).unwrap();
        assert_eq!(historical.browser_coverage.last_observed_at_unix, None);
        assert!(!historical.coverage_known);
        assert!(historical.browser_coverage.observed);
    }

    struct Fixture {
        root: PathBuf,
        store: WebContextStore,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let root = std::env::current_dir()
                .unwrap()
                .join("target")
                .join(format!("catdesk-web-context-{label}-{}", Uuid::new_v4()));
            let store = WebContextStore::open(&root).expect("new store");
            Self { root, store }
        }

        fn key(project: &str, chat: &str, generation: u64) -> ConversationKey {
            ConversationKey {
                project_id: project.into(),
                conversation_url: format!("https://chatgpt.com/c/{chat}"),
                target_generation: generation,
            }
        }

        fn observe(
            &self,
            key: &ConversationKey,
            event_id: &str,
            revision: u64,
            dedup_key: &str,
            kind: VisibleSourceKind,
            text: Option<&str>,
        ) -> PressureStatus {
            self.store
                .record_observation(
                    key,
                    Observation {
                        event_id,
                        revision,
                        dedup_key,
                        source_kind: kind,
                        visible_text: text,
                        observed_at_unix: 100,
                    },
                )
                .expect("observation")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if self
                .root
                .starts_with(std::env::current_dir().unwrap().join("target"))
            {
                let _ = fs::remove_dir_all(&self.root);
            }
        }
    }

    #[test]
    fn event_activity_survives_cold_reads_without_poll_retry_or_cross_conversation_refresh() {
        let fixture = Fixture::new("event-activity");
        let alpha = Fixture::key("alpha", "activity-alpha", 1);
        let beta = Fixture::key("beta", "activity-beta", 1);
        for key in [&alpha, &beta] {
            fixture
                .store
                .bind_observed_target(key.clone(), None)
                .unwrap();
        }
        let observe = |key: &ConversationKey, id: &str, revision, text: &str, timestamp| {
            fixture
                .store
                .record_observation(
                    key,
                    Observation {
                        event_id: id,
                        revision,
                        dedup_key: id,
                        source_kind: VisibleSourceKind::ToolArguments,
                        visible_text: Some(text),
                        observed_at_unix: timestamp,
                    },
                )
                .unwrap()
        };
        assert_eq!(
            fixture
                .store
                .pressure(&alpha)
                .unwrap()
                .last_observed_event_at_unix,
            None
        );
        observe(&alpha, "newer-event", 1, "alpha original", 20);
        observe(&alpha, "older-event", 1, "out of order", 10);
        observe(&beta, "beta-event", 1, "beta only", 30);
        let alpha_path = fixture.store.ledger_path(&alpha);
        let before = fs::read(&alpha_path).unwrap();
        // A later receipt of identical content is not fresh project activity.
        assert_eq!(
            observe(&alpha, "newer-event", 1, "alpha original", 99).last_observed_event_at_unix,
            Some(20)
        );
        let reader = WebContextStore::open_read_only(&fixture.root).unwrap();
        for _ in 0..2 {
            assert_eq!(
                reader.pressure(&alpha).unwrap().last_observed_event_at_unix,
                Some(20)
            );
            assert_eq!(
                reader.pressure(&beta).unwrap().last_observed_event_at_unix,
                Some(30)
            );
        }
        assert_eq!(fs::read(&alpha_path).unwrap(), before);
        assert_eq!(
            observe(&alpha, "newer-event", 2, "alpha changed", 40).last_observed_event_at_unix,
            Some(40)
        );
        let next = Fixture::key("alpha", "activity-alpha-next", 2);
        fixture
            .store
            .bind_observed_target(next.clone(), Some(&alpha))
            .unwrap();
        assert_eq!(
            reader.pressure(&next).unwrap().last_observed_event_at_unix,
            None
        );
        let historical = reader.pressure(&alpha).unwrap();
        assert!(!historical.is_current);
        assert_eq!(historical.last_observed_event_at_unix, Some(40));
        assert_eq!(
            reader.pressure(&beta).unwrap().last_observed_event_at_unix,
            Some(30)
        );
        // Older serialized status snapshots remain readable with unknown activity.
        let mut legacy = serde_json::to_value(&historical).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("lastObservedEventAtUnix");
        assert_eq!(
            serde_json::from_value::<PressureStatus>(legacy)
                .unwrap()
                .last_observed_event_at_unix,
            None
        );
    }

    #[test]
    fn batch_import_refuses_empty_oversized_and_internal_conflict_without_writes() {
        let fixture = Fixture::new("batch-import");
        let key = Fixture::key("alpha", "batch-import", 1);
        fixture
            .store
            .bind_observed_target(key.clone(), None)
            .unwrap();
        let before = fs::read(fixture.store.ledger_path(&key)).unwrap();
        let event = |text| Observation {
            event_id: "same-event",
            revision: 1,
            dedup_key: "same-event",
            source_kind: VisibleSourceKind::UserMessage,
            visible_text: Some(text),
            observed_at_unix: 1,
        };
        assert_eq!(
            fixture
                .store
                .record_observation_batch(&key, [])
                .unwrap_err(),
            "WEB_CONTEXT_INPUT_SCOPE_INVALID"
        );
        assert_eq!(
            fixture
                .store
                .record_observation_batch(&key, (0..513).map(|_| event("text")))
                .unwrap_err(),
            "WEB_CONTEXT_INPUT_SCOPE_INVALID"
        );
        assert_eq!(
            fixture
                .store
                .record_observation_batch(&key, [event("first"), event("different")])
                .unwrap_err(),
            "WEB_CONTEXT_EVENT_REVISION_CONFLICT"
        );
        assert_eq!(fs::read(fixture.store.ledger_path(&key)).unwrap(), before);
        let reader = WebContextStore::open_read_only(&fixture.root).unwrap();
        assert_eq!(
            reader
                .record_observation_batch(&key, [event("text")])
                .unwrap_err(),
            "WEB_CONTEXT_READ_ONLY"
        );
        assert_eq!(fs::read(fixture.store.ledger_path(&key)).unwrap(), before);
    }

    #[test]
    fn admitted_traffic_finishes_only_on_exact_known_history_without_reactivating_it() {
        let fixture = Fixture::new("traffic-history");
        let old = Fixture::key("alpha", "traffic-old", 1);
        let next = Fixture::key("alpha", "traffic-next", 2);
        let beta = Fixture::key("beta", "traffic-beta", 1);
        fixture
            .store
            .bind_observed_target(old.clone(), None)
            .unwrap();
        fixture
            .store
            .bind_observed_target(beta.clone(), None)
            .unwrap();
        fixture
            .store
            .bind_observed_target(next.clone(), Some(&old))
            .unwrap();
        let project_before = fs::read(fixture.store.project_path("alpha")).unwrap();
        let next_before = fs::read(fixture.store.ledger_path(&next)).unwrap();
        let beta_before = fs::read(fixture.store.ledger_path(&beta)).unwrap();
        let event = || Observation {
            event_id: "mcp-invocation:result:0",
            revision: 1,
            dedup_key: "mcp-invocation:result:0",
            source_kind: VisibleSourceKind::ToolResult,
            visible_text: Some("known final result"),
            observed_at_unix: 1,
        };
        assert_eq!(
            fixture
                .store
                .record_observation_batch(&old, [event()])
                .unwrap_err(),
            "WEB_CONTEXT_OLD_GENERATION"
        );
        let result = fixture
            .store
            .record_existing_conversation_batch(&old, [event()])
            .unwrap();
        assert!(!result.is_current);
        assert_eq!(
            result.observed_tool_result_tokens,
            estimate_tokens("known final result")
        );
        assert_eq!(
            fixture
                .store
                .record_existing_conversation_batch(&old, [event()])
                .unwrap(),
            result
        );
        assert_eq!(
            fs::read(fixture.store.project_path("alpha")).unwrap(),
            project_before
        );
        assert_eq!(
            fs::read(fixture.store.ledger_path(&next)).unwrap(),
            next_before
        );
        assert_eq!(
            fs::read(fixture.store.ledger_path(&beta)).unwrap(),
            beta_before
        );
        let unrecorded = Fixture::key("alpha", "unrecorded", 3);
        assert_eq!(
            fixture
                .store
                .record_existing_conversation_batch(&unrecorded, [event()])
                .unwrap_err(),
            "WEB_CONTEXT_OLD_GENERATION"
        );
        assert!(!fixture.store.ledger_path(&unrecorded).exists());
        assert_eq!(fixture.store.active_key("alpha").unwrap(), Some(next));
        assert!(!result.coverage_known);
        assert!(!result.automatic_rollover_enabled);
    }

    #[test]
    fn read_only_context_never_creates_state_and_refuses_mutation() {
        let fixture = Fixture::new("read-only");
        assert!(WebContextStore::open_read_only(fixture.root.join("absent")).is_err());
        assert!(!fixture.root.join("absent").exists());
        let reader = WebContextStore::open_read_only(&fixture.root).unwrap();
        assert!(reader.active_key("alpha").is_err());
        assert!(!fixture.root.join("ledger.lock").exists());
        let key = Fixture::key("alpha", "read-only-chat", 1);
        fixture
            .store
            .bind_observed_target(key.clone(), None)
            .unwrap();
        let before = fs::read(fixture.store.project_path("alpha")).unwrap();
        assert_eq!(reader.active_key("alpha").unwrap(), Some(key.clone()));
        assert!(reader.pressure(&key).is_ok());
        assert_eq!(reader.project_history("alpha").unwrap().len(), 1);
        assert!(reader.active_pressure("alpha").unwrap().is_some());
        let ledger_before = fs::read(fixture.store.ledger_path(&key)).unwrap();
        let (pressure, due) = reader.rollover_readiness(&key).unwrap();
        assert_eq!(pressure.key, key);
        assert!(!due);
        assert_eq!(
            fs::read(fixture.store.ledger_path(&key)).unwrap(),
            ledger_before
        );
        assert_eq!(
            reader
                .configure_policy("alpha", ContextPressurePolicy::default())
                .unwrap_err(),
            "WEB_CONTEXT_READ_ONLY"
        );
        assert_eq!(
            fs::read(fixture.store.project_path("alpha")).unwrap(),
            before
        );
    }

    #[test]
    fn read_only_context_returns_busy_without_disturbing_the_writer() {
        let fixture = Fixture::new("read-only-busy");
        let key = Fixture::key("alpha", "read-only-busy-chat", 1);
        fixture
            .store
            .bind_observed_target(key.clone(), None)
            .unwrap();
        let reader = WebContextStore::open_read_only(&fixture.root).unwrap();
        let held = fixture.store.lock().unwrap();
        assert_eq!(reader.active_key("alpha").unwrap_err(), "WEB_CONTEXT_BUSY");
        assert_eq!(
            reader.project_history("alpha").unwrap_err(),
            "WEB_CONTEXT_BUSY"
        );
        assert_eq!(
            reader.rollover_readiness(&key).unwrap_err(),
            "WEB_CONTEXT_BUSY"
        );
        drop(held);
        assert!(reader.pressure(&key).is_ok());
    }

    #[test]
    fn online_context_refuses_contended_reads_and_writes_without_changing_state() {
        let fixture = Fixture::new("online-busy");
        let key = Fixture::key("alpha", "online-busy-chat", 1);
        fixture
            .store
            .bind_observed_target(key.clone(), None)
            .unwrap();
        let online = WebContextStore::open_nonblocking(&fixture.root).unwrap();
        let project_before = fs::read(fixture.store.project_path("alpha")).unwrap();
        let ledger_before = fs::read(fixture.store.ledger_path(&key)).unwrap();
        let held = fixture.store.lock().unwrap();
        assert_eq!(online.active_key("alpha").unwrap_err(), "WEB_CONTEXT_BUSY");
        assert_eq!(online.pressure(&key).unwrap_err(), "WEB_CONTEXT_BUSY");
        assert_eq!(
            online
                .configure_policy("alpha", ContextPressurePolicy::default())
                .unwrap_err(),
            "WEB_CONTEXT_BUSY"
        );
        assert_eq!(
            online
                .bind_observed_target(key.clone(), Some(&key))
                .unwrap_err(),
            "WEB_CONTEXT_BUSY"
        );
        drop(held);
        assert_eq!(
            fs::read(fixture.store.project_path("alpha")).unwrap(),
            project_before
        );
        assert_eq!(
            fs::read(fixture.store.ledger_path(&key)).unwrap(),
            ledger_before
        );
        online
            .configure_policy("alpha", ContextPressurePolicy::default())
            .unwrap();
        assert_eq!(online.pressure(&key).unwrap().key, key);
    }

    #[test]
    fn default_policy_is_conservative_and_not_provider_authoritative() {
        let fixture = Fixture::new("default");
        let key = Fixture::key("alpha", "alpha-1", 1);
        fixture
            .store
            .bind_observed_target(key.clone(), None)
            .unwrap();
        let status = fixture.store.pressure(&key).unwrap();
        assert_eq!(status.configured_budget_tokens, 96_000);
        assert_eq!(status.trigger_tokens, 72_000);
        assert_eq!(status.warning_tokens, 57_600);
        assert!(!status.warning_reached);
        assert!(!status.automatic_rollover_enabled);
        assert!(status.advisory);
        assert!(!status.origin_authenticated);
        assert_eq!(
            status.target_sha256,
            sha256(key.conversation_url.as_bytes())
        );
    }

    #[test]
    fn warning_policy_is_legacy_compatible_and_validates_before_writes() {
        let fixture = Fixture::new("warning-policy");
        let legacy = serde_json::json!({
            "configuredBudgetTokens":96000,"safetyHeadroomTokens":24000,
            "unknownSourceReserveTokens":8000,"automaticRolloverEnabled":false,
            "automaticObservationEnabled":false,"incompleteHistoryRollover":false
        });
        let policy: ContextPressurePolicy = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(serde_json::to_value(&policy).unwrap(), legacy);
        assert_eq!(policy.warning_tokens(), 57_600);
        fixture
            .store
            .configure_policy("alpha", policy.clone())
            .unwrap();
        let before = fs::read(fixture.store.project_path("alpha")).unwrap();
        for warning in [0, 72_001, u64::MAX] {
            let mut invalid = policy.clone();
            invalid.warning_threshold_tokens = Some(warning);
            assert_eq!(
                fixture
                    .store
                    .configure_policy("alpha", invalid)
                    .unwrap_err(),
                "WEB_CONTEXT_POLICY_INVALID"
            );
            assert_eq!(
                fs::read(fixture.store.project_path("alpha")).unwrap(),
                before
            );
        }
        let minimal = ContextPressurePolicy {
            configured_budget_tokens: 1,
            safety_headroom_tokens: 0,
            unknown_source_reserve_tokens: 0,
            ..Default::default()
        };
        minimal.validate().unwrap();
        assert_eq!(minimal.warning_tokens(), 1);
    }

    #[test]
    fn warning_is_scoped_durable_observed_only_and_does_not_trigger_rollover() {
        let fixture = Fixture::new("warning-boundary");
        let alpha = Fixture::key("alpha", "warning-alpha", 1);
        let beta = Fixture::key("beta", "warning-beta", 1);
        for key in [&alpha, &beta] {
            fixture
                .store
                .bind_observed_target(key.clone(), None)
                .unwrap();
        }
        let observed = fixture.observe(
            &alpha,
            "observed",
            1,
            "observed",
            VisibleSourceKind::UserMessage,
            Some("hello context"),
        );
        let tokens = observed.observed_estimate_tokens;
        assert!(tokens > 0);
        let mut policy = ContextPressurePolicy {
            configured_budget_tokens: tokens + 100,
            safety_headroom_tokens: 50,
            unknown_source_reserve_tokens: 0,
            warning_threshold_tokens: Some(tokens + 1),
            automatic_rollover_enabled: true,
            ..Default::default()
        };
        fixture
            .store
            .configure_policy("alpha", policy.clone())
            .unwrap();
        assert!(!fixture.store.pressure(&alpha).unwrap().warning_reached);
        policy.warning_threshold_tokens = Some(tokens);
        fixture.store.configure_policy("alpha", policy).unwrap();
        let warning = fixture.store.pressure(&alpha).unwrap();
        assert!(warning.warning_reached && !warning.near_capacity);
        assert!(!warning.coverage_known);
        assert_eq!(warning.pressure_assessment, "UNKNOWN_OR_INCOMPLETE");
        assert!(
            fixture
                .store
                .request_rollover_if_due(&alpha, &alpha.target_sha256(), "task-one", true, 101)
                .unwrap()
                .is_none()
        );
        assert!(!fixture.store.pressure(&beta).unwrap().warning_reached);
        assert_eq!(
            fixture.store.pressure(&beta).unwrap().warning_tokens,
            57_600
        );
        let reopened = WebContextStore::open_read_only(&fixture.root).unwrap();
        assert!(reopened.pressure(&alpha).unwrap().warning_reached);
        assert_eq!(reopened.pressure(&alpha).unwrap().warning_tokens, tokens);
        assert!(!reopened.pressure(&beta).unwrap().warning_reached);
    }

    #[test]
    fn streaming_revision_and_duplicate_surfaces_do_not_sum_snapshots() {
        let fixture = Fixture::new("stream-dedup");
        let key = Fixture::key("alpha", "alpha-2", 1);
        fixture
            .store
            .bind_observed_target(key.clone(), None)
            .unwrap();
        let first = fixture.observe(
            &key,
            "user-1",
            1,
            "turn-1",
            VisibleSourceKind::UserMessage,
            Some("hello"),
        );
        let replay = fixture.observe(
            &key,
            "user-1",
            1,
            "turn-1",
            VisibleSourceKind::UserMessage,
            Some("hello"),
        );
        assert_eq!(first.visible_unique_tokens, replay.visible_unique_tokens);
        let revised = fixture.observe(
            &key,
            "user-1",
            2,
            "turn-1",
            VisibleSourceKind::UserMessage,
            Some("hello world with more words"),
        );
        assert_eq!(
            revised.visible_unique_tokens,
            estimate_tokens("hello world with more words")
        );
        let duplicate = fixture.observe(
            &key,
            "assistant-copy",
            1,
            "turn-1",
            VisibleSourceKind::AssistantMessage,
            Some("hello world with more words"),
        );
        assert_eq!(
            duplicate.visible_unique_tokens,
            revised.visible_unique_tokens
        );
        let arguments = fixture.observe(
            &key,
            "tool-args",
            1,
            "tool-args-1",
            VisibleSourceKind::ToolArguments,
            Some("a large model-to-tool argument"),
        );
        assert_eq!(
            arguments.visible_unique_tokens,
            duplicate.visible_unique_tokens + estimate_tokens("a large model-to-tool argument")
        );
        assert_eq!(
            arguments.observed_tool_argument_tokens,
            estimate_tokens("a large model-to-tool argument")
        );
        assert!(
            fixture
                .store
                .record_observation(
                    &key,
                    Observation {
                        event_id: "user-1",
                        revision: 1,
                        dedup_key: "turn-1",
                        source_kind: VisibleSourceKind::UserMessage,
                        visible_text: Some("stale"),
                        observed_at_unix: 100,
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn exact_legacy_content_replay_preserves_stored_token_estimate() {
        let fixture = Fixture::new("legacy-estimate-replay");
        let key = Fixture::key("alpha", "legacy-estimate", 1);
        fixture
            .store
            .bind_observed_target(key.clone(), None)
            .unwrap();
        // Simulate an older tokenizer's persisted count without changing the
        // content/identity. A normal observation cannot supply token metadata.
        let text = "known unchanged text ".repeat(30);
        fixture.observe(
            &key,
            "legacy",
            1,
            "legacy",
            VisibleSourceKind::ToolResult,
            Some(&text),
        );
        let mut ledger = fixture.store.read_ledger(&key).unwrap();
        ledger.events[0].estimated_tokens = Some(17);
        fixture.store.write_ledger(&ledger).unwrap();
        let before = fs::read(fixture.store.ledger_path(&key)).unwrap();
        let replay = fixture.observe(
            &key,
            "legacy",
            1,
            "legacy",
            VisibleSourceKind::ToolResult,
            Some(&text),
        );
        assert_eq!(replay.observed_tool_result_tokens, 17);
        assert_eq!(fs::read(fixture.store.ledger_path(&key)).unwrap(), before);
        for (id, excerpt) in [("browser-complete", false), ("browser-excerpt", true)] {
            let record = || {
                if excerpt {
                    fixture.store.record_browser_excerpt_observation(
                        &key,
                        id,
                        VisibleSourceKind::AssistantMessage,
                        &text,
                        100,
                    )
                } else {
                    fixture.store.record_browser_observation(
                        &key,
                        id,
                        VisibleSourceKind::UserMessage,
                        Some(&text),
                        100,
                    )
                }
            };
            record().unwrap();
            let mut ledger = fixture.store.read_ledger(&key).unwrap();
            let event = ledger
                .events
                .iter_mut()
                .find(|event| event.event_id == id)
                .unwrap();
            event.estimated_tokens = Some(17);
            fixture.store.write_ledger(&ledger).unwrap();
            let before = fs::read(fixture.store.ledger_path(&key)).unwrap();
            record().unwrap();
            assert_eq!(
                fs::read(fixture.store.ledger_path(&key)).unwrap(),
                before,
                "same browser content cannot invent a revision after an estimator change"
            );
        }
        let before = fs::read(fixture.store.ledger_path(&key)).unwrap();
        assert!(
            fixture
                .store
                .record_observation(
                    &key,
                    Observation {
                        event_id: "legacy",
                        revision: 1,
                        dedup_key: "legacy",
                        source_kind: VisibleSourceKind::ToolResult,
                        visible_text: Some("different content"),
                        observed_at_unix: 100,
                    }
                )
                .is_err()
        );
        assert_eq!(fs::read(fixture.store.ledger_path(&key)).unwrap(), before);
    }

    #[test]
    fn retained_arguments_and_worker_messages_trigger_only_their_conversation() {
        let fixture = Fixture::new("retained-tool-worker-pressure");
        let alpha = Fixture::key("alpha", "tool-worker-alpha", 1);
        let beta = Fixture::key("beta", "tool-worker-beta", 1);
        for key in [&alpha, &beta] {
            fixture
                .store
                .bind_observed_target(key.clone(), None)
                .unwrap();
        }
        let arguments = "retained model-to-tool arguments ".repeat(30);
        let worker = "model-visible worker delivery ".repeat(30);
        let total = estimate_tokens(&arguments) + estimate_tokens(&worker);
        let mut policy = ContextPressurePolicy {
            configured_budget_tokens: total,
            safety_headroom_tokens: 1,
            unknown_source_reserve_tokens: 0,
            ..ContextPressurePolicy::default()
        };
        fixture
            .store
            .configure_policy("alpha", policy.clone())
            .unwrap();
        let sibling = fixture.observe(
            &beta,
            "same-event",
            1,
            "same-content",
            VisibleSourceKind::UserMessage,
            Some("beta"),
        );
        let first = fixture.observe(
            &alpha,
            "same-event",
            1,
            "same-content",
            VisibleSourceKind::ToolArguments,
            Some(&arguments),
        );
        assert_eq!(first.observed_estimate_tokens, estimate_tokens(&arguments));
        assert!(!first.near_capacity);
        let pressured = fixture.observe(
            &alpha,
            "worker",
            1,
            "worker-content",
            VisibleSourceKind::WorkerMessage,
            Some(&worker),
        );
        assert_eq!(pressured.observed_estimate_tokens, total);
        assert!(pressured.near_capacity);
        assert!(
            fixture
                .store
                .request_rollover_if_due(&alpha, &alpha.target_sha256(), "task", true, 200,)
                .unwrap()
                .is_none(),
            "pressure alone cannot enable automatic rollover"
        );
        let duplicate = fixture.observe(
            &alpha,
            "worker-copy",
            1,
            "worker-content",
            VisibleSourceKind::ToolResult,
            Some(&worker),
        );
        assert_eq!(duplicate.observed_estimate_tokens, total);
        assert_eq!(
            duplicate.observed_tool_result_tokens,
            estimate_tokens(&worker)
        );
        let revised_text = format!("{worker}more retained worker context");
        let revised = fixture.observe(
            &alpha,
            "worker",
            2,
            "worker-content",
            VisibleSourceKind::WorkerMessage,
            Some(&revised_text),
        );
        assert_eq!(
            revised.observed_estimate_tokens,
            estimate_tokens(&arguments) + estimate_tokens(&revised_text)
        );
        let reopened = WebContextStore::open(&fixture.root).unwrap();
        assert_eq!(reopened.pressure(&alpha).unwrap(), revised);
        assert_eq!(reopened.pressure(&beta).unwrap(), sibling);
        policy.automatic_rollover_enabled = true;
        reopened.configure_policy("alpha", policy).unwrap();
        let intent = reopened
            .request_rollover_if_due(&alpha, &alpha.target_sha256(), "task", true, 201)
            .unwrap()
            .expect("retained observed content reaches configured pressure");
        assert_eq!(intent.trigger_reason, "OBSERVED_PRESSURE");
        assert_eq!(
            intent.estimated_upper_tokens,
            revised.observed_estimate_tokens
        );
        let next = Fixture::key("alpha", "tool-worker-alpha-next", 2);
        reopened
            .bind_observed_target(next.clone(), Some(&alpha))
            .unwrap();
        assert_eq!(
            reopened.pressure(&next).unwrap().observed_estimate_tokens,
            0
        );
        assert!(!reopened.pressure(&alpha).unwrap().is_current);
        assert_eq!(reopened.pressure(&beta).unwrap(), sibling);
    }

    #[test]
    fn browser_observation_replays_stable_hash_and_advances_changed_content() {
        let fixture = Fixture::new("browser-revisions");
        let key = Fixture::key("alpha", "browser-revisions", 1);
        fixture
            .store
            .bind_observed_target(key.clone(), None)
            .unwrap();
        let first = fixture
            .store
            .record_browser_observation(
                &key,
                "visible-user-1",
                VisibleSourceKind::UserMessage,
                Some("hello"),
                100,
            )
            .unwrap();
        let replay = fixture
            .store
            .record_browser_observation(
                &key,
                "visible-user-1",
                VisibleSourceKind::UserMessage,
                Some("hello"),
                101,
            )
            .unwrap();
        assert_eq!(first, replay);
        assert_eq!(
            fixture.store.read_ledger(&key).unwrap().events[0].revision,
            1
        );
        let revised = fixture
            .store
            .record_browser_observation(
                &key,
                "visible-user-1",
                VisibleSourceKind::UserMessage,
                Some("hello world with more words"),
                102,
            )
            .unwrap();
        assert_eq!(revised.recorded_events, 1);
        assert_eq!(
            revised.visible_unique_tokens,
            estimate_tokens("hello world with more words")
        );
        assert_eq!(
            fixture.store.read_ledger(&key).unwrap().events[0].revision,
            2
        );
        let unknown = fixture
            .store
            .record_browser_observation(
                &key,
                "unknown-history",
                VisibleSourceKind::UnknownAttachment,
                None,
                103,
            )
            .unwrap();
        assert_eq!(unknown.unknown_source_count, 1);
        let unknown_replay = fixture
            .store
            .record_browser_observation(
                &key,
                "unknown-history",
                VisibleSourceKind::UnknownAttachment,
                None,
                104,
            )
            .unwrap();
        assert_eq!(unknown, unknown_replay);
        assert_eq!(
            fixture.store.read_ledger(&key).unwrap().events[1].revision,
            1
        );
        assert_eq!(
            fixture
                .store
                .record_browser_observation(
                    &key,
                    "visible-user-1",
                    VisibleSourceKind::AssistantMessage,
                    Some("role changed"),
                    105,
                )
                .unwrap_err(),
            "WEB_CONTEXT_EVENT_REVISION_CONFLICT"
        );
        assert_eq!(
            fixture
                .store
                .record_browser_observation(
                    &key,
                    "unknown-history",
                    VisibleSourceKind::UnknownAttachment,
                    Some("unexpected bytes"),
                    105,
                )
                .unwrap_err(),
            "WEB_CONTEXT_EVENT_CONTENT_INVALID"
        );
    }

    #[test]
    fn browser_observation_serializes_concurrent_revisions_without_lost_updates() {
        let fixture = Fixture::new("browser-concurrent");
        let key = Fixture::key("alpha", "browser-concurrent", 1);
        fixture
            .store
            .bind_observed_target(key.clone(), None)
            .unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|index| {
                let store = fixture.store.clone();
                let key = key.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let text = format!("independent browser revision {index}");
                    barrier.wait();
                    store.record_browser_observation(
                        &key,
                        "streaming-assistant-1",
                        VisibleSourceKind::AssistantMessage,
                        Some(&text),
                        200 + index,
                    )
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("thread").expect("observation");
        }
        let ledger = fixture.store.read_ledger(&key).unwrap();
        assert_eq!(ledger.events.len(), 1);
        assert_eq!(ledger.events[0].revision, 8);
        assert_eq!(ledger.events[0].dedup_key, "streaming-assistant-1");
    }

    #[test]
    fn browser_observation_rejects_old_generation_without_cross_project_effects() {
        let fixture = Fixture::new("browser-projects");
        let alpha = Fixture::key("alpha", "browser-alpha", 1);
        let beta = Fixture::key("beta", "browser-beta", 1);
        let gamma = Fixture::key("gamma", "browser-gamma", 1);
        for key in [&alpha, &beta, &gamma] {
            fixture
                .store
                .bind_observed_target(key.clone(), None)
                .unwrap();
            fixture
                .store
                .record_browser_observation(
                    key,
                    "visible-turn-1",
                    VisibleSourceKind::UserMessage,
                    Some(&format!("{} source message", key.project_id)),
                    100,
                )
                .unwrap();
        }
        let beta_before = fixture.store.pressure(&beta).unwrap();
        let gamma_before = fixture.store.pressure(&gamma).unwrap();
        let successor = Fixture::key("alpha", "browser-alpha-next", 2);
        fixture
            .store
            .bind_observed_target(successor.clone(), Some(&alpha))
            .unwrap();
        assert_eq!(
            fixture
                .store
                .record_browser_observation(
                    &alpha,
                    "visible-turn-1",
                    VisibleSourceKind::UserMessage,
                    Some("late source update"),
                    200,
                )
                .unwrap_err(),
            "WEB_CONTEXT_OLD_GENERATION"
        );
        fixture
            .store
            .record_browser_observation(
                &successor,
                "visible-turn-1",
                VisibleSourceKind::UserMessage,
                Some("successor only"),
                201,
            )
            .unwrap();
        assert_eq!(fixture.store.pressure(&beta).unwrap(), beta_before);
        assert_eq!(fixture.store.pressure(&gamma).unwrap(), gamma_before);
        assert_eq!(
            fixture.store.read_ledger(&alpha).unwrap().events[0].revision,
            1
        );
        assert_eq!(
            fixture.store.read_ledger(&successor).unwrap().events[0].revision,
            1
        );
    }

    #[test]
    fn incomplete_browser_coverage_is_durable_idempotent_and_project_scoped() {
        let fixture = Fixture::new("browser-coverage");
        let alpha = Fixture::key("alpha", "coverage-alpha", 1);
        let beta = Fixture::key("beta", "coverage-beta", 1);
        fixture
            .store
            .bind_observed_target(alpha.clone(), None)
            .unwrap();
        fixture
            .store
            .bind_observed_target(beta.clone(), None)
            .unwrap();
        let first = fixture
            .store
            .record_browser_coverage(
                &alpha,
                BrowserCoverageObservation {
                    observed_at_unix: 1,
                    initial_history_gap: true,
                    identity_gap: true,
                    text_gap: false,
                    overflow_gap: true,
                },
            )
            .unwrap();
        assert!(!first.coverage_known);
        assert_eq!(first.pressure_assessment, "UNKNOWN_OR_INCOMPLETE");
        assert!(first.browser_coverage.identity_gap);
        assert!(first.browser_coverage.overflow_gap);
        assert_eq!(first.unknown_source_count, 0);
        let replay = fixture
            .store
            .record_browser_coverage(
                &alpha,
                BrowserCoverageObservation {
                    observed_at_unix: 1,
                    initial_history_gap: true,
                    identity_gap: false,
                    text_gap: false,
                    overflow_gap: false,
                },
            )
            .unwrap();
        assert_eq!(first, replay);
        assert_eq!(
            fixture.store.pressure(&beta).unwrap().browser_coverage,
            BrowserCoverageState::default()
        );
        let reopened = WebContextStore::open(&fixture.root).unwrap();
        assert_eq!(reopened.pressure(&alpha).unwrap(), first);
        assert_eq!(
            reopened
                .record_browser_coverage(
                    &alpha,
                    BrowserCoverageObservation {
                        observed_at_unix: 1,
                        initial_history_gap: false,
                        identity_gap: false,
                        text_gap: false,
                        overflow_gap: false,
                    }
                )
                .unwrap_err(),
            "WEB_CONTEXT_COVERAGE_UNPROVEN"
        );
        let successor = Fixture::key("alpha", "coverage-alpha-next", 2);
        reopened
            .bind_observed_target(successor.clone(), Some(&alpha))
            .unwrap();
        assert_eq!(
            reopened
                .record_browser_coverage(
                    &alpha,
                    BrowserCoverageObservation {
                        observed_at_unix: 1,
                        initial_history_gap: true,
                        identity_gap: false,
                        text_gap: false,
                        overflow_gap: false,
                    }
                )
                .unwrap_err(),
            "WEB_CONTEXT_OLD_GENERATION"
        );
        assert_eq!(
            reopened.pressure(&alpha).unwrap().browser_coverage,
            first.browser_coverage
        );
        assert_eq!(
            reopened.pressure(&successor).unwrap().browser_coverage,
            BrowserCoverageState::default()
        );
    }

    #[test]
    fn incomplete_history_rollover_needs_capture_explicit_guard_and_quiescence() {
        let fixture = Fixture::new("coverage-guard");
        let key = Fixture::key("alpha", "coverage-guard", 1);
        fixture
            .store
            .bind_observed_target(key.clone(), None)
            .unwrap();
        let mut policy = ContextPressurePolicy {
            configured_budget_tokens: 100,
            safety_headroom_tokens: 40,
            unknown_source_reserve_tokens: 8,
            automatic_rollover_enabled: true,
            automatic_observation_enabled: false,
            incomplete_history_rollover: false,
            ..Default::default()
        };
        fixture
            .store
            .configure_policy("alpha", policy.clone())
            .unwrap();
        assert!(
            fixture
                .store
                .request_rollover_if_due(&key, &key.target_sha256(), "task-1", true, 100)
                .unwrap()
                .is_none()
        );
        fixture
            .store
            .record_browser_coverage(
                &key,
                BrowserCoverageObservation {
                    observed_at_unix: 1,
                    initial_history_gap: true,
                    identity_gap: false,
                    text_gap: false,
                    overflow_gap: false,
                },
            )
            .unwrap();
        let status = fixture.store.pressure(&key).unwrap();
        assert_eq!(status.observed_estimate_tokens, 0);
        assert!(!status.near_capacity);
        assert_eq!(status.pressure_assessment, "UNKNOWN_OR_INCOMPLETE");
        assert!(
            fixture
                .store
                .request_rollover_if_due(&key, &key.target_sha256(), "task-1", true, 101)
                .unwrap()
                .is_none()
        );
        policy.incomplete_history_rollover = true;
        fixture
            .store
            .configure_policy("alpha", policy.clone())
            .unwrap();
        assert!(
            fixture
                .store
                .request_rollover_if_due(&key, &key.target_sha256(), "task-1", false, 102)
                .unwrap()
                .is_none()
        );
        let intent = fixture
            .store
            .request_rollover_if_due(&key, &key.target_sha256(), "task-1", true, 103)
            .unwrap()
            .unwrap();
        assert_eq!(intent.trigger_reason, "INCOMPLETE_HISTORY_GUARD");
        assert_eq!(intent.estimated_upper_tokens, 0);
        assert!(!fixture.store.pressure(&key).unwrap().near_capacity);
        assert!(fixture.store.rollover_readiness(&key).unwrap().1);
        let replacement = Fixture::key("alpha", "coverage-guard-next", 2);
        fixture
            .store
            .bind_observed_target(replacement.clone(), Some(&key))
            .unwrap();
        fixture
            .store
            .record_browser_coverage(
                &replacement,
                BrowserCoverageObservation {
                    observed_at_unix: 1,
                    initial_history_gap: true,
                    identity_gap: false,
                    text_gap: false,
                    overflow_gap: false,
                },
            )
            .unwrap();
        assert!(!fixture.store.rollover_readiness(&replacement).unwrap().1);
        assert!(!fixture.store.rollover_readiness(&key).unwrap().1);
        assert!(
            fixture
                .store
                .request_rollover_if_due(
                    &replacement,
                    &replacement.target_sha256(),
                    "task-2",
                    true,
                    104,
                )
                .unwrap()
                .is_none()
        );
        let observed_text = "visible context ".repeat(80);
        fixture.observe(
            &replacement,
            "new-chat-growth",
            1,
            "new-chat-growth",
            VisibleSourceKind::UserMessage,
            Some(&observed_text),
        );
        let later = fixture
            .store
            .request_rollover_if_due(
                &replacement,
                &replacement.target_sha256(),
                "task-2",
                true,
                105,
            )
            .unwrap()
            .unwrap();
        assert_eq!(later.trigger_reason, "OBSERVED_PRESSURE");
    }

    #[test]
    fn three_projects_and_old_generation_remain_independent() {
        let fixture = Fixture::new("projects");
        let alpha = Fixture::key("alpha", "alpha-3", 1);
        let beta = Fixture::key("beta", "beta-3", 4);
        let gamma = Fixture::key("gamma", "gamma-3", 7);
        for key in [&alpha, &beta, &gamma] {
            fixture
                .store
                .bind_observed_target(key.clone(), None)
                .unwrap();
        }
        assert_eq!(
            fixture.store.active_key("alpha").unwrap(),
            Some(alpha.clone())
        );
        assert_eq!(fixture.store.active_key("unregistered").unwrap(), None);
        fixture.observe(
            &alpha,
            "a-1",
            1,
            "a-1",
            VisibleSourceKind::UserMessage,
            Some("alpha only"),
        );
        fixture.observe(
            &beta,
            "b-1",
            1,
            "b-1",
            VisibleSourceKind::UserMessage,
            Some("beta only"),
        );
        fixture.observe(
            &gamma,
            "g-1",
            1,
            "g-1",
            VisibleSourceKind::UserMessage,
            Some("gamma only"),
        );
        let beta_before = fixture.store.pressure(&beta).unwrap();
        let successor = Fixture::key("alpha", "alpha-4", 2);
        fixture
            .store
            .bind_observed_target(successor.clone(), Some(&alpha))
            .unwrap();
        assert!(
            fixture
                .store
                .record_observation(
                    &alpha,
                    Observation {
                        event_id: "a-late",
                        revision: 1,
                        dedup_key: "a-late",
                        source_kind: VisibleSourceKind::ToolResult,
                        visible_text: Some("late old event"),
                        observed_at_unix: 101,
                    }
                )
                .unwrap_err()
                .contains("OLD_GENERATION")
        );
        assert_eq!(fixture.store.pressure(&beta).unwrap(), beta_before);
        assert_eq!(fixture.store.project_history("alpha").unwrap().len(), 2);
        assert!(!fixture.store.pressure(&alpha).unwrap().is_current);
        assert!(fixture.store.pressure(&successor).unwrap().is_current);
        assert!(
            fixture
                .store
                .bind_observed_target(Fixture::key("delta", "beta-3", 1), None)
                .is_err()
        );
    }

    #[test]
    fn unknown_source_reserve_opt_out_quiescence_and_restart_intent() {
        let fixture = Fixture::new("intent");
        let key = Fixture::key("alpha", "alpha-intent", 1);
        fixture
            .store
            .bind_observed_target(key.clone(), None)
            .unwrap();
        let mut policy = ContextPressurePolicy {
            configured_budget_tokens: 100,
            safety_headroom_tokens: 40,
            unknown_source_reserve_tokens: 70,
            automatic_rollover_enabled: false,
            automatic_observation_enabled: false,
            incomplete_history_rollover: false,
            ..Default::default()
        };
        fixture
            .store
            .configure_policy("alpha", policy.clone())
            .unwrap();
        let status = fixture.observe(
            &key,
            "attachment-1",
            1,
            "attachment-1",
            VisibleSourceKind::UnknownAttachment,
            None,
        );
        assert_eq!(status.unknown_source_count, 1);
        assert_eq!(status.estimated_upper_tokens, 70);
        assert!(!status.near_capacity);
        assert_eq!(status.pressure_assessment, "UNKNOWN_OR_INCOMPLETE");
        assert!(
            fixture
                .store
                .request_rollover_if_due(&key, &key.target_sha256(), "task-1", true, 200)
                .unwrap()
                .is_none()
        );
        policy.automatic_rollover_enabled = true;
        fixture
            .store
            .configure_policy("alpha", policy.clone())
            .unwrap();
        assert!(
            fixture
                .store
                .request_rollover_if_due(&key, &key.target_sha256(), "task-1", false, 200)
                .unwrap()
                .is_none()
        );
        assert!(
            fixture
                .store
                .request_rollover_if_due(&key, &key.target_sha256(), "task-1", true, 200)
                .unwrap()
                .is_none()
        );
        let observed_text = "visible context ".repeat(80);
        let observed = fixture.observe(
            &key,
            "message-pressure",
            1,
            "message-pressure",
            VisibleSourceKind::UserMessage,
            Some(&observed_text),
        );
        assert!(observed.near_capacity);
        let intent = fixture
            .store
            .request_rollover_if_due(&key, &key.target_sha256(), "task-1", true, 200)
            .unwrap()
            .unwrap();
        assert_eq!(intent.status, "REQUESTED");
        assert_eq!(intent.trigger_reason, "OBSERVED_PRESSURE");
        assert_eq!(intent.quiescence_evidence, "CALLER_OBSERVED_ADVISORY");
        let replay = fixture
            .store
            .request_rollover_if_due(&key, &key.target_sha256(), "task-2", true, 201)
            .unwrap()
            .unwrap();
        assert_eq!(intent, replay);
        let reopened = WebContextStore::open(&fixture.root).unwrap();
        assert_eq!(
            reopened.pressure(&key).unwrap().rollover_intent,
            Some(intent)
        );
        assert!(
            reopened
                .request_rollover_if_due(&key, &key.target_sha256(), "task-2", false, 202)
                .unwrap()
                .is_none()
        );
        policy.automatic_rollover_enabled = false;
        reopened.configure_policy("alpha", policy).unwrap();
        assert!(
            reopened
                .request_rollover_if_due(&key, &key.target_sha256(), "task-2", true, 203)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn bounded_text_and_metadata_persist_without_transcript() {
        let fixture = Fixture::new("private");
        let key = Fixture::key("alpha", "alpha-private", 1);
        fixture
            .store
            .bind_observed_target(key.clone(), None)
            .unwrap();
        let secret = "private visible browser text abcdefghijklmnop";
        fixture.observe(
            &key,
            "user-secret",
            1,
            "user-secret",
            VisibleSourceKind::UserMessage,
            Some(secret),
        );
        let bytes = fs::read(fixture.store.ledger_path(&key)).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(secret));
        assert!(String::from_utf8_lossy(&bytes).contains(&sha256(secret.as_bytes())));
        let oversized = "x".repeat(MAX_VISIBLE_TEXT_BYTES + 1);
        assert!(
            fixture
                .store
                .record_observation(
                    &key,
                    Observation {
                        event_id: "oversized",
                        revision: 1,
                        dedup_key: "oversized",
                        source_kind: VisibleSourceKind::ToolResult,
                        visible_text: Some(&oversized),
                        observed_at_unix: 101,
                    }
                )
                .unwrap_err()
                .contains("OVERSIZED")
        );
        assert_eq!(fixture.store.pressure(&key).unwrap().recorded_events, 1);
    }

    #[test]
    fn missing_target_record_after_partial_write_can_be_finished_without_reset() {
        let fixture = Fixture::new("partial-bind");
        let key = Fixture::key("alpha", "alpha-partial", 1);
        let staged = ConversationLedger {
            schema_version: SCHEMA_VERSION,
            key: key.clone(),
            events: Vec::new(),
            rollover_intent: None,
            browser_coverage: BrowserCoverageState::default(),
        };
        fixture.store.write_ledger(&staged).unwrap();
        fixture
            .store
            .bind_observed_target(key.clone(), None)
            .unwrap();
        fixture
            .store
            .bind_observed_target(key.clone(), None)
            .unwrap();
        assert_eq!(fixture.store.active_key("alpha").unwrap(), Some(key));
    }

    #[test]
    fn redirected_parent_is_rejected_before_child_creation_when_symlinks_available() {
        let fixture = Fixture::new("redirected");
        let outside = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("catdesk-web-context-outside-{}", Uuid::new_v4()));
        fs::create_dir(&outside).unwrap();
        let link = fixture.root.join("redirected-parent");
        #[cfg(windows)]
        let created = std::os::windows::fs::symlink_dir(&outside, &link).is_ok();
        #[cfg(unix)]
        let created = std::os::unix::fs::symlink(&outside, &link).is_ok();
        if created {
            assert!(WebContextStore::open(link.join("should-not-exist")).is_err());
            assert!(!outside.join("should-not-exist").exists());
            fs::remove_dir(&link).unwrap();
        }
        fs::remove_dir(&outside).unwrap();
    }
}
