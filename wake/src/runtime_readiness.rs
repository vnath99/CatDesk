//! Advisory observation through the existing stopped-host browser owner.
//! No event claim, USER input, assistant action, or delivery reconciliation.
use super::*;

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
#[cfg(windows)]
pub fn inspect_registered_readiness(
    store: &Store,
    project: &str,
    target: &Target,
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
    // Reuse the existing observer primitives. Their fixed source path must
    // match these compiled bytes; callers cannot supply an alternate helper.
    let primitives = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("READINESS_SOURCE_UNAVAILABLE")?
        .join("scripts/wake_bridge.py");
    require_plain_install_file(&primitives)?;
    let expected = format!(
        "{:x}",
        Sha256::digest(include_bytes!("../../scripts/wake_bridge.py"))
    );
    if sha256_file(&primitives, "READINESS_SOURCE_UNAVAILABLE")? != expected {
        return Err("READINESS_SOURCE_HASH_MISMATCH".into());
    }
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
    Ok(
        serde_json::json!({"action":"web_browser_inspect","projectId":project,
        "status":if result.is_ok() && final_state.is_some() {"BROWSER_READINESS_OBSERVED"} else {"BROWSER_OBSERVATION_INCOMPLETE"},
        "failureReason":result.err(),"lastPhase":phase,"finalState":final_state,
        "browserObservation":surface,"tabObservation":tabs,"observations":rows,
        "targetGeneration":target.generation,"targetDigest":target.digest,
        "advisory":true,"browserReadOnly":true,"sendAttempted":false,
        "submissionOutcome":"NOT_ASSESSED","authenticationVerified":false,
        "ownedTreeExited":true,"hostStarted":false,"schedulerStarted":false}),
    )
}

#[cfg(not(windows))]
pub fn inspect_registered_readiness(
    _: &Store,
    _: &str,
    _: &Target,
    _: &dyn Fn() -> Result<()>,
) -> Result<serde_json::Value> {
    Err("READINESS_PLATFORM_UNSUPPORTED".into())
}

#[cfg(test)]
mod tests {
    use super::*;

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
