//! Bounded, metadata-only readiness probe shared by the existing profile probe
//! and the guarded development inspector. START precedes every browser import.
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Whitelist metadata at the native boundary, even if a helper prints content.
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

pub(crate) fn historical_response_observation(line: &str) -> Option<serde_json::Value> {
    let text = line.strip_prefix("PROBE:HISTORY_RESPONSE:")?;
    sent_response_observation(&format!("PROBE:RESPONSE:{text}"))
}

pub(crate) fn sent_response_observation(line: &str) -> Option<serde_json::Value> {
    let text = line.strip_prefix("PROBE:RESPONSE:")?;
    // The optional closed eight-button shape can exceed the legacy 2 KiB.
    // Only this response frame grows; its metadata validator stays mandatory.
    if text.len() > 4096 {
        return None;
    }
    crate::runtime::canonical_response_observation(serde_json::from_str(text).ok()?)
}

pub(crate) fn task_response_diagnostics(line: &str) -> Option<serde_json::Value> {
    let text = line.strip_prefix("PROBE:TASK:")?;
    if text.len() > 4096 {
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
    run_bounded_observed_input(command, timeout, rows, b"START\n")
}

/// The program is compiled into the reviewed controller, not accepted from MCP
/// or a caller-selected file. Only a short loader crosses the command line.
pub(crate) fn embedded_program_loader(program: &[u8]) -> Result<String, &'static str> {
    use sha2::{Digest, Sha256};
    if program.is_empty() || program.len() > 64 * 1024 {
        return Err("PROBE_PROGRAM_SIZE_INVALID");
    }
    let hash = format!("{:x}", Sha256::digest(program));
    Ok(format!(
        "import hashlib,sys;p=sys.stdin.buffer.read({});assert len(p)=={} and hashlib.sha256(p).hexdigest()=='{}';exec(compile(p,'<CatDeskWakeReadinessProbe>','exec'),{{'__name__':'__main__'}})",
        program.len(),
        program.len(),
        hash
    ))
}

pub(crate) fn run_bounded_observed_program(
    command: &mut Command,
    timeout: Duration,
    rows: &mut Vec<String>,
    program: &[u8],
) -> Result<(), &'static str> {
    rows.clear();
    embedded_program_loader(program)?;
    let mut input = Vec::with_capacity(program.len() + 6);
    input.extend_from_slice(program);
    input.extend_from_slice(b"START\n");
    run_bounded_observed_input(command, timeout, rows, &input)
}

