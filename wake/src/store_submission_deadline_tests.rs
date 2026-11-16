use super::*;
use std::{sync::mpsc, thread, time::Duration};

fn claimed_fixture() -> (Store, HostLease, Delivery) {
    let base = std::env::temp_dir().join(format!(
        "submission-deadline-{}-{}",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::Relaxed),
    ));
    fs::create_dir(&base).unwrap();
    let store = Store::open_scoped_for_test(&base.join("wake"), &base).unwrap();
    store.initialize().unwrap();
    let target = store
        .set_target("alpha", 0, "https://chatgpt.com/c/deadline-fixture")
        .unwrap();
    let event = store
        .produce_expected(
            "deadline-event",
            "alpha",
            "engineering_attention",
            "fixture message never sent",
            &target,
        )
        .unwrap();
    let host = store.host_lock().unwrap();
    let delivery = store.claim(&event, &host).unwrap();
    (store, host, delivery)
}

#[test]
fn submission_deadline_expiring_while_transaction_lock_is_held_never_writes_a_fence() {
    let (store, host, claimed) = claimed_fixture();
    let path = store.delivery_path(&claimed.event.event_id);
    let before = fs::read(&path).unwrap();
    let locked = store.lock().unwrap();
    thread::scope(|scope| {
        let (started_tx, started_rx) = mpsc::sync_channel(0);
        let shared = &store;
        let id = &claimed.event.event_id;
        let owner = host.owner();
        let worker = scope.spawn(move || {
            let deadline = Instant::now() + Duration::from_millis(50);
            started_tx.send(deadline).unwrap();
            shared.begin_submission_before_deadline(id, owner, deadline)
        });
        let deadline = started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        // Keep the actual Store transaction locked until this monotonic
        // admission has expired. No sleep-based guess at acquisition order.
        while Instant::now() < deadline {
            thread::yield_now();
        }
        assert!(
            !store
                .submission_start_path(&claimed.event.event_id)
                .exists()
        );
        drop(locked);
        assert_eq!(worker.join().unwrap().unwrap_err(), "SCOPED_REVIEW_TIMEOUT");
    });
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(
        !store
            .submission_start_path(&claimed.event.event_id)
            .exists()
    );
    assert!(store.timer(&claimed.event.event_id).unwrap().is_none());
    // Legacy/unscoped transitions retain their original semantics.
    assert_eq!(
        store
            .transition(
                &claimed.event.event_id,
                host.owner(),
                Phase::Submitting,
                None,
                None
            )
            .unwrap()
            .phase,
        Phase::Submitting
    );
}

#[test]
fn submission_deadline_never_relabels_or_restarts_an_existing_uncertain_fence() {
    let (store, host, claimed) = claimed_fixture();
    store
        .transition(
            &claimed.event.event_id,
            host.owner(),
            Phase::Submitting,
            None,
            None,
        )
        .unwrap();
    let path = store.delivery_path(&claimed.event.event_id);
    let witness = store.submission_start_path(&claimed.event.event_id);
    let before = fs::read(&path).unwrap();
    let start = fs::read(&witness).unwrap();
    assert!(
        store
            .begin_submission_before_deadline(&claimed.event.event_id, host.owner(), Instant::now())
            .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(&witness).unwrap(), start);
    let unchanged = store.delivery(&claimed.event.event_id).unwrap().unwrap();
    assert_eq!(unchanged.phase, Phase::Submitting);
    assert!(unchanged.receipt.is_none());
}
