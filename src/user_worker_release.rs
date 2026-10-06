//! T-0374 per-user immutable ordinary-worker release foundation.
//!
//! This module is intentionally a storage/validation primitive. It neither
//! starts a worker nor owns the external MCP route. A later reviewed host
//! cutover must derive the production root from trusted OS current-user
//! identity and feed the validated manifest digest into the existing fixed
//! supervisor registration/OS-peer-attestation path.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
#[cfg(windows)]
use std::{
    ffi::{OsString, c_void},
    os::windows::ffi::{OsStrExt, OsStringExt},
};

use crate::control_plane_supervisor::WorkerBackendRegistrationV1;
use crate::windows_protected_fs::{ProtectedDirectoryGuard, read_optional_relative_regular};

const SCHEMA: u32 = 1;
const ROLE: &str = "catdesk-ordinary-worker-v1";
const MAX_IMAGE: u64 = 512 * 1024 * 1024;
const MAX_RECORD: u64 = 64 * 1024;
static EPHEMERAL: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UserWorkerReleaseManifestV1 {
    pub schema: u32,
    pub generation: u64,
    pub role: String,
    pub image_sha256: String,
    pub image_length: u64,
    pub source_snapshot_id: String,
    pub review_session_id: String,
    pub review_record_id: String,
    pub review_authority_sha256: String,
    pub attestation_id: String,
    pub attestation_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pointer {
    generation: u64,
    manifest_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveStateV1 {
    schema: u32,
    current: Pointer,
    previous: Option<Pointer>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct UserWorkerReleaseReadbackV1 {
    pub generation: u64,
    pub manifest_sha256: String,
    pub manifest: UserWorkerReleaseManifestV1,
}

/// Exact committed release identities for the current pointer and, when
/// retained, its one validated rollback predecessor. This is a projection, not
/// a release-selection API: production callers cannot supply either pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UserWorkerReleasePrestateV1 {
    pub current: UserWorkerReleaseReadbackV1,
    pub previous: Option<UserWorkerReleaseReadbackV1>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum ProtectedHostPrestateClassificationV1 {
    ReadyForReviewedActivation,
    NoUserRelease,
    NoRollbackAuthority,
    UserReleaseInvalid,
    SupervisorStateUnavailable,
    SupervisorNotReady,
    IdentityDivergence,
}

/// Bounded refinement of the compatibility `USER_RELEASE_INVALID` result.
/// It never returns a filesystem path, raw OS error, secret, or untrusted
/// caller assertion. `None` means that the release portion was structurally
/// valid (or simply absent) and another classification controls the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum UserReleaseDiagnosticV1 {
    ReleaseRootUnavailable,
    ProtectedPathOrReparseRefused,
    ActiveStateMissing,
    ActiveStateMalformed,
    ActiveStateChangedDuringReadback,
    CurrentImmutableReleaseMissing,
    CurrentImmutableReleaseMalformedOrTampered,
    PreviousImmutableReleaseMissing,
    PreviousImmutableReleaseMalformedOrTampered,
}

/// Bounded zero-choice output for a later reviewed cutover decision. No path,
/// endpoint, process ID, secret, or externally-owned runtime identity is
/// reported or accepted as input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ProtectedHostPrestateV1 {
    pub schema: u32,
    pub classification: ProtectedHostPrestateClassificationV1,
    pub release_diagnostic: Option<UserReleaseDiagnosticV1>,
    pub current_release: Option<UserWorkerReleaseReadbackV1>,
    pub previous_release: Option<UserWorkerReleaseReadbackV1>,
    pub supervisor: Option<crate::control_plane_supervisor::FixedSupervisorPrestateV1>,
}

/// Bounded, read-only host-preflight output. `ReadyForActivation` is never
/// inferred from ordinary health: a fixed supervisor pipe must still attach
/// OS-attested peer-image evidence during the separately authorized action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UserWorkerHostPreflightV1 {
    Refused {
        reason: &'static str,
    },
    SupervisorPeerAttestationRequired {
        generation: u64,
        manifest_sha256: String,
    },
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|_| "release serialization failed".into())
}

fn metadata_is_reparse_or_symlink(path: &Path) -> Result<bool, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "release object unavailable")?;
    if metadata.file_type().is_symlink() {
        return Ok(true);
    }
    #[cfg(windows)]
    if metadata.file_attributes() & 0x0400 != 0 {
        return Ok(true);
    }
    Ok(false)
}

fn require_plain_directory(path: &Path, label: &str) -> Result<(), String> {
    if metadata_is_reparse_or_symlink(path)? {
        return Err(format!("{label} is unsafe"));
    }
    if !fs::metadata(path)
        .map_err(|_| format!("{label} unavailable"))?
        .is_dir()
    {
        return Err(format!("{label} is unsafe"));
    }
    Ok(())
}

fn require_plain_file(path: &Path, limit: u64, label: &str) -> Result<Vec<u8>, String> {
    if metadata_is_reparse_or_symlink(path)? {
        return Err(format!("{label} is unsafe"));
    }
    let metadata = fs::metadata(path).map_err(|_| format!("{label} unavailable"))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(format!("{label} is unsafe"));
    }
    fs::read(path).map_err(|_| format!("{label} unreadable"))
}

impl UserWorkerReleaseManifestV1 {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA
            || self.generation == 0
            || self.role != ROLE
            || self.image_length == 0
            || self.image_length > MAX_IMAGE
            || !valid_hex(&self.image_sha256)
            || !valid_hex(&self.review_authority_sha256)
            || !valid_hex(&self.attestation_sha256)
            || !component(&self.source_snapshot_id)
            || !component(&self.review_session_id)
            || !component(&self.review_record_id)
            || !component(&self.attestation_id)
        {
            return Err("release manifest is invalid".into());
        }
        Ok(())
    }
}

