//! Read-only inventory of evidence that can prevent a direct Codex turn.
//!
//! This deliberately does not acquire or clear a writer lock, open a state
//! store (whose constructor creates directories), or infer that a native
//! provider child has exited from a terminal session label.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::delegated::autonomy_accounting::{
    ActivitySpanStatusV1, EXECUTION_ACCOUNTING_SCHEMA_VERSION, ExecutionAccountingLedgerV1,
    ExecutionAccountingRecordV1, validate as validate_accounting_ledger,
};
use crate::delegated::autonomy_projects::AutonomousProjectV1;
use crate::delegated::autonomy_runtime::inspect_project_writer_pending_marker;
use crate::delegated::autonomy_state::{
    AUTONOMY_STATE_SCHEMA_VERSION, AutonomousPassedVerificationCheckpointV1,
    AutonomousQueueTaskStateV1, AutonomousQueueV1, AutonomousSessionSnapshotV1,
    AutonomousSessionStateV1, validate_passed_verification_checkpoint,
};

const MAX_SESSION_COUNT: usize = 1_024;
const MAX_DIRECTORY_ENTRIES: usize = 2_048;
const MAX_SESSION_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LEDGER_BYTES: u64 = 16 * 1024 * 1024;
const MAX_AGGREGATE_EVIDENCE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ACCOUNTING_RECORDS: usize = 1_024;
const MAX_SPANS_PER_RECORD: usize = 256;
const MAX_REPORT_ROWS: usize = 4_096;

fn redirected(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    false
}

fn checked_directory(path: &Path) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err("CODEX_RECOVERY: managed state directory is unreadable".into()),
    };
    if !metadata.is_dir() || redirected(&metadata) {
        return Err("CODEX_RECOVERY: managed state directory is unsafe".into());
    }
    Ok(true)
}

fn open_read_only_no_follow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Open the reparse point itself so a replaced link cannot redirect
        // the evidence read to another project or user file.
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

fn same_file_identity(
    file: &File,
    path: &Path,
    opened: &fs::Metadata,
    observed: &fs::Metadata,
) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let Ok(second) = open_read_only_no_follow(path) else {
            return false;
        };
        let Ok(second_metadata) = second.metadata() else {
            return false;
        };
        if !second_metadata.is_file() || redirected(&second_metadata) {
            return false;
        }
        let Ok(first) = crate::windows_protected_fs::handle_information(
            file.as_raw_handle(),
            "Codex recovery evidence",
        ) else {
            return false;
        };
        let Ok(second) = crate::windows_protected_fs::handle_information(
            second.as_raw_handle(),
            "Codex recovery evidence",
        ) else {
            return false;
        };
        let _ = (opened, observed);
        first.file_attributes & 0x400 == 0
            && second.file_attributes & 0x400 == 0
            && first.volume_serial_number == second.volume_serial_number
            && first.file_index_high == second.file_index_high
            && first.file_index_low == second.file_index_low
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = (file, path);
        opened.dev() == observed.dev() && opened.ino() == observed.ino()
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (file, path, opened, observed);
        false
    }
}

fn checked_file_bytes(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("CODEX_RECOVERY: managed evidence is unreadable".into()),
    };
    if !metadata.is_file() || redirected(&metadata) || metadata.len() > limit {
        return Err("CODEX_RECOVERY: managed evidence is unsafe or oversized".into());
    }
    let file = open_read_only_no_follow(path)
        .map_err(|_| "CODEX_RECOVERY: managed evidence is unreadable or redirected".to_string())?;
    let opened = file
        .metadata()
        .map_err(|_| "CODEX_RECOVERY: managed evidence handle is unreadable".to_string())?;
    let observed = fs::symlink_metadata(path)
        .map_err(|_| "CODEX_RECOVERY: managed evidence path changed".to_string())?;
    if !opened.is_file()
        || redirected(&opened)
        || opened.len() > limit
        || !observed.is_file()
        || redirected(&observed)
        || !same_file_identity(&file, path, &opened, &observed)
    {
        return Err("CODEX_RECOVERY: managed evidence handle is unsafe or changed".into());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "CODEX_RECOVERY: managed evidence is unreadable".to_string())?;
    if bytes.len() as u64 > limit {
        return Err("CODEX_RECOVERY: managed evidence changed or exceeded its size bound".into());
    }
    Ok(Some(bytes))
}

fn read_json<T: DeserializeOwned>(
    path: &Path,
    limit: u64,
    total_bytes: &mut u64,
    evidence_hasher: &mut Sha256,
) -> Result<Option<T>, String> {
    let remaining = MAX_AGGREGATE_EVIDENCE_BYTES.saturating_sub(*total_bytes);
    let Some(bytes) = checked_file_bytes(path, limit.min(remaining))? else {
        return Ok(None);
    };
    *total_bytes = total_bytes
        .checked_add(bytes.len() as u64)
        .filter(|count| *count <= MAX_AGGREGATE_EVIDENCE_BYTES)
        .ok_or("CODEX_RECOVERY: aggregate evidence exceeds report bound")?;
    let digest = Sha256::digest(&bytes);
    evidence_hasher.update(
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_bytes(),
    );
    evidence_hasher.update(digest);
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "CODEX_RECOVERY: managed evidence has invalid JSON or schema".into())
}

fn safe_id(value: &str) -> String {
    if !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        value.to_owned()
    } else {
        format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
    }
}

fn inflight_task(state: &AutonomousQueueTaskStateV1) -> bool {
    matches!(
        state,
        AutonomousQueueTaskStateV1::WorkerRunning
            | AutonomousQueueTaskStateV1::Running
            | AutonomousQueueTaskStateV1::Verifying
            | AutonomousQueueTaskStateV1::Repairing
    )
}

/// Only bounded metadata needed to explain a claim. Checkpoint diff, review,
/// provider handle and task text are never retained in the report rows.
#[derive(Clone)]
enum CheckpointFacts {
    Invalid,
    Valid(ValidatedCheckpointFacts),
}

#[derive(Clone)]
struct ValidatedCheckpointFacts {
    session_id: String,
    project_id: String,
    task_id: String,
    contract_hash: String,
    provider_id: String,
    provider_turn_count: u32,
    codex_profile_thread_id: Option<String>,
    source_expectation_recorded: bool,
}

impl CheckpointFacts {
    fn checked(checkpoint: AutonomousPassedVerificationCheckpointV1, session_id: &str) -> Self {
        if validate_passed_verification_checkpoint(&checkpoint, session_id).is_err() {
            return Self::Invalid;
        }
        Self::Valid(ValidatedCheckpointFacts {
            session_id: checkpoint.session_id,
            project_id: checkpoint.project_id,
            task_id: checkpoint.logical_task_id,
            contract_hash: checkpoint.approved_contract_hash,
            provider_id: checkpoint.provider_id,
            provider_turn_count: checkpoint.provider_turn_count,
            codex_profile_thread_id: checkpoint
                .codex_turn_profile
                .map(|profile| profile.codex_thread_id),
            source_expectation_recorded: checkpoint.reviewed_source_expectation.is_some(),
        })
    }
}

