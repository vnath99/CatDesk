//! Explicit, unprotected development Wake runtime for a new isolated control
//! workspace. This never installs or selects the production Wake package.

#![cfg(windows)]

use crate::{
    Result, VERSION,
    install_broker_offline_pins::full_identity_of_handle,
    install_broker_offline_v2::FullFileIdentityV2,
    protocol, runtime,
    store::{Config, Store, now, read},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, c_void},
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, IsTerminal, Read, Write},
    os::windows::{
        ffi::OsStrExt,
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const PURPOSE: &str = "CATDESK_WAKE_DEVELOPMENT_UNPROTECTED_V1";
const DEV_DIR: &str = "web-development";
const ROOT_DIR: &str = "runtime";
const PACKAGE_UPDATE_PURPOSE: &str = "CATDESK_WAKE_DEV_PACKAGE_UPDATE_V1";
const PACKAGE_UPDATE_ATTEMPT: &str = "package-update-attempt.json";
const RELEASE_VERIFICATION: &str = "verification.json";
const PACKAGE_FILES: [&str; 4] = [
    "CatDeskWakeHost.exe",
    "CatDeskBinagotchy.exe",
    "adapter.py",
    "wake_bridge.py",
];
const MAX_PACKAGE_FILE: u64 = 160 * 1024 * 1024;
const MAX_PYTHON_FILES: usize = 100_000;
const MAX_PYTHON_BYTES: u64 = 3 * 1024 * 1024 * 1024;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_ALL: u32 = 7;
const OPEN_EXISTING: u32 = 3;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const INVALID_HANDLE: *mut c_void = -1isize as *mut c_void;

#[path = "development_foreground.rs"]
mod foreground;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        path: *const u16,
        access: u32,
        share: u32,
        security: *mut c_void,
        disposition: u32,
        flags: u32,
        template: *mut c_void,
    ) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn IsProcessInJob(process: *mut c_void, job: *mut c_void, result: *mut i32) -> i32;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProductionRoot {
    path: String,
    identity: Option<FullFileIdentityV2>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Authority {
    schema_version: u32,
    purpose: String,
    workspace: String,
    workspace_identity: FullFileIdentityV2,
    runtime_identity: FullFileIdentityV2,
    package_id: String,
    artifacts: BTreeMap<String, String>,
    manifest_identity: FullFileIdentityV2,
    manifest_sha256: String,
    pointer_identity: FullFileIdentityV2,
    pointer_sha256: String,
    python_identity: FullFileIdentityV2,
    python_sha256: String,
    production_roots: Vec<ProductionRoot>,
    prepared_utc: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PackageUpdateAttempt {
    schema_version: u32,
    purpose: String,
    workspace: String,
    runtime_identity: FullFileIdentityV2,
    old_authority: Authority,
    old_authority_sha256: String,
    new_authority: Authority,
    new_authority_sha256: String,
    pointer_stage_name: String,
    authority_stage_name: String,
    started_utc: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    old_pointer_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retained_verification: Option<ReleaseVerification>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    restoration: Option<RetainedRelease>,
}

/// Local operator-provided development evidence. This records the reviewed
/// source/build/health references; it does not perform those reviews or confer
/// protected production authority. Existing package/authority schemas stay intact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReleaseVerification {
    schema_version: u32,
    package_id: String,
    manifest_sha256: String,
    artifacts: BTreeMap<String, String>,
    source_commit: String,
    build_evidence_sha256: String,
    review_evidence_sha256: String,
    working_health_evidence_sha256: String,
    working_health_observed_utc: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RetainedRelease {
    package_id: String,
    artifacts: BTreeMap<String, String>,
    manifest_identity: FullFileIdentityV2,
    manifest_sha256: String,
    verification: ReleaseVerification,
}

fn lowercase_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_release_verification(
    record: &ReleaseVerification,
    authority: &Authority,
) -> Result<()> {
    validate_verification_binding(
        record,
        &authority.package_id,
        &authority.manifest_sha256,
        &authority.artifacts,
    )
}

fn validate_verification_binding(
    record: &ReleaseVerification,
    package_id: &str,
    manifest_sha256: &str,
    artifacts: &BTreeMap<String, String>,
) -> Result<()> {
    if record.schema_version != 1
        || record.package_id != package_id
        || record.manifest_sha256 != manifest_sha256
        || record.artifacts != *artifacts
        || !lowercase_hex(&record.source_commit, 40)
        || [
            &record.build_evidence_sha256,
            &record.review_evidence_sha256,
            &record.working_health_evidence_sha256,
        ]
        .into_iter()
        .any(|value| !lowercase_hex(value, 64))
        || record.working_health_observed_utc == 0
        || record.working_health_observed_utc > now()
    {
        return Err("DEV_RELEASE_VERIFICATION_INVALID".into());
    }
    Ok(())
}

fn read_release_verification(
    root: &Path,
    authority: &Authority,
) -> Result<Option<ReleaseVerification>> {
    let package = root.join("versions").join(&authority.package_id);
    let path = package.join(RELEASE_VERIFICATION);
    if !present(&path)? {
        return Ok(None);
    }
    plain(&package, true)?;
    let record: ReleaseVerification = read_json(&path, 16 * 1024)?;
    validate_release_verification(&record, authority)?;
    let manifest = package.join("manifest.json");
    if identity(&manifest, false)? != authority.manifest_identity
        || hash_file(&manifest, 16 * 1024)? != authority.manifest_sha256
    {
        return Err("DEV_RELEASE_VERIFICATION_PACKAGE_CHANGED".into());
    }
    for name in PACKAGE_FILES {
        if authority.artifacts.get(name) != Some(&hash_file(&package.join(name), MAX_PACKAGE_FILE)?)
        {
            return Err("DEV_RELEASE_VERIFICATION_PACKAGE_CHANGED".into());
        }
    }
    Ok(Some(record))
}

fn update_pointer(
    old: &Authority,
    new_id: &str,
    hashes: &BTreeMap<String, String>,
    old_pointer_json: Option<&str>,
    retained_verification: Option<&ReleaseVerification>,
) -> Result<Value> {
    let mut pointer = package_pointer(new_id, hashes);
    let old_pointer = match old_pointer_json {
        Some(bytes) => {
            if bytes.len() > 4096
                || format!("{:x}", Sha256::digest(bytes.as_bytes())) != old.pointer_sha256
            {
                return Err("DEV_PACKAGE_UPDATE_OLD_POINTER_INVALID".into());
            }
            Some(
                serde_json::from_str::<Value>(bytes)
                    .map_err(|_| "DEV_PACKAGE_UPDATE_OLD_POINTER_INVALID")?,
            )
        }
        None => None,
    };
    let previous = match retained_verification {
        Some(verification) => {
            if old_pointer.is_none() {
                return Err("DEV_PACKAGE_UPDATE_OLD_POINTER_INVALID".into());
            }
            validate_release_verification(verification, old)?;
            Some(
                serde_json::to_value(RetainedRelease {
                    package_id: old.package_id.clone(),
                    artifacts: old.artifacts.clone(),
                    manifest_identity: old.manifest_identity.clone(),
                    manifest_sha256: old.manifest_sha256.clone(),
                    verification: verification.clone(),
                })
                .map_err(|_| "DEV_RUNTIME_ENCODING_INVALID")?,
            )
        }
        None => old_pointer
            .as_ref()
            .and_then(|value| value.get("previousVerifiedRelease"))
            .cloned(),
    };
    if let Some(previous) = previous
        && previous.get("packageId").and_then(Value::as_str) != Some(new_id)
    {
        pointer["previousVerifiedRelease"] = previous;
    }
    if serde_json::to_vec(&pointer)
        .map_err(|_| "DEV_RUNTIME_ENCODING_INVALID")?
        .len()
        > 4096
    {
        return Err("DEV_PACKAGE_UPDATE_POINTER_CAPACITY".into());
    }
    Ok(pointer)
}

/// Record local operator-reviewed evidence for this exact selected development
/// package. No health/CI claim is inferred by merely selecting or hashing bytes.
pub fn record_release_verification(workspace: &Path, input_file: &Path) -> Result<Value> {
    let workspace = canonical_workspace(workspace)?;
    let (root, _) = validated_authority(&workspace)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    let store = Store::open(&root)?;
    let _host = store.host_lock()?;
    let _browser = runtime::browser_session_lock(&root, Duration::ZERO)?;
    let control: runtime::Control = read(&root.join("control.json"))?;
    if control.schema_version != 1 || control.desired != "STOPPED" {
        return Err("DEV_PACKAGE_UPDATE_HOST_NOT_STOPPED".into());
    }
    let (_, authority) = validated_authority(&workspace)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    plain(input_file, false)?;
    let input =
        fs::canonicalize(input_file).map_err(|_| "DEV_RELEASE_VERIFICATION_INPUT_INVALID")?;
    if !input.starts_with(&workspace) {
        return Err("DEV_RELEASE_VERIFICATION_INPUT_SCOPE_INVALID".into());
    }
    let record: ReleaseVerification = read_json(&input, 16 * 1024)?;
    validate_release_verification(&record, &authority)?;
    let package = root.join("versions").join(&authority.package_id);
    let path = package.join(RELEASE_VERIFICATION);
    let state = match read_release_verification(&root, &authority)? {
        Some(existing) if existing == record => "VERIFICATION_ALREADY_RECORDED",
        Some(_) => return Err("DEV_RELEASE_VERIFICATION_CONFLICT".into()),
        None => {
            let stage = package.join(update_stage_name("verification", &authority.package_id)?);
            write_json_new(&stage, &record)?;
            fs::rename(&stage, &path).map_err(|_| "DEV_RELEASE_VERIFICATION_COMMIT_FAILED")?;
            if read_release_verification(&root, &authority)?.as_ref() != Some(&record) {
                return Err("DEV_RELEASE_VERIFICATION_READBACK_FAILED".into());
            }
            "VERIFICATION_RECORDED"
        }
    };
    Ok(json!({"mode":"DEVELOPMENT_UNPROTECTED","status":state,
        "evidenceAuthority":"LOCAL_OPERATOR_PROVIDED","packageId":authority.package_id,
        "sourceCommit":record.source_commit,"healthChecksPerformed":false,
        "productionAuthorityCreated":false,"hostStarted":false,"browserAction":"NONE"}))
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain([0]).collect()
}

fn plain(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "DEV_RUNTIME_OBJECT_UNAVAILABLE")?;
    if metadata.file_attributes() & 0x400 != 0
        || metadata.file_type().is_symlink()
        || directory != metadata.is_dir()
        || (!directory && !metadata.is_file())
    {
        return Err("DEV_RUNTIME_OBJECT_UNSAFE".into());
    }
    Ok(())
}

fn identity(path: &Path, directory: bool) -> Result<FullFileIdentityV2> {
    plain(path, directory)?;
    let raw = unsafe {
        CreateFileW(
            wide(path.as_os_str()).as_ptr(),
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_ALL,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
            std::ptr::null_mut(),
        )
    };
    if raw.is_null() || raw == INVALID_HANDLE {
        return Err("DEV_RUNTIME_OBJECT_UNAVAILABLE".into());
    }
    let file = unsafe { File::from_raw_handle(raw) };
    full_identity_of_handle(file.as_raw_handle())
}

fn hash_file(path: &Path, limit: u64) -> Result<String> {
    plain(path, false)?;
    let mut file = File::open(path).map_err(|_| "DEV_RUNTIME_OBJECT_UNAVAILABLE")?;
    let before = file
        .metadata()
        .map_err(|_| "DEV_RUNTIME_OBJECT_UNAVAILABLE")?;
    if before.len() > limit {
        return Err("DEV_RUNTIME_FILE_SIZE_INVALID".into());
    }
    let mut hash = Sha256::new();
    let mut read = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "DEV_RUNTIME_READ_FAILED")?;
        if count == 0 {
            break;
        }
        read = read
            .checked_add(count as u64)
            .ok_or("DEV_RUNTIME_FILE_SIZE_INVALID")?;
        if read > limit {
            return Err("DEV_RUNTIME_FILE_SIZE_INVALID".into());
        }
        hash.update(&buffer[..count]);
    }
    let after = file
        .metadata()
        .map_err(|_| "DEV_RUNTIME_OBJECT_UNAVAILABLE")?;
    if read != before.len() || after.len() != before.len() {
        return Err("DEV_RUNTIME_CONTENT_CHANGED".into());
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "DEV_RUNTIME_CREATE_CONFLICT")?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "DEV_RUNTIME_WRITE_FAILED".into())
}

fn copy_new(source: &Path, destination: &Path, limit: u64) -> Result<String> {
    let before = hash_file(source, limit)?;
    let mut input = File::open(source).map_err(|_| "DEV_RUNTIME_READ_FAILED")?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| "DEV_RUNTIME_CREATE_CONFLICT")?;
    let size = std::io::copy(&mut input, &mut output).map_err(|_| "DEV_RUNTIME_COPY_FAILED")?;
    output.sync_all().map_err(|_| "DEV_RUNTIME_COPY_FAILED")?;
    if size > limit
        || hash_file(destination, limit)? != before
        || hash_file(source, limit)? != before
    {
        return Err("DEV_RUNTIME_COPY_MISMATCH".into());
    }
    Ok(before)
}

fn dev_dir(workspace: &Path) -> PathBuf {
    workspace.join(".catdesk").join(DEV_DIR)
}
fn runtime_root(workspace: &Path) -> PathBuf {
    dev_dir(workspace).join(ROOT_DIR)
}

fn canonical_workspace(workspace: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(workspace).map_err(|_| "DEV_WORKSPACE_UNAVAILABLE")?;
    plain(&canonical, true)?;
    if !canonical.is_absolute() {
        return Err("DEV_WORKSPACE_INVALID".into());
    }
    Ok(canonical)
}

fn historical_root() -> Result<PathBuf> {
    let home = std::env::var_os("USERPROFILE").ok_or("DEV_PRODUCTION_ROOT_UNKNOWN")?;
    Ok(PathBuf::from(home)
        .join(r"AppData\Local\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\CatDeskWake"))
}

fn known_production_paths() -> Result<Vec<PathBuf>> {
    let direct = runtime::default_root()?;
    let historical = historical_root()?;
    if direct == historical {
        Ok(vec![direct])
    } else {
        Ok(vec![direct, historical])
    }
}

fn production_roots() -> Result<Vec<ProductionRoot>> {
    let mut roots = Vec::new();
    for path in known_production_paths()? {
        let object = match fs::symlink_metadata(&path) {
            Ok(_) => Some(identity(&path, true)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err("DEV_PRODUCTION_ROOT_UNAVAILABLE".into()),
        };
        roots.push(ProductionRoot {
            path: path.to_string_lossy().into_owned(),
            identity: object,
        });
    }
    Ok(roots)
}

fn require_new_workspace(workspace: &Path) -> Result<()> {
    let mut entries = fs::read_dir(workspace).map_err(|_| "DEV_WORKSPACE_UNAVAILABLE")?;
    match entries.next() {
        None => Ok(()),
        Some(Ok(entry)) if entry.file_name() == ".catdesk" && entries.next().is_none() => {
            plain(&entry.path(), true)?;
            if fs::read_dir(entry.path())
                .map_err(|_| "DEV_WORKSPACE_UNAVAILABLE")?
                .next()
                .is_none()
            {
                Ok(())
            } else {
                Err("DEV_WORKSPACE_NOT_NEW".into())
            }
        }
        _ => Err("DEV_WORKSPACE_NOT_NEW".into()),
    }
}

fn require_distinct(
    workspace: &Path,
    source: &Path,
    python: &Path,
    roots: &[ProductionRoot],
) -> Result<()> {
    if source.starts_with(workspace)
        || python.starts_with(workspace)
        || source == python
        || source.starts_with(python)
        || python.starts_with(source)
    {
        return Err("DEV_SOURCE_OVERLAP".into());
    }
    let workspace_id = identity(workspace, true)?;
    for root in roots {
        let path = Path::new(&root.path);
        if workspace.starts_with(path)
            || path.starts_with(workspace)
            || root.identity.as_ref() == Some(&workspace_id)
        {
            return Err("DEV_PRODUCTION_ROOT_COLLISION".into());
        }
    }
    Ok(())
}

fn source_artifacts(source: &Path) -> Result<BTreeMap<String, String>> {
    let expected = PACKAGE_FILES
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();
    for entry in fs::read_dir(source).map_err(|_| "DEV_PACKAGE_UNAVAILABLE")? {
        let entry = entry.map_err(|_| "DEV_PACKAGE_UNAVAILABLE")?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "DEV_PACKAGE_INVENTORY_INVALID")?;
        actual.insert(name);
    }
    if actual != expected {
        return Err("DEV_PACKAGE_INVENTORY_INVALID".into());
    }
    let mut hashes = BTreeMap::new();
    for name in PACKAGE_FILES {
        if fs::metadata(source.join(name))
            .map_err(|_| "DEV_PACKAGE_UNAVAILABLE")?
            .len()
            == 0
        {
            return Err("DEV_PACKAGE_INVENTORY_INVALID".into());
        }
        hashes.insert(
            name.into(),
            hash_file(&source.join(name), MAX_PACKAGE_FILE)?,
        );
    }
    let adapter = hash_file(&source.join("adapter.py"), 4 * 1024 * 1024)?;
    let bridge = hash_file(&source.join("wake_bridge.py"), 4 * 1024 * 1024)?;
    if adapter != format!("{:x}", Sha256::digest(include_bytes!("../adapter.py")))
        || bridge
            != format!(
                "{:x}",
                Sha256::digest(include_bytes!("../../scripts/wake_bridge.py"))
            )
    {
        return Err("DEV_PACKAGE_SCRIPT_SOURCE_MISMATCH".into());
    }
    Ok(hashes)
}

#[derive(Default)]
struct CopyBudget {
    files: usize,
    bytes: u64,
}

fn copy_python_tree(source: &Path, destination: &Path, budget: &mut CopyBudget) -> Result<()> {
    plain(source, true)?;
    fs::create_dir(destination).map_err(|_| "DEV_RUNTIME_CREATE_CONFLICT")?;
    for entry in fs::read_dir(source).map_err(|_| "DEV_PYTHON_UNAVAILABLE")? {
        let entry = entry.map_err(|_| "DEV_PYTHON_UNAVAILABLE")?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&from).map_err(|_| "DEV_PYTHON_UNAVAILABLE")?;
        if metadata.file_attributes() & 0x400 != 0 || metadata.file_type().is_symlink() {
            return Err("DEV_PYTHON_LINK_REFUSED".into());
        }
        if metadata.is_dir() {
            copy_python_tree(&from, &to, budget)?;
        } else if metadata.is_file() {
            budget.files = budget.files.checked_add(1).ok_or("DEV_PYTHON_TOO_LARGE")?;
            budget.bytes = budget
                .bytes
                .checked_add(metadata.len())
                .ok_or("DEV_PYTHON_TOO_LARGE")?;
            if budget.files > MAX_PYTHON_FILES || budget.bytes > MAX_PYTHON_BYTES {
                return Err("DEV_PYTHON_TOO_LARGE".into());
            }
            copy_new(&from, &to, MAX_PYTHON_BYTES)?;
        } else {
            return Err("DEV_PYTHON_INVENTORY_INVALID".into());
        }
    }
    Ok(())
}

