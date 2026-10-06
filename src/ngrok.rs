use crate::openai_tunnel::{
    OPENAI_TUNNEL_READINESS_TIMEOUT, OfficialRuntimeRecoveryStateV1, OfficialRuntimeStatus,
    RuntimeMonitorConfig, RuntimeMonitorState, TunnelClientDiscoveryOptions,
    build_runtime_connect_command, classify_official_runtime_recovery_state,
    connect_official_runtime, credential_environment_present, default_user_tools_dir,
    discover_tunnel_client, monitor_health_from_status, official_runtime_status,
    probe_tunnel_client_readiness, run_tunnel_client_doctor, should_attempt_recovery,
    spawn_tunnel_client_run, trusted_tunnel_client_executable_fingerprint,
    tunnel_id_environment_present, verified_official_runtime_recovery_identity,
};
use crate::state::{SharedState, load_ngrok_authtoken, user_home_dir};
use crate::tunnel::{
    TransportHealth, TransportHealthSnapshot, TunnelMode, connection_fingerprint,
    external_public_mcp_url, public_no_auth_warning, redact_full_mcp_url,
};
use ngrok::prelude::*;
use reqwest::Url;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const EXISTING_RUNTIME_NOT_READY_WARNING: &str =
    "Official tunnel-client runtime exists but is not ready; CatDesk will monitor it";

fn recovery_is_eligible_for_official_runtime(
    health: TransportHealth,
    runtime_is_running: bool,
    local_mcp_ready: bool,
    auto_recover: bool,
    credential_present: bool,
    tunnel_id_present: bool,
) -> bool {
    if !auto_recover || !local_mcp_ready || !credential_present || !tunnel_id_present {
        return false;
    }
    matches!(health, TransportHealth::Disconnected)
        || (matches!(health, TransportHealth::Degraded) && runtime_is_running)
}

fn clear_existing_runtime_not_ready_warning(warnings: &mut Vec<String>) {
    warnings.retain(|warning| warning != EXISTING_RUNTIME_NOT_READY_WARNING);
}

pub async fn start_transport(state: SharedState) -> Result<(), String> {
    let mode = {
        let app = state.lock().await;
        app.tunnel_config.mode
    };
    match mode {
        TunnelMode::ManagedEphemeralNgrok => start(state).await,
        TunnelMode::ManagedStableNgrok => Err(
            "managed_stable_ngrok is unavailable because T-0025C0 did not prove an account-assigned stable domain; no ephemeral fallback was started"
                .into(),
        ),
        TunnelMode::ExternalTunnel => configure_external_tunnel(state).await,
        TunnelMode::OpenaiSecureTunnel => configure_openai_secure_tunnel(state).await,
    }
}

