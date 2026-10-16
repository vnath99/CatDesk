//! Direct, headless Codex CLI adapter primitives.
//!
//! The adapter only owns a process it launched from an explicit executable
//! path. It neither reads authentication storage nor accepts process paths
//! from MCP. T-0028C keeps it separate from the released delegated worker;
//! later controller work will persist its handles and choose when to invoke it.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader as SyncBufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc::{self, error::TryRecvError};

use super::codex_model_policy::{
    CodexCliTurnProfileEvidenceV1, LEGACY_TERRA_MODEL, SOL_MEDIUM_MODEL, required_effort,
};
use super::contracts::{TurnId, WorkerSessionId};
use super::runtime::{
    NormalizedProviderEventKind, NormalizedProviderEventV1, ProviderCapabilitiesV1,
    ProviderHealthStatus, ProviderHealthV1, ProviderSessionV1, ProviderType, RuntimeError,
};
use super::worker_provider::{
    ProviderEventBatchV1, ProviderFuture, ProviderIdV1, ProviderTurnHandleV1,
    WorkerProviderTurnRequestV1, WorkerProviderV1,
};

const DEFAULT_MAX_EVENT_BYTES: usize = 64 * 1024;
const DEFAULT_MAX_DIAGNOSTIC_BYTES: usize = 8 * 1024;
const MAX_PROMPT_BYTES: usize = 24 * 1024;
const DIAGNOSTIC_SCHEMA_VERSION: u32 = 1;
const MAX_DIAGNOSTIC_EVENTS: usize = 64;
const MAX_DIAGNOSTIC_EVENT_TEXT_BYTES: usize = 2 * 1024;
const MAX_ROLLOUT_SCAN_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ROLLOUT_LINE_BYTES: usize = 1024 * 1024;
const MAX_ROLLOUT_DIRECTORY_ENTRIES: usize = 50_000;
const MAX_NATIVE_STDOUT_LINE_BYTES: usize = 64 * 1024;
const MAX_NATIVE_STDOUT_EVENTS: usize = 50_000;
const MAX_NATIVE_STDOUT_BYTES: usize = 32 * 1024 * 1024;
const POST_EXIT_STREAM_DRAIN_WAIT: Duration = Duration::from_millis(500);
const NATIVE_TIMESTAMP_SKEW_SECS: u64 = 1;
const NATIVE_EVENT_TIMESTAMP_SKEW: Duration = Duration::from_millis(1);
const CATDESK_REQUIRED_CODEX_MODEL: &str = LEGACY_TERRA_MODEL;

