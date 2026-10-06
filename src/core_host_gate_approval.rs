//! T-0311 Option-A request-bound approval primitive.
//!
//! This module has no capture, MCP, filesystem, or evaluator integration.
//! T-0312 may consume only receipts that passed these pure typed checks.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::delegated::autonomy_supervisor::{
    CoreHostGateReviewAuthorityV1, resolve_core_host_gate_review_authority,
};

pub(crate) const CORE_HOST_GATE_APPROVAL_SCHEMA_VERSION: u32 = 1;
pub(crate) const CORE_HOST_GATE_APPROVAL_PRODUCT: &str = "CatDesk";
pub(crate) const CORE_HOST_GATE_APPROVAL_PROJECT: &str = "catdesk";
pub(crate) const CORE_HOST_GATE_APPROVAL_PURPOSE: &str = "core-host-gate-approval-v1";
/// The only reviewed-completion artifact capable of binding a host request.
pub(crate) const CORE_HOST_GATE_APPROVAL_REVIEW_ARTIFACT_ID: &str =
    "src/core-host-gate-approval-request-v1.json";

const MAX_IDENTITY_BYTES: usize = 128;
const MAX_ARTIFACT_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum CoreHostGateV1 {
    T0223,
    T0222,
    T0152,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum CoreHostGateApprovalVerdictV1 {
    Approve,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CoreHostGateRuntimeIdentityV1 {
    pub(crate) build_identity: String,
    pub(crate) generation: u64,
    pub(crate) host_session_id: String,
}

/// This is product-derived by the future fixed observation/capture seam. No
/// MCP/CLI surface accepts it, and a receipt cannot be issued without the
/// exact independently reviewed artifact described below.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CoreHostGateApprovalRequestV1 {
    pub(crate) schema_version: u32,
    pub(crate) product: String,
    pub(crate) project_id: String,
    pub(crate) purpose: String,
    pub(crate) gate: CoreHostGateV1,
    pub(crate) observation_id: String,
    pub(crate) observation_sha256: String,
    pub(crate) t0224_session_id: String,
    pub(crate) t0224_review_record_id: String,
    pub(crate) current_target_sha256: String,
    pub(crate) runtime: CoreHostGateRuntimeIdentityV1,
    pub(crate) issued_at_unix: u64,
    pub(crate) revision: u64,
}

/// Immutable completion artifact. Its literal path is compiled above and it
/// must be listed in the reviewed completion's exact artifact set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CoreHostGateApprovalReviewArtifactV1 {
    pub(crate) schema_version: u32,
    pub(crate) product: String,
    pub(crate) project_id: String,
    pub(crate) purpose: String,
    pub(crate) request: CoreHostGateApprovalRequestV1,
    pub(crate) request_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CoreHostGateApprovalV1 {
    #[serde(flatten)]
    pub(crate) request: CoreHostGateApprovalRequestV1,
    pub(crate) request_sha256: String,
    pub(crate) review_record_id: String,
    pub(crate) review_session_id: String,
    pub(crate) review_contract_hash: String,
    pub(crate) review_completion_sha256: String,
    pub(crate) review_contract_remeasurement_sha256: String,
    pub(crate) reviewer_authority_sha256: String,
    pub(crate) verdict: CoreHostGateApprovalVerdictV1,
}

/// Canonical bytes are the deterministic serde encoding of this fixed struct;
/// there are no maps, optional fields, aliases, or caller-specified fields.
pub(crate) fn canonical_request_bytes(
    request: &CoreHostGateApprovalRequestV1,
) -> Result<Vec<u8>, String> {
    validate_request(request)?;
    serde_json::to_vec(request).map_err(|_| "core host gate request is unavailable".into())
}

pub(crate) fn canonical_request_sha256(
    request: &CoreHostGateApprovalRequestV1,
) -> Result<String, String> {
    let mut digest = Sha256::new();
    digest.update(canonical_request_bytes(request)?);
    Ok(format!("{:x}", digest.finalize()))
}

pub(crate) fn review_artifact_for_request(
    request: CoreHostGateApprovalRequestV1,
) -> Result<CoreHostGateApprovalReviewArtifactV1, String> {
    let request_sha256 = canonical_request_sha256(&request)?;
    Ok(CoreHostGateApprovalReviewArtifactV1 {
        schema_version: CORE_HOST_GATE_APPROVAL_SCHEMA_VERSION,
        product: CORE_HOST_GATE_APPROVAL_PRODUCT.into(),
        project_id: CORE_HOST_GATE_APPROVAL_PROJECT.into(),
        purpose: CORE_HOST_GATE_APPROVAL_PURPOSE.into(),
        request,
        request_sha256,
    })
}

pub(crate) fn canonical_review_artifact_bytes(
    artifact: &CoreHostGateApprovalReviewArtifactV1,
) -> Result<Vec<u8>, String> {
    validate_review_artifact(artifact)?;
    serde_json::to_vec(artifact).map_err(|_| "core host gate review artifact is unavailable".into())
}

pub(crate) fn parse_review_artifact(
    bytes: &[u8],
) -> Result<CoreHostGateApprovalReviewArtifactV1, String> {
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err("core host gate review artifact is invalid".into());
    }
    let artifact: CoreHostGateApprovalReviewArtifactV1 = serde_json::from_slice(bytes)
        .map_err(|_| "core host gate review artifact is invalid".to_string())?;
    if canonical_review_artifact_bytes(&artifact)? != bytes {
        return Err("core host gate review artifact is noncanonical".into());
    }
    Ok(artifact)
}