async fn configure_openai_secure_tunnel(state: SharedState) -> Result<(), String> {
    let (config, workspace_root) = {
        let app = state.lock().await;
        (app.openai_tunnel_config.clone(), app.workspace_root.clone())
    };
    if config.profile_name.trim().is_empty() {
        set_transport_health(
            &state,
            TransportHealth::BlockedMissingProfile,
            "OpenAI tunnel profile name is missing",
        )
        .await;
        return Err("openai_secure_tunnel requires openai_tunnel.profile_name".into());
    }
    if config.process_mode.is_external_foreground() {
        let mut app = state.lock().await;
        app.ngrok_running = false;
        app.ngrok_url = None;
        app.transport_health =
            TransportHealthSnapshot::configured_unverified(app.tunnel_config.remote_self_check);
        app.transport_health.warnings.push(
            "OpenAI Secure MCP Tunnel external mode configured; CatDesk did not launch tunnel-client or inspect credentials"
                .into(),
        );
        app.log(
            "INFO",
            "OpenAI Secure MCP Tunnel external mode configured; operator owns tunnel-client".into(),
        );
        // External foreground ownership has no authoritative runtime status
        // surface.  Do not infer readiness from a legacy configured admin URL.
        drop(app);
        return Ok(());
    }

    let home = user_home_dir().map_err(|error| error.to_string())?;
    let explicit_client_path = config.client_path.as_ref().map(PathBuf::from);
    let discovery = TunnelClientDiscoveryOptions {
        explicit_path: explicit_client_path.clone(),
        path_var: if explicit_client_path.is_some() {
            None
        } else {
            std::env::var_os("PATH")
        },
        user_tools_dir: if explicit_client_path.is_some() {
            home.join(".catdesk")
                .join("tools")
                .join("__explicit_client_path_only__")
        } else {
            default_user_tools_dir(&home)
        },
        known_paths: Vec::new(),
        forbidden_roots: vec![PathBuf::from(&workspace_root)],
    };
    let metadata = match discover_tunnel_client(&discovery).await {
        Ok(metadata) => metadata,
        Err(error) => {
            set_transport_health(
                &state,
                TransportHealth::BlockedMissingClient,
                "official tunnel-client not found or not runnable",
            )
            .await;
            return Err(error.to_string());
        }
    };

    match config.process_mode {
        mode if mode.is_external_foreground() => {
            unreachable!("external mode returned before discovery")
        }
        mode if mode.uses_official_runtime() => {
            configure_official_runtime_mode(state, metadata.path).await
        }
        mode if mode.uses_legacy_direct_managed() => {
            if !credential_environment_present(|name| std::env::var_os(name)) {
                set_transport_health(
                    &state,
                    TransportHealth::BlockedMissingCredential,
                    "CONTROL_PLANE_API_KEY is not present in the CatDesk process environment",
                )
                .await;
                return Err(
                    "openai_secure_tunnel requires CONTROL_PLANE_API_KEY in the process environment"
                        .into(),
                );
            }
            if let Err(error) = run_tunnel_client_doctor(&metadata.path, &config.profile_name).await
            {
                set_transport_health(
                    &state,
                    TransportHealth::Failed,
                    "tunnel-client doctor failed; inspect the operator-owned profile",
                )
                .await;
                return Err(error.to_string());
            }
            let child = match spawn_tunnel_client_run(&metadata.path, &config.profile_name) {
                Ok(child) => child,
                Err(error) => {
                    set_transport_health(
                        &state,
                        TransportHealth::Failed,
                        "failed to start CatDesk-owned tunnel-client process",
                    )
                    .await;
                    return Err(error.to_string());
                }
            };
            let mut app = state.lock().await;
            app.ngrok_running = false;
            app.ngrok_url = None;
            app.openai_tunnel_child = Some(child);
            app.transport_health =
                TransportHealthSnapshot::configured_unverified(app.tunnel_config.remote_self_check);
            app.transport_health.health = TransportHealth::Connecting;
            app.transport_health.warnings.push(
                "CatDesk started tunnel-client in managed mode and will stop only this child process"
                    .into(),
            );
            app.log(
                "INFO",
                "OpenAI Secure MCP Tunnel managed process started with redacted profile identity"
                    .into(),
            );
            drop(app);
            refresh_openai_readiness_from_admin_url(&state).await;
            Ok(())
        }
        _ => Err("unsupported OpenAI tunnel process mode".into()),
    }
}

