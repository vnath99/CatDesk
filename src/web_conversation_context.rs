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
    pub automatic_rollover_enabled: bool,
}

impl Default for ContextPressurePolicy {
    fn default() -> Self {
        Self {
            configured_budget_tokens: 96_000,
            safety_headroom_tokens: 24_000,
            unknown_source_reserve_tokens: 8_000,
            automatic_rollover_enabled: false,
        }
    }
}

impl ContextPressurePolicy {
    fn validate(&self) -> Result<(), String> {
        if self.configured_budget_tokens == 0
            || self.configured_budget_tokens > 2_000_000
            || self.safety_headroom_tokens >= self.configured_budget_tokens
            || self.unknown_source_reserve_tokens > self.configured_budget_tokens
        {
            return Err("WEB_CONTEXT_POLICY_INVALID".into());
        }
        Ok(())
    }

    pub fn trigger_tokens(&self) -> u64 {
        self.configured_budget_tokens - self.safety_headroom_tokens
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
    Handoff,
    UnknownAttachment,
}

impl VisibleSourceKind {
    fn counts_in_visible_pressure(self) -> bool {
        matches!(
            self,
            Self::UserMessage | Self::AssistantMessage | Self::ToolResult | Self::Handoff
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConversationLedger {
    schema_version: u32,
    key: ConversationKey,
    events: Vec<EventMetadata>,
    rollover_intent: Option<RolloverIntent>,
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
    pub visible_unique_tokens: u64,
    pub unknown_source_count: usize,
    pub estimated_upper_tokens: u64,
    pub observed_tool_argument_tokens: u64,
    pub observed_tool_result_tokens: u64,
    pub configured_budget_tokens: u64,
    pub safety_headroom_tokens: u64,
    pub trigger_tokens: u64,
    pub near_capacity: bool,
    pub automatic_rollover_enabled: bool,
    pub rollover_intent: Option<RolloverIntent>,
    pub estimate_method: String,
    pub advisory: bool,
    pub origin_authenticated: bool,
}

#[derive(Clone, Debug)]
pub struct WebContextStore {
    root: PathBuf,
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
        Ok(Self { root })
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
        };
        if self.ledger_path(&key).exists() {
            let existing = self.read_ledger(&key)?;
            if !existing.events.is_empty() || existing.rollover_intent.is_some() {
                return Err("WEB_CONTEXT_TARGET_ALREADY_RECORDED".into());
            }
        } else {
            self.write_ledger(&ledger)?;
        }
        project.history.push(key.clone());
        project.active = Some(key);
        self.write_project(&project)
    }

    /// Records only model-visible text supplied by the caller. Tool arguments
    /// are retained as traffic metadata but excluded from context occupancy.
    pub fn record_observation(
        &self,
        key: &ConversationKey,
        observation: Observation<'_>,
    ) -> Result<PressureStatus, String> {
        key.validate()?;
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
        let event = EventMetadata {
            event_id: observation.event_id.into(),
            revision: observation.revision,
            dedup_key: observation.dedup_key.into(),
            source_kind: observation.source_kind,
            content_sha256,
            visible_bytes,
            estimated_tokens,
            observed_at_unix: observation.observed_at_unix,
        };
        let _lock = self.lock()?;
        let project = self.read_project(&key.project_id)?;
        if project.active.as_ref() != Some(key) {
            return Err("WEB_CONTEXT_OLD_GENERATION".into());
        }
        let mut ledger = self.read_ledger(key)?;
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
                    || event.estimated_tokens != previous.estimated_tokens
                {
                    return Err("WEB_CONTEXT_EVENT_REVISION_CONFLICT".into());
                }
            } else {
                *previous = event;
                self.write_ledger(&ledger)?;
            }
        } else {
            if ledger.events.len() >= MAX_EVENTS {
                return Err("WEB_CONTEXT_EVENT_LIMIT".into());
            }
            ledger.events.push(event);
            self.write_ledger(&ledger)?;
        }
        Ok(pressure_status(&project, &ledger))
    }

    pub fn pressure(&self, key: &ConversationKey) -> Result<PressureStatus, String> {
        key.validate()?;
        let _lock = self.lock()?;
        let project = self.read_project(&key.project_id)?;
        if !project.history.contains(key) {
            return Err("WEB_CONTEXT_TARGET_NOT_RECORDED".into());
        }
        Ok(pressure_status(&project, &self.read_ledger(key)?))
    }

    pub fn project_history(&self, project_id: &str) -> Result<Vec<PressureStatus>, String> {
        valid_identifier(project_id)?;
        let _lock = self.lock()?;
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
        let _lock = self.lock()?;
        Ok(self.read_project_or_default(project_id)?.active)
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
        if let Some(existing) = &ledger.rollover_intent {
            return Ok(Some(existing.clone()));
        }
        let status = pressure_status(&project, &ledger);
        if !project.policy.automatic_rollover_enabled || !status.near_capacity || !quiescent {
            return Ok(None);
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
        };
        ledger.rollover_intent = Some(intent.clone());
        self.write_ledger(&ledger)?;
        Ok(Some(intent))
    }

    fn lock(&self) -> Result<File, String> {
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
        file.lock().map_err(io_error)?;
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
            || ledger.rollover_intent.as_ref().is_some_and(|intent| {
                intent.key != *key
                    || intent.target_sha256 != key.target_sha256()
                    || intent.status != "REQUESTED"
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
        if event.source_kind.counts_in_visible_pressure() {
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
    PressureStatus {
        key: ledger.key.clone(),
        target_sha256: ledger.key.target_sha256(),
        is_current: project.active.as_ref() == Some(&ledger.key),
        recorded_events: ledger.events.len(),
        visible_unique_tokens,
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
        near_capacity: estimated_upper_tokens >= project.policy.trigger_tokens(),
        automatic_rollover_enabled: project.policy.automatic_rollover_enabled,
        rollover_intent: ledger.rollover_intent.clone(),
        estimate_method: "O200K_VISIBLE_TEXT_PROXY_WITH_UNKNOWN_RESERVE".into(),
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
    o200k_base_singleton()
        .encode_with_special_tokens(text)
        .len()
        .try_into()
        .unwrap_or(u64::MAX)
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
        assert!(!status.automatic_rollover_enabled);
        assert!(status.advisory);
        assert!(!status.origin_authenticated);
        assert_eq!(
            status.target_sha256,
            sha256(key.conversation_url.as_bytes())
        );
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
            duplicate.visible_unique_tokens
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
        assert!(status.near_capacity);
        assert!(
            fixture
                .store
                .request_rollover_if_due(&key, &key.target_sha256(), "task-1", true, 200)
                .unwrap()
                .is_none()
        );
        policy.automatic_rollover_enabled = true;
        fixture.store.configure_policy("alpha", policy).unwrap();
        assert!(
            fixture
                .store
                .request_rollover_if_due(&key, &key.target_sha256(), "task-1", false, 200)
                .unwrap()
                .is_none()
        );
        let intent = fixture
            .store
            .request_rollover_if_due(&key, &key.target_sha256(), "task-1", true, 200)
            .unwrap()
            .unwrap();
        assert_eq!(intent.status, "REQUESTED");
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
