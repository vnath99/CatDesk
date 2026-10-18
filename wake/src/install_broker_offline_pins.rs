//! Retained physical object pins for the offline V2 diagnostic.
//!
//! The diagnostic's path-opened handles exclude new write/delete opens and
//! bind both legacy file information and full FILE_ID_INFO. A borrowed root
//! handle retains its caller's share mode; its exact child `host.lock` is
//! separately pinned and locked. Neither proves that the selected stores
//! exhaust the historical token-specific inventory, that a launch path still
//! resolves to a selected root, or that an install phase is authorized.

use crate::{
    Result,
    install_broker_approval::ObjectIdentityV1,
    install_broker_offline_v2::{
        FullFileIdentityV2, OfflineMigrationObservationV2, OfflineObjectIdentityV2,
    },
};
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path, PathBuf},
    ptr,
};

const FILE_SHARE_READ: u32 = 1;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_ID_INFO_CLASS: u32 = 18;
const FILE_OPEN: u32 = 1;
const FILE_READ_DATA: u32 = 1;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_NON_DIRECTORY_FILE: u32 = 0x40;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x20;
const OBJ_CASE_INSENSITIVE: u32 = 0x40;
const MAX_POINTER_BYTES: u64 = 4096;
const MAX_HOST_IMAGE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_OLD_STORE_CANDIDATES: usize = 8;
const HOST_LOCK_LEAF: &str = "host.lock";

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

#[repr(C)]
#[derive(Default)]
struct FileIdInfo {
    volume_serial: u64,
    file_id: [u8; 16],
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileInformationByHandle(handle: *mut c_void, info: *mut FileInfo) -> i32;
    fn GetFileInformationByHandleEx(
        handle: *mut c_void,
        information_class: u32,
        info: *mut c_void,
        size: u32,
    ) -> i32;
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

fn inspect(file: &File, directory: bool) -> Result<OfflineObjectIdentityV2> {
    let raw = file.as_raw_handle();
    let mut legacy = FileInfo::default();
    if unsafe { GetFileInformationByHandle(raw, &mut legacy) } == 0
        || legacy.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (legacy.attributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || (!directory && legacy.links != 1)
        || legacy.volume == 0
        || (legacy.index_high == 0 && legacy.index_low == 0)
    {
        return Err("WAKE_OFFLINE_PIN_IDENTITY_INVALID".into());
    }
    Ok(OfflineObjectIdentityV2 {
        legacy: ObjectIdentityV1 {
            volume_serial: legacy.volume,
            file_index_high: legacy.index_high,
            file_index_low: legacy.index_low,
        },
        full: full_identity_of_handle(raw)?,
    })
}

/// Read the complete ID from an already retained handle after its owner has
/// validated the fixed namespace, object type and final reparse attribute.
/// This helper creates no pin of its own; the caller must keep that handle.
pub(crate) fn full_identity_of_handle(raw: *mut c_void) -> Result<FullFileIdentityV2> {
    let mut full = FileIdInfo::default();
    if unsafe {
        GetFileInformationByHandleEx(
            raw,
            FILE_ID_INFO_CLASS,
            (&raw mut full).cast(),
            std::mem::size_of::<FileIdInfo>() as u32,
        )
    } == 0
        || full.volume_serial == 0
        || full.file_id == [0; 16]
    {
        return Err("WAKE_OFFLINE_PIN_IDENTITY_INVALID".into());
    }
    Ok(FullFileIdentityV2 {
        volume_serial: full.volume_serial,
        file_id_hex: full
            .file_id
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    })
}

/// A single exact opened object. Keeping the value alive retains its handle.
struct OfflineObjectPinV2 {
    file: File,
    identity: OfflineObjectIdentityV2,
    directory: bool,
}

impl OfflineObjectPinV2 {
    fn open(path: &Path, directory: bool) -> Result<Self> {
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
            || path.as_os_str().encode_wide().any(|unit| unit == 0)
        {
            return Err("WAKE_OFFLINE_PIN_PATH_INVALID".into());
        }
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|_| "WAKE_OFFLINE_PIN_OPEN_FAILED")?;
        let identity = inspect(&file, directory)?;
        Ok(Self {
            file,
            identity,
            directory,
        })
    }

    fn matches(&self, expected: &OfflineObjectIdentityV2) -> Result<()> {
        if &self.identity != expected || inspect(&self.file, self.directory)? != self.identity {
            return Err("WAKE_OFFLINE_PIN_OBSERVATION_MISMATCH".into());
        }
        Ok(())
    }
}

fn open_host_lock_under(root: &OfflineObjectPinV2) -> Result<OfflineObjectPinV2> {
    if !root.directory {
        return Err("WAKE_OFFLINE_HOST_ROOT_INVALID".into());
    }
    let mut units: Vec<u16> = HOST_LOCK_LEAF.encode_utf16().collect();
    let mut name = UnicodeString {
        length: (units.len() * 2) as u16,
        maximum_length: (units.len() * 2) as u16,
        buffer: units.as_mut_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: root.file.as_raw_handle(),
        object_name: &mut name,
        attributes: OBJ_CASE_INSENSITIVE,
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
            FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
            0,
        )
    };
    if result < 0 || raw.is_null() || raw == (-1_isize as *mut c_void) {
        return Err("WAKE_OFFLINE_HOST_LOCK_OPEN_FAILED".into());
    }
    let file = unsafe { File::from_raw_handle(raw) };
    let identity = inspect(&file, false)?;
    Ok(OfflineObjectPinV2 {
        file,
        identity,
        directory: false,
    })
}

