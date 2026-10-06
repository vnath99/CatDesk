//! Fixed, local-only supervisor control transport.
//!
//! This module is deliberately separate from MCP.  It is compiled only on
//! Windows and is the sole place where a connected named-pipe handle may be
//! translated into supervisor peer evidence.  JSON never supplies a PID,
//! token, image path, or image digest as authentication.

#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

#[cfg(windows)]
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
#[cfg(windows)]
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};

#[cfg(windows)]
use crate::control_plane_supervisor::{
    ControlPlaneSupervisorError, ControlPlaneSupervisorStoreV1, FIXED_LOCAL_BACKEND_PORT,
    FIXED_SUPERVISOR_CONTROL_PIPE, SupervisorControlPeerV1, SupervisorControlRequestV1,
    SupervisorControlResponseV1, bind_request_to_os_attested_peer,
    fixed_current_worker_registration,
};

/// Bound framing prevents a pipe client from consuming unbounded supervisor
/// memory.
#[cfg(windows)]
const MAX_CONTROL_RECORD_BYTES: usize = 64 * 1024;
#[cfg(windows)]
const MAX_PRINCIPAL_SID_BYTES: usize = 1024;

/// Source-linked policy token consumed by the production pipe server before it
/// obtains the supervisor token or accepts a connection.  It is deliberately
/// an enum rather than caller-set booleans: the sole `Enforced` variant names
/// the exact admission/attestation chain implemented below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FixedPipePrincipalPolicyDescriptorV1 {
    SupervisorTokenUserAndSessionBeforeDecodeWithImageBinding,
    Unproven,
}

pub(crate) fn fixed_pipe_principal_policy_descriptor() -> FixedPipePrincipalPolicyDescriptorV1 {
    FixedPipePrincipalPolicyDescriptorV1::SupervisorTokenUserAndSessionBeforeDecodeWithImageBinding
}

pub(crate) fn require_fixed_pipe_principal_policy(
    descriptor: FixedPipePrincipalPolicyDescriptorV1,
) -> Result<(), ControlPlaneSupervisorError> {
    match descriptor {
        FixedPipePrincipalPolicyDescriptorV1::SupervisorTokenUserAndSessionBeforeDecodeWithImageBinding => Ok(()),
        FixedPipePrincipalPolicyDescriptorV1::Unproven => Err(ControlPlaneSupervisorError::InvalidState),
    }
}

/// The product principal is not configuration. The stable supervisor obtains
/// it from its own OS token at startup and requires the canonical ordinary
/// daemon to present that exact TokenUser plus TokenSessionId before any
/// request bytes are decoded. System and Administrators remain only in the
/// DACL as operating-system recovery identities; this control protocol grants
/// them no request role at all.
#[cfg(windows)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct ExpectedWorkerPrincipalV1 {
    sid: Vec<u8>,
    sid_sddl: String,
    session_id: u32,
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PipeClientRoleV1 {
    Worker,
    Recovery,
}

#[cfg(windows)]
#[repr(C)]
struct SecurityAttributes {
    n_length: u32,
    security_descriptor: *mut c_void,
    inherit_handle: i32,
}

/// Owns the descriptor while `CreateNamedPipeW` consumes the attributes.  The
/// ACL is installed on every pipe instance, not merely documented for an
/// external installer, so a client must first pass the fixed local policy.
#[cfg(windows)]
struct PipeSecurityDescriptor(*mut c_void);

#[cfg(windows)]
impl Drop for PipeSecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0) };
        }
    }
}

#[cfg(windows)]
unsafe extern "system" {
    fn GetNamedPipeClientProcessId(pipe: *mut c_void, client_process_id: *mut u32) -> i32;
    fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> *mut c_void;
    fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn GetTokenInformation(
        token: *mut c_void,
        token_information_class: u32,
        token_information: *mut c_void,
        token_information_length: u32,
        return_length: *mut u32,
    ) -> i32;
    fn QueryFullProcessImageNameW(
        process: *mut c_void,
        flags: u32,
        buffer: *mut u16,
        size: *mut u32,
    ) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn GetLengthSid(sid: *mut c_void) -> u32;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

#[cfg(windows)]
#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        string_security_descriptor: *const u16,
        string_sd_revision: u32,
        security_descriptor: *mut *mut c_void,
        security_descriptor_size: *mut u32,
    ) -> i32;
}

