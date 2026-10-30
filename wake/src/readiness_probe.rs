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

pub fn run_bounded(command: &mut Command, timeout: Duration) -> Result<Vec<String>, &'static str> {
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
    result?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or("PROBE_PIPE_FAILED")?
        .take(16_385)
        .read_to_end(&mut bytes)
        .map_err(|_| "PROBE_OUTPUT_FAILED")?;
    if bytes.len() > 16_384 {
        return Err("PROBE_OUTPUT_TOO_LARGE");
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "PROBE_OUTPUT_INVALID")?;
    Ok(text
        .lines()
        .filter(|line| {
            let Some(state) = line.strip_prefix("PROBE:") else {
                return false;
            };
            if state.starts_with("SURFACE:") {
                return surface_observation(line).is_some();
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
            surface_observation(line)
                .map(|value| format!("PROBE:SURFACE:{value}"))
                .unwrap_or_else(|| line.to_owned())
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn surface() -> serde_json::Value {
        serde_json::json!({"route":"EXACT","document":"COMPLETE","history":"PRESENT",
            "editor":"READY","composer":"EMPTY","generation":"IDLE","assistant":"LOAD_ERROR",
            "exchange":"UNATTRIBUTED","recovery":"OBSERVATION_ONLY"})
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
        let script = format!(
            "if([Console]::In.ReadLine() -cne 'START'){{exit 2}}; for($i=0;$i -lt 90;$i++){{[Console]::Out.WriteLine(('x'*150))}}; [Console]::Out.WriteLine('PROBE:SURFACE:{row}'); [Console]::Out.WriteLine('PROBE:FINAL:EXACT_READY_RESPONSE_LOAD_ERROR')"
        );
        command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        let rows = run_bounded(&mut command, Duration::from_secs(5)).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(surface_observation(&rows[0]), Some(surface()));
        assert_eq!(rows[1], "PROBE:FINAL:EXACT_READY_RESPONSE_LOAD_ERROR");
        assert!(!rows.join(" ").contains("PRIVATE_SENTINEL"));
    }
}
