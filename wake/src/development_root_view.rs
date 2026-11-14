//! Explicit reconciliation of an unprotected runtime's exclusion-root view.
//! No protected file, package pointer, profile, target or task is written.
use super::*;

fn digest<T: Serialize>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "DEV_RUNTIME_ENCODING_INVALID")?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

// The only supported transition: both recorded spellings named one object;
// the explicit package-cache path STILL names that object, and the direct
// spelling now names a different, non-development directory. Neither a lost
// anchor nor an arbitrary replaced root is reconstruction authority.
pub(super) fn require_known_alias_split(
    authority: &Authority,
    observed: &[ProductionRoot],
) -> Result<()> {
    let old = &authority.production_roots;
    if old.len() != 2
        || observed.len() != 2
        || old.iter().zip(observed).any(|(a, b)| a.path != b.path)
        || old[0].identity.is_none()
        || old[0].identity != old[1].identity
        || observed[1].identity != old[1].identity
        || observed[0].identity.is_none()
        || observed[0].identity == old[0].identity
        || observed.iter().any(|r| {
            r.identity.as_ref() == Some(&authority.runtime_identity)
                || r.identity.as_ref() == Some(&authority.workspace_identity)
        })
    {
        return Err("DEV_ROOT_VIEW_NOT_KNOWN_ALIAS_SPLIT".into());
    }
    Ok(())
}

fn result(status: &str, old: &str, view: &str, new: &str) -> Value {
    json!({"mode":"DEVELOPMENT_UNPROTECTED","status":status,
        "oldAuthoritySha256":old,"rootViewSha256":view,"newAuthoritySha256":new,
        "protectedWrites":0,"packageSelectionChanged":false,
        "profileChanged":false,"bindingsChanged":false,"hostStarted":false,
        "browserStarted":false,"sendAttempted":false})
}

/// Preflight and execution use the same existing leases, immutable package
/// checks and bounded profile-quiescence observer. A preflight does not write a
/// proposal. Execution requires both exact hashes and replaces ONE unprotected
/// authority file atomically, retaining its original bytes under their hash.
/// This is not protected Wake migration, installation or release authority.
pub fn reconcile_root_view(
    workspace: &Path,
    expected_authority_sha256: &str,
    expected_view_sha256: Option<&str>,
    execute: bool,
) -> Result<Value> {
    reconcile_with_observer(
        workspace,
        expected_authority_sha256,
        expected_view_sha256,
        execute,
        login_recovery::observe_profile,
    )
}

