#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

const DEFAULT_PROFILE_NAME: &str = "catdesk-local";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
pub const OPENAI_TUNNEL_READINESS_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CHILD_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 128 * 1024 * 1024;
const OFFICIAL_LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/openai/tunnel-client/releases/latest";
const OFFICIAL_RELEASES_PAGE: &str = "https://github.com/openai/tunnel-client/releases/latest";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenaiTunnelProcessMode {
    #[default]
    External,
    Managed,
}

impl OpenaiTunnelProcessMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::External => "external",
            Self::Managed => "managed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OpenaiTunnelConfig {
    #[serde(default)]
    pub client_path: Option<String>,
    #[serde(default = "default_profile_name")]
    pub profile_name: String,
    #[serde(default)]
    pub process_mode: OpenaiTunnelProcessMode,
    #[serde(default)]
    pub admin_ui_url: Option<String>,
}

impl Default for OpenaiTunnelConfig {
    fn default() -> Self {
        Self {
            client_path: None,
            profile_name: default_profile_name(),
            process_mode: OpenaiTunnelProcessMode::External,
            admin_ui_url: None,
        }
    }
}

impl OpenaiTunnelConfig {
    pub fn normalized(mut self) -> Self {
        self.client_path = normalize_optional_string(self.client_path.take());
        self.profile_name = self.profile_name.trim().to_string();
        if self.profile_name.is_empty() {
            self.profile_name = default_profile_name();
        }
        self.admin_ui_url = normalize_optional_string(self.admin_ui_url.take());
        self
    }
}

#[derive(Clone, Debug)]
pub struct TunnelClientDiscoveryOptions {
    pub explicit_path: Option<PathBuf>,
    pub path_var: Option<OsString>,
    pub user_tools_dir: PathBuf,
    pub known_paths: Vec<PathBuf>,
    pub forbidden_roots: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelClientMetadata {
    pub path: PathBuf,
    pub source: String,
    pub version_output: String,
    pub supports_http_mcp: bool,
    pub supports_doctor: bool,
    pub supports_admin_ui: bool,
    pub supported_profile_operations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelClientReleaseAsset {
    pub name: String,
    pub download_url: String,
    pub size: u64,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelClientReleasePlan {
    pub release_tag: String,
    pub release_url: String,
    pub windows_asset: TunnelClientReleaseAsset,
    pub checksum_available: bool,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TunnelClientInstallOutcome {
    pub installed_path: PathBuf,
    pub previous_backup_path: Option<PathBuf>,
    pub checksum_verified: bool,
}

pub struct ManagedTunnelProcess {
    child: Child,
    stdout_task: Option<JoinHandle<String>>,
    stderr_task: Option<JoinHandle<String>>,
}

impl ManagedTunnelProcess {
    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    pub fn start_kill(&mut self) -> std::io::Result<()> {
        self.child.start_kill()
    }

    pub async fn wait_with_timeout(&mut self, timeout: Duration) -> std::io::Result<bool> {
        match tokio::time::timeout(timeout, self.child.wait()).await {
            Ok(result) => result.map(|_| true),
            Err(_) => Ok(false),
        }
    }

    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }

    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    pub async fn collect_output(&mut self) -> String {
        let mut output = String::new();
        if let Some(task) = self.stdout_task.take()
            && let Ok(text) = task.await
        {
            output.push_str(&text);
        }
        if let Some(task) = self.stderr_task.take()
            && let Ok(text) = task.await
        {
            output.push_str(&text);
        }
        redact_tunnel_output(&output)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenaiTunnelReadiness {
    pub ready: bool,
    pub health_live: bool,
    pub redacted_reason: Option<String>,
}

#[derive(Debug)]
pub enum OpenaiTunnelError {
    NotFound,
    InvalidPath(String),
    Unsupported(String),
    Io(String),
    Command(String),
    Network(String),
    Release(String),
    Checksum(String),
    Archive(String),
}

impl std::fmt::Display for OpenaiTunnelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "official tunnel-client was not found"),
            Self::InvalidPath(message) => write!(f, "invalid tunnel-client path: {message}"),
            Self::Unsupported(message) => write!(f, "unsupported tunnel-client: {message}"),
            Self::Io(message) => write!(f, "tunnel-client file error: {message}"),
            Self::Command(message) => write!(f, "tunnel-client command failed: {message}"),
            Self::Network(message) => write!(f, "tunnel-client network error: {message}"),
            Self::Release(message) => write!(f, "tunnel-client release error: {message}"),
            Self::Checksum(message) => write!(f, "tunnel-client checksum error: {message}"),
            Self::Archive(message) => write!(f, "tunnel-client archive error: {message}"),
        }
    }
}

impl std::error::Error for OpenaiTunnelError {}

pub fn default_profile_name() -> String {
    DEFAULT_PROFILE_NAME.to_string()
}

pub fn default_user_tools_dir(home: &Path) -> PathBuf {
    home.join(".catdesk").join("tools").join("tunnel-client")
}

pub fn official_latest_release_api_url() -> &'static str {
    OFFICIAL_LATEST_RELEASE_API
}

