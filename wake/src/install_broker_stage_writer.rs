//! Broker-owned first mutation of an exact signed Wake package.
//!
//! This source slice deliberately stops at a protected STAGED record. Only a
//! fixed, separately provisioned service may eventually use it. It never
//! publishes a package, switches a pointer, stops an owner, or launches Wake.
//! A residual `staging/candidate` is sticky attention, even if its bytes match.

#![cfg(windows)]

use crate::{
    Result,
    install_broker_approval::{ObjectIdentityV1, VerifiedApprovalV1},
    install_broker_candidate_stage::{
        BrokerCandidateStageReadbackV1, inspect_stage_under_guard,
        require_empty_staging_under_guard,
    },
    install_broker_durability::{ParentFlushPoint, flush_pinned_directory},
    install_broker_ledger::{
        ApprovalContinuityV1, BrokerLedgerLease, InstallPhaseV1, StageBindingV1,
    },
    install_broker_policy::{BrokerProtectedPolicyV1, PolicyVerifiedApprovalV1},
    install_broker_preflight::{canonical_protected_security, protected_security_raw},
    install_broker_source::{PinnedCandidateSourceV1, open_fixed_candidate_source},
};
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};

const FILE_OPEN: u32 = 1;
const FILE_CREATE: u32 = 2;
const FILE_READ_DATA: u32 = 1;
const FILE_WRITE_DATA: u32 = 2;
const FILE_ADD_FILE: u32 = 2;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const READ_CONTROL: u32 = 0x0002_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_READ: u32 = 1;
const FILE_SHARE_WRITE: u32 = 2;
const FILE_DIRECTORY_FILE: u32 = 1;
const FILE_NON_DIRECTORY_FILE: u32 = 0x40;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x20;
const FILE_WRITE_THROUGH: u32 = 2;
const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;
const STAGING: &str = "staging";
const CANDIDATE: &str = "candidate";
const MANIFEST: &str = "manifest.json";
const ARTIFACTS: [&str; 4] = [
    "CatDeskWakeHost.exe",
    "CatDeskBinagotchy.exe",
    "adapter.py",
    "wake_bridge.py",
];

enum StageSecurityMode {
    Protected(String),
    #[cfg(test)]
    Fixture,
}

impl StageSecurityMode {
    fn service_sid(&self) -> Option<&str> {
        match self {
            Self::Protected(value) => Some(value),
            #[cfg(test)]
            Self::Fixture => None,
        }
    }

    fn require_empty(&self, root: &OwnedHandle, root_id: &ObjectIdentityV1) -> Result<()> {
        match self {
            Self::Protected(sid) => require_empty_staging_under_guard(root, root_id, sid),
            #[cfg(test)]
            Self::Fixture => {
                crate::install_broker_candidate_stage::require_empty_staging_fixture(root)
            }
        }
    }

    fn inspect(
        &self,
        root: &OwnedHandle,
        root_id: &ObjectIdentityV1,
        approval: &VerifiedApprovalV1,
    ) -> Result<BrokerCandidateStageReadbackV1> {
        match self {
            Self::Protected(sid) => inspect_stage_under_guard(root, root_id, approval, sid),
            #[cfg(test)]
            Self::Fixture => crate::install_broker_candidate_stage::inspect_stage_fixture(
                root, root_id, approval,
            ),
        }
    }
}

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
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileInformationByHandle(handle: *mut c_void, info: *mut FileInfo) -> i32;
    fn LocalFree(pointer: *mut c_void) -> *mut c_void;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        text: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        length: *mut u32,
    ) -> i32;
}

struct LocalDescriptor(*mut c_void);
impl Drop for LocalDescriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0) };
        }
    }
}

fn identity(raw: *mut c_void, directory: bool) -> Result<ObjectIdentityV1> {
    let mut info = FileInfo::default();
    if unsafe { GetFileInformationByHandle(raw, &mut info) } == 0
        || (info.attributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (!directory && info.links != 1)
        || info.volume == 0
        || (info.index_high == 0 && info.index_low == 0)
    {
        return Err("WAKE_BROKER_STAGE_OBJECT_INVALID".into());
    }
    Ok(ObjectIdentityV1 {
        volume_serial: info.volume,
        file_index_high: info.index_high,
        file_index_low: info.index_low,
    })
}

fn descriptor(service_sid: &str, directory: bool) -> Result<LocalDescriptor> {
    let sddl = canonical_protected_security(service_sid, directory)?;
    let wide = sddl.encode_utf16().chain([0]).collect::<Vec<_>>();
    let mut raw = ptr::null_mut();
    let mut length = 0_u32;
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            1,
            &mut raw,
            &mut length,
        )
    } == 0
        || raw.is_null()
        || length == 0
        || length > 16 * 1024
    {
        if !raw.is_null() {
            unsafe { LocalFree(raw) };
        }
        return Err("WAKE_BROKER_STAGE_SECURITY_INVALID".into());
    }
    Ok(LocalDescriptor(raw))
}

