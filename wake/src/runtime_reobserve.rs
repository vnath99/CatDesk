//! Explicit one-shot recovery of a SENT turn; never a sender or scheduler.
use super::*;
use serde_json::{Value, json};

/// Re-observe one exact receipt under stopped-host ownership. The caller's
/// commit guard holds the project registry lock only for the final Wake CAS.
/// An old timer's automatic observation budget is deliberately not renewed.
pub fn reobserve_sent_response_guarded<F>(
    store: &Store,
    project: &str,
    target: &Target,
    event: &str,
    keep_running: &dyn Fn() -> Result<()>,
    commit_guard: F,
) -> Result<Value>
where
    F: FnOnce(&mut dyn FnMut() -> Result<bool>) -> Result<bool>,
{
    reobserve_with_observer(
        store,
        project,
        target,
        event,
        keep_running,
        commit_guard,
        |delivery, deadline, guard| {
            let installed = installed_bootstrap_directory(store)?;
            let mut adapter = Adapter::spawn_from_installed_guarded(
                store.root(),
                &installed,
                deadline.saturating_duration_since(Instant::now()),
                guard,
            )?;
            let result =
                response::read_sent_response(store, &mut adapter, delivery, deadline, &mut || {
                    guard()
                });
            let _ = adapter.allow_graceful_exit(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(5)),
            );
            result
        },
    )
}