fn hash_pinned_file(pin: &OfflineObjectPinV2, limit: u64) -> Result<String> {
    let before = pin
        .file
        .metadata()
        .map_err(|_| "WAKE_OFFLINE_PIN_READ_FAILED")?
        .len();
    if before > limit {
        return Err("WAKE_OFFLINE_PIN_FILE_OVERSIZE".into());
    }
    let mut reader = pin
        .file
        .try_clone()
        .map_err(|_| "WAKE_OFFLINE_PIN_READ_FAILED")?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_OFFLINE_PIN_READ_FAILED")?;
    let mut remaining = before;
    let mut sha = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    while remaining != 0 {
        let size = remaining.min(buffer.len() as u64) as usize;
        let read = reader
            .read(&mut buffer[..size])
            .map_err(|_| "WAKE_OFFLINE_PIN_READ_FAILED")?;
        if read == 0 {
            return Err("WAKE_OFFLINE_PIN_CONTENT_CHANGED".into());
        }
        sha.update(&buffer[..read]);
        remaining -= read as u64;
    }
    if reader
        .read(&mut buffer[..1])
        .map_err(|_| "WAKE_OFFLINE_PIN_READ_FAILED")?
        != 0
        || pin
            .file
            .metadata()
            .map_err(|_| "WAKE_OFFLINE_PIN_READ_FAILED")?
            .len()
            != before
    {
        return Err("WAKE_OFFLINE_PIN_CONTENT_CHANGED".into());
    }
    Ok(format!("{:x}", sha.finalize()))
}

/// A path to an old root or an already opened old-root handle. A borrowed
/// handle is duplicated and retained; this does not attest how its caller
/// found the object, its ACL/share provenance, or the completeness of stores.
#[derive(Clone, Copy)]
pub enum OfflineOldStoreRootSelectionV2<'a> {
    Path(&'a Path),
    RetainedHandle(&'a File),
}

/// One independently selected old-store candidate. Expected identities must
/// come from a separately reviewed observation; neither selector is itself a
/// signed inventory of every historical token-specific store.
#[derive(Clone, Copy)]
pub struct OfflineOldStoreLockCandidateV2<'a> {
    pub old_runtime_root: OfflineOldStoreRootSelectionV2<'a>,
    pub expected_root: &'a OfflineObjectIdentityV2,
    pub expected_lock: &'a OfflineObjectIdentityV2,
}

struct PinnedOldStoreLockV2 {
    root_path: Option<PathBuf>,
    root: OfflineObjectPinV2,
    lock: OfflineObjectPinV2,
}