pub(crate) fn review_artifact_binds_request(
    artifact: &CoreHostGateApprovalReviewArtifactV1,
    request: &CoreHostGateApprovalRequestV1,
) -> Result<(), String> {
    validate_review_artifact(artifact)?;
    if artifact.request != *request || artifact.request_sha256 != canonical_request_sha256(request)?
    {
        return Err("core host gate review artifact does not bind request".into());
    }
    Ok(())
}

/// Revalidates the acknowledged completion and requires its exact typed
/// request artifact before returning a purpose-bound authority identity.
pub(crate) fn derive_core_host_gate_review_authority(
    workspace: &Path,
    record_id: &str,
    request: &CoreHostGateApprovalRequestV1,
) -> Result<CoreHostGateReviewAuthorityV1, String> {
    validate_request(request)?;
    let authority = resolve_core_host_gate_review_authority(workspace, record_id, request)
        .map_err(|_| "core host gate review authority is unavailable".to_string())?;
    validate_authority(&authority, request)?;
    Ok(authority)
}

pub(crate) fn issue_core_host_gate_approval(
    request: CoreHostGateApprovalRequestV1,
    authority: &CoreHostGateReviewAuthorityV1,
) -> Result<CoreHostGateApprovalV1, String> {
    validate_authority(authority, &request)?;
    let request_sha256 = canonical_request_sha256(&request)?;
    Ok(CoreHostGateApprovalV1 {
        request,
        request_sha256,
        review_record_id: authority.review_record_id.clone(),
        review_session_id: authority.review_session_id.clone(),
        review_contract_hash: authority.review_contract_hash.clone(),
        review_completion_sha256: authority.review_completion_sha256.clone(),
        review_contract_remeasurement_sha256: authority.remeasurement_digest.clone(),
        reviewer_authority_sha256: authority.authority_sha256.clone(),
        verdict: CoreHostGateApprovalVerdictV1::Approve,
    })
}

/// Pure receipt validation only. There is intentionally no persistence API in
/// T-0311-R1; T-0312 owns fixed protected ProgramData ledger integration.
pub(crate) fn validate_core_host_gate_approval(
    receipt: &CoreHostGateApprovalV1,
    expected: &CoreHostGateApprovalRequestV1,
    authority: &CoreHostGateReviewAuthorityV1,
) -> Result<(), String> {
    validate_receipt_shape(receipt)?;
    validate_authority(authority, expected)?;
    if receipt.request != *expected
        || receipt.request_sha256 != canonical_request_sha256(expected)?
        || receipt.review_record_id != authority.review_record_id
        || receipt.review_session_id != authority.review_session_id
        || receipt.review_contract_hash != authority.review_contract_hash
        || receipt.review_completion_sha256 != authority.review_completion_sha256
        || receipt.review_contract_remeasurement_sha256 != authority.remeasurement_digest
        || receipt.reviewer_authority_sha256 != authority.authority_sha256
        || receipt.verdict != CoreHostGateApprovalVerdictV1::Approve
    {
        return Err("core host gate approval binding is invalid".into());
    }
    Ok(())
}

