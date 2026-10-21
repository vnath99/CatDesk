//! V2 materialization through the existing fixed-name stage primitives.
//! No V1 approval or live-owner fields are fabricated. Production cannot
//! obtain a V2 lease while its independent admission gates remain held.

use super::*;
use crate::{
    install_broker_approval::PackageManifestV1,
    install_broker_ledger::{OfflineLedgerLeaseV2, StageBindingV2},
    install_broker_offline_pins::full_identity_of_handle,
    install_broker_offline_v2::OfflineObjectIdentityV2,
    install_broker_source::{pin_fixed_candidate_manifest, pin_staged_candidate_manifest},
};

fn full_object(raw: *mut c_void, directory: bool) -> Result<OfflineObjectIdentityV2> {
    Ok(OfflineObjectIdentityV2 {
        legacy: identity(raw, directory)?,
        full: full_identity_of_handle(raw)?,
    })
}

/// Freshly materialized bytes and retained exact objects. Binding data alone
/// cannot construct this value or authorize a STAGED record.
pub struct BrokerStageReceiptV2<'p> {
    source: PinnedCandidateSourceV1,
    staged: PinnedCandidateSourceV1,
    security: StageSecurityMode,
    policy: Option<&'p BrokerProtectedPolicyV1>,
    root_id: OfflineObjectIdentityV2,
    manifest: PackageManifestV1,
    manifest_sha256: String,
    intent_sha256: String,
    binding: StageBindingV2,
    staging: OwnedHandle,
    packages: OwnedHandle,
    candidate: OwnedHandle,
    manifest_file: OwnedHandle,
    artifacts: [OwnedHandle; 4],
}

impl BrokerStageReceiptV2<'_> {
    /// Object data is a readback, not a serializable installation capability.
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
            return Err("WAKE_OFFLINE_STAGE_APPROVAL_MISMATCH".into());
        }
        if let Some(policy) = self.policy {
            policy.revalidate()?;
            let (sid, _) = policy.checked_endpoint_sids()?;
            if self.security.service_sid() != Some(sid)
                || policy.readback()?.protected_root != root_id.legacy
            {
                return Err("WAKE_BROKER_STAGE_POLICY_CHANGED".into());
            }
        } else {
            #[cfg(not(test))]
            return Err("WAKE_BROKER_STAGE_POLICY_CHANGED".into());
            #[cfg(test)]
            if !matches!(self.security, StageSecurityMode::Fixture) {
                return Err("WAKE_BROKER_STAGE_POLICY_CHANGED".into());
            }
        }
        self.source.revalidate_manifest(manifest, manifest_sha256)?;
        self.staged.revalidate_manifest(manifest, manifest_sha256)?;
        let now = binding_of(
            &self.staging,
            &self.packages,
            &self.candidate,
            &self.manifest_file,
            &self.artifacts,
        )?;
        if now != self.binding {
            return Err("WAKE_OFFLINE_STAGE_OBJECT_CHANGED".into());
        }
        let (parent, candidate, manifest_file, artifacts) = self.staged.package_pins();
        if full_object(parent.as_raw_handle(), true)? != now.staging_directory
            || full_object(candidate.as_raw_handle(), true)? != now.candidate_directory
            || full_object(manifest_file.as_raw_handle(), false)? != now.manifest_file
        {
            return Err("WAKE_OFFLINE_STAGE_OBJECT_CHANGED".into());
        }
        for (index, file) in artifacts.iter().enumerate() {
            if full_object(file.as_raw_handle(), false)? != now.artifacts[index] {
                return Err("WAKE_OFFLINE_STAGE_OBJECT_CHANGED".into());
            }
        }
        require_security(root, &self.security, true)?;
        for directory in [&self.staging, &self.packages, &self.candidate] {
            require_security(directory, &self.security, true)?;
        }
        require_security(&self.manifest_file, &self.security, false)?;
        for artifact in &self.artifacts {
            require_security(artifact, &self.security, false)?;
        }
        if let Some(policy) = self.policy {
            policy.revalidate()?;
        }
        Ok(now)
    }
}

