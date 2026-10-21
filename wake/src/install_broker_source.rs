//! Pinned, untrusted byte intake for a future fixed Wake install broker.
//!
//! Only the compiled `C:\ProgramData\CatDesk\WakeSubmissionV1\candidate`
//! source is opened. Its bytes are never an authority until a separately
//! protected stage has been created and bound to the transaction ledger.

#![cfg(windows)]

use crate::{
    Result,
    install_broker_approval::{PackageManifestV1, VerifiedApprovalV1, validate_manifest},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::{OsStr, c_void},
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr,
};

const SOURCE_CHAIN: [&str; 4] = ["ProgramData", "CatDesk", "WakeSubmissionV1", "candidate"];
const ARTIFACT_NAMES: [&str; 4] = [
    "CatDeskWakeHost.exe",
    "CatDeskBinagotchy.exe",
    "adapter.py",
    "wake_bridge.py",
];
const MANIFEST_NAME: &str = "manifest.json";
const MAX_MANIFEST_BYTES: usize = 4096;
const MAX_DIRECTORY_ENTRIES: usize = 16;
const FILE_OPEN: u32 = 1;
const FILE_READ_DATA: u32 = 1;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_READ: u32 = 1;
const FILE_SHARE_WRITE: u32 = 2;
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
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *mut c_void,
        disposition: u32,
        flags: u32,
        template: *mut c_void,
    ) -> *mut c_void;
    fn GetFileInformationByHandle(handle: *mut c_void, info: *mut FileInfo) -> i32;
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !matches!(value, "." | "..")
        && !value.ends_with(['.', ' '])
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn info(raw: *mut c_void, directory: bool) -> Result<FileInfo> {
    let mut result = FileInfo::default();
    if unsafe { GetFileInformationByHandle(raw, &mut result) } == 0 {
        return Err("WAKE_BROKER_SOURCE_OBJECT_INVALID".into());
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
        return Err("WAKE_BROKER_SOURCE_OBJECT_INVALID".into());
    }
    Ok(value)
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

fn same_object(before: &FileInfo, after: &FileInfo) -> bool {
    before.volume == after.volume
        && before.index_high == after.index_high
        && before.index_low == after.index_low
        && before.attributes == after.attributes
}

fn size(info: &FileInfo) -> u64 {
    (u64::from(info.size_high) << 32) | u64::from(info.size_low)
}

fn open_path_directory(path: &OsStr) -> Result<OwnedHandle> {
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    let wide: Vec<u16> = path.encode_wide().chain(std::iter::once(0)).collect();
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null_mut(),
            3,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err("WAKE_BROKER_SOURCE_ROOT_UNAVAILABLE".into());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    info(handle.as_raw_handle(), true)?;
    Ok(handle)
}

fn open_relative(parent: &OwnedHandle, name: &str, directory: bool) -> Result<OwnedHandle> {
    if !valid_component(name) {
        return Err("WAKE_BROKER_SOURCE_COMPONENT_INVALID".into());
    }
    let mut units: Vec<u16> = name.encode_utf16().collect();
    let length =
        u16::try_from(units.len() * 2).map_err(|_| "WAKE_BROKER_SOURCE_COMPONENT_INVALID")?;
    let mut object_name = UnicodeString {
        length,
        maximum_length: length,
        buffer: units.as_mut_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: parent.as_raw_handle(),
        object_name: &mut object_name,
        attributes: 0x40, // OBJ_CASE_INSENSITIVE; directory readback checks exact case.
        security_descriptor: ptr::null_mut(),
        security_quality_of_service: ptr::null_mut(),
    };
    let mut iosb = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut raw = ptr::null_mut();
    let result = unsafe {
        NtCreateFile(
            &mut raw,
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &mut attributes,
            &mut iosb,
            ptr::null_mut(),
            0,
            if directory {
                FILE_SHARE_READ | FILE_SHARE_WRITE
            } else {
                FILE_SHARE_READ
            },
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
        return Err("WAKE_BROKER_SOURCE_OBJECT_UNAVAILABLE".into());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    info(handle.as_raw_handle(), directory)?;
    Ok(handle)
}

fn fresh_directory(directory: &OwnedHandle) -> Result<OwnedHandle> {
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
    let result = unsafe {
        NtCreateFile(
            &mut raw,
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &mut attributes,
            &mut iosb,
            ptr::null_mut(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_OPEN,
            FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
            ptr::null_mut(),
            0,
        )
    };
    if result < 0 || raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err("WAKE_BROKER_SOURCE_INVENTORY_UNAVAILABLE".into());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    if !same_object(
        &info(handle.as_raw_handle(), true)?,
        &info(directory.as_raw_handle(), true)?,
    ) {
        return Err("WAKE_BROKER_SOURCE_OBJECT_CHANGED".into());
    }
    Ok(handle)
}

fn inventory(directory: &OwnedHandle, filter: Option<&str>, expected: &[&str]) -> Result<()> {
    let fresh = fresh_directory(directory)?;
    let expected: BTreeSet<String> = expected.iter().map(|name| (*name).to_string()).collect();
    let mut seen = BTreeSet::new();
    let mut pattern = filter.map(|name| name.encode_utf16().collect::<Vec<_>>());
    let mut filter_name = pattern.as_mut().map(|units| {
        let length = u16::try_from(units.len() * 2).expect("fixed component length");
        UnicodeString {
            length,
            maximum_length: length,
            buffer: units.as_mut_ptr(),
        }
    });
    for index in 0..=MAX_DIRECTORY_ENTRIES {
        let mut aligned = [0u64; 128];
        let mut iosb = IoStatusBlock {
            status: 0,
            information: 0,
        };
        let result = unsafe {
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
                if index == 0 {
                    filter_name.as_mut().map_or(ptr::null_mut(), |value| value)
                } else {
                    ptr::null_mut()
                },
                u8::from(index == 0),
            )
        } as u32;
        if result == STATUS_NO_MORE_FILES || (index == 0 && result == STATUS_NO_SUCH_FILE) {
            return if seen == expected {
                Ok(())
            } else {
                Err("WAKE_BROKER_SOURCE_INVENTORY_MISMATCH".into())
            };
        }
        if result != 0
            || index == MAX_DIRECTORY_ENTRIES
            || iosb.information < 12
            || iosb.information > std::mem::size_of_val(&aligned)
        {
            return Err("WAKE_BROKER_SOURCE_INVENTORY_UNAVAILABLE".into());
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
            return Err("WAKE_BROKER_SOURCE_INVENTORY_UNAVAILABLE".into());
        }
        let units: Vec<u16> = bytes[12..12 + name_length]
            .chunks_exact(2)
            .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
            .collect();
        let name =
            String::from_utf16(&units).map_err(|_| "WAKE_BROKER_SOURCE_INVENTORY_MISMATCH")?;
        if matches!(name.as_str(), "." | "..") {
            continue;
        }
        if !expected.contains(name.as_str()) || !seen.insert(name) {
            return Err("WAKE_BROKER_SOURCE_INVENTORY_MISMATCH".into());
        }
    }
    Err("WAKE_BROKER_SOURCE_INVENTORY_UNAVAILABLE".into())
}

fn read_manifest(file: &mut File) -> Result<(Vec<u8>, FileInfo)> {
    let before = info(file.as_raw_handle(), false)?;
    let length = size(&before);
    if length == 0 || length > MAX_MANIFEST_BYTES as u64 {
        return Err("WAKE_BROKER_SOURCE_MANIFEST_INVALID".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_BROKER_SOURCE_READ_FAILED")?;
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WAKE_BROKER_SOURCE_READ_FAILED")?;
    if bytes.len() as u64 != length || !stable(&before, &info(file.as_raw_handle(), false)?) {
        return Err("WAKE_BROKER_SOURCE_OBJECT_CHANGED".into());
    }
    Ok((bytes, before))
}

fn hash_artifact(file: &mut File, expected_length: u64, expected_sha256: &str) -> Result<FileInfo> {
    let before = info(file.as_raw_handle(), false)?;
    if size(&before) != expected_length || expected_length == 0 {
        return Err("WAKE_BROKER_SOURCE_ARTIFACT_INVALID".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_BROKER_SOURCE_READ_FAILED")?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut count_total = 0u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "WAKE_BROKER_SOURCE_READ_FAILED")?;
        if count == 0 {
            break;
        }
        count_total = count_total
            .checked_add(count as u64)
            .ok_or("WAKE_BROKER_SOURCE_ARTIFACT_INVALID")?;
        if count_total > expected_length {
            return Err("WAKE_BROKER_SOURCE_ARTIFACT_INVALID".into());
        }
        digest.update(&buffer[..count]);
    }
    if count_total != expected_length || format!("{:x}", digest.finalize()) != expected_sha256 {
        return Err("WAKE_BROKER_SOURCE_HASH_MISMATCH".into());
    }
    if !stable(&before, &info(file.as_raw_handle(), false)?) {
        return Err("WAKE_BROKER_SOURCE_OBJECT_CHANGED".into());
    }
    Ok(before)
}

/// The source is untrusted, but all bytes and names are pinned and compared
/// with the already verified detached approval. This type cannot install.
pub(crate) struct PinnedCandidateSourceV1 {
    ancestors: Vec<OwnedHandle>,
    chain: Vec<String>,
    exclusive_parent: bool,
    directory_info: Vec<FileInfo>,
    manifest: File,
    manifest_info: FileInfo,
    manifest_bytes: Vec<u8>,
    artifacts: [File; 4],
    artifact_info: [FileInfo; 4],
}

impl PinnedCandidateSourceV1 {
    pub(crate) fn manifest_bytes(&self) -> &[u8] {
        &self.manifest_bytes
    }

    #[cfg(test)]
    pub(crate) fn artifact(&self, index: usize) -> &File {
        &self.artifacts[index]
    }

    pub(crate) fn artifact_mut(&mut self, index: usize) -> &mut File {
        &mut self.artifacts[index]
    }

    /// Rehash the same pinned bytes and compare every fixed-name reopen before
    /// a protected stage records provenance. A source change only refuses use.
    pub(crate) fn revalidate(&mut self, approval: &VerifiedApprovalV1) -> Result<()> {
        self.revalidate_manifest(
            &approval.payload().manifest,
            &approval.payload().manifest_sha256,
        )
    }

    /// Manifest-layout byte checks shared with V2. This does not turn an
    /// unsigned manifest or these read pins into installation authority.
    pub(crate) fn revalidate_manifest(
        &mut self,
        manifest: &PackageManifestV1,
        manifest_sha256: &str,
    ) -> Result<()> {
        validate_manifest(manifest)?;
        let canonical =
            serde_json::to_vec(manifest).map_err(|_| "WAKE_BROKER_SOURCE_MANIFEST_INVALID")?;
        if canonical.len() > MAX_MANIFEST_BYTES
            || format!("{:x}", Sha256::digest(&canonical)) != manifest_sha256
            || self.manifest_bytes != canonical
        {
            return Err("WAKE_BROKER_SOURCE_MANIFEST_MISMATCH".into());
        }
        for (index, handle) in self.ancestors.iter().enumerate() {
            let now = info(handle.as_raw_handle(), true)?;
            if !same_object(&self.directory_info[index], &now)
                || (index + 2 >= self.ancestors.len() && !stable(&self.directory_info[index], &now))
            {
                return Err("WAKE_BROKER_SOURCE_OBJECT_CHANGED".into());
            }
            if index > 0 {
                inventory(
                    &self.ancestors[index - 1],
                    Some(&self.chain[index - 1]),
                    &[&self.chain[index - 1]],
                )?;
                let reopened =
                    open_relative(&self.ancestors[index - 1], &self.chain[index - 1], true)?;
                if !same_object(&now, &info(reopened.as_raw_handle(), true)?) {
                    return Err("WAKE_BROKER_SOURCE_OBJECT_CHANGED".into());
                }
            }
        }
        let submission = &self.ancestors[self.ancestors.len() - 2];
        let candidate = &self.ancestors[self.ancestors.len() - 1];
        let package_name = self
            .chain
            .last()
            .ok_or("WAKE_BROKER_SOURCE_COMPONENT_INVALID")?;
        inventory(
            submission,
            if self.exclusive_parent {
                None
            } else {
                Some(package_name)
            },
            &[package_name],
        )?;
        inventory(
            candidate,
            None,
            &[
                MANIFEST_NAME,
                ARTIFACT_NAMES[0],
                ARTIFACT_NAMES[1],
                ARTIFACT_NAMES[2],
                ARTIFACT_NAMES[3],
            ],
        )?;
        let (manifest_bytes, now_manifest) = read_manifest(&mut self.manifest)?;
        if manifest_bytes != canonical || !stable(&self.manifest_info, &now_manifest) {
            return Err("WAKE_BROKER_SOURCE_MANIFEST_MISMATCH".into());
        }
        let reopened = open_relative(candidate, MANIFEST_NAME, false)?;
        if !same_object(&now_manifest, &info(reopened.as_raw_handle(), false)?) {
            return Err("WAKE_BROKER_SOURCE_OBJECT_CHANGED".into());
        }
        for (index, name) in ARTIFACT_NAMES.iter().enumerate() {
            let expected = &manifest.artifacts[index];
            let now = hash_artifact(
                &mut self.artifacts[index],
                expected.byte_length,
                &expected.sha256,
            )?;
            if !stable(&self.artifact_info[index], &now) {
                return Err("WAKE_BROKER_SOURCE_OBJECT_CHANGED".into());
            }
            let reopened = open_relative(candidate, name, false)?;
            if !same_object(&now, &info(reopened.as_raw_handle(), false)?) {
                return Err("WAKE_BROKER_SOURCE_OBJECT_CHANGED".into());
            }
        }
        inventory(
            candidate,
            None,
            &[
                MANIFEST_NAME,
                ARTIFACT_NAMES[0],
                ARTIFACT_NAMES[1],
                ARTIFACT_NAMES[2],
                ARTIFACT_NAMES[3],
            ],
        )?;
        Ok(())
    }

    /// Retained objects only, in fixed manifest-layout order. The caller must
    /// keep this value alive and separately check protected-stage security.
    pub(crate) fn package_pins(&self) -> (&OwnedHandle, &OwnedHandle, &File, &[File; 4]) {
        (
            &self.ancestors[self.ancestors.len() - 2],
            &self.ancestors[self.ancestors.len() - 1],
            &self.manifest,
            &self.artifacts,
        )
    }
}

fn open_candidate_in(
    root: OwnedHandle,
    chain: &[&'static str],
    approval: &VerifiedApprovalV1,
) -> Result<PinnedCandidateSourceV1> {
    open_candidate_manifest_in(
        root,
        chain,
        &approval.payload().manifest,
        &approval.payload().manifest_sha256,
    )
}

fn open_candidate_manifest_in(
    root: OwnedHandle,
    chain: &[&'static str],
    manifest: &PackageManifestV1,
    manifest_sha256: &str,
) -> Result<PinnedCandidateSourceV1> {
    if chain.len() < 2 || chain.last() != Some(&"candidate") {
        return Err("WAKE_BROKER_SOURCE_COMPONENT_INVALID".into());
    }
    open_manifest_package_in(root, chain, manifest, manifest_sha256, true)
}

fn open_manifest_package_in(
    root: OwnedHandle,
    chain: &[&str],
    manifest: &PackageManifestV1,
    manifest_sha256: &str,
    exclusive_parent: bool,
) -> Result<PinnedCandidateSourceV1> {
    if chain.len() < 2 || chain.iter().any(|name| !valid_component(name)) {
        return Err("WAKE_BROKER_SOURCE_COMPONENT_INVALID".into());
    }
    validate_manifest(manifest)?;
    let mut ancestors = vec![root];
    let mut directory_info = vec![info(ancestors[0].as_raw_handle(), true)?];
    for component in chain {
        let parent = ancestors.last().expect("root held");
        inventory(parent, Some(component), &[*component])?;
        let child = open_relative(parent, component, true)?;
        let child_info = info(child.as_raw_handle(), true)?;
        if child_info.volume != directory_info[0].volume {
            return Err("WAKE_BROKER_SOURCE_OBJECT_INVALID".into());
        }
        directory_info.push(child_info);
        ancestors.push(child);
    }
    let submission = &ancestors[ancestors.len() - 2];
    let candidate = &ancestors[ancestors.len() - 1];
    let package_name = chain.last().ok_or("WAKE_BROKER_SOURCE_COMPONENT_INVALID")?;
    inventory(
        submission,
        if exclusive_parent {
            None
        } else {
            Some(package_name)
        },
        &[*package_name],
    )?;
    inventory(
        candidate,
        None,
        &[
            MANIFEST_NAME,
            ARTIFACT_NAMES[0],
            ARTIFACT_NAMES[1],
            ARTIFACT_NAMES[2],
            ARTIFACT_NAMES[3],
        ],
    )?;
    let manifest_file = File::from(open_relative(candidate, MANIFEST_NAME, false)?);
    let mut artifact_vec = Vec::with_capacity(4);
    for name in ARTIFACT_NAMES {
        artifact_vec.push(File::from(open_relative(candidate, name, false)?));
    }
    let artifacts: [File; 4] = artifact_vec
        .try_into()
        .map_err(|_| "WAKE_BROKER_SOURCE_INVENTORY_MISMATCH")?;
    let mut source = PinnedCandidateSourceV1 {
        ancestors,
        chain: chain.iter().map(|name| (*name).to_owned()).collect(),
        exclusive_parent,
        directory_info,
        manifest: manifest_file,
        manifest_info: FileInfo::default(),
        manifest_bytes: Vec::new(),
        artifacts,
        artifact_info: [FileInfo::default(); 4],
    };
    let (bytes, manifest_info) = read_manifest(&mut source.manifest)?;
    source.manifest_info = manifest_info;
    source.manifest_bytes = bytes;
    for index in 0..4 {
        let signed = &manifest.artifacts[index];
        source.artifact_info[index] = hash_artifact(
            &mut source.artifacts[index],
            signed.byte_length,
            &signed.sha256,
        )?;
    }
    source.revalidate_manifest(manifest, manifest_sha256)?;
    Ok(source)
}

pub(crate) fn open_fixed_candidate_source(
    approval: &VerifiedApprovalV1,
) -> Result<PinnedCandidateSourceV1> {
    let volume = open_path_directory(OsStr::new(r"C:\"))?;
    open_candidate_in(volume, &SOURCE_CHAIN, approval)
}

/// Same fixed, untrusted source; V2 supplies only its already checked manifest
/// layout. There is no caller-selected path and no owner-field conversion.
pub(crate) fn pin_fixed_candidate_manifest(
    manifest: &PackageManifestV1,
    manifest_sha256: &str,
) -> Result<PinnedCandidateSourceV1> {
    let volume = open_path_directory(OsStr::new(r"C:\"))?;
    open_candidate_manifest_in(volume, &SOURCE_CHAIN, manifest, manifest_sha256)
}

pub(crate) fn pin_staged_candidate_manifest(
    root: &OwnedHandle,
    manifest: &PackageManifestV1,
    manifest_sha256: &str,
) -> Result<PinnedCandidateSourceV1> {
    let retained = root
        .try_clone()
        .map_err(|_| "WAKE_BROKER_SOURCE_OBJECT_UNAVAILABLE")?;
    open_candidate_manifest_in(
        retained,
        &["staging", "candidate"],
        manifest,
        manifest_sha256,
    )
}

/// Read pins for the exact signed package name under the fixed packages role.
/// This only validates bytes; the V2 publication receipt separately checks
/// protected ACLs, full identities, location and durable transaction history.
pub(crate) fn pin_published_candidate_manifest(
    root: &OwnedHandle,
    manifest: &PackageManifestV1,
    manifest_sha256: &str,
) -> Result<PinnedCandidateSourceV1> {
    let retained = root
        .try_clone()
        .map_err(|_| "WAKE_BROKER_SOURCE_OBJECT_UNAVAILABLE")?;
    open_manifest_package_in(
        retained,
        &["packages", &manifest.package_id],
        manifest,
        manifest_sha256,
        false,
    )
}

#[cfg(test)]
pub(crate) fn pin_fixture_candidate_manifest(
    root: &std::path::Path,
    manifest: &PackageManifestV1,
    manifest_sha256: &str,
) -> Result<PinnedCandidateSourceV1> {
    let volume = open_path_directory(root.as_os_str())?;
    open_candidate_manifest_in(
        volume,
        &["WakeSubmissionV1", "candidate"],
        manifest,
        manifest_sha256,
    )
}

#[cfg(test)]
pub(crate) fn open_fixture_candidate_source(
    root: &std::path::Path,
    approval: &VerifiedApprovalV1,
) -> Result<PinnedCandidateSourceV1> {
    let volume = open_path_directory(root.as_os_str())?;
    open_candidate_in(volume, &["WakeSubmissionV1", "candidate"], approval)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_broker_approval::{self, ArtifactV1, verify_approval};
    use std::{
        io::Read,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(1);

    struct Fixture {
        root: PathBuf,
        approval: VerifiedApprovalV1,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "catdesk-broker-source-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let candidate = root.join("WakeSubmissionV1").join("candidate");
            std::fs::create_dir_all(&candidate).expect("source directories");
            let blobs: [&[u8]; 4] = [
                b"host-binary",
                b"gui-binary",
                b"adapter-code",
                b"bridge-code",
            ];
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
            let manifest = serde_json::to_vec(&payload.manifest).expect("canonical manifest");
            payload.manifest_sha256 = format!("{:x}", Sha256::digest(&manifest));
            observed.now_utc_seconds = 1500;
            for (name, blob) in ARTIFACT_NAMES.into_iter().zip(blobs) {
                std::fs::write(candidate.join(name), blob).expect("source artifact");
            }
            std::fs::write(candidate.join(MANIFEST_NAME), manifest).expect("source manifest");
            let envelope = install_broker_approval::tests::envelope(&payload, &key);
            let approval = verify_approval(&envelope, &key.verifying_key(), &observed)
                .expect("verified source fixture");
            Self { root, approval }
        }

        fn candidate(&self) -> PathBuf {
            self.root.join("WakeSubmissionV1").join("candidate")
        }

        fn open(&self) -> Result<PinnedCandidateSourceV1> {
            let root = open_path_directory(self.root.as_os_str())?;
            open_candidate_in(root, &["WakeSubmissionV1", "candidate"], &self.approval)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).expect("fixture cleanup");
        }
    }

    #[test]
    fn fixed_shape_source_pins_exact_signed_bytes_and_refuses_write_rename() {
        let fixture = Fixture::new();
        let mut source = fixture.open().expect("signed source");
        assert_eq!(
            source.manifest_bytes(),
            serde_json::to_vec(&fixture.approval.payload().manifest).unwrap()
        );
        let mut reader = source.artifact(0).try_clone().expect("pinned duplicate");
        reader.seek(SeekFrom::Start(0)).expect("rewind");
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).expect("read");
        assert_eq!(bytes, b"host-binary");
        drop(reader);
        assert!(std::fs::write(fixture.candidate().join(ARTIFACT_NAMES[0]), b"changed").is_err());
        assert!(
            std::fs::rename(
                fixture.candidate().join(ARTIFACT_NAMES[0]),
                fixture.candidate().join("renamed.exe")
            )
            .is_err()
        );
        source
            .artifact_mut(0)
            .seek(SeekFrom::Start(0))
            .expect("mutable source handle");
        source
            .revalidate(&fixture.approval)
            .expect("pinned revalidation");
        drop(source);
    }

    #[test]
    fn extra_entry_case_alias_and_noncanonical_manifest_fail() {
        let fixture = Fixture::new();
        let extra = fixture.candidate().join("extra.exe");
        std::fs::write(&extra, b"extra").expect("extra");
        assert_eq!(
            fixture.open().err().unwrap(),
            "WAKE_BROKER_SOURCE_INVENTORY_MISMATCH"
        );
        std::fs::remove_file(&extra).expect("remove extra");
        let original = fixture.candidate().join("adapter.py");
        let middle = fixture.candidate().join("adapter.tmp");
        std::fs::rename(&original, &middle).expect("rename intermediate");
        std::fs::rename(&middle, fixture.candidate().join("Adapter.py")).expect("case alias");
        assert_eq!(
            fixture.open().err().unwrap(),
            "WAKE_BROKER_SOURCE_INVENTORY_MISMATCH"
        );
        std::fs::rename(fixture.candidate().join("Adapter.py"), &middle)
            .expect("restore intermediate");
        std::fs::rename(&middle, &original).expect("restore");
        let pretty =
            serde_json::to_vec_pretty(&fixture.approval.payload().manifest).expect("pretty");
        std::fs::write(fixture.candidate().join(MANIFEST_NAME), pretty).expect("noncanonical");
        assert_eq!(
            fixture.open().err().unwrap(),
            "WAKE_BROKER_SOURCE_MANIFEST_MISMATCH"
        );
    }

    #[test]
    fn wrong_hash_length_hardlink_and_reparse_metadata_fail() {
        let fixture = Fixture::new();
        let file = fixture.candidate().join("adapter.py");
        std::fs::write(&file, b"adapter-xode").expect("same length change");
        assert_eq!(
            fixture.open().err().unwrap(),
            "WAKE_BROKER_SOURCE_HASH_MISMATCH"
        );
        std::fs::write(&file, b"adapter-code-long").expect("wrong length");
        assert_eq!(
            fixture.open().err().unwrap(),
            "WAKE_BROKER_SOURCE_ARTIFACT_INVALID"
        );
        std::fs::write(&file, b"adapter-code").expect("restore");
        let link = fixture.root.join("outside-link.py");
        std::fs::hard_link(&file, &link).expect("hardlink");
        assert_eq!(
            fixture.open().err().unwrap(),
            "WAKE_BROKER_SOURCE_OBJECT_INVALID"
        );
        std::fs::remove_file(link).expect("remove link");
        let bad = FileInfo {
            attributes: FILE_ATTRIBUTE_REPARSE_POINT,
            links: 1,
            volume: 1,
            index_low: 1,
            ..FileInfo::default()
        };
        assert_eq!(
            validate_info(bad, false).unwrap_err(),
            "WAKE_BROKER_SOURCE_OBJECT_INVALID"
        );
    }
}