fn run_bounded_observed_input(
    command: &mut Command,
    timeout: Duration,
    rows: &mut Vec<String>,
    input: &[u8],
) -> Result<(), &'static str> {
    rows.clear();
    if input.len() > 64 * 1024 + 6 {
        return Err("PROBE_PROGRAM_SIZE_INVALID");
    }
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
    // A blocked stdin write is part of the same deadline. Drain it only after
    // the existing Job proves all readers exited; never abandon a writer.
    let deadline = Instant::now() + timeout;
    let mut stdin = child.stdin.take().ok_or("PROBE_PIPE_FAILED")?;
    let input = input.to_vec();
    let (input_tx, input_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = stdin.write_all(&input).map_err(|_| "PROBE_PIPE_FAILED");
        drop(stdin);
        let _ = input_tx.send(result);
    });
    let result = (|| {
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
    let input_result = input_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "PROBE_CLEANUP_UNVERIFIED")?;
    let bytes = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "PROBE_CLEANUP_UNVERIFIED")??;
    if bytes.len() > 16_384 {
        return Err("PROBE_OUTPUT_TOO_LARGE");
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "PROBE_OUTPUT_INVALID")?;
    *rows = sanitized_rows(text);
    result.and(input_result)
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
            if state.starts_with("TAB:") {
                return tab_observation(line).is_some();
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
            } else if let Some(value) = sent_response_observation(line) {
                format!("PROBE:RESPONSE:{value}")
            } else if let Some(value) = historical_response_observation(line) {
                format!("PROBE:HISTORY_RESPONSE:{value}")
            } else if let Some(value) = task_response_diagnostics(line) {
                format!("PROBE:TASK:{value}")
            } else {
                line.to_owned()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
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

#[cfg(test)]
fn identity_hint_fixture() -> serde_json::Value {
    serde_json::json!({"toolDiagnostics":{"schemaVersion":1,
        "basis":"VISIBLE_UI_MARKERS_ONLY_NOT_EXECUTION_RECEIPT","scope":"EXACT_USER_EXCHANGE",
        "selectorCoverage":"KNOWN_MARKERS_ONLY","callMarkers":0,"resultMarkers":0,
        "errorMarkers":0,"busyMarkers":0,"countOverflow":false},
        "assistantHints":{"schemaVersion":5,"basis":"ASSISTANT_TEXT_HINTS_ONLY_NOT_VERIFIED_EXECUTION",
            "toolUnavailable":false,"contextUnavailable":false,"approvalRequested":false,
            "nativeDispatchMentioned":false,"knownCodes":[],"refusalCategories":["DISPATCH_REJECTED"],
            "scanTruncated":false,"unavailableSubjects":[],"mentionedTools":[],"argumentIssues":[],
            "cachedInvariants":[],"identityIssues":["FULL_PRIVATE_CONTEXT_UNVERIFIED"],
            "trace":{"schemaVersion":1,"scope":"EXACT_USER_EXCHANGE",
                "basis":"PRESENTATION_ONLY_NOT_TOOL_RECEIPT","knownControls":1,"expandedControls":0,
                "collapsedControls":1,"unprovenControls":0,"linkedPanels":0,"visiblePanels":0}}})
}

#[test]
fn body_coverage_is_closed_advisory_metadata_and_never_completion_authority() {
    let mut controls = serde_json::json!({"pairedTurn":true,"assistantBodyPresent":true,
        "completionActionFound":true,"assistantInsideSemanticUnit":true,
        "bodyCoverage":{"selectedBlocks":1,"canonicalBody":"QUALIFIED","unselectedTextPresent":false}});
    for name in [
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
    ] {
        controls[name] = serde_json::json!(0);
    }
    let value = serde_json::json!({"eventId":format!("wake-intent-{}","a".repeat(64)),
        "targetGeneration":1,"targetDigest":"b".repeat(64),"messageDigest":"c".repeat(64),
        "stage":"RESPONSE_COMPLETE","diagnosticReason":"COMPLETE","completionControls":controls});
    assert_eq!(
        sent_response_observation(&format!("PROBE:RESPONSE:{value}")),
        Some(value.clone())
    );
    for (field, invalid) in [
        ("selectedBlocks", serde_json::json!(true)),
        ("selectedBlocks", serde_json::json!(0)),
        ("selectedBlocks", serde_json::json!(65)),
        ("canonicalBody", serde_json::json!("PRIVATE_BODY")),
        ("unselectedTextPresent", serde_json::json!("PRIVATE_VALUE")),
        ("rawText", serde_json::json!("PRIVATE_RESPONSE")),
    ] {
        let mut bad = value.clone();
        bad["completionControls"]["bodyCoverage"][field] = invalid;
        assert!(sent_response_observation(&format!("PROBE:RESPONSE:{bad}")).is_none());
    }
    let mut unproven = value;
    unproven["completionControls"]["completionActionFound"] = serde_json::json!(false);
    assert!(sent_response_observation(&format!("PROBE:RESPONSE:{unproven}")).is_none());
}

#[test]
fn identity_hints_accept_only_fixed_categories_and_preserve_old_wire_versions() {
    let value = identity_hint_fixture();
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{value}")),
        Some(value.clone())
    );
    let mut old = value.clone();
    old["assistantHints"]["schemaVersion"] = serde_json::json!(4);
    assert!(task_response_diagnostics(&format!("PROBE:TASK:{old}")).is_none());
    old["assistantHints"]
        .as_object_mut()
        .unwrap()
        .remove("identityIssues");
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{old}")),
        Some(old)
    );
    for invalid in [
        serde_json::json!(["PRIVATE_KEY_VALUE"]),
        serde_json::json!([
            "FULL_PRIVATE_CONTEXT_UNVERIFIED",
            "FULL_PRIVATE_CONTEXT_UNVERIFIED"
        ]),
        serde_json::json!(["NATIVE_CALL_MISSING", "ASSIGNMENT_MISSING"]),
        serde_json::json!(true),
        serde_json::json!({"rawText":"PRIVATE_CONTEXT"}),
    ] {
        let mut bad = value.clone();
        bad["assistantHints"]["identityIssues"] = invalid;
        assert!(task_response_diagnostics(&format!("PROBE:TASK:{bad}")).is_none());
        assert!(sanitized_rows(&format!("PROBE:TASK:{bad}")).is_empty());
    }
    for field in [
        "identityIssues",
        "mentionedTools",
        "argumentIssues",
        "cachedInvariants",
    ] {
        let mut bad = value.clone();
        bad["assistantHints"].as_object_mut().unwrap().remove(field);
        assert!(task_response_diagnostics(&format!("PROBE:TASK:{bad}")).is_none());
    }
    let mut bad = value;
    bad["assistantHints"]["rawText"] = serde_json::json!("PRIVATE_BODY");
    assert!(sanitized_rows(&format!("PROBE:TASK:{bad}")).is_empty());
}

