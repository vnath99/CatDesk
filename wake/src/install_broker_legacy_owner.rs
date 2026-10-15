//! Argument-free, read-only inventory of possible legacy Wake owners.
//!
//! The installed dev.84 host predates the root pin and nonce owner endpoint.
//! A process name, path, open lock file, or per-user pointer cannot prove that
//! it owns the byte-range lock or selects the connected store. This diagnostic
//! therefore never returns an approval observation or an installation lease.

use crate::install_broker_approval::ObjectIdentityV1;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    fs::File,
    mem::size_of,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Path, PathBuf},
    ptr,
};

const PROCESS_NAME: &str = "CatDeskWakeHost.exe";
const DIRECT_ROOT_SLOT: &str = "CURRENT_TOKEN_LOCAL_APP_DATA";
const CODEX_CACHE_SLOT: &str = "CURRENT_TOKEN_CODEX_LOCAL_CACHE";
const ROOT_LEAF: &str = "CatDeskWake";
const CODEX_CACHE_SUFFIX: &str =
    r"Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\CatDeskWake";
const LOCK_LEAF: &str = "host.lock";
const TH32CS_SNAPPROCESS: u32 = 2;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const FILE_SHARE_READ: u32 = 1;
const FILE_SHARE_WRITE: u32 = 2;
const OPEN_EXISTING: u32 = 3;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const WAIT_TIMEOUT: u32 = 258;
const ERROR_NO_MORE_FILES: u32 = 18;
const INVALID_HANDLE: *mut c_void = -1isize as *mut c_void;

// FOLDERID_LocalAppData. The current token's known folder is only a fixed
// forensic candidate; a service token may see a different user's store.
const LOCAL_APP_DATA: Guid = Guid {
    data1: 0xf1b32785,
    data2: 0x6fba,
    data3: 0x4fcf,
    data4: [0x9d, 0x55, 0x7b, 0x8e, 0x7f, 0x15, 0x70, 0x91],
};

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FileTime {
    low: u32,
    high: u32,
}