/// Resolves only the Windows current-user profile from the known-folder API.
/// Environment variables, PATH, and caller paths have no role in this root.
#[cfg(windows)]
fn os_current_user_profile() -> Result<PathBuf, String> {
    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHGetKnownFolderPath(
            id: *const Guid,
            flags: u32,
            token: *mut c_void,
            path: *mut *mut u16,
        ) -> i32;
    }
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoTaskMemFree(memory: *mut c_void);
    }
    const FOLDERID_PROFILE: Guid = Guid {
        data1: 0x5e6c_858f,
        data2: 0x0e22,
        data3: 0x4760,
        data4: [0x9a, 0xfe, 0xea, 0x33, 0x17, 0xb6, 0x71, 0x73],
    };
    let mut raw = std::ptr::null_mut();
    if unsafe { SHGetKnownFolderPath(&FOLDERID_PROFILE, 0, std::ptr::null_mut(), &mut raw) } < 0
        || raw.is_null()
    {
        return Err("user release root unavailable".into());
    }
    let mut length = 0_usize;
    while length <= 32_768 && unsafe { *raw.add(length) } != 0 {
        length += 1;
    }
    if length == 0 || length > 32_768 {
        unsafe { CoTaskMemFree(raw.cast()) };
        return Err("user release root unavailable".into());
    }
    let profile = PathBuf::from(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(raw, length)
    }));
    unsafe { CoTaskMemFree(raw.cast()) };
    require_plain_directory(&profile, "current-user profile")?;
    Ok(profile)
}

#[cfg(not(windows))]
fn os_current_user_profile() -> Result<PathBuf, String> {
    Err("user release root unavailable".into())
}

/// Compiled, current-user-only root. The retained pinned chain is the
/// authority for production reopen; no public API accepts a root or filename.
fn protected_current_user_release_root() -> Result<ProtectedDirectoryGuard, String> {
    let profile = os_current_user_profile()?;
    let mut guard = ProtectedDirectoryGuard::acquire(&profile, "current-user release profile")?;
    for (component, label) in [
        ("AppData", "current-user release AppData"),
        ("Local", "current-user release Local"),
        ("CatDesk", "current-user release product"),
        ("WorkerReleases", "current-user release root"),
    ] {
        guard.descend_existing(component, label)?;
    }
    Ok(guard)
}

fn validate_pointer(pointer: &Pointer) -> Result<(), String> {
    if pointer.generation == 0 || !valid_hex(&pointer.manifest_sha256) {
        return Err("release pointer corrupt".into());
    }
    Ok(())
}

fn parse_active_state(bytes: &[u8]) -> Result<ActiveStateV1, String> {
    let state: ActiveStateV1 =
        serde_json::from_slice(bytes).map_err(|_| "release active state corrupt")?;
    if state.schema != SCHEMA {
        return Err("release active state corrupt".into());
    }
    validate_pointer(&state.current)?;
    if let Some(previous) = &state.previous {
        validate_pointer(previous)?;
        if previous.generation >= state.current.generation {
            return Err("release state generations invalid".into());
        }
    }
    Ok(state)
}

#[cfg(windows)]
fn atomic_replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err("release active state commit failed".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination).map_err(|_| "release active state commit failed".to_string())
}

fn read_active_state_from_fixed_root() -> Result<Option<(Vec<u8>, ActiveStateV1)>, String> {
    let guard = protected_current_user_release_root().map_err(|error| {
        if error.contains("unsafe") || error.contains("identity") {
            "USER_RELEASE_DIAGNOSTIC_PROTECTED_PATH".to_string()
        } else {
            "USER_RELEASE_DIAGNOSTIC_ROOT_UNAVAILABLE".to_string()
        }
    })?;
    let Some(state_bytes) = read_optional_relative_regular(
        &guard,
        "active-state.json",
        MAX_RECORD,
        "user release active state",
    )
    .map_err(|_| "USER_RELEASE_DIAGNOSTIC_PROTECTED_PATH".to_string())?
    else {
        return Ok(None);
    };
    let state = parse_active_state(&state_bytes)
        .map_err(|_| "USER_RELEASE_DIAGNOSTIC_ACTIVE_STATE_MALFORMED".to_string())?;
    Ok(Some((state_bytes, state)))
}

fn read_pointer_from_fixed_root(
    pointer: &Pointer,
    previous: bool,
) -> Result<UserWorkerReleaseReadbackV1, String> {
    let missing = if previous {
        "USER_RELEASE_DIAGNOSTIC_PREVIOUS_MISSING"
    } else {
        "USER_RELEASE_DIAGNOSTIC_CURRENT_MISSING"
    };
    let invalid = if previous {
        "USER_RELEASE_DIAGNOSTIC_PREVIOUS_INVALID"
    } else {
        "USER_RELEASE_DIAGNOSTIC_CURRENT_INVALID"
    };
    let mut guard = protected_current_user_release_root()
        .map_err(|_| "USER_RELEASE_DIAGNOSTIC_PROTECTED_PATH".to_string())?;
    if !guard
        .descend_optional_existing("versions", "user release versions")
        .map_err(|_| "USER_RELEASE_DIAGNOSTIC_PROTECTED_PATH".to_string())?
    {
        return Err(missing.into());
    }
    let version = format!("{}-{}", pointer.generation, pointer.manifest_sha256);
    if !guard
        .descend_optional_existing(&version, "user release version")
        .map_err(|_| "USER_RELEASE_DIAGNOSTIC_PROTECTED_PATH".to_string())?
    {
        return Err(missing.into());
    }
    let Some(manifest_bytes) = read_optional_relative_regular(
        &guard,
        "manifest.json",
        MAX_RECORD,
        "user release manifest",
    )
    .map_err(|_| "USER_RELEASE_DIAGNOSTIC_PROTECTED_PATH".to_string())?
    else {
        return Err(missing.into());
    };
    if digest(&manifest_bytes) != pointer.manifest_sha256 {
        return Err(invalid.into());
    }
    let manifest: UserWorkerReleaseManifestV1 =
        serde_json::from_slice(&manifest_bytes).map_err(|_| invalid.to_string())?;
    manifest.validate().map_err(|_| invalid.to_string())?;
    if manifest.generation != pointer.generation {
        return Err(invalid.into());
    }
    let Some(image) =
        read_optional_relative_regular(&guard, "worker.exe", MAX_IMAGE, "user release image")
            .map_err(|_| "USER_RELEASE_DIAGNOSTIC_PROTECTED_PATH".to_string())?
    else {
        return Err(missing.into());
    };
    if image.len() as u64 != manifest.image_length || digest(&image) != manifest.image_sha256 {
        return Err(invalid.into());
    }
    Ok(UserWorkerReleaseReadbackV1 {
        generation: pointer.generation,
        manifest_sha256: pointer.manifest_sha256.clone(),
        manifest,
    })
}

