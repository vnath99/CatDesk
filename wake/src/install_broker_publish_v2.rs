//! V2 publication through the existing handle-relative no-replace rename.
//! A durable attempt precedes release of stage pins. No pointer or owner
//! transition follows from PUBLISHED; production admission remains held.

use super::*;
use crate::{
    install_broker_reader::directory_inventory,
    install_broker_source::pin_published_candidate_manifest,
};

/// Exact moved bytes retained through PUBLISHED. Serialized object bindings
/// cannot construct this receipt or authorize another publication attempt.
pub struct BrokerPublishedReceiptV2<'p> {
    bytes: PinnedCandidateSourceV1,
    security: StageSecurityMode,
    policy: Option<&'p BrokerProtectedPolicyV1>,
    root_id: OfflineObjectIdentityV2,
    manifest: PackageManifestV1,
    manifest_sha256: String,
    intent_sha256: String,
    binding: StageBindingV2,
    exact_security: [(String, String); 9],
    staging: OwnedHandle,
    packages: OwnedHandle,
    package: OwnedHandle,
    manifest_file: OwnedHandle,
    artifacts: [OwnedHandle; 4],
}

impl BrokerPublishedReceiptV2<'_> {
    pub fn stage_binding(&self) -> &StageBindingV2 {
        &self.binding
    }

    pub(crate) fn binding_for_ledger(
        &mut self,
        root: &OwnedHandle,
        root_id: &OfflineObjectIdentityV2,
        intent_sha256: &str,
        manifest: &PackageManifestV1,
        manifest_sha256: &str,
    ) -> Result<StageBindingV2> {
        if self.root_id != *root_id
            || self.intent_sha256 != intent_sha256
            || self.manifest != *manifest
            || self.manifest_sha256 != manifest_sha256
            || full_object(root.as_raw_handle(), true)? != *root_id
        {
            return Err("WAKE_OFFLINE_PUBLICATION_APPROVAL_MISMATCH".into());
        }
        require_policy(self.policy, &self.security, root_id)?;
        self.bytes.revalidate_manifest(manifest, manifest_sha256)?;
        let now = binding_of(
            &self.staging,
            &self.packages,
            &self.package,
            &self.manifest_file,
            &self.artifacts,
        )?;
        if now != self.binding {
            return Err("WAKE_OFFLINE_PUBLICATION_OBJECT_CHANGED".into());
        }
        let (parent, package, manifest_file, artifacts) = self.bytes.package_pins();
        if full_object(parent.as_raw_handle(), true)? != now.packages_directory
            || full_object(package.as_raw_handle(), true)? != now.candidate_directory
            || full_object(manifest_file.as_raw_handle(), false)? != now.manifest_file
        {
            return Err("WAKE_OFFLINE_PUBLICATION_OBJECT_CHANGED".into());
        }
        for (index, file) in artifacts.iter().enumerate() {
            if full_object(file.as_raw_handle(), false)? != now.artifacts[index] {
                return Err("WAKE_OFFLINE_PUBLICATION_OBJECT_CHANGED".into());
            }
        }
        let handles = [
            root,
            &self.staging,
            &self.packages,
            &self.package,
            &self.manifest_file,
            &self.artifacts[0],
            &self.artifacts[1],
            &self.artifacts[2],
            &self.artifacts[3],
        ];
        for (index, handle) in handles.into_iter().enumerate() {
            require_security(handle, &self.security, index < 4)?;
            if protected_security_raw(handle.as_raw_handle())? != self.exact_security[index] {
                return Err("WAKE_BROKER_PUBLICATION_ACL_MISMATCH".into());
            }
        }
        require_location(&self.staging, &self.packages, &manifest.package_id, true)?;
        require_policy(self.policy, &self.security, root_id)?;
        Ok(now)
    }
}

