//! Bounded, metadata-only readiness probe shared by the existing profile probe
//! and the guarded development inspector. START precedes every browser import.
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Whitelist metadata at the native boundary, even if a helper prints content.
pub(crate) fn composer_app_observation(line: &str) -> Option<serde_json::Value> {
    let text = line.strip_prefix("PROBE:APP:")?;
    if text.len() > 1024 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let version = value["schemaVersion"].as_u64()?;
    if !matches!(version, 1 | 2)
        || value.as_object()?.len() != if version == 1 { 9 } else { 11 }
        || (version == 2
            && (!matches!(
                value["plusPopup"].as_str()?,
                "UNKNOWN"
                    | "ABSENT"
                    | "MENU"
                    | "DIALOG"
                    | "LISTBOX"
                    | "TREE"
                    | "GRID"
                    | "TRUE"
                    | "FALSE"
            ) || !matches!(
                value["plusControlReference"].as_str()?,
                "UNKNOWN" | "ABSENT" | "PRESENT" | "INVALID"
            )))
        || value["basis"] != "PRESENTATION_ONLY_NOT_TOOL_CATALOG"
        || !matches!(
            value["scope"].as_str()?,
            "UNPROVEN" | "COMPOSER_FORM" | "COMPOSER_CONTAINER"
        )
        || !matches!(
            value["plusButtonType"].as_str()?,
            "UNKNOWN" | "BUTTON" | "SUBMIT"
        )
        || !matches!(
            value["plusButtonState"].as_str()?,
            "UNKNOWN" | "ENABLED" | "DISABLED"
        )
        || !matches!(
            value["plusExpanded"].as_str()?,
            "UNKNOWN" | "OPEN" | "CLOSED"
        )
        || !["editorCount", "plusButtonCount", "v5MarkerCount"]
            .iter()
            .all(|key| value[*key].as_u64().is_some_and(|n| n <= 16))
    {
        return None;
    }
    Some(value)
}

pub(crate) fn app_menu_observation(line: &str) -> Option<serde_json::Value> {
    let text = line.strip_prefix("PROBE:APP_MENU:")?;
    if text.len() > 1536 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let flags = [
        "openAttempted",
        "openRequested",
        "openOutcomeUnknown",
        "closeAttempted",
        "closedVerified",
    ];
    let version = value["schemaVersion"].as_u64()?;
    if !matches!(version, 1..=4)
        || value.as_object()?.len()
            != match version {
                1 => 12,
                2 => 13,
                3 => 17,
                _ => 20,
            }
        || (version >= 2
            && !matches!(
                value["ownershipEvidence"].as_str()?,
                "NONE" | "CONTROL_LINK" | "FRESH_MENU_TRANSITION"
            ))
        || (version >= 3
            && (!matches!(
                value["expandedState"].as_str()?,
                "UNKNOWN" | "OPEN" | "CLOSED"
            ) || !["menuCount", "dialogCount", "otherPopupCount"]
                .iter()
                .all(|key| value[*key].as_u64().is_some_and(|n| n <= 16))))
        || (version == 4
            && (!matches!(
                value["panelKind"].as_str()?,
                "UNPROVEN" | "ROLE_MENU" | "HTML_POPOVER" | "KNOWN_PORTAL" | "INLINE_COMPOSER"
            ) || !["panelCount", "panelActionCount"]
                .iter()
                .all(|key| value[*key].as_u64().is_some_and(|n| n <= 16))))
        || value["basis"] != "TRANSIENT_MENU_ONLY_NOT_TOOL_CATALOG"
        || !matches!(
            value["state"].as_str()?,
            "UNPROVEN"
                | "OPEN_REQUESTED"
                | "CLOSE_REQUESTED"
                | "OBSERVED"
                | "CLOSED"
                | "DRAFT_PRESENT"
                | "GENERATION_ACTIVE"
                | "CONTROL_UNPROVEN"
                | "UNSUPPORTED_MENU"
                | "PRIOR_MENU_OPEN"
                | "MENU_UNPROVEN"
                | "CONTROL_READY"
                | "CONTROL_STATE_UNPROVEN"
                | "MENU_ABSENT"
                | "MENU_AMBIGUOUS"
                | "POPUP_CONFLICT"
                | "CONTROL_LINK_UNPROVEN"
                | "PANEL_AMBIGUOUS"
        )
        || !flags.iter().all(|key| value[*key].as_bool().is_some())
        || ![
            "v5ItemCount",
            "v5DisabledCount",
            "appsItemCount",
            "moreItemCount",
        ]
        .iter()
        .all(|key| value[*key].as_u64().is_some_and(|n| n <= 16))
        || value["v5DisabledCount"].as_u64()? > value["v5ItemCount"].as_u64()?
        || (value["openRequested"] == true && value["openAttempted"] != true)
        || (value["closeAttempted"] == true && value["openAttempted"] != true)
        || (value["state"] == "OBSERVED"
            && !(value["openRequested"] == true
                && value["closeAttempted"] == true
                && value["closedVerified"] == true
                && value["openOutcomeUnknown"] == false
                && (version == 1 || value["ownershipEvidence"] != "NONE")))
        || (version == 3
            && value["state"] == "OBSERVED"
            && (value["menuCount"] != 1
                || value["dialogCount"] != 0
                || value["otherPopupCount"] != 0
                || value["expandedState"] != "OPEN"))
        || (version == 4
            && value["state"] == "OBSERVED"
            && (value["panelCount"] != 1
                || value["dialogCount"] != 0
                || value["otherPopupCount"] != 0
                || value["expandedState"] != "OPEN"
                || if value["panelKind"] == "ROLE_MENU" {
                    value["menuCount"] != 1
                } else {
                    value["ownershipEvidence"] != "FRESH_MENU_TRANSITION"
                        || value["menuCount"] != 0
                        || !matches!(
                            value["panelKind"].as_str()?,
                            "HTML_POPOVER" | "KNOWN_PORTAL" | "INLINE_COMPOSER"
                        )
                        || (value["panelKind"] == "INLINE_COMPOSER"
                            && value["panelActionCount"].as_u64()? == 0)
                }))
    {
        return None;
    }
    Some(value)
}