#[test]
fn full_identity_hint_vocabulary_fits_bounded_task_row_without_arbitrary_text() {
    let mut value = identity_hint_fixture();
    let issues: Vec<String> = [
        "FULL_PRIVATE_CONTEXT",
        "ASSIGNMENT",
        "DELIVERED_USER_MESSAGE",
        "NATIVE_CALL",
        "EXECUTION_GRANT",
        "CONVERSATION_TARGET",
    ]
    .iter()
    .flat_map(|domain| {
        ["MISSING", "MISMATCH", "UNVERIFIED"].map(|state| format!("{domain}_{state}"))
    })
    .collect();
    value["assistantHints"]["identityIssues"] = serde_json::json!(issues);
    value["assistantHints"]["knownCodes"] = serde_json::json!([
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
        "PLAN_REQUIRED"
    ]);
    value["assistantHints"]["cachedInvariants"] = serde_json::json!([
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
        "PAYLOAD_NOT_OBJECT"
    ]);
    let text = value.to_string();
    assert!(text.len() > 2048 && text.len() <= 4096);
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{text}")),
        Some(value)
    );
    let oversized = format!("PROBE:TASK:{}", " ".repeat(4097));
    assert!(task_response_diagnostics(&oversized).is_none());
}

#[test]
fn ordered_refusal_terms_are_fixed_bounded_redactions_not_response_content_or_authority() {
    let mut value = identity_hint_fixture();
    value["assistantHints"]["schemaVersion"] = serde_json::json!(6);
    value["assistantHints"]["refusalTerms"] = serde_json::json!([
        "DISPATCH", "BLOCKED", "REDACTED", "CONTEXT", "IS", "NOT", "VERIFIED"
    ]);
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{value}")),
        Some(value.clone())
    );
    for bad in [
        serde_json::json!(["PRIVATE_VALUE"]),
        serde_json::json!(["execution-key"]),
        serde_json::json!(["REDACTED", "REDACTED"]),
        serde_json::json!(["https://example.invalid/c/private"]),
        serde_json::json!(["a".repeat(64)]),
        serde_json::json!(true),
        serde_json::json!([1]),
        serde_json::json!(vec!["REDACTED"; 33]),
    ] {
        let mut invalid = value.clone();
        invalid["assistantHints"]["refusalTerms"] = bad;
        assert!(sanitized_rows(&format!("PROBE:TASK:{invalid}")).is_empty());
    }
    let mut repeated = value.clone();
    repeated["assistantHints"]["refusalTerms"] = serde_json::json!(["NOT", "NOT", "VERIFIED"]);
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{repeated}")),
        Some(repeated)
    );
    let mut old = value.clone();
    old["assistantHints"]["schemaVersion"] = serde_json::json!(5);
    assert!(task_response_diagnostics(&format!("PROBE:TASK:{old}")).is_none());
    old["assistantHints"]
        .as_object_mut()
        .unwrap()
        .remove("refusalTerms");
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{old}")),
        Some(old)
    );
    value["assistantHints"]
        .as_object_mut()
        .unwrap()
        .remove("refusalTerms");
    assert!(task_response_diagnostics(&format!("PROBE:TASK:{value}")).is_none());
}