fn package_id(hashes: &BTreeMap<String, String>) -> Result<String> {
    if hashes.len() != PACKAGE_FILES.len()
        || PACKAGE_FILES.iter().any(|name| {
            !hashes.get(*name).is_some_and(|hash| {
                hash.len() == 64
                    && hash
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            })
        })
    {
        return Err("DEV_PACKAGE_INVENTORY_INVALID".into());
    }
    let host = hashes
        .get("CatDeskWakeHost.exe")
        .ok_or("DEV_PACKAGE_INVENTORY_INVALID")?;
    let gui = hashes
        .get("CatDeskBinagotchy.exe")
        .ok_or("DEV_PACKAGE_INVENTORY_INVALID")?;
    Ok(format!("{}-{}-{}", VERSION, &host[..12], &gui[..12]))
}

// The original development package ID used only the two executable hashes.
// Keep accepting it for already prepared roots, while upgrades use every
// immutable artifact so a bridge-only repair cannot collide with that root.
fn updated_package_id(hashes: &BTreeMap<String, String>) -> Result<String> {
    package_id(hashes)?;
    let mut digest = Sha256::new();
    for name in PACKAGE_FILES {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(hashes[name].as_bytes());
        digest.update([0]);
    }
    Ok(format!("{}-{:x}", VERSION, digest.finalize()))
}

pub(crate) fn updated_package_id_from_pointer(pointer: &Value) -> Result<String> {
    let mut hashes = BTreeMap::new();
    for (field, name) in [
        ("hostSha256", "CatDeskWakeHost.exe"),
        ("binagotchySha256", "CatDeskBinagotchy.exe"),
        ("adapterSha256", "adapter.py"),
        ("wakeBridgeSha256", "wake_bridge.py"),
    ] {
        let hash = pointer
            .get(field)
            .and_then(Value::as_str)
            .ok_or("DEV_RUNTIME_POINTER_INVALID")?;
        hashes.insert(name.to_owned(), hash.to_owned());
    }
    updated_package_id(&hashes)
}

fn package_id_matches(hashes: &BTreeMap<String, String>, id: &str) -> Result<bool> {
    Ok(package_id(hashes)? == id || updated_package_id(hashes)? == id)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, limit: u64) -> Result<T> {
    plain(path, false)?;
    let bytes = fs::read(path).map_err(|_| "DEV_RUNTIME_READ_FAILED")?;
    if bytes.is_empty() || bytes.len() as u64 > limit {
        return Err("DEV_RUNTIME_RECORD_INVALID".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "DEV_RUNTIME_RECORD_INVALID".into())
}

fn write_json_new(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| "DEV_RUNTIME_ENCODING_INVALID")?;
    write_new(path, &bytes)
}

fn package_manifest(hashes: &BTreeMap<String, String>) -> Value {
    json!({
        "schemaVersion":1,"version":VERSION,"protocolVersion":1,
        "installedUtc":now().to_string(),"artifacts":hashes,
        "acceptance":"DEVELOPMENT_NOT_ACTIVATED"
    })
}

fn package_pointer(id: &str, hashes: &BTreeMap<String, String>) -> Value {
    json!({
        "schemaVersion":1,"directory":id,
        "hostSha256":hashes["CatDeskWakeHost.exe"],
        "binagotchySha256":hashes["CatDeskBinagotchy.exe"],
        "adapterSha256":hashes["adapter.py"],
        "wakeBridgeSha256":hashes["wake_bridge.py"]
    })
}

fn verify_update_package(
    root: &Path,
    id: &str,
    hashes: &BTreeMap<String, String>,
) -> Result<(FullFileIdentityV2, String)> {
    if !package_id_matches(hashes, id)? {
        return Err("DEV_PACKAGE_UPDATE_IDENTITY_MISMATCH".into());
    }
    let package = root.join("versions").join(id);
    plain(&package, true)?;
    for name in PACKAGE_FILES {
        if hash_file(&package.join(name), MAX_PACKAGE_FILE)? != hashes[name] {
            return Err("DEV_PACKAGE_UPDATE_CONTENT_MISMATCH".into());
        }
    }
    let manifest_path = package.join("manifest.json");
    let manifest: Value = read_json(&manifest_path, 16 * 1024)?;
    if manifest.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || manifest.get("version").and_then(Value::as_str) != Some(VERSION)
        || manifest.get("protocolVersion").and_then(Value::as_u64) != Some(1)
        || manifest.get("acceptance").and_then(Value::as_str) != Some("DEVELOPMENT_NOT_ACTIVATED")
        || manifest.get("artifacts")
            != Some(&serde_json::to_value(hashes).map_err(|_| "DEV_RUNTIME_ENCODING_INVALID")?)
    {
        return Err("DEV_PACKAGE_UPDATE_MANIFEST_INVALID".into());
    }
    Ok((
        identity(&manifest_path, false)?,
        hash_file(&manifest_path, 16 * 1024)?,
    ))
}

fn stage_update_package(
    root: &Path,
    source: &Path,
    id: &str,
    hashes: &BTreeMap<String, String>,
) -> Result<(FullFileIdentityV2, String)> {
    let final_dir = root.join("versions").join(id);
    if present(&final_dir)? {
        return verify_update_package(root, id, hashes);
    }
    let stage_name = format!(
        ".update-{}-{}-{}",
        std::process::id(),
        now(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "DEV_RUNTIME_CLOCK_UNAVAILABLE")?
            .subsec_nanos()
    );
    let stage = root.join("versions").join(stage_name);
    fs::create_dir(&stage).map_err(|_| "DEV_RUNTIME_CREATE_CONFLICT")?;
    for name in PACKAGE_FILES {
        if copy_new(&source.join(name), &stage.join(name), MAX_PACKAGE_FILE)? != hashes[name] {
            return Err("DEV_RUNTIME_COPY_MISMATCH".into());
        }
    }
    write_json_new(&stage.join("manifest.json"), &package_manifest(hashes))?;
    for name in PACKAGE_FILES {
        if hash_file(&stage.join(name), MAX_PACKAGE_FILE)? != hashes[name] {
            return Err("DEV_RUNTIME_COPY_MISMATCH".into());
        }
    }
    fs::rename(&stage, &final_dir).map_err(|_| "DEV_PACKAGE_UPDATE_PUBLISH_FAILED")?;
    verify_update_package(root, id, hashes)
}

fn update_stage_name(prefix: &str, id: &str) -> Result<String> {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "DEV_RUNTIME_CLOCK_UNAVAILABLE")?
        .as_nanos();
    Ok(format!(
        "{prefix}-{id}-{}-{suffix}.json",
        std::process::id()
    ))
}

fn valid_update_stage_name(name: &str, prefix: &str, id: &str) -> bool {
    name.starts_with(&format!("{prefix}-{id}-"))
        && name.ends_with(".json")
        && name.len() <= 192
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
}