fn managed_reasoning_override(model_id: &str) -> Option<String> {
    required_effort(model_id).map(|effort| format!("model_reasoning_effort={effort}"))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CodexCliApprovalPolicyV1 {
    Never,
}

impl CodexCliApprovalPolicyV1 {
    const fn as_flag_value(&self) -> &'static str {
        match self {
            Self::Never => "never",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CodexCliSandboxV1 {
    ReadOnly,
    WorkspaceWrite,
}

impl CodexCliSandboxV1 {
    const fn as_flag_value(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliConfigV1 {
    pub executable: PathBuf,
    pub working_directory: PathBuf,
    /// Optional operator-local Codex home. Child Codex processes see it as
    /// `CODEX_HOME`; CatDesk reads only bounded native session metadata below
    /// `sessions/` for actual-turn model readback. It never reads auth/config
    /// storage or serializes rollout transcripts. MCP cannot select this path.
    pub operator_codex_home: Option<PathBuf>,
    pub model_id: Option<String>,
    /// Host-owned state directories that the worker must not be able to
    /// modify. The host sets these; an MCP request cannot supply them.
    /// Sol workspace-write turns require at least one verified root.
    #[serde(default)]
    pub host_authority_roots: Vec<PathBuf>,
    pub sandbox: CodexCliSandboxV1,
    pub approval_policy: CodexCliApprovalPolicyV1,
    pub max_event_bytes: usize,
    pub max_diagnostic_bytes: usize,
    /// Optional operator-local location for redacted autonomous-turn evidence.
    /// This is intentionally not an MCP-selectable path.
    pub diagnostic_root: Option<PathBuf>,
}

impl CodexCliConfigV1 {
    pub fn new(executable: PathBuf, working_directory: PathBuf) -> Result<Self, RuntimeError> {
        let config = Self {
            executable,
            working_directory,
            operator_codex_home: None,
            model_id: None,
            host_authority_roots: Vec::new(),
            sandbox: CodexCliSandboxV1::WorkspaceWrite,
            approval_policy: CodexCliApprovalPolicyV1::Never,
            max_event_bytes: DEFAULT_MAX_EVENT_BYTES,
            max_diagnostic_bytes: DEFAULT_MAX_DIAGNOSTIC_BYTES,
            diagnostic_root: None,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), RuntimeError> {
        let extension = self
            .executable
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(extension.as_str(), "cmd" | "bat" | "ps1") {
            return Err(RuntimeError::Validation(
                "Codex CLI executable must be a direct executable, not a shell shim".into(),
            ));
        }
        if !self.executable.is_file() {
            return Err(RuntimeError::Validation(
                "Codex CLI executable must be an existing file".into(),
            ));
        }
        if !self.working_directory.is_dir() {
            return Err(RuntimeError::Validation(
                "Codex CLI working directory must be an existing directory".into(),
            ));
        }
        if let Some(codex_home) = &self.operator_codex_home {
            if !codex_home.is_dir() {
                return Err(RuntimeError::Validation(
                    "operator-local Codex config root must be an existing directory".into(),
                ));
            }
        }
        if self.max_event_bytes == 0 || self.max_diagnostic_bytes == 0 {
            return Err(RuntimeError::Validation(
                "Codex CLI byte limits must be positive".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_sol_worker_authority_boundary(&self) -> Result<(), RuntimeError> {
        if self.model_id.as_deref() != Some(SOL_MEDIUM_MODEL)
            || self.sandbox != CodexCliSandboxV1::WorkspaceWrite
        {
            return Ok(());
        }
        validate_sol_worker_launch_boundary(&self.working_directory, &self.host_authority_roots)
    }
}

/// A necessary path-topology check before Sol workspace-write starts. This
/// does not establish a security boundary: the ordinary interactive user can
/// still modify host-owned files elsewhere on the same machine.
pub(crate) fn validate_sol_worker_write_roots(
    worker_directory: &Path,
    host_authority_roots: &[PathBuf],
) -> Result<(), RuntimeError> {
    if host_authority_roots.is_empty() {
        return Err(RuntimeError::Validation(
            "Sol Codex worker has no host authority boundary".into(),
        ));
    }
    let working_directory = worker_directory.canonicalize().map_err(|_| {
        RuntimeError::Validation("Sol Codex worker directory is unavailable".into())
    })?;
    for root in host_authority_roots {
        if !root.is_dir() {
            return Err(RuntimeError::Validation(
                "Sol Codex host authority root is unavailable".into(),
            ));
        }
        let authority_root = root.canonicalize().map_err(|_| {
            RuntimeError::Validation("Sol Codex host authority root is unavailable".into())
        })?;
        // Check both the configured names and the resolved objects. This
        // also refuses a junction/symlink placed inside the writable root
        // that points at an authority directory elsewhere.
        if paths_overlap(worker_directory, root)
            || paths_overlap(&working_directory, &authority_root)
        {
            return Err(RuntimeError::Validation(
                "Sol Codex writable workspace overlaps host authority".into(),
            ));
        }
    }
    Ok(())
}

/// A Sol turn needs both disjoint paths and a separately launched, verified OS
/// identity that cannot write the host's authority store. The existing CLI
/// process is launched as CatDesk's interactive user through `tokio::Command`;
/// neither Codex's workspace-write policy nor a different path constrains
/// that user. Keep the live launch closed until a dedicated producer token,
/// owner/ACL proof, and bounded output handoff have been implemented.
pub(crate) fn validate_sol_worker_launch_boundary(
    worker_directory: &Path,
    host_authority_roots: &[PathBuf],
) -> Result<(), RuntimeError> {
    validate_sol_worker_write_roots(worker_directory, host_authority_roots)?;
    Err(RuntimeError::Validation(
        "Sol Codex worker OS isolation is unproven".into(),
    ))
}

fn paths_overlap(first: &Path, second: &Path) -> bool {
    first.starts_with(second) || second.starts_with(first)
}

pub(crate) fn is_sol_worker_authority_boundary_error(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::Validation(message)
            if matches!(
                message.as_str(),
                "Sol Codex worker has no host authority boundary"
                    | "Sol Codex worker directory is unavailable"
                    | "Sol Codex host authority root is unavailable"
                    | "Sol Codex writable workspace overlaps host authority"
                    | "Sol Codex worker OS isolation is unproven"
            )
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliCapabilitiesV1 {
    pub cli_version: String,
    pub supports_jsonl: bool,
    pub supports_resume: bool,
    pub supports_explicit_session_id: bool,
    pub supports_read_only_sandbox: bool,
    pub supports_workspace_write_sandbox: bool,
    pub supports_output_schema: bool,
    pub supports_output_last_message: bool,
    pub supports_owned_process_cancellation: bool,
}

impl CodexCliCapabilitiesV1 {
    pub fn from_command_output(
        version: &str,
        exec_help: &str,
        resume_help: &str,
    ) -> Result<Self, RuntimeError> {
        let version = version.trim();
        if !version.starts_with("codex-cli ") {
            return Err(RuntimeError::Validation(
                "Codex CLI version output did not have the expected prefix".into(),
            ));
        }
        Ok(Self {
            cli_version: bounded_redacted_text(version, 128),
            supports_jsonl: exec_help.contains("--json") && resume_help.contains("--json"),
            supports_resume: exec_help.contains("resume")
                && resume_help.contains("Resume a previous session"),
            supports_explicit_session_id: resume_help.contains("[SESSION_ID]"),
            supports_read_only_sandbox: exec_help.contains("read-only"),
            supports_workspace_write_sandbox: exec_help.contains("workspace-write"),
            supports_output_schema: exec_help.contains("--output-schema"),
            supports_output_last_message: exec_help.contains("--output-last-message"),
            supports_owned_process_cancellation: true,
        })
    }

    pub fn required_for_autonomous_loop(&self) -> Result<(), RuntimeError> {
        if self.supports_jsonl
            && self.supports_resume
            && self.supports_explicit_session_id
            && self.supports_read_only_sandbox
            && self.supports_workspace_write_sandbox
            && self.supports_owned_process_cancellation
        {
            Ok(())
        } else {
            Err(RuntimeError::Validation(
                "installed Codex CLI lacks a required autonomous-loop capability".into(),
            ))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliTurnRequestV1 {
    pub worker_session_id: WorkerSessionId,
    pub turn_id: TurnId,
    pub prompt: String,
}

impl CodexCliTurnRequestV1 {
    pub fn validate(&self) -> Result<(), RuntimeError> {
        if self.prompt.trim().is_empty() {
            return Err(RuntimeError::Validation(
                "Codex prompt must not be empty".into(),
            ));
        }
        if self.prompt.len() > MAX_PROMPT_BYTES {
            return Err(RuntimeError::BudgetExceeded(
                "Codex prompt exceeds bounded adapter input".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliTurnHandleV1 {
    pub handle_id: String,
    pub worker_session_id: WorkerSessionId,
    pub turn_id: TurnId,
    pub resumed_thread_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliSessionSnapshotV1 {
    pub codex_thread_id: String,
    pub provider_version: String,
    /// Actual model from completed native-turn metadata, if readback passed.
    /// An invocation flag alone never populates this field.
    pub model_id: Option<String>,
    pub working_directory: PathBuf,
    pub last_completed_turn: Option<TurnId>,
    pub last_event_sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliEventV1 {
    pub event_type: String,
    pub thread_id: Option<String>,
    pub item_type: Option<String>,
    pub text: Option<String>,
    pub retry_after_seconds: Option<u64>,
    pub bounded_summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CodexCliTurnStateV1 {
    Running,
    Completed,
    Failed,
    Cancelled,
    RateLimited,
}

/// Bounded availability classification for the autonomous provider route.
/// Explicit plan/credit language wins over a coincident HTTP 429; an
/// ambiguous terminal error never authorizes fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodexAvailabilityFailureV1 {
    TransientRateLimited,
    CreditsExhausted,
}

/// Host-local time captured at the same boundary as a provider-attested
/// exhaustion diagnostic.  The parser accepts this closed value rather than a
/// caller-selected timezone or date.  Unix time remains the durable form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CodexTrustedLocalClockV1 {
    pub observed_at_unix: u64,
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
}

/// Parses exactly one provider-attested `try again at h:mm AM/PM` phrase, with
/// an optional explicit provider-local `Mon 2nd, 2026` date.
/// This is deliberately available only after explicit credit-exhaustion
/// classification; absence, malformed text, or multiple phrases leaves the
/// route in fail-closed Qwen stickiness rather than inventing eligibility.
pub(crate) fn parse_codex_credits_reset_after(
    diagnostic: &str,
    clock: CodexTrustedLocalClockV1,
) -> Option<u64> {
    if classify_codex_availability_failure(diagnostic)
        != Some(CodexAvailabilityFailureV1::CreditsExhausted)
        || !valid_trusted_local_clock(clock)
    {
        return None;
    }
    static RESET_PHRASE: OnceLock<regex::Regex> = OnceLock::new();
    let matcher = RESET_PHRASE.get_or_init(|| {
        regex::Regex::new(
            r"(?i)(?:^|[^[:alpha:]])try again at (?:(?<month>jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec) (?<day>[1-9]|[12][0-9]|3[01])(?<ordinal>st|nd|rd|th), (?<year>[0-9]{4}) )?(?<hour>[1-9]|1[0-2]):(?<minute>[0-5][0-9]) (?<meridiem>AM|PM)(?:$|[^[:alnum:]])",
        )
        .expect("fixed Codex reset phrase regex")
    });
    let mut matches = matcher.captures_iter(diagnostic);
    let captures = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let hour: u16 = captures.name("hour")?.as_str().parse().ok()?;
    let minute: u16 = captures.name("minute")?.as_str().parse().ok()?;
    let meridiem = captures.name("meridiem")?.as_str();
    let hour_24 = match (hour, meridiem.eq_ignore_ascii_case("AM")) {
        (12, true) => 0,
        (12, false) => 12,
        (hour, true) => hour,
        (hour, false) => hour.checked_add(12)?,
    };
    let reset_seconds = u64::from(hour_24) * 3_600 + u64::from(minute) * 60;
    if let Some(month) = captures.name("month") {
        let year: u16 = captures.name("year")?.as_str().parse().ok()?;
        let day: u16 = captures.name("day")?.as_str().parse().ok()?;
        let month = month_number(month.as_str())?;
        valid_calendar_date(year, month, day).then_some(())?;
        valid_ordinal(day, captures.name("ordinal")?.as_str()).then_some(())?;
        let observed_day = days_since_unix_epoch(clock.year, clock.month, clock.day)?;
        let reset_day = days_since_unix_epoch(year, month, day)?;
        let day_delta = reset_day.checked_sub(observed_day)?;
        let observed_seconds =
            u64::from(clock.hour) * 3_600 + u64::from(clock.minute) * 60 + u64::from(clock.second);
        let delta_seconds = day_delta
            .checked_mul(86_400)?
            .checked_add(i64::try_from(reset_seconds).ok()?)?
            .checked_sub(i64::try_from(observed_seconds).ok()?)?;
        return (delta_seconds > 0).then(|| {
            clock
                .observed_at_unix
                .checked_add(u64::try_from(delta_seconds).ok()?)
        })?;
    }
    let observed_seconds =
        u64::from(clock.hour) * 3_600 + u64::from(clock.minute) * 60 + u64::from(clock.second);
    let delta = if reset_seconds > observed_seconds {
        reset_seconds - observed_seconds
    } else {
        86_400 - observed_seconds + reset_seconds
    };
    clock.observed_at_unix.checked_add(delta)
}

fn month_number(value: &str) -> Option<u16> {
    match value.to_ascii_lowercase().as_str() {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

fn valid_calendar_date(year: u16, month: u16, day: u16) -> bool {
    year >= 1970 && (1..=12).contains(&month) && (1..=days_in_month(year, month)).contains(&day)
}

fn valid_ordinal(day: u16, ordinal: &str) -> bool {
    let expected = match day % 100 {
        11..=13 => "th",
        _ => match day % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    };
    ordinal.eq_ignore_ascii_case(expected)
}

fn days_since_unix_epoch(year: u16, month: u16, day: u16) -> Option<i64> {
    valid_calendar_date(year, month, day).then_some(())?;
    let prior_year_days = (1970..year).try_fold(0_i64, |days, current_year| {
        days.checked_add(if days_in_month(current_year, 2) == 29 {
            366
        } else {
            365
        })
    })?;
    let prior_month_days = (1..month).try_fold(0_i64, |days, current_month| {
        days.checked_add(i64::from(days_in_month(year, current_month)))
    })?;
    prior_year_days
        .checked_add(prior_month_days)?
        .checked_add(i64::from(day.checked_sub(1)?))
}

fn valid_trusted_local_clock(clock: CodexTrustedLocalClockV1) -> bool {
    valid_calendar_date(clock.year, clock.month, clock.day)
        && clock.hour < 24
        && clock.minute < 60
        && clock.second < 60
}

const fn days_in_month(year: u16, month: u16) -> u16 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        _ => 0,
    }
}

/// The production parser obtains local civil time from the host OS.  The
/// session's `now_unix` is the durable instant; the local date/time only
/// resolves the provider's 12-hour clock phrase and deterministic rollover.
#[cfg(windows)]
pub(crate) fn trusted_host_local_clock(now_unix: u64) -> Option<CodexTrustedLocalClockV1> {
    #[repr(C)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(system_time: *mut SystemTime);
    }
    let mut system_time = SystemTime {
        year: 0,
        month: 0,
        day_of_week: 0,
        day: 0,
        hour: 0,
        minute: 0,
        second: 0,
        milliseconds: 0,
    };
    // `GetLocalTime` fills the caller-owned fixed-size SYSTEMTIME and has no
    // failure return; validate every field before treating it as trusted.
    unsafe { GetLocalTime(&mut system_time) };
    let clock = CodexTrustedLocalClockV1 {
        observed_at_unix: now_unix,
        year: system_time.year,
        month: system_time.month,
        day: system_time.day,
        hour: system_time.hour,
        minute: system_time.minute,
        second: system_time.second,
    };
    valid_trusted_local_clock(clock).then_some(clock)
}

#[cfg(not(windows))]
pub(crate) fn trusted_host_local_clock(_now_unix: u64) -> Option<CodexTrustedLocalClockV1> {
    None
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliEventBatchV1 {
    pub events: Vec<CodexCliEventV1>,
    pub normalized_events: Vec<NormalizedProviderEventV1>,
    pub next_cursor: u64,
    pub thread_id: Option<String>,
    pub state: CodexCliTurnStateV1,
    pub terminal: bool,
    /// Codex-owned rollout metadata for this completed native turn. Never
    /// derived from the requested CLI flags or an older resumed turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_profile: Option<CodexCliTurnProfileEvidenceV1>,
}

#[derive(Debug)]
enum StreamMessage {
    Event(CodexCliEventV1),
    /// A provider JSONL line exceeded CatDesk's bounded event size. The body
    /// is deliberately discarded without parsing or logging it; the turn may
    /// continue and provider process exit status remains authoritative.
    OversizedEvent(usize),
    ParseError(String),
    Stderr(String),
}

#[derive(Debug)]
struct ActiveCodexTurn {
    child: Child,
    turn_id: TurnId,
    receiver: mpsc::Receiver<StreamMessage>,
    events: Vec<CodexCliEventV1>,
    normalized_events: Vec<NormalizedProviderEventV1>,
    thread_id: Option<String>,
    stderr: String,
    exited: Option<std::process::ExitStatus>,
    exit_observed_at: Option<SystemTime>,
    stream_closed: bool,
    cancelled: bool,
    cancel_event_emitted: bool,
    started_at: Instant,
    launched_at: SystemTime,
    rollout_root: Option<PathBuf>,
    rollout_anchor: Option<RolloutAnchor>,
    actual_profile: Option<CodexCliTurnProfileEvidenceV1>,
    diagnostic: CodexCliExecutionDiagnosticV1,
    diagnostic_persisted: bool,
}

/// A pre-launch open file and byte boundary for a resumed Codex thread. The
/// retained handle prevents a same-name replacement from supplying a prior
/// turn's metadata as evidence for the child just launched.
#[derive(Debug)]
struct RolloutAnchor {
    path: PathBuf,
    file: File,
    length: u64,
}

/// A retained byte boundary for an existing native Codex thread. The open
/// handle is intentionally not serializable: a process receipt or a requested
/// model flag cannot reconstruct post-launch evidence after a crash.
#[derive(Debug)]
pub(crate) struct CodexCliNativeTurnReadbackV1 {
    rollout_root: PathBuf,
    bound_thread_id: String,
    anchor: RolloutAnchor,
}

/// Bounded stdout markers from the exact native child whose rollout is read.
/// Callers must feed every JSONL stdout line and mark EOF only after draining
/// the pipe. No transcript or authentication content is retained.
#[derive(Debug)]
pub(crate) struct CodexCliNativeStdoutEvidenceV1 {
    bound_thread_id: String,
    event_count: usize,
    total_bytes: usize,
    thread_started: bool,
    turn_started: bool,
    turn_completed: bool,
    eof: bool,
    invalid: bool,
}

impl CodexCliNativeStdoutEvidenceV1 {
    pub(crate) fn new(bound_thread_id: &str) -> Option<Self> {
        valid_codex_thread_id(bound_thread_id).then(|| Self {
            bound_thread_id: bound_thread_id.to_owned(),
            event_count: 0,
            total_bytes: 0,
            thread_started: false,
            turn_started: false,
            turn_completed: false,
            eof: false,
            invalid: false,
        })
    }

    pub(crate) fn observe_line(&mut self, line: &[u8]) {
        if self.invalid || self.eof {
            self.invalid = true;
            return;
        }
        self.event_count += 1;
        self.total_bytes = self.total_bytes.saturating_add(line.len());
        if self.event_count > MAX_NATIVE_STDOUT_EVENTS
            || line.len() > MAX_NATIVE_STDOUT_LINE_BYTES
            || self.total_bytes > MAX_NATIVE_STDOUT_BYTES
        {
            self.invalid = true;
            return;
        }
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            self.invalid = true;
            return;
        };
        let Some(event_type) = value.get("type").and_then(Value::as_str) else {
            self.invalid = true;
            return;
        };
        if event_type.is_empty() || event_type.len() > 128 {
            self.invalid = true;
            return;
        }
        if value
            .get("thread_id")
            .is_some_and(|thread| thread.as_str() != Some(self.bound_thread_id.as_str()))
        {
            self.invalid = true;
            return;
        }
        match event_type {
            "thread.started" if !self.thread_started && !self.turn_started => {
                self.thread_started = value.get("thread_id").and_then(Value::as_str)
                    == Some(self.bound_thread_id.as_str());
                self.invalid = !self.thread_started;
            }
            "turn.started" if self.thread_started && !self.turn_started => {
                self.turn_started = true;
            }
            "turn.completed" if self.turn_started && !self.turn_completed => {
                self.turn_completed = true;
            }
            "thread.started" | "turn.started" | "turn.completed" | "turn.failed" | "error" => {
                self.invalid = true;
            }
            _ if !self.turn_started || self.turn_completed => {
                self.invalid = true;
            }
            _ => {}
        }
    }

    pub(crate) fn invalidate(&mut self) {
        self.invalid = true;
    }

    pub(crate) fn mark_eof(&mut self) {
        self.eof = true;
    }

    fn completed_for(&self, thread_id: &str) -> bool {
        !self.invalid
            && self.eof
            && self.bound_thread_id == thread_id
            && self.thread_started
            && self.turn_started
            && self.turn_completed
    }
}

/// Bounded evidence for an autonomous provider turn. It records process
/// configuration and provider-visible output, never authentication values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliExecutionDiagnosticV1 {
    pub schema_version: u32,
    pub executable: String,
    pub cli_version: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub canonical_working_directory: String,
    pub sandbox: CodexCliSandboxV1,
    pub approval_policy: CodexCliApprovalPolicyV1,
    pub model_id: Option<String>,
    pub codex_thread_id: Option<String>,
    pub catdesk_session_id: String,
    pub catdesk_turn_id: String,
    pub invocation_id: String,
    pub environment_variable_names: Vec<String>,
    pub process_exit_code: Option<i32>,
    pub elapsed_millis: u128,
    pub stderr: String,
    pub events: Vec<CodexCliEventV1>,
}

pub struct CodexCliProviderV1 {
    config: CodexCliConfigV1,
    capabilities: CodexCliCapabilitiesV1,
    active_turns: BTreeMap<String, ActiveCodexTurn>,
    worker_handles: BTreeMap<String, CodexCliTurnHandleV1>,
    next_handle_sequence: u64,
}

impl CodexCliProviderV1 {
    pub async fn discover(config: CodexCliConfigV1) -> Result<Self, RuntimeError> {
        config.validate()?;
        let version =
            run_bounded_command(&config, ["--version"], config.max_diagnostic_bytes).await?;
        let exec_help =
            run_bounded_command(&config, ["exec", "--help"], config.max_diagnostic_bytes).await?;
        let resume_help = run_bounded_command(
            &config,
            ["exec", "resume", "--help"],
            config.max_diagnostic_bytes,
        )
        .await?;
        let capabilities = CodexCliCapabilitiesV1::from_command_output(
            &version.stdout,
            &exec_help.stdout,
            &resume_help.stdout,
        )?;
        capabilities.required_for_autonomous_loop()?;
        Ok(Self {
            config,
            capabilities,
            active_turns: BTreeMap::new(),
            worker_handles: BTreeMap::new(),
            next_handle_sequence: 1,
        })
    }

    pub fn capabilities(&self) -> &CodexCliCapabilitiesV1 {
        &self.capabilities
    }

    pub fn provider_id(&self) -> ProviderIdV1 {
        ProviderIdV1::CodexCli
    }

    pub async fn start_turn(
        &mut self,
        request: CodexCliTurnRequestV1,
    ) -> Result<CodexCliTurnHandleV1, RuntimeError> {
        request.validate()?;
        self.spawn_turn(request, None).await
    }

    pub async fn resume_turn(
        &mut self,
        thread_id: &str,
        request: CodexCliTurnRequestV1,
    ) -> Result<CodexCliTurnHandleV1, RuntimeError> {
        request.validate()?;
        if thread_id.trim().is_empty() {
            return Err(RuntimeError::Validation(
                "Codex resume requires a non-empty captured thread id".into(),
            ));
        }
        self.spawn_turn(request, Some(thread_id)).await
    }

    async fn spawn_turn(
        &mut self,
        request: CodexCliTurnRequestV1,
        resume_thread_id: Option<&str>,
    ) -> Result<CodexCliTurnHandleV1, RuntimeError> {
        self.config.validate_sol_worker_authority_boundary()?;
        let mut rollout_root = codex_rollout_root(&self.config);
        let mut command = Command::new(&self.config.executable);
        command.current_dir(&self.config.working_directory);
        apply_operator_codex_context(&mut command, &self.config);
        // These are documented global CLI controls. `--ignore-user-config`
        // made the Windows CLI fall back to a read-only sandbox despite an
        // explicit workspace-write request, so we retain user configuration
        // but prevent project exec-policy rules from changing this turn.
        command
            .arg("--sandbox")
            .arg(self.config.sandbox.as_flag_value())
            .arg("--ask-for-approval")
            .arg(self.config.approval_policy.as_flag_value());
        command.arg("exec");
        if let Some(thread_id) = resume_thread_id {
            command.arg("resume").arg(thread_id);
        }
        command.arg("--json").arg("--ignore-rules");
        if resume_thread_id.is_none() {
            command.arg("--color").arg("never");
        }
        if let Some(model_id) = &self.config.model_id {
            command.arg("--model").arg(model_id);
            if let Some(override_value) = managed_reasoning_override(model_id) {
                command.arg("-c").arg(override_value);
            }
        }
        command
            .arg(&request.prompt)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env_remove("CODEX_API_KEY")
            .env_remove("OPENAI_API_KEY")
            .env_remove("CONTROL_PLANE_API_KEY")
            .env("NO_COLOR", "1");
        command.kill_on_drop(true);
        // Take the resume byte boundary at the last possible point before
        // spawn; command construction may otherwise leave a measurable gap
        // in which another writer could append a turn to the same rollout.
        let rollout_anchor = match (rollout_root.as_deref(), resume_thread_id) {
            (Some(root), Some(thread_id)) => match open_prelaunch_rollout(root, thread_id) {
                Ok(anchor) => anchor,
                Err(()) => {
                    rollout_root = None;
                    None
                }
            },
            _ => None,
        };
        let launched_at = SystemTime::now();
        let mut child = command.spawn().map_err(|error| {
            RuntimeError::Provider(format!("failed to launch Codex CLI: {error}"))
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RuntimeError::Provider("Codex CLI stdout pipe unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| RuntimeError::Provider("Codex CLI stderr pipe unavailable".into()))?;
        let (sender, receiver) = mpsc::channel(256);
        spawn_stdout_reader(stdout, sender.clone(), self.config.max_event_bytes);
        spawn_stderr_reader(stderr, sender, self.config.max_diagnostic_bytes);
        let handle_id = format!("codex-cli-turn-{}", self.next_handle_sequence);
        let mut diagnostic = execution_diagnostic(
            &self.config,
            &self.capabilities.cli_version,
            &request,
            resume_thread_id,
        );
        diagnostic.invocation_id = handle_id.clone();
        let handle = CodexCliTurnHandleV1 {
            handle_id,
            worker_session_id: request.worker_session_id,
            turn_id: request.turn_id,
            resumed_thread_id: resume_thread_id.map(ToOwned::to_owned),
        };
        self.next_handle_sequence += 1;
        self.active_turns.insert(
            handle.handle_id.clone(),
            ActiveCodexTurn {
                child,
                turn_id: handle.turn_id.clone(),
                receiver,
                events: Vec::new(),
                normalized_events: Vec::new(),
                thread_id: resume_thread_id.map(ToOwned::to_owned),
                stderr: String::new(),
                exited: None,
                exit_observed_at: None,
                stream_closed: false,
                cancelled: false,
                cancel_event_emitted: false,
                started_at: Instant::now(),
                launched_at,
                rollout_root,
                rollout_anchor,
                actual_profile: None,
                diagnostic,
                diagnostic_persisted: false,
            },
        );
        Ok(handle)
    }

    pub async fn poll_events(
        &mut self,
        handle: &CodexCliTurnHandleV1,
        after_cursor: u64,
    ) -> Result<CodexCliEventBatchV1, RuntimeError> {
        let active = self
            .active_turns
            .get_mut(&handle.handle_id)
            .ok_or_else(|| RuntimeError::Validation("unknown Codex CLI turn handle".into()))?;
        drain_active_turn(active, &handle.turn_id, self.config.max_diagnostic_bytes).await?;
        let start = usize::try_from(after_cursor)
            .map_err(|_| RuntimeError::Validation("Codex event cursor is too large".into()))?;
        let mut events = active
            .events
            .iter()
            .skip(start)
            .cloned()
            .collect::<Vec<_>>();
        let mut normalized_events = active
            .normalized_events
            .iter()
            .skip(start)
            .cloned()
            .collect::<Vec<_>>();
        if active.cancelled && !active.cancel_event_emitted {
            active.cancel_event_emitted = true;
            let event = CodexCliEventV1 {
                event_type: "cancelled".into(),
                thread_id: active.thread_id.clone(),
                item_type: None,
                text: None,
                retry_after_seconds: None,
                bounded_summary: "Codex child process cancelled by CatDesk".into(),
            };
            let normalized = NormalizedProviderEventV1 {
                provider_id: ProviderIdV1::CodexCli.as_str().into(),
                turn_id: handle.turn_id.clone(),
                kind: NormalizedProviderEventKind::CancelAck,
                text: Some("Codex child process cancelled by CatDesk".into()),
                tool_call: None,
            };
            active.events.push(event.clone());
            active.normalized_events.push(normalized.clone());
            events.push(event);
            normalized_events.push(normalized);
        }
        let state = active_turn_state(active);
        if !matches!(state, CodexCliTurnStateV1::Running) {
            if matches!(state, CodexCliTurnStateV1::Completed) && active.actual_profile.is_none() {
                active.actual_profile = read_completed_turn_profile(active);
            }
            persist_execution_diagnostic(&self.config, active)?;
        }
        Ok(CodexCliEventBatchV1 {
            events,
            normalized_events,
            next_cursor: active.events.len() as u64,
            thread_id: active.thread_id.clone(),
            terminal: !matches!(state, CodexCliTurnStateV1::Running),
            state,
            actual_profile: active.actual_profile.clone(),
        })
    }

    /// A turn remains owned until both the direct Codex process has exited
    /// and its output readers have closed. This readback also covers a child
    /// spawned before its worker handle was returned to the controller.
    pub(crate) async fn has_unfinished_turns(&mut self) -> Result<bool, RuntimeError> {
        for active in self.active_turns.values_mut() {
            let turn_id = active.turn_id.clone();
            drain_active_turn(active, &turn_id, self.config.max_diagnostic_bytes).await?;
        }
        Ok(self
            .active_turns
            .values()
            .any(|active| active.exited.is_none() || !active.stream_closed))
    }

    pub async fn cancel(&mut self, handle: &CodexCliTurnHandleV1) -> Result<(), RuntimeError> {
        let active = self
            .active_turns
            .get_mut(&handle.handle_id)
            .ok_or_else(|| RuntimeError::Validation("unknown Codex CLI turn handle".into()))?;
        if active.exited.is_some() {
            if active.stream_closed {
                return Err(RuntimeError::Validation(
                    "cannot cancel a completed Codex CLI turn".into(),
                ));
            }
            // The process is gone but an inherited or stalled output pipe is
            // still preventing terminal readback. Cancel this pending drain
            // without attempting to kill a recycled process id.
            active.receiver.close();
            active.cancelled = true;
            return Ok(());
        }
        let pid = active
            .child
            .id()
            .ok_or_else(|| RuntimeError::Provider("Codex CLI child has no process id".into()))?;
        terminate_owned_process_tree(pid).await?;
        active.cancelled = true;
        active.exited = active.child.try_wait().map_err(|error| {
            RuntimeError::Provider(format!(
                "failed to observe cancelled Codex process: {error}"
            ))
        })?;
        if active.exited.is_some() {
            active.exit_observed_at = Some(SystemTime::now());
        }
        Ok(())
    }

    pub fn captured_thread_id(&self, handle: &CodexCliTurnHandleV1) -> Option<&str> {
        self.active_turns
            .get(&handle.handle_id)
            .and_then(|active| active.thread_id.as_deref())
    }

    pub fn turn_profile(
        &self,
        handle: &CodexCliTurnHandleV1,
    ) -> Option<&CodexCliTurnProfileEvidenceV1> {
        self.active_turns
            .get(&handle.handle_id)
            .and_then(|active| active.actual_profile.as_ref())
    }

    pub fn session_snapshot(
        &self,
        handle: &CodexCliTurnHandleV1,
    ) -> Result<CodexCliSessionSnapshotV1, RuntimeError> {
        let active = self
            .active_turns
            .get(&handle.handle_id)
            .ok_or_else(|| RuntimeError::Validation("unknown Codex CLI turn handle".into()))?;
        let codex_thread_id = active.thread_id.clone().ok_or_else(|| {
            RuntimeError::Validation("Codex CLI thread id has not been observed".into())
        })?;
        Ok(CodexCliSessionSnapshotV1 {
            codex_thread_id,
            provider_version: self.capabilities.cli_version.clone(),
            model_id: active
                .actual_profile
                .as_ref()
                .map(|evidence| evidence.model_id.clone()),
            working_directory: self.config.working_directory.clone(),
            last_completed_turn: (active.stream_closed
                && active.exited.is_some_and(|status| status.success())
                && active
                    .events
                    .iter()
                    .any(|event| event.event_type == "turn.completed"))
            .then(|| handle.turn_id.clone()),
            last_event_sequence: active.events.len() as u64,
        })
    }

    pub fn bounded_stderr(&self, handle: &CodexCliTurnHandleV1) -> Option<&str> {
        self.active_turns
            .get(&handle.handle_id)
            .map(|active| active.stderr.as_str())
    }
}

fn execution_diagnostic(
    config: &CodexCliConfigV1,
    cli_version: &str,
    request: &CodexCliTurnRequestV1,
    resume_thread_id: Option<&str>,
) -> CodexCliExecutionDiagnosticV1 {
    let canonical = config
        .working_directory
        .canonicalize()
        .unwrap_or_else(|_| config.working_directory.clone());
    let mut environment_variable_names = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|name| {
            let normalized = name.to_ascii_uppercase();
            normalized.starts_with("CODEX_")
                || normalized.starts_with("OPENAI_")
                || normalized.starts_with("CATDESK_")
                || normalized == "HTTP_PROXY"
                || normalized == "HTTPS_PROXY"
                || normalized == "NO_PROXY"
        })
        .collect::<Vec<_>>();
    environment_variable_names.sort();
    environment_variable_names.dedup();
    let mut argv = vec![
        "--sandbox".into(),
        config.sandbox.as_flag_value().into(),
        "--ask-for-approval".into(),
        config.approval_policy.as_flag_value().into(),
        "exec".into(),
    ];
    if resume_thread_id.is_some() {
        argv.extend(["resume".into(), "<captured-thread-id>".into()]);
    }
    argv.extend(["--json".into(), "--ignore-rules".into()]);
    if resume_thread_id.is_none() {
        argv.extend(["--color".into(), "never".into()]);
    }
    if let Some(model_id) = &config.model_id {
        argv.extend(["--model".into(), "<configured-model>".into()]);
        if required_effort(model_id).is_some() {
            argv.extend([
                "-c".into(),
                "model_reasoning_effort=<configured-reasoning-effort>".into(),
            ]);
        }
    }
    argv.push("<bounded-prompt-omitted>".into());
    CodexCliExecutionDiagnosticV1 {
        schema_version: DIAGNOSTIC_SCHEMA_VERSION,
        executable: config.executable.to_string_lossy().into_owned(),
        cli_version: cli_version.into(),
        argv,
        cwd: config.working_directory.to_string_lossy().into_owned(),
        canonical_working_directory: canonical.to_string_lossy().into_owned(),
        sandbox: config.sandbox.clone(),
        approval_policy: config.approval_policy.clone(),
        model_id: config.model_id.clone(),
        codex_thread_id: None,
        catdesk_session_id: request.worker_session_id.as_str().into(),
        catdesk_turn_id: request.turn_id.as_str().into(),
        invocation_id: String::new(),
        environment_variable_names,
        process_exit_code: None,
        elapsed_millis: 0,
        stderr: String::new(),
        events: Vec::new(),
    }
}

fn persist_execution_diagnostic(
    config: &CodexCliConfigV1,
    active: &mut ActiveCodexTurn,
) -> Result<(), RuntimeError> {
    if active.diagnostic_persisted {
        return Ok(());
    }
    active.diagnostic.codex_thread_id = active.thread_id.clone();
    active.diagnostic.process_exit_code = active.exited.and_then(|status| status.code());
    active.diagnostic.elapsed_millis = active.started_at.elapsed().as_millis();
    active.diagnostic.stderr = bounded_redacted_text(&active.stderr, config.max_diagnostic_bytes);
    active.diagnostic.events = active
        .events
        .iter()
        .take(MAX_DIAGNOSTIC_EVENTS)
        .map(bounded_diagnostic_event)
        .collect();
    if let Some(root) = &config.diagnostic_root {
        fs::create_dir_all(root).map_err(|error| {
            RuntimeError::Provider(format!(
                "failed to create Codex diagnostic directory: {error}"
            ))
        })?;
        let path = root.join(format!(
            "codex-{}-{}.json",
            active.diagnostic.catdesk_session_id, active.diagnostic.invocation_id
        ));
        let temporary = root.join(format!(
            ".{}.tmp",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        let mut file = File::create(&temporary).map_err(|error| {
            RuntimeError::Provider(format!(
                "failed to create bounded Codex diagnostic: {error}"
            ))
        })?;
        serde_json::to_writer_pretty(&mut file, &active.diagnostic).map_err(|error| {
            RuntimeError::Provider(format!(
                "failed to serialize bounded Codex diagnostic: {error}"
            ))
        })?;
        file.write_all(b"\n").map_err(|error| {
            RuntimeError::Provider(format!("failed to write bounded Codex diagnostic: {error}"))
        })?;
        file.sync_all().map_err(|error| {
            RuntimeError::Provider(format!("failed to sync bounded Codex diagnostic: {error}"))
        })?;
        drop(file);
        fs::rename(&temporary, &path).map_err(|error| {
            RuntimeError::Provider(format!(
                "failed to persist bounded Codex diagnostic: {error}"
            ))
        })?;
    }
    active.diagnostic_persisted = true;
    Ok(())
}

fn bounded_diagnostic_event(event: &CodexCliEventV1) -> CodexCliEventV1 {
    CodexCliEventV1 {
        event_type: event.event_type.clone(),
        thread_id: event.thread_id.clone(),
        item_type: event.item_type.clone(),
        text: event
            .text
            .as_deref()
            .map(|text| bounded_redacted_text(text, MAX_DIAGNOSTIC_EVENT_TEXT_BYTES)),
        retry_after_seconds: event.retry_after_seconds,
        bounded_summary: bounded_redacted_text(
            &event.bounded_summary,
            MAX_DIAGNOSTIC_EVENT_TEXT_BYTES,
        ),
    }
}

impl WorkerProviderV1 for CodexCliProviderV1 {
    fn provider_id(&self) -> ProviderIdV1 {
        ProviderIdV1::CodexCli
    }

    fn capabilities(&self) -> ProviderCapabilitiesV1 {
        ProviderCapabilitiesV1 {
            provider_id: self.provider_id().as_str().into(),
            provider_type: ProviderType::LocalApi,
            supports_streaming: true,
            supports_native_tool_calls: false,
            supports_session_continuity: self.capabilities.supports_resume,
            supports_cancellation: self.capabilities.supports_owned_process_cancellation,
            context_limit: MAX_PROMPT_BYTES,
            output_limit: self.config.max_event_bytes,
        }
    }

    fn create_session(
        &self,
        model_id: &str,
        worker_session_id: &WorkerSessionId,
    ) -> ProviderSessionV1 {
        ProviderSessionV1 {
            provider_id: self.provider_id().as_str().into(),
            provider_session_id: format!("codex-unbound-{}", worker_session_id.as_str()),
            model_id: model_id.into(),
            keep_alive: None,
        }
    }

    fn start_turn<'a>(
        &'a mut self,
        request: WorkerProviderTurnRequestV1,
    ) -> ProviderFuture<'a, ProviderTurnHandleV1> {
        Box::pin(async move { self.start_worker_turn(request, None).await })
    }

    fn resume_turn<'a>(
        &'a mut self,
        request: WorkerProviderTurnRequestV1,
    ) -> ProviderFuture<'a, ProviderTurnHandleV1> {
        Box::pin(async move {
            let thread_id = request.provider_session.provider_session_id.clone();
            if thread_id.starts_with("codex-unbound-") {
                return Err(RuntimeError::Validation(
                    "Codex continuation requires a captured provider thread id".into(),
                ));
            }
            self.start_worker_turn(request, Some(thread_id)).await
        })
    }

    fn poll_events<'a>(
        &'a mut self,
        handle: &'a ProviderTurnHandleV1,
        after_cursor: u64,
    ) -> ProviderFuture<'a, ProviderEventBatchV1> {
        Box::pin(async move {
            let codex_handle = self
                .worker_handles
                .get(&handle.handle_id)
                .cloned()
                .ok_or_else(|| RuntimeError::Validation("unknown Codex worker handle".into()))?;
            let mut batch =
                CodexCliProviderV1::poll_events(self, &codex_handle, after_cursor).await?;
            if matches!(batch.state, CodexCliTurnStateV1::RateLimited)
                && !batch
                    .normalized_events
                    .iter()
                    .any(|event| event.kind == NormalizedProviderEventKind::TerminalError)
            {
                batch.normalized_events.push(NormalizedProviderEventV1 {
                    provider_id: self.provider_id().as_str().into(),
                    turn_id: handle.turn_id.clone(),
                    kind: NormalizedProviderEventKind::TerminalError,
                    text: Some("Codex CLI rate limited".into()),
                    tool_call: None,
                });
            }
            if matches!(batch.state, CodexCliTurnStateV1::Failed)
                && !batch
                    .normalized_events
                    .iter()
                    .any(|event| event.kind == NormalizedProviderEventKind::TerminalError)
            {
                let stderr = self
                    .active_turns
                    .get(&codex_handle.handle_id)
                    .map(|active| active.stderr.as_str())
                    .filter(|text| !text.trim().is_empty())
                    .map(|text| bounded_redacted_text(text, self.config.max_diagnostic_bytes))
                    .unwrap_or_else(|| "Codex CLI turn failed without structured error".into());
                batch.normalized_events.push(NormalizedProviderEventV1 {
                    provider_id: self.provider_id().as_str().into(),
                    turn_id: handle.turn_id.clone(),
                    kind: NormalizedProviderEventKind::TerminalError,
                    text: Some(stderr),
                    tool_call: None,
                });
            }
            Ok(ProviderEventBatchV1 {
                events: batch.normalized_events,
                next_cursor: batch.next_cursor,
                terminal: batch.terminal,
                provider_session_id: batch.thread_id.or(codex_handle.resumed_thread_id),
                codex_turn_profile: batch.actual_profile,
            })
        })
    }

    fn cancel<'a>(&'a mut self, handle: &'a ProviderTurnHandleV1) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            let codex_handle = self
                .worker_handles
                .get(&handle.handle_id)
                .cloned()
                .ok_or_else(|| RuntimeError::Validation("unknown Codex worker handle".into()))?;
            CodexCliProviderV1::cancel(self, &codex_handle).await
        })
    }

    fn status<'a>(&'a self) -> ProviderFuture<'a, ProviderHealthV1> {
        Box::pin(async move {
            Ok(ProviderHealthV1 {
                provider_id: self.provider_id().as_str().into(),
                status: ProviderHealthStatus::Available,
                available_models: self.config.model_id.iter().cloned().collect(),
                detail: "Codex CLI capabilities were discovered before controller launch".into(),
            })
        })
    }
}

impl CodexCliProviderV1 {
    async fn start_worker_turn(
        &mut self,
        request: WorkerProviderTurnRequestV1,
        resume_thread_id: Option<String>,
    ) -> Result<ProviderTurnHandleV1, RuntimeError> {
        if request.provider_session.provider_id != self.provider_id().as_str()
            || !request.turn.tool_definitions.is_empty()
        {
            return Err(RuntimeError::Validation(
                "Codex autonomous turns require a matching session and no CatDesk tool definitions"
                    .into(),
            ));
        }
        if self
            .config
            .model_id
            .as_deref()
            .is_some_and(|configured| configured != request.turn.model_id)
        {
            return Err(RuntimeError::Validation(
                "Codex worker model does not match the discovered local configuration".into(),
            ));
        }
        let prompt = autonomous_prompt(&request)?;
        let cli_request = CodexCliTurnRequestV1 {
            worker_session_id: request.turn.worker_session_id.clone(),
            turn_id: request.turn.turn_id.clone(),
            prompt,
        };
        let codex_handle = match resume_thread_id.as_deref() {
            Some(thread_id) => {
                CodexCliProviderV1::resume_turn(self, thread_id, cli_request).await?
            }
            None => CodexCliProviderV1::start_turn(self, cli_request).await?,
        };
        let handle = ProviderTurnHandleV1 {
            provider_id: self.provider_id(),
            handle_id: codex_handle.handle_id.clone(),
            provider_session_id: resume_thread_id.unwrap_or_else(|| {
                format!("codex-unbound-{}", request.turn.worker_session_id.as_str())
            }),
            worker_session_id: request.turn.worker_session_id,
            turn_id: request.turn.turn_id,
        };
        self.worker_handles
            .insert(handle.handle_id.clone(), codex_handle);
        Ok(handle)
    }
}

fn autonomous_prompt(request: &WorkerProviderTurnRequestV1) -> Result<String, RuntimeError> {
    let context = serde_json::to_string(&request.turn.context_json).map_err(|error| {
        RuntimeError::Validation(format!("failed to serialize Codex task context: {error}"))
    })?;
    let instructions = request
        .history
        .iter()
        .map(|message| format!("{}:\n{}", message.role, message.content))
        .collect::<Vec<_>>();
    let prompt = format!(
        "You are the coding worker for an approved CatDesk autonomous task. Work directly in the current workspace now.\n\nApproved task context: {context}\n\nInstructions:\n{}\n\nHard boundaries:\n- Do not use CatDesk MCP tools; none are available to you.\n- Operate only in the current approved workspace.\n- Do not change Git branches, publish Git changes, access credentials, or use a fallback provider.\n- Perform the requested repository work before replying.\n- CatDesk independently runs verification; do not claim verified completion.",
        instructions.join("\n\n")
    );
    if prompt.len() > MAX_PROMPT_BYTES {
        return Err(RuntimeError::BudgetExceeded(
            "Codex autonomous prompt exceeds bounded adapter input".into(),
        ));
    }
    Ok(prompt)
}

#[derive(Clone, Debug)]
struct CommandOutputV1 {
    stdout: String,
}

async fn run_bounded_command<const N: usize>(
    config: &CodexCliConfigV1,
    arguments: [&str; N],
    max_bytes: usize,
) -> Result<CommandOutputV1, RuntimeError> {
    let mut command = Command::new(&config.executable);
    command
        .args(arguments)
        .current_dir(&config.working_directory)
        .stdin(std::process::Stdio::null());
    apply_operator_codex_context(&mut command, config);
    let output = command
        .output()
        .await
        .map_err(|error| RuntimeError::Provider(format!("failed to execute Codex CLI: {error}")))?;
    let stdout = bounded_redacted_text(&String::from_utf8_lossy(&output.stdout), max_bytes);
    let stderr = bounded_redacted_text(&String::from_utf8_lossy(&output.stderr), max_bytes);
    if !output.status.success() {
        return Err(RuntimeError::Provider(format!(
            "Codex CLI command failed with {:?}: {stderr}",
            output.status.code()
        )));
    }
    Ok(CommandOutputV1 { stdout })
}

/// Copies the operator-selected Codex home into the child environment. Auth
/// and config files remain opaque to CatDesk; native session rollout readback
/// is limited to the separate bounded `sessions/` scanner above.
fn apply_operator_codex_context(command: &mut Command, config: &CodexCliConfigV1) {
    if let Some(codex_home) = &config.operator_codex_home {
        command.env("CODEX_HOME", codex_home);
    }
}

fn codex_rollout_root(config: &CodexCliConfigV1) -> Option<PathBuf> {
    native_codex_rollout_root(config.operator_codex_home.as_deref())
}

fn native_codex_rollout_root(operator_codex_home: Option<&Path>) -> Option<PathBuf> {
    let home = operator_codex_home
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("CODEX_HOME").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|home| PathBuf::from(home).join(".codex"))
        })?;
    Some(home.join("sessions"))
}

/// Capture the only existing rollout for an exact resumed thread immediately
/// before launching the native child. Unknown, missing, or ambiguous layouts
/// provide no evidence; callers may still run a turn with UNVERIFIED profile.
pub(crate) fn prepare_resumed_native_turn_readback(
    operator_codex_home: Option<&Path>,
    bound_thread_id: &str,
) -> Option<CodexCliNativeTurnReadbackV1> {
    let rollout_root = native_codex_rollout_root(operator_codex_home)?;
    let anchor = open_prelaunch_rollout(&rollout_root, bound_thread_id).ok()??;
    Some(CodexCliNativeTurnReadbackV1 {
        rollout_root,
        bound_thread_id: bound_thread_id.to_owned(),
        anchor,
    })
}

impl CodexCliNativeTurnReadbackV1 {
    /// Return native model/effort metadata only when this held prelaunch file
    /// grew with one completed turn inside the observed child process window
    /// and the child's bounded stdout independently completed on this thread.
    pub(crate) fn read_completed(
        &self,
        launched_at: SystemTime,
        exit_observed_at: SystemTime,
        stdout: &CodexCliNativeStdoutEvidenceV1,
    ) -> Option<CodexCliTurnProfileEvidenceV1> {
        if !stdout.completed_for(&self.bound_thread_id)
            || self.anchor.file.metadata().ok()?.len() <= self.anchor.length
        {
            return None;
        }
        let paths = rollout_paths(&self.rollout_root, &self.bound_thread_id).ok()?;
        let [current_path] = paths.as_slice() else {
            return None;
        };
        if current_path != &self.anchor.path
            || File::open(current_path)
                .ok()?
                .metadata()
                .ok()?
                .created()
                .ok()?
                > launched_at
        {
            return None;
        }
        read_postlaunch_rollout_profile(
            &self.rollout_root,
            &self.bound_thread_id,
            launched_at,
            exit_observed_at,
            Some(&self.anchor),
        )
    }
}

fn valid_codex_thread_id(thread_id: &str) -> bool {
    uuid::Uuid::parse_str(thread_id).is_ok_and(|parsed| parsed.to_string() == thread_id)
}

/// Enumerate only Codex's three date-directory levels and accept exact UUID
/// filenames. Every candidate resolves under the canonical sessions root;
/// unknown layouts and excessive inventories fail closed.
fn rollout_paths(root: &Path, thread_id: &str) -> Result<Vec<PathBuf>, ()> {
    if !valid_codex_thread_id(thread_id) {
        return Err(());
    }
    let canonical_root = fs::canonicalize(root).map_err(|_| ())?;
    let mut paths = Vec::new();
    let mut entries = 0;
    fn visit(
        directory: &Path,
        depth: u8,
        canonical_root: &Path,
        thread_id: &str,
        paths: &mut Vec<PathBuf>,
        entries: &mut usize,
    ) -> Result<(), ()> {
        for entry in fs::read_dir(directory).map_err(|_| ())? {
            let entry = entry.map_err(|_| ())?;
            *entries += 1;
            if *entries > MAX_ROLLOUT_DIRECTORY_ENTRIES {
                return Err(());
            }
            let kind = entry.file_type().map_err(|_| ())?;
            if depth > 0 {
                if kind.is_dir() {
                    visit(
                        &entry.path(),
                        depth - 1,
                        canonical_root,
                        thread_id,
                        paths,
                        entries,
                    )?;
                }
            } else if kind.is_file() {
                let filename = entry.file_name();
                let filename = filename.to_str().ok_or(())?;
                if filename.starts_with("rollout-")
                    && filename.ends_with(&format!("-{thread_id}.jsonl"))
                {
                    let path = fs::canonicalize(entry.path()).map_err(|_| ())?;
                    if !path.starts_with(canonical_root) {
                        return Err(());
                    }
                    paths.push(path);
                    if paths.len() > 2 {
                        return Err(());
                    }
                }
            }
        }
        Ok(())
    }
    visit(
        &canonical_root,
        3,
        &canonical_root,
        thread_id,
        &mut paths,
        &mut entries,
    )?;
    Ok(paths)
}

fn open_prelaunch_rollout(root: &Path, thread_id: &str) -> Result<Option<RolloutAnchor>, ()> {
    let paths = rollout_paths(root, thread_id)?;
    let [path] = paths.as_slice() else {
        return if paths.is_empty() { Ok(None) } else { Err(()) };
    };
    let file = File::open(path).map_err(|_| ())?;
    let length = file.metadata().map_err(|_| ())?.len();
    Ok(Some(RolloutAnchor {
        path: path.clone(),
        file,
        length,
    }))
}

/// Read just enough of a completed child rollout to bind its native turn ID
/// and selected model/effort. Transcript text is never retained or logged.
/// Codex stdout names the thread but not the native turn ID, so the retained
/// pre-launch offset and controller's single-writer ownership are both needed
/// to associate appended rollout records with this child process.
fn read_completed_turn_profile(active: &ActiveCodexTurn) -> Option<CodexCliTurnProfileEvidenceV1> {
    let stdout_threads = active
        .events
        .iter()
        .filter(|event| event.event_type == "thread.started")
        .collect::<Vec<_>>();
    let [stdout_thread] = stdout_threads.as_slice() else {
        return None;
    };
    let thread_id = stdout_thread.thread_id.as_deref()?;
    if !valid_codex_thread_id(thread_id)
        || active.thread_id.as_deref() != Some(thread_id)
        || active
            .events
            .iter()
            .filter(|event| event.event_type == "turn.started")
            .count()
            != 1
        || active
            .events
            .iter()
            .filter(|event| event.event_type == "turn.completed")
            .count()
            != 1
        || active.events.iter().any(|event| {
            matches!(
                event.event_type.as_str(),
                "turn.failed" | "parse.error" | "oversized.event"
            )
        })
    {
        return None;
    }
    read_postlaunch_rollout_profile(
        active.rollout_root.as_deref()?,
        thread_id,
        active.launched_at,
        active.exit_observed_at?,
        active.rollout_anchor.as_ref(),
    )
}

fn read_postlaunch_rollout_profile(
    root: &Path,
    thread_id: &str,
    launched_at: SystemTime,
    exit_observed_at: SystemTime,
    anchor: Option<&RolloutAnchor>,
) -> Option<CodexCliTurnProfileEvidenceV1> {
    let paths = rollout_paths(root, thread_id).ok()?;
    let appended_anchor = match anchor {
        Some(anchor) => {
            let length = anchor.file.metadata().ok()?.len();
            if length < anchor.length {
                return None;
            }
            (length > anchor.length).then_some(anchor)
        }
        None => None,
    };
    let mut new_files = Vec::new();
    for path in paths {
        let file = File::open(&path).ok()?;
        let created = file.metadata().ok()?.created().ok()?;
        if created < launched_at {
            continue;
        }
        // A same-path replacement created after launch is a second candidate,
        // even when a retained pre-launch handle also received new bytes.
        new_files.push(file);
        if new_files.len() > 1 {
            return None;
        }
    }
    match (appended_anchor, new_files.as_slice()) {
        (Some(anchor), []) => read_rollout_profile_in_window(
            &anchor.file,
            anchor.length,
            thread_id,
            launched_at,
            exit_observed_at,
        ),
        (None, [file]) => {
            read_rollout_profile_in_window(file, 0, thread_id, launched_at, exit_observed_at)
        }
        _ => None,
    }
}

fn bounded_rollout_line<R: BufRead>(
    reader: &mut R,
    scanned: &mut u64,
) -> Result<Option<Vec<u8>>, ()> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(|_| ())?;
        if available.is_empty() {
            return if line.is_empty() { Ok(None) } else { Err(()) };
        }
        let count = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        *scanned = scanned.checked_add(count as u64).ok_or(())?;
        if *scanned > MAX_ROLLOUT_SCAN_BYTES || line.len() + count > MAX_ROLLOUT_LINE_BYTES {
            return Err(());
        }
        let has_newline = available[count - 1] == b'\n';
        line.extend_from_slice(&available[..count]);
        reader.consume(count);
        if has_newline {
            return Ok(Some(line));
        }
    }
}

#[cfg(test)]
fn read_rollout_profile(
    file: &File,
    offset: u64,
    thread_id: &str,
) -> Option<CodexCliTurnProfileEvidenceV1> {
    read_rollout_profile_inner(file, offset, thread_id, None)
}

fn read_rollout_profile_in_window(
    file: &File,
    offset: u64,
    thread_id: &str,
    launched_at: SystemTime,
    exit_observed_at: SystemTime,
) -> Option<CodexCliTurnProfileEvidenceV1> {
    let launch = launched_at.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let exit = exit_observed_at.duration_since(UNIX_EPOCH).ok()?.as_secs();
    if exit < launch {
        return None;
    }
    read_rollout_profile_inner(
        file,
        offset,
        thread_id,
        Some(NativeTurnWindow {
            earliest_epoch: launch.saturating_sub(NATIVE_TIMESTAMP_SKEW_SECS),
            latest_epoch: exit.saturating_add(NATIVE_TIMESTAMP_SKEW_SECS),
            earliest_event: launched_at.checked_sub(NATIVE_EVENT_TIMESTAMP_SKEW)?,
            latest_event: exit_observed_at.checked_add(NATIVE_EVENT_TIMESTAMP_SKEW)?,
        }),
    )
}

#[derive(Clone, Copy)]
struct NativeTurnWindow {
    earliest_epoch: u64,
    latest_epoch: u64,
    earliest_event: SystemTime,
    latest_event: SystemTime,
}

fn parse_rollout_event_timestamp(value: &Value) -> Option<SystemTime> {
    let bytes = value.get("timestamp")?.as_str()?.as_bytes();
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let decimal = |digits: &[u8]| -> Option<u32> {
        digits.iter().try_fold(0_u32, |number, digit| {
            digit
                .is_ascii_digit()
                .then(|| number * 10 + u32::from(digit - b'0'))
        })
    };
    let year = i32::try_from(decimal(&bytes[0..4])?).ok()?;
    let month = u8::try_from(decimal(&bytes[5..7])?).ok()?;
    let day = u8::try_from(decimal(&bytes[8..10])?).ok()?;
    let hour = u8::try_from(decimal(&bytes[11..13])?).ok()?;
    let minute = u8::try_from(decimal(&bytes[14..16])?).ok()?;
    let second = u8::try_from(decimal(&bytes[17..19])?).ok()?;
    let nanos = if bytes.len() == 20 && bytes[19] == b'Z' {
        0
    } else {
        if bytes[19] != b'.' || bytes.last() != Some(&b'Z') {
            return None;
        }
        let fraction = &bytes[20..bytes.len() - 1];
        if fraction.is_empty() || fraction.len() > 9 {
            return None;
        }
        decimal(fraction)? * 10_u32.pow(u32::try_from(9 - fraction.len()).ok()?)
    };
    let date =
        time::Date::from_calendar_date(year, time::Month::try_from(month).ok()?, day).ok()?;
    let clock = time::Time::from_hms_nano(hour, minute, second, nanos).ok()?;
    let unix_nanos = time::PrimitiveDateTime::new(date, clock)
        .assume_utc()
        .unix_timestamp_nanos();
    if unix_nanos < 0 {
        return None;
    }
    let seconds = u64::try_from(unix_nanos / 1_000_000_000).ok()?;
    let remainder = u64::try_from(unix_nanos % 1_000_000_000).ok()?;
    UNIX_EPOCH
        .checked_add(Duration::from_secs(seconds))?
        .checked_add(Duration::from_nanos(remainder))
}

fn read_rollout_profile_inner(
    file: &File,
    offset: u64,
    thread_id: &str,
    time_window: Option<NativeTurnWindow>,
) -> Option<CodexCliTurnProfileEvidenceV1> {
    let mut reader = SyncBufReader::new(file.try_clone().ok()?);
    reader.seek(SeekFrom::Start(0)).ok()?;
    let mut scanned = 0;
    let first = bounded_rollout_line(&mut reader, &mut scanned).ok()??;
    let first: Value = serde_json::from_slice(&first).ok()?;
    if first.get("type")?.as_str()? != "session_meta"
        || first.pointer("/payload/id")?.as_str()? != thread_id
    {
        return None;
    }
    if offset != 0 {
        if offset < reader.stream_position().ok()? || offset > file.metadata().ok()?.len() {
            return None;
        }
        reader.seek(SeekFrom::Start(offset)).ok()?;
        scanned = 0;
    }
    let mut turn_id: Option<String> = None;
    let mut model_id: Option<String> = None;
    let mut reasoning_effort: Option<String> = None;
    let mut started_epoch: Option<u64> = None;
    let mut started_event_time: Option<SystemTime> = None;
    let mut context_count = 0;
    let mut completed = false;
    let mut hasher = Sha256::new();
    hasher.update(b"codex-cli-native-turn-profile-v1\n");
    while let Some(line) = bounded_rollout_line(&mut reader, &mut scanned).ok()? {
        let value: Value = serde_json::from_slice(&line).ok()?;
        match (
            value.get("type")?.as_str()?,
            value.pointer("/payload/type").and_then(Value::as_str),
        ) {
            ("event_msg", Some("task_started")) => {
                if turn_id.is_some() {
                    return None;
                }
                let id = value.pointer("/payload/turn_id")?.as_str()?;
                if !valid_codex_thread_id(id)
                    || value.pointer("/payload/root_turn_id")?.as_str()? != id
                {
                    return None;
                }
                if let Some(window) = time_window {
                    let started = value.pointer("/payload/started_at")?.as_u64()?;
                    let event_time = parse_rollout_event_timestamp(&value)?;
                    if started < window.earliest_epoch
                        || started > window.latest_epoch
                        || event_time < window.earliest_event
                        || event_time > window.latest_event
                    {
                        return None;
                    }
                    started_epoch = Some(started);
                    started_event_time = Some(event_time);
                }
                turn_id = Some(id.to_owned());
                hash_rollout_metadata(
                    &mut hasher,
                    &serde_json::json!({
                        "record":"task_started",
                        "turn_id":id,
                        "root_turn_id":value.pointer("/payload/root_turn_id"),
                        "started_at":value.pointer("/payload/started_at"),
                    }),
                );
            }
            ("turn_context", _) if turn_id.is_some() && !completed => {
                if value.pointer("/payload/turn_id")?.as_str()? != turn_id.as_deref()?
                    || value.pointer("/payload/root_turn_id")?.as_str()? != turn_id.as_deref()?
                {
                    return None;
                }
                let model = value.pointer("/payload/model")?.as_str()?;
                let effort = value.pointer("/payload/effort")?.as_str()?;
                if model.is_empty() || effort.is_empty() || model.len() > 128 || effort.len() > 32 {
                    return None;
                }
                if model_id
                    .as_deref()
                    .is_some_and(|previous| previous != model)
                    || reasoning_effort
                        .as_deref()
                        .is_some_and(|previous| previous != effort)
                {
                    return None;
                }
                model_id = Some(model.to_owned());
                reasoning_effort = Some(effort.to_owned());
                context_count += 1;
                if context_count > 32 {
                    return None;
                }
                hash_rollout_metadata(
                    &mut hasher,
                    &serde_json::json!({
                        "record":"turn_context",
                        "turn_id":turn_id.as_deref(),
                        "root_turn_id":value.pointer("/payload/root_turn_id"),
                        "model":model,
                        "effort":effort,
                    }),
                );
            }
            ("event_msg", Some("task_complete")) => {
                let completion_root = match value.pointer("/payload/root_turn_id") {
                    Some(root) => Some(root.as_str()?),
                    None => None,
                };
                if completed
                    || context_count == 0
                    || value.pointer("/payload/turn_id")?.as_str()? != turn_id.as_deref()?
                    || completion_root.is_some_and(|root| Some(root) != turn_id.as_deref())
                {
                    return None;
                }
                if let Some(window) = time_window {
                    let completion_start = value.pointer("/payload/started_at")?.as_u64()?;
                    let completion_end = value.pointer("/payload/completed_at")?.as_u64()?;
                    let event_time = parse_rollout_event_timestamp(&value)?;
                    if Some(completion_start) != started_epoch
                        || completion_end < completion_start
                        || completion_end > window.latest_epoch
                        || event_time < started_event_time?
                        || event_time > window.latest_event
                    {
                        return None;
                    }
                }
                completed = true;
                hash_rollout_metadata(
                    &mut hasher,
                    &serde_json::json!({
                        "record":"task_complete",
                        "turn_id":turn_id.as_deref(),
                        "root_turn_id":completion_root,
                        "started_at":value.pointer("/payload/started_at"),
                        "completed_at":value.pointer("/payload/completed_at"),
                    }),
                );
            }
            ("turn_context", _) if completed => return None,
            ("turn_context", _) => return None,
            _ => {}
        }
    }
    if !completed {
        return None;
    }
    Some(CodexCliTurnProfileEvidenceV1 {
        codex_thread_id: thread_id.to_owned(),
        codex_turn_id: turn_id?,
        model_id: model_id?,
        reasoning_effort: reasoning_effort?,
        evidence_sha256: format!("{:x}", hasher.finalize()),
    })
}

fn hash_rollout_metadata(hasher: &mut Sha256, projection: &Value) {
    let bytes = projection.to_string();
    hasher.update((bytes.len() as u32).to_be_bytes());
    hasher.update(bytes.as_bytes());
}

fn spawn_stdout_reader(
    stdout: tokio::process::ChildStdout,
    sender: mpsc::Sender<StreamMessage>,
    max_event_bytes: usize,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) if line.len() <= max_event_bytes => {
                    let message = match parse_codex_jsonl_event(&line, max_event_bytes) {
                        Ok(event) => StreamMessage::Event(event),
                        Err(error) => StreamMessage::ParseError(format!("{error:?}")),
                    };
                    if sender.send(message).await.is_err() {
                        return;
                    }
                }
                Ok(Some(line)) => {
                    if sender
                        .send(StreamMessage::OversizedEvent(line.len()))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(None) => return,
                Err(error) => {
                    let _ = sender
                        .send(StreamMessage::ParseError(format!(
                            "failed to read Codex JSONL output: {error}"
                        )))
                        .await;
                    return;
                }
            }
        }
    });
}

fn spawn_stderr_reader(
    stderr: tokio::process::ChildStderr,
    sender: mpsc::Sender<StreamMessage>,
    max_bytes: usize,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let line = bounded_redacted_text(&line, max_bytes);
                    if sender.send(StreamMessage::Stderr(line)).await.is_err() {
                        return;
                    }
                }
                Ok(None) => return,
                Err(error) => {
                    let _ = sender
                        .send(StreamMessage::Stderr(format!(
                            "failed to read Codex stderr: {error}"
                        )))
                        .await;
                    return;
                }
            }
        }
    });
}