/// Read-only production reopen through the compiled user root and pinned,
/// no-follow children. Both committed pointers are separately revalidated, and
/// the active-state bytes are reopened at the end to reject a changing
/// current/previous relation. This never treats a workspace path,
/// `target/release`, or an unbound digest as a release authority.
pub(crate) fn read_current_user_worker_release_prestate()
-> Result<Option<UserWorkerReleasePrestateV1>, String> {
    let Some((initial_state_bytes, state)) = read_active_state_from_fixed_root()? else {
        return Ok(None);
    };
    let current = read_pointer_from_fixed_root(&state.current, false)?;
    let previous = state
        .previous
        .as_ref()
        .map(|pointer| read_pointer_from_fixed_root(pointer, true))
        .transpose()?;
    let Some((final_state_bytes, _)) = read_active_state_from_fixed_root()? else {
        return Err("USER_RELEASE_DIAGNOSTIC_ACTIVE_STATE_CHANGED".into());
    };
    if initial_state_bytes != final_state_bytes {
        return Err("USER_RELEASE_DIAGNOSTIC_ACTIVE_STATE_CHANGED".into());
    }
    Ok(Some(UserWorkerReleasePrestateV1 { current, previous }))
}

/// Compatibility current-only projection for existing fixed registration and
/// preflight callers. New cutover inspection must use the paired prestate
/// reader above so a rollback predecessor cannot be silently omitted.
pub(crate) fn read_current_user_worker_release()
-> Result<Option<UserWorkerReleaseReadbackV1>, String> {
    Ok(read_current_user_worker_release_prestate()?.map(|state| state.current))
}

/// The only supervisor-facing representation of the current immutable user
/// release. The fixed registration still requires the executing worker image
/// to equal this manifest; the pipe server later replaces worker claims with
/// OS-attested peer-image evidence before supervisor state changes.
pub(crate) fn fixed_current_user_worker_registration() -> Result<WorkerBackendRegistrationV1, String>
{
    let release =
        read_current_user_worker_release()?.ok_or_else(|| "user release is missing".to_string())?;
    crate::control_plane_supervisor::fixed_current_worker_registration(
        &release.manifest.image_sha256,
    )
    .map_err(|_| "user release worker registration refused".into())
}

/// Performs no pipe connection, backend registration, process launch, or
/// durable write. It first reopens the exact selected release under the fixed
/// trusted current-user root, then checks the existing compiled registration
/// constraints against the running image. A successful local image check is
/// deliberately still only `SupervisorPeerAttestationRequired`: the server's
/// OS-attested connected-peer check remains a later, mutation-authorizing
/// boundary and cannot be manufactured by this preflight.
pub(crate) fn preflight_current_user_worker_host() -> UserWorkerHostPreflightV1 {
    let release = match read_current_user_worker_release() {
        Ok(Some(release)) => release,
        Ok(None) => {
            return UserWorkerHostPreflightV1::Refused {
                reason: "USER_RELEASE_MISSING",
            };
        }
        Err(_) => {
            return UserWorkerHostPreflightV1::Refused {
                reason: "USER_RELEASE_INVALID",
            };
        }
    };
    let registration_matches = crate::control_plane_supervisor::fixed_current_worker_registration(
        &release.manifest.image_sha256,
    )
    .is_ok();
    classify_host_preflight(release, registration_matches)
}

fn classify_host_preflight(
    release: UserWorkerReleaseReadbackV1,
    registration_matches: bool,
) -> UserWorkerHostPreflightV1 {
    if !registration_matches {
        return UserWorkerHostPreflightV1::Refused {
            reason: "SUPERVISOR_WORKER_IMAGE_MISMATCH",
        };
    }
    UserWorkerHostPreflightV1::SupervisorPeerAttestationRequired {
        generation: release.generation,
        manifest_sha256: release.manifest_sha256,
    }
}

fn backend_matches_release(
    backend: &crate::control_plane_supervisor::SupervisorBackendIdentityV1,
    release: &UserWorkerReleaseReadbackV1,
) -> bool {
    // The supervisor registration names its value `*_manifest_sha256` for
    // legacy protocol compatibility, but its fixed registration constructor
    // binds all four fields to the executable image digest. The immutable
    // release pointer digest remains separately reported as `manifest_sha256`.
    let identity = release.manifest.image_sha256.as_str();
    backend.declared_manifest_sha256 == identity
        && backend.observed_manifest_sha256 == identity
        && backend.expected_process_identity_sha256 == identity
        && backend.observed_process_identity_sha256 == identity
}

