//! Bounded, local receipt accounting for the existing project observer.
//! This module never creates/binds a target, sends work or advances a rollover.
use crate::web_conversation_context::{ConversationKey, WebContextStore};
use catdesk_wake::{
    protocol::{Target, digest},
    store::{Phase, Store},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub(super) fn reconcile(
    workspace: &Path,
    project: &str,
    keep: &dyn Fn() -> Result<(), String>,
) -> Result<Value, String> {
    keep()?;
    let key = super::paired_key(workspace, project)?;
    let target = Target::new(key.target_generation, key.conversation_url.clone())?;
    let root = workspace.join(".catdesk/web-context");
    if !root.try_exists().map_err(|_| "WEB_CONTEXT_UNAVAILABLE")? {
        return Ok(json!({"status":"UNOBSERVED","browserAction":"NONE","rolloverRequested":false}));
    }
    let confirm = || {
        keep()?;
        crate::mcp::with_current_project_web_target(
            workspace,
            project,
            target.generation,
            &target.digest,
            |_, _| Ok(()),
        )
    };
    confirm()?;
    let meter = WebContextStore::open_nonblocking(root)?;
    if meter.active_key(project)?.as_ref() != Some(&key) {
        return Err("WEB_CONTEXT_TARGET_CHANGED".into());
    }
    let wake = Store::open_read_only(&crate::wake_protocol_client::wake_root_for_workspace(
        workspace,
    )?)?;
    let output = reconcile_with_store(&wake, &meter, &key, &target, keep)?;
    confirm()?;
    if meter.active_key(project)?.as_ref() != Some(&key) {
        return Err("WEB_CONTEXT_TARGET_CHANGED".into());
    }
    Ok(output)
}

fn reconcile_with_store(
    wake: &Store,
    meter: &WebContextStore,
    key: &ConversationKey,
    target: &Target,
    keep: &dyn Fn() -> Result<(), String>,
) -> Result<Value, String> {
    keep()?;
    if key.target_generation != target.generation
        || key.conversation_url != target.url
        || key.target_sha256() != target.digest
        || wake.config()?.targets.get(&key.project_id) != Some(target)
        || meter.active_key(&key.project_id)?.as_ref() != Some(key)
    {
        return Err("WEB_CONTEXT_TARGET_CHANGED".into());
    }
    let known = meter.delivered_user_ids(key)?;
    let mut timers = wake.timers_for_project(&key.project_id)?;
    if timers.len() > 10_000 {
        return Err("WEB_CONTEXT_RECEIPT_SCAN_LIMIT".into());
    }
    timers.sort_by(|a, b| (a.started_utc, &a.event_id).cmp(&(b.started_utc, &b.event_id)));
    let deadline = Instant::now() + Duration::from_secs(5);
    let (mut recorded, mut already_known, mut unconfirmed, mut body_unavailable, mut limit_reached) =
        (0, 0, 0, 0, false);
    for timer in timers {
        keep()?;
        if Instant::now() >= deadline || recorded == 64 {
            limit_reached = true;
            break;
        }
        if timer.target_generation != target.generation || timer.target_digest != target.digest {
            continue;
        }
        if known.contains(&format!("delivered-user:{}", digest(&timer.event_id))) {
            already_known += 1;
            continue;
        }
        let Some(delivery) = wake.delivery(&timer.event_id)? else {
            body_unavailable += 1;
            continue;
        };
        if delivery.phase != Phase::Sent {
            unconfirmed += 1;
            continue;
        }
        if delivery.target != *target || delivery.event.project_id != key.project_id {
            return Err("WEB_CONTEXT_USER_RECEIPT_INVALID".into());
        }
        let receipt = delivery
            .receipt
            .as_ref()
            .ok_or("WEB_CONTEXT_USER_RECEIPT_INVALID")?;
        // Assistant failure, receipt age and timer completion do not undo a
        // persisted USER delivery. Unknown/SUBMITTING outcomes never enter here.
        meter.record_delivered_user(key, &delivery.event.message, receipt)?;
        recorded += 1;
    }
    keep()?;
    Ok(
        json!({"action":"web_context_reconcile_receipts","status":if limit_reached {"PARTIAL"} else {"RECORDED"},
        "projectId":key.project_id,"targetGeneration":target.generation,"targetSha256":target.digest,
        "recordedReceipts":recorded,"alreadyKnownReceipts":already_known,"unconfirmedSkipped":unconfirmed,
        "bodyUnavailableSkipped":body_unavailable,"limitReached":limit_reached,
        "observedTokenProxy":meter.pressure(key)?.observed_estimate_tokens,
        "browserAction":"NONE","hostStarted":false,"rolloverRequested":false,"rawTextPersisted":false,"providerContextExact":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use catdesk_wake::store::{Receipt, ResponseState, atomic, now};
    use std::{cell::Cell, fs};
    fn fixture() -> (Store, WebContextStore, ConversationKey, Target) {
        let root = std::env::temp_dir().join(format!("observer-receipts-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let wake = Store::open_scoped_for_test(&root.join("wake"), &root).unwrap();
        wake.initialize().unwrap();
        let target = wake
            .set_target("alpha", 0, "https://chatgpt.com/c/receipt-observer-alpha")
            .unwrap();
        let key = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: target.url.clone(),
            target_generation: target.generation,
        };
        let meter = WebContextStore::open(root.join("context")).unwrap();
        meter.bind_observed_target(key.clone(), None).unwrap();
        (wake, meter, key, target)
    }
    fn delivered(wake: &Store, project: &str, id: &str) -> String {
        let event = wake
            .produce(id, project, "test", &format!("Bounded wake {id}"))
            .unwrap();
        let host = wake.host_lock().unwrap();
        let d = wake.claim(&event, &host).unwrap();
        wake.transition(id, &d.owner, Phase::Submitting, None, None)
            .unwrap();
        let r = Receipt {
            schema_version: 1,
            event_id: id.into(),
            target_generation: d.target.generation,
            target_digest: d.target.digest.clone(),
            message_digest: digest(&event.message),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        wake.record_exact_receipt_timer(id, &r).unwrap();
        wake.transition(id, &d.owner, Phase::Sent, Some(r), None)
            .unwrap();
        id.into()
    }

    #[test]
    fn confirmed_user_receipt_counts_despite_failed_assistant_and_replay_changes_no_state() {
        let (wake, meter, key, target) = fixture();
        let id = delivered(&wake, "alpha", "failed-assistant");
        wake.set_timer_response_state(&id, ResponseState::Attention)
            .unwrap();
        let before_timer = fs::read(wake.root().join("turns").join(format!("{id}.json"))).unwrap();
        let before_delivery =
            fs::read(wake.root().join("deliveries").join(format!("{id}.json"))).unwrap();
        let first = reconcile_with_store(&wake, &meter, &key, &target, &|| Ok(())).unwrap();
        assert_eq!(first["recordedReceipts"], 1);
        assert_eq!(
            meter.pressure(&key).unwrap().known_delivered_user_messages,
            1
        );
        let p = meter.pressure(&key).unwrap();
        let second = reconcile_with_store(&wake, &meter, &key, &target, &|| Ok(())).unwrap();
        assert_eq!(second["recordedReceipts"], 0);
        assert_eq!(second["alreadyKnownReceipts"], 1);
        assert_eq!(meter.pressure(&key).unwrap(), p);
        assert_eq!(
            fs::read(wake.root().join("turns").join(format!("{id}.json"))).unwrap(),
            before_timer
        );
        assert_eq!(
            fs::read(wake.root().join("deliveries").join(format!("{id}.json"))).unwrap(),
            before_delivery
        );
    }

    #[test]
    fn unknown_submission_and_manual_body_are_preserved_without_becoming_known_user_text() {
        let (wake, meter, key, target) = fixture();
        let event = wake
            .produce("unknown", "alpha", "test", "unconfirmed body")
            .unwrap();
        let host = wake.host_lock().unwrap();
        let d = wake.claim(&event, &host).unwrap();
        wake.transition(&event.event_id, &d.owner, Phase::Submitting, None, None)
            .unwrap();
        wake.start_submission_timer(&event.event_id).unwrap();
        drop(host);
        wake.start_manual_turn_timer("manual-turn-observer", "alpha", &target)
            .unwrap();
        let before = fs::read(wake.root().join("deliveries/unknown.json")).unwrap();
        let result = reconcile_with_store(&wake, &meter, &key, &target, &|| Ok(())).unwrap();
        assert_eq!(result["recordedReceipts"], 0);
        assert_eq!(result["unconfirmedSkipped"], 1);
        assert_eq!(result["bodyUnavailableSkipped"], 1);
        assert_eq!(
            meter.pressure(&key).unwrap().known_delivered_user_messages,
            0
        );
        assert_eq!(
            fs::read(wake.root().join("deliveries/unknown.json")).unwrap(),
            before
        );
    }

    #[test]
    fn attributable_broken_project_does_not_prevent_sibling_receipt_accounting() {
        let (wake, meter, alpha, target) = fixture();
        let beta_target = wake
            .set_target("beta", 0, "https://chatgpt.com/c/receipt-observer-beta")
            .unwrap();
        let beta = ConversationKey {
            project_id: "beta".into(),
            conversation_url: beta_target.url.clone(),
            target_generation: beta_target.generation,
        };
        meter.bind_observed_target(beta.clone(), None).unwrap();
        delivered(&wake, "alpha", "alpha-sent");
        delivered(&wake, "beta", "beta-sent");
        let path = wake.root().join("turns/beta-sent.json");
        let mut value: Value = catdesk_wake::store::read(&path).unwrap();
        value["startedUtc"] = json!(0);
        atomic(&path, &value).unwrap();
        assert!(reconcile_with_store(&wake, &meter, &beta, &beta_target, &|| Ok(())).is_err());
        let alpha_result =
            reconcile_with_store(&wake, &meter, &alpha, &target, &|| Ok(())).unwrap();
        assert_eq!(alpha_result["recordedReceipts"], 1);
        assert_eq!(
            meter.pressure(&beta).unwrap().known_delivered_user_messages,
            0
        );
        assert_eq!(catdesk_wake::store::read::<Value>(&path).unwrap(), value);
    }

    #[test]
    fn bounded_receipt_batches_make_durable_progress_without_a_new_cursor() {
        let (wake, meter, key, target) = fixture();
        for n in 0..65 {
            delivered(&wake, "alpha", &format!("batch-{n:03}"));
        }
        let first = reconcile_with_store(&wake, &meter, &key, &target, &|| Ok(())).unwrap();
        assert!(first["recordedReceipts"].as_u64().unwrap() <= 64);
        assert_eq!(first["limitReached"], true);
        for _ in 0..3 {
            reconcile_with_store(&wake, &meter, &key, &target, &|| Ok(())).unwrap();
        }
        assert_eq!(
            meter.pressure(&key).unwrap().known_delivered_user_messages,
            65
        );
        let last = reconcile_with_store(&wake, &meter, &key, &target, &|| Ok(())).unwrap();
        assert_eq!(last["recordedReceipts"], 0);
        assert_eq!(last["alreadyKnownReceipts"], 65);
    }

    #[test]
    fn cancellation_and_foreign_target_refuse_before_recording_receipts() {
        let (wake, meter, key, target) = fixture();
        delivered(&wake, "alpha", "one-sent");
        assert_eq!(
            reconcile_with_store(
                &wake,
                &meter,
                &key,
                &target,
                &|| Err("OWNER_STOPPED".into())
            )
            .unwrap_err(),
            "OWNER_STOPPED"
        );
        let foreign = Target::new(
            target.generation,
            "https://chatgpt.com/c/foreign-observer".into(),
        )
        .unwrap();
        assert_eq!(
            reconcile_with_store(&wake, &meter, &key, &foreign, &|| Ok(())).unwrap_err(),
            "WEB_CONTEXT_TARGET_CHANGED"
        );
        assert_eq!(
            meter.pressure(&key).unwrap().known_delivered_user_messages,
            0
        );
        let calls = Cell::new(0);
        let keep = || {
            calls.set(calls.get() + 1);
            if calls.get() > 2 {
                Err("OWNER_STOPPED".into())
            } else {
                Ok(())
            }
        };
        assert!(reconcile_with_store(&wake, &meter, &key, &target, &keep).is_err());
        // Any admitted metadata survives cancellation; a later pass remains idempotent.
        reconcile_with_store(&wake, &meter, &key, &target, &|| Ok(())).unwrap();
        assert_eq!(
            meter.pressure(&key).unwrap().known_delivered_user_messages,
            1
        );
    }

    #[cfg(windows)]
    #[test]
    fn existing_auto_step_accounts_receipts_without_a_new_browser_or_scheduler() {
        let (control, _, wake, _, _) = crate::web_work::tests::assignment_fixture_for("alpha");
        let target = wake.config().unwrap().targets["alpha"].clone();
        let key = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: target.url.clone(),
            target_generation: target.generation,
        };
        let meter = WebContextStore::open(control.join(".catdesk/web-context")).unwrap();
        meter.bind_observed_target(key.clone(), None).unwrap();
        delivered(&wake, "alpha", "auto-receipt");
        let (_, value) = super::super::auto::run_step_with_capture(
            &control,
            "alpha",
            &key,
            &target,
            &|| Ok(()),
            || Ok(json!({"fixtureCapture":true})),
        )
        .unwrap();
        assert_eq!(value["receiptAccounting"]["recordedReceipts"], 1);
        assert_eq!(
            meter.pressure(&key).unwrap().known_delivered_user_messages,
            1
        );
    }

    #[cfg(windows)]
    #[test]
    fn concurrent_context_change_is_reported_without_rebinding_or_erasing_admitted_history() {
        let (control, _, wake, _, _) = crate::web_work::tests::assignment_fixture_for("alpha");
        let target = wake.config().unwrap().targets["alpha"].clone();
        let key = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: target.url.clone(),
            target_generation: target.generation,
        };
        let meter = WebContextStore::open(control.join(".catdesk/web-context")).unwrap();
        meter.bind_observed_target(key.clone(), None).unwrap();
        delivered(&wake, "alpha", "context-race-receipt");
        let next = ConversationKey {
            conversation_url: "https://chatgpt.com/c/next-context-race".into(),
            target_generation: target.generation + 1,
            ..key.clone()
        };
        let changed = Cell::new(false);
        let keep = || {
            if !changed.get() && meter.pressure(&key)?.known_delivered_user_messages == 1 {
                meter.bind_observed_target(next.clone(), Some(&key))?;
                changed.set(true);
            }
            Ok(())
        };
        assert_eq!(
            reconcile(&control, "alpha", &keep).unwrap_err(),
            "WEB_CONTEXT_TARGET_CHANGED"
        );
        assert_eq!(meter.active_key("alpha").unwrap(), Some(next));
        assert_eq!(
            meter.pressure(&key).unwrap().known_delivered_user_messages,
            1
        );
        assert_eq!(wake.config().unwrap().targets["alpha"], target);
    }
}
