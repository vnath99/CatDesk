//! Bounded foreground developer backend using the existing headless MCP entry
//! and suspended-child Job owner. It never releases a persistent process.

use crate::reviewed_candidate_owner::{OwnedCandidateChild, SpawnFailure};
use catdesk_wake::development_runtime::{self, BackendSelection};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[path = "operator_development_project.rs"]
mod project;
pub(super) use project::{ProjectOptions, parse_project_options, run_project};

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
            let legacy = ready_value(&encode(&ready), selection, 7, &token).unwrap();
            assert_eq!(legacy["status"], "DEV_BACKEND_READY");
            assert!(legacy.get("projectScope").is_none());
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

    #[test]
    fn project_backend_command_and_readiness_pin_project_and_assignment() {
        with_selection(|selection| {
            for assigned in [false, true] {
                let options = ProjectOptions {
                    selection: crate::project_mcp::Selection {
                        project_id: "alpha".into(),
                        target_generation: 7,
                        target_sha256: "a".repeat(64),
                    },
                    assignment: assigned.then(|| selection.workspace.join("approved-task.json")),
                    maximum_seconds: 9,
                };
                let token = "k".repeat(64);
                let command = backend_command_for(selection, 9, &token, Some(&options));
                let args: Vec<_> = command
                    .get_args()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect();
                for pair in [
                    ["--port", "0"],
                    ["--project-id", "alpha"],
                    ["--project-target-generation", "7"],
                    ["--mcp-path", "/project/mcp"],
                    [
                        "--tool-mode",
                        if assigned { "multi-tools" } else { "read-only" },
                    ],
                ] {
                    assert!(args.windows(2).any(|w| w == pair));
                }
                assert_eq!(
                    args.iter().any(|a| a == "--project-task-contract"),
                    assigned
                );
                assert!(
                    args.contains(&"--no-ngrok".into()) && args.contains(&"--no-web-auto".into())
                );
                assert!(!args.iter().any(|a| a.contains(&token)));
                let mut expected = json!({"projectId":"alpha","targetGeneration":7,"targetSha256":"a".repeat(64),
                    "executionWorkspace":selection.workspace,"nativeWriterClaimed":false,"assignedTask":null});
                if assigned {
                    expected["assignedTask"] = json!({"taskId":"task-a","executionWorkspace":selection.workspace.join("isolated"),
                    "registeredWorkspace":selection.workspace,"nativeWriterClaimed":false});
                }
                let mut scope = expected.clone();
                scope["access"] = json!(if assigned {
                    "READ_ONLY_INSPECTION_WITH_ASSIGNED_TASK_PATCH_AND_COMMAND_TOOLS"
                } else {
                    "READ_ONLY"
                });
                let ready = json!({"status":"ready","mode":"Computer","tool_mode":if assigned {"multi-tools"} else {"read-only"},
                    "workspace":selection.workspace,"config_path":selection.workspace.join(".catdesk/web-development/catdesk.toml"),
                    "maximum_runtime_seconds":9,"web_auto_scheduler_enabled":false,"project_scope":scope,
                    "auth":{"scheme":"bearer","token_redacted":true},"mcp_url":"http://127.0.0.1:49123/project/mcp"});
                let encode = |v: &Value| {
                    let mut b = serde_json::to_vec(v).unwrap();
                    b.push(b'\n');
                    b
                };
                assert!(
                    ready_value_for(
                        &encode(&ready),
                        selection,
                        9,
                        &token,
                        Some((&options, &expected))
                    )
                    .is_ok()
                );
                for fault in 0..8 {
                    let mut bad = ready.clone();
                    match fault {
                        0 => bad["project_scope"]["projectId"] = json!("beta"),
                        1 => bad["project_scope"]["targetGeneration"] = json!(8),
                        2 => bad["project_scope"]["targetSha256"] = json!("b".repeat(64)),
                        3 => {
                            bad["project_scope"]["executionWorkspace"] =
                                json!(selection.workspace.parent())
                        }
                        4 => bad["project_scope"]["nativeWriterClaimed"] = json!(true),
                        5 => bad["project_scope"]["assignedTask"] = json!({"taskId":"foreign"}),
                        6 => bad["mcp_url"] = json!("http://127.0.0.1:3200/project/mcp"),
                        _ => bad["tool_mode"] = json!("supervisor-only"),
                    }
                    assert!(
                        ready_value_for(
                            &encode(&bad),
                            selection,
                            9,
                            &token,
                            Some((&options, &expected))
                        )
                        .is_err(),
                        "fault {fault}"
                    );
                }
            }
        });
    }

    #[test]
    fn startup_fallback_requires_known_exit_proof_and_no_published_readiness() {
        for (code, proof) in [
            ("DEV_BACKEND_START_FAILED", ExitProof::NoChildCreated),
            ("DEV_BACKEND_READY_TIMEOUT", ExitProof::OwnedTreeDrained),
            ("DEV_BACKEND_READY_EOF", ExitProof::OwnedTreeDrained),
            (
                "DEV_BACKEND_EXITED_BEFORE_READY",
                ExitProof::OwnedTreeDrained,
            ),
        ] {
            assert!(AttemptFailure::new(code, false, proof).may_restore());
            assert!(!AttemptFailure::new(code, true, proof).may_restore());
            assert!(!AttemptFailure::new(code, false, ExitProof::Unproven).may_restore());
        }
        for code in [
            "DEV_BACKEND_CHILD_FAILED",
            "DEV_BACKEND_READY_INVALID",
            "DEV_BACKEND_OUTPUT_INVALID",
            "DEV_BACKEND_READY_OUTPUT_FAILED",
            "DEV_BACKEND_OWNED_TREE_EXIT_UNPROVEN",
            "UNKNOWN",
        ] {
            assert!(!AttemptFailure::new(code, false, ExitProof::OwnedTreeDrained).may_restore());
        }
        assert!(
            !AttemptFailure::new(
                "DEV_BACKEND_START_FAILED",
                false,
                ExitProof::OwnedTreeDrained
            )
            .may_restore()
        );
        assert!(
            !AttemptFailure::new("DEV_BACKEND_READY_EOF", false, ExitProof::NoChildCreated)
                .may_restore()
        );
    }
}