pub fn official_releases_page_url() -> &'static str {
    OFFICIAL_RELEASES_PAGE
}

pub fn find_tunnel_client_candidates(options: &TunnelClientDiscoveryOptions) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = options.explicit_path.as_ref() {
        candidates.push(path.clone());
    }
    if let Some(path_var) = options.path_var.as_ref() {
        for dir in std::env::split_paths(path_var) {
            for file_name in executable_names() {
                candidates.push(dir.join(file_name));
            }
        }
    }
    for file_name in executable_names() {
        candidates.push(options.user_tools_dir.join(file_name));
    }
    candidates.extend(options.known_paths.iter().cloned());
    dedupe_paths(candidates)
}

pub async fn discover_tunnel_client(
    options: &TunnelClientDiscoveryOptions,
) -> Result<TunnelClientMetadata, OpenaiTunnelError> {
    for candidate in find_tunnel_client_candidates(options) {
        match validate_client_candidate(&candidate, &options.forbidden_roots).await {
            Ok(metadata) => return Ok(metadata),
            Err(OpenaiTunnelError::InvalidPath(_)) | Err(OpenaiTunnelError::Unsupported(_)) => {
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    Err(OpenaiTunnelError::NotFound)
}

pub async fn validate_client_candidate(
    path: &Path,
    forbidden_roots: &[PathBuf],
) -> Result<TunnelClientMetadata, OpenaiTunnelError> {
    if !path.exists() || !path.is_file() {
        return Err(OpenaiTunnelError::InvalidPath("path is not a file".into()));
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !executable_names().iter().any(|name| *name == file_name) {
        return Err(OpenaiTunnelError::InvalidPath(
            "filename is not tunnel-client".into(),
        ));
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| OpenaiTunnelError::Io(redact_path_error(error)))?;
    for root in forbidden_roots {
        if let Ok(root) = std::fs::canonicalize(root)
            && canonical.starts_with(root)
        {
            return Err(OpenaiTunnelError::InvalidPath(
                "client must not be installed inside the controlled repository".into(),
            ));
        }
    }
    let version_output = run_client_command(path, ["--version"]).await?;
    if !version_output
        .to_ascii_lowercase()
        .contains("tunnel-client")
    {
        return Err(OpenaiTunnelError::Unsupported(
            "version output did not identify tunnel-client".into(),
        ));
    }
    let quickstart_output = run_client_command(path, ["help", "quickstart"]).await?;
    Ok(TunnelClientMetadata {
        path: canonical,
        source: "discovered".into(),
        version_output: bounded_line(&version_output),
        supports_http_mcp: quickstart_output.contains("--mcp-server-url")
            || quickstart_output.contains("mcp.server-url"),
        supports_doctor: quickstart_output.contains("doctor"),
        supports_admin_ui: quickstart_output.contains("/ui")
            || quickstart_output.to_ascii_lowercase().contains("admin ui"),
        supported_profile_operations: supported_profile_operations(&quickstart_output),
    })
}

pub async fn latest_official_release_plan(
    client: &reqwest::Client,
) -> Result<TunnelClientReleasePlan, OpenaiTunnelError> {
    let response = client
        .get(OFFICIAL_LATEST_RELEASE_API)
        .header("user-agent", "CatDesk tunnel-client discovery")
        .send()
        .await
        .map_err(|error| OpenaiTunnelError::Network(redact_network_error(&error.to_string())))?;
    if !response.status().is_success() {
        return Err(OpenaiTunnelError::Network(format!(
            "official release lookup returned HTTP {}",
            response.status().as_u16()
        )));
    }
    let value: Value = response
        .json()
        .await
        .map_err(|_| OpenaiTunnelError::Release("release metadata was not valid JSON".into()))?;
    release_plan_from_github_json(&value)
}

pub fn release_plan_from_github_json(
    value: &Value,
) -> Result<TunnelClientReleasePlan, OpenaiTunnelError> {
    if value
        .get("prerelease")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(OpenaiTunnelError::Release(
            "latest official release metadata pointed to a prerelease".into(),
        ));
    }
    let release_tag = value
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| OpenaiTunnelError::Release("release tag missing".into()))?
        .to_string();
    let release_url = value
        .get("html_url")
        .and_then(Value::as_str)
        .unwrap_or(OFFICIAL_RELEASES_PAGE)
        .to_string();
    let assets = value
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| OpenaiTunnelError::Release("release assets missing".into()))?;
    let windows_asset = select_windows_asset(assets)?;
    let checksum_available = windows_asset.sha256.is_some();
    let warning = if checksum_available {
        None
    } else {
        Some("Official release metadata did not include a checksum; require operator confirmation before install".into())
    };
    Ok(TunnelClientReleasePlan {
        release_tag,
        release_url,
        windows_asset,
        checksum_available,
        warning,
    })
}

pub fn select_windows_asset(
    assets: &[Value],
) -> Result<TunnelClientReleaseAsset, OpenaiTunnelError> {
    let required_arch = windows_arch_asset_token()?;
    for asset in assets {
        let name = asset
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let lower = name.to_ascii_lowercase();
        if lower.contains("windows")
            && lower.contains(required_arch)
            && (lower.ends_with(".zip") || lower.ends_with(".exe"))
        {
            let download_url = asset
                .get("browser_download_url")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    OpenaiTunnelError::Release("Windows asset download URL missing".into())
                })?
                .to_string();
            validate_official_download_url(&download_url)?;
            let size = asset
                .get("size")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            if size == 0 || size > MAX_DOWNLOAD_BYTES {
                return Err(OpenaiTunnelError::Release(
                    "Windows asset size was missing or outside CatDesk bounds".into(),
                ));
            }
            let sha256 = asset
                .get("digest")
                .and_then(Value::as_str)
                .and_then(parse_sha256_digest)
                .or_else(|| {
                    asset
                        .get("sha256")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            return Ok(TunnelClientReleaseAsset {
                name: name.to_string(),
                download_url,
                size,
                sha256,
            });
        }
    }
    Err(OpenaiTunnelError::Release(
        "release did not include a Windows tunnel-client artifact for this architecture".into(),
    ))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn verify_archive_checksum(bytes: &[u8], expected: &str) -> Result<(), OpenaiTunnelError> {
    let expected = expected.trim().to_ascii_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(OpenaiTunnelError::Checksum(
            "expected SHA-256 digest is malformed".into(),
        ));
    }
    let actual = sha256_hex(bytes);
    if actual != expected {
        return Err(OpenaiTunnelError::Checksum(
            "downloaded archive SHA-256 did not match official metadata".into(),
        ));
    }
    Ok(())
}

pub fn install_client_archive_bytes(
    archive_bytes: &[u8],
    expected_sha256: Option<&str>,
    install_dir: &Path,
) -> Result<TunnelClientInstallOutcome, OpenaiTunnelError> {
    if archive_bytes.is_empty() || archive_bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err(OpenaiTunnelError::Archive(
            "archive size was missing or outside CatDesk bounds".into(),
        ));
    }
    if let Some(expected) = expected_sha256 {
        verify_archive_checksum(archive_bytes, expected)?;
    } else {
        return Err(OpenaiTunnelError::Checksum(
            "official SHA-256 verification is required for unattended install".into(),
        ));
    }
    let parent = install_dir
        .parent()
        .ok_or_else(|| OpenaiTunnelError::Archive("install directory must have a parent".into()))?;
    std::fs::create_dir_all(parent).map_err(|error| OpenaiTunnelError::Io(error.to_string()))?;
    let staging = parent.join(format!(".tunnel-client-staging-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&staging).map_err(|error| OpenaiTunnelError::Io(error.to_string()))?;
    let extraction = extract_archive_to_dir(archive_bytes, &staging);
    if let Err(error) = extraction {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    let binary = find_extracted_client(&staging).ok_or_else(|| {
        let _ = std::fs::remove_dir_all(&staging);
        OpenaiTunnelError::Archive("archive did not contain tunnel-client executable".into())
    })?;
    std::fs::create_dir_all(install_dir)
        .map_err(|error| OpenaiTunnelError::Io(error.to_string()))?;
    let destination = install_dir.join(binary.file_name().unwrap_or_default());
    let previous_backup_path = if destination.exists() {
        let backup = install_dir.join(format!(
            "{}.previous",
            destination
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("tunnel-client")
        ));
        let _ = std::fs::remove_file(&backup);
        std::fs::rename(&destination, &backup)
            .map_err(|error| OpenaiTunnelError::Io(error.to_string()))?;
        Some(backup)
    } else {
        None
    };
    if let Err(error) = std::fs::rename(&binary, &destination) {
        if let Some(backup) = previous_backup_path.as_ref() {
            let _ = std::fs::rename(backup, &destination);
        }
        let _ = std::fs::remove_dir_all(&staging);
        return Err(OpenaiTunnelError::Io(error.to_string()));
    }
    let _ = std::fs::remove_dir_all(&staging);
    Ok(TunnelClientInstallOutcome {
        installed_path: destination,
        previous_backup_path,
        checksum_verified: expected_sha256.is_some(),
    })
}

pub fn startup_never_downloads_client(config: &OpenaiTunnelConfig) -> bool {
    !config
        .client_path
        .as_deref()
        .is_some_and(|value| value.starts_with("https://"))
}

pub fn credential_environment_present(env_lookup: impl Fn(&str) -> Option<OsString>) -> bool {
    env_lookup("CONTROL_PLANE_API_KEY").is_some()
}

pub async fn run_tunnel_client_doctor(
    path: &Path,
    profile_name: &str,
) -> Result<String, OpenaiTunnelError> {
    if profile_name.trim().is_empty() {
        return Err(OpenaiTunnelError::Unsupported(
            "OpenAI tunnel profile name is missing".into(),
        ));
    }
    run_client_command(path, ["doctor", "--profile", profile_name, "--explain"]).await
}

pub fn spawn_tunnel_client_run(
    path: &Path,
    profile_name: &str,
) -> Result<ManagedTunnelProcess, OpenaiTunnelError> {
    if profile_name.trim().is_empty() {
        return Err(OpenaiTunnelError::Unsupported(
            "OpenAI tunnel profile name is missing".into(),
        ));
    }
    let mut child = Command::new(path)
        .arg("run")
        .arg("--profile")
        .arg(profile_name)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| OpenaiTunnelError::Command(error.to_string()))?;
    let stdout_task = child.stdout.take().map(read_bounded_child_output);
    let stderr_task = child.stderr.take().map(read_bounded_child_output);
    Ok(ManagedTunnelProcess {
        child,
        stdout_task,
        stderr_task,
    })
}

pub async fn probe_tunnel_client_readiness(
    admin_base_url: &str,
    timeout: Duration,
) -> Result<OpenaiTunnelReadiness, OpenaiTunnelError> {
    let base = normalize_admin_base_url(admin_base_url)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|error| OpenaiTunnelError::Network(redact_network_error(&error.to_string())))?;
    let started = std::time::Instant::now();
    let health_url = base
        .join("healthz")
        .map_err(|_| OpenaiTunnelError::InvalidPath("health endpoint was invalid".into()))?;
    let ready_url = base
        .join("readyz")
        .map_err(|_| OpenaiTunnelError::InvalidPath("ready endpoint was invalid".into()))?;

    let mut health_live = false;
    let mut last_reason = None;
    while started.elapsed() < timeout {
        match client.get(health_url.clone()).send().await {
            Ok(response) if response.status().is_success() => health_live = true,
            Ok(response) if response.status().is_redirection() => {
                return Err(OpenaiTunnelError::Network(
                    "health endpoint redirected; refusing readiness inference".into(),
                ));
            }
            Ok(_) | Err(_) => {}
        }

        match client.get(ready_url.clone()).send().await {
            Ok(response) if response.status().is_success() => {
                return Ok(OpenaiTunnelReadiness {
                    ready: true,
                    health_live,
                    redacted_reason: None,
                });
            }
            Ok(response) if response.status().is_redirection() => {
                return Err(OpenaiTunnelError::Network(
                    "ready endpoint redirected; refusing readiness inference".into(),
                ));
            }
            Ok(response) => {
                last_reason = Some(format!(
                    "readyz returned HTTP {}",
                    response.status().as_u16()
                ));
            }
            Err(error) => {
                last_reason = Some(redact_network_error(&error.to_string()));
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    Ok(OpenaiTunnelReadiness {
        ready: false,
        health_live,
        redacted_reason: last_reason,
    })
}

pub fn normalize_admin_base_url(value: &str) -> Result<reqwest::Url, OpenaiTunnelError> {
    let trimmed = value.trim().trim_end_matches('/');
    let without_ui = trimmed.strip_suffix("/ui").unwrap_or(trimmed);
    let mut url = reqwest::Url::parse(without_ui).map_err(|_| {
        OpenaiTunnelError::InvalidPath("admin health URL must be a valid loopback URL".into())
    })?;
    if url.scheme() != "http" {
        return Err(OpenaiTunnelError::InvalidPath(
            "admin health URL must use http on loopback".into(),
        ));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(OpenaiTunnelError::InvalidPath(
            "admin health URL must not include credentials, query, or fragment".into(),
        ));
    }
    let host = url.host_str().ok_or_else(|| {
        OpenaiTunnelError::InvalidPath("admin health URL must include a host".into())
    })?;
    let is_loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|addr| addr.is_loopback());
    if !is_loopback {
        return Err(OpenaiTunnelError::InvalidPath(
            "admin health URL must be loopback-only".into(),
        ));
    }
    url.set_path("/");
    Ok(url)
}

fn read_bounded_child_output<R>(mut reader: R) -> JoinHandle<String>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut output = Vec::new();
        let mut buf = [0_u8; 1024];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let remaining = MAX_CHILD_OUTPUT_BYTES.saturating_sub(output.len());
                    if remaining == 0 {
                        break;
                    }
                    output.extend_from_slice(&buf[..n.min(remaining)]);
                }
            }
        }
        redact_tunnel_output(&String::from_utf8_lossy(&output))
    })
}