#[test]
fn refusal_report_provenance_is_closed_consistent_and_not_native_evidence() {
    let mut value = identity_hint_fixture();
    value["assistantHints"]["schemaVersion"] = serde_json::json!(7);
    value["assistantHints"]["refusalTerms"] = serde_json::json!(["BLOCKED", "CONTEXT", "MISSING"]);
    value["assistantHints"]["refusalReport"] = serde_json::json!({"source":"STRUCTURED","state":"BLOCKED","reasonFieldPresent":true,"projectionOmitted":false});
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{value}")),
        Some(value.clone())
    );
    for (field, replacement) in [
        ("source", serde_json::json!("PRIVATE_BODY")),
        ("state", serde_json::json!("CODEX_PRIVATE_ERROR")),
        ("reasonFieldPresent", serde_json::json!(1)),
        ("projectionOmitted", serde_json::json!(true)),
        ("rawText", serde_json::json!("PRIVATE_SECRET")),
        ("source", serde_json::json!("NONE")),
        ("state", serde_json::json!("UNPROVEN")),
    ] {
        let mut invalid = value.clone();
        invalid["assistantHints"]["refusalReport"][field] = replacement;
        assert!(sanitized_rows(&format!("PROBE:TASK:{invalid}")).is_empty());
    }
    let mut prose = value.clone();
    prose["assistantHints"]["refusalReport"]["source"] = serde_json::json!("PROSE");
    assert!(task_response_diagnostics(&format!("PROBE:TASK:{prose}")).is_none());
    prose["assistantHints"]["refusalReport"]["reasonFieldPresent"] = serde_json::json!(false);
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{prose}")),
        Some(prose)
    );
    let mut none = value.clone();
    none["assistantHints"]["refusalTerms"] = serde_json::json!([]);
    none["assistantHints"]["refusalReport"] = serde_json::json!({"source":"NONE","state":"UNPROVEN","reasonFieldPresent":false,"projectionOmitted":true});
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{none}")),
        Some(none)
    );
    value["assistantHints"]
        .as_object_mut()
        .unwrap()
        .remove("refusalReport");
    assert!(task_response_diagnostics(&format!("PROBE:TASK:{value}")).is_none());
    value["assistantHints"]["schemaVersion"] = serde_json::json!(6);
    assert_eq!(
        task_response_diagnostics(&format!("PROBE:TASK:{value}")),
        Some(value)
    );
}

#[test]
fn passive_rows_never_return_duplicate_key_private_values_or_action_frames() {
    let value = identity_hint_fixture();
    let encoded = value.to_string();
    // Prefix one duplicate field explicitly; do not assume serde object key
    // order or the fixture's current assistant-hint schema version.
    let duplicate = format!(
        "{{\"assistantHints\":\"PRIVATE_SENTINEL\",{}",
        encoded
            .strip_prefix('{')
            .expect("task fixture is an object")
    );
    assert_ne!(duplicate, encoded);
    assert!(duplicate.contains(r#""assistantHints":"PRIVATE_SENTINEL"#));
    assert_eq!(duplicate.matches(r#""assistantHints":"#).count(), 2);
    let rows = sanitized_rows(&format!("PROBE:TASK:{duplicate}"));
    assert_eq!(rows, [format!("PROBE:TASK:{value}")]);
    assert!(!rows[0].contains("PRIVATE_SENTINEL"));
    for row in [
        "PROBE:APP:{}",
        "PROBE:TRACE:{}",
        "PROBE:RECEIPT:{}",
        "PROBE:SUBMISSION:{}",
    ] {
        assert!(sanitized_rows(row).is_empty());
    }
    assert!(embedded_program_loader(b"").is_err());
    assert!(embedded_program_loader(&vec![b'x'; 65537]).is_err());
    assert!(
        embedded_program_loader(b"print('PROBE:FINAL:HOME')")
            .unwrap()
            .contains("hashlib.sha256")
    );
}
