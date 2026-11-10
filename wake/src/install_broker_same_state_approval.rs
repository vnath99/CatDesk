//! Signed same-state-root selection evidence. Signature proof is read-only;
//! only the fixed policy, admitted native operator and retained physical pins
//! may turn it into the separate policy-owned transaction capability.

use crate::{
    Result,
    install_broker_approval::{PackageManifestV1, validate_manifest},
    install_broker_offline_v2::FullFileIdentityV2,
};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"CatDeskWakeSameStateRootSelectionApprovalV3\0";
const MAX_ENVELOPE_BYTES: usize = 16 * 1024;
const MAX_AGE_SECONDS: u64 = 7 * 24 * 60 * 60;

pub const SAME_STATE_SELECT_OPERATION: &str = "SELECT_SAME_STATE_RELEASE";
pub const SAME_STATE_ROLLBACK_OPERATION: &str = "ROLLBACK_SAME_STATE_RELEASE";

/// Read-only operation classification; it never supplies admission authority.
pub fn is_same_state_rollback_operation(operation: &str) -> bool {
    operation == SAME_STATE_ROLLBACK_OPERATION
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SameStateObjectV3 {
    pub identity: FullFileIdentityV2,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SameStateSnapshotV3 {
    pub root: FullFileIdentityV2,
    pub current: SameStateObjectV3,
    pub control: SameStateObjectV3,
    pub config: SameStateObjectV3,
    pub desired: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SameStateEvidenceVerdictV3 {
    Pass,
    Fail,
    Inconclusive,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SameStateLocalQualificationV3 {
    pub receipt_sha256: String,
    pub source_commit: String,
    pub manifest_sha256: String,
    pub verdict: SameStateEvidenceVerdictV3,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SameStateIndependentReviewV3 {
    pub receipt_sha256: String,
    pub source_commit: String,
    pub manifest_sha256: String,
    pub reviewer_identity: String,
    pub verdict: SameStateEvidenceVerdictV3,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SameStateWorkingEvidenceV3 {
    pub scope: String,
    pub receipt_sha256: String,
    pub source_commit: String,
    pub manifest_sha256: String,
    pub observed_utc_seconds: u64,
    pub owner_verified: bool,
    pub startup_verified: bool,
    pub normal_stop_verified: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SameStateQualifiedReleaseV3 {
    pub manifest: PackageManifestV1,
    /// Exact raw bytes of the runtime schema-1 MAP manifest, not the broker
    /// array manifest. Both commitments must match independently pinned data.
    pub runtime_manifest_sha256: String,
    pub source_commit: String,
    pub local_qualification: SameStateLocalQualificationV3,
    pub independent_review: SameStateIndependentReviewV3,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_evidence: Option<SameStateWorkingEvidenceV3>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum SameStateRollbackAuthorityV3 {
    VerifiedWorkingPrevious {
        release: SameStateQualifiedReleaseV3,
    },
    ReviewedSourceReconstruction {
        release: SameStateQualifiedReleaseV3,
        #[serde(rename = "oldRawSelectionEscrowSha256")]
        old_raw_selection_escrow_sha256: String,
        #[serde(rename = "reconstructionReceiptSha256")]
        reconstruction_receipt_sha256: String,
    },
}

impl SameStateRollbackAuthorityV3 {
    pub fn release(&self) -> &SameStateQualifiedReleaseV3 {
        match self {
            Self::VerifiedWorkingPrevious { release }
            | Self::ReviewedSourceReconstruction { release, .. } => release,
        }
    }
    pub fn is_reconstruction(&self) -> bool {
        matches!(self, Self::ReviewedSourceReconstruction { .. })
    }
}

/// Twelve top-level fields. No caller path, PID, key or synthetic CI tuple.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SameStateRootSelectionApprovalV3 {
    pub schema_version: u32,
    pub operation: String,
    pub approval_id: String,
    pub issued_utc_seconds: u64,
    pub expires_utc_seconds: u64,
    pub install_generation: u64,
    pub reviewer_identity: String,
    pub policy_sha256: String,
    pub state_snapshot: SameStateSnapshotV3,
    pub candidate: SameStateQualifiedReleaseV3,
    pub rollback_authority: SameStateRollbackAuthorityV3,
    pub planned_selection_sha256: String,
}

/// Pure comparison data, never transaction authority. The policy factory must
/// derive this from its own pinned resources and protected ledger clock.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SameStateRetainedResumeV3 {
    pub original_snapshot_sha256: String,
    pub before_selection_sha256: String,
    pub planned_selection_sha256: String,
    pub original_escrow_sha256: String,
    /// Derived from the exact protected ledger attempt, never request input.
    pub selection_attempt_present: bool,
}

/// Derived only from the parsed ORIGINAL selection/retained escrow by the
/// policy-owned resource reader. No filesystem locator or current health.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SameStatePreviousSelectionV3 {
    pub pointer: serde_json::Value,
    pub qualification_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SameStateObservedV3 {
    pub now_utc_seconds: u64,
    pub next_install_generation: u64,
    pub policy_sha256: String,
    pub state_snapshot: SameStateSnapshotV3,
    pub candidate_manifest_sha256: String,
    pub candidate_runtime_manifest_sha256: String,
    pub previous_manifest_sha256: String,
    pub previous_runtime_manifest_sha256: String,
    pub planned_selection_sha256: String,
    pub allow_reviewed_reconstruction: bool,
    pub approved_reviewer_identity: String,
    /// Derived from the pinned before-image parser; Some only for the legacy
    /// flat document. Reconstruction is deliberately limited to that cutover.
    pub legacy_current_package_id: Option<String>,
    pub retained_resume: Option<SameStateRetainedResumeV3>,
    pub original_previous: Option<SameStatePreviousSelectionV3>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedSameStateSelectionApprovalV3 {
    payload: SameStateRootSelectionApprovalV3,
    signed_payload_sha256: String,
    signed_envelope_sha256: String,
    verification_key_sha256: String,
    verified_at_utc_seconds: u64,
}

impl VerifiedSameStateSelectionApprovalV3 {
    pub fn payload(&self) -> &SameStateRootSelectionApprovalV3 {
        &self.payload
    }
    pub fn signed_payload_sha256(&self) -> &str {
        &self.signed_payload_sha256
    }
    pub fn signed_envelope_sha256(&self) -> &str {
        &self.signed_envelope_sha256
    }
    pub fn verification_key_sha256(&self) -> &str {
        &self.verification_key_sha256
    }
    pub fn verified_at_utc_seconds(&self) -> u64 {
        self.verified_at_utc_seconds
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignedEnvelopeV3 {
    payload: SameStateRootSelectionApprovalV3,
    signature_hex: String,
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn identity(value: &FullFileIdentityV2) -> bool {
    value.volume_serial != 0 && hex(&value.file_id_hex, 32) && value.file_id_hex != "0".repeat(32)
}

fn valid_snapshot(value: &SameStateSnapshotV3) -> bool {
    let files = [&value.current, &value.control, &value.config];
    identity(&value.root)
        && value.desired == "STOPPED"
        && files.iter().all(|f| {
            identity(&f.identity)
                && hex(&f.sha256, 64)
                && f.identity.volume_serial == value.root.volume_serial
                && f.identity != value.root
        })
        && files[0].identity != files[1].identity
        && files[0].identity != files[2].identity
        && files[1].identity != files[2].identity
}

pub fn same_state_manifest_sha256(manifest: &PackageManifestV1) -> Result<String> {
    validate_manifest(manifest).map_err(|_| "WAKE_SELECTION_PACKAGE_INVALID")?;
    let bytes = serde_json::to_vec(manifest).map_err(|_| "WAKE_SELECTION_ENCODING_INVALID")?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn same_state_release_pointer_v3(
    release: &SameStateQualifiedReleaseV3,
) -> Result<serde_json::Value> {
    validate_manifest(&release.manifest).map_err(|_| "WAKE_SELECTION_PACKAGE_INVALID")?;
    let artifacts = &release.manifest.artifacts;
    Ok(
        serde_json::json!({"schemaVersion":1,"directory":release.manifest.package_id,
        "hostSha256":artifacts[0].sha256,"binagotchySha256":artifacts[1].sha256,
        "adapterSha256":artifacts[2].sha256,"wakeBridgeSha256":artifacts[3].sha256}),
    )
}

pub fn same_state_release_qualification_sha256(
    release: &SameStateQualifiedReleaseV3,
) -> Result<String> {
    validate_manifest(&release.manifest).map_err(|_| "WAKE_SELECTION_PACKAGE_INVALID")?;
    let bytes = serde_json::to_vec(release).map_err(|_| "WAKE_SELECTION_ENCODING_INVALID")?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn validate_release(release: &SameStateQualifiedReleaseV3, working: bool, now: u64) -> Result<()> {
    let manifest = same_state_manifest_sha256(&release.manifest)?;
    let local = &release.local_qualification;
    let review = &release.independent_review;
    if !hex(&release.runtime_manifest_sha256, 64)
        || !hex(&release.source_commit, 40)
        || !hex(&local.receipt_sha256, 64)
        || local.source_commit != release.source_commit
        || local.manifest_sha256 != manifest
        || local.verdict != SameStateEvidenceVerdictV3::Pass
        || !hex(&review.receipt_sha256, 64)
        || review.source_commit != release.source_commit
        || review.manifest_sha256 != manifest
        || review.verdict != SameStateEvidenceVerdictV3::Pass
        || !crate::protocol::valid_id(&review.reviewer_identity)
        || review.reviewer_identity.len() > 128
    {
        return Err("WAKE_SELECTION_QUALIFICATION_INVALID".into());
    }
    match release.working_evidence.as_ref() {
        Some(proof)
            if !hex(&proof.receipt_sha256, 64)
                || proof.scope != "WAKE_RUNTIME"
                || proof.source_commit != release.source_commit
                || proof.manifest_sha256 != manifest
                || proof.observed_utc_seconds == 0
                || proof.observed_utc_seconds > now
                || !proof.owner_verified
                || !proof.startup_verified
                || !proof.normal_stop_verified =>
        {
            Err("WAKE_SELECTION_WORKING_EVIDENCE_REQUIRED".into())
        }
        None if working => Err("WAKE_SELECTION_WORKING_EVIDENCE_REQUIRED".into()),
        _ => Ok(()),
    }
}

fn validate_payload(
    payload: &SameStateRootSelectionApprovalV3,
    observed: &SameStateObservedV3,
) -> Result<()> {
    if payload.schema_version != 3
        || !matches!(
            payload.operation.as_str(),
            SAME_STATE_SELECT_OPERATION | SAME_STATE_ROLLBACK_OPERATION
        )
        || !crate::protocol::valid_id(&payload.approval_id)
        || !crate::protocol::valid_id(&payload.reviewer_identity)
        || payload.reviewer_identity.len() > 128
        || payload.reviewer_identity != observed.approved_reviewer_identity
    {
        return Err("WAKE_SELECTION_PURPOSE_INVALID".into());
    }
    if payload.issued_utc_seconds > observed.now_utc_seconds
        || observed.now_utc_seconds > payload.expires_utc_seconds
        || payload.expires_utc_seconds <= payload.issued_utc_seconds
        || payload.expires_utc_seconds - payload.issued_utc_seconds > MAX_AGE_SECONDS
    {
        return Err("WAKE_SELECTION_TIME_INVALID".into());
    }
    if payload.install_generation == 0
        || payload.install_generation != observed.next_install_generation
        || !hex(&payload.policy_sha256, 64)
        || payload.policy_sha256 != observed.policy_sha256
        || !valid_snapshot(&payload.state_snapshot)
        || !valid_snapshot(&observed.state_snapshot)
        || !hex(&payload.planned_selection_sha256, 64)
        || payload.planned_selection_sha256 != observed.planned_selection_sha256
        || payload.planned_selection_sha256 == payload.state_snapshot.current.sha256
    {
        return Err("WAKE_SELECTION_STATE_MISMATCH".into());
    }
    if let Some(resume) = &observed.retained_resume {
        let original_snapshot = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&payload.state_snapshot)
                    .map_err(|_| "WAKE_SELECTION_ENCODING_INVALID")?
            )
        );
        if resume.original_snapshot_sha256 != original_snapshot
            || resume.before_selection_sha256 != payload.state_snapshot.current.sha256
            || resume.original_escrow_sha256 != payload.state_snapshot.current.sha256
            || resume.planned_selection_sha256 != payload.planned_selection_sha256
        {
            return Err("WAKE_SELECTION_STATE_MISMATCH".into());
        }
    }
    if payload.state_snapshot != observed.state_snapshot {
        let resume = observed
            .retained_resume
            .as_ref()
            .ok_or("WAKE_SELECTION_STATE_MISMATCH")?;
        let mut projected = observed.state_snapshot.clone();
        projected.current = payload.state_snapshot.current.clone();
        if !resume.selection_attempt_present
            || projected != payload.state_snapshot
            || observed.state_snapshot.current.sha256 != payload.planned_selection_sha256
        {
            return Err("WAKE_SELECTION_STATE_MISMATCH".into());
        }
    }
    validate_release(&payload.candidate, false, observed.now_utc_seconds)?;
    let previous = payload.rollback_authority.release();
    validate_release(previous, true, observed.now_utc_seconds)?;
    let rollback = is_same_state_rollback_operation(&payload.operation);
    if (!rollback && payload.candidate.manifest.package_id == previous.manifest.package_id)
        || same_state_manifest_sha256(&payload.candidate.manifest)?
            != observed.candidate_manifest_sha256
        || payload.candidate.runtime_manifest_sha256 != observed.candidate_runtime_manifest_sha256
        || same_state_manifest_sha256(&previous.manifest)? != observed.previous_manifest_sha256
        || previous.runtime_manifest_sha256 != observed.previous_runtime_manifest_sha256
    {
        return Err("WAKE_SELECTION_PACKAGE_MISMATCH".into());
    }
    if rollback {
        let original_previous = observed
            .original_previous
            .as_ref()
            .ok_or("WAKE_SELECTION_ROLLBACK_INVALID")?;
        if !matches!(
            &payload.rollback_authority,
            SameStateRollbackAuthorityV3::VerifiedWorkingPrevious { .. }
        ) || observed.legacy_current_package_id.is_some()
            || &payload.candidate != previous
            || original_previous.pointer != same_state_release_pointer_v3(previous)?
            || original_previous.qualification_sha256
                != same_state_release_qualification_sha256(previous)?
        {
            return Err("WAKE_SELECTION_ROLLBACK_INVALID".into());
        }
        return Ok(());
    }
    match &payload.rollback_authority {
        SameStateRollbackAuthorityV3::VerifiedWorkingPrevious { release } => {
            if observed
                .legacy_current_package_id
                .as_ref()
                .is_some_and(|id| id != &release.manifest.package_id)
            {
                return Err("WAKE_SELECTION_PREDECESSOR_MISMATCH".into());
            }
        }
        SameStateRollbackAuthorityV3::ReviewedSourceReconstruction {
            release,
            old_raw_selection_escrow_sha256,
            reconstruction_receipt_sha256,
        } => {
            let old = observed
                .legacy_current_package_id
                .as_ref()
                .ok_or("WAKE_SELECTION_RECONSTRUCTION_INVALID")?;
            if !observed.allow_reviewed_reconstruction
                || release.manifest.package_id == *old
                || payload.candidate.manifest.package_id == *old
                || !hex(reconstruction_receipt_sha256, 64)
                || old_raw_selection_escrow_sha256 != &payload.state_snapshot.current.sha256
            {
                return Err("WAKE_SELECTION_RECONSTRUCTION_INVALID".into());
            }
        }
    }
    Ok(())
}

fn signed_bytes(payload: &SameStateRootSelectionApprovalV3) -> Result<Vec<u8>> {
    let mut bytes = DOMAIN.to_vec();
    bytes.extend(serde_json::to_vec(payload).map_err(|_| "WAKE_SELECTION_ENCODING_INVALID")?);
    Ok(bytes)
}

/// A pure read-only signature proof, deliberately not the policy-owned grant.
/// Production must derive observed values from retained pins and exact ledger
/// admission, including the original acceptance clock for a proven resume.
pub fn verify_same_state_selection_approval_v3(
    envelope_bytes: &[u8],
    key: &VerifyingKey,
    observed: &SameStateObservedV3,
) -> Result<VerifiedSameStateSelectionApprovalV3> {
    if envelope_bytes.is_empty() || envelope_bytes.len() > MAX_ENVELOPE_BYTES {
        return Err("WAKE_SELECTION_ENCODING_INVALID".into());
    }
    let envelope: SignedEnvelopeV3 =
        serde_json::from_slice(envelope_bytes).map_err(|_| "WAKE_SELECTION_ENCODING_INVALID")?;
    if serde_json::to_vec(&envelope).map_err(|_| "WAKE_SELECTION_ENCODING_INVALID")?
        != envelope_bytes
        || !hex(&envelope.signature_hex, 128)
    {
        return Err("WAKE_SELECTION_ENCODING_INVALID".into());
    }
    validate_payload(&envelope.payload, observed)?;
    let mut signature = [0u8; 64];
    for (slot, pair) in signature
        .iter_mut()
        .zip(envelope.signature_hex.as_bytes().chunks_exact(2))
    {
        *slot = u8::from_str_radix(
            std::str::from_utf8(pair).map_err(|_| "WAKE_SELECTION_ENCODING_INVALID")?,
            16,
        )
        .map_err(|_| "WAKE_SELECTION_ENCODING_INVALID")?;
    }
    let signed = signed_bytes(&envelope.payload)?;
    key.verify_strict(&signed, &Signature::from_bytes(&signature))
        .map_err(|_| "WAKE_SELECTION_SIGNATURE_INVALID")?;
    Ok(VerifiedSameStateSelectionApprovalV3 {
        payload: envelope.payload,
        signed_payload_sha256: format!("{:x}", Sha256::digest(signed)),
        signed_envelope_sha256: format!("{:x}", Sha256::digest(envelope_bytes)),
        verification_key_sha256: format!("{:x}", Sha256::digest(key.as_bytes())),
        verified_at_utc_seconds: observed.now_utc_seconds,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuntimeManifestV3 {
    schema_version: u32,
    version: String,
    protocol_version: u32,
    installed_utc: String,
    artifacts: RuntimeArtifactsV3,
    acceptance: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeArtifactsV3 {
    #[serde(rename = "CatDeskWakeHost.exe")]
    host: String,
    #[serde(rename = "CatDeskBinagotchy.exe")]
    gui: String,
    #[serde(rename = "adapter.py")]
    adapter: String,
    #[serde(rename = "wake_bridge.py")]
    bridge: String,
}

/// Verify the exact runtime MAP representation against the signed artifact
/// array. Length checks of actual files remain the retained pin owner's duty.
pub fn validate_same_state_runtime_manifest_v3(
    bytes: &[u8],
    release: &SameStateQualifiedReleaseV3,
) -> Result<()> {
    if bytes.is_empty()
        || bytes.len() > 16 * 1024
        || format!("{:x}", Sha256::digest(bytes)) != release.runtime_manifest_sha256
    {
        return Err("WAKE_SELECTION_RUNTIME_MANIFEST_INVALID".into());
    }
    let manifest: RuntimeManifestV3 =
        serde_json::from_slice(bytes).map_err(|_| "WAKE_SELECTION_RUNTIME_MANIFEST_INVALID")?;
    validate_manifest(&release.manifest).map_err(|_| "WAKE_SELECTION_RUNTIME_MANIFEST_INVALID")?;
    if manifest.schema_version != 1
        || manifest.protocol_version != 1
        || manifest.version != release.manifest.version
        || manifest.installed_utc.is_empty()
        || manifest.installed_utc.len() > 128
        || manifest.acceptance != "DEVELOPMENT_NOT_ACTIVATED"
        || [
            manifest.artifacts.host,
            manifest.artifacts.gui,
            manifest.artifacts.adapter,
            manifest.artifacts.bridge,
        ]
        .iter()
        .zip(&release.manifest.artifacts)
        .any(|(actual, expected)| actual != &expected.sha256)
    {
        return Err("WAKE_SELECTION_RUNTIME_MANIFEST_INVALID".into());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::install_broker_approval::ArtifactV1;
    use ed25519_dalek::{Signer, SigningKey};

    fn runtime_bytes(manifest: &PackageManifestV1) -> Vec<u8> {
        let artifacts = manifest
            .artifacts
            .iter()
            .map(|a| (a.name.clone(), a.sha256.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion":1,"version":manifest.version,"protocolVersion":1,
            "installedUtc":"fixture","artifacts":artifacts,"acceptance":"DEVELOPMENT_NOT_ACTIVATED"
        }))
        .unwrap()
    }

    fn release(version: u32, seed: u8, working: bool) -> SameStateQualifiedReleaseV3 {
        let names = [
            "CatDeskWakeHost.exe",
            "CatDeskBinagotchy.exe",
            "adapter.py",
            "wake_bridge.py",
        ];
        let artifacts = std::array::from_fn(|i| ArtifactV1 {
            name: names[i].into(),
            byte_length: (i as u64 + 1) * 100,
            sha256: format!("{:064x}", u64::from(seed) + i as u64),
        });
        let manifest = PackageManifestV1 {
            schema_version: 1,
            package_id: format!(
                "1.0.0-dev.{version}-{}-{}",
                &artifacts[0].sha256[..12],
                &artifacts[1].sha256[..12]
            ),
            version: format!("1.0.0-dev.{version}"),
            artifacts,
        };
        let digest = same_state_manifest_sha256(&manifest).unwrap();
        let source = format!("{seed:040x}");
        SameStateQualifiedReleaseV3 {
            runtime_manifest_sha256: format!("{:x}", Sha256::digest(runtime_bytes(&manifest))),
            manifest,
            source_commit: source.clone(),
            local_qualification: SameStateLocalQualificationV3 {
                receipt_sha256: "a".repeat(64),
                source_commit: source.clone(),
                manifest_sha256: digest.clone(),
                verdict: SameStateEvidenceVerdictV3::Pass,
            },
            independent_review: SameStateIndependentReviewV3 {
                receipt_sha256: "b".repeat(64),
                source_commit: source.clone(),
                manifest_sha256: digest.clone(),
                reviewer_identity: "independent-source-reviewer".into(),
                verdict: SameStateEvidenceVerdictV3::Pass,
            },
            working_evidence: working.then_some(SameStateWorkingEvidenceV3 {
                scope: "WAKE_RUNTIME".into(),
                receipt_sha256: "c".repeat(64),
                source_commit: source,
                manifest_sha256: digest,
                observed_utc_seconds: 800,
                owner_verified: true,
                startup_verified: true,
                normal_stop_verified: true,
            }),
        }
    }

    pub(crate) fn fixture() -> (
        SameStateRootSelectionApprovalV3,
        SameStateObservedV3,
        SigningKey,
    ) {
        let id = |n: u8| FullFileIdentityV2 {
            volume_serial: 7,
            file_id_hex: format!("{n:032x}"),
        };
        let object = |n| SameStateObjectV3 {
            identity: id(n),
            sha256: format!("{n:064x}"),
        };
        let snapshot = SameStateSnapshotV3 {
            root: id(1),
            current: object(2),
            control: object(3),
            config: object(4),
            desired: "STOPPED".into(),
        };
        let candidate = release(91, 17, false);
        let previous = release(90, 33, true);
        let payload = SameStateRootSelectionApprovalV3 {
            schema_version: 3,
            operation: "SELECT_SAME_STATE_RELEASE".into(),
            approval_id: "selection-approval-fixture".into(),
            issued_utc_seconds: 900,
            expires_utc_seconds: 2000,
            install_generation: 1,
            reviewer_identity: "release-reviewer".into(),
            policy_sha256: "d".repeat(64),
            state_snapshot: snapshot.clone(),
            candidate: candidate.clone(),
            rollback_authority: SameStateRollbackAuthorityV3::VerifiedWorkingPrevious {
                release: previous.clone(),
            },
            planned_selection_sha256: "e".repeat(64),
        };
        let observed = SameStateObservedV3 {
            now_utc_seconds: 1000,
            next_install_generation: 1,
            policy_sha256: payload.policy_sha256.clone(),
            state_snapshot: snapshot,
            candidate_manifest_sha256: same_state_manifest_sha256(&candidate.manifest).unwrap(),
            candidate_runtime_manifest_sha256: candidate.runtime_manifest_sha256,
            previous_manifest_sha256: same_state_manifest_sha256(&previous.manifest).unwrap(),
            previous_runtime_manifest_sha256: previous.runtime_manifest_sha256,
            planned_selection_sha256: payload.planned_selection_sha256.clone(),
            allow_reviewed_reconstruction: false,
            approved_reviewer_identity: payload.reviewer_identity.clone(),
            legacy_current_package_id: None,
            retained_resume: None,
            original_previous: None,
        };
        (payload, observed, SigningKey::from_bytes(&[7; 32]))
    }

    pub(crate) fn envelope(
        payload: &SameStateRootSelectionApprovalV3,
        key: &SigningKey,
    ) -> Vec<u8> {
        let signature = key.sign(&signed_bytes(payload).unwrap()).to_bytes();
        let signature_hex = signature.iter().map(|b| format!("{b:02x}")).collect();
        serde_json::to_vec(&SignedEnvelopeV3 {
            payload: payload.clone(),
            signature_hex,
        })
        .unwrap()
    }

    pub(crate) fn rollback_fixture() -> (
        SameStateRootSelectionApprovalV3,
        SameStateObservedV3,
        SigningKey,
    ) {
        let (mut payload, mut observed, key) = fixture();
        let target = payload.rollback_authority.release().clone();
        payload.operation = SAME_STATE_ROLLBACK_OPERATION.into();
        payload.candidate = target.clone();
        observed.candidate_manifest_sha256 = observed.previous_manifest_sha256.clone();
        observed.candidate_runtime_manifest_sha256 =
            observed.previous_runtime_manifest_sha256.clone();
        observed.original_previous = Some(SameStatePreviousSelectionV3 {
            pointer: same_state_release_pointer_v3(&target).unwrap(),
            qualification_sha256: same_state_release_qualification_sha256(&target).unwrap(),
        });
        (payload, observed, key)
    }

    #[test]
    fn same_state_signature_is_read_only_exact_bounded_and_versioned() {
        let (payload, observed, key) = fixture();
        let before = (payload.clone(), observed.clone());
        let bytes = envelope(&payload, &key);
        assert!(bytes.len() <= MAX_ENVELOPE_BYTES);
        assert_eq!(
            serde_json::to_value(&payload)
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            12
        );
        let proof =
            verify_same_state_selection_approval_v3(&bytes, &key.verifying_key(), &observed)
                .unwrap();
        assert_eq!(proof.payload(), &payload);
        assert_eq!(
            proof.signed_envelope_sha256(),
            format!("{:x}", Sha256::digest(&bytes))
        );
        assert_eq!(
            proof.verification_key_sha256(),
            format!("{:x}", Sha256::digest(key.verifying_key().as_bytes()))
        );
        assert_eq!(proof.verified_at_utc_seconds(), 1000);
        assert_eq!((payload, observed), before);
    }

    #[test]
    fn same_state_wrong_key_domain_and_noncanonical_or_extra_fields_refuse() {
        let (payload, observed, key) = fixture();
        let other = SigningKey::from_bytes(&[9; 32]);
        assert_eq!(
            verify_same_state_selection_approval_v3(
                &envelope(&payload, &key),
                &other.verifying_key(),
                &observed
            )
            .unwrap_err(),
            "WAKE_SELECTION_SIGNATURE_INVALID"
        );
        let wrong_domain = key.sign(&serde_json::to_vec(&payload).unwrap()).to_bytes();
        let bytes = serde_json::to_vec(&SignedEnvelopeV3 {
            payload: payload.clone(),
            signature_hex: wrong_domain.iter().map(|b| format!("{b:02x}")).collect(),
        })
        .unwrap();
        assert_eq!(
            verify_same_state_selection_approval_v3(&bytes, &key.verifying_key(), &observed)
                .unwrap_err(),
            "WAKE_SELECTION_SIGNATURE_INVALID"
        );
        let bytes = envelope(&payload, &key);
        let mut pretty = bytes.clone();
        pretty.push(b'\n');
        assert!(
            verify_same_state_selection_approval_v3(&pretty, &key.verifying_key(), &observed)
                .is_err()
        );
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["callerPath"] = serde_json::json!("private path must not be exported");
        assert_eq!(
            verify_same_state_selection_approval_v3(
                &serde_json::to_vec(&value).unwrap(),
                &key.verifying_key(),
                &observed
            )
            .unwrap_err(),
            "WAKE_SELECTION_ENCODING_INVALID"
        );
        assert!(
            verify_same_state_selection_approval_v3(
                &vec![b'x'; MAX_ENVELOPE_BYTES + 1],
                &key.verifying_key(),
                &observed
            )
            .is_err()
        );
    }

    #[test]
    fn same_state_time_generation_policy_reviewer_and_physical_snapshot_are_exact() {
        let (payload, observed, key) = fixture();
        let mut cases = Vec::new();
        let mut bad = payload.clone();
        bad.expires_utc_seconds = 999;
        cases.push(bad);
        let mut bad = payload.clone();
        bad.issued_utc_seconds = 1001;
        cases.push(bad);
        let mut bad = payload.clone();
        bad.expires_utc_seconds = 900 + MAX_AGE_SECONDS + 1;
        cases.push(bad);
        let mut bad = payload.clone();
        bad.install_generation = 2;
        cases.push(bad);
        let mut bad = payload.clone();
        bad.policy_sha256 = "f".repeat(64);
        cases.push(bad);
        let mut bad = payload.clone();
        bad.reviewer_identity = "unapproved-reviewer".into();
        cases.push(bad);
        let mut bad = payload.clone();
        bad.state_snapshot.current.identity.file_id_hex = "a".repeat(32);
        cases.push(bad);
        let mut bad = payload.clone();
        bad.state_snapshot.current.sha256 = "a".repeat(64);
        cases.push(bad);
        let mut bad = payload.clone();
        bad.state_snapshot.control = bad.state_snapshot.current.clone();
        cases.push(bad);
        let mut bad = payload.clone();
        bad.state_snapshot.desired = "RUNNING".into();
        cases.push(bad);
        let mut bad = payload.clone();
        bad.planned_selection_sha256 = "f".repeat(64);
        cases.push(bad);
        for bad in cases {
            assert!(
                verify_same_state_selection_approval_v3(
                    &envelope(&bad, &key),
                    &key.verifying_key(),
                    &observed
                )
                .is_err()
            );
        }
    }

    #[test]
    fn same_state_local_and_independent_source_evidence_cannot_be_ci_or_inconclusive() {
        let (payload, observed, key) = fixture();
        for verdict in [
            SameStateEvidenceVerdictV3::Fail,
            SameStateEvidenceVerdictV3::Inconclusive,
        ] {
            let mut bad = payload.clone();
            bad.candidate.independent_review.verdict = verdict;
            assert_eq!(
                verify_same_state_selection_approval_v3(
                    &envelope(&bad, &key),
                    &key.verifying_key(),
                    &observed
                )
                .unwrap_err(),
                "WAKE_SELECTION_QUALIFICATION_INVALID"
            );
        }
        let mut bad = payload.clone();
        bad.candidate.local_qualification.source_commit = "f".repeat(40);
        assert!(
            verify_same_state_selection_approval_v3(
                &envelope(&bad, &key),
                &key.verifying_key(),
                &observed
            )
            .is_err()
        );
        let mut bad = payload.clone();
        bad.candidate.independent_review.manifest_sha256 = "f".repeat(64);
        assert!(
            verify_same_state_selection_approval_v3(
                &envelope(&bad, &key),
                &key.verifying_key(),
                &observed
            )
            .is_err()
        );
        let mut value = serde_json::to_value(SignedEnvelopeV3 {
            payload,
            signature_hex: "a".repeat(128),
        })
        .unwrap();
        value["payload"]["ci"] = serde_json::json!({"runId":123});
        assert_eq!(
            verify_same_state_selection_approval_v3(
                &serde_json::to_vec(&value).unwrap(),
                &key.verifying_key(),
                &observed
            )
            .unwrap_err(),
            "WAKE_SELECTION_ENCODING_INVALID"
        );
    }

    #[test]
    fn same_state_previous_needs_real_bound_working_evidence() {
        let (payload, observed, key) = fixture();
        for variant in 0..6 {
            let mut bad = payload.clone();
            let SameStateRollbackAuthorityV3::VerifiedWorkingPrevious { release } =
                &mut bad.rollback_authority
            else {
                unreachable!()
            };
            match variant {
                0 => release.working_evidence = None,
                1 => release.working_evidence.as_mut().unwrap().owner_verified = false,
                2 => {
                    release
                        .working_evidence
                        .as_mut()
                        .unwrap()
                        .normal_stop_verified = false
                }
                3 => release.working_evidence.as_mut().unwrap().manifest_sha256 = "f".repeat(64),
                4 => {
                    release
                        .working_evidence
                        .as_mut()
                        .unwrap()
                        .observed_utc_seconds = 1001
                }
                _ => {
                    release.working_evidence.as_mut().unwrap().scope = "DEVELOPMENT_BACKEND".into()
                }
            }
            assert_eq!(
                verify_same_state_selection_approval_v3(
                    &envelope(&bad, &key),
                    &key.verifying_key(),
                    &observed
                )
                .unwrap_err(),
                "WAKE_SELECTION_WORKING_EVIDENCE_REQUIRED"
            );
        }
    }

    #[test]
    fn same_state_reconstruction_is_default_off_escrowed_and_not_legacy_health() {
        let (mut payload, mut observed, key) = fixture();
        let previous = payload.rollback_authority.release().clone();
        payload.rollback_authority = SameStateRollbackAuthorityV3::ReviewedSourceReconstruction {
            release: previous,
            old_raw_selection_escrow_sha256: payload.state_snapshot.current.sha256.clone(),
            reconstruction_receipt_sha256: "f".repeat(64),
        };
        observed.legacy_current_package_id = Some("unqualified-dev84".into());
        assert_eq!(
            verify_same_state_selection_approval_v3(
                &envelope(&payload, &key),
                &key.verifying_key(),
                &observed
            )
            .unwrap_err(),
            "WAKE_SELECTION_RECONSTRUCTION_INVALID"
        );
        observed.allow_reviewed_reconstruction = true;
        verify_same_state_selection_approval_v3(
            &envelope(&payload, &key),
            &key.verifying_key(),
            &observed,
        )
        .unwrap();
        let mut after_v2 = observed.clone();
        after_v2.legacy_current_package_id = None;
        assert!(
            verify_same_state_selection_approval_v3(
                &envelope(&payload, &key),
                &key.verifying_key(),
                &after_v2
            )
            .is_err()
        );
        let mut fake_legacy = observed.clone();
        fake_legacy.legacy_current_package_id = Some(
            payload
                .rollback_authority
                .release()
                .manifest
                .package_id
                .clone(),
        );
        assert!(
            verify_same_state_selection_approval_v3(
                &envelope(&payload, &key),
                &key.verifying_key(),
                &fake_legacy
            )
            .is_err()
        );
        let SameStateRollbackAuthorityV3::ReviewedSourceReconstruction {
            old_raw_selection_escrow_sha256,
            ..
        } = &mut payload.rollback_authority
        else {
            unreachable!()
        };
        *old_raw_selection_escrow_sha256 = "a".repeat(64);
        assert!(
            verify_same_state_selection_approval_v3(
                &envelope(&payload, &key),
                &key.verifying_key(),
                &observed
            )
            .is_err()
        );
    }

    #[test]
    fn same_state_runtime_map_manifest_is_exact_and_distinct_from_array_manifest() {
        let (payload, _, _) = fixture();
        let release = &payload.candidate;
        let bytes = runtime_bytes(&release.manifest);
        validate_same_state_runtime_manifest_v3(&bytes, release).unwrap();
        assert!(
            validate_same_state_runtime_manifest_v3(
                &serde_json::to_vec(&release.manifest).unwrap(),
                release
            )
            .is_err()
        );
        let mut changed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        changed["artifacts"]["adapter.py"] = serde_json::json!("f".repeat(64));
        let changed = serde_json::to_vec(&changed).unwrap();
        let mut declared = release.clone();
        declared.runtime_manifest_sha256 = format!("{:x}", Sha256::digest(&changed));
        assert!(validate_same_state_runtime_manifest_v3(&changed, &declared).is_err());
        let text = String::from_utf8(bytes).unwrap();
        let duplicate = text
            .replacen(
                "\"schemaVersion\":1",
                "\"schemaVersion\":1,\"schemaVersion\":1",
                1,
            )
            .into_bytes();
        declared.runtime_manifest_sha256 = format!("{:x}", Sha256::digest(&duplicate));
        assert!(validate_same_state_runtime_manifest_v3(&duplicate, &declared).is_err());
    }

    #[test]
    fn same_state_post_pointer_resume_retains_actual_current_and_requires_exact_ledger_evidence() {
        let (payload, mut observed, key) = fixture();
        observed.state_snapshot.current.identity.file_id_hex = "a".repeat(32);
        observed.state_snapshot.current.sha256 = payload.planned_selection_sha256.clone();
        let actual_current = observed.state_snapshot.current.clone();
        let resume = SameStateRetainedResumeV3 {
            original_snapshot_sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&payload.state_snapshot).unwrap())
            ),
            before_selection_sha256: payload.state_snapshot.current.sha256.clone(),
            planned_selection_sha256: payload.planned_selection_sha256.clone(),
            original_escrow_sha256: payload.state_snapshot.current.sha256.clone(),
            selection_attempt_present: true,
        };
        assert!(
            verify_same_state_selection_approval_v3(
                &envelope(&payload, &key),
                &key.verifying_key(),
                &observed
            )
            .is_err()
        );
        observed.retained_resume = Some(resume);
        verify_same_state_selection_approval_v3(
            &envelope(&payload, &key),
            &key.verifying_key(),
            &observed,
        )
        .unwrap();
        assert_eq!(
            observed.state_snapshot.current, actual_current,
            "never replace actual physical current with signed before claims"
        );
        for variant in 0..7 {
            let mut bad = observed.clone();
            match variant {
                0 => {
                    bad.retained_resume
                        .as_mut()
                        .unwrap()
                        .selection_attempt_present = false
                }
                1 => {
                    bad.retained_resume
                        .as_mut()
                        .unwrap()
                        .original_snapshot_sha256 = "f".repeat(64)
                }
                2 => bad.retained_resume.as_mut().unwrap().original_escrow_sha256 = "f".repeat(64),
                3 => {
                    bad.retained_resume
                        .as_mut()
                        .unwrap()
                        .before_selection_sha256 = "f".repeat(64)
                }
                4 => bad.state_snapshot.config.sha256 = "f".repeat(64),
                5 => bad.state_snapshot.control.identity.file_id_hex = "f".repeat(32),
                _ => bad.state_snapshot.current.sha256 = "f".repeat(64),
            }
            assert!(
                verify_same_state_selection_approval_v3(
                    &envelope(&payload, &key),
                    &key.verifying_key(),
                    &bad
                )
                .is_err()
            );
        }
    }

    #[test]
    fn same_state_rollback_selects_exact_qualified_previous_without_current_artifact_health() {
        let (payload, observed, key) = rollback_fixture();
        assert_eq!(payload.candidate, *payload.rollback_authority.release());
        assert!(observed.legacy_current_package_id.is_none());
        assert_ne!(
            payload.state_snapshot.current.sha256,
            payload.candidate.manifest.artifacts[0].sha256
        );
        // The before image is selection metadata. No failed current binary,
        // manifest or working-health assertion is supplied to this verifier.
        let proof = verify_same_state_selection_approval_v3(
            &envelope(&payload, &key),
            &key.verifying_key(),
            &observed,
        )
        .unwrap();
        assert_eq!(proof.payload().operation, SAME_STATE_ROLLBACK_OPERATION);
        assert_eq!(
            serde_json::to_value(&payload)
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            12
        );
    }

    #[test]
    fn same_state_rollback_refuses_missing_changed_legacy_or_reconstructed_predecessor() {
        let (payload, observed, key) = rollback_fixture();
        for variant in 0..7 {
            let mut bad = payload.clone();
            let mut physical = observed.clone();
            match variant {
                0 => physical.original_previous = None,
                1 => {
                    physical.original_previous.as_mut().unwrap().pointer["hostSha256"] =
                        "f".repeat(64).into()
                }
                2 => {
                    physical
                        .original_previous
                        .as_mut()
                        .unwrap()
                        .qualification_sha256 = "f".repeat(64)
                }
                3 => physical.legacy_current_package_id = Some("unqualified-dev84".into()),
                4 => bad.candidate.working_evidence = None,
                5 => {
                    bad.rollback_authority =
                        SameStateRollbackAuthorityV3::ReviewedSourceReconstruction {
                            release: bad.candidate.clone(),
                            old_raw_selection_escrow_sha256: bad
                                .state_snapshot
                                .current
                                .sha256
                                .clone(),
                            reconstruction_receipt_sha256: "f".repeat(64),
                        }
                }
                _ => bad.operation = "ROLLBACK".into(),
            }
            assert!(
                verify_same_state_selection_approval_v3(
                    &envelope(&bad, &key),
                    &key.verifying_key(),
                    &physical
                )
                .is_err()
            );
        }
    }

    #[test]
    fn same_state_rollback_requires_full_review_and_wake_runtime_and_keeps_select_distinct() {
        let (payload, observed, key) = rollback_fixture();
        for variant in 0..3 {
            let mut bad = payload.clone();
            let mut target = bad.candidate.clone();
            match variant {
                0 => target.working_evidence.as_mut().unwrap().scope = "DEVELOPMENT_BACKEND".into(),
                1 => target.independent_review.verdict = SameStateEvidenceVerdictV3::Inconclusive,
                _ => target.working_evidence.as_mut().unwrap().owner_verified = false,
            }
            bad.candidate = target.clone();
            bad.rollback_authority =
                SameStateRollbackAuthorityV3::VerifiedWorkingPrevious { release: target };
            assert!(
                verify_same_state_selection_approval_v3(
                    &envelope(&bad, &key),
                    &key.verifying_key(),
                    &observed
                )
                .is_err()
            );
        }
        let mut selection = payload;
        selection.operation = SAME_STATE_SELECT_OPERATION.into();
        assert_eq!(
            verify_same_state_selection_approval_v3(
                &envelope(&selection, &key),
                &key.verifying_key(),
                &observed
            )
            .unwrap_err(),
            "WAKE_SELECTION_PACKAGE_MISMATCH"
        );
    }

    #[test]
    fn same_state_rollback_resume_uses_escrowed_original_previous_and_exact_attempt() {
        let (payload, mut observed, key) = rollback_fixture();
        observed.state_snapshot.current.identity.file_id_hex = "a".repeat(32);
        observed.state_snapshot.current.sha256 = payload.planned_selection_sha256.clone();
        observed.retained_resume = Some(SameStateRetainedResumeV3 {
            original_snapshot_sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&payload.state_snapshot).unwrap())
            ),
            before_selection_sha256: payload.state_snapshot.current.sha256.clone(),
            planned_selection_sha256: payload.planned_selection_sha256.clone(),
            original_escrow_sha256: payload.state_snapshot.current.sha256.clone(),
            selection_attempt_present: true,
        });
        verify_same_state_selection_approval_v3(
            &envelope(&payload, &key),
            &key.verifying_key(),
            &observed,
        )
        .unwrap();
        observed.original_previous = None;
        assert!(
            verify_same_state_selection_approval_v3(
                &envelope(&payload, &key),
                &key.verifying_key(),
                &observed
            )
            .is_err()
        );
    }
}