struct ClaimFacts {
    session: Arc<SessionFacts>,
    task_id: String,
    task_state: AutonomousQueueTaskStateV1,
}

struct SessionFacts {
    session_id: String,
    session_state: AutonomousSessionStateV1,
    current_task_id: Option<String>,
    provider_thread_id: Option<String>,
    expected_codex_thread_id: Option<String>,
    approved_contract_hash: Option<String>,
    provider_turn_count: u32,
    codex_profile_tracking_started: bool,
    codex_profile_violation: bool,
    checkpoint: Option<CheckpointFacts>,
}

fn claim_hash(session_id: &str, task_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"catdesk-recovery-claim-v1\0");
    hasher.update(session_id.as_bytes());
    hasher.update([0]);
    hasher.update(task_id.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

/// Gap codes describe missing or conflicting recorded evidence only. They do
/// not certify that any provider process exited or that a checkpoint is valid.
fn claim_gap_codes(
    claim: &ClaimFacts,
    records: &[ExecutionAccountingRecordV1],
    project_id: &str,
) -> BTreeSet<&'static str> {
    let session = &claim.session;
    let mut gaps = BTreeSet::from([
        "OWNER_PROCESS_IDENTITY_UNRECORDED",
        "SOURCE_BOUND_CHILD_EXIT_UNPROVEN",
    ]);
    if session.current_task_id.as_deref() != Some(claim.task_id.as_str()) {
        gaps.insert("CURRENT_TASK_MISMATCH");
    }
    if session.codex_profile_violation {
        gaps.insert("CODEX_TURN_PROFILE_VIOLATION");
    }
    let matching = records
        .iter()
        .filter(|record| {
            record.catdesk_session_id == session.session_id && record.task_id == claim.task_id
        })
        .collect::<Vec<_>>();
    if matching.is_empty() {
        gaps.insert("ACCOUNTING_RECORD_MISSING");
    } else if matching.len() > 1 {
        gaps.insert("ACCOUNTING_RECORD_AMBIGUOUS");
    }
    for record in matching {
        let open_span = record.activity_spans.iter().any(|span| {
            span.status == ActivitySpanStatusV1::Open || span.ended_at_unix_millis.is_none()
        });
        if record.ended_at_unix_millis.is_none() {
            gaps.insert("ACCOUNTING_RECORD_UNENDED");
        }
        if open_span {
            gaps.insert("ACCOUNTING_OPEN_SPAN");
            if record.ended_at_unix_millis.is_some() {
                gaps.insert("ACCOUNTING_ENDED_WITH_OPEN_SPAN");
            }
        }
        if record.activity_evidence_truncated {
            gaps.insert("ACCOUNTING_ACTIVITY_TRUNCATED");
        }
        if record.provider_turns == 0 {
            gaps.insert("ACCOUNTING_PROVIDER_TURN_COUNT_ZERO");
        }
        if record.provider_id == "codex-cli" {
            if !session.codex_profile_tracking_started
                || record.provider_turns == 0
                || record.codex_turn_profiles.len() < record.provider_turns as usize
            {
                gaps.insert("CODEX_TURN_PROFILE_INCOMPLETE");
            }
            if !record.codex_turn_profiles.is_empty() && record.requested_model.is_none() {
                gaps.insert("CODEX_TURN_PROFILE_BINDING_MISSING");
            }
            if record.codex_turn_profiles.iter().any(|profile| {
                record
                    .provider_session_id
                    .as_deref()
                    .is_some_and(|thread_id| profile.codex_thread_id != thread_id)
                    || record
                        .requested_model
                        .as_deref()
                        .is_some_and(|model_id| profile.model_id != model_id)
            }) {
                gaps.insert("CODEX_TURN_PROFILE_BINDING_MISMATCH");
            }
            if session.provider_thread_id.is_none()
                || session.expected_codex_thread_id.is_none()
                || record.provider_session_id.is_none()
            {
                gaps.insert("CODEX_THREAD_BINDING_MISSING");
            }
            if session.provider_thread_id != session.expected_codex_thread_id
                || session.provider_thread_id != record.provider_session_id
            {
                gaps.insert("CODEX_THREAD_BINDING_MISMATCH");
            }
        } else if session.provider_thread_id.is_some()
            && record.provider_session_id.is_some()
            && session.provider_thread_id != record.provider_session_id
        {
            gaps.insert("PROVIDER_SESSION_MISMATCH");
        }
        if let Some(CheckpointFacts::Valid(checkpoint)) = &session.checkpoint {
            if checkpoint.provider_id != record.provider_id {
                gaps.insert("CHECKPOINT_PROVIDER_MISMATCH");
            }
        }
    }
    if matches!(session.checkpoint.as_ref(), Some(CheckpointFacts::Invalid)) {
        gaps.insert("CHECKPOINT_INVALID");
    }
    if let Some(CheckpointFacts::Valid(checkpoint)) = &session.checkpoint {
        if !matches!(
            session.session_state,
            AutonomousSessionStateV1::Queued
                | AutonomousSessionStateV1::Verifying
                | AutonomousSessionStateV1::RecoveringAfterRestart
                | AutonomousSessionStateV1::WaitingForChatgpt
                | AutonomousSessionStateV1::WaitingForUser
        ) {
            gaps.insert("CHECKPOINT_SESSION_STATE_INELIGIBLE");
        }
        if checkpoint.session_id != session.session_id
            || checkpoint.project_id != project_id
            || checkpoint.task_id != claim.task_id
            || session.approved_contract_hash.as_deref() != Some(checkpoint.contract_hash.as_str())
            || checkpoint.provider_turn_count != session.provider_turn_count
        {
            gaps.insert("CHECKPOINT_BINDING_MISMATCH");
        }
        if checkpoint.provider_id == "codex-cli" {
            if checkpoint.codex_profile_thread_id.is_none() {
                gaps.insert("CHECKPOINT_CODEX_PROFILE_MISSING");
            } else if checkpoint.codex_profile_thread_id != session.provider_thread_id {
                gaps.insert("CHECKPOINT_CODEX_PROFILE_THREAD_MISMATCH");
            }
        }
        if !checkpoint.source_expectation_recorded {
            gaps.insert("CHECKPOINT_SOURCE_EXPECTATION_MISSING");
        }
    }
    gaps
}

fn push_bounded_row(
    rows: &mut Vec<Value>,
    row: Value,
    total_rows: &mut usize,
) -> Result<(), String> {
    if *total_rows >= MAX_REPORT_ROWS {
        return Err("CODEX_RECOVERY: report row count exceeds bound".into());
    }
    rows.push(row);
    *total_rows += 1;
    Ok(())
}

