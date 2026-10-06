use catdesk_wake::{
    protocol::{Event, validate_target},
    store::{Phase, Receipt, Store, now},
};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Barrier},
    thread,
};
static ID: AtomicU64 = AtomicU64::new(0);
const CHAT_A: &str = "https://chatgpt.com/c/chat-a";
const CHAT_B: &str = "https://chatgpt.com/c/chat-b";

#[test]
fn manual_publication_rejects_wrong_generation_or_digest_without_queue_mutation() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let target = store.config().unwrap().targets["catdesk"].clone();
    for wrong in [
        catdesk_wake::protocol::Target {
            generation: target.generation + 1,
            ..target.clone()
        },
        catdesk_wake::protocol::Target {
            digest: "f".repeat(64),
            ..target.clone()
        },
    ] {
        assert_eq!(
            store
                .produce_expected("manual-bound", "catdesk", "test", "MANUAL DEBUG", &wrong)
                .unwrap_err(),
            "TARGET_EXPECTATION_MISMATCH"
        );
        assert!(store.events().unwrap().is_empty());
    }
    let first = store
        .produce_expected("manual-bound", "catdesk", "test", "MANUAL DEBUG", &target)
        .unwrap();
    assert_eq!(
        first,
        store
            .produce_expected("manual-bound", "catdesk", "test", "MANUAL DEBUG", &target)
            .unwrap()
    );
    let next = store.set_target("catdesk", 1, CHAT_B).unwrap();
    assert_eq!(
        store
            .produce_expected("manual-bound", "catdesk", "test", "MANUAL DEBUG", &next)
            .unwrap_err(),
        "TARGET_EXPECTATION_MISMATCH"
    );
    assert_eq!(store.events().unwrap(), vec![first]);
}

