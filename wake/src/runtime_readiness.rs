//! Observation through the existing stopped-host owner. Passive modes have no
//! UI action; explicit AppMenu permits only one linked-menu open/close, never
//! event claims, USER input, item selection or delivery reconciliation.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Advisory,
    Receipt,
    ResponseOwnerHeld,
    SentDiagnostics,
    HistoricalDiagnostics,
    AppMenu,
    Trace,
}

/// Passive exact older exchange only. Never applies a timer, sends or retries.
pub fn inspect_historical_task_response(
    store: &Store,
    project: &str,
    target: &Target,
    event: &str,
    keep_current: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    let (delivery, timer) = store.sent_response_scope(project, target, event)?;
    if timer.response_state != ResponseState::Complete {
        return Err("READINESS_COMPLETED_RESPONSE_REQUIRED".into());
    }
    inspect_readiness(
        store,
        project,
        target,
        Some(&delivery),
        Mode::HistoricalDiagnostics,
        keep_current,
    )
}

pub fn inspect_sent_task_trace(
    store: &Store,
    project: &str,
    target: &Target,
    event: &str,
    keep_current: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    let (delivery, timer) = store.sent_response_scope(project, target, event)?;
    if timer.response_state != ResponseState::Complete {
        return Err("READINESS_COMPLETED_RESPONSE_REQUIRED".into());
    }
    inspect_readiness(
        store,
        project,
        target,
        Some(&delivery),
        Mode::Trace,
        keep_current,
    )
}

#[cfg(any(windows, test))]
fn fence_trace_attempt(store: &Store, delivery: &crate::store::Delivery, hash: &str) -> Result<()> {
    let logs = store.root().join("logs");
    require_plain_install_directory(&logs)?;
    let path = logs.join(format!(
        "response-trace-{}.attempt.json",
        delivery.event.event_id
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                "READINESS_TRACE_ALREADY_RECORDED"
            } else {
                "READINESS_TRACE_FENCE_FAILED"
            }
        })?;
    let bytes=serde_json::to_vec(&serde_json::json!({"schemaVersion":1,"purpose":"ONE_TASK_BOUND_TRACE_OPEN",
        "projectId":delivery.event.project_id,"eventId":delivery.event.event_id,"targetGeneration":delivery.target.generation,
        "targetDigest":delivery.target.digest,"messageDigest":delivery.message_digest,"sourceDeliverySha256":hash,
        "controllerPid":std::process::id(),"recordedUtc":now(),"maximumClicks":1,"sendAllowed":false,"phase":"ATTEMPT_FENCED"})).map_err(|_|"READINESS_TRACE_FENCE_FAILED")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "READINESS_TRACE_FENCE_FAILED".into())
}

/// Explicit transient menu inspection of one completed receipt-bound task.
/// No menu item is selected and no input, settings or authority is changed.
pub fn inspect_sent_task_app_menu(
    store: &Store,
    project: &str,
    target: &Target,
    event: &str,
    keep_current: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    let (delivery, timer) = store.sent_response_scope(project, target, event)?;
    if timer.response_state != ResponseState::Complete {
        return Err("READINESS_COMPLETED_RESPONSE_REQUIRED".into());
    }
    inspect_readiness(
        store,
        project,
        target,
        Some(&delivery),
        Mode::AppMenu,
        keep_current,
    )
}

/// Explicit content-free inspection of an exact SENT ordinary/task exchange.
/// This never creates review/producer authority or changes a delivery/timer.
pub fn inspect_sent_task_response_diagnostics(
    store: &Store,
    project: &str,
    target: &Target,
    event: &str,
    keep_current: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    let (delivery, _) = store.sent_response_scope(project, target, event)?;
    inspect_readiness(
        store,
        project,
        target,
        Some(&delivery),
        Mode::SentDiagnostics,
        keep_current,
    )
}