impl FileTime {
    fn value(self) -> u64 {
        (u64::from(self.high) << 32) | u64::from(self.low)
    }
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

#[repr(C)]
struct ProcessEntry {
    size: u32,
    usage: u32,
    pid: u32,
    default_heap_id: usize,
    module_id: u32,
    threads: u32,
    parent_pid: u32,
    priority: i32,
    flags: u32,
    exe_file: [u16; 260],
}

impl Default for ProcessEntry {
    fn default() -> Self {
        Self {
            size: size_of::<Self>() as u32,
            usage: 0,
            pid: 0,
            default_heap_id: 0,
            module_id: 0,
            threads: 0,
            parent_pid: 0,
            priority: 0,
            flags: 0,
            exe_file: [0; 260],
        }
    }
}

unsafe extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
    fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry) -> i32;
    fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry) -> i32;
    fn GetLastError() -> u32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn GetProcessId(process: *mut c_void) -> u32;
    fn GetProcessTimes(
        process: *mut c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn QueryFullProcessImageNameW(
        process: *mut c_void,
        flags: u32,
        buffer: *mut u16,
        size: *mut u32,
    ) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
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

#[link(name = "shell32")]
unsafe extern "system" {
    fn SHGetKnownFolderPath(
        folder: *const Guid,
        flags: u32,
        token: *mut c_void,
        path: *mut *mut u16,
    ) -> i32;
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoTaskMemFree(memory: *mut c_void);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LegacyOwnerDispositionV1 {
    NoMatchingNamedProcessInSnapshot,
    Ambiguous,
    UnprovenLegacyOwner,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyProcessCandidateV1 {
    pub pid: u32,
    pub creation_time_100ns: Option<u64>,
    pub image_path_sha256: Option<String>,
    pub stable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyRootCandidateV1 {
    pub slot: &'static str,
    pub root_object: Option<ObjectIdentityV1>,
    pub lock_object: Option<ObjectIdentityV1>,
    pub stable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyOwnerInventoryV1 {
    pub schema_version: u32,
    pub disposition: LegacyOwnerDispositionV1,
    pub reason: &'static str,
    pub processes: Vec<LegacyProcessCandidateV1>,
    pub roots: Vec<LegacyRootCandidateV1>,
}

fn classify(
    process_inventory_ok: bool,
    processes: &[LegacyProcessCandidateV1],
    roots: &[LegacyRootCandidateV1],
) -> (LegacyOwnerDispositionV1, &'static str) {
    use LegacyOwnerDispositionV1::{
        Ambiguous, NoMatchingNamedProcessInSnapshot, UnprovenLegacyOwner,
    };
    if !process_inventory_ok {
        return (UnprovenLegacyOwner, "PROCESS_INVENTORY_UNAVAILABLE");
    }
    if processes.is_empty() {
        return (
            NoMatchingNamedProcessInSnapshot,
            "NO_MATCHING_PROCESS_IN_SNAPSHOT",
        );
    }
    if processes.len() != 1 {
        return (Ambiguous, "MULTIPLE_PROCESS_CANDIDATES");
    }
    if !processes[0].stable {
        return (
            UnprovenLegacyOwner,
            "PROCESS_IDENTITY_CHANGED_OR_UNAVAILABLE",
        );
    }
    if roots.iter().any(|root| !root.stable) {
        return (UnprovenLegacyOwner, "CANDIDATE_FILE_ID_DRIFT");
    }
    let distinct_roots = roots
        .iter()
        .filter_map(|root| root.root_object.as_ref())
        .map(|id| (id.volume_serial, id.file_index_high, id.file_index_low))
        .collect::<std::collections::HashSet<_>>();
    if distinct_roots.len() > 1 {
        return (Ambiguous, "SPLIT_ROOT_CANDIDATES");
    }
    (UnprovenLegacyOwner, "LEGACY_HOST_HAS_NO_OWNER_CHALLENGE")
}

/// Inspect only fixed process name and current-token known-folder candidates.
/// No caller supplies a root or PID. `NO_MATCHING_NAMED_PROCESS_IN_SNAPSHOT`
/// means only that the
/// Toolhelp snapshot had no matching process name. The copied IDs are
/// forensic and the result is never a `BrokerObservedStateV1` or an
/// installation capability. Ancestor path traversal is not owner proof.
pub fn inspect_legacy_owner_candidates() -> LegacyOwnerInventoryV1 {
    let process_pins = process_candidates();
    let root_pins = root_candidates();
    let processes: Vec<LegacyProcessCandidateV1> = process_pins
        .as_ref()
        .map(|pins| pins.iter().map(ProcessPin::readback).collect::<Vec<_>>())
        .unwrap_or_default();
    let roots = root_pins.iter().map(RootPin::readback).collect::<Vec<_>>();
    let (disposition, reason) = classify(process_pins.is_ok(), &processes, &roots);
    LegacyOwnerInventoryV1 {
        schema_version: 1,
        disposition,
        reason,
        processes,
        roots,
    }
}

struct ProcessPin {
    pid: u32,
    process: Option<File>,
    creation: Option<u64>,
    image_hash: Option<String>,
}

impl ProcessPin {
    fn new(pid: u32) -> Self {
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
        let process = (!raw.is_null() && raw != INVALID_HANDLE)
            .then(|| unsafe { File::from_raw_handle(raw) });
        let creation = process.as_ref().and_then(process_creation);
        let image_hash = process.as_ref().and_then(process_image_hash);
        Self {
            pid,
            process,
            creation,
            image_hash,
        }
    }

    fn readback(&self) -> LegacyProcessCandidateV1 {
        let stable = self.process.as_ref().is_some_and(|process| {
            let after_image = process_image_hash(process);
            same_process_identity(
                self.pid,
                unsafe { GetProcessId(process.as_raw_handle()) },
                self.creation,
                process_creation(process),
                self.image_hash.as_deref(),
                after_image.as_deref(),
                unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } == WAIT_TIMEOUT,
            )
        });
        LegacyProcessCandidateV1 {
            pid: self.pid,
            creation_time_100ns: self.creation,
            image_path_sha256: self.image_hash.clone(),
            stable,
        }
    }
}

fn same_process_identity(
    expected_pid: u32,
    observed_pid: u32,
    before_creation: Option<u64>,
    after_creation: Option<u64>,
    before_image: Option<&str>,
    after_image: Option<&str>,
    live: bool,
) -> bool {
    expected_pid != 0
        && observed_pid == expected_pid
        && before_creation.is_some()
        && before_creation == after_creation
        && before_image.is_some()
        && before_image == after_image
        && live
}

fn process_candidates() -> std::result::Result<Vec<ProcessPin>, ()> {
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if raw.is_null() || raw == INVALID_HANDLE {
        return Err(());
    }
    let snapshot = unsafe { File::from_raw_handle(raw) };
    let mut entry = ProcessEntry::default();
    if unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) } == 0 {
        return Err(());
    }
    let mut pids = Vec::new();
    loop {
        let end = entry
            .exe_file
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.exe_file.len());
        let name = String::from_utf16_lossy(&entry.exe_file[..end]);
        if name.eq_ignore_ascii_case(PROCESS_NAME) {
            pids.push(entry.pid);
        }
        entry = ProcessEntry::default();
        if unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) } == 0 {
            if unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
                return Err(());
            }
            break;
        }
    }
    pids.sort_unstable();
    pids.dedup();
    Ok(pids.into_iter().map(ProcessPin::new).collect())
}