async fn run_client_command<const N: usize>(
    path: &Path,
    args: [&str; N],
) -> Result<String, OpenaiTunnelError> {
    let output = tokio::time::timeout(COMMAND_TIMEOUT, Command::new(path).args(args).output())
        .await
        .map_err(|_| OpenaiTunnelError::Command("client command timed out".into()))?
        .map_err(|error| OpenaiTunnelError::Command(error.to_string()))?;
    if !output.status.success() {
        return Err(OpenaiTunnelError::Command(format!(
            "client exited with status {}",
            output.status.code().unwrap_or(-1)
        )));
    }
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    if text.trim().is_empty() {
        text = String::from_utf8_lossy(&output.stderr).to_string();
    }
    Ok(text)
}

fn executable_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["tunnel-client.exe", "tunnel-client.cmd", "tunnel-client"]
    } else {
        &["tunnel-client"]
    }
}

fn supported_profile_operations(help: &str) -> Vec<String> {
    let mut operations = Vec::new();
    let lower = help.to_ascii_lowercase();
    for (needle, name) in [
        ("init", "init"),
        ("profile", "profile"),
        ("doctor", "doctor"),
        ("run", "run"),
    ] {
        if lower.contains(needle) {
            operations.push(name.to_string());
        }
    }
    operations
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::new();
    for path in paths {
        let key = path.to_string_lossy().to_ascii_lowercase();
        if seen.insert(key) {
            deduped.push(path);
        }
    }
    deduped
}