impl PinnedOldStoreLockV2 {
    fn open(candidate: &OfflineOldStoreLockCandidateV2<'_>) -> Result<Self> {
        let (root, root_path) = match candidate.old_runtime_root {
            OfflineOldStoreRootSelectionV2::Path(path) => (
                OfflineObjectPinV2::open(path, true)?,
                Some(path.to_path_buf()),
            ),
            OfflineOldStoreRootSelectionV2::RetainedHandle(file) => {
                let file = file
                    .try_clone()
                    .map_err(|_| "WAKE_OFFLINE_HOST_ROOT_OPEN_FAILED")?;
                let identity = inspect(&file, true)?;
                (
                    OfflineObjectPinV2 {
                        file,
                        identity,
                        directory: true,
                    },
                    None,
                )
            }
        };
        root.matches(candidate.expected_root)?;
        let lock = open_host_lock_under(&root)?;
        lock.matches(candidate.expected_lock)?;
        Ok(Self {
            root_path,
            root,
            lock,
        })
    }

    fn revalidate(&self) -> Result<()> {
        self.root.matches(&self.root.identity)?;
        self.lock.matches(&self.lock.identity)?;
        if let Some(root_path) = &self.root_path {
            let reopened_root = OfflineObjectPinV2::open(root_path, true)?;
            reopened_root.matches(&self.root.identity)?;
            open_host_lock_under(&reopened_root)?.matches(&self.lock.identity)?;
        } else {
            open_host_lock_under(&self.root)?.matches(&self.lock.identity)?;
        }
        Ok(())
    }
}

/// Retains one exclusive Wake host byte-range lock for each distinct physical
/// root/lock pair in a bounded supplied set. Physical aliases are checked and
/// retained, but only one alias acquires a lock. This proves selected locks
/// are held, not complete inventory, owner exit, launch quarantine or install
/// authority. Dropping the set releases only its own locks and read pins.
pub struct OfflineOldStoreLockSetV2 {
    pinned: Vec<PinnedOldStoreLockV2>,
}

impl OfflineOldStoreLockSetV2 {
    pub fn acquire(candidates: &[OfflineOldStoreLockCandidateV2<'_>]) -> Result<Self> {
        if candidates.is_empty() || candidates.len() > MAX_OLD_STORE_CANDIDATES {
            return Err("WAKE_OFFLINE_HOST_LOCK_CANDIDATES_INVALID".into());
        }
        let mut pinned: Vec<PinnedOldStoreLockV2> = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let selected = PinnedOldStoreLockV2::open(candidate)?;
            let existing_lock = pinned
                .iter()
                .find(|earlier| earlier.lock.identity.full == selected.lock.identity.full);
            if let Some(earlier) = existing_lock {
                if earlier.root.identity != selected.root.identity
                    || earlier.lock.identity != selected.lock.identity
                {
                    return Err("WAKE_OFFLINE_HOST_LOCK_ALIAS_CONFLICT".into());
                }
            } else {
                if pinned
                    .iter()
                    .any(|earlier| earlier.root.identity.full == selected.root.identity.full)
                {
                    return Err("WAKE_OFFLINE_HOST_LOCK_ALIAS_CONFLICT".into());
                }
                selected
                    .lock
                    .file
                    .try_lock()
                    .map_err(|_| "WAKE_OFFLINE_HOST_LOCK_BUSY")?;
            }
            pinned.push(selected);
        }
        let result = Self { pinned };
        result.revalidate()?;
        Ok(result)
    }

    pub fn revalidate(&self) -> Result<()> {
        for pin in &self.pinned {
            pin.revalidate()?;
        }
        Ok(())
    }

    /// Require the signed first-generation root/lock role to be among the
    /// retained selected locks. Other candidate completeness is not inferred.
    pub fn require_exact_selected(&self, observed: &OfflineMigrationObservationV2) -> Result<()> {
        self.revalidate()?;
        if self.pinned.iter().any(|pin| {
            pin.root.identity == observed.old_runtime_root
                && pin.lock.identity == observed.old_runtime_lock
        }) {
            Ok(())
        } else {
            Err("WAKE_OFFLINE_HOST_LOCK_ROLE_MISMATCH".into())
        }
    }
}

