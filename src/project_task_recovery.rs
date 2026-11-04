//! Cold recovery for newly claimed, dedicated, zero-call task owners only.
//! Legacy markers are never adopted. The original marker remains present until
//! retirement is durably authorized, then its atomic archival is the commit.
use super::*;
use crate::reviewed_candidate_owner::{OwnedCandidateChild, StoppedControllerLifetime};
use sha2::{Digest, Sha256};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Claim {
    schema_version: u32,
    purpose: String,
    claim_id: String,
    project_id: String,
    task_id: String,
    scope_sha256: String,
    assignment_sha256: String,
    contract_sha256: String,
    workspace: PathBuf,
    base_commit: String,
    branch: String,
    marker_nonce_sha256: String,
    lifetime_id: String,
    owner_pid: u32,
    owner_creation: u64,
    owner_session: u32,
    owner_job: String,
    executable_sha256: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Recovery {
    schema_version: u32,
    claim_id: String,
    project_id: String,
    task_id: String,
    assignment_sha256: String,
    claim_sha256: String,
    marker_sha256: String,
    journal: Vec<(String, String)>,
    recoverer_sha256: String,
    state: String,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn encoded<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|_| "PROJECT_TASK_OWNER_INVALID".into())
}
fn persist(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path
        .try_exists()
        .map_err(|_| "PROJECT_TASK_ARCHIVE_UNAVAILABLE")?
    {
        return exact_file(path, bytes);
    }
    use std::io::Write;
    let temporary = path.with_extension(format!("owned-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| "PROJECT_TASK_ARCHIVE_UNAVAILABLE")?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "PROJECT_TASK_ARCHIVE_UNAVAILABLE")?;
    drop(file);
    crate::delegated::autonomy_runtime::rename_writer_evidence(&temporary, path, bytes)
        .map_err(|_| "PROJECT_TASK_ARCHIVE_UNAVAILABLE")?;
    exact_file(path, bytes)
}
fn read<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&read_assignment_bytes(path)?)
        .map_err(|_| "PROJECT_TASK_OWNER_INVALID".into())
}
fn marker(id: &str, claim_hash: &str) -> Vec<u8> {
    format!("project-task-v1:{id}:{claim_hash}").into_bytes()
}
fn id_ok(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn digest_ok(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn current_executable_hash() -> Result<String, String> {
    let path = std::env::current_exe().map_err(|_| "PROJECT_TASK_OWNER_IMAGE_UNAVAILABLE")?;
    let bytes = fs::read(path).map_err(|_| "PROJECT_TASK_OWNER_IMAGE_UNAVAILABLE")?;
    Ok(digest(&bytes))
}
fn is_clean(root: &Path) -> Result<bool, String> {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let mut command = std::process::Command::new("git");
    command
        .args(["--no-optional-locks", "--no-pager", "-C"])
        .arg(root)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::delegated::autonomy_supervisor::configure_read_only_git_command(&mut command)
        .map_err(|_| "PROJECT_TASK_RECOVERY_GIT_UNAVAILABLE")?;
    let mut child = OwnedCandidateChild::spawn(&mut command)
        .map_err(|_| "PROJECT_TASK_RECOVERY_GIT_UNAVAILABLE")?;
    let stdout = child
        .child_mut()
        .stdout
        .take()
        .ok_or("PROJECT_TASK_RECOVERY_GIT_UNAVAILABLE")?;
    let stderr = child
        .child_mut()
        .stderr
        .take()
        .ok_or("PROJECT_TASK_RECOVERY_GIT_UNAVAILABLE")?;
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(4097).read_to_end(&mut bytes).map(|_| bytes)
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.take(4097).read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.child_mut().try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => break None,
        }
    };
    child
        .terminate_tree_and_confirm_quiescent()
        .map_err(|_| "PROJECT_TASK_RECOVERY_GIT_UNAVAILABLE")?;
    let output = out
        .join()
        .map_err(|_| "PROJECT_TASK_RECOVERY_GIT_UNAVAILABLE")?
        .map_err(|_| "PROJECT_TASK_RECOVERY_GIT_UNAVAILABLE")?;
    let error = err
        .join()
        .map_err(|_| "PROJECT_TASK_RECOVERY_GIT_UNAVAILABLE")?
        .map_err(|_| "PROJECT_TASK_RECOVERY_GIT_UNAVAILABLE")?;
    if !status.is_some_and(|status| status.success()) || output.len() > 4096 || error.len() > 4096 {
        return Err("PROJECT_TASK_RECOVERY_GIT_UNAVAILABLE".into());
    }
    Ok(output.is_empty())
}
fn clean(root: &Path) -> Result<(), String> {
    if !is_clean(root)? {
        return Err("PROJECT_TASK_RECOVERY_WORKTREE_NOT_CLEAN".into());
    }
    Ok(())
}