fn reobserve_with_observer<F, O>(
    store: &Store,
    project: &str,
    target: &Target,
    event: &str,
    keep_running: &dyn Fn() -> Result<()>,
    commit_guard: F,
    observer: O,
) -> Result<Value>
where
    F: FnOnce(&mut dyn FnMut() -> Result<bool>) -> Result<bool>,
    O: FnOnce(&crate::store::Delivery, Instant, &dyn Fn() -> Result<()>) -> Result<Value>,
{
    let deadline = Instant::now() + Duration::from_secs(90);
    let owner_guard = || {
        if Instant::now() >= deadline {
            return Err("RESPONSE_REOBSERVE_TIMEOUT".into());
        }
        if desired(store)? != "STOPPED" {
            return Err("RESPONSE_REOBSERVE_HOST_NOT_STOPPED".into());
        }
        Ok(())
    };
    let guard = || {
        keep_running()?;
        owner_guard()
    };
    guard()?;
    let _owner = store.host_lock()?;
    guard()?;
    let (_, timer) = store.sent_response_scope(project, target, event)?;
    if timer.response_state == ResponseState::Complete {
        return Ok(
            json!({"responseState":"COMPLETE","alreadyComplete":true,"browserStarted":false,"sendAttempted":false}),
        );
    }
    ensure_source_snapshot_available(store, project, target, Some(event))?;
    let attempt = store.begin_response_reobservation(project, target, event)?;
    let result: Result<Value> = (|| {
        guard()?;
        let witness = observer(&attempt.delivery, deadline, &guard)?;
        guard()?;
        let complete = commit_guard(&mut || {
            // The caller now holds its registration lock. Only check local
            // owner/deadline here; paired reads would invert target locks.
            owner_guard()?;
            store.finish_response_reobservation(&attempt, &witness)
        })?;
        Ok(
            json!({"responseState":if complete {"COMPLETE"} else {"PENDING"},
            "alreadyComplete":false,"browserStarted":true,"sendAttempted":false}),
        )
    })();
    if let Err(reason) = result.as_ref() {
        // A failed outcome append never changes the timer; an existing outcome
        // after a partial commit is preserved rather than overwritten.
        let _ = store.fail_response_reobservation(&attempt, reason);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::super::scheduler::tests::{fixture, sent};
    use super::*;

    fn witness(delivery: &crate::store::Delivery, stage: &str) -> Value {
        json!({"stage":stage,"eventId":delivery.event.event_id,
            "targetGeneration":delivery.target.generation,"targetDigest":delivery.target.digest,
            "messageDigest":delivery.message_digest})
    }

    fn timer_bytes(store: &Store) -> Vec<u8> {
        std::fs::read(store.root().join("turns/alpha-1.json")).unwrap()
    }

    fn log_count(store: &Store) -> usize {
        std::fs::read_dir(store.root().join("logs"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("response-reobserve-")
            })
            .count()
    }

    #[test]
    fn reobserve_aged_attention_commits_only_fresh_witness_and_preserves_prior_timer() {
        let (store, lease) = fixture("reobserve-aged");
        let mut delivery = sent(&store, &lease, "alpha-1");
        store
            .set_timer_response_state("alpha-1", ResponseState::Attention)
            .unwrap();
        let mut timer = store.timer("alpha-1").unwrap().unwrap();
        timer.started_utc = now() - 7200;
        delivery.receipt.as_mut().unwrap().sent_utc = timer.started_utc;
        atomic(&store.root().join("turns/alpha-1.json"), &timer).unwrap();
        atomic(&store.root().join("deliveries/alpha-1.json"), &delivery).unwrap();
        let config = std::fs::read(store.root().join("config.json")).unwrap();
        let before_events = store.events().unwrap();
        let attempt = store
            .begin_response_reobservation("alpha", &delivery.target, "alpha-1")
            .unwrap();
        assert_eq!(attempt.timer, timer);
        assert!(
            store
                .finish_response_reobservation(&attempt, &witness(&delivery, "RESPONSE_COMPLETE"))
                .unwrap()
        );
        let after = store.timer("alpha-1").unwrap().unwrap();
        assert_eq!(after.started_utc, timer.started_utc);
        assert_eq!(after.response_state, ResponseState::Complete);
        assert!(after.completed_utc.is_some());
        assert_eq!(store.delivery("alpha-1").unwrap().unwrap(), delivery);
        assert_eq!(
            std::fs::read(store.root().join("config.json")).unwrap(),
            config
        );
        assert_eq!(store.events().unwrap(), before_events);
        assert_eq!(log_count(&store), 2);
        let records: Vec<Value> = std::fs::read_dir(store.root().join("logs"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("response-reobserve-")
            })
            .map(|entry| read(&entry.path()).unwrap())
            .collect();
        assert!(
            records
                .iter()
                .all(|row| row["priorTimer"]["responseState"] == "ATTENTION")
        );
        assert!(
            store
                .finish_response_reobservation(&attempt, &witness(&delivery, "RESPONSE_COMPLETE"))
                .is_err()
        );
        assert_eq!(log_count(&store), 2);
    }

    #[test]
    fn reobserve_pending_and_failure_preserve_exact_attention_timer_bytes() {
        for pending in [true, false] {
            let (store, lease) = fixture("reobserve-no-completion");
            let delivery = sent(&store, &lease, "alpha-1");
            store
                .set_timer_response_state("alpha-1", ResponseState::Attention)
                .unwrap();
            let before = timer_bytes(&store);
            let attempt = store
                .begin_response_reobservation("alpha", &delivery.target, "alpha-1")
                .unwrap();
            if pending {
                assert!(
                    !store
                        .finish_response_reobservation(
                            &attempt,
                            &witness(&delivery, "RESPONSE_PENDING")
                        )
                        .unwrap()
                );
            } else {
                store
                    .fail_response_reobservation(&attempt, "RESPONSE_SEQUENCE_DRIFT")
                    .unwrap();
            }
            assert_eq!(timer_bytes(&store), before);
            assert_eq!(log_count(&store), 2);
        }
    }

    #[test]
    fn failed_assistant_observations_preserve_delivery_and_allow_sibling_wake() {
        for reason in [
            "RESPONSE_LOAD_ERROR",
            "RESPONSE_NETWORK_ERROR",
            "RESPONSE_EXPECTED_USER_DUPLICATED",
            "RESPONSE_PROBE_TIMEOUT",
        ] {
            let (store, delivery) = ready_fixture("failed-response-sibling-progress");
            let before_timer = timer_bytes(&store);
            let path = store.root().join("deliveries/alpha-1.json");
            let before_delivery = std::fs::read(&path).unwrap();
            let result = reobserve_with_observer(
                &store,
                "alpha",
                &delivery.target,
                "alpha-1",
                &|| Ok(()),
                |_| panic!("failed assistant must not commit completion"),
                |_, _, _| Err(reason.into()),
            );
            assert_eq!(result.unwrap_err(), reason);
            assert_eq!(timer_bytes(&store), before_timer);
            assert_eq!(std::fs::read(&path).unwrap(), before_delivery);
            assert!(!store.root().join("queue/alpha-1.json").exists());
            let events = super::super::scheduler::fair_round(
                &store,
                &store.config().unwrap().targets,
                None,
                &std::collections::BTreeMap::new(),
                crate::store::now(),
            )
            .unwrap();
            let beta = events
                .iter()
                .find(|event| event.event_id == "beta-1")
                .unwrap();
            let lease = store.host_lock().unwrap();
            assert_eq!(store.claim(beta, &lease).unwrap().phase, Phase::Claimed);
        }
    }

    #[test]
    fn reobserve_scope_and_receipt_rejections_create_no_journal() {
        let (store, lease) = fixture("reobserve-scope");
        let mut delivery = sent(&store, &lease, "alpha-1");
        assert!(
            store
                .begin_response_reobservation("beta", &delivery.target, "alpha-1")
                .is_err()
        );
        assert!(
            store
                .begin_response_reobservation("alpha", &delivery.target, "../alpha-1")
                .is_err()
        );
        let mut wrong = delivery.target.clone();
        wrong.generation += 1;
        assert!(
            store
                .begin_response_reobservation("alpha", &wrong, "alpha-1")
                .is_err()
        );
        delivery.receipt.as_mut().unwrap().message_digest = "a".repeat(64);
        atomic(&store.root().join("deliveries/alpha-1.json"), &delivery).unwrap();
        assert!(
            store
                .begin_response_reobservation("alpha", &delivery.target, "alpha-1")
                .is_err()
        );
        assert_eq!(log_count(&store), 0);
    }

    #[test]
    fn reobserve_cas_rejects_changed_timer_delivery_or_current_target() {
        for change in ["timer", "delivery", "target"] {
            let (store, lease) = fixture("reobserve-cas");
            let mut delivery = sent(&store, &lease, "alpha-1");
            let attempt = store
                .begin_response_reobservation("alpha", &delivery.target, "alpha-1")
                .unwrap();
            match change {
                "timer" => store
                    .set_timer_response_state("alpha-1", ResponseState::Attention)
                    .unwrap(),
                "delivery" => {
                    delivery.reason = Some("CHANGED".into());
                    atomic(&store.root().join("deliveries/alpha-1.json"), &delivery).unwrap();
                }
                _ => {
                    store
                        .set_target(
                            "alpha",
                            delivery.target.generation,
                            "https://chatgpt.com/c/other",
                        )
                        .unwrap();
                }
            }
            let before = timer_bytes(&store);
            assert!(
                store
                    .finish_response_reobservation(
                        &attempt,
                        &witness(&delivery, "RESPONSE_COMPLETE")
                    )
                    .is_err()
            );
            assert_eq!(timer_bytes(&store), before);
            assert_eq!(log_count(&store), 1);
        }
    }

    #[test]
    fn reobserve_rejects_forged_or_extra_completion_fields() {
        for field in [
            "eventId",
            "targetGeneration",
            "targetDigest",
            "messageDigest",
            "extra",
            "stage",
        ] {
            let (store, lease) = fixture("reobserve-witness");
            let delivery = sent(&store, &lease, "alpha-1");
            let attempt = store
                .begin_response_reobservation("alpha", &delivery.target, "alpha-1")
                .unwrap();
            let before = timer_bytes(&store);
            let mut value = witness(&delivery, "RESPONSE_COMPLETE");
            value[field] = json!("forged");
            assert!(
                store
                    .finish_response_reobservation(&attempt, &value)
                    .is_err(),
                "{field}"
            );
            assert_eq!(timer_bytes(&store), before);
            assert_eq!(log_count(&store), 1);
        }
    }

    #[test]
    fn reobserve_already_complete_is_idempotent_without_browser_or_journal() {
        let (store, lease) = fixture("reobserve-complete");
        let delivery = sent(&store, &lease, "alpha-1");
        store
            .record_response_probe("alpha-1", &delivery.target, true)
            .unwrap();
        atomic(
            &store.root().join("control.json"),
            &json!({"schemaVersion":1,"desired":"STOPPED"}),
        )
        .unwrap();
        drop(lease);
        let before = timer_bytes(&store);
        let result = reobserve_sent_response_guarded(
            &store,
            "alpha",
            &delivery.target,
            "alpha-1",
            &|| Ok(()),
            |_| panic!("commit not needed"),
        )
        .unwrap();
        assert_eq!(result["alreadyComplete"], true);
        assert_eq!(result["browserStarted"], false);
        assert_eq!(timer_bytes(&store), before);
        assert_eq!(log_count(&store), 0);
    }

    #[test]
    fn reobserve_refuses_live_host_control_and_cancellation_before_browser() {
        let (store, lease) = fixture("reobserve-owner");
        let delivery = sent(&store, &lease, "alpha-1");
        for desired in ["RUNNING", "PAUSED", "STOPPED"] {
            atomic(
                &store.root().join("control.json"),
                &json!({"schemaVersion":1,"desired":desired}),
            )
            .unwrap();
            assert!(
                reobserve_sent_response_guarded(
                    &store,
                    "alpha",
                    &delivery.target,
                    "alpha-1",
                    &|| Ok(()),
                    |_| panic!("no commit")
                )
                .is_err()
            );
        }
        drop(lease);
        assert_eq!(
            reobserve_sent_response_guarded(
                &store,
                "alpha",
                &delivery.target,
                "alpha-1",
                &|| Err("TEST_CANCELLED".into()),
                |_| panic!("no commit")
            )
            .unwrap_err(),
            "TEST_CANCELLED"
        );
        assert_eq!(log_count(&store), 0);
    }

    fn ready_fixture(label: &str) -> (Store, crate::store::Delivery) {
        let (store, lease) = fixture(label);
        let delivery = sent(&store, &lease, "alpha-1");
        store
            .set_timer_response_state("alpha-1", ResponseState::Attention)
            .unwrap();
        std::fs::remove_file(store.root().join("queue/alpha-2.json")).unwrap();
        atomic(
            &store.root().join("control.json"),
            &json!({"schemaVersion":1,"desired":"STOPPED"}),
        )
        .unwrap();
        drop(lease);
        (store, delivery)
    }

    #[test]
    fn reobserve_first_entry_runs_observer_then_authority_guard_then_cas() {
        let (store, delivery) = ready_fixture("reobserve-pipeline");
        let sequence = std::cell::Cell::new(0);
        let registry_locked = std::cell::Cell::new(false);
        let result = reobserve_with_observer(
            &store,
            "alpha",
            &delivery.target,
            "alpha-1",
            &|| {
                assert!(
                    !registry_locked.get(),
                    "paired authority read under registry lock"
                );
                Ok(())
            },
            |commit| {
                assert_eq!(sequence.replace(2), 1);
                assert_eq!(
                    store.timer("alpha-1").unwrap().unwrap().response_state,
                    ResponseState::Attention
                );
                registry_locked.set(true);
                let result = commit();
                registry_locked.set(false);
                result
            },
            |current, deadline, guard| {
                assert_eq!(sequence.replace(1), 0);
                assert!(
                    deadline > Instant::now()
                        && deadline <= Instant::now() + Duration::from_secs(90)
                );
                guard()?;
                assert_eq!(current, &delivery);
                Ok(witness(current, "RESPONSE_COMPLETE"))
            },
        )
        .unwrap();
        assert_eq!(sequence.get(), 2);
        assert_eq!(result["responseState"], "COMPLETE");
        assert_eq!(
            store.timer("alpha-1").unwrap().unwrap().response_state,
            ResponseState::Complete
        );
        assert_eq!(log_count(&store), 2);
    }

    #[test]
    fn reobserve_first_entry_cancellation_and_timeout_preserve_timer_and_error() {
        for reason in ["TEST_CANCELLED", "RESPONSE_REOBSERVE_TIMEOUT"] {
            let (store, delivery) = ready_fixture("reobserve-pipeline-failed");
            let before = timer_bytes(&store);
            let cancelled = std::cell::Cell::new(false);
            let result = reobserve_with_observer(
                &store,
                "alpha",
                &delivery.target,
                "alpha-1",
                &|| {
                    if cancelled.get() {
                        Err(reason.into())
                    } else {
                        Ok(())
                    }
                },
                |_| panic!("failed observation must not reach commit"),
                |current, _, guard| {
                    cancelled.set(true);
                    guard()?;
                    Ok(witness(current, "RESPONSE_COMPLETE"))
                },
            );
            assert_eq!(result.unwrap_err(), reason);
            assert_eq!(timer_bytes(&store), before);
            let outcome = std::fs::read_dir(store.root().join("logs"))
                .unwrap()
                .filter_map(|entry| entry.ok())
                .find(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .ends_with("-outcome.json")
                })
                .unwrap();
            let value: Value = read(&outcome.path()).unwrap();
            assert_eq!(value["reason"], reason);
        }
    }

    #[test]
    fn reobserve_existing_outcome_or_commit_guard_failure_never_changes_timer() {
        let (store, delivery) = ready_fixture("reobserve-log-collision");
        let before = timer_bytes(&store);
        let attempt = store
            .begin_response_reobservation("alpha", &delivery.target, "alpha-1")
            .unwrap();
        let begin = std::fs::read_dir(store.root().join("logs"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .find(|entry| entry.file_name().to_string_lossy().ends_with("-begin.json"))
            .unwrap()
            .path();
        let outcome = begin.with_file_name(
            begin
                .file_name()
                .unwrap()
                .to_string_lossy()
                .replace("-begin.json", "-outcome.json"),
        );
        std::fs::write(&outcome, b"preserved outcome").unwrap();
        assert!(
            store
                .finish_response_reobservation(&attempt, &witness(&delivery, "RESPONSE_COMPLETE"))
                .is_err()
        );
        assert_eq!(std::fs::read(&outcome).unwrap(), b"preserved outcome");
        assert_eq!(timer_bytes(&store), before);
        let result = reobserve_with_observer(
            &store,
            "alpha",
            &delivery.target,
            "alpha-1",
            &|| Ok(()),
            |_| Err("WEB_REVIEW_TARGET_CHANGED".into()),
            |current, _, _| Ok(witness(current, "RESPONSE_COMPLETE")),
        );
        assert_eq!(result.unwrap_err(), "WEB_REVIEW_TARGET_CHANGED");
        assert_eq!(timer_bytes(&store), before);
    }
}
