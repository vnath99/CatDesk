//! An explicitly bounded branch of the existing release-pair transaction.
//! It never releases a serving process or reinterprets a V1 serving receipt.
use super::*;

const MODE: &str = "QUALIFIED_STOPPED";
const PROFILE: &str = "HEADLESS_LOCAL";

// The legacy writer removes its destination before rename. Stopped recovery
// must retain the previous witness through interruption, so this branch uses
// the platform's replace-existing atomic rename after flushing a unique file.
fn atomic_json_write<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path.parent().ok_or("stopped evidence parent unavailable")?;
    fs::create_dir_all(parent).map_err(|_| "stopped evidence directory unavailable")?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("stopped evidence destination is unsafe".into());
        }
    }
    let temporary = parent.join(format!(".stopped-{}.tmp", Uuid::new_v4().simple()));
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|_| "stopped evidence serialization failed")?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|_| "stopped evidence temporary unavailable")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "stopped evidence flush failed")?;
    drop(file);
    fs::rename(temporary, path).map_err(|_| "stopped evidence atomic publication failed".into())
}

pub(crate) struct ControllerEntry {
    pub(crate) transaction_id: String,
    _lease: std::fs::File,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Authority {
    schema_version: u8,
    mode: String,
    controller_sha256: String,
    controller_session_id: u32,
    authorization: ReleasePairReconcileAuthorizationV1,
}

#[cfg(windows)]
fn current_session_id() -> Result<u32, String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ProcessIdToSessionId(pid: u32, session: *mut u32) -> i32;
    }
    let mut session = 0;
    if unsafe { ProcessIdToSessionId(std::process::id(), &mut session) } == 0 {
        return Err("stopped controller session is unavailable".into());
    }
    Ok(session)
}
#[cfg(not(windows))]
fn current_session_id() -> Result<u32, String> {
    Err("stopped controller requires a Windows session".into())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Preflight {
    pub schema_version: u8,
    pub mode: String,
    pub confirmation_token: String,
    pub expires_at_unix: u64,
    authorization: Authority,
}

impl Preflight {
    pub(crate) fn candidate_sha256(&self) -> &str {
        &self.authorization.authorization.candidate_sha256
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Claim {
    schema_version: u8,
    mode: String,
    authorization_id: String,
    transaction_id: String,
    state: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ResultRecord {
    pub schema_version: u8,
    pub mode: String,
    pub profile: String,
    pub state: String,
    pub transaction_id: String,
    pub candidate_sha256: String,
    receipt_sha256: Option<String>,
}

fn stopped_path(workspace: &Path, name: &str) -> PathBuf {
    release_pair_control_dir(workspace).join(name)
}

fn checked_mode(schema: u8, mode: &str) -> Result<(), String> {
    if schema != 2 || mode != MODE {
        return Err("stopped release-pair mode is invalid".into());
    }
    Ok(())
}

async fn verify_local_mcp(endpoint: &str, token: &str) -> Result<usize, String> {
    let url = reqwest::Url::parse(endpoint).map_err(|_| "stopped local endpoint invalid")?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("stopped local endpoint scope invalid".into());
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| "stopped MCP client unavailable")?;
    let tools = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});
    let unauth = client
        .post(endpoint)
        .json(&tools)
        .send()
        .await
        .map_err(|_| "stopped unauthenticated probe unavailable")?;
    if unauth.status() != reqwest::StatusCode::UNAUTHORIZED {
        return Err("stopped unauthorized request was not refused".into());
    }
    let initialize = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{
        "protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"catdesk-stopped-qualification","version":"2"}}});
    let mut count = 0;
    for (body, initializing) in [(initialize, true), (tools, false)] {
        let mut response = client
            .post(endpoint)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|_| "stopped authenticated MCP request failed")?;
        if response.status() != reqwest::StatusCode::OK {
            return Err("stopped authenticated response was not200".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "stopped MCP body read failed")?
        {
            if bytes.len() + chunk.len() > crate::tunnel::MCP_SELF_CHECK_MAX_RESPONSE_BYTES {
                return Err("stopped MCP body exceeded bound".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "stopped MCP body invalid")?;
        if value.get("error").is_some() {
            return Err("stopped MCP reported protocol error".into());
        }
        if initializing {
            if !value["result"]["serverInfo"]["name"]
                .as_str()
                .is_some_and(|name| !name.is_empty() && name.len() <= 128)
            {
                return Err("stopped MCP server identity missing".into());
            }
        } else {
            count = crate::tunnel::validate_required_mcp_tools(&value)
                .map_err(|_| "stopped MCP required tools missing")?;
        }
    }
    Ok(count)
}

fn validate_authority(workspace: &Path, authority: &Authority, fresh: bool) -> Result<(), String> {
    checked_mode(authority.schema_version, &authority.mode)?;
    if current_session_id()? != authority.controller_session_id {
        return Err("stopped controller session differs".into());
    }
    let a = &authority.authorization;
    if a.schema_version != 1
        || (fresh && a.expires_at_unix < now_unix())
        || [a.authorization_id.as_str(), a.transaction_id.as_str()]
            .iter()
            .any(|s| s.len() != 32 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("stopped release-pair authority is unavailable".into());
    }
    validate_release_pair_review_freshness(workspace, a, fresh)?;
    for (relative, expected) in [
        (
            "scripts/reconcile-reviewed-catdesk-release-pair.ps1",
            &a.script_sha256,
        ),
        (
            "scripts/catdesk-release-recovery.ps1",
            &a.recovery_helper_sha256,
        ),
    ] {
        let path = canonical_workspace_script(
            workspace,
            &workspace.join(relative),
            "stopped fixed helper",
        )?;
        if sha256_file(&path)? != *expected {
            return Err("stopped fixed helper changed".into());
        }
    }
    let powershell = crate::operator_facade::trusted_windows_powershell(workspace)?;
    let controller =
        std::env::current_exe().map_err(|_| "stopped controller identity unavailable")?;
    if sha256_file(&powershell)? != a.powershell_sha256
        || sha256_file(&controller)? != authority.controller_sha256
    {
        return Err("stopped controller or interpreter changed".into());
    }
    Ok(())
}

pub(crate) fn prepare(workspace: &Path, transaction: &str) -> Result<Preflight, String> {
    prepare_with_template(
        workspace,
        transaction,
        || build_release_pair_reconcile_preflight(workspace),
        |authority| validate_authority(workspace, authority, true),
        || Ok(()),
    )
}

fn prepare_with_template(
    workspace: &Path,
    transaction: &str,
    template: impl FnOnce() -> Result<ReleasePairReconcilePreflightV1, String>,
    admit: impl Fn(&Authority) -> Result<(), String>,
    before_publication: impl FnOnce() -> Result<(), String>,
) -> Result<Preflight, String> {
    let workspace = canonical_directory(workspace, "stopped release-pair workspace")?;
    if release_pair_claim_path(&workspace).exists()
        || release_pair_authorization_path(&workspace).exists()
        || stopped_path(&workspace, "transaction.json").exists()
    {
        return Err("release-pair transaction already exists; inspect its original mode".into());
    }
    if release_pair_preflight_path(&workspace).exists() {
        let previous: Preflight = read_bounded_json(
            &release_pair_preflight_path(&workspace),
            "stopped preflight",
        )?;
        checked_mode(previous.schema_version, &previous.mode)?;
        if previous.authorization.authorization.transaction_id != transaction {
            return Err("stopped prepare transaction differs".into());
        }
        admit(&previous.authorization)?;
        return Ok(previous);
    }
    // Reuse every existing C196/producer/divergent-original prerequisite. The
    // new token and outer schema bind an explicit stopped-only action.
    let mut base = template()?;
    base.authorization.transaction_id = transaction.to_string();
    let authority = Authority {
        schema_version: 2,
        mode: MODE.into(),
        controller_sha256: sha256_file(
            &std::env::current_exe().map_err(|_| "controller unavailable")?,
        )?,
        controller_session_id: current_session_id()?,
        authorization: base.authorization,
    };
    admit(&authority)?;
    let preflight = Preflight {
        schema_version: 2,
        mode: MODE.into(),
        confirmation_token: Uuid::new_v4().simple().to_string(),
        expires_at_unix: authority.authorization.expires_at_unix,
        authorization: authority,
    };
    before_publication()?;
    atomic_json_write(&release_pair_preflight_path(&workspace), &preflight)?;
    Ok(preflight)
}

fn read_authority_claim(workspace: &Path) -> Result<Authority, String> {
    let authority: Authority = read_bounded_json(
        &release_pair_authorization_path(workspace),
        "stopped authority",
    )?;
    checked_mode(authority.schema_version, &authority.mode)?;
    let claim: Claim = read_bounded_json(&release_pair_claim_path(workspace), "stopped claim")?;
    checked_mode(claim.schema_version, &claim.mode)?;
    if claim.state != "STOPPED_CLAIMED_PENDING"
        || claim.authorization_id != authority.authorization.authorization_id
        || claim.transaction_id != authority.authorization.transaction_id
    {
        return Err("stopped release-pair claim differs".into());
    }
    Ok(authority)
}

fn read_recovery_authority(workspace: &Path, transaction: &str) -> Result<Authority, String> {
    if release_pair_claim_path(workspace).exists() {
        return read_authority_claim(workspace);
    }
    let authority: Authority = read_bounded_json(
        &release_pair_authorization_path(workspace),
        "stopped authority",
    )?;
    checked_mode(authority.schema_version, &authority.mode)?;
    if authority.authorization.transaction_id != transaction
        || ["transaction.json", "qualification.json", "result.json"]
            .iter()
            .any(|name| stopped_path(workspace, name).exists())
    {
        return Err("stopped incomplete claim boundary is ambiguous".into());
    }
    Ok(authority)
}

pub(crate) fn result(workspace: &Path) -> Result<Option<ResultRecord>, String> {
    let workspace = canonical_directory(workspace, "stopped release-pair workspace")?;
    if !release_pair_result_path(&workspace).exists() {
        return Ok(None);
    }
    let authority = read_authority_claim(&workspace)?;
    let result: ResultRecord =
        read_bounded_json(&release_pair_result_path(&workspace), "stopped result")?;
    checked_mode(result.schema_version, &result.mode)?;
    if result.profile != PROFILE
        || result.transaction_id != authority.authorization.transaction_id
        || result.candidate_sha256 != authority.authorization.candidate_sha256
        || !matches!(
            result.state.as_str(),
            "QUALIFIED_STOPPED" | "RESTORED_ORIGINAL_STOPPED" | "STOPPED_RECOVERY_REQUIRED"
        )
    {
        return Err("stopped release-pair result differs".into());
    }
    if result.state == MODE {
        let receipt = stopped_path(&workspace, "qualified-stopped.json");
        let journal: serde_json::Value = read_bounded_json(
            &stopped_path(&workspace, "transaction.json"),
            "stopped journal",
        )?;
        let proof: serde_json::Value = read_bounded_json(&receipt, "stopped receipt")?;
        let qualification = stopped_path(&workspace, "qualification.json");
        let quiescence = stopped_path(&workspace, "stopped-quiescence.json");
        let q: serde_json::Value = read_bounded_json(&qualification, "stopped qualification")?;
        let exit: serde_json::Value = read_bounded_json(&quiescence, "stopped exit proof")?;
        let base = &authority.authorization;
        if result.receipt_sha256.as_deref() != Some(sha256_file(&receipt)?.as_str())
            || journal["schemaVersion"] != 2
            || journal["mode"] != MODE
            || journal["phase"] != "COMPLETED"
            || proof["schemaVersion"] != 2
            || proof["mode"] != MODE
            || proof["state"] != MODE
            || proof["transactionId"] != result.transaction_id
            || proof["candidateSha256"] != result.candidate_sha256
            || journal["transactionId"] != base.transaction_id
            || journal["authorizationId"] != base.authorization_id
            || journal["candidateSha256"] != base.candidate_sha256
            || journal["originalBinarySha256"] != base.original_binary_sha256
            || journal["originalSidecarFileSha256"] != base.original_sidecar_file_sha256
            || proof["authorizationId"] != base.authorization_id
            || proof["profile"] != PROFILE
            || proof["qualificationSha256"] != sha256_file(&qualification)?
            || proof["quiescenceSha256"] != sha256_file(&quiescence)?
            || q["schemaVersion"] != 2
            || q["mode"] != MODE
            || q["profile"] != PROFILE
            || q["authorizationId"] != base.authorization_id
            || q["transactionId"] != base.transaction_id
            || q["candidateSha256"] != base.candidate_sha256
            || q["ownerPid"]
                .as_u64()
                .is_none_or(|pid| pid == 0 || pid > u32::MAX as u64)
            || q["ownerStartedAtUtc"].as_str().is_none_or(str::is_empty)
            || q["qualifiedAtUnix"].as_u64().is_none_or(|time| time == 0)
            || q["unauthorizedStatus"] != 401
            || q["authorizedStatus"] != 200
            || q["initializeVerified"] != true
            || q["toolsListVerified"] != true
            || q["toolCount"].as_u64().is_none_or(|count| count < 2)
            || exit["schemaVersion"] != 2
            || exit["mode"] != MODE
            || exit["state"] != "OWNED_TREE_EXITED"
            || exit["authorizationId"] != base.authorization_id
            || exit["transactionId"] != base.transaction_id
            || exit["qualificationSha256"] != sha256_file(&qualification)?
            || exit["candidateJob"]
                != format!(
                    "Local\\CatDesk.ReleasePairStopped.{}.candidate",
                    base.transaction_id
                )
            || exit["helperJob"]
                != format!(
                    "Local\\CatDesk.ReleasePairStopped.{}.helper",
                    base.transaction_id
                )
            || !exact_fields(
                &journal,
                &[
                    "schemaVersion",
                    "mode",
                    "authorizationId",
                    "transactionId",
                    "phase",
                    "candidateSha256",
                    "originalBinarySha256",
                    "originalSidecarFileSha256",
                ],
            )
            || !exact_fields(
                &proof,
                &[
                    "schemaVersion",
                    "mode",
                    "state",
                    "profile",
                    "authorizationId",
                    "transactionId",
                    "candidateSha256",
                    "qualificationSha256",
                    "quiescenceSha256",
                ],
            )
            || !exact_fields(
                &q,
                &[
                    "schemaVersion",
                    "mode",
                    "profile",
                    "authorizationId",
                    "transactionId",
                    "candidateSha256",
                    "ownerPid",
                    "ownerStartedAtUtc",
                    "unauthorizedStatus",
                    "authorizedStatus",
                    "initializeVerified",
                    "toolsListVerified",
                    "toolCount",
                    "qualifiedAtUnix",
                ],
            )
            || !exact_fields(
                &exit,
                &[
                    "schemaVersion",
                    "mode",
                    "state",
                    "authorizationId",
                    "transactionId",
                    "qualificationSha256",
                    "candidateJob",
                    "helperJob",
                ],
            )
        {
            return Err("stopped terminal proof is incomplete".into());
        }
    }
    if result.state == "STOPPED_RECOVERY_REQUIRED" && result.receipt_sha256.is_some() {
        return Err("stopped unknown result claims a receipt".into());
    }
    if result.state == "RESTORED_ORIGINAL_STOPPED" {
        let journal: serde_json::Value = read_bounded_json(
            &stopped_path(&workspace, "transaction.json"),
            "stopped journal",
        )?;
        if journal["schemaVersion"] != 2
            || journal["mode"] != MODE
            || journal["phase"] != "ROLLED_BACK"
            || journal["transactionId"] != result.transaction_id
            || result.receipt_sha256.is_some()
            || sha256_file(&workspace.join("target/release/catdesk.exe"))?
                != authority.authorization.original_binary_sha256
            || sha256_file(&workspace.join("target/release/catdesk.exe.sha256"))?
                != authority.authorization.original_sidecar_file_sha256
        {
            return Err("stopped rollback terminal proof differs".into());
        }
    }
    Ok(Some(result))
}

fn exact_fields(value: &serde_json::Value, fields: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == fields.len() && fields.iter().all(|field| object.contains_key(*field))
    })
}

#[cfg(windows)]
mod native {
    use super::*;
    use crate::reviewed_candidate_owner::OwnedCandidateChild;
    use std::ffi::c_void;
    use std::os::windows::fs::OpenOptionsExt;

    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct ControllerOwner {
        schema_version: u8,
        mode: String,
        transaction_id: String,
        pid: u32,
        creation_time: u64,
        controller_sha256: String,
        session_id: u32,
        controller_job: String,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetProcessTimes(
            handle: *mut c_void,
            created: *mut FileTime,
            exited: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn GetLastError() -> u32;
    }

    fn process_creation(pid: u32) -> Result<Option<u64>, String> {
        let handle = unsafe { OpenProcess(0x0010_1000, 0, pid) };
        if handle.is_null() {
            return if unsafe { GetLastError() } == 87 {
                Ok(None)
            } else {
                Err("stopped controller process observation unavailable".into())
            };
        }
        let result = (|| {
            if unsafe { WaitForSingleObject(handle, 0) } == 0 {
                return Ok(None);
            }
            let (mut created, mut exited, mut kernel, mut user) = (
                FileTime::default(),
                FileTime::default(),
                FileTime::default(),
                FileTime::default(),
            );
            if unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) }
                == 0
            {
                return Err("stopped controller process instance unavailable".into());
            }
            Ok(Some(
                (u64::from(created.high) << 32) | u64::from(created.low),
            ))
        })();
        unsafe { CloseHandle(handle) };
        result
    }

    pub(super) fn enter_controller(
        workspace: &Path,
        kind: &str,
        value: Option<&str>,
    ) -> Result<ControllerEntry, String> {
        let workspace = canonical_directory(workspace, "stopped controller workspace")?;
        let (transaction, session, image) = match kind {
            "prepare" => {
                if release_pair_claim_path(&workspace).exists()
                    || release_pair_authorization_path(&workspace).exists()
                    || stopped_path(&workspace, "transaction.json").exists()
                {
                    return Err("stopped prepare already has a transaction".into());
                }
                let transaction = if release_pair_preflight_path(&workspace).exists() {
                    let preflight: Preflight = read_bounded_json(
                        &release_pair_preflight_path(&workspace),
                        "stopped preflight",
                    )?;
                    checked_mode(preflight.schema_version, &preflight.mode)?;
                    preflight.authorization.authorization.transaction_id
                } else if stopped_path(&workspace, "stopped-owner.json").exists() {
                    let prior: ControllerOwner = read_bounded_json(
                        &stopped_path(&workspace, "stopped-owner.json"),
                        "stopped preparation owner",
                    )?;
                    checked_mode(prior.schema_version, &prior.mode)?;
                    prior.transaction_id
                } else {
                    Uuid::new_v4().simple().to_string()
                };
                (
                    transaction,
                    current_session_id()?,
                    sha256_file(
                        &std::env::current_exe().map_err(|_| "controller image unavailable")?,
                    )?,
                )
            }
            "confirm" => {
                let preflight: Preflight = read_bounded_json(
                    &release_pair_preflight_path(&workspace),
                    "stopped preflight",
                )?;
                checked_mode(preflight.schema_version, &preflight.mode)?;
                if value != Some(preflight.confirmation_token.as_str()) {
                    return Err("stopped confirmation differs".into());
                }
                (
                    preflight.authorization.authorization.transaction_id,
                    preflight.authorization.controller_session_id,
                    preflight.authorization.controller_sha256,
                )
            }
            "resume" => {
                let authority = read_recovery_authority(
                    &workspace,
                    value.ok_or("stopped recovery identity missing")?,
                )?;
                if value != Some(authority.authorization.transaction_id.as_str()) {
                    return Err("stopped recovery transaction differs".into());
                }
                (
                    authority.authorization.transaction_id,
                    authority.controller_session_id,
                    authority.controller_sha256,
                )
            }
            _ => return Err("stopped controller action is invalid".into()),
        };
        if transaction.len() != 32
            || !transaction.bytes().all(|b| b.is_ascii_hexdigit())
            || session != current_session_id()?
            || image
                != sha256_file(
                    &std::env::current_exe().map_err(|_| "controller image unavailable")?,
                )?
        {
            return Err("stopped controller identity or session differs".into());
        }
        let owner = stopped_path(&workspace, "stopped-owner.json");
        let old_exited = || -> Result<(), String> {
            if owner.exists() {
                let prior: ControllerOwner =
                    read_bounded_json(&owner, "stopped controller witness")?;
                checked_mode(prior.schema_version, &prior.mode)?;
                let expected = format!(
                    "Local\\CatDesk.ReleasePairStopped.{transaction}.controller.{}.{}.{}",
                    session, prior.pid, prior.creation_time
                );
                if prior.pid == 0
                    || prior.creation_time == 0
                    || prior.transaction_id != transaction
                    || prior.session_id != session
                    || prior.controller_sha256 != image
                    || prior.controller_job != expected
                    || process_creation(prior.pid)? == Some(prior.creation_time)
                {
                    return Err("original stopped controller exit is unproven".into());
                }
                OwnedCandidateChild::prove_stopped_named_job_quiescent(&prior.controller_job)?;
            } else if ["transaction.json", "qualification.json"]
                .iter()
                .any(|name| stopped_path(&workspace, name).exists())
            {
                return Err("stopped controller witness is missing".into());
            }
            for role in ["candidate", "helper"] {
                OwnedCandidateChild::prove_stopped_named_job_quiescent(&format!(
                    "Local\\CatDesk.ReleasePairStopped.{transaction}.{role}"
                ))?;
            }
            Ok(())
        };
        old_exited()?;
        // No helper/review/git or transaction write precedes this attachment.
        let lifetime =
            OwnedCandidateChild::enter_stopped_controller_lifetime(&transaction, &workspace)?;
        fs::create_dir_all(release_pair_control_dir(&workspace))
            .map_err(|_| "stopped controller control root unavailable")?;
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(stopped_path(&workspace, "stopped-owner.lock"))
            .map_err(|_| "stopped release-pair controller still owns the transaction")?;
        old_exited()?;
        atomic_json_write(
            &owner,
            &ControllerOwner {
                schema_version: 2,
                mode: MODE.into(),
                transaction_id: transaction.clone(),
                pid: lifetime.pid,
                creation_time: lifetime.creation_time,
                controller_sha256: image,
                session_id: lifetime.session_id,
                controller_job: lifetime.job_name,
            },
        )?;
        Ok(ControllerEntry {
            transaction_id: transaction,
            _lease: lease,
        })
    }

    fn job_name(a: &Authority, role: &str) -> String {
        format!(
            "Local\\CatDesk.ReleasePairStopped.{}.{}",
            a.authorization.transaction_id, role
        )
    }

    fn phase(
        workspace: &Path,
        a: &Authority,
        phase: crate::operator_facade::ReleasePairPhase,
        pid: Option<u32>,
        start: Option<&str>,
        expected: &str,
    ) -> Result<ReleasePairScriptResultV1, String> {
        validate_authority(workspace, a, false)?;
        let invocation = crate::operator_facade::release_pair_invocation(
            workspace,
            phase,
            &a.authorization.authorization_id,
            &a.authorization.transaction_id,
            pid,
            start,
        )?;
        let mut command = Command::new(&invocation.program);
        command
            .args(invocation.args)
            .current_dir(workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut owner =
            OwnedCandidateChild::spawn_stopped_named(&mut command, &job_name(a, "helper"))?;
        let stdout = owner
            .child_mut()
            .stdout
            .take()
            .ok_or("stopped helper stdout unavailable")?;
        let stderr = owner
            .child_mut()
            .stderr
            .take()
            .ok_or("stopped helper stderr unavailable")?;
        let read = |stream: Box<dyn Read + Send>| {
            thread::spawn(move || {
                let mut bytes = Vec::new();
                let result = stream.take(8193).read_to_end(&mut bytes);
                (result.is_ok(), bytes)
            })
        };
        let out = read(Box::new(stdout));
        let err = read(Box::new(stderr));
        let deadline = Instant::now() + Duration::from_secs(30);
        let success = loop {
            match owner.child_mut().try_wait() {
                Ok(Some(status)) => break status.success(),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
                _ => break false,
            }
        };
        owner.terminate_tree_and_confirm_quiescent()?;
        let (out_ok, out) = out
            .join()
            .map_err(|_| "stopped helper output unavailable")?;
        let (err_ok, err) = err
            .join()
            .map_err(|_| "stopped helper output unavailable")?;
        if !success || !out_ok || !err_ok || out.len() > 4096 || !err.is_empty() {
            return Err("stopped release-pair phase requires recovery".into());
        }
        let proof: ReleasePairScriptResultV1 =
            serde_json::from_slice(&out).map_err(|_| "stopped helper result is malformed")?;
        if proof.state != expected
            || proof.transaction_id != a.authorization.transaction_id
            || pid.is_some_and(|pid| proof.owner_pid != Some(pid))
        {
            return Err("stopped helper result differs".into());
        }
        Ok(proof)
    }

    fn prove_jobs_empty(a: &Authority) -> Result<(), String> {
        OwnedCandidateChild::prove_stopped_named_job_quiescent(&job_name(a, "candidate"))?;
        OwnedCandidateChild::prove_stopped_named_job_quiescent(&job_name(a, "helper"))?;
        Ok(())
    }

    fn prove_empty(workspace: &Path, a: &Authority) -> Result<(), String> {
        prove_jobs_empty(a)?;
        phase(
            workspace,
            a,
            crate::operator_facade::ReleasePairPhase::StoppedEmpty,
            None,
            None,
            "CANONICAL_OWNER_EMPTY",
        )?;
        Ok(())
    }

    fn qualify(workspace: &Path, a: &Authority) -> Result<(), String> {
        let config = stopped_path(workspace, "qualification-config.toml");
        let mut config_value = crate::state::AppConfig::default();
        config_value.identity.installation_id = Some(a.authorization.transaction_id.clone());
        config_value.mcp.route_id = Some("catdesk-qualified-stopped-v2".into());
        let text =
            toml::to_string(&config_value).map_err(|_| "stopped private config unavailable")?;
        if config.exists() {
            if fs::read_to_string(&config).map_err(|_| "stopped private config unreadable")? != text
            {
                return Err("stopped private config changed".into());
            }
        } else {
            let temporary = config.with_extension(format!("{}.tmp", Uuid::new_v4().simple()));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| "stopped private config unavailable")?;
            file.write_all(text.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|_| "stopped private config write failed")?;
            drop(file);
            fs::rename(temporary, &config)
                .map_err(|_| "stopped private config publication failed")?;
        }
        let token = Uuid::new_v4().simple().to_string();
        let mut command = Command::new(workspace.join("target/release/catdesk.exe"));
        command
            .args([
                "--headless-mcp",
                "--no-ngrok",
                "--no-web-auto",
                "--max-runtime-seconds",
                "60",
                "--host",
                "127.0.0.1",
                "--port",
                "3200",
                "--mode",
                "computer",
                "--tool-mode",
                "multi-tools",
            ])
            .arg("--workspace")
            .arg(workspace)
            .arg("--config-path")
            .arg(&config)
            .args(["--mcp-path", "/catdesk-qualified-stopped-v2/mcp"])
            .env("CATDESK_MCP_AUTH_TOKEN", &token)
            .env_remove("CONTROL_PLANE_API_KEY")
            .env_remove("CATDESK_OPENAI_TUNNEL_ID")
            .current_dir(workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut owner =
            OwnedCandidateChild::spawn_stopped_named(&mut command, &job_name(a, "candidate"))?;
        let outcome = (|| {
            if !wait_child_ready(owner.child_mut(), 3200, Duration::from_secs(15)) {
                return Err("stopped candidate readiness unavailable".into());
            }
            let first = phase(
                workspace,
                a,
                crate::operator_facade::ReleasePairPhase::StoppedProbe,
                Some(owner.pid()),
                None,
                "SERVING_VERIFIED",
            )?;
            let started = first
                .owner_started_at_utc
                .ok_or("stopped candidate instance unavailable")?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "stopped MCP qualification unavailable")?;
            let tools = runtime.block_on(verify_local_mcp(
                "http://127.0.0.1:3200/catdesk-qualified-stopped-v2/mcp",
                &token,
            ))?;
            phase(
                workspace,
                a,
                crate::operator_facade::ReleasePairPhase::StoppedProbe,
                Some(owner.pid()),
                Some(&started),
                "SERVING_VERIFIED",
            )?;
            atomic_json_write(
                &stopped_path(workspace, "qualification.json"),
                &serde_json::json!({
                "schemaVersion":2,"mode":MODE,"profile":PROFILE,"authorizationId":a.authorization.authorization_id,
                "transactionId":a.authorization.transaction_id,"candidateSha256":a.authorization.candidate_sha256,
                "ownerPid":owner.pid(),"ownerStartedAtUtc":started,"unauthorizedStatus":401,
                "authorizedStatus":200,"initializeVerified":true,"toolsListVerified":true,"toolCount":tools,"qualifiedAtUnix":now_unix()}),
            )?;
            Ok::<_, String>(())
        })();
        owner.terminate_tree_and_confirm_quiescent()?;
        prove_empty(workspace, a)?;
        outcome
    }

    pub(super) fn run(
        workspace: &Path,
        token: Option<&str>,
        resume: bool,
        rollback: bool,
    ) -> Result<ResultRecord, String> {
        let workspace = canonical_directory(workspace, "stopped release-pair workspace")?;
        let _entry =
            enter_controller(&workspace, if resume { "resume" } else { "confirm" }, token)?;
        // Reject a legacy/foreign mode before even creating our runtime lock.
        if resume {
            let authority = read_recovery_authority(
                &workspace,
                token.ok_or("stopped recovery identity missing")?,
            )?;
            if token != Some(authority.authorization.transaction_id.as_str()) {
                return Err("stopped recovery transaction differs".into());
            }
            validate_authority(&workspace, &authority, false)?;
        } else {
            let preflight: Preflight = read_bounded_json(
                &release_pair_preflight_path(&workspace),
                "stopped preflight",
            )?;
            checked_mode(preflight.schema_version, &preflight.mode)?;
            validate_authority(&workspace, &preflight.authorization, true)?;
            if token != Some(preflight.confirmation_token.as_str())
                || preflight.expires_at_unix < now_unix()
                || release_pair_claim_path(&workspace).exists()
                || release_pair_authorization_path(&workspace).exists()
            {
                return Err("stopped confirmation expired, differs or was already consumed".into());
            }
        }
        let a = if resume {
            let authority = read_recovery_authority(
                &workspace,
                token.ok_or("stopped recovery identity missing")?,
            )?;
            if token != Some(authority.authorization.transaction_id.as_str()) {
                return Err("stopped recovery transaction differs".into());
            }
            authority
        } else {
            let preflight: Preflight = read_bounded_json(
                &release_pair_preflight_path(&workspace),
                "stopped preflight",
            )?;
            checked_mode(preflight.schema_version, &preflight.mode)?;
            if token != Some(preflight.confirmation_token.as_str())
                || preflight.expires_at_unix < now_unix()
            {
                return Err("stopped confirmation expired or differs".into());
            }
            if release_pair_claim_path(&workspace).exists()
                || release_pair_authorization_path(&workspace).exists()
            {
                return Err("stopped transaction already claimed; use explicit recovery without reconfirmation".into());
            }
            validate_authority(&workspace, &preflight.authorization, true)?;
            if !release_pair_observed_matches(
                &measure_divergent_release_pair(&workspace)?,
                &preflight.authorization.authorization,
            ) {
                return Err("stopped original tuple changed".into());
            }
            atomic_json_write(
                &release_pair_authorization_path(&workspace),
                &preflight.authorization,
            )?;
            let base = &preflight.authorization.authorization;
            atomic_json_write(
                &release_pair_claim_path(&workspace),
                &Claim {
                    schema_version: 2,
                    mode: MODE.into(),
                    authorization_id: base.authorization_id.clone(),
                    transaction_id: base.transaction_id.clone(),
                    state: "STOPPED_CLAIMED_PENDING".into(),
                },
            )?;
            preflight.authorization
        };
        validate_authority(&workspace, &a, !resume)?;
        if resume && !release_pair_claim_path(&workspace).exists() {
            if !release_pair_observed_matches(
                &measure_divergent_release_pair(&workspace)?,
                &a.authorization,
            ) {
                return Err("stopped incomplete claim original tuple changed".into());
            }
            atomic_json_write(
                &release_pair_claim_path(&workspace),
                &Claim {
                    schema_version: 2,
                    mode: MODE.into(),
                    authorization_id: a.authorization.authorization_id.clone(),
                    transaction_id: a.authorization.transaction_id.clone(),
                    state: "STOPPED_CLAIMED_PENDING".into(),
                },
            )?;
        }
        if let Some(prior) = result(&workspace)? {
            if matches!(prior.state.as_str(), MODE | "RESTORED_ORIGINAL_STOPPED") {
                return Ok(prior);
            }
        }
        // Query named Jobs only AFTER the original controller has exited. No
        // observation handle is retained across its kill-on-close boundary.
        prove_jobs_empty(&a)?;
        // Publish the controller's exact process instance before any helper
        // can run. A crash before this record cannot have launched a child.
        phase(
            &workspace,
            &a,
            crate::operator_facade::ReleasePairPhase::StoppedEmpty,
            None,
            None,
            "CANONICAL_OWNER_EMPTY",
        )?;
        atomic_json_write(
            &stopped_path(&workspace, "stopped-recovery-exit.json"),
            &serde_json::json!({
            "schemaVersion":2,"mode":MODE,"state":"OWNED_TREE_EXITED","authorizationId":a.authorization.authorization_id,
            "transactionId":a.authorization.transaction_id,"candidateJob":job_name(&a,"candidate"),"helperJob":job_name(&a,"helper")}),
        )?;
        let operation = (|| {
            if rollback {
                phase(
                    &workspace,
                    &a,
                    crate::operator_facade::ReleasePairPhase::StoppedRollback,
                    None,
                    None,
                    "RESTORED_ORIGINAL_STOPPED",
                )?;
                return Ok(());
            }
            phase(
                &workspace,
                &a,
                crate::operator_facade::ReleasePairPhase::StoppedStage,
                None,
                None,
                "STAGED",
            )?;
            if !stopped_path(&workspace, "qualification.json").exists() {
                qualify(&workspace, &a)?;
            }
            prove_empty(&workspace, &a)?;
            atomic_json_write(
                &stopped_path(&workspace, "stopped-quiescence.json"),
                &serde_json::json!({
                "schemaVersion":2,"mode":MODE,"state":"OWNED_TREE_EXITED","authorizationId":a.authorization.authorization_id,
                "transactionId":a.authorization.transaction_id,"qualificationSha256":sha256_file(&stopped_path(&workspace,"qualification.json"))?,
                "candidateJob":job_name(&a,"candidate"),"helperJob":job_name(&a,"helper")}),
            )?;
            phase(
                &workspace,
                &a,
                crate::operator_facade::ReleasePairPhase::StoppedCommit,
                None,
                None,
                MODE,
            )?;
            Ok::<_, String>(())
        })();
        let record = ResultRecord {
            schema_version: 2,
            mode: MODE.into(),
            profile: PROFILE.into(),
            state: if operation.is_ok() {
                if rollback {
                    "RESTORED_ORIGINAL_STOPPED".into()
                } else {
                    MODE.into()
                }
            } else {
                "STOPPED_RECOVERY_REQUIRED".into()
            },
            transaction_id: a.authorization.transaction_id.clone(),
            candidate_sha256: a.authorization.candidate_sha256.clone(),
            receipt_sha256: if operation.is_ok() && !rollback {
                Some(sha256_file(&stopped_path(
                    &workspace,
                    "qualified-stopped.json",
                ))?)
            } else {
                None
            },
        };
        atomic_json_write(&release_pair_result_path(&workspace), &record)?;
        operation?;
        Ok(record)
    }
}

pub(crate) fn confirm(
    workspace: &Path,
    token: Option<&str>,
    resume: bool,
) -> Result<ResultRecord, String> {
    #[cfg(windows)]
    {
        native::run(workspace, token, resume, false)
    }
    #[cfg(not(windows))]
    {
        let _ = (workspace, token, resume);
        Err("stopped qualification requires Windows ownership".into())
    }
}

pub(crate) fn enter_controller(
    workspace: &Path,
    kind: &str,
    value: Option<&str>,
) -> Result<ControllerEntry, String> {
    #[cfg(windows)]
    {
        native::enter_controller(workspace, kind, value)
    }
    #[cfg(not(windows))]
    {
        let _ = (workspace, kind, value);
        Err("stopped controller requires Windows ownership".into())
    }
}

pub(crate) fn rollback(workspace: &Path, transaction: &str) -> Result<ResultRecord, String> {
    #[cfg(windows)]
    {
        native::run(workspace, Some(transaction), true, true)
    }
    #[cfg(not(windows))]
    {
        let _ = (workspace, transaction);
        Err("stopped rollback requires Windows ownership".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_template() -> ReleasePairReconcilePreflightV1 {
        let a = "a".repeat(32);
        let tx = "b".repeat(32);
        let hash = "c".repeat(64);
        let authorization:ReleasePairReconcileAuthorizationV1=serde_json::from_value(serde_json::json!({
            "schemaVersion":1,"authorizationId":a,"transactionId":tx,"expiresAtUnix":now_unix()+120,
            "reviewedPreflightSha256":hash,"reviewedAuthorizationId":a,"candidateRelativePath":"build/candidate.exe",
            "candidateSha256":hash,"originalBinarySha256":hash,"originalSidecarFileSha256":hash,"originalSidecarValue":hash,
            "slotABinarySha256":hash,"slotASidecarFileSha256":hash,"slotAManifestSha256":hash,"originalPointerSha256":hash,
            "scriptSha256":hash,"recoveryHelperSha256":hash,"powershellSha256":hash})).unwrap();
        ReleasePairReconcilePreflightV1 {
            schema_version: 1,
            confirmation_token: "d".repeat(32),
            expires_at_unix: now_unix() + 120,
            authorization,
        }
    }

    #[cfg(windows)]
    #[test]
    fn stopped_prepare_publishes_only_v2_and_refuses_foreign_preflight() {
        let root = std::env::temp_dir().join(format!("stopped-prepare-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let template = fixture_template();
        let tx = "e".repeat(32);
        let failure = prepare_with_template(
            &root,
            &tx,
            || Ok(template.clone()),
            |_| Ok(()),
            || {
                assert!(!release_pair_preflight_path(&root).exists());
                Err("fixture interrupted before publication".into())
            },
        );
        assert!(failure.is_err());
        assert!(!release_pair_preflight_path(&root).exists());
        let preflight = prepare_with_template(
            &root,
            &tx,
            || Ok(template.clone()),
            |_| Ok(()),
            || {
                assert!(!release_pair_preflight_path(&root).exists());
                Ok(())
            },
        )
        .unwrap();
        let published: serde_json::Value =
            read_bounded_json(&release_pair_preflight_path(&root), "fixture").unwrap();
        assert_eq!(published["schemaVersion"], 2);
        assert_eq!(published["mode"], MODE);
        assert_eq!(preflight.authorization.authorization.transaction_id, tx);
        let owner = stopped_path(&root, "stopped-owner.json");
        atomic_json_write(&owner,&serde_json::json!({"schemaVersion":2,"mode":MODE,"transactionId":tx,
            "pid":42,"creationTime":123,"controllerSha256":preflight.authorization.controller_sha256,
            "sessionId":preflight.authorization.controller_session_id,"controllerJob":"frozen fixture witness"})).unwrap();
        let before_stopped = fs::read(release_pair_preflight_path(&root)).unwrap();
        let before_owner = fs::read(&owner).unwrap();
        assert!(prepare_release_pair_reconcile(&root).is_err());
        assert_eq!(
            fs::read(release_pair_preflight_path(&root)).unwrap(),
            before_stopped
        );
        assert_eq!(fs::read(owner).unwrap(), before_owner);
        atomic_json_write(&release_pair_preflight_path(&root), &template).unwrap();
        let before = sha256_file(&release_pair_preflight_path(&root)).unwrap();
        let reached = std::cell::Cell::new(false);
        assert!(
            prepare_with_template(
                &root,
                &tx,
                || {
                    reached.set(true);
                    Ok(template)
                },
                |_| Ok(()),
                || Ok(())
            )
            .is_err()
        );
        assert!(!reached.get());
        assert_eq!(
            sha256_file(&release_pair_preflight_path(&root)).unwrap(),
            before
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn stopped_rust_serialized_authority_is_accepted_by_fixed_powershell_reader() {
        let root = std::env::temp_dir().join(format!("stopped-wire-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("build")).unwrap();
        fs::create_dir_all(release_pair_control_dir(&root)).unwrap();
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        let script = repository.join("scripts/reconcile-reviewed-catdesk-release-pair.ps1");
        let helper = repository.join("scripts/catdesk-release-recovery.ps1");
        let program = crate::operator_facade::trusted_windows_powershell(&root).unwrap();
        let candidate = root.join("build/candidate.exe");
        fs::write(&candidate, b"inert serialized-authority fixture").unwrap();
        let mut base = fixture_template().authorization;
        base.script_sha256 = sha256_file(&script).unwrap();
        base.recovery_helper_sha256 = sha256_file(&helper).unwrap();
        base.powershell_sha256 = sha256_file(&program).unwrap();
        base.candidate_sha256 = sha256_file(&candidate).unwrap();
        let authority = Authority {
            schema_version: 2,
            mode: MODE.into(),
            controller_sha256: "e".repeat(64),
            controller_session_id: current_session_id().unwrap(),
            authorization: base.clone(),
        };
        atomic_json_write(&release_pair_authorization_path(&root), &authority).unwrap();
        atomic_json_write(
            &release_pair_claim_path(&root),
            &Claim {
                schema_version: 2,
                mode: MODE.into(),
                authorization_id: base.authorization_id.clone(),
                transaction_id: base.transaction_id.clone(),
                state: "STOPPED_CLAIMED_PENDING".into(),
            },
        )
        .unwrap();
        let quoted = |path: &Path| path.to_string_lossy().replace('\'', "''");
        let driver = root.join("read-wire.ps1");
        fs::write(&driver,format!("$ErrorActionPreference='Stop'\n. '{}' -LibraryOnly\n$paths=Get-ReconcilePaths '{}'\n$proof=Get-StoppedReconcileAuthority $paths '{}' '{}'\nif($proof.Outer.mode -cne 'QUALIFIED_STOPPED'){{throw 'wire mode differs'}}\nWrite-Output 'SERDE_AUTHORITY_ACCEPTED'\n",quoted(&script),quoted(&root),base.authorization_id,base.transaction_id)).unwrap();
        let observed = crate::operator_facade::run_fixed_powershell_invocation_blocking(
            crate::operator_facade::RepairInvocation {
                program,
                args: vec![
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-File".into(),
                    driver.to_string_lossy().into_owned(),
                ],
                workspace: root.clone(),
            },
        )
        .unwrap();
        assert!(
            observed.success,
            "fixed authority reader refused actual Rust payload"
        );
        assert!(observed.stdout.contains("SERDE_AUTHORITY_ACCEPTED"));
        assert!(observed.stderr.is_empty());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn stopped_schema_and_mode_never_accept_serving_authority() {
        assert!(checked_mode(1, MODE).is_err());
        assert!(checked_mode(2, "SERVING").is_err());
        assert!(checked_mode(2, MODE).is_ok());
        assert!(
            serde_json::from_value::<Claim>(serde_json::json!({"schemaVersion":1,
            "authorizationId":"a","transactionId":"b","state":"CLAIMED_PENDING"}))
            .is_err()
        );
        assert!(!exact_fields(
            &serde_json::json!({"schemaVersion":2,"mode":MODE,"extra":true}),
            &["schemaVersion", "mode"]
        ));
    }

    #[tokio::test]
    async fn stopped_mcp_qualification_requires_exact_status_identity_and_tools() {
        use axum::{
            Json, Router,
            http::{HeaderMap, StatusCode},
            routing::post,
        };
        for (status, identity, tools, accepted) in [
            (StatusCode::OK, true, true, true),
            (StatusCode::CREATED, true, true, false),
            (StatusCode::OK, false, true, false),
            (StatusCode::OK, true, false, false),
        ] {
            let router=Router::new().route("/mcp",post(move|headers:HeaderMap,Json(body):Json<serde_json::Value>|async move {
                if headers.get("authorization").and_then(|v|v.to_str().ok())!=Some("Bearer fixture-token-for-stopped-qualification") {
                    return (StatusCode::UNAUTHORIZED,Json(serde_json::json!({"error":"unauthorized"})));
                }
                let value=if body["method"]=="initialize" {
                    if identity {serde_json::json!({"result":{"serverInfo":{"name":"CatDesk"}}})}
                    else {serde_json::json!({"result":{"serverInfo":{}}})}
                } else if tools {serde_json::json!({"result":{"tools":[{"name":"catdesk_instruction"},{"name":"delegated_run_list"}]}})}
                else {serde_json::json!({"result":{"tools":[]}})};
                (status,Json(value))
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
            let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
            let proof =
                verify_local_mcp(&endpoint, "fixture-token-for-stopped-qualification").await;
            task.abort();
            let _ = task.await;
            assert_eq!(proof.is_ok(), accepted);
            if accepted {
                assert_eq!(proof.unwrap(), 2);
            }
        }
        assert!(
            verify_local_mcp(
                "http://example.invalid/mcp",
                "fixture-token-for-stopped-qualification"
            )
            .await
            .is_err()
        );
    }

    #[test]
    fn stopped_terminal_result_requires_exact_journal_and_qualification_chain() {
        let root = std::env::temp_dir().join(format!("stopped-result-{}", Uuid::new_v4()));
        fs::create_dir_all(release_pair_control_dir(&root)).unwrap();
        let a = "a".repeat(32);
        let tx = "b".repeat(32);
        let digest = "c".repeat(64);
        let base = serde_json::json!({"schemaVersion":1,"authorizationId":a,"transactionId":tx,"expiresAtUnix":1,
            "reviewedPreflightSha256":digest,"reviewedAuthorizationId":a,"candidateRelativePath":"build/catdesk.exe",
            "candidateSha256":digest,"originalBinarySha256":digest,"originalSidecarFileSha256":digest,"originalSidecarValue":digest,
            "slotABinarySha256":digest,"slotASidecarFileSha256":digest,"slotAManifestSha256":digest,"originalPointerSha256":digest,
            "scriptSha256":digest,"recoveryHelperSha256":digest,"powershellSha256":digest});
        atomic_json_write(
            &release_pair_authorization_path(&root),
            &serde_json::json!({"schemaVersion":2,"mode":MODE,
            "controllerSha256":digest,"controllerSessionId":1,"authorization":base}),
        )
        .unwrap();
        atomic_json_write(
            &release_pair_claim_path(&root),
            &Claim {
                schema_version: 2,
                mode: MODE.into(),
                authorization_id: a.clone(),
                transaction_id: tx.clone(),
                state: "STOPPED_CLAIMED_PENDING".into(),
            },
        )
        .unwrap();
        let mut journal = serde_json::json!({"schemaVersion":2,"mode":MODE,"authorizationId":a,"transactionId":tx,"phase":"COMPLETED",
            "candidateSha256":digest,"originalBinarySha256":digest,"originalSidecarFileSha256":digest});
        atomic_json_write(&stopped_path(&root, "transaction.json"), &journal).unwrap();
        let qualification = serde_json::json!({"schemaVersion":2,"mode":MODE,"profile":PROFILE,"authorizationId":a,"transactionId":tx,
            "candidateSha256":digest,"ownerPid":42,"ownerStartedAtUtc":"2026-10-25T00:00:00Z","unauthorizedStatus":401,
            "authorizedStatus":200,"initializeVerified":true,"toolsListVerified":true,"toolCount":79,"qualifiedAtUnix":1792886400});
        atomic_json_write(&stopped_path(&root, "qualification.json"), &qualification).unwrap();
        let qhash = sha256_file(&stopped_path(&root, "qualification.json")).unwrap();
        let exit = serde_json::json!({"schemaVersion":2,"mode":MODE,"state":"OWNED_TREE_EXITED","authorizationId":a,"transactionId":tx,
            "qualificationSha256":qhash,"candidateJob":format!("Local\\CatDesk.ReleasePairStopped.{tx}.candidate"),"helperJob":format!("Local\\CatDesk.ReleasePairStopped.{tx}.helper")});
        atomic_json_write(&stopped_path(&root, "stopped-quiescence.json"), &exit).unwrap();
        let receipt = serde_json::json!({"schemaVersion":2,"mode":MODE,"state":MODE,"profile":PROFILE,"authorizationId":a,"transactionId":tx,
            "candidateSha256":digest,"qualificationSha256":qhash,"quiescenceSha256":sha256_file(&stopped_path(&root,"stopped-quiescence.json")).unwrap()});
        atomic_json_write(&stopped_path(&root, "qualified-stopped.json"), &receipt).unwrap();
        atomic_json_write(
            &release_pair_result_path(&root),
            &ResultRecord {
                schema_version: 2,
                mode: MODE.into(),
                profile: PROFILE.into(),
                state: MODE.into(),
                transaction_id: tx.clone(),
                candidate_sha256: digest,
                receipt_sha256: Some(
                    sha256_file(&stopped_path(&root, "qualified-stopped.json")).unwrap(),
                ),
            },
        )
        .unwrap();
        assert_eq!(result(&root).unwrap().unwrap().state, MODE);
        journal["transactionId"] = serde_json::json!("f".repeat(32));
        atomic_json_write(&stopped_path(&root, "transaction.json"), &journal).unwrap();
        assert!(result(&root).is_err());
        journal["transactionId"] = serde_json::json!(tx);
        atomic_json_write(&stopped_path(&root, "transaction.json"), &journal).unwrap();
        let mut changed = qualification;
        changed["authorizedStatus"] = serde_json::json!(201);
        atomic_json_write(&stopped_path(&root, "qualification.json"), &changed).unwrap();
        assert!(result(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