fn open_child(
    parent: &OwnedHandle,
    component: &'static str,
    directory: bool,
    create: bool,
    service_sid: Option<&str>,
) -> Result<OwnedHandle> {
    let mut units = component.encode_utf16().collect::<Vec<_>>();
    let length =
        u16::try_from(units.len() * 2).map_err(|_| "WAKE_BROKER_STAGE_COMPONENT_INVALID")?;
    let mut name = UnicodeString {
        length,
        maximum_length: length,
        buffer: units.as_mut_ptr(),
    };
    let sd = if create {
        service_sid
            .map(|sid| descriptor(sid, directory))
            .transpose()?
    } else {
        None
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: parent.as_raw_handle(),
        object_name: &mut name,
        attributes: 0x40,
        security_descriptor: sd.as_ref().map_or(ptr::null_mut(), |sd| sd.0),
        security_quality_of_service: ptr::null_mut(),
    };
    let mut iosb = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut raw = ptr::null_mut();
    let access = FILE_READ_ATTRIBUTES
        | READ_CONTROL
        | SYNCHRONIZE
        | if create {
            if directory {
                FILE_ADD_FILE
            } else {
                FILE_WRITE_DATA
            }
        } else {
            FILE_READ_DATA
        };
    let status = unsafe {
        NtCreateFile(
            &mut raw,
            access,
            &mut attributes,
            &mut iosb,
            ptr::null_mut(),
            if directory {
                FILE_ATTRIBUTE_DIRECTORY
            } else {
                0
            },
            if directory {
                if create || component == STAGING {
                    FILE_SHARE_READ | FILE_SHARE_WRITE
                } else {
                    FILE_SHARE_READ
                }
            } else if create {
                0
            } else {
                FILE_SHARE_READ
            },
            if create { FILE_CREATE } else { FILE_OPEN },
            FILE_SYNCHRONOUS_IO_NONALERT
                | FILE_OPEN_REPARSE_POINT
                | if create { FILE_WRITE_THROUGH } else { 0 }
                | if directory {
                    FILE_DIRECTORY_FILE
                } else {
                    FILE_NON_DIRECTORY_FILE
                },
            ptr::null_mut(),
            0,
        )
    };
    if status < 0 || raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err(if create {
            "WAKE_BROKER_STAGE_CREATE_FAILED"
        } else {
            "WAKE_BROKER_STAGE_OBJECT_UNAVAILABLE"
        }
        .into());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    identity(handle.as_raw_handle(), directory)?;
    if create && let Some(service_sid) = service_sid {
        let (owner, actual) = protected_security_raw(handle.as_raw_handle())?;
        if owner != "S-1-5-18" || actual != canonical_protected_security(service_sid, directory)? {
            return Err("WAKE_BROKER_STAGE_ACL_MISMATCH".into());
        }
    }
    Ok(handle)
}

fn write_blob(
    parent: &OwnedHandle,
    name: &'static str,
    bytes: &[u8],
    sid: Option<&str>,
) -> Result<()> {
    let handle = open_child(parent, name, false, true, sid)?;
    let mut file = File::from(handle);
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "WAKE_BROKER_STAGE_WRITE_FAILED".into())
}

fn copy_artifact(
    parent: &OwnedHandle,
    name: &'static str,
    source: &mut File,
    expected_length: u64,
    expected_sha256: &str,
    sid: Option<&str>,
) -> Result<()> {
    let handle = open_child(parent, name, false, true, sid)?;
    let mut destination = File::from(handle);
    copy_exact(source, &mut destination, expected_length, expected_sha256)
}

