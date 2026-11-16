//! Fixed operator-approved argv through the installed native command sandbox.
//! No model turn, shell interpolation, machine setup or unrestricted fallback.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommandProfile {
    pub id: String,
    pub argv: Vec<String>,
    pub timeout_seconds: u64,
}

impl CommandProfile {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !catdesk_wake::protocol::valid_id(&self.id)
            || self.id.len() > 128
            || self.argv.is_empty()
            || self.argv.len() > 32
            || self
                .argv
                .iter()
                .any(|arg| arg.len() > 8192 || arg.contains('\0'))
            || self.argv.iter().map(String::len).sum::<usize>() > 32768
            || !(1..=120).contains(&self.timeout_seconds)
        {
            return Err("PROJECT_COMMAND_PROFILE_INVALID".into());
        }
        let executable = Path::new(&self.argv[0]);
        if !executable.is_absolute()
            || executable
                .extension()
                .is_none_or(|s| !s.eq_ignore_ascii_case("exe"))
            || !executable.is_file()
        {
            return Err("PROJECT_COMMAND_DIRECT_EXECUTABLE_REQUIRED".into());
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandResult {
    pub profile_id: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cleanup_proved: bool,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub native_cli_sha256: String,
    pub executable_sha256: String,
    pub profile_sha256: String,
    pub elapsed_ms: u64,
    pub preparation_ms: u64,
    pub model_calls: u32,
    pub native_session_action: &'static str,
    pub write_scope: &'static str,
    pub read_scope: &'static str,
    pub task_completion_verified: bool,
}

impl CommandResult {
    /// Advisory classification of bounded process output, never execution or
    /// recovery authority: a child can print the same text as the sandbox.
    pub(crate) fn failure_diagnostic(&self) -> Option<serde_json::Value> {
        const SPAWN_DENIED: &str = "windows sandbox failed: runner failed during SpawnChild: CreateProcessAsUserW failed: 5 (Access is denied.)";
        if self.success
            || self.timed_out
            || !self.cleanup_proved
            || self.stderr_truncated
            || !self.exit_code.is_some_and(|code| code != 0)
            || !self.stderr.lines().any(|line| {
                line.strip_prefix(SPAWN_DENIED)
                    .is_some_and(|tail| tail.is_empty() || tail.starts_with(" | "))
            })
        {
            return None;
        }
        Some(serde_json::json!({
            "schemaVersion": 1,
            "layer": "NATIVE_SANDBOX",
            "reason": "SPAWN_ACCESS_DENIED_REPORTED",
            "basis": "REPORTED_PROCESS_OUTPUT",
            "targetExecution": "UNVERIFIED",
            "retryAuthorized": false
        }))
    }
}

// Only failures emitted before spawning are known non-launches. Process/pipe/
// exit/cleanup uncertainty always retains the task's durable writer marker.
pub(crate) fn failed_before_launch(reason: &str) -> bool {
    matches!(
        reason,
        "PROJECT_COMMAND_PROFILE_INVALID"
            | "PROJECT_COMMAND_DIRECT_EXECUTABLE_REQUIRED"
            | "PROJECT_COMMAND_NATIVE_SANDBOX_UNAVAILABLE"
            | "PROJECT_COMMAND_IMAGE_UNAVAILABLE"
            | "PROJECT_COMMAND_NATIVE_CONTEXT_UNAVAILABLE"
            | "PROJECT_COMMAND_POLICY_OVERLAP"
            | "PROJECT_COMMAND_POLICY_INVALID"
            | "PROJECT_COMMAND_TEMP_UNAVAILABLE"
            | "PROJECT_COMMAND_BUDGET_EXPIRED"
            | "PROJECT_COMMAND_BUDGET_EXPIRED_BEFORE_LAUNCH"
    )
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::delegated::autonomy_runtime::{operator_codex_home, resolve_codex_executable};
    use crate::reviewed_candidate_owner::OwnedCandidateChild;
    use sha2::{Digest, Sha256};
    use std::fs::{File, OpenOptions};
    use std::io::Read;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::process::{Command, Stdio};
    use std::sync::mpsc::{self, Receiver};
    use std::time::Instant;

    const CAPTURE: usize = 64 * 1024;

    fn retained_image(path: &Path) -> Result<(File, String), String> {
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(path)
            .map_err(|_| "PROJECT_COMMAND_IMAGE_UNAVAILABLE")?;
        let metadata = file
            .metadata()
            .map_err(|_| "PROJECT_COMMAND_IMAGE_UNAVAILABLE")?;
        if !metadata.is_file()
            || metadata.file_attributes() & 0x400 != 0
            || metadata.len() > 512 * 1024 * 1024
        {
            return Err("PROJECT_COMMAND_IMAGE_UNAVAILABLE".into());
        }
        let mut hasher = Sha256::new();
        let mut chunk = [0; 32768];
        loop {
            let n = file
                .read(&mut chunk)
                .map_err(|_| "PROJECT_COMMAND_IMAGE_UNAVAILABLE")?;
            if n == 0 {
                break;
            }
            hasher.update(&chunk[..n]);
        }
        Ok((file, format!("{:x}", hasher.finalize())))
    }

    fn pump(mut reader: impl Read + Send + 'static) -> Receiver<Result<(String, bool), ()>> {
        let (send, receive) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let mut truncated = false;
            let mut chunk = [0; 8192];
            let value = loop {
                match reader.read(&mut chunk) {
                    Ok(0) => break Ok((String::from_utf8_lossy(&kept).into_owned(), truncated)),
                    Ok(n) => {
                        let available = CAPTURE.saturating_sub(kept.len());
                        kept.extend_from_slice(&chunk[..n.min(available)]);
                        truncated |= n > available;
                    }
                    Err(_) => break Err(()),
                }
            };
            let _ = send.send(value);
        });
        receive
    }