pub(crate) fn trace_failure(line: &str) -> Option<serde_json::Value> {
    let text = line.strip_prefix("PROBE:TRACE_FAILURE:")?;
    if text.len() > 512 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    if value.as_object()?.len() != 4
        || value["schemaVersion"].as_u64() != Some(1)
        || !matches!(value["phase"].as_str()?, "PREPARE" | "OPEN" | "READ")
        || !matches!(
            value["boundary"].as_str()?,
            "COOPERATIVE_RESPONSE" | "SNAPSHOT" | "DOCUMENT" | "PANEL_STABILITY"
        )
        || !matches!(
            value["reason"].as_str()?,
            "RESPONSE_STATE_UNPROVEN"
                | "RESPONSE_LOAD_ERROR"
                | "RESPONSE_NETWORK_ERROR"
                | "LOGIN_OR_PROFILE_REQUIRED"
                | "RESPONSE_EXPECTED_USER_NOT_FOUND"
                | "RESPONSE_EXPECTED_USER_DUPLICATED"
                | "RESPONSE_SEQUENCE_DRIFT"
                | "RESPONSE_TARGET_DRIFT"
                | "RESPONSE_USER_DOCUMENT_NOT_HYDRATED"
                | "RESPONSE_STATE_QUERY_FAILED"
                | "RESPONSE_COMPLETION_UNPROVEN"
                | "RESPONSE_TIMEOUT_RETRY_REQUIRED"
                | "TRACE_READ_WINDOW_EXPIRED"
                | "TRACE_PROTOCOL_INVALID"
                | "OBSERVER_EXCEPTION"
        )
    {
        return None;
    }
    Some(value)
}

#[test]
fn trace_failure_protocol_retains_only_closed_phase_boundary_and_reason() {
    let value = serde_json::json!({"schemaVersion":1,"phase":"READ","boundary":"PANEL_STABILITY","reason":"TRACE_READ_WINDOW_EXPIRED"});
    assert_eq!(
        trace_failure(&format!("PROBE:TRACE_FAILURE:{value}")),
        Some(value.clone())
    );
    for (field, bad) in [
        ("phase", serde_json::json!("PRIVATE_PHASE")),
        ("boundary", serde_json::json!("PRIVATE_TEXT")),
        ("reason", serde_json::json!("PRIVATE_ERROR")),
        ("rawText", serde_json::json!("PRIVATE_CONTEXT")),
    ] {
        let mut changed = value.clone();
        changed[field] = bad;
        assert!(trace_failure(&format!("PROBE:TRACE_FAILURE:{changed}")).is_none());
        assert!(sanitized_rows(&format!("PROBE:TRACE_FAILURE:{changed}")).is_empty());
    }
}

pub(crate) fn trace_observation(line: &str) -> Option<serde_json::Value> {
    let text = line.strip_prefix("PROBE:TRACE:")?;
    if text.len() > 4096 {
        return None;
    }
    crate::runtime::canonical_trace_observation(serde_json::from_str(text).ok()?)
}

#[test]
fn trace_protocol_preserves_closed_error_and_argument_types_without_values_or_retry_authority() {
    let value = serde_json::json!({"schemaVersion":1,"basis":"TASK_BOUND_TRACE_UI_NOT_NATIVE_RECEIPT","state":"OBSERVED",
        "openAttempted":true,"openRequested":true,"openOutcomeUnknown":false,"controlType":"BUTTON","controlCount":1,
        "expandedState":"OPEN","panelKind":"FRESH_CHILD","panelCount":1,"knownCodes":["WEB_TASK_CACHED_ARGUMENTS_INVALID","INVALID_ARGUMENT"],
        "cachedInvariants":["CWD_OVERRIDE_FORBIDDEN"],"mentionedTools":["RUN_COMMAND"],"requestMatchCount":1,
        "argumentTypes":{"command":"EMPTY_RESERVED_PROBE","cwd":"NULL","timeout":"ABSENT","dry_run":"ABSENT","include_full_output":"ABSENT","allow_without_plan":"ABSENT"},
        "unknownOuterFields":false,"scanTruncated":false});
    assert_eq!(
        trace_observation(&format!("PROBE:TRACE:{value}")),
        Some(value.clone())
    );
    for (field, bad) in [
        ("panelCount", serde_json::json!(2)),
        ("openAttempted", serde_json::json!(false)),
        ("knownCodes", serde_json::json!(["PRIVATE_BODY"])),
        ("rawPanel", serde_json::json!("PRIVATE_VALUE")),
        (
            "argumentTypes",
            serde_json::json!({"command":"PRIVATE_COMMAND"}),
        ),
        ("state", serde_json::json!("UNPROVEN")),
    ] {
        let mut changed = value.clone();
        changed[field] = bad;
        assert!(trace_observation(&format!("PROBE:TRACE:{changed}")).is_none());
        assert!(sanitized_rows(&format!("PROBE:TRACE:{changed}")).is_empty());
    }
    assert_eq!(
        sanitized_rows("PROBE:TRACE_STATE:UNPROVEN\nPROBE:TRACE_STATE:PRIVATE_BODY"),
        ["PROBE:TRACE_STATE:UNPROVEN"]
    );
}

pub(crate) fn surface_observation(line: &str) -> Option<serde_json::Value> {
    let text = line.strip_prefix("PROBE:SURFACE:")?;
    if text.len() > 1024 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let object = value.as_object()?;
    if object.len() != 9
        || !object.iter().all(|(key, value)| {
            let Some(word) = value.as_str() else {
                return false;
            };
            match key.as_str() {
                "route" => matches!(
                    word,
                    "EXACT"
                        | "HOME"
                        | "AUTH_REQUIRED"
                        | "SECURITY_CHECK"
                        | "NETWORK_ERROR"
                        | "TARGET_DRIFT"
                        | "UNKNOWN"
                ),
                "document" => matches!(word, "COMPLETE" | "LOADING" | "UNKNOWN"),
                "history" => matches!(word, "PRESENT" | "ABSENT" | "UNKNOWN"),
                "editor" => matches!(word, "READY" | "MISSING" | "AMBIGUOUS" | "UNKNOWN"),
                "composer" => matches!(word, "EMPTY" | "DRAFT_PRESENT" | "UNKNOWN"),
                "generation" => matches!(word, "ACTIVE" | "IDLE" | "UNKNOWN"),
                "assistant" => matches!(
                    word,
                    "LOAD_ERROR" | "DELIVERY_TIMEOUT" | "PRESENT" | "ABSENT" | "UNKNOWN"
                ),
                "exchange" => word == "UNATTRIBUTED",
                "recovery" => word == "OBSERVATION_ONLY",
                _ => false,
            }
        })
    {
        return None;
    }
    Some(value)
}