async fn drain_active_turn(
    active: &mut ActiveCodexTurn,
    turn_id: &TurnId,
    max_diagnostic_bytes: usize,
) -> Result<(), RuntimeError> {
    loop {
        match active.receiver.try_recv() {
            Ok(message) => record_stream_message(active, turn_id, max_diagnostic_bytes, message),
            Err(TryRecvError::Disconnected) => {
                active.stream_closed = true;
                break;
            }
            Err(TryRecvError::Empty) => break,
        }
    }
    if active.exited.is_none() {
        active.exited = active.child.try_wait().map_err(|error| {
            RuntimeError::Provider(format!("failed to poll Codex CLI process: {error}"))
        })?;
        if active.exited.is_some() {
            active.exit_observed_at = Some(SystemTime::now());
        }
    }
    if active.exited.is_some() && !active.stream_closed {
        // The child can exit before its stdout/stderr reader tasks enqueue the
        // final JSONL lines. A bounded wait gives those readers a chance to
        // finish. If it expires, leave the turn running for a later poll;
        // never report a completed turn with an incomplete event stream.
        let deadline = tokio::time::Instant::now() + POST_EXIT_STREAM_DRAIN_WAIT;
        loop {
            match tokio::time::timeout_at(deadline, active.receiver.recv()).await {
                Ok(Some(message)) => {
                    record_stream_message(active, turn_id, max_diagnostic_bytes, message)
                }
                Ok(None) => {
                    active.stream_closed = true;
                    break;
                }
                Err(_) => break,
            }
        }
    }
    Ok(())
}

