//! Handle-scoped, create-once broker transaction records.
//!
//! A future independently installed broker supplies an already pinned and
//! protected root handle. This module never selects a path, provisions a
//! directory, changes a service, reads a candidate package, or authorizes an
//! install mutation. Its file names are derived only from the signed install
//! generation and fixed phase names. A returned lease holds the intent file
//! exclusively until dropped; reopening checks every completed phase.
//!
//! File data is flushed before a phase is reported. Parent-directory metadata
//! durability across sudden power loss is not established by this primitive;
//! the broker must prove that separately before using it as a mutation gate.
//! The protected bootstrap must also pre-create the fixed global lock file,
//! establish its owner/DACL and supply a protected root with list access.
//! This code cannot prove the trustworthiness of a supplied root or clock.
//! An empty history cannot attest its own first generation: bootstrap must
//! bind that first generation to an independently protected predecessor pointer.
//! `advance` only records a claimed phase; the future broker must prove each
//! corresponding physical side effect and recovery invariant before calling it.

use crate::{
    Result,
    install_broker_approval::{
        BrokerObservedStateV1, ObjectIdentityV1, VerifiedApprovalV1, verify_approval,
    },
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
    const ORDER: [Self; 7] = [
        Self::Intent,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LedgerRecordV1 {
    schema_version: u32,
    purpose: String,
    install_generation: u64,
    approval_id: String,
    signed_approval_sha256: String,
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
    phase: String,
    prior_record_sha256: String,
}

#[derive(Clone, Debug)]
struct Binding {
    install_generation: u64,
    approval_id: String,
    signed_approval_sha256: String,
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
}

impl Binding {
    fn record(&self, phase: InstallPhaseV1, prior_record_sha256: &str) -> LedgerRecordV1 {
        LedgerRecordV1 {
            schema_version: 1,
            purpose: "catdesk-wake-protected-install-transaction".into(),
            install_generation: self.install_generation,
            approval_id: self.approval_id.clone(),
            signed_approval_sha256: self.signed_approval_sha256.clone(),
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

fn lowercase_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
fn ledger_inventory(root: &OwnedHandle) -> Result<BTreeMap<u64, [bool; 7]>> {
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
        let slots = found.entry(generation).or_insert([false; 7]);
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
    present: &[bool; 7],
    observed_root: &ObjectIdentityV1,
) -> Result<(LedgerRecordV1, InstallPhaseV1)> {
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
        || !lowercase_hex(&intent.manifest_sha256, 64)
        || intent.accepted_at_utc_seconds == 0
    {
        return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
    }
    let binding = Binding {
        install_generation: intent.install_generation,
        approval_id: intent.approval_id.clone(),
        signed_approval_sha256: intent.signed_approval_sha256.clone(),
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
        digest = read_exact(&mut file, &binding.record(phase, &digest))
            .map_err(|_| "WAKE_LEDGER_RECORD_CORRUPT")?;
        current = phase;
    }
    Ok((intent, current))
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
    for (generation, present) in inventory {
        if generation > binding.install_generation {
            return Err("WAKE_LEDGER_STALE_GENERATION".into());
        }
        if last_prior.is_some_and(|prior| prior.checked_add(1) != Some(generation)) {
            return Err("WAKE_LEDGER_STALE_GENERATION".into());
        }
        let (intent, phase) = historical_intent(root, generation, &present, observed_root)?;
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
            if intent.approval_id == binding.approval_id
                || intent.signed_approval_sha256 == binding.signed_approval_sha256
            {
                return Err("WAKE_LEDGER_APPROVAL_REPLAY".into());
            }
            last_prior = Some(generation);
        }
    }
    if last_prior.is_some_and(|prior| prior.checked_add(1) != Some(binding.install_generation)) {
        return Err("WAKE_LEDGER_STALE_GENERATION".into());
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
    let (intent, _) = historical_intent(root, generation, present, &observed_root)?;
    Ok(Some(intent))
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
    let original = BrokerObservedStateV1 {
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
    };
    let approval = verify_approval(envelope_bytes, broker_trust_root, &original)?;
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

impl<'a> BrokerLedgerLease<'a> {
    pub fn begin(
        root: &'a OwnedHandle,
        approval: &VerifiedApprovalV1,
        expected: &LedgerExpectationV1,
    ) -> Result<Self> {
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
        {
            return Err("WAKE_LEDGER_PREDECESSOR_MISMATCH".into());
        }
        let global_lock = acquire_global_lock(root)?;
        let binding = Binding {
            install_generation: payload.install_generation,
            approval_id: payload.approval_id.clone(),
            signed_approval_sha256: approval.signed_payload_sha256().into(),
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
        let mut current = InstallPhaseV1::Intent;
        let mut gap = false;
        for phase in InstallPhaseV1::ORDER.iter().copied().skip(1) {
            let name = record_name(binding.install_generation, phase);
            match open_optional(root, &name)? {
                Some(mut file) => {
                    if newly_created || gap {
                        return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
                    }
                    digest = read_exact(&mut file, &binding.record(phase, &digest))?;
                    current = phase;
                }
                None => gap = true,
            }
        }
        if root_info(root)? != observed_root {
            return Err("WAKE_LEDGER_ROOT_CHANGED".into());
        }
        Ok(Self {
            root,
            _global_lock: global_lock,
            _intent_file: intent_file,
            binding,
            current,
            current_digest: digest,
        })
    }

    pub fn phase(&self) -> InstallPhaseV1 {
        self.current
    }

    /// Records exactly the next fixed phase. `Ok(false)` means the requested
    /// phase is already current, including duplicate COMPLETE after reopen.
    pub fn advance(&mut self, next: InstallPhaseV1) -> Result<bool> {
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

    #[test]
    fn create_once_crash_reopen_and_ordered_completion_are_exact() {
        let (root, handle, approval, expected) = fixture();
        let mut lease = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("intent");
        assert_eq!(lease.phase(), InstallPhaseV1::Intent);
        assert_eq!(
            lease.advance(InstallPhaseV1::Published).unwrap_err(),
            "WAKE_LEDGER_PHASE_ORDER_INVALID"
        );
        assert!(lease.advance(InstallPhaseV1::Staged).expect("stage"));
        drop(lease);
        let mut resumed = BrokerLedgerLease::begin(&handle, &approval, &expected).expect("reopen");
        assert_eq!(resumed.phase(), InstallPhaseV1::Staged);
        assert!(!resumed.advance(InstallPhaseV1::Staged).expect("idempotent"));
        for phase in InstallPhaseV1::ORDER.iter().copied().skip(2) {
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
        drop(first);
        let (mut payload, mut observed, key) = approval_test::fixture();
        payload.target_protected_root = expected.protected_root.clone();
        observed.target_protected_root = expected.protected_root.clone();
        payload.install_generation += 1;
        observed.next_install_generation = payload.install_generation;
        let next = LedgerExpectationV1 {
            next_install_generation: payload.install_generation,
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
        let (changed, recovered) = verify_exact_resume(
            &handle,
            &changed_envelope,
            &key.verifying_key(),
            payload.install_generation,
            observed.now_utc_seconds,
        )
        .expect("different signature still cryptographically valid");
        assert_eq!(
            BrokerLedgerLease::begin(&handle, &changed, &recovered)
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
        for generation in 2..=4 {
            let approval = if generation == 2 {
                first_approval.clone()
            } else {
                signed_for_generation(
                    &first_expected.protected_root,
                    generation,
                    generation as u128,
                )
            };
            let expected = LedgerExpectationV1 {
                next_install_generation: generation,
                ..first_expected.clone()
            };
            let mut lease =
                BrokerLedgerLease::begin(&handle, &approval, &expected).expect("historical intent");
            for phase in InstallPhaseV1::ORDER.iter().copied().skip(1) {
                lease.advance(phase).expect("complete history");
            }
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
        for generation in 2..=3 {
            let approval = if generation == 2 {
                first_approval.clone()
            } else {
                signed_for_generation(&first_expected.protected_root, generation, 3)
            };
            let expected = LedgerExpectationV1 {
                next_install_generation: generation,
                ..first_expected.clone()
            };
            let mut lease =
                BrokerLedgerLease::begin(&handle, &approval, &expected).expect("historical intent");
            for phase in InstallPhaseV1::ORDER.iter().copied().skip(1) {
                lease.advance(phase).expect("complete history");
            }
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
