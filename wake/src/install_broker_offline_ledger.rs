//! Explicit offline V2 INTENT, staging and publication transaction boundary.
//!
//! Shares the V1 ledger namespace and global lock, but never interprets a V1
//! PID or process snapshot as an offline migration. The production admission
//! entry point still refuses before INTENT because independent launch-entry
//! quarantine is not implemented. Disposable fixtures exercise the create-once
//! engine through physical publication. Restart recovery and reader/owner
//! cutover remain incomplete; this source does not activate an installation.

use super::*;
use crate::{
    install_broker_approval::PackageManifestV1,
    install_broker_ci_fetch::require_broker_owned_offline_exact_ci,
    install_broker_durability::RetainedDirectoryWriterV2,
    install_broker_offline_pins::{
        OfflineMigrationObjectPathsV2, OfflineMigrationPinsV2, OfflineOldStoreLockSetV2,
        full_identity_of_handle,
    },
    install_broker_offline_v2::{
        CheckedOfflinePairV2, OfflineDesiredStateV2, OfflineMigrationObservationV2,
        OfflineObjectIdentityV2,
    },
};
use std::sync::Arc;

const DOMAIN_V2: &[u8] = b"CatDeskWakeOfflineInstallLedgerV2\0";
// V1 retains its 4-KiB limit. Full V2 object identities in STAGED need their
// own fixed bound; earlier V2 INTENT/marker canonical bytes stay unchanged.
const MAX_OFFLINE_RECORD_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageBindingV2 {
    pub staging_directory: OfflineObjectIdentityV2,
    pub packages_directory: OfflineObjectIdentityV2,
    pub candidate_directory: OfflineObjectIdentityV2,
    pub manifest_file: OfflineObjectIdentityV2,
    pub artifacts: [OfflineObjectIdentityV2; 4],
}

fn valid_stage_binding_v2(stage: &StageBindingV2, root: &OfflineObjectIdentityV2) -> bool {
    let objects = [
        &stage.staging_directory,
        &stage.packages_directory,
        &stage.candidate_directory,
        &stage.manifest_file,
        &stage.artifacts[0],
        &stage.artifacts[1],
        &stage.artifacts[2],
        &stage.artifacts[3],
    ];
    objects.iter().enumerate().all(|(index, object)| {
        crate::install_broker_offline_v2::valid_object(object)
            && object.full.volume_serial == root.full.volume_serial
            && object.legacy.volume_serial == root.legacy.volume_serial
            && object.full != root.full
            && objects[..index]
                .iter()
                .all(|prior| prior.full != object.full)
    })
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stage_binding: Option<StageBindingV2>,
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
            stage_binding: None,
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
    let (record, digest) = read_offline_record(file)?;
    if record.phase != "INTENT"
        || record.prior_record_sha256 != "0".repeat(64)
        || record.stage_binding.is_some()
    {
        return Err("WAKE_OFFLINE_LEDGER_RECORD_INVALID".into());
    }
    Ok((record, digest))
}

