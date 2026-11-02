//! Explicit no-send reconciliation of an already delivered Web review.
use super::*;
use crate::delegated::autonomy_projects::resolve_registered_project_from_control_root;

/// Inspect only this already-dispatched review; never grant a replacement send.
pub(super) fn reconcile(workspace: &Path, project: &str, event: &str) -> Result<Value, String> {
    inspect_submitting(workspace, project, event, false)
}

pub(super) fn diagnose(workspace: &Path, project: &str, event: &str) -> Result<Value, String> {
    diagnose_using(
        workspace,
        project,
        event,
        catdesk_wake::runtime::diagnosed_sent_snapshot,
    )
}

fn diagnose_using<F>(
    workspace: &Path,
    project: &str,
    event: &str,
    snapshotter: F,
) -> Result<Value, String>
where
    F: FnOnce(
        &catdesk_wake::store::Store,
        &str,
        &catdesk_wake::protocol::Target,
        &str,
        &dyn Fn() -> Result<(), String>,
    ) -> catdesk_wake::runtime::SnapshotReadReport,
{
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let store = catdesk_wake::store::Store::open_read_only(&root)?;
    // This is only phase dispatch. The existing reader independently checks
    // the exact receipt, project, paired target and source isolation before it
    // opens a browser, and rechecks the pair after snapshot cleanup.
    if !store
        .delivery(event)?
        .is_some_and(|delivery| delivery.phase == catdesk_wake::store::Phase::Sent)
    {
        return inspect_submitting(workspace, project, event, true);
    }
    let lease = crate::operator_facade::acquire_web_work_lease(workspace, project)?;
    let key = paired_key(workspace, project)?;
    let target =
        catdesk_wake::protocol::Target::new(key.target_generation, key.conversation_url.clone())?;
    let guard = || lease.check_target(&target);
    guard()?;
    let read = read_review_using(
        workspace,
        project,
        event,
        Some((target.generation, &target.digest)),
        &guard,
        snapshotter,
    )?;
    guard()?;
    let verified = read["eventVerified"] == true;
    let mut diagnosis = json!({
        "scope":"CONFIRMED_SENT_RESPONSE", "snapshotVerified":verified,
        "responseState":if verified {"SNAPSHOT_VERIFIED"} else {"UNVERIFIED"},
        "diagnostic":read["diagnostic"], "browserReadOnly":true,
        "responseBodyExported":false, "receiptCreated":false,
        "stateChanged":false, "sendAttempted":false,
        "retryAuthorized":false, "workResolved":false
    });
    if !verified {
        diagnosis["reason"] = json!(catdesk_wake::runtime::snapshot_diagnostic_code(
            read["reason"].as_str().unwrap_or("SOURCE_SNAPSHOT_FAILED")
        ));
    }
    if let Some(tools) = read.get("toolDiagnostics") {
        diagnosis["toolDiagnostics"] = tools.clone();
    }
    // Do not forward private response text, snapshot, composer, URL, or an
    // adapter's raw failure. A verified read does not mutate response timers.
    Ok(
        json!({"action":"web_review_diagnose", "projectId":project,"eventId":event,
        "targetGeneration":target.generation,"targetDigest":target.digest,
        "deliveryPhase":"SENT", "diagnosis":diagnosis,
        "sendAttempted":false,"stateChanged":false,"workResolved":false}),
    )
}

fn inspect_submitting(
    workspace: &Path,
    project: &str,
    event: &str,
    diagnostic: bool,
) -> Result<Value, String> {
    let lease = crate::operator_facade::acquire_web_work_lease(workspace, project)?;
    let key = paired_key(workspace, project)?;
    let target =
        catdesk_wake::protocol::Target::new(key.target_generation, key.conversation_url.clone())?;
    lease.check_target(&target)?;
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let store = catdesk_wake::store::Store::open(&root)?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        store.root(),
        &target.url,
    )?;
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
            .map_err(|_| "WEB_REVIEW_REGISTRY_UNAVAILABLE")?;
    let check_pair = || {
        if paired_key(workspace, project)? != key {
            return Err("WEB_REVIEW_TARGET_CHANGED".into());
        }
        lease.check_target(&target)
    };
    let check_authority =
        |delivery: &catdesk_wake::store::Delivery,
         commit: &mut dyn FnMut() -> Result<(), String>| {
            registry.with_registration_lock(|| {
                let scope = resolve_registered_project_from_control_root(workspace, project)
                    .map_err(|_| "WEB_REVIEW_PROJECT_UNAVAILABLE")?;
                if scope.project.chatgpt_target_url.as_deref() != Some(target.url.as_str())
                    || scope.project.chatgpt_target_sha256.as_deref()
                        != Some(target.digest.as_str())
                {
                    return Err("WEB_REVIEW_TARGET_CHANGED".into());
                }
                lease.check_target(&target)?;
                crate::web_work_state::WorkLedger::open(workspace, project)?
                    .verify_review_reconciliation(&delivery.event, &target)?;
                commit()
            })
        };
    let result = if diagnostic {
        catdesk_wake::runtime::diagnose_scoped_review_guarded(
            &store,
            project,
            &target,
            event,
            &check_pair,
            check_authority,
        )
    } else {
        catdesk_wake::runtime::reconcile_scoped_review_guarded(
            &store,
            project,
            &target,
            event,
            &check_pair,
            check_authority,
        )
    }?;
    check_pair()?;
    if diagnostic {
        return Ok(
            json!({"action":"web_review_diagnose","projectId":project,"eventId":event,
            "targetGeneration":target.generation,"targetDigest":target.digest,
            "diagnosis":result,"sendAttempted":false,"stateChanged":false,"workResolved":false}),
        );
    }
    Ok(
        json!({"action":"web_review_reconcile","projectId":project,"eventId":event,
        "target":target,"reconciliation":result,"sendAttempted":false,"workResolved":false}),
    )
}

