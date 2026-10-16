//! Current-user Codex CLI access for an explicitly registered project.
//!
//! This local operator route is separate from CatDesk's autonomous worker and
//! independent WakeHost. It never reads Codex credentials or launches through
//! a shell. The native CLI owns its normal session and approval behavior.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::delegated::autonomy_projects::{
    AutonomousProjectV1, resolve_registered_project_from_control_root,
};
use crate::delegated::autonomy_runtime::{operator_codex_home, resolve_codex_executable};

const MAX_TASK_BYTES: usize = 24 * 1024;
const MAX_PROFILE_BYTES: usize = 128;
const CODEX_PROBE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexScope {
    project_id: String,
    workspace: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperatorCodexAction {
    Status(CodexScope),
    Run {
        scope: CodexScope,
        task: String,
        model: Option<String>,
        effort: Option<String>,
    },
    Resume {
        scope: CodexScope,
        model: Option<String>,
        effort: Option<String>,
    },
    /// Explicit native picker across the current user's sessions. This has
    /// no project claim or CatDesk workspace-writer lock.
    GlobalResumeAll,
}

/// `operator codex` is a local entry point. Project-scoped actions name both
/// the registered project and its approved workspace. The sole exception is
/// the explicit, manual `resume --all` native picker; it claims no project.
pub(crate) fn parse_action(args: &[String]) -> Result<OperatorCodexAction, String> {
    let verb = args.get(2).map(String::as_str).ok_or_else(|| {
        "CODEX_USAGE: expected status, run, or resume with --project-id and --workspace".to_string()
    })?;
    if !matches!(verb, "status" | "run" | "resume") {
        return Err("CODEX_USAGE: expected status, run, or resume".into());
    }
    let mut project_id = None;
    let mut workspace = None;
    let mut task = None;
    let mut model = None;
    let mut effort = None;
    let mut all = false;
    let mut index = 3;
    while index < args.len() {
        let flag = args[index].as_str();
        if flag == "--all" {
            if all {
                return Err("CODEX_USAGE: --all may only be supplied once".into());
            }
            all = true;
            index += 1;
            continue;
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| "CODEX_USAGE: an option requires a value".to_string())?;
        if value.is_empty() || value.len() > MAX_TASK_BYTES {
            return Err("CODEX_USAGE: an option value is empty or too large".into());
        }
        let slot = match flag {
            "--project-id" => &mut project_id,
            "--workspace" => &mut workspace,
            "--task" => &mut task,
            "--model" => &mut model,
            "--effort" => &mut effort,
            _ => return Err("CODEX_USAGE: unsupported option".into()),
        };
        if slot.replace(value.clone()).is_some() {
            return Err(format!("CODEX_USAGE: {flag} may only be supplied once"));
        }
        index += 2;
    }
    if verb == "resume" && all {
        return if project_id.is_none()
            && workspace.is_none()
            && task.is_none()
            && model.is_none()
            && effort.is_none()
        {
            Ok(OperatorCodexAction::GlobalResumeAll)
        } else {
            Err("CODEX_USAGE: resume --all is a global manual picker and accepts no project, workspace, task, or profile options".into())
        };
    }
    let scope = CodexScope {
        project_id: project_id.ok_or("CODEX_USAGE: --project-id is required")?,
        workspace: PathBuf::from(workspace.ok_or("CODEX_USAGE: --workspace is required")?),
    };
    if !scope.workspace.is_absolute() {
        return Err("CODEX_SCOPE: --workspace must be an absolute path".into());
    }
    if model.as_deref().is_some_and(|value| !valid_model(value)) {
        return Err("CODEX_USAGE: --model has an invalid identifier".into());
    }
    if effort.as_deref().is_some_and(|value| !valid_effort(value)) {
        return Err(
            "CODEX_USAGE: --effort must be minimal, low, medium, high, xhigh, max, or ultra".into(),
        );
    }
    match verb {
        "status" if task.is_none() && model.is_none() && effort.is_none() && !all => {
            Ok(OperatorCodexAction::Status(scope))
        }
        "run" if !all => {
            let task = task.ok_or("CODEX_USAGE: run requires --task")?;
            if task.trim().is_empty() || task.len() > MAX_TASK_BYTES || task.contains('\0') {
                return Err("CODEX_USAGE: --task is empty or invalid".into());
            }
            Ok(OperatorCodexAction::Run {
                scope,
                task,
                model,
                effort,
            })
        }
        "resume" if task.is_none() => Ok(OperatorCodexAction::Resume {
            scope,
            model,
            effort,
        }),
        _ => Err("CODEX_USAGE: options do not match the requested action".into()),
    }
}

fn valid_model(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PROFILE_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_effort(value: &str) -> bool {
    matches!(
        value,
        "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
    )
}

fn registered_project(
    catdesk_workspace: &Path,
    scope: &CodexScope,
) -> Result<AutonomousProjectV1, String> {
    let selected = scope
        .workspace
        .canonicalize()
        .map_err(|_| "CODEX_SCOPE: selected project workspace is unavailable".to_string())?;
    let resolved = resolve_registered_project_from_control_root(
        catdesk_workspace,
        &scope.project_id,
    )
    .map_err(|_| {
        "CODEX_SCOPE: CatDesk control Git identity or selected project registration/Git origin is unavailable"
            .to_string()
    })?;
    if selected != resolved.project.workspace {
        return Err("CODEX_SCOPE: selected workspace differs from the registered project".into());
    }
    Ok(resolved.project)
}

fn exact_thread(project: &AutonomousProjectV1) -> Result<&str, String> {
    let thread = project.codex_thread_id.as_deref().ok_or_else(|| {
        "CODEX_THREAD: no thread is bound; use the existing project thread adoption action"
            .to_string()
    })?;
    if !uuid::Uuid::parse_str(thread).is_ok_and(|id| id.to_string() == thread) {
        return Err("CODEX_THREAD: bound thread ID is not a canonical UUID".into());
    }
    Ok(thread)
}

fn resolve_native_cli() -> Result<(PathBuf, Option<PathBuf>), String> {
    let executable = resolve_codex_executable().map_err(|_| {
        "CODEX_UNAVAILABLE: no direct Codex executable found; check PATH or CATDESK_CODEX_CLI_EXECUTABLE"
            .to_string()
    })?;
    let codex_home = operator_codex_home().map_err(|_| {
        "CODEX_HOME_UNAVAILABLE: CATDESK_CODEX_HOME must name an existing directory".to_string()
    })?;
    Ok((executable, codex_home))
}

fn codex_command(executable: &Path, workspace: &Path, codex_home: Option<&Path>) -> Command {
    let mut command = Command::new(executable);
    command.current_dir(workspace);
    // CatDesk passes the user's authenticated Codex home by reference. It
    // neither opens auth files nor passes API keys to the child.
    command
        .env_remove("CODEX_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("CONTROL_PLANE_API_KEY");
    if let Some(home) = codex_home {
        command.env("CODEX_HOME", home);
    }
    command
}

fn invocation_args(
    action: &OperatorCodexAction,
    project: &AutonomousProjectV1,
) -> Result<Vec<String>, String> {
    // Noninteractive `exec` reports an effective approval policy of `never`
    // on the installed CLI. Make that explicit; workspace-write still bounds
    // this operator-approved task. Interactive resume can request approvals.
    let approval = if matches!(action, OperatorCodexAction::Run { .. }) {
        "never"
    } else {
        "on-request"
    };
    let mut args = vec![
        "--cd".into(),
        project.workspace.to_string_lossy().into_owned(),
        "--sandbox".into(),
        "workspace-write".into(),
        "--ask-for-approval".into(),
        approval.into(),
    ];
    match action {
        OperatorCodexAction::Status(_) => return Ok(Vec::new()),
        OperatorCodexAction::Run { model, effort, .. } => {
            args.extend(["exec".into(), "resume".into()]);
            // An exec resume turn does not inherit the preceding turn's
            // sandbox. The global flag and resume-local override both assert
            // this exact workspace policy.
            args.extend(["-c".into(), "sandbox_mode=\"workspace-write\"".into()]);
            append_profile(&mut args, model.as_deref(), effort.as_deref());
            args.push(exact_thread(project)?.into());
            args.push("-".into());
        }
        OperatorCodexAction::Resume { model, effort, .. } => {
            args.push("resume".into());
            args.extend(["-c".into(), "sandbox_mode=\"workspace-write\"".into()]);
            append_profile(&mut args, model.as_deref(), effort.as_deref());
            args.push(exact_thread(project)?.into());
        }
        OperatorCodexAction::GlobalResumeAll => {
            return Err("CODEX_SCOPE: global picker has no project-scoped invocation".into());
        }
    }
    Ok(args)
}

fn append_profile(args: &mut Vec<String>, model: Option<&str>, effort: Option<&str>) {
    if let Some(model) = model {
        args.extend(["--model".into(), model.into()]);
    }
    if let Some(effort) = effort {
        args.extend(["-c".into(), format!("model_reasoning_effort={effort}")]);
    }
}

fn operator_run_lock(
    catdesk_workspace: &Path,
    project: &AutonomousProjectV1,
) -> Result<File, String> {
    let mut hasher = Sha256::new();
    hasher.update(project.workspace.to_string_lossy().as_bytes());
    let path = catdesk_workspace
        .join(".catdesk")
        .join("projects")
        .join(format!("operator-codex-{:x}.lock", hasher.finalize()));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|_| "CODEX_LOCK: project operator lock is unavailable".to_string())?;
    file.try_lock().map_err(|_| {
        "CODEX_LOCK: another CatDesk operator Codex command is active for this project".to_string()
    })?;
    Ok(file)
}

async fn bounded_probe(mut command: Command, args: &[&str]) -> Result<(bool, String), String> {
    command.args(args).stdin(Stdio::null()).kill_on_drop(true);
    let output = tokio::time::timeout(CODEX_PROBE_TIMEOUT, command.output())
        .await
        .map_err(|_| "CODEX_UNAVAILABLE: native Codex status timed out".to_string())?
        .map_err(|_| "CODEX_UNAVAILABLE: native Codex status could not start".to_string())?;
    let body = String::from_utf8_lossy(&output.stdout);
    let version = if args == ["--version"] {
        body.trim().chars().take(128).collect()
    } else {
        String::new()
    };
    Ok((output.status.success(), version))
}

pub(crate) async fn execute_action(
    action: OperatorCodexAction,
    catdesk_workspace: &Path,
) -> Result<Value, String> {
    if matches!(action, OperatorCodexAction::GlobalResumeAll) {
        return execute_global_resume_picker(catdesk_workspace).await;
    }
    let scope = match &action {
        OperatorCodexAction::Status(scope)
        | OperatorCodexAction::Run { scope, .. }
        | OperatorCodexAction::Resume { scope, .. } => scope,
        OperatorCodexAction::GlobalResumeAll => unreachable!(),
    };
    let project = registered_project(catdesk_workspace, scope)?;
    let (executable, codex_home) = resolve_native_cli()?;
    if let OperatorCodexAction::Status(_) = &action {
        let (version_ok, version) = bounded_probe(
            codex_command(&executable, &project.workspace, codex_home.as_deref()),
            &["--version"],
        )
        .await?;
        if !version_ok || !version.starts_with("codex-cli ") {
            return Err(
                "CODEX_UNAVAILABLE: native executable did not report a Codex CLI version".into(),
            );
        }
        let (authenticated, _) = bounded_probe(
            codex_command(&executable, &project.workspace, codex_home.as_deref()),
            &["login", "status"],
        )
        .await?;
        return Ok(json!({
            "action":"codex_status", "status":"success", "projectId":project.project_id,
            "workspace":project.workspace, "threadId":project.codex_thread_id,
            "executable":executable, "version":version, "authenticated":authenticated,
            "route":"current_user_native_cli",
        }));
    }
    let args = invocation_args(&action, &project)?;
    let _lock = operator_run_lock(catdesk_workspace, &project)?;
    let mut command = codex_command(&executable, &project.workspace, codex_home.as_deref());
    command
        .args(&args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // If the CatDesk operator action is cancelled, dropping this child must
    // terminate the direct Codex CLI process before its project lock drops.
    command.kill_on_drop(true);
    if matches!(action, OperatorCodexAction::Run { .. }) {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::inherit());
    }
    let mut child = command
        .spawn()
        .map_err(|_| "CODEX_UNAVAILABLE: native Codex process could not start".to_string())?;
    if let OperatorCodexAction::Run { task, .. } = &action {
        let Some(mut stdin) = child.stdin.take() else {
            return Err("CODEX_IO: native Codex prompt pipe is unavailable".into());
        };
        if stdin.write_all(task.as_bytes()).await.is_err() || stdin.shutdown().await.is_err() {
            let _ = child.kill().await;
            return Err("CODEX_IO: native Codex did not accept the task prompt".into());
        }
    }
    let status = child
        .wait()
        .await
        .map_err(|_| "CODEX_IO: native Codex exit status is unavailable".to_string())?;
    if !status.success() {
        return Err(format!(
            "CODEX_EXIT: native Codex returned {}; inspect its terminal output for auth, quota, approval, or task errors",
            status
                .code()
                .map_or("signal".into(), |code| code.to_string())
        ));
    }
    let (verb, model, effort) = match &action {
        OperatorCodexAction::Run { model, effort, .. } => ("run", model, effort),
        OperatorCodexAction::Resume { model, effort, .. } => ("resume", model, effort),
        OperatorCodexAction::Status(_) => unreachable!(),
        OperatorCodexAction::GlobalResumeAll => unreachable!(),
    };
    Ok(json!({
        "action":format!("codex_{verb}"), "status":"native_cli_exited_zero", "exitCode":0,
        "projectId":project.project_id, "workspace":project.workspace,
        "threadId":project.codex_thread_id.as_deref(),
        "requestedModel":model, "requestedEffort":effort,
        "requestedSandbox":"workspace-write",
        "requestedApprovalPolicy":if verb == "run" { "never" } else { "on-request" },
        "executable":executable,
    }))
}

async fn execute_global_resume_picker(catdesk_workspace: &Path) -> Result<Value, String> {
    // Verify the caller is in the registered CatDesk control Git root, then
    // hand over to the user's native picker. No selected project, task thread,
    // or project lock is claimed by this manual cross-session route.
    let control = resolve_registered_project_from_control_root(catdesk_workspace, "catdesk")
        .map_err(|_| "CODEX_SCOPE: CatDesk control Git identity is unavailable".to_string())?;
    let (executable, codex_home) = resolve_native_cli()?;
    let mut command = codex_command(
        &executable,
        &control.control_workspace,
        codex_home.as_deref(),
    );
    command
        .args([
            "--sandbox",
            "workspace-write",
            "--ask-for-approval",
            "on-request",
            "resume",
            "--all",
            "--include-non-interactive",
            "-c",
            "sandbox_mode=\"workspace-write\"",
        ])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // The picker is manual and global, but cancellation still owns its direct
    // CLI child. Codex's separately owned shared daemon is untouched.
    command.kill_on_drop(true);
    let status = command
        .spawn()
        .map_err(|_| "CODEX_UNAVAILABLE: native Codex picker could not start".to_string())?
        .wait()
        .await
        .map_err(|_| "CODEX_IO: native Codex picker exit status is unavailable".to_string())?;
    if !status.success() {
        return Err(format!(
            "CODEX_EXIT: native Codex picker returned {}; inspect its terminal output",
            status
                .code()
                .map_or("signal".into(), |code| code.to_string())
        ));
    }
    Ok(json!({
        "action":"codex_global_resume_picker", "status":"native_cli_exited_zero", "exitCode":0,
        "projectClaim":false, "threadId":null, "requestedSandbox":"workspace-write",
        "requestedApprovalPolicy":"on-request", "executable":executable,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegated::autonomy_projects::AutonomousProjectRegistryStoreV1;
    use std::fs;
    use std::process::Command as StdCommand;

    fn parse(items: &[&str]) -> Result<OperatorCodexAction, String> {
        parse_action(&items.iter().map(|item| (*item).into()).collect::<Vec<_>>())
    }

    #[test]
    fn parser_requires_explicit_project_workspace_and_rejects_bypass_flags() {
        let base = [
            "operator",
            "codex",
            "run",
            "--project-id",
            "catdesk",
            "--workspace",
            "C:/work/catdesk",
        ];
        assert!(parse(&base).is_err());
        assert!(parse(&[&base[..], &["--task", "edit the README"]].concat()).is_ok());
        assert!(
            parse(
                &[
                    &base[..],
                    &["--task", "edit", "--sandbox", "danger-full-access"]
                ]
                .concat()
            )
            .is_err()
        );
        assert!(parse(&[&base[..], &["--task", "edit", "--task", "other"]].concat()).is_err());
        assert!(parse(&[&base[..], &["--task", "edit", "--all"]].concat()).is_err());
        assert!(
            parse(&[
                "operator",
                "codex",
                "status",
                "--project-id",
                "catdesk",
                "--workspace",
                "C:/work/catdesk",
                "--all",
            ])
            .is_err()
        );
        assert!(parse(&[&base[..], &["--task", "edit", "--model", "gpt;evil"]].concat()).is_err());
        assert!(
            parse(&[&base[..], &["--task", "edit", "--effort", "impossible"]].concat()).is_err()
        );
        assert!(
            parse(&[
                "operator",
                "codex",
                "resume",
                "--project-id",
                "catdesk",
                "--workspace",
                "relative",
                "--all"
            ])
            .is_err()
        );
    }

    #[test]
    fn run_uses_exact_bound_thread_and_keeps_task_off_native_argv() {
        let thread = "019fe33e-5c82-7540-a41c-9ae497527e57";
        let project = AutonomousProjectV1 {
            project_id: "catdesk".into(),
            workspace: PathBuf::from("C:/work/catdesk"),
            git_identity: "fixture".into(),
            verification_profile: "rust_full".into(),
            codex_thread_id: Some(thread.into()),
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
        };
        let action = parse(&[
            "operator",
            "codex",
            "run",
            "--project-id",
            "catdesk",
            "--workspace",
            "C:/work/catdesk",
            "--task",
            "private task text",
            "--model",
            "gpt-6.1-sol",
            "--effort",
            "medium",
        ])
        .expect("parse");
        let args = invocation_args(&action, &project).expect("args");
        assert_eq!(
            &args[2..8],
            &[
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "never",
                "exec",
                "resume"
            ]
        );
        assert_eq!(args[0], "--cd");
        assert_eq!(args[1], "C:/work/catdesk");
        assert!(args.contains(&"sandbox_mode=\"workspace-write\"".to_string()));
        assert_eq!(args[args.len() - 2], thread);
        assert!(args.contains(&"gpt-6.1-sol".to_string()));
        assert!(args.contains(&"model_reasoning_effort=medium".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("-"));
        assert!(!args.iter().any(|arg| arg.contains("private task")));
    }

    #[test]
    fn global_picker_has_no_project_claim_and_exact_resume_stays_bound() {
        let project = AutonomousProjectV1 {
            project_id: "catdesk".into(),
            workspace: PathBuf::from("C:/work/catdesk"),
            git_identity: "fixture".into(),
            verification_profile: "rust_full".into(),
            codex_thread_id: Some("019fe33e-5c82-7540-a41c-9ae497527e57".into()),
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
        };
        let exact = parse(&[
            "operator",
            "codex",
            "resume",
            "--project-id",
            "catdesk",
            "--workspace",
            "C:/work/catdesk",
        ])
        .expect("exact");
        let all = parse(&["operator", "codex", "resume", "--all"]).expect("all");
        assert_eq!(all, OperatorCodexAction::GlobalResumeAll);
        assert!(
            parse(&[
                "operator",
                "codex",
                "resume",
                "--all",
                "--project-id",
                "catdesk",
                "--workspace",
                "C:/work/catdesk",
            ])
            .is_err()
        );
        assert!(
            invocation_args(&exact, &project)
                .expect("args")
                .contains(&project.codex_thread_id.clone().unwrap())
        );
        assert!(invocation_args(&all, &project).is_err());
        assert!(
            !invocation_args(&exact, &project)
                .expect("args")
                .contains(&"--all".to_string())
        );
    }

    fn fixture_git_root(root: &Path, origin: &str) {
        fs::create_dir_all(root).expect("fixture root");
        for args in [
            &["init", "-q"][..],
            &["remote", "add", "origin", origin][..],
        ] {
            assert!(
                StdCommand::new("git")
                    .arg("-C")
                    .arg(root)
                    .args(args)
                    .status()
                    .expect("Git fixture command")
                    .success()
            );
        }
    }

    #[test]
    fn registered_project_cannot_be_selected_with_sibling_workspace() {
        let temp_root = std::env::temp_dir().canonicalize().expect("temp root");
        let root = temp_root.join(format!("catdesk-operator-codex-{}", uuid::Uuid::new_v4()));
        let catdesk = root.join("catdesk");
        let sibling = root.join("sibling");
        fixture_git_root(&catdesk, "https://example.test/catdesk.git");
        fixture_git_root(&sibling, "https://example.test/sibling.git");
        fs::create_dir_all(catdesk.join(".catdesk")).expect("control state");
        let store = AutonomousProjectRegistryStoreV1::open(catdesk.join(".catdesk/projects"))
            .expect("registry");
        store.initialize(2, 1).expect("initialize");
        for (id, workspace) in [("catdesk", &catdesk), ("sibling", &sibling)] {
            store
                .register_project(AutonomousProjectV1 {
                    project_id: id.into(),
                    workspace: workspace.clone(),
                    git_identity: format!("https://example.test/{id}.git"),
                    verification_profile: "rust_full".into(),
                    codex_thread_id: None,
                    chatgpt_target_url: None,
                    chatgpt_target_sha256: None,
                })
                .expect("register");
        }
        let good = CodexScope {
            project_id: "catdesk".into(),
            workspace: catdesk.clone(),
        };
        let cross = CodexScope {
            project_id: "catdesk".into(),
            workspace: sibling,
        };
        assert_eq!(
            registered_project(&catdesk, &good)
                .expect("same project")
                .project_id,
            "catdesk"
        );
        assert!(registered_project(&catdesk, &cross).is_err());
        let sibling_scope = CodexScope {
            project_id: "sibling".into(),
            workspace: cross.workspace.clone(),
        };
        assert_eq!(
            registered_project(&catdesk, &sibling_scope)
                .expect("registered sibling")
                .project_id,
            "sibling"
        );
        assert!(
            StdCommand::new("git")
                .arg("-C")
                .arg(&cross.workspace)
                .args([
                    "remote",
                    "set-url",
                    "origin",
                    "https://example.test/drift.git"
                ])
                .status()
                .expect("drift sibling origin")
                .success()
        );
        assert!(registered_project(&catdesk, &sibling_scope).is_err());
        assert!(
            StdCommand::new("git")
                .arg("-C")
                .arg(&catdesk)
                .args([
                    "remote",
                    "set-url",
                    "origin",
                    "https://example.test/drift.git"
                ])
                .status()
                .expect("drift control origin")
                .success()
        );
        assert!(registered_project(&catdesk, &good).is_err());
        let resolved = root.canonicalize().expect("fixture root");
        assert!(resolved.starts_with(&temp_root));
        assert!(
            resolved
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("catdesk-operator-codex-")
        );
        fs::remove_dir_all(resolved).expect("cleanup fixture");
    }
}