fn validate_review_artifact(artifact: &CoreHostGateApprovalReviewArtifactV1) -> Result<(), String> {
    if artifact.schema_version != CORE_HOST_GATE_APPROVAL_SCHEMA_VERSION
        || artifact.product != CORE_HOST_GATE_APPROVAL_PRODUCT
        || artifact.project_id != CORE_HOST_GATE_APPROVAL_PROJECT
        || artifact.purpose != CORE_HOST_GATE_APPROVAL_PURPOSE
        || artifact.request_sha256 != canonical_request_sha256(&artifact.request)?
    {
        return Err("core host gate review artifact is invalid".into());
    }
    Ok(())
}

fn validate_receipt_shape(receipt: &CoreHostGateApprovalV1) -> Result<(), String> {
    validate_request(&receipt.request)?;
    if receipt.request_sha256 != canonical_request_sha256(&receipt.request)?
        || receipt.verdict != CoreHostGateApprovalVerdictV1::Approve
        || !valid_identity(&receipt.review_record_id)
        || !valid_identity(&receipt.review_session_id)
        || !valid_contract_hash(&receipt.review_contract_hash)
        || !valid_sha256(&receipt.review_completion_sha256)
        || !valid_sha256(&receipt.review_contract_remeasurement_sha256)
        || !valid_sha256(&receipt.reviewer_authority_sha256)
    {
        return Err("core host gate approval is invalid".into());
    }
    Ok(())
}

pub(crate) fn validate_request(request: &CoreHostGateApprovalRequestV1) -> Result<(), String> {
    if request.schema_version != CORE_HOST_GATE_APPROVAL_SCHEMA_VERSION
        || request.product != CORE_HOST_GATE_APPROVAL_PRODUCT
        || request.project_id != CORE_HOST_GATE_APPROVAL_PROJECT
        || request.purpose != CORE_HOST_GATE_APPROVAL_PURPOSE
        || !valid_identity(&request.observation_id)
        || !valid_sha256(&request.observation_sha256)
        || !valid_identity(&request.t0224_session_id)
        || !valid_identity(&request.t0224_review_record_id)
        || !valid_sha256(&request.current_target_sha256)
        || !valid_identity(&request.runtime.build_identity)
        || request.runtime.generation == 0
        || !valid_identity(&request.runtime.host_session_id)
        || request.issued_at_unix == 0
        || request.revision == 0
    {
        return Err("core host gate approval request is invalid".into());
    }
    Ok(())
}