pub(super) fn reobserve(workspace: &Path, project: &str, event: &str) -> Result<Value, String> {
    let lease = crate::operator_facade::acquire_web_work_lease(workspace, project)?;
    let key = paired_key(workspace, project)?;
    let target =
        catdesk_wake::protocol::Target::new(key.target_generation, key.conversation_url.clone())?;
    lease.check_target(&target)?;
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let store = catdesk_wake::store::Store::open(&root)?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        store.root(),
        &target.url,
    )?;
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
            .map_err(|_| "WEB_REVIEW_REGISTRY_UNAVAILABLE")?;
    let check_pair = || {
        if paired_key(workspace, project)? != key {
            return Err("WEB_REVIEW_TARGET_CHANGED".into());
        }
        lease.check_target(&target)
    };
    let result = catdesk_wake::runtime::reobserve_sent_response_guarded(
        &store,
        project,
        &target,
        event,
        &check_pair,
        |commit| {
            registry.with_registration_lock(|| {
                // Binding takes this registration lock before its Wake CAS. Do not
                // reacquire the global paired-target mutex while holding it.
                let scope = resolve_registered_project_from_control_root(workspace, project)
                    .map_err(|_| "WEB_REVIEW_PROJECT_UNAVAILABLE")?;
                if scope.project.chatgpt_target_url.as_deref() != Some(target.url.as_str())
                    || scope.project.chatgpt_target_sha256.as_deref()
                        != Some(target.digest.as_str())
                {
                    return Err("WEB_REVIEW_TARGET_CHANGED".into());
                }
                lease.check_target(&target)?;
                commit()
            })
        },
    )?;
    check_pair()?;
    Ok(
        json!({"action":"web_review_reobserve","projectId":project,"eventId":event,
        "target":target,"observation":result,"sendAttempted":false,"workResolved":false}),
    )
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use catdesk_wake::{
        runtime::{SnapshotDiagnostic, SnapshotReadPhase, SnapshotReadReport},
        store::{Delivery, Phase, Receipt, Store, now},
    };

    fn sent_fixture() -> (std::path::PathBuf, Store, Delivery) {
        let (control, _, store, _, event) = crate::web_work::tests::assignment_fixture_for("alpha");
        store.publish(&event).unwrap();
        let owner = store.host_lock().unwrap();
        let delivery = store.claim(&event, &owner).unwrap();
        store
            .transition(
                &event.event_id,
                owner.owner(),
                Phase::Submitting,
                None,
                None,
            )
            .unwrap();
        let receipt = Receipt {
            schema_version: 1,
            event_id: event.event_id.clone(),
            target_generation: delivery.target.generation,
            target_digest: delivery.target.digest.clone(),
            message_digest: delivery.message_digest.clone(),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        store
            .record_exact_receipt_timer(&event.event_id, &receipt)
            .unwrap();
        store
            .transition(
                &event.event_id,
                owner.owner(),
                Phase::Sent,
                Some(receipt),
                None,
            )
            .unwrap();
        drop(owner);
        let sent = store.delivery(&event.event_id).unwrap().unwrap();
        (control, store, sent)
    }

    fn pins(control: &Path, store: &Store, event: &str) -> Vec<(std::path::PathBuf, Vec<u8>)> {
        [
            store.root().join("config.json"),
            store.root().join("control.json"),
            store.root().join(format!("deliveries/{event}.json")),
            store.root().join(format!("turns/{event}.json")),
            control.join(".catdesk/projects/projects.json"),
            control.join(".catdesk/web-work/alpha.json"),
        ]
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect()
    }

    fn report(snapshot: Option<Value>, failure: Option<&str>) -> SnapshotReadReport {
        SnapshotReadReport {
            snapshot,
            failure: failure.map(str::to_owned),
            diagnostic: SnapshotDiagnostic {
                phase: SnapshotReadPhase::TargetOpen,
                elapsed_millis: 20,
                budget_millis: 90000,
            },
        }
    }

    #[test]
    fn sent_diagnosis_uses_existing_reader_and_exports_no_response_or_url() {
        let (control, store, delivery) = sent_fixture();
        let before = pins(&control, &store, &delivery.event.event_id);
        let result = diagnose_using(
            &control,
            "alpha",
            &delivery.event.event_id,
            |_, project, target, event, guard| {
                assert_eq!(project, "alpha");
                assert_eq!(target, &delivery.target);
                assert_eq!(event, delivery.event.event_id);
                guard().unwrap();
                report(
                    Some(json!({"privateDraft":"do not export this draft", "messages":[
                        {"role":"USER","text":"earlier unrelated request","textComplete":true},
                        {"role":"ASSISTANT","text":"This response couldn't load","textComplete":true},
                        {"role":"USER","text":delivery.event.message,"textComplete":true},
                        {"role":"ASSISTANT","text":"private current answer","textComplete":true}
                    ]})),
                    None,
                )
            },
        )
        .unwrap();
        assert_eq!(result["deliveryPhase"], "SENT");
        assert_eq!(result["diagnosis"]["snapshotVerified"], true);
        assert_eq!(result["diagnosis"]["responseState"], "SNAPSHOT_VERIFIED");
        assert_eq!(result["diagnosis"]["responseBodyExported"], false);
        assert_eq!(result["stateChanged"], false);
        assert_eq!(result["workResolved"], false);
        let rendered = result.to_string();
        for private in [
            "private current answer",
            "do not export this draft",
            delivery.target.url.as_str(),
            delivery.event.message.as_str(),
        ] {
            assert!(!rendered.contains(private));
        }
        for (path, bytes) in before {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn sent_diagnosis_keeps_missing_duplicate_timeout_and_load_failures_unverified() {
        let (control, store, delivery) = sent_fixture();
        let before = pins(&control, &store, &delivery.event.event_id);
        for failure in [
            "SOURCE_REVIEW_ASSISTANT_NOT_MOUNTED",
            "SOURCE_REVIEW_DUPLICATE_USER",
            "SOURCE_SNAPSHOT_TIMEOUT",
            "LOGIN_OR_PROFILE_REQUIRED",
            "SOURCE_SNAPSHOT_TIMEOUT: private adapter output",
            "PRIVATE_TOKEN_OR_DRAFT",
        ] {
            let result = diagnose_using(
                &control,
                "alpha",
                &delivery.event.event_id,
                |_, _, _, _, _| report(Some(json!({"private":"never export"})), Some(failure)),
            )
            .unwrap();
            let diagnosis = &result["diagnosis"];
            assert_eq!(diagnosis["snapshotVerified"], false);
            assert_eq!(diagnosis["responseState"], "UNVERIFIED");
            assert_eq!(
                diagnosis["reason"],
                catdesk_wake::runtime::snapshot_diagnostic_code(failure)
            );
            assert_eq!(diagnosis["retryAuthorized"], false);
            assert!(!result.to_string().contains("private"));
            assert!(!result.to_string().contains("PRIVATE_TOKEN"));
        }
        let result = diagnose_using(
            &control,
            "alpha",
            &delivery.event.event_id,
            |_, _, _, _, _| report(Some(json!({"messages":[
                {"role":"USER","text":delivery.event.message,"textComplete":true},
                {"role":"ASSISTANT","text":"This response couldn't load","textComplete":true}
            ]})), None),
        )
        .unwrap();
        assert_eq!(
            result["diagnosis"]["reason"],
            "WEB_REVIEW_RESPONSE_LOAD_ERROR"
        );
        for (path, bytes) in before {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn sent_diagnosis_refuses_foreign_project_and_corrupt_receipt_before_browser() {
        let (control, store, delivery) = sent_fixture();
        assert!(
            diagnose_using(
                &control,
                "catdesk",
                &delivery.event.event_id,
                |_, _, _, _, _| panic!("foreign project must not open a browser")
            )
            .is_err()
        );
        let mut corrupt = delivery.clone();
        corrupt.receipt.as_mut().unwrap().target_generation += 1;
        let path = store
            .root()
            .join(format!("deliveries/{}.json", delivery.event.event_id));
        catdesk_wake::store::atomic(&path, &corrupt).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(
            diagnose_using(
                &control,
                "alpha",
                &delivery.event.event_id,
                |_, _, _, _, _| panic!("invalid receipt must not open a browser")
            )
            .is_err()
        );
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn submitting_diagnosis_never_uses_sent_reader_or_creates_receipt() {
        let (control, store, mut delivery) = sent_fixture();
        delivery.phase = Phase::Submitting;
        delivery.receipt = None;
        let path = store
            .root()
            .join(format!("deliveries/{}.json", delivery.event.event_id));
        catdesk_wake::store::atomic(&path, &delivery).unwrap();
        let before = std::fs::read(&path).unwrap();
        // This fixture has no historical dispatched review authorization. The
        // original submitting path must refuse it rather than using the new
        // receipt-based observer or inventing a successful submission.
        assert!(
            diagnose_using(
                &control,
                "alpha",
                &delivery.event.event_id,
                |_, _, _, _, _| panic!("unknown submission cannot use sent observer")
            )
            .is_err()
        );
        assert_eq!(std::fs::read(path).unwrap(), before);
        assert!(
            store
                .delivery(&delivery.event.event_id)
                .unwrap()
                .unwrap()
                .receipt
                .is_none()
        );
    }
}