/// URL classes and value shapes only. Never pass tab IDs, URLs or page text.
pub(crate) fn tab_observation(line: &str) -> Option<serde_json::Value> {
    let text = line.strip_prefix("PROBE:TAB:")?;
    if text.len() > 1024 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let object = value.as_object()?;
    if object.len() != 12 || value.get("schemaVersion")?.as_u64()? != 1 {
        return None;
    }
    for (key, item) in object {
        let valid = match key.as_str() {
            "schemaVersion" => item.as_u64() == Some(1),
            "tabCount" => item.as_i64().is_some_and(|n| (-1..=32).contains(&n)),
            "metadataMatches" | "unreadableMetadata" => item.as_u64().is_some_and(|n| n <= 32),
            "candidateShape" | "controlledShape" => matches!(
                item.as_str(),
                Some("UNOBSERVED" | "STRING" | "WRAPPED" | "OTHER")
            ),
            "candidateRoute" | "controlledRoute" => matches!(
                item.as_str(),
                Some("UNKNOWN" | "EXACT" | "HOME" | "AUTH" | "NETWORK" | "OTHER" | "INVALID")
            ),
            "candidateFrame" => matches!(item.as_str(), Some("UNKNOWN" | "TOP" | "SUBFRAME")),
            "alternateRoute" => item.is_boolean(),
            "stage" => matches!(
                item.as_str(),
                Some("ENUMERATION" | "METADATA" | "DOCUMENT" | "SWITCH" | "VERIFIED")
            ),
            "state" => matches!(
                item.as_str(),
                Some(
                    "TARGET_TAB_UNAVAILABLE"
                        | "TARGET_TAB_MISSING"
                        | "TARGET_TAB_AMBIGUOUS"
                        | "TARGET_TAB_DRIFT"
                        | "TARGET_TAB_EXACT"
                )
            ),
            _ => false,
        };
        if !valid {
            return None;
        }
    }
    let count = value["tabCount"].as_i64()?;
    let examined = value["metadataMatches"].as_u64()? + value["unreadableMetadata"].as_u64()?;
    if count >= 0 && examined > count as u64 || count < 0 && examined != 0 {
        return None;
    }
    Some(value)
}

pub(crate) fn submission_observation(line: &str) -> Option<serde_json::Value> {
    let text = line.strip_prefix("PROBE:SUBMISSION:")?;
    if text.len() > 2048 {
        return None;
    }
    crate::runtime::canonical_submission_observation(serde_json::from_str(text).ok()?)
}

pub(crate) fn persisted_receipt(line: &str) -> Option<crate::store::Receipt> {
    let text = line.strip_prefix("PROBE:RECEIPT:")?;
    if text.len() > 1024 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    if value.as_object()?.len() != 7 {
        return None;
    }
    let receipt: crate::store::Receipt = serde_json::from_value(value).ok()?;
    let hex = |s: &str| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if receipt.schema_version != 1
        || receipt.target_generation == 0
        || receipt.sent_utc == 0
        || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
        || !receipt
            .event_id
            .strip_prefix("web-work-")
            .or_else(|| receipt.event_id.strip_prefix("wake-intent-"))
            .is_some_and(hex)
        || !hex(&receipt.target_digest)
        || !hex(&receipt.message_digest)
    {
        return None;
    }
    Some(receipt)
}

pub(crate) fn historical_response_observation(line: &str) -> Option<serde_json::Value> {
    let text = line.strip_prefix("PROBE:HISTORY_RESPONSE:")?;
    sent_response_observation(&format!("PROBE:RESPONSE:{text}"))
}

#[test]
fn historical_response_protocol_is_separate_from_current_turn_and_rejects_text() {
    let value = serde_json::json!({"stage":"RESPONSE_COMPLETE","eventId":format!("wake-intent-{}","c".repeat(64)),"targetGeneration":1,
        "targetDigest":"a".repeat(64),"messageDigest":"b".repeat(64),"diagnosticReason":"COMPLETE"});
    let row = format!("PROBE:HISTORY_RESPONSE:{value}");
    assert_eq!(historical_response_observation(&row), Some(value.clone()));
    assert!(sent_response_observation(&row).is_none());
    assert_eq!(sanitized_rows(&row), [row]);
    let mut unsafe_value = value;
    unsafe_value["body"] = serde_json::json!("PRIVATE_CONTEXT");
    assert!(
        historical_response_observation(&format!("PROBE:HISTORY_RESPONSE:{unsafe_value}"))
            .is_none()
    );
}

pub(crate) fn sent_response_observation(line: &str) -> Option<serde_json::Value> {
    let text = line.strip_prefix("PROBE:RESPONSE:")?;
    if text.len() > 2048 {
        return None;
    }
    crate::runtime::canonical_response_observation(serde_json::from_str(text).ok()?)
}

pub(crate) fn task_response_diagnostics(line: &str) -> Option<serde_json::Value> {
    let text = line.strip_prefix("PROBE:TASK:")?;
    if text.len() > 2048 {
        return None;
    }
    crate::runtime::canonical_task_response_diagnostics(serde_json::from_str(text).ok()?)
}

pub fn run_bounded(command: &mut Command, timeout: Duration) -> Result<Vec<String>, &'static str> {
    let mut rows = Vec::new();
    run_bounded_observed(command, timeout, &mut rows)?;
    Ok(rows)
}

/// Retain only whitelisted partial observations after a failed bounded helper.
/// Rows are exposed only after the exact owned Job and output reader exit.
/// A timeout is never a final page state or proof about message submission.
pub fn run_bounded_observed(
    command: &mut Command,
    timeout: Duration,
    rows: &mut Vec<String>,
) -> Result<(), &'static str> {
    rows.clear();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "PROBE_LAUNCH_FAILED")?;
    let job = match crate::process_job::Job::assign(&child) {
        Ok(job) => job,
        Err(_) => {
            drop(child.stdin.take());
            let _ = child.kill();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return Err("PROBE_CONTAINMENT_FAILED"),
                    _ if Instant::now() >= deadline => return Err("PROBE_CLEANUP_UNVERIFIED"),
                    _ => std::thread::sleep(Duration::from_millis(25)),
                }
            }
        }
    };
    // Drain while the helper runs: waiting before reading can fill the Windows
    // pipe and turn a healthy observer into a misleading process timeout.
    // Memory stays bounded even if library output exceeds the protocol budget.
    let output = child.stdout.take().ok_or("PROBE_PIPE_FAILED")?;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut output = output;
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        let result = loop {
            match output.read(&mut buffer) {
                Ok(0) => break Ok(bytes),
                Ok(n) => {
                    let keep = n.min(16_385usize.saturating_sub(bytes.len()));
                    bytes.extend_from_slice(&buffer[..keep]);
                }
                Err(_) => break Err("PROBE_OUTPUT_FAILED"),
            }
        };
        let _ = tx.send(result);
    });
    let result = (|| {
        child
            .stdin
            .take()
            .ok_or("PROBE_PIPE_FAILED")?
            .write_all(b"START\n")
            .map_err(|_| "PROBE_PIPE_FAILED")?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child.try_wait().map_err(|_| "PROBE_WAIT_FAILED")? {
                return if status.success() {
                    Ok(())
                } else {
                    Err("PROBE_FAILED")
                };
            }
            if Instant::now() >= deadline {
                return Err("PROBE_PROCESS_TIMEOUT");
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    })();
    // A normal helper exit is insufficient: retained descendants may still own
    // Chrome/profile resources or the output pipe. Prove the exact Job empty.
    job.terminate_and_wait_empty(Duration::from_secs(5))
        .map_err(|_| "PROBE_CLEANUP_UNVERIFIED")?;
    child.wait().map_err(|_| "PROBE_CLEANUP_UNVERIFIED")?;
    let bytes = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "PROBE_CLEANUP_UNVERIFIED")??;
    if bytes.len() > 16_384 {
        return Err("PROBE_OUTPUT_TOO_LARGE");
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "PROBE_OUTPUT_INVALID")?;
    *rows = sanitized_rows(text);
    result
}

