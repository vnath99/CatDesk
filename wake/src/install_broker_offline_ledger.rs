//! Explicit offline V2 INTENT encoding and locked, read-only replay.
//!
//! Shares the V1 ledger namespace and global lock, but never interprets a V1
//! PID or process snapshot as an offline migration. There is deliberately no
//! production writer or lease: independent inventory/quarantine admission,
//! broker-fetched CI, later phases and installed-reader cutover remain held.

use super::*;
use crate::{
    install_broker_offline_pins::{
        OfflineMigrationPinsV2, OfflineOldStoreLockSetV2, full_identity_of_handle,
    },
    install_broker_offline_v2::{
        CheckedOfflinePairV2, OfflineDesiredStateV2, OfflineMigrationObservationV2,
        OfflineObjectIdentityV2,
    },
};

const DOMAIN_V2: &[u8] = b"CatDeskWakeOfflineInstallLedgerV2\0";

/// The distinct schema has no old PID fields and persists the exact signed
/// pair, full protected/predecessor IDs, paired target and offline disposition.
/// Remaining signed fields are covered by the two envelope hashes. This is
/// not serialized from an operator-selected unverified observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OfflineIntentRecordV2 {
    schema_version: u32,
    purpose: String,
    phase: String,
    install_generation: u64,
    accepted_at_utc_seconds: u64,
    anchor_id: String,
    approval_id: String,
    anchor_envelope_sha256: String,
    approval_envelope_sha256: String,
    verification_key_sha256: String,
    protected_root: OfflineObjectIdentityV2,
    predecessor_package_id: String,
    predecessor_pointer: OfflineObjectIdentityV2,
    predecessor_pointer_sha256: String,
    package_id: String,
    manifest_sha256: String,
    source_commit: String,
    project_id: String,
    target_conversation_id: String,
    target_generation: u64,
    target_snapshot_sha256: String,
    project_target_snapshot_sha256: String,
    original_desired_state: OfflineDesiredStateV2,
    closed_store_inventory_sha256: String,
    quarantined_launch_entrypoints_sha256: String,
    prior_record_sha256: String,
}

impl OfflineIntentRecordV2 {
    fn from_checked(pair: &CheckedOfflinePairV2, accepted_at: u64) -> Self {
        let anchor = pair.anchor();
        let approval = pair.approval();
        let readback = pair.readback();
        Self {
            schema_version: 2,
            purpose: "catdesk-wake-offline-install-ledger".into(),
            phase: "INTENT".into(),
            install_generation: 1,
            accepted_at_utc_seconds: accepted_at,
            anchor_id: anchor.anchor_id.clone(),
            approval_id: approval.approval_id.clone(),
            anchor_envelope_sha256: readback.anchor_envelope_sha256.clone(),
            approval_envelope_sha256: readback.approval_envelope_sha256.clone(),
            verification_key_sha256: readback.verification_key_sha256.clone(),
            protected_root: anchor.target_protected_root.clone(),
            predecessor_package_id: anchor.predecessor_package_id.clone(),
            predecessor_pointer: anchor.predecessor_pointer.clone(),
            predecessor_pointer_sha256: anchor.predecessor_pointer_sha256.clone(),
            package_id: approval.manifest.package_id.clone(),
            manifest_sha256: approval.manifest_sha256.clone(),
            source_commit: approval.source_commit.clone(),
            project_id: anchor.project_id.clone(),
            target_conversation_id: anchor.target_conversation_id.clone(),
            target_generation: anchor.target_generation,
            target_snapshot_sha256: anchor.target_snapshot_sha256.clone(),
            project_target_snapshot_sha256: anchor.project_target_snapshot_sha256.clone(),
            original_desired_state: anchor.desired_state,
            closed_store_inventory_sha256: anchor.closed_store_inventory_sha256.clone(),
            quarantined_launch_entrypoints_sha256: anchor
                .quarantined_launch_entrypoints_sha256
                .clone(),
            prior_record_sha256: "0".repeat(64),
        }
    }
}

