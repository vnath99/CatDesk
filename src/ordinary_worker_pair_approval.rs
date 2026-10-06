//! Purpose-separated reviewed authority for one ordinary-worker predecessor/current pair.
//!
//! This module has no artifact writer, protected-storage, release-store, or runtime
//! integration. It only derives a typed authority after an exact immutable reviewed
//! completion artifact binds the canonical pair request.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::delegated::autonomy_supervisor::{
    OrdinaryWorkerPairReviewAuthorityV1, resolve_ordinary_worker_pair_review_authority,
};

pub(crate) const ORDINARY_WORKER_PAIR_APPROVAL_SCHEMA_VERSION: u32 = 1;
pub(crate) const ORDINARY_WORKER_PAIR_APPROVAL_PRODUCT: &str = "CatDesk";
pub(crate) const ORDINARY_WORKER_PAIR_APPROVAL_PROJECT: &str = "catdesk";
pub(crate) const ORDINARY_WORKER_PAIR_APPROVAL_PURPOSE: &str =
    "catdesk-ordinary-worker-pair-approval-v1";
pub(crate) const ORDINARY_WORKER_PAIR_APPROVAL_ROLE: &str = "catdesk-ordinary-worker-v1";
/// The only immutable completion artifact permitted to bind this authority.
pub(crate) const ORDINARY_WORKER_PAIR_APPROVAL_REVIEW_ARTIFACT_ID: &str =
    "src/ordinary-worker-pair-approval-v1.json";

const MAX_ARTIFACT_BYTES: usize = 16 * 1024;
const MAX_IDENTITY_BYTES: usize = 128;
const MAX_IMAGE_BYTES: u64 = 512 * 1024 * 1024;
const PREDECESSOR_ARTIFACT_ID: &str = "ordinary-worker-predecessor-v1";
const CURRENT_ARTIFACT_ID: &str = "ordinary-worker-current-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OrdinaryWorkerPairDescriptorV1 {
    pub(crate) artifact_id: String,
    pub(crate) generation: u64,
    pub(crate) image_sha256: String,
    pub(crate) image_length: u64,
    pub(crate) source_snapshot_id: String,
    pub(crate) review_session_id: String,
    pub(crate) review_record_id: String,
    pub(crate) review_authority_sha256: String,
    pub(crate) review_contract_hash: String,
    pub(crate) review_completion_sha256: String,
    pub(crate) review_remeasurement_digest: String,
    pub(crate) attestation_id: String,
    pub(crate) attestation_sha256: String,
    pub(crate) attestation_candidate_sha256: String,
    pub(crate) attestation_candidate_length: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OrdinaryWorkerPairApprovalRequestV1 {
    pub(crate) schema_version: u32,
    pub(crate) product: String,
    pub(crate) project_id: String,
    pub(crate) purpose: String,
    pub(crate) role: String,
    pub(crate) predecessor: OrdinaryWorkerPairDescriptorV1,
    pub(crate) current: OrdinaryWorkerPairDescriptorV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OrdinaryWorkerPairApprovalReviewArtifactV1 {
    pub(crate) schema_version: u32,
    pub(crate) product: String,
    pub(crate) project_id: String,
    pub(crate) purpose: String,
    pub(crate) request: OrdinaryWorkerPairApprovalRequestV1,
    pub(crate) request_sha256: String,
}

pub(crate) fn canonical_pair_request_bytes(
    request: &OrdinaryWorkerPairApprovalRequestV1,
) -> Result<Vec<u8>, String> {
    validate_pair_request(request)?;
    serde_json::to_vec(request).map_err(|_| "ordinary worker pair request is unavailable".into())
}

pub(crate) fn canonical_pair_request_sha256(
    request: &OrdinaryWorkerPairApprovalRequestV1,
) -> Result<String, String> {
    Ok(digest(&canonical_pair_request_bytes(request)?))
}

pub(crate) fn review_artifact_for_pair_request(
    request: OrdinaryWorkerPairApprovalRequestV1,
) -> Result<OrdinaryWorkerPairApprovalReviewArtifactV1, String> {
    let request_sha256 = canonical_pair_request_sha256(&request)?;
    Ok(OrdinaryWorkerPairApprovalReviewArtifactV1 {
        schema_version: ORDINARY_WORKER_PAIR_APPROVAL_SCHEMA_VERSION,
        product: ORDINARY_WORKER_PAIR_APPROVAL_PRODUCT.into(),
        project_id: ORDINARY_WORKER_PAIR_APPROVAL_PROJECT.into(),
        purpose: ORDINARY_WORKER_PAIR_APPROVAL_PURPOSE.into(),
        request,
        request_sha256,
    })
}

pub(crate) fn canonical_review_artifact_bytes(
    artifact: &OrdinaryWorkerPairApprovalReviewArtifactV1,
) -> Result<Vec<u8>, String> {
    validate_review_artifact(artifact)?;
    serde_json::to_vec(artifact)
        .map_err(|_| "ordinary worker pair review artifact is unavailable".into())
}

pub(crate) fn parse_review_artifact(
    bytes: &[u8],
) -> Result<OrdinaryWorkerPairApprovalReviewArtifactV1, String> {
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err("ordinary worker pair review artifact is invalid".into());
    }
    let artifact = serde_json::from_slice(bytes)
        .map_err(|_| "ordinary worker pair review artifact is invalid".to_string())?;
    if canonical_review_artifact_bytes(&artifact)? != bytes {
        return Err("ordinary worker pair review artifact is noncanonical".into());
    }
    Ok(artifact)
}

