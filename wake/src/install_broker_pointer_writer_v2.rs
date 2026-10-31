//! Create the first protected V2 current.json without replacing an existing
//! protected pointer. The separate legacy predecessor is never written here.
//! All operational admission/owner/launch gates remain independent.

use super::*;
use crate::{
    install_broker_ledger::OfflineLedgerLeaseV2,
    install_broker_offline_pins::full_identity_of_handle,
    install_broker_offline_v2::OfflineObjectIdentityV2,
    install_broker_reader::{ProtectedInstallPinV2, pin_created_offline_pointer},
    install_broker_stage_writer::BrokerPublishedReceiptV2,
};

fn full_pointer(file: &File) -> Result<OfflineObjectIdentityV2> {
    Ok(OfflineObjectIdentityV2 {
        legacy: identity(file.as_raw_handle())?,
        full: full_identity_of_handle(file.as_raw_handle())?,
    })
}

pub fn create_first_protected_pointer_v2<'p>(
    lease: &mut OfflineLedgerLeaseV2<'_, 'p>,
    published: BrokerPublishedReceiptV2<'p>,
) -> Result<ProtectedInstallPinV2<'p>> {
    if lease.policy().is_none() {
        return Err("WAKE_OFFLINE_POINTER_POLICY_REQUIRED".into());
    }
    create_pointer(lease, published)
}

fn create_pointer<'p>(
    lease: &mut OfflineLedgerLeaseV2<'_, 'p>,
    mut published: BrokerPublishedReceiptV2<'p>,
) -> Result<ProtectedInstallPinV2<'p>> {
    let plan = lease.prepare_initial_pointer_namespace(&mut published)?;
    let sid = lease
        .policy()
        .map(|policy| policy.checked_endpoint_sids().map(|pair| pair.0))
        .transpose()?;
    let mut temp = create_temp(lease.protected_root(), 1, sid)?;
    let expected = full_pointer(&temp)?;
    let expected_security = protected_security_raw(temp.as_raw_handle())?;
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
        || full_pointer(&temp)? != expected
    {
        return Err("WAKE_BROKER_POINTER_TEMP_READBACK_FAILED".into());
    }
    lease.revalidate_initial_pointer_namespace(&plan, &mut published)?;
    lease.flush_initial_pointer_parent(ParentFlushPoint::PointerTempEntry)?;
    #[cfg(test)]
    if let Some(hook) = BEFORE_RENAME.with(|slot| slot.borrow_mut().take()) {
        hook();
    }
    lease.revalidate_initial_pointer_namespace(&plan, &mut published)?;
    rename_current(&temp, lease.protected_root(), false)
        .map_err(|_| "WAKE_OFFLINE_POINTER_PUBLISH_FAILED")?;
    lease.flush_initial_pointer_parent(ParentFlushPoint::PointerReplacement)?;
    if full_pointer(&temp)? != expected {
        return Err("WAKE_OFFLINE_POINTER_OBJECT_CHANGED".into());
    }
    drop(temp);
    #[cfg(test)]
    if let Some(hook) = BEFORE_READBACK.with(|slot| slot.borrow_mut().take()) {
        hook();
    }
    let mut switched =
        pin_created_offline_pointer(lease, published, &plan, &expected, expected_security)?;
    lease.record_initial_pointer_created(&mut switched)?;
    switched.revalidate(lease)?;
    Ok(switched)
}

#[cfg(test)]
pub(crate) fn create_offline_pointer_fixture<'p>(
    lease: &mut OfflineLedgerLeaseV2<'_, 'p>,
    published: BrokerPublishedReceiptV2<'p>,
) -> Result<ProtectedInstallPinV2<'p>> {
    create_pointer(lease, published)
}

