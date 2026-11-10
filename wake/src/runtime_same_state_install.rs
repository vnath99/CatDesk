//! Retained existing-root resources for the purpose-specific signed V3 grant.
//! No public path, key, verifier closure or Boolean can enter this transaction.
#![cfg(windows)]
use super::*;
use crate::{
    install_broker_approval::same_state::{
        SameStateObjectV3, SameStateObservedV3, SameStateRetainedResumeV3,
        SameStateRootSelectionApprovalV3, SameStateSnapshotV3,
        validate_same_state_runtime_manifest_v3,
    },
    install_broker_ledger::{
        AdmittedSameStateLedgerLeaseV3, InstallPhaseV1, SameStateLedgerAdmissionV3,
    },
    install_broker_offline_pins::OfflineObjectPinV2,
    install_broker_policy::{BrokerProtectedPolicyV1, PolicyVerifiedSameStateSelectionV3},
    install_broker_source::{RuntimePackagePinsV3, RuntimeSourceRoleV3},
};
use serde_json::{Value, json};

/// Bounded final-disposition receipts bind signed claims to retained bytes.
/// Evidence hashes refer to underlying immutable records, never this wrapper.
/// The trusted reviewer still establishes the provenance and independence of
/// those records; these closed facts cannot turn a partial review or backend
/// health check into full source or Wake-runtime qualification.
#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum SameStateReceiptV3 {
    SourceQualification {
        schema_version: u32,
        source_commit: String,
        manifest_sha256: String,
        verdict: String,
        scope: String,
        evidence_sha256: Vec<String>,
    },
    IndependentSourceReview {
        schema_version: u32,
        source_commit: String,
        manifest_sha256: String,
        verdict: String,
        scope: String,
        evidence_sha256: Vec<String>,
        reviewer_identity: String,
        full_source_reviewed: bool,
        findings_count: u64,
        needed_reads_count: u64,
    },
    WakeWorking {
        schema_version: u32,
        source_commit: String,
        manifest_sha256: String,
        verdict: String,
        scope: String,
        evidence_sha256: Vec<String>,
        observed_utc_seconds: u64,
        owner_verified: bool,
        startup_verified: bool,
        normal_stop_verified: bool,
    },
    ReviewedSourceReconstruction {
        schema_version: u32,
        source_commit: String,
        manifest_sha256: String,
        verdict: String,
        scope: String,
        evidence_sha256: Vec<String>,
        old_raw_selection_escrow_sha256: String,
    },
}