fn require_security(handle: &OwnedHandle, mode: &StageSecurityMode, directory: bool) -> Result<()> {
    if let Some(sid) = mode.service_sid() {
        let (owner, actual) = protected_security_raw(handle.as_raw_handle())?;
        if owner != "S-1-5-18" || actual != canonical_protected_security(sid, directory)? {
            return Err("WAKE_BROKER_STAGE_ACL_MISMATCH".into());
        }
    }
    Ok(())
}

fn binding_of(
    staging: &OwnedHandle,
    packages: &OwnedHandle,
    candidate: &OwnedHandle,
    manifest: &OwnedHandle,
    artifacts: &[OwnedHandle; 4],
) -> Result<StageBindingV2> {
    let artifacts: [OfflineObjectIdentityV2; 4] = artifacts
        .iter()
        .map(|file| full_object(file.as_raw_handle(), false))
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| "WAKE_OFFLINE_STAGE_OBJECT_CHANGED")?;
    Ok(StageBindingV2 {
        staging_directory: full_object(staging.as_raw_handle(), true)?,
        packages_directory: full_object(packages.as_raw_handle(), true)?,
        candidate_directory: full_object(candidate.as_raw_handle(), true)?,
        manifest_file: full_object(manifest.as_raw_handle(), false)?,
        artifacts,
    })
}

pub fn materialize_fixed_candidate_stage_v2<'p>(
    lease: &mut OfflineLedgerLeaseV2<'_, 'p>,
) -> Result<BrokerStageReceiptV2<'p>> {
    if lease.phase() != "INTENT" {
        return Err("WAKE_OFFLINE_STAGE_ATTENTION".into());
    }
    let policy = lease.policy().ok_or("WAKE_BROKER_STAGE_POLICY_CHANGED")?;
    lease.revalidate_inputs()?;
    let (sid, _) = policy.checked_endpoint_sids()?;
    let source =
        pin_fixed_candidate_manifest(lease.approved_manifest(), lease.approved_manifest_sha256())?;
    let mut receipt = materialize_pinned_stage(
        lease,
        source,
        StageSecurityMode::Protected(sid.into()),
        Some(policy),
    )?;
    lease.record_staged(&mut receipt)?;
    Ok(receipt)
}