#[cfg(windows)]
#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertSidToStringSidW(sid: *mut c_void, string_sid: *mut *mut u16) -> i32;
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct SidAndAttributes {
    sid: *mut c_void,
    attributes: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct TokenUser {
    user: SidAndAttributes,
}

/// Starts one pipe instance at a time. The caller supplies no endpoint name;
/// `FIXED_SUPERVISOR_CONTROL_PIPE` is compiled policy.  The server derives the
/// client PID from the connected instance before parsing a record and rejects
/// token evidence it cannot query. Image/listener attestation remains bound by
/// the closed register record and supervisor's fixed worker policy.
#[cfg(windows)]
pub async fn serve_fixed_control_pipe(
    store: ControlPlaneSupervisorStoreV1,
) -> Result<(), ControlPlaneSupervisorError> {
    let policy = fixed_pipe_principal_policy_descriptor();
    require_fixed_pipe_principal_policy(policy)?;
    // The supported production lifecycle starts both the stable supervisor and
    // the ordinary `catdesk.exe --catdesk-daemon` worker as the interactive
    // product user in one session. Session zero / LocalSystem therefore fails
    // closed rather than turning this pipe into a service-identity boundary.
    let expected_worker = expected_worker_principal_from_supervisor_token(policy)?;
    let mut first_instance = true;
    loop {
        let server = create_fixed_pipe_instance(first_instance, &expected_worker)?;
        first_instance = false;
        if server.connect().await.is_err() {
            // A failed individual client connection is not a loss of the
            // long-lived pipe surface. The next instance still receives the
            // fixed DACL; instance creation failure remains fatal above.
            continue;
        }
        let mut server = server;
        // Connected PID, liveness, token user/session and image hash are
        // derived from the accepted pipe instance *before* any request bytes
        // are parsed. Non-worker principals, malformed tokens, and malformed
        // records are rejected per connection without killing the pipe server.
        let Ok(peer) = peer_from_connected_pipe(&server, &expected_worker, policy) else {
            continue;
        };
        let _ = serve_one_fixed_control_request(&store, &peer, &mut server).await;
    }
}

#[cfg(windows)]
fn create_fixed_pipe_instance(
    first: bool,
    expected_worker: &ExpectedWorkerPrincipalV1,
) -> Result<NamedPipeServer, ControlPlaneSupervisorError> {
    let descriptor = fixed_pipe_security_descriptor(expected_worker)?;
    let mut attributes = SecurityAttributes {
        n_length: std::mem::size_of::<SecurityAttributes>() as u32,
        security_descriptor: descriptor.0,
        inherit_handle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .in_buffer_size(MAX_CONTROL_RECORD_BYTES as u32)
        .out_buffer_size(MAX_CONTROL_RECORD_BYTES as u32);
    // SAFETY: attributes and its SDDL-derived descriptor outlive the Win32
    // creation call. Tokio passes the pointer directly to CreateNamedPipeW.
    unsafe {
        options
            .create_with_security_attributes_raw(
                FIXED_SUPERVISOR_CONTROL_PIPE,
                (&mut attributes as *mut SecurityAttributes).cast(),
            )
            .map_err(|_| ControlPlaneSupervisorError::Io)
    }
}

#[cfg(windows)]
fn fixed_pipe_security_descriptor(
    expected_worker: &ExpectedWorkerPrincipalV1,
) -> Result<PipeSecurityDescriptor, ControlPlaneSupervisorError> {
    let sddl_text = fixed_pipe_sddl_for_expected_worker(expected_worker)?;
    let mut sddl: Vec<u16> = std::ffi::OsStr::new(&sddl_text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut descriptor = std::ptr::null_mut();
    let mut descriptor_size = 0_u32;
    // SECURITY_DESCRIPTOR_REVISION is 1. A missing or malformed descriptor
    // fails closed; there is intentionally no default-ACL fallback.
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_mut_ptr(),
            1,
            &mut descriptor,
            &mut descriptor_size,
        )
    };
    if converted == 0 || descriptor.is_null() || descriptor_size == 0 {
        return Err(ControlPlaneSupervisorError::InvalidState);
    }
    Ok(PipeSecurityDescriptor(descriptor))
}

#[cfg(windows)]
fn fixed_pipe_sddl_for_expected_worker(
    expected_worker: &ExpectedWorkerPrincipalV1,
) -> Result<String, ControlPlaneSupervisorError> {
    if expected_worker.sid.is_empty()
        || expected_worker.sid.len() > MAX_PRINCIPAL_SID_BYTES
        || expected_worker.sid_sddl.is_empty()
        || expected_worker.sid_sddl.len() > 256
        || expected_worker.session_id == 0
        || expected_worker.sid_sddl == "S-1-5-18"
        || !expected_worker
            .sid_sddl
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(ControlPlaneSupervisorError::InvalidState);
    }
    // No ALL SERVICES, Everyone, Users, or Authenticated Users ACE exists.
    // The only variable fragment is an OS-derived TokenUser SID from this
    // supervisor's startup token, never CLI/config/environment/JSON input.
    Ok(format!(
        "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{})",
        expected_worker.sid_sddl
    ))
}