fn copy_exact(
    source: &mut File,
    destination: &mut File,
    expected_length: u64,
    expected_sha256: &str,
) -> Result<()> {
    source
        .seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_BROKER_SOURCE_CHANGED")?;
    let mut digest = Sha256::new();
    let mut remaining = expected_length;
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let limit = buffer.len().min(remaining as usize);
        let read = source
            .read(&mut buffer[..limit])
            .map_err(|_| "WAKE_BROKER_SOURCE_CHANGED")?;
        if read == 0 {
            return Err("WAKE_BROKER_SOURCE_CHANGED".into());
        }
        digest.update(&buffer[..read]);
        destination
            .write_all(&buffer[..read])
            .map_err(|_| "WAKE_BROKER_STAGE_WRITE_FAILED")?;
        remaining -= read as u64;
    }
    if source
        .read(&mut buffer[..1])
        .map_err(|_| "WAKE_BROKER_SOURCE_CHANGED")?
        != 0
        || format!("{:x}", digest.finalize()) != expected_sha256
    {
        return Err("WAKE_BROKER_SOURCE_CHANGED".into());
    }
    destination
        .sync_all()
        .map_err(|_| "WAKE_BROKER_STAGE_WRITE_FAILED".into())
}

/// Retains the fresh-create stage and source pins until STAGED is recorded.
/// Its private fields prevent a caller from inventing a physical stage claim.
pub struct BrokerStageReceiptV1<'p> {
    approval: VerifiedApprovalV1,
    policy: Option<&'p BrokerProtectedPolicyV1>,
    source: PinnedCandidateSourceV1,
    security: StageSecurityMode,
    snapshot: BrokerCandidateStageReadbackV1,
    _staging: OwnedHandle,
    candidate: OwnedHandle,
    manifest: OwnedHandle,
    artifacts: [OwnedHandle; 4],
}

impl BrokerStageReceiptV1<'_> {
    pub(crate) fn stage_binding(&self) -> StageBindingV1 {
        StageBindingV1 {
            staging_directory: self.snapshot.staging_directory.clone(),
            candidate_directory: self.snapshot.candidate_directory.clone(),
            manifest_file: self.snapshot.manifest_file.clone(),
            artifacts: std::array::from_fn(|index| self.snapshot.artifacts[index].object.clone()),
        }
    }

    pub(crate) fn revalidate_for_ledger(
        &mut self,
        root: &OwnedHandle,
        root_id: &ObjectIdentityV1,
        generation: u64,
        approval_id: &str,
        signed_digest: &str,
        continuity: &ApprovalContinuityV1,
    ) -> Result<()> {
        if let Some(policy) = self.policy {
            let readback = policy.readback()?;
            let (service_sid, _) = policy.checked_endpoint_sids()?;
            if readback.protected_root != *root_id
                || self.security.service_sid() != Some(service_sid)
                || readback.policy_file != continuity.policy_file
                || readback.policy_sha256 != continuity.policy_sha256
                || readback.approval_public_key_sha256 != continuity.approval_public_key_sha256
            {
                return Err("WAKE_BROKER_STAGE_POLICY_CHANGED".into());
            }
        } else {
            #[cfg(not(test))]
            return Err("WAKE_BROKER_STAGE_POLICY_CHANGED".into());
            #[cfg(test)]
            if !matches!(self.security, StageSecurityMode::Fixture) {
                return Err("WAKE_BROKER_STAGE_POLICY_CHANGED".into());
            }
        }
        if self.approval.payload().install_generation != generation
            || self.approval.payload().approval_id != approval_id
            || self.approval.signed_payload_sha256() != signed_digest
            || self.approval.signed_envelope_sha256() != continuity.signed_envelope_sha256
            || self.approval.verification_key_sha256() != continuity.approval_public_key_sha256
            || self.snapshot.protected_root != *root_id
        {
            return Err("WAKE_BROKER_STAGE_APPROVAL_BINDING_INVALID".into());
        }
        self.source.revalidate(&self.approval)?;
        let current = self.security.inspect(root, root_id, &self.approval)?;
        if current != self.snapshot
            || identity(self._staging.as_raw_handle(), true)? != self.snapshot.staging_directory
            || identity(self.candidate.as_raw_handle(), true)? != self.snapshot.candidate_directory
            || identity(self.manifest.as_raw_handle(), false)? != self.snapshot.manifest_file
            || self.artifacts.iter().enumerate().any(|(index, file)| {
                identity(file.as_raw_handle(), false).ok()
                    != Some(self.snapshot.artifacts[index].object.clone())
            })
        {
            return Err("WAKE_BROKER_STAGE_OBJECT_CHANGED".into());
        }
        Ok(())
    }
}