pub(crate) fn review_artifact_binds_request(
    artifact: &OrdinaryWorkerPairApprovalReviewArtifactV1,
    request: &OrdinaryWorkerPairApprovalRequestV1,
) -> Result<(), String> {
    validate_review_artifact(artifact)?;
    if artifact.request != *request
        || artifact.request_sha256 != canonical_pair_request_sha256(request)?
    {
        return Err("ordinary worker pair review artifact does not bind request".into());
    }
    Ok(())
}

pub(crate) fn derive_ordinary_worker_pair_review_authority(
    workspace: &Path,
    record_id: &str,
    request: &OrdinaryWorkerPairApprovalRequestV1,
) -> Result<OrdinaryWorkerPairReviewAuthorityV1, String> {
    validate_pair_request(request)?;
    let authority = resolve_ordinary_worker_pair_review_authority(workspace, record_id, request)
        .map_err(|_| "ordinary worker pair review authority is unavailable".to_string())?;
    validate_authority(&authority, request)?;
    Ok(authority)
}

pub(crate) fn validate_pair_request(
    request: &OrdinaryWorkerPairApprovalRequestV1,
) -> Result<(), String> {
    if request.schema_version != ORDINARY_WORKER_PAIR_APPROVAL_SCHEMA_VERSION
        || request.product != ORDINARY_WORKER_PAIR_APPROVAL_PRODUCT
        || request.project_id != ORDINARY_WORKER_PAIR_APPROVAL_PROJECT
        || request.purpose != ORDINARY_WORKER_PAIR_APPROVAL_PURPOSE
        || request.role != ORDINARY_WORKER_PAIR_APPROVAL_ROLE
    {
        return Err("ordinary worker pair request is invalid".into());
    }
    validate_descriptor(&request.predecessor, PREDECESSOR_ARTIFACT_ID)?;
    validate_descriptor(&request.current, CURRENT_ARTIFACT_ID)?;
    if request.predecessor.generation >= request.current.generation
        || request.predecessor.image_sha256 == request.current.image_sha256
        || request.predecessor.image_length == request.current.image_length
            && request.predecessor.image_sha256 == request.current.image_sha256
    {
        return Err("ordinary worker pair request relation is invalid".into());
    }
    Ok(())
}

fn validate_review_artifact(
    artifact: &OrdinaryWorkerPairApprovalReviewArtifactV1,
) -> Result<(), String> {
    if artifact.schema_version != ORDINARY_WORKER_PAIR_APPROVAL_SCHEMA_VERSION
        || artifact.product != ORDINARY_WORKER_PAIR_APPROVAL_PRODUCT
        || artifact.project_id != ORDINARY_WORKER_PAIR_APPROVAL_PROJECT
        || artifact.purpose != ORDINARY_WORKER_PAIR_APPROVAL_PURPOSE
        || artifact.request_sha256 != canonical_pair_request_sha256(&artifact.request)?
    {
        return Err("ordinary worker pair review artifact is invalid".into());
    }
    Ok(())
}