fn process_creation(process: &File) -> Option<u64> {
    let mut creation = FileTime::default();
    let mut exit = FileTime::default();
    let mut kernel = FileTime::default();
    let mut user = FileTime::default();
    (unsafe {
        GetProcessTimes(
            process.as_raw_handle(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } != 0
        && creation.value() != 0)
        .then(|| creation.value())
}

fn process_image_hash(process: &File) -> Option<String> {
    let mut wide = vec![0u16; 32_768];
    let mut size = wide.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(process.as_raw_handle(), 0, wide.as_mut_ptr(), &mut size)
    } == 0
        || size == 0
        || size as usize >= wide.len()
    {
        return None;
    }
    let path = std::ffi::OsString::from_wide(&wide[..size as usize]);
    let path = path.to_string_lossy().replace('/', "\\").to_lowercase();
    Some(format!("{:x}", Sha256::digest(path.as_bytes())))
}

struct RootPin {
    slot: &'static str,
    path: PathBuf,
    root: Option<File>,
    lock: Option<File>,
    root_id: Option<ObjectIdentityV1>,
    lock_id: Option<ObjectIdentityV1>,
}

impl RootPin {
    fn new(slot: &'static str, path: PathBuf) -> Self {
        let root = open_no_follow(&path, true);
        let root_id = root.as_ref().and_then(|file| object_id(file, true));
        let lock = open_no_follow(&path.join(LOCK_LEAF), false);
        let lock_id = lock.as_ref().and_then(|file| object_id(file, false));
        Self {
            slot,
            path,
            root,
            lock,
            root_id,
            lock_id,
        }
    }

    fn readback(&self) -> LegacyRootCandidateV1 {
        let root_reopen = open_no_follow(&self.path, true);
        let lock_reopen = open_no_follow(&self.path.join(LOCK_LEAF), false);
        let root_now = root_reopen.as_ref().and_then(|file| object_id(file, true));
        let lock_now = lock_reopen.as_ref().and_then(|file| object_id(file, false));
        let stable = self.root.is_some() == root_reopen.is_some()
            && self.lock.is_some() == lock_reopen.is_some()
            && self.root.as_ref().is_none_or(|_| self.root_id.is_some())
            && self.lock.as_ref().is_none_or(|_| self.lock_id.is_some())
            && self.root_id == root_now
            && self.lock_id == lock_now;
        LegacyRootCandidateV1 {
            slot: self.slot,
            root_object: self.root_id.clone(),
            lock_object: self.lock_id.clone(),
            stable,
        }
    }
}

fn root_candidates() -> Vec<RootPin> {
    let Some(local) = current_local_app_data() else {
        return Vec::new();
    };
    vec![
        RootPin::new(DIRECT_ROOT_SLOT, local.join(ROOT_LEAF)),
        RootPin::new(CODEX_CACHE_SLOT, local.join(CODEX_CACHE_SUFFIX)),
    ]
}

fn current_local_app_data() -> Option<PathBuf> {
    let mut raw = ptr::null_mut();
    if unsafe { SHGetKnownFolderPath(&LOCAL_APP_DATA, 0, ptr::null_mut(), &mut raw) } != 0
        || raw.is_null()
    {
        return None;
    }
    let mut len = 0usize;
    while len < 32_768 && unsafe { *raw.add(len) } != 0 {
        len += 1;
    }
    let path = (len < 32_768).then(|| {
        let wide = unsafe { std::slice::from_raw_parts(raw, len) };
        PathBuf::from(std::ffi::OsString::from_wide(wide))
    });
    unsafe { CoTaskMemFree(raw.cast()) };
    path
}

fn open_no_follow(path: &Path, directory: bool) -> Option<File> {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let flags = FILE_FLAG_OPEN_REPARSE_POINT
        | if directory {
            FILE_FLAG_BACKUP_SEMANTICS
        } else {
            0
        };
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null_mut(),
            OPEN_EXISTING,
            flags,
            ptr::null_mut(),
        )
    };
    (!raw.is_null() && raw != INVALID_HANDLE).then(|| unsafe { File::from_raw_handle(raw) })
}