fn read_offline_record(file: &mut File) -> Result<(OfflineIntentRecordV2, String)> {
    file_info_bounded(file, None, MAX_OFFLINE_RECORD_BYTES)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    let mut bytes = Vec::new();
    file.take(MAX_OFFLINE_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    file_info_bounded(file, Some(bytes.len()), MAX_OFFLINE_RECORD_BYTES)?;
    let record: OfflineIntentRecordV2 =
        serde_json::from_slice(&bytes).map_err(|_| "WAKE_OFFLINE_LEDGER_RECORD_INVALID")?;
    if serde_json::to_vec(&record).map_err(|_| "WAKE_OFFLINE_LEDGER_RECORD_INVALID")? != bytes
        || record.schema_version != 2
        || record.install_generation != 1
        || record.purpose != "catdesk-wake-offline-install-ledger"
        || !matches!(
            record.phase.as_str(),
            "INTENT" | "STAGE_STARTED" | "STAGED" | "PUBLICATION_ATTEMPT" | "PUBLISHED"
        )
        || match (&record.stage_binding, record.phase.as_str()) {
            (Some(stage), "STAGED" | "PUBLICATION_ATTEMPT" | "PUBLISHED") => {
                !valid_stage_binding_v2(stage, &record.protected_root)
            }
            (None, "INTENT" | "STAGE_STARTED") => false,
            _ => true,
        }
        || !lowercase_hex(&record.prior_record_sha256, 64)
    {
        return Err("WAKE_OFFLINE_LEDGER_RECORD_INVALID".into());
    }
    let mut digest = Sha256::new();
    digest.update(DOMAIN_V2);
    digest.update(&bytes);
    Ok((record, format!("{:x}", digest.finalize())))
}

fn require_offline_root(root: &OwnedHandle, expected: &OfflineObjectIdentityV2) -> Result<()> {
    if root_info(root)? != expected.legacy
        || full_identity_of_handle(root.as_raw_handle())? != expected.full
    {
        return Err("WAKE_OFFLINE_LEDGER_ROOT_CHANGED".into());
    }
    Ok(())
}

fn require_offline_inventory(root: &OwnedHandle, stage_started: bool) -> Result<()> {
    let inventory = ledger_inventory(root)?;
    let mut slots = [false; LEDGER_SLOT_COUNT];
    slots[InstallPhaseV1::Intent.index()] = true;
    slots[InstallPhaseV1::StageStarted.index()] = stage_started;
    if inventory.len() != 1 || inventory.get(&1) != Some(&slots) {
        return Err("WAKE_OFFLINE_LEDGER_RECORD_CHANGED".into());
    }
    Ok(())
}

/// There is no constructor from a supplied hash or observation: selected
/// locks alone cannot close the historical launcher inventory. Keep this
/// explicit refusal until an independently reviewed retained proof exists.
fn require_offline_launch_quarantine() -> Result<()> {
    Err("WAKE_OFFLINE_LAUNCH_QUARANTINE_UNVERIFIED".into())
}

fn pending_offline_intent(root: &OwnedHandle) -> Result<Option<OfflineIntentRecordV2>> {
    let inventory = ledger_inventory(root)?;
    if inventory.is_empty() {
        return Ok(None);
    }
    let mut intent_slots = [false; LEDGER_SLOT_COUNT];
    intent_slots[InstallPhaseV1::Intent.index()] = true;
    let mut started_slots = intent_slots;
    started_slots[InstallPhaseV1::StageStarted.index()] = true;
    if inventory.len() != 1
        || !matches!(inventory.get(&1), Some(slots) if *slots == intent_slots || *slots == started_slots)
    {
        return Err("WAKE_OFFLINE_LEDGER_LATER_OR_MIXED_HISTORY".into());
    }
    let mut intent_file = open_optional(root, &record_name(1, InstallPhaseV1::Intent))?
        .ok_or("WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    let (intent, digest) = read_offline_intent(&mut intent_file)?;
    if inventory.get(&1) == Some(&started_slots) {
        let mut marker = open_optional(root, &record_name(1, InstallPhaseV1::StageStarted))?
            .ok_or("WAKE_LEDGER_RECORD_UNAVAILABLE")?;
        let (actual, _) = read_offline_record(&mut marker)?;
        let mut expected = intent.clone();
        expected.phase = "STAGE_STARTED".into();
        expected.prior_record_sha256 = digest;
        if actual != expected {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        return Err("WAKE_OFFLINE_STAGE_ATTENTION".into());
    }
    Ok(Some(intent))
}

fn write_offline_record(file: &mut File, record: &OfflineIntentRecordV2) -> Result<String> {
    file_info_bounded(file, Some(0), MAX_OFFLINE_RECORD_BYTES)?;
    let bytes = serde_json::to_vec(record).map_err(|_| "WAKE_LEDGER_ENCODING_INVALID")?;
    if bytes.is_empty() || bytes.len() > MAX_OFFLINE_RECORD_BYTES {
        return Err("WAKE_LEDGER_ENCODING_INVALID".into());
    }
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "WAKE_LEDGER_WRITE_FAILED")?;
    let (actual, digest) = read_offline_record(file)?;
    if actual != *record {
        return Err("WAKE_OFFLINE_LEDGER_RECORD_INVALID".into());
    }
    Ok(digest)
}

/// Owns the same global and intent locks as V1 plus the retained policy,
/// offline-role and old-store borrows captured by its private validator.
/// No public constructor accepts a caller-selected key or validation closure.
pub struct OfflineLedgerLeaseV2<'a, 'p> {
    root: &'a OwnedHandle,
    _global_lock: File,
    intent_file: File,
    intent: OfflineIntentRecordV2,
    intent_digest: String,
    revalidate: Box<dyn Fn() -> Result<()> + 'p>,
    root_flush: OfflineRootFlush,
    manifest: PackageManifestV1,
    policy: Option<&'p BrokerProtectedPolicyV1>,
    stage_attempted: bool,
    stage_recorded: bool,
    stage_started_digest: Option<String>,
    staged_binding: Option<StageBindingV2>,
    staged_digest: Option<String>,
    publication_attempted: bool,
    publication_attempt_digest: Option<String>,
    publication_finish_attempted: bool,
    published_digest: Option<String>,
}

enum OfflineRootFlush {
    Retained(Arc<RetainedDirectoryWriterV2>),
    #[cfg(test)]
    FixtureReopen,
}

impl OfflineRootFlush {
    fn require_root(&self, root: &OwnedHandle, expected: &OfflineObjectIdentityV2) -> Result<()> {
        match self {
            Self::Retained(writer) => writer.require_root(root, expected),
            #[cfg(test)]
            Self::FixtureReopen => require_offline_root(root, expected),
        }
    }

    fn flush(
        &self,
        root: &OwnedHandle,
        expected: &OfflineObjectIdentityV2,
        point: ParentFlushPoint,
    ) -> Result<()> {
        match self {
            Self::Retained(writer) => writer.flush(root, expected, point),
            #[cfg(test)]
            Self::FixtureReopen => flush_pinned_directory(root, &expected.legacy, point),
        }
    }
}

struct OfflineMutationChecks<'p> {
    revalidate: Box<dyn Fn() -> Result<()> + 'p>,
    root_flush: OfflineRootFlush,
}

impl<'a> BrokerLedgerGuard<'a> {
    /// Future fixed-broker V2 admission. Current physical observations must
    /// originate in the broker while its global lock/clock are retained.
    /// The missing independent quarantine proof refuses before CI or INTENT;
    /// signing a matching supplied observation never bypasses that gate.
    pub fn accept_offline_v2<'p>(
        self,
        policy: &'p BrokerProtectedPolicyV1,
        anchor_wire: &'p [u8],
        approval_wire: &'p [u8],
        observed: &OfflineMigrationObservationV2,
        paths: &OfflineMigrationObjectPathsV2<'_>,
        stores: &'p OfflineOldStoreLockSetV2,
    ) -> Result<OfflineLedgerLeaseV2<'a, 'p>> {
        // Writer acquisition occurs only after this guard owns the matching
        // global ledger lock. Diagnostic callers keep open_read_only unchanged.
        let pins = Arc::new(policy.open_offline_transaction_pins(&self, paths)?);
        let writer = pins.transaction_root_writer()?;
        writer.require_root(self.root, &observed.target_protected_root)?;
        let accepted_at = pending_offline_intent(self.root)?
            .map_or(observed.now_utc_seconds, |record| {
                record.accepted_at_utc_seconds
            });
        if observed.now_utc_seconds < accepted_at {
            return Err("WAKE_OFFLINE_LEDGER_CLOCK_ROLLBACK".into());
        }
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
        let retained_pins = Arc::clone(&pins);
        let revalidate = Box::new(move || {
            let current = policy.inspect_offline_pair_pinned(
                anchor_wire,
                approval_wire,
                &original,
                &retained_pins,
                stores,
            )?;
            if current.readback() != &expected_pair {
                return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
            }
            Ok(())
        });
        let mut lease = self.accept_offline_checked_with_root(
            approved.checked(),
            observed,
            OfflineMutationChecks {
                revalidate,
                root_flush: OfflineRootFlush::Retained(writer),
            },
            require_offline_launch_quarantine,
            || require_broker_owned_offline_exact_ci(&approved),
        )?;
        lease.policy = Some(policy);
        Ok(lease)
    }

    #[cfg(test)]
    fn accept_offline_checked<'p>(
        self,
        pair: &CheckedOfflinePairV2,
        observed: &OfflineMigrationObservationV2,
        revalidate: Box<dyn Fn() -> Result<()> + 'p>,
        admit: impl FnOnce() -> Result<()>,
        check_ci: impl FnOnce() -> Result<()>,
    ) -> Result<OfflineLedgerLeaseV2<'a, 'p>> {
        self.accept_offline_checked_with_root(
            pair,
            observed,
            OfflineMutationChecks {
                revalidate,
                root_flush: OfflineRootFlush::FixtureReopen,
            },
            admit,
            check_ci,
        )
    }

    fn accept_offline_checked_with_root<'p>(
        self,
        pair: &CheckedOfflinePairV2,
        observed: &OfflineMigrationObservationV2,
        checks: OfflineMutationChecks<'p>,
        admit: impl FnOnce() -> Result<()>,
        check_ci: impl FnOnce() -> Result<()>,
    ) -> Result<OfflineLedgerLeaseV2<'a, 'p>> {
        let OfflineMutationChecks {
            revalidate,
            root_flush,
        } = checks;
        let accepted_at = pair.checked_at_utc_seconds();
        require_offline_root(self.root, &observed.target_protected_root)?;
        root_flush.require_root(self.root, &observed.target_protected_root)?;
        if observed.now_utc_seconds < accepted_at {
            return Err("WAKE_OFFLINE_LEDGER_CLOCK_ROLLBACK".into());
        }
        let binding = OfflineIntentRecordV2::from_checked(pair, accepted_at);
        let existing = pending_offline_intent(self.root)?;
        if existing.as_ref().is_some_and(|record| *record != binding)
            || binding.protected_root != observed.target_protected_root
        {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        if existing.is_none() && accepted_at != observed.now_utc_seconds {
            return Err("WAKE_OFFLINE_LEDGER_FRESH_TIME_MISMATCH".into());
        }
        revalidate()?;
        admit()?;
        check_ci()?;
        revalidate()?;
        require_offline_root(self.root, &binding.protected_root)?;
        // Recheck after external admission/CI work, before the first create.
        if pending_offline_intent(self.root)? != existing {
            return Err("WAKE_OFFLINE_LEDGER_RECORD_CHANGED".into());
        }
        let name = record_name(1, InstallPhaseV1::Intent);
        let mut file = if existing.is_some() {
            open_optional(self.root, &name)?.ok_or("WAKE_LEDGER_RECORD_UNAVAILABLE")?
        } else {
            open_relative(self.root, &name, FILE_CREATE).map_err(|_| "WAKE_LEDGER_CREATE_FAILED")?
        };
        let digest = if existing.is_some() {
            let (actual, digest) = read_offline_intent(&mut file)?;
            if actual != binding {
                return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
            }
            digest
        } else {
            write_offline_record(&mut file, &binding)?
        };
        root_flush.flush(self.root, &binding.protected_root, ParentFlushPoint::Intent)?;
        revalidate()?;
        require_offline_root(self.root, &binding.protected_root)?;
        require_offline_inventory(self.root, false)?;
        if read_offline_intent(&mut file)? != (binding.clone(), digest.clone()) {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        Ok(OfflineLedgerLeaseV2 {
            root: self.root,
            _global_lock: self.global_lock,
            intent_file: file,
            intent: binding,
            intent_digest: digest,
            revalidate,
            root_flush,
            manifest: pair.approval().manifest.clone(),
            policy: None,
            stage_attempted: false,
            stage_recorded: false,
            stage_started_digest: None,
            staged_binding: None,
            staged_digest: None,
            publication_attempted: false,
            publication_attempt_digest: None,
            publication_finish_attempted: false,
            published_digest: None,
        })
    }
}

impl<'p> OfflineLedgerLeaseV2<'_, 'p> {
    pub fn phase(&self) -> &'static str {
        if self.published_digest.is_some() {
            "PUBLISHED"
        } else if self.staged_binding.is_some() {
            "STAGED"
        } else if self.stage_recorded {
            "STAGE_STARTED"
        } else {
            "INTENT"
        }
    }

    pub(crate) fn protected_root(&self) -> &OwnedHandle {
        self.root
    }
    pub(crate) fn protected_root_identity(&self) -> &OfflineObjectIdentityV2 {
        &self.intent.protected_root
    }
    pub(crate) fn approved_manifest(&self) -> &PackageManifestV1 {
        &self.manifest
    }
    pub(crate) fn approved_manifest_sha256(&self) -> &str {
        &self.intent.manifest_sha256
    }
    pub(crate) fn intent_record_sha256(&self) -> &str {
        &self.intent_digest
    }
    pub(crate) fn policy(&self) -> Option<&'p BrokerProtectedPolicyV1> {
        self.policy
    }
    pub(crate) fn revalidate_inputs(&self) -> Result<()> {
        (self.revalidate)()?;
        self.root_flush
            .require_root(self.root, &self.intent.protected_root)?;
        require_offline_root(self.root, &self.intent.protected_root)
    }

    /// Write/flush the no-replay boundary before any future physical stage
    /// creation. This is not a stage receipt. A failed attempt cannot be retried
    /// on this lease, and a retained marker is sticky attention on reopen.
    pub fn record_stage_started(&mut self) -> Result<()> {
        if self.stage_attempted {
            return Err("WAKE_OFFLINE_STAGE_ATTENTION".into());
        }
        (self.revalidate)()?;
        require_offline_root(self.root, &self.intent.protected_root)?;
        if read_offline_intent(&mut self.intent_file)?
            != (self.intent.clone(), self.intent_digest.clone())
        {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        let inventory = ledger_inventory(self.root)?;
        let mut slots = [false; LEDGER_SLOT_COUNT];
        slots[InstallPhaseV1::Intent.index()] = true;
        if inventory.len() != 1 || inventory.get(&1) != Some(&slots) {
            return Err("WAKE_OFFLINE_STAGE_ATTENTION".into());
        }
        self.stage_attempted = true;
        let mut marker = self.intent.clone();
        marker.phase = "STAGE_STARTED".into();
        marker.prior_record_sha256 = self.intent_digest.clone();
        let mut file = open_relative(
            self.root,
            &record_name(1, InstallPhaseV1::StageStarted),
            FILE_CREATE,
        )
        .map_err(|_| "WAKE_OFFLINE_STAGE_ATTENTION")?;
        let digest = write_offline_record(&mut file, &marker)?;
        self.root_flush.flush(
            self.root,
            &self.intent.protected_root,
            ParentFlushPoint::StageStarted,
        )?;
        (self.revalidate)()?;
        require_offline_root(self.root, &self.intent.protected_root)?;
        require_offline_inventory(self.root, true)?;
        if read_offline_record(&mut file)? != (marker, digest.clone()) {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        self.stage_recorded = true;
        self.stage_started_digest = Some(digest);
        Ok(())
    }

    /// Only the opaque fresh-create receipt may supply STAGED identities.
    pub(crate) fn record_staged(
        &mut self,
        receipt: &mut crate::install_broker_stage_writer::BrokerStageReceiptV2<'_>,
    ) -> Result<bool> {
        if !self.stage_recorded || self.publication_attempted {
            return Err("WAKE_OFFLINE_STAGE_ATTENTION".into());
        }
        self.revalidate_inputs()?;
        let binding = receipt.binding_for_ledger(
            self.root,
            &self.intent.protected_root,
            &self.intent_digest,
            &self.manifest,
            &self.intent.manifest_sha256,
        )?;
        if !valid_stage_binding_v2(&binding, &self.intent.protected_root)
            || self
                .staged_binding
                .as_ref()
                .is_some_and(|prior| *prior != binding)
        {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        let mut marker_file =
            open_optional(self.root, &record_name(1, InstallPhaseV1::StageStarted))?
                .ok_or("WAKE_LEDGER_RECORD_UNAVAILABLE")?;
        let mut marker = self.intent.clone();
        marker.phase = "STAGE_STARTED".into();
        marker.prior_record_sha256 = self.intent_digest.clone();
        let (actual_marker, marker_digest) = read_offline_record(&mut marker_file)?;
        if actual_marker != marker || Some(&marker_digest) != self.stage_started_digest.as_ref() {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        let mut staged = marker;
        staged.phase = "STAGED".into();
        staged.prior_record_sha256 = marker_digest;
        staged.stage_binding = Some(binding.clone());
        let inventory_before = ledger_inventory(self.root)?;
        let mut before_slots = [false; LEDGER_SLOT_COUNT];
        before_slots[InstallPhaseV1::Intent.index()] = true;
        before_slots[InstallPhaseV1::StageStarted.index()] = true;
        let mut existing_slots = before_slots;
        existing_slots[InstallPhaseV1::Staged.index()] = true;
        if inventory_before.len() != 1
            || !matches!(inventory_before.get(&1),
            Some(slots) if *slots == before_slots || *slots == existing_slots)
        {
            return Err("WAKE_OFFLINE_LEDGER_RECORD_CHANGED".into());
        }
        let name = record_name(1, InstallPhaseV1::Staged);
        let (mut file, created) = create_or_reopen(self.root, &name)?;
        let digest = if created {
            write_offline_record(&mut file, &staged)?
        } else {
            let (record, digest) = read_offline_record(&mut file)?;
            if record != staged {
                return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
            }
            digest
        };
        self.root_flush.flush(
            self.root,
            &self.intent.protected_root,
            ParentFlushPoint::Staged,
        )?;
        self.revalidate_inputs()?;
        if receipt.binding_for_ledger(
            self.root,
            &self.intent.protected_root,
            &self.intent_digest,
            &self.manifest,
            &self.intent.manifest_sha256,
        )? != binding
            || read_offline_record(&mut file)? != (staged, digest.clone())
        {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        let inventory = ledger_inventory(self.root)?;
        let mut slots = [false; LEDGER_SLOT_COUNT];
        for phase in [
            InstallPhaseV1::Intent,
            InstallPhaseV1::StageStarted,
            InstallPhaseV1::Staged,
        ] {
            slots[phase.index()] = true;
        }
        if inventory.len() != 1 || inventory.get(&1) != Some(&slots) {
            return Err("WAKE_OFFLINE_LEDGER_RECORD_CHANGED".into());
        }
        let advanced = self.staged_binding.is_none();
        self.staged_binding = Some(binding);
        self.staged_digest = Some(digest);
        Ok(advanced)
    }

    fn publication_inventory(&self, attempt: bool, published: bool) -> Result<()> {
        let mut expected = [false; LEDGER_SLOT_COUNT];
        for phase in [
            InstallPhaseV1::Intent,
            InstallPhaseV1::StageStarted,
            InstallPhaseV1::Staged,
        ] {
            expected[phase.index()] = true;
        }
        expected[PUBLICATION_ATTEMPT_SLOT] = attempt;
        expected[InstallPhaseV1::Published.index()] = published;
        let actual = ledger_inventory(self.root)?;
        if actual.len() != 1 || actual.get(&1) != Some(&expected) {
            return Err("WAKE_OFFLINE_PUBLICATION_ATTENTION".into());
        }
        Ok(())
    }

    fn exact_staged_record(&mut self) -> Result<(OfflineIntentRecordV2, String)> {
        self.revalidate_inputs()?;
        if read_offline_intent(&mut self.intent_file)?
            != (self.intent.clone(), self.intent_digest.clone())
        {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        let mut marker = self.intent.clone();
        marker.phase = "STAGE_STARTED".into();
        marker.prior_record_sha256 = self.intent_digest.clone();
        let mut marker_file =
            open_optional(self.root, &record_name(1, InstallPhaseV1::StageStarted))?
                .ok_or("WAKE_LEDGER_RECORD_UNAVAILABLE")?;
        let (actual, marker_digest) = read_offline_record(&mut marker_file)?;
        if actual != marker || Some(&marker_digest) != self.stage_started_digest.as_ref() {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        marker.phase = "STAGED".into();
        marker.prior_record_sha256 = marker_digest;
        marker.stage_binding = Some(
            self.staged_binding
                .clone()
                .ok_or("WAKE_OFFLINE_PUBLICATION_ATTENTION")?,
        );
        let mut file = open_optional(self.root, &record_name(1, InstallPhaseV1::Staged))?
            .ok_or("WAKE_LEDGER_RECORD_UNAVAILABLE")?;
        let (actual, digest) = read_offline_record(&mut file)?;
        if actual != marker || Some(&digest) != self.staged_digest.as_ref() {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        Ok((actual, digest))
    }

    /// Durable no-retry boundary before releasing fresh stage pins. The
    /// serialized marker alone cannot authorize rename or PUBLISHED.
    pub(crate) fn record_publication_attempt(
        &mut self,
        receipt: &mut crate::install_broker_stage_writer::BrokerStageReceiptV2<'_>,
    ) -> Result<()> {
        if self.phase() != "STAGED" || self.publication_attempted {
            return Err("WAKE_OFFLINE_PUBLICATION_ATTENTION".into());
        }
        let binding = receipt.binding_for_ledger(
            self.root,
            &self.intent.protected_root,
            &self.intent_digest,
            &self.manifest,
            &self.intent.manifest_sha256,
        )?;
        if self.staged_binding.as_ref() != Some(&binding) {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        let (mut marker, staged_digest) = self.exact_staged_record()?;
        self.publication_inventory(false, false)?;
        marker.phase = "PUBLICATION_ATTEMPT".into();
        marker.prior_record_sha256 = staged_digest;
        // Even a failed create is not retried on this live lease.
        self.publication_attempted = true;
        let mut file = open_relative(self.root, &publication_attempt_name(1), FILE_CREATE)
            .map_err(|_| "WAKE_OFFLINE_PUBLICATION_ATTENTION")?;
        let digest = write_offline_record(&mut file, &marker)?;
        self.root_flush.flush(
            self.root,
            &self.intent.protected_root,
            ParentFlushPoint::PublicationAttempt,
        )?;
        self.revalidate_inputs()?;
        self.exact_staged_record()?;
        self.publication_inventory(true, false)?;
        if read_offline_record(&mut file)? != (marker, digest.clone())
            || receipt.binding_for_ledger(
                self.root,
                &self.intent.protected_root,
                &self.intent_digest,
                &self.manifest,
                &self.intent.manifest_sha256,
            )? != binding
        {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        self.publication_attempt_digest = Some(digest);
        Ok(())
    }

    fn exact_publication_attempt(&mut self) -> Result<(OfflineIntentRecordV2, String)> {
        let (mut expected, staged_digest) = self.exact_staged_record()?;
        expected.phase = "PUBLICATION_ATTEMPT".into();
        expected.prior_record_sha256 = staged_digest;
        let mut file = open_optional(self.root, &publication_attempt_name(1))?
            .ok_or("WAKE_OFFLINE_PUBLICATION_ATTENTION")?;
        let (actual, digest) = read_offline_record(&mut file)?;
        if actual != expected || Some(&digest) != self.publication_attempt_digest.as_ref() {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        Ok((actual, digest))
    }

    pub(crate) fn revalidate_publication_attempt(&mut self) -> Result<()> {
        if self.phase() != "STAGED"
            || !self.publication_attempted
            || self.publication_finish_attempted
            || self.publication_attempt_digest.is_none()
        {
            return Err("WAKE_OFFLINE_PUBLICATION_ATTENTION".into());
        }
        self.exact_publication_attempt()?;
        self.publication_inventory(true, false)
    }

    /// Only a retained moved-package receipt may finish this live attempt.
    /// Errors leave the marker/record intact and never authorize another rename.
    pub(crate) fn record_published(
        &mut self,
        receipt: &mut crate::install_broker_stage_writer::BrokerPublishedReceiptV2<'_>,
    ) -> Result<()> {
        if self.phase() != "STAGED"
            || !self.publication_attempted
            || self.publication_finish_attempted
            || self.publication_attempt_digest.is_none()
        {
            return Err("WAKE_OFFLINE_PUBLICATION_ATTENTION".into());
        }
        let binding = receipt.binding_for_ledger(
            self.root,
            &self.intent.protected_root,
            &self.intent_digest,
            &self.manifest,
            &self.intent.manifest_sha256,
        )?;
        if self.staged_binding.as_ref() != Some(&binding) {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        let (mut published, attempt_digest) = self.exact_publication_attempt()?;
        self.publication_inventory(true, false)?;
        published.phase = "PUBLISHED".into();
        published.prior_record_sha256 = attempt_digest;
        self.publication_finish_attempted = true;
        let mut file = open_relative(
            self.root,
            &record_name(1, InstallPhaseV1::Published),
            FILE_CREATE,
        )
        .map_err(|_| "WAKE_OFFLINE_PUBLICATION_ATTENTION")?;
        let digest = write_offline_record(&mut file, &published)?;
        self.root_flush.flush(
            self.root,
            &self.intent.protected_root,
            ParentFlushPoint::LaterLedgerPhase,
        )?;
        self.revalidate_inputs()?;
        self.exact_publication_attempt()?;
        self.publication_inventory(true, true)?;
        if read_offline_record(&mut file)? != (published, digest.clone())
            || receipt.binding_for_ledger(
                self.root,
                &self.intent.protected_root,
                &self.intent_digest,
                &self.manifest,
                &self.intent.manifest_sha256,
            )? != binding
        {
            return Err("WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT".into());
        }
        self.published_digest = Some(digest);
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn begin_offline_fixture<'a>(
    guard: BrokerLedgerGuard<'a>,
    pair: &CheckedOfflinePairV2,
    observed: &OfflineMigrationObservationV2,
) -> Result<OfflineLedgerLeaseV2<'a, 'static>> {
    guard.accept_offline_checked(pair, observed, Box::new(|| Ok(())), || Ok(()), || Ok(()))
}

#[cfg(test)]
pub(crate) fn begin_offline_writer_fixture<'a>(
    guard: BrokerLedgerGuard<'a>,
    pair: &CheckedOfflinePairV2,
    observed: &OfflineMigrationObservationV2,
    writer: Arc<RetainedDirectoryWriterV2>,
) -> Result<OfflineLedgerLeaseV2<'a, 'static>> {
    guard.accept_offline_checked_with_root(
        pair,
        observed,
        OfflineMutationChecks {
            revalidate: Box::new(|| Ok(())),
            root_flush: OfflineRootFlush::Retained(writer),
        },
        || Ok(()),
        || Ok(()),
    )
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

    fn begin_fixture<'a>(
        guard: BrokerLedgerGuard<'a>,
        pair: &CheckedOfflinePairV2,
        observed: &OfflineMigrationObservationV2,
    ) -> Result<OfflineLedgerLeaseV2<'a, 'static>> {
        guard.accept_offline_checked(pair, observed, Box::new(|| Ok(())), || Ok(()), || Ok(()))
    }

    #[test]
    fn staged_full_ids_reject_aliases_without_assuming_legacy_ids_are_unique() {
        use crate::install_broker_offline_v2::FullFileIdentityV2;
        let (anchor, _, _, _) = signed_test::fixture();
        let root = anchor.target_protected_root;
        let object = |index: u32| OfflineObjectIdentityV2 {
            legacy: root.legacy.clone(),
            full: FullFileIdentityV2 {
                volume_serial: root.full.volume_serial,
                file_id_hex: format!("ffffffffffffffffffffffff{index:08x}"),
            },
        };
        let mut stage = StageBindingV2 {
            staging_directory: object(1),
            packages_directory: object(2),
            candidate_directory: object(3),
            manifest_file: object(4),
            artifacts: std::array::from_fn(|index| object(5 + index as u32)),
        };
        assert!(
            valid_stage_binding_v2(&stage, &root),
            "full identities are the distinctness premise"
        );
        stage.artifacts[0].full = stage.candidate_directory.full.clone();
        assert!(!valid_stage_binding_v2(&stage, &root));
        stage.artifacts[0] = object(5);
        stage.artifacts[0].full.volume_serial += 1;
        assert!(!valid_stage_binding_v2(&stage, &root));
    }

    #[test]
    fn mismatched_retained_writer_refuses_before_intent_and_exact_flush_failure_reopens() {
        use crate::install_broker_durability::fail_next_flush;
        let (first_path, first, _, _, first_observed, _) = fixture();
        let (path, root, anchor, approval, observed, key) = fixture();
        let pair = checked(
            &anchor,
            &approval,
            &observed,
            &key,
            observed.now_utc_seconds,
        )
        .unwrap();
        let wrong = Arc::new(
            RetainedDirectoryWriterV2::acquire(&first, &first_observed.target_protected_root)
                .unwrap(),
        );
        assert_eq!(
            begin_offline_writer_fixture(
                BrokerLedgerGuard::acquire(&root).unwrap(),
                &pair,
                &observed,
                wrong
            )
            .err()
            .unwrap(),
            "WAKE_OFFLINE_TRANSACTION_ROOT_MISMATCH"
        );
        let intent = path.join(record_name(1, InstallPhaseV1::Intent));
        assert!(!intent.exists());
        let writer = Arc::new(
            RetainedDirectoryWriterV2::acquire(&root, &observed.target_protected_root).unwrap(),
        );
        fail_next_flush(ParentFlushPoint::Intent);
        assert_eq!(
            begin_offline_writer_fixture(
                BrokerLedgerGuard::acquire(&root).unwrap(),
                &pair,
                &observed,
                Arc::clone(&writer)
            )
            .err()
            .unwrap(),
            "WAKE_BROKER_PARENT_FLUSH_FAILED"
        );
        let bytes = std::fs::read(&intent).unwrap();
        let lease = begin_offline_writer_fixture(
            BrokerLedgerGuard::acquire(&root).unwrap(),
            &pair,
            &observed,
            Arc::clone(&writer),
        )
        .unwrap();
        assert_eq!(lease.phase(), "INTENT");
        drop(lease);
        drop(writer);
        assert_eq!(std::fs::read(&intent).unwrap(), bytes);
        drop(first);
        drop(root);
        super::super::tests::remove_fixture(&first_path, 1);
        super::super::tests::remove_fixture(&path, 1);
    }

    #[test]
    fn production_quarantine_and_independent_ci_refuse_before_intent() {
        use std::cell::Cell;
        let (root, handle, anchor, approval, observed, key) = fixture();
        let pair = checked(
            &anchor,
            &approval,
            &observed,
            &key,
            observed.now_utc_seconds,
        )
        .unwrap();
        let ci_called = Cell::new(false);
        assert_eq!(
            BrokerLedgerGuard::acquire(&handle)
                .unwrap()
                .accept_offline_checked(
                    &pair,
                    &observed,
                    Box::new(|| Ok(())),
                    require_offline_launch_quarantine,
                    || {
                        ci_called.set(true);
                        Ok(())
                    },
                )
                .err()
                .unwrap(),
            "WAKE_OFFLINE_LAUNCH_QUARANTINE_UNVERIFIED"
        );
        assert!(!ci_called.get(), "no network after missing quarantine");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        assert_eq!(
            BrokerLedgerGuard::acquire(&handle)
                .unwrap()
                .accept_offline_checked(
                    &pair,
                    &observed,
                    Box::new(|| Ok(())),
                    || Ok(()),
                    || Err("CI_FIXTURE_DENIED".into()),
                )
                .err()
                .unwrap(),
            "CI_FIXTURE_DENIED"
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        drop(handle);
        super::super::tests::remove_fixture(&root, 1);
    }

    #[test]
    fn offline_intent_create_once_retains_locks_and_reopens_without_rewriting() {
        let (root, handle, anchor, approval, mut observed, key) = fixture();
        let accepted_at = observed.now_utc_seconds;
        let pair = checked(&anchor, &approval, &observed, &key, accepted_at).unwrap();
        let path = root.join(record_name(1, InstallPhaseV1::Intent));
        let lease = begin_fixture(
            BrokerLedgerGuard::acquire(&handle).unwrap(),
            &pair,
            &observed,
        )
        .unwrap();
        assert_eq!(lease.phase(), "INTENT");
        assert!(BrokerLedgerGuard::acquire(&handle).is_err());
        assert!(std::fs::write(&path, b"replace").is_err());
        let digest = lease.intent_digest.clone();
        drop(lease);
        let bytes = std::fs::read(&path).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        observed.now_utc_seconds = anchor.expires_utc_seconds + 1;
        let resumed = begin_fixture(
            BrokerLedgerGuard::acquire(&handle).unwrap(),
            &pair,
            &observed,
        )
        .unwrap();
        assert_eq!(resumed.intent.accepted_at_utc_seconds, accepted_at);
        assert_eq!(resumed.intent_digest, digest);
        drop(resumed);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        drop(handle);
        super::super::tests::remove_fixture(&root, 1);
    }

    #[test]
    fn intent_parent_flush_failure_retains_exact_record_for_guarded_reopen() {
        use crate::install_broker_durability::fail_next_flush;
        let (root, handle, anchor, approval, observed, key) = fixture();
        let pair = checked(
            &anchor,
            &approval,
            &observed,
            &key,
            observed.now_utc_seconds,
        )
        .unwrap();
        fail_next_flush(ParentFlushPoint::Intent);
        assert!(
            begin_fixture(
                BrokerLedgerGuard::acquire(&handle).unwrap(),
                &pair,
                &observed
            )
            .is_err()
        );
        let path = root.join(record_name(1, InstallPhaseV1::Intent));
        let original = std::fs::read(&path).unwrap();
        let lease = begin_fixture(
            BrokerLedgerGuard::acquire(&handle).unwrap(),
            &pair,
            &observed,
        )
        .unwrap();
        assert_eq!(lease.phase(), "INTENT");
        drop(lease);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(
            !root
                .join(record_name(1, InstallPhaseV1::StageStarted))
                .exists()
        );
        drop(handle);
        super::super::tests::remove_fixture(&root, 1);
    }

    #[test]
    fn offline_stage_started_is_hash_bound_and_never_replayed() {
        let (root, handle, anchor, approval, observed, key) = fixture();
        let pair = checked(
            &anchor,
            &approval,
            &observed,
            &key,
            observed.now_utc_seconds,
        )
        .unwrap();
        let mut lease = begin_fixture(
            BrokerLedgerGuard::acquire(&handle).unwrap(),
            &pair,
            &observed,
        )
        .unwrap();
        let digest = lease.intent_digest.clone();
        lease.record_stage_started().unwrap();
        assert_eq!(lease.phase(), "STAGE_STARTED");
        assert_eq!(
            lease.record_stage_started().unwrap_err(),
            "WAKE_OFFLINE_STAGE_ATTENTION"
        );
        drop(lease);
        let path = root.join(record_name(1, InstallPhaseV1::StageStarted));
        let mut marker: OfflineIntentRecordV2 =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(marker.prior_record_sha256, digest);
        assert_eq!(
            begin_fixture(
                BrokerLedgerGuard::acquire(&handle).unwrap(),
                &pair,
                &observed
            )
            .err()
            .unwrap(),
            "WAKE_OFFLINE_STAGE_ATTENTION"
        );
        marker.prior_record_sha256 = "f".repeat(64);
        std::fs::write(&path, serde_json::to_vec(&marker).unwrap()).unwrap();
        assert_eq!(
            begin_fixture(
                BrokerLedgerGuard::acquire(&handle).unwrap(),
                &pair,
                &observed
            )
            .err()
            .unwrap(),
            "WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT"
        );
        drop(handle);
        super::super::tests::remove_fixture(&root, 1);
    }

    #[test]
    fn stage_marker_parent_flush_failure_is_sticky_on_same_and_next_lease() {
        use crate::install_broker_durability::fail_next_flush;
        let (root, handle, anchor, approval, observed, key) = fixture();
        let pair = checked(
            &anchor,
            &approval,
            &observed,
            &key,
            observed.now_utc_seconds,
        )
        .unwrap();
        let mut lease = begin_fixture(
            BrokerLedgerGuard::acquire(&handle).unwrap(),
            &pair,
            &observed,
        )
        .unwrap();
        fail_next_flush(ParentFlushPoint::StageStarted);
        assert!(lease.record_stage_started().is_err());
        assert_eq!(
            lease.phase(),
            "INTENT",
            "failed durability is not a recorded phase"
        );
        assert_eq!(
            lease.record_stage_started().unwrap_err(),
            "WAKE_OFFLINE_STAGE_ATTENTION"
        );
        drop(lease);
        let path = root.join(record_name(1, InstallPhaseV1::StageStarted));
        let marker = std::fs::read(&path).unwrap();
        assert_eq!(
            begin_fixture(
                BrokerLedgerGuard::acquire(&handle).unwrap(),
                &pair,
                &observed
            )
            .err()
            .unwrap(),
            "WAKE_OFFLINE_STAGE_ATTENTION"
        );
        assert_eq!(std::fs::read(&path).unwrap(), marker);
        drop(handle);
        super::super::tests::remove_fixture(&root, 1);
    }

    #[test]
    fn changed_proof_before_and_after_intent_never_returns_a_lease() {
        use std::cell::Cell;
        let (root, handle, anchor, approval, observed, key) = fixture();
        let pair = checked(
            &anchor,
            &approval,
            &observed,
            &key,
            observed.now_utc_seconds,
        )
        .unwrap();
        for failure_call in [1, 2, 3] {
            let calls = Cell::new(0);
            let result = BrokerLedgerGuard::acquire(&handle)
                .unwrap()
                .accept_offline_checked(
                    &pair,
                    &observed,
                    Box::new(|| {
                        let call = calls.get() + 1;
                        calls.set(call);
                        if call == failure_call {
                            Err("PIN_FIXTURE_CHANGED".into())
                        } else {
                            Ok(())
                        }
                    }),
                    || Ok(()),
                    || Ok(()),
                );
            assert_eq!(result.err().unwrap(), "PIN_FIXTURE_CHANGED");
            assert_eq!(
                root.join(record_name(1, InstallPhaseV1::Intent)).exists(),
                failure_call == 3
            );
            assert!(
                !root
                    .join(record_name(1, InstallPhaseV1::StageStarted))
                    .exists()
            );
        }
        drop(handle);
        super::super::tests::remove_fixture(&root, 1);
    }

    #[test]
    fn late_directory_change_cannot_return_intent_or_stage_success() {
        use std::cell::Cell;
        for during_stage in [false, true] {
            let (root, handle, anchor, approval, observed, key) = fixture();
            let pair = checked(
                &anchor,
                &approval,
                &observed,
                &key,
                observed.now_utc_seconds,
            )
            .unwrap();
            let calls = Cell::new(0);
            let late_path = root.join(record_name(2, InstallPhaseV1::Intent));
            let result = BrokerLedgerGuard::acquire(&handle)
                .unwrap()
                .accept_offline_checked(
                    &pair,
                    &observed,
                    Box::new(|| {
                        let call = calls.get() + 1;
                        calls.set(call);
                        if call == if during_stage { 5 } else { 3 } {
                            std::fs::write(&late_path, b"late unrelated generation").unwrap();
                        }
                        Ok(())
                    }),
                    || Ok(()),
                    || Ok(()),
                );
            if during_stage {
                let mut lease = result.unwrap();
                assert_eq!(
                    lease.record_stage_started().unwrap_err(),
                    "WAKE_OFFLINE_LEDGER_RECORD_CHANGED"
                );
                assert_eq!(lease.phase(), "INTENT");
                assert_eq!(
                    lease.record_stage_started().unwrap_err(),
                    "WAKE_OFFLINE_STAGE_ATTENTION"
                );
                drop(lease);
            } else {
                assert_eq!(result.err().unwrap(), "WAKE_OFFLINE_LEDGER_RECORD_CHANGED");
            }
            assert!(root.join(record_name(1, InstallPhaseV1::Intent)).exists());
            assert_eq!(
                root.join(record_name(1, InstallPhaseV1::StageStarted))
                    .exists(),
                during_stage
            );
            assert_eq!(
                std::fs::read(&late_path).unwrap(),
                b"late unrelated generation"
            );
            std::fs::remove_file(&late_path).unwrap();
            drop(handle);
            super::super::tests::remove_fixture(&root, 1);
        }
    }

    #[test]
    fn fresh_time_conflict_and_torn_intent_never_overwrite_records() {
        let (root, handle, anchor, approval, mut observed, key) = fixture();
        let accepted_at = observed.now_utc_seconds;
        let pair = checked(&anchor, &approval, &observed, &key, accepted_at).unwrap();
        observed.now_utc_seconds += 1;
        assert_eq!(
            begin_fixture(
                BrokerLedgerGuard::acquire(&handle).unwrap(),
                &pair,
                &observed
            )
            .err()
            .unwrap(),
            "WAKE_OFFLINE_LEDGER_FRESH_TIME_MISMATCH"
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        observed.now_utc_seconds = accepted_at;
        let original = write_fixture(&root, &pair, accepted_at);
        let path = root.join(record_name(1, InstallPhaseV1::Intent));
        let mut changed = anchor.clone();
        changed.desired_state = OfflineDesiredStateV2::Stopped;
        let mut changed_observed = observed.clone();
        changed_observed.desired_state = changed.desired_state;
        let changed_pair =
            checked(&changed, &approval, &changed_observed, &key, accepted_at).unwrap();
        assert_eq!(
            begin_fixture(
                BrokerLedgerGuard::acquire(&handle).unwrap(),
                &changed_pair,
                &changed_observed
            )
            .err()
            .unwrap(),
            "WAKE_OFFLINE_LEDGER_APPROVAL_CONFLICT"
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::write(&path, b"torn INTENT").unwrap();
        assert_eq!(
            begin_fixture(
                BrokerLedgerGuard::acquire(&handle).unwrap(),
                &pair,
                &observed
            )
            .err()
            .unwrap(),
            "WAKE_OFFLINE_LEDGER_RECORD_INVALID"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"torn INTENT");
        drop(handle);
        super::super::tests::remove_fixture(&root, 1);
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