/// Copy only the five fixed source entries into a freshly created, fixed
/// protected slot while INTENT/global lease is live. An existing slot fails
/// closed. The receipt is not publication or activation authority.
pub fn materialize_fixed_candidate_stage<'p>(
    lease: &mut BrokerLedgerLease<'_>,
    approved: &PolicyVerifiedApprovalV1<'p>,
) -> Result<BrokerStageReceiptV1<'p>> {
    let policy = approved.policy();
    let approval = approved.verified();
    if lease.phase() == InstallPhaseV1::StageStarted {
        return Err("WAKE_BROKER_STAGE_ATTENTION".into());
    }
    if lease.phase() != InstallPhaseV1::Intent || !lease.matches_policy_approval(approved) {
        return Err("WAKE_BROKER_STAGE_APPROVAL_BINDING_INVALID".into());
    }
    let source = open_fixed_candidate_source(approval)?;
    policy.revalidate()?;
    let policy_root = policy.readback()?.protected_root;
    let root = lease.protected_root();
    if identity(root.as_raw_handle(), true)? != policy_root
        || approval.payload().target_protected_root != policy_root
    {
        return Err("WAKE_BROKER_STAGE_ROOT_CHANGED".into());
    }
    let (service_sid, _) = policy.checked_endpoint_sids()?;
    let result = materialize_pinned_candidate_stage(
        lease,
        approval,
        source,
        StageSecurityMode::Protected(service_sid.into()),
        Some(policy),
    )?;
    policy.revalidate()?;
    Ok(result)
}

fn materialize_pinned_candidate_stage<'p>(
    lease: &mut BrokerLedgerLease<'_>,
    approval: &VerifiedApprovalV1,
    mut source: PinnedCandidateSourceV1,
    security: StageSecurityMode,
    policy: Option<&'p BrokerProtectedPolicyV1>,
) -> Result<BrokerStageReceiptV1<'p>> {
    if lease.phase() == InstallPhaseV1::StageStarted {
        return Err("WAKE_BROKER_STAGE_ATTENTION".into());
    }
    if lease.phase() != InstallPhaseV1::Intent || !lease.matches_approval(approval) {
        return Err("WAKE_BROKER_STAGE_APPROVAL_BINDING_INVALID".into());
    }
    if let Some(policy) = policy {
        let readback = policy.readback()?;
        let continuity = lease.approval_continuity();
        if readback.policy_file != continuity.policy_file
            || readback.policy_sha256 != continuity.policy_sha256
            || readback.approval_public_key_sha256 != continuity.approval_public_key_sha256
        {
            return Err("WAKE_BROKER_STAGE_POLICY_CHANGED".into());
        }
    } else {
        #[cfg(not(test))]
        return Err("WAKE_BROKER_STAGE_POLICY_CHANGED".into());
        #[cfg(test)]
        if !matches!(security, StageSecurityMode::Fixture) {
            return Err("WAKE_BROKER_STAGE_POLICY_CHANGED".into());
        }
    }
    let root_id = identity(lease.protected_root().as_raw_handle(), true)?;
    if approval.payload().target_protected_root != root_id {
        return Err("WAKE_BROKER_STAGE_ROOT_CHANGED".into());
    }
    source.revalidate(approval)?;
    security.require_empty(lease.protected_root(), &root_id)?;
    let service_sid = security.service_sid();
    let staging = open_child(lease.protected_root(), STAGING, true, false, service_sid)?;
    let staging_id = identity(staging.as_raw_handle(), true)?;
    if let Some(service_sid) = service_sid {
        let (owner, actual) = protected_security_raw(staging.as_raw_handle())?;
        if owner != "S-1-5-18" || actual != canonical_protected_security(service_sid, true)? {
            return Err("WAKE_BROKER_STAGE_ACL_MISMATCH".into());
        }
    }
    // No candidate create is allowed until this write-ahead marker and its
    // parent directory entry are durable. A later failure is sticky attention.
    lease.record_stage_started()?;
    let root = lease.protected_root();
    let candidate = open_child(&staging, CANDIDATE, true, true, service_sid)?;
    let created_id = identity(candidate.as_raw_handle(), true)?;
    flush_pinned_directory(&staging, &staging_id, ParentFlushPoint::StageCandidateEntry)?;
    let payload = approval.payload();
    for (index, name) in ARTIFACTS.into_iter().enumerate() {
        let signed = &payload.manifest.artifacts[index];
        copy_artifact(
            &candidate,
            name,
            source.artifact_mut(index),
            signed.byte_length,
            &signed.sha256,
            service_sid,
        )?;
        #[cfg(test)]
        if FAIL_AFTER_ARTIFACT.with(|failure| {
            if failure.get() == Some(index) {
                failure.set(None);
                true
            } else {
                false
            }
        }) {
            return Err("WAKE_BROKER_STAGE_FAULT_INJECTED".into());
        }
    }
    write_blob(&candidate, MANIFEST, source.manifest_bytes(), service_sid)?;
    flush_pinned_directory(&candidate, &created_id, ParentFlushPoint::StageLeafEntries)?;
    drop(candidate);
    drop(staging);
    source.revalidate(approval)?;
    let snapshot = security.inspect(root, &root_id, approval)?;
    if snapshot.candidate_directory != created_id {
        return Err("WAKE_BROKER_STAGE_OBJECT_CHANGED".into());
    }
    let staging = open_child(root, STAGING, true, false, service_sid)?;
    let candidate = open_child(&staging, CANDIDATE, true, false, service_sid)?;
    let manifest = open_child(&candidate, MANIFEST, false, false, service_sid)?;
    let artifacts = ARTIFACTS
        .into_iter()
        .map(|name| open_child(&candidate, name, false, false, service_sid))
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| "WAKE_BROKER_STAGE_INVENTORY_MISMATCH")?;
    let mut receipt = BrokerStageReceiptV1 {
        approval: approval.clone(),
        policy,
        source,
        security,
        snapshot,
        _staging: staging,
        candidate,
        manifest,
        artifacts,
    };
    receipt.revalidate_for_ledger(
        root,
        &root_id,
        payload.install_generation,
        &payload.approval_id,
        approval.signed_payload_sha256(),
        lease.approval_continuity(),
    )?;
    Ok(receipt)
}