fn validate_authority(
    authority: &CoreHostGateReviewAuthorityV1,
    request: &CoreHostGateApprovalRequestV1,
) -> Result<(), String> {
    if authority.schema_version != CORE_HOST_GATE_APPROVAL_SCHEMA_VERSION
        || authority.product != CORE_HOST_GATE_APPROVAL_PRODUCT
        || authority.project_id != CORE_HOST_GATE_APPROVAL_PROJECT
        || authority.purpose != CORE_HOST_GATE_APPROVAL_PURPOSE
        || authority.request_sha256 != canonical_request_sha256(request)?
        || !valid_identity(&authority.review_record_id)
        || !valid_identity(&authority.review_session_id)
        || !valid_contract_hash(&authority.review_contract_hash)
        || !valid_sha256(&authority.review_completion_sha256)
        || !valid_sha256(&authority.remeasurement_digest)
        || !valid_sha256(&authority.authority_sha256)
    {
        return Err("core host gate review authority is invalid".into());
    }
    Ok(())
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTITY_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_contract_hash(value: &str) -> bool {
    value.len() == "fnv1a64:".len() + 16
        && value.starts_with("fnv1a64:")
        && value["fnv1a64:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(letter: char) -> String {
        letter.to_string().repeat(64)
    }

    fn request(gate: CoreHostGateV1, revision: u64) -> CoreHostGateApprovalRequestV1 {
        CoreHostGateApprovalRequestV1 {
            schema_version: 1,
            product: "CatDesk".into(),
            project_id: "catdesk".into(),
            purpose: CORE_HOST_GATE_APPROVAL_PURPOSE.into(),
            gate,
            observation_id: "observation-1".into(),
            observation_sha256: sha('a'),
            t0224_session_id: "wake-session".into(),
            t0224_review_record_id: "wake-record".into(),
            current_target_sha256: sha('b'),
            runtime: CoreHostGateRuntimeIdentityV1 {
                build_identity: "build-1".into(),
                generation: 1,
                host_session_id: "host-session-1".into(),
            },
            issued_at_unix: 1,
            revision,
        }
    }

    fn authority(request: &CoreHostGateApprovalRequestV1) -> CoreHostGateReviewAuthorityV1 {
        CoreHostGateReviewAuthorityV1 {
            schema_version: 1,
            product: "CatDesk".into(),
            project_id: "catdesk".into(),
            purpose: CORE_HOST_GATE_APPROVAL_PURPOSE.into(),
            request_sha256: canonical_request_sha256(request).unwrap(),
            review_record_id: "review-record".into(),
            review_session_id: "review-session".into(),
            review_contract_hash: "fnv1a64:0123456789abcdef".into(),
            review_completion_sha256: sha('c'),
            remeasurement_digest: sha('d'),
            authority_sha256: sha('e'),
        }
    }

    #[test]
    fn exact_typed_artifact_binds_the_canonical_request() {
        let request = request(CoreHostGateV1::T0223, 1);
        let artifact = review_artifact_for_request(request.clone()).unwrap();
        let bytes = canonical_review_artifact_bytes(&artifact).unwrap();
        assert_eq!(parse_review_artifact(&bytes).unwrap(), artifact);
        review_artifact_binds_request(&artifact, &request).unwrap();
        let authority = authority(&request);
        let receipt = issue_core_host_gate_approval(request.clone(), &authority).unwrap();
        validate_core_host_gate_approval(&receipt, &request, &authority).unwrap();
    }

    #[test]
    fn changed_request_or_authority_digest_cannot_issue_or_validate() {
        let request = request(CoreHostGateV1::T0223, 1);
        let authority = authority(&request);
        let receipt = issue_core_host_gate_approval(request.clone(), &authority).unwrap();
        for mutate in [
            |r: &mut CoreHostGateApprovalRequestV1| r.observation_sha256 = sha('f'),
            |r: &mut CoreHostGateApprovalRequestV1| r.t0224_session_id = "other-session".into(),
            |r: &mut CoreHostGateApprovalRequestV1| {
                r.t0224_review_record_id = "other-record".into()
            },
            |r: &mut CoreHostGateApprovalRequestV1| r.current_target_sha256 = sha('f'),
            |r: &mut CoreHostGateApprovalRequestV1| r.runtime.build_identity = "build-2".into(),
            |r: &mut CoreHostGateApprovalRequestV1| r.runtime.generation = 2,
            |r: &mut CoreHostGateApprovalRequestV1| r.revision = 2,
        ] {
            let mut changed = request.clone();
            mutate(&mut changed);
            assert!(issue_core_host_gate_approval(changed.clone(), &authority).is_err());
            assert!(validate_core_host_gate_approval(&receipt, &changed, &authority).is_err());
        }
        let mut changed = request.clone();
        changed.gate = CoreHostGateV1::T0222;
        assert!(validate_core_host_gate_approval(&receipt, &changed, &authority).is_err());
    }

    #[test]
    fn wrong_domain_ack_only_and_noncanonical_artifacts_fail_closed() {
        let request = request(CoreHostGateV1::T0223, 1);
        let mut authority = authority(&request);
        authority.request_sha256 = sha('f');
        assert!(issue_core_host_gate_approval(request.clone(), &authority).is_err());
        let mut artifact = review_artifact_for_request(request.clone()).unwrap();
        artifact.purpose = "independent_final_review".into();
        assert!(review_artifact_binds_request(&artifact, &request).is_err());
        assert!(parse_review_artifact(br#"{\"schemaVersion\":1,\"product\":\"CatDesk\",\"projectId\":\"catdesk\",\"purpose\":\"core-host-gate-approval-v1\",\"gate\":\"ALL\"}"#).is_err());
    }
}
