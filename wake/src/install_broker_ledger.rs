//! Handle-scoped, create-once broker transaction records.
//!
//! A future independently installed broker supplies an already pinned and
//! protected root handle. This module never selects a path, provisions a
//! directory, changes a service, reads a candidate package, or authorizes an
//! install mutation. Its file names are derived only from the signed install
//! generation and fixed phase names. A returned lease holds the intent file
//! exclusively until dropped; reopening checks every completed phase.
//!
//! File data and the parent directory are flushed before a new phase is
//! reported. The future broker must still validate the filesystem and storage
//! power-loss behavior before using this as a mutation gate.
//! The protected bootstrap must also pre-create the fixed global lock file,
//! establish its owner/DACL and supply a protected root with list access.
//! This code cannot prove the trustworthiness of a supplied root or clock.
//! An empty history cannot attest its own first generation: bootstrap must
//! bind that first generation to an independently protected predecessor pointer.
//! Production has no generic phase advancement past STAGED. PUBLISHED and
//! later writes remain fixture-only until their physical side effects and
//! recovery invariants are proven by typed broker-owned receipts.

use crate::{
    Result,
    install_broker_approval::{
        BrokerObservedStateV1, ObjectIdentityV1, VerifiedApprovalV1, WakeInstallApprovalV1,
        verify_approval,
    },
    install_broker_candidate_stage::{
        BrokerCandidateStageReadbackV1, inspect_stage_under_guard,
        require_empty_staging_under_guard,
    },
    install_broker_ci_fetch::require_broker_owned_exact_ci,
    install_broker_durability::{ParentFlushPoint, flush_pinned_directory},
    install_broker_policy::{BrokerProtectedPolicyV1, PolicyVerifiedApprovalV1},
    install_broker_reader::{
        ProtectedInstallPinV1, ProtectedInstallReadOnlySnapshotV1, ProtectedPublishedPackagePinV1,
        PublicationLocationV1, inspect_publication_location_under_guard,
        pin_protected_install_under_guard, pin_published_package_under_guard,
    },
    install_broker_stage_writer::BrokerStageReceiptV1,
};
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    ffi::c_void,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};

const MAX_RECORD_BYTES: usize = 4096;
const MAX_DIRECTORY_ENTRIES: usize = 4096;
const DOMAIN: &[u8] = b"CatDeskWakeInstallLedgerV1\0";
const GLOBAL_LOCK_NAME: &str = "install-ledger.lock";
const FILE_OPEN: u32 = 1;
const FILE_CREATE: u32 = 2;
const FILE_READ_DATA: u32 = 1;
const FILE_WRITE_DATA: u32 = 2;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_NON_DIRECTORY_FILE: u32 = 0x40;
const FILE_DIRECTORY_FILE: u32 = 0x1;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x20;
const FILE_WRITE_THROUGH: u32 = 0x2;
const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const STATUS_OBJECT_NAME_NOT_FOUND: u32 = 0xC000_0034;
const STATUS_OBJECT_NAME_COLLISION: u32 = 0xC000_0035;
const STATUS_SHARING_VIOLATION: u32 = 0xC000_0043;
const STATUS_NO_SUCH_FILE: u32 = 0xC000_000F;
const STATUS_NO_MORE_FILES: u32 = 0x8000_0006;
const FILE_NAMES_INFORMATION: u32 = 12;
const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: *mut c_void,
    object_name: *mut UnicodeString,
    attributes: u32,
    security_descriptor: *mut c_void,
    security_quality_of_service: *mut c_void,
}

#[repr(C)]
struct IoStatusBlock {
    status: isize,
    information: usize,
}

#[repr(C)]
#[derive(Default)]
struct FileInfo {
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

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        handle: *mut *mut c_void,
        access: u32,
        attributes: *mut ObjectAttributes,
        status: *mut IoStatusBlock,
        allocation: *mut c_void,
        file_attributes: u32,
        share: u32,
        disposition: u32,
        options: u32,
        ea: *mut c_void,
        ea_size: u32,
    ) -> i32;
    fn NtQueryDirectoryFile(
        handle: *mut c_void,
        event: *mut c_void,
        apc_routine: *mut c_void,
        apc_context: *mut c_void,
        status: *mut IoStatusBlock,
        information: *mut c_void,
        length: u32,
        information_class: u32,
        return_single_entry: u8,
        file_name: *mut UnicodeString,
        restart_scan: u8,
    ) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileInformationByHandle(handle: *mut c_void, info: *mut FileInfo) -> i32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallPhaseV1 {
    Intent,
    StageStarted,
    Staged,
    Published,
    OldOwnerStopped,
    PointerSwitched,
    NewOwnerProven,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OriginalDesiredStateV1 {
    Running,
    Paused,
    Stopped,
}

impl InstallPhaseV1 {
    const ORDER: [Self; 8] = [
        Self::Intent,
        Self::StageStarted,
        Self::Staged,
        Self::Published,
        Self::OldOwnerStopped,
        Self::PointerSwitched,
        Self::NewOwnerProven,
        Self::Complete,
    ];

    fn index(self) -> usize {
        Self::ORDER
            .iter()
            .position(|phase| *phase == self)
            .expect("all enum variants appear in ORDER")
    }

    fn name(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::StageStarted => "stage-started",
            Self::Staged => "staged",
            Self::Published => "published",
            Self::OldOwnerStopped => "old-owner-stopped",
            Self::PointerSwitched => "pointer-switched",
            Self::NewOwnerProven => "new-owner-proven",
            Self::Complete => "complete",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerExpectationV1 {
    /// Observed by the fixed broker's own clock, never from the request.
    pub now_utc_seconds: u64,
    pub next_install_generation: u64,
    pub predecessor_package_id: String,
    pub predecessor_pointer: ObjectIdentityV1,
    pub protected_root: ObjectIdentityV1,
    /// Read from the reconciled old owner before the first stop action.
    pub original_desired_state: OriginalDesiredStateV1,
}

/// Physical objects created and retained by the broker's protected stage
/// writer. These identities are persisted in STAGED and every later phase.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageBindingV1 {
    pub staging_directory: ObjectIdentityV1,
    pub candidate_directory: ObjectIdentityV1,
    pub manifest_file: ObjectIdentityV1,
    pub artifacts: [ObjectIdentityV1; 4],
    /// Present only for stages whose fixed `packages` parent was pinned and
    /// durably bound before publication. Omission preserves canonical older
    /// V1 records, but cannot authorize a future package rename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packages_directory: Option<ObjectIdentityV1>,
}

/// Exact canonical bytes and physical file selected as the new current.json.
/// This is journal evidence only; a future broker-owned switch writer must
/// retain and revalidate the pointer handle across recording the phase.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointerSwitchBindingV1 {
    pub pointer_file: ObjectIdentityV1,
    pub pointer_sha256: String,
}

/// A point-in-time diagnosis of one existing transaction. Matching STAGED IDs
/// at either the original slot or the signed package destination does not
/// retain the package, owner or pointer pins needed for install mutation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerExactResumeReadbackV1 {
    pub schema_version: u32,
    pub install_generation: u64,
    pub approval_id: String,
    pub phase: &'static str,
    pub stage: Option<BrokerCandidateStageReadbackV1>,
    pub ledger_stage_binding_matched: bool,
    /// True only when the signed package destination's exact bytes, security
    /// and six physical package object IDs match the durable STAGED binding.
    pub published_package_binding_matched: bool,
    /// True only when the physical staging parent still matches STAGED.
    pub staging_parent_object_verified: bool,
    /// True only when STAGED durably names the fixed packages parent and the
    /// pinned readback matched that exact physical directory. Older STAGED
    /// records remain readable but cannot supply this publication premise.
    pub packages_parent_object_verified: bool,
    pub disposition: &'static str,
}

/// A completed ledger head and six installed package object IDs agree at one
/// locked readback. The staging parent is not checked. Older completed records
/// may lack a pointer-switch binding, and even a contiguous ledger
/// beginning above generation one is not a migration anchor. The live old
/// owner is also unverified. This value is diagnostic; it does not reverify
/// the historical signed approval envelope or become an install observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerCompletedPredecessorReadbackV1 {
    pub schema_version: u32,
    pub install_generation: u64,
    pub approval_id: String,
    pub protected_install: ProtectedInstallReadOnlySnapshotV1,
    pub installed_package_object_ids_matched: bool,
    pub staging_parent_object_verified: bool,
    /// True only when the recorded new file ID and SHA-256 match the pinned
    /// current.json object and exact canonical bytes at this readback.
    pub pointer_switch_record_binding_present: bool,
    pub first_generation_migration_anchor_verified: bool,
    /// No current reader proves the serving dev.84 owner and its loaded image.
    pub old_live_owner_verified: bool,
    pub disposition: &'static str,
}

#[derive(Clone, Copy)]
enum PredecessorReaderMode {
    Protected,
    #[cfg(test)]
    Fixture,
}

#[derive(Clone, Copy)]
enum ResumeStageInspection<'a> {
    Protected(std::marker::PhantomData<&'a ()>),
    #[cfg(test)]
    Fixture,
    #[cfg(test)]
    FixturePublication(&'a crate::install_broker_reader::PublicationFixturePolicyV1),
}

fn inspect_publication_for_resume(
    root: &OwnedHandle,
    root_id: &ObjectIdentityV1,
    policy: &BrokerProtectedPolicyV1,
    approved: &VerifiedApprovalV1,
    stage: &StageBindingV1,
    mode: ResumeStageInspection<'_>,
) -> Result<(
    PublicationLocationV1,
    Option<ProtectedPublishedPackagePinV1>,
)> {
    let location = match mode {
        ResumeStageInspection::Protected(_) => {
            let (service_sid, _) = policy.checked_endpoint_sids()?;
            inspect_publication_location_under_guard(root, root_id, approved, stage, service_sid)
        }
        #[cfg(test)]
        ResumeStageInspection::FixturePublication(fixture_policy) => {
            crate::install_broker_reader::inspect_publication_location_fixture(
                root,
                root_id,
                approved,
                stage,
                fixture_policy,
            )
        }
        #[cfg(test)]
        ResumeStageInspection::Fixture => Err("WAKE_LEDGER_RESUME_PHASE_UNSUPPORTED".into()),
    }?;
    let pin = match (&location, mode) {
        (PublicationLocationV1::Moved(expected), ResumeStageInspection::Protected(_)) => {
            let (service_sid, _) = policy.checked_endpoint_sids()?;
            let pin =
                pin_published_package_under_guard(root, root_id, approved, stage, service_sid)?;
            if pin.snapshot() != expected.as_ref() {
                return Err("WAKE_PROTECTED_PUBLICATION_OBJECT_CHANGED".into());
            }
            Some(pin)
        }
        #[cfg(test)]
        (
            PublicationLocationV1::Moved(expected),
            ResumeStageInspection::FixturePublication(fixture_policy),
        ) => {
            let pin = crate::install_broker_reader::pin_published_package_fixture(
                root,
                root_id,
                approved,
                stage,
                fixture_policy,
            )?;
            if pin.snapshot() != expected.as_ref() {
                return Err("WAKE_PROTECTED_PUBLICATION_OBJECT_CHANGED".into());
            }
            Some(pin)
        }
        (PublicationLocationV1::Stage, _) => None,
        #[cfg(test)]
        (PublicationLocationV1::Moved(_), ResumeStageInspection::Fixture) => {
            return Err("WAKE_LEDGER_RESUME_PHASE_UNSUPPORTED".into());
        }
    };
    Ok((location, pin))
}

/// The exact signature and protected trust root accepted with INTENT. Every
/// subsequent record repeats this binding, including after crash recovery.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ApprovalContinuityV1 {
    pub(crate) signed_envelope_sha256: String,
    pub(crate) policy_file: ObjectIdentityV1,
    pub(crate) policy_sha256: String,
    pub(crate) approval_public_key_sha256: String,
}

impl ApprovalContinuityV1 {
    fn from_policy(approved: &PolicyVerifiedApprovalV1<'_>) -> Self {
        let policy = approved.readback_snapshot();
        Self {
            signed_envelope_sha256: approved.verified().signed_envelope_sha256().into(),
            policy_file: policy.policy_file.clone(),
            policy_sha256: policy.policy_sha256.clone(),
            approval_public_key_sha256: policy.approval_public_key_sha256.clone(),
        }
    }

    #[cfg(test)]
    fn fixture(approval: &VerifiedApprovalV1, root: &ObjectIdentityV1) -> Self {
        let mut policy_file = root.clone();
        policy_file.file_index_low = if root.file_index_low == u32::MAX {
            u32::MAX - 1
        } else {
            root.file_index_low + 1
        };
        Self {
            signed_envelope_sha256: approval.signed_envelope_sha256().into(),
            policy_file,
            policy_sha256: "0".repeat(64),
            approval_public_key_sha256: approval.verification_key_sha256().into(),
        }
    }

    fn valid_for(&self, root: &ObjectIdentityV1) -> bool {
        lowercase_hex(&self.signed_envelope_sha256, 64)
            && lowercase_hex(&self.policy_sha256, 64)
            && lowercase_hex(&self.approval_public_key_sha256, 64)
            && self.policy_file.volume_serial == root.volume_serial
            && (self.policy_file.file_index_high != 0 || self.policy_file.file_index_low != 0)
            && self.policy_file != *root
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LedgerRecordV1 {
    schema_version: u32,
    purpose: String,
    install_generation: u64,
    approval_id: String,
    signed_approval_sha256: String,
    approval_continuity: ApprovalContinuityV1,
    accepted_at_utc_seconds: u64,
    package_id: String,
    manifest_sha256: String,
    predecessor_package_id: String,
    predecessor_pointer: ObjectIdentityV1,
    protected_root: ObjectIdentityV1,
    old_runtime_root: ObjectIdentityV1,
    old_runtime_lock: ObjectIdentityV1,
    old_host_image: ObjectIdentityV1,
    old_host_image_sha256: String,
    old_host_pid: u32,
    old_host_creation_time_100ns: u64,
    original_desired_state: OriginalDesiredStateV1,
    stage_binding: Option<StageBindingV1>,
    // Omit None so the exact canonical bytes of pre-binding V1 records still
    // round-trip. A missing binding never grants a new transaction authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pointer_switch_binding: Option<PointerSwitchBindingV1>,
    phase: String,
    prior_record_sha256: String,
}

#[derive(Clone, Debug)]
struct Binding {
    install_generation: u64,
    approval_id: String,
    signed_approval_sha256: String,
    approval_continuity: ApprovalContinuityV1,
    accepted_at_utc_seconds: u64,
    package_id: String,
    manifest_sha256: String,
    predecessor_package_id: String,
    predecessor_pointer: ObjectIdentityV1,
    protected_root: ObjectIdentityV1,
    old_runtime_root: ObjectIdentityV1,
    old_runtime_lock: ObjectIdentityV1,
    old_host_image: ObjectIdentityV1,
    old_host_image_sha256: String,
    old_host_pid: u32,
    old_host_creation_time_100ns: u64,
    original_desired_state: OriginalDesiredStateV1,
    stage_binding: Option<StageBindingV1>,
    pointer_switch_binding: Option<PointerSwitchBindingV1>,
}

impl Binding {
    fn from_approval(
        approval: &VerifiedApprovalV1,
        original_desired_state: OriginalDesiredStateV1,
        approval_continuity: ApprovalContinuityV1,
    ) -> Self {
        let payload = approval.payload();
        Self {
            install_generation: payload.install_generation,
            approval_id: payload.approval_id.clone(),
            signed_approval_sha256: approval.signed_payload_sha256().into(),
            approval_continuity,
            accepted_at_utc_seconds: approval.verified_at_utc_seconds(),
            package_id: payload.manifest.package_id.clone(),
            manifest_sha256: payload.manifest_sha256.clone(),
            predecessor_package_id: payload.predecessor_package_id.clone(),
            predecessor_pointer: payload.predecessor_pointer.clone(),
            protected_root: payload.target_protected_root.clone(),
            old_runtime_root: payload.old_runtime_root.clone(),
            old_runtime_lock: payload.old_runtime_lock.clone(),
            old_host_image: payload.old_host_image.clone(),
            old_host_image_sha256: payload.old_host_image_sha256.clone(),
            old_host_pid: payload.old_host_pid,
            old_host_creation_time_100ns: payload.old_host_creation_time_100ns,
            original_desired_state,
            stage_binding: None,
            pointer_switch_binding: None,
        }
    }

    fn record(&self, phase: InstallPhaseV1, prior_record_sha256: &str) -> LedgerRecordV1 {
        LedgerRecordV1 {
            schema_version: 1,
            purpose: "catdesk-wake-protected-install-transaction".into(),
            install_generation: self.install_generation,
            approval_id: self.approval_id.clone(),
            signed_approval_sha256: self.signed_approval_sha256.clone(),
            approval_continuity: self.approval_continuity.clone(),
            accepted_at_utc_seconds: self.accepted_at_utc_seconds,
            package_id: self.package_id.clone(),
            manifest_sha256: self.manifest_sha256.clone(),
            predecessor_package_id: self.predecessor_package_id.clone(),
            predecessor_pointer: self.predecessor_pointer.clone(),
            protected_root: self.protected_root.clone(),
            old_runtime_root: self.old_runtime_root.clone(),
            old_runtime_lock: self.old_runtime_lock.clone(),
            old_host_image: self.old_host_image.clone(),
            old_host_image_sha256: self.old_host_image_sha256.clone(),
            old_host_pid: self.old_host_pid,
            old_host_creation_time_100ns: self.old_host_creation_time_100ns,
            original_desired_state: self.original_desired_state,
            stage_binding: self.stage_binding.clone(),
            pointer_switch_binding: self.pointer_switch_binding.clone(),
            phase: phase.name().into(),
            prior_record_sha256: prior_record_sha256.into(),
        }
    }
}

/// Holds the intent record with share mode zero for the complete transaction
/// observation. Dropping it releases the process lease but never erases a
/// record. A different approval at the same generation remains a conflict.
pub struct BrokerLedgerLease<'a> {
    root: &'a OwnedHandle,
    _global_lock: File,
    _intent_file: File,
    binding: Binding,
    current: InstallPhaseV1,
    current_digest: String,
}

/// Owns the fixed global lock before the broker samples predecessor, old
/// owner, clock, or source state. Only a guarded acceptance can write INTENT.
pub struct BrokerLedgerGuard<'a> {
    root: &'a OwnedHandle,
    global_lock: File,
}

