//! Persist and publish one exact-package review invitation.
//!
//! Preparation alone is historical evidence, not delivery, review, ACK, or
//! next-task authority. Publication rechecks local disclosure, registration,
//! package bytes and installed Wake authority, then persists its target witness
//! before producing an idempotent event. The invitation itself is immutable.

use super::*;
use crate::delegated::autonomy_projects::canonical_project_chat_target;

const MAX_INVITATION_BYTES: u64 = 4096;
const MAX_PUBLICATION_INTENT_BYTES: u64 = 2048;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewPublicationIntentV1 {
    schema_version: u32,
    project_id: String,
    receipt_id: String,
    package_sha256: String,
    invitation_sha256: String,
    event_id: String,
    target: catdesk_wake::protocol::Target,
}

fn publication_event_id(receipt_id: &str) -> String {
    format!("queue-review-{receipt_id}")
}

fn publication_intent_path(store: &QueueStore, receipt_id: &str) -> PathBuf {
    store
        .root
        .join("receipts")
        .join(format!("{receipt_id}.review-publication-intent.json"))
}

fn read_publication_intent(
    store: &QueueStore,
    receipt_id: &str,
) -> Result<Option<ReviewPublicationIntentV1>, String> {
    let Some(bytes) = read_regular_bounded(
        &publication_intent_path(store, receipt_id),
        MAX_PUBLICATION_INTENT_BYTES,
    )?
    else {
        return Ok(None);
    };
    let intent: ReviewPublicationIntentV1 = serde_json::from_slice(&bytes)
        .map_err(|_| "CODEX_QUEUE_REVIEW_PUBLISH: saved intent is invalid")?;
    intent
        .target
        .validate()
        .map_err(|_| "CODEX_QUEUE_REVIEW_PUBLISH: saved target is invalid")?;
    let mut canonical = serde_json::to_vec(&intent)
        .map_err(|_| "CODEX_QUEUE_REVIEW_PUBLISH: saved intent cannot be encoded")?;
    canonical.push(b'\n');
    if bytes != canonical
        || intent.schema_version != 1
        || intent.receipt_id != receipt_id
        || intent.event_id != publication_event_id(receipt_id)
        || !valid_sha256_hex(&intent.package_sha256)
        || !valid_sha256_hex(&intent.invitation_sha256)
    {
        return Err("CODEX_QUEUE_REVIEW_PUBLISH: saved intent binding is invalid".into());
    }
    Ok(Some(intent))
}

