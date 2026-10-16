//! A local operator can record a decision about one captured, verified queue
//! package, even if the source worktree has since changed. This record is not
//! an independent ChatGPT review or Wake authorization.

use super::*;
use std::io::{self, IsTerminal, Read};

const MAX_OPERATOR_REVIEW_BYTES: u64 = 4096;
const MAX_CONFIRMATION_BYTES: usize = 256;
const OPERATOR_REVIEW_AUTHORITY: &str = "OPERATOR_REVIEW_ONLY";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OperatorReviewRecordV1 {
    schema_version: u32,
    authority: String,
    project_id: String,
    task_id: String,
    receipt_id: String,
    receipt_sha256: String,
    review_candidate_sha256: String,
    verification_sha256: String,
    package_sha256: String,
    project_binding_sha256: String,
    target_sha256: Option<String>,
    decision: String,
    reviewed_at_unix: u64,
    independent_review: String,
    wake_ready: bool,
    checksum_sha256: String,
}

fn valid_decision(decision: &str) -> bool {
    matches!(decision, "ACCEPT" | "REJECT" | "INCONCLUSIVE")
}

fn exact_confirmation(answer: &str, decision: &str, receipt_id: &str, sha256: &str) -> bool {
    valid_decision(decision)
        && answer.trim_end_matches(['\r', '\n']) == format!("{decision} {receipt_id} {sha256}")
}

fn read_confirmation_line(mut reader: impl Read) -> Result<String, String> {
    let mut bytes = Vec::with_capacity(128);
    for _ in 0..MAX_CONFIRMATION_BYTES {
        let mut byte = [0];
        reader
            .read_exact(&mut byte)
            .map_err(|_| "CODEX_QUEUE_OPERATOR_REVIEW: terminal confirmation was incomplete")?;
        bytes.push(byte[0]);
        if byte[0] == b'\n' {
            return String::from_utf8(bytes).map_err(|_| {
                "CODEX_QUEUE_OPERATOR_REVIEW: terminal confirmation is not UTF-8".into()
            });
        }
    }
    Err("CODEX_QUEUE_OPERATOR_REVIEW: terminal confirmation exceeds 256 bytes".into())
}

fn record_path(store: &QueueStore, receipt_id: &str) -> PathBuf {
    store
        .root
        .join("receipts")
        .join(format!("{receipt_id}.operator-review.json"))
}

fn expected_record(
    project: &AutonomousProjectV1,
    package: &QueueReviewPackage,
    package_sha256: &str,
    decision: &str,
    reviewed_at_unix: u64,
) -> Result<OperatorReviewRecordV1, String> {
    if !valid_decision(decision) || reviewed_at_unix == 0 {
        return Err("CODEX_QUEUE_OPERATOR_REVIEW: invalid decision or clock".into());
    }
    let mut record = OperatorReviewRecordV1 {
        schema_version: 1,
        authority: OPERATOR_REVIEW_AUTHORITY.into(),
        project_id: project.project_id.clone(),
        task_id: package.receipt.task_id.clone(),
        receipt_id: package.receipt_id.clone(),
        receipt_sha256: package.receipt_sha256.clone(),
        review_candidate_sha256: package.review_candidate_sha256.clone(),
        verification_sha256: package.verification_sha256.clone(),
        package_sha256: package_sha256.into(),
        project_binding_sha256: project_binding_sha256(project)?,
        target_sha256: project.chatgpt_target_sha256.clone(),
        decision: decision.into(),
        reviewed_at_unix,
        independent_review: "PENDING".into(),
        wake_ready: false,
        checksum_sha256: String::new(),
    };
    let bytes = serde_json::to_vec(&record)
        .map_err(|_| "CODEX_QUEUE_OPERATOR_REVIEW: decision cannot be encoded")?;
    record.checksum_sha256 = format!(
        "{:x}",
        Sha256::digest([b"CATDESK_OPERATOR_REVIEW_V1\0".as_slice(), &bytes].concat())
    );
    Ok(record)
}