fn require_policy(
    policy: Option<&BrokerProtectedPolicyV1>,
    security: &StageSecurityMode,
    root: &OfflineObjectIdentityV2,
) -> Result<()> {
    if let Some(policy) = policy {
        policy.revalidate()?;
        let (sid, _) = policy.checked_endpoint_sids()?;
        if security.service_sid() != Some(sid) || policy.readback()?.protected_root != root.legacy {
            return Err("WAKE_BROKER_PUBLICATION_POLICY_CHANGED".into());
        }
    } else {
        #[cfg(not(test))]
        return Err("WAKE_BROKER_PUBLICATION_POLICY_CHANGED".into());
        #[cfg(test)]
        if !matches!(security, StageSecurityMode::Fixture) {
            return Err("WAKE_BROKER_PUBLICATION_POLICY_CHANGED".into());
        }
    }
    Ok(())
}

fn require_location(
    staging: &OwnedHandle,
    packages: &OwnedHandle,
    package_id: &str,
    moved: bool,
) -> Result<()> {
    let stage = directory_inventory(staging, 16)?;
    let packages = directory_inventory(packages, 256)?;
    let expected_stage = if moved {
        Vec::new()
    } else {
        vec![CANDIDATE.to_owned()]
    };
    if stage.into_iter().collect::<Vec<_>>() != expected_stage
        || packages.contains(package_id) != moved
        || packages
            .iter()
            .any(|name| name.eq_ignore_ascii_case(package_id) && name != package_id)
    {
        return Err("WAKE_OFFLINE_PUBLICATION_LOCATION_INVALID".into());
    }
    Ok(())
}

/// Consume one fresh STAGED receipt under the same global transaction lock.
/// Every error after the attempt marker leaves history and bytes intact.
pub fn publish_fixed_candidate_stage_v2<'p>(
    lease: &mut OfflineLedgerLeaseV2<'_, 'p>,
    receipt: BrokerStageReceiptV2<'p>,
) -> Result<BrokerPublishedReceiptV2<'p>> {
    let policy = lease
        .policy()
        .ok_or("WAKE_BROKER_PUBLICATION_POLICY_CHANGED")?;
    if !receipt
        .policy
        .is_some_and(|value| std::ptr::eq(value, policy))
    {
        return Err("WAKE_BROKER_PUBLICATION_POLICY_CHANGED".into());
    }
    publish_pinned_stage(lease, receipt)
}

