//! Persist an exact-package review invitation without publishing a Wake event.
//!
//! The queue CLI cannot yet prove that its Wake store is the installed
//! WakeHost's protected store. This create-once record is therefore historical
//! evidence, not a delivery-ready event, review, ACK, or next-task authority.
//! A future publisher must recheck live disclosure and project registration,
//! then bind the current Wake target *generation* in the protected Wake store
//! before producing any event. This record contains only the registered URL's
//! raw SHA-256 digest (the same calculation as Wake's target digest).

use super::*;
use crate::delegated::autonomy_projects::canonical_project_chat_target;

const MAX_INVITATION_BYTES: u64 = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewInvitationV1 {
    schema_version: u32,
    project_id: String,
    task_id: String,
    receipt_id: String,
    package_sha256: String,
    project_binding_sha256: String,
    target_sha256: String,
    message: String,
    prepared_at_unix: u64,
    independent_review: String,
    wake_publication: String,
    wake_ready: bool,
    checksum_sha256: String,
}

pub(super) fn registered_wake_target_sha256(project: &AutonomousProjectV1) -> Result<&str, String> {
    let url = project
        .chatgpt_target_url
        .as_deref()
        .ok_or("CODEX_QUEUE_REVIEW_INVITE: registered ChatGPT target is absent")?;
    let expected = project
        .chatgpt_target_sha256
        .as_deref()
        .ok_or("CODEX_QUEUE_REVIEW_INVITE: registered target digest is absent")?;
    if canonical_project_chat_target(url).ok().as_deref() != Some(url)
        || catdesk_wake::protocol::validate_target(url).is_err()
        || catdesk_wake::protocol::digest(url) != expected
    {
        return Err(
            "CODEX_QUEUE_REVIEW_INVITE: registered target is outside exact Wake scope".into(),
        );
    }
    Ok(expected)
}

fn record_path(store: &QueueStore, receipt_id: &str) -> PathBuf {
    store
        .root
        .join("receipts")
        .join(format!("{receipt_id}.review-invitation.json"))
}

fn expected_record(
    project: &AutonomousProjectV1,
    package: &QueueReviewPackage,
    package_sha256: &str,
    target_sha256: &str,
    prepared_at_unix: u64,
    observed_at_unix: u64,
) -> Result<ReviewInvitationV1, String> {
    if prepared_at_unix < package.recorded_at_unix || prepared_at_unix > observed_at_unix {
        return Err("CODEX_QUEUE_REVIEW_INVITE: invalid preparation chronology".into());
    }
    let message = format!(
        "CatDesk queue review request for project {}, receipt {}, package SHA-256 {}. Read this exact package with catdesk_codex_queue_review_package_read and inspect the captured source and verification evidence. This request is not an independent review decision or approval.",
        project.project_id, package.receipt_id, package_sha256
    );
    let mut record = ReviewInvitationV1 {
        schema_version: 1,
        project_id: project.project_id.clone(),
        task_id: package.receipt.task_id.clone(),
        receipt_id: package.receipt_id.clone(),
        package_sha256: package_sha256.into(),
        project_binding_sha256: project_binding_sha256(project)?,
        target_sha256: target_sha256.into(),
        message,
        prepared_at_unix,
        independent_review: "PENDING".into(),
        wake_publication: "NOT_PUBLISHED".into(),
        wake_ready: false,
        checksum_sha256: String::new(),
    };
    let encoded = serde_json::to_vec(&record)
        .map_err(|_| "CODEX_QUEUE_REVIEW_INVITE: invitation cannot be encoded")?;
    let mut hasher = Sha256::new();
    hasher.update(b"CATDESK_QUEUE_REVIEW_INVITATION_V1\0");
    hasher.update(encoded);
    record.checksum_sha256 = format!("{:x}", hasher.finalize());
    Ok(record)
}

fn read_record(
    store: &QueueStore,
    project: &AutonomousProjectV1,
    package: &QueueReviewPackage,
    package_sha256: &str,
    target_sha256: &str,
    observed_at_unix: u64,
) -> Result<Option<(ReviewInvitationV1, String)>, String> {
    let Some(bytes) = read_regular_bounded(
        &record_path(store, &package.receipt_id),
        MAX_INVITATION_BYTES,
    )?
    else {
        return Ok(None);
    };
    let record: ReviewInvitationV1 = serde_json::from_slice(&bytes)
        .map_err(|_| "CODEX_QUEUE_REVIEW_INVITE: persisted invitation is invalid")?;
    let expected = expected_record(
        project,
        package,
        package_sha256,
        target_sha256,
        record.prepared_at_unix,
        observed_at_unix,
    )?;
    if record != expected {
        return Err("CODEX_QUEUE_REVIEW_INVITE: persisted invitation binding changed".into());
    }
    let mut canonical = serde_json::to_vec(&expected)
        .map_err(|_| "CODEX_QUEUE_REVIEW_INVITE: invitation cannot be encoded")?;
    canonical.push(b'\n');
    if bytes != canonical {
        return Err("CODEX_QUEUE_REVIEW_INVITE: persisted invitation bytes changed".into());
    }
    Ok(Some((record, format!("{:x}", Sha256::digest(&bytes)))))
}

