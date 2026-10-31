//! One explicit recovery transition for login-only markers. No queue, rollover,
//! delivery, draft, release selection or authentication state is edited here.
use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LoginMarker {
    schema_version: u32,
    purpose: String,
    target_sha256: String,
    started_utc: u64,
}

fn marker_target(path: &Path, expected: &str) -> Result<LoginMarker> {
    if expected.len() != 64
        || !expected
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || hash_file(path, 4096)? != expected
    {
        return Err("DEV_LOGIN_RECOVERY_MARKER_CHANGED".into());
    }
    let value: LoginMarker = read_json(path, 4096)?;
    if value.schema_version != 1
        || value.purpose != "DEV_LOGIN_OWNED_BROWSER_V1"
        || value.target_sha256.len() != 64
        || value.started_utc == 0
        || value.started_utc > now()
    {
        return Err("DEV_LOGIN_RECOVERY_MARKER_UNSUPPORTED".into());
    }
    Ok(value)
}

fn observe_profile(root: &Path) -> Result<()> {
    let python = runtime::browser_process_root(&root.join("runtime/python.exe"))?;
    let profile = runtime::browser_process_root(&root.join("browser-profile"))?;
    plain(&profile, true)?;
    let mut command = Command::new(&python);
    command.args([
        "-I",
        "-c",
        include_str!("../../scripts/wake_profile_quiescence.py"),
    ]);
    command.arg(&profile).arg(&python).current_dir(root);
    let rows = crate::readiness_probe::run_bounded(&mut command, Duration::from_secs(15))
        .map_err(|_| "DEV_LOGIN_RECOVERY_INVENTORY_UNVERIFIED")?;
    match rows.as_slice() {
        [row] if row == "PROBE:FINAL:PROFILE_QUIESCENT" => Ok(()),
        [row] if row == "PROBE:FINAL:PROFILE_HELPER_ACTIVE" => {
            Err("DEV_LOGIN_RECOVERY_HELPER_ACTIVE".into())
        }
        [row] if row == "PROBE:FINAL:PROFILE_BROWSER_ACTIVE" => {
            Err("DEV_LOGIN_RECOVERY_BROWSER_ACTIVE".into())
        }
        [row] if row == "PROBE:FINAL:PROFILE_OWNER_UNATTRIBUTED" => {
            Err("DEV_LOGIN_RECOVERY_OWNER_UNATTRIBUTED".into())
        }
        _ => Err("DEV_LOGIN_RECOVERY_INVENTORY_UNVERIFIED".into()),
    }
}

/// `paired` must read the existing project registry and this exact selected
/// store, under the caller's existing binding read lock. It must not mutate.
/// Preflight obtains the same leases and performs the same native observation
/// as commit. A free lock is never treated as process-tree exit proof.
pub fn reconcile_login(
    workspace: &Path,
    expected_marker_sha256: &str,
    execute: bool,
    paired: impl Fn(&Store) -> Result<protocol::Target>,
) -> Result<Value> {
    reconcile_with_observer(
        workspace,
        expected_marker_sha256,
        execute,
        paired,
        observe_profile,
    )
}

pub(super) fn reconcile_with_observer(
    workspace: &Path,
    expected: &str,
    execute: bool,
    paired: impl Fn(&Store) -> Result<protocol::Target>,
    observe: impl Fn(&Path) -> Result<()>,
) -> Result<Value> {
    let workspace = canonical_workspace(workspace)?;
    let dev = dev_dir(&workspace);
    let marker = dev.join("login-active.json");
    let value = marker_target(&marker, expected)?;
    let (root, authority) =
        validated_authority_for_login_recovery(&workspace, false, false, Some(expected))?
            .ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    let _backend = backend_lock(&workspace)?;
    let store = Store::open(&root)?;
    let _host = store.host_lock()?;
    let _browser = runtime::browser_session_lock(&root, Duration::ZERO)?;
    require_stopped_control(&root)?;
    let target = paired(&store)?;
    target.validate()?;
    if target.digest != value.target_sha256 {
        return Err("DEV_LOGIN_RECOVERY_TARGET_CHANGED".into());
    }
    validate_against_production(&authority, &target.url)?;
    let archive = dev.join(format!("login-recovered-{expected}.json"));
    let journal = dev.join(format!("login-recovery-{expected}.json"));
    if present(&archive)? {
        return Err("DEV_LOGIN_RECOVERY_ARCHIVE_EXISTS".into());
    }
    // This is current independent native evidence, not a claim about the old
    // Job or a historical message. Unrelated proven browser trees may run.
    observe(&root)?;
    if validated_authority_for_login_recovery(&workspace, false, false, Some(expected))?
        != Some((root.clone(), authority))
        || paired(&store)? != target
    {
        return Err("DEV_LOGIN_RECOVERY_AUTHORITY_CHANGED".into());
    }
    require_stopped_control(&root)?;
    marker_target(&marker, expected)?;
    let result = json!({"schemaVersion":1,"mode":"DEVELOPMENT_UNPROTECTED",
        "status":if execute {"LOGIN_ONLY_MARKER_ARCHIVED"} else {"LOGIN_RECOVERY_PREFLIGHT_PASSED"},
        "markerSha256":expected,"targetSha256":target.digest,"targetGeneration":target.generation,
        "currentProfileQuiescenceVerified":true,"historicalJobExitInferred":false,
        "historicalSubmissionOutcome":"UNCHANGED_UNPROVEN","markerArchived":execute,
        "browserStarted":false,"hostStarted":false,"messagesSent":0,
        "bindingChanged":false,"draftsInspected":false,"releaseSelectionChanged":false});
    if !execute {
        return Ok(result);
    }
    let intent = json!({"schemaVersion":1,"purpose":"DEV_LOGIN_ONLY_RECOVERY_V1",
        "markerSha256":expected,"targetSha256":target.digest,"targetGeneration":target.generation,
        "phase":"PREPARED_ARCHIVE_EXISTENCE_IS_COMMIT_WITNESS",
        "historicalSubmissionOutcome":"UNCHANGED_UNPROVEN"});
    if present(&journal)? {
        if read_json::<Value>(&journal, 4096)? != intent {
            return Err("DEV_LOGIN_RECOVERY_JOURNAL_CHANGED".into());
        }
    } else {
        write_json_new(&journal, &intent)?;
    }
    marker_target(&marker, expected)?;
    // Same-directory, write-through, no replacement: preserve the original
    // bytes as the commit witness. A crash before this leaves the active fence.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    if unsafe {
        MoveFileExW(
            wide(marker.as_os_str()).as_ptr(),
            wide(archive.as_os_str()).as_ptr(),
            8,
        )
    } == 0
    {
        return Err("DEV_LOGIN_RECOVERY_ARCHIVE_FAILED".into());
    }
    if hash_file(&archive, 4096)? != expected || present(&marker)? {
        return Err("DEV_LOGIN_RECOVERY_COMMIT_UNVERIFIED".into());
    }
    Ok(result)
}
