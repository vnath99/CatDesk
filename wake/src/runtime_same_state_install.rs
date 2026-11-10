//! Retained existing-root resources for the purpose-specific signed V3 grant.
//! No public path, key, verifier closure or Boolean can enter this transaction.
#![cfg(windows)]
use super::*;
use crate::{
    install_broker_approval::same_state::{
        SameStateObjectV3, SameStateObservedV3, SameStatePreviousSelectionV3,
        SameStateRetainedResumeV3, SameStateRootSelectionApprovalV3, SameStateSnapshotV3,
        validate_same_state_runtime_manifest_v3,
    },
    install_broker_ledger::{
        AdmittedSameStateLedgerLeaseV3, InstallPhaseV1,
        SameStateFailedCurrentImageV3 as FailedCurrentImageV3, SameStateLedgerAdmissionV3,
    },
    install_broker_offline_pins::OfflineObjectPinV2,
    install_broker_policy::{BrokerProtectedPolicyV1, PolicyVerifiedSameStateSelectionV3},
    install_broker_source::{
        RuntimePackagePinsV3, RuntimePackagePublicationV3, RuntimePackageStageV3,
        RuntimeSourceRoleV3,
    },
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

fn original_previous(raw: &[u8]) -> Result<SameStatePreviousSelectionV3> {
    let inspected = install_selection::inspect_install_selection(raw)?;
    let previous = &inspected["selection"]["previous"];
    let pointer = previous
        .get("pointer")
        .ok_or("WAKE_SAME_STATE_ROLLBACK_PREVIOUS_REQUIRED")?
        .clone();
    let qualification_sha256 = previous["qualificationSha256"]
        .as_str()
        .ok_or("WAKE_SAME_STATE_ROLLBACK_PREVIOUS_REQUIRED")?
        .to_owned();
    Ok(SameStatePreviousSelectionV3 {
        pointer,
        qualification_sha256,
    })
}

/// Actual read handles only. Its factory derives all paths from fixed policy
/// and fixed submission roles. It grants no mutation authority.
pub(crate) struct SameStateReadPinsV3 {
    rollback: bool,
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
    pub(crate) fn observe_for_operation(
        policy: &BrokerProtectedPolicyV1,
        admission: &SameStateLedgerAdmissionV3,
        operation: &str,
    ) -> Result<Self> {
        match operation {
            "SELECT_SAME_STATE_RELEASE" => Self::observe(policy, admission),
            "ROLLBACK_SAME_STATE_RELEASE" => {
                Self::observe_with_mode(policy, admission, SameStateIntakeV3::Fixed, true)
            }
            _ => Err("WAKE_SAME_STATE_OPERATION_INVALID".into()),
        }
    }
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
        Self::observe_with_mode(policy, admission, intake, false)
    }
    fn observe_with_mode(
        policy: &BrokerProtectedPolicyV1,
        admission: &SameStateLedgerAdmissionV3,
        intake: SameStateIntakeV3,
        rollback: bool,
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
        let target = if rollback {
            Some(original_previous(&original_raw)?)
        } else {
            None
        };
        let candidate = if let Some(target) = &target {
            RuntimePackagePinsV3::installed(&root, install_id(&target.pointer)?)?
        } else {
            match &intake {
                SameStateIntakeV3::Fixed => {
                    RuntimePackagePinsV3::fixed(RuntimeSourceRoleV3::Candidate)?
                }
                #[cfg(test)]
                SameStateIntakeV3::Isolated(path) => {
                    RuntimePackagePinsV3::isolated(path, RuntimeSourceRoleV3::Candidate)?
                }
            }
        };
        let original: Value = serde_json::from_slice(&original_raw)
            .map_err(|_| "WAKE_SAME_STATE_SELECTION_INVALID")?;
        let previous = if let Some(target) = &target {
            RuntimePackagePinsV3::installed(&root, install_id(&target.pointer)?)?
        } else {
            match &intake {
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
            rollback,
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
        if self.rollback != (payload.operation == "ROLLBACK_SAME_STATE_RELEASE") {
            return Err("WAKE_SAME_STATE_OPERATION_INVALID".into());
        }
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
        self.planned = if self.rollback {
            let target = original_previous(&self.original_raw)?;
            if target.pointer != candidate
                || target.qualification_sha256 != candidate_qualification
                || payload.candidate != *previous_release
                || payload.rollback_authority.is_reconstruction()
            {
                return Err("WAKE_SAME_STATE_ROLLBACK_TARGET_MISMATCH".into());
            }
            install_selection::propose_install_selection_rollback(
                &self.original_raw,
                &digest(&self.original_raw),
                &verify,
            )?
        } else if payload.rollback_authority.is_reconstruction() {
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
            original_previous: if self.rollback {
                Some(original_previous(&self.original_raw)?)
            } else {
                None
            },
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
        if self.rollback {
            roles.retain(|(name, _)| name.starts_with("previous-"));
        }
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
        if self.rollback != (payload.operation == "ROLLBACK_SAME_STATE_RELEASE")
            || comparison != payload.state_snapshot
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
    failed_image_observations: Vec<FailedCurrentImageV3>,
    missing_images: Vec<PathBuf>,
    _failed_image_parents: Vec<(PathBuf, OfflineObjectPinV2)>,
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
        let rollback = grant.payload().operation == "ROLLBACK_SAME_STATE_RELEASE";
        let mut failed_image_observations = Vec::new();
        let mut missing_images = Vec::new();
        let mut failed_image_parents = Vec::new();
        if rollback {
            let versions = store.root().join("versions");
            failed_image_parents.push((
                versions.clone(),
                OfflineObjectPinV2::same_state_directory(&versions)?,
            ));
            let old_directory = versions.join(install_id(&original)?);
            match std::fs::symlink_metadata(&old_directory) {
                Ok(_) => failed_image_parents.push((
                    old_directory.clone(),
                    OfflineObjectPinV2::same_state_directory(&old_directory)?,
                )),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("WAKE_SAME_STATE_FAILED_IMAGE_OBSERVATION_UNAVAILABLE".into()),
            }
        }
        for (name, field) in [
            ("CatDeskWakeHost.exe", "hostSha256"),
            ("CatDeskBinagotchy.exe", "binagotchySha256"),
        ] {
            let image = store
                .root()
                .join("versions")
                .join(install_id(&original)?)
                .join(name);
            if rollback {
                match std::fs::symlink_metadata(&image) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        failed_image_observations.push(FailedCurrentImageV3 {
                            name: name.into(),
                            actual_sha256: None,
                            byte_length: None,
                            identity: None,
                        });
                        missing_images.push(image);
                        continue;
                    }
                    Err(_) => {
                        return Err("WAKE_SAME_STATE_FAILED_IMAGE_OBSERVATION_UNAVAILABLE".into());
                    }
                    Ok(_) => {}
                }
            }
            let pin = OfflineObjectPinV2::open(&image, false)?;
            let mut pin = pin.exclude_new_image_opens(&image)?;
            let actual_raw = pin.read_bounded(80 * 1024 * 1024)?;
            let actual_sha256 = digest(&actual_raw);
            if !rollback && actual_sha256 != pointer_hash(&original, field)? {
                return Err("WAKE_SAME_STATE_LEGACY_IMAGE_MISMATCH".into());
            }
            if rollback {
                failed_image_observations.push(FailedCurrentImageV3 {
                    name: name.into(),
                    actual_sha256: Some(actual_sha256),
                    byte_length: Some(actual_raw.len() as u64),
                    identity: Some(pin.identity().full.clone()),
                });
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
            failed_image_observations,
            missing_images,
            _failed_image_parents: failed_image_parents,
        })
    }
    pub(crate) fn grant(&self) -> &PolicyVerifiedSameStateSelectionV3<'p> {
        &self.grant
    }
    fn revalidate_failed_images(&mut self) -> Result<()> {
        if self.grant.payload().operation != "ROLLBACK_SAME_STATE_RELEASE" {
            return Ok(());
        }
        for (path, pin) in &self._failed_image_parents {
            pin.revalidate_path(path)?;
        }
        for path in &self.missing_images {
            if !matches!(std::fs::symlink_metadata(path), Err(ref error) if error.kind() == std::io::ErrorKind::NotFound)
            {
                return Err("WAKE_SAME_STATE_FAILED_IMAGE_CHANGED".into());
            }
        }
        let mut held = self._legacy_images.iter_mut();
        for observation in &self.failed_image_observations {
            if let Some(expected) = &observation.actual_sha256 {
                let pin = held.next().ok_or("WAKE_SAME_STATE_FAILED_IMAGE_CHANGED")?;
                let raw = pin.read_bounded(80 * 1024 * 1024)?;
                if digest(&raw) != *expected
                    || Some(raw.len() as u64) != observation.byte_length
                    || Some(&pin.identity().full) != observation.identity.as_ref()
                {
                    return Err("WAKE_SAME_STATE_FAILED_IMAGE_CHANGED".into());
                }
            }
        }
        Ok(())
    }

    pub(crate) fn commit_selection(
        self,
        ledger: &mut AdmittedSameStateLedgerLeaseV3<'_, 'p>,
    ) -> Result<Value> {
        self.commit_selection_inner(ledger, None)
    }
    #[cfg(test)]
    fn commit_selection_until_publication(
        self,
        ledger: &mut AdmittedSameStateLedgerLeaseV3<'_, 'p>,
    ) -> Result<Value> {
        self.commit_selection_inner(ledger, Some("after-publication"))
    }
    #[cfg(test)]
    fn commit_selection_until(
        self,
        ledger: &mut AdmittedSameStateLedgerLeaseV3<'_, 'p>,
        stop_after: InstallPhaseV1,
    ) -> Result<Value> {
        self.commit_selection_inner(
            ledger,
            Some(if stop_after == InstallPhaseV1::OldOwnerStopped {
                "before-pointer"
            } else {
                "after-pointer"
            }),
        )
    }
    fn commit_selection_inner(
        mut self,
        ledger: &mut AdmittedSameStateLedgerLeaseV3<'_, 'p>,
        stop_after: Option<&str>,
    ) -> Result<Value> {
        self.grant.revalidate()?;
        self.revalidate_failed_images()?;
        let payload = self.grant.payload().clone();
        let rollback = payload.operation == "ROLLBACK_SAME_STATE_RELEASE";
        if rollback
            && let Some(retained) = ledger.failed_current_images()?
            && retained != self.failed_image_observations
        {
            return Err("WAKE_SAME_STATE_FAILED_IMAGE_CHANGED".into());
        }
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
        if rollback && ledger.phase() == InstallPhaseV1::Intent {
            // Existing predecessor pins and all STOPPED ownership leases are
            // retained. No package creation, staging or publication occurs.
            self.grant.revalidate()?;
            self.revalidate_failed_images()?;
            ledger.record_rollback_stopped(&self.failed_image_observations)?;
            if stop_after == Some("before-pointer") {
                return Err("FIXTURE_SAME_STATE_COMMIT_INTERRUPTED".into());
            }
        }
        if !rollback && ledger.phase() == InstallPhaseV1::Intent {
            ledger.record_phase(InstallPhaseV1::StageStarted, None)?;
            let mut pins = self.grant.pins_mut()?;
            materialize(
                &self.store,
                &mut pins.candidate,
                &stage_name(&envelope_sha, "candidate"),
                self.grant.policy(),
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
                    self.grant.policy(),
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
            let publication_pins = if ledger.publication_attempt_present()? {
                let pins = self.grant.pins_mut()?;
                verify_package(&self.store, &candidate_id, &pins.candidate)?;
                verify_package(&self.store, &previous_id, &pins.previous)?;
                Vec::new()
            } else {
                let publication = RuntimePackagePublicationV3::prepare(
                    self.store.root(),
                    &payload,
                    &envelope_sha,
                    ledger,
                    self.grant.policy(),
                )?;
                self.grant.revalidate()?;
                ledger.record_publication_attempt()?;
                publication.publish(ledger)?
            };
            if stop_after == Some("after-publication") {
                return Err("FIXTURE_SAME_STATE_COMMIT_INTERRUPTED".into());
            }
            ledger.record_phase(InstallPhaseV1::Published, None)?;
            drop(publication_pins);
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
            self.revalidate_failed_images()?;
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
            if stop_after == Some("after-pointer") {
                return Err("FIXTURE_SAME_STATE_COMMIT_INTERRUPTED".into());
            }
            ledger.record_phase(
                InstallPhaseV1::PointerSwitched,
                Some(&ledger.binding().planned_selection_sha256.clone()),
            )?;
        }
        if ledger.phase() == InstallPhaseV1::PointerSwitched {
            self.grant.revalidate()?;
            self.revalidate_failed_images()?;
            ledger.record_phase(
                InstallPhaseV1::NewOwnerProven,
                Some(&ledger.binding().planned_selection_sha256.clone()),
            )?;
        }
        if ledger.phase() == InstallPhaseV1::NewOwnerProven {
            self.grant.revalidate()?;
            self.revalidate_failed_images()?;
            ledger.record_phase(
                InstallPhaseV1::Complete,
                Some(&ledger.binding().planned_selection_sha256.clone()),
            )?;
        }
        let complete = ledger.complete_record_sha256()?.to_owned();
        self.grant.revalidate()?;
        self.revalidate_failed_images()?;
        if handoff_path.exists() {
            self.require_handoff(&binding)?;
            std::fs::remove_file(&handoff_path)
                .map_err(|_| "HOST_INSTALL_HANDOFF_CLEANUP_FAILED")?;
        }
        if rollback {
            return Ok(
                json!({"action":"same_state_wake_rollback","status":"ROLLED_BACK_STOPPED",
                "current":candidate_id,"previous":null,"selectionSha256":ledger.binding().planned_selection_sha256,
                "completeRecordSha256":complete,"failedCurrentImages":self.failed_image_observations,
                "failedArtifactsPreservedInPlace":true,"absenceBoundary":"SUPPORTED_SELECTORS_ONLY",
                "hostStarted":false,"browserStarted":false,"messagesSent":0,"workingAcceptance":false}),
            );
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

fn materialize(
    store: &Store,
    source: &mut RuntimePackagePinsV3,
    name: &str,
    policy: &BrokerProtectedPolicyV1,
) -> Result<()> {
    source.revalidate()?;
    let security =
        crate::install_broker_stage_writer::StageSecurityMode::from_same_state_policy(policy)?;
    let stage = RuntimePackageStageV3::create(store.root(), name, security)?;
    stage.copy_package(source)?;
    verify_package(store, name, source)?;
    stage.revalidate()
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

#[cfg(test)]
#[path = "runtime_same_state_install_tests.rs"]
mod tests;
