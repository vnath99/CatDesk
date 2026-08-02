use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(not(test))]
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::browser::DetectedBrowser;
use crate::mascot::{self, MascotPack};
use crate::openai_tunnel::OpenaiTunnelConfig;
use crate::theme;
use crate::tunnel::{
    AtomicWritePlan, ConfigSaveOutcome, McpTransportConfig, TransportHealthSnapshot,
    TransportIdentityConfig, TransportIdentitySnapshot, TransportSecurityConfig, TunnelConfig,
    build_identity_snapshot, current_startup_time, dirty_build_state, ensure_installation_id,
    ensure_persistent_route_if_required, promote_pending_route_after_restart,
    tmp_path_for_atomic_write, validate_transport,
};

/// Log entry displayed in the TUI.
#[derive(Clone)]
pub struct LogEntry {
    pub time: String,
    pub level: &'static str,
    pub message: String,
}

/// MCP request flow rendered as a single timeline line.
#[derive(Clone)]
pub struct FlowLane {
    pub flow_id: String,
    pub short_id: String,
    pub events: Vec<String>,
    pub bootstrap_status_active: bool,
    pub bootstrap_completed_steps: usize,
    pub bootstrap_pending_steps: VecDeque<usize>,
    pub bootstrap_status_close_deadline_ms: Option<u128>,
    pub anim_queue: VecDeque<FlowAnimSegment>,
    pub last_direction: FlowDirection,
    pub closing_started_ms: Option<u128>,
    pub closing_step_ms: u64,
}

#[derive(Clone, Default)]
pub struct FlowBootstrapProgress {
    pub completed_steps: usize,
    pub pending_steps: VecDeque<usize>,
}

const APP_CONFIG_DIR_NAME: &str = ".catdesk";
const APP_CONFIG_FILE_NAME: &str = "config.toml";
const DEFERRED_ACL_WARNING: &str =
    "owner-only config ACL mutation is disabled pending a SID-based T-0025 hardening pass";
#[cfg(not(test))]
static ACL_WARNING_PRESENTED: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
thread_local! {
    static ACL_WARNING_PRESENTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub tool_call_count: u64,
}

impl UsageTotals {
    pub fn accumulate(&mut self, input_tokens: u64, output_tokens: u64, tool_call_count: u64) {
        self.input_tokens = self.input_tokens.saturating_add(input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(output_tokens);
        self.total_tokens = self.input_tokens.saturating_add(self.output_tokens);
        self.tool_call_count = self.tool_call_count.saturating_add(tool_call_count);
    }

    fn normalized(mut self) -> Self {
        self.total_tokens = self.input_tokens.saturating_add(self.output_tokens);
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentsPathMode {
    #[default]
    Default,
    Workspace,
    Catdesk,
    Codex,
    Disabled,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenStatsLayout {
    Disable,
    #[default]
    Right,
    Bottom,
}

impl TokenStatsLayout {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Right => "right",
            Self::Bottom => "bottom",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShowDetailMode {
    Disable,
    #[default]
    Expanded,
    Collapsed,
}

impl ShowDetailMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Expanded => "expanded",
            Self::Collapsed => "collapsed",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub ngrok_authtoken: Option<String>,
    #[serde(default)]
    pub mcp: McpTransportConfig,
    #[serde(default)]
    pub tunnel: TunnelConfig,
    #[serde(default)]
    pub openai_tunnel: OpenaiTunnelConfig,
    #[serde(default)]
    pub security: TransportSecurityConfig,
    #[serde(default)]
    pub identity: TransportIdentityConfig,
    #[serde(default)]
    pub agents_path_mode: AgentsPathMode,
    #[serde(default)]
    pub token_stats_layout: TokenStatsLayout,
    #[serde(default)]
    pub show_detail_mode: ShowDetailMode,
    #[serde(default)]
    pub partner_binagotchy_seed: Option<String>,
    #[serde(default)]
    pub set_catdesk_as_co_author: bool,
    pub theme: String,
    pub mode: Mode,
    pub tool_mode: ToolMode,
    pub usage_totals: UsageTotals,
    pub selected_browser: Option<DetectedBrowser>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ngrok_authtoken: None,
            mcp: McpTransportConfig::default(),
            tunnel: TunnelConfig::default(),
            openai_tunnel: OpenaiTunnelConfig::default(),
            security: TransportSecurityConfig::default(),
            identity: TransportIdentityConfig::default(),
            agents_path_mode: AgentsPathMode::Default,
            token_stats_layout: TokenStatsLayout::Right,
            show_detail_mode: ShowDetailMode::Expanded,
            partner_binagotchy_seed: None,
            set_catdesk_as_co_author: false,
            theme: theme::DEFAULT_THEME_ID.to_string(),
            mode: Mode::Both,
            tool_mode: ToolMode::MultiTools,
            usage_totals: UsageTotals::default(),
            selected_browser: None,
        }
    }
}

impl AppConfig {
    fn normalized(mut self) -> Self {
        self.ngrok_authtoken = self
            .ngrok_authtoken
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.mcp.bind_host = self.mcp.bind_host.trim().to_string();
        self.mcp.route_id = crate::tunnel::normalize_optional_string(self.mcp.route_id.take());
        self.mcp.route_rotation.pending_route_id = crate::tunnel::normalize_optional_string(
            self.mcp.route_rotation.pending_route_id.take(),
        );
        self.tunnel.public_base_url =
            crate::tunnel::normalize_optional_string(self.tunnel.public_base_url.take());
        self.tunnel.ngrok_domain =
            crate::tunnel::normalize_optional_string(self.tunnel.ngrok_domain.take());
        self.tunnel.ngrok_config_path =
            crate::tunnel::normalize_optional_string(self.tunnel.ngrok_config_path.take());
        self.openai_tunnel = self.openai_tunnel.normalized();
        self.identity.installation_id =
            crate::tunnel::normalize_optional_string(self.identity.installation_id.take());
        self.identity.last_connection_fingerprint = crate::tunnel::normalize_optional_string(
            self.identity.last_connection_fingerprint.take(),
        );
        self.partner_binagotchy_seed = self
            .partner_binagotchy_seed
            .take()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());
        self.usage_totals = self.usage_totals.normalized();
        self
    }

    fn load_from_path(path: &Path) -> std::io::Result<Self> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e),
        };
        let config = toml::from_str::<Self>(&text).map_err(std::io::Error::other)?;
        let config = config.normalized();
        validate_transport(&config.mcp, &config.tunnel).map_err(std::io::Error::other)?;
        Ok(config)
    }

    #[allow(dead_code)]
    fn load_or_initialize_from_path(path: &Path) -> std::io::Result<Self> {
        Self::load_or_initialize_from_path_with_warnings(path).map(|(config, _warnings)| config)
    }

    fn load_or_initialize_from_path_with_warnings(
        path: &Path,
    ) -> std::io::Result<(Self, Vec<String>)> {
        let existing_config = path.exists();
        let mut config = Self::load_from_path(path)?;
        let mut changed = false;
        let mut warnings = Vec::new();
        changed |= ensure_installation_id(&mut config.identity);
        changed |= promote_pending_route_after_restart(&mut config.mcp);
        changed |= ensure_persistent_route_if_required(&mut config.mcp, &config.tunnel);
        validate_transport(&config.mcp, &config.tunnel).map_err(std::io::Error::other)?;
        if changed {
            if existing_config {
                warnings.extend(create_one_time_migration_backup(path)?);
            }
            warnings.extend(config.save_to_path_checked(path)?.warnings);
        }
        Ok((config, warnings))
    }

    fn save_to_path(&self, path: &Path) -> std::io::Result<()> {
        self.save_to_path_checked(path).map(|_| ())
    }

    fn save_to_path_checked(&self, path: &Path) -> std::io::Result<ConfigSaveOutcome> {
        self.save_to_path_with_plan(path, &AtomicWritePlan::default(), || Ok(()))
    }

    fn save_to_path_with_plan<F>(
        &self,
        path: &Path,
        plan: &AtomicWritePlan,
        before_replace: F,
    ) -> std::io::Result<ConfigSaveOutcome>
    where
        F: FnOnce() -> std::io::Result<()>,
    {
        let config = self.clone().normalized();
        validate_transport(&config.mcp, &config.tunnel).map_err(std::io::Error::other)?;
        let parent = path.parent().ok_or_else(|| {
            std::io::Error::other("failed to resolve config directory for config.toml")
        })?;
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }

        let text = toml::to_string_pretty(&config).map_err(std::io::Error::other)?;
        let mut options = OpenOptions::new();
        options.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let tmp_path = tmp_path_for_atomic_write(path, plan)?;
        let write_result = (|| {
            let mut file = options.open(&tmp_path)?;
            use std::io::Write as _;
            file.write_all(text.as_bytes())?;
            file.flush()?;
            file.sync_all()?;
            drop(file);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600))?;
            }
            #[cfg(test)]
            if let Some(message) = plan.fail_after_tmp_created {
                return Err(std::io::Error::other(message));
            }
            Ok(())
        })();
        if let Err(error) = write_result {
            cleanup_tmp_path_preserving_error(&tmp_path, error, plan)?;
        }
        if let Err(error) = before_replace() {
            cleanup_tmp_path_preserving_error(&tmp_path, error, plan)?;
        }
        #[cfg(test)]
        if let Some(message) = plan.fail_replace_with {
            cleanup_tmp_path_preserving_error(&tmp_path, std::io::Error::other(message), plan)?;
        }
        if let Err(error) = replace_config_file(&tmp_path, path) {
            cleanup_tmp_path_preserving_error(&tmp_path, error, plan)?;
        }
        let warnings = harden_config_permissions(path);
        Ok(ConfigSaveOutcome { warnings })
    }
}

fn cleanup_tmp_path_preserving_error(
    tmp_path: &Path,
    error: std::io::Error,
    _plan: &AtomicWritePlan,
) -> std::io::Result<()> {
    #[cfg(test)]
    let cleanup_result = if let Some(message) = _plan.fail_cleanup_with {
        Err(std::io::Error::other(message))
    } else {
        fs::remove_file(tmp_path)
    };
    #[cfg(not(test))]
    let cleanup_result = fs::remove_file(tmp_path);

    match cleanup_result {
        Ok(()) => {}
        Err(cleanup_error) if cleanup_error.kind() == std::io::ErrorKind::NotFound => {}
        Err(cleanup_error) => {
            return Err(std::io::Error::other(format!(
                "config save failed: {error}; additionally failed to remove temporary config file (<redacted config temp>): {cleanup_error}"
            )));
        }
    }
    Err(error)
}

