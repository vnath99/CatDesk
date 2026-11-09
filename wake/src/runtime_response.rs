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

fn valid_completion_controls(value: &Value) -> bool {
    const COUNTS: &[&str] = &[
        "semanticAssistantUnits",
        "roleAssistantNodes",
        "copyTestIdInTurn",
        "copyLabelInTurn",
        "copyResponseLabelInTurn",
        "copyMessageLabelInTurn",
        "copyTextInTurn",
        "knownCopyInAssistant",
        "knownCopyInBody",
        "sharedCopyAfterAssistant",
        "toolbarContainersInTurn",
        "buttonsInTurn",
        "copyTestIdInParent",
        "copyLabelInParent",
        "copyResponseLabelInParent",
        "copyMessageLabelInParent",
    ];
    const FLAGS: &[&str] = &[
        "pairedTurn",
        "assistantBodyPresent",
        "completionActionFound",
        "assistantInsideSemanticUnit",
    ];
    let button_shape = value.get("buttonShape");
    value.as_object().is_some_and(|map| {
        map.len() == COUNTS.len() + FLAGS.len() + usize::from(button_shape.is_some())
    }) && COUNTS.iter().all(|key| {
        value
            .get(key)
            .and_then(Value::as_u64)
            .is_some_and(|n| n <= 64)
    }) && FLAGS
        .iter()
        .all(|key| value.get(key).and_then(Value::as_bool).is_some())
        && button_shape.is_none_or(valid_button_shape)
}

/// Fixed vocabulary and capped structural metadata only. These observations
/// must never qualify an assistant completion action or grant an interaction.
fn valid_button_shape(value: &Value) -> bool {
    const LABELS: &[&str] = &[
        "ABSENT",
        "OTHER",
        "COPY",
        "COPY_MESSAGE",
        "COPY_RESPONSE",
        "GOOD_RESPONSE",
        "BAD_RESPONSE",
        "READ_ALOUD",
        "MORE",
        "RETRY",
        "SHARE",
        "REGENERATE",
    ];
    let Some(map) = value.as_object() else {
        return false;
    };
    let Some(buttons) = value.get("buttons").and_then(Value::as_array) else {
        return false;
    };
    let Some(overflow) = value.get("countOverflow").and_then(Value::as_bool) else {
        return false;
    };
    let member = |row: &Value, key: &str, allowed: &[&str]| {
        row.get(key)
            .and_then(Value::as_str)
            .is_some_and(|text| allowed.contains(&text))
    };
    map.len() == 3
        && value.get("schemaVersion").and_then(Value::as_u64) == Some(1)
        && buttons.len() <= 8
        && (!overflow || buttons.len() == 8)
        && buttons.iter().all(|row| {
            row.as_object().is_some_and(|map| map.len() == 10)
                && ["aria", "title", "tooltip"]
                    .iter()
                    .all(|key| member(row, key, LABELS))
                && member(row, "testId", &["ABSENT", "OTHER", "COPY"])
                && member(row, "dataState", &["ABSENT", "OTHER", "OPEN", "CLOSED"])
                && member(
                    row,
                    "location",
                    &["USER", "BODY", "ASSISTANT", "SHARED", "AUXILIARY", "OTHER"],
                )
                && member(row, "toolbar", &["ACTION", "ROLE", "NONE"])
                && ["visible", "afterAssistant"]
                    .iter()
                    .all(|key| row.get(key).and_then(Value::as_bool).is_some())
                && row
                    .get("svgCount")
                    .and_then(Value::as_u64)
                    .is_some_and(|n| n <= 4)
        })
}

