//! Opt-in task checkpoints. The assignment stays immutable; only an exact,
//! journal-bound parent/tree/commit witness can advance its accepted HEAD.
use super::*;
use crate::delegated::journal::{ToolCallRecordV1, ToolMutationKind};
use std::{
    collections::BTreeMap,
    io::Write,
    process::Stdio,
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub(super) struct Policy {
    pub checkpoint: bool,
    pub sync: bool,
    pub origin: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Snapshot {
    head: String,
    scope_sha256: String,
    files: BTreeMap<String, Option<String>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Intent {
    schema_version: u32,
    task_id: String,
    call_id: String,
    request_sha256: String,
    assignment_sha256: String,
    original_base: String,
    branch: String,
    parent: String,
    tree: String,
    commit: String,
    before_index_tree: String,
    verification_calls: Vec<String>,
    snapshot: Snapshot,
}

fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID".into())
}
fn save(path: &Path, value: &impl Serialize) -> Result<(), String> {
    exact_file(path, &encode(value)?)
}
fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&read_assignment_bytes(path)?)
        .map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID".into())
}
fn oid(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

thread_local! { static GIT_DEADLINE: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) }; }

pub(super) struct Deadline(Option<Instant>);
impl Drop for Deadline {
    fn drop(&mut self) {
        GIT_DEADLINE.with(|slot| slot.set(self.0));
    }
}
pub(super) fn deadline_at(until: Instant) -> Deadline {
    let previous = GIT_DEADLINE.with(|slot| {
        let previous = slot.get();
        slot.set(Some(previous.map_or(until, |old| old.min(until))));
        previous
    });
    Deadline(previous)
}

/// Fixed reviewed Git, finite process ownership, bounded output, no shell,
/// hooks, signing, credential prompt or caller-selected transport/arguments.
fn git(root: &Path, args: &[&str], index: Option<&Path>, seconds: u64) -> Result<Vec<u8>, String> {
    // Reading config names executes neither filters nor filesystem monitors.
    // Names only, never values or credentials, leave this private boundary.
    let names = git_process(
        root,
        &["config", "--name-only", "--list", "-z"],
        None,
        10,
        &[],
    )?;
    if names.len() > 65536 {
        return Err("PROJECT_TASK_GIT_FILTER_CONFIG_INVALID".into());
    }
    let mut drivers = std::collections::BTreeSet::new();
    for name in names.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let name =
            std::str::from_utf8(name).map_err(|_| "PROJECT_TASK_GIT_FILTER_CONFIG_INVALID")?;
        let Some(rest) = name.strip_prefix("filter.") else {
            continue;
        };
        let Some((driver, property)) = rest.rsplit_once('.') else {
            return Err("PROJECT_TASK_GIT_FILTER_CONFIG_INVALID".into());
        };
        if !["clean", "smudge", "process", "required"].contains(&property) {
            continue;
        }
        if driver.is_empty()
            || driver.len() > 128
            || !driver
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || drivers.len() >= 64
        {
            return Err("PROJECT_TASK_GIT_FILTER_CONFIG_INVALID".into());
        }
        drivers.insert(driver.to_owned());
    }
    let mut overrides = Vec::new();
    for driver in drivers {
        for property in ["clean", "smudge", "process", "required"] {
            overrides.push("-c".to_owned());
            overrides.push(format!(
                "filter.{driver}.{property}={}",
                if property == "required" { "false" } else { "" }
            ));
        }
    }
    if overrides.iter().map(String::len).sum::<usize>() > 12000 {
        return Err("PROJECT_TASK_GIT_FILTER_CONFIG_INVALID".into());
    }
    git_process(root, args, index, seconds, &overrides)
}

fn git_process(
    root: &Path,
    args: &[&str],
    index: Option<&Path>,
    seconds: u64,
    overrides: &[String],
) -> Result<Vec<u8>, String> {
    let remaining = GIT_DEADLINE.with(|slot| {
        slot.get()
            .map(|until| until.saturating_duration_since(Instant::now()))
    });
    let bound = remaining.map_or(Duration::from_secs(seconds), |remaining| {
        remaining.min(Duration::from_secs(seconds))
    });
    if bound.is_zero() {
        return Err("PROJECT_TASK_GIT_BUDGET_EXPIRED".into());
    }
    let operation_deadline = Instant::now() + bound;
    let trusted = crate::delegated::github_bootstrap::resolve_trusted_git_identity(root)
        .map_err(|_| "PROJECT_TASK_GIT_UNAVAILABLE")?;
    let mut command = std::process::Command::new(&trusted.canonical);
    command
        .current_dir(root)
        .args([
            "--no-pager",
            "--literal-pathspecs",
            "--no-replace-objects",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "push.pushOption=",
        ])
        .args(overrides)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (count, (name, _)) in std::env::vars_os().enumerate() {
        if count >= 4096 {
            return Err("PROJECT_TASK_GIT_ENVIRONMENT_INVALID".into());
        }
        if name
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
        {
            command.env_remove(name);
        }
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("SSH_ASKPASS");
    crate::delegated::autonomy_supervisor::configure_read_only_git_command(&mut command)
        .map_err(|_| "PROJECT_TASK_GIT_UNAVAILABLE")?;
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", git_path(index)?);
    }
    #[cfg(test)]
    if args.first() == Some(&"ls-remote")
        && REMOTE_CLEANUP_FAILURE
            .lock()
            .map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID")?
            .as_deref()
            == Some(root)
    {
        return Err("PROJECT_TASK_GIT_CLEANUP_UNPROVEN".into());
    }
    // Trusted executable resolution and environment preparation may consume
    // the remaining budget. No new CAS/push starts after that deadline.
    if Instant::now() >= operation_deadline {
        return Err("PROJECT_TASK_GIT_BUDGET_EXPIRED".into());
    }
    #[cfg(windows)]
    let mut owner = crate::reviewed_candidate_owner::OwnedCandidateChild::spawn(&mut command)
        .map_err(|_| "PROJECT_TASK_GIT_UNAVAILABLE")?;
    #[cfg(windows)]
    let child = owner.child_mut();
    #[cfg(not(windows))]
    let mut owned_child = command
        .spawn()
        .map_err(|_| "PROJECT_TASK_GIT_UNAVAILABLE")?;
    #[cfg(not(windows))]
    let child = &mut owned_child;
    let stdout = child.stdout.take().ok_or("PROJECT_TASK_GIT_UNAVAILABLE")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = operation_deadline;
    let status = loop {
        #[cfg(windows)]
        let child = owner.child_mut();
        #[cfg(not(windows))]
        let child = &mut owned_child;
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => break None,
        }
    };
    #[cfg(windows)]
    let cleanup = owner.terminate_tree_and_confirm_quiescent().is_ok();
    #[cfg(not(windows))]
    let cleanup = {
        if status.is_none() {
            let _ = owned_child.kill();
        }
        owned_child.wait().is_ok()
    };
    let bytes = after_git_cleanup(cleanup, || {
        reader
            .join()
            .map_err(|_| "PROJECT_TASK_GIT_OUTCOME_UNKNOWN".to_owned())?
            .map_err(|_| "PROJECT_TASK_GIT_OUTCOME_UNKNOWN".to_owned())
    })?;
    if bytes.len() > 1024 * 1024 || !status.is_some_and(|s| s.success()) {
        return Err("PROJECT_TASK_GIT_OUTCOME_UNKNOWN".into());
    }
    Ok(bytes)
}

fn after_git_cleanup(
    cleanup_proved: bool,
    readback: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    // An unproved tree exit must retain the task's writer fence even when
    // readback also fails. Do not join a pipe reader that may still have a
    // surviving writer; the cleanup refusal takes precedence.
    if !cleanup_proved {
        return Err("PROJECT_TASK_GIT_CLEANUP_UNPROVEN".into());
    }
    readback()
}
fn text(root: &Path, args: &[&str], index: Option<&Path>) -> Result<String, String> {
    String::from_utf8(git(root, args, index, 10)?)
        .map(|s| s.trim().to_owned())
        .map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID".into())
}

