//! One marked protected current.json replacement after publication and an
//! independently recorded old-owner stop. This module does not prove either
//! predecessor migration or the stop action, and the service does not call it.

#![cfg(windows)]

use crate::{
    Result,
    install_broker_approval::{ObjectIdentityV1, VerifiedApprovalV1},
    install_broker_durability::{ParentFlushPoint, flush_pinned_directory},
    install_broker_ledger::{BrokerLedgerLease, InstallPhaseV1},
    install_broker_policy::PolicyVerifiedApprovalV1,
    install_broker_preflight::{canonical_protected_security, protected_security_raw},
    install_broker_reader::{
        ProtectedInstallPinV1, ProtectedInstallReadOnlySnapshotV1,
        pin_protected_install_under_guard, plan_protected_pointer_from_stage,
    },
    install_broker_stage_writer::BrokerPublishedReceiptV1,
};
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};

const FILE_CREATE: u32 = 2;
const FILE_READ_DATA: u32 = 1;
const FILE_WRITE_DATA: u32 = 2;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const READ_CONTROL: u32 = 0x0002_0000;
const DELETE: u32 = 0x0001_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_NON_DIRECTORY_FILE: u32 = 0x40;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x20;
const FILE_WRITE_THROUGH: u32 = 2;
const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_RENAME_INFORMATION_CLASS: u32 = 10;
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

#[repr(C)]
struct RenameInformation {
    replace_if_exists: u8,
    reserved: [u8; 7],
    root_directory: *mut c_void,
    file_name_length: u32,
    file_name: [u16; 32],
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
    fn NtSetInformationFile(
        handle: *mut c_void,
        status: *mut IoStatusBlock,
        information: *mut c_void,
        length: u32,
        information_class: u32,
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

fn identity(raw: *mut c_void) -> Result<ObjectIdentityV1> {
    let mut info = FileInfo::default();
    if unsafe { GetFileInformationByHandle(raw, &mut info) } == 0
        || info.attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
        || info.links != 1
        || info.volume == 0
        || (info.index_high == 0 && info.index_low == 0)
    {
        return Err("WAKE_BROKER_POINTER_OBJECT_INVALID".into());
    }
    Ok(ObjectIdentityV1 {
        volume_serial: info.volume,
        file_index_high: info.index_high,
        file_index_low: info.index_low,
    })
}

fn descriptor(service_sid: &str) -> Result<LocalDescriptor> {
    let sddl = canonical_protected_security(service_sid, false)?;
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
        return Err("WAKE_BROKER_POINTER_SECURITY_INVALID".into());
    }
    Ok(LocalDescriptor(raw))
}

fn create_temp(root: &OwnedHandle, generation: u64, service_sid: Option<&str>) -> Result<File> {
    let name = format!("current-{generation:020}.pending.json");
    let mut units = name.encode_utf16().collect::<Vec<_>>();
    let length = u16::try_from(units.len() * 2).map_err(|_| "WAKE_BROKER_POINTER_NAME_INVALID")?;
    let mut unicode = UnicodeString {
        length,
        maximum_length: length,
        buffer: units.as_mut_ptr(),
    };
    let sd = service_sid.map(descriptor).transpose()?;
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: root.as_raw_handle(),
        object_name: &mut unicode,
        attributes: 0x40,
        security_descriptor: sd.as_ref().map_or(ptr::null_mut(), |sd| sd.0),
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
            FILE_READ_DATA
                | FILE_WRITE_DATA
                | FILE_READ_ATTRIBUTES
                | READ_CONTROL
                | DELETE
                | SYNCHRONIZE,
            &mut attributes,
            &mut status,
            ptr::null_mut(),
            0,
            0,
            FILE_CREATE,
            FILE_NON_DIRECTORY_FILE
                | FILE_SYNCHRONOUS_IO_NONALERT
                | FILE_OPEN_REPARSE_POINT
                | FILE_WRITE_THROUGH,
            ptr::null_mut(),
            0,
        )
    };
    if result != 0 || status.status != 0 || raw.is_null() || raw == INVALID_HANDLE_VALUE {
        if !raw.is_null() && raw != INVALID_HANDLE_VALUE {
            drop(unsafe { OwnedHandle::from_raw_handle(raw) });
        }
        return Err("WAKE_BROKER_POINTER_TEMP_CREATE_FAILED".into());
    }
    let file = unsafe { File::from_raw_handle(raw) };
    identity(file.as_raw_handle())?;
    if let Some(service_sid) = service_sid {
        let (owner, actual) = protected_security_raw(file.as_raw_handle())?;
        if owner != "S-1-5-18" || actual != canonical_protected_security(service_sid, false)? {
            return Err("WAKE_BROKER_POINTER_ACL_MISMATCH".into());
        }
    }
    Ok(file)
}