fn replace_update_file(from: &Path, to: &Path) -> Result<()> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
        fn GetLastError() -> u32;
    }
    if from.parent() != to.parent() {
        return Err("DEV_PACKAGE_UPDATE_PATH_INVALID".into());
    }
    let from_wide = wide(from.as_os_str());
    let to_wide = wide(to.as_os_str());
    for retry in 0..20 {
        if unsafe { MoveFileExW(from_wide.as_ptr(), to_wide.as_ptr(), 1 | 8) } != 0 {
            return Ok(());
        }
        let code = unsafe { GetLastError() };
        if !matches!(code, 5 | 32 | 33) || retry == 19 {
            return Err("DEV_PACKAGE_UPDATE_REPLACE_FAILED".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err("DEV_PACKAGE_UPDATE_REPLACE_FAILED".into())
}

/// Create one fresh, user-writable development runtime. Partial preparation is
/// sticky: this function never repairs or retries an uncertain copy.
pub fn prepare(
    control_workspace: &Path,
    source_package: &Path,
    python_runtime: &Path,
) -> Result<Value> {
    let workspace = canonical_workspace(control_workspace)?;
    require_new_workspace(&workspace)?;
    plain(source_package, true)?;
    plain(python_runtime, true)?;
    let source = fs::canonicalize(source_package).map_err(|_| "DEV_PACKAGE_UNAVAILABLE")?;
    let python = fs::canonicalize(python_runtime).map_err(|_| "DEV_PYTHON_UNAVAILABLE")?;
    let roots = production_roots()?;
    require_distinct(&workspace, &source, &python, &roots)?;
    let hashes = source_artifacts(&source)?;
    if fs::metadata(python.join("python.exe"))
        .map_err(|_| "DEV_PYTHON_UNAVAILABLE")?
        .len()
        == 0
    {
        return Err("DEV_PYTHON_UNAVAILABLE".into());
    }
    hash_file(&python.join("python.exe"), MAX_PACKAGE_FILE)?;
    let id = package_id(&hashes)?;

    let catdesk = workspace.join(".catdesk");
    if !catdesk.exists() {
        fs::create_dir(&catdesk).map_err(|_| "DEV_RUNTIME_CREATE_CONFLICT")?;
    }
    fs::create_dir(catdesk.join("wake-bridge")).map_err(|_| "DEV_RUNTIME_CREATE_CONFLICT")?;
    write_json_new(
        &catdesk.join("wake-bridge/owner.json"),
        &json!({
            "schemaVersion":1,"owner":"independent_v1",
            "runtimeMode":"DEVELOPMENT_UNPROTECTED","purpose":PURPOSE
        }),
    )?;
    let dev = dev_dir(&workspace);
    fs::create_dir(&dev).map_err(|_| "DEV_RUNTIME_CREATE_CONFLICT")?;
    write_json_new(
        &dev.join("prepare-attempt.json"),
        &json!({"schemaVersion":1,"purpose":PURPOSE,"startedUtc":now()}),
    )?;
    let root = runtime_root(&workspace);
    let store = Store::open(&root)?;
    store.initialize()?;
    runtime::control(&store, "STOPPED")?;
    fs::create_dir(root.join("browser-profile")).map_err(|_| "DEV_RUNTIME_CREATE_CONFLICT")?;
    fs::create_dir(root.join("versions")).map_err(|_| "DEV_RUNTIME_CREATE_CONFLICT")?;
    let package = root.join("versions").join(&id);
    fs::create_dir(&package).map_err(|_| "DEV_RUNTIME_CREATE_CONFLICT")?;
    for name in PACKAGE_FILES {
        let copied = copy_new(&source.join(name), &package.join(name), MAX_PACKAGE_FILE)?;
        if hashes.get(name).map(String::as_str) != Some(copied.as_str()) {
            return Err("DEV_RUNTIME_COPY_MISMATCH".into());
        }
    }
    let manifest = json!({
        "schemaVersion":1,"version":VERSION,"protocolVersion":1,
        "installedUtc":now().to_string(),"artifacts":hashes,
        "acceptance":"DEVELOPMENT_NOT_ACTIVATED"
    });
    let manifest_path = package.join("manifest.json");
    write_json_new(&manifest_path, &manifest)?;
    let python_dest = root.join("runtime");
    copy_python_tree(&python, &python_dest, &mut CopyBudget::default())?;
    let pointer = json!({
        "schemaVersion":1,"directory":id,
        "hostSha256":hashes["CatDeskWakeHost.exe"],
        "binagotchySha256":hashes["CatDeskBinagotchy.exe"],
        "adapterSha256":hashes["adapter.py"],
        "wakeBridgeSha256":hashes["wake_bridge.py"]
    });
    let pointer_path = root.join("current.json");
    write_json_new(&pointer_path, &pointer)?;

    fs::create_dir(catdesk.join("autonomy")).map_err(|_| "DEV_RUNTIME_CREATE_CONFLICT")?;
    write_new(&catdesk.join("autonomy/review-inbox.json"), b"[]")?;
    runtime::set_review_source(&store, workspace.to_str().ok_or("DEV_WORKSPACE_INVALID")?)?;
    write_json_new(
        &root.join("activation.json"),
        &runtime::Activation {
            schema_version: 1,
            owner: "CatDeskWake".into(),
            migration_audit: "DEVELOPMENT_NEW_ROOT".into(),
            accept_after_utc: now(),
        },
    )?;

    let authority = Authority {
        schema_version: 1,
        purpose: PURPOSE.into(),
        workspace: workspace.to_string_lossy().into_owned(),
        workspace_identity: identity(&workspace, true)?,
        runtime_identity: identity(&root, true)?,
        package_id: id.clone(),
        artifacts: hashes.clone(),
        manifest_identity: identity(&manifest_path, false)?,
        manifest_sha256: hash_file(&manifest_path, 16 * 1024)?,
        pointer_identity: identity(&pointer_path, false)?,
        pointer_sha256: hash_file(&pointer_path, 4096)?,
        python_identity: identity(&python_dest.join("python.exe"), false)?,
        python_sha256: hash_file(&python_dest.join("python.exe"), MAX_PACKAGE_FILE)?,
        production_roots: roots,
        prepared_utc: now(),
    };
    write_json_new(&dev.join("authority.json"), &authority)?;
    root_for_workspace(&workspace)?.ok_or("DEV_RUNTIME_AUTHORITY_UNAVAILABLE")?;
    Ok(
        json!({"mode":"DEVELOPMENT_UNPROTECTED","status":"PREPARED_STOPPED","runtimeRoot":root,"packageId":id,"preparedUtc":authority.prepared_utc}),
    )
}

fn validate_update_attempt(
    workspace: &Path,
    root: &Path,
    attempt: &PackageUpdateAttempt,
    source_hashes: &BTreeMap<String, String>,
) -> Result<()> {
    let old = &attempt.old_authority;
    let new = &attempt.new_authority;
    if attempt.schema_version != 1
        || attempt.purpose != PACKAGE_UPDATE_PURPOSE
        || attempt.started_utc == 0
        || attempt.workspace != workspace.to_string_lossy()
        || attempt.runtime_identity != identity(root, true)?
        || old.workspace != attempt.workspace
        || old.runtime_identity != attempt.runtime_identity
        || old.workspace_identity != identity(workspace, true)?
        || !package_id_matches(&old.artifacts, &old.package_id)?
        || (attempt.restoration.is_none() && new.package_id != updated_package_id(source_hashes)?)
        || new.artifacts != *source_hashes
        || new.workspace != old.workspace
        || new.workspace_identity != old.workspace_identity
        || new.runtime_identity != old.runtime_identity
        || new.production_roots != old.production_roots
        || new.python_identity != old.python_identity
        || new.python_sha256 != old.python_sha256
        || new.prepared_utc != old.prepared_utc
        || new.purpose != old.purpose
        || new.schema_version != old.schema_version
        || !valid_update_stage_name(
            &attempt.pointer_stage_name,
            "current-update",
            &new.package_id,
        )
        || !valid_update_stage_name(
            &attempt.authority_stage_name,
            "authority-update",
            &new.package_id,
        )
        || [
            &attempt.old_authority_sha256,
            &attempt.new_authority_sha256,
            &new.pointer_sha256,
            &new.manifest_sha256,
        ]
        .into_iter()
        .any(|hash| {
            hash.len() != 64
                || !hash
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
    {
        return Err("DEV_PACKAGE_UPDATE_ATTEMPT_INVALID".into());
    }
    attempt_pointer(root, attempt, source_hashes)?;
    let (manifest_identity, manifest_sha256) =
        verify_update_package(root, &new.package_id, source_hashes)?;
    if new.manifest_identity != manifest_identity || new.manifest_sha256 != manifest_sha256 {
        return Err("DEV_PACKAGE_UPDATE_MANIFEST_MISMATCH".into());
    }
    Ok(())
}

fn attempt_pointer(
    root: &Path,
    attempt: &PackageUpdateAttempt,
    hashes: &BTreeMap<String, String>,
) -> Result<Value> {
    let Some(previous) = &attempt.restoration else {
        return update_pointer(
            &attempt.old_authority,
            &attempt.new_authority.package_id,
            hashes,
            attempt.old_pointer_json.as_deref(),
            attempt.retained_verification.as_ref(),
        );
    };
    let old_json = attempt
        .old_pointer_json
        .as_deref()
        .ok_or("DEV_RELEASE_RESTORE_BINDING_INVALID")?;
    if old_json.len() > 4096
        || format!("{:x}", Sha256::digest(old_json.as_bytes()))
            != attempt.old_authority.pointer_sha256
        || attempt.retained_verification.is_some()
        || previous.package_id == attempt.old_authority.package_id
        || attempt.new_authority.package_id != previous.package_id
        || attempt.new_authority.artifacts != previous.artifacts
        || attempt.new_authority.manifest_identity != previous.manifest_identity
        || attempt.new_authority.manifest_sha256 != previous.manifest_sha256
    {
        return Err("DEV_RELEASE_RESTORE_BINDING_INVALID".into());
    }
    let old_pointer: Value =
        serde_json::from_str(old_json).map_err(|_| "DEV_RELEASE_RESTORE_BINDING_INVALID")?;
    if old_pointer.get("previousVerifiedRelease")
        != Some(&serde_json::to_value(previous).map_err(|_| "DEV_RUNTIME_ENCODING_INVALID")?)
    {
        return Err("DEV_RELEASE_RESTORE_BINDING_INVALID".into());
    }
    validate_retained_release(root, previous)?;
    // The failed current release must never become its own recovery fallback.
    Ok(package_pointer(&previous.package_id, hashes))
}

fn package_update_result(old: &Authority, new: &Authority, status: &str) -> Value {
    json!({"mode":"DEVELOPMENT_UNPROTECTED","status":status,
        "oldPackageId":old.package_id,"packageId":new.package_id,
        "browserProfilePreserved":true,"runtimeStatePreserved":true,
        "hostStarted":false,"browserAction":"NONE"})
}

fn complete_package_update(
    workspace: &Path,
    root: &Path,
    attempt: &PackageUpdateAttempt,
    source_hashes: &BTreeMap<String, String>,
) -> Result<Value> {
    validate_update_attempt(workspace, root, attempt, source_hashes)?;
    let dev = dev_dir(workspace);
    let pointer_path = root.join("current.json");
    let authority_path = dev.join("authority.json");
    let pointer_stage = root.join(&attempt.pointer_stage_name);
    let authority_stage = dev.join(&attempt.authority_stage_name);

    let authority: Authority = read_json(&authority_path, 32 * 1024)?;
    let authority_hash = hash_file(&authority_path, 32 * 1024)?;
    let old_authority =
        authority == attempt.old_authority && authority_hash == attempt.old_authority_sha256;
    let new_authority =
        authority == attempt.new_authority && authority_hash == attempt.new_authority_sha256;
    if !old_authority && !new_authority {
        return Err("DEV_PACKAGE_UPDATE_AUTHORITY_DIVERGED".into());
    }
    let pointer_hash = hash_file(&pointer_path, 4096)?;
    let pointer_identity = identity(&pointer_path, false)?;
    let old_pointer = pointer_hash == attempt.old_authority.pointer_sha256
        && pointer_identity == attempt.old_authority.pointer_identity;
    let new_pointer = pointer_hash == attempt.new_authority.pointer_sha256
        && pointer_identity == attempt.new_authority.pointer_identity;
    if !old_pointer && !new_pointer || new_authority && !new_pointer {
        return Err("DEV_PACKAGE_UPDATE_POINTER_DIVERGED".into());
    }

    if old_authority && old_pointer {
        if hash_file(&pointer_stage, 4096)? != attempt.new_authority.pointer_sha256
            || identity(&pointer_stage, false)? != attempt.new_authority.pointer_identity
            || read_json::<Value>(&pointer_stage, 4096)?
                != attempt_pointer(root, attempt, source_hashes)?
        {
            return Err("DEV_PACKAGE_UPDATE_POINTER_STAGE_INVALID".into());
        }
        if validated_authority_options(workspace, true, attempt.restoration.is_some())?
            .as_ref()
            .map(|(_, value)| value)
            != Some(&attempt.old_authority)
        {
            return Err("DEV_PACKAGE_UPDATE_OLD_AUTHORITY_INVALID".into());
        }
        replace_update_file(&pointer_stage, &pointer_path)?;
    }
    // The pointer-first window is intentional. A durable attempt keeps normal
    // root selection closed until this exact authority catches up on retry.
    if old_authority {
        if hash_file(&authority_stage, 32 * 1024)? != attempt.new_authority_sha256
            || read_json::<Authority>(&authority_stage, 32 * 1024)? != attempt.new_authority
        {
            return Err("DEV_PACKAGE_UPDATE_AUTHORITY_STAGE_INVALID".into());
        }
        if hash_file(&pointer_path, 4096)? != attempt.new_authority.pointer_sha256
            || identity(&pointer_path, false)? != attempt.new_authority.pointer_identity
        {
            return Err("DEV_PACKAGE_UPDATE_POINTER_DIVERGED".into());
        }
        replace_update_file(&authority_stage, &authority_path)?;
    }
    if validated_authority_mode(workspace, true)?
        .as_ref()
        .map(|(_, value)| value)
        != Some(&attempt.new_authority)
    {
        return Err("DEV_PACKAGE_UPDATE_READBACK_FAILED".into());
    }
    fs::remove_file(dev.join(PACKAGE_UPDATE_ATTEMPT))
        .map_err(|_| "DEV_PACKAGE_UPDATE_MARKER_REMAINS")?;
    if validated_authority(workspace)?
        .as_ref()
        .map(|(_, value)| value)
        != Some(&attempt.new_authority)
    {
        return Err("DEV_PACKAGE_UPDATE_READBACK_FAILED".into());
    }
    Ok(package_update_result(
        &attempt.old_authority,
        &attempt.new_authority,
        if attempt.restoration.is_some() {
            "PACKAGE_PREVIOUS_RESTORED_STOPPED"
        } else {
            "PACKAGE_UPDATED_STOPPED"
        },
    ))
}

/// Replace only this isolated development runtime's immutable package. The
/// browser profile, Python runtime, Wake queues and project registry are never
/// copied, reset or selected from a different workspace.
pub fn update_package(workspace: &Path, source_package: &Path) -> Result<Value> {
    let workspace = canonical_workspace(workspace)?;
    let dev = dev_dir(&workspace);
    let root = runtime_root(&workspace);
    plain(&dev, true)?;
    plain(&root, true)?;
    let _backend = backend_lock(&workspace)?;
    plain(source_package, true)?;
    let source = fs::canonicalize(source_package).map_err(|_| "DEV_PACKAGE_UNAVAILABLE")?;
    if source.starts_with(&workspace) || workspace.starts_with(&source) {
        return Err("DEV_SOURCE_OVERLAP".into());
    }
    let attempt_path = dev.join(PACKAGE_UPDATE_ATTEMPT);
    if !present(&attempt_path)? {
        validated_authority(&workspace)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    }
    let store = Store::open(&root)?;
    let _host = store.host_lock()?;
    let _browser = runtime::browser_session_lock(&root, Duration::ZERO)?;
    let control: runtime::Control = read(&root.join("control.json"))?;
    if control.schema_version != 1 || control.desired != "STOPPED" {
        return Err("DEV_PACKAGE_UPDATE_HOST_NOT_STOPPED".into());
    }
    let hashes = source_artifacts(&source)?;
    if present(&attempt_path)? {
        let attempt: PackageUpdateAttempt = read_json(&attempt_path, 32 * 1024)?;
        return complete_package_update(&workspace, &root, &attempt, &hashes);
    }
    let (_, old) = validated_authority(&workspace)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    if old.artifacts == hashes {
        return Ok(package_update_result(&old, &old, "PACKAGE_ALREADY_CURRENT"));
    }
    let attempt = stage_package_update_attempt(&workspace, &root, &source, &old, &hashes)?;
    complete_package_update(&workspace, &root, &attempt, &hashes)
}

/// Finish a journaled selection, or restore the exact recorded working predecessor
/// when current package files fail validation. Metadata, workspace ownership and
/// all predecessor bytes remain strict. No host or browser is launched.
pub fn recover_package(workspace: &Path) -> Result<Value> {
    let workspace = canonical_workspace(workspace)?;
    let dev = dev_dir(&workspace);
    let root = runtime_root(&workspace);
    plain(&dev, true)?;
    plain(&root, true)?;
    let _backend = backend_lock(&workspace)?;
    let attempt_path = dev.join(PACKAGE_UPDATE_ATTEMPT);
    if !present(&attempt_path)? {
        validated_authority_options(&workspace, false, true)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    }
    let store = Store::open(&root)?;
    let _host = store.host_lock()?;
    let _browser = runtime::browser_session_lock(&root, Duration::ZERO)?;
    let control: runtime::Control = read(&root.join("control.json"))?;
    if control.schema_version != 1 || control.desired != "STOPPED" {
        return Err("DEV_PACKAGE_UPDATE_HOST_NOT_STOPPED".into());
    }
    if present(&attempt_path)? {
        let attempt: PackageUpdateAttempt = read_json(&attempt_path, 32 * 1024)?;
        return complete_package_update(
            &workspace,
            &root,
            &attempt,
            &attempt.new_authority.artifacts,
        );
    }
    if let Ok(Some((_, current))) = validated_authority(&workspace) {
        return Ok(package_update_result(
            &current,
            &current,
            "PACKAGE_CURRENT_VERIFIED_STOPPED",
        ));
    }
    let (_, old) =
        validated_authority_options(&workspace, false, true)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    let previous = read_retained_release(&root)
        .map_err(|_| "DEV_RELEASE_RECONSTRUCTION_REQUIRED_PREDECESSOR_INVALID")?
        .ok_or("DEV_RELEASE_RECONSTRUCTION_REQUIRED_NO_VERIFIED_PREDECESSOR")?;
    let attempt = stage_previous_package_attempt(&workspace, &root, &old, previous)?;
    complete_package_update(
        &workspace,
        &root,
        &attempt,
        &attempt.new_authority.artifacts,
    )
}

fn stage_previous_package_attempt(
    workspace: &Path,
    root: &Path,
    old: &Authority,
    previous: RetainedRelease,
) -> Result<PackageUpdateAttempt> {
    validate_retained_release(root, &previous)?;
    let dev = dev_dir(workspace);
    let id = &previous.package_id;
    let pointer_stage_name = update_stage_name("current-update", id)?;
    let pointer_stage = root.join(&pointer_stage_name);
    write_json_new(&pointer_stage, &package_pointer(id, &previous.artifacts))?;
    let mut new = old.clone();
    new.package_id = id.clone();
    new.artifacts = previous.artifacts.clone();
    new.manifest_identity = previous.manifest_identity.clone();
    new.manifest_sha256 = previous.manifest_sha256.clone();
    new.pointer_identity = identity(&pointer_stage, false)?;
    new.pointer_sha256 = hash_file(&pointer_stage, 4096)?;
    let authority_stage_name = update_stage_name("authority-update", id)?;
    let authority_stage = dev.join(&authority_stage_name);
    write_json_new(&authority_stage, &new)?;
    let attempt = PackageUpdateAttempt {
        schema_version: 1,
        purpose: PACKAGE_UPDATE_PURPOSE.into(),
        workspace: workspace.to_string_lossy().into_owned(),
        runtime_identity: identity(root, true)?,
        old_authority: old.clone(),
        old_authority_sha256: hash_file(&dev.join("authority.json"), 32 * 1024)?,
        new_authority: new,
        new_authority_sha256: hash_file(&authority_stage, 32 * 1024)?,
        pointer_stage_name,
        authority_stage_name,
        started_utc: now(),
        old_pointer_json: Some(
            fs::read_to_string(root.join("current.json"))
                .map_err(|_| "DEV_PACKAGE_UPDATE_OLD_POINTER_INVALID")?,
        ),
        retained_verification: None,
        restoration: Some(previous),
    };
    validate_update_attempt(workspace, root, &attempt, &attempt.new_authority.artifacts)?;
    write_json_new(&dev.join(PACKAGE_UPDATE_ATTEMPT), &attempt)?;
    Ok(attempt)
}

fn stage_package_update_attempt(
    workspace: &Path,
    root: &Path,
    source: &Path,
    old: &Authority,
    hashes: &BTreeMap<String, String>,
) -> Result<PackageUpdateAttempt> {
    let dev = dev_dir(workspace);
    let attempt_path = dev.join(PACKAGE_UPDATE_ATTEMPT);
    let id = updated_package_id(hashes)?;
    let (manifest_identity, manifest_sha256) = stage_update_package(root, source, &id, hashes)?;
    let pointer_stage_name = update_stage_name("current-update", &id)?;
    let pointer_stage = root.join(&pointer_stage_name);
    let old_pointer_json = fs::read_to_string(root.join("current.json"))
        .map_err(|_| "DEV_PACKAGE_UPDATE_OLD_POINTER_INVALID")?;
    let retained_verification = read_release_verification(root, old).ok().flatten();
    write_json_new(
        &pointer_stage,
        &update_pointer(
            old,
            &id,
            hashes,
            Some(&old_pointer_json),
            retained_verification.as_ref(),
        )?,
    )?;
    let mut new = old.clone();
    new.package_id = id.clone();
    new.artifacts = hashes.clone();
    new.manifest_identity = manifest_identity;
    new.manifest_sha256 = manifest_sha256;
    new.pointer_identity = identity(&pointer_stage, false)?;
    new.pointer_sha256 = hash_file(&pointer_stage, 4096)?;
    let authority_stage_name = update_stage_name("authority-update", &id)?;
    let authority_stage = dev.join(&authority_stage_name);
    write_json_new(&authority_stage, &new)?;
    let attempt = PackageUpdateAttempt {
        schema_version: 1,
        purpose: PACKAGE_UPDATE_PURPOSE.into(),
        workspace: workspace.to_string_lossy().into_owned(),
        runtime_identity: identity(root, true)?,
        old_authority: old.clone(),
        old_authority_sha256: hash_file(&dev.join("authority.json"), 32 * 1024)?,
        new_authority: new,
        new_authority_sha256: hash_file(&authority_stage, 32 * 1024)?,
        pointer_stage_name,
        authority_stage_name,
        started_utc: now(),
        old_pointer_json: Some(old_pointer_json),
        retained_verification,
        restoration: None,
    };
    validate_update_attempt(workspace, root, &attempt, hashes)?;
    write_json_new(&attempt_path, &attempt)?;
    Ok(attempt)
}

fn present(path: &Path) -> Result<bool> {
    path.try_exists()
        .map_err(|_| "DEV_RUNTIME_AUTHORITY_UNAVAILABLE".into())
}

fn validated_authority(workspace: &Path) -> Result<Option<(PathBuf, Authority)>> {
    validated_authority_mode(workspace, false)
}

fn validated_authority_mode(
    workspace: &Path,
    allow_package_update: bool,
) -> Result<Option<(PathBuf, Authority)>> {
    validated_authority_options(workspace, allow_package_update, false)
}

// Only predecessor restoration may retire unavailable package files. All current
// metadata, root identities, Python and ownership checks are still required.
fn validated_authority_options(
    workspace: &Path,
    allow_package_update: bool,
    allow_retiring_package_files: bool,
) -> Result<Option<(PathBuf, Authority)>> {
    let workspace = canonical_workspace(workspace)?;
    let dev = dev_dir(&workspace);
    if !present(&dev)? {
        let selector = workspace.join(".catdesk/wake-bridge/owner.json");
        if present(&selector)? {
            let value: Value = read_json(&selector, 4096)?;
            if value.get("runtimeMode").and_then(Value::as_str) == Some("DEVELOPMENT_UNPROTECTED")
                || value.get("purpose").and_then(Value::as_str) == Some(PURPOSE)
            {
                return Err("DEV_RUNTIME_AUTHORITY_DISAPPEARED".into());
            }
        }
        return Ok(None);
    }
    plain(&dev, true)?;
    if present(&dev.join("login-active.json"))? {
        return Err("DEV_LOGIN_CLEANUP_UNVERIFIED".into());
    }
    if !allow_package_update && present(&dev.join(PACKAGE_UPDATE_ATTEMPT))? {
        return Err("DEV_PACKAGE_UPDATE_INCOMPLETE".into());
    }
    let attempt = dev.join("prepare-attempt.json");
    let authority_path = dev.join("authority.json");
    if !present(&attempt)? || !present(&authority_path)? {
        return Err("DEV_RUNTIME_PREPARE_INCOMPLETE".into());
    }
    let attempt: Value = read_json(&attempt, 4096)?;
    if attempt.get("purpose").and_then(Value::as_str) != Some(PURPOSE)
        || attempt.get("schemaVersion").and_then(Value::as_u64) != Some(1)
    {
        return Err("DEV_RUNTIME_PREPARE_INVALID".into());
    }
    let authority: Authority = read_json(&authority_path, 32 * 1024)?;
    if authority.schema_version != 1
        || authority.purpose != PURPOSE
        || authority.workspace != workspace.to_string_lossy()
        || authority.workspace_identity != identity(&workspace, true)?
    {
        return Err("DEV_RUNTIME_AUTHORITY_MISMATCH".into());
    }
    let known = known_production_paths()?;
    if authority.production_roots.len() != known.len()
        || authority
            .production_roots
            .iter()
            .zip(&known)
            .any(|(recorded, expected)| Path::new(&recorded.path) != expected)
    {
        return Err("DEV_PRODUCTION_ROOT_SET_MISMATCH".into());
    }
    let root = runtime_root(&workspace);
    if authority.runtime_identity != identity(&root, true)? {
        return Err("DEV_RUNTIME_ROOT_MISMATCH".into());
    }
    let package = root.join("versions").join(&authority.package_id);
    if !package_id_matches(&authority.artifacts, &authority.package_id)? {
        return Err("DEV_RUNTIME_PACKAGE_MISMATCH".into());
    }
    plain(&package, true)?;
    for name in PACKAGE_FILES {
        if !allow_retiring_package_files
            && authority.artifacts.get(name).map(String::as_str)
                != Some(hash_file(&package.join(name), MAX_PACKAGE_FILE)?.as_str())
        {
            return Err("DEV_RUNTIME_PACKAGE_MISMATCH".into());
        }
    }
    let manifest_path = package.join("manifest.json");
    if authority.manifest_identity != identity(&manifest_path, false)?
        || authority.manifest_sha256 != hash_file(&manifest_path, 16 * 1024)?
    {
        return Err("DEV_RUNTIME_MANIFEST_MISMATCH".into());
    }
    let manifest: Value = read_json(&manifest_path, 16 * 1024)?;
    if manifest.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || manifest.get("version").and_then(Value::as_str) != Some(VERSION)
        || manifest.get("protocolVersion").and_then(Value::as_u64) != Some(1)
        || manifest.get("acceptance").and_then(Value::as_str) != Some("DEVELOPMENT_NOT_ACTIVATED")
        || manifest.get("artifacts")
            != Some(
                &serde_json::to_value(&authority.artifacts)
                    .map_err(|_| "DEV_RUNTIME_MANIFEST_INVALID")?,
            )
    {
        return Err("DEV_RUNTIME_MANIFEST_INVALID".into());
    }
    let pointer_path = root.join("current.json");
    if authority.pointer_identity != identity(&pointer_path, false)?
        || authority.pointer_sha256 != hash_file(&pointer_path, 4096)?
    {
        return Err("DEV_RUNTIME_POINTER_MISMATCH".into());
    }
    let pointer: Value = read_json(&pointer_path, 4096)?;
    if pointer.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || pointer.get("directory").and_then(Value::as_str) != Some(authority.package_id.as_str())
        || [
            ("hostSha256", "CatDeskWakeHost.exe"),
            ("binagotchySha256", "CatDeskBinagotchy.exe"),
            ("adapterSha256", "adapter.py"),
            ("wakeBridgeSha256", "wake_bridge.py"),
        ]
        .into_iter()
        .any(|(field, name)| {
            pointer.get(field).and_then(Value::as_str)
                != authority.artifacts.get(name).map(String::as_str)
        })
    {
        return Err("DEV_RUNTIME_POINTER_INVALID".into());
    }
    let python = root.join("runtime").join("python.exe");
    if authority.python_identity != identity(&python, false)?
        || authority.python_sha256 != hash_file(&python, MAX_PACKAGE_FILE)?
    {
        return Err("DEV_RUNTIME_PYTHON_MISMATCH".into());
    }
    let selector: Value = read_json(&workspace.join(".catdesk/wake-bridge/owner.json"), 4096)?;
    if selector.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || selector.get("owner").and_then(Value::as_str) != Some("independent_v1")
        || selector.get("runtimeMode").and_then(Value::as_str) != Some("DEVELOPMENT_UNPROTECTED")
        || selector.get("purpose").and_then(Value::as_str) != Some(PURPOSE)
    {
        return Err("DEV_RUNTIME_SELECTOR_INVALID".into());
    }
    let activation: runtime::Activation = read_json(&root.join("activation.json"), 4096)?;
    if activation.schema_version != 1
        || activation.owner != "CatDeskWake"
        || activation.migration_audit != "DEVELOPMENT_NEW_ROOT"
        || activation.accept_after_utc == 0
    {
        return Err("DEV_RUNTIME_ACTIVATION_INVALID".into());
    }
    for item in &authority.production_roots {
        let path = Path::new(&item.path);
        let observed = match fs::symlink_metadata(path) {
            Ok(_) => Some(identity(path, true)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err("DEV_PRODUCTION_ROOT_UNAVAILABLE".into()),
        };
        if item.identity.is_some() && item.identity != observed
            || observed.as_ref() == Some(&authority.runtime_identity)
        {
            return Err("DEV_PRODUCTION_ROOT_MISMATCH".into());
        }
    }
    Ok(Some((root, authority)))
}

/// A missing authority is the only case in which CatDesk may select its
/// existing installed Wake root. A partial or altered development authority
/// always refuses that fallback.
pub fn root_for_workspace(workspace: &Path) -> Result<Option<PathBuf>> {
    Ok(validated_authority(workspace)?.map(|(root, _)| root))
}

fn backend_lock(workspace: &Path) -> Result<File> {
    let dev = dev_dir(workspace);
    plain(&dev, true)?;
    let path = dev.join("backend.lock");
    if present(&path)? {
        plain(&path, false)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|_| "DEV_BACKEND_LEASE_UNAVAILABLE")?;
    file.try_lock().map_err(|_| "DEV_BACKEND_ALREADY_RUNNING")?;
    Ok(file)
}

/// Retains this workspace's backend lease and read-only image/selection pins.
/// Healthy backend startup is independent of Wake/browser ownership. Only an
/// actual package recovery uses the existing STOPPED/exclusive recovery gates.
pub struct BackendSelection {
    pub workspace: PathBuf,
    pub executable: PathBuf,
    pub package_id: String,
    pub app_sha256: String,
    pub recovery: String,
    _lease: File,
    _pins: Vec<File>,
    authority: Authority,
}

pub fn select_backend(workspace: &Path) -> Result<BackendSelection> {
    let workspace = canonical_workspace(workspace)?;
    let recovery = match validated_authority(&workspace) {
        Ok(Some(_)) => "CURRENT_PACKAGE_VERIFIED".to_owned(),
        Ok(None) => return Err("DEV_RUNTIME_NOT_PREPARED".into()),
        Err(_) => recover_package(&workspace)?["status"]
            .as_str()
            .ok_or("DEV_BACKEND_RECOVERY_STATUS_INVALID")?
            .to_owned(),
    };
    let lease = backend_lock(&workspace)?;
    pin_backend(workspace, lease, recovery)
}

fn pin_backend(workspace: PathBuf, lease: File, recovery: String) -> Result<BackendSelection> {
    use std::os::windows::fs::OpenOptionsExt;
    let (root, authority) = validated_authority(&workspace)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    let executable = root
        .join("versions")
        .join(&authority.package_id)
        .join("CatDeskBinagotchy.exe");
    let mut pins = Vec::new();
    for path in [
        &executable,
        &root.join("current.json"),
        &dev_dir(&workspace).join("authority.json"),
    ] {
        plain(path, false)?;
        pins.push(
            OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(path)
                .map_err(|_| "DEV_BACKEND_SELECTION_PIN_UNAVAILABLE")?,
        );
    }
    if validated_authority(&workspace)?
        .as_ref()
        .map(|(_, value)| value)
        != Some(&authority)
    {
        return Err("DEV_BACKEND_SELECTION_CHANGED".into());
    }
    Ok(BackendSelection {
        workspace,
        executable,
        package_id: authority.package_id.clone(),
        app_sha256: authority.artifacts["CatDeskBinagotchy.exe"].clone(),
        recovery,
        _lease: lease,
        _pins: pins,
        authority,
    })
}

impl BackendSelection {
    /// The caller must first prove that its failed pre-ready child tree is gone.
    /// Retain the existing backend lease across rollback; display fields cannot
    /// choose a different workspace/package or substitute an authority snapshot.
    pub fn restore_after_startup_failure(mut self) -> Result<Self> {
        let workspace = canonical_workspace(Path::new(&self.authority.workspace))?;
        let (root, current) = validated_authority(&workspace)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?;
        if current != self.authority {
            return Err("DEV_BACKEND_SELECTION_CHANGED".into());
        }
        let store = Store::open(&root)?;
        let _host = store.host_lock()?;
        let _browser = runtime::browser_session_lock(&root, Duration::ZERO)?;
        let control: runtime::Control = read(&root.join("control.json"))?;
        if control.schema_version != 1 || control.desired != "STOPPED" {
            return Err("DEV_PACKAGE_UPDATE_HOST_NOT_STOPPED".into());
        }
        let previous = read_retained_release(&root)
            .map_err(|_| "DEV_RELEASE_RECONSTRUCTION_REQUIRED_PREDECESSOR_INVALID")?
            .ok_or("DEV_RELEASE_RECONSTRUCTION_REQUIRED_NO_VERIFIED_PREDECESSOR")?;
        // No mapped child remains. Release only the read pins, retaining the
        // backend lease until the restored selection has its new pins.
        self._pins.clear();
        let attempt = stage_previous_package_attempt(&workspace, &root, &current, previous)?;
        complete_package_update(
            &workspace,
            &root,
            &attempt,
            &attempt.new_authority.artifacts,
        )?;
        pin_backend(
            workspace,
            self._lease,
            "PACKAGE_PREVIOUS_RESTORED_STOPPED".into(),
        )
    }
}

fn conversation_id(url: &str) -> Result<&str> {
    protocol::validate_target(url)?;
    Ok(url
        .rsplit('/')
        .next()
        .ok_or("TARGET_INVALID_URL")?
        .strip_prefix("WEB:")
        .unwrap_or_else(|| url.rsplit('/').next().unwrap()))
}

fn validate_against_production(authority: &Authority, target_url: &str) -> Result<()> {
    let wanted = conversation_id(target_url)?;
    for item in &authority.production_roots {
        let root = Path::new(&item.path);
        let observed = match fs::symlink_metadata(root) {
            Ok(_) => Some(identity(root, true)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err("DEV_PRODUCTION_ROOT_UNAVAILABLE".into()),
        };
        if item.identity.is_some() && item.identity != observed {
            return Err("DEV_PRODUCTION_ROOT_MISMATCH".into());
        }
        if observed.is_none() {
            continue;
        }
        let config: Config = read_json(&root.join("config.json"), 1024 * 1024)
            .map_err(|_| "DEV_PRODUCTION_TARGETS_UNAVAILABLE")?;
        config
            .validate()
            .map_err(|_| "DEV_PRODUCTION_TARGETS_INVALID")?;
        for target in config
            .targets
            .values()
            .chain(config.history.iter().map(|entry| &entry.target))
            .chain(
                config
                    .retired_targets
                    .values()
                    .filter_map(|entry| entry.last_target.as_ref()),
            )
        {
            target
                .validate()
                .map_err(|_| "DEV_PRODUCTION_TARGETS_INVALID")?;
            if conversation_id(&target.url)? == wanted {
                return Err("DEV_TARGET_SHARED_WITH_PRODUCTION".into());
            }
        }
    }
    Ok(())
}

/// Read-only check for a new controlled conversation. This cannot prove that
/// other user profiles or unlisted Wake installations have no matching URL.
pub fn validate_target_isolation(workspace: &Path, target_url: &str) -> Result<()> {
    protocol::validate_target(target_url)?;
    if let Some((_, authority)) = validated_authority(workspace)? {
        validate_against_production(&authority, target_url)?;
    }
    Ok(())
}

/// Check the exact Wake root already selected by a caller. In particular,
/// deletion of the development authority cannot make a selected dev operation
/// silently fall back to the installed root.
pub fn validate_selected_target_isolation(
    workspace: &Path,
    selected_root: &Path,
    target_url: &str,
) -> Result<()> {
    protocol::validate_target(target_url)?;
    let expected_dev_root = runtime_root(&canonical_workspace(workspace)?);
    match validated_authority(workspace)? {
        Some((root, authority)) => {
            if identity(selected_root, true)? != identity(&root, true)? {
                return Err("DEV_RUNTIME_SELECTED_ROOT_MISMATCH".into());
            }
            validate_against_production(&authority, target_url)
        }
        None if selected_root == expected_dev_root => {
            Err("DEV_RUNTIME_AUTHORITY_UNAVAILABLE".into())
        }
        None => Ok(()),
    }
}

/// WakeHost calls this at the final BEFORE_SUBMIT boundary, after any time
/// spent waiting in the queue or adapter. Installed roots are unaffected.
pub fn validate_target_isolation_for_store(store_root: &Path, target_url: &str) -> Result<()> {
    let parent = store_root.parent().ok_or("DEV_RUNTIME_ROOT_INVALID")?;
    if parent.file_name() != Some(OsStr::new(DEV_DIR))
        || store_root.file_name() != Some(OsStr::new(ROOT_DIR))
        || parent.parent().and_then(Path::file_name) != Some(OsStr::new(".catdesk"))
    {
        return Ok(());
    }
    let workspace = parent
        .parent()
        .and_then(Path::parent)
        .ok_or("DEV_RUNTIME_ROOT_INVALID")?;
    validate_selected_target_isolation(workspace, store_root, target_url)
}

pub fn status(workspace: &Path) -> Result<Value> {
    let Some((root, authority)) = validated_authority(workspace)? else {
        return Ok(json!({"mode":"NOT_CONFIGURED","status":"ABSENT"}));
    };
    let store = Store::open(&root)?;
    let live = runtime::status(&store)?;
    let current_verification = match read_release_verification(&root, &authority) {
        Ok(Some(record)) => json!({"status":"OPERATOR_RECORDED",
            "sourceCommit":record.source_commit,
            "workingHealthObservedUtc":record.working_health_observed_utc}),
        Ok(None) => json!({"status":"NOT_RECORDED"}),
        Err(_) => json!({"status":"INVALID_OR_PACKAGE_CHANGED"}),
    };
    let previous_verification = retained_predecessor_status(&root);
    Ok(
        json!({"mode":"DEVELOPMENT_UNPROTECTED","runtimeRoot":root,"packageId":authority.package_id,
        "hostObservation":live.host,"observedPid":live.pid,"liveOwnerVerified":false,
        "browser":live.browser,"queueDepth":live.queue_depth,
        "targets":live.targets.keys().collect::<Vec<_>>(),"attention":live.attention,
        "currentVerification":current_verification,"previousVerifiedRelease":previous_verification}),
    )
}

fn read_retained_release(root: &Path) -> Result<Option<RetainedRelease>> {
    let pointer: Value = read_json(&root.join("current.json"), 4096)?;
    let Some(value) = pointer.get("previousVerifiedRelease") else {
        return Ok(None);
    };
    let previous: RetainedRelease = serde_json::from_value(value.clone())
        .map_err(|_| "DEV_RELEASE_PREDECESSOR_RECORD_INVALID")?;
    validate_retained_release(root, &previous)?;
    Ok(Some(previous))
}

fn validate_retained_release(root: &Path, previous: &RetainedRelease) -> Result<()> {
    if !package_id_matches(&previous.artifacts, &previous.package_id)? {
        return Err("DEV_RELEASE_PREDECESSOR_RECORD_INVALID".into());
    }
    validate_verification_binding(
        &previous.verification,
        &previous.package_id,
        &previous.manifest_sha256,
        &previous.artifacts,
    )?;
    let package = root.join("versions").join(&previous.package_id);
    plain(&package, true)?;
    let manifest = package.join("manifest.json");
    if identity(&manifest, false)? != previous.manifest_identity
        || hash_file(&manifest, 16 * 1024)? != previous.manifest_sha256
    {
        return Err("DEV_RELEASE_VERIFICATION_PACKAGE_CHANGED".into());
    }
    for name in PACKAGE_FILES {
        if previous.artifacts.get(name) != Some(&hash_file(&package.join(name), MAX_PACKAGE_FILE)?)
        {
            return Err("DEV_RELEASE_VERIFICATION_PACKAGE_CHANGED".into());
        }
    }
    Ok(())
}

fn retained_predecessor_status(root: &Path) -> Value {
    match read_retained_release(root) {
        Ok(Some(previous)) => json!({"status":"OPERATOR_RECORDED_PACKAGE_RETAINED",
            "packageId":previous.package_id,"sourceCommit":previous.verification.source_commit,
            "workingHealthObservedUtc":previous.verification.working_health_observed_utc}),
        Ok(None) => json!({"status":"NOT_RECORDED"}),
        Err(_) => json!({"status":"INVALID_OR_PACKAGE_CHANGED"}),
    }
}

fn validated_launch(workspace: &Path) -> Result<(Store, PathBuf)> {
    let (root, authority) = validated_authority(workspace)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    let store = Store::open(&root)?;
    if store.config()?.targets.is_empty() {
        return Err("DEV_RUNTIME_NO_TARGETS".into());
    }
    for target in store.config()?.targets.values() {
        validate_against_production(&authority, &target.url)?;
    }
    let executable = root
        .join("versions")
        .join(&authority.package_id)
        .join("CatDeskWakeHost.exe");
    Ok((store, executable))
}

pub fn start(workspace: &Path) -> Result<Value> {
    let (store, executable) = validated_launch(workspace)?;
    if let Err(error) = store.host_lock() {
        if error == "HOST_ALREADY_RUNNING" {
            let live = runtime::status(&store)?;
            return Ok(
                json!({"mode":"DEVELOPMENT_UNPROTECTED","status":"OWNER_PRESENT","host":live.host,"pid":live.pid}),
            );
        }
        return Err(error);
    }
    // A child of an outer kill-on-close Job is not a durable host after the
    // operator exits. The explicit dev start refuses that launch context.
    let mut in_job = 0;
    if unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) } == 0 {
        return Err("DEV_RUNTIME_LAUNCH_CONTEXT_UNAVAILABLE".into());
    }
    if in_job != 0 {
        return Err("DEV_RUNTIME_OUTER_JOB_REFUSED".into());
    }
    let previous: Value = read_json(&store.root().join("control.json"), 4096)?;
    let previous_desired = previous
        .get("desired")
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "RUNNING" | "PAUSED" | "STOPPED"))
        .ok_or("DEV_RUNTIME_CONTROL_INVALID")?
        .to_owned();
    runtime::control(&store, "RUNNING")?;
    let mut command = Command::new(&executable);
    command
        .arg("--dev-host")
        .arg(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            if let Ok(lease) = store.host_lock() {
                drop(lease);
                runtime::control(&store, &previous_desired)?;
            }
            return Err("DEV_RUNTIME_START_FAILED".into());
        }
    };
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(100));
        if child
            .try_wait()
            .map_err(|_| "DEV_RUNTIME_CHILD_STATUS_UNAVAILABLE")?
            .is_some()
        {
            return Err("DEV_RUNTIME_HOST_EXITED".into());
        }
        if let Ok(live) = runtime::status(&store)
            && live.host == "RUNNING"
            && live.pid == child.id()
        {
            return Ok(
                json!({"mode":"DEVELOPMENT_UNPROTECTED","status":"START_PENDING","observedPid":live.pid}),
            );
        }
    }
    Ok(json!({"mode":"DEVELOPMENT_UNPROTECTED","status":"START_PENDING"}))
}