fn backend_command(selection: &BackendSelection, maximum_seconds: u64, token: &str) -> Command {
    backend_command_for(selection, maximum_seconds, token, None)
}

fn backend_command_for(
    selection: &BackendSelection,
    maximum_seconds: u64,
    token: &str,
    project: Option<&ProjectOptions>,
) -> Command {
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
            if project.is_some() {
                "/project/mcp"
            } else {
                "/central/mcp"
            },
            "--mode",
            "computer",
            "--tool-mode",
            match project {
                Some(p) if p.assignment.is_some() => "multi-tools",
                Some(_) => "read-only",
                None => "supervisor-only",
            },
            "--no-ngrok",
            "--no-web-auto",
            "--max-runtime-seconds",
        ])
        .arg(maximum_seconds.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(project) = project {
        command
            .arg("--project-id")
            .arg(&project.selection.project_id)
            .arg("--project-target-generation")
            .arg(project.selection.target_generation.to_string())
            .arg("--project-target-sha256")
            .arg(&project.selection.target_sha256);
        if let Some(assignment) = &project.assignment {
            command.arg("--project-task-contract").arg(assignment);
        }
    }
    command
        .env_remove("CONTROL_PLANE_API_KEY")
        .env_remove("CATDESK_OPENAI_TUNNEL_ID");
    command
}

fn ready_value(
    bytes: &[u8],
    selection: &BackendSelection,
    maximum_seconds: u64,
    token: &str,
) -> Result<Value, String> {
    ready_value_for(bytes, selection, maximum_seconds, token, None)
}

