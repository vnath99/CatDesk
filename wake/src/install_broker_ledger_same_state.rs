//! Versioned same-state transactions in the existing protected broker journal.
//! The fixed policy/global lease and phase filenames are shared with V1. No
//! request path, independent registry, or caller-supplied admission is accepted.
use super::*;
use crate::install_broker_offline_v2::FullFileIdentityV2;
use crate::install_broker_policy::{BrokerProtectedPolicyV1, PolicyVerifiedSameStateSelectionV3};

const PURPOSE: &str = "catdesk-wake-same-state-selection-transaction";
const MAX_V3_RECORD_BYTES: usize = 48 * 1024;

fn v3_file_info(file: &File, expected_length: Option<usize>) -> Result<()> {
    file_info_bounded(file, expected_length, MAX_V3_RECORD_BYTES)
}

pub(super) type SameStateHistoryV3 = (
    SameStateLedgerBindingV3,
    InstallPhaseV1,
    String,
    Vec<u8>,
    Vec<u8>,
);

pub(super) fn phase_name_v3(phase: InstallPhaseV1) -> &'static str {
    if phase == InstallPhaseV1::NewOwnerProven {
        "selection-verified-stopped"
    } else {
        phase.name()
    }
}

pub(super) fn record_name_v3(generation: u64, phase: InstallPhaseV1) -> String {
    format!(
        "install-transaction-{generation:020}-{}.json",
        phase_name_v3(phase)
    )
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SameStateLedgerBindingV3 {
    pub(crate) install_generation: u64,
    pub(crate) approval_id: String,
    pub(crate) signed_payload_sha256: String,
    pub(crate) signed_envelope_sha256: String,
    pub(crate) policy_sha256: String,
    pub(crate) verification_key_sha256: String,
    pub(crate) protected_root: ObjectIdentityV1,
    pub(crate) state_root: FullFileIdentityV2,
    pub(crate) accepted_at_utc_seconds: u64,
    pub(crate) state_snapshot_sha256: String,
    pub(crate) candidate_sha256: String,
    pub(crate) rollback_authority_sha256: String,
    pub(crate) before_selection_sha256: String,
    pub(crate) planned_selection_sha256: String,
}

impl SameStateLedgerBindingV3 {
    fn validate(&self, root: &ObjectIdentityV1) -> Result<()> {
        if self.install_generation == 0
            || self.accepted_at_utc_seconds == 0
            || !crate::protocol::valid_id(&self.approval_id)
            || self.protected_root != *root
            || self.state_root.volume_serial == 0
            || !lowercase_hex(&self.state_root.file_id_hex, 32)
            || self.state_root.file_id_hex.bytes().all(|v| v == b'0')
            || [
                &self.signed_payload_sha256,
                &self.signed_envelope_sha256,
                &self.policy_sha256,
                &self.verification_key_sha256,
                &self.state_snapshot_sha256,
                &self.candidate_sha256,
                &self.rollback_authority_sha256,
                &self.before_selection_sha256,
                &self.planned_selection_sha256,
            ]
            .iter()
            .any(|v| !lowercase_hex(v, 64))
            || self.before_selection_sha256 == self.planned_selection_sha256
        {
            return Err("WAKE_SAME_STATE_LEDGER_BINDING_INVALID".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SameStateRecordV3 {
    schema_version: u32,
    purpose: String,
    binding: SameStateLedgerBindingV3,
    phase: String,
    prior_record_sha256: String,
    /// The original exact selection is escrowed once, before any state write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    original_selection_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    signed_envelope_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    observed_selection_sha256: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SelectionAttemptV3 {
    schema_version: u32,
    purpose: String,
    binding: SameStateLedgerBindingV3,
    stopped_record_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublicationAttemptV3 {
    schema_version: u32,
    purpose: String,
    binding: SameStateLedgerBindingV3,
    staged_record_sha256: String,
}

fn publication_attempt(
    root: &OwnedHandle,
    binding: &SameStateLedgerBindingV3,
    staged: &str,
) -> Result<PublicationAttemptV3> {
    let mut file = open_optional(root, &publication_attempt_name(binding.install_generation))?
        .ok_or("WAKE_SAME_STATE_PUBLICATION_ATTEMPT_MISSING")?;
    v3_file_info(&file, None)?;
    let mut raw = Vec::new();
    (&mut file)
        .take(MAX_V3_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    v3_file_info(&file, Some(raw.len()))?;
    let actual: PublicationAttemptV3 =
        serde_json::from_slice(&raw).map_err(|_| "WAKE_SAME_STATE_PUBLICATION_ATTEMPT_INVALID")?;
    let expected = PublicationAttemptV3 {
        schema_version: 3,
        purpose: "catdesk-wake-same-state-before-publication".into(),
        binding: binding.clone(),
        staged_record_sha256: staged.into(),
    };
    if actual != expected
        || serde_json::to_vec(&actual).map_err(|_| "WAKE_LEDGER_ENCODING_INVALID")? != raw
    {
        return Err("WAKE_SAME_STATE_PUBLICATION_ATTEMPT_INVALID".into());
    }
    Ok(actual)
}

fn selection_attempt(
    root: &OwnedHandle,
    binding: &SameStateLedgerBindingV3,
    stopped: &str,
) -> Result<SelectionAttemptV3> {
    let mut file = open_optional(root, &pointer_attempt_name(binding.install_generation))?
        .ok_or("WAKE_SAME_STATE_SELECTION_ATTEMPT_MISSING")?;
    v3_file_info(&file, None)?;
    let mut raw = Vec::new();
    (&mut file)
        .take(MAX_V3_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    v3_file_info(&file, Some(raw.len()))?;
    let actual: SelectionAttemptV3 =
        serde_json::from_slice(&raw).map_err(|_| "WAKE_SAME_STATE_SELECTION_ATTEMPT_INVALID")?;
    let expected = SelectionAttemptV3 {
        schema_version: 3,
        purpose: "catdesk-wake-same-state-before-selection-write".into(),
        binding: binding.clone(),
        stopped_record_sha256: stopped.into(),
    };
    if actual != expected
        || serde_json::to_vec(&actual).map_err(|_| "WAKE_LEDGER_ENCODING_INVALID")? != raw
    {
        return Err("WAKE_SAME_STATE_SELECTION_ATTEMPT_INVALID".into());
    }
    Ok(actual)
}

fn bytes(record: &SameStateRecordV3) -> Result<Vec<u8>> {
    let bytes =
        serde_json::to_vec(record).map_err(|_| "WAKE_SAME_STATE_LEDGER_ENCODING_INVALID")?;
    if bytes.is_empty() || bytes.len() > MAX_V3_RECORD_BYTES {
        return Err("WAKE_SAME_STATE_LEDGER_ENCODING_INVALID".into());
    }
    Ok(bytes)
}

fn read_v3(file: &mut File) -> Result<(SameStateRecordV3, String)> {
    v3_file_info(file, None)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    let mut raw = Vec::new();
    file.take(MAX_V3_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    v3_file_info(file, Some(raw.len()))?;
    let value: SameStateRecordV3 =
        serde_json::from_slice(&raw).map_err(|_| "WAKE_SAME_STATE_LEDGER_RECORD_CORRUPT")?;
    if bytes(&value)? != raw {
        return Err("WAKE_SAME_STATE_LEDGER_RECORD_CORRUPT".into());
    }
    Ok((value, record_digest(&raw)))
}

fn write_v3(
    root: &OwnedHandle,
    root_id: &ObjectIdentityV1,
    record: &SameStateRecordV3,
) -> Result<String> {
    // Encoding/size refusal must precede even an empty protected file create.
    let encoded = bytes(record)?;
    let phase = InstallPhaseV1::ORDER
        .iter()
        .copied()
        .find(|p| phase_name_v3(*p) == record.phase)
        .ok_or("WAKE_SAME_STATE_LEDGER_PHASE_INVALID")?;
    let (mut file, created) = create_or_reopen(
        root,
        &record_name_v3(record.binding.install_generation, phase),
    )?;
    if created {
        v3_file_info(&file, Some(0))?;
        file.write_all(&encoded)
            .and_then(|_| file.sync_all())
            .map_err(|_| "WAKE_LEDGER_WRITE_FAILED")?;
    }
    let (actual, digest) = read_v3(&mut file)?;
    if actual != *record {
        return Err("WAKE_SAME_STATE_LEDGER_RECORD_CORRUPT".into());
    }
    flush_pinned_directory(root, root_id, ParentFlushPoint::LaterLedgerPhase)?;
    Ok(digest)
}

pub(super) fn is_v3_generation(root: &OwnedHandle, generation: u64) -> Result<bool> {
    let file = open_optional(root, &record_name_v3(generation, InstallPhaseV1::Intent))?
        .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
    v3_file_info(&file, None)?;
    let mut raw = Vec::new();
    file.take(MAX_V3_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "WAKE_LEDGER_RECORD_UNAVAILABLE")?;
    let value: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|_| "WAKE_LEDGER_RECORD_CORRUPT")?;
    Ok(value["schemaVersion"] == 3)
}

fn escrow_bytes(record: &SameStateRecordV3) -> Result<Vec<u8>> {
    let hex = record
        .original_selection_hex
        .as_deref()
        .ok_or("WAKE_SAME_STATE_ESCROW_MISSING")?;
    if hex.is_empty()
        || hex.len() > 8192
        || !hex.len().is_multiple_of(2)
        || !hex
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
    {
        return Err("WAKE_SAME_STATE_ESCROW_INVALID".into());
    }
    let decode = |v: u8| {
        if v.is_ascii_digit() {
            v - b'0'
        } else {
            v - b'a' + 10
        }
    };
    let raw: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|p| decode(p[0]) * 16 + decode(p[1]))
        .collect();
    if format!("{:x}", Sha256::digest(&raw)) != record.binding.before_selection_sha256 {
        return Err("WAKE_SAME_STATE_ESCROW_INVALID".into());
    }
    Ok(raw)
}

fn envelope_escrow_bytes(record: &SameStateRecordV3) -> Result<Vec<u8>> {
    let hex = record
        .signed_envelope_hex
        .as_deref()
        .ok_or("WAKE_SAME_STATE_ENVELOPE_ESCROW_MISSING")?;
    if hex.is_empty()
        || hex.len() > 32 * 1024
        || !hex.len().is_multiple_of(2)
        || !hex
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
    {
        return Err("WAKE_SAME_STATE_ENVELOPE_ESCROW_INVALID".into());
    }
    let decode = |v: u8| {
        if v.is_ascii_digit() {
            v - b'0'
        } else {
            v - b'a' + 10
        }
    };
    let raw: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|p| decode(p[0]) * 16 + decode(p[1]))
        .collect();
    if format!("{:x}", Sha256::digest(&raw)) != record.binding.signed_envelope_sha256 {
        return Err("WAKE_SAME_STATE_ENVELOPE_ESCROW_INVALID".into());
    }
    Ok(raw)
}

pub(super) fn history_v3(
    root: &OwnedHandle,
    generation: u64,
    present: &[bool; LEDGER_SLOT_COUNT],
    root_id: &ObjectIdentityV1,
) -> Result<SameStateHistoryV3> {
    if !present[0] {
        return Err("WAKE_SAME_STATE_LEDGER_RECORD_CORRUPT".into());
    }
    let mut prior = "0".repeat(64);
    let mut binding = None;
    let mut escrow = None;
    let mut envelope = None;
    let mut current = InstallPhaseV1::Intent;
    let mut stopped_digest = None;
    let mut staged_digest = None;
    let mut gap = false;
    for phase in InstallPhaseV1::ORDER {
        if !present[phase.index()] {
            gap = true;
            continue;
        }
        if gap {
            return Err("WAKE_SAME_STATE_LEDGER_PHASE_INVALID".into());
        }
        let mut file = open_optional(root, &record_name_v3(generation, phase))?
            .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
        let (record, digest) = read_v3(&mut file)?;
        record.binding.validate(root_id)?;
        if record.schema_version != 3
            || record.purpose != PURPOSE
            || record.phase != phase_name_v3(phase)
            || record.binding.install_generation != generation
            || record.prior_record_sha256 != prior
            || binding.as_ref().is_some_and(|b| b != &record.binding)
        {
            return Err("WAKE_SAME_STATE_LEDGER_RECORD_CORRUPT".into());
        }
        if phase == InstallPhaseV1::Intent {
            escrow = Some(escrow_bytes(&record)?);
            envelope = Some(envelope_escrow_bytes(&record)?);
        } else if record.original_selection_hex.is_some() || record.signed_envelope_hex.is_some() {
            return Err("WAKE_SAME_STATE_ESCROW_INVALID".into());
        }
        if phase.index() >= InstallPhaseV1::PointerSwitched.index() {
            if record.observed_selection_sha256.as_deref()
                != Some(record.binding.planned_selection_sha256.as_str())
            {
                return Err("WAKE_SAME_STATE_SELECTION_READBACK_INVALID".into());
            }
        } else if record.observed_selection_sha256.is_some() {
            return Err("WAKE_SAME_STATE_SELECTION_READBACK_INVALID".into());
        }
        if phase == InstallPhaseV1::OldOwnerStopped {
            stopped_digest = Some(digest.clone());
        }
        if phase == InstallPhaseV1::Staged {
            staged_digest = Some(digest.clone());
        }
        binding = Some(record.binding);
        prior = digest;
        current = phase;
    }
    let binding = binding.ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
    if present[PUBLICATION_ATTEMPT_SLOT] {
        publication_attempt(
            root,
            &binding,
            staged_digest
                .as_deref()
                .ok_or("WAKE_SAME_STATE_PUBLICATION_ATTEMPT_INVALID")?,
        )?;
    } else if current.index() >= InstallPhaseV1::Published.index() {
        return Err("WAKE_SAME_STATE_PUBLICATION_ATTEMPT_MISSING".into());
    }
    if present[POINTER_ATTEMPT_SLOT] {
        selection_attempt(
            root,
            &binding,
            stopped_digest
                .as_deref()
                .ok_or("WAKE_SAME_STATE_SELECTION_ATTEMPT_INVALID")?,
        )?;
    } else if current.index() >= InstallPhaseV1::PointerSwitched.index() {
        return Err("WAKE_SAME_STATE_SELECTION_ATTEMPT_MISSING".into());
    }
    Ok((
        binding,
        current,
        prior,
        escrow.ok_or("WAKE_SAME_STATE_ESCROW_MISSING")?,
        envelope.ok_or("WAKE_SAME_STATE_ENVELOPE_ESCROW_MISSING")?,
    ))
}

/// Issued only under the existing protected global lock. It is evidence for
/// verifying the exact envelope, not permission to alter selection.
pub(crate) struct SameStateLedgerAdmissionV3 {
    pub(crate) install_generation: u64,
    pub(crate) accepted_at_utc_seconds: u64,
    pub(crate) resume_binding: Option<SameStateLedgerBindingV3>,
    pub(crate) original_selection: Option<Vec<u8>>,
    pub(crate) original_envelope: Option<Vec<u8>>,
    pub(crate) selection_attempt_present: bool,
    envelope_sha256: String,
}

impl SameStateLedgerAdmissionV3 {
    pub(crate) fn envelope_sha256(&self) -> &str {
        &self.envelope_sha256
    }
}

pub(crate) struct AdmittedSameStateLedgerLeaseV3<'a, 'p> {
    root: &'a OwnedHandle,
    _global_lock: File,
    intent_file: File,
    policy: &'p BrokerProtectedPolicyV1,
    binding: SameStateLedgerBindingV3,
    phase: InstallPhaseV1,
    digest: String,
}

impl BrokerLedgerGuard<'_> {
    pub(crate) fn same_state_admission_v3(
        &self,
        envelope_sha256: &str,
        now: u64,
    ) -> Result<SameStateLedgerAdmissionV3> {
        if !lowercase_hex(envelope_sha256, 64) || now == 0 {
            return Err("WAKE_SAME_STATE_ADMISSION_INVALID".into());
        }
        let root_id = root_info(self.root)?;
        let inventory = ledger_inventory(self.root)?;
        let mut next = 1;
        let mut last_time = 0;
        for (&generation, present) in &inventory {
            if generation != next {
                return Err("WAKE_LEDGER_STALE_GENERATION".into());
            }
            if is_v3_generation(self.root, generation)? {
                let (binding, phase, _, original, envelope) =
                    history_v3(self.root, generation, present, &root_id)?;
                if binding.accepted_at_utc_seconds < last_time {
                    return Err("WAKE_LEDGER_CLOCK_ROLLBACK".into());
                }
                if binding.signed_envelope_sha256 == envelope_sha256 {
                    return Ok(SameStateLedgerAdmissionV3 {
                        install_generation: generation,
                        accepted_at_utc_seconds: binding.accepted_at_utc_seconds,
                        resume_binding: Some(binding),
                        original_selection: Some(original),
                        original_envelope: Some(envelope),
                        selection_attempt_present: present[POINTER_ATTEMPT_SLOT],
                        envelope_sha256: envelope_sha256.into(),
                    });
                }
                if phase != InstallPhaseV1::Complete {
                    return Err("WAKE_LEDGER_PRIOR_INCOMPLETE".into());
                }
                last_time = binding.accepted_at_utc_seconds;
            } else {
                let (intent, phase, _, _) =
                    historical_intent(self.root, generation, present, &root_id)?;
                if phase != InstallPhaseV1::Complete {
                    return Err("WAKE_LEDGER_PRIOR_INCOMPLETE".into());
                }
                if intent.accepted_at_utc_seconds < last_time {
                    return Err("WAKE_LEDGER_CLOCK_ROLLBACK".into());
                }
                last_time = intent.accepted_at_utc_seconds;
            }
            next = generation
                .checked_add(1)
                .ok_or("WAKE_LEDGER_GENERATION_INVALID")?;
        }
        if now < last_time {
            return Err("WAKE_LEDGER_CLOCK_ROLLBACK".into());
        }
        Ok(SameStateLedgerAdmissionV3 {
            install_generation: next,
            accepted_at_utc_seconds: now,
            resume_binding: None,
            original_selection: None,
            original_envelope: None,
            selection_attempt_present: false,
            envelope_sha256: envelope_sha256.into(),
        })
    }
}

impl<'a> BrokerLedgerGuard<'a> {
    pub(crate) fn begin_same_state_v3<'p>(
        self,
        grant: &PolicyVerifiedSameStateSelectionV3<'p>,
        admission: &SameStateLedgerAdmissionV3,
        original_selection: &[u8],
    ) -> Result<AdmittedSameStateLedgerLeaseV3<'a, 'p>> {
        grant.revalidate()?;
        let binding = grant.ledger_binding(admission.accepted_at_utc_seconds)?;
        binding.validate(&root_info(self.root)?)?;
        if binding.install_generation != admission.install_generation
            || binding.signed_envelope_sha256 != admission.envelope_sha256
            || format!("{:x}", Sha256::digest(original_selection))
                != binding.before_selection_sha256
            || original_selection.is_empty()
            || original_selection.len() > 4096
            || grant.envelope_bytes().is_empty()
            || grant.envelope_bytes().len() > 16 * 1024
            || format!("{:x}", Sha256::digest(grant.envelope_bytes()))
                != binding.signed_envelope_sha256
        {
            return Err("WAKE_SAME_STATE_ADMISSION_INVALID".into());
        }
        for (&generation, present) in &ledger_inventory(self.root)? {
            let (id, payload) = if is_v3_generation(self.root, generation)? {
                let (old, _, _, _, _) =
                    history_v3(self.root, generation, present, &binding.protected_root)?;
                (old.approval_id, old.signed_payload_sha256)
            } else {
                let (old, _, _, _) =
                    historical_intent(self.root, generation, present, &binding.protected_root)?;
                (old.approval_id, old.signed_approval_sha256)
            };
            if (id == binding.approval_id || payload == binding.signed_payload_sha256)
                && !(generation == binding.install_generation
                    && admission.resume_binding.as_ref() == Some(&binding))
            {
                return Err("WAKE_LEDGER_APPROVAL_REPLAY".into());
            }
        }
        let (phase, digest) = if let Some(previous) = &admission.resume_binding {
            if previous != &binding {
                return Err("WAKE_LEDGER_APPROVAL_CONFLICT".into());
            }
            let inventory = ledger_inventory(self.root)?;
            let (_, phase, digest, original, envelope) = history_v3(
                self.root,
                binding.install_generation,
                inventory
                    .get(&binding.install_generation)
                    .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?,
                &binding.protected_root,
            )?;
            if original != original_selection || envelope != grant.envelope_bytes() {
                return Err("WAKE_SAME_STATE_ESCROW_INVALID".into());
            }
            (phase, digest)
        } else {
            let record = SameStateRecordV3 {
                schema_version: 3,
                purpose: PURPOSE.into(),
                binding: binding.clone(),
                phase: InstallPhaseV1::Intent.name().into(),
                prior_record_sha256: "0".repeat(64),
                original_selection_hex: Some(
                    original_selection
                        .iter()
                        .map(|v| format!("{v:02x}"))
                        .collect(),
                ),
                signed_envelope_hex: Some(
                    grant
                        .envelope_bytes()
                        .iter()
                        .map(|v| format!("{v:02x}"))
                        .collect(),
                ),
                observed_selection_sha256: None,
            };
            (
                InstallPhaseV1::Intent,
                write_v3(self.root, &binding.protected_root, &record)?,
            )
        };
        let mut intent_file = open_optional(
            self.root,
            &record_name_v3(binding.install_generation, InstallPhaseV1::Intent),
        )?
        .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
        let (intent, _) = read_v3(&mut intent_file)?;
        if intent.binding != binding
            || intent.phase != InstallPhaseV1::Intent.name()
            || intent.schema_version != 3
            || intent.purpose != PURPOSE
            || intent.prior_record_sha256 != "0".repeat(64)
            || escrow_bytes(&intent)? != original_selection
            || envelope_escrow_bytes(&intent)? != grant.envelope_bytes()
        {
            return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
        }
        Ok(AdmittedSameStateLedgerLeaseV3 {
            root: self.root,
            _global_lock: self.global_lock,
            intent_file,
            policy: grant.policy(),
            binding,
            phase,
            digest,
        })
    }
}

impl AdmittedSameStateLedgerLeaseV3<'_, '_> {
    pub(crate) fn phase(&self) -> InstallPhaseV1 {
        self.phase
    }
    pub(crate) fn binding(&self) -> &SameStateLedgerBindingV3 {
        &self.binding
    }
    pub(crate) fn record_phase(
        &mut self,
        next: InstallPhaseV1,
        observed_selection_sha256: Option<&str>,
    ) -> Result<()> {
        self.policy.revalidate()?;
        if self.policy.readback()?.policy_sha256 != self.binding.policy_sha256
            || root_info(self.root)? != self.binding.protected_root
            || next.index() != self.phase.index() + 1
            || (next.index() >= InstallPhaseV1::PointerSwitched.index()
                && observed_selection_sha256
                    != Some(self.binding.planned_selection_sha256.as_str()))
            || (next.index() < InstallPhaseV1::PointerSwitched.index()
                && observed_selection_sha256.is_some())
        {
            return Err("WAKE_SAME_STATE_LEDGER_PHASE_INVALID".into());
        }
        let actual_digest = if self.phase == InstallPhaseV1::Intent {
            read_v3(&mut self.intent_file)?.1
        } else {
            let mut current = open_optional(
                self.root,
                &record_name_v3(self.binding.install_generation, self.phase),
            )?
            .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
            read_v3(&mut current)?.1
        };
        if actual_digest != self.digest {
            return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
        }
        if next == InstallPhaseV1::PointerSwitched {
            selection_attempt(self.root, &self.binding, &self.digest)?;
        }
        if next == InstallPhaseV1::Published {
            publication_attempt(self.root, &self.binding, &self.digest)?;
        }
        let record = SameStateRecordV3 {
            schema_version: 3,
            purpose: PURPOSE.into(),
            binding: self.binding.clone(),
            phase: phase_name_v3(next).into(),
            prior_record_sha256: self.digest.clone(),
            original_selection_hex: None,
            signed_envelope_hex: None,
            observed_selection_sha256: observed_selection_sha256.map(str::to_owned),
        };
        self.digest = write_v3(self.root, &self.binding.protected_root, &record)?;
        self.phase = next;
        Ok(())
    }
    pub(crate) fn record_pointer_attempt(&mut self) -> Result<()> {
        self.policy.revalidate()?;
        if self.phase != InstallPhaseV1::OldOwnerStopped
            || root_info(self.root)? != self.binding.protected_root
        {
            return Err("WAKE_SAME_STATE_SELECTION_ATTEMPT_INVALID".into());
        }
        let expected = SelectionAttemptV3 {
            schema_version: 3,
            purpose: "catdesk-wake-same-state-before-selection-write".into(),
            binding: self.binding.clone(),
            stopped_record_sha256: self.digest.clone(),
        };
        let (mut file, created) = create_or_reopen(
            self.root,
            &pointer_attempt_name(self.binding.install_generation),
        )?;
        if created {
            v3_file_info(&file, Some(0))?;
            let raw = serde_json::to_vec(&expected).map_err(|_| "WAKE_LEDGER_ENCODING_INVALID")?;
            file.write_all(&raw)
                .and_then(|_| file.sync_all())
                .map_err(|_| "WAKE_LEDGER_WRITE_FAILED")?;
        }
        drop(file);
        selection_attempt(self.root, &self.binding, &self.digest)?;
        flush_pinned_directory(
            self.root,
            &self.binding.protected_root,
            ParentFlushPoint::PointerAttempt,
        )
    }
    pub(crate) fn publication_attempt_present(&self) -> Result<bool> {
        let Some(mut staged) = open_optional(
            self.root,
            &record_name_v3(self.binding.install_generation, InstallPhaseV1::Staged),
        )?
        else {
            return Ok(false);
        };
        let staged_digest = read_v3(&mut staged)?.1;
        drop(staged);
        if open_optional(
            self.root,
            &publication_attempt_name(self.binding.install_generation),
        )?
        .is_none()
        {
            return Ok(false);
        }
        publication_attempt(self.root, &self.binding, &staged_digest)?;
        Ok(true)
    }
    pub(crate) fn record_publication_attempt(&mut self) -> Result<()> {
        self.policy.revalidate()?;
        if self.phase != InstallPhaseV1::Staged
            || root_info(self.root)? != self.binding.protected_root
        {
            return Err("WAKE_SAME_STATE_PUBLICATION_ATTEMPT_INVALID".into());
        }
        let expected = PublicationAttemptV3 {
            schema_version: 3,
            purpose: "catdesk-wake-same-state-before-publication".into(),
            binding: self.binding.clone(),
            staged_record_sha256: self.digest.clone(),
        };
        let (mut file, created) = create_or_reopen(
            self.root,
            &publication_attempt_name(self.binding.install_generation),
        )?;
        if created {
            v3_file_info(&file, Some(0))?;
            let raw = serde_json::to_vec(&expected).map_err(|_| "WAKE_LEDGER_ENCODING_INVALID")?;
            file.write_all(&raw)
                .and_then(|_| file.sync_all())
                .map_err(|_| "WAKE_LEDGER_WRITE_FAILED")?;
        }
        drop(file);
        publication_attempt(self.root, &self.binding, &self.digest)?;
        flush_pinned_directory(
            self.root,
            &self.binding.protected_root,
            ParentFlushPoint::PublicationAttempt,
        )
    }
    pub(crate) fn complete_record_sha256(&self) -> Result<&str> {
        if self.phase != InstallPhaseV1::Complete {
            return Err("WAKE_SAME_STATE_LEDGER_NOT_COMPLETE".into());
        }
        self.policy.revalidate()?;
        let mut file = open_optional(
            self.root,
            &record_name_v3(self.binding.install_generation, InstallPhaseV1::Complete),
        )?
        .ok_or("WAKE_LEDGER_RECORD_CORRUPT")?;
        if read_v3(&mut file)?.1 != self.digest
            || root_info(self.root)? != self.binding.protected_root
        {
            return Err("WAKE_LEDGER_RECORD_CORRUPT".into());
        }
        Ok(&self.digest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(raw: &[u8]) -> SameStateLedgerBindingV3 {
        SameStateLedgerBindingV3 {
            install_generation: 1,
            approval_id: "signed-selection-one".into(),
            signed_payload_sha256: "1".repeat(64),
            signed_envelope_sha256: "2".repeat(64),
            policy_sha256: "3".repeat(64),
            verification_key_sha256: "4".repeat(64),
            protected_root: ObjectIdentityV1 {
                volume_serial: 1,
                file_index_high: 0,
                file_index_low: 1,
            },
            state_root: FullFileIdentityV2 {
                volume_serial: 2,
                file_id_hex: "5".repeat(32),
            },
            accepted_at_utc_seconds: 100,
            state_snapshot_sha256: "6".repeat(64),
            candidate_sha256: "7".repeat(64),
            rollback_authority_sha256: "8".repeat(64),
            before_selection_sha256: format!("{:x}", Sha256::digest(raw)),
            planned_selection_sha256: "9".repeat(64),
        }
    }

    fn fixture_record(
        binding: &SameStateLedgerBindingV3,
        phase: InstallPhaseV1,
        prior: &str,
        selection: &[u8],
        envelope: &[u8],
    ) -> SameStateRecordV3 {
        SameStateRecordV3 {
            schema_version: 3,
            purpose: PURPOSE.into(),
            binding: binding.clone(),
            phase: phase_name_v3(phase).into(),
            prior_record_sha256: prior.into(),
            original_selection_hex: (phase == InstallPhaseV1::Intent)
                .then(|| selection.iter().map(|v| format!("{v:02x}")).collect()),
            signed_envelope_hex: (phase == InstallPhaseV1::Intent)
                .then(|| envelope.iter().map(|v| format!("{v:02x}")).collect()),
            observed_selection_sha256: (phase.index() >= InstallPhaseV1::PointerSwitched.index())
                .then(|| binding.planned_selection_sha256.clone()),
        }
    }

    #[test]
    fn same_state_native_oversized_intent_leaves_no_file_or_replay_poison() {
        let (path, root, _, _) = super::super::tests::fixture();
        let guard = BrokerLedgerGuard::acquire(&root).unwrap();
        let mut b = binding(b"original");
        b.protected_root = root_info(&root).unwrap();
        let mut record = fixture_record(
            &b,
            InstallPhaseV1::Intent,
            &"0".repeat(64),
            b"original",
            b"fixture",
        );
        record.signed_envelope_hex = Some("a".repeat(MAX_V3_RECORD_BYTES));
        assert_eq!(
            write_v3(&root, &b.protected_root, &record).unwrap_err(),
            "WAKE_SAME_STATE_LEDGER_ENCODING_INVALID"
        );
        assert!(
            !path
                .join(record_name_v3(1, InstallPhaseV1::Intent))
                .exists()
        );
        assert_eq!(
            guard
                .same_state_admission_v3(&"a".repeat(64), 1000)
                .unwrap()
                .install_generation,
            1
        );
        drop(guard);
        drop(root);
        super::super::tests::remove_fixture(&path, 1);
    }

    #[test]
    fn same_state_native_full_size_escrow_reopens_without_widening_v1_limit() {
        let (path, root, _, _) = super::super::tests::fixture();
        let guard = BrokerLedgerGuard::acquire(&root).unwrap();
        let original = vec![b' '; 4096];
        let envelope = vec![b'a'; 16 * 1024];
        let mut binding = binding(&original);
        binding.protected_root = root_info(&root).unwrap();
        binding.signed_envelope_sha256 = format!("{:x}", Sha256::digest(&envelope));
        let record = fixture_record(
            &binding,
            InstallPhaseV1::Intent,
            &"0".repeat(64),
            &original,
            &envelope,
        );
        assert!(bytes(&record).unwrap().len() > MAX_RECORD_BYTES);
        write_v3(&root, &binding.protected_root, &record).unwrap();
        let mut file = open_optional(&root, &record_name_v3(1, InstallPhaseV1::Intent))
            .unwrap()
            .unwrap();
        assert_eq!(
            file_info(&file, None).unwrap_err(),
            "WAKE_LEDGER_RECORD_INVALID"
        );
        let (actual, _) = read_v3(&mut file).unwrap();
        assert_eq!(actual, record);
        drop(file);
        assert!(is_v3_generation(&root, 1).unwrap());
        let admission = guard
            .same_state_admission_v3(&binding.signed_envelope_sha256, 9000)
            .unwrap();
        assert_eq!(
            admission.accepted_at_utc_seconds,
            binding.accepted_at_utc_seconds
        );
        assert_eq!(admission.original_selection, Some(original));
        assert_eq!(admission.original_envelope, Some(envelope));
        drop(guard);
        drop(root);
        super::super::tests::remove_fixture(&path, 1);
    }

    #[test]
    fn same_state_native_post_pointer_resume_preserves_both_escrows_and_consumption() {
        let (path, root, _, _) = super::super::tests::fixture();
        let guard = BrokerLedgerGuard::acquire(&root).unwrap();
        let original = b"original selection\r\n";
        let envelope = b"synthetic metadata fixture, no installation authority";
        let mut b = binding(original);
        b.protected_root = root_info(&root).unwrap();
        b.signed_envelope_sha256 = format!("{:x}", Sha256::digest(envelope));
        let mut prior = "0".repeat(64);
        let mut staged = String::new();
        let mut stopped = String::new();
        for phase in InstallPhaseV1::ORDER {
            if phase == InstallPhaseV1::Published {
                let attempt = PublicationAttemptV3 {
                    schema_version: 3,
                    purpose: "catdesk-wake-same-state-before-publication".into(),
                    binding: b.clone(),
                    staged_record_sha256: staged.clone(),
                };
                std::fs::write(
                    path.join(publication_attempt_name(1)),
                    serde_json::to_vec(&attempt).unwrap(),
                )
                .unwrap();
            }
            if phase == InstallPhaseV1::PointerSwitched {
                let attempt = SelectionAttemptV3 {
                    schema_version: 3,
                    purpose: "catdesk-wake-same-state-before-selection-write".into(),
                    binding: b.clone(),
                    stopped_record_sha256: stopped.clone(),
                };
                std::fs::write(
                    path.join(pointer_attempt_name(1)),
                    serde_json::to_vec(&attempt).unwrap(),
                )
                .unwrap();
            }
            prior = write_v3(
                &root,
                &b.protected_root,
                &fixture_record(&b, phase, &prior, original, envelope),
            )
            .unwrap();
            if phase == InstallPhaseV1::Staged {
                staged = prior.clone();
            }
            if phase == InstallPhaseV1::OldOwnerStopped {
                stopped = prior.clone();
            }
            if phase == InstallPhaseV1::PointerSwitched {
                let admission = guard
                    .same_state_admission_v3(&b.signed_envelope_sha256, 9000)
                    .unwrap();
                assert_eq!(admission.accepted_at_utc_seconds, b.accepted_at_utc_seconds);
                assert_eq!(admission.resume_binding.as_ref(), Some(&b));
                assert_eq!(
                    admission.original_selection.as_deref(),
                    Some(original.as_slice())
                );
                assert_eq!(
                    admission.original_envelope.as_deref(),
                    Some(envelope.as_slice())
                );
                assert!(admission.selection_attempt_present);
                assert_eq!(
                    guard
                        .same_state_admission_v3(&"f".repeat(64), 9000)
                        .err()
                        .unwrap(),
                    "WAKE_LEDGER_PRIOR_INCOMPLETE"
                );
            }
        }
        let inventory = ledger_inventory(&root).unwrap();
        let (_, phase, actual, raw, signed) =
            history_v3(&root, 1, &inventory[&1], &b.protected_root).unwrap();
        assert_eq!(phase, InstallPhaseV1::Complete);
        assert_eq!(actual, prior);
        assert_eq!(raw, original);
        assert_eq!(signed, envelope);
        assert!(
            path.join(record_name_v3(1, InstallPhaseV1::NewOwnerProven))
                .exists()
        );
        assert!(
            !path
                .join(record_name(1, InstallPhaseV1::NewOwnerProven))
                .exists()
        );
        assert_eq!(
            guard
                .same_state_admission_v3(&"f".repeat(64), 9000)
                .unwrap()
                .install_generation,
            2
        );
        // A conflicting second accepted ID is detected by the shared native
        // history reader; this fixture writes metadata only, never a grant.
        let mut duplicate = b.clone();
        duplicate.install_generation = 2;
        duplicate.accepted_at_utc_seconds = 200;
        duplicate.signed_envelope_sha256 = format!("{:x}", Sha256::digest(b"other fixture"));
        duplicate.signed_payload_sha256 = "e".repeat(64);
        write_v3(
            &root,
            &b.protected_root,
            &fixture_record(
                &duplicate,
                InstallPhaseV1::Intent,
                &"0".repeat(64),
                original,
                b"other fixture",
            ),
        )
        .unwrap();
        assert_eq!(
            validated_latest_accepted_at(&root, &b.protected_root).unwrap_err(),
            "WAKE_LEDGER_APPROVAL_REPLAY"
        );
        drop(guard);
        drop(root);
        std::fs::remove_file(path.join(record_name_v3(2, InstallPhaseV1::Intent))).unwrap();
        std::fs::remove_file(path.join(record_name_v3(1, InstallPhaseV1::NewOwnerProven))).unwrap();
        std::fs::remove_file(path.join(pointer_attempt_name(1))).unwrap();
        super::super::tests::remove_fixture(&path, 1);
    }

    #[test]
    fn same_state_native_partial_publication_marker_is_sticky_and_never_new_admission() {
        let (path, root, _, _) = super::super::tests::fixture();
        let guard = BrokerLedgerGuard::acquire(&root).unwrap();
        let raw = b"old selection";
        let envelope = b"fixture approval";
        let mut b = binding(raw);
        b.protected_root = root_info(&root).unwrap();
        b.signed_envelope_sha256 = format!("{:x}", Sha256::digest(envelope));
        let mut prior = "0".repeat(64);
        for phase in [
            InstallPhaseV1::Intent,
            InstallPhaseV1::StageStarted,
            InstallPhaseV1::Staged,
        ] {
            prior = write_v3(
                &root,
                &b.protected_root,
                &fixture_record(&b, phase, &prior, raw, envelope),
            )
            .unwrap();
        }
        let intent_before =
            std::fs::read(path.join(record_name_v3(1, InstallPhaseV1::Intent))).unwrap();
        std::fs::write(path.join(publication_attempt_name(1)), b"").unwrap();
        assert!(
            guard
                .same_state_admission_v3(&b.signed_envelope_sha256, 9000)
                .is_err()
        );
        assert!(
            guard
                .same_state_admission_v3(&"f".repeat(64), 9000)
                .is_err()
        );
        assert_eq!(
            std::fs::metadata(path.join(publication_attempt_name(1)))
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            std::fs::read(path.join(record_name_v3(1, InstallPhaseV1::Intent))).unwrap(),
            intent_before
        );
        assert!(
            !path
                .join(record_name_v3(2, InstallPhaseV1::Intent))
                .exists()
        );
        drop(guard);
        drop(root);
        super::super::tests::remove_fixture(&path, 1);
    }

    #[test]
    fn same_state_native_generation_and_replay_share_existing_v1_history() {
        use crate::install_broker_approval::tests as approval_test;
        let (path, root, _, _) = super::super::tests::fixture();
        let root_id = root_info(&root).unwrap();
        let (mut payload, mut observed, key) = approval_test::fixture();
        payload.install_generation = 1;
        observed.next_install_generation = 1;
        payload.target_protected_root = root_id.clone();
        observed.target_protected_root = root_id.clone();
        let approved = verify_approval(
            &approval_test::envelope(&payload, &key),
            &key.verifying_key(),
            &observed,
        )
        .unwrap();
        let expected = LedgerExpectationV1 {
            now_utc_seconds: observed.now_utc_seconds,
            next_install_generation: 1,
            predecessor_package_id: payload.predecessor_package_id.clone(),
            predecessor_pointer: payload.predecessor_pointer.clone(),
            protected_root: root_id.clone(),
            original_desired_state: OriginalDesiredStateV1::Stopped,
        };
        let mut old = BrokerLedgerGuard::acquire(&root)
            .unwrap()
            .begin_fixture(&approved, &expected)
            .unwrap();
        for phase in InstallPhaseV1::ORDER.into_iter().skip(1) {
            old.advance(phase).unwrap();
        }
        drop(old);
        let guard = BrokerLedgerGuard::acquire(&root).unwrap();
        let now = observed.now_utc_seconds + 1;
        assert_eq!(
            guard
                .same_state_admission_v3(&"f".repeat(64), now)
                .unwrap()
                .install_generation,
            2
        );
        let original = b"existing selection";
        let envelope = b"duplicate fixture identity";
        let mut duplicate = binding(original);
        duplicate.install_generation = 2;
        duplicate.protected_root = root_id.clone();
        duplicate.approval_id = payload.approval_id;
        duplicate.accepted_at_utc_seconds = now;
        duplicate.signed_envelope_sha256 = format!("{:x}", Sha256::digest(envelope));
        write_v3(
            &root,
            &root_id,
            &fixture_record(
                &duplicate,
                InstallPhaseV1::Intent,
                &"0".repeat(64),
                original,
                envelope,
            ),
        )
        .unwrap();
        assert_eq!(
            validated_latest_accepted_at(&root, &root_id).unwrap_err(),
            "WAKE_LEDGER_APPROVAL_REPLAY"
        );
        drop(guard);
        drop(root);
        std::fs::remove_file(path.join(record_name_v3(2, InstallPhaseV1::Intent))).unwrap();
        super::super::tests::remove_fixture(&path, 1);
    }

    #[test]
    fn same_state_ledger_binding_rejects_wrong_root_and_ambiguous_selection() {
        let mut b = binding(b"original selection");
        b.validate(&b.protected_root).unwrap();
        let mut other = b.protected_root.clone();
        other.file_index_low += 1;
        assert!(b.validate(&other).is_err());
        b.planned_selection_sha256 = b.before_selection_sha256.clone();
        assert!(b.validate(&b.protected_root).is_err());
        b.planned_selection_sha256 = "PRIVATE".into();
        assert!(b.validate(&b.protected_root).is_err());
    }

    #[test]
    fn same_state_ledger_original_pointer_escrow_is_exact_and_hash_bound() {
        let raw = b"{\"schemaVersion\":1}\r\n";
        let mut record = SameStateRecordV3 {
            schema_version: 3,
            purpose: PURPOSE.into(),
            binding: binding(raw),
            phase: "intent".into(),
            prior_record_sha256: "0".repeat(64),
            original_selection_hex: Some(raw.iter().map(|v| format!("{v:02x}")).collect()),
            signed_envelope_hex: Some("00".into()),
            observed_selection_sha256: None,
        };
        assert_eq!(escrow_bytes(&record).unwrap(), raw);
        record.original_selection_hex = Some("00".into());
        assert!(escrow_bytes(&record).is_err());
        record.original_selection_hex = Some("gg".into());
        assert!(escrow_bytes(&record).is_err());
        record.original_selection_hex = None;
        assert!(escrow_bytes(&record).is_err());
    }

    #[test]
    fn same_state_ledger_bounded_envelope_and_selection_fit_before_any_file_create() {
        let selection = vec![b' '; 4096];
        let envelope = vec![b'a'; 16 * 1024];
        let mut binding = binding(&selection);
        binding.signed_envelope_sha256 = format!("{:x}", Sha256::digest(&envelope));
        let mut record = SameStateRecordV3 {
            schema_version: 3,
            purpose: PURPOSE.into(),
            binding,
            phase: "intent".into(),
            prior_record_sha256: "0".repeat(64),
            original_selection_hex: Some(selection.iter().map(|v| format!("{v:02x}")).collect()),
            signed_envelope_hex: Some(envelope.iter().map(|v| format!("{v:02x}")).collect()),
            observed_selection_sha256: None,
        };
        assert!(bytes(&record).unwrap().len() <= MAX_V3_RECORD_BYTES);
        assert_eq!(escrow_bytes(&record).unwrap(), selection);
        assert_eq!(envelope_escrow_bytes(&record).unwrap(), envelope);
        record.signed_envelope_hex = Some("a".repeat(MAX_V3_RECORD_BYTES));
        assert!(bytes(&record).is_err());
        // write_v3 calls this encoder before its first create_or_reopen.
    }
}
