//! Receipt-bound passive response observation through the existing stopped-host owner.
//! This module cannot type, submit, retry, open menus/traces, rebind a target or
//! install an adapter. Diagnostic hints never establish native execution authority.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    ResponseOwnerHeld,
    SentDiagnostics,
    HistoricalDiagnostics,
}

pub fn inspect_historical_task_response(
    store: &Store,
    project: &str,
    target: &Target,
    event: &str,
    keep_current: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    let (delivery, timer) = store
        .sent_response_scope(project, target, event)
        .map_err(|_| "RESPONSE_HISTORY_RECEIPT_OR_TIMER_REFUSED")?;
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
    .map_err(|_| "RESPONSE_HISTORY_BROWSER_OWNER_OR_ADMISSION_REFUSED".into())
}

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
    verified_observer_asset(source, adjacent, "wake_bridge.py", expected)
}

#[cfg(any(windows, test))]
fn verified_packaged_primitives(
    source: &Path,
    image_directory: &Path,
    expected: &str,
) -> Result<PathBuf> {
    let sibling = image_directory.join("wake_bridge.py");
    match std::fs::symlink_metadata(&sibling) {
        Ok(_) => {
            require_plain_install_file(&sibling)?;
            if sha256_file(&sibling, "READINESS_SOURCE_UNAVAILABLE")? != expected {
                return Err("READINESS_SOURCE_HASH_MISMATCH".into());
            }
            Ok(sibling)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            verified_primitives(source, &image_directory.join("browser-observer"), expected)
        }
        Err(_) => Err("READINESS_SOURCE_UNAVAILABLE".into()),
    }
}

