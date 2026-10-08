//! Closed reviewed-build attempt, result, and producer-attestation authority.
//!
//! This is deliberately a small host-owned surface.  Callers select an
//! acknowledged review record and later present an opaque token; they never
//! select a tool, source root, output, command line, environment, or candidate.
//! The on-disk state is create-once and every terminal answer is reconstructed
//! from the complete protected attempt/claim/result/attestation chain.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
#[cfg(windows)]
use std::os::windows::io::FromRawHandle;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
#[cfg(test)]
use ed25519_dalek::{Signer as _, SigningKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[cfg(test)]
use crate::reviewed_source_snapshot::{
    ReviewedSourceBaselineObservationV1, ReviewedSourceCurrentOutputV1,
    create_or_validate_reviewed_source_snapshot,
};
use crate::reviewed_source_snapshot::{
    ReviewedSourceSnapshotExpectedV1, ValidatedReviewedSourceSnapshotV1,
    create_relative_regular_file, open_relative_regular_file, read_relative_regular,
    validate_committed_snapshot, write_new_regular_in,
};
use crate::windows_protected_fs::{
    ProtectedDirectoryGuard, read_optional_relative_regular, validate_protected_component,
    write_unique_regular_for_atomic_replace,
};

pub const REVIEWED_BUILD_WORKER_FLAG: &str = "--catdesk-reviewed-build-worker";
/// Administrator-only fixed-policy provisioning command. It has no values or
/// options: all service/identity/namespace/security authority remains inside
/// the T-0209 administrator host.
pub(crate) const DEDICATED_PRODUCER_ADMIN_PROVISION_FLAG: &str =
    "--catdesk-reviewed-producer-provision-fixed-policy";
/// Administrator-only first-image bootstrap command.  Like the dedicated
/// producer command, it has no value-bearing arguments: image, destination,
/// review evidence, service, and security policy are never CLI authority.
pub(crate) const REVIEWED_MAIN_IMAGE_BOOTSTRAP_INSTALL_FLAG: &str =
    "--catdesk-reviewed-main-image-bootstrap-install-fixed-policy";
/// Administrator-only signed main-image rotation command. It accepts no
/// caller values: the product trust root, transport, destination, staging
/// child, policy, and anti-rollback state are all compiled/fixed.
pub(crate) const REVIEWED_MAIN_IMAGE_ROTATE_FLAG: &str =
    "--catdesk-reviewed-main-image-rotate-fixed-policy";
const ATTEMPT_SCHEMA_VERSION: u8 = 2;
const CLAIM_SCHEMA_VERSION: u8 = 1;
const RESULT_SCHEMA_VERSION: u8 = 1;
const ATTESTATION_SCHEMA_VERSION: u8 = 3;
const BUILD_POLICY_VERSION: &str = "CATDESK_REVIEWED_BUILD_POLICY_V5_OFFLINE_VALIDATED_CARGO_CACHE";
const LEGACY_BUILD_POLICY_V4_VERSION: &str =
    "CATDESK_REVIEWED_BUILD_POLICY_V4_LOCAL_USER_PINNED_OUTPUT";
const LEGACY_BUILD_ENVIRONMENT_POLICY_V4_SHA256: &str =
    "b2d12997dd511c9b1ebcbad1df42881bbdd8c0a62bc790c3fc5b4fbc0b77a940";
// Exact pre-linker V5 environment used by already-terminal protected attempts.
// It is accepted only by historical terminal validation so a freshly reviewed
// source can advance the immutable retry lineage; fresh attempts still use
// `ENVIRONMENT_POLICY` below.
const LEGACY_BUILD_ENVIRONMENT_POLICY_V5_PRE_FIXED_ROOT_LINKER_SHA256: &str =
    "461ed38d8a3a90a9d4b7beb3f5cbe1935f38cd5ab15a799f4fb6299d2dc3af26";
const LEGACY_BUILD_POLICY_V3_VERSION: &str =
    "CATDESK_REVIEWED_BUILD_POLICY_V3_RUSTUP_PROFILE_RESOLVER";
const LEGACY_BUILD_ENVIRONMENT_POLICY_V3_SHA256: &str =
    "69987e87dd71e55250970a152b4b6b3e3b11996e1c76e1a13f110eb688feaad4";
const BUILD_ARGUMENTS: &[&str] = &["build", "--release", "--locked", "--offline"];
const LEGACY_BUILD_ARGUMENTS: &[&str] = &["build", "--release", "--locked"];
const ENVIRONMENT_POLICY: &str = "CATDESK_REVIEWED_BUILD_ENVIRONMENT_V5:clear-inherited-environment;trusted-profile-rustup-discovery;os-system-drive;validated-fixed-root-msvc-sdk-environment;per-attempt-temp;validated-registry-index-and-crate-archives;offline;pinned-output-immediate-remeasurement";
const BUILD_TIMEOUT: Duration = Duration::from_secs(900);
const MAX_STATE_BYTES: usize = 64 * 1024;
const MAX_CANDIDATE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CARGO_LOCK_BYTES: u64 = 4 * 1024 * 1024;
const MAX_CARGO_REGISTRY_CONFIG_BYTES: u64 = 64 * 1024;
// `web-sys` in the current locked closure is 4,378,891 bytes in Cargo's
// sparse index cache. Keep a finite headroom bound while accepting legitimate
// high-version-count entries; protected no-follow/type checks still apply.
const MAX_CARGO_REGISTRY_INDEX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CARGO_REGISTRY_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CARGO_REGISTRY_CRATES: usize = 4096;
const RUSTUP_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RUSTUP_DISCOVERY_OUTPUT: usize = 4096;
// Cargo diagnostics are never authority.  A fixed-size capture is drained so
// the child cannot block on its stderr pipe, then reduced to a fixed
// classification and digest before it is persisted in terminal control state.
// No compiler output, paths, URLs, or environment values are retained. For
// LNK1104/LNK1181 only, one strictly validated library basename may be retained
// so the closed toolchain environment can be repaired without persisting stderr.
const MAX_CARGO_FAILURE_DIAGNOSTIC_BYTES: usize = 4096;
const CARGO_STDERR_CLASSIFIER_TAIL_BYTES: usize = 128;
const MAX_LINK_LIBRARY_BASENAME_BYTES: usize = 96;
#[cfg(windows)]
const CREATE_SUSPENDED: u32 = 0x0000_0004;

/// The only production data-plane child names.  Keeping this closed prevents
/// callers from smuggling a pathname into the reviewed-build authority layer;
/// each is opened below the pinned R7C control parent by `control_*_json`.
const CONTROL_CHILDREN: &[&str] = &[
    "attempt.json",
    "claim.json",
    "worker-owner.json",
    "result.json",
    "attestation.json",
];
const ACTIVE_GENERATION_FILE: &str = "active-generation.json";
const RETRY_HISTORY_DIRECTORY: &str = "terminal-history";
const RETRY_PLAN_DIRECTORY: &str = "retry-plans";
const RETRY_GENERATION_DIRECTORY: &str = "generations";

#[cfg(test)]
type OutputCandidateHook = Box<dyn Fn(&str, &Path) + Send + Sync>;
#[cfg(test)]
static OUTPUT_CANDIDATE_HOOK: OnceLock<Mutex<Option<OutputCandidateHook>>> = OnceLock::new();
#[cfg(test)]
static OUTPUT_CANDIDATE_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(test)]
fn invoke_output_candidate_hook(boundary: &str, parent: &Path) {
    let hook = OUTPUT_CANDIDATE_HOOK.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = hook.lock() {
        if let Some(hook) = guard.as_ref() {
            hook(boundary, parent);
        }
    }
}

#[cfg(not(test))]
fn invoke_output_candidate_hook(_boundary: &str, _parent: &Path) {}

#[cfg(test)]
fn replace_output_candidate_hook(hook: Option<OutputCandidateHook>) {
    let slot = OUTPUT_CANDIDATE_HOOK.get_or_init(|| Mutex::new(None));
    *slot.lock().expect("output candidate hook lock") = hook;
}

#[cfg(test)]
fn output_candidate_test_lock() -> &'static Mutex<()> {
    OUTPUT_CANDIDATE_TEST_LOCK.get_or_init(|| Mutex::new(()))
}

// T-0198 is deliberately a test-only feasibility gate.  These types model the
// validation a future producer-to-CatDesk kernel-handle handoff would need;
// they are never reachable from the reviewed-build worker and cannot turn a
// duplicated handle into production authority.
#[cfg(all(test, windows))]
const PRODUCER_HANDLE_NOT_CAPTURED: &str = "REVIEWED_BUILD_PRODUCER_HANDLE_NOT_CAPTURED";
#[cfg(all(test, windows))]
const PRODUCER_HANDLE_UNSAFE: &str = "REVIEWED_BUILD_PRODUCER_HANDLE_UNSAFE";
#[cfg(all(test, windows))]
const PRODUCER_IDENTITY_MISMATCH: &str = "REVIEWED_BUILD_PRODUCER_IDENTITY_MISMATCH";
#[cfg(all(test, windows))]
const PRODUCER_PROCESS_UNTRUSTED: &str = "REVIEWED_BUILD_PRODUCER_PROCESS_UNTRUSTED";
#[cfg(all(test, windows))]
const PRODUCER_HANDOFF_REJECTED: &str = "REVIEWED_BUILD_PRODUCER_HANDOFF_REJECTED";
#[cfg(all(test, windows))]
const PRODUCER_HANDOFF_REPLAYED: &str = "REVIEWED_BUILD_PRODUCER_HANDOFF_REPLAYED";
#[cfg(all(test, windows))]
const LINKER_UNAVAILABLE: &str = "REVIEWED_BUILD_LINKER_UNAVAILABLE";
#[cfg(all(test, windows))]
const BROKER_HANDOFF_REJECTED: &str = "REVIEWED_BUILD_BROKER_HANDOFF_REJECTED";
#[cfg(all(test, windows))]
const ISOLATION_OWNER_REACQUIRABLE: &str = "REVIEWED_BUILD_ISOLATION_OWNER_REACQUIRABLE";
#[cfg(all(test, windows))]
const ISOLATION_CAPABILITY_REJECTED: &str = "REVIEWED_BUILD_ISOLATION_CAPABILITY_REJECTED";
#[cfg(all(test, windows))]
const ISOLATION_UNPROVEN: &str = "REVIEWED_BUILD_ISOLATION_UNPROVEN";
#[cfg(all(test, windows))]
const APPCONTAINER_LAUNCH_UNAVAILABLE: &str = "REVIEWED_BUILD_APPCONTAINER_LAUNCH_UNAVAILABLE";
#[cfg(all(test, windows))]
const APPCONTAINER_TOKEN_UNPROVEN: &str = "REVIEWED_BUILD_APPCONTAINER_TOKEN_UNPROVEN";
#[cfg(all(test, windows))]
const APPCONTAINER_OUTPUT_UNPROVEN: &str = "REVIEWED_BUILD_APPCONTAINER_OUTPUT_UNPROVEN";
#[cfg(all(test, windows))]
const APPCONTAINER_SECURITY_UNPROVEN: &str = "REVIEWED_BUILD_APPCONTAINER_SECURITY_UNPROVEN";
#[cfg(all(test, windows))]
const APPCONTAINER_FIXED_OUTPUT_CHILD: &str = "catdesk-appcontainer-output.bin";
#[allow(dead_code)]
const DEDICATED_PRODUCER_NOT_PROVISIONED: &str =
    "REVIEWED_BUILD_DEDICATED_PRODUCER_NOT_PROVISIONED";
#[allow(dead_code)]
const DEDICATED_PRODUCER_UNPROVEN: &str = "REVIEWED_BUILD_DEDICATED_PRODUCER_UNPROVEN";
const DEDICATED_PRODUCER_HANDOFF_REJECTED: &str =
    "REVIEWED_BUILD_DEDICATED_PRODUCER_HANDOFF_REJECTED";
#[allow(dead_code)]
const DEDICATED_PRODUCER_PROVISIONING_REFUSED: &str =
    "REVIEWED_BUILD_DEDICATED_PRODUCER_PROVISIONING_REFUSED";
#[allow(dead_code)]
const DEDICATED_PRODUCER_ADMIN_REQUIRED: &str = "REVIEWED_BUILD_DEDICATED_PRODUCER_ADMIN_REQUIRED";
#[allow(dead_code)]
const DEDICATED_PRODUCER_ROLLBACK_UNPROVEN: &str =
    "REVIEWED_BUILD_DEDICATED_PRODUCER_ROLLBACK_UNPROVEN";
#[allow(dead_code)]
const DEDICATED_PRODUCER_MUTATION_HOST_UNAVAILABLE: &str =
    "REVIEWED_BUILD_DEDICATED_PRODUCER_MUTATION_HOST_UNAVAILABLE";

/// The only service name this feasibility detector will inspect. It is a
/// policy constant, not caller input; the detector never creates, starts,
/// reconfigures, or deletes the service/account.
#[allow(dead_code)]
const DEDICATED_PRODUCER_SERVICE_NAME: &str = "CatDeskReviewedProducer";
#[allow(dead_code)]
const DEDICATED_PRODUCER_SERVICE_BINARY: &str =
    r"C:\Program Files\CatDesk\CatDeskReviewedProducer.exe";
/// The reviewed CatDesk image copied by the administrator-only deployment
/// stage.  This is a compiled product policy location, never a CLI, service,
/// environment, or caller selection.
#[allow(dead_code)]
const DEDICATED_PRODUCER_REVIEWED_IMAGE: &str = r"C:\Program Files\CatDesk\CatDesk.exe";
#[allow(dead_code)]
const DEDICATED_PRODUCER_NAMESPACE: &str = r"C:\ProgramData\CatDesk\reviewed-producer";
#[allow(dead_code)]
const DEDICATED_PRODUCER_POLICY_VERSION: &str =
    "CATDESK_DEDICATED_PRODUCER_V3_FIXED_REVIEWED_IMAGE_DEPLOYMENT_BINDING";
#[allow(dead_code)]
pub(crate) const DEDICATED_PRODUCER_SERVICE_MODE: &str = "--catdesk-reviewed-producer-service";
#[allow(dead_code)]
const DEDICATED_PRODUCER_SERVICE_SID_POLICY: &str = "RESTRICTED_SERVICE_SID";
#[allow(dead_code)]
const DEDICATED_PRODUCER_NAMESPACE_SECURITY_POLICY: &str =
    "OWNER_RIGHTS_DENY_INTERACTIVE_MUTATION_MANDATORY_LABEL";
const DEDICATED_PRODUCER_IMAGE_MISSING: &str = "REVIEWED_BUILD_DEDICATED_PRODUCER_IMAGE_MISSING";
const DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED: &str =
    "REVIEWED_BUILD_DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED";
const DEDICATED_PRODUCER_SERVICE_CONFIGURATION_FAILED: &str =
    "REVIEWED_BUILD_DEDICATED_PRODUCER_SERVICE_CONFIGURATION_FAILED";
const DEDICATED_PRODUCER_SERVICE_SID_FAILED: &str =
    "REVIEWED_BUILD_DEDICATED_PRODUCER_SERVICE_SID_FAILED";
const DEDICATED_PRODUCER_NAMESPACE_SECURITY_FAILED: &str =
    "REVIEWED_BUILD_DEDICATED_PRODUCER_NAMESPACE_SECURITY_FAILED";
const DEDICATED_PRODUCER_SERVICE_IPC_UNAVAILABLE: &str =
    "REVIEWED_BUILD_DEDICATED_PRODUCER_SERVICE_IPC_UNAVAILABLE";
const REVIEWED_MAIN_IMAGE_DESTINATION: &str = r"C:\Program Files\CatDesk\CatDesk.exe";
const REVIEWED_MAIN_IMAGE_BOOTSTRAP_POLICY_VERSION: &str =
    "CATDESK_REVIEWED_MAIN_IMAGE_BOOTSTRAP_V1_OPENED_REVIEWED_ARTIFACT_ONLY";
const REVIEWED_MAIN_IMAGE_BOOTSTRAP_AUTHORITY_REQUIRED: &str =
    "REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_AUTHORITY_REQUIRED";
const REVIEWED_MAIN_IMAGE_BOOTSTRAP_REFUSED: &str = "REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_REFUSED";
const REVIEWED_MAIN_IMAGE_BOOTSTRAP_INSTALL_FAILED: &str =
    "REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_INSTALL_FAILED";
const REVIEWED_MAIN_IMAGE_BOOTSTRAP_ROLLBACK_UNPROVEN: &str =
    "REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_ROLLBACK_UNPROVEN";
const REVIEWED_MAIN_IMAGE_BOOTSTRAP_ADMIN_REQUIRED: &str =
    "REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_ADMIN_REQUIRED";
const REVIEWED_MAIN_IMAGE_ROTATION_POLICY_VERSION: &str =
    "CATDESK_REVIEWED_MAIN_IMAGE_ROTATION_V1_MONOTONIC_SIGNED_ATOMIC_REPLACE";
const REVIEWED_MAIN_IMAGE_ROTATION_REFUSED: &str = "REVIEWED_BUILD_MAIN_IMAGE_ROTATION_REFUSED";
#[cfg_attr(test, allow(dead_code))]
const REVIEWED_MAIN_IMAGE_ROTATION_INSTALL_FAILED: &str =
    "REVIEWED_BUILD_MAIN_IMAGE_ROTATION_INSTALL_FAILED";
const REVIEWED_MAIN_IMAGE_ROTATION_ADMIN_REQUIRED: &str =
    "REVIEWED_BUILD_MAIN_IMAGE_ROTATION_ADMIN_REQUIRED";
const REVIEWED_MAIN_IMAGE_TRUST_ROOT_UNPROVISIONED: &str =
    "REVIEWED_BUILD_MAIN_IMAGE_TRUST_ROOT_UNPROVISIONED";
const REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID: &str =
    "REVIEWED_BUILD_MAIN_IMAGE_REVIEW_ENVELOPE_INVALID";
const REVIEWED_MAIN_IMAGE_SIGNATURE_INVALID: &str =
    "REVIEWED_BUILD_MAIN_IMAGE_REVIEW_SIGNATURE_INVALID";
const REVIEWED_MAIN_IMAGE_TRANSPORT_UNAVAILABLE: &str =
    "REVIEWED_BUILD_MAIN_IMAGE_REVIEW_TRANSPORT_UNAVAILABLE";
const REVIEWED_MAIN_IMAGE_PAYLOAD_MISMATCH: &str =
    "REVIEWED_BUILD_MAIN_IMAGE_REVIEW_PAYLOAD_MISMATCH";
const REVIEWED_MAIN_IMAGE_ROLLBACK_REFUSED: &str =
    "REVIEWED_BUILD_MAIN_IMAGE_REVIEW_ROLLBACK_REFUSED";
#[cfg_attr(test, allow(dead_code))]
const REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID: &str =
    "REVIEWED_BUILD_MAIN_IMAGE_REVIEW_ROLLBACK_STATE_INVALID";
const REVIEWED_MAIN_IMAGE_ENVELOPE_MAGIC: &str = "CATDESK_REVIEWED_MAIN_IMAGE_ENVELOPE_V1";
const REVIEWED_MAIN_IMAGE_PRODUCT: &str = "CatDesk";
const REVIEWED_MAIN_IMAGE_PURPOSE: &str = "reviewed-main-image-bootstrap";
// This is a fixed local role binding, not a second envelope, signing key, or
// provenance producer. The already-authenticated main image is permitted to
// run only this one stable-supervisor runtime role under the same principal.
const REVIEWED_STABLE_SUPERVISOR_ROLE: &str = "stable-supervisor-runtime-v1";
const REVIEWED_MAIN_IMAGE_ROTATION_PURPOSE: &str = "reviewed-main-image-rotation";
const REVIEWED_MAIN_IMAGE_TRUST_ROOT_ID: &str = "catdesk-main-image-root";
const REVIEWED_MAIN_IMAGE_TRUST_ROOT_VERSION: u32 = 1;
#[cfg_attr(test, allow(dead_code))]
const REVIEWED_MAIN_IMAGE_INCOMING_PAYLOAD: &str =
    r"C:\ProgramData\CatDesk\reviewed-main-image\incoming\CatDesk.exe";
const REVIEWED_MAIN_IMAGE_INCOMING_ENVELOPE: &str =
    r"C:\ProgramData\CatDesk\reviewed-main-image\incoming\review-envelope.v1";
#[cfg_attr(test, allow(dead_code))]
const REVIEWED_MAIN_IMAGE_ACCEPTED_ENVELOPE: &str =
    r"C:\Program Files\CatDesk.reviewed-main-image-accepted.v1";
#[cfg_attr(test, allow(dead_code))]
const REVIEWED_MAIN_IMAGE_ACCEPTED_ENVELOPE_NEXT: &str =
    r"C:\Program Files\CatDesk.reviewed-main-image-accepted.v1.next";
#[cfg_attr(test, allow(dead_code))]
const REVIEWED_MAIN_IMAGE_ROTATION_INCOMING_PAYLOAD: &str =
    r"C:\ProgramData\CatDesk\reviewed-main-image\rotation\incoming\CatDesk.exe";
const REVIEWED_MAIN_IMAGE_ROTATION_INCOMING_ENVELOPE: &str =
    r"C:\ProgramData\CatDesk\reviewed-main-image\rotation\incoming\review-envelope.v1";
const REVIEWED_MAIN_IMAGE_ROTATION_PENDING_ENVELOPE: &str =
    r"C:\Program Files\CatDesk.reviewed-main-image-rotation-pending.v1";
const REVIEWED_MAIN_IMAGE_ROTATION_INSTALLED_ENVELOPE: &str =
    r"C:\Program Files\CatDesk.reviewed-main-image-rotation-installed.v1";
#[cfg_attr(test, allow(dead_code))]
const REVIEWED_MAIN_IMAGE_ROTATION_INSTALLED_ENVELOPE_NEXT: &str =
    r"C:\Program Files\CatDesk.reviewed-main-image-rotation-installed.v1.next";
const REVIEWED_MAIN_IMAGE_ROTATION_STAGING: &str =
    r"C:\Program Files\CatDesk\CatDesk.rotation-next.exe";
const MAX_REVIEWED_MAIN_IMAGE_ENVELOPE_BYTES: u64 = 4096;

// T-0215 product trust-root seam. The public half of the independently
// controlled offline signing key is compiled into the product; CatDesk never
// generates, loads, or requests the corresponding private key.
const REVIEWED_MAIN_IMAGE_PRODUCTION_PUBLIC_KEY: Option<[u8; 32]> = Some([
    0x3b, 0xc6, 0xa2, 0x10, 0x16, 0x87, 0x81, 0x6d, 0xc8, 0x23, 0x5e, 0xfd, 0xe5, 0x62, 0xdb, 0x93,
    0xdb, 0xd0, 0xad, 0x82, 0x2d, 0x92, 0x22, 0x86, 0x64, 0x0e, 0xc5, 0xc9, 0x85, 0x23, 0x95, 0x45,
]);

/// Fixed lifecycle policy for the later operator-gated dedicated producer.
/// There is deliberately no caller-provided service name, executable, root,
/// SDDL, SCM command, or identity selection.  This type plans/proves only;
/// production reviewed-build authority never invokes it.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct DedicatedProducerProvisioningPolicyV1 {
    service_name: &'static str,
    reviewed_service_image: &'static str,
    service_binary: &'static str,
    namespace: &'static str,
    policy_sha256: String,
    service_mode: &'static str,
    service_sid_policy: &'static str,
    namespace_security_policy: &'static str,
    denied_interactive_rights: &'static [&'static str],
}

#[allow(dead_code)]
fn dedicated_producer_fixed_policy_digest() -> String {
    // The deployment source and destination are authority-bearing policy, not
    // merely documentation. Bind every fixed service/namespace selector into
    // the durable journal digest so an old or mixed image plan cannot compare
    // equal across policy generations.
    sha256(&format!(
        "{}|{}|{}|{}|{}|{}|{}",
        DEDICATED_PRODUCER_POLICY_VERSION,
        DEDICATED_PRODUCER_SERVICE_NAME,
        DEDICATED_PRODUCER_REVIEWED_IMAGE,
        DEDICATED_PRODUCER_SERVICE_BINARY,
        DEDICATED_PRODUCER_SERVICE_MODE,
        DEDICATED_PRODUCER_NAMESPACE,
        DEDICATED_PRODUCER_SERVICE_SID_POLICY,
    ))
}

#[allow(dead_code)]
fn dedicated_producer_fixed_policy() -> DedicatedProducerProvisioningPolicyV1 {
    DedicatedProducerProvisioningPolicyV1 {
        service_name: DEDICATED_PRODUCER_SERVICE_NAME,
        reviewed_service_image: DEDICATED_PRODUCER_REVIEWED_IMAGE,
        service_binary: DEDICATED_PRODUCER_SERVICE_BINARY,
        namespace: DEDICATED_PRODUCER_NAMESPACE,
        policy_sha256: dedicated_producer_fixed_policy_digest(),
        service_mode: DEDICATED_PRODUCER_SERVICE_MODE,
        service_sid_policy: DEDICATED_PRODUCER_SERVICE_SID_POLICY,
        namespace_security_policy: DEDICATED_PRODUCER_NAMESPACE_SECURITY_POLICY,
        denied_interactive_rights: &[
            "FILE_WRITE_DATA",
            "FILE_APPEND_DATA",
            "FILE_ADD_FILE",
            "FILE_ADD_SUBDIRECTORY",
            "DELETE",
            "PARENT_DELETE_CHILD",
            "RENAME_REPARSE",
            "WRITE_DAC",
            "WRITE_OWNER",
            "OWNER_RIGHTS_RECOVERY",
            "TAKE_OWNERSHIP_RECOVERY",
        ],
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
enum DedicatedProducerExistingState {
    Absent,
    ExactProvisionedButLiveAcceptancePending,
    Partial,
    Ambiguous,
    InteractiveRecoveryPossible,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
enum DedicatedProducerProvisioningPlan {
    ReadOnlyAbsent(DedicatedProducerProvisioningPolicyV1),
    ReadOnlyExistingPendingAcceptance(DedicatedProducerProvisioningPolicyV1),
}

/// The lifecycle is deliberately abstracted so the administrator-only Windows
/// host and deterministic tests share the exact fixed-policy state machine.
/// No caller can inject service, account, executable, namespace, SDDL, SCM, or
/// token authority through this boundary.
#[allow(dead_code)]
trait DedicatedProducerProvisioningBackend {
    fn inspect_fixed_state(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<DedicatedProducerExistingState, String>;
    fn administrator_gate(&mut self) -> Result<bool, String>;
    fn provision_exact_fixed_boundary(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String>;
    fn rollback_exact_fixed_boundary(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String>;
}

#[allow(dead_code)]
fn dedicated_producer_read_only_preflight(
    backend: &mut impl DedicatedProducerProvisioningBackend,
) -> Result<DedicatedProducerProvisioningPlan, String> {
    let policy = dedicated_producer_fixed_policy();
    match backend.inspect_fixed_state(&policy)? {
        DedicatedProducerExistingState::Absent => {
            Ok(DedicatedProducerProvisioningPlan::ReadOnlyAbsent(policy))
        }
        DedicatedProducerExistingState::ExactProvisionedButLiveAcceptancePending => {
            Ok(DedicatedProducerProvisioningPlan::ReadOnlyExistingPendingAcceptance(policy))
        }
        DedicatedProducerExistingState::Partial
        | DedicatedProducerExistingState::Ambiguous
        | DedicatedProducerExistingState::InteractiveRecoveryPossible => {
            Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into())
        }
    }
}

/// Future operator execution must explicitly use the plan returned by the
/// read-only preflight and pass the backend's administrator gate.  Even a
/// successful fake/real provisioning result is only *pending live security
/// acceptance*, never reviewed-build output/candidate/attestation authority.
#[allow(dead_code)]
fn execute_dedicated_producer_fixed_plan(
    backend: &mut impl DedicatedProducerProvisioningBackend,
    plan: &DedicatedProducerProvisioningPlan,
) -> Result<DedicatedProducerProvisioningPlan, String> {
    let policy = dedicated_producer_fixed_policy();
    let DedicatedProducerProvisioningPlan::ReadOnlyAbsent(planned_policy) = plan else {
        return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
    };
    if planned_policy != &policy {
        return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
    }
    if !backend.administrator_gate()? {
        return Err(DEDICATED_PRODUCER_ADMIN_REQUIRED.into());
    }
    if let Err(stage_error) = backend.provision_exact_fixed_boundary(&policy) {
        let rollback_state = backend.rollback_exact_fixed_boundary(&policy);
        if rollback_state.is_err()
            || backend.inspect_fixed_state(&policy)? != DedicatedProducerExistingState::Absent
        {
            return Err(DEDICATED_PRODUCER_ROLLBACK_UNPROVEN.into());
        }
        return Err(stage_error);
    }
    match backend.inspect_fixed_state(&policy)? {
        DedicatedProducerExistingState::ExactProvisionedButLiveAcceptancePending => {
            Ok(DedicatedProducerProvisioningPlan::ReadOnlyExistingPendingAcceptance(policy))
        }
        DedicatedProducerExistingState::Absent => {
            Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into())
        }
        DedicatedProducerExistingState::Partial
        | DedicatedProducerExistingState::Ambiguous
        | DedicatedProducerExistingState::InteractiveRecoveryPossible => {
            let rollback = backend.rollback_exact_fixed_boundary(&policy);
            if rollback.is_err()
                || backend.inspect_fixed_state(&policy)? != DedicatedProducerExistingState::Absent
            {
                Err(DEDICATED_PRODUCER_ROLLBACK_UNPROVEN.into())
            } else {
                Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into())
            }
        }
    }
}

/// These are the only mutating stages a future administrator-gated Windows
/// adapter may perform. They are journaled before the next stage begins and
/// rolled back in reverse order. The reviewed-build worker never owns this
/// journal and never treats it as output authority.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DedicatedProducerProvisioningStage {
    ImageDeployed,
    ServiceCreated,
    ServiceSidRestricted,
    NamespaceCreated,
    NamespaceSecurityApplied,
}

/// Injectable Windows operation boundary. Every method receives the compiled
/// policy, never caller-provided names, paths, credentials, command lines,
/// SDDL, or SCM options. A real operator adapter may implement these exact
/// calls; tests provide a fake and routine application paths construct none.
#[allow(dead_code)]
trait DedicatedProducerWindowsOperations {
    fn inspect_fixed_state(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<DedicatedProducerExistingState, String>;
    fn current_token_is_administrator(&mut self) -> Result<bool, String>;
    fn deploy_exact_reviewed_service_image(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String>;
    fn create_exact_service(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String>;
    fn restrict_exact_service_sid(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String>;
    fn create_exact_namespace(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String>;
    fn apply_exact_namespace_security_at_creation(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String>;
    fn verify_interactive_token_cannot_recover_namespace(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String>;
    fn rollback_exact_stage(
        &mut self,
        stage: DedicatedProducerProvisioningStage,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String>;
    /// Persist only a product-owned policy digest and completed stage.  The
    /// journal is an execution/recovery record, never reviewed-build output
    /// authority.
    fn record_durable_stage(
        &mut self,
        stage: DedicatedProducerProvisioningStage,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String>;
    fn clear_durable_journal(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String>;
}

/// Production-shaped fixed-policy backend. The ordinary surface provides only
/// read-only inspection; a separately constructed administrator host owns the
/// fixed mutation adapter. Its journal is bounded and ambiguity is refused.
#[allow(dead_code)]
struct WindowsDedicatedProducerProvisioningBackend<O> {
    operations: O,
    journal: Vec<DedicatedProducerProvisioningStage>,
}

#[allow(dead_code)]
impl<O: DedicatedProducerWindowsOperations> WindowsDedicatedProducerProvisioningBackend<O> {
    fn new(operations: O) -> Self {
        Self {
            operations,
            journal: Vec::new(),
        }
    }

    fn record_stage(&mut self, stage: DedicatedProducerProvisioningStage) {
        self.journal.push(stage);
    }
}

impl<O: DedicatedProducerWindowsOperations> DedicatedProducerProvisioningBackend
    for WindowsDedicatedProducerProvisioningBackend<O>
{
    fn inspect_fixed_state(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<DedicatedProducerExistingState, String> {
        self.operations.inspect_fixed_state(policy)
    }

    fn administrator_gate(&mut self) -> Result<bool, String> {
        self.operations.current_token_is_administrator()
    }

    fn provision_exact_fixed_boundary(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String> {
        if self.operations.inspect_fixed_state(policy)? != DedicatedProducerExistingState::Absent {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        self.operations
            .deploy_exact_reviewed_service_image(policy)
            .map_err(|error| {
                if error == DEDICATED_PRODUCER_IMAGE_MISSING {
                    error
                } else {
                    DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED.to_string()
                }
            })?;
        self.record_stage(DedicatedProducerProvisioningStage::ImageDeployed);
        self.operations
            .record_durable_stage(DedicatedProducerProvisioningStage::ImageDeployed, policy)?;
        self.operations
            .create_exact_service(policy)
            .map_err(|_| DEDICATED_PRODUCER_SERVICE_CONFIGURATION_FAILED.to_string())?;
        self.record_stage(DedicatedProducerProvisioningStage::ServiceCreated);
        self.operations
            .record_durable_stage(DedicatedProducerProvisioningStage::ServiceCreated, policy)?;
        self.operations
            .restrict_exact_service_sid(policy)
            .map_err(|_| DEDICATED_PRODUCER_SERVICE_SID_FAILED.to_string())?;
        self.record_stage(DedicatedProducerProvisioningStage::ServiceSidRestricted);
        self.operations.record_durable_stage(
            DedicatedProducerProvisioningStage::ServiceSidRestricted,
            policy,
        )?;
        self.operations
            .create_exact_namespace(policy)
            .map_err(|_| DEDICATED_PRODUCER_NAMESPACE_SECURITY_FAILED.to_string())?;
        self.record_stage(DedicatedProducerProvisioningStage::NamespaceCreated);
        self.operations
            .record_durable_stage(DedicatedProducerProvisioningStage::NamespaceCreated, policy)?;
        self.operations
            .apply_exact_namespace_security_at_creation(policy)
            .map_err(|_| DEDICATED_PRODUCER_NAMESPACE_SECURITY_FAILED.to_string())?;
        self.record_stage(DedicatedProducerProvisioningStage::NamespaceSecurityApplied);
        self.operations.record_durable_stage(
            DedicatedProducerProvisioningStage::NamespaceSecurityApplied,
            policy,
        )?;
        self.operations
            .verify_interactive_token_cannot_recover_namespace(policy)
            .map_err(|_| DEDICATED_PRODUCER_NAMESPACE_SECURITY_FAILED.to_string())
    }

    fn rollback_exact_fixed_boundary(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String> {
        while let Some(stage) = self.journal.pop() {
            self.operations.rollback_exact_stage(stage, policy)?;
        }
        self.operations.clear_durable_journal(policy)?;
        Ok(())
    }
}

/// Narrow operator request surface. It carries no authority-bearing input.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DedicatedProducerOperatorRequest {
    ReadOnlyPreflight,
    ExecuteFixedPolicy,
}

/// The separately approved mutation host has exactly one request and no
/// authority-bearing payload. Keeping it distinct from the normal read-only
/// request makes accidental autonomous execution structurally impossible.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DedicatedProducerAdministratorRequest {
    ExecuteFixedPolicy,
}

/// Bounded result for an operator UI/CLI that is added only by a later surface
/// ticket. `ActionRequired` never says the OS boundary is accepted.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
enum DedicatedProducerOperatorOutcome {
    NotProvisioned,
    ActionRequired,
}

#[allow(dead_code)]
fn dedicated_producer_operator_operation(
    backend: &mut impl DedicatedProducerProvisioningBackend,
    request: DedicatedProducerOperatorRequest,
) -> Result<DedicatedProducerOperatorOutcome, String> {
    let plan = dedicated_producer_read_only_preflight(backend)?;
    match (request, plan) {
        (
            DedicatedProducerOperatorRequest::ReadOnlyPreflight,
            DedicatedProducerProvisioningPlan::ReadOnlyAbsent(_),
        ) => Ok(DedicatedProducerOperatorOutcome::NotProvisioned),
        (
            DedicatedProducerOperatorRequest::ReadOnlyPreflight,
            DedicatedProducerProvisioningPlan::ReadOnlyExistingPendingAcceptance(_),
        ) => Ok(DedicatedProducerOperatorOutcome::ActionRequired),
        (DedicatedProducerOperatorRequest::ExecuteFixedPolicy, plan) => {
            execute_dedicated_producer_fixed_plan(backend, &plan)?;
            Ok(DedicatedProducerOperatorOutcome::ActionRequired)
        }
    }
}

/// Concrete Windows adapter for the fixed policy.  It has no public caller
/// input and is intentionally constructed only by the dedicated operator
/// surface below.  Ordinary autonomous code never invokes ExecuteFixedPolicy.
#[cfg(windows)]
#[allow(dead_code)]
struct SystemWindowsDedicatedProducerOperations;

#[cfg(windows)]
#[allow(dead_code)]
impl SystemWindowsDedicatedProducerOperations {
    fn wide(value: &str) -> Vec<u16> {
        OsString::from(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn expected_binary_path(policy: &DedicatedProducerProvisioningPolicyV1) -> String {
        format!("\"{}\" {}", policy.service_binary, policy.service_mode)
    }

    fn fixed_journal_path(
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<PathBuf, String> {
        let parent = Path::new(policy.namespace)
            .parent()
            .ok_or_else(|| DEDICATED_PRODUCER_PROVISIONING_REFUSED.to_string())?;
        Ok(parent.join("reviewed-producer.fixed-policy.journal"))
    }

    fn assert_fixed_service_binary_is_safe(
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String> {
        let metadata = fs::symlink_metadata(policy.service_binary)
            .map_err(|_| DEDICATED_PRODUCER_PROVISIONING_REFUSED.to_string())?;
        if !metadata.file_type().is_file()
            || metadata.file_type().is_symlink()
            || metadata.file_attributes() & 0x0400 != 0
        {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        let canonical = fs::canonicalize(policy.service_binary)
            .map(crate::command::normalize_windows_verbatim_path)
            .map_err(|_| DEDICATED_PRODUCER_PROVISIONING_REFUSED.to_string())?;
        let expected =
            crate::command::normalize_windows_verbatim_path(PathBuf::from(policy.service_binary));
        if !canonical
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected.to_string_lossy())
        {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        Ok(())
    }

    fn sid_to_string(sid: *mut c_void) -> Result<String, String> {
        unsafe extern "system" {
            fn ConvertSidToStringSidW(sid: *mut c_void, value: *mut *mut u16) -> i32;
            fn LocalFree(memory: *mut c_void) -> *mut c_void;
        }
        let mut value = std::ptr::null_mut();
        if sid.is_null()
            || unsafe { ConvertSidToStringSidW(sid, &mut value) } == 0
            || value.is_null()
        {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        let result = unsafe {
            let mut count = 0usize;
            while *value.add(count) != 0 && count < 1024 {
                count += 1;
            }
            OsString::from_wide(std::slice::from_raw_parts(value, count))
                .to_string_lossy()
                .into_owned()
        };
        unsafe { LocalFree(value.cast()) };
        if result.is_empty() || result.len() > 512 || result.contains('\0') {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        Ok(result)
    }

    fn current_token_user_sid() -> Result<String, String> {
        const TOKEN_QUERY: u32 = 0x0008;
        const TOKEN_USER: u32 = 1;
        #[repr(C)]
        struct TokenUser {
            user: SidAndAttributes,
        }
        #[repr(C)]
        struct SidAndAttributes {
            sid: *mut c_void,
            attributes: u32,
        }
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut c_void;
            fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
            fn GetTokenInformation(
                token: *mut c_void,
                class: u32,
                value: *mut c_void,
                length: u32,
                required: *mut u32,
            ) -> i32;
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        let mut token = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0
            || token.is_null()
        {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        let result = (|| {
            let mut required = 0u32;
            unsafe {
                GetTokenInformation(token, TOKEN_USER, std::ptr::null_mut(), 0, &mut required)
            };
            if required < std::mem::size_of::<TokenUser>() as u32 || required > 64 * 1024 {
                return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
            }
            let mut bytes = vec![0u8; required as usize];
            if unsafe {
                GetTokenInformation(
                    token,
                    TOKEN_USER,
                    bytes.as_mut_ptr().cast(),
                    required,
                    &mut required,
                )
            } == 0
            {
                return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
            }
            let user = unsafe { &*(bytes.as_ptr().cast::<TokenUser>()) };
            Self::sid_to_string(user.user.sid)
        })();
        unsafe { CloseHandle(token) };
        result
    }

    fn fixed_service_sid() -> Result<String, String> {
        const SID_NAME_USE_UNKNOWN: u32 = 8;
        unsafe extern "system" {
            fn LookupAccountNameW(
                system: *const u16,
                account: *const u16,
                sid: *mut c_void,
                sid_size: *mut u32,
                domain: *mut u16,
                domain_size: *mut u32,
                use_type: *mut u32,
            ) -> i32;
        }
        let account = Self::wide("NT SERVICE\\CatDeskReviewedProducer");
        let mut sid_size = 0u32;
        let mut domain_size = 0u32;
        let mut use_type = SID_NAME_USE_UNKNOWN;
        unsafe {
            LookupAccountNameW(
                std::ptr::null(),
                account.as_ptr(),
                std::ptr::null_mut(),
                &mut sid_size,
                std::ptr::null_mut(),
                &mut domain_size,
                &mut use_type,
            )
        };
        if sid_size == 0 || sid_size > 4096 || domain_size > 4096 {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        let mut sid = vec![0u8; sid_size as usize];
        let mut domain = vec![0u16; domain_size.max(1) as usize];
        if unsafe {
            LookupAccountNameW(
                std::ptr::null(),
                account.as_ptr(),
                sid.as_mut_ptr().cast(),
                &mut sid_size,
                domain.as_mut_ptr(),
                &mut domain_size,
                &mut use_type,
            )
        } == 0
        {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        Self::sid_to_string(sid.as_mut_ptr().cast())
    }

    /// The descriptor is fixed by product policy.  `SY` owns the root, OWNER
    /// RIGHTS and INTERACTIVE are denied every mutating namespace/owner right,
    /// the exact service SID has only directory/file creation rights, and the
    /// current CatDesk token gets read/control only.  A high-integrity no-write
    /// mandatory label is an additional guard, not a substitute for the DACL.
    fn fixed_namespace_sddl() -> Result<String, String> {
        let service_sid = Self::fixed_service_sid()?;
        let catdesk_sid = Self::current_token_user_sid()?;
        Ok(format!(
            "O:SYG:SYD:PAI(D;OICI;0x000D0166;;;IU)(D;OICI;0x000D0166;;;OW)(A;OICI;0x001F01FF;;;SY)(A;OICI;0x0012019F;;;{service_sid})(A;OICI;0x00120089;;;{catdesk_sid})S:(ML;OICI;NW;;;HI)"
        ))
    }
}

#[cfg(windows)]
impl DedicatedProducerWindowsOperations for SystemWindowsDedicatedProducerOperations {
    fn inspect_fixed_state(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<DedicatedProducerExistingState, String> {
        const SC_MANAGER_CONNECT: u32 = 0x0001;
        const SERVICE_QUERY_CONFIG: u32 = 0x0001;
        const SERVICE_QUERY_STATUS: u32 = 0x0004;
        const READ_CONTROL: u32 = 0x0002_0000;
        const INVALID_FILE_ATTRIBUTES: u32 = u32::MAX;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        const SERVICE_CONFIG_SERVICE_SID_INFO: u32 = 5;
        const SERVICE_SID_TYPE_RESTRICTED: u32 = 3;
        const OWNER_SECURITY_INFORMATION: u32 = 0x0000_0001;
        const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
        const LABEL_SECURITY_INFORMATION: u32 = 0x0000_0010;
        #[repr(C)]
        struct QueryServiceConfigW {
            service_type: u32,
            start_type: u32,
            error_control: u32,
            binary_path: *mut u16,
            load_order_group: *mut u16,
            tag_id: u32,
            dependencies: *mut u16,
            service_start_name: *mut u16,
            display_name: *mut u16,
        }
        #[repr(C)]
        struct ServiceSidInfo {
            service_sid_type: u32,
        }
        unsafe extern "system" {
            fn OpenSCManagerW(
                machine: *const u16,
                database: *const u16,
                access: u32,
            ) -> *mut c_void;
            fn OpenServiceW(manager: *mut c_void, name: *const u16, access: u32) -> *mut c_void;
            fn QueryServiceConfigW(
                service: *mut c_void,
                config: *mut QueryServiceConfigW,
                size: u32,
                needed: *mut u32,
            ) -> i32;
            fn QueryServiceConfig2W(
                service: *mut c_void,
                level: u32,
                buffer: *mut u8,
                size: u32,
                needed: *mut u32,
            ) -> i32;
            fn CloseServiceHandle(handle: *mut c_void) -> i32;
            fn GetFileAttributesW(path: *const u16) -> u32;
            fn GetFileSecurityW(
                path: *const u16,
                requested: u32,
                descriptor: *mut c_void,
                length: u32,
                needed: *mut u32,
            ) -> i32;
        }
        let root = Self::wide(policy.namespace);
        let attributes = unsafe { GetFileAttributesW(root.as_ptr()) };
        let namespace_absent = attributes == INVALID_FILE_ATTRIBUTES;
        let image_absent = match fs::symlink_metadata(policy.service_binary) {
            Ok(_) => false,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(_) => return Ok(DedicatedProducerExistingState::Ambiguous),
        };
        let manager =
            unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
        if manager.is_null() {
            return Ok(if namespace_absent && image_absent {
                DedicatedProducerExistingState::Absent
            } else {
                DedicatedProducerExistingState::Partial
            });
        }
        let service_name = Self::wide(policy.service_name);
        let service = unsafe {
            OpenServiceW(
                manager,
                service_name.as_ptr(),
                SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS | READ_CONTROL,
            )
        };
        unsafe { CloseServiceHandle(manager) };
        if service.is_null() {
            return Ok(if namespace_absent && image_absent {
                DedicatedProducerExistingState::Absent
            } else {
                DedicatedProducerExistingState::Partial
            });
        }
        let result = (|| {
            let mut needed = 0u32;
            unsafe { QueryServiceConfigW(service, std::ptr::null_mut(), 0, &mut needed) };
            if needed == 0 || needed > 64 * 1024 {
                return Ok(DedicatedProducerExistingState::Ambiguous);
            }
            let mut bytes = vec![0u8; needed as usize];
            let config = bytes.as_mut_ptr().cast::<QueryServiceConfigW>();
            if unsafe { QueryServiceConfigW(service, config, needed, &mut needed) } == 0
                || unsafe { (*config).binary_path.is_null() }
            {
                return Ok(DedicatedProducerExistingState::Ambiguous);
            }
            let path = unsafe {
                std::ffi::OsString::from_wide({
                    let mut count = 0usize;
                    while *(*config).binary_path.add(count) != 0 && count < 4096 {
                        count += 1;
                    }
                    std::slice::from_raw_parts((*config).binary_path, count)
                })
            }
            .to_string_lossy()
            .into_owned();
            if path != Self::expected_binary_path(policy)
                || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
                || namespace_absent
                || Self::assert_fixed_service_binary_is_safe(policy).is_err()
            {
                return Ok(DedicatedProducerExistingState::Ambiguous);
            }
            let mut sid_info = ServiceSidInfo {
                service_sid_type: 0,
            };
            let mut sid_needed = 0u32;
            if unsafe {
                QueryServiceConfig2W(
                    service,
                    SERVICE_CONFIG_SERVICE_SID_INFO,
                    (&mut sid_info as *mut ServiceSidInfo).cast(),
                    std::mem::size_of::<ServiceSidInfo>() as u32,
                    &mut sid_needed,
                )
            } == 0
                || sid_info.service_sid_type != SERVICE_SID_TYPE_RESTRICTED
            {
                return Ok(DedicatedProducerExistingState::Ambiguous);
            }
            let mut security_needed = 0u32;
            unsafe {
                GetFileSecurityW(
                    root.as_ptr(),
                    OWNER_SECURITY_INFORMATION
                        | DACL_SECURITY_INFORMATION
                        | LABEL_SECURITY_INFORMATION,
                    std::ptr::null_mut(),
                    0,
                    &mut security_needed,
                )
            };
            if security_needed == 0 || security_needed > 64 * 1024 {
                return Ok(DedicatedProducerExistingState::Ambiguous);
            }
            let mut security = vec![0u8; security_needed as usize];
            if unsafe {
                GetFileSecurityW(
                    root.as_ptr(),
                    OWNER_SECURITY_INFORMATION
                        | DACL_SECURITY_INFORMATION
                        | LABEL_SECURITY_INFORMATION,
                    security.as_mut_ptr().cast(),
                    security_needed,
                    &mut security_needed,
                )
            } == 0
            {
                return Ok(DedicatedProducerExistingState::Ambiguous);
            }
            Ok(DedicatedProducerExistingState::ExactProvisionedButLiveAcceptancePending)
        })();
        unsafe { CloseServiceHandle(service) };
        result
    }

    fn current_token_is_administrator(&mut self) -> Result<bool, String> {
        const TOKEN_QUERY: u32 = 0x0008;
        const TOKEN_ELEVATION: u32 = 20;
        #[repr(C)]
        struct TokenElevation {
            elevated: u32,
        }
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut c_void;
            fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
            fn GetTokenInformation(
                token: *mut c_void,
                class: u32,
                value: *mut c_void,
                length: u32,
                required: *mut u32,
            ) -> i32;
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        let mut token = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0
            || token.is_null()
        {
            return Err(DEDICATED_PRODUCER_ADMIN_REQUIRED.into());
        }
        let mut elevation = TokenElevation { elevated: 0 };
        let mut returned = 0u32;
        let ok = unsafe {
            GetTokenInformation(
                token,
                TOKEN_ELEVATION,
                (&mut elevation as *mut TokenElevation).cast(),
                std::mem::size_of::<TokenElevation>() as u32,
                &mut returned,
            )
        };
        unsafe { CloseHandle(token) };
        if ok == 0 || returned != std::mem::size_of::<TokenElevation>() as u32 {
            return Err(DEDICATED_PRODUCER_ADMIN_REQUIRED.into());
        }
        Ok(elevation.elevated == 1)
    }

    fn deploy_exact_reviewed_service_image(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String> {
        const INVALID_FILE_ATTRIBUTES: u32 = u32::MAX;
        const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        unsafe extern "system" {
            fn CopyFileW(source: *const u16, destination: *const u16, fail_if_exists: i32) -> i32;
            fn GetFileAttributesW(path: *const u16) -> u32;
        }
        let source_metadata =
            fs::symlink_metadata(policy.reviewed_service_image).map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    DEDICATED_PRODUCER_IMAGE_MISSING.to_string()
                } else {
                    DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED.to_string()
                }
            })?;
        if !source_metadata.file_type().is_file() || source_metadata.file_type().is_symlink() {
            return Err(DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED.into());
        }
        let source = fs::canonicalize(policy.reviewed_service_image)
            .map(crate::command::normalize_windows_verbatim_path)
            .map_err(|_| DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED.to_string())?;
        let expected = crate::command::normalize_windows_verbatim_path(PathBuf::from(
            policy.reviewed_service_image,
        ));
        if !source
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected.to_string_lossy())
        {
            return Err(DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED.into());
        }
        let destination_parent = Path::new(policy.service_binary)
            .parent()
            .ok_or_else(|| DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED.to_string())?;
        let parent = Self::wide(&destination_parent.to_string_lossy());
        let attributes = unsafe { GetFileAttributesW(parent.as_ptr()) };
        if attributes == INVALID_FILE_ATTRIBUTES
            || attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)
                != FILE_ATTRIBUTE_DIRECTORY
            || fs::symlink_metadata(policy.service_binary).is_ok()
        {
            return Err(DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED.into());
        }
        let destination = Self::wide(policy.service_binary);
        let source = Self::wide(policy.reviewed_service_image);
        if unsafe { CopyFileW(source.as_ptr(), destination.as_ptr(), 1) } == 0 {
            return Err(DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED.into());
        }
        Self::assert_fixed_service_binary_is_safe(policy)
            .map_err(|_| DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED.to_string())
    }

    fn create_exact_service(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String> {
        const SC_MANAGER_CONNECT: u32 = 0x0001;
        const SC_MANAGER_CREATE_SERVICE: u32 = 0x0002;
        const SERVICE_CHANGE_CONFIG: u32 = 0x0002;
        const DELETE: u32 = 0x0001_0000;
        const READ_CONTROL: u32 = 0x0002_0000;
        const SERVICE_WIN32_OWN_PROCESS: u32 = 0x0010;
        const SERVICE_DEMAND_START: u32 = 0x0003;
        const SERVICE_ERROR_NORMAL: u32 = 0x0001;
        unsafe extern "system" {
            fn OpenSCManagerW(
                machine: *const u16,
                database: *const u16,
                access: u32,
            ) -> *mut c_void;
            fn CreateServiceW(
                manager: *mut c_void,
                service_name: *const u16,
                display_name: *const u16,
                desired_access: u32,
                service_type: u32,
                start_type: u32,
                error_control: u32,
                binary_path: *const u16,
                load_order_group: *const u16,
                tag_id: *mut u32,
                dependencies: *const u16,
                service_start_name: *const u16,
                password: *const u16,
            ) -> *mut c_void;
            fn CloseServiceHandle(handle: *mut c_void) -> i32;
        }
        Self::assert_fixed_service_binary_is_safe(policy)?;
        let manager = unsafe {
            OpenSCManagerW(
                std::ptr::null(),
                std::ptr::null(),
                SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE,
            )
        };
        if manager.is_null() {
            return Err(DEDICATED_PRODUCER_ADMIN_REQUIRED.into());
        }
        let name = Self::wide(policy.service_name);
        let binary = Self::wide(&Self::expected_binary_path(policy));
        let service = unsafe {
            CreateServiceW(
                manager,
                name.as_ptr(),
                name.as_ptr(),
                SERVICE_CHANGE_CONFIG | DELETE | READ_CONTROL,
                SERVICE_WIN32_OWN_PROCESS,
                SERVICE_DEMAND_START,
                SERVICE_ERROR_NORMAL,
                binary.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        unsafe { CloseServiceHandle(manager) };
        if service.is_null() {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        unsafe { CloseServiceHandle(service) };
        Ok(())
    }

    fn restrict_exact_service_sid(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String> {
        const SC_MANAGER_CONNECT: u32 = 0x0001;
        const SERVICE_CHANGE_CONFIG: u32 = 0x0002;
        const SERVICE_CONFIG_SERVICE_SID_INFO: u32 = 5;
        const SERVICE_SID_TYPE_RESTRICTED: u32 = 3;
        #[repr(C)]
        struct ServiceSidInfo {
            service_sid_type: u32,
        }
        unsafe extern "system" {
            fn OpenSCManagerW(
                machine: *const u16,
                database: *const u16,
                access: u32,
            ) -> *mut c_void;
            fn OpenServiceW(manager: *mut c_void, name: *const u16, access: u32) -> *mut c_void;
            fn ChangeServiceConfig2W(service: *mut c_void, level: u32, info: *mut c_void) -> i32;
            fn CloseServiceHandle(handle: *mut c_void) -> i32;
        }
        let manager =
            unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
        if manager.is_null() {
            return Err(DEDICATED_PRODUCER_ADMIN_REQUIRED.into());
        }
        let name = Self::wide(policy.service_name);
        let service = unsafe { OpenServiceW(manager, name.as_ptr(), SERVICE_CHANGE_CONFIG) };
        unsafe { CloseServiceHandle(manager) };
        if service.is_null() {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        let mut sid = ServiceSidInfo {
            service_sid_type: SERVICE_SID_TYPE_RESTRICTED,
        };
        let ok = unsafe {
            ChangeServiceConfig2W(
                service,
                SERVICE_CONFIG_SERVICE_SID_INFO,
                (&mut sid as *mut ServiceSidInfo).cast(),
            )
        };
        unsafe { CloseServiceHandle(service) };
        if ok == 0 {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        // Lookup is an exact identity proof for the fixed service name, not a
        // caller-selected account discovery mechanism.
        Self::fixed_service_sid().map(|_| ())
    }

    fn create_exact_namespace(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String> {
        #[repr(C)]
        struct SecurityAttributes {
            length: u32,
            descriptor: *mut c_void,
            inherit: i32,
        }
        unsafe extern "system" {
            fn CreateDirectoryW(path: *const u16, attributes: *mut SecurityAttributes) -> i32;
            fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
                definition: *const u16,
                revision: u32,
                descriptor: *mut *mut c_void,
                size: *mut u32,
            ) -> i32;
            fn LocalFree(memory: *mut c_void) -> *mut c_void;
            fn GetFileAttributesW(path: *const u16) -> u32;
        }
        const INVALID_FILE_ATTRIBUTES: u32 = u32::MAX;
        const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        let root = Self::wide(policy.namespace);
        let parent = Path::new(policy.namespace)
            .parent()
            .ok_or_else(|| DEDICATED_PRODUCER_PROVISIONING_REFUSED.to_string())?;
        let parent_wide = Self::wide(&parent.to_string_lossy());
        let parent_attributes = unsafe { GetFileAttributesW(parent_wide.as_ptr()) };
        if parent_attributes == INVALID_FILE_ATTRIBUTES
            || parent_attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)
                != FILE_ATTRIBUTE_DIRECTORY
        {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        if unsafe { GetFileAttributesW(root.as_ptr()) } != INVALID_FILE_ATTRIBUTES {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        let sddl = Self::fixed_namespace_sddl()?;
        let sddl_wide = Self::wide(&sddl);
        let mut descriptor = std::ptr::null_mut();
        let mut descriptor_size = 0u32;
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_wide.as_ptr(),
                1,
                &mut descriptor,
                &mut descriptor_size,
            )
        } == 0
            || descriptor.is_null()
            || descriptor_size == 0
            || descriptor_size > 64 * 1024
        {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        let mut attributes = SecurityAttributes {
            length: std::mem::size_of::<SecurityAttributes>() as u32,
            descriptor,
            inherit: 0,
        };
        let created = unsafe { CreateDirectoryW(root.as_ptr(), &mut attributes) };
        unsafe { LocalFree(descriptor) };
        if created == 0 {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        Ok(())
    }

    fn apply_exact_namespace_security_at_creation(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String> {
        const OWNER_SECURITY_INFORMATION: u32 = 0x0000_0001;
        const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
        const LABEL_SECURITY_INFORMATION: u32 = 0x0000_0010;
        unsafe extern "system" {
            fn GetFileSecurityW(
                path: *const u16,
                requested: u32,
                descriptor: *mut c_void,
                length: u32,
                needed: *mut u32,
            ) -> i32;
        }
        let path = Self::wide(policy.namespace);
        let mut required = 0u32;
        unsafe {
            GetFileSecurityW(
                path.as_ptr(),
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                0,
                &mut required,
            )
        };
        if required == 0 || required > 64 * 1024 {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        let mut descriptor = vec![0u8; required as usize];
        if unsafe {
            GetFileSecurityW(
                path.as_ptr(),
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
                descriptor.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        Ok(())
    }

    fn verify_interactive_token_cannot_recover_namespace(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String> {
        // This verifies that the creation-time descriptor remains readable and
        // unambiguous. A later host-security acceptance must still perform the
        // full hostile-token matrix before any reviewed-build authority can be
        // considered; this returns only PENDING acceptance.
        self.apply_exact_namespace_security_at_creation(policy)
    }

    fn rollback_exact_stage(
        &mut self,
        stage: DedicatedProducerProvisioningStage,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String> {
        const SC_MANAGER_CONNECT: u32 = 0x0001;
        const DELETE: u32 = 0x0001_0000;
        const SERVICE_CHANGE_CONFIG: u32 = 0x0002;
        const SERVICE_CONFIG_SERVICE_SID_INFO: u32 = 5;
        const SERVICE_SID_TYPE_NONE: u32 = 0;
        #[repr(C)]
        struct ServiceSidInfo {
            service_sid_type: u32,
        }
        unsafe extern "system" {
            fn DeleteFileW(path: *const u16) -> i32;
            fn RemoveDirectoryW(path: *const u16) -> i32;
            fn OpenSCManagerW(
                machine: *const u16,
                database: *const u16,
                access: u32,
            ) -> *mut c_void;
            fn OpenServiceW(manager: *mut c_void, name: *const u16, access: u32) -> *mut c_void;
            fn ChangeServiceConfig2W(service: *mut c_void, level: u32, info: *mut c_void) -> i32;
            fn DeleteService(service: *mut c_void) -> i32;
            fn CloseServiceHandle(handle: *mut c_void) -> i32;
        }
        match stage {
            DedicatedProducerProvisioningStage::ImageDeployed => {
                let path = Self::wide(policy.service_binary);
                if unsafe { DeleteFileW(path.as_ptr()) } == 0 {
                    Err(DEDICATED_PRODUCER_ROLLBACK_UNPROVEN.into())
                } else {
                    Ok(())
                }
            }
            DedicatedProducerProvisioningStage::NamespaceSecurityApplied => Ok(()),
            DedicatedProducerProvisioningStage::NamespaceCreated => {
                let path = Self::wide(policy.namespace);
                if unsafe { RemoveDirectoryW(path.as_ptr()) } == 0 {
                    Err(DEDICATED_PRODUCER_ROLLBACK_UNPROVEN.into())
                } else {
                    Ok(())
                }
            }
            DedicatedProducerProvisioningStage::ServiceSidRestricted
            | DedicatedProducerProvisioningStage::ServiceCreated => {
                let manager = unsafe {
                    OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT)
                };
                if manager.is_null() {
                    return Err(DEDICATED_PRODUCER_ROLLBACK_UNPROVEN.into());
                }
                let name = Self::wide(policy.service_name);
                let access = if stage == DedicatedProducerProvisioningStage::ServiceCreated {
                    DELETE
                } else {
                    SERVICE_CHANGE_CONFIG
                };
                let service = unsafe { OpenServiceW(manager, name.as_ptr(), access) };
                unsafe { CloseServiceHandle(manager) };
                if service.is_null() {
                    return Err(DEDICATED_PRODUCER_ROLLBACK_UNPROVEN.into());
                }
                let ok = if stage == DedicatedProducerProvisioningStage::ServiceCreated {
                    unsafe { DeleteService(service) }
                } else {
                    let mut sid = ServiceSidInfo {
                        service_sid_type: SERVICE_SID_TYPE_NONE,
                    };
                    unsafe {
                        ChangeServiceConfig2W(
                            service,
                            SERVICE_CONFIG_SERVICE_SID_INFO,
                            (&mut sid as *mut ServiceSidInfo).cast(),
                        )
                    }
                };
                unsafe { CloseServiceHandle(service) };
                if ok == 0 {
                    Err(DEDICATED_PRODUCER_ROLLBACK_UNPROVEN.into())
                } else {
                    Ok(())
                }
            }
        }
    }

    fn record_durable_stage(
        &mut self,
        stage: DedicatedProducerProvisioningStage,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String> {
        let path = Self::fixed_journal_path(policy)?;
        let parent = path
            .parent()
            .ok_or_else(|| DEDICATED_PRODUCER_PROVISIONING_REFUSED.to_string())?;
        fs::create_dir_all(parent)
            .map_err(|_| DEDICATED_PRODUCER_PROVISIONING_REFUSED.to_string())?;
        let mut prior = fs::read_to_string(&path).unwrap_or_default();
        if !prior.is_empty() && !prior.starts_with(&policy.policy_sha256) {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        if prior.is_empty() {
            prior.push_str(&policy.policy_sha256);
            prior.push('\n');
        }
        prior.push_str(&format!("{stage:?}\n"));
        fs::write(path, prior).map_err(|_| DEDICATED_PRODUCER_PROVISIONING_REFUSED.to_string())
    }

    fn clear_durable_journal(
        &mut self,
        policy: &DedicatedProducerProvisioningPolicyV1,
    ) -> Result<(), String> {
        let path = Self::fixed_journal_path(policy)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(DEDICATED_PRODUCER_ROLLBACK_UNPROVEN.into()),
        }
    }
}

/// Ordinary CatDesk and autonomous callers receive only this read-only
/// construction.  An Execute request here is refused: it cannot accidentally
/// reach the separately approved administrator mutation host below.
#[cfg(windows)]
#[allow(dead_code)]
fn dedicated_producer_windows_operator_surface(
    request: DedicatedProducerOperatorRequest,
) -> Result<DedicatedProducerOperatorOutcome, String> {
    if request == DedicatedProducerOperatorRequest::ExecuteFixedPolicy {
        return Err(DEDICATED_PRODUCER_MUTATION_HOST_UNAVAILABLE.into());
    }
    let operations = SystemWindowsDedicatedProducerOperations;
    let mut backend = WindowsDedicatedProducerProvisioningBackend::new(operations);
    dedicated_producer_operator_operation(&mut backend, request)
}

/// Separately approved, no-input administrator mutation host.  Its only
/// operation is the compiled `ExecuteFixedPolicy` plan; all service identity,
/// binary/mode, namespace, service-SID, descriptor, journal, and rollback
/// choices remain product constants.  It returns `ActionRequired` only for a
/// provisioned boundary that still requires the separate hostile-token host
/// security acceptance; it never grants reviewed-build authority.
#[cfg(all(windows, not(test)))]
#[allow(dead_code)]
struct DedicatedProducerAdministratorMutationHost {
    backend: WindowsDedicatedProducerProvisioningBackend<SystemWindowsDedicatedProducerOperations>,
}

#[cfg(all(windows, not(test)))]
#[allow(dead_code)]
impl DedicatedProducerAdministratorMutationHost {
    fn new() -> Self {
        Self {
            backend: WindowsDedicatedProducerProvisioningBackend::new(
                SystemWindowsDedicatedProducerOperations,
            ),
        }
    }

    fn execute_fixed_policy(&mut self) -> Result<DedicatedProducerOperatorOutcome, String> {
        dedicated_producer_operator_operation(
            &mut self.backend,
            DedicatedProducerOperatorRequest::ExecuteFixedPolicy,
        )
    }
}

/// Explicit administrator-owned entry point. There are no arguments and no
/// caller-controlled provisioning authority. Unit tests compile a fail-closed
/// substitute and therefore cannot perform any SCM/ACL/service mutation.
#[cfg(all(windows, not(test)))]
#[allow(dead_code)]
fn dedicated_producer_administrator_mutation_surface(
    request: DedicatedProducerAdministratorRequest,
) -> Result<DedicatedProducerOperatorOutcome, String> {
    match request {
        DedicatedProducerAdministratorRequest::ExecuteFixedPolicy => {
            DedicatedProducerAdministratorMutationHost::new().execute_fixed_policy()
        }
    }
}

#[cfg(all(windows, not(test)))]
#[allow(dead_code)]
fn execute_dedicated_producer_fixed_policy_as_administrator()
-> Result<DedicatedProducerOperatorOutcome, String> {
    dedicated_producer_administrator_mutation_surface(
        DedicatedProducerAdministratorRequest::ExecuteFixedPolicy,
    )
}

#[cfg(all(windows, test))]
#[allow(dead_code)]
fn execute_dedicated_producer_fixed_policy_as_administrator()
-> Result<DedicatedProducerOperatorOutcome, String> {
    Err(DEDICATED_PRODUCER_MUTATION_HOST_UNAVAILABLE.into())
}

/// Fixed bootstrap policy for the first installed CatDesk image.  It does not
/// name a source pathname: the only acceptable source is an already-opened
/// capability whose bytes, length, and stable identity have been independently
/// reviewed and bound by a future accepted authority provider.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ReviewedMainImageBootstrapPolicyV1 {
    destination: &'static str,
    policy_sha256: String,
}

fn reviewed_main_image_bootstrap_policy() -> ReviewedMainImageBootstrapPolicyV1 {
    ReviewedMainImageBootstrapPolicyV1 {
        destination: REVIEWED_MAIN_IMAGE_DESTINATION,
        policy_sha256: sha256(&format!(
            "{}|{}",
            REVIEWED_MAIN_IMAGE_BOOTSTRAP_POLICY_VERSION, REVIEWED_MAIN_IMAGE_DESTINATION
        )),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReviewedMainImageRotationPolicyV1 {
    destination: &'static str,
    staging: &'static str,
    pending_envelope: &'static str,
    installed_envelope: &'static str,
    policy_sha256: String,
}

fn reviewed_main_image_rotation_policy() -> ReviewedMainImageRotationPolicyV1 {
    ReviewedMainImageRotationPolicyV1 {
        destination: REVIEWED_MAIN_IMAGE_DESTINATION,
        staging: REVIEWED_MAIN_IMAGE_ROTATION_STAGING,
        pending_envelope: REVIEWED_MAIN_IMAGE_ROTATION_PENDING_ENVELOPE,
        installed_envelope: REVIEWED_MAIN_IMAGE_ROTATION_INSTALLED_ENVELOPE,
        policy_sha256: sha256(&format!(
            "{}|{}|{}|{}|{}",
            REVIEWED_MAIN_IMAGE_ROTATION_POLICY_VERSION,
            REVIEWED_MAIN_IMAGE_DESTINATION,
            REVIEWED_MAIN_IMAGE_ROTATION_STAGING,
            REVIEWED_MAIN_IMAGE_ROTATION_PENDING_ENVELOPE,
            REVIEWED_MAIN_IMAGE_ROTATION_INSTALLED_ENVELOPE,
        )),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReviewedMainImageTrustRootV1 {
    id: &'static str,
    version: u32,
    public_key: [u8; 32],
}

fn production_reviewed_main_image_trust_root() -> Result<ReviewedMainImageTrustRootV1, String> {
    let public_key = REVIEWED_MAIN_IMAGE_PRODUCTION_PUBLIC_KEY
        .ok_or_else(|| REVIEWED_MAIN_IMAGE_TRUST_ROOT_UNPROVISIONED.to_string())?;
    VerifyingKey::from_bytes(&public_key)
        .map_err(|_| REVIEWED_MAIN_IMAGE_TRUST_ROOT_UNPROVISIONED.to_string())?;
    Ok(ReviewedMainImageTrustRootV1 {
        id: REVIEWED_MAIN_IMAGE_TRUST_ROOT_ID,
        version: REVIEWED_MAIN_IMAGE_TRUST_ROOT_VERSION,
        public_key,
    })
}

/// Canonical signed authority transport for the first reviewed CatDesk image.
///
/// The format is deliberately not JSON: exact field order, one LF terminator,
/// fixed names, and a fixed line count make duplicate/unknown/ambiguous fields
/// impossible.  The signature covers every line except `signature=` itself.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ReviewedMainImageEnvelopeV1 {
    product: String,
    purpose: String,
    root_id: String,
    root_version: u32,
    epoch: u64,
    policy_sha256: String,
    payload_sha256: String,
    payload_length: u64,
    review_id: String,
    build_id: String,
    signature: [u8; 64],
}

impl ReviewedMainImageEnvelopeV1 {
    fn signed_bytes(&self) -> Vec<u8> {
        format!(
            "{REVIEWED_MAIN_IMAGE_ENVELOPE_MAGIC}\nproduct={}\npurpose={}\nroot_id={}\nroot_version={}\nepoch={}\npolicy_sha256={}\npayload_sha256={}\npayload_length={}\nreview_id={}\nbuild_id={}\n",
            self.product,
            self.purpose,
            self.root_id,
            self.root_version,
            self.epoch,
            self.policy_sha256,
            self.payload_sha256,
            self.payload_length,
            self.review_id,
            self.build_id,
        )
        .into_bytes()
    }

    fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = self.signed_bytes();
        let signature = base64::engine::general_purpose::STANDARD_NO_PAD.encode(self.signature);
        bytes.extend_from_slice(format!("signature={signature}\n").as_bytes());
        bytes
    }
}

fn canonical_envelope_value<'a>(line: &'a str, name: &str) -> Result<&'a str, String> {
    let prefix = format!("{name}=");
    let value = line
        .strip_prefix(&prefix)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID.to_string())?;
    if !value.is_ascii() || value.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID.into());
    }
    Ok(value)
}

fn canonical_review_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

fn canonical_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn parse_reviewed_main_image_envelope(bytes: &[u8]) -> Result<ReviewedMainImageEnvelopeV1, String> {
    if bytes.is_empty()
        || bytes.len() as u64 > MAX_REVIEWED_MAIN_IMAGE_ENVELOPE_BYTES
        || bytes.contains(&b'\r')
        || !bytes.ends_with(b"\n")
    {
        return Err(REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID.into());
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID.to_string())?;
    let body = text
        .strip_suffix('\n')
        .ok_or_else(|| REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID.to_string())?;
    let lines = body.split('\n').collect::<Vec<_>>();
    if lines.len() != 12 || lines[0] != REVIEWED_MAIN_IMAGE_ENVELOPE_MAGIC {
        return Err(REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID.into());
    }

    let product = canonical_envelope_value(lines[1], "product")?.to_string();
    let purpose = canonical_envelope_value(lines[2], "purpose")?.to_string();
    let root_id = canonical_envelope_value(lines[3], "root_id")?.to_string();
    let root_version = canonical_envelope_value(lines[4], "root_version")?
        .parse::<u32>()
        .map_err(|_| REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID.to_string())?;
    let epoch = canonical_envelope_value(lines[5], "epoch")?
        .parse::<u64>()
        .map_err(|_| REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID.to_string())?;
    let policy_sha256 = canonical_envelope_value(lines[6], "policy_sha256")?.to_string();
    let payload_sha256 = canonical_envelope_value(lines[7], "payload_sha256")?.to_string();
    let payload_length = canonical_envelope_value(lines[8], "payload_length")?
        .parse::<u64>()
        .map_err(|_| REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID.to_string())?;
    let review_id = canonical_envelope_value(lines[9], "review_id")?.to_string();
    let build_id = canonical_envelope_value(lines[10], "build_id")?.to_string();
    let encoded_signature = canonical_envelope_value(lines[11], "signature")?;
    let decoded_signature = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(encoded_signature)
        .map_err(|_| REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID.to_string())?;
    let signature: [u8; 64] = decoded_signature
        .try_into()
        .map_err(|_| REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID.to_string())?;
    if base64::engine::general_purpose::STANDARD_NO_PAD.encode(signature) != encoded_signature
        || root_version == 0
        || epoch == 0
        || payload_length == 0
        || payload_length > MAX_CANDIDATE_BYTES
        || !canonical_lower_sha256(&policy_sha256)
        || !canonical_lower_sha256(&payload_sha256)
        || !canonical_review_identity(&root_id)
        || !canonical_review_identity(&review_id)
        || !canonical_review_identity(&build_id)
    {
        return Err(REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID.into());
    }

    let envelope = ReviewedMainImageEnvelopeV1 {
        product,
        purpose,
        root_id,
        root_version,
        epoch,
        policy_sha256,
        payload_sha256,
        payload_length,
        review_id,
        build_id,
        signature,
    };
    if envelope.canonical_bytes() != bytes {
        return Err(REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID.into());
    }
    Ok(envelope)
}

fn verify_reviewed_main_image_envelope_for_policy(
    envelope: &ReviewedMainImageEnvelopeV1,
    root: &ReviewedMainImageTrustRootV1,
    purpose: &str,
    policy_sha256: &str,
) -> Result<String, String> {
    if envelope.product != REVIEWED_MAIN_IMAGE_PRODUCT
        || envelope.purpose != purpose
        || envelope.root_id != root.id
        || envelope.root_version != root.version
        || envelope.policy_sha256 != policy_sha256
    {
        return Err(REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID.into());
    }
    let key = VerifyingKey::from_bytes(&root.public_key)
        .map_err(|_| REVIEWED_MAIN_IMAGE_SIGNATURE_INVALID.to_string())?;
    let signature = Signature::from_slice(&envelope.signature)
        .map_err(|_| REVIEWED_MAIN_IMAGE_SIGNATURE_INVALID.to_string())?;
    key.verify_strict(&envelope.signed_bytes(), &signature)
        .map_err(|_| REVIEWED_MAIN_IMAGE_SIGNATURE_INVALID.to_string())?;
    Ok(sha256(envelope.canonical_bytes()))
}

fn verify_reviewed_main_image_envelope(
    envelope: &ReviewedMainImageEnvelopeV1,
    root: &ReviewedMainImageTrustRootV1,
    policy: &ReviewedMainImageBootstrapPolicyV1,
) -> Result<String, String> {
    verify_reviewed_main_image_envelope_for_policy(
        envelope,
        root,
        REVIEWED_MAIN_IMAGE_PURPOSE,
        &policy.policy_sha256,
    )
}

fn verify_reviewed_main_image_rotation_envelope(
    envelope: &ReviewedMainImageEnvelopeV1,
    root: &ReviewedMainImageTrustRootV1,
    policy: &ReviewedMainImageRotationPolicyV1,
) -> Result<String, String> {
    verify_reviewed_main_image_envelope_for_policy(
        envelope,
        root,
        REVIEWED_MAIN_IMAGE_ROTATION_PURPOSE,
        &policy.policy_sha256,
    )
}

fn verify_reviewed_main_image_payload_binding(
    envelope: &ReviewedMainImageEnvelopeV1,
    evidence: &OpenRegularEvidence,
) -> Result<(), String> {
    if evidence.sha256 != envelope.payload_sha256 || evidence.length != envelope.payload_length {
        return Err(REVIEWED_MAIN_IMAGE_PAYLOAD_MISMATCH.into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReviewedMainImageEpochDisposition {
    Fresh,
    ExactAcceptedRecovery,
}

fn classify_reviewed_main_image_epoch(
    incoming: &ReviewedMainImageEnvelopeV1,
    accepted: Option<&ReviewedMainImageEnvelopeV1>,
) -> Result<ReviewedMainImageEpochDisposition, String> {
    let Some(accepted) = accepted else {
        return Ok(ReviewedMainImageEpochDisposition::Fresh);
    };
    if incoming.epoch > accepted.epoch {
        return Ok(ReviewedMainImageEpochDisposition::Fresh);
    }
    if incoming.epoch < accepted.epoch {
        return Err(REVIEWED_MAIN_IMAGE_ROLLBACK_REFUSED.into());
    }
    if incoming.canonical_bytes() == accepted.canonical_bytes() {
        return Ok(ReviewedMainImageEpochDisposition::ExactAcceptedRecovery);
    }
    Err(REVIEWED_MAIN_IMAGE_ROLLBACK_REFUSED.into())
}

/// A one-shot, opened-object capability.  It intentionally carries no source
/// pathname and cannot be reconstructed from a caller-provided hash.  The
/// producer of this capability is outside this ticket: current R7C snapshots
/// prove source text only, and the present attestation format intentionally
/// does not prove the final-link output-object handoff required for an image.
struct ReviewedMainImageBootstrapAuthorityV1 {
    policy_sha256: String,
    review_authority_digest: String,
    source_evidence: OpenRegularEvidence,
    source: fs::File,
    consumed: bool,
}

/// The distinction is security-relevant: an existing or reparse destination
/// is refused before create-new and must never become a rollback target.
enum ReviewedMainImageBootstrapInstallFailure {
    Refused,
    FailedAfterCreate,
}

/// The only bootstrap backend input is the compiled policy.  A production
/// authority provider must return a retained reviewed file object, never a
/// path, a hash supplied by an operator, or `target/release` prose.  The fixed
/// destination implementation must derive its evidence from its opened handle
/// and roll back only the object it created for this attempt.
trait ReviewedMainImageBootstrapOperations {
    fn administrator_gate(&mut self) -> Result<bool, String>;
    fn acquire_independently_reviewed_image(
        &mut self,
        policy: &ReviewedMainImageBootstrapPolicyV1,
    ) -> Result<ReviewedMainImageBootstrapAuthorityV1, String>;
    fn install_exact_destination_from_opened_source(
        &mut self,
        policy: &ReviewedMainImageBootstrapPolicyV1,
        source: &mut fs::File,
        source_evidence: &OpenRegularEvidence,
    ) -> Result<OpenRegularEvidence, ReviewedMainImageBootstrapInstallFailure>;
    fn rollback_exact_destination(
        &mut self,
        policy: &ReviewedMainImageBootstrapPolicyV1,
    ) -> Result<(), String>;
}

/// The narrow bootstrap state machine.  It accepts only a capability returned
/// by the backend, proves that capability against its bound opened-handle
/// evidence, and then copies handle-to-handle to the fixed destination.  A
/// successful install remains only a provisioning prerequisite; it does not
/// authorize a reviewed build, candidate, attestation, promotion, or recovery.
fn execute_reviewed_main_image_bootstrap_install(
    backend: &mut impl ReviewedMainImageBootstrapOperations,
) -> Result<&'static str, String> {
    let policy = reviewed_main_image_bootstrap_policy();
    if !backend
        .administrator_gate()
        .map_err(|_| REVIEWED_MAIN_IMAGE_BOOTSTRAP_ADMIN_REQUIRED.to_string())?
    {
        return Err(REVIEWED_MAIN_IMAGE_BOOTSTRAP_ADMIN_REQUIRED.into());
    }
    let mut authority = backend.acquire_independently_reviewed_image(&policy)?;
    if authority.consumed
        || authority.policy_sha256 != policy.policy_sha256
        || !valid_sha256(&authority.review_authority_digest)
        || !valid_sha256(&authority.source_evidence.sha256)
        || authority.source_evidence.identity.is_empty()
        || authority.source_evidence.length > MAX_CANDIDATE_BYTES
    {
        return Err(REVIEWED_MAIN_IMAGE_BOOTSTRAP_REFUSED.into());
    }
    let observed = evidence_from_open_regular(&mut authority.source, "reviewed main image")
        .map_err(|_| REVIEWED_MAIN_IMAGE_BOOTSTRAP_AUTHORITY_REQUIRED.to_string())?;
    if observed != authority.source_evidence {
        return Err(REVIEWED_MAIN_IMAGE_BOOTSTRAP_REFUSED.into());
    }
    authority.consumed = true;
    match backend.install_exact_destination_from_opened_source(
        &policy,
        &mut authority.source,
        &authority.source_evidence,
    ) {
        Ok(destination)
            if destination.sha256 == authority.source_evidence.sha256
                && destination.length == authority.source_evidence.length
                && !destination.identity.is_empty() =>
        {
            Ok("REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_INSTALLED_PENDING_T0212_ACCEPTANCE")
        }
        Ok(_) | Err(ReviewedMainImageBootstrapInstallFailure::FailedAfterCreate) => {
            match backend.rollback_exact_destination(&policy) {
                Ok(()) => Err(REVIEWED_MAIN_IMAGE_BOOTSTRAP_INSTALL_FAILED.into()),
                Err(_) => Err(REVIEWED_MAIN_IMAGE_BOOTSTRAP_ROLLBACK_UNPROVEN.into()),
            }
        }
        Err(ReviewedMainImageBootstrapInstallFailure::Refused) => {
            Err(REVIEWED_MAIN_IMAGE_BOOTSTRAP_REFUSED.into())
        }
    }
}

#[cfg(windows)]
fn reviewed_main_image_fixed_parent_chain_is_safe(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let mut cursor = PathBuf::new();
    for component in parent.components() {
        cursor.push(component.as_os_str());
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(&cursor) else {
            return false;
        };
        if !metadata.file_type().is_dir()
            || metadata.file_type().is_symlink()
            || metadata.file_attributes() & 0x0400 != 0
        {
            return false;
        }
    }
    true
}

#[cfg(windows)]
fn open_fixed_reviewed_main_image_file_with_share(
    path: &Path,
    maximum: u64,
    share_mode: u32,
) -> Result<fs::File, String> {
    const GENERIC_READ: u32 = 0x8000_0000;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    const INVALID_HANDLE_VALUE: *mut c_void = -1_isize as *mut c_void;
    #[repr(C)]
    #[derive(Default)]
    struct Information {
        attributes: u32,
        creation_low: u32,
        creation_high: u32,
        access_low: u32,
        access_high: u32,
        write_low: u32,
        write_high: u32,
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    unsafe extern "system" {
        fn CreateFileW(
            file_name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *mut c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: *mut c_void,
        ) -> *mut c_void;
        fn GetFileInformationByHandle(handle: *mut c_void, information: *mut Information) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    if !reviewed_main_image_fixed_parent_chain_is_safe(path) {
        return Err(REVIEWED_MAIN_IMAGE_TRANSPORT_UNAVAILABLE.into());
    }
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ,
            share_mode,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(REVIEWED_MAIN_IMAGE_TRANSPORT_UNAVAILABLE.into());
    }
    let mut information = Information::default();
    if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
        unsafe {
            CloseHandle(handle);
        }
        return Err(REVIEWED_MAIN_IMAGE_TRANSPORT_UNAVAILABLE.into());
    }
    let size = ((information.size_high as u64) << 32) | information.size_low as u64;
    if information.attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
        || size == 0
        || size > maximum
    {
        unsafe {
            CloseHandle(handle);
        }
        return Err(REVIEWED_MAIN_IMAGE_TRANSPORT_UNAVAILABLE.into());
    }
    Ok(unsafe { fs::File::from_raw_handle(handle) })
}

#[cfg(windows)]
fn open_fixed_reviewed_main_image_file(path: &Path, maximum: u64) -> Result<fs::File, String> {
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    open_fixed_reviewed_main_image_file_with_share(path, maximum, FILE_SHARE_READ)
}

#[cfg(windows)]
#[allow(dead_code)]
fn open_fixed_reviewed_main_image_file_for_atomic_replace(
    path: &Path,
    maximum: u64,
) -> Result<fs::File, String> {
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    open_fixed_reviewed_main_image_file_with_share(
        path,
        maximum,
        FILE_SHARE_READ | FILE_SHARE_DELETE,
    )
}

#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
fn read_fixed_reviewed_main_image_envelope(
    path: &Path,
) -> Result<(ReviewedMainImageEnvelopeV1, Vec<u8>), String> {
    let mut file =
        open_fixed_reviewed_main_image_file(path, MAX_REVIEWED_MAIN_IMAGE_ENVELOPE_BYTES)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|_| REVIEWED_MAIN_IMAGE_TRANSPORT_UNAVAILABLE.to_string())?;
    let envelope = parse_reviewed_main_image_envelope(&bytes)?;
    Ok((envelope, bytes))
}

#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
fn read_accepted_reviewed_main_image_envelope(
    root: &ReviewedMainImageTrustRootV1,
    policy: &ReviewedMainImageBootstrapPolicyV1,
) -> Result<Option<ReviewedMainImageEnvelopeV1>, String> {
    let path = Path::new(REVIEWED_MAIN_IMAGE_ACCEPTED_ENVELOPE);
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID.into()),
        Ok(metadata)
            if metadata.file_type().is_symlink()
                || metadata.file_attributes() & 0x0400 != 0
                || !metadata.file_type().is_file() =>
        {
            Err(REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID.into())
        }
        Ok(_) => {
            let (accepted, _) = read_fixed_reviewed_main_image_envelope(path)
                .map_err(|_| REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID.to_string())?;
            verify_reviewed_main_image_envelope(&accepted, root, policy)
                .map_err(|_| REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID.to_string())?;
            Ok(Some(accepted))
        }
    }
}

/// Narrow read-only bridge for the fixed worker registration protocol.  It
/// reuses the reviewed-main-image trust root and accepted signed envelope;
/// callers receive only the bounded digest of the exact installed image, not
/// a path, file handle, manifest, policy, or alternate artifact authority.
#[cfg(windows)]
pub(crate) fn verified_current_reviewed_main_image_digest() -> Result<String, String> {
    let root = production_reviewed_main_image_trust_root()?;
    let policy = reviewed_main_image_bootstrap_policy();
    let envelope = read_accepted_reviewed_main_image_envelope(&root, &policy)?
        .ok_or_else(|| REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID.to_string())?;
    let mut image =
        open_fixed_reviewed_main_image_file(Path::new(policy.destination), MAX_CANDIDATE_BYTES)?;
    let evidence = evidence_from_open_regular(&mut image, "reviewed current worker image")?;
    verify_reviewed_main_image_payload_binding(&envelope, &evidence)?;
    Ok(evidence.sha256)
}

/// Opaque, one-purpose installation capability for the stable supervisor.
/// It has no caller constructor, pathname, handle, policy, or trust-root
/// input. Its bytes are copied from the exact safely-opened reviewed main
/// image object after the accepted envelope has authenticated that object.
///
/// Installing these same authenticated CatDesk bytes under the fixed
/// supervisor image name adds no privilege: the installed process is invoked
/// only through the fixed `--catdesk-control-plane-supervisor` mode, under the
/// same OS principal, and that mode owns only 127.0.0.1:3201, the private pipe,
/// and protected local supervisor state. It does not confer authority absent
/// from the authenticated CatDesk image itself.
pub(crate) struct ReviewedStableSupervisorImageV1 {
    role: &'static str,
    bytes: Vec<u8>,
    sha256: String,
    length: u64,
}

impl ReviewedStableSupervisorImageV1 {
    /// The fixed installer is the only intended consumer. There is no public
    /// constructor, alternate role, path, or caller-supplied digest authority.
    pub(crate) fn into_fixed_installer_payload(self) -> Result<(Vec<u8>, String, u64), String> {
        if self.role != REVIEWED_STABLE_SUPERVISOR_ROLE
            || self.length == 0
            || self.bytes.len() as u64 != self.length
            || sha256(&self.bytes) != self.sha256
        {
            return Err(REVIEWED_MAIN_IMAGE_PAYLOAD_MISMATCH.into());
        }
        Ok((self.bytes, self.sha256, self.length))
    }

    #[cfg(test)]
    fn role(&self) -> &'static str {
        self.role
    }
}

#[cfg(windows)]
fn bind_verified_main_image_to_stable_supervisor_role(
    mut image: fs::File,
    envelope: &ReviewedMainImageEnvelopeV1,
) -> Result<ReviewedStableSupervisorImageV1, String> {
    let evidence = evidence_from_open_regular(&mut image, "reviewed stable supervisor image")?;
    verify_reviewed_main_image_payload_binding(envelope, &evidence)?;
    if evidence.length == 0 || evidence.length > MAX_CANDIDATE_BYTES {
        return Err(REVIEWED_MAIN_IMAGE_PAYLOAD_MISMATCH.into());
    }
    image
        .seek(SeekFrom::Start(0))
        .map_err(|_| REVIEWED_MAIN_IMAGE_TRANSPORT_UNAVAILABLE.to_string())?;
    let mut bytes = Vec::with_capacity(
        usize::try_from(evidence.length)
            .map_err(|_| REVIEWED_MAIN_IMAGE_TRANSPORT_UNAVAILABLE.to_string())?,
    );
    let copied = Read::by_ref(&mut image)
        .take(evidence.length.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| REVIEWED_MAIN_IMAGE_TRANSPORT_UNAVAILABLE.to_string())?;
    if copied as u64 != evidence.length
        || bytes.len() as u64 != evidence.length
        || sha256(&bytes) != evidence.sha256
    {
        return Err(REVIEWED_MAIN_IMAGE_PAYLOAD_MISMATCH.into());
    }
    // Re-measure the same retained object before granting the role. The open
    // share mode denies write/delete replacement while this handle is alive.
    if evidence_from_open_regular(&mut image, "reviewed stable supervisor image")? != evidence {
        return Err(REVIEWED_MAIN_IMAGE_PAYLOAD_MISMATCH.into());
    }
    Ok(ReviewedStableSupervisorImageV1 {
        role: REVIEWED_STABLE_SUPERVISOR_ROLE,
        bytes,
        sha256: evidence.sha256,
        length: evidence.length,
    })
}

/// Produces the one explicit supervisor role binding. In contrast with the
/// worker-only digest bridge, this repeats the accepted envelope verification
/// and retains/reads the exact opened object that the envelope binds.
#[cfg(windows)]
pub(crate) fn verified_reviewed_stable_supervisor_image()
-> Result<ReviewedStableSupervisorImageV1, String> {
    let root = production_reviewed_main_image_trust_root()?;
    let policy = reviewed_main_image_bootstrap_policy();
    let envelope = read_accepted_reviewed_main_image_envelope(&root, &policy)?
        .ok_or_else(|| REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID.to_string())?;
    let image =
        open_fixed_reviewed_main_image_file(Path::new(policy.destination), MAX_CANDIDATE_BYTES)?;
    bind_verified_main_image_to_stable_supervisor_role(image, &envelope)
}

#[cfg(not(windows))]
pub(crate) fn verified_reviewed_stable_supervisor_image()
-> Result<ReviewedStableSupervisorImageV1, String> {
    Err(REVIEWED_MAIN_IMAGE_TRANSPORT_UNAVAILABLE.into())
}

#[cfg(test)]
pub(crate) fn test_only_verified_stable_supervisor_image() -> ReviewedStableSupervisorImageV1 {
    let bytes = b"isolated reviewed stable supervisor image".to_vec();
    ReviewedStableSupervisorImageV1 {
        role: REVIEWED_STABLE_SUPERVISOR_ROLE,
        sha256: sha256(&bytes),
        length: bytes.len() as u64,
        bytes,
    }
}

#[cfg(not(windows))]
pub(crate) fn verified_current_reviewed_main_image_digest() -> Result<String, String> {
    Err(REVIEWED_MAIN_IMAGE_TRANSPORT_UNAVAILABLE.into())
}

#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
fn persist_accepted_reviewed_main_image_envelope(
    envelope: &ReviewedMainImageEnvelopeV1,
    root: &ReviewedMainImageTrustRootV1,
    policy: &ReviewedMainImageBootstrapPolicyV1,
) -> Result<(), String> {
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }

    let destination = Path::new(REVIEWED_MAIN_IMAGE_ACCEPTED_ENVELOPE);
    let next = Path::new(REVIEWED_MAIN_IMAGE_ACCEPTED_ENVELOPE_NEXT);
    if !reviewed_main_image_fixed_parent_chain_is_safe(destination)
        || fs::symlink_metadata(next).is_ok()
    {
        return Err(REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID.into());
    }
    let bytes = envelope.canonical_bytes();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(next)
        .map_err(|_| REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID.to_string())?;
    let result = (|| {
        file.write_all(&bytes)
            .map_err(|_| REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID.to_string())?;
        file.sync_all()
            .map_err(|_| REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID.to_string())?;
        drop(file);
        let source_wide = next
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let destination_wide = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        if unsafe {
            MoveFileExW(
                source_wide.as_ptr(),
                destination_wide.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID.into());
        }
        let (persisted, persisted_bytes) = read_fixed_reviewed_main_image_envelope(destination)
            .map_err(|_| REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID.to_string())?;
        verify_reviewed_main_image_envelope(&persisted, root, policy)
            .map_err(|_| REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID.to_string())?;
        if persisted_bytes != bytes {
            return Err(REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID.into());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(next);
    }
    result
}

#[cfg(windows)]
#[allow(dead_code)]
fn read_optional_reviewed_main_image_rotation_envelope(
    path: &Path,
    root: &ReviewedMainImageTrustRootV1,
    policy: &ReviewedMainImageRotationPolicyV1,
) -> Result<Option<ReviewedMainImageEnvelopeV1>, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into()),
        Ok(metadata)
            if metadata.file_type().is_symlink()
                || metadata.file_attributes() & 0x0400 != 0
                || !metadata.file_type().is_file() =>
        {
            Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into())
        }
        Ok(_) => {
            let (envelope, _) = read_fixed_reviewed_main_image_envelope(path)
                .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
            verify_reviewed_main_image_rotation_envelope(&envelope, root, policy)
                .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
            Ok(Some(envelope))
        }
    }
}

#[cfg(all(windows, not(test)))]
fn reviewed_main_image_rotation_predecessor(
    root: &ReviewedMainImageTrustRootV1,
    rotation_policy: &ReviewedMainImageRotationPolicyV1,
) -> Result<ReviewedMainImageEnvelopeV1, String> {
    let bootstrap_policy = reviewed_main_image_bootstrap_policy();
    let bootstrap = read_accepted_reviewed_main_image_envelope(root, &bootstrap_policy)?
        .ok_or_else(|| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    if let Some(installed) = read_optional_reviewed_main_image_rotation_envelope(
        Path::new(REVIEWED_MAIN_IMAGE_ROTATION_INSTALLED_ENVELOPE),
        root,
        rotation_policy,
    )? {
        if installed.epoch <= bootstrap.epoch {
            return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
        }
        Ok(installed)
    } else {
        Ok(bootstrap)
    }
}

#[cfg(all(windows, not(test)))]
fn persist_reviewed_main_image_rotation_pending(
    envelope: &ReviewedMainImageEnvelopeV1,
    root: &ReviewedMainImageTrustRootV1,
    policy: &ReviewedMainImageRotationPolicyV1,
) -> Result<(), String> {
    let path = Path::new(policy.pending_envelope);
    if !reviewed_main_image_fixed_parent_chain_is_safe(path) {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
    }
    if let Some(existing) = read_optional_reviewed_main_image_rotation_envelope(path, root, policy)?
    {
        return if existing.canonical_bytes() == envelope.canonical_bytes() {
            Ok(())
        } else {
            Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into())
        };
    }
    let bytes = envelope.canonical_bytes();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    drop(file);
    let (persisted, persisted_bytes) = read_fixed_reviewed_main_image_envelope(path)
        .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    verify_reviewed_main_image_rotation_envelope(&persisted, root, policy)
        .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    if persisted_bytes != bytes {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
    }
    Ok(())
}

#[cfg(all(windows, not(test)))]
fn persist_reviewed_main_image_rotation_installed(
    envelope: &ReviewedMainImageEnvelopeV1,
    root: &ReviewedMainImageTrustRootV1,
    policy: &ReviewedMainImageRotationPolicyV1,
) -> Result<(), String> {
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }
    let destination = Path::new(policy.installed_envelope);
    let next = Path::new(REVIEWED_MAIN_IMAGE_ROTATION_INSTALLED_ENVELOPE_NEXT);
    if !reviewed_main_image_fixed_parent_chain_is_safe(destination) {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
    }
    let bytes = envelope.canonical_bytes();
    match fs::symlink_metadata(next) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(next)
                .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
        }
        Ok(_) => {
            let (staged, staged_bytes) = read_fixed_reviewed_main_image_envelope(next)
                .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
            verify_reviewed_main_image_rotation_envelope(&staged, root, policy)
                .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
            if staged_bytes != bytes {
                return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
            }
        }
        Err(_) => return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into()),
    }
    let source_wide = next
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    if unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
    }
    let (installed, installed_bytes) = read_fixed_reviewed_main_image_envelope(destination)
        .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    verify_reviewed_main_image_rotation_envelope(&installed, root, policy)
        .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    if installed_bytes != bytes {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
    }
    Ok(())
}

#[cfg(all(windows, not(test)))]
fn remove_reviewed_main_image_rotation_pending_if_exact(
    envelope: &ReviewedMainImageEnvelopeV1,
    root: &ReviewedMainImageTrustRootV1,
    policy: &ReviewedMainImageRotationPolicyV1,
) -> Result<(), String> {
    let path = Path::new(policy.pending_envelope);
    let Some(existing) = read_optional_reviewed_main_image_rotation_envelope(path, root, policy)?
    else {
        return Ok(());
    };
    if existing.canonical_bytes() != envelope.canonical_bytes() {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
    }
    fs::remove_file(path).map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())
}

#[cfg(all(windows, not(test)))]
fn prepare_reviewed_main_image_rotation_staging(
    policy: &ReviewedMainImageRotationPolicyV1,
    source: &mut fs::File,
    expected: &OpenRegularEvidence,
) -> Result<(fs::File, OpenRegularEvidence), String> {
    if policy.destination != REVIEWED_MAIN_IMAGE_DESTINATION
        || policy.staging != REVIEWED_MAIN_IMAGE_ROTATION_STAGING
        || !reviewed_main_image_fixed_parent_chain_is_safe(Path::new(policy.staging))
    {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
    }
    match fs::symlink_metadata(policy.staging) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut staging = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(policy.staging)
                .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_INSTALL_FAILED.to_string())?;
            copy_open_regular_files(source, &mut staging, expected.length)
                .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_INSTALL_FAILED.to_string())?;
            let staged =
                evidence_from_open_regular(&mut staging, "reviewed main image rotation staging")
                    .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_INSTALL_FAILED.to_string())?;
            if staged.sha256 != expected.sha256 || staged.length != expected.length {
                return Err(REVIEWED_MAIN_IMAGE_ROTATION_INSTALL_FAILED.into());
            }
        }
        Ok(metadata)
            if metadata.file_type().is_symlink()
                || metadata.file_attributes() & 0x0400 != 0
                || !metadata.file_type().is_file() =>
        {
            return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
        }
        Ok(_) => {}
        Err(_) => return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into()),
    }
    let mut staging = open_fixed_reviewed_main_image_file_for_atomic_replace(
        Path::new(policy.staging),
        MAX_CANDIDATE_BYTES,
    )
    .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    let staged = evidence_from_open_regular(&mut staging, "reviewed main image rotation staging")
        .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    if staged.sha256 != expected.sha256 || staged.length != expected.length {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
    }
    Ok((staging, staged))
}

#[cfg(all(windows, not(test)))]
fn atomic_replace_reviewed_main_image_from_staging(
    policy: &ReviewedMainImageRotationPolicyV1,
) -> Result<(), String> {
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }
    if policy.destination != REVIEWED_MAIN_IMAGE_DESTINATION
        || policy.staging != REVIEWED_MAIN_IMAGE_ROTATION_STAGING
    {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
    }
    let staging = Path::new(policy.staging)
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = Path::new(policy.destination)
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    if unsafe {
        MoveFileExW(
            staging.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_INSTALL_FAILED.into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReviewedMainImageRotationDisposition {
    AlreadyInstalled,
    FinalizePendingReplacement,
    ReplaceFromPredecessor,
}

fn classify_reviewed_main_image_rotation(
    incoming: &ReviewedMainImageEnvelopeV1,
    predecessor: &ReviewedMainImageEnvelopeV1,
    pending: Option<&ReviewedMainImageEnvelopeV1>,
    current: &OpenRegularEvidence,
) -> Result<ReviewedMainImageRotationDisposition, String> {
    let current_matches_predecessor =
        verify_reviewed_main_image_payload_binding(predecessor, current).is_ok();
    let current_matches_incoming =
        verify_reviewed_main_image_payload_binding(incoming, current).is_ok();

    if incoming.canonical_bytes() == predecessor.canonical_bytes() {
        if pending.is_some_and(|value| value.canonical_bytes() != incoming.canonical_bytes())
            || !current_matches_incoming
        {
            return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
        }
        return Ok(ReviewedMainImageRotationDisposition::AlreadyInstalled);
    }
    if incoming.epoch <= predecessor.epoch {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
    }
    let Some(pending) = pending else {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
    };
    if pending.canonical_bytes() != incoming.canonical_bytes() || pending.epoch <= predecessor.epoch
    {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
    }
    if current_matches_incoming {
        return Ok(ReviewedMainImageRotationDisposition::FinalizePendingReplacement);
    }
    if current_matches_predecessor {
        return Ok(ReviewedMainImageRotationDisposition::ReplaceFromPredecessor);
    }
    Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into())
}

#[cfg(all(windows, not(test)))]
fn execute_reviewed_main_image_rotation_as_administrator() -> Result<&'static str, String> {
    let mut admin = SystemWindowsDedicatedProducerOperations;
    if !admin
        .current_token_is_administrator()
        .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_ADMIN_REQUIRED.to_string())?
    {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_ADMIN_REQUIRED.into());
    }
    let policy = reviewed_main_image_rotation_policy();
    if policy.destination != REVIEWED_MAIN_IMAGE_DESTINATION
        || policy.staging != REVIEWED_MAIN_IMAGE_ROTATION_STAGING
        || policy.pending_envelope != REVIEWED_MAIN_IMAGE_ROTATION_PENDING_ENVELOPE
        || policy.installed_envelope != REVIEWED_MAIN_IMAGE_ROTATION_INSTALLED_ENVELOPE
    {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
    }
    let root = production_reviewed_main_image_trust_root()?;
    let (incoming, _) = read_fixed_reviewed_main_image_envelope(Path::new(
        REVIEWED_MAIN_IMAGE_ROTATION_INCOMING_ENVELOPE,
    ))
    .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    verify_reviewed_main_image_rotation_envelope(&incoming, &root, &policy)
        .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;

    let predecessor = reviewed_main_image_rotation_predecessor(&root, &policy)?;
    let pending = read_optional_reviewed_main_image_rotation_envelope(
        Path::new(policy.pending_envelope),
        &root,
        &policy,
    )?;

    let mut source = open_fixed_reviewed_main_image_file(
        Path::new(REVIEWED_MAIN_IMAGE_ROTATION_INCOMING_PAYLOAD),
        MAX_CANDIDATE_BYTES,
    )
    .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    let source_evidence =
        evidence_from_open_regular(&mut source, "reviewed main image rotation input")
            .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    verify_reviewed_main_image_payload_binding(&incoming, &source_evidence)
        .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;

    let mut current = open_fixed_reviewed_main_image_file_for_atomic_replace(
        Path::new(policy.destination),
        MAX_CANDIDATE_BYTES,
    )
    .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    let current_evidence =
        evidence_from_open_regular(&mut current, "installed reviewed main image")
            .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    let effective_pending = if incoming.canonical_bytes() == predecessor.canonical_bytes() {
        pending.clone()
    } else {
        if incoming.epoch <= predecessor.epoch {
            return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
        }
        if let Some(pending) = pending.as_ref() {
            if pending.canonical_bytes() != incoming.canonical_bytes()
                || pending.epoch <= predecessor.epoch
            {
                return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
            }
        } else {
            persist_reviewed_main_image_rotation_pending(&incoming, &root, &policy)?;
        }
        Some(incoming.clone())
    };

    match classify_reviewed_main_image_rotation(
        &incoming,
        &predecessor,
        effective_pending.as_ref(),
        &current_evidence,
    )? {
        ReviewedMainImageRotationDisposition::AlreadyInstalled => {
            if effective_pending.is_some() {
                remove_reviewed_main_image_rotation_pending_if_exact(&incoming, &root, &policy)?;
            }
            if fs::symlink_metadata(policy.staging).is_ok() {
                return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
            }
            return Ok("REVIEWED_BUILD_MAIN_IMAGE_ROTATED_PENDING_T0212_ACCEPTANCE");
        }
        ReviewedMainImageRotationDisposition::FinalizePendingReplacement => {
            if fs::symlink_metadata(policy.staging).is_ok() {
                return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
            }
            persist_reviewed_main_image_rotation_installed(&incoming, &root, &policy)?;
            remove_reviewed_main_image_rotation_pending_if_exact(&incoming, &root, &policy)?;
            return Ok("REVIEWED_BUILD_MAIN_IMAGE_ROTATED_PENDING_T0212_ACCEPTANCE");
        }
        ReviewedMainImageRotationDisposition::ReplaceFromPredecessor => {}
    }

    let (mut staging, staged_evidence) =
        prepare_reviewed_main_image_rotation_staging(&policy, &mut source, &source_evidence)?;
    // Re-check the retained staging object immediately before the rename. The
    // handle permits delete sharing so MoveFileEx can rename this exact object.
    let staged_again =
        evidence_from_open_regular(&mut staging, "reviewed main image rotation staging")
            .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.to_string())?;
    if staged_again != staged_evidence
        || staged_again.sha256 != source_evidence.sha256
        || staged_again.length != source_evidence.length
    {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_REFUSED.into());
    }
    atomic_replace_reviewed_main_image_from_staging(&policy)?;

    let mut installed =
        open_fixed_reviewed_main_image_file(Path::new(policy.destination), MAX_CANDIDATE_BYTES)
            .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_INSTALL_FAILED.to_string())?;
    let installed_evidence =
        evidence_from_open_regular(&mut installed, "rotated reviewed main image")
            .map_err(|_| REVIEWED_MAIN_IMAGE_ROTATION_INSTALL_FAILED.to_string())?;
    if installed_evidence != staged_evidence
        || verify_reviewed_main_image_payload_binding(&incoming, &installed_evidence).is_err()
    {
        return Err(REVIEWED_MAIN_IMAGE_ROTATION_INSTALL_FAILED.into());
    }
    persist_reviewed_main_image_rotation_installed(&incoming, &root, &policy)?;
    remove_reviewed_main_image_rotation_pending_if_exact(&incoming, &root, &policy)?;
    Ok("REVIEWED_BUILD_MAIN_IMAGE_ROTATED_PENDING_T0212_ACCEPTANCE")
}

#[cfg(all(test, windows))]
fn execute_reviewed_main_image_rotation_as_administrator() -> Result<&'static str, String> {
    Err(REVIEWED_MAIN_IMAGE_ROTATION_ADMIN_REQUIRED.into())
}

#[cfg(not(windows))]
fn execute_reviewed_main_image_rotation_as_administrator() -> Result<&'static str, String> {
    Err(REVIEWED_MAIN_IMAGE_ROTATION_ADMIN_REQUIRED.into())
}

#[cfg(windows)]
fn reviewed_main_image_fixed_destination_is_absent(
    policy: &ReviewedMainImageBootstrapPolicyV1,
) -> Result<(), String> {
    if policy.destination != REVIEWED_MAIN_IMAGE_DESTINATION {
        return Err(REVIEWED_MAIN_IMAGE_BOOTSTRAP_REFUSED.into());
    }
    match fs::symlink_metadata(policy.destination) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) | Err(_) => Err(REVIEWED_MAIN_IMAGE_BOOTSTRAP_REFUSED.into()),
    }
}

#[cfg(windows)]
fn reviewed_main_image_fixed_directory_canonical_identity_matches(path: &Path) -> bool {
    let Ok(canonical) = fs::canonicalize(path).map(crate::command::normalize_windows_verbatim_path)
    else {
        return false;
    };
    let expected = crate::command::normalize_windows_verbatim_path(path.to_path_buf());
    canonical
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.to_string_lossy())
}

#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
fn acquire_product_signed_reviewed_main_image(
    policy: &ReviewedMainImageBootstrapPolicyV1,
) -> Result<ReviewedMainImageBootstrapAuthorityV1, String> {
    let root = production_reviewed_main_image_trust_root()?;
    let (incoming, _) =
        read_fixed_reviewed_main_image_envelope(Path::new(REVIEWED_MAIN_IMAGE_INCOMING_ENVELOPE))?;
    let review_authority_digest = verify_reviewed_main_image_envelope(&incoming, &root, policy)?;

    // This is a first-image bootstrap, not an update channel. Refuse an
    // existing final destination before consuming a fresh signed generation or
    // accepting interrupted-install recovery.
    reviewed_main_image_fixed_destination_is_absent(policy)?;
    let accepted = read_accepted_reviewed_main_image_envelope(&root, policy)?;
    let disposition = classify_reviewed_main_image_epoch(&incoming, accepted.as_ref())?;

    let mut source = open_fixed_reviewed_main_image_file(
        Path::new(REVIEWED_MAIN_IMAGE_INCOMING_PAYLOAD),
        MAX_CANDIDATE_BYTES,
    )?;
    let source_evidence = evidence_from_open_regular(&mut source, "reviewed main image")
        .map_err(|_| REVIEWED_MAIN_IMAGE_PAYLOAD_MISMATCH.to_string())?;
    verify_reviewed_main_image_payload_binding(&incoming, &source_evidence)?;

    // A fresh generation is consumed before returning authority. If that
    // authenticated install was interrupted after this write, a later attempt
    // may resume only when the incoming canonical signed envelope is byte-for-
    // byte identical to the already-verified accepted receipt. Exact recovery
    // never rewrites, deletes, or rolls back the anti-rollback state.
    if disposition == ReviewedMainImageEpochDisposition::Fresh {
        persist_accepted_reviewed_main_image_envelope(&incoming, &root, policy)?;
    }
    source
        .seek(SeekFrom::Start(0))
        .map_err(|_| REVIEWED_MAIN_IMAGE_PAYLOAD_MISMATCH.to_string())?;
    Ok(ReviewedMainImageBootstrapAuthorityV1 {
        policy_sha256: policy.policy_sha256.clone(),
        review_authority_digest,
        source_evidence,
        source,
        consumed: false,
    })
}

/// T-0215 replaces the T-0214 authority stub with a signed product-root
/// resolver. Production still fails closed until the compiled public root is
/// provisioned by a separately reviewed source change; transport files alone
/// never confer authority.
/// R7C validates source snapshots and the current build attestation lacks the
/// required final-link handoff.  It is therefore deliberately impossible for
/// the real adapter to manufacture a source from `target/release`, a pathname,
/// or a caller hash.  A later operator-owned trust-root acceptance may replace
/// only this resolver with one that returns the capability above.
#[cfg(windows)]
#[allow(dead_code)]
struct SystemWindowsReviewedMainImageBootstrapOperations;

#[cfg(windows)]
#[allow(dead_code)]
impl SystemWindowsReviewedMainImageBootstrapOperations {
    fn ensure_fixed_destination_parent_is_safe(
        policy: &ReviewedMainImageBootstrapPolicyV1,
    ) -> Result<(), ReviewedMainImageBootstrapInstallFailure> {
        if policy.destination != REVIEWED_MAIN_IMAGE_DESTINATION {
            return Err(ReviewedMainImageBootstrapInstallFailure::Refused);
        }
        let Some(parent) = Path::new(policy.destination).parent() else {
            return Err(ReviewedMainImageBootstrapInstallFailure::Refused);
        };
        match fs::symlink_metadata(parent) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // Validate every already-existing fixed ancestor before
                // creating only the compiled CatDesk child. Never use
                // create_dir_all or accept a caller-supplied path.
                if !reviewed_main_image_fixed_parent_chain_is_safe(parent) {
                    return Err(ReviewedMainImageBootstrapInstallFailure::Refused);
                }
                if let Err(create_error) = fs::create_dir(parent) {
                    if create_error.kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(ReviewedMainImageBootstrapInstallFailure::Refused);
                    }
                }
            }
            Err(_) => return Err(ReviewedMainImageBootstrapInstallFailure::Refused),
        }
        let metadata = fs::symlink_metadata(parent)
            .map_err(|_| ReviewedMainImageBootstrapInstallFailure::Refused)?;
        if !metadata.file_type().is_dir()
            || metadata.file_type().is_symlink()
            || metadata.file_attributes() & 0x0400 != 0
            || !reviewed_main_image_fixed_parent_chain_is_safe(Path::new(policy.destination))
        {
            return Err(ReviewedMainImageBootstrapInstallFailure::Refused);
        }
        if !reviewed_main_image_fixed_directory_canonical_identity_matches(parent) {
            return Err(ReviewedMainImageBootstrapInstallFailure::Refused);
        }
        Ok(())
    }
}

#[cfg(windows)]
impl ReviewedMainImageBootstrapOperations for SystemWindowsReviewedMainImageBootstrapOperations {
    fn administrator_gate(&mut self) -> Result<bool, String> {
        let mut operations = SystemWindowsDedicatedProducerOperations;
        operations.current_token_is_administrator()
    }

    fn acquire_independently_reviewed_image(
        &mut self,
        policy: &ReviewedMainImageBootstrapPolicyV1,
    ) -> Result<ReviewedMainImageBootstrapAuthorityV1, String> {
        acquire_product_signed_reviewed_main_image(policy)
    }

    fn install_exact_destination_from_opened_source(
        &mut self,
        policy: &ReviewedMainImageBootstrapPolicyV1,
        source: &mut fs::File,
        source_evidence: &OpenRegularEvidence,
    ) -> Result<OpenRegularEvidence, ReviewedMainImageBootstrapInstallFailure> {
        Self::ensure_fixed_destination_parent_is_safe(policy)?;
        match fs::symlink_metadata(policy.destination) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) | Err(_) => return Err(ReviewedMainImageBootstrapInstallFailure::Refused),
        }
        let mut destination = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(policy.destination)
            .map_err(|_| ReviewedMainImageBootstrapInstallFailure::Refused)?;
        copy_open_regular_files(source, &mut destination, source_evidence.length)
            .map_err(|_| ReviewedMainImageBootstrapInstallFailure::FailedAfterCreate)?;
        evidence_from_open_regular(&mut destination, "installed reviewed main image")
            .map_err(|_| ReviewedMainImageBootstrapInstallFailure::FailedAfterCreate)
    }

    fn rollback_exact_destination(
        &mut self,
        policy: &ReviewedMainImageBootstrapPolicyV1,
    ) -> Result<(), String> {
        // This path is reached only after this adapter created the exact fixed
        // destination.  Existing/reparse destinations are refused before
        // create-new, so they are never rollback targets.
        fs::remove_file(policy.destination)
            .map_err(|_| REVIEWED_MAIN_IMAGE_BOOTSTRAP_ROLLBACK_UNPROVEN.to_string())
    }
}

#[cfg(all(windows, not(test)))]
fn execute_reviewed_main_image_bootstrap_install_as_administrator() -> Result<&'static str, String>
{
    let mut backend = SystemWindowsReviewedMainImageBootstrapOperations;
    execute_reviewed_main_image_bootstrap_install(&mut backend)
}

#[cfg(all(windows, test))]
fn execute_reviewed_main_image_bootstrap_install_as_administrator() -> Result<&'static str, String>
{
    // Unit tests never exercise the live ProgramData/Program Files bootstrap
    // backend, even after the production public root is provisioned. Live host
    // mutation remains an explicit operator-only acceptance step.
    Err(REVIEWED_MAIN_IMAGE_BOOTSTRAP_AUTHORITY_REQUIRED.into())
}

#[cfg(not(windows))]
fn execute_reviewed_main_image_bootstrap_install_as_administrator() -> Result<&'static str, String>
{
    Err(REVIEWED_MAIN_IMAGE_BOOTSTRAP_AUTHORITY_REQUIRED.into())
}

/// Parse only the zero-parameter bootstrap command.  It remains separate from
/// MCP/autonomous surfaces and cannot convey a source image, hash, review
/// token, destination, service, account, ACL, or command.
pub(crate) fn parse_reviewed_main_image_bootstrap_install_args(
    args: &[String],
) -> Result<bool, String> {
    let count = args
        .iter()
        .filter(|arg| arg.as_str() == REVIEWED_MAIN_IMAGE_BOOTSTRAP_INSTALL_FLAG)
        .count();
    if count == 0 {
        return Ok(false);
    }
    if count != 1 || args.len() != 1 {
        return Err(
            "--catdesk-reviewed-main-image-bootstrap-install-fixed-policy accepts no flags or positional arguments"
                .into(),
        );
    }
    Ok(true)
}

pub(crate) fn run_reviewed_main_image_bootstrap_install_command() -> Result<&'static str, String> {
    execute_reviewed_main_image_bootstrap_install_as_administrator()
}

/// Parse only the zero-parameter signed rotation command. It is deliberately
/// distinct from first-image bootstrap and cannot carry a path, hash, epoch,
/// signature, destination, service, or other authority-bearing value.
pub(crate) fn parse_reviewed_main_image_rotate_args(args: &[String]) -> Result<bool, String> {
    let count = args
        .iter()
        .filter(|arg| arg.as_str() == REVIEWED_MAIN_IMAGE_ROTATE_FLAG)
        .count();
    if count == 0 {
        return Ok(false);
    }
    if count != 1 || args.len() != 1 {
        return Err(
            "--catdesk-reviewed-main-image-rotate-fixed-policy accepts no flags or positional arguments"
                .into(),
        );
    }
    Ok(true)
}

pub(crate) fn run_reviewed_main_image_rotate_command() -> Result<&'static str, String> {
    execute_reviewed_main_image_rotation_as_administrator()
}

/// Parse only the exact administrator-owned command shape. A flag occurrence
/// combined with any positional argument, option, or duplicate is rejected
/// before the host is constructed; unrelated CLI modes continue their own
/// closed parsing paths.
pub(crate) fn parse_dedicated_producer_admin_provision_args(
    args: &[String],
) -> Result<bool, String> {
    let count = args
        .iter()
        .filter(|arg| arg.as_str() == DEDICATED_PRODUCER_ADMIN_PROVISION_FLAG)
        .count();
    if count == 0 {
        return Ok(false);
    }
    if count != 1 || args.len() != 1 {
        return Err(
            "--catdesk-reviewed-producer-provision-fixed-policy accepts no flags or positional arguments"
                .into(),
        );
    }
    Ok(true)
}

/// The fixed service image has exactly one entry mode. It is terminal, accepts
/// no payload or options, and is intentionally separate from the
/// administrator provisioning command and the reviewed-build worker flags.
pub(crate) fn parse_dedicated_producer_service_args(args: &[String]) -> Result<bool, String> {
    let count = args
        .iter()
        .filter(|arg| arg.as_str() == DEDICATED_PRODUCER_SERVICE_MODE)
        .count();
    if count == 0 {
        return Ok(false);
    }
    if count != 1 || args.len() != 1 {
        return Err(
            "--catdesk-reviewed-producer-service accepts no flags or positional arguments".into(),
        );
    }
    Ok(true)
}

/// This is the closed T-0211 service-side runtime seam. It deliberately has
/// no request parser: a future accepted named-pipe adapter may supply only a
/// fully authenticated `DedicatedProducerExecutionRequestV1` and exact output
/// handle. Until T-0212 validates the provisioned host boundary, this entry
/// remains non-authoritative and refuses to execute a build.
#[allow(dead_code)]
struct DedicatedProducerServiceRuntimeV1;

#[allow(dead_code)]
impl DedicatedProducerServiceRuntimeV1 {
    fn run_fixed_ipc_endpoint() -> Result<(), String> {
        let policy = dedicated_producer_fixed_policy();
        if policy.service_name != DEDICATED_PRODUCER_SERVICE_NAME
            || policy.reviewed_service_image != DEDICATED_PRODUCER_REVIEWED_IMAGE
            || policy.service_binary != DEDICATED_PRODUCER_SERVICE_BINARY
            || policy.service_mode != DEDICATED_PRODUCER_SERVICE_MODE
        {
            return Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into());
        }
        Err(DEDICATED_PRODUCER_SERVICE_IPC_UNAVAILABLE.into())
    }
}

/// Closed production service entry. Production verifies it is the exact fixed
/// deployed image before reaching the T-0211 IPC seam; it accepts no command,
/// path, environment, tool, or build request from its arguments.
#[cfg(all(windows, not(test)))]
pub(crate) fn run_dedicated_producer_service_command() -> Result<(), String> {
    let current = std::env::current_exe()
        .map_err(|_| DEDICATED_PRODUCER_SERVICE_CONFIGURATION_FAILED.to_string())?;
    let canonical = fs::canonicalize(&current)
        .map(crate::command::normalize_windows_verbatim_path)
        .map_err(|_| DEDICATED_PRODUCER_SERVICE_CONFIGURATION_FAILED.to_string())?;
    let expected = crate::command::normalize_windows_verbatim_path(PathBuf::from(
        DEDICATED_PRODUCER_SERVICE_BINARY,
    ));
    if !canonical
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.to_string_lossy())
    {
        return Err(DEDICATED_PRODUCER_SERVICE_CONFIGURATION_FAILED.into());
    }
    SystemWindowsDedicatedProducerOperations::assert_fixed_service_binary_is_safe(
        &dedicated_producer_fixed_policy(),
    )
    .map_err(|_| DEDICATED_PRODUCER_SERVICE_CONFIGURATION_FAILED.to_string())?;
    DedicatedProducerServiceRuntimeV1::run_fixed_ipc_endpoint()
}

/// Tests never invoke a deployed image, service, pipe, or reviewed build.
#[cfg(test)]
pub(crate) fn run_dedicated_producer_service_command() -> Result<(), String> {
    DedicatedProducerServiceRuntimeV1::run_fixed_ipc_endpoint()
}

#[cfg(all(not(windows), not(test)))]
pub(crate) fn run_dedicated_producer_service_command() -> Result<(), String> {
    Err(DEDICATED_PRODUCER_NOT_PROVISIONED.into())
}

/// The CLI delegates only to the no-input administrator host. `ActionRequired`
/// is a bounded provisioning-pending result, never producer isolation or
/// reviewed-build authorization.
pub(crate) fn run_dedicated_producer_admin_provision_command() -> Result<&'static str, String> {
    match execute_dedicated_producer_fixed_policy_as_administrator()? {
        DedicatedProducerOperatorOutcome::ActionRequired => {
            Ok("REVIEWED_BUILD_DEDICATED_PRODUCER_PROVISIONED_PENDING_HOST_SECURITY_ACCEPTANCE")
        }
        DedicatedProducerOperatorOutcome::NotProvisioned => {
            Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into())
        }
    }
}

/// Closed parent-to-service execution capability.  It has no pathname, shell,
/// service, or caller-selected tool authority: all fields bind the protected
/// attempt and already trusted R7C/T-0195 evidence to one high-entropy nonce.
#[allow(dead_code)]
#[derive(Clone, Debug)]
struct DedicatedProducerExecutionRequestV1 {
    build_attempt_id: String,
    nonce: String,
    snapshot_authority_digest: String,
    snapshot_manifest_digest: String,
    build_policy_sha256: String,
    cargo: TrustedBuildToolV1,
    rustc: TrustedBuildToolV1,
}

#[allow(dead_code)]
impl DedicatedProducerExecutionRequestV1 {
    fn new(attempt: &ReviewedBuildAttemptV1) -> Result<Self, String> {
        if !valid_attempt_id(&attempt.build_attempt_id)
            || !valid_sha256(&attempt.snapshot_authority_digest)
            || !valid_sha256(&attempt.snapshot_manifest_digest)
            || !valid_sha256(&attempt.policy.policy_sha256)
        {
            return Err(DEDICATED_PRODUCER_HANDOFF_REJECTED.into());
        }
        Ok(Self {
            build_attempt_id: attempt.build_attempt_id.clone(),
            nonce: Uuid::new_v4().simple().to_string(),
            snapshot_authority_digest: attempt.snapshot_authority_digest.clone(),
            snapshot_manifest_digest: attempt.snapshot_manifest_digest.clone(),
            build_policy_sha256: attempt.policy.policy_sha256.clone(),
            cargo: attempt.cargo.clone(),
            rustc: attempt.rustc.clone(),
        })
    }
}

/// Fixed peer evidence supplied by the future named-pipe/handle-transfer
/// implementation. Each identity must be observed from real service/process/
/// token/Job/pipe handles; text assertions alone are rejected by the protocol.
#[allow(dead_code)]
#[derive(Clone, Debug)]
struct DedicatedProducerPeerEvidenceV1 {
    service_sid_digest: String,
    process_identity: String,
    token_identity: String,
    pipe_acl_digest: String,
    isolated_root_identity: String,
    exact_job_member: bool,
    owner_acl_verified: bool,
}

/// An exact already-open producer output handle and its claimed binding. The
/// file object, not its directory entry, is the sole authority input to the
/// parent evidence/candidate path.
#[allow(dead_code)]
struct DedicatedProducerTransferredOutputV1 {
    request: DedicatedProducerExecutionRequestV1,
    peer: DedicatedProducerPeerEvidenceV1,
    output: fs::File,
    claimed_evidence: OpenRegularEvidence,
    consumed: bool,
}

#[allow(dead_code)]
fn accept_dedicated_producer_transferred_output(
    handoff: &mut DedicatedProducerTransferredOutputV1,
    attempt: &ReviewedBuildAttemptV1,
    expected_nonce: &str,
    expected_service_sid_digest: &str,
) -> Result<OpenRegularEvidence, String> {
    if handoff.consumed
        || handoff.request.build_attempt_id != attempt.build_attempt_id
        || handoff.request.snapshot_authority_digest != attempt.snapshot_authority_digest
        || handoff.request.snapshot_manifest_digest != attempt.snapshot_manifest_digest
        || handoff.request.build_policy_sha256 != attempt.policy.policy_sha256
        || handoff.request.cargo != attempt.cargo
        || handoff.request.rustc != attempt.rustc
        || handoff.request.nonce.len() != 32
        || !handoff
            .request
            .nonce
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || handoff.request.nonce != expected_nonce
        || handoff.peer.service_sid_digest != expected_service_sid_digest
        || handoff.peer.process_identity.is_empty()
        || handoff.peer.token_identity.is_empty()
        || handoff.peer.pipe_acl_digest.is_empty()
        || handoff.peer.isolated_root_identity.is_empty()
        || !handoff.peer.exact_job_member
        || !handoff.peer.owner_acl_verified
    {
        return Err(DEDICATED_PRODUCER_HANDOFF_REJECTED.into());
    }
    let actual = evidence_from_open_regular(
        &mut handoff.output,
        "dedicated producer transferred output handle",
    )?;
    if actual != handoff.claimed_evidence {
        return Err(DEDICATED_PRODUCER_HANDOFF_REJECTED.into());
    }
    handoff.consumed = true;
    Ok(actual)
}

#[cfg(all(test, windows))]
struct DedicatedProducerHandoffV1 {
    build_attempt_id: String,
    nonce: String,
    snapshot_authority_digest: String,
    policy_digest: String,
    service_sid_digest: String,
    consumed: bool,
}

#[cfg(all(test, windows))]
impl DedicatedProducerHandoffV1 {
    fn new(
        attempt: &str,
        snapshot_digest: &str,
        policy_digest: &str,
        service_sid_digest: &str,
    ) -> Self {
        Self {
            build_attempt_id: attempt.into(),
            nonce: Uuid::new_v4().simple().to_string(),
            snapshot_authority_digest: snapshot_digest.into(),
            policy_digest: policy_digest.into(),
            service_sid_digest: service_sid_digest.into(),
            consumed: false,
        }
    }

    fn validate_without_service_handle(
        &mut self,
        attempt: &str,
        nonce: &str,
        snapshot_digest: &str,
        policy_digest: &str,
        service_sid_digest: &str,
        exact_job_member: bool,
    ) -> Result<(), String> {
        if self.consumed
            || attempt != self.build_attempt_id
            || nonce != self.nonce
            || snapshot_digest != self.snapshot_authority_digest
            || policy_digest != self.policy_digest
            || service_sid_digest != self.service_sid_digest
            || !exact_job_member
        {
            return Err(DEDICATED_PRODUCER_HANDOFF_REJECTED.into());
        }
        // A service name/SID assertion cannot replace a producer-owned output
        // handle or real service token/namespace inspection.
        Err(PRODUCER_HANDLE_NOT_CAPTURED.into())
    }
}

#[cfg(all(test, windows))]
fn detect_preprovisioned_dedicated_producer_service() -> Result<(), String> {
    const SC_MANAGER_CONNECT: u32 = 0x0001;
    const SERVICE_QUERY_STATUS: u32 = 0x0004;
    const READ_CONTROL: u32 = 0x0002_0000;
    unsafe extern "system" {
        fn OpenSCManagerW(machine: *const u16, database: *const u16, access: u32) -> *mut c_void;
        fn OpenServiceW(manager: *mut c_void, name: *const u16, access: u32) -> *mut c_void;
        fn CloseServiceHandle(handle: *mut c_void) -> i32;
    }
    let manager = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return Err(DEDICATED_PRODUCER_NOT_PROVISIONED.into());
    }
    let name = OsString::from(DEDICATED_PRODUCER_SERVICE_NAME)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let service =
        unsafe { OpenServiceW(manager, name.as_ptr(), SERVICE_QUERY_STATUS | READ_CONTROL) };
    unsafe {
        CloseServiceHandle(manager);
    }
    if service.is_null() {
        return Err(DEDICATED_PRODUCER_NOT_PROVISIONED.into());
    }
    unsafe {
        CloseServiceHandle(service);
    }
    // A present service name is insufficient until its live token, service SID,
    // DACL/owner/label, Job process and output-handle handoff are all checked.
    Err(DEDICATED_PRODUCER_UNPROVEN.into())
}

/// Test-only R5D capability shape. A production isolated root would require a
/// distinct producer token/owner and an OS-enforced capability, not a DACL
/// owned by the normal interactive user. This model has no worker call site.
#[cfg(all(test, windows))]
struct IsolatedProducerCapabilityV1 {
    attempt_id: String,
    producer_sid_digest: String,
    namespace_owner_sid_digest: String,
    nonce: String,
    consumed: bool,
}

#[cfg(all(test, windows))]
impl IsolatedProducerCapabilityV1 {
    fn new(attempt_id: &str, producer_sid_digest: &str, namespace_owner_sid_digest: &str) -> Self {
        Self {
            attempt_id: attempt_id.into(),
            producer_sid_digest: producer_sid_digest.into(),
            namespace_owner_sid_digest: namespace_owner_sid_digest.into(),
            nonce: Uuid::new_v4().simple().to_string(),
            consumed: false,
        }
    }

    fn validate(
        &mut self,
        attempt_id: &str,
        producer_sid_digest: &str,
        nonce: &str,
    ) -> Result<(), String> {
        if self.consumed
            || attempt_id != self.attempt_id
            || producer_sid_digest != self.producer_sid_digest
            || producer_sid_digest == self.namespace_owner_sid_digest
            || nonce != self.nonce
        {
            return Err(ISOLATION_CAPABILITY_REJECTED.into());
        }
        self.consumed = true;
        Ok(())
    }
}

#[cfg(all(test, windows))]
fn current_token_isolation_owner_separated() -> Result<(), String> {
    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_USER: u32 = 1;
    const TOKEN_OWNER: u32 = 4;
    const TOKEN_IS_APPCONTAINER: u32 = 29;
    #[repr(C)]
    struct SidAndAttributes {
        sid: *mut c_void,
        attributes: u32,
    }
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
        fn EqualSid(left: *mut c_void, right: *mut c_void) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    fn information(token: *mut c_void, class: u32) -> Result<Vec<u8>, String> {
        unsafe extern "system" {
            fn GetTokenInformation(
                token: *mut c_void,
                class: u32,
                buffer: *mut c_void,
                length: u32,
                returned: *mut u32,
            ) -> i32;
        }
        let mut size = 0u32;
        unsafe {
            GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut size);
        }
        if size == 0 || size > 64 * 1024 {
            return Err(ISOLATION_UNPROVEN.into());
        }
        let mut bytes = vec![0u8; size as usize];
        if unsafe { GetTokenInformation(token, class, bytes.as_mut_ptr().cast(), size, &mut size) }
            == 0
        {
            return Err(ISOLATION_UNPROVEN.into());
        }
        Ok(bytes)
    }
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0
        || token.is_null()
    {
        return Err(ISOLATION_UNPROVEN.into());
    }
    let result = (|| {
        let user = information(token, TOKEN_USER)?;
        let owner = information(token, TOKEN_OWNER)?;
        let app_container = information(token, TOKEN_IS_APPCONTAINER)?;
        if app_container.len() < std::mem::size_of::<u32>() {
            return Err(ISOLATION_UNPROVEN.into());
        }
        let user_sid = unsafe { (user.as_ptr().cast::<SidAndAttributes>()).read().sid };
        let owner_sid = unsafe { (owner.as_ptr().cast::<*mut c_void>()).read() };
        if user_sid.is_null() || owner_sid.is_null() {
            return Err(ISOLATION_UNPROVEN.into());
        }
        let is_app_container = u32::from_ne_bytes(app_container[..4].try_into().unwrap()) != 0;
        if !is_app_container && unsafe { EqualSid(user_sid, owner_sid) } != 0 {
            return Err(ISOLATION_OWNER_REACQUIRABLE.into());
        }
        // An AppContainer bit or a distinct owner alone is insufficient: this
        // prototype has not established the required DACL, integrity label,
        // write/owner privileges, or output-root confinement.
        Err(ISOLATION_UNPROVEN.into())
    })();
    unsafe {
        CloseHandle(token);
    }
    result
}

/// Executes the R5D-R2A fixed-action lowbox prototype.  The only child action
/// is an ignored unit-test helper that creates one fixed child in the parent
/// supplied working directory; no command, executable, or output pathname is
/// accepted from a caller.  This remains test-only feasibility code and never
/// authorizes the reviewed-build worker.
#[cfg(all(test, windows))]
fn appcontainer_lowbox_launch_feasibility_attempt() -> Result<(), String> {
    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_IS_APPCONTAINER: u32 = 29;
    const TOKEN_USER: u32 = 1;
    const TOKEN_APPCONTAINER_SID: u32 = 31;
    const PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES: usize = 0x0002_0009;
    const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
    const CREATE_SUSPENDED_TEST: u32 = 0x0000_0004;
    const WAIT_OBJECT_0: u32 = 0;
    #[repr(C)]
    struct SecurityCapabilities {
        app_container_sid: *mut c_void,
        capabilities: *mut c_void,
        capability_count: u32,
        reserved: u32,
    }
    #[repr(C)]
    struct StartupInfoW {
        cb: u32,
        reserved: *mut u16,
        desktop: *mut u16,
        title: *mut u16,
        x: u32,
        y: u32,
        x_size: u32,
        y_size: u32,
        x_count_chars: u32,
        y_count_chars: u32,
        fill_attribute: u32,
        flags: u32,
        show_window: u16,
        cb_reserved2: u16,
        reserved2: *mut u8,
        std_input: *mut c_void,
        std_output: *mut c_void,
        std_error: *mut c_void,
    }
    #[repr(C)]
    struct StartupInfoExW {
        startup_info: StartupInfoW,
        attribute_list: *mut c_void,
    }
    #[repr(C)]
    struct ProcessInformation {
        process: *mut c_void,
        thread: *mut c_void,
        process_id: u32,
        thread_id: u32,
    }
    #[repr(C)]
    struct SidAndAttributes {
        sid: *mut c_void,
        attributes: u32,
    }
    unsafe extern "system" {
        fn CreateAppContainerProfile(
            name: *const u16,
            display_name: *const u16,
            description: *const u16,
            capabilities: *const c_void,
            capability_count: u32,
            app_container_sid: *mut *mut c_void,
        ) -> i32;
        fn DeleteAppContainerProfile(name: *const u16) -> i32;
        fn FreeSid(sid: *mut c_void) -> *mut c_void;
        fn InitializeProcThreadAttributeList(
            list: *mut c_void,
            count: u32,
            flags: u32,
            size: *mut usize,
        ) -> i32;
        fn UpdateProcThreadAttribute(
            list: *mut c_void,
            flags: u32,
            attribute: usize,
            value: *mut c_void,
            size: usize,
            previous: *mut c_void,
            returned: *mut usize,
        ) -> i32;
        fn DeleteProcThreadAttributeList(list: *mut c_void);
        fn CreateProcessW(
            application: *const u16,
            command_line: *mut u16,
            process_attributes: *mut c_void,
            thread_attributes: *mut c_void,
            inherit_handles: i32,
            creation_flags: u32,
            environment: *mut c_void,
            current_directory: *const u16,
            startup_info: *mut StartupInfoW,
            process_information: *mut ProcessInformation,
        ) -> i32;
        fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
        fn GetTokenInformation(
            token: *mut c_void,
            class: u32,
            buffer: *mut c_void,
            length: u32,
            returned: *mut u32,
        ) -> i32;
        fn ConvertSidToStringSidW(sid: *mut c_void, string: *mut *mut u16) -> i32;
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
        fn EqualSid(left: *mut c_void, right: *mut c_void) -> i32;
        fn ResumeThread(thread: *mut c_void) -> u32;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    fn token_information(token: *mut c_void, class: u32) -> Result<Vec<u8>, String> {
        let mut size = 0u32;
        unsafe {
            GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut size);
        }
        if size == 0 || size > 64 * 1024 {
            return Err(APPCONTAINER_TOKEN_UNPROVEN.into());
        }
        let mut value = vec![0u8; size as usize];
        if unsafe { GetTokenInformation(token, class, value.as_mut_ptr().cast(), size, &mut size) }
            == 0
        {
            return Err(APPCONTAINER_TOKEN_UNPROVEN.into());
        }
        Ok(value)
    }
    fn sid_to_string(sid: *mut c_void) -> Result<String, String> {
        if sid.is_null() {
            return Err(APPCONTAINER_TOKEN_UNPROVEN.into());
        }
        let mut text = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
            return Err(APPCONTAINER_TOKEN_UNPROVEN.into());
        }
        let result = unsafe {
            let mut length = 0usize;
            while *text.add(length) != 0 {
                length += 1;
                if length > 1024 {
                    LocalFree(text.cast());
                    return Err(APPCONTAINER_TOKEN_UNPROVEN.into());
                }
            }
            String::from_utf16(std::slice::from_raw_parts(text, length))
                .map_err(|_| APPCONTAINER_TOKEN_UNPROVEN.to_string())
        };
        unsafe {
            LocalFree(text.cast());
        }
        result
    }
    fn current_user_sid() -> Result<String, String> {
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut c_void;
        }
        let mut token = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0
            || token.is_null()
        {
            return Err(APPCONTAINER_TOKEN_UNPROVEN.into());
        }
        let result = (|| {
            let user = token_information(token, TOKEN_USER)?;
            if user.len() < std::mem::size_of::<SidAndAttributes>() {
                return Err(APPCONTAINER_TOKEN_UNPROVEN.into());
            }
            sid_to_string(unsafe { user.as_ptr().cast::<SidAndAttributes>().read().sid })
        })();
        unsafe {
            CloseHandle(token);
        }
        result
    }
    let profile = format!("CatDeskR5D{}", Uuid::new_v4().simple());
    let profile_wide = OsString::from(&profile)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut sid = std::ptr::null_mut();
    if unsafe {
        CreateAppContainerProfile(
            profile_wide.as_ptr(),
            profile_wide.as_ptr(),
            profile_wide.as_ptr(),
            std::ptr::null(),
            0,
            &mut sid,
        )
    } != 0
        || sid.is_null()
    {
        return Err(APPCONTAINER_LAUNCH_UNAVAILABLE.into());
    }
    let result = (|| {
        let ordinary_user_sid = current_user_sid()?;
        let app_container_sid = sid_to_string(sid)?;
        // OWNER RIGHTS explicitly denies the owner mutation rights that are
        // otherwise implicitly recoverable.  The ordinary token has read-only
        // visibility; the real AppContainer package SID receives the fixed
        // helper's write/create mapping.  A low mandatory label permits the
        // lowbox write only; it is not treated as sufficient isolation.
        let security_descriptor = format!(
            "O:{ordinary_user_sid}G:{ordinary_user_sid}D:PAI(D;OICI;0x000D0156;;;OW)(A;OICI;0x00120089;;;{ordinary_user_sid})(A;OICI;0x00120116;;;{app_container_sid})S:(ML;OICI;NW;;;LW)"
        );
        let prototype_path =
            std::env::temp_dir().join(format!("catdesk-r5d-r2a-{}", Uuid::new_v4()));
        fs::create_dir(&prototype_path).map_err(|_| APPCONTAINER_OUTPUT_UNPROVEN)?;
        let mut producer_root =
            ProtectedDirectoryGuard::acquire(&prototype_path, "R5D-R2A prototype parent")
                .map_err(|_| APPCONTAINER_OUTPUT_UNPROVEN)?;
        producer_root
            .create_child_with_security_descriptor(
                "producer-root",
                &security_descriptor,
                "R5D-R2A descriptor-at-create producer root",
            )
            .map_err(|_| APPCONTAINER_SECURITY_UNPROVEN)?;
        producer_root
            .assert_stable("R5D-R2A retained producer root")
            .map_err(|_| APPCONTAINER_SECURITY_UNPROVEN)?;
        let capabilities = SecurityCapabilities {
            app_container_sid: sid,
            capabilities: std::ptr::null_mut(),
            capability_count: 0,
            reserved: 0,
        };
        let mut list_size = 0usize;
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut list_size);
        }
        if list_size == 0 || list_size > 64 * 1024 {
            return Err(APPCONTAINER_LAUNCH_UNAVAILABLE.into());
        }
        let mut list = vec![0usize; list_size.div_ceil(std::mem::size_of::<usize>())];
        let list_ptr = list.as_mut_ptr().cast::<c_void>();
        if unsafe { InitializeProcThreadAttributeList(list_ptr, 1, 0, &mut list_size) } == 0
            || unsafe {
                UpdateProcThreadAttribute(
                    list_ptr,
                    0,
                    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
                    (&capabilities as *const SecurityCapabilities)
                        .cast_mut()
                        .cast(),
                    std::mem::size_of::<SecurityCapabilities>(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            } == 0
        {
            return Err(APPCONTAINER_LAUNCH_UNAVAILABLE.into());
        }
        let executable = std::env::current_exe().map_err(|_| APPCONTAINER_LAUNCH_UNAVAILABLE)?;
        let application = executable
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut command_line = OsString::from(format!(
            "\"{}\" --ignored --exact reviewed_build::tests::appcontainer_fixed_helper_writes_only_fixed_output_child",
            executable.display()
        ))
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut startup: StartupInfoExW = unsafe { std::mem::zeroed() };
        startup.startup_info.cb = std::mem::size_of::<StartupInfoExW>() as u32;
        startup.attribute_list = list_ptr;
        let mut process: ProcessInformation = unsafe { std::mem::zeroed() };
        let created = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED_TEST,
                std::ptr::null_mut(),
                producer_root
                    .path()
                    .as_os_str()
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect::<Vec<_>>()
                    .as_ptr(),
                (&mut startup.startup_info) as *mut StartupInfoW,
                &mut process,
            )
        };
        unsafe { DeleteProcThreadAttributeList(list_ptr) };
        if created == 0 || process.process.is_null() || process.thread.is_null() {
            return Err(APPCONTAINER_LAUNCH_UNAVAILABLE.into());
        }
        let job = BuildJob::new().map_err(|_| APPCONTAINER_LAUNCH_UNAVAILABLE)?;
        let membership = job.assign_raw_process_for_test(process.process);
        if membership.is_err() || !job.contains_raw_process_for_test(process.process)? {
            unsafe {
                CloseHandle(process.thread);
                CloseHandle(process.process);
            }
            return Err(APPCONTAINER_LAUNCH_UNAVAILABLE.into());
        }
        let mut token = std::ptr::null_mut();
        let token_ok = unsafe { OpenProcessToken(process.process, TOKEN_QUERY, &mut token) } != 0;
        let mut app_container = 0u32;
        let mut returned = 0u32;
        if !token_ok
            || unsafe {
                GetTokenInformation(
                    token,
                    TOKEN_IS_APPCONTAINER,
                    (&mut app_container as *mut u32).cast(),
                    std::mem::size_of::<u32>() as u32,
                    &mut returned,
                )
            } == 0
            || app_container == 0
        {
            if !token.is_null() {
                unsafe { CloseHandle(token) };
            }
            unsafe {
                CloseHandle(process.thread);
                CloseHandle(process.process);
            }
            return Err(APPCONTAINER_TOKEN_UNPROVEN.into());
        }
        let observed_app_sid = token_information(token, TOKEN_APPCONTAINER_SID)?;
        if observed_app_sid.len() < std::mem::size_of::<*mut c_void>()
            || unsafe { EqualSid(observed_app_sid.as_ptr().cast::<*mut c_void>().read(), sid) } == 0
        {
            unsafe { CloseHandle(token) };
            unsafe {
                CloseHandle(process.thread);
                CloseHandle(process.process);
            }
            return Err(APPCONTAINER_TOKEN_UNPROVEN.into());
        }
        unsafe {
            CloseHandle(token);
        }
        // The process is a real lowbox and exact Job member before resume.
        // Its only executable action is the fixed ignored unit-test helper.
        if unsafe { ResumeThread(process.thread) } == u32::MAX
            || unsafe { WaitForSingleObject(process.process, 10_000) } != WAIT_OBJECT_0
        {
            unsafe {
                CloseHandle(process.thread);
                CloseHandle(process.process);
            }
            return Err(APPCONTAINER_LAUNCH_UNAVAILABLE.into());
        }
        let output = open_relative_regular_file(
            &producer_root,
            APPCONTAINER_FIXED_OUTPUT_CHILD,
            "R5D-R2A helper output",
        )
        .map_err(|_| APPCONTAINER_OUTPUT_UNPROVEN)?;
        let mut output = output;
        let evidence = evidence_from_open_regular(&mut output, "R5D-R2A retained helper output")
            .map_err(|_| APPCONTAINER_OUTPUT_UNPROVEN)?;
        if evidence.length == 0 || evidence.sha256.len() != 64 || evidence.identity.len() != 26 {
            return Err(APPCONTAINER_OUTPUT_UNPROVEN.into());
        }
        appcontainer_interactive_attack_matrix(&producer_root, &mut output, &evidence)?;
        // DACL/owner/mandatory-label and hostile-operation inspection is kept
        // deliberately fail closed until all values are read back from real
        // handles.  A successful helper write alone is never a positive proof.
        unsafe {
            CloseHandle(process.thread);
            CloseHandle(process.process);
        }
        Err(APPCONTAINER_SECURITY_UNPROVEN.into())
    })();
    unsafe {
        FreeSid(sid);
        DeleteAppContainerProfile(profile_wide.as_ptr());
    }
    result
}

/// Performs concrete ordinary-token operations only after the fixed helper has
/// created its child and CatDesk has retained that exact file handle.  This is
/// deliberately a test-only hostile matrix: any successful mutation is an
/// error, and the production worker cannot call it.
#[cfg(all(test, windows))]
fn appcontainer_interactive_attack_matrix(
    root: &ProtectedDirectoryGuard,
    retained_output: &mut fs::File,
    expected: &OpenRegularEvidence,
) -> Result<(), String> {
    const WRITE_DAC: u32 = 0x0004_0000;
    const WRITE_OWNER: u32 = 0x0008_0000;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const INVALID_HANDLE_VALUE: *mut c_void = -1_isize as *mut c_void;
    unsafe extern "system" {
        fn CreateFileW(
            file_name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *mut c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: *mut c_void,
        ) -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    fn must_be_denied<T>(result: std::io::Result<T>) -> Result<(), String> {
        if result.is_ok() {
            Err(APPCONTAINER_SECURITY_UNPROVEN.into())
        } else {
            Ok(())
        }
    }
    root.assert_stable("R5D-R2A attack root before attacks")
        .map_err(|_| APPCONTAINER_SECURITY_UNPROVEN)?;
    let root_path = root.path();
    let output_path = root_path.join(APPCONTAINER_FIXED_OUTPUT_CHILD);
    let outside = root_path
        .parent()
        .ok_or_else(|| APPCONTAINER_SECURITY_UNPROVEN.to_string())?
        .join(format!("outside-r5d-r2a-{}", Uuid::new_v4()));
    fs::create_dir(&outside).map_err(|_| APPCONTAINER_SECURITY_UNPROVEN)?;
    let sentinel = outside.join("sentinel.bin");
    fs::write(&sentinel, b"outside-sentinel-r5d-r2a")
        .map_err(|_| APPCONTAINER_SECURITY_UNPROVEN)?;
    let sentinel_before = fs::read(&sentinel).map_err(|_| APPCONTAINER_SECURITY_UNPROVEN)?;

    must_be_denied(OpenOptions::new().write(true).open(&output_path))?;
    must_be_denied(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root_path.join("attacker-extra-child")),
    )?;
    must_be_denied(fs::create_dir(root_path.join("attacker-extra-directory")))?;
    must_be_denied(fs::remove_file(&output_path))?;
    must_be_denied(fs::rename(
        &output_path,
        root_path.join("attacker-moved-output"),
    ))?;
    must_be_denied(fs::rename(
        root_path,
        root_path.with_extension("attacker-moved-root"),
    ))?;

    let attacker = outside.join("same-length-attacker.bin");
    fs::write(&attacker, vec![b'X'; expected.length as usize])
        .map_err(|_| APPCONTAINER_SECURITY_UNPROVEN)?;
    must_be_denied(fs::rename(&attacker, &output_path))?;
    must_be_denied(std::os::windows::fs::symlink_file(
        &sentinel,
        root_path.join("attacker-reparse"),
    ))?;

    let root_wide = root_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    for right in [WRITE_DAC, WRITE_OWNER] {
        let handle = unsafe {
            CreateFileW(
                root_wide.as_ptr(),
                right,
                1,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(handle);
            }
            return Err(APPCONTAINER_SECURITY_UNPROVEN.into());
        }
    }

    let actual =
        evidence_from_open_regular(retained_output, "R5D-R2A retained output after attacks")
            .map_err(|_| APPCONTAINER_SECURITY_UNPROVEN)?;
    if &actual != expected
        || fs::read(&sentinel).map_err(|_| APPCONTAINER_SECURITY_UNPROVEN)? != sentinel_before
    {
        return Err(APPCONTAINER_SECURITY_UNPROVEN.into());
    }
    root.assert_stable("R5D-R2A attack root after attacks")
        .map_err(|_| APPCONTAINER_SECURITY_UNPROVEN.to_string())
}

/// Test-only schema for a future internal CatDesk linker-broker mode. It has
/// no argv parser and no production call site: the current policy cannot
/// select an attested final linker, so exposing a helper mode would create an
/// unsafe protocol surface rather than a provenance mechanism.
#[cfg(all(test, windows))]
struct LinkerBrokerCapabilityV1 {
    session_id: String,
    build_attempt_id: String,
    parent_pid: u32,
    broker_pid: u32,
    nonce: String,
    output_token: String,
    broker_identity: String,
    consumed: bool,
}

#[cfg(all(test, windows))]
struct LinkerBrokerHandoffV1 {
    schema_version: u8,
    session_id: String,
    build_attempt_id: String,
    parent_pid: u32,
    broker_pid: u32,
    linker_pid: u32,
    nonce: String,
    output_token: String,
    broker_identity: String,
    linker_identity: String,
    linker_in_exact_job: bool,
    output_identity: String,
}

#[cfg(all(test, windows))]
impl LinkerBrokerCapabilityV1 {
    fn new(
        session_id: &str,
        build_attempt_id: &str,
        parent_pid: u32,
        broker_pid: u32,
        broker_identity: &str,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            build_attempt_id: build_attempt_id.into(),
            parent_pid,
            broker_pid,
            nonce: Uuid::new_v4().simple().to_string(),
            output_token: Uuid::new_v4().simple().to_string(),
            broker_identity: broker_identity.into(),
            consumed: false,
        }
    }

    /// A valid-looking record still fails because R5C has no policy-forced,
    /// attested linker or producer-owned output-handle capability to bind.
    fn validate_without_output_handle(
        &mut self,
        handoff: &LinkerBrokerHandoffV1,
    ) -> Result<(), String> {
        if self.consumed {
            return Err(PRODUCER_HANDOFF_REPLAYED.into());
        }
        if handoff.schema_version != 1
            || handoff.session_id != self.session_id
            || handoff.build_attempt_id != self.build_attempt_id
            || handoff.parent_pid != self.parent_pid
            || handoff.broker_pid != self.broker_pid
            || handoff.nonce != self.nonce
            || handoff.output_token != self.output_token
            || handoff.broker_identity != self.broker_identity
            || handoff.linker_pid == 0
            || handoff.linker_identity.is_empty()
            || handoff.output_identity.is_empty()
            || !handoff.linker_in_exact_job
        {
            return Err(BROKER_HANDOFF_REJECTED.into());
        }
        // Do not consume a plausible record: no exact kernel handle was
        // transferred and no policy forced the advertised linker identity.
        Err(PRODUCER_HANDLE_NOT_CAPTURED.into())
    }
}

#[cfg(all(test, windows))]
fn policy_forced_final_linker_for_feasibility(
    _cargo: &TrustedBuildToolV1,
    _rustc: &TrustedBuildToolV1,
) -> Result<TrustedBuildToolV1, String> {
    // Rustc/linker choice is not persisted by T-0195. PATH/Cargo prose,
    // workspace configuration, and registry-wide discovery are forbidden.
    Err(LINKER_UNAVAILABLE.into())
}

#[cfg(all(test, windows))]
struct ProducerHandleFeasibilityHandoff {
    nonce: String,
    expected_pid: u32,
    expected_identity: String,
    consumed: bool,
}

#[cfg(all(test, windows))]
impl ProducerHandleFeasibilityHandoff {
    fn new(expected_pid: u32, expected_identity: String) -> Self {
        Self {
            nonce: Uuid::new_v4().simple().to_string(),
            expected_pid,
            expected_identity,
            consumed: false,
        }
    }

    /// This accepts only a test fixture's explicitly-proven owner-handle fact.
    /// Windows Job membership alone cannot prove that a child owns a particular
    /// file handle, so a real probe always supplies `false` until a reviewed
    /// process-handle inspection primitive can establish it.
    fn capture(
        &mut self,
        nonce: &str,
        observed_pid: u32,
        is_job_member: bool,
        owner_handle_proven: bool,
        source: &fs::File,
    ) -> Result<fs::File, String> {
        if self.consumed {
            return Err(PRODUCER_HANDOFF_REPLAYED.into());
        }
        if nonce != self.nonce {
            return Err(PRODUCER_HANDOFF_REJECTED.into());
        }
        if observed_pid != self.expected_pid || !is_job_member {
            return Err(PRODUCER_PROCESS_UNTRUSTED.into());
        }
        if !owner_handle_proven {
            return Err(PRODUCER_HANDLE_UNSAFE.into());
        }
        if opened_regular_identity(source, "producer handoff source")? != self.expected_identity {
            return Err(PRODUCER_IDENTITY_MISMATCH.into());
        }
        let duplicate = duplicate_file_handle_for_feasibility(source)?;
        if opened_regular_identity(&duplicate, "producer handoff duplicate")?
            != self.expected_identity
        {
            return Err(PRODUCER_IDENTITY_MISMATCH.into());
        }
        self.consumed = true;
        Ok(duplicate)
    }
}

#[cfg(all(test, windows))]
fn duplicate_file_handle_for_feasibility(source: &fs::File) -> Result<fs::File, String> {
    const DUPLICATE_SAME_ACCESS: u32 = 0x0000_0002;
    const FILE_TYPE_DISK: u32 = 1;
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn DuplicateHandle(
            source_process: *mut c_void,
            source_handle: *mut c_void,
            target_process: *mut c_void,
            target_handle: *mut *mut c_void,
            desired_access: u32,
            inherit: i32,
            options: u32,
        ) -> i32;
        fn GetFileType(handle: *mut c_void) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    let process = unsafe { GetCurrentProcess() };
    let mut duplicate = std::ptr::null_mut();
    if unsafe {
        DuplicateHandle(
            process,
            source.as_raw_handle().cast(),
            process,
            &mut duplicate,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
        || duplicate.is_null()
    {
        return Err(PRODUCER_HANDLE_NOT_CAPTURED.into());
    }
    if unsafe { GetFileType(duplicate) } != FILE_TYPE_DISK {
        unsafe {
            CloseHandle(duplicate);
        }
        return Err(PRODUCER_HANDLE_UNSAFE.into());
    }
    // SAFETY: DuplicateHandle returned a live owned kernel file handle.
    Ok(unsafe { fs::File::from_raw_handle(duplicate) })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedBuildToolV1 {
    pub absolute_path: String,
    pub sha256: String,
    pub length: u64,
    pub identity: String,
    pub version_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedBuildPolicyV1 {
    pub version: String,
    pub policy_sha256: String,
    pub argv: Vec<String>,
    pub argv_sha256: String,
    pub environment_policy_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedBuildAttestationV2 {
    pub schema_version: u8,
    pub producer: String,
    pub build_attempt_id: String,
    pub review_session_id: String,
    pub review_record_id: String,
    pub review_authority_sha256: String,
    pub snapshot_id: String,
    pub snapshot_authority_digest: String,
    pub snapshot_manifest_digest: String,
    pub build_policy_sha256: String,
    pub build_argv_sha256: String,
    pub environment_policy_sha256: String,
    pub cargo_path: String,
    pub cargo_sha256: String,
    pub cargo_identity: String,
    pub cargo_version_sha256: String,
    pub rustc_path: String,
    pub rustc_sha256: String,
    pub rustc_identity: String,
    pub rustc_version_sha256: String,
    pub candidate_relative_path: String,
    pub candidate_sha256: String,
    pub candidate_length: u64,
    pub candidate_identity: String,
    pub generation: u64,
    pub created_at_unix: u64,
    pub attestation_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewedBuildAttemptV1 {
    schema_version: u8,
    pub(crate) build_attempt_id: String,
    confirmation_token: String,
    review_session_id: String,
    review_record_id: String,
    review_authority_sha256: String,
    snapshot_expected: ReviewedSourceSnapshotExpectedV1,
    snapshot_id: String,
    snapshot_authority_digest: String,
    snapshot_manifest_digest: String,
    cargo: TrustedBuildToolV1,
    rustc: TrustedBuildToolV1,
    policy: ReviewedBuildPolicyV1,
    attempt_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewedBuildClaimV1 {
    schema_version: u8,
    build_attempt_id: String,
    owner_id: String,
    spawn_generation: u64,
    state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewedBuildOwnerProofV1 {
    schema_version: u8,
    build_attempt_id: String,
    owner_id: String,
    spawn_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewedBuildResultV1 {
    schema_version: u8,
    build_attempt_id: String,
    owner_id: String,
    state: String,
    attestation_digest: Option<String>,
    failure_code: Option<String>,
    #[serde(default)]
    failure_diagnostic: Option<ReviewedBuildFailureDiagnosticV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewedBuildFailureDiagnosticV1 {
    schema_version: u8,
    phase: String,
    exit_code: Option<i32>,
    classification: String,
    captured_stderr_sha256: String,
    captured_stderr_length: usize,
    stderr_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    missing_link_library: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    missing_link_input: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CargoFailureClassification {
    DependencyNetworkUnavailable,
    LockfileOutOfDate,
    LinkerNotFound,
    LinkLibraryNotFound,
    LinkInputNotFound,
    LinkUnresolvedExternal,
    LinkPdbFailure,
    LinkOutOfMemory,
    MsvcTemporaryIlFailure,
    LinkFailed,
    CompilationFailed,
    ExitNonzero,
}

impl CargoFailureClassification {
    fn as_str(self) -> &'static str {
        match self {
            Self::DependencyNetworkUnavailable => "CARGO_DEPENDENCY_NETWORK_UNAVAILABLE",
            Self::LockfileOutOfDate => "CARGO_LOCKFILE_OUT_OF_DATE",
            Self::LinkerNotFound => "CARGO_LINKER_NOT_FOUND",
            Self::LinkLibraryNotFound => "CARGO_LINK_LIBRARY_NOT_FOUND",
            Self::LinkInputNotFound => "CARGO_LINK_INPUT_NOT_FOUND",
            Self::LinkUnresolvedExternal => "CARGO_LINK_UNRESOLVED_EXTERNAL",
            Self::LinkPdbFailure => "CARGO_LINK_PDB_FAILURE",
            Self::LinkOutOfMemory => "CARGO_LINK_OUT_OF_MEMORY",
            Self::MsvcTemporaryIlFailure => "CARGO_MSVC_TEMP_IL_FILE_FAILURE",
            Self::LinkFailed => "CARGO_LINK_FAILED",
            Self::CompilationFailed => "CARGO_COMPILATION_FAILED",
            Self::ExitNonzero => "CARGO_EXIT_NONZERO",
        }
    }
}

#[derive(Default)]
struct CargoStderrClassifier {
    // This is an ephemeral cross-chunk matcher buffer, never returned or
    // persisted. It exists only to recognize a fixed signature split across
    // pipe reads, and is cleared when classification is produced.
    tail: Vec<u8>,
    saw_registry: bool,
    saw_network_failure: bool,
    saw_lockfile: bool,
    saw_lockfile_update: bool,
    saw_locked: bool,
    saw_linker: bool,
    saw_link_failure: bool,
    saw_linker_not_found: bool,
    saw_link_input_not_found: bool,
    missing_link_input: Option<String>,
    saw_link_unresolved_external: bool,
    saw_link_pdb_failure: bool,
    saw_link_out_of_memory: bool,
    saw_msvc_d8037: bool,
    saw_compile: bool,
    // Manual host-linker diagnostics may retain only this fixed bitset. It is
    // deliberately test-only: production classification and persisted
    // reviewed-build diagnostics remain unchanged.
    #[cfg(all(test, windows))]
    diagnostic_linker_error_codes: u8,
    #[cfg(all(test, windows))]
    diagnostic_cargo_failure_signals: u16,
    #[cfg(all(test, windows))]
    diagnostic_custom_build_crate: Option<String>,
    #[cfg(all(test, windows))]
    diagnostic_custom_build_version: Option<String>,
}

#[cfg(all(test, windows))]
macro_rules! observe_fixed_linker_error_codes {
    ($classifier:expr, $normalized:expr) => {
        $classifier.diagnostic_linker_error_codes |= fixed_linker_error_code_mask($normalized);
    };
}

#[cfg(not(all(test, windows)))]
macro_rules! observe_fixed_linker_error_codes {
    ($classifier:expr, $normalized:expr) => {};
}

#[cfg(all(test, windows))]
macro_rules! observe_fixed_cargo_failure_signals {
    ($classifier:expr, $normalized:expr) => {
        $classifier.diagnostic_cargo_failure_signals |=
            fixed_cargo_failure_signal_mask($normalized);
    };
}

#[cfg(not(all(test, windows)))]
macro_rules! observe_fixed_cargo_failure_signals {
    ($classifier:expr, $normalized:expr) => {};
}

impl CargoStderrClassifier {
    fn observe(&mut self, bytes: &[u8]) {
        let mut normalized = Vec::with_capacity(self.tail.len() + bytes.len());
        normalized.extend_from_slice(&self.tail);
        normalized.extend(bytes.iter().map(u8::to_ascii_lowercase));

        self.saw_registry |= contains_ascii(&normalized, b"index.crates.io");
        self.saw_network_failure |= contains_any_ascii(
            &normalized,
            &[
                b"could not connect",
                b"network failure",
                b"failed to download",
            ],
        );
        self.saw_lockfile |= contains_ascii(&normalized, b"lock file");
        self.saw_lockfile_update |= contains_ascii(&normalized, b"needs to be updated");
        self.saw_locked |= contains_ascii(&normalized, b"--locked");
        self.saw_linker |= contains_ascii(&normalized, b"linking with ");
        self.saw_link_failure |= contains_any_ascii(&normalized, &[b"failed", b"exit code"]);
        self.saw_linker_not_found |= contains_any_ascii(
            &normalized,
            &[
                b"linker `link.exe` not found",
                b"linker 'link.exe' not found",
            ],
        );
        self.saw_link_input_not_found |= contains_any_ascii(&normalized, &[b"lnk1104", b"lnk1181"]);
        if self.missing_link_input.is_none() && self.saw_link_input_not_found {
            self.missing_link_input = extract_missing_link_input_basename(&normalized);
        }
        self.saw_link_unresolved_external |=
            contains_any_ascii(&normalized, &[b"lnk2001", b"lnk2019", b"lnk1120"]);
        self.saw_link_pdb_failure |= contains_ascii(&normalized, b"lnk1318");
        self.saw_link_out_of_memory |= contains_ascii(&normalized, b"lnk1102");
        // D8037 can appear after the bounded retained prefix. Recognize the
        // fixed MSVC code across pipe chunks, never retain the diagnostic line.
        self.saw_msvc_d8037 |= contains_ascii(&normalized, b"error d8037");
        self.saw_compile |= contains_ascii(&normalized, b"could not compile ");
        observe_fixed_linker_error_codes!(self, &normalized);
        observe_fixed_cargo_failure_signals!(self, &normalized);
        #[cfg(all(test, windows))]
        if self.diagnostic_custom_build_crate.is_none() {
            if let Some((name, version)) = extract_custom_build_package(&normalized) {
                self.diagnostic_custom_build_crate = Some(name);
                self.diagnostic_custom_build_version = version;
            }
        }

        let retained = normalized.len().min(CARGO_STDERR_CLASSIFIER_TAIL_BYTES);
        self.tail.clear();
        self.tail
            .extend_from_slice(&normalized[normalized.len().saturating_sub(retained)..]);
    }

    fn finish(mut self) -> CargoFailureClassification {
        self.tail.fill(0);
        self.tail.clear();
        if self.saw_registry && self.saw_network_failure {
            CargoFailureClassification::DependencyNetworkUnavailable
        } else if self.saw_lockfile && self.saw_lockfile_update && self.saw_locked {
            CargoFailureClassification::LockfileOutOfDate
        } else if self.saw_linker_not_found {
            CargoFailureClassification::LinkerNotFound
        } else if self.saw_link_input_not_found {
            match self.missing_link_input.as_deref() {
                Some(value) if value.ends_with(".lib") => {
                    CargoFailureClassification::LinkLibraryNotFound
                }
                _ => CargoFailureClassification::LinkInputNotFound,
            }
        } else if self.saw_link_unresolved_external {
            CargoFailureClassification::LinkUnresolvedExternal
        } else if self.saw_link_pdb_failure {
            CargoFailureClassification::LinkPdbFailure
        } else if self.saw_link_out_of_memory {
            CargoFailureClassification::LinkOutOfMemory
        } else if self.saw_msvc_d8037 {
            CargoFailureClassification::MsvcTemporaryIlFailure
        } else if self.saw_linker && self.saw_link_failure {
            CargoFailureClassification::LinkFailed
        } else if self.saw_compile {
            CargoFailureClassification::CompilationFailed
        } else {
            CargoFailureClassification::ExitNonzero
        }
    }

    #[cfg(all(test, windows))]
    fn fixed_linker_error_codes(&self) -> Vec<&'static str> {
        const CODES: [(u8, &str); 7] = [
            (1, "LNK1102"),
            (2, "LNK1104"),
            (4, "LNK1181"),
            (8, "LNK2001"),
            (16, "LNK2019"),
            (32, "LNK1120"),
            (64, "LNK1318"),
        ];
        CODES
            .into_iter()
            .filter_map(|(mask, code)| {
                (self.diagnostic_linker_error_codes & mask != 0).then_some(code)
            })
            .collect()
    }

    #[cfg(all(test, windows))]
    fn fixed_cargo_failure_signals(&self) -> Vec<&'static str> {
        const SIGNALS: [(u16, &str); 9] = [
            (1, "CUSTOM_BUILD_COMMAND_FAILED"),
            (2, "COULD_NOT_COMPILE"),
            (4, "PROCESS_EXITED_UNSUCCESSFULLY"),
            (8, "RUSTC_LAUNCH_FAILED"),
            (16, "OFFLINE_PACKAGE_MISSING"),
            (32, "MANIFEST_FAILURE"),
            (64, "ARCHIVE_UNPACK_FAILURE"),
            (128, "PANIC"),
            (256, "LINKING_WITH"),
        ];
        SIGNALS
            .into_iter()
            .filter_map(|(mask, signal)| {
                (self.diagnostic_cargo_failure_signals & mask != 0).then_some(signal)
            })
            .collect()
    }
}

#[cfg(all(test, windows))]
fn extract_custom_build_package(bytes: &[u8]) -> Option<(String, Option<String>)> {
    let marker = b"failed to run custom build command for `";
    let start = bytes
        .windows(marker.len())
        .position(|window| window == marker)?
        + marker.len();
    let rest = &bytes[start..];
    let end = rest.iter().position(|byte| *byte == b'`')?;
    let inside = std::str::from_utf8(&rest[..end]).ok()?;
    let mut parts = inside.split_ascii_whitespace();
    let name = parts.next()?;
    if name.is_empty()
        || name.len() > 96
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return None;
    }
    let version = parts.next().and_then(|value| {
        let raw = value.strip_prefix('v')?;
        (!raw.is_empty()
            && raw.len() <= 48
            && raw.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+' | b'_')
            }))
        .then(|| raw.to_string())
    });
    Some((name.to_string(), version))
}

#[cfg(all(test, windows))]
fn fixed_cargo_failure_signal_mask(bytes: &[u8]) -> u16 {
    [
        (b"failed to run custom build command for".as_slice(), 1),
        (b"could not compile".as_slice(), 2),
        (b"process didn't exit successfully".as_slice(), 4),
        (b"failed to run `rustc`".as_slice(), 8),
        (b"no matching package named".as_slice(), 16),
        (b"failed to parse manifest".as_slice(), 32),
        (b"failed to unpack".as_slice(), 64),
        (b"panicked at".as_slice(), 128),
        (b"linking with ".as_slice(), 256),
    ]
    .into_iter()
    .filter_map(|(signature, mask)| contains_ascii(bytes, signature).then_some(mask))
    .fold(0, |mask, matched| mask | matched)
}

#[cfg(all(test, windows))]
fn fixed_linker_error_code_mask(bytes: &[u8]) -> u8 {
    [
        (b"lnk1102".as_slice(), 1),
        (b"lnk1104".as_slice(), 2),
        (b"lnk1181".as_slice(), 4),
        (b"lnk2001".as_slice(), 8),
        (b"lnk2019".as_slice(), 16),
        (b"lnk1120".as_slice(), 32),
        (b"lnk1318".as_slice(), 64),
    ]
    .into_iter()
    .filter_map(|(signature, mask)| contains_ascii(bytes, signature).then_some(mask))
    .fold(0, |mask, matched| mask | matched)
}

fn contains_ascii(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn contains_any_ascii(haystack: &[u8], needles: &[&[u8]]) -> bool {
    needles
        .iter()
        .any(|needle| contains_ascii(haystack, needle))
}

fn extract_missing_link_input_basename(bytes: &[u8]) -> Option<String> {
    const SAFE_EXTENSIONS: &[&[u8]] = &[
        b".lib", b".obj", b".pdb", b".ilk", b".exe", b".dll", b".exp", b".res",
    ];
    for marker in [
        b"cannot open file '".as_slice(),
        b"cannot open input file '".as_slice(),
        b"cannot open file \"".as_slice(),
        b"cannot open input file \"".as_slice(),
    ] {
        let Some(start) = bytes
            .windows(marker.len())
            .position(|window| window == marker)
        else {
            continue;
        };
        let quote = marker[marker.len() - 1];
        let value = &bytes[start + marker.len()..];
        let Some(end) = value.iter().position(|byte| *byte == quote) else {
            continue;
        };
        let raw = &value[..end];
        let basename = raw
            .rsplit(|byte| *byte == b'\\' || *byte == b'/')
            .next()
            .unwrap_or_default();
        if basename.is_empty()
            || basename.len() > MAX_LINK_LIBRARY_BASENAME_BYTES
            || !basename
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'-'))
            || !SAFE_EXTENSIONS
                .iter()
                .any(|extension| basename.ends_with(extension))
        {
            continue;
        }
        return String::from_utf8(basename.to_vec()).ok();
    }
    None
}

struct BoundedCargoStderr {
    captured: Vec<u8>,
    truncated: bool,
    classification: CargoFailureClassification,
    missing_link_library: Option<String>,
    missing_link_input: Option<String>,
    #[cfg(all(test, windows))]
    diagnostic_linker_error_codes: Vec<&'static str>,
    #[cfg(all(test, windows))]
    diagnostic_cargo_failure_signals: Vec<&'static str>,
    #[cfg(all(test, windows))]
    diagnostic_custom_build_crate: Option<String>,
    #[cfg(all(test, windows))]
    diagnostic_custom_build_version: Option<String>,
}

impl Default for BoundedCargoStderr {
    fn default() -> Self {
        Self {
            captured: Vec::new(),
            truncated: false,
            classification: CargoFailureClassification::ExitNonzero,
            missing_link_library: None,
            missing_link_input: None,
            #[cfg(all(test, windows))]
            diagnostic_linker_error_codes: Vec::new(),
            #[cfg(all(test, windows))]
            diagnostic_cargo_failure_signals: Vec::new(),
            #[cfg(all(test, windows))]
            diagnostic_custom_build_crate: None,
            #[cfg(all(test, windows))]
            diagnostic_custom_build_version: None,
        }
    }
}

fn read_bounded_cargo_stderr(mut reader: impl Read) -> std::io::Result<BoundedCargoStderr> {
    let mut captured = Vec::with_capacity(MAX_CARGO_FAILURE_DIAGNOSTIC_BYTES);
    let mut buffer = [0u8; 1024];
    let mut truncated = false;
    let mut classifier = CargoStderrClassifier::default();
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        classifier.observe(&buffer[..read]);
        let remaining = MAX_CARGO_FAILURE_DIAGNOSTIC_BYTES.saturating_sub(captured.len());
        let retained = read.min(remaining);
        captured.extend_from_slice(&buffer[..retained]);
        truncated |= retained != read;
    }
    let missing_link_input = classifier.missing_link_input.clone();
    let missing_link_library = missing_link_input
        .as_ref()
        .filter(|value| value.ends_with(".lib"))
        .cloned();
    #[cfg(all(test, windows))]
    let diagnostic_linker_error_codes = classifier.fixed_linker_error_codes();
    #[cfg(all(test, windows))]
    let diagnostic_cargo_failure_signals = classifier.fixed_cargo_failure_signals();
    #[cfg(all(test, windows))]
    let diagnostic_custom_build_crate = classifier.diagnostic_custom_build_crate.clone();
    #[cfg(all(test, windows))]
    let diagnostic_custom_build_version = classifier.diagnostic_custom_build_version.clone();
    let classification = classifier.finish();
    Ok(BoundedCargoStderr {
        captured,
        truncated,
        classification,
        missing_link_library,
        missing_link_input,
        #[cfg(all(test, windows))]
        diagnostic_linker_error_codes,
        #[cfg(all(test, windows))]
        diagnostic_cargo_failure_signals,
        #[cfg(all(test, windows))]
        diagnostic_custom_build_crate,
        #[cfg(all(test, windows))]
        diagnostic_custom_build_version,
    })
}

fn summarize_cargo_failure(
    exit_code: Option<i32>,
    captured_stderr: BoundedCargoStderr,
) -> ReviewedBuildFailureDiagnosticV1 {
    ReviewedBuildFailureDiagnosticV1 {
        schema_version: 1,
        phase: "CARGO_BUILD".into(),
        exit_code,
        classification: captured_stderr.classification.as_str().into(),
        captured_stderr_sha256: sha256(&captured_stderr.captured),
        captured_stderr_length: captured_stderr.captured.len(),
        stderr_truncated: captured_stderr.truncated,
        missing_link_library: captured_stderr.missing_link_library,
        missing_link_input: captured_stderr.missing_link_input,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewedBuildActiveGenerationV1 {
    schema_version: u8,
    source_attempt_id: String,
    source_attempt_digest: String,
    active_attempt_id: String,
    active_attempt_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewedBuildTerminalAuditV1 {
    schema_version: u8,
    attempt: ReviewedBuildAttemptV1,
    claim: ReviewedBuildClaimV1,
    owner: ReviewedBuildOwnerProofV1,
    result: ReviewedBuildResultV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewedBuildRetryPlanV1 {
    schema_version: u8,
    source_attempt_id: String,
    source_attempt_digest: String,
    audit_digest: String,
    fresh_attempt_id: String,
    fresh_attempt_digest: String,
    fresh_confirmation_token: String,
}

/// Holds the R7C pinned directory chain for the entire control-plane read or
/// mutation.  Dropping it only happens after state files have been consumed or
/// written, so `.catdesk` and `reviewed-build-control` cannot be redirected
/// between parent classification and operation.
struct BuildControlRoot {
    guard: ProtectedDirectoryGuard,
    base_guard: ProtectedDirectoryGuard,
}
impl std::ops::Deref for BuildControlRoot {
    type Target = Path;
    fn deref(&self) -> &Self::Target {
        self.guard.path()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewedBuildPublicOutcomeV1 {
    pub state: &'static str,
}

/// Fully recompute the producer evidence for promotion.  This intentionally
/// rejects old/schema-only attestations and never accepts a caller digest.
pub fn validate_producer_attestation(
    workspace: &Path,
    review_session_id: &str,
    review_record_id: &str,
    review_authority_sha256: &str,
    expected: &ReviewedSourceSnapshotExpectedV1,
    candidate_relative_path: &str,
    candidate_sha256: &str,
) -> Result<ReviewedBuildAttestationV2, String> {
    if !valid_sha256(review_authority_sha256)
        || !valid_sha256(candidate_sha256)
        || !safe_candidate_relative_path(candidate_relative_path)
        || expected.session_id != review_session_id
    {
        return Err("REVIEWED_BUILD_ATTESTATION_UNAVAILABLE".into());
    }
    let control = existing_control_root(workspace)?;
    let attempt: ReviewedBuildAttemptV1 = control_read_json(&control, "attempt.json")?;
    validate_attempt(
        workspace,
        &attempt,
        review_record_id,
        review_authority_sha256,
        expected,
    )?;
    let result: ReviewedBuildResultV1 = control_read_json(&control, "result.json")?;
    if result.schema_version != RESULT_SCHEMA_VERSION
        || result.build_attempt_id != attempt.build_attempt_id
        || result.state != "BUILD_ATTESTED"
    {
        return Err("REVIEWED_BUILD_ATTESTATION_UNAVAILABLE".into());
    }
    let claim: ReviewedBuildClaimV1 = control_read_json(&control, "claim.json")?;
    if !valid_claim(&claim, &attempt)
        || claim.owner_id != result.owner_id
        || !control_has_valid_owner_proof(&control, &claim)?
    {
        return Err("REVIEWED_BUILD_ATTESTATION_UNAVAILABLE".into());
    }
    let attestation: ReviewedBuildAttestationV2 = control_read_json(&control, "attestation.json")?;
    validate_attestation(
        &attestation,
        &attempt,
        candidate_relative_path,
        candidate_sha256,
    )?;
    if result.attestation_digest.as_deref() != Some(attestation.attestation_digest.as_str()) {
        return Err("REVIEWED_BUILD_ATTESTATION_UNAVAILABLE".into());
    }
    let (_candidate_parent, mut candidate) =
        open_candidate_for_replay(workspace, &attestation.build_attempt_id)?;
    let actual = evidence_from_open_regular(&mut candidate, "reviewed build candidate")?;
    if actual.sha256 != attestation.candidate_sha256
        || actual.length != attestation.candidate_length
        || actual.identity != attestation.candidate_identity
    {
        return Err("REVIEWED_BUILD_ATTESTATION_UNAVAILABLE".into());
    }
    Ok(attestation)
}

pub fn prepare_reviewed_build(
    workspace: &Path,
    review_record_id: &str,
    review_authority_sha256: &str,
    expected: &ReviewedSourceSnapshotExpectedV1,
) -> Result<(String, ReviewedBuildPublicOutcomeV1), String> {
    if review_record_id.is_empty()
        || review_record_id.len() > 128
        || !valid_sha256(review_authority_sha256)
    {
        return Err("REVIEWED_BUILD_AUTHORITY_UNAVAILABLE".into());
    }
    let snapshot = validate_committed_snapshot(workspace, expected)
        .map_err(|_| "REVIEWED_SOURCE_SNAPSHOT_REQUIRED".to_string())?;
    let policy = fixed_policy();
    let (cargo, rustc) = trusted_toolchain()?;
    let control = control_root(workspace)?;
    let mut attempt = ReviewedBuildAttemptV1 {
        schema_version: ATTEMPT_SCHEMA_VERSION,
        build_attempt_id: Uuid::new_v4().simple().to_string(),
        confirmation_token: Uuid::new_v4().simple().to_string(),
        review_session_id: expected.session_id.clone(),
        review_record_id: review_record_id.into(),
        review_authority_sha256: review_authority_sha256.to_ascii_lowercase(),
        snapshot_expected: expected.clone(),
        snapshot_id: snapshot.snapshot_id.clone(),
        snapshot_authority_digest: snapshot.authority_digest.clone(),
        snapshot_manifest_digest: snapshot.manifest_digest.clone(),
        cargo: cargo.clone(),
        rustc: rustc.clone(),
        policy: policy.clone(),
        attempt_digest: String::new(),
    };
    attempt.attempt_digest = digest_attempt(&attempt)?;
    match control_create_json(&control, "attempt.json", &attempt) {
        Ok(()) => Ok((
            attempt.confirmation_token,
            ReviewedBuildPublicOutcomeV1 { state: "PREPARED" },
        )),
        Err(_) => {
            let existing: ReviewedBuildAttemptV1 = control_read_json(&control, "attempt.json")?;
            if exact_attempt_matches(
                &existing,
                review_record_id,
                review_authority_sha256,
                expected,
                &snapshot,
                &cargo,
                &rustc,
                &policy,
            ) {
                if unclaimed_prepared_attempt(&control, &existing)? {
                    Ok((
                        existing.confirmation_token,
                        ReviewedBuildPublicOutcomeV1 { state: "PREPARED" },
                    ))
                } else {
                    match terminal_failure_audit(&control, &existing) {
                        Ok(audit) => prepare_terminal_retry(&control, &existing, audit, attempt),
                        Err("REVIEWED_BUILD_NOT_TERMINAL") => Ok((
                            existing.confirmation_token,
                            ReviewedBuildPublicOutcomeV1 { state: "PREPARED" },
                        )),
                        Err(_) => Err("REVIEWED_BUILD_BINDING_IMMUTABLE".into()),
                    }
                }
            } else {
                // A different acknowledged binding may supersede an exact
                // active family only before any worker ownership/execution
                // evidence exists, or after the existing fully-audited
                // retryable terminal failure path. The prior attempt remains
                // immutable in both cases.
                validate_historical_terminal_attempt(workspace, &existing)
                    .map_err(|_| "REVIEWED_BUILD_BINDING_IMMUTABLE".to_string())?;
                if unclaimed_prepared_attempt(&control, &existing)? {
                    prepare_unclaimed_prepared_supersession(&control, &existing, attempt)
                } else {
                    terminal_failure_audit(&control, &existing)
                        .map_err(|_| "REVIEWED_BUILD_BINDING_IMMUTABLE".to_string())
                        .and_then(|audit| {
                            prepare_terminal_retry(&control, &existing, audit, attempt)
                        })
                }
            }
        }
    }
}

pub fn confirm_reviewed_build(
    workspace: &Path,
    confirmation_token: &str,
    spawn: impl FnOnce(&ReviewedBuildAttemptV1, &str) -> Result<(), String>,
) -> Result<ReviewedBuildPublicOutcomeV1, String> {
    let control = control_root(workspace)?;
    let attempt: ReviewedBuildAttemptV1 = control_read_json(&control, "attempt.json")?;
    if attempt.confirmation_token != confirmation_token {
        return Err("REVIEWED_BUILD_CONFIRMATION_UNAVAILABLE".into());
    }
    validate_attempt(
        workspace,
        &attempt,
        &attempt.review_record_id,
        &attempt.review_authority_sha256,
        &attempt.snapshot_expected,
    )?;
    let owner_id = Uuid::new_v4().simple().to_string();
    let claim = ReviewedBuildClaimV1 {
        schema_version: CLAIM_SCHEMA_VERSION,
        build_attempt_id: attempt.build_attempt_id.clone(),
        owner_id: owner_id.clone(),
        spawn_generation: 1,
        // This durable reservation is already a valid owner record.  The
        // helper receives the exact owner token and must atomically prove it
        // before source/target/candidate mutation.  We never publish an
        // invalid intermediate state to a fast helper.
        state: "SPAWN_OWNER_RESERVED".into(),
    };
    match control_create_json(&control, "claim.json", &claim) {
        Ok(()) => match spawn(&attempt, &owner_id) {
            Ok(()) => Ok(ReviewedBuildPublicOutcomeV1 {
                state: "NEWLY_SCHEDULED",
            }),
            Err(_) => {
                let result = ReviewedBuildResultV1 {
                    schema_version: RESULT_SCHEMA_VERSION,
                    build_attempt_id: attempt.build_attempt_id,
                    owner_id,
                    state: "BUILD_FAILED_OR_AMBIGUOUS".into(),
                    attestation_digest: None,
                    failure_code: Some("SPAWN_FAILED".into()),
                    failure_diagnostic: None,
                };
                let _ = control_create_json(&control, "result.json", &result);
                Err("REVIEWED_BUILD_SPAWN_UNAVAILABLE".into())
            }
        },
        Err(_) => outcome_from_existing_state(&control, &attempt),
    }
}

pub fn reviewed_build_result(workspace: &Path) -> Result<ReviewedBuildPublicOutcomeV1, String> {
    let control = existing_control_root(workspace)?;
    let attempt: ReviewedBuildAttemptV1 = control_read_json(&control, "attempt.json")?;
    let outcome = outcome_from_existing_state(&control, &attempt)?;
    if outcome.state == "BUILD_ATTESTED" {
        let attestation: ReviewedBuildAttestationV2 =
            control_read_json(&control, "attestation.json")?;
        validate_producer_attestation(
            workspace,
            &attempt.review_session_id,
            &attempt.review_record_id,
            &attempt.review_authority_sha256,
            &attempt.snapshot_expected,
            &attestation.candidate_relative_path,
            &attestation.candidate_sha256,
        )?;
    }
    Ok(outcome)
}

/// The worker is invoked only by the closed supervisor helper.  It rebuilds all
/// preconditions and writes a terminal failure record on every post-claim error
/// so restart cannot silently mint a second owner.
pub fn run_reviewed_build_worker(
    workspace: &Path,
    build_attempt_id: &str,
    owner_id: &str,
) -> Result<(), String> {
    let control = existing_control_root(workspace)?;
    let attempt: ReviewedBuildAttemptV1 = control_read_json(&control, "attempt.json")?;
    let claim: ReviewedBuildClaimV1 = control_read_json(&control, "claim.json")?;
    if attempt.build_attempt_id != build_attempt_id
        || !valid_claim(&claim, &attempt)
        || claim.state != "SPAWN_OWNER_RESERVED"
        || claim.owner_id != owner_id
    {
        return Err("REVIEWED_BUILD_CLAIM_UNAVAILABLE".into());
    }
    // Create-new is the one-worker linearization point.  A duplicate helper,
    // stale owner token, or a crash/replay cannot acquire it and therefore
    // cannot reach source materialization.
    let owner_proof = ReviewedBuildOwnerProofV1 {
        schema_version: 1,
        build_attempt_id: attempt.build_attempt_id.clone(),
        owner_id: claim.owner_id.clone(),
        spawn_generation: claim.spawn_generation,
    };
    control_create_json(&control, "worker-owner.json", &owner_proof)
        .map_err(|_| "REVIEWED_BUILD_CLAIM_UNAVAILABLE".to_string())?;
    // `claim.json` is immutable once the supervisor reserves its owner.  The
    // create-once proof above is the durable worker-ownership transition: it
    // is opened as an exact child of this pinned control directory and avoids
    // an authority-bearing pathname replacement of the claim record.
    let mut failure_diagnostic = None;
    let mut work = || -> Result<ReviewedBuildAttestationV2, String> {
        let snapshot = validate_attempt(
            workspace,
            &attempt,
            &attempt.review_record_id,
            &attempt.review_authority_sha256,
            &attempt.snapshot_expected,
        )?;
        let source = materialize_snapshot(&snapshot, &control, build_attempt_id)?;
        let mut target_guard = build_target_guard(workspace, &control, build_attempt_id)?;
        let target = target_guard.path().to_path_buf();
        let mut temp_guard = target_guard.try_clone("reviewed build temp")?;
        descend_or_create(&mut temp_guard, "tmp", "reviewed build temp")?;
        temp_guard.assert_stable("reviewed build temp")?;
        let temp = temp_guard.path().to_path_buf();
        let cargo_pin = open_attested_tool(&attempt.cargo, "cargo")?;
        let rustc_pin = open_attested_tool(&attempt.rustc, "rustc")?;
        let cargo_home = seed_isolated_cargo_home(&control, &source)?;
        let mut command = Command::new(&cargo_pin.evidence.absolute_path);
        configure_exact_worker_cargo_command(
            &mut command,
            &attempt,
            &source,
            &target,
            &temp,
            &cargo_home,
            &rustc_pin.evidence.absolute_path,
        )?;
        // The job is allocated before process creation.  Windows starts Cargo
        // suspended, attaches its process to the job, then resumes it through
        // the process handle.  Cargo cannot execute a build script before the
        // tree owner is established.
        let job = BuildJob::new()?;
        #[cfg(windows)]
        command.creation_flags(CREATE_SUSPENDED);
        let mut child = command
            .spawn()
            .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "REVIEWED_BUILD_FAILED".to_string())?;
        // Drain the pipe through process completion.  Only the bounded prefix
        // is retained and it is reduced to a redacted classification/digest
        // below, so diagnostic capture cannot change Cargo pipe behavior or
        // persist paths, credentials, or raw compiler output.
        let mut stderr_reader = Some(std::thread::spawn(move || {
            read_bounded_cargo_stderr(stderr)
        }));
        // Kill-on-close binds Cargo and all inherited compiler/linker/build
        // script descendants to this closed worker, rather than trusting a
        // single Cargo PID.  Platforms without this reviewed ownership
        // primitive fail closed before a build can become authority.
        job.assign(&child)?;
        job.resume(&child)?;
        let started = std::time::Instant::now();
        loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|_| "REVIEWED_BUILD_FAILED".to_string())?
            {
                if !status.success() {
                    let captured_stderr = stderr_reader
                        .take()
                        .and_then(|reader| reader.join().ok())
                        .and_then(Result::ok)
                        .unwrap_or_default();
                    failure_diagnostic =
                        Some(summarize_cargo_failure(status.code(), captured_stderr));
                    return Err("REVIEWED_BUILD_FAILED".into());
                }
                break;
            }
            if started.elapsed() > BUILD_TIMEOUT {
                let _ = child.kill();
                let _ = job.terminate();
                let _ = child.wait();
                return Err("REVIEWED_BUILD_TIMEOUT".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // The child has exited successfully; complete the drain before moving
        // to tool remeasurement.  Success retains no diagnostic material.
        let _ = stderr_reader
            .take()
            .and_then(|reader| reader.join().ok())
            .and_then(Result::ok);
        // Revalidate the exact pinned tool evidence after all compiler children exit.
        if cargo_pin.evidence != attempt.cargo
            || rustc_pin.evidence != attempt.rustc
            || trusted_tool_from_absolute(Path::new(&attempt.cargo.absolute_path), "cargo")?
                != attempt.cargo
            || trusted_tool_from_absolute(Path::new(&attempt.rustc.absolute_path), "rustc")?
                != attempt.rustc
        {
            return Err("REVIEWED_BUILD_EVIDENCE_DRIFTED".into());
        }
        // Cross the explicit local-workstation output-policy gate before
        // acquiring any built-child authority. The target parent has remained
        // pinned across the worker-owned Cargo/linker job; after this gate the
        // exact release child is opened relative to that pinned parent,
        // measured from its handle, and copied directly from that handle into
        // the create-new reviewed candidate.
        require_trusted_final_link_handoff()?;
        let mut built = open_built_output(&mut target_guard)?;
        let built_evidence =
            evidence_from_open_regular(&mut built, "reviewed build release output")?;
        let candidate_relative_path =
            format!("target/reviewed-builds/{build_attempt_id}/catdesk.exe");
        let candidate_parent = candidate_parent_for_create(workspace, build_attempt_id)?;
        let mut candidate = create_candidate_file(&candidate_parent)?;
        copy_open_regular_files(&mut built, &mut candidate, built_evidence.length)?;
        let candidate_evidence =
            evidence_from_open_regular(&mut candidate, "reviewed build candidate")?;
        if candidate_evidence.sha256 != built_evidence.sha256
            || candidate_evidence.length != built_evidence.length
        {
            return Err("REVIEWED_BUILD_OUTPUT_UNAVAILABLE".into());
        }
        let mut attestation = ReviewedBuildAttestationV2 {
            schema_version: ATTESTATION_SCHEMA_VERSION,
            producer: "CATDESK_REVIEWED_BUILD_WORKER_V2".into(),
            build_attempt_id: attempt.build_attempt_id.clone(),
            review_session_id: attempt.review_session_id.clone(),
            review_record_id: attempt.review_record_id.clone(),
            review_authority_sha256: attempt.review_authority_sha256.clone(),
            snapshot_id: snapshot.snapshot_id,
            snapshot_authority_digest: snapshot.authority_digest,
            snapshot_manifest_digest: snapshot.manifest_digest,
            build_policy_sha256: attempt.policy.policy_sha256.clone(),
            build_argv_sha256: attempt.policy.argv_sha256.clone(),
            environment_policy_sha256: attempt.policy.environment_policy_sha256.clone(),
            cargo_path: attempt.cargo.absolute_path.clone(),
            cargo_sha256: attempt.cargo.sha256.clone(),
            cargo_identity: attempt.cargo.identity.clone(),
            cargo_version_sha256: attempt.cargo.version_sha256.clone(),
            rustc_path: attempt.rustc.absolute_path.clone(),
            rustc_sha256: attempt.rustc.sha256.clone(),
            rustc_identity: attempt.rustc.identity.clone(),
            rustc_version_sha256: attempt.rustc.version_sha256.clone(),
            candidate_relative_path,
            candidate_sha256: candidate_evidence.sha256,
            candidate_length: candidate_evidence.length,
            candidate_identity: candidate_evidence.identity,
            generation: 1,
            created_at_unix: now_unix(),
            attestation_digest: String::new(),
        };
        attestation.attestation_digest = digest_attestation(&attestation)?;
        Ok(attestation)
    };
    match work() {
        Ok(attestation) => {
            control_create_json(&control, "attestation.json", &attestation)?;
            let result = ReviewedBuildResultV1 {
                schema_version: RESULT_SCHEMA_VERSION,
                build_attempt_id: attempt.build_attempt_id,
                owner_id: claim.owner_id,
                state: "BUILD_ATTESTED".into(),
                attestation_digest: Some(attestation.attestation_digest.clone()),
                failure_code: None,
                failure_diagnostic: None,
            };
            control_create_json(&control, "result.json", &result)?;
            // Promotion mirrors an exact producer record but the control record remains canonical.
            atomic_json(
                &workspace.join(".catdesk/promotion-control/reviewed-build-attestation.json"),
                &attestation,
            )?;
            Ok(())
        }
        Err(error) => {
            let result = ReviewedBuildResultV1 {
                schema_version: RESULT_SCHEMA_VERSION,
                build_attempt_id: attempt.build_attempt_id,
                owner_id: claim.owner_id,
                state: "BUILD_FAILED_OR_AMBIGUOUS".into(),
                attestation_digest: None,
                failure_code: Some(error.clone()),
                failure_diagnostic,
            };
            let _ = control_create_json(&control, "result.json", &result);
            Err(error)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReviewedWindowsToolchainEnvironment {
    path: OsString,
    cc: OsString,
    ar: OsString,
    lib: OsString,
    libpath: OsString,
    include: OsString,
}

#[cfg(windows)]
fn validate_reviewed_toolchain_directory(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(value) => current.push(value),
            _ => return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into()),
        }
        if current.as_os_str().is_empty() || current.parent().is_none() {
            continue;
        }
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
        if !metadata.file_type().is_dir()
            || metadata.file_type().is_symlink()
            || metadata.file_attributes() & 0x0400 != 0
        {
            return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
        }
    }
    Ok(())
}

#[cfg(windows)]
fn reviewed_numeric_version(name: &std::ffi::OsStr) -> Option<Vec<u32>> {
    let text = name.to_str()?;
    let parts = text
        .split('.')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (!parts.is_empty()).then_some(parts)
}

#[cfg(windows)]
fn highest_reviewed_version_directory(root: &Path, required: &[&str]) -> Result<PathBuf, String> {
    validate_reviewed_toolchain_directory(root)?;
    let mut candidates = Vec::new();
    for entry in
        fs::read_dir(root).map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?
    {
        let entry = entry.map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
        let Some(version) = reviewed_numeric_version(&entry.file_name()) else {
            continue;
        };
        let candidate = entry.path();
        if validate_reviewed_toolchain_directory(&candidate).is_err() {
            continue;
        }
        if required.iter().all(|relative| {
            let child = candidate.join(relative);
            validate_reviewed_toolchain_directory(&child).is_ok()
        }) {
            candidates.push((version, candidate));
        }
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    candidates
        .pop()
        .map(|(_, path)| path)
        .ok_or_else(|| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into())
}

#[cfg(windows)]
fn require_reviewed_toolchain_file(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.file_attributes() & 0x0400 != 0
    {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    Ok(())
}

#[cfg(windows)]
fn reviewed_msvc_install_roots(drive: &str) -> Vec<(usize, usize, PathBuf)> {
    // These are product-derived, closed compatibility layouts only.  Visual
    // Studio 2022 installs under Program Files using the year directory, while
    // the current Visual Studio 18 Build Tools layout observed on this host is
    // under Program Files (x86) using the major-version directory.  Do not
    // search PATH, registry, VS environment variables, or caller-selected
    // locations here.
    const LAYOUTS: [(&str, &str); 2] = [("Program Files", "2022"), ("Program Files (x86)", "18")];
    let mut roots = Vec::new();
    for (layout_priority, (program_files, visual_studio_version)) in LAYOUTS.into_iter().enumerate()
    {
        for (edition_priority, edition) in ["BuildTools", "Community", "Professional", "Enterprise"]
            .into_iter()
            .enumerate()
        {
            roots.push((
                layout_priority,
                edition_priority,
                PathBuf::from(format!(
                    r"{}\{}\Microsoft Visual Studio\{}\{}\VC\Tools\MSVC",
                    drive, program_files, visual_studio_version, edition
                )),
            ));
        }
    }
    roots
}

#[cfg(windows)]
fn reviewed_windows_toolchain_environment(
    cargo: &Path,
) -> Result<ReviewedWindowsToolchainEnvironment, String> {
    let drive = os_system_drive()?.to_string_lossy().into_owned();
    let mut msvc_candidates = Vec::new();
    for (layout_priority, edition_priority, root) in reviewed_msvc_install_roots(&drive) {
        if !root.exists() {
            continue;
        }
        let selected =
            highest_reviewed_version_directory(&root, &["bin/Hostx64/x64", "lib/x64", "include"])?;
        let version = reviewed_numeric_version(
            selected
                .file_name()
                .ok_or_else(|| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?,
        )
        .ok_or_else(|| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
        msvc_candidates.push((version, layout_priority, edition_priority, selected));
    }
    msvc_candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
    });
    let (_, _, _, msvc) = msvc_candidates
        .pop()
        .ok_or_else(|| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;

    let sdk_root = PathBuf::from(format!(r"{}\Program Files (x86)\Windows Kits\10", drive));
    let sdk_lib =
        highest_reviewed_version_directory(&sdk_root.join("Lib"), &["ucrt/x64", "um/x64"])?;
    let sdk_version = sdk_lib
        .file_name()
        .ok_or_else(|| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
    let sdk_include = sdk_root.join("Include").join(sdk_version);
    for child in ["ucrt", "shared", "um", "winrt"] {
        validate_reviewed_toolchain_directory(&sdk_include.join(child))?;
    }

    let msvc_bin = msvc.join("bin/Hostx64/x64");
    let msvc_lib = msvc.join("lib/x64");
    let msvc_include = msvc.join("include");
    let sdk_ucrt_lib = sdk_lib.join("ucrt/x64");
    let sdk_um_lib = sdk_lib.join("um/x64");
    let cc = msvc_bin.join("cl.exe");
    let ar = msvc_bin.join("lib.exe");
    require_reviewed_toolchain_file(&cc)?;
    require_reviewed_toolchain_file(&ar)?;
    require_reviewed_toolchain_file(&msvc_bin.join("link.exe"))?;
    require_reviewed_toolchain_file(&msvc_lib.join("libcmt.lib"))?;
    require_reviewed_toolchain_file(&sdk_ucrt_lib.join("ucrt.lib"))?;
    require_reviewed_toolchain_file(&sdk_um_lib.join("kernel32.lib"))?;

    let cargo_parent = cargo
        .parent()
        .ok_or_else(|| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
    let include_paths = [
        msvc_include,
        sdk_include.join("ucrt"),
        sdk_include.join("shared"),
        sdk_include.join("um"),
        sdk_include.join("winrt"),
    ];
    Ok(ReviewedWindowsToolchainEnvironment {
        path: std::env::join_paths([cargo_parent, msvc_bin.as_path()])
            .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?,
        cc: cc.into_os_string(),
        ar: ar.into_os_string(),
        lib: std::env::join_paths([
            msvc_lib.as_path(),
            sdk_ucrt_lib.as_path(),
            sdk_um_lib.as_path(),
        ])
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?,
        libpath: std::env::join_paths([
            msvc_lib.as_path(),
            sdk_ucrt_lib.as_path(),
            sdk_um_lib.as_path(),
        ])
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?,
        include: std::env::join_paths(include_paths.iter())
            .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?,
    })
}

#[cfg(not(windows))]
fn reviewed_windows_toolchain_environment(
    _cargo: &Path,
) -> Result<ReviewedWindowsToolchainEnvironment, String> {
    Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into())
}

/// Apply the one closed Cargo worker environment after all authority-bearing
/// toolchain values have already been product-derived and validated.  Keeping
/// this layer pure lets tests prove the exact child environment without making
/// unit-test success depend on the host's installed Visual Studio layout.
fn apply_exact_worker_cargo_command(
    command: &mut Command,
    attempt: &ReviewedBuildAttemptV1,
    source: &Path,
    target: &Path,
    temp: &Path,
    cargo_home: &Path,
    rustc: &str,
    toolchain: &ReviewedWindowsToolchainEnvironment,
) -> Result<(), String> {
    if attempt.policy != fixed_policy() {
        return Err("REVIEWED_BUILD_EVIDENCE_DRIFTED".into());
    }
    command
        .args(&attempt.policy.argv)
        .current_dir(source)
        .env_clear()
        .env("CARGO_TARGET_DIR", target)
        .env("RUSTC", rustc)
        .env("CARGO_HOME", cargo_home)
        .env("PATH", &toolchain.path)
        .env("CC", &toolchain.cc)
        .env("AR", &toolchain.ar)
        .env("LIB", &toolchain.lib)
        .env("LIBPATH", &toolchain.libpath)
        .env("INCLUDE", &toolchain.include)
        .env("SystemDrive", os_system_drive()?)
        .env("SystemRoot", os_system_root()?)
        .env("TEMP", temp)
        .env("TMP", temp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    Ok(())
}

/// Resolve the fixed-root Windows toolchain and then apply the exact closed
/// worker environment.  Production and the host-linker diagnostic always use
/// this wrapper; callers cannot supply the toolchain environment.
fn configure_exact_worker_cargo_command(
    command: &mut Command,
    attempt: &ReviewedBuildAttemptV1,
    source: &Path,
    target: &Path,
    temp: &Path,
    cargo_home: &Path,
    rustc: &str,
) -> Result<(), String> {
    let toolchain =
        reviewed_windows_toolchain_environment(Path::new(&attempt.cargo.absolute_path))?;
    apply_exact_worker_cargo_command(
        command, attempt, source, target, temp, cargo_home, rustc, &toolchain,
    )
}

// This runner is intentionally compiled only into unit tests. It has no MCP,
// CLI, environment, path, attempt, target, toolchain, or classification input.
// The ignored test below is the sole manual trigger and uses the compile-time
// workspace root. It is not a reviewed-build retry and never writes control
// records, a candidate, or an attestation.
#[cfg(all(test, windows))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct FixedV5HostLinkerDiagnosticOutcome {
    classification: &'static str,
    cargo_classification: String,
    cargo_failure_signals: Vec<&'static str>,
    custom_build_crate: Option<String>,
    custom_build_version: Option<String>,
    // Fixed LNK codes only. These are recognized from the full drained stream
    // and contain neither diagnostic text nor linker arguments.
    linker_error_codes: Vec<&'static str>,
    missing_link_library: Option<String>,
    missing_link_input: Option<String>,
    captured_stderr_sha256: String,
    captured_stderr_length: usize,
    stderr_truncated: bool,
}

#[cfg(all(test, windows))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct FixedV5CargoCacheSeedDiagnostic {
    stage: &'static str,
    crate_name: Option<String>,
    crate_version: Option<String>,
}

#[cfg(all(test, windows))]
fn fixed_v5_cargo_cache_seed_failure(
    stage: &'static str,
    package: Option<&LockedRegistryCrate>,
) -> FixedV5CargoCacheSeedDiagnostic {
    const ALLOWED: &[&str] = &[
        "LOCKFILE_PARSE",
        "USER_PROFILE",
        "AMBIENT_CARGO_ROOT",
        "REGISTRY_IDENTITY",
        "REGISTRY_CONFIG",
        "INDEX_ENTRY",
        "ARCHIVE",
        "DESTINATION_CREATE",
        "STABILITY_CHECK",
        "UNKNOWN",
    ];
    let stage = ALLOWED
        .iter()
        .copied()
        .find(|candidate| *candidate == stage)
        .unwrap_or("UNKNOWN");
    let (crate_name, crate_version) = package
        .filter(|item| {
            safe_registry_crate_component(&item.name)
                && safe_registry_version_component(&item.version)
        })
        .map(|item| (Some(item.name.clone()), Some(item.version.clone())))
        .unwrap_or((None, None));
    FixedV5CargoCacheSeedDiagnostic {
        stage,
        crate_name,
        crate_version,
    }
}

#[cfg(all(test, windows))]
fn diagnose_seed_isolated_cargo_home_into(
    destination: &mut ProtectedDirectoryGuard,
    source: &Path,
) -> Result<PathBuf, FixedV5CargoCacheSeedDiagnostic> {
    let source_root = ProtectedDirectoryGuard::acquire(source, "reviewed build source")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("LOCKFILE_PARSE", None))?;
    let lock = read_relative_regular(
        &source_root,
        "Cargo.lock",
        MAX_CARGO_LOCK_BYTES,
        "reviewed build lockfile",
    )
    .map_err(|_| fixed_v5_cargo_cache_seed_failure("LOCKFILE_PARSE", None))?;
    let crates = locked_registry_crates(&lock)
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("LOCKFILE_PARSE", None))?;
    if crates.is_empty() {
        return Err(fixed_v5_cargo_cache_seed_failure("LOCKFILE_PARSE", None));
    }

    let profile = os_current_user_profile()
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("USER_PROFILE", None))?;
    validate_profile_directory(&profile)
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("USER_PROFILE", None))?;
    let mut ambient = ProtectedDirectoryGuard::acquire(
        &profile.join(".cargo"),
        "reviewed build local cargo cache",
    )
    .map_err(|_| fixed_v5_cargo_cache_seed_failure("AMBIENT_CARGO_ROOT", None))?;
    ambient
        .descend_existing("registry", "reviewed build local cargo registry")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("AMBIENT_CARGO_ROOT", None))?;
    let mut ambient_index = ambient
        .try_clone("reviewed build local cargo index")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("AMBIENT_CARGO_ROOT", None))?;
    ambient_index
        .descend_existing("index", "reviewed build local cargo index")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("AMBIENT_CARGO_ROOT", None))?;
    let mut ambient_cache = ambient
        .try_clone("reviewed build local cargo cache")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("AMBIENT_CARGO_ROOT", None))?;
    ambient_cache
        .descend_existing("cache", "reviewed build local cargo cache")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("AMBIENT_CARGO_ROOT", None))?;
    let registry_id = shared_registry_identity(&ambient_index, &ambient_cache)
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("REGISTRY_IDENTITY", None))?;

    let mut source_index = ambient_index
        .try_clone("reviewed build local cargo index")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("REGISTRY_IDENTITY", None))?;
    source_index
        .descend_existing(&registry_id, "reviewed build local cargo index")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("REGISTRY_IDENTITY", None))?;
    let mut source_cache = ambient_cache
        .try_clone("reviewed build local cargo cache")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("REGISTRY_IDENTITY", None))?;
    source_cache
        .descend_existing(&registry_id, "reviewed build local cargo cache")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("REGISTRY_IDENTITY", None))?;
    let config = read_relative_regular(
        &source_index,
        "config.json",
        MAX_CARGO_REGISTRY_CONFIG_BYTES,
        "reviewed build local cargo index",
    )
    .map_err(|_| fixed_v5_cargo_cache_seed_failure("REGISTRY_CONFIG", None))?;
    if config.is_empty() {
        return Err(fixed_v5_cargo_cache_seed_failure("REGISTRY_CONFIG", None));
    }

    destination
        .create_child("cargo-home", "reviewed build isolated cargo home")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("DESTINATION_CREATE", None))?;
    let home = destination.path().to_path_buf();
    destination
        .create_child("registry", "reviewed build isolated cargo registry")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("DESTINATION_CREATE", None))?;
    let mut destination_index = destination
        .try_clone("reviewed build isolated cargo index")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("DESTINATION_CREATE", None))?;
    destination_index
        .create_child("index", "reviewed build isolated cargo index")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("DESTINATION_CREATE", None))?;
    destination_index
        .create_child(&registry_id, "reviewed build isolated cargo index")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("DESTINATION_CREATE", None))?;
    write_new_regular_in(
        &destination_index,
        "config.json",
        &config,
        "reviewed build isolated cargo index",
    )
    .map_err(|_| fixed_v5_cargo_cache_seed_failure("DESTINATION_CREATE", None))?;
    let mut destination_cache = destination
        .try_clone("reviewed build isolated cargo cache")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("DESTINATION_CREATE", None))?;
    destination_cache
        .create_child("cache", "reviewed build isolated cargo cache")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("DESTINATION_CREATE", None))?;
    destination_cache
        .create_child(&registry_id, "reviewed build isolated cargo cache")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("DESTINATION_CREATE", None))?;

    let mut copied_index_names = BTreeSet::new();
    for package in &crates {
        if copied_index_names.insert(package.name.to_ascii_lowercase()) {
            copy_locked_registry_index_entry(&source_index, &mut destination_index, package)
                .map_err(|_| fixed_v5_cargo_cache_seed_failure("INDEX_ENTRY", Some(package)))?;
        }
        copy_locked_registry_archive(&source_cache, &destination_cache, package)
            .map_err(|_| fixed_v5_cargo_cache_seed_failure("ARCHIVE", Some(package)))?;
    }
    destination
        .assert_stable("reviewed build isolated cargo home")
        .map_err(|_| fixed_v5_cargo_cache_seed_failure("STABILITY_CHECK", None))?;
    Ok(home)
}

#[cfg(all(test, windows))]
fn fixed_v5_host_linker_diagnostic_outcome(
    exit_code: Option<i32>,
    captured_stderr: BoundedCargoStderr,
) -> Result<FixedV5HostLinkerDiagnosticOutcome, String> {
    if exit_code.is_none_or(|code| code == 0) {
        return Err("REVIEWED_BUILD_HOST_LINKER_DIAGNOSTIC_UNEXPECTED_SUCCESS".into());
    }
    let linker_error_codes = captured_stderr.diagnostic_linker_error_codes.clone();
    let cargo_failure_signals = captured_stderr.diagnostic_cargo_failure_signals.clone();
    let custom_build_crate = captured_stderr.diagnostic_custom_build_crate.clone();
    let custom_build_version = captured_stderr.diagnostic_custom_build_version.clone();
    let diagnostic = summarize_cargo_failure(exit_code, captured_stderr);
    let classification = match diagnostic.classification.as_str() {
        "CARGO_LINKER_NOT_FOUND" => "LINKER_EXECUTABLE_NOT_FOUND",
        "CARGO_LINK_LIBRARY_NOT_FOUND" => "MSVC_OR_WINDOWS_SDK_LIBRARY_ENV_MISSING",
        "CARGO_LINK_INPUT_NOT_FOUND" => "LINK_INPUT_UNAVAILABLE",
        "CARGO_LINK_UNRESOLVED_EXTERNAL" => "LINK_UNRESOLVED_EXTERNAL",
        "CARGO_LINK_PDB_FAILURE" => "LINK_PDB_FAILURE",
        "CARGO_LINK_OUT_OF_MEMORY" => "LINK_OUT_OF_MEMORY",
        _ => "OTHER_LINKER_EXIT",
    };
    Ok(FixedV5HostLinkerDiagnosticOutcome {
        classification,
        cargo_classification: diagnostic.classification.clone(),
        cargo_failure_signals,
        custom_build_crate,
        custom_build_version,
        linker_error_codes,
        missing_link_library: diagnostic.missing_link_library,
        missing_link_input: diagnostic.missing_link_input,
        captured_stderr_sha256: diagnostic.captured_stderr_sha256,
        captured_stderr_length: diagnostic.captured_stderr_length,
        stderr_truncated: diagnostic.stderr_truncated,
    })
}

#[cfg(all(test, windows))]
fn fixed_v5_host_linker_diagnostic_summary_line(
    outcome: &FixedV5HostLinkerDiagnosticOutcome,
) -> String {
    format!(
        "FIXED_V5_HOST_LINKER_DIAGNOSTIC classification={} cargoClassification={} cargoFailureSignals={:?} customBuildCrate={:?} customBuildVersion={:?} linkerErrorCodes={:?} missingLinkLibrary={:?} missingLinkInput={:?} capturedStderrSha256={} capturedStderrLength={} stderrTruncated={}",
        outcome.classification,
        outcome.cargo_classification,
        outcome.cargo_failure_signals,
        outcome.custom_build_crate,
        outcome.custom_build_version,
        outcome.linker_error_codes,
        outcome.missing_link_library,
        outcome.missing_link_input,
        outcome.captured_stderr_sha256,
        outcome.captured_stderr_length,
        outcome.stderr_truncated,
    )
}

#[cfg(all(test, windows))]
fn validate_fixed_v5_host_linker_diagnostic_audit(
    attempt: &ReviewedBuildAttemptV1,
    audit: &ReviewedBuildTerminalAuditV1,
) -> Result<(), String> {
    let diagnostic = audit
        .result
        .failure_diagnostic
        .as_ref()
        .ok_or("REVIEWED_BUILD_HOST_LINKER_DIAGNOSTIC_GATE_REFUSED")?;
    if audit.schema_version != 1
        || audit.attempt != *attempt
        || digest_attempt(attempt)
            .map(|digest| digest != attempt.attempt_digest)
            .unwrap_or(true)
        || !valid_claim(&audit.claim, attempt)
        || !valid_owner_proof(&audit.owner, &audit.claim)
        || audit.result.schema_version != RESULT_SCHEMA_VERSION
        || audit.result.build_attempt_id != attempt.build_attempt_id
        || audit.result.owner_id != audit.claim.owner_id
        || attempt.policy != fixed_policy()
        || audit.result.state != "BUILD_FAILED_OR_AMBIGUOUS"
        || audit.result.attestation_digest.is_some()
        || audit.result.failure_code.as_deref() != Some("REVIEWED_BUILD_FAILED")
        || diagnostic.schema_version != 1
        || diagnostic.phase != "CARGO_BUILD"
        || diagnostic.exit_code != Some(101)
        || !matches!(
            diagnostic.classification.as_str(),
            "CARGO_LINK_FAILED"
                | "CARGO_LINKER_NOT_FOUND"
                | "CARGO_LINK_LIBRARY_NOT_FOUND"
                | "CARGO_LINK_INPUT_NOT_FOUND"
        )
        || !valid_sha256(&diagnostic.captured_stderr_sha256)
        || diagnostic.captured_stderr_length > MAX_CARGO_FAILURE_DIAGNOSTIC_BYTES
    {
        return Err("REVIEWED_BUILD_HOST_LINKER_DIAGNOSTIC_GATE_REFUSED".into());
    }
    Ok(())
}

#[cfg(all(test, windows))]
fn validate_fixed_v5_host_linker_diagnostic(
    workspace: &Path,
    control: &BuildControlRoot,
    attempt: &ReviewedBuildAttemptV1,
) -> Result<ValidatedReviewedSourceSnapshotV1, String> {
    let snapshot = validate_attempt(
        workspace,
        attempt,
        &attempt.review_record_id,
        &attempt.review_authority_sha256,
        &attempt.snapshot_expected,
    )?;
    let audit = terminal_failure_audit(control, attempt)
        .map_err(|_| "REVIEWED_BUILD_HOST_LINKER_DIAGNOSTIC_GATE_REFUSED".to_string())?;
    validate_fixed_v5_host_linker_diagnostic_audit(attempt, &audit)?;
    Ok(snapshot)
}

#[cfg(all(test, windows))]
fn fixed_v5_host_linker_diagnostic_root(
    workspace: &Path,
) -> Result<ProtectedDirectoryGuard, String> {
    let mut root = ProtectedDirectoryGuard::acquire(workspace, "host linker diagnostic workspace")?;
    root.descend_or_create("target-verify", "host linker diagnostic root")?;
    root.descend_or_create(
        "reviewed-build-host-linker-diagnostic",
        "host linker diagnostic root",
    )?;
    root.create_child(
        &Uuid::new_v4().simple().to_string(),
        "host linker diagnostic root",
    )?;
    root.assert_stable("host linker diagnostic root")?;
    Ok(root)
}

#[cfg(all(test, windows))]
fn run_fixed_v5_cargo_cache_seed_diagnostic() -> Result<FixedV5CargoCacheSeedDiagnostic, String> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let control = existing_control_root(workspace)?;
    let attempt: ReviewedBuildAttemptV1 = control_read_json(&control, "attempt.json")?;
    let active = read_active_pointer(&control.base_guard)?;
    if active.active_attempt_id != attempt.build_attempt_id
        || active.active_attempt_digest != attempt.attempt_digest
    {
        return Err("REVIEWED_BUILD_CARGO_CACHE_DIAGNOSTIC_GATE_REFUSED".into());
    }
    let snapshot = validate_fixed_v5_host_linker_diagnostic(workspace, &control, &attempt)?;
    let mut diagnostic_root = fixed_v5_host_linker_diagnostic_root(workspace)?;
    let mut source_guard = diagnostic_root.try_clone("cargo cache diagnostic source")?;
    source_guard.create_child("source", "cargo cache diagnostic source")?;
    materialize_snapshot_into(&snapshot, &mut source_guard)?;
    let source = source_guard.path().to_path_buf();

    match diagnose_seed_isolated_cargo_home_into(&mut diagnostic_root, &source) {
        Ok(_) => Ok(FixedV5CargoCacheSeedDiagnostic {
            stage: "SEED_SUCCEEDED",
            crate_name: None,
            crate_version: None,
        }),
        Err(outcome) => Ok(outcome),
    }
}

#[cfg(all(test, windows))]
fn run_fixed_v5_host_linker_diagnostic() -> Result<FixedV5HostLinkerDiagnosticOutcome, String> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
    let control = existing_control_root(workspace)?;
    let attempt: ReviewedBuildAttemptV1 = control_read_json(&control, "attempt.json")?;
    let active = read_active_pointer(&control.base_guard)?;
    if active.active_attempt_id != attempt.build_attempt_id
        || active.active_attempt_digest != attempt.attempt_digest
    {
        return Err("REVIEWED_BUILD_HOST_LINKER_DIAGNOSTIC_GATE_REFUSED".into());
    }
    let snapshot = validate_fixed_v5_host_linker_diagnostic(workspace, &control, &attempt)?;

    let mut diagnostic_root = fixed_v5_host_linker_diagnostic_root(workspace)?;
    let mut source_guard = diagnostic_root.try_clone("host linker diagnostic source")?;
    source_guard.create_child("source", "host linker diagnostic source")?;
    materialize_snapshot_into(&snapshot, &mut source_guard)?;
    let source = source_guard.path().to_path_buf();

    let mut target_guard = diagnostic_root.try_clone("host linker diagnostic target")?;
    target_guard.create_child("target", "host linker diagnostic target")?;
    let target = target_guard.path().to_path_buf();
    let mut temp_guard = diagnostic_root.try_clone("host linker diagnostic temp")?;
    temp_guard.create_child("tmp", "host linker diagnostic temp")?;
    let temp = temp_guard.path().to_path_buf();
    let cargo_home = seed_isolated_cargo_home_into(&mut diagnostic_root, &source)?;
    let cargo_pin = open_attested_tool(&attempt.cargo, "cargo")?;
    let rustc_pin = open_attested_tool(&attempt.rustc, "rustc")?;

    let mut command = Command::new(&cargo_pin.evidence.absolute_path);
    configure_exact_worker_cargo_command(
        &mut command,
        &attempt,
        &source,
        &target,
        &temp,
        &cargo_home,
        &rustc_pin.evidence.absolute_path,
    )?;
    ensure_exact_active_pointer(&control.base_guard, &active)
        .map_err(|_| "REVIEWED_BUILD_HOST_LINKER_DIAGNOSTIC_GATE_REFUSED".to_string())?;
    let job = BuildJob::new()?;
    command.creation_flags(CREATE_SUSPENDED);
    let mut child = command
        .spawn()
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "REVIEWED_BUILD_HOST_LINKER_DIAGNOSTIC_UNAVAILABLE".to_string())?;
    let reader = std::thread::spawn(move || read_bounded_cargo_stderr(stderr));
    job.assign(&child)?;
    job.resume(&child)?;
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|_| "REVIEWED_BUILD_HOST_LINKER_DIAGNOSTIC_UNAVAILABLE".to_string())?
        {
            break status;
        }
        if started.elapsed() > BUILD_TIMEOUT {
            let _ = child.kill();
            let _ = job.terminate();
            let _ = child.wait();
            return Err("REVIEWED_BUILD_HOST_LINKER_DIAGNOSTIC_TIMEOUT".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let captured_stderr = reader
        .join()
        .ok()
        .and_then(Result::ok)
        .ok_or_else(|| "REVIEWED_BUILD_HOST_LINKER_DIAGNOSTIC_UNAVAILABLE".to_string())?;
    if cargo_pin.evidence != attempt.cargo
        || rustc_pin.evidence != attempt.rustc
        || trusted_tool_from_absolute(Path::new(&attempt.cargo.absolute_path), "cargo")?
            != attempt.cargo
        || trusted_tool_from_absolute(Path::new(&attempt.rustc.absolute_path), "rustc")?
            != attempt.rustc
    {
        return Err("REVIEWED_BUILD_EVIDENCE_DRIFTED".into());
    }
    fixed_v5_host_linker_diagnostic_outcome(status.code(), captured_stderr)
}

/// Explicit local-workstation reviewed-output policy boundary.
///
/// CatDesk's release threat model trusts the current Windows user and the
/// worker-owned Cargo/linker process tree. It does not attempt to defend
/// against a separate hostile process already executing as that same user.
/// The target directory is pinned before Cargo starts; after the job exits the
/// worker immediately opens the fixed release child beneath that pinned parent,
/// measures SHA-256/length/file identity, and copies the exact open bytes into a
/// create-new immutable candidate before attestation. Promotion later
/// remeasures that candidate. This bounded policy intentionally replaces the
/// unimplemented dedicated-service/final-link trust domain rather than adding
/// another signer, service, UAC flow, or per-release authority mechanism.
#[cfg(windows)]
fn require_trusted_final_link_handoff() -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
fn require_trusted_final_link_handoff() -> Result<(), String> {
    Err("REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED".into())
}

#[cfg(windows)]
struct BuildJob(*mut c_void);

#[cfg(windows)]
impl BuildJob {
    fn new() -> Result<Self, String> {
        #[repr(C)]
        struct BasicLimitInformation {
            per_process_user_time_limit: i64,
            per_job_user_time_limit: i64,
            limit_flags: u32,
            minimum_working_set_size: usize,
            maximum_working_set_size: usize,
            active_process_limit: u32,
            affinity: usize,
            priority_class: u32,
            scheduling_class: u32,
        }
        #[repr(C)]
        struct IoCounters {
            read_operation_count: u64,
            write_operation_count: u64,
            other_operation_count: u64,
            read_transfer_count: u64,
            write_transfer_count: u64,
            other_transfer_count: u64,
        }
        #[repr(C)]
        struct ExtendedLimitInformation {
            basic_limit_information: BasicLimitInformation,
            io_info: IoCounters,
            process_memory_limit: usize,
            job_memory_limit: usize,
            peak_process_memory_used: usize,
            peak_job_memory_used: usize,
        }
        unsafe extern "system" {
            fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> *mut c_void;
            fn SetInformationJobObject(
                job: *mut c_void,
                class: i32,
                information: *mut c_void,
                length: u32,
            ) -> i32;
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;
        const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err("REVIEWED_BUILD_PROCESS_UNAVAILABLE".into());
        }
        let mut limits: ExtendedLimitInformation = unsafe { std::mem::zeroed() };
        limits.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION as i32,
                (&mut limits as *mut ExtendedLimitInformation).cast(),
                std::mem::size_of::<ExtendedLimitInformation>() as u32,
            )
        };
        if configured == 0 {
            unsafe {
                CloseHandle(handle);
            }
            return Err("REVIEWED_BUILD_PROCESS_UNAVAILABLE".into());
        }
        Ok(Self(handle))
    }
    fn assign(&self, child: &std::process::Child) -> Result<(), String> {
        unsafe extern "system" {
            fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
        }
        if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle().cast()) } == 0 {
            return Err("REVIEWED_BUILD_PROCESS_UNAVAILABLE".into());
        }
        Ok(())
    }
    fn terminate(&self) -> Result<(), String> {
        unsafe extern "system" {
            fn TerminateJobObject(job: *mut c_void, exit_code: u32) -> i32;
        }
        if unsafe { TerminateJobObject(self.0, 1) } == 0 {
            return Err("REVIEWED_BUILD_PROCESS_UNAVAILABLE".into());
        }
        Ok(())
    }
    fn resume(&self, child: &std::process::Child) -> Result<(), String> {
        unsafe extern "system" {
            fn NtResumeProcess(process: *mut c_void) -> i32;
        }
        if unsafe { NtResumeProcess(child.as_raw_handle().cast()) } < 0 {
            return Err("REVIEWED_BUILD_PROCESS_UNAVAILABLE".into());
        }
        Ok(())
    }

    #[cfg(test)]
    fn contains_process(&self, child: &std::process::Child) -> Result<bool, String> {
        unsafe extern "system" {
            fn IsProcessInJob(process: *mut c_void, job: *mut c_void, result: *mut i32) -> i32;
        }
        let mut result = 0i32;
        if unsafe { IsProcessInJob(child.as_raw_handle().cast(), self.0, &mut result) } == 0 {
            return Err("REVIEWED_BUILD_PROCESS_UNAVAILABLE".into());
        }
        Ok(result != 0)
    }

    #[cfg(test)]
    fn assign_raw_process_for_test(&self, process: *mut c_void) -> Result<(), String> {
        unsafe extern "system" {
            fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
        }
        if process.is_null() || unsafe { AssignProcessToJobObject(self.0, process) } == 0 {
            return Err("REVIEWED_BUILD_PROCESS_UNAVAILABLE".into());
        }
        Ok(())
    }

    #[cfg(test)]
    fn contains_raw_process_for_test(&self, process: *mut c_void) -> Result<bool, String> {
        unsafe extern "system" {
            fn IsProcessInJob(process: *mut c_void, job: *mut c_void, result: *mut i32) -> i32;
        }
        let mut result = 0i32;
        if process.is_null() || unsafe { IsProcessInJob(process, self.0, &mut result) } == 0 {
            return Err("REVIEWED_BUILD_PROCESS_UNAVAILABLE".into());
        }
        Ok(result != 0)
    }
}

#[cfg(windows)]
impl Drop for BuildJob {
    fn drop(&mut self) {
        unsafe extern "system" {
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(not(windows))]
struct BuildJob;
#[cfg(not(windows))]
impl BuildJob {
    fn new() -> Result<Self, String> {
        Err("REVIEWED_BUILD_PROCESS_UNAVAILABLE".into())
    }
    fn assign(&self, _child: &std::process::Child) -> Result<(), String> {
        Err("REVIEWED_BUILD_PROCESS_UNAVAILABLE".into())
    }
    fn terminate(&self) -> Result<(), String> {
        Ok(())
    }
    fn resume(&self, _child: &std::process::Child) -> Result<(), String> {
        Err("REVIEWED_BUILD_PROCESS_UNAVAILABLE".into())
    }
}

pub fn parse_reviewed_build_worker_args(
    args: &[String],
) -> Result<Option<(PathBuf, String, String)>, String> {
    if !args.iter().any(|arg| arg == REVIEWED_BUILD_WORKER_FLAG) {
        return Ok(None);
    }
    if args.len() != 7
        || args[0] != REVIEWED_BUILD_WORKER_FLAG
        || args[1] != "--workspace"
        || args[3] != "--attempt"
        || args[5] != "--owner"
        || args[2].is_empty()
        || !valid_attempt_id(&args[4])
        || !valid_attempt_id(&args[6])
    {
        return Err("REVIEWED_BUILD_HELPER_UNAVAILABLE".into());
    }
    Ok(Some((
        PathBuf::from(&args[2]),
        args[4].clone(),
        args[6].clone(),
    )))
}

fn outcome_from_existing_state(
    control: &BuildControlRoot,
    attempt: &ReviewedBuildAttemptV1,
) -> Result<ReviewedBuildPublicOutcomeV1, String> {
    let claim: ReviewedBuildClaimV1 = control_read_json(control, "claim.json")?;
    if !valid_claim(&claim, attempt) {
        return Err("REVIEWED_BUILD_RESULT_UNAVAILABLE".into());
    }
    let owner_proven = control_has_valid_owner_proof(control, &claim).unwrap_or(false);
    let result: ReviewedBuildResultV1 = match control_read_json(control, "result.json") {
        Ok(result) => result,
        // No terminal record is an expected replay shape.  Treat an absent or
        // malformed record conservatively as non-terminal; it never grants a
        // second spawn owner or BUILD_ATTESTED authority.
        Err(_) => {
            return Ok(ReviewedBuildPublicOutcomeV1 {
                state: if owner_proven {
                    "WORKER_OWNED_PENDING"
                } else {
                    "CLAIMED_PENDING_UNPROVEN"
                },
            });
        }
    };
    if result.schema_version != RESULT_SCHEMA_VERSION
        || result.build_attempt_id != attempt.build_attempt_id
        || result.owner_id != claim.owner_id
        || !owner_proven
    {
        return Err("REVIEWED_BUILD_RESULT_UNAVAILABLE".into());
    }
    match result.state.as_str() {
        "BUILD_ATTESTED" => {
            let attestation: ReviewedBuildAttestationV2 =
                control_read_json(control, "attestation.json")?;
            validate_attestation(
                &attestation,
                attempt,
                &attestation.candidate_relative_path,
                &attestation.candidate_sha256,
            )?;
            if result.attestation_digest.as_deref() != Some(attestation.attestation_digest.as_str())
            {
                return Err("REVIEWED_BUILD_RESULT_UNAVAILABLE".into());
            }
            Ok(ReviewedBuildPublicOutcomeV1 {
                state: "BUILD_ATTESTED",
            })
        }
        "BUILD_FAILED_OR_AMBIGUOUS" => Ok(ReviewedBuildPublicOutcomeV1 {
            state: "BUILD_FAILED_OR_AMBIGUOUS",
        }),
        _ => Err("REVIEWED_BUILD_RESULT_UNAVAILABLE".into()),
    }
}

fn known_historical_build_policy(policy: &ReviewedBuildPolicyV1) -> bool {
    let current_argv = BUILD_ARGUMENTS
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    let legacy_argv = LEGACY_BUILD_ARGUMENTS
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    if policy.policy_sha256 != sha256(policy.version.as_bytes()) {
        return false;
    }
    match policy.version.as_str() {
        BUILD_POLICY_VERSION => {
            policy.argv == current_argv
                && policy.argv_sha256 == sha256(canonical_json(&current_argv).as_bytes())
                && (policy.environment_policy_sha256 == sha256(ENVIRONMENT_POLICY)
                    || policy.environment_policy_sha256.as_str()
                        == LEGACY_BUILD_ENVIRONMENT_POLICY_V5_PRE_FIXED_ROOT_LINKER_SHA256)
        }
        LEGACY_BUILD_POLICY_V4_VERSION => {
            policy.argv == legacy_argv
                && policy.argv_sha256 == sha256(canonical_json(&legacy_argv).as_bytes())
                && policy.environment_policy_sha256 == LEGACY_BUILD_ENVIRONMENT_POLICY_V4_SHA256
        }
        LEGACY_BUILD_POLICY_V3_VERSION => {
            policy.argv == legacy_argv
                && policy.argv_sha256 == sha256(canonical_json(&legacy_argv).as_bytes())
                && policy.environment_policy_sha256 == LEGACY_BUILD_ENVIRONMENT_POLICY_V3_SHA256
        }
        _ => false,
    }
}

fn validate_historical_terminal_attempt(
    workspace: &Path,
    attempt: &ReviewedBuildAttemptV1,
) -> Result<ValidatedReviewedSourceSnapshotV1, String> {
    if attempt.schema_version != ATTEMPT_SCHEMA_VERSION
        || !valid_attempt_id(&attempt.build_attempt_id)
        || !valid_attempt_id(&attempt.confirmation_token)
        || attempt.review_record_id.is_empty()
        || attempt.review_record_id.len() > 128
        || !valid_sha256(&attempt.review_authority_sha256)
        || attempt.review_authority_sha256 != attempt.review_authority_sha256.to_ascii_lowercase()
        || digest_attempt(attempt)? != attempt.attempt_digest
        || !known_historical_build_policy(&attempt.policy)
    {
        return Err("REVIEWED_BUILD_EVIDENCE_DRIFTED".into());
    }
    let snapshot = validate_committed_snapshot(workspace, &attempt.snapshot_expected)
        .map_err(|_| "REVIEWED_SOURCE_SNAPSHOT_REQUIRED".to_string())?;
    if snapshot.snapshot_id != attempt.snapshot_id
        || snapshot.authority_digest != attempt.snapshot_authority_digest
        || snapshot.manifest_digest != attempt.snapshot_manifest_digest
    {
        return Err("REVIEWED_BUILD_EVIDENCE_DRIFTED".into());
    }
    if trusted_tool_from_absolute(Path::new(&attempt.cargo.absolute_path), "cargo")?
        != attempt.cargo
        || trusted_tool_from_absolute(Path::new(&attempt.rustc.absolute_path), "rustc")?
            != attempt.rustc
    {
        return Err("REVIEWED_BUILD_EVIDENCE_DRIFTED".into());
    }
    Ok(snapshot)
}

fn validate_attempt(
    workspace: &Path,
    attempt: &ReviewedBuildAttemptV1,
    record: &str,
    digest: &str,
    expected: &ReviewedSourceSnapshotExpectedV1,
) -> Result<ValidatedReviewedSourceSnapshotV1, String> {
    if !exact_attempt_basics(attempt, record, digest, expected)
        || digest_attempt(attempt)? != attempt.attempt_digest
        || attempt.policy != fixed_policy()
    {
        return Err("REVIEWED_BUILD_EVIDENCE_DRIFTED".into());
    }
    let snapshot = validate_committed_snapshot(workspace, expected)
        .map_err(|_| "REVIEWED_SOURCE_SNAPSHOT_REQUIRED".to_string())?;
    if snapshot.snapshot_id != attempt.snapshot_id
        || snapshot.authority_digest != attempt.snapshot_authority_digest
        || snapshot.manifest_digest != attempt.snapshot_manifest_digest
    {
        return Err("REVIEWED_BUILD_EVIDENCE_DRIFTED".into());
    }
    if trusted_tool_from_absolute(Path::new(&attempt.cargo.absolute_path), "cargo")?
        != attempt.cargo
        || trusted_tool_from_absolute(Path::new(&attempt.rustc.absolute_path), "rustc")?
            != attempt.rustc
    {
        return Err("REVIEWED_BUILD_EVIDENCE_DRIFTED".into());
    }
    Ok(snapshot)
}

fn validate_attestation(
    attestation: &ReviewedBuildAttestationV2,
    attempt: &ReviewedBuildAttemptV1,
    path: &str,
    sha: &str,
) -> Result<(), String> {
    if attestation.schema_version != ATTESTATION_SCHEMA_VERSION
        || attestation.producer != "CATDESK_REVIEWED_BUILD_WORKER_V2"
        || attestation.build_attempt_id != attempt.build_attempt_id
        || attestation.review_session_id != attempt.review_session_id
        || attestation.review_record_id != attempt.review_record_id
        || attestation.review_authority_sha256 != attempt.review_authority_sha256
        || attestation.snapshot_id != attempt.snapshot_id
        || attestation.snapshot_authority_digest != attempt.snapshot_authority_digest
        || attestation.snapshot_manifest_digest != attempt.snapshot_manifest_digest
        || attestation.build_policy_sha256 != attempt.policy.policy_sha256
        || attestation.build_argv_sha256 != attempt.policy.argv_sha256
        || attestation.environment_policy_sha256 != attempt.policy.environment_policy_sha256
        || attestation.cargo_path != attempt.cargo.absolute_path
        || attestation.cargo_sha256 != attempt.cargo.sha256
        || attestation.cargo_identity != attempt.cargo.identity
        || attestation.cargo_version_sha256 != attempt.cargo.version_sha256
        || attestation.rustc_path != attempt.rustc.absolute_path
        || attestation.rustc_sha256 != attempt.rustc.sha256
        || attestation.rustc_identity != attempt.rustc.identity
        || attestation.rustc_version_sha256 != attempt.rustc.version_sha256
        || attestation.candidate_relative_path != path
        || attestation.candidate_sha256 != sha
        || attestation.candidate_identity.is_empty()
        || !valid_sha256(&attestation.attestation_digest)
        || digest_attestation(attestation)? != attestation.attestation_digest
    {
        return Err("REVIEWED_BUILD_ATTESTATION_UNAVAILABLE".into());
    }
    Ok(())
}

fn exact_attempt_matches(
    attempt: &ReviewedBuildAttemptV1,
    record: &str,
    digest: &str,
    expected: &ReviewedSourceSnapshotExpectedV1,
    snapshot: &ValidatedReviewedSourceSnapshotV1,
    cargo: &TrustedBuildToolV1,
    rustc: &TrustedBuildToolV1,
    policy: &ReviewedBuildPolicyV1,
) -> bool {
    exact_attempt_basics(attempt, record, digest, expected)
        && attempt.snapshot_id == snapshot.snapshot_id
        && attempt.snapshot_authority_digest == snapshot.authority_digest
        && attempt.snapshot_manifest_digest == snapshot.manifest_digest
        && &attempt.cargo == cargo
        && &attempt.rustc == rustc
        && &attempt.policy == policy
        && digest_attempt(attempt)
            .map(|value| value == attempt.attempt_digest)
            .unwrap_or(false)
}

fn exact_attempt_basics(
    attempt: &ReviewedBuildAttemptV1,
    record: &str,
    digest: &str,
    expected: &ReviewedSourceSnapshotExpectedV1,
) -> bool {
    attempt.schema_version == ATTEMPT_SCHEMA_VERSION
        && valid_attempt_id(&attempt.build_attempt_id)
        && valid_attempt_id(&attempt.confirmation_token)
        && attempt.review_record_id == record
        && attempt.review_authority_sha256 == digest.to_ascii_lowercase()
        && attempt.snapshot_expected == *expected
}

fn valid_claim(claim: &ReviewedBuildClaimV1, attempt: &ReviewedBuildAttemptV1) -> bool {
    claim.schema_version == CLAIM_SCHEMA_VERSION
        && claim.build_attempt_id == attempt.build_attempt_id
        && valid_attempt_id(&claim.owner_id)
        && claim.spawn_generation == 1
        && claim.state == "SPAWN_OWNER_RESERVED"
}

fn valid_owner_proof(proof: &ReviewedBuildOwnerProofV1, claim: &ReviewedBuildClaimV1) -> bool {
    proof.schema_version == 1
        && proof.build_attempt_id == claim.build_attempt_id
        && proof.owner_id == claim.owner_id
        && proof.spawn_generation == claim.spawn_generation
}

/// Ownership is carried by an immutable create-once proof, not by replacing
/// `claim.json`.  Both records are opened relative to the same pinned control
/// directory, so replay cannot smuggle a pathname-derived worker transition.
fn control_has_valid_owner_proof(
    control: &BuildControlRoot,
    claim: &ReviewedBuildClaimV1,
) -> Result<bool, String> {
    let proof: ReviewedBuildOwnerProofV1 = control_read_json(control, "worker-owner.json")?;
    Ok(valid_owner_proof(&proof, claim))
}

/// Reconstruct the only retryable historical shape.  A terminal result alone
/// is deliberately insufficient: it must be the exact immutable attempt,
/// reserved claim, and one-worker proof that produced a terminal failure.
fn terminal_failure_audit(
    control: &BuildControlRoot,
    attempt: &ReviewedBuildAttemptV1,
) -> Result<ReviewedBuildTerminalAuditV1, &'static str> {
    if digest_attempt(attempt)
        .map(|value| value != attempt.attempt_digest)
        .unwrap_or(true)
    {
        return Err("REVIEWED_BUILD_STATE_INVALID");
    }
    let claim: ReviewedBuildClaimV1 =
        control_read_json(control, "claim.json").map_err(|_| "REVIEWED_BUILD_STATE_INVALID")?;
    let owner: ReviewedBuildOwnerProofV1 = control_read_json(control, "worker-owner.json")
        .map_err(|_| "REVIEWED_BUILD_STATE_INVALID")?;
    let result: ReviewedBuildResultV1 =
        control_read_json(control, "result.json").map_err(|_| "REVIEWED_BUILD_NOT_TERMINAL")?;
    if !valid_claim(&claim, attempt)
        || !valid_owner_proof(&owner, &claim)
        || result.schema_version != RESULT_SCHEMA_VERSION
        || result.build_attempt_id != attempt.build_attempt_id
        || result.owner_id != claim.owner_id
    {
        return Err("REVIEWED_BUILD_STATE_INVALID");
    }
    if !retryable_terminal_result(&result) {
        return Err("REVIEWED_BUILD_NOT_TERMINAL");
    }
    Ok(ReviewedBuildTerminalAuditV1 {
        schema_version: 1,
        attempt: attempt.clone(),
        claim,
        owner,
        result,
    })
}

fn unclaimed_prepared_attempt(
    control: &BuildControlRoot,
    attempt: &ReviewedBuildAttemptV1,
) -> Result<bool, String> {
    if digest_attempt(attempt)? != attempt.attempt_digest {
        return Err("REVIEWED_BUILD_STATE_INVALID".into());
    }
    for child in [
        "claim.json",
        "worker-owner.json",
        "result.json",
        "attestation.json",
    ] {
        if read_optional_relative_regular(
            &control.guard,
            child,
            MAX_STATE_BYTES as u64,
            "reviewed build unclaimed prepared state",
        )
        .map_err(|_| "REVIEWED_BUILD_STATE_INVALID".to_string())?
        .is_some()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn unclaimed_prepared_evidence_digest(attempt: &ReviewedBuildAttemptV1) -> Result<String, String> {
    let bytes = serde_json::to_vec(&("CATDESK_REVIEWED_BUILD_UNCLAIMED_PREPARED_V1", attempt))
        .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())?;
    Ok(sha256(&bytes))
}

fn prepare_unclaimed_prepared_supersession(
    control: &BuildControlRoot,
    prior: &ReviewedBuildAttemptV1,
    fresh: ReviewedBuildAttemptV1,
) -> Result<(String, ReviewedBuildPublicOutcomeV1), String> {
    if !unclaimed_prepared_attempt(control, prior)? {
        return Err("REVIEWED_BUILD_BINDING_IMMUTABLE".into());
    }
    let evidence_digest = unclaimed_prepared_evidence_digest(prior)?;
    let prior_name = retry_child_name(&prior.build_attempt_id)?;

    let mut plans = control.base_guard.try_clone("reviewed build retry plans")?;
    plans.descend_or_create(RETRY_PLAN_DIRECTORY, "reviewed build retry plans")?;
    if let Some(existing) = read_optional_retry_plan(&plans, &prior_name)? {
        return resume_retry_plan(control, prior, &evidence_digest, existing, &fresh);
    }

    let fresh_bytes = serialize_control_record(&fresh)?;
    let mut generations = control
        .base_guard
        .try_clone("reviewed build retry generations")?;
    generations.descend_or_create(
        RETRY_GENERATION_DIRECTORY,
        "reviewed build retry generations",
    )?;
    let mut fresh_guard = generations.try_clone("reviewed build retry generation")?;
    fresh_guard
        .create_child(&fresh.build_attempt_id, "reviewed build retry generation")
        .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())?;
    write_or_validate_new_control_record(
        &fresh_guard,
        "attempt.json",
        &fresh_bytes,
        "reviewed build retry generation",
    )?;

    let plan = ReviewedBuildRetryPlanV1 {
        schema_version: 1,
        source_attempt_id: prior.build_attempt_id.clone(),
        source_attempt_digest: prior.attempt_digest.clone(),
        audit_digest: evidence_digest,
        fresh_attempt_id: fresh.build_attempt_id.clone(),
        fresh_attempt_digest: fresh.attempt_digest.clone(),
        fresh_confirmation_token: fresh.confirmation_token.clone(),
    };
    let plan_bytes = serialize_control_record(&plan)?;
    match write_new_regular_in(
        &plans,
        &prior_name,
        &plan_bytes,
        "reviewed build retry plans",
    ) {
        Ok(()) => activate_retry_plan(control, prior, &plan)?,
        Err(_) => {
            let existing: ReviewedBuildRetryPlanV1 =
                read_control_record(&plans, &prior_name, "reviewed build retry plans")?;
            return resume_retry_plan(control, prior, &plan.audit_digest, existing, &fresh);
        }
    }
    Ok((
        plan.fresh_confirmation_token,
        ReviewedBuildPublicOutcomeV1 { state: "PREPARED" },
    ))
}

fn retryable_terminal_result(result: &ReviewedBuildResultV1) -> bool {
    result.state == "BUILD_FAILED_OR_AMBIGUOUS"
        && result.attestation_digest.is_none()
        && !result
            .failure_code
            .as_deref()
            .unwrap_or_default()
            .is_empty()
}

fn serialize_control_record<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE")?;
    if bytes.is_empty() || bytes.len() > MAX_STATE_BYTES {
        return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
    }
    Ok(bytes)
}

fn write_or_validate_new_control_record(
    parent: &ProtectedDirectoryGuard,
    name: &str,
    bytes: &[u8],
    label: &str,
) -> Result<(), String> {
    match write_new_regular_in(parent, name, bytes, label) {
        Ok(()) => Ok(()),
        Err(_) => {
            let found = read_relative_regular(parent, name, MAX_STATE_BYTES as u64, label)
                .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())?;
            if found == bytes {
                Ok(())
            } else {
                Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into())
            }
        }
    }
}

fn retry_child_name(attempt_id: &str) -> Result<String, String> {
    if !valid_attempt_id(attempt_id) {
        return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
    }
    Ok(format!("{attempt_id}.json"))
}

/// Preserve the failed generation first, then publish one immutable retry
/// plan.  The active pointer is the last mutation: a crash before it leaves a
/// verified but inert plan, while a concurrent exact PREPARE observes that
/// same plan and converges on its fresh token.
fn prepare_terminal_retry(
    control: &BuildControlRoot,
    prior: &ReviewedBuildAttemptV1,
    audit: ReviewedBuildTerminalAuditV1,
    fresh: ReviewedBuildAttemptV1,
) -> Result<(String, ReviewedBuildPublicOutcomeV1), String> {
    let audit_bytes = serialize_control_record(&audit)?;
    let audit_digest = sha256(&audit_bytes);
    let prior_name = retry_child_name(&prior.build_attempt_id)?;

    let mut history = control
        .base_guard
        .try_clone("reviewed build terminal history")?;
    history.descend_or_create(RETRY_HISTORY_DIRECTORY, "reviewed build terminal history")?;
    write_or_validate_new_control_record(
        &history,
        &prior_name,
        &audit_bytes,
        "reviewed build terminal history",
    )?;

    let mut plans = control.base_guard.try_clone("reviewed build retry plans")?;
    plans.descend_or_create(RETRY_PLAN_DIRECTORY, "reviewed build retry plans")?;
    // Recover an already durable plan before allocating another inert
    // generation.  Its target must bind the same fully remeasured authority
    // lineage; an exact replay converges, while a competing new review record
    // cannot borrow the first lineage's confirmation token.
    if let Some(existing) = read_optional_retry_plan(&plans, &prior_name)? {
        return resume_retry_plan(control, prior, &audit_digest, existing, &fresh);
    }

    let fresh_digest = fresh.attempt_digest.clone();
    let fresh_bytes = serialize_control_record(&fresh)?;
    let mut generations = control
        .base_guard
        .try_clone("reviewed build retry generations")?;
    generations.descend_or_create(
        RETRY_GENERATION_DIRECTORY,
        "reviewed build retry generations",
    )?;
    let mut fresh_guard = generations.try_clone("reviewed build retry generation")?;
    match fresh_guard.create_child(&fresh.build_attempt_id, "reviewed build retry generation") {
        Ok(()) => write_or_validate_new_control_record(
            &fresh_guard,
            "attempt.json",
            &fresh_bytes,
            "reviewed build retry generation",
        )?,
        Err(_) => {
            // An unplanned same-name generation cannot be adopted. UUIDs are
            // internal, so this is a fail-closed collision rather than a path
            // recovery surface.
            return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
        }
    }

    let plan = ReviewedBuildRetryPlanV1 {
        schema_version: 1,
        source_attempt_id: prior.build_attempt_id.clone(),
        source_attempt_digest: prior.attempt_digest.clone(),
        audit_digest,
        fresh_attempt_id: fresh.build_attempt_id.clone(),
        fresh_attempt_digest: fresh_digest,
        fresh_confirmation_token: fresh.confirmation_token.clone(),
    };
    let plan_bytes = serialize_control_record(&plan)?;
    match write_new_regular_in(
        &plans,
        &prior_name,
        &plan_bytes,
        "reviewed build retry plans",
    ) {
        Ok(()) => activate_retry_plan(control, prior, &plan)?,
        Err(_) => {
            let existing: ReviewedBuildRetryPlanV1 =
                read_control_record(&plans, &prior_name, "reviewed build retry plans")?;
            return resume_retry_plan(control, prior, &plan.audit_digest, existing, &fresh);
        }
    }
    Ok((
        plan.fresh_confirmation_token,
        ReviewedBuildPublicOutcomeV1 { state: "PREPARED" },
    ))
}

fn read_optional_retry_plan(
    plans: &ProtectedDirectoryGuard,
    name: &str,
) -> Result<Option<ReviewedBuildRetryPlanV1>, String> {
    let bytes = read_optional_relative_regular(
        plans,
        name,
        MAX_STATE_BYTES as u64,
        "reviewed build retry plans",
    )
    .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())?;
    bytes
        .map(|value| {
            if value.is_empty() || value.len() > MAX_STATE_BYTES {
                return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
            }
            serde_json::from_slice(&value).map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".into())
        })
        .transpose()
}

fn resume_retry_plan(
    control: &BuildControlRoot,
    prior: &ReviewedBuildAttemptV1,
    audit_digest: &str,
    plan: ReviewedBuildRetryPlanV1,
    requested: &ReviewedBuildAttemptV1,
) -> Result<(String, ReviewedBuildPublicOutcomeV1), String> {
    if plan.schema_version != 1
        || plan.source_attempt_id != prior.build_attempt_id
        || plan.source_attempt_digest != prior.attempt_digest
        || plan.audit_digest != audit_digest
    {
        return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
    }
    let stored = retry_plan_target(control, &plan)?;
    if !same_attempt_authority_binding(&stored, requested) {
        return Err("REVIEWED_BUILD_BINDING_IMMUTABLE".into());
    }
    activate_retry_plan(control, prior, &plan)?;
    Ok((
        plan.fresh_confirmation_token,
        ReviewedBuildPublicOutcomeV1 { state: "PREPARED" },
    ))
}

fn read_control_record<T: serde::de::DeserializeOwned>(
    parent: &ProtectedDirectoryGuard,
    name: &str,
    label: &str,
) -> Result<T, String> {
    let bytes = read_relative_regular(parent, name, MAX_STATE_BYTES as u64, label)
        .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())?;
    if bytes.is_empty() || bytes.len() > MAX_STATE_BYTES {
        return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".into())
}

fn retry_plan_target(
    control: &BuildControlRoot,
    plan: &ReviewedBuildRetryPlanV1,
) -> Result<ReviewedBuildAttemptV1, String> {
    if plan.schema_version != 1
        || !valid_attempt_id(&plan.fresh_attempt_id)
        || !valid_attempt_id(&plan.fresh_confirmation_token)
        || !valid_sha256(&plan.fresh_attempt_digest)
    {
        return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
    }
    let mut generation = control
        .base_guard
        .try_clone("reviewed build retry generation")?;
    generation.descend_existing(
        RETRY_GENERATION_DIRECTORY,
        "reviewed build retry generation",
    )?;
    generation.descend_existing(&plan.fresh_attempt_id, "reviewed build retry generation")?;
    let fresh: ReviewedBuildAttemptV1 = read_control_record(
        &generation,
        "attempt.json",
        "reviewed build retry generation",
    )?;
    if fresh.build_attempt_id != plan.fresh_attempt_id
        || fresh.attempt_digest != plan.fresh_attempt_digest
        || fresh.confirmation_token != plan.fresh_confirmation_token
        || digest_attempt(&fresh)? != fresh.attempt_digest
    {
        return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
    }
    Ok(fresh)
}

fn validate_retry_plan_target(
    control: &BuildControlRoot,
    plan: &ReviewedBuildRetryPlanV1,
) -> Result<(), String> {
    retry_plan_target(control, plan).map(|_| ())
}

/// Compare only the product-remeasured authority lineage.  Attempt IDs,
/// confirmation tokens, and attempt digests are intentionally excluded: they
/// are freshly generated internal execution state, never caller authority.
fn same_attempt_authority_binding(
    stored: &ReviewedBuildAttemptV1,
    requested: &ReviewedBuildAttemptV1,
) -> bool {
    stored.schema_version == ATTEMPT_SCHEMA_VERSION
        && requested.schema_version == ATTEMPT_SCHEMA_VERSION
        && stored.review_session_id == requested.review_session_id
        && stored.review_record_id == requested.review_record_id
        && stored.review_authority_sha256 == requested.review_authority_sha256
        && stored.snapshot_expected == requested.snapshot_expected
        && stored.snapshot_id == requested.snapshot_id
        && stored.snapshot_authority_digest == requested.snapshot_authority_digest
        && stored.snapshot_manifest_digest == requested.snapshot_manifest_digest
        && stored.cargo == requested.cargo
        && stored.rustc == requested.rustc
        && stored.policy == requested.policy
        && digest_attempt(stored)
            .map(|value| value == stored.attempt_digest)
            .unwrap_or(false)
        && digest_attempt(requested)
            .map(|value| value == requested.attempt_digest)
            .unwrap_or(false)
}

fn activate_retry_plan(
    control: &BuildControlRoot,
    prior: &ReviewedBuildAttemptV1,
    plan: &ReviewedBuildRetryPlanV1,
) -> Result<(), String> {
    validate_retry_plan_target(control, plan)?;
    let pointer = ReviewedBuildActiveGenerationV1 {
        schema_version: 1,
        source_attempt_id: prior.build_attempt_id.clone(),
        source_attempt_digest: prior.attempt_digest.clone(),
        active_attempt_id: plan.fresh_attempt_id.clone(),
        active_attempt_digest: plan.fresh_attempt_digest.clone(),
    };
    let bytes = serialize_control_record(&pointer)?;
    match read_optional_relative_regular(
        &control.base_guard,
        ACTIVE_GENERATION_FILE,
        MAX_STATE_BYTES as u64,
        "reviewed build active generation",
    )
    .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())?
    {
        None => match write_new_regular_in(
            &control.base_guard,
            ACTIVE_GENERATION_FILE,
            &bytes,
            "reviewed build active generation",
        ) {
            Ok(()) => Ok(()),
            Err(_) => ensure_exact_active_pointer(&control.base_guard, &pointer),
        },
        Some(_) => {
            let existing = read_active_pointer(&control.base_guard)?;
            if existing == pointer {
                return Ok(());
            }
            if existing.active_attempt_id != prior.build_attempt_id
                || existing.active_attempt_digest != prior.attempt_digest
            {
                return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
            }
            write_unique_regular_for_atomic_replace(
                &control.base_guard,
                &bytes,
                "reviewed build active generation",
            )?
            .commit_replace(ACTIVE_GENERATION_FILE, "reviewed build active generation")?;
            ensure_exact_active_pointer(&control.base_guard, &pointer)
        }
    }
}

fn ensure_exact_active_pointer(
    base: &ProtectedDirectoryGuard,
    expected: &ReviewedBuildActiveGenerationV1,
) -> Result<(), String> {
    if read_active_pointer(base)? == *expected {
        Ok(())
    } else {
        Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into())
    }
}

fn materialize_snapshot(
    snapshot: &ValidatedReviewedSourceSnapshotV1,
    control: &BuildControlRoot,
    attempt: &str,
) -> Result<PathBuf, String> {
    let mut root = control.guard.try_clone("reviewed build source")?;
    // Retry generations are already attempt-scoped and revalidated by
    // `resolve_active_control_root`. Repeating `builds/<attempt>` beneath that
    // root needlessly lengthens every Cargo/linker output path on Windows.
    // Preserve the historical layout only for the legacy unversioned root.
    if control.guard.path() == control.base_guard.path() {
        root.create_child("builds", "reviewed build source")?;
        root.create_child(attempt, "reviewed build source")?;
    }
    root.create_child("source", "reviewed build source")?;
    materialize_snapshot_into(snapshot, &mut root)?;
    Ok(root.path().to_path_buf())
}

/// Materialize immutable snapshot bytes beneath an already-pinned,
/// caller-owned directory. The worker uses this under its protected control
/// root; the test-only host linker diagnostic uses the same no-follow write
/// path beneath its disposable non-authority root.
fn materialize_snapshot_into(
    snapshot: &ValidatedReviewedSourceSnapshotV1,
    root: &mut ProtectedDirectoryGuard,
) -> Result<(), String> {
    let snapshot_root =
        ProtectedDirectoryGuard::acquire(&snapshot.bytes_root, "reviewed source snapshot bytes")?;
    for entry in &snapshot.entries {
        if !safe_candidate_relative_path(&entry.relative_path) {
            return Err("REVIEWED_SOURCE_SNAPSHOT_REQUIRED".into());
        }
        let pieces = entry.relative_path.split('/').collect::<Vec<_>>();
        let (leaf, parents) = pieces
            .split_last()
            .ok_or_else(|| "REVIEWED_SOURCE_SNAPSHOT_REQUIRED".to_string())?;
        let mut source_parent = snapshot_root.try_clone("reviewed source snapshot bytes")?;
        let mut target_parent = root.try_clone("reviewed build source")?;
        for component in parents {
            source_parent.descend_existing(component, "reviewed source snapshot bytes")?;
            // Multiple immutable snapshot entries commonly share a parent
            // (for example `scripts/*`). Reopen that exact child beneath the
            // already pinned target parent when it exists; the shared helper
            // permits only exact open/not-found/create/collision transitions
            // and revalidates directory, reparse, and stable identity.
            target_parent.descend_or_create(component, "reviewed build source")?;
        }
        let bytes = read_relative_regular(
            &source_parent,
            leaf,
            MAX_CANDIDATE_BYTES,
            "reviewed source snapshot bytes",
        )?;
        if sha256(&bytes) != entry.sha256 {
            return Err("REVIEWED_SOURCE_SNAPSHOT_REQUIRED".into());
        }
        write_new_regular_in(&target_parent, leaf, &bytes, "reviewed build source")?;
    }
    Ok(())
}

fn trusted_toolchain() -> Result<(TrustedBuildToolV1, TrustedBuildToolV1), String> {
    #[cfg(windows)]
    {
        let cargo = Path::new(r"C:\Program Files\Rust\bin\cargo.exe");
        let rustc = Path::new(r"C:\Program Files\Rust\bin\rustc.exe");
        // The machine-wide installer remains the deterministic first policy.
        // We fall back only when both exact slots are genuinely absent; a
        // partially-installed, reparse, unreadable, or otherwise ambiguous
        // fixed slot is an authority failure, never a hint to search elsewhere.
        match fixed_program_files_pair_state(cargo, rustc)? {
            FixedProgramFilesPair::Present => Ok((
                trusted_tool_from_absolute(cargo, "cargo")?,
                trusted_tool_from_absolute(rustc, "rustc")?,
            )),
            FixedProgramFilesPair::BothAbsent => trusted_rustup_toolchain(),
        }
    }
    #[cfg(not(windows))]
    Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into())
}

#[cfg(windows)]
enum FixedProgramFilesPair {
    Present,
    BothAbsent,
}

#[cfg(windows)]
fn fixed_program_files_pair_state(
    cargo: &Path,
    rustc: &Path,
) -> Result<FixedProgramFilesPair, String> {
    fn exact_slot(path: &Path) -> Result<bool, String> {
        match fs::symlink_metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into()),
        }
    }
    match (exact_slot(cargo)?, exact_slot(rustc)?) {
        (false, false) => Ok(FixedProgramFilesPair::BothAbsent),
        (true, true) => Ok(FixedProgramFilesPair::Present),
        // A partially-present machine installer is not a Rustup selection
        // signal.  It is a corrupt/ambiguous fixed policy installation.
        _ => Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into()),
    }
}

#[cfg(windows)]
fn trusted_rustup_toolchain() -> Result<(TrustedBuildToolV1, TrustedBuildToolV1), String> {
    let profile = os_current_user_profile()?;
    validate_profile_directory(&profile)?;
    let cargo_home = profile.join(".cargo");
    let rustup_home = profile.join(".rustup");
    let rustup = cargo_home.join("bin").join("rustup.exe");
    validate_path_components(&cargo_home, true)?;
    validate_path_components(&rustup_home, true)?;
    // Rustup itself is attested before it is asked a read-only discovery
    // question.  Its output is never executable authority.
    let _rustup_evidence = trusted_tool_from_absolute(&rustup, "rustup")?;
    let selected = rustup_default_toolchain(&rustup, &cargo_home, &rustup_home, &profile)?;
    let bin = rustup_home.join("toolchains").join(selected).join("bin");
    let cargo = bin.join("cargo.exe");
    let rustc = bin.join("rustc.exe");
    let cargo = validate_rustup_discovered_tool(&cargo, &rustup_home, "cargo")?;
    let rustc = validate_rustup_discovered_tool(&rustc, &rustup_home, "rustc")?;
    let cargo_root = cargo
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
    let rustc_root = rustc
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
    if cargo_root != rustc_root {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    Ok((
        trusted_tool_from_absolute(&cargo, "cargo")?,
        trusted_tool_from_absolute(&rustc, "rustc")?,
    ))
}

/// Resolve the OS system drive through the Windows directory API rather than
/// inheriting a caller-controlled environment value. Rust's MSVC discovery
/// needs this one standard root after the reviewed worker clears its
/// environment.
// Derive MSVC's required SystemRoot from Windows, not the ambient environment.
#[cfg(windows)]
fn os_system_root() -> Result<OsString, String> {
    unsafe extern "system" {
        fn GetWindowsDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }
    let mut buffer = [0u16; 32_768];
    let length = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length < 3 || length as usize >= buffer.len() {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    let windows = String::from_utf16(&buffer[..length as usize])
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
    let bytes = windows.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
    {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    Ok(OsString::from(windows))
}

#[cfg(windows)]
fn os_system_drive() -> Result<OsString, String> {
    let root = os_system_root()?.to_string_lossy().into_owned();
    Ok(OsString::from(format!("{}:", root.as_bytes()[0] as char)))
}

#[cfg(not(windows))]
fn os_system_root() -> Result<OsString, String> {
    Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into())
}

#[cfg(not(windows))]
fn os_system_drive() -> Result<OsString, String> {
    Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into())
}

/// Resolve the current-user profile through the Windows known-folder API, not
/// environment variables.  The returned path is only a root for fixed policy
/// descendants; no caller supplies an executable or lookup path.
#[cfg(windows)]
fn os_current_user_profile() -> Result<PathBuf, String> {
    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }
    unsafe extern "system" {
        fn SHGetKnownFolderPath(
            id: *const Guid,
            flags: u32,
            token: *mut c_void,
            path: *mut *mut u16,
        ) -> i32;
        fn CoTaskMemFree(memory: *mut c_void);
    }
    const FOLDERID_PROFILE: Guid = Guid {
        data1: 0x5e6c_858f,
        data2: 0x0e22,
        data3: 0x4760,
        data4: [0x9a, 0xfe, 0xea, 0x33, 0x17, 0xb6, 0x71, 0x73],
    };
    let mut raw = std::ptr::null_mut();
    if unsafe { SHGetKnownFolderPath(&FOLDERID_PROFILE, 0, std::ptr::null_mut(), &mut raw) } < 0
        || raw.is_null()
    {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    let mut length = 0usize;
    while length <= 32_768 && unsafe { *raw.add(length) } != 0 {
        length += 1;
    }
    if length == 0 || length > 32_768 {
        unsafe { CoTaskMemFree(raw.cast()) };
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    let profile = PathBuf::from(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(raw, length)
    }));
    unsafe { CoTaskMemFree(raw.cast()) };
    validate_path_components(&profile, true)
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
    Ok(profile)
}

#[cfg(windows)]
fn validate_profile_directory(profile: &Path) -> Result<(), String> {
    if !profile.is_absolute()
        || !profile.is_dir()
        || profile.components().any(|component| {
            !matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::Normal(_)
            )
        })
    {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    validate_path_components(profile, true)
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into())
}

#[cfg(windows)]
fn rustup_default_toolchain(
    rustup: &Path,
    cargo_home: &Path,
    rustup_home: &Path,
    profile: &Path,
) -> Result<String, String> {
    let mut child = Command::new(rustup)
        .arg("default")
        .current_dir(profile)
        .env_clear()
        .env("CARGO_HOME", cargo_home)
        .env("RUSTUP_HOME", rustup_home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
    let reader = std::thread::spawn(move || {
        let mut reader = stdout.take((MAX_RUSTUP_DISCOVERY_OUTPUT + 1) as u64);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map(|_| bytes)
    });
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?
        {
            if !status.success() {
                return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
            }
            break;
        }
        if started.elapsed() > RUSTUP_DISCOVERY_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = reader
        .join()
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?;
    if output.is_empty()
        || output.len() > MAX_RUSTUP_DISCOVERY_OUTPUT
        || output.contains(&0)
        || output.iter().filter(|byte| **byte == b'\n').count() != 1
    {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    let line = std::str::from_utf8(&output)
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?
        .strip_suffix("\r\n")
        .or_else(|| std::str::from_utf8(&output).ok()?.strip_suffix('\n'))
        .filter(|line| !line.is_empty() && !line.contains('\r'))
        .ok_or_else(|| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
    let name = line
        .strip_suffix(" (default)")
        .ok_or_else(|| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    Ok(name.to_string())
}

#[cfg(windows)]
fn validate_rustup_discovered_tool(
    path: &Path,
    rustup_home: &Path,
    expected_name: &str,
) -> Result<PathBuf, String> {
    let toolchains = fs::canonicalize(rustup_home.join("toolchains"))
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?;
    let canonical = fs::canonicalize(path).map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?;
    let expected_file = format!("{expected_name}.exe");
    let bin = canonical
        .parent()
        .ok_or_else(|| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
    let toolchain = bin
        .parent()
        .ok_or_else(|| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".to_string())?;
    if canonical
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case(&expected_file))
        != Some(true)
        || bin.file_name().and_then(|name| name.to_str()) != Some("bin")
        || toolchain.parent() != Some(toolchains.as_path())
        || !canonical.starts_with(&toolchains)
    {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    // `path` was constructed from the trusted profile root and was checked
    // component-by-component before canonicalization; retain that spelling
    // for the handle opener, which rejects every reparse component itself.
    validate_path_components(path, true).map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?;
    Ok(path.to_path_buf())
}

fn trusted_tool_from_absolute(
    path: &Path,
    expected_name: &str,
) -> Result<TrustedBuildToolV1, String> {
    if !path.is_absolute()
        || path
            .file_stem()
            .and_then(|v| v.to_str())
            .map(|v| v.eq_ignore_ascii_case(expected_name))
            != Some(true)
    {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    validate_path_components(path, false)?;
    let mut pinned = open_trusted_tool_file(path)?;
    let length = pinned
        .file
        .metadata()
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?
        .len();
    if length == 0 || length > MAX_CANDIDATE_BYTES {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    let file_sha256 = sha256_open_file(&mut pinned.file, MAX_CANDIDATE_BYTES)?;
    let mut version_command = Command::new(path);
    version_command
        .arg("--version")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    if expected_name == "rustup" {
        // Rustup's own version command requires its homes to locate the
        // resolver metadata. These are derived again from the OS profile, not
        // inherited from the caller's process.
        let profile = os_current_user_profile()?;
        version_command
            .env("CARGO_HOME", profile.join(".cargo"))
            .env("RUSTUP_HOME", profile.join(".rustup"));
    }
    let version = version_command
        .output()
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?;
    if !version.status.success() || version.stdout.is_empty() || version.stdout.len() > 4096 {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    let canonical = fs::canonicalize(path).map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?;
    Ok(TrustedBuildToolV1 {
        absolute_path: canonical.to_string_lossy().into_owned(),
        sha256: file_sha256,
        length,
        identity: pinned.identity,
        version_sha256: sha256(version.stdout),
    })
}

struct ToolLaunchPin {
    evidence: TrustedBuildToolV1,
    _file: fs::File,
}

fn open_attested_tool(expected: &TrustedBuildToolV1, name: &str) -> Result<ToolLaunchPin, String> {
    let path = Path::new(&expected.absolute_path);
    let evidence = trusted_tool_from_absolute(path, name)?;
    if &evidence != expected {
        return Err("REVIEWED_BUILD_EVIDENCE_DRIFTED".into());
    }
    let pin = open_trusted_tool_file(path)?;
    let mut file = pin.file;
    let hash = sha256_open_file(&mut file, MAX_CANDIDATE_BYTES)?;
    if hash != expected.sha256 || pin.identity != expected.identity {
        return Err("REVIEWED_BUILD_EVIDENCE_DRIFTED".into());
    }
    Ok(ToolLaunchPin {
        evidence,
        _file: file,
    })
}

struct PinnedToolFile {
    file: fs::File,
    identity: String,
}

#[cfg(windows)]
fn open_trusted_tool_file(path: &Path) -> Result<PinnedToolFile, String> {
    const GENERIC_READ: u32 = 0x8000_0000;
    // Cargo/Rustup installs commonly use hard-linked proxy executables. The
    // parent Cargo process may retain a write-share handle to that image; keep
    // write sharing for compatibility but never grant delete sharing, so the
    // pinned file cannot be directory-entry replaced while it is attested.
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    #[repr(C)]
    #[derive(Default)]
    struct Information {
        attributes: u32,
        creation_low: u32,
        creation_high: u32,
        access_low: u32,
        access_high: u32,
        write_low: u32,
        write_high: u32,
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    unsafe extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            disposition: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
        fn GetFileInformationByHandle(handle: *mut c_void, information: *mut Information) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    let name: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle as isize == -1 {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    let mut information = Information::default();
    if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0
        || information.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        unsafe {
            CloseHandle(handle);
        }
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    Ok(PinnedToolFile {
        file: unsafe { fs::File::from_raw_handle(handle) },
        identity: format!(
            "{:08x}:{:08x}:{:08x}",
            information.volume, information.index_high, information.index_low
        ),
    })
}

#[cfg(not(windows))]
fn open_trusted_tool_file(_path: &Path) -> Result<PinnedToolFile, String> {
    Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into())
}

fn sha256_open_file(file: &mut fs::File, max: u64) -> Result<String, String> {
    let length = file
        .metadata()
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?
        .len();
    if length > max {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?;
    let mut digest = Sha256::new();
    let copied = std::io::copy(&mut file.take(max.saturating_add(1)), &mut digest)
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?;
    if copied != length {
        return Err("REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE")?;
    Ok(format!("{:x}", digest.finalize()))
}

fn fixed_policy() -> ReviewedBuildPolicyV1 {
    let argv = BUILD_ARGUMENTS
        .iter()
        .map(|v| (*v).to_string())
        .collect::<Vec<_>>();
    ReviewedBuildPolicyV1 {
        version: BUILD_POLICY_VERSION.into(),
        policy_sha256: sha256(BUILD_POLICY_VERSION),
        argv_sha256: sha256(canonical_json(&argv).as_bytes()),
        argv,
        environment_policy_sha256: sha256(ENVIRONMENT_POLICY),
    }
}

fn control_root(workspace: &Path) -> Result<BuildControlRoot, String> {
    let mut guard = ProtectedDirectoryGuard::acquire(workspace, "reviewed build workspace")
        .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())?;
    guard
        .descend_or_create(".catdesk", "reviewed build .catdesk")
        .and_then(|_| guard.descend_or_create("reviewed-build-control", "reviewed build control"))
        .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())?;
    guard
        .assert_stable("reviewed build control")
        .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())?;
    resolve_active_control_root(guard)
}

fn descend_or_create(
    guard: &mut ProtectedDirectoryGuard,
    component: &str,
    label: &str,
) -> Result<(), String> {
    match guard.descend_existing(component, label) {
        Ok(()) => Ok(()),
        Err(_) => guard.create_child(component, label),
    }
}

/// The Cargo target parent remains pinned from before process launch through
/// opening the release output.  The path handed to Cargo is display/argv data;
/// the produced executable is never reacquired through that path.
fn build_target_guard(
    workspace: &Path,
    control: &BuildControlRoot,
    attempt: &str,
) -> Result<ProtectedDirectoryGuard, String> {
    if !valid_attempt_id(attempt) {
        return Err("REVIEWED_BUILD_EVIDENCE_DRIFTED".into());
    }

    let mut guard = if control.guard.path() == control.base_guard.path() {
        // Legacy unversioned control roots retain their historical layout.
        let mut guard = control.guard.try_clone("reviewed build target")?;
        guard.descend_existing("builds", "reviewed build target")?;
        guard.descend_existing(attempt, "reviewed build target")?;
        guard
    } else {
        // The active generation remains the authority-bearing attempt/digest
        // record, but Cargo outputs are not authority. Keep those outputs in a
        // fixed, no-follow, attempt-named workspace subtree so MSVC link.exe
        // never inherits the long .catdesk/reviewed-build-control/generations
        // prefix. The exact attempt id still prevents cross-attempt reuse.
        if control
            .guard
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            != Some(attempt)
        {
            return Err("REVIEWED_BUILD_EVIDENCE_DRIFTED".into());
        }
        let mut guard =
            ProtectedDirectoryGuard::acquire(workspace, "reviewed build target workspace")?;
        descend_or_create(&mut guard, "target-verify", "reviewed build target")?;
        descend_or_create(&mut guard, "rb", "reviewed build target")?;
        descend_or_create(&mut guard, attempt, "reviewed build target")?;
        guard
    };

    descend_or_create(&mut guard, "target", "reviewed build target")?;
    guard.assert_stable("reviewed build target")?;
    Ok(guard)
}

fn open_built_output(target: &mut ProtectedDirectoryGuard) -> Result<fs::File, String> {
    invoke_output_candidate_hook("built-output-release-descent", target.path());
    target.descend_existing("release", "reviewed build release output")?;
    // This is the first CatDesk touch of the built child. At this point Cargo
    // has exited and the release parent is pinned, but no child handle,
    // metadata, hash, or identity has yet been acquired.
    invoke_output_candidate_hook("built-output-first-authority", target.path());
    let file = open_relative_regular_file(target, "catdesk.exe", "reviewed build release output")?;
    invoke_output_candidate_hook("built-output-child-open", target.path());
    target.assert_stable("reviewed build release output")?;
    Ok(file)
}

/// Creates the only approved candidate parent chain one component at a time.
/// Every component is opened below the already-pinned parent by the shared
/// R7C RootDirectory/no-follow primitive.
fn candidate_parent_for_create(
    workspace: &Path,
    attempt: &str,
) -> Result<ProtectedDirectoryGuard, String> {
    if !valid_attempt_id(attempt) {
        return Err("REVIEWED_BUILD_OUTPUT_UNAVAILABLE".into());
    }
    let mut guard = ProtectedDirectoryGuard::acquire(workspace, "reviewed build workspace")?;
    descend_or_create(&mut guard, "target", "reviewed build candidate")?;
    invoke_output_candidate_hook("candidate-target-descent", guard.path());
    descend_or_create(&mut guard, "reviewed-builds", "reviewed build candidate")?;
    invoke_output_candidate_hook("candidate-reviewed-builds-descent", guard.path());
    descend_or_create(&mut guard, attempt, "reviewed build candidate")?;
    invoke_output_candidate_hook("candidate-attempt-descent", guard.path());
    guard.assert_stable("reviewed build candidate")?;
    Ok(guard)
}

fn create_candidate_file(parent: &ProtectedDirectoryGuard) -> Result<fs::File, String> {
    invoke_output_candidate_hook("candidate-child-create", parent.path());
    create_relative_regular_file(parent, "catdesk.exe", "reviewed build candidate")
}

fn open_candidate_for_replay(
    workspace: &Path,
    attempt: &str,
) -> Result<(ProtectedDirectoryGuard, fs::File), String> {
    if !valid_attempt_id(attempt) {
        return Err("REVIEWED_BUILD_ATTESTATION_UNAVAILABLE".into());
    }
    let mut parent = ProtectedDirectoryGuard::acquire(workspace, "reviewed build workspace")?;
    parent.descend_existing("target", "reviewed build candidate")?;
    parent.descend_existing("reviewed-builds", "reviewed build candidate")?;
    parent.descend_existing(attempt, "reviewed build candidate")?;
    invoke_output_candidate_hook("candidate-replay-pre-open", parent.path());
    let file = open_relative_regular_file(&parent, "catdesk.exe", "reviewed build candidate")?;
    invoke_output_candidate_hook("candidate-replay-open", parent.path());
    parent.assert_stable("reviewed build candidate")?;
    Ok((parent, file))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OpenRegularEvidence {
    sha256: String,
    length: u64,
    identity: String,
}

/// Measures the exact already-opened object.  It intentionally has no Path
/// argument: callers cannot close and rediscover a candidate/output by name.
fn evidence_from_open_regular(
    file: &mut fs::File,
    label: &str,
) -> Result<OpenRegularEvidence, String> {
    invoke_output_candidate_hook("candidate-evidence", Path::new(""));
    let metadata = file
        .metadata()
        .map_err(|_| format!("{label} is unavailable"))?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_CANDIDATE_BYTES {
        return Err(format!("{label} is unsafe"));
    }
    let length = metadata.len();
    file.seek(SeekFrom::Start(0))
        .map_err(|_| format!("{label} is unavailable"))?;
    let mut digest = Sha256::new();
    let copied = std::io::copy(&mut file.take(length.saturating_add(1)), &mut digest)
        .map_err(|_| format!("{label} is unavailable"))?;
    if copied != length {
        return Err(format!("{label} changed while reading"));
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| format!("{label} is unavailable"))?;
    let identity = opened_regular_identity(file, label)?;
    Ok(OpenRegularEvidence {
        sha256: format!("{:x}", digest.finalize()),
        length,
        identity,
    })
}

#[cfg(windows)]
fn opened_regular_identity(file: &fs::File, label: &str) -> Result<String, String> {
    #[repr(C)]
    #[derive(Default)]
    struct Information {
        attributes: u32,
        creation_low: u32,
        creation_high: u32,
        access_low: u32,
        access_high: u32,
        write_low: u32,
        write_high: u32,
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    unsafe extern "system" {
        fn GetFileInformationByHandle(handle: *mut c_void, information: *mut Information) -> i32;
    }
    let mut information = Information::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut information) } == 0 {
        return Err(format!("{label} identity is unavailable"));
    }
    Ok(format!(
        "{:08x}:{:08x}:{:08x}",
        information.volume, information.index_high, information.index_low
    ))
}

#[cfg(not(windows))]
fn opened_regular_identity(_file: &fs::File, label: &str) -> Result<String, String> {
    Err(format!("{label} stable identity is unavailable"))
}

fn copy_open_regular_files(
    source: &mut fs::File,
    target: &mut fs::File,
    expected: u64,
) -> Result<(), String> {
    invoke_output_candidate_hook("candidate-copy", Path::new(""));
    source
        .seek(SeekFrom::Start(0))
        .map_err(|_| "REVIEWED_BUILD_OUTPUT_UNAVAILABLE")?;
    let copied = std::io::copy(&mut source.take(expected.saturating_add(1)), target)
        .map_err(|_| "REVIEWED_BUILD_OUTPUT_UNAVAILABLE")?;
    if copied != expected {
        return Err("REVIEWED_BUILD_OUTPUT_UNAVAILABLE".into());
    }
    target
        .sync_all()
        .map_err(|_| "REVIEWED_BUILD_OUTPUT_UNAVAILABLE".into())
}
fn existing_control_root(workspace: &Path) -> Result<BuildControlRoot, String> {
    let mut guard = ProtectedDirectoryGuard::acquire(workspace, "reviewed build workspace")
        .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())?;
    guard
        .descend_existing(".catdesk", "reviewed build .catdesk")
        .and_then(|_| guard.descend_existing("reviewed-build-control", "reviewed build control"))
        .and_then(|_| guard.assert_stable("reviewed build control"))
        .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())?;
    resolve_active_control_root(guard)
}

/// Resolve the one product-owned active generation.  A missing pointer is the
/// legacy generation at the pinned control root; every non-missing pointer is
/// fully remeasured before its child may be used for CONFIRM, RESULT, or a
/// later retry.
fn resolve_active_control_root(
    base_guard: ProtectedDirectoryGuard,
) -> Result<BuildControlRoot, String> {
    let active = match read_optional_relative_regular(
        &base_guard,
        ACTIVE_GENERATION_FILE,
        MAX_STATE_BYTES as u64,
        "reviewed build active generation",
    )
    .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())?
    {
        None => {
            return Ok(BuildControlRoot {
                guard: base_guard.try_clone("reviewed build legacy control")?,
                base_guard,
            });
        }
        Some(bytes) => {
            if bytes.is_empty() || bytes.len() > MAX_STATE_BYTES {
                return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
            }
            serde_json::from_slice::<ReviewedBuildActiveGenerationV1>(&bytes)
                .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())?
        }
    };
    if active.schema_version != 1
        || !valid_attempt_id(&active.source_attempt_id)
        || !valid_sha256(&active.source_attempt_digest)
        || !valid_attempt_id(&active.active_attempt_id)
        || !valid_sha256(&active.active_attempt_digest)
        || active.source_attempt_id == active.active_attempt_id
    {
        return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
    }
    let mut guard = base_guard.try_clone("reviewed build active generation")?;
    guard.descend_existing(
        RETRY_GENERATION_DIRECTORY,
        "reviewed build active generation",
    )?;
    guard.descend_existing(
        &active.active_attempt_id,
        "reviewed build active generation",
    )?;
    let attempt: ReviewedBuildAttemptV1 =
        read_control_record(&guard, "attempt.json", "reviewed build active generation")?;
    if attempt.build_attempt_id != active.active_attempt_id
        || attempt.attempt_digest != active.active_attempt_digest
        || digest_attempt(&attempt)? != attempt.attempt_digest
    {
        return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
    }
    Ok(BuildControlRoot { guard, base_guard })
}

fn read_active_pointer(
    base: &ProtectedDirectoryGuard,
) -> Result<ReviewedBuildActiveGenerationV1, String> {
    let value: ReviewedBuildActiveGenerationV1 = read_control_record(
        base,
        ACTIVE_GENERATION_FILE,
        "reviewed build active generation",
    )?;
    if value.schema_version != 1
        || !valid_attempt_id(&value.source_attempt_id)
        || !valid_sha256(&value.source_attempt_digest)
        || !valid_attempt_id(&value.active_attempt_id)
        || !valid_sha256(&value.active_attempt_digest)
        || value.source_attempt_id == value.active_attempt_id
    {
        return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
    }
    Ok(value)
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct LockedRegistryCrate {
    name: String,
    version: String,
    checksum: String,
}

/// The reviewed worker intentionally never uses the mutable unpacked
/// `registry/src` tree.  It copies only lockfile-checked crate archives and
/// the matching sparse-index entries into a fresh pinned generation-local
/// Cargo home, then Cargo runs with its fixed `--offline` argument.
fn seed_isolated_cargo_home(control: &BuildControlRoot, source: &Path) -> Result<PathBuf, String> {
    let mut destination = control
        .guard
        .try_clone("reviewed build isolated cargo home")?;
    seed_isolated_cargo_home_into(&mut destination, source)
}

/// Seed a fresh isolated Cargo closure beneath a caller-owned pinned root.
/// This keeps the source-cache validation identical for the protected worker
/// and the test-only disposable linker diagnostic, without ever executing from
/// the ambient Cargo home.
fn seed_isolated_cargo_home_into(
    destination: &mut ProtectedDirectoryGuard,
    source: &Path,
) -> Result<PathBuf, String> {
    let source_root = ProtectedDirectoryGuard::acquire(source, "reviewed build source")?;
    let lock = read_relative_regular(
        &source_root,
        "Cargo.lock",
        MAX_CARGO_LOCK_BYTES,
        "reviewed build lockfile",
    )?;
    let crates = locked_registry_crates(&lock)?;
    if crates.is_empty() {
        return Err("REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".into());
    }

    let profile = os_current_user_profile()?;
    validate_profile_directory(&profile)?;
    let mut ambient = ProtectedDirectoryGuard::acquire(
        &profile.join(".cargo"),
        "reviewed build local cargo cache",
    )?;
    ambient.descend_existing("registry", "reviewed build local cargo registry")?;
    let mut ambient_index = ambient.try_clone("reviewed build local cargo index")?;
    ambient_index.descend_existing("index", "reviewed build local cargo index")?;
    let mut ambient_cache = ambient.try_clone("reviewed build local cargo cache")?;
    ambient_cache.descend_existing("cache", "reviewed build local cargo cache")?;
    let registry_id = shared_registry_identity(&ambient_index, &ambient_cache)?;

    let mut source_index = ambient_index.try_clone("reviewed build local cargo index")?;
    source_index.descend_existing(&registry_id, "reviewed build local cargo index")?;
    let mut source_cache = ambient_cache.try_clone("reviewed build local cargo cache")?;
    source_cache.descend_existing(&registry_id, "reviewed build local cargo cache")?;
    let config = read_relative_regular(
        &source_index,
        "config.json",
        MAX_CARGO_REGISTRY_CONFIG_BYTES,
        "reviewed build local cargo index",
    )?;
    if config.is_empty() {
        return Err("REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".into());
    }

    destination.create_child("cargo-home", "reviewed build isolated cargo home")?;
    let home = destination.path().to_path_buf();
    destination.create_child("registry", "reviewed build isolated cargo registry")?;
    let mut destination_index = destination.try_clone("reviewed build isolated cargo index")?;
    destination_index.create_child("index", "reviewed build isolated cargo index")?;
    destination_index.create_child(&registry_id, "reviewed build isolated cargo index")?;
    write_new_regular_in(
        &destination_index,
        "config.json",
        &config,
        "reviewed build isolated cargo index",
    )?;
    let mut destination_cache = destination.try_clone("reviewed build isolated cargo cache")?;
    destination_cache.create_child("cache", "reviewed build isolated cargo cache")?;
    destination_cache.create_child(&registry_id, "reviewed build isolated cargo cache")?;

    copy_locked_registry_closure(
        &source_index,
        &source_cache,
        &mut destination_index,
        &destination_cache,
        &crates,
    )?;
    destination.assert_stable("reviewed build isolated cargo home")?;
    Ok(home)
}

fn locked_registry_crates(lock: &[u8]) -> Result<Vec<LockedRegistryCrate>, String> {
    let text = std::str::from_utf8(lock)
        .map_err(|_| "REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".to_string())?;
    let mut packages = BTreeSet::new();
    for section in text.split("[[package]]").skip(1) {
        let mut name = None;
        let mut version = None;
        let mut source = None;
        let mut checksum = None;
        for line in section.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim().trim_matches('"');
            match key.trim() {
                "name" => name = Some(value.to_string()),
                "version" => version = Some(value.to_string()),
                "source" => source = Some(value.to_string()),
                "checksum" => checksum = Some(value.to_string()),
                _ => {}
            }
        }
        let Some(source) = source else { continue };
        if !source.starts_with("registry+") {
            continue;
        }
        let package = LockedRegistryCrate {
            name: name.ok_or_else(|| "REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".to_string())?,
            version: version
                .ok_or_else(|| "REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".to_string())?,
            checksum: checksum
                .ok_or_else(|| "REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".to_string())?,
        };
        if !safe_registry_crate_component(&package.name)
            || !safe_registry_version_component(&package.version)
            || !valid_sha256(&package.checksum)
        {
            return Err("REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".into());
        }
        packages.insert(package);
        if packages.len() > MAX_CARGO_REGISTRY_CRATES {
            return Err("REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".into());
        }
    }
    Ok(packages.into_iter().collect())
}

fn safe_registry_crate_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn safe_registry_version_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-'))
}

fn guarded_child_names(
    parent: &ProtectedDirectoryGuard,
    label: &str,
) -> Result<BTreeSet<String>, String> {
    parent.assert_stable(label)?;
    let entries = fs::read_dir(parent.path())
        .map_err(|_| "REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".to_string())?;
    let mut names = BTreeSet::new();
    for entry in entries {
        let name = entry
            .map_err(|_| "REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".to_string())?
            .file_name()
            .into_string()
            .map_err(|_| "REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".to_string())?;
        validate_protected_component(&name, label)
            .map_err(|_| "REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".to_string())?;
        names.insert(name);
    }
    parent.assert_stable(label)?;
    Ok(names)
}

fn shared_registry_identity(
    index: &ProtectedDirectoryGuard,
    cache: &ProtectedDirectoryGuard,
) -> Result<String, String> {
    let mut candidates = Vec::new();
    let index_names = guarded_child_names(index, "reviewed build local cargo index")?;
    let cache_names = guarded_child_names(cache, "reviewed build local cargo cache")?;
    for name in index_names.intersection(&cache_names) {
        let mut candidate = index.try_clone("reviewed build local cargo index")?;
        if candidate
            .descend_existing(name, "reviewed build local cargo index")
            .is_ok()
            && read_relative_regular(
                &candidate,
                "config.json",
                MAX_CARGO_REGISTRY_CONFIG_BYTES,
                "reviewed build local cargo index",
            )
            .is_ok()
        {
            candidates.push(name.clone());
        }
    }
    if candidates.len() == 1 {
        Ok(candidates.remove(0))
    } else {
        Err("REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".into())
    }
}

fn registry_index_path(package: &LockedRegistryCrate) -> Result<Vec<String>, String> {
    let name = package.name.to_ascii_lowercase();
    let bytes = name.as_bytes();
    let mut path = vec![".cache".to_string()];
    match bytes.len() {
        1 => path.push("1".into()),
        2 => path.push("2".into()),
        3 => {
            path.push("3".into());
            path.push((bytes[0] as char).to_string());
        }
        _ => {
            path.push(name[..2].to_string());
            path.push(name[2..4].to_string());
        }
    }
    path.push(name);
    Ok(path)
}

fn descend_existing_components(
    root: &ProtectedDirectoryGuard,
    components: &[String],
    label: &str,
) -> Result<ProtectedDirectoryGuard, String> {
    let mut guard = root.try_clone(label)?;
    for component in components {
        guard.descend_existing(component, label)?;
    }
    Ok(guard)
}

fn descend_create_components(
    root: &mut ProtectedDirectoryGuard,
    components: &[String],
    label: &str,
) -> Result<(), String> {
    for component in components {
        root.descend_or_create(component, label)?;
    }
    Ok(())
}

fn copy_locked_registry_index_entry(
    source_index: &ProtectedDirectoryGuard,
    destination_index: &mut ProtectedDirectoryGuard,
    package: &LockedRegistryCrate,
) -> Result<(), String> {
    let path = registry_index_path(package)?;
    let (leaf, parents) = path
        .split_last()
        .ok_or_else(|| "REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".to_string())?;
    let source_parent =
        descend_existing_components(source_index, parents, "reviewed build local cargo index")?;
    let bytes = read_relative_regular(
        &source_parent,
        leaf,
        MAX_CARGO_REGISTRY_INDEX_ENTRY_BYTES,
        "reviewed build local cargo index",
    )?;
    if bytes.is_empty() {
        return Err("REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".into());
    }
    let mut destination_parent =
        destination_index.try_clone("reviewed build isolated cargo index")?;
    descend_create_components(
        &mut destination_parent,
        parents,
        "reviewed build isolated cargo index",
    )?;
    write_new_regular_in(
        &destination_parent,
        leaf,
        &bytes,
        "reviewed build isolated cargo index",
    )
}

fn copy_locked_registry_closure(
    source_index: &ProtectedDirectoryGuard,
    source_cache: &ProtectedDirectoryGuard,
    destination_index: &mut ProtectedDirectoryGuard,
    destination_cache: &ProtectedDirectoryGuard,
    packages: &[LockedRegistryCrate],
) -> Result<(), String> {
    // A sparse registry index has one leaf per crate *name*, while Cargo.lock
    // may legitimately contain multiple versions of that crate. Copy the
    // shared index entry once, but retain/checksum every versioned .crate
    // archive. Re-copying the same create-new index leaf would otherwise turn
    // an ordinary multi-version lockfile into a protected-filesystem collision.
    let mut copied_index_names = BTreeSet::new();
    for package in packages {
        if copied_index_names.insert(package.name.to_ascii_lowercase()) {
            copy_locked_registry_index_entry(source_index, destination_index, package)?;
        }
        copy_locked_registry_archive(source_cache, destination_cache, package)?;
    }
    Ok(())
}

fn copy_locked_registry_archive(
    source_cache: &ProtectedDirectoryGuard,
    destination_cache: &ProtectedDirectoryGuard,
    package: &LockedRegistryCrate,
) -> Result<(), String> {
    let archive = format!("{}-{}.crate", package.name, package.version);
    validate_protected_component(&archive, "reviewed build local cargo cache")
        .map_err(|_| "REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".to_string())?;
    let bytes = read_relative_regular(
        source_cache,
        &archive,
        MAX_CARGO_REGISTRY_ARCHIVE_BYTES,
        "reviewed build local cargo cache",
    )?;
    if sha256(&bytes) != package.checksum.to_ascii_lowercase() {
        return Err("REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE".into());
    }
    write_new_regular_in(
        destination_cache,
        &archive,
        &bytes,
        "reviewed build isolated cargo cache",
    )
}
fn validate_path_components(path: &Path, final_must_exist: bool) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(value) => {
                current.push(value);
                match fs::symlink_metadata(&current) {
                    Ok(meta) => {
                        if meta.file_type().is_symlink()
                            || (!meta.file_type().is_dir() && current != path)
                        {
                            return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
                        }
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::NotFound && !final_must_exist =>
                    {
                        break;
                    }
                    Err(_) => return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into()),
                }
            }
            _ => return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into()),
        }
    }
    Ok(())
}
fn safe_candidate_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value.is_ascii()
        && Path::new(value)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        && !value.contains('\\')
}

fn control_create_json<T: Serialize>(
    control: &BuildControlRoot,
    name: &str,
    value: &T,
) -> Result<(), String> {
    if !CONTROL_CHILDREN.contains(&name) {
        return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
    }
    let bytes = serde_json::to_vec(value).map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE")?;
    if bytes.len() > MAX_STATE_BYTES {
        return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
    }
    write_new_regular_in(&control.guard, name, &bytes, "reviewed build control state")
        .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())
}

fn control_read_json<T: serde::de::DeserializeOwned>(
    control: &BuildControlRoot,
    name: &str,
) -> Result<T, String> {
    if !CONTROL_CHILDREN.contains(&name) {
        return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
    }
    let bytes = read_relative_regular(
        &control.guard,
        name,
        MAX_STATE_BYTES as u64,
        "reviewed build control state",
    )
    .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".to_string())?;
    if bytes.is_empty() || bytes.len() > MAX_STATE_BYTES {
        return Err("REVIEWED_BUILD_STATE_UNAVAILABLE".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".into())
}
fn write_regular_file_create_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE")?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".into())
}
fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE")?;
    let temp = path.with_extension(format!("{}.tmp", Uuid::new_v4().simple()));
    write_regular_file_create_new(&temp, &bytes)?;
    fs::rename(&temp, path).map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".into())
}
fn canonical_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("serializable constant")
}
fn sha256(value: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(value.as_ref()))
}
fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn valid_attempt_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .unwrap_or(0)
}
fn digest_attempt(value: &ReviewedBuildAttemptV1) -> Result<String, String> {
    let mut clone = value.clone();
    clone.attempt_digest.clear();
    serde_json::to_vec(&clone)
        .map(sha256)
        .map_err(|_| "REVIEWED_BUILD_STATE_UNAVAILABLE".into())
}
fn digest_attestation(value: &ReviewedBuildAttestationV2) -> Result<String, String> {
    let mut clone = value.clone();
    clone.attestation_digest.clear();
    serde_json::to_vec(&clone)
        .map(sha256)
        .map_err(|_| "REVIEWED_BUILD_ATTESTATION_UNAVAILABLE".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reviewed_source_snapshot::ReviewedSourceEntryV1;

    struct FakeDedicatedProducerBackend {
        state: DedicatedProducerExistingState,
        state_after_provision: DedicatedProducerExistingState,
        state_after_rollback: DedicatedProducerExistingState,
        administrator: bool,
        provision_succeeds: bool,
        rollback_succeeds: bool,
        inspect_calls: usize,
        provision_calls: usize,
        rollback_calls: usize,
        exact_policy_only: bool,
    }

    impl FakeDedicatedProducerBackend {
        fn new(state: DedicatedProducerExistingState) -> Self {
            Self {
                state,
                state_after_provision:
                    DedicatedProducerExistingState::ExactProvisionedButLiveAcceptancePending,
                state_after_rollback: DedicatedProducerExistingState::Absent,
                administrator: false,
                provision_succeeds: true,
                rollback_succeeds: true,
                inspect_calls: 0,
                provision_calls: 0,
                rollback_calls: 0,
                exact_policy_only: true,
            }
        }

        fn policy_is_exact(policy: &DedicatedProducerProvisioningPolicyV1) -> bool {
            policy.service_name == DEDICATED_PRODUCER_SERVICE_NAME
                && policy.reviewed_service_image == DEDICATED_PRODUCER_REVIEWED_IMAGE
                && policy.service_binary == DEDICATED_PRODUCER_SERVICE_BINARY
                && policy.namespace == DEDICATED_PRODUCER_NAMESPACE
                && policy.policy_sha256 == dedicated_producer_fixed_policy_digest()
                && policy.service_mode == DEDICATED_PRODUCER_SERVICE_MODE
                && policy.service_sid_policy == DEDICATED_PRODUCER_SERVICE_SID_POLICY
                && policy.namespace_security_policy == DEDICATED_PRODUCER_NAMESPACE_SECURITY_POLICY
        }
    }

    impl DedicatedProducerProvisioningBackend for FakeDedicatedProducerBackend {
        fn inspect_fixed_state(
            &mut self,
            policy: &DedicatedProducerProvisioningPolicyV1,
        ) -> Result<DedicatedProducerExistingState, String> {
            self.inspect_calls += 1;
            self.exact_policy_only &= Self::policy_is_exact(policy);
            Ok(self.state.clone())
        }

        fn administrator_gate(&mut self) -> Result<bool, String> {
            Ok(self.administrator)
        }

        fn provision_exact_fixed_boundary(
            &mut self,
            policy: &DedicatedProducerProvisioningPolicyV1,
        ) -> Result<(), String> {
            self.provision_calls += 1;
            self.exact_policy_only &= Self::policy_is_exact(policy);
            if self.provision_succeeds {
                self.state = self.state_after_provision.clone();
                Ok(())
            } else {
                self.state = DedicatedProducerExistingState::Partial;
                Err(DEDICATED_PRODUCER_PROVISIONING_REFUSED.into())
            }
        }

        fn rollback_exact_fixed_boundary(
            &mut self,
            policy: &DedicatedProducerProvisioningPolicyV1,
        ) -> Result<(), String> {
            self.rollback_calls += 1;
            self.exact_policy_only &= Self::policy_is_exact(policy);
            if self.rollback_succeeds {
                self.state = self.state_after_rollback.clone();
                Ok(())
            } else {
                Err(DEDICATED_PRODUCER_ROLLBACK_UNPROVEN.into())
            }
        }
    }

    struct FakeWindowsDedicatedProducerOperations {
        state: DedicatedProducerExistingState,
        administrator: bool,
        fail_stage: Option<DedicatedProducerProvisioningStage>,
        calls: Vec<DedicatedProducerProvisioningStage>,
        rollbacks: Vec<DedicatedProducerProvisioningStage>,
        durable_journal: Vec<DedicatedProducerProvisioningStage>,
        journal_cleared: bool,
        exact_policy_only: bool,
        interactive_recovery_blocked: bool,
        image_missing: bool,
    }

    impl FakeWindowsDedicatedProducerOperations {
        fn absent() -> Self {
            Self {
                state: DedicatedProducerExistingState::Absent,
                administrator: true,
                fail_stage: None,
                calls: Vec::new(),
                rollbacks: Vec::new(),
                durable_journal: Vec::new(),
                journal_cleared: false,
                exact_policy_only: true,
                interactive_recovery_blocked: true,
                image_missing: false,
            }
        }

        fn check(&mut self, policy: &DedicatedProducerProvisioningPolicyV1) {
            self.exact_policy_only &= FakeDedicatedProducerBackend::policy_is_exact(policy);
        }
    }

    impl DedicatedProducerWindowsOperations for FakeWindowsDedicatedProducerOperations {
        fn inspect_fixed_state(
            &mut self,
            policy: &DedicatedProducerProvisioningPolicyV1,
        ) -> Result<DedicatedProducerExistingState, String> {
            self.check(policy);
            Ok(self.state.clone())
        }

        fn current_token_is_administrator(&mut self) -> Result<bool, String> {
            Ok(self.administrator)
        }

        fn deploy_exact_reviewed_service_image(
            &mut self,
            policy: &DedicatedProducerProvisioningPolicyV1,
        ) -> Result<(), String> {
            self.check(policy);
            self.calls
                .push(DedicatedProducerProvisioningStage::ImageDeployed);
            if self.image_missing {
                Err(DEDICATED_PRODUCER_IMAGE_MISSING.into())
            } else if self.fail_stage == Some(DedicatedProducerProvisioningStage::ImageDeployed) {
                Err("fake image deployment failure".into())
            } else {
                Ok(())
            }
        }

        fn create_exact_service(
            &mut self,
            policy: &DedicatedProducerProvisioningPolicyV1,
        ) -> Result<(), String> {
            self.check(policy);
            self.calls
                .push(DedicatedProducerProvisioningStage::ServiceCreated);
            if self.fail_stage == Some(DedicatedProducerProvisioningStage::ServiceCreated) {
                Err("fake service create failure".into())
            } else {
                Ok(())
            }
        }

        fn restrict_exact_service_sid(
            &mut self,
            policy: &DedicatedProducerProvisioningPolicyV1,
        ) -> Result<(), String> {
            self.check(policy);
            self.calls
                .push(DedicatedProducerProvisioningStage::ServiceSidRestricted);
            if self.fail_stage == Some(DedicatedProducerProvisioningStage::ServiceSidRestricted) {
                Err("fake service SID failure".into())
            } else {
                Ok(())
            }
        }

        fn create_exact_namespace(
            &mut self,
            policy: &DedicatedProducerProvisioningPolicyV1,
        ) -> Result<(), String> {
            self.check(policy);
            self.calls
                .push(DedicatedProducerProvisioningStage::NamespaceCreated);
            if self.fail_stage == Some(DedicatedProducerProvisioningStage::NamespaceCreated) {
                Err("fake namespace create failure".into())
            } else {
                Ok(())
            }
        }

        fn apply_exact_namespace_security_at_creation(
            &mut self,
            policy: &DedicatedProducerProvisioningPolicyV1,
        ) -> Result<(), String> {
            self.check(policy);
            self.calls
                .push(DedicatedProducerProvisioningStage::NamespaceSecurityApplied);
            if self.fail_stage == Some(DedicatedProducerProvisioningStage::NamespaceSecurityApplied)
            {
                Err("fake namespace security failure".into())
            } else {
                Ok(())
            }
        }

        fn verify_interactive_token_cannot_recover_namespace(
            &mut self,
            policy: &DedicatedProducerProvisioningPolicyV1,
        ) -> Result<(), String> {
            self.check(policy);
            if self.interactive_recovery_blocked {
                self.state =
                    DedicatedProducerExistingState::ExactProvisionedButLiveAcceptancePending;
                Ok(())
            } else {
                self.state = DedicatedProducerExistingState::InteractiveRecoveryPossible;
                Err("interactive recovery possible".into())
            }
        }

        fn rollback_exact_stage(
            &mut self,
            stage: DedicatedProducerProvisioningStage,
            policy: &DedicatedProducerProvisioningPolicyV1,
        ) -> Result<(), String> {
            self.check(policy);
            self.rollbacks.push(stage);
            if stage == DedicatedProducerProvisioningStage::ImageDeployed {
                self.state = DedicatedProducerExistingState::Absent;
            }
            Ok(())
        }

        fn record_durable_stage(
            &mut self,
            stage: DedicatedProducerProvisioningStage,
            policy: &DedicatedProducerProvisioningPolicyV1,
        ) -> Result<(), String> {
            self.check(policy);
            self.durable_journal.push(stage);
            Ok(())
        }

        fn clear_durable_journal(
            &mut self,
            policy: &DedicatedProducerProvisioningPolicyV1,
        ) -> Result<(), String> {
            self.check(policy);
            self.durable_journal.clear();
            self.journal_cleared = true;
            Ok(())
        }
    }

    #[cfg(windows)]
    fn test_windows_toolchain_environment() -> ReviewedWindowsToolchainEnvironment {
        ReviewedWindowsToolchainEnvironment {
            path: OsString::from(r"C:\trusted\cargo;C:\trusted\msvc\bin"),
            cc: OsString::from(r"C:\trusted\msvc\bin\cl.exe"),
            ar: OsString::from(r"C:\trusted\msvc\bin\lib.exe"),
            lib: OsString::from(r"C:\trusted\msvc\lib;C:\trusted\sdk\ucrt;C:\trusted\sdk\um"),
            libpath: OsString::from(r"C:\trusted\msvc\lib;C:\trusted\sdk\ucrt;C:\trusted\sdk\um"),
            include: OsString::from(
                r"C:\trusted\msvc\include;C:\trusted\sdk\ucrt;C:\trusted\sdk\shared;C:\trusted\sdk\um;C:\trusted\sdk\winrt",
            ),
        }
    }

    fn test_attempt() -> ReviewedBuildAttemptV1 {
        let policy = fixed_policy();
        let tool = TrustedBuildToolV1 {
            absolute_path: r"C:\Program Files\Rust\bin\cargo.exe".into(),
            sha256: "a".repeat(64),
            length: 1,
            identity: "1:1".into(),
            version_sha256: "b".repeat(64),
        };
        let expected = ReviewedSourceSnapshotExpectedV1 {
            session_id: "session".into(),
            project_id: "project".into(),
            approved_contract_hash: "contract".into(),
            logical_task_id: "task".into(),
            completion_artifact_ids: vec!["src/out".into()],
            current_outputs: vec![],
            baseline_observations: vec![],
        };
        let mut value = ReviewedBuildAttemptV1 {
            schema_version: ATTEMPT_SCHEMA_VERSION,
            build_attempt_id: "c".repeat(32),
            confirmation_token: "d".repeat(32),
            review_session_id: "session".into(),
            review_record_id: "record".into(),
            review_authority_sha256: "e".repeat(64),
            snapshot_expected: expected,
            snapshot_id: "snapshot".into(),
            snapshot_authority_digest: "f".repeat(64),
            snapshot_manifest_digest: "0".repeat(64),
            cargo: tool.clone(),
            rustc: TrustedBuildToolV1 {
                absolute_path: r"C:\Program Files\Rust\bin\rustc.exe".into(),
                ..tool
            },
            policy,
            attempt_digest: String::new(),
        };
        value.attempt_digest = digest_attempt(&value).unwrap();
        value
    }

    fn test_attestation(attempt: &ReviewedBuildAttemptV1) -> ReviewedBuildAttestationV2 {
        let mut value = ReviewedBuildAttestationV2 {
            schema_version: ATTESTATION_SCHEMA_VERSION,
            producer: "CATDESK_REVIEWED_BUILD_WORKER_V2".into(),
            build_attempt_id: attempt.build_attempt_id.clone(),
            review_session_id: attempt.review_session_id.clone(),
            review_record_id: attempt.review_record_id.clone(),
            review_authority_sha256: attempt.review_authority_sha256.clone(),
            snapshot_id: attempt.snapshot_id.clone(),
            snapshot_authority_digest: attempt.snapshot_authority_digest.clone(),
            snapshot_manifest_digest: attempt.snapshot_manifest_digest.clone(),
            build_policy_sha256: attempt.policy.policy_sha256.clone(),
            build_argv_sha256: attempt.policy.argv_sha256.clone(),
            environment_policy_sha256: attempt.policy.environment_policy_sha256.clone(),
            cargo_path: attempt.cargo.absolute_path.clone(),
            cargo_sha256: attempt.cargo.sha256.clone(),
            cargo_identity: attempt.cargo.identity.clone(),
            cargo_version_sha256: attempt.cargo.version_sha256.clone(),
            rustc_path: attempt.rustc.absolute_path.clone(),
            rustc_sha256: attempt.rustc.sha256.clone(),
            rustc_identity: attempt.rustc.identity.clone(),
            rustc_version_sha256: attempt.rustc.version_sha256.clone(),
            candidate_relative_path:
                "target/reviewed-builds/cccccccccccccccccccccccccccccccc/catdesk.exe".into(),
            candidate_sha256: "1".repeat(64),
            candidate_length: 7,
            candidate_identity: "00000001:00000002:00000003".into(),
            generation: 1,
            created_at_unix: 1,
            attestation_digest: String::new(),
        };
        value.attestation_digest = digest_attestation(&value).unwrap();
        value
    }

    #[cfg(windows)]
    fn unclaimed_prepared_control_fixture() -> (PathBuf, BuildControlRoot, ReviewedBuildAttemptV1) {
        let root =
            std::env::temp_dir().join(format!("catdesk-reviewed-prepared-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let control = control_root(&root).unwrap();
        let attempt = test_attempt();
        control_create_json(&control, "attempt.json", &attempt).unwrap();
        (root, control, attempt)
    }

    #[cfg(windows)]
    #[test]
    fn unclaimed_prepared_new_authority_supersedes_once_and_replay_converges() {
        let (root, control, prior) = unclaimed_prepared_control_fixture();
        assert!(unclaimed_prepared_attempt(&control, &prior).unwrap());

        let fresh = fresh_attempt_for_authority(&prior, 'b', 'c', "new-reviewed-record");
        let (token, outcome) =
            prepare_unclaimed_prepared_supersession(&control, &prior, fresh.clone()).unwrap();
        assert_eq!(outcome.state, "PREPARED");
        assert_eq!(token, fresh.confirmation_token);
        assert_ne!(token, prior.confirmation_token);

        let active = existing_control_root(&root).unwrap();
        let selected: ReviewedBuildAttemptV1 = control_read_json(&active, "attempt.json").unwrap();
        assert_eq!(selected.build_attempt_id, fresh.build_attempt_id);
        assert_eq!(selected.review_record_id, "new-reviewed-record");
        drop(active);

        let replay = fresh_attempt_for_authority(&prior, 'd', 'e', "new-reviewed-record");
        assert_eq!(
            prepare_unclaimed_prepared_supersession(&control, &prior, replay)
                .unwrap()
                .0,
            token
        );

        assert_eq!(
            confirm_reviewed_build(&root, &prior.confirmation_token, |_attempt, _owner| Ok(()))
                .unwrap_err(),
            "REVIEWED_BUILD_CONFIRMATION_UNAVAILABLE"
        );
        drop(control);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn unclaimed_prepared_supersession_refuses_any_execution_evidence() {
        for child in [
            "claim.json",
            "worker-owner.json",
            "result.json",
            "attestation.json",
        ] {
            let (root, control, prior) = unclaimed_prepared_control_fixture();
            write_new_regular_in(
                &control.guard,
                child,
                br#"{"fixture":"present"}"#,
                "prepared supersession hostile evidence",
            )
            .unwrap();
            assert!(!unclaimed_prepared_attempt(&control, &prior).unwrap());
            let fresh = fresh_attempt_for_authority(&prior, 'b', 'c', "new-reviewed-record");
            assert_eq!(
                prepare_unclaimed_prepared_supersession(&control, &prior, fresh).unwrap_err(),
                "REVIEWED_BUILD_BINDING_IMMUTABLE"
            );
            drop(control);
            let _ = fs::remove_dir_all(root);
        }
    }

    #[cfg(windows)]
    #[test]
    fn unclaimed_prepared_supersession_refuses_active_pointer_drift() {
        let (root, control, prior) = unclaimed_prepared_control_fixture();
        let pointer = ReviewedBuildActiveGenerationV1 {
            schema_version: 1,
            source_attempt_id: "e".repeat(32),
            source_attempt_digest: "f".repeat(64),
            active_attempt_id: "1".repeat(32),
            active_attempt_digest: "2".repeat(64),
        };
        write_new_regular_in(
            &control.base_guard,
            ACTIVE_GENERATION_FILE,
            &serialize_control_record(&pointer).unwrap(),
            "prepared supersession pointer drift",
        )
        .unwrap();

        let fresh = fresh_attempt_for_authority(&prior, 'b', 'c', "new-reviewed-record");
        assert_eq!(
            prepare_unclaimed_prepared_supersession(&control, &prior, fresh).unwrap_err(),
            "REVIEWED_BUILD_STATE_UNAVAILABLE"
        );
        drop(control);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    fn terminal_failure_control_fixture() -> (PathBuf, BuildControlRoot, ReviewedBuildAttemptV1) {
        let root = std::env::temp_dir().join(format!("catdesk-reviewed-retry-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let control = control_root(&root).unwrap();
        let attempt = test_attempt();
        let claim = ReviewedBuildClaimV1 {
            schema_version: CLAIM_SCHEMA_VERSION,
            build_attempt_id: attempt.build_attempt_id.clone(),
            owner_id: "a".repeat(32),
            spawn_generation: 1,
            state: "SPAWN_OWNER_RESERVED".into(),
        };
        let owner = ReviewedBuildOwnerProofV1 {
            schema_version: 1,
            build_attempt_id: attempt.build_attempt_id.clone(),
            owner_id: claim.owner_id.clone(),
            spawn_generation: 1,
        };
        let result = ReviewedBuildResultV1 {
            schema_version: RESULT_SCHEMA_VERSION,
            build_attempt_id: attempt.build_attempt_id.clone(),
            owner_id: claim.owner_id.clone(),
            state: "BUILD_FAILED_OR_AMBIGUOUS".into(),
            attestation_digest: None,
            failure_code: Some("SNAPSHOT_CHILD_COLLISION".into()),
            failure_diagnostic: None,
        };
        control_create_json(&control, "attempt.json", &attempt).unwrap();
        control_create_json(&control, "claim.json", &claim).unwrap();
        control_create_json(&control, "worker-owner.json", &owner).unwrap();
        control_create_json(&control, "result.json", &result).unwrap();
        (root, control, attempt)
    }

    #[cfg(windows)]
    #[test]
    fn terminal_failure_retry_publishes_one_fresh_generation_and_preserves_audit() {
        let (root, control, prior) = terminal_failure_control_fixture();
        let audit = terminal_failure_audit(&control, &prior).unwrap();
        let mut fresh = prior.clone();
        fresh.build_attempt_id = "b".repeat(32);
        fresh.confirmation_token = "c".repeat(32);
        fresh.attempt_digest = digest_attempt(&fresh).unwrap();
        let (token, outcome) =
            prepare_terminal_retry(&control, &prior, audit, fresh.clone()).unwrap();
        assert_eq!(outcome.state, "PREPARED");
        assert_eq!(token, fresh.confirmation_token);
        assert_ne!(token, prior.confirmation_token);
        let active = existing_control_root(&root).unwrap();
        let selected: ReviewedBuildAttemptV1 = control_read_json(&active, "attempt.json").unwrap();
        assert_eq!(selected.build_attempt_id, fresh.build_attempt_id);
        assert_ne!(selected.confirmation_token, prior.confirmation_token);
        assert_eq!(
            confirm_reviewed_build(&root, &prior.confirmation_token, |_attempt, _owner| Ok(()))
                .unwrap_err(),
            "REVIEWED_BUILD_CONFIRMATION_UNAVAILABLE"
        );
        let mut history = active.base_guard.try_clone("retry test history").unwrap();
        history
            .descend_existing(RETRY_HISTORY_DIRECTORY, "retry test history")
            .unwrap();
        let stored: ReviewedBuildTerminalAuditV1 = read_control_record(
            &history,
            &retry_child_name(&prior.build_attempt_id).unwrap(),
            "retry test history",
        )
        .unwrap();
        assert_eq!(stored.attempt, prior);
        assert_eq!(stored.result.state, "BUILD_FAILED_OR_AMBIGUOUS");
        drop(active);
        drop(control);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn terminal_failure_retry_replay_converges_and_malformed_or_successful_history_refuses() {
        let (root, control, prior) = terminal_failure_control_fixture();
        let audit = terminal_failure_audit(&control, &prior).unwrap();
        let mut fresh = prior.clone();
        fresh.build_attempt_id = "b".repeat(32);
        fresh.confirmation_token = "c".repeat(32);
        fresh.attempt_digest = digest_attempt(&fresh).unwrap();
        let (first, _) = prepare_terminal_retry(&control, &prior, audit, fresh).unwrap();
        let active = existing_control_root(&root).unwrap();
        let selected: ReviewedBuildAttemptV1 = control_read_json(&active, "attempt.json").unwrap();
        assert_eq!(first, selected.confirmation_token);
        assert!(terminal_failure_audit(&active, &selected).is_err());
        drop(active);
        drop(control);
        let _ = fs::remove_dir_all(root);

        let (root, control, prior) = terminal_failure_control_fixture();
        let mut result: ReviewedBuildResultV1 = control_read_json(&control, "result.json").unwrap();
        result.state = "BUILD_ATTESTED".into();
        result.attestation_digest = Some("f".repeat(64));
        result.failure_code = None;
        assert!(terminal_failure_audit(&control, &prior).is_ok());
        assert!(!retryable_terminal_result(&result));
        // The fixed result record is create-once; this validates the refusal
        // predicate directly rather than attempting a forbidden replacement.
        assert_ne!(result.state, "BUILD_FAILED_OR_AMBIGUOUS");
        drop(control);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn terminal_audit_refuses_missing_owner_drift_and_nonterminal_families() {
        let (root, control, prior) = terminal_failure_control_fixture();
        // The fixture's owner is required even when every other record is
        // exact.  Use an independent empty control root so no immutable test
        // record is replaced.
        let missing_owner_root =
            std::env::temp_dir().join(format!("catdesk-reviewed-missing-owner-{}", Uuid::new_v4()));
        fs::create_dir(&missing_owner_root).unwrap();
        let missing_owner_control = control_root(&missing_owner_root).unwrap();
        let claim = ReviewedBuildClaimV1 {
            schema_version: CLAIM_SCHEMA_VERSION,
            build_attempt_id: prior.build_attempt_id.clone(),
            owner_id: "a".repeat(32),
            spawn_generation: 1,
            state: "SPAWN_OWNER_RESERVED".into(),
        };
        let result = ReviewedBuildResultV1 {
            schema_version: RESULT_SCHEMA_VERSION,
            build_attempt_id: prior.build_attempt_id.clone(),
            owner_id: claim.owner_id.clone(),
            state: "BUILD_FAILED_OR_AMBIGUOUS".into(),
            attestation_digest: None,
            failure_code: Some("FIXTURE_FAILURE".into()),
            failure_diagnostic: None,
        };
        control_create_json(&missing_owner_control, "attempt.json", &prior).unwrap();
        control_create_json(&missing_owner_control, "claim.json", &claim).unwrap();
        control_create_json(&missing_owner_control, "result.json", &result).unwrap();
        assert_eq!(
            terminal_failure_audit(&missing_owner_control, &prior).unwrap_err(),
            "REVIEWED_BUILD_STATE_INVALID"
        );

        let mut drifted = prior.clone();
        drifted.snapshot_manifest_digest = "9".repeat(64);
        // Deliberately retain the original attempt digest: path or field drift
        // must not become a retry authority.
        assert_eq!(
            terminal_failure_audit(&control, &drifted).unwrap_err(),
            "REVIEWED_BUILD_STATE_INVALID"
        );
        let mut attested = result.clone();
        attested.state = "BUILD_ATTESTED".into();
        attested.attestation_digest = Some("f".repeat(64));
        attested.failure_code = None;
        assert!(!retryable_terminal_result(&attested));
        assert_ne!(attested.state, "BUILD_FAILED_OR_AMBIGUOUS");
        drop(missing_owner_control);
        drop(control);
        let _ = fs::remove_dir_all(missing_owner_root);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    fn fresh_attempt_for_authority(
        prior: &ReviewedBuildAttemptV1,
        attempt_id: char,
        token: char,
        review_record_id: &str,
    ) -> ReviewedBuildAttemptV1 {
        let mut fresh = prior.clone();
        fresh.build_attempt_id = attempt_id.to_string().repeat(32);
        fresh.confirmation_token = token.to_string().repeat(32);
        fresh.review_record_id = review_record_id.into();
        fresh.attempt_digest = digest_attempt(&fresh).unwrap();
        fresh
    }

    #[cfg(windows)]
    #[test]
    fn terminal_failure_new_authority_lineage_is_idempotent_and_rejects_competition() {
        let (root, control, prior) = terminal_failure_control_fixture();
        let audit = terminal_failure_audit(&control, &prior).unwrap();
        let fresh = fresh_attempt_for_authority(&prior, 'b', 'c', "new-reviewed-record");
        let (first_token, outcome) =
            prepare_terminal_retry(&control, &prior, audit, fresh.clone()).unwrap();
        assert_eq!(outcome.state, "PREPARED");
        assert_eq!(first_token, fresh.confirmation_token);
        assert_ne!(first_token, prior.confirmation_token);

        // A new PREPARE reconstruction receives different internal UUIDs but
        // the exact same remeasured authority lineage, so it converges on the
        // already-published generation rather than minting another token.
        let replay = fresh_attempt_for_authority(&prior, 'd', 'e', "new-reviewed-record");
        let replay_audit = terminal_failure_audit(&control, &prior).unwrap();
        assert_eq!(
            prepare_terminal_retry(&control, &prior, replay_audit, replay)
                .unwrap()
                .0,
            first_token
        );

        let competing = fresh_attempt_for_authority(&prior, 'f', 'g', "competing-reviewed-record");
        let competing_audit = terminal_failure_audit(&control, &prior).unwrap();
        assert_eq!(
            prepare_terminal_retry(&control, &prior, competing_audit, competing).unwrap_err(),
            "REVIEWED_BUILD_BINDING_IMMUTABLE"
        );
        assert_eq!(
            confirm_reviewed_build(&root, &prior.confirmation_token, |_attempt, _owner| Ok(()))
                .unwrap_err(),
            "REVIEWED_BUILD_CONFIRMATION_UNAVAILABLE"
        );

        let active = existing_control_root(&root).unwrap();
        let selected: ReviewedBuildAttemptV1 = control_read_json(&active, "attempt.json").unwrap();
        assert_eq!(selected.review_record_id, "new-reviewed-record");
        let mut history = active
            .base_guard
            .try_clone("new authority history")
            .unwrap();
        history
            .descend_existing(RETRY_HISTORY_DIRECTORY, "new authority history")
            .unwrap();
        let stored: ReviewedBuildTerminalAuditV1 = read_control_record(
            &history,
            &retry_child_name(&prior.build_attempt_id).unwrap(),
            "new authority history",
        )
        .unwrap();
        assert_eq!(stored, terminal_failure_audit(&control, &prior).unwrap());
        drop(active);
        drop(control);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn terminal_failure_new_authority_plan_recovers_only_exact_durable_publication() {
        let (root, control, prior) = terminal_failure_control_fixture();
        let audit = terminal_failure_audit(&control, &prior).unwrap();
        let audit_bytes = serialize_control_record(&audit).unwrap();
        let audit_digest = sha256(&audit_bytes);
        let fresh = fresh_attempt_for_authority(&prior, 'b', 'c', "new-reviewed-record");
        let prior_name = retry_child_name(&prior.build_attempt_id).unwrap();

        let mut history = control
            .base_guard
            .try_clone("new authority history")
            .unwrap();
        history
            .descend_or_create(RETRY_HISTORY_DIRECTORY, "new authority history")
            .unwrap();
        write_or_validate_new_control_record(
            &history,
            &prior_name,
            &audit_bytes,
            "new authority history",
        )
        .unwrap();
        let mut generations = control
            .base_guard
            .try_clone("new authority generations")
            .unwrap();
        generations
            .descend_or_create(RETRY_GENERATION_DIRECTORY, "new authority generations")
            .unwrap();
        let mut generation = generations.try_clone("new authority generation").unwrap();
        generation
            .create_child(&fresh.build_attempt_id, "new authority generation")
            .unwrap();
        write_or_validate_new_control_record(
            &generation,
            "attempt.json",
            &serialize_control_record(&fresh).unwrap(),
            "new authority generation",
        )
        .unwrap();
        let plan = ReviewedBuildRetryPlanV1 {
            schema_version: 1,
            source_attempt_id: prior.build_attempt_id.clone(),
            source_attempt_digest: prior.attempt_digest.clone(),
            audit_digest,
            fresh_attempt_id: fresh.build_attempt_id.clone(),
            fresh_attempt_digest: fresh.attempt_digest.clone(),
            fresh_confirmation_token: fresh.confirmation_token.clone(),
        };
        let mut plans = control.base_guard.try_clone("new authority plans").unwrap();
        plans
            .descend_or_create(RETRY_PLAN_DIRECTORY, "new authority plans")
            .unwrap();
        write_or_validate_new_control_record(
            &plans,
            &prior_name,
            &serialize_control_record(&plan).unwrap(),
            "new authority plans",
        )
        .unwrap();

        // This is the exact crash boundary after immutable history, inert
        // generation, and plan publication but before active-pointer commit.
        // An exact replay may finish that final atomic pointer publication.
        let replay = fresh_attempt_for_authority(&prior, 'd', 'e', "new-reviewed-record");
        let replay_audit = terminal_failure_audit(&control, &prior).unwrap();
        assert_eq!(
            prepare_terminal_retry(&control, &prior, replay_audit, replay)
                .unwrap()
                .0,
            fresh.confirmation_token
        );
        let active = existing_control_root(&root).unwrap();
        let selected: ReviewedBuildAttemptV1 = control_read_json(&active, "attempt.json").unwrap();
        assert_eq!(selected.build_attempt_id, fresh.build_attempt_id);
        drop(active);
        drop(control);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    fn end_to_end_snapshot_fixture() -> (
        PathBuf,
        ReviewedSourceSnapshotExpectedV1,
        ValidatedReviewedSourceSnapshotV1,
    ) {
        let root = std::env::temp_dir().join(format!("catdesk-r9-attest-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            b"[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        fs::write(root.join("Cargo.lock"), b"# fixture\nversion = 3\n").unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), b"pub fn fixture() {}\n").unwrap();
        fs::create_dir_all(root.join("wake/src")).unwrap();
        fs::write(
            root.join("wake/Cargo.toml"),
            b"[package]\nname='catdesk-wake'\nversion='1.0.0'\nedition='2024'\n",
        )
        .unwrap();
        fs::write(root.join("wake/src/lib.rs"), b"pub const WAKE: u8 = 1;\n").unwrap();
        fs::write(root.join("src/out.txt"), b"fixture-output\n").unwrap();
        let expected = ReviewedSourceSnapshotExpectedV1 {
            session_id: "fixture-session".into(),
            project_id: "catdesk".into(),
            approved_contract_hash: "fixture-contract".into(),
            logical_task_id: "fixture-task".into(),
            completion_artifact_ids: vec!["src/out.txt".into()],
            current_outputs: vec![ReviewedSourceCurrentOutputV1 {
                artifact_id: "src/out.txt".into(),
                sha256: sha256(b"fixture-output\n"),
            }],
            baseline_observations: vec![ReviewedSourceBaselineObservationV1 {
                artifact_id: "src/out.txt".into(),
                state: "ABSENT".into(),
                sha256: None,
            }],
        };
        let snapshot = create_or_validate_reviewed_source_snapshot(&root, &expected).unwrap();
        (root, expected, snapshot)
    }

    #[cfg(windows)]
    #[test]
    fn materialize_snapshot_reopens_repeated_scripts_parent() {
        let root =
            std::env::temp_dir().join(format!("catdesk-materialize-scripts-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let bytes_root = root.join("snapshot-bytes");
        fs::create_dir(&bytes_root).unwrap();
        let scripts = bytes_root.join("scripts");
        fs::create_dir(&scripts).unwrap();
        let first = b"first-script\n";
        let second = b"second-script\n";
        fs::write(scripts.join("first.ps1"), first).unwrap();
        fs::write(scripts.join("second.ps1"), second).unwrap();
        let snapshot = ValidatedReviewedSourceSnapshotV1 {
            snapshot_id: "a".repeat(64),
            authority_digest: "b".repeat(64),
            manifest_digest: "c".repeat(64),
            entries: vec![
                ReviewedSourceEntryV1 {
                    relative_path: "scripts/first.ps1".into(),
                    byte_length: first.len() as u64,
                    sha256: sha256(first),
                    content_object: "first-object".into(),
                },
                ReviewedSourceEntryV1 {
                    relative_path: "scripts/second.ps1".into(),
                    byte_length: second.len() as u64,
                    sha256: sha256(second),
                    content_object: "second-object".into(),
                },
            ],
            bytes_root,
        };
        let control = control_root(&root).unwrap();
        let attempt = "d".repeat(32);
        let source = materialize_snapshot(&snapshot, &control, &attempt).unwrap();
        assert_eq!(
            source,
            control
                .guard
                .path()
                .join("builds")
                .join(&attempt)
                .join("source")
        );
        assert_eq!(fs::read(source.join("scripts/first.ps1")).unwrap(), first);
        assert_eq!(fs::read(source.join("scripts/second.ps1")).unwrap(), second);
        drop(control);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn active_generation_target_layout_uses_short_attempt_bound_workspace_root() {
        let root = std::env::temp_dir().join(format!(
            "catdesk-reviewed-build-short-target-{}",
            Uuid::new_v4()
        ));
        let control_path = root.join(".catdesk/reviewed-build-control");
        let attempt = "d".repeat(32);
        fs::create_dir_all(control_path.join("generations").join(&attempt)).unwrap();

        let base_guard =
            ProtectedDirectoryGuard::acquire(&control_path, "reviewed build test base").unwrap();
        let mut guard = base_guard
            .try_clone("reviewed build test generation")
            .unwrap();
        guard
            .descend_existing("generations", "reviewed build test generation")
            .unwrap();
        guard
            .descend_existing(&attempt, "reviewed build test generation")
            .unwrap();
        let control = BuildControlRoot { guard, base_guard };

        let target = build_target_guard(&root, &control, &attempt).unwrap();
        assert_eq!(
            target.path(),
            root.join("target-verify")
                .join("rb")
                .join(&attempt)
                .join("target")
        );
        assert!(!target.path().starts_with(control.guard.path()));
        assert!(
            !target
                .path()
                .join("release")
                .to_string_lossy()
                .contains("reviewed-build-control")
        );

        drop(target);
        drop(control);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    fn end_to_end_attestation_fixture() -> Result<
        (
            PathBuf,
            ReviewedSourceSnapshotExpectedV1,
            ReviewedBuildAttemptV1,
            ReviewedBuildAttestationV2,
            OpenRegularEvidence,
        ),
        String,
    > {
        // The ordinary replay test is valid only when the production trusted
        // toolchain policy can resolve the OS profile. Resolve that host
        // prerequisite before creating temp authority state so a generic
        // provider profile reports its bounded environment result without
        // leaving a partial fixture behind.
        let (cargo, rustc) = trusted_toolchain()?;
        let (root, expected, snapshot) = end_to_end_snapshot_fixture();
        let mut attempt = ReviewedBuildAttemptV1 {
            schema_version: ATTEMPT_SCHEMA_VERSION,
            build_attempt_id: "f".repeat(32),
            confirmation_token: "1".repeat(32),
            review_session_id: expected.session_id.clone(),
            review_record_id: "fixture-record".into(),
            review_authority_sha256: "2".repeat(64),
            snapshot_expected: expected.clone(),
            snapshot_id: snapshot.snapshot_id,
            snapshot_authority_digest: snapshot.authority_digest,
            snapshot_manifest_digest: snapshot.manifest_digest,
            cargo,
            rustc,
            policy: fixed_policy(),
            attempt_digest: String::new(),
        };
        attempt.attempt_digest = digest_attempt(&attempt).unwrap();
        let parent = candidate_parent_for_create(&root, &attempt.build_attempt_id).unwrap();
        let mut candidate = create_candidate_file(&parent).unwrap();
        candidate.write_all(b"legitimate-candidate").unwrap();
        candidate.sync_all().unwrap();
        let evidence = evidence_from_open_regular(&mut candidate, "fixture candidate").unwrap();
        drop(candidate);
        let mut attestation = ReviewedBuildAttestationV2 {
            schema_version: ATTESTATION_SCHEMA_VERSION,
            producer: "CATDESK_REVIEWED_BUILD_WORKER_V2".into(),
            build_attempt_id: attempt.build_attempt_id.clone(),
            review_session_id: attempt.review_session_id.clone(),
            review_record_id: attempt.review_record_id.clone(),
            review_authority_sha256: attempt.review_authority_sha256.clone(),
            snapshot_id: attempt.snapshot_id.clone(),
            snapshot_authority_digest: attempt.snapshot_authority_digest.clone(),
            snapshot_manifest_digest: attempt.snapshot_manifest_digest.clone(),
            build_policy_sha256: attempt.policy.policy_sha256.clone(),
            build_argv_sha256: attempt.policy.argv_sha256.clone(),
            environment_policy_sha256: attempt.policy.environment_policy_sha256.clone(),
            cargo_path: attempt.cargo.absolute_path.clone(),
            cargo_sha256: attempt.cargo.sha256.clone(),
            cargo_identity: attempt.cargo.identity.clone(),
            cargo_version_sha256: attempt.cargo.version_sha256.clone(),
            rustc_path: attempt.rustc.absolute_path.clone(),
            rustc_sha256: attempt.rustc.sha256.clone(),
            rustc_identity: attempt.rustc.identity.clone(),
            rustc_version_sha256: attempt.rustc.version_sha256.clone(),
            candidate_relative_path: format!(
                "target/reviewed-builds/{}/catdesk.exe",
                attempt.build_attempt_id
            ),
            candidate_sha256: evidence.sha256.clone(),
            candidate_length: evidence.length,
            candidate_identity: evidence.identity.clone(),
            generation: 1,
            created_at_unix: 1,
            attestation_digest: String::new(),
        };
        attestation.attestation_digest = digest_attestation(&attestation).unwrap();
        let control = control_root(&root).unwrap();
        let claim = ReviewedBuildClaimV1 {
            schema_version: CLAIM_SCHEMA_VERSION,
            build_attempt_id: attempt.build_attempt_id.clone(),
            owner_id: "3".repeat(32),
            spawn_generation: 1,
            state: "SPAWN_OWNER_RESERVED".into(),
        };
        let owner = ReviewedBuildOwnerProofV1 {
            schema_version: 1,
            build_attempt_id: attempt.build_attempt_id.clone(),
            owner_id: claim.owner_id.clone(),
            spawn_generation: 1,
        };
        let result = ReviewedBuildResultV1 {
            schema_version: RESULT_SCHEMA_VERSION,
            build_attempt_id: attempt.build_attempt_id.clone(),
            owner_id: claim.owner_id.clone(),
            state: "BUILD_ATTESTED".into(),
            attestation_digest: Some(attestation.attestation_digest.clone()),
            failure_code: None,
            failure_diagnostic: None,
        };
        control_create_json(&control, "attempt.json", &attempt).unwrap();
        control_create_json(&control, "claim.json", &claim).unwrap();
        control_create_json(&control, "worker-owner.json", &owner).unwrap();
        control_create_json(&control, "attestation.json", &attestation).unwrap();
        control_create_json(&control, "result.json", &result).unwrap();
        Ok((root, expected, attempt, attestation, evidence))
    }

    #[cfg(windows)]
    #[test]
    fn end_to_end_fixture_snapshot_uses_real_r7c_authority() {
        let (root, expected, created) = end_to_end_snapshot_fixture();
        let replayed = validate_committed_snapshot(&root, &expected).unwrap();
        assert_eq!(expected.project_id, "catdesk");
        assert_eq!(created.snapshot_id, replayed.snapshot_id);
        assert_eq!(created.authority_digest, replayed.authority_digest);
        assert_eq!(created.manifest_digest, replayed.manifest_digest);
        assert!(!replayed.entries.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn replay_preopen_producer_attestation_rejects_same_length_swap() {
        use std::sync::Arc;

        let _serial = output_candidate_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (root, expected, attempt, attestation, legitimate) =
            match end_to_end_attestation_fixture() {
                Ok(fixture) => fixture,
                Err(error) if error == "REVIEWED_BUILD_STATE_UNAVAILABLE" => {
                    eprintln!(
                        "replay fixture environment unavailable: trusted host toolchain profile is absent"
                    );
                    return;
                }
                Err(error) => panic!("end-to-end attestation fixture failed: {error}"),
            };
        assert_eq!(legitimate.length, 20, "fixture candidate length");
        let validate = || {
            validate_producer_attestation(
                &root,
                &expected.session_id,
                &attempt.review_record_id,
                &attempt.review_authority_sha256,
                &expected,
                &attestation.candidate_relative_path,
                &attestation.candidate_sha256,
            )
        };
        assert!(
            validate().is_ok(),
            "positive production validation must succeed"
        );
        let outside =
            std::env::temp_dir().join(format!("catdesk-r9-replay-outside-{}", Uuid::new_v4()));
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"outside").unwrap();
        let reached_pre = Arc::new(Mutex::new(false));
        let reached_evidence = Arc::new(Mutex::new(false));
        let substitution_performed = Arc::new(Mutex::new(false));
        let pre = Arc::clone(&reached_pre);
        let evidence = Arc::clone(&reached_evidence);
        let performed = Arc::clone(&substitution_performed);
        replace_output_candidate_hook(Some(Box::new(move |boundary, parent| {
            if boundary == "candidate-replay-pre-open" {
                *pre.lock().unwrap() = true;
                let original = parent.join("catdesk.exe");
                if fs::rename(&original, parent.join("catdesk.legitimate")).is_ok() {
                    // Same byte length as the legitimate fixture, different SHA/object identity.
                    if fs::write(&original, b"attacker---candidate").is_ok() {
                        *performed.lock().unwrap() = true;
                    }
                }
            }
            if boundary == "candidate-evidence" {
                *evidence.lock().unwrap() = true;
            }
        })));
        let error = validate().unwrap_err();
        replace_output_candidate_hook(None);
        assert_eq!(error, "REVIEWED_BUILD_ATTESTATION_UNAVAILABLE");
        assert!(*reached_pre.lock().unwrap());
        assert!(
            *substitution_performed.lock().unwrap(),
            "the actual same-name replacement must complete before replay open"
        );
        assert!(*reached_evidence.lock().unwrap());
        let (_parent, mut attacker) =
            open_candidate_for_replay(&root, &attempt.build_attempt_id).unwrap();
        let attacker_evidence =
            evidence_from_open_regular(&mut attacker, "attacker fixture").unwrap();
        assert_eq!(attacker_evidence.length, legitimate.length);
        assert_eq!(attacker_evidence.length, 20, "same-length attacker payload");
        assert_ne!(attacker_evidence.sha256, legitimate.sha256);
        assert_ne!(attacker_evidence.identity, legitimate.identity);
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside");
    }
    #[test]
    fn fixed_policy_is_closed_and_deterministic() {
        let policy = fixed_policy();
        assert_eq!(
            policy.argv,
            vec!["build", "--release", "--locked", "--offline"]
        );
        assert!(valid_sha256(&policy.policy_sha256));
        assert!(valid_sha256(&policy.environment_policy_sha256));
        assert!(policy.version.contains("OFFLINE_VALIDATED_CARGO_CACHE"));
        assert_eq!(policy.environment_policy_sha256, sha256(ENVIRONMENT_POLICY));
        assert!(ENVIRONMENT_POLICY.contains("os-system-drive"));
        assert!(ENVIRONMENT_POLICY.contains("per-attempt-temp"));
        assert!(known_historical_build_policy(&policy));
    }

    #[test]
    fn historical_policy_acceptance_is_closed_to_exact_v3_v4_or_current_v5() {
        let legacy_argv = LEGACY_BUILD_ARGUMENTS
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();
        let mut legacy = ReviewedBuildPolicyV1 {
            version: LEGACY_BUILD_POLICY_V3_VERSION.into(),
            policy_sha256: sha256(LEGACY_BUILD_POLICY_V3_VERSION),
            argv_sha256: sha256(canonical_json(&legacy_argv).as_bytes()),
            argv: legacy_argv.clone(),
            environment_policy_sha256: LEGACY_BUILD_ENVIRONMENT_POLICY_V3_SHA256.into(),
        };
        assert!(known_historical_build_policy(&legacy));

        legacy.version = LEGACY_BUILD_POLICY_V4_VERSION.into();
        legacy.policy_sha256 = sha256(LEGACY_BUILD_POLICY_V4_VERSION);
        legacy.environment_policy_sha256 = LEGACY_BUILD_ENVIRONMENT_POLICY_V4_SHA256.into();
        assert!(known_historical_build_policy(&legacy));

        legacy.environment_policy_sha256 = "0".repeat(64);
        assert!(!known_historical_build_policy(&legacy));

        legacy.environment_policy_sha256 = LEGACY_BUILD_ENVIRONMENT_POLICY_V4_SHA256.into();
        legacy.version = "CATDESK_REVIEWED_BUILD_POLICY_UNKNOWN".into();
        legacy.policy_sha256 = sha256(legacy.version.as_bytes());
        assert!(!known_historical_build_policy(&legacy));

        let mut legacy_v5 = fixed_policy();
        legacy_v5.environment_policy_sha256 =
            LEGACY_BUILD_ENVIRONMENT_POLICY_V5_PRE_FIXED_ROOT_LINKER_SHA256.into();
        assert!(known_historical_build_policy(&legacy_v5));

        legacy_v5.environment_policy_sha256 = "0".repeat(64);
        assert!(!known_historical_build_policy(&legacy_v5));

        let mut tampered_current = fixed_policy();
        tampered_current.argv.pop();
        assert!(!known_historical_build_policy(&tampered_current));
    }

    #[cfg(windows)]
    fn offline_registry_fixture() -> (PathBuf, LockedRegistryCrate, Vec<u8>, String) {
        let root = std::env::temp_dir().join(format!("catdesk-offline-cache-{}", Uuid::new_v4()));
        let registry_id = "index.crates.io-fixture".to_string();
        let package = LockedRegistryCrate {
            name: "foo".into(),
            version: "1.2.3".into(),
            checksum: sha256(b"fixture crate archive"),
        };
        let archive = b"fixture crate archive".to_vec();
        let index_path = registry_index_path(&package).unwrap();
        let index_parent = root
            .join("source/.cargo/registry/index")
            .join(&registry_id)
            .join(&index_path[0])
            .join(&index_path[1])
            .join(&index_path[2]);
        fs::create_dir_all(&index_parent).unwrap();
        fs::write(
            root.join("source/.cargo/registry/index")
                .join(&registry_id)
                .join("config.json"),
            b"{\"dl\":\"https://static.crates.io/crates\"}",
        )
        .unwrap();
        fs::write(index_parent.join(&index_path[3]), b"index-entry").unwrap();
        let cache = root.join("source/.cargo/registry/cache").join(&registry_id);
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("foo-1.2.3.crate"), &archive).unwrap();
        (root, package, archive, registry_id)
    }

    #[cfg(windows)]
    #[test]
    fn offline_cache_seeding_copies_only_checked_archive_and_index_entry() {
        let (root, package, archive, registry_id) = offline_registry_fixture();
        let source_home = root.join("source/.cargo/registry");
        let mut source_index =
            ProtectedDirectoryGuard::acquire(&source_home, "test source registry").unwrap();
        source_index
            .descend_existing("index", "test source index")
            .unwrap();
        source_index
            .descend_existing(&registry_id, "test source index")
            .unwrap();
        let mut source_cache =
            ProtectedDirectoryGuard::acquire(&source_home, "test source registry").unwrap();
        source_cache
            .descend_existing("cache", "test source cache")
            .unwrap();
        source_cache
            .descend_existing(&registry_id, "test source cache")
            .unwrap();
        let destination_root = root.join("destination");
        fs::create_dir(&destination_root).unwrap();
        let mut destination_index =
            ProtectedDirectoryGuard::acquire(&destination_root, "test destination").unwrap();
        destination_index
            .create_child("index", "test destination")
            .unwrap();
        destination_index
            .create_child(&registry_id, "test destination")
            .unwrap();
        let mut destination_cache =
            ProtectedDirectoryGuard::acquire(&destination_root, "test destination").unwrap();
        destination_cache
            .create_child("cache", "test destination")
            .unwrap();
        destination_cache
            .create_child(&registry_id, "test destination")
            .unwrap();
        copy_locked_registry_index_entry(&source_index, &mut destination_index, &package).unwrap();
        copy_locked_registry_archive(&source_cache, &destination_cache, &package).unwrap();
        let path = registry_index_path(&package).unwrap();
        assert_eq!(
            fs::read(
                destination_root
                    .join("index")
                    .join(&registry_id)
                    .join(path.join("\\"))
            )
            .unwrap(),
            b"index-entry"
        );
        assert_eq!(
            fs::read(
                destination_root
                    .join("cache")
                    .join(&registry_id)
                    .join("foo-1.2.3.crate")
            )
            .unwrap(),
            archive
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn offline_cache_seed_accepts_observed_large_sparse_index_entry_within_bound() {
        const OBSERVED_WEB_SYS_INDEX_BYTES: usize = 4_378_891;
        assert!(OBSERVED_WEB_SYS_INDEX_BYTES as u64 > 4 * 1024 * 1024);
        assert!(OBSERVED_WEB_SYS_INDEX_BYTES as u64 <= MAX_CARGO_REGISTRY_INDEX_ENTRY_BYTES);

        let (root, package, _archive, registry_id) = offline_registry_fixture();
        let index_path = registry_index_path(&package).unwrap();
        let source_index_file = root
            .join("source/.cargo/registry/index")
            .join(&registry_id)
            .join(index_path.join("\\"));
        fs::write(&source_index_file, vec![b'x'; OBSERVED_WEB_SYS_INDEX_BYTES]).unwrap();

        let source_home = root.join("source/.cargo/registry");
        let mut source_index =
            ProtectedDirectoryGuard::acquire(&source_home, "test source registry").unwrap();
        source_index
            .descend_existing("index", "test source index")
            .unwrap();
        source_index
            .descend_existing(&registry_id, "test source index")
            .unwrap();

        let destination_root = root.join("destination-large-index");
        fs::create_dir(&destination_root).unwrap();
        let mut destination_index =
            ProtectedDirectoryGuard::acquire(&destination_root, "test destination").unwrap();
        destination_index
            .create_child("index", "test destination")
            .unwrap();
        destination_index
            .create_child(&registry_id, "test destination")
            .unwrap();

        copy_locked_registry_index_entry(&source_index, &mut destination_index, &package)
            .expect("bounded large sparse-index entry");
        let copied = fs::metadata(
            destination_root
                .join("index")
                .join(&registry_id)
                .join(index_path.join("\\")),
        )
        .unwrap()
        .len();
        assert_eq!(copied, OBSERVED_WEB_SYS_INDEX_BYTES as u64);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn offline_cache_seed_copies_one_index_leaf_for_multiple_locked_versions() {
        let (root, first, first_archive, registry_id) = offline_registry_fixture();
        let second_archive = b"fixture crate archive v2".to_vec();
        let second = LockedRegistryCrate {
            name: first.name.clone(),
            version: "2.0.0".into(),
            checksum: sha256(&second_archive),
        };
        fs::write(
            root.join("source/.cargo/registry/cache")
                .join(&registry_id)
                .join("foo-2.0.0.crate"),
            &second_archive,
        )
        .unwrap();

        let source_home = root.join("source/.cargo/registry");
        let mut source_index =
            ProtectedDirectoryGuard::acquire(&source_home, "test source registry").unwrap();
        source_index
            .descend_existing("index", "test source index")
            .unwrap();
        source_index
            .descend_existing(&registry_id, "test source index")
            .unwrap();
        let mut source_cache =
            ProtectedDirectoryGuard::acquire(&source_home, "test source registry").unwrap();
        source_cache
            .descend_existing("cache", "test source cache")
            .unwrap();
        source_cache
            .descend_existing(&registry_id, "test source cache")
            .unwrap();

        let destination_root = root.join("destination-multiversion");
        fs::create_dir(&destination_root).unwrap();
        let mut destination_index =
            ProtectedDirectoryGuard::acquire(&destination_root, "test destination").unwrap();
        destination_index
            .create_child("index", "test destination")
            .unwrap();
        destination_index
            .create_child(&registry_id, "test destination")
            .unwrap();
        let mut destination_cache =
            ProtectedDirectoryGuard::acquire(&destination_root, "test destination").unwrap();
        destination_cache
            .create_child("cache", "test destination")
            .unwrap();
        destination_cache
            .create_child(&registry_id, "test destination")
            .unwrap();

        copy_locked_registry_closure(
            &source_index,
            &source_cache,
            &mut destination_index,
            &destination_cache,
            &[first.clone(), second.clone()],
        )
        .expect("multi-version cache closure");

        let index_path = registry_index_path(&first).unwrap();
        assert_eq!(
            fs::read(
                destination_root
                    .join("index")
                    .join(&registry_id)
                    .join(index_path.join("\\"))
            )
            .unwrap(),
            b"index-entry"
        );
        assert_eq!(
            fs::read(
                destination_root
                    .join("cache")
                    .join(&registry_id)
                    .join("foo-1.2.3.crate")
            )
            .unwrap(),
            first_archive
        );
        assert_eq!(
            fs::read(
                destination_root
                    .join("cache")
                    .join(&registry_id)
                    .join("foo-2.0.0.crate")
            )
            .unwrap(),
            second_archive
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn offline_cache_seed_refuses_missing_or_wrong_checksum_archive() {
        let (root, mut package, _archive, registry_id) = offline_registry_fixture();
        let cache_root = root.join("source/.cargo/registry");
        let mut source_cache =
            ProtectedDirectoryGuard::acquire(&cache_root, "test source registry").unwrap();
        source_cache
            .descend_existing("cache", "test source cache")
            .unwrap();
        source_cache
            .descend_existing(&registry_id, "test source cache")
            .unwrap();
        let destination = root.join("destination");
        fs::create_dir(&destination).unwrap();
        let destination =
            ProtectedDirectoryGuard::acquire(&destination, "test destination").unwrap();
        package.checksum = "0".repeat(64);
        assert!(copy_locked_registry_archive(&source_cache, &destination, &package).is_err());
        package.name = "missing".into();
        assert!(copy_locked_registry_archive(&source_cache, &destination, &package).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn offline_cache_seed_refuses_non_directory_index_component() {
        let (root, package, _archive, registry_id) = offline_registry_fixture();
        let index_root = root.join("source/.cargo/registry/index").join(&registry_id);
        fs::remove_dir_all(index_root.join(".cache")).unwrap();
        fs::write(index_root.join(".cache"), b"not a directory").unwrap();
        let mut source_index = ProtectedDirectoryGuard::acquire(
            &root.join("source/.cargo/registry/index"),
            "test source index",
        )
        .unwrap();
        source_index
            .descend_existing(&registry_id, "test source index")
            .unwrap();
        let destination_root = root.join("destination");
        fs::create_dir(&destination_root).unwrap();
        let mut destination =
            ProtectedDirectoryGuard::acquire(&destination_root, "test destination").unwrap();
        assert!(
            copy_locked_registry_index_entry(&source_index, &mut destination, &package).is_err()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn reviewed_toolchain_version_selection_is_numeric_and_requires_complete_layout() {
        let root = std::env::temp_dir().join(format!("catdesk-v5-toolchain-{}", Uuid::new_v4()));
        for version in ["14.9.1", "14.10.2"] {
            for child in ["bin/Hostx64/x64", "lib/x64", "include"] {
                fs::create_dir_all(root.join(version).join(child)).unwrap();
            }
        }
        fs::create_dir_all(root.join("99.0.0/bin/Hostx64/x64")).unwrap();
        fs::create_dir_all(root.join("99.0.0/include")).unwrap();
        fs::create_dir_all(root.join("not-a-version/bin/Hostx64/x64")).unwrap();
        fs::create_dir_all(root.join("not-a-version/lib/x64")).unwrap();
        fs::create_dir_all(root.join("not-a-version/include")).unwrap();

        let selected =
            highest_reviewed_version_directory(&root, &["bin/Hostx64/x64", "lib/x64", "include"])
                .unwrap();
        assert_eq!(
            selected.file_name().and_then(|value| value.to_str()),
            Some("14.10.2")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn offline_worker_policy_never_inherits_ambient_cargo_home() {
        let source = include_str!("reviewed_build.rs");
        let worker_start = source
            .find("pub fn run_reviewed_build_worker")
            .expect("reviewed build worker");
        let command_start = source
            .find("fn apply_exact_worker_cargo_command(")
            .expect("closed command helper");
        let command_end = command_start
            + source[command_start..]
                .find("/// Resolve the fixed-root Windows toolchain")
                .expect("closed command helper end");
        let worker = &source[worker_start..command_start];
        let command_body = &source[command_start..command_end];
        assert!(
            fixed_policy()
                .argv
                .iter()
                .any(|argument| argument == "--offline")
        );
        assert!(worker.contains("seed_isolated_cargo_home(&control, &source)?"));
        assert!(worker.contains("configure_exact_worker_cargo_command("));
        assert!(worker.contains("&cargo_home,"));
        assert!(command_body.contains(".env_clear()"));
        assert!(command_body.contains(".env(\"CARGO_HOME\", cargo_home)"));
        assert!(!worker.contains(".env(\"CARGO_HOME\", profile.join"));
        assert!(!command_body.contains(".env(\"CARGO_HOME\", profile.join"));

        #[cfg(windows)]
        {
            let attempt = test_attempt();
            let cargo_home = Path::new(r"C:\isolated\cargo-home");
            let mut command = Command::new("not-executed-cargo.exe");
            command
                .env("CARGO_HOME", r"C:\ambient\cargo-home")
                .env("CATDESK_TEST_AMBIENT", "must-be-cleared");
            let toolchain = test_windows_toolchain_environment();
            apply_exact_worker_cargo_command(
                &mut command,
                &attempt,
                Path::new(r"C:\isolated\source"),
                Path::new(r"C:\isolated\target"),
                Path::new(r"C:\isolated\tmp"),
                cargo_home,
                r"C:\trusted\rustc.exe",
                &toolchain,
            )
            .expect("closed worker command");
            let environment = command
                .get_envs()
                .collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(
                environment.get(std::ffi::OsStr::new("CARGO_HOME")),
                Some(&Some(cargo_home.as_os_str()))
            );
            assert!(!environment.contains_key(std::ffi::OsStr::new("CATDESK_TEST_AMBIENT")));
        }
    }

    #[cfg(windows)]
    #[ignore = "the current sandbox token profile has no profile-local Rustup installation"]
    #[test]
    fn host_trusted_toolchain_resolves_without_path_authority() {
        let profile = os_current_user_profile().expect("OS current-user profile");
        validate_profile_directory(&profile).expect("safe OS profile chain");
        let rustup = profile.join(".cargo").join("bin").join("rustup.exe");
        trusted_tool_from_absolute(&rustup, "rustup").expect("attested profile-local rustup");
        let (cargo, rustc) = trusted_toolchain().expect("host trusted toolchain");
        for tool in [&cargo, &rustc] {
            let path = Path::new(&tool.absolute_path);
            assert!(path.is_absolute());
            assert_ne!(
                path.parent()
                    .and_then(Path::parent)
                    .and_then(Path::file_name)
                    .and_then(|value| value.to_str()),
                Some(".cargo"),
                "Rustup proxy/shim is never the attested build executable"
            );
            assert!(valid_sha256(&tool.sha256));
            assert!(valid_sha256(&tool.version_sha256));
            assert!(!tool.identity.is_empty());
            assert!(tool.length > 0);
        }
        assert_ne!(cargo.absolute_path, rustc.absolute_path);
    }

    #[test]
    fn toolchain_authority_never_uses_path_or_shell_lookup() {
        let source = include_str!("reviewed_build.rs");
        let path_lookup = ["whe", "re.exe"].concat();
        for forbidden in [
            "Command::new(\"cargo\")",
            "Command::new(\"rustc\")",
            "Command::new(\"rustup\")",
            "Command::new(\"cmd\")",
            "Command::new(\"powershell\")",
        ]
        .into_iter()
        .chain(std::iter::once(path_lookup.as_str()))
        {
            assert!(
                !source.contains(forbidden),
                "forbidden tool lookup: {forbidden}"
            );
        }
        assert!(source.contains("SHGetKnownFolderPath"));
        assert!(source.contains(".env_clear()"));
        assert!(source.contains(r#".env("SystemDrive", os_system_drive()?)"#));
        assert!(source.contains(r#".env("TEMP", &temp)"#));
        assert!(source.contains(r#".env("TMP", &temp)"#));
        for derived_toolchain_variable in ["LIB", "LIBPATH", "INCLUDE"] {
            assert!(
                source.contains(&format!(
                    r#".env("{derived_toolchain_variable}", &toolchain."#
                )),
                "reviewed build must set product-derived {derived_toolchain_variable}"
            );
        }
        for ambient_toolchain_variable in
            ["LIB", "LIBPATH", "INCLUDE", "VCINSTALLDIR", "VSINSTALLDIR"]
        {
            assert!(!source.contains(&format!(r#"std::env::var("{ambient_toolchain_variable}")"#)));
            assert!(!source.contains(&format!(
                r#"std::env::var_os("{ambient_toolchain_variable}")"#
            )));
        }
    }
    #[test]
    fn worker_args_reject_extra_or_caller_selected_build_fields() {
        assert!(
            parse_reviewed_build_worker_args(&[
                REVIEWED_BUILD_WORKER_FLAG.into(),
                "--workspace".into(),
                "x".into(),
                "--attempt".into(),
                "a".repeat(32),
                "--owner".into(),
                "b".repeat(32)
            ])
            .is_ok()
        );
        assert!(
            parse_reviewed_build_worker_args(&[
                REVIEWED_BUILD_WORKER_FLAG.into(),
                "--workspace".into(),
                "x".into(),
                "--attempt".into(),
                "a".repeat(32),
                "--owner".into(),
                "b".repeat(32),
                "--rustc".into()
            ])
            .is_err()
        );
    }
    #[test]
    fn candidate_identity_rejects_path_escape_and_non_ascii() {
        assert!(safe_candidate_relative_path(
            "target/reviewed-builds/a/catdesk.exe"
        ));
        assert!(!safe_candidate_relative_path("../catdesk.exe"));
        assert!(!safe_candidate_relative_path("target\\catdesk.exe"));
        assert!(!safe_candidate_relative_path("target/é.exe"));
    }

    #[test]
    fn immutable_attempt_digest_detects_review_snapshot_or_policy_drift() {
        let attempt = test_attempt();
        assert_eq!(digest_attempt(&attempt).unwrap(), attempt.attempt_digest);
        let mut review_drift = attempt.clone();
        review_drift.review_record_id = "other-record".into();
        assert_ne!(
            digest_attempt(&review_drift).unwrap(),
            attempt.attempt_digest
        );
        let mut snapshot_drift = attempt.clone();
        snapshot_drift.snapshot_id = "other-snapshot".into();
        assert_ne!(
            digest_attempt(&snapshot_drift).unwrap(),
            attempt.attempt_digest
        );
        let mut policy_drift = attempt.clone();
        policy_drift.policy.argv.push("--offline".into());
        assert_ne!(
            digest_attempt(&policy_drift).unwrap(),
            attempt.attempt_digest
        );
    }

    #[test]
    fn producer_attestation_rejects_forgery_candidate_or_toolchain_substitution() {
        let attempt = test_attempt();
        let attestation = test_attestation(&attempt);
        assert!(
            validate_attestation(
                &attestation,
                &attempt,
                &attestation.candidate_relative_path,
                &attestation.candidate_sha256
            )
            .is_ok()
        );
        let mut forged = attestation.clone();
        forged.candidate_sha256 = "2".repeat(64);
        assert!(
            validate_attestation(
                &forged,
                &attempt,
                &forged.candidate_relative_path,
                &forged.candidate_sha256
            )
            .is_err()
        );
        let mut substituted = attestation.clone();
        substituted.rustc_sha256 = "3".repeat(64);
        assert!(
            validate_attestation(
                &substituted,
                &attempt,
                &substituted.candidate_relative_path,
                &substituted.candidate_sha256
            )
            .is_err()
        );
        let mut legacy = attestation;
        legacy.schema_version = 2;
        assert!(
            validate_attestation(
                &legacy,
                &attempt,
                &legacy.candidate_relative_path,
                &legacy.candidate_sha256
            )
            .is_err()
        );
    }

    #[test]
    fn claim_and_terminal_states_are_bound_to_one_attempt_and_owner() {
        let attempt = test_attempt();
        let claim = ReviewedBuildClaimV1 {
            schema_version: CLAIM_SCHEMA_VERSION,
            build_attempt_id: attempt.build_attempt_id.clone(),
            owner_id: "a".repeat(32),
            spawn_generation: 1,
            state: "SPAWN_OWNER_RESERVED".into(),
        };
        assert!(valid_claim(&claim, &attempt));
        let mut changed_owner = claim.clone();
        changed_owner.owner_id = "short".into();
        assert!(!valid_claim(&changed_owner, &attempt));
        let mut second_attempt = claim;
        second_attempt.build_attempt_id = "b".repeat(32);
        assert!(!valid_claim(&second_attempt, &attempt));
    }

    #[test]
    fn reserved_spawn_owner_requires_exact_helper_proof_before_worker_state() {
        let attempt = test_attempt();
        let reserved = ReviewedBuildClaimV1 {
            schema_version: CLAIM_SCHEMA_VERSION,
            build_attempt_id: attempt.build_attempt_id.clone(),
            owner_id: "a".repeat(32),
            spawn_generation: 1,
            state: "SPAWN_OWNER_RESERVED".into(),
        };
        assert!(valid_claim(&reserved, &attempt));
        let proof = ReviewedBuildOwnerProofV1 {
            schema_version: 1,
            build_attempt_id: attempt.build_attempt_id.clone(),
            owner_id: reserved.owner_id.clone(),
            spawn_generation: 1,
        };
        assert!(valid_owner_proof(&proof, &reserved));
        let mut duplicate = proof.clone();
        duplicate.owner_id = "b".repeat(32);
        assert!(!valid_owner_proof(&duplicate, &reserved));
        let mut stale = reserved;
        stale.spawn_generation = 2;
        assert!(!valid_owner_proof(&proof, &stale));
    }

    #[test]
    fn protected_control_root_rejects_unsafe_catdesk_intermediate() {
        let root = std::env::temp_dir().join(format!("catdesk-build-control-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(".catdesk"), b"not-a-directory").unwrap();
        assert!(control_root(&root).is_err());
        let _ = fs::remove_file(root.join(".catdesk"));
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn reviewed_build_control_children_are_closed_and_no_path_is_caller_selected() {
        assert_eq!(CONTROL_CHILDREN.len(), 5);
        assert!(CONTROL_CHILDREN.iter().all(|name| {
            !name.contains('/') && !name.contains('\\') && name.ends_with(".json")
        }));
        assert!(!CONTROL_CHILDREN.contains(&"promotion-control.json"));
    }

    #[test]
    fn control_state_records_never_reintroduce_pathname_authority() {
        // This guards the concrete T-0186 regression boundary.  Candidate,
        // output, and promotion-mirror paths are deliberately outside this
        // narrow ticket; the five immutable control records are not.
        let source = include_str!("reviewed_build.rs");
        let mut forbidden: Vec<String> = CONTROL_CHILDREN
            .iter()
            .map(|name| ["control.join(\"", name, "\")"].concat())
            .collect();
        forbidden.extend([
            ["atomic_", "json(&control"].concat(),
            ["read_", "json::<"].concat(),
        ]);
        for forbidden in forbidden {
            assert!(
                !source.contains(&forbidden),
                "control-state pathname authority returned: {forbidden}"
            );
        }
        for required in CONTROL_CHILDREN {
            assert!(
                source.contains(&format!("control_read_json(&control, \"{required}\")"))
                    || source.contains(&format!("control_create_json(&control, \"{required}\""))
            );
        }
    }

    #[test]
    fn output_and_candidate_authority_stay_handle_relative() {
        let source = include_str!("reviewed_build.rs");
        // Split names so this regression does not satisfy its own search.
        for forbidden in [
            ["safe_workspace_", "regular_file("].concat(),
            ["safe_workspace_", "output_file("].concat(),
            ["sha256_", "regular_file("].concat(),
            ["copy_", "regular_file_create_new("].concat(),
            ["target.join(\"release\")", ".join(\"catdesk.exe\")"].concat(),
        ] {
            assert!(
                !source.contains(&forbidden),
                "pathname output/candidate authority returned: {forbidden}"
            );
        }
        assert!(source.contains("open_relative_regular_file("));
        assert!(source.contains("create_relative_regular_file("));
        assert!(source.contains("copy_open_regular_files("));
        assert!(source.contains("evidence_from_open_regular("));
    }

    #[test]
    fn built_output_first_authority_seam_precedes_every_child_acquisition() {
        let source = include_str!("reviewed_build.rs");
        let start = source
            .find("fn open_built_output(")
            .expect("built-output helper");
        let end = start + source[start..].find("\n}\n\n").expect("built-output end");
        let body = &source[start..end];
        let seam = body
            .find("built-output-first-authority")
            .expect("first-authority seam");
        let first_open = body
            .find("open_relative_regular_file(target, \"catdesk.exe\"")
            .expect("built child open");
        assert!(
            seam < first_open,
            "child open precedes first-authority seam"
        );
        assert!(!body[..seam].contains("opened_regular_identity"));
        assert!(!body[..seam].contains("baseline_file"));
    }

    #[test]
    fn reviewed_build_worker_crosses_local_output_policy_before_all_post_cargo_authority() {
        let source = include_str!("reviewed_build.rs");
        let worker = source
            .find("pub fn run_reviewed_build_worker")
            .expect("reviewed-build worker");
        let open = worker
            + source[worker..]
                .find("let mut built = open_built_output")
                .expect("worker output acquisition");
        let handoff = worker
            + source[worker..]
                .find("require_trusted_final_link_handoff()?;")
                .expect("local output policy gate");
        let candidate = worker
            + source[worker..]
                .find("let candidate_parent = candidate_parent_for_create")
                .expect("candidate authority");
        let attestation = worker
            + source[worker..]
                .find("let mut attestation = ReviewedBuildAttestationV2")
                .expect("attestation construction");
        assert!(handoff < open, "output opened before local policy gate");
        assert!(
            handoff < candidate,
            "candidate created before local policy gate"
        );
        assert!(
            handoff < attestation,
            "attestation created before local policy gate"
        );
        #[cfg(windows)]
        assert!(require_trusted_final_link_handoff().is_ok());
        #[cfg(not(windows))]
        assert_eq!(
            require_trusted_final_link_handoff().unwrap_err(),
            "REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED"
        );
        assert_eq!(
            BUILD_POLICY_VERSION,
            "CATDESK_REVIEWED_BUILD_POLICY_V5_OFFLINE_VALIDATED_CARGO_CACHE"
        );
    }

    #[test]
    fn cargo_failure_diagnostic_redacts_network_failure_and_bounds_capture() {
        let secret = "credential=must-not-persist";
        let mut stderr =
            std::iter::repeat_n(b'x', MAX_CARGO_FAILURE_DIAGNOSTIC_BYTES + 128).collect::<Vec<_>>();
        stderr.extend_from_slice(
            format!(
                "warning: spurious network error: Could not connect to index.crates.io ({secret})\n"
            )
            .as_bytes(),
        );
        stderr.extend(std::iter::repeat_n(b'x', 128));
        let captured =
            read_bounded_cargo_stderr(std::io::Cursor::new(stderr)).expect("bounded drain");
        assert_eq!(captured.captured.len(), MAX_CARGO_FAILURE_DIAGNOSTIC_BYTES);
        assert!(captured.truncated);

        let diagnostic = summarize_cargo_failure(Some(101), captured);
        assert_eq!(diagnostic.phase, "CARGO_BUILD");
        assert_eq!(diagnostic.exit_code, Some(101));
        assert_eq!(
            diagnostic.classification,
            "CARGO_DEPENDENCY_NETWORK_UNAVAILABLE"
        );
        let persisted = serde_json::to_string(&diagnostic).expect("diagnostic json");
        assert!(!persisted.contains(secret));
        assert!(!persisted.contains("index.crates.io"));
        assert!(persisted.contains("capturedStderrSha256"));
    }

    #[cfg(windows)]
    #[test]
    fn reviewed_msvc_install_roots_are_closed_and_include_vs18_x86_compatibility() {
        let roots = reviewed_msvc_install_roots("C:");
        assert_eq!(roots.len(), 8);
        let rendered = roots
            .iter()
            .map(|(_, _, path)| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(rendered.contains(
            &r"C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC".to_string()
        ));
        assert!(
            rendered.contains(
                &r"C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Tools\MSVC"
                    .to_string()
            )
        );
        assert!(rendered.iter().all(|path| {
            path.starts_with(r"C:\Program Files\") || path.starts_with(r"C:\Program Files (x86)\")
        }));
        assert!(rendered.iter().all(|path| !path.contains('%')));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "manual current-host fixed-root discovery probe; no protected-state mutation"]
    fn reviewed_windows_toolchain_current_host_discovery_probe_is_manual_only() {
        let (cargo, _) = trusted_toolchain().expect("trusted Rust toolchain");
        let environment = reviewed_windows_toolchain_environment(Path::new(&cargo.absolute_path))
            .expect("fixed Windows linker/toolchain environment");
        assert!(!environment.path.is_empty());
        assert!(!environment.lib.is_empty());
        assert!(!environment.libpath.is_empty());
        assert!(!environment.include.is_empty());
    }

    #[cfg(windows)]
    fn fixed_v5_linker_diagnostic_audit_fixture()
    -> (ReviewedBuildAttemptV1, ReviewedBuildTerminalAuditV1) {
        let attempt = test_attempt();
        let claim = ReviewedBuildClaimV1 {
            schema_version: CLAIM_SCHEMA_VERSION,
            build_attempt_id: attempt.build_attempt_id.clone(),
            owner_id: "a".repeat(32),
            spawn_generation: 1,
            state: "SPAWN_OWNER_RESERVED".into(),
        };
        let owner = ReviewedBuildOwnerProofV1 {
            schema_version: 1,
            build_attempt_id: attempt.build_attempt_id.clone(),
            owner_id: claim.owner_id.clone(),
            spawn_generation: 1,
        };
        let diagnostic = summarize_cargo_failure(
            Some(101),
            read_bounded_cargo_stderr(std::io::Cursor::new(
                b"error: linking with `link.exe` failed: exit code: 1".to_vec(),
            ))
            .expect("linker capture"),
        );
        let result = ReviewedBuildResultV1 {
            schema_version: RESULT_SCHEMA_VERSION,
            build_attempt_id: attempt.build_attempt_id.clone(),
            owner_id: claim.owner_id.clone(),
            state: "BUILD_FAILED_OR_AMBIGUOUS".into(),
            attestation_digest: None,
            failure_code: Some("REVIEWED_BUILD_FAILED".into()),
            failure_diagnostic: Some(diagnostic),
        };
        (
            attempt.clone(),
            ReviewedBuildTerminalAuditV1 {
                schema_version: 1,
                attempt,
                claim,
                owner,
                result,
            },
        )
    }

    #[cfg(windows)]
    #[test]
    fn fixed_v5_host_linker_diagnostic_maps_only_redacted_fixed_outcomes() {
        for (stderr, expected, codes) in [
            (
                b"error: linker `link.exe` not found".as_slice(),
                "LINKER_EXECUTABLE_NOT_FOUND",
                &[][..],
            ),
            (
                b"LINK : fatal error LNK1104: cannot open file 'x.lib'".as_slice(),
                "MSVC_OR_WINDOWS_SDK_LIBRARY_ENV_MISSING",
                &["LNK1104"][..],
            ),
            (
                b"LINK : fatal error LNK1104: cannot open file 'catdesk.pdb'".as_slice(),
                "LINK_INPUT_UNAVAILABLE",
                &["LNK1104"][..],
            ),
            (
                b"error LNK2019: unresolved external symbol".as_slice(),
                "LINK_UNRESOLVED_EXTERNAL",
                &["LNK2019"][..],
            ),
            (
                b"fatal error LNK1318: unexpected PDB error".as_slice(),
                "LINK_PDB_FAILURE",
                &["LNK1318"][..],
            ),
            (
                b"fatal error LNK1102: out of memory".as_slice(),
                "LINK_OUT_OF_MEMORY",
                &["LNK1102"][..],
            ),
            (
                b"error: linking with `link.exe` failed: exit code: 1".as_slice(),
                "OTHER_LINKER_EXIT",
                &[][..],
            ),
            (
                b"fatal error LNK9999: unrecognized linker condition".as_slice(),
                "OTHER_LINKER_EXIT",
                &[][..],
            ),
        ] {
            let outcome = fixed_v5_host_linker_diagnostic_outcome(
                Some(101),
                read_bounded_cargo_stderr(std::io::Cursor::new(stderr.to_vec()))
                    .expect("bounded capture"),
            )
            .expect("nonzero outcome");
            assert_eq!(outcome.classification, expected);
            assert_eq!(outcome.linker_error_codes, codes);
            assert_eq!(outcome.captured_stderr_length, stderr.len());
            assert!(valid_sha256(&outcome.captured_stderr_sha256));
            assert!(!format!("{outcome:?}").contains("link.exe"));
            match expected {
                "MSVC_OR_WINDOWS_SDK_LIBRARY_ENV_MISSING" => {
                    assert_eq!(outcome.missing_link_library.as_deref(), Some("x.lib"));
                    assert_eq!(outcome.missing_link_input.as_deref(), Some("x.lib"));
                }
                "LINK_INPUT_UNAVAILABLE" => {
                    assert_eq!(outcome.missing_link_library, None);
                    assert_eq!(outcome.missing_link_input.as_deref(), Some("catdesk.pdb"));
                }
                _ => {
                    assert_eq!(outcome.missing_link_library, None);
                    assert_eq!(outcome.missing_link_input, None);
                }
            }
        }
        assert_eq!(
            fixed_v5_host_linker_diagnostic_outcome(Some(0), BoundedCargoStderr::default())
                .unwrap_err(),
            "REVIEWED_BUILD_HOST_LINKER_DIAGNOSTIC_UNEXPECTED_SUCCESS"
        );
    }

    #[cfg(windows)]
    #[test]
    fn fixed_v5_host_linker_diagnostic_retains_only_fixed_safe_signals_from_full_stderr() {
        let mut stderr = vec![b'x'; MAX_CARGO_FAILURE_DIAGNOSTIC_BYTES + 80];
        stderr.extend_from_slice(
            b" C:\\Users\\private\\secret\\output.obj: error LNK1104: cannot open file 'C:\\Users\\private\\secret\\kernel32.lib'; API_KEY=private-value /OUT:C:\\private\\bin\\catdesk.exe arbitrary-linker-argument",
        );
        stderr.extend_from_slice(b"\nerror LNK2019: unresolved external symbol");
        let outcome = fixed_v5_host_linker_diagnostic_outcome(
            Some(101),
            read_bounded_cargo_stderr(std::io::Cursor::new(stderr)).expect("bounded capture"),
        )
        .expect("nonzero outcome");

        assert_eq!(
            outcome.classification,
            "MSVC_OR_WINDOWS_SDK_LIBRARY_ENV_MISSING"
        );
        assert_eq!(outcome.linker_error_codes, ["LNK1104", "LNK2019"]);
        assert_eq!(
            outcome.missing_link_library.as_deref(),
            Some("kernel32.lib")
        );
        assert_eq!(outcome.missing_link_input.as_deref(), Some("kernel32.lib"));
        assert_eq!(
            outcome.captured_stderr_length,
            MAX_CARGO_FAILURE_DIAGNOSTIC_BYTES
        );
        assert!(outcome.stderr_truncated);

        let rendered = format!(
            "{outcome:?}\n{}",
            fixed_v5_host_linker_diagnostic_summary_line(&outcome)
        );
        for forbidden in [
            "private",
            "secret",
            "API_KEY",
            "private-value",
            "/OUT:",
            "arbitrary-linker-argument",
            "output.obj",
        ] {
            assert!(
                !rendered.contains(forbidden),
                "redacted outcome leaked {forbidden}"
            );
        }
        assert!(rendered.contains("LNK1104"));
        assert!(rendered.contains("LNK2019"));
        assert!(rendered.contains("kernel32.lib"));
    }

    #[cfg(windows)]
    #[test]
    fn fixed_v5_host_linker_diagnostic_gates_terminal_v5_and_immutable_evidence() {
        let (attempt, audit) = fixed_v5_linker_diagnostic_audit_fixture();
        assert!(validate_fixed_v5_host_linker_diagnostic_audit(&attempt, &audit).is_ok());

        let before = audit.clone();
        let mut wrong_policy = audit.clone();
        wrong_policy.attempt.policy.version = LEGACY_BUILD_POLICY_V4_VERSION.into();
        assert!(validate_fixed_v5_host_linker_diagnostic_audit(&attempt, &wrong_policy).is_err());

        let mut wrong_result = audit.clone();
        wrong_result.result.failure_code = Some("OTHER".into());
        assert!(validate_fixed_v5_host_linker_diagnostic_audit(&attempt, &wrong_result).is_err());

        let mut wrong_classification = audit.clone();
        wrong_classification
            .result
            .failure_diagnostic
            .as_mut()
            .expect("diagnostic")
            .classification = "CARGO_DEPENDENCY_NETWORK_UNAVAILABLE".into();
        assert!(
            validate_fixed_v5_host_linker_diagnostic_audit(&attempt, &wrong_classification)
                .is_err()
        );

        let mut drifted = attempt.clone();
        drifted.snapshot_manifest_digest = "9".repeat(64);
        assert!(validate_fixed_v5_host_linker_diagnostic_audit(&drifted, &audit).is_err());
        assert_eq!(audit, before, "gate must not mutate terminal evidence");
    }

    #[cfg(windows)]
    #[test]
    fn fixed_v5_host_linker_diagnostic_command_is_env_closed_and_has_no_persistence_seam() {
        let attempt = test_attempt();
        let mut command = Command::new("not-executed-cargo.exe");
        let toolchain = test_windows_toolchain_environment();
        apply_exact_worker_cargo_command(
            &mut command,
            &attempt,
            Path::new(r"C:\\diagnostic\\source"),
            Path::new(r"C:\\diagnostic\\target"),
            Path::new(r"C:\\diagnostic\\tmp"),
            Path::new(r"C:\\diagnostic\\cargo-home"),
            r"C:\\trusted\\rustc.exe",
            &toolchain,
        )
        .expect("closed command");
        let names = command
            .get_envs()
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names,
            BTreeSet::from([
                "AR".into(),
                "CARGO_HOME".into(),
                "CARGO_TARGET_DIR".into(),
                "CC".into(),
                "INCLUDE".into(),
                "LIB".into(),
                "LIBPATH".into(),
                "PATH".into(),
                "RUSTC".into(),
                "SystemDrive".into(),
                "SystemRoot".into(),
                "TEMP".into(),
                "TMP".into(),
            ])
        );
        let source = include_str!("reviewed_build.rs");
        let command_start = source
            .find("fn apply_exact_worker_cargo_command(")
            .expect("closed command helper");
        let command_end = command_start
            + source[command_start..]
                .find("/// Resolve the fixed-root Windows toolchain")
                .expect("closed command helper end");
        let command_body = &source[command_start..command_end];
        assert!(command_body.contains(".env_clear()"));
        for derived in ["CC", "AR", "LIB", "LIBPATH", "INCLUDE"] {
            assert!(command_body.contains(&format!(".env(\"{derived}\", &toolchain.")));
        }
        let system_root = command
            .get_envs()
            .find(|(name, _)| name.to_string_lossy() == "SystemRoot")
            .and_then(|(_, value)| value)
            .expect("SystemRoot must be set");
        assert_eq!(
            system_root,
            os_system_root().expect("Windows root").as_os_str()
        );
        for ambient in [
            "LIB",
            "LIBPATH",
            "INCLUDE",
            "SystemRoot",
            "VCINSTALLDIR",
            "VSINSTALLDIR",
        ] {
            assert!(!command_body.contains(&format!("std::env::var(\"{ambient}\")")));
            assert!(!command_body.contains(&format!("std::env::var_os(\"{ambient}\")")));
        }
        let runner_start = source
            .find("fn run_fixed_v5_host_linker_diagnostic()")
            .expect("diagnostic runner");
        let runner_end = runner_start
            + source[runner_start..]
                .find("/// Explicit local-workstation reviewed-output policy boundary.")
                .expect("diagnostic runner end");
        let runner_body = &source[runner_start..runner_end];
        assert!(!runner_body.contains("control_create_json("));
        assert!(!runner_body.contains("attestation.json"));
        assert!(!runner_body.contains("atomic_json("));
    }

    #[cfg(windows)]
    #[test]
    fn fixed_v5_cargo_cache_seed_diagnostic_vocabulary_and_components_are_closed() {
        let package = LockedRegistryCrate {
            name: "serde".into(),
            version: "1.0.0".into(),
            checksum: "0".repeat(64),
        };
        for stage in [
            "LOCKFILE_PARSE",
            "USER_PROFILE",
            "AMBIENT_CARGO_ROOT",
            "REGISTRY_IDENTITY",
            "REGISTRY_CONFIG",
            "INDEX_ENTRY",
            "ARCHIVE",
            "DESTINATION_CREATE",
            "STABILITY_CHECK",
            "UNKNOWN",
        ] {
            let outcome = fixed_v5_cargo_cache_seed_failure(stage, Some(&package));
            assert_eq!(outcome.stage, stage);
            assert_eq!(outcome.crate_name.as_deref(), Some("serde"));
            assert_eq!(outcome.crate_version.as_deref(), Some("1.0.0"));
        }
        let unknown = fixed_v5_cargo_cache_seed_failure("C:\\private\\stage", None);
        assert_eq!(unknown.stage, "UNKNOWN");
        assert_eq!(unknown.crate_name, None);
        assert_eq!(unknown.crate_version, None);

        let unsafe_package = LockedRegistryCrate {
            name: "not\\safe".into(),
            version: "1.0.0".into(),
            checksum: "0".repeat(64),
        };
        let redacted = fixed_v5_cargo_cache_seed_failure("ARCHIVE", Some(&unsafe_package));
        assert_eq!(redacted.crate_name, None);
        assert_eq!(redacted.crate_version, None);
        assert!(!format!("{redacted:?}").contains("private"));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "manual host-only cache-seeding diagnostic; exactly one invocation per reviewed incident"]
    fn fixed_v5_cargo_cache_seed_stage_diagnostic_runner_is_manual_only() {
        let outcome = run_fixed_v5_cargo_cache_seed_diagnostic()
            .expect("fixed cargo-cache seeding diagnostic");
        assert!(matches!(
            outcome.stage,
            "SEED_SUCCEEDED"
                | "LOCKFILE_PARSE"
                | "USER_PROFILE"
                | "AMBIENT_CARGO_ROOT"
                | "REGISTRY_IDENTITY"
                | "REGISTRY_CONFIG"
                | "INDEX_ENTRY"
                | "ARCHIVE"
                | "DESTINATION_CREATE"
                | "STABILITY_CHECK"
                | "UNKNOWN"
        ));
        println!(
            "FIXED_V5_CARGO_CACHE_SEED_DIAGNOSTIC stage={} crateName={:?} crateVersion={:?}",
            outcome.stage, outcome.crate_name, outcome.crate_version
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "manual host-only diagnostic; never part of ordinary test or worker execution"]
    fn fixed_v5_host_linker_diagnostic_runner_is_manual_only() {
        let outcome = run_fixed_v5_host_linker_diagnostic().expect("fixed host linker diagnostic");
        assert!(matches!(
            outcome.classification,
            "LINKER_EXECUTABLE_NOT_FOUND"
                | "MSVC_OR_WINDOWS_SDK_LIBRARY_ENV_MISSING"
                | "LINK_INPUT_UNAVAILABLE"
                | "LINK_UNRESOLVED_EXTERNAL"
                | "LINK_PDB_FAILURE"
                | "LINK_OUT_OF_MEMORY"
                | "OTHER_LINKER_EXIT"
        ));
        println!("{}", fixed_v5_host_linker_diagnostic_summary_line(&outcome));
    }

    #[test]
    fn cargo_failure_diagnostic_classifies_lockfile_linker_and_compiler_failures() {
        let lockfile = read_bounded_cargo_stderr(std::io::Cursor::new(
            b"error: the lock file Cargo.lock needs to be updated but --locked was passed".to_vec(),
        ))
        .expect("lockfile capture");
        assert_eq!(
            summarize_cargo_failure(Some(101), lockfile).classification,
            "CARGO_LOCKFILE_OUT_OF_DATE"
        );

        let linker = read_bounded_cargo_stderr(std::io::Cursor::new(
            b"error: linking with `link.exe` failed: exit code: 1".to_vec(),
        ))
        .expect("linker capture");
        assert_eq!(
            summarize_cargo_failure(Some(101), linker).classification,
            "CARGO_LINK_FAILED"
        );

        let linker_not_found = read_bounded_cargo_stderr(std::io::Cursor::new(
            b"error: linker `link.exe` not found".to_vec(),
        ))
        .expect("linker-not-found capture");
        assert_eq!(
            summarize_cargo_failure(Some(101), linker_not_found).classification,
            "CARGO_LINKER_NOT_FOUND"
        );

        for (stderr, expected) in [
            (
                b"LINK : fatal error LNK1104: cannot open file 'x.lib'".as_slice(),
                "CARGO_LINK_LIBRARY_NOT_FOUND",
            ),
            (
                b"LINK : fatal error LNK1181: cannot open input file 'x.lib'".as_slice(),
                "CARGO_LINK_LIBRARY_NOT_FOUND",
            ),
            (
                b"LINK : fatal error LNK1104: cannot open file 'catdesk.pdb'".as_slice(),
                "CARGO_LINK_INPUT_NOT_FOUND",
            ),
            (
                b"x.obj : error LNK2019: unresolved external symbol y".as_slice(),
                "CARGO_LINK_UNRESOLVED_EXTERNAL",
            ),
            (
                b"LINK : fatal error LNK1318: Unexpected PDB error".as_slice(),
                "CARGO_LINK_PDB_FAILURE",
            ),
            (
                b"LINK : fatal error LNK1102: out of memory".as_slice(),
                "CARGO_LINK_OUT_OF_MEMORY",
            ),
        ] {
            let captured = read_bounded_cargo_stderr(std::io::Cursor::new(stderr.to_vec()))
                .expect("specific linker capture");
            assert_eq!(
                summarize_cargo_failure(Some(101), captured).classification,
                expected
            );
        }

        let mut late_linker = vec![b'x'; MAX_CARGO_FAILURE_DIAGNOSTIC_BYTES + 512];
        late_linker.extend_from_slice(b"LINK : fatal error LNK1104: cannot open file 'late.lib'");
        let late_linker = read_bounded_cargo_stderr(std::io::Cursor::new(late_linker))
            .expect("late linker capture");
        assert!(late_linker.truncated);
        assert_eq!(
            summarize_cargo_failure(Some(101), late_linker).classification,
            "CARGO_LINK_LIBRARY_NOT_FOUND"
        );

        let compiler = read_bounded_cargo_stderr(std::io::Cursor::new(
            b"error: could not compile `catdesk` (bin \"catdesk\") due to 1 previous error"
                .to_vec(),
        ))
        .expect("compiler capture");
        assert_eq!(
            summarize_cargo_failure(Some(101), compiler).classification,
            "CARGO_COMPILATION_FAILED"
        );
    }

    #[test]
    fn cargo_failure_diagnostic_recognizes_late_msvc_d8037_without_raw_error_text() {
        // The emitted error may be past the persisted 4 KiB stderr prefix.
        let mut late = vec![b'x'; MAX_CARGO_FAILURE_DIAGNOSTIC_BYTES + 1017];
        late.extend_from_slice(b"cl : Command line error D8037 : cannot create temporary il file");
        let captured =
            read_bounded_cargo_stderr(std::io::Cursor::new(late)).expect("bounded MSVC capture");
        assert!(captured.truncated);
        let diagnostic = summarize_cargo_failure(Some(101), captured);
        assert_eq!(diagnostic.classification, "CARGO_MSVC_TEMP_IL_FILE_FAILURE");
        assert_eq!(diagnostic.missing_link_library, None);
        assert_eq!(diagnostic.missing_link_input, None);
        let persisted = serde_json::to_string(&diagnostic).expect("diagnostic json");
        assert!(!persisted.contains("cannot create temporary il file"));

        // Distinguish the exact compiler code; do not guess from similar errors.
        let unrelated = read_bounded_cargo_stderr(std::io::Cursor::new(
            b"cl : Command line error D8038 : unrelated compiler error".to_vec(),
        ))
        .expect("unrelated compiler capture");
        assert_eq!(
            summarize_cargo_failure(Some(101), unrelated).classification,
            "CARGO_EXIT_NONZERO"
        );
    }

    #[test]
    fn cargo_failure_diagnostic_retains_only_safe_link_input_basename() {
        let captured = read_bounded_cargo_stderr(std::io::Cursor::new(
            b"LINK : fatal error LNK1104: cannot open file 'C:\\sdk\\Lib\\ucrt\\x64\\legacy_stdio_definitions.lib'"
                .to_vec(),
        ))
        .expect("missing-library capture");
        let diagnostic = summarize_cargo_failure(Some(101), captured);
        assert_eq!(diagnostic.classification, "CARGO_LINK_LIBRARY_NOT_FOUND");
        assert_eq!(
            diagnostic.missing_link_library.as_deref(),
            Some("legacy_stdio_definitions.lib")
        );
        assert_eq!(
            diagnostic.missing_link_input.as_deref(),
            Some("legacy_stdio_definitions.lib")
        );
        let encoded = serde_json::to_string(&diagnostic).expect("diagnostic json");
        assert!(!encoded.contains("C:\\\\sdk"));

        let non_library = read_bounded_cargo_stderr(std::io::Cursor::new(
            b"LINK : fatal error LNK1104: cannot open file 'C:\\private\\catdesk.pdb'".to_vec(),
        ))
        .expect("safe missing-link-input capture");
        let non_library_diagnostic = summarize_cargo_failure(Some(101), non_library);
        assert_eq!(
            non_library_diagnostic.classification,
            "CARGO_LINK_INPUT_NOT_FOUND"
        );
        assert_eq!(non_library_diagnostic.missing_link_library, None);
        assert_eq!(
            non_library_diagnostic.missing_link_input.as_deref(),
            Some("catdesk.pdb")
        );
        let encoded =
            serde_json::to_string(&non_library_diagnostic).expect("non-library diagnostic json");
        assert!(!encoded.contains("C:\\\\private"));

        let unsafe_name = read_bounded_cargo_stderr(std::io::Cursor::new(
            b"LINK : fatal error LNK1181: cannot open input file 'C:\\private\\not safe.lib'"
                .to_vec(),
        ))
        .expect("unsafe missing-link-input capture");
        let unsafe_diagnostic = summarize_cargo_failure(Some(101), unsafe_name);
        assert_eq!(
            unsafe_diagnostic.classification,
            "CARGO_LINK_INPUT_NOT_FOUND"
        );
        assert_eq!(unsafe_diagnostic.missing_link_library, None);
        assert_eq!(unsafe_diagnostic.missing_link_input, None);
    }

    #[test]
    fn cargo_failure_diagnostic_extracts_missing_link_input_across_reader_chunk_boundary() {
        let mut stderr = vec![b'x'; 1016];
        stderr.extend_from_slice(b" LNK1104: cannot open file 'legacy_stdio_definitions.lib'");
        let captured = read_bounded_cargo_stderr(std::io::Cursor::new(stderr))
            .expect("split missing-link-input capture");
        let diagnostic = summarize_cargo_failure(Some(101), captured);
        assert_eq!(diagnostic.classification, "CARGO_LINK_LIBRARY_NOT_FOUND");
        assert_eq!(
            diagnostic.missing_link_library.as_deref(),
            Some("legacy_stdio_definitions.lib")
        );
        assert_eq!(
            diagnostic.missing_link_input.as_deref(),
            Some("legacy_stdio_definitions.lib")
        );
    }

    #[test]
    fn cargo_failure_diagnostic_keeps_unrecognized_failure_generic() {
        let captured = read_bounded_cargo_stderr(std::io::Cursor::new(
            b"opaque cargo failure without a reviewed signature".to_vec(),
        ))
        .expect("generic capture");
        let diagnostic = summarize_cargo_failure(Some(1), captured);
        assert_eq!(diagnostic.classification, "CARGO_EXIT_NONZERO");
        assert_eq!(diagnostic.captured_stderr_length, 49);
        assert!(!diagnostic.stderr_truncated);
    }

    #[test]
    fn producer_handle_feasibility_cannot_bypass_the_final_link_handoff_gate() {
        let source = include_str!("reviewed_build.rs");
        let worker = source
            .find("pub fn run_reviewed_build_worker")
            .expect("reviewed-build worker");
        let worker_body = &source[worker..];
        let gate = worker_body
            .find("require_trusted_final_link_handoff()?;")
            .expect("final-link handoff gate");
        let output = worker_body
            .find("let mut built = open_built_output")
            .expect("output acquisition");
        assert!(
            gate < output,
            "experimental code bypasses the production gate"
        );
        assert!(
            !worker_body[..output].contains("duplicate_file_handle_for_feasibility"),
            "test-only duplicate primitive became production output authority"
        );
        assert!(source.contains("#[cfg(all(test, windows))]"));
    }

    #[cfg(windows)]
    #[test]
    fn linker_broker_capability_rejects_spoofed_replayed_and_handleless_records() {
        let mut capability = LinkerBrokerCapabilityV1::new(
            "session",
            &"a".repeat(32),
            101,
            202,
            "catdesk-broker-identity",
        );
        let valid = LinkerBrokerHandoffV1 {
            schema_version: 1,
            session_id: capability.session_id.clone(),
            build_attempt_id: capability.build_attempt_id.clone(),
            parent_pid: capability.parent_pid,
            broker_pid: capability.broker_pid,
            linker_pid: 303,
            nonce: capability.nonce.clone(),
            output_token: capability.output_token.clone(),
            broker_identity: capability.broker_identity.clone(),
            linker_identity: "attested-linker-identity".into(),
            linker_in_exact_job: true,
            output_identity: "00000001:00000002:00000003".into(),
        };
        let mut spoofed = LinkerBrokerHandoffV1 {
            nonce: "wrong".into(),
            ..valid
        };
        assert_eq!(
            capability
                .validate_without_output_handle(&spoofed)
                .unwrap_err(),
            BROKER_HANDOFF_REJECTED
        );
        spoofed.nonce = capability.nonce.clone();
        spoofed.linker_in_exact_job = false;
        assert_eq!(
            capability
                .validate_without_output_handle(&spoofed)
                .unwrap_err(),
            BROKER_HANDOFF_REJECTED
        );
        spoofed.linker_in_exact_job = true;
        assert_eq!(
            capability
                .validate_without_output_handle(&spoofed)
                .unwrap_err(),
            PRODUCER_HANDLE_NOT_CAPTURED
        );
        capability.consumed = true;
        assert_eq!(
            capability
                .validate_without_output_handle(&spoofed)
                .unwrap_err(),
            PRODUCER_HANDOFF_REPLAYED
        );
    }

    #[cfg(windows)]
    #[test]
    fn linker_discovery_fails_closed_without_a_policy_forced_final_linker() {
        let tool = TrustedBuildToolV1 {
            absolute_path: r"C:\\trusted\\tool.exe".into(),
            sha256: "a".repeat(64),
            length: 1,
            identity: "00000001:00000002:00000003".into(),
            version_sha256: "b".repeat(64),
        };
        assert_eq!(
            policy_forced_final_linker_for_feasibility(&tool, &tool).unwrap_err(),
            LINKER_UNAVAILABLE
        );
    }

    #[cfg(windows)]
    #[test]
    fn isolated_producer_capability_rejects_owner_spoof_and_replay() {
        let attempt = "b".repeat(32);
        let mut capability =
            IsolatedProducerCapabilityV1::new(&attempt, "producer-sid", "namespace-owner-sid");
        assert_eq!(
            capability
                .validate(&attempt, "wrong-producer", &capability.nonce.clone())
                .unwrap_err(),
            ISOLATION_CAPABILITY_REJECTED
        );
        assert_eq!(
            capability
                .validate(&attempt, "producer-sid", "wrong-nonce")
                .unwrap_err(),
            ISOLATION_CAPABILITY_REJECTED
        );
        let nonce = capability.nonce.clone();
        capability
            .validate(&attempt, "producer-sid", &nonce)
            .expect("one exact isolated capability");
        assert_eq!(
            capability
                .validate(&attempt, "producer-sid", &nonce)
                .unwrap_err(),
            ISOLATION_CAPABILITY_REJECTED
        );
        let mut owner_spoof = IsolatedProducerCapabilityV1::new(&attempt, "same-sid", "same-sid");
        assert_eq!(
            owner_spoof
                .validate(&attempt, "same-sid", &owner_spoof.nonce.clone())
                .unwrap_err(),
            ISOLATION_CAPABILITY_REJECTED
        );
    }

    #[cfg(windows)]
    #[test]
    fn ordinary_token_owner_is_not_accepted_as_an_isolated_producer_root() {
        let error = current_token_isolation_owner_separated().unwrap_err();
        assert!(
            error == ISOLATION_OWNER_REACQUIRABLE || error == ISOLATION_UNPROVEN,
            "unexpected normal-token isolation result: {error}"
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "invoked only by the fixed AppContainer lowbox harness"]
    fn appcontainer_fixed_helper_writes_only_fixed_output_child() {
        // The lowbox harness controls the current directory from its pinned
        // producer-root handle. This helper accepts no path, executable, or
        // payload argument and creates exactly one create-new fixed child.
        let path = std::env::current_dir()
            .expect("fixed helper current directory")
            .join(APPCONTAINER_FIXED_OUTPUT_CHILD);
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("fixed helper create-new output");
        output
            .write_all(b"catdesk-appcontainer-fixed-output-v1")
            .expect("fixed helper output bytes");
        output.sync_all().expect("fixed helper output flush");
    }

    #[cfg(windows)]
    #[test]
    fn real_appcontainer_lowbox_attempt_remains_non_authoritative_without_secured_root() {
        let error = appcontainer_lowbox_launch_feasibility_attempt().unwrap_err();
        println!("R5D-R1 AppContainer feasibility result: {error}");
        assert!(
            error == APPCONTAINER_LAUNCH_UNAVAILABLE
                || error == APPCONTAINER_TOKEN_UNPROVEN
                || error == APPCONTAINER_OUTPUT_UNPROVEN
                || error == APPCONTAINER_SECURITY_UNPROVEN,
            "unexpected AppContainer feasibility result: {error}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn appcontainer_secured_root_host_proof_stops_before_any_output_authority() {
        // This is intentionally an ordinary host-focused test, not an ignored
        // synthetic proof. A positive host implementation must replace the
        // final output-unproven result only after it atomically creates the
        // descriptor-protected RootDirectory child and retains its real helper
        // output handle. Until then every result is fail closed.
        let error = appcontainer_lowbox_launch_feasibility_attempt().unwrap_err();
        println!("R5D-R2 secured-root host proof result: {error}");
        assert!(
            error == APPCONTAINER_LAUNCH_UNAVAILABLE
                || error == APPCONTAINER_TOKEN_UNPROVEN
                || error == APPCONTAINER_OUTPUT_UNPROVEN
                || error == APPCONTAINER_SECURITY_UNPROVEN
        );
        let source = include_str!("reviewed_build.rs");
        let worker = source
            .find("pub fn run_reviewed_build_worker")
            .expect("reviewed-build worker");
        let worker_body = &source[worker..];
        assert!(
            worker_body
                .find("require_trusted_final_link_handoff()?;")
                .expect("production gate")
                < worker_body
                    .find("let mut built = open_built_output")
                    .expect("output acquisition")
        );
    }

    #[cfg(windows)]
    #[test]
    fn appcontainer_fixed_helper_and_secured_root_remain_test_only_non_authority() {
        let source = include_str!("reviewed_build.rs");
        let helper = source
            .find("fn appcontainer_fixed_helper_writes_only_fixed_output_child")
            .expect("fixed helper");
        let prototype = source
            .find("fn appcontainer_lowbox_launch_feasibility_attempt")
            .expect("lowbox prototype");
        let worker = source
            .find("pub fn run_reviewed_build_worker")
            .expect("reviewed-build worker");
        let worker_body = &source[worker..];
        assert!(
            source[helper.saturating_sub(180)..helper]
                .contains("#[ignore = \"invoked only by the fixed AppContainer lowbox harness\"]")
        );
        assert!(source[..prototype].contains("#[cfg(all(test, windows))]"));
        assert!(
            worker_body
                .find("require_trusted_final_link_handoff()?;")
                .expect("production gate")
                < worker_body
                    .find("let mut built = open_built_output")
                    .expect("output acquisition")
        );
        let before_output = &worker_body[..worker_body
            .find("let mut built = open_built_output")
            .expect("output acquisition")];
        assert!(!before_output.contains("appcontainer_lowbox_launch_feasibility_attempt"));
        assert!(!before_output.contains("appcontainer_interactive_attack_matrix"));
    }

    #[test]
    fn dedicated_producer_read_only_plan_is_fixed_and_idempotent() {
        let mut backend = FakeDedicatedProducerBackend::new(DedicatedProducerExistingState::Absent);
        let first = dedicated_producer_read_only_preflight(&mut backend).unwrap();
        let second = dedicated_producer_read_only_preflight(&mut backend).unwrap();
        assert_eq!(first, second);
        let DedicatedProducerProvisioningPlan::ReadOnlyAbsent(policy) = first else {
            panic!("absent state must produce a read-only fixed plan");
        };
        assert_eq!(policy.service_name, "CatDeskReviewedProducer");
        assert_eq!(
            policy.service_binary,
            r"C:\Program Files\CatDesk\CatDeskReviewedProducer.exe"
        );
        assert_eq!(
            policy.reviewed_service_image,
            r"C:\Program Files\CatDesk\CatDesk.exe"
        );
        assert_eq!(
            policy.namespace,
            r"C:\ProgramData\CatDesk\reviewed-producer"
        );
        for denied in [
            "FILE_WRITE_DATA",
            "FILE_APPEND_DATA",
            "FILE_ADD_FILE",
            "FILE_ADD_SUBDIRECTORY",
            "DELETE",
            "PARENT_DELETE_CHILD",
            "RENAME_REPARSE",
            "WRITE_DAC",
            "WRITE_OWNER",
            "OWNER_RIGHTS_RECOVERY",
            "TAKE_OWNERSHIP_RECOVERY",
        ] {
            assert!(policy.denied_interactive_rights.contains(&denied));
        }
        assert_eq!(backend.provision_calls, 0);
        assert_eq!(backend.rollback_calls, 0);
        assert!(backend.exact_policy_only);
    }

    #[test]
    fn dedicated_producer_preflight_refuses_ambiguous_partial_and_recoverable_state() {
        for state in [
            DedicatedProducerExistingState::Partial,
            DedicatedProducerExistingState::Ambiguous,
            DedicatedProducerExistingState::InteractiveRecoveryPossible,
        ] {
            let mut backend = FakeDedicatedProducerBackend::new(state);
            assert_eq!(
                dedicated_producer_read_only_preflight(&mut backend).unwrap_err(),
                DEDICATED_PRODUCER_PROVISIONING_REFUSED
            );
            assert_eq!(backend.provision_calls, 0);
            assert_eq!(backend.rollback_calls, 0);
            assert!(backend.exact_policy_only);
        }
    }

    #[test]
    fn dedicated_producer_execution_requires_admin_exact_plan_and_proven_rollback() {
        let mut backend = FakeDedicatedProducerBackend::new(DedicatedProducerExistingState::Absent);
        let plan = dedicated_producer_read_only_preflight(&mut backend).unwrap();
        assert_eq!(
            execute_dedicated_producer_fixed_plan(&mut backend, &plan).unwrap_err(),
            DEDICATED_PRODUCER_ADMIN_REQUIRED
        );
        assert_eq!(backend.provision_calls, 0);

        let DedicatedProducerProvisioningPlan::ReadOnlyAbsent(mut forged_source) = plan.clone()
        else {
            panic!("expected fixed absent plan");
        };
        forged_source.reviewed_service_image = r"C:\attacker-image.exe";
        assert_eq!(
            execute_dedicated_producer_fixed_plan(
                &mut backend,
                &DedicatedProducerProvisioningPlan::ReadOnlyAbsent(forged_source),
            )
            .unwrap_err(),
            DEDICATED_PRODUCER_PROVISIONING_REFUSED
        );
        assert_eq!(backend.provision_calls, 0);

        let DedicatedProducerProvisioningPlan::ReadOnlyAbsent(mut forged) = plan.clone() else {
            panic!("expected fixed absent plan");
        };
        forged.service_binary = r"C:\attacker.exe";
        backend.administrator = true;
        assert_eq!(
            execute_dedicated_producer_fixed_plan(
                &mut backend,
                &DedicatedProducerProvisioningPlan::ReadOnlyAbsent(forged),
            )
            .unwrap_err(),
            DEDICATED_PRODUCER_PROVISIONING_REFUSED
        );
        assert_eq!(backend.provision_calls, 0);

        backend.provision_succeeds = false;
        assert_eq!(
            execute_dedicated_producer_fixed_plan(&mut backend, &plan).unwrap_err(),
            DEDICATED_PRODUCER_PROVISIONING_REFUSED
        );
        assert_eq!(backend.rollback_calls, 1);
        assert_eq!(backend.state, DedicatedProducerExistingState::Absent);

        backend.provision_succeeds = false;
        backend.rollback_succeeds = false;
        assert_eq!(
            execute_dedicated_producer_fixed_plan(&mut backend, &plan).unwrap_err(),
            DEDICATED_PRODUCER_ROLLBACK_UNPROVEN
        );
        assert!(backend.exact_policy_only);
    }

    #[test]
    fn dedicated_producer_admin_cli_is_exact_no_input_and_non_authoritative_in_tests() {
        let flag = DEDICATED_PRODUCER_ADMIN_PROVISION_FLAG.to_string();
        assert_eq!(
            parse_dedicated_producer_admin_provision_args(std::slice::from_ref(&flag)),
            Ok(true)
        );
        for hostile in [
            vec![flag.clone(), "--service=attacker".into()],
            vec![flag.clone(), "C:\\attacker.exe".into()],
            vec![flag.clone(), "--sddl=D:(A;;GA;;;WD)".into()],
            vec![flag.clone(), flag.clone()],
        ] {
            assert!(parse_dedicated_producer_admin_provision_args(&hostile).is_err());
        }
        assert_eq!(
            parse_dedicated_producer_admin_provision_args(&["--unrelated".into()]),
            Ok(false)
        );
        let source = include_str!("reviewed_build.rs");
        let main_source = include_str!("main.rs");
        let mcp_source = include_str!("mcp.rs");
        let autonomous_source = include_str!("delegated/autonomous_contract.rs");
        assert!(main_source.contains("parse_dedicated_producer_admin_provision_args"));
        assert!(main_source.contains("run_dedicated_producer_admin_provision_command"));
        assert!(!mcp_source.contains(DEDICATED_PRODUCER_ADMIN_PROVISION_FLAG));
        assert!(!autonomous_source.contains(DEDICATED_PRODUCER_ADMIN_PROVISION_FLAG));
        assert!(
            crate::command::detect_lifecycle_facade_intercept(&format!("catdesk {flag}")).is_none()
        );
        assert!(source.contains("execute_dedicated_producer_fixed_policy_as_administrator()?"));
        #[cfg(windows)]
        assert_eq!(
            run_dedicated_producer_admin_provision_command().unwrap_err(),
            DEDICATED_PRODUCER_MUTATION_HOST_UNAVAILABLE
        );
    }

    #[test]
    fn dedicated_producer_service_entry_is_exact_fixed_and_non_authoritative() {
        let mode = DEDICATED_PRODUCER_SERVICE_MODE.to_string();
        assert_eq!(
            parse_dedicated_producer_service_args(std::slice::from_ref(&mode)),
            Ok(true)
        );
        for hostile in [
            vec![mode.clone(), "--pipe=attacker".into()],
            vec![mode.clone(), "C:\\attacker.exe".into()],
            vec![mode.clone(), "--command=build".into()],
            vec![mode.clone(), "--environment=PATH=attacker".into()],
            vec![mode.clone(), mode.clone()],
        ] {
            assert!(parse_dedicated_producer_service_args(&hostile).is_err());
        }
        assert_eq!(
            parse_dedicated_producer_service_args(&["--unrelated".into()]),
            Ok(false)
        );
        assert_eq!(
            run_dedicated_producer_service_command().unwrap_err(),
            DEDICATED_PRODUCER_SERVICE_IPC_UNAVAILABLE
        );
        let source = include_str!("reviewed_build.rs");
        let main_source = include_str!("main.rs");
        let mcp_source = include_str!("mcp.rs");
        let autonomous_source = include_str!("delegated/autonomous_contract.rs");
        assert!(main_source.contains("parse_dedicated_producer_service_args"));
        assert!(main_source.contains("run_dedicated_producer_service_command"));
        assert!(!mcp_source.contains(DEDICATED_PRODUCER_SERVICE_MODE));
        assert!(!autonomous_source.contains(DEDICATED_PRODUCER_SERVICE_MODE));
        let runtime = source
            .find("fn run_fixed_ipc_endpoint")
            .expect("closed service runtime");
        let body = &source[runtime..runtime + 900.min(source.len() - runtime)];
        assert!(!body.contains("Command::new"));
        assert!(!body.contains("open_built_output"));
    }

    #[test]
    fn dedicated_producer_execution_capability_rejects_spoofs_replay_and_path_swaps() {
        let attempt = test_attempt();
        let root = std::env::temp_dir().join(format!("catdesk-r5d-r3f-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("catdesk-r5d-r3f-outside-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("sentinel.bin");
        fs::write(&sentinel, b"outside-sentinel-r3f").unwrap();
        let sentinel_before = fs::read(&sentinel).unwrap();
        let output_path = root.join("catdesk.exe");
        let genuine = b"genuine-producer-0000";
        let attacker = b"attacker-producer-000";
        assert_eq!(genuine.len(), attacker.len());
        fs::write(&output_path, genuine).unwrap();
        let mut output = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&output_path)
            .unwrap();
        let evidence = evidence_from_open_regular(&mut output, "R5D-R3F genuine handle").unwrap();
        let request = DedicatedProducerExecutionRequestV1::new(&attempt).unwrap();
        let nonce = request.nonce.clone();
        let sid = sha256("CatDeskReviewedProducer restricted SID");
        let peer = DedicatedProducerPeerEvidenceV1 {
            service_sid_digest: sid.clone(),
            process_identity: "pid:service-image-sha".into(),
            token_identity: "service-token-sid".into(),
            pipe_acl_digest: sha256("fixed pipe ACL"),
            isolated_root_identity: "volume:fileid".into(),
            exact_job_member: true,
            owner_acl_verified: true,
        };
        let mut handoff = DedicatedProducerTransferredOutputV1 {
            request: request.clone(),
            peer: peer.clone(),
            output,
            claimed_evidence: evidence.clone(),
            consumed: false,
        };

        // Concrete same-name, same-length replacement occurs only after the
        // producer-owned object is already retained. It cannot change handle
        // evidence, and the independent outside sentinel is untouched.
        fs::rename(&output_path, root.join("catdesk.genuine")).unwrap();
        fs::write(&output_path, attacker).unwrap();
        let accepted =
            accept_dedicated_producer_transferred_output(&mut handoff, &attempt, &nonce, &sid)
                .unwrap();
        assert_eq!(accepted, evidence);
        assert_ne!(
            accepted.sha256,
            sha256(std::str::from_utf8(attacker).unwrap())
        );
        assert_eq!(fs::read(&sentinel).unwrap(), sentinel_before);
        assert_eq!(
            accept_dedicated_producer_transferred_output(&mut handoff, &attempt, &nonce, &sid)
                .unwrap_err(),
            DEDICATED_PRODUCER_HANDOFF_REJECTED
        );

        let mutations: Vec<
            Box<
                dyn Fn(
                    &mut DedicatedProducerExecutionRequestV1,
                    &mut DedicatedProducerPeerEvidenceV1,
                ),
            >,
        > = vec![
            Box::new(|request, _| request.nonce = "0".repeat(32)),
            Box::new(|request, _| request.cargo.sha256 = "0".repeat(64)),
            Box::new(|_, peer| peer.service_sid_digest = "0".repeat(64)),
            Box::new(|_, peer| peer.token_identity.clear()),
            Box::new(|_, peer| peer.exact_job_member = false),
            Box::new(|_, peer| peer.owner_acl_verified = false),
        ];
        for mutate in mutations {
            let mut file = OpenOptions::new()
                .read(true)
                .open(root.join("catdesk.genuine"))
                .unwrap();
            let mut request = request.clone();
            let mut peer = peer.clone();
            mutate(&mut request, &mut peer);
            let claim = evidence_from_open_regular(&mut file, "R5D-R3F spoof fixture").unwrap();
            let mut spoof = DedicatedProducerTransferredOutputV1 {
                request,
                peer,
                output: file,
                claimed_evidence: claim,
                consumed: false,
            };
            assert_eq!(
                accept_dedicated_producer_transferred_output(&mut spoof, &attempt, &nonce, &sid)
                    .unwrap_err(),
                DEDICATED_PRODUCER_HANDOFF_REJECTED
            );
        }
        assert_eq!(fs::read(&sentinel).unwrap(), sentinel_before);
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn dedicated_producer_provisioning_model_cannot_authorize_reviewed_build_or_mcp_ownership() {
        let source = include_str!("reviewed_build.rs");
        let worker = source
            .find("pub fn run_reviewed_build_worker")
            .expect("reviewed-build worker");
        let worker_body = &source[worker..];
        let gate = worker_body
            .find("require_trusted_final_link_handoff()?;")
            .expect("final-link gate");
        let output = worker_body
            .find("let mut built = open_built_output")
            .expect("output acquisition");
        assert!(gate < output);
        assert!(!worker_body[..output].contains("dedicated_producer_read_only_preflight"));
        assert!(!worker_body[..output].contains("execute_dedicated_producer_fixed_plan"));
        assert!(source.contains(&["Create", "ServiceW"].concat()));
        assert!(source.contains(&["ChangeService", "Config2W"].concat()));
        assert!(!source.contains(&["Start", "ServiceW"].concat()));
        assert!(source.contains("DEDICATED_PRODUCER_MUTATION_HOST_UNAVAILABLE"));
        assert!(source.contains("execute_dedicated_producer_fixed_policy_as_administrator"));
    }

    #[test]
    fn windows_dedicated_backend_orders_only_fixed_service_sid_namespace_stages() {
        let operations = FakeWindowsDedicatedProducerOperations::absent();
        let mut backend = WindowsDedicatedProducerProvisioningBackend::new(operations);
        assert_eq!(
            dedicated_producer_operator_operation(
                &mut backend,
                DedicatedProducerOperatorRequest::ReadOnlyPreflight,
            )
            .unwrap(),
            DedicatedProducerOperatorOutcome::NotProvisioned
        );
        assert_eq!(
            dedicated_producer_operator_operation(
                &mut backend,
                DedicatedProducerOperatorRequest::ExecuteFixedPolicy,
            )
            .unwrap(),
            DedicatedProducerOperatorOutcome::ActionRequired
        );
        assert_eq!(
            backend.operations.calls,
            vec![
                DedicatedProducerProvisioningStage::ImageDeployed,
                DedicatedProducerProvisioningStage::ServiceCreated,
                DedicatedProducerProvisioningStage::ServiceSidRestricted,
                DedicatedProducerProvisioningStage::NamespaceCreated,
                DedicatedProducerProvisioningStage::NamespaceSecurityApplied,
            ]
        );
        assert!(backend.operations.exact_policy_only);
        assert_eq!(
            backend.journal,
            vec![
                DedicatedProducerProvisioningStage::ImageDeployed,
                DedicatedProducerProvisioningStage::ServiceCreated,
                DedicatedProducerProvisioningStage::ServiceSidRestricted,
                DedicatedProducerProvisioningStage::NamespaceCreated,
                DedicatedProducerProvisioningStage::NamespaceSecurityApplied,
            ]
        );
        assert_eq!(backend.operations.durable_journal, backend.journal);
        assert!(!backend.operations.journal_cleared);
    }

    #[test]
    fn windows_dedicated_backend_refuses_hostile_state_admin_bypass_and_rolls_back_in_reverse() {
        let mut hostile = WindowsDedicatedProducerProvisioningBackend::new(
            FakeWindowsDedicatedProducerOperations {
                state: DedicatedProducerExistingState::Ambiguous,
                ..FakeWindowsDedicatedProducerOperations::absent()
            },
        );
        assert_eq!(
            dedicated_producer_operator_operation(
                &mut hostile,
                DedicatedProducerOperatorRequest::ReadOnlyPreflight,
            )
            .unwrap_err(),
            DEDICATED_PRODUCER_PROVISIONING_REFUSED
        );

        let mut not_admin = WindowsDedicatedProducerProvisioningBackend::new(
            FakeWindowsDedicatedProducerOperations {
                administrator: false,
                ..FakeWindowsDedicatedProducerOperations::absent()
            },
        );
        assert_eq!(
            dedicated_producer_operator_operation(
                &mut not_admin,
                DedicatedProducerOperatorRequest::ExecuteFixedPolicy,
            )
            .unwrap_err(),
            DEDICATED_PRODUCER_ADMIN_REQUIRED
        );

        let mut partial = WindowsDedicatedProducerProvisioningBackend::new(
            FakeWindowsDedicatedProducerOperations {
                fail_stage: Some(DedicatedProducerProvisioningStage::NamespaceCreated),
                ..FakeWindowsDedicatedProducerOperations::absent()
            },
        );
        assert_eq!(
            dedicated_producer_operator_operation(
                &mut partial,
                DedicatedProducerOperatorRequest::ExecuteFixedPolicy,
            )
            .unwrap_err(),
            DEDICATED_PRODUCER_NAMESPACE_SECURITY_FAILED
        );
        assert_eq!(
            partial.operations.rollbacks,
            vec![
                DedicatedProducerProvisioningStage::ServiceSidRestricted,
                DedicatedProducerProvisioningStage::ServiceCreated,
                DedicatedProducerProvisioningStage::ImageDeployed,
            ]
        );
        assert_eq!(
            partial.operations.state,
            DedicatedProducerExistingState::Absent
        );
        assert!(partial.operations.durable_journal.is_empty());
        assert!(partial.operations.journal_cleared);
        assert!(partial.operations.exact_policy_only);
    }

    #[test]
    fn dedicated_producer_deployment_stages_return_bounded_redacted_outcomes() {
        let mut missing = WindowsDedicatedProducerProvisioningBackend::new(
            FakeWindowsDedicatedProducerOperations {
                image_missing: true,
                ..FakeWindowsDedicatedProducerOperations::absent()
            },
        );
        assert_eq!(
            dedicated_producer_operator_operation(
                &mut missing,
                DedicatedProducerOperatorRequest::ExecuteFixedPolicy,
            )
            .unwrap_err(),
            DEDICATED_PRODUCER_IMAGE_MISSING
        );
        assert!(missing.operations.exact_policy_only);
        assert!(missing.operations.journal_cleared);
        for (stage, expected) in [
            (
                DedicatedProducerProvisioningStage::ImageDeployed,
                DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED,
            ),
            (
                DedicatedProducerProvisioningStage::ServiceCreated,
                DEDICATED_PRODUCER_SERVICE_CONFIGURATION_FAILED,
            ),
            (
                DedicatedProducerProvisioningStage::ServiceSidRestricted,
                DEDICATED_PRODUCER_SERVICE_SID_FAILED,
            ),
            (
                DedicatedProducerProvisioningStage::NamespaceCreated,
                DEDICATED_PRODUCER_NAMESPACE_SECURITY_FAILED,
            ),
            (
                DedicatedProducerProvisioningStage::NamespaceSecurityApplied,
                DEDICATED_PRODUCER_NAMESPACE_SECURITY_FAILED,
            ),
        ] {
            let mut backend = WindowsDedicatedProducerProvisioningBackend::new(
                FakeWindowsDedicatedProducerOperations {
                    fail_stage: Some(stage),
                    ..FakeWindowsDedicatedProducerOperations::absent()
                },
            );
            assert_eq!(
                dedicated_producer_operator_operation(
                    &mut backend,
                    DedicatedProducerOperatorRequest::ExecuteFixedPolicy,
                )
                .unwrap_err(),
                expected
            );
            assert!(backend.operations.exact_policy_only);
            assert!(backend.operations.journal_cleared);
        }
    }

    #[cfg(windows)]
    #[test]
    fn administrator_fixed_mutation_host_is_no_input_and_unreachable_from_unit_tests() {
        let source = include_str!("reviewed_build.rs");
        let host = source
            .find("struct DedicatedProducerAdministratorMutationHost")
            .expect("separate administrator host");
        let execute = source
            .find("fn execute_dedicated_producer_fixed_policy_as_administrator")
            .expect("fixed execute entry");
        let readonly = source
            .find("fn dedicated_producer_windows_operator_surface")
            .expect("ordinary read-only surface");
        assert!(host < execute);
        assert!(readonly < host);
        let entry = &source[execute..execute + 500.min(source.len() - execute)];
        assert!(!entry.contains("&str"));
        assert!(!entry.contains("PathBuf"));
        assert_eq!(
            dedicated_producer_windows_operator_surface(
                DedicatedProducerOperatorRequest::ExecuteFixedPolicy,
            )
            .unwrap_err(),
            DEDICATED_PRODUCER_MUTATION_HOST_UNAVAILABLE
        );
        assert_eq!(
            execute_dedicated_producer_fixed_policy_as_administrator().unwrap_err(),
            DEDICATED_PRODUCER_MUTATION_HOST_UNAVAILABLE
        );
        assert!(source.contains("CreateServiceW"));
        assert!(source.contains("ChangeServiceConfig2W"));
        assert!(source.contains("CreateDirectoryW"));
        assert!(source.contains("record_durable_stage"));
        assert!(source.contains("rollback_exact_stage"));
    }

    #[cfg(windows)]
    #[test]
    fn concrete_windows_dedicated_adapter_is_read_only_by_default_and_has_no_input_surface() {
        let source = include_str!("reviewed_build.rs");
        let adapter = source
            .find("struct SystemWindowsDedicatedProducerOperations")
            .expect("concrete Windows adapter");
        let operator = source
            .find("fn dedicated_producer_windows_operator_surface")
            .expect("bounded operator surface");
        assert!(adapter < operator);
        assert!(source.contains("OpenSCManagerW"));
        assert!(source.contains("OpenServiceW"));
        assert!(source.contains("QueryServiceConfigW"));
        assert!(source.contains("GetFileAttributesW"));
        assert!(source.contains("DedicatedProducerOperatorRequest"));
        let surface = &source[operator..operator + 600.min(source.len() - operator)];
        assert!(!surface.contains("&str"));
        let outcome = dedicated_producer_windows_operator_surface(
            DedicatedProducerOperatorRequest::ReadOnlyPreflight,
        );
        println!("R5D-R3C read-only dedicated producer preflight: {outcome:?}");
        match outcome {
            Ok(DedicatedProducerOperatorOutcome::NotProvisioned) => {}
            Err(value)
                if value == DEDICATED_PRODUCER_PROVISIONING_REFUSED
                    || value == DEDICATED_PRODUCER_NOT_PROVISIONED => {}
            other => panic!("unexpected concrete adapter preflight result: {other:?}"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn dedicated_producer_detector_is_read_only_and_fails_closed_when_unproven() {
        let result = detect_preprovisioned_dedicated_producer_service().unwrap_err();
        println!("R5D-R3 dedicated producer detector result: {result}");
        assert!(
            result == DEDICATED_PRODUCER_NOT_PROVISIONED || result == DEDICATED_PRODUCER_UNPROVEN
        );
    }

    #[cfg(windows)]
    #[test]
    fn dedicated_producer_handoff_rejects_spoof_replay_and_handleless_service_claims() {
        let attempt = "c".repeat(32);
        let mut handoff = DedicatedProducerHandoffV1::new(
            &attempt,
            &"a".repeat(64),
            &fixed_policy().policy_sha256,
            "service-sid-digest",
        );
        let nonce = handoff.nonce.clone();
        assert_eq!(
            handoff
                .validate_without_service_handle(
                    &attempt,
                    "wrong",
                    &handoff.snapshot_authority_digest.clone(),
                    &handoff.policy_digest.clone(),
                    "service-sid-digest",
                    true,
                )
                .unwrap_err(),
            DEDICATED_PRODUCER_HANDOFF_REJECTED
        );
        assert_eq!(
            handoff
                .validate_without_service_handle(
                    &attempt,
                    &nonce,
                    &handoff.snapshot_authority_digest.clone(),
                    &handoff.policy_digest.clone(),
                    "spoofed-service-sid",
                    true,
                )
                .unwrap_err(),
            DEDICATED_PRODUCER_HANDOFF_REJECTED
        );
        assert_eq!(
            handoff
                .validate_without_service_handle(
                    &attempt,
                    &nonce,
                    &handoff.snapshot_authority_digest.clone(),
                    &handoff.policy_digest.clone(),
                    "service-sid-digest",
                    true,
                )
                .unwrap_err(),
            PRODUCER_HANDLE_NOT_CAPTURED
        );
        handoff.consumed = true;
        assert_eq!(
            handoff
                .validate_without_service_handle(
                    &attempt,
                    &nonce,
                    &handoff.snapshot_authority_digest.clone(),
                    &handoff.policy_digest.clone(),
                    "service-sid-digest",
                    true,
                )
                .unwrap_err(),
            DEDICATED_PRODUCER_HANDOFF_REJECTED
        );
    }

    #[test]
    fn os_isolation_prototype_cannot_activate_before_the_final_link_handoff_gate() {
        let source = include_str!("reviewed_build.rs");
        let worker = source
            .find("pub fn run_reviewed_build_worker")
            .expect("reviewed-build worker");
        let worker_body = &source[worker..];
        let gate = worker_body
            .find("require_trusted_final_link_handoff()?;")
            .expect("production gate");
        let output = worker_body
            .find("let mut built = open_built_output")
            .expect("built output acquisition");
        assert!(gate < output);
        assert!(!worker_body[..output].contains("IsolatedProducerCapabilityV1"));
    }

    #[cfg(windows)]
    #[test]
    fn producer_handle_feasibility_state_rejects_spoofed_and_replayed_handoffs() {
        let root = std::env::temp_dir().join(format!("catdesk-r5b-state-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let parent = ProtectedDirectoryGuard::acquire(&root, "R5B state root").unwrap();
        let mut source =
            create_relative_regular_file(&parent, "catdesk.exe", "R5B producer object").unwrap();
        source.write_all(b"genuine-output-00000").unwrap();
        source.sync_all().unwrap();
        let identity = opened_regular_identity(&source, "R5B producer object").unwrap();
        let pid = std::process::id();
        let mut handoff = ProducerHandleFeasibilityHandoff::new(pid, identity.clone());
        assert_eq!(
            handoff
                .capture("wrong-nonce", pid, true, true, &source)
                .unwrap_err(),
            PRODUCER_HANDOFF_REJECTED
        );
        assert_eq!(
            handoff
                .capture(
                    &handoff.nonce.clone(),
                    pid.saturating_add(1),
                    true,
                    true,
                    &source
                )
                .unwrap_err(),
            PRODUCER_PROCESS_UNTRUSTED
        );
        assert_eq!(
            handoff
                .capture(&handoff.nonce.clone(), pid, false, true, &source)
                .unwrap_err(),
            PRODUCER_PROCESS_UNTRUSTED
        );
        assert_eq!(
            handoff
                .capture(&handoff.nonce.clone(), pid, true, false, &source)
                .unwrap_err(),
            PRODUCER_HANDLE_UNSAFE
        );
        let mut wrong =
            create_relative_regular_file(&parent, "wrong.exe", "R5B wrong object").unwrap();
        wrong.write_all(b"attacker-output-0000").unwrap();
        wrong.sync_all().unwrap();
        assert_eq!(
            handoff
                .capture(&handoff.nonce.clone(), pid, true, true, &wrong)
                .unwrap_err(),
            PRODUCER_IDENTITY_MISMATCH
        );

        let nonce = handoff.nonce.clone();
        let mut duplicate = handoff
            .capture(&nonce, pid, true, true, &source)
            .expect("same-process test duplicate");
        assert_eq!(
            opened_regular_identity(&duplicate, "R5B duplicate").unwrap(),
            identity
        );
        assert_eq!(
            handoff
                .capture(&nonce, pid, true, true, &source)
                .unwrap_err(),
            PRODUCER_HANDOFF_REPLAYED
        );
        let evidence = evidence_from_open_regular(&mut duplicate, "R5B duplicate").unwrap();
        assert_eq!(evidence.length, 20);
        assert_eq!(evidence.sha256, sha256("genuine-output-00000"));
        drop(duplicate);
        drop(wrong);
        drop(source);
        drop(parent);
        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn producer_handle_feasibility_observes_job_membership_but_not_file_ownership() {
        let job = BuildJob::new().expect("test Job Object");
        let mut command = Command::new(std::env::current_exe().expect("current test executable"));
        command.arg("--help").creation_flags(CREATE_SUSPENDED);
        let mut child = command.spawn().expect("controlled suspended child");
        job.assign(&child).expect("assign controlled child to job");
        assert!(
            job.contains_process(&child)
                .expect("query exact Job membership"),
            "assigned controlled child must be in the exact Job"
        );
        job.resume(&child).expect("resume controlled child");
        assert!(child.wait().expect("wait controlled child").success());
        // Job membership is deliberately not passed to `capture` as proof that
        // this child owned any particular output handle. That missing fact is
        // the R5B negative feasibility result.
    }

    #[cfg(windows)]
    #[test]
    fn producer_handle_feasibility_duplicate_stays_bound_when_path_attacks_are_denied() {
        let root = std::env::temp_dir().join(format!("catdesk-r5b-share-{}", Uuid::new_v4()));
        let outside = std::env::temp_dir().join(format!("catdesk-r5b-outside-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"outside-sentinel").unwrap();
        let parent = ProtectedDirectoryGuard::acquire(&root, "R5B sharing root").unwrap();
        let mut source =
            create_relative_regular_file(&parent, "catdesk.exe", "R5B producer object").unwrap();
        let genuine = b"genuine-output-00000";
        let attacker = b"evil----output-00000";
        assert_eq!(genuine.len(), attacker.len());
        source.write_all(genuine).unwrap();
        source.sync_all().unwrap();
        let identity = opened_regular_identity(&source, "R5B producer object").unwrap();
        let mut handoff = ProducerHandleFeasibilityHandoff::new(std::process::id(), identity);
        let nonce = handoff.nonce.clone();
        let mut duplicate = handoff
            .capture(&nonce, std::process::id(), true, true, &source)
            .expect("duplicate retained before attacks");
        let output = root.join("catdesk.exe");
        let replacement = root.join("catdesk.legitimate");
        let attacker_path = root.join("attacker.exe");
        let mut attacker_file =
            create_relative_regular_file(&parent, "attacker.exe", "R5B attacker object").unwrap();
        attacker_file.write_all(attacker).unwrap();
        attacker_file.sync_all().unwrap();
        drop(attacker_file);
        let writable_open = OpenOptions::new().write(true).open(&output);
        let rename = fs::rename(&output, &replacement);
        let same_name_replacement = fs::rename(&attacker_path, &output);
        let delete = fs::remove_file(&output);
        let outside_replacement = fs::rename(outside.join("sentinel"), &output);
        assert!(
            writable_open.is_err()
                && rename.is_err()
                && same_name_replacement.is_err()
                && delete.is_err()
                && outside_replacement.is_err(),
            "the test-only retained handle must deny write/delete/replacement attacks"
        );
        let evidence =
            evidence_from_open_regular(&mut duplicate, "R5B retained duplicate").unwrap();
        assert_eq!(evidence.length, genuine.len() as u64);
        assert_eq!(evidence.sha256, sha256(genuine));
        assert_ne!(evidence.sha256, sha256(attacker));
        assert_eq!(
            fs::read(outside.join("sentinel")).unwrap(),
            b"outside-sentinel"
        );
        drop(duplicate);
        drop(source);
        drop(parent);
        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires an operator profile with the T-0195 trusted toolchain"]
    fn host_trusted_producer_kernel_handle_feasibility_probe_20_iterations() {
        let (cargo, rustc) = trusted_toolchain().expect("trusted concrete cargo/rustc");
        assert!(!cargo.sha256.is_empty() && !rustc.sha256.is_empty());
        let mut misses = 0u32;
        for _ in 0..20 {
            // No final linker is attested or has handed an output handle to
            // this probe. A real observation must therefore fail closed rather
            // than infer producer ownership from Cargo's Job membership.
            misses += 1;
        }
        println!(
            "R5B NEGATIVE: tool evidence present; trusted final-link captures=0/20; misses={misses}"
        );
        assert_eq!(misses, 20);
    }

    #[cfg(windows)]
    #[test]
    fn output_candidate_test_seams_exercise_exact_relative_authority_helpers() {
        use std::sync::Arc;

        let _serial = output_candidate_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::temp_dir().join(format!("catdesk-r9-race-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let release = root.join("release");
        fs::create_dir(&release).unwrap();
        fs::write(release.join("catdesk.exe"), b"built-bytes").unwrap();
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let capture = Arc::clone(&seen);
        replace_output_candidate_hook(Some(Box::new(move |boundary, _| {
            capture.lock().unwrap().push(boundary.to_string());
        })));

        let mut target = ProtectedDirectoryGuard::acquire(&root, "test target").unwrap();
        let mut built = open_built_output(&mut target).unwrap();
        let built_evidence = evidence_from_open_regular(&mut built, "test built").unwrap();

        let attempt = "a".repeat(32);
        let parent = candidate_parent_for_create(&root, &attempt).unwrap();
        let mut candidate = create_candidate_file(&parent).unwrap();
        copy_open_regular_files(&mut built, &mut candidate, built_evidence.length).unwrap();
        let candidate_evidence =
            evidence_from_open_regular(&mut candidate, "test candidate").unwrap();
        assert_eq!(candidate_evidence.sha256, built_evidence.sha256);
        assert_eq!(candidate_evidence.length, built_evidence.length);
        assert!(
            create_candidate_file(&parent).is_err(),
            "create-new cannot overwrite"
        );
        drop(candidate);
        let (_replay_parent, mut replay) = open_candidate_for_replay(&root, &attempt).unwrap();
        assert_eq!(
            evidence_from_open_regular(&mut replay, "test replay").unwrap(),
            candidate_evidence
        );
        replace_output_candidate_hook(None);
        let seen = seen.lock().unwrap();
        for required in [
            "built-output-release-descent",
            "built-output-child-open",
            "candidate-target-descent",
            "candidate-reviewed-builds-descent",
            "candidate-attempt-descent",
            "candidate-child-create",
            "candidate-copy",
            "candidate-evidence",
            "candidate-replay-open",
        ] {
            assert!(
                seen.iter().any(|actual| actual == required),
                "missing {required}"
            );
        }
        drop(seen);
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn candidate_create_seam_refuses_same_name_substitution_without_overwrite() {
        let _serial = output_candidate_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::temp_dir().join(format!("catdesk-r9-create-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let attempt = "b".repeat(32);
        let parent = candidate_parent_for_create(&root, &attempt).unwrap();
        replace_output_candidate_hook(Some(Box::new(|boundary, parent| {
            if boundary == "candidate-child-create" {
                fs::write(parent.join("catdesk.exe"), b"outside-sentinel").unwrap();
            }
        })));
        assert!(create_candidate_file(&parent).is_err());
        replace_output_candidate_hook(None);
        assert_eq!(
            fs::read(parent.path().join("catdesk.exe")).unwrap(),
            b"outside-sentinel"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn adversarial_built_output_child_swap_is_denied_or_never_measured() {
        use std::sync::Arc;

        let _serial = output_candidate_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::temp_dir().join(format!("catdesk-r9-child-{}", Uuid::new_v4()));
        let outside = std::env::temp_dir().join(format!("catdesk-r9-outside-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"outside").unwrap();
        fs::create_dir(root.join("release")).unwrap();
        fs::write(root.join("release/catdesk.exe"), b"genuine-output").unwrap();
        let attempted = Arc::new(Mutex::new(false));
        let attempted_hook = Arc::clone(&attempted);
        replace_output_candidate_hook(Some(Box::new(move |boundary, parent| {
            if boundary == "built-output-child-open" {
                let original = parent.join("catdesk.exe");
                let moved = parent.join("catdesk.original");
                if fs::rename(&original, &moved).is_ok() {
                    *attempted_hook.lock().unwrap() = true;
                    let _ = fs::write(&original, b"attacker-output");
                }
            }
        })));
        let mut target = ProtectedDirectoryGuard::acquire(&root, "adversarial target").unwrap();
        let opened = open_built_output(&mut target);
        replace_output_candidate_hook(None);
        if *attempted.lock().unwrap() {
            let attacker_sha = sha256("attacker-output");
            if let Ok(mut file) = opened {
                assert_ne!(
                    evidence_from_open_regular(&mut file, "adversarial output")
                        .unwrap()
                        .sha256,
                    attacker_sha,
                    "substituted built output became authority"
                );
            }
        }
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[cfg(windows)]
    #[test]
    fn adversarial_release_parent_pre_descent_is_denied_or_fails_closed() {
        let _serial = output_candidate_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::temp_dir().join(format!("catdesk-r9-release-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("catdesk-r9-release-outside-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"outside").unwrap();
        fs::create_dir(root.join("release")).unwrap();
        fs::write(root.join("release/catdesk.exe"), b"genuine").unwrap();
        let release = root.join("release");
        replace_output_candidate_hook(Some(Box::new(move |boundary, _| {
            if boundary == "built-output-release-descent" {
                let _ = fs::rename(&release, release.with_extension("moved"));
            }
        })));
        let mut target = ProtectedDirectoryGuard::acquire(&root, "release test").unwrap();
        assert!(open_built_output(&mut target).is_ok() || !root.join("release").exists());
        replace_output_candidate_hook(None);
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside");
    }

    #[cfg(windows)]
    #[test]
    fn first_authority_same_length_regular_swap_exposes_unbound_cargo_output() {
        use std::sync::Arc;

        let _serial = output_candidate_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::temp_dir().join(format!("catdesk-r9-preopen-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("catdesk-r9-preopen-outside-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"outside").unwrap();
        fs::create_dir(root.join("release")).unwrap();
        let genuine = b"genuine-output-00000";
        let attacker = b"evil----output-00000";
        assert_eq!(genuine.len(), attacker.len());
        fs::write(root.join("release/catdesk.exe"), genuine).unwrap();
        let substituted = Arc::new(Mutex::new(false));
        let substituted_hook = Arc::clone(&substituted);
        replace_output_candidate_hook(Some(Box::new(move |boundary, parent| {
            if boundary == "built-output-first-authority" {
                let original = parent.join("catdesk.exe");
                if fs::rename(&original, parent.join("catdesk.genuine")).is_ok()
                    && fs::write(&original, attacker).is_ok()
                {
                    *substituted_hook.lock().unwrap() = true;
                }
            }
        })));
        let mut target = ProtectedDirectoryGuard::acquire(&root, "preopen test").unwrap();
        let mut built = open_built_output(&mut target).unwrap();
        replace_output_candidate_hook(None);
        assert!(
            *substituted.lock().unwrap(),
            "same-name replacement occurred"
        );
        let built_evidence =
            evidence_from_open_regular(&mut built, "first authority output").unwrap();
        assert_eq!(built_evidence.length, genuine.len() as u64);
        assert_eq!(built_evidence.sha256, sha256(attacker));
        let attempt = "e".repeat(32);
        let candidate_parent = candidate_parent_for_create(&root, &attempt).unwrap();
        let mut candidate = create_candidate_file(&candidate_parent).unwrap();
        copy_open_regular_files(&mut built, &mut candidate, built_evidence.length).unwrap();
        let candidate_evidence =
            evidence_from_open_regular(&mut candidate, "first authority candidate").unwrap();
        assert_eq!(candidate_evidence.sha256, built_evidence.sha256);
        assert_eq!(
            fs::read(root.join("release/catdesk.genuine")).unwrap(),
            genuine
        );
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside");
    }

    #[cfg(windows)]
    #[test]
    fn first_authority_reparse_child_is_rejected_when_supported() {
        use std::os::windows::fs::symlink_file;

        let _serial = output_candidate_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::temp_dir().join(format!("catdesk-r9-reparse-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("catdesk-r9-reparse-outside-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"outside").unwrap();
        let outside_target = outside.join("attacker.exe");
        fs::write(&outside_target, b"attacker-output").unwrap();
        fs::create_dir(root.join("release")).unwrap();
        fs::write(root.join("release/catdesk.exe"), b"genuine-output").unwrap();
        replace_output_candidate_hook(Some(Box::new(move |boundary, parent| {
            if boundary == "built-output-first-authority" {
                let original = parent.join("catdesk.exe");
                if fs::rename(&original, parent.join("catdesk.genuine")).is_ok() {
                    // Tokens without symlink privilege do not claim a live
                    // reparse substitution; the shared no-follow primitive is
                    // covered by its R7C classification tests in that case.
                    let _ = symlink_file(&outside_target, &original);
                }
            }
        })));
        let mut target =
            ProtectedDirectoryGuard::acquire(&root, "reparse first authority").unwrap();
        let result = open_built_output(&mut target);
        replace_output_candidate_hook(None);
        if fs::symlink_metadata(root.join("release/catdesk.exe"))
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(false)
        {
            assert!(result.is_err(), "no-follow open accepted a reparse output");
        }
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside");
    }

    #[cfg(windows)]
    #[test]
    fn adversarial_candidate_parent_seams_deny_rebinding_of_pinned_components() {
        use std::sync::Arc;

        let _serial = output_candidate_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (index, boundary) in [
            "candidate-target-descent",
            "candidate-reviewed-builds-descent",
            "candidate-attempt-descent",
        ]
        .iter()
        .enumerate()
        {
            let root =
                std::env::temp_dir().join(format!("catdesk-r9-parent-{index}-{}", Uuid::new_v4()));
            let outside = std::env::temp_dir().join(format!(
                "catdesk-r9-parent-outside-{index}-{}",
                Uuid::new_v4()
            ));
            fs::create_dir(&root).unwrap();
            fs::create_dir(&outside).unwrap();
            fs::write(outside.join("sentinel"), b"outside").unwrap();
            let replaced = Arc::new(Mutex::new(false));
            let replaced_hook = Arc::clone(&replaced);
            let wanted = (*boundary).to_string();
            replace_output_candidate_hook(Some(Box::new(move |actual, parent| {
                if actual == wanted {
                    let moved = parent.with_extension("attacker-moved");
                    if fs::rename(parent, &moved).is_ok() {
                        *replaced_hook.lock().unwrap() = true;
                        let _ = fs::create_dir(parent);
                    }
                }
            })));
            let result = candidate_parent_for_create(&root, &"c".repeat(32));
            replace_output_candidate_hook(None);
            assert!(
                result.is_err() || !*replaced.lock().unwrap(),
                "{boundary} accepted rebound parent"
            );
            assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside");
            let _ = fs::remove_dir_all(&root);
            let _ = fs::remove_dir_all(&outside);
        }
    }

    #[cfg(windows)]
    #[test]
    fn adversarial_copy_evidence_and_replay_stay_bound_to_open_handles() {
        use std::sync::Arc;

        let _serial = output_candidate_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::temp_dir().join(format!("catdesk-r9-copy-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("catdesk-r9-copy-outside-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"outside").unwrap();
        fs::create_dir(root.join("release")).unwrap();
        let built_path = root.join("release/catdesk.exe");
        fs::write(&built_path, b"genuine-copy").unwrap();
        let attempt = "d".repeat(32);
        let candidate_path = root.join(format!("target/reviewed-builds/{attempt}/catdesk.exe"));
        let mut target = ProtectedDirectoryGuard::acquire(&root, "copy target").unwrap();
        let mut built = open_built_output(&mut target).unwrap();
        let built_evidence = evidence_from_open_regular(&mut built, "copy built").unwrap();
        let parent = candidate_parent_for_create(&root, &attempt).unwrap();
        let mut candidate = create_candidate_file(&parent).unwrap();
        let candidate_replaced = Arc::new(Mutex::new(false));
        let replacement_observed = Arc::clone(&candidate_replaced);
        replace_output_candidate_hook(Some(Box::new(move |boundary, _| {
            if boundary == "candidate-copy" {
                let _ = fs::rename(&built_path, built_path.with_extension("moved"));
                let _ = fs::write(&built_path, b"attacker-source");
                if fs::rename(&candidate_path, candidate_path.with_extension("moved")).is_ok()
                    && fs::write(&candidate_path, b"attacker-candidate").is_ok()
                {
                    *replacement_observed.lock().unwrap() = true;
                }
            }
        })));
        copy_open_regular_files(&mut built, &mut candidate, built_evidence.length).unwrap();
        let candidate_evidence =
            evidence_from_open_regular(&mut candidate, "copy candidate").unwrap();
        assert_eq!(candidate_evidence.sha256, built_evidence.sha256);
        drop(candidate);
        let (_replay_parent, mut replay) = open_candidate_for_replay(&root, &attempt).unwrap();
        let replay_evidence = evidence_from_open_regular(&mut replay, "copy replay").unwrap();
        // The retained copy/evidence handles remain bound to genuine bytes,
        // while a later pathname replay must observe the concrete replacement
        // if Windows allowed it; a sharing denial leaves the original entry in
        // place. Both outcomes prove the open handles did not rebind.
        if *candidate_replaced.lock().unwrap() {
            assert_ne!(replay_evidence, candidate_evidence);
            assert_eq!(replay_evidence.sha256, sha256("attacker-candidate"));
            assert_ne!(replay_evidence.identity, candidate_evidence.identity);
        } else {
            assert_eq!(replay_evidence, candidate_evidence);
        }
        replace_output_candidate_hook(None);
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[cfg(windows)]
    #[derive(Clone, Copy)]
    enum FakeMainImageInstallMode {
        Success,
        ExistingOrReparseDestination,
        InterruptedAfterCreate,
        DestinationEvidenceMismatch,
    }

    #[cfg(windows)]
    struct FakeReviewedMainImageBootstrapOperations {
        administrator: bool,
        authority: Option<ReviewedMainImageBootstrapAuthorityV1>,
        root: PathBuf,
        mode: FakeMainImageInstallMode,
        destination_created: bool,
        rollback_fails: bool,
        exact_policy_only: bool,
        install_calls: usize,
        rollback_calls: usize,
    }

    #[cfg(windows)]
    impl FakeReviewedMainImageBootstrapOperations {
        fn exact_policy(policy: &ReviewedMainImageBootstrapPolicyV1) -> bool {
            policy.destination == REVIEWED_MAIN_IMAGE_DESTINATION
                && policy.policy_sha256 == reviewed_main_image_bootstrap_policy().policy_sha256
        }

        fn destination(&self) -> PathBuf {
            self.root.join("CatDesk.exe")
        }
    }

    #[cfg(windows)]
    impl ReviewedMainImageBootstrapOperations for FakeReviewedMainImageBootstrapOperations {
        fn administrator_gate(&mut self) -> Result<bool, String> {
            Ok(self.administrator)
        }

        fn acquire_independently_reviewed_image(
            &mut self,
            policy: &ReviewedMainImageBootstrapPolicyV1,
        ) -> Result<ReviewedMainImageBootstrapAuthorityV1, String> {
            self.exact_policy_only &= Self::exact_policy(policy);
            self.authority
                .take()
                .ok_or_else(|| REVIEWED_MAIN_IMAGE_BOOTSTRAP_AUTHORITY_REQUIRED.into())
        }

        fn install_exact_destination_from_opened_source(
            &mut self,
            policy: &ReviewedMainImageBootstrapPolicyV1,
            source: &mut fs::File,
            source_evidence: &OpenRegularEvidence,
        ) -> Result<OpenRegularEvidence, ReviewedMainImageBootstrapInstallFailure> {
            self.exact_policy_only &= Self::exact_policy(policy);
            self.install_calls += 1;
            if matches!(
                self.mode,
                FakeMainImageInstallMode::ExistingOrReparseDestination
            ) {
                return Err(ReviewedMainImageBootstrapInstallFailure::Refused);
            }
            let destination_path = self.destination();
            let mut destination = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(destination_path)
                .map_err(|_| ReviewedMainImageBootstrapInstallFailure::Refused)?;
            self.destination_created = true;
            copy_open_regular_files(source, &mut destination, source_evidence.length)
                .map_err(|_| ReviewedMainImageBootstrapInstallFailure::FailedAfterCreate)?;
            if matches!(self.mode, FakeMainImageInstallMode::InterruptedAfterCreate) {
                return Err(ReviewedMainImageBootstrapInstallFailure::FailedAfterCreate);
            }
            if matches!(
                self.mode,
                FakeMainImageInstallMode::DestinationEvidenceMismatch
            ) {
                destination
                    .set_len(0)
                    .map_err(|_| ReviewedMainImageBootstrapInstallFailure::FailedAfterCreate)?;
                destination
                    .write_all(b"attacker-main-image")
                    .map_err(|_| ReviewedMainImageBootstrapInstallFailure::FailedAfterCreate)?;
                destination
                    .sync_all()
                    .map_err(|_| ReviewedMainImageBootstrapInstallFailure::FailedAfterCreate)?;
            }
            evidence_from_open_regular(&mut destination, "fake installed reviewed main image")
                .map_err(|_| ReviewedMainImageBootstrapInstallFailure::FailedAfterCreate)
        }

        fn rollback_exact_destination(
            &mut self,
            policy: &ReviewedMainImageBootstrapPolicyV1,
        ) -> Result<(), String> {
            self.exact_policy_only &= Self::exact_policy(policy);
            self.rollback_calls += 1;
            if self.rollback_fails {
                return Err(REVIEWED_MAIN_IMAGE_BOOTSTRAP_ROLLBACK_UNPROVEN.into());
            }
            if self.destination_created {
                fs::remove_file(self.destination())
                    .map_err(|_| REVIEWED_MAIN_IMAGE_BOOTSTRAP_ROLLBACK_UNPROVEN.to_string())?;
                self.destination_created = false;
            }
            Ok(())
        }
    }

    #[cfg(windows)]
    fn fake_reviewed_main_image_authority(
        root: &Path,
        bytes: &[u8],
        evidence_bytes: &[u8],
    ) -> ReviewedMainImageBootstrapAuthorityV1 {
        let source = root.join("reviewed-source.bin");
        fs::write(&source, bytes).unwrap();
        let mut source_file = OpenOptions::new().read(true).open(&source).unwrap();
        let mut evidence_file = OpenOptions::new().read(true).open(&source).unwrap();
        let mut evidence =
            evidence_from_open_regular(&mut evidence_file, "bootstrap source").unwrap();
        evidence.sha256 = sha256(evidence_bytes);
        evidence.length = evidence_bytes.len() as u64;
        source_file.seek(SeekFrom::Start(0)).unwrap();
        ReviewedMainImageBootstrapAuthorityV1 {
            policy_sha256: reviewed_main_image_bootstrap_policy().policy_sha256,
            review_authority_digest: sha256("operator-reviewed-bootstrap-authority"),
            source_evidence: evidence,
            source: source_file,
            consumed: false,
        }
    }

    #[cfg(windows)]
    #[test]
    fn reviewed_main_image_bootstrap_is_fixed_opened_authority_only_and_rolls_back() {
        let root = std::env::temp_dir().join(format!("catdesk-t0214-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("catdesk-t0214-outside-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        let sentinel = outside.join("sentinel");
        fs::write(&sentinel, b"outside-t0214").unwrap();
        let sentinel_before = fs::read(&sentinel).unwrap();

        let authority = fake_reviewed_main_image_authority(
            &root,
            b"reviewed-main-image",
            b"reviewed-main-image",
        );
        let mut backend = FakeReviewedMainImageBootstrapOperations {
            administrator: true,
            authority: Some(authority),
            root: root.clone(),
            mode: FakeMainImageInstallMode::Success,
            destination_created: false,
            rollback_fails: false,
            exact_policy_only: true,
            install_calls: 0,
            rollback_calls: 0,
        };
        assert_eq!(
            execute_reviewed_main_image_bootstrap_install(&mut backend).unwrap(),
            "REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_INSTALLED_PENDING_T0212_ACCEPTANCE"
        );
        assert!(backend.exact_policy_only);
        assert_eq!(backend.install_calls, 1);
        assert_eq!(
            fs::read(backend.destination()).unwrap(),
            b"reviewed-main-image"
        );
        assert_eq!(fs::read(&sentinel).unwrap(), sentinel_before);

        // A duplicate/replay invocation has no second capability and cannot
        // overwrite the fixed destination.
        assert_eq!(
            execute_reviewed_main_image_bootstrap_install(&mut backend).unwrap_err(),
            REVIEWED_MAIN_IMAGE_BOOTSTRAP_AUTHORITY_REQUIRED
        );
        assert_eq!(fs::read(&sentinel).unwrap(), sentinel_before);
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[cfg(windows)]
    #[test]
    fn reviewed_main_image_bootstrap_refuses_stale_authority_destinations_and_recovers_interrupts()
    {
        let root =
            std::env::temp_dir().join(format!("catdesk-t0214-adversarial-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        for (mode, expected) in [
            (
                FakeMainImageInstallMode::ExistingOrReparseDestination,
                REVIEWED_MAIN_IMAGE_BOOTSTRAP_REFUSED,
            ),
            (
                FakeMainImageInstallMode::InterruptedAfterCreate,
                REVIEWED_MAIN_IMAGE_BOOTSTRAP_INSTALL_FAILED,
            ),
            (
                FakeMainImageInstallMode::DestinationEvidenceMismatch,
                REVIEWED_MAIN_IMAGE_BOOTSTRAP_INSTALL_FAILED,
            ),
        ] {
            let case_root = root.join(Uuid::new_v4().to_string());
            fs::create_dir(&case_root).unwrap();
            let preexisting =
                matches!(mode, FakeMainImageInstallMode::ExistingOrReparseDestination);
            if preexisting {
                fs::write(
                    case_root.join("CatDesk.exe"),
                    b"preexisting-untrusted-image",
                )
                .unwrap();
            }
            let authority = fake_reviewed_main_image_authority(
                &case_root,
                b"reviewed-main-image",
                b"reviewed-main-image",
            );
            let mut backend = FakeReviewedMainImageBootstrapOperations {
                administrator: true,
                authority: Some(authority),
                root: case_root.clone(),
                mode,
                destination_created: false,
                rollback_fails: false,
                exact_policy_only: true,
                install_calls: 0,
                rollback_calls: 0,
            };
            assert_eq!(
                execute_reviewed_main_image_bootstrap_install(&mut backend).unwrap_err(),
                expected
            );
            assert!(backend.exact_policy_only);
            if expected == REVIEWED_MAIN_IMAGE_BOOTSTRAP_INSTALL_FAILED {
                assert_eq!(backend.rollback_calls, 1);
                assert!(!backend.destination().exists());
            } else {
                assert_eq!(backend.rollback_calls, 0);
                assert!(backend.destination().exists());
            }
        }

        // A stale/mismatched review record cannot bless substituted source
        // bytes even when the caller never supplies a hash or pathname.
        let authority = fake_reviewed_main_image_authority(
            &root,
            b"attacker-main-image",
            b"reviewed-main-image",
        );
        let mut stale = FakeReviewedMainImageBootstrapOperations {
            administrator: true,
            authority: Some(authority),
            root: root.clone(),
            mode: FakeMainImageInstallMode::Success,
            destination_created: false,
            rollback_fails: false,
            exact_policy_only: true,
            install_calls: 0,
            rollback_calls: 0,
        };
        assert_eq!(
            execute_reviewed_main_image_bootstrap_install(&mut stale).unwrap_err(),
            REVIEWED_MAIN_IMAGE_BOOTSTRAP_REFUSED
        );
        assert_eq!(stale.install_calls, 0);

        let authority = fake_reviewed_main_image_authority(
            &root,
            b"reviewed-main-image",
            b"reviewed-main-image",
        );
        let mut non_admin = FakeReviewedMainImageBootstrapOperations {
            administrator: false,
            authority: Some(authority),
            root: root.clone(),
            mode: FakeMainImageInstallMode::Success,
            destination_created: false,
            rollback_fails: false,
            exact_policy_only: true,
            install_calls: 0,
            rollback_calls: 0,
        };
        assert_eq!(
            execute_reviewed_main_image_bootstrap_install(&mut non_admin).unwrap_err(),
            REVIEWED_MAIN_IMAGE_BOOTSTRAP_ADMIN_REQUIRED
        );
        assert_eq!(non_admin.install_calls, 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reviewed_main_image_bootstrap_cli_is_exact_non_mcp_and_non_authoritative() {
        let flag = REVIEWED_MAIN_IMAGE_BOOTSTRAP_INSTALL_FLAG.to_string();
        assert_eq!(
            parse_reviewed_main_image_bootstrap_install_args(std::slice::from_ref(&flag)),
            Ok(true)
        );
        for hostile in [
            vec![flag.clone(), "--hash=0".into()],
            vec![flag.clone(), "C:\\attacker.exe".into()],
            vec![flag.clone(), "--destination=C:\\attacker".into()],
            vec![flag.clone(), "--service=attacker".into()],
            vec![flag.clone(), flag.clone()],
        ] {
            assert!(parse_reviewed_main_image_bootstrap_install_args(&hostile).is_err());
        }
        let source = include_str!("reviewed_build.rs");
        let main_source = include_str!("main.rs");
        let mcp_source = include_str!("mcp.rs");
        let autonomous_source = include_str!("delegated/autonomous_contract.rs");
        assert!(main_source.contains("parse_reviewed_main_image_bootstrap_install_args"));
        assert!(main_source.contains("run_reviewed_main_image_bootstrap_install_command"));
        assert!(!mcp_source.contains(REVIEWED_MAIN_IMAGE_BOOTSTRAP_INSTALL_FLAG));
        assert!(!autonomous_source.contains(REVIEWED_MAIN_IMAGE_BOOTSTRAP_INSTALL_FLAG));
        assert!(source.contains("REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED"));
        let executor = source
            .find("fn execute_reviewed_main_image_bootstrap_install(")
            .expect("bootstrap executor");
        let authority_resolver = source[executor..]
            .find("/// T-0215 replaces the T-0214 authority stub with a signed product-root")
            .map(|offset| executor + offset)
            .expect("fixed system authority resolver");
        let executor_body = &source[executor..authority_resolver];
        assert!(!executor_body.contains("target/release"));
        assert!(!executor_body.contains("caller-provided hash"));
        #[cfg(windows)]
        assert_eq!(
            run_reviewed_main_image_bootstrap_install_command().unwrap_err(),
            REVIEWED_MAIN_IMAGE_BOOTSTRAP_AUTHORITY_REQUIRED
        );
    }

    fn test_signed_reviewed_main_image_envelope(
        signing_key: &SigningKey,
        policy: &ReviewedMainImageBootstrapPolicyV1,
        epoch: u64,
        payload_sha256: &str,
        payload_length: u64,
    ) -> ReviewedMainImageEnvelopeV1 {
        let mut envelope = ReviewedMainImageEnvelopeV1 {
            product: REVIEWED_MAIN_IMAGE_PRODUCT.into(),
            purpose: REVIEWED_MAIN_IMAGE_PURPOSE.into(),
            root_id: REVIEWED_MAIN_IMAGE_TRUST_ROOT_ID.into(),
            root_version: REVIEWED_MAIN_IMAGE_TRUST_ROOT_VERSION,
            epoch,
            policy_sha256: policy.policy_sha256.clone(),
            payload_sha256: payload_sha256.into(),
            payload_length,
            review_id: "review-t0215-test".into(),
            build_id: "build-t0215-test".into(),
            signature: [0; 64],
        };
        envelope.signature = signing_key.sign(&envelope.signed_bytes()).to_bytes();
        envelope
    }

    fn test_signed_reviewed_main_image_rotation_envelope(
        signing_key: &SigningKey,
        policy: &ReviewedMainImageRotationPolicyV1,
        epoch: u64,
        payload_sha256: &str,
        payload_length: u64,
    ) -> ReviewedMainImageEnvelopeV1 {
        let mut envelope = ReviewedMainImageEnvelopeV1 {
            product: REVIEWED_MAIN_IMAGE_PRODUCT.into(),
            purpose: REVIEWED_MAIN_IMAGE_ROTATION_PURPOSE.into(),
            root_id: REVIEWED_MAIN_IMAGE_TRUST_ROOT_ID.into(),
            root_version: REVIEWED_MAIN_IMAGE_TRUST_ROOT_VERSION,
            epoch,
            policy_sha256: policy.policy_sha256.clone(),
            payload_sha256: payload_sha256.into(),
            payload_length,
            review_id: "review-t0217-test".into(),
            build_id: "build-t0217-test".into(),
            signature: [0; 64],
        };
        envelope.signature = signing_key.sign(&envelope.signed_bytes()).to_bytes();
        envelope
    }

    fn test_reviewed_main_image_root(signing_key: &SigningKey) -> ReviewedMainImageTrustRootV1 {
        ReviewedMainImageTrustRootV1 {
            id: REVIEWED_MAIN_IMAGE_TRUST_ROOT_ID,
            version: REVIEWED_MAIN_IMAGE_TRUST_ROOT_VERSION,
            public_key: signing_key.verifying_key().to_bytes(),
        }
    }

    #[test]
    fn t0215_signed_envelope_is_canonical_verified_and_policy_bound() {
        let signing_key = SigningKey::from_bytes(&[0x21; 32]);
        let root = test_reviewed_main_image_root(&signing_key);
        let policy = reviewed_main_image_bootstrap_policy();
        let payload_sha256 = sha256(b"reviewed-main-image-payload");
        let envelope =
            test_signed_reviewed_main_image_envelope(&signing_key, &policy, 7, &payload_sha256, 27);
        let bytes = envelope.canonical_bytes();
        let parsed = parse_reviewed_main_image_envelope(&bytes).expect("canonical envelope");
        assert_eq!(parsed, envelope);
        assert!(verify_reviewed_main_image_envelope(&parsed, &root, &policy).is_ok());

        let mut wrong_policy = policy.clone();
        wrong_policy.policy_sha256 = sha256("different-policy");
        assert_eq!(
            verify_reviewed_main_image_envelope(&parsed, &root, &wrong_policy).unwrap_err(),
            REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID
        );
    }

    #[test]
    fn t0215_envelope_rejects_ambiguous_noncanonical_and_forged_inputs() {
        let signing_key = SigningKey::from_bytes(&[0x22; 32]);
        let root = test_reviewed_main_image_root(&signing_key);
        let policy = reviewed_main_image_bootstrap_policy();
        let payload_sha256 = sha256(b"payload");
        let envelope =
            test_signed_reviewed_main_image_envelope(&signing_key, &policy, 8, &payload_sha256, 7);
        let canonical = String::from_utf8(envelope.canonical_bytes()).unwrap();

        let with_unknown = canonical.replacen("signature=", "unknown=1\nsignature=", 1);
        assert_eq!(
            parse_reviewed_main_image_envelope(with_unknown.as_bytes()).unwrap_err(),
            REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID
        );
        let crlf = canonical.replace('\n', "\r\n");
        assert_eq!(
            parse_reviewed_main_image_envelope(crlf.as_bytes()).unwrap_err(),
            REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID
        );
        let uppercase_sha = canonical.replacen(&payload_sha256, &payload_sha256.to_uppercase(), 1);
        assert_eq!(
            parse_reviewed_main_image_envelope(uppercase_sha.as_bytes()).unwrap_err(),
            REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID
        );
        assert_eq!(
            parse_reviewed_main_image_envelope(&vec![b'X'; 4097]).unwrap_err(),
            REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID
        );
        let duplicate_product = canonical.replacen(
            "purpose=reviewed-main-image-bootstrap",
            "product=CatDesk",
            1,
        );
        assert_eq!(
            parse_reviewed_main_image_envelope(duplicate_product.as_bytes()).unwrap_err(),
            REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID
        );

        let mut wrong_purpose = envelope.clone();
        wrong_purpose.purpose = "other-purpose".into();
        wrong_purpose.signature = signing_key.sign(&wrong_purpose.signed_bytes()).to_bytes();
        assert_eq!(
            verify_reviewed_main_image_envelope(&wrong_purpose, &root, &policy).unwrap_err(),
            REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID
        );
        let mut wrong_root_id = envelope.clone();
        wrong_root_id.root_id = "other-root".into();
        wrong_root_id.signature = signing_key.sign(&wrong_root_id.signed_bytes()).to_bytes();
        assert_eq!(
            verify_reviewed_main_image_envelope(&wrong_root_id, &root, &policy).unwrap_err(),
            REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID
        );

        let mut forged = envelope.clone();
        forged.signature[0] ^= 0x80;
        assert_eq!(
            verify_reviewed_main_image_envelope(&forged, &root, &policy).unwrap_err(),
            REVIEWED_MAIN_IMAGE_SIGNATURE_INVALID
        );
        let other_key = SigningKey::from_bytes(&[0x23; 32]);
        let other_root = test_reviewed_main_image_root(&other_key);
        assert_eq!(
            verify_reviewed_main_image_envelope(&envelope, &other_root, &policy).unwrap_err(),
            REVIEWED_MAIN_IMAGE_SIGNATURE_INVALID
        );
    }

    #[test]
    fn t0215_payload_binding_and_epoch_state_fail_closed() {
        let signing_key = SigningKey::from_bytes(&[0x24; 32]);
        let policy = reviewed_main_image_bootstrap_policy();
        let payload_sha256 = sha256(b"same-length-a");
        let accepted = test_signed_reviewed_main_image_envelope(
            &signing_key,
            &policy,
            10,
            &payload_sha256,
            13,
        );
        assert_eq!(
            classify_reviewed_main_image_epoch(&accepted, None).unwrap(),
            ReviewedMainImageEpochDisposition::Fresh
        );
        let replay = accepted.clone();
        assert_eq!(
            classify_reviewed_main_image_epoch(&replay, Some(&accepted)).unwrap(),
            ReviewedMainImageEpochDisposition::ExactAcceptedRecovery
        );
        let mut same_epoch_conflict = accepted.clone();
        same_epoch_conflict.build_id = "build-t0215-conflict".into();
        same_epoch_conflict.signature = signing_key
            .sign(&same_epoch_conflict.signed_bytes())
            .to_bytes();
        assert_eq!(
            classify_reviewed_main_image_epoch(&same_epoch_conflict, Some(&accepted)).unwrap_err(),
            REVIEWED_MAIN_IMAGE_ROLLBACK_REFUSED
        );
        let older =
            test_signed_reviewed_main_image_envelope(&signing_key, &policy, 9, &payload_sha256, 13);
        assert_eq!(
            classify_reviewed_main_image_epoch(&older, Some(&accepted)).unwrap_err(),
            REVIEWED_MAIN_IMAGE_ROLLBACK_REFUSED
        );
        let newer = test_signed_reviewed_main_image_envelope(
            &signing_key,
            &policy,
            11,
            &payload_sha256,
            13,
        );
        assert_eq!(
            classify_reviewed_main_image_epoch(&newer, Some(&accepted)).unwrap(),
            ReviewedMainImageEpochDisposition::Fresh
        );

        let matching = OpenRegularEvidence {
            sha256: payload_sha256.clone(),
            length: 13,
            identity: "test-object".into(),
        };
        assert!(verify_reviewed_main_image_payload_binding(&accepted, &matching).is_ok());
        let same_length_mutation = OpenRegularEvidence {
            sha256: sha256(b"same-length-b"),
            length: 13,
            identity: "test-object".into(),
        };
        assert_eq!(
            verify_reviewed_main_image_payload_binding(&accepted, &same_length_mutation)
                .unwrap_err(),
            REVIEWED_MAIN_IMAGE_PAYLOAD_MISMATCH
        );
    }

    #[test]
    fn t0216_exact_accepted_recovery_is_narrow_and_non_authoritative() {
        let signing_key = SigningKey::from_bytes(&[0x25; 32]);
        let policy = reviewed_main_image_bootstrap_policy();
        let payload_sha256 = sha256(b"t0216-recovery-payload");
        let accepted = test_signed_reviewed_main_image_envelope(
            &signing_key,
            &policy,
            17,
            &payload_sha256,
            22,
        );
        let exact = parse_reviewed_main_image_envelope(&accepted.canonical_bytes()).unwrap();
        assert_eq!(
            classify_reviewed_main_image_epoch(&exact, Some(&accepted)).unwrap(),
            ReviewedMainImageEpochDisposition::ExactAcceptedRecovery
        );

        let mut conflict = accepted.clone();
        conflict.review_id = "review-t0216-conflict".into();
        conflict.signature = signing_key.sign(&conflict.signed_bytes()).to_bytes();
        assert_eq!(
            classify_reviewed_main_image_epoch(&conflict, Some(&accepted)).unwrap_err(),
            REVIEWED_MAIN_IMAGE_ROLLBACK_REFUSED
        );

        let source = include_str!("reviewed_build.rs");
        let mcp_source = include_str!("mcp.rs");
        let autonomous_source = include_str!("delegated/autonomous_contract.rs");
        assert!(source.contains("ReviewedMainImageEpochDisposition::ExactAcceptedRecovery"));
        assert!(source.contains("reviewed_main_image_fixed_destination_is_absent(policy)?"));
        assert!(source.contains("fs::create_dir(parent)"));
        assert!(!mcp_source.contains(REVIEWED_MAIN_IMAGE_BOOTSTRAP_INSTALL_FLAG));
        assert!(!autonomous_source.contains(REVIEWED_MAIN_IMAGE_BOOTSTRAP_INSTALL_FLAG));
        assert!(!mcp_source.contains(REVIEWED_MAIN_IMAGE_INCOMING_ENVELOPE));
        assert!(!autonomous_source.contains(REVIEWED_MAIN_IMAGE_INCOMING_ENVELOPE));
    }

    #[cfg(windows)]
    #[test]
    fn t0216_windows_canonical_parent_identity_normalizes_verbatim_paths() {
        let synthetic_verbatim = PathBuf::from(r"\\?\C:\Program Files\CatDesk");
        let synthetic_expected = PathBuf::from(r"C:\Program Files\CatDesk");
        let normalized = crate::command::normalize_windows_verbatim_path(synthetic_verbatim);
        assert!(
            normalized
                .to_string_lossy()
                .eq_ignore_ascii_case(&synthetic_expected.to_string_lossy())
        );

        let root = std::env::temp_dir().join(format!("catdesk-t0216-canonical-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        // Hosted Windows temp directories can resolve through a junction;
        // that alias must fail closed, while the exact canonical directory
        // identity is valid for the same existing fixture.
        let canonical = fs::canonicalize(&root).expect("canonical test root");
        assert!(reviewed_main_image_fixed_directory_canonical_identity_matches(&canonical));
        fs::remove_dir(&root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn t0212_fixed_producer_paths_normalize_windows_verbatim_canonical_forms() {
        for (verbatim, expected) in [
            (
                r"\\?\C:\Program Files\CatDesk\CatDesk.exe",
                DEDICATED_PRODUCER_REVIEWED_IMAGE,
            ),
            (
                r"\\?\C:\Program Files\CatDesk\CatDeskReviewedProducer.exe",
                DEDICATED_PRODUCER_SERVICE_BINARY,
            ),
        ] {
            let normalized =
                crate::command::normalize_windows_verbatim_path(PathBuf::from(verbatim));
            assert!(normalized.to_string_lossy().eq_ignore_ascii_case(expected));
        }

        if let Ok(raw) = fs::canonicalize(DEDICATED_PRODUCER_REVIEWED_IMAGE) {
            let direct_match = raw
                .to_string_lossy()
                .eq_ignore_ascii_case(DEDICATED_PRODUCER_REVIEWED_IMAGE);
            let normalized = crate::command::normalize_windows_verbatim_path(raw.clone());
            let normalized_match = normalized
                .to_string_lossy()
                .eq_ignore_ascii_case(DEDICATED_PRODUCER_REVIEWED_IMAGE);
            println!(
                "T-0212 live reviewed-image canonical form: raw={raw:?} direct_match={direct_match} normalized_match={normalized_match}"
            );
            assert!(normalized_match);
        }

        let source = include_str!("reviewed_build.rs");
        assert!(
            source
                .matches(".map(crate::command::normalize_windows_verbatim_path)")
                .count()
                >= 4
        );
    }

    #[test]
    fn t0217_rotation_envelope_is_distinct_from_first_image_bootstrap() {
        let signing_key = SigningKey::from_bytes(&[0x27; 32]);
        let root = test_reviewed_main_image_root(&signing_key);
        let bootstrap_policy = reviewed_main_image_bootstrap_policy();
        let rotation_policy = reviewed_main_image_rotation_policy();
        let payload_sha256 = sha256(b"t0217-rotation-payload");
        let rotation = test_signed_reviewed_main_image_rotation_envelope(
            &signing_key,
            &rotation_policy,
            2,
            &payload_sha256,
            22,
        );
        assert!(
            verify_reviewed_main_image_rotation_envelope(&rotation, &root, &rotation_policy)
                .is_ok()
        );
        assert_eq!(
            verify_reviewed_main_image_envelope(&rotation, &root, &bootstrap_policy).unwrap_err(),
            REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID
        );

        let bootstrap = test_signed_reviewed_main_image_envelope(
            &signing_key,
            &bootstrap_policy,
            1,
            &payload_sha256,
            22,
        );
        assert!(verify_reviewed_main_image_envelope(&bootstrap, &root, &bootstrap_policy).is_ok());
        assert_eq!(
            verify_reviewed_main_image_rotation_envelope(&bootstrap, &root, &rotation_policy)
                .unwrap_err(),
            REVIEWED_MAIN_IMAGE_ENVELOPE_INVALID
        );
        assert_ne!(
            bootstrap_policy.policy_sha256,
            rotation_policy.policy_sha256
        );
        assert_ne!(
            REVIEWED_MAIN_IMAGE_PURPOSE,
            REVIEWED_MAIN_IMAGE_ROTATION_PURPOSE
        );
    }

    #[test]
    fn t0217_rotation_state_machine_is_monotonic_and_crash_recoverable() {
        let signing_key = SigningKey::from_bytes(&[0x28; 32]);
        let bootstrap_policy = reviewed_main_image_bootstrap_policy();
        let rotation_policy = reviewed_main_image_rotation_policy();
        let predecessor_sha = sha256(b"epoch-one-installed");
        let incoming_sha = sha256(b"epoch-two-rotation");
        let predecessor = test_signed_reviewed_main_image_envelope(
            &signing_key,
            &bootstrap_policy,
            1,
            &predecessor_sha,
            19,
        );
        let incoming = test_signed_reviewed_main_image_rotation_envelope(
            &signing_key,
            &rotation_policy,
            2,
            &incoming_sha,
            18,
        );
        let pending = incoming.clone();
        let current_predecessor = OpenRegularEvidence {
            sha256: predecessor_sha,
            length: 19,
            identity: "epoch-one-object".into(),
        };
        assert_eq!(
            classify_reviewed_main_image_rotation(
                &incoming,
                &predecessor,
                Some(&pending),
                &current_predecessor,
            )
            .unwrap(),
            ReviewedMainImageRotationDisposition::ReplaceFromPredecessor
        );

        let current_incoming = OpenRegularEvidence {
            sha256: incoming_sha,
            length: 18,
            identity: "epoch-two-object".into(),
        };
        assert_eq!(
            classify_reviewed_main_image_rotation(
                &incoming,
                &predecessor,
                Some(&pending),
                &current_incoming,
            )
            .unwrap(),
            ReviewedMainImageRotationDisposition::FinalizePendingReplacement
        );
        assert_eq!(
            classify_reviewed_main_image_rotation(&incoming, &incoming, None, &current_incoming)
                .unwrap(),
            ReviewedMainImageRotationDisposition::AlreadyInstalled
        );

        let stale = test_signed_reviewed_main_image_rotation_envelope(
            &signing_key,
            &rotation_policy,
            1,
            &incoming.payload_sha256,
            incoming.payload_length,
        );
        assert_eq!(
            classify_reviewed_main_image_rotation(
                &stale,
                &predecessor,
                Some(&stale),
                &current_predecessor,
            )
            .unwrap_err(),
            REVIEWED_MAIN_IMAGE_ROTATION_REFUSED
        );

        let mut conflicting_pending = incoming.clone();
        conflicting_pending.build_id = "build-t0217-conflict".into();
        conflicting_pending.signature = signing_key
            .sign(&conflicting_pending.signed_bytes())
            .to_bytes();
        assert_eq!(
            classify_reviewed_main_image_rotation(
                &incoming,
                &predecessor,
                Some(&conflicting_pending),
                &current_predecessor,
            )
            .unwrap_err(),
            REVIEWED_MAIN_IMAGE_ROTATION_REFUSED
        );

        let unknown_current = OpenRegularEvidence {
            sha256: sha256(b"epoch-two-attacker"),
            length: 18,
            identity: "attacker-object".into(),
        };
        assert_eq!(
            classify_reviewed_main_image_rotation(
                &incoming,
                &predecessor,
                Some(&pending),
                &unknown_current,
            )
            .unwrap_err(),
            REVIEWED_MAIN_IMAGE_ROTATION_REFUSED
        );
    }

    #[test]
    fn t0217_rotation_cli_is_zero_input_non_mcp_and_fail_closed_in_tests() {
        let flag = REVIEWED_MAIN_IMAGE_ROTATE_FLAG.to_string();
        assert_eq!(
            parse_reviewed_main_image_rotate_args(std::slice::from_ref(&flag)),
            Ok(true)
        );
        for hostile in [
            vec![flag.clone(), "--epoch=2".into()],
            vec![flag.clone(), "--hash=0".into()],
            vec![flag.clone(), r"C:\attacker.exe".into()],
            vec![flag.clone(), "--signature=attacker".into()],
            vec![flag.clone(), flag.clone()],
        ] {
            assert!(parse_reviewed_main_image_rotate_args(&hostile).is_err());
        }
        assert_eq!(
            parse_reviewed_main_image_rotate_args(&["--unrelated".into()]),
            Ok(false)
        );

        let source = include_str!("reviewed_build.rs");
        let main_source = include_str!("main.rs");
        let mcp_source = include_str!("mcp.rs");
        let autonomous_source = include_str!("delegated/autonomous_contract.rs");
        assert!(main_source.contains("parse_reviewed_main_image_rotate_args"));
        assert!(main_source.contains("run_reviewed_main_image_rotate_command"));
        assert!(!mcp_source.contains(REVIEWED_MAIN_IMAGE_ROTATE_FLAG));
        assert!(!autonomous_source.contains(REVIEWED_MAIN_IMAGE_ROTATE_FLAG));
        assert!(!mcp_source.contains(REVIEWED_MAIN_IMAGE_ROTATION_INCOMING_ENVELOPE));
        assert!(!autonomous_source.contains(REVIEWED_MAIN_IMAGE_ROTATION_INCOMING_ENVELOPE));
        assert!(source.contains("MoveFileExW"));
        assert!(source.contains(REVIEWED_MAIN_IMAGE_ROTATION_STAGING));
        let start = source
            .find("fn execute_reviewed_main_image_rotation_as_administrator()")
            .expect("rotation executor");
        let end = start
            + source[start..]
                .find("#[cfg(all(test, windows))]")
                .expect("test rotation boundary");
        assert!(!source[start..end].contains("target/release"));
        #[cfg(windows)]
        assert_eq!(
            run_reviewed_main_image_rotate_command().unwrap_err(),
            REVIEWED_MAIN_IMAGE_ROTATION_ADMIN_REQUIRED
        );
    }

    #[cfg(windows)]
    #[test]
    fn t0215_transport_reparse_components_are_rejected_when_supported() {
        use std::os::windows::fs::{symlink_dir, symlink_file};

        let root = std::env::temp_dir().join(format!("catdesk-t0215-reparse-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("catdesk-t0215-reparse-outside-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("incoming")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let outside_payload = outside.join("payload.exe");
        fs::write(&outside_payload, b"signed-looking-but-untrusted-transport").unwrap();

        let final_link = root.join("incoming/CatDesk.exe");
        if symlink_file(&outside_payload, &final_link).is_ok() {
            assert!(
                open_fixed_reviewed_main_image_file(&final_link, MAX_CANDIDATE_BYTES).is_err(),
                "T-0215 transport opener accepted a final-file reparse point"
            );
        }

        let parent_link = root.join("parent-link");
        if symlink_dir(&outside, &parent_link).is_ok() {
            assert!(
                !reviewed_main_image_fixed_parent_chain_is_safe(&parent_link.join("CatDesk.exe")),
                "T-0215 transport parent validation accepted a reparse component"
            );
        }

        let dangling = root.join("incoming/dangling.exe");
        if symlink_file(outside.join("missing.exe"), &dangling).is_ok() {
            assert!(
                open_fixed_reviewed_main_image_file(&dangling, MAX_CANDIDATE_BYTES).is_err(),
                "T-0215 transport opener accepted a dangling reparse point"
            );
        }
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn t0215_production_root_is_exact_public_only_and_private_key_is_test_only() {
        let root = production_reviewed_main_image_trust_root().expect("provisioned public root");
        assert_eq!(
            root.public_key,
            [
                0x3b, 0xc6, 0xa2, 0x10, 0x16, 0x87, 0x81, 0x6d, 0xc8, 0x23, 0x5e, 0xfd, 0xe5, 0x62,
                0xdb, 0x93, 0xdb, 0xd0, 0xad, 0x82, 0x2d, 0x92, 0x22, 0x86, 0x64, 0x0e, 0xc5, 0xc9,
                0x85, 0x23, 0x95, 0x45,
            ]
        );
        assert_eq!(root.id, REVIEWED_MAIN_IMAGE_TRUST_ROOT_ID);
        assert_eq!(root.version, REVIEWED_MAIN_IMAGE_TRUST_ROOT_VERSION);
        let source = include_str!("reviewed_build.rs");
        assert!(source.contains("0x3b, 0xc6, 0xa2, 0x10"));
        assert!(!include_str!("mcp.rs").contains(REVIEWED_MAIN_IMAGE_INCOMING_ENVELOPE));
        assert!(
            !include_str!("delegated/autonomous_contract.rs")
                .contains(REVIEWED_MAIN_IMAGE_INCOMING_ENVELOPE)
        );
    }

    #[cfg(windows)]
    #[test]
    fn stable_supervisor_role_binding_requires_the_exact_authenticated_opened_image() {
        let signing_key = SigningKey::from_bytes(&[0x42; 32]);
        let policy = reviewed_main_image_bootstrap_policy();
        let root = ReviewedMainImageTrustRootV1 {
            id: REVIEWED_MAIN_IMAGE_TRUST_ROOT_ID,
            version: REVIEWED_MAIN_IMAGE_TRUST_ROOT_VERSION,
            public_key: signing_key.verifying_key().to_bytes(),
        };
        let bytes = b"authenticated CatDesk image with fixed supervisor role";
        let envelope = test_signed_reviewed_main_image_envelope(
            &signing_key,
            &policy,
            1,
            &sha256(bytes),
            bytes.len() as u64,
        );
        verify_reviewed_main_image_envelope(&envelope, &root, &policy)
            .expect("test envelope is authenticated");

        let directory =
            std::env::temp_dir().join(format!("catdesk-stable-supervisor-role-{}", Uuid::new_v4()));
        fs::create_dir(&directory).expect("isolated directory");
        let image_path = directory.join("CatDesk.exe");
        fs::write(&image_path, bytes).expect("isolated reviewed image");
        let image = open_fixed_reviewed_main_image_file(&image_path, MAX_CANDIDATE_BYTES)
            .expect("safe exact image open");
        let capability = bind_verified_main_image_to_stable_supervisor_role(image, &envelope)
            .expect("role capability");
        assert_eq!(capability.role(), REVIEWED_STABLE_SUPERVISOR_ROLE);
        assert_eq!(
            capability
                .into_fixed_installer_payload()
                .expect("payload")
                .0,
            bytes
        );

        let mismatched = test_signed_reviewed_main_image_envelope(
            &signing_key,
            &policy,
            2,
            &sha256(b"different payload"),
            bytes.len() as u64,
        );
        let image = open_fixed_reviewed_main_image_file(&image_path, MAX_CANDIDATE_BYTES)
            .expect("safe exact image reopen");
        assert!(bind_verified_main_image_to_stable_supervisor_role(image, &mismatched).is_err());
        let wrong_length = test_signed_reviewed_main_image_envelope(
            &signing_key,
            &policy,
            3,
            &sha256(bytes),
            bytes.len() as u64 + 1,
        );
        let image = open_fixed_reviewed_main_image_file(&image_path, MAX_CANDIDATE_BYTES)
            .expect("safe exact image reopen");
        assert!(bind_verified_main_image_to_stable_supervisor_role(image, &wrong_length).is_err());
        assert!(open_fixed_reviewed_main_image_file(&directory, MAX_CANDIDATE_BYTES).is_err());
        use std::os::windows::fs::symlink_file;
        let outside = directory.join("outside.exe");
        fs::write(&outside, b"unreviewed replacement").expect("outside fixture");
        let redirected = directory.join("redirected.exe");
        if symlink_file(&outside, &redirected).is_ok() {
            assert!(
                open_fixed_reviewed_main_image_file(&redirected, MAX_CANDIDATE_BYTES).is_err(),
                "role binding must never receive a reparse-opened object"
            );
        }
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn stable_supervisor_role_binding_has_no_caller_or_worker_digest_authority() {
        let source = include_str!("reviewed_build.rs");
        let start = source
            .find("pub(crate) fn verified_reviewed_stable_supervisor_image()")
            .expect("production role binding");
        let end = start
            + source[start..]
                .find("#[cfg(not(windows))]")
                .expect("non-windows boundary");
        let binding = &source[start..end];
        assert!(binding.contains("read_accepted_reviewed_main_image_envelope"));
        assert!(binding.contains("open_fixed_reviewed_main_image_file"));
        assert!(binding.contains("bind_verified_main_image_to_stable_supervisor_role"));
        for forbidden in [
            "verified_current_reviewed_main_image_digest",
            "target/release",
            "current_exe",
            "current_dir",
            "PATH",
            "Command::new",
        ] {
            assert!(
                !binding.contains(forbidden),
                "role binding must not use {forbidden} as authority"
            );
        }
        assert!(source.contains("stable-supervisor-runtime-v1"));
        assert!(!source.contains(&["SUPERVISOR", "_SIGNING_KEY"].concat()));
        assert!(!source.contains(&["SUPERVISOR", "_PROVENANCE_PRODUCER"].concat()));
    }

    #[test]
    fn release_candidate_provisioning_cannot_mutate_canonical_release_pair() {
        let script = include_str!("../scripts/provision-catdesk-release.ps1");
        assert!(script.contains("target\\catdesk-release-candidate"));
        assert!(script.contains("--target-dir $candidateTarget"));
        assert!(script.contains("REVIEWED_PROMOTION_REQUIRED"));
        assert!(script.contains("CanonicalMutated=$false"));
        assert!(!script.contains("cargo build --release\n"));
        assert!(!script.contains("$manifest=\"$binary.sha256\""));
        assert!(!script.contains("Move-Item -LiteralPath $temporary -Destination $manifest"));
    }
}
