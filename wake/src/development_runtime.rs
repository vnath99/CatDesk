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
    io::{BufReader, IsTerminal, Read, Write},
    os::windows::{
        ffi::OsStrExt,
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

const PURPOSE: &str = "CATDESK_WAKE_DEVELOPMENT_UNPROTECTED_V1";
const DEV_DIR: &str = "web-development";
const ROOT_DIR: &str = "runtime";
const PACKAGE_UPDATE_PURPOSE: &str = "CATDESK_WAKE_DEV_PACKAGE_UPDATE_V1";
const PACKAGE_UPDATE_ATTEMPT: &str = "package-update-attempt.json";
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
    if updated_package_id(hashes)? != id {
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
        || new.package_id != updated_package_id(source_hashes)?
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
    let (manifest_identity, manifest_sha256) =
        verify_update_package(root, &new.package_id, source_hashes)?;
    if new.manifest_identity != manifest_identity || new.manifest_sha256 != manifest_sha256 {
        return Err("DEV_PACKAGE_UPDATE_MANIFEST_MISMATCH".into());
    }
    Ok(())
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
                != package_pointer(&attempt.new_authority.package_id, source_hashes)
        {
            return Err("DEV_PACKAGE_UPDATE_POINTER_STAGE_INVALID".into());
        }
        if validated_authority_mode(workspace, true)?
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
        "PACKAGE_UPDATED_STOPPED",
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
    write_json_new(&pointer_stage, &package_pointer(&id, hashes))?;
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
        if authority.artifacts.get(name).map(String::as_str)
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
        if config.schema_version != 1 || config.targets.len() > 64 || config.history.len() > 128 {
            return Err("DEV_PRODUCTION_TARGETS_INVALID".into());
        }
        for target in config
            .targets
            .values()
            .chain(config.history.iter().map(|entry| &entry.target))
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
    Ok(
        json!({"mode":"DEVELOPMENT_UNPROTECTED","runtimeRoot":root,"packageId":authority.package_id,
        "hostObservation":live.host,"observedPid":live.pid,"liveOwnerVerified":false,
        "browser":live.browser,"queueDepth":live.queue_depth,
        "targets":live.targets.keys().collect::<Vec<_>>(),"attention":live.attention}),
    )
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
    Ok(LoginPreflight {
        root,
        python,
        profile,
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
            .recv_timeout(Duration::from_secs(120))
            .map_err(|_| "DEV_LOGIN_BROWSER_OPEN_UNPROVEN")??;
        if opened != "LOGIN_BROWSER_OPEN" {
            return Err("DEV_LOGIN_BROWSER_OPEN_UNPROVEN".into());
        }
        println!(
            "LOGIN_BROWSER_OPEN — sign in in the visible browser, then press Enter here to close it."
        );
        std::io::stdout()
            .flush()
            .map_err(|_| "DEV_LOGIN_TERMINAL_UNAVAILABLE")?;
        let mut acknowledgement = String::new();
        std::io::stdin()
            .read_line(&mut acknowledgement)
            .map_err(|_| "DEV_LOGIN_TERMINAL_UNAVAILABLE")?;
        if !matches!(acknowledgement.as_str(), "\n" | "\r\n") {
            return Err("DEV_LOGIN_ENTER_REQUIRED".into());
        }
        input
            .write_all(b"\n")
            .and_then(|_| input.flush())
            .map_err(|_| "DEV_LOGIN_HELPER_INPUT_UNAVAILABLE")?;
        let closed = rx
            .recv_timeout(Duration::from_secs(30))
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
