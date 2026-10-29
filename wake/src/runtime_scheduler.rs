//! Fair browser slices over the existing queue and durable response timers.
//! A pending response reserves only its own project's conversation.
use crate::{
    Result,
    protocol::{Event, Target},
    store::{Phase, ResponseState, Store},
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn fair_round(
    store: &Store,
    targets: &BTreeMap<String, Target>,
    cursor: Option<&str>,
    probe_after: &BTreeMap<String, u64>,
    observed_utc: u64,
) -> Result<Vec<Event>> {
    let mut chosen = BTreeMap::<String, Event>::new();
    let mut uncertain = BTreeMap::<String, Event>::new();
    let mut blocked_projects = BTreeSet::new();
    // Existing created-time ordering is retained inside each project.
    for event in store.events()? {
        if targets
            .get(&event.project_id)
            .is_none_or(|target| target.generation != event.target_generation)
        {
            continue;
        }
        let delivery = store.delivery(&event.event_id)?;
        if delivery
            .as_ref()
            .is_none_or(|value| !matches!(value.phase, Phase::Submitting | Phase::Sent))
            && store.submission_witness_present(&event.event_id) != Ok(false)
        {
            // Partial/unreadable submission evidence reserves this project,
            // including a crash after start-witness commit but before the
            // SUBMITTING delivery write. Sibling projects remain eligible.
            blocked_projects.insert(event.project_id.clone());
            continue;
        }
        if delivery
            .as_ref()
            .is_some_and(|value| matches!(value.phase, Phase::Sent | Phase::Stale))
        {
            continue;
        }
        if event.is_expired_at(observed_utc)? {
            match delivery.as_ref().map(|value| &value.phase) {
                None => continue,
                Some(Phase::Claimed | Phase::Attention) => {
                    // A pre-submit crash may have left an owned composer draft.
                    // Expiry cannot discard it or authorize another event in
                    // that conversation. Other projects remain schedulable.
                    blocked_projects.insert(event.project_id.clone());
                    continue;
                }
                _ => {}
            }
        }
        if delivery.as_ref().is_some_and(|value| {
            value.phase == Phase::Attention && value.reason.as_deref() != Some("CHATGPT_NOT_IDLE")
        }) {
            continue;
        }
        if delivery
            .as_ref()
            .is_some_and(|value| value.phase == Phase::Submitting)
        {
            uncertain
                .entry(event.project_id.clone())
                .or_insert_with(|| event.clone());
        }
        chosen.entry(event.project_id.clone()).or_insert(event);
    }
    for project in blocked_projects {
        chosen.remove(&project);
    }
    // The crash boundary can precede same-tab acceptance and its timer.
    // Such a delivery still owns the conversation until read-only recovery.
    chosen.extend(uncertain);
    let mut reserved = BTreeSet::new();
    let mut timers = store.timers()?;
    timers.sort_by(|a, b| (a.started_utc, &a.event_id).cmp(&(b.started_utc, &b.event_id)));
    for timer in timers {
        if timer.response_state == ResponseState::Complete
            || targets.get(&timer.project_id).is_none_or(|target| {
                target.generation != timer.target_generation || target.digest != timer.target_digest
            })
            || !reserved.insert(timer.project_id.clone())
        {
            continue;
        }
        // Manual turns and unknown receipts reserve this project, too. They
        // never authorize a read-only probe of an invented Wake delivery.
        chosen.remove(&timer.project_id);
        let Some(delivery) = store.delivery(&timer.event_id)? else {
            continue;
        };
        if delivery.target
            != *targets
                .get(&timer.project_id)
                .ok_or("TARGET_NOT_CONFIGURED")?
        {
            continue;
        }
        match delivery.phase {
            Phase::Submitting => {
                chosen.insert(timer.project_id, delivery.event);
            }
            Phase::Sent
                if delivery.receipt.is_some()
                    && probe_after
                        .get(&timer.event_id)
                        .is_none_or(|deadline| *deadline <= observed_utc) =>
            {
                chosen.insert(timer.project_id, delivery.event);
            }
            _ => {}
        }
    }
    let mut events: Vec<_> = chosen.into_values().collect();
    if let Some(cursor) = cursor {
        let split = events.partition_point(|event| event.project_id.as_str() <= cursor);
        events.rotate_left(split);
    }
    Ok(events)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::{
        PROTOCOL_VERSION,
        protocol::{WakeMetadata, WakePurpose, digest},
        store::{Delivery, HostLease, Receipt, now},
    };

    pub(crate) fn fixture(label: &str) -> (Store, HostLease) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "catdesk-cooperative-{label}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let store = Store::open_scoped_for_test(&base.join("wake"), &base).unwrap();
        store.initialize().unwrap();
        for (index, project) in ["alpha", "beta", "gamma"].iter().enumerate() {
            store
                .set_target(
                    project,
                    0,
                    &format!("https://chatgpt.com/c/6ab06a56-56c0-83e9-9697-9bc7c380c15{index}"),
                )
                .unwrap();
            for sequence in 1..=2 {
                store
                    .produce(
                        &format!("{project}-{sequence}"),
                        project,
                        "review_ready",
                        &format!("Review {project} step {sequence}"),
                    )
                    .unwrap();
            }
        }
        let lease = store.host_lock().unwrap();
        (store, lease)
    }

    pub(crate) fn sent(store: &Store, lease: &HostLease, id: &str) -> Delivery {
        let event = store
            .events()
            .unwrap()
            .into_iter()
            .find(|event| event.event_id == id)
            .unwrap();
        let delivery = store.claim(&event, lease).unwrap();
        store
            .transition(id, lease.owner(), Phase::Submitting, None, None)
            .unwrap();
        let receipt = Receipt {
            schema_version: PROTOCOL_VERSION,
            event_id: id.into(),
            target_generation: delivery.target.generation,
            target_digest: delivery.target.digest,
            message_digest: digest(&event.message),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        store.record_exact_receipt_timer(id, &receipt).unwrap();
        store
            .transition(id, lease.owner(), Phase::Sent, Some(receipt), None)
            .unwrap()
    }

    fn ids(store: &Store, cursor: Option<&str>, deadlines: &BTreeMap<String, u64>) -> Vec<String> {
        fair_round(
            store,
            &store.config().unwrap().targets,
            cursor,
            deadlines,
            now(),
        )
        .unwrap()
        .into_iter()
        .map(|event| event.event_id)
        .collect()
    }

    #[test]
    fn fair_round_selects_one_oldest_event_per_project_and_rotates() {
        let (store, _lease) = fixture("fair");
        assert_eq!(
            ids(&store, None, &BTreeMap::new()),
            ["alpha-1", "beta-1", "gamma-1"]
        );
        assert_eq!(
            ids(&store, Some("alpha"), &BTreeMap::new()),
            ["beta-1", "gamma-1", "alpha-1"]
        );
        assert_eq!(
            ids(&store, Some("gamma"), &BTreeMap::new()),
            ["alpha-1", "beta-1", "gamma-1"]
        );
    }

    #[test]
    fn pending_response_reserves_only_its_project_and_survives_restart() {
        let (store, lease) = fixture("restart");
        let first = sent(&store, &lease, "alpha-1");
        let cursor = store.delivery("alpha-1").unwrap().unwrap().event.project_id;
        assert_eq!(
            ids(&store, Some(&cursor), &BTreeMap::new()),
            ["beta-1", "gamma-1", "alpha-1"]
        );
        let future = BTreeMap::from([("alpha-1".into(), now() + 60)]);
        assert_eq!(ids(&store, None, &future), ["beta-1", "gamma-1"]);
        drop(lease);
        let reopened =
            Store::open_scoped_for_test(store.root(), store.root().parent().unwrap()).unwrap();
        assert_eq!(
            ids(&reopened, Some(&cursor), &BTreeMap::new()),
            ["beta-1", "gamma-1", "alpha-1"]
        );
        reopened
            .record_response_probe("alpha-1", &first.target, true)
            .unwrap();
        assert_eq!(
            ids(&reopened, None, &BTreeMap::new()),
            ["alpha-2", "beta-1", "gamma-1"]
        );
        assert_eq!(
            reopened.delivery("alpha-1").unwrap().unwrap().phase,
            Phase::Sent
        );
    }

    #[test]
    fn manual_turn_blocks_only_its_conversation_without_inventing_a_probe() {
        let (store, _lease) = fixture("manual");
        let target = store.config().unwrap().targets["alpha"].clone();
        store
            .start_manual_turn_timer("manual-turn-alpha", "alpha", &target)
            .unwrap();
        assert_eq!(ids(&store, None, &BTreeMap::new()), ["beta-1", "gamma-1"]);
    }

    #[test]
    fn accepted_unreceipted_submission_is_reconciled_before_next_project_turn() {
        let (store, lease) = fixture("submitting");
        let first = store
            .events()
            .unwrap()
            .into_iter()
            .find(|event| event.event_id == "alpha-1")
            .unwrap();
        store.claim(&first, &lease).unwrap();
        store
            .transition("alpha-1", lease.owner(), Phase::Submitting, None, None)
            .unwrap();
        store.start_submission_timer("alpha-1").unwrap();
        assert_eq!(
            ids(&store, None, &BTreeMap::new()),
            ["alpha-1", "beta-1", "gamma-1"]
        );
        assert_eq!(
            store.delivery("alpha-1").unwrap().unwrap().phase,
            Phase::Submitting
        );
    }

    #[test]
    fn submitting_without_acceptance_timer_still_reserves_conversation() {
        let (store, lease) = fixture("uncertain-no-timer");
        let uncertain = store
            .events()
            .unwrap()
            .into_iter()
            .find(|event| event.event_id == "alpha-2")
            .unwrap();
        store.claim(&uncertain, &lease).unwrap();
        store
            .transition("alpha-2", lease.owner(), Phase::Submitting, None, None)
            .unwrap();
        assert!(store.timer("alpha-2").unwrap().is_none());
        assert_eq!(
            ids(&store, None, &BTreeMap::new()),
            ["alpha-2", "beta-1", "gamma-1"]
        );
    }

    #[test]
    fn orphaned_pre_submit_witness_holds_only_its_project_and_survives_restart() {
        for claimed in [false, true] {
            for kind in ["submission", "timer"] {
                let (store, lease) = fixture("orphaned-pre-submit");
                let event = store
                    .events()
                    .unwrap()
                    .into_iter()
                    .find(|event| event.event_id == "alpha-1")
                    .unwrap();
                if claimed {
                    store.claim(&event, &lease).unwrap();
                }
                let target = store.config().unwrap().targets["alpha"].clone();
                let (path, witness) = if kind == "submission" {
                    (
                        store.root().join("logs/submission-start-alpha-1.json"),
                        serde_json::json!({
                            "schemaVersion":PROTOCOL_VERSION,"eventId":event.event_id,
                            "targetGeneration":target.generation,"targetDigest":target.digest,"startedUtc":event.created_utc
                        }),
                    )
                } else {
                    (
                        store.root().join("turns/alpha-1.json"),
                        serde_json::json!({
                            "schemaVersion":PROTOCOL_VERSION,"eventId":event.event_id,"projectId":event.project_id,
                            "targetGeneration":target.generation,"targetDigest":target.digest,"startedUtc":event.created_utc,
                            "completedUtc":null,"responseState":"OBSERVING"
                        }),
                    )
                };
                let bytes = serde_json::to_vec(&witness).unwrap();
                std::fs::write(&path, &bytes).unwrap();
                let original_delivery = store.delivery(&event.event_id).unwrap();
                let queue_path = store.root().join("queue/alpha-1.json");
                let queue_bytes = std::fs::read(&queue_path).unwrap();
                assert_eq!(ids(&store, None, &BTreeMap::new()), ["beta-1", "gamma-1"]);
                assert_eq!(
                    store.claim(&event, &lease).unwrap_err(),
                    "SUBMISSION_RECONCILIATION_REQUIRED"
                );
                drop(lease);
                let reopened =
                    Store::open_scoped_for_test(store.root(), store.root().parent().unwrap())
                        .unwrap();
                assert_eq!(
                    ids(&reopened, None, &BTreeMap::new()),
                    ["beta-1", "gamma-1"]
                );
                assert_eq!(
                    reopened.delivery(&event.event_id).unwrap(),
                    original_delivery
                );
                assert_eq!(std::fs::read(path).unwrap(), bytes);
                assert_eq!(std::fs::read(queue_path).unwrap(), queue_bytes);
                std::fs::remove_dir_all(store.root().parent().unwrap()).unwrap();
            }
        }
    }

    fn manual_event(store: &Store, created_utc: u64, expires_utc: u64) -> Event {
        let event = Event {
            schema_version: PROTOCOL_VERSION,
            event_id: "alpha-manual".into(),
            project_id: "alpha".into(),
            event_type: "engineering_attention".into(),
            created_utc,
            message: "Continue alpha's assigned task".into(),
            target_generation: store.config().unwrap().targets["alpha"].generation,
            audit_references: Vec::new(),
        }
        .with_wake_metadata(WakeMetadata {
            purpose: WakePurpose::ManualEndTurn,
            source_event_id: "alpha-source-event".into(),
            source_turn_id: "alpha-source-turn".into(),
            priority: 255,
            expires_utc,
        })
        .unwrap();
        store.publish(&event).unwrap();
        event
    }

    #[test]
    fn wake_metadata_expiry_skips_only_unclaimed_work_and_preserves_claimed_drafts_on_restart() {
        let (store, lease) = fixture("expiry-claimed");
        let event = manual_event(&store, now() - 120, now() - 60);
        // The expired intent remains durable, but does not consume a slice.
        assert_eq!(
            ids(&store, None, &BTreeMap::new()),
            ["alpha-1", "beta-1", "gamma-1"]
        );
        assert!(store.delivery(&event.event_id).unwrap().is_none());
        let claimed = store.claim(&event, &lease).unwrap();
        assert_eq!(ids(&store, None, &BTreeMap::new()), ["beta-1", "gamma-1"]);
        drop(lease);
        let reopened =
            Store::open_scoped_for_test(store.root(), store.root().parent().unwrap()).unwrap();
        assert_eq!(
            ids(&reopened, None, &BTreeMap::new()),
            ["beta-1", "gamma-1"]
        );
        assert_eq!(reopened.delivery(&event.event_id).unwrap(), Some(claimed));
        assert!(reopened.events().unwrap().contains(&event));
        std::fs::remove_dir_all(store.root().parent().unwrap()).unwrap();
    }

    #[test]
    fn wake_metadata_expiry_never_suppresses_unknown_submit_or_sent_response_observation() {
        let (store, lease) = fixture("expiry-reconciliation");
        let event = manual_event(&store, now(), now() + 300);
        let delivery = store.claim(&event, &lease).unwrap();
        store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Submitting,
                None,
                None,
            )
            .unwrap();
        let observed_after_expiry = event.wake_metadata().unwrap().unwrap().expires_utc;
        let round = || {
            fair_round(
                &store,
                &store.config().unwrap().targets,
                None,
                &BTreeMap::new(),
                observed_after_expiry,
            )
            .unwrap()
            .into_iter()
            .map(|event| event.event_id)
            .collect::<Vec<_>>()
        };
        assert_eq!(round(), ["alpha-manual", "beta-1", "gamma-1"]);
        assert_eq!(
            store.claim(&event, &lease).unwrap_err(),
            "SUBMISSION_RECONCILIATION_REQUIRED"
        );
        let receipt = Receipt {
            schema_version: PROTOCOL_VERSION,
            event_id: event.event_id.clone(),
            target_generation: delivery.target.generation,
            target_digest: delivery.target.digest,
            message_digest: digest(&event.message),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        store
            .record_exact_receipt_timer(&event.event_id, &receipt)
            .unwrap();
        store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Sent,
                Some(receipt),
                None,
            )
            .unwrap();
        assert_eq!(round(), ["alpha-manual", "beta-1", "gamma-1"]);
        assert_eq!(
            store.delivery(&event.event_id).unwrap().unwrap().phase,
            Phase::Sent
        );
        drop(lease);
        std::fs::remove_dir_all(store.root().parent().unwrap()).unwrap();
    }

    #[test]
    fn wake_metadata_priority_does_not_override_fifo_or_project_rotation() {
        let (store, lease) = fixture("priority-fairness");
        manual_event(&store, now() + 1, now() + 300);
        assert_eq!(
            ids(&store, Some("alpha"), &BTreeMap::new()),
            ["beta-1", "gamma-1", "alpha-1"]
        );
        drop(lease);
        std::fs::remove_dir_all(store.root().parent().unwrap()).unwrap();
    }
}