fn unknown_outcome(intent: &ReviewPublicationIntentV1, reason: &str) -> Value {
    json!({
        "action":"codex_queue_review_invitation_publish",
        "publication":"OUTCOME_UNKNOWN",
        "reason":reason,
        "projectId":intent.project_id,
        "receiptId":intent.receipt_id,
        "eventId":intent.event_id,
        "targetGeneration":intent.target.generation,
        "targetDigest":intent.target.digest,
        "eventAccepted":null,
        "deliveryProved":false,
        "chatgptAcknowledged":false,
        "independentReviewProved":false,
        "nextTaskAuthorized":false,
    })
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
    before_first_write: impl FnOnce(u64, &str) -> Result<(), String>,
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
        before_first_write(observed_at_unix, target_sha256)?;
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

pub(super) fn publish_locked_with<T, P>(
    store: &QueueStore,
    project: &AutonomousProjectV1,
    receipt_id: &str,
    expected_package_sha256: &str,
    target_for: T,
    publish: P,
) -> Result<Value, String>
where
    T: FnOnce(u64) -> Result<catdesk_wake::protocol::Target, String>,
    P: FnOnce(
        &str,
        u64,
        &str,
        &catdesk_wake::protocol::Target,
    ) -> Result<catdesk_wake::protocol::Event, String>,
{
    let saved = read_publication_intent(store, receipt_id)?;
    if saved.as_ref().is_some_and(|intent| {
        intent.project_id != project.project_id || intent.package_sha256 != expected_package_sha256
    }) {
        return Err("CODEX_QUEUE_REVIEW_PUBLISH: saved request differs".into());
    }
    let evidence = (|| {
        let package =
            review_reader::bound_package(store, project, receipt_id, expected_package_sha256)?;
        let target_sha256 = registered_wake_target_sha256(project)?;
        if package.receipt.target_sha256.as_deref() != Some(target_sha256) {
            return Err("CODEX_QUEUE_REVIEW_PUBLISH: receipt target changed".into());
        }
        let observed = unix_now()?;
        review_reader::active_disclosure_window_at(
            store,
            project,
            receipt_id,
            expected_package_sha256,
            observed,
        )?;
        read_record(
            store,
            project,
            &package,
            expected_package_sha256,
            target_sha256,
            observed,
        )?
        .ok_or("CODEX_QUEUE_REVIEW_PUBLISH: prepared invitation is required".into())
    })();
    let (invitation, invitation_sha256) = match evidence {
        Ok(value) => value,
        Err(reason) => {
            return saved
                .as_ref()
                .map(|intent| unknown_outcome(intent, "BOUND_EVIDENCE_CHANGED"))
                .map(Ok)
                .unwrap_or(Err(reason));
        }
    };
    if saved
        .as_ref()
        .is_some_and(|intent| intent.invitation_sha256 != invitation_sha256)
    {
        return Ok(unknown_outcome(
            saved.as_ref().expect("checked saved intent"),
            "INVITATION_CHANGED",
        ));
    }
    let current = match target_for(invitation.prepared_at_unix) {
        Ok(target) => target,
        Err(reason) => {
            return saved
                .as_ref()
                .map(|intent| unknown_outcome(intent, "WAKE_AUTHORITY_CHANGED"))
                .map(Ok)
                .unwrap_or(Err(reason));
        }
    };
    current.validate()?;
    if current.digest != invitation.target_sha256 {
        return saved
            .as_ref()
            .map(|intent| unknown_outcome(intent, "WAKE_TARGET_CHANGED"))
            .map(Ok)
            .unwrap_or(Err("CODEX_QUEUE_REVIEW_PUBLISH: Wake target changed".into()));
    }
    let intent = if let Some(saved) = saved {
        if saved.target != current {
            return Ok(unknown_outcome(&saved, "WAKE_TARGET_CHANGED"));
        }
        saved
    } else {
        let intent = ReviewPublicationIntentV1 {
            schema_version: 1,
            project_id: project.project_id.clone(),
            receipt_id: receipt_id.into(),
            package_sha256: expected_package_sha256.into(),
            invitation_sha256,
            event_id: publication_event_id(receipt_id),
            target: current,
        };
        let bytes = serde_json::to_vec(&intent)
            .map_err(|_| "CODEX_QUEUE_REVIEW_PUBLISH: intent cannot be encoded")?;
        if bytes.len().saturating_add(1) as u64 > MAX_PUBLICATION_INTENT_BYTES {
            return Err("CODEX_QUEUE_REVIEW_PUBLISH: intent is oversized".into());
        }
        create_new_record(&publication_intent_path(store, receipt_id), &bytes)?;
        if read_publication_intent(store, receipt_id)?.as_ref() != Some(&intent) {
            return Err("CODEX_QUEUE_REVIEW_PUBLISH: intent readback changed".into());
        }
        intent
    };
    let event = match publish(
        &intent.event_id,
        invitation.prepared_at_unix,
        &invitation.message,
        &intent.target,
    ) {
        Ok(event) => event,
        Err(reason) => {
            let code = if reason == "QUEUE_EVENT_ID_CONFLICT" {
                "EVENT_ID_CONFLICT"
            } else if matches!(
                reason.as_str(),
                "TARGET_EXPECTATION_MISMATCH"
                    | "WAKE_PROJECT_TARGET_MISMATCH"
                    | "WAKE_PROJECT_TARGET_CHANGED"
                    | "WAKE_HISTORICAL_EVENT_RETAINED"
            ) {
                "WAKE_TARGET_CHANGED"
            } else {
                "PUBLICATION_UNCERTAIN"
            };
            return Ok(unknown_outcome(&intent, code));
        }
    };
    event.validate()?;
    if event.event_id != intent.event_id
        || event.project_id != intent.project_id
        || event.event_type != "engineering_attention"
        || event.message != invitation.message
        || event.target_generation != intent.target.generation
    {
        return Ok(unknown_outcome(&intent, "EVENT_READBACK_CHANGED"));
    }
    Ok(json!({
        "action":"codex_queue_review_invitation_publish",
        "publication":"WAKE_EVENT_ACCEPTED",
        "projectId":intent.project_id,
        "receiptId":intent.receipt_id,
        "packageSha256":intent.package_sha256,
        "invitationSha256":intent.invitation_sha256,
        "eventId":event.event_id,
        "eventType":event.event_type,
        "eventCreatedUtc":event.created_utc,
        "targetGeneration":intent.target.generation,
        "targetDigest":intent.target.digest,
        "eventAccepted":true,
        "deliveryProved":false,
        "chatgptAcknowledged":false,
        "independentReviewProved":false,
        "nextTaskAuthorized":false,
        "meaning":"Wake accepted the exact review request event. Delivery, ChatGPT review and any next task require separate evidence.",
    }))
}

pub(crate) fn publish_for_mcp(
    control_workspace: &Path,
    project_id: &str,
    receipt_id: &str,
    expected_package_sha256: &str,
) -> Result<Value, String> {
    valid_slug(project_id)?;
    if !Uuid::parse_str(receipt_id).is_ok_and(|id| id.to_string() == receipt_id)
        || !valid_sha256_hex(expected_package_sha256)
        || expected_package_sha256 != expected_package_sha256.to_ascii_lowercase()
    {
        return Err("CODEX_QUEUE_REVIEW_PUBLISH: exact receipt and package are required".into());
    }
    let control = resolve_registered_project_from_control_root(control_workspace, "catdesk")
        .map_err(|_| "CODEX_QUEUE_REVIEW_PUBLISH: control registration unavailable")?;
    let store = QueueStore::open_read_only(&control.project)?
        .ok_or("CODEX_QUEUE_REVIEW_PUBLISH: queue state unavailable")?;
    let _receipt_lock = store.verification_lock(receipt_id)?;
    let _queue_lock = store.lock()?;
    let registry = scheduler(control_workspace)?;
    registry.with_registration_lock(|| {
        let saved = read_publication_intent(&store, receipt_id)?;
        if saved.as_ref().is_some_and(|intent| {
            intent.project_id != project_id || intent.package_sha256 != expected_package_sha256
        }) {
            return Err("CODEX_QUEUE_REVIEW_PUBLISH: saved request differs".into());
        }
        let current_control =
            resolve_registered_project_from_control_root(control_workspace, "catdesk");
        if current_control
            .as_ref()
            .map_or(true, |current| current.project != control.project)
        {
            return saved
                .as_ref()
                .map(|intent| unknown_outcome(intent, "CONTROL_REGISTRATION_CHANGED"))
                .map(Ok)
                .unwrap_or(Err(
                    "CODEX_QUEUE_REVIEW_PUBLISH: control registration changed".into(),
                ));
        }
        let current_project =
            resolve_registered_project_from_control_root(control_workspace, project_id);
        let project = match current_project {
            Ok(current) => current.project,
            Err(_) => {
                return saved
                    .as_ref()
                    .map(|intent| unknown_outcome(intent, "PROJECT_REGISTRATION_CHANGED"))
                    .map(Ok)
                    .unwrap_or(Err(
                        "CODEX_QUEUE_REVIEW_PUBLISH: project registration changed".into(),
                    ));
            }
        };
        publish_locked_with(
            &store,
            &project,
            receipt_id,
            expected_package_sha256,
            |prepared_at| {
                crate::wake_protocol_client::registered_invitation_target(
                    control_workspace,
                    project_id,
                    prepared_at,
                )
            },
            |event_id, prepared_at, message, target| {
                crate::wake_protocol_client::publish_registered_invitation(
                    control_workspace,
                    project_id,
                    event_id,
                    prepared_at,
                    message,
                    target,
                )
            },
        )
    })
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
            |observed_at, expected_target_digest| {
                let target = crate::wake_protocol_client::registered_invitation_target(
                    control_workspace,
                    &project.project_id,
                    observed_at,
                )?;
                if target.digest != expected_target_digest {
                    return Err("CODEX_QUEUE_REVIEW_INVITE: Wake target changed".into());
                }
                Ok(())
            },
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
        |_, _| Ok(()),
    )
}

#[cfg(test)]
pub(super) fn prepare_in_store_with_preflight_for_test(
    store: &QueueStore,
    project: &AutonomousProjectV1,
    receipt_id: &str,
    expected_package_sha256: &str,
    before_first_write: impl FnOnce(u64, &str) -> Result<(), String>,
) -> Result<Value, String> {
    prepare_locked(
        store,
        project,
        receipt_id,
        expected_package_sha256,
        unix_now,
        before_first_write,
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
    prepare_locked(
        store,
        project,
        receipt_id,
        expected_package_sha256,
        || Ok(observed_at_unix),
        |_, _| Ok(()),
    )
}
