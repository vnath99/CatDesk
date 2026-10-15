//! One-shot, read-only service admission for a future fixed Wake broker.
//!
//! The separately provisioned service must connect the fixed endpoint with a
//! bounded deadline and give this function exclusive ownership of its last
//! request read. This module consumes that endpoint, checks the sender of the
//! single message, and verifies the detached approval with the pinned policy.
//! It deliberately has no install, ledger, CI-fetch, response, or launch API.
//! The observation supplied here must eventually be assembled by the fixed
//! service from its own pinned protected and old-owner objects, never from a
//! request. No legacy `install.ps1` manifest or candidate hash is used as
//! package-byte evidence. Until that provider and the remaining transaction
//! gates exist, even a successful readback is not installation authority.

#![cfg(windows)]

use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::Result;
use crate::install_broker_approval::{BrokerObservedStateV1, ObjectIdentityV1, VerifiedApprovalV1};
use crate::install_broker_endpoint::FixedBrokerPipeEndpoint;
use crate::install_broker_policy::BrokerProtectedPolicyV1;
use crate::install_broker_request::read_admitted_approval_frame;

/// A diagnostic receipt only. It contains no signed envelope, policy key,
/// endpoint, verified-approval object, ledger lease, or mutation capability.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerSignatureBindingReadbackV1 {
    pub schema_version: u32,
    pub approval_id: String,
    pub install_generation: u64,
    pub package_id: String,
    pub manifest_sha256: String,
    pub source_commit: String,
    pub signed_payload_sha256: String,
    pub operator_sid_sha256: String,
    pub operator_session_id: u32,
    pub pending_gates: [&'static str; 6],
    pub disposition: &'static str,
}

fn require_policy_root_before_read(
    policy_root: &ObjectIdentityV1,
    observed: &BrokerObservedStateV1,
) -> Result<()> {
    if observed.target_protected_root != *policy_root {
        return Err("WAKE_BROKER_SERVICE_PROTECTED_ROOT_MISMATCH".into());
    }
    Ok(())
}

fn signed_readback(
    approval: &VerifiedApprovalV1,
    operator_sid_sha256: &str,
    operator_session_id: u32,
) -> BrokerSignatureBindingReadbackV1 {
    let payload = approval.payload();
    BrokerSignatureBindingReadbackV1 {
        schema_version: 1,
        approval_id: payload.approval_id.clone(),
        install_generation: payload.install_generation,
        package_id: payload.manifest.package_id.clone(),
        manifest_sha256: payload.manifest_sha256.clone(),
        source_commit: payload.source_commit.clone(),
        signed_payload_sha256: approval.signed_payload_sha256().into(),
        operator_sid_sha256: operator_sid_sha256.into(),
        operator_session_id,
        pending_gates: [
            "TRUSTED_BROKER_CLOCK_AND_PINNED_PREDECESSOR_OBSERVATIONS",
            "BROKER_OWNED_EXACT_FOUR_ARTIFACT_BYTE_INVENTORY",
            "INDEPENDENT_EXACT_SOURCE_GITHUB_CI_FETCH",
            "PROTECTED_LEDGER_REPLAY_AND_DURABILITY",
            "PINNED_OLD_AND_NEW_INSTALLED_OWNER_PROOF",
            "PROTECTED_POINTER_LAUNCH_READER_AND_SERVICE_BOOTSTRAP",
        ],
        disposition: "SIGNATURE_AND_SUPPLIED_BINDING_READ_ONLY_NO_INSTALL_AUTHORITY",
    }
}

/// Consume one already-connected fixed endpoint. The policy must be loaded
/// from the compiled protected root. `observed` is read-only evidence which a
/// future service must derive independently from pinned objects and its own
/// clock. This function does not independently fetch CI, inspect candidate
/// bytes, create an intent record, or change any install state.
///
/// The caller must enforce a bounded connect/read deadline and must not read
/// from the pipe between connection and this call. A successful receipt does
/// not authorize retry, publication, activation, or a client success reply.
pub fn inspect_one_connected_submission_read_only(
    mut endpoint: FixedBrokerPipeEndpoint,
    policy: BrokerProtectedPolicyV1,
    observed: &BrokerObservedStateV1,
) -> Result<BrokerSignatureBindingReadbackV1> {
    let policy_root = policy.readback()?.protected_root;
    require_policy_root_before_read(&policy_root, observed)?;
    let frame = read_admitted_approval_frame(
        &mut endpoint,
        &policy,
        Duration::from_secs(10),
        &AtomicBool::new(false),
    )?;
    let approval = policy.verify_approval(frame.approval_envelope(), observed)?;
    // The readback remains advisory. The service has no trusted candidate
    // stage, independent fixed GitHub fetcher, or phase transaction here.
    Ok(signed_readback(
        &approval,
        frame.operator_evidence().operator_sid_sha256(),
        frame.operator_evidence().session_id(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_broker_approval::{tests, verify_approval};

    fn signed_fixture() -> (BrokerObservedStateV1, VerifiedApprovalV1) {
        let (payload, observed, key) = tests::fixture();
        let envelope = tests::envelope(&payload, &key);
        let approval = verify_approval(&envelope, &key.verifying_key(), &observed)
            .expect("signed fixture approval");
        (observed, approval)
    }

    #[test]
    fn readback_reports_signature_evidence_without_claiming_install_or_ci() {
        let (_, approval) = signed_fixture();
        let receipt = signed_readback(&approval, &"a".repeat(64), 7);
        assert_eq!(receipt.schema_version, 1);
        assert_eq!(receipt.approval_id, approval.payload().approval_id);
        assert_eq!(
            receipt.install_generation,
            approval.payload().install_generation
        );
        assert_eq!(receipt.package_id, approval.payload().manifest.package_id);
        assert_eq!(receipt.source_commit, approval.payload().source_commit);
        assert_eq!(
            receipt.signed_payload_sha256,
            approval.signed_payload_sha256()
        );
        assert_eq!(receipt.operator_session_id, 7);
        assert_eq!(
            receipt.disposition,
            "SIGNATURE_AND_SUPPLIED_BINDING_READ_ONLY_NO_INSTALL_AUTHORITY"
        );
        let json = serde_json::to_value(receipt).expect("serialize readback");
        assert!(json.get("approvalEnvelope").is_none());
        assert!(json.get("approvedForInstall").is_none());
        let gates = json["pendingGates"].as_array().expect("pending gates");
        assert_eq!(gates.len(), 6);
        assert!(
            gates
                .iter()
                .any(|gate| gate == "BROKER_OWNED_EXACT_FOUR_ARTIFACT_BYTE_INVENTORY")
        );
        assert!(
            gates
                .iter()
                .any(|gate| gate == "INDEPENDENT_EXACT_SOURCE_GITHUB_CI_FETCH")
        );
        assert!(
            gates
                .iter()
                .any(|gate| gate == "PINNED_OLD_AND_NEW_INSTALLED_OWNER_PROOF")
        );
    }

    #[test]
    fn wrong_protected_root_is_refused_before_request_read() {
        let (mut observed, _) = signed_fixture();
        let true_root = observed.target_protected_root.clone();
        observed.target_protected_root.file_index_low += 1;
        assert_eq!(
            require_policy_root_before_read(&true_root, &observed).unwrap_err(),
            "WAKE_BROKER_SERVICE_PROTECTED_ROOT_MISMATCH"
        );
    }
}
