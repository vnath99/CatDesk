//! Fixed, protected durable evidence for the core host gates (T-0312).
//!
//! This module is deliberately not an observation producer and not an
//! acceptance evaluator.  It persists only an already request-bound,
//! independently authorized approval alongside the exact product-derived
//! observation request that it approves.  The preflight evaluator remains the
//! only component that classifies acceptance.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core_host_gate_approval::{
    CORE_HOST_GATE_APPROVAL_PRODUCT, CORE_HOST_GATE_APPROVAL_PROJECT,
    CoreHostGateApprovalRequestV1, CoreHostGateApprovalV1, CoreHostGateV1,
    canonical_request_sha256, validate_core_host_gate_approval, validate_request,
};
use crate::delegated::autonomy_supervisor::CoreHostGateReviewAuthorityV1;

#[cfg(windows)]
use crate::windows_protected_fs::{
    PinnedDirectory, ProtectedDirectoryGuard, read_optional_relative_regular,
    write_unique_regular_for_atomic_replace,
};

pub(crate) const CORE_HOST_GATE_EVIDENCE_SCHEMA_VERSION: u32 = 1;
pub(crate) const CORE_HOST_GATE_EVIDENCE_PURPOSE: &str = "core-host-gate-evidence-v1";
const MAX_EVIDENCE_BYTES: u64 = 32 * 1024;
const EVIDENCE_FILES: [(CoreHostGateV1, &str); 3] = [
    (CoreHostGateV1::T0223, "t0223.json"),
    (CoreHostGateV1::T0222, "t0222.json"),
    (CoreHostGateV1::T0152, "t0152.json"),
];

/// A product-owned fixed location.  No operator, MCP request, or caller can
/// select a different production root, directory, or filename.
fn fixed_core_host_gate_evidence_root() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\ProgramData\CatDesk\CoreHostGateEvidence")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/var/lib/catdesk/core-host-gate-evidence")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CoreHostGateEvidenceRecordV1 {
    pub(crate) schema_version: u32,
    pub(crate) product: String,
    pub(crate) project_id: String,
    pub(crate) purpose: String,
    pub(crate) request: CoreHostGateApprovalRequestV1,
    pub(crate) request_sha256: String,
    pub(crate) reviewer_authority: CoreHostGateReviewAuthorityV1,
    pub(crate) approval: CoreHostGateApprovalV1,
    pub(crate) finalized_at_unix: u64,
}