fn create_one_time_migration_backup(path: &Path) -> std::io::Result<Vec<String>> {
    let backup_path = path.with_file_name(format!("{APP_CONFIG_FILE_NAME}.pre-t0025b-backup"));
    if backup_path.exists() {
        return Ok(Vec::new());
    }
    fs::copy(path, &backup_path)?;
    Ok(harden_config_permissions(&backup_path))
}

#[cfg(not(windows))]
fn replace_config_file(tmp_path: &Path, path: &Path) -> std::io::Result<()> {
    fs::rename(tmp_path, path)
}

#[cfg(windows)]
fn replace_config_file(tmp_path: &Path, path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let existing = wide(tmp_path);
    let new = wide(path);
    let ok = unsafe {
        MoveFileExW(
            existing.as_ptr(),
            new.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn harden_config_permissions(_path: &Path) -> Vec<String> {
    #[cfg(windows)]
    {
        vec![DEFERRED_ACL_WARNING.into()]
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

fn presentation_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut presented = Vec::new();
    for warning in warnings {
        if warning == DEFERRED_ACL_WARNING {
            if mark_acl_warning_presented() {
                continue;
            }
            presented.push(DEFERRED_ACL_WARNING.to_string());
        } else {
            presented.push(warning);
        }
    }
    presented
}

#[cfg(not(test))]
fn mark_acl_warning_presented() -> bool {
    ACL_WARNING_PRESENTED.swap(true, Ordering::Relaxed)
}

#[cfg(test)]
fn mark_acl_warning_presented() -> bool {
    ACL_WARNING_PRESENTED.with(|presented| {
        let was_presented = presented.get();
        presented.set(true);
        was_presented
    })
}

#[cfg(test)]
fn reset_presentation_warning_dedupe_for_test() {
    ACL_WARNING_PRESENTED.with(|presented| presented.set(false));
}

/// Direction for flow animation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FlowDirection {
    Forward,  // request: Your computer -> ChatGPT Web
    Backward, // response: ChatGPT Web -> Your computer
}

pub enum ServerUiEvent {
    IncrementRequestCount,
    SetRemoteConnected(bool),
    RecordFlow {
        flow_id: String,
        events: Vec<String>,
        direction: FlowDirection,
    },
    BeginFlowClose {
        flow_id: String,
    },
    Log {
        level: &'static str,
        message: String,
    },
}

/// Per-flow queued animation segment.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FlowAnimKind {
    Move,
    Turn,
}

#[derive(Clone, Copy)]
pub struct FlowAnimSegment {
    pub kind: FlowAnimKind,
    pub direction: FlowDirection,
    pub started_ms: u128,
    pub ends_ms: u128,
    pub step_ms: u64,
    pub start_cells: usize,
    pub end_cells: usize,
}

#[derive(Clone, Copy)]
pub struct FlowBootstrapStep {
    pub event: &'static str,
    pub label: &'static str,
}

#[derive(Clone, Copy)]
pub struct FlowBootstrapPhase {
    pub title: &'static str,
    pub steps: &'static [FlowBootstrapStep],
}

const FLOW_BOOTSTRAP_PHASE_1_STEPS: &[FlowBootstrapStep] = &[
    FlowBootstrapStep {
        event: "initialize",
        label: "initialize#1",
    },
    FlowBootstrapStep {
        event: "initialize",
        label: "initialize#2",
    },
    FlowBootstrapStep {
        event: "notifications/initialized",
        label: "initialized",
    },
    FlowBootstrapStep {
        event: "tools/list",
        label: "tools/list",
    },
];

const FLOW_BOOTSTRAP_PHASE_2_STEPS: &[FlowBootstrapStep] = &[
    FlowBootstrapStep {
        event: "initialize",
        label: "initialize#1",
    },
    FlowBootstrapStep {
        event: "initialize",
        label: "initialize#2",
    },
    FlowBootstrapStep {
        event: "notifications/initialized",
        label: "initialized",
    },
    FlowBootstrapStep {
        event: "resources/list",
        label: "resources/list",
    },
];

const FLOW_BOOTSTRAP_WIDGET_READ_STEPS: &[FlowBootstrapStep] = &[
    FlowBootstrapStep {
        event: "resources/read:run_command",
        label: "run_command",
    },
    FlowBootstrapStep {
        event: "resources/read:catdesk_instruction",
        label: "instruction",
    },
    FlowBootstrapStep {
        event: "resources/read:read",
        label: "read",
    },
    FlowBootstrapStep {
        event: "resources/read:search",
        label: "search",
    },
    FlowBootstrapStep {
        event: "resources/read:write",
        label: "write",
    },
    FlowBootstrapStep {
        event: "resources/read:edit",
        label: "edit",
    },
    FlowBootstrapStep {
        event: "resources/read:delete",
        label: "delete",
    },
];

const FLOW_BOOTSTRAP_PHASE_3_STEPS: &[FlowBootstrapStep] = &[
    FlowBootstrapStep {
        event: "initialize",
        label: "initialize#1",
    },
    FlowBootstrapStep {
        event: "initialize",
        label: "initialize#2",
    },
    FlowBootstrapStep {
        event: "notifications/initialized",
        label: "initialized",
    },
    FLOW_BOOTSTRAP_WIDGET_READ_STEPS[0],
    FLOW_BOOTSTRAP_WIDGET_READ_STEPS[1],
    FLOW_BOOTSTRAP_WIDGET_READ_STEPS[2],
    FLOW_BOOTSTRAP_WIDGET_READ_STEPS[3],
    FLOW_BOOTSTRAP_WIDGET_READ_STEPS[4],
    FLOW_BOOTSTRAP_WIDGET_READ_STEPS[5],
    FLOW_BOOTSTRAP_WIDGET_READ_STEPS[6],
];

const FLOW_BOOTSTRAP_PHASE_4_STEPS: &[FlowBootstrapStep] = &[
    FlowBootstrapStep {
        event: "initialize",
        label: "initialize#1",
    },
    FlowBootstrapStep {
        event: "initialize",
        label: "initialize#2",
    },
    FlowBootstrapStep {
        event: "notifications/initialized",
        label: "initialized",
    },
    FlowBootstrapStep {
        event: "tools/list",
        label: "tools/list",
    },
    FLOW_BOOTSTRAP_WIDGET_READ_STEPS[0],
    FLOW_BOOTSTRAP_WIDGET_READ_STEPS[1],
    FLOW_BOOTSTRAP_WIDGET_READ_STEPS[2],
    FLOW_BOOTSTRAP_WIDGET_READ_STEPS[3],
    FLOW_BOOTSTRAP_WIDGET_READ_STEPS[4],
    FLOW_BOOTSTRAP_WIDGET_READ_STEPS[5],
    FLOW_BOOTSTRAP_WIDGET_READ_STEPS[6],
];

const FLOW_BOOTSTRAP_PHASE_5_STEPS: &[FlowBootstrapStep] = &[
    FlowBootstrapStep {
        event: "initialize",
        label: "initialize#1",
    },
    FlowBootstrapStep {
        event: "initialize",
        label: "initialize#2",
    },
    FlowBootstrapStep {
        event: "notifications/initialized",
        label: "initialized",
    },
    FlowBootstrapStep {
        event: "resources/list",
        label: "resources/list",
    },
];

pub const FLOW_BOOTSTRAP_PHASES: &[FlowBootstrapPhase] = &[
    FlowBootstrapPhase {
        title: "Checking tools",
        steps: FLOW_BOOTSTRAP_PHASE_1_STEPS,
    },
    FlowBootstrapPhase {
        title: "Checking resources",
        steps: FLOW_BOOTSTRAP_PHASE_2_STEPS,
    },
    FlowBootstrapPhase {
        title: "Loading widgets",
        steps: FLOW_BOOTSTRAP_PHASE_3_STEPS,
    },
    FlowBootstrapPhase {
        title: "Refreshing widgets",
        steps: FLOW_BOOTSTRAP_PHASE_4_STEPS,
    },
    FlowBootstrapPhase {
        title: "Final resource check",
        steps: FLOW_BOOTSTRAP_PHASE_5_STEPS,
    },
];

/// Which MCP backends to enable.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Computer, // run_command only
    Browser,  // chrome-devtools-mcp only
    Both,     // both
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Computer => "Computer",
            Mode::Browser => "Browser",
            Mode::Both => "Both",
        }
    }
    pub fn computer_enabled(self) -> bool {
        matches!(self, Mode::Computer | Mode::Both)
    }
    pub fn browser_enabled(self) -> bool {
        matches!(self, Mode::Browser | Mode::Both)
    }
}

/// Which local toolset to expose in MCP.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolMode {
    MultiTools,     // codex/claude-style workspace tools
    SupervisorOnly, // delegated supervisor tools plus read-only inspection
    ReadOnly,       // read-only safe tools only
}

impl ToolMode {
    pub fn all() -> &'static [Self] {
        const TOOL_MODES: [ToolMode; 3] = [
            ToolMode::MultiTools,
            ToolMode::SupervisorOnly,
            ToolMode::ReadOnly,
        ];
        &TOOL_MODES
    }

    pub fn label(self) -> &'static str {
        match self {
            ToolMode::MultiTools => "multi-tools",
            ToolMode::SupervisorOnly => "supervisor-only",
            ToolMode::ReadOnly => "read-only",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            ToolMode::MultiTools => "Expose workspace read/write tools plus run_command.",
            ToolMode::SupervisorOnly => {
                "Expose read-only inspection plus delegated supervisor tools."
            }
            ToolMode::ReadOnly => "Expose safe read-only workspace tools only.",
        }
    }

    pub fn run_command_enabled(self) -> bool {
        matches!(self, ToolMode::MultiTools)
    }

    pub fn write_tools_enabled(self) -> bool {
        matches!(self, ToolMode::MultiTools)
    }

    pub fn supervisor_tools_enabled(self) -> bool {
        matches!(self, ToolMode::MultiTools | ToolMode::SupervisorOnly)
    }

    pub fn read_only(self) -> bool {
        matches!(self, ToolMode::ReadOnly | ToolMode::SupervisorOnly)
    }
}