fn record_stream_message(
    active: &mut ActiveCodexTurn,
    turn_id: &TurnId,
    max_diagnostic_bytes: usize,
    message: StreamMessage,
) {
    match message {
        StreamMessage::Event(event) => {
            if let Some(thread_id) = &event.thread_id {
                active.thread_id = Some(thread_id.clone());
            }
            active
                .normalized_events
                .push(normalize_codex_event(&event, turn_id));
            active.events.push(event);
        }
        StreamMessage::OversizedEvent(byte_count) => {
            let summary = format!(
                "Codex JSONL event body omitted because it exceeded the configured byte limit ({} bytes)",
                byte_count
            );
            let event = CodexCliEventV1 {
                event_type: "oversized.event".into(),
                thread_id: active.thread_id.clone(),
                item_type: None,
                text: None,
                retry_after_seconds: None,
                bounded_summary: bounded_redacted_text(&summary, max_diagnostic_bytes),
            };
            active.normalized_events.push(NormalizedProviderEventV1 {
                provider_id: ProviderIdV1::CodexCli.as_str().into(),
                turn_id: turn_id.clone(),
                kind: NormalizedProviderEventKind::TextDelta,
                text: Some(event.bounded_summary.clone()),
                tool_call: None,
            });
            active.events.push(event);
        }
        StreamMessage::ParseError(error) => {
            let error = bounded_redacted_text(&error, max_diagnostic_bytes);
            active.normalized_events.push(NormalizedProviderEventV1 {
                provider_id: ProviderIdV1::CodexCli.as_str().into(),
                turn_id: turn_id.clone(),
                kind: NormalizedProviderEventKind::TerminalError,
                text: Some(error.clone()),
                tool_call: None,
            });
            active.events.push(CodexCliEventV1 {
                event_type: "parse.error".into(),
                thread_id: active.thread_id.clone(),
                item_type: None,
                text: None,
                retry_after_seconds: None,
                bounded_summary: error,
            });
        }
        StreamMessage::Stderr(line) => {
            append_bounded(&mut active.stderr, &line, max_diagnostic_bytes)
        }
    }
}

