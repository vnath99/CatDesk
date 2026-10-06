//! Version-independent control-plane supervisor foundation (T-0223 R1).
//!
//! This module owns only durable, non-secret front-door and backend-health
//! state.  It deliberately does not start workers, create a tunnel, read an
//! API key, or select an executable.  A later host installer may place this
//! supervisor outside versioned release artifacts and connect its fixed local
//! `/mcp` front door to an attested backend registration protocol.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[cfg(windows)]
use crate::windows_protected_fs::{
    PinnedDirectory, ProtectedDirectoryGuard, create_delete_on_close_regular_lock,
    read_optional_relative_regular, read_relative_regular,
    remove_exact_relative_regular_with_bytes, write_new_regular_in,
    write_unique_regular_for_atomic_replace,
};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, HeaderName, Method, Response, StatusCode},
    response::IntoResponse,
    routing::{any, get},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

pub const SUPERVISOR_STATE_SCHEMA_VERSION: u32 = 1;
pub const FIXED_FRONT_DOOR_PATH: &str = "/mcp";
pub const FIXED_LOCAL_BACKEND_NAME: &str = "catdesk-local-backend";
pub const FIXED_LOCAL_BACKEND_ENDPOINT: &str = "http://127.0.0.1:3200/mcp";
pub const FIXED_LOCAL_BACKEND_PORT: u16 = 3200;
pub const FIXED_SUPERVISOR_LOOPBACK_HOST: &str = "127.0.0.1";
pub const FIXED_SUPERVISOR_LOOPBACK_PORT: u16 = 3201;
pub const FIXED_SUPERVISOR_IMAGE_NAME: &str = "catdesk-control-plane-supervisor.exe";
pub const FIXED_SUPERVISOR_CONTROL_PIPE: &str = r"\\.\pipe\CatDeskControlPlaneSupervisorV1";
pub(crate) const FIXED_STABLE_SUPERVISOR_RUNTIME_FLAG: &str = "--catdesk-control-plane-supervisor";
const MAX_STATE_BYTES: u64 = 64 * 1024;
const STATE_FILE: &str = "control-plane-supervisor.json";
const INSTALL_CURRENT_FILE: &str = "supervisor-current.json";
const INSTALL_LKG_FILE: &str = "supervisor-lkg.json";
const INSTALL_WRITER_LOCK: &str = "supervisor-install.lock";
const MAX_PROXY_REQUEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_PROXY_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PROXY_CONCURRENCY: usize = 16;
const PROXY_DEADLINE: Duration = Duration::from_secs(20);

/// Fixed host locations for a separately-installed supervisor. No current
/// release path, workspace path, environment variable, or caller input can
/// select either path.
pub fn fixed_supervisor_install_root() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\ProgramData\CatDesk\ControlPlaneSupervisor")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/var/lib/catdesk/control-plane-supervisor")
    }
}

pub fn fixed_supervisor_front_door() -> &'static str {
    FIXED_FRONT_DOOR_PATH
}

/// Parses the only CatDesk.exe stable-supervisor role entry. This mode has no
/// caller authority beyond selecting the compiled fixed role; all network,
/// pipe, state, image, principal, and startup values remain product-owned.
pub(crate) fn parse_fixed_stable_supervisor_runtime_args(args: &[String]) -> Result<bool, String> {
    let count = args
        .iter()
        .filter(|arg| arg.as_str() == FIXED_STABLE_SUPERVISOR_RUNTIME_FLAG)
        .count();
    if count == 0 {
        return Ok(false);
    }
    if count != 1 || args.len() != 1 {
        return Err("--catdesk-control-plane-supervisor accepts no additional arguments".into());
    }
    Ok(true)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FixedStableSupervisorRuntimeErrorV1 {
    RuntimeOwnershipUnproven,
    NotInstalledOrUnavailable,
    StateUnavailable,
    FrontDoorUnavailable,
    FixedPipeUnavailable,
    RequiredSurfaceFailed,
}

impl FixedStableSupervisorRuntimeErrorV1 {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeOwnershipUnproven => "CONTROL_PLANE_SUPERVISOR_RUNTIME_OWNERSHIP_UNPROVEN",
            Self::NotInstalledOrUnavailable => "CONTROL_PLANE_NOT_INSTALLED_OR_UNAVAILABLE",
            Self::StateUnavailable => "CONTROL_PLANE_STATE_UNAVAILABLE",
            Self::FrontDoorUnavailable => "CONTROL_PLANE_SUPERVISOR_FRONT_DOOR_UNAVAILABLE",
            Self::FixedPipeUnavailable => "CONTROL_PLANE_SUPERVISOR_FIXED_PIPE_UNAVAILABLE",
            Self::RequiredSurfaceFailed => "CONTROL_PLANE_SUPERVISOR_REQUIRED_SURFACE_FAILED",
        }
    }
}

/// The one shared stable-supervisor runtime. Both the standalone zero-argument
/// wrapper and the authenticated CatDesk role entry call this exact function,
/// so the two surfaces cannot diverge in listener, pipe, or ownership policy.
pub(crate) async fn run_fixed_stable_supervisor_runtime()
-> Result<(), FixedStableSupervisorRuntimeErrorV1> {
    require_stable_supervisor_runtime_capability(stable_supervisor_runtime_capability_descriptor())
        .map_err(|_| FixedStableSupervisorRuntimeErrorV1::RuntimeOwnershipUnproven)?;
    let store = ControlPlaneSupervisorStoreV1::fixed_read_only()
        .map_err(|_| FixedStableSupervisorRuntimeErrorV1::NotInstalledOrUnavailable)?;
    store
        .load()
        .map_err(|_| FixedStableSupervisorRuntimeErrorV1::StateUnavailable)?;
    let listener = tokio::net::TcpListener::bind((
        FIXED_SUPERVISOR_LOOPBACK_HOST,
        FIXED_SUPERVISOR_LOOPBACK_PORT,
    ))
    .await
    .map_err(|_| FixedStableSupervisorRuntimeErrorV1::FrontDoorUnavailable)?;

    #[cfg(not(windows))]
    {
        let _ = listener;
        return Err(FixedStableSupervisorRuntimeErrorV1::FixedPipeUnavailable);
    }

    #[cfg(windows)]
    {
        // Both long-lived surfaces are mandatory. If either future ends, the
        // other is dropped and this runtime terminates instead of half-running.
        let front_door_store = store.clone();
        let front_door = async move {
            axum::serve(listener, fixed_front_door_router(front_door_store))
                .await
                .map_err(|_| ())
        };
        let control_pipe = async move {
            crate::windows_supervisor_control_pipe::serve_fixed_control_pipe(store)
                .await
                .map_err(|_| ())
        };
        tokio::try_join!(front_door, control_pipe)
            .map(|_| ())
            .map_err(|_| FixedStableSupervisorRuntimeErrorV1::RequiredSurfaceFailed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LocalBackendHealthV1 {
    Missing,
    Crashed,
    Ready,
    Unknown,
}

impl LocalBackendHealthV1 {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "LOCAL_BACKEND_MISSING",
            Self::Crashed => "LOCAL_BACKEND_CRASHED",
            Self::Ready => "LOCAL_BACKEND_READY",
            Self::Unknown => "LOCAL_BACKEND_UNKNOWN",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RemoteRouteHealthV1 {
    Attached,
    Detached,
    Unknown,
}

impl RemoteRouteHealthV1 {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Attached => "REMOTE_ROUTE_ATTACHED",
            Self::Detached => "REMOTE_ROUTE_DETACHED",
            Self::Unknown => "REMOTE_ROUTE_UNKNOWN",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ControlPlaneReadinessV1 {
    Ready,
    LocalBackendUnavailable,
    RemoteRouteDetached,
    RemoteRouteUnknown,
}

/// Read-only source-level assessment for the separately reviewed T-0274 host
/// activation.  It neither creates the fixed ProgramData root nor starts a
/// listener, pipe, scheduler, or supervisor process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupervisorActivationReadinessV1 {
    Ready,
    RootUnavailable,
    StateInvalid,
    CurrentReceiptMissing,
    ReceiptOrImageInvalid,
    FixedPolicyInvalid,
    PrincipalPolicyUnproven,
    RuntimeOwnershipUnproven,
}

impl SupervisorActivationReadinessV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "SUPERVISOR_ACTIVATION_READY",
            Self::RootUnavailable => "SUPERVISOR_ROOT_UNAVAILABLE",
            Self::StateInvalid => "SUPERVISOR_STATE_INVALID",
            Self::CurrentReceiptMissing => "SUPERVISOR_CURRENT_RECEIPT_MISSING",
            Self::ReceiptOrImageInvalid => "SUPERVISOR_RECEIPT_OR_IMAGE_INVALID",
            Self::FixedPolicyInvalid => "SUPERVISOR_FIXED_POLICY_INVALID",
            Self::PrincipalPolicyUnproven => "SUPERVISOR_PRINCIPAL_POLICY_UNPROVEN",
            Self::RuntimeOwnershipUnproven => "SUPERVISOR_RUNTIME_OWNERSHIP_UNPROVEN",
        }
    }
}

/// Fixed capability token consumed by the zero-argument supervisor entrypoint
/// before it binds either long-lived surface. It declares only the ownership
/// actually implemented there: fixed local 3201, the private fixed pipe, and
/// protected local state. It intentionally grants no tunnel lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StableSupervisorRuntimeCapabilityV1 {
    FixedLocalFrontDoorPrivatePipeProtectedStateOnly,
    Unproven,
}

pub(crate) fn stable_supervisor_runtime_capability_descriptor()
-> StableSupervisorRuntimeCapabilityV1 {
    StableSupervisorRuntimeCapabilityV1::FixedLocalFrontDoorPrivatePipeProtectedStateOnly
}

pub(crate) fn require_stable_supervisor_runtime_capability(
    capability: StableSupervisorRuntimeCapabilityV1,
) -> Result<(), ControlPlaneSupervisorError> {
    match capability {
        StableSupervisorRuntimeCapabilityV1::FixedLocalFrontDoorPrivatePipeProtectedStateOnly => {
            Ok(())
        }
        StableSupervisorRuntimeCapabilityV1::Unproven => {
            Err(ControlPlaneSupervisorError::InvalidState)
        }
    }
}

impl ControlPlaneReadinessV1 {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "CONTROL_PLANE_READY",
            Self::LocalBackendUnavailable => "CONTROL_PLANE_LOCAL_BACKEND_UNAVAILABLE",
            Self::RemoteRouteDetached => "CONTROL_PLANE_REMOTE_ROUTE_DETACHED",
            Self::RemoteRouteUnknown => "CONTROL_PLANE_REMOTE_ROUTE_UNKNOWN",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerBackendRegistrationV1 {
    /// Opaque stable registration identifier, never an executable path.
    pub backend_id: String,
    /// Manifest assertion supplied by the fixed worker registration protocol.
    pub declared_manifest_sha256: String,
    /// Independent observed manifest assertion supplied by the supervisor-side
    /// verifier. A mismatch never replaces the active backend.
    pub observed_manifest_sha256: String,
    /// Must be the compiled fixed loopback MCP endpoint, never a caller
    /// chosen URL.
    pub endpoint: String,
    /// The expected/observed process identity values are bounded SHA-256
    /// assertions supplied by the fixed worker launch/inspection protocol.
    pub expected_process_identity_sha256: String,
    pub observed_process_identity_sha256: String,
}

/// Non-secret identity projection of an already validated fixed backend.
/// This is report-only: it intentionally omits the fixed endpoint and any
/// path, process ID, tunnel credential, or mutation authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SupervisorBackendIdentityV1 {
    pub declared_manifest_sha256: String,
    pub observed_manifest_sha256: String,
    pub expected_process_identity_sha256: String,
    pub observed_process_identity_sha256: String,
}

impl From<&WorkerBackendRegistrationV1> for SupervisorBackendIdentityV1 {
    fn from(registration: &WorkerBackendRegistrationV1) -> Self {
        Self {
            declared_manifest_sha256: registration.declared_manifest_sha256.clone(),
            observed_manifest_sha256: registration.observed_manifest_sha256.clone(),
            expected_process_identity_sha256: registration.expected_process_identity_sha256.clone(),
            observed_process_identity_sha256: registration.observed_process_identity_sha256.clone(),
        }
    }
}

/// Bounded, read-only view of fixed supervisor state for the later reviewed
/// user-worker cutover CAS decision. It is not a pipe status request and does
/// not imply that ordinary health can authorize activation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct FixedSupervisorPrestateV1 {
    pub activation_readiness: String,
    pub registration_generation: u64,
    pub local_backend_health: String,
    pub active_backend: Option<SupervisorBackendIdentityV1>,
    pub rollback_backend: Option<SupervisorBackendIdentityV1>,
}

