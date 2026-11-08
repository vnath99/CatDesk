//! One existing ordinary work invitation through the stopped-host sender.
//! The request journal selects the event; no caller message or queue scan.
use super::*;
use catdesk_wake::{
    protocol::{Event, Target},
    runtime,
    store::{Store, read},
};

fn validate_selection(
    project: &str,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    event: &Event,
) -> Result<(), String> {
    event.validate()?;
    let expected_id = format!(
        "web-work-{}",
        protocol::digest(&format!("{}\0{}", project, record.request_id))
    );
    if record.phase != "OPEN"
        || record.target != *target
        || record.event_id != expected_id
        || event.event_id != expected_id
        || event.project_id != project
        || event.event_type != "engineering_attention"
        || event.target_generation != target.generation
        || !event.audit_references.is_empty()
        || protocol::digest(&event.message) != record.message_sha256
    {
        return Err("WEB_WORK_DELIVERY_SELECTION_CHANGED".into());
    }
    Ok(())
}

pub(crate) fn deliver(workspace: &Path, project: &str, request_id: &str) -> Result<Value, String> {
    selected_work(workspace, project, request_id, false)
}

pub(crate) fn inspect(workspace: &Path, project: &str, request_id: &str) -> Result<Value, String> {
    selected_work(workspace, project, request_id, true)
}