/// These paths select objects to inspect; only independently reviewed signed
/// full identities may bind their roles. A caller-selected path by itself is
/// not evidence that every historic token-specific store was found.
pub struct OfflineMigrationObjectPathsV2<'a> {
    pub predecessor_pointer: &'a Path,
    pub old_runtime_root: &'a Path,
    pub old_runtime_lock: &'a Path,
    pub old_host_image: &'a Path,
    pub project_registration_object: &'a Path,
    pub target_protected_root: &'a Path,
    pub broker_policy_file: &'a Path,
    pub broker_service_image: &'a Path,
}

/// Eight retained role handles. The fixed-policy diagnostic requires this
/// value for its entire signed-pair comparison and rechecks it afterward.
pub struct OfflineMigrationPinsV2 {
    predecessor_pointer: OfflineObjectPinV2,
    old_runtime_root: OfflineObjectPinV2,
    old_runtime_lock: OfflineObjectPinV2,
    old_host_image: OfflineObjectPinV2,
    project_registration_object: OfflineObjectPinV2,
    target_protected_root: OfflineObjectPinV2,
    broker_policy_file: OfflineObjectPinV2,
    broker_service_image: OfflineObjectPinV2,
}

impl OfflineMigrationPinsV2 {
    pub fn open_read_only(paths: &OfflineMigrationObjectPathsV2<'_>) -> Result<Self> {
        Ok(Self {
            predecessor_pointer: OfflineObjectPinV2::open(paths.predecessor_pointer, false)?,
            old_runtime_root: OfflineObjectPinV2::open(paths.old_runtime_root, true)?,
            old_runtime_lock: OfflineObjectPinV2::open(paths.old_runtime_lock, false)?,
            old_host_image: OfflineObjectPinV2::open(paths.old_host_image, false)?,
            project_registration_object: OfflineObjectPinV2::open(
                paths.project_registration_object,
                false,
            )?,
            target_protected_root: OfflineObjectPinV2::open(paths.target_protected_root, true)?,
            broker_policy_file: OfflineObjectPinV2::open(paths.broker_policy_file, false)?,
            broker_service_image: OfflineObjectPinV2::open(paths.broker_service_image, false)?,
        })
    }