#[cfg(test)]
thread_local! {
    static BEFORE_RENAME: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None);
    static BEFORE_READBACK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        install_broker_durability::fail_next_flush,
        install_broker_stage_writer::{
            prepare_offline_recovery_fixture, publish_offline_recovery_fixture,
            reopen_published_after_verified_history, tests::TransactionFixture,
        },
    };
    use std::os::windows::ffi::OsStrExt;

    fn record(fixture: &TransactionFixture, suffix: &str) -> std::path::PathBuf {
        fixture.root_path().join(format!(
            "install-transaction-00000000000000000001-{suffix}.json"
        ))
    }

    fn fingerprint(fixture: &TransactionFixture) -> Vec<(String, Vec<u8>)> {
        [
            "current.json",
            "current-00000000000000000001.pending.json",
            "install-transaction-00000000000000000001-pointer-attempt.json",
            "install-transaction-00000000000000000001-pointer-switched.json",
        ]
        .into_iter()
        .filter_map(|name| {
            std::fs::read(fixture.root_path().join(name))
                .ok()
                .map(|bytes| (name.into(), bytes))
        })
        .collect()
    }

    #[test]
    fn offline_pointer_cutover_creates_exact_pointer_and_retains_reader_without_touching_legacy_fixture()
     {
        let legacy = TransactionFixture::new();
        let legacy_path = legacy.root_path().join("current.json");
        std::fs::write(
            &legacy_path,
            b"separate legacy fixture; not owner authority",
        )
        .unwrap();
        let legacy_id = full_pointer(&File::open(&legacy_path).unwrap()).unwrap();
        let fixture = TransactionFixture::new();
        let (mut lease, staged, _, observed) = prepare_offline_recovery_fixture(&fixture);
        let published = publish_offline_recovery_fixture(&mut lease, staged).unwrap();
        let stage = published.stage_binding().clone();
        let original_published = std::fs::read(record(&fixture, "published")).unwrap();
        let mut pin = create_pointer(&mut lease, published).unwrap();
        assert_eq!(lease.phase(), "POINTER_SWITCHED");
        pin.revalidate(&lease).unwrap();
        assert_eq!(pin.snapshot().stage_binding, stage);
        let current_path = fixture.root_path().join("current.json");
        let current = std::fs::read(&current_path).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&current).unwrap();
        assert_eq!(value["schemaVersion"], 2);
        assert_eq!(value["purpose"], "catdesk-wake-offline-protected-pointer");
        assert_eq!(
            value["predecessorPointerSha256"],
            observed.predecessor_pointer_sha256
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(&current)),
            pin.pointer_binding().pointer_sha256
        );
        assert_eq!(
            full_pointer(&File::open(&current_path).unwrap()).unwrap(),
            pin.pointer_binding().pointer_file
        );
        assert!(
            !fixture
                .root_path()
                .join("current-00000000000000000001.pending.json")
                .exists()
        );
        let completed = std::fs::read(record(&fixture, "pointer-switched")).unwrap();
        assert!(completed.len() <= 8192);
        let data: serde_json::Value = serde_json::from_slice(&completed).unwrap();
        assert_eq!(
            data["pointerBinding"],
            serde_json::to_value(pin.pointer_binding()).unwrap()
        );
        let mut hash = Sha256::new();
        hash.update(b"CatDeskWakeOfflinePointerCreatedV2\0");
        hash.update(&completed);
        assert_eq!(
            lease.pointer_record_sha256(),
            Some(format!("{:x}", hash.finalize()).as_str())
        );
        assert_eq!(
            std::fs::read(record(&fixture, "published")).unwrap(),
            original_published
        );
        assert_eq!(
            full_pointer(&File::open(&legacy_path).unwrap()).unwrap(),
            legacy_id
        );
        assert_eq!(
            std::fs::read(&legacy_path).unwrap(),
            b"separate legacy fixture; not owner authority"
        );
        drop(lease);
        assert!(std::fs::write(&current_path, b"replace").is_err());
        assert!(std::fs::rename(&current_path, fixture.root_path().join("moved.json")).is_err());
        drop(pin);
        fixture.cleanup();
        legacy.cleanup();
    }

    #[test]
    fn offline_pointer_cutover_flush_failures_preserve_residue_and_cannot_replay() {
        for point in [
            ParentFlushPoint::PointerAttempt,
            ParentFlushPoint::PointerTempEntry,
            ParentFlushPoint::PointerReplacement,
            ParentFlushPoint::LaterLedgerPhase,
        ] {
            let fixture = TransactionFixture::new();
            let (mut lease, staged, _, _) = prepare_offline_recovery_fixture(&fixture);
            let published = publish_offline_recovery_fixture(&mut lease, staged).unwrap();
            fail_next_flush(point);
            assert_eq!(
                create_pointer(&mut lease, published).err().unwrap(),
                "WAKE_BROKER_PARENT_FLUSH_FAILED"
            );
            assert_eq!(lease.phase(), "PUBLISHED");
            assert!(lease.pointer_record_sha256().is_none());
            assert_eq!(
                fixture.root_path().join("current.json").exists(),
                matches!(
                    point,
                    ParentFlushPoint::PointerReplacement | ParentFlushPoint::LaterLedgerPhase
                )
            );
            assert_eq!(
                record(&fixture, "pointer-switched").exists(),
                point == ParentFlushPoint::LaterLedgerPhase
            );
            let before = fingerprint(&fixture);
            let again = reopen_published_after_verified_history(&lease, None).unwrap();
            assert_eq!(
                create_pointer(&mut lease, again).err().unwrap(),
                "WAKE_OFFLINE_POINTER_ATTENTION"
            );
            assert_eq!(fingerprint(&fixture), before);
            drop(lease);
            fixture.cleanup();
        }
    }

    #[test]
    fn offline_pointer_cutover_native_collision_after_marker_preserves_foreign_current() {
        let fixture = TransactionFixture::new();
        let (mut lease, staged, _, _) = prepare_offline_recovery_fixture(&fixture);
        let published = publish_offline_recovery_fixture(&mut lease, staged).unwrap();
        let current = fixture.root_path().join("current.json");
        BEFORE_RENAME.with(|slot| {
            *slot.borrow_mut() = Some(Box::new({
                let current = current.clone();
                move || {
                    std::fs::write(current, b"foreign current must survive").unwrap();
                }
            }))
        });
        assert_eq!(
            create_pointer(&mut lease, published).err().unwrap(),
            "WAKE_OFFLINE_POINTER_PUBLISH_FAILED"
        );
        assert_eq!(
            std::fs::read(current).unwrap(),
            b"foreign current must survive"
        );
        assert!(
            fixture
                .root_path()
                .join("current-00000000000000000001.pending.json")
                .exists()
        );
        assert!(!record(&fixture, "pointer-switched").exists());
        drop(lease);
        fixture.cleanup();
    }

    #[test]
    fn offline_pointer_cutover_reader_refuses_replaced_identity_or_changed_bytes() {
        for replace in [false, true] {
            let fixture = TransactionFixture::new();
            let (mut lease, staged, _, _) = prepare_offline_recovery_fixture(&fixture);
            let published = publish_offline_recovery_fixture(&mut lease, staged).unwrap();
            let root = fixture.root_path().to_owned();
            BEFORE_READBACK.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    let current = root.join("current.json");
                    if replace {
                        let bytes = std::fs::read(&current).unwrap();
                        std::fs::rename(&current, root.join("preserved-created-pointer.json"))
                            .unwrap();
                        std::fs::write(current, bytes).unwrap();
                    } else {
                        std::fs::write(current, b"changed pointer bytes").unwrap();
                    }
                }))
            });
            let error = create_pointer(&mut lease, published).err().unwrap();
            assert_eq!(
                error,
                if replace {
                    "WAKE_OFFLINE_POINTER_OBJECT_CHANGED"
                } else {
                    "WAKE_OFFLINE_POINTER_BYTES_MISMATCH"
                }
            );
            assert!(!record(&fixture, "pointer-switched").exists());
            drop(lease);
            fixture.cleanup();
        }
    }

    #[test]
    fn offline_pointer_cutover_torn_marker_vetoes_rename_and_preserves_pending_file() {
        let fixture = TransactionFixture::new();
        let (mut lease, staged, _, _) = prepare_offline_recovery_fixture(&fixture);
        let published = publish_offline_recovery_fixture(&mut lease, staged).unwrap();
        let marker = record(&fixture, "pointer-attempt");
        BEFORE_RENAME.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                std::fs::write(marker, b"torn").unwrap();
            }))
        });
        assert_eq!(
            create_pointer(&mut lease, published).err().unwrap(),
            "WAKE_OFFLINE_POINTER_ATTEMPT_INVALID"
        );
        assert!(!fixture.root_path().join("current.json").exists());
        assert!(
            fixture
                .root_path()
                .join("current-00000000000000000001.pending.json")
                .exists()
        );
        assert!(!record(&fixture, "pointer-switched").exists());
        drop(lease);
        fixture.cleanup();
    }

    #[test]
    fn offline_pointer_cutover_public_entry_requires_policy_and_reader_rejects_other_root() {
        let first = TransactionFixture::new();
        let (mut first_lease, first_stage, _, _) = prepare_offline_recovery_fixture(&first);
        let first_published =
            publish_offline_recovery_fixture(&mut first_lease, first_stage).unwrap();
        assert_eq!(
            create_first_protected_pointer_v2(&mut first_lease, first_published)
                .err()
                .unwrap(),
            "WAKE_OFFLINE_POINTER_POLICY_REQUIRED"
        );
        assert!(!record(&first, "pointer-attempt").exists());
        let first_published = reopen_published_after_verified_history(&first_lease, None).unwrap();
        let mut first_pin = create_pointer(&mut first_lease, first_published).unwrap();
        let second = TransactionFixture::new();
        let (mut second_lease, second_stage, _, _) = prepare_offline_recovery_fixture(&second);
        let second_published =
            publish_offline_recovery_fixture(&mut second_lease, second_stage).unwrap();
        let second_pin = create_pointer(&mut second_lease, second_published).unwrap();
        assert_eq!(
            first_pin.revalidate(&second_lease).unwrap_err(),
            "WAKE_OFFLINE_POINTER_BINDING_MISMATCH"
        );
        first_pin.revalidate(&first_lease).unwrap();
        drop(first_pin);
        drop(second_pin);
        drop(first_lease);
        drop(second_lease);
        first.cleanup();
        second.cleanup();
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn SetFileSecurityW(path: *const u16, information: u32, descriptor: *mut c_void) -> i32;
    }

    #[test]
    fn offline_pointer_cutover_reader_rejects_acl_drift_since_temp_creation() {
        let fixture = TransactionFixture::new();
        let (mut lease, staged, _, _) = prepare_offline_recovery_fixture(&fixture);
        let published = publish_offline_recovery_fixture(&mut lease, staged).unwrap();
        let current = fixture.root_path().join("current.json");
        BEFORE_READBACK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                let text: Vec<u16> = "D:P(A;;FA;;;WD)".encode_utf16().chain([0]).collect();
                let mut raw = ptr::null_mut();
                let mut length = 0;
                assert_ne!(
                    unsafe {
                        ConvertStringSecurityDescriptorToSecurityDescriptorW(
                            text.as_ptr(),
                            1,
                            &mut raw,
                            &mut length,
                        )
                    },
                    0
                );
                let descriptor = LocalDescriptor(raw);
                let wide: Vec<u16> = current.as_os_str().encode_wide().chain([0]).collect();
                assert_ne!(
                    unsafe { SetFileSecurityW(wide.as_ptr(), 4, descriptor.0) },
                    0
                );
            }))
        });
        assert!(create_pointer(&mut lease, published).is_err());
        assert!(!record(&fixture, "pointer-switched").exists());
        drop(lease);
        fixture.cleanup();
    }
}
