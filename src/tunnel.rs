use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const DEFAULT_MCP_BIND_HOST: &str = "127.0.0.1";
pub const DEFAULT_MCP_PORT: u16 = 3200;
pub const MCP_SELF_CHECK_MAX_RESPONSE_BYTES: usize = 512 * 1024;
pub const MCP_SELF_CHECK_TIMEOUT: Duration = Duration::from_secs(5);
const MIN_ROUTE_LEN: usize = 24;
const MAX_ROUTE_LEN: usize = 96;
const REQUIRED_BASELINE_TOOL: &str = "catdesk_instruction";
const REQUIRED_SUPERVISOR_TOOL: &str = "delegated_run_list";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelMode {
    #[default]
    ManagedEphemeralNgrok,
    ManagedStableNgrok,
    ExternalTunnel,
    OpenaiSecureTunnel,
}

impl TunnelMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ManagedEphemeralNgrok => "managed_ephemeral_ngrok",
            Self::ManagedStableNgrok => "managed_stable_ngrok",
            Self::ExternalTunnel => "external_tunnel",
            Self::OpenaiSecureTunnel => "openai_secure_tunnel",
        }
    }

    pub fn unimplemented_message(self) -> Option<String> {
        match self {
            Self::ManagedEphemeralNgrok
            | Self::ManagedStableNgrok
            | Self::ExternalTunnel
            | Self::OpenaiSecureTunnel => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct McpTransportConfig {
    #[serde(default = "default_bind_host")]
    pub bind_host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub route_id: Option<String>,
    #[serde(default)]
    pub display_full_url: bool,
    #[serde(default)]
    pub require_auth_token: bool,
    #[serde(default)]
    pub route_rotation: RouteRotationState,
}

impl Default for McpTransportConfig {
    fn default() -> Self {
        Self {
            bind_host: default_bind_host(),
            port: default_port(),
            route_id: None,
            display_full_url: false,
            require_auth_token: false,
            route_rotation: RouteRotationState::default(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RouteRotationState {
    #[serde(default)]
    pub pending_route_id: Option<String>,
    #[serde(default)]
    pub restart_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TunnelConfig {
    #[serde(default)]
    pub mode: TunnelMode,
    #[serde(default)]
    pub public_base_url: Option<String>,
    #[serde(default)]
    pub ngrok_domain: Option<String>,
    #[serde(default)]
    pub ngrok_config_path: Option<String>,
    #[serde(default = "default_manage_process")]
    pub manage_process: bool,
    #[serde(default)]
    pub remote_self_check: bool,
}

impl Default for TunnelConfig {
    fn default() -> Self {
        Self {
            mode: TunnelMode::ManagedEphemeralNgrok,
            public_base_url: None,
            ngrok_domain: None,
            ngrok_config_path: None,
            manage_process: true,
            remote_self_check: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TransportSecurityConfig {
    #[serde(default = "default_warn_on_public_no_auth")]
    pub warn_on_public_no_auth: bool,
    #[serde(default = "default_redact_connection_url")]
    pub redact_connection_url: bool,
    #[serde(default = "default_allow_one_time_reveal")]
    pub allow_one_time_reveal: bool,
}

impl Default for TransportSecurityConfig {
    fn default() -> Self {
        Self {
            warn_on_public_no_auth: true,
            redact_connection_url: true,
            allow_one_time_reveal: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TransportIdentityConfig {
    #[serde(default)]
    pub installation_id: Option<String>,
    #[serde(default)]
    pub last_connection_fingerprint: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransportHealth {
    #[default]
    Disabled,
    ConfiguredUnverified,
    LocalReady,
    Connecting,
    ConnectedVerified,
    Degraded,
    Disconnected,
    BlockedMissingClient,
    BlockedMissingProfile,
    BlockedMissingCredential,
    BlockedMissingTunnelId,
    BlockedInvalidConfiguration,
    BlockedLocalMcpUnavailable,
    BlockedRemoteEndpointUnavailable,
    BlockedPermissionUnknown,
    BlockedUnsupported,
    Failed,
}

impl TransportHealth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "DISABLED",
            Self::ConfiguredUnverified => "CONFIGURED_UNVERIFIED",
            Self::LocalReady => "LOCAL_READY",
            Self::Connecting => "CONNECTING",
            Self::ConnectedVerified => "CONNECTED_VERIFIED",
            Self::Degraded => "DEGRADED",
            Self::Disconnected => "DISCONNECTED",
            Self::BlockedMissingClient => "BLOCKED_MISSING_CLIENT",
            Self::BlockedMissingProfile => "BLOCKED_MISSING_PROFILE",
            Self::BlockedMissingCredential => "BLOCKED_MISSING_CREDENTIAL",
            Self::BlockedMissingTunnelId => "BLOCKED_MISSING_TUNNEL_ID",
            Self::BlockedInvalidConfiguration => "BLOCKED_INVALID_CONFIGURATION",
            Self::BlockedLocalMcpUnavailable => "BLOCKED_LOCAL_MCP_UNAVAILABLE",
            Self::BlockedRemoteEndpointUnavailable => "BLOCKED_REMOTE_ENDPOINT_UNAVAILABLE",
            Self::BlockedPermissionUnknown => "BLOCKED_PERMISSION_UNKNOWN",
            Self::BlockedUnsupported => "BLOCKED_UNSUPPORTED",
            Self::Failed => "FAILED",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportHealthSnapshot {
    pub health: TransportHealth,
    pub local_mcp: String,
    pub remote_check_enabled: bool,
    pub last_checked_at: Option<String>,
    pub warnings: Vec<String>,
    pub redacted_reason: Option<String>,
}

impl TransportHealthSnapshot {
    pub fn disabled() -> Self {
        Self {
            health: TransportHealth::Disabled,
            local_mcp: "NOT_CHECKED".into(),
            remote_check_enabled: false,
            last_checked_at: None,
            warnings: Vec::new(),
            redacted_reason: None,
        }
    }

    pub fn configured_unverified(remote_check_enabled: bool) -> Self {
        Self {
            health: TransportHealth::ConfiguredUnverified,
            local_mcp: "NOT_CHECKED".into(),
            remote_check_enabled,
            last_checked_at: None,
            warnings: Vec::new(),
            redacted_reason: None,
        }
    }
}

impl Default for TransportHealthSnapshot {
    fn default() -> Self {
        Self::disabled()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpSelfCheckReport {
    pub endpoint_fingerprint: String,
    pub initialize_ok: bool,
    pub tools_list_ok: bool,
    pub required_tools_present: bool,
    pub tool_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct TransportIdentitySnapshot {
    pub installation_id: String,
    pub server_instance_id: String,
    pub git_commit: String,
    pub dirty_build: String,
    pub binary_fingerprint: String,
    pub startup_time: String,
    pub workspace_hash: String,
    pub transport_mode: String,
    pub connection_fingerprint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigSaveOutcome {
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtomicWritePlan {
    pub tmp_path_suffix: String,
    #[cfg(test)]
    pub fail_after_tmp_created: Option<&'static str>,
    #[cfg(test)]
    pub fail_replace_with: Option<&'static str>,
    #[cfg(test)]
    pub fail_cleanup_with: Option<&'static str>,
}

impl Default for AtomicWritePlan {
    fn default() -> Self {
        Self {
            tmp_path_suffix: format!(".tmp-{}", Uuid::new_v4()),
            #[cfg(test)]
            fail_after_tmp_created: None,
            #[cfg(test)]
            fail_replace_with: None,
            #[cfg(test)]
            fail_cleanup_with: None,
        }
    }
}

fn default_bind_host() -> String {
    DEFAULT_MCP_BIND_HOST.to_string()
}

fn default_port() -> u16 {
    DEFAULT_MCP_PORT
}

fn default_manage_process() -> bool {
    true
}

fn default_warn_on_public_no_auth() -> bool {
    true
}

fn default_redact_connection_url() -> bool {
    true
}

fn default_allow_one_time_reveal() -> bool {
    true
}

pub fn generate_persistent_route_id() -> String {
    let mut bytes = [0_u8; 24];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn validate_route_id(route: &str) -> Result<(), String> {
    if route.len() < MIN_ROUTE_LEN || route.len() > MAX_ROUTE_LEN {
        return Err(format!(
            "MCP route id must be between {MIN_ROUTE_LEN} and {MAX_ROUTE_LEN} characters"
        ));
    }
    if matches!(route, "." | "..") {
        return Err("MCP route id must not be a traversal segment".into());
    }
    if !route
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("MCP route id must use only A-Z, a-z, 0-9, `_`, and `-`".into());
    }
    Ok(())
}

pub fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn normalize_public_base_url(value: &str) -> Result<String, String> {
    reject_whitespace_or_control(value, "public_base_url")?;
    let trimmed = value.trim_end_matches('/');
    let parsed = reqwest::Url::parse(trimmed)
        .map_err(|_| "public_base_url must be a valid https origin".to_string())?;
    if parsed.scheme() != "https" {
        return Err("public_base_url must be an https origin".into());
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(
            "public_base_url must be an https origin without path, query, or credentials".into(),
        );
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "public_base_url must include a hostname".to_string())?;
    let normalized_host = normalize_hostname(host, "public_base_url hostname")?;
    let port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    Ok(format!("https://{normalized_host}{port}"))
}

pub fn normalize_ngrok_domain(value: &str) -> Result<String, String> {
    reject_whitespace_or_control(value, "ngrok_domain")?;
    if value.contains("://")
        || value.contains('/')
        || value.contains('?')
        || value.contains('#')
        || value.contains('@')
        || value.contains(':')
    {
        return Err("ngrok_domain must be a hostname without scheme, port, path, query, fragment, or credentials".into());
    }
    normalize_hostname(value, "ngrok_domain")
}

pub fn external_public_mcp_url(base_url: &str, mcp_path: &str) -> Result<String, String> {
    let base = normalize_public_base_url(base_url)?;
    if !mcp_path.starts_with('/') || !mcp_path.ends_with("/mcp") {
        return Err("MCP path must be an absolute /<route>/mcp path".into());
    }
    Ok(format!("{base}{mcp_path}"))
}

pub fn public_no_auth_warning() -> &'static str {
    "PUBLIC DEVELOPMENT ENDPOINT: anyone with the full MCP URL may be able to invoke enabled tools. Do not share the URL. Restrict the workspace and enabled actions."
}

fn reject_whitespace_or_control(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(format!(
            "{label} must not contain whitespace or control characters"
        ));
    }
    Ok(())
}

fn normalize_hostname(value: &str, label: &str) -> Result<String, String> {
    let host = value.to_ascii_lowercase();
    if host.is_empty() || host.len() > 253 || host.starts_with('.') || host.ends_with('.') {
        return Err(format!("{label} must be a valid hostname"));
    }
    if host.contains("..") || !host.contains('.') {
        return Err(format!("{label} must contain valid DNS labels"));
    }
    for label_part in host.split('.') {
        if label_part.is_empty()
            || label_part.len() > 63
            || label_part.starts_with('-')
            || label_part.ends_with('-')
            || !label_part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(format!("{label} contains an invalid DNS label"));
        }
    }
    Ok(host)
}

pub fn requires_persistent_route(mode: TunnelMode) -> bool {
    matches!(
        mode,
        TunnelMode::ManagedStableNgrok
            | TunnelMode::ExternalTunnel
            | TunnelMode::OpenaiSecureTunnel
    )
}

pub fn requires_loopback(mode: TunnelMode) -> bool {
    requires_persistent_route(mode)
}

pub fn is_loopback_bind_host(host: &str) -> bool {
    let normalized = host.trim();
    if normalized.eq_ignore_ascii_case("localhost") {
        return true;
    }
    normalized
        .parse::<IpAddr>()
        .is_ok_and(|addr| addr.is_loopback())
}

#[allow(dead_code)]
pub fn manages_tunnel_process(mode: TunnelMode) -> bool {
    matches!(
        mode,
        TunnelMode::ManagedEphemeralNgrok | TunnelMode::ManagedStableNgrok
    )
}

pub fn validate_transport(mcp: &McpTransportConfig, tunnel: &TunnelConfig) -> Result<(), String> {
    if mcp.port == 0 {
        return Err("mcp.port must be between 1 and 65535".into());
    }
    if mcp.bind_host.trim().is_empty()
        || mcp
            .bind_host
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(
            "mcp.bind_host must not be empty or contain whitespace/control characters".into(),
        );
    }
    if requires_loopback(tunnel.mode) && !is_loopback_bind_host(&mcp.bind_host) {
        return Err("private transport modes must bind to a loopback address".into());
    }
    if tunnel.manage_process != manages_tunnel_process(tunnel.mode) {
        return Err(format!(
            "tunnel.manage_process={} is not supported for tunnel mode `{}`",
            tunnel.manage_process,
            tunnel.mode.as_str()
        ));
    }
    if mcp.display_full_url {
        return Err("mcp.display_full_url is reserved and must remain false".into());
    }
    if mcp.require_auth_token {
        return Err(
            "mcp.require_auth_token is reserved until compatible ChatGPT authentication is proven"
                .into(),
        );
    }
    if tunnel.ngrok_config_path.is_some() {
        return Err("tunnel.ngrok_config_path is reserved for T-0025C".into());
    }
    if let Some(route) = mcp.route_id.as_deref() {
        validate_route_id(route)?;
    }
    if let Some(route) = mcp.route_rotation.pending_route_id.as_deref() {
        validate_route_id(route)?;
    }
    if mcp.route_rotation.pending_route_id.is_some() != mcp.route_rotation.restart_required {
        return Err(
            "mcp.route_rotation.pending_route_id must be present exactly when restart_required is true"
                .into(),
        );
    }
    if matches!(tunnel.mode, TunnelMode::ManagedStableNgrok) {
        let domain = tunnel
            .ngrok_domain
            .as_deref()
            .ok_or_else(|| "managed_stable_ngrok requires tunnel.ngrok_domain".to_string())?;
        normalize_ngrok_domain(domain)?;
    } else if tunnel.ngrok_domain.is_some() {
        return Err("tunnel.ngrok_domain is only supported with managed_stable_ngrok".into());
    }
    if matches!(tunnel.mode, TunnelMode::ExternalTunnel) {
        let base_url = tunnel
            .public_base_url
            .as_deref()
            .ok_or_else(|| "external_tunnel requires tunnel.public_base_url".to_string())?;
        normalize_public_base_url(base_url)?;
    } else if tunnel.public_base_url.is_some() {
        return Err("tunnel.public_base_url is only supported with external_tunnel".into());
    }
    Ok(())
}

pub fn ensure_persistent_route_if_required(
    mcp: &mut McpTransportConfig,
    tunnel: &TunnelConfig,
) -> bool {
    if requires_persistent_route(tunnel.mode) && mcp.route_id.as_deref().is_none_or(str::is_empty) {
        mcp.route_id = Some(generate_persistent_route_id());
        return true;
    }
    false
}

#[allow(dead_code)]
pub fn rotate_route_restart_required(mcp: &mut McpTransportConfig) {
    mcp.route_rotation.pending_route_id = Some(generate_persistent_route_id());
    mcp.route_rotation.restart_required = true;
}

pub fn promote_pending_route_after_restart(mcp: &mut McpTransportConfig) -> bool {
    if !mcp.route_rotation.restart_required {
        return false;
    }
    let Some(pending) = mcp.route_rotation.pending_route_id.take() else {
        return false;
    };
    mcp.route_id = Some(pending);
    mcp.route_rotation.restart_required = false;
    true
}

pub fn ensure_installation_id(identity: &mut TransportIdentityConfig) -> bool {
    if identity
        .installation_id
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        identity.installation_id = Some(Uuid::new_v4().to_string());
        return true;
    }
    false
}

#[allow(dead_code)]
pub fn redact_full_mcp_url(url: &str) -> String {
    let Some((base, _route)) = url.rsplit_once('/') else {
        return "<redacted>".into();
    };
    let Some((origin, _slug)) = base.rsplit_once('/') else {
        return "<redacted>".into();
    };
    format!("{origin}/<redacted>/mcp")
}

#[allow(dead_code)]
pub fn connection_fingerprint(url: &str) -> String {
    let normalized = normalize_endpoint_for_fingerprint(url).unwrap_or_else(|| {
        url.trim()
            .trim_end_matches('/')
            .replace('\\', "/")
            .to_string()
    });
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in normalized.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{hash:016x}")
}

fn normalize_endpoint_for_fingerprint(value: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(value.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    let port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let path = parsed.path().trim_end_matches('/');
    let path = if path.is_empty() { "/" } else { path };
    Some(format!(
        "{}://{host}{port}{path}",
        parsed.scheme().to_ascii_lowercase()
    ))
}

#[allow(dead_code)]
pub fn workspace_fingerprint(path: &str) -> String {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    connection_fingerprint(&normalized)
}

pub fn current_startup_time() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| format!("unix:{}", duration.as_secs()))
        .unwrap_or_else(|_| "unix:0".into())
}

pub fn validate_remote_mcp_endpoint(url: &str) -> Result<(), String> {
    reject_whitespace_or_control(url, "remote MCP endpoint")?;
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| "remote MCP endpoint must be a valid URL".to_string())?;
    if parsed.scheme() != "https" {
        return Err("remote MCP endpoint must use https".into());
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(
            "remote MCP endpoint must not contain credentials, query strings, or fragments".into(),
        );
    }
    let Some(host) = parsed.host_str() else {
        return Err("remote MCP endpoint must include a hostname".into());
    };
    normalize_hostname(host, "remote MCP endpoint hostname")?;
    if parsed.path() == "/" || !parsed.path().ends_with("/mcp") {
        return Err("remote MCP endpoint path must end with /mcp".into());
    }
    Ok(())
}

pub fn validate_required_mcp_tools(value: &Value) -> Result<usize, String> {
    let tools = value
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .ok_or_else(|| "tools/list response did not contain result.tools".to_string())?;
    let mut has_baseline = false;
    let mut has_supervisor = false;
    for tool in tools {
        match tool.get("name").and_then(Value::as_str) {
            Some(REQUIRED_BASELINE_TOOL) => has_baseline = true,
            Some(REQUIRED_SUPERVISOR_TOOL) => has_supervisor = true,
            _ => {}
        }
    }
    if !has_baseline {
        return Err("tools/list did not include required catdesk_instruction tool".into());
    }
    if !has_supervisor {
        return Err("tools/list did not include required delegated_run_list tool".into());
    }
    Ok(tools.len())
}

pub async fn run_mcp_endpoint_self_check(
    endpoint: &str,
    auth_token: Option<&str>,
    timeout: Duration,
    require_https: bool,
) -> Result<McpSelfCheckReport, String> {
    if require_https {
        validate_remote_mcp_endpoint(endpoint)?;
    }
    let parsed = reqwest::Url::parse(endpoint)
        .map_err(|_| "MCP self-check endpoint must be a valid URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("MCP self-check endpoint must use http or https".into());
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(
            "MCP self-check endpoint must not contain credentials, query strings, or fragments"
                .into(),
        );
    }

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|_| "failed to build bounded MCP self-check client".to_string())?;

    let initialize = json!({
        "jsonrpc": "2.0",
        "id": "catdesk-self-check-initialize",
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "catdesk-self-check", "version": "1" }
        }
    });
    let initialize_value = send_bounded_mcp_request(&client, endpoint, auth_token, &initialize)
        .await
        .map_err(|error| format!("initialize failed: {error}"))?;
    if initialize_value.get("error").is_some()
        || initialize_value
            .get("result")
            .and_then(|result| result.get("serverInfo"))
            .is_none()
    {
        return Err("initialize response did not include a valid server identity".into());
    }

    let tools_list = json!({
        "jsonrpc": "2.0",
        "id": "catdesk-self-check-tools",
        "method": "tools/list",
        "params": {}
    });
    let tools_value = send_bounded_mcp_request(&client, endpoint, auth_token, &tools_list)
        .await
        .map_err(|error| format!("tools/list failed: {error}"))?;
    let tool_count = validate_required_mcp_tools(&tools_value)?;
    Ok(McpSelfCheckReport {
        endpoint_fingerprint: connection_fingerprint(endpoint),
        initialize_ok: true,
        tools_list_ok: true,
        required_tools_present: true,
        tool_count,
    })
}

async fn send_bounded_mcp_request(
    client: &reqwest::Client,
    endpoint: &str,
    auth_token: Option<&str>,
    body: &Value,
) -> Result<Value, String> {
    let mut request = client
        .post(endpoint)
        .header("catdesk-self-check", "1")
        .json(body);
    if let Some(token) = auth_token {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|error| redacted_network_error(&error.to_string()))?;
    let status = response.status();
    if status.is_redirection() {
        return Err("endpoint returned a redirect; refusing cross-origin MCP self-check".into());
    }
    if !status.is_success() {
        return Err(format!("endpoint returned HTTP {}", status.as_u16()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| redacted_network_error(&error.to_string()))?;
    if bytes.len() > MCP_SELF_CHECK_MAX_RESPONSE_BYTES {
        return Err("endpoint response exceeded MCP self-check size limit".into());
    }
    serde_json::from_slice::<Value>(&bytes)
        .map_err(|_| "endpoint returned malformed JSON-RPC".to_string())
}

fn redacted_network_error(error: &str) -> String {
    let first = error.lines().next().unwrap_or("network error");
    first
        .replace("http://", "<scheme>://")
        .replace("https://", "<scheme>://")
        .chars()
        .take(240)
        .collect()
}

#[allow(dead_code)]
pub fn git_commit() -> String {
    option_env!("GIT_COMMIT")
        .or(option_env!("VERGEN_GIT_SHA"))
        .unwrap_or("unknown")
        .to_string()
}

#[allow(dead_code)]
pub fn dirty_build_state() -> String {
    option_env!("GIT_DIRTY")
        .map(|value| {
            if matches!(value, "1" | "true" | "TRUE" | "dirty") {
                "dirty"
            } else if matches!(value, "0" | "false" | "FALSE" | "clean") {
                "clean"
            } else {
                "unknown"
            }
        })
        .unwrap_or("unknown")
        .to_string()
}

#[allow(dead_code)]
pub fn binary_fingerprint() -> String {
    let Ok(exe) = std::env::current_exe() else {
        return "unknown".into();
    };
    let Ok(metadata) = std::fs::metadata(&exe) else {
        return connection_fingerprint(&exe.to_string_lossy());
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    connection_fingerprint(&format!(
        "{}:{}:{}",
        exe.to_string_lossy(),
        metadata.len(),
        modified
    ))
}

#[allow(dead_code)]
pub fn build_identity_snapshot(
    installation_id: String,
    server_instance_id: String,
    startup_time: String,
    workspace_root: &str,
    tunnel_mode: TunnelMode,
    connection_url: Option<&str>,
) -> TransportIdentitySnapshot {
    TransportIdentitySnapshot {
        installation_id,
        server_instance_id,
        git_commit: git_commit(),
        dirty_build: dirty_build_state(),
        binary_fingerprint: binary_fingerprint(),
        startup_time,
        workspace_hash: workspace_fingerprint(workspace_root),
        transport_mode: tunnel_mode.as_str().to_string(),
        connection_fingerprint: connection_url.map(connection_fingerprint),
    }
}

pub fn tmp_path_for_atomic_write(
    path: &Path,
    plan: &AtomicWritePlan,
) -> std::io::Result<std::path::PathBuf> {
    let file_name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("config path has no file name"))?;
    Ok(path.with_file_name(format!(
        "{}{}",
        file_name.to_string_lossy(),
        plan.tmp_path_suffix
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};
    use serde_json::json;
    use tokio::task::JoinHandle;

    #[test]
    fn every_documented_tunnel_mode_parses() {
        for mode in [
            "managed_ephemeral_ngrok",
            "managed_stable_ngrok",
            "external_tunnel",
            "openai_secure_tunnel",
        ] {
            let parsed: TunnelMode = toml::from_str(&format!("mode = \"{mode}\""))
                .map(|table: TunnelConfig| table.mode)
                .expect("parse mode");
            assert_eq!(parsed.as_str(), mode);
        }
    }

    #[test]
    fn unknown_tunnel_mode_is_rejected() {
        let error = toml::from_str::<TunnelConfig>("mode = \"surprise\"").unwrap_err();
        assert!(error.to_string().contains("unknown variant"));
    }

    #[test]
    fn route_traversal_and_unsafe_characters_are_rejected() {
        for route in [
            "../mcp",
            "hello/world/with/slash-and-long-enough",
            "hello%20world_with_enough_chars",
            "hello world with enough characters",
            "dot.segment.with.enough.characters",
        ] {
            assert!(validate_route_id(route).is_err(), "{route}");
        }
    }

    #[test]
    fn generated_route_uses_conservative_url_safe_alphabet() {
        let route = generate_persistent_route_id();
        validate_route_id(&route).expect("generated route is valid");
        assert!(!route.contains('/'));
        assert!(!route.contains('+'));
        assert!(!route.contains('='));
    }

    #[test]
    fn remote_mcp_endpoint_requires_https_without_credentials_or_query() {
        let host = "example.ngrok-free.app";
        let path = "/AbCd/mcp";
        assert!(validate_remote_mcp_endpoint(&format!("https://{host}{path}")).is_ok());
        for endpoint in [
            format!("http://{host}{path}"),
            format!("https://user@{host}{path}"),
            format!("https://{host}{path}?q=1"),
            format!("https://{host}{path}#fragment"),
            format!("https://{host}/"),
        ] {
            assert!(
                validate_remote_mcp_endpoint(&endpoint).is_err(),
                "{endpoint}"
            );
        }
    }

    #[test]
    fn required_mcp_tools_are_validated_from_tools_list_result() {
        let response = json!({
            "jsonrpc": "2.0",
            "id": "tools",
            "result": {
                "tools": [
                    { "name": "catdesk_instruction" },
                    { "name": "delegated_run_list" }
                ]
            }
        });
        assert_eq!(validate_required_mcp_tools(&response).expect("tools"), 2);

        let missing = json!({
            "jsonrpc": "2.0",
            "id": "tools",
            "result": { "tools": [{ "name": "catdesk_instruction" }] }
        });
        assert!(validate_required_mcp_tools(&missing).is_err());
    }

    async fn ok_mcp(Json(body): Json<Value>) -> Json<Value> {
        match body.get("method").and_then(Value::as_str) {
            Some("initialize") => Json(json!({
                "jsonrpc": "2.0",
                "id": body.get("id").cloned().unwrap_or(Value::Null),
                "result": {
                    "protocolVersion": "2025-03-26",
                    "serverInfo": { "name": "catdesk", "version": "test" }
                }
            })),
            Some("tools/list") => Json(json!({
                "jsonrpc": "2.0",
                "id": body.get("id").cloned().unwrap_or(Value::Null),
                "result": {
                    "tools": [
                        { "name": "catdesk_instruction" },
                        { "name": "delegated_run_list" }
                    ]
                }
            })),
            _ => Json(json!({
                "jsonrpc": "2.0",
                "id": body.get("id").cloned().unwrap_or(Value::Null),
                "error": { "code": -32601, "message": "method not found" }
            })),
        }
    }

    async fn missing_tools_mcp(Json(body): Json<Value>) -> Json<Value> {
        match body.get("method").and_then(Value::as_str) {
            Some("initialize") => Json(json!({
                "jsonrpc": "2.0",
                "id": body.get("id").cloned().unwrap_or(Value::Null),
                "result": { "serverInfo": { "name": "catdesk", "version": "test" } }
            })),
            Some("tools/list") => Json(json!({
                "jsonrpc": "2.0",
                "id": body.get("id").cloned().unwrap_or(Value::Null),
                "result": { "tools": [{ "name": "catdesk_instruction" }] }
            })),
            _ => Json(
                json!({ "jsonrpc": "2.0", "id": body.get("id").cloned().unwrap_or(Value::Null), "result": {} }),
            ),
        }
    }

    async fn malformed_mcp() -> impl IntoResponse {
        (StatusCode::OK, "not json")
    }

    async fn oversized_mcp(Json(body): Json<Value>) -> impl IntoResponse {
        if body.get("method").and_then(Value::as_str) == Some("initialize") {
            return Json(json!({
                "jsonrpc": "2.0",
                "id": body.get("id").cloned().unwrap_or(Value::Null),
                "result": { "serverInfo": { "name": "catdesk", "version": "test" } }
            }))
            .into_response();
        }
        Json(json!({
            "jsonrpc": "2.0",
            "id": body.get("id").cloned().unwrap_or(Value::Null),
            "result": { "padding": "x".repeat(MCP_SELF_CHECK_MAX_RESPONSE_BYTES + 1) }
        }))
        .into_response()
    }

    async fn redirect_mcp() -> impl IntoResponse {
        (
            StatusCode::TEMPORARY_REDIRECT,
            [(
                "location",
                format!("https://{}{}", "other.example.invalid", "/route/mcp"),
            )],
            "",
        )
    }

    async fn start_fake_mcp(router: Router) -> (String, JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.expect("serve");
        });
        (format!("http://{addr}{}", "/route/mcp"), server)
    }

    #[tokio::test]
    async fn local_mcp_self_check_succeeds_for_initialize_and_tools_list() {
        let (url, server) = start_fake_mcp(Router::new().route("/route/mcp", post(ok_mcp))).await;
        let report = run_mcp_endpoint_self_check(&url, None, Duration::from_secs(2), false)
            .await
            .expect("self-check");
        assert!(report.initialize_ok);
        assert!(report.tools_list_ok);
        assert!(report.required_tools_present);
        assert_eq!(report.tool_count, 2);
        server.abort();
    }

    #[tokio::test]
    async fn mcp_self_check_rejects_missing_required_tool() {
        let (url, server) =
            start_fake_mcp(Router::new().route("/route/mcp", post(missing_tools_mcp))).await;
        let error = run_mcp_endpoint_self_check(&url, None, Duration::from_secs(2), false)
            .await
            .unwrap_err();
        assert!(error.contains("delegated_run_list"));
        server.abort();
    }

    #[tokio::test]
    async fn mcp_self_check_rejects_malformed_json_and_oversized_response() {
        let (malformed_url, malformed_server) =
            start_fake_mcp(Router::new().route("/route/mcp", post(malformed_mcp))).await;
        let malformed =
            run_mcp_endpoint_self_check(&malformed_url, None, Duration::from_secs(2), false)
                .await
                .unwrap_err();
        assert!(malformed.contains("malformed JSON-RPC"));
        malformed_server.abort();

        let (oversized_url, oversized_server) =
            start_fake_mcp(Router::new().route("/route/mcp", post(oversized_mcp))).await;
        let oversized =
            run_mcp_endpoint_self_check(&oversized_url, None, Duration::from_secs(2), false)
                .await
                .unwrap_err();
        assert!(oversized.contains("size limit"));
        oversized_server.abort();
    }

    #[tokio::test]
    async fn mcp_self_check_rejects_redirects() {
        let (url, server) =
            start_fake_mcp(Router::new().route("/route/mcp", post(redirect_mcp))).await;
        let error = run_mcp_endpoint_self_check(&url, None, Duration::from_secs(2), false)
            .await
            .unwrap_err();
        assert!(error.contains("redirect"));
        server.abort();
    }

    #[test]
    fn diagnostics_redact_route_and_endpoint() {
        let redacted = redact_full_mcp_url(&format!(
            "{}{}{}",
            "https://example.ngrok-free.app/", "AbCdEf123456789012345678", "/mcp"
        ));
        assert_eq!(redacted, "https://example.ngrok-free.app/<redacted>/mcp");
        assert!(!redacted.contains("AbCdEf"));
    }

    #[test]
    fn external_public_mcp_url_normalizes_origin_and_preserves_route() {
        let url = external_public_mcp_url(
            "HTTPS://EXAMPLE.NGROK-FREE.APP/",
            "/AbCdEf123456789012345678/mcp",
        )
        .expect("external url");

        assert_eq!(
            url,
            format!(
                "{}{}{}",
                "https://example.ngrok-free.app/", "AbCdEf123456789012345678", "/mcp"
            )
        );
        assert_ne!(
            connection_fingerprint(&url),
            connection_fingerprint(&format!(
                "{}{}{}",
                "https://example.ngrok-free.app/", "abcdef123456789012345678", "/mcp"
            ))
        );
    }

    #[test]
    fn connection_fingerprint_is_stable_for_same_normalized_endpoint() {
        let a = connection_fingerprint(&format!(
            "{}{}{}",
            "HTTPS://EXAMPLE.NGROK-FREE.APP/", "AbCd", "/mcp"
        ));
        let b = connection_fingerprint(&format!(
            "{}{}{}",
            "https://example.ngrok-free.app/", "AbCd", "/mcp"
        ));
        assert_eq!(a, b);
        assert_eq!(
            a,
            connection_fingerprint(&format!(
                "{}{}{}",
                "https://example.ngrok-free.app/", "AbCd", "/mcp"
            ))
        );
    }

    #[test]
    fn connection_fingerprint_preserves_route_path_casing() {
        let mixed = connection_fingerprint(&format!(
            "{}{}{}",
            "https://example.ngrok-free.app/", "AbCd", "/mcp"
        ));
        let lower = connection_fingerprint(&format!(
            "{}{}{}",
            "https://example.ngrok-free.app/", "abcd", "/mcp"
        ));
        assert_ne!(mixed, lower);
    }

    #[test]
    fn connection_fingerprint_ignores_optional_final_trailing_slash() {
        let without = connection_fingerprint(&format!(
            "{}{}{}",
            "https://example.ngrok-free.app/", "AbCd", "/mcp"
        ));
        let with = connection_fingerprint(&format!(
            "{}{}{}",
            "https://example.ngrok-free.app/", "AbCd", "/mcp/"
        ));
        assert_eq!(without, with);
    }

    #[test]
    fn route_rotation_marks_restart_required() {
        let mut mcp = McpTransportConfig::default();
        rotate_route_restart_required(&mut mcp);
        assert!(mcp.route_rotation.restart_required);
        assert!(mcp.route_rotation.pending_route_id.is_some());
    }

    #[test]
    fn route_rotation_state_invariants_are_enforced() {
        let tunnel = TunnelConfig::default();
        validate_transport(&McpTransportConfig::default(), &tunnel)
            .expect("inactive route rotation is valid");

        let pending = "abcdefghijklmnopqrstuvwxyz012345";
        let mcp = McpTransportConfig {
            route_rotation: RouteRotationState {
                pending_route_id: Some(pending.into()),
                restart_required: true,
            },
            ..McpTransportConfig::default()
        };
        validate_transport(&mcp, &tunnel).expect("pending restart state is valid");

        let mcp = McpTransportConfig {
            route_rotation: RouteRotationState {
                pending_route_id: None,
                restart_required: true,
            },
            ..McpTransportConfig::default()
        };
        assert!(validate_transport(&mcp, &tunnel).is_err());

        let mcp = McpTransportConfig {
            route_rotation: RouteRotationState {
                pending_route_id: Some(pending.into()),
                restart_required: false,
            },
            ..McpTransportConfig::default()
        };
        assert!(validate_transport(&mcp, &tunnel).is_err());
    }

    #[test]
    fn zero_port_and_non_loopback_stable_bind_are_rejected() {
        let mut mcp = McpTransportConfig {
            port: 0,
            ..McpTransportConfig::default()
        };
        let tunnel = TunnelConfig::default();
        assert!(validate_transport(&mcp, &tunnel).is_err());

        mcp.port = DEFAULT_MCP_PORT;
        mcp.bind_host = "0.0.0.0".into();
        let tunnel = TunnelConfig {
            mode: TunnelMode::ManagedStableNgrok,
            ngrok_domain: Some("example.ngrok-free.app".into()),
            ..TunnelConfig::default()
        };
        assert!(validate_transport(&mcp, &tunnel).is_err());
    }

    #[test]
    fn malformed_public_base_url_and_ngrok_domain_are_rejected() {
        assert!(normalize_public_base_url("http://example.com").is_err());
        assert!(normalize_public_base_url("https://example.com/path").is_err());
        assert!(normalize_public_base_url("https://user:pass@example.com").is_err());
        assert!(normalize_public_base_url("https://example..com").is_err());
        assert!(normalize_public_base_url("https://-example.com").is_err());
        assert!(normalize_public_base_url("https://example.com:bad").is_err());
        assert!(normalize_ngrok_domain("https://example.ngrok-free.app/path").is_err());
        assert!(normalize_ngrok_domain("not-a-domain").is_err());
        assert!(normalize_ngrok_domain("example..ngrok-free.app").is_err());
        assert!(normalize_ngrok_domain("-example.ngrok-free.app").is_err());
        assert!(normalize_ngrok_domain("example.ngrok-free.app:443").is_err());
    }

    #[test]
    fn mode_classification_is_explicit() {
        assert!(!requires_persistent_route(
            TunnelMode::ManagedEphemeralNgrok
        ));
        assert!(requires_persistent_route(TunnelMode::ManagedStableNgrok));
        assert!(requires_persistent_route(TunnelMode::ExternalTunnel));
        assert!(requires_persistent_route(TunnelMode::OpenaiSecureTunnel));
        assert!(!requires_loopback(TunnelMode::ManagedEphemeralNgrok));
        assert!(requires_loopback(TunnelMode::OpenaiSecureTunnel));
        assert!(manages_tunnel_process(TunnelMode::ManagedStableNgrok));
        assert!(!manages_tunnel_process(TunnelMode::ExternalTunnel));
    }

    #[test]
    fn process_ownership_combinations_are_validated() {
        let mcp = McpTransportConfig {
            route_id: Some("abcdefghijklmnopqrstuvwxyz012345".into()),
            ..McpTransportConfig::default()
        };
        let external = TunnelConfig {
            mode: TunnelMode::ExternalTunnel,
            public_base_url: Some("https://example.invalid".into()),
            manage_process: true,
            ..TunnelConfig::default()
        };
        assert!(validate_transport(&mcp, &external).is_err());

        let external = TunnelConfig {
            manage_process: false,
            ..external
        };
        validate_transport(&mcp, &external).expect("external tunnel is externally owned");
    }

    #[test]
    fn openai_secure_tunnel_requires_route_and_loopback_classification() {
        assert!(requires_persistent_route(TunnelMode::OpenaiSecureTunnel));
        assert!(requires_loopback(TunnelMode::OpenaiSecureTunnel));

        let mut mcp = McpTransportConfig::default();
        let tunnel = TunnelConfig {
            mode: TunnelMode::OpenaiSecureTunnel,
            manage_process: false,
            ..TunnelConfig::default()
        };
        assert!(ensure_persistent_route_if_required(&mut mcp, &tunnel));
        assert!(mcp.route_id.is_some());
        validate_transport(&mcp, &tunnel).expect("openai mode foundation validates");
    }

    #[test]
    fn identity_snapshot_serializes_unknown_dirty_build_state() {
        let snapshot = build_identity_snapshot(
            "installation".into(),
            "server".into(),
            "unix:1".into(),
            "C:/workspace",
            TunnelMode::ManagedEphemeralNgrok,
            None,
        );
        let value = serde_json::to_value(snapshot).expect("serialize identity");
        assert_eq!(value["dirtyBuild"], "unknown");
        assert!(
            value["binaryFingerprint"]
                .as_str()
                .is_some_and(|v| !v.is_empty())
        );
    }
}