fn replace_current(temp: &File, root: &OwnedHandle) -> Result<()> {
    let units = "current.json".encode_utf16().collect::<Vec<_>>();
    let mut rename = RenameInformation {
        replace_if_exists: 1,
        reserved: [0; 7],
        root_directory: root.as_raw_handle(),
        file_name_length: u32::try_from(units.len() * 2)
            .map_err(|_| "WAKE_BROKER_POINTER_NAME_INVALID")?,
        file_name: [0; 32],
    };
    rename.file_name[..units.len()].copy_from_slice(&units);
    let length =
        u32::try_from(std::mem::offset_of!(RenameInformation, file_name) + units.len() * 2)
            .map_err(|_| "WAKE_BROKER_POINTER_NAME_INVALID")?;
    let mut status = IoStatusBlock {
        status: 0,
        information: 0,
    };
    if unsafe {
        NtSetInformationFile(
            temp.as_raw_handle(),
            &mut status,
            &raw mut rename as *mut c_void,
            length,
            FILE_RENAME_INFORMATION_CLASS,
        )
    } != 0
        || status.status != 0
    {
        return Err("WAKE_BROKER_POINTER_REPLACE_FAILED".into());
    }
    Ok(())
}

enum ReaderMode<'a> {
    Protected(&'a str),
    #[cfg(test)]
    Fixture,
}

impl ReaderMode<'_> {
    fn service_sid(&self) -> Option<&str> {
        match self {
            Self::Protected(value) => Some(value),
            #[cfg(test)]
            Self::Fixture => None,
        }
    }

    fn pin(&self, root: &OwnedHandle, root_id: &ObjectIdentityV1) -> Result<ProtectedInstallPinV1> {
        match self {
            Self::Protected(service_sid) => {
                pin_protected_install_under_guard(root, root_id, service_sid)
            }
            #[cfg(test)]
            Self::Fixture => {
                crate::install_broker_reader::pin_protected_install_fixture(root, root_id)
            }
        }
    }
}

/// A fixed broker's typed continuation after a separate, durable old-owner
/// stop. This routine never creates INTENT, accepts a request key, stops an
/// owner, installs a service, or starts Wake. An uncertain replacement is
/// attention-only; the same lease and a reopened one both refuse replay.
pub fn switch_fixed_protected_pointer(
    lease: &mut BrokerLedgerLease<'_>,
    approved: &PolicyVerifiedApprovalV1<'_>,
    published: &BrokerPublishedReceiptV1<'_>,
) -> Result<ProtectedInstallReadOnlySnapshotV1> {
    if !lease.matches_policy_approval(approved) {
        return Err("WAKE_BROKER_POINTER_APPROVAL_CHANGED".into());
    }
    let (service_sid, _) = approved.policy().checked_endpoint_sids()?;
    switch_with_reader(
        lease,
        approved.verified(),
        published,
        ReaderMode::Protected(service_sid),
    )
}