    pub fn revalidate_matching(&self, observed: &OfflineMigrationObservationV2) -> Result<()> {
        self.predecessor_pointer
            .matches(&observed.predecessor_pointer)?;
        if hash_pinned_file(&self.predecessor_pointer, MAX_POINTER_BYTES)?
            != observed.predecessor_pointer_sha256
        {
            return Err("WAKE_OFFLINE_PIN_CONTENT_MISMATCH".into());
        }
        self.old_runtime_root.matches(&observed.old_runtime_root)?;
        self.old_runtime_lock.matches(&observed.old_runtime_lock)?;
        self.old_host_image.matches(&observed.old_host_image)?;
        if hash_pinned_file(&self.old_host_image, MAX_HOST_IMAGE_BYTES)?
            != observed.old_host_image_sha256
        {
            return Err("WAKE_OFFLINE_PIN_CONTENT_MISMATCH".into());
        }
        self.project_registration_object
            .matches(&observed.project_registration_object)?;
        self.target_protected_root
            .matches(&observed.target_protected_root)?;
        self.broker_policy_file
            .matches(&observed.broker_policy_file)?;
        self.broker_service_image
            .matches(&observed.broker_service_image)?;
        self.predecessor_pointer
            .matches(&observed.predecessor_pointer)?;
        self.old_host_image.matches(&observed.old_host_image)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::{fs, path::PathBuf, time::SystemTime};

    fn fixture_root() -> PathBuf {
        let marker = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "catdesk-wake-offline-fileid-{}-{marker}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("fixture root");
        root
    }

    fn old_store_identities(root: &Path) -> (OfflineObjectIdentityV2, OfflineObjectIdentityV2) {
        let pinned_root = OfflineObjectPinV2::open(root, true).expect("old root pin");
        let pinned_lock = open_host_lock_under(&pinned_root).expect("relative host lock pin");
        (pinned_root.identity, pinned_lock.identity)
    }

    fn clean_store_fixture(parent: &Path, old: &Path) {
        fs::remove_file(old.join(HOST_LOCK_LEAF)).expect("clean host lock");
        for child in ["queue", "archive", "deliveries", "turns", "logs"] {
            fs::remove_dir(old.join(child)).expect("clean empty store child");
        }
        fs::remove_dir(old).expect("clean old store");
        fs::remove_dir(parent).expect("clean fixture parent");
    }

    #[test]
    fn retained_exact_old_host_lock_contends_with_real_store_lease_and_releases() {
        let parent = fixture_root();
        let old = parent.join("old");
        let store = Store::open_scoped_for_test(&old, &parent).expect("fixture store");
        fs::write(old.join(HOST_LOCK_LEAF), b"").expect("existing host lock");
        let (root_id, lock_id) = old_store_identities(&old);
        let candidate = OfflineOldStoreLockCandidateV2 {
            old_runtime_root: OfflineOldStoreRootSelectionV2::Path(&old),
            expected_root: &root_id,
            expected_lock: &lock_id,
        };
        let running = store.host_lock().expect("real host lease");
        assert!(OfflineOldStoreLockSetV2::acquire(&[candidate]).is_err());
        drop(running);

        let held = OfflineOldStoreLockSetV2::acquire(&[candidate]).expect("exclusive old lock");
        held.revalidate().expect("selected lock still pinned");
        assert!(store.host_lock().is_err());
        let competing_reader = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(old.join(HOST_LOCK_LEAF))
            .expect("independent read handle");
        assert!(competing_reader.try_lock().is_err());
        drop(competing_reader);
        drop(held);
        let resumed = store.host_lock().expect("old host lock released on drop");
        drop(resumed);
        clean_store_fixture(&parent, &old);
    }

    #[test]
    fn bounded_old_store_set_locks_two_roots_and_deduplicates_only_same_pair() {
        let parent = fixture_root();
        let first = parent.join("first");
        let second = parent.join("second");
        let first_store = Store::open_scoped_for_test(&first, &parent).expect("first store");
        let second_store = Store::open_scoped_for_test(&second, &parent).expect("second store");
        for root in [&first, &second] {
            fs::write(root.join(HOST_LOCK_LEAF), b"").expect("existing host lock");
        }
        let (first_root, first_lock) = old_store_identities(&first);
        let (second_root, second_lock) = old_store_identities(&second);
        assert_ne!(first_lock.full, second_lock.full);
        let retained_first =
            OfflineObjectPinV2::open(&first, true).expect("real first root handle");
        let a = OfflineOldStoreLockCandidateV2 {
            old_runtime_root: OfflineOldStoreRootSelectionV2::RetainedHandle(&retained_first.file),
            expected_root: &first_root,
            expected_lock: &first_lock,
        };
        let b = OfflineOldStoreLockCandidateV2 {
            old_runtime_root: OfflineOldStoreRootSelectionV2::Path(&second),
            expected_root: &second_root,
            expected_lock: &second_lock,
        };
        let a_path = OfflineOldStoreLockCandidateV2 {
            old_runtime_root: OfflineOldStoreRootSelectionV2::Path(&first),
            expected_root: &first_root,
            expected_lock: &first_lock,
        };
        let held = OfflineOldStoreLockSetV2::acquire(&[a, a_path, b]).expect("two distinct locks");
        assert_eq!(held.pinned.len(), 3);
        held.revalidate().expect("every candidate path and ID");
        assert!(first_store.host_lock().is_err());
        assert!(second_store.host_lock().is_err());
        drop(held);
        drop(first_store.host_lock().expect("first released"));
        drop(second_store.host_lock().expect("second released"));
        let wrong_handle_role = OfflineOldStoreLockCandidateV2 {
            old_runtime_root: OfflineOldStoreRootSelectionV2::RetainedHandle(&retained_first.file),
            expected_root: &second_root,
            expected_lock: &first_lock,
        };
        assert_eq!(
            OfflineOldStoreLockSetV2::acquire(&[wrong_handle_role])
                .err()
                .unwrap(),
            "WAKE_OFFLINE_PIN_OBSERVATION_MISMATCH"
        );
        drop(retained_first);
        for root in [&first, &second] {
            fs::remove_file(root.join(HOST_LOCK_LEAF)).expect("clean lock");
            for child in ["queue", "archive", "deliveries", "turns", "logs"] {
                fs::remove_dir(root.join(child)).expect("clean empty store child");
            }
            fs::remove_dir(root).expect("clean store");
        }
        fs::remove_dir(parent).expect("clean fixture parent");
    }

    #[test]
    fn later_busy_store_releases_earlier_lock_and_wrong_role_fails() {
        let parent = fixture_root();
        let first = parent.join("first");
        let second = parent.join("second");
        let first_store = Store::open_scoped_for_test(&first, &parent).expect("first store");
        let second_store = Store::open_scoped_for_test(&second, &parent).expect("second store");
        for root in [&first, &second] {
            fs::write(root.join(HOST_LOCK_LEAF), b"").expect("existing host lock");
        }
        let (first_root, first_lock) = old_store_identities(&first);
        let (second_root, second_lock) = old_store_identities(&second);
        let a = OfflineOldStoreLockCandidateV2 {
            old_runtime_root: OfflineOldStoreRootSelectionV2::Path(&first),
            expected_root: &first_root,
            expected_lock: &first_lock,
        };
        let b = OfflineOldStoreLockCandidateV2 {
            old_runtime_root: OfflineOldStoreRootSelectionV2::Path(&second),
            expected_root: &second_root,
            expected_lock: &second_lock,
        };
        let running_second = second_store.host_lock().expect("second owner");
        assert!(OfflineOldStoreLockSetV2::acquire(&[a, b]).is_err());
        drop(
            first_store
                .host_lock()
                .expect("first acquisition rolled back"),
        );
        drop(running_second);
        let wrong = OfflineOldStoreLockCandidateV2 {
            old_runtime_root: OfflineOldStoreRootSelectionV2::Path(&first),
            expected_root: &first_root,
            expected_lock: &second_lock,
        };
        assert_eq!(
            OfflineOldStoreLockSetV2::acquire(&[wrong]).err().unwrap(),
            "WAKE_OFFLINE_PIN_OBSERVATION_MISMATCH"
        );
        for root in [&first, &second] {
            fs::remove_file(root.join(HOST_LOCK_LEAF)).expect("clean lock");
            for child in ["queue", "archive", "deliveries", "turns", "logs"] {
                fs::remove_dir(root.join(child)).expect("clean empty store child");
            }
            fs::remove_dir(root).expect("clean store");
        }
        fs::remove_dir(parent).expect("clean fixture parent");
    }

    #[test]
    fn hard_linked_lock_object_is_rejected_before_alias_deduplication() {
        let parent = fixture_root();
        let first = parent.join("first");
        let second = parent.join("second");
        fs::create_dir(&first).expect("first root");
        fs::create_dir(&second).expect("second root");
        fs::write(first.join(HOST_LOCK_LEAF), b"").expect("first host lock");
        let (first_root, first_lock) = old_store_identities(&first);
        fs::hard_link(first.join(HOST_LOCK_LEAF), second.join(HOST_LOCK_LEAF))
            .expect("physical lock alias");
        let a = OfflineOldStoreLockCandidateV2 {
            old_runtime_root: OfflineOldStoreRootSelectionV2::Path(&first),
            expected_root: &first_root,
            expected_lock: &first_lock,
        };
        assert_eq!(
            OfflineOldStoreLockSetV2::acquire(&[a]).err().unwrap(),
            "WAKE_OFFLINE_PIN_IDENTITY_INVALID"
        );
        fs::remove_file(second.join(HOST_LOCK_LEAF)).expect("clean second link");
        let fresh = OfflineOldStoreLockSetV2::acquire(&[a]).expect("failed set released first");
        drop(fresh);
        fs::remove_file(first.join(HOST_LOCK_LEAF)).expect("clean first link");
        fs::remove_dir(first).expect("clean first root");
        fs::remove_dir(second).expect("clean second root");
        fs::remove_dir(parent).expect("clean fixture parent");
    }

    #[test]
    fn retained_file_id_info_rejects_role_drift_and_blocks_new_writer() {
        let root = fixture_root();
        let first = root.join("first.json");
        let second = root.join("second.json");
        fs::write(&first, b"first").expect("first");
        fs::write(&second, b"second").expect("second");
        let pin = OfflineObjectPinV2::open(&first, false).expect("full file pin");
        let other = OfflineObjectPinV2::open(&second, false).expect("second full file pin");
        assert_ne!(pin.identity.full, other.identity.full);
        pin.matches(&pin.identity).expect("retained object");
        assert_eq!(
            pin.matches(&other.identity).unwrap_err(),
            "WAKE_OFFLINE_PIN_OBSERVATION_MISMATCH"
        );
        let mut forged = pin.identity.clone();
        forged.full.file_id_hex = other.identity.full.file_id_hex.clone();
        assert_eq!(
            pin.matches(&forged).unwrap_err(),
            "WAKE_OFFLINE_PIN_OBSERVATION_MISMATCH"
        );
        assert!(OpenOptions::new().write(true).open(&first).is_err());
        assert!(fs::remove_file(&first).is_err());
        drop(pin);
        drop(other);
        assert!(OpenOptions::new().write(true).open(&first).is_ok());
        fs::remove_file(first).expect("clean first");
        fs::remove_file(second).expect("clean second");
        fs::remove_dir(root).expect("clean root");
    }

    #[test]
    fn directory_and_invalid_paths_are_refused_as_wrong_roles() {
        let root = fixture_root();
        let child = root.join("child");
        fs::create_dir(&child).expect("child");
        let file = root.join("item");
        fs::write(&file, b"item").expect("item");
        OfflineObjectPinV2::open(&child, true)
            .expect("directory pin")
            .matches(&OfflineObjectPinV2::open(&child, true).unwrap().identity)
            .expect("same directory");
        assert_eq!(
            OfflineObjectPinV2::open(&file, true).err().unwrap(),
            "WAKE_OFFLINE_PIN_IDENTITY_INVALID"
        );
        assert_eq!(
            OfflineObjectPinV2::open(Path::new("relative"), false)
                .err()
                .unwrap(),
            "WAKE_OFFLINE_PIN_PATH_INVALID"
        );
        fs::remove_file(file).expect("clean file");
        fs::remove_dir(child).expect("clean child");
        fs::remove_dir(root).expect("clean root");
    }

    #[test]
    fn eight_role_pins_require_the_exact_full_identity_for_every_observation() {
        let root = fixture_root();
        let old = root.join("old");
        let protected = root.join("protected");
        fs::create_dir(&old).expect("old root");
        fs::create_dir(&protected).expect("protected root");
        let pointer = old.join("current.json");
        let lock = old.join("host.lock");
        let image = root.join("WakeHost.exe");
        let project = root.join("project.json");
        let policy = protected.join("broker-policy.json");
        let service = root.join("broker.exe");
        for path in [&pointer, &lock, &image, &project, &policy, &service] {
            fs::write(path, b"fixture").expect("fixture object");
        }
        let paths = OfflineMigrationObjectPathsV2 {
            predecessor_pointer: &pointer,
            old_runtime_root: &old,
            old_runtime_lock: &lock,
            old_host_image: &image,
            project_registration_object: &project,
            target_protected_root: &protected,
            broker_policy_file: &policy,
            broker_service_image: &service,
        };
        let pins = OfflineMigrationPinsV2::open_read_only(&paths).expect("eight retained pins");
        let mut observed = OfflineMigrationObservationV2 {
            now_utc_seconds: 1,
            predecessor_package_id: "old".into(),
            predecessor_pointer: pins.predecessor_pointer.identity.clone(),
            predecessor_pointer_sha256: format!("{:x}", Sha256::digest(b"fixture")),
            old_runtime_root: pins.old_runtime_root.identity.clone(),
            old_runtime_lock: pins.old_runtime_lock.identity.clone(),
            old_host_image: pins.old_host_image.identity.clone(),
            old_host_image_sha256: format!("{:x}", Sha256::digest(b"fixture")),
            target_conversation_id: String::new(),
            target_generation: 1,
            target_snapshot_sha256: String::new(),
            project_id: String::new(),
            project_registration_object: pins.project_registration_object.identity.clone(),
            project_target_conversation_id: String::new(),
            project_target_generation: 1,
            project_target_snapshot_sha256: String::new(),
            unresolved_events_sha256: String::new(),
            ambiguous_event_disposition:
                crate::install_broker_offline_v2::AmbiguousEventDispositionV2::QuarantinedNoReplay,
            desired_state: crate::install_broker_offline_v2::OfflineDesiredStateV2::Stopped,
            closed_store_inventory_sha256: String::new(),
            quarantined_launch_entrypoints_sha256: String::new(),
            target_protected_root: pins.target_protected_root.identity.clone(),
            broker_policy_file: pins.broker_policy_file.identity.clone(),
            broker_policy_sha256: String::new(),
            broker_service_image: pins.broker_service_image.identity.clone(),
            broker_service_image_sha256: String::new(),
        };
        pins.revalidate_matching(&observed)
            .expect("all eight pinned roles");
        let selected = OfflineOldStoreLockCandidateV2 {
            old_runtime_root: OfflineOldStoreRootSelectionV2::Path(&old),
            expected_root: &observed.old_runtime_root,
            expected_lock: &observed.old_runtime_lock,
        };
        let held = OfflineOldStoreLockSetV2::acquire(&[selected]).expect("selected old lock");
        held.require_exact_selected(&observed)
            .expect("signed old root and lock role selected");
        let mut wrong_selected = observed.clone();
        wrong_selected.old_runtime_lock = observed.predecessor_pointer.clone();
        assert_eq!(
            held.require_exact_selected(&wrong_selected).unwrap_err(),
            "WAKE_OFFLINE_HOST_LOCK_ROLE_MISMATCH"
        );
        drop(held);
        let mut wrong_pointer_hash = observed.clone();
        wrong_pointer_hash.predecessor_pointer_sha256 = "0".repeat(64);
        assert_eq!(
            pins.revalidate_matching(&wrong_pointer_hash).unwrap_err(),
            "WAKE_OFFLINE_PIN_CONTENT_MISMATCH"
        );
        let mut wrong_image_hash = observed.clone();
        wrong_image_hash.old_host_image_sha256 = "0".repeat(64);
        assert_eq!(
            pins.revalidate_matching(&wrong_image_hash).unwrap_err(),
            "WAKE_OFFLINE_PIN_CONTENT_MISMATCH"
        );
        observed.old_runtime_lock.full.file_id_hex =
            pins.predecessor_pointer.identity.full.file_id_hex.clone();
        assert_eq!(
            pins.revalidate_matching(&observed).unwrap_err(),
            "WAKE_OFFLINE_PIN_OBSERVATION_MISMATCH"
        );
        drop(pins);
        fs::write(&pointer, vec![0_u8; (MAX_POINTER_BYTES + 1) as usize])
            .expect("oversize pointer fixture");
        let oversized = OfflineMigrationPinsV2::open_read_only(&paths).expect("oversize pin");
        observed.predecessor_pointer = oversized.predecessor_pointer.identity.clone();
        observed.predecessor_pointer_sha256 = format!(
            "{:x}",
            Sha256::digest(vec![0_u8; (MAX_POINTER_BYTES + 1) as usize])
        );
        observed.old_runtime_lock = oversized.old_runtime_lock.identity.clone();
        assert_eq!(
            oversized.revalidate_matching(&observed).unwrap_err(),
            "WAKE_OFFLINE_PIN_FILE_OVERSIZE"
        );
        drop(oversized);
        for path in [&pointer, &lock, &image, &project, &policy, &service] {
            fs::remove_file(path).expect("clean fixture object");
        }
        fs::remove_dir(old).expect("clean old root");
        fs::remove_dir(protected).expect("clean protected root");
        fs::remove_dir(root).expect("clean fixture root");
    }
}
