use serde::{Deserialize, Serialize};

use super::EXECUTION_CONTRACT_SCHEMA_VERSION;
use super::advisor::{AdviceRequestV1, AdviceResponseV1};
use super::contracts::{
    ApprovalId, ArtifactId, EscalationPacketV1, ItemId, RunId, RunState, ToolCallId, TurnId,
    WorkerSessionId, stable_hash,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LifecycleEvent {
    Created,
    Started,
    Delta,
    Completed,
    Failed,
    Cancelled,
    OutcomeUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelopeV1 {
    pub schema_version: u32,
    pub event_sequence: u64,
    pub run_id: RunId,
    pub worker_session_id: Option<WorkerSessionId>,
    pub turn_id: Option<TurnId>,
    pub item_id: Option<ItemId>,
    pub lifecycle_event: LifecycleEvent,
    pub request_hash: String,
    pub result_hash: Option<String>,
    pub payload: EventPayloadV1,
}

impl EventEnvelopeV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != EXECUTION_CONTRACT_SCHEMA_VERSION {
            return Err("unsupported event schema_version".into());
        }
        if self.event_sequence == 0 {
            return Err("event_sequence must start at 1".into());
        }
        if self.request_hash.trim().is_empty() {
            return Err("request_hash must not be empty".into());
        }
        Ok(())
    }

    pub fn with_result_hash(mut self) -> Result<Self, String> {
        self.result_hash = Some(stable_hash(&self.payload)?);
        Ok(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EventPayloadV1 {
    RunStateChanged { state: RunState },
    Delta { text: String },
    Item { item: Box<TurnItemV1> },
    AdviceRequest { request: Box<AdviceRequestV1> },
    AdviceResponse { response: Box<AdviceResponseV1> },
    Failed { error: String },
    Cancelled { reason: String },
    OutcomeUnknown { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TurnItemV1 {
    AgentMessage(AgentMessageItem),
    ToolCall(ToolCallItem),
    ApprovalRequest(ApprovalRequestItem),
    ToolResult(ToolResultItem),
    DiffArtifact(DiffArtifact),
    VerificationArtifact(VerificationArtifact),
    EscalationArtifact(EscalationArtifact),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessageItem {
    pub item_id: ItemId,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallItem {
    pub item_id: ItemId,
    pub tool_call_id: ToolCallId,
    pub tool_name: String,
    pub request_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequestItem {
    pub item_id: ItemId,
    pub approval_id: ApprovalId,
    pub request_hash: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultItem {
    pub item_id: ItemId,
    pub tool_call_id: ToolCallId,
    pub result_hash: String,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffArtifact {
    pub artifact_id: ArtifactId,
    pub path: String,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationArtifact {
    pub artifact_id: ArtifactId,
    pub status: String,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EscalationArtifact {
    pub artifact_id: ArtifactId,
    pub packet: EscalationPacketV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventCursor {
    pub after_sequence: u64,
    pub limit: usize,
}

pub fn validate_append_order(
    last_sequence: Option<u64>,
    event: &EventEnvelopeV1,
) -> Result<(), String> {
    event.validate()?;
    let expected = last_sequence.unwrap_or(0).saturating_add(1);
    if event.event_sequence != expected {
        return Err(format!(
            "out-of-order event_sequence {}; expected {}",
            event.event_sequence, expected
        ));
    }
    Ok(())
}

pub fn poll_events(events: &[EventEnvelopeV1], cursor: EventCursor) -> Vec<EventEnvelopeV1> {
    let mut filtered = events
        .iter()
        .filter(|event| event.event_sequence > cursor.after_sequence)
        .cloned()
        .collect::<Vec<_>>();
    filtered.sort_by_key(|event| event.event_sequence);
    filtered.truncate(cursor.limit);
    filtered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(sequence: u64) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: 1,
            event_sequence: sequence,
            run_id: RunId::new("run-t0013").expect("run id"),
            worker_session_id: None,
            turn_id: None,
            item_id: None,
            lifecycle_event: LifecycleEvent::Created,
            request_hash: "fnv1a64:abc".into(),
            result_hash: None,
            payload: EventPayloadV1::RunStateChanged {
                state: RunState::Draft,
            },
        }
        .with_result_hash()
        .expect("hash")
    }

    #[test]
    fn events_validate_sequence_ordering() {
        validate_append_order(None, &event(1)).expect("first event");
        validate_append_order(Some(1), &event(2)).expect("second event");
        assert!(validate_append_order(Some(2), &event(4)).is_err());
    }

    #[test]
    fn event_polling_is_ordered_and_idempotent() {
        let events = vec![event(3), event(1), event(2)];
        let page = poll_events(
            &events,
            EventCursor {
                after_sequence: 1,
                limit: 10,
            },
        );
        assert_eq!(
            page.iter()
                .map(|event| event.event_sequence)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        let replay = poll_events(
            &events,
            EventCursor {
                after_sequence: 1,
                limit: 10,
            },
        );
        assert_eq!(page, replay);
    }
}
