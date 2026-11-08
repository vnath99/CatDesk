//! Advisory observation through the existing stopped-host browser owner.
//! No event claim, USER input, assistant action, or delivery reconciliation.
use super::*;

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
    inspect_readiness(store, project, target, None, keep_current)
}

pub fn inspect_registered_delivery_readiness(
    store: &Store,
    project: &str,
    target: &Target,
    delivery: &crate::store::Delivery,
    keep_current: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    inspect_readiness(store, project, target, Some(delivery), keep_current)
}

#[cfg(any(windows, test))]
fn require_delivery(
    store: &Store,
    project: &str,
    target: &Target,
    delivery: &crate::store::Delivery,
) -> Result<()> {
    delivery.event.validate()?;
    if !matches!(
        delivery.phase,
        Phase::Claimed | Phase::Submitting | Phase::Attention
    ) || delivery.receipt.is_some()
        || delivery.target != *target
        || delivery.event.project_id != project
        || delivery.event.target_generation != target.generation
        || delivery.event.event_type != "engineering_attention"
        || !delivery.event.audit_references.is_empty()
        || delivery.message_digest != crate::protocol::digest(&delivery.event.message)
        || !delivery
            .event
            .event_id
            .strip_prefix("web-work-")
            .is_some_and(|id| {
                id.len() == 64
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        || store.delivery(&delivery.event.event_id)?.as_ref() != Some(delivery)
    {
        return Err("READINESS_WORK_DELIVERY_CHANGED".into());
    }
    Ok(())
}

#[cfg(windows)]
fn inspect_readiness(
    store: &Store,
    project: &str,
    target: &Target,
    delivery: Option<&crate::store::Delivery>,
    keep_current: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    if !crate::protocol::valid_id(project) {
        return Err("READINESS_PROJECT_INVALID".into());
    }
    require_selection(store, project, target)?;
    keep_current()?;
    let _host = store.host_lock()?;
    let _browser = acquire_browser_quiescence(store)?;
    require_selection(store, project, target)?;
    keep_current()?;
    if let Some(delivery) = delivery {
        require_delivery(store, project, target, delivery)?;
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
        Some(hash)
    } else {
        None
    };
    let mut rows = Vec::new();
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
        require_delivery(store, project, target, delivery)?;
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
    Ok(
        serde_json::json!({"action":if delivery.is_some() {"web_work_inspect"} else {"web_browser_inspect"},"projectId":project,
        "status":if result.is_ok() && final_state.is_some() {"BROWSER_READINESS_OBSERVED"} else {"BROWSER_OBSERVATION_INCOMPLETE"},
        "failureReason":result.err(),"lastPhase":phase,"finalState":final_state,
        "browserObservation":surface,"tabObservation":tabs,"observations":rows,
        "targetGeneration":target.generation,"targetDigest":target.digest,
        "eventId":delivery.map(|d| &d.event.event_id),"sourceDeliverySha256":delivery_hash,
        "submissionObservation":submission,"submissionObservationState":submission_state,
        "advisory":true,"browserReadOnly":true,"sendAttempted":false,
        "receiptCreated":false,"stateChanged":false,"retryAuthorized":false,"workResolved":false,
        "submissionOutcome":"NOT_ASSESSED","authenticationVerified":false,
        "ownedTreeExited":true,"hostStarted":false,"schedulerStarted":false}),
    )
}

#[cfg(not(windows))]
fn inspect_readiness(
    _: &Store,
    _: &str,
    _: &Target,
    _: Option<&crate::store::Delivery>,
    _: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    Err("READINESS_PLATFORM_UNSUPPORTED".into())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            require_delivery(&store, "alpha", &target, &delivery).unwrap();
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
                assert!(require_delivery(&store, "alpha", &target, &changed).is_err());
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