    fn policy(root: &Path, deny: &[PathBuf], _codex_home: Option<&Path>) -> Result<String, String> {
        let mut filesystem = toml::map::Map::new();
        filesystem.insert(":root".into(), toml::Value::String("read".into()));
        filesystem.insert(
            root.to_string_lossy().into_owned(),
            toml::Value::String("write".into()),
        );
        for name in [".git", ".catdesk", ".codex", ".agents"] {
            filesystem.insert(
                root.join(name).to_string_lossy().into_owned(),
                toml::Value::String("read".into()),
            );
        }
        for path in deny.iter().map(PathBuf::as_path) {
            if root.starts_with(path) || path.starts_with(root) {
                return Err("PROJECT_COMMAND_POLICY_OVERLAP".into());
            }
        }
        // Read-deny ACLs in the installed legacy sandbox were observed
        // interrupting another native-equivalent command. Keep its existing
        // root-read posture; do not mutate another checkout/auth home's ACLs.
        // The per-command write capability is still only this task root.
        // Replace one unique profile table at CLI precedence. No saved config
        // is edited and managed requirements are explicitly included below.
        let entries = filesystem
            .iter()
            .map(|(key, value)| {
                let key =
                    serde_json::to_string(key).map_err(|_| "PROJECT_COMMAND_POLICY_INVALID")?;
                let value =
                    serde_json::to_string(value.as_str().ok_or("PROJECT_COMMAND_POLICY_INVALID")?)
                        .map_err(|_| "PROJECT_COMMAND_POLICY_INVALID")?;
                Ok(format!("{key}={value}"))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(format!(
            "{{filesystem={{{}}},network={{enabled=false}}}}",
            entries.join(",")
        ))
    }

    pub(super) fn run(
        root: &Path,
        deny: &[PathBuf],
        profile: &CommandProfile,
        budget: Duration,
    ) -> Result<CommandResult, String> {
        profile.validate()?;
        // Keep canonical paths for identity, but pass ordinary local paths to
        // tools such as Windows PowerShell which classify verbatim script
        // paths as remote and apply different signature requirements.
        let normal_root = crate::command::normalize_windows_verbatim_path(root.to_path_buf());
        let root = normal_root.as_path();
        let deny: Vec<PathBuf> = deny
            .iter()
            .cloned()
            .map(crate::command::normalize_windows_verbatim_path)
            .collect();
        let started = Instant::now();
        let maximum = budget.min(Duration::from_secs(profile.timeout_seconds));
        if maximum < Duration::from_secs(1) {
            return Err("PROJECT_COMMAND_BUDGET_EXPIRED".into());
        }
        let cli =
            resolve_codex_executable().map_err(|_| "PROJECT_COMMAND_NATIVE_SANDBOX_UNAVAILABLE")?;
        let (_cli_pin, native_cli_sha256) = retained_image(&cli)?;
        let (_program_pin, executable_sha256) = retained_image(Path::new(&profile.argv[0]))?;
        let codex_home = operator_codex_home()
            .map_err(|_| "PROJECT_COMMAND_NATIVE_CONTEXT_UNAVAILABLE")?
            .or_else(|| std::env::var_os("USERPROFILE").map(|s| PathBuf::from(s).join(".codex")));
        let name = format!(
            "catdesk_task_{}",
            &catdesk_wake::protocol::digest(&root.to_string_lossy())[..16]
        );
        let override_value = format!(
            "permissions.{name}={}",
            policy(root, &deny, codex_home.as_deref())?
        );
        let mut command = Command::new(&cli);
        command.current_dir(root).env_clear();
        // Never pass MCP bearer, API/provider credentials or inherited Git
        // tracing/config redirects to a command. Keep normal runtime discovery.
        for key in [
            "PATH",
            "PATHEXT",
            "SystemRoot",
            "WINDIR",
            "COMSPEC",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "USERNAME",
            "USERDOMAIN",
            "PROGRAMFILES",
            "PROGRAMFILES(X86)",
            "PROGRAMDATA",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let temp = root.join(".catdesk-command-temp");
        match std::fs::create_dir(&temp) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("PROJECT_COMMAND_TEMP_UNAVAILABLE".into()),
        }
        let temp_info =
            std::fs::symlink_metadata(&temp).map_err(|_| "PROJECT_COMMAND_TEMP_UNAVAILABLE")?;
        if !temp_info.is_dir() || temp_info.file_attributes() & 0x400 != 0 {
            return Err("PROJECT_COMMAND_TEMP_UNAVAILABLE".into());
        }
        command.env("TEMP", &temp).env("TMP", &temp);
        if let Some(home) = &codex_home {
            command.env("CODEX_HOME", home);
        }
        command
            .args([
                "sandbox",
                "-c",
                &override_value,
                "-P",
                &name,
                "--include-managed-config",
                "-C",
            ])
            .arg(root)
            .arg("--")
            .args(profile.argv.iter().map(|arg| {
                if arg.starts_with("\\\\?\\") && Path::new(arg).exists() {
                    crate::command::normalize_windows_verbatim_path(PathBuf::from(arg))
                        .to_string_lossy()
                        .into_owned()
                } else {
                    arg.clone()
                }
            }))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let preparation_ms = started.elapsed().as_millis() as u64;
        if started.elapsed() >= maximum {
            return Err("PROJECT_COMMAND_BUDGET_EXPIRED_BEFORE_LAUNCH".into());
        }
        let mut owner =
            OwnedCandidateChild::spawn(&mut command).map_err(|_| "PROJECT_COMMAND_SPAWN_FAILED")?;
        let stdout = pump(
            owner
                .child_mut()
                .stdout
                .take()
                .ok_or("PROJECT_COMMAND_PIPE_UNAVAILABLE")?,
        );
        let stderr = pump(
            owner
                .child_mut()
                .stderr
                .take()
                .ok_or("PROJECT_COMMAND_PIPE_UNAVAILABLE")?,
        );
        let deadline = started + maximum;
        let mut exit_code = None;
        let mut timed_out = false;
        loop {
            match owner.child_mut().try_wait() {
                Ok(Some(status)) => {
                    exit_code = status.code();
                    break;
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(15))
                }
                Ok(None) => {
                    timed_out = true;
                    break;
                }
                Err(_) => break,
            }
        }
        // Normal parent exit also drains/terminates any remaining descendants.
        // Only retained Job/Child handles are used, never enumerated PID kills.
        owner
            .terminate_tree_and_confirm_quiescent()
            .map_err(|_| "PROJECT_COMMAND_CLEANUP_UNPROVEN")?;
        let (stdout, stdout_truncated) = stdout
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "PROJECT_COMMAND_OUTPUT_EXIT_UNPROVEN")?
            .map_err(|_| "PROJECT_COMMAND_OUTPUT_UNAVAILABLE")?;
        let (stderr, stderr_truncated) = stderr
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| "PROJECT_COMMAND_OUTPUT_EXIT_UNPROVEN")?
            .map_err(|_| "PROJECT_COMMAND_OUTPUT_UNAVAILABLE")?;
        if !timed_out && exit_code.is_none() {
            return Err("PROJECT_COMMAND_EXIT_UNPROVEN".into());
        }
        let profile_sha256 = catdesk_wake::protocol::digest(
            &serde_json::to_string(profile).map_err(|_| "PROJECT_COMMAND_PROFILE_INVALID")?,
        );
        Ok(CommandResult {
            profile_id: profile.id.clone(),
            exit_code,
            timed_out,
            cleanup_proved: true,
            success: !timed_out && exit_code == Some(0),
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            native_cli_sha256,
            executable_sha256,
            profile_sha256,
            elapsed_ms: started.elapsed().as_millis() as u64,
            preparation_ms,
            model_calls: 0,
            native_session_action: "NONE",
            write_scope: "ASSIGNED_WORKTREE_EXCEPT_CONTROL_METADATA",
            read_scope: "EXISTING_NATIVE_ROOT_READ_POSTURE_FILE_TOOLS_REMAIN_PROJECT_SCOPED",
            task_completion_verified: false,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[derive(Clone, Copy)]
        enum RuntimePlacement {
            Installed,
            Workspace,
            PrivateDenied,
            PrivateProvisioned,
        }

        fn native_canary(
            mut profile: CommandProfile,
            expected: impl Fn(&str) -> bool,
            placement: RuntimePlacement,
        ) {
            let fixture = std::env::temp_dir().join(format!(
                "catdesk-native-command-canary-{}",
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir(&fixture).unwrap();
            let root = fixture.join("work");
            std::fs::create_dir(&root).unwrap();
            let mut staged_files = serde_json::Map::new();
            let mut external_runtime = None;
            if !matches!(placement, RuntimePlacement::Installed) {
                let source = Path::new(&profile.argv[0]).parent().unwrap();
                let target = if matches!(
                    placement,
                    RuntimePlacement::PrivateDenied | RuntimePlacement::PrivateProvisioned
                ) {
                    // Use a fresh private location outside the workspace's
                    // ancestors/default temp access, so the negative control
                    // cannot accidentally inherit access from the write root.
                    let home = PathBuf::from(
                        std::env::var_os("CATDESK_TEST_PRIVATE_RUNTIME_PARENT")
                            .expect("PRIVATE_RUNTIME_FIXTURE_PARENT_REQUIRED"),
                    );
                    let target = home.join(format!(
                        "catdesk-private-runtime-probe-{}",
                        uuid::Uuid::new_v4()
                    ));
                    assert!(target.is_absolute() && target.parent() == Some(home.as_path()));
                    external_runtime = Some(target.clone());
                    target
                } else {
                    root.join("runtime")
                };
                std::fs::create_dir(&target).unwrap();
                for name in [
                    "python.exe",
                    "python3.dll",
                    "python312.dll",
                    "vcruntime140.dll",
                    "vcruntime140_1.dll",
                ] {
                    let (_source_pin, expected_sha) = retained_image(&source.join(name)).unwrap();
                    std::fs::copy(source.join(name), target.join(name)).unwrap();
                    let (_target_pin, actual_sha) = retained_image(&target.join(name)).unwrap();
                    assert_eq!(actual_sha, expected_sha, "STAGED_RUNTIME_IMAGE_CHANGED");
                    staged_files.insert(name.into(), serde_json::json!(actual_sha));
                }
                if matches!(
                    placement,
                    RuntimePlacement::PrivateDenied | RuntimePlacement::PrivateProvisioned
                ) {
                    std::fs::create_dir_all(target.join("Lib/encodings")).unwrap();
                    let mut files = vec![source.join("Lib/os.py")];
                    files.extend(
                        std::fs::read_dir(source.join("Lib/encodings"))
                            .unwrap()
                            .map(|entry| entry.unwrap().path())
                            .filter(|path| path.extension().is_some_and(|ext| ext == "py")),
                    );
                    files.sort();
                    let mut manifest = Vec::new();
                    for path in files {
                        let (_pin, hash) = retained_image(&path).unwrap();
                        let relative = path.strip_prefix(source).unwrap();
                        std::fs::copy(&path, target.join(relative)).unwrap();
                        let (_copy_pin, copy_hash) =
                            retained_image(&target.join(relative)).unwrap();
                        assert_eq!(hash, copy_hash, "STAGED_STDLIB_CHANGED");
                        manifest.push((relative.to_string_lossy().into_owned(), hash));
                    }
                    staged_files.insert(
                        "minimalStdlib".into(),
                        serde_json::json!({
                            "fileCount": manifest.len(),
                            "manifestSha256": format!("{:x}", Sha256::digest(serde_json::to_vec(&manifest).unwrap()))
                        }),
                    );
                }
                // Only this disposable probe profile changes. Original tasks,
                // installed interpreter, grants and saved configuration do not.
                profile.id = "python-staged-version-only".into();
                profile.argv[0] = target.join("python.exe").to_string_lossy().into_owned();
            }
            if matches!(placement, RuntimePlacement::PrivateDenied) {
                let baseline = run(&root, &[], &profile, Duration::from_secs(15)).unwrap();
                let receipt = serde_json::to_vec(&baseline).unwrap();
                std::fs::write(root.join("baseline.private.json"), &receipt).unwrap();
                let refused = !baseline.success
                    && baseline.cleanup_proved
                    && baseline.failure_diagnostic().is_some();
                println!(
                    "\nCATDESK_RUNTIME_BASELINE={}",
                    serde_json::json!({
                        "launchRefused": refused, "exitCode": baseline.exit_code,
                        "cleanupProved": baseline.cleanup_proved, "timedOut": baseline.timed_out,
                        "failureDiagnostic": baseline.failure_diagnostic(),
                        "privateReceiptSha256": format!("{:x}", Sha256::digest(&receipt))
                    })
                );
                assert!(refused, "RUNTIME_ACCESS_BASELINE_NOT_REPRODUCED");
                return;
            }
            let result = run(&root, &[], &profile, Duration::from_secs(15));
            let (passed, evidence) = match result {
                Ok(value) => {
                    let receipt = serde_json::to_vec(&value).unwrap();
                    std::fs::write(root.join("command-result.private.json"), &receipt).unwrap();
                    let expected_output_observed = expected(&value.stdout);
                    (
                        value.success && value.cleanup_proved && expected_output_observed,
                        serde_json::json!({
                            "exitCode": value.exit_code, "timedOut": value.timed_out,
                            "cleanupProved": value.cleanup_proved,
                            "nativeCliSha256": value.native_cli_sha256,
                            "elapsedMs": value.elapsed_ms,
                            "expectedOutputObserved": expected_output_observed,
                            "failureDiagnostic": value.failure_diagnostic(),
                            "stagedRuntimeFiles": staged_files,
                            "privateReceiptSha256": format!("{:x}", Sha256::digest(&receipt))
                        }),
                    )
                }
                Err(reason) => (false, serde_json::json!({"error": reason})),
            };
            // No arbitrary native output, profile paths or environment in test
            // reports. Failed fixtures remain local for bounded diagnosis.
            println!("\nCATDESK_NATIVE_COMMAND_CANARY={evidence}");
            let mut write_denied = true;
            if passed && matches!(placement, RuntimePlacement::PrivateProvisioned) {
                let mut check = profile.clone();
                check.id = "runtime-read-only-write-probe".into();
                check.argv = vec![
                    profile.argv[0].clone(),
                    "-I".into(),
                    "-B".into(),
                    "-S".into(),
                    "-c".into(),
                    "import sys\ntry:\n f=open(sys.executable+'.write-probe','wb')\nexcept PermissionError:\n print('READ_ONLY_RUNTIME_CONFIRMED')\nelse:\n f.close()\n raise SystemExit('RUNTIME_WRITE_SCOPE_BROKEN')".into(),
                ];
                let result = run(&root, &[], &check, Duration::from_secs(15)).unwrap();
                let receipt = serde_json::to_vec(&result).unwrap();
                std::fs::write(root.join("runtime-write-probe.private.json"), &receipt).unwrap();
                write_denied = result.success
                    && result.cleanup_proved
                    && result.stdout.trim() == "READ_ONLY_RUNTIME_CONFIRMED"
                    && !PathBuf::from(format!("{}.write-probe", profile.argv[0])).exists();
                println!(
                    "\nCATDESK_RUNTIME_WRITE_PROBE={}",
                    serde_json::json!({
                        "writeDenied": write_denied, "exitCode": result.exit_code,
                        "cleanupProved": result.cleanup_proved,"timedOut": result.timed_out,
                        "privateReceiptSha256": format!("{:x}", Sha256::digest(&receipt))
                    })
                );
            }
            if passed && write_denied {
                if let Some(runtime) = external_runtime {
                    let home = PathBuf::from(
                        std::env::var_os("CATDESK_TEST_PRIVATE_RUNTIME_PARENT").unwrap(),
                    )
                    .canonicalize()
                    .unwrap();
                    assert_eq!(
                        runtime.canonicalize().unwrap().parent(),
                        Some(home.as_path())
                    );
                    assert!(
                        runtime
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with("catdesk-private-runtime-probe-")
                    );
                    std::fs::remove_dir_all(runtime).unwrap();
                }
                std::fs::remove_dir_all(&fixture).unwrap();
            }
            assert!(passed, "NATIVE_COMMAND_CANARY_NOT_QUALIFIED");
            assert!(write_denied, "NATIVE_RUNTIME_READ_ONLY_SCOPE_NOT_QUALIFIED");
        }

        #[test]
        #[ignore = "bounded real Windows sandbox check; requires already configured native CLI"]
        fn native_system_lookup_uses_actual_command_owner() {
            let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
            let expected = system.join("cmd.exe");
            let profile = CommandProfile {
                id: "system-lookup-only".into(),
                argv: vec![
                    system.join("where.exe").to_string_lossy().into_owned(),
                    "cmd.exe".into(),
                ],
                timeout_seconds: 15,
            };
            native_canary(
                profile,
                |output| {
                    output.lines().any(|line| {
                        line.trim()
                            .eq_ignore_ascii_case(&expected.to_string_lossy())
                    })
                },
                RuntimePlacement::Installed,
            );
        }

        fn approved_python_profile() -> (CommandProfile, File) {
            let image = PathBuf::from(
                std::env::var_os("CATDESK_TEST_NATIVE_PYTHON")
                    .expect("APPROVED_TEST_INTERPRETER_REQUIRED"),
            );
            let expected_sha = std::env::var("CATDESK_TEST_NATIVE_PYTHON_SHA256")
                .expect("APPROVED_TEST_INTERPRETER_SHA_REQUIRED");
            assert!(image.is_absolute() && image.file_name().unwrap() == "python.exe");
            let (pin, actual_sha) = retained_image(&image).unwrap();
            assert_eq!(
                actual_sha, expected_sha,
                "APPROVED_TEST_INTERPRETER_CHANGED"
            );
            (
                CommandProfile {
                    id: "python-version-only".into(),
                    argv: vec![
                        image.to_string_lossy().into_owned(),
                        "-I".into(),
                        "-B".into(),
                        "--version".into(),
                    ],
                    timeout_seconds: 15,
                },
                pin,
            )
        }

        #[test]
        #[ignore = "version-only native probe; requires exact approved interpreter path and SHA"]
        fn native_python_version_uses_actual_command_owner() {
            let (profile, _pin) = approved_python_profile();
            native_canary(
                profile,
                |output| output.trim().starts_with("Python 3.12."),
                RuntimePlacement::Installed,
            );
        }

        #[test]
        #[ignore = "version-only disposable runtime copy; requires exact approved interpreter identity"]
        fn native_staged_python_version_uses_actual_command_owner() {
            let (profile, _pin) = approved_python_profile();
            native_canary(
                profile,
                |output| output.trim().starts_with("Python 3.12."),
                RuntimePlacement::Workspace,
            );
        }

        #[test]
        #[ignore = "disposable runtime read/write boundary; requires exact approved interpreter identity"]
        fn native_private_runtime_without_access_is_refused() {
            let (profile, _pin) = approved_python_profile();
            native_canary(
                profile,
                |output| output.trim().starts_with("Python 3.12."),
                RuntimePlacement::PrivateDenied,
            );
        }

        #[test]
        #[ignore = "disposable preprovisioned read-only runtime; existing sandbox readers only"]
        fn native_private_runtime_with_access_is_read_only() {
            let (profile, _pin) = approved_python_profile();
            native_canary(
                profile,
                |output| output.trim().starts_with("Python 3.12."),
                RuntimePlacement::PrivateProvisioned,
            );
        }

        #[test]
        fn command_policy_has_one_write_root_and_native_metadata_read_only() {
            let root = Path::new("C:/fixture/task");
            let text = format!(
                "policy={}",
                policy(
                    root,
                    &[PathBuf::from("C:/fixture/native")],
                    Some(Path::new("C:/fixture/auth"))
                )
                .unwrap()
            );
            let value: toml::Value = toml::from_str(&text).unwrap();
            let rules = value["policy"]["filesystem"].as_table().unwrap();
            assert_eq!(
                rules
                    .values()
                    .filter(|v| v.as_str() == Some("write"))
                    .count(),
                1
            );
            assert_eq!(rules["C:/fixture/task"].as_str(), Some("write"));
            assert_eq!(rules[":root"].as_str(), Some("read"));
            assert!(!rules.values().any(|v| v.as_str() == Some("deny")));
            assert_eq!(value["policy"]["network"]["enabled"].as_bool(), Some(false));
            assert!(policy(root, &[PathBuf::from("C:/fixture")], None).is_err());
        }
        #[test]
        fn command_output_is_drained_with_bounded_retention() {
            let output = pump(std::io::Cursor::new(vec![b'x'; CAPTURE * 3]));
            let (text, truncated) = output
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            assert_eq!(text.len(), CAPTURE);
            assert!(truncated);
        }
    }
}

pub(crate) fn run(
    root: &Path,
    deny: &[PathBuf],
    profile: &CommandProfile,
    budget: Duration,
) -> Result<CommandResult, String> {
    #[cfg(windows)]
    {
        windows::run(root, deny, profile, budget)
    }
    #[cfg(not(windows))]
    {
        let _ = (root, deny, profile, budget);
        Err("PROJECT_COMMAND_WINDOWS_SANDBOX_REQUIRED".into())
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    fn result() -> CommandResult {
        CommandResult {
            profile_id: "fixed-check".into(),
            exit_code: Some(1),
            timed_out: false,
            cleanup_proved: true,
            success: false,
            stdout: String::new(),
            stderr: "windows sandbox failed: runner failed during SpawnChild: CreateProcessAsUserW failed: 5 (Access is denied.) | cwd=PRIVATE_PATH | cmd=PRIVATE_ARGUMENT".into(),
            stdout_truncated: false,
            stderr_truncated: false,
            native_cli_sha256: String::new(),
            executable_sha256: String::new(),
            profile_sha256: String::new(),
            elapsed_ms: 1,
            preparation_ms: 0,
            model_calls: 0,
            native_session_action: "NONE",
            write_scope: "ASSIGNED_WORKTREE_EXCEPT_CONTROL_METADATA",
            read_scope: "EXISTING_NATIVE_ROOT_READ_POSTURE_FILE_TOOLS_REMAIN_PROJECT_SCOPED",
            task_completion_verified: false,
        }
    }

    #[test]
    fn reported_spawn_denial_is_private_and_never_replay_authority() {
        let diagnostic = result().failure_diagnostic().unwrap();
        assert_eq!(diagnostic["layer"], "NATIVE_SANDBOX");
        assert_eq!(diagnostic["reason"], "SPAWN_ACCESS_DENIED_REPORTED");
        assert_eq!(diagnostic["basis"], "REPORTED_PROCESS_OUTPUT");
        assert_eq!(diagnostic["targetExecution"], "UNVERIFIED");
        assert_eq!(diagnostic["retryAuthorized"], false);
        assert!(!diagnostic.to_string().contains("PRIVATE_"));
        // Even spoofed process output must not enter the trusted prelaunch list.
        assert!(!failed_before_launch("SPAWN_ACCESS_DENIED_REPORTED"));
        assert!(!failed_before_launch(&result().stderr));
    }

    #[test]
    fn uncertain_or_successful_results_do_not_get_failure_hints() {
        for case in 0..6 {
            let mut value = result();
            match case {
                0 => value.success = true,
                1 => value.exit_code = Some(0),
                2 => value.exit_code = None,
                3 => value.timed_out = true,
                4 => value.cleanup_proved = false,
                5 => value.stderr_truncated = true,
                _ => unreachable!(),
            }
            assert!(value.failure_diagnostic().is_none(), "case {case}");
        }
    }

    #[test]
    fn unrelated_errors_and_partial_native_signatures_are_not_classified() {
        for text in [
            "Access is denied.",
            "CreateProcessAsUserW failed: 5 (Access is denied.)",
            "windows sandbox failed: runner failed during SpawnChild: CreateProcessAsUserW failed: 50 (Access is denied.)",
            "windows sandbox failed: runner failed during SpawnChild: CreateProcessAsUserW failed: 5 (Access is denied.)unexpected",
            "quoted: windows sandbox failed: runner failed during SpawnChild: CreateProcessAsUserW failed: 5 (Access is denied.)",
        ] {
            let mut value = result();
            value.stderr = text.into();
            assert!(value.failure_diagnostic().is_none());
        }
    }
}