#[test]
fn checked_manual_publication_and_rollover_never_assign_the_unexpected_target() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let target = store.config().unwrap().targets["catdesk"].clone();
    let barrier = Arc::new(Barrier::new(2));
    let root = fixture.root.clone();
    let gate = barrier.clone();
    let publisher = thread::spawn(move || {
        let store = Store::open_scoped_for_test(&root, root.parent().unwrap()).unwrap();
        gate.wait();
        store.produce_expected("manual-race", "catdesk", "test", "MANUAL DEBUG", &target)
    });
    barrier.wait();
    store.set_target("catdesk", 1, CHAT_B).unwrap();
    match publisher.join().unwrap() {
        Ok(event) => assert_eq!(event.target_generation, 1),
        Err(error) => assert_eq!(error, "TARGET_EXPECTATION_MISMATCH"),
    }
    assert!(
        store
            .events()
            .unwrap()
            .iter()
            .all(|event| event.target_generation == 1)
    );
}

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let trusted_parent = std::env::current_dir()
            .unwrap()
            .join("wake")
            .join("target")
            .join("protocol-store-fixtures");
        fs::create_dir_all(&trusted_parent).unwrap();
        let root = trusted_parent.join(format!(
            "catdesk-wake-test-{}-{}-{}",
            std::process::id(),
            now(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        // Production Store::open intentionally validates from the volume root.
        // The test-only constructor starts beneath this existing canonical
        // fixture parent and applies the same descendant checks.
        let store = Store::open_scoped_for_test(&root, &trusted_parent).unwrap();
        store.initialize().unwrap();
        store.set_target("catdesk", 0, CHAT_A).unwrap();
        Self { root }
    }
    fn store(&self) -> Store {
        Store::open_scoped_for_test(&self.root, self.root.parent().unwrap()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn event() -> Event {
    Event {
        schema_version: 1,
        event_id: "review-123".into(),
        project_id: "catdesk".into(),
        event_type: "review_ready".into(),
        created_utc: now(),
        message: "CatDesk review-123 is ready for review.".into(),
        target_generation: 1,
        audit_references: vec![],
    }
}

#[test]
fn exact_url_grammar_preserves_narrow_web_compatibility() {
    for url in [CHAT_A, "https://chatgpt.com/c/WEB:abc-123"] {
        validate_target(url).unwrap();
    }
    for url in [
        "https://chatgpt.com/c/",
        "https://chatgpt.com/c/WEB:",
        "https://chatgpt.com/c/web:abc",
        "https://chatgpt.com/c/WEB:a:b",
        "https://chatgpt.com/c/a?q=1",
        "https://chatgpt.com:443/c/a",
        "https://chatgpt.com.evil/c/a",
        " https://chatgpt.com/c/a",
        "https://chatgpt.com/c/a/",
        "https://chatgpt.com/c/%61",
    ] {
        assert!(validate_target(url).is_err(), "{url}");
    }
}
#[test]
fn v1_allows_additive_fields_but_rejects_unknown_major() {
    let mut json = serde_json::to_value(event()).unwrap();
    json["futureOptionalField"] = true.into();
    let read: Event = serde_json::from_value(json.clone()).unwrap();
    read.validate().unwrap();
    json["schemaVersion"] = 2.into();
    let read: Event = serde_json::from_value(json).unwrap();
    assert!(read.validate().is_err());
}
#[test]
fn event_bounds_reject_paths_controls_and_unbounded_payloads() {
    let mut e = event();
    e.event_id = "../escape".into();
    assert!(e.validate().is_err());
    e = event();
    e.message = "x".repeat(2049);
    assert!(e.validate().is_err());
    e = event();
    e.message = "hello\nworld".into();
    assert!(e.validate().is_err());
    e = event();
    e.project_id = "WEB:catdesk".into();
    assert!(e.validate().is_err());
}
#[test]
fn queue_is_durable_idempotent_and_rejects_changed_duplicate() {
    let f = Fixture::new();
    let s = f.store();
    let mut e = event();
    s.publish(&e).unwrap();
    s.publish(&e).unwrap();
    assert_eq!(f.store().events().unwrap(), vec![e.clone()]);
    e.message.push('!');
    assert_eq!(s.publish(&e).unwrap_err(), "QUEUE_EVENT_ID_CONFLICT");
}
#[test]
fn interrupted_temp_write_is_invisible_to_queue() {
    let f = Fixture::new();
    fs::write(f.root.join("queue/interrupted.tmp"), b"{partial").unwrap();
    assert!(f.store().events().unwrap().is_empty());
}
#[test]
fn concurrent_target_updates_have_one_winner_and_visible_cas_conflict() {
    let f = Fixture::new();
    let barrier = Arc::new(Barrier::new(2));
    let jobs: Vec<_> = [CHAT_B, "https://chatgpt.com/c/chat-c"]
        .into_iter()
        .map(|url| {
            let root = f.root.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let s = Store::open_scoped_for_test(&root, root.parent().unwrap()).unwrap();
                barrier.wait();
                s.set_target("catdesk", 1, url)
            })
        })
        .collect();
    let results: Vec<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().find_map(|r| r.as_ref().err()).unwrap(),
        "CONFIG_CAS_CONFLICT"
    );
    assert_eq!(f.store().config().unwrap().targets["catdesk"].generation, 2);
}
#[test]
fn pending_event_never_moves_to_new_target() {
    let f = Fixture::new();
    let s = f.store();
    let e = event();
    s.publish(&e).unwrap();
    s.set_target("catdesk", 1, CHAT_B).unwrap();
    let lease = s.host_lock().unwrap();
    assert_eq!(s.claim(&e, &lease).unwrap_err(), "TARGET_GENERATION_STALE");
    assert_eq!(s.events().unwrap()[0].target_generation, 1);
}
#[test]
fn single_host_claim_and_pre_submit_recovery() {
    let f = Fixture::new();
    let s = f.store();
    let e = event();
    s.publish(&e).unwrap();
    let lease = s.host_lock().unwrap();
    assert!(s.host_lock().is_err());
    let first = s.claim(&e, &lease).unwrap();
    drop(lease);
    let restarted = f.store();
    let lease = restarted.host_lock().unwrap();
    let second = restarted.claim(&e, &lease).unwrap();
    assert_ne!(first.owner, second.owner);
    assert_eq!(
        s.transition(&e.event_id, &first.owner, Phase::Submitting, None, None)
            .unwrap_err(),
        "SUBMISSION_OWNER_CONFLICT"
    );
}
#[test]
fn target_change_before_submit_is_refused_at_boundary() {
    let f = Fixture::new();
    let s = f.store();
    let e = event();
    s.publish(&e).unwrap();
    let lease = s.host_lock().unwrap();
    s.claim(&e, &lease).unwrap();
    s.set_target("catdesk", 1, CHAT_B).unwrap();
    assert_eq!(
        s.transition(&e.event_id, lease.owner(), Phase::Submitting, None, None)
            .unwrap_err(),
        "TARGET_GENERATION_STALE"
    );
}
#[test]
fn crash_during_submission_never_replays_or_retargets() {
    let f = Fixture::new();
    let s = f.store();
    let e = event();
    s.publish(&e).unwrap();
    let lease = s.host_lock().unwrap();
    s.claim(&e, &lease).unwrap();
    s.transition(&e.event_id, lease.owner(), Phase::Submitting, None, None)
        .unwrap();
    drop(lease);
    let lease = s.host_lock().unwrap();
    assert_eq!(
        s.claim(&e, &lease).unwrap_err(),
        "SUBMISSION_RECONCILIATION_REQUIRED"
    );
    assert_eq!(
        s.set_target("catdesk", 1, CHAT_B).unwrap_err(),
        "CONFIG_SUBMISSION_RECONCILIATION_REQUIRED"
    );
}
#[test]
fn explicit_target_rollover_quarantines_submitting_without_replay() {
    let f = Fixture::new();
    let s = f.store();
    let e = event();
    s.publish(&e).unwrap();
    let lease = s.host_lock().unwrap();
    let before = s.claim(&e, &lease).unwrap();
    s.transition(&e.event_id, lease.owner(), Phase::Submitting, None, None)
        .unwrap();
    drop(lease);

    let updated = s
        .set_target_quarantining_submitting("catdesk", 1, CHAT_B)
        .unwrap();
    assert_eq!(updated.generation, 2);
    assert_eq!(updated.url, CHAT_B);

    let quarantined = s.delivery(&e.event_id).unwrap().unwrap();
    assert_eq!(quarantined.phase, Phase::Submitting);
    assert_eq!(quarantined.target, before.target);
    assert!(quarantined.receipt.is_none());
    assert_eq!(s.events().unwrap(), vec![e.clone()]);

    let lease = s.host_lock().unwrap();
    assert_eq!(s.claim(&e, &lease).unwrap_err(), "TARGET_GENERATION_STALE");
    assert_eq!(
        s.set_target("catdesk", 2, CHAT_A).unwrap_err(),
        "CONFIG_SUBMISSION_RECONCILIATION_REQUIRED"
    );
}

#[test]
fn rollover_allows_later_generation_progress_while_old_submission_stays_quarantined() {
    let f = Fixture::new();
    let s = f.store();
    let old = event();
    s.publish(&old).unwrap();
    let old_lease = s.host_lock().unwrap();
    let old_delivery = s.claim(&old, &old_lease).unwrap();
    s.transition(
        &old.event_id,
        old_lease.owner(),
        Phase::Submitting,
        None,
        Some("SUBMIT_CLEARED_NO_APPEND".into()),
    )
    .unwrap();
    drop(old_lease);

    let updated = s
        .set_target_quarantining_submitting("catdesk", 1, CHAT_B)
        .unwrap();
    assert_eq!(updated.generation, 2);

    let mut later = event();
    later.event_id = "review-124".into();
    later.message = "CatDesk review-124 is ready for review.".into();
    later.target_generation = 2;
    s.publish(&later).unwrap();

    let lease = s.host_lock().unwrap();
    let claimed = s.claim(&later, &lease).unwrap();
    assert_eq!(claimed.target, updated);
    s.transition(
        &later.event_id,
        lease.owner(),
        Phase::Submitting,
        None,
        None,
    )
    .unwrap();
    let receipt = Receipt {
        schema_version: 1,
        event_id: later.event_id.clone(),
        target_generation: claimed.target.generation,
        target_digest: claimed.target.digest.clone(),
        message_digest: claimed.message_digest.clone(),
        sent_utc: now(),
        evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
    };
    s.transition(
        &later.event_id,
        lease.owner(),
        Phase::Sent,
        Some(receipt),
        None,
    )
    .unwrap();

    let quarantined = s.delivery(&old.event_id).unwrap().unwrap();
    assert_eq!(quarantined.phase, Phase::Submitting);
    assert_eq!(quarantined.target, old_delivery.target);
    assert_eq!(
        quarantined.reason.as_deref(),
        Some("SUBMIT_CLEARED_NO_APPEND")
    );
    assert!(quarantined.receipt.is_none());
    assert_eq!(s.events().unwrap(), vec![old]);
}

#[test]
fn exact_receipt_survives_restart_and_prevents_replay() {
    let f = Fixture::new();
    let s = f.store();
    let e = event();
    s.publish(&e).unwrap();
    let lease = s.host_lock().unwrap();
    let d = s.claim(&e, &lease).unwrap();
    s.transition(&e.event_id, lease.owner(), Phase::Submitting, None, None)
        .unwrap();
    let receipt = Receipt {
        schema_version: 1,
        event_id: e.event_id.clone(),
        target_generation: 1,
        target_digest: d.target.digest,
        message_digest: d.message_digest,
        sent_utc: now(),
        evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
    };
    let mut stale = receipt.clone();
    stale.target_generation = 2;
    assert_eq!(
        s.transition(&e.event_id, lease.owner(), Phase::Sent, Some(stale), None)
            .unwrap_err(),
        "RECEIPT_INVALID"
    );
    s.transition(
        &e.event_id,
        lease.owner(),
        Phase::Sent,
        Some(receipt.clone()),
        None,
    )
    .unwrap();
    drop(lease);
    assert_eq!(
        f.store().delivery(&e.event_id).unwrap().unwrap().receipt,
        Some(receipt)
    );
    let lease = s.host_lock().unwrap();
    assert!(s.claim(&e, &lease).is_err());
}
#[test]
fn malformed_receipt_fails_closed() {
    let f = Fixture::new();
    fs::write(f.root.join("deliveries/review-123.json"), b"{broken").unwrap();
    assert!(f.store().delivery("review-123").is_err());
}

#[test]
fn producer_replay_keeps_original_target_generation() {
    let f = Fixture::new();
    let s = f.store();
    let first = s
        .produce("review-1", "catdesk", "review_ready", "bounded wake")
        .unwrap();
    s.set_target("catdesk", 1, CHAT_B).unwrap();
    assert_eq!(
        s.produce("review-1", "catdesk", "review_ready", "bounded wake")
            .unwrap(),
        first
    );
    let lease = s.host_lock().unwrap();
    assert!(s.claim(&first, &lease).is_err());
}

#[test]
fn pre_submit_retry_is_explicit_and_submitting_can_never_be_downgraded() {
    let f = Fixture::new();
    let s = f.store();
    let e = event();
    s.publish(&e).unwrap();
    let lease = s.host_lock().unwrap();
    s.claim(&e, &lease).unwrap();
    s.transition(
        &e.event_id,
        lease.owner(),
        Phase::Attention,
        None,
        Some("LOGIN_OR_PROFILE_REQUIRED".into()),
    )
    .unwrap();
    s.retry_pre_submit(&e.event_id).unwrap();
    s.claim(&e, &lease).unwrap();
    s.transition(&e.event_id, lease.owner(), Phase::Submitting, None, None)
        .unwrap();
    assert!(s.retry_pre_submit(&e.event_id).is_err());
    assert!(
        s.transition(
            &e.event_id,
            lease.owner(),
            Phase::Attention,
            None,
            Some("UNKNOWN".into())
        )
        .is_err()
    );
}

#[test]
fn stale_retirement_archives_exact_event_and_is_idempotent() {
    let f = Fixture::new();
    let s = f.store();
    let e = event();
    s.publish(&e).unwrap();

    s.retire_stale(&e.event_id).unwrap();
    assert!(s.events().unwrap().is_empty());
    let delivery = s.delivery(&e.event_id).unwrap().unwrap();
    assert_eq!(delivery.phase, Phase::Stale);
    assert_eq!(delivery.reason.as_deref(), Some("OPERATOR_RETIRED"));
    assert!(delivery.receipt.is_none());
    let archived: Event =
        serde_json::from_slice(&fs::read(f.root.join("archive/review-123.json")).unwrap()).unwrap();
    assert_eq!(archived, e);

    let delivery_before = fs::read(f.root.join("deliveries/review-123.json")).unwrap();
    let archive_before = fs::read(f.root.join("archive/review-123.json")).unwrap();
    s.retire_stale(&e.event_id).unwrap();
    assert_eq!(
        delivery_before,
        fs::read(f.root.join("deliveries/review-123.json")).unwrap()
    );
    assert_eq!(
        archive_before,
        fs::read(f.root.join("archive/review-123.json")).unwrap()
    );
}

#[test]
fn stale_retirement_refuses_submitting_and_sent_boundaries() {
    let f = Fixture::new();
    let s = f.store();
    let e = event();
    s.publish(&e).unwrap();
    let lease = s.host_lock().unwrap();
    s.claim(&e, &lease).unwrap();
    s.transition(&e.event_id, lease.owner(), Phase::Submitting, None, None)
        .unwrap();
    assert_eq!(
        s.retire_stale(&e.event_id).unwrap_err(),
        "STALE_RETIREMENT_REFUSED"
    );
    assert!(f.root.join("queue/review-123.json").exists());
    assert!(!f.root.join("archive/review-123.json").exists());
    assert_eq!(
        s.delivery(&e.event_id).unwrap().unwrap().phase,
        Phase::Submitting
    );

    let f = Fixture::new();
    let s = f.store();
    let e = event();
    s.publish(&e).unwrap();
    let lease = s.host_lock().unwrap();
    let claimed = s.claim(&e, &lease).unwrap();
    s.transition(&e.event_id, lease.owner(), Phase::Submitting, None, None)
        .unwrap();
    let receipt = Receipt {
        schema_version: 1,
        event_id: e.event_id.clone(),
        target_generation: e.target_generation,
        target_digest: claimed.target.digest,
        message_digest: claimed.message_digest,
        sent_utc: now(),
        evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
    };
    s.transition(
        &e.event_id,
        lease.owner(),
        Phase::Sent,
        Some(receipt.clone()),
        None,
    )
    .unwrap();
    assert_eq!(
        s.retire_stale(&e.event_id).unwrap_err(),
        "STALE_RETIREMENT_REFUSED"
    );
    let sent = s.delivery(&e.event_id).unwrap().unwrap();
    assert_eq!(sent.phase, Phase::Sent);
    assert_eq!(sent.receipt, Some(receipt));
    assert!(!f.root.join("queue/review-123.json").exists());
    assert!(f.root.join("archive/review-123.json").exists());
}

#[test]
fn stale_retirement_and_submit_race_has_exactly_one_safe_winner() {
    let f = Fixture::new();
    let s = f.store();
    let e = event();
    s.publish(&e).unwrap();
    let lease = s.host_lock().unwrap();
    s.claim(&e, &lease).unwrap();
    let owner = lease.owner().to_string();
    let barrier = Arc::new(Barrier::new(2));

    let retire_root = f.root.clone();
    let retire_barrier = barrier.clone();
    let id = e.event_id.clone();
    let retire = thread::spawn(move || {
        let store =
            Store::open_scoped_for_test(&retire_root, retire_root.parent().unwrap()).unwrap();
        retire_barrier.wait();
        store.retire_stale(&id)
    });

    let submit_root = f.root.clone();
    let submit_barrier = barrier.clone();
    let id = e.event_id.clone();
    let submit = thread::spawn(move || {
        let store =
            Store::open_scoped_for_test(&submit_root, submit_root.parent().unwrap()).unwrap();
        submit_barrier.wait();
        store.transition(&id, &owner, Phase::Submitting, None, None)
    });

    let retire_result = retire.join().unwrap();
    let submit_result = submit.join().unwrap();
    assert_eq!(
        usize::from(retire_result.is_ok()) + usize::from(submit_result.is_ok()),
        1
    );

    let delivery = s.delivery(&e.event_id).unwrap().unwrap();
    match delivery.phase {
        Phase::Stale => {
            assert_eq!(delivery.reason.as_deref(), Some("OPERATOR_RETIRED"));
            assert!(!f.root.join("queue/review-123.json").exists());
            assert!(f.root.join("archive/review-123.json").exists());
            assert!(submit_result.is_err());
        }
        Phase::Submitting => {
            assert!(f.root.join("queue/review-123.json").exists());
            assert!(!f.root.join("archive/review-123.json").exists());
            assert!(retire_result.is_err());
        }
        other => panic!("unsafe race terminal phase: {other:?}"),
    }
}

#[test]
fn stale_retirement_rejects_queue_identity_mismatch_without_archiving() {
    let f = Fixture::new();
    let s = f.store();
    let mut e = event();
    s.publish(&e).unwrap();
    e.event_id = "review-other".into();
    fs::write(
        f.root.join("queue/review-123.json"),
        serde_json::to_vec_pretty(&e).unwrap(),
    )
    .unwrap();

    assert_eq!(
        s.retire_stale("review-123").unwrap_err(),
        "QUEUE_ID_MISMATCH"
    );
    assert!(!f.root.join("archive/review-123.json").exists());
    assert!(s.delivery("review-123").unwrap().is_none());
}

#[test]
fn noncanonical_message_whitespace_is_rejected_before_any_dispatch() {
    let mut e = event();
    for text in ["hello  world", " leading", "trailing ", "hello\u{a0}world"] {
        e.message = text.into();
        assert!(e.validate().is_err());
    }
}

#[test]
fn no_target_means_no_host_start_and_no_event() {
    let f = Fixture::new();
    let empty = f.root.join("empty");
    let s = Store::open_scoped_for_test(&empty, &f.root).unwrap();
    s.initialize().unwrap();
    catdesk_wake::runtime::start_installed(&s).unwrap();
    assert!(!empty.join("control.json").exists());
    assert!(
        s.produce("event", "catdesk", "review_ready", "bounded wake")
            .is_err()
    );
    assert!(s.events().unwrap().is_empty());
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_browser_install_fixture(root: &std::path::Path) -> PathBuf {
    let install = root.join("versions/test-install");
    fs::create_dir_all(&install).unwrap();
    let adapter = b"adapter-reviewed";
    let bridge = b"bridge-reviewed";
    fs::write(install.join("adapter.py"), adapter).unwrap();
    fs::write(install.join("wake_bridge.py"), bridge).unwrap();
    let adapter_hash = digest(adapter);
    let bridge_hash = digest(bridge);
    fs::write(
        install.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "artifacts": {
                "adapter.py": adapter_hash,
                "wake_bridge.py": bridge_hash,
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("current.json"),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "directory": "test-install",
            "hostSha256": "0".repeat(64),
            "binagotchySha256": "0".repeat(64),
            "adapterSha256": digest(adapter),
            "wakeBridgeSha256": digest(bridge),
        }))
        .unwrap(),
    )
    .unwrap();
    install
}

