//! Provider-neutral lifecycle primitives for autonomous workers.
//!
//! This module deliberately sits beside the existing integrated Ollama loop.
//! T-0028B proves the adapter seam without changing a released Qwen run path.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use super::contracts::{TurnId, WorkerSessionId};
use super::runtime::{
    FakeProvider, NormalizedProviderEventKind, NormalizedProviderEventV1, OllamaAdapter,
    ProviderCapabilitiesV1, ProviderClientV1, ProviderHealthStatus, ProviderHealthV1,
    ProviderMessageV1, ProviderSessionV1, ProviderTurnRequestV1, ProviderTurnResultV1,
    ProviderType, RuntimeError,
};

pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, RuntimeError>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderIdV1 {
    Ollama,
    CodexCli,
    Fake,
}

impl ProviderIdV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::CodexCli => "codex-cli",
            Self::Fake => "fake",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "ollama" => Ok(Self::Ollama),
            "codex-cli" => Ok(Self::CodexCli),
            "fake" => Ok(Self::Fake),
            _ => Err(format!("unsupported provider id: {value}")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerProviderTurnRequestV1 {
    pub provider_session: ProviderSessionV1,
    pub turn: ProviderTurnRequestV1,
    pub history: Vec<ProviderMessageV1>,
    pub json_envelope_recovery: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTurnHandleV1 {
    pub provider_id: ProviderIdV1,
    pub handle_id: String,
    pub provider_session_id: String,
    pub worker_session_id: WorkerSessionId,
    pub turn_id: TurnId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEventBatchV1 {
    pub events: Vec<NormalizedProviderEventV1>,
    pub next_cursor: u64,
    pub terminal: bool,
}

pub trait WorkerProviderV1: Send {
    fn provider_id(&self) -> ProviderIdV1;
    fn capabilities(&self) -> ProviderCapabilitiesV1;
    fn create_session(
        &self,
        model_id: &str,
        worker_session_id: &WorkerSessionId,
    ) -> ProviderSessionV1;
    fn start_turn<'a>(
        &'a mut self,
        request: WorkerProviderTurnRequestV1,
    ) -> ProviderFuture<'a, ProviderTurnHandleV1>;
    fn resume_turn<'a>(
        &'a mut self,
        request: WorkerProviderTurnRequestV1,
    ) -> ProviderFuture<'a, ProviderTurnHandleV1>;
    fn poll_events<'a>(
        &'a mut self,
        handle: &'a ProviderTurnHandleV1,
        after_cursor: u64,
    ) -> ProviderFuture<'a, ProviderEventBatchV1>;
    fn cancel<'a>(&'a mut self, handle: &'a ProviderTurnHandleV1) -> ProviderFuture<'a, ()>;
    fn status<'a>(&'a self) -> ProviderFuture<'a, ProviderHealthV1>;
}

pub struct OllamaWorkerProviderV1 {
    adapter: OllamaAdapter,
    keep_alive: Option<String>,
    completed_turns: BTreeMap<String, ProviderTurnResultV1>,
    next_handle_sequence: u64,
}

impl OllamaWorkerProviderV1 {
    pub fn new(base_url: &str, keep_alive: Option<String>) -> Result<Self, RuntimeError> {
        Ok(Self {
            adapter: OllamaAdapter::new(base_url, keep_alive.clone())?,
            keep_alive,
            completed_turns: BTreeMap::new(),
            next_handle_sequence: 1,
        })
    }

    fn next_handle(&mut self, request: &WorkerProviderTurnRequestV1) -> ProviderTurnHandleV1 {
        let handle_id = format!("ollama-turn-{}", self.next_handle_sequence);
        self.next_handle_sequence += 1;
        ProviderTurnHandleV1 {
            provider_id: ProviderIdV1::Ollama,
            handle_id,
            provider_session_id: request.provider_session.provider_session_id.clone(),
            worker_session_id: request.turn.worker_session_id.clone(),
            turn_id: request.turn.turn_id.clone(),
        }
    }

    fn validate_request(&self, request: &WorkerProviderTurnRequestV1) -> Result<(), RuntimeError> {
        if request.provider_session.provider_id != ProviderIdV1::Ollama.as_str() {
            return Err(RuntimeError::Validation(
                "Ollama provider session has a mismatched provider id".into(),
            ));
        }
        if request.provider_session.model_id != request.turn.model_id {
            return Err(RuntimeError::Validation(
                "Ollama provider session model does not match turn model".into(),
            ));
        }
        Ok(())
    }
}