/// Creates the only worker registration record that the production pipe
/// client can send.  The caller selects neither an endpoint nor an identity:
/// both manifest and process expectations are the bounded digest of the
/// current worker image and the exact accepted reviewed-image digest. The pipe
/// server independently replaces the two `observed_*` fields with its
/// OS-attested connected-peer image digest before any supervisor state can be
/// changed.
pub(crate) fn fixed_current_worker_registration(
    reviewed_manifest_sha256: &str,
) -> Result<WorkerBackendRegistrationV1, ControlPlaneSupervisorError> {
    const MAX_WORKER_IMAGE_BYTES: u64 = 512 * 1024 * 1024;
    let image = std::env::current_exe().map_err(|_| ControlPlaneSupervisorError::Io)?;
    let metadata = fs::metadata(&image).map_err(|_| ControlPlaneSupervisorError::Io)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_WORKER_IMAGE_BYTES {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    if !valid_sha256(reviewed_manifest_sha256) {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    let digest = sha256_hex(&fs::read(image).map_err(|_| ControlPlaneSupervisorError::Io)?);
    if digest != reviewed_manifest_sha256 {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    Ok(WorkerBackendRegistrationV1 {
        backend_id: FIXED_LOCAL_BACKEND_NAME.into(),
        declared_manifest_sha256: digest.clone(),
        // These fields are transport placeholders only.  The fixed control
        // pipe overwrites them from the connected process before dispatch.
        observed_manifest_sha256: digest.clone(),
        endpoint: FIXED_LOCAL_BACKEND_ENDPOINT.into(),
        expected_process_identity_sha256: digest.clone(),
        observed_process_identity_sha256: digest,
    })
}

/// Converts only a fixed registration request into supervisor-observed
/// evidence.  This is deliberately at the supervisor transport boundary: two
/// equal values sent by a worker can never count as independent observation.
pub(crate) fn bind_request_to_os_attested_peer(
    request: SupervisorControlRequestV1,
    peer: &SupervisorControlPeerV1,
) -> Result<SupervisorControlRequestV1, ControlPlaneSupervisorError> {
    let SupervisorControlRequestV1::RegisterBackend {
        mut registration,
        expected_generation,
        listener_ready,
    } = request
    else {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    };

    if !listener_ready
        || registration.backend_id != FIXED_LOCAL_BACKEND_NAME
        || registration.endpoint != FIXED_LOCAL_BACKEND_ENDPOINT
        || registration.declared_manifest_sha256 != peer.observed_process_identity_sha256
        || registration.expected_process_identity_sha256 != peer.observed_process_identity_sha256
    {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    registration.observed_manifest_sha256 = peer.observed_process_identity_sha256.clone();
    registration.observed_process_identity_sha256 = peer.observed_process_identity_sha256.clone();
    Ok(SupervisorControlRequestV1::RegisterBackend {
        registration,
        expected_generation,
        listener_ready: true,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StableTunnelIdentityV1 {
    /// An OpenAI runtime tunnel ID is non-secret routing identity. Credentials
    /// are intentionally not represented anywhere in this state model.
    pub tunnel_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlPlaneSupervisorStateV1 {
    pub schema_version: u32,
    pub fixed_front_door_path: String,
    pub active_backend: Option<WorkerBackendRegistrationV1>,
    pub rollback_backend: Option<WorkerBackendRegistrationV1>,
    pub local_backend_health: LocalBackendHealthV1,
    pub remote_route_health: RemoteRouteHealthV1,
    pub tunnel_identity: Option<StableTunnelIdentityV1>,
    pub registration_generation: u64,
    pub last_failure: Option<String>,
}

impl Default for ControlPlaneSupervisorStateV1 {
    fn default() -> Self {
        Self {
            schema_version: SUPERVISOR_STATE_SCHEMA_VERSION,
            fixed_front_door_path: FIXED_FRONT_DOOR_PATH.into(),
            active_backend: None,
            rollback_backend: None,
            local_backend_health: LocalBackendHealthV1::Missing,
            remote_route_health: RemoteRouteHealthV1::Unknown,
            tunnel_identity: None,
            registration_generation: 0,
            last_failure: None,
        }
    }
}

impl ControlPlaneSupervisorStateV1 {
    pub fn readiness(&self) -> ControlPlaneReadinessV1 {
        if self.active_backend.is_none() || self.local_backend_health != LocalBackendHealthV1::Ready
        {
            return ControlPlaneReadinessV1::LocalBackendUnavailable;
        }
        match self.remote_route_health {
            RemoteRouteHealthV1::Attached => ControlPlaneReadinessV1::Ready,
            RemoteRouteHealthV1::Detached => ControlPlaneReadinessV1::RemoteRouteDetached,
            RemoteRouteHealthV1::Unknown => ControlPlaneReadinessV1::RemoteRouteUnknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendRegistrationOutcomeV1 {
    Registered,
    Idempotent,
    RefusedManifestMismatch,
    RefusedBackendUnavailable,
    RefusedListenerIdentityMismatch,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ControlPlaneSupervisorError {
    InvalidState,
    InvalidRegistration,
    TunnelIdentityConflict,
    RollbackUnavailable,
    Io,
}

/// Closed control-plane records. These are deliberately not MCP records and
/// contain no URL, path, command, tunnel mutation, or credential field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum SupervisorControlRequestV1 {
    RegisterBackend {
        registration: WorkerBackendRegistrationV1,
        expected_generation: u64,
        listener_ready: bool,
    },
    ReportBackendHealth {
        backend_id: String,
        health: LocalBackendHealthV1,
        expected_generation: u64,
    },
    RollbackBackend {
        expected_generation: u64,
    },
    Status,
    RecordRemoteRouteStatus {
        observed_http_status: Option<u16>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupervisorControlPeerV1 {
    /// Real Windows pipe server code must derive these values from the client
    /// process/token and pipe handle, never from the JSON request. Its
    /// transport also requires the connected TokenUser and TokenSessionId to
    /// match the supervisor's OS-snapshotted ordinary daemon principal before
    /// this peer can reach control-record decoding.
    pub authenticated_local_peer: bool,
    pub process_id: u32,
    pub observed_process_identity_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupervisorControlResponseV1 {
    pub outcome: String,
    pub registration_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupervisorStartupRegistrationPlanV1 {
    None,
    Create,
    Repair,
    Ambiguous,
}

/// Internal boundary for the fixed named-pipe transport. The concrete Windows
/// server must create this peer from pipe/process/token handles; no decoded
/// request record can construct it. Keeping the dispatch input private to the
/// transport boundary prevents a future worker from treating a PID/hash field
/// as authentication.
pub trait SupervisorControlTransportV1 {
    fn dispatch_os_attested(
        &self,
        request: SupervisorControlRequestV1,
    ) -> Result<SupervisorControlResponseV1, ControlPlaneSupervisorError>;
}

/// Durable supervisor-owned state. `root` is the installation's fixed state
/// root in production; tests pass a temporary root. It is never derived from
/// a worker artifact or from tunnel credentials.
#[derive(Clone, Debug)]
pub struct ControlPlaneSupervisorStoreV1 {
    root: PathBuf,
}

impl ControlPlaneSupervisorStoreV1 {
    #[cfg(test)]
    pub fn open_test_seam(root: PathBuf) -> Result<Self, ControlPlaneSupervisorError> {
        open_store_root(root, true)
    }

    pub fn fixed_installation() -> Result<Self, ControlPlaneSupervisorError> {
        open_store_root(fixed_supervisor_install_root(), true)
    }

    /// The separately-installed supervisor's zero-argument status process is
    /// read-only. In particular it must not create ProgramData merely because
    /// an operator asks whether the stable supervisor is installed.
    pub fn fixed_read_only() -> Result<Self, ControlPlaneSupervisorError> {
        open_store_root(fixed_supervisor_install_root(), false)
    }
}

/// Reopens only the compiled protected supervisor root and projects the
/// already validated durable state. This does not open the control pipe,
/// create a root, start a listener, or change backend state.
pub(crate) fn read_fixed_supervisor_prestate()
-> Result<FixedSupervisorPrestateV1, ControlPlaneSupervisorError> {
    let readiness = assess_fixed_supervisor_activation_readiness();
    let store = ControlPlaneSupervisorStoreV1::fixed_read_only()?;
    let state = store.load()?;
    Ok(project_supervisor_prestate(state, readiness))
}

fn project_supervisor_prestate(
    state: ControlPlaneSupervisorStateV1,
    readiness: SupervisorActivationReadinessV1,
) -> FixedSupervisorPrestateV1 {
    FixedSupervisorPrestateV1 {
        activation_readiness: readiness.as_str().into(),
        registration_generation: state.registration_generation,
        local_backend_health: state.local_backend_health.as_str().into(),
        active_backend: state
            .active_backend
            .as_ref()
            .map(SupervisorBackendIdentityV1::from),
        rollback_backend: state
            .rollback_backend
            .as_ref()
            .map(SupervisorBackendIdentityV1::from),
    }
}

#[cfg(test)]
pub(crate) fn project_supervisor_prestate_test_seam(
    state: ControlPlaneSupervisorStateV1,
    readiness: SupervisorActivationReadinessV1,
) -> FixedSupervisorPrestateV1 {
    validate_state(&state).expect("test supervisor state must be valid");
    project_supervisor_prestate(state, readiness)
}

fn open_store_root(
    root: PathBuf,
    create_missing: bool,
) -> Result<ControlPlaneSupervisorStoreV1, ControlPlaneSupervisorError> {
    #[cfg(windows)]
    {
        let _ = protected_supervisor_root(&root, create_missing)?;
    }
    #[cfg(not(windows))]
    {
        let _ = (root, create_missing);
        return Err(ControlPlaneSupervisorError::InvalidState);
    }
    Ok(ControlPlaneSupervisorStoreV1 { root })
}

#[cfg(windows)]
fn protected_supervisor_root(
    root: &Path,
    create_missing: bool,
) -> Result<ProtectedDirectoryGuard, ControlPlaneSupervisorError> {
    // Production begins from the fixed ProgramData anchor and keeps its
    // ancestor handles alive through every state/install operation.  The
    // otherwise-identical branch below is only the crate-private test seam.
    if root == fixed_supervisor_install_root() {
        let program_data = PinnedDirectory::acquire(
            Path::new(r"C:\ProgramData"),
            "supervisor ProgramData parent",
        )
        .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
        let mut guard = ProtectedDirectoryGuard::from_pinned_root(program_data);
        descend_fixed_supervisor_component(&mut guard, "CatDesk", create_missing)?;
        descend_fixed_supervisor_component(&mut guard, "ControlPlaneSupervisor", create_missing)?;
        return Ok(guard);
    }
    let parent = root
        .parent()
        .ok_or(ControlPlaneSupervisorError::InvalidState)?;
    let component = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or(ControlPlaneSupervisorError::InvalidState)?;
    let parent = PinnedDirectory::acquire(parent, "supervisor fixed parent")
        .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
    let mut guard = ProtectedDirectoryGuard::from_pinned_root(parent);
    descend_fixed_supervisor_component(&mut guard, component, create_missing)?;
    guard
        .assert_stable("supervisor root")
        .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
    Ok(guard)
}

#[cfg(windows)]
fn descend_fixed_supervisor_component(
    guard: &mut ProtectedDirectoryGuard,
    component: &str,
    create_missing: bool,
) -> Result<(), ControlPlaneSupervisorError> {
    if guard
        .descend_optional_existing(component, "supervisor root")
        .map_err(|_| ControlPlaneSupervisorError::InvalidState)?
    {
        return Ok(());
    }
    if !create_missing {
        return Err(ControlPlaneSupervisorError::InvalidState);
    }
    // An unsafe existing child cannot be silently treated as absent: the
    // preceding optional RootDirectory/no-follow open distinguishes only the
    // exact NT missing-name result; create refuses occupation or a reparse.
    guard
        .create_child(component, "supervisor root")
        .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
    Ok(())
}

impl ControlPlaneSupervisorStoreV1 {
    pub fn load(&self) -> Result<ControlPlaneSupervisorStateV1, ControlPlaneSupervisorError> {
        #[cfg(windows)]
        {
            let root = protected_supervisor_root(&self.root, false)?;
            let Some(bytes) = read_optional_relative_regular(
                &root,
                STATE_FILE,
                MAX_STATE_BYTES,
                "supervisor state",
            )
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)?
            else {
                return Ok(Default::default());
            };
            let state: ControlPlaneSupervisorStateV1 = serde_json::from_slice(&bytes)
                .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
            validate_state(&state)?;
            Ok(state)
        }
        #[cfg(not(windows))]
        Err(ControlPlaneSupervisorError::InvalidState)
    }

    pub fn save(
        &self,
        state: &ControlPlaneSupervisorStateV1,
    ) -> Result<(), ControlPlaneSupervisorError> {
        validate_state(state)?;
        let bytes = serde_json::to_vec(state).map_err(|_| ControlPlaneSupervisorError::Io)?;
        #[cfg(windows)]
        {
            let root = protected_supervisor_root(&self.root, false)?;
            write_unique_regular_for_atomic_replace(&root, &bytes, "supervisor state temporary")
                .and_then(|temporary| {
                    temporary.commit_replace(STATE_FILE, "supervisor state commit")
                })
                .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
            Ok(())
        }
        #[cfg(not(windows))]
        Err(ControlPlaneSupervisorError::InvalidState)
    }

    pub fn register_backend(
        &self,
        registration: WorkerBackendRegistrationV1,
        local_listener_ready: bool,
    ) -> Result<BackendRegistrationOutcomeV1, ControlPlaneSupervisorError> {
        validate_registration(&registration)?;
        let mut state = self.load()?;
        if registration.declared_manifest_sha256 != registration.observed_manifest_sha256 {
            state.last_failure = Some("BACKEND_MANIFEST_MISMATCH".into());
            self.save(&state)?;
            return Ok(BackendRegistrationOutcomeV1::RefusedManifestMismatch);
        }
        if registration.expected_process_identity_sha256
            != registration.observed_process_identity_sha256
        {
            state.last_failure = Some("BACKEND_LISTENER_IDENTITY_MISMATCH".into());
            self.save(&state)?;
            return Ok(BackendRegistrationOutcomeV1::RefusedListenerIdentityMismatch);
        }
        if !local_listener_ready {
            state.last_failure = Some("BACKEND_LOCAL_LISTENER_UNAVAILABLE".into());
            self.save(&state)?;
            return Ok(BackendRegistrationOutcomeV1::RefusedBackendUnavailable);
        }
        if state.active_backend.as_ref() == Some(&registration)
            && state.local_backend_health == LocalBackendHealthV1::Ready
        {
            return Ok(BackendRegistrationOutcomeV1::Idempotent);
        }
        state.rollback_backend = state.active_backend.clone();
        state.active_backend = Some(registration);
        state.local_backend_health = LocalBackendHealthV1::Ready;
        state.registration_generation = state.registration_generation.saturating_add(1);
        state.last_failure = None;
        self.save(&state)?;
        Ok(BackendRegistrationOutcomeV1::Registered)
    }

    /// The sole state-mutating supervisor control dispatcher. Production pipe
    /// adapters must supply OS-observed peer evidence; a worker never opens or
    /// edits the durable state file itself. A stale CAS or peer/identity
    /// mismatch is refused before any candidate can replace the active route.
    pub fn dispatch_control(
        &self,
        peer: &SupervisorControlPeerV1,
        request: SupervisorControlRequestV1,
    ) -> Result<SupervisorControlResponseV1, ControlPlaneSupervisorError> {
        if !peer.authenticated_local_peer
            || peer.process_id == 0
            || !valid_sha256(&peer.observed_process_identity_sha256)
        {
            return Err(ControlPlaneSupervisorError::InvalidRegistration);
        }
        let state = self.load()?;
        let expected_generation = match &request {
            SupervisorControlRequestV1::RegisterBackend {
                expected_generation,
                ..
            }
            | SupervisorControlRequestV1::ReportBackendHealth {
                expected_generation,
                ..
            }
            | SupervisorControlRequestV1::RollbackBackend {
                expected_generation,
            } => Some(*expected_generation),
            SupervisorControlRequestV1::Status
            | SupervisorControlRequestV1::RecordRemoteRouteStatus { .. } => None,
        };
        if expected_generation.is_some_and(|expected| expected != state.registration_generation) {
            return Err(ControlPlaneSupervisorError::InvalidRegistration);
        }
        let outcome = match request {
            SupervisorControlRequestV1::RegisterBackend {
                registration,
                listener_ready,
                ..
            } => {
                if registration.observed_process_identity_sha256
                    != peer.observed_process_identity_sha256
                {
                    return Err(ControlPlaneSupervisorError::InvalidRegistration);
                }
                match self.register_backend(registration, listener_ready)? {
                    BackendRegistrationOutcomeV1::Registered => "REGISTERED",
                    BackendRegistrationOutcomeV1::Idempotent => "IDEMPOTENT",
                    _ => "REFUSED",
                }
            }
            SupervisorControlRequestV1::ReportBackendHealth {
                backend_id, health, ..
            } => {
                self.report_backend_health(&backend_id, health)?;
                "HEALTH_RECORDED"
            }
            SupervisorControlRequestV1::RollbackBackend { .. } => {
                self.rollback_backend()?;
                "ROLLED_BACK"
            }
            SupervisorControlRequestV1::Status => "STATUS",
            SupervisorControlRequestV1::RecordRemoteRouteStatus {
                observed_http_status,
            } => {
                self.record_remote_route_status(observed_http_status)?;
                "REMOTE_ROUTE_RECORDED"
            }
        };
        Ok(SupervisorControlResponseV1 {
            outcome: outcome.into(),
            registration_generation: self.load()?.registration_generation,
        })
    }

    /// Product-owned dry-run desired state. It never creates Scheduler/service
    /// state and has no caller-selected image, path, or command.
    pub fn fixed_startup_registration_plan(&self) -> SupervisorStartupRegistrationPlanV1 {
        match self.load() {
            Ok(state) if state.schema_version == SUPERVISOR_STATE_SCHEMA_VERSION => {
                SupervisorStartupRegistrationPlanV1::Create
            }
            Ok(_) => SupervisorStartupRegistrationPlanV1::Ambiguous,
            Err(ControlPlaneSupervisorError::InvalidState) => {
                SupervisorStartupRegistrationPlanV1::Ambiguous
            }
            Err(_) => SupervisorStartupRegistrationPlanV1::Repair,
        }
    }

    pub fn report_backend_health(
        &self,
        backend_id: &str,
        health: LocalBackendHealthV1,
    ) -> Result<(), ControlPlaneSupervisorError> {
        if !valid_backend_id(backend_id) || health == LocalBackendHealthV1::Unknown {
            return Err(ControlPlaneSupervisorError::InvalidRegistration);
        }
        let mut state = self.load()?;
        if state
            .active_backend
            .as_ref()
            .map(|backend| backend.backend_id.as_str())
            != Some(backend_id)
        {
            return Err(ControlPlaneSupervisorError::InvalidRegistration);
        }
        state.local_backend_health = health;
        state.last_failure = match health {
            LocalBackendHealthV1::Missing => Some("BACKEND_BINARY_MISSING".into()),
            LocalBackendHealthV1::Crashed => Some("BACKEND_CRASHED".into()),
            LocalBackendHealthV1::Ready | LocalBackendHealthV1::Unknown => None,
        };
        self.save(&state)
    }

    pub fn rollback_backend(&self) -> Result<(), ControlPlaneSupervisorError> {
        let mut state = self.load()?;
        let rollback = state
            .rollback_backend
            .clone()
            .ok_or(ControlPlaneSupervisorError::RollbackUnavailable)?;
        state.active_backend = Some(rollback);
        state.local_backend_health = LocalBackendHealthV1::Ready;
        state.registration_generation = state.registration_generation.saturating_add(1);
        state.last_failure = Some("BACKEND_ROLLBACK_APPLIED".into());
        self.save(&state)
    }

    /// Records an observation only. No remote runtime is created, stopped,
    /// reconfigured, or authenticated by this operation. HTTP 404 is an
    /// explicit detached route, never a connected state.
    pub fn record_remote_route_status(
        &self,
        http_status: Option<u16>,
    ) -> Result<(), ControlPlaneSupervisorError> {
        let mut state = self.load()?;
        state.remote_route_health = match http_status {
            Some(status) if (200..300).contains(&status) => RemoteRouteHealthV1::Attached,
            Some(404) => RemoteRouteHealthV1::Detached,
            _ => RemoteRouteHealthV1::Unknown,
        };
        state.last_failure = match state.remote_route_health {
            RemoteRouteHealthV1::Detached => Some("REMOTE_ROUTE_DETACHED".into()),
            _ => state.last_failure,
        };
        self.save(&state)
    }

    /// Stores only a validated non-secret tunnel ID. A missing process
    /// environment is therefore not a requirement to rediscover an existing
    /// persisted configuration identity. Credentials cannot cross this API.
    pub fn record_tunnel_identity(
        &self,
        configured_tunnel_id: &str,
    ) -> Result<(), ControlPlaneSupervisorError> {
        if !valid_tunnel_id(configured_tunnel_id) {
            return Err(ControlPlaneSupervisorError::InvalidRegistration);
        }
        let mut state = self.load()?;
        let identity = StableTunnelIdentityV1 {
            tunnel_id: configured_tunnel_id.into(),
        };
        if state
            .tunnel_identity
            .as_ref()
            .is_some_and(|known| known != &identity)
        {
            return Err(ControlPlaneSupervisorError::TunnelIdentityConflict);
        }
        state.tunnel_identity = Some(identity);
        self.save(&state)
    }

    pub fn resolve_tunnel_identity(
        &self,
        process_environment_tunnel_id: Option<&str>,
    ) -> Result<Option<StableTunnelIdentityV1>, ControlPlaneSupervisorError> {
        let state = self.load()?;
        let configured = state.tunnel_identity;
        if let Some(environment) = process_environment_tunnel_id {
            if !valid_tunnel_id(environment) {
                return Err(ControlPlaneSupervisorError::InvalidRegistration);
            }
            if configured
                .as_ref()
                .is_some_and(|known| known.tunnel_id != environment)
            {
                return Err(ControlPlaneSupervisorError::TunnelIdentityConflict);
            }
        }
        Ok(configured.or_else(|| {
            process_environment_tunnel_id.map(|tunnel_id| StableTunnelIdentityV1 {
                tunnel_id: tunnel_id.into(),
            })
        }))
    }

    /// A bounded, non-secret control response for the fixed local `/mcp`
    /// front door. It remains available even if no worker may be routed.
    pub fn front_door_status(&self) -> Result<FrontDoorStatusV1, ControlPlaneSupervisorError> {
        let state = self.load()?;
        Ok(FrontDoorStatusV1 {
            fixed_front_door_path: FIXED_FRONT_DOOR_PATH.into(),
            local_backend: state.local_backend_health.as_str().into(),
            remote_route: state.remote_route_health.as_str().into(),
            readiness: state.readiness().as_str().into(),
            active_backend_id: state.active_backend.map(|backend| backend.backend_id),
            registration_generation: state.registration_generation,
        })
    }

    /// Routes only to the compiled loopback worker endpoint. The returned
    /// decision contains no executable path, shell command, credential, or
    /// arbitrary request URL. An actual R3 host listener consumes this exact
    /// decision; R2 intentionally does not bind a live socket.
    pub fn front_door_route(&self) -> Result<FrontDoorRouteV1, ControlPlaneSupervisorError> {
        let state = self.load()?;
        let Some(backend) = state.active_backend else {
            return Ok(FrontDoorRouteV1::Unavailable("LOCAL_BACKEND_MISSING"));
        };
        if state.local_backend_health != LocalBackendHealthV1::Ready {
            return Ok(FrontDoorRouteV1::Unavailable(
                state.local_backend_health.as_str(),
            ));
        }
        if backend.endpoint != FIXED_LOCAL_BACKEND_ENDPOINT {
            return Err(ControlPlaneSupervisorError::InvalidState);
        }
        Ok(FrontDoorRouteV1::Backend {
            backend_id: backend.backend_id,
            endpoint: FIXED_LOCAL_BACKEND_ENDPOINT.into(),
            registration_generation: state.registration_generation,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontDoorStatusV1 {
    pub fixed_front_door_path: String,
    pub local_backend: String,
    pub remote_route: String,
    pub readiness: String,
    pub active_backend_id: Option<String>,
    pub registration_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrontDoorRouteV1 {
    Backend {
        backend_id: String,
        endpoint: String,
        /// Captured with the endpoint before proxying. A later handoff cannot
        /// change an in-flight request's backend selection.
        registration_generation: u64,
    },
    Unavailable(&'static str),
}

/// Builds the stable supervisor's fixed local MCP front door. Its only route
/// target is the already validated, compiled worker endpoint; callers never
/// select a URL, executable, route, manifest, tunnel, or credential.
pub fn fixed_front_door_router(store: ControlPlaneSupervisorStoreV1) -> Router {
    let proxy = SupervisorProxyV1::new(store);
    Router::new()
        .route(FIXED_FRONT_DOOR_PATH, any(proxy_mcp_request))
        .route("/status", get(front_door_get))
        .with_state(proxy)
}

async fn front_door_get(State(proxy): State<SupervisorProxyV1>) -> (StatusCode, Json<Value>) {
    let (status, body) =
        fixed_front_door_control_response(&proxy.store, "catdesk/control-plane-status");
    (status, Json(body))
}

#[derive(Clone)]
struct SupervisorProxyV1 {
    store: ControlPlaneSupervisorStoreV1,
    client: reqwest::Client,
    permits: Arc<Semaphore>,
    #[cfg(test)]
    test_worker_endpoint: Option<String>,
}

impl SupervisorProxyV1 {
    fn new(store: ControlPlaneSupervisorStoreV1) -> Self {
        // The target is never derived from this client or an incoming request.
        // Redirects are disabled so a worker cannot turn the fixed loopback
        // route into arbitrary URL authority.
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(PROXY_DEADLINE)
            .timeout(PROXY_DEADLINE)
            .build()
            .expect("fixed reqwest client configuration is valid");
        Self {
            store,
            client,
            permits: Arc::new(Semaphore::new(MAX_PROXY_CONCURRENCY)),
            #[cfg(test)]
            test_worker_endpoint: None,
        }
    }

    #[cfg(test)]
    fn new_test(store: ControlPlaneSupervisorStoreV1, endpoint: String) -> Self {
        assert!(valid_test_loopback_endpoint(&endpoint));
        let mut proxy = Self::new(store);
        proxy.test_worker_endpoint = Some(endpoint);
        proxy
    }
}

#[cfg(test)]
fn fixed_front_door_router_test_seam(
    store: ControlPlaneSupervisorStoreV1,
    endpoint: String,
) -> Router {
    let proxy = SupervisorProxyV1::new_test(store, endpoint);
    Router::new()
        .route(FIXED_FRONT_DOOR_PATH, any(proxy_mcp_request))
        .route("/status", get(front_door_get))
        .with_state(proxy)
}

#[cfg(test)]
fn valid_test_loopback_endpoint(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url
            .port()
            .is_some_and(|port| port != FIXED_SUPERVISOR_LOOPBACK_PORT)
        && url.path() == FIXED_FRONT_DOOR_PATH
        && url.query().is_none()
}

/// The route is read once, before any request bytes are sent. `Backend` carries
/// the generation captured from durable state, so N -> N+1 registration or a
/// rollback affects only later requests. Existing requests finish (or return a
/// bounded transport failure) against their originally captured worker.
async fn proxy_mcp_request(
    State(proxy): State<SupervisorProxyV1>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    if body.len() > MAX_PROXY_REQUEST_BYTES {
        return bounded_proxy_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "CONTROL_PLANE_REQUEST_TOO_LARGE",
        );
    }
    let Ok(permit) = proxy.permits.clone().try_acquire_owned() else {
        return bounded_proxy_error(StatusCode::TOO_MANY_REQUESTS, "CONTROL_PLANE_BACKPRESSURE");
    };
    let route = match proxy.store.front_door_route() {
        Ok(FrontDoorRouteV1::Backend { endpoint, .. }) => endpoint,
        Ok(FrontDoorRouteV1::Unavailable(_)) => {
            return bounded_proxy_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "CONTROL_PLANE_BACKEND_UNAVAILABLE",
            );
        }
        Err(_) => {
            return bounded_proxy_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "CONTROL_PLANE_STATE_UNAVAILABLE",
            );
        }
    };
    #[cfg(test)]
    let route = proxy.test_worker_endpoint.clone().unwrap_or(route);
    // Defense in depth: registration validation already requires this exact
    // URL, and the supervisor bind port differs from worker port 3200.
    if !allowable_proxy_endpoint(&route) {
        return bounded_proxy_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "CONTROL_PLANE_ROUTE_REFUSED",
        );
    }
    let mut request = proxy.client.request(method, route).body(body);
    request = copy_end_to_end_headers(request, &headers);
    let response = match request.send().await {
        Ok(response) => response,
        Err(_) => {
            return bounded_proxy_error(
                StatusCode::BAD_GATEWAY,
                "CONTROL_PLANE_BACKEND_UNREACHABLE",
            );
        }
    };
    if response
        .content_length()
        .is_some_and(|length| length > MAX_PROXY_RESPONSE_BYTES)
    {
        return bounded_proxy_error(StatusCode::BAD_GATEWAY, "CONTROL_PLANE_RESPONSE_TOO_LARGE");
    }
    let status = response.status();
    let response_headers = response.headers().clone();
    // Buffering is deliberate: it gives the stable front door a hard response
    // bound and avoids an unbounded stream surviving a backend generation
    // change. MCP's POST/GET/DELETE body semantics and status/headers remain
    // transparent within this documented 8 MiB transport cap.
    let bytes = match response.bytes().await {
        Ok(bytes) if bytes.len() as u64 <= MAX_PROXY_RESPONSE_BYTES => bytes,
        Ok(_) => {
            return bounded_proxy_error(
                StatusCode::BAD_GATEWAY,
                "CONTROL_PLANE_RESPONSE_TOO_LARGE",
            );
        }
        Err(_) => {
            return bounded_proxy_error(
                StatusCode::BAD_GATEWAY,
                "CONTROL_PLANE_BACKEND_STREAM_FAILED",
            );
        }
    };
    let mut builder = Response::builder().status(status);
    copy_response_headers(
        builder.headers_mut().expect("new response headers"),
        &response_headers,
    );
    // Keep the concurrency permit until the bounded response body exists.
    drop(permit);
    builder.body(Body::from(bytes)).unwrap_or_else(|_| {
        bounded_proxy_error(StatusCode::BAD_GATEWAY, "CONTROL_PLANE_RESPONSE_INVALID")
    })
}

fn allowable_proxy_endpoint(value: &str) -> bool {
    if value == FIXED_LOCAL_BACKEND_ENDPOINT {
        return true;
    }
    #[cfg(test)]
    {
        valid_test_loopback_endpoint(value)
    }
    #[cfg(not(test))]
    {
        false
    }
}

fn is_hop_by_hop_header(name: &HeaderName, headers: &HeaderMap) -> bool {
    const FIXED: [&str; 8] = [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ];
    if FIXED
        .iter()
        .any(|fixed| name.as_str().eq_ignore_ascii_case(fixed))
    {
        return true;
    }
    headers
        .get("connection")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|tokens| {
            tokens
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case(name.as_str()))
        })
}

fn copy_end_to_end_headers(
    mut request: reqwest::RequestBuilder,
    headers: &HeaderMap,
) -> reqwest::RequestBuilder {
    for (name, value) in headers {
        if !is_hop_by_hop_header(name, headers) && name != axum::http::header::HOST {
            request = request.header(name, value);
        }
    }
    request
}

fn copy_response_headers(destination: &mut HeaderMap, headers: &HeaderMap) {
    for (name, value) in headers {
        if !is_hop_by_hop_header(name, headers) {
            destination.append(name, value.clone());
        }
    }
}

fn bounded_proxy_error(status: StatusCode, code: &'static str) -> Response<Body> {
    (status, Json(json!({"error": code}))).into_response()
}

/// The sole R2 local control protocol: report bounded readiness or return the
/// compiled loopback routing decision. It is intentionally not a generic MCP
/// proxy until the R3 host listener and authenticated worker bridge exist.
pub fn fixed_front_door_control_response(
    store: &ControlPlaneSupervisorStoreV1,
    method: &str,
) -> (StatusCode, Value) {
    match method {
        "catdesk/control-plane-status" => match store.front_door_status() {
            Ok(status) => (StatusCode::OK, json!(status)),
            Err(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"readiness": "CONTROL_PLANE_STATE_UNAVAILABLE"}),
            ),
        },
        "catdesk/control-plane-route" => match store.front_door_route() {
            Ok(FrontDoorRouteV1::Backend {
                backend_id,
                endpoint,
                ..
            }) => (
                StatusCode::OK,
                json!({"route": "LOCAL_BACKEND_READY", "backendId": backend_id, "endpoint": endpoint}),
            ),
            Ok(FrontDoorRouteV1::Unavailable(reason)) => {
                (StatusCode::SERVICE_UNAVAILABLE, json!({"route": reason}))
            }
            Err(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"route": "CONTROL_PLANE_STATE_UNAVAILABLE"}),
            ),
        },
        _ => (
            StatusCode::BAD_REQUEST,
            json!({"error": "CONTROL_PLANE_METHOD_UNSUPPORTED"}),
        ),
    }
}

/// A fixed-purpose, dry-run-only staging/promotion plan. The plan knows no
/// caller-provided source path: R3's reviewed host installer supplies the
/// exact separately-built image through a private trusted deployment seam.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupervisorInstallerPlanV1 {
    pub stable_root: PathBuf,
    pub version_directory: String,
    pub staged_image_name: String,
    pub manifest_sha256: String,
    pub promote_lkg: bool,
}

pub fn reviewed_supervisor_installer_plan(
    declared_manifest_sha256: &str,
    observed_manifest_sha256: &str,
) -> Result<SupervisorInstallerPlanV1, ControlPlaneSupervisorError> {
    if !valid_sha256(declared_manifest_sha256)
        || declared_manifest_sha256 != observed_manifest_sha256
    {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    let prefix = &declared_manifest_sha256[..16];
    Ok(SupervisorInstallerPlanV1 {
        stable_root: fixed_supervisor_install_root(),
        version_directory: format!("versions/{prefix}"),
        staged_image_name: FIXED_SUPERVISOR_IMAGE_NAME.into(),
        manifest_sha256: declared_manifest_sha256.into(),
        promote_lkg: true,
    })
}

/// Validates only relative children of the fixed root. R2 invokes this in
/// dry-run tests; R3 must additionally use handle-relative no-follow opens on
/// Windows before writing the plan's side-by-side directory or LKG pointer.
pub fn validated_fixed_install_child(
    relative: &str,
) -> Result<PathBuf, ControlPlaneSupervisorError> {
    let path = Path::new(relative);
    if relative.is_empty()
        || relative.len() > 512
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    Ok(fixed_supervisor_install_root().join(path))
}

/// Fixed-purpose side-by-side installer writer. The production constructor is
/// compiled to the product-owned ProgramData root; the only byte input is an
/// internal reviewed-image capability used by a later administrator surface,
/// never a CLI/MCP path or command. Tests inject only temporary roots.
#[derive(Clone, Debug)]
pub struct SupervisorInstallerWriterV1 {
    root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorInstallReceiptV1 {
    pub manifest_sha256: String,
    pub version_directory: String,
}

/// Fixed-purpose snapshot for the supervisor lifecycle transaction.  It is
/// obtained only from the fixed protected root and retains exact receipt bytes
/// or a positively established handle-relative absence for each fixed child.
/// It has no public constructor, path, or generic deletion authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SupervisorInstallReceiptSnapshotV1 {
    current: SupervisorReceiptPresenceV1,
    lkg: SupervisorReceiptPresenceV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SupervisorReceiptPresenceV1 {
    Absent,
    Present {
        receipt: SupervisorInstallReceiptV1,
        bytes: Vec<u8>,
    },
}

impl SupervisorInstallerWriterV1 {
    pub fn fixed_policy() -> Self {
        Self {
            root: fixed_supervisor_install_root(),
        }
    }
    #[cfg(test)]
    pub(crate) fn test_seam(root: PathBuf) -> Self {
        Self { root }
    }

    /// This is deliberately crate-private: public operator surfaces cannot
    /// pass bytes, paths, manifests, or policy values to the writer.
    pub(crate) fn install_exact_reviewed_image(
        &self,
        reviewed_bytes: &[u8],
        expected_manifest_sha256: &str,
    ) -> Result<SupervisorInstallReceiptV1, ControlPlaneSupervisorError> {
        let receipt =
            self.prepare_exact_reviewed_image(reviewed_bytes, expected_manifest_sha256)?;
        self.commit_prepared_exact_reviewed_image(&receipt)?;
        Ok(receipt)
    }

    /// Creates or verifies the reviewed version directory but deliberately
    /// does not advance `supervisor-current.json` or LKG.  The staged version
    /// is inert until the lifecycle transaction later commits this receipt.
    pub(crate) fn prepare_exact_reviewed_image(
        &self,
        reviewed_bytes: &[u8],
        expected_manifest_sha256: &str,
    ) -> Result<SupervisorInstallReceiptV1, ControlPlaneSupervisorError> {
        validate_reviewed_supervisor_image_bytes(reviewed_bytes, expected_manifest_sha256)?;
        #[cfg(windows)]
        {
            let root = protected_supervisor_root(&self.root, true)?;
            // The handle itself owns deletion at close. A stale lock or a
            // substituted lock name is refused; this ticket deliberately adds
            // no pathname stale-lock cleanup authority.
            let _lock = create_delete_on_close_regular_lock(
                &root,
                INSTALL_WRITER_LOCK,
                "supervisor install lock",
            )
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
            self.prepare_locked_protected(&root, reviewed_bytes, expected_manifest_sha256)
        }
        #[cfg(not(windows))]
        Err(ControlPlaneSupervisorError::InvalidState)
    }

    /// Advances the protected current receipt only after a prior inert version
    /// receipt has been freshly re-opened and re-hashed under the fixed root.
    /// This is intentionally separate from prepare so scheduler registration
    /// failure cannot advance current state.
    pub(crate) fn commit_prepared_exact_reviewed_image(
        &self,
        receipt: &SupervisorInstallReceiptV1,
    ) -> Result<(), ControlPlaneSupervisorError> {
        #[cfg(windows)]
        {
            let root = protected_supervisor_root(&self.root, false)?;
            let _lock = create_delete_on_close_regular_lock(
                &root,
                INSTALL_WRITER_LOCK,
                "supervisor install lock",
            )
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
            receipt_matches_pinned_image(&root, receipt)?;
            let current = self.read_install_receipt_protected(&root, INSTALL_CURRENT_FILE)?;
            if current.as_ref() != Some(receipt) {
                self.write_install_receipt_protected(&root, INSTALL_LKG_FILE, current.as_ref())?;
            }
            self.write_install_receipt_protected(&root, INSTALL_CURRENT_FILE, Some(receipt))
        }
        #[cfg(not(windows))]
        {
            let _ = receipt;
            Err(ControlPlaneSupervisorError::InvalidState)
        }
    }

    /// Snapshots the exact current and LKG receipt pair under one pinned root
    /// before the lifecycle performs any mutation. Absence comes only from
    /// `read_optional_relative_regular`'s no-follow RootDirectory-relative
    /// missing-child outcome; it is never inferred from a pathname probe.
    pub(crate) fn snapshot_exact_install_receipts(
        &self,
    ) -> Result<SupervisorInstallReceiptSnapshotV1, ControlPlaneSupervisorError> {
        #[cfg(windows)]
        {
            let root = protected_supervisor_root(&self.root, false)?;
            let _lock = create_delete_on_close_regular_lock(
                &root,
                INSTALL_WRITER_LOCK,
                "supervisor install lock",
            )
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
            let current = self.snapshot_install_receipt_presence(&root, INSTALL_CURRENT_FILE)?;
            let lkg = self.snapshot_install_receipt_presence(&root, INSTALL_LKG_FILE)?;
            Ok(SupervisorInstallReceiptSnapshotV1 { current, lkg })
        }
        #[cfg(not(windows))]
        Err(ControlPlaneSupervisorError::InvalidState)
    }

    /// Restores only a snapshot obtained above, and only if the live pair is
    /// still one of the two states this transaction can have produced.  It is
    /// deliberately not a generic receipt writer or recovery API.
    pub(crate) fn restore_exact_install_receipt_snapshot(
        &self,
        snapshot: &SupervisorInstallReceiptSnapshotV1,
        prepared: &SupervisorInstallReceiptV1,
    ) -> Result<(), ControlPlaneSupervisorError> {
        #[cfg(windows)]
        {
            let root = protected_supervisor_root(&self.root, false)?;
            let _lock = create_delete_on_close_regular_lock(
                &root,
                INSTALL_WRITER_LOCK,
                "supervisor install lock",
            )
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
            receipt_matches_pinned_image(&root, prepared)?;
            let current = self.snapshot_install_receipt_presence(&root, INSTALL_CURRENT_FILE)?;
            let lkg = self.snapshot_install_receipt_presence(&root, INSTALL_LKG_FILE)?;
            if !receipt_presence_is_transaction_current(&current, &snapshot.current, prepared)
                || !receipt_presence_is_transaction_lkg(&lkg, &snapshot.lkg, &snapshot.current)
            {
                return Err(ControlPlaneSupervisorError::InvalidState);
            }
            self.restore_install_receipt_presence(&root, INSTALL_LKG_FILE, &snapshot.lkg, &lkg)?;
            self.restore_install_receipt_presence(
                &root,
                INSTALL_CURRENT_FILE,
                &snapshot.current,
                &current,
            )
        }
        #[cfg(not(windows))]
        {
            let _ = (snapshot, prepared);
            Err(ControlPlaneSupervisorError::InvalidState)
        }
    }

    #[cfg(windows)]
    fn prepare_locked_protected(
        &self,
        root: &ProtectedDirectoryGuard,
        bytes: &[u8],
        digest: &str,
    ) -> Result<SupervisorInstallReceiptV1, ControlPlaneSupervisorError> {
        let version_directory = format!("versions/{}", &digest[..16]);
        let version_name = &digest[..16];
        let mut versions = root
            .try_clone("supervisor versions root")
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
        versions
            .descend_or_create("versions", "supervisor versions root")
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
        let mut version = versions
            .try_clone("supervisor version")
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
        if version
            .descend_optional_existing(version_name, "supervisor version")
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)?
        {
            let image = read_relative_regular(
                &version,
                FIXED_SUPERVISOR_IMAGE_NAME,
                256 * 1024 * 1024,
                "supervisor reviewed image",
            )
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
            if sha256_hex(&image) != digest {
                return Err(ControlPlaneSupervisorError::InvalidState);
            }
        } else {
            let mut staging = versions
                .try_clone("supervisor staging")
                .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
            staging
                .create_unique_renameable_child("supervisor staging")
                .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
            write_new_regular_in(
                &staging,
                FIXED_SUPERVISOR_IMAGE_NAME,
                bytes,
                "supervisor staged image",
            )
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
            let staged_image = read_relative_regular(
                &staging,
                FIXED_SUPERVISOR_IMAGE_NAME,
                256 * 1024 * 1024,
                "supervisor staged image",
            )
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
            if sha256_hex(&staged_image) != digest {
                return Err(ControlPlaneSupervisorError::InvalidState);
            }
            staging
                .rename_direct_child_within_parent(
                    &versions,
                    version_name,
                    "supervisor version commit",
                )
                .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
        }
        let receipt = SupervisorInstallReceiptV1 {
            manifest_sha256: digest.into(),
            version_directory,
        };
        Ok(receipt)
    }

    #[cfg(windows)]
    fn read_install_receipt_protected(
        &self,
        root: &ProtectedDirectoryGuard,
        name: &str,
    ) -> Result<Option<SupervisorInstallReceiptV1>, ControlPlaneSupervisorError> {
        match read_optional_relative_regular(root, name, MAX_STATE_BYTES, "supervisor receipt")
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)?
        {
            Some(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| ControlPlaneSupervisorError::InvalidState),
            None => Ok(None),
        }
    }

    #[cfg(windows)]
    fn snapshot_install_receipt_presence(
        &self,
        root: &ProtectedDirectoryGuard,
        name: &str,
    ) -> Result<SupervisorReceiptPresenceV1, ControlPlaneSupervisorError> {
        let Some(bytes) =
            read_optional_relative_regular(root, name, MAX_STATE_BYTES, "supervisor receipt")
                .map_err(|_| ControlPlaneSupervisorError::InvalidState)?
        else {
            return Ok(SupervisorReceiptPresenceV1::Absent);
        };
        let receipt: SupervisorInstallReceiptV1 = serde_json::from_slice(&bytes)
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
        receipt_matches_pinned_image(root, &receipt)?;
        Ok(SupervisorReceiptPresenceV1::Present { receipt, bytes })
    }

    #[cfg(windows)]
    fn restore_install_receipt_presence(
        &self,
        root: &ProtectedDirectoryGuard,
        name: &str,
        prior: &SupervisorReceiptPresenceV1,
        live: &SupervisorReceiptPresenceV1,
    ) -> Result<(), ControlPlaneSupervisorError> {
        match (prior, live) {
            (SupervisorReceiptPresenceV1::Absent, SupervisorReceiptPresenceV1::Absent) => Ok(()),
            (
                SupervisorReceiptPresenceV1::Absent,
                SupervisorReceiptPresenceV1::Present { bytes, .. },
            ) => {
                // Exact object/bytes only; the shared primitive reopens no
                // pathname and proves absence after disposition finalizes.
                remove_exact_relative_regular_with_bytes(
                    root,
                    name,
                    bytes,
                    "supervisor receipt rollback",
                )
                .map_err(|_| ControlPlaneSupervisorError::InvalidState)
            }
            (SupervisorReceiptPresenceV1::Present { receipt, bytes }, _) => {
                receipt_matches_pinned_image(root, receipt)?;
                self.write_install_receipt_bytes_protected(root, name, receipt, bytes)
            }
        }
    }

    #[cfg(windows)]
    fn write_install_receipt_protected(
        &self,
        root: &ProtectedDirectoryGuard,
        name: &str,
        receipt: Option<&SupervisorInstallReceiptV1>,
    ) -> Result<(), ControlPlaneSupervisorError> {
        let Some(receipt) = receipt else {
            return Ok(());
        };
        if !valid_sha256(&receipt.manifest_sha256)
            || validated_relative_install_path(&receipt.version_directory).is_err()
        {
            return Err(ControlPlaneSupervisorError::InvalidState);
        }
        let bytes = serde_json::to_vec(receipt).map_err(|_| ControlPlaneSupervisorError::Io)?;
        write_unique_regular_for_atomic_replace(root, &bytes, "supervisor receipt temporary")
            .and_then(|temporary| temporary.commit_replace(name, "supervisor receipt commit"))
            .map_err(|_| ControlPlaneSupervisorError::InvalidState)
    }

    #[cfg(windows)]
    fn write_install_receipt_bytes_protected(
        &self,
        root: &ProtectedDirectoryGuard,
        name: &str,
        receipt: &SupervisorInstallReceiptV1,
        bytes: &[u8],
    ) -> Result<(), ControlPlaneSupervisorError> {
        let parsed: SupervisorInstallReceiptV1 =
            serde_json::from_slice(bytes).map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
        if &parsed != receipt {
            return Err(ControlPlaneSupervisorError::InvalidState);
        }
        write_unique_regular_for_atomic_replace(
            root,
            bytes,
            "supervisor receipt rollback temporary",
        )
        .and_then(|temporary| temporary.commit_replace(name, "supervisor receipt rollback commit"))
        .map_err(|_| ControlPlaneSupervisorError::InvalidState)
    }
}

fn receipt_presence_receipt(
    value: &SupervisorReceiptPresenceV1,
) -> Option<&SupervisorInstallReceiptV1> {
    match value {
        SupervisorReceiptPresenceV1::Absent => None,
        SupervisorReceiptPresenceV1::Present { receipt, .. } => Some(receipt),
    }
}

fn receipt_presence_is_transaction_current(
    live: &SupervisorReceiptPresenceV1,
    prior: &SupervisorReceiptPresenceV1,
    prepared: &SupervisorInstallReceiptV1,
) -> bool {
    live == prior || receipt_presence_receipt(live) == Some(prepared)
}

fn receipt_presence_is_transaction_lkg(
    live: &SupervisorReceiptPresenceV1,
    prior_lkg: &SupervisorReceiptPresenceV1,
    prior_current: &SupervisorReceiptPresenceV1,
) -> bool {
    live == prior_lkg || receipt_presence_receipt(live) == receipt_presence_receipt(prior_current)
}

fn validate_reviewed_supervisor_image_bytes(
    reviewed_bytes: &[u8],
    expected_manifest_sha256: &str,
) -> Result<(), ControlPlaneSupervisorError> {
    if reviewed_bytes.is_empty()
        || reviewed_bytes.len() > 256 * 1024 * 1024
        || !valid_sha256(expected_manifest_sha256)
        || sha256_hex(reviewed_bytes) != expected_manifest_sha256
    {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    Ok(())
}

/// Checks the fixed supervisor install/state using the same no-follow,
/// handle-relative authority as mutation.  This is deliberately a report,
/// not an installation or activation API.
pub fn assess_fixed_supervisor_activation_readiness() -> SupervisorActivationReadinessV1 {
    assess_supervisor_activation_readiness_at(&fixed_supervisor_install_root())
}

/// Returns the one receipt-bound installed image pathname needed by the native
/// Task Scheduler action.  The path is not an authority supplied by an
/// operator: before it is returned the fixed protected root, current receipt,
/// relative version layout, and exact opened image digest have all been
/// revalidated through the same pinned/no-follow authority used by the
/// installer.  Task Scheduler itself requires a path string for its native
/// action record; no filesystem operation is reopened through this path here.
#[cfg(windows)]
pub(crate) fn fixed_current_supervisor_startup_action_path()
-> Result<PathBuf, ControlPlaneSupervisorError> {
    let root_path = fixed_supervisor_install_root();
    let root = protected_supervisor_root(&root_path, false)?;
    let receipt = read_install_receipt_from_guard(&root, INSTALL_CURRENT_FILE)?
        .ok_or(ControlPlaneSupervisorError::InvalidState)?;
    receipt_matches_pinned_image(&root, &receipt)?;
    root.assert_stable("supervisor startup action root")
        .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
    Ok(root_path
        .join(receipt.version_directory)
        .join(FIXED_SUPERVISOR_IMAGE_NAME))
}

/// Computes the inert side-by-side action path for an already authenticated
/// reviewed supervisor payload.  Unlike the current-receipt bridge, this
/// requires no existing install state: lifecycle code uses it to prepare a
/// clean first-install task definition before the current receipt is advanced.
/// The digest is internal role-capability evidence, never an operator input.
pub(crate) fn planned_reviewed_supervisor_startup_action_path(
    reviewed_manifest_sha256: &str,
) -> Result<PathBuf, ControlPlaneSupervisorError> {
    if !valid_sha256(reviewed_manifest_sha256) {
        return Err(ControlPlaneSupervisorError::InvalidState);
    }
    Ok(fixed_supervisor_install_root()
        .join("versions")
        .join(&reviewed_manifest_sha256[..16])
        .join(FIXED_SUPERVISOR_IMAGE_NAME))
}

#[cfg(not(windows))]
pub(crate) fn fixed_current_supervisor_startup_action_path()
-> Result<PathBuf, ControlPlaneSupervisorError> {
    Err(ControlPlaneSupervisorError::InvalidState)
}

#[cfg(test)]
pub(crate) fn assess_supervisor_activation_readiness_test_seam(
    root: &Path,
) -> SupervisorActivationReadinessV1 {
    assess_supervisor_activation_readiness_at(root)
}

fn assess_supervisor_activation_readiness_at(root_path: &Path) -> SupervisorActivationReadinessV1 {
    if FIXED_SUPERVISOR_LOOPBACK_HOST != "127.0.0.1"
        || FIXED_SUPERVISOR_LOOPBACK_PORT != 3201
        || FIXED_LOCAL_BACKEND_PORT != 3200
        || FIXED_LOCAL_BACKEND_ENDPOINT != "http://127.0.0.1:3200/mcp"
        || FIXED_SUPERVISOR_CONTROL_PIPE != r"\\.\pipe\CatDeskControlPlaneSupervisorV1"
    {
        return SupervisorActivationReadinessV1::FixedPolicyInvalid;
    }
    #[cfg(windows)]
    {
        if let Err(reason) = readiness_policy_gate(
            crate::windows_supervisor_control_pipe::fixed_pipe_principal_policy_descriptor(),
            stable_supervisor_runtime_capability_descriptor(),
        ) {
            return reason;
        }
        let root = match protected_supervisor_root(root_path, false) {
            Ok(root) => root,
            Err(_) => return SupervisorActivationReadinessV1::RootUnavailable,
        };
        let store = ControlPlaneSupervisorStoreV1 {
            root: root_path.to_path_buf(),
        };
        if store.load().is_err() {
            return SupervisorActivationReadinessV1::StateInvalid;
        }
        let current = match read_install_receipt_from_guard(&root, INSTALL_CURRENT_FILE) {
            Ok(Some(receipt)) => receipt,
            Ok(None) => return SupervisorActivationReadinessV1::CurrentReceiptMissing,
            Err(_) => return SupervisorActivationReadinessV1::ReceiptOrImageInvalid,
        };
        if receipt_matches_pinned_image(&root, &current).is_err() {
            return SupervisorActivationReadinessV1::ReceiptOrImageInvalid;
        }
        match read_install_receipt_from_guard(&root, INSTALL_LKG_FILE) {
            Ok(Some(receipt)) if receipt_matches_pinned_image(&root, &receipt).is_err() => {
                return SupervisorActivationReadinessV1::ReceiptOrImageInvalid;
            }
            Ok(_) => {}
            Err(_) => return SupervisorActivationReadinessV1::ReceiptOrImageInvalid,
        }
        SupervisorActivationReadinessV1::Ready
    }
    #[cfg(not(windows))]
    {
        let _ = root_path;
        SupervisorActivationReadinessV1::RootUnavailable
    }
}

#[cfg(windows)]
fn readiness_policy_gate(
    principal: crate::windows_supervisor_control_pipe::FixedPipePrincipalPolicyDescriptorV1,
    runtime: StableSupervisorRuntimeCapabilityV1,
) -> Result<(), SupervisorActivationReadinessV1> {
    crate::windows_supervisor_control_pipe::require_fixed_pipe_principal_policy(principal)
        .map_err(|_| SupervisorActivationReadinessV1::PrincipalPolicyUnproven)?;
    require_stable_supervisor_runtime_capability(runtime)
        .map_err(|_| SupervisorActivationReadinessV1::RuntimeOwnershipUnproven)
}

#[cfg(windows)]
fn read_install_receipt_from_guard(
    root: &ProtectedDirectoryGuard,
    name: &str,
) -> Result<Option<SupervisorInstallReceiptV1>, ControlPlaneSupervisorError> {
    match read_optional_relative_regular(root, name, MAX_STATE_BYTES, "supervisor receipt")
        .map_err(|_| ControlPlaneSupervisorError::InvalidState)?
    {
        Some(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| ControlPlaneSupervisorError::InvalidState),
        None => Ok(None),
    }
}

#[cfg(windows)]
fn receipt_matches_pinned_image(
    root: &ProtectedDirectoryGuard,
    receipt: &SupervisorInstallReceiptV1,
) -> Result<(), ControlPlaneSupervisorError> {
    if !valid_sha256(&receipt.manifest_sha256)
        || receipt.version_directory != format!("versions/{}", &receipt.manifest_sha256[..16])
    {
        return Err(ControlPlaneSupervisorError::InvalidState);
    }
    let mut version = root
        .try_clone("supervisor receipt root")
        .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
    version
        .descend_existing("versions", "supervisor receipt versions")
        .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
    version
        .descend_existing(&receipt.manifest_sha256[..16], "supervisor receipt version")
        .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
    let image = read_relative_regular(
        &version,
        FIXED_SUPERVISOR_IMAGE_NAME,
        256 * 1024 * 1024,
        "supervisor receipt image",
    )
    .map_err(|_| ControlPlaneSupervisorError::InvalidState)?;
    if sha256_hex(&image) != receipt.manifest_sha256 {
        return Err(ControlPlaneSupervisorError::InvalidState);
    }
    Ok(())
}

fn validated_relative_install_path(value: &str) -> Result<(), ControlPlaneSupervisorError> {
    if value.is_empty()
        || value.len() > 256
        || Path::new(value).is_absolute()
        || Path::new(value)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(ControlPlaneSupervisorError::InvalidState);
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validate_state(
    state: &ControlPlaneSupervisorStateV1,
) -> Result<(), ControlPlaneSupervisorError> {
    if state.schema_version != SUPERVISOR_STATE_SCHEMA_VERSION
        || state.fixed_front_door_path != FIXED_FRONT_DOOR_PATH
        || state.last_failure.as_ref().is_some_and(|reason| {
            !matches!(
                reason.as_str(),
                "BACKEND_MANIFEST_MISMATCH"
                    | "BACKEND_LOCAL_LISTENER_UNAVAILABLE"
                    | "BACKEND_BINARY_MISSING"
                    | "BACKEND_CRASHED"
                    | "BACKEND_ROLLBACK_APPLIED"
                    | "REMOTE_ROUTE_DETACHED"
                    | "BACKEND_LISTENER_IDENTITY_MISMATCH"
            )
        })
        || state
            .active_backend
            .as_ref()
            .is_some_and(|backend| validate_registration(backend).is_err())
        || state
            .rollback_backend
            .as_ref()
            .is_some_and(|backend| validate_registration(backend).is_err())
        || state
            .tunnel_identity
            .as_ref()
            .is_some_and(|identity| !valid_tunnel_id(&identity.tunnel_id))
    {
        return Err(ControlPlaneSupervisorError::InvalidState);
    }
    Ok(())
}

fn validate_registration(
    registration: &WorkerBackendRegistrationV1,
) -> Result<(), ControlPlaneSupervisorError> {
    if !valid_backend_id(&registration.backend_id)
        || !valid_sha256(&registration.declared_manifest_sha256)
        || !valid_sha256(&registration.observed_manifest_sha256)
        || registration.endpoint != FIXED_LOCAL_BACKEND_ENDPOINT
        || !valid_sha256(&registration.expected_process_identity_sha256)
        || !valid_sha256(&registration.observed_process_identity_sha256)
    {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    Ok(())
}

fn valid_backend_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_tunnel_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn store() -> ControlPlaneSupervisorStoreV1 {
        let root = std::env::temp_dir().join(format!("catdesk-control-plane-{}", Uuid::new_v4()));
        ControlPlaneSupervisorStoreV1::open_test_seam(root).expect("temporary supervisor state")
    }

    fn backend(id: &str) -> WorkerBackendRegistrationV1 {
        WorkerBackendRegistrationV1 {
            backend_id: id.into(),
            declared_manifest_sha256: "a".repeat(64),
            observed_manifest_sha256: "a".repeat(64),
            endpoint: FIXED_LOCAL_BACKEND_ENDPOINT.into(),
            expected_process_identity_sha256: "b".repeat(64),
            observed_process_identity_sha256: "b".repeat(64),
        }
    }

    #[test]
    fn fixed_supervisor_prestate_projects_only_validated_backend_identities() {
        let mut state = ControlPlaneSupervisorStateV1 {
            local_backend_health: LocalBackendHealthV1::Ready,
            registration_generation: 7,
            active_backend: Some(backend("active")),
            rollback_backend: Some(backend("rollback")),
            ..Default::default()
        };
        let projection = project_supervisor_prestate_test_seam(
            state.clone(),
            SupervisorActivationReadinessV1::Ready,
        );
        assert_eq!(projection.registration_generation, 7);
        assert_eq!(projection.local_backend_health, "LOCAL_BACKEND_READY");
        assert_eq!(
            projection
                .active_backend
                .as_ref()
                .unwrap()
                .declared_manifest_sha256,
            "a".repeat(64)
        );
        assert!(
            !serde_json::to_string(&projection)
                .unwrap()
                .contains("endpoint")
        );

        state.rollback_backend.as_mut().unwrap().endpoint = "http://127.0.0.1:1/mcp".into();
        assert!(validate_state(&state).is_err());
        state.rollback_backend = Some(backend("rollback"));
        state.registration_generation = 0;
        let projection = project_supervisor_prestate_test_seam(
            state,
            SupervisorActivationReadinessV1::ReceiptOrImageInvalid,
        );
        assert_eq!(projection.registration_generation, 0);
        assert_eq!(
            projection.activation_readiness,
            "SUPERVISOR_RECEIPT_OR_IMAGE_INVALID"
        );
    }

    #[test]
    fn local_backend_and_remote_route_health_are_independent() {
        let store = store();
        assert_eq!(
            store
                .register_backend(backend("worker-a"), true)
                .expect("register"),
            BackendRegistrationOutcomeV1::Registered
        );
        store
            .record_remote_route_status(Some(404))
            .expect("remote 404");
        let state = store.load().expect("state");
        assert_eq!(state.local_backend_health, LocalBackendHealthV1::Ready);
        assert_eq!(state.remote_route_health, RemoteRouteHealthV1::Detached);
        assert_eq!(
            state.readiness(),
            ControlPlaneReadinessV1::RemoteRouteDetached
        );
        store
            .record_remote_route_status(Some(204))
            .expect("remote attached");
        assert_eq!(
            store.load().expect("state").readiness(),
            ControlPlaneReadinessV1::Ready
        );
    }

    #[test]
    fn missing_crashed_and_manifest_mismatch_preserve_supervisor_state() {
        let store = store();
        store
            .register_backend(backend("worker-a"), true)
            .expect("register");
        store.record_remote_route_status(Some(200)).expect("route");
        store
            .report_backend_health("worker-a", LocalBackendHealthV1::Crashed)
            .expect("crash");
        assert_eq!(
            store.load().expect("crashed state").readiness(),
            ControlPlaneReadinessV1::LocalBackendUnavailable
        );
        let mut mismatched = backend("worker-b");
        mismatched.observed_manifest_sha256 = "b".repeat(64);
        assert_eq!(
            store.register_backend(mismatched, true).expect("refusal"),
            BackendRegistrationOutcomeV1::RefusedManifestMismatch
        );
        let state = store.load().expect("state survives");
        assert_eq!(
            state.active_backend.expect("old backend").backend_id,
            "worker-a"
        );
        assert_eq!(state.local_backend_health, LocalBackendHealthV1::Crashed);
        store
            .report_backend_health("worker-a", LocalBackendHealthV1::Missing)
            .expect("missing");
        assert_eq!(
            store.load().expect("missing state").local_backend_health,
            LocalBackendHealthV1::Missing
        );
    }

    #[test]
    fn handoff_failure_rolls_back_and_registration_is_idempotent_after_restart() {
        let store = store();
        store
            .register_backend(backend("worker-a"), true)
            .expect("first");
        assert_eq!(
            store
                .register_backend(backend("worker-a"), true)
                .expect("idempotent"),
            BackendRegistrationOutcomeV1::Idempotent
        );
        assert_eq!(
            store
                .register_backend(backend("worker-b"), false)
                .expect("failed handoff"),
            BackendRegistrationOutcomeV1::RefusedBackendUnavailable
        );
        assert_eq!(
            store
                .load()
                .expect("old backend")
                .active_backend
                .expect("backend")
                .backend_id,
            "worker-a"
        );
        store
            .register_backend(backend("worker-b"), true)
            .expect("handoff");
        store.rollback_backend().expect("rollback");
        let restarted = store.load().expect("supervisor restart");
        assert_eq!(
            restarted
                .active_backend
                .expect("rollback backend")
                .backend_id,
            "worker-a"
        );
        assert_eq!(restarted.local_backend_health, LocalBackendHealthV1::Ready);
    }

    #[test]
    fn persisted_tunnel_identity_survives_missing_process_environment_without_secrets() {
        let store = store();
        store
            .record_tunnel_identity("tun_existing_identity")
            .expect("persist identity");
        assert_eq!(
            store
                .resolve_tunnel_identity(None)
                .expect("no process environment")
                .expect("persisted tunnel")
                .tunnel_id,
            "tun_existing_identity"
        );
        assert!(matches!(
            store.resolve_tunnel_identity(Some("tun_different")),
            Err(ControlPlaneSupervisorError::TunnelIdentityConflict)
        ));
        let state = serde_json::to_string(&store.load().expect("state")).expect("serialize");
        assert!(!state.contains("CONTROL_PLANE_API_KEY"));
        assert!(!state.to_ascii_lowercase().contains("credential"));
    }

    #[test]
    fn fixed_front_door_stays_bounded_and_routes_only_an_exact_ready_backend() {
        let store = store();
        assert_eq!(
            store.front_door_route().expect("missing worker route"),
            FrontDoorRouteV1::Unavailable("LOCAL_BACKEND_MISSING")
        );
        let missing = store.front_door_status().expect("missing status");
        assert_eq!(missing.fixed_front_door_path, "/mcp");
        assert_eq!(missing.readiness, "CONTROL_PLANE_LOCAL_BACKEND_UNAVAILABLE");

        store
            .register_backend(backend("worker-a"), true)
            .expect("register");
        assert_eq!(
            store.front_door_route().expect("fixed route"),
            FrontDoorRouteV1::Backend {
                backend_id: "worker-a".into(),
                endpoint: FIXED_LOCAL_BACKEND_ENDPOINT.into(),
                registration_generation: 1,
            }
        );
        store
            .report_backend_health("worker-a", LocalBackendHealthV1::Crashed)
            .expect("crash");
        assert_eq!(
            store.front_door_route().expect("crashed route"),
            FrontDoorRouteV1::Unavailable("LOCAL_BACKEND_CRASHED")
        );
        let (status, response) =
            fixed_front_door_control_response(&store, "catdesk/control-plane-route");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response, json!({"route": "LOCAL_BACKEND_CRASHED"}));
        let (status, response) = fixed_front_door_control_response(&store, "evil/command");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            response,
            json!({"error": "CONTROL_PLANE_METHOD_UNSUPPORTED"})
        );
    }

    #[test]
    fn listener_identity_mismatch_and_bad_endpoint_cannot_replace_active_backend() {
        let store = store();
        store
            .register_backend(backend("worker-a"), true)
            .expect("old backend");
        let mut wrong_identity = backend("worker-b");
        wrong_identity.observed_process_identity_sha256 = "c".repeat(64);
        assert_eq!(
            store
                .register_backend(wrong_identity, true)
                .expect("identity refusal"),
            BackendRegistrationOutcomeV1::RefusedListenerIdentityMismatch
        );
        let mut arbitrary_endpoint = backend("worker-c");
        arbitrary_endpoint.endpoint = "https://example.invalid/mcp".into();
        assert_eq!(
            store.register_backend(arbitrary_endpoint, true),
            Err(ControlPlaneSupervisorError::InvalidRegistration)
        );
        assert_eq!(
            store
                .load()
                .expect("state")
                .active_backend
                .expect("old")
                .backend_id,
            "worker-a"
        );
    }

    #[test]
    fn installer_plan_is_fixed_side_by_side_and_rejects_paths_or_manifest_drift() {
        let manifest = "d".repeat(64);
        let plan = reviewed_supervisor_installer_plan(&manifest, &manifest).expect("plan");
        assert_eq!(plan.stable_root, fixed_supervisor_install_root());
        assert_eq!(plan.staged_image_name, FIXED_SUPERVISOR_IMAGE_NAME);
        assert!(plan.version_directory.starts_with("versions/"));
        assert!(plan.promote_lkg);
        assert!(reviewed_supervisor_installer_plan(&manifest, &"e".repeat(64)).is_err());
        assert!(validated_fixed_install_child("versions/abcd/image.exe").is_ok());
        for unsafe_child in ["../worker.exe", "C:/worker.exe", "versions/../lkg", ""] {
            assert!(
                validated_fixed_install_child(unsafe_child).is_err(),
                "{unsafe_child}"
            );
        }
    }

    #[test]
    fn temporary_root_installer_is_idempotent_keeps_lkg_and_rejects_digest_drift() {
        let root =
            std::env::temp_dir().join(format!("catdesk-supervisor-install-{}", Uuid::new_v4()));
        let writer = SupervisorInstallerWriterV1::test_seam(root.clone());
        let one = b"reviewed supervisor image one";
        let one_digest = sha256_hex(one);
        let first = writer
            .install_exact_reviewed_image(one, &one_digest)
            .expect("first install");
        assert_eq!(
            writer
                .install_exact_reviewed_image(one, &one_digest)
                .expect("idempotent"),
            first
        );
        assert!(
            writer
                .install_exact_reviewed_image(one, &"0".repeat(64))
                .is_err()
        );
        let two = b"reviewed supervisor image two";
        let two_digest = sha256_hex(two);
        let second = writer
            .install_exact_reviewed_image(two, &two_digest)
            .expect("update");
        assert_ne!(first.version_directory, second.version_directory);
        let protected_root =
            protected_supervisor_root(&root, false).expect("protected fixture root");
        let lkg = writer
            .read_install_receipt_protected(&protected_root, INSTALL_LKG_FILE)
            .expect("lkg")
            .expect("prior");
        assert_eq!(lkg, first);
        assert!(
            root.join(&second.version_directory)
                .join(FIXED_SUPERVISOR_IMAGE_NAME)
                .is_file()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn prepared_reviewed_version_is_inert_until_explicit_current_commit() {
        let root =
            std::env::temp_dir().join(format!("catdesk-supervisor-prepare-{}", Uuid::new_v4()));
        let writer = SupervisorInstallerWriterV1::test_seam(root.clone());
        let bytes = b"prepared reviewed image";
        let digest = sha256_hex(bytes);
        let receipt = writer
            .prepare_exact_reviewed_image(bytes, &digest)
            .expect("prepare isolated reviewed version");
        assert!(root.join("versions").is_dir());
        assert!(
            !root.join(INSTALL_CURRENT_FILE).exists(),
            "prepare may not advance current receipt"
        );
        writer
            .commit_prepared_exact_reviewed_image(&receipt)
            .expect("commit exact prepared receipt");
        assert!(root.join(INSTALL_CURRENT_FILE).is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn protected_receipt_snapshot_restores_exact_current_and_lkg_after_a_later_commit() {
        let root = std::env::temp_dir().join(format!(
            "catdesk-supervisor-receipt-rollback-{}",
            Uuid::new_v4()
        ));
        let writer = SupervisorInstallerWriterV1::test_seam(root.clone());
        let first_bytes = b"reviewed supervisor first";
        let first = writer
            .install_exact_reviewed_image(first_bytes, &sha256_hex(first_bytes))
            .expect("first receipt");
        let second_bytes = b"reviewed supervisor second";
        let second = writer
            .install_exact_reviewed_image(second_bytes, &sha256_hex(second_bytes))
            .expect("second receipt");
        let snapshot = writer
            .snapshot_exact_install_receipts()
            .expect("snapshot current and lkg");
        let third_bytes = b"reviewed supervisor third";
        let third = writer
            .prepare_exact_reviewed_image(third_bytes, &sha256_hex(third_bytes))
            .expect("prepare third");
        writer
            .commit_prepared_exact_reviewed_image(&third)
            .expect("commit third");
        writer
            .restore_exact_install_receipt_snapshot(&snapshot, &third)
            .expect("restore only exact transaction states");
        let protected_root = protected_supervisor_root(&root, false).expect("protected root");
        assert_eq!(
            writer
                .read_install_receipt_protected(&protected_root, INSTALL_CURRENT_FILE)
                .expect("current")
                .expect("present"),
            second
        );
        assert_eq!(
            writer
                .read_install_receipt_protected(&protected_root, INSTALL_LKG_FILE)
                .expect("lkg")
                .expect("present"),
            first
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn protected_receipt_snapshot_restores_each_present_absent_combination() {
        let root = std::env::temp_dir().join(format!(
            "catdesk-supervisor-receipt-presence-{}",
            Uuid::new_v4()
        ));
        let writer = SupervisorInstallerWriterV1::test_seam(root.clone());
        let first_bytes = b"presence first";
        let first = writer
            .prepare_exact_reviewed_image(first_bytes, &sha256_hex(first_bytes))
            .expect("prepare first");

        // Absent/Absent: commit then exact-byte removal restores both absent.
        let absent_absent = writer
            .snapshot_exact_install_receipts()
            .expect("AA snapshot");
        writer
            .commit_prepared_exact_reviewed_image(&first)
            .expect("AA transaction commit");
        writer
            .restore_exact_install_receipt_snapshot(&absent_absent, &first)
            .expect("AA exact restore");

        // Present/Absent: a fresh commit produces current while LKG stays
        // absent; a later transaction must remove only its produced LKG.
        writer
            .commit_prepared_exact_reviewed_image(&first)
            .expect("establish PA");
        let present_absent = writer
            .snapshot_exact_install_receipts()
            .expect("PA snapshot");
        let second_bytes = b"presence second";
        let second = writer
            .prepare_exact_reviewed_image(second_bytes, &sha256_hex(second_bytes))
            .expect("prepare second");
        writer
            .commit_prepared_exact_reviewed_image(&second)
            .expect("PA transaction commit");
        writer
            .restore_exact_install_receipt_snapshot(&present_absent, &second)
            .expect("PA exact restore");

        // Present/Present is the normal upgrade shape.
        writer
            .commit_prepared_exact_reviewed_image(&second)
            .expect("establish PP");
        let present_present = writer
            .snapshot_exact_install_receipts()
            .expect("PP snapshot");
        let third_bytes = b"presence third";
        let third = writer
            .prepare_exact_reviewed_image(third_bytes, &sha256_hex(third_bytes))
            .expect("prepare third");
        writer
            .commit_prepared_exact_reviewed_image(&third)
            .expect("PP transaction commit");
        writer
            .restore_exact_install_receipt_snapshot(&present_present, &third)
            .expect("PP exact restore");

        // Absent/Present is an otherwise unusual but representable protected
        // state.  It is snapshotted independently, never inferred from LKG.
        let protected_root = protected_supervisor_root(&root, false).expect("protected root");
        let current_bytes = read_optional_relative_regular(
            &protected_root,
            INSTALL_CURRENT_FILE,
            MAX_STATE_BYTES,
            "test current",
        )
        .expect("current bytes")
        .expect("current present");
        remove_exact_relative_regular_with_bytes(
            &protected_root,
            INSTALL_CURRENT_FILE,
            &current_bytes,
            "test exact current removal",
        )
        .expect("remove only exact test current");
        let absent_present = writer
            .snapshot_exact_install_receipts()
            .expect("AP snapshot");
        writer
            .commit_prepared_exact_reviewed_image(&third)
            .expect("AP transaction commit");
        writer
            .restore_exact_install_receipt_snapshot(&absent_present, &third)
            .expect("AP exact restore");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stale_legacy_temp_and_stage_residue_cannot_wedge_state_or_install_retry() {
        let state_store = store();
        let state_root = state_store.root.clone();
        let stale_state_temp = state_root.join(format!("{STATE_FILE}.new"));
        fs::write(&stale_state_temp, b"stale pre-crash state temporary")
            .expect("stale state fixture");
        state_store
            .register_backend(backend("worker-after-stale-temp"), true)
            .expect("state save must bypass stale legacy temp");
        assert_eq!(
            state_store
                .load()
                .expect("state after retry")
                .active_backend
                .expect("registered backend")
                .backend_id,
            "worker-after-stale-temp"
        );
        assert_eq!(
            fs::read(&stale_state_temp).expect("stale state remains inert"),
            b"stale pre-crash state temporary"
        );

        let install_root = std::env::temp_dir().join(format!(
            "catdesk-supervisor-stale-install-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(install_root.join("versions")).expect("fixture versions root");
        let image = b"reviewed supervisor image after crash residue";
        let digest = sha256_hex(image);
        let version_name = &digest[..16];
        let stale_stage = install_root
            .join("versions")
            .join(format!(".stage-{version_name}"));
        fs::create_dir(&stale_stage).expect("stale stage fixture");
        fs::write(
            stale_stage.join(FIXED_SUPERVISOR_IMAGE_NAME),
            b"partial stale bytes",
        )
        .expect("stale image fixture");
        let stale_current_temp = install_root.join(format!("{INSTALL_CURRENT_FILE}.new"));
        fs::write(&stale_current_temp, b"stale pre-crash receipt temporary")
            .expect("stale receipt fixture");

        let writer = SupervisorInstallerWriterV1::test_seam(install_root.clone());
        let receipt = writer
            .install_exact_reviewed_image(image, &digest)
            .expect("install retry must bypass stale stage and receipt temp");
        assert_eq!(receipt.manifest_sha256, digest);
        assert_eq!(
            fs::read(
                install_root
                    .join(&receipt.version_directory)
                    .join(FIXED_SUPERVISOR_IMAGE_NAME)
            )
            .expect("committed reviewed image"),
            image
        );
        assert_eq!(
            fs::read(&stale_current_temp).expect("stale receipt remains inert"),
            b"stale pre-crash receipt temporary"
        );
        assert!(
            stale_stage.is_dir(),
            "stale stage is inert, not pathname-deleted"
        );

        let _ = fs::remove_dir_all(state_root);
        let _ = fs::remove_dir_all(install_root);
    }

    #[test]
    fn activation_readiness_is_read_only_and_requires_exact_current_receipt_image_binding() {
        let root =
            std::env::temp_dir().join(format!("catdesk-supervisor-readiness-{}", Uuid::new_v4()));
        let writer = SupervisorInstallerWriterV1::test_seam(root.clone());
        let image = b"reviewed supervisor readiness image";
        let digest = sha256_hex(image);
        let receipt = writer
            .install_exact_reviewed_image(image, &digest)
            .expect("fixture install");
        let current_path = root.join(INSTALL_CURRENT_FILE);
        let image_path = root
            .join(&receipt.version_directory)
            .join(FIXED_SUPERVISOR_IMAGE_NAME);
        let before_current = fs::read(&current_path).expect("current bytes");
        let before_image = fs::read(&image_path).expect("image bytes");
        assert_eq!(
            assess_supervisor_activation_readiness_test_seam(&root),
            SupervisorActivationReadinessV1::Ready
        );
        assert_eq!(
            fs::read(&current_path).expect("current after"),
            before_current
        );
        assert_eq!(fs::read(&image_path).expect("image after"), before_image);

        fs::write(&current_path, b"{\"manifestSha256\":\"bad\"}").expect("corrupt fixture");
        assert_eq!(
            assess_supervisor_activation_readiness_test_seam(&root),
            SupervisorActivationReadinessV1::ReceiptOrImageInvalid
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn activation_readiness_refuses_an_absent_root_without_creating_it() {
        let root = std::env::temp_dir().join(format!(
            "catdesk-supervisor-readiness-absent-{}",
            Uuid::new_v4()
        ));
        assert!(!root.exists());
        assert_eq!(
            assess_supervisor_activation_readiness_test_seam(&root),
            SupervisorActivationReadinessV1::RootUnavailable
        );
        assert!(!root.exists());
    }

    #[cfg(windows)]
    #[test]
    fn activation_readiness_policy_gate_fails_closed_on_principal_or_runtime_drift() {
        use crate::windows_supervisor_control_pipe::FixedPipePrincipalPolicyDescriptorV1;

        assert!(
            readiness_policy_gate(
                crate::windows_supervisor_control_pipe::fixed_pipe_principal_policy_descriptor(),
                stable_supervisor_runtime_capability_descriptor(),
            )
            .is_ok()
        );
        assert_eq!(
            readiness_policy_gate(
                FixedPipePrincipalPolicyDescriptorV1::Unproven,
                stable_supervisor_runtime_capability_descriptor(),
            ),
            Err(SupervisorActivationReadinessV1::PrincipalPolicyUnproven)
        );
        assert_eq!(
            readiness_policy_gate(
                crate::windows_supervisor_control_pipe::fixed_pipe_principal_policy_descriptor(),
                StableSupervisorRuntimeCapabilityV1::Unproven,
            ),
            Err(SupervisorActivationReadinessV1::RuntimeOwnershipUnproven)
        );

        let pipe_source = include_str!("windows_supervisor_control_pipe.rs");
        let peer_gate = pipe_source
            .find("peer_from_connected_pipe(&server, &expected_worker, policy)")
            .expect("production peer gate");
        let request_dispatch = pipe_source
            .find("serve_one_fixed_control_request(&store, &peer, &mut server)")
            .expect("production request dispatch");
        assert!(
            peer_gate < request_dispatch,
            "principal admission must precede request decode/dispatch"
        );

        let source = include_str!("control_plane_supervisor.rs");
        let start = source
            .find("pub(crate) async fn run_fixed_stable_supervisor_runtime()")
            .expect("shared runtime start");
        let end = start
            + source[start..]
                .find("#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]")
                .expect("shared runtime end");
        let runtime_source = &source[start..end];
        let ownership_gate = runtime_source
            .find("require_stable_supervisor_runtime_capability")
            .expect("runtime ownership gate");
        let listener_bind = runtime_source
            .find("TcpListener::bind")
            .expect("fixed front-door bind");
        assert!(
            ownership_gate < listener_bind,
            "runtime ownership gate must precede long-lived bind"
        );
        for forbidden_owner in [
            "cloudflared",
            "tunnel_client",
            "official_runtime",
            "SecureMcp",
        ] {
            assert!(
                !runtime_source.contains(forbidden_owner),
                "stable supervisor must not own external Secure MCP lifecycle: {forbidden_owner}"
            );
        }
    }

    #[test]
    fn migrated_supervisor_authority_uses_only_protected_relative_operations() {
        let source = include_str!("control_plane_supervisor.rs");
        for (start, end) in [
            (
                "fn protected_supervisor_root(",
                "impl ControlPlaneSupervisorStoreV1 {",
            ),
            ("    pub fn load(&self)", "    pub fn register_backend("),
            (
                "    pub(crate) fn install_exact_reviewed_image(",
                "fn validated_relative_install_path(",
            ),
        ] {
            let start = source.find(start).expect("authority start");
            let end = source[start..]
                .find(end)
                .map(|offset| start + offset)
                .expect("authority end");
            let body = &source[start..end];
            for forbidden in [
                ".exists()",
                "OpenOptions::",
                "fs::read",
                "fs::write",
                "fs::rename",
                "fs::remove_file",
                "fs::create_dir_all",
            ] {
                assert!(
                    !body.contains(forbidden),
                    "migrated authority reintroduced {forbidden}"
                );
            }
            assert!(body.contains("protected_") || body.contains("relative_"));
        }
    }

    #[test]
    fn closed_control_dispatch_requires_peer_identity_and_generation_cas() {
        let store = store();
        let peer = SupervisorControlPeerV1 {
            authenticated_local_peer: true,
            process_id: 7,
            observed_process_identity_sha256: "b".repeat(64),
        };
        let accepted = store
            .dispatch_control(
                &peer,
                SupervisorControlRequestV1::RegisterBackend {
                    registration: backend("worker-control"),
                    expected_generation: 0,
                    listener_ready: true,
                },
            )
            .expect("registration");
        assert_eq!(accepted.outcome, "REGISTERED");
        assert_eq!(accepted.registration_generation, 1);
        let replay = store
            .dispatch_control(
                &peer,
                SupervisorControlRequestV1::RegisterBackend {
                    registration: backend("worker-control"),
                    expected_generation: 1,
                    listener_ready: true,
                },
            )
            .expect("idempotent replay");
        assert_eq!(replay.outcome, "IDEMPOTENT");
        assert!(
            store
                .dispatch_control(
                    &peer,
                    SupervisorControlRequestV1::RollbackBackend {
                        expected_generation: 0
                    }
                )
                .is_err()
        );
        let wrong_peer = SupervisorControlPeerV1 {
            authenticated_local_peer: false,
            ..peer
        };
        assert!(
            store
                .dispatch_control(&wrong_peer, SupervisorControlRequestV1::Status)
                .is_err()
        );
    }

    #[test]
    fn fixed_pipe_replaces_worker_claimed_observations_with_os_peer_evidence() {
        let peer = SupervisorControlPeerV1 {
            authenticated_local_peer: true,
            process_id: 77,
            observed_process_identity_sha256: "c".repeat(64),
        };
        let mut registration = backend(FIXED_LOCAL_BACKEND_NAME);
        registration.declared_manifest_sha256 = peer.observed_process_identity_sha256.clone();
        registration.expected_process_identity_sha256 =
            peer.observed_process_identity_sha256.clone();
        registration.observed_manifest_sha256 = "a".repeat(64);
        registration.observed_process_identity_sha256 = "a".repeat(64);
        let bound = bind_request_to_os_attested_peer(
            SupervisorControlRequestV1::RegisterBackend {
                registration,
                expected_generation: 0,
                listener_ready: true,
            },
            &peer,
        )
        .expect("supervisor binds the connected process identity");
        let SupervisorControlRequestV1::RegisterBackend { registration, .. } = bound else {
            panic!("only the closed registration request is accepted");
        };
        assert_eq!(
            registration.observed_manifest_sha256,
            peer.observed_process_identity_sha256
        );
        assert_eq!(
            registration.observed_process_identity_sha256,
            peer.observed_process_identity_sha256
        );

        let bad = SupervisorControlRequestV1::RegisterBackend {
            registration: backend(FIXED_LOCAL_BACKEND_NAME),
            expected_generation: 0,
            listener_ready: true,
        };
        assert!(bind_request_to_os_attested_peer(bad, &peer).is_err());
    }

    #[test]
    fn rejected_pipe_evidence_preserves_the_previous_backend() {
        let store = store();
        store
            .register_backend(backend("previous-worker"), true)
            .expect("old backend");
        let peer = SupervisorControlPeerV1 {
            authenticated_local_peer: true,
            process_id: 88,
            observed_process_identity_sha256: "d".repeat(64),
        };
        // This request contains matching worker-supplied observations, but it
        // cannot masquerade as the independently observed pipe peer.
        assert!(
            bind_request_to_os_attested_peer(
                SupervisorControlRequestV1::RegisterBackend {
                    registration: backend("replacement-worker"),
                    expected_generation: 1,
                    listener_ready: true,
                },
                &peer,
            )
            .is_err()
        );
        assert_eq!(
            store
                .load()
                .expect("old supervisor state remains readable")
                .active_backend
                .expect("old backend retained")
                .backend_id,
            "previous-worker"
        );
    }

    #[test]
    fn worker_registration_reuses_signed_reviewed_image_evidence_before_pipe_connect() {
        let worker_source = include_str!("main.rs");
        let reviewed_source = include_str!("reviewed_build.rs");
        assert!(worker_source.contains("verified_current_reviewed_main_image_digest"));
        assert!(reviewed_source.contains("read_accepted_reviewed_main_image_envelope"));
        assert!(reviewed_source.contains("verify_reviewed_main_image_payload_binding"));
        assert!(
            reviewed_source.contains("pub(crate) fn verified_current_reviewed_main_image_digest")
        );
    }

    #[test]
    fn supervisor_binary_requires_front_door_and_fixed_pipe_together() {
        let source = include_str!("bin/catdesk-control-plane-supervisor.rs");
        let runtime_source = include_str!("control_plane_supervisor.rs");
        assert!(source.contains("run_fixed_stable_supervisor_runtime"));
        assert!(runtime_source.contains("tokio::try_join!(front_door, control_pipe)"));
        assert!(runtime_source.contains("serve_fixed_control_pipe(store)"));
        assert!(runtime_source.contains("CONTROL_PLANE_SUPERVISOR_REQUIRED_SURFACE_FAILED"));
        assert!(!source.contains("axum::serve(listener, fixed_front_door_router(store))"));
    }

    #[test]
    fn fixed_catdesk_supervisor_runtime_entry_is_singleton_and_has_no_mode_authority() {
        let flag = FIXED_STABLE_SUPERVISOR_RUNTIME_FLAG.to_string();
        assert_eq!(
            parse_fixed_stable_supervisor_runtime_args(std::slice::from_ref(&flag)),
            Ok(true)
        );
        for hostile in [
            vec![flag.clone(), "--port".into(), "3201".into()],
            vec![flag.clone(), "--pipe".into(), "x".into()],
            vec![flag.clone(), "--image".into(), "x".into()],
            vec![flag.clone(), "--sid".into(), "x".into()],
            vec![flag.clone(), "--tunnel".into(), "x".into()],
            vec![flag.clone(), flag.clone()],
        ] {
            assert!(parse_fixed_stable_supervisor_runtime_args(&hostile).is_err());
        }
        assert_eq!(
            parse_fixed_stable_supervisor_runtime_args(&["--catdesk-daemon".to_string()]),
            Ok(false)
        );
        assert_eq!(
            parse_fixed_stable_supervisor_runtime_args(&[
                "operator".into(),
                "supervisor".into(),
                "status".into(),
            ]),
            Ok(false)
        );
        let main_source = include_str!("main.rs");
        assert!(main_source.contains("parse_fixed_stable_supervisor_runtime_args(&args)"));
        assert!(main_source.contains("run_fixed_stable_supervisor_runtime().await"));
        let source = include_str!("control_plane_supervisor.rs");
        let start = source
            .find("pub(crate) async fn run_fixed_stable_supervisor_runtime()")
            .expect("shared runtime start");
        let end = start
            + source[start..]
                .find("#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]")
                .expect("shared runtime end");
        let runtime_source = &source[start..end];
        for forbidden in [
            "target/release",
            "current_exe",
            "current_dir",
            "PATH",
            "Command::new",
        ] {
            assert!(
                !runtime_source.contains(forbidden),
                "fixed supervisor runtime must not derive trust from {forbidden}"
            );
        }
    }

    #[test]
    fn startup_plan_is_fixed_dry_run_and_data_plane_has_no_control_route() {
        let store = store();
        assert_eq!(
            store.fixed_startup_registration_plan(),
            SupervisorStartupRegistrationPlanV1::Create
        );
        let source = include_str!("control_plane_supervisor.rs");
        let proxy_start = source.find("async fn proxy_mcp_request").expect("proxy");
        let proxy_end = source[proxy_start..]
            .find("fn is_hop_by_hop_header")
            .expect("proxy boundary");
        let proxy = &source[proxy_start..proxy_start + proxy_end];
        assert!(!proxy.contains("dispatch_control("));
        assert!(!FIXED_FRONT_DOOR_PATH.contains("control"));
    }

    #[test]
    fn ordinary_worker_reload_source_has_no_stable_supervisor_root_authority() {
        let reload_source = include_str!("daemon_reload.rs");
        assert!(!reload_source.contains("ControlPlaneSupervisor"));
        assert!(!reload_source.contains("control-plane-supervisor"));
    }

    #[test]
    fn state_root_rejects_file_replacement_and_oversized_state() {
        let root =
            std::env::temp_dir().join(format!("catdesk-control-plane-file-{}", Uuid::new_v4()));
        fs::write(&root, b"not a directory").expect("file root");
        assert!(matches!(
            ControlPlaneSupervisorStoreV1::open_test_seam(root.clone()),
            Err(ControlPlaneSupervisorError::InvalidState)
        ));
        let _ = fs::remove_file(root);

        let store = store();
        fs::write(
            store.root.join(STATE_FILE),
            vec![b'x'; (MAX_STATE_BYTES + 1) as usize],
        )
        .expect("oversized state");
        assert_eq!(store.load(), Err(ControlPlaneSupervisorError::InvalidState));
    }

    #[test]
    fn fixed_boundary_rejects_paths_and_keeps_worker_artifacts_out_of_authority() {
        assert_eq!(fixed_supervisor_front_door(), "/mcp");
        assert_eq!(FIXED_LOCAL_BACKEND_NAME, "catdesk-local-backend");
        assert!(
            !fixed_supervisor_install_root()
                .to_string_lossy()
                .contains("target")
        );
        let store = store();
        let mut unsafe_registration = backend("worker-a");
        unsafe_registration.backend_id = "C:/target/release/catdesk.exe".into();
        assert_eq!(
            store.register_backend(unsafe_registration, true),
            Err(ControlPlaneSupervisorError::InvalidRegistration)
        );
    }

    async fn spawn_loopback_router(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral listener");
        let address = listener.local_addr().expect("address");
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        (format!("http://{address}"), task)
    }

    #[tokio::test]
    async fn fixed_front_door_proxies_post_get_and_delete_to_one_ephemeral_snapshot() {
        let worker = Router::new().route(
            "/mcp",
            any(
                |method: Method, headers: HeaderMap, body: Bytes| async move {
                    let header = headers
                        .get("x-end-to-end")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("missing");
                    Response::builder()
                        .status(StatusCode::ACCEPTED)
                        .header("x-worker-method", method.as_str())
                        .header("x-worker-header", header)
                        .body(Body::from(body))
                        .expect("response")
                },
            ),
        );
        let (worker_base, worker_task) = spawn_loopback_router(worker).await;
        let store = store();
        store
            .register_backend(backend("worker-a"), true)
            .expect("register");
        let supervisor = fixed_front_door_router_test_seam(store, format!("{worker_base}/mcp"));
        let (supervisor_base, supervisor_task) = spawn_loopback_router(supervisor).await;
        let client = reqwest::Client::new();
        for method in [
            reqwest::Method::POST,
            reqwest::Method::GET,
            reqwest::Method::DELETE,
        ] {
            let response = client
                .request(method.clone(), format!("{supervisor_base}/mcp"))
                .header("x-end-to-end", "kept")
                .header("connection", "x-removed")
                .header("x-removed", "not-forwarded")
                .body("mcp-body")
                .send()
                .await
                .expect("proxy response");
            assert_eq!(response.status(), StatusCode::ACCEPTED);
            assert_eq!(response.headers()["x-worker-method"], method.as_str());
            assert_eq!(response.headers()["x-worker-header"], "kept");
            assert_eq!(
                response.bytes().await.expect("body"),
                Bytes::from_static(b"mcp-body")
            );
        }
        supervisor_task.abort();
        worker_task.abort();
    }

    #[tokio::test]
    async fn proxy_refuses_missing_backend_and_test_seam_cannot_be_arbitrary() {
        let store = store();
        assert!(!valid_test_loopback_endpoint("https://example.invalid/mcp"));
        assert!(!valid_test_loopback_endpoint("http://127.0.0.1:3201/mcp"));
        assert!(!valid_test_loopback_endpoint(
            "http://127.0.0.1:3333/not-mcp"
        ));
        let supervisor = fixed_front_door_router(store);
        let (base, task) = spawn_loopback_router(supervisor).await;
        let response = reqwest::Client::new()
            .post(format!("{base}/mcp"))
            .body("body")
            .send()
            .await
            .expect("bounded response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.json::<Value>().await.expect("json")["error"],
            "CONTROL_PLANE_BACKEND_UNAVAILABLE"
        );
        task.abort();
    }
}