#[test]
fn installed_browser_artifacts_are_bound_to_current_pointer_and_manifest() {
    let f = Fixture::new();
    let install = write_browser_install_fixture(&f.root);
    let verified = catdesk_wake::runtime::verify_installed_browser_artifacts(&f.root, &install)
        .expect("valid reviewed browser artifacts");
    let canonical_install = fs::canonicalize(&install).unwrap();
    assert_eq!(verified.0, canonical_install.join("adapter.py"));
    assert_eq!(verified.1, canonical_install.join("wake_bridge.py"));

    fs::write(install.join("adapter.py"), b"adapter-tampered").unwrap();
    assert_eq!(
        catdesk_wake::runtime::verify_installed_browser_artifacts(&f.root, &install).unwrap_err(),
        "BROWSER_ADAPTER_HASH_MISMATCH"
    );
    fs::write(install.join("adapter.py"), b"adapter-reviewed").unwrap();

    fs::remove_file(install.join("adapter.py")).unwrap();
    assert_eq!(
        catdesk_wake::runtime::verify_installed_browser_artifacts(&f.root, &install).unwrap_err(),
        "BROWSER_ADAPTER_UNAVAILABLE"
    );
    fs::write(install.join("adapter.py"), b"adapter-reviewed").unwrap();

    let manifest_path = install.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["artifacts"]["adapter.py"] = serde_json::json!("f".repeat(64));
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert_eq!(
        catdesk_wake::runtime::verify_installed_browser_artifacts(&f.root, &install).unwrap_err(),
        "BROWSER_ADAPTER_HASH_MISMATCH"
    );

    write_browser_install_fixture(&f.root);
    fs::write(install.join("wake_bridge.py"), b"bridge-tampered").unwrap();
    assert_eq!(
        catdesk_wake::runtime::verify_installed_browser_artifacts(&f.root, &install).unwrap_err(),
        "BROWSER_PRIMITIVES_HASH_MISMATCH"
    );
}

#[test]
fn installed_host_hash_is_still_checked_before_browser_artifacts() {
    let f = Fixture::new();
    let install = write_browser_install_fixture(&f.root);
    fs::write(
        install.join("CatDeskWakeHost.exe"),
        b"not-the-reviewed-host",
    )
    .unwrap();
    assert_eq!(
        catdesk_wake::runtime::start_installed(&f.store()).unwrap_err(),
        "HOST_EXECUTABLE_HASH_MISMATCH"
    );
}