pub(super) fn validate_paths(
    root: &Path,
    contract: &ExecutionContractV1,
    paths: &[String],
) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    if paths.len() > 128 {
        return Err("PROJECT_TASK_CHECKPOINT_PATHS_INVALID".into());
    }
    let scope = crate::workspace_tools::TaskReadScope::new(
        root,
        &contract.allowed_paths,
        &contract.forbidden_paths,
    )?;
    let mut seen = std::collections::BTreeSet::new();
    for path in paths {
        // A reviewed file list, never a Git pathspec or a recursive directory.
        if path.is_empty()
            || path.len() > 512
            || path.contains(['\\', ':', '*', '?', '\0', '\n', '\r'])
            || path
                .split('/')
                .any(|c| c.is_empty() || c == "." || c == "..")
            || !seen.insert(path)
        {
            return Err("PROJECT_TASK_CHECKPOINT_PATHS_INVALID".into());
        }
        let actual = scope.resolve_requested(path, false)?;
        if scope.relative_path(&actual)? != *path
            || actual.is_dir()
            || (actual.exists() && !regular(&actual))
        {
            return Err("PROJECT_TASK_CHECKPOINT_PATHS_INVALID".into());
        }
    }
    Ok(())
}

pub(super) fn snapshot(
    root: &Path,
    contract: &ExecutionContractV1,
    paths: &[String],
) -> Result<Snapshot, String> {
    validate_paths(root, contract, paths)?;
    let scope = crate::workspace_tools::TaskReadScope::new(
        root,
        &contract.allowed_paths,
        &contract.forbidden_paths,
    )?;
    let mut files = BTreeMap::new();
    for path in paths {
        let actual = scope.resolve_requested(path, false)?;
        let bytes = if actual.exists() {
            Some(fs::read(&actual).map_err(|_| "PROJECT_TASK_CHECKPOINT_FILE_UNAVAILABLE")?)
        } else {
            None
        };
        files.insert(path.clone(), bytes.as_deref().map(hash));
    }
    Ok(Snapshot {
        head: text(root, &["rev-parse", "HEAD"], None)?,
        scope_sha256: hash(&encode(&(contract, paths))?),
        files,
    })
}

pub(super) fn load_policy(directory: &Path, assignment_sha256: &str) -> Result<Policy, String> {
    let path = directory.join("execution-grant.json");
    if !path.exists()
        || directory
            .join("execution-revoked.json")
            .try_exists()
            .map_err(|_| "WEB_TASK_GRANT_UNAVAILABLE")?
    {
        return Ok(Policy::default());
    }
    let grant: WebExecutionGrant = load(&path)?;
    if grant.assignment.assignment_sha256 != assignment_sha256
        || (grant.allow_git_sync && !grant.allow_git_checkpoint)
    {
        return Err("WEB_TASK_GRANT_SCOPE_MISMATCH".into());
    }
    Ok(Policy {
        checkpoint: grant.allow_git_checkpoint,
        sync: grant.allow_git_sync,
        origin: grant.git_origin,
    })
}

pub(super) fn freeze_origin(root: &Path, source: &Path) -> Result<String, String> {
    let origin = text(root, &["config", "--get", "remote.origin.url"], None)?;
    let literal = Path::new(&origin).is_absolute()
        || origin.starts_with("https://")
        || origin.starts_with("ssh://")
        || (origin.contains('@') && origin.contains(':') && !origin.contains("::"));
    if !literal
        || origin.is_empty()
        || origin.starts_with('-')
        || origin.contains(['\0', '\n', '\r'])
        || origin != text(source, &["config", "--get", "remote.origin.url"], None)?
    {
        return Err("PROJECT_TASK_GIT_ORIGIN_CHANGED".into());
    }
    Ok(origin)
}

fn intents(directory: &Path) -> Result<Vec<(PathBuf, Intent)>, String> {
    let root = directory.join("git-checkpoints");
    if !root.exists() {
        return Ok(Vec::new());
    }
    plain_directory(&root)?;
    let mut output = Vec::new();
    for entry in fs::read_dir(root).map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID")? {
        let path = entry
            .map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID")?
            .path();
        if path.extension().and_then(|s| s.to_str()) == Some("json")
            && path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".intent.json")
        {
            if output.len() >= 256 {
                return Err("PROJECT_TASK_GIT_EVIDENCE_INVALID".into());
            }
            let value: Intent = load(&path)?;
            output.push((path, value));
        }
    }
    Ok(output)
}

fn validate_intent(
    root: &Path,
    intent: &Intent,
    assignment: &str,
    base: &str,
    branch: &str,
) -> Result<(), String> {
    if intent.schema_version != 1
        || intent.assignment_sha256 != assignment
        || intent.original_base != base
        || intent.branch != branch
        || !oid(&intent.parent)
        || !oid(&intent.tree)
        || !oid(&intent.commit)
        || intent.snapshot.head != intent.parent
        || intent.before_index_tree
            != text(
                root,
                &["rev-parse", &format!("{}^{{tree}}", intent.parent)],
                None,
            )?
        || text(root, &["rev-parse", &format!("{}^", intent.commit)], None)? != intent.parent
        || text(
            root,
            &["rev-parse", &format!("{}^{{tree}}", intent.commit)],
            None,
        )? != intent.tree
        || text(root, &["show", "-s", "--format=%B", &intent.commit], None)?
            != message(&intent.task_id, &intent.call_id, &intent.request_sha256)
    {
        return Err("PROJECT_TASK_CHECKPOINT_CHAIN_INVALID".into());
    }
    let changed = git(
        root,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "-z",
            &intent.parent,
            &intent.commit,
        ],
        None,
        10,
    )?;
    if changed
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .any(|name| {
            std::str::from_utf8(name).map_or(true, |name| !intent.snapshot.files.contains_key(name))
        })
    {
        return Err("PROJECT_TASK_CHECKPOINT_CHAIN_INVALID".into());
    }
    Ok(())
}

/// Read-only admission recognizes an exact pending CAS witness too. It does
/// not turn an arbitrary descendant of the assignment base into authority.
pub(super) fn accepted_head(
    root: &Path,
    directory: &Path,
    assignment: &str,
    base: &str,
    branch: &str,
) -> Result<String, String> {
    let mut remaining = intents(directory)?;
    remaining.retain(|(path,_)| !matches!(load::<Value>(&path.with_extension("receipt")), Ok(value) if value["status"]=="CHECKPOINT_NOT_APPLIED"));
    let mut head = base.to_owned();
    while !remaining.is_empty() {
        let matches: Vec<_> = remaining
            .iter()
            .enumerate()
            .filter(|(_, (_, i))| i.parent == head)
            .map(|(n, _)| n)
            .collect();
        if matches.len() != 1 {
            return Err("PROJECT_TASK_CHECKPOINT_CHAIN_INVALID".into());
        }
        let (_, intent) = remaining.remove(matches[0]);
        validate_intent(root, &intent, assignment, base, branch)?;
        let journal = DelegatedJournal::open(directory.join("journal"))
            .map_err(|_| "PROJECT_TASK_CHECKPOINT_CHAIN_INVALID")?;
        let run =
            RunId::new(&intent.task_id).map_err(|_| "PROJECT_TASK_CHECKPOINT_CHAIN_INVALID")?;
        let calls = journal
            .load_tool_calls(&run)
            .map_err(|_| "PROJECT_TASK_CHECKPOINT_CHAIN_INVALID")?;
        let call = calls
            .get(&intent.call_id)
            .ok_or("PROJECT_TASK_CHECKPOINT_CHAIN_INVALID")?;
        if call.tool_name != "git.checkpoint"
            || call.arguments_hash != intent.request_sha256
            || !matches!(
                call.status,
                ToolCallStatus::Executing | ToolCallStatus::Completed
            )
        {
            return Err("PROJECT_TASK_CHECKPOINT_CHAIN_INVALID".into());
        }
        if call.status == ToolCallStatus::Completed {
            let bytes = read_assignment_bytes(&path_for_receipt(directory, &intent.call_id))?;
            if call.result_hash.as_deref() != Some(hash(&bytes).as_str()) {
                return Err("PROJECT_TASK_CHECKPOINT_CHAIN_INVALID".into());
            }
        }
        let actual = text(root, &["rev-parse", "HEAD"], None)?;
        // An intent whose CAS never happened cannot authorize later work.
        if actual == head && remaining.is_empty() {
            return Ok(head);
        }
        head = intent.commit;
    }
    Ok(head)
}

fn path_for_receipt(directory: &Path, call: &str) -> PathBuf {
    directory
        .join("git-checkpoints")
        .join(format!("{}.intent.receipt", hash(call.as_bytes())))
}

fn message(task: &str, call: &str, request: &str) -> String {
    format!("CatDesk task checkpoint\n\ntask={task}\ncall={call}\nrequest={request}")
}

