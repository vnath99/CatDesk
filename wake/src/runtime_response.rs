//! One bounded, read-only response visit. It never resubmits an event.
use super::{Adapter, Status, desired, persist_live_status};
use crate::{
    Result,
    store::{Delivery, Phase, Store},
};
use serde_json::{Value, json};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

fn apply_observation(store: &Store, delivery: &Delivery, value: &Value) -> Result<bool> {
    let complete = validate_observation(delivery, value)?;
    store.record_response_probe(&delivery.event.event_id, &delivery.target, complete)?;
    Ok(complete)
}

pub(crate) fn validate_observation(delivery: &Delivery, value: &Value) -> Result<bool> {
    let receipt = delivery
        .receipt
        .as_ref()
        .ok_or("RESPONSE_PROBE_RECEIPT_REQUIRED")?;
    let stage = value.get("stage").and_then(Value::as_str);
    if delivery.phase != Phase::Sent
        || !matches!(stage, Some("RESPONSE_PENDING" | "RESPONSE_COMPLETE"))
        || !value.as_object().is_some_and(|map| map.len() == 5)
        || value.get("eventId").and_then(Value::as_str) != Some(delivery.event.event_id.as_str())
        || value.get("targetGeneration").and_then(Value::as_u64) != Some(delivery.target.generation)
        || value.get("targetDigest").and_then(Value::as_str)
            != Some(delivery.target.digest.as_str())
        || value.get("messageDigest").and_then(Value::as_str)
            != Some(receipt.message_digest.as_str())
    {
        return Err("RESPONSE_PROBE_SCOPE_INVALID".into());
    }
    Ok(stage == Some("RESPONSE_COMPLETE"))
}

pub(super) fn observe_sent_response(
    store: &Store,
    adapter: &mut Adapter,
    delivery: &Delivery,
    status: &mut Status,
    deadline: Instant,
) -> Result<bool> {
    let value = read_sent_response(store, adapter, delivery, deadline, &mut || {
        persist_live_status(store, status)?;
        if desired(store)? != "RUNNING" {
            return Err("HOST_PAUSED_DURING_RESPONSE_PROBE".into());
        }
        Ok(())
    })?;
    let complete = apply_observation(store, delivery, &value)?;
    status.browser = if complete {
        "RESPONSE_COMPLETED"
    } else {
        "RESPONSE_PENDING"
    }
    .into();
    status.submission = "SENT".into();
    status.login = "READY".into();
    status.attention = None;
    Ok(complete)
}