#[cfg(any(windows, test))]
fn verified_primitives(source: &Path, adjacent: &Path, expected: &str) -> Result<PathBuf> {
    let path = match std::fs::symlink_metadata(adjacent) {
        Ok(_) => {
            require_plain_install_directory(adjacent)?;
            adjacent.join("wake_bridge.py")
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => source.to_path_buf(),
        Err(_) => return Err("READINESS_SOURCE_UNAVAILABLE".into()),
    };
    require_plain_install_file(&path)?;
    if sha256_file(&path, "READINESS_SOURCE_UNAVAILABLE")? != expected {
        return Err("READINESS_SOURCE_HASH_MISMATCH".into());
    }
    Ok(path)
}

#[cfg(any(windows, test))]
fn require_selection(store: &Store, project: &str, target: &Target) -> Result<()> {
    target.validate()?;
    let control: Control = read(&store.root().join("control.json"))?;
    if control.schema_version != 1 || control.desired != "STOPPED" {
        return Err("READINESS_REQUIRES_STOPPED_WAKE".into());
    }
    if store.config()?.targets.get(project) != Some(target) {
        return Err("READINESS_REGISTERED_TARGET_CHANGED".into());
    }
    Ok(())
}

/// A caller must hold its project lease and recheck the paired registration.
/// Only the shared browser portion is serialized; no metadata lock spans it.
pub fn inspect_registered_readiness(
    store: &Store,
    project: &str,
    target: &Target,
    keep_current: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    inspect_readiness(store, project, target, None, Mode::Advisory, keep_current)
}

pub fn inspect_registered_delivery_readiness(
    store: &Store,
    project: &str,
    target: &Target,
    delivery: &crate::store::Delivery,
    keep_current: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    inspect_readiness(
        store,
        project,
        target,
        Some(delivery),
        Mode::Advisory,
        keep_current,
    )
}

/// Explicit fresh-document receipt proof through the same bounded owner.
/// This never submits, retries an assistant response, or mutates the journal.
pub fn read_registered_delivery_receipt(
    store: &Store,
    project: &str,
    target: &Target,
    delivery: &crate::store::Delivery,
    keep_current: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    require_receipt_selection(store, delivery)?;
    inspect_readiness(
        store,
        project,
        target,
        Some(delivery),
        Mode::Receipt,
        keep_current,
    )
}

/// Only the existing reobservation owner calls this while retaining host ownership.
pub(super) fn read_sent_response_observation(
    store: &Store,
    project: &str,
    target: &Target,
    delivery: &crate::store::Delivery,
    keep_current: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    let observed = inspect_readiness(
        store,
        project,
        target,
        Some(delivery),
        Mode::ResponseOwnerHeld,
        keep_current,
    )?;
    if let Some(value) = observed.get("responseObservation").filter(|v| !v.is_null()) {
        return Ok(value.clone());
    }
    let reason = observed["observations"].as_array().and_then(|rows| {
        rows.iter()
            .rev()
            .find_map(|row| row.as_str()?.strip_prefix("PROBE:RESPONSE_STATE:"))
    });
    Err(reason.unwrap_or("RESPONSE_PROBE_UNPROVEN").into())
}

fn require_receipt_selection(store: &Store, delivery: &crate::store::Delivery) -> Result<()> {
    delivery.event.validate()?;
    if delivery.phase != Phase::Submitting
        || delivery.receipt.is_some()
        || !store
            .root()
            .join("logs")
            .join(format!("submission-start-{}.json", delivery.event.event_id))
            .is_file()
    {
        return Err("READINESS_SUBMISSION_WITNESS_REQUIRED".into());
    }
    store.submission_started_utc(delivery)?;
    Ok(())
}

#[cfg(any(windows, test))]
fn require_delivery(
    store: &Store,
    project: &str,
    target: &Target,
    delivery: &crate::store::Delivery,
    response_only: bool,
) -> Result<()> {
    delivery.event.validate()?;
    let phase_valid = if response_only {
        store
            .sent_response_scope(project, target, &delivery.event.event_id)?
            .0
            == *delivery
    } else {
        matches!(
            delivery.phase,
            Phase::Claimed | Phase::Submitting | Phase::Attention
        ) && delivery.receipt.is_none()
    };
    let ordinary = delivery.event.audit_references.is_empty()
        && delivery
            .event
            .event_id
            .strip_prefix("web-work-")
            .is_some_and(exact_hash);
    let continuation = delivery
        .event
        .event_id
        .strip_prefix("wake-intent-")
        .is_some_and(exact_hash)
        && delivery.event.wake_metadata()?.is_some_and(|metadata| {
            metadata.purpose == crate::protocol::WakePurpose::NewTask
                && metadata
                    .source_event_id
                    .strip_prefix("web-work-")
                    .is_some_and(exact_hash)
                && metadata
                    .source_turn_id
                    .strip_prefix("initial-context-")
                    .is_some_and(exact_hash)
                && delivery.event.audit_references.len() == 5
        });
    if !phase_valid
        || !matches!(
            delivery.phase,
            Phase::Claimed | Phase::Submitting | Phase::Attention | Phase::Sent
        )
        || delivery.target != *target
        || delivery.event.project_id != project
        || delivery.event.target_generation != target.generation
        || delivery.event.event_type != "engineering_attention"
        || !(ordinary || continuation)
        || delivery.message_digest != crate::protocol::digest(&delivery.event.message)
        || store.delivery(&delivery.event.event_id)?.as_ref() != Some(delivery)
    {
        return Err("READINESS_WORK_DELIVERY_CHANGED".into());
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn exact_hash(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(windows)]
fn inspect_readiness(
    store: &Store,
    project: &str,
    target: &Target,
    delivery: Option<&crate::store::Delivery>,
    mode: Mode,
    keep_current: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    let receipt_only = mode == Mode::Receipt;
    let response_only = matches!(
        mode,
        Mode::ResponseOwnerHeld
            | Mode::SentDiagnostics
            | Mode::AppMenu
            | Mode::Trace
            | Mode::HistoricalDiagnostics
    );
    if !crate::protocol::valid_id(project) {
        return Err("READINESS_PROJECT_INVALID".into());
    }
    require_selection(store, project, target)?;
    keep_current()?;
    let _host = if mode == Mode::ResponseOwnerHeld {
        None
    } else {
        Some(store.host_lock()?)
    };
    let _browser = acquire_browser_quiescence(store)?;
    require_selection(store, project, target)?;
    keep_current()?;
    if let Some(delivery) = delivery {
        require_delivery(store, project, target, delivery, response_only)?;
        if receipt_only {
            require_receipt_selection(store, delivery)?;
        }
    }
    // Verify installed identity without requiring new-chat/bootstrap support.
    installed_browser_directory(store)?;
    require_plain_install_directory(&store.root().join("runtime"))?;
    let python = crate::development_runtime::browser_child_path(
        &store.root().join("runtime/python.exe"),
        false,
    )?;
    let profile = crate::development_runtime::browser_child_path(
        &store.root().join("browser-profile"),
        true,
    )?;
    // A separately qualified controller can retain its exact observer artifact
    // beside the image, so later source edits cannot invalidate an operator's
    // pending read-only command. Both locations require the compiled digest;
    // a present bad adjacent artifact never falls back to mutable source.
    let source_primitives = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("READINESS_SOURCE_UNAVAILABLE")?
        .join("scripts/wake_bridge.py");
    let adjacent = std::env::current_exe()
        .map_err(|_| "READINESS_SOURCE_UNAVAILABLE")?
        .parent()
        .ok_or("READINESS_SOURCE_UNAVAILABLE")?
        .join("browser-observer");
    let expected = format!(
        "{:x}",
        Sha256::digest(include_bytes!("../../scripts/wake_bridge.py"))
    );
    let primitives = verified_primitives(&source_primitives, &adjacent, &expected)?;
    let mut command = Command::new(python);
    command
        .arg("-I")
        .arg("-c")
        .arg(include_str!("../../scripts/wake_readiness_visual_probe.py"))
        .arg("--await-start")
        .arg("--profile")
        .arg(profile)
        .arg("--conversation-url")
        .arg(&target.url)
        .arg("--primitives")
        .arg(primitives)
        .arg("--primitives-sha256")
        .arg(expected)
        .arg("--read-only")
        .arg("--seconds")
        .arg("20")
        .current_dir(store.root());
    let delivery_hash = if let Some(delivery) = delivery {
        let path = store
            .root()
            .join("deliveries")
            .join(format!("{}.json", delivery.event.event_id));
        require_plain_install_file(&path)?;
        let hash = sha256_file(&path, "READINESS_WORK_DELIVERY_CHANGED")?;
        command
            .arg("--delivery-file")
            .arg(path)
            .arg("--delivery-sha256")
            .arg(&hash);
        if receipt_only {
            command.arg("--receipt-only");
        }
        if response_only {
            command.arg("--response-only");
        }
        // Retained-owner reobservation admits the same receipt-bound task
        // continuations as explicit diagnostics. Both are content-free and
        // require the Python reader's matching semantic-response flag.
        if matches!(
            mode,
            Mode::SentDiagnostics
                | Mode::ResponseOwnerHeld
                | Mode::AppMenu
                | Mode::Trace
                | Mode::HistoricalDiagnostics
        ) {
            command.arg("--task-diagnostics");
        }
        if mode == Mode::AppMenu {
            command.arg("--inspect-app-menu");
        }
        if mode == Mode::Trace {
            command.arg("--inspect-response-trace");
        }
        if mode == Mode::HistoricalDiagnostics {
            command.arg("--historical-response");
        }
        Some(hash)
    } else {
        None
    };
    let mut rows = Vec::new();
    if mode == Mode::Trace {
        keep_current()?;
        fence_trace_attempt(
            store,
            delivery.ok_or("READINESS_WORK_DELIVERY_CHANGED")?,
            delivery_hash
                .as_deref()
                .ok_or("READINESS_WORK_DELIVERY_CHANGED")?,
        )?;
    }
    let result = crate::readiness_probe::run_bounded_observed(
        &mut command,
        Duration::from_secs(90),
        &mut rows,
    );
    // These errors positively follow Job-empty and output-reader exit. Other
    // failures retain their precise native error without claiming cleanup.
    if let Err(reason) = result
        && !matches!(
            reason,
            "PROBE_PROCESS_TIMEOUT"
                | "PROBE_FAILED"
                | "PROBE_OUTPUT_FAILED"
                | "PROBE_OUTPUT_TOO_LARGE"
                | "PROBE_OUTPUT_INVALID"
        )
    {
        return Err(reason.into());
    }
    require_selection(store, project, target)?;
    keep_current()?;
    if let Some(delivery) = delivery {
        require_delivery(store, project, target, delivery, response_only)?;
        let path = store
            .root()
            .join("deliveries")
            .join(format!("{}.json", delivery.event.event_id));
        if Some(sha256_file(&path, "READINESS_WORK_DELIVERY_CHANGED")?) != delivery_hash {
            return Err("READINESS_WORK_DELIVERY_CHANGED".into());
        }
    }
    let final_state = rows
        .iter()
        .rev()
        .find_map(|row| row.strip_prefix("PROBE:FINAL:"));
    let phase = rows
        .iter()
        .rev()
        .find_map(|row| row.strip_prefix("PROBE:PHASE:"));
    let surface = rows
        .iter()
        .rev()
        .find_map(|row| crate::readiness_probe::surface_observation(row));
    let tabs = rows
        .iter()
        .rev()
        .find_map(|row| crate::readiness_probe::tab_observation(row));
    let submission = rows
        .iter()
        .rev()
        .find_map(|row| crate::readiness_probe::submission_observation(row));
    let submission_state = rows
        .iter()
        .rev()
        .find_map(|row| row.strip_prefix("PROBE:SUBMISSION_STATE:"));
    let receipts: Vec<_> = rows
        .iter()
        .filter_map(|row| crate::readiness_probe::persisted_receipt(row))
        .collect();
    let receipt = if receipt_only
        && result.is_ok()
        && phase == Some("BROWSER_CLOSED")
        && final_state.is_some()
        && !rows.iter().any(|row| row == "PROBE:RECEIPT_STATE:UNPROVEN")
        && tabs
            .as_ref()
            .is_some_and(|v| v["state"] == "TARGET_TAB_EXACT" && v["candidateFrame"] == "TOP")
        && surface
            .as_ref()
            .is_some_and(|v| v["route"] == "EXACT" && v["document"] == "COMPLETE")
        && receipts.len() == 1
    {
        let receipt = &receipts[0];
        let d = delivery.ok_or("READINESS_WORK_DELIVERY_CHANGED")?;
        if receipt.event_id != d.event.event_id
            || receipt.target_generation != target.generation
            || receipt.target_digest != target.digest
            || receipt.message_digest != d.message_digest
            || receipt.sent_utc < store.submission_started_utc(d)?
            || receipt.sent_utc > now()
        {
            return Err("READINESS_RECEIPT_BINDING_INVALID".into());
        }
        Some(receipt)
    } else {
        None
    };
    let response = if response_only
        && result.is_ok()
        && phase == Some("BROWSER_CLOSED")
        && final_state.is_some()
        && tabs
            .as_ref()
            .is_some_and(|v| v["state"] == "TARGET_TAB_EXACT" && v["candidateFrame"] == "TOP")
        && surface
            .as_ref()
            .is_some_and(|v| v["route"] == "EXACT" && v["document"] == "COMPLETE")
        && !rows
            .iter()
            .any(|row| row.starts_with("PROBE:RESPONSE_STATE:"))
    {
        let values: Vec<_> = rows
            .iter()
            .filter_map(|row| {
                if mode == Mode::HistoricalDiagnostics {
                    crate::readiness_probe::historical_response_observation(row)
                } else {
                    crate::readiness_probe::sent_response_observation(row)
                }
            })
            .collect();
        if values.len() == 1 {
            response::validate_observation(
                delivery.ok_or("READINESS_WORK_DELIVERY_CHANGED")?,
                &values[0],
            )?;
            Some(values[0].clone())
        } else {
            None
        }
    } else {
        None
    };
    let diagnostics: Vec<_> = rows
        .iter()
        .filter_map(|row| crate::readiness_probe::task_response_diagnostics(row))
        .collect();
    let app_observation = rows
        .iter()
        .rev()
        .find_map(|row| crate::readiness_probe::composer_app_observation(row));
    let task_diagnostics = if matches!(
        mode,
        Mode::SentDiagnostics | Mode::AppMenu | Mode::Trace | Mode::HistoricalDiagnostics
    ) && response
        .as_ref()
        .is_some_and(|v| v["stage"] == "RESPONSE_COMPLETE")
        && diagnostics.len() == 1
    {
        Some(&diagnostics[0])
    } else {
        None
    };
    let trace = rows
        .iter()
        .rev()
        .find_map(|row| crate::readiness_probe::trace_observation(row));
    let trace_failure = rows
        .iter()
        .rev()
        .find_map(|row| crate::readiness_probe::trace_failure(row));
    if mode == Mode::Trace {
        let path = store.root().join("logs").join(format!(
            "response-trace-{}.outcome.json",
            delivery
                .ok_or("READINESS_WORK_DELIVERY_CHANGED")?
                .event
                .event_id
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| "READINESS_TRACE_OUTCOME_FAILED")?;
        let bytes = serde_json::to_vec(
            &serde_json::json!({"schemaVersion":1,"ownedTreeExited":true,"traceObservation":trace,"traceFailure":trace_failure,
            "phase":phase,"finalState":final_state,"recordedUtc":now(),"sendAttempted":false}),
        )
        .map_err(|_| "READINESS_TRACE_OUTCOME_FAILED")?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "READINESS_TRACE_OUTCOME_FAILED")?;
    }
    Ok(
        serde_json::json!({"action":if delivery.is_some() {"web_work_inspect"} else {"web_browser_inspect"},"projectId":project,
        "status":if result.is_ok() && final_state.is_some() {"BROWSER_READINESS_OBSERVED"} else {"BROWSER_OBSERVATION_INCOMPLETE"},
        "failureReason":result.err(),"lastPhase":phase,"finalState":final_state,
        "browserObservation":surface,"tabObservation":tabs,"observations":rows,
        "targetGeneration":target.generation,"targetDigest":target.digest,
        "eventId":delivery.map(|d| &d.event.event_id),"sourceDeliverySha256":delivery_hash,
        "submissionObservation":submission,"submissionObservationState":submission_state,
        "advisory":!receipt_only,"browserReadOnly":!matches!(mode,Mode::AppMenu|Mode::Trace),"transientMenuInspection":mode==Mode::AppMenu,"transientTraceInspection":mode==Mode::Trace,"diagnosticJournalChanged":mode==Mode::Trace,"sendAttempted":false,
        "receiptCreated":false,"stateChanged":false,"retryAuthorized":false,"workResolved":false,
        "persistedReceipt":receipt,"receiptProofRequested":receipt_only,
        "responseObservation":if mode==Mode::HistoricalDiagnostics {None} else {response.clone()},
        "historicalResponseObservation":if mode==Mode::HistoricalDiagnostics {response} else {None},
        "historicalReadOnly":mode==Mode::HistoricalDiagnostics,
        "taskDiagnostics":task_diagnostics,"appPresentation":app_observation,"traceObservation":trace,"traceFailure":trace_failure,
        "appMenuObservation":rows.iter().rev().find_map(|row|crate::readiness_probe::app_menu_observation(row)),"reviewAuthorityGranted":false,
        "submissionOutcome":if receipt.is_some() {"PERSISTED_RECEIPT_PROVEN"} else {"NOT_ASSESSED"},"authenticationVerified":false,
        "ownedTreeExited":true,"hostStarted":false,"schedulerStarted":false}),
    )
}

#[cfg(not(windows))]
fn inspect_readiness(
    _: &Store,
    _: &str,
    _: &Target,
    _: Option<&crate::store::Delivery>,
    _: Mode,
    _: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    Err("READINESS_PLATFORM_UNSUPPORTED".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sent_continuation_inspection_requires_exact_semantic_receipt_without_retry_or_mutation() {
        let (store, lease) = super::super::scheduler::tests::fixture("sent-continuation-inspect");
        let target = store.config().unwrap().targets["alpha"].clone();
        let event = store
            .produce_expected_wake(
                "alpha",
                "Continue exact context",
                &target,
                crate::store::WakeIntent {
                    purpose: crate::protocol::WakePurpose::NewTask,
                    source_event_id: &format!("web-work-{}", "a".repeat(64)),
                    source_turn_id: &format!("initial-context-{}", "b".repeat(64)),
                    priority: 0,
                    ttl_seconds: 600,
                },
            )
            .unwrap();
        let claimed = store.claim(&event, &lease).unwrap();
        assert!(require_delivery(&store, "alpha", &target, &claimed, true).is_err());
        require_delivery(&store, "alpha", &target, &claimed, false).unwrap();
        let submitting = store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Submitting,
                None,
                None,
            )
            .unwrap();
        require_delivery(&store, "alpha", &target, &submitting, false).unwrap();
        assert!(require_delivery(&store, "beta", &target, &submitting, false).is_err());
        let receipt = Receipt {
            schema_version: 1,
            event_id: event.event_id.clone(),
            target_generation: target.generation,
            target_digest: target.digest.clone(),
            message_digest: crate::protocol::digest(&event.message),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        store
            .record_exact_receipt_timer(&event.event_id, &receipt)
            .unwrap();
        let sent = store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Sent,
                Some(receipt),
                None,
            )
            .unwrap();
        let path = store
            .root()
            .join("deliveries")
            .join(format!("{}.json", event.event_id));
        let before = std::fs::read(&path).unwrap();
        require_delivery(&store, "alpha", &target, &sent, true).unwrap();
        assert_eq!(
            inspect_sent_task_app_menu(&store, "alpha", &target, &event.event_id, &|| Ok(()))
                .unwrap_err(),
            "READINESS_COMPLETED_RESPONSE_REQUIRED"
        );
        assert!(require_delivery(&store, "alpha", &target, &sent, false).is_err());
        assert!(require_delivery(&store, "beta", &target, &sent, true).is_err());
        let mut unknown = sent.clone();
        unknown.phase = Phase::Submitting;
        unknown.receipt = None;
        assert!(require_delivery(&store, "alpha", &target, &unknown, true).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(
            inspect_sent_task_trace(&store, "alpha", &target, &event.event_id, &|| Ok(()))
                .unwrap_err(),
            "READINESS_COMPLETED_RESPONSE_REQUIRED"
        );
        assert_eq!(
            inspect_historical_task_response(&store, "alpha", &target, &event.event_id, &|| Ok(()))
                .unwrap_err(),
            "READINESS_COMPLETED_RESPONSE_REQUIRED"
        );
        store.complete_timer(&event.event_id).unwrap();
        let hash = sha256_file(&path, "fixture").unwrap();
        fence_trace_attempt(&store, &sent, &hash).unwrap();
        let fence = store
            .root()
            .join("logs")
            .join(format!("response-trace-{}.attempt.json", event.event_id));
        let witness = std::fs::read(&fence).unwrap();
        assert_eq!(
            fence_trace_attempt(&store, &sent, &"f".repeat(64)).unwrap_err(),
            "READINESS_TRACE_ALREADY_RECORDED"
        );
        assert_eq!(std::fs::read(&fence).unwrap(), witness);
        assert!(!String::from_utf8(witness).unwrap().contains(&event.message));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn sent_delivery_requires_explicit_receipt_bound_response_scope() {
        let (store, lease) = super::super::scheduler::tests::fixture("ordinary-response-scope");
        drop(lease);
        let target = store.config().unwrap().targets["alpha"].clone();
        let event = store
            .produce_expected_with_audit(
                &format!("web-work-{}", "a".repeat(64)),
                "alpha",
                "engineering_attention",
                "ordinary response",
                &target,
                &[],
            )
            .unwrap();
        let host = store.host_lock().unwrap();
        let claimed = store.claim(&event, &host).unwrap();
        store
            .transition(
                &event.event_id,
                &claimed.owner,
                Phase::Submitting,
                None,
                None,
            )
            .unwrap();
        let receipt = Receipt {
            schema_version: 1,
            event_id: event.event_id.clone(),
            target_generation: target.generation,
            target_digest: target.digest.clone(),
            message_digest: crate::protocol::digest(&event.message),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        store
            .record_exact_receipt_timer(&event.event_id, &receipt)
            .unwrap();
        let sent = store
            .transition(
                &event.event_id,
                &claimed.owner,
                Phase::Sent,
                Some(receipt),
                None,
            )
            .unwrap();
        drop(host);
        require_delivery(&store, "alpha", &target, &sent, true).unwrap();
        assert!(require_delivery(&store, "alpha", &target, &sent, false).is_err());
        assert!(require_delivery(&store, "beta", &target, &sent, true).is_err());
        let mut foreign = sent.clone();
        foreign.message_digest = "b".repeat(64);
        assert!(require_delivery(&store, "alpha", &target, &foreign, true).is_err());
    }

    #[test]
    fn packaged_observer_survives_source_edits_but_bad_adjacent_never_falls_back() {
        let root = std::env::temp_dir().join(format!(
            "readiness-artifact-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let source = root.join("source.py");
        let adjacent = root.join("browser-observer");
        std::fs::write(&source, b"reviewed").unwrap();
        let expected = format!("{:x}", Sha256::digest(b"reviewed"));
        assert_eq!(
            verified_primitives(&source, &adjacent, &expected).unwrap(),
            source
        );
        std::fs::create_dir(&adjacent).unwrap();
        let retained = adjacent.join("wake_bridge.py");
        std::fs::write(&retained, b"reviewed").unwrap();
        std::fs::write(&source, b"later source revision").unwrap();
        assert_eq!(
            verified_primitives(&source, &adjacent, &expected).unwrap(),
            retained
        );
        std::fs::write(&source, b"reviewed").unwrap();
        std::fs::write(&retained, b"unreviewed").unwrap();
        assert_eq!(
            verified_primitives(&source, &adjacent, &expected).unwrap_err(),
            "READINESS_SOURCE_HASH_MISMATCH"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ambiguous_ordinary_inspection_preserves_delivery_and_refuses_changed_identity() {
        for phase in [Phase::Claimed, Phase::Submitting, Phase::Attention] {
            let (store, lease) = super::super::scheduler::tests::fixture("ordinary-inspection");
            drop(lease);
            atomic(
                &store.root().join("control.json"),
                &Control {
                    schema_version: 1,
                    desired: "STOPPED".into(),
                },
            )
            .unwrap();
            let target = store.config().unwrap().targets["alpha"].clone();
            let event = store
                .produce_expected_with_audit(
                    &format!("web-work-{}", "a".repeat(64)),
                    "alpha",
                    "engineering_attention",
                    "ordinary invitation",
                    &target,
                    &[],
                )
                .unwrap();
            let host = store.host_lock().unwrap();
            let claimed = store.claim(&event, &host).unwrap();
            let delivery = if phase == Phase::Claimed {
                claimed.clone()
            } else {
                store
                    .transition(
                        &event.event_id,
                        &claimed.owner,
                        phase,
                        None,
                        Some("retained-uncertainty".into()),
                    )
                    .unwrap()
            };
            drop(host);
            let path = store
                .root()
                .join("deliveries")
                .join(format!("{}.json", event.event_id));
            let before = std::fs::read(&path).unwrap();
            require_delivery(&store, "alpha", &target, &delivery, false).unwrap();
            for field in ["project", "digest", "message", "phase", "event", "target"] {
                let mut changed = delivery.clone();
                match field {
                    "project" => changed.event.project_id = "beta".into(),
                    "digest" => changed.message_digest = "f".repeat(64),
                    "message" => changed.event.message = "foreign instruction".into(),
                    "phase" => changed.phase = Phase::Sent,
                    "event" => changed.event.event_id = format!("web-work-{}", "b".repeat(64)),
                    _ => changed.target = Target::new(2, target.url.clone()).unwrap(),
                }
                assert!(require_delivery(&store, "alpha", &target, &changed, false).is_err());
            }
            #[cfg(windows)]
            assert_eq!(
                inspect_registered_delivery_readiness(
                    &store,
                    "alpha",
                    &target,
                    &delivery,
                    &|| Err("GUARD_CANCELLED".into())
                )
                .unwrap_err(),
                "GUARD_CANCELLED"
            );
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(store.delivery(&event.event_id).unwrap(), Some(delivery));
        }
    }

    #[test]
    fn readiness_selection_refuses_running_or_changed_project_without_browser() {
        let root = std::env::temp_dir().join(format!(
            "readiness-selection-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = Store::open(&root).unwrap();
        store.initialize().unwrap();
        let target = Target::new(1, "https://chatgpt.com/c/readiness-fixture".into()).unwrap();
        atomic(
            &root.join("control.json"),
            &Control {
                schema_version: 1,
                desired: "RUNNING".into(),
            },
        )
        .unwrap();
        assert_eq!(
            require_selection(&store, "alpha", &target).unwrap_err(),
            "READINESS_REQUIRES_STOPPED_WAKE"
        );
        atomic(
            &root.join("control.json"),
            &Control {
                schema_version: 1,
                desired: "STOPPED".into(),
            },
        )
        .unwrap();
        // No target registration is permission to observe an arbitrary URL.
        assert!(require_selection(&store, "alpha", &target).is_err());
        let actual = store.set_target("alpha", 0, &target.url).unwrap();
        require_selection(&store, "alpha", &actual).unwrap();
        assert_eq!(
            require_selection(&store, "beta", &actual).unwrap_err(),
            "READINESS_REGISTERED_TARGET_CHANGED"
        );
        let changed = store
            .set_target("alpha", 1, "https://chatgpt.com/c/other-fixture")
            .unwrap();
        assert_eq!(
            require_selection(&store, "alpha", &actual).unwrap_err(),
            "READINESS_REGISTERED_TARGET_CHANGED"
        );
        require_selection(&store, "alpha", &changed).unwrap();
        #[cfg(windows)]
        {
            // Real file leases refuse before package/interpreter admission.
            let host = store.host_lock().unwrap();
            assert_eq!(
                inspect_registered_readiness(&store, "alpha", &changed, &|| Ok(())).unwrap_err(),
                "HOST_ALREADY_RUNNING"
            );
            drop(host);
            let browser = acquire_browser_quiescence(&store).unwrap();
            assert_eq!(
                inspect_registered_readiness(&store, "alpha", &changed, &|| Ok(())).unwrap_err(),
                "BROWSER_SESSION_BUSY"
            );
            drop(browser);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