#[cfg(any(windows, test))]
fn verified_observer_asset(
    source: &Path,
    adjacent: &Path,
    name: &str,
    expected: &str,
) -> Result<PathBuf> {
    let path = match std::fs::symlink_metadata(adjacent) {
        Ok(_) => {
            require_plain_install_directory(adjacent)?;
            adjacent.join(name)
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
    // Keep the fixed native stability failure inside the CLI's already allowed
    // RESPONSE_ vocabulary instead of redacting it to WEB_OPERATION_REFUSED.
    Err(match reason {
        Some("UNSTABLE") => "RESPONSE_STATE_UNSTABLE",
        Some(reason) => reason,
        None => "RESPONSE_PROBE_UNPROVEN",
    }
    .into())
}

#[cfg(any(windows, test))]
fn require_delivery(
    store: &Store,
    project: &str,
    target: &Target,
    delivery: &crate::store::Delivery,
    mode: Mode,
) -> Result<()> {
    delivery.event.validate()?;
    let phase_valid = store
        .sent_response_scope(project, target, &delivery.event.event_id)?
        .0
        == *delivery;
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
        && delivery.event.audit_references.len() == 5
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
        });
    // An exact source review may use only the passive current inspector. It
    // cannot obtain retained-owner completion or historical task authority.
    let source_review = mode == Mode::SentDiagnostics
        && delivery.event.event_type == "source_review"
        && delivery
            .event
            .event_id
            .strip_prefix("web-work-")
            .is_some_and(exact_hash)
        && delivery.event.audit_references.len() == 1
        && delivery.event.audit_references[0]
            .strip_prefix("catdesk-review-v1:")
            .is_some_and(|grant| grant.len() <= 128 && crate::protocol::valid_id(grant));
    if !phase_valid
        || delivery.phase != Phase::Sent
        || delivery.target != *target
        || delivery.event.project_id != project
        || delivery.event.target_generation != target.generation
        || !((delivery.event.event_type == "engineering_attention" && (ordinary || continuation))
            || source_review)
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
    if !crate::protocol::valid_id(project) {
        return Err("READINESS_PROJECT_INVALID".into());
    }
    let delivery = delivery.ok_or("READINESS_WORK_DELIVERY_CHANGED")?;
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
    require_delivery(store, project, target, delivery, mode)?;
    // Retain installed owner/adapter identity without demanding that the old
    // selected adapter implement this new read-only diagnostic operation.
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
    let source_primitives = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("READINESS_SOURCE_UNAVAILABLE")?
        .join("scripts/wake_bridge.py");
    let image_directory = std::env::current_exe()
        .map_err(|_| "READINESS_SOURCE_UNAVAILABLE")?
        .parent()
        .ok_or("READINESS_SOURCE_UNAVAILABLE")?
        .to_path_buf();
    let expected = format!(
        "{:x}",
        Sha256::digest(include_bytes!("../../scripts/wake_bridge.py"))
    );
    let primitives = verified_packaged_primitives(&source_primitives, &image_directory, &expected)?;
    let program = include_bytes!("../../scripts/wake_readiness_visual_probe.py");
    let loader = crate::readiness_probe::embedded_program_loader(program)?;
    let delivery_path = store
        .root()
        .join("deliveries")
        .join(format!("{}.json", delivery.event.event_id));
    require_plain_install_file(&delivery_path)?;
    let delivery_hash = sha256_file(&delivery_path, "READINESS_WORK_DELIVERY_CHANGED")?;
    let mut command = Command::new(python);
    command
        .arg("-I")
        .arg("-c")
        .arg(loader)
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
        .arg(if mode == Mode::ResponseOwnerHeld {
            "60"
        } else {
            "20"
        })
        .arg("--delivery-file")
        .arg(&delivery_path)
        .arg("--delivery-sha256")
        .arg(&delivery_hash)
        .arg("--response-only")
        .arg("--task-diagnostics")
        .current_dir(store.root());
    if mode == Mode::ResponseOwnerHeld {
        command.arg("--await-existing-response");
    }
    if mode == Mode::HistoricalDiagnostics {
        command.arg("--historical-response");
    }
    if mode == Mode::SentDiagnostics && delivery.event.event_type == "source_review" {
        command.arg("--source-review-diagnostics");
    }
    let mut rows = Vec::new();
    let result = crate::readiness_probe::run_bounded_observed_program(
        &mut command,
        Duration::from_secs(90),
        &mut rows,
        program,
    );
    // These failures are returned only after Job-empty and pipe-reader exit.
    // Any other native ownership failure cannot claim cleanup or page state.
    if let Err(reason) = result
        && !matches!(
            reason,
            "PROBE_PROCESS_TIMEOUT"
                | "PROBE_PIPE_FAILED"
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
    require_delivery(store, project, target, delivery, mode)?;
    if sha256_file(&delivery_path, "READINESS_WORK_DELIVERY_CHANGED")? != delivery_hash {
        return Err("READINESS_WORK_DELIVERY_CHANGED".into());
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
    let response = if result.is_ok()
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
            response::validate_observation(delivery, &values[0])?;
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
    let task_diagnostics = if mode != Mode::ResponseOwnerHeld
        && response
            .as_ref()
            .is_some_and(|v| v["stage"] == "RESPONSE_COMPLETE")
        && diagnostics.len() == 1
    {
        Some(&diagnostics[0])
    } else {
        None
    };
    Ok(
        serde_json::json!({"action":"web_work_inspect","projectId":project,
        "status":if result.is_ok() && final_state.is_some() {"BROWSER_READINESS_OBSERVED"} else {"BROWSER_OBSERVATION_INCOMPLETE"},
        "failureReason":result.err(),"lastPhase":phase,"finalState":final_state,
        "browserObservation":surface,"tabObservation":tabs,"observations":rows,
        "targetGeneration":target.generation,"targetDigest":target.digest,"eventId":delivery.event.event_id,
        "sourceDeliverySha256":delivery_hash,"advisory":true,"browserReadOnly":true,
        "sendAttempted":false,"receiptCreated":false,"stateChanged":false,"retryAuthorized":false,
        "workResolved":false,"reviewAuthorityGranted":false,"authenticationVerified":false,
        "responseObservation":if mode == Mode::HistoricalDiagnostics {None} else {response.clone()},
        "historicalResponseObservation":if mode == Mode::HistoricalDiagnostics {response} else {None},
        "historicalReadOnly":mode==Mode::HistoricalDiagnostics,"taskDiagnostics":task_diagnostics,
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
    use super::super::scheduler::tests::{fixture, sent};
    use super::*;

    fn ordinary(label: &str) -> (Store, crate::store::Delivery) {
        let (store, lease) = fixture(label);
        let id = format!("web-work-{}", "c".repeat(64));
        store
            .produce(&id, "alpha", "engineering_attention", "bounded task")
            .unwrap();
        let delivery = sent(&store, &lease, &id);
        drop(lease);
        (store, delivery)
    }

    #[test]
    fn passive_scope_requires_original_sent_receipt_and_exact_project_target() {
        let (store, delivery) = ordinary("passive-response-scope");
        let before = std::fs::read(
            store
                .root()
                .join("deliveries")
                .join(format!("{}.json", delivery.event.event_id)),
        )
        .unwrap();
        require_delivery(
            &store,
            "alpha",
            &delivery.target,
            &delivery,
            Mode::SentDiagnostics,
        )
        .unwrap();
        assert!(
            require_delivery(
                &store,
                "beta",
                &delivery.target,
                &delivery,
                Mode::SentDiagnostics
            )
            .is_err()
        );
        let mut changed = delivery.clone();
        changed.message_digest = "a".repeat(64);
        assert!(
            require_delivery(
                &store,
                "alpha",
                &delivery.target,
                &changed,
                Mode::SentDiagnostics
            )
            .is_err()
        );
        assert_eq!(
            before,
            std::fs::read(
                store
                    .root()
                    .join("deliveries")
                    .join(format!("{}.json", delivery.event.event_id))
            )
            .unwrap()
        );
    }

    #[test]
    fn historical_reader_requires_durable_complete_timer_before_browser_admission() {
        let (store, delivery) = ordinary("passive-historical-complete");
        assert_eq!(
            inspect_historical_task_response(
                &store,
                "alpha",
                &delivery.target,
                &delivery.event.event_id,
                &|| Ok(())
            )
            .unwrap_err(),
            "READINESS_COMPLETED_RESPONSE_REQUIRED"
        );
    }

    #[test]
    fn passive_source_review_cannot_become_retained_completion_owner() {
        let (store, lease) = fixture("passive-review-scope");
        let id = format!("web-work-{}", "d".repeat(64));
        let target = store.config().unwrap().targets["alpha"].clone();
        store
            .produce_expected_with_audit(
                &id,
                "alpha",
                "source_review",
                "public source review",
                &target,
                &["catdesk-review-v1:valid-review".into()],
            )
            .unwrap();
        let delivery = sent(&store, &lease, &id);
        require_delivery(&store, "alpha", &target, &delivery, Mode::SentDiagnostics).unwrap();
        assert!(
            require_delivery(&store, "alpha", &target, &delivery, Mode::ResponseOwnerHeld).is_err()
        );
        assert!(
            require_delivery(
                &store,
                "alpha",
                &target,
                &delivery,
                Mode::HistoricalDiagnostics
            )
            .is_err()
        );
    }

    #[test]
    fn bad_adjacent_observer_bytes_never_fall_back_to_source() {
        let (store, _lease) = fixture("passive-observer-assets");
        let root = store.root().join("assets-test");
        std::fs::create_dir(&root).unwrap();
        let source = root.join("source.py");
        std::fs::write(&source, b"good").unwrap();
        let image = root.join("image");
        std::fs::create_dir(&image).unwrap();
        let expected = format!("{:x}", Sha256::digest(b"good"));
        assert_eq!(
            verified_packaged_primitives(&source, &image, &expected).unwrap(),
            source
        );
        std::fs::write(image.join("wake_bridge.py"), b"bad").unwrap();
        assert_eq!(
            verified_packaged_primitives(&source, &image, &expected).unwrap_err(),
            "READINESS_SOURCE_HASH_MISMATCH"
        );
    }
}