#[cfg(test)]
thread_local! {
    static FAIL_AFTER_ARTIFACT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        install_broker_approval::{self, ArtifactV1, verify_approval},
        install_broker_durability::fail_next_flush,
        install_broker_ledger::{BrokerLedgerGuard, LedgerExpectationV1, OriginalDesiredStateV1},
        install_broker_source::open_fixture_candidate_source,
    };
    use std::{
        os::windows::ffi::OsStrExt,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(1);

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const OPEN_EXISTING: u32 = 3;

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

    struct TransactionFixture {
        base: PathBuf,
        root: OwnedHandle,
        approval: VerifiedApprovalV1,
        expectation: LedgerExpectationV1,
    }

    impl TransactionFixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "catdesk-stage-transaction-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let source = base.join("WakeSubmissionV1").join("candidate");
            std::fs::create_dir_all(&source).expect("source fixture");
            std::fs::create_dir(base.join(STAGING)).expect("staging fixture");
            std::fs::write(base.join("install-ledger.lock"), b"").expect("global lock");
            let blobs: [&[u8]; 4] = [
                b"host-binary",
                b"gui-binary",
                b"adapter-code",
                b"bridge-code",
            ];
            let mut units = base
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let raw = unsafe {
                CreateFileW(
                    units.as_mut_ptr(),
                    FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_OPEN_REPARSE_POINT,
                    ptr::null_mut(),
                )
            };
            assert!(!raw.is_null() && raw != INVALID_HANDLE_VALUE);
            let root = unsafe { OwnedHandle::from_raw_handle(raw) };
            let root_id = identity(root.as_raw_handle(), true).expect("root id");
            let (mut payload, mut observed, key) = install_broker_approval::tests::fixture();
            payload.target_protected_root = root_id.clone();
            observed.target_protected_root = root_id.clone();
            payload.manifest.artifacts = std::array::from_fn(|index| ArtifactV1 {
                name: ARTIFACTS[index].into(),
                byte_length: blobs[index].len() as u64,
                sha256: format!("{:x}", Sha256::digest(blobs[index])),
            });
            payload.manifest.package_id = format!(
                "{}-{}-{}",
                payload.manifest.version,
                &payload.manifest.artifacts[0].sha256[..12],
                &payload.manifest.artifacts[1].sha256[..12]
            );
            let manifest = serde_json::to_vec(&payload.manifest).expect("manifest");
            payload.manifest_sha256 = format!("{:x}", Sha256::digest(&manifest));
            for (name, blob) in ARTIFACTS.into_iter().zip(blobs) {
                std::fs::write(source.join(name), blob).expect("source artifact");
            }
            std::fs::write(source.join(MANIFEST), manifest).expect("source manifest");
            let envelope = install_broker_approval::tests::envelope(&payload, &key);
            let approval = verify_approval(&envelope, &key.verifying_key(), &observed)
                .expect("approved fixture");
            let expectation = LedgerExpectationV1 {
                now_utc_seconds: observed.now_utc_seconds,
                next_install_generation: payload.install_generation,
                predecessor_package_id: payload.predecessor_package_id,
                predecessor_pointer: payload.predecessor_pointer,
                protected_root: root_id,
                original_desired_state: OriginalDesiredStateV1::Running,
            };
            Self {
                base,
                root,
                approval,
                expectation,
            }
        }

        fn source(&self) -> PinnedCandidateSourceV1 {
            open_fixture_candidate_source(&self.base, &self.approval).expect("pinned source")
        }

        fn staged_path(&self) -> PathBuf {
            self.base.join(STAGING).join(CANDIDATE)
        }

        fn cleanup(self) {
            let base = self.base.clone();
            drop(self);
            std::fs::remove_dir_all(base).expect("fixture cleanup");
        }
    }

    fn fixture_paths() -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!(
            "catdesk-stage-copy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&base).expect("create fixture");
        (base.join("source"), base.join("stage"))
    }

    #[test]
    fn bounded_copy_hashes_exact_source_and_flushes_destination() {
        let (source_path, stage_path) = fixture_paths();
        let base = source_path.parent().unwrap().to_path_buf();
        let bytes = b"fixed approved artifact bytes";
        std::fs::write(&source_path, bytes).expect("source");
        let mut source = File::open(&source_path).expect("source pin");
        let mut stage = File::create(&stage_path).expect("stage fixture");
        let hash = format!("{:x}", Sha256::digest(bytes));
        copy_exact(&mut source, &mut stage, bytes.len() as u64, &hash).expect("exact copy");
        drop(stage);
        assert_eq!(std::fs::read(&stage_path).expect("read stage"), bytes);
        std::fs::remove_dir_all(base).expect("cleanup");
    }

    #[test]
    fn source_length_and_same_size_hash_mismatch_fail_closed() {
        let (source_path, stage_path) = fixture_paths();
        let base = source_path.parent().unwrap().to_path_buf();
        std::fs::write(&source_path, b"bad!").expect("source");
        let mut source = File::open(&source_path).expect("source pin");
        let mut stage = File::create(&stage_path).expect("stage fixture");
        let approved = format!("{:x}", Sha256::digest(b"good"));
        assert_eq!(
            copy_exact(&mut source, &mut stage, 4, &approved).unwrap_err(),
            "WAKE_BROKER_SOURCE_CHANGED"
        );
        assert_eq!(
            copy_exact(&mut source, &mut stage, 3, &approved).unwrap_err(),
            "WAKE_BROKER_SOURCE_CHANGED"
        );
        assert_eq!(
            copy_exact(&mut source, &mut stage, 5, &approved).unwrap_err(),
            "WAKE_BROKER_SOURCE_CHANGED"
        );
        drop(stage);
        std::fs::remove_dir_all(base).expect("cleanup");
    }

    #[test]
    fn intent_fresh_copy_typed_staged_record_and_exact_reopen() {
        let fixture = TransactionFixture::new();
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("global lock first");
        let mut lease = guard
            .begin_fixture(&fixture.approval, &fixture.expectation)
            .expect("durable intent");
        let mut receipt = materialize_pinned_candidate_stage(
            &mut lease,
            &fixture.approval,
            fixture.source(),
            StageSecurityMode::Fixture,
            None,
        )
        .expect("fresh protected-shape copy");
        let binding = receipt.stage_binding();
        assert!(lease.record_staged(&mut receipt).expect("typed STAGED"));
        assert_eq!(lease.phase(), InstallPhaseV1::Staged);
        drop(receipt);
        drop(lease);
        let bytes = std::fs::read(
            fixture
                .base
                .join("install-transaction-00000000000000000002-staged.json"),
        )
        .expect("staged record");
        let record: serde_json::Value = serde_json::from_slice(&bytes).expect("record JSON");
        assert_eq!(
            record["stageBinding"],
            serde_json::to_value(&binding).unwrap()
        );
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("reopen lock");
        let resumed = guard
            .begin_fixture(&fixture.approval, &fixture.expectation)
            .expect("exact resume");
        assert_eq!(resumed.phase(), InstallPhaseV1::Staged);
        drop(resumed);
        fixture.cleanup();
    }

    #[test]
    fn marker_flush_failure_never_creates_candidate_and_reopens_as_attention() {
        let fixture = TransactionFixture::new();
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("global lock");
        let mut lease = guard
            .begin_fixture(&fixture.approval, &fixture.expectation)
            .expect("intent");
        fail_next_flush(ParentFlushPoint::StageStarted);
        assert_eq!(
            materialize_pinned_candidate_stage(
                &mut lease,
                &fixture.approval,
                fixture.source(),
                StageSecurityMode::Fixture,
                None,
            )
            .err()
            .unwrap(),
            "WAKE_BROKER_PARENT_FLUSH_FAILED"
        );
        assert_eq!(lease.phase(), InstallPhaseV1::Intent);
        assert!(!fixture.staged_path().exists());
        assert!(
            fixture
                .base
                .join("install-transaction-00000000000000000002-stage-started.json")
                .exists()
        );
        drop(lease);
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("reopen lock");
        let mut resumed = guard
            .begin_fixture(&fixture.approval, &fixture.expectation)
            .expect("exact marker reopen");
        assert_eq!(resumed.phase(), InstallPhaseV1::StageStarted);
        assert_eq!(
            materialize_pinned_candidate_stage(
                &mut resumed,
                &fixture.approval,
                fixture.source(),
                StageSecurityMode::Fixture,
                None,
            )
            .err()
            .unwrap(),
            "WAKE_BROKER_STAGE_ATTENTION"
        );
        assert!(!fixture.staged_path().exists());
        drop(resumed);
        fixture.cleanup();
    }

    #[test]
    fn parent_flush_failures_leave_stage_attempt_sticky_across_reopen() {
        for (point, has_leaves) in [
            (ParentFlushPoint::StageCandidateEntry, false),
            (ParentFlushPoint::StageLeafEntries, true),
        ] {
            let fixture = TransactionFixture::new();
            let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("global lock");
            let mut lease = guard
                .begin_fixture(&fixture.approval, &fixture.expectation)
                .expect("intent");
            fail_next_flush(point);
            assert_eq!(
                materialize_pinned_candidate_stage(
                    &mut lease,
                    &fixture.approval,
                    fixture.source(),
                    StageSecurityMode::Fixture,
                    None,
                )
                .err()
                .unwrap(),
                "WAKE_BROKER_PARENT_FLUSH_FAILED"
            );
            assert_eq!(lease.phase(), InstallPhaseV1::StageStarted);
            assert!(fixture.staged_path().is_dir());
            assert_eq!(
                fixture.staged_path().join(ARTIFACTS[0]).exists(),
                has_leaves
            );
            assert!(
                !fixture
                    .base
                    .join("install-transaction-00000000000000000002-staged.json")
                    .exists()
            );
            drop(lease);
            let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("reopen lock");
            let mut resumed = guard
                .begin_fixture(&fixture.approval, &fixture.expectation)
                .expect("exact marker reopen");
            assert_eq!(resumed.phase(), InstallPhaseV1::StageStarted);
            assert_eq!(
                materialize_pinned_candidate_stage(
                    &mut resumed,
                    &fixture.approval,
                    fixture.source(),
                    StageSecurityMode::Fixture,
                    None,
                )
                .err()
                .unwrap(),
                "WAKE_BROKER_STAGE_ATTENTION"
            );
            assert!(fixture.staged_path().is_dir());
            drop(resumed);
            fixture.cleanup();
        }
    }

    #[test]
    fn injected_abort_after_first_artifact_retains_partial_stage_and_marker() {
        let fixture = TransactionFixture::new();
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("global lock");
        let mut lease = guard
            .begin_fixture(&fixture.approval, &fixture.expectation)
            .expect("intent");
        FAIL_AFTER_ARTIFACT.with(|failure| failure.set(Some(0)));
        assert_eq!(
            materialize_pinned_candidate_stage(
                &mut lease,
                &fixture.approval,
                fixture.source(),
                StageSecurityMode::Fixture,
                None,
            )
            .err()
            .unwrap(),
            "WAKE_BROKER_STAGE_FAULT_INJECTED"
        );
        assert_eq!(lease.phase(), InstallPhaseV1::StageStarted);
        assert!(fixture.staged_path().join(ARTIFACTS[0]).exists());
        assert!(!fixture.staged_path().join(ARTIFACTS[1]).exists());
        let bytes = std::fs::read(fixture.staged_path().join(ARTIFACTS[0])).unwrap();
        drop(lease);
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("reopen lock");
        let mut resumed = guard
            .begin_fixture(&fixture.approval, &fixture.expectation)
            .expect("marker reopen");
        assert_eq!(resumed.phase(), InstallPhaseV1::StageStarted);
        assert_eq!(
            materialize_pinned_candidate_stage(
                &mut resumed,
                &fixture.approval,
                fixture.source(),
                StageSecurityMode::Fixture,
                None,
            )
            .err()
            .unwrap(),
            "WAKE_BROKER_STAGE_ATTENTION"
        );
        assert_eq!(
            std::fs::read(fixture.staged_path().join(ARTIFACTS[0])).unwrap(),
            bytes
        );
        drop(resumed);
        fixture.cleanup();
    }

    #[test]
    fn staged_record_flush_failure_never_reuses_the_stage_slot() {
        let fixture = TransactionFixture::new();
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("global lock");
        let mut lease = guard
            .begin_fixture(&fixture.approval, &fixture.expectation)
            .expect("intent");
        let mut receipt = materialize_pinned_candidate_stage(
            &mut lease,
            &fixture.approval,
            fixture.source(),
            StageSecurityMode::Fixture,
            None,
        )
        .expect("copied stage");
        fail_next_flush(ParentFlushPoint::Staged);
        assert_eq!(
            lease.record_staged(&mut receipt).unwrap_err(),
            "WAKE_BROKER_PARENT_FLUSH_FAILED"
        );
        assert_eq!(lease.phase(), InstallPhaseV1::StageStarted);
        let before = std::fs::read(fixture.staged_path().join(ARTIFACTS[0])).unwrap();
        drop(receipt);
        drop(lease);
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("reopen lock");
        let mut resumed = guard
            .begin_fixture(&fixture.approval, &fixture.expectation)
            .expect("exact record reopen");
        // The simulated flush failure leaves an exact record on this test
        // filesystem. A real power loss may instead leave only StageStarted.
        assert_eq!(resumed.phase(), InstallPhaseV1::Staged);
        assert_eq!(
            materialize_pinned_candidate_stage(
                &mut resumed,
                &fixture.approval,
                fixture.source(),
                StageSecurityMode::Fixture,
                None,
            )
            .err()
            .unwrap(),
            "WAKE_BROKER_STAGE_APPROVAL_BINDING_INVALID"
        );
        assert_eq!(
            std::fs::read(fixture.staged_path().join(ARTIFACTS[0])).unwrap(),
            before
        );
        drop(resumed);
        fixture.cleanup();
    }

    #[test]
    fn residual_partial_stage_is_sticky_and_never_records_staged() {
        let fixture = TransactionFixture::new();
        std::fs::create_dir(fixture.staged_path()).expect("orphan candidate");
        std::fs::write(fixture.staged_path().join(ARTIFACTS[0]), b"partial")
            .expect("partial stage");
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("global lock first");
        let mut lease = guard
            .begin_fixture(&fixture.approval, &fixture.expectation)
            .expect("intent");
        let result = materialize_pinned_candidate_stage(
            &mut lease,
            &fixture.approval,
            fixture.source(),
            StageSecurityMode::Fixture,
            None,
        );
        assert_eq!(
            result.err().unwrap(),
            "WAKE_BROKER_STAGE_INVENTORY_MISMATCH"
        );
        assert_eq!(lease.phase(), InstallPhaseV1::Intent);
        assert!(
            !fixture
                .base
                .join("install-transaction-00000000000000000002-staged.json")
                .exists()
        );
        assert_eq!(
            std::fs::read(fixture.staged_path().join(ARTIFACTS[0])).unwrap(),
            b"partial"
        );
        drop(lease);
        fixture.cleanup();
    }
}
