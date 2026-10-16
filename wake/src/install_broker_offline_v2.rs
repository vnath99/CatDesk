//! Signed, read-only first-generation offline migration contract.
//!
//! The installed dev.84 host cannot answer the new owner challenge, and the
//! V1 package approval requires a live old PID. This separate V2 format binds
//! an independently reviewed offline predecessor to a package approval. It
//! deliberately returns no ledger lease, owner proof or installation token.
//! The supplied old-store observation must later be replaced by retained
//! broker-owned physical pins before any transaction can be enabled.

use crate::{
    Result,
    install_broker_approval::{ExactCiV1, ObjectIdentityV1, PackageManifestV1, validate_manifest},
};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const ANCHOR_DOMAIN: &[u8] = b"CatDeskWakeOfflineMigrationAnchorV2\0";
const APPROVAL_DOMAIN: &[u8] = b"CatDeskWakeOfflineInstallApprovalV2\0";
const MAX_ENVELOPE_BYTES: usize = 16 * 1024;
const MAX_AGE_SECONDS: u64 = 7 * 24 * 60 * 60;
const REPOSITORY: &str = "vnath99/CatDesk";
const WORKFLOW: &str = ".github/workflows/ci.yml";
const WORKFLOW_ID: u64 = 311_012_404;
const BRANCH: &str = "orchestrator/chatgpt-codex-autonomous-loop";
const JOBS: [&str; 3] = [
    "Rust",
    "Python Wake and advisor tests",
    "Independent WakeHost Rust",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OfflineDesiredStateV2 {
    Running,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AmbiguousEventDispositionV2 {
    QuarantinedNoReplay,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OfflineMigrationAnchorV2 {
    pub schema_version: u32,
    pub purpose: String,
    pub operation: String,
    pub anchor_id: String,
    pub issued_utc_seconds: u64,
    pub expires_utc_seconds: u64,
    pub reviewer_identity: String,
    pub install_generation: u64,
    pub predecessor_package_id: String,
    pub predecessor_pointer: ObjectIdentityV1,
    pub predecessor_pointer_sha256: String,
    pub old_runtime_root: ObjectIdentityV1,
    pub old_runtime_lock: ObjectIdentityV1,
    pub old_host_image: ObjectIdentityV1,
    pub old_host_image_sha256: String,
    pub target_conversation_id: String,
    pub target_generation: u64,
    pub target_snapshot_sha256: String,
    pub project_id: String,
    pub project_registration_object: ObjectIdentityV1,
    pub project_target_conversation_id: String,
    pub project_target_generation: u64,
    pub project_target_snapshot_sha256: String,
    pub unresolved_events_sha256: String,
    pub ambiguous_event_disposition: AmbiguousEventDispositionV2,
    pub desired_state: OfflineDesiredStateV2,
    pub closed_store_inventory_sha256: String,
    pub quarantined_launch_entrypoints_sha256: String,
    pub target_protected_root: ObjectIdentityV1,
    pub broker_policy_file: ObjectIdentityV1,
    pub broker_policy_sha256: String,
    pub broker_service_image: ObjectIdentityV1,
    pub broker_service_image_sha256: String,
    pub candidate_manifest_sha256: String,
    pub source_commit: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WakeOfflineInstallApprovalV2 {
    pub schema_version: u32,
    pub purpose: String,
    pub operation: String,
    pub approval_id: String,
    pub issued_utc_seconds: u64,
    pub expires_utc_seconds: u64,
    pub reviewer_identity: String,
    pub install_generation: u64,
    pub anchor_id: String,
    pub anchor_envelope_sha256: String,
    pub predecessor_package_id: String,
    pub predecessor_pointer: ObjectIdentityV1,
    pub target_protected_root: ObjectIdentityV1,
    pub manifest: PackageManifestV1,
    pub manifest_sha256: String,
    pub source_commit: String,
    pub ci: ExactCiV1,
}

/// Supplied comparison data is deliberately not an owner or store capability.
/// A future broker must obtain it from retained, independently pinned objects
/// while it holds the global ledger lock and all candidate old-store locks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineMigrationObservationV2 {
    pub now_utc_seconds: u64,
    pub predecessor_package_id: String,
    pub predecessor_pointer: ObjectIdentityV1,
    pub predecessor_pointer_sha256: String,
    pub old_runtime_root: ObjectIdentityV1,
    pub old_runtime_lock: ObjectIdentityV1,
    pub old_host_image: ObjectIdentityV1,
    pub old_host_image_sha256: String,
    pub target_conversation_id: String,
    pub target_generation: u64,
    pub target_snapshot_sha256: String,
    pub project_id: String,
    pub project_registration_object: ObjectIdentityV1,
    pub project_target_conversation_id: String,
    pub project_target_generation: u64,
    pub project_target_snapshot_sha256: String,
    pub unresolved_events_sha256: String,
    pub ambiguous_event_disposition: AmbiguousEventDispositionV2,
    pub desired_state: OfflineDesiredStateV2,
    pub closed_store_inventory_sha256: String,
    pub quarantined_launch_entrypoints_sha256: String,
    pub target_protected_root: ObjectIdentityV1,
    pub broker_policy_file: ObjectIdentityV1,
    pub broker_policy_sha256: String,
    pub broker_service_image: ObjectIdentityV1,
    pub broker_service_image_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflinePairReadbackV2 {
    pub schema_version: u32,
    pub anchor_id: String,
    pub approval_id: String,
    pub anchor_envelope_sha256: String,
    pub approval_envelope_sha256: String,
    pub verification_key_sha256: String,
    pub install_generation: u64,
    pub disposition: &'static str,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignedEnvelope<T> {
    payload: T,
    signature_hex: String,
}

fn lowercase_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'@'))
}

fn valid_object(value: &ObjectIdentityV1) -> bool {
    value.volume_serial != 0 && (value.file_index_high != 0 || value.file_index_low != 0)
}

fn valid_time(issued: u64, expires: u64, now: u64) -> bool {
    issued <= now && now <= expires && expires > issued && expires - issued <= MAX_AGE_SECONDS
}

fn signature_bytes(value: &str) -> Result<[u8; 64]> {
    if !lowercase_hex(value, 128) {
        return Err("WAKE_OFFLINE_SIGNATURE_INVALID".into());
    }
    let mut bytes = [0_u8; 64];
    for (destination, pair) in bytes.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        let digit = |byte: u8| match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            _ => unreachable!("checked lowercase hex"),
        };
        *destination = (digit(pair[0]) << 4) | digit(pair[1]);
    }
    Ok(bytes)
}