fn selected_work(
    workspace: &Path,
    project: &str,
    request_id: &str,
    inspect_only: bool,
) -> Result<Value, String> {
    if inspect_only
        && wake_protocol_client::wake_runtime_mode_for_workspace(workspace)? != "INSTALLED_STORE"
    {
        return Err("WEB_BROWSER_INSTALLED_SCOPE_REQUIRED".into());
    }
    let prepared = PreparedProjectScopeV1::resolve(workspace, project)
        .map_err(|_| "WEB_WORK_PROJECT_UNAVAILABLE")?;
    let lease = crate::operator_facade::acquire_web_work_lease(workspace, project)?;
    let ledger = WorkLedger::open(workspace, project)?;
    ledger.require_ordinary_request(request_id)?;
    let record = ledger
        .record(request_id)?
        .ok_or("WEB_WORK_REQUEST_MISSING")?;
    let paired = mcp::operator_read_project_web_target(workspace, project)
        .map_err(|_| "WEB_WORK_PAIRED_TARGET_REQUIRED")?
        .ok_or("WEB_WORK_PAIRED_TARGET_REQUIRED")?;
    let target = Target::new(paired.generation, paired.url)?;
    lease.check_target(&target)?;
    let wake = Store::open(&wake_protocol_client::wake_root_for_workspace(workspace)?)?;
    // SENT, SUBMITTING and other historical outcomes are refused by the
    // existing sender before opening an adapter; they never grant replay.
    let prior = wake.delivery(&record.event_id)?;
    if inspect_only && prior.is_none() {
        return Err("WEB_WORK_DELIVERY_MISSING".into());
    }
    let event = match &prior {
        Some(delivery) => delivery.event.clone(),
        None => read(
            &wake
                .root()
                .join("queue")
                .join(format!("{}.json", record.event_id)),
        )?,
    };
    validate_selection(project, &record, &target, &event)?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &target.url,
    )?;
    let guard = || {
        lease.check_target(&target)?;
        // Keep this metadata-only admission atomic with the existing paired
        // target/registration lock order. Never retain shared locks while the
        // adapter observes, types, submits, or drains its receipt.
        mcp::with_current_project_web_target(
            workspace,
            project,
            target.generation,
            &target.digest,
            |_, current| {
                prepared
                    .revalidate_registration()
                    .map_err(|_| "WEB_WORK_PROJECT_REGISTRATION_CHANGED")?;
                if current.url != target.url {
                    return Err("WEB_WORK_TARGET_CHANGED".into());
                }
                ledger.require_ordinary_request(request_id)?;
                if ledger.record(request_id)?.as_ref() != Some(&record) {
                    return Err("WEB_WORK_DELIVERY_SELECTION_CHANGED".into());
                }
                Ok(())
            },
        )
    };
    guard()?;
    if inspect_only {
        let mut result = runtime::inspect_registered_delivery_readiness(
            &wake,
            project,
            &target,
            prior.as_ref().ok_or("WEB_WORK_DELIVERY_MISSING")?,
            &guard,
        )?;
        result["requestId"] = json!(request_id);
        return Ok(result);
    }
    let mut before_submit = |actual: &Event, actual_target: &Target| {
        guard()?;
        if actual != &event || actual_target != &target {
            return Err("WEB_WORK_DELIVERY_SELECTION_CHANGED".into());
        }
        validate_selection(project, &record, actual_target, actual)
    };
    runtime::run_scoped_work_request(&wake, &event, &target, 120, &mut before_submit, &guard)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn work_inspection_preserves_development_isolation_without_browser_or_queue_changes() {
        let (control, _, wake, _, event) = crate::web_work::tests::assignment_fixture_for("alpha");
        wake.publish(&event).unwrap();
        let path = wake
            .root()
            .join("queue")
            .join(format!("{}.json", event.event_id));
        let before = std::fs::read(&path).unwrap();
        assert_eq!(
            inspect(&control, "alpha", "request-a").unwrap_err(),
            "WEB_BROWSER_INSTALLED_SCOPE_REQUIRED"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(wake.delivery(&event.event_id).unwrap().is_none());
    }

    #[test]
    fn scoped_work_delivery_replay_refusal_preserves_real_request_and_receipt_files() {
        use catdesk_wake::store::{Phase, Receipt};
        for phase in [Phase::Submitting, Phase::Sent] {
            let (control, _, wake, _, event) =
                crate::web_work::tests::assignment_fixture_for("alpha");
            wake.publish(&event).unwrap();
            let host = wake.host_lock().unwrap();
            let claimed = wake.claim(&event, &host).unwrap();
            wake.transition(
                &event.event_id,
                &claimed.owner,
                Phase::Submitting,
                None,
                None,
            )
            .unwrap();
            if phase == Phase::Sent {
                let target = wake.config().unwrap().targets["alpha"].clone();
                let receipt = Receipt {
                    schema_version: 1,
                    event_id: event.event_id.clone(),
                    target_generation: target.generation,
                    target_digest: target.digest,
                    message_digest: protocol::digest(&event.message),
                    sent_utc: now(),
                    evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
                };
                // Synthetic evidence is confined to this disposable fixture.
                wake.record_exact_receipt_timer(&event.event_id, &receipt)
                    .unwrap();
                wake.transition(
                    &event.event_id,
                    &claimed.owner,
                    Phase::Sent,
                    Some(receipt),
                    None,
                )
                .unwrap();
            }
            drop(host);
            let files = [
                control.join(".catdesk/projects/projects.json"),
                control.join(".catdesk/web-work/alpha.json"),
                wake.root()
                    .join(format!("deliveries/{}.json", event.event_id)),
            ];
            let before: Vec<_> = files
                .iter()
                .map(std::fs::read)
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(
                deliver(&control, "alpha", "request-a").unwrap_err(),
                "SCOPED_REVIEW_REPLAY_REFUSED"
            );
            for (file, bytes) in files.iter().zip(before) {
                assert_eq!(std::fs::read(file).unwrap(), bytes);
            }
            assert_eq!(
                wake.delivery(&event.event_id).unwrap().unwrap().phase,
                phase
            );
        }
    }

    #[test]
    fn scoped_work_delivery_busy_registration_refuses_before_claim_or_browser() {
        let (control, _, wake, _, event) = crate::web_work::tests::assignment_fixture_for("alpha");
        wake.publish(&event).unwrap();
        let registry =
            AutonomousProjectRegistryStoreV1::open_read_only(control.join(".catdesk/projects"))
                .unwrap();
        let queue = wake.root().join(format!("queue/{}.json", event.event_id));
        let before = std::fs::read(&queue).unwrap();
        registry
            .with_registration_lock(|| {
                assert_eq!(
                    deliver(&control, "alpha", "request-a").unwrap_err(),
                    "WEB_CONTEXT_REGISTRATION_BUSY"
                );
                Ok(())
            })
            .unwrap();
        assert!(wake.delivery(&event.event_id).unwrap().is_none());
        assert_eq!(std::fs::read(&queue).unwrap(), before);
    }

    #[test]
    fn scoped_work_delivery_requires_exact_open_ordinary_request() {
        let (control, _, wake, _, event) = crate::web_work::tests::assignment_fixture_for("alpha");
        let record = WorkLedger::open(&control, "alpha")
            .unwrap()
            .record("request-a")
            .unwrap()
            .unwrap();
        let target = wake.config().unwrap().targets["alpha"].clone();
        validate_selection("alpha", &record, &target, &event).unwrap();
        for change in [
            "project",
            "request",
            "event",
            "closed",
            "generation",
            "message",
            "role",
            "audit",
            "target",
        ] {
            let mut record = record.clone();
            let mut event = event.clone();
            let mut target = target.clone();
            match change {
                "project" => event.project_id = "beta".into(),
                "request" => record.request_id = "another".into(),
                "event" => event.event_id = "web-work-other".into(),
                "closed" => record.phase = "CLOSED".into(),
                "generation" => event.target_generation += 1,
                "message" => event.message.push_str(" changed"),
                "role" => event.event_type = "source_review".into(),
                "audit" => event
                    .audit_references
                    .push("catdesk-review-v1:grant".into()),
                "target" => target.generation += 1,
                _ => unreachable!(),
            }
            assert!(
                validate_selection("alpha", &record, &target, &event).is_err(),
                "{change}"
            );
        }
    }
}
