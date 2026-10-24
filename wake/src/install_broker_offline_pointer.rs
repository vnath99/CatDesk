//! Durable admission for the first protected V2 pointer's namespace write.
//! The predecessor remains in its separately pinned legacy runtime. Existing
//! protected current/pending objects are never approved for replacement here.

use super::*;
use crate::install_broker_stage_writer::BrokerPublishedReceiptV2;

const MAX_POINTER_V2_BYTES: usize = 8 * 1024;
const ATTEMPT_DOMAIN: &[u8] = b"CatDeskWakeOfflinePointerAttemptV2\0";
const CREATED_DOMAIN: &[u8] = b"CatDeskWakeOfflinePointerCreatedV2\0";

fn valid_pointer_binding(
    binding: &OfflinePointerBindingV2,
    intent: &OfflineIntentRecordV2,
    stage: &StageBindingV2,
    plan_sha256: &str,
) -> bool {
    let root = &intent.protected_root;
    let objects = [
        &root.full,
        &intent.predecessor_pointer.full,
        &stage.staging_directory.full,
        &stage.packages_directory.full,
        &stage.candidate_directory.full,
        &stage.manifest_file.full,
        &stage.artifacts[0].full,
        &stage.artifacts[1].full,
        &stage.artifacts[2].full,
        &stage.artifacts[3].full,
    ];
    crate::install_broker_offline_v2::valid_object(&binding.pointer_file)
        && binding.pointer_file.full.volume_serial == root.full.volume_serial
        && binding.pointer_file.legacy.volume_serial == root.legacy.volume_serial
        && !objects.contains(&&binding.pointer_file.full)
        && binding.pointer_sha256 == plan_sha256
}

