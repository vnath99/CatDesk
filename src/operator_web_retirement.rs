//! Recoverable removal of a quiescent Web project from the existing registries.
//! Workspace files, completed work and conversation history are never deleted.

use crate::{
    delegated::{
        autonomy_projects::{AutonomousProjectRegistryStoreV1, AutonomousProjectV1},
        autonomy_runtime::acquire_project_writer_lock,
        runtime::RuntimeError,
    },
    web_work_state::WorkLedger,
};
use catdesk_wake::{
    protocol::Target,
    store::{Store, now, read},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs, path::Path};

const MAX_RETIREMENT_INTENT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Intent {
    schema_version: u32,
    retirement_id: String,
    project: AutonomousProjectV1,
    expected_target: Option<Target>,
    retired_utc: u64,
}

fn target_for(
    project: &AutonomousProjectV1,
    generation: u64,
    digest: Option<&str>,
) -> Result<Option<Target>, String> {
    match (
        &project.chatgpt_target_url,
        &project.chatgpt_target_sha256,
        digest,
    ) {
        (None, None, None) if generation == 0 => Ok(None),
        (Some(url), Some(saved), Some(expected)) if saved == expected && generation > 0 => {
            let target = Target::new(generation, url.clone())?;
            if target.digest != expected {
                return Err("WEB_RETIREMENT_TARGET_CHANGED".into());
            }
            Ok(Some(target))
        }
        _ => Err("WEB_RETIREMENT_TARGET_CHANGED".into()),
    }
}

fn eligible(project: &AutonomousProjectV1) -> Result<(), String> {
    if project.project_id == "catdesk"
        || project.codex_thread_id.is_some()
        || project.verification_profile != "web_only_v1"
    {
        return Err("WEB_RETIREMENT_WEB_ONLY_REQUIRED".into());
    }
    Ok(())
}

fn idle(workspace: &Path, project: &str) -> Result<(), String> {
    let status = WorkLedger::status(workspace, project);
    if !matches!(
        status["status"].as_str(),
        Some("UNOBSERVED" | "DURABLE_SNAPSHOT")
    ) || !status["active"].is_null()
    {
        return Err("WEB_RETIREMENT_WORK_PENDING".into());
    }
    // Older assigned-task journals have no durable clean-close proof. A
    // released process lock alone cannot prove an isolated writer finished.
    // Preserve that evidence for explicit task recovery instead of retiring
    // its project. Only this project's task directory can block retirement.
    let tasks = workspace.join(".catdesk/project-mcp-tasks").join(project);
    if tasks
        .try_exists()
        .map_err(|_| "WEB_RETIREMENT_TASK_RECOVERY_REQUIRED")?
    {
        super::rollover::plain(&tasks, true)?;
        if fs::read_dir(&tasks)
            .map_err(|_| "WEB_RETIREMENT_TASK_RECOVERY_REQUIRED")?
            .next()
            .is_some()
        {
            return Err("WEB_RETIREMENT_TASK_RECOVERY_REQUIRED".into());
        }
    }
    Ok(())
}

fn verify_wake(
    wake: &Store,
    project: &str,
    id: &str,
    expected: Option<&Target>,
) -> Result<bool, String> {
    let config = wake.config()?;
    if let Some(retired) = config.retired_targets.get(project) {
        if retired.retirement_id == id && retired.last_target.as_ref() == expected {
            return Ok(true);
        }
    } else if config.targets.get(project) == expected {
        return Ok(false);
    }
    Err("WEB_RETIREMENT_WAKE_CHANGED".into())
}

/// One stable operator request; a Wake-first crash resumes the same intent.
pub(super) fn retire(
    workspace: &Path,
    project: &str,
    retirement_id: &str,
    expected_generation: u64,
    expected_digest: Option<&str>,
) -> Result<Value, String> {
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "WEB_RETIREMENT_CONTROL_UNAVAILABLE")?;
    let root = crate::wake_protocol_client::wake_root_for_workspace(&workspace)?;
    let wake = Store::open(&root)?;
    retire_with_store(
        &workspace,
        project,
        retirement_id,
        expected_generation,
        expected_digest,
        &wake,
        &|target| {
            catdesk_wake::development_runtime::validate_selected_target_isolation(
                &workspace,
                wake.root(),
                &target.url,
            )
        },
        || Ok(()),
    )
}