impl<'a> BrokerLedgerGuard<'a> {
    pub fn acquire(root: &'a OwnedHandle) -> Result<Self> {
        root_info(root)?;
        let global_lock = acquire_global_lock(root)?;
        Ok(Self { root, global_lock })
    }

    /// Use this pinned handle only for broker-owned observations while the
    /// global lock is held. No request value is installation evidence.
    pub fn root(&self) -> &OwnedHandle {
        self.root
    }

    /// Return the most recent protected acceptance time while retaining the
    /// global lock. This is only a rollback floor for a future broker clock:
    /// an empty ledger cannot establish trustworthy initial time or generation.
    pub(crate) fn latest_accepted_at_utc_seconds(
        &self,
        policy: &BrokerProtectedPolicyV1,
    ) -> Result<Option<u64>> {
        let root_id = root_info(self.root)?;
        if policy.readback()?.protected_root != root_id {
            return Err("WAKE_LEDGER_ROOT_CHANGED".into());
        }
        let latest = validated_latest_accepted_at(self.root, &root_id)?;
        policy.revalidate()?;
        Ok(latest)
    }

    /// Corroborate only a previously COMPLETE protected package under this
    /// retained global lock. This cannot establish the dev.84 first-generation
    /// migration, the old live owner, or the pointer-switch side effect.
    pub fn inspect_completed_predecessor(
        &self,
        policy: &BrokerProtectedPolicyV1,
    ) -> Result<BrokerCompletedPredecessorReadbackV1> {
        self.inspect_completed_predecessor_with(policy, PredecessorReaderMode::Protected)
    }

    #[cfg(test)]
    fn inspect_completed_predecessor_fixture(
        &self,
        policy: &BrokerProtectedPolicyV1,
    ) -> Result<BrokerCompletedPredecessorReadbackV1> {
        self.inspect_completed_predecessor_with(policy, PredecessorReaderMode::Fixture)
    }

    fn inspect_completed_predecessor_with(
        &self,
        policy: &BrokerProtectedPolicyV1,
        reader_mode: PredecessorReaderMode,
    ) -> Result<BrokerCompletedPredecessorReadbackV1> {
        let (readback, _pin) = self.pin_completed_predecessor_with(policy, reader_mode)?;
        Ok(readback)
    }

    /// Keep the physical pointer and package pins live for a subsequent
    /// guarded comparison. The public projection above deliberately drops
    /// these pins and cannot be used to authorize an install.
    fn pin_completed_predecessor_with(
        &self,
        policy: &BrokerProtectedPolicyV1,
        reader_mode: PredecessorReaderMode,
    ) -> Result<(BrokerCompletedPredecessorReadbackV1, ProtectedInstallPinV1)> {
        let root_id = root_info(self.root)?;
        let policy_before = policy.readback()?;
        if policy_before.protected_root != root_id {
            return Err("WAKE_LEDGER_ROOT_CHANGED".into());
        }
        let (inventory, head, stage, pointer_binding) = completed_head(self.root, &root_id)?;
        if head.approval_continuity.policy_file != policy_before.policy_file
            || head.approval_continuity.policy_sha256 != policy_before.policy_sha256
            || head.approval_continuity.approval_public_key_sha256
                != policy_before.approval_public_key_sha256
        {
            return Err("WAKE_BROKER_PREDECESSOR_POLICY_MISMATCH".into());
        }
        let (service_sid, _) = policy.checked_endpoint_sids()?;
        let pin = match reader_mode {
            PredecessorReaderMode::Protected => {
                pin_protected_install_under_guard(self.root, &root_id, service_sid)?
            }
            #[cfg(test)]
            PredecessorReaderMode::Fixture => {
                crate::install_broker_reader::pin_protected_install_fixture(self.root, &root_id)?
            }
        };
        let protected_install = pin.snapshot().clone();
        compare_completed_head(
            &head,
            &stage,
            pointer_binding.as_ref(),
            &protected_install,
            pin.pointer_sha256(),
        )?;
        pin.revalidate(self.root, &root_id)?;
        let (after_inventory, after_head, after_stage, after_pointer_binding) =
            completed_head(self.root, &root_id)?;
        if after_inventory != inventory
            || after_head != head
            || after_stage != stage
            || after_pointer_binding != pointer_binding
        {
            return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
        }
        if root_info(self.root)? != root_id || policy.readback()? != policy_before {
            return Err("WAKE_BROKER_PREDECESSOR_POLICY_MISMATCH".into());
        }
        // Keep the pinned package, policy and global lock live through the
        // last read. The public projection drops the retained pin.
        let readback = BrokerCompletedPredecessorReadbackV1 {
            schema_version: 1,
            install_generation: head.install_generation,
            approval_id: head.approval_id,
            protected_install,
            installed_package_object_ids_matched: true,
            staging_parent_object_verified: false,
            pointer_switch_record_binding_present: pointer_binding.is_some(),
            first_generation_migration_anchor_verified: false,
            old_live_owner_verified: false,
            disposition: if pointer_binding.is_some() {
                "COMPLETE_POINTER_BINDING_MATCHES_READBACK_NO_INSTALL_AUTHORITY"
            } else {
                "COMPLETE_PACKAGE_IDS_MATCH_POINTER_UNBOUND_NO_INSTALL_AUTHORITY"
            },
        };
        Ok((readback, pin))
    }

    /// Compare a proposed fresh successor to the retained, fully replayed
    /// COMPLETE head and the exact currently pinned protected install. This
    /// checks predecessor correspondence only; the first dev.84 migration
    /// anchor and old live owner still have no trusted production proof.
    fn pin_fresh_successor_predecessor_with(
        &self,
        policy: &BrokerProtectedPolicyV1,
        approved: &PolicyVerifiedApprovalV1<'_>,
        expected: &LedgerExpectationV1,
        reader_mode: PredecessorReaderMode,
    ) -> Result<(BrokerCompletedPredecessorReadbackV1, ProtectedInstallPinV1)> {
        let (readback, pin) = self.pin_completed_predecessor_with(policy, reader_mode)?;
        if approved.readback_snapshot() != &policy.readback()? {
            return Err("WAKE_BROKER_PREDECESSOR_SUCCESSOR_MISMATCH".into());
        }
        compare_fresh_successor(&readback, approved.verified().payload(), expected)?;
        pin.revalidate(self.root, &readback.protected_install.protected_root)?;
        policy.revalidate()?;
        Ok((readback, pin))
    }

    /// Diagnose only an existing exact transaction while this guard retains
    /// the global lock. The observation callback is a future service seam: it
    /// must sample broker-owned pinned objects and a trusted clock. This
    /// readback creates no record, flushes no directory and returns no lease,
    /// approval capability or install authority.
    pub fn inspect_exact_resume(
        &self,
        policy: &BrokerProtectedPolicyV1,
        envelope: &[u8],
        generation: u64,
        observe_current: impl FnOnce(
            &OwnedHandle,
        ) -> Result<(BrokerObservedStateV1, OriginalDesiredStateV1)>,
    ) -> Result<BrokerExactResumeReadbackV1> {
        self.inspect_exact_resume_with(
            policy,
            envelope,
            generation,
            observe_current,
            ResumeStageInspection::Protected(std::marker::PhantomData),
        )
    }

    #[cfg(test)]
    pub(crate) fn inspect_exact_resume_fixture(
        &self,
        policy: &BrokerProtectedPolicyV1,
        envelope: &[u8],
        generation: u64,
        observe_current: impl FnOnce(
            &OwnedHandle,
        ) -> Result<(BrokerObservedStateV1, OriginalDesiredStateV1)>,
    ) -> Result<BrokerExactResumeReadbackV1> {
        self.inspect_exact_resume_with(
            policy,
            envelope,
            generation,
            observe_current,
            ResumeStageInspection::Fixture,
        )
    }

    #[cfg(test)]
    pub(crate) fn inspect_publication_seam_fixture(
        &self,
        policy: &BrokerProtectedPolicyV1,
        envelope: &[u8],
        generation: u64,
        observe_current: impl FnOnce(
            &OwnedHandle,
        ) -> Result<(BrokerObservedStateV1, OriginalDesiredStateV1)>,
        publication_policy: &crate::install_broker_reader::PublicationFixturePolicyV1,
    ) -> Result<BrokerExactResumeReadbackV1> {
        self.inspect_exact_resume_with(
            policy,
            envelope,
            generation,
            observe_current,
            ResumeStageInspection::FixturePublication(publication_policy),
        )
    }

    fn inspect_exact_resume_with(
        &self,
        policy: &BrokerProtectedPolicyV1,
        envelope: &[u8],
        generation: u64,
        observe_current: impl FnOnce(
            &OwnedHandle,
        ) -> Result<(BrokerObservedStateV1, OriginalDesiredStateV1)>,
        stage_inspection: ResumeStageInspection<'_>,
    ) -> Result<BrokerExactResumeReadbackV1> {
        if generation == 0 {
            return Err("WAKE_LEDGER_GENERATION_INVALID".into());
        }
        let root_id = root_info(self.root)?;
        if policy.readback()?.protected_root != root_id {
            return Err("WAKE_LEDGER_ROOT_CHANGED".into());
        }
        let inventory = ledger_inventory(self.root)?;
        let present = inventory
            .get(&generation)
            .ok_or("WAKE_LEDGER_NO_PRIOR_INTENT")?;
        let (intent, phase, stage_binding, pointer_binding) =
            historical_intent(self.root, generation, present, &root_id)?;
        let original = original_observation(&intent);
        let approved = policy.verify_approval_for_transaction(envelope, &original)?;
        let binding = Binding::from_approval(
            approved.verified(),
            intent.original_desired_state,
            ApprovalContinuityV1::from_policy(&approved),
        );
        if intent != binding.record(InstallPhaseV1::Intent, &"0".repeat(64)) {
            return Err("WAKE_LEDGER_APPROVAL_CONFLICT".into());
        }
        if !preflight_history(self.root, &binding, &root_id)? {
            return Err("WAKE_LEDGER_NO_PRIOR_INTENT".into());
        }
        let (current, desired) = observe_current(self.root)?;
        if current.now_utc_seconds < intent.accepted_at_utc_seconds {
            return Err("WAKE_LEDGER_CLOCK_ROLLBACK".into());
        }
        let mut expected_current = original;
        expected_current.now_utc_seconds = current.now_utc_seconds;
        if current != expected_current || desired != intent.original_desired_state {
            return Err("WAKE_LEDGER_RESUME_PHYSICAL_MISMATCH".into());
        }
        let mut published_pin = None;
        let (stage, published_package_binding_matched, staging_parent_object_verified, disposition) =
            match phase {
                InstallPhaseV1::Intent => {
                    match stage_inspection {
                        ResumeStageInspection::Protected(_) => {
                            let (service_sid, _) = policy.checked_endpoint_sids()?;
                            require_empty_staging_under_guard(self.root, &root_id, service_sid)?;
                        }
                        #[cfg(test)]
                        ResumeStageInspection::Fixture
                        | ResumeStageInspection::FixturePublication(_) => {
                            crate::install_broker_candidate_stage::require_empty_staging_fixture(
                                self.root,
                            )?;
                        }
                    }
                    (
                        None,
                        false,
                        false,
                        "INTENT_STAGING_EMPTY_NO_INSTALL_AUTHORITY",
                    )
                }
                InstallPhaseV1::StageStarted => (
                    None,
                    false,
                    false,
                    "STAGE_STARTED_ATTENTION_NO_INSTALL_AUTHORITY",
                ),
                InstallPhaseV1::Staged => {
                    let durable_stage = stage_binding
                        .as_ref()
                        .ok_or("WAKE_LEDGER_STAGE_BINDING_INVALID")?;
                    let location = match stage_inspection {
                        #[cfg(test)]
                        ResumeStageInspection::Fixture => None,
                        _ => {
                            let (location, pin) = inspect_publication_for_resume(
                                self.root,
                                &root_id,
                                policy,
                                approved.verified(),
                                durable_stage,
                                stage_inspection,
                            )?;
                            published_pin = pin;
                            Some(location)
                        }
                    };
                    if let Some(PublicationLocationV1::Moved(_)) = location {
                        (
                            None,
                            true,
                            true,
                            "STAGED_PACKAGE_MOVED_PUBLISHED_RECORD_MISSING_ATTENTION_NO_INSTALL_AUTHORITY",
                        )
                    } else {
                        let snapshot = match stage_inspection {
                            ResumeStageInspection::Protected(_) => {
                                let (service_sid, _) = policy.checked_endpoint_sids()?;
                                inspect_stage_under_guard(
                                    self.root,
                                    &root_id,
                                    approved.verified(),
                                    service_sid,
                                )?
                            }
                            #[cfg(test)]
                            ResumeStageInspection::Fixture
                            | ResumeStageInspection::FixturePublication(_) => {
                                crate::install_broker_candidate_stage::inspect_stage_fixture(
                                    self.root,
                                    &root_id,
                                    approved.verified(),
                                )?
                            }
                        };
                        let observed_stage = StageBindingV1 {
                            staging_directory: snapshot.staging_directory.clone(),
                            packages_directory: match stage_inspection {
                                #[cfg(test)]
                                ResumeStageInspection::Fixture => None,
                                _ => durable_stage.packages_directory.clone(),
                            },
                            candidate_directory: snapshot.candidate_directory.clone(),
                            manifest_file: snapshot.manifest_file.clone(),
                            artifacts: std::array::from_fn(|index| {
                                snapshot.artifacts[index].object.clone()
                            }),
                        };
                        if durable_stage != &observed_stage {
                            return Err("WAKE_LEDGER_STAGE_BINDING_INVALID".into());
                        }
                        (
                            Some(snapshot),
                            false,
                            true,
                            "STAGED_IDS_MATCH_NO_INSTALL_AUTHORITY",
                        )
                    }
                }
                InstallPhaseV1::Published => {
                    let durable_stage = stage_binding
                        .as_ref()
                        .ok_or("WAKE_LEDGER_STAGE_BINDING_INVALID")?;
                    let (location, pin) = inspect_publication_for_resume(
                        self.root,
                        &root_id,
                        policy,
                        approved.verified(),
                        durable_stage,
                        stage_inspection,
                    )?;
                    published_pin = pin;
                    match location {
                        PublicationLocationV1::Moved(_) => (
                            None,
                            true,
                            true,
                            "PUBLISHED_PACKAGE_IDS_MATCH_NO_INSTALL_AUTHORITY",
                        ),
                        PublicationLocationV1::Stage => {
                            return Err("WAKE_BROKER_PUBLICATION_PHASE_MISMATCH".into());
                        }
                    }
                }
                _ => return Err("WAKE_LEDGER_RESUME_PHASE_UNSUPPORTED".into()),
            };
        // The result is a point-in-time diagnosis. Reopen this generation's
        // records and the policy after stage I/O. Prior history was checked
        // under the cooperative global lock, not snapshotted against a
        // noncooperating writer with protected-ledger write access.
        if ledger_inventory(self.root)? != inventory {
            return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
        }
        let (last_intent, last_phase, last_binding, last_pointer_binding) =
            historical_intent(self.root, generation, present, &root_id)?;
        if last_intent != intent
            || last_phase != phase
            || last_binding != stage_binding
            || last_pointer_binding != pointer_binding
        {
            return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
        }
        policy.revalidate()?;
        if root_info(self.root)? != root_id {
            return Err("WAKE_LEDGER_ROOT_CHANGED".into());
        }
        if let Some(pin) = &published_pin {
            pin.revalidate(self.root, &root_id)?;
        }
        Ok(BrokerExactResumeReadbackV1 {
            schema_version: 1,
            install_generation: generation,
            approval_id: intent.approval_id,
            phase: phase.name(),
            ledger_stage_binding_matched: stage.is_some(),
            published_package_binding_matched,
            staging_parent_object_verified,
            packages_parent_object_verified: (stage.is_some() || published_pin.is_some())
                && stage_binding
                    .as_ref()
                    .is_some_and(|binding| binding.packages_directory.is_some()),
            stage,
            disposition,
        })
    }
}

fn lowercase_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn valid_stage_binding(stage: &StageBindingV1, root: &ObjectIdentityV1) -> bool {
    let objects = [
        &stage.staging_directory,
        &stage.candidate_directory,
        &stage.manifest_file,
        &stage.artifacts[0],
        &stage.artifacts[1],
        &stage.artifacts[2],
        &stage.artifacts[3],
    ];
    let fixed_objects_valid = objects.iter().enumerate().all(|(index, object)| {
        object.volume_serial == root.volume_serial
            && (object.file_index_high != 0 || object.file_index_low != 0)
            && objects[..index].iter().all(|prior| *prior != *object)
    });
    fixed_objects_valid
        && stage.packages_directory.as_ref().is_none_or(|packages| {
            packages.volume_serial == root.volume_serial
                && (packages.file_index_high != 0 || packages.file_index_low != 0)
                && packages != root
                && !objects.contains(&packages)
        })
}

fn valid_pointer_switch_binding(
    pointer: &PointerSwitchBindingV1,
    root: &ObjectIdentityV1,
    predecessor: &ObjectIdentityV1,
    stage: &StageBindingV1,
) -> bool {
    let file = &pointer.pointer_file;
    file.volume_serial == root.volume_serial
        && (file.file_index_high != 0 || file.file_index_low != 0)
        && file != root
        && file != predecessor
        && file != &stage.staging_directory
        && stage.packages_directory.as_ref() != Some(file)
        && file != &stage.candidate_directory
        && file != &stage.manifest_file
        && !stage.artifacts.contains(file)
        && lowercase_hex(&pointer.pointer_sha256, 64)
}

fn root_info(root: &OwnedHandle) -> Result<ObjectIdentityV1> {
    let mut info = FileInfo::default();
    if unsafe { GetFileInformationByHandle(root.as_raw_handle(), &mut info) } == 0
        || info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || info.attributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || info.volume == 0
        || (info.index_high == 0 && info.index_low == 0)
    {
        return Err("WAKE_LEDGER_ROOT_INVALID".into());
    }
    Ok(ObjectIdentityV1 {
        volume_serial: info.volume,
        file_index_high: info.index_high,
        file_index_low: info.index_low,
    })
}

