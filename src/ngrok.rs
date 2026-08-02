use crate::openai_tunnel::{
    OPENAI_TUNNEL_READINESS_TIMEOUT, OpenaiTunnelProcessMode, TunnelClientDiscoveryOptions,
    credential_environment_present, default_user_tools_dir, discover_tunnel_client,
    probe_tunnel_client_readiness, run_tunnel_client_doctor, spawn_tunnel_client_run,
};
use crate::state::{SharedState, load_ngrok_authtoken, user_home_dir};
use crate::tunnel::{
    TransportHealth, TransportHealthSnapshot, TunnelMode, connection_fingerprint,
    external_public_mcp_url, public_no_auth_warning, redact_full_mcp_url,
};
use ngrok::prelude::*;
use reqwest::Url;
use std::path::PathBuf;

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
    if matches!(config.process_mode, OpenaiTunnelProcessMode::External) {
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
        drop(app);
        refresh_openai_readiness_from_admin_url(&state).await;
        return Ok(());
    }

    let home = user_home_dir().map_err(|error| error.to_string())?;
    let discovery = TunnelClientDiscoveryOptions {
        explicit_path: config.client_path.as_ref().map(PathBuf::from),
        path_var: std::env::var_os("PATH"),
        user_tools_dir: default_user_tools_dir(&home),
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
    if !credential_environment_present(|name| std::env::var_os(name)) {
        set_transport_health(
            &state,
            TransportHealth::BlockedMissingCredential,
            "CONTROL_PLANE_API_KEY is not present in the CatDesk process environment",
        )
        .await;
        return Err(
            "openai_secure_tunnel requires CONTROL_PLANE_API_KEY in the process environment".into(),
        );
    }
    if let Err(error) = run_tunnel_client_doctor(&metadata.path, &config.profile_name).await {
        set_transport_health(
            &state,
            TransportHealth::Failed,
            "tunnel-client doctor failed; inspect the operator-owned profile",
        )
        .await;
        return Err(error.to_string());
    }

    match config.process_mode {
        OpenaiTunnelProcessMode::External => {
            unreachable!("external mode returned before discovery")
        }
        OpenaiTunnelProcessMode::Managed => {
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
    }
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
            app.transport_health
                .warnings
                .push("OpenAI tunnel-client /readyz returned ready".into());
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