fn reconcile_one(
    root: &Path,
    path: &Path,
    intent: &Intent,
    assignment: &str,
    base: &str,
    branch: &str,
) -> Result<Value, String> {
    validate_intent(root, intent, assignment, base, branch)?;
    if text(root, &["rev-parse", "HEAD"], None)? != intent.commit {
        return Err("PROJECT_TASK_GIT_OUTCOME_UNKNOWN".into());
    }
    let index = text(root, &["write-tree"], None)?;
    if index != intent.tree {
        if index != intent.before_index_tree {
            return Err("PROJECT_TASK_GIT_INDEX_CHANGED".into());
        }
        git(root, &["read-tree", &intent.commit], None, 10)?;
    }
    let receipt = json!({"schemaVersion":1,"status":"CHECKPOINTED","taskId":intent.task_id,"callId":intent.call_id,
        "originalBaseCommit":intent.original_base,"parentCommit":intent.parent,"commit":intent.commit,
        "tree":intent.tree,"branch":intent.branch,"assignmentSha256":intent.assignment_sha256,
        "requestSha256":intent.request_sha256,"verificationCalls":intent.verification_calls,"files":intent.snapshot.files});
    save(&path.with_extension("receipt"), &receipt)?;
    Ok(receipt)
}

pub(super) fn reconcile_saved(
    root: &Path,
    directory: &Path,
    contract: &ExecutionContractV1,
    assignment: &str,
    base: &str,
    branch: &str,
) -> Result<(), String> {
    let journal = DelegatedJournal::open(directory.join("journal"))
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    let run = RunId::new(&contract.task_id).map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID")?;
    for (path, intent) in intents(directory)? {
        let calls = journal
            .load_tool_calls(&run)
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
        let record = calls
            .get(&intent.call_id)
            .ok_or("PROJECT_TASK_CHECKPOINT_CHAIN_INVALID")?;
        if intent.task_id != contract.task_id
            || record.tool_name != "git.checkpoint"
            || record.arguments_hash != intent.request_sha256
        {
            return Err("PROJECT_TASK_CHECKPOINT_CHAIN_INVALID".into());
        }
        if record.status == ToolCallStatus::Executing {
            let actual = text(root, &["rev-parse", "HEAD"], None)?;
            if actual == intent.parent
                && text(root, &["write-tree"], None)? == intent.before_index_tree
            {
                validate_intent(root, &intent, assignment, base, branch)?;
                save(
                    &path.with_extension("receipt"),
                    &json!({"status":"CHECKPOINT_NOT_APPLIED","commit":intent.commit,"parentCommit":intent.parent}),
                )?;
                journal
                    .transition_tool_call(
                        &run,
                        &record.tool_call_id,
                        ToolCallStatus::Failed,
                        None,
                        Some("Exact CAS was not applied; original call will not replay".into()),
                    )
                    .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
                continue;
            }
            let receipt = reconcile_one(root, &path, &intent, assignment, base, branch)?;
            journal
                .transition_tool_call(
                    &run,
                    &record.tool_call_id,
                    ToolCallStatus::Completed,
                    Some(hash(&encode(&receipt)?)),
                    Some("Exact saved checkpoint recovered".into()),
                )
                .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
        } else if record.status != ToolCallStatus::Completed
            && !(record.status == ToolCallStatus::Failed
                && load::<Value>(&path.with_extension("receipt"))?["status"]
                    == "CHECKPOINT_NOT_APPLIED")
        {
            return Err("PROJECT_TASK_RECOVERY_REQUIRED".into());
        }
    }
    let calls = journal
        .load_tool_calls(&run)
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    for record in calls
        .values()
        .filter(|r| r.tool_name == "git.sync" && r.status == ToolCallStatus::Executing)
    {
        let attempt = directory.join(format!(
            "git-sync-{}.json",
            hash(record.tool_call_id.as_str().as_bytes())
        ));
        let value: Value = load(&attempt)?;
        let commit = value["commit"]
            .as_str()
            .filter(|s| oid(s))
            .ok_or("PROJECT_TASK_GIT_EVIDENCE_INVALID")?;
        let policy = load_policy(directory, assignment)?;
        let origin = policy
            .origin
            .as_deref()
            .ok_or("PROJECT_TASK_GIT_SYNC_NOT_GRANTED")?;
        if !policy.sync
            || value["requestSha256"] != record.arguments_hash
            || value["branch"] != branch
            || value["originSha256"] != hash(origin.as_bytes())
            || text(root, &["config", "--get", "remote.origin.url"], None)? != origin
            || !intents(directory)?.iter().any(|(_, i)| i.commit == commit)
        {
            return Err("PROJECT_TASK_GIT_EVIDENCE_INVALID".into());
        }
        let reference = format!("refs/heads/{branch}");
        let remote = match text(root, &["ls-remote", "--refs", origin, &reference], None) {
            Ok(value) => Some(value),
            Err(reason) if reason == "PROJECT_TASK_GIT_CLEANUP_UNPROVEN" => return Err(reason),
            Err(_) => None,
        };
        let proved = remote.as_deref() == Some(format!("{commit}\t{reference}").as_str());
        let result = json!({"status":if proved{"SYNCED"}else{"REMOTE_OUTCOME_UNKNOWN"},"commit":commit,"branch":branch,"remoteExactCommitProved":proved,"automaticPushReplayAllowed":false});
        save(&attempt.with_extension("recovered"), &result)?;
        journal
            .transition_tool_call(
                &run,
                &record.tool_call_id,
                ToolCallStatus::Completed,
                Some(hash(&encode(&result)?)),
                Some(result["status"].as_str().unwrap().into()),
            )
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    }
    Ok(())
}

pub(super) fn reconcile(task: &ProjectTask) -> Result<(), String> {
    if task.git_policy.checkpoint {
        reconcile_saved(
            &task.root,
            &task.directory,
            &task.contract,
            &task.assignment_sha256,
            &task.original_base,
            &task.witness.3,
        )?;
    }
    Ok(())
}

pub(super) fn has_checkpoint(directory: &Path) -> Result<bool, String> {
    Ok(!intents(directory)?.is_empty())
}

pub(super) fn prior_head(directory: &Path, head: &str, base: &str) -> Result<bool, String> {
    Ok(head == base
        || intents(directory)?
            .iter()
            .any(|(_, intent)| intent.commit == head))
}

fn verify(task: &ProjectTask, ids: &[String], current: &Snapshot) -> Result<(), String> {
    if ids.is_empty() || ids.len() > 8 || task.command_profiles.is_empty() {
        return Err("PROJECT_TASK_SUCCESSFUL_VERIFICATION_REQUIRED".into());
    }
    let run = RunId::new(&task.contract.task_id).map_err(|_| "PROJECT_TASK_CALL_INVALID")?;
    let calls = task
        .journal
        .load_tool_calls(&run)
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    let mut profiles = std::collections::BTreeSet::new();
    for id in ids {
        let record = calls
            .get(id)
            .ok_or("PROJECT_TASK_SUCCESSFUL_VERIFICATION_REQUIRED")?;
        let bytes = read_assignment_bytes(
            &task
                .directory
                .join("command-results")
                .join(format!("{}.json", catdesk_wake::protocol::digest(id))),
        )?;
        let receipt: Value = serde_json::from_slice(&bytes)
            .map_err(|_| "PROJECT_TASK_SUCCESSFUL_VERIFICATION_REQUIRED")?;
        if record.tool_name != "verify.command"
            || record.status != ToolCallStatus::Completed
            || record.result_hash.as_deref() != Some(hash(&bytes).as_str())
            || receipt["provenance"]["taskId"] != task.contract.task_id
            || receipt["provenance"]["projectId"] != task.project.project.project_id
            || receipt["provenance"]["callId"] != *id
            || receipt["provenance"]["requestSha256"] != record.arguments_hash
            || receipt["provenance"]["baseCommit"] != task.original_base
            || receipt["provenance"]["branch"] != task.witness.3
            || receipt["success"] != true
            || receipt["exitCode"] != 0
            || receipt["timedOut"] != false
            || receipt["cleanupProved"] != true
            || receipt["checkpointSnapshot"]
                != serde_json::to_value(current).map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID")?
        {
            return Err("PROJECT_TASK_SUCCESSFUL_VERIFICATION_REQUIRED".into());
        }
        profiles.insert(
            receipt["profileId"]
                .as_str()
                .ok_or("PROJECT_TASK_SUCCESSFUL_VERIFICATION_REQUIRED")?
                .to_owned(),
        );
    }
    if task
        .command_profiles
        .iter()
        .any(|profile| !profiles.contains(&profile.id))
    {
        return Err("PROJECT_TASK_SUCCESSFUL_VERIFICATION_REQUIRED".into());
    }
    Ok(())
}

