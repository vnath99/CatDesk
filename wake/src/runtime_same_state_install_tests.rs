//! Disposable actual-file/lease fixtures only; inert images are never run.
use super::*;
use crate::install_broker_approval::{ArtifactV1, PackageManifestV1, same_state::*};
use crate::install_broker_ledger::BrokerLedgerGuard;
use ed25519_dalek::SigningKey;
use std::os::windows::{fs::OpenOptionsExt, io::OwnedHandle};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn receipt(role: &str, payload: &SameStateRootSelectionApprovalV3) -> Value {
    let release = if role.starts_with("candidate-") {
        &payload.candidate
    } else {
        payload.rollback_authority.release()
    };
    let mut value = json!({"schemaVersion":3,"sourceCommit":release.source_commit,
        "manifestSha256":same_state_manifest_sha256(&release.manifest).unwrap(),"verdict":"PASS",
        "evidenceSha256":["1".repeat(64)]});
    match role {
        "candidate-local.json" | "previous-local.json" => {
            value["kind"] = json!("SOURCE_QUALIFICATION");
            value["scope"] = json!("EXACT_SOURCE");
        }
        "candidate-review.json" | "previous-review.json" => {
            value["kind"] = json!("INDEPENDENT_SOURCE_REVIEW");
            value["scope"] = json!("FULL_SOURCE");
            value["reviewerIdentity"] = json!(release.independent_review.reviewer_identity);
            value["fullSourceReviewed"] = json!(true);
            value["findingsCount"] = json!(0);
            value["neededReadsCount"] = json!(0);
        }
        "previous-working.json" => {
            let working = release.working_evidence.as_ref().unwrap();
            value["kind"] = json!("WAKE_WORKING");
            value["scope"] = json!(working.scope);
            value["observedUtcSeconds"] = json!(working.observed_utc_seconds);
            value["ownerVerified"] = json!(working.owner_verified);
            value["startupVerified"] = json!(working.startup_verified);
            value["normalStopVerified"] = json!(working.normal_stop_verified);
        }
        "reconstruction.json" => {
            value["kind"] = json!("REVIEWED_SOURCE_RECONSTRUCTION");
            value["scope"] = json!("LEGACY_SELECTION_RECONSTRUCTION");
            value["oldRawSelectionEscrowSha256"] = json!(payload.state_snapshot.current.sha256);
        }
        _ => panic!("unknown fixture role"),
    }
    value
}

#[test]
fn same_state_receipts_bind_full_review_and_wake_scope_to_retained_bytes() {
    let (payload, _, _) = crate::install_broker_approval::same_state::tests::fixture();
    for role in [
        "candidate-local.json",
        "candidate-review.json",
        "previous-local.json",
        "previous-review.json",
        "previous-working.json",
    ] {
        let valid = receipt(role, &payload);
        validate_receipt(role, &serde_json::to_vec(&valid).unwrap(), &payload).unwrap();
        for (field, value) in [
            ("sourceCommit", json!("f".repeat(40))),
            ("manifestSha256", json!("f".repeat(64))),
            ("verdict", json!("INCONCLUSIVE")),
            ("evidenceSha256", json!([])),
            ("unknown", json!(true)),
        ] {
            let mut changed = valid.clone();
            changed[field] = value;
            assert!(
                validate_receipt(role, &serde_json::to_vec(&changed).unwrap(), &payload).is_err(),
                "{role} {field}"
            );
        }
    }
    for (field, value) in [
        ("fullSourceReviewed", json!(false)),
        ("neededReadsCount", json!(1)),
        ("findingsCount", json!(1)),
        ("scope", json!("FOCUSED_PARTIAL")),
        ("reviewerIdentity", json!("other-reviewer")),
    ] {
        let mut changed = receipt("candidate-review.json", &payload);
        changed[field] = value;
        assert!(
            validate_receipt(
                "candidate-review.json",
                &serde_json::to_vec(&changed).unwrap(),
                &payload
            )
            .is_err()
        );
    }
    for (field, value) in [
        ("scope", json!("DEVELOPMENT_BACKEND")),
        ("ownerVerified", json!(false)),
        ("startupVerified", json!(false)),
        ("normalStopVerified", json!(false)),
        ("observedUtcSeconds", json!(801)),
    ] {
        let mut changed = receipt("previous-working.json", &payload);
        changed[field] = value;
        assert!(
            validate_receipt(
                "previous-working.json",
                &serde_json::to_vec(&changed).unwrap(),
                &payload
            )
            .is_err()
        );
    }
    let valid = serde_json::to_string(&receipt("candidate-review.json", &payload)).unwrap();
    let duplicate = valid.replacen(
        "\"schemaVersion\":3",
        "\"schemaVersion\":3,\"schemaVersion\":3",
        1,
    );
    assert!(validate_receipt("candidate-review.json", duplicate.as_bytes(), &payload).is_err());
    assert!(validate_receipt("previous-working.json", valid.as_bytes(), &payload).is_err());
}