async fn configure_official_runtime_mode(
    state: SharedState,
    client_path: PathBuf,
) -> Result<(), String> {
    let (alias, profile, auto_connect, auto_recover, local_mcp_url) = {
        let app = state.lock().await;
        (
            app.openai_tunnel_config.runtime_alias.clone(),
            app.openai_tunnel_config.profile_name.clone(),
            app.openai_tunnel_config.auto_connect,
            app.openai_tunnel_config.auto_recover,
            format!(
                "http://{}:{}{}",
                app.mcp_bind_host,
                app.port,
                app.mcp_path()
            ),
        )
    };
    let trusted_client_identity = trusted_tunnel_client_executable_fingerprint(&client_path)
        .map_err(|_| "trusted official runtime client identity is unavailable".to_string())?;
    match official_runtime_status(&client_path, &alias).await {
        Ok(status) if status.fully_ready() && status.health_base_url.is_some() => {
            let mut app = state.lock().await;
            app.ngrok_running = false;
            app.ngrok_url = None;
            app.openai_tunnel_config.admin_ui_url = status.admin_ui_url.clone();
            app.transport_health =
                TransportHealthSnapshot::configured_unverified(app.tunnel_config.remote_self_check);
            // The monitor is the sole place that may promote this to
            // CONNECTED_VERIFIED after dynamic /healthz, /readyz, and local MCP
            // checks all pass.
            app.transport_health.health = TransportHealth::Connecting;
            app.transport_health.local_mcp = "NOT_CHECKED".into();
            app.transport_health.redacted_reason =
                Some("awaiting authoritative dynamic health and local MCP readiness".into());
            app.transport_health.warnings.push(
                "Official tunnel-client runtime was already running; CatDesk attached monitoring without creating a duplicate"
                    .into(),
            );
            app.log(
                "INFO",
                "OpenAI Secure MCP official runtime already ready".into(),
            );
        }
        Ok(status) if status.process_running => {
            let local_mcp_ready = crate::tunnel::run_mcp_endpoint_self_check(
                &local_mcp_url,
                None,
                crate::tunnel::MCP_SELF_CHECK_TIMEOUT,
                false,
            )
            .await
            .is_ok();
            let recovery_identity = verified_official_runtime_recovery_identity(
                &client_path,
                &trusted_client_identity,
                &alias,
                &profile,
                &status,
            )
            .ok();
            let mut app = state.lock().await;
            app.ngrok_running = false;
            app.ngrok_url = None;
            app.openai_tunnel_config.admin_ui_url = status.admin_ui_url.clone();
            app.transport_health =
                TransportHealthSnapshot::configured_unverified(app.tunnel_config.remote_self_check);
            app.transport_health.health = TransportHealth::Connecting;
            app.transport_health.redacted_reason = Some(if !local_mcp_ready {
                "OFFICIAL_RUNTIME_REATTACH_WAITING_LOCAL_MCP".into()
            } else if !auto_recover {
                "OFFICIAL_RUNTIME_REATTACH_DISABLED".into()
            } else if recovery_identity.is_none() {
                "OFFICIAL_RUNTIME_IDENTITY_UNAVAILABLE".into()
            } else if !tunnel_id_environment_present(|name| std::env::var_os(name)) {
                "OFFICIAL_RUNTIME_TUNNEL_REFERENCE_UNAVAILABLE".into()
            } else if !credential_environment_present(|name| std::env::var_os(name)) {
                "OFFICIAL_RUNTIME_CREDENTIAL_REFERENCE_UNAVAILABLE".into()
            } else {
                "OFFICIAL_RUNTIME_REATTACH_PENDING".into()
            });
            app.transport_health
                .warnings
                .push(EXISTING_RUNTIME_NOT_READY_WARNING.into());
            let reconnect_now = local_mcp_ready
                && auto_recover
                && recovery_identity.is_some()
                && tunnel_id_environment_present(|name| std::env::var_os(name))
                && credential_environment_present(|name| std::env::var_os(name));
            if reconnect_now {
                app.log(
                    "WARN",
                    "Attempting bounded official runtime reattach through the configured alias"
                        .into(),
                );
            }
            drop(app);
            if reconnect_now {
                if let Ok(tunnel_id) = std::env::var("CATDESK_OPENAI_TUNNEL_ID") {
                    if connect_official_runtime(&client_path, &alias, &tunnel_id, &local_mcp_url)
                        .await
                        .is_err()
                    {
                        let mut app = state.lock().await;
                        app.transport_health.health = TransportHealth::Degraded;
                        app.transport_health.redacted_reason =
                            Some("OFFICIAL_RUNTIME_REATTACH_FAILED".into());
                    }
                }
            }
        }
        Ok(_) | Err(_) => {
            if !auto_connect {
                set_transport_health(
                    &state,
                    TransportHealth::ConfiguredUnverified,
                    "official runtime is absent and auto_connect is disabled",
                )
                .await;
            } else if !tunnel_id_environment_present(|name| std::env::var_os(name)) {
                set_transport_health(
                    &state,
                    TransportHealth::BlockedMissingTunnelId,
                    "CATDESK_OPENAI_TUNNEL_ID is not present in the CatDesk process environment",
                )
                .await;
            } else if !credential_environment_present(|name| std::env::var_os(name)) {
                set_transport_health(
                    &state,
                    TransportHealth::BlockedMissingCredential,
                    "CONTROL_PLANE_API_KEY is not present in the CatDesk process environment",
                )
                .await;
            } else {
                let local_mcp_ready = crate::tunnel::run_mcp_endpoint_self_check(
                    &local_mcp_url,
                    None,
                    crate::tunnel::MCP_SELF_CHECK_TIMEOUT,
                    false,
                )
                .await
                .is_ok();
                if !local_mcp_ready {
                    let mut app = state.lock().await;
                    app.transport_health = TransportHealthSnapshot::configured_unverified(
                        app.tunnel_config.remote_self_check,
                    );
                    app.transport_health.health = TransportHealth::Connecting;
                    app.transport_health.local_mcp = "FAILED".into();
                    app.transport_health.redacted_reason = Some(
                        "waiting for local CatDesk MCP readiness before official runtime connect"
                            .into(),
                    );
                    app.log(
                        "INFO",
                        "Official runtime connect deferred until local CatDesk MCP is ready".into(),
                    );
                    drop(app);
                    start_openai_runtime_monitor_if_needed(
                        state,
                        client_path,
                        alias,
                        profile,
                        trusted_client_identity,
                    )
                    .await;
                    return Ok(());
                }
                let tunnel_id = std::env::var("CATDESK_OPENAI_TUNNEL_ID")
                    .map_err(|_| "CATDESK_OPENAI_TUNNEL_ID is missing".to_string())?;
                let command =
                    build_runtime_connect_command(&client_path, &alias, &tunnel_id, &local_mcp_url)
                        .map_err(|error| error.to_string())?;
                {
                    let mut app = state.lock().await;
                    app.log(
                        "INFO",
                        format!("Starting official runtime: {}", command.redacted_display),
                    );
                    app.transport_health = TransportHealthSnapshot::configured_unverified(
                        app.tunnel_config.remote_self_check,
                    );
                    app.transport_health.health = TransportHealth::Connecting;
                }
                connect_official_runtime(&client_path, &alias, &tunnel_id, &local_mcp_url)
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    start_openai_runtime_monitor_if_needed(
        state,
        client_path,
        alias,
        profile,
        trusted_client_identity,
    )
    .await;
    Ok(())
}

async fn start_openai_runtime_monitor_if_needed(
    state: SharedState,
    client_path: PathBuf,
    alias: String,
    profile: String,
    trusted_client_identity: String,
) {
    let config = {
        let mut app = state.lock().await;
        if app.openai_tunnel_monitor_task.is_some() {
            return;
        }
        let config = RuntimeMonitorConfig::from_openai_config(&app.openai_tunnel_config);
        app.log(
            "INFO",
            format!(
                "OpenAI Secure MCP runtime monitor armed for alias <redacted> using profile <redacted>; poll {}s",
                config.poll_interval.as_secs()
            ),
        );
        config
    };
    let monitor_state = state.clone();
    let handle = tokio::spawn(async move {
        let mut state_tracker = RuntimeMonitorState::default();
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let should_continue = {
                let app = monitor_state.lock().await;
                matches!(app.tunnel_config.mode, TunnelMode::OpenaiSecureTunnel)
                    && app
                        .openai_tunnel_config
                        .process_mode
                        .uses_official_runtime()
            };
            if !should_continue {
                break;
            }
            let runtime = official_runtime_status(&client_path, &alias)
                .await
                .unwrap_or_else(|error| OfficialRuntimeStatus::stopped(&alias, error.to_string()));
            let (local_endpoint, health_base_url, auto_recover) = {
                let app = monitor_state.lock().await;
                (
                    format!(
                        "http://{}:{}{}",
                        app.mcp_bind_host,
                        app.port,
                        app.mcp_path()
                    ),
                    runtime.health_base_url.clone(),
                    app.openai_tunnel_config.auto_recover,
                )
            };
            let local_mcp_ready = crate::tunnel::run_mcp_endpoint_self_check(
                &local_endpoint,
                None,
                crate::tunnel::MCP_SELF_CHECK_TIMEOUT,
                false,
            )
            .await
            .is_ok();
            let readiness = if let Some(health_base_url) = health_base_url.as_deref() {
                probe_tunnel_client_readiness(health_base_url, OPENAI_TUNNEL_READINESS_TIMEOUT)
                    .await
                    .ok()
            } else {
                None
            };
            let mut runtime_for_health = runtime.clone();
            runtime_for_health.healthy = runtime_for_health.healthy
                && readiness.as_ref().is_some_and(|report| report.health_live);
            runtime_for_health.ready =
                runtime_for_health.ready && readiness.as_ref().is_some_and(|report| report.ready);
            if readiness.is_none() && runtime_for_health.redacted_reason.is_none() {
                runtime_for_health.redacted_reason = Some(
                    "authoritative official runtime health URL was unavailable or rejected".into(),
                );
            }
            let existing_runtime_identity = if runtime_for_health.process_running {
                verified_official_runtime_recovery_identity(
                    &client_path,
                    &trusted_client_identity,
                    &alias,
                    &profile,
                    &runtime_for_health,
                )
                .ok()
            } else {
                None
            };
            let recovery_state = classify_official_runtime_recovery_state(
                local_mcp_ready,
                &runtime_for_health,
                existing_runtime_identity.is_some(),
            );
            if matches!(
                recovery_state,
                OfficialRuntimeRecoveryStateV1::AmbiguousOrMismatchedRuntime
            ) {
                runtime_for_health.redacted_reason =
                    Some("OFFICIAL_RUNTIME_IDENTITY_UNAVAILABLE".into());
            }
            let (health, reason) = monitor_health_from_status(
                &runtime_for_health,
                local_mcp_ready,
                &mut state_tracker,
                &config,
                Instant::now(),
            );
            {
                let mut app = monitor_state.lock().await;
                app.transport_health.health = health;
                app.transport_health.local_mcp = if local_mcp_ready {
                    "READY".into()
                } else {
                    "FAILED".into()
                };
                app.transport_health.redacted_reason = reason.clone();
                app.transport_health.last_checked_at = Some(crate::tunnel::current_startup_time());
                if matches!(health, TransportHealth::ConnectedVerified) {
                    clear_existing_runtime_not_ready_warning(&mut app.transport_health.warnings);
                }
                if let Some(ui_url) = runtime.admin_ui_url.clone() {
                    app.openai_tunnel_config.admin_ui_url = Some(ui_url);
                }
            }
            if recovery_is_eligible_for_official_runtime(
                health,
                runtime_for_health.process_running,
                local_mcp_ready,
                auto_recover,
                credential_environment_present(|name| std::env::var_os(name)),
                tunnel_id_environment_present(|name| std::env::var_os(name)),
            ) && (!runtime_for_health.process_running || existing_runtime_identity.is_some())
                && should_attempt_recovery(&mut state_tracker, &config, Instant::now())
            {
                if let Ok(tunnel_id) = std::env::var("CATDESK_OPENAI_TUNNEL_ID") {
                    let command = build_runtime_connect_command(
                        &client_path,
                        &alias,
                        &tunnel_id,
                        &local_endpoint,
                    );
                    match command {
                        Ok(command) => {
                            monitor_state.lock().await.log(
                                "WARN",
                                format!(
                                    "Attempting bounded official runtime recovery: {}",
                                    command.redacted_display
                                ),
                            );
                            let _ = connect_official_runtime(
                                &client_path,
                                &alias,
                                &tunnel_id,
                                &local_endpoint,
                            )
                            .await;
                        }
                        Err(error) => {
                            monitor_state.lock().await.log(
                                "ERROR",
                                format!("Official runtime recovery blocked: {error}"),
                            );
                        }
                    }
                }
            }
            let sleep_for = match health {
                TransportHealth::ConnectedVerified => config.poll_interval,
                TransportHealth::Degraded | TransportHealth::Disconnected => {
                    config.degraded_interval
                }
                _ => config.poll_interval,
            };
            tokio::time::sleep(sleep_for).await;
        }
        let mut app = monitor_state.lock().await;
        app.log(
            "INFO",
            "OpenAI Secure MCP runtime monitor stopped; official runtime was not stopped by CatDesk"
                .into(),
        );
        app.openai_tunnel_monitor_task = None;
        let _ = profile;
    });
    state.lock().await.openai_tunnel_monitor_task = Some(handle);
}

pub async fn refresh_openai_readiness_from_admin_url(state: &SharedState) {
    let admin_url = {
        let app = state.lock().await;
        app.openai_tunnel_config.admin_ui_url.clone()
    };
    let Some(admin_url) = admin_url else {
        return;
    };
    {
        let mut app = state.lock().await;
        if app.transport_health.health != TransportHealth::ConnectedVerified {
            app.transport_health.health = TransportHealth::Connecting;
        }
    }
    match probe_tunnel_client_readiness(&admin_url, OPENAI_TUNNEL_READINESS_TIMEOUT).await {
        Ok(report) if report.ready => {
            let mut app = state.lock().await;
            app.transport_health.health = TransportHealth::ConnectedVerified;
            app.transport_health.redacted_reason = None;
            app.transport_health.last_checked_at = Some(crate::tunnel::current_startup_time());
            clear_existing_runtime_not_ready_warning(&mut app.transport_health.warnings);
            let ready_warning = "OpenAI tunnel-client /readyz returned ready".to_string();
            if !app.transport_health.warnings.contains(&ready_warning) {
                app.transport_health.warnings.push(ready_warning);
            }
        }
        Ok(report) => {
            let mut app = state.lock().await;
            if app.transport_health.health != TransportHealth::Disconnected {
                app.transport_health.health = TransportHealth::Connecting;
            }
            app.transport_health.redacted_reason = report.redacted_reason;
            app.transport_health.last_checked_at = Some(crate::tunnel::current_startup_time());
        }
        Err(error) => {
            let mut app = state.lock().await;
            app.transport_health.health = TransportHealth::Failed;
            app.transport_health.redacted_reason = Some(error.to_string());
            app.transport_health.last_checked_at = Some(crate::tunnel::current_startup_time());
        }
    }
}

pub async fn refresh_openai_child_exit_status(state: &SharedState) {
    let mut app = state.lock().await;
    let Some(child) = app.openai_tunnel_child.as_mut() else {
        return;
    };
    match child.try_wait() {
        Ok(Some(status)) => {
            app.transport_health.health = if status.success() {
                TransportHealth::Disconnected
            } else {
                TransportHealth::Failed
            };
            app.transport_health.redacted_reason = Some(format!(
                "managed tunnel-client exited with status {}",
                status.code().unwrap_or(-1)
            ));
            app.openai_tunnel_child = None;
        }
        Ok(None) => {}
        Err(_) => {
            app.transport_health.health = TransportHealth::Failed;
            app.transport_health.redacted_reason =
                Some("managed tunnel-client status could not be inspected".into());
        }
    }
}

async fn set_transport_health(state: &SharedState, health: TransportHealth, reason: &str) {
    let mut app = state.lock().await;
    app.transport_health = TransportHealthSnapshot::configured_unverified(false);
    app.transport_health.health = health;
    app.transport_health.local_mcp = "NOT_CHECKED".into();
    app.transport_health.redacted_reason = Some(reason.into());
    app.log("ERROR", format!("Transport health: {}", health.as_str()));
}

async fn configure_external_tunnel(state: SharedState) -> Result<(), String> {
    let mut app = state.lock().await;
    let base_url = app
        .tunnel_config
        .public_base_url
        .clone()
        .ok_or_else(|| "external_tunnel requires tunnel.public_base_url".to_string())?;
    let public_mcp_url = external_public_mcp_url(&base_url, &app.mcp_path())?;
    let fingerprint = connection_fingerprint(&public_mcp_url);
    app.ngrok_running = false;
    app.ngrok_url = Some(base_url);
    app.transport_identity.last_connection_fingerprint = Some(fingerprint.clone());
    app.transport_health =
        TransportHealthSnapshot::configured_unverified(app.tunnel_config.remote_self_check);
    app.log(
        "INFO",
        "External tunnel mode active; CatDesk did not launch ngrok".into(),
    );
    app.log(
        "INFO",
        format!("MCP Server URL: {}", redact_full_mcp_url(&public_mcp_url)),
    );
    app.log("INFO", format!("Connection fingerprint: {fingerprint}"));
    app.log("WARN", public_no_auth_warning().into());
    app.persist_state_with_log();
    Ok(())
}

/// Start an ngrok HTTP tunnel using the embedded Rust SDK.
pub async fn start(state: SharedState) -> Result<(), String> {
    let (port, mcp_path) = {
        let app = state.lock().await;
        if app.ngrok_running {
            return Err("ngrok is already running".into());
        }
        (app.port, app.mcp_path())
    };
    let authtoken = load_ngrok_authtoken()
        .map_err(|e| format!("Failed to read ~/.catdesk/config.toml: {e}"))?
        .ok_or_else(|| "ngrok authtoken is not configured".to_string())?;
    let forwards_to: Url = format!("http://127.0.0.1:{port}")
        .parse()
        .map_err(|e| format!("Invalid forward URL: {e}"))?;

    let mut forwarder = ngrok::Session::builder()
        .authtoken(authtoken)
        .connect()
        .await
        .map_err(|e| format!("Failed to connect ngrok session: {e}"))?
        .http_endpoint()
        .listen_and_forward(forwards_to)
        .await
        .map_err(|e| format!("Failed to open ngrok tunnel: {e}"))?;
    let url = forwarder.url().to_string();

    let state_clone = state.clone();
    let watcher = tokio::spawn(async move {
        let result = forwarder.join().await;
        let mut app = state_clone.lock().await;
        match result {
            Ok(Ok(())) => app.log("WARN", "ngrok tunnel exited".into()),
            Ok(Err(e)) => app.log("ERROR", format!("ngrok tunnel failed: {e}")),
            Err(e) if e.is_cancelled() => return,
            Err(e) => app.log("ERROR", format!("ngrok tunnel join failed: {e}")),
        }
        app.ngrok_running = false;
        app.ngrok_url = None;
        app.remote_connected = false;
        app.last_remote_activity_ms = None;
    });

    {
        let mut app = state.lock().await;
        let public_mcp_url = format!("{url}{mcp_path}");
        let fingerprint = connection_fingerprint(&public_mcp_url);
        app.ngrok_task = Some(watcher);
        app.ngrok_running = true;
        app.ngrok_url = Some(url.clone());
        app.transport_identity.last_connection_fingerprint = Some(fingerprint.clone());
        app.transport_health.health = TransportHealth::ConnectedVerified;
        app.transport_health.local_mcp = "NOT_CHECKED".into();
        app.transport_health.remote_check_enabled = false;
        app.transport_health.last_checked_at = Some(crate::tunnel::current_startup_time());
        app.log("INFO", "ngrok SDK tunnel started".into());
        app.log("INFO", format!("ngrok URL: {}", redact_full_mcp_url(&url)));
        app.log(
            "INFO",
            format!("MCP Server URL: {}", redact_full_mcp_url(&public_mcp_url)),
        );
        app.log("INFO", format!("Connection fingerprint: {fingerprint}"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor_config() -> RuntimeMonitorConfig {
        RuntimeMonitorConfig {
            poll_interval: Duration::from_secs(5),
            degraded_interval: Duration::from_secs(2),
            command_timeout: Duration::from_secs(5),
            failure_threshold: 2,
            recovery_cooldown: Duration::from_secs(30),
            max_recovery_attempts: 2,
            recovery_window: Duration::from_secs(600),
        }
    }

    #[test]
    fn existing_degraded_runtime_recovery_requires_every_safety_gate_and_budget() {
        let config = monitor_config();
        let now = Instant::now();
        let mut tracker = RuntimeMonitorState::default();

        assert!(recovery_is_eligible_for_official_runtime(
            TransportHealth::Degraded,
            true,
            true,
            true,
            true,
            true,
        ));
        assert!(should_attempt_recovery(&mut tracker, &config, now));
        assert!(!should_attempt_recovery(
            &mut tracker,
            &config,
            now + Duration::from_secs(1),
        ));

        for (auto_recover, credential_present, tunnel_id_present, local_mcp_ready) in [
            (false, true, true, true),
            (true, false, true, true),
            (true, true, false, true),
            (true, true, true, false),
        ] {
            assert!(!recovery_is_eligible_for_official_runtime(
                TransportHealth::Degraded,
                true,
                local_mcp_ready,
                auto_recover,
                credential_present,
                tunnel_id_present,
            ));
        }
        assert!(!recovery_is_eligible_for_official_runtime(
            TransportHealth::Degraded,
            false,
            true,
            true,
            true,
            true,
        ));
        assert!(!recovery_is_eligible_for_official_runtime(
            TransportHealth::Connecting,
            true,
            true,
            true,
            true,
            true,
        ));
        assert!(recovery_is_eligible_for_official_runtime(
            TransportHealth::Disconnected,
            false,
            true,
            true,
            true,
            true,
        ));
    }

    #[test]
    fn existing_runtime_recovery_command_never_stops_removes_or_creates_a_runtime() {
        let command = build_runtime_connect_command(
            &PathBuf::from("tunnel-client.exe"),
            "catdesk-local",
            "tunnel-id",
            "http://127.0.0.1:3200/AbCdEf123456789012345678/mcp",
        )
        .expect("connect command");
        assert_eq!(command.args[0..2], ["runtimes", "connect"]);
        assert!(
            !command
                .args
                .iter()
                .any(|argument| matches!(argument.as_str(), "stop" | "remove" | "rm" | "create"))
        );
    }

    #[test]
    fn connected_verified_clears_only_the_stale_existing_runtime_warning() {
        let mut warnings = vec![
            EXISTING_RUNTIME_NOT_READY_WARNING.to_string(),
            "unrelated operator warning".to_string(),
        ];
        clear_existing_runtime_not_ready_warning(&mut warnings);
        assert_eq!(warnings, vec!["unrelated operator warning"]);
    }
}