fn classify_protected_host_prestate(
    release: Result<Option<UserWorkerReleasePrestateV1>, String>,
    supervisor: Result<crate::control_plane_supervisor::FixedSupervisorPrestateV1, ()>,
) -> ProtectedHostPrestateV1 {
    let (current_release, previous_release, release_classification, release_diagnostic) =
        match release {
            Ok(None) => (
                None,
                None,
                Some(ProtectedHostPrestateClassificationV1::NoUserRelease),
                Some(UserReleaseDiagnosticV1::ActiveStateMissing),
            ),
            Err(error) => (
                None,
                None,
                Some(ProtectedHostPrestateClassificationV1::UserReleaseInvalid),
                Some(classify_release_diagnostic(&error)),
            ),
            Ok(Some(release)) => (Some(release.current), release.previous, None, None),
        };
    let supervisor = supervisor.ok();
    let classification = release_classification.unwrap_or_else(|| {
        let Some(previous) = &previous_release else {
            return ProtectedHostPrestateClassificationV1::NoRollbackAuthority;
        };
        let Some(supervisor) = &supervisor else {
            return ProtectedHostPrestateClassificationV1::SupervisorStateUnavailable;
        };
        let (Some(active), Some(rollback), Some(current)) = (
            &supervisor.active_backend,
            &supervisor.rollback_backend,
            &current_release,
        ) else {
            return ProtectedHostPrestateClassificationV1::NoRollbackAuthority;
        };
        if supervisor.activation_readiness != "SUPERVISOR_ACTIVATION_READY"
            || supervisor.local_backend_health != "LOCAL_BACKEND_READY"
            || supervisor.registration_generation == 0
        {
            return ProtectedHostPrestateClassificationV1::SupervisorNotReady;
        }
        if !backend_matches_release(active, current) || !backend_matches_release(rollback, previous)
        {
            return ProtectedHostPrestateClassificationV1::IdentityDivergence;
        }
        ProtectedHostPrestateClassificationV1::ReadyForReviewedActivation
    });
    ProtectedHostPrestateV1 {
        schema: SCHEMA,
        classification,
        release_diagnostic,
        current_release,
        previous_release,
        supervisor,
    }
}

fn classify_release_diagnostic(error: &str) -> UserReleaseDiagnosticV1 {
    match error {
        "USER_RELEASE_DIAGNOSTIC_ROOT_UNAVAILABLE" => {
            UserReleaseDiagnosticV1::ReleaseRootUnavailable
        }
        "USER_RELEASE_DIAGNOSTIC_PROTECTED_PATH" => {
            UserReleaseDiagnosticV1::ProtectedPathOrReparseRefused
        }
        "USER_RELEASE_DIAGNOSTIC_ACTIVE_STATE_MALFORMED" => {
            UserReleaseDiagnosticV1::ActiveStateMalformed
        }
        "USER_RELEASE_DIAGNOSTIC_ACTIVE_STATE_CHANGED" => {
            UserReleaseDiagnosticV1::ActiveStateChangedDuringReadback
        }
        "USER_RELEASE_DIAGNOSTIC_CURRENT_MISSING" => {
            UserReleaseDiagnosticV1::CurrentImmutableReleaseMissing
        }
        "USER_RELEASE_DIAGNOSTIC_PREVIOUS_MISSING" => {
            UserReleaseDiagnosticV1::PreviousImmutableReleaseMissing
        }
        "USER_RELEASE_DIAGNOSTIC_PREVIOUS_INVALID" => {
            UserReleaseDiagnosticV1::PreviousImmutableReleaseMalformedOrTampered
        }
        _ => UserReleaseDiagnosticV1::CurrentImmutableReleaseMalformedOrTampered,
    }
}

/// Zero-input fixed protected-host inspector. The reader uses only compiled
/// current-user and ProgramData roots, and its output is bounded JSON for an
/// independently reviewed later cutover decision. It does not inspect, own,
/// or modify the external Secure MCP runtime.
pub(crate) fn read_fixed_protected_host_prestate() -> ProtectedHostPrestateV1 {
    classify_protected_host_prestate(
        read_current_user_worker_release_prestate(),
        crate::control_plane_supervisor::read_fixed_supervisor_prestate().map_err(|_| ()),
    )
}

pub(crate) fn fixed_protected_host_prestate_json() -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(&read_fixed_protected_host_prestate())
        .map_err(|_| "protected host prestate serialization failed")?;
    if bytes.len() as u64 > MAX_RECORD {
        return Err("protected host prestate exceeds bound".into());
    }
    Ok(bytes)
}

/// Test/host-adapter seam. Production code must obtain its root from a trusted
/// current-user known-folder resolver; no production caller-selected path is
/// exposed by this foundation.
pub(crate) struct UserWorkerReleaseStoreV1 {
    root: PathBuf,
}