fn check_claim(
    claim: &Claim,
    project: &ResolvedProjectScopeV1,
    inspected: &AssignmentInspection,
) -> Result<(), String> {
    if claim.schema_version != 1
        || claim.purpose != "PROJECT_TASK_ZERO_CALLS_V1"
        || !id_ok(&claim.claim_id)
        || !id_ok(&claim.lifetime_id)
        || claim.project_id != project.project.project_id
        || claim.task_id != inspected.contract.task_id
        || claim.scope_sha256 != assignment_scope_sha256(project)
        || claim.assignment_sha256 != digest(&inspected.bytes)
        || claim.contract_sha256 != digest(&encoded(&inspected.contract)?)
        || claim.workspace != inspected.root
        || claim.base_commit != inspected.witness.2
        || claim.branch != inspected.witness.3
        || claim.marker_nonce_sha256 != digest(claim.claim_id.as_bytes())
        || !digest_ok(&claim.executable_sha256)
    {
        return Err("PROJECT_TASK_OWNER_SCOPE_CHANGED".into());
    }
    Ok(())
}

fn zero_journal(
    directory: &Path,
    inspected: &AssignmentInspection,
) -> Result<Vec<(String, String)>, String> {
    let root = directory.join("journal");
    let mut files = Vec::new();
    let mut pending = vec![
        root.clone(),
        directory.join("jobs"),
        directory.join("command-results"),
    ];
    let mut directories = 0;
    while let Some(dir) = pending.pop() {
        directories += 1;
        if directories > 128 {
            return Err("PROJECT_TASK_RECOVERY_JOURNAL_UNSAFE".into());
        }
        if !dir.exists() {
            continue;
        }
        plain_directory(&dir)?;
        for entry in fs::read_dir(&dir).map_err(|_| "PROJECT_TASK_RECOVERY_JOURNAL_UNAVAILABLE")? {
            let path = entry
                .map_err(|_| "PROJECT_TASK_RECOVERY_JOURNAL_UNAVAILABLE")?
                .path();
            if path.is_dir() {
                plain_directory(&path)?;
                pending.push(path);
                continue;
            }
            if !regular(&path) || files.len() >= 64 {
                return Err("PROJECT_TASK_RECOVERY_JOURNAL_UNSAFE".into());
            }
            if !path.starts_with(&root) {
                return Err("PROJECT_TASK_RECOVERY_ACTIVITY_RECORDED".into());
            }
            let bytes = read_assignment_bytes(&path)?;
            files.push((
                path.strip_prefix(directory)
                    .map_err(|_| "PROJECT_TASK_RECOVERY_JOURNAL_UNSAFE")?
                    .to_string_lossy()
                    .into_owned(),
                digest(&bytes),
            ));
        }
    }
    if files.is_empty() {
        return Ok(files);
    }
    let journal =
        DelegatedJournal::open(&root).map_err(|_| "PROJECT_TASK_RECOVERY_JOURNAL_UNAVAILABLE")?;
    let run =
        RunId::new(&inspected.contract.task_id).map_err(|_| "PROJECT_TASK_CONTRACT_INVALID")?;
    let snapshot = journal
        .load_run(&run)
        .map_err(|_| "PROJECT_TASK_RECOVERY_JOURNAL_UNAVAILABLE")?;
    if serde_json::to_value(snapshot).map_err(|_| "PROJECT_TASK_RECOVERY_JOURNAL_UNAVAILABLE")?["state"]
        != "RUNNING"
    {
        return Err("PROJECT_TASK_RECOVERY_ACTIVITY_RECORDED".into());
    }
    if journal
        .load_contract(&run)
        .map_err(|_| "PROJECT_TASK_RECOVERY_JOURNAL_UNAVAILABLE")?
        != inspected.contract
        || !journal
            .load_tool_calls(&run)
            .map_err(|_| "PROJECT_TASK_RECOVERY_JOURNAL_UNAVAILABLE")?
            .is_empty()
        || !journal
            .load_patch_proposals(&run)
            .map_err(|_| "PROJECT_TASK_RECOVERY_JOURNAL_UNAVAILABLE")?
            .is_empty()
        || !journal
            .load_patch_applications(&run)
            .map_err(|_| "PROJECT_TASK_RECOVERY_JOURNAL_UNAVAILABLE")?
            .is_empty()
    {
        return Err("PROJECT_TASK_RECOVERY_ACTIVITY_RECORDED".into());
    }
    for (relative, _) in &files {
        let path = directory.join(relative);
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("PROJECT_TASK_RECOVERY_JOURNAL_UNSAFE")?;
        if name == format!("{}.json", inspected.contract.task_id)
            && relative.contains("integrated_state")
        {
            let state: Value = read(&path)?;
            let allowed = [
                "schemaVersion",
                "runId",
                "patchProposals",
                "lastVerification",
                "lastDiff",
                "completedToolCallIds",
                "providerHistory",
                "completedToolCalls",
                "consecutiveFailedVerifications",
                "adviceConsultedForCurrentFailure",
                "advisorConsultationsUsed",
                "recentReadExcerpts",
                "nextEventSequence",
                "correctiveTurnsUsed",
                "jsonToolRecoveryTurnsRemaining",
            ];
            if state["schemaVersion"] != 1
                || state["runId"] != inspected.contract.task_id
                || state
                    .as_object()
                    .is_none_or(|map| map.keys().any(|key| !allowed.contains(&key.as_str())))
            {
                return Err("PROJECT_TASK_RECOVERY_JOURNAL_UNSAFE".into());
            }
            for key in [
                "patchProposals",
                "completedToolCallIds",
                "providerHistory",
                "recentReadExcerpts",
            ] {
                if !state[key].as_array().is_some_and(Vec::is_empty) {
                    return Err("PROJECT_TASK_RECOVERY_ACTIVITY_RECORDED".into());
                }
            }
            if state["completedToolCalls"] != 0
                || !state["lastVerification"].is_null()
                || !state["lastDiff"].is_null()
            {
                return Err("PROJECT_TASK_RECOVERY_ACTIVITY_RECORDED".into());
            }
            for key in [
                "consecutiveFailedVerifications",
                "advisorConsultationsUsed",
                "correctiveTurnsUsed",
                "jsonToolRecoveryTurnsRemaining",
            ] {
                if state[key] != 0 {
                    return Err("PROJECT_TASK_RECOVERY_ACTIVITY_RECORDED".into());
                }
            }
        } else if name == "events.jsonl" {
            let text = String::from_utf8(read_assignment_bytes(&path)?)
                .map_err(|_| "PROJECT_TASK_RECOVERY_JOURNAL_UNSAFE")?;
            for line in text.lines() {
                let event: Value = serde_json::from_str(line)
                    .map_err(|_| "PROJECT_TASK_RECOVERY_JOURNAL_UNSAFE")?;
                if event["lifecycleEvent"] != "STARTED"
                    || event["payload"]["kind"] != "runStateChanged"
                    || event["payload"]["state"] != "RUNNING"
                {
                    return Err("PROJECT_TASK_RECOVERY_ACTIVITY_RECORDED".into());
                }
            }
        } else if !matches!(
            name,
            "contract.json"
                | "state.json"
                | "tool_calls.json"
                | "patch_proposals.json"
                | "patch_applications.json"
        ) {
            return Err("PROJECT_TASK_RECOVERY_JOURNAL_UNSAFE".into());
        }
    }
    files.sort();
    Ok(files)
}