fn switch_with_reader(
    lease: &mut BrokerLedgerLease<'_>,
    approval: &VerifiedApprovalV1,
    published: &BrokerPublishedReceiptV1<'_>,
    reader: ReaderMode<'_>,
) -> Result<ProtectedInstallReadOnlySnapshotV1> {
    // This explicit gate prevents PUBLISHED from implying owner quiescence.
    if lease.phase() != InstallPhaseV1::OldOwnerStopped || !lease.matches_approval(approval) {
        return Err("WAKE_BROKER_POINTER_OLD_OWNER_STOP_REQUIRED".into());
    }
    let plan = plan_protected_pointer_from_stage(approval, published.stage_binding())?;
    let payload = approval.payload();
    let root_id = &payload.target_protected_root;
    let root = lease.protected_root();
    let predecessor = reader.pin(root, root_id)?;
    lease.record_pointer_switch_attempt(approval, published, &predecessor, plan.sha256())?;
    // The protected reader denies DELETE sharing. Release this pin only after
    // the no-replay marker is durable; another reader may still veto replace.
    drop(predecessor);
    let root = lease.protected_root();
    let mut temp = create_temp(root, payload.install_generation, reader.service_sid())?;
    let moved_identity = identity(temp.as_raw_handle())?;
    temp.write_all(plan.canonical_bytes())
        .and_then(|_| temp.sync_all())
        .map_err(|_| "WAKE_BROKER_POINTER_TEMP_WRITE_FAILED")?;
    temp.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_BROKER_POINTER_TEMP_READBACK_FAILED")?;
    let mut readback = Vec::new();
    (&mut temp)
        .take(plan.canonical_bytes().len() as u64 + 1)
        .read_to_end(&mut readback)
        .map_err(|_| "WAKE_BROKER_POINTER_TEMP_READBACK_FAILED")?;
    if readback != plan.canonical_bytes()
        || format!("{:x}", Sha256::digest(&readback)) != plan.sha256()
    {
        return Err("WAKE_BROKER_POINTER_TEMP_READBACK_FAILED".into());
    }
    flush_pinned_directory(root, root_id, ParentFlushPoint::PointerTempEntry)?;
    replace_current(&temp, root)?;
    flush_pinned_directory(root, root_id, ParentFlushPoint::PointerReplacement)?;
    if identity(temp.as_raw_handle())? != moved_identity {
        return Err("WAKE_BROKER_POINTER_OBJECT_CHANGED".into());
    }
    drop(temp);
    let switched = reader.pin(root, root_id)?;
    if switched.snapshot().pointer != moved_identity {
        return Err("WAKE_BROKER_POINTER_OBJECT_CHANGED".into());
    }
    lease.record_pointer_switched(approval, published, &switched, plan.sha256())?;
    switched.revalidate(lease.protected_root(), root_id)?;
    Ok(switched.snapshot().clone())
}