impl UserWorkerReleaseStoreV1 {
    pub(crate) fn open_for_test(root: PathBuf) -> Result<Self, String> {
        if root.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err("test root must be relative".into());
        }
        fs::create_dir_all(&root).map_err(|_| "release root unavailable")?;
        require_plain_directory(&root, "release root")?;
        Ok(Self { root })
    }

    fn versions(&self) -> PathBuf {
        self.root.join("versions")
    }

    fn state_path(&self) -> PathBuf {
        self.root.join("active-state.json")
    }

    fn ensure_root(&self) -> Result<(), String> {
        require_plain_directory(&self.root, "release root")?;
        if self.versions().exists() {
            require_plain_directory(&self.versions(), "release versions")?;
        }
        Ok(())
    }

    fn read_active_state(&self) -> Result<Option<ActiveStateV1>, String> {
        self.ensure_root()?;
        let path = self.state_path();
        if !path.exists() {
            return Ok(None);
        }
        let bytes = require_plain_file(&path, MAX_RECORD, "release active state")?;
        parse_active_state(&bytes).map(Some)
    }

    fn write_active_state(&self, state: &ActiveStateV1) -> Result<(), String> {
        self.ensure_root()?;
        parse_active_state(&canonical(state)?)?;
        let bytes = canonical(state)?;
        let nonce = EPHEMERAL.fetch_add(1, Ordering::Relaxed);
        let tmp = self.root.join(format!(".active-state.{nonce}.tmp"));
        if tmp.exists() {
            return Err("release active state staging conflict".into());
        }
        {
            let mut file =
                fs::File::create(&tmp).map_err(|_| "release active state write failed")?;
            use std::io::Write;
            file.write_all(&bytes)
                .map_err(|_| "release active state write failed")?;
            file.sync_all()
                .map_err(|_| "release active state sync failed")?;
        }
        require_plain_file(&tmp, MAX_RECORD, "release active state staging")?;
        let destination = self.state_path();
        if destination.exists() {
            require_plain_file(&destination, MAX_RECORD, "release active state")?;
        }
        atomic_replace_file(&tmp, &destination)
    }

    fn validate_committed_state(&self, state: &ActiveStateV1) -> Result<(), String> {
        self.validate_prepared(state.current.generation, &state.current.manifest_sha256)?;
        if let Some(previous) = &state.previous {
            self.validate_prepared(previous.generation, &previous.manifest_sha256)?;
        }
        Ok(())
    }

    fn validate_prepared(
        &self,
        generation: u64,
        manifest_sha256: &str,
    ) -> Result<UserWorkerReleaseReadbackV1, String> {
        if generation == 0 || !valid_hex(manifest_sha256) {
            return Err("prepared release identity invalid".into());
        }
        self.ensure_root()?;
        let dir = self
            .versions()
            .join(format!("{generation}-{manifest_sha256}"));
        require_plain_directory(&dir, "prepared release")?;
        let bytes =
            require_plain_file(&dir.join("manifest.json"), MAX_RECORD, "prepared manifest")?;
        if digest(&bytes) != manifest_sha256 {
            return Err("prepared manifest tampered".into());
        }
        let manifest: UserWorkerReleaseManifestV1 =
            serde_json::from_slice(&bytes).map_err(|_| "prepared manifest corrupt")?;
        manifest.validate()?;
        if manifest.generation != generation {
            return Err("prepared generation mismatched".into());
        }
        let image = require_plain_file(&dir.join("worker.exe"), MAX_IMAGE, "prepared image")?;
        if digest(&image) != manifest.image_sha256 || image.len() as u64 != manifest.image_length {
            return Err("prepared image tampered".into());
        }
        Ok(UserWorkerReleaseReadbackV1 {
            generation,
            manifest_sha256: manifest_sha256.into(),
            manifest,
        })
    }

    pub(crate) fn prepare(
        &self,
        manifest: &UserWorkerReleaseManifestV1,
        image: &[u8],
    ) -> Result<String, String> {
        manifest.validate()?;
        if image.len() as u64 != manifest.image_length || digest(image) != manifest.image_sha256 {
            return Err("release image binding failed".into());
        }
        self.ensure_root()?;
        if !self.versions().exists() {
            fs::create_dir(self.versions()).map_err(|_| "release versions unavailable")?;
        }
        require_plain_directory(&self.versions(), "release versions")?;

        let manifest_bytes = canonical(manifest)?;
        let md = digest(&manifest_bytes);
        let dir = self
            .versions()
            .join(format!("{}-{md}", manifest.generation));
        let generation_prefix = format!("{}-", manifest.generation);
        for entry in fs::read_dir(self.versions()).map_err(|_| "release versions unavailable")? {
            let entry = entry.map_err(|_| "release versions unavailable")?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&generation_prefix) && entry.path() != dir {
                return Err("release generation conflict".into());
            }
        }
        if dir.exists() {
            let readback = self.validate_prepared(manifest.generation, &md)?;
            if readback.manifest == *manifest {
                return Ok(md);
            }
            return Err("release generation conflict".into());
        }

        let nonce = EPHEMERAL.fetch_add(1, Ordering::Relaxed);
        let stage = self
            .versions()
            .join(format!(".stage-{}-{md}-{nonce}", manifest.generation));
        fs::create_dir(&stage).map_err(|_| "release stage unavailable")?;
        require_plain_directory(&stage, "release stage")?;
        fs::write(stage.join("worker.exe"), image).map_err(|_| "release image write failed")?;
        fs::write(stage.join("manifest.json"), &manifest_bytes)
            .map_err(|_| "release manifest write failed")?;
        require_plain_file(&stage.join("worker.exe"), MAX_IMAGE, "release staged image")?;
        require_plain_file(
            &stage.join("manifest.json"),
            MAX_RECORD,
            "release staged manifest",
        )?;
        fs::rename(stage, &dir).map_err(|_| "release prepare commit failed")?;
        self.validate_prepared(manifest.generation, &md)?;
        Ok(md)
    }

    pub(crate) fn readback(
        &self,
        generation: u64,
        manifest_sha256: &str,
    ) -> Result<UserWorkerReleaseReadbackV1, String> {
        self.validate_prepared(generation, manifest_sha256)
    }

    pub(crate) fn activate(
        &self,
        expected: u64,
        generation: u64,
        manifest_sha256: &str,
        ready: bool,
    ) -> Result<(), String> {
        if !ready {
            return Err("release readiness refused".into());
        }
        let state = self.read_active_state()?;
        if let Some(committed) = &state {
            self.validate_committed_state(committed)?;
        }
        let current_generation = state.as_ref().map_or(0, |s| s.current.generation);
        if current_generation != expected || generation <= expected {
            return Err("release generation CAS refused".into());
        }
        self.validate_prepared(generation, manifest_sha256)?;
        let next = Pointer {
            generation,
            manifest_sha256: manifest_sha256.into(),
        };
        let next_state = ActiveStateV1 {
            schema: SCHEMA,
            current: next,
            previous: state.map(|s| s.current),
        };
        self.write_active_state(&next_state)
    }

    pub(crate) fn rollback(&self, expected_current: u64) -> Result<u64, String> {
        let state = self
            .read_active_state()?
            .ok_or_else(|| "release rollback has no current version".to_string())?;
        self.validate_committed_state(&state)?;
        if state.current.generation != expected_current {
            return Err("release rollback CAS refused".into());
        }
        let previous = state
            .previous
            .ok_or_else(|| "release rollback has no previous version".to_string())?;
        self.validate_prepared(previous.generation, &previous.manifest_sha256)?;
        let generation = previous.generation;
        self.write_active_state(&ActiveStateV1 {
            schema: SCHEMA,
            current: previous,
            previous: None,
        })?;
        Ok(generation)
    }

    /// Revalidates all durable accepted state after interruption. Stale stage
    /// directories and active-state temp files are inert and ignored. A
    /// malformed/tampered committed state or prepared image fails closed; an
    /// absent initial state is valid.
    pub(crate) fn reconcile(&self) -> Result<Option<UserWorkerReleaseReadbackV1>, String> {
        self.ensure_root()?;
        let Some(state) = self.read_active_state()? else {
            return Ok(None);
        };
        self.validate_committed_state(&state)?;
        self.validate_prepared(state.current.generation, &state.current.manifest_sha256)
            .map(Some)
    }

    pub(crate) fn current_manifest_digest(&self) -> Result<Option<String>, String> {
        Ok(self
            .read_active_state()?
            .map(|state| state.current.manifest_sha256))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestRoot(PathBuf);
    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn store() -> (UserWorkerReleaseStoreV1, TestRoot) {
        let p = std::env::temp_dir().join(format!(
            "catdesk-t0374-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            EPHEMERAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        (UserWorkerReleaseStoreV1 { root: p.clone() }, TestRoot(p))
    }

    fn m(g: u64, b: &[u8]) -> UserWorkerReleaseManifestV1 {
        UserWorkerReleaseManifestV1 {
            schema: 1,
            generation: g,
            role: ROLE.into(),
            image_sha256: digest(b),
            image_length: b.len() as u64,
            source_snapshot_id: "snapshot".into(),
            review_session_id: "review-session".into(),
            review_record_id: "review".into(),
            review_authority_sha256: "a".repeat(64),
            attestation_id: "attestation".into(),
            attestation_sha256: "b".repeat(64),
        }
    }

    #[test]
    fn prepared_is_inert_readable_and_activation_is_monotonic_cas_bound() {
        let (s, _root) = store();
        let b = b"worker";
        let x = m(1, b);
        let d = s.prepare(&x, b).unwrap();
        assert_eq!(s.current_manifest_digest().unwrap(), None);
        assert_eq!(s.readback(1, &d).unwrap().manifest, x);
        assert!(s.activate(1, 1, &d, true).is_err());
        assert!(s.activate(0, 1, &d, false).is_err());
        s.activate(0, 1, &d, true).unwrap();
        assert_eq!(s.current_manifest_digest().unwrap(), Some(d.clone()));
        assert!(s.activate(1, 1, &d, true).is_err());
    }

    #[test]
    fn exact_prepare_replay_is_idempotent_and_same_generation_conflict_refuses() {
        let (s, _root) = store();
        let b = b"worker";
        let x = m(1, b);
        let d = s.prepare(&x, b).unwrap();
        assert_eq!(s.prepare(&x, b).unwrap(), d);
        let y = m(1, b"other");
        assert!(s.prepare(&y, b"other").is_err());
    }

    #[test]
    fn tamper_and_interrupted_stage_fail_closed_without_losing_current() {
        let (s, _root) = store();
        let first = m(1, b"one");
        let d1 = s.prepare(&first, b"one").unwrap();
        s.activate(0, 1, &d1, true).unwrap();

        let second = m(2, b"two");
        let d2 = s.prepare(&second, b"two").unwrap();
        fs::create_dir(s.versions().join(".stage-3-deadbeef-interrupted")).unwrap();
        assert_eq!(s.reconcile().unwrap().unwrap().generation, 1);

        fs::write(
            s.versions().join(format!("2-{d2}")).join("worker.exe"),
            b"tampered",
        )
        .unwrap();
        assert!(s.activate(1, 2, &d2, true).is_err());
        assert_eq!(s.reconcile().unwrap().unwrap().generation, 1);
    }

    #[test]
    fn rollback_restores_exact_prior_release_and_stale_cas_refuses() {
        let (s, _root) = store();
        let d1 = s.prepare(&m(1, b"one"), b"one").unwrap();
        s.activate(0, 1, &d1, true).unwrap();
        let d2 = s.prepare(&m(2, b"two"), b"two").unwrap();
        s.activate(1, 2, &d2, true).unwrap();
        assert!(s.rollback(1).is_err());
        assert_eq!(s.rollback(2).unwrap(), 1);
        assert_eq!(s.reconcile().unwrap().unwrap().manifest_sha256, d1);
    }

    #[test]
    fn malformed_or_invalid_durable_state_refuses() {
        let (s, _root) = store();
        let invalid = ActiveStateV1 {
            schema: SCHEMA,
            current: Pointer {
                generation: 2,
                manifest_sha256: "a".repeat(64),
            },
            previous: Some(Pointer {
                generation: 2,
                manifest_sha256: "b".repeat(64),
            }),
        };
        fs::write(s.state_path(), canonical(&invalid).unwrap()).unwrap();
        assert!(s.reconcile().is_err());
    }

    #[test]
    fn interrupted_active_state_stage_is_inert_and_committed_state_survives() {
        let (s, _root) = store();
        let d1 = s.prepare(&m(1, b"one"), b"one").unwrap();
        s.activate(0, 1, &d1, true).unwrap();
        fs::write(s.root.join(".active-state.999999.tmp"), b"interrupted").unwrap();
        assert_eq!(s.reconcile().unwrap().unwrap().generation, 1);
        assert_eq!(s.current_manifest_digest().unwrap(), Some(d1));
    }

    #[test]
    fn activation_and_rollback_commit_current_and_previous_together() {
        let (s, _root) = store();
        let d1 = s.prepare(&m(1, b"one"), b"one").unwrap();
        s.activate(0, 1, &d1, true).unwrap();
        let d2 = s.prepare(&m(2, b"two"), b"two").unwrap();
        s.activate(1, 2, &d2, true).unwrap();

        let activated = s.read_active_state().unwrap().unwrap();
        assert_eq!(activated.current.generation, 2);
        assert_eq!(activated.current.manifest_sha256, d2);
        assert_eq!(activated.previous.as_ref().unwrap().generation, 1);
        assert_eq!(activated.previous.as_ref().unwrap().manifest_sha256, d1);

        assert_eq!(s.rollback(2).unwrap(), 1);
        let rolled_back = s.read_active_state().unwrap().unwrap();
        assert_eq!(rolled_back.current.generation, 1);
        assert_eq!(rolled_back.current.manifest_sha256, d1);
        assert!(rolled_back.previous.is_none());
    }

    #[test]
    fn tampered_committed_binding_blocks_activation_and_rollback() {
        let (s, _root) = store();
        let d1 = s.prepare(&m(1, b"one"), b"one").unwrap();
        s.activate(0, 1, &d1, true).unwrap();
        let d2 = s.prepare(&m(2, b"two"), b"two").unwrap();
        s.activate(1, 2, &d2, true).unwrap();

        let mut state = s.read_active_state().unwrap().unwrap();
        state.current.manifest_sha256 = "c".repeat(64);
        fs::write(s.state_path(), canonical(&state).unwrap()).unwrap();

        let d3 = s.prepare(&m(3, b"three"), b"three").unwrap();
        assert!(s.activate(2, 3, &d3, true).is_err());
        assert!(s.rollback(2).is_err());
        assert!(s.reconcile().is_err());
    }

    #[cfg(windows)]
    #[test]
    fn reparse_substitution_is_refused() {
        use std::os::windows::fs::symlink_dir;
        let (s, _root) = store();
        let outside = std::env::temp_dir().join(format!(
            "catdesk-t0374-outside-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(s.versions()).unwrap();
        let link = s.versions().join(format!("1-{}", "a".repeat(64)));
        if symlink_dir(&outside, &link).is_ok() {
            assert!(s.readback(1, &"a".repeat(64)).is_err());
        }
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn source_keeps_legacy_and_external_runtime_out_of_release_authority() {
        let source = include_str!("user_worker_release.rs");
        let product_root = ["Program", "Files"].join(" ");
        let external_module = ["openai", "tunnel"].join("_");
        let external_setup = ["setup", "secure", "mcp"].join("-");
        assert!(!source.contains("join(\"target\")"));
        assert!(!source.contains(&product_root));
        assert!(!source.contains(&external_module));
        assert!(!source.contains(&external_setup));
        assert!(source.contains("ROLE: &str = \"catdesk-ordinary-worker-v1\""));
        assert!(source.contains("SHGetKnownFolderPath"));
        assert!(source.contains("fixed_current_worker_registration"));
        assert!(!source.contains("std::env::var(\"LOCALAPPDATA\")"));
    }

    #[test]
    fn review_session_record_authority_and_attestation_are_required_bindings() {
        let mut manifest = m(1, b"worker");
        assert!(manifest.validate().is_ok());
        manifest.review_session_id.clear();
        assert!(manifest.validate().is_err());
        manifest = m(1, b"worker");
        manifest.review_authority_sha256 = "A".repeat(64);
        assert!(manifest.validate().is_err());
        manifest = m(1, b"worker");
        manifest.attestation_sha256 = "not-a-digest".into();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn host_preflight_is_read_only_and_requires_later_peer_attestation() {
        let (store, _root) = store();
        let digest = store.prepare(&m(1, b"worker"), b"worker").unwrap();
        let release = store.readback(1, &digest).unwrap();
        assert_eq!(
            classify_host_preflight(release.clone(), false),
            UserWorkerHostPreflightV1::Refused {
                reason: "SUPERVISOR_WORKER_IMAGE_MISMATCH"
            }
        );
        assert_eq!(
            classify_host_preflight(release, true),
            UserWorkerHostPreflightV1::SupervisorPeerAttestationRequired {
                generation: 1,
                manifest_sha256: digest,
            }
        );
        assert_eq!(store.current_manifest_digest().unwrap(), None);
        let source = include_str!("user_worker_release.rs");
        assert!(source.contains("guard.descend_existing(component, label)?"));
        let pipe_client = ["open", "fixed", "pipe", "client"].join("_");
        let registration = ["Register", "Backend"].concat();
        assert!(!source.contains(&pipe_client));
        assert!(!source.contains(&registration));
    }

    fn release_prestate(current: UserWorkerReleaseReadbackV1) -> UserWorkerReleasePrestateV1 {
        UserWorkerReleasePrestateV1 {
            previous: Some(UserWorkerReleaseReadbackV1 {
                generation: current.generation - 1,
                manifest_sha256: "b".repeat(64),
                manifest: UserWorkerReleaseManifestV1 {
                    generation: current.generation - 1,
                    image_sha256: "b".repeat(64),
                    image_length: 1,
                    ..current.manifest.clone()
                },
            }),
            current,
        }
    }

    fn supervisor_backend(
        identity: &str,
    ) -> crate::control_plane_supervisor::SupervisorBackendIdentityV1 {
        crate::control_plane_supervisor::SupervisorBackendIdentityV1 {
            declared_manifest_sha256: identity.into(),
            observed_manifest_sha256: identity.into(),
            expected_process_identity_sha256: identity.into(),
            observed_process_identity_sha256: identity.into(),
        }
    }

    fn ready_supervisor(
        current: &UserWorkerReleaseReadbackV1,
        previous: &UserWorkerReleaseReadbackV1,
    ) -> crate::control_plane_supervisor::FixedSupervisorPrestateV1 {
        crate::control_plane_supervisor::FixedSupervisorPrestateV1 {
            activation_readiness: "SUPERVISOR_ACTIVATION_READY".into(),
            registration_generation: 2,
            local_backend_health: "LOCAL_BACKEND_READY".into(),
            active_backend: Some(supervisor_backend(&current.manifest.image_sha256)),
            rollback_backend: Some(supervisor_backend(&previous.manifest.image_sha256)),
        }
    }

    #[test]
    fn protected_host_prestate_requires_exact_current_prior_and_supervisor_identities() {
        let current_manifest = m(2, b"two");
        let current = UserWorkerReleaseReadbackV1 {
            generation: 2,
            manifest_sha256: digest(&canonical(&current_manifest).unwrap()),
            manifest: current_manifest,
        };
        let release = release_prestate(current);
        let supervisor = ready_supervisor(&release.current, release.previous.as_ref().unwrap());
        let ready = classify_protected_host_prestate(Ok(Some(release.clone())), Ok(supervisor));
        assert_eq!(
            ready.classification,
            ProtectedHostPrestateClassificationV1::ReadyForReviewedActivation
        );
        assert!(ready.previous_release.is_some());
        assert!(ready.supervisor.is_some());

        assert_eq!(
            classify_protected_host_prestate(Ok(None), Err(())).classification,
            ProtectedHostPrestateClassificationV1::NoUserRelease
        );
        assert_eq!(
            classify_protected_host_prestate(Err("tampered".into()), Err(())).classification,
            ProtectedHostPrestateClassificationV1::UserReleaseInvalid
        );
        assert_eq!(
            classify_protected_host_prestate(
                Ok(Some(UserWorkerReleasePrestateV1 {
                    current: release.current.clone(),
                    previous: None,
                })),
                Err(())
            )
            .classification,
            ProtectedHostPrestateClassificationV1::NoRollbackAuthority
        );
        assert_eq!(
            classify_protected_host_prestate(Ok(Some(release.clone())), Err(())).classification,
            ProtectedHostPrestateClassificationV1::SupervisorStateUnavailable
        );
    }

    #[test]
    fn protected_host_prestate_refuses_readiness_and_identity_divergence_without_mutation() {
        let current_manifest = m(2, b"two");
        let current = UserWorkerReleaseReadbackV1 {
            generation: 2,
            manifest_sha256: digest(&canonical(&current_manifest).unwrap()),
            manifest: current_manifest,
        };
        let release = release_prestate(current);
        let mut supervisor = ready_supervisor(&release.current, release.previous.as_ref().unwrap());
        supervisor.local_backend_health = "LOCAL_BACKEND_CRASHED".into();
        assert_eq!(
            classify_protected_host_prestate(Ok(Some(release.clone())), Ok(supervisor))
                .classification,
            ProtectedHostPrestateClassificationV1::SupervisorNotReady
        );
        let mut supervisor = ready_supervisor(&release.current, release.previous.as_ref().unwrap());
        supervisor
            .rollback_backend
            .as_mut()
            .unwrap()
            .observed_process_identity_sha256 = "c".repeat(64);
        assert_eq!(
            classify_protected_host_prestate(Ok(Some(release)), Ok(supervisor)).classification,
            ProtectedHostPrestateClassificationV1::IdentityDivergence
        );

        let source = include_str!("user_worker_release.rs");
        let mutating_pipe = ["open", "fixed", "pipe", "client"].join("_");
        let process_start = ["Command", ":", ":new"].concat();
        assert!(!source.contains(&mutating_pipe));
        assert!(!source.contains(&process_start));
        assert!(source.contains("read_fixed_supervisor_prestate"));
        assert!(source.contains("read_current_user_worker_release_prestate"));
    }

    #[test]
    fn protected_host_prestate_output_and_inspector_grammar_are_bounded_and_zero_choice() {
        let current_manifest = m(2, b"two");
        let current = UserWorkerReleaseReadbackV1 {
            generation: 2,
            manifest_sha256: digest(&canonical(&current_manifest).unwrap()),
            manifest: current_manifest,
        };
        let release = release_prestate(current);
        let supervisor = ready_supervisor(&release.current, release.previous.as_ref().unwrap());
        let output = classify_protected_host_prestate(Ok(Some(release)), Ok(supervisor));
        let bytes = serde_json::to_vec(&output).unwrap();
        assert!(bytes.len() as u64 <= MAX_RECORD);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("READY_FOR_REVIEWED_ACTIVATION"));
        assert!(!text.contains("endpoint"));
        assert!(!text.contains("processId"));

        let inspector = include_str!("bin/catdesk-user-worker-host-prestate.rs");
        assert!(inspector.contains("args_os().len() != 1"));
        assert!(inspector.contains("USER_WORKER_HOST_PRESTATE_ZERO_ARGUMENT_ONLY"));
        assert!(!inspector.contains("std::env::args().skip"));
    }

    #[test]
    fn release_diagnostic_taxonomy_preserves_coarse_refusal_without_paths() {
        let cases = [
            (
                "USER_RELEASE_DIAGNOSTIC_ROOT_UNAVAILABLE",
                UserReleaseDiagnosticV1::ReleaseRootUnavailable,
            ),
            (
                "USER_RELEASE_DIAGNOSTIC_PROTECTED_PATH",
                UserReleaseDiagnosticV1::ProtectedPathOrReparseRefused,
            ),
            (
                "USER_RELEASE_DIAGNOSTIC_ACTIVE_STATE_MALFORMED",
                UserReleaseDiagnosticV1::ActiveStateMalformed,
            ),
            (
                "USER_RELEASE_DIAGNOSTIC_CURRENT_MISSING",
                UserReleaseDiagnosticV1::CurrentImmutableReleaseMissing,
            ),
            (
                "USER_RELEASE_DIAGNOSTIC_CURRENT_INVALID",
                UserReleaseDiagnosticV1::CurrentImmutableReleaseMalformedOrTampered,
            ),
            (
                "USER_RELEASE_DIAGNOSTIC_PREVIOUS_MISSING",
                UserReleaseDiagnosticV1::PreviousImmutableReleaseMissing,
            ),
            (
                "USER_RELEASE_DIAGNOSTIC_PREVIOUS_INVALID",
                UserReleaseDiagnosticV1::PreviousImmutableReleaseMalformedOrTampered,
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(classify_release_diagnostic(input), expected);
            let prestate = classify_protected_host_prestate(Err(input.into()), Err(()));
            assert_eq!(
                prestate.classification,
                ProtectedHostPrestateClassificationV1::UserReleaseInvalid
            );
            assert_eq!(prestate.release_diagnostic, Some(expected));
            let json = serde_json::to_string(&prestate).unwrap();
            assert!(!json.contains("AppData"));
            assert!(!json.contains("ProgramData"));
        }
        let absent = classify_protected_host_prestate(Ok(None), Err(()));
        assert_eq!(
            absent.release_diagnostic,
            Some(UserReleaseDiagnosticV1::ActiveStateMissing)
        );
    }
}