fn file_info(file: &File, expected_length: Option<usize>) -> Result<()> {
    let mut info = FileInfo::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
        || info.attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0
        || info.links != 1
    {
        return Err("WAKE_LEDGER_RECORD_INVALID".into());
    }
    let length = (u64::from(info.size_high) << 32) | u64::from(info.size_low);
    if length > MAX_RECORD_BYTES as u64
        || expected_length.is_some_and(|expected| length != expected as u64)
    {
        return Err("WAKE_LEDGER_RECORD_INVALID".into());
    }
    Ok(())
}

fn record_name(generation: u64, phase: InstallPhaseV1) -> String {
    format!("install-transaction-{generation:020}-{}.json", phase.name())
}

fn open_relative(
    root: &OwnedHandle,
    name: &str,
    disposition: u32,
) -> std::result::Result<File, u32> {
    // Callers derive names exclusively via record_name. No caller path, slash,
    // dot component, ADS, or arbitrary child can enter this NT relative open.
    if name.len() > 96
        || name.is_empty()
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
        })
    {
        return Err(u32::MAX);
    }
    let mut units: Vec<u16> = name.encode_utf16().collect();
    let mut unicode = UnicodeString {
        length: (units.len() * 2) as u16,
        maximum_length: (units.len() * 2) as u16,
        buffer: units.as_mut_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: root.as_raw_handle(),
        object_name: &mut unicode,
        attributes: 0x40,
        security_descriptor: ptr::null_mut(),
        security_quality_of_service: ptr::null_mut(),
    };
    let mut status = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut handle = ptr::null_mut();
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &mut attributes,
            &mut status,
            ptr::null_mut(),
            0,
            0, // no read/write/delete sharing: this opened record is the lease
            disposition,
            FILE_NON_DIRECTORY_FILE
                | FILE_SYNCHRONOUS_IO_NONALERT
                | FILE_WRITE_THROUGH
                | FILE_OPEN_REPARSE_POINT,
            ptr::null_mut(),
            0,
        )
    };
    if result < 0 || handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(result as u32);
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn create_or_reopen(root: &OwnedHandle, name: &str) -> Result<(File, bool)> {
    match open_relative(root, name, FILE_CREATE) {
        Ok(file) => Ok((file, true)),
        Err(STATUS_OBJECT_NAME_COLLISION) => match open_relative(root, name, FILE_OPEN) {
            Ok(file) => Ok((file, false)),
            Err(STATUS_SHARING_VIOLATION) => Err("WAKE_LEDGER_CONTENDER_BUSY".into()),
            Err(_) => Err("WAKE_LEDGER_RECORD_UNAVAILABLE".into()),
        },
        Err(STATUS_SHARING_VIOLATION) => Err("WAKE_LEDGER_CONTENDER_BUSY".into()),
        Err(_) => Err("WAKE_LEDGER_CREATE_FAILED".into()),
    }
}

fn open_optional(root: &OwnedHandle, name: &str) -> Result<Option<File>> {
    match open_relative(root, name, FILE_OPEN) {
        Ok(file) => Ok(Some(file)),
        Err(STATUS_OBJECT_NAME_NOT_FOUND) => Ok(None),
        Err(STATUS_SHARING_VIOLATION) => Err("WAKE_LEDGER_CONTENDER_BUSY".into()),
        Err(_) => Err("WAKE_LEDGER_RECORD_UNAVAILABLE".into()),
    }
}

fn acquire_global_lock(root: &OwnedHandle) -> Result<File> {
    let lock = match open_relative(root, GLOBAL_LOCK_NAME, FILE_OPEN) {
        Ok(file) => file,
        Err(STATUS_OBJECT_NAME_NOT_FOUND) => return Err("WAKE_LEDGER_LOCK_UNPROVISIONED".into()),
        Err(STATUS_SHARING_VIOLATION) => return Err("WAKE_LEDGER_CONTENDER_BUSY".into()),
        Err(_) => return Err("WAKE_LEDGER_LOCK_UNAVAILABLE".into()),
    };
    file_info(&lock, Some(0))?;
    Ok(lock)
}

fn open_fresh_inventory_directory(root: &OwnedHandle) -> Result<OwnedHandle> {
    // NtQueryDirectoryFile captures its search expression on the FILE_OBJECT's
    // first query. An empty relative object name reopens the pinned directory
    // as a fresh FILE_OBJECT; its physical identity is checked below.
    let mut unicode = UnicodeString {
        length: 0,
        maximum_length: 0,
        buffer: ptr::null_mut(),
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: root.as_raw_handle(),
        object_name: &mut unicode,
        attributes: 0x40,
        security_descriptor: ptr::null_mut(),
        security_quality_of_service: ptr::null_mut(),
    };
    let mut status = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut raw = ptr::null_mut();
    let result = unsafe {
        NtCreateFile(
            &mut raw,
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &mut attributes,
            &mut status,
            ptr::null_mut(),
            0,
            1 | 2, // share read/write but deny delete/rename while scanning
            FILE_OPEN,
            FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
            ptr::null_mut(),
            0,
        )
    };
    if result < 0 || raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err("WAKE_LEDGER_INVENTORY_UNAVAILABLE".into());
    }
    let fresh = unsafe { OwnedHandle::from_raw_handle(raw) };
    if root_info(&fresh)? != root_info(root)? {
        return Err("WAKE_LEDGER_ROOT_CHANGED".into());
    }
    Ok(fresh)
}

fn parse_record_name(name: &str) -> Result<(u64, InstallPhaseV1)> {
    let rest = name
        .strip_prefix("install-transaction-")
        .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
    let digits = rest.get(..20).ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
    }
    let generation: u64 = digits.parse().map_err(|_| "WAKE_LEDGER_RECORD_CORRUPT")?;
    let suffix = rest
        .get(20..)
        .and_then(|value| value.strip_prefix('-'))
        .and_then(|value| value.strip_suffix(".json"))
        .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
    let phase = InstallPhaseV1::ORDER
        .iter()
        .copied()
        .find(|phase| phase.name() == suffix)
        .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
    if generation == 0 || record_name(generation, phase) != name {
        return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
    }
    Ok((generation, phase))
}

/// Enumerate only the fixed record prefix through the pinned directory handle.
/// The caller holds the pre-provisioned global lock throughout the scan and
/// subsequent transaction, so another cooperating broker cannot race it.
fn ledger_inventory(root: &OwnedHandle) -> Result<BTreeMap<u64, [bool; 8]>> {
    let fresh = open_fresh_inventory_directory(root)?;
    let mut pattern: Vec<u16> = "install-transaction-*".encode_utf16().collect();
    let mut filter = UnicodeString {
        length: (pattern.len() * 2) as u16,
        maximum_length: (pattern.len() * 2) as u16,
        buffer: pattern.as_mut_ptr(),
    };
    let mut found = BTreeMap::new();
    for index in 0..=MAX_DIRECTORY_ENTRIES {
        let mut aligned = [0u64; 128];
        let mut status = IoStatusBlock {
            status: 0,
            information: 0,
        };
        let result = unsafe {
            NtQueryDirectoryFile(
                fresh.as_raw_handle(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut status,
                aligned.as_mut_ptr().cast(),
                std::mem::size_of_val(&aligned) as u32,
                FILE_NAMES_INFORMATION,
                1,
                if index == 0 {
                    &mut filter
                } else {
                    ptr::null_mut()
                },
                u8::from(index == 0),
            )
        } as u32;
        if result == STATUS_NO_MORE_FILES || (index == 0 && result == STATUS_NO_SUCH_FILE) {
            return Ok(found);
        }
        if result != 0 || index == MAX_DIRECTORY_ENTRIES {
            return Err("WAKE_LEDGER_INVENTORY_UNAVAILABLE".into());
        }
        if status.information < 12 || status.information > std::mem::size_of_val(&aligned) {
            return Err("WAKE_LEDGER_INVENTORY_UNAVAILABLE".into());
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(aligned.as_ptr().cast::<u8>(), status.information)
        };
        let next_offset = u32::from_ne_bytes(bytes[..4].try_into().unwrap());
        let name_length = u32::from_ne_bytes(bytes[8..12].try_into().unwrap()) as usize;
        if next_offset != 0
            || name_length == 0
            || !name_length.is_multiple_of(2)
            || name_length > bytes.len() - 12
        {
            return Err("WAKE_LEDGER_INVENTORY_UNAVAILABLE".into());
        }
        let units: Vec<u16> = bytes[12..12 + name_length]
            .chunks_exact(2)
            .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
            .collect();
        let name = String::from_utf16(&units).map_err(|_| "WAKE_LEDGER_RECORD_CORRUPT")?;
        let (generation, phase) = parse_record_name(&name)?;
        let slots = found.entry(generation).or_insert([false; 8]);
        if slots[phase.index()] {
            return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
        }
        slots[phase.index()] = true;
    }
    Err("WAKE_LEDGER_INVENTORY_UNAVAILABLE".into())
}

fn canonical_bytes(record: &LedgerRecordV1) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(record).map_err(|_| "WAKE_LEDGER_ENCODING_INVALID")?;
    if bytes.is_empty() || bytes.len() > MAX_RECORD_BYTES {
        return Err("WAKE_LEDGER_ENCODING_INVALID".into());
    }
    Ok(bytes)
}

fn record_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn read_record(file: &mut File) -> Result<(LedgerRecordV1, String)> {
    file_info(file, None)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    if bytes.is_empty() || bytes.len() > MAX_RECORD_BYTES {
        return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
    }
    file_info(file, Some(bytes.len()))?;
    let actual: LedgerRecordV1 =
        serde_json::from_slice(&bytes).map_err(|_| "WAKE_LEDGER_RECORD_CORRUPT")?;
    if serde_json::to_vec(&actual).map_err(|_| "WAKE_LEDGER_RECORD_CORRUPT")? != bytes {
        return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
    }
    Ok((actual, record_digest(&bytes)))
}

fn read_exact(file: &mut File, expected: &LedgerRecordV1) -> Result<String> {
    let (actual, digest) = read_record(file)?;
    if actual != *expected {
        return Err("WAKE_LEDGER_APPROVAL_CONFLICT".into());
    }
    Ok(digest)
}

fn historical_intent(
    root: &OwnedHandle,
    generation: u64,
    present: &[bool; 8],
    observed_root: &ObjectIdentityV1,
) -> Result<(
    LedgerRecordV1,
    InstallPhaseV1,
    Option<StageBindingV1>,
    Option<PointerSwitchBindingV1>,
)> {
    if !present[0] {
        return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
    }
    let mut intent_file = open_optional(root, &record_name(generation, InstallPhaseV1::Intent))?
        .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
    let (intent, mut digest) = read_record(&mut intent_file)?;
    if intent.schema_version != 1
        || intent.purpose != "catdesk-wake-protected-install-transaction"
        || intent.install_generation != generation
        || intent.phase != InstallPhaseV1::Intent.name()
        || intent.prior_record_sha256 != "0".repeat(64)
        || intent.protected_root != *observed_root
        || !lowercase_hex(&intent.approval_id, 32)
        || !lowercase_hex(&intent.signed_approval_sha256, 64)
        || !intent.approval_continuity.valid_for(observed_root)
        || !lowercase_hex(&intent.manifest_sha256, 64)
        || intent.accepted_at_utc_seconds == 0
    {
        return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
    }
    if intent.stage_binding.is_some() || intent.pointer_switch_binding.is_some() {
        return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
    }
    let mut binding = Binding {
        install_generation: intent.install_generation,
        approval_id: intent.approval_id.clone(),
        signed_approval_sha256: intent.signed_approval_sha256.clone(),
        approval_continuity: intent.approval_continuity.clone(),
        accepted_at_utc_seconds: intent.accepted_at_utc_seconds,
        package_id: intent.package_id.clone(),
        manifest_sha256: intent.manifest_sha256.clone(),
        predecessor_package_id: intent.predecessor_package_id.clone(),
        predecessor_pointer: intent.predecessor_pointer.clone(),
        protected_root: intent.protected_root.clone(),
        old_runtime_root: intent.old_runtime_root.clone(),
        old_runtime_lock: intent.old_runtime_lock.clone(),
        old_host_image: intent.old_host_image.clone(),
        old_host_image_sha256: intent.old_host_image_sha256.clone(),
        old_host_pid: intent.old_host_pid,
        old_host_creation_time_100ns: intent.old_host_creation_time_100ns,
        original_desired_state: intent.original_desired_state,
        stage_binding: None,
        pointer_switch_binding: None,
    };
    let mut current = InstallPhaseV1::Intent;
    let mut gap = false;
    for phase in InstallPhaseV1::ORDER.iter().copied().skip(1) {
        if !present[phase.index()] {
            gap = true;
            continue;
        }
        if gap {
            return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
        }
        let mut file = open_optional(root, &record_name(generation, phase))?
            .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
        let (record, next_digest) = read_record(&mut file)?;
        if phase == InstallPhaseV1::Staged {
            let stage = record
                .stage_binding
                .as_ref()
                .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
            if !valid_stage_binding(stage, observed_root) {
                return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
            }
            binding.stage_binding = Some(stage.clone());
        }
        if phase == InstallPhaseV1::PointerSwitched
            && let Some(pointer) = record.pointer_switch_binding.as_ref()
        {
            let stage = binding
                .stage_binding
                .as_ref()
                .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
            if !valid_pointer_switch_binding(
                pointer,
                observed_root,
                &binding.predecessor_pointer,
                stage,
            ) {
                return Err("WAKE_LEDGER_POINTER_BINDING_INVALID".into());
            }
            binding.pointer_switch_binding = Some(pointer.clone());
        }
        if record != binding.record(phase, &digest) {
            return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
        }
        digest = next_digest;
        current = phase;
    }
    Ok((
        intent,
        current,
        binding.stage_binding,
        binding.pointer_switch_binding,
    ))
}

/// Read the rollback floor only from a fully chained protected history. The
/// caller holds the global lock and has bound this root to protected policy.
fn validated_latest_accepted_at(
    root: &OwnedHandle,
    root_id: &ObjectIdentityV1,
) -> Result<Option<u64>> {
    let inventory = ledger_inventory(root)?;
    let mut last_generation: Option<u64> = None;
    let mut last_accepted: Option<u64> = None;
    let mut last_phase = InstallPhaseV1::Complete;
    let mut approval_ids = HashSet::new();
    let mut approval_digests = HashSet::new();
    for (&generation, present) in &inventory {
        if last_generation.is_some_and(|prior| prior.checked_add(1) != Some(generation)) {
            return Err("WAKE_LEDGER_STALE_GENERATION".into());
        }
        if last_phase != InstallPhaseV1::Complete {
            return Err("WAKE_LEDGER_PRIOR_INCOMPLETE".into());
        }
        let (intent, phase, _, _) = historical_intent(root, generation, present, root_id)?;
        if !approval_ids.insert(intent.approval_id.clone())
            || !approval_digests.insert(intent.signed_approval_sha256.clone())
        {
            return Err("WAKE_LEDGER_APPROVAL_REPLAY".into());
        }
        if last_accepted.is_some_and(|prior| intent.accepted_at_utc_seconds < prior) {
            return Err("WAKE_LEDGER_CLOCK_ROLLBACK".into());
        }
        last_generation = Some(generation);
        last_accepted = Some(intent.accepted_at_utc_seconds);
        last_phase = phase;
    }
    if ledger_inventory(root)? != inventory {
        return Err("WAKE_LEDGER_INVENTORY_CHANGED".into());
    }
    Ok(last_accepted)
}

type CompletedHeadV1 = (
    BTreeMap<u64, [bool; 8]>,
    LedgerRecordV1,
    StageBindingV1,
    Option<PointerSwitchBindingV1>,
);

fn completed_head(root: &OwnedHandle, root_id: &ObjectIdentityV1) -> Result<CompletedHeadV1> {
    let inventory = ledger_inventory(root)?;
    let (&generation, present) = inventory
        .last_key_value()
        .ok_or("WAKE_BROKER_PREDECESSOR_UNANCHORED")?;
    // This validates the whole contiguous, replay-free, time-ordered history,
    // not just the final record. It also checks a second inventory readback.
    validated_latest_accepted_at(root, root_id)?;
    let mut prior_package: Option<String> = None;
    let mut prior_pointer: Option<ObjectIdentityV1> = None;
    for (&past_generation, past_present) in &inventory {
        let (past, _, _, switched_pointer) =
            historical_intent(root, past_generation, past_present, root_id)?;
        if prior_package
            .as_ref()
            .is_some_and(|package| package != &past.predecessor_package_id)
            || prior_pointer
                .as_ref()
                .is_some_and(|pointer| pointer != &past.predecessor_pointer)
        {
            return Err("WAKE_BROKER_PREDECESSOR_HISTORY_MISMATCH".into());
        }
        prior_package = Some(past.package_id);
        prior_pointer = switched_pointer.map(|pointer| pointer.pointer_file);
    }
    let (head, phase, stage, pointer) = historical_intent(root, generation, present, root_id)?;
    if phase != InstallPhaseV1::Complete {
        return Err("WAKE_BROKER_PREDECESSOR_INCOMPLETE".into());
    }
    let stage = stage.ok_or("WAKE_LEDGER_STAGE_BINDING_INVALID")?;
    if ledger_inventory(root)? != inventory {
        return Err("WAKE_LEDGER_INVENTORY_CHANGED".into());
    }
    Ok((inventory, head, stage, pointer))
}