fn write_package(
    path: &Path,
    version: &str,
    seed: u8,
    working: bool,
) -> SameStateQualifiedReleaseV3 {
    std::fs::create_dir_all(path).unwrap();
    let names = [
        "CatDeskWakeHost.exe",
        "CatDeskBinagotchy.exe",
        "adapter.py",
        "wake_bridge.py",
    ];
    let artifacts = std::array::from_fn(|index| {
        let raw = vec![seed + index as u8; 32 + index];
        std::fs::write(path.join(names[index]), &raw).unwrap();
        ArtifactV1 {
            name: names[index].into(),
            byte_length: raw.len() as u64,
            sha256: digest(&raw),
        }
    });
    let manifest = PackageManifestV1 {
        schema_version: 1,
        version: version.into(),
        package_id: format!(
            "{version}-{}-{}",
            &artifacts[0].sha256[..12],
            &artifacts[1].sha256[..12]
        ),
        artifacts,
    };
    let map = manifest
        .artifacts
        .iter()
        .map(|a| (a.name.clone(), a.sha256.clone()))
        .collect::<BTreeMap<_, _>>();
    let raw = serde_json::to_vec(&json!({"schemaVersion":1,"version":version,"protocolVersion":1,
        "installedUtc":"isolated-source-fixture","artifacts":map,"acceptance":"DEVELOPMENT_NOT_ACTIVATED"})).unwrap();
    std::fs::write(path.join("manifest.json"), &raw).unwrap();
    let sha = same_state_manifest_sha256(&manifest).unwrap();
    let source = format!("{seed:040x}");
    SameStateQualifiedReleaseV3 {
        manifest,
        runtime_manifest_sha256: digest(&raw),
        source_commit: source.clone(),
        local_qualification: SameStateLocalQualificationV3 {
            receipt_sha256: "a".repeat(64),
            source_commit: source.clone(),
            manifest_sha256: sha.clone(),
            verdict: SameStateEvidenceVerdictV3::Pass,
        },
        independent_review: SameStateIndependentReviewV3 {
            receipt_sha256: "b".repeat(64),
            source_commit: source.clone(),
            manifest_sha256: sha.clone(),
            reviewer_identity: "independent-source-reviewer".into(),
            verdict: SameStateEvidenceVerdictV3::Pass,
        },
        working_evidence: working.then_some(SameStateWorkingEvidenceV3 {
            scope: "WAKE_RUNTIME".into(),
            receipt_sha256: "c".repeat(64),
            source_commit: source,
            manifest_sha256: sha,
            observed_utc_seconds: 800,
            owner_verified: true,
            startup_verified: true,
            normal_stop_verified: true,
        }),
    }
}

