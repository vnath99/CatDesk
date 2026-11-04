//! Exact V2 restart recovery. Uncertain attempts never authorize another
//! rename. Reopened receipts require canonical signed history, current
//! physical package proof and the same independent admission gates as entry.

use super::pointer::PointerRecoveryRecordsV2;
use super::*;
use crate::install_broker_reader::{ProtectedInstallPinV2, reopen_recovered_offline_pointer};
use crate::install_broker_stage_writer::{
    BrokerPublishedReceiptV2, BrokerStageReceiptV2, reopen_published_after_verified_history,
    reopen_stage_after_verified_history,
};

/// A retained lease plus physically revalidated package. No launch, pointer
/// switch or owner transfer follows from recovery of these phases.
pub enum OfflineRecoveryV2<'a, 'p> {
    Staged {
        lease: OfflineLedgerLeaseV2<'a, 'p>,
        receipt: Box<BrokerStageReceiptV2<'p>>,
    },
    Published {
        lease: OfflineLedgerLeaseV2<'a, 'p>,
        receipt: Box<BrokerPublishedReceiptV2<'p>>,
    },
    PointerSwitched {
        lease: OfflineLedgerLeaseV2<'a, 'p>,
        reader: Box<ProtectedInstallPinV2<'p>>,
    },
}

struct RecoveryRecord {
    file: File,
    record: OfflineIntentRecordV2,
    digest: String,
}

struct RecoveryHistory {
    records: Vec<RecoveryRecord>,
    inventory: BTreeMap<u64, [bool; LEDGER_SLOT_COUNT]>,
    published: bool,
    pointer: Option<PointerRecoveryRecordsV2>,
}

impl RecoveryHistory {
    fn open(root: &OwnedHandle) -> Result<Self> {
        let inventory = ledger_inventory(root)?;
        let mut staged = [false; LEDGER_SLOT_COUNT];
        for phase in [
            InstallPhaseV1::Intent,
            InstallPhaseV1::StageStarted,
            InstallPhaseV1::Staged,
        ] {
            staged[phase.index()] = true;
        }
        let mut attempted = staged;
        attempted[PUBLICATION_ATTEMPT_SLOT] = true;
        let mut published = attempted;
        published[InstallPhaseV1::Published.index()] = true;
        let mut pointer_attempted = published;
        pointer_attempted[POINTER_ATTEMPT_SLOT] = true;
        let mut pointer_created = pointer_attempted;
        pointer_created[InstallPhaseV1::PointerSwitched.index()] = true;
        let slots = inventory
            .get(&1)
            .ok_or("WAKE_OFFLINE_LEDGER_LATER_OR_MIXED_HISTORY")?;
        if inventory.len() != 1
            || ![
                staged,
                attempted,
                published,
                pointer_attempted,
                pointer_created,
            ]
            .contains(slots)
        {
            return Err("WAKE_OFFLINE_LEDGER_LATER_OR_MIXED_HISTORY".into());
        }
        let has_attempt = slots[PUBLICATION_ATTEMPT_SLOT];
        let is_published = slots[InstallPhaseV1::Published.index()];
        let has_pointer_attempt = slots[POINTER_ATTEMPT_SLOT];
        let has_pointer = slots[InstallPhaseV1::PointerSwitched.index()];
        let mut names = vec![
            (record_name(1, InstallPhaseV1::Intent), "INTENT"),
            (
                record_name(1, InstallPhaseV1::StageStarted),
                "STAGE_STARTED",
            ),
            (record_name(1, InstallPhaseV1::Staged), "STAGED"),
        ];
        if has_attempt {
            names.push((publication_attempt_name(1), "PUBLICATION_ATTEMPT"));
        }
        if is_published {
            names.push((record_name(1, InstallPhaseV1::Published), "PUBLISHED"));
        }
        let mut records: Vec<RecoveryRecord> = Vec::with_capacity(names.len());
        for (name, phase) in names {
            let mut file = open_optional(root, &name)?.ok_or("WAKE_LEDGER_RECORD_UNAVAILABLE")?;
            let (record, digest) = if records.is_empty() {
                read_offline_intent(&mut file)?
            } else {
                read_offline_record(&mut file)?
            };
            if let Some(prior) = records.last() {
                let mut expected = prior.record.clone();
                expected.phase = phase.into();
                expected.prior_record_sha256 = prior.digest.clone();
                if phase == "STAGED" {
                    expected.stage_binding = record.stage_binding.clone();
                }
                if record != expected {
                    return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
                }
            }
            records.push(RecoveryRecord {
                file,
                record,
                digest,
            });
        }
        let pointer = if has_pointer {
            Some(PointerRecoveryRecordsV2::open(
                root,
                &records[0].record,
                &records[0].digest,
                &records[4].digest,
                records[2]
                    .record
                    .stage_binding
                    .as_ref()
                    .ok_or("WAKE_OFFLINE_POINTER_RECORD_INVALID")?,
            )?)
        } else {
            None
        };
        let mut history = Self {
            records,
            inventory,
            published: is_published,
            pointer,
        };
        history.revalidate(root)?;
        if has_attempt && !is_published {
            return Err("WAKE_OFFLINE_PUBLICATION_ATTENTION".into());
        }
        if has_pointer_attempt && !has_pointer {
            return Err("WAKE_OFFLINE_POINTER_ATTENTION".into());
        }
        Ok(history)
    }

