//! Purpose-separated reviewed authority for one exact CatDesk daemon-reload candidate.
//!
//! This module is deliberately pure: it has no MCP, process, protected-state,
//! filesystem-writer, or runtime integration. A reload review can authorize
//! only the exact immutable candidate identity encoded in the canonical request.

use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const DAEMON_RELOAD_APPROVAL_SCHEMA_VERSION: u32 = 1;
pub(crate) const DAEMON_RELOAD_APPROVAL_PRODUCT: &str = "CatDesk";
pub(crate) const DAEMON_RELOAD_APPROVAL_PROJECT: &str = "catdesk";
pub(crate) const DAEMON_RELOAD_APPROVAL_PURPOSE: &str = "catdesk-daemon-reload-approval-v1";
/// The only immutable reviewed-completion artifact permitted to bind a reload.
pub(crate) const DAEMON_RELOAD_APPROVAL_REVIEW_ARTIFACT_ID: &str =
    "src/daemon-reload-approval-request-v1.json";

const MAX_ARTIFACT_BYTES: usize = 8 * 1024;
const MAX_IDENTITY_BYTES: usize = 128;
const MAX_PATH_BYTES: usize = 4096;
const MAX_IMAGE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DaemonReloadApprovalRequestV1 {
    pub(crate) schema_version: u32,
    pub(crate) product: String,
    pub(crate) project_id: String,
    pub(crate) purpose: String,
    pub(crate) candidate_relative_path: String,
    pub(crate) candidate_sha256: String,
    pub(crate) candidate_length: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DaemonReloadApprovalReviewArtifactV1 {
    pub(crate) schema_version: u32,
    pub(crate) product: String,
    pub(crate) project_id: String,
    pub(crate) purpose: String,
    pub(crate) request: DaemonReloadApprovalRequestV1,
    pub(crate) request_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DaemonReloadReviewAuthorityV1 {
    pub(crate) schema_version: u32,
    pub(crate) product: String,
    pub(crate) project_id: String,
    pub(crate) purpose: String,
    pub(crate) request_sha256: String,
    pub(crate) review_record_id: String,
    pub(crate) review_session_id: String,
    pub(crate) review_contract_hash: String,
    pub(crate) review_completion_sha256: String,
    pub(crate) remeasurement_digest: String,
    pub(crate) authority_sha256: String,
}

pub(crate) fn canonical_request_bytes(
    request: &DaemonReloadApprovalRequestV1,
) -> Result<Vec<u8>, String> {
    validate_request(request)?;
    serde_json::to_vec(request).map_err(|_| "daemon reload approval request is unavailable".into())
}

pub(crate) fn canonical_request_sha256(
    request: &DaemonReloadApprovalRequestV1,
) -> Result<String, String> {
    Ok(digest(&canonical_request_bytes(request)?))
}

#[allow(dead_code)] // Used by review/candidate-authority producers and unit fixtures, not runtime execution.
pub(crate) fn review_artifact_for_request(
    request: DaemonReloadApprovalRequestV1,
) -> Result<DaemonReloadApprovalReviewArtifactV1, String> {
    let request_sha256 = canonical_request_sha256(&request)?;
    Ok(DaemonReloadApprovalReviewArtifactV1 {
        schema_version: DAEMON_RELOAD_APPROVAL_SCHEMA_VERSION,
        product: DAEMON_RELOAD_APPROVAL_PRODUCT.into(),
        project_id: DAEMON_RELOAD_APPROVAL_PROJECT.into(),
        purpose: DAEMON_RELOAD_APPROVAL_PURPOSE.into(),
        request,
        request_sha256,
    })
}

pub(crate) fn canonical_review_artifact_bytes(
    artifact: &DaemonReloadApprovalReviewArtifactV1,
) -> Result<Vec<u8>, String> {
    validate_review_artifact(artifact)?;
    serde_json::to_vec(artifact)
        .map_err(|_| "daemon reload approval review artifact is unavailable".into())
}

pub(crate) fn parse_review_artifact(
    bytes: &[u8],
) -> Result<DaemonReloadApprovalReviewArtifactV1, String> {
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err("daemon reload approval review artifact is invalid".into());
    }
    let artifact: DaemonReloadApprovalReviewArtifactV1 = serde_json::from_slice(bytes)
        .map_err(|_| "daemon reload approval review artifact is invalid".to_string())?;
    if canonical_review_artifact_bytes(&artifact)? != bytes {
        return Err("daemon reload approval review artifact is noncanonical".into());
    }
    Ok(artifact)
}

pub(crate) fn review_artifact_binds_request(
    artifact: &DaemonReloadApprovalReviewArtifactV1,
    request: &DaemonReloadApprovalRequestV1,
) -> Result<(), String> {
    validate_review_artifact(artifact)?;
    if artifact.request != *request || artifact.request_sha256 != canonical_request_sha256(request)?
    {
        return Err("daemon reload approval review artifact does not bind request".into());
    }
    Ok(())
}

pub(crate) fn validate_request(request: &DaemonReloadApprovalRequestV1) -> Result<(), String> {
    if request.schema_version != DAEMON_RELOAD_APPROVAL_SCHEMA_VERSION
        || request.product != DAEMON_RELOAD_APPROVAL_PRODUCT
        || request.project_id != DAEMON_RELOAD_APPROVAL_PROJECT
        || request.purpose != DAEMON_RELOAD_APPROVAL_PURPOSE
        || !valid_candidate_relative_path(&request.candidate_relative_path)
        || !valid_sha256(&request.candidate_sha256)
        || request.candidate_length == 0
        || request.candidate_length > MAX_IMAGE_BYTES
    {
        return Err("daemon reload approval request is invalid".into());
    }
    Ok(())
}

pub(crate) fn validate_authority(
    authority: &DaemonReloadReviewAuthorityV1,
    request: &DaemonReloadApprovalRequestV1,
) -> Result<(), String> {
    if authority.schema_version != DAEMON_RELOAD_APPROVAL_SCHEMA_VERSION
        || authority.product != DAEMON_RELOAD_APPROVAL_PRODUCT
        || authority.project_id != DAEMON_RELOAD_APPROVAL_PROJECT
        || authority.purpose != DAEMON_RELOAD_APPROVAL_PURPOSE
        || authority.request_sha256 != canonical_request_sha256(request)?
        || !valid_identity(&authority.review_record_id)
        || !valid_identity(&authority.review_session_id)
        || !valid_contract_hash(&authority.review_contract_hash)
        || !valid_sha256(&authority.review_completion_sha256)
        || !valid_sha256(&authority.remeasurement_digest)
        || !valid_sha256(&authority.authority_sha256)
    {
        return Err("daemon reload review authority is invalid".into());
    }
    Ok(())
}

fn validate_review_artifact(artifact: &DaemonReloadApprovalReviewArtifactV1) -> Result<(), String> {
    if artifact.schema_version != DAEMON_RELOAD_APPROVAL_SCHEMA_VERSION
        || artifact.product != DAEMON_RELOAD_APPROVAL_PRODUCT
        || artifact.project_id != DAEMON_RELOAD_APPROVAL_PROJECT
        || artifact.purpose != DAEMON_RELOAD_APPROVAL_PURPOSE
        || artifact.request_sha256 != canonical_request_sha256(&artifact.request)?
    {
        return Err("daemon reload approval review artifact is invalid".into());
    }
    Ok(())
}

fn valid_candidate_relative_path(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_PATH_BYTES {
        return false;
    }
    let path = Path::new(value);
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
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

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(letter: char) -> String {
        letter.to_string().repeat(64)
    }

    fn request(path: &str, hash: char, length: u64) -> DaemonReloadApprovalRequestV1 {
        DaemonReloadApprovalRequestV1 {
            schema_version: DAEMON_RELOAD_APPROVAL_SCHEMA_VERSION,
            product: DAEMON_RELOAD_APPROVAL_PRODUCT.into(),
            project_id: DAEMON_RELOAD_APPROVAL_PROJECT.into(),
            purpose: DAEMON_RELOAD_APPROVAL_PURPOSE.into(),
            candidate_relative_path: path.into(),
            candidate_sha256: sha(hash),
            candidate_length: length,
        }
    }

    fn authority(request: &DaemonReloadApprovalRequestV1) -> DaemonReloadReviewAuthorityV1 {
        DaemonReloadReviewAuthorityV1 {
            schema_version: DAEMON_RELOAD_APPROVAL_SCHEMA_VERSION,
            product: DAEMON_RELOAD_APPROVAL_PRODUCT.into(),
            project_id: DAEMON_RELOAD_APPROVAL_PROJECT.into(),
            purpose: DAEMON_RELOAD_APPROVAL_PURPOSE.into(),
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
    fn exact_typed_artifact_binds_one_candidate() {
        let request = request(r"target-verify\t0436\catdesk.exe", 'a', 123);
        let artifact = review_artifact_for_request(request.clone()).unwrap();
        let bytes = canonical_review_artifact_bytes(&artifact).unwrap();
        assert_eq!(parse_review_artifact(&bytes).unwrap(), artifact);
        review_artifact_binds_request(&artifact, &request).unwrap();
        validate_authority(&authority(&request), &request).unwrap();
    }

    #[test]
    fn candidate_substitution_fails_closed_even_with_valid_review_artifact() {
        let candidate_a = request(r"target-verify\candidate-a\catdesk.exe", 'a', 123);
        let artifact = review_artifact_for_request(candidate_a.clone()).unwrap();
        for candidate_b in [
            request(r"target-verify\candidate-b\catdesk.exe", 'a', 123),
            request(r"target-verify\candidate-a\catdesk.exe", 'b', 123),
            request(r"target-verify\candidate-a\catdesk.exe", 'a', 124),
        ] {
            assert!(review_artifact_binds_request(&artifact, &candidate_b).is_err());
            assert!(validate_authority(&authority(&candidate_a), &candidate_b).is_err());
        }
    }

    #[test]
    fn hostile_paths_hashes_and_noncanonical_artifacts_fail_closed() {
        for path in ["", r"..\outside.exe", r".\candidate.exe", r"C:\outside.exe"] {
            assert!(validate_request(&request(path, 'a', 123)).is_err());
        }
        let mut invalid_hash = request(r"target-verify\candidate\catdesk.exe", 'a', 123);
        invalid_hash.candidate_sha256 = "A".repeat(64);
        assert!(validate_request(&invalid_hash).is_err());

        let artifact =
            review_artifact_for_request(request(r"target-verify\candidate\catdesk.exe", 'a', 123))
                .unwrap();
        let mut noncanonical = canonical_review_artifact_bytes(&artifact).unwrap();
        noncanonical.push(b'\n');
        assert!(parse_review_artifact(&noncanonical).is_err());
    }
}