fn verify_envelope<T>(wire: &[u8], domain: &[u8], key: &VerifyingKey) -> Result<(T, String)>
where
    T: for<'de> Deserialize<'de> + Serialize,
{
    if wire.is_empty() || wire.len() > MAX_ENVELOPE_BYTES {
        return Err("WAKE_OFFLINE_ENCODING_INVALID".into());
    }
    let envelope: SignedEnvelope<T> =
        serde_json::from_slice(wire).map_err(|_| "WAKE_OFFLINE_ENCODING_INVALID")?;
    if serde_json::to_vec(&envelope).map_err(|_| "WAKE_OFFLINE_ENCODING_INVALID")? != wire {
        return Err("WAKE_OFFLINE_ENCODING_INVALID".into());
    }
    let mut signed = domain.to_vec();
    signed.extend(
        serde_json::to_vec(&envelope.payload).map_err(|_| "WAKE_OFFLINE_ENCODING_INVALID")?,
    );
    let signature = Signature::from_bytes(&signature_bytes(&envelope.signature_hex)?);
    key.verify_strict(&signed, &signature)
        .map_err(|_| "WAKE_OFFLINE_SIGNATURE_INVALID")?;
    Ok((envelope.payload, format!("{:x}", Sha256::digest(wire))))
}

fn validate_anchor(
    anchor: &OfflineMigrationAnchorV2,
    observed: &OfflineMigrationObservationV2,
) -> Result<()> {
    if anchor.schema_version != 2
        || anchor.purpose != "catdesk-wake-first-generation-offline-anchor"
        || anchor.operation != "MIGRATE_DEV84_OFFLINE"
        || !lowercase_hex(&anchor.anchor_id, 32)
        || !identifier(&anchor.reviewer_identity, 128)
        || anchor.install_generation != 1
        || !identifier(&anchor.predecessor_package_id, 200)
        || !identifier(&anchor.target_conversation_id, 128)
        || anchor.target_generation == 0
        || !identifier(&anchor.project_id, 128)
        || anchor.project_target_conversation_id != anchor.target_conversation_id
        || anchor.project_target_generation != anchor.target_generation
        || !lowercase_hex(&anchor.predecessor_pointer_sha256, 64)
        || !lowercase_hex(&anchor.old_host_image_sha256, 64)
        || !lowercase_hex(&anchor.target_snapshot_sha256, 64)
        || !lowercase_hex(&anchor.project_target_snapshot_sha256, 64)
        || !lowercase_hex(&anchor.unresolved_events_sha256, 64)
        || !lowercase_hex(&anchor.closed_store_inventory_sha256, 64)
        || !lowercase_hex(&anchor.quarantined_launch_entrypoints_sha256, 64)
        || !lowercase_hex(&anchor.broker_policy_sha256, 64)
        || !lowercase_hex(&anchor.broker_service_image_sha256, 64)
        || !lowercase_hex(&anchor.candidate_manifest_sha256, 64)
        || !lowercase_hex(&anchor.source_commit, 40)
        || [
            &anchor.predecessor_pointer,
            &anchor.old_runtime_root,
            &anchor.old_runtime_lock,
            &anchor.old_host_image,
            &anchor.project_registration_object,
            &anchor.target_protected_root,
            &anchor.broker_policy_file,
            &anchor.broker_service_image,
        ]
        .iter()
        .any(|object| !valid_object(object))
        || anchor.predecessor_pointer == anchor.old_runtime_root
        || anchor.predecessor_pointer == anchor.old_runtime_lock
        || anchor.predecessor_pointer == anchor.old_host_image
        || anchor.predecessor_pointer == anchor.target_protected_root
        || anchor.old_runtime_root == anchor.old_runtime_lock
        || anchor.old_runtime_root == anchor.old_host_image
        || anchor.old_runtime_root == anchor.target_protected_root
        || anchor.old_runtime_lock == anchor.old_host_image
        || anchor.predecessor_pointer.volume_serial != anchor.old_runtime_root.volume_serial
        || anchor.old_runtime_lock.volume_serial != anchor.old_runtime_root.volume_serial
        || anchor.old_host_image.volume_serial != anchor.old_runtime_root.volume_serial
    {
        return Err("WAKE_OFFLINE_ANCHOR_INVALID".into());
    }
    if !valid_time(
        anchor.issued_utc_seconds,
        anchor.expires_utc_seconds,
        observed.now_utc_seconds,
    ) {
        return Err("WAKE_OFFLINE_TIME_INVALID".into());
    }
    if anchor.predecessor_package_id != observed.predecessor_package_id
        || anchor.predecessor_pointer != observed.predecessor_pointer
        || anchor.predecessor_pointer_sha256 != observed.predecessor_pointer_sha256
        || anchor.old_runtime_root != observed.old_runtime_root
        || anchor.old_runtime_lock != observed.old_runtime_lock
        || anchor.old_host_image != observed.old_host_image
        || anchor.old_host_image_sha256 != observed.old_host_image_sha256
        || anchor.target_conversation_id != observed.target_conversation_id
        || anchor.target_generation != observed.target_generation
        || anchor.target_snapshot_sha256 != observed.target_snapshot_sha256
        || anchor.project_id != observed.project_id
        || anchor.project_registration_object != observed.project_registration_object
        || anchor.project_target_conversation_id != observed.project_target_conversation_id
        || anchor.project_target_generation != observed.project_target_generation
        || anchor.project_target_snapshot_sha256 != observed.project_target_snapshot_sha256
        || anchor.unresolved_events_sha256 != observed.unresolved_events_sha256
        || anchor.ambiguous_event_disposition != observed.ambiguous_event_disposition
        || anchor.desired_state != observed.desired_state
        || anchor.closed_store_inventory_sha256 != observed.closed_store_inventory_sha256
        || anchor.quarantined_launch_entrypoints_sha256
            != observed.quarantined_launch_entrypoints_sha256
        || anchor.target_protected_root != observed.target_protected_root
        || anchor.broker_policy_file != observed.broker_policy_file
        || anchor.broker_policy_sha256 != observed.broker_policy_sha256
        || anchor.broker_service_image != observed.broker_service_image
        || anchor.broker_service_image_sha256 != observed.broker_service_image_sha256
    {
        return Err("WAKE_OFFLINE_ANCHOR_OBSERVATION_MISMATCH".into());
    }
    Ok(())
}