/// Exact INTENT inspection only. Matching this readback does not prove that
/// the original admission was authorized or that any migration phase ran.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineIntentReadbackV2 {
    pub schema_version: u32,
    pub install_generation: u64,
    pub anchor_id: String,
    pub approval_id: String,
    pub accepted_at_utc_seconds: u64,
    pub intent_record_sha256: String,
    pub protected_root: OfflineObjectIdentityV2,
    pub disposition: &'static str,
}

fn read_offline_intent(file: &mut File) -> Result<(OfflineIntentRecordV2, String)> {
    file_info(file, None)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    file_info(file, Some(bytes.len()))?;
    let record: OfflineIntentRecordV2 =
        serde_json::from_slice(&bytes).map_err(|_| "WAKE_OFFLINE_LEDGER_RECORD_INVALID")?;
    if serde_json::to_vec(&record).map_err(|_| "WAKE_OFFLINE_LEDGER_RECORD_INVALID")? != bytes
        || record.schema_version != 2
        || record.install_generation != 1
        || record.purpose != "catdesk-wake-offline-install-ledger"
        || record.phase != "INTENT"
        || record.prior_record_sha256 != "0".repeat(64)
    {
        return Err("WAKE_OFFLINE_LEDGER_RECORD_INVALID".into());
    }
    let mut digest = Sha256::new();
    digest.update(DOMAIN_V2);
    digest.update(&bytes);
    Ok((record, format!("{:x}", digest.finalize())))
}

