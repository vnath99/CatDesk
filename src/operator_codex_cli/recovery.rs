//! Read-only inventory of evidence that can prevent a direct Codex turn.
//!
//! This deliberately does not acquire or clear a writer lock, open a state
//! store (whose constructor creates directories), or infer that a native
//! provider child has exited from a terminal session label.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::delegated::autonomy_accounting::{
    ActivitySpanStatusV1, EXECUTION_ACCOUNTING_SCHEMA_VERSION, ExecutionAccountingLedgerV1,
};
use crate::delegated::autonomy_projects::AutonomousProjectV1;
use crate::delegated::autonomy_runtime::inspect_project_writer_pending_marker;
use crate::delegated::autonomy_state::{
    AUTONOMY_STATE_SCHEMA_VERSION, AutonomousQueueTaskStateV1, AutonomousQueueV1,
    AutonomousSessionSnapshotV1, AutonomousSessionStateV1,
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
                for task in queue
                    .tasks
                    .into_iter()
                    .filter(|task| inflight_task(&task.state))
                {
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
            if let Some(ledger) = ledger {
                if ledger.schema_version != EXECUTION_ACCOUNTING_SCHEMA_VERSION
                    || ledger.records.len() > MAX_ACCOUNTING_RECORDS
                {
                    return Err(
                        "CODEX_RECOVERY: accounting ledger has an unsupported bound or schema"
                            .into(),
                    );
                }
                total_accounting_records = ledger.records.len();
                for record in ledger.records {
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
        assert_eq!(report["launchAuthorized"], false);
        assert_eq!(fs::read(&state_path).expect("after"), before);
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