fn bounded_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(160)
        .collect()
}

fn parse_sha256_digest(value: &str) -> Option<String> {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix("sha256:") {
        return Some(rest.to_ascii_lowercase());
    }
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Some(value.to_ascii_lowercase());
    }
    None
}

fn windows_arch_asset_token() -> Result<&'static str, OpenaiTunnelError> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("amd64"),
        "aarch64" => Ok("arm64"),
        _ => Err(OpenaiTunnelError::Release(
            "unsupported Windows tunnel-client architecture".into(),
        )),
    }
}

fn validate_official_download_url(url: &str) -> Result<(), OpenaiTunnelError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| OpenaiTunnelError::Release("download URL was invalid".into()))?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("github.com")
        || !parsed
            .path()
            .starts_with("/openai/tunnel-client/releases/download/")
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(OpenaiTunnelError::Release(
            "download URL was not an official openai/tunnel-client release asset".into(),
        ));
    }
    Ok(())
}

pub fn redact_tunnel_output(text: &str) -> String {
    let patterns = [
        regex::Regex::new(r"sk-[A-Za-z0-9_-]{12,}").expect("regex"),
        regex::Regex::new(r"(?i)(CONTROL_PLANE_API_KEY=)[^\s]+").expect("regex"),
        regex::Regex::new(r"(?i)(authorization:\s*bearer\s+)[^\s]+").expect("regex"),
        regex::Regex::new(r"https?://[^\s]+/[A-Za-z0-9_-]{16,}/mcp").expect("regex"),
    ];
    let mut redacted = text.to_string();
    for pattern in patterns {
        redacted = pattern.replace_all(&redacted, "<redacted>").to_string();
    }
    redacted.chars().take(MAX_CHILD_OUTPUT_BYTES).collect()
}