fn git_path(path: &Path) -> Result<String, String> {
    if cfg!(windows) && path.components().any(|c| matches!(c, std::path::Component::Normal(name) if name.to_str().is_none_or(|name| name.ends_with('.') || name.ends_with(' ')))) {
        return Err("PROJECT_TASK_GIT_PATH_INVALID".into());
    }
    crate::command::normalize_windows_verbatim_path(path.to_owned())
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "PROJECT_TASK_GIT_PATH_INVALID".into())
}

fn index_entries(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let bytes = git(root, &["ls-files", "--stage", "-z"], None, 10)?;
    let mut entries = BTreeMap::new();
    for row in bytes.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let row = std::str::from_utf8(row).map_err(|_| "PROJECT_TASK_GIT_INDEX_CHANGED")?;
        let (metadata, name) = row
            .split_once('\t')
            .ok_or("PROJECT_TASK_GIT_INDEX_CHANGED")?;
        let parts: Vec<_> = metadata.split(' ').collect();
        if parts.len() != 3
            || !oid(parts[1])
            || parts[2] != "0"
            || entries
                .insert(name.to_owned(), parts[0].to_owned())
                .is_some()
        {
            return Err("PROJECT_TASK_GIT_INDEX_CHANGED".into());
        }
    }
    Ok(entries)
}

fn status_within(root: &Path, files: &BTreeMap<String, Option<String>>) -> Result<bool, String> {
    let bytes = git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        None,
        10,
    )?;
    for row in bytes.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let name = std::str::from_utf8(row.get(3..).ok_or("PROJECT_TASK_GIT_INDEX_CHANGED")?)
            .map_err(|_| "PROJECT_TASK_GIT_INDEX_CHANGED")?;
        if !files.contains_key(name) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn checkpoint(task: &ProjectTask, request: &NormalizedToolCallV1) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Args {
        verification_call_ids: Vec<String>,
    }
    let args: Args = serde_json::from_value(request.arguments.clone())
        .map_err(|_| "PROJECT_TASK_GIT_ARGUMENTS_INVALID")?;
    let current = snapshot(&task.root, &task.contract, &task.checkpoint_paths)?;
    verify(task, &args.verification_call_ids, &current)?;
    // Reject unrelated changes without permitting Git clean/process filters.
    // The approved paths are compared as RAW bytes; EOL attributes may report
    // them modified even when they equal our exact raw committed postimages.
    if !status_within(&task.root, &current.files)? {
        return Err("PROJECT_TASK_CHECKPOINT_UNREVIEWED_CHANGES".into());
    }
    let before_index_tree = text(&task.root, &["write-tree"], None)?;
    if before_index_tree != text(&task.root, &["rev-parse", "HEAD^{tree}"], None)? {
        return Err("PROJECT_TASK_GIT_INDEX_CHANGED".into());
    }
    let entries = index_entries(&task.root)?;
    let root = task.directory.join("git-checkpoints");
    fs::create_dir_all(&root).map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID")?;
    plain_directory(&root)?;
    let key = hash(request.tool_call_id.as_str().as_bytes());
    let index = root.join(format!("{key}.index"));
    if index.exists() {
        return Err("PROJECT_TASK_GIT_RECORDED_NO_REPLAY".into());
    }
    git(&task.root, &["read-tree", "HEAD"], Some(&index), 10)?;
    for path in &task.checkpoint_paths {
        let mode = entries.get(path).map(String::as_str).unwrap_or("100644");
        if !["100644", "100755"].contains(&mode) {
            return Err("PROJECT_TASK_CHECKPOINT_FILE_MODE_INVALID".into());
        }
        let Some(expected) = &current.files[path] else {
            if entries.contains_key(path) {
                git(
                    &task.root,
                    &["update-index", "--force-remove", "--", path],
                    Some(&index),
                    10,
                )?;
            }
            continue;
        };
        let scope = crate::workspace_tools::TaskReadScope::new(
            &task.root,
            &task.contract.allowed_paths,
            &task.contract.forbidden_paths,
        )?;
        let actual = scope.resolve_requested(path, false)?;
        if !regular(&actual) {
            return Err("PROJECT_TASK_CHECKPOINT_FILE_MODE_INVALID".into());
        }
        let bytes = fs::read(actual).map_err(|_| "PROJECT_TASK_CHECKPOINT_FILE_UNAVAILABLE")?;
        if hash(&bytes) != *expected {
            return Err("PROJECT_TASK_VERIFIED_FILES_CHANGED".into());
        }
        let temporary = root.join(format!("{key}.blob"));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID")?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID")?;
        let blob = text(
            &task.root,
            &[
                "hash-object",
                "-w",
                "--no-filters",
                "--",
                git_path(&temporary)?.as_str(),
            ],
            None,
        )?;
        drop(file);
        fs::remove_file(temporary).map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID")?;
        if !oid(&blob) {
            return Err("PROJECT_TASK_GIT_EVIDENCE_INVALID".into());
        }
        git(
            &task.root,
            &["update-index", "--add", "--cacheinfo", mode, &blob, path],
            Some(&index),
            10,
        )?;
    }
    if snapshot(&task.root, &task.contract, &task.checkpoint_paths)? != current {
        return Err("PROJECT_TASK_VERIFIED_FILES_CHANGED".into());
    }
    let tree = text(&task.root, &["write-tree"], Some(&index))?;
    if tree == text(&task.root, &["rev-parse", "HEAD^{tree}"], None)? {
        return Err("PROJECT_TASK_CHECKPOINT_NO_CHANGES".into());
    }
    fs::remove_file(&index).map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID")?;
    let commit = text(
        &task.root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &current.head,
            "-m",
            &message(
                &task.contract.task_id,
                request.tool_call_id.as_str(),
                &request.arguments_hash,
            ),
        ],
        None,
    )?;
    let intent = Intent {
        schema_version: 1,
        task_id: task.contract.task_id.clone(),
        call_id: request.tool_call_id.as_str().into(),
        request_sha256: request.arguments_hash.clone(),
        assignment_sha256: task.assignment_sha256.clone(),
        original_base: task.original_base.clone(),
        branch: task.witness.3.clone(),
        parent: current.head.clone(),
        tree,
        commit: commit.clone(),
        before_index_tree,
        verification_calls: args.verification_call_ids,
        snapshot: current,
    };
    let path = root.join(format!("{key}.intent.json"));
    save(&path, &intent)?;
    failpoint("BEFORE_HEAD_CAS")?;
    git(
        &task.root,
        &[
            "update-ref",
            &format!("refs/heads/{}", task.witness.3),
            &commit,
            &intent.parent,
        ],
        None,
        10,
    )?;
    failpoint("AFTER_HEAD_CAS")?;
    reconcile_one(
        &task.root,
        &path,
        &intent,
        &task.assignment_sha256,
        &task.original_base,
        &task.witness.3,
    )
}

