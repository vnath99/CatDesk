//! Read-only selection of the fixed protected Wake package.
//!
//! The public entry point has no path, package ID, or policy argument. It
//! requires the separately installed broker's read-only preflight, then pins
//! the compiled ProgramData root and resolves a canonical protected pointer.
//! Every package byte is read through a no-follow, no-write-share handle and
//! checked against the exact four-file canonical manifest. This returns a
//! snapshot only: it does not grant installation or launch authority. A future
//! broker must additionally verify the completed ledger transaction, prove
//! ancestor effective access/integrity and worker denials, hold its pins into
//! launch, and verify the new process image and owner. The legacy runtime
//! launch readers have not been switched to this module.

use crate::{
    Result,
    install_broker_approval::{
        ObjectIdentityV1, PackageManifestV1, VerifiedApprovalV1, validate_manifest,
    },
    install_broker_ledger::{StageBindingV1, valid_stage_binding},
    install_broker_preflight::{
        acquire_fixed_protected_root_for_reader, canonical_protected_security,
        protected_security_raw,
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::c_void,
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};

const MAX_POINTER_BYTES: usize = 4096;
const MAX_MANIFEST_BYTES: usize = 4096;
const MAX_DIRECTORY_ENTRIES: usize = 16;
const MAX_PACKAGE_CONTAINER_ENTRIES: usize = 256;
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
const ARTIFACT_NAMES: [&str; 4] = [
    "CatDeskWakeHost.exe",
    "CatDeskBinagotchy.exe",
    "adapter.py",
    "wake_bridge.py",
];

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProtectedPointerV1 {
    schema_version: u32,
    purpose: String,
    install_generation: u64,
    approval_id: String,
    predecessor_package_id: String,
    predecessor_pointer: ObjectIdentityV1,
    protected_root: ObjectIdentityV1,
    package_id: String,
    package_directory: ObjectIdentityV1,
    manifest_file: ObjectIdentityV1,
    manifest_sha256: String,
}

/// Canonical proposed pointer bytes and their exact SHA-256. This is a claim
/// assembled from a verified approval and supplied stage IDs, not proof that
/// the package was published under `packages` or that a pointer was switched.
/// The verifier's key is not bound to protected broker policy here. It carries
/// no file handle, ledger lease, or install authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtectedPointerPlanV1 {
    canonical_bytes: Vec<u8>,
    sha256: String,
}