fn sanitized_rows(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| {
            let Some(state) = line.strip_prefix("PROBE:") else {
                return false;
            };
            if state.starts_with("SURFACE:") {
                return surface_observation(line).is_some();
            }
            if state.starts_with("APP:") {
                return composer_app_observation(line).is_some();
            }
            if state.starts_with("APP_MENU:") {
                return app_menu_observation(line).is_some();
            }
            if state.starts_with("APP_MENU_STATE:") {
                return state == "APP_MENU_STATE:UNPROVEN";
            }
            if state.starts_with("TRACE:") {
                return trace_observation(line).is_some();
            }
            if state.starts_with("TRACE_STATE:") {
                return state == "TRACE_STATE:UNPROVEN";
            }
            if state.starts_with("TRACE_FAILURE:") {
                return trace_failure(line).is_some();
            }
            if state.starts_with("APP_STATE:") {
                return state == "APP_STATE:UNPROVEN";
            }
            if state.starts_with("TAB:") {
                return tab_observation(line).is_some();
            }
            if state.starts_with("SUBMISSION:") {
                return submission_observation(line).is_some();
            }
            if state.starts_with("RECEIPT:") {
                return persisted_receipt(line).is_some();
            }
            if state.starts_with("RECEIPT_STATE:") {
                return state == "RECEIPT_STATE:UNPROVEN";
            }
            if state.starts_with("RESPONSE:") {
                return sent_response_observation(line).is_some();
            }
            if state.starts_with("HISTORY_RESPONSE:") {
                return historical_response_observation(line).is_some();
            }
            if state.starts_with("TASK:") {
                return task_response_diagnostics(line).is_some();
            }
            if let Some(reason) = state.strip_prefix("RESPONSE_STATE:") {
                return matches!(
                    reason,
                    "UNPROVEN"
                        | "UNSTABLE"
                        | "RESPONSE_LOAD_ERROR"
                        | "RESPONSE_NETWORK_ERROR"
                        | "LOGIN_OR_PROFILE_REQUIRED"
                        | "RESPONSE_EXPECTED_USER_DUPLICATED"
                        | "RESPONSE_EXPECTED_USER_NOT_FOUND"
                        | "RESPONSE_USER_REPRESENTATION_MISMATCH"
                        | "RESPONSE_SEQUENCE_QUERY_FAILED"
                        | "RESPONSE_SEQUENCE_DRIFT"
                        | "RESPONSE_STATE_UNPROVEN"
                        | "RESPONSE_TIMEOUT_RETRY_REQUIRED"
                        | "RESPONSE_TARGET_DRIFT"
                        | "RESPONSE_USER_DOCUMENT_NOT_HYDRATED"
                );
            }
            if let Some(state) = state.strip_prefix("SUBMISSION_STATE:") {
                return matches!(
                    state,
                    "TARGET_UNAVAILABLE" | "OBSERVATION_UNAVAILABLE" | "OBSERVED"
                );
            }
            if let Some(phase) = state.strip_prefix("PHASE:") {
                return matches!(
                    phase,
                    "IMPORT_BROWSER"
                        | "BROWSER_START"
                        | "CDP_NAVIGATION"
                        | "OBSERVING"
                        | "BROWSER_CLOSED"
                );
            }
            if let Some(reason) = state.strip_prefix("HOME_CHECK:") {
                return matches!(
                    reason,
                    "EMPTY"
                        | "NOT_HOME"
                        | "BUSY"
                        | "DOCUMENT_LOADING"
                        | "EDITOR_MISSING"
                        | "EDITOR_AMBIGUOUS"
                        | "EDITOR_UNSUPPORTED"
                        | "DRAFT_PRESENT"
                        | "EDITOR_MEDIA_PRESENT"
                        | "ATTACHMENT_INDICATOR"
                        | "EVALUATION_UNKNOWN"
                        | "EVALUATION_FAILED"
                        | "NAVIGATION_ATTEMPTED"
                        | "NAVIGATION_FAILED"
                );
            }
            if let Some(code) = state.strip_prefix("NETWORK:") {
                return matches!(
                    code,
                    "UNKNOWN"
                        | "ERR_NETWORK_CHANGED"
                        | "ERR_INTERNET_DISCONNECTED"
                        | "ERR_NAME_NOT_RESOLVED"
                        | "ERR_CONNECTION_CLOSED"
                        | "ERR_CONNECTION_RESET"
                        | "ERR_CONNECTION_ABORTED"
                        | "ERR_CONNECTION_REFUSED"
                        | "ERR_CONNECTION_TIMED_OUT"
                        | "ERR_TIMED_OUT"
                        | "ERR_PROXY_CONNECTION_FAILED"
                        | "ERR_TUNNEL_CONNECTION_FAILED"
                        | "ERR_CERT_DATE_INVALID"
                        | "ERR_CERT_AUTHORITY_INVALID"
                        | "ERR_CERT_COMMON_NAME_INVALID"
                        | "ERR_BLOCKED_BY_CLIENT"
                        | "ERR_BLOCKED_BY_ADMINISTRATOR"
                        | "ERR_BLOCKED_BY_RESPONSE"
                        | "ERR_HTTP_RESPONSE_CODE_FAILURE"
                        | "ERR_TOO_MANY_RETRIES"
                        | "ERR_EMPTY_RESPONSE"
                        | "ERR_FAILED"
                        | "ERR_HTTP2_PROTOCOL_ERROR"
                        | "ERR_SSL_PROTOCOL_ERROR"
                        | "ERR_QUIC_PROTOCOL_ERROR"
                );
            }
            let state = state.strip_prefix("FINAL:").unwrap_or(state);
            matches!(
                state,
                "EXACT_READY_IDLE"
                    | "PROFILE_QUIESCENT"
                    | "PROFILE_HELPER_ACTIVE"
                    | "PROFILE_BROWSER_ACTIVE"
                    | "PROFILE_OWNER_UNATTRIBUTED"
                    | "PROFILE_INVENTORY_UNVERIFIED"
                    | "EXACT_READY_RESPONSE_LOAD_ERROR"
                    | "EXACT_READY_RESPONSE_TIMEOUT"
                    | "EXACT_DRAFT_PRESENT"
                    | "EXACT_DOCUMENT_LOADING"
                    | "EXACT_EDITOR_AMBIGUOUS"
                    | "OBSERVATION_UNAVAILABLE"
                    | "TARGET_TAB_EXACT"
                    | "TARGET_TAB_MISSING"
                    | "TARGET_TAB_AMBIGUOUS"
                    | "TARGET_TAB_DRIFT"
                    | "TARGET_TAB_UNAVAILABLE"
                    | "EXACT_READY_BUSY"
                    | "EXACT_NO_EDITOR"
                    | "LOGIN_VISIBLE"
                    | "CAPTCHA_VISIBLE"
                    | "HOME"
                    | "TARGET_DRIFT"
                    | "CHROME_ERROR"
                    | "URL_UNAVAILABLE"
                    | "URL_INVALID"
            )
        })
        .take(64)
        .map(|line| {
            // Return canonical whitelisted values, never raw JSON containing
            // duplicate-key values or formatting from an untrusted helper.
            if let Some(value) = surface_observation(line) {
                format!("PROBE:SURFACE:{value}")
            } else if let Some(value) = tab_observation(line) {
                format!("PROBE:TAB:{value}")
            } else if let Some(value) = submission_observation(line) {
                format!("PROBE:SUBMISSION:{value}")
            } else {
                line.to_owned()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn response_protocol_reuses_closed_validator_and_rejects_content_or_foreign_shapes() {
        let value = serde_json::json!({"stage":"RESPONSE_COMPLETE","eventId":format!("web-work-{}","a".repeat(64)),"targetGeneration":1,"targetDigest":"a".repeat(64),"messageDigest":"b".repeat(64),"diagnosticReason":"COMPLETE"});
        assert_eq!(
            super::sent_response_observation(&format!("PROBE:RESPONSE:{value}")),
            Some(value.clone())
        );
        for (key, bad) in [
            ("response", serde_json::json!("PRIVATE_BODY")),
            ("diagnosticReason", serde_json::json!("PRIVATE_ERROR")),
            ("stage", serde_json::json!("SENT")),
            ("targetGeneration", serde_json::json!(true)),
            ("messageDigest", serde_json::json!("INVALID")),
        ] {
            let mut v = value.clone();
            v[key] = bad;
            assert!(super::sent_response_observation(&format!("PROBE:RESPONSE:{v}")).is_none());
        }
        assert_eq!(
            super::sanitized_rows(
                "PROBE:RESPONSE_STATE:RESPONSE_LOAD_ERROR\nPROBE:RESPONSE_STATE:PRIVATE_BODY"
            ),
            ["PROBE:RESPONSE_STATE:RESPONSE_LOAD_ERROR"]
        );
    }
    #[test]
    fn receipt_protocol_allows_only_closed_identity_fields_and_bounded_digests() {
        let receipt = serde_json::json!({"schemaVersion":1,"eventId":format!("web-work-{}","a".repeat(64)),
            "targetGeneration":1,"targetDigest":"b".repeat(64),"messageDigest":"c".repeat(64),"sentUtc":1,"evidence":"EXACT_USER_MESSAGE_APPENDED"});
        assert!(super::persisted_receipt(&format!("PROBE:RECEIPT:{receipt}")).is_some());
        let mut continuation = receipt.clone();
        continuation["eventId"] = serde_json::json!(format!("wake-intent-{}", "a".repeat(64)));
        assert!(super::persisted_receipt(&format!("PROBE:RECEIPT:{continuation}")).is_some());
        assert_eq!(
            super::sanitized_rows(&format!("PROBE:RECEIPT:{continuation}")),
            [format!("PROBE:RECEIPT:{continuation}")]
        );
        for id in [
            "wake-intent-private".into(),
            format!("wake-intent-{}", "A".repeat(64)),
            format!("wake-intent-{}", "a".repeat(63)),
            format!("source-review-{}", "a".repeat(64)),
        ] {
            continuation["eventId"] = serde_json::json!(id);
            assert!(super::persisted_receipt(&format!("PROBE:RECEIPT:{continuation}")).is_none());
        }
        for (key, value) in [
            ("schemaVersion", serde_json::json!(true)),
            ("eventId", serde_json::json!("private")),
            ("targetDigest", serde_json::json!("B".repeat(64))),
            ("messageDigest", serde_json::json!("private")),
            ("sentUtc", serde_json::json!(0)),
            ("evidence", serde_json::json!("SUBMISSION_ACCEPTED")),
            ("body", serde_json::json!("PRIVATE_BODY")),
        ] {
            let mut bad = receipt.clone();
            bad[key] = value;
            assert!(super::persisted_receipt(&format!("PROBE:RECEIPT:{bad}")).is_none());
        }
        assert_eq!(
            super::sanitized_rows("PROBE:RECEIPT_STATE:UNPROVEN\nPROBE:RECEIPT_STATE:PRIVATE_BODY"),
            ["PROBE:RECEIPT_STATE:UNPROVEN"]
        );
    }
    use super::*;
    #[test]
    fn submission_rows_share_strict_metadata_validator_without_receipt_authority() {
        let value = serde_json::json!({"composerClass":"OTHER","composerBytes":4,
            "legacyAvailable":false,"readersAgree":null,"legacyMatchesExpected":null,
            "expectedUserCount":2,"lastUserMatches":true,"trustedMatches":2,"turnsAfter":1,
            "stable":true,"coverage":"VISIBLE_BOUNDED_ONLY"});
        assert_eq!(
            submission_observation(&format!("PROBE:SUBMISSION:{value}")),
            Some(value.clone())
        );
        for (key, bad) in [
            ("expectedUserCount", serde_json::json!(513)),
            ("expectedUserCount", serde_json::json!(true)),
            ("composerClass", serde_json::json!("PRIVATE_DRAFT")),
            ("composerText", serde_json::json!("PRIVATE_DRAFT")),
            ("receiptCreated", serde_json::json!(true)),
            ("coverage", serde_json::json!("FULL_DURABLE_HISTORY")),
        ] {
            let mut changed = value.clone();
            changed[key] = bad;
            assert!(submission_observation(&format!("PROBE:SUBMISSION:{changed}")).is_none());
        }
    }
    #[test]
    fn partial_protocol_rejects_private_phases_and_unattributed_submission_claims() {
        assert_eq!(
            sanitized_rows(
                "PROBE:PHASE:CDP_NAVIGATION\nPROBE:PHASE:private\nPROBE:SENT\nPRIVATE_BODY\n"
            ),
            ["PROBE:PHASE:CDP_NAVIGATION"]
        );
    }

    #[cfg(windows)]
    #[test]
    fn timed_out_probe_preserves_only_fixed_partial_phase_after_cleanup() {
        let shell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut command = Command::new(shell);
        command.args(["-NoProfile", "-NonInteractive", "-Command",
            "if([Console]::In.ReadLine() -cne 'START'){exit 2}; [Console]::Out.WriteLine('PROBE:PHASE:CDP_NAVIGATION'); [Console]::Out.WriteLine('PRIVATE_DRAFT'); Start-Sleep -Seconds 30"]);
        let mut rows = Vec::new();
        let start = Instant::now();
        assert_eq!(
            run_bounded_observed(&mut command, Duration::from_secs(2), &mut rows),
            Err("PROBE_PROCESS_TIMEOUT")
        );
        assert_eq!(rows, ["PROBE:PHASE:CDP_NAVIGATION"]);
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    #[cfg(windows)]
    #[test]
    fn over_limit_pipe_is_drained_but_never_exported() {
        let shell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut command = Command::new(shell);
        command.args(["-NoProfile", "-NonInteractive", "-Command",
            "if([Console]::In.ReadLine() -cne 'START'){exit 2}; for($i=0;$i -lt 2000;$i++){[Console]::Out.WriteLine(('x'*150))}; [Console]::Out.WriteLine('PROBE:FINAL:EXACT_READY_IDLE')"]);
        let mut rows = Vec::new();
        assert_eq!(
            run_bounded_observed(&mut command, Duration::from_secs(10), &mut rows),
            Err("PROBE_OUTPUT_TOO_LARGE")
        );
        assert!(rows.is_empty());
    }
    fn surface() -> serde_json::Value {
        serde_json::json!({"route":"EXACT","document":"COMPLETE","history":"PRESENT",
            "editor":"READY","composer":"EMPTY","generation":"IDLE","assistant":"LOAD_ERROR",
            "exchange":"UNATTRIBUTED","recovery":"OBSERVATION_ONLY"})
    }
    fn tab() -> serde_json::Value {
        serde_json::json!({"schemaVersion":1,"tabCount":2,"metadataMatches":1,
            "unreadableMetadata":0,"candidateShape":"STRING","candidateRoute":"HOME",
            "candidateFrame":"TOP","controlledShape":"STRING","controlledRoute":"HOME",
            "alternateRoute":false,"stage":"DOCUMENT","state":"TARGET_TAB_DRIFT"})
    }
    #[test]
    fn tab_protocol_accepts_only_bounded_counts_shapes_and_closed_routes() {
        let value = tab();
        assert_eq!(
            tab_observation(&format!("PROBE:TAB:{value}")),
            Some(value.clone())
        );
        for (key, invalid) in [
            ("tabCount", serde_json::json!(33)),
            ("metadataMatches", serde_json::json!(3)),
            ("metadataMatches", serde_json::json!(true)),
            ("schemaVersion", serde_json::json!(true)),
            (
                "candidateRoute",
                serde_json::json!("https://chatgpt.com/c/private"),
            ),
            ("candidateShape", serde_json::json!("PRIVATE_CONTENT")),
            ("alternateRoute", serde_json::json!("true")),
        ] {
            let mut changed = value.clone();
            changed[key] = invalid;
            assert!(tab_observation(&format!("PROBE:TAB:{changed}")).is_none());
        }
        let mut changed = value;
        changed["privateBody"] = serde_json::json!("PRIVATE_DRAFT");
        assert!(tab_observation(&format!("PROBE:TAB:{changed}")).is_none());
    }
    #[test]
    fn surface_protocol_accepts_only_closed_metadata_vocabulary() {
        let value = surface();
        assert_eq!(
            surface_observation(&format!("PROBE:SURFACE:{value}")),
            Some(value.clone())
        );
        for changed in [
            serde_json::json!({"draft":"PRIVATE_CONTENT"}),
            serde_json::json!([]),
            serde_json::Value::Null,
        ] {
            assert!(surface_observation(&format!("PROBE:SURFACE:{changed}")).is_none());
        }
        for (key, bad) in [
            ("composer", serde_json::json!("PRIVATE_DRAFT")),
            ("route", serde_json::json!("https://chatgpt.com/c/private")),
            ("generation", serde_json::json!(true)),
            ("exchange", serde_json::json!("CURRENT_EXACT_USER")),
        ] {
            let mut changed = value.clone();
            changed[key] = bad;
            assert!(surface_observation(&format!("PROBE:SURFACE:{changed}")).is_none());
        }
        let mut extra = value;
        extra["body"] = serde_json::json!("PRIVATE_ANSWER");
        assert!(surface_observation(&format!("PROBE:SURFACE:{extra}")).is_none());
    }
    #[cfg(windows)]
    #[test]
    fn bounded_probe_accepts_near_limit_output_and_canonicalizes_metadata() {
        let shell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut command = Command::new(shell);
        let row = surface()
            .to_string()
            .replacen('{', "{\"composer\":\"PRIVATE_SENTINEL\",", 1);
        let tab_row = tab()
            .to_string()
            .replacen('{', "{\"state\":\"PRIVATE_TAB_SENTINEL\",", 1);
        let script = format!(
            "if([Console]::In.ReadLine() -cne 'START'){{exit 2}}; for($i=0;$i -lt 90;$i++){{[Console]::Out.WriteLine(('x'*150))}}; [Console]::Out.WriteLine('PROBE:SURFACE:{row}'); [Console]::Out.WriteLine('PROBE:TAB:{tab_row}'); [Console]::Out.WriteLine('PROBE:FINAL:EXACT_READY_RESPONSE_LOAD_ERROR')"
        );
        command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        let rows = run_bounded(&mut command, Duration::from_secs(5)).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(surface_observation(&rows[0]), Some(surface()));
        assert_eq!(tab_observation(&rows[1]), Some(tab()));
        assert_eq!(rows[2], "PROBE:FINAL:EXACT_READY_RESPONSE_LOAD_ERROR");
        assert!(!rows.join(" ").contains("PRIVATE_SENTINEL"));
        assert!(!rows.join(" ").contains("PRIVATE_TAB_SENTINEL"));
    }
}
#[test]
fn task_diagnostics_are_closed_content_free_and_cannot_be_execution_receipts() {
    let value = serde_json::json!({"toolDiagnostics":{"schemaVersion":1,
            "basis":"VISIBLE_UI_MARKERS_ONLY_NOT_EXECUTION_RECEIPT","scope":"EXACT_USER_EXCHANGE",
            "selectorCoverage":"KNOWN_MARKERS_ONLY","callMarkers":0,"resultMarkers":0,
            "errorMarkers":0,"busyMarkers":0,"countOverflow":false},"assistantHints":{"schemaVersion":1,
            "basis":"ASSISTANT_TEXT_HINTS_ONLY_NOT_VERIFIED_EXECUTION","toolUnavailable":true,
            "contextUnavailable":false,"approvalRequested":false,"nativeDispatchMentioned":false,"knownCodes":["HTTP_429"]}});
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{value}")),
        Some(value.clone())
    );
    for (field, replacement) in [
        ("knownCodes", serde_json::json!(["PRIVATE_BODY"])),
        ("knownCodes", serde_json::json!(["HTTP_429", "HTTP_429"])),
        ("toolUnavailable", serde_json::json!(1)),
        ("text", serde_json::json!("PRIVATE_KEY")),
    ] {
        let mut changed = value.clone();
        changed["assistantHints"][field] = replacement;
        let row = format!("PROBE:TASK:{changed}");
        assert!(task_response_diagnostics(&row).is_none());
        assert!(sanitized_rows(&row).is_empty());
    }
    let mut classified = value.clone();
    classified["assistantHints"]["schemaVersion"] = serde_json::json!(2);
    classified["assistantHints"]["scanTruncated"] = serde_json::json!(false);
    classified["assistantHints"]["knownCodes"] =
        serde_json::json!(["WEB_TASK_CACHED_ARGUMENTS_INVALID", "INVALID_ARGUMENT"]);
    classified["assistantHints"]["refusalCategories"] = serde_json::json!(["DISPATCH_REJECTED"]);
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{classified}")),
        Some(classified.clone())
    );
    let mut admission_codes = classified.clone();
    admission_codes["assistantHints"]["knownCodes"] = serde_json::json!([
        "WEB_TASK_ARGUMENTS_INVALID",
        "WEB_TASK_NATIVE_ARGUMENTS_INVALID",
        "WEB_TASK_REQUEST_ID_REQUIRED",
        "WEB_TASK_CENTRAL_MODE_REQUIRED",
        "WEB_TASK_CENTRAL_AUTH_REQUIRED",
        "PLAN_REQUIRED"
    ]);
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{admission_codes}")),
        Some(admission_codes)
    );
    for replacement in [
        serde_json::json!(["PRIVATE_BODY"]),
        serde_json::json!(["DISPATCH_REJECTED", "DISPATCH_REJECTED"]),
        serde_json::json!("DISPATCH_REJECTED"),
    ] {
        let mut invalid = classified.clone();
        invalid["assistantHints"]["refusalCategories"] = replacement;
        assert!(sanitized_rows(&format!("PROBE:TASK:{invalid}")).is_empty());
    }
    let mut changed = value;
    let mut scoped = classified.clone();
    scoped["assistantHints"]["schemaVersion"] = serde_json::json!(3);
    scoped["assistantHints"]["unavailableSubjects"] =
        serde_json::json!(["NATIVE_CALL", "EXECUTION_KEY"]);
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{scoped}")),
        Some(scoped.clone())
    );
    for invalid in [
        serde_json::json!(["PRIVATE_VALUE"]),
        serde_json::json!(["NATIVE_CALL", "NATIVE_CALL"]),
        serde_json::json!(["EXECUTION_KEY", "RUN_COMMAND"]),
        serde_json::json!(true),
    ] {
        let mut bad = scoped.clone();
        bad["assistantHints"]["unavailableSubjects"] = invalid;
        assert!(sanitized_rows(&format!("PROBE:TASK:{bad}")).is_empty());
    }
    let mut expanded = scoped.clone();
    expanded["assistantHints"]["schemaVersion"] = serde_json::json!(4);
    expanded["assistantHints"]["mentionedTools"] =
        serde_json::json!(["RUN_COMMAND", "NATIVE_DISPATCH"]);
    expanded["assistantHints"]["argumentIssues"] = serde_json::json!(["JSON_SYNTAX"]);
    expanded["assistantHints"]["cachedInvariants"] = serde_json::json!(["PAYLOAD_JSON_INVALID"]);
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{expanded}")),
        Some(expanded.clone())
    );
    for field in ["mentionedTools", "argumentIssues", "cachedInvariants"] {
        for invalid in [
            serde_json::json!(["PRIVATE_VALUE"]),
            serde_json::json!(true),
            serde_json::json!(["JSON_SYNTAX", "JSON_SYNTAX"]),
        ] {
            let mut bad = expanded.clone();
            bad["assistantHints"][field] = invalid;
            assert!(sanitized_rows(&format!("PROBE:TASK:{bad}")).is_empty());
        }
    }
    let mut traced = expanded.clone();
    traced["assistantHints"]["trace"] = serde_json::json!({"schemaVersion":1,"scope":"EXACT_USER_EXCHANGE",
        "basis":"PRESENTATION_ONLY_NOT_TOOL_RECEIPT","knownControls":1,"expandedControls":0,
        "collapsedControls":1,"unprovenControls":0,"linkedPanels":1,"visiblePanels":0});
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{traced}")),
        Some(traced.clone())
    );
    for (field, invalid) in [
        ("knownControls", serde_json::json!(true)),
        ("knownControls", serde_json::json!(65)),
        ("knownControls", serde_json::json!(2)),
        ("visiblePanels", serde_json::json!(1)),
        ("rawLabel", serde_json::json!("PRIVATE_TRACE")),
        ("scope", serde_json::json!("OTHER_EXCHANGE")),
    ] {
        let mut bad = traced.clone();
        bad["assistantHints"]["trace"][field] = invalid;
        assert!(sanitized_rows(&format!("PROBE:TASK:{bad}")).is_empty());
    }
    let mut bad = expanded;
    bad["assistantHints"]["argumentValues"] = serde_json::json!("PRIVATE_CONTEXT");
    assert!(sanitized_rows(&format!("PROBE:TASK:{bad}")).is_empty());
    let mut bad = scoped;
    bad["assistantHints"]["rawText"] = serde_json::json!("PRIVATE_BODY");
    assert!(sanitized_rows(&format!("PROBE:TASK:{bad}")).is_empty());
    changed["toolDiagnostics"]["callMarkers"] = serde_json::json!(65);
    assert!(task_response_diagnostics(&format!("PROBE:TASK:{changed}")).is_none());
}

