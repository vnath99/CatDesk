//! Exact, detached package approval for the *separately installed* Wake install
//! broker. This module does not grant installation authority to WakeHost. Only
//! a fixed broker, with its own protected trust root and observed object state,
//! may call it before a protected install transaction.

use crate::Result;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"CatDeskWakeInstallApprovalV1\0";
const MAX_ENVELOPE_BYTES: usize = 16 * 1024;
const MAX_APPROVAL_AGE_SECONDS: u64 = 7 * 24 * 60 * 60;
const BRANCH: &str = "orchestrator/chatgpt-codex-autonomous-loop";
const WORKFLOW: &str = ".github/workflows/ci.yml";
const REPOSITORY: &str = "vnath99/CatDesk";
const WORKFLOW_ID: u64 = 311_012_404;
const ARTIFACTS: [(&str, u64); 4] = [
    ("CatDeskWakeHost.exe", 80 * 1024 * 1024),
    ("CatDeskBinagotchy.exe", 80 * 1024 * 1024),
    ("adapter.py", 1024 * 1024),
    ("wake_bridge.py", 1024 * 1024),
];
const JOBS: [&str; 3] = [
    "Rust",
    "Python Wake and advisor tests",
    "Independent WakeHost Rust",
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObjectIdentityV1 {
    pub volume_serial: u32,
    pub file_index_high: u32,
    pub file_index_low: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactV1 {
    pub name: String,
    pub byte_length: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackageManifestV1 {
    pub schema_version: u32,
    pub package_id: String,
    pub version: String,
    pub artifacts: [ArtifactV1; 4],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CiJobV1 {
    pub name: String,
    pub job_id: u64,
    pub conclusion: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExactCiV1 {
    pub repository: String,
    pub workflow_path: String,
    pub workflow_id: u64,
    pub branch: String,
    pub head_sha: String,
    pub event: String,
    pub run_status: String,
    pub run_conclusion: String,
    pub run_id: u64,
    pub run_attempt: u32,
    pub jobs: [CiJobV1; 3],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WakeInstallApprovalV1 {
    pub schema_version: u32,
    pub purpose: String,
    pub operation: String,
    pub approval_id: String,
    pub issued_utc_seconds: u64,
    pub expires_utc_seconds: u64,
    pub install_generation: u64,
    pub manifest: PackageManifestV1,
    /// SHA-256 of the canonical JSON encoding of `manifest`, not a digest of
    /// caller-supplied metadata. The broker must compare staged manifest bytes.
    pub manifest_sha256: String,
    pub source_commit: String,
    pub ci: ExactCiV1,
    pub reviewer_identity: String,
    pub predecessor_package_id: String,
    pub predecessor_pointer: ObjectIdentityV1,
    pub target_protected_root: ObjectIdentityV1,
    pub old_runtime_root: ObjectIdentityV1,
    pub old_runtime_lock: ObjectIdentityV1,
    pub old_host_image: ObjectIdentityV1,
    pub old_host_image_sha256: String,
    pub old_host_pid: u32,
    pub old_host_creation_time_100ns: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApprovalEnvelopeV1 {
    payload: WakeInstallApprovalV1,
    signature_hex: String,
}

/// All fields here must come from the fixed broker's own protected records
/// and pinned object/process observations, never from the submitting client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerObservedStateV1 {
    pub now_utc_seconds: u64,
    pub next_install_generation: u64,
    pub predecessor_package_id: String,
    pub predecessor_pointer: ObjectIdentityV1,
    pub target_protected_root: ObjectIdentityV1,
    pub old_runtime_root: ObjectIdentityV1,
    pub old_runtime_lock: ObjectIdentityV1,
    pub old_host_image: ObjectIdentityV1,
    pub old_host_image_sha256: String,
    pub old_host_pid: u32,
    pub old_host_creation_time_100ns: u64,
}

/// A verified approval has no write capability. The offline issuer must have
/// independently checked the run and every job against GitHub. The future
/// broker must still inventory and hash pinned candidate handles, compare the
/// staged canonical manifest, persist a replay transaction, and recheck
/// physical objects before its first mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedApprovalV1 {
    payload: WakeInstallApprovalV1,
    signed_payload_sha256: String,
    signed_envelope_sha256: String,
    verification_key_sha256: String,
    verified_at_utc_seconds: u64,
}

impl VerifiedApprovalV1 {
    pub fn payload(&self) -> &WakeInstallApprovalV1 {
        &self.payload
    }

    pub fn signed_payload_sha256(&self) -> &str {
        &self.signed_payload_sha256
    }

    /// Digest of the exact canonical envelope, including its signature.
    pub fn signed_envelope_sha256(&self) -> &str {
        &self.signed_envelope_sha256
    }

    /// Digest of the verification key used for this proof. A transaction
    /// must also bind that key to the independently protected broker policy.
    pub fn verification_key_sha256(&self) -> &str {
        &self.verification_key_sha256
    }

    /// Time checked by the verifier. A future broker must supply its own
    /// current clock for a fresh acceptance, or the protected intent's
    /// original acceptance time for exact-only crash recovery.
    pub fn verified_at_utc_seconds(&self) -> u64 {
        self.verified_at_utc_seconds
    }
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

fn decode_hex<const N: usize>(hex: &str) -> Result<[u8; N]> {
    if !lowercase_hex(hex, N * 2) {
        return Err("WAKE_APPROVAL_SIGNATURE_INVALID".into());
    }
    let mut bytes = [0u8; N];
    for (byte, pair) in bytes.iter_mut().zip(hex.as_bytes().chunks_exact(2)) {
        let digit = |value: u8| -> u8 {
            match value {
                b'0'..=b'9' => value - b'0',
                b'a'..=b'f' => value - b'a' + 10,
                _ => unreachable!("lowercase_hex checked digits"),
            }
        };
        *byte = (digit(pair[0]) << 4) | digit(pair[1]);
    }
    Ok(bytes)
}

fn canonical_payload_bytes(payload: &WakeInstallApprovalV1) -> Result<Vec<u8>> {
    serde_json::to_vec(payload).map_err(|_| "WAKE_APPROVAL_ENCODING_INVALID".into())
}

fn signed_bytes(payload: &WakeInstallApprovalV1) -> Result<Vec<u8>> {
    let canonical = canonical_payload_bytes(payload)?;
    let mut bytes = Vec::with_capacity(DOMAIN.len() + canonical.len());
    bytes.extend_from_slice(DOMAIN);
    bytes.extend_from_slice(&canonical);
    Ok(bytes)
}

pub(crate) fn validate_manifest(manifest: &PackageManifestV1) -> Result<()> {
    if manifest.schema_version != 1 || !manifest.version.starts_with("1.0.0-dev.") {
        return Err("WAKE_APPROVAL_PACKAGE_INVALID".into());
    }
    let release = &manifest.version["1.0.0-dev.".len()..];
    if release.is_empty() || release.len() > 6 || !release.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("WAKE_APPROVAL_PACKAGE_INVALID".into());
    }
    for (artifact, (name, max_bytes)) in manifest.artifacts.iter().zip(ARTIFACTS) {
        if artifact.name != name
            || artifact.byte_length == 0
            || artifact.byte_length > max_bytes
            || !lowercase_hex(&artifact.sha256, 64)
        {
            return Err("WAKE_APPROVAL_PACKAGE_INVALID".into());
        }
    }
    let expected_id = format!(
        "{}-{}-{}",
        manifest.version,
        &manifest.artifacts[0].sha256[..12],
        &manifest.artifacts[1].sha256[..12]
    );
    if manifest.package_id != expected_id {
        return Err("WAKE_APPROVAL_PACKAGE_INVALID".into());
    }
    Ok(())
}

fn validate_payload(
    payload: &WakeInstallApprovalV1,
    observed: &BrokerObservedStateV1,
) -> Result<()> {
    if payload.schema_version != 1
        || payload.purpose != "catdesk-wake-protected-install"
        || payload.operation != "PUBLISH_AND_ACTIVATE"
        || !lowercase_hex(&payload.approval_id, 32)
        || !identifier(&payload.reviewer_identity, 128)
    {
        return Err("WAKE_APPROVAL_PURPOSE_INVALID".into());
    }
    if payload.issued_utc_seconds > observed.now_utc_seconds
        || observed.now_utc_seconds > payload.expires_utc_seconds
        || payload.expires_utc_seconds <= payload.issued_utc_seconds
        || payload.expires_utc_seconds - payload.issued_utc_seconds > MAX_APPROVAL_AGE_SECONDS
    {
        return Err("WAKE_APPROVAL_TIME_INVALID".into());
    }
    validate_manifest(&payload.manifest)?;
    let manifest_bytes = serde_json::to_vec(&payload.manifest)
        .map_err(|_| "WAKE_APPROVAL_ENCODING_INVALID".to_string())?;
    let manifest_digest = format!("{:x}", Sha256::digest(manifest_bytes));
    if payload.manifest_sha256 != manifest_digest {
        return Err("WAKE_APPROVAL_MANIFEST_INVALID".into());
    }
    if !lowercase_hex(&payload.source_commit, 40)
        || payload.ci.head_sha != payload.source_commit
        || payload.ci.repository != REPOSITORY
        || payload.ci.workflow_path != WORKFLOW
        || payload.ci.workflow_id != WORKFLOW_ID
        || payload.ci.branch != BRANCH
        || payload.ci.event != "push"
        || payload.ci.run_status != "completed"
        || payload.ci.run_conclusion != "success"
        || payload.ci.run_id == 0
        || payload.ci.run_attempt == 0
    {
        return Err("WAKE_APPROVAL_CI_INVALID".into());
    }
    for (job, expected) in payload.ci.jobs.iter().zip(JOBS) {
        if job.name != expected || job.job_id == 0 || job.conclusion != "success" {
            return Err("WAKE_APPROVAL_CI_INVALID".into());
        }
    }
    if payload.ci.jobs[0].job_id == payload.ci.jobs[1].job_id
        || payload.ci.jobs[0].job_id == payload.ci.jobs[2].job_id
        || payload.ci.jobs[1].job_id == payload.ci.jobs[2].job_id
    {
        return Err("WAKE_APPROVAL_CI_INVALID".into());
    }
    if payload.install_generation != observed.next_install_generation
        || payload.install_generation == 0
        || !identifier(&payload.predecessor_package_id, 200)
        || !valid_object(&payload.predecessor_pointer)
        || !valid_object(&payload.target_protected_root)
        || !valid_object(&payload.old_runtime_root)
        || !valid_object(&payload.old_runtime_lock)
        || !valid_object(&payload.old_host_image)
        || !lowercase_hex(&payload.old_host_image_sha256, 64)
        || payload.target_protected_root == payload.old_runtime_root
        || payload.old_runtime_root == payload.old_runtime_lock
        || payload.predecessor_package_id != observed.predecessor_package_id
        || payload.predecessor_pointer != observed.predecessor_pointer
        || payload.target_protected_root != observed.target_protected_root
        || payload.old_runtime_root != observed.old_runtime_root
        || payload.old_runtime_lock != observed.old_runtime_lock
        || payload.old_host_image != observed.old_host_image
        || payload.old_host_image_sha256 != observed.old_host_image_sha256
        || payload.old_host_pid != observed.old_host_pid
        || payload.old_host_creation_time_100ns != observed.old_host_creation_time_100ns
        || payload.old_host_pid == 0
        || payload.old_host_creation_time_100ns == 0
    {
        return Err("WAKE_APPROVAL_OBJECT_MISMATCH".into());
    }
    Ok(())
}

/// Verify a detached signature using a key supplied only by the broker's
/// independently installed protected policy. There is deliberately no key or
/// path in the untrusted envelope and no candidate-host call site.
pub fn verify_approval(
    envelope_bytes: &[u8],
    broker_trust_root: &VerifyingKey,
    observed: &BrokerObservedStateV1,
) -> Result<VerifiedApprovalV1> {
    if envelope_bytes.is_empty() || envelope_bytes.len() > MAX_ENVELOPE_BYTES {
        return Err("WAKE_APPROVAL_ENCODING_INVALID".into());
    }
    let envelope: ApprovalEnvelopeV1 = serde_json::from_slice(envelope_bytes)
        .map_err(|_| "WAKE_APPROVAL_ENCODING_INVALID".to_string())?;
    let canonical =
        serde_json::to_vec(&envelope).map_err(|_| "WAKE_APPROVAL_ENCODING_INVALID".to_string())?;
    if canonical != envelope_bytes {
        return Err("WAKE_APPROVAL_ENCODING_INVALID".into());
    }
    validate_payload(&envelope.payload, observed)?;
    let signature = Signature::from_bytes(&decode_hex::<64>(&envelope.signature_hex)?);
    let signed = signed_bytes(&envelope.payload)?;
    broker_trust_root
        .verify_strict(&signed, &signature)
        .map_err(|_| "WAKE_APPROVAL_SIGNATURE_INVALID".to_string())?;
    Ok(VerifiedApprovalV1 {
        payload: envelope.payload,
        signed_payload_sha256: format!("{:x}", Sha256::digest(signed)),
        signed_envelope_sha256: format!("{:x}", Sha256::digest(envelope_bytes)),
        verification_key_sha256: format!("{:x}", Sha256::digest(broker_trust_root.as_bytes())),
        verified_at_utc_seconds: observed.now_utc_seconds,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn object(id: u32) -> ObjectIdentityV1 {
        ObjectIdentityV1 {
            volume_serial: 123,
            file_index_high: 0,
            file_index_low: id,
        }
    }

    pub(crate) fn fixture() -> (WakeInstallApprovalV1, BrokerObservedStateV1, SigningKey) {
        let artifacts = ARTIFACTS.map(|(name, _)| ArtifactV1 {
            name: name.into(),
            byte_length: 100,
            sha256: format!("{:064x}", name.len()),
        });
        let version = "1.0.0-dev.85".to_string();
        let manifest = PackageManifestV1 {
            schema_version: 1,
            package_id: format!(
                "{}-{}-{}",
                version,
                &artifacts[0].sha256[..12],
                &artifacts[1].sha256[..12]
            ),
            version,
            artifacts,
        };
        let payload = WakeInstallApprovalV1 {
            schema_version: 1,
            purpose: "catdesk-wake-protected-install".into(),
            operation: "PUBLISH_AND_ACTIVATE".into(),
            approval_id: "a".repeat(32),
            issued_utc_seconds: 1_000,
            expires_utc_seconds: 2_000,
            install_generation: 2,
            manifest_sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&manifest).expect("manifest"))
            ),
            manifest,
            source_commit: "b".repeat(40),
            ci: ExactCiV1 {
                repository: REPOSITORY.into(),
                workflow_path: WORKFLOW.into(),
                workflow_id: WORKFLOW_ID,
                branch: BRANCH.into(),
                head_sha: "b".repeat(40),
                event: "push".into(),
                run_status: "completed".into(),
                run_conclusion: "success".into(),
                run_id: 123,
                run_attempt: 1,
                jobs: JOBS.map(|name| CiJobV1 {
                    name: name.into(),
                    job_id: 100 + name.len() as u64,
                    conclusion: "success".into(),
                }),
            },
            reviewer_identity: "operator@example.test".into(),
            predecessor_package_id: "1.0.0-dev.84-example".into(),
            predecessor_pointer: object(4),
            target_protected_root: object(1),
            old_runtime_root: object(2),
            old_runtime_lock: object(3),
            old_host_image: object(5),
            old_host_image_sha256: "c".repeat(64),
            old_host_pid: 42,
            old_host_creation_time_100ns: 7,
        };
        let observed = BrokerObservedStateV1 {
            now_utc_seconds: 1_500,
            next_install_generation: payload.install_generation,
            predecessor_package_id: payload.predecessor_package_id.clone(),
            predecessor_pointer: payload.predecessor_pointer.clone(),
            target_protected_root: payload.target_protected_root.clone(),
            old_runtime_root: payload.old_runtime_root.clone(),
            old_runtime_lock: payload.old_runtime_lock.clone(),
            old_host_image: payload.old_host_image.clone(),
            old_host_image_sha256: payload.old_host_image_sha256.clone(),
            old_host_pid: payload.old_host_pid,
            old_host_creation_time_100ns: payload.old_host_creation_time_100ns,
        };
        (payload, observed, SigningKey::from_bytes(&[7u8; 32]))
    }

    pub(crate) fn envelope(payload: &WakeInstallApprovalV1, key: &SigningKey) -> Vec<u8> {
        let signature = key.sign(&signed_bytes(payload).expect("signed bytes"));
        serde_json::to_vec(&ApprovalEnvelopeV1 {
            payload: payload.clone(),
            signature_hex: signature
                .to_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        })
        .expect("envelope")
    }

    #[test]
    fn exact_approval_is_verified_and_binds_all_observed_objects() {
        let (payload, observed, key) = fixture();
        let bytes = envelope(&payload, &key);
        let verified = verify_approval(&bytes, &key.verifying_key(), &observed).expect("verified");
        assert_eq!(verified.payload, payload);
        assert!(lowercase_hex(&verified.signed_payload_sha256, 64));
    }

    #[test]
    fn rejects_wrong_key_and_tampering_even_when_canonical() {
        let (mut payload, observed, key) = fixture();
        let bytes = envelope(&payload, &key);
        let wrong = SigningKey::from_bytes(&[8u8; 32]);
        assert_eq!(
            verify_approval(&bytes, &wrong.verifying_key(), &observed).unwrap_err(),
            "WAKE_APPROVAL_SIGNATURE_INVALID"
        );
        payload.reviewer_identity = "other@example.test".into();
        let changed = ApprovalEnvelopeV1 {
            payload,
            signature_hex: serde_json::from_slice::<ApprovalEnvelopeV1>(&bytes)
                .expect("envelope")
                .signature_hex,
        };
        assert_eq!(
            verify_approval(
                &serde_json::to_vec(&changed).expect("canonical"),
                &key.verifying_key(),
                &observed
            )
            .unwrap_err(),
            "WAKE_APPROVAL_SIGNATURE_INVALID"
        );
    }

    #[test]
    fn rejects_noncanonical_duplicate_unknown_and_oversized_wire() {
        let (payload, observed, key) = fixture();
        let bytes = envelope(&payload, &key);
        let mut whitespace = bytes.clone();
        whitespace.push(b' ');
        let duplicate_payload = [
            &bytes[..bytes.len() - 1],
            b",\"payload\":".as_slice(),
            serde_json::to_vec(&payload).expect("payload").as_slice(),
            b"}".as_slice(),
        ]
        .concat();
        let unknown_field = [&bytes[..bytes.len() - 1], b",\"unknown\":1}"].concat();
        let mut extra_artifact: serde_json::Value =
            serde_json::from_slice(&bytes).expect("envelope value");
        extra_artifact["payload"]["manifest"]["artifacts"]
            .as_array_mut()
            .expect("artifact array")
            .push(serde_json::to_value(&payload.manifest.artifacts[0]).expect("artifact"));
        let mut reordered: serde_json::Value =
            serde_json::from_slice(&bytes).expect("envelope value");
        reordered["payload"]["reviewerIdentity"] =
            serde_json::Value::String(payload.reviewer_identity.clone());
        for bad in [
            whitespace,
            duplicate_payload,
            unknown_field,
            serde_json::to_vec(&extra_artifact).expect("extra artifact"),
            serde_json::to_vec(&reordered).expect("reordered fields"),
            vec![b' '; MAX_ENVELOPE_BYTES + 1],
        ] {
            assert_eq!(
                verify_approval(&bad, &key.verifying_key(), &observed).unwrap_err(),
                "WAKE_APPROVAL_ENCODING_INVALID"
            );
        }
    }

    #[test]
    fn rejects_purpose_ci_package_and_manifest_mismatch_with_valid_signatures() {
        let (payload, observed, key) = fixture();
        let mut cases = Vec::new();
        let mut wrong = payload.clone();
        wrong.purpose = "catdesk-main-image".into();
        cases.push((wrong, "WAKE_APPROVAL_PURPOSE_INVALID"));
        let mut wrong = payload.clone();
        wrong.ci.head_sha = "c".repeat(40);
        cases.push((wrong, "WAKE_APPROVAL_CI_INVALID"));
        let mut wrong = payload.clone();
        wrong.ci.jobs[2].conclusion = "failure".into();
        cases.push((wrong, "WAKE_APPROVAL_CI_INVALID"));
        let mut wrong = payload.clone();
        wrong.ci.event = "workflow_dispatch".into();
        cases.push((wrong, "WAKE_APPROVAL_CI_INVALID"));
        let mut wrong = payload.clone();
        wrong.ci.run_conclusion = "failure".into();
        cases.push((wrong, "WAKE_APPROVAL_CI_INVALID"));
        let mut wrong = payload.clone();
        wrong.ci.workflow_id += 1;
        cases.push((wrong, "WAKE_APPROVAL_CI_INVALID"));
        let mut wrong = payload.clone();
        wrong.manifest.artifacts[3].name = "other.py".into();
        cases.push((wrong, "WAKE_APPROVAL_PACKAGE_INVALID"));
        let mut wrong = payload.clone();
        wrong.manifest.artifacts[0].byte_length += 1;
        cases.push((wrong, "WAKE_APPROVAL_MANIFEST_INVALID"));
        for (bad, reason) in cases {
            assert_eq!(
                verify_approval(&envelope(&bad, &key), &key.verifying_key(), &observed)
                    .unwrap_err(),
                reason
            );
        }
    }

    #[test]
    fn rejects_copied_approval_for_other_predecessor_root_owner_or_generation() {
        let (payload, observed, key) = fixture();
        let bytes = envelope(&payload, &key);
        let mut cases = Vec::new();
        let mut wrong = observed.clone();
        wrong.predecessor_package_id.push_str("-other");
        cases.push(wrong);
        let mut wrong = observed.clone();
        wrong.target_protected_root.file_index_low += 1;
        cases.push(wrong);
        let mut wrong = observed.clone();
        wrong.predecessor_pointer.file_index_low += 1;
        cases.push(wrong);
        let mut wrong = observed.clone();
        wrong.old_runtime_lock.file_index_low += 1;
        cases.push(wrong);
        let mut wrong = observed.clone();
        wrong.old_host_image_sha256 = "d".repeat(64);
        cases.push(wrong);
        let mut wrong = observed.clone();
        wrong.old_host_image.file_index_low += 1;
        cases.push(wrong);
        let mut wrong = observed.clone();
        wrong.old_host_creation_time_100ns += 1;
        cases.push(wrong);
        let mut wrong = observed.clone();
        wrong.next_install_generation += 1;
        cases.push(wrong);
        for other in cases {
            assert_eq!(
                verify_approval(&bytes, &key.verifying_key(), &other).unwrap_err(),
                "WAKE_APPROVAL_OBJECT_MISMATCH"
            );
        }
    }

    #[test]
    fn rejects_expired_or_future_or_overlong_approval() {
        let (mut payload, mut observed, key) = fixture();
        observed.now_utc_seconds = 2_001;
        assert_eq!(
            verify_approval(&envelope(&payload, &key), &key.verifying_key(), &observed)
                .unwrap_err(),
            "WAKE_APPROVAL_TIME_INVALID"
        );
        observed.now_utc_seconds = 999;
        assert_eq!(
            verify_approval(&envelope(&payload, &key), &key.verifying_key(), &observed)
                .unwrap_err(),
            "WAKE_APPROVAL_TIME_INVALID"
        );
        observed.now_utc_seconds = 1_500;
        payload.expires_utc_seconds = payload.issued_utc_seconds + MAX_APPROVAL_AGE_SECONDS + 1;
        assert_eq!(
            verify_approval(&envelope(&payload, &key), &key.verifying_key(), &observed)
                .unwrap_err(),
            "WAKE_APPROVAL_TIME_INVALID"
        );
    }
}
