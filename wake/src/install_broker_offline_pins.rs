//! Retained, read-only physical object pins for the offline V2 diagnostic.
//!
//! Each handle excludes new write/delete opens and binds both legacy file
//! information and full FILE_ID_INFO. This does not close the historical
//! token-specific store/launcher inventory, exclude an already open writer,
//! prove a byte-range lock is free, or authorize any install phase.

use crate::{
    Result,
    install_broker_approval::ObjectIdentityV1,
    install_broker_offline_v2::{
        FullFileIdentityV2, OfflineMigrationObservationV2, OfflineObjectIdentityV2,
    },
};
use std::{
    ffi::c_void,
    fs::{File, OpenOptions},
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::{Component, Path},
};

const FILE_SHARE_READ: u32 = 1;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_ID_INFO_CLASS: u32 = 18;

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
        self.old_runtime_root.matches(&observed.old_runtime_root)?;
        self.old_runtime_lock.matches(&observed.old_runtime_lock)?;
        self.old_host_image.matches(&observed.old_host_image)?;
        self.project_registration_object
            .matches(&observed.project_registration_object)?;
        self.target_protected_root
            .matches(&observed.target_protected_root)?;
        self.broker_policy_file
            .matches(&observed.broker_policy_file)?;
        self.broker_service_image
            .matches(&observed.broker_service_image)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            predecessor_pointer_sha256: String::new(),
            old_runtime_root: pins.old_runtime_root.identity.clone(),
            old_runtime_lock: pins.old_runtime_lock.identity.clone(),
            old_host_image: pins.old_host_image.identity.clone(),
            old_host_image_sha256: String::new(),
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
        observed.old_runtime_lock.full.file_id_hex =
            pins.predecessor_pointer.identity.full.file_id_hex.clone();
        assert_eq!(
            pins.revalidate_matching(&observed).unwrap_err(),
            "WAKE_OFFLINE_PIN_OBSERVATION_MISMATCH"
        );
        drop(pins);
        for path in [&pointer, &lock, &image, &project, &policy, &service] {
            fs::remove_file(path).expect("clean fixture object");
        }
        fs::remove_dir(old).expect("clean old root");
        fs::remove_dir(protected).expect("clean protected root");
        fs::remove_dir(root).expect("clean fixture root");
    }
}
