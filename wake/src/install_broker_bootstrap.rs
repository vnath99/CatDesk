//! First provisioning of the existing fixed broker, never a release approval.
//! A separately held key signs this purpose; native invocation additionally
//! pins the exact envelope and public-key fingerprints. No private key enters.
#![cfg(windows)]

use crate::{Result, install_broker_offline_v2::FullFileIdentityV2};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"CatDeskWakeFirstBrokerBootstrapV1\0";
const MAX_ENVELOPE: usize = 16 * 1024;
const PURPOSE: &str = "FIRST_FIXED_BROKER_PROVISION_ONLY";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BootstrapArtifactV1 {
    pub(crate) path: String,
    pub(crate) byte_length: u64,
    pub(crate) sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BootstrapInstallerV1 {
    pub(crate) byte_length: u64,
    pub(crate) sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BootstrapStateV1 {
    pub(crate) path: String,
    pub(crate) identity: FullFileIdentityV2,
    pub(crate) current_sha256: String,
    pub(crate) control_sha256: String,
    pub(crate) config_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BootstrapManifestV1 {
    pub(crate) schema_version: u32,
    pub(crate) purpose: String,
    pub(crate) bootstrap_id: String,
    pub(crate) issued_utc_seconds: u64,
    pub(crate) expires_utc_seconds: u64,
    pub(crate) source_commit: String,
    pub(crate) installer: BootstrapInstallerV1,
    pub(crate) broker_artifact: BootstrapArtifactV1,
    pub(crate) independent_review: BootstrapArtifactV1,
    pub(crate) approval_public_key_hex: String,
    pub(crate) approval_public_key_sha256: String,
    pub(crate) operator_sid: String,
    pub(crate) reviewer_identity: String,
    pub(crate) state_root: BootstrapStateV1,
    pub(crate) allow_reviewed_reconstruction: bool,
    pub(crate) allow_operator_service_start: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BootstrapEnvelopeV1 {
    payload: BootstrapManifestV1,
    signature_hex: String,
}

/// No public constructor; this is a signed bootstrap purpose, not a release
/// qualification/grant and cannot be passed into any selection transaction.
pub(crate) struct VerifiedBootstrapV1 {
    pub(crate) manifest: BootstrapManifestV1,
    envelope_sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IndependentBootstrapReviewV1 {
    schema_version: u32,
    kind: String,
    scope: String,
    source_commit: String,
    installer_sha256: String,
    broker_artifact_sha256: String,
    reviewer_identity: String,
    independent: bool,
    full_bootstrap_source_reviewed: bool,
    verdict: String,
    findings_count: u64,
    needed_reads_count: u64,
    evidence_sha256: Vec<String>,
}

pub(crate) fn digest(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}
fn hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn decoded<const N: usize>(value: &str) -> Result<[u8; N]> {
    if !hex(value, N * 2) {
        return Err("WAKE_BOOTSTRAP_SIGNATURE_INVALID".into());
    }
    let mut result = [0; N];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        result[index] = u8::from_str_radix(
            std::str::from_utf8(pair).map_err(|_| "WAKE_BOOTSTRAP_SIGNATURE_INVALID")?,
            16,
        )
        .map_err(|_| "WAKE_BOOTSTRAP_SIGNATURE_INVALID")?;
    }
    Ok(result)
}
fn local_path(value: &str) -> bool {
    let raw = value.as_bytes();
    raw.len() >= 3
        && raw.len() <= 2048
        && raw[0].is_ascii_alphabetic()
        && raw[1] == b':'
        && matches!(raw[2], b'\\' | b'/')
        && !value.chars().any(char::is_control)
        && !std::path::Path::new(value).components().any(|c| {
            matches!(
                c,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
        && !value.split(['\\', '/']).any(|v| matches!(v, "." | ".."))
        && !value[2..].contains(':')
}

fn verify_bootstrap(
    raw: &[u8],
    expected_sha256: &str,
    expected_key_sha256: &str,
    now: u64,
) -> Result<VerifiedBootstrapV1> {
    if raw.is_empty()
        || raw.len() > MAX_ENVELOPE
        || !hex(expected_sha256, 64)
        || digest(raw) != expected_sha256
        || !hex(expected_key_sha256, 64)
    {
        return Err("WAKE_BOOTSTRAP_ENVELOPE_IDENTITY_INVALID".into());
    }
    let envelope: BootstrapEnvelopeV1 =
        serde_json::from_slice(raw).map_err(|_| "WAKE_BOOTSTRAP_ENCODING_INVALID")?;
    if serde_json::to_vec(&envelope).map_err(|_| "WAKE_BOOTSTRAP_ENCODING_INVALID")? != raw {
        return Err("WAKE_BOOTSTRAP_ENCODING_INVALID".into());
    }
    let m = &envelope.payload;
    let key_raw = decoded::<32>(&m.approval_public_key_hex)?;
    let key = VerifyingKey::from_bytes(&key_raw).map_err(|_| "WAKE_BOOTSTRAP_KEY_INVALID")?;
    if key.is_weak()
        || digest(&key_raw) != expected_key_sha256
        || m.approval_public_key_sha256 != expected_key_sha256
    {
        return Err("WAKE_BOOTSTRAP_KEY_INVALID".into());
    }
    if m.schema_version != 1
        || m.purpose != PURPOSE
        || !crate::protocol::valid_id(&m.bootstrap_id)
        || !crate::protocol::valid_id(&m.reviewer_identity)
        || m.reviewer_identity.len() > 128
        || !hex(&m.source_commit, 40)
        || m.issued_utc_seconds == 0
        || m.issued_utc_seconds > now
        || now > m.expires_utc_seconds
        || m.expires_utc_seconds <= m.issued_utc_seconds
        || m.expires_utc_seconds - m.issued_utc_seconds > 86400
        || !m.allow_operator_service_start
        || !m.operator_sid.starts_with("S-1-")
        || m.operator_sid.len() > 128
        || !local_path(&m.state_root.path)
        || m.state_root.identity.volume_serial == 0
        || !hex(&m.state_root.identity.file_id_hex, 32)
        || m.state_root.identity.file_id_hex == "0".repeat(32)
        || [
            &m.installer.sha256,
            &m.broker_artifact.sha256,
            &m.independent_review.sha256,
            &m.state_root.current_sha256,
            &m.state_root.control_sha256,
            &m.state_root.config_sha256,
        ]
        .iter()
        .any(|s| !hex(s, 64))
        || m.installer.byte_length == 0
        || m.installer.byte_length > 80 * 1024 * 1024
        || m.broker_artifact.byte_length == 0
        || m.broker_artifact.byte_length > 80 * 1024 * 1024
        || m.independent_review.byte_length == 0
        || m.independent_review.byte_length > 64 * 1024
        || !local_path(&m.broker_artifact.path)
        || !local_path(&m.independent_review.path)
    {
        return Err("WAKE_BOOTSTRAP_SCOPE_INVALID".into());
    }
    let mut signed = DOMAIN.to_vec();
    signed.extend(serde_json::to_vec(m).map_err(|_| "WAKE_BOOTSTRAP_ENCODING_INVALID")?);
    key.verify_strict(
        &signed,
        &Signature::from_bytes(&decoded::<64>(&envelope.signature_hex)?),
    )
    .map_err(|_| "WAKE_BOOTSTRAP_SIGNATURE_INVALID")?;
    Ok(VerifiedBootstrapV1 {
        manifest: envelope.payload,
        envelope_sha256: expected_sha256.into(),
    })
}

pub(crate) fn validate_independent_review(
    raw: &[u8],
    manifest: &BootstrapManifestV1,
) -> Result<()> {
    let r: IndependentBootstrapReviewV1 =
        serde_json::from_slice(raw).map_err(|_| "WAKE_BOOTSTRAP_REVIEW_INVALID")?;
    if r.schema_version != 1
        || r.kind != "INDEPENDENT_BOOTSTRAP_REVIEW"
        || r.scope != PURPOSE
        || r.source_commit != manifest.source_commit
        || r.installer_sha256 != manifest.installer.sha256
        || r.broker_artifact_sha256 != manifest.broker_artifact.sha256
        || r.reviewer_identity != manifest.reviewer_identity
        || !r.independent
        || !r.full_bootstrap_source_reviewed
        || r.verdict != "PASS"
        || r.findings_count != 0
        || r.needed_reads_count != 0
        || r.evidence_sha256.is_empty()
        || r.evidence_sha256.len() > 32
        || r.evidence_sha256.iter().any(|v| !hex(v, 64))
        || r.evidence_sha256
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != r.evidence_sha256.len()
    {
        return Err("WAKE_BOOTSTRAP_REVIEW_INVALID".into());
    }
    Ok(())
}

fn readback(verified: &VerifiedBootstrapV1) -> Value {
    json!({"schemaVersion":1,"purpose":PURPOSE,"bootstrapId":verified.manifest.bootstrap_id,
        "envelopeSha256":verified.envelope_sha256,"sourceCommit":verified.manifest.source_commit,
        "brokerArtifactSha256":verified.manifest.broker_artifact.sha256,
        "approvalPublicKeySha256":verified.manifest.approval_public_key_sha256,
        "releaseApprovalCreated":false,"releaseReviewAuthorityCreated":false,
        "workingReleaseQualified":false,"selectionChanged":false,"serviceStarted":false,
        "namespacePowerLossQualified":false})
}

/// Bounded read-only native plan. No service, ACL, directory or key is created.
fn inspect_bootstrap_plan(
    raw: &[u8],
    expected_sha256: &str,
    expected_key_sha256: &str,
) -> Result<Value> {
    let verified = verify_bootstrap(
        raw,
        expected_sha256,
        expected_key_sha256,
        crate::store::now(),
    )?;
    let _operator =
        crate::install_broker_operator::admit_bootstrap_operator(&verified.manifest.operator_sid)?;
    crate::install_broker_bootstrap_files::RestorePrivilege::check_available()?;
    let files =
        crate::install_broker_bootstrap_files::FixedBootstrapFiles::observe(&verified.manifest)?;
    let scm = crate::install_broker_bootstrap_scm::BootstrapScmAbsent::observe()?;
    files.revalidate_absent()?;
    scm.revalidate_absent()?;
    let mut value = readback(&verified);
    value["status"] = json!("FIRST_BOOTSTRAP_INPUTS_AND_ABSENCE_VERIFIED_NO_MUTATION");
    value["operatorInvocationStillRequired"] = json!(true);
    Ok(value)
}

/// The sole first-provision mutation composition. Every failure preserves
/// partial objects and refuses subsequent create-new attempts; no cleanup,
/// replacement, signing, release admission or automatic service start occurs.
fn provision_fixed_broker(
    raw: &[u8],
    expected_sha256: &str,
    expected_key_sha256: &str,
) -> Result<Value> {
    let verified = verify_bootstrap(
        raw,
        expected_sha256,
        expected_key_sha256,
        crate::store::now(),
    )?;
    let manifest = &verified.manifest;
    // This evidence is native current-process admission, never caller JSON.
    let _operator =
        crate::install_broker_operator::admit_bootstrap_operator(&manifest.operator_sid)?;
    let mut files = crate::install_broker_bootstrap_files::FixedBootstrapFiles::observe(manifest)?;
    let scm = crate::install_broker_bootstrap_scm::BootstrapScmAbsent::observe()?;
    let _restore = crate::install_broker_bootstrap_files::RestorePrivilege::acquire()?;
    files.revalidate_absent()?;
    scm.revalidate_absent()?;
    let final_now = crate::store::now();
    if final_now < manifest.issued_utc_seconds || final_now > manifest.expires_utc_seconds {
        return Err("WAKE_BOOTSTRAP_FINAL_CLOCK_INVALID".into());
    }
    let mut created = scm.create_stopped(
        &manifest.operator_sid,
        manifest.allow_operator_service_start,
    )?;
    let provisioned = files.create_fixed(created.service_sid(), manifest)?;
    provisioned.revalidate()?;
    created.revalidate_stopped()?;
    files.revalidate_state_and_sources()?;
    created.finalize_security()?;
    provisioned.revalidate()?;
    created.revalidate_stopped()?;
    // Existing readers independently consume the final native layout/policy.
    let final_snapshot = crate::install_broker_preflight::inspect_fixed_broker_read_only()?;
    let policy = crate::install_broker_policy::load_fixed_broker_policy_read_only()?;
    let policy_readback = policy.readback()?;
    if final_snapshot.service_image_sha256 != manifest.broker_artifact.sha256
        || policy_readback.policy_sha256 != provisioned.policy_sha256()
    {
        return Err("WAKE_BOOTSTRAP_FINAL_READBACK_MISMATCH".into());
    }
    let mut value = readback(&verified);
    value["status"] = json!("FIXED_BROKER_PROVISIONED_STOPPED_START_ONCE_AVAILABLE");
    value["policySha256"] = json!(policy_readback.policy_sha256);
    value["serviceStopped"] = json!(true);
    value["protectedNamespaceCreated"] = json!(true);
    value["operatorStartOnceConfigured"] = json!(true);
    Ok(value)
}

pub fn preflight_fixed_broker(
    path: &std::path::Path,
    expected_approval_sha256: &str,
    expected_public_key_sha256: &str,
) -> Result<Value> {
    let raw = crate::install_broker_bootstrap_files::read_bootstrap_approval(path)?;
    inspect_bootstrap_plan(&raw, expected_approval_sha256, expected_public_key_sha256)
}

pub fn bootstrap_fixed_broker(
    path: &std::path::Path,
    expected_approval_sha256: &str,
    expected_public_key_sha256: &str,
) -> Result<Value> {
    let raw = crate::install_broker_bootstrap_files::read_bootstrap_approval(path)?;
    provision_fixed_broker(&raw, expected_approval_sha256, expected_public_key_sha256)
}

#[cfg(test)]
#[path = "install_broker_bootstrap_tests.rs"]
mod tests;
