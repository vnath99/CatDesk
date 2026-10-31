//! Independent bounded project listeners using the existing installed image.
//! Startup shares the release gate briefly; serving retains immutable pins,
//! never a registry/task lease or the global release gate.
use super::*;

fn command(
    control: &Path,
    binary: &Path,
    config: &Path,
    selection: &crate::project_mcp::Selection,
    seconds: u64,
    token: &str,
) -> Command {
    let mut c = Command::new(binary);
    c.current_dir(control)
        .env("WORKSPACE_ROOT", control)
        .env("CATDESK_MCP_AUTH_TOKEN", token)
        .env_remove("CONTROL_PLANE_API_KEY")
        .env_remove("CATDESK_OPENAI_TUNNEL_ID")
        .args(["--headless-mcp", "--host", "127.0.0.1", "--port", "0"])
        .arg("--workspace")
        .arg(control)
        .arg("--config-path")
        .arg(config)
        .args(["--mcp-path", PROJECT_ROUTE, "--mode", "computer"])
        .args(["--tool-mode", "read-only", "--no-ngrok", "--no-web-auto"])
        .arg("--max-runtime-seconds")
        .arg(seconds.to_string())
        .arg("--project-id")
        .arg(&selection.project_id)
        .arg("--project-target-generation")
        .arg(selection.target_generation.to_string())
        .arg("--project-target-sha256")
        .arg(&selection.target_sha256)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}

fn scope(value: &Value, expected: &Value) -> Result<(), String> {
    for field in [
        "projectId",
        "targetGeneration",
        "targetSha256",
        "executionWorkspace",
    ] {
        if value[field] != expected[field] || value[field].is_null() {
            return Err("INSTALLED_PROJECT_SCOPE_CHANGED".into());
        }
    }
    if value["access"] != "READ_ONLY"
        || value["nativeWriterClaimed"] != false
        || value["conversationOriginAuthenticated"] != false
        || !value["assignedTask"].is_null()
    {
        return Err("INSTALLED_PROJECT_SCOPE_CHANGED".into());
    }
    Ok(())
}

fn ready(
    value: &Value,
    control: &Path,
    config: &Path,
    expected: &Value,
    seconds: u64,
) -> Result<String, String> {
    let actual = value["workspace"]
        .as_str()
        .and_then(|s| Path::new(s).canonicalize().ok())
        .map(crate::command::normalize_windows_verbatim_path);
    if value["status"] != "ready"
        || value["mode"] != "Computer"
        || value["tool_mode"] != "read-only"
        || value["maximum_runtime_seconds"] != seconds
        || value["web_auto_scheduler_enabled"] != false
        || value["auth"]["scheme"] != "bearer"
        || value["auth"]["token_redacted"] != true
        || actual.as_deref() != Some(control)
        || value["config_path"].as_str().map(Path::new) != Some(config)
    {
        return Err("INSTALLED_PROJECT_READY_INVALID".into());
    }
    scope(&value["project_scope"], expected)?;
    let endpoint = value["mcp_url"]
        .as_str()
        .ok_or("INSTALLED_PROJECT_READY_INVALID")?;
    let url = reqwest::Url::parse(endpoint).map_err(|_| "INSTALLED_PROJECT_READY_INVALID")?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none_or(|port| port == 0)
        || url.path() != PROJECT_ROUTE
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("INSTALLED_PROJECT_READY_INVALID".into());
    }
    Ok(endpoint.into())
}