fn valid_ci(ci: &ExactCiV1, source_commit: &str) -> bool {
    ci.repository == REPOSITORY
        && ci.workflow_path == WORKFLOW
        && ci.workflow_id == WORKFLOW_ID
        && ci.branch == BRANCH
        && ci.head_sha == source_commit
        && ci.event == "push"
        && ci.run_status == "completed"
        && ci.run_conclusion == "success"
        && ci.run_id != 0
        && ci.run_attempt != 0
        && ci
            .jobs
            .iter()
            .zip(JOBS)
            .all(|(job, name)| job.name == name && job.job_id != 0 && job.conclusion == "success")
        && ci.jobs[0].job_id != ci.jobs[1].job_id
        && ci.jobs[0].job_id != ci.jobs[2].job_id
        && ci.jobs[1].job_id != ci.jobs[2].job_id
}

fn validate_approval(
    approval: &WakeOfflineInstallApprovalV2,
    anchor: &OfflineMigrationAnchorV2,
    anchor_digest: &str,
    now: u64,
) -> Result<()> {
    if approval.schema_version != 2
        || approval.purpose != "catdesk-wake-protected-offline-install"
        || approval.operation != "PUBLISH_AND_ACTIVATE_AFTER_OFFLINE_ANCHOR"
        || !lowercase_hex(&approval.approval_id, 32)
        || !identifier(&approval.reviewer_identity, 128)
        || !lowercase_hex(&approval.anchor_envelope_sha256, 64)
        || !lowercase_hex(&approval.manifest_sha256, 64)
        || !lowercase_hex(&approval.source_commit, 40)
    {
        return Err("WAKE_OFFLINE_APPROVAL_INVALID".into());
    }
    if !valid_time(
        approval.issued_utc_seconds,
        approval.expires_utc_seconds,
        now,
    ) {
        return Err("WAKE_OFFLINE_TIME_INVALID".into());
    }
    validate_manifest(&approval.manifest).map_err(|_| "WAKE_OFFLINE_PACKAGE_INVALID")?;
    if approval.manifest.version != "1.0.0-dev.85" {
        return Err("WAKE_OFFLINE_PACKAGE_INVALID".into());
    }
    let manifest_digest = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&approval.manifest).map_err(|_| "WAKE_OFFLINE_PACKAGE_INVALID")?
        )
    );
    if approval.manifest_sha256 != manifest_digest {
        return Err("WAKE_OFFLINE_PACKAGE_INVALID".into());
    }
    if !valid_ci(&approval.ci, &approval.source_commit) {
        return Err("WAKE_OFFLINE_CI_INVALID".into());
    }
    if approval.install_generation != anchor.install_generation
        || approval.anchor_id != anchor.anchor_id
        || approval.anchor_envelope_sha256 != anchor_digest
        || approval.predecessor_package_id != anchor.predecessor_package_id
        || approval.predecessor_pointer != anchor.predecessor_pointer
        || approval.target_protected_root != anchor.target_protected_root
        || approval.manifest_sha256 != anchor.candidate_manifest_sha256
        || approval.source_commit != anchor.source_commit
    {
        return Err("WAKE_OFFLINE_APPROVAL_ANCHOR_MISMATCH".into());
    }
    Ok(())
}