fn valid_observation_metadata(value: &Value) -> bool {
    let stage = value.get("stage").and_then(Value::as_str);
    let controls_valid = value.get("completionControls").is_none_or(|controls| {
        value.get("diagnosticReason").is_some()
            && valid_completion_controls(controls)
            && (stage != Some("RESPONSE_COMPLETE")
                || (controls
                    .get("assistantBodyPresent")
                    .and_then(Value::as_bool)
                    == Some(true)
                    && controls
                        .get("completionActionFound")
                        .and_then(Value::as_bool)
                        == Some(true)))
    });
    let expected_fields = 5 + usize::from(value.get("completionControls").is_some());
    let diagnostic_valid = match value.get("diagnosticReason") {
        None => value
            .as_object()
            .is_some_and(|map| map.len() == expected_fields),
        Some(reason) => {
            value
                .as_object()
                .is_some_and(|map| map.len() == expected_fields + 1)
                && match stage {
                    Some("RESPONSE_COMPLETE") => reason.as_str() == Some("COMPLETE"),
                    Some("RESPONSE_PENDING") => matches!(
                        reason.as_str(),
                        Some(
                            "DOCUMENT_LOADING"
                                | "GENERATION_ACTIVE"
                                | "EDITOR_NOT_READY"
                                | "ASSISTANT_NOT_MOUNTED"
                                | "ASSISTANT_BODY_UNPROVEN"
                                | "ASSISTANT_COMPLETION_ACTION_UNPROVEN"
                        )
                    ),
                    _ => false,
                }
        }
    };
    matches!(stage, Some("RESPONSE_PENDING" | "RESPONSE_COMPLETE"))
        && diagnostic_valid
        && controls_valid
}

