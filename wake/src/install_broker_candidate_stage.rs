//! Read-only byte inventory for one proposed broker-owned candidate stage.
//!
//! The only production location is the compiled protected root's
//! `staging/candidate` slot. A future fixed broker must create and fill it
//! under its global transaction lease; this reader does not create, copy,
//! approve, publish, activate, or infer how the bytes arrived. In particular,
//! matching a signed manifest cannot distinguish a freshly copied stage from
//! stale identical bytes. A protected stage-creation record bound to the
//! ledger generation and pinned stage object is still required.

#![cfg(windows)]

use crate::{
    Result,
    install_broker_approval::{
        BrokerObservedStateV1, ObjectIdentityV1, VerifiedApprovalV1, validate_manifest,
    },
    install_broker_policy::BrokerProtectedPolicyV1,
    install_broker_preflight::{
        acquire_fixed_protected_root_for_reader, canonical_protected_security,
        protected_security_raw,
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::c_void,
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};

const STAGING_NAME: &str = "staging";
const SLOT_NAME: &str = "candidate";
const MANIFEST_NAME: &str = "manifest.json";
const ARTIFACT_NAMES: [&str; 4] = [
    "CatDeskWakeHost.exe",
    "CatDeskBinagotchy.exe",
    "adapter.py",
    "wake_bridge.py",
];
const MAX_MANIFEST_BYTES: usize = 4096;
const MAX_DIRECTORY_ENTRIES: usize = 16;
const FILE_OPEN: u32 = 1;
const FILE_READ_DATA: u32 = 1;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const READ_CONTROL: u32 = 0x0002_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_READ: u32 = 1;
const FILE_DIRECTORY_FILE: u32 = 1;
const FILE_NON_DIRECTORY_FILE: u32 = 0x40;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x20;
const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_NAMES_INFORMATION: u32 = 12;
const STATUS_NO_SUCH_FILE: u32 = 0xC000_000F;
const STATUS_NO_MORE_FILES: u32 = 0x8000_0006;
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
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateArtifactReadbackV1 {
    pub name: &'static str,
    pub byte_length: u64,
    pub sha256: String,
    pub object: ObjectIdentityV1,
}

/// A byte-consistency observation, never a stage provenance or install grant.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerCandidateStageReadbackV1 {
    pub schema_version: u32,
    pub approval_id: String,
    pub install_generation: u64,
    pub package_id: String,
    pub signed_payload_sha256: String,
    pub manifest_sha256: String,
    pub protected_root: ObjectIdentityV1,
    pub staging_directory: ObjectIdentityV1,
    pub candidate_directory: ObjectIdentityV1,
    pub manifest_file: ObjectIdentityV1,
    pub artifacts: [CandidateArtifactReadbackV1; 4],
    pub stage_creation_and_ledger_binding_proven: bool,
    pub disposition: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExactSecurity {
    owner: String,
    sddl: String,
}

#[derive(Clone)]
struct StagePolicy {
    root: ExactSecurity,
    staging: ExactSecurity,
    candidate: ExactSecurity,
    manifest: ExactSecurity,
    artifacts: [ExactSecurity; 4],
}

impl StagePolicy {
    fn production(service_sid: &str) -> Result<Self> {
        let directory = ExactSecurity {
            owner: "S-1-5-18".into(),
            sddl: canonical_protected_security(service_sid, true)?,
        };
        let file = ExactSecurity {
            owner: "S-1-5-18".into(),
            sddl: canonical_protected_security(service_sid, false)?,
        };
        Ok(Self {
            root: directory.clone(),
            staging: directory.clone(),
            candidate: directory,
            manifest: file.clone(),
            artifacts: std::array::from_fn(|_| file.clone()),
        })
    }
}

fn info(raw: *mut c_void, directory: bool) -> Result<FileInfo> {
    let mut result = FileInfo::default();
    if unsafe { GetFileInformationByHandle(raw, &mut result) } == 0 {
        return Err("WAKE_BROKER_STAGE_OBJECT_INVALID".into());
    }
    validate_info(result, directory)
}

fn validate_info(value: FileInfo, directory: bool) -> Result<FileInfo> {
    if value.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (value.attributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || (!directory && value.links != 1)
        || value.volume == 0
        || (value.index_high == 0 && value.index_low == 0)
    {
        return Err("WAKE_BROKER_STAGE_OBJECT_INVALID".into());
    }
    Ok(value)
}

fn identity(value: &FileInfo) -> ObjectIdentityV1 {
    ObjectIdentityV1 {
        volume_serial: value.volume,
        file_index_high: value.index_high,
        file_index_low: value.index_low,
    }
}

fn stable(before: &FileInfo, after: &FileInfo) -> bool {
    before.attributes == after.attributes
        && before.creation_low == after.creation_low
        && before.creation_high == after.creation_high
        && before.write_low == after.write_low
        && before.write_high == after.write_high
        && before.volume == after.volume
        && before.size_high == after.size_high
        && before.size_low == after.size_low
        && before.links == after.links
        && before.index_high == after.index_high
        && before.index_low == after.index_low
}

fn security(raw: *mut c_void) -> Result<ExactSecurity> {
    let (owner, sddl) = protected_security_raw(raw)?;
    Ok(ExactSecurity { owner, sddl })
}

fn require_security(raw: *mut c_void, expected: &ExactSecurity) -> Result<()> {
    if security(raw)? != *expected {
        return Err("WAKE_BROKER_STAGE_ACL_MISMATCH".into());
    }
    Ok(())
}

fn open_relative(parent: &OwnedHandle, name: &'static str, directory: bool) -> Result<OwnedHandle> {
    // Every production component is compiled above, never parsed from a
    // request, approval package ID, environment variable, or stage manifest.
    let mut units: Vec<u16> = name.encode_utf16().collect();
    let length =
        u16::try_from(units.len() * 2).map_err(|_| "WAKE_BROKER_STAGE_COMPONENT_INVALID")?;
    let mut object_name = UnicodeString {
        length,
        maximum_length: length,
        buffer: units.as_mut_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: parent.as_raw_handle(),
        object_name: &mut object_name,
        attributes: 0x40, // OBJ_CASE_INSENSITIVE; exact inventory checks case
        security_descriptor: ptr::null_mut(),
        security_quality_of_service: ptr::null_mut(),
    };
    let mut iosb = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut raw = ptr::null_mut();
    let status = unsafe {
        NtCreateFile(
            &mut raw,
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            &mut attributes,
            &mut iosb,
            ptr::null_mut(),
            0,
            FILE_SHARE_READ, // hold each object against write/delete sharing
            FILE_OPEN,
            FILE_SYNCHRONOUS_IO_NONALERT
                | FILE_OPEN_REPARSE_POINT
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
        return Err("WAKE_BROKER_STAGE_OBJECT_UNAVAILABLE".into());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    info(handle.as_raw_handle(), directory)?;
    Ok(handle)
}

fn reopen_directory_for_inventory(directory: &OwnedHandle) -> Result<OwnedHandle> {
    // A new FILE_OBJECT avoids a prior search filter hiding an entry.
    let mut empty = UnicodeString {
        length: 0,
        maximum_length: 0,
        buffer: ptr::null_mut(),
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: directory.as_raw_handle(),
        object_name: &mut empty,
        attributes: 0x40,
        security_descriptor: ptr::null_mut(),
        security_quality_of_service: ptr::null_mut(),
    };
    let mut iosb = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut raw = ptr::null_mut();
    let status = unsafe {
        NtCreateFile(
            &mut raw,
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &mut attributes,
            &mut iosb,
            ptr::null_mut(),
            0,
            FILE_SHARE_READ,
            FILE_OPEN,
            FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
            ptr::null_mut(),
            0,
        )
    };
    if status < 0 || raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err("WAKE_BROKER_STAGE_INVENTORY_UNAVAILABLE".into());
    }
    let reopened = unsafe { OwnedHandle::from_raw_handle(raw) };
    if identity(&info(reopened.as_raw_handle(), true)?)
        != identity(&info(directory.as_raw_handle(), true)?)
    {
        return Err("WAKE_BROKER_STAGE_OBJECT_CHANGED".into());
    }
    Ok(reopened)
}

fn exact_inventory(directory: &OwnedHandle, expected: &[&str]) -> Result<()> {
    let fresh = reopen_directory_for_inventory(directory)?;
    let expected: BTreeSet<String> = expected.iter().map(|name| (*name).to_string()).collect();
    let mut seen = BTreeSet::new();
    for index in 0..=MAX_DIRECTORY_ENTRIES {
        let mut aligned = [0u64; 128];
        let mut iosb = IoStatusBlock {
            status: 0,
            information: 0,
        };
        let status = unsafe {
            NtQueryDirectoryFile(
                fresh.as_raw_handle(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut iosb,
                aligned.as_mut_ptr().cast(),
                std::mem::size_of_val(&aligned) as u32,
                FILE_NAMES_INFORMATION,
                1,
                ptr::null_mut(),
                u8::from(index == 0),
            )
        } as u32;
        if status == STATUS_NO_MORE_FILES || (index == 0 && status == STATUS_NO_SUCH_FILE) {
            return if seen == expected {
                Ok(())
            } else {
                Err("WAKE_BROKER_STAGE_INVENTORY_MISMATCH".into())
            };
        }
        if status != 0
            || index == MAX_DIRECTORY_ENTRIES
            || iosb.information < 12
            || iosb.information > std::mem::size_of_val(&aligned)
        {
            return Err("WAKE_BROKER_STAGE_INVENTORY_UNAVAILABLE".into());
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(aligned.as_ptr().cast::<u8>(), iosb.information) };
        let next = u32::from_ne_bytes(bytes[..4].try_into().unwrap());
        let name_length = u32::from_ne_bytes(bytes[8..12].try_into().unwrap()) as usize;
        if next != 0
            || name_length == 0
            || !name_length.is_multiple_of(2)
            || name_length > bytes.len() - 12
        {
            return Err("WAKE_BROKER_STAGE_INVENTORY_UNAVAILABLE".into());
        }
        let units: Vec<u16> = bytes[12..12 + name_length]
            .chunks_exact(2)
            .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
            .collect();
        let name =
            String::from_utf16(&units).map_err(|_| "WAKE_BROKER_STAGE_INVENTORY_MISMATCH")?;
        if matches!(name.as_str(), "." | "..") {
            continue;
        }
        if !expected.contains(name.as_str()) || !seen.insert(name) {
            return Err("WAKE_BROKER_STAGE_INVENTORY_MISMATCH".into());
        }
    }
    Err("WAKE_BROKER_STAGE_INVENTORY_UNAVAILABLE".into())
}

fn read_bounded(file: &mut File, expected: &ExactSecurity) -> Result<(Vec<u8>, FileInfo)> {
    let before = info(file.as_raw_handle(), false)?;
    require_security(file.as_raw_handle(), expected)?;
    let length = (u64::from(before.size_high) << 32) | u64::from(before.size_low);
    if length == 0 || length > MAX_MANIFEST_BYTES as u64 {
        return Err("WAKE_BROKER_STAGE_MANIFEST_SIZE_INVALID".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_BROKER_STAGE_READ_FAILED")?;
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WAKE_BROKER_STAGE_READ_FAILED")?;
    if bytes.len() as u64 != length
        || !stable(&before, &info(file.as_raw_handle(), false)?)
        || security(file.as_raw_handle())? != *expected
    {
        return Err("WAKE_BROKER_STAGE_OBJECT_CHANGED".into());
    }
    Ok((bytes, before))
}

fn hash_artifact(
    file: &mut File,
    name: &'static str,
    expected_length: u64,
    expected_sha256: &str,
    expected_security: &ExactSecurity,
    root_volume: u32,
) -> Result<CandidateArtifactReadbackV1> {
    let before = info(file.as_raw_handle(), false)?;
    require_security(file.as_raw_handle(), expected_security)?;
    let length = (u64::from(before.size_high) << 32) | u64::from(before.size_low);
    if before.volume != root_volume || length != expected_length || length == 0 {
        return Err("WAKE_BROKER_STAGE_ARTIFACT_INVALID".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_BROKER_STAGE_READ_FAILED")?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut read = 0u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "WAKE_BROKER_STAGE_READ_FAILED")?;
        if count == 0 {
            break;
        }
        read = read
            .checked_add(count as u64)
            .ok_or("WAKE_BROKER_STAGE_ARTIFACT_INVALID")?;
        if read > expected_length {
            return Err("WAKE_BROKER_STAGE_ARTIFACT_INVALID".into());
        }
        digest.update(&buffer[..count]);
    }
    let actual_sha256 = format!("{:x}", digest.finalize());
    if read != expected_length || actual_sha256 != expected_sha256 {
        return Err("WAKE_BROKER_STAGE_HASH_MISMATCH".into());
    }
    if !stable(&before, &info(file.as_raw_handle(), false)?)
        || security(file.as_raw_handle())? != *expected_security
    {
        return Err("WAKE_BROKER_STAGE_OBJECT_CHANGED".into());
    }
    Ok(CandidateArtifactReadbackV1 {
        name,
        byte_length: read,
        sha256: actual_sha256,
        object: identity(&before),
    })
}

fn inspect_pinned(
    root: &OwnedHandle,
    expected_root: &ObjectIdentityV1,
    approval: &VerifiedApprovalV1,
    policy: &StagePolicy,
) -> Result<BrokerCandidateStageReadbackV1> {
    let payload = approval.payload();
    validate_manifest(&payload.manifest)?;
    let canonical_manifest =
        serde_json::to_vec(&payload.manifest).map_err(|_| "WAKE_BROKER_STAGE_MANIFEST_INVALID")?;
    let manifest_sha256 = format!("{:x}", Sha256::digest(&canonical_manifest));
    if payload.target_protected_root != *expected_root || payload.manifest_sha256 != manifest_sha256
    {
        return Err("WAKE_BROKER_STAGE_APPROVAL_BINDING_INVALID".into());
    }
    let root_before = info(root.as_raw_handle(), true)?;
    if identity(&root_before) != *expected_root {
        return Err("WAKE_BROKER_STAGE_ROOT_CHANGED".into());
    }
    require_security(root.as_raw_handle(), &policy.root)?;

    let staging = open_relative(root, STAGING_NAME, true)?;
    let staging_before = info(staging.as_raw_handle(), true)?;
    require_security(staging.as_raw_handle(), &policy.staging)?;
    if staging_before.volume != root_before.volume {
        return Err("WAKE_BROKER_STAGE_OBJECT_INVALID".into());
    }
    exact_inventory(&staging, &[SLOT_NAME])?;

    let candidate = open_relative(&staging, SLOT_NAME, true)?;
    let candidate_before = info(candidate.as_raw_handle(), true)?;
    require_security(candidate.as_raw_handle(), &policy.candidate)?;
    if candidate_before.volume != root_before.volume {
        return Err("WAKE_BROKER_STAGE_OBJECT_INVALID".into());
    }
    exact_inventory(
        &candidate,
        &[
            MANIFEST_NAME,
            ARTIFACT_NAMES[0],
            ARTIFACT_NAMES[1],
            ARTIFACT_NAMES[2],
            ARTIFACT_NAMES[3],
        ],
    )?;

    let manifest_handle = open_relative(&candidate, MANIFEST_NAME, false)?;
    let mut manifest = File::from(manifest_handle);
    let (manifest_bytes, manifest_before) = read_bounded(&mut manifest, &policy.manifest)?;
    if manifest_before.volume != root_before.volume
        || manifest_bytes != canonical_manifest
        || format!("{:x}", Sha256::digest(&manifest_bytes)) != manifest_sha256
    {
        return Err("WAKE_BROKER_STAGE_MANIFEST_MISMATCH".into());
    }

    // Pin every leaf before hashing any one of them. Each pin rejects
    // write/delete sharing for the duration of the complete inventory.
    let mut files = Vec::with_capacity(ARTIFACT_NAMES.len());
    for name in ARTIFACT_NAMES {
        files.push(File::from(open_relative(&candidate, name, false)?));
    }
    let mut artifacts = Vec::with_capacity(ARTIFACT_NAMES.len());
    for (index, file) in files.iter_mut().enumerate() {
        let signed = &payload.manifest.artifacts[index];
        artifacts.push(hash_artifact(
            file,
            ARTIFACT_NAMES[index],
            signed.byte_length,
            &signed.sha256,
            &policy.artifacts[index],
            root_before.volume,
        )?);
    }
    let artifacts: [CandidateArtifactReadbackV1; 4] = artifacts
        .try_into()
        .map_err(|_| "WAKE_BROKER_STAGE_INVENTORY_MISMATCH")?;

    let (manifest_after, _) = read_bounded(&mut manifest, &policy.manifest)?;
    let mut reopened_manifest = File::from(open_relative(&candidate, MANIFEST_NAME, false)?);
    let (reopened_bytes, reopened_info) = read_bounded(&mut reopened_manifest, &policy.manifest)?;
    let reopened_staging = open_relative(root, STAGING_NAME, true)?;
    let reopened_candidate = open_relative(&reopened_staging, SLOT_NAME, true)?;
    if manifest_after != manifest_bytes
        || reopened_bytes != manifest_bytes
        || identity(&reopened_info) != identity(&manifest_before)
        || identity(&info(reopened_staging.as_raw_handle(), true)?) != identity(&staging_before)
        || identity(&info(reopened_candidate.as_raw_handle(), true)?) != identity(&candidate_before)
        || !stable(&root_before, &info(root.as_raw_handle(), true)?)
        || !stable(&staging_before, &info(staging.as_raw_handle(), true)?)
        || !stable(&candidate_before, &info(candidate.as_raw_handle(), true)?)
        || security(root.as_raw_handle())? != policy.root
        || security(staging.as_raw_handle())? != policy.staging
        || security(candidate.as_raw_handle())? != policy.candidate
    {
        return Err("WAKE_BROKER_STAGE_OBJECT_CHANGED".into());
    }
    for (index, file) in files.iter().enumerate() {
        if identity(&info(file.as_raw_handle(), false)?) != artifacts[index].object
            || security(file.as_raw_handle())? != policy.artifacts[index]
        {
            return Err("WAKE_BROKER_STAGE_OBJECT_CHANGED".into());
        }
    }
    exact_inventory(&staging, &[SLOT_NAME])?;
    exact_inventory(
        &candidate,
        &[
            MANIFEST_NAME,
            ARTIFACT_NAMES[0],
            ARTIFACT_NAMES[1],
            ARTIFACT_NAMES[2],
            ARTIFACT_NAMES[3],
        ],
    )?;

    Ok(BrokerCandidateStageReadbackV1 {
        schema_version: 1,
        approval_id: payload.approval_id.clone(),
        install_generation: payload.install_generation,
        package_id: payload.manifest.package_id.clone(),
        signed_payload_sha256: approval.signed_payload_sha256().into(),
        manifest_sha256,
        protected_root: expected_root.clone(),
        staging_directory: identity(&staging_before),
        candidate_directory: identity(&candidate_before),
        manifest_file: identity(&manifest_before),
        artifacts,
        stage_creation_and_ledger_binding_proven: false,
        disposition: "MATCHED_SIGNED_STAGE_BYTES_READ_ONLY_NO_INSTALL_AUTHORITY",
    })
}

/// Inspect exactly `C:\ProgramData\CatDesk\WakeProtectedInstallV1\staging\candidate`.
/// Verify the envelope with the independently pinned protected policy key and
/// broker-owned observations, then inspect the fixed stage. This check grants
/// no mutation rights. The caller must assemble `observed` from broker-owned
/// pins and a trusted clock, never from the request; this function cannot
/// establish that provenance itself.
pub fn inspect_fixed_candidate_stage_read_only(
    policy: &BrokerProtectedPolicyV1,
    envelope: &[u8],
    observed: &BrokerObservedStateV1,
) -> Result<BrokerCandidateStageReadbackV1> {
    let policy_root = policy.readback()?.protected_root;
    let guard = acquire_fixed_protected_root_for_reader()?;
    let (policy_service_sid, _) = policy.checked_endpoint_sids()?;
    if policy_root != *guard.root_identity()
        || observed.target_protected_root != policy_root
        || policy_service_sid != guard.service_sid()
    {
        return Err("WAKE_BROKER_STAGE_APPROVAL_BINDING_INVALID".into());
    }
    let approval = policy.verify_approval(envelope, observed)?;
    let stage_policy = StagePolicy::production(guard.service_sid())?;
    let snapshot = inspect_pinned(
        guard.root(),
        guard.root_identity(),
        &approval,
        &stage_policy,
    )?;
    // The policy's original and fixed-name-reopened bytes/security must still
    // match after the stage read. The returned value remains advisory.
    policy.revalidate()?;
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_broker_approval::{self, ArtifactV1, verify_approval};
    use std::{
        os::windows::ffi::OsStrExt,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    const FILE_SHARE_WRITE: u32 = 2;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    static NEXT: AtomicU64 = AtomicU64::new(1);

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

    struct Fixture {
        root_path: PathBuf,
        root: OwnedHandle,
        root_id: ObjectIdentityV1,
        policy: StagePolicy,
        approval: VerifiedApprovalV1,
    }

    impl Fixture {
        fn new() -> Self {
            let root_path = std::env::temp_dir().join(format!(
                "catdesk-broker-stage-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let candidate_path = root_path.join(STAGING_NAME).join(SLOT_NAME);
            std::fs::create_dir_all(&candidate_path).expect("stage fixture directories");
            let blobs: [&[u8]; 4] = [
                b"host-binary",
                b"gui-binary",
                b"adapter-code",
                b"bridge-code",
            ];
            let mut bytes = root_path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let raw = unsafe {
                CreateFileW(
                    bytes.as_mut_ptr(),
                    FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    ptr::null_mut(),
                )
            };
            assert!(!raw.is_null() && raw != INVALID_HANDLE_VALUE);
            let root = unsafe { OwnedHandle::from_raw_handle(raw) };
            let root_id = identity(&info(root.as_raw_handle(), true).expect("root info"));

            let (mut payload, mut observed, key) = install_broker_approval::tests::fixture();
            payload.manifest.artifacts = std::array::from_fn(|index| ArtifactV1 {
                name: ARTIFACT_NAMES[index].into(),
                byte_length: blobs[index].len() as u64,
                sha256: format!("{:x}", Sha256::digest(blobs[index])),
            });
            payload.manifest.package_id = format!(
                "{}-{}-{}",
                payload.manifest.version,
                &payload.manifest.artifacts[0].sha256[..12],
                &payload.manifest.artifacts[1].sha256[..12]
            );
            let manifest_bytes = serde_json::to_vec(&payload.manifest).expect("manifest JSON");
            payload.manifest_sha256 = format!("{:x}", Sha256::digest(&manifest_bytes));
            payload.target_protected_root = root_id.clone();
            observed.target_protected_root = root_id.clone();
            for (name, blob) in ARTIFACT_NAMES.into_iter().zip(blobs) {
                std::fs::write(candidate_path.join(name), blob).expect("stage artifact");
            }
            std::fs::write(candidate_path.join(MANIFEST_NAME), manifest_bytes)
                .expect("stage manifest");
            let envelope = install_broker_approval::tests::envelope(&payload, &key);
            let approval = verify_approval(&envelope, &key.verifying_key(), &observed)
                .expect("signed fixture approval");

            let staging = open_relative(&root, STAGING_NAME, true).expect("staging pin");
            let candidate = open_relative(&staging, SLOT_NAME, true).expect("candidate pin");
            let manifest = open_relative(&candidate, MANIFEST_NAME, false).expect("manifest pin");
            let artifacts = std::array::from_fn(|index| {
                let artifact =
                    open_relative(&candidate, ARTIFACT_NAMES[index], false).expect("artifact pin");
                security(artifact.as_raw_handle()).expect("artifact security")
            });
            let policy = StagePolicy {
                root: security(root.as_raw_handle()).expect("root security"),
                staging: security(staging.as_raw_handle()).expect("staging security"),
                candidate: security(candidate.as_raw_handle()).expect("candidate security"),
                manifest: security(manifest.as_raw_handle()).expect("manifest security"),
                artifacts,
            };
            Self {
                root_path,
                root,
                root_id,
                policy,
                approval,
            }
        }

        fn candidate_path(&self) -> PathBuf {
            self.root_path.join(STAGING_NAME).join(SLOT_NAME)
        }

        fn inspect(&self) -> Result<BrokerCandidateStageReadbackV1> {
            inspect_pinned(&self.root, &self.root_id, &self.approval, &self.policy)
        }

        fn cleanup(self) {
            let candidate_path = self.candidate_path();
            drop(self.root);
            for name in [
                MANIFEST_NAME,
                ARTIFACT_NAMES[0],
                ARTIFACT_NAMES[1],
                ARTIFACT_NAMES[2],
                ARTIFACT_NAMES[3],
            ] {
                std::fs::remove_file(candidate_path.join(name)).expect("remove fixture leaf");
            }
            std::fs::remove_dir(&candidate_path).expect("remove fixture candidate");
            std::fs::remove_dir(self.root_path.join(STAGING_NAME)).expect("remove fixture staging");
            std::fs::remove_dir(self.root_path).expect("remove fixture root");
        }
    }

    #[test]
    fn signed_four_bytes_match_but_stale_stage_origin_is_not_proven() {
        let fixture = Fixture::new();
        let snapshot = fixture.inspect().expect("exact stage bytes");
        assert_eq!(snapshot.approval_id, fixture.approval.payload().approval_id);
        assert_eq!(
            snapshot.package_id,
            fixture.approval.payload().manifest.package_id
        );
        assert_eq!(snapshot.artifacts.len(), 4);
        assert_eq!(snapshot.artifacts[3].name, "wake_bridge.py");
        assert!(!snapshot.stage_creation_and_ledger_binding_proven);
        assert_eq!(
            snapshot.disposition,
            "MATCHED_SIGNED_STAGE_BYTES_READ_ONLY_NO_INSTALL_AUTHORITY"
        );
        let json = serde_json::to_value(snapshot).expect("snapshot JSON");
        assert!(json.get("approvedForInstall").is_none());
        fixture.cleanup();
    }

    #[test]
    fn changed_same_length_bytes_and_wrong_length_fail_closed() {
        let fixture = Fixture::new();
        let artifact = fixture.candidate_path().join("adapter.py");
        std::fs::write(&artifact, b"adapter-xode").expect("change same-length bytes");
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_BROKER_STAGE_HASH_MISMATCH"
        );
        std::fs::write(&artifact, b"adapter-code-extra").expect("change length");
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_BROKER_STAGE_ARTIFACT_INVALID"
        );
        std::fs::write(&artifact, b"adapter-code").expect("restore artifact");
        fixture.cleanup();
    }

    #[test]
    fn extra_stage_or_candidate_entry_fails_exact_inventory() {
        let fixture = Fixture::new();
        let extra = fixture.candidate_path().join("extra.exe");
        std::fs::write(&extra, b"extra").expect("extra candidate file");
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_BROKER_STAGE_INVENTORY_MISMATCH"
        );
        std::fs::remove_file(extra).expect("remove extra candidate file");
        let extra = fixture.root_path.join(STAGING_NAME).join("other");
        std::fs::create_dir(&extra).expect("extra stage directory");
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_BROKER_STAGE_INVENTORY_MISMATCH"
        );
        std::fs::remove_dir(extra).expect("remove extra stage directory");
        fixture.cleanup();
    }

    #[test]
    fn candidate_manifest_and_signed_root_cannot_override_approval() {
        let fixture = Fixture::new();
        let manifest = fixture.candidate_path().join(MANIFEST_NAME);
        let pretty = serde_json::to_vec_pretty(&fixture.approval.payload().manifest)
            .expect("pretty manifest");
        std::fs::write(&manifest, pretty).expect("change manifest encoding");
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_BROKER_STAGE_MANIFEST_MISMATCH"
        );
        std::fs::write(
            &manifest,
            serde_json::to_vec(&fixture.approval.payload().manifest).expect("canonical manifest"),
        )
        .expect("restore manifest");
        let mut wrong_root = fixture.root_id.clone();
        wrong_root.file_index_low = wrong_root.file_index_low.wrapping_add(1);
        assert_eq!(
            inspect_pinned(
                &fixture.root,
                &wrong_root,
                &fixture.approval,
                &fixture.policy
            )
            .unwrap_err(),
            "WAKE_BROKER_STAGE_APPROVAL_BINDING_INVALID"
        );
        fixture.cleanup();
    }

    #[test]
    fn wrong_acl_and_hardlink_fail_closed() {
        let fixture = Fixture::new();
        let mut wrong_policy = fixture.policy.clone();
        wrong_policy.artifacts[0].sddl.push('X');
        assert_eq!(
            inspect_pinned(
                &fixture.root,
                &fixture.root_id,
                &fixture.approval,
                &wrong_policy
            )
            .unwrap_err(),
            "WAKE_BROKER_STAGE_ACL_MISMATCH"
        );
        let hardlink = fixture.root_path.join("outside-link.exe");
        std::fs::hard_link(fixture.candidate_path().join(ARTIFACT_NAMES[0]), &hardlink)
            .expect("fixture hardlink");
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_BROKER_STAGE_OBJECT_INVALID"
        );
        std::fs::remove_file(hardlink).expect("remove fixture hardlink");
        fixture.cleanup();
    }

    #[test]
    fn pinned_artifact_denies_concurrent_write_and_rename() {
        let fixture = Fixture::new();
        let staging = open_relative(&fixture.root, STAGING_NAME, true).expect("staging pin");
        let candidate = open_relative(&staging, SLOT_NAME, true).expect("candidate pin");
        let artifact = open_relative(&candidate, ARTIFACT_NAMES[0], false).expect("artifact pin");
        let artifact_path = fixture.candidate_path().join(ARTIFACT_NAMES[0]);
        assert!(std::fs::write(&artifact_path, b"changed").is_err());
        assert!(
            std::fs::rename(&artifact_path, fixture.candidate_path().join("renamed.exe")).is_err()
        );
        drop(artifact);
        drop(candidate);
        drop(staging);
        fixture.inspect().expect("stage remains intact");
        fixture.cleanup();
    }

    #[test]
    fn reparse_metadata_and_case_alias_are_rejected() {
        let fixture = Fixture::new();
        let info = FileInfo {
            attributes: FILE_ATTRIBUTE_REPARSE_POINT,
            volume: 1,
            links: 1,
            index_low: 1,
            ..FileInfo::default()
        };
        assert_eq!(
            validate_info(info, false).unwrap_err(),
            "WAKE_BROKER_STAGE_OBJECT_INVALID"
        );
        let candidate = fixture.candidate_path();
        std::fs::rename(candidate.join("adapter.py"), candidate.join("adapter.tmp"))
            .expect("intermediate rename");
        std::fs::rename(candidate.join("adapter.tmp"), candidate.join("Adapter.py"))
            .expect("case alias");
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_BROKER_STAGE_INVENTORY_MISMATCH"
        );
        std::fs::rename(candidate.join("Adapter.py"), candidate.join("adapter.py"))
            .expect("restore name");
        fixture.cleanup();
    }
}