fn reconcile_with_observer(
    workspace: &Path,
    expected_authority_sha256: &str,
    expected_view_sha256: Option<&str>,
    execute: bool,
    observe: impl FnOnce(&Path) -> Result<()>,
) -> Result<Value> {
    if !valid_hash(expected_authority_sha256)
        || expected_view_sha256.is_some_and(|v| !valid_hash(v))
        || execute && expected_view_sha256.is_none()
    {
        return Err("DEV_ROOT_VIEW_EXPECTED_HASH_REQUIRED".into());
    }
    let workspace = canonical_workspace(workspace)?;
    let dev = dev_dir(&workspace);
    let root = runtime_root(&workspace);
    let path = dev.join("authority.json");
    let backup = dev.join(format!("root-view-authority-{expected_authority_sha256}.json"));
    let current_hash = hash_file(&path, 32 * 1024)?;
    let observed = production_roots()?;
    let view_hash = digest(&observed)?;
    if expected_view_sha256.is_some_and(|value| value != view_hash) {
        return Err("DEV_ROOT_VIEW_OBSERVATION_CHANGED".into());
    }
    if current_hash != expected_authority_sha256 {
        // Read-only recognition of an already committed exact transition. A
        // lost reply never causes another mutation or a broader reconstruction.
        if hash_file(&backup, 32 * 1024)? != expected_authority_sha256 {
            return Err("DEV_ROOT_VIEW_AUTHORITY_CHANGED".into());
        }
        let old: Authority = read_json(&backup, 32 * 1024)?;
        require_known_alias_split(&old, &observed)?;
        let mut expected = old;
        expected.production_roots = observed;
        let actual = validated_authority(&workspace)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?.1;
        if expected != actual || expected_view_sha256.is_none() {
            return Err("DEV_ROOT_VIEW_AUTHORITY_CHANGED".into());
        }
        return Ok(result("ROOT_VIEW_ALREADY_RECONCILED", expected_authority_sha256, &view_hash, &current_hash));
    }
    let _backend = backend_lock(&workspace)?;
    let _projects = project_backend_lock(&workspace, false)?;
    let validated = || validated_authority_for_root_view(&workspace, false, false, None, Some(&observed))?
        .ok_or_else(|| "DEV_RUNTIME_NOT_PREPARED".to_string());
    let (_, old) = validated()?;
    let store = Store::open(&root)?;
    let _host = store.host_lock()?;
    let _browser = runtime::browser_session_lock(&root, Duration::ZERO)?;
    require_stopped_control(&root)?;
    observe(&root)?;
    require_stopped_control(&root)?;
    if validated()?.1 != old || hash_file(&path, 32 * 1024)? != expected_authority_sha256 {
        return Err("DEV_ROOT_VIEW_AUTHORITY_CHANGED".into());
    }
    let mut new = old;
    new.production_roots = observed.clone();
    let bytes = serde_json::to_vec_pretty(&new).map_err(|_| "DEV_RUNTIME_ENCODING_INVALID")?;
    let new_hash = format!("{:x}", Sha256::digest(&bytes));
    if !execute {
        return Ok(result("ROOT_VIEW_PREFLIGHT_ONLY", expected_authority_sha256, &view_hash, &new_hash));
    }
    let original = fs::read(&path).map_err(|_| "DEV_RUNTIME_READ_FAILED")?;
    if format!("{:x}", Sha256::digest(&original)) != expected_authority_sha256 {
        return Err("DEV_ROOT_VIEW_AUTHORITY_CHANGED".into());
    }
    retain_exact(&backup, &original)?;
    let stage = dev.join(format!("root-view-stage-{new_hash}.json"));
    retain_exact(&stage, &bytes)?;
    // Recheck after durable staging, before the single atomic replacement.
    validated()?;
    require_stopped_control(&root)?;
    if hash_file(&path, 32 * 1024)? != expected_authority_sha256 {
        return Err("DEV_ROOT_VIEW_AUTHORITY_CHANGED".into());
    }
    replace_update_file(&stage, &path)?;
    if hash_file(&path, 32 * 1024)? != new_hash
        || validated_authority(&workspace)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?.1 != new
    {
        return Err("DEV_ROOT_VIEW_READBACK_FAILED".into());
    }
    Ok(result("ROOT_VIEW_RECONCILED_STOPPED", expected_authority_sha256, &view_hash, &new_hash))
}