fn validate_receipt(
    role: &str,
    raw: &[u8],
    payload: &SameStateRootSelectionApprovalV3,
) -> Result<()> {
    use crate::install_broker_approval::same_state::{
        SameStateRollbackAuthorityV3, same_state_manifest_sha256,
    };
    let receipt: SameStateReceiptV3 =
        serde_json::from_slice(raw).map_err(|_| "WAKE_SAME_STATE_RECEIPT_SEMANTICS_INVALID")?;
    let release = if role.starts_with("candidate-") {
        &payload.candidate
    } else {
        payload.rollback_authority.release()
    };
    let manifest = same_state_manifest_sha256(&release.manifest)?;
    let common =
        |schema: u32, source: &str, recorded_manifest: &str, verdict: &str, evidence: &[String]| {
            schema == 3
                && source == release.source_commit
                && recorded_manifest == manifest
                && verdict == "PASS"
                && !evidence.is_empty()
                && evidence.len() <= 32
                && evidence.iter().all(|sha| {
                    sha.len() == 64
                        && sha
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
                && evidence
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == evidence.len()
        };
    let valid = match (role, receipt) {
        (
            "candidate-local.json" | "previous-local.json",
            SameStateReceiptV3::SourceQualification {
                schema_version,
                source_commit,
                manifest_sha256,
                verdict,
                scope,
                evidence_sha256,
            },
        ) => {
            common(
                schema_version,
                &source_commit,
                &manifest_sha256,
                &verdict,
                &evidence_sha256,
            ) && scope == "EXACT_SOURCE"
        }
        (
            "candidate-review.json" | "previous-review.json",
            SameStateReceiptV3::IndependentSourceReview {
                schema_version,
                source_commit,
                manifest_sha256,
                verdict,
                scope,
                evidence_sha256,
                reviewer_identity,
                full_source_reviewed,
                findings_count,
                needed_reads_count,
            },
        ) => {
            common(
                schema_version,
                &source_commit,
                &manifest_sha256,
                &verdict,
                &evidence_sha256,
            ) && scope == "FULL_SOURCE"
                && reviewer_identity == release.independent_review.reviewer_identity
                && full_source_reviewed
                && findings_count == 0
                && needed_reads_count == 0
        }
        (
            "previous-working.json",
            SameStateReceiptV3::WakeWorking {
                schema_version,
                source_commit,
                manifest_sha256,
                verdict,
                scope,
                evidence_sha256,
                observed_utc_seconds,
                owner_verified,
                startup_verified,
                normal_stop_verified,
            },
        ) => {
            common(
                schema_version,
                &source_commit,
                &manifest_sha256,
                &verdict,
                &evidence_sha256,
            ) && release.working_evidence.as_ref().is_some_and(|working| {
                scope == "WAKE_RUNTIME"
                    && scope == working.scope
                    && observed_utc_seconds == working.observed_utc_seconds
                    && owner_verified
                    && owner_verified == working.owner_verified
                    && startup_verified
                    && startup_verified == working.startup_verified
                    && normal_stop_verified
                    && normal_stop_verified == working.normal_stop_verified
            })
        }
        (
            "reconstruction.json",
            SameStateReceiptV3::ReviewedSourceReconstruction {
                schema_version,
                source_commit,
                manifest_sha256,
                verdict,
                scope,
                evidence_sha256,
                old_raw_selection_escrow_sha256,
            },
        ) => {
            common(
                schema_version,
                &source_commit,
                &manifest_sha256,
                &verdict,
                &evidence_sha256,
            ) && scope == "LEGACY_SELECTION_RECONSTRUCTION"
                && matches!(&payload.rollback_authority, SameStateRollbackAuthorityV3::ReviewedSourceReconstruction {
                old_raw_selection_escrow_sha256: expected, ..
            } if *expected == old_raw_selection_escrow_sha256
                && *expected == payload.state_snapshot.current.sha256)
        }
        _ => false,
    };
    if !valid {
        return Err("WAKE_SAME_STATE_RECEIPT_SEMANTICS_INVALID".into());
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn object(pin: &mut OfflineObjectPinV2) -> Result<(SameStateObjectV3, Vec<u8>)> {
    let raw = pin.read_bounded(64 * 1024)?;
    Ok((
        SameStateObjectV3 {
            identity: pin.identity().full.clone(),
            sha256: digest(&raw),
        },
        raw,
    ))
}

/// Actual read handles only. Its factory derives all paths from fixed policy
/// and fixed submission roles. It grants no mutation authority.
pub(crate) struct SameStateReadPinsV3 {
    root: PathBuf,
    _ancestors: Vec<OfflineObjectPinV2>,
    current: Option<OfflineObjectPinV2>,
    control: OfflineObjectPinV2,
    config: OfflineObjectPinV2,
    snapshot: SameStateSnapshotV3,
    current_raw: Vec<u8>,
    original_raw: Vec<u8>,
    candidate: RuntimePackagePinsV3,
    previous: RuntimePackagePinsV3,
    planned: Vec<u8>,
    receipts: Vec<(PathBuf, OfflineObjectPinV2, String)>,
    resume: Option<SameStateRetainedResumeV3>,
    _receipt_directory: Option<OfflineObjectPinV2>,
    receipt_root: PathBuf,
}

enum SameStateIntakeV3 {
    Fixed,
    #[cfg(test)]
    Isolated(PathBuf),
}

impl SameStateReadPinsV3 {
    pub(crate) fn observe(
        policy: &BrokerProtectedPolicyV1,
        admission: &SameStateLedgerAdmissionV3,
    ) -> Result<Self> {
        Self::observe_with_intake(policy, admission, SameStateIntakeV3::Fixed)
    }

    fn observe_with_intake(
        policy: &BrokerProtectedPolicyV1,
        admission: &SameStateLedgerAdmissionV3,
        intake: SameStateIntakeV3,
    ) -> Result<Self> {
        let configured = policy.same_state_resource_v2()?;
        let root = PathBuf::from(&configured.path);
        // Existing-only Store validation precedes any writable/open/create API.
        let read_store = Store::open_read_only(&root)?;
        if read_store.root()
            != std::fs::canonicalize(&root).map_err(|_| "WAKE_SAME_STATE_ROOT_INVALID")?
        {
            return Err("WAKE_SAME_STATE_ROOT_INVALID".into());
        }
        let mut ancestors = Vec::new();
        for path in root.ancestors().collect::<Vec<_>>().into_iter().rev() {
            ancestors.push(OfflineObjectPinV2::same_state_directory(path)?);
        }
        let root_identity = ancestors
            .last()
            .ok_or("WAKE_SAME_STATE_ROOT_INVALID")?
            .identity()
            .full
            .clone();
        if root_identity != configured.identity {
            return Err("WAKE_SAME_STATE_ROOT_MISMATCH".into());
        }
        let mut current = OfflineObjectPinV2::open(&root.join("current.json"), false)?;
        let mut control = OfflineObjectPinV2::open(&root.join("control.json"), false)?;
        let mut config = OfflineObjectPinV2::open(&root.join("config.json"), false)?;
        let (current_object, current_raw) = object(&mut current)?;
        let (control_object, control_raw) = object(&mut control)?;
        let (config_object, _) = object(&mut config)?;
        let desired: Control =
            serde_json::from_slice(&control_raw).map_err(|_| "WAKE_SAME_STATE_CONTROL_INVALID")?;
        if desired.schema_version != 1 || desired.desired != "STOPPED" {
            return Err("WAKE_SAME_STATE_STOPPED_REQUIRED".into());
        }
        let original_raw = admission
            .original_selection
            .clone()
            .unwrap_or_else(|| current_raw.clone());
        if let Some(binding) = &admission.resume_binding
            && digest(&current_raw) != binding.before_selection_sha256
            && digest(&current_raw) != binding.planned_selection_sha256
        {
            return Err("WAKE_SAME_STATE_SELECTION_UNKNOWN".into());
        }
        let candidate = match &intake {
            SameStateIntakeV3::Fixed => {
                RuntimePackagePinsV3::fixed(RuntimeSourceRoleV3::Candidate)?
            }
            #[cfg(test)]
            SameStateIntakeV3::Isolated(path) => {
                RuntimePackagePinsV3::isolated(path, RuntimeSourceRoleV3::Candidate)?
            }
        };
        let original: Value = serde_json::from_slice(&original_raw)
            .map_err(|_| "WAKE_SAME_STATE_SELECTION_INVALID")?;
        let previous = match &intake {
            #[cfg(test)]
            SameStateIntakeV3::Isolated(path) => {
                RuntimePackagePinsV3::isolated(path, RuntimeSourceRoleV3::Previous)?
            }
            SameStateIntakeV3::Fixed
                if Path::new(r"C:\ProgramData\CatDesk\WakeSubmissionV1\previous").exists() =>
            {
                RuntimePackagePinsV3::fixed(RuntimeSourceRoleV3::Previous)?
            }
            SameStateIntakeV3::Fixed => {
                let pointer = original
                    .get("selection")
                    .and_then(|s| s.get("previous"))
                    .and_then(|p| p.get("pointer"))
                    .unwrap_or(&original);
                RuntimePackagePinsV3::installed(&root, install_id(pointer)?)?
            }
        };
        let receipt_root = match intake {
            SameStateIntakeV3::Fixed => {
                PathBuf::from(r"C:\ProgramData\CatDesk\WakeSubmissionV1\receipts")
            }
            #[cfg(test)]
            SameStateIntakeV3::Isolated(path) => path.join("receipts"),
        };
        let resume = admission
            .resume_binding
            .as_ref()
            .map(|b| SameStateRetainedResumeV3 {
                original_snapshot_sha256: b.state_snapshot_sha256.clone(),
                before_selection_sha256: b.before_selection_sha256.clone(),
                planned_selection_sha256: b.planned_selection_sha256.clone(),
                original_escrow_sha256: digest(&original_raw),
                selection_attempt_present: admission.selection_attempt_present,
            });
        Ok(Self {
            root,
            _ancestors: ancestors,
            current: Some(current),
            control,
            config,
            snapshot: SameStateSnapshotV3 {
                root: root_identity,
                current: current_object,
                control: control_object,
                config: config_object,
                desired: desired.desired,
            },
            current_raw,
            original_raw,
            candidate,
            previous,
            planned: Vec::new(),
            receipts: Vec::new(),
            resume,
            _receipt_directory: None,
            receipt_root,
        })
    }
    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> &SameStateSnapshotV3 {
        &self.snapshot
    }
    pub(crate) fn original_selection(&self) -> &[u8] {
        &self.original_raw
    }
    #[cfg(test)]
    pub(crate) fn planned_selection(&self) -> &[u8] {
        &self.planned
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn observed(
        &mut self,
        policy: &BrokerProtectedPolicyV1,
        payload: &SameStateRootSelectionApprovalV3,
        admission: &SameStateLedgerAdmissionV3,
    ) -> Result<SameStateObservedV3> {
        self.revalidate()?;
        validate_same_state_runtime_manifest_v3(
            self.candidate.runtime_manifest_bytes(),
            &payload.candidate,
        )?;
        validate_same_state_runtime_manifest_v3(
            self.previous.runtime_manifest_bytes(),
            payload.rollback_authority.release(),
        )?;
        if self.candidate.manifest() != &payload.candidate.manifest
            || self.previous.manifest() != &payload.rollback_authority.release().manifest
        {
            return Err("WAKE_SAME_STATE_PACKAGE_MISMATCH".into());
        }
        self.pin_receipts(payload)?;
        let candidate_qualification = digest(
            &serde_json::to_vec(&payload.candidate)
                .map_err(|_| "WAKE_SAME_STATE_APPROVAL_INVALID")?,
        );
        let previous_release = payload.rollback_authority.release();
        let previous_qualification = digest(
            &serde_json::to_vec(previous_release)
                .map_err(|_| "WAKE_SAME_STATE_APPROVAL_INVALID")?,
        );
        let candidate = self.candidate.pointer();
        let previous = self.previous.pointer();
        let verify = |p: &Value, q: &str, working: bool| {
            if (p == &candidate && q == candidate_qualification && !working)
                || (p == &previous
                    && q == previous_qualification
                    && working
                    && previous_release.working_evidence.is_some())
            {
                Ok(())
            } else {
                Err("WAKE_SAME_STATE_RELEASE_QUALIFICATION_MISMATCH".into())
            }
        };
        self.planned = if payload.rollback_authority.is_reconstruction() {
            install_selection::propose_reconstructed_install_selection(
                &self.original_raw,
                &digest(&self.original_raw),
                &candidate,
                &candidate_qualification,
                &previous,
                &previous_qualification,
                &verify,
            )?
        } else {
            let original: Value = serde_json::from_slice(&self.original_raw)
                .map_err(|_| "WAKE_SAME_STATE_SELECTION_INVALID")?;
            let legacy = original
                .get("selection")
                .is_none()
                .then_some(previous_qualification.as_str());
            install_selection::propose_install_selection(
                &self.original_raw,
                &digest(&self.original_raw),
                &candidate,
                &candidate_qualification,
                legacy,
                &verify,
            )?
        };
        let original: Value = serde_json::from_slice(&self.original_raw)
            .map_err(|_| "WAKE_SAME_STATE_SELECTION_INVALID")?;
        Ok(SameStateObservedV3 {
            now_utc_seconds: admission.accepted_at_utc_seconds,
            next_install_generation: admission.install_generation,
            policy_sha256: policy.readback()?.policy_sha256,
            state_snapshot: self.snapshot.clone(),
            candidate_manifest_sha256: self.candidate.manifest_sha256()?,
            candidate_runtime_manifest_sha256: self.candidate.runtime_manifest_sha256(),
            previous_manifest_sha256: self.previous.manifest_sha256()?,
            previous_runtime_manifest_sha256: self.previous.runtime_manifest_sha256(),
            planned_selection_sha256: digest(&self.planned),
            allow_reviewed_reconstruction: policy
                .same_state_resource_v2()?
                .allow_reviewed_reconstruction,
            approved_reviewer_identity: policy.same_state_reviewer_identity_v2()?.into(),
            legacy_current_package_id: original
                .get("selection")
                .is_none()
                .then(|| install_id(&original).map(str::to_owned))
                .transpose()?,
            retained_resume: self.resume.clone(),
        })
    }
    fn pin_receipts(&mut self, payload: &SameStateRootSelectionApprovalV3) -> Result<()> {
        let previous = payload.rollback_authority.release();
        let working = previous
            .working_evidence
            .as_ref()
            .ok_or("WAKE_SAME_STATE_WORKING_EVIDENCE_REQUIRED")?;
        let mut roles = vec![
            (
                "candidate-local.json",
                payload
                    .candidate
                    .local_qualification
                    .receipt_sha256
                    .as_str(),
            ),
            (
                "candidate-review.json",
                payload.candidate.independent_review.receipt_sha256.as_str(),
            ),
            (
                "previous-local.json",
                previous.local_qualification.receipt_sha256.as_str(),
            ),
            (
                "previous-review.json",
                previous.independent_review.receipt_sha256.as_str(),
            ),
            ("previous-working.json", working.receipt_sha256.as_str()),
        ];
        if let crate::install_broker_approval::same_state::SameStateRollbackAuthorityV3::ReviewedSourceReconstruction{reconstruction_receipt_sha256,..}=&payload.rollback_authority {
            roles.push(("reconstruction.json",reconstruction_receipt_sha256));
        }
        if self.receipts.is_empty() {
            self._receipt_directory = Some(OfflineObjectPinV2::same_state_directory(
                &self.receipt_root,
            )?);
            for (name, expected) in &roles {
                let path = self.receipt_root.join(name);
                let mut pin = OfflineObjectPinV2::open(&path, false)?;
                let raw = pin.read_bounded(1024 * 1024)?;
                if digest(&raw) != *expected {
                    return Err("WAKE_SAME_STATE_RECEIPT_MISMATCH".into());
                }
                validate_receipt(name, &raw, payload)?;
                self.receipts.push((path, pin, (*expected).into()));
            }
        }
        if self.receipts.len() != roles.len()
            || self
                .receipts
                .iter()
                .zip(&roles)
                .any(|((path, _, sha), (name, expected))| {
                    path.file_name().is_none_or(|v| v != *name) || sha != *expected
                })
        {
            return Err("WAKE_SAME_STATE_RECEIPT_MISMATCH".into());
        }
        for ((_, pin, expected), (name, _)) in self.receipts.iter_mut().zip(&roles) {
            let raw = pin.read_bounded(1024 * 1024)?;
            if digest(&raw) != *expected {
                return Err("WAKE_SAME_STATE_RECEIPT_MISMATCH".into());
            }
            validate_receipt(name, &raw, payload)?;
        }
        Ok(())
    }
    pub(crate) fn revalidate_for_payload(
        &mut self,
        payload: &SameStateRootSelectionApprovalV3,
    ) -> Result<()> {
        self.revalidate()?;
        let mut comparison = self.snapshot.clone();
        comparison.current = payload.state_snapshot.current.clone();
        let before = self.snapshot.current == payload.state_snapshot.current;
        let signed_snapshot_sha = digest(
            &serde_json::to_vec(&payload.state_snapshot)
                .map_err(|_| "WAKE_SAME_STATE_APPROVAL_INVALID")?,
        );
        let after = self.resume.as_ref().is_some_and(|r| {
            r.selection_attempt_present
                && r.original_snapshot_sha256 == signed_snapshot_sha
                && r.before_selection_sha256 == payload.state_snapshot.current.sha256
                && r.planned_selection_sha256 == payload.planned_selection_sha256
                && r.original_escrow_sha256 == digest(&self.original_raw)
                && self.snapshot.current.sha256 == payload.planned_selection_sha256
        });
        if comparison != payload.state_snapshot
            || (!before && !after)
            || digest(&self.planned) != payload.planned_selection_sha256
            || self.candidate.manifest() != &payload.candidate.manifest
            || self.previous.manifest() != &payload.rollback_authority.release().manifest
        {
            return Err("WAKE_SAME_STATE_SELECTION_CHANGED".into());
        }
        validate_same_state_runtime_manifest_v3(
            self.candidate.runtime_manifest_bytes(),
            &payload.candidate,
        )?;
        validate_same_state_runtime_manifest_v3(
            self.previous.runtime_manifest_bytes(),
            payload.rollback_authority.release(),
        )?;
        self.pin_receipts(payload)
    }
    pub(crate) fn revalidate(&mut self) -> Result<()> {
        for (index, path) in self
            .root
            .ancestors()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .enumerate()
        {
            self._ancestors[index].revalidate_path(path)?;
        }
        let pin = self
            .current
            .as_mut()
            .ok_or("WAKE_SAME_STATE_CURRENT_HANDOFF_ACTIVE")?;
        pin.revalidate_path(&self.root.join("current.json"))?;
        if pin.read_bounded(64 * 1024)? != self.current_raw {
            return Err("WAKE_SAME_STATE_SELECTION_CHANGED".into());
        }
        for (name, pin, expected) in [
            (
                "control.json",
                &mut self.control,
                &self.snapshot.control.sha256,
            ),
            (
                "config.json",
                &mut self.config,
                &self.snapshot.config.sha256,
            ),
        ] {
            pin.revalidate_path(&self.root.join(name))?;
            if digest(&pin.read_bounded(64 * 1024)?) != *expected {
                return Err("WAKE_SAME_STATE_SNAPSHOT_CHANGED".into());
            }
        }
        self.candidate.revalidate()?;
        self.previous.revalidate()?;
        for (path, pin, expected) in &mut self.receipts {
            pin.revalidate_path(path)?;
            if digest(&pin.read_bounded(1024 * 1024)?) != *expected {
                return Err("WAKE_SAME_STATE_RECEIPT_MISMATCH".into());
            }
        }
        Ok(())
    }
}

pub(crate) struct SameStateInstallResources<'p> {
    grant: PolicyVerifiedSameStateSelectionV3<'p>,
    store: Store,
    _install: ReviewedInstallLease,
    _root: crate::install_authority::HostRootPin,
    _host: HostLease,
    _browser: File,
    _legacy_images: Vec<OfflineObjectPinV2>,
}

impl<'p> SameStateInstallResources<'p> {
    pub(crate) fn acquire(grant: PolicyVerifiedSameStateSelectionV3<'p>) -> Result<Self> {
        grant.revalidate()?;
        let path = grant.pins_mut()?.root().to_owned();
        // Existing-only validation already ran; opening this same root cannot
        // create a new profile or substitute an environment-specific store.
        let store = Store::open_existing_writer(&path)?;
        let install = reviewed_install_lease(&store)?;
        if reviewed_install_handoff_active(&store)? {
            let transaction = read_reviewed_install_handoff(&store)?;
            let binding = transaction
                .same_state
                .ok_or("HOST_INSTALL_HANDOFF_CONFLICT")?;
            if binding.approval_envelope_sha256 != digest(grant.envelope_bytes())
                || binding.before_selection_sha256 != grant.payload().state_snapshot.current.sha256
                || binding.planned_selection_sha256 != grant.payload().planned_selection_sha256
                || transaction.prior_desired != "STOPPED"
            {
                return Err("HOST_INSTALL_HANDOFF_CONFLICT".into());
            }
        }
        let root = crate::install_authority::HostRootPin::acquire(store.root())?;
        let host = stopped_host_lease(&store)?;
        let browser = acquire_browser_quiescence(&store)?;
        let original: Value = serde_json::from_slice(&grant.original_selection_bytes()?)
            .map_err(|_| "WAKE_SAME_STATE_SELECTION_INVALID")?;
        if grant.pins_mut()?.previous.manifest().package_id == install_id(&original)? {
            // The old image must become execution-exclusive. Do not drop our
            // own fallback read pins to manufacture that capability. A fixed
            // inbox copy of that ID would later reopen the excluded installed
            // image too, so this transition requires a distinct fallback ID.
            return Err("WAKE_SAME_STATE_LEGACY_PREVIOUS_IMAGE_OVERLAP".into());
        }
        let mut legacy_images = Vec::new();
        for (name, field) in [
            ("CatDeskWakeHost.exe", "hostSha256"),
            ("CatDeskBinagotchy.exe", "binagotchySha256"),
        ] {
            let image = store
                .root()
                .join("versions")
                .join(install_id(&original)?)
                .join(name);
            let pin = OfflineObjectPinV2::open(&image, false)?;
            let mut pin = pin.exclude_new_image_opens(&image)?;
            if digest(&pin.read_bounded(80 * 1024 * 1024)?) != pointer_hash(&original, field)? {
                return Err("WAKE_SAME_STATE_LEGACY_IMAGE_MISMATCH".into());
            }
            legacy_images.push(pin);
        }
        // The old supported start_executable waits at most 100*100ms for the
        // host lease. Drain already-captured selector calls while exact old
        // images cannot be opened and our singleton remains held. This proves
        // only known selector actors; it is not arbitrary launcher quarantine.
        let drain = Instant::now() + Duration::from_secs(10);
        loop {
            let inventory = crate::install_broker_legacy_owner::inspect_legacy_owner_candidates();
            if inventory.disposition!=crate::install_broker_legacy_owner::LegacyOwnerDispositionV1::NoMatchingNamedProcessInSnapshot || !inventory.processes.is_empty() {
                return Err("WAKE_SAME_STATE_LEGACY_OWNER_UNPROVEN".into());
            }
            if Instant::now() >= drain {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        grant.revalidate()?;
        Ok(Self {
            grant,
            store,
            _install: install,
            _root: root,
            _host: host,
            _browser: browser,
            _legacy_images: legacy_images,
        })
    }
    pub(crate) fn grant(&self) -> &PolicyVerifiedSameStateSelectionV3<'p> {
        &self.grant
    }

    pub(crate) fn commit_selection(
        self,
        ledger: &mut AdmittedSameStateLedgerLeaseV3<'_, 'p>,
    ) -> Result<Value> {
        self.grant.revalidate()?;
        let payload = self.grant.payload();
        if ledger.binding()
            != &self
                .grant
                .ledger_binding(ledger.binding().accepted_at_utc_seconds)?
        {
            return Err("WAKE_SAME_STATE_LEDGER_BINDING_INVALID".into());
        }
        let candidate_id = payload.candidate.manifest.package_id.clone();
        let previous_id = payload
            .rollback_authority
            .release()
            .manifest
            .package_id
            .clone();
        let envelope_sha = digest(self.grant.envelope_bytes());
        let binding = SameStateHandoffV3 {
            schema_version: 3,
            approval_envelope_sha256: envelope_sha.clone(),
            before_selection_sha256: ledger.binding().before_selection_sha256.clone(),
            planned_selection_sha256: ledger.binding().planned_selection_sha256.clone(),
            selection_write_started: false,
        };
        let handoff_path = reviewed_install_transaction_path(&self.store);
        if ledger.phase() != InstallPhaseV1::Complete && !handoff_path.exists() {
            atomic(
                &handoff_path,
                &ReviewedInstallTransaction {
                    schema_version: 1,
                    candidate_directory: candidate_id.clone(),
                    prior_desired: "STOPPED".into(),
                    operator_stop_requested: false,
                    same_state: Some(binding.clone()),
                },
            )?;
        }
        if handoff_path.exists() {
            self.require_handoff(&binding)?;
        }
        if ledger.phase() == InstallPhaseV1::Intent {
            ledger.record_phase(InstallPhaseV1::StageStarted, None)?;
            let mut pins = self.grant.pins_mut()?;
            materialize(
                &self.store,
                &mut pins.candidate,
                &stage_name(&envelope_sha, "candidate"),
            )?;
            if !self
                .store
                .root()
                .join("versions")
                .join(&previous_id)
                .exists()
            {
                materialize(
                    &self.store,
                    &mut pins.previous,
                    &stage_name(&envelope_sha, "previous"),
                )?;
            }
        }
        if ledger.phase() == InstallPhaseV1::StageStarted {
            let pins = self.grant.pins_mut()?;
            verify_package(
                &self.store,
                &stage_name(&envelope_sha, "candidate"),
                &pins.candidate,
            )?;
            if !self
                .store
                .root()
                .join("versions")
                .join(&previous_id)
                .exists()
            {
                verify_package(
                    &self.store,
                    &stage_name(&envelope_sha, "previous"),
                    &pins.previous,
                )?;
            } else {
                verify_package(&self.store, &previous_id, &pins.previous)?;
            }
            drop(pins);
            ledger.record_phase(InstallPhaseV1::Staged, None)?;
        }
        if ledger.phase() == InstallPhaseV1::Staged {
            // A publication attempt is sticky. After a crash, only two exact
            // complete destinations prove publication; never repeat a rename.
            if ledger.publication_attempt_present()? {
                let pins = self.grant.pins_mut()?;
                verify_package(&self.store, &candidate_id, &pins.candidate)?;
                verify_package(&self.store, &previous_id, &pins.previous)?;
            } else {
                ledger.record_publication_attempt()?;
                let pins = self.grant.pins_mut()?;
                let ids = [candidate_id.as_str(), previous_id.as_str()];
                if !self
                    .store
                    .root()
                    .join("versions")
                    .join(&previous_id)
                    .exists()
                {
                    publish_package(
                        &self.store,
                        &pins.previous,
                        &stage_name(&envelope_sha, "previous"),
                        &ids,
                        &self._install,
                    )?;
                }
                publish_package(
                    &self.store,
                    &pins.candidate,
                    &stage_name(&envelope_sha, "candidate"),
                    &ids,
                    &self._install,
                )?;
            }
            ledger.record_phase(InstallPhaseV1::Published, None)?;
        }
        if ledger.phase() == InstallPhaseV1::Published {
            let pins = self.grant.pins_mut()?;
            verify_package(&self.store, &candidate_id, &pins.candidate)?;
            verify_package(&self.store, &previous_id, &pins.previous)?;
            drop(pins);
            self.grant.revalidate()?;
            // Actual stopped singleton/browser leases remain owned by self.
            ledger.record_phase(InstallPhaseV1::OldOwnerStopped, None)?;
        }
        if ledger.phase() == InstallPhaseV1::OldOwnerStopped {
            self.grant.revalidate()?;
            ledger.record_pointer_attempt()?;
            let mut transaction = self.require_handoff(&binding)?;
            transaction
                .same_state
                .as_mut()
                .ok_or("HOST_INSTALL_HANDOFF_CONFLICT")?
                .selection_write_started = true;
            atomic(&handoff_path, &transaction)?;
            let mut pins = self.grant.pins_mut()?;
            let planned = pins.planned.clone();
            let current_hash = digest(&pins.current_raw);
            if current_hash == ledger.binding().before_selection_sha256 {
                pins.resume = Some(SameStateRetainedResumeV3 {
                    original_snapshot_sha256: ledger.binding().state_snapshot_sha256.clone(),
                    before_selection_sha256: ledger.binding().before_selection_sha256.clone(),
                    planned_selection_sha256: ledger.binding().planned_selection_sha256.clone(),
                    original_escrow_sha256: digest(&pins.original_raw),
                    selection_attempt_present: true,
                });
                // Release ONLY the old pointer's incompatible DELETE share at
                // the already-journaled atomic writer boundary. All root,
                // package, config/control and ownership pins remain held.
                drop(pins.current.take());
                crate::store::atomic_bytes(&self.store.root().join("current.json"), &planned)?;
                let mut selected =
                    OfflineObjectPinV2::open(&self.store.root().join("current.json"), false)?;
                let (object, raw) = object(&mut selected)?;
                if raw != planned {
                    return Err("WAKE_SAME_STATE_SELECTION_UNKNOWN".into());
                }
                pins.snapshot.current = object;
                pins.current_raw = raw;
                pins.current = Some(selected);
            } else if current_hash != ledger.binding().planned_selection_sha256 {
                return Err("WAKE_SAME_STATE_SELECTION_UNKNOWN".into());
            }
            drop(pins);
            self.grant.revalidate()?;
            ledger.record_phase(
                InstallPhaseV1::PointerSwitched,
                Some(&ledger.binding().planned_selection_sha256.clone()),
            )?;
        }
        if ledger.phase() == InstallPhaseV1::PointerSwitched {
            self.grant.revalidate()?;
            ledger.record_phase(
                InstallPhaseV1::NewOwnerProven,
                Some(&ledger.binding().planned_selection_sha256.clone()),
            )?;
        }
        if ledger.phase() == InstallPhaseV1::NewOwnerProven {
            self.grant.revalidate()?;
            ledger.record_phase(
                InstallPhaseV1::Complete,
                Some(&ledger.binding().planned_selection_sha256.clone()),
            )?;
        }
        let complete = ledger.complete_record_sha256()?.to_owned();
        self.grant.revalidate()?;
        if handoff_path.exists() {
            self.require_handoff(&binding)?;
            std::fs::remove_file(&handoff_path)
                .map_err(|_| "HOST_INSTALL_HANDOFF_CLEANUP_FAILED")?;
        }
        Ok(
            json!({"action":"same_state_wake_selection","status":"SELECTED_STOPPED","current":candidate_id,"previous":previous_id,
            "selectionSha256":ledger.binding().planned_selection_sha256,"completeRecordSha256":complete,
            "hostStarted":false,"browserStarted":false,"messagesSent":0,"workingAcceptance":false}),
        )
    }

    fn require_handoff(&self, expected: &SameStateHandoffV3) -> Result<ReviewedInstallTransaction> {
        let transaction = read_reviewed_install_handoff(&self.store)?;
        let actual = transaction
            .same_state
            .as_ref()
            .ok_or("HOST_INSTALL_HANDOFF_CONFLICT")?;
        let mut comparison = actual.clone();
        comparison.selection_write_started = false;
        if transaction.candidate_directory != self.grant.payload().candidate.manifest.package_id
            || transaction.prior_desired != "STOPPED"
            || &comparison != expected
        {
            return Err("HOST_INSTALL_HANDOFF_CONFLICT".into());
        }
        Ok(transaction)
    }
}

fn stage_name(envelope: &str, role: &str) -> String {
    format!(".staging-v3-{}-{role}", &envelope[..32])
}

fn materialize(store: &Store, source: &mut RuntimePackagePinsV3, name: &str) -> Result<()> {
    source.revalidate()?;
    let directory = store.root().join("versions").join(name);
    std::fs::create_dir(&directory).map_err(|_| "WAKE_SAME_STATE_STAGE_ATTENTION")?;
    use std::io::{Seek, SeekFrom, Write};
    for index in 0..4 {
        let description = source.manifest().artifacts[index].clone();
        let mut target = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(&description.name))
            .map_err(|_| "WAKE_SAME_STATE_STAGE_ATTENTION")?;
        let input = source.artifact_mut(index);
        input
            .seek(SeekFrom::Start(0))
            .map_err(|_| "WAKE_SAME_STATE_STAGE_ATTENTION")?;
        let copied = std::io::copy(&mut input.take(description.byte_length + 1), &mut target)
            .map_err(|_| "WAKE_SAME_STATE_STAGE_ATTENTION")?;
        if copied != description.byte_length {
            return Err("WAKE_SAME_STATE_STAGE_ATTENTION".into());
        }
        target
            .sync_all()
            .map_err(|_| "WAKE_SAME_STATE_STAGE_ATTENTION")?;
    }
    let mut manifest = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("manifest.json"))
        .map_err(|_| "WAKE_SAME_STATE_STAGE_ATTENTION")?;
    manifest
        .write_all(source.runtime_manifest_bytes())
        .and_then(|_| manifest.sync_all())
        .map_err(|_| "WAKE_SAME_STATE_STAGE_ATTENTION")?;
    // Native readback deliberately denies write sharing. Close our own stage
    // writer before requiring that stronger immutable-source handle.
    drop(manifest);
    source.revalidate()?;
    verify_package(store, name, source)
}

fn verify_package(store: &Store, id: &str, source: &RuntimePackagePinsV3) -> Result<()> {
    let observed = RuntimePackagePinsV3::installed(store.root(), id)?;
    if observed.manifest() != source.manifest()
        || observed.runtime_manifest_bytes() != source.runtime_manifest_bytes()
    {
        return Err("WAKE_SAME_STATE_PUBLISHED_PACKAGE_MISMATCH".into());
    }
    Ok(())
}

fn publish_package(
    store: &Store,
    source: &RuntimePackagePinsV3,
    stage: &str,
    allowed: &[&str],
    lease: &ReviewedInstallLease,
) -> Result<()> {
    let m = source.manifest();
    let hashes = ReviewedInstallExpectedHashes::new(
        &m.artifacts[0].sha256,
        &m.artifacts[1].sha256,
        &m.artifacts[2].sha256,
        &m.artifacts[3].sha256,
    )?;
    publish_reviewed_install_owned(
        store,
        stage,
        &m.package_id,
        Some(&hashes),
        &m.version,
        allowed,
        lease,
    )?;
    verify_package(store, &m.package_id, source)
}

#[cfg(test)]
#[path = "runtime_same_state_install_tests.rs"]
mod tests;
