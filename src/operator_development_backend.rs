//! Bounded foreground developer backend using the existing headless MCP entry
//! and suspended-child Job owner. It never releases a persistent process.

use crate::reviewed_candidate_owner::OwnedCandidateChild;
use catdesk_wake::development_runtime::{self, BackendSelection};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub(super) async fn run(workspace: &Path, maximum_seconds: u64) -> Result<Value, String> {
    if !(1..=900).contains(&maximum_seconds) {
        return Err("DEV_BACKEND_BUDGET_INVALID".into());
    }
    let token = process_auth(std::env::var("CATDESK_MCP_AUTH_TOKEN").ok())?;
    let workspace = workspace.to_path_buf();
    tokio::task::spawn_blocking(move || run_owned(&workspace, maximum_seconds, &token))
        .await
        .map_err(|_| "DEV_BACKEND_OWNER_FAILED")?
}

fn process_auth(token: Option<String>) -> Result<String, String> {
    token
        .filter(|token| {
            token.len() >= 24
                && token.len() <= 4096
                && !token.chars().any(|c| c.is_whitespace() || c.is_control())
        })
        .ok_or("DEV_BACKEND_PROCESS_AUTH_REQUIRED".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn with_selection(check: impl FnOnce(&BackendSelection)) {
        let parent =
            std::env::temp_dir().join(format!("catdesk-backend-contract-{}", uuid::Uuid::new_v4()));
        let workspace = parent.join("control");
        let package = parent.join("package");
        let python = parent.join("python");
        for path in [&workspace, &package, &python] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(package.join("CatDeskWakeHost.exe"), b"inert test host").unwrap();
        fs::write(package.join("CatDeskBinagotchy.exe"), b"inert test app").unwrap();
        fs::write(
            package.join("adapter.py"),
            include_bytes!("../wake/adapter.py"),
        )
        .unwrap();
        fs::write(
            package.join("wake_bridge.py"),
            include_bytes!("../scripts/wake_bridge.py"),
        )
        .unwrap();
        fs::write(python.join("python.exe"), b"inert test python").unwrap();
        fs::write(python.join("__init__.py"), b"").unwrap();
        development_runtime::prepare(&workspace, &package, &python).unwrap();
        let selection = development_runtime::select_backend(&workspace).unwrap();
        check(&selection);
        drop(selection);
        let canonical = parent.canonicalize().unwrap();
        assert_eq!(
            canonical.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        assert!(
            canonical
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("catdesk-backend-contract-")
        );
        fs::remove_dir_all(canonical).unwrap();
    }

    #[test]
    fn fixed_backend_command_keeps_auth_and_runtime_scope_out_of_caller_arguments() {
        with_selection(|selection| {
            let token = "x".repeat(64);
            let command = backend_command(selection, 7, &token);
            let arguments: Vec<_> = command
                .get_args()
                .map(|value| value.to_string_lossy().into_owned())
                .collect();
            assert_eq!(command.get_program(), selection.executable.as_os_str());
            assert_eq!(
                command.get_current_dir(),
                Some(selection.workspace.as_path())
            );
            assert!(
                !arguments.iter().any(|arg| arg.contains(&token)
                    || arg == "--auth-token"
                    || arg == "--web-auto")
            );
            assert!(arguments.iter().any(|arg| arg == "--no-web-auto"));
            assert!(arguments.iter().any(|arg| arg == "--no-ngrok"));
            for pair in [
                ["--host", "127.0.0.1"],
                ["--port", "0"],
                ["--max-runtime-seconds", "7"],
            ] {
                assert!(arguments.windows(2).any(|window| window == pair));
            }
            assert!(
                command
                    .get_envs()
                    .any(|(name, value)| name == "CATDESK_MCP_AUTH_TOKEN"
                        && value == Some(std::ffi::OsStr::new(&token)))
            );
        });
    }

    #[test]
    fn backend_ready_refuses_wrong_workspace_endpoint_scheduler_or_secret_echo() {
        with_selection(|selection| {
            let token = "x".repeat(64);
            let ready = json!({"status":"ready","mode":"Computer","tool_mode":"supervisor-only",
                "workspace":selection.workspace,"config_path":selection.workspace.join(".catdesk/web-development/catdesk.toml"),
                "maximum_runtime_seconds":7,"web_auto_scheduler_enabled":false,"project_scope":null,
                "auth":{"scheme":"bearer","token_redacted":true},"mcp_url":"http://127.0.0.1:49123/central/mcp"});
            let encode = |value: &Value| {
                let mut bytes = serde_json::to_vec(value).unwrap();
                bytes.push(b'\n');
                bytes
            };
            assert_eq!(
                ready_value(&encode(&ready), selection, 7, &token).unwrap()["status"],
                "DEV_BACKEND_READY"
            );
            for fault in 0..9 {
                let mut changed = ready.clone();
                match fault {
                    0 => changed["workspace"] = json!(selection.workspace.parent().unwrap()),
                    1 => {
                        changed["config_path"] =
                            json!(selection.workspace.join("other-config.toml"))
                    }
                    2 => changed["mcp_url"] = json!("http://example.test:49123/central/mcp"),
                    3 => changed["mcp_url"] = json!("http://127.0.0.1:3200/central/mcp"),
                    4 => changed["web_auto_scheduler_enabled"] = json!(true),
                    5 => changed["project_scope"] = json!({"projectId":"unrequested-project"}),
                    6 => changed["maximum_runtime_seconds"] = json!(8),
                    7 => changed["auth"]["token_redacted"] = json!(false),
                    8 => changed["unexpectedSecret"] = json!(token),
                    _ => unreachable!(),
                }
                assert_eq!(
                    ready_value(&encode(&changed), selection, 7, &token).unwrap_err(),
                    "DEV_BACKEND_READY_INVALID"
                );
            }
            assert!(ready_value(&vec![b'a'; 64 * 1024 + 1], selection, 7, &token).is_err());
        });
    }

    #[test]
    fn backend_process_auth_requires_a_bounded_nonempty_header_safe_value() {
        for value in [
            None,
            Some("short".into()),
            Some("x".repeat(4097)),
            Some(format!("{}\n", "x".repeat(32))),
            Some(format!("{}\0", "x".repeat(32))),
        ] {
            assert_eq!(
                process_auth(value).unwrap_err(),
                "DEV_BACKEND_PROCESS_AUTH_REQUIRED"
            );
        }
        assert_eq!(process_auth(Some("x".repeat(24))).unwrap(), "x".repeat(24));
    }
}

fn backend_command(selection: &BackendSelection, maximum_seconds: u64, token: &str) -> Command {
    let mut command = Command::new(&selection.executable);
    command
        .current_dir(&selection.workspace)
        .env("WORKSPACE_ROOT", &selection.workspace)
        .env("CATDESK_MCP_AUTH_TOKEN", token)
        .args([
            "--headless-mcp",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--workspace",
        ])
        .arg(&selection.workspace)
        .arg("--config-path")
        .arg(
            selection
                .workspace
                .join(".catdesk/web-development/catdesk.toml"),
        )
        .args([
            "--mcp-path",
            "/central/mcp",
            "--mode",
            "computer",
            "--tool-mode",
            "supervisor-only",
            "--no-ngrok",
            "--no-web-auto",
            "--max-runtime-seconds",
        ])
        .arg(maximum_seconds.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn ready_value(
    bytes: &[u8],
    selection: &BackendSelection,
    maximum_seconds: u64,
    token: &str,
) -> Result<Value, String> {
    let line = std::str::from_utf8(bytes).map_err(|_| "DEV_BACKEND_READY_INVALID")?;
    if bytes.len() > 64 * 1024 || !bytes.ends_with(b"\n") || line.contains(token) {
        return Err("DEV_BACKEND_READY_INVALID".into());
    }
    let ready: Value = serde_json::from_str(line).map_err(|_| "DEV_BACKEND_READY_INVALID")?;
    let url = ready["mcp_url"]
        .as_str()
        .and_then(|value| reqwest::Url::parse(value).ok())
        .ok_or("DEV_BACKEND_READY_INVALID")?;
    let workspace = ready["workspace"]
        .as_str()
        .map(Path::new)
        .and_then(|p| p.canonicalize().ok());
    let config = ready["config_path"].as_str().map(Path::new);
    if ready["status"] != "ready"
        || ready["maximum_runtime_seconds"].as_u64() != Some(maximum_seconds)
        || ready["web_auto_scheduler_enabled"] != false
        || !ready["project_scope"].is_null()
        || ready["mode"] != "Computer"
        || ready["tool_mode"] != "supervisor-only"
        || ready["auth"]["scheme"] != "bearer"
        || ready["auth"]["token_redacted"] != true
        || workspace.as_deref() != Some(selection.workspace.as_path())
        || config
            != Some(
                selection
                    .workspace
                    .join(".catdesk/web-development/catdesk.toml")
                    .as_path(),
            )
        || url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_none_or(|port| port == 0 || port == 3200)
        || url.path() != "/central/mcp"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("DEV_BACKEND_READY_INVALID".into());
    }
    Ok(
        json!({"status":"DEV_BACKEND_READY", "mode":"DEVELOPMENT_UNPROTECTED",
        "workspace":selection.workspace, "packageId":selection.package_id,
        "appSha256":selection.app_sha256, "recovery":selection.recovery,
        "mcpUrl":url.as_str(), "maximumRuntimeSeconds":maximum_seconds,
        "webAutoSchedulerEnabled":false,"authTokenRedacted":true}),
    )
}

fn run_owned(workspace: &Path, maximum_seconds: u64, token: &str) -> Result<Value, String> {
    let selection = development_runtime::select_backend(workspace)?;
    let mut command = backend_command(&selection, maximum_seconds, token);
    let started = Instant::now();
    let mut owned =
        OwnedCandidateChild::spawn(&mut command).map_err(|_| "DEV_BACKEND_START_FAILED")?;
    let stdout = owned
        .child_mut()
        .stdout
        .take()
        .ok_or("DEV_BACKEND_OUTPUT_UNAVAILABLE")?;
    let stderr = owned
        .child_mut()
        .stderr
        .take()
        .ok_or("DEV_BACKEND_OUTPUT_UNAVAILABLE")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let output_reader = thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::new();
        let result = Read::by_ref(&mut reader)
            .take(64 * 1024 + 1)
            .read_until(b'\n', &mut line)
            .map(|_| line)
            .map_err(|_| "DEV_BACKEND_READY_UNAVAILABLE");
        let _ = sender.send(result);
        super::bounded_reader(reader)
    });
    let error_reader = thread::spawn(move || super::bounded_reader(stderr));
    let result: Result<std::process::ExitStatus, String> = (|| {
        let bytes = receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "DEV_BACKEND_READY_TIMEOUT")??;
        let mut ready = ready_value(&bytes, &selection, maximum_seconds, token)?;
        if owned
            .child_mut()
            .try_wait()
            .map_err(|_| "DEV_BACKEND_WAIT_FAILED")?
            .is_some()
        {
            return Err("DEV_BACKEND_EXITED_BEFORE_READY".into());
        }
        ready["pid"] = json!(owned.pid());
        println!("{ready}");
        std::io::stdout()
            .flush()
            .map_err(|_| "DEV_BACKEND_READY_OUTPUT_FAILED")?;
        let deadline = Duration::from_secs(maximum_seconds + 15);
        loop {
            if let Some(exit) = owned
                .child_mut()
                .try_wait()
                .map_err(|_| "DEV_BACKEND_WAIT_FAILED")?
            {
                return if exit.success() {
                    Ok(exit)
                } else {
                    Err("DEV_BACKEND_CHILD_FAILED".into())
                };
            }
            if started.elapsed() >= deadline {
                return Err("DEV_BACKEND_OWNER_DEADLINE".into());
            }
            thread::sleep(Duration::from_millis(50));
        }
    })();
    // Keep selection/image pins until exact Job accounting proves the complete
    // child tree empty, including descendants of a naturally exited backend.
    let cleanup = owned
        .terminate_tree_and_confirm_quiescent()
        .map_err(|_| "DEV_BACKEND_OWNED_TREE_EXIT_UNPROVEN");
    cleanup?;
    let output = output_reader
        .join()
        .map_err(|_| "DEV_BACKEND_OUTPUT_UNAVAILABLE")?;
    let error = error_reader
        .join()
        .map_err(|_| "DEV_BACKEND_OUTPUT_UNAVAILABLE")?;
    let exit = result?;
    if output.overflow
        || output.read_failed
        || error.overflow
        || error.read_failed
        || output.text.contains(token)
        || error.text.contains(token)
    {
        return Err("DEV_BACKEND_OUTPUT_INVALID".into());
    }
    Ok(
        json!({"mode":"DEVELOPMENT_UNPROTECTED","status":"FOREGROUND_BACKEND_EXITED",
        "packageId":selection.package_id,"appSha256":selection.app_sha256,
        "recovery":selection.recovery,"exitCode":exit.code(),"ownedTreeExited":true,
        "maximumRuntimeSeconds":maximum_seconds,"browserAction":"NONE","wakeAction":"NONE"}),
    )
}