/// The only data the preflight adapter receives from this ledger.  It cannot
/// receive an acceptance boolean or arbitrary operator evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CoreHostGateEvidenceProjectionV1 {
    pub(crate) project_id: String,
    pub(crate) session_id: String,
    pub(crate) record_id: String,
    pub(crate) target_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CoreHostGateEvidenceReadV1 {
    Missing,
    Invalid,
    Exact(CoreHostGateEvidenceProjectionV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CoreHostGateEvidenceReadSetV1 {
    pub(crate) t0223: CoreHostGateEvidenceReadV1,
    pub(crate) t0222: CoreHostGateEvidenceReadV1,
    pub(crate) t0152: CoreHostGateEvidenceReadV1,
}

/// Fixed production finalizer.  The root is compiled and no storage location
/// appears in this API.  T-0312 does not call it against real ProgramData.
#[allow(dead_code)] // The future fixed capture seam is T-0313.
pub(crate) fn finalize_fixed_core_host_gate_evidence(
    request: &CoreHostGateApprovalRequestV1,
    approval: &CoreHostGateApprovalV1,
    authority: &CoreHostGateReviewAuthorityV1,
    finalized_at_unix: u64,
) -> Result<(), String> {
    finalize_at_root(
        &fixed_core_host_gate_evidence_root(),
        request,
        approval,
        authority,
        finalized_at_unix,
    )
}

/// Read-only production bridge.  Absence and every protected-FS/canonical
/// failure remain non-authoritative and are never promoted by this module.
pub(crate) fn read_fixed_core_host_gate_evidence() -> CoreHostGateEvidenceReadSetV1 {
    read_at_root(&fixed_core_host_gate_evidence_root())
}

#[cfg(test)]
fn finalize_for_test(
    root: &Path,
    request: &CoreHostGateApprovalRequestV1,
    approval: &CoreHostGateApprovalV1,
    authority: &CoreHostGateReviewAuthorityV1,
    finalized_at_unix: u64,
) -> Result<(), String> {
    finalize_at_root(root, request, approval, authority, finalized_at_unix)
}

#[cfg(test)]
fn read_for_test(root: &Path) -> CoreHostGateEvidenceReadSetV1 {
    read_at_root(root)
}

fn finalize_at_root(
    root: &Path,
    request: &CoreHostGateApprovalRequestV1,
    approval: &CoreHostGateApprovalV1,
    authority: &CoreHostGateReviewAuthorityV1,
    finalized_at_unix: u64,
) -> Result<(), String> {
    validate_request(request)?;
    validate_core_host_gate_approval(approval, request, authority)?;
    if finalized_at_unix == 0 {
        return Err("core host gate evidence finalization is invalid".into());
    }
    let record = CoreHostGateEvidenceRecordV1 {
        schema_version: CORE_HOST_GATE_EVIDENCE_SCHEMA_VERSION,
        product: CORE_HOST_GATE_APPROVAL_PRODUCT.into(),
        project_id: CORE_HOST_GATE_APPROVAL_PROJECT.into(),
        purpose: CORE_HOST_GATE_EVIDENCE_PURPOSE.into(),
        request: request.clone(),
        request_sha256: canonical_request_sha256(request)?,
        reviewer_authority: authority.clone(),
        approval: approval.clone(),
        finalized_at_unix,
    };
    let bytes = canonical_record_bytes(&record)?;
    #[cfg(windows)]
    {
        let guard = protected_evidence_root(root, true)?;
        let name = evidence_file(request.gate)?;
        match read_record_from_guard(&guard, name)? {
            None => {}
            Some((existing, existing_bytes)) if existing == record && existing_bytes == bytes => {
                return Ok(());
            }
            Some((existing, _)) if existing.request.revision > request.revision => {
                return Err("core host gate evidence rollback is refused".into());
            }
            Some((existing, _)) if existing.request.revision == request.revision => {
                return Err("core host gate evidence conflict is refused".into());
            }
            Some((existing, _))
                if existing.reviewer_authority.review_record_id
                    == record.reviewer_authority.review_record_id =>
            {
                // T-0311-R1 binds one independent completion to one canonical
                // request. A later revision cannot reuse that completion for a
                // changed observation, target, T-0224 tuple, runtime, or
                // authority identity.
                return Err("core host gate evidence reviewer replay is refused".into());
            }
            Some(_) => {}
        }
        for (gate, other_name) in EVIDENCE_FILES {
            if gate == request.gate {
                continue;
            }
            if let Some((other, _)) = read_record_from_guard(&guard, other_name)?
                && (other.request_sha256 == record.request_sha256
                    || other.reviewer_authority.review_record_id
                        == record.reviewer_authority.review_record_id)
            {
                return Err("core host gate evidence cross-gate replay is refused".into());
            }
        }
        write_unique_regular_for_atomic_replace(
            &guard,
            &bytes,
            "core host gate evidence temporary",
        )
        .and_then(|temporary| temporary.commit_replace(name, "core host gate evidence commit"))
        .map_err(|_| "core host gate evidence is unavailable".to_string())?;
        // Reopen through the same pinned root and prove exact canonical bytes.
        let Some((reopened, reopened_bytes)) = read_record_from_guard(&guard, name)? else {
            return Err("core host gate evidence commit is unavailable".into());
        };
        if reopened != record || reopened_bytes != bytes {
            return Err("core host gate evidence commit is invalid".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (root, bytes);
        Err("core host gate evidence protected storage is unavailable".into())
    }
}

fn read_at_root(root: &Path) -> CoreHostGateEvidenceReadSetV1 {
    #[cfg(windows)]
    {
        let Ok(guard) = protected_evidence_root(root, false) else {
            return missing_set();
        };
        CoreHostGateEvidenceReadSetV1 {
            t0223: read_projection(&guard, CoreHostGateV1::T0223),
            t0222: read_projection(&guard, CoreHostGateV1::T0222),
            t0152: read_projection(&guard, CoreHostGateV1::T0152),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        missing_set()
    }
}

fn missing_set() -> CoreHostGateEvidenceReadSetV1 {
    CoreHostGateEvidenceReadSetV1 {
        t0223: CoreHostGateEvidenceReadV1::Missing,
        t0222: CoreHostGateEvidenceReadV1::Missing,
        t0152: CoreHostGateEvidenceReadV1::Missing,
    }
}

#[cfg(windows)]
fn read_projection(
    guard: &ProtectedDirectoryGuard,
    gate: CoreHostGateV1,
) -> CoreHostGateEvidenceReadV1 {
    let Ok(name) = evidence_file(gate) else {
        return CoreHostGateEvidenceReadV1::Invalid;
    };
    match read_record_from_guard(guard, name) {
        Ok(None) => CoreHostGateEvidenceReadV1::Missing,
        Ok(Some((record, _))) if record.request.gate == gate => {
            CoreHostGateEvidenceReadV1::Exact(CoreHostGateEvidenceProjectionV1 {
                project_id: record.request.project_id,
                session_id: record.request.t0224_session_id,
                record_id: record.request.t0224_review_record_id,
                target_sha256: record.request.current_target_sha256,
            })
        }
        _ => CoreHostGateEvidenceReadV1::Invalid,
    }
}

fn canonical_record_bytes(record: &CoreHostGateEvidenceRecordV1) -> Result<Vec<u8>, String> {
    validate_record(record)?;
    serde_json::to_vec(record).map_err(|_| "core host gate evidence is unavailable".into())
}

#[cfg(windows)]
fn read_record_from_guard(
    guard: &ProtectedDirectoryGuard,
    name: &str,
) -> Result<Option<(CoreHostGateEvidenceRecordV1, Vec<u8>)>, String> {
    let Some(bytes) =
        read_optional_relative_regular(guard, name, MAX_EVIDENCE_BYTES, "core host gate evidence")
            .map_err(|_| "core host gate evidence is unavailable".to_string())?
    else {
        return Ok(None);
    };
    let record: CoreHostGateEvidenceRecordV1 = serde_json::from_slice(&bytes)
        .map_err(|_| "core host gate evidence is invalid".to_string())?;
    if canonical_record_bytes(&record)? != bytes {
        return Err("core host gate evidence is noncanonical".into());
    }
    Ok(Some((record, bytes)))
}

fn validate_record(record: &CoreHostGateEvidenceRecordV1) -> Result<(), String> {
    if record.schema_version != CORE_HOST_GATE_EVIDENCE_SCHEMA_VERSION
        || record.product != CORE_HOST_GATE_APPROVAL_PRODUCT
        || record.project_id != CORE_HOST_GATE_APPROVAL_PROJECT
        || record.purpose != CORE_HOST_GATE_EVIDENCE_PURPOSE
        || record.finalized_at_unix == 0
        || record.request_sha256 != canonical_request_sha256(&record.request)?
    {
        return Err("core host gate evidence is invalid".into());
    }
    validate_core_host_gate_approval(
        &record.approval,
        &record.request,
        &record.reviewer_authority,
    )
    .map_err(|_| "core host gate evidence authority is invalid".to_string())?;
    Ok(())
}

fn evidence_file(gate: CoreHostGateV1) -> Result<&'static str, String> {
    EVIDENCE_FILES
        .iter()
        .find_map(|(candidate, name)| (*candidate == gate).then_some(*name))
        .ok_or_else(|| "core host gate is invalid".into())
}

#[cfg(windows)]
fn protected_evidence_root(
    root: &Path,
    create_missing: bool,
) -> Result<ProtectedDirectoryGuard, String> {
    let fixed = fixed_core_host_gate_evidence_root();
    if root == fixed {
        let program_data = PinnedDirectory::acquire(
            Path::new(r"C:\ProgramData"),
            "core host gate evidence ProgramData parent",
        )
        .map_err(|_| "core host gate evidence is unavailable".to_string())?;
        let mut guard = ProtectedDirectoryGuard::from_pinned_root(program_data);
        descend_fixed_component(&mut guard, "CatDesk", create_missing)?;
        descend_fixed_component(&mut guard, "CoreHostGateEvidence", create_missing)?;
        return Ok(guard);
    }
    // Alternate roots are compiled only into test builds.  Production callers
    // cannot pass a root and the public finalizer always selects ProgramData.
    #[cfg(test)]
    {
        let parent = root
            .parent()
            .ok_or_else(|| "core host gate evidence is unavailable".to_string())?;
        let component = root
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| "core host gate evidence is unavailable".to_string())?;
        let parent = PinnedDirectory::acquire(parent, "core host gate evidence test parent")
            .map_err(|_| "core host gate evidence is unavailable".to_string())?;
        let mut guard = ProtectedDirectoryGuard::from_pinned_root(parent);
        descend_fixed_component(&mut guard, component, create_missing)?;
        Ok(guard)
    }
    #[cfg(not(test))]
    {
        let _ = create_missing;
        Err("core host gate evidence root is unavailable".into())
    }
}

#[cfg(windows)]
fn descend_fixed_component(
    guard: &mut ProtectedDirectoryGuard,
    component: &str,
    create_missing: bool,
) -> Result<(), String> {
    if guard
        .descend_optional_existing(component, "core host gate evidence root")
        .map_err(|_| "core host gate evidence root is unavailable".to_string())?
    {
        return Ok(());
    }
    if !create_missing {
        return Err("core host gate evidence root is unavailable".into());
    }
    guard
        .create_child(component, "core host gate evidence root")
        .map_err(|_| "core host gate evidence root is unavailable".to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::core_host_gate_approval::{
        CORE_HOST_GATE_APPROVAL_PURPOSE, CORE_HOST_GATE_APPROVAL_SCHEMA_VERSION,
        CoreHostGateRuntimeIdentityV1, issue_core_host_gate_approval,
    };

    fn sha(letter: char) -> String {
        std::iter::repeat_n(letter, 64).collect()
    }

    fn request(gate: CoreHostGateV1, revision: u64) -> CoreHostGateApprovalRequestV1 {
        CoreHostGateApprovalRequestV1 {
            schema_version: CORE_HOST_GATE_APPROVAL_SCHEMA_VERSION,
            product: CORE_HOST_GATE_APPROVAL_PRODUCT.into(),
            project_id: CORE_HOST_GATE_APPROVAL_PROJECT.into(),
            purpose: CORE_HOST_GATE_APPROVAL_PURPOSE.into(),
            gate,
            observation_id: "observed-host-state".into(),
            observation_sha256: sha('a'),
            t0224_session_id: "wake-session".into(),
            t0224_review_record_id: "wake-review".into(),
            current_target_sha256: sha('b'),
            runtime: CoreHostGateRuntimeIdentityV1 {
                build_identity: "reviewed-build".into(),
                generation: 7,
                host_session_id: "host-session".into(),
            },
            issued_at_unix: 1,
            revision,
        }
    }

    fn authority(request: &CoreHostGateApprovalRequestV1) -> CoreHostGateReviewAuthorityV1 {
        CoreHostGateReviewAuthorityV1 {
            schema_version: CORE_HOST_GATE_APPROVAL_SCHEMA_VERSION,
            product: CORE_HOST_GATE_APPROVAL_PRODUCT.into(),
            project_id: CORE_HOST_GATE_APPROVAL_PROJECT.into(),
            purpose: CORE_HOST_GATE_APPROVAL_PURPOSE.into(),
            request_sha256: canonical_request_sha256(request).unwrap(),
            review_record_id: "approval-review".into(),
            review_session_id: "approval-session".into(),
            review_contract_hash: "fnv1a64:0123456789abcdef".into(),
            review_completion_sha256: sha('c'),
            remeasurement_digest: sha('d'),
            authority_sha256: sha('e'),
        }
    }

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "catdesk-core-host-gate-evidence-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn finalized_exact_record_is_read_only_projected_and_idempotent() {
        let root = root("exact");
        let requested = request(CoreHostGateV1::T0223, 1);
        let review_authority = authority(&requested);
        let receipt = issue_core_host_gate_approval(requested.clone(), &review_authority).unwrap();
        finalize_for_test(&root, &requested, &receipt, &review_authority, 2).unwrap();
        finalize_for_test(&root, &requested, &receipt, &review_authority, 2).unwrap();
        assert!(matches!(
            read_for_test(&root).t0223,
            CoreHostGateEvidenceReadV1::Exact(CoreHostGateEvidenceProjectionV1 { ref project_id, ref session_id, ref record_id, ref target_sha256 })
                if project_id == "catdesk" && session_id == "wake-session" && record_id == "wake-review" && target_sha256 == &sha('b')
        ));
    }

    #[test]
    fn mismatches_replay_and_noncanonical_protected_records_fail_closed() {
        let root = root("hostile");
        let requested = request(CoreHostGateV1::T0223, 2);
        let review_authority = authority(&requested);
        let receipt = issue_core_host_gate_approval(requested.clone(), &review_authority).unwrap();
        finalize_for_test(&root, &requested, &receipt, &review_authority, 3).unwrap();

        let mut changed = requested.clone();
        changed.observation_sha256 = sha('f');
        let changed_authority = authority(&changed);
        let changed_receipt =
            issue_core_host_gate_approval(changed.clone(), &changed_authority).unwrap();
        assert!(
            finalize_for_test(&root, &changed, &changed_receipt, &changed_authority, 4).is_err()
        );

        let older = request(CoreHostGateV1::T0223, 1);
        let older_authority = authority(&older);
        let older_receipt = issue_core_host_gate_approval(older.clone(), &older_authority).unwrap();
        assert!(finalize_for_test(&root, &older, &older_receipt, &older_authority, 4).is_err());

        let mut replayed = requested.clone();
        replayed.observation_sha256 = sha('e');
        replayed.revision = 3;
        let mut replayed_authority = authority(&replayed);
        // Model a malicious durable replay of the already-consumed review
        // completion for a changed request. The real T-0311-R1 resolver also
        // refuses it, but this ledger must independently retain that boundary.
        replayed_authority.review_record_id = review_authority.review_record_id.clone();
        let replayed_receipt =
            issue_core_host_gate_approval(replayed.clone(), &replayed_authority).unwrap();
        assert!(
            finalize_for_test(&root, &replayed, &replayed_receipt, &replayed_authority, 4).is_err()
        );

        let mut cross = requested.clone();
        cross.gate = CoreHostGateV1::T0222;
        let cross_authority = authority(&cross);
        let cross_receipt = issue_core_host_gate_approval(cross.clone(), &cross_authority).unwrap();
        assert!(finalize_for_test(&root, &cross, &cross_receipt, &cross_authority, 4).is_err());

        let path = root.join("t0223.json");
        fs::write(path, br#"{\"schemaVersion\":1}"#).unwrap();
        assert_eq!(
            read_for_test(&root).t0223,
            CoreHostGateEvidenceReadV1::Invalid
        );
    }
}