pub(crate) fn canonical_observation(value: Value) -> Option<Value> {
    let hex = |v: &Value| {
        v.as_str().is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    };
    if !valid_observation_metadata(&value)
        || !value["eventId"].as_str().is_some_and(|id| {
            id.strip_prefix("web-work-")
                .or_else(|| id.strip_prefix("wake-intent-"))
                .is_some_and(|v| {
                    v.len() == 64
                        && v.bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
        })
        || !value["targetGeneration"].as_u64().is_some_and(|n| n > 0)
        || !hex(&value["targetDigest"])
        || !hex(&value["messageDigest"])
    {
        return None;
    }
    Some(value)
}

pub(crate) fn canonical_task_diagnostics(value: Value) -> Option<Value> {
    let tools = value.get("toolDiagnostics")?;
    let hints = value.get("assistantHints")?;
    let counts = [
        "callMarkers",
        "resultMarkers",
        "errorMarkers",
        "busyMarkers",
    ];
    let flags = [
        "toolUnavailable",
        "contextUnavailable",
        "approvalRequested",
        "nativeDispatchMentioned",
    ];
    let codes = [
        "HTTP_429",
        "WEB_WORK_ROLLOVER_PENDING",
        "WEB_ROLLOVER_PROJECT_BUSY",
        "WEB_TASK_GRANT_SCOPE_MISMATCH",
        "WEB_TASK_NATIVE_NOT_GRANTED",
        "PROJECT_TASK_CALL_ALREADY_COMPLETED",
        "PROJECT_TASK_RECOVERY_REQUIRED",
        "WEB_TASK_CACHED_ARGUMENTS_INVALID",
        "INVALID_ARGUMENT",
        "UNAUTHORIZED",
        "WEB_TASK_REQUEST_INVALID",
        "WEB_TASK_BINDING_CHANGED",
        "WEB_TASK_ARGUMENTS_INVALID",
        "WEB_TASK_NATIVE_ARGUMENTS_INVALID",
        "WEB_TASK_REQUEST_ID_REQUIRED",
        "WEB_TASK_CENTRAL_MODE_REQUIRED",
        "WEB_TASK_CENTRAL_AUTH_REQUIRED",
        "PLAN_REQUIRED",
    ];
    let version = hints["schemaVersion"].as_u64()?;
    let refusals = [
        "IDENTITY_MISMATCH",
        "TOOL_UNAVAILABLE",
        "CONTEXT_UNAVAILABLE",
        "AUTHORIZATION_REQUIRED",
        "DISPATCH_REJECTED",
    ];
    let valid_refusals = version == 1
        || hints
            .get("refusalCategories")
            .and_then(Value::as_array)
            .is_some_and(|selected| {
                let canonical: Vec<Value> = refusals
                    .iter()
                    .filter(|kind| selected.contains(&json!(kind)))
                    .map(|kind| json!(kind))
                    .collect();
                *selected == canonical
            });
    let subjects = [
        "RUN_COMMAND",
        "CENTRAL_TASK_TOOL",
        "NATIVE_CALL",
        "PRIVATE_CONTEXT",
        "EXECUTION_KEY",
        "CATDESK_APP",
    ];
    let valid_subjects = version < 3
        || hints
            .get("unavailableSubjects")
            .and_then(Value::as_array)
            .is_some_and(|selected| {
                let canonical: Vec<Value> = subjects
                    .iter()
                    .filter(|subject| selected.contains(&json!(subject)))
                    .map(|subject| json!(subject))
                    .collect();
                *selected == canonical
            });
    let categories: &[(&str, &[&str])] = &[
        (
            "mentionedTools",
            &[
                "RUN_COMMAND",
                "CENTRAL_TASK_TOOL",
                "NATIVE_DISPATCH",
                "NATIVE_STATUS",
            ],
        ),
        (
            "argumentIssues",
            &[
                "JSON_SYNTAX",
                "MISSING_REQUIRED_FIELD",
                "UNEXPECTED_FIELD",
                "TYPE_MISMATCH",
                "TOOL_NAME_REJECTED",
                "CACHED_ENVELOPE_REJECTED",
            ],
        ),
        (
            "cachedInvariants",
            &[
                "ARGUMENTS_NOT_OBJECT",
                "COMMAND_NOT_STRING",
                "PLAN_FLAG_NOT_BOOLEAN",
                "DRY_RUN_NOT_FALSE",
                "FULL_OUTPUT_NOT_FALSE",
                "CWD_OVERRIDE_FORBIDDEN",
                "TIMEOUT_OVERRIDE_FORBIDDEN",
                "UNSUPPORTED_OUTER_FIELD",
                "COMMAND_TOO_LARGE",
                "RESERVED_PREFIX_INVALID",
                "PAYLOAD_JSON_INVALID",
                "PAYLOAD_NOT_OBJECT",
            ],
        ),
    ];
    let valid_argument_hints = version != 4
        || categories.iter().all(|(field, allowed)| {
            hints
                .get(*field)
                .and_then(Value::as_array)
                .is_some_and(|selected| {
                    let canonical: Vec<Value> = allowed
                        .iter()
                        .filter(|item| selected.contains(&json!(item)))
                        .map(|item| json!(item))
                        .collect();
                    *selected == canonical
                })
        });
    let trace = hints.get("trace");
    let valid_trace = trace.is_none_or(|trace| {
        let counts = [
            "knownControls",
            "expandedControls",
            "collapsedControls",
            "unprovenControls",
            "linkedPanels",
            "visiblePanels",
        ];
        version == 4
            && trace
                .as_object()
                .is_some_and(|map| map.len() == counts.len() + 3)
            && trace["schemaVersion"].as_u64() == Some(1)
            && trace["scope"] == "EXACT_USER_EXCHANGE"
            && trace["basis"] == "PRESENTATION_ONLY_NOT_TOOL_RECEIPT"
            && counts
                .iter()
                .all(|field| trace[*field].as_u64().is_some_and(|n| n <= 64))
            && trace["expandedControls"].as_u64().unwrap_or(0)
                + trace["collapsedControls"].as_u64().unwrap_or(0)
                + trace["unprovenControls"].as_u64().unwrap_or(0)
                == trace["knownControls"].as_u64().unwrap_or(0)
            && trace["visiblePanels"].as_u64() <= trace["linkedPanels"].as_u64()
            && trace["visiblePanels"].as_u64() <= trace["expandedControls"].as_u64()
            && trace["linkedPanels"].as_u64() <= trace["knownControls"].as_u64()
    });
    let selected = hints.get("knownCodes")?.as_array()?;
    let canonical: Vec<Value> = codes
        .iter()
        .filter(|code| selected.contains(&json!(code)))
        .map(|code| json!(code))
        .collect();
    if value.as_object()?.len() != 2
        || tools.as_object()?.len() != 9
        || hints.as_object()?.len()
            != if version == 4 {
                13 + usize::from(trace.is_some())
            } else if version == 3 {
                10
            } else if version == 2 {
                9
            } else {
                7
            }
        || (version >= 2 && hints["scanTruncated"].as_bool().is_none())
        || tools["schemaVersion"].as_u64() != Some(1)
        || tools["basis"] != "VISIBLE_UI_MARKERS_ONLY_NOT_EXECUTION_RECEIPT"
        || tools["scope"] != "EXACT_USER_EXCHANGE"
        || tools["selectorCoverage"] != "KNOWN_MARKERS_ONLY"
        || tools["countOverflow"].as_bool().is_none()
        || !counts
            .iter()
            .all(|field| tools[*field].as_u64().is_some_and(|n| n <= 64))
        || !matches!(version, 1..=4)
        || hints["basis"] != "ASSISTANT_TEXT_HINTS_ONLY_NOT_VERIFIED_EXECUTION"
        || !flags.iter().all(|field| hints[*field].as_bool().is_some())
        || *selected != canonical
        || !valid_refusals
        || !valid_subjects
        || !valid_argument_hints
        || !valid_trace
    {
        return None;
    }
    Some(value)
}

pub(crate) fn canonical_trace_observation(value: Value) -> Option<Value> {
    let flags = [
        "openAttempted",
        "openRequested",
        "openOutcomeUnknown",
        "unknownOuterFields",
        "scanTruncated",
    ];
    let counts = ["controlCount", "panelCount", "requestMatchCount"];
    if value.as_object()?.len() != 18
        || value["schemaVersion"].as_u64() != Some(1)
        || value["basis"] != "TASK_BOUND_TRACE_UI_NOT_NATIVE_RECEIPT"
        || !matches!(
            value["state"].as_str()?,
            "UNPROVEN"
                | "CONTROL_READY"
                | "CONTROL_ABSENT"
                | "CONTROL_AMBIGUOUS"
                | "CONTROL_UNSUPPORTED"
                | "DRAFT_PRESENT"
                | "GENERATION_ACTIVE"
                | "PANEL_CONFLICT"
                | "OPEN_REQUESTED"
                | "OPEN_OUTCOME_UNKNOWN"
                | "CONTROL_CHANGED"
                | "PANEL_UNPROVEN"
                | "PANEL_AMBIGUOUS"
                | "OBSERVED"
                | "RESPONSE_CHANGED"
        )
        || !matches!(
            value["controlType"].as_str()?,
            "UNKNOWN" | "BUTTON" | "SUMMARY"
        )
        || !matches!(
            value["expandedState"].as_str()?,
            "UNKNOWN" | "OPEN" | "CLOSED"
        )
        || !matches!(
            value["panelKind"].as_str()?,
            "NONE" | "CONTROL_LINK" | "DETAILS" | "FRESH_CHILD" | "FRESH_ROLE"
        )
        || !flags.iter().all(|key| value[*key].as_bool().is_some())
        || !counts
            .iter()
            .all(|key| value[*key].as_u64().is_some_and(|n| n <= 64))
    {
        return None;
    }
    // Reuse the same closed error/tool vocabulary; these neutral flags are
    // validation input only and are never returned as execution evidence.
    canonical_task_diagnostics(
        json!({"toolDiagnostics":{"schemaVersion":1,"basis":"VISIBLE_UI_MARKERS_ONLY_NOT_EXECUTION_RECEIPT",
        "scope":"EXACT_USER_EXCHANGE","selectorCoverage":"KNOWN_MARKERS_ONLY","callMarkers":0,"resultMarkers":0,"errorMarkers":0,"busyMarkers":0,"countOverflow":false},
        "assistantHints":{"schemaVersion":4,"basis":"ASSISTANT_TEXT_HINTS_ONLY_NOT_VERIFIED_EXECUTION","toolUnavailable":false,"contextUnavailable":false,"approvalRequested":false,"nativeDispatchMentioned":false,
        "knownCodes":value["knownCodes"],"refusalCategories":[],"scanTruncated":false,"unavailableSubjects":[],"mentionedTools":value["mentionedTools"],"argumentIssues":[],"cachedInvariants":value["cachedInvariants"]}}),
    )?;
    let arguments = &value["argumentTypes"];
    if !arguments.is_null()
        && (arguments.as_object()?.len() != 6
            || arguments["command"] != "EMPTY_RESERVED_PROBE"
            || ![
                "cwd",
                "timeout",
                "dry_run",
                "include_full_output",
                "allow_without_plan",
            ]
            .iter()
            .all(|key| {
                matches!(
                    arguments[*key].as_str(),
                    Some(
                        "ABSENT"
                            | "NULL"
                            | "BOOLEAN"
                            | "STRING"
                            | "NUMBER"
                            | "OBJECT"
                            | "ARRAY"
                            | "OTHER"
                    )
                )
            }))
    {
        return None;
    }
    if value["openRequested"] == true && value["openAttempted"] != true {
        return None;
    }
    if value["state"] == "OBSERVED" {
        if value["openRequested"] != true
            || value["openOutcomeUnknown"] != false
            || value["expandedState"] != "OPEN"
            || value["controlCount"] != 1
            || value["panelCount"] != 1
            || value["panelKind"] == "NONE"
            || (!arguments.is_null()) != (value["requestMatchCount"] == 1)
        {
            return None;
        }
    } else if !value["knownCodes"].as_array()?.is_empty()
        || !value["cachedInvariants"].as_array()?.is_empty()
        || !value["mentionedTools"].as_array()?.is_empty()
        || !arguments.is_null()
        || value["unknownOuterFields"] != false
        || value["requestMatchCount"] != 0
    {
        return None;
    }
    Some(value)
}

pub(crate) fn validate_observation(delivery: &Delivery, value: &Value) -> Result<bool> {
    let receipt = delivery
        .receipt
        .as_ref()
        .ok_or("RESPONSE_PROBE_RECEIPT_REQUIRED")?;
    let stage = value.get("stage").and_then(Value::as_str);
    if delivery.phase != Phase::Sent
        || !valid_observation_metadata(value)
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
    fn diagnostic_is_closed_stage_consistent_and_never_completes_pending() {
        let (store, lease) = fixture("probe-diagnostic");
        let delivery = sent(&store, &lease, "alpha-1");
        for reason in [
            "DOCUMENT_LOADING",
            "GENERATION_ACTIVE",
            "EDITOR_NOT_READY",
            "ASSISTANT_NOT_MOUNTED",
            "ASSISTANT_BODY_UNPROVEN",
            "ASSISTANT_COMPLETION_ACTION_UNPROVEN",
        ] {
            let mut value = observation(&delivery, "RESPONSE_PENDING");
            value["diagnosticReason"] = json!(reason);
            assert!(!apply_observation(&store, &delivery, &value).unwrap());
            value["stage"] = json!("RESPONSE_COMPLETE");
            assert_eq!(
                apply_observation(&store, &delivery, &value).unwrap_err(),
                "RESPONSE_PROBE_SCOPE_INVALID"
            );
        }
        for reason in [
            json!("PRIVATE_PAGE_TEXT"),
            json!("COMPLETE"),
            json!(null),
            json!({"text":"private"}),
        ] {
            let mut value = observation(&delivery, "RESPONSE_PENDING");
            value["diagnosticReason"] = reason;
            assert_eq!(
                apply_observation(&store, &delivery, &value).unwrap_err(),
                "RESPONSE_PROBE_SCOPE_INVALID"
            );
        }
        assert_eq!(
            store.timer("alpha-1").unwrap().unwrap().response_state,
            ResponseState::Observing
        );
        let mut complete = observation(&delivery, "RESPONSE_COMPLETE");
        complete["diagnosticReason"] = json!("COMPLETE");
        assert!(apply_observation(&store, &delivery, &complete).unwrap());
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

    #[test]
    fn completion_control_metadata_is_closed_capped_scoped_and_not_completion_authority() {
        let (store, lease) = fixture("probe-control-shape");
        let delivery = sent(&store, &lease, "alpha-1");
        let controls = json!({"semanticAssistantUnits":1,"roleAssistantNodes":0,
            "copyTestIdInTurn":0,"copyLabelInTurn":1,"copyResponseLabelInTurn":0,
            "copyMessageLabelInTurn":0,"copyTextInTurn":1,"knownCopyInAssistant":0,
            "knownCopyInBody":0,"sharedCopyAfterAssistant":0,"toolbarContainersInTurn":0,
            "buttonsInTurn":1,"copyTestIdInParent":1,"copyLabelInParent":1,
            "copyResponseLabelInParent":0,"copyMessageLabelInParent":0,
            "pairedTurn":true,"assistantBodyPresent":true,"completionActionFound":false,
            "assistantInsideSemanticUnit":true});
        let mut value = observation(&delivery, "RESPONSE_PENDING");
        value["diagnosticReason"] = json!("ASSISTANT_COMPLETION_ACTION_UNPROVEN");
        value["completionControls"] = controls.clone();
        assert!(!apply_observation(&store, &delivery, &value).unwrap());
        let button = json!({"aria":"OTHER","title":"ABSENT","tooltip":"COPY",
            "testId":"ABSENT","dataState":"CLOSED","location":"SHARED",
            "toolbar":"ACTION","visible":true,"afterAssistant":true,"svgCount":1});
        let shape = json!({"schemaVersion":1,"countOverflow":false,"buttons":[button.clone()]});
        let mut shaped = value.clone();
        shaped["completionControls"]["buttonShape"] = shape.clone();
        assert!(!apply_observation(&store, &delivery, &shaped).unwrap());
        // Fixed copy-looking structural metadata cannot turn an unproved
        // assistant completion action into a completed response.
        shaped["stage"] = json!("RESPONSE_COMPLETE");
        shaped["diagnosticReason"] = json!("COMPLETE");
        assert_eq!(
            apply_observation(&store, &delivery, &shaped).unwrap_err(),
            "RESPONSE_PROBE_SCOPE_INVALID"
        );
        for invalid_shape in [
            Value::Null,
            json!({}),
            json!({"schemaVersion":true,"countOverflow":false,"buttons":[]}),
            json!({"schemaVersion":2,"countOverflow":false,"buttons":[]}),
            json!({"schemaVersion":1,"countOverflow":false,"buttons":[],"private":"draft"}),
            json!({"schemaVersion":1,"countOverflow":true,"buttons":[button.clone()]}),
            json!({"schemaVersion":1,"countOverflow":false,"buttons":vec![button.clone();9]}),
        ] {
            let mut invalid = value.clone();
            invalid["completionControls"]["buttonShape"] = invalid_shape;
            assert_eq!(
                apply_observation(&store, &delivery, &invalid).unwrap_err(),
                "RESPONSE_PROBE_SCOPE_INVALID"
            );
        }
        for (key, bad) in [
            ("aria", json!("PRIVATE_LABEL")),
            ("title", json!("PRIVATE_TITLE")),
            ("tooltip", json!("PRIVATE_TOOLTIP")),
            ("testId", json!("PRIVATE_ID")),
            ("dataState", json!("expanded")),
            ("location", json!("PRIVATE_PATH")),
            ("toolbar", json!("PRIVATE_CLASS")),
            ("visible", json!(1)),
            ("afterAssistant", Value::Null),
            ("svgCount", json!(5)),
            ("svgCount", json!(true)),
            ("svgCount", json!(-1)),
            ("extra", json!("PRIVATE_DRAFT")),
        ] {
            let mut invalid = value.clone();
            invalid["completionControls"]["buttonShape"] = shape.clone();
            invalid["completionControls"]["buttonShape"]["buttons"][0][key] = bad;
            assert_eq!(
                apply_observation(&store, &delivery, &invalid).unwrap_err(),
                "RESPONSE_PROBE_SCOPE_INVALID"
            );
        }
        let mut capped = value.clone();
        capped["completionControls"]["buttonShape"] = json!({"schemaVersion":1,
            "countOverflow":true,"buttons":vec![button;8]});
        assert!(!apply_observation(&store, &delivery, &capped).unwrap());
        for (field, bad) in [
            ("copyLabelInTurn", json!(65)),
            ("copyLabelInTurn", json!(-1)),
            ("copyLabelInTurn", json!(true)),
            ("copyLabelInTurn", json!("private text")),
            ("pairedTurn", json!(1)),
            ("extra", json!("private draft")),
        ] {
            let mut invalid = value.clone();
            invalid["completionControls"][field] = bad;
            assert_eq!(
                apply_observation(&store, &delivery, &invalid).unwrap_err(),
                "RESPONSE_PROBE_SCOPE_INVALID"
            );
        }
        let mut no_reason = value.clone();
        no_reason
            .as_object_mut()
            .unwrap()
            .remove("diagnosticReason");
        assert_eq!(
            apply_observation(&store, &delivery, &no_reason).unwrap_err(),
            "RESPONSE_PROBE_SCOPE_INVALID"
        );
        value["stage"] = json!("RESPONSE_COMPLETE");
        value["diagnosticReason"] = json!("COMPLETE");
        assert_eq!(
            apply_observation(&store, &delivery, &value).unwrap_err(),
            "RESPONSE_PROBE_SCOPE_INVALID"
        );
        assert_eq!(
            store.timer("alpha-1").unwrap().unwrap().response_state,
            ResponseState::Observing
        );
    }
}