    fn revalidate(&mut self, root: &OwnedHandle) -> Result<()> {
        for entry in &mut self.records {
            if read_offline_record(&mut entry.file)? != (entry.record.clone(), entry.digest.clone())
            {
                return Err("WAKE_OFFLINE_LEDGER_RECORD_CHANGED".into());
            }
        }
        if let Some(pointer) = self.pointer.as_mut() {
            pointer.revalidate(root)?;
        }
        if ledger_inventory(root)? != self.inventory {
            return Err("WAKE_OFFLINE_LEDGER_RECORD_CHANGED".into());
        }
        Ok(())
    }
}

impl<'a> BrokerLedgerGuard<'a> {
    /// Re-admit only complete STAGED, PUBLISHED or POINTER_SWITCHED history. Original acceptance
    /// time verifies that exact signed pair; current observed roles, locks,
    /// independent quarantine and exact CI are still required for resumption.
    pub fn resume_offline_v2<'p>(
        self,
        policy: &'p BrokerProtectedPolicyV1,
        anchor_wire: &'p [u8],
        approval_wire: &'p [u8],
        observed: &OfflineMigrationObservationV2,
        paths: &OfflineMigrationObjectPathsV2<'_>,
        stores: &'p OfflineOldStoreLockSetV2,
    ) -> Result<OfflineRecoveryV2<'a, 'p>> {
        let history = RecoveryHistory::open(self.root)?;
        let accepted_at = history.records[0].record.accepted_at_utc_seconds;
        if observed.now_utc_seconds < accepted_at {
            return Err("WAKE_OFFLINE_LEDGER_CLOCK_ROLLBACK".into());
        }
        let pins = Arc::new(policy.open_offline_transaction_pins(&self, paths)?);
        let writer = pins.transaction_root_writer()?;
        let mut original = observed.clone();
        original.now_utc_seconds = accepted_at;
        let approved = policy.check_offline_pair_for_ledger(
            anchor_wire,
            approval_wire,
            &original,
            &pins,
            stores,
        )?;
        let expected_pair = approved.checked().readback().clone();
        let revalidate = Box::new(move || {
            let current = policy.inspect_offline_pair_pinned(
                anchor_wire,
                approval_wire,
                &original,
                &pins,
                stores,
            )?;
            if current.readback() != &expected_pair {
                return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
            }
            Ok(())
        });
        self.resume_offline_checked(
            history,
            approved.checked(),
            observed,
            OfflineMutationChecks {
                revalidate,
                root_flush: OfflineRootFlush::Retained(writer),
            },
            Some(policy),
            || {
                require_offline_launch_quarantine()?;
                require_broker_owned_offline_exact_ci(&approved)
            },
        )
    }

    fn resume_offline_checked<'p>(
        self,
        mut history: RecoveryHistory,
        pair: &CheckedOfflinePairV2,
        observed: &OfflineMigrationObservationV2,
        checks: OfflineMutationChecks<'p>,
        policy: Option<&'p BrokerProtectedPolicyV1>,
        admit: impl FnOnce() -> Result<()>,
    ) -> Result<OfflineRecoveryV2<'a, 'p>> {
        let intent = history.records[0].record.clone();
        if observed.now_utc_seconds < intent.accepted_at_utc_seconds {
            return Err("WAKE_OFFLINE_LEDGER_CLOCK_ROLLBACK".into());
        }
        if pair.checked_at_utc_seconds() != intent.accepted_at_utc_seconds
            || OfflineIntentRecordV2::from_checked(pair, intent.accepted_at_utc_seconds) != intent
            || observed.target_protected_root != intent.protected_root
        {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        checks
            .root_flush
            .require_root(self.root, &intent.protected_root)?;
        (checks.revalidate)()?;
        history.revalidate(self.root)?;
        admit()?;
        (checks.revalidate)()?;
        history.revalidate(self.root)?;
        let intent_file = history.records[0]
            .file
            .try_clone()
            .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
        let lease = OfflineLedgerLeaseV2 {
            root: self.root,
            _global_lock: self.global_lock,
            intent_file,
            intent,
            intent_digest: history.records[0].digest.clone(),
            revalidate: checks.revalidate,
            root_flush: checks.root_flush,
            manifest: pair.approval().manifest.clone(),
            policy,
            stage_attempted: true,
            stage_recorded: true,
            stage_started_digest: Some(history.records[1].digest.clone()),
            staged_binding: history.records[2].record.stage_binding.clone(),
            staged_digest: Some(history.records[2].digest.clone()),
            publication_attempted: history.published,
            publication_attempt_digest: history.records.get(3).map(|entry| entry.digest.clone()),
            publication_finish_attempted: history.published,
            published_digest: history.records.get(4).map(|entry| entry.digest.clone()),
            pointer_namespace_attempted: history.pointer.is_some(),
            pointer_namespace_digest: history
                .pointer
                .as_ref()
                .map(|record| record.attempt_digest().to_owned()),
            pointer_finish_attempted: history.pointer.is_some(),
            pointer_binding: history
                .pointer
                .as_ref()
                .map(|record| record.binding().clone()),
            pointer_digest: history
                .pointer
                .as_ref()
                .map(|record| record.created_digest().to_owned()),
        };
        let result = if let Some(pointer) = history.pointer.as_ref() {
            let receipt = reopen_published_after_verified_history(&lease, policy)?;
            let mut reader = reopen_recovered_offline_pointer(&lease, receipt, pointer.binding())?;
            lease.root_flush.flush(
                lease.root,
                &lease.intent.protected_root,
                ParentFlushPoint::LaterLedgerPhase,
            )?;
            reader.revalidate(&lease)?;
            history.revalidate(lease.root)?;
            OfflineRecoveryV2::PointerSwitched {
                lease,
                reader: Box::new(reader),
            }
        } else if history.published {
            let mut receipt = reopen_published_after_verified_history(&lease, policy)?;
            lease.root_flush.flush(
                lease.root,
                &lease.intent.protected_root,
                ParentFlushPoint::LaterLedgerPhase,
            )?;
            lease.revalidate_inputs()?;
            receipt.binding_for_ledger(
                lease.root,
                &lease.intent.protected_root,
                &lease.intent_digest,
                &lease.manifest,
                &lease.intent.manifest_sha256,
            )?;
            history.revalidate(lease.root)?;
            OfflineRecoveryV2::Published {
                lease,
                receipt: Box::new(receipt),
            }
        } else {
            let mut receipt = reopen_stage_after_verified_history(&lease, policy)?;
            lease.root_flush.flush(
                lease.root,
                &lease.intent.protected_root,
                ParentFlushPoint::Staged,
            )?;
            lease.revalidate_inputs()?;
            receipt.binding_for_ledger(
                lease.root,
                &lease.intent.protected_root,
                &lease.intent_digest,
                &lease.manifest,
                &lease.intent.manifest_sha256,
            )?;
            history.revalidate(lease.root)?;
            OfflineRecoveryV2::Staged {
                lease,
                receipt: Box::new(receipt),
            }
        };
        // All original record pins remain live through package proof/flush.
        // The returned lease retains the duplicated exclusive INTENT pin.
        drop(history);
        Ok(result)
    }
}