fn sync(task: &ProjectTask, request: &NormalizedToolCallV1) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Args {
        checkpoint_call_id: String,
    }
    let args: Args = serde_json::from_value(request.arguments.clone())
        .map_err(|_| "PROJECT_TASK_GIT_ARGUMENTS_INVALID")?;
    let path = task.directory.join("git-checkpoints").join(format!(
        "{}.intent.json",
        hash(args.checkpoint_call_id.as_bytes())
    ));
    let intent: Intent = load(&path)?;
    let checkpoint = reconcile_one(
        &task.root,
        &path,
        &intent,
        &task.assignment_sha256,
        &task.original_base,
        &task.witness.3,
    )?;
    let origin = task
        .git_policy
        .origin
        .as_deref()
        .ok_or("PROJECT_TASK_GIT_SYNC_NOT_GRANTED")?;
    if freeze_origin(&task.root, &task.project.project.workspace)? != origin {
        return Err("PROJECT_TASK_GIT_ORIGIN_CHANGED".into());
    }
    let reference = format!("refs/heads/{}", intent.branch);
    // Any prior attempt for this exact commit is observation-only, even if
    // the caller changes callId. A lost push reply never authorizes a resend.
    let mut prior_attempt = false;
    for entry in fs::read_dir(&task.directory).map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID")? {
        let entry = entry.map_err(|_| "PROJECT_TASK_GIT_EVIDENCE_INVALID")?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("git-sync-") && name.ends_with(".json") {
            let value: Value = load(&entry.path())?;
            if value["commit"] == intent.commit {
                prior_attempt = true;
            }
        }
    }
    let attempt = task.directory.join(format!(
        "git-sync-{}.json",
        hash(request.tool_call_id.as_str().as_bytes())
    ));
    let pushed = if prior_attempt {
        false
    } else {
        save(
            &attempt,
            &json!({"status":"REMOTE_OUTCOME_UNKNOWN","commit":intent.commit,"branch":intent.branch,
            "originSha256":hash(origin.as_bytes()),"requestSha256":request.arguments_hash,"checkpoint":checkpoint}),
        )?;
        failpoint("BEFORE_PUSH")?;
        git(
            &task.root,
            &[
                "push",
                "--porcelain",
                "--no-verify",
                "--no-follow-tags",
                "--recurse-submodules=no",
                "--signed=false",
                "--",
                origin,
                &format!("{}:{reference}", intent.commit),
            ],
            None,
            45,
        )
        .map(|_| true)
        .or_else(|reason| {
            if reason == "PROJECT_TASK_GIT_CLEANUP_UNPROVEN" {
                Err(reason)
            } else {
                Ok(false)
            }
        })?
    };
    let remote = match text(
        &task.root,
        &["ls-remote", "--refs", origin, &reference],
        None,
    ) {
        Ok(value) => Some(value),
        Err(reason) if reason == "PROJECT_TASK_GIT_CLEANUP_UNPROVEN" => return Err(reason),
        Err(_) => None,
    };
    let proved = remote.as_deref() == Some(format!("{}\t{reference}", intent.commit).as_str());
    let result = json!({"status":if proved{"SYNCED"}else{"REMOTE_OUTCOME_UNKNOWN"},"commit":intent.commit,"branch":intent.branch,
        "pushReturnedSuccess":pushed,"remoteExactCommitProved":proved,"automaticPushReplayAllowed":false});
    save(&attempt.with_extension("receipt"), &result)?;
    Ok(result)
}

pub(super) fn execute(
    task: &ProjectTask,
    request: &NormalizedToolCallV1,
    run: &RunId,
) -> Result<Value, String> {
    if !task.git_policy.checkpoint || (request.tool_name == "git.sync" && !task.git_policy.sync) {
        return Err("PROJECT_TASK_GIT_NOT_GRANTED".into());
    }
    task.journal
        .record_tool_call_requested(ToolCallRecordV1 {
            schema_version: 1,
            run_id: run.clone(),
            tool_call_id: request.tool_call_id.clone(),
            tool_name: request.tool_name.clone(),
            request_hash: request.arguments_hash.clone(),
            arguments_hash: request.arguments_hash.clone(),
            mutation_kind: ToolMutationKind::Mutating,
            status: ToolCallStatus::Requested,
            result_hash: None,
            outcome_summary: None,
        })
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    for phase in [ToolCallStatus::PolicyAllowed, ToolCallStatus::Executing] {
        task.journal
            .transition_tool_call(run, &request.tool_call_id, phase, None, None)
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    }
    let result = if request.tool_name == "git.checkpoint" {
        checkpoint(task, request)
    } else {
        sync(task, request)
    };
    match result {
        Ok(receipt) => {
            task.journal
                .transition_tool_call(
                    run,
                    &request.tool_call_id,
                    ToolCallStatus::Completed,
                    Some(hash(&encode(&receipt)?)),
                    Some(receipt["status"].as_str().unwrap_or("RECORDED").into()),
                )
                .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
            Ok(json!({"toolName":request.tool_name,"payload":receipt}))
        }
        Err(reason) => {
            // A durable intent or push attempt fences mutation. Only exact
            // positive reconciliation can retire it; never retry that call.
            let has_intent = task
                .directory
                .join("git-checkpoints")
                .join(format!(
                    "{}.intent.json",
                    hash(request.tool_call_id.as_str().as_bytes())
                ))
                .exists();
            let has_push = task
                .directory
                .join(format!(
                    "git-sync-{}.json",
                    hash(request.tool_call_id.as_str().as_bytes())
                ))
                .exists();
            if !has_intent && !has_push && reason != "PROJECT_TASK_GIT_CLEANUP_UNPROVEN" {
                task.journal
                    .transition_tool_call(
                        run,
                        &request.tool_call_id,
                        ToolCallStatus::Failed,
                        None,
                        Some(reason.clone()),
                    )
                    .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
            }
            Err(reason)
        }
    }
}