fn prepare_locked(
    store: &QueueStore,
    project: &AutonomousProjectV1,
    receipt_id: &str,
    expected_package_sha256: &str,
    clock: impl FnOnce() -> Result<u64, String>,
) -> Result<Value, String> {
    let package =
        review_reader::bound_package(store, project, receipt_id, expected_package_sha256)?;
    let target_sha256 = registered_wake_target_sha256(project)?;
    if package.receipt.target_sha256.as_deref() != Some(target_sha256) {
        return Err("CODEX_QUEUE_REVIEW_INVITE: receipt target differs from registration".into());
    }
    let observed_at_unix = clock()?;
    let (disclosure_created_at_unix, disclosure_expires_at_unix) =
        review_reader::active_disclosure_window_at(
            store,
            project,
            receipt_id,
            expected_package_sha256,
            observed_at_unix,
        )?;
    let (record, invitation_sha256, idempotent) = if let Some((record, digest)) = read_record(
        store,
        project,
        &package,
        expected_package_sha256,
        target_sha256,
        observed_at_unix,
    )? {
        (record, digest, true)
    } else {
        let record = expected_record(
            project,
            &package,
            expected_package_sha256,
            target_sha256,
            observed_at_unix,
            observed_at_unix,
        )?;
        let encoded = serde_json::to_vec(&record)
            .map_err(|_| "CODEX_QUEUE_REVIEW_INVITE: invitation cannot be encoded")?;
        if encoded.len().saturating_add(1) as u64 > MAX_INVITATION_BYTES {
            return Err("CODEX_QUEUE_REVIEW_INVITE: invitation is oversized".into());
        }
        create_new_record(&record_path(store, receipt_id), &encoded)?;
        let (readback, digest) = read_record(
            store,
            project,
            &package,
            expected_package_sha256,
            target_sha256,
            observed_at_unix,
        )?
        .ok_or("CODEX_QUEUE_REVIEW_INVITE: invitation readback is missing")?;
        if readback != record {
            return Err("CODEX_QUEUE_REVIEW_INVITE: invitation readback changed".into());
        }
        (record, digest, false)
    };
    Ok(json!({
        "action":"codex_queue_review_invite_prepare",
        "idempotent":idempotent,
        "invitation":record,
        "invitationSha256":invitation_sha256,
        "artifactPath":record_path(store, receipt_id),
        "disclosureCreatedAtUnixObserved":disclosure_created_at_unix,
        "disclosureExpiresAtUnixObserved":disclosure_expires_at_unix,
        "observedAtUnix":observed_at_unix,
        "meaning":"Exact-package review request persisted under current registration and local disclosure. No Wake event, ChatGPT ACK, independent decision, or next-task authorization was created.",
    }))
}

pub(super) fn prepare(
    control_workspace: &Path,
    original_control: &AutonomousProjectV1,
    project: &AutonomousProjectV1,
    receipt_id: &str,
    expected_package_sha256: &str,
) -> Result<Value, String> {
    if !crate::wake_protocol_client::selected(control_workspace) {
        return Err("CODEX_QUEUE_REVIEW_INVITE: independent Wake owner is not selected".into());
    }
    let store = QueueStore::open_read_only(original_control)?
        .ok_or("CODEX_QUEUE_REVIEW_INVITE: queue state is unavailable")?;
    let _receipt_lock = store.verification_lock(receipt_id)?;
    let _queue_lock = store.lock()?;
    let registry = scheduler(control_workspace)?;
    registry.with_registration_lock(|| {
        let current_control =
            resolve_registered_project_from_control_root(control_workspace, "catdesk")
                .map_err(|_| "CODEX_QUEUE_REVIEW_INVITE: control registration changed")?;
        let current_project =
            resolve_registered_project_from_control_root(control_workspace, &project.project_id)
                .map_err(|_| "CODEX_QUEUE_REVIEW_INVITE: project registration changed")?;
        if current_control.project != *original_control || current_project.project != *project {
            return Err("CODEX_QUEUE_REVIEW_INVITE: registration changed".into());
        }
        if !crate::wake_protocol_client::selected(control_workspace) {
            return Err("CODEX_QUEUE_REVIEW_INVITE: Wake owner selection changed".into());
        }
        prepare_locked(
            &store,
            project,
            receipt_id,
            expected_package_sha256,
            unix_now,
        )
    })
}

#[cfg(test)]
pub(super) fn prepare_in_store_for_test(
    store: &QueueStore,
    project: &AutonomousProjectV1,
    receipt_id: &str,
    expected_package_sha256: &str,
) -> Result<Value, String> {
    prepare_locked(
        store,
        project,
        receipt_id,
        expected_package_sha256,
        unix_now,
    )
}

#[cfg(test)]
pub(super) fn prepare_in_store_at_for_test(
    store: &QueueStore,
    project: &AutonomousProjectV1,
    receipt_id: &str,
    expected_package_sha256: &str,
    observed_at_unix: u64,
) -> Result<Value, String> {
    prepare_locked(store, project, receipt_id, expected_package_sha256, || {
        Ok(observed_at_unix)
    })
}