pub(super) fn lock_and_recover(
    project: &ResolvedProjectScopeV1,
    inspected: &AssignmentInspection,
) -> Result<ProjectWriterGuard, String> {
    let writer =
        crate::delegated::autonomy_runtime::acquire_project_task_writer_lock(&inspected.root)
            .map_err(|_| "PROJECT_TASK_WRITER_BUSY_OR_UNRESOLVED")?;
    let pending = writer
        .task_marker()
        .map_err(|_| "PROJECT_TASK_RECOVERY_MARKER_UNAVAILABLE")?;
    let fence = writer
        .task_recovery_fence()
        .map_err(|_| "PROJECT_TASK_RECOVERY_FENCE_UNAVAILABLE")?;
    if pending.is_none() && fence.is_none() {
        return Ok(writer);
    }
    let id = if let Some(bytes) = &fence {
        let rec: Recovery =
            serde_json::from_slice(bytes).map_err(|_| "PROJECT_TASK_RECOVERY_FENCE_INVALID")?;
        rec.claim_id
    } else {
        String::from_utf8(
            pending
                .clone()
                .ok_or("PROJECT_TASK_RECOVERY_MARKER_UNAVAILABLE")?,
        )
        .map_err(|_| "PROJECT_TASK_LEGACY_OWNER_UNRECOVERABLE")?
        .strip_prefix("project-task-v1:")
        .ok_or("PROJECT_TASK_LEGACY_OWNER_UNRECOVERABLE")?
        .split(':')
        .next()
        .ok_or("PROJECT_TASK_LEGACY_OWNER_UNRECOVERABLE")?
        .to_owned()
    };
    if !id_ok(&id) {
        return Err("PROJECT_TASK_RECOVERY_FENCE_INVALID".into());
    }
    let directory = task_directory(project, &inspected.contract.task_id, false)?;
    let claim_path = directory.join(format!("owner-{id}.json"));
    let claim: Claim = read(&claim_path)?;
    check_claim(&claim, project, inspected)?;
    let claim_hash = digest(&read_assignment_bytes(&claim_path)?);
    let expected_marker = marker(&id, &claim_hash);
    if pending
        .as_ref()
        .is_some_and(|bytes| bytes != &expected_marker)
    {
        return Err("PROJECT_TASK_RECOVERY_MARKER_CHANGED".into());
    }
    let prepared_path = directory.join(format!("recovery-{id}-prepared.json"));
    let authorized_path = directory.join(format!("recovery-{id}-authorized.json"));
    let retired_path = directory.join(format!("recovery-{id}-retired.json"));
    let prepared = if prepared_path.exists() {
        let rec: Recovery = read(&prepared_path)?;
        rec
    } else {
        OwnedCandidateChild::prove_project_task_owner_exited(
            &claim.lifetime_id,
            &claim.owner_job,
            claim.owner_session,
            claim.owner_pid,
            claim.owner_creation,
        )?;
        clean(&inspected.root)?;
        Recovery {
            schema_version: 1,
            claim_id: id.clone(),
            project_id: claim.project_id.clone(),
            task_id: claim.task_id.clone(),
            assignment_sha256: claim.assignment_sha256.clone(),
            claim_sha256: digest(&read_assignment_bytes(&claim_path)?),
            marker_sha256: digest(&expected_marker),
            journal: zero_journal(&directory, inspected)?,
            recoverer_sha256: current_executable_hash()?,
            state: "PREPARED".into(),
        }
    };
    if prepared.schema_version != 1
        || !digest_ok(&prepared.recoverer_sha256)
        || prepared.claim_id != id
        || prepared.state != "PREPARED"
        || prepared.project_id != claim.project_id
        || prepared.task_id != claim.task_id
        || prepared.assignment_sha256 != claim.assignment_sha256
        || prepared.claim_sha256 != digest(&read_assignment_bytes(&claim_path)?)
        || prepared.marker_sha256 != digest(&expected_marker)
    {
        return Err("PROJECT_TASK_RECOVERY_FENCE_INVALID".into());
    }
    let prepared_bytes = encoded(&prepared)?;
    persist(&prepared_path, &prepared_bytes)?;
    writer
        .write_task_recovery_fence(&prepared_bytes)
        .map_err(|_| "PROJECT_TASK_RECOVERY_FENCE_INVALID")?;
    let mut authorized = prepared.clone();
    authorized.state = "RETIREMENT_AUTHORIZED".into();
    if !authorized_path.exists() {
        OwnedCandidateChild::prove_project_task_owner_exited(
            &claim.lifetime_id,
            &claim.owner_job,
            claim.owner_session,
            claim.owner_pid,
            claim.owner_creation,
        )?;
        clean(&inspected.root)?;
        if zero_journal(&directory, inspected)? != prepared.journal {
            return Err("PROJECT_TASK_RECOVERY_JOURNAL_CHANGED".into());
        }
        if pending.as_deref() != Some(expected_marker.as_slice()) {
            return Err("PROJECT_TASK_RECOVERY_MARKER_CHANGED".into());
        }
        persist(&authorized_path, &encoded(&authorized)?)?;
    } else {
        let existing: Recovery = read(&authorized_path)?;
        if existing != authorized {
            return Err("PROJECT_TASK_RECOVERY_FENCE_INVALID".into());
        }
    }
    if pending.is_some() {
        OwnedCandidateChild::prove_project_task_owner_exited(
            &claim.lifetime_id,
            &claim.owner_job,
            claim.owner_session,
            claim.owner_pid,
            claim.owner_creation,
        )?;
        clean(&inspected.root)?;
        if zero_journal(&directory, inspected)? != prepared.journal {
            return Err("PROJECT_TASK_RECOVERY_JOURNAL_CHANGED".into());
        }
    }
    failpoint("AUTHORIZED")?;
    writer
        .archive_task_marker(&id, &expected_marker)
        .map_err(|_| "PROJECT_TASK_RECOVERY_ARCHIVE_UNPROVEN")?;
    failpoint("ARCHIVED")?;
    let mut retired = authorized;
    retired.state = "RETIRED_ZERO_CALLS".into();
    retired.recoverer_sha256 = current_executable_hash()?;
    if retired_path.exists() {
        let prior: Recovery = read(&retired_path)?;
        if !digest_ok(&prior.recoverer_sha256) {
            return Err("PROJECT_TASK_RECOVERY_FENCE_INVALID".into());
        }
        retired.recoverer_sha256 = prior.recoverer_sha256.clone();
        if retired != prior {
            return Err("PROJECT_TASK_RECOVERY_FENCE_INVALID".into());
        }
    } else {
        persist(&retired_path, &encoded(&retired)?)?;
    }
    writer
        .finish_task_recovery_fence(&id, &prepared_bytes)
        .map_err(|_| "PROJECT_TASK_RECOVERY_FENCE_UNAVAILABLE")?;
    Ok(writer)
}