fn object_id(file: &File, directory: bool) -> Option<ObjectIdentityV1> {
    let mut info = FileInfo::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
        || (info.attributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || info.volume == 0
        || (info.index_high == 0 && info.index_low == 0)
    {
        return None;
    }
    Some(ObjectIdentityV1 {
        volume_serial: info.volume,
        file_index_high: info.index_high,
        file_index_low: info.index_low,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(index: u32) -> ObjectIdentityV1 {
        ObjectIdentityV1 {
            volume_serial: 1,
            file_index_high: 0,
            file_index_low: index,
        }
    }

    fn process(pid: u32, stable: bool) -> LegacyProcessCandidateV1 {
        LegacyProcessCandidateV1 {
            pid,
            creation_time_100ns: Some(42),
            image_path_sha256: Some("a".repeat(64)),
            stable,
        }
    }

    fn root(index: u32, stable: bool) -> LegacyRootCandidateV1 {
        LegacyRootCandidateV1 {
            slot: DIRECT_ROOT_SLOT,
            root_object: Some(object(index)),
            lock_object: Some(object(index + 1)),
            stable,
        }
    }

    #[test]
    fn valid_looking_pointer_cannot_create_a_live_owner() {
        let _forensic_pointer =
            br#"{"schemaVersion":1,"directory":"1.0.0-dev.84-02431feba5ed-1c6bcdfcedf0"}"#;
        // There is deliberately no pointer input to this classifier.
        assert_eq!(
            classify(true, &[], &[root(10, true)]),
            (
                LegacyOwnerDispositionV1::NoMatchingNamedProcessInSnapshot,
                "NO_MATCHING_PROCESS_IN_SNAPSHOT"
            )
        );
        assert_eq!(
            classify(false, &[], &[root(10, true)]).0,
            LegacyOwnerDispositionV1::UnprovenLegacyOwner
        );
    }

    #[test]
    fn duplicate_process_or_split_root_remains_ambiguous() {
        assert_eq!(
            classify(
                true,
                &[process(11, true), process(12, true)],
                &[root(10, true)]
            )
            .0,
            LegacyOwnerDispositionV1::Ambiguous
        );
        let mut second = root(20, true);
        second.slot = CODEX_CACHE_SLOT;
        assert_eq!(
            classify(true, &[process(11, true)], &[root(10, true), second]).0,
            LegacyOwnerDispositionV1::Ambiguous
        );
    }

    #[test]
    fn process_or_file_identity_drift_and_open_lock_are_never_owner_proof() {
        let image = "a".repeat(64);
        assert!(same_process_identity(
            11,
            11,
            Some(42),
            Some(42),
            Some(&image),
            Some(&image),
            true
        ));
        assert!(!same_process_identity(
            11,
            12,
            Some(42),
            Some(42),
            Some(&image),
            Some(&image),
            true
        ));
        assert!(!same_process_identity(
            11,
            11,
            Some(42),
            Some(43),
            Some(&image),
            Some(&image),
            true
        ));
        assert!(!same_process_identity(
            11,
            11,
            Some(42),
            Some(42),
            Some(&image),
            Some(&image),
            false
        ));
        assert_eq!(
            classify(true, &[process(11, false)], &[root(10, true)]).1,
            "PROCESS_IDENTITY_CHANGED_OR_UNAVAILABLE"
        );
        assert_eq!(
            classify(true, &[process(11, true)], &[root(10, false)]).1,
            "CANDIDATE_FILE_ID_DRIFT"
        );
        let opened_lock_only = LegacyRootCandidateV1 {
            slot: DIRECT_ROOT_SLOT,
            root_object: None,
            lock_object: Some(object(11)),
            stable: true,
        };
        assert_eq!(
            classify(true, &[process(11, true)], &[opened_lock_only]),
            (
                LegacyOwnerDispositionV1::UnprovenLegacyOwner,
                "LEGACY_HOST_HAS_NO_OWNER_CHALLENGE"
            )
        );
    }

    #[test]
    fn public_inventory_is_read_only_diagnostic_on_any_host_snapshot() {
        let inventory = inspect_legacy_owner_candidates();
        assert!(matches!(
            inventory.disposition,
            LegacyOwnerDispositionV1::NoMatchingNamedProcessInSnapshot
                | LegacyOwnerDispositionV1::Ambiguous
                | LegacyOwnerDispositionV1::UnprovenLegacyOwner
        ));
        let value = serde_json::to_value(inventory).expect("serialize bounded diagnostic");
        let keys = value.as_object().expect("top-level object").keys();
        assert_eq!(
            keys.map(String::as_str).collect::<Vec<_>>(),
            [
                "disposition",
                "processes",
                "reason",
                "roots",
                "schemaVersion"
            ]
        );
    }
}