fn active_turn_state(active: &ActiveCodexTurn) -> CodexCliTurnStateV1 {
    if active.exited.is_none() || !active.stream_closed {
        CodexCliTurnStateV1::Running
    } else if active.cancelled {
        CodexCliTurnStateV1::Cancelled
    } else if active.events.iter().any(|event| {
        event.retry_after_seconds.is_some() || is_rate_limit_text(&event.bounded_summary)
    }) || is_rate_limit_text(&active.stderr)
    {
        CodexCliTurnStateV1::RateLimited
    } else if let Some(status) = active.exited {
        if status.success() {
            CodexCliTurnStateV1::Completed
        } else {
            CodexCliTurnStateV1::Failed
        }
    } else {
        unreachable!("terminal Codex state requires an observed process exit")
    }
}

fn normalize_codex_event(event: &CodexCliEventV1, turn_id: &TurnId) -> NormalizedProviderEventV1 {
    let kind = match event.event_type.as_str() {
        "item.started" if event.item_type.as_deref() == Some("command_execution") => {
            NormalizedProviderEventKind::TextDelta
        }
        "item.completed" if event.item_type.as_deref() == Some("command_execution") => {
            NormalizedProviderEventKind::TextDelta
        }
        "turn.completed" => NormalizedProviderEventKind::CompletionClaim,
        "turn.failed" | "error" => NormalizedProviderEventKind::TerminalError,
        _ => NormalizedProviderEventKind::TextDelta,
    };
    NormalizedProviderEventV1 {
        provider_id: ProviderIdV1::CodexCli.as_str().into(),
        turn_id: turn_id.clone(),
        kind,
        text: event
            .text
            .clone()
            .or_else(|| Some(event.bounded_summary.clone())),
        tool_call: None,
    }
}

pub fn parse_codex_jsonl_event(
    line: &str,
    max_event_bytes: usize,
) -> Result<CodexCliEventV1, RuntimeError> {
    if line.len() > max_event_bytes {
        return Err(RuntimeError::BudgetExceeded(
            "Codex JSONL event exceeds configured byte limit".into(),
        ));
    }
    let value: Value = serde_json::from_str(line).map_err(|error| {
        RuntimeError::MalformedOutput(format!("invalid Codex JSONL event: {error}"))
    })?;
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| RuntimeError::MalformedOutput("Codex event omitted type".into()))?
        .to_string();
    let thread_id = value
        .get("thread_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let item = value.get("item");
    let item_type = item
        .and_then(|item| item.get("type"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let text = item
        .and_then(|item| item.get("text"))
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .map(|text| bounded_redacted_text(text, max_event_bytes));
    let retry_after_seconds = value
        .get("retry_after")
        .or_else(|| value.get("retryAfter"))
        .and_then(Value::as_u64);
    let item_status = item
        .and_then(|item| item.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("<none>");
    let item_command = item
        .and_then(|item| item.get("command"))
        .and_then(Value::as_str)
        .map(|command| bounded_redacted_text(command, 2_048))
        .unwrap_or_else(|| "<none>".into());
    let change_count = item
        .and_then(|item| item.get("changes"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let bounded_summary = bounded_redacted_text(
        &format!(
            "Codex event type={event_type}; itemType={}; itemStatus={item_status}; changeCount={change_count}; command={item_command}; retryAfter={}",
            item_type.as_deref().unwrap_or("<none>"),
            retry_after_seconds
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<none>".into())
        ),
        max_event_bytes.min(DEFAULT_MAX_DIAGNOSTIC_BYTES),
    );
    Ok(CodexCliEventV1 {
        event_type,
        thread_id,
        item_type,
        text,
        retry_after_seconds,
        bounded_summary,
    })
}

pub fn classify_rate_limit(event: &CodexCliEventV1) -> Option<u64> {
    if let Some(retry_after) = event.retry_after_seconds {
        return Some(retry_after);
    }
    if is_rate_limit_text(&event.bounded_summary)
        || event.text.as_deref().is_some_and(is_rate_limit_text)
    {
        return Some(0);
    }
    None
}

async fn terminate_owned_process_tree(pid: u32) -> Result<(), RuntimeError> {
    #[cfg(windows)]
    {
        let output = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(std::process::Stdio::null())
            .output()
            .await
            .map_err(|error| {
                RuntimeError::Provider(format!("failed to cancel Codex process: {error}"))
            })?;
        if !output.status.success() {
            return Err(RuntimeError::Provider(format!(
                "owned Codex process cancellation failed with {:?}",
                output.status.code()
            )));
        }
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        return Err(RuntimeError::Provider(
            "owned Codex process-tree cancellation is not implemented on this platform".into(),
        ));
    }
    Ok(())
}

fn append_bounded(target: &mut String, text: &str, max_bytes: usize) {
    if target.len() >= max_bytes {
        return;
    }
    if !target.is_empty() {
        target.push('\n');
    }
    let remaining = max_bytes.saturating_sub(target.len());
    target.push_str(&bounded_redacted_text(text, remaining));
}

fn bounded_redacted_text(value: &str, max_bytes: usize) -> String {
    let redacted = secret_pattern()
        .replace_all(value, "$1=<redacted>")
        .into_owned();
    let redacted = api_key_pattern().replace_all(&redacted, "<redacted-api-key>");
    truncate_utf8(&redacted, max_bytes)
}

fn secret_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?i)\b(token|password|secret|api[_-]?key)\s*[:=]\s*[^\s]+")
            .expect("valid secret redaction regex")
    })
}

fn api_key_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"\bsk-[A-Za-z0-9_-]{20,}").expect("valid API key redaction regex")
    })
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.into();
    }
    let mut end = 0;
    for (index, character) in value.char_indices() {
        if index + character.len_utf8() > max_bytes {
            break;
        }
        end = index + character.len_utf8();
    }
    format!("{}...", &value[..end])
}

fn is_rate_limit_text(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("rate limit")
        || value.contains("too many requests")
        || value.contains("http 429")
        || value.contains("status 429")
}