impl WorkerProviderV1 for OllamaWorkerProviderV1 {
    fn provider_id(&self) -> ProviderIdV1 {
        ProviderIdV1::Ollama
    }

    fn capabilities(&self) -> ProviderCapabilitiesV1 {
        ProviderCapabilitiesV1 {
            provider_id: self.provider_id().as_str().into(),
            provider_type: ProviderType::LocalApi,
            supports_streaming: false,
            supports_native_tool_calls: true,
            supports_session_continuity: true,
            supports_cancellation: false,
            context_limit: 128 * 1024,
            output_limit: 64 * 1024,
        }
    }

    fn create_session(
        &self,
        model_id: &str,
        worker_session_id: &WorkerSessionId,
    ) -> ProviderSessionV1 {
        ProviderSessionV1 {
            provider_id: self.provider_id().as_str().into(),
            provider_session_id: format!("ollama-history-{}", worker_session_id.as_str()),
            model_id: model_id.into(),
            keep_alive: self.keep_alive.clone(),
        }
    }

    fn start_turn<'a>(
        &'a mut self,
        request: WorkerProviderTurnRequestV1,
    ) -> ProviderFuture<'a, ProviderTurnHandleV1> {
        Box::pin(async move {
            self.validate_request(&request)?;
            let result = if request.json_envelope_recovery {
                self.adapter
                    .chat_messages_once_accepting_json_envelope(
                        &request.turn.model_id,
                        &request.history,
                        request.turn.turn_id.clone(),
                    )
                    .await?
            } else {
                self.adapter
                    .chat_messages_once(
                        &request.turn.model_id,
                        &request.history,
                        &request.turn.tool_definitions,
                        request.turn.turn_id.clone(),
                    )
                    .await?
            };
            let handle = self.next_handle(&request);
            self.completed_turns
                .insert(handle.handle_id.clone(), result);
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
        after_cursor: u64,
    ) -> ProviderFuture<'a, ProviderEventBatchV1> {
        Box::pin(async move {
            if handle.provider_id != self.provider_id() {
                return Err(RuntimeError::Validation(
                    "provider turn handle belongs to another provider".into(),
                ));
            }
            let result = self.completed_turns.get(&handle.handle_id).ok_or_else(|| {
                RuntimeError::Validation("unknown Ollama provider turn handle".into())
            })?;
            let start = usize::try_from(after_cursor).map_err(|_| {
                RuntimeError::Validation("provider event cursor does not fit usize".into())
            })?;
            let events = result.events.iter().skip(start).cloned().collect();
            Ok(ProviderEventBatchV1 {
                events,
                next_cursor: result.events.len() as u64,
                terminal: result.terminal,
            })
        })
    }

    fn cancel<'a>(&'a mut self, handle: &'a ProviderTurnHandleV1) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            if handle.provider_id != self.provider_id() {
                return Err(RuntimeError::Validation(
                    "provider turn handle belongs to another provider".into(),
                ));
            }
            Err(RuntimeError::Provider(
                "Ollama adapter has no in-flight cancellation handle; cancel at the CatDesk turn boundary"
                    .into(),
            ))
        })
    }

    fn status<'a>(&'a self) -> ProviderFuture<'a, ProviderHealthV1> {
        Box::pin(async move { self.adapter.health().await })
    }
}

pub struct FakeWorkerProviderV1 {
    provider: FakeProvider,
    completed_turns: BTreeMap<String, ProviderTurnResultV1>,
    cancelled_turns: BTreeSet<String>,
    next_handle_sequence: u64,
}

impl FakeWorkerProviderV1 {
    pub fn new(provider: FakeProvider) -> Self {
        Self {
            provider,
            completed_turns: BTreeMap::new(),
            cancelled_turns: BTreeSet::new(),
            next_handle_sequence: 1,
        }
    }

    fn next_handle(&mut self, request: &WorkerProviderTurnRequestV1) -> ProviderTurnHandleV1 {
        let handle_id = format!("fake-turn-{}", self.next_handle_sequence);
        self.next_handle_sequence += 1;
        ProviderTurnHandleV1 {
            provider_id: ProviderIdV1::Fake,
            handle_id,
            provider_session_id: request.provider_session.provider_session_id.clone(),
            worker_session_id: request.turn.worker_session_id.clone(),
            turn_id: request.turn.turn_id.clone(),
        }
    }