fn compare_completed_head(
    head: &LedgerRecordV1,
    stage: &StageBindingV1,
    pointer_binding: Option<&PointerSwitchBindingV1>,
    snapshot: &ProtectedInstallReadOnlySnapshotV1,
    pointer_sha256: &str,
) -> Result<()> {
    if snapshot.install_generation != head.install_generation
        || snapshot.approval_id != head.approval_id
        || snapshot.predecessor_package_id != head.predecessor_package_id
        || snapshot.predecessor_pointer != head.predecessor_pointer
        || snapshot.package_id != head.package_id
        || snapshot.manifest_sha256 != head.manifest_sha256
        || snapshot.protected_root != head.protected_root
        || snapshot.package_directory != stage.candidate_directory
        || snapshot.manifest_file != stage.manifest_file
        || snapshot
            .artifacts
            .iter()
            .zip(&stage.artifacts)
            .any(|(artifact, recorded)| artifact.object != *recorded)
    {
        return Err("WAKE_BROKER_PREDECESSOR_MISMATCH".into());
    }
    if pointer_binding.is_some_and(|binding| {
        snapshot.pointer != binding.pointer_file || pointer_sha256 != binding.pointer_sha256
    }) {
        return Err("WAKE_BROKER_PREDECESSOR_POINTER_MISMATCH".into());
    }
    Ok(())
}

/// Compare only the proposed successor fields. The caller separately pins and
/// revalidates the physical predecessor under the global lock; a match here
/// never proves migration or the old live owner.
fn compare_fresh_successor(
    predecessor: &BrokerCompletedPredecessorReadbackV1,
    payload: &WakeInstallApprovalV1,
    expected: &LedgerExpectationV1,
) -> Result<()> {
    if !predecessor.pointer_switch_record_binding_present {
        return Err("WAKE_BROKER_PREDECESSOR_POINTER_UNBOUND".into());
    }
    let next_generation = predecessor
        .install_generation
        .checked_add(1)
        .ok_or("WAKE_LEDGER_STALE_GENERATION")?;
    let installed = &predecessor.protected_install;
    if expected.next_install_generation != next_generation
        || payload.install_generation != next_generation
        || expected.predecessor_package_id != installed.package_id
        || payload.predecessor_package_id != installed.package_id
        || expected.predecessor_pointer != installed.pointer
        || payload.predecessor_pointer != installed.pointer
        || expected.protected_root != installed.protected_root
        || payload.target_protected_root != installed.protected_root
    {
        return Err("WAKE_BROKER_PREDECESSOR_SUCCESSOR_MISMATCH".into());
    }
    Ok(())
}

/// Validate every prior ledger generation and reject later records or reused
/// approval IDs before creating a fresh intent. The fixed global lease is
/// already held by the caller.
fn preflight_history(
    root: &OwnedHandle,
    binding: &Binding,
    observed_root: &ObjectIdentityV1,
) -> Result<bool> {
    let inventory = ledger_inventory(root)?;
    let mut last_prior: Option<u64> = None;
    let mut same_generation_exists = false;
    let mut seen_approval_ids = HashSet::new();
    let mut seen_approval_digests = HashSet::new();
    let mut prior_output: Option<(String, ObjectIdentityV1)> = None;
    for (generation, present) in inventory {
        if generation > binding.install_generation {
            return Err("WAKE_LEDGER_STALE_GENERATION".into());
        }
        if last_prior.is_some_and(|prior| prior.checked_add(1) != Some(generation)) {
            return Err("WAKE_LEDGER_STALE_GENERATION".into());
        }
        let (intent, phase, _, pointer) =
            historical_intent(root, generation, &present, observed_root)?;
        if prior_output.as_ref().is_some_and(|(package, pointer)| {
            intent.predecessor_package_id != *package || intent.predecessor_pointer != *pointer
        }) {
            return Err("WAKE_LEDGER_PREDECESSOR_MISMATCH".into());
        }
        if !seen_approval_ids.insert(intent.approval_id.clone())
            || !seen_approval_digests.insert(intent.signed_approval_sha256.clone())
        {
            return Err("WAKE_LEDGER_APPROVAL_REPLAY".into());
        }
        if generation == binding.install_generation {
            same_generation_exists = true;
        } else {
            if phase != InstallPhaseV1::Complete {
                return Err("WAKE_LEDGER_PRIOR_INCOMPLETE".into());
            }
            let pointer = pointer.ok_or("WAKE_LEDGER_PRIOR_POINTER_UNBOUND")?;
            if intent.approval_id == binding.approval_id
                || intent.signed_approval_sha256 == binding.signed_approval_sha256
            {
                return Err("WAKE_LEDGER_APPROVAL_REPLAY".into());
            }
            last_prior = Some(generation);
            prior_output = Some((intent.package_id, pointer.pointer_file));
        }
    }
    if last_prior.is_some_and(|prior| prior.checked_add(1) != Some(binding.install_generation)) {
        return Err("WAKE_LEDGER_STALE_GENERATION".into());
    }
    if prior_output.as_ref().is_some_and(|(package, pointer)| {
        binding.predecessor_package_id != *package || binding.predecessor_pointer != *pointer
    }) {
        return Err("WAKE_LEDGER_PREDECESSOR_MISMATCH".into());
    }
    Ok(same_generation_exists)
}

fn read_intent_for_resume(root: &OwnedHandle, generation: u64) -> Result<Option<LedgerRecordV1>> {
    if generation == 0 {
        return Err("WAKE_LEDGER_GENERATION_INVALID".into());
    }
    let observed_root = root_info(root)?;
    let _global_lock = acquire_global_lock(root)?;
    let inventory = ledger_inventory(root)?;
    let Some(present) = inventory.get(&generation) else {
        return Ok(None);
    };
    let (intent, _, _, _) = historical_intent(root, generation, present, &observed_root)?;
    Ok(Some(intent))
}

fn original_observation(intent: &LedgerRecordV1) -> BrokerObservedStateV1 {
    BrokerObservedStateV1 {
        now_utc_seconds: intent.accepted_at_utc_seconds,
        next_install_generation: intent.install_generation,
        predecessor_package_id: intent.predecessor_package_id.clone(),
        predecessor_pointer: intent.predecessor_pointer.clone(),
        target_protected_root: intent.protected_root.clone(),
        old_runtime_root: intent.old_runtime_root.clone(),
        old_runtime_lock: intent.old_runtime_lock.clone(),
        old_host_image: intent.old_host_image.clone(),
        old_host_image_sha256: intent.old_host_image_sha256.clone(),
        old_host_pid: intent.old_host_pid,
        old_host_creation_time_100ns: intent.old_host_creation_time_100ns,
    }
}

/// Reverify the original signed envelope against the protected intent's first
/// acceptance time and original physical observations. The broker must get
/// `now_utc_seconds` from its own clock and separately reconcile the current
/// physical state for the recorded phase. `begin` then rechecks the signed
/// digest and record; this function never creates an intent or mutates install
/// state. Its returned expectation is drawn from the protected original record.
pub fn verify_exact_resume(
    root: &OwnedHandle,
    envelope_bytes: &[u8],
    broker_trust_root: &VerifyingKey,
    generation: u64,
    now_utc_seconds: u64,
) -> Result<(VerifiedApprovalV1, LedgerExpectationV1)> {
    let intent = read_intent_for_resume(root, generation)?.ok_or("WAKE_LEDGER_NO_PRIOR_INTENT")?;
    if now_utc_seconds < intent.accepted_at_utc_seconds {
        return Err("WAKE_LEDGER_CLOCK_ROLLBACK".into());
    }
    let original = original_observation(&intent);
    let approval = verify_approval(envelope_bytes, broker_trust_root, &original)?;
    if approval.signed_payload_sha256() != intent.signed_approval_sha256
        || approval.signed_envelope_sha256() != intent.approval_continuity.signed_envelope_sha256
        || approval.verification_key_sha256()
            != intent.approval_continuity.approval_public_key_sha256
    {
        return Err("WAKE_LEDGER_APPROVAL_CONFLICT".into());
    }
    let expectation = LedgerExpectationV1 {
        now_utc_seconds,
        next_install_generation: intent.install_generation,
        predecessor_package_id: intent.predecessor_package_id,
        predecessor_pointer: intent.predecessor_pointer,
        protected_root: intent.protected_root,
        original_desired_state: intent.original_desired_state,
    };
    Ok((approval, expectation))
}

fn write_new(file: &mut File, expected: &LedgerRecordV1) -> Result<String> {
    file_info(file, Some(0))?;
    let bytes = canonical_bytes(expected)?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "WAKE_LEDGER_WRITE_FAILED")?;
    let digest = read_exact(file, expected)?;
    if digest != record_digest(&bytes) {
        return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
    }
    Ok(digest)
}

impl<'a> BrokerLedgerGuard<'a> {
    /// Invoke the observation provider only while this global lock is held,
    /// then verify the envelope against the pinned protected policy. A fresh
    /// successor must match a retained, bound completed pointer and package.
    /// The first dev.84 migration anchor and live old-owner evidence remain
    /// unavailable, so this production path refuses before INTENT. Once those
    /// proofs exist, exact CI must still be fetched from the fixed GitHub
    /// origin before creating INTENT. A closure cannot prove provenance.
    pub fn accept_with_observation<'p>(
        self,
        policy: &'p BrokerProtectedPolicyV1,
        envelope: &[u8],
        observe: impl FnOnce(&OwnedHandle) -> Result<(BrokerObservedStateV1, LedgerExpectationV1)>,
    ) -> Result<(BrokerLedgerLease<'a>, PolicyVerifiedApprovalV1<'p>)> {
        self.accept_with_guarded_predecessor_mode(
            policy,
            envelope,
            observe,
            PredecessorReaderMode::Protected,
        )
    }

    #[cfg(test)]
    fn accept_with_guarded_predecessor_fixture<'p>(
        self,
        policy: &'p BrokerProtectedPolicyV1,
        envelope: &[u8],
        observe: impl FnOnce(&OwnedHandle) -> Result<(BrokerObservedStateV1, LedgerExpectationV1)>,
    ) -> Result<(BrokerLedgerLease<'a>, PolicyVerifiedApprovalV1<'p>)> {
        self.accept_with_guarded_predecessor_mode(
            policy,
            envelope,
            observe,
            PredecessorReaderMode::Fixture,
        )
    }

    fn accept_with_guarded_predecessor_mode<'p>(
        self,
        policy: &'p BrokerProtectedPolicyV1,
        envelope: &[u8],
        observe: impl FnOnce(&OwnedHandle) -> Result<(BrokerObservedStateV1, LedgerExpectationV1)>,
        reader_mode: PredecessorReaderMode,
    ) -> Result<(BrokerLedgerLease<'a>, PolicyVerifiedApprovalV1<'p>)> {
        let (observed, expected) = observe(self.root)?;
        let approved = policy.verify_approval_for_transaction(envelope, &observed)?;
        let (predecessor, pin) =
            self.pin_fresh_successor_predecessor_with(policy, &approved, &expected, reader_mode)?;
        if !predecessor.first_generation_migration_anchor_verified {
            return Err("WAKE_BROKER_PREDECESSOR_MIGRATION_UNANCHORED".into());
        }
        if !predecessor.old_live_owner_verified {
            return Err("WAKE_BROKER_OLD_OWNER_UNVERIFIED".into());
        }
        require_broker_owned_exact_ci(&approved)?;
        pin.revalidate(self.root, &predecessor.protected_install.protected_root)?;
        let lease = self.begin(&approved, &expected)?;
        pin.revalidate(
            lease.protected_root(),
            &predecessor.protected_install.protected_root,
        )?;
        Ok((lease, approved))
    }

    #[cfg(test)]
    fn accept_with_observation_using<'p>(
        self,
        policy: &'p BrokerProtectedPolicyV1,
        envelope: &[u8],
        observe: impl FnOnce(&OwnedHandle) -> Result<(BrokerObservedStateV1, LedgerExpectationV1)>,
        check_ci: impl FnOnce(&PolicyVerifiedApprovalV1<'_>) -> Result<()>,
    ) -> Result<(BrokerLedgerLease<'a>, PolicyVerifiedApprovalV1<'p>)> {
        let (observed, expected) = observe(self.root)?;
        let approved = policy.verify_approval_for_transaction(envelope, &observed)?;
        // Test-only CI seam: a failing check returns before fixture INTENT.
        // Production requires the separate predecessor and owner gates above.
        check_ci(&approved)?;
        let lease = self.begin(&approved, &expected)?;
        Ok((lease, approved))
    }

    #[cfg(test)]
    pub(crate) fn accept_with_observation_fixture<'p>(
        self,
        policy: &'p BrokerProtectedPolicyV1,
        envelope: &[u8],
        observe: impl FnOnce(&OwnedHandle) -> Result<(BrokerObservedStateV1, LedgerExpectationV1)>,
    ) -> Result<(BrokerLedgerLease<'a>, PolicyVerifiedApprovalV1<'p>)> {
        self.accept_with_observation_using(policy, envelope, observe, |_| Ok(()))
    }

    #[cfg(test)]
    pub(crate) fn accept_with_observation_ci_rejection_fixture<'p>(
        self,
        policy: &'p BrokerProtectedPolicyV1,
        envelope: &[u8],
        observe: impl FnOnce(&OwnedHandle) -> Result<(BrokerObservedStateV1, LedgerExpectationV1)>,
    ) -> Result<(BrokerLedgerLease<'a>, PolicyVerifiedApprovalV1<'p>)> {
        self.accept_with_observation_using(policy, envelope, observe, |_| {
            Err("WAKE_BROKER_CI_FIXTURE_DENIED".into())
        })
    }

    fn begin(
        self,
        approved: &PolicyVerifiedApprovalV1<'_>,
        expected: &LedgerExpectationV1,
    ) -> Result<BrokerLedgerLease<'a>> {
        let policy = approved.policy();
        let policy_root = policy.readback()?.protected_root;
        if root_info(self.root)? != policy_root {
            return Err("WAKE_LEDGER_ROOT_CHANGED".into());
        }
        let lease = self.begin_verified(
            approved.verified(),
            expected,
            ApprovalContinuityV1::from_policy(approved),
        )?;
        policy.revalidate()?;
        Ok(lease)
    }

    #[cfg(test)]
    pub(crate) fn begin_fixture(
        self,
        approval: &VerifiedApprovalV1,
        expected: &LedgerExpectationV1,
    ) -> Result<BrokerLedgerLease<'a>> {
        self.begin_verified(
            approval,
            expected,
            ApprovalContinuityV1::fixture(approval, &expected.protected_root),
        )
    }

    fn begin_verified(
        self,
        approval: &VerifiedApprovalV1,
        expected: &LedgerExpectationV1,
        continuity: ApprovalContinuityV1,
    ) -> Result<BrokerLedgerLease<'a>> {
        let root = self.root;
        let payload = approval.payload();
        let observed_root = root_info(root)?;
        if expected.next_install_generation == 0
            || expected.now_utc_seconds < approval.verified_at_utc_seconds()
            || payload.install_generation != expected.next_install_generation
            || payload.predecessor_package_id != expected.predecessor_package_id
            || payload.predecessor_pointer != expected.predecessor_pointer
            || payload.target_protected_root != expected.protected_root
            || observed_root != expected.protected_root
            || !lowercase_hex(approval.signed_payload_sha256(), 64)
            || !continuity.valid_for(&observed_root)
            || continuity.signed_envelope_sha256 != approval.signed_envelope_sha256()
            || continuity.approval_public_key_sha256 != approval.verification_key_sha256()
        {
            return Err("WAKE_LEDGER_PREDECESSOR_MISMATCH".into());
        }
        let global_lock = self.global_lock;
        let binding = Binding {
            install_generation: payload.install_generation,
            approval_id: payload.approval_id.clone(),
            signed_approval_sha256: approval.signed_payload_sha256().into(),
            approval_continuity: continuity,
            accepted_at_utc_seconds: approval.verified_at_utc_seconds(),
            package_id: payload.manifest.package_id.clone(),
            manifest_sha256: payload.manifest_sha256.clone(),
            predecessor_package_id: payload.predecessor_package_id.clone(),
            predecessor_pointer: payload.predecessor_pointer.clone(),
            protected_root: payload.target_protected_root.clone(),
            old_runtime_root: payload.old_runtime_root.clone(),
            old_runtime_lock: payload.old_runtime_lock.clone(),
            old_host_image: payload.old_host_image.clone(),
            old_host_image_sha256: payload.old_host_image_sha256.clone(),
            old_host_pid: payload.old_host_pid,
            old_host_creation_time_100ns: payload.old_host_creation_time_100ns,
            original_desired_state: expected.original_desired_state,
            stage_binding: None,
            pointer_switch_binding: None,
        };
        let existing = preflight_history(root, &binding, &observed_root)?;
        if !existing
            && (expected.now_utc_seconds != approval.verified_at_utc_seconds()
                || expected.now_utc_seconds < payload.issued_utc_seconds
                || expected.now_utc_seconds > payload.expires_utc_seconds)
        {
            return Err("WAKE_LEDGER_FRESH_APPROVAL_EXPIRED".into());
        }
        let intent_name = record_name(binding.install_generation, InstallPhaseV1::Intent);
        let (mut intent_file, newly_created) = if existing {
            let file =
                open_optional(root, &intent_name)?.ok_or("WAKE_LEDGER_RECORD_UNAVAILABLE")?;
            (file, false)
        } else {
            let file = match open_relative(root, &intent_name, FILE_CREATE) {
                Ok(file) => file,
                Err(STATUS_SHARING_VIOLATION | STATUS_OBJECT_NAME_COLLISION) => {
                    return Err("WAKE_LEDGER_CONTENDER_BUSY".into());
                }
                Err(_) => return Err("WAKE_LEDGER_CREATE_FAILED".into()),
            };
            (file, true)
        };
        let intent = binding.record(InstallPhaseV1::Intent, &"0".repeat(64));
        let mut digest = if newly_created {
            write_new(&mut intent_file, &intent)?
        } else {
            read_exact(&mut intent_file, &intent)?
        };
        // The record bytes alone do not persist the new directory entry. On
        // reopen this also completes a prior failed parent flush before any
        // later stage mutation can be considered.
        flush_pinned_directory(root, &observed_root, ParentFlushPoint::Intent)?;
        let mut current = InstallPhaseV1::Intent;
        let mut binding = binding;
        let mut gap = false;
        for phase in InstallPhaseV1::ORDER.iter().copied().skip(1) {
            let name = record_name(binding.install_generation, phase);
            match open_optional(root, &name)? {
                Some(mut file) => {
                    if newly_created || gap {
                        return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
                    }
                    let (record, next_digest) = read_record(&mut file)?;
                    if phase == InstallPhaseV1::Staged {
                        let stage = record
                            .stage_binding
                            .as_ref()
                            .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
                        if !valid_stage_binding(stage, &observed_root) {
                            return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
                        }
                        binding.stage_binding = Some(stage.clone());
                    }
                    if phase == InstallPhaseV1::PointerSwitched
                        && let Some(pointer) = record.pointer_switch_binding.as_ref()
                    {
                        let stage = binding
                            .stage_binding
                            .as_ref()
                            .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
                        if !valid_pointer_switch_binding(
                            pointer,
                            &observed_root,
                            &binding.predecessor_pointer,
                            stage,
                        ) {
                            return Err("WAKE_LEDGER_POINTER_BINDING_INVALID".into());
                        }
                        binding.pointer_switch_binding = Some(pointer.clone());
                    }
                    if record != binding.record(phase, &digest) {
                        return Err("WAKE_LEDGER_APPROVAL_CONFLICT".into());
                    }
                    digest = next_digest;
                    current = phase;
                }
                None => gap = true,
            }
        }
        if root_info(root)? != observed_root {
            return Err("WAKE_LEDGER_ROOT_CHANGED".into());
        }
        Ok(BrokerLedgerLease {
            root,
            _global_lock: global_lock,
            _intent_file: intent_file,
            binding,
            current,
            current_digest: digest,
        })
    }
}

