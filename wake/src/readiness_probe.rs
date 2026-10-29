//! Bounded, metadata-only readiness probe shared by the existing profile probe
//! and the guarded development inspector. START precedes every browser import.
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

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
        .map(str::to_owned)
        .collect())
}