fn ready_value_for(
    bytes: &[u8],
    selection: &BackendSelection,
    maximum_seconds: u64,
    token: &str,
    project: Option<(&ProjectOptions, &Value)>,
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
        || match project {
            None => !ready["project_scope"].is_null(),
            Some((_, expected)) => {
                project::verify_scope(&ready["project_scope"], expected).is_err()
            }
        }
        || ready["mode"] != "Computer"
        || ready["tool_mode"]
            != match project {
                Some((p, _)) if p.assignment.is_some() => "multi-tools",
                Some(_) => "read-only",
                None => "supervisor-only",
            }
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
        || url.path()
            != if project.is_some() {
                "/project/mcp"
            } else {
                "/central/mcp"
            }
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("DEV_BACKEND_READY_INVALID".into());
    }
    let mut output = json!({"status":"DEV_BACKEND_READY", "mode":"DEVELOPMENT_UNPROTECTED",
        "workspace":selection.workspace, "packageId":selection.package_id,
        "appSha256":selection.app_sha256, "recovery":selection.recovery,
        "mcpUrl":url.as_str(), "maximumRuntimeSeconds":maximum_seconds,
        "webAutoSchedulerEnabled":false,"authTokenRedacted":true});
    if let Some((_, expected)) = project {
        output["projectScope"] = expected.clone();
    }
    Ok(output)
}