impl BrokerLedgerGuard<'_> {
    /// Inspect the one fixed first-generation offline INTENT while retaining
    /// the existing global lock. Original acceptance time is used only to
    /// inspect an expired signed pair; it cannot authorize a fresh install or
    /// resume. Current physical roles, policy and selected locks are rechecked.
    /// Missing INTENT returns None without creating anything. Any later phase
    /// or mixed history is refused until its V2 reader is implemented.
    pub fn inspect_offline_intent_v2(
        &self,
        policy: &BrokerProtectedPolicyV1,
        anchor_wire: &[u8],
        approval_wire: &[u8],
        observed: &OfflineMigrationObservationV2,
        pins: &OfflineMigrationPinsV2,
        stores: &OfflineOldStoreLockSetV2,
    ) -> Result<Option<OfflineIntentReadbackV2>> {
        self.inspect_offline_intent_with(observed, |accepted_at| {
            let mut original = observed.clone();
            original.now_utc_seconds = accepted_at;
            policy.inspect_offline_pair_pinned(anchor_wire, approval_wire, &original, pins, stores)
        })
    }

    fn inspect_offline_intent_with(
        &self,
        observed: &OfflineMigrationObservationV2,
        check_pair: impl FnOnce(u64) -> Result<CheckedOfflinePairV2>,
    ) -> Result<Option<OfflineIntentReadbackV2>> {
        let inventory = ledger_inventory(self.root)?;
        if inventory.is_empty() {
            return Ok(None);
        }
        let mut expected_slots = [false; LEDGER_SLOT_COUNT];
        expected_slots[InstallPhaseV1::Intent.index()] = true;
        if inventory.len() != 1 || inventory.get(&1) != Some(&expected_slots) {
            return Err("WAKE_OFFLINE_LEDGER_LATER_OR_MIXED_HISTORY".into());
        }
        let mut file = open_optional(self.root, &record_name(1, InstallPhaseV1::Intent))?
            .ok_or("WAKE_LEDGER_RECORD_UNAVAILABLE")?;
        let (record, digest) = read_offline_intent(&mut file)?;
        if observed.now_utc_seconds < record.accepted_at_utc_seconds {
            return Err("WAKE_OFFLINE_LEDGER_CLOCK_ROLLBACK".into());
        }
        let require_root = || -> Result<()> {
            if root_info(self.root)? != observed.target_protected_root.legacy
                || full_identity_of_handle(self.root.as_raw_handle())?
                    != observed.target_protected_root.full
            {
                return Err("WAKE_OFFLINE_LEDGER_ROOT_CHANGED".into());
            }
            Ok(())
        };
        require_root()?;
        let checked = check_pair(record.accepted_at_utc_seconds)?;
        if record != OfflineIntentRecordV2::from_checked(&checked, record.accepted_at_utc_seconds) {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        require_root()?;
        // The file remains exclusively pinned across signature/policy work.
        if read_offline_intent(&mut file)? != (record.clone(), digest.clone())
            || ledger_inventory(self.root)? != inventory
        {
            return Err("WAKE_OFFLINE_LEDGER_RECORD_CHANGED".into());
        }
        Ok(Some(OfflineIntentReadbackV2 {
            schema_version: 2,
            install_generation: 1,
            anchor_id: record.anchor_id,
            approval_id: record.approval_id,
            accepted_at_utc_seconds: record.accepted_at_utc_seconds,
            intent_record_sha256: digest,
            protected_root: record.protected_root,
            disposition: "EXACT_OFFLINE_INTENT_READ_ONLY_NO_INSTALL_AUTHORITY",
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_broker_offline_v2::{
        OfflineMigrationAnchorV2, WakeOfflineInstallApprovalV2, check_offline_pair,
        tests as signed_test,
    };
    use ed25519_dalek::SigningKey;
    use std::path::{Path, PathBuf};

    fn fixture() -> (
        PathBuf,
        OwnedHandle,
        OfflineMigrationAnchorV2,
        WakeOfflineInstallApprovalV2,
        OfflineMigrationObservationV2,
        SigningKey,
    ) {
        let (root, handle, _, _) = super::super::tests::fixture();
        let (mut anchor, mut approval, mut observed, key) = signed_test::fixture();
        let identity = OfflineObjectIdentityV2 {
            legacy: root_info(&handle).expect("fixture legacy root"),
            full: full_identity_of_handle(handle.as_raw_handle()).expect("fixture full root"),
        };
        anchor.target_protected_root = identity.clone();
        approval.target_protected_root = identity.clone();
        observed.target_protected_root = identity;
        (root, handle, anchor, approval, observed, key)
    }

    fn checked(
        anchor: &OfflineMigrationAnchorV2,
        approval: &WakeOfflineInstallApprovalV2,
        observed: &OfflineMigrationObservationV2,
        key: &SigningKey,
        at: u64,
    ) -> Result<CheckedOfflinePairV2> {
        let (anchor_wire, approval_wire) = signed_test::signed_pair(anchor, approval, key);
        let mut original = observed.clone();
        original.now_utc_seconds = at;
        check_offline_pair(
            &anchor_wire,
            &approval_wire,
            &key.verifying_key(),
            &original,
        )
    }

    // Tests provision disposable records directly. Production deliberately
    // exposes no writer until the missing independent admission gates exist.
    fn write_fixture(root: &Path, pair: &CheckedOfflinePairV2, at: u64) -> Vec<u8> {
        let bytes = serde_json::to_vec(&OfflineIntentRecordV2::from_checked(pair, at))
            .expect("fixture canonical intent");
        assert!(bytes.len() <= MAX_RECORD_BYTES);
        std::fs::write(root.join(record_name(1, InstallPhaseV1::Intent)), &bytes)
            .expect("fixture intent");
        bytes
    }

    #[test]
    fn absent_offline_intent_is_read_only_and_does_not_check_a_pair() {
        let (root, handle, _, _, observed, _) = fixture();
        let guard = BrokerLedgerGuard::acquire(&handle).expect("global guard");
        assert!(
            guard
                .inspect_offline_intent_with(&observed, |_| panic!("no INTENT"))
                .expect("empty inventory")
                .is_none()
        );
        assert_eq!(std::fs::read_dir(&root).expect("inventory").count(), 1);
        drop(guard);
        drop(handle);
        super::super::tests::remove_fixture(&root, 1);
    }

    #[test]
    fn exact_offline_intent_inspects_expired_pair_without_v1_or_write_authority() {
        let (root, handle, anchor, approval, mut observed, key) = fixture();
        let accepted_at = observed.now_utc_seconds;
        let pair = checked(&anchor, &approval, &observed, &key, accepted_at).expect("signed pair");
        let bytes = write_fixture(&root, &pair, accepted_at);
        let guard = BrokerLedgerGuard::acquire(&handle).expect("global guard");
        assert!(
            BrokerLedgerGuard::acquire(&handle).is_err(),
            "one shared global lock"
        );
        observed.now_utc_seconds = anchor.expires_utc_seconds + 1;
        assert!(
            checked(
                &anchor,
                &approval,
                &observed,
                &key,
                observed.now_utc_seconds
            )
            .is_err()
        );
        let result = guard
            .inspect_offline_intent_with(&observed, |at| {
                checked(&anchor, &approval, &observed, &key, at)
            })
            .expect("historical inspection")
            .expect("existing intent");
        assert_eq!(result.accepted_at_utc_seconds, accepted_at);
        assert_eq!(result.protected_root, observed.target_protected_root);
        assert!(result.disposition.ends_with("NO_INSTALL_AUTHORITY"));
        assert_ne!(
            result.intent_record_sha256,
            record_digest(&bytes),
            "distinct V2 hash domain"
        );
        assert!(!String::from_utf8_lossy(&bytes).contains("oldHostPid"));
        assert!(
            validated_latest_accepted_at(&handle, &root_info(&handle).unwrap()).is_err(),
            "V1 must not interpret offline INTENT as a migration anchor"
        );
        assert_eq!(
            std::fs::read(root.join(record_name(1, InstallPhaseV1::Intent))).unwrap(),
            bytes
        );
        drop(guard);
        drop(handle);
        super::super::tests::remove_fixture(&root, 1);
    }

    #[test]
    fn offline_intent_checks_full_root_clock_and_exact_signed_disposition() {
        let (root, handle, anchor, approval, observed, key) = fixture();
        let at = observed.now_utc_seconds;
        let pair = checked(&anchor, &approval, &observed, &key, at).unwrap();
        let original = write_fixture(&root, &pair, at);
        let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
        let mut wrong = observed.clone();
        wrong.target_protected_root.full.file_id_hex = "f".repeat(32);
        assert_eq!(
            guard
                .inspect_offline_intent_with(&wrong, |_| panic!("root first"))
                .unwrap_err(),
            "WAKE_OFFLINE_LEDGER_ROOT_CHANGED"
        );
        wrong = observed.clone();
        wrong.now_utc_seconds = at - 1;
        assert_eq!(
            guard
                .inspect_offline_intent_with(&wrong, |_| panic!("clock first"))
                .unwrap_err(),
            "WAKE_OFFLINE_LEDGER_CLOCK_ROLLBACK"
        );
        for change in [
            "targetSnapshotSha256",
            "closedStoreInventorySha256",
            "quarantinedLaunchEntrypointsSha256",
            "anchorEnvelopeSha256",
            "approvalEnvelopeSha256",
            "verificationKeySha256",
        ] {
            let mut record: serde_json::Value = serde_json::from_slice(&original).unwrap();
            record[change] = serde_json::Value::String("f".repeat(64));
            // Re-encode with the typed struct to keep the canonical ordering.
            let typed: OfflineIntentRecordV2 = serde_json::from_value(record).unwrap();
            std::fs::write(
                root.join(record_name(1, InstallPhaseV1::Intent)),
                serde_json::to_vec(&typed).unwrap(),
            )
            .unwrap();
            assert_eq!(
                guard
                    .inspect_offline_intent_with(&observed, |accepted| {
                        checked(&anchor, &approval, &observed, &key, accepted)
                    })
                    .unwrap_err(),
                "WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT",
                "{change}"
            );
        }
        drop(guard);
        drop(handle);
        super::super::tests::remove_fixture(&root, 1);
    }

    #[test]
    fn offline_intent_rejects_noncanonical_unknown_torn_and_v1_records() {
        let (root, handle, anchor, approval, observed, key) = fixture();
        let at = observed.now_utc_seconds;
        let pair = checked(&anchor, &approval, &observed, &key, at).unwrap();
        let original = write_fixture(&root, &pair, at);
        let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
        for bytes in [
            [original.as_slice(), b" "].concat(),
            [&original[..original.len() - 1], b",\"oldHostPid\":7}"].concat(),
            [&original[..original.len() - 1], b",\"schemaVersion\":2}"].concat(),
            original[..original.len() / 2].to_vec(),
            b"{\"schemaVersion\":1,\"oldHostPid\":7}".to_vec(),
        ] {
            std::fs::write(root.join(record_name(1, InstallPhaseV1::Intent)), &bytes).unwrap();
            assert_eq!(
                guard
                    .inspect_offline_intent_with(&observed, |_| panic!("encoding first"))
                    .unwrap_err(),
                "WAKE_OFFLINE_LEDGER_RECORD_INVALID"
            );
            assert_eq!(
                std::fs::read(root.join(record_name(1, InstallPhaseV1::Intent))).unwrap(),
                bytes
            );
        }
        drop(guard);
        drop(handle);
        super::super::tests::remove_fixture(&root, 1);
    }

    #[test]
    fn offline_intent_rejects_later_mixed_and_orphan_attempt_history() {
        let (root, handle, anchor, approval, observed, key) = fixture();
        let pair = checked(
            &anchor,
            &approval,
            &observed,
            &key,
            observed.now_utc_seconds,
        )
        .unwrap();
        write_fixture(&root, &pair, observed.now_utc_seconds);
        let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
        for name in [
            record_name(1, InstallPhaseV1::StageStarted),
            record_name(2, InstallPhaseV1::Intent),
            publication_attempt_name(1),
            pointer_attempt_name(1),
        ] {
            let path = root.join(name);
            std::fs::write(&path, b"unimplemented phase").unwrap();
            assert_eq!(
                guard
                    .inspect_offline_intent_with(&observed, |_| panic!("inventory first"))
                    .unwrap_err(),
                "WAKE_OFFLINE_LEDGER_LATER_OR_MIXED_HISTORY"
            );
            assert_eq!(std::fs::read(&path).unwrap(), b"unimplemented phase");
            std::fs::remove_file(&path).unwrap();
        }
        drop(guard);
        drop(handle);
        super::super::tests::remove_fixture(&root, 1);
    }

    #[test]
    fn offline_intent_retains_file_pin_and_rechecks_history_after_verification() {
        let (root, handle, anchor, approval, observed, key) = fixture();
        let at = observed.now_utc_seconds;
        let pair = checked(&anchor, &approval, &observed, &key, at).unwrap();
        let original = write_fixture(&root, &pair, at);
        let intent_path = root.join(record_name(1, InstallPhaseV1::Intent));
        let later_path = root.join(record_name(1, InstallPhaseV1::StageStarted));
        let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
        assert_eq!(
            guard
                .inspect_offline_intent_with(&observed, |accepted| {
                    assert!(
                        std::fs::write(&intent_path, b"replace").is_err(),
                        "write denied while pinned"
                    );
                    assert!(
                        std::fs::remove_file(&intent_path).is_err(),
                        "delete denied while pinned"
                    );
                    // Simulate an uncooperative directory writer. The reader must
                    // detect even a late recognized phase without repairing it.
                    std::fs::write(&later_path, b"late phase").unwrap();
                    checked(&anchor, &approval, &observed, &key, accepted)
                })
                .unwrap_err(),
            "WAKE_OFFLINE_LEDGER_RECORD_CHANGED"
        );
        assert_eq!(std::fs::read(&intent_path).unwrap(), original);
        std::fs::remove_file(&later_path).unwrap();
        drop(guard);
        drop(handle);
        super::super::tests::remove_fixture(&root, 1);
    }
}