/// Shared application state across server, ngrok, and TUI.
pub struct AppState {
    pub theme: String,
    pub mode: Mode,
    pub tool_mode: ToolMode,
    pub mcp_slug: String,
    pub installation_id: String,
    #[allow(dead_code)]
    pub server_instance_id: String,
    #[allow(dead_code)]
    pub startup_time: String,
    pub tunnel_config: TunnelConfig,
    pub transport_identity: TransportIdentityConfig,
    pub transport_health: TransportHealthSnapshot,
    pub server_running: bool,
    pub ngrok_running: bool,
    pub ngrok_url: Option<String>,
    pub remote_connected: bool,
    pub last_remote_activity_ms: Option<u128>,
    pub devtools_running: bool,
    pub port: u16,
    pub workspace_root: String,
    pub mascot_seed: u64,
    pub partner_binagotchy_seed: Option<String>,
    pub set_catdesk_as_co_author: bool,
    pub mascot: MascotPack,
    pub detected_browsers: Vec<DetectedBrowser>,
    pub selected_browser: Option<DetectedBrowser>,
    pub logs: Vec<LogEntry>,
    pub flows: Vec<FlowLane>,
    pub flow_bootstrap_progress: HashMap<String, FlowBootstrapProgress>,
    pub request_count: u64,
    pub usage_totals: UsageTotals,
    pub session_usage_totals: UsageTotals,
    config_path: PathBuf,
    pub server_handle: Option<tokio::task::JoinHandle<()>>,
    pub ngrok_task: Option<tokio::task::JoinHandle<()>>,
    pub remote_browser_child: Option<tokio::process::Child>,
    pub devtools_child: Option<tokio::process::Child>,
}

pub type SharedState = Arc<Mutex<AppState>>;

pub const FLOW_ANIM_CELLS: usize = 32;
const FLOW_LINK_CELLS: u64 = FLOW_ANIM_CELLS as u64;
const FLOW_CHAIN_DELAY_CELLS: u64 = 0;
const FLOW_FORWARD_ANIMATION_DURATION_MS: u64 = 125;
const FLOW_BACKWARD_ANIMATION_DURATION_MS: u64 = 125;
const FLOW_STEP_FIXED_MS: u64 =
    (FLOW_FORWARD_ANIMATION_DURATION_MS + FLOW_LINK_CELLS - 1) / FLOW_LINK_CELLS;
const FLOW_TURN_TRANSITION_MS: u64 = 24;
const FLOW_CLOSE_PRUNE_MULTIPLIER: u64 = 3;
const FLOW_BOOTSTRAP_STATUS_CLOSE_DELAY_MS: u128 = 3_000;

fn short_flow_id(flow_id: &str) -> String {
    flow_id[..flow_id.len().min(8)].to_string()
}

pub fn user_home_dir() -> std::io::Result<PathBuf> {
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home));
    }

    #[cfg(windows)]
    {
        if let Some(user_profile) =
            std::env::var_os("USERPROFILE").filter(|value| !value.is_empty())
        {
            return Ok(PathBuf::from(user_profile));
        }

        let home_drive = std::env::var_os("HOMEDRIVE").filter(|value| !value.is_empty());
        let home_path = std::env::var_os("HOMEPATH").filter(|value| !value.is_empty());
        if let (Some(home_drive), Some(home_path)) = (home_drive, home_path) {
            let mut path = PathBuf::from(home_drive);
            path.push(home_path);
            return Ok(path);
        }
    }

    Err(std::io::Error::other(
        "could not resolve the user home directory from HOME, USERPROFILE, or HOMEDRIVE/HOMEPATH",
    ))
}

pub fn app_config_path() -> std::io::Result<PathBuf> {
    Ok(user_home_dir()?
        .join(APP_CONFIG_DIR_NAME)
        .join(APP_CONFIG_FILE_NAME))
}

pub fn load_app_config() -> std::io::Result<AppConfig> {
    AppConfig::load_from_path(&app_config_path()?)
}

pub fn load_ngrok_authtoken() -> std::io::Result<Option<String>> {
    Ok(load_app_config()?.ngrok_authtoken)
}

pub fn save_ngrok_authtoken(token: &str) -> std::io::Result<PathBuf> {
    let path = app_config_path()?;
    let mut config = AppConfig::load_from_path(&path)?;
    config.ngrok_authtoken = Some(token.to_string());
    config.save_to_path(&path)?;
    Ok(path)
}

pub fn save_agents_path_mode(mode: AgentsPathMode) -> std::io::Result<PathBuf> {
    let path = app_config_path()?;
    let mut config = AppConfig::load_from_path(&path)?;
    config.agents_path_mode = mode;
    config.save_to_path(&path)?;
    Ok(path)
}

pub fn save_token_stats_layout(layout: TokenStatsLayout) -> std::io::Result<PathBuf> {
    let path = app_config_path()?;
    let mut config = AppConfig::load_from_path(&path)?;
    config.token_stats_layout = layout;
    config.save_to_path(&path)?;
    Ok(path)
}

pub fn save_show_detail_mode(mode: ShowDetailMode) -> std::io::Result<PathBuf> {
    let path = app_config_path()?;
    let mut config = AppConfig::load_from_path(&path)?;
    config.show_detail_mode = mode;
    config.save_to_path(&path)?;
    Ok(path)
}

pub(crate) fn parse_seed_hex(seed: &str) -> std::io::Result<u64> {
    u64::from_str_radix(seed, 16).map_err(|error| {
        std::io::Error::other(format!("invalid partner Binagotchy seed `{seed}`: {error}"))
    })
}

fn now_hms() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

fn now_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn derive_flow_step_ms() -> u64 {
    FLOW_STEP_FIXED_MS
}

fn prune_finished_segments(queue: &mut VecDeque<FlowAnimSegment>, now_ms: u128) {
    while let Some(seg) = queue.front() {
        if seg.ends_ms <= now_ms {
            queue.pop_front();
        } else {
            break;
        }
    }
}

fn current_queue_segment(
    queue: &VecDeque<FlowAnimSegment>,
    now_ms: u128,
) -> Option<FlowAnimSegment> {
    if let Some(seg) = queue
        .iter()
        .find(|seg| seg.started_ms <= now_ms && now_ms < seg.ends_ms)
    {
        return Some(*seg);
    }
    queue.front().copied()
}

pub(crate) fn flow_anim_lit_count(seg: FlowAnimSegment, now_ms: u128) -> usize {
    if seg.started_ms >= seg.ends_ms {
        return seg.end_cells;
    }
    if now_ms <= seg.started_ms {
        return seg.start_cells;
    }
    if now_ms >= seg.ends_ms {
        return seg.end_cells;
    }

    let duration_ms = seg.ends_ms.saturating_sub(seg.started_ms);
    if duration_ms == 0 {
        return seg.end_cells;
    }

    let elapsed_ms = now_ms.saturating_sub(seg.started_ms);
    let distance = seg.end_cells.abs_diff(seg.start_cells) as u128;
    let progressed = ((distance * elapsed_ms) / duration_ms) as usize;

    if seg.end_cells >= seg.start_cells {
        (seg.start_cells + progressed).min(seg.end_cells)
    } else {
        seg.start_cells
            .saturating_sub(progressed.min(seg.start_cells - seg.end_cells))
    }
}

fn move_segment_duration_ms(
    direction: FlowDirection,
    _step_ms: u64,
    start_cells: usize,
    end_cells: usize,
) -> u128 {
    let cells_to_travel = end_cells.abs_diff(start_cells) as u128;
    if cells_to_travel == 0 {
        return 0;
    }
    let base_duration_ms = match direction {
        FlowDirection::Forward => FLOW_FORWARD_ANIMATION_DURATION_MS as u128,
        FlowDirection::Backward => FLOW_BACKWARD_ANIMATION_DURATION_MS as u128,
    };
    ((cells_to_travel + FLOW_CHAIN_DELAY_CELLS as u128) * base_duration_ms)
        .div_ceil(FLOW_LINK_CELLS as u128)
}

fn enqueue_flow_segment(
    queue: &mut VecDeque<FlowAnimSegment>,
    direction: FlowDirection,
    now_ms: u128,
    step_ms: u64,
) {
    prune_finished_segments(queue, now_ms);

    let current_seg = current_queue_segment(queue, now_ms);
    let current_direction = current_seg
        .map(|seg| seg.direction)
        .or_else(|| queue.back().map(|seg| seg.direction));
    let current_cells = current_seg
        .map(|seg| flow_anim_lit_count(seg, now_ms))
        .or_else(|| queue.back().map(|seg| seg.end_cells))
        .unwrap_or(0)
        .min(FLOW_ANIM_CELLS);

    queue.clear();

    let mut start_ms = now_ms;
    let mut move_start_cells = 0usize;

    if let Some(current_direction) = current_direction {
        if current_direction == direction {
            move_start_cells = current_cells;
        } else if current_cells > 0 {
            let turn_end = start_ms + FLOW_TURN_TRANSITION_MS as u128;
            queue.push_back(FlowAnimSegment {
                kind: FlowAnimKind::Turn,
                direction: current_direction,
                started_ms: start_ms,
                ends_ms: turn_end,
                step_ms,
                start_cells: current_cells,
                end_cells: 0,
            });
            start_ms = turn_end;
        }
    }

    let move_end =
        start_ms + move_segment_duration_ms(direction, step_ms, move_start_cells, FLOW_ANIM_CELLS);
    if move_end > start_ms {
        queue.push_back(FlowAnimSegment {
            kind: FlowAnimKind::Move,
            direction,
            started_ms: start_ms,
            ends_ms: move_end,
            step_ms,
            start_cells: move_start_cells,
            end_cells: FLOW_ANIM_CELLS,
        });
    }
}

fn flow_bootstrap_step(index: usize) -> Option<&'static FlowBootstrapStep> {
    let mut offset = 0;
    for phase in FLOW_BOOTSTRAP_PHASES {
        let end = offset + phase.steps.len();
        if index < end {
            return phase.steps.get(index - offset);
        }
        offset = end;
    }
    None
}

fn flow_bootstrap_steps_total() -> usize {
    FLOW_BOOTSTRAP_PHASES
        .iter()
        .map(|phase| phase.steps.len())
        .sum()
}

fn events_start_bootstrap_status(events: &[String]) -> bool {
    events.iter().any(|event| event == "initialize")
}

fn is_bootstrap_status_event(event: &str) -> bool {
    FLOW_BOOTSTRAP_PHASES
        .iter()
        .flat_map(|phase| phase.steps)
        .any(|step| step.event == event)
}

fn events_are_bootstrap_status_events(events: &[String]) -> bool {
    events.iter().all(|event| is_bootstrap_status_event(event))
}

fn advance_bootstrap_progress(
    completed_steps: &mut usize,
    pending_steps: &mut VecDeque<usize>,
    events: &[String],
    direction: FlowDirection,
) {
    match direction {
        FlowDirection::Forward => {
            for event in events {
                let next_index = completed_steps.saturating_add(pending_steps.len());
                let Some(step) = flow_bootstrap_step(next_index) else {
                    break;
                };
                if step.event != event {
                    continue;
                }
                if step.event == "notifications/initialized" {
                    *completed_steps = next_index + 1;
                    continue;
                }
                pending_steps.push_back(next_index);
            }
        }
        FlowDirection::Backward => {
            for event in events {
                let Some(pending_index) = pending_steps.front().copied() else {
                    break;
                };
                let Some(step) = flow_bootstrap_step(pending_index) else {
                    pending_steps.clear();
                    break;
                };
                if step.event == event {
                    pending_steps.pop_front();
                    *completed_steps = pending_index + 1;
                }
            }
        }
    }
}

