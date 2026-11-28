//! One explicit recovery transition for login-only markers. No queue, rollover,
//! delivery, draft, release selection or authentication state is edited here.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitialLogin {
    project_id: String,
    intent_id: String,
    journal_sha256: String,
    journal_identity: FullFileIdentityV2,
}

fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl InitialLogin {
    /// Keep the existing Windows file pin while the project lease is held.
    /// Recovery must reopen the same filesystem object and exact raw bytes.
    pub fn pin(
        project: &str,
        intent: &str,
        path: &Path,
    ) -> Result<(Self, impl Fn() -> Result<()> + use<>)> {
        if !protocol::valid_id(project) || !protocol::valid_id(intent) {
            return Err("DEV_INITIAL_LOGIN_IDENTITY_INVALID".into());
        }
        let path = path.to_owned();
        let mut pin = crate::install_broker_offline_pins::OfflineObjectPinV2::open(&path, false)?;
        let bytes = pin.read_bounded(24 * 1024)?;
        let proof = Self {
            project_id: project.into(),
            intent_id: intent.into(),
            journal_sha256: format!("{:x}", Sha256::digest(bytes)),
            journal_identity: pin.identity().full.clone(),
        };
        Ok((proof, move || pin.revalidate_path(&path)))
    }

    pub(super) fn marker(&self) -> Value {
        json!({"schemaVersion":1, "purpose":"DEV_INITIAL_LOGIN_OWNED_BROWSER_V1",
            "initialLogin":self, "startedUtc":now()})
    }
}

#[derive(Deserialize)]
#[serde(tag = "purpose", rename_all_fields = "camelCase", deny_unknown_fields)]
enum LoginMarker {
    #[serde(rename = "DEV_LOGIN_OWNED_BROWSER_V1")]
    Paired {
        schema_version: u32,
        target_sha256: String,
        started_utc: u64,
    },
    #[serde(rename = "DEV_INITIAL_LOGIN_OWNED_BROWSER_V1")]
    Initial {
        schema_version: u32,
        initial_login: InitialLogin,
        started_utc: u64,
    },
}

#[derive(PartialEq, Eq)]
pub(super) enum LoginEvidence {
    Paired(protocol::Target),
    Initial(InitialLogin),
}

impl LoginEvidence {
    fn check(&self, marker: &LoginMarker, authority: &Authority) -> Result<()> {
        match (self, marker) {
            (Self::Paired(target), LoginMarker::Paired { target_sha256, .. })
                if &target.digest == target_sha256 =>
            {
                target.validate()?;
                validate_against_production(authority, &target.url)
            }
            (Self::Initial(proof), LoginMarker::Initial { initial_login, .. })
                if proof == initial_login =>
            {
                validate_production_roots(authority, None)
            }
            _ => Err("DEV_LOGIN_RECOVERY_TARGET_CHANGED".into()),
        }
    }

    fn add_receipt_fields(&self, value: &mut Value) {
        match self {
            Self::Paired(target) => {
                value["targetSha256"] = json!(target.digest);
                value["targetGeneration"] = json!(target.generation);
            }
            Self::Initial(proof) => value["initialLogin"] = json!(proof),
        }
    }
}

fn marker_target(path: &Path, expected: &str) -> Result<LoginMarker> {
    if !sha256(expected) || hash_file(path, 4096)? != expected {
        return Err("DEV_LOGIN_RECOVERY_MARKER_CHANGED".into());
    }
    let value: LoginMarker = read_json(path, 4096)?;
    let (version, started, valid) = match &value {
        LoginMarker::Paired {
            schema_version,
            target_sha256,
            started_utc,
        } => (*schema_version, *started_utc, sha256(target_sha256)),
        LoginMarker::Initial {
            schema_version,
            initial_login,
            started_utc,
        } => (
            *schema_version,
            *started_utc,
            protocol::valid_id(&initial_login.project_id)
                && protocol::valid_id(&initial_login.intent_id)
                && sha256(&initial_login.journal_sha256)
                && initial_login.journal_identity.volume_serial != 0
                && initial_login.journal_identity.file_id_hex.len() == 32
                && initial_login
                    .journal_identity
                    .file_id_hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && initial_login
                    .journal_identity
                    .file_id_hex
                    .bytes()
                    .any(|b| b != b'0'),
        ),
    };
    if version != 1 || !valid || started == 0 || started > now() {
        return Err("DEV_LOGIN_RECOVERY_MARKER_UNSUPPORTED".into());
    }
    Ok(value)
}

pub(super) fn observe_profile(root: &Path) -> Result<()> {
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
    reconcile_evidence(
        workspace,
        expected,
        execute,
        |store| paired(store).map(LoginEvidence::Paired),
        observe,
    )
}

pub fn reconcile_initial_login(
    workspace: &Path,
    expected: &str,
    execute: bool,
    proof: &InitialLogin,
    check: impl Fn(&Store) -> Result<()>,
) -> Result<Value> {
    reconcile_evidence(
        workspace,
        expected,
        execute,
        |store| {
            check(store)?;
            Ok(LoginEvidence::Initial(proof.clone()))
        },
        observe_profile,
    )
}

pub(super) fn reconcile_evidence(
    workspace: &Path,
    expected: &str,
    execute: bool,
    evidence: impl Fn(&Store) -> Result<LoginEvidence>,
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
    let store = Store::open_read_only(&root)?;
    let _host = Store::open_existing_writer(&root)?.host_lock()?;
    let _browser = runtime::browser_session_lock(&root, Duration::ZERO)?;
    require_stopped_control(&root)?;
    let original = evidence(&store)?;
    original.check(&value, &authority)?;
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
        || evidence(&store)? != original
    {
        return Err("DEV_LOGIN_RECOVERY_AUTHORITY_CHANGED".into());
    }
    require_stopped_control(&root)?;
    marker_target(&marker, expected)?;
    let mut result = json!({"schemaVersion":1,"mode":"DEVELOPMENT_UNPROTECTED",
        "status":if execute {"LOGIN_ONLY_MARKER_ARCHIVED"} else {"LOGIN_RECOVERY_PREFLIGHT_PASSED"},
        "markerSha256":expected,
        "currentProfileQuiescenceVerified":true,"historicalJobExitInferred":false,
        "historicalSubmissionOutcome":"UNCHANGED_UNPROVEN","markerArchived":execute,
        "browserStarted":false,"hostStarted":false,"messagesSent":0,
        "bindingChanged":false,"draftsInspected":false,"releaseSelectionChanged":false});
    original.add_receipt_fields(&mut result);
    if !execute {
        return Ok(result);
    }
    let mut intent = json!({"schemaVersion":1,"purpose":"DEV_LOGIN_ONLY_RECOVERY_V1",
        "markerSha256":expected,
        "phase":"PREPARED_ARCHIVE_EXISTENCE_IS_COMMIT_WITNESS",
        "historicalSubmissionOutcome":"UNCHANGED_UNPROVEN"});
    original.add_receipt_fields(&mut intent);
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