pub fn classify_codex_availability_failure(value: &str) -> Option<CodexAvailabilityFailureV1> {
    let value = bounded_redacted_text(value, DEFAULT_MAX_DIAGNOSTIC_BYTES).to_ascii_lowercase();
    if [
        "usage limit reached",
        "you've hit your usage limit",
        "you have hit your usage limit",
        "credits exhausted",
        "insufficient credits",
        "plan limit reached",
        "plan allowance exhausted",
        "codex allowance exhausted",
    ]
    .iter()
    .any(|signal| value.contains(signal))
    {
        Some(CodexAvailabilityFailureV1::CreditsExhausted)
    } else if is_rate_limit_text(&value) {
        Some(CodexAvailabilityFailureV1::TransientRateLimited)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegated::contracts::RunId;
    use crate::delegated::journal::ToolMutationKind;
    use crate::delegated::runtime::{ProviderMessageV1, ProviderTurnRequestV1, ToolDefinitionV1};

    fn direct_executable() -> PathBuf {
        std::env::current_exe().expect("test executable")
    }

    fn config() -> CodexCliConfigV1 {
        CodexCliConfigV1::new(
            direct_executable(),
            std::env::current_dir().expect("working directory"),
        )
        .expect("config")
    }

    async fn exited_test_turn(receiver: mpsc::Receiver<StreamMessage>) -> ActiveCodexTurn {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd.exe");
            command.args(["/C", "exit", "0"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "exit 0"]);
            command
        };
        let mut child = command.spawn().expect("spawn test child");
        let exited = child.wait().await.expect("wait for test child");
        assert!(exited.success());
        let request = CodexCliTurnRequestV1 {
            worker_session_id: WorkerSessionId::new("reader-test-worker").expect("worker"),
            turn_id: TurnId::new("reader-test-turn").expect("turn"),
            prompt: "test".into(),
        };
        ActiveCodexTurn {
            child,
            turn_id: request.turn_id.clone(),
            receiver,
            events: Vec::new(),
            normalized_events: Vec::new(),
            thread_id: None,
            stderr: String::new(),
            exited: Some(exited),
            exit_observed_at: Some(SystemTime::now()),
            stream_closed: false,
            cancelled: false,
            cancel_event_emitted: false,
            started_at: Instant::now(),
            launched_at: SystemTime::now(),
            rollout_root: None,
            rollout_anchor: None,
            actual_profile: None,
            diagnostic: execution_diagnostic(&config(), "codex-cli 0.160.0", &request, None),
            diagnostic_persisted: false,
        }
    }

    #[tokio::test]
    async fn child_exit_waits_for_delayed_final_reader_event() {
        let (sender, receiver) = mpsc::channel(4);
        let mut active = exited_test_turn(receiver).await;
        let delayed_reader = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            sender
                .send(StreamMessage::Event(
                    parse_codex_jsonl_event(r#"{"type":"turn.completed"}"#, 1024)
                        .expect("completion event"),
                ))
                .await
                .expect("send completion after child exit");
        });
        let turn_id = TurnId::new("reader-test-turn").expect("turn");
        drain_active_turn(&mut active, &turn_id, 1024)
            .await
            .expect("drain delayed reader");
        delayed_reader.await.expect("reader task");
        assert!(active.stream_closed);
        assert_eq!(active.events.len(), 1);
        assert_eq!(active.events[0].event_type, "turn.completed");
        assert_eq!(active_turn_state(&active), CodexCliTurnStateV1::Completed);
    }

    #[tokio::test]
    async fn unclosed_reader_keeps_exited_child_nonterminal_until_next_poll() {
        let (sender, receiver) = mpsc::channel(4);
        let mut active = exited_test_turn(receiver).await;
        let turn_id = TurnId::new("reader-test-turn").expect("turn");
        drain_active_turn(&mut active, &turn_id, 1024)
            .await
            .expect("bounded first drain");
        assert!(!active.stream_closed);
        assert_eq!(active_turn_state(&active), CodexCliTurnStateV1::Running);
        sender
            .send(StreamMessage::Event(
                parse_codex_jsonl_event(r#"{"type":"turn.completed"}"#, 1024)
                    .expect("completion event"),
            ))
            .await
            .expect("send completion after bounded wait");
        drop(sender);
        drain_active_turn(&mut active, &turn_id, 1024)
            .await
            .expect("second drain");
        assert!(active.stream_closed);
        assert_eq!(active.events[0].event_type, "turn.completed");
        assert_eq!(active_turn_state(&active), CodexCliTurnStateV1::Completed);
    }

    #[tokio::test]
    async fn cancelled_turn_retains_child_ownership_until_exit_and_reader_close() {
        let (sender, receiver) = mpsc::channel(4);
        let mut active = exited_test_turn(receiver).await;
        active.cancelled = true;
        assert_eq!(active_turn_state(&active), CodexCliTurnStateV1::Running);
        drop(sender);
        let turn_id = active.turn_id.clone();
        drain_active_turn(&mut active, &turn_id, 1024)
            .await
            .expect("drain cancelled child");
        assert_eq!(active_turn_state(&active), CodexCliTurnStateV1::Cancelled);
    }

    #[tokio::test]
    async fn pending_reader_can_be_cancelled_after_child_exit() {
        let (sender, receiver) = mpsc::channel(4);
        let active = exited_test_turn(receiver).await;
        let handle = CodexCliTurnHandleV1 {
            handle_id: "pending-reader".into(),
            worker_session_id: WorkerSessionId::new("reader-test-worker").expect("worker"),
            turn_id: TurnId::new("reader-test-turn").expect("turn"),
            resumed_thread_id: None,
        };
        let mut provider = CodexCliProviderV1 {
            config: config(),
            capabilities: CodexCliCapabilitiesV1::from_command_output(
                "codex-cli 0.160.0",
                "exec resume --json read-only workspace-write",
                "Resume a previous session [SESSION_ID] --json",
            )
            .expect("capabilities"),
            active_turns: BTreeMap::from([(handle.handle_id.clone(), active)]),
            worker_handles: BTreeMap::new(),
            next_handle_sequence: 2,
        };
        provider
            .cancel(&handle)
            .await
            .expect("cancel pending drain");
        assert!(
            !provider
                .has_unfinished_turns()
                .await
                .expect("observe drained cancelled child")
        );
        let cancelled = provider
            .active_turns
            .get(&handle.handle_id)
            .expect("active turn");
        assert_eq!(active_turn_state(cancelled), CodexCliTurnStateV1::Cancelled);
        assert!(
            sender
                .send(StreamMessage::Stderr("late".into()))
                .await
                .is_err()
        );
    }

    fn rollout_record(value: Value) -> String {
        format!("{}\n", value)
    }

    fn rollout_session(thread_id: &str) -> String {
        rollout_record(serde_json::json!({"type":"session_meta","payload":{"id":thread_id}}))
    }

    fn rollout_turn(turn_id: &str, model: &str, effort: &str) -> String {
        [
            rollout_record(serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":turn_id,"root_turn_id":turn_id}})),
            rollout_record(serde_json::json!({"type":"turn_context","payload":{"turn_id":turn_id,"root_turn_id":turn_id,"model":model,"effort":effort}})),
            rollout_record(serde_json::json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":turn_id,"last_agent_message":"private final text"}})),
        ]
        .concat()
    }

    fn rollout_turn_at(
        turn_id: &str,
        model: &str,
        effort: &str,
        started_at: u64,
        completed_at: u64,
    ) -> String {
        let event_timestamp = rollout_timestamp(SystemTime::now());
        rollout_turn_at_timestamp(
            turn_id,
            model,
            effort,
            started_at,
            completed_at,
            &event_timestamp,
        )
    }

    fn rollout_turn_at_timestamp(
        turn_id: &str,
        model: &str,
        effort: &str,
        started_at: u64,
        completed_at: u64,
        event_timestamp: &str,
    ) -> String {
        [
            rollout_record(serde_json::json!({"type":"event_msg","timestamp":event_timestamp,"payload":{"type":"task_started","turn_id":turn_id,"root_turn_id":turn_id,"started_at":started_at}})),
            rollout_record(serde_json::json!({"type":"turn_context","payload":{"turn_id":turn_id,"root_turn_id":turn_id,"model":model,"effort":effort}})),
            rollout_record(serde_json::json!({"type":"event_msg","timestamp":event_timestamp,"payload":{"type":"task_complete","turn_id":turn_id,"started_at":started_at,"completed_at":completed_at}})),
        ]
        .concat()
    }

    fn rollout_timestamp(at: SystemTime) -> String {
        let nanos = at.duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        time::OffsetDateTime::from_unix_timestamp_nanos(i128::try_from(nanos).expect("nanos"))
            .expect("test timestamp")
            .format(&time::format_description::well_known::Rfc3339)
            .expect("format timestamp")
    }

    #[test]
    fn native_rollout_timestamp_parser_accepts_observed_millisecond_schema() {
        let parsed = parse_rollout_event_timestamp(
            &serde_json::json!({"timestamp":"2026-10-14T10:27:27.197Z"}),
        )
        .expect("observed CLI timestamp");
        let since_epoch = parsed.duration_since(UNIX_EPOCH).expect("post-epoch");
        assert_eq!(since_epoch.as_secs(), 1_791_973_647);
        assert_eq!(since_epoch.subsec_millis(), 197);
        assert!(
            parse_rollout_event_timestamp(
                &serde_json::json!({"timestamp":"2026-10-14T10:27:27.197+00:00"})
            )
            .is_none()
        );
    }

    fn with_rollout_file(contents: &str, test: impl FnOnce(&Path, &File)) {
        let path = std::env::temp_dir().join(format!(
            "catdesk-codex-rollout-{}.jsonl",
            uuid::Uuid::new_v4()
        ));
        fs::write(&path, contents).expect("write rollout fixture");
        let file = File::open(&path).expect("open rollout fixture");
        test(&path, &file);
        drop(file);
        fs::remove_file(&path).expect("remove rollout fixture");
    }

    #[test]
    fn native_rollout_profile_binds_exact_session_and_completed_turn() {
        let thread_id = "01a139f3-ccac-7d00-bca6-8ec83668f752";
        let turn_id = "01a139f3-e21f-7e31-b61c-9de39d2f7e2e";
        let transcript = rollout_record(serde_json::json!({
            "type":"response_item","payload":{"type":"message","text":"private transcript"}
        }));
        let contents = format!(
            "{}{}{}",
            rollout_session(thread_id),
            transcript,
            rollout_turn(turn_id, "gpt-6.1-sol", "medium")
        );
        with_rollout_file(&contents, |_, file| {
            let evidence = read_rollout_profile(file, 0, thread_id).expect("native profile");
            assert_eq!(evidence.codex_thread_id, thread_id);
            assert_eq!(evidence.codex_turn_id, turn_id);
            assert_eq!(evidence.model_id, "gpt-6.1-sol");
            assert_eq!(evidence.reasoning_effort, "medium");
            assert!(evidence.matches(thread_id, "gpt-6.1-sol"));
            assert!(!evidence.matches(thread_id, "gpt-5.6-terra"));
            assert_eq!(evidence.evidence_sha256.len(), 64);
            assert!(
                read_rollout_profile(file, 0, "00000000-0000-0000-0000-000000000001").is_none()
            );
        });
        let without_transcript = format!(
            "{}{}",
            rollout_session(thread_id),
            rollout_turn(turn_id, "gpt-6.1-sol", "medium")
        );
        with_rollout_file(&without_transcript, |_, file| {
            let evidence = read_rollout_profile(file, 0, thread_id).expect("native profile");
            with_rollout_file(&contents, |_, original| {
                assert_eq!(
                    evidence.evidence_sha256,
                    read_rollout_profile(original, 0, thread_id)
                        .expect("original native profile")
                        .evidence_sha256,
                    "transcript content must not enter the metadata digest"
                );
            });
            with_rollout_file(
                &without_transcript.replace("private final text", "different final text"),
                |_, changed_final| {
                    assert_eq!(
                        evidence.evidence_sha256,
                        read_rollout_profile(changed_final, 0, thread_id)
                            .expect("changed final text profile")
                            .evidence_sha256,
                        "final message text must not enter the metadata digest"
                    );
                },
            );
        });
    }

    #[test]
    fn resumed_rollout_reads_only_the_post_launch_native_turn() {
        let thread_id = "01a139f3-ccac-7d00-bca6-8ec83668f752";
        let old_turn = "01a139f3-e21f-7e31-b61c-9de39d2f7e2e";
        let new_turn = "01a139f5-e21f-7e31-b61c-9de39d2f7e2e";
        let prefix = format!(
            "{}{}",
            rollout_session(thread_id),
            rollout_turn(old_turn, "gpt-5.6-terra", "high")
        );
        with_rollout_file(&prefix, |path, file| {
            let launch_offset = file.metadata().expect("metadata").len();
            assert!(read_rollout_profile(file, launch_offset, thread_id).is_none());
            let mut append = fs::OpenOptions::new()
                .append(true)
                .open(path)
                .expect("append rollout");
            append
                .write_all(rollout_turn(new_turn, "gpt-6.1-sol", "medium").as_bytes())
                .expect("append native turn");
            append.sync_all().expect("sync native turn");
            let evidence = read_rollout_profile(file, launch_offset, thread_id)
                .expect("post-launch native profile");
            assert_eq!(evidence.codex_turn_id, new_turn);
            assert_eq!(evidence.model_id, "gpt-6.1-sol");
            assert_eq!(evidence.reasoning_effort, "medium");
            assert!(
                read_rollout_profile(file, 0, thread_id).is_none(),
                "full historical replay contains two turns and is ambiguous"
            );
        });
    }

    #[test]
    fn prelaunch_append_cannot_supply_the_child_turn_profile() {
        let thread_id = "01a139f3-ccac-7d00-bca6-8ec83668f752";
        let foreign_turn = "01a139f5-e21f-7e31-b61c-9de39d2f7e2e";
        with_rollout_file(&rollout_session(thread_id), |path, held_file| {
            let prelaunch_offset = held_file.metadata().expect("metadata").len();
            let stale_start = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_secs()
                .saturating_sub(10);
            fs::OpenOptions::new()
                .append(true)
                .open(path)
                .expect("open prelaunch rollout")
                .write_all(
                    rollout_turn_at(
                        foreign_turn,
                        "gpt-6.1-sol",
                        "medium",
                        stale_start,
                        stale_start,
                    )
                    .as_bytes(),
                )
                .expect("append foreign turn after anchor");
            let launched_at = SystemTime::now();
            assert!(
                read_rollout_profile(held_file, prelaunch_offset, thread_id).is_some(),
                "the foreign append otherwise has a valid native profile"
            );
            assert!(
                read_rollout_profile_in_window(
                    held_file,
                    prelaunch_offset,
                    thread_id,
                    launched_at,
                    SystemTime::now(),
                )
                .is_none(),
                "a prelaunch native start cannot be attributed to this child"
            );
        });
        let now = SystemTime::now();
        let now_epoch = now.duration_since(UNIX_EPOCH).expect("clock").as_secs();
        with_rollout_file(
            &format!(
                "{}{}",
                rollout_session(thread_id),
                rollout_turn_at(
                    foreign_turn,
                    "gpt-6.1-sol",
                    "medium",
                    now_epoch,
                    now_epoch + 60,
                )
            ),
            |_, file| {
                assert!(
                    read_rollout_profile_in_window(file, 0, thread_id, now, SystemTime::now())
                        .is_none(),
                    "completion after child-exit observation must be rejected"
                );
            },
        );
        let synthetic_launch =
            UNIX_EPOCH + Duration::from_secs(2_000_000_000) + Duration::from_millis(500);
        let foreign_event = synthetic_launch - Duration::from_millis(100);
        let same_second_foreign = format!(
            "{}{}",
            rollout_session(thread_id),
            rollout_turn_at_timestamp(
                foreign_turn,
                "gpt-6.1-sol",
                "medium",
                2_000_000_000,
                2_000_000_000,
                &rollout_timestamp(foreign_event),
            )
        );
        with_rollout_file(&same_second_foreign, |_, file| {
            assert!(
                read_rollout_profile_in_window(
                    file,
                    0,
                    thread_id,
                    synthetic_launch,
                    synthetic_launch + Duration::from_millis(100),
                )
                .is_none(),
                "millisecond event time must reject a prelaunch turn from the same Unix second"
            );
        });
    }

    #[test]
    fn native_rollout_profile_rejects_partial_or_changed_turn_metadata() {
        let thread_id = "01a139f3-ccac-7d00-bca6-8ec83668f752";
        let turn_id = "01a139f3-e21f-7e31-b61c-9de39d2f7e2e";
        let start = rollout_record(
            serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":turn_id,"root_turn_id":turn_id}}),
        );
        let context = rollout_record(
            serde_json::json!({"type":"turn_context","payload":{"turn_id":turn_id,"model":"gpt-6.1-sol","effort":"medium"}}),
        );
        let downgraded = rollout_record(
            serde_json::json!({"type":"turn_context","payload":{"turn_id":turn_id,"model":"gpt-5.6-terra","effort":"high"}}),
        );
        let complete = rollout_record(
            serde_json::json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":turn_id}}),
        );
        let cases = [
            format!("{}{}{}", rollout_session(thread_id), start, context),
            format!(
                "{}{}{}{}{}",
                rollout_session(thread_id),
                start,
                context,
                downgraded,
                complete
            ),
            format!(
                "{}{}{}{}{}",
                rollout_session(thread_id),
                start,
                context,
                complete,
                complete
            ),
            format!(
                "{}{}{}{}{}",
                rollout_session(thread_id),
                start,
                context,
                complete,
                context
            ),
            format!(
                "{}{}{}{}{}",
                rollout_session(thread_id),
                start,
                context,
                complete,
                start
            ),
        ];
        for contents in cases {
            with_rollout_file(&contents, |_, file| {
                assert!(read_rollout_profile(file, 0, thread_id).is_none());
            });
        }
    }

    #[test]
    fn native_rollout_profile_requires_consistent_root_turn_identity() {
        let thread_id = "01a139f3-ccac-7d00-bca6-8ec83668f752";
        let turn_id = "01a139f3-e21f-7e31-b61c-9de39d2f7e2e";
        let other = "01a139f5-e21f-7e31-b61c-9de39d2f7e2e";
        let start = rollout_record(
            serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":turn_id,"root_turn_id":turn_id}}),
        );
        let context = |root: Option<&str>| {
            let mut value = serde_json::json!({"type":"turn_context","payload":{"turn_id":turn_id,"model":"gpt-6.1-sol","effort":"medium"}});
            if let Some(root) = root {
                value["payload"]["root_turn_id"] = serde_json::json!(root);
            }
            rollout_record(value)
        };
        let complete = |root: Option<&str>| {
            let mut value = serde_json::json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":turn_id}});
            if let Some(root) = root {
                value["payload"]["root_turn_id"] = serde_json::json!(root);
            }
            rollout_record(value)
        };
        for (context_root, completion_root) in [
            (None, None),
            (Some(other), None),
            (Some(turn_id), Some(other)),
        ] {
            let contents = format!(
                "{}{}{}{}",
                rollout_session(thread_id),
                start,
                context(context_root),
                complete(completion_root)
            );
            with_rollout_file(&contents, |_, file| {
                assert!(read_rollout_profile(file, 0, thread_id).is_none());
            });
        }
        let absent_completion_root = format!(
            "{}{}{}{}",
            rollout_session(thread_id),
            start,
            context(Some(turn_id)),
            complete(None)
        );
        let matching_completion_root = format!(
            "{}{}{}{}",
            rollout_session(thread_id),
            start,
            context(Some(turn_id)),
            complete(Some(turn_id))
        );
        with_rollout_file(&absent_completion_root, |_, file| {
            let without = read_rollout_profile(file, 0, thread_id).expect("real CLI shape");
            with_rollout_file(&matching_completion_root, |_, file| {
                let with = read_rollout_profile(file, 0, thread_id)
                    .expect("optional matching completion root");
                assert_ne!(without.evidence_sha256, with.evidence_sha256);
            });
        });
    }

    #[test]
    fn rollout_discovery_requires_exact_thread_and_post_launch_boundary() {
        let thread_id = "01a139f3-ccac-7d00-bca6-8ec83668f752";
        let turn_id = "01a139f3-e21f-7e31-b61c-9de39d2f7e2e";
        let base = std::env::temp_dir().join(format!(
            "catdesk-codex-rollout-discovery-{}",
            uuid::Uuid::new_v4()
        ));
        let root = base.join("sessions");
        let day_one = root.join("2026").join("10").join("14");
        let day_two = root.join("2026").join("10").join("15");
        fs::create_dir_all(&day_one).expect("create first date");
        fs::create_dir_all(&day_two).expect("create second date");
        let name = format!("rollout-2026-10-14T06-27-26-{thread_id}.jsonl");
        let old = day_one.join(&name);
        fs::write(
            &old,
            format!(
                "{}{}",
                rollout_session(thread_id),
                rollout_turn_at(
                    turn_id,
                    "gpt-5.6-terra",
                    "high",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("clock")
                        .as_secs(),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("clock")
                        .as_secs(),
                )
            ),
        )
        .expect("write pre-launch rollout");
        let launch = SystemTime::now();
        assert!(read_postlaunch_rollout_profile(&root, thread_id, launch, launch, None).is_none());
        let anchor = open_prelaunch_rollout(&root, thread_id)
            .expect("unambiguous pre-launch rollout")
            .expect("anchor");
        assert_eq!(anchor.length, fs::metadata(&old).expect("metadata").len());
        let new_turn = "01a139f5-e21f-7e31-b61c-9de39d2f7e2e";
        fs::OpenOptions::new()
            .append(true)
            .open(&old)
            .expect("open old rollout for append")
            .write_all(
                rollout_turn_at(
                    new_turn,
                    "gpt-6.1-sol",
                    "medium",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("clock")
                        .as_secs(),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("clock")
                        .as_secs(),
                )
                .as_bytes(),
            )
            .expect("append new native turn");
        let appended = read_postlaunch_rollout_profile(
            &root,
            thread_id,
            launch,
            SystemTime::now(),
            Some(&anchor),
        )
        .expect("held pre-launch handle and offset");
        assert_eq!(appended.codex_turn_id, new_turn);
        let new = day_two.join(name);
        fs::write(
            &new,
            format!(
                "{}{}",
                rollout_session(thread_id),
                rollout_record(serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":new_turn,"root_turn_id":new_turn}}))
            ),
        )
        .expect("write partial second matching rollout");
        assert!(
            read_postlaunch_rollout_profile(
                &root,
                thread_id,
                launch,
                SystemTime::now(),
                Some(&anchor),
            )
            .is_none(),
            "a partial exact-thread candidate must block the otherwise valid append"
        );
        fs::write(
            &new,
            format!(
                "{}{}",
                rollout_session(thread_id),
                rollout_turn_at(
                    new_turn,
                    "gpt-6.1-sol",
                    "medium",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("clock")
                        .as_secs(),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("clock")
                        .as_secs(),
                )
            ),
        )
        .expect("write second matching rollout");
        assert!(
            read_postlaunch_rollout_profile(
                &root,
                thread_id,
                launch,
                SystemTime::now(),
                Some(&anchor),
            )
            .is_none(),
            "two post-launch candidates are ambiguous"
        );
        assert!(
            open_prelaunch_rollout(&root, thread_id).is_err(),
            "a resumed thread cannot anchor two matching files"
        );
        drop(anchor);
        fs::remove_file(&old).expect("remove old rollout");
        fs::remove_file(&new).expect("remove new rollout");
        fs::remove_dir(&day_one).expect("remove first date");
        fs::remove_dir(&day_two).expect("remove second date");
        fs::remove_dir(root.join("2026").join("10")).expect("remove month");
        fs::remove_dir(root.join("2026")).expect("remove year");
        fs::remove_dir(&root).expect("remove sessions root");
        fs::remove_dir(&base).expect("remove fixture root");
    }

    fn completed_native_stdout(thread_id: &str) -> CodexCliNativeStdoutEvidenceV1 {
        let mut stdout = CodexCliNativeStdoutEvidenceV1::new(thread_id).expect("thread id");
        stdout.observe_line(
            serde_json::json!({"type":"thread.started","thread_id":thread_id})
                .to_string()
                .as_bytes(),
        );
        stdout.observe_line(br#"{"type":"turn.started"}"#);
        stdout.observe_line(br#"{"type":"item.completed","item":{"text":"private transcript"}}"#);
        stdout.observe_line(br#"{"type":"turn.completed"}"#);
        stdout.mark_eof();
        stdout
    }

    #[test]
    fn native_stdout_markers_require_exact_thread_single_turn_and_eof() {
        let thread_id = "01a139f3-ccac-7d00-bca6-8ec83668f752";
        let mut no_eof = CodexCliNativeStdoutEvidenceV1::new(thread_id).expect("thread");
        no_eof.observe_line(
            serde_json::json!({"type":"thread.started","thread_id":thread_id})
                .to_string()
                .as_bytes(),
        );
        no_eof.observe_line(br#"{"type":"turn.started"}"#);
        no_eof.observe_line(br#"{"type":"turn.completed"}"#);
        assert!(!no_eof.completed_for(thread_id));
        no_eof.mark_eof();
        assert!(no_eof.completed_for(thread_id));
        assert!(!no_eof.completed_for("01a139f5-e21f-7e31-b61c-9de39d2f7e2e"));

        let mut duplicate = completed_native_stdout(thread_id);
        duplicate.observe_line(br#"{"type":"turn.completed"}"#);
        assert!(!duplicate.completed_for(thread_id));
        let mut wrong_thread = CodexCliNativeStdoutEvidenceV1::new(thread_id).expect("thread");
        wrong_thread.observe_line(
            br#"{"type":"thread.started","thread_id":"01a139f5-e21f-7e31-b61c-9de39d2f7e2e"}"#,
        );
        wrong_thread.mark_eof();
        assert!(!wrong_thread.completed_for(thread_id));
        let mut oversized = CodexCliNativeStdoutEvidenceV1::new(thread_id).expect("thread");
        oversized.observe_line(&vec![b'x'; MAX_NATIVE_STDOUT_LINE_BYTES + 1]);
        oversized.mark_eof();
        assert!(!oversized.completed_for(thread_id));
        let mut failed = completed_native_stdout(thread_id);
        failed.invalidate();
        assert!(!failed.completed_for(thread_id));
        let mut total_oversized = CodexCliNativeStdoutEvidenceV1::new(thread_id).expect("thread");
        total_oversized.total_bytes = MAX_NATIVE_STDOUT_BYTES;
        total_oversized.observe_line(br#"{"type":"thread.started"}"#);
        total_oversized.mark_eof();
        assert!(!total_oversized.completed_for(thread_id));
    }

    #[test]
    fn resumed_native_readback_requires_held_growth_and_stdout_completion() {
        let thread_id = "01a139f3-ccac-7d00-bca6-8ec83668f752";
        let old_turn = "01a139f3-e21f-7e31-b61c-9de39d2f7e2e";
        let new_turn = "01a139f5-e21f-7e31-b61c-9de39d2f7e2e";
        let base = std::env::temp_dir().join(format!(
            "catdesk-native-resume-readback-{}",
            uuid::Uuid::new_v4()
        ));
        let sessions = base.join("sessions");
        let first_day = sessions.join("2026").join("10").join("15");
        let second_day = sessions.join("2026").join("10").join("16");
        fs::create_dir_all(&first_day).expect("create rollout directory");
        let name = format!("rollout-2026-10-15T10-27-26-{thread_id}.jsonl");
        let old_path = first_day.join(&name);
        fs::write(
            &old_path,
            format!(
                "{}{}",
                rollout_session(thread_id),
                rollout_turn(old_turn, "gpt-5.6-terra", "high")
            ),
        )
        .expect("write prelaunch rollout");
        let reader = prepare_resumed_native_turn_readback(Some(&base), thread_id)
            .expect("held exact rollout");
        let launch = SystemTime::now();
        let stdout = completed_native_stdout(thread_id);
        assert!(
            reader
                .read_completed(launch, SystemTime::now(), &stdout)
                .is_none(),
            "preexisting completed turn must not count as this child"
        );
        let now_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_secs();
        fs::OpenOptions::new()
            .append(true)
            .open(&old_path)
            .expect("open rollout for append")
            .write_all(
                rollout_turn_at(new_turn, "gpt-6.1-sol", "medium", now_epoch, now_epoch).as_bytes(),
            )
            .expect("append new native turn");
        let exit = SystemTime::now();
        let profile = reader
            .read_completed(launch, exit, &stdout)
            .expect("postlaunch native metadata and stdout");
        assert_eq!(profile.codex_turn_id, new_turn);
        assert!(profile.matches(thread_id, "gpt-6.1-sol"));
        assert!(!profile.matches(thread_id, "gpt-5.6-terra"));
        let mut incomplete_stdout = CodexCliNativeStdoutEvidenceV1::new(thread_id).expect("thread");
        incomplete_stdout.observe_line(
            serde_json::json!({"type":"thread.started","thread_id":thread_id})
                .to_string()
                .as_bytes(),
        );
        incomplete_stdout.mark_eof();
        assert!(
            reader
                .read_completed(launch, exit, &incomplete_stdout)
                .is_none()
        );

        fs::create_dir_all(&second_day).expect("create second date");
        let second_path = second_day.join(&name);
        fs::write(&second_path, rollout_session(thread_id)).expect("ambiguous same-thread file");
        assert!(
            reader.read_completed(launch, exit, &stdout).is_none(),
            "second matching rollout must refuse profile evidence"
        );
        drop(reader);
        fs::remove_file(&old_path).expect("remove first rollout");
        fs::remove_file(&second_path).expect("remove second rollout");
        fs::remove_dir(&first_day).expect("remove first day");
        fs::remove_dir(&second_day).expect("remove second day");
        fs::remove_dir(sessions.join("2026").join("10")).expect("remove month");
        fs::remove_dir(sessions.join("2026")).expect("remove year");
        fs::remove_dir(&sessions).expect("remove sessions root");
        fs::remove_dir(&base).expect("remove fixture root");
    }

    #[test]
    fn autonomous_prompt_is_bounded_and_never_serializes_tool_definitions() {
        let worker_session_id = WorkerSessionId::new("codex-worker").expect("worker");
        let request = WorkerProviderTurnRequestV1 {
            provider_session: ProviderSessionV1 {
                provider_id: "codex-cli".into(),
                provider_session_id: "codex-unbound-codex-worker".into(),
                model_id: "default".into(),
                keep_alive: None,
            },
            turn: ProviderTurnRequestV1 {
                run_id: RunId::new("run-1").expect("run"),
                worker_session_id: worker_session_id.clone(),
                turn_id: TurnId::new("turn-1").expect("turn"),
                model_id: "default".into(),
                context_json: serde_json::json!({"contractHash":"abc"}),
                tool_definitions: vec![ToolDefinitionV1 {
                    name: "should-not-reach-codex".into(),
                    description: "test only".into(),
                    input_schema: serde_json::json!({"type":"object"}),
                    mutation_kind: ToolMutationKind::ReadOnly,
                }],
                max_output_bytes: 1024,
            },
            history: vec![ProviderMessageV1 {
                role: "user".into(),
                content: "bounded autonomous task".into(),
                tool_call_id: None,
                tool_name: None,
            }],
            json_envelope_recovery: false,
        };
        let prompt = autonomous_prompt(&request).expect("prompt");
        assert!(prompt.len() <= MAX_PROMPT_BYTES);
        assert!(!prompt.contains("should-not-reach-codex"));
        assert!(prompt.contains("bounded autonomous task"));
    }

    #[test]
    fn parses_capabilities_without_assuming_undocumented_flags() {
        let capabilities = CodexCliCapabilitiesV1::from_command_output(
            "codex-cli 0.146.1",
            "Usage: codex exec\n --json\n --sandbox <SANDBOX_MODE>\n read-only workspace-write\n --output-schema\n --output-last-message\n resume",
            "Resume a previous session\n[SESSION_ID]\n--json\n--last",
        )
        .expect("capabilities");
        capabilities
            .required_for_autonomous_loop()
            .expect("required");
        assert!(capabilities.supports_resume);
        assert!(capabilities.supports_jsonl);
    }

    #[test]
    fn capability_discovery_fails_closed_when_resume_is_missing() {
        let capabilities = CodexCliCapabilitiesV1::from_command_output(
            "codex-cli 0.146.1",
            "Usage: codex exec --json --sandbox read-only workspace-write",
            "Usage: something else",
        )
        .expect("parse partial capability output");
        assert!(capabilities.required_for_autonomous_loop().is_err());
    }

    #[test]
    fn structured_events_capture_thread_and_bound_sensitive_text() {
        let event = parse_codex_jsonl_event(
            r#"{"type":"thread.started","thread_id":"thread-123"}"#,
            1024,
        )
        .expect("thread event");
        assert_eq!(event.thread_id.as_deref(), Some("thread-123"));
        let secret_value = ["synthetic", "test", "value"].join("-");
        let line = serde_json::json!({
            "type": "item.completed",
            "item": {
                "type": "agent_message",
                "text": format!("{}={secret_value}", "token")
            }
        })
        .to_string();
        let message = parse_codex_jsonl_event(&line, 1024).expect("message event");
        assert!(!message.text.expect("text").contains(&secret_value));
        assert!(!message.bounded_summary.contains(&secret_value));
    }

    #[test]
    fn fake_cli_jsonl_fixture_preserves_thread_and_turn_boundaries() {
        let fixture = [
            r#"{"type":"thread.started","thread_id":"thread-fixture-1"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"bounded result"}}"#,
            r#"{"type":"turn.completed"}"#,
        ];
        let events = fixture
            .iter()
            .map(|line| parse_codex_jsonl_event(line, 1024))
            .collect::<Result<Vec<_>, _>>()
            .expect("fixture parses");
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].thread_id.as_deref(), Some("thread-fixture-1"));
        assert_eq!(events[3].event_type, "turn.completed");
        assert!(
            events
                .iter()
                .all(|event| event.bounded_summary.len() <= 1024)
        );
    }

    #[test]
    fn oversized_event_placeholder_is_non_terminal_and_contains_no_body() {
        let event = CodexCliEventV1 {
            event_type: "oversized.event".into(),
            thread_id: Some("thread-1".into()),
            item_type: None,
            text: None,
            retry_after_seconds: None,
            bounded_summary:
                "Codex JSONL event body omitted because it exceeded the configured byte limit (70000 bytes)"
                    .into(),
        };
        let turn_id = TurnId::new("turn-oversized").expect("turn");
        let normalized = normalize_codex_event(&event, &turn_id);
        assert_eq!(normalized.kind, NormalizedProviderEventKind::TextDelta);
        assert!(event.text.is_none());
        assert!(event.bounded_summary.contains("body omitted"));
    }

    #[test]
    fn malformed_or_oversized_jsonl_is_rejected() {
        assert!(parse_codex_jsonl_event("not json", 64).is_err());
        assert!(parse_codex_jsonl_event(r#"{"type":"thread.started"}"#, 4).is_err());
        assert!(parse_codex_jsonl_event(r#"{"thread_id":"missing-type"}"#, 1024).is_err());
    }

    #[test]
    fn rate_limit_classification_uses_structured_retry_or_bounded_text() {
        let structured = CodexCliEventV1 {
            event_type: "error".into(),
            thread_id: None,
            item_type: None,
            text: None,
            retry_after_seconds: Some(42),
            bounded_summary: "provider error".into(),
        };
        assert_eq!(classify_rate_limit(&structured), Some(42));
        let text = CodexCliEventV1 {
            retry_after_seconds: None,
            bounded_summary: "Too many requests from provider".into(),
            ..structured
        };
        assert_eq!(classify_rate_limit(&text), Some(0));
    }

    #[test]
    fn credit_exhaustion_classifier_is_conservative_and_precedes_429() {
        assert_eq!(
            classify_codex_availability_failure("HTTP 429: usage limit reached; try later"),
            Some(CodexAvailabilityFailureV1::CreditsExhausted)
        );
        assert_eq!(
            classify_codex_availability_failure("HTTP 429 too many requests"),
            Some(CodexAvailabilityFailureV1::TransientRateLimited)
        );
        assert_eq!(classify_codex_availability_failure("limit reached"), None);
        assert_eq!(
            classify_codex_availability_failure(
                "You've hit your usage limit. Try again at 10:54 AM."
            ),
            Some(CodexAvailabilityFailureV1::CreditsExhausted)
        );
        assert_eq!(
            classify_codex_availability_failure("your usage limit setting was read successfully"),
            None,
            "a settings/status sentence is not an exhaustion diagnostic"
        );
        assert_eq!(
            classify_codex_availability_failure("secret=not-for-handoff credits exhausted"),
            Some(CodexAvailabilityFailureV1::CreditsExhausted)
        );
    }

    fn reset_clock(hour: u16, minute: u16, second: u16) -> CodexTrustedLocalClockV1 {
        CodexTrustedLocalClockV1 {
            observed_at_unix: 1_000_000,
            year: 2026,
            month: 8,
            day: 28,
            hour,
            minute,
            second,
        }
    }

    #[test]
    fn credit_exhaustion_reset_parser_handles_12_hour_edges_and_next_day_rollover() {
        let dated_clock = CodexTrustedLocalClockV1 {
            observed_at_unix: 2_000_000,
            year: 2026,
            month: 9,
            day: 1,
            hour: 23,
            minute: 59,
            second: 45,
        };
        assert_eq!(
            parse_codex_credits_reset_after(
                "You've hit your usage limit. Try again at Sep 2nd, 2026 12:50 AM.",
                dated_clock,
            ),
            Some(2_003_015),
            "the live provider date is resolved against the trusted local date without rollover invention"
        );
        assert_eq!(
            parse_codex_credits_reset_after(
                "You've hit your usage limit. Try again at 12:00 AM.",
                reset_clock(23, 59, 45),
            ),
            Some(1_000_015)
        );
        assert_eq!(
            parse_codex_credits_reset_after(
                "credits exhausted; try again at 12:00 PM.",
                reset_clock(11, 59, 59),
            ),
            Some(1_000_001)
        );
        assert_eq!(
            parse_codex_credits_reset_after(
                "usage limit reached; try again at 10:54 AM.",
                reset_clock(10, 53, 45),
            ),
            Some(1_000_015)
        );
        assert_eq!(
            parse_codex_credits_reset_after(
                "usage limit reached; try again at 10:54 AM.",
                reset_clock(10, 54, 0),
            ),
            Some(1_086_400),
            "a passed or exactly-current clock rolls to the deterministic next local day"
        );
    }

    #[test]
    fn credit_exhaustion_reset_parser_rejects_unattested_malformed_or_ambiguous_phrases() {
        let clock = reset_clock(10, 0, 0);
        for value in [
            "HTTP 429. Try again at 10:54 AM.",
            "usage limit reached",
            "usage limit reached; try again at 0:54 AM.",
            "usage limit reached; try again at 10:60 AM.",
            "usage limit reached; try again at 10:54 AM; try again at 11:00 AM.",
            "usage limit reached; try again at 10:54 AMish.",
            "usage limit reached; try again at Sep 31st, 2026 10:54 AM.",
            "usage limit reached; try again at Sep 2th, 2026 10:54 AM.",
            "usage limit reached; try again at Sep 2nd, 2026 10:54 AM; try again at 11:00 AM.",
            "usage limit reached; try again at Aug 27th, 2026 10:54 AM.",
        ] {
            assert_eq!(
                parse_codex_credits_reset_after(value, clock),
                None,
                "{value}"
            );
        }
    }

    #[test]
    fn shell_shims_and_invalid_directories_are_rejected() {
        let directory = std::env::current_dir().expect("working directory");
        let shim = CodexCliConfigV1 {
            executable: PathBuf::from("C:\\temp\\codex.cmd"),
            working_directory: directory.clone(),
            operator_codex_home: None,
            model_id: None,
            host_authority_roots: Vec::new(),
            sandbox: CodexCliSandboxV1::ReadOnly,
            approval_policy: CodexCliApprovalPolicyV1::Never,
            max_event_bytes: 1,
            max_diagnostic_bytes: 1,
            diagnostic_root: None,
        };
        assert!(shim.validate().is_err());
        let mut invalid = config();
        invalid.working_directory = PathBuf::from("C:\\missing-catdesk-workspace");
        assert!(invalid.validate().is_err());
    }

    #[tokio::test]
    async fn sol_workspace_write_refuses_host_authority_overlap_before_spawning() {
        let fixture = std::env::temp_dir().join(format!(
            "catdesk-sol-authority-boundary-{}",
            uuid::Uuid::new_v4()
        ));
        let workspace = fixture.join("workspace");
        let authority = workspace.join(".catdesk").join("autonomy");
        let sibling = fixture.join("workspace-other");
        fs::create_dir_all(&authority).expect("host state fixture");
        fs::create_dir_all(&sibling).expect("sibling fixture");
        let mut config =
            CodexCliConfigV1::new(direct_executable(), workspace).expect("worker config");
        config.model_id = Some(SOL_MEDIUM_MODEL.into());
        assert!(
            config.validate_sol_worker_authority_boundary().is_err(),
            "unknown host authority is not an acceptable Sol worker boundary"
        );
        config.host_authority_roots = vec![authority];
        let mut provider = CodexCliProviderV1 {
            config: config.clone(),
            capabilities: CodexCliCapabilitiesV1::from_command_output(
                "codex-cli 0.160.0",
                "exec resume --json read-only workspace-write",
                "Resume a previous session [SESSION_ID] --json",
            )
            .expect("capabilities"),
            active_turns: BTreeMap::new(),
            worker_handles: BTreeMap::new(),
            next_handle_sequence: 1,
        };
        let request = CodexCliTurnRequestV1 {
            worker_session_id: WorkerSessionId::new("sol-authority-worker").expect("worker"),
            turn_id: TurnId::new("sol-authority-turn").expect("turn"),
            prompt: "bounded request".into(),
        };
        let error = provider
            .start_turn(request)
            .await
            .expect_err("overlap must hold");
        assert!(format!("{error:?}").contains("overlaps host authority"));
        assert!(provider.active_turns.is_empty());
        config.host_authority_roots = vec![sibling];
        validate_sol_worker_write_roots(&config.working_directory, &config.host_authority_roots)
            .expect("sibling paths are disjoint");
        let error = config
            .validate_sol_worker_authority_boundary()
            .expect_err("disjoint paths cannot replace a worker OS identity");
        assert!(format!("{error:?}").contains("OS isolation is unproven"));
        provider.config = config;
        let error = provider
            .start_turn(CodexCliTurnRequestV1 {
                worker_session_id: WorkerSessionId::new("sol-disjoint-worker").expect("worker"),
                turn_id: TurnId::new("sol-disjoint-turn").expect("turn"),
                prompt: "bounded request".into(),
            })
            .await
            .expect_err("direct CLI launch must remain closed for disjoint paths");
        assert!(format!("{error:?}").contains("OS isolation is unproven"));
        assert!(provider.active_turns.is_empty());
        fs::remove_dir_all(&fixture).expect("retire owned test fixture");
    }

    #[test]
    fn command_construction_removes_credential_environment_names() {
        let config = config();
        assert_eq!(config.sandbox.as_flag_value(), "workspace-write");
        let request = CodexCliTurnRequestV1 {
            worker_session_id: WorkerSessionId::new("codex-worker").expect("worker"),
            turn_id: TurnId::new("codex-turn").expect("turn"),
            prompt: "bounded prompt".into(),
        };
        request.validate().expect("prompt");
        assert!(
            CodexCliTurnRequestV1 {
                prompt: " ".into(),
                ..request
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn diagnostic_records_the_effective_safe_invocation_without_user_config_bypass() {
        let request = CodexCliTurnRequestV1 {
            worker_session_id: WorkerSessionId::new("codex-worker").expect("worker"),
            turn_id: TurnId::new("codex-turn").expect("turn"),
            prompt: "bounded prompt".into(),
        };
        let diagnostic = execution_diagnostic(&config(), "codex-cli 0.146.1", &request, None);
        assert_eq!(diagnostic.sandbox, CodexCliSandboxV1::WorkspaceWrite);
        assert_eq!(diagnostic.approval_policy, CodexCliApprovalPolicyV1::Never);
        assert!(
            diagnostic
                .argv
                .windows(2)
                .any(|pair| { pair == ["--sandbox".to_string(), "workspace-write".to_string()] })
        );
        assert!(diagnostic.argv.contains(&"--ignore-rules".to_string()));
        assert!(
            !diagnostic
                .argv
                .contains(&"--ignore-user-config".to_string())
        );
        assert!(!diagnostic.argv.join(" ").contains("bounded prompt"));
    }

    #[test]
    fn cli_resume_diagnostic_uses_only_the_captured_canonical_thread_slot() {
        let request = CodexCliTurnRequestV1 {
            worker_session_id: WorkerSessionId::new("codex-worker").expect("worker"),
            turn_id: TurnId::new("codex-turn").expect("turn"),
            prompt: "bounded prompt".into(),
        };
        let diagnostic = execution_diagnostic(
            &config(),
            "codex-cli 0.146.1",
            &request,
            Some("existing-gui-visible-thread"),
        );
        assert!(
            diagnostic
                .argv
                .windows(2)
                .any(|pair| { pair == ["resume".to_string(), "<captured-thread-id>".to_string()] })
        );
        assert!(
            !diagnostic.argv.contains(&"--color".to_string()),
            "resume must not create a new color-configured exec session"
        );
        assert!(
            !diagnostic
                .argv
                .join(" ")
                .contains("existing-gui-visible-thread"),
            "diagnostics retain only the captured-thread slot, not a raw GUI history id"
        );
    }

    #[test]
    fn terra_model_diagnostic_records_required_high_reasoning_override() {
        let request = CodexCliTurnRequestV1 {
            worker_session_id: WorkerSessionId::new("codex-worker").expect("worker"),
            turn_id: TurnId::new("codex-turn").expect("turn"),
            prompt: "bounded prompt".into(),
        };
        let mut terra = config();
        terra.model_id = Some(CATDESK_REQUIRED_CODEX_MODEL.into());
        let diagnostic = execution_diagnostic(&terra, "codex-cli 0.146.1", &request, None);
        assert_eq!(
            diagnostic.model_id.as_deref(),
            Some(CATDESK_REQUIRED_CODEX_MODEL)
        );
        assert!(diagnostic.argv.windows(2).any(|pair| {
            pair == [
                "-c".to_string(),
                "model_reasoning_effort=<configured-reasoning-effort>".to_string(),
            ]
        }));
    }

    #[test]
    fn sol_medium_diagnostic_records_explicit_reasoning_override() {
        assert_eq!(
            managed_reasoning_override("gpt-6.1-sol").as_deref(),
            Some("model_reasoning_effort=medium")
        );
        assert_eq!(
            managed_reasoning_override("gpt-5.6-terra").as_deref(),
            Some("model_reasoning_effort=high")
        );
        assert!(managed_reasoning_override("unsupported").is_none());
        let request = CodexCliTurnRequestV1 {
            worker_session_id: WorkerSessionId::new("codex-worker").expect("worker"),
            turn_id: TurnId::new("codex-turn").expect("turn"),
            prompt: "bounded prompt".into(),
        };
        let mut sol = config();
        sol.model_id = Some(super::super::codex_model_policy::SOL_MEDIUM_MODEL.into());
        let diagnostic = execution_diagnostic(&sol, "codex-cli 0.146.1", &request, None);
        assert_eq!(diagnostic.model_id.as_deref(), Some("gpt-6.1-sol"));
        assert!(diagnostic.argv.windows(2).any(|pair| {
            pair == [
                "-c".to_string(),
                "model_reasoning_effort=<configured-reasoning-effort>".to_string(),
            ]
        }));
    }

    #[test]
    fn structured_diagnostic_summary_preserves_command_kind_and_redacts_values() {
        let sensitive_command = format!("echo {}={}", "token", "diagnostic-secret");
        let event = parse_codex_jsonl_event(
            &serde_json::json!({
                "type": "item.completed",
                "item": {
                    "type": "command_execution",
                    "status": "completed",
                    "command": sensitive_command,
                }
            })
            .to_string(),
            4096,
        )
        .expect("command event");
        assert_eq!(event.item_type.as_deref(), Some("command_execution"));
        assert!(event.bounded_summary.contains("itemStatus=completed"));
        assert!(!event.bounded_summary.contains("diagnostic-secret"));
    }
}