#[test]
#[cfg(test)]
fn composer_app_metadata_is_bounded_presentation_not_catalog_or_action_authority() {
    let value = serde_json::json!({"schemaVersion":1,"scope":"COMPOSER_FORM","editorCount":1,
        "plusButtonCount":1,"plusButtonType":"BUTTON","plusButtonState":"ENABLED","plusExpanded":"CLOSED",
        "v5MarkerCount":0,"basis":"PRESENTATION_ONLY_NOT_TOOL_CATALOG"});
    let line = format!("PROBE:APP:{value}");
    assert_eq!(composer_app_observation(&line), Some(value.clone()));
    assert_eq!(sanitized_rows(&line), [line]);
    let mut container = value.clone();
    container["scope"] = serde_json::json!("COMPOSER_CONTAINER");
    assert_eq!(
        composer_app_observation(&format!("PROBE:APP:{container}")),
        Some(container)
    );
    for (key, invalid) in [
        ("schemaVersion", serde_json::json!(true)),
        ("editorCount", serde_json::json!(17)),
        ("v5MarkerCount", serde_json::json!(true)),
        ("scope", serde_json::json!("AUTHENTICATED")),
        ("plusButtonType", serde_json::json!("PRIVATE_VALUE")),
        ("rawText", serde_json::json!("PRIVATE_DRAFT")),
    ] {
        let mut changed = value.clone();
        changed[key] = invalid;
        assert!(sanitized_rows(&format!("PROBE:APP:{changed}")).is_empty());
    }
    let mut current = value.clone();
    current["schemaVersion"] = serde_json::json!(2);
    current["plusPopup"] = serde_json::json!("DIALOG");
    current["plusControlReference"] = serde_json::json!("PRESENT");
    assert_eq!(
        composer_app_observation(&format!("PROBE:APP:{current}")),
        Some(current.clone())
    );
    for (key, bad) in [
        ("plusPopup", serde_json::json!("PRIVATE_VALUE")),
        ("plusControlReference", serde_json::json!("PRIVATE_ID")),
        ("rawControlId", serde_json::json!("PRIVATE_ID")),
    ] {
        let mut changed = current.clone();
        changed[key] = bad;
        assert!(sanitized_rows(&format!("PROBE:APP:{changed}")).is_empty());
    }
    current.as_object_mut().unwrap().remove("plusPopup");
    assert!(composer_app_observation(&format!("PROBE:APP:{current}")).is_none());
}