fn read_record(
    store: &QueueStore,
    project: &AutonomousProjectV1,
    package: &QueueReviewPackage,
    package_sha256: &str,
) -> Result<Option<(OperatorReviewRecordV1, String)>, String> {
    let Some(bytes) = read_regular_bounded(
        &record_path(store, &package.receipt_id),
        MAX_OPERATOR_REVIEW_BYTES,
    )?
    else {
        return Ok(None);
    };
    let record: OperatorReviewRecordV1 = serde_json::from_slice(&bytes)
        .map_err(|_| "CODEX_QUEUE_OPERATOR_REVIEW: decision record is invalid")?;
    let expected = expected_record(
        project,
        package,
        package_sha256,
        &record.decision,
        record.reviewed_at_unix,
    )?;
    if record != expected {
        return Err("CODEX_QUEUE_OPERATOR_REVIEW: decision binding changed".into());
    }
    Ok(Some((record, format!("{:x}", Sha256::digest(&bytes)))))
}

fn summary(record: &OperatorReviewRecordV1, digest: &str, idempotent: bool) -> Value {
    json!({
        "action":"codex_queue_operator_review",
        "idempotent":idempotent,
        "authority":record.authority,
        "projectId":record.project_id,
        "taskId":record.task_id,
        "receiptId":record.receipt_id,
        "receiptSha256":record.receipt_sha256,
        "reviewCandidateSha256":record.review_candidate_sha256,
        "verificationSha256":record.verification_sha256,
        "packageSha256":record.package_sha256,
        "projectBindingSha256":record.project_binding_sha256,
        "targetSha256":record.target_sha256,
        "decision":record.decision,
        "reviewedAtUnix":record.reviewed_at_unix,
        "recordSha256":digest,
        "independentReview":record.independent_review,
        "wakeReady":record.wake_ready,
        "meaning":"Local operator decision about captured evidence only; no independent ChatGPT review, inbox record, or Wake authority.",
    })
}

fn decide_locked(
    store: &QueueStore,
    project: &AutonomousProjectV1,
    receipt_id: &str,
    expected_package_sha256: &str,
    decision: &str,
    create_after_confirmation: bool,
) -> Result<Option<Value>, String> {
    if !valid_decision(decision) {
        return Err(
            "CODEX_QUEUE_OPERATOR_REVIEW: decision must be ACCEPT, REJECT, or INCONCLUSIVE".into(),
        );
    }
    let package =
        review_reader::bound_package(store, project, receipt_id, expected_package_sha256)?;
    if let Some((record, digest)) = read_record(store, project, &package, expected_package_sha256)?
    {
        if record.decision != decision {
            return Err("CODEX_QUEUE_OPERATOR_REVIEW: a different decision already exists".into());
        }
        return Ok(Some(summary(&record, &digest, true)));
    }
    if !create_after_confirmation {
        return Ok(None);
    }
    let record = expected_record(
        project,
        &package,
        expected_package_sha256,
        decision,
        unix_now()?,
    )?;
    let encoded = serde_json::to_vec(&record)
        .map_err(|_| "CODEX_QUEUE_OPERATOR_REVIEW: decision cannot be encoded")?;
    if encoded.len().saturating_add(1) as u64 > MAX_OPERATOR_REVIEW_BYTES {
        return Err("CODEX_QUEUE_OPERATOR_REVIEW: decision is oversized".into());
    }
    create_new_record(&record_path(store, receipt_id), &encoded)?;
    let (readback, digest) = read_record(store, project, &package, expected_package_sha256)?
        .ok_or("CODEX_QUEUE_OPERATOR_REVIEW: decision readback is missing")?;
    if readback != record {
        return Err("CODEX_QUEUE_OPERATOR_REVIEW: decision readback changed".into());
    }
    Ok(Some(summary(&record, &digest, false)))
}

fn ensure_current_registration(
    control_workspace: &Path,
    control: &AutonomousProjectV1,
    project: &AutonomousProjectV1,
) -> Result<(), String> {
    let current_control =
        resolve_registered_project_from_control_root(control_workspace, "catdesk")
            .map_err(|_| "CODEX_QUEUE_OPERATOR_REVIEW: control registration changed")?;
    let current_project =
        resolve_registered_project_from_control_root(control_workspace, &project.project_id)
            .map_err(|_| "CODEX_QUEUE_OPERATOR_REVIEW: project registration changed")?;
    if current_control.project != *control || current_project.project != *project {
        return Err("CODEX_QUEUE_OPERATOR_REVIEW: registration changed during review".into());
    }
    Ok(())
}