/// Keep the exact WakeHost child attached to this operator invocation. This
/// mode is usable from an outer Job without trying to break away from it;
/// another session may issue stop, then this call returns the child exit.
/// A private kill-on-close Job owns all descendants before runtime startup.
pub fn run_foreground(workspace: &Path) -> Result<Value> {
    run_foreground_inner(workspace, None)
}

pub fn run_foreground_bounded(workspace: &Path, maximum_seconds: u64) -> Result<Value> {
    if !(1..=900).contains(&maximum_seconds) {
        return Err("DEV_RUNTIME_FOREGROUND_BUDGET_INVALID".into());
    }
    run_foreground_inner(workspace, Some(maximum_seconds))
}

fn run_foreground_inner(workspace: &Path, maximum_seconds: Option<u64>) -> Result<Value> {
    let (store, executable) = validated_launch(workspace)?;
    let host = fs::read(&executable).map_err(|_| "DEV_RUNTIME_EXECUTABLE_UNAVAILABLE")?;
    if !host
        .windows(foreground::OWNER_GATE.len())
        .any(|part| part == foreground::OWNER_GATE)
    {
        return Err("DEV_RUNTIME_FOREGROUND_PACKAGE_REQUIRED".into());
    }
    drop(store.host_lock()?);
    let previous: Value = read_json(&store.root().join("control.json"), 4096)?;
    let previous_desired = previous
        .get("desired")
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "RUNNING" | "PAUSED" | "STOPPED"))
        .ok_or("DEV_RUNTIME_CONTROL_INVALID")?
        .to_owned();
    runtime::control(&store, "RUNNING")?;
    let mut command = Command::new(&executable);
    command
        .arg("--dev-foreground-host")
        .arg(workspace)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let (pid, exit, deadline_reached) = match foreground::run_with_deadline(
        &mut command,
        maximum_seconds.map(Duration::from_secs),
        || runtime::control(&store, "STOPPED"),
    ) {
        Ok(result) => result,
        Err(error) => {
            if let Ok(lease) = store.host_lock() {
                drop(lease);
                let restore = if maximum_seconds.is_some() {
                    "STOPPED"
                } else {
                    &previous_desired
                };
                runtime::control(&store, restore)?;
            }
            return Err(error);
        }
    };
    Ok(
        json!({"mode":"DEVELOPMENT_UNPROTECTED","status":"FOREGROUND_HOST_EXITED",
        "pid":pid,"success":exit.success(),"exitCode":exit.code(),"ownedTreeExited":true,
        "maximumRuntimeSeconds":maximum_seconds,"deadlineReached":deadline_reached}),
    )
}

/// Internal foreground entry: EOF or a mismatched owner gate exits before any
/// runtime/control/browser action. Detached development start remains separate.
pub fn run_foreground_host(workspace: &Path) -> Result<()> {
    foreground::require_owner(std::io::stdin().lock())?;
    run_host(workspace)
}

struct LoginPreflight {
    root: PathBuf,
    python: PathBuf,
    profile: PathBuf,
    primitives: PathBuf,
    primitives_sha256: String,
    target_generation: u64,
    _host: crate::store::HostLease,
    _browser: File,
}