impl AppState {
    pub fn new(port: u16, workspace_root: String) -> std::io::Result<Self> {
        let config_path = app_config_path()?;
        Self::from_config_path_with_archive(port, workspace_root, config_path, true)
    }

    pub(crate) fn new_headless(
        port: u16,
        workspace_root: String,
        config_path: PathBuf,
    ) -> std::io::Result<Self> {
        Self::from_config_path_with_archive(port, workspace_root, config_path, false)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        port: u16,
        workspace_root: String,
        config_path: PathBuf,
    ) -> std::io::Result<Self> {
        Self::from_config_path_with_archive(port, workspace_root, config_path, false)
    }

    #[cfg(test)]
    fn from_config_path(
        port: u16,
        workspace_root: String,
        config_path: PathBuf,
    ) -> std::io::Result<Self> {
        Self::from_config_path_with_archive(port, workspace_root, config_path, false)
    }

    fn from_config_path_with_archive(
        port: u16,
        workspace_root: String,
        config_path: PathBuf,
        _archive_startup_mascot: bool,
    ) -> std::io::Result<Self> {
        let (config, startup_warnings) =
            AppConfig::load_or_initialize_from_path_with_warnings(&config_path)?;
        if let Some(message) = config.tunnel.mode.unimplemented_message() {
            return Err(std::io::Error::other(message));
        }
        let installation_id = config
            .identity
            .installation_id
            .clone()
            .ok_or_else(|| std::io::Error::other("missing CatDesk installationId"))?;
        let partner_binagotchy_seed = config.partner_binagotchy_seed.clone();
        let mascot_seed = if let Some(seed) = partner_binagotchy_seed.as_deref() {
            parse_seed_hex(seed)?
        } else {
            rand::random::<u64>()
        };
        let mascot = mascot::build_workspace_mascot(mascot_seed);
        #[cfg(not(test))]
        if _archive_startup_mascot && partner_binagotchy_seed.is_none() {
            mascot::archive_startup_mascot(mascot_seed)?;
        }
        let mut state = Self {
            theme: config.theme,
            mode: config.mode,
            tool_mode: config.tool_mode,
            mcp_slug: config
                .mcp
                .route_id
                .clone()
                .unwrap_or_else(generate_mcp_slug),
            installation_id,
            server_instance_id: Uuid::new_v4().to_string(),
            startup_time: current_startup_time(),
            tunnel_config: config.tunnel.clone(),
            transport_identity: config.identity,
            transport_health: TransportHealthSnapshot::configured_unverified(
                config.tunnel.remote_self_check,
            ),
            server_running: false,
            ngrok_running: false,
            ngrok_url: None,
            remote_connected: false,
            last_remote_activity_ms: None,
            devtools_running: false,
            port,
            mascot_seed,
            partner_binagotchy_seed,
            set_catdesk_as_co_author: config.set_catdesk_as_co_author,
            mascot,
            workspace_root,
            detected_browsers: Vec::new(),
            selected_browser: config.selected_browser,
            logs: Vec::new(),
            flows: Vec::new(),
            flow_bootstrap_progress: HashMap::new(),
            request_count: 0,
            usage_totals: config.usage_totals,
            session_usage_totals: UsageTotals::default(),
            config_path,
            server_handle: None,
            ngrok_task: None,
            remote_browser_child: None,
            devtools_child: None,
        };
        for warning in presentation_warnings(startup_warnings) {
            state.log("WARN", warning);
        }
        Ok(state)
    }

    pub fn current_theme(&self) -> &'static theme::ThemeDef {
        theme::resolve(&self.theme)
    }

    pub fn mcp_path(&self) -> String {
        format!("/{}/mcp", self.mcp_slug)
    }

    pub fn public_mcp_url(&self) -> Option<String> {
        self.ngrok_url
            .as_ref()
            .map(|url| format!("{url}{}", self.mcp_path()))
    }

    pub fn log(&mut self, level: &'static str, message: String) {
        let now = now_hms();
        self.logs.push(LogEntry {
            time: now,
            level,
            message,
        });
        if self.logs.len() > 500 {
            self.logs.remove(0);
        }
    }

    fn app_config(&self) -> std::io::Result<AppConfig> {
        let mut config = AppConfig::load_from_path(&self.config_path)?;
        config.partner_binagotchy_seed = self.partner_binagotchy_seed.clone();
        config.set_catdesk_as_co_author = self.set_catdesk_as_co_author;
        config.theme = self.theme.clone();
        config.mode = self.mode;
        config.tool_mode = self.tool_mode;
        config.usage_totals = self.usage_totals.clone().normalized();
        config.selected_browser = self.selected_browser.clone();
        config.tunnel = self.tunnel_config.clone();
        config.identity = self.transport_identity.clone();
        config.identity.installation_id = Some(self.installation_id.clone());
        Ok(config.normalized())
    }

    pub fn persist_state(&self) -> std::io::Result<()> {
        self.app_config()?.save_to_path(&self.config_path)
    }

    pub fn persist_state_with_log(&mut self) {
        match self
            .app_config()
            .and_then(|config| config.save_to_path_checked(&self.config_path))
        {
            Ok(outcome) => {
                for warning in presentation_warnings(outcome.warnings) {
                    self.log("WARN", warning);
                }
            }
            Err(e) => {
                self.log("WARN", format!("Failed to persist app state: {e}"));
            }
        }
    }

    #[allow(dead_code)]
    pub fn transport_identity_snapshot(&self) -> TransportIdentitySnapshot {
        build_identity_snapshot(
            self.installation_id.clone(),
            self.server_instance_id.clone(),
            self.startup_time.clone(),
            &self.workspace_root,
            self.tunnel_config.mode,
            self.public_mcp_url().as_deref(),
        )
    }

    pub fn transport_status_payload(&self) -> serde_json::Value {
        let identity = self.transport_identity_snapshot();
        serde_json::json!({
            "toolName": "catdesk_transport_status",
            "transportMode": self.tunnel_config.mode.as_str(),
            "transportHealth": self.transport_health.health.as_str(),
            "localMcp": self.transport_health.local_mcp.clone(),
            "remoteCheckEnabled": self.tunnel_config.remote_self_check,
            "lastCheckedAt": self.transport_health.last_checked_at.clone(),
            "installationFingerprint": crate::tunnel::connection_fingerprint(&self.installation_id),
            "serverInstanceFingerprint": crate::tunnel::connection_fingerprint(&self.server_instance_id),
            "connectionFingerprint": self.transport_identity.last_connection_fingerprint.clone(),
            "buildState": dirty_build_state(),
            "warnings": self.transport_health.warnings.clone(),
            "redactedReason": self.transport_health.redacted_reason.clone(),
            "identity": {
                "gitCommit": identity.git_commit,
                "dirtyBuild": identity.dirty_build,
                "binaryFingerprint": identity.binary_fingerprint,
                "startupTime": identity.startup_time,
                "workspaceHash": identity.workspace_hash,
                "transportMode": identity.transport_mode,
                "connectionFingerprint": identity.connection_fingerprint,
            }
        })
    }

    pub fn record_turn_usage(&mut self, input_tokens: u64, output_tokens: u64) {
        self.usage_totals.accumulate(input_tokens, output_tokens, 1);
        self.session_usage_totals
            .accumulate(input_tokens, output_tokens, 1);
    }

    pub fn apply_server_ui_event(&mut self, event: ServerUiEvent) {
        match event {
            ServerUiEvent::IncrementRequestCount => {
                self.request_count = self.request_count.saturating_add(1);
            }
            ServerUiEvent::SetRemoteConnected(connected) => {
                self.remote_connected = connected;
                if connected {
                    self.last_remote_activity_ms = Some(now_unix_millis());
                } else {
                    self.last_remote_activity_ms = None;
                }
            }
            ServerUiEvent::RecordFlow {
                flow_id,
                events,
                direction,
            } => {
                self.record_flow(&flow_id, &events, direction);
            }
            ServerUiEvent::BeginFlowClose { flow_id } => {
                self.begin_flow_close(&flow_id);
            }
            ServerUiEvent::Log { level, message } => {
                self.log(level, message);
            }
        }
    }
}

impl AppState {
    pub fn record_flow(&mut self, flow_id: &str, events: &[String], direction: FlowDirection) {
        if events.is_empty() {
            return;
        }
        let now_ms = now_unix_millis();
        self.last_remote_activity_ms = Some(now_ms);
        self.remote_connected = true;
        let step_ms = derive_flow_step_ms();
        let mut bootstrap = self
            .flow_bootstrap_progress
            .get(flow_id)
            .cloned()
            .unwrap_or_default();
        let starts_bootstrap_status = events_start_bootstrap_status(events);
        let only_bootstrap_status_events = events_are_bootstrap_status_events(events);

        if let Some(idx) = self.flows.iter().position(|flow| flow.flow_id == flow_id) {
            let mut flow = self.flows.remove(idx);
            if starts_bootstrap_status {
                flow.bootstrap_status_active = true;
            } else if flow.bootstrap_status_active && !only_bootstrap_status_events {
                flow.bootstrap_status_active = false;
                flow.bootstrap_status_close_deadline_ms = None;
            }
            flow.events.extend(events.iter().cloned());
            if flow.events.len() > 12 {
                let drop_n = flow.events.len() - 12;
                flow.events.drain(0..drop_n);
            }
            flow.bootstrap_completed_steps = bootstrap.completed_steps;
            flow.bootstrap_pending_steps = bootstrap.pending_steps.clone();
            advance_bootstrap_progress(
                &mut flow.bootstrap_completed_steps,
                &mut flow.bootstrap_pending_steps,
                events,
                direction,
            );
            bootstrap.completed_steps = flow.bootstrap_completed_steps;
            bootstrap.pending_steps = flow.bootstrap_pending_steps.clone();
            self.flow_bootstrap_progress
                .insert(flow_id.to_string(), bootstrap);
            flow.closing_started_ms = None;
            flow.closing_step_ms = 0;
            flow.bootstrap_status_close_deadline_ms = None;
            flow.last_direction = direction;
            enqueue_flow_segment(&mut flow.anim_queue, direction, now_ms, step_ms);
            self.flows.insert(0, flow);
            return;
        }

        let mut trimmed = events.to_vec();
        if trimmed.len() > 12 {
            trimmed = trimmed[trimmed.len() - 12..].to_vec();
        }
        self.flows.insert(
            0,
            FlowLane {
                flow_id: flow_id.to_string(),
                short_id: short_flow_id(flow_id),
                events: trimmed,
                bootstrap_status_active: starts_bootstrap_status,
                bootstrap_completed_steps: bootstrap.completed_steps,
                bootstrap_pending_steps: bootstrap.pending_steps.clone(),
                bootstrap_status_close_deadline_ms: None,
                anim_queue: VecDeque::new(),
                last_direction: direction,
                closing_started_ms: None,
                closing_step_ms: 0,
            },
        );
        if let Some(flow) = self.flows.first_mut() {
            advance_bootstrap_progress(
                &mut flow.bootstrap_completed_steps,
                &mut flow.bootstrap_pending_steps,
                events,
                direction,
            );
            bootstrap.completed_steps = flow.bootstrap_completed_steps;
            bootstrap.pending_steps = flow.bootstrap_pending_steps.clone();
            self.flow_bootstrap_progress
                .insert(flow_id.to_string(), bootstrap);
            enqueue_flow_segment(&mut flow.anim_queue, direction, now_ms, step_ms);
        }
    }

