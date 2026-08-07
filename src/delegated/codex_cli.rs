//! Direct, headless Codex CLI adapter primitives.
//!
//! The adapter only owns a process it launched from an explicit executable
//! path. It neither reads authentication storage nor accepts process paths
//! from MCP. T-0028C keeps it separate from the released delegated worker;
//! later controller work will persist its handles and choose when to invoke it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc::{self, error::TryRecvError};

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
    pub model_id: Option<String>,
    pub sandbox: CodexCliSandboxV1,
    pub max_event_bytes: usize,
    pub max_diagnostic_bytes: usize,
}

impl CodexCliConfigV1 {
    pub fn new(executable: PathBuf, working_directory: PathBuf) -> Result<Self, RuntimeError> {
        let config = Self {
            executable,
            working_directory,
            model_id: None,
            sandbox: CodexCliSandboxV1::WorkspaceWrite,
            max_event_bytes: DEFAULT_MAX_EVENT_BYTES,
            max_diagnostic_bytes: DEFAULT_MAX_DIAGNOSTIC_BYTES,
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
        if self.max_event_bytes == 0 || self.max_diagnostic_bytes == 0 {
            return Err(RuntimeError::Validation(
                "Codex CLI byte limits must be positive".into(),
            ));
        }
        Ok(())
    }
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliEventBatchV1 {
    pub events: Vec<CodexCliEventV1>,
    pub normalized_events: Vec<NormalizedProviderEventV1>,
    pub next_cursor: u64,
    pub thread_id: Option<String>,
    pub state: CodexCliTurnStateV1,
    pub terminal: bool,
}

#[derive(Debug)]
enum StreamMessage {
    Event(CodexCliEventV1),
    ParseError(String),
    Stderr(String),
}

#[derive(Debug)]
struct ActiveCodexTurn {
    child: Child,
    receiver: mpsc::Receiver<StreamMessage>,
    events: Vec<CodexCliEventV1>,
    normalized_events: Vec<NormalizedProviderEventV1>,
    thread_id: Option<String>,
    stderr: String,
    exited: Option<std::process::ExitStatus>,
    cancelled: bool,
    cancel_event_emitted: bool,
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
        let mut command = Command::new(&self.config.executable);
        command.current_dir(&self.config.working_directory);
        command.arg("exec");
        if let Some(thread_id) = resume_thread_id {
            command.arg("resume").arg(thread_id);
        }
        command
            .arg("--json")
            .arg("--ignore-user-config")
            .arg("--color")
            .arg("never");
        if resume_thread_id.is_none() {
            command
                .arg("--sandbox")
                .arg(self.config.sandbox.as_flag_value());
        }
        if let Some(model_id) = &self.config.model_id {
            command.arg("--model").arg(model_id);
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
        let handle = CodexCliTurnHandleV1 {
            handle_id: format!("codex-cli-turn-{}", self.next_handle_sequence),
            worker_session_id: request.worker_session_id,
            turn_id: request.turn_id,
            resumed_thread_id: resume_thread_id.map(ToOwned::to_owned),
        };
        self.next_handle_sequence += 1;
        self.active_turns.insert(
            handle.handle_id.clone(),
            ActiveCodexTurn {
                child,
                receiver,
                events: Vec::new(),
                normalized_events: Vec::new(),
                thread_id: resume_thread_id.map(ToOwned::to_owned),
                stderr: String::new(),
                exited: None,
                cancelled: false,
                cancel_event_emitted: false,
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
        Ok(CodexCliEventBatchV1 {
            events,
            normalized_events,
            next_cursor: active.events.len() as u64,
            thread_id: active.thread_id.clone(),
            terminal: !matches!(state, CodexCliTurnStateV1::Running),
            state,
        })
    }

    pub async fn cancel(&mut self, handle: &CodexCliTurnHandleV1) -> Result<(), RuntimeError> {
        let active = self
            .active_turns
            .get_mut(&handle.handle_id)
            .ok_or_else(|| RuntimeError::Validation("unknown Codex CLI turn handle".into()))?;
        if active.exited.is_some() {
            return Err(RuntimeError::Validation(
                "cannot cancel a completed Codex CLI turn".into(),
            ));
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
        Ok(())
    }

    pub fn captured_thread_id(&self, handle: &CodexCliTurnHandleV1) -> Option<&str> {
        self.active_turns
            .get(&handle.handle_id)
            .and_then(|active| active.thread_id.as_deref())
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
            model_id: self.config.model_id.clone(),
            working_directory: self.config.working_directory.clone(),
            last_completed_turn: active
                .exited
                .is_some_and(|status| status.success())
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
                batch.normalized_events.push(NormalizedProviderEventV1 {
                    provider_id: self.provider_id().as_str().into(),
                    turn_id: handle.turn_id.clone(),
                    kind: NormalizedProviderEventKind::TerminalError,
                    text: Some("Codex CLI turn failed without structured error".into()),
                    tool_call: None,
                });
            }
            Ok(ProviderEventBatchV1 {
                events: batch.normalized_events,
                next_cursor: batch.next_cursor,
                terminal: batch.terminal,
                provider_session_id: batch.thread_id.or(codex_handle.resumed_thread_id),
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
    let history = request
        .history
        .iter()
        .map(|message| serde_json::json!({"role": message.role, "content": message.content}))
        .collect::<Vec<_>>();
    let prompt = serde_json::to_string(&serde_json::json!({
        "protocol": "catdesk.autonomous.v1",
        "contract": request.turn.context_json,
        "messages": history,
        "restrictions": [
            "Do not use CatDesk MCP tools or receive CatDesk tool definitions.",
            "Operate only within the already approved workspace and stop when the task is complete.",
            "Treat verification as CatDesk-controlled and do not claim verified completion."
        ]
    }))
    .map_err(|error| RuntimeError::Validation(format!("failed to serialize Codex autonomous prompt: {error}")))?;
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
    let output = Command::new(&config.executable)
        .args(arguments)
        .current_dir(&config.working_directory)
        .stdin(std::process::Stdio::null())
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
                Ok(Some(_)) => {
                    if sender
                        .send(StreamMessage::ParseError(
                            "Codex JSONL event exceeded configured byte limit".into(),
                        ))
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
            Ok(StreamMessage::Event(event)) => {
                if let Some(thread_id) = &event.thread_id {
                    active.thread_id = Some(thread_id.clone());
                }
                active
                    .normalized_events
                    .push(normalize_codex_event(&event, turn_id));
                active.events.push(event);
            }
            Ok(StreamMessage::ParseError(error)) => {
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
            Ok(StreamMessage::Stderr(line)) => {
                append_bounded(&mut active.stderr, &line, max_diagnostic_bytes)
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
        }
    }
    if active.exited.is_none() {
        active.exited = active.child.try_wait().map_err(|error| {
            RuntimeError::Provider(format!("failed to poll Codex CLI process: {error}"))
        })?;
    }
    Ok(())
}

fn active_turn_state(active: &ActiveCodexTurn) -> CodexCliTurnStateV1 {
    if active.cancelled {
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
        CodexCliTurnStateV1::Running
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
    let bounded_summary = bounded_redacted_text(
        &format!(
            "Codex event type={event_type}; itemType={}; retryAfter={}",
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
    fn shell_shims_and_invalid_directories_are_rejected() {
        let directory = std::env::current_dir().expect("working directory");
        let shim = CodexCliConfigV1 {
            executable: PathBuf::from("C:\\temp\\codex.cmd"),
            working_directory: directory.clone(),
            model_id: None,
            sandbox: CodexCliSandboxV1::ReadOnly,
            max_event_bytes: 1,
            max_diagnostic_bytes: 1,
        };
        assert!(shim.validate().is_err());
        let mut invalid = config();
        invalid.working_directory = PathBuf::from("C:\\missing-catdesk-workspace");
        assert!(invalid.validate().is_err());
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
}
