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
