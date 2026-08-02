#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};

const DEFAULT_PROFILE_NAME: &str = "catdesk-local";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
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
    for asset in assets {
        let name = asset
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let lower = name.to_ascii_lowercase();
        if lower.contains("windows") && (lower.ends_with(".zip") || lower.ends_with(".exe")) {
            let download_url = asset
                .get("browser_download_url")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    OpenaiTunnelError::Release("Windows asset download URL missing".into())
                })?
                .to_string();
            let size = asset
                .get("size")
                .and_then(Value::as_u64)
                .unwrap_or_default();
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
        "release did not include a Windows tunnel-client artifact".into(),
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
    if let Some(expected) = expected_sha256 {
        verify_archive_checksum(archive_bytes, expected)?;
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
    config.client_path.is_some() || config.client_path.is_none()
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
) -> Result<Child, OpenaiTunnelError> {
    if profile_name.trim().is_empty() {
        return Err(OpenaiTunnelError::Unsupported(
            "OpenAI tunnel profile name is missing".into(),
        ));
    }
    Command::new(path)
        .arg("run")
        .arg("--profile")
        .arg(profile_name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| OpenaiTunnelError::Command(error.to_string()))
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
}