#[test]
#[cfg(test)]
fn app_menu_protocol_requires_closed_counts_and_confirmed_close_for_observed_state() {
    let value = serde_json::json!({"schemaVersion":1,"basis":"TRANSIENT_MENU_ONLY_NOT_TOOL_CATALOG","state":"OBSERVED",
        "openAttempted":true,"openRequested":true,"openOutcomeUnknown":false,"closeAttempted":true,"closedVerified":true,
        "v5ItemCount":1,"v5DisabledCount":0,"appsItemCount":1,"moreItemCount":0});
    let line = format!("PROBE:APP_MENU:{value}");
    assert_eq!(app_menu_observation(&line), Some(value.clone()));
    assert_eq!(sanitized_rows(&line), [line]);
    for (key, bad) in [
        ("rawText", serde_json::json!("PRIVATE_KEY")),
        ("v5ItemCount", serde_json::json!(17)),
        ("v5DisabledCount", serde_json::json!(2)),
        ("closedVerified", serde_json::json!(false)),
        ("openAttempted", serde_json::json!(false)),
        ("openOutcomeUnknown", serde_json::json!(true)),
        ("closeAttempted", serde_json::json!(false)),
        ("state", serde_json::json!("PRIVATE_STATE")),
    ] {
        let mut changed = value.clone();
        changed[key] = bad;
        assert!(sanitized_rows(&format!("PROBE:APP_MENU:{changed}")).is_empty());
    }
    assert_eq!(
        sanitized_rows("PROBE:APP_MENU_STATE:UNPROVEN\nPROBE:APP_MENU_STATE:PRIVATE_BODY"),
        ["PROBE:APP_MENU_STATE:UNPROVEN"]
    );
    let mut current = value;
    current["schemaVersion"] = serde_json::json!(2);
    current["ownershipEvidence"] = serde_json::json!("FRESH_MENU_TRANSITION");
    assert_eq!(
        app_menu_observation(&format!("PROBE:APP_MENU:{current}")),
        Some(current.clone())
    );
    for bad in ["NONE", "PRIVATE_VALUE"] {
        let mut changed = current.clone();
        changed["ownershipEvidence"] = serde_json::json!(bad);
        assert!(app_menu_observation(&format!("PROBE:APP_MENU:{changed}")).is_none());
    }
    current.as_object_mut().unwrap().remove("ownershipEvidence");
    assert!(app_menu_observation(&format!("PROBE:APP_MENU:{current}")).is_none());
    current["schemaVersion"] = serde_json::json!(3);
    current["ownershipEvidence"] = serde_json::json!("FRESH_MENU_TRANSITION");
    current["menuCount"] = serde_json::json!(1);
    current["dialogCount"] = serde_json::json!(0);
    current["otherPopupCount"] = serde_json::json!(0);
    current["expandedState"] = serde_json::json!("OPEN");
    assert_eq!(
        app_menu_observation(&format!("PROBE:APP_MENU:{current}")),
        Some(current.clone())
    );
    for (key, bad) in [
        ("menuCount", serde_json::json!(0)),
        ("dialogCount", serde_json::json!(1)),
        ("otherPopupCount", serde_json::json!(17)),
        ("expandedState", serde_json::json!("PRIVATE_VALUE")),
    ] {
        let mut changed = current.clone();
        changed[key] = bad;
        assert!(app_menu_observation(&format!("PROBE:APP_MENU:{changed}")).is_none());
    }
    current["schemaVersion"] = serde_json::json!(4);
    current["menuCount"] = serde_json::json!(0);
    current["panelKind"] = serde_json::json!("INLINE_COMPOSER");
    current["panelCount"] = serde_json::json!(1);
    current["panelActionCount"] = serde_json::json!(1);
    assert_eq!(
        app_menu_observation(&format!("PROBE:APP_MENU:{current}")),
        Some(current.clone())
    );
    for (key, bad) in [
        ("panelCount", serde_json::json!(2)),
        ("panelActionCount", serde_json::json!(0)),
        ("panelKind", serde_json::json!("PRIVATE_LABEL")),
        ("ownershipEvidence", serde_json::json!("CONTROL_LINK")),
        ("rawPanel", serde_json::json!("PRIVATE_CONTEXT")),
    ] {
        let mut changed = current.clone();
        changed[key] = bad;
        assert!(app_menu_observation(&format!("PROBE:APP_MENU:{changed}")).is_none());
    }
}