fn run_owned(workspace: &Path, maximum_seconds: u64, token: &str) -> Result<Value, String> {
    let selection = development_runtime::select_backend(workspace)?;
    match attempt_backend(&selection, maximum_seconds, token, None, None) {
        Ok(result) => Ok(result),
        Err(failure) if failure.may_restore() => {
            let metadata = json!({"attempted":true,"maximumStartupAttempts":2,
                "failedPackageId":selection.package_id,"failedAppSha256":selection.app_sha256,
                "reason":failure.code,"failedProcessOutcome":failure.exit_proof.label()});
            let previous = selection.restore_after_startup_failure()?;
            // One fallback only. A failed predecessor is reported; it cannot
            // resurrect the failed current release or recursively retry work.
            attempt_backend(&previous, maximum_seconds, token, Some(&metadata), None)
                .map_err(|failure| failure.code)
        }
        Err(failure) => Err(failure.code),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExitProof {
    NoChildCreated,
    OwnedTreeDrained,
    Unproven,
}

impl ExitProof {
    fn label(self) -> &'static str {
        match self {
            Self::NoChildCreated => "NO_CHILD_CREATED",
            Self::OwnedTreeDrained => "OWNED_TREE_DRAINED",
            Self::Unproven => "UNPROVEN",
        }
    }
}

struct AttemptFailure {
    code: String,
    ready_published: bool,
    exit_proof: ExitProof,
}

impl AttemptFailure {
    fn new(code: impl Into<String>, ready_published: bool, exit_proof: ExitProof) -> Self {
        Self {
            code: code.into(),
            ready_published,
            exit_proof,
        }
    }
    fn may_restore(&self) -> bool {
        !self.ready_published
            && match self.exit_proof {
                ExitProof::NoChildCreated => self.code == "DEV_BACKEND_START_FAILED",
                ExitProof::OwnedTreeDrained => matches!(
                    self.code.as_str(),
                    "DEV_BACKEND_READY_TIMEOUT"
                        | "DEV_BACKEND_READY_EOF"
                        | "DEV_BACKEND_EXITED_BEFORE_READY"
                ),
                ExitProof::Unproven => false,
            }
    }
}

fn attempt_backend(
    selection: &BackendSelection,
    maximum_seconds: u64,
    token: &str,
    fallback: Option<&Value>,
    project: Option<(&ProjectOptions, &Value)>,
) -> Result<Value, AttemptFailure> {
    let mut command = match project {
        Some((options, _)) => backend_command_for(selection, maximum_seconds, token, Some(options)),
        None => backend_command(selection, maximum_seconds, token),
    };
    let started = Instant::now();
    let mut owned = OwnedCandidateChild::spawn_classified(&mut command).map_err(|failure| {
        AttemptFailure::new(
            "DEV_BACKEND_START_FAILED",
            false,
            match failure {
                SpawnFailure::NoChildCreated => ExitProof::NoChildCreated,
                SpawnFailure::Owner(_) | SpawnFailure::UnprovenOwner(_) => ExitProof::Unproven,
            },
        )
    })?;
    let stdout = owned.child_mut().stdout.take().ok_or_else(|| {
        AttemptFailure::new("DEV_BACKEND_OUTPUT_UNAVAILABLE", false, ExitProof::Unproven)
    })?;
    let stderr = owned.child_mut().stderr.take().ok_or_else(|| {
        AttemptFailure::new("DEV_BACKEND_OUTPUT_UNAVAILABLE", false, ExitProof::Unproven)
    })?;
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
    let mut ready_published = false;
    let result: Result<std::process::ExitStatus, String> = (|| {
        let bytes =
            receiver
                .recv_timeout(Duration::from_secs(10))
                .map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => "DEV_BACKEND_READY_TIMEOUT",
                    mpsc::RecvTimeoutError::Disconnected => "DEV_BACKEND_READY_UNAVAILABLE",
                })??;
        if bytes.is_empty() {
            return Err("DEV_BACKEND_READY_EOF".into());
        }
        let mut ready = match project {
            Some(_) => ready_value_for(&bytes, selection, maximum_seconds, token, project)?,
            None => ready_value(&bytes, selection, maximum_seconds, token)?,
        };
        if owned
            .child_mut()
            .try_wait()
            .map_err(|_| "DEV_BACKEND_WAIT_FAILED")?
            .is_some()
        {
            return Err("DEV_BACKEND_EXITED_BEFORE_READY".into());
        }
        ready["pid"] = json!(owned.pid());
        if let Some(metadata) = fallback {
            ready["startupFallback"] = metadata.clone();
        }
        // A partial/failed write may already have exposed readiness to a caller.
        // From this point onward there is never automatic backend replacement.
        ready_published = true;
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
    owned.terminate_tree_and_confirm_quiescent().map_err(|_| {
        AttemptFailure::new(
            "DEV_BACKEND_OWNED_TREE_EXIT_UNPROVEN",
            ready_published,
            ExitProof::Unproven,
        )
    })?;
    let output = output_reader.join().map_err(|_| {
        AttemptFailure::new(
            "DEV_BACKEND_OUTPUT_UNAVAILABLE",
            ready_published,
            ExitProof::OwnedTreeDrained,
        )
    })?;
    let error = error_reader.join().map_err(|_| {
        AttemptFailure::new(
            "DEV_BACKEND_OUTPUT_UNAVAILABLE",
            ready_published,
            ExitProof::OwnedTreeDrained,
        )
    })?;
    if output.overflow
        || output.read_failed
        || error.overflow
        || error.read_failed
        || output.text.contains(token)
        || error.text.contains(token)
    {
        return Err(AttemptFailure::new(
            "DEV_BACKEND_OUTPUT_INVALID",
            ready_published,
            ExitProof::OwnedTreeDrained,
        ));
    }
    let exit = result
        .map_err(|code| AttemptFailure::new(code, ready_published, ExitProof::OwnedTreeDrained))?;
    Ok(
        json!({"mode":"DEVELOPMENT_UNPROTECTED","status":"FOREGROUND_BACKEND_EXITED",
        "packageId":selection.package_id,"appSha256":selection.app_sha256,
        "recovery":selection.recovery,"exitCode":exit.code(),"ownedTreeExited":true,
        "maximumRuntimeSeconds":maximum_seconds,"browserAction":"NONE","wakeAction":"NONE",
        "startupFallback":fallback}),
    )
}