    fn validate_request(&self, request: &WorkerProviderTurnRequestV1) -> Result<(), RuntimeError> {
        if request.provider_session.provider_id != ProviderIdV1::Fake.as_str() {
            return Err(RuntimeError::Validation(
                "fake provider session has a mismatched provider id".into(),
            ));
        }
        Ok(())
    }
}

impl WorkerProviderV1 for FakeWorkerProviderV1 {
    fn provider_id(&self) -> ProviderIdV1 {
        ProviderIdV1::Fake
    }

    fn capabilities(&self) -> ProviderCapabilitiesV1 {
        FakeProvider::capabilities()
    }

    fn create_session(
        &self,
        model_id: &str,
        _worker_session_id: &WorkerSessionId,
    ) -> ProviderSessionV1 {
        self.provider.create_session(model_id)
    }

    fn start_turn<'a>(
        &'a mut self,
        request: WorkerProviderTurnRequestV1,
    ) -> ProviderFuture<'a, ProviderTurnHandleV1> {
        Box::pin(async move {
            self.validate_request(&request)?;
            let allowed_tools = request
                .turn
                .tool_definitions
                .iter()
                .map(|tool| tool.name.clone())
                .collect();
            let result =
                self.provider
                    .send_turn(&request.turn, &allowed_tools, &request.history)?;
            let handle = self.next_handle(&request);
            self.completed_turns
                .insert(handle.handle_id.clone(), result);
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
        after_cursor: u64,
    ) -> ProviderFuture<'a, ProviderEventBatchV1> {
        Box::pin(async move {
            if handle.provider_id != self.provider_id() {
                return Err(RuntimeError::Validation(
                    "provider turn handle belongs to another provider".into(),
                ));
            }
            if self.cancelled_turns.contains(&handle.handle_id) {
                return Ok(ProviderEventBatchV1 {
                    events: vec![NormalizedProviderEventV1 {
                        provider_id: self.provider_id().as_str().into(),
                        turn_id: handle.turn_id.clone(),
                        kind: NormalizedProviderEventKind::CancelAck,
                        text: Some("fake provider turn cancelled".into()),
                        tool_call: None,
                    }],
                    next_cursor: after_cursor.saturating_add(1),
                    terminal: true,
                });
            }
            let result = self.completed_turns.get(&handle.handle_id).ok_or_else(|| {
                RuntimeError::Validation("unknown fake provider turn handle".into())
            })?;
            let start = usize::try_from(after_cursor).map_err(|_| {
                RuntimeError::Validation("provider event cursor does not fit usize".into())
            })?;
            let events = result.events.iter().skip(start).cloned().collect();
            Ok(ProviderEventBatchV1 {
                events,
                next_cursor: result.events.len() as u64,
                terminal: result.terminal,
            })
        })
    }

    fn cancel<'a>(&'a mut self, handle: &'a ProviderTurnHandleV1) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            if handle.provider_id != self.provider_id()
                || !self.completed_turns.contains_key(&handle.handle_id)
            {
                return Err(RuntimeError::Validation(
                    "unknown fake provider turn handle".into(),
                ));
            }
            self.cancelled_turns.insert(handle.handle_id.clone());
            Ok(())
        })
    }

    fn status<'a>(&'a self) -> ProviderFuture<'a, ProviderHealthV1> {
        Box::pin(async move {
            Ok(ProviderHealthV1 {
                provider_id: self.provider_id().as_str().into(),
                status: ProviderHealthStatus::Available,
                available_models: vec!["fake-model".into()],
                detail: "deterministic fake provider available".into(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::delegated::contracts::{RunId, TurnId, WorkerSessionId};
    use crate::delegated::journal::ToolMutationKind;
    use crate::delegated::runtime::{FakeProviderTurn, ToolDefinitionV1};

    fn worker_session() -> WorkerSessionId {
        WorkerSessionId::new("worker-provider-test").expect("worker session")
    }

    fn request(provider: &dyn WorkerProviderV1, turn: &str) -> WorkerProviderTurnRequestV1 {
        let worker_session_id = worker_session();
        WorkerProviderTurnRequestV1 {
            provider_session: provider.create_session("fake-model", &worker_session_id),
            turn: ProviderTurnRequestV1 {
                run_id: RunId::new("provider-run").expect("run"),
                worker_session_id,
                turn_id: TurnId::new(turn).expect("turn"),
                model_id: "fake-model".into(),
                context_json: json!({"objective":"provider lifecycle test"}),
                tool_definitions: vec![ToolDefinitionV1 {
                    name: "read".into(),
                    description: "read only".into(),
                    input_schema: json!({"type":"object"}),
                    mutation_kind: ToolMutationKind::ReadOnly,
                }],
                max_output_bytes: 1024,
            },
            history: vec![ProviderMessageV1 {
                role: "user".into(),
                content: "complete the test".into(),
                tool_call_id: None,
                tool_name: None,
            }],
            json_envelope_recovery: false,
        }
    }

    #[test]
    fn provider_ids_are_conservative_and_round_trip() {
        assert_eq!(ProviderIdV1::parse("ollama"), Ok(ProviderIdV1::Ollama));
        assert_eq!(ProviderIdV1::parse("codex-cli"), Ok(ProviderIdV1::CodexCli));
        assert_eq!(ProviderIdV1::parse("fake"), Ok(ProviderIdV1::Fake));
        assert!(ProviderIdV1::parse("browser").is_err());
        let json = serde_json::to_string(&ProviderIdV1::CodexCli).expect("serialize");
        assert_eq!(json, "\"codex-cli\"");
    }

    #[tokio::test]
    async fn fake_provider_lifecycle_polls_with_cursor_and_resumes_same_session() {
        let mut provider = FakeWorkerProviderV1::new(FakeProvider::new(vec![
            FakeProviderTurn::Text("first event".into()),
            FakeProviderTurn::Complete("second event".into()),
        ]));
        let initial = request(&provider, "turn-1");
        let session_id = initial.provider_session.provider_session_id.clone();
        let handle = provider.start_turn(initial).await.expect("start");
        let first = provider.poll_events(&handle, 0).await.expect("first poll");
        assert_eq!(first.events.len(), 1);
        assert_eq!(first.next_cursor, 1);
        assert!(!first.terminal);
        let empty = provider
            .poll_events(&handle, first.next_cursor)
            .await
            .expect("cursor poll");
        assert!(empty.events.is_empty());
        let mut resume = request(&provider, "turn-2");
        resume.provider_session.provider_session_id = session_id.clone();
        let resumed = provider.resume_turn(resume).await.expect("resume");
        assert_eq!(resumed.provider_session_id, session_id);
        let completed = provider
            .poll_events(&resumed, 0)
            .await
            .expect("resume poll");
        assert!(completed.terminal);
        assert_eq!(
            completed.events[0].kind,
            NormalizedProviderEventKind::CompletionClaim
        );
    }

    #[tokio::test]
    async fn fake_provider_cancellation_is_terminal_and_does_not_replay_result() {
        let mut provider =
            FakeWorkerProviderV1::new(FakeProvider::new(vec![FakeProviderTurn::Complete(
                "completed before cancellation test".into(),
            )]));
        let handle = provider
            .start_turn(request(&provider, "turn-cancel"))
            .await
            .expect("start");
        provider.cancel(&handle).await.expect("cancel");
        let batch = provider.poll_events(&handle, 0).await.expect("poll");
        assert!(batch.terminal);
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].kind, NormalizedProviderEventKind::CancelAck);
        assert_ne!(
            batch.events[0].kind,
            NormalizedProviderEventKind::CompletionClaim
        );
    }

    #[test]
    fn ollama_wrapper_preserves_local_only_qwen_compatibility() {
        let provider = OllamaWorkerProviderV1::new("http://127.0.0.1:11434", Some("5m".into()))
            .expect("ollama wrapper");
        let capabilities = provider.capabilities();
        assert_eq!(provider.provider_id(), ProviderIdV1::Ollama);
        assert_eq!(capabilities.provider_id, "ollama");
        assert_eq!(capabilities.provider_type, ProviderType::LocalApi);
        assert!(capabilities.supports_native_tool_calls);
        assert!(capabilities.supports_session_continuity);
        assert!(!capabilities.supports_cancellation);
        let session = provider.create_session("qwen3.6:35b-a3b", &worker_session());
        assert_eq!(session.provider_id, "ollama");
        assert_eq!(session.model_id, "qwen3.6:35b-a3b");
        assert_eq!(session.keep_alive.as_deref(), Some("5m"));
        assert!(session.provider_session_id.starts_with("ollama-history-"));
    }
}