async fn verify_mcp(endpoint: &str, token: &str, expected: &Value) -> Result<(), String> {
    let target = reqwest::Url::parse(endpoint).map_err(|_| "INSTALLED_PROJECT_READY_INVALID")?;
    if target.scheme() != "http"
        || target.host_str() != Some("127.0.0.1")
        || target.port().is_none_or(|port| port == 0)
        || target.path() != PROJECT_ROUTE
        || !target.username().is_empty()
        || target.password().is_some()
        || target.query().is_some()
        || target.fragment().is_some()
    {
        return Err("INSTALLED_PROJECT_READY_INVALID".into());
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| "INSTALLED_BACKEND_HTTP_UNAVAILABLE")?;
    let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});
    let denied = client
        .post(endpoint)
        .json(&list)
        .send()
        .await
        .map_err(|_| "INSTALLED_BACKEND_HTTP_UNAVAILABLE")?;
    if denied.status() != reqwest::StatusCode::UNAUTHORIZED {
        return Err("INSTALLED_BACKEND_AUTH_UNPROVEN".into());
    }
    for (body, kind) in [
        (
            json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"catdesk-installed-project","version":"1"}}}),
            "initialize",
        ),
        (list, "list"),
        (
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"catdesk_project_scope","arguments":{}}}),
            "scope",
        ),
    ] {
        let mut response = client
            .post(endpoint)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|_| "INSTALLED_BACKEND_HTTP_UNAVAILABLE")?;
        if response.status() != reqwest::StatusCode::OK {
            return Err("INSTALLED_BACKEND_AUTH_UNPROVEN".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "INSTALLED_BACKEND_HTTP_UNAVAILABLE")?
        {
            if bytes.len() + chunk.len() > 1048576 {
                return Err("INSTALLED_BACKEND_HTTP_OVERSIZED".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "INSTALLED_BACKEND_HTTP_INVALID")?;
        if value.get("error").is_some() {
            return Err("INSTALLED_PROJECT_SCOPE_CHANGED".into());
        }
        match kind {
            "initialize" => scope(&value["result"]["projectScope"], expected)?,
            "scope" => scope(&value["result"]["structuredContent"], expected)?,
            _ => {
                let tools = value["result"]["tools"]
                    .as_array()
                    .ok_or("INSTALLED_BACKEND_CATALOG_INVALID")?;
                if tools.len() != READ_TOOLS.len() + 1
                    || READ_TOOLS
                        .iter()
                        .copied()
                        .chain(["catdesk_project_scope"])
                        .any(|name| tools.iter().filter(|tool| tool["name"] == name).count() != 1)
                {
                    return Err("INSTALLED_BACKEND_CATALOG_INVALID".into());
                }
            }
        }
    }
    Ok(())
}

fn read_trust(root: &Path, helper: &Path) -> String {
    format!(
        "{} {}",
        prefix(root, helper),
        selected_release_proof_script()
    )
}

fn admission_command(root: &Path, helper: &Path, wait_seconds: u64) -> String {
    let acquire = format!(
        "$until=[DateTime]::UtcNow.AddSeconds({wait_seconds});$lease=$null;while($null -eq $lease){{try{{$lease=Enter-CatDeskStoppedLifecycleLease $root}}catch{{$cause=$_.Exception;while($null -ne $cause.InnerException){{$cause=$cause.InnerException}};$code=$cause.HResult -band 65535;if($code -notin @(32,33) -or [DateTime]::UtcNow -ge $until){{throw}};Start-Sleep -Milliseconds 25}}}};"
    );
    trust_command(root, helper).replacen(
        "$lease=Enter-CatDeskStoppedLifecycleLease $root;",
        &acquire,
        1,
    )
}

fn first_admission(reader: &Reader, wait_seconds: u64) -> Result<Value, String> {
    let bytes = reader
        .first
        .recv_timeout(Duration::from_secs(wait_seconds + 15))
        .map_err(|_| "INSTALLED_BACKEND_READY_TIMEOUT")??;
    if bytes.is_empty() || bytes.len() > 65536 {
        return Err("INSTALLED_BACKEND_READY_INVALID".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "INSTALLED_BACKEND_READY_INVALID".into())
}

#[cfg(windows)]
fn owner(
    root: &Path,
    endpoint: &str,
    pid: u32,
    binary: &Path,
    expected: &str,
) -> Result<Value, String> {
    let port = reqwest::Url::parse(endpoint)
        .ok()
        .and_then(|url| url.port())
        .ok_or("INSTALLED_PROJECT_READY_INVALID")?;
    let hash_helper = regular(
        root,
        "scripts/reconcile-reviewed-catdesk-release-pair.ps1",
        131072,
    )?;
    let script = format!(
        "$ErrorActionPreference='Stop';$stage='LIBRARY';try{{. {} -LibraryOnly;$stage='PROCESS';$p=Get-Process -Id {pid} -ErrorAction Stop;$stage='IMAGE_PATH';if(-not $p.Path -or [IO.Path]::GetFullPath($p.Path) -ine {}){{throw [InvalidOperationException]::new('image')}};$stage='IMAGE_HASH';if((Get-ReconcileHash $p.Path) -cne '{expected}'){{throw [InvalidOperationException]::new('hash')}};$stage='LISTENER';$l=@(Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction Stop);if($l.Count -ne 1 -or $l[0].OwningProcess -ne {pid} -or [string]$l[0].LocalAddress -cne '127.0.0.1'){{throw [InvalidOperationException]::new('listener')}};$stage='RESULT';[pscustomobject]@{{schemaVersion=1;status='VERIFIED';proof=[pscustomobject]@{{pid={pid};port={port};startedAtUtc=$p.StartTime.ToUniversalTime().ToString('o');imageSha256='{expected}'}}}}|ConvertTo-Json -Compress}}catch{{$cause=$_.Exception;while($null -ne $cause.InnerException){{$cause=$cause.InnerException}};[pscustomobject]@{{schemaVersion=1;status='REFUSED';stage=$stage;errorType=$cause.GetType().Name;hresult=[int]$cause.HResult}}|ConvertTo-Json -Compress}}",
        quote(&hash_helper),
        quote(binary)
    );
    let mut cmd = ps_command(root, script)?;
    cmd.stdin(Stdio::null());
    let mut helper = OwnedCandidateChild::spawn(&mut cmd)
        .map_err(|_| "INSTALLED_PROJECT_OWNER_HELPER_SPAWN_FAILED")?;
    let reader = readers(&mut helper)?;
    let response = first(&reader).map_err(|reason| {
        if reason == "INSTALLED_BACKEND_READY_TIMEOUT" {
            "INSTALLED_PROJECT_OWNER_HELPER_TIMEOUT".to_owned()
        } else {
            "INSTALLED_PROJECT_OWNER_HELPER_OUTPUT_INVALID".to_owned()
        }
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let exited = loop {
        match helper.child_mut().try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            _ => break false,
        }
    };
    helper
        .terminate_tree_and_confirm_quiescent()
        .map_err(|_| "INSTALLED_PROJECT_OWNER_HELPER_CLEANUP_UNPROVEN")?;
    finish_readers(reader, "").map_err(|_| "INSTALLED_PROJECT_OWNER_HELPER_OUTPUT_INVALID")?;
    // A timeout remains a timeout even if the helper finishes during cleanup.
    let response = response?;
    if !exited {
        return Err("INSTALLED_PROJECT_OWNER_HELPER_EXIT_FAILED".into());
    }
    let value = owner_response(response)?;
    if value["pid"] != pid
        || value["port"] != port
        || value["imageSha256"] != expected
        || value["startedAtUtc"].as_str().is_none_or(str::is_empty)
    {
        return Err("INSTALLED_PROJECT_OWNER_UNPROVEN".into());
    }
    Ok(value)
}

fn owner_response(response: Value) -> Result<Value, String> {
    if response["schemaVersion"] != 1 {
        return Err("INSTALLED_PROJECT_OWNER_HELPER_OUTPUT_INVALID".into());
    }
    match response["status"].as_str() {
        Some("VERIFIED") if response.as_object().is_some_and(|object| object.len() == 3) => {
            Ok(response["proof"].clone())
        }
        Some("REFUSED") if response.as_object().is_some_and(|object| object.len() == 5) => {
            let stage = response["stage"]
                .as_str()
                .ok_or("INSTALLED_PROJECT_OWNER_HELPER_OUTPUT_INVALID")?;
            let kind = response["errorType"]
                .as_str()
                .ok_or("INSTALLED_PROJECT_OWNER_HELPER_OUTPUT_INVALID")?;
            let hresult = response["hresult"]
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .ok_or("INSTALLED_PROJECT_OWNER_HELPER_OUTPUT_INVALID")?;
            if !matches!(
                stage,
                "LIBRARY" | "PROCESS" | "IMAGE_PATH" | "IMAGE_HASH" | "LISTENER" | "RESULT"
            ) || kind.is_empty()
                || kind.len() > 64
                || !kind.bytes().all(|byte| byte.is_ascii_alphanumeric())
            {
                return Err("INSTALLED_PROJECT_OWNER_HELPER_OUTPUT_INVALID".into());
            }
            Err(format!(
                "INSTALLED_PROJECT_OWNER_REFUSED_{stage}_{}_{:08X}",
                kind.to_ascii_uppercase(),
                hresult as u32
            ))
        }
        _ => Err("INSTALLED_PROJECT_OWNER_HELPER_OUTPUT_INVALID".into()),
    }
}

pub(super) fn verify_pins(expected: &[(PathBuf, String)]) -> Result<(), String> {
    for (path, digest) in expected {
        if &hash(path)? != digest {
            return Err("INSTALLED_BACKEND_INPUT_CHANGED".into());
        }
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn pin_inputs(
    root: &Path,
    control: &Path,
    proof: &Value,
) -> Result<(Vec<fs::File>, Vec<(PathBuf, String)>, PathBuf), String> {
    use std::os::windows::fs::OpenOptionsExt;
    let runtime = catdesk_wake::development_runtime::root_for_workspace(control)?
        .ok_or("INSTALLED_PROJECT_DEVELOPMENT_AUTHORITY_REQUIRED")?;
    let runtime = crate::command::normalize_windows_verbatim_path(runtime);
    let authority_path = regular(control, ".catdesk/web-development/authority.json", 32768)?;
    let authority: Value = serde_json::from_slice(
        &fs::read(&authority_path).map_err(|_| "INSTALLED_BACKEND_FILE_UNAVAILABLE")?,
    )
    .map_err(|_| "INSTALLED_PROJECT_DEVELOPMENT_AUTHORITY_REQUIRED")?;
    let package = authority["packageId"]
        .as_str()
        .ok_or("INSTALLED_PROJECT_DEVELOPMENT_AUTHORITY_REQUIRED")?;
    if package.len() > 160
        || Path::new(package).components().count() != 1
        || package == "."
        || package == ".."
    {
        return Err("INSTALLED_PROJECT_DEVELOPMENT_AUTHORITY_REQUIRED".into());
    }
    let config = regular(control, ".catdesk/web-development/catdesk.toml", 65536)?;
    let mut paths = vec![config.clone(), authority_path];
    for relative in [
        "target/release/catdesk.exe",
        "target/release/catdesk.exe.sha256",
        ".catdesk/release-recovery/current.json",
        ".catdesk/release-recovery/slot-b/catdesk.exe",
        ".catdesk/release-recovery/slot-b/catdesk.exe.sha256",
        ".catdesk/release-recovery/slot-b/manifest.json",
        "scripts/catdesk-release-recovery.ps1",
        "scripts/reconcile-reviewed-catdesk-release-pair.ps1",
    ] {
        paths.push(regular(root, relative, 134217728)?);
    }
    for name in [
        "authorization.json",
        "claim.json",
        "transaction.json",
        "qualification.json",
        "stopped-quiescence.json",
        "qualified-stopped.json",
    ] {
        paths.push(regular(
            root,
            &format!(".catdesk/release-pair-control/{name}"),
            12288,
        )?);
    }
    // Legacy migration inputs remain retained for the previous version, but
    // the current selected artifact and its native promotion receipt must also
    // stay pinned while this independent project endpoint serves.
    let slot = proof["slot"]
        .as_str()
        .filter(|slot| matches!(*slot, "slot-a" | "slot-b"))
        .ok_or("INSTALLED_BACKEND_AUTHORITY_UNPROVEN")?;
    for leaf in ["catdesk.exe", "catdesk.exe.sha256", "manifest.json"] {
        paths.push(regular(
            root,
            &format!(".catdesk/release-recovery/{slot}/{leaf}"),
            134217728,
        )?);
    }
    if proof["authorityKind"] == "reviewed_promotion" {
        paths.push(regular(
            root,
            ".catdesk/promotion-recovery/reviewed-promotion.json",
            2048,
        )?);
    }
    for relative in [
        ".catdesk/web-development/prepare-attempt.json",
        ".catdesk/wake-bridge/owner.json",
    ] {
        paths.push(regular(control, relative, 4096)?);
    }
    for relative in ["current.json", "activation.json", "runtime/python.exe"] {
        paths.push(regular(&runtime, relative, 134217728)?);
    }
    for name in [
        "CatDeskBinagotchy.exe",
        "CatDeskWakeHost.exe",
        "adapter.py",
        "wake_bridge.py",
        "manifest.json",
    ] {
        paths.push(regular(
            &runtime,
            &format!("versions/{package}/{name}"),
            134217728,
        )?);
    }
    paths.push(super::super::trusted_windows_powershell(root)?);
    paths.sort();
    paths.dedup();
    let expected = paths
        .iter()
        .map(|path| Ok((path.clone(), hash(path)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let pins = paths
        .iter()
        .map(|path| {
            OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(path)
                .map_err(|_| String::from("INSTALLED_BACKEND_INPUT_PIN_FAILED"))
        })
        .collect::<Result<Vec<_>, String>>()?;
    verify_pins(&expected)?;
    catdesk_wake::development_runtime::root_for_workspace(control)?
        .ok_or("INSTALLED_PROJECT_DEVELOPMENT_AUTHORITY_REQUIRED")?;
    crate::state::AppConfig::require_headless_no_migration(&config)
        .map_err(|_| "INSTALLED_PROJECT_CONFIG_MIGRATION_REQUIRED")?;
    Ok((pins, expected, config))
}

#[cfg(windows)]
fn release_holder(
    holder: &mut Option<OwnedCandidateChild>,
    watch: &mut Option<HolderWatch>,
    reader: &mut Option<Reader>,
    tree_exited: &mut bool,
) -> Result<(), String> {
    // Disarm only after the child, all pins and authenticated scope are proven.
    // The dedicated controller and child Jobs remain armed throughout.
    drop(watch.take());
    let mut owned = holder.take().ok_or("INSTALLED_PROJECT_HANDOFF_FAILED")?;
    let mut input = owned
        .child_mut()
        .stdin
        .take()
        .ok_or("INSTALLED_PROJECT_HANDOFF_FAILED")?;
    input
        .write_all(b"STOP\n")
        .and_then(|()| input.flush())
        .map_err(|_| "INSTALLED_PROJECT_HANDOFF_FAILED")?;
    drop(input);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match owned
            .child_mut()
            .try_wait()
            .map_err(|_| "INSTALLED_PROJECT_HANDOFF_FAILED")?
        {
            Some(status) if status.success() => break,
            Some(_) => return Err("INSTALLED_PROJECT_HANDOFF_FAILED".into()),
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            None => return Err("INSTALLED_PROJECT_HANDOFF_FAILED".into()),
        }
    }
    owned.terminate_tree_and_confirm_quiescent()?;
    *tree_exited = true;
    finish_readers(reader.take().ok_or("INSTALLED_PROJECT_HANDOFF_FAILED")?, "")
}

#[cfg(windows)]
fn cleanup_holder(
    holder: &mut Option<OwnedCandidateChild>,
    reader: &mut Option<Reader>,
    tree_exited: &mut bool,
) -> Result<(), String> {
    let stopped = if let Some(mut owned) = holder.take() {
        if let Some(mut input) = owned.child_mut().stdin.take() {
            let _ = input.write_all(b"STOP\n");
        }
        let result = owned.terminate_tree_and_confirm_quiescent();
        if result.is_ok() {
            *tree_exited = true;
        }
        result
    } else if *tree_exited {
        Ok(())
    } else {
        Err("INSTALLED_PROJECT_HOLDER_EXIT_UNPROVEN".into())
    };
    let output = reader
        .take()
        .map_or(Ok(()), |reader| finish_readers(reader, ""));
    stopped.and(output)
}

#[cfg(windows)]
pub(super) fn run_owned(
    workspace: &Path,
    options: &ProjectOptions,
    token: &str,
) -> Result<Value, String> {
    let root = workspace
        .canonicalize()
        .map(crate::command::normalize_windows_verbatim_path)
        .map_err(|_| "INSTALLED_BACKEND_ROOT_UNAVAILABLE")?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let lifetime = OwnedCandidateChild::enter_installed_backend_lifetime(&id, &root)?;
    let _deadline_guard = DeadlineGuard::arm(options.maximum_seconds);
    let deadline = Instant::now() + Duration::from_secs(options.maximum_seconds);
    let control = options
        .control
        .canonicalize()
        .map(crate::command::normalize_windows_verbatim_path)
        .map_err(|_| "INSTALLED_PROJECT_CONTROL_UNAVAILABLE")?;
    catdesk_wake::development_runtime::root_for_workspace(&control)?
        .ok_or("INSTALLED_PROJECT_DEVELOPMENT_AUTHORITY_REQUIRED")?;
    let binary = regular(&root, "target/release/catdesk.exe", 134217728)?;
    let mut expected_hash = String::new();
    let recovery = regular(&root, "scripts/catdesk-release-recovery.ps1", 131072)?;
    if hash(&recovery)?
        != format!(
            "{:x}",
            Sha256::digest(include_bytes!("../scripts/catdesk-release-recovery.ps1"))
        )
    {
        return Err("INSTALLED_BACKEND_HELPER_UNQUALIFIED".into());
    }
    let auth: Value = serde_json::from_slice(
        &fs::read(root.join(".catdesk/release-pair-control/authorization.json"))
            .map_err(|_| "INSTALLED_BACKEND_AUTHORITY_UNAVAILABLE")?,
    )
    .map_err(|_| "INSTALLED_BACKEND_AUTHORITY_INVALID")?;
    let reconcile = regular(
        &root,
        "scripts/reconcile-reviewed-catdesk-release-pair.ps1",
        131072,
    )?;
    let powershell = super::super::trusted_windows_powershell(&root)?;
    if auth["authorization"]["scriptSha256"] != hash(&reconcile)?
        || auth["authorization"]["recoveryHelperSha256"] != hash(&recovery)?
        || auth["authorization"]["powershellSha256"] != hash(&powershell)?
    {
        return Err("INSTALLED_BACKEND_HELPER_UNQUALIFIED".into());
    }
    use std::os::windows::fs::OpenOptionsExt;
    let fixed_inputs = [&recovery, &reconcile, &powershell]
        .into_iter()
        .map(|path| Ok((path.clone(), hash(path)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let _fixed_pins = fixed_inputs
        .iter()
        .map(|(path, _)| {
            OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(path)
                .map_err(|_| String::from("INSTALLED_BACKEND_INPUT_PIN_FAILED"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    verify_pins(&fixed_inputs)?;
    if auth["authorization"]["scriptSha256"] != hash(&reconcile)?
        || auth["authorization"]["recoveryHelperSha256"] != hash(&recovery)?
        || auth["authorization"]["powershellSha256"] != hash(&powershell)?
    {
        return Err("INSTALLED_BACKEND_HELPER_UNQUALIFIED".into());
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "INSTALLED_BACKEND_HTTP_UNAVAILABLE")?;
    let context = rt.block_on(crate::project_mcp::ProjectContext::bind_with_task(
        &control,
        options.selection.clone(),
        None,
        options.maximum_seconds,
    ))?;
    let expected_scope = context.metadata();
    let admission_seconds = deadline
        .saturating_duration_since(Instant::now())
        .as_secs()
        .checked_sub(30)
        .filter(|seconds| *seconds > 0)
        .ok_or("INSTALLED_BACKEND_INSUFFICIENT_BUDGET")?
        .min(30);
    let mut holder_command = ps_command(
        &root,
        admission_command(&root, &recovery, admission_seconds),
    )?;
    holder_command.stdin(Stdio::piped());
    let mut held = OwnedCandidateChild::spawn(&mut holder_command)?;
    let mut holder_reader = Some(readers(&mut held)?);
    let mut holder = Some(held);
    let mut watch = None;
    let mut directory = None;
    let mut child_exited = false;
    let mut holder_released = false;
    let mut holder_tree_exited = false;
    let mut endpoint = None;
    let outcome: Result<(), String> = (|| {
        let proof = first_admission(
            holder_reader
                .as_ref()
                .ok_or("INSTALLED_PROJECT_HANDOFF_FAILED")?,
            admission_seconds,
        )?;
        live(holder.as_mut().ok_or("INSTALLED_PROJECT_HANDOFF_FAILED")?)?;
        watch = Some(HolderWatch::start(
            holder.as_mut().ok_or("INSTALLED_PROJECT_HANDOFF_FAILED")?,
        )?);
        expected_hash = hash(&binary)?;
        validate_trust(&proof, &expected_hash)?;
        let (_pins, expected_inputs, config) = pin_inputs(&root, &control, &proof)?;
        expected_hash = hash(&binary)?;
        validate_trust(&proof, &expected_hash)?;
        // Complete byte pins are already held. This second supported read
        // binds the selected pointer/slot/receipt to those exact pinned bytes.
        let pinned_proof = ps_once(&root, read_trust(&root, &recovery))?;
        validate_trust(&pinned_proof, &expected_hash)?;
        if pinned_proof != proof {
            return Err("INSTALLED_BACKEND_AUTHORITY_CHANGED".into());
        }
        verify_pins(&expected_inputs)?;
        let history = history_snapshot(&root)?;
        let path = run_directory(&root, &id)?;
        directory = Some(path.clone());
        save(
            &path.join("owner.json"),
            &json!({"schemaVersion":1,"runId":id,"profile":"INSTALLED_PROJECT_READ_ONLY","controllerPid":lifetime.pid,"controllerCreation":lifetime.creation_time,"controllerSession":lifetime.session_id,"controllerJob":lifetime.job_name,"installedSha256":expected_hash,"configSha256":hash(&config)?,"maximumRuntimeSeconds":options.maximum_seconds,"projectScope":expected_scope,"controlWorkspace":control,"nativeWriterClaimed":false}),
        )?;
        let child_seconds = deadline
            .saturating_duration_since(Instant::now())
            .as_secs()
            .checked_sub(30)
            .filter(|s| *s > 0)
            .ok_or("INSTALLED_BACKEND_INSUFFICIENT_BUDGET")?;
        let mut cmd = command(
            &control,
            &binary,
            &config,
            &options.selection,
            child_seconds,
            token,
        );
        live(holder.as_mut().ok_or("INSTALLED_PROJECT_HANDOFF_FAILED")?)?;
        let mut child = OwnedCandidateChild::spawn(&mut cmd)?;
        let reader = readers(&mut child)?;
        let started = Instant::now();
        let child_outcome: Result<(), String> = (|| {
            let advertised = first(&reader)?;
            let url = ready(
                &advertised,
                &control,
                &config,
                &expected_scope,
                child_seconds,
            )?;
            endpoint = Some(url.clone());
            live(&mut child)?;
            live(holder.as_mut().ok_or("INSTALLED_PROJECT_HANDOFF_FAILED")?)?;
            let before = owner(&root, &url, child.pid(), &binary, &expected_hash)?;
            rt.block_on(verify_mcp(&url, token, &expected_scope))?;
            live(&mut child)?;
            live(holder.as_mut().ok_or("INSTALLED_PROJECT_HANDOFF_FAILED")?)?;
            let after = owner(&root, &url, child.pid(), &binary, &expected_hash)?;
            if before != after {
                return Err("INSTALLED_PROJECT_OWNER_UNPROVEN".into());
            }
            verify_pins(&expected_inputs)?;
            release_holder(
                &mut holder,
                &mut watch,
                &mut holder_reader,
                &mut holder_tree_exited,
            )?;
            holder_released = true;
            live(&mut child)?;
            let output = json!({"status":"INSTALLED_PROJECT_READY","profile":"INSTALLED_PROJECT_READ_ONLY","runId":id,"pid":child.pid(),"startedAtUtc":before["startedAtUtc"],"installedSha256":expected_hash,"mcpUrl":url,"commandMaximumSeconds":options.maximum_seconds,"maximumRuntimeSeconds":child_seconds,"projectScope":expected_scope,"readOnly":true,"globalStartupLeaseReleased":true,"webAutoSchedulerEnabled":false,"nativeDaemon":false,"unattended":false});
            save(&path.join("ready.json"), &output)?;
            println!("{output}");
            std::io::stdout()
                .flush()
                .map_err(|_| "INSTALLED_BACKEND_OUTPUT_UNAVAILABLE")?;
            loop {
                if let Some(status) = child
                    .child_mut()
                    .try_wait()
                    .map_err(|_| "INSTALLED_BACKEND_WAIT_FAILED")?
                {
                    return if status.success() {
                        Ok(())
                    } else {
                        Err("INSTALLED_BACKEND_CHILD_FAILED".into())
                    };
                }
                if started.elapsed() > Duration::from_secs(child_seconds + 5) {
                    return Err("INSTALLED_BACKEND_OWNER_DEADLINE".into());
                }
                thread::sleep(Duration::from_millis(50));
            }
        })();
        child.terminate_tree_and_confirm_quiescent()?;
        child_exited = true;
        finish_readers(reader, token)?;
        // Only this endpoint must be empty. A peer project may still serve.
        if let Some(url) = &endpoint {
            let port = reqwest::Url::parse(url)
                .ok()
                .and_then(|url| url.port())
                .ok_or("INSTALLED_PROJECT_READY_INVALID")?;
            let empty = ps_once(
                &root,
                format!(
                    "$ErrorActionPreference='Stop';$n=@([Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpListeners()|Where-Object Port -eq {port}).Count;[pscustomobject]@{{empty=($n -eq 0)}}|ConvertTo-Json -Compress"
                ),
            )?;
            if empty["empty"] != true {
                return Err("INSTALLED_PROJECT_PORT_NOT_EMPTY".into());
            }
        }
        verify_pins(&expected_inputs)?;
        if history_snapshot(&root)? != history {
            return Err("INSTALLED_BACKEND_HISTORY_CHANGED".into());
        }
        child_outcome
    })();
    drop(watch.take());
    let cleanup = cleanup_holder(&mut holder, &mut holder_reader, &mut holder_tree_exited);
    let error = outcome
        .as_ref()
        .err()
        .cloned()
        .or_else(|| cleanup.as_ref().err().cloned());
    let result = json!({"status":if error.is_none(){"INSTALLED_PROJECT_EXITED"}else{"ATTENTION"},"profile":"INSTALLED_PROJECT_READ_ONLY","runId":id,"installedSha256":expected_hash,"projectScope":expected_scope,"ownedTreeExited":child_exited && cleanup.is_ok(),"globalStartupLeaseReleased":holder_released,"errorCode":error,"nativeDaemon":false,"unattended":false});
    if let Some(path) = directory {
        save(&path.join("result.json"), &result)?;
    }
    outcome?;
    cleanup?;
    Ok(result)
}

#[cfg(not(windows))]
pub(super) fn run_owned(_: &Path, _: &ProjectOptions, _: &str) -> Result<Value, String> {
    Err("INSTALLED_BACKEND_WINDOWS_REQUIRED".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn actual_owner_proof_accepts_owned_listener_and_reports_wrong_hash_stage() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}{PROJECT_ROUTE}", listener.local_addr().unwrap());
        let image = std::env::current_exe()
            .unwrap()
            .canonicalize()
            .map(crate::command::normalize_windows_verbatim_path)
            .unwrap();
        let root = std::env::current_dir().unwrap();
        let digest = hash(&image).unwrap();
        let proof = owner(&root, &endpoint, std::process::id(), &image, &digest).unwrap();
        assert_eq!(proof["pid"], std::process::id());
        assert_eq!(proof["imageSha256"], digest);
        let reason = owner(
            &root,
            &endpoint,
            std::process::id(),
            &image,
            &"0".repeat(64),
        )
        .unwrap_err();
        assert!(reason.starts_with("INSTALLED_PROJECT_OWNER_REFUSED_IMAGE_HASH_"));
        assert!(!reason.contains(&image.to_string_lossy().to_string()));
    }

    #[test]
    fn owner_diagnostics_reject_unbounded_or_unrecognized_fields() {
        let value = json!({"schemaVersion":1,"status":"REFUSED","stage":"LISTENER","errorType":"Win32Exception","hresult":-2147467259i64});
        assert!(
            owner_response(value.clone())
                .unwrap_err()
                .starts_with("INSTALLED_PROJECT_OWNER_REFUSED_LISTENER_WIN32EXCEPTION_")
        );
        for (field, bad) in [
            ("stage", json!("C:/private/path")),
            ("errorType", json!("private error message")),
            ("hresult", json!(4294967296u64)),
        ] {
            let mut changed = value.clone();
            changed[field] = bad;
            assert_eq!(
                owner_response(changed).unwrap_err(),
                "INSTALLED_PROJECT_OWNER_HELPER_OUTPUT_INVALID"
            );
        }
        let mut extra = value;
        extra["message"] = json!("private path");
        assert_eq!(
            owner_response(extra).unwrap_err(),
            "INSTALLED_PROJECT_OWNER_HELPER_OUTPUT_INVALID"
        );
    }

    #[cfg(windows)]
    #[test]
    fn holder_handoff_success_and_output_failure_never_reuse_consumed_owner() {
        for overflow in [false, true] {
            let mut c = Command::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
            let after = if overflow {
                "[Console]::Out.Write(('x'*70000));"
            } else {
                ""
            };
            c.args(["-NoProfile", "-NonInteractive", "-Command", &format!("[Console]::Out.WriteLine('{{}}');if([Console]::In.ReadLine() -cne 'STOP'){{exit 2}};{after}")])
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
            let mut owned = OwnedCandidateChild::spawn(&mut c).unwrap();
            let mut reader = Some(readers(&mut owned).unwrap());
            first(reader.as_ref().unwrap()).unwrap();
            let mut owned = Some(owned);
            let mut watch = None;
            let mut exited = false;
            let handoff = release_holder(&mut owned, &mut watch, &mut reader, &mut exited);
            assert_eq!(handoff.is_ok(), !overflow);
            assert!(exited && owned.is_none() && reader.is_none());
            assert!(cleanup_holder(&mut owned, &mut reader, &mut exited).is_ok());
        }
    }

    #[test]
    fn startup_contention_wait_is_bounded_and_does_not_retry_authority_checks() {
        let text = admission_command(Path::new("C:/root"), Path::new("C:/root/helper.ps1"), 7);
        assert!(text.contains("AddSeconds(7)"));
        assert!(text.contains("$code -notin @(32,33)"));
        assert_eq!(
            text.matches("Enter-CatDeskStoppedLifecycleLease").count(),
            1
        );
        assert_eq!(
            text.matches("Test-CatDeskStoppedReconciliationStartupReady")
                .count(),
            1
        );
        assert!(
            text.find("while($null -eq $lease)").unwrap()
                < text
                    .find("Test-CatDeskStoppedReconciliationStartupReady")
                    .unwrap()
        );
    }

    fn scope_fixture(id: &str) -> Value {
        json!({"projectId":id,"targetGeneration":1,"targetSha256":"a".repeat(64),"executionWorkspace":format!("C:/projects/{id}"),"access":"READ_ONLY","nativeWriterClaimed":false,"conversationOriginAuthenticated":false,"assignedTask":null})
    }

    #[test]
    fn project_scope_rejects_cross_project_witness_and_write_claims() {
        let expected = scope_fixture("alpha");
        assert!(scope(&expected, &expected).is_ok());
        for (key, value) in [
            ("projectId", json!("beta")),
            ("executionWorkspace", json!("C:/other")),
            ("targetGeneration", json!(2)),
            ("targetSha256", json!("b".repeat(64))),
            ("access", json!("WRITE")),
            ("nativeWriterClaimed", json!(true)),
            ("conversationOriginAuthenticated", json!(true)),
            ("assignedTask", json!({"id":"task"})),
        ] {
            let mut changed = expected.clone();
            changed[key] = value;
            assert!(scope(&changed, &expected).is_err(), "{key}");
        }
    }

    #[test]
    fn project_command_uses_only_existing_scoped_read_only_entry() {
        let selection = crate::project_mcp::Selection {
            project_id: "alpha".into(),
            target_generation: 1,
            target_sha256: "a".repeat(64),
        };
        let c = command(
            Path::new("C:/control"),
            Path::new("C:/installed/catdesk.exe"),
            Path::new("C:/control/.catdesk/web-development/catdesk.toml"),
            &selection,
            60,
            "private-token",
        );
        let args = c
            .get_args()
            .map(|v| v.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.windows(2).any(|pair| pair == ["--port", "0"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--project-id", "alpha"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--tool-mode", "read-only"])
        );
        assert!(args.iter().any(|arg| arg == "--no-web-auto"));
        assert!(!args.iter().any(|arg| arg.contains("private-token")
            || arg == "--project-task-contract"
            || arg == "--catdesk-daemon"));
    }

    #[test]
    fn project_ready_binds_exact_loopback_scope_without_requiring_canonical_port() {
        let root = std::env::temp_dir().join(format!("project-ready-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let root = root
            .canonicalize()
            .map(crate::command::normalize_windows_verbatim_path)
            .unwrap();
        let config = root.join("catdesk.toml");
        let expected = scope_fixture("alpha");
        let value = json!({"status":"ready","mode":"Computer","tool_mode":"read-only","maximum_runtime_seconds":60,"web_auto_scheduler_enabled":false,"auth":{"scheme":"bearer","token_redacted":true},"workspace":root,"config_path":config,"project_scope":expected,"mcp_url":"http://127.0.0.1:48123/project/mcp"});
        assert!(ready(&value, &root, &config, &expected, 60).is_ok());
        for url in [
            "http://localhost:48123/project/mcp",
            "http://127.0.0.1/project/mcp",
            "http://127.0.0.1:48123/wrong",
            "http://user@127.0.0.1:48123/project/mcp",
        ] {
            let mut changed = value.clone();
            changed["mcp_url"] = json!(url);
            assert!(ready(&changed, &root, &config, &expected, 60).is_err());
        }
        let mut changed = value.clone();
        changed["project_scope"] = scope_fixture("beta");
        assert!(ready(&changed, &root, &config, &expected, 60).is_err());
        fs::remove_dir(root).unwrap();
    }

    #[tokio::test]
    async fn project_readiness_refusal_does_not_block_independent_peer() {
        use axum::{
            Json, Router,
            http::{HeaderMap, StatusCode},
            routing::post,
        };
        let expected = scope_fixture("alpha");
        let mut peers = Vec::new();
        for correct in [false, true] {
            let answer = if correct {
                expected.clone()
            } else {
                scope_fixture("foreign")
            };
            let app=Router::new().route(PROJECT_ROUTE,post(move|headers:HeaderMap,Json(body):Json<Value>|{let answer=answer.clone();async move{
                if headers.get("authorization").and_then(|h|h.to_str().ok())!=Some("Bearer project-test-token-000000000") {return (StatusCode::UNAUTHORIZED,Json(json!({"error":"unauthorized"})));}
                let value=match body["method"].as_str(){Some("initialize")=>json!({"result":{"projectScope":answer}}),Some("tools/list")=>json!({"result":{"tools":READ_TOOLS.iter().copied().chain(["catdesk_project_scope"]).map(|name|json!({"name":name})).collect::<Vec<_>>()}}),_=>json!({"result":{"structuredContent":answer}})};
                (StatusCode::OK,Json(value))
            }}));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}{PROJECT_ROUTE}", listener.local_addr().unwrap());
            let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            peers.push((url, task));
        }
        let (refused, progress) = tokio::join!(
            verify_mcp(&peers[0].0, "project-test-token-000000000", &expected),
            verify_mcp(&peers[1].0, "project-test-token-000000000", &expected)
        );
        assert!(refused.is_err());
        assert!(progress.is_ok());
        assert!(
            verify_mcp(&peers[1].0, "project-test-token-000000000", &expected)
                .await
                .is_ok()
        );
        for (_, task) in peers {
            task.abort();
            let _ = task.await;
        }
    }

    #[cfg(windows)]
    #[test]
    fn independent_reader_pins_hold_release_bytes_until_last_project_exits() {
        use std::os::windows::fs::OpenOptionsExt;
        let path = std::env::temp_dir().join(format!("project-pin-{}", uuid::Uuid::new_v4()));
        fs::write(&path, b"trusted").unwrap();
        let a = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let b = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        drop(a);
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        drop(b);
        assert!(OpenOptions::new().write(true).open(&path).is_ok());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn changed_or_missing_pinned_input_is_refused_without_reconstruction() {
        let path = std::env::temp_dir().join(format!("project-input-{}", uuid::Uuid::new_v4()));
        fs::write(&path, b"trusted").unwrap();
        let original = vec![(path.clone(), hash(&path).unwrap())];
        assert!(verify_pins(&original).is_ok());
        fs::write(&path, b"changed").unwrap();
        assert!(verify_pins(&original).is_err());
        fs::remove_file(&path).unwrap();
        assert!(verify_pins(&original).is_err());
    }
}