fn require_login_target(store: &Store, url: &str) -> Result<()> {
    let config = store.config()?;
    let matched = config
        .targets
        .values()
        .filter(|target| target.url == url)
        .count();
    match matched {
        1 => Ok(()),
        0 => Err("DEV_LOGIN_TARGET_NOT_REGISTERED".into()),
        _ => Err("DEV_LOGIN_TARGET_AMBIGUOUS".into()),
    }
}

fn require_stopped_control(root: &Path) -> Result<()> {
    let control: runtime::Control = read_json(&root.join("control.json"), 4096)?;
    if control.schema_version != 1 || control.desired != "STOPPED" {
        return Err("DEV_LOGIN_HOST_NOT_STOPPED".into());
    }
    Ok(())
}

fn require_observed_target_generation(store: &Store, url: &str, expected: u64) -> Result<()> {
    require_login_target(store, url)?;
    if store
        .config()?
        .targets
        .values()
        .find(|target| target.url == url)
        .map(|target| target.generation)
        != Some(expected)
    {
        return Err("DEV_LOGIN_TARGET_GENERATION_CHANGED".into());
    }
    Ok(())
}

fn login_preflight(workspace: &Path, target_url: &str) -> Result<LoginPreflight> {
    protocol::validate_target(target_url)?;
    let (root, authority) = validated_authority(workspace)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    validate_against_production(&authority, target_url)?;
    let store = Store::open(&root)?;
    require_stopped_control(&root)?;
    require_login_target(&store, target_url)?;
    let host = store.host_lock()?;
    let browser = runtime::browser_session_lock(&root, Duration::ZERO)?;
    // A target or control change while locks were acquired must not open a
    // browser under a stale binding. The host lease prevents host startup.
    let (checked_root, checked_authority) =
        validated_authority(workspace)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    if identity(&checked_root, true)? != identity(&root, true)? {
        return Err("DEV_RUNTIME_SELECTED_ROOT_MISMATCH".into());
    }
    validate_against_production(&checked_authority, target_url)?;
    require_stopped_control(&root)?;
    require_login_target(&store, target_url)?;
    let python = root.join("runtime/python.exe");
    let profile = root.join("browser-profile");
    plain(&profile, true)?;
    let primitives = root
        .join("versions")
        .join(&checked_authority.package_id)
        .join("wake_bridge.py");
    let primitives_sha256 = checked_authority
        .artifacts
        .get("wake_bridge.py")
        .cloned()
        .ok_or("DEV_RUNTIME_PACKAGE_MANIFEST_MISMATCH")?;
    let target_generation = store
        .config()?
        .targets
        .values()
        .find(|target| target.url == target_url)
        .ok_or("DEV_LOGIN_TARGET_NOT_REGISTERED")?
        .generation;
    Ok(LoginPreflight {
        root,
        python,
        profile,
        primitives,
        primitives_sha256,
        target_generation,
        _host: host,
        _browser: browser,
    })
}

fn read_login_token(reader: &mut BufReader<impl Read>) -> Result<String> {
    let mut token = Vec::new();
    for _ in 0..128 {
        let mut byte = [0u8; 1];
        reader
            .read_exact(&mut byte)
            .map_err(|_| "DEV_LOGIN_HELPER_OUTPUT_UNAVAILABLE")?;
        if byte[0] == b'\n' {
            if token.last() == Some(&b'\r') {
                token.pop();
            }
            return String::from_utf8(token).map_err(|_| "DEV_LOGIN_HELPER_OUTPUT_INVALID".into());
        }
        token.push(byte[0]);
    }
    Err("DEV_LOGIN_HELPER_OUTPUT_INVALID".into())
}

fn login_remaining(deadline: Instant, maximum: Duration) -> Result<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err("DEV_LOGIN_INTERACTION_TIMEOUT".into())
    } else {
        Ok(remaining.min(maximum))
    }
}

fn wait_login_enter(reader: impl Read + Send + 'static, wait: Duration) -> Result<()> {
    let (tx, rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        // Never retain or export arbitrary terminal input. Three bytes are
        // sufficient to reject anything other than a single Enter.
        let mut line = String::new();
        let result = BufReader::new(reader.take(3))
            .read_line(&mut line)
            .map_err(|_| "DEV_LOGIN_TERMINAL_UNAVAILABLE".to_owned())
            .and_then(|_| {
                if matches!(line.as_str(), "\n" | "\r\n") {
                    Ok(())
                } else {
                    Err("DEV_LOGIN_ENTER_REQUIRED".into())
                }
            });
        let _ = tx.send(result);
    });
    rx.recv_timeout(wait)
        .map_err(|_| "DEV_LOGIN_INTERACTION_TIMEOUT".to_owned())?
}

fn cleanup_login_child(
    mut child: Child,
    job: &crate::process_job::Job,
    marker: &Path,
) -> Result<std::process::ExitStatus> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut exit = loop {
        match child.try_wait() {
            Ok(Some(exit)) => break Some(exit),
            Ok(None) if std::time::Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(50));
            }
            _ => break None,
        }
    };
    if exit.is_none() {
        // The helper has not closed normally. Terminate only its retained Job.
        let _ = job.terminate();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while exit.is_none() && std::time::Instant::now() < deadline {
            exit = child.try_wait().ok().flatten();
            if exit.is_none() {
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
    let exit = exit.ok_or("DEV_LOGIN_CLEANUP_UNVERIFIED")?;
    drop(child);
    if job.wait_empty(Duration::from_secs(5)).is_err() {
        job.terminate_and_wait_empty(Duration::from_secs(10))
            .map_err(|_| "DEV_LOGIN_CLEANUP_UNVERIFIED")?;
    }
    plain(marker, false).map_err(|_| "DEV_LOGIN_CLEANUP_UNVERIFIED")?;
    fs::remove_file(marker).map_err(|_| "DEV_LOGIN_CLEANUP_UNVERIFIED")?;
    Ok(exit)
}

/// Inspect the existing registered development profile without interaction,
/// message entry, queue mutation, a new profile, or production policy changes.
pub fn inspect_readiness(workspace: &Path, target_url: &str) -> Result<Value> {
    let prepared = login_preflight(workspace, target_url)?;
    let marker = dev_dir(&canonical_workspace(workspace)?).join("login-active.json");
    write_json_new(
        &marker,
        &json!({"schemaVersion":1,"purpose":"DEV_LOGIN_OWNED_BROWSER_V1",
        "targetSha256":protocol::digest(target_url),"startedUtc":now()}),
    )?;
    let mut command = Command::new(&prepared.python);
    command
        .arg("-I")
        .arg("-c")
        .arg(include_str!("../../scripts/wake_readiness_visual_probe.py"))
        .arg("--await-start")
        .arg("--profile")
        .arg(&prepared.profile)
        .arg("--conversation-url")
        .arg(target_url)
        .arg("--primitives")
        .arg(&prepared.primitives)
        .arg("--primitives-sha256")
        .arg(&prepared.primitives_sha256)
        .arg("--read-only")
        .arg("--seconds")
        .arg("20")
        .current_dir(&prepared.root);
    let result = crate::readiness_probe::run_bounded(&mut command, Duration::from_secs(90));
    if result.as_ref().err() == Some(&"PROBE_CLEANUP_UNVERIFIED") {
        // Reuse the existing sticky owner marker when cleanup is unproven.
        return Err("DEV_READINESS_CLEANUP_UNVERIFIED".into());
    }
    plain(&marker, false)?;
    fs::remove_file(&marker).map_err(|_| "DEV_READINESS_CLEANUP_UNVERIFIED")?;
    let rows = result.map_err(str::to_owned)?;
    let final_state = rows
        .last()
        .and_then(|row| row.strip_prefix("PROBE:FINAL:"))
        .ok_or("DEV_READINESS_FINAL_UNOBSERVED")?;
    require_stopped_control(&prepared.root)?;
    require_observed_target_generation(
        &Store::open(&prepared.root)?,
        target_url,
        prepared.target_generation,
    )?;
    let browser_observation = if final_state == "OBSERVATION_UNAVAILABLE" {
        None
    } else {
        rows.iter()
            .rev()
            .find_map(|line| crate::readiness_probe::surface_observation(line))
    };
    Ok(
        json!({"mode":"DEVELOPMENT_UNPROTECTED","status":"BROWSER_READINESS_OBSERVED",
        "observations":rows,"finalState":final_state,"authenticationVerified":false,
        "browserObservation":browser_observation,"registeredTargetSha256":protocol::digest(target_url),
        "registeredTargetGeneration":prepared.target_generation,
        "advisory":true,"browserReadOnly":true,"sendAttempted":false,
        "hostStarted":false,"schedulerStarted":false,"ownedTreeExited":true}),
    )
}

/// Open only the prepared development browser profile for manual sign-in.
/// This intentionally never inspects authentication or reports readiness.
/// The browser lock stays held until the private Job proves all descendants
/// exited. Uncertain cleanup leaves a sticky marker requiring review.
pub fn login(workspace: &Path, target_url: &str) -> Result<Value> {
    let prepared = login_preflight(workspace, target_url)?;
    if !std::io::stdin().is_terminal() {
        return Err("DEV_LOGIN_INTERACTIVE_TERMINAL_REQUIRED".into());
    }
    let python = runtime::browser_process_root(&prepared.python)?;
    let profile = runtime::browser_process_root(&prepared.profile)?;
    let marker = dev_dir(&canonical_workspace(workspace)?).join("login-active.json");
    write_json_new(
        &marker,
        &json!({"schemaVersion":1,"purpose":"DEV_LOGIN_OWNED_BROWSER_V1",
            "targetSha256":protocol::digest(target_url),"startedUtc":now()}),
    )?;
    let mut command = Command::new(python);
    command
        .arg("-I")
        .arg("-c")
        .arg(include_str!("../../scripts/wake_development_login.py"))
        .arg(profile)
        .arg(target_url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .current_dir(&prepared.root);
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            fs::remove_file(&marker).map_err(|_| "DEV_LOGIN_CLEANUP_UNVERIFIED")?;
            return Err("DEV_LOGIN_HELPER_START_FAILED".into());
        }
    };
    // The helper waits for START before importing SeleniumBase or launching a
    // browser, so assignment failure can only leave this exact idle process.
    let job = match crate::process_job::Job::assign(&child) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let stopped = child.wait().is_ok();
            if !stopped {
                return Err("DEV_LOGIN_CLEANUP_UNVERIFIED".into());
            }
            fs::remove_file(&marker).map_err(|_| "DEV_LOGIN_CLEANUP_UNVERIFIED")?;
            return Err(error);
        }
    };
    // Bound the whole owned interaction, including browser opening, human
    // acknowledgement and normal close. Expiry still takes the same retained
    // Job cleanup path; it does not clear an earlier interrupted owner marker.
    let interaction_deadline = Instant::now() + Duration::from_secs(120);
    let interaction = (|| -> Result<()> {
        let output = child
            .stdout
            .take()
            .ok_or("DEV_LOGIN_HELPER_OUTPUT_UNAVAILABLE")?;
        let (tx, rx) = mpsc::sync_channel(2);
        thread::spawn(move || {
            let mut reader = BufReader::new(output);
            for _ in 0..2 {
                if tx.send(read_login_token(&mut reader)).is_err() {
                    break;
                }
            }
        });
        let mut input = child
            .stdin
            .take()
            .ok_or("DEV_LOGIN_HELPER_INPUT_UNAVAILABLE")?;
        input
            .write_all(b"START\n")
            .and_then(|_| input.flush())
            .map_err(|_| "DEV_LOGIN_HELPER_INPUT_UNAVAILABLE")?;
        let opened = rx
            .recv_timeout(login_remaining(
                interaction_deadline,
                Duration::from_secs(120),
            )?)
            .map_err(|_| "DEV_LOGIN_BROWSER_OPEN_UNPROVEN")??;
        if opened != "LOGIN_BROWSER_OPEN" {
            return Err("DEV_LOGIN_BROWSER_OPEN_UNPROVEN".into());
        }
        println!(
            "LOGIN_BROWSER_OPEN — inspect/sign in in the visible browser, then press Enter here. This session has a 120-second interaction limit, followed by guarded cleanup."
        );
        std::io::stdout()
            .flush()
            .map_err(|_| "DEV_LOGIN_TERMINAL_UNAVAILABLE")?;
        wait_login_enter(
            std::io::stdin(),
            login_remaining(interaction_deadline, Duration::from_secs(120))?,
        )?;
        input
            .write_all(b"\n")
            .and_then(|_| input.flush())
            .map_err(|_| "DEV_LOGIN_HELPER_INPUT_UNAVAILABLE")?;
        let closed = rx
            .recv_timeout(login_remaining(
                interaction_deadline,
                Duration::from_secs(30),
            )?)
            .map_err(|_| "DEV_LOGIN_BROWSER_CLOSE_UNPROVEN")??;
        if closed != "LOGIN_BROWSER_CLOSED" {
            return Err("DEV_LOGIN_BROWSER_CLOSE_UNPROVEN".into());
        }
        Ok(())
    })();
    // Even a helper success is insufficient until all Chrome descendants in
    // this retained Job have exited. Any cleanup doubt keeps the sticky marker.
    let exit = cleanup_login_child(child, &job, &marker)?;
    interaction?;
    if !exit.success() {
        return Err("DEV_LOGIN_HELPER_FAILED".into());
    }
    Ok(json!({"mode":"DEVELOPMENT_UNPROTECTED",
        "status":"LOGIN_WINDOW_CLOSED_UNVERIFIED","authenticationVerified":false}))
}