fn validate_descriptor(
    descriptor: &OrdinaryWorkerPairDescriptorV1,
    expected_artifact_id: &str,
) -> Result<(), String> {
    if descriptor.artifact_id != expected_artifact_id
        || descriptor.generation == 0
        || descriptor.image_length == 0
        || descriptor.image_length > MAX_IMAGE_BYTES
        || !valid_sha256(&descriptor.image_sha256)
        || !valid_identity(&descriptor.source_snapshot_id)
        || !valid_identity(&descriptor.review_session_id)
        || !valid_identity(&descriptor.review_record_id)
        || !valid_sha256(&descriptor.review_authority_sha256)
        || !valid_contract_hash(&descriptor.review_contract_hash)
        || !valid_sha256(&descriptor.review_completion_sha256)
        || !valid_sha256(&descriptor.review_remeasurement_digest)
        || !valid_identity(&descriptor.attestation_id)
        || !valid_sha256(&descriptor.attestation_sha256)
        || descriptor.attestation_candidate_sha256 != descriptor.image_sha256
        || descriptor.attestation_candidate_length != descriptor.image_length
    {
        return Err("ordinary worker pair descriptor is invalid".into());
    }
    Ok(())
}

pub(crate) fn validate_authority(
    authority: &OrdinaryWorkerPairReviewAuthorityV1,
    request: &OrdinaryWorkerPairApprovalRequestV1,
) -> Result<(), String> {
    if authority.schema_version != ORDINARY_WORKER_PAIR_APPROVAL_SCHEMA_VERSION
        || authority.product != ORDINARY_WORKER_PAIR_APPROVAL_PRODUCT
        || authority.project_id != ORDINARY_WORKER_PAIR_APPROVAL_PROJECT
        || authority.purpose != ORDINARY_WORKER_PAIR_APPROVAL_PURPOSE
        || authority.request_sha256 != canonical_pair_request_sha256(request)?
        || !valid_identity(&authority.review_record_id)
        || !valid_identity(&authority.review_session_id)
        || !valid_contract_hash(&authority.review_contract_hash)
        || !valid_sha256(&authority.review_completion_sha256)
        || !valid_sha256(&authority.remeasurement_digest)
        || !valid_sha256(&authority.authority_sha256)
    {
        return Err("ordinary worker pair review authority is invalid".into());
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
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

    fn descriptor(slot: &str, generation: u64, image: char) -> OrdinaryWorkerPairDescriptorV1 {
        OrdinaryWorkerPairDescriptorV1 {
            artifact_id: slot.into(),
            generation,
            image_sha256: sha(image),
            image_length: generation + 10,
            source_snapshot_id: format!("snapshot-{generation}"),
            review_session_id: "review-session".into(),
            review_record_id: "review-record".into(),
            review_authority_sha256: sha('c'),
            review_contract_hash: "fnv1a64:0123456789abcdef".into(),
            review_completion_sha256: sha('b'),
            review_remeasurement_digest: sha('c'),
            attestation_id: format!("attestation-{generation}"),
            attestation_sha256: sha('d'),
            attestation_candidate_sha256: sha(image),
            attestation_candidate_length: generation + 10,
        }
    }

    fn request() -> OrdinaryWorkerPairApprovalRequestV1 {
        OrdinaryWorkerPairApprovalRequestV1 {
            schema_version: 1,
            product: "CatDesk".into(),
            project_id: "catdesk".into(),
            purpose: ORDINARY_WORKER_PAIR_APPROVAL_PURPOSE.into(),
            role: ORDINARY_WORKER_PAIR_APPROVAL_ROLE.into(),
            predecessor: descriptor(PREDECESSOR_ARTIFACT_ID, 1, 'e'),
            current: descriptor(CURRENT_ARTIFACT_ID, 2, 'f'),
        }
    }

    #[test]
    fn exact_typed_artifact_binds_canonical_pair_request_idempotently() {
        let request = request();
        let artifact = review_artifact_for_pair_request(request.clone()).unwrap();
        let bytes = canonical_review_artifact_bytes(&artifact).unwrap();
        assert_eq!(parse_review_artifact(&bytes).unwrap(), artifact);
        assert_eq!(
            canonical_pair_request_sha256(&request).unwrap(),
            artifact.request_sha256
        );
        review_artifact_binds_request(&artifact, &request).unwrap();
    }

    #[test]
    fn hostile_shapes_domains_and_pair_relations_fail_closed() {
        let request = request();
        let artifact = review_artifact_for_pair_request(request.clone()).unwrap();
        assert!(parse_review_artifact(br#"{\"schemaVersion\":1,\"unknown\":true}"#).is_err());
        let mut noncanonical = canonical_review_artifact_bytes(&artifact).unwrap();
        noncanonical.push(b'\n');
        assert!(parse_review_artifact(&noncanonical).is_err());
        for mutate in [
            |value: &mut OrdinaryWorkerPairApprovalRequestV1| value.product = "Other".into(),
            |value: &mut OrdinaryWorkerPairApprovalRequestV1| {
                value.role = "reviewed-main-image".into()
            },
            |value: &mut OrdinaryWorkerPairApprovalRequestV1| value.predecessor.generation = 2,
            |value: &mut OrdinaryWorkerPairApprovalRequestV1| {
                value.predecessor.image_sha256 = sha('f')
            },
            |value: &mut OrdinaryWorkerPairApprovalRequestV1| {
                value.current.attestation_candidate_length = 1
            },
            |value: &mut OrdinaryWorkerPairApprovalRequestV1| {
                value.current.review_session_id = "other".into()
            },
        ] {
            let mut changed = request.clone();
            mutate(&mut changed);
            assert!(review_artifact_binds_request(&artifact, &changed).is_err());
        }
    }

    fn authority(
        request: &OrdinaryWorkerPairApprovalRequestV1,
    ) -> OrdinaryWorkerPairReviewAuthorityV1 {
        OrdinaryWorkerPairReviewAuthorityV1 {
            schema_version: 1,
            product: "CatDesk".into(),
            project_id: "catdesk".into(),
            purpose: ORDINARY_WORKER_PAIR_APPROVAL_PURPOSE.into(),
            request_sha256: canonical_pair_request_sha256(request).unwrap(),
            review_record_id: "pair-approval-record".into(),
            review_session_id: "pair-approval-session".into(),
            review_contract_hash: "fnv1a64:0123456789abcdef".into(),
            review_completion_sha256: sha('a'),
            remeasurement_digest: sha('b'),
            authority_sha256: sha('c'),
        }
    }

    #[test]
    fn authority_is_request_bound_and_pair_module_has_no_writer_or_runtime_path() {
        let request = request();
        let authority = authority(&request);
        validate_authority(&authority, &request).unwrap();
        for mutate in [
            |value: &mut OrdinaryWorkerPairApprovalRequestV1| {
                value.current.source_snapshot_id = "other".into()
            },
            |value: &mut OrdinaryWorkerPairApprovalRequestV1| {
                value.current.review_authority_sha256 = sha('d')
            },
            |value: &mut OrdinaryWorkerPairApprovalRequestV1| {
                value.current.attestation_sha256 = sha('e')
            },
            |value: &mut OrdinaryWorkerPairApprovalRequestV1| value.current.image_length += 1,
        ] {
            let mut changed = request.clone();
            mutate(&mut changed);
            assert!(validate_authority(&authority, &changed).is_err());
        }
        let source = include_str!("ordinary_worker_pair_approval.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        for forbidden in [
            "ProtectedDirectoryGuard",
            "ProgramData",
            "Command::",
            "activate(",
            "write(",
            "create_dir",
        ] {
            assert!(
                !source.contains(forbidden),
                "pair authority must not acquire writer/runtime authority: {forbidden}"
            );
        }
    }
}