    pub fn begin_flow_close(&mut self, flow_id: &str) {
        let now_ms = now_unix_millis();
        self.flow_bootstrap_progress.remove(flow_id);
        if let Some(flow) = self.flows.iter_mut().find(|flow| flow.flow_id == flow_id) {
            if flow.closing_started_ms.is_none() {
                flow.closing_started_ms = Some(now_ms);
                flow.closing_step_ms = flow
                    .anim_queue
                    .back()
                    .map(|seg| seg.step_ms.max(1))
                    .unwrap_or_else(derive_flow_step_ms);
                flow.anim_queue.clear();
                flow.bootstrap_status_active = false;
                flow.bootstrap_status_close_deadline_ms = None;
            }
        }
    }

    pub fn prune_closed_flows(&mut self) {
        let now_ms = now_unix_millis();
        let bootstrap_steps_total = flow_bootstrap_steps_total();

        for flow in &mut self.flows {
            prune_finished_segments(&mut flow.anim_queue, now_ms);
            if !flow.bootstrap_status_active {
                flow.bootstrap_status_close_deadline_ms = None;
                continue;
            }
            let bootstrap_complete = flow.bootstrap_completed_steps >= bootstrap_steps_total
                && flow.bootstrap_pending_steps.is_empty();
            if flow.closing_started_ms.is_none() && bootstrap_complete {
                if flow.anim_queue.is_empty() {
                    match flow.bootstrap_status_close_deadline_ms {
                        Some(deadline) if now_ms >= deadline => {
                            flow.bootstrap_status_active = false;
                            flow.bootstrap_status_close_deadline_ms = None;
                        }
                        Some(_) => {}
                        None => {
                            flow.bootstrap_status_close_deadline_ms =
                                Some(now_ms + FLOW_BOOTSTRAP_STATUS_CLOSE_DELAY_MS);
                        }
                    }
                } else {
                    flow.bootstrap_status_close_deadline_ms = None;
                }
            } else {
                flow.bootstrap_status_close_deadline_ms = None;
            }
        }
        self.flows.retain(|flow| {
            let Some(closing_started_ms) = flow.closing_started_ms else {
                return true;
            };
            let step_ms = flow.closing_step_ms.max(1) as u128;
            let ttl_ms = (FLOW_LINK_CELLS * FLOW_CLOSE_PRUNE_MULTIPLIER) as u128 * step_ms;
            now_ms.saturating_sub(closing_started_ms) < ttl_ms
        });
    }
}

