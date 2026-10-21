//! Durable admission for the first protected V2 pointer's namespace write.
//! The predecessor remains in its separately pinned legacy runtime. Existing
//! protected current/pending objects are never approved for replacement here.

use super::*;
use crate::install_broker_stage_writer::BrokerPublishedReceiptV2;

const MAX_POINTER_V2_BYTES: usize = 8 * 1024;
const ATTEMPT_DOMAIN: &[u8] = b"CatDeskWakeOfflinePointerAttemptV2\0";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PointerDataV2 {
    schema_version: u32,
    purpose: &'static str,
    install_generation: u64,
    anchor_id: String,
    approval_id: String,
    intent_record_sha256: String,
    protected_root: OfflineObjectIdentityV2,
    predecessor_package_id: String,
    predecessor_pointer: OfflineObjectIdentityV2,
    predecessor_pointer_sha256: String,
    package_id: String,
    manifest_sha256: String,
    stage_binding: StageBindingV2,
}

/// Canonical planned bytes only. This value cannot create a lease, authorize
/// an attempt or prove that current.json exists or that an owner is stopped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflinePointerPlanV2 {
    canonical_bytes: Vec<u8>,
    sha256: String,
}

impl OfflinePointerPlanV2 {
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PointerNamespaceAttemptV2 {
    schema_version: u32,
    purpose: String,
    install_generation: u64,
    intent_record_sha256: String,
    published_record_sha256: String,
    protected_root: OfflineObjectIdentityV2,
    predecessor_pointer: OfflineObjectIdentityV2,
    predecessor_pointer_sha256: String,
    closed_store_inventory_sha256: String,
    quarantined_launch_entrypoints_sha256: String,
    planned_pointer_sha256: String,
}

fn read_attempt(file: &mut File) -> Result<(PointerNamespaceAttemptV2, String)> {
    file_info_bounded(file, None, MAX_OFFLINE_RECORD_BYTES)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    let mut bytes = Vec::new();
    file.take(MAX_OFFLINE_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    file_info_bounded(file, Some(bytes.len()), MAX_OFFLINE_RECORD_BYTES)?;
    let value: PointerNamespaceAttemptV2 =
        serde_json::from_slice(&bytes).map_err(|_| "WAKE_OFFLINE_POINTER_ATTEMPT_INVALID")?;
    if serde_json::to_vec(&value).map_err(|_| "WAKE_OFFLINE_POINTER_ATTEMPT_INVALID")? != bytes {
        return Err("WAKE_OFFLINE_POINTER_ATTEMPT_INVALID".into());
    }
    let mut hash = Sha256::new();
    hash.update(ATTEMPT_DOMAIN);
    hash.update(bytes);
    Ok((value, format!("{:x}", hash.finalize())))
}

impl OfflineLedgerLeaseV2<'_, '_> {
    fn require_published_receipt(
        &self,
        published: &mut BrokerPublishedReceiptV2<'_>,
    ) -> Result<()> {
        let binding = published.binding_for_ledger(
            self.root,
            &self.intent.protected_root,
            &self.intent_digest,
            &self.manifest,
            &self.intent.manifest_sha256,
        )?;
        if self.staged_binding.as_ref() != Some(&binding) {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        Ok(())
    }
    fn pointer_plan(&self) -> Result<OfflinePointerPlanV2> {
        let data = PointerDataV2 {
            schema_version: 2,
            purpose: "catdesk-wake-offline-protected-pointer",
            install_generation: 1,
            anchor_id: self.intent.anchor_id.clone(),
            approval_id: self.intent.approval_id.clone(),
            intent_record_sha256: self.intent_digest.clone(),
            protected_root: self.intent.protected_root.clone(),
            predecessor_package_id: self.intent.predecessor_package_id.clone(),
            predecessor_pointer: self.intent.predecessor_pointer.clone(),
            predecessor_pointer_sha256: self.intent.predecessor_pointer_sha256.clone(),
            package_id: self.intent.package_id.clone(),
            manifest_sha256: self.intent.manifest_sha256.clone(),
            stage_binding: self.stage_binding_for_recovery()?.clone(),
        };
        let canonical_bytes =
            serde_json::to_vec(&data).map_err(|_| "WAKE_OFFLINE_POINTER_PLAN_INVALID")?;
        if canonical_bytes.is_empty() || canonical_bytes.len() > MAX_POINTER_V2_BYTES {
            return Err("WAKE_OFFLINE_POINTER_PLAN_INVALID".into());
        }
        let sha256 = format!("{:x}", Sha256::digest(&canonical_bytes));
        Ok(OfflinePointerPlanV2 {
            canonical_bytes,
            sha256,
        })
    }

    fn exact_published_record(&mut self) -> Result<String> {
        let (mut expected, attempt_digest) = self.exact_publication_attempt()?;
        expected.phase = "PUBLISHED".into();
        expected.prior_record_sha256 = attempt_digest;
        let mut file = open_optional(self.root, &record_name(1, InstallPhaseV1::Published))?
            .ok_or("WAKE_LEDGER_RECORD_UNAVAILABLE")?;
        let (actual, digest) = read_offline_record(&mut file)?;
        if actual != expected || Some(&digest) != self.published_digest.as_ref() {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        Ok(digest)
    }

    fn pointer_inventory(&self, attempted: bool) -> Result<()> {
        let mut slots = [false; LEDGER_SLOT_COUNT];
        for phase in [
            InstallPhaseV1::Intent,
            InstallPhaseV1::StageStarted,
            InstallPhaseV1::Staged,
            InstallPhaseV1::Published,
        ] {
            slots[phase.index()] = true;
        }
        slots[PUBLICATION_ATTEMPT_SLOT] = true;
        slots[POINTER_ATTEMPT_SLOT] = attempted;
        let inventory = ledger_inventory(self.root)?;
        if inventory.len() != 1 || inventory.get(&1) != Some(&slots) {
            return Err("WAKE_OFFLINE_POINTER_ATTENTION".into());
        }
        Ok(())
    }

    fn expected_pointer_attempt(
        &self,
        plan: &OfflinePointerPlanV2,
        published_digest: String,
    ) -> PointerNamespaceAttemptV2 {
        PointerNamespaceAttemptV2 {
            schema_version: 2,
            purpose: "catdesk-wake-offline-pointer-attempt".into(),
            install_generation: 1,
            intent_record_sha256: self.intent_digest.clone(),
            published_record_sha256: published_digest,
            protected_root: self.intent.protected_root.clone(),
            predecessor_pointer: self.intent.predecessor_pointer.clone(),
            predecessor_pointer_sha256: self.intent.predecessor_pointer_sha256.clone(),
            closed_store_inventory_sha256: self.intent.closed_store_inventory_sha256.clone(),
            quarantined_launch_entrypoints_sha256: self
                .intent
                .quarantined_launch_entrypoints_sha256
                .clone(),
            planned_pointer_sha256: plan.sha256.clone(),
        }
    }

    /// Prepare one first-pointer namespace mutation. Admission is inherited
    /// from the opaque V2 lease; PUBLISHED alone is not an old-owner proof.
    /// The fixed service still cannot obtain that lease while quarantine is
    /// unimplemented. No pointer is created or owner launched by this method.
    pub fn prepare_initial_pointer_namespace(
        &mut self,
        published: &mut BrokerPublishedReceiptV2<'_>,
    ) -> Result<OfflinePointerPlanV2> {
        if self.phase() != "PUBLISHED" || self.pointer_namespace_attempted {
            return Err("WAKE_OFFLINE_POINTER_ATTENTION".into());
        }
        self.revalidate_inputs()?;
        self.require_published_receipt(published)?;
        let plan = self.pointer_plan()?;
        let digest = self.exact_published_record()?;
        self.pointer_inventory(false)?;
        for name in ["current.json", "current-00000000000000000001.pending.json"] {
            if open_optional(self.root, name)?.is_some() {
                return Err("WAKE_OFFLINE_POINTER_EXISTING_OBJECT".into());
            }
        }
        let expected = self.expected_pointer_attempt(&plan, digest);
        self.pointer_namespace_attempted = true;
        let mut file = open_relative(self.root, &pointer_attempt_name(1), FILE_CREATE)
            .map_err(|_| "WAKE_OFFLINE_POINTER_ATTENTION")?;
        file_info_bounded(&file, Some(0), MAX_OFFLINE_RECORD_BYTES)?;
        let bytes =
            serde_json::to_vec(&expected).map_err(|_| "WAKE_OFFLINE_POINTER_ATTEMPT_INVALID")?;
        if bytes.is_empty() || bytes.len() > MAX_OFFLINE_RECORD_BYTES {
            return Err("WAKE_OFFLINE_POINTER_ATTEMPT_INVALID".into());
        }
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "WAKE_LEDGER_WRITE_FAILED")?;
        let (actual, digest) = read_attempt(&mut file)?;
        if actual != expected {
            return Err("WAKE_OFFLINE_POINTER_ATTEMPT_INVALID".into());
        }
        self.root_flush.flush(
            self.root,
            &self.intent.protected_root,
            ParentFlushPoint::PointerAttempt,
        )?;
        self.exact_published_record()?;
        self.pointer_inventory(true)?;
        self.require_published_receipt(published)?;
        if read_attempt(&mut file)? != (expected, digest.clone()) {
            return Err("WAKE_OFFLINE_POINTER_ATTEMPT_INVALID".into());
        }
        self.root_flush
            .prepare_namespace_write(self.root, &self.intent.protected_root)?;
        self.revalidate_inputs()?;
        self.pointer_namespace_digest = Some(digest);
        Ok(plan)
    }

    /// Recheck the exact marked operation without creating or retrying it.
    pub fn revalidate_initial_pointer_namespace(
        &mut self,
        plan: &OfflinePointerPlanV2,
        published: &mut BrokerPublishedReceiptV2<'_>,
    ) -> Result<()> {
        if self.phase() != "PUBLISHED"
            || !self.pointer_namespace_attempted
            || self.pointer_namespace_digest.is_none()
        {
            return Err("WAKE_OFFLINE_POINTER_ATTENTION".into());
        }
        self.revalidate_inputs()?;
        if self.pointer_plan()? != *plan {
            return Err("WAKE_OFFLINE_POINTER_PLAN_INVALID".into());
        }
        let published_digest = self.exact_published_record()?;
        self.pointer_inventory(true)?;
        let mut file = open_optional(self.root, &pointer_attempt_name(1))?
            .ok_or("WAKE_OFFLINE_POINTER_ATTENTION")?;
        let (actual, digest) = read_attempt(&mut file)?;
        if actual != self.expected_pointer_attempt(plan, published_digest)
            || Some(&digest) != self.pointer_namespace_digest.as_ref()
        {
            return Err("WAKE_OFFLINE_POINTER_ATTEMPT_INVALID".into());
        }
        self.require_published_receipt(published)?;
        Ok(())
    }
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
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};

    fn writable(root: &std::path::Path) -> bool {
        OpenOptions::new()
            .write(true)
            .share_mode(3)
            .custom_flags(0x0220_0000)
            .open(root)
            .is_ok()
    }

    #[test]
    fn offline_pointer_namespace_requires_durable_typed_publication_before_writer_handoff() {
        let fixture = TransactionFixture::new();
        let (mut lease, staged, pair, observed) = prepare_offline_recovery_fixture(&fixture);
        let mut published = publish_offline_recovery_fixture(&mut lease, staged).unwrap();
        assert!(!writable(fixture.root_path()));
        let before_published = std::fs::read(
            fixture
                .root_path()
                .join(record_name(1, InstallPhaseV1::Published)),
        )
        .unwrap();
        let plan = lease
            .prepare_initial_pointer_namespace(&mut published)
            .unwrap();
        assert!(
            writable(fixture.root_path()),
            "only the marked namespace phase permits compatible WRITE opens"
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(plan.canonical_bytes())),
            plan.sha256()
        );
        assert!(plan.canonical_bytes().len() <= MAX_POINTER_V2_BYTES);
        let value: serde_json::Value = serde_json::from_slice(plan.canonical_bytes()).unwrap();
        assert_eq!(value["schemaVersion"], 2);
        assert_eq!(
            value["predecessorPointerSha256"],
            observed.predecessor_pointer_sha256
        );
        assert!(
            !fixture.root_path().join("current.json").exists(),
            "preparation creates no pointer"
        );
        lease
            .revalidate_initial_pointer_namespace(&plan, &mut published)
            .unwrap();
        assert_eq!(
            lease
                .prepare_initial_pointer_namespace(&mut published)
                .unwrap_err(),
            "WAKE_OFFLINE_POINTER_ATTENTION"
        );
        let marker_path = fixture.root_path().join(pointer_attempt_name(1));
        let marker_before = std::fs::read(&marker_path).unwrap();
        drop(published);
        drop(lease);
        assert_eq!(
            std::fs::read(
                fixture
                    .root_path()
                    .join(record_name(1, InstallPhaseV1::Published))
            )
            .unwrap(),
            before_published
        );
        let guard = BrokerLedgerGuard::acquire(fixture.root()).unwrap();
        assert_eq!(
            guard
                .accept_offline_checked(&pair, &observed, Box::new(|| Ok(())), || Ok(()), || Ok(()))
                .err()
                .unwrap(),
            "WAKE_OFFLINE_LEDGER_LATER_OR_MIXED_HISTORY"
        );
        assert_eq!(std::fs::read(marker_path).unwrap(), marker_before);
        fixture.cleanup();
    }

    #[test]
    fn offline_pointer_namespace_existing_current_or_pending_is_preserved_before_marker() {
        for leaf in ["current.json", "current-00000000000000000001.pending.json"] {
            let fixture = TransactionFixture::new();
            let file = fixture.root_path().join(leaf);
            std::fs::write(&file, b"foreign keep").unwrap();
            let (mut lease, staged, _, _) = prepare_offline_recovery_fixture(&fixture);
            let mut published = publish_offline_recovery_fixture(&mut lease, staged).unwrap();
            assert_eq!(
                lease
                    .prepare_initial_pointer_namespace(&mut published)
                    .unwrap_err(),
                "WAKE_OFFLINE_POINTER_EXISTING_OBJECT"
            );
            assert!(!writable(fixture.root_path()));
            assert!(!fixture.root_path().join(pointer_attempt_name(1)).exists());
            assert_eq!(std::fs::read(file).unwrap(), b"foreign keep");
            drop(published);
            drop(lease);
            fixture.cleanup();
        }
    }

    #[test]
    fn offline_pointer_namespace_flush_failure_is_sticky_and_never_relaxes_writer() {
        let fixture = TransactionFixture::new();
        let (mut lease, staged, _, _) = prepare_offline_recovery_fixture(&fixture);
        let mut published = publish_offline_recovery_fixture(&mut lease, staged).unwrap();
        fail_next_flush(ParentFlushPoint::PointerAttempt);
        assert_eq!(
            lease
                .prepare_initial_pointer_namespace(&mut published)
                .unwrap_err(),
            "WAKE_BROKER_PARENT_FLUSH_FAILED"
        );
        assert!(!writable(fixture.root_path()));
        let marker_path = fixture.root_path().join(pointer_attempt_name(1));
        let before = std::fs::read(&marker_path).unwrap();
        assert_eq!(
            lease
                .prepare_initial_pointer_namespace(&mut published)
                .unwrap_err(),
            "WAKE_OFFLINE_POINTER_ATTENTION"
        );
        assert_eq!(std::fs::read(marker_path).unwrap(), before);
        assert!(!fixture.root_path().join("current.json").exists());
        drop(published);
        drop(lease);
        fixture.cleanup();
    }

    #[test]
    fn offline_pointer_namespace_changed_plan_or_marker_refuses_revalidation() {
        let fixture = TransactionFixture::new();
        let (mut lease, staged, _, _) = prepare_offline_recovery_fixture(&fixture);
        let mut published = publish_offline_recovery_fixture(&mut lease, staged).unwrap();
        let plan = lease
            .prepare_initial_pointer_namespace(&mut published)
            .unwrap();
        let mut wrong = plan.clone();
        wrong.canonical_bytes.push(b' ');
        assert_eq!(
            lease
                .revalidate_initial_pointer_namespace(&wrong, &mut published)
                .unwrap_err(),
            "WAKE_OFFLINE_POINTER_PLAN_INVALID"
        );
        std::fs::write(fixture.root_path().join(pointer_attempt_name(1)), b"torn").unwrap();
        assert_eq!(
            lease
                .revalidate_initial_pointer_namespace(&plan, &mut published)
                .unwrap_err(),
            "WAKE_OFFLINE_POINTER_ATTEMPT_INVALID"
        );
        assert!(!fixture.root_path().join("current.json").exists());
        drop(published);
        drop(lease);
        fixture.cleanup();
    }

    #[test]
    fn offline_pointer_namespace_cross_root_receipt_never_marks_or_changes_either_root() {
        let first = TransactionFixture::new();
        let second = TransactionFixture::new();
        let (mut first_lease, first_stage, _, _) = prepare_offline_recovery_fixture(&first);
        let first_published =
            publish_offline_recovery_fixture(&mut first_lease, first_stage).unwrap();
        let (mut second_lease, second_stage, _, _) = prepare_offline_recovery_fixture(&second);
        let mut second_published =
            publish_offline_recovery_fixture(&mut second_lease, second_stage).unwrap();
        assert_eq!(
            first_lease
                .prepare_initial_pointer_namespace(&mut second_published)
                .unwrap_err(),
            "WAKE_OFFLINE_PUBLICATION_APPROVAL_MISMATCH"
        );
        for fixture in [&first, &second] {
            assert!(!writable(fixture.root_path()));
            assert!(!fixture.root_path().join(pointer_attempt_name(1)).exists());
            assert!(!fixture.root_path().join("current.json").exists());
        }
        drop(first_published);
        drop(second_published);
        drop(first_lease);
        drop(second_lease);
        first.cleanup();
        second.cleanup();
    }
}