fn publish_pinned_stage<'p>(
    lease: &mut OfflineLedgerLeaseV2<'_, 'p>,
    mut receipt: BrokerStageReceiptV2<'p>,
) -> Result<BrokerPublishedReceiptV2<'p>> {
    if lease.phase() != "STAGED" {
        return Err("WAKE_OFFLINE_PUBLICATION_ATTENTION".into());
    }
    let root_id = lease.protected_root_identity().clone();
    let intent_sha256 = lease.intent_record_sha256().to_owned();
    let manifest = lease.approved_manifest().clone();
    let manifest_sha256 = lease.approved_manifest_sha256().to_owned();
    let binding = receipt.binding_for_ledger(
        lease.protected_root(),
        &root_id,
        &intent_sha256,
        &manifest,
        &manifest_sha256,
    )?;
    require_location(
        &receipt.staging,
        &receipt.packages,
        &manifest.package_id,
        false,
    )?;
    let handles = [
        lease.protected_root(),
        &receipt.staging,
        &receipt.packages,
        &receipt.candidate,
        &receipt.manifest_file,
        &receipt.artifacts[0],
        &receipt.artifacts[1],
        &receipt.artifacts[2],
        &receipt.artifacts[3],
    ];
    let exact_security: [(String, String); 9] = handles
        .into_iter()
        .map(|handle| protected_security_raw(handle.as_raw_handle()))
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| "WAKE_BROKER_PUBLICATION_ACL_MISMATCH")?;
    let policy = receipt.policy;
    let security = receipt.security.clone();
    // No delete-denying stage pin drops until the attempt is durable.
    lease.record_publication_attempt(&mut receipt)?;
    drop(receipt);
    #[cfg(test)]
    if let Some(hook) = AFTER_RELEASE.with(|slot| slot.borrow_mut().take()) {
        hook();
    }
    lease.revalidate_publication_attempt()?;
    let root = lease.protected_root();
    require_policy(policy, &security, &root_id)?;
    let staging = open_publication_child(root, STAGING, false)?;
    let packages = open_publication_child(root, PACKAGES, false)?;
    if full_object(staging.as_raw_handle(), true)? != binding.staging_directory
        || full_object(packages.as_raw_handle(), true)? != binding.packages_directory
    {
        return Err("WAKE_OFFLINE_PUBLICATION_PARENT_CHANGED".into());
    }
    // The inventory reader needs FILE_LIST_DIRECTORY; retain separate read
    // pins only until this pre-rename location check finishes.
    {
        let stage_read = open_child(root, STAGING, true, false, security.service_sid())?;
        let package_read = open_child(root, PACKAGES, true, false, security.service_sid())?;
        require_location(&stage_read, &package_read, &manifest.package_id, false)?;
    }
    let candidate = open_publication_child(&staging, CANDIDATE, true)?;
    if full_object(candidate.as_raw_handle(), true)? != binding.candidate_directory {
        return Err("WAKE_OFFLINE_PUBLICATION_OBJECT_CHANGED".into());
    }
    for (index, handle) in [root, &staging, &packages, &candidate]
        .into_iter()
        .enumerate()
    {
        require_security(handle, &security, true)?;
        if protected_security_raw(handle.as_raw_handle())? != exact_security[index] {
            return Err("WAKE_BROKER_PUBLICATION_ACL_MISMATCH".into());
        }
    }
    lease.revalidate_publication_attempt()?;
    rename_candidate_noreplace(&candidate, &packages, &manifest.package_id)?;
    if full_object(candidate.as_raw_handle(), true)? != binding.candidate_directory {
        return Err("WAKE_OFFLINE_PUBLICATION_OBJECT_CHANGED".into());
    }
    flush_pinned_directory(
        &staging,
        &binding.staging_directory.legacy,
        ParentFlushPoint::PublicationSourceParent,
    )?;
    flush_pinned_directory(
        &packages,
        &binding.packages_directory.legacy,
        ParentFlushPoint::PublicationDestinationParent,
    )?;
    drop(candidate);
    drop(staging);
    drop(packages);
    lease.revalidate_inputs()?;
    let root = lease.protected_root();
    let bytes = pin_published_candidate_manifest(root, &manifest, &manifest_sha256)?;
    let sid = security.service_sid();
    let staging = open_child(root, STAGING, true, false, sid)?;
    let packages = open_child(root, PACKAGES, true, false, sid)?;
    let package = open_child(&packages, &manifest.package_id, true, false, sid)?;
    let manifest_file = open_child(&package, MANIFEST, false, false, sid)?;
    let artifacts: [OwnedHandle; 4] = ARTIFACTS
        .into_iter()
        .map(|name| open_child(&package, name, false, false, sid))
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| "WAKE_OFFLINE_PUBLICATION_OBJECT_CHANGED")?;
    let mut published = BrokerPublishedReceiptV2 {
        bytes,
        security,
        policy,
        root_id,
        manifest,
        manifest_sha256,
        intent_sha256,
        binding,
        exact_security,
        staging,
        packages,
        package,
        manifest_file,
        artifacts,
    };
    lease.record_published(&mut published)?;
    Ok(published)
}