fn decide_with_registration_in_store(
    store: &QueueStore,
    control_workspace: &Path,
    control: &AutonomousProjectV1,
    project: &AutonomousProjectV1,
    receipt_id: &str,
    expected_package_sha256: &str,
    decision: &str,
    create_after_confirmation: bool,
) -> Result<Option<Value>, String> {
    let _receipt_lock = store.verification_lock(receipt_id)?;
    let _queue_lock = store.lock()?;
    let registry = scheduler(control_workspace)?;
    // Target CAS and profile binding hold this registry lock when publishing.
    // Keep it through the final package check, create-once write and readback.
    registry.with_registration_lock(|| {
        ensure_current_registration(control_workspace, control, project)?;
        decide_locked(
            store,
            project,
            receipt_id,
            expected_package_sha256,
            decision,
            create_after_confirmation,
        )
    })
}

pub(super) fn record_local_decision(
    control_workspace: &Path,
    control: &AutonomousProjectV1,
    project: &AutonomousProjectV1,
    receipt_id: &str,
    expected_package_sha256: &str,
    decision: &str,
) -> Result<Value, String> {
    let store = QueueStore::open_read_only(control)?
        .ok_or("CODEX_QUEUE_OPERATOR_REVIEW: queue state is unavailable")?;
    record_local_decision_in_store(
        &store,
        control_workspace,
        control,
        project,
        receipt_id,
        expected_package_sha256,
        decision,
    )
}

fn record_local_decision_in_store(
    store: &QueueStore,
    control_workspace: &Path,
    control: &AutonomousProjectV1,
    project: &AutonomousProjectV1,
    receipt_id: &str,
    expected_package_sha256: &str,
    decision: &str,
) -> Result<Value, String> {
    if let Some(existing) = decide_with_registration_in_store(
        store,
        control_workspace,
        control,
        project,
        receipt_id,
        expected_package_sha256,
        decision,
        false,
    )? {
        return Ok(existing);
    }
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(
            "CODEX_QUEUE_OPERATOR_REVIEW: an interactive local terminal is required".into(),
        );
    }
    eprintln!(
        "Record a local operator review decision for project {}?",
        project.project_id
    );
    eprintln!("Receipt: {receipt_id}");
    eprintln!("Package SHA-256: {expected_package_sha256}");
    eprintln!("Decision: {decision}");
    eprintln!(
        "This records OPERATOR_REVIEW_ONLY. It does not approve an independent review or Wake."
    );
    eprint!("Type {decision} {receipt_id} {expected_package_sha256} to continue: ");
    io::stderr()
        .flush()
        .map_err(|_| "CODEX_QUEUE_OPERATOR_REVIEW: terminal prompt failed")?;
    let answer = read_confirmation_line(io::stdin().lock())?;
    if !exact_confirmation(&answer, decision, receipt_id, expected_package_sha256) {
        return Err("CODEX_QUEUE_OPERATOR_REVIEW: exact decision was not confirmed".into());
    }
    // The human pause grants no stale state authority. The final registration
    // comparison and package validation happen while all three locks are held.
    decide_with_registration_in_store(
        store,
        control_workspace,
        control,
        project,
        receipt_id,
        expected_package_sha256,
        decision,
        true,
    )?
    .ok_or_else(|| "CODEX_QUEUE_OPERATOR_REVIEW: decision was not published".into())
}

#[cfg(test)]
pub(super) fn test_record_local_decision_in_store(
    store: &QueueStore,
    control_workspace: &Path,
    control: &AutonomousProjectV1,
    project: &AutonomousProjectV1,
    receipt_id: &str,
    expected_package_sha256: &str,
    decision: &str,
) -> Result<Value, String> {
    record_local_decision_in_store(
        store,
        control_workspace,
        control,
        project,
        receipt_id,
        expected_package_sha256,
        decision,
    )
}

