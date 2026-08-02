use crate::state::{SharedState, load_ngrok_authtoken};
use crate::tunnel::{
    TransportHealth, TransportHealthSnapshot, TunnelMode, connection_fingerprint,
    external_public_mcp_url, public_no_auth_warning, redact_full_mcp_url,
};
use ngrok::prelude::*;
use reqwest::Url;

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
        TunnelMode::OpenaiSecureTunnel => {
            Err("openai_secure_tunnel is unavailable until T-0025D feasibility passes".into())
        }
    }
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
        app.ngrok_task = Some(watcher);
        app.ngrok_running = true;
        app.ngrok_url = Some(url.clone());
        app.transport_health.health = TransportHealth::ConnectedVerified;
        app.transport_health.local_mcp = "NOT_CHECKED".into();
        app.transport_health.remote_check_enabled = false;
        app.transport_health.last_checked_at = Some(crate::tunnel::current_startup_time());
        app.log("INFO", "ngrok SDK tunnel started".into());
        app.log("INFO", format!("ngrok URL: {url}"));
        app.log("INFO", format!("MCP Server URL: {url}{mcp_path}"));
    }

    Ok(())
}