pub fn stop(workspace: &Path) -> Result<Value> {
    let (root, _) = validated_authority(workspace)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    let store = Store::open(&root)?;
    runtime::control(&store, "STOPPED")?;
    for _ in 0..50 {
        if let Ok(lease) = store.host_lock() {
            drop(lease);
            return Ok(
                json!({"mode":"DEVELOPMENT_UNPROTECTED","status":"HOST_LEASE_RELEASED","control":"STOPPED"}),
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(json!({"mode":"DEVELOPMENT_UNPROTECTED","status":"STOP_REQUESTED"}))
}

pub fn run_host(workspace: &Path) -> Result<()> {
    let (root, authority) = validated_authority(workspace)?.ok_or("DEV_RUNTIME_NOT_PREPARED")?;
    let executable = std::env::current_exe().map_err(|_| "DEV_RUNTIME_EXECUTABLE_UNAVAILABLE")?;
    let expected = root
        .join("versions")
        .join(&authority.package_id)
        .join("CatDeskWakeHost.exe");
    if identity(&executable, false)? != identity(&expected, false)?
        || hash_file(&executable, MAX_PACKAGE_FILE)? != authority.artifacts["CatDeskWakeHost.exe"]
    {
        return Err("DEV_RUNTIME_EXECUTABLE_MISMATCH".into());
    }
    let store = Store::open(&root)?;
    for target in store.config()?.targets.values() {
        validate_against_production(&authority, &target.url)?;
    }
    runtime::run_resilient(&store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn fixture() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let parent = std::env::temp_dir().join(format!(
            "catdesk-wake-dev-runtime-{}-{}-{}",
            std::process::id(),
            now(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let workspace = parent.join("control");
        let package = parent.join("package");
        let python = parent.join("python");
        fs::create_dir(&parent).unwrap();
        for dir in [&workspace, &package, &python] {
            fs::create_dir(dir).unwrap();
        }
        fs::write(
            package.join("CatDeskWakeHost.exe"),
            b"isolated fixture host",
        )
        .unwrap();
        fs::write(
            package.join("CatDeskBinagotchy.exe"),
            b"isolated fixture gui",
        )
        .unwrap();
        fs::write(package.join("adapter.py"), include_bytes!("../adapter.py")).unwrap();
        fs::write(
            package.join("wake_bridge.py"),
            include_bytes!("../../scripts/wake_bridge.py"),
        )
        .unwrap();
        fs::write(python.join("python.exe"), b"isolated fixture python").unwrap();
        fs::write(python.join("__init__.py"), b"").unwrap();
        (parent, workspace, package, python)
    }

    fn cleanup(parent: &Path) {
        let canonical = fs::canonicalize(parent).unwrap();
        assert!(canonical.starts_with(fs::canonicalize(std::env::temp_dir()).unwrap()));
        assert!(
            parent
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("catdesk-wake-dev-runtime-")
        );
        fs::remove_dir_all(parent).unwrap();
    }

    fn prepare_fixture(workspace: &Path, package: &Path, python: &Path) -> Value {
        prepare(workspace, package, python).unwrap()
    }

    #[test]
    fn foreground_requires_owned_startup_protocol_before_control_change() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let root = root_for_workspace(&workspace).unwrap().unwrap();
        let store = Store::open(&root).unwrap();
        store
            .set_target(
                "canary-project",
                0,
                "https://chatgpt.com/c/foreground-fixture-only",
            )
            .unwrap();
        let before = fs::read(root.join("control.json")).unwrap();
        assert_eq!(
            run_foreground(&workspace).unwrap_err(),
            "DEV_RUNTIME_FOREGROUND_PACKAGE_REQUIRED"
        );
        assert_eq!(fs::read(root.join("control.json")).unwrap(), before);
        for budget in [0, 901, u64::MAX] {
            assert_eq!(
                run_foreground_bounded(&workspace, budget).unwrap_err(),
                "DEV_RUNTIME_FOREGROUND_BUDGET_INVALID"
            );
        }
        assert_eq!(fs::read(root.join("control.json")).unwrap(), before);
        cleanup(&parent);
    }

    #[test]
    fn package_update_preserves_profile_queue_targets_and_old_version() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let root = root_for_workspace(&workspace).unwrap().unwrap();
        let store = Store::open(&root).unwrap();
        let target = "https://chatgpt.com/c/dev-update-preserved-project";
        let bound = store.set_target("project-a", 0, target).unwrap();
        store
            .produce_expected(
                "dev-update-preserved",
                "project-a",
                "test",
                "Preserved queued test event",
                &bound,
            )
            .unwrap();
        let profile = root.join("browser-profile/preserved.txt");
        fs::write(&profile, b"authenticated profile fixture sentinel").unwrap();
        let old = validated_authority(&workspace).unwrap().unwrap().1;
        let old_package = root.join("versions").join(&old.package_id);
        let old_pointer = fs::read(root.join("current.json")).unwrap();
        let old_config = fs::read(root.join("config.json")).unwrap();
        let old_queue = store.events().unwrap();
        let old_profile = fs::read(&profile).unwrap();
        let updated_source = parent.join("updated-package");
        fs::create_dir(&updated_source).unwrap();
        for name in PACKAGE_FILES {
            fs::copy(package.join(name), updated_source.join(name)).unwrap();
        }
        fs::write(
            updated_source.join("CatDeskWakeHost.exe"),
            b"updated fixture host",
        )
        .unwrap();
        let result = update_package(&workspace, &updated_source).unwrap();
        assert_eq!(result["status"], "PACKAGE_UPDATED_STOPPED");
        let updated = validated_authority(&workspace).unwrap().unwrap().1;
        assert_ne!(old.package_id, updated.package_id);
        assert!(old_package.is_dir());
        assert_eq!(
            fs::read(old_package.join("CatDeskWakeHost.exe")).unwrap(),
            b"isolated fixture host"
        );
        assert_ne!(fs::read(root.join("current.json")).unwrap(), old_pointer);
        assert_eq!(fs::read(root.join("config.json")).unwrap(), old_config);
        assert_eq!(store.events().unwrap(), old_queue);
        assert_eq!(fs::read(&profile).unwrap(), old_profile);
        assert_eq!(store.config().unwrap().targets["project-a"].url, target);
        assert!(
            !dev_dir(&fs::canonicalize(&workspace).unwrap())
                .join(PACKAGE_UPDATE_ATTEMPT)
                .exists()
        );
        assert_eq!(
            update_package(&workspace, &updated_source).unwrap()["status"],
            "PACKAGE_ALREADY_CURRENT"
        );
        cleanup(&parent);
    }

    #[test]
    fn package_update_busy_or_running_refuses_without_switch() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let root = root_for_workspace(&workspace).unwrap().unwrap();
        let updated_source = parent.join("updated-package");
        fs::create_dir(&updated_source).unwrap();
        for name in PACKAGE_FILES {
            fs::copy(package.join(name), updated_source.join(name)).unwrap();
        }
        fs::write(
            updated_source.join("CatDeskWakeHost.exe"),
            b"updated fixture host",
        )
        .unwrap();
        let original = fs::read(root.join("current.json")).unwrap();
        let store = Store::open(&root).unwrap();
        let host = store.host_lock().unwrap();
        assert!(update_package(&workspace, &updated_source).is_err());
        drop(host);
        let browser = runtime::browser_session_lock(&root, Duration::ZERO).unwrap();
        assert_eq!(
            update_package(&workspace, &updated_source).unwrap_err(),
            "BROWSER_SESSION_BUSY"
        );
        drop(browser);
        runtime::control(&store, "RUNNING").unwrap();
        assert_eq!(
            update_package(&workspace, &updated_source).unwrap_err(),
            "DEV_PACKAGE_UPDATE_HOST_NOT_STOPPED"
        );
        runtime::control(&store, "STOPPED").unwrap();
        assert_eq!(fs::read(root.join("current.json")).unwrap(), original);
        assert!(
            !dev_dir(&fs::canonicalize(&workspace).unwrap())
                .join(PACKAGE_UPDATE_ATTEMPT)
                .exists()
        );
        cleanup(&parent);
    }

    #[test]
    fn package_update_marker_closes_normal_selection_and_resumes_pointer_first() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let workspace = fs::canonicalize(&workspace).unwrap();
        let root = root_for_workspace(&workspace).unwrap().unwrap();
        let updated_source = parent.join("updated-package");
        fs::create_dir(&updated_source).unwrap();
        for name in PACKAGE_FILES {
            fs::copy(package.join(name), updated_source.join(name)).unwrap();
        }
        fs::write(
            updated_source.join("CatDeskWakeHost.exe"),
            b"updated fixture host",
        )
        .unwrap();
        let source_hashes = source_artifacts(&updated_source).unwrap();
        let old = validated_authority(&workspace).unwrap().unwrap().1;
        let attempt =
            stage_package_update_attempt(&workspace, &root, &updated_source, &old, &source_hashes)
                .unwrap();
        assert_eq!(
            root_for_workspace(&workspace).unwrap_err(),
            "DEV_PACKAGE_UPDATE_INCOMPLETE"
        );
        replace_update_file(
            &root.join(&attempt.pointer_stage_name),
            &root.join("current.json"),
        )
        .unwrap();
        assert_eq!(
            root_for_workspace(&workspace).unwrap_err(),
            "DEV_PACKAGE_UPDATE_INCOMPLETE"
        );
        assert_eq!(
            update_package(&workspace, &updated_source).unwrap()["status"],
            "PACKAGE_UPDATED_STOPPED"
        );
        assert_eq!(
            validated_authority(&workspace)
                .unwrap()
                .unwrap()
                .1
                .package_id,
            attempt.new_authority.package_id
        );
        cleanup(&parent);
    }

    #[test]
    fn package_id_upgrade_distinguishes_bridge_only_changes() {
        let (_, _, package, _) = fixture();
        let old = source_artifacts(&package).unwrap();
        let mut changed = old.clone();
        changed.insert("wake_bridge.py".into(), "a".repeat(64));
        assert_eq!(package_id(&old).unwrap(), package_id(&changed).unwrap());
        assert_ne!(
            updated_package_id(&old).unwrap(),
            updated_package_id(&changed).unwrap()
        );
        cleanup(package.parent().unwrap());
    }

    fn package_recovery_fixture() -> (PathBuf, PathBuf, PathBuf, PackageUpdateAttempt) {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let workspace = fs::canonicalize(&workspace).unwrap();
        let root = root_for_workspace(&workspace).unwrap().unwrap();
        let store = Store::open(&root).unwrap();
        let target = store
            .set_target("project-a", 0, "https://chatgpt.com/c/recovery-fixture")
            .unwrap();
        store
            .produce_expected(
                "dev-recovery",
                "project-a",
                "test",
                "Retain queued work",
                &target,
            )
            .unwrap();
        fs::write(
            root.join("browser-profile/preserved.txt"),
            b"profile sentinel",
        )
        .unwrap();
        let source = parent.join("updated-package");
        fs::create_dir(&source).unwrap();
        for name in PACKAGE_FILES {
            fs::copy(package.join(name), source.join(name)).unwrap();
        }
        fs::write(source.join("CatDeskWakeHost.exe"), b"updated fixture host").unwrap();
        let hashes = source_artifacts(&source).unwrap();
        let old = validated_authority(&workspace).unwrap().unwrap().1;
        let attempt =
            stage_package_update_attempt(&workspace, &root, &source, &old, &hashes).unwrap();
        // Simulate expiration of the build directory, retaining its bytes for
        // diagnosis. Recovery must use the already published immutable package.
        fs::rename(&source, parent.join("retained-build-output")).unwrap();
        assert!(update_package(&workspace, &source).is_err());
        (parent, workspace, root, attempt)
    }

    #[test]
    fn package_recovery_finishes_each_interrupted_selection_without_build_directory() {
        for phase in 0..=2 {
            let (parent, workspace, root, attempt) = package_recovery_fixture();
            let store = Store::open(&root).unwrap();
            let config = fs::read(root.join("config.json")).unwrap();
            let profile = fs::read(root.join("browser-profile/preserved.txt")).unwrap();
            let events = store.events().unwrap();
            let old_image = root
                .join("versions")
                .join(&attempt.old_authority.package_id)
                .join("CatDeskWakeHost.exe");
            let old_bytes = fs::read(&old_image).unwrap();
            if phase >= 1 {
                replace_update_file(
                    &root.join(&attempt.pointer_stage_name),
                    &root.join("current.json"),
                )
                .unwrap();
            }
            if phase >= 2 {
                replace_update_file(
                    &dev_dir(&workspace).join(&attempt.authority_stage_name),
                    &dev_dir(&workspace).join("authority.json"),
                )
                .unwrap();
            }
            assert_eq!(
                root_for_workspace(&workspace).unwrap_err(),
                "DEV_PACKAGE_UPDATE_INCOMPLETE"
            );
            let recovered = recover_package(&workspace).unwrap();
            assert_eq!(recovered["status"], "PACKAGE_UPDATED_STOPPED");
            assert_eq!(recovered["hostStarted"], false);
            assert_eq!(recovered["browserAction"], "NONE");
            assert_eq!(
                validated_authority(&workspace).unwrap().unwrap().1,
                attempt.new_authority
            );
            assert_eq!(fs::read(root.join("config.json")).unwrap(), config);
            assert_eq!(
                fs::read(root.join("browser-profile/preserved.txt")).unwrap(),
                profile
            );
            assert_eq!(store.events().unwrap(), events);
            assert_eq!(fs::read(old_image).unwrap(), old_bytes);
            assert_eq!(
                read_json::<Value>(&root.join("control.json"), 4096).unwrap()["desired"],
                "STOPPED"
            );
            let pointer = fs::read(root.join("current.json")).unwrap();
            let authority = fs::read(dev_dir(&workspace).join("authority.json")).unwrap();
            assert_eq!(
                recover_package(&workspace).unwrap()["status"],
                "PACKAGE_CURRENT_VERIFIED_STOPPED"
            );
            assert_eq!(fs::read(root.join("current.json")).unwrap(), pointer);
            assert_eq!(
                fs::read(dev_dir(&workspace).join("authority.json")).unwrap(),
                authority
            );
            cleanup(&parent);
        }
    }

    #[test]
    fn package_recovery_does_not_accept_changed_staged_artifacts() {
        let (parent, workspace, root, attempt) = package_recovery_fixture();
        let pointer = fs::read(root.join("current.json")).unwrap();
        let authority = fs::read(dev_dir(&workspace).join("authority.json")).unwrap();
        let marker = fs::read(dev_dir(&workspace).join(PACKAGE_UPDATE_ATTEMPT)).unwrap();
        fs::write(
            root.join("versions")
                .join(attempt.new_authority.package_id)
                .join("CatDeskWakeHost.exe"),
            b"unexpected changed bytes",
        )
        .unwrap();
        assert_eq!(
            recover_package(&workspace).unwrap_err(),
            "DEV_PACKAGE_UPDATE_CONTENT_MISMATCH"
        );
        assert_eq!(fs::read(root.join("current.json")).unwrap(), pointer);
        assert_eq!(
            fs::read(dev_dir(&workspace).join("authority.json")).unwrap(),
            authority
        );
        assert_eq!(
            fs::read(dev_dir(&workspace).join(PACKAGE_UPDATE_ATTEMPT)).unwrap(),
            marker
        );
        cleanup(&parent);
    }

    #[test]
    fn package_recovery_does_not_overwrite_an_unknown_pointer() {
        let (parent, workspace, root, _) = package_recovery_fixture();
        fs::write(root.join("current.json"), b"unknown selection").unwrap();
        let authority = fs::read(dev_dir(&workspace).join("authority.json")).unwrap();
        let marker = fs::read(dev_dir(&workspace).join(PACKAGE_UPDATE_ATTEMPT)).unwrap();
        assert_eq!(
            recover_package(&workspace).unwrap_err(),
            "DEV_PACKAGE_UPDATE_POINTER_DIVERGED"
        );
        assert_eq!(
            fs::read(root.join("current.json")).unwrap(),
            b"unknown selection"
        );
        assert_eq!(
            fs::read(dev_dir(&workspace).join("authority.json")).unwrap(),
            authority
        );
        assert_eq!(
            fs::read(dev_dir(&workspace).join(PACKAGE_UPDATE_ATTEMPT)).unwrap(),
            marker
        );
        cleanup(&parent);
    }

    #[test]
    fn package_recovery_requires_stopped_exclusive_host_and_browser() {
        let (parent, workspace, root, _) = package_recovery_fixture();
        let store = Store::open(&root).unwrap();
        let pointer = fs::read(root.join("current.json")).unwrap();
        let host = store.host_lock().unwrap();
        assert!(recover_package(&workspace).is_err());
        drop(host);
        let browser = runtime::browser_session_lock(&root, Duration::ZERO).unwrap();
        assert_eq!(
            recover_package(&workspace).unwrap_err(),
            "BROWSER_SESSION_BUSY"
        );
        drop(browser);
        runtime::control(&store, "RUNNING").unwrap();
        assert_eq!(
            recover_package(&workspace).unwrap_err(),
            "DEV_PACKAGE_UPDATE_HOST_NOT_STOPPED"
        );
        assert_eq!(fs::read(root.join("current.json")).unwrap(), pointer);
        runtime::control(&store, "STOPPED").unwrap();
        assert_eq!(
            recover_package(&workspace).unwrap()["status"],
            "PACKAGE_UPDATED_STOPPED"
        );
        cleanup(&parent);
    }

    #[test]
    fn package_recovery_of_healthy_selection_is_noncreating_and_idempotent() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let root = root_for_workspace(&workspace).unwrap().unwrap();
        let pointer = fs::read(root.join("current.json")).unwrap();
        let authority = fs::read(dev_dir(&workspace).join("authority.json")).unwrap();
        for _ in 0..2 {
            assert_eq!(
                recover_package(&workspace).unwrap()["status"],
                "PACKAGE_CURRENT_VERIFIED_STOPPED"
            );
        }
        assert_eq!(fs::read(root.join("current.json")).unwrap(), pointer);
        assert_eq!(
            fs::read(dev_dir(&workspace).join("authority.json")).unwrap(),
            authority
        );
        assert!(!dev_dir(&workspace).join(PACKAGE_UPDATE_ATTEMPT).exists());
        cleanup(&parent);
    }

    fn verification_fixture_record(authority: &Authority) -> ReleaseVerification {
        ReleaseVerification {
            schema_version: 1,
            package_id: authority.package_id.clone(),
            manifest_sha256: authority.manifest_sha256.clone(),
            artifacts: authority.artifacts.clone(),
            source_commit: "a".repeat(40),
            build_evidence_sha256: "b".repeat(64),
            review_evidence_sha256: "c".repeat(64),
            working_health_evidence_sha256: "d".repeat(64),
            working_health_observed_utc: now(),
        }
    }

    fn record_fixture_verification(workspace: &Path) -> ReleaseVerification {
        let authority = validated_authority(workspace).unwrap().unwrap().1;
        let record = verification_fixture_record(&authority);
        let input = workspace.join(format!("verification-{}.json", authority.package_id));
        write_json_new(&input, &record).unwrap();
        let result = record_release_verification(workspace, &input).unwrap();
        assert_eq!(result["status"], "VERIFICATION_RECORDED");
        assert_eq!(result["evidenceAuthority"], "LOCAL_OPERATOR_PROVIDED");
        assert_eq!(result["healthChecksPerformed"], false);
        assert_eq!(result["productionAuthorityCreated"], false);
        record
    }

    fn update_fixture_host(parent: &Path, workspace: &Path, name: &str) {
        let root = root_for_workspace(workspace).unwrap().unwrap();
        let authority = validated_authority(workspace).unwrap().unwrap().1;
        let package = root.join("versions").join(authority.package_id);
        let source = parent.join(name);
        fs::create_dir(&source).unwrap();
        for file in PACKAGE_FILES {
            fs::copy(package.join(file), source.join(file)).unwrap();
        }
        fs::write(source.join("CatDeskWakeHost.exe"), name.as_bytes()).unwrap();
        assert_eq!(
            update_package(workspace, &source).unwrap()["status"],
            "PACKAGE_UPDATED_STOPPED"
        );
    }

    #[test]
    fn verification_is_immutable_local_and_preserves_legacy_selection_bytes() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let (root, authority) = validated_authority(&workspace).unwrap().unwrap();
        let authority_before = fs::read(dev_dir(&workspace).join("authority.json")).unwrap();
        let pointer_before = fs::read(root.join("current.json")).unwrap();
        let manifest_path = root
            .join("versions")
            .join(&authority.package_id)
            .join("manifest.json");
        let manifest_before = fs::read(&manifest_path).unwrap();
        let mut record = record_fixture_verification(&workspace);
        let input = workspace.join(format!("verification-{}.json", authority.package_id));
        assert_eq!(
            record_release_verification(&workspace, &input).unwrap()["status"],
            "VERIFICATION_ALREADY_RECORDED"
        );
        record.source_commit = "e".repeat(40);
        fs::write(&input, serde_json::to_vec(&record).unwrap()).unwrap();
        assert_eq!(
            record_release_verification(&workspace, &input).unwrap_err(),
            "DEV_RELEASE_VERIFICATION_CONFLICT"
        );
        assert_eq!(
            fs::read(dev_dir(&workspace).join("authority.json")).unwrap(),
            authority_before
        );
        assert_eq!(fs::read(root.join("current.json")).unwrap(), pointer_before);
        assert_eq!(fs::read(manifest_path).unwrap(), manifest_before);
        assert_eq!(
            status(&workspace).unwrap()["currentVerification"]["sourceCommit"],
            "a".repeat(40)
        );
        assert_eq!(
            status(&workspace).unwrap()["previousVerifiedRelease"]["status"],
            "NOT_RECORDED"
        );
        cleanup(&parent);
    }

    #[test]
    fn qualified_predecessor_survives_unqualified_updates_in_existing_pointer() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let (root, original) = validated_authority(&workspace).unwrap().unwrap();
        record_fixture_verification(&workspace);
        update_fixture_host(&parent, &workspace, "version-b");
        let first = read_retained_release(&root).unwrap().unwrap();
        assert_eq!(first.package_id, original.package_id);
        assert_eq!(first.artifacts, original.artifacts);
        assert_eq!(
            status(&workspace).unwrap()["currentVerification"]["status"],
            "NOT_RECORDED"
        );
        let previous = read_json::<Value>(&root.join("current.json"), 4096).unwrap()["previousVerifiedRelease"].clone();
        update_fixture_host(&parent, &workspace, "version-c");
        assert_eq!(
            read_json::<Value>(&root.join("current.json"), 4096).unwrap()["previousVerifiedRelease"],
            previous
        );
        assert_eq!(
            read_retained_release(&root).unwrap().unwrap().package_id,
            original.package_id
        );
        let (_, current) = validated_authority(&workspace).unwrap().unwrap();
        assert_ne!(current.package_id, original.package_id);
        assert_eq!(
            status(&workspace).unwrap()["previousVerifiedRelease"]["packageId"],
            original.package_id
        );
        // No second selection registry or retained-transaction store is created.
        assert!(
            !dev_dir(&workspace)
                .join("last-verified-package-update.json")
                .exists()
        );
        cleanup(&parent);
    }

    #[test]
    fn damaged_predecessor_metadata_does_not_block_healthy_current_selection() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let (root, original) = validated_authority(&workspace).unwrap().unwrap();
        record_fixture_verification(&workspace);
        update_fixture_host(&parent, &workspace, "healthy-current");
        fs::write(
            root.join("versions")
                .join(original.package_id)
                .join("CatDeskWakeHost.exe"),
            b"damaged predecessor",
        )
        .unwrap();
        assert!(root_for_workspace(&workspace).unwrap().is_some());
        assert_eq!(
            status(&workspace).unwrap()["previousVerifiedRelease"]["status"],
            "INVALID_OR_PACKAGE_CHANGED"
        );
        update_fixture_host(&parent, &workspace, "another-healthy-current");
        assert!(root_for_workspace(&workspace).unwrap().is_some());
        assert_eq!(
            status(&workspace).unwrap()["previousVerifiedRelease"]["status"],
            "INVALID_OR_PACKAGE_CHANGED"
        );
        cleanup(&parent);
    }

    #[test]
    fn verification_refuses_mismatched_evidence_and_outside_input_without_selection_writes() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let (root, authority) = validated_authority(&workspace).unwrap().unwrap();
        let before = fs::read(root.join("current.json")).unwrap();
        for fault in 0..4 {
            let mut record = verification_fixture_record(&authority);
            match fault {
                0 => record.source_commit = "A".repeat(40),
                1 => record
                    .artifacts
                    .insert("CatDeskWakeHost.exe".into(), "e".repeat(64))
                    .map(|_| ())
                    .unwrap(),
                2 => record.manifest_sha256 = "f".repeat(64),
                3 => record.working_health_observed_utc = now() + 3600,
                _ => unreachable!(),
            }
            let input = workspace.join(format!("bad-verification-{fault}.json"));
            write_json_new(&input, &record).unwrap();
            assert_eq!(
                record_release_verification(&workspace, &input).unwrap_err(),
                "DEV_RELEASE_VERIFICATION_INVALID"
            );
            assert_eq!(fs::read(root.join("current.json")).unwrap(), before);
        }
        let input = parent.join("outside-verification.json");
        write_json_new(&input, &verification_fixture_record(&authority)).unwrap();
        assert_eq!(
            record_release_verification(&workspace, &input).unwrap_err(),
            "DEV_RELEASE_VERIFICATION_INPUT_SCOPE_INVALID"
        );
        assert!(
            !root
                .join("versions")
                .join(authority.package_id)
                .join(RELEASE_VERIFICATION)
                .exists()
        );
        cleanup(&parent);
    }

    #[test]
    fn verification_requires_stopped_exclusive_ownership_and_pending_selection_stays_closed() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let (root, authority) = validated_authority(&workspace).unwrap().unwrap();
        let input = workspace.join("verification-input.json");
        write_json_new(&input, &verification_fixture_record(&authority)).unwrap();
        let store = Store::open(&root).unwrap();
        let host = store.host_lock().unwrap();
        assert!(record_release_verification(&workspace, &input).is_err());
        drop(host);
        let browser = runtime::browser_session_lock(&root, Duration::ZERO).unwrap();
        assert_eq!(
            record_release_verification(&workspace, &input).unwrap_err(),
            "BROWSER_SESSION_BUSY"
        );
        drop(browser);
        runtime::control(&store, "RUNNING").unwrap();
        assert_eq!(
            record_release_verification(&workspace, &input).unwrap_err(),
            "DEV_PACKAGE_UPDATE_HOST_NOT_STOPPED"
        );
        runtime::control(&store, "STOPPED").unwrap();
        record_release_verification(&workspace, &input).unwrap();
        let source = parent.join("pending-package");
        fs::create_dir(&source).unwrap();
        for file in PACKAGE_FILES {
            fs::copy(package.join(file), source.join(file)).unwrap();
        }
        fs::write(source.join("CatDeskWakeHost.exe"), b"pending fixture host").unwrap();
        let hashes = source_artifacts(&source).unwrap();
        stage_package_update_attempt(
            &fs::canonicalize(&workspace).unwrap(),
            &root,
            &source,
            &authority,
            &hashes,
        )
        .unwrap();
        assert_eq!(
            record_release_verification(&workspace, &input).unwrap_err(),
            "DEV_PACKAGE_UPDATE_INCOMPLETE"
        );
        assert_eq!(
            recover_package(&workspace).unwrap()["status"],
            "PACKAGE_UPDATED_STOPPED"
        );
        assert_eq!(
            read_retained_release(&root).unwrap().unwrap().package_id,
            authority.package_id
        );
        cleanup(&parent);
    }

    #[test]
    fn legacy_package_update_journal_without_verification_fields_still_recovers() {
        let (parent, workspace, root, _) = package_recovery_fixture();
        let path = dev_dir(&workspace).join(PACKAGE_UPDATE_ATTEMPT);
        let mut value: Value = read_json(&path, 32 * 1024).unwrap();
        value.as_object_mut().unwrap().remove("oldPointerJson");
        value
            .as_object_mut()
            .unwrap()
            .remove("retainedVerification");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            recover_package(&workspace).unwrap()["status"],
            "PACKAGE_UPDATED_STOPPED"
        );
        assert!(read_retained_release(&root).unwrap().is_none());
        cleanup(&parent);
    }

    #[test]
    fn recover_package_restores_recorded_predecessor_after_current_image_failure() {
        for (name, missing) in PACKAGE_FILES
            .into_iter()
            .flat_map(|name| [(name, false), (name, true)])
        {
            let (parent, workspace, package, python) = fixture();
            prepare_fixture(&workspace, &package, &python);
            let (root, previous) = validated_authority(&workspace).unwrap().unwrap();
            record_fixture_verification(&workspace);
            update_fixture_host(&parent, &workspace, "failed-current");
            let current = validated_authority(&workspace).unwrap().unwrap().1;
            let image = root.join("versions").join(&current.package_id).join(name);
            if missing {
                fs::remove_file(&image).unwrap();
            } else {
                fs::write(&image, b"failed current bytes preserved").unwrap();
            }
            let result = recover_package(&workspace)
                .expect("intact qualified predecessor must restore despite failed current image");
            assert_eq!(result["status"], "PACKAGE_PREVIOUS_RESTORED_STOPPED");
            assert_eq!(result["packageId"], previous.package_id);
            assert_eq!(
                validated_authority(&workspace)
                    .unwrap()
                    .unwrap()
                    .1
                    .manifest_identity,
                previous.manifest_identity
            );
            if missing {
                assert!(!image.exists());
            } else {
                assert_eq!(fs::read(&image).unwrap(), b"failed current bytes preserved");
            }
            assert!(read_retained_release(&root).unwrap().is_none());
            let before = fs::read(root.join("current.json")).unwrap();
            assert_eq!(
                recover_package(&workspace).unwrap()["status"],
                "PACKAGE_CURRENT_VERIFIED_STOPPED"
            );
            assert_eq!(fs::read(root.join("current.json")).unwrap(), before);
            cleanup(&parent);
        }
    }

    fn previous_recovery_fixture() -> (PathBuf, PathBuf, PathBuf, Authority, Authority) {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let (root, previous) = validated_authority(&workspace).unwrap().unwrap();
        record_fixture_verification(&workspace);
        update_fixture_host(&parent, &workspace, "failed-current");
        let current = validated_authority(&workspace).unwrap().unwrap().1;
        let store = Store::open(&root).unwrap();
        let target = store
            .set_target(
                "project-a",
                0,
                "https://chatgpt.com/c/dev-previous-retained",
            )
            .unwrap();
        store
            .produce_expected(
                "dev-previous-event",
                "project-a",
                "test",
                "Preserved queued work",
                &target,
            )
            .unwrap();
        fs::write(
            root.join("browser-profile").join("preserved.txt"),
            b"profile sentinel",
        )
        .unwrap();
        fs::write(
            root.join("versions")
                .join(&current.package_id)
                .join("CatDeskWakeHost.exe"),
            b"failed current preserved",
        )
        .unwrap();
        (parent, workspace, root, previous, current)
    }

    #[test]
    fn predecessor_restoration_recovers_every_journal_boundary_without_failed_version_oscillation()
    {
        for phase in 0..=2 {
            let (parent, workspace, root, previous, current) = previous_recovery_fixture();
            let store = Store::open(&root).unwrap();
            let config = fs::read(root.join("config.json")).unwrap();
            let events = store.events().unwrap();
            let proof_path = root
                .join("versions")
                .join(&previous.package_id)
                .join(RELEASE_VERIFICATION);
            let proof = fs::read(&proof_path).unwrap();
            let retained = read_retained_release(&root).unwrap().unwrap();
            let attempt = stage_previous_package_attempt(
                &fs::canonicalize(&workspace).unwrap(),
                &root,
                &current,
                retained,
            )
            .unwrap();
            if phase >= 1 {
                replace_update_file(
                    &root.join(&attempt.pointer_stage_name),
                    &root.join("current.json"),
                )
                .unwrap();
            }
            if phase >= 2 {
                replace_update_file(
                    &dev_dir(&workspace).join(&attempt.authority_stage_name),
                    &dev_dir(&workspace).join("authority.json"),
                )
                .unwrap();
            }
            assert_eq!(
                root_for_workspace(&workspace).unwrap_err(),
                "DEV_PACKAGE_UPDATE_INCOMPLETE"
            );
            assert_eq!(
                recover_package(&workspace).unwrap()["status"],
                "PACKAGE_PREVIOUS_RESTORED_STOPPED"
            );
            assert_eq!(
                validated_authority(&workspace)
                    .unwrap()
                    .unwrap()
                    .1
                    .package_id,
                previous.package_id
            );
            assert_eq!(fs::read(&proof_path).unwrap(), proof);
            assert_eq!(fs::read(root.join("config.json")).unwrap(), config);
            assert_eq!(store.events().unwrap(), events);
            assert_eq!(
                fs::read(root.join("browser-profile").join("preserved.txt")).unwrap(),
                b"profile sentinel"
            );
            assert_eq!(
                fs::read(
                    root.join("versions")
                        .join(&current.package_id)
                        .join("CatDeskWakeHost.exe")
                )
                .unwrap(),
                b"failed current preserved"
            );
            assert!(read_retained_release(&root).unwrap().is_none());
            fs::write(
                root.join("versions")
                    .join(&previous.package_id)
                    .join("CatDeskWakeHost.exe"),
                b"also failed",
            )
            .unwrap();
            assert_eq!(
                recover_package(&workspace).unwrap_err(),
                "DEV_RELEASE_RECONSTRUCTION_REQUIRED_NO_VERIFIED_PREDECESSOR"
            );
            cleanup(&parent);
        }
    }

    #[test]
    fn failed_current_without_trustworthy_predecessor_requires_reconstruction_without_selection_writes()
     {
        for fault in 0..3 {
            let (parent, workspace, package, python) = fixture();
            prepare_fixture(&workspace, &package, &python);
            let (root, previous) = validated_authority(&workspace).unwrap().unwrap();
            if fault != 0 {
                record_fixture_verification(&workspace);
            }
            update_fixture_host(&parent, &workspace, "failed-current");
            let current = validated_authority(&workspace).unwrap().unwrap().1;
            fs::write(
                root.join("versions")
                    .join(&current.package_id)
                    .join("CatDeskWakeHost.exe"),
                b"failed current",
            )
            .unwrap();
            if fault == 1 {
                fs::write(
                    root.join("versions")
                        .join(&previous.package_id)
                        .join("CatDeskWakeHost.exe"),
                    b"failed previous",
                )
                .unwrap();
            }
            if fault == 2 {
                fs::remove_file(
                    root.join("versions")
                        .join(&previous.package_id)
                        .join("manifest.json"),
                )
                .unwrap();
            }
            let pointer = fs::read(root.join("current.json")).unwrap();
            let authority = fs::read(dev_dir(&workspace).join("authority.json")).unwrap();
            let error = recover_package(&workspace).unwrap_err();
            assert!(error.starts_with("DEV_RELEASE_RECONSTRUCTION_REQUIRED_"));
            assert_eq!(fs::read(root.join("current.json")).unwrap(), pointer);
            assert_eq!(
                fs::read(dev_dir(&workspace).join("authority.json")).unwrap(),
                authority
            );
            assert!(!dev_dir(&workspace).join(PACKAGE_UPDATE_ATTEMPT).exists());
            cleanup(&parent);
        }
    }

    #[test]
    fn predecessor_restoration_requires_intact_current_metadata_python_and_exclusive_ownership() {
        for fault in 0..6 {
            let (parent, workspace, root, _, current) = previous_recovery_fixture();
            let store = Store::open(&root).unwrap();
            let host = if fault == 0 {
                Some(store.host_lock().unwrap())
            } else {
                None
            };
            let browser = if fault == 1 {
                Some(runtime::browser_session_lock(&root, Duration::ZERO).unwrap())
            } else {
                None
            };
            match fault {
                2 => runtime::control(&store, "RUNNING").unwrap(),
                3 => fs::write(
                    root.join("versions")
                        .join(&current.package_id)
                        .join("manifest.json"),
                    b"unknown manifest",
                )
                .unwrap(),
                4 => fs::write(root.join("runtime").join("python.exe"), b"unknown python").unwrap(),
                5 => fs::write(root.join("current.json"), b"unknown pointer").unwrap(),
                _ => (),
            }
            let pointer = fs::read(root.join("current.json")).unwrap();
            let authority = fs::read(dev_dir(&workspace).join("authority.json")).unwrap();
            assert!(recover_package(&workspace).is_err());
            assert_eq!(fs::read(root.join("current.json")).unwrap(), pointer);
            assert_eq!(
                fs::read(dev_dir(&workspace).join("authority.json")).unwrap(),
                authority
            );
            assert!(!dev_dir(&workspace).join(PACKAGE_UPDATE_ATTEMPT).exists());
            drop(browser);
            drop(host);
            cleanup(&parent);
        }
    }

    #[test]
    fn interrupted_predecessor_restoration_refuses_changed_evidence_or_assets() {
        for fault in 0..4 {
            let (parent, workspace, root, previous, current) = previous_recovery_fixture();
            let retained = read_retained_release(&root).unwrap().unwrap();
            let mut attempt = stage_previous_package_attempt(
                &fs::canonicalize(&workspace).unwrap(),
                &root,
                &current,
                retained,
            )
            .unwrap();
            match fault {
                0 => fs::write(
                    root.join("versions")
                        .join(&previous.package_id)
                        .join("CatDeskWakeHost.exe"),
                    b"changed after staging",
                )
                .unwrap(),
                1 => {
                    attempt
                        .restoration
                        .as_mut()
                        .unwrap()
                        .verification
                        .source_commit = "f".repeat(40)
                }
                2 => attempt.old_pointer_json.as_mut().unwrap().push(' '),
                3 => attempt.retained_verification = Some(verification_fixture_record(&current)),
                _ => unreachable!(),
            }
            let journal = dev_dir(&workspace).join(PACKAGE_UPDATE_ATTEMPT);
            fs::write(&journal, serde_json::to_vec(&attempt).unwrap()).unwrap();
            let marker = fs::read(&journal).unwrap();
            let pointer = fs::read(root.join("current.json")).unwrap();
            let authority = fs::read(dev_dir(&workspace).join("authority.json")).unwrap();
            assert!(recover_package(&workspace).is_err());
            assert_eq!(fs::read(root.join("current.json")).unwrap(), pointer);
            assert_eq!(
                fs::read(dev_dir(&workspace).join("authority.json")).unwrap(),
                authority
            );
            assert_eq!(fs::read(&journal).unwrap(), marker);
            cleanup(&parent);
        }
    }

    #[test]
    fn healthy_backend_selection_is_independent_of_running_wake_and_browser() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let root = root_for_workspace(&workspace).unwrap().unwrap();
        let store = Store::open(&root).unwrap();
        runtime::control(&store, "RUNNING").unwrap();
        let host = store.host_lock().unwrap();
        let browser = runtime::browser_session_lock(&root, Duration::ZERO).unwrap();
        let pointer = fs::read(root.join("current.json")).unwrap();
        let selection = select_backend(&workspace).unwrap();
        assert_eq!(selection.recovery, "CURRENT_PACKAGE_VERIFIED");
        assert_eq!(
            selection.executable.file_name().unwrap(),
            "CatDeskBinagotchy.exe"
        );
        assert_eq!(fs::read(root.join("current.json")).unwrap(), pointer);
        assert_eq!(
            read_json::<Value>(&root.join("control.json"), 4096).unwrap()["desired"],
            "RUNNING"
        );
        assert_eq!(
            select_backend(&workspace).err().unwrap(),
            "DEV_BACKEND_ALREADY_RUNNING"
        );
        assert_eq!(
            recover_package(&workspace).unwrap_err(),
            "DEV_BACKEND_ALREADY_RUNNING"
        );
        drop(selection);
        drop(browser);
        drop(host);
        cleanup(&parent);
    }

    #[test]
    fn selected_backend_pins_and_lease_prevent_selection_changes_until_release() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let root = root_for_workspace(&workspace).unwrap().unwrap();
        let selection = select_backend(&workspace).unwrap();
        let pointer = fs::read(root.join("current.json")).unwrap();
        for path in [
            &selection.executable,
            &root.join("current.json"),
            &dev_dir(&workspace).join("authority.json"),
        ] {
            assert!(fs::write(path, b"forbidden change while selected").is_err());
        }
        assert_eq!(
            update_package(&workspace, &package).unwrap_err(),
            "DEV_BACKEND_ALREADY_RUNNING"
        );
        assert_eq!(fs::read(root.join("current.json")).unwrap(), pointer);
        drop(selection);
        update_fixture_host(&parent, &workspace, "after-backend-exit");
        cleanup(&parent);
    }

    #[test]
    fn backend_selection_restores_failed_current_and_keeps_unknown_metadata_closed() {
        let (parent, workspace, root, previous, _) = previous_recovery_fixture();
        let selection = select_backend(&workspace).unwrap();
        assert_eq!(selection.recovery, "PACKAGE_PREVIOUS_RESTORED_STOPPED");
        assert_eq!(selection.package_id, previous.package_id);
        assert_eq!(
            selection.app_sha256,
            previous.artifacts["CatDeskBinagotchy.exe"]
        );
        drop(selection);
        fs::write(root.join("current.json"), b"unknown selection").unwrap();
        assert!(select_backend(&workspace).is_err());
        assert_eq!(
            fs::read(root.join("current.json")).unwrap(),
            b"unknown selection"
        );
        cleanup(&parent);
    }

    #[test]
    fn backend_selection_finishes_pending_update_and_refuses_unprepared_workspace() {
        let (parent, workspace, _, _) = fixture();
        assert_eq!(
            select_backend(&workspace).err().unwrap(),
            "DEV_RUNTIME_NOT_PREPARED"
        );
        assert!(!dev_dir(&workspace).exists());
        cleanup(&parent);
        let (parent, workspace, root, attempt) = package_recovery_fixture();
        let selection = select_backend(&workspace).unwrap();
        assert_eq!(selection.package_id, attempt.new_authority.package_id);
        assert_eq!(selection.recovery, "PACKAGE_UPDATED_STOPPED");
        assert!(!dev_dir(&workspace).join(PACKAGE_UPDATE_ATTEMPT).exists());
        assert_eq!(
            read_json::<Value>(&root.join("control.json"), 4096).unwrap()["desired"],
            "STOPPED"
        );
        drop(selection);
        cleanup(&parent);
    }

    #[test]
    fn backend_selection_leases_do_not_serialize_independent_workspaces() {
        let (a_parent, a_workspace, a_package, a_python) = fixture();
        let (b_parent, b_workspace, b_package, b_python) = fixture();
        prepare_fixture(&a_workspace, &a_package, &a_python);
        prepare_fixture(&b_workspace, &b_package, &b_python);
        let a = select_backend(&a_workspace).unwrap();
        let b = select_backend(&b_workspace).unwrap();
        assert_ne!(a.workspace, b.workspace);
        assert_eq!(
            update_package(&a_workspace, &a_package).unwrap_err(),
            "DEV_BACKEND_ALREADY_RUNNING"
        );
        drop(a);
        update_fixture_host(&a_parent, &a_workspace, "independent-next-version");
        assert_eq!(
            select_backend(&b_workspace).err().unwrap(),
            "DEV_BACKEND_ALREADY_RUNNING"
        );
        assert!(root_for_workspace(&b_workspace).unwrap().is_some());
        drop(b);
        cleanup(&a_parent);
        cleanup(&b_parent);
    }

    fn healthy_current_with_predecessor() -> (PathBuf, PathBuf, PathBuf, Authority, Authority) {
        let (parent, workspace, root, previous, current) = previous_recovery_fixture();
        fs::copy(
            parent.join("failed-current/CatDeskWakeHost.exe"),
            root.join("versions")
                .join(&current.package_id)
                .join("CatDeskWakeHost.exe"),
        )
        .unwrap();
        assert_eq!(validated_authority(&workspace).unwrap().unwrap().1, current);
        (parent, workspace, root, previous, current)
    }

    #[test]
    fn startup_failure_restores_valid_current_using_sealed_snapshot_and_retains_lease() {
        let (parent, workspace, root, previous, current) = healthy_current_with_predecessor();
        let image = root
            .join("versions")
            .join(&current.package_id)
            .join("CatDeskBinagotchy.exe");
        let original = fs::read(&image).unwrap();
        let mut selection = select_backend(&workspace).unwrap();
        selection.workspace = parent.clone();
        selection.package_id = "ignored-display-value".into();
        selection.app_sha256 = "f".repeat(64);
        let restored = selection.restore_after_startup_failure().unwrap();
        assert_eq!(restored.workspace, fs::canonicalize(&workspace).unwrap());
        assert_eq!(restored.package_id, previous.package_id);
        assert_eq!(
            restored.app_sha256,
            previous.artifacts["CatDeskBinagotchy.exe"]
        );
        assert_eq!(restored.recovery, "PACKAGE_PREVIOUS_RESTORED_STOPPED");
        assert_eq!(fs::read(&image).unwrap(), original);
        assert_eq!(
            select_backend(&workspace).err().unwrap(),
            "DEV_BACKEND_ALREADY_RUNNING"
        );
        let pointer = fs::read(root.join("current.json")).unwrap();
        assert_eq!(
            restored.restore_after_startup_failure().err().unwrap(),
            "DEV_RELEASE_RECONSTRUCTION_REQUIRED_NO_VERIFIED_PREDECESSOR"
        );
        assert_eq!(fs::read(root.join("current.json")).unwrap(), pointer);
        cleanup(&parent);
    }

    #[test]
    fn startup_failure_restoration_preserves_selection_on_busy_owner_or_stale_snapshot() {
        for fault in 0..4 {
            let (parent, workspace, root, previous, _) = healthy_current_with_predecessor();
            let store = Store::open(&root).unwrap();
            let mut selection = select_backend(&workspace).unwrap();
            let host = if fault == 0 {
                Some(store.host_lock().unwrap())
            } else {
                None
            };
            let browser = if fault == 1 {
                Some(runtime::browser_session_lock(&root, Duration::ZERO).unwrap())
            } else {
                None
            };
            if fault == 2 {
                runtime::control(&store, "RUNNING").unwrap();
            }
            if fault == 3 {
                selection.authority.package_id = previous.package_id;
            }
            let pointer = fs::read(root.join("current.json")).unwrap();
            let authority = fs::read(dev_dir(&workspace).join("authority.json")).unwrap();
            assert!(selection.restore_after_startup_failure().is_err());
            assert_eq!(fs::read(root.join("current.json")).unwrap(), pointer);
            assert_eq!(
                fs::read(dev_dir(&workspace).join("authority.json")).unwrap(),
                authority
            );
            assert!(!dev_dir(&workspace).join(PACKAGE_UPDATE_ATTEMPT).exists());
            drop(browser);
            drop(host);
            cleanup(&parent);
        }
    }

    #[test]
    fn startup_failure_without_eligible_predecessor_does_not_change_selection() {
        for fault in 0..3 {
            let (parent, workspace, package, python) = fixture();
            prepare_fixture(&workspace, &package, &python);
            let (root, previous) = validated_authority(&workspace).unwrap().unwrap();
            if fault != 0 {
                record_fixture_verification(&workspace);
            }
            update_fixture_host(&parent, &workspace, "valid-current");
            let selection = select_backend(&workspace).unwrap();
            if fault == 1 {
                fs::write(
                    root.join("versions")
                        .join(&previous.package_id)
                        .join("CatDeskWakeHost.exe"),
                    b"damaged previous",
                )
                .unwrap();
            }
            if fault == 2 {
                fs::remove_file(
                    root.join("versions")
                        .join(&previous.package_id)
                        .join("manifest.json"),
                )
                .unwrap();
            }
            let pointer = fs::read(root.join("current.json")).unwrap();
            let authority = fs::read(dev_dir(&workspace).join("authority.json")).unwrap();
            assert!(
                selection
                    .restore_after_startup_failure()
                    .err()
                    .unwrap()
                    .starts_with("DEV_RELEASE_RECONSTRUCTION_REQUIRED_")
            );
            assert_eq!(fs::read(root.join("current.json")).unwrap(), pointer);
            assert_eq!(
                fs::read(dev_dir(&workspace).join("authority.json")).unwrap(),
                authority
            );
            assert!(!dev_dir(&workspace).join(PACKAGE_UPDATE_ATTEMPT).exists());
            cleanup(&parent);
        }
    }

    #[test]
    fn interrupted_startup_failure_restoration_uses_existing_cold_recovery_journal() {
        use std::os::windows::fs::OpenOptionsExt;
        let (parent, workspace, root, previous, _) = healthy_current_with_predecessor();
        let selection = select_backend(&workspace).unwrap();
        let authority_path = dev_dir(&workspace).join("authority.json");
        let held = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&authority_path)
            .unwrap();
        assert_eq!(
            selection.restore_after_startup_failure().err().unwrap(),
            "DEV_PACKAGE_UPDATE_REPLACE_FAILED"
        );
        assert_eq!(
            root_for_workspace(&workspace).unwrap_err(),
            "DEV_PACKAGE_UPDATE_INCOMPLETE"
        );
        drop(held);
        assert_eq!(
            recover_package(&workspace).unwrap()["status"],
            "PACKAGE_PREVIOUS_RESTORED_STOPPED"
        );
        assert_eq!(
            validated_authority(&workspace)
                .unwrap()
                .unwrap()
                .1
                .package_id,
            previous.package_id
        );
        assert!(read_retained_release(&root).unwrap().is_none());
        cleanup(&parent);
    }

    #[test]
    fn login_protocol_accepts_only_bounded_fixed_lines_with_windows_newlines() {
        for line in ["LOGIN_BROWSER_OPEN\n", "LOGIN_BROWSER_OPEN\r\n"] {
            assert_eq!(
                read_login_token(&mut BufReader::new(line.as_bytes())).unwrap(),
                "LOGIN_BROWSER_OPEN"
            );
        }
        assert_eq!(
            read_login_token(&mut BufReader::new(b"never-terminated".as_slice())).unwrap_err(),
            "DEV_LOGIN_HELPER_OUTPUT_UNAVAILABLE"
        );
        let oversized = format!("{}\n", "x".repeat(129));
        assert_eq!(
            read_login_token(&mut BufReader::new(oversized.as_bytes())).unwrap_err(),
            "DEV_LOGIN_HELPER_OUTPUT_INVALID"
        );
    }

    #[test]
    fn login_enter_is_bounded_and_never_accepts_text_or_eof() {
        for bytes in [b"\n".as_slice(), b"\r\n"] {
            wait_login_enter(std::io::Cursor::new(bytes.to_vec()), Duration::from_secs(1)).unwrap();
        }
        for bytes in [b"".as_slice(), b"private input\n", b"\r", b"x\n"] {
            assert_eq!(
                wait_login_enter(std::io::Cursor::new(bytes.to_vec()), Duration::from_secs(1))
                    .unwrap_err(),
                "DEV_LOGIN_ENTER_REQUIRED"
            );
        }
        assert_eq!(
            login_remaining(
                Instant::now() - Duration::from_secs(1),
                Duration::from_secs(120)
            )
            .unwrap_err(),
            "DEV_LOGIN_INTERACTION_TIMEOUT"
        );
    }

    #[test]
    fn absent_login_enter_expires_without_waiting_for_input_thread() {
        struct HeldInput(mpsc::Receiver<()>, mpsc::Sender<()>);
        impl Read for HeldInput {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                let _ = self.0.recv();
                Ok(0)
            }
        }
        impl Drop for HeldInput {
            fn drop(&mut self) {
                let _ = self.1.send(());
            }
        }
        let (release, held) = mpsc::channel();
        let (exited, exit) = mpsc::channel();
        let start = Instant::now();
        assert_eq!(
            wait_login_enter(HeldInput(held, exited), Duration::from_millis(25)).unwrap_err(),
            "DEV_LOGIN_INTERACTION_TIMEOUT"
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        release.send(()).unwrap();
        exit.recv_timeout(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn expired_login_uses_existing_exact_job_cleanup_before_retiring_own_marker() {
        use std::os::windows::process::CommandExt;
        let (parent, workspace, _, _) = fixture();
        let marker = workspace.join("owned-login-test.json");
        fs::write(&marker, b"retained fixture marker").unwrap();
        let shell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut child = Command::new(shell)
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "if([Console]::In.ReadLine() -cne 'START'){exit 2}; [Console]::Out.WriteLine('READY'); Start-Sleep -Seconds 600"])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn().unwrap();
        let job = crate::process_job::Job::assign(&child).unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let _ = tx.send(read_login_token(&mut BufReader::new(output)));
        });
        let mut input = child.stdin.take().unwrap();
        input.write_all(b"START\n").unwrap();
        input.flush().unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap(),
            "READY"
        );
        assert!(marker.is_file());
        assert!(job.active_processes().unwrap() >= 1);
        let start = Instant::now();
        let exit = cleanup_login_child(child, &job, &marker).unwrap();
        assert!(!exit.success());
        assert!(start.elapsed() < Duration::from_secs(25));
        assert_eq!(job.active_processes().unwrap(), 0);
        assert!(!marker.exists());
        drop(input);
        drop(job);
        cleanup(&parent);
    }

    #[test]
    fn login_preflight_requires_exact_target_stopped_host_and_free_browser() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let root = root_for_workspace(&workspace).unwrap().unwrap();
        let store = Store::open(&root).unwrap();
        let url = "https://chatgpt.com/g/canary-project/c/canary-unique-login-1";
        assert_eq!(
            login_preflight(&workspace, url).err().unwrap(),
            "DEV_LOGIN_TARGET_NOT_REGISTERED"
        );
        store.set_target("canary-project", 0, url).unwrap();
        assert_eq!(
            login_preflight(&workspace, "https://chatgpt.com/c/other")
                .err()
                .unwrap(),
            "DEV_LOGIN_TARGET_NOT_REGISTERED"
        );
        let selected = login_preflight(&workspace, url).unwrap();
        assert_eq!(selected.root, root);
        assert_eq!(selected.target_generation, 1);
        assert_eq!(selected.primitives.file_name().unwrap(), "wake_bridge.py");
        assert_eq!(
            hash_file(&selected.primitives, MAX_PACKAGE_FILE).unwrap(),
            selected.primitives_sha256
        );
        store
            .set_target(
                "canary-project",
                1,
                "https://chatgpt.com/c/intervening-generation",
            )
            .unwrap();
        store.set_target("canary-project", 2, url).unwrap();
        assert_eq!(
            require_observed_target_generation(&store, url, selected.target_generation)
                .unwrap_err(),
            "DEV_LOGIN_TARGET_GENERATION_CHANGED"
        );
        require_observed_target_generation(&store, url, 3).unwrap();
        assert_eq!(
            login_preflight(&workspace, url).err().unwrap(),
            "HOST_ALREADY_RUNNING"
        );
        drop(selected);
        let busy = runtime::browser_session_lock(&root, Duration::ZERO).unwrap();
        assert_eq!(
            login_preflight(&workspace, url).err().unwrap(),
            "BROWSER_SESSION_BUSY"
        );
        drop(busy);
        runtime::control(&store, "RUNNING").unwrap();
        assert_eq!(
            login_preflight(&workspace, url).err().unwrap(),
            "DEV_LOGIN_HOST_NOT_STOPPED"
        );
        runtime::control(&store, "STOPPED").unwrap();
        let selected = login_preflight(&workspace, url).unwrap();
        drop(selected);
        cleanup(&parent);
    }

    #[test]
    fn unresolved_login_marker_blocks_all_dev_authority_selection() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        write_json_new(
            &dev_dir(&fs::canonicalize(&workspace).unwrap()).join("login-active.json"),
            &json!({"schemaVersion":1,"purpose":"DEV_LOGIN_OWNED_BROWSER_V1"}),
        )
        .unwrap();
        assert_eq!(
            root_for_workspace(&workspace).unwrap_err(),
            "DEV_LOGIN_CLEANUP_UNVERIFIED"
        );
        assert_eq!(
            status(&workspace).unwrap_err(),
            "DEV_LOGIN_CLEANUP_UNVERIFIED"
        );
        cleanup(&parent);
    }

    #[test]
    fn explicit_prepare_is_sticky_and_readback_rejects_source_drift() {
        let (parent, workspace, package, python) = fixture();
        assert!(root_for_workspace(&workspace).unwrap().is_none());
        let result = prepare_fixture(&workspace, &package, &python);
        assert_eq!(result["mode"], "DEVELOPMENT_UNPROTECTED");
        let root = root_for_workspace(&workspace).unwrap().unwrap();
        assert_eq!(root, runtime_root(&fs::canonicalize(&workspace).unwrap()));
        assert_eq!(status(&workspace).unwrap()["hostObservation"], "STOPPED");
        assert_eq!(
            fs::metadata(root.join("runtime/__init__.py"))
                .unwrap()
                .len(),
            0
        );
        assert!(prepare(&workspace, &package, &python).is_err());
        let pointer = root.join("current.json");
        fs::write(&pointer, b"{}").unwrap();
        assert!(root_for_workspace(&workspace).is_err());
        assert!(
            validate_selected_target_isolation(&workspace, &root, "https://chatgpt.com/c/canary")
                .is_err()
        );
        cleanup(&parent);
    }

    #[test]
    fn partial_attempt_never_falls_back_and_nonempty_workspace_refuses_prepare() {
        let (parent, workspace, package, python) = fixture();
        fs::create_dir_all(dev_dir(&workspace)).unwrap();
        write_json_new(
            &dev_dir(&workspace).join("prepare-attempt.json"),
            &json!({"schemaVersion":1,"purpose":PURPOSE}),
        )
        .unwrap();
        assert_eq!(
            root_for_workspace(&workspace).unwrap_err(),
            "DEV_RUNTIME_PREPARE_INCOMPLETE"
        );
        assert_eq!(
            prepare(&workspace, &package, &python).unwrap_err(),
            "DEV_WORKSPACE_NOT_NEW"
        );
        cleanup(&parent);
    }

    #[test]
    fn exact_conversation_id_is_isolated_across_project_routes_and_history() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let (_, mut authority) = validated_authority(&workspace).unwrap().unwrap();
        let production = parent.join("fixture-installed");
        fs::create_dir(&production).unwrap();
        let current =
            protocol::Target::new(1, "https://chatgpt.com/g/project-a/c/first".into()).unwrap();
        let historic = protocol::Target::new(1, "https://chatgpt.com/c/older".into()).unwrap();
        let config = Config {
            schema_version: 1,
            targets: BTreeMap::from([("project-a".into(), current)]),
            retired_targets: BTreeMap::new(),
            history: vec![crate::store::TargetChange {
                project_id: "project-a".into(),
                target: historic,
                changed_utc: now(),
            }],
        };
        write_json_new(&production.join("config.json"), &config).unwrap();
        authority.production_roots = vec![ProductionRoot {
            path: production.to_string_lossy().into_owned(),
            identity: Some(identity(&production, true).unwrap()),
        }];
        assert_eq!(
            validate_against_production(&authority, "https://chatgpt.com/c/first").unwrap_err(),
            "DEV_TARGET_SHARED_WITH_PRODUCTION"
        );
        assert_eq!(
            validate_against_production(&authority, "https://chatgpt.com/g/another/c/older")
                .unwrap_err(),
            "DEV_TARGET_SHARED_WITH_PRODUCTION"
        );
        validate_against_production(&authority, "https://chatgpt.com/c/fresh-canary").unwrap();
        fs::write(production.join("config.json"), b"not json").unwrap();
        assert_eq!(
            validate_against_production(&authority, "https://chatgpt.com/c/fresh-canary")
                .unwrap_err(),
            "DEV_PRODUCTION_TARGETS_UNAVAILABLE"
        );
        cleanup(&parent);
    }

    #[test]
    fn missing_dev_directory_and_corrupt_authority_never_fall_back() {
        let (parent, workspace, package, python) = fixture();
        prepare_fixture(&workspace, &package, &python);
        let authority_path = dev_dir(&workspace).join("authority.json");
        let original = fs::read(&authority_path).unwrap();
        let mut record: Authority = serde_json::from_slice(&original).unwrap();
        record
            .artifacts
            .insert("CatDeskWakeHost.exe".into(), "a".into());
        fs::write(&authority_path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(root_for_workspace(&workspace).is_err());
        fs::write(&authority_path, &original).unwrap();
        record = serde_json::from_slice(&original).unwrap();
        record.production_roots.clear();
        fs::write(&authority_path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert_eq!(
            root_for_workspace(&workspace).unwrap_err(),
            "DEV_PRODUCTION_ROOT_SET_MISMATCH"
        );
        let gone = workspace.join(".catdesk/removed-dev-fixture");
        fs::rename(dev_dir(&workspace), gone).unwrap();
        assert_eq!(
            root_for_workspace(&workspace).unwrap_err(),
            "DEV_RUNTIME_AUTHORITY_DISAPPEARED"
        );
        cleanup(&parent);
    }
}