#[cfg(test)]
pub(crate) fn resume_offline_writer_fixture<'a>(
    guard: BrokerLedgerGuard<'a>,
    pair: &CheckedOfflinePairV2,
    observed: &OfflineMigrationObservationV2,
    writer: Arc<RetainedDirectoryWriterV2>,
) -> Result<OfflineRecoveryV2<'a, 'static>> {
    let history = RecoveryHistory::open(guard.root)?;
    guard.resume_offline_checked(
        history,
        pair,
        observed,
        OfflineMutationChecks {
            revalidate: Box::new(|| Ok(())),
            root_flush: OfflineRootFlush::Retained(writer),
        },
        None,
        || Ok(()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        install_broker_durability::fail_next_flush,
        install_broker_stage_writer::{
            prepare_offline_recovery_fixture, publish_offline_recovery_fixture,
            tests::TransactionFixture,
        },
    };

    fn resume<'a>(
        fixture: &'a TransactionFixture,
        pair: &CheckedOfflinePairV2,
        observed: &OfflineMigrationObservationV2,
    ) -> Result<OfflineRecoveryV2<'a, 'static>> {
        let guard = BrokerLedgerGuard::acquire(fixture.root())?;
        let writer = Arc::new(RetainedDirectoryWriterV2::acquire(
            fixture.root(),
            &observed.target_protected_root,
        )?);
        resume_offline_writer_fixture(guard, pair, observed, writer)
    }

    fn path(fixture: &TransactionFixture, phase: &str) -> std::path::PathBuf {
        fixture.root_path().join(format!(
            "install-transaction-00000000000000000001-{phase}.json"
        ))
    }

    fn records(fixture: &TransactionFixture) -> Vec<(String, Vec<u8>)> {
        [
            "intent",
            "stage-started",
            "staged",
            "publication-attempt",
            "published",
        ]
        .into_iter()
        .filter_map(|phase| {
            std::fs::read(path(fixture, phase))
                .ok()
                .map(|bytes| (phase.into(), bytes))
        })
        .collect()
    }

    #[test]
    fn offline_recovery_staged_reopens_without_intake_and_publishes_once() {
        let fixture = TransactionFixture::new();
        let (lease, receipt, pair, mut observed) = prepare_offline_recovery_fixture(&fixture);
        let original_binding = receipt.stage_binding().clone();
        drop(receipt);
        drop(lease);
        let before = records(&fixture);
        std::fs::rename(
            fixture.root_path().join("WakeSubmissionV1"),
            fixture.root_path().join("preserved-intake"),
        )
        .unwrap();
        observed.now_utc_seconds = 10_000; // Original pair remains checked at acceptance.
        let OfflineRecoveryV2::Staged { mut lease, receipt } =
            resume(&fixture, &pair, &observed).unwrap()
        else {
            panic!("STAGED recovery")
        };
        assert_eq!(lease.phase(), "STAGED");
        assert_eq!(receipt.stage_binding(), &original_binding);
        assert_eq!(
            BrokerLedgerGuard::acquire(fixture.root()).err().unwrap(),
            "WAKE_LEDGER_CONTENDER_BUSY"
        );
        assert!(std::fs::write(path(&fixture, "intent"), b"replace").is_err());
        let published = publish_offline_recovery_fixture(&mut lease, *receipt).unwrap();
        assert_eq!(lease.phase(), "PUBLISHED");
        assert_eq!(published.stage_binding(), &original_binding);
        drop(published);
        drop(lease);
        for (phase, bytes) in before {
            assert_eq!(std::fs::read(path(&fixture, &phase)).unwrap(), bytes);
        }
        assert!(
            fixture
                .root_path()
                .join("preserved-intake/candidate/manifest.json")
                .exists()
        );
        fixture.cleanup();
    }

    #[test]
    fn offline_recovery_published_reopens_exact_bytes_without_another_rename_or_record() {
        let fixture = TransactionFixture::new();
        let (mut lease, receipt, pair, observed) = prepare_offline_recovery_fixture(&fixture);
        let published = publish_offline_recovery_fixture(&mut lease, receipt).unwrap();
        let binding = published.stage_binding().clone();
        let package_id = lease.approved_manifest().package_id.clone();
        drop(published);
        drop(lease);
        let before = records(&fixture);
        for _ in 0..2 {
            let OfflineRecoveryV2::Published {
                mut lease,
                mut receipt,
            } = resume(&fixture, &pair, &observed).unwrap()
            else {
                panic!("PUBLISHED recovery")
            };
            assert_eq!(lease.phase(), "PUBLISHED");
            assert_eq!(receipt.stage_binding(), &binding);
            assert_eq!(
                lease.record_published(&mut receipt).unwrap_err(),
                "WAKE_OFFLINE_PUBLICATION_ATTENTION"
            );
            let package = fixture.root_path().join("packages").join(&package_id);
            assert!(std::fs::write(package.join("adapter.py"), b"replace").is_err());
            assert!(!fixture.root_path().join("current.json").exists());
            drop(lease);
            assert!(std::fs::rename(package, fixture.root_path().join("moved")).is_err());
            drop(receipt);
            assert_eq!(records(&fixture), before);
        }
        fixture.cleanup();
    }

    #[test]
    fn offline_recovery_finishes_existing_published_parent_flush_without_rewriting() {
        let fixture = TransactionFixture::new();
        let (mut lease, receipt, pair, observed) = prepare_offline_recovery_fixture(&fixture);
        fail_next_flush(ParentFlushPoint::LaterLedgerPhase);
        assert_eq!(
            publish_offline_recovery_fixture(&mut lease, receipt)
                .err()
                .unwrap(),
            "WAKE_BROKER_PARENT_FLUSH_FAILED"
        );
        drop(lease);
        let before = records(&fixture);
        assert_eq!(before.len(), 5);
        let result = resume(&fixture, &pair, &observed).unwrap();
        assert!(matches!(result, OfflineRecoveryV2::Published { .. }));
        drop(result);
        assert_eq!(records(&fixture), before);
        fixture.cleanup();
    }

    #[test]
    fn offline_recovery_uncertain_publication_stays_attention_in_all_locations() {
        for point in [
            ParentFlushPoint::PublicationAttempt,
            ParentFlushPoint::PublicationSourceParent,
            ParentFlushPoint::PublicationDestinationParent,
        ] {
            let fixture = TransactionFixture::new();
            let (mut lease, receipt, pair, observed) = prepare_offline_recovery_fixture(&fixture);
            fail_next_flush(point);
            assert_eq!(
                publish_offline_recovery_fixture(&mut lease, receipt)
                    .err()
                    .unwrap(),
                "WAKE_BROKER_PARENT_FLUSH_FAILED"
            );
            drop(lease);
            let before = records(&fixture);
            assert_eq!(
                resume(&fixture, &pair, &observed).err().unwrap(),
                "WAKE_OFFLINE_PUBLICATION_ATTENTION"
            );
            assert_eq!(records(&fixture), before);
            fixture.cleanup();
        }
    }

    #[test]
    fn offline_recovery_full_identity_alias_and_changed_bytes_refuse_without_writes() {
        for change_bytes in [false, true] {
            let fixture = TransactionFixture::new();
            let (lease, receipt, pair, observed) = prepare_offline_recovery_fixture(&fixture);
            drop(receipt);
            drop(lease);
            if change_bytes {
                std::fs::write(
                    fixture.root_path().join("staging/candidate/adapter.py"),
                    b"changed",
                )
                .unwrap();
            } else {
                let mut record: OfflineIntentRecordV2 =
                    serde_json::from_slice(&std::fs::read(path(&fixture, "staged")).unwrap())
                        .unwrap();
                record
                    .stage_binding
                    .as_mut()
                    .unwrap()
                    .candidate_directory
                    .full
                    .file_id_hex = "f".repeat(32);
                std::fs::write(
                    path(&fixture, "staged"),
                    serde_json::to_vec(&record).unwrap(),
                )
                .unwrap();
            }
            let before = records(&fixture);
            assert!(resume(&fixture, &pair, &observed).is_err());
            assert_eq!(records(&fixture), before);
            assert!(!path(&fixture, "publication-attempt").exists());
            fixture.cleanup();
        }
    }

    #[test]
    fn offline_recovery_changed_chain_clock_and_mixed_history_refuse() {
        for mode in 0..3 {
            let fixture = TransactionFixture::new();
            let (mut lease, receipt, pair, mut observed) =
                prepare_offline_recovery_fixture(&fixture);
            let published = publish_offline_recovery_fixture(&mut lease, receipt).unwrap();
            drop(published);
            drop(lease);
            match mode {
                0 => {
                    let mut record: OfflineIntentRecordV2 = serde_json::from_slice(
                        &std::fs::read(path(&fixture, "published")).unwrap(),
                    )
                    .unwrap();
                    record.prior_record_sha256 = "f".repeat(64);
                    std::fs::write(
                        path(&fixture, "published"),
                        serde_json::to_vec(&record).unwrap(),
                    )
                    .unwrap();
                }
                1 => observed.now_utc_seconds = pair.checked_at_utc_seconds() - 1,
                _ => std::fs::write(
                    fixture
                        .root_path()
                        .join("install-transaction-00000000000000000002-intent.json"),
                    b"foreign",
                )
                .unwrap(),
            }
            let before = records(&fixture);
            assert!(resume(&fixture, &pair, &observed).is_err());
            assert_eq!(records(&fixture), before);
            fixture.cleanup();
        }
    }

    #[test]
    fn offline_recovery_missing_quarantine_refuses_before_ci_or_parent_flush() {
        let fixture = TransactionFixture::new();
        let (lease, receipt, pair, observed) = prepare_offline_recovery_fixture(&fixture);
        drop(receipt);
        drop(lease);
        let before = records(&fixture);
        let guard = BrokerLedgerGuard::acquire(fixture.root()).unwrap();
        let history = RecoveryHistory::open(fixture.root()).unwrap();
        let writer = Arc::new(
            RetainedDirectoryWriterV2::acquire(fixture.root(), &observed.target_protected_root)
                .unwrap(),
        );
        let ci = std::cell::Cell::new(false);
        assert_eq!(
            guard
                .resume_offline_checked(
                    history,
                    &pair,
                    &observed,
                    OfflineMutationChecks {
                        revalidate: Box::new(|| Ok(())),
                        root_flush: OfflineRootFlush::Retained(writer)
                    },
                    None,
                    || {
                        require_offline_launch_quarantine()?;
                        ci.set(true);
                        Ok(())
                    }
                )
                .err()
                .unwrap(),
            "WAKE_OFFLINE_LAUNCH_QUARANTINE_UNVERIFIED"
        );
        assert!(!ci.get());
        assert_eq!(records(&fixture), before);
        fixture.cleanup();
    }

    #[test]
    fn offline_recovery_parent_flush_failure_preserves_exact_history_for_retry() {
        let fixture = TransactionFixture::new();
        let (lease, receipt, pair, observed) = prepare_offline_recovery_fixture(&fixture);
        drop(receipt);
        drop(lease);
        let before = records(&fixture);
        fail_next_flush(ParentFlushPoint::Staged);
        assert_eq!(
            resume(&fixture, &pair, &observed).err().unwrap(),
            "WAKE_BROKER_PARENT_FLUSH_FAILED"
        );
        assert_eq!(records(&fixture), before);
        let recovered = resume(&fixture, &pair, &observed).unwrap();
        assert!(matches!(recovered, OfflineRecoveryV2::Staged { .. }));
        drop(recovered);
        assert_eq!(records(&fixture), before);
        fixture.cleanup();
    }

    fn pointer_completed(
        fixture: &TransactionFixture,
    ) -> (
        OfflineLedgerLeaseV2<'_, 'static>,
        ProtectedInstallPinV2<'static>,
        CheckedOfflinePairV2,
        OfflineMigrationObservationV2,
    ) {
        let (mut lease, staged, pair, observed) = prepare_offline_recovery_fixture(fixture);
        let published = publish_offline_recovery_fixture(&mut lease, staged).unwrap();
        let reader = crate::install_broker_pointer_writer::create_offline_pointer_fixture(
            &mut lease, published,
        )
        .unwrap();
        (lease, reader, pair, observed)
    }

    fn pointer_fingerprint(fixture: &TransactionFixture) -> Vec<(String, Vec<u8>, String)> {
        let mut names: Vec<String> = [
            "intent",
            "stage-started",
            "staged",
            "publication-attempt",
            "published",
            "pointer-attempt",
            "pointer-switched",
        ]
        .into_iter()
        .map(|phase| format!("install-transaction-00000000000000000001-{phase}.json"))
        .collect();
        names.extend([
            "current.json".into(),
            "current-00000000000000000001.pending.json".into(),
        ]);
        names
            .into_iter()
            .filter_map(|name| {
                let file = File::open(fixture.root_path().join(&name)).ok()?;
                let id = full_identity_of_handle(file.as_raw_handle()).unwrap();
                Some((
                    name.clone(),
                    std::fs::read(fixture.root_path().join(name)).unwrap(),
                    format!("{}:{}", id.volume_serial, id.file_id_hex),
                ))
            })
            .collect()
    }

    #[test]
    fn offline_pointer_recovery_reopens_complete_phase_without_record_or_pointer_recreation() {
        let fixture = TransactionFixture::new();
        let (lease, reader, pair, mut observed) = pointer_completed(&fixture);
        let binding = reader.pointer_binding().clone();
        let digest = lease.pointer_record_sha256().unwrap().to_owned();
        drop(reader);
        drop(lease);
        let before = pointer_fingerprint(&fixture);
        assert_eq!(before.len(), 8);
        observed.now_utc_seconds = 10_000;
        for _ in 0..2 {
            let OfflineRecoveryV2::PointerSwitched {
                mut lease,
                mut reader,
            } = resume(&fixture, &pair, &observed).unwrap()
            else {
                panic!("complete pointer")
            };
            assert_eq!(lease.phase(), "POINTER_SWITCHED");
            assert_eq!(lease.pointer_record_sha256(), Some(digest.as_str()));
            assert_eq!(reader.pointer_binding(), &binding);
            reader.revalidate(&lease).unwrap();
            let again = reopen_published_after_verified_history(&lease, None).unwrap();
            assert_eq!(
                crate::install_broker_pointer_writer::create_offline_pointer_fixture(
                    &mut lease, again
                )
                .err()
                .unwrap(),
                "WAKE_OFFLINE_POINTER_ATTENTION"
            );
            drop(lease);
            assert!(std::fs::write(fixture.root_path().join("current.json"), b"rewrite").is_err());
            drop(reader);
            assert_eq!(pointer_fingerprint(&fixture), before);
        }
        fixture.cleanup();
    }

    #[test]
    fn offline_pointer_recovery_finishes_failed_final_parent_flush_without_rewriting() {
        let fixture = TransactionFixture::new();
        let (mut lease, staged, pair, observed) = prepare_offline_recovery_fixture(&fixture);
        let published = publish_offline_recovery_fixture(&mut lease, staged).unwrap();
        fail_next_flush(ParentFlushPoint::LaterLedgerPhase);
        assert_eq!(
            crate::install_broker_pointer_writer::create_offline_pointer_fixture(
                &mut lease, published
            )
            .err()
            .unwrap(),
            "WAKE_BROKER_PARENT_FLUSH_FAILED"
        );
        drop(lease);
        let before = pointer_fingerprint(&fixture);
        assert_eq!(before.len(), 8);
        let result = resume(&fixture, &pair, &observed).unwrap();
        assert!(matches!(result, OfflineRecoveryV2::PointerSwitched { .. }));
        drop(result);
        assert_eq!(pointer_fingerprint(&fixture), before);
        fixture.cleanup();
    }

    #[test]
    fn offline_pointer_recovery_partial_attempt_is_attention_before_and_after_native_rename() {
        for point in [
            ParentFlushPoint::PointerAttempt,
            ParentFlushPoint::PointerTempEntry,
            ParentFlushPoint::PointerReplacement,
        ] {
            let fixture = TransactionFixture::new();
            let (mut lease, staged, pair, observed) = prepare_offline_recovery_fixture(&fixture);
            let published = publish_offline_recovery_fixture(&mut lease, staged).unwrap();
            fail_next_flush(point);
            assert!(
                crate::install_broker_pointer_writer::create_offline_pointer_fixture(
                    &mut lease, published
                )
                .is_err()
            );
            drop(lease);
            let before = pointer_fingerprint(&fixture);
            assert_eq!(
                resume(&fixture, &pair, &observed).err().unwrap(),
                "WAKE_OFFLINE_POINTER_ATTENTION"
            );
            assert_eq!(pointer_fingerprint(&fixture), before);
            fixture.cleanup();
        }
    }

    fn replace_json_string(file: &std::path::Path, field: &str, replacement: &str) {
        let bytes = std::fs::read(file).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let old = value[field].as_str().unwrap();
        let original = String::from_utf8(bytes).unwrap();
        let changed = original.replacen(
            &format!("\"{field}\":\"{old}\""),
            &format!("\"{field}\":\"{replacement}\""),
            1,
        );
        assert_ne!(original, changed);
        std::fs::write(file, changed).unwrap();
    }

    #[test]
    fn offline_pointer_recovery_refuses_tampered_attempt_link_and_full_identity() {
        for mode in 0..3 {
            let fixture = TransactionFixture::new();
            let (lease, reader, pair, observed) = pointer_completed(&fixture);
            let id = reader
                .pointer_binding()
                .pointer_file
                .full
                .file_id_hex
                .clone();
            drop(reader);
            drop(lease);
            if mode == 0 {
                replace_json_string(
                    &path(&fixture, "pointer-attempt"),
                    "plannedPointerSha256",
                    &"f".repeat(64),
                );
            } else if mode == 1 {
                replace_json_string(
                    &path(&fixture, "pointer-switched"),
                    "pointerAttemptSha256",
                    &"f".repeat(64),
                );
            } else {
                let file = path(&fixture, "pointer-switched");
                let bytes = std::fs::read_to_string(&file).unwrap();
                let changed = bytes.replacen(&id, &"f".repeat(32), 1);
                assert_ne!(bytes, changed);
                std::fs::write(file, changed).unwrap();
            }
            let before = pointer_fingerprint(&fixture);
            assert!(resume(&fixture, &pair, &observed).is_err());
            assert_eq!(pointer_fingerprint(&fixture), before);
            fixture.cleanup();
        }
    }

    #[test]
    fn offline_pointer_recovery_refuses_replaced_current_or_changed_pointer_bytes() {
        for replace in [false, true] {
            let fixture = TransactionFixture::new();
            let (lease, reader, pair, observed) = pointer_completed(&fixture);
            drop(reader);
            drop(lease);
            let current = fixture.root_path().join("current.json");
            if replace {
                let bytes = std::fs::read(&current).unwrap();
                std::fs::rename(&current, fixture.root_path().join("preserved-current.json"))
                    .unwrap();
                std::fs::write(&current, bytes).unwrap();
            } else {
                std::fs::write(&current, b"changed").unwrap();
            }
            let before = pointer_fingerprint(&fixture);
            assert!(resume(&fixture, &pair, &observed).is_err());
            assert_eq!(pointer_fingerprint(&fixture), before);
            fixture.cleanup();
        }
    }

    #[test]
    fn offline_pointer_recovery_refuses_extra_pending_unknown_fields_and_clock_rollback() {
        for mode in 0..3 {
            let fixture = TransactionFixture::new();
            let (lease, reader, pair, mut observed) = pointer_completed(&fixture);
            drop(reader);
            drop(lease);
            match mode {
                0 => std::fs::write(
                    fixture
                        .root_path()
                        .join("current-00000000000000000001.pending.json"),
                    b"foreign pending",
                )
                .unwrap(),
                1 => {
                    let file = path(&fixture, "pointer-switched");
                    let mut bytes = std::fs::read(&file).unwrap();
                    assert_eq!(bytes.pop(), Some(b'}'));
                    bytes.extend(b",\"unknown\":true}");
                    std::fs::write(file, bytes).unwrap();
                }
                _ => observed.now_utc_seconds = pair.checked_at_utc_seconds() - 1,
            }
            let before = pointer_fingerprint(&fixture);
            assert!(resume(&fixture, &pair, &observed).is_err());
            assert_eq!(pointer_fingerprint(&fixture), before);
            fixture.cleanup();
        }
    }

    #[test]
    fn offline_pointer_recovery_flush_failure_keeps_complete_history_recoverable() {
        let fixture = TransactionFixture::new();
        let (lease, reader, pair, observed) = pointer_completed(&fixture);
        drop(reader);
        drop(lease);
        let before = pointer_fingerprint(&fixture);
        fail_next_flush(ParentFlushPoint::LaterLedgerPhase);
        assert_eq!(
            resume(&fixture, &pair, &observed).err().unwrap(),
            "WAKE_BROKER_PARENT_FLUSH_FAILED"
        );
        assert_eq!(pointer_fingerprint(&fixture), before);
        let result = resume(&fixture, &pair, &observed).unwrap();
        drop(result);
        assert_eq!(pointer_fingerprint(&fixture), before);
        fixture.cleanup();
    }

    #[test]
    fn offline_pointer_recovery_retains_all_seven_record_pins_through_admission() {
        let fixture = TransactionFixture::new();
        let (lease, reader, pair, observed) = pointer_completed(&fixture);
        drop(reader);
        drop(lease);
        let before = pointer_fingerprint(&fixture);
        let guard = BrokerLedgerGuard::acquire(fixture.root()).unwrap();
        let history = RecoveryHistory::open(fixture.root()).unwrap();
        let writer = Arc::new(
            RetainedDirectoryWriterV2::acquire(fixture.root(), &observed.target_protected_root)
                .unwrap(),
        );
        let result = guard
            .resume_offline_checked(
                history,
                &pair,
                &observed,
                OfflineMutationChecks {
                    revalidate: Box::new(|| Ok(())),
                    root_flush: OfflineRootFlush::Retained(writer),
                },
                None,
                || {
                    for phase in [
                        "intent",
                        "stage-started",
                        "staged",
                        "publication-attempt",
                        "published",
                        "pointer-attempt",
                        "pointer-switched",
                    ] {
                        assert!(
                            std::fs::write(path(&fixture, phase), b"blocked").is_err(),
                            "journal pin: {phase}"
                        );
                    }
                    Ok(())
                },
            )
            .unwrap();
        assert!(matches!(result, OfflineRecoveryV2::PointerSwitched { .. }));
        drop(result);
        assert_eq!(pointer_fingerprint(&fixture), before);
        fixture.cleanup();
    }
}