struct Fixture {
    path: PathBuf,
    state: PathBuf,
    intake: PathBuf,
    broker: PathBuf,
    handle: OwnedHandle,
    policy: BrokerProtectedPolicyV1,
    payload: SameStateRootSelectionApprovalV3,
    before: Vec<u8>,
    protected: Vec<(PathBuf, Vec<u8>)>,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "catdesk-same-state-v3-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        let state = path.join("state");
        let intake = path.join("intake");
        let broker = path.join("broker");
        std::fs::create_dir_all(state.join("versions")).unwrap();
        std::fs::create_dir_all(intake.join("receipts")).unwrap();
        std::fs::create_dir(&broker).unwrap();
        let store = Store::open(&state).unwrap();
        store.initialize().unwrap();
        atomic(
            &state.join("control.json"),
            &Control {
                schema_version: 1,
                desired: "STOPPED".into(),
            },
        )
        .unwrap();
        for name in ["install.lock", "host.lock", "browser-session.lock"] {
            std::fs::write(state.join(name), b"").unwrap();
        }
        std::fs::write(broker.join("install-ledger.lock"), b"").unwrap();
        let old = write_package(
            &state.join("versions").join("old-source"),
            "1.0.0-dev.890",
            5,
            false,
        );
        std::fs::rename(
            state.join("versions").join("old-source"),
            state.join("versions").join(&old.manifest.package_id),
        )
        .unwrap();
        let oldpins = RuntimePackagePinsV3::installed(&state, &old.manifest.package_id).unwrap();
        let before = serde_json::to_vec(&oldpins.pointer()).unwrap();
        drop(oldpins);
        std::fs::write(state.join("current.json"), &before).unwrap();
        std::fs::create_dir(state.join("profile")).unwrap();
        std::fs::write(state.join("profile").join("preserve"), b"profile sentinel").unwrap();
        std::fs::create_dir(state.join("task-journals")).unwrap();
        std::fs::write(
            state.join("task-journals").join("unresolved"),
            b"unknown task must remain",
        )
        .unwrap();
        let candidate = write_package(&intake.join("candidate"), "1.0.0-dev.892", 17, false);
        let previous = write_package(&intake.join("previous"), "1.0.0-dev.891", 33, true);
        let root_pin = OfflineObjectPinV2::same_state_directory(&state).unwrap();
        let state_id = root_pin.identity().full.clone();
        drop(root_pin);
        let broker_pin = OfflineObjectPinV2::same_state_directory(&broker).unwrap();
        let broker_id = broker_pin.identity().legacy.clone();
        drop(broker_pin);
        let handle: OwnedHandle = OpenOptions::new()
            .access_mode(0x0012_0081)
            .share_mode(3)
            .custom_flags(0x0220_0000)
            .open(&broker)
            .unwrap()
            .into();
        let policy =
            crate::install_broker_policy::tests::load_same_state_fixture_policy_for_existing_root(
                &broker,
                &handle,
                &broker_id,
                &state,
                state_id,
                true,
                "release-reviewer",
            )
            .unwrap();
        let (mut payload, _, _) = crate::install_broker_approval::same_state::tests::fixture();
        payload.candidate = candidate;
        payload.rollback_authority = SameStateRollbackAuthorityV3::ReviewedSourceReconstruction {
            release: previous,
            old_raw_selection_escrow_sha256: digest(&before),
            reconstruction_receipt_sha256: "d".repeat(64),
        };
        payload.policy_sha256 = policy.readback().unwrap().policy_sha256;
        let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
        let admission = guard
            .same_state_admission_v3(&"0".repeat(64), 1000)
            .unwrap();
        let pins = SameStateReadPinsV3::observe_with_intake(
            &policy,
            &admission,
            SameStateIntakeV3::Isolated(intake.clone()),
        )
        .unwrap();
        payload.state_snapshot = pins.snapshot().clone();
        drop(pins);
        drop(guard);
        for role in [
            "candidate-local.json",
            "candidate-review.json",
            "previous-local.json",
            "previous-review.json",
            "previous-working.json",
            "reconstruction.json",
        ] {
            let raw = serde_json::to_vec(&receipt(role, &payload)).unwrap();
            let sha = digest(&raw);
            std::fs::write(intake.join("receipts").join(role), raw).unwrap();
            let previous = match &mut payload.rollback_authority {
                SameStateRollbackAuthorityV3::ReviewedSourceReconstruction { release, .. } => {
                    release
                }
                _ => unreachable!(),
            };
            match role {
                "candidate-local.json" => {
                    payload.candidate.local_qualification.receipt_sha256 = sha
                }
                "candidate-review.json" => {
                    payload.candidate.independent_review.receipt_sha256 = sha
                }
                "previous-local.json" => previous.local_qualification.receipt_sha256 = sha,
                "previous-review.json" => previous.independent_review.receipt_sha256 = sha,
                "previous-working.json" => {
                    previous.working_evidence.as_mut().unwrap().receipt_sha256 = sha
                }
                "reconstruction.json" => {
                    let SameStateRollbackAuthorityV3::ReviewedSourceReconstruction {
                        reconstruction_receipt_sha256,
                        ..
                    } = &mut payload.rollback_authority
                    else {
                        unreachable!()
                    };
                    *reconstruction_receipt_sha256 = sha;
                }
                _ => unreachable!(),
            }
        }
        let protected = [
            state.join("config.json"),
            state.join("control.json"),
            state.join("profile").join("preserve"),
            state.join("task-journals").join("unresolved"),
        ]
        .into_iter()
        .map(|p| {
            let b = std::fs::read(&p).unwrap();
            (p, b)
        })
        .collect();
        Self {
            path,
            state,
            intake,
            broker,
            handle,
            policy,
            payload,
            before,
            protected,
        }
    }
    fn pins(&self, admission: &SameStateLedgerAdmissionV3) -> SameStateReadPinsV3 {
        SameStateReadPinsV3::observe_with_mode(
            &self.policy,
            admission,
            SameStateIntakeV3::Isolated(self.intake.clone()),
            self.payload.operation == "ROLLBACK_SAME_STATE_RELEASE",
        )
        .unwrap()
    }
    fn rollback() -> Self {
        let mut fixture = Self::new();
        let previous = fixture.payload.rollback_authority.release().clone();
        std::fs::rename(
            fixture.intake.join("previous"),
            fixture
                .state
                .join("versions")
                .join(&previous.manifest.package_id),
        )
        .unwrap();
        let target =
            crate::install_broker_approval::same_state::same_state_release_pointer_v3(&previous)
                .unwrap();
        let qualification =
            crate::install_broker_approval::same_state::same_state_release_qualification_sha256(
                &previous,
            )
            .unwrap();
        let mut current: Value = serde_json::from_slice(&fixture.before).unwrap();
        current["selection"] = json!({"schemaVersion":2,"generation":7,
            "current":{"pointer":serde_json::from_slice::<Value>(&fixture.before).unwrap(),"qualificationSha256":"f".repeat(64)},
            "previous":{"pointer":target,"qualificationSha256":qualification}});
        fixture.before = serde_json::to_vec(&current).unwrap();
        std::fs::write(fixture.state.join("current.json"), &fixture.before).unwrap();
        fixture.payload.operation = "ROLLBACK_SAME_STATE_RELEASE".into();
        fixture.payload.approval_id = "rollback-fixture-one".into();
        fixture.payload.candidate = previous.clone();
        fixture.payload.rollback_authority =
            SameStateRollbackAuthorityV3::VerifiedWorkingPrevious { release: previous };
        let guard = BrokerLedgerGuard::acquire(&fixture.handle).unwrap();
        let admission = guard
            .same_state_admission_v3(&"0".repeat(64), 1000)
            .unwrap();
        fixture.payload.state_snapshot = fixture.pins(&admission).snapshot().clone();
        drop(guard);
        fixture
    }
    fn signed(&mut self, guard: &BrokerLedgerGuard<'_>) -> Vec<u8> {
        let prospective = guard
            .same_state_admission_v3(&"0".repeat(64), 1000)
            .unwrap();
        let mut pins = self.pins(&prospective);
        let observed = pins
            .observed(&self.policy, &self.payload, &prospective)
            .unwrap();
        self.payload.planned_selection_sha256 = observed.planned_selection_sha256;
        assert_eq!(
            digest(pins.planned_selection()),
            self.payload.planned_selection_sha256
        );
        drop(pins);
        crate::install_broker_approval::same_state::tests::envelope(
            &self.payload,
            &SigningKey::from_bytes(&[7; 32]),
        )
    }
    fn grant<'p>(
        &'p self,
        admission: &SameStateLedgerAdmissionV3,
        envelope: Vec<u8>,
    ) -> PolicyVerifiedSameStateSelectionV3<'p> {
        let operator = crate::install_broker_operator::isolated_same_state_operator_fixture(
            &self.policy.readback().unwrap().operator_sid_sha256,
        );
        let frame =
            crate::install_broker_request::isolated_same_state_frame_fixture(envelope, operator);
        self.policy
            .verify_same_state_selection_v3(&frame, self.pins(admission), admission)
            .unwrap()
    }
    fn preserved(&self) {
        for (p, raw) in &self.protected {
            assert_eq!(&std::fs::read(p).unwrap(), raw);
        }
    }
    fn cleanup(self) {
        let path = self.path.clone();
        drop(self);
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn same_state_real_files_signed_reconstruction_selects_stopped_and_preserves_state() {
    let mut fixture = Fixture::new();
    let handle = fixture.handle.try_clone().unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let envelope = fixture.signed(&guard);
    let admission = guard
        .same_state_admission_v3(&digest(&envelope), 1000)
        .unwrap();
    let grant = fixture.grant(&admission, envelope);
    let resources = SameStateInstallResources::acquire(grant).unwrap();
    let original = resources.grant().original_selection_bytes().unwrap();
    let mut ledger = guard
        .begin_same_state_v3(resources.grant(), &admission, &original)
        .unwrap();
    let result = resources.commit_selection(&mut ledger).unwrap();
    assert_eq!(result["status"], "SELECTED_STOPPED");
    assert_eq!(result["workingAcceptance"], false);
    assert_eq!(ledger.phase(), InstallPhaseV1::Complete);
    assert_eq!(
        digest(&std::fs::read(fixture.state.join("current.json")).unwrap()),
        fixture.payload.planned_selection_sha256
    );
    assert!(
        !reviewed_install_transaction_path(&Store::open_read_only(&fixture.state).unwrap())
            .exists()
    );
    assert!(
        fixture
            .state
            .join("versions")
            .join(install_id(&serde_json::from_slice::<Value>(&fixture.before).unwrap()).unwrap())
            .exists()
    );
    fixture.preserved();
    drop(ledger);
    drop(handle);
    fixture.cleanup();
}

#[test]
fn same_state_receipt_misrepresentation_refuses_before_intent() {
    let mut fixture = Fixture::new();
    let handle = fixture.handle.try_clone().unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let envelope = fixture.signed(&guard);
    let admission = guard
        .same_state_admission_v3(&digest(&envelope), 1000)
        .unwrap();
    let mut review = receipt("candidate-review.json", &fixture.payload);
    review["scope"] = json!("FOCUSED_PARTIAL");
    let raw = serde_json::to_vec(&review).unwrap();
    std::fs::write(fixture.intake.join("receipts/candidate-review.json"), &raw).unwrap();
    fixture.payload.candidate.independent_review.receipt_sha256 = digest(&raw);
    let mut pins = fixture.pins(&admission);
    assert!(
        pins.observed(&fixture.policy, &fixture.payload, &admission)
            .is_err()
    );
    drop(pins);
    assert_eq!(
        std::fs::read(fixture.state.join("current.json")).unwrap(),
        fixture.before
    );
    assert_eq!(
        std::fs::read_dir(&fixture.broker).unwrap().count(),
        2,
        "only pre-existing policy and global lock"
    );
    drop(guard);
    drop(handle);
    fixture.cleanup();
}

#[test]
fn same_state_existing_legacy_handoff_is_preserved_before_new_intent() {
    let mut fixture = Fixture::new();
    let handle = fixture.handle.try_clone().unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let envelope = fixture.signed(&guard);
    let admission = guard
        .same_state_admission_v3(&digest(&envelope), 1000)
        .unwrap();
    let grant = fixture.grant(&admission, envelope);
    let store = Store::open_existing_writer(&fixture.state).unwrap();
    let path = reviewed_install_transaction_path(&store);
    atomic(
        &path,
        &ReviewedInstallTransaction {
            schema_version: 1,
            candidate_directory: "historical-candidate".into(),
            prior_desired: "RUNNING".into(),
            operator_stop_requested: true,
            same_state: None,
        },
    )
    .unwrap();
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        SameStateInstallResources::acquire(grant).err().unwrap(),
        "HOST_INSTALL_HANDOFF_CONFLICT"
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert_eq!(std::fs::read_dir(&fixture.broker).unwrap().count(), 2);
    fixture.preserved();
    drop(guard);
    drop(handle);
    fixture.cleanup();
}

#[test]
fn same_state_native_package_pins_bound_size_and_partial_staging_never_overwrite() {
    let fixture = Fixture::new();
    let mut source =
        RuntimePackagePinsV3::isolated(&fixture.intake, RuntimeSourceRoleV3::Candidate).unwrap();
    assert!(
        OpenOptions::new()
            .write(true)
            .open(fixture.intake.join("candidate/adapter.py"))
            .is_err(),
        "real retained source pin denies writes"
    );
    let store = Store::open_existing_writer(&fixture.state).unwrap();
    let stage = ".staging-v3-isolated-candidate";
    materialize(&store, &mut source, stage).unwrap();
    verify_package(&store, stage, &source).unwrap();
    let sentinel = fixture
        .state
        .join("versions")
        .join(".staging-v3-isolated-partial");
    std::fs::create_dir(&sentinel).unwrap();
    std::fs::write(sentinel.join("adapter.py"), b"partial bytes retained").unwrap();
    assert!(materialize(&store, &mut source, ".staging-v3-isolated-partial").is_err());
    assert_eq!(
        std::fs::read(sentinel.join("adapter.py")).unwrap(),
        b"partial bytes retained"
    );
    drop(source);
    let image = OpenOptions::new()
        .write(true)
        .open(fixture.intake.join("candidate/CatDeskWakeHost.exe"))
        .unwrap();
    image.set_len(80 * 1024 * 1024 + 1).unwrap();
    drop(image);
    assert_eq!(
        RuntimePackagePinsV3::isolated(&fixture.intake, RuntimeSourceRoleV3::Candidate)
            .err()
            .unwrap(),
        "WAKE_SAME_STATE_PACKAGE_SIZE_INVALID"
    );
    assert_eq!(
        std::fs::read(fixture.state.join("current.json")).unwrap(),
        fixture.before
    );
    fixture.preserved();
    fixture.cleanup();
}

#[test]
fn same_state_rollback_real_damaged_current_selects_only_intact_previous() {
    let mut fixture = Fixture::rollback();
    let old: Value = serde_json::from_slice(&fixture.before).unwrap();
    let image = fixture
        .state
        .join("versions")
        .join(install_id(&old).unwrap())
        .join("CatDeskWakeHost.exe");
    let damaged = b"damaged old image is retained, never declared healthy";
    std::fs::write(&image, damaged).unwrap();
    let handle = fixture.handle.try_clone().unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let envelope = fixture.signed(&guard);
    let admission = guard
        .same_state_admission_v3(&digest(&envelope), 1000)
        .unwrap();
    let grant = fixture.grant(&admission, envelope);
    let resources = SameStateInstallResources::acquire(grant).unwrap();
    let original = resources.grant().original_selection_bytes().unwrap();
    let mut ledger = guard
        .begin_same_state_v3(resources.grant(), &admission, &original)
        .unwrap();
    let result = resources.commit_selection(&mut ledger).unwrap();
    assert_eq!(result["status"], "ROLLED_BACK_STOPPED");
    assert_eq!(
        result["failedCurrentImages"][0]["actualSha256"],
        digest(damaged)
    );
    assert_eq!(result["workingAcceptance"], false);
    assert_eq!(std::fs::read(&image).unwrap(), damaged);
    let after = install_selection::inspect_install_selection(
        &std::fs::read(fixture.state.join("current.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(after["selection"]["generation"], 8);
    assert!(after["selection"]["previous"].is_null());
    assert_eq!(
        after["current"]["directory"],
        fixture.payload.candidate.manifest.package_id
    );
    assert_eq!(ledger.phase(), InstallPhaseV1::Complete);
    for entry in std::fs::read_dir(&fixture.broker).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(
            !name.ends_with("stage-started.json")
                && !name.ends_with("staged.json")
                && !name.ends_with("published.json")
                && !name.ends_with("publication-attempt.json")
        );
    }
    assert!(
        std::fs::read_dir(fixture.state.join("versions"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".staging-"))
    );
    fixture.preserved();
    drop(ledger);
    drop(handle);
    fixture.cleanup();
}

#[test]
fn same_state_rollback_missing_current_reports_supported_selector_absence_boundary() {
    let mut fixture = Fixture::rollback();
    let old: Value = serde_json::from_slice(&fixture.before).unwrap();
    let image = fixture
        .state
        .join("versions")
        .join(install_id(&old).unwrap())
        .join("CatDeskWakeHost.exe");
    std::fs::remove_file(&image).unwrap();
    let handle = fixture.handle.try_clone().unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let envelope = fixture.signed(&guard);
    let admission = guard
        .same_state_admission_v3(&digest(&envelope), 1000)
        .unwrap();
    let resources =
        SameStateInstallResources::acquire(fixture.grant(&admission, envelope)).unwrap();
    let original = resources.grant().original_selection_bytes().unwrap();
    let mut ledger = guard
        .begin_same_state_v3(resources.grant(), &admission, &original)
        .unwrap();
    let result = resources.commit_selection(&mut ledger).unwrap();
    assert!(result["failedCurrentImages"][0]["actualSha256"].is_null());
    assert_eq!(result["absenceBoundary"], "SUPPORTED_SELECTORS_ONLY");
    assert!(!image.exists());
    fixture.preserved();
    drop(ledger);
    drop(handle);
    fixture.cleanup();
}

#[test]
fn same_state_rollback_altered_previous_or_current_cas_refuses_without_intent() {
    let mut fixture = Fixture::rollback();
    let handle = fixture.handle.try_clone().unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let envelope = fixture.signed(&guard);
    let admission = guard
        .same_state_admission_v3(&digest(&envelope), 1000)
        .unwrap();
    let operator = crate::install_broker_operator::isolated_same_state_operator_fixture(
        &fixture.policy.readback().unwrap().operator_sid_sha256,
    );
    let frame =
        crate::install_broker_request::isolated_same_state_frame_fixture(envelope, operator);
    let pins = fixture.pins(&admission);
    let mut current: Value = serde_json::from_slice(&fixture.before).unwrap();
    current["selection"]["generation"] = json!(99);
    // Retained native pins deny replacement, rather than accepting a CAS from
    // another reader. A stale signed snapshot is tested after releasing them.
    assert!(std::fs::write(fixture.state.join("current.json"), b"changed").is_err());
    drop(pins);
    std::fs::write(
        fixture.state.join("current.json"),
        serde_json::to_vec(&current).unwrap(),
    )
    .unwrap();
    assert!(
        fixture
            .policy
            .verify_same_state_selection_v3(&frame, fixture.pins(&admission), &admission)
            .is_err()
    );
    std::fs::write(fixture.state.join("current.json"), &fixture.before).unwrap();
    let target = fixture
        .state
        .join("versions")
        .join(&fixture.payload.candidate.manifest.package_id)
        .join("adapter.py");
    std::fs::write(&target, b"altered fallback").unwrap();
    assert!(
        SameStateReadPinsV3::observe_with_mode(
            &fixture.policy,
            &admission,
            SameStateIntakeV3::Isolated(fixture.intake.clone()),
            true
        )
        .is_err()
    );
    assert_eq!(std::fs::read_dir(&fixture.broker).unwrap().count(), 2);
    fixture.preserved();
    drop(guard);
    drop(handle);
    fixture.cleanup();
}

#[test]
fn same_state_rollback_failed_image_in_use_refuses_without_killing_or_intent() {
    let mut fixture = Fixture::rollback();
    let old: Value = serde_json::from_slice(&fixture.before).unwrap();
    let image = fixture
        .state
        .join("versions")
        .join(install_id(&old).unwrap())
        .join("CatDeskWakeHost.exe");
    let held = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&image)
        .unwrap();
    let handle = fixture.handle.try_clone().unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let envelope = fixture.signed(&guard);
    let admission = guard
        .same_state_admission_v3(&digest(&envelope), 1000)
        .unwrap();
    assert_eq!(
        SameStateInstallResources::acquire(fixture.grant(&admission, envelope))
            .err()
            .unwrap(),
        "WAKE_OFFLINE_LAUNCH_IMAGE_UNAVAILABLE"
    );
    assert_eq!(
        std::fs::read(fixture.state.join("current.json")).unwrap(),
        fixture.before
    );
    assert_eq!(std::fs::read_dir(&fixture.broker).unwrap().count(), 2);
    drop(held);
    fixture.preserved();
    drop(guard);
    drop(handle);
    fixture.cleanup();
}

#[test]
fn same_state_rollback_real_before_and_after_pointer_interruptions_resume_exactly() {
    for stop in [
        InstallPhaseV1::OldOwnerStopped,
        InstallPhaseV1::PointerSwitched,
    ] {
        let mut fixture = Fixture::rollback();
        let handle = fixture.handle.try_clone().unwrap();
        let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
        let envelope = fixture.signed(&guard);
        let admission = guard
            .same_state_admission_v3(&digest(&envelope), 1000)
            .unwrap();
        let resources =
            SameStateInstallResources::acquire(fixture.grant(&admission, envelope.clone()))
                .unwrap();
        let original = resources.grant().original_selection_bytes().unwrap();
        let mut ledger = guard
            .begin_same_state_v3(resources.grant(), &admission, &original)
            .unwrap();
        assert_eq!(
            resources
                .commit_selection_until(&mut ledger, stop)
                .err()
                .unwrap(),
            "FIXTURE_SAME_STATE_COMMIT_INTERRUPTED"
        );
        assert_eq!(ledger.phase(), InstallPhaseV1::OldOwnerStopped);
        drop(ledger);
        let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
        assert!(
            guard
                .same_state_admission_v3(&"f".repeat(64), 1001)
                .is_err()
        );
        let admission = guard
            .same_state_admission_v3(&digest(&envelope), 1001)
            .unwrap();
        let resources =
            SameStateInstallResources::acquire(fixture.grant(&admission, envelope.clone()))
                .unwrap();
        let original = resources.grant().original_selection_bytes().unwrap();
        let mut ledger = guard
            .begin_same_state_v3(resources.grant(), &admission, &original)
            .unwrap();
        resources.commit_selection(&mut ledger).unwrap();
        let complete = ledger.complete_record_sha256().unwrap().to_owned();
        drop(ledger);
        // Model a lost reply: reconnecting only the exact signed transaction
        // reads its existing COMPLETE and cannot create a second generation.
        let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
        let admission = guard
            .same_state_admission_v3(&digest(&envelope), 1002)
            .unwrap();
        assert_eq!(admission.install_generation, 1);
        let resources =
            SameStateInstallResources::acquire(fixture.grant(&admission, envelope)).unwrap();
        let original = resources.grant().original_selection_bytes().unwrap();
        let mut ledger = guard
            .begin_same_state_v3(resources.grant(), &admission, &original)
            .unwrap();
        resources.commit_selection(&mut ledger).unwrap();
        assert_eq!(ledger.complete_record_sha256().unwrap(), complete);
        fixture.preserved();
        drop(ledger);
        drop(handle);
        fixture.cleanup();
    }
}

#[test]
fn same_state_rollback_resume_refuses_changed_failed_image_observation() {
    let mut fixture = Fixture::rollback();
    let old: Value = serde_json::from_slice(&fixture.before).unwrap();
    let image = fixture
        .state
        .join("versions")
        .join(install_id(&old).unwrap())
        .join("CatDeskWakeHost.exe");
    let handle = fixture.handle.try_clone().unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let envelope = fixture.signed(&guard);
    let admission = guard
        .same_state_admission_v3(&digest(&envelope), 1000)
        .unwrap();
    let resources =
        SameStateInstallResources::acquire(fixture.grant(&admission, envelope.clone())).unwrap();
    let original = resources.grant().original_selection_bytes().unwrap();
    let mut ledger = guard
        .begin_same_state_v3(resources.grant(), &admission, &original)
        .unwrap();
    assert!(
        resources
            .commit_selection_until(&mut ledger, InstallPhaseV1::OldOwnerStopped)
            .is_err()
    );
    drop(ledger);
    std::fs::write(&image, b"changed after interruption; preserve and refuse").unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let admission = guard
        .same_state_admission_v3(&digest(&envelope), 1001)
        .unwrap();
    let resources =
        SameStateInstallResources::acquire(fixture.grant(&admission, envelope)).unwrap();
    let original = resources.grant().original_selection_bytes().unwrap();
    let mut ledger = guard
        .begin_same_state_v3(resources.grant(), &admission, &original)
        .unwrap();
    assert_eq!(
        resources.commit_selection(&mut ledger).err().unwrap(),
        "WAKE_SAME_STATE_FAILED_IMAGE_CHANGED"
    );
    assert_eq!(ledger.phase(), InstallPhaseV1::OldOwnerStopped);
    assert_eq!(
        std::fs::read(&image).unwrap(),
        b"changed after interruption; preserve and refuse"
    );
    assert_eq!(
        std::fs::read(fixture.state.join("current.json")).unwrap(),
        fixture.before
    );
    fixture.preserved();
    drop(ledger);
    drop(handle);
    fixture.cleanup();
}

#[test]
fn same_state_rollback_complete_readback_retains_original_failed_image_diagnostics() {
    let mut fixture = Fixture::rollback();
    let old: Value = serde_json::from_slice(&fixture.before).unwrap();
    let image = fixture
        .state
        .join("versions")
        .join(install_id(&old).unwrap())
        .join("CatDeskWakeHost.exe");
    let handle = fixture.handle.try_clone().unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let envelope = fixture.signed(&guard);
    let admission = guard
        .same_state_admission_v3(&digest(&envelope), 1000)
        .unwrap();
    let resources =
        SameStateInstallResources::acquire(fixture.grant(&admission, envelope.clone())).unwrap();
    let original = resources.grant().original_selection_bytes().unwrap();
    let mut ledger = guard
        .begin_same_state_v3(resources.grant(), &admission, &original)
        .unwrap();
    resources.commit_selection(&mut ledger).unwrap();
    let complete = ledger.complete_record_sha256().unwrap().to_owned();
    let observations = ledger.failed_current_images().unwrap().unwrap();
    assert_eq!(observations.len(), 2);
    drop(ledger);
    std::fs::write(
        &image,
        b"later independent mutation is not historical preservation",
    )
    .unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let admission = guard
        .same_state_admission_v3(&digest(&envelope), 1001)
        .unwrap();
    let resources =
        SameStateInstallResources::acquire(fixture.grant(&admission, envelope)).unwrap();
    let original = resources.grant().original_selection_bytes().unwrap();
    let mut ledger = guard
        .begin_same_state_v3(resources.grant(), &admission, &original)
        .unwrap();
    assert_eq!(
        resources.commit_selection(&mut ledger).err().unwrap(),
        "WAKE_SAME_STATE_FAILED_IMAGE_CHANGED"
    );
    assert_eq!(ledger.complete_record_sha256().unwrap(), complete);
    assert_eq!(
        ledger.failed_current_images().unwrap().unwrap(),
        observations
    );
    assert_eq!(
        std::fs::read(&image).unwrap(),
        b"later independent mutation is not historical preservation"
    );
    fixture.preserved();
    drop(ledger);
    drop(handle);
    fixture.cleanup();
}
