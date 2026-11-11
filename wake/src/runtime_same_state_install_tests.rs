//! Disposable actual-file/lease fixtures only; inert images are never run.
use super::*;
use crate::install_broker_approval::{ArtifactV1, PackageManifestV1, same_state::*};
use crate::install_broker_ledger::BrokerLedgerGuard;
use ed25519_dalek::SigningKey;
use std::os::windows::{
    fs::OpenOptionsExt,
    io::{AsRawHandle, OwnedHandle},
};
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
        Self::with_local_updates(false)
    }
    fn with_local_updates(local_updates: bool) -> Self {
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
                (true, local_updates),
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
        Self::qualified_selection(false, true)
    }
    fn qualified_selection(local_updates: bool, rollback: bool) -> Self {
        let mut fixture = Self::with_local_updates(local_updates);
        let previous = fixture.payload.rollback_authority.release().clone();
        let published_previous = fixture
            .state
            .join("versions")
            .join(&previous.manifest.package_id);
        if rollback {
            std::fs::rename(fixture.intake.join("previous"), &published_previous).unwrap();
        } else {
            // SELECT still consumes the fixed previous input while validating
            // the same already-published predecessor in the current document.
            std::fs::create_dir(&published_previous).unwrap();
            for name in [
                "CatDeskWakeHost.exe",
                "CatDeskBinagotchy.exe",
                "adapter.py",
                "wake_bridge.py",
                "manifest.json",
            ] {
                std::fs::copy(
                    fixture.intake.join("previous").join(name),
                    published_previous.join(name),
                )
                .unwrap();
            }
        }
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
        if rollback {
            fixture.payload.operation = "ROLLBACK_SAME_STATE_RELEASE".into();
            fixture.payload.approval_id = "rollback-fixture-one".into();
            fixture.payload.candidate = previous.clone();
        }
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
        self.bind_planned_selection(guard);
        crate::install_broker_approval::same_state::tests::envelope(
            &self.payload,
            &SigningKey::from_bytes(&[7; 32]),
        )
    }
    fn local_request(&mut self, guard: &BrokerLedgerGuard<'_>) -> Vec<u8> {
        self.bind_planned_selection(guard);
        encode_local_user_selection_request_v1(&self.payload).unwrap()
    }
    fn bind_planned_selection(&mut self, guard: &BrokerLedgerGuard<'_>) {
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
fn same_state_local_user_unenrolled_policy_refuses_before_intent() {
    let mut fixture = Fixture::qualified_selection(false, false);
    let handle = fixture.handle.try_clone().unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let request = fixture.local_request(&guard);
    let admission = guard
        .same_state_admission_v3(&digest(&request), 1000)
        .unwrap();
    let operator = crate::install_broker_operator::isolated_same_state_operator_fixture(
        &fixture.policy.readback().unwrap().operator_sid_sha256,
    );
    let frame = crate::install_broker_request::isolated_same_state_frame_fixture(request, operator);
    assert_eq!(
        fixture
            .policy
            .verify_same_state_selection_v3(&frame, fixture.pins(&admission), &admission)
            .err()
            .unwrap(),
        "WAKE_LOCAL_UPDATE_POLICY_REQUIRED"
    );
    assert_eq!(
        std::fs::read(fixture.state.join("current.json")).unwrap(),
        fixture.before
    );
    assert_eq!(std::fs::read_dir(&fixture.broker).unwrap().count(), 2);
    fixture.preserved();
    drop(guard);
    drop(handle);
    fixture.cleanup();
}

#[test]
fn same_state_local_user_enrolled_select_and_rollback_preserve_state() {
    for rollback in [false, true] {
        let mut fixture = Fixture::qualified_selection(true, rollback);
        let handle = fixture.handle.try_clone().unwrap();
        let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
        let request = fixture.local_request(&guard);
        let wire: Value = serde_json::from_slice(&request).unwrap();
        assert_eq!(wire["authorization"], "LOCAL_USER_POLICY");
        assert!(wire.get("signatureHex").is_none());
        let admission = guard
            .same_state_admission_v3(&digest(&request), 1000)
            .unwrap();
        let resources =
            SameStateInstallResources::acquire(fixture.grant(&admission, request)).unwrap();
        let original = resources.grant().original_selection_bytes().unwrap();
        let mut ledger = guard
            .begin_same_state_v3(resources.grant(), &admission, &original)
            .unwrap();
        assert!(ledger.binding().local_user_authorized());
        assert_eq!(
            ledger.binding().verification_key_sha256,
            ledger.binding().policy_sha256
        );
        let result = resources.commit_selection(&mut ledger).unwrap();
        assert_eq!(
            result["status"],
            if rollback {
                "ROLLED_BACK_STOPPED"
            } else {
                "SELECTED_STOPPED"
            }
        );
        assert_eq!(result["workingAcceptance"], false);
        assert_eq!(ledger.phase(), InstallPhaseV1::Complete);
        assert_eq!(
            digest(&std::fs::read(fixture.state.join("current.json")).unwrap()),
            fixture.payload.planned_selection_sha256
        );
        fixture.preserved();
        drop(ledger);
        drop(handle);
        fixture.cleanup();
    }
}

#[test]
fn same_state_local_user_pointer_interruptions_resume_only_the_original_request() {
    for rollback in [false, true] {
        for stop in [
            InstallPhaseV1::OldOwnerStopped,
            InstallPhaseV1::PointerSwitched,
        ] {
            let mut fixture = Fixture::qualified_selection(true, rollback);
            let handle = fixture.handle.try_clone().unwrap();
            let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
            let request = fixture.local_request(&guard);
            let admission = guard
                .same_state_admission_v3(&digest(&request), 1000)
                .unwrap();
            let resources =
                SameStateInstallResources::acquire(fixture.grant(&admission, request.clone()))
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
            let interrupted = std::fs::read(fixture.state.join("current.json")).unwrap();
            if stop == InstallPhaseV1::OldOwnerStopped {
                assert_eq!(interrupted, fixture.before);
            } else {
                assert_eq!(
                    digest(&interrupted),
                    fixture.payload.planned_selection_sha256
                );
            }
            drop(ledger);

            let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
            assert!(
                guard
                    .same_state_admission_v3(&"f".repeat(64), 1001)
                    .is_err()
            );
            let admission = guard
                .same_state_admission_v3(&digest(&request), 1001)
                .unwrap();
            assert_eq!(admission.accepted_at_utc_seconds, 1000);
            assert_eq!(
                admission.selection_attempt_present,
                stop == InstallPhaseV1::PointerSwitched
            );
            assert!(
                admission
                    .resume_binding
                    .as_ref()
                    .unwrap()
                    .local_user_authorized()
            );
            let resources =
                SameStateInstallResources::acquire(fixture.grant(&admission, request.clone()))
                    .unwrap();
            let original = resources.grant().original_selection_bytes().unwrap();
            let mut ledger = guard
                .begin_same_state_v3(resources.grant(), &admission, &original)
                .unwrap();
            resources.commit_selection(&mut ledger).unwrap();
            let complete = ledger.complete_record_sha256().unwrap().to_owned();
            let selected = std::fs::read(fixture.state.join("current.json")).unwrap();
            drop(ledger);

            // A lost reply can read only the same COMPLETE; it cannot produce
            // a second generation or repeat package publication/selection.
            let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
            let admission = guard
                .same_state_admission_v3(&digest(&request), 1002)
                .unwrap();
            assert_eq!(admission.install_generation, 1);
            let resources =
                SameStateInstallResources::acquire(fixture.grant(&admission, request)).unwrap();
            let original = resources.grant().original_selection_bytes().unwrap();
            let mut ledger = guard
                .begin_same_state_v3(resources.grant(), &admission, &original)
                .unwrap();
            resources.commit_selection(&mut ledger).unwrap();
            assert_eq!(ledger.complete_record_sha256().unwrap(), complete);
            assert_eq!(
                std::fs::read(fixture.state.join("current.json")).unwrap(),
                selected
            );
            fixture.preserved();
            drop(ledger);
            drop(handle);
            fixture.cleanup();
        }
    }
}

#[test]
fn same_state_local_user_unknown_pointer_is_preserved_after_interruption() {
    let mut fixture = Fixture::qualified_selection(true, true);
    let handle = fixture.handle.try_clone().unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let request = fixture.local_request(&guard);
    let admission = guard
        .same_state_admission_v3(&digest(&request), 1000)
        .unwrap();
    let resources =
        SameStateInstallResources::acquire(fixture.grant(&admission, request.clone())).unwrap();
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
    let unknown = b"unrelated current bytes must be preserved";
    std::fs::write(fixture.state.join("current.json"), unknown).unwrap();
    let count = std::fs::read_dir(&fixture.broker).unwrap().count();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let admission = guard
        .same_state_admission_v3(&digest(&request), 1001)
        .unwrap();
    assert_eq!(
        SameStateReadPinsV3::observe_with_mode(
            &fixture.policy,
            &admission,
            SameStateIntakeV3::Isolated(fixture.intake.clone()),
            true
        )
        .err()
        .unwrap(),
        "WAKE_SAME_STATE_SELECTION_UNKNOWN"
    );
    assert_eq!(
        std::fs::read(fixture.state.join("current.json")).unwrap(),
        unknown
    );
    assert_eq!(std::fs::read_dir(&fixture.broker).unwrap().count(), count);
    fixture.preserved();
    drop(guard);
    drop(handle);
    fixture.cleanup();
}

#[test]
fn same_state_local_user_reconstruction_refuses_before_intent() {
    let mut fixture = Fixture::with_local_updates(true);
    let handle = fixture.handle.try_clone().unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    fixture.bind_planned_selection(&guard);
    assert_eq!(
        encode_local_user_selection_request_v1(&fixture.payload)
            .err()
            .unwrap(),
        "WAKE_LOCAL_USER_SELECTION_RECONSTRUCTION_REFUSED"
    );
    let request = format!(
        "{{\"authorization\":\"LOCAL_USER_POLICY\",\"payload\":{}}}",
        serde_json::to_string(&fixture.payload).unwrap()
    )
    .into_bytes();
    let admission = guard
        .same_state_admission_v3(&digest(&request), 1000)
        .unwrap();
    let operator = crate::install_broker_operator::isolated_same_state_operator_fixture(
        &fixture.policy.readback().unwrap().operator_sid_sha256,
    );
    let frame = crate::install_broker_request::isolated_same_state_frame_fixture(request, operator);
    assert_eq!(
        fixture
            .policy
            .verify_same_state_selection_v3(&frame, fixture.pins(&admission), &admission)
            .err()
            .unwrap(),
        "WAKE_LOCAL_USER_SELECTION_RECONSTRUCTION_REFUSED"
    );
    assert_eq!(
        std::fs::read(fixture.state.join("current.json")).unwrap(),
        fixture.before
    );
    assert_eq!(std::fs::read_dir(&fixture.broker).unwrap().count(), 2);
    fixture.preserved();
    drop(guard);
    drop(handle);
    fixture.cleanup();
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
    materialize(&store, &mut source, stage, &fixture.policy).unwrap();
    verify_package(&store, stage, &source).unwrap();
    let sentinel = fixture
        .state
        .join("versions")
        .join(".staging-v3-isolated-partial");
    std::fs::create_dir(&sentinel).unwrap();
    std::fs::write(sentinel.join("adapter.py"), b"partial bytes retained").unwrap();
    assert!(
        materialize(
            &store,
            &mut source,
            ".staging-v3-isolated-partial",
            &fixture.policy
        )
        .is_err()
    );
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

fn inert_junction(handle: &File, target: Option<&Path>) -> std::io::Result<()> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn DeviceIoControl(
            handle: *mut std::ffi::c_void,
            code: u32,
            input: *const std::ffi::c_void,
            input_size: u32,
            output: *mut std::ffi::c_void,
            output_size: u32,
            returned: *mut u32,
            overlapped: *mut std::ffi::c_void,
        ) -> i32;
    }
    let mut raw = 0xa0000003u32.to_le_bytes().to_vec();
    let code = if let Some(target) = target {
        let print = target.canonicalize()?.to_string_lossy().into_owned();
        let substitute = format!(r"\??\{}", print.strip_prefix(r"\\?\").unwrap_or(&print));
        let substitute = substitute.encode_utf16().collect::<Vec<_>>();
        let print = print.encode_utf16().collect::<Vec<_>>();
        let data_length = 8 + 2 * (substitute.len() + print.len() + 2);
        if data_length + 8 > 16_384 {
            return Err(std::io::Error::other("inert junction target exceeds bound"));
        }
        raw.extend_from_slice(&(data_length as u16).to_le_bytes());
        raw.extend_from_slice(&0u16.to_le_bytes());
        raw.extend_from_slice(&0u16.to_le_bytes());
        raw.extend_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
        raw.extend_from_slice(&(((substitute.len() + 1) * 2) as u16).to_le_bytes());
        raw.extend_from_slice(&((print.len() * 2) as u16).to_le_bytes());
        for unit in substitute.into_iter().chain([0]).chain(print).chain([0]) {
            raw.extend_from_slice(&unit.to_le_bytes());
        }
        0x000900a4
    } else {
        raw.extend_from_slice(&[0; 4]);
        0x000900ac
    };
    let mut returned = 0;
    let result = unsafe {
        DeviceIoControl(
            handle.as_raw_handle(),
            code,
            raw.as_ptr().cast(),
            raw.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn inert_directory_attributes(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .access_mode(0x00120100)
        .share_mode(7)
        .custom_flags(0x02200000)
        .open(path)
}

#[test]
fn same_state_materialize_retains_stage_against_rename_and_metadata_write() {
    let fixture = Fixture::new();
    let outside = fixture.path.join("outside-inert");
    let control = fixture.path.join("junction-control");
    std::fs::create_dir(&outside).unwrap();
    std::fs::create_dir(&control).unwrap();
    drop(
        OpenOptions::new()
            .write(true)
            .share_mode(7)
            .custom_flags(0x02200000)
            .open(&control)
            .unwrap(),
    );
    // Real unprivileged mount-point positive control, not an assumed attack.
    let control_handle = inert_directory_attributes(&control).unwrap();
    inert_junction(&control_handle, Some(&outside)).unwrap();
    let control_is_exact = control.canonicalize().unwrap() == outside.canonicalize().unwrap();
    inert_junction(&control_handle, None).unwrap();
    drop(control_handle);

    let store = Store::open_existing_writer(&fixture.state).unwrap();
    let stage_name = ".staging-v3-inert-namespace";
    let stage_path = fixture.state.join("versions").join(stage_name);
    let stage = RuntimePackageStageV3::create(
        store.root(),
        stage_name,
        crate::install_broker_stage_writer::StageSecurityMode::from_same_state_policy(
            &fixture.policy,
        )
        .unwrap(),
    )
    .unwrap();
    let rename_denied = std::fs::rename(&stage_path, fixture.path.join("retired-stage")).is_err();
    let stage_write_denied = OpenOptions::new()
        .write(true)
        .share_mode(7)
        .custom_flags(0x02200000)
        .open(&stage_path)
        .is_err();
    let versions_write_denied = OpenOptions::new()
        .write(true)
        .share_mode(7)
        .custom_flags(0x02200000)
        .open(fixture.state.join("versions"))
        .is_err();
    let root_write_denied = OpenOptions::new()
        .write(true)
        .share_mode(7)
        .custom_flags(0x02200000)
        .open(&fixture.state)
        .is_err();
    let mut metadata_route_succeeded = false;
    let mut relative_create = None;
    if let Ok(handle) = inert_directory_attributes(&stage_path)
        && inert_junction(&handle, Some(&outside)).is_ok()
    {
        metadata_route_succeeded = true;
        // Simulate metadata mutation after the last user-mode check.
        relative_create = Some(stage.fixture_relative_create_after_metadata_change());
        inert_junction(&handle, None).unwrap();
    }
    let outside_unchanged = std::fs::read_dir(&outside).unwrap().next().is_none();
    let successful_create_stayed_in_stage = relative_create.as_ref().is_none_or(|result| {
        result.is_err()
            || std::fs::read(stage_path.join("adapter.py"))
                .is_ok_and(|bytes| bytes == b"inert relative-create sentinel")
    });
    let retained_identity = stage.revalidate();
    drop(stage);
    let mut source =
        RuntimePackagePinsV3::isolated(&fixture.intake, RuntimeSourceRoleV3::Candidate).unwrap();
    let copied = materialize(
        &store,
        &mut source,
        ".staging-v3-inert-positive",
        &fixture.policy,
    );
    let verified = verify_package(&store, ".staging-v3-inert-positive", &source);
    drop(source);
    fixture.preserved();
    fixture.cleanup();
    assert!(control_is_exact);
    assert!(rename_denied && stage_write_denied && versions_write_denied && root_write_denied);
    assert!(
        outside_unchanged,
        "relative creation must not write into the inert junction target"
    );
    assert!(successful_create_stayed_in_stage);
    assert!(retained_identity.is_ok());
    assert!(copied.is_ok() && verified.is_ok());
    if metadata_route_succeeded {
        assert!(
            relative_create.is_some(),
            "actual metadata mutation must exercise the leaf primitive"
        );
    }
}

#[test]
fn same_state_materialize_refuses_reparse_versions_before_any_write() {
    let fixture = Fixture::new();
    let versions = fixture.state.join("versions");
    let retained = fixture.state.join("versions-retained");
    let outside = fixture.path.join("outside-inert");
    std::fs::create_dir(&outside).unwrap();
    std::fs::rename(&versions, &retained).unwrap();
    std::fs::create_dir(&versions).unwrap();
    let handle = inert_directory_attributes(&versions).unwrap();
    inert_junction(&handle, Some(&outside)).unwrap();
    let control_is_exact = versions.canonicalize().unwrap() == outside.canonicalize().unwrap();
    let store = Store::open_existing_writer(&fixture.state).unwrap();
    let mut source =
        RuntimePackagePinsV3::isolated(&fixture.intake, RuntimeSourceRoleV3::Candidate).unwrap();
    let refused = materialize(
        &store,
        &mut source,
        ".staging-v3-inert-substituted",
        &fixture.policy,
    );
    let outside_unchanged = std::fs::read_dir(&outside).unwrap().next().is_none();
    inert_junction(&handle, None).unwrap();
    drop(handle);
    std::fs::remove_dir(&versions).unwrap();
    std::fs::rename(&retained, &versions).unwrap();
    drop(source);
    fixture.preserved();
    fixture.cleanup();
    assert!(control_is_exact && refused.is_err());
    assert!(
        outside_unchanged,
        "substituted versions must refuse before any outside write"
    );
}

fn with_staged_publication(
    fixture: &mut Fixture,
    check: impl FnOnce(&Fixture, &mut AdmittedSameStateLedgerLeaseV3<'_, '_>, &str),
) {
    let handle = fixture.handle.try_clone().unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let envelope = fixture.signed(&guard);
    let envelope_sha = digest(&envelope);
    let admission = guard.same_state_admission_v3(&envelope_sha, 1000).unwrap();
    let resources =
        SameStateInstallResources::acquire(fixture.grant(&admission, envelope)).unwrap();
    let original = resources.grant().original_selection_bytes().unwrap();
    let mut ledger = guard
        .begin_same_state_v3(resources.grant(), &admission, &original)
        .unwrap();
    ledger
        .record_phase(InstallPhaseV1::StageStarted, None)
        .unwrap();
    let store = Store::open_existing_writer(&fixture.state).unwrap();
    {
        let mut pins = resources.grant().pins_mut().unwrap();
        materialize(
            &store,
            &mut pins.candidate,
            &stage_name(&envelope_sha, "candidate"),
            &fixture.policy,
        )
        .unwrap();
        materialize(
            &store,
            &mut pins.previous,
            &stage_name(&envelope_sha, "previous"),
            &fixture.policy,
        )
        .unwrap();
    }
    ledger.record_phase(InstallPhaseV1::Staged, None).unwrap();
    check(fixture, &mut ledger, &envelope_sha);
    RuntimePackagePublicationV3::fixture_clear_hooks();
    drop(ledger);
    drop(resources);
    drop(handle);
}

#[test]
fn same_state_publication_retains_exact_bytes_through_relative_rename() {
    let mut fixture = Fixture::new();
    with_staged_publication(&mut fixture, |fixture, ledger, envelope_sha| {
        let publication = RuntimePackagePublicationV3::prepare(
            &fixture.state,
            &fixture.payload,
            envelope_sha,
            ledger,
            &fixture.policy,
        )
        .unwrap();
        let stage = fixture
            .state
            .join("versions")
            .join(stage_name(envelope_sha, "candidate"));
        assert!(
            OpenOptions::new()
                .write(true)
                .open(stage.join("adapter.py"))
                .is_err()
        );
        ledger.record_publication_attempt().unwrap();
        let pins = publication.publish(ledger).unwrap();
        assert_eq!(pins.len(), 2);
        assert!(
            pins.iter()
                .any(|p| p.manifest() == &fixture.payload.candidate.manifest)
        );
        assert!(
            pins.iter()
                .any(|p| p.manifest() == &fixture.payload.rollback_authority.release().manifest)
        );
        ledger
            .record_phase(InstallPhaseV1::Published, None)
            .unwrap();
        for release in [
            &fixture.payload.candidate,
            fixture.payload.rollback_authority.release(),
        ] {
            let package = fixture
                .state
                .join("versions")
                .join(&release.manifest.package_id);
            assert!(
                OpenOptions::new()
                    .access_mode(0x00010000)
                    .share_mode(7)
                    .custom_flags(0x02200000)
                    .open(&package)
                    .is_err(),
                "retained source capability must deny another DELETE handle, including parent DELETE_CHILD admission"
            );
            assert!(
                std::fs::rename(&package, fixture.path.join("cannot-retire-published")).is_err()
            );
            assert!(std::fs::remove_dir(&package).is_err());
            for artifact in &release.manifest.artifacts {
                assert_eq!(
                    digest(&std::fs::read(package.join(&artifact.name)).unwrap()),
                    artifact.sha256
                );
                assert!(
                    OpenOptions::new()
                        .write(true)
                        .open(package.join(&artifact.name))
                        .is_err()
                );
            }
        }
        assert_eq!(
            std::fs::read(fixture.state.join("current.json")).unwrap(),
            fixture.before
        );
        drop(pins);
        // PUBLISHED releases only this bounded transaction's capabilities.
        // The user-owned fixture permits renaming after their explicit drop;
        // it does not claim production owner/DACL or future package authority.
        let package = fixture
            .state
            .join("versions")
            .join(&fixture.payload.candidate.manifest.package_id);
        let retired = fixture
            .state
            .join("versions")
            .join("retired-after-published");
        std::fs::rename(&package, &retired).unwrap();
        std::fs::rename(&retired, &package).unwrap();
    });
    fixture.preserved();
    fixture.cleanup();
}

#[test]
fn same_state_publication_proven_destination_releases_children_at_protected_handoff() {
    let mut fixture = Fixture::new();
    let succeeded = std::sync::Arc::new(std::sync::Mutex::new(false));
    let observed = succeeded.clone();
    let children_closed = std::sync::Arc::new(std::sync::Mutex::new(false));
    let closure_observed = children_closed.clone();
    with_staged_publication(&mut fixture, |fixture, ledger, envelope_sha| {
        let mut publication = RuntimePackagePublicationV3::prepare(
            &fixture.state,
            &fixture.payload,
            envelope_sha,
            ledger,
            &fixture.policy,
        )
        .unwrap();
        publication.fixture_use_proven_destination().unwrap();
        let stage = fixture
            .state
            .join("versions")
            .join(stage_name(envelope_sha, "previous"));
        for name in [
            "manifest.json",
            "CatDeskWakeHost.exe",
            "CatDeskBinagotchy.exe",
            "adapter.py",
            "wake_bridge.py",
        ] {
            assert!(
                OpenOptions::new()
                    .write(true)
                    .open(stage.join(name))
                    .is_err(),
                "all five byte pins must remain retained"
            );
        }
        RuntimePackagePublicationV3::fixture_hook(
            false,
            Box::new(move || {
                // These are disposable user-owned fixture objects. Exclusive READ
                // opens establish the real descendant-handle release, not a claim
                // that this process has production SYSTEM ownership/ACL authority.
                let closed = [
                    "manifest.json",
                    "CatDeskWakeHost.exe",
                    "CatDeskBinagotchy.exe",
                    "adapter.py",
                    "wake_bridge.py",
                ]
                .into_iter()
                .all(|name| {
                    OpenOptions::new()
                        .read(true)
                        .share_mode(0)
                        .open(stage.join(name))
                        .is_ok()
                });
                *closure_observed.lock().unwrap() = closed;
            }),
        );
        ledger.record_publication_attempt().unwrap();
        let result = publication.publish(ledger);
        eprintln!(
            "V3_PUBLICATION_PROTECTED_HANDOFF access=0x1200a0 childReadPinsBeforeHandoff=5 childReadPinsAtRename=0 result={:?}",
            result.as_ref().map(|pins| pins.len())
        );
        *observed.lock().unwrap() = result.is_ok();
        assert_eq!(
            std::fs::read(fixture.state.join("current.json")).unwrap(),
            fixture.before
        );
        drop(result);
    });
    let succeeded = *succeeded.lock().unwrap();
    let children_closed = *children_closed.lock().unwrap();
    fixture.preserved();
    fixture.cleanup();
    assert!(
        succeeded && children_closed,
        "fresh V3 protected-handoff fixture refused; see exact native NTSTATUS"
    );
}

#[test]
fn same_state_publication_protected_mode_refuses_user_owned_stage() {
    use crate::install_broker_stage_writer::StageSecurityMode;
    let fixture = Fixture::new();
    assert!(fixture.policy.is_fixture_root());
    assert!(matches!(
        StageSecurityMode::from_same_state_policy(&fixture.policy).unwrap(),
        StageSecurityMode::Fixture
    ));
    let path = fixture
        .state
        .join("versions")
        .join(".staging-v3-unprotected-control");
    std::fs::create_dir(&path).unwrap();
    let handle = OpenOptions::new()
        .access_mode(0x00120081)
        .share_mode(7)
        .custom_flags(0x02200000)
        .open(&path)
        .unwrap();
    let service = fixture.policy.checked_endpoint_sids().unwrap().0.to_owned();
    let protected = StageSecurityMode::Protected(service);
    // This calls the actual production owner/DACL validator on a real owned
    // directory. The fixture never asks the OS to assign SYSTEM ownership.
    let refused = protected.readback(&handle, true);
    assert!(protected.creation_descriptor(true).unwrap().is_some());
    assert!(protected.creation_descriptor(false).unwrap().is_some());
    drop(handle);
    fixture.preserved();
    fixture.cleanup();
    assert_eq!(refused.unwrap_err(), "WAKE_BROKER_PUBLICATION_ACL_MISMATCH");
}

#[test]
fn same_state_publication_substitution_after_attempt_cannot_write_outside() {
    let mut fixture = Fixture::new();
    with_staged_publication(&mut fixture, |fixture, ledger, envelope_sha| {
        let publication = RuntimePackagePublicationV3::prepare(
            &fixture.state,
            &fixture.payload,
            envelope_sha,
            ledger,
            &fixture.policy,
        )
        .unwrap();
        let stage = fixture
            .state
            .join("versions")
            .join(stage_name(envelope_sha, "previous"));
        let retired = fixture.path.join("retired-publication-stage");
        let outside = fixture.path.join("outside-publication");
        std::fs::create_dir(&outside).unwrap();
        let substituted = std::sync::Arc::new(std::sync::Mutex::new(false));
        let flag = substituted.clone();
        let moved = retired.clone();
        let path = stage.clone();
        let target = outside.clone();
        RuntimePackagePublicationV3::fixture_hook(
            true,
            Box::new(move || {
                if std::fs::rename(&path, &moved).is_ok() {
                    std::fs::create_dir(&path).unwrap();
                    let handle = inert_directory_attributes(&path).unwrap();
                    inert_junction(&handle, Some(&target)).unwrap();
                    *flag.lock().unwrap() = true;
                }
            }),
        );
        ledger.record_publication_attempt().unwrap();
        let result = publication.publish(ledger);
        let replaced = *substituted.lock().unwrap();
        if replaced {
            let handle = inert_directory_attributes(&stage).unwrap();
            inert_junction(&handle, None).unwrap();
            drop(handle);
            std::fs::remove_dir(&stage).unwrap();
            std::fs::rename(&retired, &stage).unwrap();
        }
        let outside_unchanged = std::fs::read_dir(&outside).unwrap().next().is_none();
        assert!(!replaced || result.is_err());
        assert!(outside_unchanged);
        assert!(ledger.publication_attempt_present().unwrap());
        assert!(
            RuntimePackagePublicationV3::prepare(
                &fixture.state,
                &fixture.payload,
                envelope_sha,
                ledger,
                &fixture.policy,
            )
            .is_err()
        );
        drop(result);
    });
    fixture.preserved();
    fixture.cleanup();
}

#[test]
fn same_state_publication_metadata_mutation_cannot_redirect_native_effect() {
    let mut fixture = Fixture::new();
    with_staged_publication(&mut fixture, |fixture, ledger, envelope_sha| {
        let outside = fixture.path.join("outside-publication");
        let control = fixture.path.join("publication-junction-control");
        std::fs::create_dir(&outside).unwrap();
        std::fs::create_dir(&control).unwrap();
        std::fs::write(outside.join("sentinel"), b"outside namespace remains exact").unwrap();
        let handle = inert_directory_attributes(&control).unwrap();
        inert_junction(&handle, Some(&outside)).unwrap();
        assert_eq!(
            control.canonicalize().unwrap(),
            outside.canonicalize().unwrap()
        );
        inert_junction(&handle, None).unwrap();
        drop(handle);
        let publication = RuntimePackagePublicationV3::prepare(
            &fixture.state,
            &fixture.payload,
            envelope_sha,
            ledger,
            &fixture.policy,
        )
        .unwrap();
        let changed = std::sync::Arc::new(std::sync::Mutex::new(None::<File>));
        let saved = changed.clone();
        let versions = fixture.state.join("versions");
        let target = outside.clone();
        RuntimePackagePublicationV3::fixture_hook(
            false,
            Box::new(move || {
                if let Ok(handle) = inert_directory_attributes(&versions)
                    && inert_junction(&handle, Some(&target)).is_ok()
                {
                    *saved.lock().unwrap() = Some(handle);
                }
            }),
        );
        ledger.record_publication_attempt().unwrap();
        let result = publication.publish(ledger);
        if let Some(handle) = changed.lock().unwrap().take() {
            inert_junction(&handle, None).unwrap();
        }
        let names = std::fs::read_dir(&outside)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, vec![std::ffi::OsString::from("sentinel")]);
        assert_eq!(
            std::fs::read(outside.join("sentinel")).unwrap(),
            b"outside namespace remains exact"
        );
        assert!(ledger.publication_attempt_present().unwrap());
        drop(result);
    });
    fixture.preserved();
    fixture.cleanup();
}

#[test]
fn same_state_publication_collision_and_interruption_are_no_replay() {
    let mut fixture = Fixture::new();
    with_staged_publication(&mut fixture, |fixture, ledger, envelope_sha| {
        let publication = RuntimePackagePublicationV3::prepare(
            &fixture.state,
            &fixture.payload,
            envelope_sha,
            ledger,
            &fixture.policy,
        )
        .unwrap();
        let collision = fixture.state.join("versions").join(
            fixture
                .payload
                .candidate
                .manifest
                .package_id
                .to_ascii_uppercase(),
        );
        let path = collision.clone();
        RuntimePackagePublicationV3::fixture_hook(
            false,
            Box::new(move || {
                // Let the first approved package move, then collide with the
                // second: a partial publication must remain attention, not replay.
                RuntimePackagePublicationV3::fixture_hook(
                    false,
                    Box::new(move || {
                        std::fs::create_dir(&path).unwrap();
                        std::fs::write(path.join("sentinel"), b"must not replace").unwrap();
                    }),
                );
            }),
        );
        ledger.record_publication_attempt().unwrap();
        assert!(publication.publish(ledger).is_err());
        assert_eq!(
            std::fs::read(collision.join("sentinel")).unwrap(),
            b"must not replace"
        );
        assert!(
            fixture
                .state
                .join("versions")
                .join(
                    &fixture
                        .payload
                        .rollback_authority
                        .release()
                        .manifest
                        .package_id
                )
                .is_dir()
        );
        let store = Store::open_read_only(&fixture.state).unwrap();
        let source =
            RuntimePackagePinsV3::isolated(&fixture.intake, RuntimeSourceRoleV3::Candidate)
                .unwrap();
        assert!(
            verify_package(
                &store,
                &fixture.payload.candidate.manifest.package_id,
                &source
            )
            .is_err()
        );
        assert!(
            RuntimePackagePublicationV3::prepare(
                &fixture.state,
                &fixture.payload,
                envelope_sha,
                ledger,
                &fixture.policy,
            )
            .is_err()
        );
    });
    fixture.preserved();
    fixture.cleanup();

    let mut fixture = Fixture::new();
    let handle = fixture.handle.try_clone().unwrap();
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let envelope = fixture.signed(&guard);
    let envelope_sha = digest(&envelope);
    let admission = guard.same_state_admission_v3(&envelope_sha, 1000).unwrap();
    let resources =
        SameStateInstallResources::acquire(fixture.grant(&admission, envelope.clone())).unwrap();
    let original = resources.grant().original_selection_bytes().unwrap();
    let mut ledger = guard
        .begin_same_state_v3(resources.grant(), &admission, &original)
        .unwrap();
    assert_eq!(
        resources
            .commit_selection_until_publication(&mut ledger)
            .unwrap_err(),
        "FIXTURE_SAME_STATE_COMMIT_INTERRUPTED"
    );
    assert_eq!(ledger.phase(), InstallPhaseV1::Staged);
    assert!(ledger.publication_attempt_present().unwrap());
    drop(ledger);
    let guard = BrokerLedgerGuard::acquire(&handle).unwrap();
    let admission = guard.same_state_admission_v3(&envelope_sha, 1001).unwrap();
    let resources =
        SameStateInstallResources::acquire(fixture.grant(&admission, envelope)).unwrap();
    let original = resources.grant().original_selection_bytes().unwrap();
    let mut ledger = guard
        .begin_same_state_v3(resources.grant(), &admission, &original)
        .unwrap();
    RuntimePackagePublicationV3::fixture_hook(
        false,
        Box::new(|| panic!("retained attempt must never rename again")),
    );
    let result = resources.commit_selection(&mut ledger).unwrap();
    RuntimePackagePublicationV3::fixture_clear_hooks();
    assert_eq!(result["status"], "SELECTED_STOPPED");
    assert_eq!(ledger.phase(), InstallPhaseV1::Complete);
    drop(ledger);
    drop(handle);
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
