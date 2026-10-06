//! Supported, read-first Codex app-server protocol boundary.
//!
//! This module deliberately models only documented account and thread
//! surfaces.  It never reads Codex auth files, sends API keys, or exposes a
//! billing/reset-credit operation.  A desktop integration supplies the JSON
//! transport; keeping that transport outside the autonomous controller makes
//! the protocol testable and prevents an unavailable app-server from being
//! mistaken for an exhausted account.

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use super::runtime::RuntimeError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const CODEX_APP_SERVER_READ_METHODS_V1: [&str; 3] =
    ["account/rateLimits/read", "thread/list", "thread/read"];
pub const CODEX_APP_SERVER_CONTINUITY_METHODS_V1: [&str; 2] = ["turn/start", "thread/turns/list"];
pub const CODEX_APP_SERVER_GOAL_METHODS_V1: [&str; 2] = ["thread/goal/get", "thread/goal/set"];
pub const CATDESK_REQUIRED_CODEX_MODEL_V1: &str = "gpt-5.6-terra";
pub const CATDESK_REQUIRED_CODEX_REASONING_EFFORT_V1: &str = "high";
const MAX_APP_SERVER_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_APP_SERVER_SKIPPED_NOTIFICATIONS: usize = 32;
const THREAD_LIST_PAGE_LIMIT: u64 = 5;
const THREAD_LIST_MAX_PAGES: usize = 8;

/// Launch settings for a CatDesk-owned supported `codex app-server --stdio`
/// process.  The only authentication-related setting is an opaque,
/// operator-selected config-root path.  CatDesk never reads it, and it is
/// never sent through MCP or JSON-RPC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexAppServerLaunchConfigV1 {
    pub executable: PathBuf,
    pub working_directory: PathBuf,
    pub operator_codex_home: Option<PathBuf>,
}

impl CodexAppServerLaunchConfigV1 {
    pub fn new(
        executable: PathBuf,
        working_directory: PathBuf,
        operator_codex_home: Option<PathBuf>,
    ) -> Result<Self, RuntimeError> {
        let config = Self {
            executable,
            working_directory,
            operator_codex_home,
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
        if matches!(extension.as_str(), "cmd" | "bat" | "ps1") || !self.executable.is_file() {
            return Err(RuntimeError::Validation(
                "Codex app-server executable must be an existing direct executable".into(),
            ));
        }
        if !self.working_directory.is_dir() {
            return Err(RuntimeError::Validation(
                "Codex app-server working directory must be an existing directory".into(),
            ));
        }
        if self
            .operator_codex_home
            .as_ref()
            .is_some_and(|path| !path.is_dir())
        {
            return Err(RuntimeError::Validation(
                "operator-local Codex config root must be an existing directory".into(),
            ));
        }
        Ok(())
    }

    /// Prepares the documented stdio app-server command.  This deliberately
    /// does not launch a shell and removes API-key variables so the only
    /// available account context is the operator's existing Codex context.
    pub fn stdio_command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command
            .current_dir(&self.working_directory)
            .args(["app-server", "--stdio"])
            // A normal host child must use Codex's ordinary current-user
            // default context. Do not accidentally inherit CatDesk recovery
            // controls or a process-scoped CODEX_HOME; an explicitly
            // validated `operator_codex_home` below is the sole exception.
            .env_remove("CATDESK_CODEX_HOME")
            .env_remove("CATDESK_CODEX_CLI_EXECUTABLE")
            .env_remove("CODEX_HOME")
            .env_remove("CODEX_API_KEY")
            .env_remove("OPENAI_API_KEY")
            .env_remove("CONTROL_PLANE_API_KEY")
            .env("NO_COLOR", "1");
        if let Some(codex_home) = &self.operator_codex_home {
            command.env("CODEX_HOME", codex_home);
        }
        command
    }

    /// Starts a host-owned process only. This type is intentionally never
    /// constructed by the workspace-write worker adapter.
    pub fn spawn_stdio_transport(&self) -> Result<CodexAppServerStdioTransportV1, RuntimeError> {
        self.validate()?;
        let mut command = self.stdio_command();
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|error| {
            RuntimeError::Provider(format!(
                "failed to launch host-owned Codex app-server: {error}"
            ))
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            RuntimeError::Provider("host-owned Codex app-server stdin pipe unavailable".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            RuntimeError::Provider("host-owned Codex app-server stdout pipe unavailable".into())
        })?;
        Ok(CodexAppServerStdioTransportV1 {
            child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
            next_request_id: 1,
            initialized: false,
        })
    }
}

/// A bounded JSON-RPC-over-stdio transport owned by the CatDesk host.
/// It does not expose arbitrary RPC access: only initialize, read methods,
/// and a resume of a previously exact/non-owned thread are admitted.
pub struct CodexAppServerStdioTransportV1 {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_request_id: u64,
    initialized: bool,
}

impl CodexAppServerStdioTransportV1 {
    pub fn initialize(&mut self) -> Result<Value, RuntimeError> {
        if self.initialized {
            return Ok(json!({"alreadyInitialized": true}));
        }
        let response = self.request(
            "initialize",
            json!({
                "clientInfo": {"name": "catdesk-host", "version": "1"},
                "capabilities": {"experimentalApi": true}
            }),
        )?;
        self.initialized = true;
        Ok(response)
    }

    pub fn shutdown(&mut self) -> Result<(), RuntimeError> {
        if self
            .child
            .try_wait()
            .map_err(|error| {
                RuntimeError::Provider(format!(
                    "failed to inspect host-owned Codex app-server: {error}"
                ))
            })?
            .is_none()
        {
            self.child.kill().map_err(|error| {
                RuntimeError::Provider(format!(
                    "failed to stop host-owned Codex app-server: {error}"
                ))
            })?;
            let _ = self.child.wait();
        }
        Ok(())
    }

    fn read_response(&mut self, request_id: u64) -> Result<Value, RuntimeError> {
        for _ in 0..MAX_APP_SERVER_SKIPPED_NOTIFICATIONS {
            let mut bytes = Vec::new();
            let read = self.stdout.read_until(b'\n', &mut bytes).map_err(|error| {
                RuntimeError::Provider(format!("failed to read Codex app-server response: {error}"))
            })?;
            if read == 0 {
                return Err(RuntimeError::Provider(
                    "host-owned Codex app-server closed stdout before a response".into(),
                ));
            }
            if bytes.len() > MAX_APP_SERVER_MESSAGE_BYTES {
                return Err(RuntimeError::BudgetExceeded(
                    "Codex app-server JSONL response exceeds bounded host limit".into(),
                ));
            }
            let response = parse_app_server_jsonl_response(&bytes)?;
            if response.get("id").and_then(Value::as_u64) != Some(request_id) {
                // Supported servers may send notifications interleaved with a
                // response. Any other response id is unsafe to associate.
                if response.get("method").is_some() && response.get("id").is_none() {
                    continue;
                }
                return Err(RuntimeError::Validation(
                    "Codex app-server response id did not match the request".into(),
                ));
            }
            if let Some(error) = response.get("error") {
                return Err(RuntimeError::Provider(format!(
                    "Codex app-server returned a bounded RPC error: {}",
                    bounded_string(&error.to_string(), 512)
                )));
            }
            return Ok(response.get("result").cloned().unwrap_or(response));
        }
        Err(RuntimeError::Validation(
            "Codex app-server emitted too many interleaved notifications".into(),
        ))
    }
}

impl Drop for CodexAppServerStdioTransportV1 {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

impl CodexAppServerTransportV1 for CodexAppServerStdioTransportV1 {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, RuntimeError> {
        if !matches!(method, "initialize" | "thread/resume")
            && !CODEX_APP_SERVER_READ_METHODS_V1.contains(&method)
            && !CODEX_APP_SERVER_CONTINUITY_METHODS_V1.contains(&method)
            && !CODEX_APP_SERVER_GOAL_METHODS_V1.contains(&method)
        {
            return Err(RuntimeError::Validation(
                "unsupported host-owned Codex app-server method".into(),
            ));
        }
        if method != "initialize" && !self.initialized {
            return Err(RuntimeError::Validation(
                "Codex app-server must be initialized before host requests".into(),
            ));
        }
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        let line = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": request_id, "method": method, "params": params
        }))
        .map_err(|_| {
            RuntimeError::Validation("Codex app-server request serialization failed".into())
        })?;
        if line.len() > MAX_APP_SERVER_MESSAGE_BYTES {
            return Err(RuntimeError::BudgetExceeded(
                "Codex app-server request exceeds bounded host limit".into(),
            ));
        }
        self.stdin.write_all(&line).map_err(|error| {
            RuntimeError::Provider(format!("failed to write Codex app-server request: {error}"))
        })?;
        self.stdin.write_all(b"\n").map_err(|error| {
            RuntimeError::Provider(format!(
                "failed to terminate Codex app-server request: {error}"
            ))
        })?;
        self.stdin.flush().map_err(|error| {
            RuntimeError::Provider(format!("failed to flush Codex app-server request: {error}"))
        })?;
        self.read_response(request_id)
    }
}