impl ProtectedPointerPlanV1 {
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedArtifactReadbackV1 {
    pub name: &'static str,
    pub byte_length: u64,
    pub sha256: String,
    pub object: ObjectIdentityV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedInstallReadOnlySnapshotV1 {
    pub schema_version: u32,
    pub install_generation: u64,
    pub approval_id: String,
    pub predecessor_package_id: String,
    pub predecessor_pointer: ObjectIdentityV1,
    pub package_id: String,
    pub version: String,
    pub manifest_sha256: String,
    pub protected_root: ObjectIdentityV1,
    pub pointer: ObjectIdentityV1,
    pub package_directory: ObjectIdentityV1,
    pub manifest_file: ObjectIdentityV1,
    pub artifacts: [ProtectedArtifactReadbackV1; 4],
    pub disposition: &'static str,
}

/// Point-in-time physical readback of a signed package at its final name.
/// It is compared with durable STAGED IDs by the ledger reader, and carries
/// no retained writer handle, publication authority, or pointer authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProtectedPublishedPackageReadbackV1 {
    pub schema_version: u32,
    pub approval_id: String,
    pub install_generation: u64,
    pub package_id: String,
    pub signed_payload_sha256: String,
    pub manifest_sha256: String,
    pub protected_root: ObjectIdentityV1,
    pub staging_directory: ObjectIdentityV1,
    /// This physical parent is durable STAGED evidence only when the stage
    /// binding also records it; older stages remain diagnostic-only.
    pub packages_directory: ObjectIdentityV1,
    pub package_directory: ObjectIdentityV1,
    pub manifest_file: ObjectIdentityV1,
    pub artifacts: [ProtectedArtifactReadbackV1; 4],
    pub disposition: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PublicationLocationV1 {
    Stage,
    Moved(Box<ProtectedPublishedPackageReadbackV1>),
}

/// Retains the exact moved package and both namespace parents across a
/// caller's ledger/history recheck. Every handle denies write and delete
/// sharing, so this read-only proof must be dropped before any future rename
/// or other publication mutation. It grants no PUBLISHED record authority.
pub(crate) struct ProtectedPublishedPackagePinV1 {
    approval: VerifiedApprovalV1,
    stage: StageBindingV1,
    snapshot: ProtectedPublishedPackageReadbackV1,
    policy: PublicationPolicy,
    staging: OwnedHandle,
    packages: OwnedHandle,
    packages_id: ObjectIdentityV1,
    package: OwnedHandle,
    manifest: OwnedHandle,
    artifacts: [OwnedHandle; 4],
}

impl ProtectedPublishedPackagePinV1 {
    pub(crate) fn snapshot(&self) -> &ProtectedPublishedPackageReadbackV1 {
        &self.snapshot
    }

    pub(crate) fn revalidate(
        &self,
        root: &OwnedHandle,
        expected_root: &ObjectIdentityV1,
    ) -> Result<()> {
        if self.snapshot.protected_root != *expected_root
            || identity(&file_info(self.staging.as_raw_handle(), true)?)
                != self.stage.staging_directory
            || identity(&file_info(self.packages.as_raw_handle(), true)?) != self.packages_id
            || identity(&file_info(self.package.as_raw_handle(), true)?)
                != self.stage.candidate_directory
            || identity(&file_info(self.manifest.as_raw_handle(), false)?)
                != self.stage.manifest_file
            || self.artifacts.iter().enumerate().any(|(index, handle)| {
                file_info(handle.as_raw_handle(), false)
                    .map(|info| identity(&info))
                    .ok()
                    != Some(self.stage.artifacts[index].clone())
            })
        {
            return Err("WAKE_PROTECTED_PUBLICATION_OBJECT_CHANGED".into());
        }
        match inspect_publication_location_pinned(
            root,
            expected_root,
            &self.approval,
            &self.stage,
            &self.policy,
        )? {
            PublicationLocationV1::Moved(snapshot) if *snapshot == self.snapshot => Ok(()),
            _ => Err("WAKE_PROTECTED_PUBLICATION_OBJECT_CHANGED".into()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExactSecurity {
    owner: String,
    sddl: String,
}

#[derive(Clone)]
struct ReaderPolicy {
    root: ExactSecurity,
    packages: ExactSecurity,
    package: ExactSecurity,
    pointer: ExactSecurity,
    manifest: ExactSecurity,
    artifacts: [ExactSecurity; 4],
}

#[derive(Clone)]
struct PublicationPolicy {
    root: ExactSecurity,
    staging: ExactSecurity,
    packages: ExactSecurity,
    package: ExactSecurity,
    manifest: ExactSecurity,
    artifacts: [ExactSecurity; 4],
}

/// ACL expectations captured from an isolated test stage before moving it.
/// Production always derives expectations from the protected service SID.
#[cfg(test)]
pub(crate) struct PublicationFixturePolicyV1(PublicationPolicy);

/// Retains the exact protected objects behind a read-only snapshot while a
/// caller holds its separate ledger lock. This value has no launch, write, or
/// install operation. Reopening the fixed names must still match these pins.
pub(crate) struct ProtectedInstallPinV1 {
    snapshot: ProtectedInstallReadOnlySnapshotV1,
    pointer_sha256: String,
    policy: ReaderPolicy,
    pointer: OwnedHandle,
    packages: OwnedHandle,
    packages_id: ObjectIdentityV1,
    package: OwnedHandle,
    manifest: OwnedHandle,
    artifacts: [OwnedHandle; 4],
}

impl ProtectedInstallPinV1 {
    pub(crate) fn snapshot(&self) -> &ProtectedInstallReadOnlySnapshotV1 {
        &self.snapshot
    }

    pub(crate) fn pointer_sha256(&self) -> &str {
        &self.pointer_sha256
    }

    pub(crate) fn revalidate(
        &self,
        root: &OwnedHandle,
        expected_root: &ObjectIdentityV1,
    ) -> Result<()> {
        if self.snapshot.protected_root != *expected_root
            || identity(&file_info(self.pointer.as_raw_handle(), false)?) != self.snapshot.pointer
            || identity(&file_info(self.packages.as_raw_handle(), true)?) != self.packages_id
            || identity(&file_info(self.package.as_raw_handle(), true)?)
                != self.snapshot.package_directory
            || identity(&file_info(self.manifest.as_raw_handle(), false)?)
                != self.snapshot.manifest_file
            || self.artifacts.iter().enumerate().any(|(index, handle)| {
                file_info(handle.as_raw_handle(), false)
                    .map(|info| identity(&info))
                    .ok()
                    != Some(self.snapshot.artifacts[index].object.clone())
            })
        {
            return Err("WAKE_PROTECTED_READER_OBJECT_CHANGED".into());
        }
        // The old handles deny write/delete sharing. Independently reopen and
        // rehash the same fixed names while those handles remain live.
        let (snapshot, pointer_sha256) = inspect_pinned(root, expected_root, &self.policy)?;
        if snapshot != self.snapshot || pointer_sha256 != self.pointer_sha256 {
            return Err("WAKE_PROTECTED_READER_OBJECT_CHANGED".into());
        }
        Ok(())
    }
}

impl ReaderPolicy {
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
            packages: directory.clone(),
            package: directory,
            pointer: file.clone(),
            manifest: file.clone(),
            artifacts: std::array::from_fn(|_| file.clone()),
        })
    }
}

impl PublicationPolicy {
    fn production(service_sid: &str) -> Result<Self> {
        let reader = ReaderPolicy::production(service_sid)?;
        Ok(Self {
            root: reader.root.clone(),
            staging: reader.root,
            packages: reader.packages,
            package: reader.package,
            manifest: reader.manifest,
            artifacts: reader.artifacts,
        })
    }
}

fn valid_object(value: &ObjectIdentityV1) -> bool {
    value.volume_serial != 0 && (value.file_index_high != 0 || value.file_index_low != 0)
}

fn lowercase_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !matches!(value, "." | "..")
        && !value.ends_with(['.', ' '])
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn file_info(raw: *mut c_void, directory: bool) -> Result<FileInfo> {
    let mut info = FileInfo::default();
    if unsafe { GetFileInformationByHandle(raw, &mut info) } == 0 {
        return Err("WAKE_PROTECTED_READER_OBJECT_INVALID".into());
    }
    validate_file_info(info, directory)
}

fn validate_file_info(info: FileInfo, directory: bool) -> Result<FileInfo> {
    if info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (info.attributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || (!directory && info.links != 1)
        || info.volume == 0
        || (info.index_high == 0 && info.index_low == 0)
    {
        return Err("WAKE_PROTECTED_READER_OBJECT_INVALID".into());
    }
    Ok(info)
}

fn identity(info: &FileInfo) -> ObjectIdentityV1 {
    ObjectIdentityV1 {
        volume_serial: info.volume,
        file_index_high: info.index_high,
        file_index_low: info.index_low,
    }
}

fn stable_file_info(before: &FileInfo, after: &FileInfo) -> bool {
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

fn exact_security(raw: *mut c_void) -> Result<ExactSecurity> {
    let (owner, sddl) = protected_security_raw(raw)?;
    Ok(ExactSecurity { owner, sddl })
}

fn assert_security(raw: *mut c_void, expected: &ExactSecurity) -> Result<()> {
    if exact_security(raw)? != *expected {
        return Err("WAKE_PROTECTED_READER_ACL_MISMATCH".into());
    }
    Ok(())
}

fn open_relative(parent: &OwnedHandle, component: &str, directory: bool) -> Result<OwnedHandle> {
    if !safe_component(component) {
        return Err("WAKE_PROTECTED_READER_COMPONENT_INVALID".into());
    }
    let mut units: Vec<u16> = component.encode_utf16().collect();
    let mut name = UnicodeString {
        length: (units.len() * 2) as u16,
        maximum_length: (units.len() * 2) as u16,
        buffer: units.as_mut_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: parent.as_raw_handle(),
        object_name: &mut name,
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
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            &mut attributes,
            &mut status,
            ptr::null_mut(),
            0,
            FILE_SHARE_READ, // deny write and delete while this pin is alive
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
    if result < 0 || raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err("WAKE_PROTECTED_READER_OBJECT_UNAVAILABLE".into());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    file_info(handle.as_raw_handle(), directory)?;
    Ok(handle)
}

fn reopen_directory_for_inventory(directory: &OwnedHandle) -> Result<OwnedHandle> {
    // A fresh FILE_OBJECT prevents a prior NtQueryDirectoryFile search filter
    // on the caller's directory handle from hiding an extra package entry.
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
            FILE_SHARE_READ,
            FILE_OPEN,
            FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
            ptr::null_mut(),
            0,
        )
    };
    if result < 0 || raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err("WAKE_PROTECTED_READER_INVENTORY_UNAVAILABLE".into());
    }
    let reopened = unsafe { OwnedHandle::from_raw_handle(raw) };
    if identity(&file_info(reopened.as_raw_handle(), true)?)
        != identity(&file_info(directory.as_raw_handle(), true)?)
    {
        return Err("WAKE_PROTECTED_READER_OBJECT_CHANGED".into());
    }
    Ok(reopened)
}

fn directory_inventory(directory: &OwnedHandle, limit: usize) -> Result<BTreeSet<String>> {
    let fresh = reopen_directory_for_inventory(directory)?;
    let mut seen = BTreeSet::new();
    for index in 0..=limit {
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
                ptr::null_mut(),
                u8::from(index == 0),
            )
        } as u32;
        if result == STATUS_NO_MORE_FILES || (index == 0 && result == STATUS_NO_SUCH_FILE) {
            return Ok(seen);
        }
        if result != 0
            || index == limit
            || status.information < 12
            || status.information > std::mem::size_of_val(&aligned)
        {
            return Err("WAKE_PROTECTED_READER_INVENTORY_UNAVAILABLE".into());
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(aligned.as_ptr().cast::<u8>(), status.information)
        };
        let next = u32::from_ne_bytes(bytes[..4].try_into().unwrap());
        let name_length = u32::from_ne_bytes(bytes[8..12].try_into().unwrap()) as usize;
        if next != 0
            || name_length == 0
            || !name_length.is_multiple_of(2)
            || name_length > bytes.len() - 12
        {
            return Err("WAKE_PROTECTED_READER_INVENTORY_UNAVAILABLE".into());
        }
        let units: Vec<u16> = bytes[12..12 + name_length]
            .chunks_exact(2)
            .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
            .collect();
        let name =
            String::from_utf16(&units).map_err(|_| "WAKE_PROTECTED_READER_INVENTORY_MISMATCH")?;
        if matches!(name.as_str(), "." | "..") {
            continue;
        }
        if !seen.insert(name) {
            return Err("WAKE_PROTECTED_READER_INVENTORY_MISMATCH".into());
        }
    }
    Err("WAKE_PROTECTED_READER_INVENTORY_UNAVAILABLE".into())
}

fn exact_package_inventory(package: &OwnedHandle) -> Result<()> {
    let expected: BTreeSet<String> = [
        "manifest.json",
        ARTIFACT_NAMES[0],
        ARTIFACT_NAMES[1],
        ARTIFACT_NAMES[2],
        ARTIFACT_NAMES[3],
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    if directory_inventory(package, MAX_DIRECTORY_ENTRIES)? != expected {
        return Err("WAKE_PROTECTED_READER_INVENTORY_MISMATCH".into());
    }
    Ok(())
}

fn read_bounded(
    file: &mut File,
    max: usize,
    expected_security: &ExactSecurity,
) -> Result<(Vec<u8>, FileInfo)> {
    let before = file_info(file.as_raw_handle(), false)?;
    assert_security(file.as_raw_handle(), expected_security)?;
    let length = (u64::from(before.size_high) << 32) | u64::from(before.size_low);
    if length == 0 || length > max as u64 {
        return Err("WAKE_PROTECTED_READER_SIZE_INVALID".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_PROTECTED_READER_READ_FAILED")?;
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WAKE_PROTECTED_READER_READ_FAILED")?;
    if bytes.len() as u64 != length {
        return Err("WAKE_PROTECTED_READER_OBJECT_CHANGED".into());
    }
    let after = file_info(file.as_raw_handle(), false)?;
    if !stable_file_info(&before, &after)
        || exact_security(file.as_raw_handle())? != *expected_security
    {
        return Err("WAKE_PROTECTED_READER_OBJECT_CHANGED".into());
    }
    Ok((bytes, before))
}

fn canonical_json<T: for<'de> Deserialize<'de> + Serialize>(bytes: &[u8]) -> Result<T> {
    let value: T =
        serde_json::from_slice(bytes).map_err(|_| "WAKE_PROTECTED_READER_ENCODING_INVALID")?;
    if serde_json::to_vec(&value).map_err(|_| "WAKE_PROTECTED_READER_ENCODING_INVALID")? != bytes {
        return Err("WAKE_PROTECTED_READER_ENCODING_INVALID".into());
    }
    Ok(value)
}

fn hash_artifact(
    file: &mut File,
    name: &'static str,
    expected_length: u64,
    expected_sha: &str,
    expected_security: &ExactSecurity,
    root_volume: u32,
) -> Result<ProtectedArtifactReadbackV1> {
    let before = file_info(file.as_raw_handle(), false)?;
    assert_security(file.as_raw_handle(), expected_security)?;
    let length = (u64::from(before.size_high) << 32) | u64::from(before.size_low);
    if before.volume != root_volume || expected_length == 0 || length != expected_length {
        return Err("WAKE_PROTECTED_READER_ARTIFACT_INVALID".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_PROTECTED_READER_READ_FAILED")?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut read = 0u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "WAKE_PROTECTED_READER_READ_FAILED")?;
        if count == 0 {
            break;
        }
        read = read
            .checked_add(count as u64)
            .ok_or("WAKE_PROTECTED_READER_ARTIFACT_INVALID")?;
        if read > expected_length {
            return Err("WAKE_PROTECTED_READER_ARTIFACT_INVALID".into());
        }
        hasher.update(&buffer[..count]);
    }
    let actual_sha = format!("{:x}", hasher.finalize());
    if read != expected_length || actual_sha != expected_sha {
        return Err("WAKE_PROTECTED_READER_HASH_MISMATCH".into());
    }
    let after = file_info(file.as_raw_handle(), false)?;
    if !stable_file_info(&before, &after)
        || exact_security(file.as_raw_handle())? != *expected_security
    {
        return Err("WAKE_PROTECTED_READER_OBJECT_CHANGED".into());
    }
    Ok(ProtectedArtifactReadbackV1 {
        name,
        byte_length: read,
        sha256: actual_sha,
        object: identity(&before),
    })
}

fn validate_pointer(pointer: &ProtectedPointerV1, root: &ObjectIdentityV1) -> Result<()> {
    if pointer.schema_version != 1
        || pointer.purpose != "catdesk-wake-protected-current"
        || pointer.install_generation == 0
        || !lowercase_hex(&pointer.approval_id, 32)
        || !lowercase_hex(&pointer.manifest_sha256, 64)
        || !safe_component(&pointer.package_id)
        || !safe_component(&pointer.predecessor_package_id)
        || !valid_object(&pointer.predecessor_pointer)
        || !valid_object(&pointer.package_directory)
        || !valid_object(&pointer.manifest_file)
        || pointer.protected_root != *root
        || pointer.package_directory.volume_serial != root.volume_serial
        || pointer.manifest_file.volume_serial != root.volume_serial
    {
        return Err("WAKE_PROTECTED_READER_POINTER_INVALID".into());
    }
    Ok(())
}

/// Plan the exact bytes a future broker-owned writer would need after it has
/// independently proven publication. `stage` is only a supplied identity
/// claim here: this function reads and writes no protected object and never
/// proves that the stage reached the final package namespace. The caller must
/// separately bind the approval key to protected broker policy.
pub fn plan_protected_pointer_from_stage(
    approval: &VerifiedApprovalV1,
    stage: &StageBindingV1,
) -> Result<ProtectedPointerPlanV1> {
    let payload = approval.payload();
    validate_manifest(&payload.manifest)?;
    let root = &payload.target_protected_root;
    let objects = [
        &stage.staging_directory,
        &stage.candidate_directory,
        &stage.manifest_file,
        &stage.artifacts[0],
        &stage.artifacts[1],
        &stage.artifacts[2],
        &stage.artifacts[3],
    ];
    if !valid_stage_binding(stage, root)
        || objects
            .iter()
            .any(|object| *object == root || *object == &payload.predecessor_pointer)
    {
        return Err("WAKE_PROTECTED_POINTER_PLAN_STAGE_INVALID".into());
    }
    let pointer = ProtectedPointerV1 {
        schema_version: 1,
        purpose: "catdesk-wake-protected-current".into(),
        install_generation: payload.install_generation,
        approval_id: payload.approval_id.clone(),
        predecessor_package_id: payload.predecessor_package_id.clone(),
        predecessor_pointer: payload.predecessor_pointer.clone(),
        protected_root: root.clone(),
        package_id: payload.manifest.package_id.clone(),
        package_directory: stage.candidate_directory.clone(),
        manifest_file: stage.manifest_file.clone(),
        manifest_sha256: payload.manifest_sha256.clone(),
    };
    validate_pointer(&pointer, root).map_err(|_| "WAKE_PROTECTED_POINTER_PLAN_INVALID")?;
    let canonical_bytes =
        serde_json::to_vec(&pointer).map_err(|_| "WAKE_PROTECTED_POINTER_PLAN_ENCODING_INVALID")?;
    if canonical_bytes.is_empty() || canonical_bytes.len() > MAX_POINTER_BYTES {
        return Err("WAKE_PROTECTED_POINTER_PLAN_ENCODING_INVALID".into());
    }
    let parsed: ProtectedPointerV1 = canonical_json(&canonical_bytes)
        .map_err(|_| "WAKE_PROTECTED_POINTER_PLAN_ENCODING_INVALID")?;
    if parsed != pointer {
        return Err("WAKE_PROTECTED_POINTER_PLAN_ENCODING_INVALID".into());
    }
    let sha256 = format!("{:x}", Sha256::digest(&canonical_bytes));
    Ok(ProtectedPointerPlanV1 {
        canonical_bytes,
        sha256,
    })
}

fn inspect_publication_package(
    parent: &OwnedHandle,
    package_name: &str,
    root_volume: u32,
    approval: &VerifiedApprovalV1,
    stage: &StageBindingV1,
    policy: &PublicationPolicy,
    canonical_manifest: &[u8],
) -> Result<[ProtectedArtifactReadbackV1; 4]> {
    let package = open_relative(parent, package_name, true)?;
    let package_before = file_info(package.as_raw_handle(), true)?;
    if identity(&package_before) != stage.candidate_directory
        || package_before.volume != root_volume
    {
        return Err("WAKE_PROTECTED_PUBLICATION_BINDING_INVALID".into());
    }
    assert_security(package.as_raw_handle(), &policy.package)?;
    exact_package_inventory(&package)?;

    let manifest_handle = open_relative(&package, "manifest.json", false)?;
    let mut manifest = File::from(manifest_handle);
    let (manifest_bytes, manifest_before) =
        read_bounded(&mut manifest, MAX_MANIFEST_BYTES, &policy.manifest)?;
    if identity(&manifest_before) != stage.manifest_file
        || manifest_before.volume != root_volume
        || manifest_bytes != canonical_manifest
        || format!("{:x}", Sha256::digest(&manifest_bytes)) != approval.payload().manifest_sha256
    {
        return Err("WAKE_PROTECTED_PUBLICATION_MANIFEST_MISMATCH".into());
    }
    let parsed: PackageManifestV1 = canonical_json(&manifest_bytes)?;
    if parsed != approval.payload().manifest {
        return Err("WAKE_PROTECTED_PUBLICATION_MANIFEST_MISMATCH".into());
    }

    // Pin all leaves before hashing any leaf. The pins deny write/delete
    // sharing while the first and second complete readbacks run.
    let mut files = ARTIFACT_NAMES
        .into_iter()
        .map(|name| open_relative(&package, name, false).map(File::from))
        .collect::<Result<Vec<_>>>()?;
    let mut artifacts = Vec::with_capacity(ARTIFACT_NAMES.len());
    for (index, file) in files.iter_mut().enumerate() {
        let signed = &approval.payload().manifest.artifacts[index];
        let readback = hash_artifact(
            file,
            ARTIFACT_NAMES[index],
            signed.byte_length,
            &signed.sha256,
            &policy.artifacts[index],
            root_volume,
        )?;
        if readback.object != stage.artifacts[index] {
            return Err("WAKE_PROTECTED_PUBLICATION_BINDING_INVALID".into());
        }
        artifacts.push(readback);
    }
    let artifacts: [ProtectedArtifactReadbackV1; 4] = artifacts
        .try_into()
        .map_err(|_| "WAKE_PROTECTED_READER_INVENTORY_MISMATCH")?;

    let (manifest_after, _) = read_bounded(&mut manifest, MAX_MANIFEST_BYTES, &policy.manifest)?;
    let reopened_package = open_relative(parent, package_name, true)?;
    let mut reopened_manifest =
        File::from(open_relative(&reopened_package, "manifest.json", false)?);
    let (reopened_bytes, reopened_manifest_info) =
        read_bounded(&mut reopened_manifest, MAX_MANIFEST_BYTES, &policy.manifest)?;
    let mut reopened_files = ARTIFACT_NAMES
        .into_iter()
        .map(|name| open_relative(&reopened_package, name, false).map(File::from))
        .collect::<Result<Vec<_>>>()?;
    for (index, file) in reopened_files.iter_mut().enumerate() {
        let signed = &approval.payload().manifest.artifacts[index];
        if hash_artifact(
            file,
            ARTIFACT_NAMES[index],
            signed.byte_length,
            &signed.sha256,
            &policy.artifacts[index],
            root_volume,
        )? != artifacts[index]
        {
            return Err("WAKE_PROTECTED_PUBLICATION_OBJECT_CHANGED".into());
        }
    }
    if manifest_after != manifest_bytes
        || reopened_bytes != manifest_bytes
        || identity(&reopened_manifest_info) != stage.manifest_file
        || identity(&file_info(reopened_package.as_raw_handle(), true)?)
            != stage.candidate_directory
        || !stable_file_info(&package_before, &file_info(package.as_raw_handle(), true)?)
        || exact_security(package.as_raw_handle())? != policy.package
        || exact_security(reopened_package.as_raw_handle())? != policy.package
    {
        return Err("WAKE_PROTECTED_PUBLICATION_OBJECT_CHANGED".into());
    }
    for (index, file) in files.iter().enumerate() {
        if identity(&file_info(file.as_raw_handle(), false)?) != stage.artifacts[index]
            || exact_security(file.as_raw_handle())? != policy.artifacts[index]
        {
            return Err("WAKE_PROTECTED_PUBLICATION_OBJECT_CHANGED".into());
        }
    }
    exact_package_inventory(&package)?;
    exact_package_inventory(&reopened_package)?;
    Ok(artifacts)
}

fn inspect_publication_location_pinned(
    root: &OwnedHandle,
    root_id: &ObjectIdentityV1,
    approval: &VerifiedApprovalV1,
    stage: &StageBindingV1,
    policy: &PublicationPolicy,
) -> Result<PublicationLocationV1> {
    // The approval and stage IDs are supplied claims. The caller must bind
    // the approval key to protected policy and the stage to durable STAGED.
    plan_protected_pointer_from_stage(approval, stage)?;
    if approval.payload().target_protected_root != *root_id {
        return Err("WAKE_PROTECTED_PUBLICATION_APPROVAL_MISMATCH".into());
    }
    let canonical_manifest = serde_json::to_vec(&approval.payload().manifest)
        .map_err(|_| "WAKE_PROTECTED_PUBLICATION_MANIFEST_MISMATCH")?;
    if format!("{:x}", Sha256::digest(&canonical_manifest)) != approval.payload().manifest_sha256 {
        return Err("WAKE_PROTECTED_PUBLICATION_MANIFEST_MISMATCH".into());
    }
    let root_before = file_info(root.as_raw_handle(), true)?;
    if identity(&root_before) != *root_id {
        return Err("WAKE_PROTECTED_PUBLICATION_ROOT_CHANGED".into());
    }
    assert_security(root.as_raw_handle(), &policy.root)?;

    let staging = open_relative(root, "staging", true)?;
    let staging_before = file_info(staging.as_raw_handle(), true)?;
    if identity(&staging_before) != stage.staging_directory
        || staging_before.volume != root_before.volume
    {
        return Err("WAKE_PROTECTED_PUBLICATION_BINDING_INVALID".into());
    }
    assert_security(staging.as_raw_handle(), &policy.staging)?;
    let packages = open_relative(root, "packages", true)?;
    let packages_before = file_info(packages.as_raw_handle(), true)?;
    if packages_before.volume != root_before.volume
        || stage
            .packages_directory
            .as_ref()
            .is_some_and(|expected| identity(&packages_before) != *expected)
    {
        return Err("WAKE_PROTECTED_PUBLICATION_BINDING_INVALID".into());
    }
    assert_security(packages.as_raw_handle(), &policy.packages)?;

    let stage_entries = directory_inventory(&staging, MAX_DIRECTORY_ENTRIES)?;
    let package_entries = directory_inventory(&packages, MAX_PACKAGE_CONTAINER_ENTRIES)?;
    let package_id = &approval.payload().manifest.package_id;
    if package_entries
        .iter()
        .any(|name| name.eq_ignore_ascii_case(package_id) && name != package_id)
    {
        return Err("WAKE_PROTECTED_PUBLICATION_LOCATION_INVALID".into());
    }
    let at_stage = stage_entries.len() == 1 && stage_entries.contains("candidate");
    let stage_empty = stage_entries.is_empty();
    let at_package = package_entries.contains(package_id);
    if (!at_stage && !stage_empty) || at_stage == at_package {
        return Err("WAKE_PROTECTED_PUBLICATION_LOCATION_INVALID".into());
    }
    let (parent, name) = if at_stage {
        (&staging, "candidate")
    } else {
        (&packages, package_id.as_str())
    };
    let artifacts = inspect_publication_package(
        parent,
        name,
        root_before.volume,
        approval,
        stage,
        policy,
        &canonical_manifest,
    )?;

    let final_stage_entries = directory_inventory(&staging, MAX_DIRECTORY_ENTRIES)?;
    let final_package_entries = directory_inventory(&packages, MAX_PACKAGE_CONTAINER_ENTRIES)?;
    if final_stage_entries != stage_entries
        || final_package_entries != package_entries
        || !stable_file_info(&root_before, &file_info(root.as_raw_handle(), true)?)
        || !stable_file_info(&staging_before, &file_info(staging.as_raw_handle(), true)?)
        || !stable_file_info(
            &packages_before,
            &file_info(packages.as_raw_handle(), true)?,
        )
        || exact_security(root.as_raw_handle())? != policy.root
        || exact_security(staging.as_raw_handle())? != policy.staging
        || exact_security(packages.as_raw_handle())? != policy.packages
    {
        return Err("WAKE_PROTECTED_PUBLICATION_OBJECT_CHANGED".into());
    }
    if at_package {
        Ok(PublicationLocationV1::Moved(Box::new(
            ProtectedPublishedPackageReadbackV1 {
                schema_version: 1,
                approval_id: approval.payload().approval_id.clone(),
                install_generation: approval.payload().install_generation,
                package_id: package_id.clone(),
                signed_payload_sha256: approval.signed_payload_sha256().into(),
                manifest_sha256: approval.payload().manifest_sha256.clone(),
                protected_root: root_id.clone(),
                staging_directory: stage.staging_directory.clone(),
                packages_directory: identity(&packages_before),
                package_directory: stage.candidate_directory.clone(),
                manifest_file: stage.manifest_file.clone(),
                artifacts,
                disposition: "MATCHED_SIGNED_PUBLISHED_BYTES_READ_ONLY_NO_INSTALL_AUTHORITY",
            },
        )))
    } else {
        Ok(PublicationLocationV1::Stage)
    }
}

fn pin_published_package_pinned(
    root: &OwnedHandle,
    root_id: &ObjectIdentityV1,
    approval: &VerifiedApprovalV1,
    stage: &StageBindingV1,
    policy: PublicationPolicy,
) -> Result<ProtectedPublishedPackagePinV1> {
    let PublicationLocationV1::Moved(snapshot) =
        inspect_publication_location_pinned(root, root_id, approval, stage, &policy)?
    else {
        return Err("WAKE_PROTECTED_PUBLICATION_LOCATION_INVALID".into());
    };
    let staging = open_relative(root, "staging", true)?;
    let packages = open_relative(root, "packages", true)?;
    let packages_id = identity(&file_info(packages.as_raw_handle(), true)?);
    let package = open_relative(&packages, &approval.payload().manifest.package_id, true)?;
    let manifest = open_relative(&package, "manifest.json", false)?;
    let artifacts = ARTIFACT_NAMES
        .into_iter()
        .map(|name| open_relative(&package, name, false))
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| "WAKE_PROTECTED_READER_INVENTORY_MISMATCH")?;
    let pin = ProtectedPublishedPackagePinV1 {
        approval: approval.clone(),
        stage: stage.clone(),
        snapshot: *snapshot,
        policy,
        staging,
        packages,
        packages_id,
        package,
        manifest,
        artifacts,
    };
    pin.revalidate(root, root_id)?;
    Ok(pin)
}

/// Retain an already moved package while the broker's ledger lock and pinned
/// policy are held. The caller must still bind this supplied approval to its
/// protected policy and `stage` to durable STAGED history. This proof is
/// deliberately read-only and cannot authorize a PUBLISHED record.
pub(crate) fn pin_published_package_under_guard(
    root: &OwnedHandle,
    root_id: &ObjectIdentityV1,
    approval: &VerifiedApprovalV1,
    stage: &StageBindingV1,
    service_sid: &str,
) -> Result<ProtectedPublishedPackagePinV1> {
    let policy = PublicationPolicy::production(service_sid)?;
    pin_published_package_pinned(root, root_id, approval, stage, policy)
}

/// Classify the exact STAGED object while the broker's ledger lock and root
/// pin are held. This is a physical diagnostic only. `VerifiedApprovalV1`
/// alone does not prove protected-policy key provenance; the caller must also
/// compare the durable STAGED record. No writer, INTENT, or pointer authority
/// is returned.
pub(crate) fn inspect_publication_location_under_guard(
    root: &OwnedHandle,
    root_id: &ObjectIdentityV1,
    approval: &VerifiedApprovalV1,
    stage: &StageBindingV1,
    service_sid: &str,
) -> Result<PublicationLocationV1> {
    let policy = PublicationPolicy::production(service_sid)?;
    inspect_publication_location_pinned(root, root_id, approval, stage, &policy)
}

#[cfg(test)]
pub(crate) fn publication_fixture_policy_before_rename(
    root: &OwnedHandle,
    approval: &VerifiedApprovalV1,
    stage: &StageBindingV1,
) -> Result<PublicationFixturePolicyV1> {
    let staging = open_relative(root, "staging", true)?;
    let packages = open_relative(root, "packages", true)?;
    let candidate = open_relative(&staging, "candidate", true)?;
    let manifest = open_relative(&candidate, "manifest.json", false)?;
    let artifacts = ARTIFACT_NAMES
        .into_iter()
        .map(|name| {
            let file = open_relative(&candidate, name, false)?;
            exact_security(file.as_raw_handle())
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| "WAKE_PROTECTED_READER_INVENTORY_MISMATCH")?;
    let policy = PublicationPolicy {
        root: exact_security(root.as_raw_handle())?,
        staging: exact_security(staging.as_raw_handle())?,
        packages: exact_security(packages.as_raw_handle())?,
        package: exact_security(candidate.as_raw_handle())?,
        manifest: exact_security(manifest.as_raw_handle())?,
        artifacts,
    };
    drop(manifest);
    drop(candidate);
    drop(packages);
    drop(staging);
    if inspect_publication_location_pinned(
        root,
        &approval.payload().target_protected_root,
        approval,
        stage,
        &policy,
    )? != PublicationLocationV1::Stage
    {
        return Err("WAKE_PROTECTED_PUBLICATION_LOCATION_INVALID".into());
    }
    Ok(PublicationFixturePolicyV1(policy))
}

#[cfg(test)]
pub(crate) fn inspect_publication_location_fixture(
    root: &OwnedHandle,
    root_id: &ObjectIdentityV1,
    approval: &VerifiedApprovalV1,
    stage: &StageBindingV1,
    policy: &PublicationFixturePolicyV1,
) -> Result<PublicationLocationV1> {
    inspect_publication_location_pinned(root, root_id, approval, stage, &policy.0)
}

#[cfg(test)]
pub(crate) fn pin_published_package_fixture(
    root: &OwnedHandle,
    root_id: &ObjectIdentityV1,
    approval: &VerifiedApprovalV1,
    stage: &StageBindingV1,
    policy: &PublicationFixturePolicyV1,
) -> Result<ProtectedPublishedPackagePinV1> {
    pin_published_package_pinned(root, root_id, approval, stage, policy.0.clone())
}

fn inspect_pinned(
    root: &OwnedHandle,
    expected_root: &ObjectIdentityV1,
    policy: &ReaderPolicy,
) -> Result<(ProtectedInstallReadOnlySnapshotV1, String)> {
    let root_info_before = file_info(root.as_raw_handle(), true)?;
    if identity(&root_info_before) != *expected_root {
        return Err("WAKE_PROTECTED_READER_ROOT_MISMATCH".into());
    }
    assert_security(root.as_raw_handle(), &policy.root)?;
    let pointer_handle = open_relative(root, "current.json", false)?;
    let pointer_object = identity(&file_info(pointer_handle.as_raw_handle(), false)?);
    let mut pointer_file = File::from(pointer_handle);
    let (pointer_bytes, _) = read_bounded(&mut pointer_file, MAX_POINTER_BYTES, &policy.pointer)?;
    let pointer: ProtectedPointerV1 = canonical_json(&pointer_bytes)?;
    validate_pointer(&pointer, expected_root)?;

    let packages = open_relative(root, "packages", true)?;
    assert_security(packages.as_raw_handle(), &policy.packages)?;
    let package = open_relative(&packages, &pointer.package_id, true)?;
    assert_security(package.as_raw_handle(), &policy.package)?;
    let package_info = file_info(package.as_raw_handle(), true)?;
    if identity(&package_info) != pointer.package_directory
        || package_info.volume != root_info_before.volume
    {
        return Err("WAKE_PROTECTED_READER_PACKAGE_MISMATCH".into());
    }
    exact_package_inventory(&package)?;

    let manifest_handle = open_relative(&package, "manifest.json", false)?;
    let manifest_object = identity(&file_info(manifest_handle.as_raw_handle(), false)?);
    let mut manifest_file = File::from(manifest_handle);
    let (manifest_bytes, manifest_info) =
        read_bounded(&mut manifest_file, MAX_MANIFEST_BYTES, &policy.manifest)?;
    if manifest_info.volume != root_info_before.volume
        || manifest_object != pointer.manifest_file
        || format!("{:x}", Sha256::digest(&manifest_bytes)) != pointer.manifest_sha256
    {
        return Err("WAKE_PROTECTED_READER_MANIFEST_MISMATCH".into());
    }
    let manifest: PackageManifestV1 = canonical_json(&manifest_bytes)?;
    validate_manifest(&manifest)?;
    if manifest.package_id != pointer.package_id {
        return Err("WAKE_PROTECTED_READER_MANIFEST_MISMATCH".into());
    }

    let mut artifact_handles = Vec::with_capacity(4);
    for name in ARTIFACT_NAMES {
        artifact_handles.push(File::from(open_relative(&package, name, false)?));
    }
    let mut artifacts = Vec::with_capacity(4);
    for (index, file) in artifact_handles.iter_mut().enumerate() {
        let entry = &manifest.artifacts[index];
        artifacts.push(hash_artifact(
            file,
            ARTIFACT_NAMES[index],
            entry.byte_length,
            &entry.sha256,
            &policy.artifacts[index],
            root_info_before.volume,
        )?);
    }
    let artifacts: [ProtectedArtifactReadbackV1; 4] = artifacts
        .try_into()
        .map_err(|_| "WAKE_PROTECTED_READER_INVENTORY_MISMATCH")?;

    // The pointer, manifest, package and root remain pinned until this final
    // readback, and each pin denies write/delete sharing.
    let (pointer_after, _) = read_bounded(&mut pointer_file, MAX_POINTER_BYTES, &policy.pointer)?;
    let reopened_pointer = open_relative(root, "current.json", false)?;
    let mut reopened_pointer_file = File::from(reopened_pointer);
    let (reopened_bytes, _) = read_bounded(
        &mut reopened_pointer_file,
        MAX_POINTER_BYTES,
        &policy.pointer,
    )?;
    if pointer_after != pointer_bytes
        || reopened_bytes != pointer_bytes
        || identity(&file_info(reopened_pointer_file.as_raw_handle(), false)?) != pointer_object
        || identity(&file_info(pointer_file.as_raw_handle(), false)?) != pointer_object
        || identity(&file_info(package.as_raw_handle(), true)?) != pointer.package_directory
        || identity(&file_info(manifest_file.as_raw_handle(), false)?) != pointer.manifest_file
        || !stable_file_info(&root_info_before, &file_info(root.as_raw_handle(), true)?)
        || exact_security(packages.as_raw_handle())? != policy.packages
        || exact_security(package.as_raw_handle())? != policy.package
        || exact_security(root.as_raw_handle())? != policy.root
    {
        return Err("WAKE_PROTECTED_READER_OBJECT_CHANGED".into());
    }
    exact_package_inventory(&package)?;
    let pointer_sha256 = format!("{:x}", Sha256::digest(&pointer_bytes));
    let snapshot = ProtectedInstallReadOnlySnapshotV1 {
        schema_version: 1,
        install_generation: pointer.install_generation,
        approval_id: pointer.approval_id,
        predecessor_package_id: pointer.predecessor_package_id,
        predecessor_pointer: pointer.predecessor_pointer,
        package_id: pointer.package_id,
        version: manifest.version,
        manifest_sha256: pointer.manifest_sha256,
        protected_root: expected_root.clone(),
        pointer: pointer_object,
        package_directory: pointer.package_directory,
        manifest_file: pointer.manifest_file,
        artifacts,
        disposition: "READ_ONLY_PROTECTED_PACKAGE_SNAPSHOT_NOT_LAUNCH_AUTHORITY",
    };
    Ok((snapshot, pointer_sha256))
}

fn pin_pinned(
    root: &OwnedHandle,
    expected_root: &ObjectIdentityV1,
    policy: ReaderPolicy,
) -> Result<ProtectedInstallPinV1> {
    let (snapshot, pointer_sha256) = inspect_pinned(root, expected_root, &policy)?;
    let pointer = open_relative(root, "current.json", false)?;
    let packages = open_relative(root, "packages", true)?;
    let packages_id = identity(&file_info(packages.as_raw_handle(), true)?);
    let package = open_relative(&packages, &snapshot.package_id, true)?;
    let manifest = open_relative(&package, "manifest.json", false)?;
    let artifacts: [OwnedHandle; 4] = ARTIFACT_NAMES
        .into_iter()
        .map(|name| open_relative(&package, name, false))
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| "WAKE_PROTECTED_READER_INVENTORY_MISMATCH")?;
    let pin = ProtectedInstallPinV1 {
        snapshot,
        pointer_sha256,
        policy,
        pointer,
        packages,
        packages_id,
        package,
        manifest,
        artifacts,
    };
    pin.revalidate(root, expected_root)?;
    Ok(pin)
}

/// The caller must already hold the fixed protected root and broker ledger
/// lock. The returned pins retain the current pointer's physical ID and exact
/// canonical-byte digest for a read-only ledger comparison. They do not prove
/// the broker performed the switch or grant install or launch authority.
pub(crate) fn pin_protected_install_under_guard(
    root: &OwnedHandle,
    expected_root: &ObjectIdentityV1,
    service_sid: &str,
) -> Result<ProtectedInstallPinV1> {
    pin_pinned(root, expected_root, ReaderPolicy::production(service_sid)?)
}

#[cfg(test)]
pub(crate) fn pin_protected_install_fixture(
    root: &OwnedHandle,
    expected_root: &ObjectIdentityV1,
) -> Result<ProtectedInstallPinV1> {
    // Test-only ACL expectations are captured from the isolated fixture.
    // Production always uses ReaderPolicy::production and the fixed service SID.
    let mut pointer = File::from(open_relative(root, "current.json", false)?);
    let pointer_security = exact_security(pointer.as_raw_handle())?;
    let (pointer_bytes, _) = read_bounded(&mut pointer, MAX_POINTER_BYTES, &pointer_security)?;
    let pointer_data: ProtectedPointerV1 = canonical_json(&pointer_bytes)?;
    validate_pointer(&pointer_data, expected_root)?;
    let packages = open_relative(root, "packages", true)?;
    let package = open_relative(&packages, &pointer_data.package_id, true)?;
    let manifest = open_relative(&package, "manifest.json", false)?;
    let artifact_security = ARTIFACT_NAMES
        .into_iter()
        .map(|name| {
            let handle = open_relative(&package, name, false)?;
            exact_security(handle.as_raw_handle())
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| "WAKE_PROTECTED_READER_INVENTORY_MISMATCH")?;
    let policy = ReaderPolicy {
        root: exact_security(root.as_raw_handle())?,
        packages: exact_security(packages.as_raw_handle())?,
        package: exact_security(package.as_raw_handle())?,
        pointer: pointer_security,
        manifest: exact_security(manifest.as_raw_handle())?,
        artifacts: artifact_security,
    };
    pin_pinned(root, expected_root, policy)
}

/// Inspect only the compiled protected ProgramData namespace. This function
/// exposes no request-selected path and performs no install or launch action.
pub fn inspect_fixed_protected_install_read_only() -> Result<ProtectedInstallReadOnlySnapshotV1> {
    let guard = acquire_fixed_protected_root_for_reader()?;
    let policy = ReaderPolicy::production(guard.service_sid())?;
    inspect_pinned(guard.root(), guard.root_identity(), &policy).map(|(snapshot, _)| snapshot)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::install_broker_approval::{ArtifactV1, tests as approval_test, verify_approval};
    use std::{
        os::windows::{ffi::OsStrExt, io::FromRawHandle},
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

    pub(crate) struct Fixture {
        root_path: PathBuf,
        root: OwnedHandle,
        root_id: ObjectIdentityV1,
        policy: ReaderPolicy,
        pointer: ProtectedPointerV1,
        manifest: PackageManifestV1,
    }

    impl Fixture {
        pub(crate) fn new() -> Self {
            let root_path = std::env::temp_dir().join(format!(
                "catdesk-protected-reader-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root_path).expect("fixture root");
            std::fs::create_dir(root_path.join("packages")).expect("package container");
            let blobs: [&[u8]; 4] = [
                b"host-binary",
                b"gui-binary",
                b"adapter-code",
                b"bridge-code",
            ];
            let artifacts = std::array::from_fn(|index| ArtifactV1 {
                name: ARTIFACT_NAMES[index].into(),
                byte_length: blobs[index].len() as u64,
                sha256: format!("{:x}", Sha256::digest(blobs[index])),
            });
            let version = "1.0.0-dev.85".to_string();
            let package_id = format!(
                "{}-{}-{}",
                version,
                &artifacts[0].sha256[..12],
                &artifacts[1].sha256[..12]
            );
            let manifest = PackageManifestV1 {
                schema_version: 1,
                package_id: package_id.clone(),
                version,
                artifacts,
            };
            let package_path = root_path.join("packages").join(&package_id);
            std::fs::create_dir(&package_path).expect("fixture package");
            for (name, blob) in ARTIFACT_NAMES.into_iter().zip(blobs) {
                std::fs::write(package_path.join(name), blob).expect("fixture artifact");
            }
            let manifest_bytes = serde_json::to_vec(&manifest).expect("canonical manifest");
            std::fs::write(package_path.join("manifest.json"), &manifest_bytes)
                .expect("fixture manifest");

            let wide: Vec<u16> = root_path
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
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    ptr::null_mut(),
                )
            };
            assert!(!raw.is_null() && raw != INVALID_HANDLE_VALUE);
            let root = unsafe { OwnedHandle::from_raw_handle(raw) };
            let root_id = identity(&file_info(root.as_raw_handle(), true).expect("root info"));
            let packages = open_relative(&root, "packages", true).expect("packages pin");
            let package = open_relative(&packages, &package_id, true).expect("package pin");
            let manifest_file =
                open_relative(&package, "manifest.json", false).expect("manifest pin");
            let pointer = ProtectedPointerV1 {
                schema_version: 1,
                purpose: "catdesk-wake-protected-current".into(),
                install_generation: 2,
                approval_id: "a".repeat(32),
                predecessor_package_id: "1.0.0-dev.84-example".into(),
                predecessor_pointer: root_id.clone(),
                protected_root: root_id.clone(),
                package_id,
                package_directory: identity(&file_info(package.as_raw_handle(), true).unwrap()),
                manifest_file: identity(&file_info(manifest_file.as_raw_handle(), false).unwrap()),
                manifest_sha256: format!("{:x}", Sha256::digest(&manifest_bytes)),
            };
            drop(manifest_file);
            std::fs::write(
                root_path.join("current.json"),
                serde_json::to_vec(&pointer).expect("canonical pointer"),
            )
            .expect("fixture pointer");
            let pointer_handle = open_relative(&root, "current.json", false).expect("pointer pin");
            let manifest_handle =
                open_relative(&package, "manifest.json", false).expect("manifest pin");
            let artifact_security = std::array::from_fn(|index| {
                let handle =
                    open_relative(&package, ARTIFACT_NAMES[index], false).expect("artifact pin");
                exact_security(handle.as_raw_handle()).expect("artifact security")
            });
            let policy = ReaderPolicy {
                root: exact_security(root.as_raw_handle()).expect("root security"),
                packages: exact_security(packages.as_raw_handle()).expect("packages security"),
                package: exact_security(package.as_raw_handle()).expect("package security"),
                pointer: exact_security(pointer_handle.as_raw_handle()).expect("pointer security"),
                manifest: exact_security(manifest_handle.as_raw_handle())
                    .expect("manifest security"),
                artifacts: artifact_security,
            };
            Self {
                root_path,
                root,
                root_id,
                policy,
                pointer,
                manifest,
            }
        }

        pub(crate) fn root_path(&self) -> &std::path::Path {
            &self.root_path
        }

        pub(crate) fn root(&self) -> &OwnedHandle {
            &self.root
        }

        pub(crate) fn root_id(&self) -> &ObjectIdentityV1 {
            &self.root_id
        }

        pub(crate) fn manifest(&self) -> &PackageManifestV1 {
            &self.manifest
        }

        pub(crate) fn pointer_predecessor(&self) -> (&str, &ObjectIdentityV1) {
            (
                &self.pointer.predecessor_package_id,
                &self.pointer.predecessor_pointer,
            )
        }

        pub(crate) fn inspect(&self) -> Result<ProtectedInstallReadOnlySnapshotV1> {
            inspect_pinned(&self.root, &self.root_id, &self.policy).map(|(snapshot, _)| snapshot)
        }

        fn package_path(&self) -> PathBuf {
            self.root_path
                .join("packages")
                .join(&self.pointer.package_id)
        }

        fn write_pointer(&self, pointer: &ProtectedPointerV1) {
            std::fs::write(
                self.root_path.join("current.json"),
                serde_json::to_vec(pointer).expect("canonical pointer"),
            )
            .expect("write fixture pointer");
        }

        pub(crate) fn cleanup(self) {
            let package_path = self.package_path();
            drop(self.root);
            std::fs::remove_file(self.root_path.join("current.json")).expect("remove pointer");
            std::fs::remove_file(package_path.join("manifest.json")).expect("remove manifest");
            for name in ARTIFACT_NAMES {
                std::fs::remove_file(package_path.join(name)).expect("remove artifact");
            }
            std::fs::remove_dir(&package_path).expect("remove package");
            std::fs::remove_dir(self.root_path.join("packages")).expect("remove packages");
            std::fs::remove_dir(&self.root_path).expect("remove root");
        }
    }

    fn verified_for_pointer_fixture(
        fixture: &Fixture,
        predecessor_package_id: Option<String>,
        predecessor_pointer: Option<ObjectIdentityV1>,
    ) -> VerifiedApprovalV1 {
        let (mut payload, mut observed, key) = approval_test::fixture();
        payload.target_protected_root = fixture.root_id.clone();
        observed.target_protected_root = fixture.root_id.clone();
        payload.manifest = fixture.manifest.clone();
        payload.manifest_sha256 = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&payload.manifest).expect("canonical manifest"))
        );
        payload.predecessor_package_id = predecessor_package_id
            .unwrap_or_else(|| fixture.pointer.predecessor_package_id.clone());
        observed.predecessor_package_id = payload.predecessor_package_id.clone();
        payload.predecessor_pointer =
            predecessor_pointer.unwrap_or_else(|| fixture.pointer.predecessor_pointer.clone());
        observed.predecessor_pointer = payload.predecessor_pointer.clone();
        verify_approval(
            &approval_test::envelope(&payload, &key),
            &key.verifying_key(),
            &observed,
        )
        .expect("signed pointer-plan fixture")
    }

    fn stage_for_pointer_fixture(fixture: &Fixture) -> StageBindingV1 {
        std::fs::create_dir(fixture.root_path.join("staging")).expect("fixture stage parent");
        let staging = open_relative(&fixture.root, "staging", true).expect("stage parent pin");
        let staging_directory =
            identity(&file_info(staging.as_raw_handle(), true).expect("stage parent ID"));
        let installed = fixture.inspect().expect("physical package fixture");
        StageBindingV1 {
            staging_directory,
            packages_directory: None,
            candidate_directory: installed.package_directory,
            manifest_file: installed.manifest_file,
            artifacts: std::array::from_fn(|index| installed.artifacts[index].object.clone()),
        }
    }

    #[test]
    fn canonical_pointer_plan_matches_reader_bytes_without_writing() {
        let fixture = Fixture::new();
        let stage = stage_for_pointer_fixture(&fixture);
        let approval = verified_for_pointer_fixture(&fixture, None, None);
        let pointer_path = fixture.root_path.join("current.json");
        let before = std::fs::read(&pointer_path).expect("original pointer bytes");
        let mut entries_before = std::fs::read_dir(&fixture.root_path)
            .expect("fixture root entries")
            .map(|entry| entry.expect("root entry").file_name())
            .collect::<Vec<_>>();
        entries_before.sort();
        let plan = plan_protected_pointer_from_stage(&approval, &stage)
            .expect("canonical non-authorizing plan");
        assert_eq!(plan.canonical_bytes(), before);
        assert_eq!(plan.sha256(), format!("{:x}", Sha256::digest(&before)));
        assert!(plan.canonical_bytes().len() <= MAX_POINTER_BYTES);
        let parsed: ProtectedPointerV1 =
            canonical_json(plan.canonical_bytes()).expect("reader parses planned bytes");
        assert_eq!(parsed, fixture.pointer);
        assert_eq!(std::fs::read(&pointer_path).unwrap(), before);
        let mut entries_after = std::fs::read_dir(&fixture.root_path)
            .expect("fixture root entries after plan")
            .map(|entry| entry.expect("root entry").file_name())
            .collect::<Vec<_>>();
        entries_after.sort();
        assert_eq!(entries_after, entries_before);
        fixture
            .inspect()
            .expect("reader still accepts fixed pointer");
        std::fs::remove_dir(fixture.root_path.join("staging")).expect("remove fixture stage");
        fixture.cleanup();
    }

    #[test]
    fn pointer_plan_refuses_invalid_stage_and_reader_incompatible_signed_name() {
        let fixture = Fixture::new();
        let stage = stage_for_pointer_fixture(&fixture);
        let approval = verified_for_pointer_fixture(&fixture, None, None);
        let original =
            std::fs::read(fixture.root_path.join("current.json")).expect("original pointer bytes");
        let mut wrong = stage.clone();
        wrong.candidate_directory.volume_serial =
            wrong.candidate_directory.volume_serial.wrapping_add(1);
        assert_eq!(
            plan_protected_pointer_from_stage(&approval, &wrong).unwrap_err(),
            "WAKE_PROTECTED_POINTER_PLAN_STAGE_INVALID"
        );
        let mut wrong = stage.clone();
        wrong.manifest_file.file_index_high = 0;
        wrong.manifest_file.file_index_low = 0;
        assert_eq!(
            plan_protected_pointer_from_stage(&approval, &wrong).unwrap_err(),
            "WAKE_PROTECTED_POINTER_PLAN_STAGE_INVALID"
        );
        let mut wrong = stage.clone();
        wrong.artifacts[1] = wrong.artifacts[0].clone();
        assert_eq!(
            plan_protected_pointer_from_stage(&approval, &wrong).unwrap_err(),
            "WAKE_PROTECTED_POINTER_PLAN_STAGE_INVALID"
        );
        let mut wrong = stage.clone();
        wrong.staging_directory = fixture.root_id.clone();
        assert_eq!(
            plan_protected_pointer_from_stage(&approval, &wrong).unwrap_err(),
            "WAKE_PROTECTED_POINTER_PLAN_STAGE_INVALID"
        );
        let predecessor_collision =
            verified_for_pointer_fixture(&fixture, None, Some(stage.artifacts[0].clone()));
        assert_eq!(
            plan_protected_pointer_from_stage(&predecessor_collision, &stage).unwrap_err(),
            "WAKE_PROTECTED_POINTER_PLAN_STAGE_INVALID"
        );
        let long_name = verified_for_pointer_fixture(&fixture, Some("x".repeat(129)), None);
        assert_eq!(
            plan_protected_pointer_from_stage(&long_name, &stage).unwrap_err(),
            "WAKE_PROTECTED_POINTER_PLAN_INVALID"
        );
        assert_eq!(
            std::fs::read(fixture.root_path.join("current.json")).unwrap(),
            original
        );
        std::fs::remove_dir(fixture.root_path.join("staging")).expect("remove fixture stage");
        fixture.cleanup();
    }

    #[test]
    fn exact_pointer_manifest_inventory_and_four_hashes_pass() {
        let fixture = Fixture::new();
        let snapshot = fixture.inspect().expect("protected fixture snapshot");
        assert_eq!(snapshot.package_id, fixture.manifest.package_id);
        assert_eq!(snapshot.install_generation, 2);
        assert_eq!(snapshot.artifacts[0].name, "CatDeskWakeHost.exe");
        assert_eq!(snapshot.artifacts[3].name, "wake_bridge.py");
        assert_eq!(
            snapshot.disposition,
            "READ_ONLY_PROTECTED_PACKAGE_SNAPSHOT_NOT_LAUNCH_AUTHORITY"
        );
        fixture.cleanup();
    }

    #[test]
    fn retained_package_pins_revalidate_fixed_names_and_refuse_wrong_root() {
        let fixture = Fixture::new();
        let pin = pin_pinned(&fixture.root, &fixture.root_id, fixture.policy.clone())
            .expect("retained protected package");
        assert_eq!(pin.snapshot().package_id, fixture.manifest.package_id);
        pin.revalidate(&fixture.root, &fixture.root_id)
            .expect("exact pinned readback");
        let mut wrong_root = fixture.root_id.clone();
        wrong_root.file_index_low = wrong_root.file_index_low.wrapping_add(1);
        assert_eq!(
            pin.revalidate(&fixture.root, &wrong_root).unwrap_err(),
            "WAKE_PROTECTED_READER_OBJECT_CHANGED"
        );
        assert!(std::fs::write(fixture.root_path.join("current.json"), b"changed").is_err());
        assert!(std::fs::write(fixture.package_path().join("adapter.py"), b"changed").is_err());
        drop(pin);
        fixture.cleanup();
    }

    #[test]
    fn changed_bytes_and_extra_payload_fail_closed() {
        let fixture = Fixture::new();
        let artifact = fixture.package_path().join("adapter.py");
        std::fs::write(&artifact, b"adapter-xode").expect("alter same-length bytes");
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_PROTECTED_READER_HASH_MISMATCH"
        );
        std::fs::write(&artifact, b"adapter-code").expect("restore fixture");
        let extra = fixture.package_path().join("extra.exe");
        std::fs::write(&extra, b"extra").expect("extra payload");
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_PROTECTED_READER_INVENTORY_MISMATCH"
        );
        std::fs::remove_file(extra).expect("remove extra fixture");
        fixture.cleanup();
    }

    #[test]
    fn artifact_length_and_manifest_digest_fail_closed() {
        let fixture = Fixture::new();
        let artifact = fixture.package_path().join("adapter.py");
        std::fs::write(&artifact, b"adapter-code-extra").expect("change artifact length");
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_PROTECTED_READER_ARTIFACT_INVALID"
        );
        std::fs::write(&artifact, b"adapter-code").expect("restore artifact");

        let mut altered = fixture.manifest.clone();
        altered.version = "1.0.0-dev.86".into();
        std::fs::write(
            fixture.package_path().join("manifest.json"),
            serde_json::to_vec(&altered).expect("canonical altered manifest"),
        )
        .expect("change manifest bytes");
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_PROTECTED_READER_MANIFEST_MISMATCH"
        );
        std::fs::write(
            fixture.package_path().join("manifest.json"),
            serde_json::to_vec(&fixture.manifest).expect("canonical original manifest"),
        )
        .expect("restore manifest bytes");
        fixture.cleanup();
    }

    #[test]
    fn valid_alternate_manifest_package_id_cannot_override_pointer() {
        let fixture = Fixture::new();
        let mut altered = fixture.manifest.clone();
        altered.version = "1.0.0-dev.86".into();
        altered.package_id = format!(
            "{}-{}-{}",
            altered.version,
            &altered.artifacts[0].sha256[..12],
            &altered.artifacts[1].sha256[..12]
        );
        let manifest_bytes = serde_json::to_vec(&altered).expect("canonical alternate manifest");
        std::fs::write(
            fixture.package_path().join("manifest.json"),
            &manifest_bytes,
        )
        .expect("write alternate manifest");
        let mut pointer = fixture.pointer.clone();
        pointer.manifest_sha256 = format!("{:x}", Sha256::digest(&manifest_bytes));
        fixture.write_pointer(&pointer);
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_PROTECTED_READER_MANIFEST_MISMATCH"
        );
        fixture.write_pointer(&fixture.pointer);
        std::fs::write(
            fixture.package_path().join("manifest.json"),
            serde_json::to_vec(&fixture.manifest).expect("canonical original manifest"),
        )
        .expect("restore manifest bytes");
        fixture.cleanup();
    }

    #[test]
    fn alternate_case_entry_fails_exact_inventory() {
        let fixture = Fixture::new();
        let package = fixture.package_path();
        std::fs::rename(package.join("adapter.py"), package.join("adapter.tmp"))
            .expect("intermediate case rename");
        std::fs::rename(package.join("adapter.tmp"), package.join("Adapter.py"))
            .expect("alternate case rename");
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_PROTECTED_READER_INVENTORY_MISMATCH"
        );
        std::fs::rename(package.join("Adapter.py"), package.join("adapter.py"))
            .expect("restore exact case");
        fixture.cleanup();
    }

    #[test]
    fn pinned_pointer_refuses_write_and_rebinding() {
        let fixture = Fixture::new();
        let pointer_path = fixture.root_path.join("current.json");
        let pinned = open_relative(&fixture.root, "current.json", false).expect("pointer pin");
        assert!(std::fs::write(&pointer_path, b"replaced").is_err());
        assert!(std::fs::rename(&pointer_path, fixture.root_path.join("former.json")).is_err());
        drop(pinned);
        fixture
            .inspect()
            .expect("unchanged pointer after pin release");
        fixture.cleanup();
    }

    #[test]
    fn hardlink_and_reparse_metadata_are_rejected() {
        let fixture = Fixture::new();
        let link = fixture.root_path.join("outside-link.exe");
        std::fs::hard_link(fixture.package_path().join("CatDeskWakeHost.exe"), &link)
            .expect("fixture hardlink");
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_PROTECTED_READER_OBJECT_INVALID"
        );
        std::fs::remove_file(link).expect("remove fixture hardlink");
        let mut info = FileInfo {
            attributes: FILE_ATTRIBUTE_REPARSE_POINT,
            volume: 1,
            links: 1,
            index_low: 1,
            ..FileInfo::default()
        };
        assert_eq!(
            validate_file_info(info, false).unwrap_err(),
            "WAKE_PROTECTED_READER_OBJECT_INVALID"
        );
        info.attributes = FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT;
        assert_eq!(
            validate_file_info(info, true).unwrap_err(),
            "WAKE_PROTECTED_READER_OBJECT_INVALID"
        );
        fixture.cleanup();
    }

    #[test]
    fn noncanonical_pointer_and_wrong_physical_binding_are_rejected() {
        let fixture = Fixture::new();
        let pretty = serde_json::to_vec_pretty(&fixture.pointer).expect("pretty pointer");
        std::fs::write(fixture.root_path.join("current.json"), pretty)
            .expect("noncanonical fixture");
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_PROTECTED_READER_ENCODING_INVALID"
        );
        let mut wrong = fixture.pointer.clone();
        wrong.package_directory.file_index_low += 1;
        fixture.write_pointer(&wrong);
        assert_eq!(
            fixture.inspect().unwrap_err(),
            "WAKE_PROTECTED_READER_PACKAGE_MISMATCH"
        );
        fixture.cleanup();
    }

    #[test]
    fn exact_acl_policy_rejects_alternate_owner_or_dacl() {
        let fixture = Fixture::new();
        let mut alternate = fixture.policy.clone();
        alternate.artifacts[0].owner = "S-1-5-18".into();
        if alternate.artifacts[0].owner == fixture.policy.artifacts[0].owner {
            alternate.artifacts[0].sddl.push('X');
        }
        assert_eq!(
            inspect_pinned(&fixture.root, &fixture.root_id, &alternate).unwrap_err(),
            "WAKE_PROTECTED_READER_ACL_MISMATCH"
        );
        fixture.cleanup();
    }

    #[test]
    fn component_policy_blocks_traversal_and_alternate_names() {
        for name in [
            "..",
            "foo\\bar",
            "foo/bar",
            "file:stream",
            "trailing.",
            "",
            ".",
        ] {
            assert!(!safe_component(name));
        }
        assert!(safe_component("1.0.0-dev.85-abc-def"));
    }
}