fn retain_exact(path: &Path, bytes: &[u8]) -> Result<()> {
    if present(path)? {
        plain(path, false)?;
        hash_file(path, 32 * 1024)?;
        if fs::read(path).map_err(|_| "DEV_RUNTIME_READ_FAILED")? != bytes {
            return Err("DEV_ROOT_VIEW_RETAINED_BYTES_CHANGED".into());
        }
        Ok(())
    } else {
        write_new(path, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{cleanup, fixture, prepare_fixture};

    fn split_fixture() -> Option<(PathBuf, PathBuf, Authority)> {
        let observed = production_roots().unwrap();
        // This integration fixture uses real Windows directory identities,
        // without writing either protected directory. Other machines still run
        // the pure negative matrix; a missing local layout is not manufactured.
        if observed.len() != 2 || observed.iter().any(|r| r.identity.is_none())
            || observed[0].identity == observed[1].identity {
            return None;
        }
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let path = dev_dir(&workspace).join("authority.json");
        let mut old: Authority = read_json(&path, 32 * 1024).unwrap();
        old.production_roots[0].identity = old.production_roots[1].identity.clone();
        fs::write(&path, serde_json::to_vec_pretty(&old).unwrap()).unwrap();
        Some((parent, workspace, old))
    }

    #[test]
    fn root_view_preflight_commit_and_lost_reply_keep_selection_and_state() {
        let Some((parent, workspace, _)) = split_fixture() else { return };
        let root = runtime_root(&workspace);
        let authority = dev_dir(&workspace).join("authority.json");
        let old = hash_file(&authority, 32 * 1024).unwrap();
        let pointer = fs::read(root.join("current.json")).unwrap();
        let profile = identity(&root.join("browser-profile"), true).unwrap();
        let config = fs::read(root.join("config.json")).unwrap();
        let control = fs::read(root.join("control.json")).unwrap();
        assert_eq!(root_for_workspace(&workspace).unwrap_err(), "DEV_PRODUCTION_ROOT_MISMATCH");
        let report = reconcile_with_observer(&workspace, &old, None, false, |_| Ok(())).unwrap();
        assert_eq!(report["status"], "ROOT_VIEW_PREFLIGHT_ONLY");
        assert_eq!(hash_file(&authority, 32 * 1024).unwrap(), old);
        let view = report["rootViewSha256"].as_str().unwrap();
        // Simulate interruption after durable staging but before replacement.
        let original = fs::read(&authority).unwrap();
        let mut staged: Authority = serde_json::from_slice(&original).unwrap();
        staged.production_roots = production_roots().unwrap();
        retain_exact(&dev_dir(&workspace).join(format!("root-view-authority-{old}.json")), &original).unwrap();
        retain_exact(&dev_dir(&workspace).join(format!("root-view-stage-{}.json", report["newAuthoritySha256"].as_str().unwrap())),
            &serde_json::to_vec_pretty(&staged).unwrap()).unwrap();
        assert_eq!(root_for_workspace(&workspace).unwrap_err(), "DEV_PRODUCTION_ROOT_MISMATCH");
        let done = reconcile_with_observer(&workspace, &old, Some(view), true, |_| Ok(())).unwrap();
        assert_eq!(done["status"], "ROOT_VIEW_RECONCILED_STOPPED");
        assert!(root_for_workspace(&workspace).unwrap().is_some());
        let committed = fs::read(&authority).unwrap();
        let repeated = reconcile_with_observer(&workspace, &old, Some(view), true, |_| panic!("must not act again")).unwrap();
        assert_eq!(repeated["status"], "ROOT_VIEW_ALREADY_RECONCILED");
        assert_eq!(fs::read(&authority).unwrap(), committed);
        assert_eq!(fs::read(root.join("current.json")).unwrap(), pointer);
        assert_eq!(identity(&root.join("browser-profile"), true).unwrap(), profile);
        assert_eq!(fs::read(root.join("config.json")).unwrap(), config);
        assert_eq!(fs::read(root.join("control.json")).unwrap(), control);
        assert_eq!(hash_file(&dev_dir(&workspace).join(format!("root-view-authority-{old}.json")), 32*1024).unwrap(), old);
        cleanup(&parent);
    }

    #[test]
    fn root_view_refuses_changed_anchor_absent_root_and_development_alias() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let mut old: Authority = read_json(&dev_dir(&workspace).join("authority.json"), 32 * 1024).unwrap();
        let anchor = FullFileIdentityV2 { volume_serial: u64::MAX, file_id_hex: format!("{:032x}", 1) };
        old.production_roots = vec![
            ProductionRoot { path: "direct-fixture".into(), identity: Some(anchor.clone()) },
            ProductionRoot { path: "package-fixture".into(), identity: Some(anchor) },
        ];
        let mut observed = old.production_roots.clone();
        observed[0].identity = Some(FullFileIdentityV2 { volume_serial: u64::MAX, file_id_hex: format!("{:032x}", 2) });
        assert!(require_known_alias_split(&old, &observed).is_ok());
        for kind in 0..6 {
            let mut roots = observed.clone();
            match kind {
                0 => roots[1].identity = roots[0].identity.clone(),
                1 => roots[0].identity = None,
                2 => roots[1].identity = None,
                3 => roots[0].identity = Some(old.runtime_identity.clone()),
                4 => roots[0].identity = Some(old.workspace_identity.clone()),
                _ => roots[0].path.push_str("-other"),
            }
            assert_eq!(require_known_alias_split(&old, &roots).unwrap_err(), "DEV_ROOT_VIEW_NOT_KNOWN_ALIAS_SPLIT");
        }
        let mut unrelated = old.clone();
        unrelated.production_roots[0].identity = observed[0].identity.clone();
        assert!(require_known_alias_split(&unrelated, &observed).is_err());
        cleanup(&parent);
    }

    #[test]
    fn root_view_refuses_hash_drift_active_owners_and_unverified_profile() {
        let Some((parent, workspace, _)) = split_fixture() else { return };
        let root = runtime_root(&workspace);
        let path = dev_dir(&workspace).join("authority.json");
        let original = fs::read(&path).unwrap();
        let old = hash_file(&path, 32 * 1024).unwrap();
        assert_eq!(reconcile_with_observer(&workspace, &old, None, true, |_| Ok(())).unwrap_err(), "DEV_ROOT_VIEW_EXPECTED_HASH_REQUIRED");
        assert_eq!(reconcile_with_observer(&workspace, &old, Some(&"0".repeat(64)), true, |_| Ok(())).unwrap_err(), "DEV_ROOT_VIEW_OBSERVATION_CHANGED");
        let lease = Store::open(&root).unwrap().host_lock().unwrap();
        assert_eq!(reconcile_with_observer(&workspace, &old, None, false, |_| Ok(())).unwrap_err(), "HOST_ALREADY_RUNNING");
        drop(lease);
        let lease = backend_lock(&workspace).unwrap();
        assert_eq!(reconcile_with_observer(&workspace, &old, None, false, |_| Ok(())).unwrap_err(), "DEV_BACKEND_ALREADY_RUNNING");
        drop(lease);
        let lease = project_backend_lock(&workspace, false).unwrap();
        assert_eq!(reconcile_with_observer(&workspace, &old, None, false, |_| Ok(())).unwrap_err(), "DEV_BACKEND_ALREADY_RUNNING");
        drop(lease);
        let lease = runtime::browser_session_lock(&root, Duration::ZERO).unwrap();
        assert_eq!(reconcile_with_observer(&workspace, &old, None, false, |_| Ok(())).unwrap_err(), "BROWSER_SESSION_BUSY");
        drop(lease);
        assert_eq!(reconcile_with_observer(&workspace, &old, None, false, |_| Err("PROFILE_NOT_QUIESCENT".into())).unwrap_err(), "PROFILE_NOT_QUIESCENT");
        assert_eq!(reconcile_with_observer(&workspace, &old, None, false, |_| {
            fs::write(&path, b"changed").unwrap(); Ok(())
        }).unwrap_err(), "DEV_RUNTIME_RECORD_INVALID");
        fs::write(&path, &original).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
        cleanup(&parent);
    }

    #[test]
    fn root_view_keeps_package_login_and_stopped_control_guards() {
        let Some((parent, workspace, old)) = split_fixture() else { return };
        let root = runtime_root(&workspace);
        let path = dev_dir(&workspace).join("authority.json");
        let hash = hash_file(&path, 32 * 1024).unwrap();
        let store = Store::open(&root).unwrap();
        runtime::control(&store, "RUNNING").unwrap();
        assert_eq!(reconcile_with_observer(&workspace, &hash, None, false, |_| Ok(())).unwrap_err(), "DEV_LOGIN_HOST_NOT_STOPPED");
        runtime::control(&store, "STOPPED").unwrap();
        let artifact = root.join("versions").join(old.package_id).join("adapter.py");
        let bytes = fs::read(&artifact).unwrap();
        fs::write(&artifact, b"unverified bytes").unwrap();
        assert_eq!(reconcile_with_observer(&workspace, &hash, None, false, |_| Ok(())).unwrap_err(), "DEV_RUNTIME_PACKAGE_MISMATCH");
        fs::write(&artifact, bytes).unwrap();
        for (name, error) in [("login-active.json", "DEV_LOGIN_CLEANUP_UNVERIFIED"),
            (PACKAGE_UPDATE_ATTEMPT, "DEV_PACKAGE_UPDATE_INCOMPLETE")] {
            let marker = dev_dir(&workspace).join(name);
            write_new(&marker, b"{}").unwrap();
            assert_eq!(reconcile_with_observer(&workspace, &hash, None, false, |_| Ok(())).unwrap_err(), error);
            fs::remove_file(marker).unwrap();
        }
        assert_eq!(hash_file(&path, 32 * 1024).unwrap(), hash);
        cleanup(&parent);
    }
}