pub fn parse_app_server_jsonl_response(line: &[u8]) -> Result<Value, RuntimeError> {
    if line.is_empty() || line.len() > MAX_APP_SERVER_MESSAGE_BYTES {
        return Err(RuntimeError::BudgetExceeded(
            "Codex app-server JSONL message is empty or exceeds host limit".into(),
        ));
    }
    let value: Value = serde_json::from_slice(line).map_err(|_| {
        RuntimeError::Validation("Codex app-server returned malformed JSONL".into())
    })?;
    if !value.is_object() {
        return Err(RuntimeError::Validation(
            "Codex app-server JSONL message must be an object".into(),
        ));
    }
    Ok(value)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodexRateLimitWindowV1 {
    pub limit_name: String,
    pub used_percent: Option<u8>,
    pub resets_at_unix: Option<u64>,
    pub reached_limit: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodexRoutingTelemetryV1 {
    pub observed_at_unix: u64,
    pub source: String,
    pub rate_limit_windows: Vec<CodexRateLimitWindowV1>,
    pub reached_limit: bool,
    pub codex_eligible_after_unix: Option<u64>,
    pub plan_type: Option<String>,
    /// The app-server may expose a non-secret balance/credit label. It is
    /// retained as bounded display metadata only; CatDesk has no mutation
    /// method for credits or billing.
    pub credit_balance_metadata: Option<String>,
    pub earned_reset_metadata: Option<String>,
    pub selected_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub thread_token_usage: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexThreadMetadataV1 {
    pub thread_id: String,
    pub cwd: String,
    pub title: Option<String>,
    pub preview: Option<String>,
    pub selected_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub token_usage: Option<String>,
    pub concurrently_owned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodexThreadResolutionV1 {
    Exact(CodexThreadMetadataV1),
    Ambiguous(Vec<CodexThreadMetadataV1>),
    NotFound,
    ConcurrentlyOwned(CodexThreadMetadataV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexHostPreparationV1 {
    pub thread: CodexThreadMetadataV1,
    pub telemetry: CodexRoutingTelemetryV1,
}

/// Bounded evidence returned when the host starts one read-only continuity
/// turn. Completion is checked separately through `thread/turns/list` before
/// a second turn can start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexContinuityTurnV1 {
    pub thread_id: String,
    pub turn_id: String,
    pub status: String,
}

/// Bounded native Goal state used by the host-only deadman resume path.
/// Objective text is intentionally not returned through CatDesk's MCP surface.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexGoalStateV1 {
    pub thread_id: String,
    pub status: String,
}

pub trait CodexAppServerTransportV1 {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, RuntimeError>;
}

/// Stateless helpers over a supported JSON-RPC transport. `resume_exact` is
/// intentionally separate from all discovery/read paths and requires a
/// previously resolved exact, non-owned thread.
pub struct CodexAppServerReadClientV1;

impl CodexAppServerReadClientV1 {
    /// Host-only preflight for a mutating Codex worker turn. A supported
    /// app-server has to provide exact thread metadata proving Terra/High;
    /// a CLI launch flag or an unverified default is not evidence.
    pub fn prepare_mutating_thread<T: CodexAppServerTransportV1>(
        transport: &mut T,
        cwd: &str,
        title_or_search: Option<&str>,
        explicit_thread_id: Option<&str>,
        observed_at_unix: u64,
    ) -> Result<CodexHostPreparationV1, RuntimeError> {
        let resolution = Self::resolve_thread(transport, cwd, title_or_search, explicit_thread_id)?;
        let CodexThreadResolutionV1::Exact(resolved) = &resolution else {
            return Err(RuntimeError::Validation(
                "Codex mutating preflight requires one exact non-owned workspace thread".into(),
            ));
        };

        let mut telemetry =
            Self::read_rate_limit_telemetry_best_effort(transport, observed_at_unix)?;
        let resumed = Self::resume_exact(transport, &resolution)?;
        let authoritative = parse_resumed_thread_metadata(&resumed)?;
        if authoritative.thread_id != resolved.thread_id
            || normalize_cwd(&authoritative.cwd)? != normalize_cwd(&resolved.cwd)?
        {
            return Err(RuntimeError::Validation(
                "Codex metadata-only resume did not preserve the exact resolved thread".into(),
            ));
        }
        enforce_terra_high(&authoritative)?;
        telemetry.selected_model = authoritative.selected_model.clone();
        telemetry.reasoning_effort = authoritative.reasoning_effort.clone();
        telemetry.thread_token_usage = authoritative.token_usage.clone();
        validate_telemetry(&telemetry)?;
        Ok(CodexHostPreparationV1 {
            thread: authoritative,
            telemetry,
        })
    }

    fn read_rate_limit_telemetry_best_effort<T: CodexAppServerTransportV1>(
        transport: &mut T,
        observed_at_unix: u64,
    ) -> Result<CodexRoutingTelemetryV1, RuntimeError> {
        match transport.request("account/rateLimits/read", json!({})) {
            Ok(account) => match parse_rate_limit_telemetry(&account, observed_at_unix) {
                Ok(telemetry) => Ok(telemetry),
                Err(_) => Ok(unavailable_rate_limit_telemetry(observed_at_unix)),
            },
            Err(_) => Ok(unavailable_rate_limit_telemetry(observed_at_unix)),
        }
    }

    pub fn refresh_telemetry<T: CodexAppServerTransportV1>(
        transport: &mut T,
        observed_at_unix: u64,
        exact_thread_id: Option<&str>,
    ) -> Result<CodexRoutingTelemetryV1, RuntimeError> {
        let mut telemetry =
            Self::read_rate_limit_telemetry_best_effort(transport, observed_at_unix)?;
        if let Some(thread_id) = exact_thread_id {
            let thread = transport.request("thread/read", json!({"threadId": thread_id}))?;
            let metadata = parse_thread_metadata(&thread)?;
            if metadata.thread_id != thread_id {
                return Err(RuntimeError::Validation(
                    "Codex app-server thread/read returned a different thread id".into(),
                ));
            }
            let resolution = if metadata.concurrently_owned {
                CodexThreadResolutionV1::ConcurrentlyOwned(metadata)
            } else {
                CodexThreadResolutionV1::Exact(metadata)
            };
            let resumed = Self::resume_exact(transport, &resolution)?;
            let authoritative = parse_resumed_thread_metadata(&resumed)?;
            if authoritative.thread_id != thread_id {
                return Err(RuntimeError::Validation(
                    "Codex metadata-only resume returned a different thread id".into(),
                ));
            }
            telemetry.selected_model = authoritative.selected_model;
            telemetry.reasoning_effort = authoritative.reasoning_effort;
            telemetry.thread_token_usage = authoritative.token_usage;
        }
        validate_telemetry(&telemetry)?;
        Ok(telemetry)
    }

    pub fn resolve_thread<T: CodexAppServerTransportV1>(
        transport: &mut T,
        cwd: &str,
        title_or_search: Option<&str>,
        explicit_thread_id: Option<&str>,
    ) -> Result<CodexThreadResolutionV1, RuntimeError> {
        let expected_cwd = normalize_cwd(cwd)?;
        if let Some(thread_id) = explicit_thread_id {
            let response = transport.request("thread/read", json!({"threadId": thread_id}))?;
            let thread = parse_thread_metadata(&response)?;
            if thread.thread_id != thread_id || normalize_cwd(&thread.cwd)? != expected_cwd {
                return Ok(CodexThreadResolutionV1::NotFound);
            }
            return if thread.concurrently_owned {
                Ok(CodexThreadResolutionV1::ConcurrentlyOwned(thread))
            } else {
                Ok(CodexThreadResolutionV1::Exact(thread))
            };
        }

        let list = transport.request(
            "thread/list",
            json!({
                "sourceKinds": ["exec"],
                "cwd": cwd,
                "limit": THREAD_LIST_PAGE_LIMIT,
                "sortKey": "recency_at",
                "sortDirection": "desc"
            }),
        )?;
        let threads = parse_thread_list(&list)?;
        let query = title_or_search.map(|value| value.trim().to_ascii_lowercase());
        let candidates = threads
            .into_iter()
            .filter(|thread| normalize_cwd(&thread.cwd).ok().as_deref() == Some(&expected_cwd))
            .filter(|thread| {
                query
                    .as_ref()
                    .is_none_or(|needle| thread_matches(thread, needle))
            })
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [] => Ok(CodexThreadResolutionV1::NotFound),
            [thread] if thread.concurrently_owned => {
                Ok(CodexThreadResolutionV1::ConcurrentlyOwned(thread.clone()))
            }
            [thread] => Ok(CodexThreadResolutionV1::Exact(thread.clone())),
            _ => Ok(CodexThreadResolutionV1::Ambiguous(candidates)),
        }
    }

    /// Bounded metadata-only discovery for an operator-confirmed adoption.
    /// Unlike continuity resolution this intentionally does not send a
    /// `sourceKinds` filter: `thread/list` is the supported app-server shared
    /// history surface, and no GUI storage is read or inferred. Every result
    /// is subsequently re-read/resumed before it can become authority.
    pub fn discover_adoptable_threads<T: CodexAppServerTransportV1>(
        transport: &mut T,
        cwd: &str,
        observed_at_unix: u64,
    ) -> Result<Vec<CodexHostPreparationV1>, RuntimeError> {
        let expected_cwd = normalize_cwd(cwd)?;
        let listed = transport.request(
            "thread/list",
            json!({
                "cwd": cwd,
                "limit": 8,
                "sortKey": "recency_at",
                "sortDirection": "desc"
            }),
        )?;
        let mut prepared = Vec::new();
        for listed_thread in parse_thread_list(&listed)?.into_iter().take(8) {
            if listed_thread.concurrently_owned
                || normalize_cwd(&listed_thread.cwd)? != expected_cwd
            {
                continue;
            }
            let mut preparation = Self::prepare_mutating_thread(
                transport,
                cwd,
                None,
                Some(&listed_thread.thread_id),
                observed_at_unix,
            )?;
            if normalize_cwd(&preparation.thread.cwd)? != expected_cwd {
                return Err(RuntimeError::Validation(
                    "Codex adoption metadata cwd drifted".into(),
                ));
            }
            // `thread/resume` is authoritative for scope/model/ownership but
            // may omit the bounded list title. Retain only that display field.
            preparation.thread.title = listed_thread.title.clone();
            if prepared.iter().any(|candidate: &CodexHostPreparationV1| {
                candidate.thread.thread_id == preparation.thread.thread_id
            }) {
                return Err(RuntimeError::Validation(
                    "Codex adoption metadata contains duplicate thread ids".into(),
                ));
            }
            prepared.push(preparation);
        }
        Ok(prepared)
    }

    /// Selects the newest exact-CWD `exec` thread that is already trusted by
    /// CatDesk durable state. Discovery never promotes an arbitrary Codex
    /// history entry into a canonical binding.
    pub fn discover_recent_trusted_exec_thread<T: CodexAppServerTransportV1>(
        transport: &mut T,
        cwd: &str,
        trusted_thread_ids: &[String],
    ) -> Result<Option<String>, RuntimeError> {
        if trusted_thread_ids.is_empty() {
            return Ok(None);
        }
        let expected_cwd = normalize_cwd(cwd)?;
        let mut cursor: Option<String> = None;
        for _ in 0..THREAD_LIST_MAX_PAGES {
            let mut params = json!({
                "sourceKinds": ["exec"],
                "cwd": cwd,
                "limit": THREAD_LIST_PAGE_LIMIT,
                "sortKey": "recency_at",
                "sortDirection": "desc"
            });
            if let Some(value) = &cursor {
                params["cursor"] = Value::String(value.clone());
            }
            let list = transport.request("thread/list", params)?;
            for thread in parse_thread_list(&list)? {
                if normalize_cwd(&thread.cwd)? == expected_cwd
                    && !thread.concurrently_owned
                    && trusted_thread_ids.iter().any(|id| id == &thread.thread_id)
                {
                    return Ok(Some(thread.thread_id));
                }
            }
            cursor = list
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(None)
    }

    /// Discovers shared Desktop/CLI history by one exact normalized title.
    /// This intentionally omits sourceKinds/cwd filters because Desktop-created
    /// threads are part of the same supported app-server history surface.
    /// Ambiguity is retained for the caller to fail closed.
    pub fn discover_exact_title_threads<T: CodexAppServerTransportV1>(
        transport: &mut T,
        title: &str,
    ) -> Result<Vec<CodexThreadMetadataV1>, RuntimeError> {
        let expected = normalize_thread_title(title)?;
        let mut cursor: Option<String> = None;
        let mut matches = Vec::new();
        for _ in 0..THREAD_LIST_MAX_PAGES {
            let mut params = json!({
                "limit": THREAD_LIST_PAGE_LIMIT,
                "sortKey": "recency_at",
                "sortDirection": "desc"
            });
            if let Some(value) = &cursor {
                params["cursor"] = Value::String(value.clone());
            }
            let list = transport.request("thread/list", params)?;
            for thread in parse_thread_list(&list)? {
                if thread
                    .title
                    .as_deref()
                    .and_then(|value| normalize_thread_title(value).ok())
                    .as_deref()
                    == Some(expected.as_str())
                {
                    matches.push(thread);
                    if matches.len() > 8 {
                        return Err(RuntimeError::Validation(
                            "Codex exact-title discovery exceeded its bounded ambiguity limit"
                                .into(),
                        ));
                    }
                }
            }
            cursor = list
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(matches)
    }

    pub fn goal_get<T: CodexAppServerTransportV1>(
        transport: &mut T,
        thread_id: &str,
    ) -> Result<Option<CodexGoalStateV1>, RuntimeError> {
        let response = transport.request("thread/goal/get", json!({"threadId": thread_id}))?;
        parse_goal_response(&response, thread_id, true)
    }

    /// Resumes only an existing paused Goal. It never creates/replaces an
    /// objective or token budget; status-only mutation preserves both.
    pub fn goal_resume_paused<T: CodexAppServerTransportV1>(
        transport: &mut T,
        thread_id: &str,
    ) -> Result<CodexGoalStateV1, RuntimeError> {
        let response = transport.request(
            "thread/goal/set",
            json!({"threadId": thread_id, "status": "active"}),
        )?;
        parse_goal_response(&response, thread_id, false)?.ok_or_else(|| {
            RuntimeError::Validation("Codex thread/goal/set returned no goal".into())
        })
    }

    /// Loads only bounded session metadata. `excludeTurns` prevents a stored
    /// thread's conversation history from being returned through the control
    /// plane while still letting Codex resolve its persisted model/reasoning.
    pub fn resume_exact<T: CodexAppServerTransportV1>(
        transport: &mut T,
        resolution: &CodexThreadResolutionV1,
    ) -> Result<Value, RuntimeError> {
        let CodexThreadResolutionV1::Exact(thread) = resolution else {
            return Err(RuntimeError::Validation(
                "Codex thread resume requires one exact non-owned thread".into(),
            ));
        };
        transport.request(
            "thread/resume",
            json!({"threadId": thread.thread_id, "excludeTurns": true}),
        )
    }

    /// Starts an intentionally harmless acceptance turn only after an exact,
    /// idle/direct-input Terra/High preflight. The prompt, model, effort, and
    /// sandbox are fixed so MCP and worker prompts cannot broaden it.
    pub fn start_harmless_continuity_turn<T: CodexAppServerTransportV1>(
        transport: &mut T,
        preparation: &CodexHostPreparationV1,
        sequence: u8,
    ) -> Result<CodexContinuityTurnV1, RuntimeError> {
        if !(1..=2).contains(&sequence) {
            return Err(RuntimeError::Validation(
                "Codex continuity turn sequence must be one or two".into(),
            ));
        }
        enforce_terra_high(&preparation.thread)?;
        if preparation.thread.concurrently_owned {
            return Err(RuntimeError::Validation(
                "Codex continuity turn requires an idle direct-input thread".into(),
            ));
        }
        let prompt = match sequence {
            1 => {
                "CatDesk continuity acceptance step 1. Do not modify files, run commands, use tools, or access the network. Reply only with a concise acceptance note that this exact existing workspace thread is available."
            }
            2 => {
                "CatDesk continuity acceptance step 2. Do not modify files, run commands, use tools, or access the network. Reply only with a concise note confirming this is the same existing workspace thread as the preceding continuity turn."
            }
            _ => unreachable!(),
        };
        let response = transport.request(
            "turn/start",
            json!({
                "threadId": preparation.thread.thread_id,
                "cwd": preparation.thread.cwd,
                "model": CATDESK_REQUIRED_CODEX_MODEL_V1,
                "effort": CATDESK_REQUIRED_CODEX_REASONING_EFFORT_V1,
                "approvalPolicy": "never",
                "sandboxPolicy": {"type": "readOnly", "networkAccess": false},
                "input": [{"type": "text", "text": prompt}]
            }),
        )?;
        parse_continuity_turn_start(&response, &preparation.thread.thread_id)
    }

    /// Reads only the status of a specific prior continuity turn. The caller
    /// must observe completion before beginning sequence two.
    pub fn continuity_turn_completed<T: CodexAppServerTransportV1>(
        transport: &mut T,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<bool, RuntimeError> {
        let response = transport.request(
            "thread/turns/list",
            json!({"threadId": thread_id, "limit": 20, "itemsView": "notLoaded", "sortDirection": "desc"}),
        )?;
        parse_continuity_turn_completed(&response, turn_id)
    }
}

pub fn parse_continuity_turn_start(
    value: &Value,
    expected_thread_id: &str,
) -> Result<CodexContinuityTurnV1, RuntimeError> {
    let payload = value.get("result").unwrap_or(value);
    let turn = payload.get("turn").unwrap_or(payload);
    let turn_id = turn.get("id").and_then(Value::as_str).ok_or_else(|| {
        RuntimeError::Validation("Codex turn/start returned no bounded turn id".into())
    })?;
    if turn_id.is_empty() || turn_id.len() > 256 {
        return Err(RuntimeError::Validation(
            "Codex turn/start returned an invalid turn id".into(),
        ));
    }
    if let Some(thread_id) = turn
        .get("threadId")
        .or_else(|| payload.get("threadId"))
        .and_then(Value::as_str)
        && thread_id != expected_thread_id
    {
        return Err(RuntimeError::Validation(
            "Codex turn/start returned a different thread id".into(),
        ));
    }
    let status = turn_status(turn).ok_or_else(|| {
        RuntimeError::Validation("Codex turn/start returned no bounded turn status".into())
    })?;
    Ok(CodexContinuityTurnV1 {
        thread_id: bounded_string(expected_thread_id, 256),
        turn_id: bounded_string(turn_id, 256),
        status,
    })
}

pub fn parse_continuity_turn_completed(value: &Value, turn_id: &str) -> Result<bool, RuntimeError> {
    let payload = value.get("result").unwrap_or(value);
    let turns = payload
        .get("turns")
        .or_else(|| payload.get("items"))
        .or_else(|| payload.get("data"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            RuntimeError::Validation("Codex thread/turns/list returned no bounded turn list".into())
        })?;
    let turn = turns
        .iter()
        .find(|turn| turn.get("id").and_then(Value::as_str) == Some(turn_id))
        .ok_or_else(|| {
            RuntimeError::Validation("continuity turn was not found on its thread".into())
        })?;
    let status = turn_status(turn)
        .ok_or_else(|| RuntimeError::Validation("continuity turn has no bounded status".into()))?;
    Ok(matches!(status.as_str(), "completed" | "complete"))
}

fn turn_status(value: &Value) -> Option<String> {
    let status = value.get("status")?;
    let value = status
        .as_str()
        .or_else(|| status.get("type").and_then(Value::as_str))?;
    if value.is_empty() || value.len() > 128 || contains_secret_marker(value) {
        None
    } else {
        Some(value.to_ascii_lowercase())
    }
}

/// Rejects both mismatch and unknown metadata. This is deliberately strict:
/// explicit process flags select intent, while app-server thread metadata is
/// the authoritative evidence required before CatDesk permits mutation.
pub fn enforce_terra_high(thread: &CodexThreadMetadataV1) -> Result<(), RuntimeError> {
    if thread.selected_model.as_deref() != Some(CATDESK_REQUIRED_CODEX_MODEL_V1)
        || thread.reasoning_effort.as_deref() != Some(CATDESK_REQUIRED_CODEX_REASONING_EFFORT_V1)
    {
        return Err(RuntimeError::Validation(
            "Codex mutating turn blocked: authoritative app-server metadata is not gpt-5.6-terra/high".into(),
        ));
    }
    Ok(())
}

pub fn parse_rate_limit_telemetry(
    value: &Value,
    observed_at_unix: u64,
) -> Result<CodexRoutingTelemetryV1, RuntimeError> {
    let payload = value.get("result").unwrap_or(value);
    let empty_windows = Vec::new();
    let windows = payload
        .get("rateLimits")
        .or_else(|| payload.get("windows"))
        .and_then(Value::as_array)
        .unwrap_or(&empty_windows)
        .iter()
        .take(16)
        .enumerate()
        .map(|(index, window)| CodexRateLimitWindowV1 {
            limit_name: bounded_string(
                window
                    .get("name")
                    .or_else(|| window.get("limitName"))
                    .and_then(Value::as_str)
                    .unwrap_or(if index == 0 { "primary" } else { "window" }),
                128,
            ),
            used_percent: number_u8(
                window
                    .get("usedPercent")
                    .or_else(|| window.get("used_percent")),
            ),
            resets_at_unix: number_u64(window.get("resetsAt").or_else(|| window.get("resets_at"))),
            reached_limit: bool_value(
                window
                    .get("reachedLimit")
                    .or_else(|| window.get("reached_limit")),
            ),
        })
        .collect::<Vec<_>>();
    let reached_limit = bool_value(
        payload
            .get("reachedLimit")
            .or_else(|| payload.get("reached_limit")),
    ) || windows.iter().any(|window| window.reached_limit);
    let codex_eligible_after_unix = if reached_limit {
        windows
            .iter()
            .filter(|window| window.reached_limit)
            .filter_map(|window| window.resets_at_unix)
            .max()
    } else {
        None
    };
    let telemetry = CodexRoutingTelemetryV1 {
        observed_at_unix,
        source: "codex-app-server/account-rateLimits-read".into(),
        rate_limit_windows: windows,
        reached_limit,
        codex_eligible_after_unix,
        plan_type: optional_bounded(payload.get("planType").and_then(Value::as_str), 256),
        credit_balance_metadata: optional_bounded(
            payload
                .get("creditBalance")
                .or_else(|| payload.get("credits"))
                .and_then(string_or_number),
            256,
        ),
        earned_reset_metadata: optional_bounded(
            payload.get("earnedReset").and_then(string_or_number),
            256,
        ),
        selected_model: None,
        reasoning_effort: None,
        thread_token_usage: None,
    };
    validate_telemetry(&telemetry)?;
    Ok(telemetry)
}

fn unavailable_rate_limit_telemetry(observed_at_unix: u64) -> CodexRoutingTelemetryV1 {
    CodexRoutingTelemetryV1 {
        observed_at_unix,
        source: "codex-app-server/account-rateLimits-unavailable".into(),
        rate_limit_windows: Vec::new(),
        reached_limit: false,
        codex_eligible_after_unix: None,
        plan_type: None,
        credit_balance_metadata: None,
        earned_reset_metadata: None,
        selected_model: None,
        reasoning_effort: None,
        thread_token_usage: None,
    }
}

/// The optional `account/rateLimits/updated` notification uses the same
/// bounded payload schema as the read response. Callers may persist this only
/// after receiving it from an already-established supported app-server
/// transport; no polling, auth scraping, or billing action is implied.
pub fn parse_rate_limit_updated(
    value: &Value,
    observed_at_unix: u64,
) -> Result<CodexRoutingTelemetryV1, RuntimeError> {
    let mut telemetry = parse_rate_limit_telemetry(value, observed_at_unix)?;
    telemetry.source = "codex-app-server/account-rateLimits-updated".into();
    Ok(telemetry)
}

pub fn parse_thread_metadata(value: &Value) -> Result<CodexThreadMetadataV1, RuntimeError> {
    let payload = value.get("result").unwrap_or(value);
    let payload = payload.get("thread").unwrap_or(payload);
    let thread_id = payload
        .get("id")
        .or_else(|| payload.get("threadId"))
        .and_then(Value::as_str)
        .ok_or_else(|| RuntimeError::Validation("Codex app-server thread has no id".into()))?;
    let cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .ok_or_else(|| RuntimeError::Validation("Codex app-server thread has no cwd".into()))?;
    Ok(CodexThreadMetadataV1 {
        thread_id: bounded_string(thread_id, 256),
        cwd: normalize_cwd(cwd)?,
        title: optional_bounded(
            payload
                .get("title")
                .or_else(|| payload.get("name"))
                .and_then(Value::as_str),
            512,
        ),
        preview: optional_bounded(payload.get("preview").and_then(Value::as_str), 512),
        selected_model: optional_bounded(
            payload
                .get("model")
                .or_else(|| payload.get("selectedModel"))
                .and_then(Value::as_str),
            256,
        ),
        reasoning_effort: optional_bounded(
            payload.get("reasoningEffort").and_then(Value::as_str),
            128,
        ),
        token_usage: optional_bounded(payload.get("tokenUsage").and_then(string_or_number), 256),
        concurrently_owned: bool_value(
            payload
                .get("concurrentlyOwned")
                .or_else(|| payload.get("activeWriter")),
        ) || payload
            .get("canAcceptDirectInput")
            .and_then(Value::as_bool)
            .is_some_and(|can_accept| !can_accept),
    })
}

pub fn parse_resumed_thread_metadata(value: &Value) -> Result<CodexThreadMetadataV1, RuntimeError> {
    let payload = value.get("result").unwrap_or(value);
    let thread_value = payload.get("thread").ok_or_else(|| {
        RuntimeError::Validation("Codex thread/resume returned no thread metadata".into())
    })?;
    let mut metadata = parse_thread_metadata(thread_value)?;
    if thread_value
        .get("canAcceptDirectInput")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err(RuntimeError::Validation(
            "Codex resumed thread is not idle/direct-input eligible".into(),
        ));
    }
    if let Some(cwd) = payload.get("cwd").and_then(Value::as_str) {
        if normalize_cwd(cwd)? != normalize_cwd(&metadata.cwd)? {
            return Err(RuntimeError::Validation(
                "Codex thread/resume returned inconsistent cwd metadata".into(),
            ));
        }
    }
    metadata.selected_model = optional_bounded(
        payload
            .get("model")
            .or_else(|| payload.get("selectedModel"))
            .and_then(Value::as_str),
        256,
    );
    metadata.reasoning_effort =
        optional_bounded(payload.get("reasoningEffort").and_then(Value::as_str), 128);
    metadata.token_usage = optional_bounded(
        payload
            .get("tokenUsage")
            .or_else(|| thread_value.get("tokenUsage"))
            .and_then(string_or_number),
        256,
    );
    metadata.concurrently_owned = false;
    Ok(metadata)
}

pub fn parse_thread_list(value: &Value) -> Result<Vec<CodexThreadMetadataV1>, RuntimeError> {
    let payload = value.get("result").unwrap_or(value);
    let items = payload
        .get("threads")
        .or_else(|| payload.get("items"))
        .or_else(|| payload.get("data"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            RuntimeError::Validation("Codex app-server thread/list returned no thread list".into())
        })?;
    items.iter().take(128).map(parse_thread_metadata).collect()
}

pub fn validate_telemetry(telemetry: &CodexRoutingTelemetryV1) -> Result<(), RuntimeError> {
    if !matches!(
        telemetry.source.as_str(),
        "codex-app-server/account-rateLimits-read"
            | "codex-app-server/account-rateLimits-updated"
            | "codex-app-server/account-rateLimits-unavailable"
    ) || telemetry.rate_limit_windows.len() > 16
        || (telemetry.source == "codex-app-server/account-rateLimits-unavailable"
            && (!telemetry.rate_limit_windows.is_empty()
                || telemetry.reached_limit
                || telemetry.codex_eligible_after_unix.is_some()
                || telemetry.plan_type.is_some()
                || telemetry.credit_balance_metadata.is_some()
                || telemetry.earned_reset_metadata.is_some()))
        || telemetry
            .rate_limit_windows
            .iter()
            .any(|window| window.limit_name.is_empty() || window.limit_name.len() > 128)
        || [
            telemetry.plan_type.as_deref(),
            telemetry.credit_balance_metadata.as_deref(),
            telemetry.earned_reset_metadata.as_deref(),
            telemetry.selected_model.as_deref(),
            telemetry.reasoning_effort.as_deref(),
            telemetry.thread_token_usage.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|value| value.len() > 512 || contains_secret_marker(value))
    {
        return Err(RuntimeError::Validation(
            "Codex routing telemetry is not bounded or is unsafe to persist".into(),
        ));
    }
    Ok(())
}

fn normalize_thread_title(value: &str) -> Result<String, RuntimeError> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() || normalized.len() > 512 || contains_secret_marker(&normalized) {
        return Err(RuntimeError::Validation(
            "Codex thread title is empty, oversized, or unsafe".into(),
        ));
    }
    Ok(normalized.to_ascii_lowercase())
}

fn parse_goal_response(
    value: &Value,
    expected_thread_id: &str,
    allow_missing: bool,
) -> Result<Option<CodexGoalStateV1>, RuntimeError> {
    let payload = value.get("result").unwrap_or(value);
    let Some(goal) = payload.get("goal") else {
        return if allow_missing {
            Ok(None)
        } else {
            Err(RuntimeError::Validation(
                "Codex goal response contained no goal".into(),
            ))
        };
    };
    if goal.is_null() {
        return if allow_missing {
            Ok(None)
        } else {
            Err(RuntimeError::Validation(
                "Codex goal response contained no goal".into(),
            ))
        };
    }
    let thread_id = goal
        .get("threadId")
        .and_then(Value::as_str)
        .ok_or_else(|| RuntimeError::Validation("Codex goal has no thread id".into()))?;
    if thread_id != expected_thread_id {
        return Err(RuntimeError::Validation(
            "Codex goal response returned a different thread id".into(),
        ));
    }
    let status = goal
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| RuntimeError::Validation("Codex goal has no status".into()))?;
    if !matches!(
        status,
        "active" | "paused" | "blocked" | "usageLimited" | "budgetLimited" | "complete"
    ) {
        return Err(RuntimeError::Validation(
            "Codex goal returned an unsupported status".into(),
        ));
    }
    Ok(Some(CodexGoalStateV1 {
        thread_id: bounded_string(thread_id, 256),
        status: status.to_string(),
    }))
}

fn thread_matches(thread: &CodexThreadMetadataV1, needle: &str) -> bool {
    thread
        .title
        .as_deref()
        .is_some_and(|title| title.to_ascii_lowercase().contains(needle))
        || thread
            .preview
            .as_deref()
            .is_some_and(|preview| preview.to_ascii_lowercase().contains(needle))
}

fn normalize_cwd(cwd: &str) -> Result<String, RuntimeError> {
    let mut value = cwd.trim().replace('\\', "/");
    if value
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("//?/"))
    {
        value = value.get(4..).unwrap_or_default().to_string();
        if value
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("unc/"))
        {
            value = format!("//{}", value.get(4..).unwrap_or_default());
        }
    }
    if value.is_empty() || value.len() > 4096 || value.contains("/../") || value.ends_with("/..") {
        return Err(RuntimeError::Validation(
            "Codex thread cwd is invalid".into(),
        ));
    }
    Ok(value.trim_end_matches('/').to_ascii_lowercase())
}

fn bool_value(value: Option<&Value>) -> bool {
    value.and_then(Value::as_bool).unwrap_or(false)
}
fn number_u8(value: Option<&Value>) -> Option<u8> {
    value
        .and_then(Value::as_u64)
        .and_then(|v| u8::try_from(v).ok())
}
fn number_u64(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64)
}
fn string_or_number(value: &Value) -> Option<&str> {
    value.as_str()
}
fn optional_bounded(value: Option<&str>, limit: usize) -> Option<String> {
    value.map(|value| bounded_string(value, limit))
}
fn bounded_string(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::path::PathBuf;

    struct FakeTransport {
        replies: VecDeque<Value>,
        methods: Vec<String>,
    }
    impl CodexAppServerTransportV1 for FakeTransport {
        fn request(&mut self, method: &str, _params: Value) -> Result<Value, RuntimeError> {
            self.methods.push(method.into());
            self.replies
                .pop_front()
                .ok_or_else(|| RuntimeError::Validation("no reply".into()))
        }
    }

    struct AccountUnavailableTransport {
        methods: Vec<String>,
    }
    impl CodexAppServerTransportV1 for AccountUnavailableTransport {
        fn request(&mut self, method: &str, _params: Value) -> Result<Value, RuntimeError> {
            self.methods.push(method.into());
            match method {
                "thread/read" => Ok(json!({"thread":{
                    "id":"thread-one",
                    "cwd":"C:/work/project",
                    "canAcceptDirectInput":null,
                    "status":{"type":"notLoaded"}
                }})),
                "account/rateLimits/read" => Err(RuntimeError::Provider(
                    "usage endpoint temporarily unavailable".into(),
                )),
                "thread/resume" => Ok(json!({
                    "thread":{
                        "id":"thread-one",
                        "cwd":"C:/work/project",
                        "canAcceptDirectInput":true,
                        "status":{"type":"idle"}
                    },
                    "cwd":"C:/work/project",
                    "model":"gpt-5.6-terra",
                    "reasoningEffort":"high"
                })),
                _ => Err(RuntimeError::Validation("unexpected method".into())),
            }
        }
    }

    #[test]
    fn exact_title_goal_resume_uses_native_goal_rpc_and_preserves_objective() {
        let mut transport = FakeTransport {
            replies: VecDeque::from([
                json!({"threads":[{
                    "id":"goal-thread",
                    "cwd":"C:/work/project",
                    "title":"Implement guide features to working",
                    "canAcceptDirectInput":true
                }]}),
                json!({"goal":{
                    "threadId":"goal-thread",
                    "objective":"keep implementing",
                    "status":"paused",
                    "tokenBudget":1234,
                    "tokensUsed":10,
                    "timeUsedSeconds":2,
                    "createdAt":1,
                    "updatedAt":2
                }}),
                json!({"goal":{
                    "threadId":"goal-thread",
                    "objective":"keep implementing",
                    "status":"active",
                    "tokenBudget":1234,
                    "tokensUsed":10,
                    "timeUsedSeconds":2,
                    "createdAt":1,
                    "updatedAt":3
                }}),
            ]),
            methods: Vec::new(),
        };
        let matches = CodexAppServerReadClientV1::discover_exact_title_threads(
            &mut transport,
            "  implement   GUIDE features to working ",
        )
        .expect("exact normalized title");
        assert_eq!(matches.len(), 1);
        let goal = CodexAppServerReadClientV1::goal_get(&mut transport, "goal-thread")
            .expect("goal/get")
            .expect("persisted goal");
        assert_eq!(goal.status, "paused");
        let resumed = CodexAppServerReadClientV1::goal_resume_paused(&mut transport, "goal-thread")
            .expect("goal/set active");
        assert_eq!(resumed.status, "active");
        assert_eq!(
            transport.methods,
            ["thread/list", "thread/goal/get", "thread/goal/set"]
        );
    }

    #[test]
    fn goal_parser_fails_closed_on_thread_or_status_drift() {
        assert!(
            parse_goal_response(
                &json!({"goal":{"threadId":"other","status":"paused"}}),
                "expected",
                true
            )
            .is_err()
        );
        assert!(
            parse_goal_response(
                &json!({"goal":{"threadId":"expected","status":"mystery"}}),
                "expected",
                true
            )
            .is_err()
        );
        assert_eq!(
            parse_goal_response(&json!({"goal":null}), "expected", true).expect("missing goal"),
            None
        );
    }

    #[test]
    fn reached_window_sets_reset_aware_eligibility_without_billing_mutation() {
        let value = json!({"rateLimits":[{"name":"weekly","usedPercent":100,"reachedLimit":true,"resetsAt":500},{"name":"short","usedPercent":2,"resetsAt":100}] ,"planType":"plus","creditBalance":"visible"});
        let telemetry = parse_rate_limit_telemetry(&value, 10).expect("telemetry");
        assert_eq!(telemetry.codex_eligible_after_unix, Some(500));
        assert_eq!(telemetry.plan_type.as_deref(), Some("plus"));
        assert!(
            !CODEX_APP_SERVER_READ_METHODS_V1
                .iter()
                .any(|method| method.contains("redeem") || method.contains("billing"))
        );
    }

    #[test]
    fn unavailable_account_telemetry_does_not_block_authoritative_terra_high() {
        let mut transport = AccountUnavailableTransport {
            methods: Vec::new(),
        };
        let preparation = CodexAppServerReadClientV1::prepare_mutating_thread(
            &mut transport,
            "C:/work/project",
            None,
            Some("thread-one"),
            10,
        )
        .expect("Terra/High preflight should survive unavailable usage telemetry");
        assert_eq!(
            preparation.telemetry.source,
            "codex-app-server/account-rateLimits-unavailable"
        );
        assert!(!preparation.telemetry.reached_limit);
        assert!(preparation.telemetry.codex_eligible_after_unix.is_none());
        assert_eq!(
            preparation.thread.selected_model.as_deref(),
            Some("gpt-5.6-terra")
        );
        assert_eq!(preparation.thread.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(
            transport.methods,
            ["thread/read", "account/rateLimits/read", "thread/resume"]
        );
    }

    #[test]
    fn exact_cwd_resolution_fails_closed_for_ambiguity_and_writer_ownership() {
        let listing = json!({"threads":[
            {"id":"one","cwd":"C:/work/project","title":"Polish BYOVD driver pipeline"},
            {"id":"two","cwd":"C:/work/project","preview":"Polish BYOVD driver pipeline"}
        ]});
        let mut transport = FakeTransport {
            replies: VecDeque::from([listing]),
            methods: Vec::new(),
        };
        assert!(matches!(
            CodexAppServerReadClientV1::resolve_thread(
                &mut transport,
                "C:\\work\\project",
                Some("BYOVD"),
                None
            )
            .expect("resolve"),
            CodexThreadResolutionV1::Ambiguous(_)
        ));
        let mut transport = FakeTransport {
            replies: VecDeque::from([
                json!({"thread":{"id":"one","cwd":"C:/work/project","title":"x","activeWriter":true}}),
            ]),
            methods: Vec::new(),
        };
        let resolution = CodexAppServerReadClientV1::resolve_thread(
            &mut transport,
            "C:/work/project",
            None,
            Some("one"),
        )
        .expect("resolve");
        assert!(matches!(
            resolution,
            CodexThreadResolutionV1::ConcurrentlyOwned(_)
        ));
        assert!(CodexAppServerReadClientV1::resume_exact(&mut transport, &resolution).is_err());
    }

    #[test]
    fn current_app_server_data_and_name_shape_is_supported_fail_closed() {
        let mut transport = FakeTransport {
            replies: VecDeque::from([json!({"data":[{
                "id":"thread-one",
                "cwd":"C:/work/project",
                "name":"Integrate Codex MCP for ChatGPT",
                "canAcceptDirectInput":false
            }],"nextCursor":null})]),
            methods: Vec::new(),
        };
        let resolution = CodexAppServerReadClientV1::resolve_thread(
            &mut transport,
            "C:/work/project",
            Some("Integrate Codex MCP for ChatGPT"),
            None,
        )
        .expect("resolve current shape");
        assert!(matches!(
            resolution,
            CodexThreadResolutionV1::ConcurrentlyOwned(_)
        ));
    }

    #[test]
    fn adoption_discovery_uses_supported_metadata_and_rechecks_terra_high() {
        let mut transport = FakeTransport {
            replies: VecDeque::from([
                json!({"threads":[{"id":"existing","cwd":"C:/work/project","name":"Visible thread"}]}),
                json!({"thread":{"id":"existing","cwd":"C:/work/project","canAcceptDirectInput":null}}),
                json!({"rateLimits":[]}),
                json!({"thread":{"id":"existing","cwd":"C:/work/project","canAcceptDirectInput":true},"cwd":"C:/work/project","model":"gpt-5.6-terra","reasoningEffort":"high"}),
            ]),
            methods: Vec::new(),
        };
        let candidates = CodexAppServerReadClientV1::discover_adoptable_threads(
            &mut transport,
            "C:/work/project",
            10,
        )
        .expect("metadata-only adoption candidates");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].thread.thread_id, "existing");
        assert_eq!(
            candidates[0].thread.title.as_deref(),
            Some("Visible thread")
        );
        assert_eq!(
            transport.methods,
            [
                "thread/list",
                "thread/read",
                "account/rateLimits/read",
                "thread/resume"
            ]
        );
    }

    #[test]
    fn refresh_reads_only_account_and_the_exact_thread_metadata() {
        let mut transport = FakeTransport {
            replies: VecDeque::from([
                json!({"rateLimits":[]}),
                json!({"thread":{"id":"thread-1","cwd":"C:/work/project","canAcceptDirectInput":null}}),
                json!({"thread":{"id":"thread-1","cwd":"C:/work/project","canAcceptDirectInput":true,"status":{"type":"idle"}},"cwd":"C:/work/project","model":"gpt-5","reasoningEffort":"high","tokenUsage":"42"}),
            ]),
            methods: Vec::new(),
        };
        let telemetry =
            CodexAppServerReadClientV1::refresh_telemetry(&mut transport, 1, Some("thread-1"))
                .expect("refresh");
        assert_eq!(telemetry.selected_model.as_deref(), Some("gpt-5"));
        assert_eq!(
            transport.methods,
            ["account/rateLimits/read", "thread/read", "thread/resume"]
        );
    }

    #[test]
    fn app_server_launch_rejects_shell_shims_and_missing_operator_context() {
        let directory = std::env::current_dir().expect("working directory");
        assert!(
            CodexAppServerLaunchConfigV1::new(
                PathBuf::from("C:\\temp\\codex.ps1"),
                directory.clone(),
                None,
            )
            .is_err()
        );
        let executable = std::env::current_exe().expect("test executable");
        let missing_context = directory.join("missing-codex-context");
        assert!(
            CodexAppServerLaunchConfigV1::new(executable, directory, Some(missing_context),)
                .is_err()
        );
    }

    #[test]
    fn app_server_launch_hands_only_the_operator_context_path_to_the_child() {
        let directory = std::env::current_dir().expect("working directory");
        let config = CodexAppServerLaunchConfigV1::new(
            std::env::current_exe().expect("test executable"),
            directory.clone(),
            Some(directory.clone()),
        )
        .expect("launch config");
        let command = config.stdio_command();
        assert!(
            command.get_envs().any(|(name, value)| {
                name == "CODEX_HOME" && value == Some(directory.as_os_str())
            })
        );
        assert!(
            command
                .get_envs()
                .any(|(name, value)| name == "CODEX_API_KEY" && value.is_none())
        );
        assert!(
            command
                .get_envs()
                .any(|(name, value)| { name == "CATDESK_CODEX_HOME" && value.is_none() })
        );
        assert!(
            command
                .get_envs()
                .any(|(name, value)| { name == "CATDESK_CODEX_CLI_EXECUTABLE" && value.is_none() })
        );
    }

    #[test]
    fn override_free_launch_clears_recovery_and_api_variables_but_keeps_current_user_context() {
        let directory = std::env::current_dir().expect("working directory");
        let config = CodexAppServerLaunchConfigV1::new(
            std::env::current_exe().expect("test executable"),
            directory,
            None,
        )
        .expect("override-free launch config");
        let command = config.stdio_command();
        for name in [
            "CATDESK_CODEX_HOME",
            "CATDESK_CODEX_CLI_EXECUTABLE",
            "CODEX_HOME",
            "CODEX_API_KEY",
            "OPENAI_API_KEY",
            "CONTROL_PLANE_API_KEY",
        ] {
            assert!(
                command
                    .get_envs()
                    .any(|(configured, value)| configured == name && value.is_none()),
                "{name} must be cleared from the app-server child"
            );
        }
        assert!(
            !command.get_envs().any(|(name, _)| name == "USERPROFILE"),
            "ordinary current-user environment variables are inherited"
        );
    }

    #[test]
    fn app_server_jsonl_parser_is_bounded_and_rejects_malformed_messages() {
        assert!(parse_app_server_jsonl_response(br#"{"id":1,"result":{}}"#).is_ok());
        assert!(parse_app_server_jsonl_response(b"not-json").is_err());
        assert!(parse_app_server_jsonl_response(b"[]").is_err());
        assert!(
            parse_app_server_jsonl_response(&vec![b'x'; MAX_APP_SERVER_MESSAGE_BYTES + 1]).is_err()
        );
    }

    #[test]
    fn terra_high_gate_rejects_unknown_and_mismatched_thread_metadata() {
        let base = CodexThreadMetadataV1 {
            thread_id: "thread-one".into(),
            cwd: "c:/work/project".into(),
            title: None,
            preview: None,
            selected_model: None,
            reasoning_effort: None,
            token_usage: None,
            concurrently_owned: false,
        };
        assert!(enforce_terra_high(&base).is_err());
        let mismatch = CodexThreadMetadataV1 {
            selected_model: Some("gpt-5.6-terra".into()),
            reasoning_effort: Some("medium".into()),
            ..base.clone()
        };
        assert!(enforce_terra_high(&mismatch).is_err());
        let accepted = CodexThreadMetadataV1 {
            selected_model: Some(CATDESK_REQUIRED_CODEX_MODEL_V1.into()),
            reasoning_effort: Some(CATDESK_REQUIRED_CODEX_REASONING_EFFORT_V1.into()),
            ..base
        };
        assert!(enforce_terra_high(&accepted).is_ok());
    }

    #[test]
    fn unloaded_exec_null_direct_input_is_not_treated_as_concurrent_ownership() {
        let metadata = parse_thread_metadata(&json!({"thread":{
            "id":"thread-one",
            "cwd":"C:/work/project",
            "status":{"type":"notLoaded"},
            "canAcceptDirectInput":null
        }}))
        .expect("unloaded metadata");
        assert!(!metadata.concurrently_owned);
    }

    #[test]
    fn windows_extended_length_cwd_matches_normal_drive_and_unc_forms() {
        assert_eq!(
            normalize_cwd(r"\\?\C:\fixture-root\Project").expect("extended drive"),
            normalize_cwd(r"C:\fixture-root\Project").expect("normal drive")
        );
        assert_eq!(
            normalize_cwd(r"\\?\UNC\server\share\Project").expect("extended UNC"),
            normalize_cwd(r"\\server\share\Project").expect("normal UNC")
        );
    }

    #[test]
    fn metadata_only_resume_is_authoritative_and_requires_direct_input() {
        let accepted = parse_resumed_thread_metadata(&json!({
            "thread":{
                "id":"thread-one",
                "cwd":"C:/work/project",
                "status":{"type":"idle"},
                "canAcceptDirectInput":true
            },
            "cwd":"C:/work/project",
            "model":"gpt-5.6-terra",
            "reasoningEffort":"high"
        }))
        .expect("authoritative resume");
        enforce_terra_high(&accepted).expect("Terra High");
        assert!(
            parse_resumed_thread_metadata(&json!({
                "thread":{
                    "id":"thread-one",
                    "cwd":"C:/work/project",
                    "canAcceptDirectInput":null
                },
                "model":"gpt-5.6-terra",
                "reasoningEffort":"high"
            }))
            .is_err()
        );
    }

    #[test]
    fn trusted_exec_discovery_paginates_without_promoting_arbitrary_history() {
        let mut transport = FakeTransport {
            replies: VecDeque::from([
                json!({"data":[
                    {"id":"arbitrary","cwd":"C:/work/project","canAcceptDirectInput":null}
                ],"nextCursor":"page-2"}),
                json!({"data":[
                    {"id":"trusted-thread","cwd":"C:/work/project","canAcceptDirectInput":null}
                ],"nextCursor":null}),
            ]),
            methods: Vec::new(),
        };
        let trusted = vec!["trusted-thread".to_string()];
        assert_eq!(
            CodexAppServerReadClientV1::discover_recent_trusted_exec_thread(
                &mut transport,
                "C:/work/project",
                &trusted,
            )
            .expect("trusted discovery")
            .as_deref(),
            Some("trusted-thread")
        );
        assert_eq!(transport.methods, ["thread/list", "thread/list"]);
    }

    #[test]
    fn bounded_continuity_turn_uses_same_thread_read_only_terra_high_request() {
        struct CaptureTransport {
            method: Option<String>,
            params: Option<Value>,
        }
        impl CodexAppServerTransportV1 for CaptureTransport {
            fn request(&mut self, method: &str, params: Value) -> Result<Value, RuntimeError> {
                self.method = Some(method.into());
                self.params = Some(params);
                Ok(json!({"turn":{"id":"turn-one","threadId":"thread-one","status":"inProgress"}}))
            }
        }
        let preparation = CodexHostPreparationV1 {
            thread: CodexThreadMetadataV1 {
                thread_id: "thread-one".into(),
                cwd: "c:/work/project".into(),
                title: None,
                preview: None,
                selected_model: Some(CATDESK_REQUIRED_CODEX_MODEL_V1.into()),
                reasoning_effort: Some(CATDESK_REQUIRED_CODEX_REASONING_EFFORT_V1.into()),
                token_usage: None,
                concurrently_owned: false,
            },
            telemetry: unavailable_rate_limit_telemetry(1),
        };
        let mut transport = CaptureTransport {
            method: None,
            params: None,
        };
        let turn = CodexAppServerReadClientV1::start_harmless_continuity_turn(
            &mut transport,
            &preparation,
            1,
        )
        .expect("turn start");
        assert_eq!(turn.thread_id, "thread-one");
        assert_eq!(turn.turn_id, "turn-one");
        assert_eq!(transport.method.as_deref(), Some("turn/start"));
        let params = transport.params.expect("params");
        assert_eq!(params["threadId"], "thread-one");
        assert_eq!(params["model"], CATDESK_REQUIRED_CODEX_MODEL_V1);
        assert_eq!(params["effort"], CATDESK_REQUIRED_CODEX_REASONING_EFFORT_V1);
        assert_eq!(params["approvalPolicy"], "never");
        assert_eq!(params["sandboxPolicy"]["type"], "readOnly");
        assert_eq!(params["sandboxPolicy"]["networkAccess"], false);
    }

    #[test]
    fn both_continuity_turns_stay_on_the_existing_thread_without_gui_or_browser_authority() {
        struct SequenceTransport {
            calls: Vec<(String, Value)>,
        }
        impl CodexAppServerTransportV1 for SequenceTransport {
            fn request(&mut self, method: &str, params: Value) -> Result<Value, RuntimeError> {
                self.calls.push((method.into(), params));
                Ok(json!({"turn": {
                    "id": format!("turn-{}", self.calls.len()),
                    "threadId": "existing-gui-visible-thread",
                    "status": "inProgress"
                }}))
            }
        }
        let preparation = CodexHostPreparationV1 {
            thread: CodexThreadMetadataV1 {
                thread_id: "existing-gui-visible-thread".into(),
                cwd: "c:/work/project".into(),
                title: Some("existing desktop history".into()),
                preview: None,
                selected_model: Some(CATDESK_REQUIRED_CODEX_MODEL_V1.into()),
                reasoning_effort: Some(CATDESK_REQUIRED_CODEX_REASONING_EFFORT_V1.into()),
                token_usage: None,
                concurrently_owned: false,
            },
            telemetry: unavailable_rate_limit_telemetry(1),
        };
        let mut transport = SequenceTransport { calls: Vec::new() };
        for sequence in 1..=2 {
            let turn = CodexAppServerReadClientV1::start_harmless_continuity_turn(
                &mut transport,
                &preparation,
                sequence,
            )
            .expect("same existing thread");
            assert_eq!(turn.thread_id, "existing-gui-visible-thread");
        }
        assert_eq!(transport.calls.len(), 2);
        for (method, params) in &transport.calls {
            assert_eq!(method, "turn/start");
            assert_eq!(params["threadId"], "existing-gui-visible-thread");
            assert_eq!(params["cwd"], "c:/work/project");
            assert_eq!(params["sandboxPolicy"]["type"], "readOnly");
            assert_eq!(params["sandboxPolicy"]["networkAccess"], false);
            assert!(!params.to_string().to_ascii_lowercase().contains("browser"));
            assert!(!params.to_string().to_ascii_lowercase().contains("wake"));
        }
    }

    #[test]
    fn second_continuity_turn_requires_exact_completed_turn_on_same_thread() {
        let mut transport = FakeTransport {
            replies: VecDeque::from([json!({"turns":[
                {"id":"other-turn","status":"completed"},
                {"id":"turn-one","status":{"type":"completed"}}
            ]})]),
            methods: Vec::new(),
        };
        assert!(
            CodexAppServerReadClientV1::continuity_turn_completed(
                &mut transport,
                "thread-one",
                "turn-one",
            )
            .expect("completed")
        );
        assert_eq!(transport.methods, ["thread/turns/list"]);
        assert!(
            parse_continuity_turn_start(
                &json!({"turn":{"id":"turn-one","threadId":"other-thread","status":"completed"}}),
                "thread-one"
            )
            .is_err()
        );
    }

    #[test]
    #[ignore = "operator-local read-only live acceptance probe"]
    fn live_operator_app_server_candidate_probe() {
        let executable = std::env::var_os("CATDESK_CODEX_CLI_EXECUTABLE")
            .map(PathBuf::from)
            .expect("CatDesk host native Codex executable");
        let cwd = std::env::current_dir().expect("workspace cwd");
        let mut transport = CodexAppServerLaunchConfigV1::new(executable, cwd.clone(), None)
            .expect("host app-server launch config")
            .spawn_stdio_transport()
            .expect("host app-server launch");
        transport.initialize().expect("app-server initialize");

        let listed = transport
            .request(
                "thread/list",
                json!({
                    "sourceKinds": ["exec"],
                    "cwd": cwd.to_string_lossy().to_string(),
                    "limit": 5,
                    "sortKey": "recency_at",
                    "sortDirection": "desc"
                }),
            )
            .expect("thread/list exec/current-cwd");
        let next_cursor_present = listed
            .get("nextCursor")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty());
        let threads = parse_thread_list(&listed).expect("parse thread/list");
        let total = threads.len();
        if let Some(raw_items) = listed
            .get("threads")
            .or_else(|| listed.get("items"))
            .or_else(|| listed.get("data"))
            .and_then(Value::as_array)
        {
            for raw in raw_items.iter().take(5) {
                println!(
                    "LIVE_PROBE raw id={:?} source={:?} status={:?} can_accept={:?} active_writer={:?} model={:?} reasoning={:?}",
                    raw.get("id"),
                    raw.get("source").or_else(|| raw.get("threadSource")),
                    raw.get("status"),
                    raw.get("canAcceptDirectInput"),
                    raw.get("activeWriter"),
                    raw.get("model").or_else(|| raw.get("selectedModel")),
                    raw.get("reasoningEffort")
                );
            }
        }
        for thread in &threads {
            println!(
                "LIVE_PROBE listed id={} cwd={} title={:?} owned={}",
                thread.thread_id, thread.cwd, thread.title, thread.concurrently_owned
            );
        }
        let expected_cwd = normalize_cwd(&cwd.to_string_lossy()).expect("normalize workspace");
        let candidates = threads
            .into_iter()
            .filter(|thread| normalize_cwd(&thread.cwd).ok().as_deref() == Some(&expected_cwd))
            .collect::<Vec<_>>();

        println!(
            "LIVE_PROBE list_count={total} exact_workspace_count={} next_cursor_present={next_cursor_present}",
            candidates.len()
        );
        for candidate in &candidates {
            let preferred = thread_matches(candidate, "integrate codex mcp for chatgpt");
            let detail = transport
                .request("thread/read", json!({"threadId": candidate.thread_id}))
                .expect("thread/read");
            let read_payload = detail.get("thread").unwrap_or(&detail);
            let read = parse_thread_metadata(read_payload).expect("parse thread/read");
            println!(
                "LIVE_PROBE candidate id={} title={:?} preferred={} owned={} list_model={:?} list_reasoning={:?} read_model={:?} read_reasoning={:?}",
                candidate.thread_id,
                candidate.title,
                preferred,
                candidate.concurrently_owned,
                candidate.selected_model,
                candidate.reasoning_effort,
                read.selected_model,
                read.reasoning_effort
            );
        }

        let account = transport
            .request("account/rateLimits/read", json!({}))
            .expect("account/rateLimits/read");
        let telemetry = parse_rate_limit_telemetry(&account, 1).expect("bounded account telemetry");
        println!(
            "LIVE_PROBE account plan={:?} windows={} reached_limit={}",
            telemetry.plan_type,
            telemetry.rate_limit_windows.len(),
            telemetry.reached_limit
        );

        if let Some(candidate) = candidates.first() {
            let resumed = transport
                .request(
                    "thread/resume",
                    json!({
                        "threadId": candidate.thread_id,
                        "excludeTurns": true
                    }),
                )
                .expect("thread/resume newest exec thread");
            let resumed_thread_id = resumed
                .get("thread")
                .and_then(|thread| thread.get("id"))
                .and_then(Value::as_str);
            println!(
                "LIVE_PROBE resume thread_id={:?} model={:?} reasoning={:?} cwd={:?} can_accept={:?} status={:?}",
                resumed_thread_id,
                resumed.get("model"),
                resumed.get("reasoningEffort"),
                resumed.get("cwd"),
                resumed
                    .get("thread")
                    .and_then(|thread| thread.get("canAcceptDirectInput")),
                resumed
                    .get("thread")
                    .and_then(|thread| thread.get("status"))
            );
        }
    }
}