fn materialize_pinned_stage<'p>(
    lease: &mut OfflineLedgerLeaseV2<'_, 'p>,
    mut source: PinnedCandidateSourceV1,
    security: StageSecurityMode,
    policy: Option<&'p BrokerProtectedPolicyV1>,
) -> Result<BrokerStageReceiptV2<'p>> {
    if lease.phase() != "INTENT" {
        return Err("WAKE_OFFLINE_STAGE_ATTENTION".into());
    }
    lease.revalidate_inputs()?;
    if policy.is_none() {
        #[cfg(not(test))]
        return Err("WAKE_BROKER_STAGE_POLICY_CHANGED".into());
        #[cfg(test)]
        if !matches!(security, StageSecurityMode::Fixture) {
            return Err("WAKE_BROKER_STAGE_POLICY_CHANGED".into());
        }
    }
    let manifest = lease.approved_manifest().clone();
    let manifest_sha256 = lease.approved_manifest_sha256().to_owned();
    let root_id = lease.protected_root_identity().clone();
    let intent_sha256 = lease.intent_record_sha256().to_owned();
    source.revalidate_manifest(&manifest, &manifest_sha256)?;
    let sid = security.service_sid();
    let (staging, packages, staging_id, packages_id) = {
        let root = lease.protected_root();
        security.require_empty(root, &root_id.legacy)?;
        require_security(root, &security, true)?;
        let staging = open_child(root, STAGING, true, false, sid)?;
        let packages = open_child(root, PACKAGES, true, false, sid)?;
        require_security(&staging, &security, true)?;
        require_security(&packages, &security, true)?;
        let staging_id = full_object(staging.as_raw_handle(), true)?;
        let packages_id = full_object(packages.as_raw_handle(), true)?;
        if staging_id.full == packages_id.full
            || staging_id.full == root_id.full
            || packages_id.full == root_id.full
            || staging_id.full.volume_serial != root_id.full.volume_serial
            || packages_id.full.volume_serial != root_id.full.volume_serial
        {
            return Err("WAKE_OFFLINE_STAGE_OBJECT_CHANGED".into());
        }
        (staging, packages, staging_id, packages_id)
    };
    lease.record_stage_started()?;
    let created = open_child(&staging, CANDIDATE, true, true, sid)?;
    let created_id = full_object(created.as_raw_handle(), true)?;
    flush_pinned_directory(
        &staging,
        &staging_id.legacy,
        ParentFlushPoint::StageCandidateEntry,
    )?;
    for (index, name) in ARTIFACTS.into_iter().enumerate() {
        let artifact = &manifest.artifacts[index];
        copy_artifact(
            &created,
            name,
            source.artifact_mut(index),
            artifact.byte_length,
            &artifact.sha256,
            sid,
        )?;
        #[cfg(test)]
        if FAIL_AFTER_ARTIFACT.with(|failure| {
            if failure.get() == Some(index) {
                failure.set(None);
                true
            } else {
                false
            }
        }) {
            return Err("WAKE_BROKER_STAGE_FAULT_INJECTED".into());
        }
    }
    write_blob(&created, MANIFEST, source.manifest_bytes(), sid)?;
    flush_pinned_directory(
        &created,
        &created_id.legacy,
        ParentFlushPoint::StageLeafEntries,
    )?;
    drop(created);
    drop(staging);
    source.revalidate_manifest(&manifest, &manifest_sha256)?;
    lease.revalidate_inputs()?;
    let root = lease.protected_root();
    let staged = pin_staged_candidate_manifest(root, &manifest, &manifest_sha256)?;
    let staging = open_child(root, STAGING, true, false, sid)?;
    let candidate = open_child(&staging, CANDIDATE, true, false, sid)?;
    let manifest_file = open_child(&candidate, MANIFEST, false, false, sid)?;
    let artifacts: [OwnedHandle; 4] = ARTIFACTS
        .into_iter()
        .map(|name| open_child(&candidate, name, false, false, sid))
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| "WAKE_BROKER_STAGE_INVENTORY_MISMATCH")?;
    let binding = binding_of(&staging, &packages, &candidate, &manifest_file, &artifacts)?;
    if binding.staging_directory != staging_id
        || binding.packages_directory != packages_id
        || binding.candidate_directory != created_id
    {
        return Err("WAKE_OFFLINE_STAGE_OBJECT_CHANGED".into());
    }
    let receipt = BrokerStageReceiptV2 {
        source,
        staged,
        security,
        policy,
        root_id,
        manifest,
        manifest_sha256,
        intent_sha256,
        binding,
        staging,
        packages,
        candidate,
        manifest_file,
        artifacts,
    };
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        install_broker_durability::fail_next_flush,
        install_broker_ledger::{BrokerLedgerGuard, begin_offline_fixture},
        install_broker_offline_v2::{
            CheckedOfflinePairV2, OfflineMigrationObservationV2, check_offline_pair,
            tests as signed_test,
        },
        install_broker_source::pin_fixture_candidate_manifest,
        install_broker_stage_writer::tests::TransactionFixture,
    };

    fn pair(fixture: &TransactionFixture) -> (CheckedOfflinePairV2, OfflineMigrationObservationV2) {
        let (mut anchor, mut approval, mut observed, key) = signed_test::fixture();
        let root = full_object(fixture.root().as_raw_handle(), true).unwrap();
        let manifest = fixture.approval().payload().manifest.clone();
        let digest = fixture.approval().payload().manifest_sha256.clone();
        anchor.target_protected_root = root.clone();
        anchor.candidate_manifest_sha256 = digest.clone();
        approval.target_protected_root = root.clone();
        approval.manifest = manifest;
        approval.manifest_sha256 = digest;
        observed.target_protected_root = root;
        let (anchor_wire, approval_wire) = signed_test::signed_pair(&anchor, &approval, &key);
        let checked = check_offline_pair(
            &anchor_wire,
            &approval_wire,
            &key.verifying_key(),
            &observed,
        )
        .unwrap();
        (checked, observed)
    }

    fn source(
        fixture: &TransactionFixture,
        pair: &CheckedOfflinePairV2,
    ) -> PinnedCandidateSourceV1 {
        pin_fixture_candidate_manifest(
            fixture.root_path(),
            &pair.approval().manifest,
            &pair.approval().manifest_sha256,
        )
        .unwrap()
    }

    fn record_path(fixture: &TransactionFixture, phase: &str) -> std::path::PathBuf {
        fixture.root_path().join(format!(
            "install-transaction-00000000000000000001-{phase}.json"
        ))
    }

    #[test]
    fn retained_writer_supports_all_root_flushes_through_staged_and_lease_owns_it() {
        use crate::{
            install_broker_durability::RetainedDirectoryWriterV2,
            install_broker_ledger::begin_offline_writer_fixture,
        };
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt, sync::Arc};
        let fixture = TransactionFixture::new();
        let (pair, observed) = pair(&fixture);
        let write_open = || {
            OpenOptions::new()
                .write(true)
                .share_mode(3)
                .custom_flags(0x0200_0000 | FILE_OPEN_REPARSE_POINT)
                .open(fixture.root_path())
        };
        drop(write_open().expect("positive write control"));
        let writer = Arc::new(
            RetainedDirectoryWriterV2::acquire(fixture.root(), &observed.target_protected_root)
                .unwrap(),
        );
        let mut lease = begin_offline_writer_fixture(
            BrokerLedgerGuard::acquire(fixture.root()).unwrap(),
            &pair,
            &observed,
            Arc::clone(&writer),
        )
        .unwrap();
        drop(writer);
        assert!(write_open().is_err(), "lease retains exclusive writer");
        let mut receipt = materialize_pinned_stage(
            &mut lease,
            source(&fixture, &pair),
            StageSecurityMode::Fixture,
            None,
        )
        .unwrap();
        lease.record_staged(&mut receipt).unwrap();
        assert_eq!(lease.phase(), "STAGED");
        assert!(write_open().is_err());
        drop(lease);
        drop(write_open().expect("root writer releases with lease"));
        drop(receipt);
        fixture.cleanup();
    }

    #[test]
    fn offline_stage_copies_exact_bytes_retains_full_ids_and_records_staged_once() {
        let fixture = TransactionFixture::new();
        let (pair, observed) = pair(&fixture);
        let mut lease = begin_offline_fixture(
            BrokerLedgerGuard::acquire(fixture.root()).unwrap(),
            &pair,
            &observed,
        )
        .unwrap();
        let mut receipt = materialize_pinned_stage(
            &mut lease,
            source(&fixture, &pair),
            StageSecurityMode::Fixture,
            None,
        )
        .unwrap();
        assert_eq!(lease.phase(), "STAGE_STARTED");
        let binding = receipt.stage_binding().clone();
        assert_eq!(binding.candidate_directory.full.file_id_hex.len(), 32);
        let directory = fixture.root_path().join("staging/candidate");
        assert_eq!(
            std::fs::read(directory.join(MANIFEST)).unwrap(),
            serde_json::to_vec(&pair.approval().manifest).unwrap()
        );
        for (index, name) in ARTIFACTS.iter().enumerate() {
            let bytes = std::fs::read(directory.join(name)).unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                pair.approval().manifest.artifacts[index].sha256
            );
            assert!(std::fs::write(directory.join(name), b"replace").is_err());
        }
        assert!(std::fs::rename(&directory, fixture.root_path().join("moved")).is_err());
        assert!(lease.record_staged(&mut receipt).unwrap());
        assert_eq!(lease.phase(), "STAGED");
        assert_eq!(
            materialize_fixed_candidate_stage_v2(&mut lease)
                .err()
                .unwrap(),
            "WAKE_OFFLINE_STAGE_ATTENTION"
        );
        assert!(!lease.record_staged(&mut receipt).unwrap());
        let journal = std::fs::read(record_path(&fixture, "staged")).unwrap();
        assert!(journal.len() <= 8192);
        let record: serde_json::Value = serde_json::from_slice(&journal).unwrap();
        assert_eq!(record["phase"], "STAGED");
        assert_eq!(
            serde_json::from_value::<StageBindingV2>(record["stageBinding"].clone()).unwrap(),
            binding
        );
        assert!(!String::from_utf8_lossy(&journal).contains("oldHostPid"));
        drop(receipt);
        drop(lease);
        let intent: serde_json::Value =
            serde_json::from_slice(&std::fs::read(record_path(&fixture, "intent")).unwrap())
                .unwrap();
        assert!(
            intent.get("stageBinding").is_none(),
            "old INTENT canonical shape preserved"
        );
        fixture.cleanup();
    }

    #[test]
    fn partial_offline_copy_is_sticky_attention_without_staged_or_retry() {
        let fixture = TransactionFixture::new();
        let (pair, observed) = pair(&fixture);
        let mut lease = begin_offline_fixture(
            BrokerLedgerGuard::acquire(fixture.root()).unwrap(),
            &pair,
            &observed,
        )
        .unwrap();
        FAIL_AFTER_ARTIFACT.with(|failure| failure.set(Some(1)));
        assert_eq!(
            materialize_pinned_stage(
                &mut lease,
                source(&fixture, &pair),
                StageSecurityMode::Fixture,
                None
            )
            .err()
            .unwrap(),
            "WAKE_BROKER_STAGE_FAULT_INJECTED"
        );
        assert_eq!(lease.phase(), "STAGE_STARTED");
        let directory = fixture.root_path().join("staging/candidate");
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2);
        assert!(!record_path(&fixture, "staged").exists());
        assert_eq!(
            materialize_pinned_stage(
                &mut lease,
                source(&fixture, &pair),
                StageSecurityMode::Fixture,
                None
            )
            .err()
            .unwrap(),
            "WAKE_OFFLINE_STAGE_ATTENTION"
        );
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2);
        drop(lease);
        assert_eq!(
            begin_offline_fixture(
                BrokerLedgerGuard::acquire(fixture.root()).unwrap(),
                &pair,
                &observed
            )
            .err()
            .unwrap(),
            "WAKE_OFFLINE_STAGE_ATTENTION"
        );
        fixture.cleanup();
    }

    #[test]
    fn offline_stage_parent_flush_failures_never_authorize_another_copy() {
        for point in [
            ParentFlushPoint::StageStarted,
            ParentFlushPoint::StageCandidateEntry,
            ParentFlushPoint::StageLeafEntries,
        ] {
            let fixture = TransactionFixture::new();
            let (pair, observed) = pair(&fixture);
            let mut lease = begin_offline_fixture(
                BrokerLedgerGuard::acquire(fixture.root()).unwrap(),
                &pair,
                &observed,
            )
            .unwrap();
            fail_next_flush(point);
            assert!(
                materialize_pinned_stage(
                    &mut lease,
                    source(&fixture, &pair),
                    StageSecurityMode::Fixture,
                    None
                )
                .is_err()
            );
            assert_eq!(
                fixture.root_path().join("staging/candidate").exists(),
                point != ParentFlushPoint::StageStarted
            );
            assert!(!record_path(&fixture, "staged").exists());
            assert_eq!(
                materialize_pinned_stage(
                    &mut lease,
                    source(&fixture, &pair),
                    StageSecurityMode::Fixture,
                    None
                )
                .err()
                .unwrap(),
                "WAKE_OFFLINE_STAGE_ATTENTION"
            );
            drop(lease);
            fixture.cleanup();
        }
    }

    #[test]
    fn retained_offline_receipt_finishes_only_exact_staged_parent_flush() {
        let fixture = TransactionFixture::new();
        let (pair, observed) = pair(&fixture);
        let mut lease = begin_offline_fixture(
            BrokerLedgerGuard::acquire(fixture.root()).unwrap(),
            &pair,
            &observed,
        )
        .unwrap();
        let mut receipt = materialize_pinned_stage(
            &mut lease,
            source(&fixture, &pair),
            StageSecurityMode::Fixture,
            None,
        )
        .unwrap();
        fail_next_flush(ParentFlushPoint::Staged);
        assert!(lease.record_staged(&mut receipt).is_err());
        assert_eq!(lease.phase(), "STAGE_STARTED");
        let before = std::fs::read(record_path(&fixture, "staged")).unwrap();
        assert!(lease.record_staged(&mut receipt).unwrap());
        assert_eq!(
            std::fs::read(record_path(&fixture, "staged")).unwrap(),
            before
        );
        drop(receipt);
        drop(lease);
        // Re-admission of later V2 phases is not implemented; it must not
        // silently perform another stage create from an INTENT-only reader.
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
        fixture.cleanup();
    }

    #[test]
    fn offline_stage_receipt_cannot_be_spliced_between_transaction_roots() {
        let first = TransactionFixture::new();
        let second = TransactionFixture::new();
        let (first_pair, first_observed) = pair(&first);
        let (second_pair, second_observed) = pair(&second);
        let mut first_lease = begin_offline_fixture(
            BrokerLedgerGuard::acquire(first.root()).unwrap(),
            &first_pair,
            &first_observed,
        )
        .unwrap();
        let mut second_lease = begin_offline_fixture(
            BrokerLedgerGuard::acquire(second.root()).unwrap(),
            &second_pair,
            &second_observed,
        )
        .unwrap();
        let mut receipt = materialize_pinned_stage(
            &mut first_lease,
            source(&first, &first_pair),
            StageSecurityMode::Fixture,
            None,
        )
        .unwrap();
        second_lease.record_stage_started().unwrap();
        assert_eq!(
            second_lease.record_staged(&mut receipt).unwrap_err(),
            "WAKE_OFFLINE_STAGE_APPROVAL_MISMATCH"
        );
        assert!(!record_path(&second, "staged").exists());
        assert!(first_lease.record_staged(&mut receipt).unwrap());
        drop(receipt);
        drop(first_lease);
        drop(second_lease);
        first.cleanup();
        second.cleanup();
    }

    #[test]
    fn unexpected_ledger_entry_refuses_staged_before_record_creation() {
        let fixture = TransactionFixture::new();
        let (pair, observed) = pair(&fixture);
        let mut lease = begin_offline_fixture(
            BrokerLedgerGuard::acquire(fixture.root()).unwrap(),
            &pair,
            &observed,
        )
        .unwrap();
        let mut receipt = materialize_pinned_stage(
            &mut lease,
            source(&fixture, &pair),
            StageSecurityMode::Fixture,
            None,
        )
        .unwrap();
        let orphan = record_path(&fixture, "pointer-attempt");
        std::fs::write(&orphan, b"unrelated attempt").unwrap();
        assert_eq!(
            lease.record_staged(&mut receipt).unwrap_err(),
            "WAKE_OFFLINE_LEDGER_RECORD_CHANGED"
        );
        assert!(!record_path(&fixture, "staged").exists());
        assert_eq!(std::fs::read(&orphan).unwrap(), b"unrelated attempt");
        drop(receipt);
        drop(lease);
        fixture.cleanup();
    }

    #[test]
    fn existing_stage_and_invalid_source_are_not_overwritten() {
        let fixture = TransactionFixture::new();
        let (pair, observed) = pair(&fixture);
        let mut lease = begin_offline_fixture(
            BrokerLedgerGuard::acquire(fixture.root()).unwrap(),
            &pair,
            &observed,
        )
        .unwrap();
        let directory = fixture.root_path().join("staging/candidate");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("foreign"), b"preserve").unwrap();
        assert!(
            materialize_pinned_stage(
                &mut lease,
                source(&fixture, &pair),
                StageSecurityMode::Fixture,
                None
            )
            .is_err()
        );
        assert_eq!(lease.phase(), "INTENT");
        assert!(!record_path(&fixture, "stage-started").exists());
        assert_eq!(
            std::fs::read(directory.join("foreign")).unwrap(),
            b"preserve"
        );
        std::fs::write(
            fixture
                .root_path()
                .join("WakeSubmissionV1/candidate/adapter.py"),
            b"wrong source",
        )
        .unwrap();
        assert!(
            pin_fixture_candidate_manifest(
                fixture.root_path(),
                &pair.approval().manifest,
                &pair.approval().manifest_sha256
            )
            .is_err()
        );
        assert!(!record_path(&fixture, "stage-started").exists());
        drop(lease);
        fixture.cleanup();
    }
}