#[cfg(test)]
pub(crate) fn switch_pointer_fixture(
    lease: &mut BrokerLedgerLease<'_>,
    approval: &VerifiedApprovalV1,
    published: &BrokerPublishedReceiptV1<'_>,
) -> Result<ProtectedInstallReadOnlySnapshotV1> {
    switch_with_reader(lease, approval, published, ReaderMode::Fixture)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        install_broker_durability::fail_next_flush, install_broker_ledger::BrokerLedgerGuard,
        install_broker_reader::pin_protected_install_fixture,
        install_broker_stage_writer::tests::TransactionFixture,
    };

    fn with_published_fixture(
        test: impl FnOnce(
            &TransactionFixture,
            &mut BrokerLedgerLease<'_>,
            &BrokerPublishedReceiptV1<'static>,
        ),
    ) {
        let fixture = TransactionFixture::new_pointer();
        let guard = BrokerLedgerGuard::acquire(fixture.root()).expect("fixture ledger lock");
        let mut lease = guard
            .begin_fixture(fixture.approval(), fixture.expectation())
            .expect("fixture intent");
        let published = fixture.publish_for_pointer(&mut lease);
        assert_eq!(lease.phase(), InstallPhaseV1::Published);
        lease
            .advance(InstallPhaseV1::OldOwnerStopped)
            .expect("test-only stopped-owner prerequisite");
        test(&fixture, &mut lease, &published);
        drop(published);
        drop(lease);
        fixture.cleanup();
    }

    fn marker(fixture: &TransactionFixture) -> std::path::PathBuf {
        fixture.root_path().join(format!(
            "install-transaction-{:020}-pointer-attempt.json",
            fixture.approval().payload().install_generation
        ))
    }

    #[test]
    fn native_switch_keeps_predecessor_and_binds_exact_new_pointer() {
        with_published_fixture(|fixture, lease, published| {
            let before =
                std::fs::read(fixture.root_path().join("current.json")).expect("old pointer bytes");
            let old_package = fixture
                .root_path()
                .join("packages")
                .join(&fixture.approval().payload().predecessor_package_id);
            let old_manifest =
                std::fs::read(old_package.join("manifest.json")).expect("old package manifest");
            let switched = switch_pointer_fixture(lease, fixture.approval(), published)
                .expect("native switch");
            assert_eq!(lease.phase(), InstallPhaseV1::PointerSwitched);
            assert_eq!(
                switched.package_id,
                fixture.approval().payload().manifest.package_id
            );
            assert_eq!(
                switched.predecessor_pointer,
                fixture.approval().payload().predecessor_pointer
            );
            assert_ne!(
                std::fs::read(fixture.root_path().join("current.json")).unwrap(),
                before
            );
            assert_eq!(
                std::fs::read(old_package.join("manifest.json")).unwrap(),
                old_manifest,
                "old package is preserved"
            );
            assert!(marker(fixture).exists());
            let revalidated = pin_protected_install_fixture(
                fixture.root(),
                &fixture.approval().payload().target_protected_root,
            )
            .expect("exact new reader pin");
            assert_eq!(revalidated.snapshot(), &switched);
        });
    }

    #[test]
    fn changed_predecessor_refuses_before_marker_or_temp() {
        with_published_fixture(|fixture, lease, published| {
            let current = fixture.root_path().join("current.json");
            let moved = fixture.root_path().join("old-pointer-fixture.json");
            let bytes = std::fs::read(&current).unwrap();
            std::fs::rename(&current, &moved).expect("move old pointer fixture");
            std::fs::write(&current, bytes).expect("same bytes new physical file");
            assert_eq!(
                switch_pointer_fixture(lease, fixture.approval(), published).unwrap_err(),
                "WAKE_BROKER_POINTER_PREDECESSOR_CHANGED"
            );
            assert!(!marker(fixture).exists());
            assert_eq!(lease.phase(), InstallPhaseV1::OldOwnerStopped);
        });
    }

    #[test]
    fn interrupted_attempt_before_rename_keeps_old_pointer_and_cannot_replay() {
        with_published_fixture(|fixture, lease, published| {
            let current = fixture.root_path().join("current.json");
            let old = std::fs::read(&current).unwrap();
            fail_next_flush(ParentFlushPoint::PointerTempEntry);
            assert_eq!(
                switch_pointer_fixture(lease, fixture.approval(), published).unwrap_err(),
                "WAKE_BROKER_PARENT_FLUSH_FAILED"
            );
            assert_eq!(std::fs::read(&current).unwrap(), old);
            assert!(marker(fixture).exists());
            assert_eq!(
                switch_pointer_fixture(lease, fixture.approval(), published).unwrap_err(),
                "WAKE_BROKER_POINTER_ATTEMPT_ATTENTION"
            );
        });
    }

    #[test]
    fn replacement_without_record_is_sticky_and_preserves_old_package() {
        with_published_fixture(|fixture, lease, published| {
            let current = fixture.root_path().join("current.json");
            let old = std::fs::read(&current).unwrap();
            let old_package = fixture
                .root_path()
                .join("packages")
                .join(&fixture.approval().payload().predecessor_package_id);
            fail_next_flush(ParentFlushPoint::PointerReplacement);
            assert_eq!(
                switch_pointer_fixture(lease, fixture.approval(), published).unwrap_err(),
                "WAKE_BROKER_PARENT_FLUSH_FAILED"
            );
            assert_ne!(std::fs::read(&current).unwrap(), old);
            assert!(old_package.is_dir());
            assert!(marker(fixture).exists());
            assert_eq!(lease.phase(), InstallPhaseV1::OldOwnerStopped);
            assert_eq!(
                switch_pointer_fixture(lease, fixture.approval(), published).unwrap_err(),
                "WAKE_BROKER_POINTER_ATTEMPT_ATTENTION"
            );
        });
    }

    #[test]
    fn independent_reader_vetoes_replace_after_marker_without_deleting_old() {
        with_published_fixture(|fixture, lease, published| {
            let current = fixture.root_path().join("current.json");
            let old = std::fs::read(&current).unwrap();
            let reader = pin_protected_install_fixture(
                fixture.root(),
                &fixture.approval().payload().target_protected_root,
            )
            .expect("independent old pointer pin");
            assert_eq!(
                switch_pointer_fixture(lease, fixture.approval(), published).unwrap_err(),
                "WAKE_BROKER_POINTER_REPLACE_FAILED"
            );
            assert!(marker(fixture).exists());
            assert_eq!(std::fs::read(&current).unwrap(), old);
            drop(reader);
        });
    }

    #[test]
    fn published_phase_alone_cannot_switch_or_mark_an_attempt() {
        let fixture = TransactionFixture::new_pointer();
        let guard = BrokerLedgerGuard::acquire(fixture.root()).unwrap();
        let mut lease = guard
            .begin_fixture(fixture.approval(), fixture.expectation())
            .unwrap();
        let published = fixture.publish_for_pointer(&mut lease);
        assert_eq!(lease.phase(), InstallPhaseV1::Published);
        assert_eq!(
            switch_pointer_fixture(&mut lease, fixture.approval(), &published).unwrap_err(),
            "WAKE_BROKER_POINTER_OLD_OWNER_STOP_REQUIRED"
        );
        assert!(!marker(&fixture).exists());
        drop(published);
        drop(lease);
        fixture.cleanup();
    }

    #[test]
    fn reopened_marked_old_owner_stopped_is_attention_before_and_after_rename() {
        for point in [
            ParentFlushPoint::PointerTempEntry,
            ParentFlushPoint::PointerReplacement,
        ] {
            let fixture = TransactionFixture::new_pointer();
            {
                let guard = BrokerLedgerGuard::acquire(fixture.root()).unwrap();
                let mut lease = guard
                    .begin_fixture(fixture.approval(), fixture.expectation())
                    .unwrap();
                let published = fixture.publish_for_pointer(&mut lease);
                lease.advance(InstallPhaseV1::OldOwnerStopped).unwrap();
                fail_next_flush(point);
                assert_eq!(
                    switch_pointer_fixture(&mut lease, fixture.approval(), &published).unwrap_err(),
                    "WAKE_BROKER_PARENT_FLUSH_FAILED"
                );
                assert!(marker(&fixture).exists());
            }
            let guard = BrokerLedgerGuard::acquire(fixture.root()).unwrap();
            let error = guard
                .begin_fixture(fixture.approval(), fixture.expectation())
                .err()
                .expect("marked phase is not resumable for mutation");
            assert_eq!(error, "WAKE_BROKER_POINTER_ATTEMPT_ATTENTION");
            fixture.cleanup();
        }
    }

    #[test]
    fn completed_protected_phase_requires_its_exact_attempt_marker() {
        for corrupt in [false, true] {
            let fixture = TransactionFixture::new_pointer();
            {
                let guard = BrokerLedgerGuard::acquire(fixture.root()).unwrap();
                let mut lease = guard
                    .begin_fixture(fixture.approval(), fixture.expectation())
                    .unwrap();
                let published = fixture.publish_for_pointer(&mut lease);
                lease.advance(InstallPhaseV1::OldOwnerStopped).unwrap();
                switch_pointer_fixture(&mut lease, fixture.approval(), &published)
                    .expect("fixture switch");
                assert_eq!(lease.phase(), InstallPhaseV1::PointerSwitched);
            }
            if corrupt {
                std::fs::write(marker(&fixture), b"{}").expect("corrupt marker fixture");
            } else {
                std::fs::remove_file(marker(&fixture)).expect("remove marker fixture");
            }
            let guard = BrokerLedgerGuard::acquire(fixture.root()).unwrap();
            let error = guard
                .begin_fixture(fixture.approval(), fixture.expectation())
                .err()
                .expect("completed phase needs its exact marker");
            assert_eq!(
                error,
                if corrupt {
                    "WAKE_LEDGER_POINTER_ATTEMPT_INVALID"
                } else {
                    "WAKE_LEDGER_POINTER_ATTEMPT_MISSING"
                }
            );
            fixture.cleanup();
        }
    }

    #[test]
    fn pointer_phase_parent_flush_failure_reopens_exact_record_without_replacing_again() {
        let fixture = TransactionFixture::new_pointer();
        let original = std::fs::read(fixture.root_path().join("current.json")).unwrap();
        {
            let guard = BrokerLedgerGuard::acquire(fixture.root()).unwrap();
            let mut lease = guard
                .begin_fixture(fixture.approval(), fixture.expectation())
                .unwrap();
            let published = fixture.publish_for_pointer(&mut lease);
            lease.advance(InstallPhaseV1::OldOwnerStopped).unwrap();
            fail_next_flush(ParentFlushPoint::LaterLedgerPhase);
            assert_eq!(
                switch_pointer_fixture(&mut lease, fixture.approval(), &published).unwrap_err(),
                "WAKE_BROKER_PARENT_FLUSH_FAILED"
            );
            assert_eq!(lease.phase(), InstallPhaseV1::OldOwnerStopped);
            assert_ne!(
                std::fs::read(fixture.root_path().join("current.json")).unwrap(),
                original
            );
        }
        let first_id = pin_protected_install_fixture(
            fixture.root(),
            &fixture.approval().payload().target_protected_root,
        )
        .unwrap()
        .snapshot()
        .pointer
        .clone();
        let guard = BrokerLedgerGuard::acquire(fixture.root()).unwrap();
        let resumed = guard
            .begin_fixture(fixture.approval(), fixture.expectation())
            .expect("exact protected pointer record survives failed parent flush");
        assert_eq!(resumed.phase(), InstallPhaseV1::PointerSwitched);
        drop(resumed);
        assert_eq!(
            pin_protected_install_fixture(
                fixture.root(),
                &fixture.approval().payload().target_protected_root,
            )
            .unwrap()
            .snapshot()
            .pointer,
            first_id
        );
        fixture.cleanup();
    }
}