/// Returns a bounded, sanitized snapshot. This is evidence for a reviewed
/// owner handoff, never a permit to start native work or clear a marker.
pub(super) fn report(project: &AutonomousProjectV1, prepare: bool) -> Result<Value, String> {
    let marker_sha256 =
        inspect_project_writer_pending_marker(&project.workspace).map_err(|_| {
            "CODEX_RECOVERY: current-user writer marker state is unavailable or unsafe".to_string()
        })?;
    let mut evidence_hasher = Sha256::new();
    let mut session_rows = Vec::new();
    let mut queue_rows = Vec::new();
    let mut ledger_rows = Vec::new();
    let mut claim_rows = Vec::new();
    let mut claim_gap_counts = BTreeMap::<&'static str, usize>::new();
    let mut observed_claims = Vec::new();
    let mut total_sessions = 0usize;
    let mut total_accounting_records = 0usize;
    let mut total_bytes = 0u64;
    let mut total_rows = 0usize;
    let catdesk_root = project.workspace.join(".catdesk");
    if checked_directory(&catdesk_root)? {
        let state_root = catdesk_root.join("autonomy");
        if checked_directory(&state_root)? {
            let canonical_root = state_root
                .canonicalize()
                .map_err(|_| "CODEX_RECOVERY: managed state root is unreadable".to_string())?;
            if !canonical_root.starts_with(&project.workspace) {
                return Err("CODEX_RECOVERY: managed state root escaped the project".into());
            }
            let mut session_dirs = Vec::new();
            let mut directory_entries = 0usize;
            for entry in fs::read_dir(&state_root)
                .map_err(|_| "CODEX_RECOVERY: managed state is unreadable".to_string())?
            {
                directory_entries += 1;
                if directory_entries > MAX_DIRECTORY_ENTRIES {
                    return Err(
                        "CODEX_RECOVERY: managed directory entry count exceeds report bound".into(),
                    );
                }
                let entry =
                    entry.map_err(|_| "CODEX_RECOVERY: managed state is unreadable".to_string())?;
                let metadata = fs::symlink_metadata(entry.path())
                    .map_err(|_| "CODEX_RECOVERY: managed state is unreadable".to_string())?;
                if redirected(&metadata) {
                    return Err("CODEX_RECOVERY: managed state contains a redirected path".into());
                }
                if metadata.is_dir() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if !matches!(name.as_str(), "diagnostics" | "qwen_tools") {
                        if safe_id(&name) != name {
                            return Err(
                                "CODEX_RECOVERY: managed session directory is invalid".into()
                            );
                        }
                        if session_dirs.len() >= MAX_SESSION_COUNT {
                            return Err(
                                "CODEX_RECOVERY: managed session count exceeds report bound".into(),
                            );
                        }
                        session_dirs.push((name, entry.path()));
                    }
                }
            }
            session_dirs.sort_by(|left, right| left.0.cmp(&right.0));
            for (session_id, session_dir) in session_dirs {
                evidence_hasher.update(session_id.as_bytes());
                let snapshot: AutonomousSessionSnapshotV1 = read_json(
                    &session_dir.join("state.json"),
                    MAX_SESSION_FILE_BYTES,
                    &mut total_bytes,
                    &mut evidence_hasher,
                )?
                .ok_or("CODEX_RECOVERY: managed session state is incomplete")?;
                let queue: AutonomousQueueV1 = read_json(
                    &session_dir.join("queue.json"),
                    MAX_SESSION_FILE_BYTES,
                    &mut total_bytes,
                    &mut evidence_hasher,
                )?
                .ok_or("CODEX_RECOVERY: managed queue is incomplete")?;
                if snapshot.schema_version != AUTONOMY_STATE_SCHEMA_VERSION
                    || snapshot.session_id != session_id
                    || snapshot.active == snapshot.state.is_terminal()
                    || queue.validate().is_err()
                {
                    return Err("CODEX_RECOVERY: managed state is inconsistent".into());
                }
                total_sessions += 1;
                if matches!(
                    snapshot.state,
                    AutonomousSessionStateV1::Running
                        | AutonomousSessionStateV1::Verifying
                        | AutonomousSessionStateV1::RecoveringAfterRestart
                ) {
                    push_bounded_row(
                        &mut session_rows,
                        json!({
                            "sessionId":session_id, "sessionState":snapshot.state,
                            "currentTaskId":snapshot.current_task_id.as_deref().map(safe_id),
                        }),
                        &mut total_rows,
                    )?;
                }
                let checkpoint = if queue.tasks.iter().any(|task| inflight_task(&task.state)) {
                    let verification_dir = session_dir.join("verification");
                    if checked_directory(&verification_dir)? {
                        read_json::<AutonomousPassedVerificationCheckpointV1>(
                            &verification_dir.join("passed-finalization.json"),
                            MAX_SESSION_FILE_BYTES,
                            &mut total_bytes,
                            &mut evidence_hasher,
                        )?
                        .map(|checkpoint| CheckpointFacts::checked(checkpoint, &session_id))
                    } else {
                        None
                    }
                } else {
                    None
                };
                let session_facts = Arc::new(SessionFacts {
                    session_id: session_id.clone(),
                    session_state: snapshot.state.clone(),
                    current_task_id: snapshot.current_task_id.clone(),
                    provider_thread_id: snapshot.provider_thread_id.clone(),
                    expected_codex_thread_id: snapshot.expected_codex_thread_id.clone(),
                    approved_contract_hash: snapshot.approved_contract_hash.clone(),
                    provider_turn_count: snapshot.provider_turn_count,
                    codex_profile_tracking_started: snapshot.codex_turn_profile_tracking_started,
                    codex_profile_violation: snapshot.codex_turn_profile_violation,
                    checkpoint,
                });
                for task in queue
                    .tasks
                    .into_iter()
                    .filter(|task| inflight_task(&task.state))
                {
                    observed_claims.push(ClaimFacts {
                        session: Arc::clone(&session_facts),
                        task_id: task.task_id.clone(),
                        task_state: task.state.clone(),
                    });
                    push_bounded_row(
                        &mut queue_rows,
                        json!({
                            "sessionId":session_id, "sessionState":snapshot.state,
                            "taskId":safe_id(&task.task_id), "taskState":task.state,
                        }),
                        &mut total_rows,
                    )?;
                }
            }
            let ledger: Option<ExecutionAccountingLedgerV1> = read_json(
                &state_root.join("execution-accounting.json"),
                MAX_LEDGER_BYTES,
                &mut total_bytes,
                &mut evidence_hasher,
            )?;
            if let Some(ref ledger) = ledger {
                if ledger.schema_version != EXECUTION_ACCOUNTING_SCHEMA_VERSION
                    || ledger.records.len() > MAX_ACCOUNTING_RECORDS
                {
                    return Err(
                        "CODEX_RECOVERY: accounting ledger has an unsupported bound or schema"
                            .into(),
                    );
                }
                validate_accounting_ledger(ledger)
                    .map_err(|_| "CODEX_RECOVERY: accounting ledger is invalid".to_string())?;
                total_accounting_records = ledger.records.len();
                for record in &ledger.records {
                    if record.project_id != project.project_id {
                        return Err(
                            "CODEX_RECOVERY: accounting ledger contains a foreign project binding"
                                .into(),
                        );
                    }
                    if record.schema_version != EXECUTION_ACCOUNTING_SCHEMA_VERSION
                        || record.activity_spans.len() > MAX_SPANS_PER_RECORD
                    {
                        return Err(
                            "CODEX_RECOVERY: accounting record has an unsupported bound or schema"
                                .into(),
                        );
                    }
                    let open_spans = record
                        .activity_spans
                        .iter()
                        .filter(|span| {
                            span.status == ActivitySpanStatusV1::Open
                                || span.ended_at_unix_millis.is_none()
                        })
                        .collect::<Vec<_>>();
                    if record.ended_at_unix_millis.is_none() || !open_spans.is_empty() {
                        let open_actors = open_spans
                            .iter()
                            .map(|span| serde_json::to_string(&span.actor).unwrap_or_default())
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .map(|encoded| {
                                serde_json::from_str::<Value>(&encoded).unwrap_or(Value::Null)
                            })
                            .collect::<Vec<_>>();
                        push_bounded_row(
                            &mut ledger_rows,
                            json!({
                                "recordId":safe_id(&record.record_id),
                                "sessionId":safe_id(&record.catdesk_session_id),
                                "taskId":safe_id(&record.task_id),
                                "endedAtRecorded":record.ended_at_unix_millis.is_some(),
                                "openSpanCount":open_spans.len(), "openActors":open_actors,
                            }),
                            &mut total_rows,
                        )?;
                    }
                }
            }
            for claim in observed_claims {
                let gaps = claim_gap_codes(
                    &claim,
                    ledger
                        .as_ref()
                        .map_or(&[][..], |ledger| ledger.records.as_slice()),
                    &project.project_id,
                );
                for code in &gaps {
                    *claim_gap_counts.entry(code).or_default() += 1;
                }
                push_bounded_row(
                    &mut claim_rows,
                    json!({
                        "claimSha256":claim_hash(&claim.session.session_id, &claim.task_id),
                        "sessionState":claim.session.session_state,
                        "taskState":claim.task_state,
                        "gapCodes":gaps,
                    }),
                    &mut total_rows,
                )?;
            }
        }
    }
    let blocker_count = session_rows.len()
        + queue_rows.len()
        + ledger_rows.len()
        + usize::from(marker_sha256.is_some());
    let mut response = json!({
        "action":if prepare { "codex_recovery_prepare" } else { "codex_recovery_status" },
        "status":if blocker_count == 0 { "NO_RECORDED_BLOCKER_OBSERVED" } else { "NEEDS_OWNER_RECOVERY" },
        "readOnly":true, "launchAuthorized":false,
        "projectId":project.project_id, "workspace":project.workspace,
        "threadId":project.codex_thread_id,
        "observedSessionCount":total_sessions,
        "observedAccountingRecordCount":total_accounting_records,
        "sessionBlockers":session_rows, "queueBlockers":queue_rows,
        "accountingReviewRows":ledger_rows,
        "claimEvidenceGapsV1":claim_rows,
        "claimEvidenceGapCountsV1":claim_gap_counts,
        "pendingWriterMarker":marker_sha256.as_ref().map(|sha| json!({"present":true,"contentSha256":sha})).unwrap_or_else(|| json!({"present":false})),
        "evidenceSha256":format!("{:x}", evidence_hasher.finalize()),
        "observationLimit":"Nontransactional read-only snapshot; concurrent same-user directory replacement may race preflight. Process exit and OS lock ownership are not proven. evidenceSha256 is a diagnostic digest, not a recovery compare-and-swap token.",
    });
    if prepare {
        response["requiredEvidence"] = json!([
            "Identify the exact owning process and session for every in-flight queue task.",
            "Prove native child exit or reviewed ownership handoff before changing durable state.",
            "Reconcile open accounting spans and queue state against source-bound receipts.",
            "Re-run recovery status after a separately reviewed reconciliation."
        ]);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegated::autonomy_accounting::{ActivityActorV1, ActivityEvidenceV1};
    use crate::delegated::autonomy_state::{AutonomousQueueTaskV1, AutonomousStateStoreV1};
    use crate::delegated::codex_model_policy::{CodexCliTurnProfileEvidenceV1, SOL_MEDIUM_MODEL};
    use crate::delegated::coordinator::{VerificationStatusV1, VerificationSummaryV1};
    use crate::reviewed_source_snapshot::{
        ReviewedSourceBaselineObservationV1, ReviewedSourceCurrentOutputV1,
        ReviewedSourceSnapshotExpectedV1,
    };

    fn fixture_project(root: &Path) -> AutonomousProjectV1 {
        AutonomousProjectV1 {
            project_id: "fixture".into(),
            workspace: root.canonicalize().expect("root"),
            git_identity: "fixture".into(),
            verification_profile: "rust_full".into(),
            codex_thread_id: None,
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
        }
    }

    fn remove_fixture(root: &Path) {
        let resolved = root.canonicalize().expect("fixture root");
        let temp = std::env::temp_dir().canonicalize().expect("temp root");
        assert!(resolved.starts_with(&temp) && resolved != temp);
        fs::remove_dir_all(resolved).expect("fixture cleanup");
    }

    fn write_fixture_snapshot(root: &Path, snapshot: &AutonomousSessionSnapshotV1) {
        fs::write(
            root.join(".catdesk/autonomy")
                .join(&snapshot.session_id)
                .join("state.json"),
            serde_json::to_vec_pretty(snapshot).expect("snapshot JSON"),
        )
        .expect("fixture snapshot");
    }

    fn has_gap(report: &Value, claim_index: usize, code: &str) -> bool {
        report["claimEvidenceGapsV1"][claim_index]["gapCodes"]
            .as_array()
            .expect("gap codes")
            .contains(&json!(code))
    }

    #[test]
    fn reports_terminal_session_with_live_queue_without_mutating_state() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("state fixture");
        store
            .create_session(
                "session_one",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "task_one".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::WorkerRunning,
                }])
                .expect("queue"),
            )
            .expect("session");
        let mut snapshot = store.load_session("session_one").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Cancelled;
        snapshot.active = false;
        store.save_session(&snapshot).expect("terminal snapshot");
        let state_path = root.join(".catdesk/autonomy/session_one/state.json");
        let before = fs::read(&state_path).expect("before");
        let project = fixture_project(&root);
        let report = report(&project, true).expect("read-only report");
        assert_eq!(report["status"], "NEEDS_OWNER_RECOVERY");
        assert_eq!(report["queueBlockers"][0]["taskState"], "WORKER_RUNNING");
        assert_eq!(report["queueBlockers"][0]["sessionState"], "CANCELLED");
        assert_eq!(report["claimEvidenceGapsV1"].as_array().unwrap().len(), 1);
        assert_eq!(
            report["claimEvidenceGapsV1"][0]["claimSha256"],
            claim_hash("session_one", "task_one")
        );
        assert!(has_gap(&report, 0, "OWNER_PROCESS_IDENTITY_UNRECORDED"));
        assert!(has_gap(&report, 0, "SOURCE_BOUND_CHILD_EXIT_UNPROVEN"));
        assert!(has_gap(&report, 0, "ACCOUNTING_RECORD_MISSING"));
        assert_eq!(report["launchAuthorized"], false);
        assert_eq!(fs::read(&state_path).expect("after"), before);
        remove_fixture(&root);
    }

    #[test]
    fn reports_historical_claim_mix_as_gaps_without_recovery_authority() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("state fixture");
        let accounting = store
            .execution_accounting_store()
            .expect("accounting store");
        for index in 0..35 {
            let session_id = format!("session_{index:02}");
            let task_id = format!("task_{index:02}");
            let task_state = if index < 30 {
                AutonomousQueueTaskStateV1::WorkerRunning
            } else {
                AutonomousQueueTaskStateV1::Verifying
            };
            store
                .create_session(
                    &session_id,
                    AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                        task_id: task_id.clone(),
                        priority: 1,
                        depends_on: Vec::new(),
                        state: task_state,
                    }])
                    .expect("queue"),
                )
                .expect("session");
            let provider_id = if index < 27 {
                "codex-cli"
            } else if index < 33 {
                "chatgpt-direct"
            } else {
                "ollama"
            };
            let thread_id = format!("thread_{index:02}");
            let provider_thread = (provider_id == "codex-cli").then_some(thread_id.as_str());
            let mut snapshot = store.load_session(&session_id).expect("snapshot");
            snapshot.state = if index < 29 {
                AutonomousSessionStateV1::Cancelled
            } else if index < 34 {
                AutonomousSessionStateV1::LeaseExpired
            } else {
                AutonomousSessionStateV1::Paused
            };
            snapshot.active = !snapshot.state.is_terminal();
            snapshot.current_task_id = Some(task_id.clone());
            snapshot.provider_turn_count = 1;
            snapshot.provider_thread_id = provider_thread.map(str::to_owned);
            snapshot.expected_codex_thread_id = provider_thread.map(str::to_owned);
            snapshot.codex_turn_profile_tracking_started = false;
            write_fixture_snapshot(&root, &snapshot);
            accounting
                .record_task_started(
                    "fixture",
                    &task_id,
                    &session_id,
                    provider_thread,
                    provider_id,
                    None,
                    None,
                    100,
                )
                .expect("accounting record");
            if index < 13 {
                accounting
                    .activity_started(
                        &session_id,
                        ActivityActorV1::CodexProviderActive,
                        ActivityEvidenceV1::ProviderLifecycle,
                        110,
                    )
                    .expect("open provider span");
            }
        }
        let project = fixture_project(&root);
        let report = report(&project, true).expect("historical fixture report");
        assert_eq!(report["queueBlockers"].as_array().unwrap().len(), 35);
        assert_eq!(report["accountingReviewRows"].as_array().unwrap().len(), 35);
        assert_eq!(report["claimEvidenceGapsV1"].as_array().unwrap().len(), 35);
        assert_eq!(
            report["claimEvidenceGapCountsV1"]["ACCOUNTING_RECORD_UNENDED"],
            35
        );
        assert_eq!(
            report["claimEvidenceGapCountsV1"]["ACCOUNTING_OPEN_SPAN"],
            13
        );
        assert_eq!(
            report["claimEvidenceGapCountsV1"]["CODEX_TURN_PROFILE_INCOMPLETE"],
            27
        );
        assert_eq!(
            report["claimEvidenceGapCountsV1"]["OWNER_PROCESS_IDENTITY_UNRECORDED"],
            35
        );
        assert_eq!(
            report["claimEvidenceGapCountsV1"]["SOURCE_BOUND_CHILD_EXIT_UNPROVEN"],
            35
        );
        assert_eq!(report["launchAuthorized"], false);
        assert!(
            report["claimEvidenceGapsV1"]
                .to_string()
                .contains("sha256:")
        );
        assert!(
            !report["claimEvidenceGapsV1"]
                .to_string()
                .contains("session_00")
        );
        remove_fixture(&root);
    }

    #[test]
    fn reports_mismatched_thread_task_and_checkpoint_bindings() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("state fixture");
        store
            .create_session(
                "session_one",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "task_one".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::WorkerRunning,
                }])
                .expect("queue"),
            )
            .expect("session");
        let mut snapshot = store.load_session("session_one").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Cancelled;
        snapshot.active = false;
        snapshot.current_task_id = Some("task_other".into());
        snapshot.provider_thread_id = Some("thread_observed".into());
        snapshot.expected_codex_thread_id = Some("thread_expected".into());
        snapshot.approved_contract_hash = Some("fnv1a64:0000000000000000".into());
        snapshot.provider_turn_count = 1;
        write_fixture_snapshot(&root, &snapshot);
        store
            .execution_accounting_store()
            .expect("accounting store")
            .record_task_started(
                "fixture",
                "task_one",
                "session_one",
                Some("thread_recorded"),
                "codex-cli",
                None,
                None,
                100,
            )
            .expect("accounting record");
        let checkpoint_contract = "fnv1a64:1111111111111111";
        let diff = "fixture diff";
        let attribution =
            serde_json::to_vec(&(checkpoint_contract, "task_else", Option::<()>::None))
                .expect("attribution JSON");
        let checkpoint = AutonomousPassedVerificationCheckpointV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            session_id: "session_one".into(),
            project_id: "fixture".into(),
            approved_contract_hash: checkpoint_contract.into(),
            logical_task_id: "task_else".into(),
            provider_id: "codex-cli".into(),
            provider_turn_count: 2,
            verification_profile: "rust_full".into(),
            verification: VerificationSummaryV1 {
                status: VerificationStatusV1::Passed,
                command: "cargo test".into(),
                summary: "passed".into(),
            },
            authoritative_diff: diff.into(),
            authoritative_diff_sha256: format!("{:x}", Sha256::digest(diff.as_bytes())),
            attribution_digest: format!("{:x}", Sha256::digest(attribution)),
            codex_turn_profile: None,
            reviewed_source_expectation: None,
        };
        store
            .write_passed_verification_checkpoint("session_one", &checkpoint)
            .expect("checkpoint fixture");
        let report = report(&fixture_project(&root), false).expect("mismatch report");
        for code in [
            "CURRENT_TASK_MISMATCH",
            "CODEX_THREAD_BINDING_MISMATCH",
            "CHECKPOINT_SESSION_STATE_INELIGIBLE",
            "CHECKPOINT_BINDING_MISMATCH",
            "CHECKPOINT_CODEX_PROFILE_MISSING",
            "CHECKPOINT_SOURCE_EXPECTATION_MISSING",
        ] {
            assert!(has_gap(&report, 0, code), "missing {code}");
        }
        assert_eq!(report["launchAuthorized"], false);
        assert!(
            !report["claimEvidenceGapsV1"]
                .to_string()
                .contains("thread_observed")
        );
        remove_fixture(&root);
    }

    #[test]
    fn tampered_checkpoint_is_only_reported_as_invalid() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("state fixture");
        store
            .create_session(
                "session_one",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "task_one".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::Verifying,
                }])
                .expect("queue"),
            )
            .expect("session");
        let contract_hash = "fnv1a64:1111111111111111";
        let mut snapshot = store.load_session("session_one").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Cancelled;
        snapshot.active = false;
        snapshot.current_task_id = Some("task_one".into());
        snapshot.approved_contract_hash = Some(contract_hash.into());
        snapshot.provider_turn_count = 1;
        write_fixture_snapshot(&root, &snapshot);
        let expectation = ReviewedSourceSnapshotExpectedV1 {
            session_id: "session_one".into(),
            project_id: "fixture".into(),
            approved_contract_hash: contract_hash.into(),
            logical_task_id: "task_one".into(),
            completion_artifact_ids: vec!["artifact_one".into()],
            current_outputs: vec![ReviewedSourceCurrentOutputV1 {
                artifact_id: "artifact_one".into(),
                sha256: "0".repeat(64),
            }],
            baseline_observations: vec![ReviewedSourceBaselineObservationV1 {
                artifact_id: "artifact_one".into(),
                state: "ABSENT".into(),
                sha256: None,
            }],
        };
        let diff = "fixture diff";
        let attribution = serde_json::to_vec(&(contract_hash, "task_one", Some(&expectation)))
            .expect("attribution JSON");
        let checkpoint = AutonomousPassedVerificationCheckpointV1 {
            schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
            session_id: "session_one".into(),
            project_id: "fixture".into(),
            approved_contract_hash: contract_hash.into(),
            logical_task_id: "task_one".into(),
            provider_id: "chatgpt-direct".into(),
            provider_turn_count: 1,
            verification_profile: "rust_full".into(),
            verification: VerificationSummaryV1 {
                status: VerificationStatusV1::Passed,
                command: "cargo test".into(),
                summary: "passed".into(),
            },
            authoritative_diff: diff.into(),
            authoritative_diff_sha256: format!("{:x}", Sha256::digest(diff.as_bytes())),
            attribution_digest: format!("{:x}", Sha256::digest(attribution)),
            codex_turn_profile: None,
            reviewed_source_expectation: Some(expectation),
        };
        store
            .write_passed_verification_checkpoint("session_one", &checkpoint)
            .expect("valid checkpoint fixture");
        let checkpoint_path =
            root.join(".catdesk/autonomy/session_one/verification/passed-finalization.json");
        for case in ["schema", "diff_hash", "attribution_hash", "expectation"] {
            let mut tampered = checkpoint.clone();
            match case {
                "schema" => tampered.schema_version = 0,
                "diff_hash" => tampered.authoritative_diff.push_str(" tampered"),
                "attribution_hash" => tampered.attribution_digest = "0".repeat(64),
                "expectation" => {
                    tampered
                        .reviewed_source_expectation
                        .as_mut()
                        .expect("expectation")
                        .session_id = "foreign_session".into();
                    let material = serde_json::to_vec(&(
                        tampered.approved_contract_hash.as_str(),
                        tampered.logical_task_id.as_str(),
                        tampered.reviewed_source_expectation.as_ref(),
                    ))
                    .expect("updated attribution JSON");
                    tampered.attribution_digest = format!("{:x}", Sha256::digest(material));
                }
                _ => unreachable!(),
            }
            fs::write(
                &checkpoint_path,
                serde_json::to_vec_pretty(&tampered).expect("tampered JSON"),
            )
            .expect("tampered checkpoint fixture");
            let report = report(&fixture_project(&root), false).expect("tampered report");
            assert!(has_gap(&report, 0, "CHECKPOINT_INVALID"), "case {case}");
            assert!(!has_gap(&report, 0, "CHECKPOINT_BINDING_MISMATCH"));
            assert_eq!(report["launchAuthorized"], false);
        }
        remove_fixture(&root);
    }

    #[test]
    fn zero_record_turn_count_cannot_look_like_complete_codex_evidence() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("state fixture");
        store
            .create_session(
                "session_one",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "task_one".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::WorkerRunning,
                }])
                .expect("queue"),
            )
            .expect("session");
        let mut snapshot = store.load_session("session_one").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Cancelled;
        snapshot.active = false;
        snapshot.current_task_id = Some("task_one".into());
        snapshot.provider_turn_count = 1;
        snapshot.provider_thread_id = Some("thread_one".into());
        snapshot.expected_codex_thread_id = Some("thread_one".into());
        write_fixture_snapshot(&root, &snapshot);
        store
            .execution_accounting_store()
            .expect("accounting store")
            .record_task_started(
                "fixture",
                "task_one",
                "session_one",
                Some("thread_one"),
                "codex-cli",
                None,
                None,
                100,
            )
            .expect("accounting record");
        let ledger_path = root.join(".catdesk/autonomy/execution-accounting.json");
        let mut ledger: Value =
            serde_json::from_slice(&fs::read(&ledger_path).expect("ledger")).expect("ledger JSON");
        ledger["records"][0]["providerTurns"] = json!(0);
        fs::write(
            &ledger_path,
            serde_json::to_vec_pretty(&ledger).expect("ledger JSON"),
        )
        .expect("fixture ledger mismatch");
        let report = report(&fixture_project(&root), false).expect("zero-turn report");
        assert!(has_gap(&report, 0, "ACCOUNTING_PROVIDER_TURN_COUNT_ZERO"));
        assert!(has_gap(&report, 0, "CODEX_TURN_PROFILE_INCOMPLETE"));
        assert_eq!(report["launchAuthorized"], false);
        remove_fixture(&root);
    }

    #[test]
    fn session_cumulative_turn_count_does_not_override_per_task_profile_count() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("state fixture");
        store
            .create_session(
                "session_one",
                AutonomousQueueV1::new(vec![
                    AutonomousQueueTaskV1 {
                        task_id: "task_done".into(),
                        priority: 1,
                        depends_on: Vec::new(),
                        state: AutonomousQueueTaskStateV1::CompletedVerified,
                    },
                    AutonomousQueueTaskV1 {
                        task_id: "task_current".into(),
                        priority: 2,
                        depends_on: vec!["task_done".into()],
                        state: AutonomousQueueTaskStateV1::WorkerRunning,
                    },
                ])
                .expect("queue"),
            )
            .expect("session");
        let mut snapshot = store.load_session("session_one").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Cancelled;
        snapshot.active = false;
        snapshot.current_task_id = Some("task_current".into());
        snapshot.provider_turn_count = 2;
        snapshot.provider_thread_id = Some("thread_one".into());
        snapshot.expected_codex_thread_id = Some("thread_one".into());
        write_fixture_snapshot(&root, &snapshot);
        let accounting = store
            .execution_accounting_store()
            .expect("accounting store");
        accounting
            .record_task_started(
                "fixture",
                "task_current",
                "session_one",
                Some("thread_one"),
                "codex-cli",
                Some(SOL_MEDIUM_MODEL),
                None,
                100,
            )
            .expect("current task record");
        accounting
            .record_exact_codex_turn_profile(
                "session_one",
                &CodexCliTurnProfileEvidenceV1 {
                    codex_thread_id: "thread_one".into(),
                    codex_turn_id: "turn-current".into(),
                    model_id: SOL_MEDIUM_MODEL.into(),
                    reasoning_effort: "medium".into(),
                    evidence_sha256: "b".repeat(64),
                },
            )
            .expect("current task profile");
        let report = report(&fixture_project(&root), false).expect("two-task report");
        assert_eq!(report["claimEvidenceGapsV1"].as_array().unwrap().len(), 1);
        assert!(!has_gap(&report, 0, "CODEX_TURN_PROFILE_INCOMPLETE"));
        assert!(!has_gap(&report, 0, "ACCOUNTING_PROVIDER_TURN_COUNT_ZERO"));
        assert!(has_gap(&report, 0, "ACCOUNTING_RECORD_UNENDED"));
        assert_eq!(report["launchAuthorized"], false);
        remove_fixture(&root);
    }

    #[test]
    fn malformed_accounting_profile_fails_before_claim_classification() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("state fixture");
        store
            .create_session(
                "session_one",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "task_one".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::WorkerRunning,
                }])
                .expect("queue"),
            )
            .expect("session");
        store
            .execution_accounting_store()
            .expect("accounting store")
            .record_task_started(
                "fixture",
                "task_one",
                "session_one",
                Some("thread_one"),
                "codex-cli",
                Some(SOL_MEDIUM_MODEL),
                None,
                100,
            )
            .expect("accounting record");
        let ledger_path = root.join(".catdesk/autonomy/execution-accounting.json");
        let mut ledger: Value =
            serde_json::from_slice(&fs::read(&ledger_path).expect("ledger")).expect("ledger JSON");
        ledger["records"][0]["codexTurnProfiles"] = json!([{
            "codexThreadId":"thread_one",
            "codexTurnId":"turn-one",
            "modelId":SOL_MEDIUM_MODEL,
            "reasoningEffort":"medium",
            "evidenceSha256":"invalid-hash"
        }]);
        fs::write(
            &ledger_path,
            serde_json::to_vec_pretty(&ledger).expect("ledger JSON"),
        )
        .expect("tampered fixture ledger");
        let error = report(&fixture_project(&root), false).expect_err("malformed profile");
        assert!(error.contains("accounting ledger is invalid"));
        assert!(!error.contains("thread_one"));
        remove_fixture(&root);
    }

    #[test]
    fn durable_profile_violation_is_reported_even_with_a_complete_profile_count() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("state fixture");
        store
            .create_session(
                "session_one",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "task_one".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::WorkerRunning,
                }])
                .expect("queue"),
            )
            .expect("session");
        let mut snapshot = store.load_session("session_one").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Cancelled;
        snapshot.active = false;
        snapshot.current_task_id = Some("task_one".into());
        snapshot.provider_turn_count = 1;
        snapshot.provider_thread_id = Some("thread_one".into());
        snapshot.expected_codex_thread_id = Some("thread_one".into());
        snapshot.codex_turn_profile_violation = true;
        write_fixture_snapshot(&root, &snapshot);
        let accounting = store
            .execution_accounting_store()
            .expect("accounting store");
        accounting
            .record_task_started(
                "fixture",
                "task_one",
                "session_one",
                Some("thread_one"),
                "codex-cli",
                Some(SOL_MEDIUM_MODEL),
                None,
                100,
            )
            .expect("accounting record");
        accounting
            .record_exact_codex_turn_profile(
                "session_one",
                &CodexCliTurnProfileEvidenceV1 {
                    codex_thread_id: "thread_one".into(),
                    codex_turn_id: "turn-one".into(),
                    model_id: SOL_MEDIUM_MODEL.into(),
                    reasoning_effort: "medium".into(),
                    evidence_sha256: "a".repeat(64),
                },
            )
            .expect("exact profile evidence");
        let report = report(&fixture_project(&root), false).expect("violation report");
        assert!(has_gap(&report, 0, "CODEX_TURN_PROFILE_VIOLATION"));
        assert!(!has_gap(&report, 0, "CODEX_TURN_PROFILE_INCOMPLETE"));
        assert_eq!(report["launchAuthorized"], false);
        remove_fixture(&root);
    }

    #[test]
    fn valid_shaped_profile_rebound_to_other_thread_or_model_is_a_gap() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("state fixture");
        store
            .create_session(
                "session_one",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "task_one".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::WorkerRunning,
                }])
                .expect("queue"),
            )
            .expect("session");
        let mut snapshot = store.load_session("session_one").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Cancelled;
        snapshot.active = false;
        snapshot.current_task_id = Some("task_one".into());
        snapshot.provider_thread_id = Some("thread_one".into());
        snapshot.expected_codex_thread_id = Some("thread_one".into());
        write_fixture_snapshot(&root, &snapshot);
        let accounting = store
            .execution_accounting_store()
            .expect("accounting store");
        accounting
            .record_task_started(
                "fixture",
                "task_one",
                "session_one",
                Some("thread_one"),
                "codex-cli",
                Some(SOL_MEDIUM_MODEL),
                None,
                100,
            )
            .expect("accounting record");
        accounting
            .record_exact_codex_turn_profile(
                "session_one",
                &CodexCliTurnProfileEvidenceV1 {
                    codex_thread_id: "thread_one".into(),
                    codex_turn_id: "turn-one".into(),
                    model_id: SOL_MEDIUM_MODEL.into(),
                    reasoning_effort: "medium".into(),
                    evidence_sha256: "a".repeat(64),
                },
            )
            .expect("exact profile");
        let ledger_path = root.join(".catdesk/autonomy/execution-accounting.json");
        let original: Value =
            serde_json::from_slice(&fs::read(&ledger_path).expect("ledger")).expect("ledger JSON");
        for case in ["thread", "model", "missing_model"] {
            let mut rebound = original.clone();
            if case == "thread" {
                rebound["records"][0]["codexTurnProfiles"][0]["codexThreadId"] =
                    json!("thread_rebound");
            } else if case == "model" {
                rebound["records"][0]["codexTurnProfiles"][0]["modelId"] = json!("gpt-5.6-terra");
                rebound["records"][0]["codexTurnProfiles"][0]["reasoningEffort"] = json!("high");
            } else {
                rebound["records"][0]["requestedModel"] = Value::Null;
            }
            fs::write(
                &ledger_path,
                serde_json::to_vec_pretty(&rebound).expect("rebound JSON"),
            )
            .expect("rebound fixture ledger");
            let report = report(&fixture_project(&root), false).expect("rebound report");
            let expected = if case == "missing_model" {
                "CODEX_TURN_PROFILE_BINDING_MISSING"
            } else {
                "CODEX_TURN_PROFILE_BINDING_MISMATCH"
            };
            assert!(has_gap(&report, 0, expected), "case {case}");
            assert!(!has_gap(&report, 0, "CODEX_TURN_PROFILE_INCOMPLETE"));
            assert_eq!(report["launchAuthorized"], false);
        }
        remove_fixture(&root);
    }

    #[test]
    fn many_claims_share_oversized_session_metadata_without_exposing_it() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("state fixture");
        let tasks = (0..128)
            .map(|index| AutonomousQueueTaskV1 {
                task_id: format!("task_{index:03}"),
                priority: 1,
                depends_on: Vec::new(),
                state: AutonomousQueueTaskStateV1::WorkerRunning,
            })
            .collect::<Vec<_>>();
        store
            .create_session("session_one", AutonomousQueueV1::new(tasks).expect("queue"))
            .expect("session");
        let mut snapshot = store.load_session("session_one").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Cancelled;
        snapshot.active = false;
        snapshot.current_task_id = Some("task_000".into());
        let oversized = "private-provider-metadata-".repeat(16_000);
        snapshot.provider_thread_id = Some(oversized.clone());
        snapshot.expected_codex_thread_id = Some(oversized.clone());
        snapshot.approved_contract_hash = Some(oversized);
        snapshot.codex_turn_profile_violation = true;
        write_fixture_snapshot(&root, &snapshot);
        let report = report(&fixture_project(&root), false).expect("many-claim report");
        assert_eq!(report["claimEvidenceGapsV1"].as_array().unwrap().len(), 128);
        assert_eq!(
            report["claimEvidenceGapCountsV1"]["CODEX_TURN_PROFILE_VIOLATION"],
            128
        );
        assert_eq!(
            report["claimEvidenceGapCountsV1"]["ACCOUNTING_RECORD_MISSING"],
            128
        );
        assert!(!report.to_string().contains("private-provider-metadata-"));
        assert!(report.to_string().len() < 100_000);
        assert_eq!(report["launchAuthorized"], false);
        remove_fixture(&root);
    }

    #[test]
    fn reports_ended_accounting_record_with_open_span_as_inconsistent() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("state fixture");
        store
            .create_session(
                "session_one",
                AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
                    task_id: "task_one".into(),
                    priority: 1,
                    depends_on: Vec::new(),
                    state: AutonomousQueueTaskStateV1::Verifying,
                }])
                .expect("queue"),
            )
            .expect("session");
        let mut snapshot = store.load_session("session_one").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::LeaseExpired;
        snapshot.active = false;
        snapshot.current_task_id = Some("task_one".into());
        write_fixture_snapshot(&root, &snapshot);
        let accounting = store
            .execution_accounting_store()
            .expect("accounting store");
        accounting
            .record_task_started(
                "fixture",
                "task_one",
                "session_one",
                None,
                "chatgpt-direct",
                None,
                None,
                100,
            )
            .expect("accounting record");
        accounting
            .activity_started(
                "session_one",
                ActivityActorV1::ChatgptWebActive,
                ActivityEvidenceV1::ChatgptExactTargetGeneratingObservation,
                110,
            )
            .expect("open span");
        accounting
            .finish_task("session_one", None, None, 120)
            .expect("ended record");
        let report = report(&fixture_project(&root), false).expect("ended/open report");
        assert_eq!(report["accountingReviewRows"][0]["endedAtRecorded"], true);
        assert!(has_gap(&report, 0, "ACCOUNTING_OPEN_SPAN"));
        assert!(has_gap(&report, 0, "ACCOUNTING_ENDED_WITH_OPEN_SPAN"));
        assert!(!has_gap(&report, 0, "ACCOUNTING_RECORD_UNENDED"));
        assert_eq!(report["launchAuthorized"], false);
        remove_fixture(&root);
    }

    #[test]
    fn reports_open_accounting_span_without_disclosing_provider_metadata() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("state fixture");
        let accounting = store
            .execution_accounting_store()
            .expect("accounting store");
        accounting
            .record_task_started(
                "fixture",
                "task_one",
                "session_one",
                Some("opaque-provider-handle"),
                "chatgpt-direct",
                None,
                None,
                100,
            )
            .expect("start accounting");
        accounting
            .activity_started(
                "session_one",
                ActivityActorV1::ChatgptWebActive,
                ActivityEvidenceV1::ChatgptExactTargetGeneratingObservation,
                110,
            )
            .expect("open span");
        let ledger_path = root.join(".catdesk/autonomy/execution-accounting.json");
        let before = fs::read(&ledger_path).expect("ledger before");
        let report = report(&fixture_project(&root), false).expect("read-only report");
        assert_eq!(report["observedAccountingRecordCount"], 1);
        assert_eq!(report["accountingReviewRows"][0]["openSpanCount"], 1);
        assert_eq!(
            report["accountingReviewRows"][0]["openActors"][0],
            "CHATGPT_WEB_ACTIVE"
        );
        assert!(!report.to_string().contains("opaque-provider-handle"));
        assert_eq!(fs::read(&ledger_path).expect("ledger after"), before);
        remove_fixture(&root);
    }

    #[test]
    fn absent_state_report_does_not_create_state_directories() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let report = report(&fixture_project(&root), false).expect("empty report");
        assert_eq!(report["status"], "NO_RECORDED_BLOCKER_OBSERVED");
        assert!(!root.join(".catdesk").exists());
        remove_fixture(&root);
    }

    #[test]
    fn aggregate_evidence_budget_refuses_a_second_read() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let path = root.join("small.json");
        fs::write(&path, b"{}").expect("fixture JSON");
        let mut total = MAX_AGGREGATE_EVIDENCE_BYTES - 3;
        let mut digest = Sha256::new();
        let first: Option<Value> =
            read_json(&path, 16, &mut total, &mut digest).expect("first bounded read");
        assert!(first.is_some());
        assert_eq!(total, MAX_AGGREGATE_EVIDENCE_BYTES - 1);
        let second: Result<Option<Value>, String> = read_json(&path, 16, &mut total, &mut digest);
        assert!(
            second.is_err(),
            "second read must not exceed aggregate budget"
        );
        remove_fixture(&root);
    }

    #[test]
    fn aggregate_row_limit_refuses_further_rows() {
        let mut rows = vec![Value::Null; MAX_REPORT_ROWS];
        let mut total = MAX_REPORT_ROWS;
        assert!(push_bounded_row(&mut rows, Value::Null, &mut total).is_err());
        assert_eq!(rows.len(), MAX_REPORT_ROWS);
    }

    #[test]
    fn foreign_project_accounting_fails_without_disclosing_record_ids() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("fixture root");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).expect("state fixture");
        store
            .execution_accounting_store()
            .expect("accounting store")
            .record_task_started(
                "other_project",
                "secret_task_identifier",
                "secret_session_identifier",
                None,
                "chatgpt-direct",
                None,
                None,
                100,
            )
            .expect("foreign record fixture");
        let error = report(&fixture_project(&root), false)
            .expect_err("foreign project record cannot be attributed to selected project");
        assert!(error.contains("foreign project binding"));
        assert!(!error.contains("secret_task_identifier"));
        assert!(!error.contains("secret_session_identifier"));
        remove_fixture(&root);
    }

    #[test]
    fn directory_entry_bound_is_checked_before_session_reads() {
        let root = std::env::temp_dir().join(format!("catdesk-recovery-{}", uuid::Uuid::new_v4()));
        let state_root = root.join(".catdesk/autonomy");
        fs::create_dir_all(&state_root).expect("fixture state root");
        for index in 0..=MAX_DIRECTORY_ENTRIES {
            fs::write(state_root.join(format!("extra-{index}.txt")), b"")
                .expect("fixture directory entry");
        }
        let error = report(&fixture_project(&root), false)
            .expect_err("oversized directory enumeration must fail early");
        assert!(error.contains("directory entry count"));
        remove_fixture(&root);
    }
}