#[cfg(test)]
static REMOTE_CLEANUP_FAILURE: Mutex<Option<PathBuf>> = Mutex::new(None);
#[cfg(test)]
thread_local! { static FAILURE_STAGE: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
fn failpoint(stage: &str) -> Result<(), String> {
    if FAILURE_STAGE.with(|slot| slot.get() == Some(stage)) {
        return Err(format!("PROJECT_TASK_GIT_TEST_{stage}"));
    }
    Ok(())
}
#[cfg(not(test))]
fn failpoint(_: &str) -> Result<(), String> {
    Ok(())
}

pub(super) fn recovery_snapshot(
    directory: &Path,
    inspected: &AssignmentInspection,
) -> Result<Vec<(String, String)>, String> {
    let assignment = hash(&inspected.bytes);
    let policy = load_policy(directory, &assignment)?;
    if !policy.checkpoint || !has_checkpoint(directory)? {
        return Err("PROJECT_TASK_RECOVERY_ACTIVITY_RECORDED".into());
    }
    let journal = DelegatedJournal::open(directory.join("journal"))
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    let run =
        RunId::new(&inspected.contract.task_id).map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    if journal
        .load_contract(&run)
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
        != inspected.contract
        || journal
            .load_tool_calls(&run)
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
            .values()
            .any(|r| !matches!(r.status, ToolCallStatus::Completed | ToolCallStatus::Failed))
        || journal
            .load_patch_applications(&run)
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
            .iter()
            .any(|r| r.status == crate::delegated::journal::PatchApplicationStatus::OutcomeUnknown)
    {
        return Err("PROJECT_TASK_RECOVERY_REQUIRED".into());
    }
    if accepted_head(
        &inspected.root,
        directory,
        &assignment,
        &inspected.original_base,
        &inspected.witness.3,
    )? != inspected.witness.2
    {
        return Err("PROJECT_TASK_WORKTREE_CHANGED".into());
    }
    let mut pending = vec![
        directory.join("journal"),
        directory.join("command-results"),
        directory.join("jobs"),
        directory.join("git-checkpoints"),
    ];
    let mut files = Vec::new();
    let mut count = 0;
    while let Some(dir) = pending.pop() {
        if !dir.exists() {
            continue;
        }
        plain_directory(&dir)?;
        count += 1;
        if count > 128 {
            return Err("PROJECT_TASK_RECOVERY_JOURNAL_UNSAFE".into());
        }
        for entry in fs::read_dir(&dir).map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")? {
            let path = entry.map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?.path();
            if path.is_dir() {
                plain_directory(&path)?;
                pending.push(path);
            } else {
                if files.len() >= 512 {
                    return Err("PROJECT_TASK_RECOVERY_JOURNAL_UNSAFE".into());
                }
                files.push((
                    path.strip_prefix(directory)
                        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
                        .to_string_lossy()
                        .into_owned(),
                    hash(&read_assignment_bytes(&path)?),
                ));
            }
        }
    }
    for entry in fs::read_dir(directory).map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")? {
        let entry = entry.map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("git-sync-") {
            files.push((
                name.to_string_lossy().into_owned(),
                hash(&read_assignment_bytes(&entry.path())?),
            ));
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(all(test, windows))]
pub(super) mod tests {
    use super::*;

    #[test]
    fn task_git_cleanup_unproved_precedes_readback_and_does_not_wait_for_it() {
        let called = std::cell::Cell::new(false);
        let result = after_git_cleanup(false, || {
            called.set(true);
            panic!("unproved cleanup must not wait for a potentially blocked pipe reader");
        });
        assert_eq!(result.unwrap_err(), "PROJECT_TASK_GIT_CLEANUP_UNPROVEN");
        assert!(!called.get());
    }

    #[test]
    fn task_git_cleanup_proved_preserves_stdout_and_readback_failure() {
        assert_eq!(
            after_git_cleanup(true, || Ok(b"exact receipt".to_vec())).unwrap(),
            b"exact receipt"
        );
        assert_eq!(
            after_git_cleanup(true, || Err("PROJECT_TASK_GIT_OUTCOME_UNKNOWN".into())).unwrap_err(),
            "PROJECT_TASK_GIT_OUTCOME_UNKNOWN"
        );
    }

    pub(in crate::project_mcp_task) struct Fixture {
        pub(in crate::project_mcp_task) scope: ResolvedProjectScopeV1,
        pub(in crate::project_mcp_task) root: PathBuf,
        pub(in crate::project_mcp_task) assignment: PathBuf,
    }
    impl Fixture {
        pub(in crate::project_mcp_task) fn new() -> Self {
            Self::configured(false)
        }
        fn configured(filter_sentinel: bool) -> Self {
            let (scope, root, assignment) = super::super::tests::fixture("checkpoint");
            text(
                &root,
                &["config", "user.name", "Synthetic checkpoint fixture"],
                None,
            )
            .unwrap();
            text(
                &root,
                &["config", "user.email", "fixture@example.test"],
                None,
            )
            .unwrap();
            let remote = scope.control_workspace.parent().unwrap().join("remote.git");
            git(
                &root,
                &["init", "--bare", git_path(&remote).unwrap().as_str()],
                None,
                10,
            )
            .unwrap();
            text(
                &root,
                &[
                    "remote",
                    "add",
                    "origin",
                    git_path(&remote).unwrap().as_str(),
                ],
                None,
            )
            .unwrap();
            if filter_sentinel {
                fs::write(
                    root.join(".gitattributes"),
                    b"src/lib.rs text eol=lf filter=sentinel\n",
                )
                .unwrap();
                // Build this fixture's initial index without configuring a
                // runnable filter; the tested route must never run it later.
                let blob = text(
                    &root,
                    &["hash-object", "-w", "--no-filters", ".gitattributes"],
                    None,
                )
                .unwrap();
                text(
                    &root,
                    &[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        "100644",
                        &blob,
                        ".gitattributes",
                    ],
                    None,
                )
                .unwrap();
                let tree = text(&root, &["write-tree"], None).unwrap();
                let parent = text(&root, &["rev-parse", "HEAD"], None).unwrap();
                let commit = text(
                    &root,
                    &[
                        "commit-tree",
                        &tree,
                        "-p",
                        &parent,
                        "-m",
                        "Synthetic attribute baseline",
                    ],
                    None,
                )
                .unwrap();
                text(
                    &root,
                    &["update-ref", "refs/heads/codex/web-task", &commit, &parent],
                    None,
                )
                .unwrap();
                let sentinel = scope
                    .control_workspace
                    .parent()
                    .unwrap()
                    .join("filter-ran.txt");
                let filter = format!(
                    "echo FILTER_RAN > '{}'",
                    sentinel.to_string_lossy().replace('\\', "/")
                );
                for name in [
                    "filter.sentinel.clean",
                    "filter.sentinel.process",
                    "filter.sentinel.smudge",
                    "core.fsmonitor",
                ] {
                    text(&root, &["config", name, &filter], None).unwrap();
                }
                text(&root, &["config", "filter.sentinel.required", "true"], None).unwrap();
            }
            let mut value: Value = serde_json::from_slice(&fs::read(&assignment).unwrap()).unwrap();
            value["baseCommit"] = json!(text(&root, &["rev-parse", "HEAD"], None).unwrap());
            value["checkpointPaths"] = json!(["src/lib.rs"]);
            value["contract"]["allowedCommandProfiles"] = json!(["check"]);
            // No command is launched by these transaction fixtures. They
            // inject the trusted, journal-bound command-result boundary.
            value["commandProfiles"] = json!([{"id":"check","argv":[std::env::var_os("SystemRoot").map(PathBuf::from).unwrap().join("System32/cmd.exe")],"timeoutSeconds":10}]);
            fs::write(&assignment, encode(&value).unwrap()).unwrap();
            let directory = state_root(&scope, "task-fixture");
            fs::create_dir_all(&directory).unwrap();
            let bytes = fs::read(&assignment).unwrap();
            fs::write(directory.join("assignment.json"), &bytes).unwrap();
            let grant = WebExecutionGrant {
                schema_version: 1,
                project_id: scope.project.project_id.clone(),
                request_id: "owned-request".into(),
                source_event_id: "owned-event".into(),
                assignment: crate::web_work_state::WorkTaskAssignment {
                    task_id: "task-fixture".into(),
                    assignment_sha256: hash(&bytes),
                    scope_sha256: assignment_scope_sha256(&scope),
                },
                key_sha256: hash(b"synthetic fixture key"),
                allow_manual_wake: false,
                allow_git_checkpoint: true,
                allow_git_sync: true,
                git_origin: Some(freeze_origin(&root, &scope.project.workspace).unwrap()),
                native_queue: None,
            };
            save(&directory.join("execution-grant.json"), &grant).unwrap();
            Self {
                scope,
                root,
                assignment,
            }
        }
        fn task(&self) -> Arc<ProjectTask> {
            ProjectTask::open(&self.scope, &self.assignment).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let parent = self
                .scope
                .control_workspace
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap();
            assert_eq!(
                parent.parent(),
                Some(std::env::temp_dir().canonicalize().unwrap().as_path())
            );
            assert!(
                parent
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("project-task-")
            );
            fs::remove_dir_all(parent).unwrap();
        }
    }
    pub(in crate::project_mcp_task) fn verified(task: &ProjectTask, id: &str, success: bool) {
        let run = RunId::new(&task.contract.task_id).unwrap();
        let call = ToolCallId::new(id).unwrap();
        let arguments_hash = hash(id.as_bytes());
        task.journal
            .record_tool_call_requested(ToolCallRecordV1 {
                schema_version: 1,
                run_id: run.clone(),
                tool_call_id: call.clone(),
                tool_name: "verify.command".into(),
                request_hash: arguments_hash.clone(),
                arguments_hash: arguments_hash.clone(),
                mutation_kind: ToolMutationKind::Mutating,
                status: ToolCallStatus::Requested,
                result_hash: None,
                outcome_summary: None,
            })
            .unwrap();
        for status in [ToolCallStatus::PolicyAllowed, ToolCallStatus::Executing] {
            task.journal
                .transition_tool_call(&run, &call, status, None, None)
                .unwrap();
        }
        let value = json!({"profileId":"check","success":success,"exitCode":if success{0}else{1},"timedOut":false,"cleanupProved":true,
            "checkpointSnapshot":snapshot(&task.root,&task.contract,&task.checkpoint_paths).unwrap(),
            "provenance":{"projectId":task.project.project.project_id,"taskId":task.contract.task_id,"callId":id,"requestSha256":arguments_hash,"baseCommit":task.original_base,"branch":task.witness.3}});
        let directory = task.directory.join("command-results");
        fs::create_dir_all(&directory).unwrap();
        save(
            &directory.join(format!("{}.json", catdesk_wake::protocol::digest(id))),
            &value,
        )
        .unwrap();
        task.journal
            .transition_tool_call(
                &run,
                &call,
                ToolCallStatus::Completed,
                Some(hash(&encode(&value).unwrap())),
                None,
            )
            .unwrap();
    }
    fn call(id: &str, tool: &str, arguments: Value) -> Value {
        json!({"taskId":"task-fixture","callId":id,"tool":tool,"arguments":arguments})
    }
    fn request(id: &str, tool: &str, arguments: Value) -> NormalizedToolCallV1 {
        NormalizedToolCallV1 {
            tool_call_id: ToolCallId::new(id).unwrap(),
            tool_name: tool.into(),
            arguments_hash: stable_text_hash(&arguments.to_string()),
            arguments,
        }
    }

    /// Test-only transaction cutoff. The real dedicated owner exits with the
    /// exact CAS witnessed, its original claim retained, and no receipt/index
    /// reconciliation. This is not a command-sandbox verification fixture.
    pub(in crate::project_mcp_task) fn leave_checkpoint_after_cas(task: &ProjectTask) {
        let request = request(
            "cold-checkpoint",
            "git.checkpoint",
            json!({"verificationCallIds":["cold-verify"]}),
        );
        let run = RunId::new("task-fixture").unwrap();
        FAILURE_STAGE.with(|slot| slot.set(Some("AFTER_HEAD_CAS")));
        let result = execute(task, &request, &run);
        FAILURE_STAGE.with(|slot| slot.set(None));
        assert_eq!(result.unwrap_err(), "PROJECT_TASK_GIT_TEST_AFTER_HEAD_CAS");
        assert_ne!(
            text(&task.root, &["rev-parse", "HEAD"], None).unwrap(),
            task.original_base
        );
    }

    #[tokio::test]
    async fn task_git_checkpoint_normal_entry_reopens_preserves_assignment_and_replay_fence() {
        let fixture = Fixture::new();
        let original = fs::read(&fixture.assignment).unwrap();
        fs::write(
            fixture.root.join("src/lib.rs"),
            "pub fn value() -> u32 { 2 }\n",
        )
        .unwrap();
        let task = fixture.task();
        verified(&task, "verify-one", true);
        let input = call(
            "save-one",
            "git.checkpoint",
            json!({"verificationCallIds":["verify-one"]}),
        );
        let saved = task.execute(input.clone()).await.unwrap();
        let commit = saved["payload"]["commit"].as_str().unwrap().to_owned();
        assert_eq!(saved["payload"]["originalBaseCommit"], task.original_base);
        assert_eq!(
            text(&fixture.root, &["status", "--porcelain"], None).unwrap(),
            ""
        );
        assert_eq!(
            task.execute(input).await.unwrap_err(),
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        task.close().await.unwrap();
        drop(task);
        let reopened = fixture.task();
        assert_eq!(reopened.witness.2, commit);
        assert!(
            reopened
                .execute(call(
                    "read-after-save",
                    "read",
                    json!({"path":"src/lib.rs"})
                ))
                .await
                .is_ok()
        );
        reopened.close().await.unwrap();
        drop(reopened);
        assert_eq!(fs::read(&fixture.assignment).unwrap(), original);
    }

    #[tokio::test]
    async fn task_git_checkpoint_rejects_completed_but_failed_verification_and_changed_postimages()
    {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("src/lib.rs"), "synthetic changed\n").unwrap();
        let task = fixture.task();
        let before = task.original_base.clone();
        verified(&task, "failed-check", false);
        assert_eq!(
            task.execute(call(
                "failed-save",
                "git.checkpoint",
                json!({"verificationCallIds":["failed-check"]})
            ))
            .await
            .unwrap_err(),
            "PROJECT_TASK_SUCCESSFUL_VERIFICATION_REQUIRED"
        );
        verified(&task, "good-check", true);
        fs::write(fixture.root.join("src/lib.rs"), "changed after check\n").unwrap();
        assert_eq!(
            task.execute(call(
                "stale-save",
                "git.checkpoint",
                json!({"verificationCallIds":["good-check"]})
            ))
            .await
            .unwrap_err(),
            "PROJECT_TASK_SUCCESSFUL_VERIFICATION_REQUIRED"
        );
        assert_eq!(
            text(&fixture.root, &["rev-parse", "HEAD"], None).unwrap(),
            before
        );
        task.close().await.unwrap();
    }

    #[tokio::test]
    async fn task_git_checkpoint_refuses_unreviewed_changes_and_unproved_head() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("src/lib.rs"), "synthetic changed\n").unwrap();
        let task = fixture.task();
        verified(&task, "check", true);
        fs::write(fixture.root.join("unreviewed.txt"), "synthetic extra\n").unwrap();
        assert_eq!(
            task.execute(call(
                "extra-save",
                "git.checkpoint",
                json!({"verificationCallIds":["check"]})
            ))
            .await
            .unwrap_err(),
            "PROJECT_TASK_CHECKPOINT_UNREVIEWED_CHANGES"
        );
        task.close().await.unwrap();
        drop(task);
        fs::remove_file(fixture.root.join("unreviewed.txt")).unwrap();
        let tree = text(&fixture.root, &["rev-parse", "HEAD^{tree}"], None).unwrap();
        let old = text(&fixture.root, &["rev-parse", "HEAD"], None).unwrap();
        let foreign = text(
            &fixture.root,
            &[
                "commit-tree",
                &tree,
                "-p",
                &old,
                "-m",
                "Unproved synthetic commit",
            ],
            None,
        )
        .unwrap();
        text(
            &fixture.root,
            &["update-ref", "refs/heads/codex/web-task", &foreign, &old],
            None,
        )
        .unwrap();
        assert!(ProjectTask::open(&fixture.scope, &fixture.assignment).is_err());
    }

    #[tokio::test]
    async fn task_git_checkpoint_recovers_exact_postcommit_interruption_without_second_commit() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("src/lib.rs"), "synthetic changed\n").unwrap();
        let task = fixture.task();
        verified(&task, "check", true);
        let request = request(
            "interrupted-save",
            "git.checkpoint",
            json!({"verificationCallIds":["check"]}),
        );
        let run = RunId::new("task-fixture").unwrap();
        FAILURE_STAGE.with(|s| s.set(Some("AFTER_HEAD_CAS")));
        let result = execute(&task, &request, &run);
        FAILURE_STAGE.with(|s| s.set(None));
        assert_eq!(result.unwrap_err(), "PROJECT_TASK_GIT_TEST_AFTER_HEAD_CAS");
        let saved = text(&fixture.root, &["rev-parse", "HEAD"], None).unwrap();
        assert_ne!(saved, task.original_base);
        reconcile(&task).unwrap();
        assert_eq!(
            text(&fixture.root, &["rev-parse", "HEAD"], None).unwrap(),
            saved
        );
        assert_eq!(
            text(&fixture.root, &["status", "--porcelain"], None).unwrap(),
            ""
        );
        assert_eq!(
            task.journal.load_tool_calls(&run).unwrap()["interrupted-save"].status,
            ToolCallStatus::Completed
        );
        task.close().await.unwrap();
        drop(task);
        let reopened = fixture.task();
        reopened.close().await.unwrap();
    }

    #[tokio::test]
    async fn task_git_sync_uncertain_attempt_never_resends_and_reads_exact_saved_remote() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("src/lib.rs"), "synthetic changed\n").unwrap();
        let task = fixture.task();
        verified(&task, "check", true);
        let saved = task
            .execute(call(
                "save",
                "git.checkpoint",
                json!({"verificationCallIds":["check"]}),
            ))
            .await
            .unwrap();
        let commit = saved["payload"]["commit"].as_str().unwrap();
        let run = RunId::new("task-fixture").unwrap();
        let pending = request("lost-push", "git.sync", json!({"checkpointCallId":"save"}));
        FAILURE_STAGE.with(|s| s.set(Some("BEFORE_PUSH")));
        assert!(execute(&task, &pending, &run).is_err());
        FAILURE_STAGE.with(|s| s.set(None));
        // Simulate a different process observing after the lost result. It may
        // inspect, but cannot initiate another push for the same checkpoint.
        let observed = sync(
            &task,
            &request(
                "inspect-unknown",
                "git.sync",
                json!({"checkpointCallId":"save"}),
            ),
        )
        .unwrap();
        assert_eq!(observed["status"], "REMOTE_OUTCOME_UNKNOWN");
        assert_eq!(
            text(
                &fixture.root,
                &[
                    "ls-remote",
                    "--refs",
                    task.git_policy.origin.as_deref().unwrap(),
                    "refs/heads/codex/web-task"
                ],
                None
            )
            .unwrap(),
            ""
        );
        // A separately completed exact push is positive reconciliation, never
        // evidence from some later arbitrary local/remote HEAD.
        git(
            &fixture.root,
            &[
                "push",
                task.git_policy.origin.as_deref().unwrap(),
                &format!("{commit}:refs/heads/codex/web-task"),
            ],
            None,
            10,
        )
        .unwrap();
        let observed = sync(
            &task,
            &request(
                "inspect-sent",
                "git.sync",
                json!({"checkpointCallId":"save"}),
            ),
        )
        .unwrap();
        assert_eq!(observed["status"], "SYNCED");
        assert_eq!(observed["commit"], commit);
        assert_eq!(observed["pushReturnedSuccess"], false);
        // This simulated interrupted call is not used for cold-owner recovery.
        task.journal
            .transition_tool_call(
                &run,
                &pending.tool_call_id,
                ToolCallStatus::Completed,
                None,
                Some("Exact remote independently reconciled in fixture".into()),
            )
            .unwrap();
        task.close().await.unwrap();
    }

    #[tokio::test]
    async fn task_git_checkpoint_raw_bytes_skip_filters_on_status_commit_and_end_turn() {
        let fixture = Fixture::configured(true);
        let body = b"RAW_VERIFIED_SYNTHETIC\r\nSECOND_LINE\r\n";
        fs::write(fixture.root.join("src/lib.rs"), body).unwrap();
        let task = fixture.task();
        verified(&task, "raw-check", true);
        let saved = task
            .execute(call(
                "raw-save",
                "git.checkpoint",
                json!({"verificationCallIds":["raw-check"]}),
            ))
            .await
            .unwrap();
        let commit = saved["payload"]["commit"].as_str().unwrap();
        let actual = git(
            &fixture.root,
            &["cat-file", "blob", &format!("{commit}:src/lib.rs")],
            None,
            10,
        )
        .unwrap();
        let inspected = inspect_assignment(&fixture.scope, &fixture.assignment, 180).unwrap();
        let end_turn = require_end_turn_checkpoint(&fixture.scope, &inspected);
        let sentinel = fixture
            .scope
            .control_workspace
            .parent()
            .unwrap()
            .join("filter-ran.txt");
        let ran = sentinel.exists();
        task.close().await.unwrap();
        assert_eq!(actual, body);
        assert!(
            !ran,
            "controlled clean/process/smudge/fsmonitor command must never run"
        );
        assert!(end_turn.is_ok());
    }

    #[tokio::test]
    async fn task_git_checkpoint_precommit_interruption_proves_not_applied_without_replay() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("src/lib.rs"), "OWNED_PENDING_CHANGE\n").unwrap();
        let task = fixture.task();
        verified(&task, "check", true);
        let pending = request(
            "before-cas",
            "git.checkpoint",
            json!({"verificationCallIds":["check"]}),
        );
        let run = RunId::new("task-fixture").unwrap();
        FAILURE_STAGE.with(|s| s.set(Some("BEFORE_HEAD_CAS")));
        let result = execute(&task, &pending, &run);
        FAILURE_STAGE.with(|s| s.set(None));
        assert_eq!(result.unwrap_err(), "PROJECT_TASK_GIT_TEST_BEFORE_HEAD_CAS");
        assert!(
            task.directory
                .join("git-checkpoints")
                .join(format!("{}.intent.json", hash(b"before-cas")))
                .is_file()
        );
        reconcile(&task).unwrap();
        let actual = text(&fixture.root, &["rev-parse", "HEAD"], None).unwrap();
        assert_eq!(actual, task.original_base);
        assert_eq!(
            task.journal.load_tool_calls(&run).unwrap()["before-cas"].status,
            ToolCallStatus::Failed
        );
        assert_eq!(
            task.execute(call(
                "before-cas",
                "git.checkpoint",
                json!({"verificationCallIds":["check"]})
            ))
            .await
            .unwrap_err(),
            "PROJECT_TASK_CALL_RECORDED_NO_REPLAY"
        );
        let end_turn = require_end_turn_checkpoint(
            &fixture.scope,
            &inspect_assignment(&fixture.scope, &fixture.assignment, 180).unwrap(),
        );
        task.close().await.unwrap();
        assert!(end_turn.is_err());
        assert_eq!(
            fs::read(fixture.root.join("src/lib.rs")).unwrap(),
            b"OWNED_PENDING_CHANGE\n"
        );
    }

    #[tokio::test]
    async fn task_git_sync_readback_cleanup_error_retains_journal_and_writer_until_reconciled() {
        let fixture = Fixture::new();
        fs::write(
            fixture.root.join("src/lib.rs"),
            "OWNED_SYNC_CLEANUP_FIXTURE\n",
        )
        .unwrap();
        let task = fixture.task();
        verified(&task, "check", true);
        task.execute(call(
            "save",
            "git.checkpoint",
            json!({"verificationCallIds":["check"]}),
        ))
        .await
        .unwrap();
        *REMOTE_CLEANUP_FAILURE.lock().unwrap() = Some(fixture.root.clone());
        let result = task
            .execute(call(
                "cleanup-sync",
                "git.sync",
                json!({"checkpointCallId":"save"}),
            ))
            .await;
        *REMOTE_CLEANUP_FAILURE.lock().unwrap() = None;
        let run = RunId::new("task-fixture").unwrap();
        let pending = task.journal.load_tool_calls(&run).unwrap()["cleanup-sync"]
            .status
            .clone();
        let uncertain_close = task.close().await;
        let retained = task.state.lock().unwrap().writer.is_some();
        // The fixture still owns the exact lease. A fresh positive remote
        // observation retires its journal without another push, then permits
        // cleanup; it does not simulate successful native process termination.
        reconcile(&task).unwrap();
        let reconciled = task.journal.load_tool_calls(&run).unwrap()["cleanup-sync"]
            .status
            .clone();
        task.state.lock().unwrap().uncertain = false;
        task.close().await.unwrap();
        assert_eq!(result.unwrap_err(), "PROJECT_TASK_GIT_CLEANUP_UNPROVEN");
        assert_eq!(pending, ToolCallStatus::Executing);
        assert_eq!(
            uncertain_close.unwrap_err(),
            "PROJECT_TASK_UNCERTAIN_MARKER_RETAINED"
        );
        assert!(retained);
        assert_eq!(reconciled, ToolCallStatus::Completed);
    }

    #[test]
    fn task_git_revoked_grant_disables_git_even_for_operator_opened_assignment() {
        let fixture = Fixture::new();
        let directory = state_root(&fixture.scope, "task-fixture");
        let assignment = hash(&fs::read(&fixture.assignment).unwrap());
        let before = load_policy(&directory, &assignment).unwrap();
        save(
            &directory.join("execution-revoked.json"),
            &json!({"revoked":true}),
        )
        .unwrap();
        let after = load_policy(&directory, &assignment).unwrap();
        assert!(before.checkpoint && before.sync);
        assert!(!after.checkpoint && !after.sync && after.origin.is_none());
    }

    #[test]
    fn task_git_legacy_grants_serialize_without_new_permissions() {
        let value = json!({"schemaVersion":1,"projectId":"alpha","requestId":"request","sourceEventId":"event","assignment":{"taskId":"task","assignmentSha256":"a","scopeSha256":"b"},"keySha256":"c"});
        let grant: WebExecutionGrant = serde_json::from_value(value.clone()).unwrap();
        assert!(!grant.allow_git_checkpoint && !grant.allow_git_sync);
        assert_eq!(serde_json::to_value(grant).unwrap(), value);
    }
}