#[cfg(test)]
thread_local! {
    static AFTER_RELEASE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        install_broker_durability::{RetainedDirectoryWriterV2, fail_next_flush},
        install_broker_ledger::{
            BrokerLedgerGuard, begin_offline_fixture, begin_offline_writer_fixture,
        },
        install_broker_source::pin_fixture_candidate_manifest,
        install_broker_stage_writer::tests::TransactionFixture,
    };
    use std::{os::windows::ffi::OsStrExt, sync::Arc};

    fn staged(
        fixture: &TransactionFixture,
    ) -> (
        OfflineLedgerLeaseV2<'_, 'static>,
        BrokerStageReceiptV2<'static>,
    ) {
        let (pair, observed) = super::super::tests::pair(fixture);
        let writer = Arc::new(
            RetainedDirectoryWriterV2::acquire(fixture.root(), &observed.target_protected_root)
                .unwrap(),
        );
        let mut lease = begin_offline_writer_fixture(
            BrokerLedgerGuard::acquire(fixture.root()).unwrap(),
            &pair,
            &observed,
            writer,
        )
        .unwrap();
        let source = pin_fixture_candidate_manifest(
            fixture.root_path(),
            &pair.approval().manifest,
            &pair.approval().manifest_sha256,
        )
        .unwrap();
        let mut receipt =
            materialize_pinned_stage(&mut lease, source, StageSecurityMode::Fixture, None).unwrap();
        lease.record_staged(&mut receipt).unwrap();
        (lease, receipt)
    }

    fn record_path(fixture: &TransactionFixture, phase: &str) -> std::path::PathBuf {
        fixture.root_path().join(format!(
            "install-transaction-00000000000000000001-{phase}.json"
        ))
    }

    fn hash_record(bytes: &[u8]) -> String {
        let mut hash = Sha256::new();
        hash.update(b"CatDeskWakeOfflineInstallLedgerV2\0");
        hash.update(bytes);
        format!("{:x}", hash.finalize())
    }

    #[test]
    fn offline_publication_moves_once_retains_full_ids_and_preserves_peer_packages() {
        let fixture = TransactionFixture::new();
        let peer = fixture.root_path().join("packages/prior-package");
        std::fs::create_dir(&peer).unwrap();
        std::fs::write(peer.join("preserved.txt"), b"prior package").unwrap();
        let (mut lease, receipt) = staged(&fixture);
        let binding = receipt.stage_binding().clone();
        let package_id = lease.approved_manifest().package_id.clone();
        let intent_sha = lease.intent_record_sha256().to_owned();
        let root_id = lease.protected_root_identity().clone();
        let manifest = lease.approved_manifest().clone();
        let manifest_sha = lease.approved_manifest_sha256().to_owned();
        let mut published = publish_pinned_stage(&mut lease, receipt).unwrap();
        assert_eq!(lease.phase(), "PUBLISHED");
        assert_eq!(published.stage_binding(), &binding);
        assert_eq!(
            published
                .binding_for_ledger(
                    fixture.root(),
                    &root_id,
                    &intent_sha,
                    &manifest,
                    &manifest_sha
                )
                .unwrap(),
            binding
        );
        let package = fixture.root_path().join("packages").join(&package_id);
        assert!(!fixture.root_path().join("staging/candidate").exists());
        assert!(std::fs::write(package.join(MANIFEST), b"replace").is_err());
        assert!(std::fs::rename(&package, fixture.root_path().join("moved")).is_err());
        assert_eq!(
            std::fs::read(peer.join("preserved.txt")).unwrap(),
            b"prior package"
        );
        let staged_bytes = std::fs::read(record_path(&fixture, "staged")).unwrap();
        let marker_bytes = std::fs::read(record_path(&fixture, "publication-attempt")).unwrap();
        let published_bytes = std::fs::read(record_path(&fixture, "published")).unwrap();
        let marker: serde_json::Value = serde_json::from_slice(&marker_bytes).unwrap();
        let final_record: serde_json::Value = serde_json::from_slice(&published_bytes).unwrap();
        assert_eq!(marker["phase"], "PUBLICATION_ATTEMPT");
        assert_eq!(marker["priorRecordSha256"], hash_record(&staged_bytes));
        assert_eq!(final_record["phase"], "PUBLISHED");
        assert_eq!(
            final_record["priorRecordSha256"],
            hash_record(&marker_bytes)
        );
        assert!(published_bytes.len() <= 8192);
        assert!(!String::from_utf8_lossy(&published_bytes).contains("oldHostPid"));
        assert!(!fixture.root_path().join("current.json").exists());
        assert_eq!(
            lease.record_published(&mut published).unwrap_err(),
            "WAKE_OFFLINE_PUBLICATION_ATTENTION"
        );
        drop(lease);
        assert!(
            std::fs::write(package.join(MANIFEST), b"replace").is_err(),
            "receipt retains package after lease drops"
        );
        drop(published);
        fixture.cleanup();
    }

    #[test]
    fn offline_publication_flush_failures_preserve_attempt_and_refuse_reentry() {
        for point in [
            ParentFlushPoint::PublicationAttempt,
            ParentFlushPoint::PublicationSourceParent,
            ParentFlushPoint::PublicationDestinationParent,
            ParentFlushPoint::LaterLedgerPhase,
        ] {
            let fixture = TransactionFixture::new();
            let (mut lease, receipt) = staged(&fixture);
            let package_id = lease.approved_manifest().package_id.clone();
            fail_next_flush(point);
            assert_eq!(
                publish_pinned_stage(&mut lease, receipt).err().unwrap(),
                "WAKE_BROKER_PARENT_FLUSH_FAILED"
            );
            assert_eq!(lease.phase(), "STAGED");
            let marker = record_path(&fixture, "publication-attempt");
            let original = std::fs::read(&marker).unwrap();
            assert_eq!(
                fixture.root_path().join("staging/candidate").exists(),
                point == ParentFlushPoint::PublicationAttempt
            );
            assert_eq!(
                fixture
                    .root_path()
                    .join("packages")
                    .join(package_id)
                    .exists(),
                point != ParentFlushPoint::PublicationAttempt
            );
            assert_eq!(
                record_path(&fixture, "published").exists(),
                point == ParentFlushPoint::LaterLedgerPhase
            );
            assert_eq!(
                materialize_fixed_candidate_stage_v2(&mut lease)
                    .err()
                    .unwrap(),
                "WAKE_OFFLINE_STAGE_ATTENTION"
            );
            drop(lease);
            let (pair, observed) = super::super::tests::pair(&fixture);
            assert_eq!(
                begin_offline_fixture(
                    BrokerLedgerGuard::acquire(fixture.root()).unwrap(),
                    &pair,
                    &observed
                )
                .err()
                .unwrap(),
                "WAKE_OFFLINE_LEDGER_LATER_OR_MIXED_HISTORY"
            );
            assert_eq!(std::fs::read(marker).unwrap(), original);
            fixture.cleanup();
        }
    }

    #[test]
    fn offline_publication_rejects_cross_root_receipt_before_marker() {
        let first = TransactionFixture::new();
        let second = TransactionFixture::new();
        let (first_lease, first_receipt) = staged(&first);
        let (mut second_lease, second_receipt) = staged(&second);
        assert_eq!(
            publish_pinned_stage(&mut second_lease, first_receipt)
                .err()
                .unwrap(),
            "WAKE_OFFLINE_STAGE_APPROVAL_MISMATCH"
        );
        assert!(!record_path(&second, "publication-attempt").exists());
        assert!(first.root_path().join("staging/candidate").exists());
        assert!(second.root_path().join("staging/candidate").exists());
        drop(first_lease);
        drop(second_lease);
        drop(second_receipt);
        first.cleanup();
        second.cleanup();
    }

    #[test]
    fn offline_publication_destination_collision_at_release_preserves_both() {
        let fixture = TransactionFixture::new();
        let (mut lease, receipt) = staged(&fixture);
        let destination = fixture
            .root_path()
            .join("packages")
            .join(&lease.approved_manifest().package_id);
        // The stage receipt holds a no-write-share packages pin. Prepare the
        // collision at the release boundary to exercise the second check.
        AFTER_RELEASE.with(|slot| {
            *slot.borrow_mut() = Some(Box::new({
                let destination = destination.clone();
                move || {
                    std::fs::create_dir(&destination).unwrap();
                    std::fs::write(destination.join("foreign.txt"), b"keep").unwrap();
                }
            }))
        });
        assert_eq!(
            publish_pinned_stage(&mut lease, receipt).err().unwrap(),
            "WAKE_OFFLINE_PUBLICATION_LOCATION_INVALID"
        );
        assert!(record_path(&fixture, "publication-attempt").exists());
        assert!(!record_path(&fixture, "published").exists());
        assert!(fixture.root_path().join("staging/candidate").exists());
        assert_eq!(
            std::fs::read(destination.join("foreign.txt")).unwrap(),
            b"keep"
        );
        drop(lease);
        fixture.cleanup();
    }

    #[test]
    fn offline_publication_reopened_full_identity_prevents_candidate_substitution() {
        let fixture = TransactionFixture::new();
        let (mut lease, receipt) = staged(&fixture);
        let root = fixture.root_path().to_owned();
        AFTER_RELEASE.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                std::fs::rename(
                    root.join("staging/candidate"),
                    root.join("packages/preserved-original"),
                )
                .unwrap();
                std::fs::create_dir(root.join("staging/candidate")).unwrap();
            }))
        });
        assert_eq!(
            publish_pinned_stage(&mut lease, receipt).err().unwrap(),
            "WAKE_OFFLINE_PUBLICATION_OBJECT_CHANGED"
        );
        assert!(
            fixture
                .root_path()
                .join("packages/preserved-original/manifest.json")
                .exists()
        );
        assert!(!record_path(&fixture, "published").exists());
        drop(lease);
        fixture.cleanup();
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn SetFileSecurityW(path: *const u16, information: u32, descriptor: *mut c_void) -> i32;
    }

    #[test]
    fn offline_publication_existing_destination_refuses_before_marker() {
        let fixture = TransactionFixture::new();
        let destination = fixture
            .root_path()
            .join("packages")
            .join(&fixture.approval().payload().manifest.package_id);
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("foreign.txt"), b"keep").unwrap();
        let (mut lease, receipt) = staged(&fixture);
        assert_eq!(
            publish_pinned_stage(&mut lease, receipt).err().unwrap(),
            "WAKE_OFFLINE_PUBLICATION_LOCATION_INVALID"
        );
        assert!(!record_path(&fixture, "publication-attempt").exists());
        assert!(fixture.root_path().join("staging/candidate").exists());
        assert_eq!(
            std::fs::read(destination.join("foreign.txt")).unwrap(),
            b"keep"
        );
        drop(lease);
        fixture.cleanup();
    }

    #[test]
    fn offline_publication_tampered_attempt_refuses_before_rename() {
        let fixture = TransactionFixture::new();
        let (mut lease, receipt) = staged(&fixture);
        let marker = record_path(&fixture, "publication-attempt");
        AFTER_RELEASE.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                std::fs::write(marker, b"torn marker").unwrap();
            }))
        });
        assert_eq!(
            publish_pinned_stage(&mut lease, receipt).err().unwrap(),
            "WAKE_OFFLINE_LEDGER_RECORD_INVALID"
        );
        assert!(fixture.root_path().join("staging/candidate").exists());
        assert!(!record_path(&fixture, "published").exists());
        drop(lease);
        fixture.cleanup();
    }

    #[test]
    fn offline_publication_checks_candidate_acl_after_pins_release_before_rename() {
        let fixture = TransactionFixture::new();
        let (mut lease, receipt) = staged(&fixture);
        let candidate = fixture.root_path().join("staging/candidate");
        AFTER_RELEASE.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                let text: Vec<u16> = "D:P(A;;FA;;;WD)".encode_utf16().chain([0]).collect();
                let mut raw = ptr::null_mut();
                let mut size = 0;
                assert_ne!(
                    unsafe {
                        ConvertStringSecurityDescriptorToSecurityDescriptorW(
                            text.as_ptr(),
                            1,
                            &mut raw,
                            &mut size,
                        )
                    },
                    0
                );
                let descriptor = LocalDescriptor(raw);
                let wide: Vec<u16> = candidate.as_os_str().encode_wide().chain([0]).collect();
                assert_ne!(
                    unsafe { SetFileSecurityW(wide.as_ptr(), 4, descriptor.0) },
                    0
                );
            }))
        });
        assert_eq!(
            publish_pinned_stage(&mut lease, receipt).err().unwrap(),
            "WAKE_BROKER_PUBLICATION_ACL_MISMATCH"
        );
        assert!(fixture.root_path().join("staging/candidate").exists());
        assert!(record_path(&fixture, "publication-attempt").exists());
        assert!(!record_path(&fixture, "published").exists());
        drop(lease);
        fixture.cleanup();
    }
}