fn generate_mcp_slug() -> String {
    let random = Uuid::new_v4();
    URL_SAFE_NO_PAD.encode(&random.as_bytes()[..12])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    #[cfg(windows)]
    fn acl_warning_test_guard() -> MutexGuard<'static, ()> {
        static ACL_WARNING_TEST_LOCK: Mutex<()> = Mutex::new(());
        let guard = ACL_WARNING_TEST_LOCK.lock().expect("acl warning test lock");
        reset_presentation_warning_dedupe_for_test();
        guard
    }

    fn test_app(name: &str) -> (AppState, PathBuf, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("{name}-{unique}"));
        std::fs::create_dir_all(&workspace).expect("create temp workspace");
        let config_path = workspace.join(APP_CONFIG_FILE_NAME);
        let app = AppState::from_config_path(
            8787,
            workspace.to_string_lossy().into_owned(),
            config_path.clone(),
        )
        .expect("create app state");
        (app, workspace, config_path)
    }

    fn temp_config_workspace(name: &str) -> (PathBuf, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("{name}-{unique}"));
        std::fs::create_dir_all(&workspace).expect("create temp workspace");
        let config_path = workspace.join(APP_CONFIG_FILE_NAME);
        (workspace, config_path)
    }

    fn temp_config_files(workspace: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(workspace)
            .expect("read temp workspace")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains(".tmp"))
            })
            .collect()
    }

    #[test]
    fn app_state_loads_persisted_config_file() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("catdesk-config-load-{unique}"));
        std::fs::create_dir_all(&workspace).expect("create temp workspace");
        let config_path = workspace.join(APP_CONFIG_FILE_NAME);
        std::fs::write(
            &config_path,
            r#"theme = "neon"
mode = "browser"
toolMode = "multiTools"

[usageTotals]
inputTokens = 120
outputTokens = 34
totalTokens = 154
toolCallCount = 7
"#,
        )
        .expect("write config file");

        let app = AppState::from_config_path(
            8787,
            workspace.to_string_lossy().into_owned(),
            config_path.clone(),
        )
        .expect("load app state");

        assert_eq!(app.theme, "neon");
        assert!(matches!(app.mode, Mode::Browser));
        assert!(matches!(app.tool_mode, ToolMode::MultiTools));
        assert_eq!(app.usage_totals.input_tokens, 120);
        assert_eq!(app.usage_totals.output_tokens, 34);
        assert_eq!(app.usage_totals.total_tokens, 154);
        assert_eq!(app.usage_totals.tool_call_count, 7);
        assert_eq!(app.session_usage_totals, UsageTotals::default());
        assert!(matches!(
            app.tunnel_config.mode,
            crate::tunnel::TunnelMode::ManagedEphemeralNgrok
        ));
        assert!(!app.installation_id.is_empty());

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn persist_state_writes_single_config_file() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("catdesk-config-save-{unique}"));
        std::fs::create_dir_all(&workspace).expect("create temp workspace");
        let config_path = workspace.join(APP_CONFIG_FILE_NAME);

        let mut app = AppState::from_config_path(
            8787,
            workspace.to_string_lossy().into_owned(),
            config_path.clone(),
        )
        .expect("create app state");
        app.theme = "neon".into();
        app.mode = Mode::Computer;
        app.tool_mode = ToolMode::ReadOnly;
        app.usage_totals.accumulate(12, 8, 3);
        app.session_usage_totals.accumulate(100, 200, 1);
        app.persist_state().expect("persist state");

        let saved = AppConfig::load_from_path(&config_path).expect("load config file");
        assert_eq!(saved.theme, "neon");
        assert!(matches!(saved.mode, Mode::Computer));
        assert!(matches!(saved.tool_mode, ToolMode::ReadOnly));
        assert_eq!(saved.usage_totals.input_tokens, 12);
        assert_eq!(saved.usage_totals.output_tokens, 8);
        assert_eq!(saved.usage_totals.total_tokens, 20);
        assert_eq!(saved.usage_totals.tool_call_count, 3);

        let reloaded = AppState::from_config_path(
            8787,
            workspace.to_string_lossy().into_owned(),
            config_path.clone(),
        )
        .expect("reload app state");
        assert_eq!(reloaded.usage_totals.total_tokens, 20);
        assert_eq!(reloaded.session_usage_totals, UsageTotals::default());

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn app_config_round_trips_ngrok_authtoken() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("catdesk-config-token-{unique}"));
        std::fs::create_dir_all(&workspace).expect("create temp config dir");
        let config_path = workspace.join(APP_CONFIG_FILE_NAME);

        let config = AppConfig {
            ngrok_authtoken: Some("test-token-123".into()),
            ..AppConfig::default()
        };
        config.save_to_path(&config_path).expect("save config");

        let saved = AppConfig::load_from_path(&config_path).expect("load config");
        assert_eq!(saved.ngrok_authtoken.as_deref(), Some("test-token-123"));

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn app_config_round_trips_agents_path_mode() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("catdesk-config-agents-mode-{unique}"));
        std::fs::create_dir_all(&workspace).expect("create temp config dir");
        let config_path = workspace.join(APP_CONFIG_FILE_NAME);

        let config = AppConfig {
            agents_path_mode: AgentsPathMode::Codex,
            ..AppConfig::default()
        };
        config.save_to_path(&config_path).expect("save config");

        let saved = AppConfig::load_from_path(&config_path).expect("load config");
        assert!(matches!(saved.agents_path_mode, AgentsPathMode::Codex));

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn app_config_round_trips_token_stats_layout() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("catdesk-config-token-layout-{unique}"));
        std::fs::create_dir_all(&workspace).expect("create temp config dir");
        let config_path = workspace.join(APP_CONFIG_FILE_NAME);

        let config = AppConfig {
            token_stats_layout: TokenStatsLayout::Bottom,
            ..AppConfig::default()
        };
        config.save_to_path(&config_path).expect("save config");

        let saved = AppConfig::load_from_path(&config_path).expect("load config");
        assert!(matches!(saved.token_stats_layout, TokenStatsLayout::Bottom));

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn app_config_round_trips_show_detail_mode() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("catdesk-config-show-detail-{unique}"));
        std::fs::create_dir_all(&workspace).expect("create temp config dir");
        let config_path = workspace.join(APP_CONFIG_FILE_NAME);

        let config = AppConfig {
            show_detail_mode: ShowDetailMode::Collapsed,
            ..AppConfig::default()
        };
        config.save_to_path(&config_path).expect("save config");

        let saved = AppConfig::load_from_path(&config_path).expect("load config");
        assert!(matches!(saved.show_detail_mode, ShowDetailMode::Collapsed));

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn transport_config_generates_persistent_route_once_and_installation_survives_reload() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("catdesk-transport-route-{unique}"));
        std::fs::create_dir_all(&workspace).expect("create temp config dir");
        let config_path = workspace.join(APP_CONFIG_FILE_NAME);
        std::fs::write(
            &config_path,
            r#"
theme = "concise"
mode = "computer"
toolMode = "multiTools"

[tunnel]
mode = "managed_stable_ngrok"
ngrok_domain = "example.ngrok-free.app"

[usageTotals]
inputTokens = 0
outputTokens = 0
totalTokens = 0
toolCallCount = 0
"#,
        )
        .expect("write stable config");

        let first = AppConfig::load_or_initialize_from_path(&config_path)
            .expect("load and initialize route");
        let first_route = first.mcp.route_id.clone().expect("route");
        let first_installation = first
            .identity
            .installation_id
            .clone()
            .expect("installation");
        crate::tunnel::validate_route_id(&first_route).expect("valid route");

        let second = AppConfig::load_or_initialize_from_path(&config_path)
            .expect("reload initialized route");
        assert_eq!(second.mcp.route_id.as_deref(), Some(first_route.as_str()));
        assert_eq!(
            second.identity.installation_id.as_deref(),
            Some(first_installation.as_str())
        );

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn external_tunnel_mode_loads_with_persistent_route_without_process_ownership() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("catdesk-transport-external-{unique}"));
        std::fs::create_dir_all(&workspace).expect("create temp config dir");
        let config_path = workspace.join(APP_CONFIG_FILE_NAME);
        std::fs::write(
            &config_path,
            r#"
theme = "concise"
mode = "computer"
toolMode = "multiTools"

[tunnel]
mode = "external_tunnel"
public_base_url = "https://example.invalid"
manage_process = false

[usageTotals]
inputTokens = 0
outputTokens = 0
totalTokens = 0
toolCallCount = 0
"#,
        )
        .expect("write external config");

        let app = AppState::from_config_path(
            8787,
            workspace.to_string_lossy().into_owned(),
            config_path.clone(),
        )
        .expect("external mode loads");

        let saved = AppConfig::load_from_path(&config_path).expect("load saved config");
        assert!(matches!(
            saved.tunnel.mode,
            crate::tunnel::TunnelMode::ExternalTunnel
        ));
        assert!(!saved.tunnel.manage_process);
        assert_eq!(saved.mcp.route_id.as_deref(), Some(app.mcp_slug.as_str()));
        assert!(app.ngrok_url.is_none());

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_file(workspace.join("config.toml.pre-t0025b-backup"));
        let _ = std::fs::remove_dir(workspace);
    }

    #[tokio::test]
    async fn external_tunnel_start_sets_public_identity_without_launching_ngrok() {
        let (workspace, config_path) = temp_config_workspace("catdesk-external-start");
        std::fs::write(
            &config_path,
            r#"
theme = "concise"
mode = "computer"
toolMode = "multiTools"

[tunnel]
mode = "external_tunnel"
public_base_url = "https://example.invalid"
manage_process = false

[usageTotals]
inputTokens = 0
outputTokens = 0
totalTokens = 0
toolCallCount = 0
"#,
        )
        .expect("write external config");
        let app = AppState::from_config_path(
            8787,
            workspace.to_string_lossy().into_owned(),
            config_path.clone(),
        )
        .expect("load external app");
        let route = app.mcp_slug.clone();
        let state = Arc::new(tokio::sync::Mutex::new(app));

        crate::ngrok::start_transport(state.clone())
            .await
            .expect("external transport starts");

        let app = state.lock().await;
        assert!(!app.ngrok_running);
        assert_eq!(app.ngrok_url.as_deref(), Some("https://example.invalid"));
        assert!(
            app.transport_identity
                .last_connection_fingerprint
                .as_deref()
                .is_some_and(|value| !value.is_empty())
        );
        let full_route = format!("https://example.invalid/{route}/mcp");
        let log_text = app
            .logs
            .iter()
            .map(|entry| entry.message.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!log_text.contains(&full_route));
        assert!(log_text.contains("External tunnel mode active"));
        assert!(log_text.contains("PUBLIC DEVELOPMENT ENDPOINT"));
        drop(app);

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_file(workspace.join("config.toml.pre-t0025b-backup"));
        let _ = std::fs::remove_dir(workspace);
    }

    #[tokio::test]
    async fn managed_stable_ngrok_start_fails_without_ephemeral_fallback() {
        let (workspace, config_path) = temp_config_workspace("catdesk-stable-start-blocked");
        std::fs::write(
            &config_path,
            r#"
theme = "concise"
mode = "computer"
toolMode = "multiTools"

[tunnel]
mode = "managed_stable_ngrok"
ngrok_domain = "example.ngrok-free.app"

[usageTotals]
inputTokens = 0
outputTokens = 0
totalTokens = 0
toolCallCount = 0
"#,
        )
        .expect("write stable config");
        let app = AppState::from_config_path(
            8787,
            workspace.to_string_lossy().into_owned(),
            config_path.clone(),
        )
        .expect("load stable app");
        let state = Arc::new(tokio::sync::Mutex::new(app));

        let error = crate::ngrok::start_transport(state.clone())
            .await
            .expect_err("stable start is blocked until C0 passes");
        assert!(error.contains("T-0025C0"));
        assert!(error.contains("no ephemeral fallback"));

        let app = state.lock().await;
        assert!(!app.ngrok_running);
        assert!(app.ngrok_url.is_none());
        drop(app);

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_file(workspace.join("config.toml.pre-t0025b-backup"));
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn invalid_transport_config_is_rejected() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("catdesk-transport-invalid-{unique}"));
        std::fs::create_dir_all(&workspace).expect("create temp config dir");
        let config_path = workspace.join(APP_CONFIG_FILE_NAME);

        std::fs::write(
            &config_path,
            r#"
theme = "concise"
mode = "computer"
toolMode = "multiTools"

[mcp]
port = 22

[usageTotals]
inputTokens = 0
outputTokens = 0
totalTokens = 0
toolCallCount = 0
"#,
        )
        .expect("write invalid port config");
        assert!(AppConfig::load_from_path(&config_path).is_err());

        std::fs::write(
            &config_path,
            r#"
theme = "concise"
mode = "computer"
toolMode = "multiTools"

[mcp]
bind_host = "0.0.0.0"

[tunnel]
mode = "managed_stable_ngrok"
ngrok_domain = "example.ngrok-free.app"

[usageTotals]
inputTokens = 0
outputTokens = 0
totalTokens = 0
toolCallCount = 0
"#,
        )
        .expect("write invalid bind config");
        assert!(AppConfig::load_from_path(&config_path).is_err());

        std::fs::write(
            &config_path,
            r#"
theme = "concise"
mode = "computer"
toolMode = "multiTools"

[mcp]
route_id = "../bad-route-that-is-long-enough"

[usageTotals]
inputTokens = 0
outputTokens = 0
totalTokens = 0
toolCallCount = 0
"#,
        )
        .expect("write invalid route config");
        assert!(AppConfig::load_from_path(&config_path).is_err());

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn route_rotation_marks_restart_required() {
        let mut config = AppConfig::default();
        crate::tunnel::rotate_route_restart_required(&mut config.mcp);
        assert!(config.mcp.route_rotation.restart_required);
        assert!(config.mcp.route_rotation.pending_route_id.is_some());
    }

    #[test]
    fn interrupted_atomic_write_keeps_prior_valid_config() {
        let (workspace, config_path) = temp_config_workspace("catdesk-atomic-config");
        let original = AppConfig {
            theme: "concise".into(),
            ..AppConfig::default()
        };
        original.save_to_path(&config_path).expect("save original");

        let replacement = AppConfig {
            theme: "neon".into(),
            ..AppConfig::default()
        };
        let plan = AtomicWritePlan {
            tmp_path_suffix: ".forced-interrupt".into(),
            fail_after_tmp_created: None,
            fail_replace_with: None,
            fail_cleanup_with: None,
        };
        let error = replacement
            .save_to_path_with_plan(&config_path, &plan, || {
                Err(std::io::Error::other("forced interruption"))
            })
            .expect_err("interrupted save fails");
        assert!(error.to_string().contains("forced interruption"));

        let saved = AppConfig::load_from_path(&config_path).expect("load prior config");
        assert_eq!(saved.theme, "concise");
        assert!(temp_config_files(&workspace).is_empty());

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn temp_cleanup_preserves_error_before_replacement() {
        let (workspace, config_path) = temp_config_workspace("catdesk-atomic-before-replace");
        let original = AppConfig {
            theme: "concise".into(),
            ..AppConfig::default()
        };
        original.save_to_path(&config_path).expect("save original");
        let replacement = AppConfig {
            theme: "neon".into(),
            ..AppConfig::default()
        };
        let plan = AtomicWritePlan {
            tmp_path_suffix: ".before-replace".into(),
            fail_after_tmp_created: None,
            fail_replace_with: None,
            fail_cleanup_with: None,
        };

        let error = replacement
            .save_to_path_with_plan(&config_path, &plan, || {
                Err(std::io::Error::other("original before-replace error"))
            })
            .expect_err("save should fail");

        assert_eq!(error.to_string(), "original before-replace error");
        assert_eq!(
            AppConfig::load_from_path(&config_path)
                .expect("load original")
                .theme,
            "concise"
        );
        assert!(temp_config_files(&workspace).is_empty());
        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn temp_cleanup_preserves_error_after_temp_created() {
        let (workspace, config_path) = temp_config_workspace("catdesk-atomic-write-failure");
        let original = AppConfig {
            theme: "concise".into(),
            ..AppConfig::default()
        };
        original.save_to_path(&config_path).expect("save original");
        let replacement = AppConfig {
            theme: "neon".into(),
            ..AppConfig::default()
        };
        let plan = AtomicWritePlan {
            tmp_path_suffix: ".write-failure".into(),
            fail_after_tmp_created: Some("original write/sync error"),
            fail_replace_with: None,
            fail_cleanup_with: None,
        };

        let error = replacement
            .save_to_path_with_plan(&config_path, &plan, || Ok(()))
            .expect_err("save should fail");

        assert_eq!(error.to_string(), "original write/sync error");
        assert_eq!(
            AppConfig::load_from_path(&config_path)
                .expect("load original")
                .theme,
            "concise"
        );
        assert!(temp_config_files(&workspace).is_empty());
        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn successful_temp_cleanup_returns_original_error_unchanged() {
        let (workspace, config_path) = temp_config_workspace("catdesk-cleanup-original-error");
        let original = AppConfig {
            theme: "concise".into(),
            ..AppConfig::default()
        };
        original.save_to_path(&config_path).expect("save original");
        let replacement = AppConfig {
            ngrok_authtoken: Some("secret-token-that-must-not-leak".into()),
            theme: "neon".into(),
            ..AppConfig::default()
        };
        let plan = AtomicWritePlan {
            tmp_path_suffix: ".cleanup-success".into(),
            fail_after_tmp_created: Some("original save error"),
            fail_replace_with: None,
            fail_cleanup_with: None,
        };

        let error = replacement
            .save_to_path_with_plan(&config_path, &plan, || Ok(()))
            .expect_err("save should fail");

        assert_eq!(error.to_string(), "original save error");
        assert!(
            !error
                .to_string()
                .contains("secret-token-that-must-not-leak")
        );
        assert!(temp_config_files(&workspace).is_empty());
        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn failed_temp_cleanup_reports_original_and_cleanup_error_without_config_contents() {
        let (workspace, config_path) = temp_config_workspace("catdesk-cleanup-failure");
        let original = AppConfig {
            theme: "concise".into(),
            ..AppConfig::default()
        };
        original.save_to_path(&config_path).expect("save original");
        let replacement = AppConfig {
            ngrok_authtoken: Some("secret-token-that-must-not-leak".into()),
            theme: "neon".into(),
            ..AppConfig::default()
        };
        let plan = AtomicWritePlan {
            tmp_path_suffix: ".cleanup-failure".into(),
            fail_after_tmp_created: Some("original save error"),
            fail_replace_with: None,
            fail_cleanup_with: Some("cleanup remove error"),
        };

        let error = replacement
            .save_to_path_with_plan(&config_path, &plan, || Ok(()))
            .expect_err("save should fail");
        let message = error.to_string();

        assert!(message.contains("original save error"));
        assert!(message.contains("cleanup remove error"));
        assert!(message.contains("<redacted config temp>"));
        assert!(!message.contains("secret-token-that-must-not-leak"));
        assert!(!message.contains("ngrok_authtoken"));
        assert_eq!(
            AppConfig::load_from_path(&config_path)
                .expect("load original")
                .theme,
            "concise"
        );
        for tmp in temp_config_files(&workspace) {
            let _ = std::fs::remove_file(tmp);
        }
        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn temp_cleanup_preserves_error_on_replacement_failure() {
        let (workspace, config_path) = temp_config_workspace("catdesk-atomic-replace-failure");
        let original = AppConfig {
            theme: "concise".into(),
            ..AppConfig::default()
        };
        original.save_to_path(&config_path).expect("save original");
        let replacement = AppConfig {
            theme: "neon".into(),
            ..AppConfig::default()
        };
        let plan = AtomicWritePlan {
            tmp_path_suffix: ".replace-failure".into(),
            fail_after_tmp_created: None,
            fail_replace_with: Some("original replace error"),
            fail_cleanup_with: None,
        };

        let error = replacement
            .save_to_path_with_plan(&config_path, &plan, || Ok(()))
            .expect_err("save should fail");

        assert_eq!(error.to_string(), "original replace error");
        assert_eq!(
            AppConfig::load_from_path(&config_path)
                .expect("load original")
                .theme,
            "concise"
        );
        assert!(temp_config_files(&workspace).is_empty());
        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn existing_config_replacement_writes_new_config_without_temp_files() {
        let (workspace, config_path) = temp_config_workspace("catdesk-existing-replace");
        let original = AppConfig {
            theme: "concise".into(),
            ..AppConfig::default()
        };
        original.save_to_path(&config_path).expect("save original");
        let replacement = AppConfig {
            theme: "neon".into(),
            ..AppConfig::default()
        };

        replacement
            .save_to_path_with_plan(&config_path, &AtomicWritePlan::default(), || {
                assert!(config_path.exists());
                assert_eq!(
                    AppConfig::load_from_path(&config_path)
                        .expect("load original during replacement")
                        .theme,
                    "concise"
                );
                Ok(())
            })
            .expect("replace existing config");

        assert_eq!(
            AppConfig::load_from_path(&config_path)
                .expect("load replacement")
                .theme,
            "neon"
        );
        assert!(temp_config_files(&workspace).is_empty());
        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn ordinary_successful_save_remains_unchanged() {
        let (workspace, config_path) = temp_config_workspace("catdesk-ordinary-save");
        let config = AppConfig {
            ngrok_authtoken: Some("test-token-123".into()),
            theme: "neon".into(),
            ..AppConfig::default()
        };

        config.save_to_path(&config_path).expect("save config");
        let saved = AppConfig::load_from_path(&config_path).expect("load config");

        assert_eq!(saved.theme, "neon");
        assert_eq!(saved.ngrok_authtoken.as_deref(), Some("test-token-123"));
        assert!(temp_config_files(&workspace).is_empty());
        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn route_promotion_after_restart_persists_pending_route() {
        let (workspace, config_path) = temp_config_workspace("catdesk-route-promotion");
        std::fs::write(
            &config_path,
            r#"
theme = "concise"
mode = "computer"
toolMode = "multiTools"

[mcp]
route_id = "oldrouteoldrouteoldroute12"

[mcp.route_rotation]
pending_route_id = "newroutenewroutenewroute12"
restart_required = true

[tunnel]
mode = "managed_stable_ngrok"
ngrok_domain = "example.ngrok-free.app"

[usageTotals]
inputTokens = 0
outputTokens = 0
totalTokens = 0
toolCallCount = 0
"#,
        )
        .expect("write pending route config");

        let config =
            AppConfig::load_or_initialize_from_path(&config_path).expect("promote pending route");
        assert_eq!(
            config.mcp.route_id.as_deref(),
            Some("newroutenewroutenewroute12")
        );
        assert!(config.mcp.route_rotation.pending_route_id.is_none());
        assert!(!config.mcp.route_rotation.restart_required);

        let saved = AppConfig::load_from_path(&config_path).expect("reload promoted route");
        assert_eq!(
            saved.mcp.route_id.as_deref(),
            Some("newroutenewroutenewroute12")
        );
        assert!(
            !std::fs::read_to_string(&config_path)
                .expect("read promoted config")
                .contains("oldrouteoldrouteoldroute12")
        );
        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_file(workspace.join("config.toml.pre-t0025b-backup"));
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn migration_backup_created_once_before_automatic_rewrite() {
        let (workspace, config_path) = temp_config_workspace("catdesk-migration-backup");
        std::fs::write(
            &config_path,
            r#"theme = "concise"
mode = "computer"
toolMode = "multiTools"

[usageTotals]
inputTokens = 0
outputTokens = 0
totalTokens = 0
toolCallCount = 0
"#,
        )
        .expect("write pre-t0025b config");
        let backup_path = workspace.join("config.toml.pre-t0025b-backup");

        AppConfig::load_or_initialize_from_path(&config_path).expect("initialize config");
        assert!(backup_path.exists());
        let backup_text = std::fs::read_to_string(&backup_path).expect("read backup");
        assert!(!backup_text.contains("installation_id"));
        let saved = std::fs::read_to_string(&config_path).expect("read migrated config");
        assert!(saved.contains("installation_id"));

        std::fs::write(&backup_path, "sentinel").expect("mark backup");
        AppConfig::load_or_initialize_from_path(&config_path).expect("reload migrated config");
        assert_eq!(
            std::fs::read_to_string(&backup_path).expect("read backup"),
            "sentinel"
        );

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_file(backup_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[cfg(windows)]
    #[test]
    fn initial_migration_makes_deferred_acl_warning_observable() {
        let _guard = acl_warning_test_guard();
        let (workspace, config_path) = temp_config_workspace("catdesk-acl-warning-observable");
        std::fs::write(
            &config_path,
            r#"theme = "concise"
mode = "computer"
toolMode = "multiTools"

[usageTotals]
inputTokens = 0
outputTokens = 0
totalTokens = 0
toolCallCount = 0
"#,
        )
        .expect("write pre-t0025b config");

        let app = AppState::from_config_path(
            8787,
            workspace.to_string_lossy().into_owned(),
            config_path.clone(),
        )
        .expect("migrate config");

        let acl_warnings: Vec<_> = app
            .logs
            .iter()
            .filter(|entry| entry.message == DEFERRED_ACL_WARNING)
            .collect();
        assert_eq!(acl_warnings.len(), 1);
        assert!(!acl_warnings[0].message.contains("config.toml"));
        assert!(!acl_warnings[0].message.contains("ngrok"));
        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_file(workspace.join("config.toml.pre-t0025b-backup"));
        let _ = std::fs::remove_dir(workspace);
    }

    #[cfg(windows)]
    #[test]
    fn repeated_persistence_does_not_repeat_deferred_acl_warning() {
        let _guard = acl_warning_test_guard();
        let (workspace, config_path) = temp_config_workspace("catdesk-acl-warning-dedupe");
        std::fs::write(
            &config_path,
            r#"theme = "concise"
mode = "computer"
toolMode = "multiTools"

[usageTotals]
inputTokens = 0
outputTokens = 0
totalTokens = 0
toolCallCount = 0
"#,
        )
        .expect("write pre-t0025b config");

        let mut app = AppState::from_config_path(
            8787,
            workspace.to_string_lossy().into_owned(),
            config_path.clone(),
        )
        .expect("migrate config");
        app.persist_state_with_log();
        app.persist_state_with_log();

        let acl_warning_count = app
            .logs
            .iter()
            .filter(|entry| entry.message == DEFERRED_ACL_WARNING)
            .count();
        assert_eq!(acl_warning_count, 1);
        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_file(workspace.join("config.toml.pre-t0025b-backup"));
        let _ = std::fs::remove_dir(workspace);
    }

    #[cfg(windows)]
    #[test]
    fn backup_creation_cannot_consume_only_deferred_acl_warning() {
        let _guard = acl_warning_test_guard();
        let (workspace, config_path) = temp_config_workspace("catdesk-acl-warning-backup");
        std::fs::write(
            &config_path,
            r#"theme = "concise"
mode = "computer"
toolMode = "multiTools"

[usageTotals]
inputTokens = 0
outputTokens = 0
totalTokens = 0
toolCallCount = 0
"#,
        )
        .expect("write pre-t0025b config");

        let app = AppState::from_config_path(
            8787,
            workspace.to_string_lossy().into_owned(),
            config_path.clone(),
        )
        .expect("migrate config");

        assert!(workspace.join("config.toml.pre-t0025b-backup").exists());
        assert_eq!(
            app.logs
                .iter()
                .filter(|entry| entry.message == DEFERRED_ACL_WARNING)
                .count(),
            1
        );
        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_file(workspace.join("config.toml.pre-t0025b-backup"));
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn server_instance_id_changes_between_process_state_instances() {
        let (first, workspace, config_path) = test_app("catdesk-identity-first");
        let second = AppState::from_config_path(
            8787,
            workspace.to_string_lossy().into_owned(),
            config_path.clone(),
        )
        .expect("reload app state");
        assert_eq!(first.installation_id, second.installation_id);
        assert_ne!(first.server_instance_id, second.server_instance_id);

        let snapshot = second.transport_identity_snapshot();
        assert_eq!(snapshot.installation_id, second.installation_id);
        assert_eq!(snapshot.server_instance_id, second.server_instance_id);
        assert_eq!(snapshot.transport_mode, "managed_ephemeral_ngrok");
        assert!(!snapshot.workspace_hash.is_empty());

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn app_state_loads_partner_binagotchy_seed() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("catdesk-config-partner-{unique}"));
        std::fs::create_dir_all(&workspace).expect("create temp workspace");
        let config_path = workspace.join(APP_CONFIG_FILE_NAME);

        std::fs::write(
            &config_path,
            r#"
theme = "concise"
mode = "both"
toolMode = "multiTools"
partnerBinagotchySeed = "00000000000000ff"

[usageTotals]
inputTokens = 0
outputTokens = 0
totalTokens = 0
toolCallCount = 0
"#,
        )
        .expect("write config file");

        let app = AppState::from_config_path(
            8787,
            workspace.to_string_lossy().into_owned(),
            config_path.clone(),
        )
        .expect("load app state");

        assert_eq!(
            app.partner_binagotchy_seed.as_deref(),
            Some("00000000000000ff")
        );
        assert_eq!(app.mascot_seed, 0xff);

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(workspace);
    }

    #[test]
    fn flow_anim_lit_count_interpolates_between_endpoints() {
        let duration_ms = move_segment_duration_ms(
            FlowDirection::Forward,
            derive_flow_step_ms(),
            0,
            FLOW_ANIM_CELLS,
        );
        let seg = FlowAnimSegment {
            kind: FlowAnimKind::Move,
            direction: FlowDirection::Forward,
            started_ms: 100,
            ends_ms: 100 + duration_ms,
            step_ms: derive_flow_step_ms(),
            start_cells: 0,
            end_cells: FLOW_ANIM_CELLS,
        };

        assert_eq!(flow_anim_lit_count(seg, 100), 0);
        assert!(flow_anim_lit_count(seg, 100 + duration_ms / 2) > 0);
        assert!(flow_anim_lit_count(seg, 100 + duration_ms / 2) < FLOW_ANIM_CELLS);
        assert_eq!(flow_anim_lit_count(seg, 100 + duration_ms), FLOW_ANIM_CELLS);
    }

    #[test]
    fn backward_move_uses_longer_duration() {
        let forward = move_segment_duration_ms(
            FlowDirection::Forward,
            derive_flow_step_ms(),
            0,
            FLOW_ANIM_CELLS,
        );
        let backward = move_segment_duration_ms(
            FlowDirection::Backward,
            derive_flow_step_ms(),
            0,
            FLOW_ANIM_CELLS,
        );

        assert_eq!(forward, FLOW_FORWARD_ANIMATION_DURATION_MS as u128);
        assert_eq!(backward, FLOW_BACKWARD_ANIMATION_DURATION_MS as u128);
    }

    #[test]
    fn enqueue_flow_segment_preempts_inflight_move() {
        let mut queue = VecDeque::new();
        let step_ms = derive_flow_step_ms();
        enqueue_flow_segment(&mut queue, FlowDirection::Forward, 0, step_ms);
        assert_eq!(queue.len(), 1);

        enqueue_flow_segment(&mut queue, FlowDirection::Backward, 40, step_ms);
        assert_eq!(queue.len(), 2);
        assert!(matches!(queue[0].kind, FlowAnimKind::Turn));
        assert!(queue[0].direction == FlowDirection::Forward);
        assert!(queue[0].start_cells > 0);
        assert_eq!(queue[0].end_cells, 0);
        assert!(matches!(queue[1].kind, FlowAnimKind::Move));
        assert!(queue[1].direction == FlowDirection::Backward);
        assert_eq!(queue[1].start_cells, 0);
        assert_eq!(queue[1].end_cells, FLOW_ANIM_CELLS);
    }

    #[test]
    fn record_flow_tool_call_does_not_activate_bootstrap_status() {
        let (mut app, workspace, config_path) = test_app("catdesk-flow-tool-call");

        app.record_flow(
            "stateless",
            &["tools/call:run_command".to_string()],
            FlowDirection::Forward,
        );

        let flow = app.flows.first().expect("missing flow");
        assert!(!flow.bootstrap_status_active);
        assert_eq!(flow.bootstrap_completed_steps, 0);
        assert!(flow.bootstrap_pending_steps.is_empty());

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn record_flow_initialize_activates_bootstrap_status() {
        let (mut app, workspace, config_path) = test_app("catdesk-flow-initialize");

        app.record_flow(
            "stateless",
            &["initialize".to_string()],
            FlowDirection::Forward,
        );

        let flow = app.flows.first().expect("missing flow");
        assert!(flow.bootstrap_status_active);
        assert_eq!(flow.bootstrap_completed_steps, 0);
        assert_eq!(flow.bootstrap_pending_steps.front(), Some(&0));

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn record_flow_bootstrap_event_keeps_bootstrap_status_active() {
        let (mut app, workspace, config_path) = test_app("catdesk-flow-bootstrap-event");

        app.record_flow(
            "stateless",
            &["initialize".to_string()],
            FlowDirection::Forward,
        );
        app.record_flow(
            "stateless",
            &["tools/list".to_string()],
            FlowDirection::Forward,
        );

        let flow = app.flows.first().expect("missing flow");
        assert!(flow.bootstrap_status_active);

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn record_flow_bootstrap_keeps_five_phases_and_expands_widget_reads() {
        let (mut app, workspace, config_path) = test_app("catdesk-flow-bootstrap-widgets");

        let sequence = [
            // Phase 1: Checking tools
            ("initialize", FlowDirection::Forward),
            ("initialize", FlowDirection::Backward),
            ("initialize", FlowDirection::Forward),
            ("initialize", FlowDirection::Backward),
            ("notifications/initialized", FlowDirection::Forward),
            ("tools/list", FlowDirection::Forward),
            ("tools/list", FlowDirection::Backward),
            // Phase 2: Checking resources
            ("initialize", FlowDirection::Forward),
            ("initialize", FlowDirection::Backward),
            ("initialize", FlowDirection::Forward),
            ("initialize", FlowDirection::Backward),
            ("notifications/initialized", FlowDirection::Forward),
            ("resources/list", FlowDirection::Forward),
            ("resources/list", FlowDirection::Backward),
            // Phase 3: Loading widgets
            ("initialize", FlowDirection::Forward),
            ("initialize", FlowDirection::Backward),
            ("initialize", FlowDirection::Forward),
            ("initialize", FlowDirection::Backward),
            ("notifications/initialized", FlowDirection::Forward),
            ("resources/read:run_command", FlowDirection::Forward),
            ("resources/read:run_command", FlowDirection::Backward),
            ("resources/read:catdesk_instruction", FlowDirection::Forward),
            (
                "resources/read:catdesk_instruction",
                FlowDirection::Backward,
            ),
            ("resources/read:read", FlowDirection::Forward),
            ("resources/read:read", FlowDirection::Backward),
            ("resources/read:search", FlowDirection::Forward),
            ("resources/read:search", FlowDirection::Backward),
            ("resources/read:write", FlowDirection::Forward),
            ("resources/read:write", FlowDirection::Backward),
            ("resources/read:edit", FlowDirection::Forward),
            ("resources/read:edit", FlowDirection::Backward),
            ("resources/read:delete", FlowDirection::Forward),
            ("resources/read:delete", FlowDirection::Backward),
            // Phase 4: Refreshing widgets
            ("initialize", FlowDirection::Forward),
            ("initialize", FlowDirection::Backward),
            ("initialize", FlowDirection::Forward),
            ("initialize", FlowDirection::Backward),
            ("notifications/initialized", FlowDirection::Forward),
            ("tools/list", FlowDirection::Forward),
            ("tools/list", FlowDirection::Backward),
            ("resources/read:run_command", FlowDirection::Forward),
            ("resources/read:run_command", FlowDirection::Backward),
            ("resources/read:catdesk_instruction", FlowDirection::Forward),
            (
                "resources/read:catdesk_instruction",
                FlowDirection::Backward,
            ),
            ("resources/read:read", FlowDirection::Forward),
            ("resources/read:read", FlowDirection::Backward),
            ("resources/read:search", FlowDirection::Forward),
            ("resources/read:search", FlowDirection::Backward),
            ("resources/read:write", FlowDirection::Forward),
            ("resources/read:write", FlowDirection::Backward),
            ("resources/read:edit", FlowDirection::Forward),
            ("resources/read:edit", FlowDirection::Backward),
            ("resources/read:delete", FlowDirection::Forward),
            ("resources/read:delete", FlowDirection::Backward),
            // Phase 5: Final resource check
            ("initialize", FlowDirection::Forward),
            ("initialize", FlowDirection::Backward),
            ("initialize", FlowDirection::Forward),
            ("initialize", FlowDirection::Backward),
            ("notifications/initialized", FlowDirection::Forward),
            ("resources/list", FlowDirection::Forward),
            ("resources/list", FlowDirection::Backward),
        ];

        for (event, direction) in sequence {
            app.record_flow("stateless", &[event.to_string()], direction);
        }

        let flow = app.flows.first().expect("missing flow");
        assert!(flow.bootstrap_status_active);
        let phase_step_counts: Vec<usize> = FLOW_BOOTSTRAP_PHASES
            .iter()
            .map(|phase| phase.steps.len())
            .collect();
        assert_eq!(phase_step_counts, vec![4, 4, 10, 11, 4]);
        assert_eq!(flow_bootstrap_steps_total(), 33);
        assert_eq!(flow.bootstrap_completed_steps, flow_bootstrap_steps_total());
        assert!(flow.bootstrap_pending_steps.is_empty());

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn record_flow_tool_call_after_initialize_deactivates_bootstrap_status() {
        let (mut app, workspace, config_path) = test_app("catdesk-flow-tool-after-initialize");

        app.record_flow(
            "stateless",
            &["initialize".to_string()],
            FlowDirection::Forward,
        );
        app.record_flow(
            "stateless",
            &["tools/call:catdesk_instruction".to_string()],
            FlowDirection::Forward,
        );

        let flow = app.flows.first().expect("missing flow");
        assert!(!flow.bootstrap_status_active);
        assert!(flow.bootstrap_status_close_deadline_ms.is_none());

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn record_flow_tool_call_after_close_does_not_reactivate_bootstrap_status() {
        let (mut app, workspace, config_path) = test_app("catdesk-flow-after-close");

        app.record_flow(
            "stateless",
            &["initialize".to_string()],
            FlowDirection::Forward,
        );
        app.begin_flow_close("stateless");
        app.record_flow(
            "stateless",
            &["tools/call:run_command".to_string()],
            FlowDirection::Forward,
        );

        let flow = app.flows.first().expect("missing flow");
        assert!(!flow.bootstrap_status_active);

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir_all(workspace);
    }
}