pub(super) fn begin(
    writer: &mut ProjectWriterGuard,
    project: &ResolvedProjectScopeV1,
    inspected: &AssignmentInspection,
    lifetime: &StoppedControllerLifetime,
) -> Result<Option<String>, String> {
    let directory = task_directory(project, &inspected.contract.task_id, true)?;
    // Eligibility for additional crash recovery must never narrow ordinary
    // warm resumption. Unknown or oversized status stays on the legacy gate.
    if !matches!(is_clean(&inspected.root), Ok(true))
        || zero_journal(&directory, inspected).is_err()
    {
        writer
            .begin_writer_turn()
            .map_err(|_| "PROJECT_TASK_WRITER_MARKER_UNAVAILABLE")?;
        return Ok(None);
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let claim = Claim {
        schema_version: 1,
        purpose: "PROJECT_TASK_ZERO_CALLS_V1".into(),
        claim_id: id.clone(),
        project_id: project.project.project_id.clone(),
        task_id: inspected.contract.task_id.clone(),
        scope_sha256: assignment_scope_sha256(project),
        assignment_sha256: digest(&inspected.bytes),
        contract_sha256: digest(&encoded(&inspected.contract)?),
        workspace: inspected.root.clone(),
        base_commit: inspected.witness.2.clone(),
        branch: inspected.witness.3.clone(),
        marker_nonce_sha256: digest(id.as_bytes()),
        lifetime_id: lifetime.transaction_id.clone(),
        owner_pid: lifetime.pid,
        owner_creation: lifetime.creation_time,
        owner_session: lifetime.session_id,
        owner_job: lifetime.job_name.clone(),
        executable_sha256: current_executable_hash()?,
    };
    persist(
        &directory.join(format!("owner-{id}.json")),
        &encoded(&claim)?,
    )?;
    writer
        .begin_task_writer_turn(&id, &digest(&encoded(&claim)?))
        .map_err(|_| "PROJECT_TASK_OWNER_MARKER_FAILED")?;
    Ok(Some(id))
}

pub(super) fn closed(
    project: &ResolvedProjectScopeV1,
    task: &str,
    claim: &str,
) -> Result<(), String> {
    persist(
        &task_directory(project, task, false)?.join(format!("owner-{claim}-closed.json")),
        &encoded(&json!({"schemaVersion":1,"claimId":claim,"state":"CLOSED"}))?,
    )
}

#[cfg(test)]
thread_local! { static FAILURE_STAGE: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
fn failpoint(stage: &str) -> Result<(), String> {
    if FAILURE_STAGE.with(|slot| slot.get() == Some(stage)) {
        return Err(format!("PROJECT_TASK_RECOVERY_TEST_{stage}"));
    }
    Ok(())
}
#[cfg(not(test))]
fn failpoint(_: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    #[test]
    #[ignore = "Dedicated child invoked only by bounded recovery tests"]
    fn task_owner_fixture_child() {
        let path = std::env::var("CATDESK_TASK_FIXTURE_INPUT").unwrap();
        let config: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let scope = ResolvedProjectScopeV1 {
            control_workspace: PathBuf::from(config["control"].as_str().unwrap()),
            project: serde_json::from_value(config["project"].clone()).unwrap(),
        };
        OwnedCandidateChild::enter_project_task_lifetime(&scope.control_workspace).unwrap();
        let task =
            ProjectTask::open(&scope, Path::new(config["assignment"].as_str().unwrap())).unwrap();
        if config["activity"] == true {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(
                    task.execute(json!({"taskId":"task-fixture","callId":"observed-call",
                    "tool":"read","arguments":{"path":"src/lib.rs"}})),
                )
                .unwrap();
        }
        let task = if config["warm"] == true {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(task.close())
                .unwrap();
            let root = PathBuf::from(task.metadata()["executionWorkspace"].as_str().unwrap());
            drop(task);
            // Exceed the bounded eligibility probe with valid retained work.
            for index in 0..90 {
                fs::write(
                    root.join("src")
                        .join(format!("{index}-{}.txt", "x".repeat(70))),
                    "preserved work",
                )
                .unwrap();
            }
            ProjectTask::open(&scope, Path::new(config["assignment"].as_str().unwrap())).unwrap()
        } else {
            task
        };
        println!("{}", task.metadata());
        std::io::stdout().flush().unwrap();
        std::thread::sleep(Duration::from_secs(20));
    }

    fn interrupted_owner(activity: bool) -> (ResolvedProjectScopeV1, PathBuf, PathBuf, String) {
        interrupted_owner_mode(activity, false)
    }

    fn interrupted_owner_mode(
        activity: bool,
        warm: bool,
    ) -> (ResolvedProjectScopeV1, PathBuf, PathBuf, String) {
        let (scope, root, assignment) = super::super::tests::fixture("recovery-alpha");
        let input = root.parent().unwrap().join("recovery-input.json");
        fs::write(
            &input,
            serde_json::to_vec(&json!({"control":scope.control_workspace,
            "project":scope.project,"assignment":assignment,"activity":activity,"warm":warm}))
            .unwrap(),
        )
        .unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "project_mcp_task::recovery::tests::task_owner_fixture_child",
                "--ignored",
                "--nocapture",
            ])
            .env(
                "CATDESK_TASK_OWNER_FIXTURE",
                uuid::Uuid::new_v4().simple().to_string(),
            )
            .env("CATDESK_TASK_FIXTURE_INPUT", input)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut owner = OwnedCandidateChild::spawn(&mut command).unwrap();
        let output = owner.child_mut().stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    let _ = tx.send(value);
                }
            }
        });
        let ready = rx.recv_timeout(Duration::from_secs(12));
        owner.terminate_tree_and_confirm_quiescent().unwrap();
        reader.join().unwrap();
        let ready = ready.expect("bounded task child did not reach READY");
        assert_eq!(
            ready["coldRecoveryProfile"],
            if warm {
                "LEGACY_NO_AUTORECOVERY"
            } else {
                "DEDICATED_NAMED_JOB_ZERO_CALLS"
            }
        );
        let claim = ready["zeroCallRecoveryOwnerClaim"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        (scope, root, assignment, claim)
    }

    #[test]
    fn recovery_real_owner_interruption_and_both_commit_phases() {
        for stage in [None, Some("AUTHORIZED"), Some("ARCHIVED")] {
            let (scope, root, assignment, id) = interrupted_owner(false);
            let inspected = inspect_assignment(&scope, &assignment, 180).unwrap();
            let directory = task_directory(&scope, "task-fixture", false).unwrap();
            let original_claim = fs::read(directory.join(format!("owner-{id}.json"))).unwrap();
            assert!(acquire_project_writer_lock(&root).is_err());
            let _lease = crate::operator_facade::acquire_web_work_lease(
                &scope.control_workspace,
                "recovery-alpha",
            )
            .unwrap();
            FAILURE_STAGE.with(|slot| slot.set(stage));
            let attempted = lock_and_recover(&scope, &inspected);
            FAILURE_STAGE.with(|slot| slot.set(None));
            if let Some(stage) = stage {
                assert_eq!(
                    attempted.err().unwrap(),
                    format!("PROJECT_TASK_RECOVERY_TEST_{stage}")
                );
                assert!(acquire_project_writer_lock(&root).is_err());
                if stage == "AUTHORIZED" {
                    fs::write(root.join("unexpected.txt"), "foreign progress").unwrap();
                    assert_eq!(
                        lock_and_recover(&scope, &inspected).err().unwrap(),
                        "PROJECT_TASK_RECOVERY_WORKTREE_NOT_CLEAN"
                    );
                    fs::remove_file(root.join("unexpected.txt")).unwrap();
                }
                drop(lock_and_recover(&scope, &inspected).unwrap());
            } else {
                drop(attempted.unwrap());
            }
            drop(acquire_project_writer_lock(&root).unwrap());
            assert_eq!(
                fs::read(directory.join(format!("owner-{id}.json"))).unwrap(),
                original_claim
            );
            let retired: Recovery =
                read(&directory.join(format!("recovery-{id}-retired.json"))).unwrap();
            assert_eq!(retired.state, "RETIRED_ZERO_CALLS");
            // Idempotent cold read does not replay the retired operation.
            drop(lock_and_recover(&scope, &inspected).unwrap());
        }
    }

    #[test]
    fn recovery_refuses_recorded_calls_and_legacy_markers() {
        let (scope, root, assignment, _) = interrupted_owner(true);
        let inspected = inspect_assignment(&scope, &assignment, 180).unwrap();
        assert_eq!(
            lock_and_recover(&scope, &inspected).err().unwrap(),
            "PROJECT_TASK_RECOVERY_ACTIVITY_RECORDED"
        );
        assert!(acquire_project_writer_lock(&root).is_err());
        let (legacy_scope, legacy_root, assignment) = super::super::tests::fixture("legacy-alpha");
        let mut writer = acquire_project_writer_lock(&legacy_root).unwrap();
        writer.begin_writer_turn().unwrap();
        let bytes = writer.task_marker().unwrap().unwrap();
        drop(writer);
        let inspected = inspect_assignment(&legacy_scope, &assignment, 180).unwrap();
        assert_eq!(
            lock_and_recover(&legacy_scope, &inspected).err().unwrap(),
            "PROJECT_TASK_LEGACY_OWNER_UNRECOVERABLE"
        );
        let writer =
            crate::delegated::autonomy_runtime::acquire_project_task_writer_lock(&legacy_root)
                .unwrap();
        assert_eq!(writer.task_marker().unwrap().unwrap(), bytes);
    }

    #[test]
    fn recovery_shared_host_is_not_a_dedicated_owner() {
        assert!(
            OwnedCandidateChild::current_project_task_lifetime()
                .unwrap()
                .is_none()
        );
        assert!(
            OwnedCandidateChild::enter_project_task_lifetime(&std::env::current_dir().unwrap())
                .is_err()
        );
    }

    #[test]
    fn recovery_eligibility_preserves_large_dirty_warm_resume() {
        let (_, root, _, claim) = interrupted_owner_mode(false, true);
        assert!(claim.is_empty());
        assert_eq!(fs::read_dir(root.join("src")).unwrap().count(), 91);
        assert!(acquire_project_writer_lock(&root).is_err());
    }

    #[test]
    fn recovery_exact_file_rename_refuses_drift_collision_and_hardlink() {
        let root =
            std::env::temp_dir().join(format!("task-recovery-rename-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        let target = root.join("target");
        fs::write(&source, "known").unwrap();
        assert!(
            crate::delegated::autonomy_runtime::rename_writer_evidence(&source, &target, b"wrong")
                .is_err()
        );
        fs::write(&target, "occupied").unwrap();
        assert!(
            crate::delegated::autonomy_runtime::rename_writer_evidence(&source, &target, b"known")
                .is_err()
        );
        assert_eq!(fs::read(&target).unwrap(), b"occupied");
        fs::remove_file(&target).unwrap();
        fs::hard_link(&source, &target).unwrap();
        assert!(
            crate::delegated::autonomy_runtime::rename_writer_evidence(
                &source,
                &root.join("archive"),
                b"known"
            )
            .is_err()
        );
        fs::remove_file(&target).unwrap();
        crate::delegated::autonomy_runtime::rename_writer_evidence(&source, &target, b"known")
            .unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&target).unwrap(), b"known");
        fs::remove_dir_all(root).unwrap();
    }
}