impl<'a> BrokerLedgerLease<'a> {
    #[cfg(test)]
    fn begin(
        root: &'a OwnedHandle,
        approval: &VerifiedApprovalV1,
        expected: &LedgerExpectationV1,
    ) -> Result<Self> {
        BrokerLedgerGuard::acquire(root)?.begin_fixture(approval, expected)
    }

    pub fn phase(&self) -> InstallPhaseV1 {
        self.current
    }

    pub(crate) fn protected_root(&self) -> &OwnedHandle {
        self.root
    }

    pub(crate) fn matches_approval(&self, approval: &VerifiedApprovalV1) -> bool {
        let payload = approval.payload();
        self.binding.install_generation == payload.install_generation
            && self.binding.approval_id == payload.approval_id
            && self.binding.signed_approval_sha256 == approval.signed_payload_sha256()
            && self.binding.approval_continuity.signed_envelope_sha256
                == approval.signed_envelope_sha256()
            && self.binding.approval_continuity.approval_public_key_sha256
                == approval.verification_key_sha256()
            && self.binding.manifest_sha256 == payload.manifest_sha256
            && self.binding.package_id == payload.manifest.package_id
            && self.binding.protected_root == payload.target_protected_root
    }

    pub(crate) fn matches_policy_approval(&self, approved: &PolicyVerifiedApprovalV1<'_>) -> bool {
        self.matches_approval(approved.verified())
            && self.binding.approval_continuity == ApprovalContinuityV1::from_policy(approved)
    }

    pub(crate) fn approval_continuity(&self) -> &ApprovalContinuityV1 {
        &self.binding.approval_continuity
    }

    /// Durable write-ahead marker for the first protected candidate create.
    /// An existing marker, including one left by a failed call or crash, is
    /// sticky attention; no second stage create is authorized from it.
    pub(crate) fn record_stage_started(&mut self) -> Result<()> {
        if self.current != InstallPhaseV1::Intent || self.binding.stage_binding.is_some() {
            return Err("WAKE_LEDGER_STAGE_ATTENTION".into());
        }
        if !self.advance_internal(InstallPhaseV1::StageStarted)? {
            return Err("WAKE_LEDGER_STAGE_ATTENTION".into());
        }
        Ok(())
    }

    /// The only production transition into STAGED. The opaque receipt owns
    /// freshly created stage pins and rechecks all bytes/IDs while this lease
    /// still holds the pre-provisioned global lock.
    pub fn record_staged(&mut self, receipt: &mut BrokerStageReceiptV1<'_>) -> Result<bool> {
        if self.current != InstallPhaseV1::StageStarted || self.binding.stage_binding.is_some() {
            return Err("WAKE_LEDGER_PHASE_ORDER_INVALID".into());
        }
        receipt.revalidate_for_ledger(
            self.root,
            &self.binding.protected_root,
            self.binding.install_generation,
            &self.binding.approval_id,
            &self.binding.signed_approval_sha256,
            &self.binding.approval_continuity,
        )?;
        let stage = receipt.stage_binding();
        if !valid_stage_binding(&stage, &self.binding.protected_root) {
            return Err("WAKE_LEDGER_STAGE_BINDING_INVALID".into());
        }
        self.binding.stage_binding = Some(stage);
        let written = self.advance_internal(InstallPhaseV1::Staged)?;
        // The receipt retains every stage pin across the ledger write. A
        // failed post-write check leaves the durable record for exact-only
        // diagnosis; it never authorizes publication or silent cleanup.
        receipt.revalidate_for_ledger(
            self.root,
            &self.binding.protected_root,
            self.binding.install_generation,
            &self.binding.approval_id,
            &self.binding.signed_approval_sha256,
            &self.binding.approval_continuity,
        )?;
        Ok(written)
    }

    #[cfg(test)]
    pub(crate) fn record_staged_fixture(&mut self, stage: StageBindingV1) -> Result<bool> {
        if self.current != InstallPhaseV1::StageStarted
            || self.binding.stage_binding.is_some()
            || !valid_stage_binding(&stage, &self.binding.protected_root)
        {
            return Err("WAKE_LEDGER_STAGE_BINDING_INVALID".into());
        }
        self.binding.stage_binding = Some(stage);
        self.advance_internal(InstallPhaseV1::Staged)
    }

    #[cfg(test)]
    pub(crate) fn advance(&mut self, next: InstallPhaseV1) -> Result<bool> {
        if next == InstallPhaseV1::Staged && self.binding.stage_binding.is_none() {
            // Ledger-only fixtures model a physically bound stage. Production
            // has no generic phase advancement path.
            let id = |offset: u32| ObjectIdentityV1 {
                volume_serial: self.binding.protected_root.volume_serial,
                file_index_high: self.binding.protected_root.file_index_high,
                file_index_low: self
                    .binding
                    .protected_root
                    .file_index_low
                    .wrapping_add(offset),
            };
            self.binding.stage_binding = Some(StageBindingV1 {
                staging_directory: id(1),
                packages_directory: None,
                candidate_directory: id(2),
                manifest_file: id(3),
                artifacts: [id(4), id(5), id(6), id(7)],
            });
        }
        if next == InstallPhaseV1::PointerSwitched && self.binding.pointer_switch_binding.is_none()
        {
            // Ledger-only tests model the future writer's private pointer
            // receipt. This fabricated ID never enters a production record.
            let stage = self.binding.stage_binding.as_ref().expect("fixture stage");
            let mut pointer = self.binding.protected_root.clone();
            for offset in 8..=32 {
                pointer.file_index_low = self
                    .binding
                    .protected_root
                    .file_index_low
                    .wrapping_add(offset);
                let candidate = PointerSwitchBindingV1 {
                    pointer_file: pointer.clone(),
                    pointer_sha256: "1".repeat(64),
                };
                if valid_pointer_switch_binding(
                    &candidate,
                    &self.binding.protected_root,
                    &self.binding.predecessor_pointer,
                    stage,
                ) {
                    self.binding.pointer_switch_binding = Some(candidate);
                    break;
                }
            }
            if self.binding.pointer_switch_binding.is_none() {
                return Err("WAKE_LEDGER_POINTER_BINDING_INVALID".into());
            }
        }
        self.advance_internal(next)
    }

    #[cfg(test)]
    fn advance_legacy_without_pointer(&mut self, next: InstallPhaseV1) -> Result<bool> {
        if next == InstallPhaseV1::PointerSwitched {
            self.advance_internal(next)
        } else {
            self.advance(next)
        }
    }

    #[cfg(test)]
    fn record_pointer_switched_fixture(&mut self, pointer: PointerSwitchBindingV1) -> Result<bool> {
        if self.current != InstallPhaseV1::OldOwnerStopped
            || self.binding.pointer_switch_binding.is_some()
            || !self.binding.stage_binding.as_ref().is_some_and(|stage| {
                valid_pointer_switch_binding(
                    &pointer,
                    &self.binding.protected_root,
                    &self.binding.predecessor_pointer,
                    stage,
                )
            })
        {
            return Err("WAKE_LEDGER_POINTER_BINDING_INVALID".into());
        }
        self.binding.pointer_switch_binding = Some(pointer);
        self.advance_internal(InstallPhaseV1::PointerSwitched)
    }