#[allow(clippy::too_many_arguments)]
fn retire_with_store(
    workspace: &Path,
    project: &str,
    retirement_id: &str,
    expected_generation: u64,
    expected_digest: Option<&str>,
    wake: &Store,
    verify_selection: &dyn Fn(&Target) -> Result<(), String>,
    after_wake: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    if !catdesk_wake::protocol::valid_id(project)
        || !catdesk_wake::protocol::valid_id(retirement_id)
        || expected_digest.is_some_and(|value| {
            value.len() != 64
                || !value
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        })
    {
        return Err("WEB_RETIREMENT_REQUEST_INVALID".into());
    }
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "WEB_RETIREMENT_CONTROL_UNAVAILABLE")?;
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
            .map_err(|_| "WEB_RETIREMENT_REGISTRY_UNAVAILABLE")?;
    let snapshot = registry
        .load_registry()
        .map_err(|_| "WEB_RETIREMENT_REGISTRY_UNAVAILABLE")?;
    let controller = snapshot
        .projects
        .iter()
        .find(|p| p.project_id == "catdesk")
        .ok_or("WEB_RETIREMENT_CONTROL_UNAVAILABLE")?;
    if controller.workspace != workspace {
        return Err("WEB_RETIREMENT_CONTROL_CHANGED".into());
    }
    let previous = registry
        .retired_project(project)
        .map_err(|_| "WEB_RETIREMENT_REGISTRY_UNAVAILABLE")?;
    let observed = snapshot
        .projects
        .iter()
        .find(|p| p.project_id == project)
        .cloned()
        .or_else(|| previous.as_ref().map(|p| p.project.clone()))
        .ok_or("WEB_RETIREMENT_PROJECT_UNAVAILABLE")?;
    eligible(&observed)?;
    let expected_target = target_for(&observed, expected_generation, expected_digest)?;
    if let Some(target) = &expected_target {
        verify_selection(target)?;
    }
    verify_wake(wake, project, retirement_id, expected_target.as_ref())?;

    let journal_dir = workspace.join(".catdesk/projects/retirements");
    let path = journal_dir.join(format!("{project}.json"));
    if journal_dir
        .try_exists()
        .map_err(|_| "WEB_RETIREMENT_JOURNAL_UNAVAILABLE")?
    {
        super::rollover::plain(&journal_dir, true)?;
    }
    let saved = if path
        .try_exists()
        .map_err(|_| "WEB_RETIREMENT_JOURNAL_UNAVAILABLE")?
    {
        super::rollover::plain(&path, false)?;
        if fs::metadata(&path)
            .map_err(|_| "WEB_RETIREMENT_JOURNAL_UNAVAILABLE")?
            .len()
            > MAX_RETIREMENT_INTENT_BYTES
        {
            return Err("WEB_RETIREMENT_JOURNAL_INVALID".into());
        }
        let saved: Intent = read(&path).map_err(|_| "WEB_RETIREMENT_JOURNAL_INVALID")?;
        if saved.schema_version != 1
            || saved.retirement_id != retirement_id
            || saved.project != observed
            || saved.expected_target != expected_target
            || saved.retired_utc == 0
        {
            return Err("WEB_RETIREMENT_INTENT_CHANGED".into());
        }
        Some(saved)
    } else {
        None
    };
    if let Some(previous) = previous {
        if !verify_wake(wake, project, retirement_id, expected_target.as_ref())? {
            return Err("WEB_RETIREMENT_WAKE_CHANGED".into());
        }
        let saved = saved.ok_or("WEB_RETIREMENT_JOURNAL_REQUIRED")?;
        if previous.retirement_id != retirement_id
            || previous.retired_utc != saved.retired_utc
            || previous.project != saved.project
        {
            return Err("WEB_RETIREMENT_INTENT_CHANGED".into());
        }
        return Ok(json!({"action":"web_project_retire","projectId":project,
            "retirementId":retirement_id,"status":"RETIRED","replayed":true,
            "workspacePreserved":true,"historyPreserved":true,"browserAction":"NONE"}));
    }

    // Share the fence used by Web requests, rollover and assigned Web tasks.
    // It does not serialize other projects or require stopping their host.
    let _project = super::rollover::acquire_web_retirement_lease(
        &workspace,
        project,
        wake,
        expected_target.as_ref(),
    )?;
    let _writer = acquire_project_writer_lock(&observed.workspace)
        .map_err(|_| "WEB_RETIREMENT_WRITER_BUSY_OR_UNRESOLVED")?;
    idle(&workspace, project)?;
    let intent = saved.unwrap_or(Intent {
        schema_version: 1,
        retirement_id: retirement_id.into(),
        project: observed.clone(),
        expected_target: expected_target.clone(),
        retired_utc: now(),
    });
    // Use the same bound for initial publication and recovery. Reject before
    // changing Wake authority if a valid registry row cannot be journaled.
    if serde_json::to_vec(&intent)
        .map_err(|_| "WEB_RETIREMENT_JOURNAL_INVALID")?
        .len() as u64
        + 1
        > MAX_RETIREMENT_INTENT_BYTES
    {
        return Err("WEB_RETIREMENT_JOURNAL_TOO_LARGE".into());
    }
    let (retired, replayed) = registry
        .retire_web_project_after(&observed, retirement_id, intent.retired_utc, || {
            idle(&workspace, project).map_err(RuntimeError::Validation)?;
            verify_wake(wake, project, retirement_id, expected_target.as_ref())
                .map_err(RuntimeError::Validation)?;
            super::rollover::directory(&journal_dir).map_err(RuntimeError::Validation)?;
            if !path.try_exists().map_err(|_| {
                RuntimeError::Validation("WEB_RETIREMENT_JOURNAL_UNAVAILABLE".into())
            })? {
                super::rollover::create_private_once_bounded(
                    &path,
                    &intent,
                    MAX_RETIREMENT_INTENT_BYTES as usize,
                )
                .map_err(RuntimeError::Validation)?;
            }
            let reread: Intent = read(&path).map_err(RuntimeError::Validation)?;
            if reread != intent {
                return Err(RuntimeError::Validation(
                    "WEB_RETIREMENT_JOURNAL_CHANGED".into(),
                ));
            }
            wake.retire_target_if_quiescent(project, expected_target.as_ref(), retirement_id)
                .map_err(RuntimeError::Validation)?;
            after_wake().map_err(RuntimeError::Validation)?;
            Ok(())
        })
        .map_err(|error| match error {
            RuntimeError::Validation(reason)
                if reason.starts_with("WEB_") || reason.starts_with("CONFIG_") =>
            {
                reason
            }
            _ => "WEB_RETIREMENT_REFUSED".into(),
        })?;
    if !verify_wake(wake, project, retirement_id, expected_target.as_ref())? {
        return Err("WEB_RETIREMENT_WAKE_CHANGED".into());
    }
    Ok(
        json!({"action":"web_project_retire","projectId":project,"retirementId":retirement_id,
        "status":"RETIRED","retiredUtc":retired.retired_utc,"replayed":replayed,
        "workspacePreserved":true,"historyPreserved":true,"browserAction":"NONE"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(
        bound: bool,
    ) -> (
        PathBuf,
        PathBuf,
        Store,
        AutonomousProjectRegistryStoreV1,
        Option<Target>,
    ) {
        let root = std::env::temp_dir().join(format!("web-retirement-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let control = root.join("control");
        fs::create_dir(&control).unwrap();
        let registry =
            AutonomousProjectRegistryStoreV1::open(control.join(".catdesk/projects")).unwrap();
        registry.initialize(1, 1).unwrap();
        for id in ["catdesk", "alpha", "beta"] {
            let workspace = if id == "catdesk" {
                control.clone()
            } else {
                root.join(id)
            };
            fs::create_dir_all(&workspace).unwrap();
            fs::write(workspace.join("keep.txt"), id).unwrap();
            registry
                .register_project(AutonomousProjectV1 {
                    project_id: id.into(),
                    workspace,
                    git_identity: format!("https://example.test/{id}.git"),
                    verification_profile: "web_only_v1".into(),
                    codex_thread_id: None,
                    chatgpt_target_url: None,
                    chatgpt_target_sha256: None,
                    chatgpt_target_history: Vec::new(),
                })
                .unwrap();
        }
        let wake = Store::open_scoped_for_test(&root.join("wake"), &root).unwrap();
        wake.initialize().unwrap();
        let target = bound.then(|| {
            let target = wake
                .set_target("alpha", 0, "https://chatgpt.com/c/alpha-one")
                .unwrap();
            registry
                .initialize_project_chat_target("alpha", &target.url)
                .unwrap();
            target
        });
        let sibling = wake
            .set_target("beta", 0, "https://chatgpt.com/c/beta-one")
            .unwrap();
        registry
            .initialize_project_chat_target("beta", &sibling.url)
            .unwrap();
        (root, control, wake, registry, target)
    }

    fn run(
        control: &Path,
        wake: &Store,
        target: Option<&Target>,
        after: impl FnOnce() -> Result<(), String>,
    ) -> Result<Value, String> {
        retire_with_store(
            control,
            "alpha",
            "retirement-one",
            target.map_or(0, |t| t.generation),
            target.map(|t| t.digest.as_str()),
            wake,
            &|_| Ok(()),
            after,
        )
    }

    #[test]
    fn retirement_commits_both_tombstones_preserves_history_and_reopens_idempotently() {
        let (root, control, wake, registry, target) = fixture(true);
        let before = wake.config().unwrap();
        let old = registry
            .load_registry()
            .unwrap()
            .projects
            .into_iter()
            .find(|p| p.project_id == "alpha")
            .unwrap();
        assert_eq!(
            run(&control, &wake, target.as_ref(), || Ok(())).unwrap()["status"],
            "RETIRED"
        );
        let after = wake.config().unwrap();
        assert_eq!(before.history, after.history);
        assert_eq!(before.targets["beta"], after.targets["beta"]);
        assert!(!after.targets.contains_key("alpha"));
        assert_eq!(
            registry.retired_project("alpha").unwrap().unwrap().project,
            old
        );
        assert_eq!(
            fs::read_to_string(root.join("alpha/keep.txt")).unwrap(),
            "alpha"
        );
        let bytes = fs::read(control.join(".catdesk/projects/projects.json")).unwrap();
        assert_eq!(
            run(&control, &wake, target.as_ref(), || panic!(
                "completed retirement callback"
            ))
            .unwrap()["replayed"],
            true
        );
        assert_eq!(
            fs::read(control.join(".catdesk/projects/projects.json")).unwrap(),
            bytes
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retirement_recovers_wake_first_interruption_without_restoring_authority() {
        let (root, control, wake, registry, target) = fixture(true);
        assert_eq!(
            run(&control, &wake, target.as_ref(), || Err(
                "WEB_TEST_INTERRUPTION".into()
            ))
            .unwrap_err(),
            "WEB_TEST_INTERRUPTION"
        );
        assert!(
            registry
                .load_registry()
                .unwrap()
                .projects
                .iter()
                .any(|p| p.project_id == "alpha")
        );
        assert!(!wake.config().unwrap().targets.contains_key("alpha"));
        let pending = fs::read(control.join(".catdesk/projects/retirements/alpha.json")).unwrap();
        assert_eq!(
            run(&control, &wake, target.as_ref(), || Ok(())).unwrap()["status"],
            "RETIRED"
        );
        assert_eq!(
            fs::read(control.join(".catdesk/projects/retirements/alpha.json")).unwrap(),
            pending
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retirement_refuses_live_work_and_project_fence_without_changing_either_authority() {
        let (root, control, wake, _, target) = fixture(true);
        let paths = [
            control.join(".catdesk/projects/projects.json"),
            wake.root().join("config.json"),
        ];
        let before: [Vec<u8>; 2] = std::array::from_fn(|index| fs::read(&paths[index]).unwrap());
        let held = super::super::rollover::acquire_web_work_lease(&control, "alpha").unwrap();
        assert!(run(&control, &wake, target.as_ref(), || Ok(())).is_err());
        drop(held);
        let ledger = WorkLedger::open(&control, "alpha").unwrap();
        let event = format!(
            "web-work-{}",
            catdesk_wake::protocol::digest("alpha\0request-one")
        );
        ledger
            .prepare(
                "request-one",
                &event,
                &"a".repeat(64),
                target.as_ref().unwrap(),
            )
            .unwrap();
        assert_eq!(
            run(&control, &wake, target.as_ref(), || Ok(())).unwrap_err(),
            "WEB_RETIREMENT_WORK_PENDING"
        );
        assert_eq!(fs::read(&paths[0]).unwrap(), before[0]);
        assert_eq!(fs::read(&paths[1]).unwrap(), before[1]);
        assert!(
            !control
                .join(".catdesk/projects/retirements/alpha.json")
                .exists()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retirement_supports_unbound_projects_and_refuses_control_and_stale_scope() {
        let (root, control, wake, registry, target) = fixture(false);
        assert!(target.is_none());
        assert!(
            retire_with_store(
                &control,
                "catdesk",
                "invalid",
                0,
                None,
                &wake,
                &|_| Ok(()),
                || Ok(())
            )
            .is_err()
        );
        assert!(
            retire_with_store(
                &control,
                "alpha",
                "retirement-one",
                1,
                Some(&"a".repeat(64)),
                &wake,
                &|_| Ok(()),
                || Ok(())
            )
            .is_err()
        );
        assert_eq!(
            run(&control, &wake, None, || Ok(())).unwrap()["status"],
            "RETIRED"
        );
        assert!(registry.retired_project("alpha").unwrap().is_some());
        assert!(
            wake.config().unwrap().retired_targets["alpha"]
                .last_target
                .is_none()
        );
        assert!(
            wake.set_target("alpha", 0, "https://chatgpt.com/c/reuse")
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retirement_preserves_unresolved_assigned_task_history_without_blocking_siblings() {
        let (root, control, wake, _, target) = fixture(true);
        let tasks = control.join(".catdesk/project-mcp-tasks");
        fs::create_dir_all(tasks.join("beta/prior-task")).unwrap();
        // A sibling's assigned work does not enter this project's idle check.
        idle(&control, "alpha").unwrap();
        fs::create_dir_all(tasks.join("alpha/interrupted-task")).unwrap();
        fs::write(
            tasks.join("alpha/interrupted-task/evidence.json"),
            b"preserved",
        )
        .unwrap();
        let before = fs::read(wake.root().join("config.json")).unwrap();
        assert_eq!(
            run(&control, &wake, target.as_ref(), || Ok(())).unwrap_err(),
            "WEB_RETIREMENT_TASK_RECOVERY_REQUIRED"
        );
        assert_eq!(fs::read(wake.root().join("config.json")).unwrap(), before);
        assert_eq!(
            fs::read(tasks.join("alpha/interrupted-task/evidence.json")).unwrap(),
            b"preserved"
        );
        assert!(
            !control
                .join(".catdesk/projects/retirements/alpha.json")
                .exists()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retirement_journal_writer_uses_matching_bound_and_never_replaces_existing_evidence() {
        let (root, _, _, _, _) = fixture(false);
        let path = root.join("large-journal.json");
        let value = "a".repeat(50 * 1024);
        assert!(super::super::rollover::create_private_once(&path, &value).is_err());
        assert!(!path.exists());
        super::super::rollover::create_private_once_bounded(
            &path,
            &value,
            MAX_RETIREMENT_INTENT_BYTES as usize,
        )
        .unwrap();
        assert_eq!(read::<String>(&path).unwrap(), value);
        assert!(
            super::super::rollover::create_private_once_bounded(
                &path,
                &"replacement",
                MAX_RETIREMENT_INTENT_BYTES as usize
            )
            .is_err()
        );
        assert_eq!(read::<String>(&path).unwrap(), value);
        fs::remove_dir_all(root).unwrap();
    }
}