fn pointer_plan_for(
    intent: &OfflineIntentRecordV2,
    intent_digest: &str,
    stage: &StageBindingV2,
) -> Result<OfflinePointerPlanV2> {
    let data = PointerDataV2 {
        schema_version: 2,
        purpose: "catdesk-wake-offline-protected-pointer",
        install_generation: 1,
        anchor_id: intent.anchor_id.clone(),
        approval_id: intent.approval_id.clone(),
        intent_record_sha256: intent_digest.into(),
        protected_root: intent.protected_root.clone(),
        predecessor_package_id: intent.predecessor_package_id.clone(),
        predecessor_pointer: intent.predecessor_pointer.clone(),
        predecessor_pointer_sha256: intent.predecessor_pointer_sha256.clone(),
        package_id: intent.package_id.clone(),
        manifest_sha256: intent.manifest_sha256.clone(),
        stage_binding: stage.clone(),
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

fn expected_attempt_for(
    intent: &OfflineIntentRecordV2,
    intent_digest: &str,
    published_digest: &str,
    plan: &OfflinePointerPlanV2,
) -> PointerNamespaceAttemptV2 {
    PointerNamespaceAttemptV2 {
        schema_version: 2,
        purpose: "catdesk-wake-offline-pointer-attempt".into(),
        install_generation: 1,
        intent_record_sha256: intent_digest.into(),
        published_record_sha256: published_digest.into(),
        protected_root: intent.protected_root.clone(),
        predecessor_pointer: intent.predecessor_pointer.clone(),
        predecessor_pointer_sha256: intent.predecessor_pointer_sha256.clone(),
        closed_store_inventory_sha256: intent.closed_store_inventory_sha256.clone(),
        quarantined_launch_entrypoints_sha256: intent.quarantined_launch_entrypoints_sha256.clone(),
        planned_pointer_sha256: plan.sha256.clone(),
    }
}

fn expected_created_for(
    intent: &OfflineIntentRecordV2,
    intent_digest: &str,
    published_digest: &str,
    attempt_digest: &str,
    binding: &OfflinePointerBindingV2,
    stage: &StageBindingV2,
) -> PointerCreatedRecordV2 {
    PointerCreatedRecordV2 {
        schema_version: 2,
        purpose: "catdesk-wake-offline-pointer-created".into(),
        install_generation: 1,
        intent_record_sha256: intent_digest.into(),
        published_record_sha256: published_digest.into(),
        pointer_attempt_sha256: attempt_digest.into(),
        protected_root: intent.protected_root.clone(),
        pointer_binding: binding.clone(),
        stage_binding: stage.clone(),
    }
}

/// Both journal pins remain live through signature admission and physical
/// pointer/package reconstruction. No file creation or rename occurs here.
pub(super) struct PointerRecoveryRecordsV2 {
    attempt_file: File,
    created_file: File,
    attempt: PointerNamespaceAttemptV2,
    created: PointerCreatedRecordV2,
    attempt_digest: String,
    created_digest: String,
}

impl PointerRecoveryRecordsV2 {
    pub(super) fn open(
        root: &OwnedHandle,
        intent: &OfflineIntentRecordV2,
        intent_digest: &str,
        published_digest: &str,
        stage: &StageBindingV2,
    ) -> Result<Self> {
        let mut attempt_file = open_optional(root, &pointer_attempt_name(1))?
            .ok_or("WAKE_OFFLINE_POINTER_ATTENTION")?;
        let (attempt, attempt_digest) = read_attempt(&mut attempt_file)?;
        let plan = pointer_plan_for(intent, intent_digest, stage)?;
        if attempt != expected_attempt_for(intent, intent_digest, published_digest, &plan) {
            return Err("WAKE_OFFLINE_POINTER_ATTEMPT_INVALID".into());
        }
        let mut created_file =
            open_optional(root, &record_name(1, InstallPhaseV1::PointerSwitched))?
                .ok_or("WAKE_OFFLINE_POINTER_ATTENTION")?;
        let (created, created_digest) = read_created(&mut created_file)?;
        if !valid_pointer_binding(&created.pointer_binding, intent, stage, plan.sha256())
            || created
                != expected_created_for(
                    intent,
                    intent_digest,
                    published_digest,
                    &attempt_digest,
                    &created.pointer_binding,
                    stage,
                )
        {
            return Err("WAKE_OFFLINE_POINTER_RECORD_INVALID".into());
        }
        let mut result = Self {
            attempt_file,
            created_file,
            attempt,
            created,
            attempt_digest,
            created_digest,
        };
        result.revalidate(root)?;
        Ok(result)
    }

    pub(super) fn revalidate(&mut self, root: &OwnedHandle) -> Result<()> {
        if read_attempt(&mut self.attempt_file)?
            != (self.attempt.clone(), self.attempt_digest.clone())
            || read_created(&mut self.created_file)?
                != (self.created.clone(), self.created_digest.clone())
        {
            return Err("WAKE_OFFLINE_POINTER_RECORD_INVALID".into());
        }
        if open_optional(root, "current-00000000000000000001.pending.json")?.is_some() {
            return Err("WAKE_OFFLINE_POINTER_ATTENTION".into());
        }
        Ok(())
    }

    pub(super) fn binding(&self) -> &OfflinePointerBindingV2 {
        &self.created.pointer_binding
    }
    pub(super) fn attempt_digest(&self) -> &str {
        &self.attempt_digest
    }
    pub(super) fn created_digest(&self) -> &str {
        &self.created_digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OfflinePointerBindingV2 {
    pub pointer_file: OfflineObjectIdentityV2,
    pub pointer_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PointerCreatedRecordV2 {
    schema_version: u32,
    purpose: String,
    install_generation: u64,
    intent_record_sha256: String,
    published_record_sha256: String,
    pointer_attempt_sha256: String,
    protected_root: OfflineObjectIdentityV2,
    pointer_binding: OfflinePointerBindingV2,
    stage_binding: StageBindingV2,
}

fn read_created(file: &mut File) -> Result<(PointerCreatedRecordV2, String)> {
    file_info_bounded(file, None, MAX_OFFLINE_RECORD_BYTES)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    let mut bytes = Vec::new();
    file.take(MAX_OFFLINE_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    file_info_bounded(file, Some(bytes.len()), MAX_OFFLINE_RECORD_BYTES)?;
    let record: PointerCreatedRecordV2 =
        serde_json::from_slice(&bytes).map_err(|_| "WAKE_OFFLINE_POINTER_RECORD_INVALID")?;
    if serde_json::to_vec(&record).map_err(|_| "WAKE_OFFLINE_POINTER_RECORD_INVALID")? != bytes {
        return Err("WAKE_OFFLINE_POINTER_RECORD_INVALID".into());
    }
    let mut hash = Sha256::new();
    hash.update(CREATED_DOMAIN);
    hash.update(bytes);
    Ok((record, format!("{:x}", hash.finalize())))
}

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
        pointer_plan_for(
            &self.intent,
            &self.intent_digest,
            self.stage_binding_for_recovery()?,
        )
    }

    pub(crate) fn checked_initial_pointer_plan(&self) -> Result<OfflinePointerPlanV2> {
        self.revalidate_inputs()?;
        if !self.pointer_namespace_attempted || self.pointer_namespace_digest.is_none() {
            return Err("WAKE_OFFLINE_POINTER_ATTENTION".into());
        }
        self.pointer_plan()
    }

    pub fn pointer_record_sha256(&self) -> Option<&str> {
        self.pointer_digest.as_deref()
    }

    pub(crate) fn flush_initial_pointer_parent(&self, point: ParentFlushPoint) -> Result<()> {
        self.revalidate_inputs()?;
        self.root_flush
            .flush(self.root, &self.intent.protected_root, point)?;
        self.revalidate_inputs()
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

    fn pointer_inventory(&self, attempted: bool, created: bool) -> Result<()> {
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
        slots[InstallPhaseV1::PointerSwitched.index()] = created;
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
        expected_attempt_for(&self.intent, &self.intent_digest, &published_digest, plan)
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
        self.pointer_inventory(false, false)?;
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
        self.pointer_inventory(true, false)?;
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
        self.check_initial_pointer_namespace(plan, published, false)
    }

    fn check_initial_pointer_namespace(
        &mut self,
        plan: &OfflinePointerPlanV2,
        published: &mut BrokerPublishedReceiptV2<'_>,
        created: bool,
    ) -> Result<()> {
        if (!created && self.phase() != "PUBLISHED")
            || !matches!(self.phase(), "PUBLISHED" | "POINTER_SWITCHED")
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
        self.pointer_inventory(true, created)?;
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

    pub(crate) fn record_initial_pointer_created(
        &mut self,
        switched: &mut crate::install_broker_reader::ProtectedInstallPinV2<'_>,
    ) -> Result<()> {
        if self.phase() != "PUBLISHED" || self.pointer_finish_attempted {
            return Err("WAKE_OFFLINE_POINTER_ATTENTION".into());
        }
        switched.revalidate(self)?;
        let plan = self.checked_initial_pointer_plan()?;
        self.check_initial_pointer_namespace(&plan, switched.published_mut(), false)?;
        let binding = switched.pointer_binding().clone();
        let stage = self.stage_binding_for_recovery()?.clone();
        if !valid_pointer_binding(&binding, &self.intent, &stage, plan.sha256()) {
            return Err("WAKE_OFFLINE_POINTER_RECORD_INVALID".into());
        }
        let published_digest = self.exact_published_record()?;
        let attempt_digest = self
            .pointer_namespace_digest
            .as_deref()
            .ok_or("WAKE_OFFLINE_POINTER_ATTENTION")?;
        let expected = expected_created_for(
            &self.intent,
            &self.intent_digest,
            &published_digest,
            attempt_digest,
            &binding,
            &stage,
        );
        self.pointer_finish_attempted = true;
        let mut file = open_relative(
            self.root,
            &record_name(1, InstallPhaseV1::PointerSwitched),
            FILE_CREATE,
        )
        .map_err(|_| "WAKE_OFFLINE_POINTER_ATTENTION")?;
        file_info_bounded(&file, Some(0), MAX_OFFLINE_RECORD_BYTES)?;
        let bytes =
            serde_json::to_vec(&expected).map_err(|_| "WAKE_OFFLINE_POINTER_RECORD_INVALID")?;
        if bytes.is_empty() || bytes.len() > MAX_OFFLINE_RECORD_BYTES {
            return Err("WAKE_OFFLINE_POINTER_RECORD_INVALID".into());
        }
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "WAKE_LEDGER_WRITE_FAILED")?;
        let (actual, digest) = read_created(&mut file)?;
        if actual != expected {
            return Err("WAKE_OFFLINE_POINTER_RECORD_INVALID".into());
        }
        self.root_flush.flush(
            self.root,
            &self.intent.protected_root,
            ParentFlushPoint::LaterLedgerPhase,
        )?;
        self.check_initial_pointer_namespace(&plan, switched.published_mut(), true)?;
        switched.revalidate(self)?;
        if read_created(&mut file)? != (expected, digest.clone()) {
            return Err("WAKE_OFFLINE_POINTER_RECORD_INVALID".into());
        }
        self.pointer_binding = Some(binding);
        self.pointer_digest = Some(digest);
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