pub(super) fn read_local_decision(
    control_workspace: &Path,
    control: &AutonomousProjectV1,
    receipt_id: &str,
) -> Result<Value, String> {
    if !Uuid::parse_str(receipt_id).is_ok_and(|id| id.to_string() == receipt_id) {
        return Err("CODEX_QUEUE_OPERATOR_REVIEW: canonical receipt UUID is required".into());
    }
    let store = QueueStore::open_read_only(control)?
        .ok_or("CODEX_QUEUE_OPERATOR_REVIEW: queue state is unavailable")?;
    let _receipt_lock = store.verification_lock(receipt_id)?;
    let _queue_lock = store.lock()?;
    let registry = scheduler(control_workspace)?;
    registry.with_registration_lock(|| {
        let current_control =
            resolve_registered_project_from_control_root(control_workspace, "catdesk")
                .map_err(|_| "CODEX_QUEUE_OPERATOR_REVIEW: control registration changed")?;
        if current_control.project != *control {
            return Err("CODEX_QUEUE_OPERATOR_REVIEW: control registration changed".into());
        }
        let receipt = store.read_receipt(receipt_id)?;
        let current =
            resolve_registered_project_from_control_root(control_workspace, &receipt.project_id)
                .map_err(|_| "CODEX_QUEUE_OPERATOR_REVIEW: project registration is unavailable")?;
        let bytes =
            read_regular_bounded(&record_path(&store, receipt_id), MAX_OPERATOR_REVIEW_BYTES)?
                .ok_or("CODEX_QUEUE_OPERATOR_REVIEW: decision record is unavailable")?;
        let saved: OperatorReviewRecordV1 = serde_json::from_slice(&bytes)
            .map_err(|_| "CODEX_QUEUE_OPERATOR_REVIEW: decision record is invalid")?;
        let package = review_reader::bound_package(
            &store,
            &current.project,
            receipt_id,
            &saved.package_sha256,
        )?;
        let (record, digest) =
            read_record(&store, &current.project, &package, &saved.package_sha256)?
                .ok_or("CODEX_QUEUE_OPERATOR_REVIEW: decision record is unavailable")?;
        Ok(summary(&record, &digest, true))
    })
}

#[cfg(test)]
pub(super) fn test_decide_in_store(
    store: &QueueStore,
    project: &AutonomousProjectV1,
    receipt_id: &str,
    expected_package_sha256: &str,
    decision: &str,
    create: bool,
) -> Result<Option<Value>, String> {
    let _receipt_lock = store.verification_lock(receipt_id)?;
    let _queue_lock = store.lock()?;
    decide_locked(
        store,
        project,
        receipt_id,
        expected_package_sha256,
        decision,
        create,
    )
}

#[cfg(test)]
pub(super) fn test_decide_with_registration_in_store(
    store: &QueueStore,
    control_workspace: &Path,
    control: &AutonomousProjectV1,
    project: &AutonomousProjectV1,
    receipt_id: &str,
    expected_package_sha256: &str,
    decision: &str,
    create: bool,
) -> Result<Option<Value>, String> {
    decide_with_registration_in_store(
        store,
        control_workspace,
        control,
        project,
        receipt_id,
        expected_package_sha256,
        decision,
        create,
    )
}

#[cfg(test)]
pub(super) fn test_read_in_store(
    store: &QueueStore,
    project: &AutonomousProjectV1,
    receipt_id: &str,
    expected_package_sha256: &str,
) -> Result<Value, String> {
    let _receipt_lock = store.verification_lock(receipt_id)?;
    let _queue_lock = store.lock()?;
    let package =
        review_reader::bound_package(store, project, receipt_id, expected_package_sha256)?;
    let (record, digest) = read_record(store, project, &package, expected_package_sha256)?
        .ok_or("CODEX_QUEUE_OPERATOR_REVIEW: decision record is unavailable")?;
    Ok(summary(&record, &digest, true))
}

#[cfg(test)]
mod tests {
    use super::{exact_confirmation, read_confirmation_line};

    #[test]
    fn confirmation_binds_decision_receipt_and_package() {
        let receipt = "f318657d-a907-462d-adad-03dc39f0d911";
        let sha = "a".repeat(64);
        assert!(exact_confirmation(
            &format!("ACCEPT {receipt} {sha}\r\n"),
            "ACCEPT",
            receipt,
            &sha
        ));
        assert!(!exact_confirmation(
            &format!("REJECT {receipt} {sha}\n"),
            "ACCEPT",
            receipt,
            &sha
        ));
        assert!(!exact_confirmation(
            &format!("ACCEPT {receipt} {}\n", "b".repeat(64)),
            "ACCEPT",
            receipt,
            &sha
        ));
        assert!(!exact_confirmation(
            &format!("ACCEPT {receipt} {sha} extra\n"),
            "ACCEPT",
            receipt,
            &sha
        ));
    }

    #[test]
    fn confirmation_input_is_bounded_without_waiting_for_eof() {
        let line = read_confirmation_line(b"ACCEPT receipt sha\n".as_slice()).unwrap();
        assert_eq!(line, "ACCEPT receipt sha\n");
        let error = read_confirmation_line(std::io::repeat(b'x')).unwrap_err();
        assert!(error.contains("exceeds 256 bytes"));
        assert!(read_confirmation_line(b"no newline".as_slice()).is_err());
    }
}