#[cfg(windows)]
async fn serve_one_fixed_control_request(
    store: &ControlPlaneSupervisorStoreV1,
    peer: &SupervisorControlPeerV1,
    server: &mut NamedPipeServer,
) -> Result<(), ControlPlaneSupervisorError> {
    let request = read_control_request(server).await?;
    let request = match request {
        SupervisorControlRequestV1::Status => SupervisorControlRequestV1::Status,
        registration @ SupervisorControlRequestV1::RegisterBackend { .. } => {
            bind_request_to_os_attested_peer(registration, peer)?
        }
        // The worker has no generic state-control client. Health, rollback,
        // and remote-route records remain unavailable through this startup
        // registration seam rather than becoming accidental worker authority.
        _ => return Err(ControlPlaneSupervisorError::InvalidRegistration),
    };
    let response = store.dispatch_control(peer, request)?;
    write_control_response(server, &response).await
}

/// Closed worker-side registration. Its only input is the already-bound
/// production listener; no caller selects a pipe, endpoint, request field, or
/// executable. A missing/refusing supervisor only returns an error to the
/// worker and cannot modify the worker's own state or listener.
#[cfg(windows)]
pub(crate) async fn register_fixed_ready_worker_listener(
    listener: &tokio::net::TcpListener,
    reviewed_manifest_sha256: &str,
) -> Result<SupervisorControlResponseV1, ControlPlaneSupervisorError> {
    let local = listener
        .local_addr()
        .map_err(|_| ControlPlaneSupervisorError::Io)?;
    if local.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
        || local.port() != FIXED_LOCAL_BACKEND_PORT
    {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    let mut status_client = open_fixed_pipe_client().await?;
    let status = request_response(&mut status_client, SupervisorControlRequestV1::Status).await?;
    drop(status_client);
    let mut registration_client = open_fixed_pipe_client().await?;
    let request = SupervisorControlRequestV1::RegisterBackend {
        registration: fixed_current_worker_registration(reviewed_manifest_sha256)?,
        expected_generation: status.registration_generation,
        listener_ready: true,
    };
    let response = request_response(&mut registration_client, request).await?;
    if !matches!(response.outcome.as_str(), "REGISTERED" | "IDEMPOTENT") {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    Ok(response)
}

#[cfg(windows)]
async fn open_fixed_pipe_client()
-> Result<tokio::net::windows::named_pipe::NamedPipeClient, ControlPlaneSupervisorError> {
    // A one-request server instance is recreated immediately after each
    // response. Bound retries bridge that small creation gap and fail closed
    // for an absent/busy supervisor; this is never a fallback endpoint.
    const OPEN_ATTEMPTS: usize = 8;
    const PIPE_BUSY: i32 = 231;
    const FILE_NOT_FOUND: i32 = 2;
    for attempt in 0..OPEN_ATTEMPTS {
        match ClientOptions::new().open(FIXED_SUPERVISOR_CONTROL_PIPE) {
            Ok(client) => return Ok(client),
            Err(error)
                if attempt + 1 < OPEN_ATTEMPTS
                    && matches!(error.raw_os_error(), Some(PIPE_BUSY | FILE_NOT_FOUND)) =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            Err(_) => return Err(ControlPlaneSupervisorError::Io),
        }
    }
    Err(ControlPlaneSupervisorError::Io)
}

#[cfg(windows)]
async fn request_response(
    client: &mut tokio::net::windows::named_pipe::NamedPipeClient,
    request: SupervisorControlRequestV1,
) -> Result<SupervisorControlResponseV1, ControlPlaneSupervisorError> {
    let bytes = encode_control_request(&request)?;
    client
        .write_all(&bytes)
        .await
        .map_err(|_| ControlPlaneSupervisorError::Io)?;
    client
        .flush()
        .await
        .map_err(|_| ControlPlaneSupervisorError::Io)?;
    read_control_response(client).await
}

#[cfg(windows)]
fn encode_control_request(
    request: &SupervisorControlRequestV1,
) -> Result<Vec<u8>, ControlPlaneSupervisorError> {
    let record = serde_json::to_vec(request).map_err(|_| ControlPlaneSupervisorError::Io)?;
    if record.is_empty() || record.len() > MAX_CONTROL_RECORD_BYTES {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    let mut framed = Vec::with_capacity(4 + record.len());
    framed.extend_from_slice(&(record.len() as u32).to_le_bytes());
    framed.extend_from_slice(&record);
    Ok(framed)
}

#[cfg(windows)]
async fn read_control_request(
    stream: &mut NamedPipeServer,
) -> Result<SupervisorControlRequestV1, ControlPlaneSupervisorError> {
    let record = read_framed_record(stream).await?;
    serde_json::from_slice(&record).map_err(|_| ControlPlaneSupervisorError::InvalidRegistration)
}

#[cfg(windows)]
async fn read_control_response(
    stream: &mut tokio::net::windows::named_pipe::NamedPipeClient,
) -> Result<SupervisorControlResponseV1, ControlPlaneSupervisorError> {
    let record = read_framed_record(stream).await?;
    let response: SupervisorControlResponseV1 = serde_json::from_slice(&record)
        .map_err(|_| ControlPlaneSupervisorError::InvalidRegistration)?;
    if !matches!(
        response.outcome.as_str(),
        "REGISTERED"
            | "IDEMPOTENT"
            | "REFUSED"
            | "STATUS"
            | "HEALTH_RECORDED"
            | "ROLLED_BACK"
            | "REMOTE_ROUTE_RECORDED"
    ) {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    Ok(response)
}

#[cfg(windows)]
async fn read_framed_record<S: AsyncRead + Unpin>(
    stream: &mut S,
) -> Result<Vec<u8>, ControlPlaneSupervisorError> {
    let mut length = [0_u8; 4];
    stream
        .read_exact(&mut length)
        .await
        .map_err(|_| ControlPlaneSupervisorError::Io)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_CONTROL_RECORD_BYTES {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    let mut record = vec![0_u8; length];
    stream
        .read_exact(&mut record)
        .await
        .map_err(|_| ControlPlaneSupervisorError::Io)?;
    Ok(record)
}

#[cfg(windows)]
async fn write_control_response(
    server: &mut NamedPipeServer,
    response: &SupervisorControlResponseV1,
) -> Result<(), ControlPlaneSupervisorError> {
    let bytes = serde_json::to_vec(response).map_err(|_| ControlPlaneSupervisorError::Io)?;
    if bytes.is_empty() || bytes.len() > MAX_CONTROL_RECORD_BYTES {
        return Err(ControlPlaneSupervisorError::Io);
    }
    server
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .await
        .map_err(|_| ControlPlaneSupervisorError::Io)?;
    server
        .write_all(&bytes)
        .await
        .map_err(|_| ControlPlaneSupervisorError::Io)?;
    server
        .flush()
        .await
        .map_err(|_| ControlPlaneSupervisorError::Io)
}

#[cfg(windows)]
fn peer_from_connected_pipe(
    pipe: &tokio::net::windows::named_pipe::NamedPipeServer,
    expected_worker: &ExpectedWorkerPrincipalV1,
    policy: FixedPipePrincipalPolicyDescriptorV1,
) -> Result<SupervisorControlPeerV1, ControlPlaneSupervisorError> {
    require_fixed_pipe_principal_policy(policy)?;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const TOKEN_QUERY: u32 = 0x0008;
    let mut pid = 0_u32;
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle().cast(), &mut pid) } == 0
        || pid == 0
    {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    let mut token = std::ptr::null_mut();
    let token_ok =
        unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } != 0 && !token.is_null();
    if !token_ok {
        unsafe { CloseHandle(process) };
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    let principal = match principal_from_token(token) {
        Ok(principal) => principal,
        Err(_) => {
            unsafe {
                CloseHandle(token);
                CloseHandle(process);
            }
            return Err(ControlPlaneSupervisorError::InvalidRegistration);
        }
    };
    let role = role_for_connected_principal(expected_worker, &principal);
    // This check is deliberately before framed-record read/decode. Recovery
    // DACL entries are OS-only break-glass identities and have no worker
    // protocol operation, so they cannot turn Status into a registration
    // probing or mutation channel.
    if !role_may_decode_worker_control(role) {
        unsafe {
            CloseHandle(token);
            CloseHandle(process);
        }
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    let mut image = vec![0_u16; 32_768];
    let mut image_length = image.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process, 0, image.as_mut_ptr(), &mut image_length) } == 0
        || image_length == 0
        || image_length as usize >= image.len()
        || unsafe { WaitForSingleObject(process, 0) } != 0x0000_0102
    {
        unsafe {
            CloseHandle(token);
            CloseHandle(process);
        }
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    let image_hash = (|| {
        let image_path = std::path::PathBuf::from(std::ffi::OsString::from_wide(
            &image[..image_length as usize],
        ));
        let image_bytes = std::fs::read(&image_path)
            .map_err(|_| ControlPlaneSupervisorError::InvalidRegistration)?;
        // The process handle refers to this exact PID object, not a reused PID.
        // Check it again after pathname read so an exited worker cannot
        // contribute image evidence to an accepted registration.
        if unsafe { WaitForSingleObject(process, 0) } != 0x0000_0102 {
            return Err(ControlPlaneSupervisorError::InvalidRegistration);
        }
        use sha2::Digest;
        Ok::<String, ControlPlaneSupervisorError>(
            sha2::Sha256::digest(&image_bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        )
    })();
    unsafe {
        CloseHandle(token);
        CloseHandle(process);
    }
    let image_hash = image_hash?;
    Ok(SupervisorControlPeerV1 {
        authenticated_local_peer: true,
        process_id: pid,
        observed_process_identity_sha256: image_hash,
    })
}

#[cfg(windows)]
fn expected_worker_principal_from_supervisor_token(
    policy: FixedPipePrincipalPolicyDescriptorV1,
) -> Result<ExpectedWorkerPrincipalV1, ControlPlaneSupervisorError> {
    require_fixed_pipe_principal_policy(policy)?;
    const TOKEN_QUERY: u32 = 0x0008;
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0
        || token.is_null()
    {
        return Err(ControlPlaneSupervisorError::InvalidState);
    }
    let principal =
        principal_from_token(token).map_err(|_| ControlPlaneSupervisorError::InvalidState);
    unsafe { CloseHandle(token) };
    let principal = principal?;
    // The supported lifecycle is an interactive current-user task plus an
    // ordinary same-session daemon. Refusing LocalSystem/session zero prevents
    // a silently broadened service identity model.
    if principal.session_id == 0 || principal.sid_sddl == "S-1-5-18" {
        return Err(ControlPlaneSupervisorError::InvalidState);
    }
    Ok(principal)
}

/// Returns only the authenticated interactive user SID for the fixed
/// supervisor-startup authority.  This deliberately reuses the production
/// pipe admission derivation above: it opens the current process token,
/// requires a non-service, nonzero-session principal, then discards the
/// numeric session id.  The session is evidence that the present caller is
/// interactive; it is not persisted into a future-logon task definition.
#[cfg(windows)]
pub(crate) fn fixed_interactive_supervisor_startup_user_sid()
-> Result<String, ControlPlaneSupervisorError> {
    let principal =
        expected_worker_principal_from_supervisor_token(fixed_pipe_principal_policy_descriptor())?;
    Ok(principal.sid_sddl)
}

#[cfg(not(windows))]
pub(crate) fn fixed_interactive_supervisor_startup_user_sid()
-> Result<String, ControlPlaneSupervisorError> {
    Err(ControlPlaneSupervisorError::InvalidState)
}

#[cfg(windows)]
fn principal_from_token(
    token: *mut c_void,
) -> Result<ExpectedWorkerPrincipalV1, ControlPlaneSupervisorError> {
    const TOKEN_USER: u32 = 1;
    const TOKEN_SESSION_ID: u32 = 12;
    let mut required = 0_u32;
    unsafe { GetTokenInformation(token, TOKEN_USER, std::ptr::null_mut(), 0, &mut required) };
    if required < std::mem::size_of::<TokenUser>() as u32
        || required as usize > MAX_CONTROL_RECORD_BYTES
    {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    let mut user = vec![0_u8; required as usize];
    let mut returned = 0_u32;
    if unsafe {
        GetTokenInformation(
            token,
            TOKEN_USER,
            user.as_mut_ptr().cast(),
            required,
            &mut returned,
        )
    } == 0
        || returned != required
    {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    // `GetTokenInformation` fills a byte buffer whose allocation alignment is
    // not a Rust guarantee. Read the C header unaligned before using the SID
    // pointer it contains.
    let token_user = unsafe { std::ptr::read_unaligned(user.as_ptr().cast::<TokenUser>()) };
    let sid = token_user.user.sid;
    let sid_length = unsafe { GetLengthSid(sid) } as usize;
    if sid.is_null() || sid_length == 0 || sid_length > MAX_PRINCIPAL_SID_BYTES {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    let sid = unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), sid_length).to_vec() };
    let sid_sddl = sid_to_sddl(token_user.user.sid)?;
    let mut session_id = 0_u32;
    returned = 0;
    if unsafe {
        GetTokenInformation(
            token,
            TOKEN_SESSION_ID,
            (&mut session_id as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
            &mut returned,
        )
    } == 0
        || returned != std::mem::size_of::<u32>() as u32
    {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    Ok(ExpectedWorkerPrincipalV1 {
        sid,
        sid_sddl,
        session_id,
    })
}

#[cfg(windows)]
fn sid_to_sddl(sid: *mut c_void) -> Result<String, ControlPlaneSupervisorError> {
    let mut raw = std::ptr::null_mut();
    if sid.is_null() || unsafe { ConvertSidToStringSidW(sid, &mut raw) } == 0 || raw.is_null() {
        return Err(ControlPlaneSupervisorError::InvalidRegistration);
    }
    let result = unsafe {
        let mut length = 0_usize;
        while length < 256 && *raw.add(length) != 0 {
            length += 1;
        }
        if length == 0 || length == 256 {
            Err(ControlPlaneSupervisorError::InvalidRegistration)
        } else {
            String::from_utf16(std::slice::from_raw_parts(raw, length))
                .map_err(|_| ControlPlaneSupervisorError::InvalidRegistration)
        }
    };
    unsafe { LocalFree(raw.cast()) };
    result
}

#[cfg(windows)]
fn role_for_connected_principal(
    expected: &ExpectedWorkerPrincipalV1,
    connected: &ExpectedWorkerPrincipalV1,
) -> PipeClientRoleV1 {
    if expected.sid == connected.sid && expected.session_id == connected.session_id {
        PipeClientRoleV1::Worker
    } else {
        PipeClientRoleV1::Recovery
    }
}

#[cfg(windows)]
fn role_may_decode_worker_control(role: PipeClientRoleV1) -> bool {
    // SYSTEM/Administrators are deliberately present only in the DACL for
    // operating-system recovery. This fixed registration pipe grants them no
    // control operation, including Status, because there is no independently
    // reviewed recovery command in this protocol.
    role == PipeClientRoleV1::Worker
}

#[cfg(not(windows))]
pub fn fixed_control_pipe_is_unavailable() -> bool {
    true
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::control_plane_supervisor::{FIXED_LOCAL_BACKEND_NAME, WorkerBackendRegistrationV1};

    fn registration() -> WorkerBackendRegistrationV1 {
        WorkerBackendRegistrationV1 {
            backend_id: FIXED_LOCAL_BACKEND_NAME.into(),
            declared_manifest_sha256: "a".repeat(64),
            observed_manifest_sha256: "a".repeat(64),
            endpoint: crate::control_plane_supervisor::FIXED_LOCAL_BACKEND_ENDPOINT.into(),
            expected_process_identity_sha256: "a".repeat(64),
            observed_process_identity_sha256: "a".repeat(64),
        }
    }

    #[test]
    fn fixed_pipe_registration_framing_is_bounded_and_closed() {
        let framed = encode_control_request(&SupervisorControlRequestV1::RegisterBackend {
            registration: registration(),
            expected_generation: 9,
            listener_ready: true,
        })
        .expect("closed registration frame");
        assert_eq!(
            u32::from_le_bytes(framed[..4].try_into().unwrap()) as usize,
            framed.len() - 4
        );
        assert!(framed.len() <= MAX_CONTROL_RECORD_BYTES + 4);
        let text = std::str::from_utf8(&framed[4..]).unwrap();
        assert!(text.contains("REGISTER_BACKEND"));
        assert!(!text.contains("\\\\.\\pipe\\arbitrary"));
    }

    #[test]
    fn fixed_pipe_source_has_os_derived_principal_acl_and_no_generic_client() {
        let source = include_str!("windows_supervisor_control_pipe.rs");
        assert!(source.contains("expected_worker_principal_from_supervisor_token"));
        assert!(source.contains("GetNamedPipeClientProcessId"));
        assert!(source.contains("TOKEN_SESSION_ID"));
        assert!(source.contains("role_for_connected_principal"));
        assert!(source.contains("before framed-record read/decode"));
        assert!(source.contains("create_with_security_attributes_raw"));
        assert!(source.contains("reject_remote_clients(true)"));
        assert!(source.contains("register_fixed_ready_worker_listener"));
        let all_services = ["S-1-5-80", "-0"].concat();
        let everyone = [";;;W", "D"].concat();
        let users = [";;;B", "U"].concat();
        let authenticated_users = [";;;A", "U"].concat();
        for forbidden_ace in [
            all_services.as_str(),
            everyone.as_str(),
            users.as_str(),
            authenticated_users.as_str(),
        ] {
            assert!(
                !source.contains(forbidden_ace),
                "pipe source must not grant broad principal {forbidden_ace}"
            );
        }
        assert!(!source.contains(&["pub async fn ", "send_control_request"].concat()));
        assert!(!source.contains(&["pub async fn ", "connect_pipe"].concat()));
        assert!(source.contains("fixed_pipe_principal_policy_descriptor"));
        assert!(source.contains("require_fixed_pipe_principal_policy(policy)?"));
    }

    #[test]
    fn principal_policy_descriptor_is_consumed_by_actual_pipe_authority() {
        assert_eq!(
            fixed_pipe_principal_policy_descriptor(),
            FixedPipePrincipalPolicyDescriptorV1::SupervisorTokenUserAndSessionBeforeDecodeWithImageBinding
        );
        assert!(
            require_fixed_pipe_principal_policy(FixedPipePrincipalPolicyDescriptorV1::Unproven)
                .is_err()
        );
    }

    fn principal(sid: &[u8], session_id: u32) -> ExpectedWorkerPrincipalV1 {
        ExpectedWorkerPrincipalV1 {
            sid: sid.to_vec(),
            sid_sddl: "S-1-5-21-100-200-300-400".into(),
            session_id,
        }
    }

    #[test]
    fn only_same_os_user_and_session_receives_worker_role_before_decode() {
        let expected = principal(b"expected-worker", 7);
        assert_eq!(
            role_for_connected_principal(&expected, &principal(b"expected-worker", 7)),
            PipeClientRoleV1::Worker
        );
        assert_eq!(
            role_for_connected_principal(&expected, &principal(b"other-worker", 7)),
            PipeClientRoleV1::Recovery
        );
        assert_eq!(
            role_for_connected_principal(&expected, &principal(b"expected-worker", 8)),
            PipeClientRoleV1::Recovery
        );
        assert!(!role_may_decode_worker_control(PipeClientRoleV1::Recovery));
        assert!(role_may_decode_worker_control(PipeClientRoleV1::Worker));
    }

    #[test]
    fn os_derived_pipe_acl_is_exact_and_rejects_unsafe_principals() {
        let expected = principal(b"expected-worker", 7);
        let sddl = fixed_pipe_sddl_for_expected_worker(&expected).expect("fixed DACL");
        assert_eq!(
            sddl,
            "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;S-1-5-21-100-200-300-400)"
        );
        for invalid in [
            ExpectedWorkerPrincipalV1 {
                sid: Vec::new(),
                sid_sddl: "S-1-5-21-100-200-300-400".into(),
                session_id: 7,
            },
            ExpectedWorkerPrincipalV1 {
                sid: b"system".to_vec(),
                sid_sddl: "S-1-5-18".into(),
                session_id: 7,
            },
            ExpectedWorkerPrincipalV1 {
                sid: b"session-zero".to_vec(),
                sid_sddl: "S-1-5-21-100-200-300-400".into(),
                session_id: 0,
            },
        ] {
            assert!(fixed_pipe_sddl_for_expected_worker(&invalid).is_err());
        }
    }
}