    fn advance_internal(&mut self, next: InstallPhaseV1) -> Result<bool> {
        if next != self.current && next.index() != self.current.index() + 1 {
            return Err("WAKE_LEDGER_PHASE_ORDER_INVALID".into());
        }
        if root_info(self.root)? != self.binding.protected_root {
            return Err("WAKE_LEDGER_ROOT_CHANGED".into());
        }
        let current_digest = if self.current == InstallPhaseV1::Intent {
            read_record(&mut self._intent_file)?.1
        } else {
            let name = record_name(self.binding.install_generation, self.current);
            let mut file = open_optional(self.root, &name)?.ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
            read_record(&mut file)?.1
        };
        if current_digest != self.current_digest {
            return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
        }
        if next == self.current {
            return Ok(false);
        }
        let record = self.binding.record(next, &self.current_digest);
        let name = record_name(self.binding.install_generation, next);
        let (mut file, newly_created) = create_or_reopen(self.root, &name)?;
        let digest = if newly_created {
            write_new(&mut file, &record)?
        } else {
            read_exact(&mut file, &record)?
        };
        let point = match next {
            InstallPhaseV1::Intent => ParentFlushPoint::Intent,
            InstallPhaseV1::StageStarted => ParentFlushPoint::StageStarted,
            InstallPhaseV1::Staged => ParentFlushPoint::Staged,
            _ => ParentFlushPoint::LaterLedgerPhase,
        };
        flush_pinned_directory(self.root, &self.binding.protected_root, point)?;
        if root_info(self.root)? != self.binding.protected_root {
            return Err("WAKE_LEDGER_ROOT_CHANGED".into());
        }
        self.current = next;
        self.current_digest = digest;
        Ok(newly_created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_broker_approval::{tests as approval_test, verify_approval};
    use crate::install_broker_durability::fail_next_flush;
    use std::{
        os::windows::{ffi::OsStrExt, io::FromRawHandle},
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(1);
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const OPEN_EXISTING: u32 = 3;
    const READ_CONTROL: u32 = 0x0002_0000;
    #[link(name = "kernel32")]
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
    }

    fn fixture() -> (
        PathBuf,
        OwnedHandle,
        VerifiedApprovalV1,
        LedgerExpectationV1,
    ) {
        let root = std::env::temp_dir().join(format!(
            "catdesk-wake-ledger-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).expect("create fixture");
        std::fs::write(root.join(GLOBAL_LOCK_NAME), b"").expect("pre-provision fixture lock");
        let wide: Vec<u16> = root
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let raw = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        assert!(!raw.is_null() && raw != INVALID_HANDLE_VALUE);
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let root_id = root_info(&handle).expect("root object");
        let (mut payload, mut observed, key) = approval_test::fixture();
        payload.target_protected_root = root_id.clone();
        observed.target_protected_root = root_id.clone();
        let envelope = approval_test::envelope(&payload, &key);
        let approval = verify_approval(&envelope, &key.verifying_key(), &observed)
            .expect("real signed fixture approval");
        let expected = LedgerExpectationV1 {
            now_utc_seconds: observed.now_utc_seconds,
            next_install_generation: payload.install_generation,
            predecessor_package_id: payload.predecessor_package_id,
            predecessor_pointer: payload.predecessor_pointer,
            protected_root: root_id,
            original_desired_state: OriginalDesiredStateV1::Running,
        };
        (root, handle, approval, expected)
    }

    fn remove_generation_records(root: &Path, generation: u64) {
        for phase in InstallPhaseV1::ORDER {
            let path = root.join(record_name(generation, phase));
            if path.exists() {
                std::fs::remove_file(&path).expect("remove exact fixture record");
            }
        }
    }

    fn remove_fixture(root: &Path, generation: u64) {
        remove_generation_records(root, generation);
        std::fs::remove_file(root.join(GLOBAL_LOCK_NAME)).expect("remove fixture lock");
        std::fs::remove_dir(root).expect("remove fixture directory");
    }

    fn signed_for_generation(
        protected_root: &ObjectIdentityV1,
        generation: u64,
        id: u128,
    ) -> VerifiedApprovalV1 {
        let (mut payload, mut observed, key) = approval_test::fixture();
        payload.target_protected_root = protected_root.clone();
        observed.target_protected_root = protected_root.clone();
        payload.install_generation = generation;
        observed.next_install_generation = generation;
        payload.approval_id = format!("{id:032x}");
        verify_approval(
            &approval_test::envelope(&payload, &key),
            &key.verifying_key(),
            &observed,
        )
        .expect("generation-specific signed approval")
    }

    fn signed_for_generation_after(
        protected_root: &ObjectIdentityV1,
        generation: u64,
        id: u128,
        predecessor_package_id: &str,
        predecessor_pointer: &ObjectIdentityV1,
    ) -> VerifiedApprovalV1 {
        let (mut payload, mut observed, key) = approval_test::fixture();
        payload.target_protected_root = protected_root.clone();
        observed.target_protected_root = protected_root.clone();
        payload.install_generation = generation;
        observed.next_install_generation = generation;
        payload.approval_id = format!("{id:032x}");
        payload.predecessor_package_id = predecessor_package_id.into();
        observed.predecessor_package_id = predecessor_package_id.into();
        payload.predecessor_pointer = predecessor_pointer.clone();
        observed.predecessor_pointer = predecessor_pointer.clone();
        verify_approval(
            &approval_test::envelope(&payload, &key),
            &key.verifying_key(),
            &observed,
        )
        .expect("predecessor-bound signed approval")
    }

    fn completed_snapshot_fixture(
        head: &LedgerRecordV1,
        stage: &StageBindingV1,
        approval: &VerifiedApprovalV1,
    ) -> ProtectedInstallReadOnlySnapshotV1 {
        ProtectedInstallReadOnlySnapshotV1 {
            schema_version: 1,
            install_generation: head.install_generation,
            approval_id: head.approval_id.clone(),
            predecessor_package_id: head.predecessor_package_id.clone(),
            predecessor_pointer: head.predecessor_pointer.clone(),
            package_id: head.package_id.clone(),
            version: approval.payload().manifest.version.clone(),
            manifest_sha256: head.manifest_sha256.clone(),
            protected_root: head.protected_root.clone(),
            pointer: ObjectIdentityV1 {
                volume_serial: head.protected_root.volume_serial,
                file_index_high: 0,
                file_index_low: 999,
            },
            package_directory: stage.candidate_directory.clone(),
            manifest_file: stage.manifest_file.clone(),
            artifacts: std::array::from_fn(|index| {
                let signed = &approval.payload().manifest.artifacts[index];
                crate::install_broker_reader::ProtectedArtifactReadbackV1 {
                    name: [
                        "CatDeskWakeHost.exe",
                        "CatDeskBinagotchy.exe",
                        "adapter.py",
                        "wake_bridge.py",
                    ][index],
                    byte_length: signed.byte_length,
                    sha256: signed.sha256.clone(),
                    object: stage.artifacts[index].clone(),
                }
            }),
            disposition: "READ_ONLY_PROTECTED_PACKAGE_SNAPSHOT_NOT_LAUNCH_AUTHORITY",
        }
    }

    #[test]
    fn stage_parent_binding_preserves_legacy_v1_bytes_and_rejects_id_aliases() {
        let id = |low| ObjectIdentityV1 {
            volume_serial: 17,
            file_index_high: 0,
            file_index_low: low,
        };
        let root = id(1);
        let mut stage = StageBindingV1 {
            staging_directory: id(2),
            candidate_directory: id(3),
            manifest_file: id(4),
            artifacts: [id(5), id(6), id(7), id(8)],
            packages_directory: None,
        };
        let old_bytes = serde_json::to_vec(&stage).expect("old V1 stage binding");
        assert!(!String::from_utf8_lossy(&old_bytes).contains("packagesDirectory"));
        let replayed: StageBindingV1 =
            serde_json::from_slice(&old_bytes).expect("readable old V1 binding");
        assert_eq!(replayed, stage);
        assert_eq!(serde_json::to_vec(&replayed).unwrap(), old_bytes);
        assert!(valid_stage_binding(&replayed, &root));

        stage.packages_directory = Some(id(9));
        assert!(valid_stage_binding(&stage, &root));
        let new_bytes = serde_json::to_vec(&stage).expect("new stage binding");
        assert!(String::from_utf8_lossy(&new_bytes).contains("packagesDirectory"));
        assert_eq!(
            serde_json::from_slice::<StageBindingV1>(&new_bytes).unwrap(),
            stage
        );
        for alias in [
            root,
            stage.staging_directory.clone(),
            stage.candidate_directory.clone(),
            stage.manifest_file.clone(),
            stage.artifacts[0].clone(),
        ] {
            stage.packages_directory = Some(alias);
            assert!(!valid_stage_binding(&stage, &id(1)));
        }
    }

    #[test]
    fn legacy_staged_record_replays_without_rewriting_canonical_bytes() {
        let (root, handle, approval, expected) = fixture();
        let mut lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("intent");
        lease
            .advance(InstallPhaseV1::StageStarted)
            .expect("stage marker");
        lease.advance(InstallPhaseV1::Staged).expect("legacy stage");
        drop(lease);
        let path = root.join(record_name(
            expected.next_install_generation,
            InstallPhaseV1::Staged,
        ));
        let bytes = std::fs::read(&path).expect("historic STAGED bytes");
        assert!(!String::from_utf8_lossy(&bytes).contains("packagesDirectory"));
        let (record, _) = read_record(&mut File::open(&path).expect("historic STAGED file"))
            .expect("strict historic record replay");
        assert_eq!(canonical_bytes(&record).unwrap(), bytes);
        assert_eq!(
            record
                .stage_binding
                .as_ref()
                .expect("historic stage binding")
                .packages_directory,
            None
        );
        let inventory = ledger_inventory(&handle).expect("historic inventory");
        let present = inventory
            .get(&expected.next_install_generation)
            .expect("historic generation");
        let (_, phase, stage, _) = historical_intent(
            &handle,
            expected.next_install_generation,
            present,
            &expected.protected_root,
        )
        .expect("strict historic chain replay");
        assert_eq!(phase, InstallPhaseV1::Staged);
        assert_eq!(stage.unwrap().packages_directory, None);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn completed_predecessor_requires_nonempty_complete_history_without_writing() {
        let (root, handle, approval, expected) = fixture();
        let guard = BrokerLedgerGuard::acquire(&handle).expect("global lock");
        assert_eq!(
            completed_head(guard.root(), &expected.protected_root).unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_UNANCHORED"
        );
        assert!(ledger_inventory(guard.root()).unwrap().is_empty());
        drop(guard);

        let lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("intent");
        drop(lease);
        let guard = BrokerLedgerGuard::acquire(&handle).expect("reopen lock");
        assert_eq!(
            completed_head(guard.root(), &expected.protected_root).unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_INCOMPLETE"
        );
        assert_eq!(ledger_inventory(guard.root()).unwrap().len(), 1);
        drop(guard);
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn completed_predecessor_compares_exact_installed_package_ids() {
        let (root, handle, approval, expected) = fixture();
        let mut lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("intent");
        for phase in InstallPhaseV1::ORDER.iter().copied().skip(1) {
            lease
                .advance_legacy_without_pointer(phase)
                .expect("complete legacy fixture transaction");
        }
        drop(lease);
        let ledger_bytes_before: [Vec<u8>; 8] = std::array::from_fn(|index| {
            std::fs::read(root.join(record_name(
                expected.next_install_generation,
                InstallPhaseV1::ORDER[index],
            )))
            .expect("original record bytes")
        });
        let guard = BrokerLedgerGuard::acquire(&handle).expect("global lock");
        let (before, head, stage, pointer_binding) =
            completed_head(guard.root(), &expected.protected_root).expect("complete head");
        assert!(pointer_binding.is_none(), "legacy V1 fixture is unbound");
        let snapshot = completed_snapshot_fixture(&head, &stage, &approval);
        compare_completed_head(&head, &stage, None, &snapshot, "")
            .expect("exact installed package correspondence");
        // An older complete record corroborates the installed package only.
        let mut other_pointer = snapshot.clone();
        other_pointer.pointer.file_index_low = other_pointer.pointer.file_index_low.wrapping_add(1);
        compare_completed_head(&head, &stage, None, &other_pointer, "")
            .expect("legacy pointer binding is absent");
        let mut wrong = snapshot.clone();
        wrong.package_directory.file_index_low =
            wrong.package_directory.file_index_low.wrapping_add(1);
        assert_eq!(
            compare_completed_head(&head, &stage, None, &wrong, "").unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_MISMATCH"
        );
        let mut wrong = snapshot.clone();
        wrong.artifacts[2].object.file_index_low =
            wrong.artifacts[2].object.file_index_low.wrapping_add(1);
        assert_eq!(
            compare_completed_head(&head, &stage, None, &wrong, "").unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_MISMATCH"
        );
        let mut wrong = snapshot.clone();
        wrong.approval_id = "f".repeat(32);
        assert_eq!(
            compare_completed_head(&head, &stage, None, &wrong, "").unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_MISMATCH"
        );
        let mut wrong = snapshot.clone();
        wrong.manifest_sha256 = "f".repeat(64);
        assert_eq!(
            compare_completed_head(&head, &stage, None, &wrong, "").unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_MISMATCH"
        );
        assert_eq!(ledger_inventory(guard.root()).unwrap(), before);
        let ledger_bytes_after: [Vec<u8>; 8] = std::array::from_fn(|index| {
            std::fs::read(root.join(record_name(
                expected.next_install_generation,
                InstallPhaseV1::ORDER[index],
            )))
            .expect("unchanged record bytes")
        });
        assert_eq!(ledger_bytes_after, ledger_bytes_before);
        assert!(ledger_bytes_before.iter().all(|bytes| {
            !bytes
                .windows(b"pointerSwitchBinding".len())
                .any(|part| part == b"pointerSwitchBinding")
        }));
        let intent_name = record_name(expected.next_install_generation, InstallPhaseV1::Intent);
        let intent_path = root.join(&intent_name);
        let old_intent = std::fs::read(&intent_path).expect("original V1 intent");
        let old_text = String::from_utf8(old_intent.clone()).expect("canonical UTF-8");
        let with_explicit_null = old_text.replace(
            "\"stageBinding\":null,\"phase\":",
            "\"stageBinding\":null,\"pointerSwitchBinding\":null,\"phase\":",
        );
        assert_ne!(with_explicit_null, old_text);
        std::fs::write(&intent_path, with_explicit_null).expect("explicit null fixture");
        let mut opened = open_optional(guard.root(), &intent_name)
            .expect("open fixture record")
            .expect("intent exists");
        assert_eq!(
            read_record(&mut opened).unwrap_err(),
            "WAKE_LEDGER_RECORD_CORRUPT"
        );
        drop(opened);
        std::fs::write(intent_path, old_intent).expect("restore canonical V1 bytes");
        assert!(
            !root
                .join(record_name(
                    expected.next_install_generation + 1,
                    InstallPhaseV1::Intent
                ))
                .exists()
        );
        drop(guard);
        let next_generation = expected.next_install_generation + 1;
        let next = signed_for_generation(&expected.protected_root, next_generation, 707);
        let next_expected = LedgerExpectationV1 {
            next_install_generation: next_generation,
            ..expected.clone()
        };
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &next, &next_expected)
                .err()
                .unwrap(),
            "WAKE_LEDGER_PRIOR_POINTER_UNBOUND"
        );
        assert!(
            !root
                .join(record_name(next_generation, InstallPhaseV1::Intent))
                .exists()
        );
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn bound_pointer_requires_both_exact_file_id_and_digest() {
        let (root, handle, approval, expected) = fixture();
        let mut lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("intent");
        for phase in InstallPhaseV1::ORDER.iter().copied().skip(1) {
            lease.advance(phase).expect("bound fixture history");
        }
        drop(lease);
        let guard = BrokerLedgerGuard::acquire(&handle).expect("global lock");
        let (_, head, stage, pointer) =
            completed_head(guard.root(), &expected.protected_root).expect("bound complete head");
        let pointer = pointer.expect("switched pointer binding");
        let mut snapshot = completed_snapshot_fixture(&head, &stage, &approval);
        snapshot.pointer = pointer.pointer_file.clone();
        compare_completed_head(
            &head,
            &stage,
            Some(&pointer),
            &snapshot,
            &pointer.pointer_sha256,
        )
        .expect("both physical ID and exact bytes match");
        assert_eq!(
            compare_completed_head(&head, &stage, Some(&pointer), &snapshot, &"f".repeat(64))
                .unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_POINTER_MISMATCH"
        );
        snapshot.pointer.file_index_low = snapshot.pointer.file_index_low.wrapping_add(1);
        assert_eq!(
            compare_completed_head(
                &head,
                &stage,
                Some(&pointer),
                &snapshot,
                &pointer.pointer_sha256,
            )
            .unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_POINTER_MISMATCH"
        );
        drop(guard);
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn pointer_binding_cannot_appear_early_disappear_or_change_after_switch() {
        let (root, handle, approval, expected) = fixture();
        let mut lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("intent");
        for phase in InstallPhaseV1::ORDER.iter().copied().skip(1) {
            lease.advance(phase).expect("bound fixture history");
        }
        drop(lease);
        let generation = expected.next_install_generation;
        let switched: LedgerRecordV1 = serde_json::from_slice(
            &std::fs::read(root.join(record_name(generation, InstallPhaseV1::PointerSwitched)))
                .expect("switched bytes"),
        )
        .expect("switched record");
        let valid_pointer = switched.pointer_switch_binding.expect("bound switch");
        for (phase, change, error) in [
            (
                InstallPhaseV1::Published,
                Some(valid_pointer.clone()),
                "WAKE_LEDGER_RECORD_CORRUPT",
            ),
            (
                InstallPhaseV1::PointerSwitched,
                None,
                "WAKE_LEDGER_RECORD_CORRUPT",
            ),
            (
                InstallPhaseV1::NewOwnerProven,
                None,
                "WAKE_LEDGER_RECORD_CORRUPT",
            ),
            (
                InstallPhaseV1::Complete,
                Some(PointerSwitchBindingV1 {
                    pointer_sha256: "f".repeat(64),
                    ..valid_pointer.clone()
                }),
                "WAKE_LEDGER_RECORD_CORRUPT",
            ),
        ] {
            let path = root.join(record_name(generation, phase));
            let original = std::fs::read(&path).expect("original canonical record");
            let mut record: LedgerRecordV1 =
                serde_json::from_slice(&original).expect("record JSON");
            record.pointer_switch_binding = change;
            std::fs::write(
                &path,
                canonical_bytes(&record).expect("canonical altered record"),
            )
            .expect("alter isolated phase");
            let guard = BrokerLedgerGuard::acquire(&handle).expect("global lock");
            assert_eq!(
                completed_head(guard.root(), &expected.protected_root).unwrap_err(),
                error
            );
            drop(guard);
            std::fs::write(path, original).expect("restore exact record bytes");
        }
        let switched_path = root.join(record_name(generation, InstallPhaseV1::PointerSwitched));
        let original = std::fs::read(&switched_path).expect("original switch record");
        let mut invalid: LedgerRecordV1 =
            serde_json::from_slice(&original).expect("switch record JSON");
        invalid
            .pointer_switch_binding
            .as_mut()
            .expect("binding")
            .pointer_sha256 = "F".repeat(64);
        std::fs::write(
            &switched_path,
            canonical_bytes(&invalid).expect("invalid binding encoded"),
        )
        .expect("alter switch binding");
        let guard = BrokerLedgerGuard::acquire(&handle).expect("global lock");
        assert_eq!(
            completed_head(guard.root(), &expected.protected_root).unwrap_err(),
            "WAKE_LEDGER_POINTER_BINDING_INVALID"
        );
        drop(guard);
        std::fs::write(switched_path, original).expect("restore exact switch bytes");
        assert!(
            !root
                .join(record_name(generation + 1, InstallPhaseV1::Intent))
                .exists()
        );
        drop(handle);
        remove_fixture(&root, generation);
    }

    #[test]
    fn switched_and_complete_parent_flush_failures_reopen_exact_bound_records() {
        let (root, handle, approval, expected) = fixture();
        let generation = expected.next_install_generation;
        let mut lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("intent");
        for phase in [
            InstallPhaseV1::StageStarted,
            InstallPhaseV1::Staged,
            InstallPhaseV1::Published,
            InstallPhaseV1::OldOwnerStopped,
        ] {
            lease.advance(phase).expect("pre-switch phase");
        }
        fail_next_flush(ParentFlushPoint::LaterLedgerPhase);
        assert_eq!(
            lease.advance(InstallPhaseV1::PointerSwitched).unwrap_err(),
            "WAKE_BROKER_PARENT_FLUSH_FAILED"
        );
        assert_eq!(lease.phase(), InstallPhaseV1::OldOwnerStopped);
        assert!(
            root.join(record_name(generation, InstallPhaseV1::PointerSwitched))
                .exists()
        );
        assert!(
            !root
                .join(record_name(generation, InstallPhaseV1::NewOwnerProven))
                .exists()
        );
        drop(lease);
        let mut lease = BrokerLedgerLease::begin(&handle, &approval, &expected)
            .expect("exact switched replay and parent reflush");
        assert_eq!(lease.phase(), InstallPhaseV1::PointerSwitched);
        assert!(
            !lease
                .advance(InstallPhaseV1::PointerSwitched)
                .expect("idempotent exact phase")
        );
        lease
            .advance(InstallPhaseV1::NewOwnerProven)
            .expect("bound owner proof fixture");
        fail_next_flush(ParentFlushPoint::LaterLedgerPhase);
        assert_eq!(
            lease.advance(InstallPhaseV1::Complete).unwrap_err(),
            "WAKE_BROKER_PARENT_FLUSH_FAILED"
        );
        assert_eq!(lease.phase(), InstallPhaseV1::NewOwnerProven);
        drop(lease);
        let mut lease = BrokerLedgerLease::begin(&handle, &approval, &expected)
            .expect("exact complete replay and parent reflush");
        assert_eq!(lease.phase(), InstallPhaseV1::Complete);
        assert!(
            !lease
                .advance(InstallPhaseV1::Complete)
                .expect("idempotent complete")
        );
        drop(lease);
        let guard = BrokerLedgerGuard::acquire(&handle).expect("global lock");
        let (_, _, _, pointer) =
            completed_head(guard.root(), &expected.protected_root).expect("complete bound head");
        assert!(pointer.is_some());
        drop(guard);
        assert!(
            !root
                .join(record_name(generation + 1, InstallPhaseV1::Intent))
                .exists()
        );
        drop(handle);
        remove_fixture(&root, generation);
    }

    #[test]
    fn completed_predecessor_rejects_cross_generation_package_drift() {
        let (root, handle, approval, expected) = fixture();
        let mut first = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("first");
        for phase in InstallPhaseV1::ORDER.iter().copied().skip(1) {
            first.advance(phase).expect("complete first");
        }
        let first_pointer = first
            .binding
            .pointer_switch_binding
            .as_ref()
            .expect("fixture switched pointer")
            .pointer_file
            .clone();
        drop(first);
        let second_generation = expected.next_install_generation + 1;
        let (mut second_payload, mut second_observed, second_key) = approval_test::fixture();
        second_payload.target_protected_root = expected.protected_root.clone();
        second_observed.target_protected_root = expected.protected_root.clone();
        second_payload.install_generation = second_generation;
        second_observed.next_install_generation = second_generation;
        second_payload.approval_id = format!("{:032x}", 9090);
        second_payload.predecessor_package_id = approval.payload().manifest.package_id.clone();
        second_observed.predecessor_package_id = second_payload.predecessor_package_id.clone();
        second_payload.predecessor_pointer = first_pointer.clone();
        second_observed.predecessor_pointer = first_pointer.clone();
        let second_approval = verify_approval(
            &approval_test::envelope(&second_payload, &second_key),
            &second_key.verifying_key(),
            &second_observed,
        )
        .expect("matching second predecessor");
        let second_expected = LedgerExpectationV1 {
            next_install_generation: second_generation,
            predecessor_package_id: second_payload.predecessor_package_id,
            predecessor_pointer: first_pointer,
            ..expected.clone()
        };
        let second = BrokerLedgerLease::begin(&handle, &second_approval, &second_expected)
            .expect("second intent");
        drop(second);
        let second_path = root.join(record_name(second_generation, InstallPhaseV1::Intent));
        let original_second = std::fs::read(&second_path).expect("second intent bytes");
        let mut second_intent: LedgerRecordV1 = serde_json::from_slice(&original_second).unwrap();
        second_intent.predecessor_package_id = "other-package".into();
        std::fs::write(&second_path, canonical_bytes(&second_intent).unwrap())
            .expect("isolated fixture drift");
        let guard = BrokerLedgerGuard::acquire(&handle).expect("global lock");
        assert_eq!(
            completed_head(guard.root(), &expected.protected_root).unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_HISTORY_MISMATCH"
        );
        drop(guard);
        let mut second_intent: LedgerRecordV1 = serde_json::from_slice(&original_second).unwrap();
        second_intent.predecessor_pointer.file_index_low = second_intent
            .predecessor_pointer
            .file_index_low
            .wrapping_add(1);
        std::fs::write(&second_path, canonical_bytes(&second_intent).unwrap())
            .expect("isolated physical pointer drift");
        let guard = BrokerLedgerGuard::acquire(&handle).expect("pointer history lock");
        assert_eq!(
            completed_head(guard.root(), &expected.protected_root).unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_HISTORY_MISMATCH"
        );
        drop(guard);
        drop(handle);
        remove_generation_records(&root, second_generation);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn completed_predecessor_integrates_pinned_policy_reader_and_ledger() {
        use crate::install_broker_policy::tests::load_fixture_policy_for_existing_root;
        use crate::install_broker_reader::tests::Fixture as ProtectedReaderFixture;

        let fixture = ProtectedReaderFixture::new();
        let root_path = fixture.root_path().to_path_buf();
        std::fs::write(root_path.join(GLOBAL_LOCK_NAME), b"").expect("preprovision fixture lock");
        let policy = load_fixture_policy_for_existing_root(
            fixture.root_path(),
            fixture.root(),
            fixture.root_id(),
        )
        .expect("pinned fixture policy");
        let (mut payload, mut observed, key) = approval_test::fixture();
        payload.target_protected_root = fixture.root_id().clone();
        observed.target_protected_root = fixture.root_id().clone();
        payload.manifest = fixture.manifest().clone();
        payload.manifest_sha256 = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&payload.manifest).expect("manifest"))
        );
        let (predecessor_package, predecessor_pointer) = fixture.pointer_predecessor();
        payload.predecessor_package_id = predecessor_package.into();
        payload.predecessor_pointer = predecessor_pointer.clone();
        observed.predecessor_package_id = payload.predecessor_package_id.clone();
        observed.predecessor_pointer = payload.predecessor_pointer.clone();
        let envelope = approval_test::envelope(&payload, &key);
        let expected = LedgerExpectationV1 {
            now_utc_seconds: observed.now_utc_seconds,
            next_install_generation: payload.install_generation,
            predecessor_package_id: payload.predecessor_package_id.clone(),
            predecessor_pointer: payload.predecessor_pointer.clone(),
            protected_root: fixture.root_id().clone(),
            original_desired_state: OriginalDesiredStateV1::Running,
        };
        let guard = BrokerLedgerGuard::acquire(fixture.root()).expect("empty-history lock");
        assert_eq!(
            guard
                .accept_with_observation(&policy, &envelope, |_| {
                    Ok((observed.clone(), expected.clone()))
                })
                .err()
                .expect("first-generation production refusal"),
            "WAKE_BROKER_PREDECESSOR_UNANCHORED"
        );
        assert!(
            !root_path
                .join(record_name(
                    payload.install_generation,
                    InstallPhaseV1::Intent
                ))
                .exists()
        );
        let guard = BrokerLedgerGuard::acquire(fixture.root()).expect("global lock");
        let (mut lease, _) = guard
            .accept_with_observation_fixture(&policy, &envelope, |_| {
                Ok((observed.clone(), expected.clone()))
            })
            .expect("fixture signed intent");
        lease
            .record_stage_started()
            .expect("write-ahead stage marker");
        let installed = fixture.inspect().expect("real protected fixture objects");
        let mut unverified_staging_parent = fixture.root_id().clone();
        unverified_staging_parent.file_index_low = unverified_staging_parent
            .file_index_low
            .wrapping_add(10_000);
        let stage = StageBindingV1 {
            staging_directory: unverified_staging_parent,
            packages_directory: None,
            candidate_directory: installed.package_directory.clone(),
            manifest_file: installed.manifest_file.clone(),
            artifacts: std::array::from_fn(|index| installed.artifacts[index].object.clone()),
        };
        lease
            .record_staged_fixture(stage)
            .expect("fixture staged binding");
        lease
            .advance(InstallPhaseV1::Published)
            .expect("published fixture");
        lease
            .advance(InstallPhaseV1::OldOwnerStopped)
            .expect("old-owner-stopped fixture");
        drop(lease);
        // A valid current.json by itself cannot fill a missing durable
        // POINTER_SWITCHED record after a crash at this seam.
        let guard = BrokerLedgerGuard::acquire(fixture.root()).expect("pre-switch reopen lock");
        assert_eq!(
            completed_head(guard.root(), fixture.root_id()).unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_INCOMPLETE"
        );
        drop(guard);
        assert!(
            !root_path
                .join(record_name(
                    payload.install_generation,
                    InstallPhaseV1::PointerSwitched
                ))
                .exists()
        );
        let guard = BrokerLedgerGuard::acquire(fixture.root()).expect("exact pre-switch reopen");
        let (mut lease, _) = guard
            .accept_with_observation_fixture(&policy, &envelope, |_| {
                Ok((observed.clone(), expected.clone()))
            })
            .expect("exact stopped phase replay");
        assert_eq!(lease.phase(), InstallPhaseV1::OldOwnerStopped);
        let pin = crate::install_broker_reader::pin_protected_install_fixture(
            fixture.root(),
            fixture.root_id(),
        )
        .expect("real pointer pin");
        let pointer_binding = PointerSwitchBindingV1 {
            pointer_file: pin.snapshot().pointer.clone(),
            pointer_sha256: pin.pointer_sha256().into(),
        };
        lease
            .record_pointer_switched_fixture(pointer_binding)
            .expect("durable fixture pointer binding");
        pin.revalidate(fixture.root(), fixture.root_id())
            .expect("pointer stays pinned through record");
        drop(pin);
        drop(lease);
        let guard = BrokerLedgerGuard::acquire(fixture.root()).expect("switched reopen lock");
        let (mut lease, _) = guard
            .accept_with_observation_fixture(&policy, &envelope, |_| {
                Ok((observed.clone(), expected.clone()))
            })
            .expect("exact switched record replay");
        assert_eq!(lease.phase(), InstallPhaseV1::PointerSwitched);
        lease
            .advance(InstallPhaseV1::NewOwnerProven)
            .expect("new-owner-proven fixture");
        lease
            .advance(InstallPhaseV1::Complete)
            .expect("complete fixture");
        drop(lease);
        let guard = BrokerLedgerGuard::acquire(fixture.root()).expect("complete reopen lock");
        let (lease, _) = guard
            .accept_with_observation_fixture(&policy, &envelope, |_| {
                Ok((observed.clone(), expected.clone()))
            })
            .expect("exact complete record replay");
        assert_eq!(lease.phase(), InstallPhaseV1::Complete);
        drop(lease);
        let ledger_bytes_before: [Vec<u8>; 8] = std::array::from_fn(|index| {
            std::fs::read(root_path.join(record_name(
                payload.install_generation,
                InstallPhaseV1::ORDER[index],
            )))
            .expect("record bytes")
        });
        let guard = BrokerLedgerGuard::acquire(fixture.root()).expect("global lock");
        let result = guard
            .inspect_completed_predecessor_fixture(&policy)
            .expect("integrated read-only projection");
        assert_eq!(result.install_generation, payload.install_generation);
        assert_eq!(result.protected_install, installed);
        assert!(result.installed_package_object_ids_matched);
        assert!(!result.staging_parent_object_verified);
        assert!(result.pointer_switch_record_binding_present);
        assert!(!result.first_generation_migration_anchor_verified);
        assert!(!result.old_live_owner_verified);
        assert!(result.disposition.ends_with("NO_INSTALL_AUTHORITY"));
        let mut successor_payload = payload.clone();
        successor_payload.install_generation += 1;
        successor_payload.approval_id = format!("{:032x}", 0x35u64);
        successor_payload.predecessor_package_id = installed.package_id.clone();
        successor_payload.predecessor_pointer = installed.pointer.clone();
        let mut successor_observed = observed.clone();
        successor_observed.next_install_generation = successor_payload.install_generation;
        successor_observed.predecessor_package_id = installed.package_id.clone();
        successor_observed.predecessor_pointer = installed.pointer.clone();
        let successor_envelope = approval_test::envelope(&successor_payload, &key);
        let successor = policy
            .verify_approval_for_transaction(&successor_envelope, &successor_observed)
            .expect("signed successor for fixture");
        let successor_expected = LedgerExpectationV1 {
            next_install_generation: successor_payload.install_generation,
            predecessor_package_id: installed.package_id.clone(),
            predecessor_pointer: installed.pointer.clone(),
            ..expected.clone()
        };
        let (matched, successor_pin) = guard
            .pin_fresh_successor_predecessor_with(
                &policy,
                &successor,
                &successor_expected,
                PredecessorReaderMode::Fixture,
            )
            .expect("pinned signed successor correspondence");
        assert_eq!(matched, result);
        assert!(!matched.first_generation_migration_anchor_verified);
        assert!(!matched.old_live_owner_verified);
        successor_pin
            .revalidate(fixture.root(), fixture.root_id())
            .expect("retained predecessor pin");
        drop(successor_pin);
        let next_intent = root_path.join(record_name(
            successor_payload.install_generation,
            InstallPhaseV1::Intent,
        ));
        assert!(!next_intent.exists());
        let mut wrong_expected = successor_expected.clone();
        wrong_expected.next_install_generation += 1;
        assert_eq!(
            compare_fresh_successor(&result, &successor_payload, &wrong_expected).unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_SUCCESSOR_MISMATCH"
        );
        let mut wrong_payload = successor_payload.clone();
        wrong_payload.install_generation += 1;
        assert_eq!(
            compare_fresh_successor(&result, &wrong_payload, &successor_expected).unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_SUCCESSOR_MISMATCH"
        );
        let mut wrong_expected = successor_expected.clone();
        wrong_expected.predecessor_package_id = "other-package".into();
        assert_eq!(
            compare_fresh_successor(&result, &successor_payload, &wrong_expected).unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_SUCCESSOR_MISMATCH"
        );
        let mut wrong_expected = successor_expected.clone();
        wrong_expected.predecessor_pointer.file_index_low = wrong_expected
            .predecessor_pointer
            .file_index_low
            .wrapping_add(1);
        assert_eq!(
            compare_fresh_successor(&result, &successor_payload, &wrong_expected).unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_SUCCESSOR_MISMATCH"
        );
        let mut wrong_payload = successor_payload.clone();
        wrong_payload.predecessor_pointer.file_index_low = wrong_payload
            .predecessor_pointer
            .file_index_low
            .wrapping_add(1);
        assert_eq!(
            compare_fresh_successor(&result, &wrong_payload, &successor_expected).unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_SUCCESSOR_MISMATCH"
        );
        let mut wrong_payload = successor_payload.clone();
        wrong_payload.predecessor_package_id = "other-package".into();
        assert_eq!(
            compare_fresh_successor(&result, &wrong_payload, &successor_expected).unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_SUCCESSOR_MISMATCH"
        );
        let mut unbound = result.clone();
        unbound.pointer_switch_record_binding_present = false;
        assert_eq!(
            compare_fresh_successor(&unbound, &successor_payload, &successor_expected).unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_POINTER_UNBOUND"
        );
        let mut overflow = result.clone();
        overflow.install_generation = u64::MAX;
        assert_eq!(
            compare_fresh_successor(&overflow, &successor_payload, &successor_expected)
                .unwrap_err(),
            "WAKE_LEDGER_STALE_GENERATION"
        );
        drop(guard);
        let guard = BrokerLedgerGuard::acquire(fixture.root()).expect("fresh successor gate lock");
        assert_eq!(
            guard
                .accept_with_guarded_predecessor_fixture(&policy, &successor_envelope, |_| Ok((
                    successor_observed.clone(),
                    successor_expected.clone()
                )),)
                .err()
                .expect("migration anchor still unavailable"),
            "WAKE_BROKER_PREDECESSOR_MIGRATION_UNANCHORED"
        );
        assert!(!next_intent.exists());
        let mut signed_wrong_payload = successor_payload.clone();
        signed_wrong_payload.predecessor_package_id = "other-package".into();
        let mut signed_wrong_observed = successor_observed.clone();
        signed_wrong_observed.predecessor_package_id =
            signed_wrong_payload.predecessor_package_id.clone();
        let signed_wrong_envelope = approval_test::envelope(&signed_wrong_payload, &key);
        let guard = BrokerLedgerGuard::acquire(fixture.root()).expect("signed drift gate lock");
        assert_eq!(
            guard
                .accept_with_guarded_predecessor_fixture(&policy, &signed_wrong_envelope, |_| {
                    Ok((signed_wrong_observed, successor_expected.clone()))
                })
                .err()
                .expect("signed but physically wrong predecessor refused"),
            "WAKE_BROKER_PREDECESSOR_SUCCESSOR_MISMATCH"
        );
        assert!(!next_intent.exists());
        let guard = BrokerLedgerGuard::acquire(fixture.root()).expect("read-only drift lock");
        let pointer_path = root_path.join("current.json");
        let original_pointer_bytes = std::fs::read(&pointer_path).expect("pointer bytes");
        let saved_pointer = root_path.join("original-pointer-fixture.json");
        std::fs::rename(&pointer_path, &saved_pointer).expect("save exact pointer file ID");
        std::fs::write(&pointer_path, &original_pointer_bytes)
            .expect("same canonical bytes in a new pointer file");
        assert_eq!(
            guard
                .inspect_completed_predecessor_fixture(&policy)
                .unwrap_err(),
            "WAKE_BROKER_PREDECESSOR_POINTER_MISMATCH"
        );
        assert_eq!(
            guard
                .pin_fresh_successor_predecessor_with(
                    &policy,
                    &successor,
                    &successor_expected,
                    PredecessorReaderMode::Fixture,
                )
                .err()
                .expect("swapped pointer physical ID refused"),
            "WAKE_BROKER_PREDECESSOR_POINTER_MISMATCH"
        );
        std::fs::remove_file(&pointer_path).expect("remove replacement pointer");
        std::fs::rename(&saved_pointer, &pointer_path).expect("restore exact pointer file ID");
        guard
            .inspect_completed_predecessor_fixture(&policy)
            .expect("original pointer binding restored");
        let changed_artifact = root_path
            .join("packages")
            .join(&payload.manifest.package_id)
            .join("adapter.py");
        std::fs::write(&changed_artifact, b"adapter-xode").expect("same-length fixture drift");
        assert_eq!(
            guard
                .inspect_completed_predecessor_fixture(&policy)
                .unwrap_err(),
            "WAKE_PROTECTED_READER_HASH_MISMATCH"
        );
        assert_eq!(
            guard
                .pin_fresh_successor_predecessor_with(
                    &policy,
                    &successor,
                    &successor_expected,
                    PredecessorReaderMode::Fixture,
                )
                .err()
                .expect("same-length artifact drift refused"),
            "WAKE_PROTECTED_READER_HASH_MISMATCH"
        );
        std::fs::write(&changed_artifact, b"adapter-code").expect("restore fixture bytes");
        let ledger_bytes_after: [Vec<u8>; 8] = std::array::from_fn(|index| {
            std::fs::read(root_path.join(record_name(
                payload.install_generation,
                InstallPhaseV1::ORDER[index],
            )))
            .expect("unchanged record bytes")
        });
        assert_eq!(ledger_bytes_after, ledger_bytes_before);
        assert!(!next_intent.exists());
        assert!(
            !root_path
                .join(record_name(
                    payload.install_generation + 1,
                    InstallPhaseV1::Intent
                ))
                .exists()
        );
        drop(guard);
        drop(policy);
        remove_generation_records(&root_path, payload.install_generation);
        std::fs::remove_file(root_path.join(GLOBAL_LOCK_NAME)).expect("remove fixture lock");
        std::fs::remove_file(root_path.join("broker-policy.json")).expect("remove fixture policy");
        fixture.cleanup();
    }

    #[test]
    fn protected_clock_floor_reads_only_validated_history_under_lock() {
        let (root, handle, approval, expected) = fixture();
        let root_id = root_info(&handle).expect("root identity");
        let guard = BrokerLedgerGuard::acquire(&handle).expect("global lock");
        assert_eq!(
            validated_latest_accepted_at(guard.root(), &root_id).unwrap(),
            None
        );
        drop(guard);

        let lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("intent");
        drop(lease);
        let guard = BrokerLedgerGuard::acquire(&handle).expect("reopen global lock");
        assert_eq!(
            validated_latest_accepted_at(guard.root(), &root_id).unwrap(),
            Some(expected.now_utc_seconds)
        );
        drop(guard);
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn protected_clock_floor_refuses_backward_later_acceptance() {
        let (root, handle, approval, expected) = fixture();
        let first_generation = expected.next_install_generation;
        let mut first =
            BrokerLedgerLease::begin(&handle, &approval, &expected).expect("first intent");
        for phase in InstallPhaseV1::ORDER.iter().copied().skip(1) {
            first.advance(phase).expect("complete first generation");
        }
        let predecessor_pointer = first
            .binding
            .pointer_switch_binding
            .as_ref()
            .expect("fixture switched pointer")
            .pointer_file
            .clone();
        drop(first);
        let second_generation = first_generation + 1;
        let second_approval = signed_for_generation_after(
            &expected.protected_root,
            second_generation,
            8787,
            &approval.payload().manifest.package_id,
            &predecessor_pointer,
        );
        let second_expected = LedgerExpectationV1 {
            next_install_generation: second_generation,
            predecessor_package_id: approval.payload().manifest.package_id.clone(),
            predecessor_pointer,
            ..expected.clone()
        };
        let second = BrokerLedgerLease::begin(&handle, &second_approval, &second_expected)
            .expect("second intent");
        drop(second);
        let second_intent = root.join(record_name(second_generation, InstallPhaseV1::Intent));
        let mut record: LedgerRecordV1 =
            serde_json::from_slice(&std::fs::read(&second_intent).unwrap()).unwrap();
        record.accepted_at_utc_seconds = expected.now_utc_seconds - 1;
        std::fs::write(&second_intent, canonical_bytes(&record).unwrap())
            .expect("write exact isolated fixture intent");

        let root_id = root_info(&handle).expect("root identity");
        let guard = BrokerLedgerGuard::acquire(&handle).expect("global lock");
        assert_eq!(
            validated_latest_accepted_at(guard.root(), &root_id).unwrap_err(),
            "WAKE_LEDGER_CLOCK_ROLLBACK"
        );
        drop(guard);
        drop(handle);
        remove_generation_records(&root, second_generation);
        remove_fixture(&root, first_generation);
    }

    #[test]
    fn create_once_crash_reopen_and_ordered_completion_are_exact() {
        let (root, handle, approval, expected) = fixture();
        let mut lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("intent");
        assert_eq!(lease.phase(), InstallPhaseV1::Intent);
        assert_eq!(
            lease.advance(InstallPhaseV1::Published).unwrap_err(),
            "WAKE_LEDGER_PHASE_ORDER_INVALID"
        );
        assert!(lease.advance(InstallPhaseV1::StageStarted).expect("marker"));
        assert!(lease.advance(InstallPhaseV1::Staged).expect("stage"));
        drop(lease);
        let mut resumed = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("reopen");
        assert_eq!(resumed.phase(), InstallPhaseV1::Staged);
        assert!(!resumed.advance(InstallPhaseV1::Staged).expect("idempotent"));
        for phase in InstallPhaseV1::ORDER.iter().copied().skip(3) {
            assert!(resumed.advance(phase).expect("ordered phase"));
        }
        drop(resumed);
        let mut complete =
            BrokerLedgerLease::begin(&handle, &approval, &expected).expect("complete reopen");
        assert_eq!(complete.phase(), InstallPhaseV1::Complete);
        assert!(
            !complete
                .advance(InstallPhaseV1::Complete)
                .expect("duplicate complete")
        );
        drop(complete);
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn intent_parent_flush_failure_stops_before_any_stage_attempt() {
        let (root, handle, approval, expected) = fixture();
        fail_next_flush(ParentFlushPoint::Intent);
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &approval, &expected)
                .err()
                .unwrap(),
            "WAKE_BROKER_PARENT_FLUSH_FAILED"
        );
        assert!(root.join(record_name(2, InstallPhaseV1::Intent)).exists());
        assert!(
            !root
                .join(record_name(2, InstallPhaseV1::StageStarted))
                .exists()
        );
        // If the directory entry survived, exact reopen can finish the flush;
        // no physical stage mutation was permitted by the failed acceptance.
        let lease = BrokerLedgerLease::begin(&handle, &approval, &expected)
            .expect("exact intent reopen after flush failure");
        assert_eq!(lease.phase(), InstallPhaseV1::Intent);
        drop(lease);
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn durable_stage_started_is_sticky_without_a_candidate_directory() {
        let (root, handle, approval, expected) = fixture();
        let mut lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("intent");
        lease.record_stage_started().expect("durable marker");
        assert_eq!(lease.phase(), InstallPhaseV1::StageStarted);
        drop(lease);
        let mut reopened =
            BrokerLedgerLease::begin(&handle, &approval, &expected).expect("marker reopen");
        assert_eq!(reopened.phase(), InstallPhaseV1::StageStarted);
        assert_eq!(
            reopened.record_stage_started().unwrap_err(),
            "WAKE_LEDGER_STAGE_ATTENTION"
        );
        assert!(!root.join(record_name(2, InstallPhaseV1::Staged)).exists());
        drop(reopened);
        let next_approval = signed_for_generation(&expected.protected_root, 3, 3);
        let next_expected = LedgerExpectationV1 {
            next_install_generation: 3,
            ..expected.clone()
        };
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &next_approval, &next_expected)
                .err()
                .unwrap(),
            "WAKE_LEDGER_PRIOR_INCOMPLETE"
        );
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn concurrent_contender_and_different_approval_at_same_generation_are_denied() {
        let (root, handle, approval, expected) = fixture();
        let lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("first");
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &approval, &expected)
                .err()
                .unwrap(),
            "WAKE_LEDGER_CONTENDER_BUSY"
        );
        drop(lease);
        let (mut other_payload, mut other_observed, key) = approval_test::fixture();
        other_payload.target_protected_root = expected.protected_root.clone();
        other_observed.target_protected_root = expected.protected_root.clone();
        other_payload.approval_id = "d".repeat(32);
        let other = verify_approval(
            &approval_test::envelope(&other_payload, &key),
            &key.verifying_key(),
            &other_observed,
        )
        .expect("other signed approval");
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &other, &expected)
                .err()
                .unwrap(),
            "WAKE_LEDGER_APPROVAL_CONFLICT"
        );
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn predecessor_generation_and_root_mismatch_create_no_record() {
        let (root, handle, approval, expected) = fixture();
        for changed in [
            LedgerExpectationV1 {
                next_install_generation: expected.next_install_generation + 1,
                ..expected.clone()
            },
            LedgerExpectationV1 {
                predecessor_package_id: "wrong".into(),
                ..expected.clone()
            },
            LedgerExpectationV1 {
                protected_root: ObjectIdentityV1 {
                    file_index_low: expected.protected_root.file_index_low + 1,
                    ..expected.protected_root.clone()
                },
                ..expected.clone()
            },
        ] {
            assert_eq!(
                BrokerLedgerLease::begin(&handle, &approval, &changed)
                    .err()
                    .unwrap(),
                "WAKE_LEDGER_PREDECESSOR_MISMATCH"
            );
            assert!(
                !root
                    .join(record_name(
                        expected.next_install_generation,
                        InstallPhaseV1::Intent
                    ))
                    .exists()
            );
        }
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn torn_or_relinked_intent_fails_closed_without_repair() {
        let (root, handle, approval, expected) = fixture();
        let lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("intent");
        drop(lease);
        let path = root.join(record_name(
            expected.next_install_generation,
            InstallPhaseV1::Intent,
        ));
        std::fs::write(&path, b"{").expect("tear exact fixture");
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &approval, &expected)
                .err()
                .unwrap(),
            "WAKE_LEDGER_RECORD_CORRUPT"
        );
        // Restore only this test's fixture to prove a linked record is denied.
        std::fs::write(&path, b"{}").expect("fixture content");
        let link = root.join("linked-fixture.json");
        std::fs::hard_link(&path, &link).expect("hardlink fixture");
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &approval, &expected)
                .err()
                .unwrap(),
            "WAKE_LEDGER_RECORD_INVALID"
        );
        std::fs::remove_file(link).expect("remove exact link");
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn future_phase_after_a_gap_is_corrupt() {
        let (root, handle, approval, expected) = fixture();
        let mut lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("intent");
        lease.advance(InstallPhaseV1::StageStarted).expect("marker");
        lease.advance(InstallPhaseV1::Staged).expect("staged");
        drop(lease);
        let staged = root.join(record_name(
            expected.next_install_generation,
            InstallPhaseV1::Staged,
        ));
        let published = root.join(record_name(
            expected.next_install_generation,
            InstallPhaseV1::Published,
        ));
        std::fs::rename(&staged, &published).expect("fixture gap");
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &approval, &expected)
                .err()
                .unwrap(),
            "WAKE_LEDGER_RECORD_CORRUPT"
        );
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn missing_intent_with_future_phase_rejects_without_creating_intent() {
        let (root, handle, approval, expected) = fixture();
        let intent = root.join(record_name(
            expected.next_install_generation,
            InstallPhaseV1::Intent,
        ));
        let future = root.join(record_name(
            expected.next_install_generation,
            InstallPhaseV1::Published,
        ));
        std::fs::write(&future, b"foreign phase residue").expect("fixture residue");
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &approval, &expected)
                .err()
                .unwrap(),
            "WAKE_LEDGER_RECORD_CORRUPT"
        );
        assert!(!intent.exists());
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn fixed_global_lock_denies_cross_generation_contender_before_intent() {
        let (root, handle, approval, expected) = fixture();
        let first = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("first");
        let (mut payload, mut observed, key) = approval_test::fixture();
        payload.target_protected_root = expected.protected_root.clone();
        observed.target_protected_root = expected.protected_root.clone();
        payload.install_generation += 1;
        observed.next_install_generation = payload.install_generation;
        payload.approval_id = "d".repeat(32);
        let second = verify_approval(
            &approval_test::envelope(&payload, &key),
            &key.verifying_key(),
            &observed,
        )
        .expect("second approval");
        let next = LedgerExpectationV1 {
            next_install_generation: payload.install_generation,
            ..expected.clone()
        };
        let next_intent = root.join(record_name(
            next.next_install_generation,
            InstallPhaseV1::Intent,
        ));
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &second, &next)
                .err()
                .unwrap(),
            "WAKE_LEDGER_CONTENDER_BUSY"
        );
        assert!(!next_intent.exists());
        drop(first);
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &second, &next)
                .err()
                .unwrap(),
            "WAKE_LEDGER_PRIOR_INCOMPLETE"
        );
        assert!(!next_intent.exists());
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn completed_history_enforces_generation_and_unique_approval_id() {
        let (root, handle, approval, expected) = fixture();
        let mut first = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("first");
        for phase in InstallPhaseV1::ORDER.iter().copied().skip(1) {
            first.advance(phase).expect("complete prior");
        }
        let predecessor_pointer = first
            .binding
            .pointer_switch_binding
            .as_ref()
            .expect("fixture switched pointer")
            .pointer_file
            .clone();
        drop(first);
        let (mut payload, mut observed, key) = approval_test::fixture();
        payload.target_protected_root = expected.protected_root.clone();
        observed.target_protected_root = expected.protected_root.clone();
        payload.install_generation += 1;
        observed.next_install_generation = payload.install_generation;
        payload.predecessor_package_id = approval.payload().manifest.package_id.clone();
        observed.predecessor_package_id = payload.predecessor_package_id.clone();
        payload.predecessor_pointer = predecessor_pointer.clone();
        observed.predecessor_pointer = predecessor_pointer.clone();
        let next = LedgerExpectationV1 {
            next_install_generation: payload.install_generation,
            predecessor_package_id: payload.predecessor_package_id.clone(),
            predecessor_pointer,
            ..expected.clone()
        };
        let same_id = verify_approval(
            &approval_test::envelope(&payload, &key),
            &key.verifying_key(),
            &observed,
        )
        .expect("signed reused ID");
        let next_intent = root.join(record_name(
            next.next_install_generation,
            InstallPhaseV1::Intent,
        ));
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &same_id, &next)
                .err()
                .unwrap(),
            "WAKE_LEDGER_APPROVAL_REPLAY"
        );
        assert!(!next_intent.exists());
        payload.approval_id = "d".repeat(32);
        let unique = verify_approval(
            &approval_test::envelope(&payload, &key),
            &key.verifying_key(),
            &observed,
        )
        .expect("unique signed ID");
        let second = BrokerLedgerLease::begin(&handle, &unique, &next).expect("next generation");
        drop(second);
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &approval, &expected)
                .err()
                .unwrap(),
            "WAKE_LEDGER_STALE_GENERATION"
        );
        std::fs::remove_file(next_intent).expect("remove exact second-generation fixture");
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn expired_approval_resumes_only_from_exact_original_intent() {
        let (root, handle, approval, expected) = fixture();
        let mut first = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("intent");
        first.advance(InstallPhaseV1::StageStarted).expect("marker");
        first.advance(InstallPhaseV1::Staged).expect("staged");
        drop(first);
        let (mut payload, mut observed, key) = approval_test::fixture();
        payload.target_protected_root = expected.protected_root.clone();
        observed.target_protected_root = expected.protected_root.clone();
        observed.now_utc_seconds = payload.expires_utc_seconds + 1;
        let envelope = approval_test::envelope(&payload, &key);
        assert_eq!(
            verify_approval(&envelope, &key.verifying_key(), &observed)
                .err()
                .unwrap(),
            "WAKE_APPROVAL_TIME_INVALID"
        );
        let (reverified, recovered) = verify_exact_resume(
            &handle,
            &envelope,
            &key.verifying_key(),
            payload.install_generation,
            observed.now_utc_seconds,
        )
        .expect("exact protected resume");
        assert_eq!(
            recovered.original_desired_state,
            OriginalDesiredStateV1::Running
        );
        let resumed = BrokerLedgerLease::begin(&handle, &reverified, &recovered)
            .expect("reopen after expiry");
        assert_eq!(resumed.phase(), InstallPhaseV1::Staged);
        drop(resumed);
        payload.approval_id = "d".repeat(32);
        let changed_envelope = approval_test::envelope(&payload, &key);
        assert_eq!(
            verify_exact_resume(
                &handle,
                &changed_envelope,
                &key.verifying_key(),
                payload.install_generation,
                observed.now_utc_seconds,
            )
            .err()
            .unwrap(),
            "WAKE_LEDGER_APPROVAL_CONFLICT"
        );
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn expired_fresh_approval_and_missing_global_lock_do_not_create_intent() {
        let (root, handle, approval, expected) = fixture();
        let intent = root.join(record_name(
            expected.next_install_generation,
            InstallPhaseV1::Intent,
        ));
        let late = LedgerExpectationV1 {
            now_utc_seconds: approval.payload().expires_utc_seconds + 1,
            ..expected.clone()
        };
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &approval, &late)
                .err()
                .unwrap(),
            "WAKE_LEDGER_FRESH_APPROVAL_EXPIRED"
        );
        assert!(!intent.exists());
        std::fs::remove_file(root.join(GLOBAL_LOCK_NAME)).expect("remove fixture lock");
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &approval, &expected)
                .err()
                .unwrap(),
            "WAKE_LEDGER_LOCK_UNPROVISIONED"
        );
        assert!(!intent.exists());
        std::fs::write(root.join(GLOBAL_LOCK_NAME), b"").expect("restore fixture lock for cleanup");
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn original_desired_state_is_immutable_across_reopen() {
        let (root, handle, approval, mut expected) = fixture();
        expected.original_desired_state = OriginalDesiredStateV1::Paused;
        let lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("paused intent");
        drop(lease);
        let changed = LedgerExpectationV1 {
            original_desired_state: OriginalDesiredStateV1::Running,
            ..expected.clone()
        };
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &approval, &changed)
                .err()
                .unwrap(),
            "WAKE_LEDGER_APPROVAL_CONFLICT"
        );
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn deleted_middle_generation_blocks_later_approval_before_intent() {
        let (root, handle, first_approval, first_expected) = fixture();
        let mut prior_output: Option<(String, ObjectIdentityV1)> = None;
        for generation in 2..=4 {
            let approval = if generation == 2 {
                first_approval.clone()
            } else {
                let (package, pointer) = prior_output.as_ref().expect("prior fixture output");
                signed_for_generation_after(
                    &first_expected.protected_root,
                    generation,
                    generation as u128,
                    package,
                    pointer,
                )
            };
            let (predecessor_package_id, predecessor_pointer) =
                prior_output.as_ref().cloned().unwrap_or_else(|| {
                    (
                        first_expected.predecessor_package_id.clone(),
                        first_expected.predecessor_pointer.clone(),
                    )
                });
            let expected = LedgerExpectationV1 {
                next_install_generation: generation,
                predecessor_package_id,
                predecessor_pointer,
                ..first_expected.clone()
            };
            let mut lease =
                BrokerLedgerLease::begin(&handle, &approval, &expected).expect("historical intent");
            for phase in InstallPhaseV1::ORDER.iter().copied().skip(1) {
                lease.advance(phase).expect("complete history");
            }
            prior_output = Some((
                approval.payload().manifest.package_id.clone(),
                lease
                    .binding
                    .pointer_switch_binding
                    .as_ref()
                    .expect("fixture switched pointer")
                    .pointer_file
                    .clone(),
            ));
        }
        remove_generation_records(&root, 3);
        let fifth = signed_for_generation(&first_expected.protected_root, 5, 5);
        let expected_fifth = LedgerExpectationV1 {
            next_install_generation: 5,
            ..first_expected.clone()
        };
        let fifth_intent = root.join(record_name(5, InstallPhaseV1::Intent));
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &fifth, &expected_fifth)
                .err()
                .unwrap(),
            "WAKE_LEDGER_STALE_GENERATION"
        );
        assert!(!fifth_intent.exists());
        remove_generation_records(&root, 4);
        drop(handle);
        remove_fixture(&root, first_expected.next_install_generation);
    }

    #[test]
    fn duplicate_historical_approval_id_blocks_a_third_unique_candidate() {
        let (root, handle, first_approval, first_expected) = fixture();
        let mut prior_output: Option<(String, ObjectIdentityV1)> = None;
        for generation in 2..=3 {
            let approval = if generation == 2 {
                first_approval.clone()
            } else {
                let (package, pointer) = prior_output.as_ref().expect("prior fixture output");
                signed_for_generation_after(
                    &first_expected.protected_root,
                    generation,
                    3,
                    package,
                    pointer,
                )
            };
            let (predecessor_package_id, predecessor_pointer) =
                prior_output.as_ref().cloned().unwrap_or_else(|| {
                    (
                        first_expected.predecessor_package_id.clone(),
                        first_expected.predecessor_pointer.clone(),
                    )
                });
            let expected = LedgerExpectationV1 {
                next_install_generation: generation,
                predecessor_package_id,
                predecessor_pointer,
                ..first_expected.clone()
            };
            let mut lease =
                BrokerLedgerLease::begin(&handle, &approval, &expected).expect("historical intent");
            for phase in InstallPhaseV1::ORDER.iter().copied().skip(1) {
                lease.advance(phase).expect("complete history");
            }
            prior_output = Some((
                approval.payload().manifest.package_id.clone(),
                lease
                    .binding
                    .pointer_switch_binding
                    .as_ref()
                    .expect("fixture switched pointer")
                    .pointer_file
                    .clone(),
            ));
        }
        // Corrupt only this isolated test fixture into an internally canonical
        // and correctly chained history that reuses generation 2's ID.
        let mut prior_digest = "0".repeat(64);
        for phase in InstallPhaseV1::ORDER {
            let path = root.join(record_name(3, phase));
            let mut record: LedgerRecordV1 =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            record.approval_id = first_approval.payload().approval_id.clone();
            record.prior_record_sha256 = prior_digest;
            let bytes = canonical_bytes(&record).expect("canonical fixture");
            prior_digest = record_digest(&bytes);
            std::fs::write(path, bytes).expect("write exact fixture record");
        }
        let fourth = signed_for_generation(&first_expected.protected_root, 4, 4);
        let expected_fourth = LedgerExpectationV1 {
            next_install_generation: 4,
            ..first_expected.clone()
        };
        let fourth_intent = root.join(record_name(4, InstallPhaseV1::Intent));
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &fourth, &expected_fourth)
                .err()
                .unwrap(),
            "WAKE_LEDGER_APPROVAL_REPLAY"
        );
        assert!(!fourth_intent.exists());
        remove_generation_records(&root, 3);
        drop(handle);
        remove_fixture(&root, first_expected.next_install_generation);
    }

    #[test]
    fn root_handle_prior_query_cannot_hide_future_phase_residue() {
        let (root, handle, approval, expected) = fixture();
        let mut pattern: Vec<u16> = "unrelated-*".encode_utf16().collect();
        let mut filter = UnicodeString {
            length: (pattern.len() * 2) as u16,
            maximum_length: (pattern.len() * 2) as u16,
            buffer: pattern.as_mut_ptr(),
        };
        let mut status = IoStatusBlock {
            status: 0,
            information: 0,
        };
        let mut output = [0u64; 128];
        let result = unsafe {
            NtQueryDirectoryFile(
                handle.as_raw_handle(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut status,
                output.as_mut_ptr().cast(),
                std::mem::size_of_val(&output) as u32,
                FILE_NAMES_INFORMATION,
                1,
                &mut filter,
                1,
            )
        } as u32;
        assert!(matches!(result, STATUS_NO_SUCH_FILE | STATUS_NO_MORE_FILES));
        let intent = root.join(record_name(
            expected.next_install_generation,
            InstallPhaseV1::Intent,
        ));
        std::fs::write(
            root.join(record_name(
                expected.next_install_generation,
                InstallPhaseV1::Published,
            )),
            b"foreign phase residue",
        )
        .expect("residue");
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &approval, &expected)
                .err()
                .unwrap(),
            "WAKE_LEDGER_RECORD_CORRUPT"
        );
        assert!(!intent.exists());
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }

    #[test]
    fn duplicate_phase_rechecks_current_record_instead_of_claiming_idempotence() {
        let (root, handle, approval, expected) = fixture();
        let mut lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("intent");
        lease.advance(InstallPhaseV1::StageStarted).expect("marker");
        lease.advance(InstallPhaseV1::Staged).expect("stage");
        let stage = root.join(record_name(
            expected.next_install_generation,
            InstallPhaseV1::Staged,
        ));
        std::fs::write(stage, b"{}").expect("corrupt isolated stage fixture");
        assert_eq!(
            lease.advance(InstallPhaseV1::Staged).unwrap_err(),
            "WAKE_LEDGER_RECORD_CORRUPT"
        );
        drop(lease);
        drop(handle);
        remove_fixture(&root, expected.next_install_generation);
    }
}