fn redact_path_error(error: std::io::Error) -> String {
    format!("{}", error.kind())
}

fn redact_network_error(error: &str) -> String {
    error
        .lines()
        .next()
        .unwrap_or("network error")
        .chars()
        .take(200)
        .collect()
}

fn extract_archive_to_dir(bytes: &[u8], dir: &Path) -> Result<(), OpenaiTunnelError> {
    let reader = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|_| OpenaiTunnelError::Archive("archive was not a valid zip".into()))?;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|_| OpenaiTunnelError::Archive("archive entry could not be read".into()))?;
        if file.is_dir() {
            continue;
        }
        let Some(name) = file.enclosed_name().map(|path| path.to_path_buf()) else {
            return Err(OpenaiTunnelError::Archive(
                "archive contained an unsafe path".into(),
            ));
        };
        let output_path = dir.join(name);
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| OpenaiTunnelError::Io(error.to_string()))?;
        }
        let mut output = std::fs::File::create(&output_path)
            .map_err(|error| OpenaiTunnelError::Io(error.to_string()))?;
        std::io::copy(&mut file, &mut output)
            .map_err(|error| OpenaiTunnelError::Io(error.to_string()))?;
    }
    Ok(())
}

fn find_extracted_client(dir: &Path) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        let entries = std::fs::read_dir(path).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if executable_names().iter().any(|name| *name == file_name) {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("catdesk-openai-tunnel-{name}-{unique}"));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn fake_client(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        if cfg!(windows) {
            std::fs::write(
                &path,
                "@echo off\r\nif \"%1\"==\"--version\" (echo tunnel-client v0.0.7 & exit /b 0)\r\nif \"%1 %2\"==\"help quickstart\" (echo tunnel-client quickstart --mcp-server-url doctor /ui init profile run & exit /b 0)\r\necho unsupported\r\nexit /b 1\r\n",
            )
            .expect("write fake client");
        } else {
            std::fs::write(
                &path,
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo tunnel-client v0.0.7; exit 0; fi\nif [ \"$1 $2\" = \"help quickstart\" ]; then echo 'tunnel-client quickstart --mcp-server-url doctor /ui init profile run'; exit 0; fi\necho unsupported\nexit 1\n",
            )
            .expect("write fake client");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        }
        path
    }

    fn zip_with_file(file_name: &str, contents: &[u8]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        writer
            .start_file(file_name, zip::write::SimpleFileOptions::default())
            .expect("start file");
        writer.write_all(contents).expect("write");
        writer.finish().expect("finish").into_inner()
    }

    #[tokio::test]
    async fn discovery_prefers_explicit_path_before_path_and_user_tools() {
        let root = temp_dir("precedence");
        let explicit = fake_client(
            &root,
            if cfg!(windows) {
                "tunnel-client.cmd"
            } else {
                "tunnel-client"
            },
        );
        let path_dir = root.join("path");
        let tools_dir = root.join("tools");
        std::fs::create_dir_all(&path_dir).expect("path dir");
        std::fs::create_dir_all(&tools_dir).expect("tools dir");
        fake_client(
            &path_dir,
            if cfg!(windows) {
                "tunnel-client.cmd"
            } else {
                "tunnel-client"
            },
        );
        fake_client(
            &tools_dir,
            if cfg!(windows) {
                "tunnel-client.cmd"
            } else {
                "tunnel-client"
            },
        );
        let options = TunnelClientDiscoveryOptions {
            explicit_path: Some(explicit.clone()),
            path_var: Some(std::env::join_paths([path_dir]).expect("path")),
            user_tools_dir: tools_dir,
            known_paths: Vec::new(),
            forbidden_roots: Vec::new(),
        };
        let metadata = discover_tunnel_client(&options).await.expect("discover");
        assert_eq!(
            metadata.path,
            std::fs::canonicalize(explicit).expect("canonical")
        );
        assert!(metadata.supports_http_mcp);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn invalid_or_unsupported_executable_is_rejected() {
        let root = temp_dir("invalid");
        let bad = root.join(if cfg!(windows) {
            "tunnel-client.cmd"
        } else {
            "tunnel-client"
        });
        std::fs::write(&bad, "not executable").expect("write bad");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        }
        let result = validate_client_candidate(&bad, &[]).await;
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn release_plan_selects_windows_asset_and_records_missing_checksum_warning() {
        let value = serde_json::json!({
            "tag_name": "v0.0.7",
            "html_url": "https://github.com/openai/tunnel-client/releases/tag/v0.0.7",
            "prerelease": false,
            "assets": [{
                "name": "tunnel-client-windows-amd64.zip",
                "browser_download_url": "https://github.com/openai/tunnel-client/releases/download/v0.0.7/tunnel-client-windows-amd64.zip",
                "size": 123
            }]
        });
        let plan = release_plan_from_github_json(&value).expect("plan");
        assert_eq!(plan.release_tag, "v0.0.7");
        assert!(!plan.checksum_available);
        assert!(plan.warning.is_some());
    }

    #[test]
    fn prerelease_release_metadata_is_rejected() {
        let value = serde_json::json!({
            "tag_name": "v0.0.8-rc1",
            "html_url": "https://github.com/openai/tunnel-client/releases/tag/v0.0.8-rc1",
            "prerelease": true,
            "assets": []
        });
        let error = release_plan_from_github_json(&value).expect_err("prerelease rejected");
        assert!(error.to_string().contains("prerelease"));
    }

    #[test]
    fn windows_asset_must_match_current_architecture_and_size_bounds() {
        let wrong_arch = if std::env::consts::ARCH == "x86_64" {
            "tunnel-client-windows-arm64.zip"
        } else {
            "tunnel-client-windows-amd64.zip"
        };
        let assets = vec![serde_json::json!({
            "name": wrong_arch,
            "browser_download_url": "https://github.com/openai/tunnel-client/releases/download/v0.0.7/tunnel-client.zip",
            "size": 123
        })];
        let error = select_windows_asset(&assets).expect_err("wrong architecture rejected");
        assert!(error.to_string().contains("architecture"));
    }

    #[test]
    fn windows_asset_download_url_must_be_official_release_url() {
        let arch = windows_arch_asset_token().expect("supported arch");
        let assets = vec![serde_json::json!({
            "name": format!("tunnel-client-windows-{arch}.zip"),
            "browser_download_url": "https://example.invalid/openai/tunnel-client/releases/download/v0.0.7/tunnel-client.zip",
            "size": 123
        })];
        let error = select_windows_asset(&assets).expect_err("unofficial URL rejected");
        assert!(error.to_string().contains("official"));
    }

    #[test]
    fn checksum_failure_blocks_archive_install() {
        let bytes = b"not the artifact";
        let error = verify_archive_checksum(bytes, &"0".repeat(64)).unwrap_err();
        assert!(error.to_string().contains("SHA-256"));
    }

    #[test]
    fn archive_extraction_failure_preserves_previous_binary() {
        let root = temp_dir("rollback");
        let install_dir = root.join("install");
        std::fs::create_dir_all(&install_dir).expect("install dir");
        let binary = install_dir.join(if cfg!(windows) {
            "tunnel-client.exe"
        } else {
            "tunnel-client"
        });
        std::fs::write(&binary, "previous").expect("previous");
        let result = install_client_archive_bytes(b"not a zip", None, &install_dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("SHA-256"));
        assert_eq!(
            std::fs::read_to_string(&binary).expect("previous"),
            "previous"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn valid_archive_installs_and_backs_up_previous_binary() {
        let root = temp_dir("install");
        let install_dir = root.join("install");
        std::fs::create_dir_all(&install_dir).expect("install dir");
        let file_name = if cfg!(windows) {
            "tunnel-client.exe"
        } else {
            "tunnel-client"
        };
        std::fs::write(install_dir.join(file_name), "previous").expect("previous");
        let archive = zip_with_file(file_name, b"new-client");
        let checksum = sha256_hex(&archive);
        let outcome =
            install_client_archive_bytes(&archive, Some(&checksum), &install_dir).expect("install");
        assert!(outcome.checksum_verified);
        assert_eq!(
            std::fs::read_to_string(outcome.installed_path).expect("installed"),
            "new-client"
        );
        assert_eq!(
            std::fs::read_to_string(outcome.previous_backup_path.unwrap()).expect("backup"),
            "previous"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn startup_policy_does_not_download_or_mutate_path_or_credentials() {
        let config = OpenaiTunnelConfig::default();
        assert!(startup_never_downloads_client(&config));
        let network_config = OpenaiTunnelConfig {
            client_path: Some("https://github.com/openai/tunnel-client/releases/latest".into()),
            ..OpenaiTunnelConfig::default()
        };
        assert!(!startup_never_downloads_client(&network_config));
        assert!(credential_environment_present(|name| {
            assert_eq!(name, "CONTROL_PLANE_API_KEY");
            Some(OsString::from("present"))
        }));
        assert!(!credential_environment_present(|_| None));
    }

    #[test]
    fn config_normalization_trims_without_storing_credentials() {
        let config = OpenaiTunnelConfig {
            client_path: Some("  C:/tools/tunnel-client.exe  ".into()),
            profile_name: "  ".into(),
            process_mode: OpenaiTunnelProcessMode::External,
            admin_ui_url: Some("  http://127.0.0.1:9900/ui  ".into()),
        }
        .normalized();
        assert_eq!(config.profile_name, DEFAULT_PROFILE_NAME);
        assert_eq!(
            config.client_path.as_deref(),
            Some("C:/tools/tunnel-client.exe")
        );
        assert_eq!(
            config.admin_ui_url.as_deref(),
            Some("http://127.0.0.1:9900/ui")
        );
    }

    #[test]
    fn admin_base_url_requires_loopback_and_strips_ui_path() {
        let url = normalize_admin_base_url("http://127.0.0.1:9900/ui").expect("loopback");
        assert_eq!(url.as_str(), "http://127.0.0.1:9900/");
        let error = normalize_admin_base_url("http://192.168.1.10:9900/ui")
            .expect_err("non-loopback rejected");
        assert!(error.to_string().contains("loopback"));
        let error = normalize_admin_base_url("http://127.0.0.1:9900/ui?token=secret")
            .expect_err("query rejected");
        assert!(error.to_string().contains("query"));
    }

    #[tokio::test]
    async fn readiness_probe_uses_readyz_not_only_healthz() {
        use axum::{Router, http::StatusCode, routing::get};
        let app = Router::new()
            .route("/healthz", get(|| async { (StatusCode::OK, "live") }))
            .route(
                "/readyz",
                get(|| async { (StatusCode::SERVICE_UNAVAILABLE, "not ready") }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let report =
            probe_tunnel_client_readiness(&format!("http://{addr}"), Duration::from_millis(500))
                .await
                .expect("probe");
        assert!(report.health_live);
        assert!(!report.ready);
        assert!(
            report
                .redacted_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("503"))
        );
        server.abort();
    }

    #[test]
    fn tunnel_output_redacts_credentials_and_routes() {
        let output = redact_tunnel_output(
            "CONTROL_PLANE_API_KEY=secret-value\nAuthorization: Bearer sk-secret-secret-secret\nhttps://example.ngrok-free.app/AbCdEfGhIjKlMnOpQrStUvWx/mcp",
        );
        assert!(!output.contains("secret-value"));
        assert!(!output.contains("sk-secret"));
        assert!(!output.contains("AbCdEfGhIjKlMnOpQrStUvWx"));
        assert!(output.contains("<redacted>"));
    }

    #[test]
    fn tunnel_output_is_bounded() {
        let output = redact_tunnel_output(&"x".repeat(MAX_CHILD_OUTPUT_BYTES + 1024));
        assert_eq!(output.len(), MAX_CHILD_OUTPUT_BYTES);
    }
}