pub(super) fn require_end_turn_checkpoint(
    project: &ResolvedProjectScopeV1,
    inspected: &AssignmentInspection,
) -> Result<(), String> {
    let directory = state_root(project, &inspected.contract.task_id);
    if !load_policy(&directory, &hash(&inspected.bytes))?.checkpoint {
        return Ok(());
    }
    let current = snapshot(
        &inspected.root,
        &inspected.contract,
        &inspected.checkpoint_paths,
    )?;
    let latest = intents(&directory)?
        .into_iter()
        .find(|(_, i)| i.commit == current.head)
        .ok_or("PROJECT_TASK_END_TURN_CHECKPOINT_REQUIRED")?;
    if current.files != latest.1.snapshot.files
        || current.scope_sha256 != latest.1.snapshot.scope_sha256
        || text(&inspected.root, &["write-tree"], None)? != latest.1.tree
        || !status_within(&inspected.root, &current.files)?
    {
        return Err("PROJECT_TASK_END_TURN_CHECKPOINT_REQUIRED".into());
    }
    Ok(())
}

pub(super) fn require_recovery_state(
    project: &ResolvedProjectScopeV1,
    inspected: &AssignmentInspection,
) -> Result<(), String> {
    match require_end_turn_checkpoint(project, inspected) {
        Ok(()) => return Ok(()),
        Err(reason) if reason == "PROJECT_TASK_GIT_CLEANUP_UNPROVEN" => return Err(reason),
        Err(_) => {}
    }
    let directory = state_root(project, &inspected.contract.task_id);
    let current = snapshot(
        &inspected.root,
        &inspected.contract,
        &inspected.checkpoint_paths,
    )?;
    for (path, intent) in intents(&directory)? {
        if load::<Value>(&path.with_extension("receipt"))?["status"] == "CHECKPOINT_NOT_APPLIED"
            && intent.parent == current.head
            && intent.snapshot == current
            && text(&inspected.root, &["write-tree"], None)? == intent.before_index_tree
            && status_within(&inspected.root, &current.files)?
        {
            return Ok(());
        }
    }
    Err("PROJECT_TASK_RECOVERY_WORKTREE_NOT_CLEAN".into())
}