/// Verify two distinct signed domains against a supplied comparison snapshot.
/// The output is a diagnostic readback only. Even a successful result does not
/// prove offline host quiescence, connected-store selection, exact GitHub CI,
/// protected policy provenance or approval by a human independent of CatDesk.
pub(crate) fn verify_offline_pair_read_only(
    anchor_wire: &[u8],
    approval_wire: &[u8],
    key: &VerifyingKey,
    observed: &OfflineMigrationObservationV2,
) -> Result<OfflinePairReadbackV2> {
    let (anchor, anchor_digest) =
        verify_envelope::<OfflineMigrationAnchorV2>(anchor_wire, ANCHOR_DOMAIN, key)?;
    validate_anchor(&anchor, observed)?;
    let (approval, approval_digest) =
        verify_envelope::<WakeOfflineInstallApprovalV2>(approval_wire, APPROVAL_DOMAIN, key)?;
    validate_approval(&approval, &anchor, &anchor_digest, observed.now_utc_seconds)?;
    Ok(OfflinePairReadbackV2 {
        schema_version: 2,
        anchor_id: anchor.anchor_id,
        approval_id: approval.approval_id,
        anchor_envelope_sha256: anchor_digest,
        approval_envelope_sha256: approval_digest,
        verification_key_sha256: format!("{:x}", Sha256::digest(key.as_bytes())),
        install_generation: approval.install_generation,
        disposition: "SIGNED_OFFLINE_PAIR_MATCHED_SUPPLIED_OBSERVATION_NO_INSTALL_AUTHORITY",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_broker_approval::tests as v1_test;
    use ed25519_dalek::{Signer, SigningKey};

    fn object(index: u32) -> ObjectIdentityV1 {
        ObjectIdentityV1 {
            volume_serial: 7,
            file_index_high: 0,
            file_index_low: index,
        }
    }

    fn signed<T: Clone + Serialize>(payload: &T, domain: &[u8], key: &SigningKey) -> Vec<u8> {
        let mut message = domain.to_vec();
        message.extend(serde_json::to_vec(payload).expect("canonical payload"));
        let signature = key.sign(&message);
        serde_json::to_vec(&SignedEnvelope {
            payload: payload.clone(),
            signature_hex: signature
                .to_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        })
        .expect("canonical envelope")
    }

    fn fixture() -> (
        OfflineMigrationAnchorV2,
        WakeOfflineInstallApprovalV2,
        OfflineMigrationObservationV2,
        SigningKey,
    ) {
        let (v1, _, key) = v1_test::fixture();
        let anchor = OfflineMigrationAnchorV2 {
            schema_version: 2,
            purpose: "catdesk-wake-first-generation-offline-anchor".into(),
            operation: "MIGRATE_DEV84_OFFLINE".into(),
            anchor_id: "a".repeat(32),
            issued_utc_seconds: 1_000,
            expires_utc_seconds: 2_000,
            reviewer_identity: "reviewer@example.test".into(),
            install_generation: 1,
            predecessor_package_id: v1.predecessor_package_id.clone(),
            predecessor_pointer: object(1),
            predecessor_pointer_sha256: "b".repeat(64),
            old_runtime_root: object(2),
            old_runtime_lock: object(3),
            old_host_image: object(4),
            old_host_image_sha256: "c".repeat(64),
            target_conversation_id: "11111111-1111-1111-1111-111111111111".into(),
            target_generation: 34,
            target_snapshot_sha256: "d".repeat(64),
            project_id: "catdesk".into(),
            project_registration_object: object(5),
            project_target_conversation_id: "11111111-1111-1111-1111-111111111111".into(),
            project_target_generation: 34,
            project_target_snapshot_sha256: "e".repeat(64),
            unresolved_events_sha256: "f".repeat(64),
            ambiguous_event_disposition: AmbiguousEventDispositionV2::QuarantinedNoReplay,
            desired_state: OfflineDesiredStateV2::Running,
            closed_store_inventory_sha256: "1".repeat(64),
            quarantined_launch_entrypoints_sha256: "2".repeat(64),
            target_protected_root: object(6),
            broker_policy_file: object(7),
            broker_policy_sha256: "3".repeat(64),
            broker_service_image: object(8),
            broker_service_image_sha256: "4".repeat(64),
            candidate_manifest_sha256: v1.manifest_sha256.clone(),
            source_commit: v1.source_commit.clone(),
        };
        let anchor_wire = signed(&anchor, ANCHOR_DOMAIN, &key);
        let approval = WakeOfflineInstallApprovalV2 {
            schema_version: 2,
            purpose: "catdesk-wake-protected-offline-install".into(),
            operation: "PUBLISH_AND_ACTIVATE_AFTER_OFFLINE_ANCHOR".into(),
            approval_id: "5".repeat(32),
            issued_utc_seconds: 1_100,
            expires_utc_seconds: 1_900,
            reviewer_identity: "reviewer@example.test".into(),
            install_generation: 1,
            anchor_id: anchor.anchor_id.clone(),
            anchor_envelope_sha256: format!("{:x}", Sha256::digest(anchor_wire)),
            predecessor_package_id: anchor.predecessor_package_id.clone(),
            predecessor_pointer: anchor.predecessor_pointer.clone(),
            target_protected_root: anchor.target_protected_root.clone(),
            manifest: v1.manifest.clone(),
            manifest_sha256: v1.manifest_sha256.clone(),
            source_commit: v1.source_commit.clone(),
            ci: v1.ci,
        };
        let observed = OfflineMigrationObservationV2 {
            now_utc_seconds: 1_500,
            predecessor_package_id: anchor.predecessor_package_id.clone(),
            predecessor_pointer: anchor.predecessor_pointer.clone(),
            predecessor_pointer_sha256: anchor.predecessor_pointer_sha256.clone(),
            old_runtime_root: anchor.old_runtime_root.clone(),
            old_runtime_lock: anchor.old_runtime_lock.clone(),
            old_host_image: anchor.old_host_image.clone(),
            old_host_image_sha256: anchor.old_host_image_sha256.clone(),
            target_conversation_id: anchor.target_conversation_id.clone(),
            target_generation: anchor.target_generation,
            target_snapshot_sha256: anchor.target_snapshot_sha256.clone(),
            project_id: anchor.project_id.clone(),
            project_registration_object: anchor.project_registration_object.clone(),
            project_target_conversation_id: anchor.project_target_conversation_id.clone(),
            project_target_generation: anchor.project_target_generation,
            project_target_snapshot_sha256: anchor.project_target_snapshot_sha256.clone(),
            unresolved_events_sha256: anchor.unresolved_events_sha256.clone(),
            ambiguous_event_disposition: anchor.ambiguous_event_disposition,
            desired_state: anchor.desired_state,
            closed_store_inventory_sha256: anchor.closed_store_inventory_sha256.clone(),
            quarantined_launch_entrypoints_sha256: anchor
                .quarantined_launch_entrypoints_sha256
                .clone(),
            target_protected_root: anchor.target_protected_root.clone(),
            broker_policy_file: anchor.broker_policy_file.clone(),
            broker_policy_sha256: anchor.broker_policy_sha256.clone(),
            broker_service_image: anchor.broker_service_image.clone(),
            broker_service_image_sha256: anchor.broker_service_image_sha256.clone(),
        };
        (anchor, approval, observed, key)
    }

    fn check(
        anchor: &OfflineMigrationAnchorV2,
        approval: &WakeOfflineInstallApprovalV2,
        observed: &OfflineMigrationObservationV2,
        key: &SigningKey,
    ) -> Result<OfflinePairReadbackV2> {
        verify_offline_pair_read_only(
            &signed(anchor, ANCHOR_DOMAIN, key),
            &signed(approval, APPROVAL_DOMAIN, key),
            &key.verifying_key(),
            observed,
        )
    }

    #[test]
    fn signed_pair_binds_first_generation_and_remains_read_only() {
        let (anchor, approval, observed, key) = fixture();
        let readback = check(&anchor, &approval, &observed, &key).expect("signed pair");
        assert_eq!(readback.schema_version, 2);
        assert_eq!(readback.install_generation, 1);
        assert_eq!(readback.anchor_id, anchor.anchor_id);
        assert_eq!(readback.approval_id, approval.approval_id);
        assert!(readback.disposition.ends_with("NO_INSTALL_AUTHORITY"));
    }

    #[test]
    fn wrong_key_domain_and_noncanonical_wire_are_refused() {
        let (anchor, approval, observed, key) = fixture();
        let anchor_wire = signed(&anchor, ANCHOR_DOMAIN, &key);
        let approval_wire = signed(&approval, APPROVAL_DOMAIN, &key);
        let wrong = SigningKey::from_bytes(&[8_u8; 32]);
        assert_eq!(
            verify_offline_pair_read_only(
                &anchor_wire,
                &approval_wire,
                &wrong.verifying_key(),
                &observed,
            )
            .unwrap_err(),
            "WAKE_OFFLINE_SIGNATURE_INVALID"
        );
        assert_eq!(
            verify_offline_pair_read_only(
                &signed(&anchor, APPROVAL_DOMAIN, &key),
                &approval_wire,
                &key.verifying_key(),
                &observed,
            )
            .unwrap_err(),
            "WAKE_OFFLINE_SIGNATURE_INVALID"
        );
        assert_eq!(
            verify_offline_pair_read_only(
                &anchor_wire,
                &signed(&approval, ANCHOR_DOMAIN, &key),
                &key.verifying_key(),
                &observed,
            )
            .unwrap_err(),
            "WAKE_OFFLINE_SIGNATURE_INVALID"
        );
        let payload_wire = serde_json::to_vec(&anchor).expect("payload");
        for bad in [
            [anchor_wire.as_slice(), b" "].concat(),
            [&anchor_wire[..anchor_wire.len() - 1], b",\"extra\":1}"].concat(),
            [
                &anchor_wire[..anchor_wire.len() - 1],
                b",\"payload\":",
                payload_wire.as_slice(),
                b"}",
            ]
            .concat(),
            vec![b'x'; MAX_ENVELOPE_BYTES + 1],
        ] {
            assert_eq!(
                verify_offline_pair_read_only(
                    &bad,
                    &approval_wire,
                    &key.verifying_key(),
                    &observed,
                )
                .unwrap_err(),
                "WAKE_OFFLINE_ENCODING_INVALID"
            );
        }
        let mut changed = anchor.clone();
        changed.old_runtime_lock.file_index_low += 1;
        let tampered = SignedEnvelope {
            payload: changed,
            signature_hex: serde_json::from_slice::<SignedEnvelope<OfflineMigrationAnchorV2>>(
                &anchor_wire,
            )
            .expect("wire")
            .signature_hex,
        };
        assert_eq!(
            verify_offline_pair_read_only(
                &serde_json::to_vec(&tampered).expect("canonical tamper"),
                &approval_wire,
                &key.verifying_key(),
                &observed,
            )
            .unwrap_err(),
            "WAKE_OFFLINE_SIGNATURE_INVALID"
        );
    }

    #[test]
    fn changed_old_store_project_target_and_launcher_claims_refuse() {
        let (anchor, approval, observed, key) = fixture();
        let mut cases = Vec::new();
        let mut bad = observed.clone();
        bad.old_runtime_lock.file_index_low += 1;
        cases.push(bad);
        let mut bad = observed.clone();
        bad.predecessor_pointer_sha256 = "0".repeat(64);
        cases.push(bad);
        let mut bad = observed.clone();
        bad.project_target_snapshot_sha256 = "0".repeat(64);
        cases.push(bad);
        let mut bad = observed.clone();
        bad.project_registration_object.file_index_low += 1;
        cases.push(bad);
        let mut bad = observed.clone();
        bad.unresolved_events_sha256 = "0".repeat(64);
        cases.push(bad);
        let mut bad = observed.clone();
        bad.closed_store_inventory_sha256 = "0".repeat(64);
        cases.push(bad);
        let mut bad = observed.clone();
        bad.quarantined_launch_entrypoints_sha256 = "0".repeat(64);
        cases.push(bad);
        for bad in cases {
            assert_eq!(
                check(&anchor, &approval, &bad, &key).unwrap_err(),
                "WAKE_OFFLINE_ANCHOR_OBSERVATION_MISMATCH"
            );
        }
    }

    #[test]
    fn invalid_anchor_cannot_recast_offline_as_live_or_split_project_target() {
        let (anchor, approval, observed, key) = fixture();
        let mut bad = anchor.clone();
        bad.project_target_generation += 1;
        assert_eq!(
            check(&bad, &approval, &observed, &key).unwrap_err(),
            "WAKE_OFFLINE_ANCHOR_INVALID"
        );
        let mut bad = anchor.clone();
        bad.install_generation = 2;
        assert_eq!(
            check(&bad, &approval, &observed, &key).unwrap_err(),
            "WAKE_OFFLINE_ANCHOR_INVALID"
        );
        let mut bad = anchor.clone();
        bad.quarantined_launch_entrypoints_sha256.clear();
        assert_eq!(
            check(&bad, &approval, &observed, &key).unwrap_err(),
            "WAKE_OFFLINE_ANCHOR_INVALID"
        );
        let mut bad = anchor.clone();
        bad.predecessor_pointer = bad.old_runtime_lock.clone();
        assert_eq!(
            check(&bad, &approval, &observed, &key).unwrap_err(),
            "WAKE_OFFLINE_ANCHOR_INVALID"
        );
        let mut bad = anchor.clone();
        bad.old_host_image.volume_serial += 1;
        assert_eq!(
            check(&bad, &approval, &observed, &key).unwrap_err(),
            "WAKE_OFFLINE_ANCHOR_INVALID"
        );
        let anchor_wire = signed(&anchor, ANCHOR_DOMAIN, &key);
        let mut value: serde_json::Value = serde_json::from_slice(&anchor_wire).unwrap();
        value["payload"]["oldHostPid"] = 42.into();
        assert_eq!(
            verify_offline_pair_read_only(
                &serde_json::to_vec(&value).unwrap(),
                &signed(&approval, APPROVAL_DOMAIN, &key),
                &key.verifying_key(),
                &observed,
            )
            .unwrap_err(),
            "WAKE_OFFLINE_ENCODING_INVALID"
        );
    }

    #[test]
    fn approval_must_bind_exact_anchor_manifest_source_and_ci() {
        let (anchor, approval, observed, key) = fixture();
        let mut cases = Vec::new();
        let mut bad = approval.clone();
        bad.anchor_envelope_sha256 = "0".repeat(64);
        cases.push((bad, "WAKE_OFFLINE_APPROVAL_ANCHOR_MISMATCH"));
        let mut bad = approval.clone();
        bad.predecessor_pointer.file_index_low += 1;
        cases.push((bad, "WAKE_OFFLINE_APPROVAL_ANCHOR_MISMATCH"));
        let mut bad = approval.clone();
        bad.source_commit = "0".repeat(40);
        bad.ci.head_sha = bad.source_commit.clone();
        cases.push((bad, "WAKE_OFFLINE_APPROVAL_ANCHOR_MISMATCH"));
        let mut bad = approval.clone();
        bad.manifest.artifacts[0].byte_length += 1;
        cases.push((bad, "WAKE_OFFLINE_PACKAGE_INVALID"));
        let mut bad = approval.clone();
        bad.ci.jobs[2].conclusion = "failure".into();
        cases.push((bad, "WAKE_OFFLINE_CI_INVALID"));
        let mut bad = approval.clone();
        bad.ci.event = "workflow_dispatch".into();
        cases.push((bad, "WAKE_OFFLINE_CI_INVALID"));
        for (bad, reason) in cases {
            assert_eq!(check(&anchor, &bad, &observed, &key).unwrap_err(), reason);
        }
        let mut later = approval.clone();
        later.manifest.version = "1.0.0-dev.86".into();
        later.manifest.package_id = format!(
            "{}-{}-{}",
            later.manifest.version,
            &later.manifest.artifacts[0].sha256[..12],
            &later.manifest.artifacts[1].sha256[..12]
        );
        later.manifest_sha256 = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&later.manifest).unwrap())
        );
        let mut later_anchor = anchor.clone();
        later_anchor.candidate_manifest_sha256 = later.manifest_sha256.clone();
        later.anchor_envelope_sha256 = format!(
            "{:x}",
            Sha256::digest(signed(&later_anchor, ANCHOR_DOMAIN, &key))
        );
        assert_eq!(
            check(&later_anchor, &later, &observed, &key).unwrap_err(),
            "WAKE_OFFLINE_PACKAGE_INVALID"
        );
    }

    #[test]
    fn time_and_v1_wire_do_not_cross_the_v2_boundary() {
        let (anchor, approval, mut observed, key) = fixture();
        observed.now_utc_seconds = 2_001;
        assert_eq!(
            check(&anchor, &approval, &observed, &key).unwrap_err(),
            "WAKE_OFFLINE_TIME_INVALID"
        );
        observed.now_utc_seconds = 1_500;
        let (v1, v1_observed, _) = v1_test::fixture();
        let v1_wire = v1_test::envelope(&v1, &key);
        assert!(
            crate::install_broker_approval::verify_approval(
                &v1_wire,
                &key.verifying_key(),
                &v1_observed
            )
            .is_ok()
        );
        assert_eq!(
            verify_offline_pair_read_only(
                &v1_wire,
                &signed(&approval, APPROVAL_DOMAIN, &key),
                &key.verifying_key(),
                &observed,
            )
            .unwrap_err(),
            "WAKE_OFFLINE_ENCODING_INVALID"
        );
    }
}
