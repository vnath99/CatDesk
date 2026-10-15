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
    install_broker_approval::{ObjectIdentityV1, PackageManifestV1, validate_manifest},
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

fn exact_package_inventory(package: &OwnedHandle) -> Result<()> {
    let fresh = reopen_directory_for_inventory(package)?;
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
    let mut seen = BTreeSet::new();
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
                ptr::null_mut(),
                u8::from(index == 0),
            )
        } as u32;
        if result == STATUS_NO_MORE_FILES || (index == 0 && result == STATUS_NO_SUCH_FILE) {
            return if seen == expected {
                Ok(())
            } else {
                Err("WAKE_PROTECTED_READER_INVENTORY_MISMATCH".into())
            };
        }
        if result != 0
            || index == MAX_DIRECTORY_ENTRIES
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
        if !expected.contains(name.as_str()) || !seen.insert(name) {
            return Err("WAKE_PROTECTED_READER_INVENTORY_MISMATCH".into());
        }
    }
    Err("WAKE_PROTECTED_READER_INVENTORY_UNAVAILABLE".into())
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

fn inspect_pinned(
    root: &OwnedHandle,
    expected_root: &ObjectIdentityV1,
    policy: &ReaderPolicy,
) -> Result<ProtectedInstallReadOnlySnapshotV1> {
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
    Ok(ProtectedInstallReadOnlySnapshotV1 {
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
    })
}

/// Inspect only the compiled protected ProgramData namespace. This function
/// exposes no request-selected path and performs no install or launch action.
pub fn inspect_fixed_protected_install_read_only() -> Result<ProtectedInstallReadOnlySnapshotV1> {
    let guard = acquire_fixed_protected_root_for_reader()?;
    let policy = ReaderPolicy::production(guard.service_sid())?;
    inspect_pinned(guard.root(), guard.root_identity(), &policy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_broker_approval::ArtifactV1;
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

    struct Fixture {
        root_path: PathBuf,
        root: OwnedHandle,
        root_id: ObjectIdentityV1,
        policy: ReaderPolicy,
        pointer: ProtectedPointerV1,
        manifest: PackageManifestV1,
    }

    impl Fixture {
        fn new() -> Self {
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

        fn inspect(&self) -> Result<ProtectedInstallReadOnlySnapshotV1> {
            inspect_pinned(&self.root, &self.root_id, &self.policy)
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

        fn cleanup(self) {
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