pub(super) fn read_sent_response(
    store: &Store,
    adapter: &mut Adapter,
    delivery: &Delivery,
    deadline: Instant,
    keep_running: &mut dyn FnMut() -> Result<()>,
) -> Result<Value> {
    keep_running()?;
    if Instant::now() >= deadline {
        return Err("RESPONSE_PROBE_TIMEOUT".into());
    }
    if delivery.phase != Phase::Sent
        || store.config()?.targets.get(&delivery.event.project_id) != Some(&delivery.target)
    {
        return Err("RESPONSE_PROBE_TARGET_CHANGED".into());
    }
    let receipt = delivery
        .receipt
        .as_ref()
        .ok_or("RESPONSE_PROBE_RECEIPT_REQUIRED")?;
    adapter.send(json!({"observeResponse":true,"event":delivery.event,"target":delivery.target,"receipt":receipt}))?;
    loop {
        keep_running()?;
        if Instant::now() >= deadline {
            return Err("RESPONSE_PROBE_TIMEOUT".into());
        }
        match adapter.output.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(value)) => match value.get("stage").and_then(Value::as_str) {
                Some("RESPONSE_PENDING" | "RESPONSE_COMPLETE") => {
                    keep_running()?;
                    return Ok(value);
                }
                Some(
                    "CDP_REUSE_BEGIN" | "CDP_REUSE_DONE" | "CDP_ACTIVATE_BEGIN"
                    | "CDP_ACTIVATE_DONE" | "TARGET_OPEN" | "PAGE_READY",
                ) => {}
                Some("ATTENTION") => {
                    return Err(value
                        .get("reason")
                        .and_then(Value::as_str)
                        .filter(|reason| crate::protocol::valid_id(reason))
                        .unwrap_or("RESPONSE_PROBE_ATTENTION")
                        .into());
                }
                _ => return Err("RESPONSE_PROBE_PROTOCOL_INVALID".into()),
            },
            Ok(Err(reason)) => return Err(reason),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("BROWSER_ADAPTER_EXITED".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::scheduler::tests::{fixture, sent};
    use super::*;
    use crate::store::ResponseState;

    fn observation(delivery: &Delivery, stage: &str) -> Value {
        json!({"stage":stage,"eventId":delivery.event.event_id,
            "targetGeneration":delivery.target.generation,"targetDigest":delivery.target.digest,
            "messageDigest":delivery.receipt.as_ref().unwrap().message_digest})
    }

    #[test]
    fn exact_pending_then_complete_is_durable_without_republishing() {
        let (store, lease) = fixture("probe-complete");
        let delivery = sent(&store, &lease, "alpha-1");
        assert!(
            !apply_observation(
                &store,
                &delivery,
                &observation(&delivery, "RESPONSE_PENDING")
            )
            .unwrap()
        );
        assert_eq!(
            store.timer("alpha-1").unwrap().unwrap().response_state,
            ResponseState::Observing
        );
        assert!(
            apply_observation(
                &store,
                &delivery,
                &observation(&delivery, "RESPONSE_COMPLETE")
            )
            .unwrap()
        );
        assert!(
            apply_observation(
                &store,
                &delivery,
                &observation(&delivery, "RESPONSE_COMPLETE")
            )
            .unwrap()
        );
        assert_eq!(
            apply_observation(
                &store,
                &delivery,
                &observation(&delivery, "RESPONSE_PENDING")
            )
            .unwrap_err(),
            "TIMER_TERMINAL"
        );
        let timer = store.timer("alpha-1").unwrap().unwrap();
        assert_eq!(timer.response_state, ResponseState::Complete);
        assert!(timer.completed_utc.is_some());
        assert_eq!(store.events().unwrap().len(), 5);
        assert_eq!(
            store.delivery("alpha-1").unwrap().unwrap().phase,
            Phase::Sent
        );
        assert!(store.timer("beta-1").unwrap().is_none());
    }

    #[test]
    fn wrong_event_target_generation_message_and_extra_fields_cannot_complete() {
        let (store, lease) = fixture("probe-binding");
        let delivery = sent(&store, &lease, "alpha-1");
        for (field, wrong) in [
            ("eventId", json!("beta-1")),
            ("targetGeneration", json!(2)),
            ("targetDigest", json!("a".repeat(64))),
            ("messageDigest", json!("b".repeat(64))),
            ("extra", json!("untrusted")),
            ("stage", json!("SENT")),
        ] {
            let mut candidate = observation(&delivery, "RESPONSE_COMPLETE");
            candidate[field] = wrong;
            assert_eq!(
                apply_observation(&store, &delivery, &candidate).unwrap_err(),
                "RESPONSE_PROBE_SCOPE_INVALID",
                "{field}"
            );
            assert_eq!(
                store.timer("alpha-1").unwrap().unwrap().response_state,
                ResponseState::Observing
            );
        }
    }

    #[test]
    fn target_change_during_probe_cannot_complete_old_conversation() {
        let (store, lease) = fixture("probe-stale");
        let delivery = sent(&store, &lease, "alpha-1");
        store
            .set_target(
                "alpha",
                delivery.target.generation,
                "https://chatgpt.com/c/6ab06a56-56c0-83e9-9697-9bc7c380c159",
            )
            .unwrap();
        assert_eq!(
            apply_observation(
                &store,
                &delivery,
                &observation(&delivery, "RESPONSE_COMPLETE")
            )
            .unwrap_err(),
            "RESPONSE_PROBE_TARGET_CHANGED"
        );
        assert_eq!(
            store.timer("alpha-1").unwrap().unwrap().response_state,
            ResponseState::Observing
        );
    }
}
