//! Dormant client-side authentication of the fixed Wake install broker.
//!
//! Connecting to a named pipe grants no trust in its server. This module sends
//! no application bytes until the *connected instance* is bound to the fixed
//! running SCM service, a live pinned process, its restricted LocalSystem
//! service token, the protected image snapshot, and the exact pipe DACL.
//! The first approval write consumes the guard and repeats that attestation.
//! This is transport identity only: it does not verify an approval, provision a
//! service, receive a broker result, or authorize installation.
//!
//! The public Windows APIs expose the process image path, not the historical
//! image-section file object. The protected image hash is corroboration under
//! the separately reviewed bootstrap/ACL boundary, not loaded-image proof.

use crate::{
    Result,
    install_broker_approval::ObjectIdentityV1,
    install_broker_endpoint::FIXED_BROKER_PIPE_NAME,
    install_broker_policy::BrokerProtectedPolicyV1,
    install_broker_preflight::{SERVICE_IMAGE, SERVICE_NAME, inspect_fixed_broker_read_only},
};
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, c_void},
    fs::File,
    io::{Read, Seek, SeekFrom},
    mem::{offset_of, size_of},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr,
};

const SYSTEM_SID: &str = "S-1-5-18";
const SYSTEM_SID_BYTES: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
const SERVICE_SID_PREFIX: &str = "S-1-5-80-";
const PIPE_CLIENT_ACCESS: u32 = 0x0012_0083; // no FILE_CREATE_PIPE_INSTANCE (0x4)
const PIPE_SERVER_FULL_ACCESS: u32 = 0x001f_01ff;
const FILE_READ_DATA: u32 = 1;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const READ_CONTROL: u32 = 0x0002_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_READ: u32 = 1;
const OPEN_EXISTING: u32 = 3;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
const SECURITY_SQOS_PRESENT: u32 = 0x0010_0000;
const SECURITY_IMPERSONATION: u32 = 0x0002_0000;
const SC_MANAGER_CONNECT: u32 = 1;
const SERVICE_QUERY_STATUS: u32 = 4;
const SERVICE_RUNNING: u32 = 4;
const SERVICE_WIN32_OWN_PROCESS: u32 = 0x10;
const SC_STATUS_PROCESS_INFO: u32 = 0;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const TOKEN_QUERY: u32 = 8;
const TOKEN_USER: u32 = 1;
const TOKEN_GROUPS: u32 = 2;
const TOKEN_TYPE: u32 = 8;
const TOKEN_RESTRICTED_SIDS: u32 = 11;
const TOKEN_SESSION_ID: u32 = 12;
const TOKEN_HAS_RESTRICTIONS: u32 = 21;
const TOKEN_INTEGRITY_LEVEL: u32 = 25;
const TOKEN_IS_APP_CONTAINER: u32 = 29;
const TOKEN_PRIMARY: u32 = 1;
const SE_GROUP_ENABLED: u32 = 4;
const SYSTEM_INTEGRITY_RID: u32 = 0x4000;
const PIPE_TYPE_MESSAGE: u32 = 4;
const PIPE_SERVER_END: u32 = 1;
const OWNER_SECURITY_INFORMATION: u32 = 1;
const DACL_SECURITY_INFORMATION: u32 = 4;
const SE_KERNEL_OBJECT: u32 = 6;
const SE_DACL_PRESENT: u16 = 4;
const SE_DACL_PROTECTED: u16 = 0x1000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const MAX_IMAGE_BYTES: u64 = 80 * 1024 * 1024;
const MAX_APPROVAL_BYTES: usize = 16 * 1024;
const MAX_TOKEN_BYTES: usize = 64 * 1024;
const MAX_TOKEN_GROUPS: usize = 256;
const MAX_SID_BYTES: usize = 68;
const MAX_SECURITY_CHARS: usize = 2048;
const WRITE_TIMEOUT_MS: u32 = 5000;
const ERROR_IO_PENDING: u32 = 997;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 258;
const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct FileTime {
    low: u32,
    high: u32,
}

impl FileTime {
    fn as_u64(self) -> u64 {
        (u64::from(self.high) << 32) | u64::from(self.low)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct FileInfo {
    attributes: u32,
    creation_low: u32,
    creation_high: u32,
    access_low: u32,
    access_high: u32,
    write_low: u32,
    write_high: u32,
    volume: u32,
    size_high: u32,
    size_low: u32,
    links: u32,
    index_high: u32,
    index_low: u32,
}

impl FileInfo {
    fn identity(self) -> ObjectIdentityV1 {
        ObjectIdentityV1 {
            volume_serial: self.volume,
            file_index_high: self.index_high,
            file_index_low: self.index_low,
        }
    }

    fn length(self) -> u64 {
        (u64::from(self.size_high) << 32) | u64::from(self.size_low)
    }

    fn stable_with(self, other: Self) -> bool {
        self.attributes == other.attributes
            && self.creation_low == other.creation_low
            && self.creation_high == other.creation_high
            && self.write_low == other.write_low
            && self.write_high == other.write_high
            && self.volume == other.volume
            && self.size_high == other.size_high
            && self.size_low == other.size_low
            && self.links == other.links
            && self.index_high == other.index_high
            && self.index_low == other.index_low
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct ServiceStatusProcess {
    service_type: u32,
    current_state: u32,
    controls_accepted: u32,
    win32_exit_code: u32,
    service_specific_exit_code: u32,
    check_point: u32,
    wait_hint: u32,
    process_id: u32,
    service_flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SidAndAttributes {
    sid: *mut c_void,
    attributes: u32,
}

#[repr(C)]
struct TokenGroupsOne {
    count: u32,
    groups: [SidAndAttributes; 1],
}

#[repr(C)]
#[derive(Default)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: *mut c_void,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *mut c_void,
        disposition: u32,
        flags: u32,
        template: *mut c_void,
    ) -> *mut c_void;
    fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> *mut c_void;
    fn GetProcessId(process: *mut c_void) -> u32;
    fn GetProcessTimes(
        process: *mut c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn QueryFullProcessImageNameW(
        process: *mut c_void,
        flags: u32,
        name: *mut u16,
        length: *mut u32,
    ) -> i32;
    fn GetNamedPipeServerProcessId(pipe: *mut c_void, process_id: *mut u32) -> i32;
    fn GetNamedPipeServerSessionId(pipe: *mut c_void, session_id: *mut u32) -> i32;
    fn GetNamedPipeInfo(
        pipe: *mut c_void,
        flags: *mut u32,
        out_size: *mut u32,
        in_size: *mut u32,
        instances: *mut u32,
    ) -> i32;
    fn GetFileInformationByHandle(handle: *mut c_void, info: *mut FileInfo) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, timeout: u32) -> u32;
    fn CreateEventW(
        security: *mut c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> *mut c_void;
    fn WriteFile(
        file: *mut c_void,
        buffer: *const c_void,
        length: u32,
        written: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn GetOverlappedResult(
        file: *mut c_void,
        overlapped: *mut Overlapped,
        transferred: *mut u32,
        wait: i32,
    ) -> i32;
    fn CancelIoEx(file: *mut c_void, overlapped: *mut Overlapped) -> i32;
    fn GetLastError() -> u32;
    fn LocalFree(value: *mut c_void) -> *mut c_void;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenSCManagerW(machine: *const u16, database: *const u16, access: u32) -> *mut c_void;
    fn OpenServiceW(manager: *mut c_void, name: *const u16, access: u32) -> *mut c_void;
    fn QueryServiceStatusEx(
        service: *mut c_void,
        level: u32,
        buffer: *mut u8,
        size: u32,
        needed: *mut u32,
    ) -> i32;
    fn CloseServiceHandle(handle: *mut c_void) -> i32;
    fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
    fn GetTokenInformation(
        token: *mut c_void,
        class: u32,
        buffer: *mut c_void,
        size: u32,
        needed: *mut u32,
    ) -> i32;
    fn ConvertStringSidToSidW(value: *const u16, sid: *mut *mut c_void) -> i32;
    fn GetLengthSid(sid: *mut c_void) -> u32;
    fn GetSecurityInfo(
        handle: *mut c_void,
        object_type: u32,
        information: u32,
        owner: *mut *mut c_void,
        group: *mut *mut c_void,
        dacl: *mut *mut c_void,
        sacl: *mut *mut c_void,
        descriptor: *mut *mut c_void,
    ) -> u32;
    fn GetSecurityDescriptorControl(
        descriptor: *mut c_void,
        control: *mut u16,
        revision: *mut u32,
    ) -> i32;
    fn ConvertSecurityDescriptorToStringSecurityDescriptorW(
        descriptor: *mut c_void,
        revision: u32,
        information: u32,
        text: *mut *mut u16,
        length: *mut u32,
    ) -> i32;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        text: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        size: *mut u32,
    ) -> i32;
}

struct ServiceHandle(*mut c_void);

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseServiceHandle(self.0) };
        }
    }
}

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0) };
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain([0]).collect()
}

fn owned(raw: *mut c_void, error: &'static str) -> Result<OwnedHandle> {
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        Err(error.into())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
    }
}

fn service_handle(raw: *mut c_void, error: &'static str) -> Result<ServiceHandle> {
    if raw.is_null() {
        Err(error.into())
    } else {
        Ok(ServiceHandle(raw))
    }
}

fn expected_pipe_sddl(service_sid: &str, operator_sid: &str) -> Result<String> {
    if !service_sid.starts_with(SERVICE_SID_PREFIX)
        || !service_sid
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-' || byte == b'S')
        || !operator_sid.starts_with("S-1-")
        || !operator_sid
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-' || byte == b'S')
        || operator_sid == SYSTEM_SID
        || operator_sid == service_sid
        || PIPE_CLIENT_ACCESS & 4 != 0
    {
        return Err("WAKE_BROKER_CLIENT_POLICY_INVALID".into());
    }
    Ok(format!(
        "O:{SYSTEM_SID}D:P(A;;0x{PIPE_SERVER_FULL_ACCESS:08X};;;{SYSTEM_SID})(A;;0x{PIPE_SERVER_FULL_ACCESS:08X};;;{service_sid})(A;;0x{PIPE_CLIENT_ACCESS:08X};;;{operator_sid})"
    ))
}

fn descriptor_text(descriptor: *mut c_void) -> Result<String> {
    let mut raw = ptr::null_mut();
    let mut length = 0;
    if unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            1,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut raw,
            &mut length,
        )
    } == 0
        || raw.is_null()
        || length == 0
        || length as usize > MAX_SECURITY_CHARS
    {
        if !raw.is_null() {
            unsafe { LocalFree(raw.cast()) };
        }
        return Err("WAKE_BROKER_CLIENT_PIPE_SECURITY_INVALID".into());
    }
    let allocation = LocalAllocation(raw.cast());
    let units = unsafe { std::slice::from_raw_parts(allocation.0.cast::<u16>(), length as usize) };
    String::from_utf16(units).map_err(|_| "WAKE_BROKER_CLIENT_PIPE_SECURITY_INVALID".into())
}

fn canonical_sddl(value: &str) -> Result<String> {
    let mut raw = ptr::null_mut();
    let mut size = 0;
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(value).as_ptr(),
            1,
            &mut raw,
            &mut size,
        )
    } == 0
        || raw.is_null()
        || size == 0
        || size > 4096
    {
        if !raw.is_null() {
            unsafe { LocalFree(raw) };
        }
        return Err("WAKE_BROKER_CLIENT_PIPE_SECURITY_INVALID".into());
    }
    descriptor_text(LocalAllocation(raw).0)
}

fn pipe_security_text(pipe: &OwnedHandle) -> Result<(String, bool)> {
    let mut owner = ptr::null_mut();
    let mut dacl = ptr::null_mut();
    let mut descriptor = ptr::null_mut();
    if unsafe {
        GetSecurityInfo(
            pipe.as_raw_handle(),
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    } != 0
        || owner.is_null()
        || dacl.is_null()
        || descriptor.is_null()
    {
        if !descriptor.is_null() {
            unsafe { LocalFree(descriptor) };
        }
        return Err("WAKE_BROKER_CLIENT_PIPE_SECURITY_INVALID".into());
    }
    let allocation = LocalAllocation(descriptor);
    let mut control = 0;
    let mut revision = 0;
    if unsafe { GetSecurityDescriptorControl(allocation.0, &mut control, &mut revision) } == 0
        || control & SE_DACL_PRESENT == 0
    {
        return Err("WAKE_BROKER_CLIENT_PIPE_SECURITY_INVALID".into());
    }
    Ok((
        descriptor_text(allocation.0)?,
        control & SE_DACL_PROTECTED != 0,
    ))
}

fn exact_pipe_security(pipe: &OwnedHandle, expected: &str) -> Result<String> {
    let (actual, protected) = pipe_security_text(pipe)?;
    if !protected || actual != canonical_sddl(expected)? {
        return Err("WAKE_BROKER_CLIENT_PIPE_SECURITY_MISMATCH".into());
    }
    Ok(actual)
}

fn fixed_service() -> Result<ServiceHandle> {
    let manager = service_handle(
        unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT) },
        "WAKE_BROKER_CLIENT_SCM_UNAVAILABLE",
    )?;
    service_handle(
        unsafe { OpenServiceW(manager.0, wide(SERVICE_NAME).as_ptr(), SERVICE_QUERY_STATUS) },
        "WAKE_BROKER_CLIENT_SERVICE_UNAVAILABLE",
    )
}

fn service_status(service: &ServiceHandle) -> Result<ServiceStatusProcess> {
    let mut status = ServiceStatusProcess::default();
    let mut needed = 0;
    if unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut ServiceStatusProcess).cast(),
            size_of::<ServiceStatusProcess>() as u32,
            &mut needed,
        )
    } == 0
        || needed > size_of::<ServiceStatusProcess>() as u32
    {
        return Err("WAKE_BROKER_CLIENT_SERVICE_STATUS_INVALID".into());
    }
    Ok(status)
}

fn service_matches(status: &ServiceStatusProcess, server_pid: u32) -> bool {
    server_pid != 0
        && status.current_state == SERVICE_RUNNING
        && status.service_type == SERVICE_WIN32_OWN_PROCESS
        && status.process_id == server_pid
        && status.service_flags == 0
}

fn connected_server(pipe: &OwnedHandle) -> Result<u32> {
    let mut pid = 0;
    let mut session = u32::MAX;
    let mut flags = 0;
    let mut out_size = 0;
    let mut in_size = 0;
    let mut instances = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) } == 0
        || pid == 0
        || unsafe { GetNamedPipeServerSessionId(pipe.as_raw_handle(), &mut session) } == 0
        || session != 0
        || unsafe {
            GetNamedPipeInfo(
                pipe.as_raw_handle(),
                &mut flags,
                &mut out_size,
                &mut in_size,
                &mut instances,
            )
        } == 0
        || flags & PIPE_SERVER_END != 0
        || flags & PIPE_TYPE_MESSAGE == 0
        || instances != 1
    {
        return Err("WAKE_BROKER_CLIENT_PIPE_PEER_INVALID".into());
    }
    Ok(pid)
}

fn process_creation(process: &OwnedHandle) -> Result<u64> {
    let mut creation = FileTime::default();
    let mut exit = FileTime::default();
    let mut kernel = FileTime::default();
    let mut user = FileTime::default();
    if unsafe {
        GetProcessTimes(
            process.as_raw_handle(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
        || creation.as_u64() == 0
    {
        return Err("WAKE_BROKER_CLIENT_PROCESS_INVALID".into());
    }
    Ok(creation.as_u64())
}

fn require_live_process(process: &OwnedHandle, pid: u32, creation: u64) -> Result<()> {
    if unsafe { GetProcessId(process.as_raw_handle()) } != pid
        || unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } != WAIT_TIMEOUT
        || process_creation(process)? != creation
    {
        return Err("WAKE_BROKER_CLIENT_PROCESS_CHANGED".into());
    }
    Ok(())
}

fn pinned_process(pid: u32) -> Result<(OwnedHandle, u64)> {
    let process = owned(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) },
        "WAKE_BROKER_CLIENT_PROCESS_UNAVAILABLE",
    )?;
    let creation = process_creation(&process)?;
    require_live_process(&process, pid, creation)?;
    Ok((process, creation))
}

fn exact_process_image_path(process: &OwnedHandle) -> Result<()> {
    // This is the process's recorded executable path, not an image-section ID.
    // The fixed protected file object is separately pinned and hashed below.
    let mut units = [0_u16; 512];
    let mut length = units.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(process.as_raw_handle(), 0, units.as_mut_ptr(), &mut length)
    } == 0
        || length == 0
        || length as usize >= units.len()
    {
        return Err("WAKE_BROKER_CLIENT_PROCESS_IMAGE_INVALID".into());
    }
    let path = String::from_utf16(&units[..length as usize])
        .map_err(|_| "WAKE_BROKER_CLIENT_PROCESS_IMAGE_INVALID")?;
    if !path.eq_ignore_ascii_case(SERVICE_IMAGE) {
        return Err("WAKE_BROKER_CLIENT_PROCESS_IMAGE_MISMATCH".into());
    }
    Ok(())
}

fn image_file_info(file: &File) -> Result<FileInfo> {
    let mut info = FileInfo::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
        || info.attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
        || info.links != 1
        || info.volume == 0
        || (info.index_high == 0 && info.index_low == 0)
        || info.length() == 0
        || info.length() > MAX_IMAGE_BYTES
    {
        return Err("WAKE_BROKER_CLIENT_IMAGE_OBJECT_INVALID".into());
    }
    Ok(info)
}

fn read_pinned_image(file: &mut File) -> Result<(FileInfo, String)> {
    let before = image_file_info(file)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_BROKER_CLIENT_IMAGE_READ_FAILED")?;
    let mut bytes = Vec::with_capacity(before.length() as usize);
    file.take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WAKE_BROKER_CLIENT_IMAGE_READ_FAILED")?;
    let after = image_file_info(file)?;
    if !before.stable_with(after) || bytes.len() as u64 != before.length() {
        return Err("WAKE_BROKER_CLIENT_IMAGE_OBJECT_CHANGED".into());
    }
    Ok((before, format!("{:x}", Sha256::digest(&bytes))))
}

fn open_fixed_image() -> Result<File> {
    let handle = owned(
        unsafe {
            CreateFileW(
                wide(SERVICE_IMAGE).as_ptr(),
                FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
                FILE_SHARE_READ, // prevent write/delete while this guard lives
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        },
        "WAKE_BROKER_CLIENT_IMAGE_UNAVAILABLE",
    )?;
    Ok(File::from(handle))
}

fn query_token_blob(token: &OwnedHandle, class: u32) -> Result<Vec<u8>> {
    let mut needed = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    if needed == 0 || needed as usize > MAX_TOKEN_BYTES {
        return Err("WAKE_BROKER_CLIENT_TOKEN_INVALID".into());
    }
    let mut bytes = vec![0_u8; needed as usize];
    let mut returned = 0;
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            bytes.as_mut_ptr().cast(),
            needed,
            &mut returned,
        )
    } == 0
        || returned == 0
        || returned > needed
    {
        return Err("WAKE_BROKER_CLIENT_TOKEN_INVALID".into());
    }
    bytes.truncate(returned as usize);
    Ok(bytes)
}

fn token_u32(token: &OwnedHandle, class: u32) -> Result<u32> {
    let bytes = query_token_blob(token, class)?;
    let array: [u8; 4] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| "WAKE_BROKER_CLIENT_TOKEN_INVALID")?;
    Ok(u32::from_le_bytes(array))
}

fn token_has_restrictions(token: &OwnedHandle) -> Result<bool> {
    // Current Windows has been observed to return one byte for class 21.
    let mut bytes = [0xa5_u8; 4];
    let mut returned = 0;
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TOKEN_HAS_RESTRICTIONS,
            bytes.as_mut_ptr().cast(),
            bytes.len() as u32,
            &mut returned,
        )
    } == 0
    {
        return Err("WAKE_BROKER_CLIENT_TOKEN_INVALID".into());
    }
    match returned {
        1 => Ok(bytes[0] != 0),
        4 => Ok(u32::from_le_bytes(bytes) != 0),
        _ => Err("WAKE_BROKER_CLIENT_TOKEN_INVALID".into()),
    }
}

fn checked_sid(blob: &[u8], entry: SidAndAttributes) -> Result<&[u8]> {
    let offset = entry
        .sid
        .addr()
        .checked_sub(blob.as_ptr().addr())
        .ok_or("WAKE_BROKER_CLIENT_TOKEN_INVALID")?;
    let head = blob
        .get(
            offset
                ..offset
                    .checked_add(8)
                    .ok_or("WAKE_BROKER_CLIENT_TOKEN_INVALID")?,
        )
        .ok_or("WAKE_BROKER_CLIENT_TOKEN_INVALID")?;
    let count = usize::from(head[1]);
    let length = 8 + count * 4;
    if head[0] != 1 || count == 0 || count > 15 || length > MAX_SID_BYTES {
        return Err("WAKE_BROKER_CLIENT_TOKEN_INVALID".into());
    }
    blob.get(
        offset
            ..offset
                .checked_add(length)
                .ok_or("WAKE_BROKER_CLIENT_TOKEN_INVALID")?,
    )
    .ok_or_else(|| "WAKE_BROKER_CLIENT_TOKEN_INVALID".into())
}

fn single_token_sid(bytes: &[u8]) -> Result<&[u8]> {
    if bytes.len() < size_of::<SidAndAttributes>() {
        return Err("WAKE_BROKER_CLIENT_TOKEN_INVALID".into());
    }
    let entry = unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<SidAndAttributes>()) };
    checked_sid(bytes, entry)
}

fn group_contains(bytes: &[u8], expected: &[u8], require_enabled: bool) -> Result<bool> {
    if bytes.len() < offset_of!(TokenGroupsOne, groups) {
        return Err("WAKE_BROKER_CLIENT_TOKEN_INVALID".into());
    }
    let count = u32::from_le_bytes(
        bytes[..4]
            .try_into()
            .map_err(|_| "WAKE_BROKER_CLIENT_TOKEN_INVALID")?,
    ) as usize;
    if count == 0 || count > MAX_TOKEN_GROUPS {
        return Err("WAKE_BROKER_CLIENT_TOKEN_INVALID".into());
    }
    let start = offset_of!(TokenGroupsOne, groups);
    let end = start
        .checked_add(
            count
                .checked_mul(size_of::<SidAndAttributes>())
                .ok_or("WAKE_BROKER_CLIENT_TOKEN_INVALID")?,
        )
        .ok_or("WAKE_BROKER_CLIENT_TOKEN_INVALID")?;
    if end > bytes.len() {
        return Err("WAKE_BROKER_CLIENT_TOKEN_INVALID".into());
    }
    let mut found = false;
    for index in 0..count {
        let entry = unsafe {
            ptr::read_unaligned(
                bytes
                    .as_ptr()
                    .add(start + index * size_of::<SidAndAttributes>())
                    .cast::<SidAndAttributes>(),
            )
        };
        let sid = checked_sid(bytes, entry)?;
        if sid == expected && (!require_enabled || entry.attributes & SE_GROUP_ENABLED != 0) {
            found = true;
        }
    }
    Ok(found)
}

fn service_sid_bytes(value: &str) -> Result<Vec<u8>> {
    if !value.starts_with(SERVICE_SID_PREFIX) || value.len() > 128 {
        return Err("WAKE_BROKER_CLIENT_POLICY_INVALID".into());
    }
    let mut raw = ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(wide(value).as_ptr(), &mut raw) } == 0 || raw.is_null() {
        return Err("WAKE_BROKER_CLIENT_POLICY_INVALID".into());
    }
    let allocation = LocalAllocation(raw);
    let length = unsafe { GetLengthSid(allocation.0) } as usize;
    if length == 0 || length > MAX_SID_BYTES {
        return Err("WAKE_BROKER_CLIENT_POLICY_INVALID".into());
    }
    Ok(unsafe { std::slice::from_raw_parts(allocation.0.cast::<u8>(), length) }.to_vec())
}

fn exact_process_token(process: &OwnedHandle, service_sid: &[u8]) -> Result<()> {
    let token = owned(
        {
            let mut raw = ptr::null_mut();
            if unsafe { OpenProcessToken(process.as_raw_handle(), TOKEN_QUERY, &mut raw) } == 0 {
                return Err("WAKE_BROKER_CLIENT_TOKEN_UNAVAILABLE".into());
            }
            raw
        },
        "WAKE_BROKER_CLIENT_TOKEN_UNAVAILABLE",
    )?;
    let user = query_token_blob(&token, TOKEN_USER)?;
    let integrity = query_token_blob(&token, TOKEN_INTEGRITY_LEVEL)?;
    let integrity_sid = single_token_sid(&integrity)?;
    if single_token_sid(&user)? != SYSTEM_SID_BYTES
        || token_u32(&token, TOKEN_TYPE)? != TOKEN_PRIMARY
        || token_u32(&token, TOKEN_SESSION_ID)? != 0
        || token_u32(&token, TOKEN_IS_APP_CONTAINER)? != 0
        || !token_has_restrictions(&token)?
        || integrity_sid.len() != 12
        || integrity_sid[2..8] != [0, 0, 0, 0, 0, 16]
        || u32::from_le_bytes(integrity_sid[8..12].try_into().expect("four bytes"))
            != SYSTEM_INTEGRITY_RID
        || !group_contains(&query_token_blob(&token, TOKEN_GROUPS)?, service_sid, true)?
        || !group_contains(
            &query_token_blob(&token, TOKEN_RESTRICTED_SIDS)?,
            service_sid,
            false,
        )?
    {
        return Err("WAKE_BROKER_CLIENT_TOKEN_MISMATCH".into());
    }
    Ok(())
}

/// Owns the one connected pipe, pinned service process and fixed image until
/// the first approval write. It cannot be built from caller-provided identity.
pub struct FixedBrokerApprovalClient<'a> {
    policy: &'a BrokerProtectedPolicyV1,
    pipe: OwnedHandle,
    service: ServiceHandle,
    process: OwnedHandle,
    image: File,
    server_pid: u32,
    process_creation: u64,
    image_info: FileInfo,
    image_sha256: String,
    service_sid_bytes: Vec<u8>,
    pipe_sddl: String,
}

impl FixedBrokerApprovalClient<'_> {
    fn revalidate(&mut self) -> Result<()> {
        self.policy.revalidate()?;
        let snapshot = inspect_fixed_broker_read_only()?;
        if snapshot.service_image != self.image_info.identity()
            || snapshot.service_image_sha256 != self.image_sha256
            || connected_server(&self.pipe)? != self.server_pid
            || !service_matches(&service_status(&self.service)?, self.server_pid)
        {
            return Err("WAKE_BROKER_CLIENT_IDENTITY_CHANGED".into());
        }
        require_live_process(&self.process, self.server_pid, self.process_creation)?;
        exact_process_image_path(&self.process)?;
        exact_process_token(&self.process, &self.service_sid_bytes)?;
        let (info, digest) = read_pinned_image(&mut self.image)?;
        if !info.stable_with(self.image_info) || digest != self.image_sha256 {
            return Err("WAKE_BROKER_CLIENT_IMAGE_CHANGED".into());
        }
        if exact_pipe_security(&self.pipe, &self.pipe_sddl)? != self.pipe_sddl {
            return Err("WAKE_BROKER_CLIENT_PIPE_SECURITY_MISMATCH".into());
        }
        // These are last because a stopped/restarted service must be caught as
        // close as possible to the single WriteFile below.
        require_live_process(&self.process, self.server_pid, self.process_creation)?;
        if connected_server(&self.pipe)? != self.server_pid
            || !service_matches(&service_status(&self.service)?, self.server_pid)
        {
            return Err("WAKE_BROKER_CLIENT_IDENTITY_CHANGED".into());
        }
        Ok(())
    }

    /// Reattest and issue exactly one bounded approval-message write on this
    /// same pipe instance. A timeout or short write has indeterminate delivery;
    /// callers must never retry it as a new approval without ledger readback.
    /// The bytes are only transported: the fixed service still must authorize
    /// the operator, signature, stage and replay transaction independently.
    pub fn write_one_approval(mut self, approval: &[u8]) -> Result<()> {
        if approval.is_empty() || approval.len() > MAX_APPROVAL_BYTES {
            return Err("WAKE_BROKER_CLIENT_APPROVAL_SIZE_INVALID".into());
        }
        self.revalidate()?;
        write_one_message(&self.pipe, approval)
    }
}

/// Connect only to the compiled local pipe. No request data is sent here.
/// Failure to inspect another-account SYSTEM token (including lack of
/// SeDebugPrivilege) is a denial; this code never enables a privilege.
pub fn connect_fixed_broker_for_approval(
    policy: &BrokerProtectedPolicyV1,
) -> Result<FixedBrokerApprovalClient<'_>> {
    connect_at(policy, FIXED_BROKER_PIPE_NAME)
}

fn open_client_pipe(pipe_name: &str) -> Result<OwnedHandle> {
    owned(
        unsafe {
            CreateFileW(
                wide(pipe_name).as_ptr(),
                PIPE_CLIENT_ACCESS,
                0,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IMPERSONATION,
                ptr::null_mut(),
            )
        },
        "WAKE_BROKER_CLIENT_PIPE_UNAVAILABLE",
    )
}

fn connect_at<'a>(
    policy: &'a BrokerProtectedPolicyV1,
    pipe_name: &str,
) -> Result<FixedBrokerApprovalClient<'a>> {
    let (service_sid, operator_sid) = policy.checked_endpoint_sids()?;
    let expected_pipe = expected_pipe_sddl(service_sid, operator_sid)?;
    let service_sid_bytes = service_sid_bytes(service_sid)?;
    let snapshot = inspect_fixed_broker_read_only()?;
    let pipe = open_client_pipe(pipe_name)?;
    // The connected object, not the name or an earlier WaitNamedPipe lookup,
    // supplies all peer facts. Nothing has yet been written to this pipe.
    let pipe_sddl = exact_pipe_security(&pipe, &expected_pipe)?;
    let server_pid = connected_server(&pipe)?;
    let service = fixed_service()?;
    if !service_matches(&service_status(&service)?, server_pid) {
        return Err("WAKE_BROKER_CLIENT_SERVICE_MISMATCH".into());
    }
    let (process, process_creation) = pinned_process(server_pid)?;
    exact_process_image_path(&process)?;
    exact_process_token(&process, &service_sid_bytes)?;
    let mut image = open_fixed_image()?;
    let (image_info, image_sha256) = read_pinned_image(&mut image)?;
    if snapshot.service_image != image_info.identity()
        || snapshot.service_image_sha256 != image_sha256
        || !service_matches(&service_status(&service)?, server_pid)
    {
        return Err("WAKE_BROKER_CLIENT_IMAGE_MISMATCH".into());
    }
    let mut client = FixedBrokerApprovalClient {
        policy,
        pipe,
        service,
        process,
        image,
        server_pid,
        process_creation,
        image_info,
        image_sha256,
        service_sid_bytes,
        pipe_sddl,
    };
    client.revalidate()?;
    Ok(client)
}

fn write_one_message(pipe: &OwnedHandle, approval: &[u8]) -> Result<()> {
    let event = owned(
        unsafe { CreateEventW(ptr::null_mut(), 1, 0, ptr::null()) },
        "WAKE_BROKER_CLIENT_WRITE_EVENT_FAILED",
    )?;
    let mut overlapped = Overlapped {
        event: event.as_raw_handle(),
        ..Overlapped::default()
    };
    let started = unsafe {
        WriteFile(
            pipe.as_raw_handle(),
            approval.as_ptr().cast(),
            approval.len() as u32,
            ptr::null_mut(),
            (&mut overlapped as *mut Overlapped).cast(),
        )
    };
    if started == 0 {
        if unsafe { GetLastError() } != ERROR_IO_PENDING {
            return Err("WAKE_BROKER_CLIENT_WRITE_FAILED".into());
        }
        match unsafe { WaitForSingleObject(event.as_raw_handle(), WRITE_TIMEOUT_MS) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => {
                unsafe { CancelIoEx(pipe.as_raw_handle(), &mut overlapped) };
                // OVERLAPPED and approval must outlive kernel I/O. A second
                // bounded wait preserves memory safety; a broken kernel/driver
                // leaves no safe in-process recovery, so fail closed by exit.
                if unsafe { WaitForSingleObject(event.as_raw_handle(), WRITE_TIMEOUT_MS) }
                    != WAIT_OBJECT_0
                {
                    std::process::abort();
                }
                let mut ignored = 0;
                unsafe {
                    GetOverlappedResult(pipe.as_raw_handle(), &mut overlapped, &mut ignored, 0)
                };
                return Err("WAKE_BROKER_CLIENT_WRITE_TIMEOUT_INDETERMINATE".into());
            }
            _ => {
                unsafe { CancelIoEx(pipe.as_raw_handle(), &mut overlapped) };
                if unsafe { WaitForSingleObject(event.as_raw_handle(), WRITE_TIMEOUT_MS) }
                    != WAIT_OBJECT_0
                {
                    std::process::abort();
                }
                return Err("WAKE_BROKER_CLIENT_WRITE_WAIT_FAILED".into());
            }
        }
    }
    let mut written = 0;
    if unsafe { GetOverlappedResult(pipe.as_raw_handle(), &mut overlapped, &mut written, 0) } == 0
        || written as usize != approval.len()
    {
        return Err("WAKE_BROKER_CLIENT_WRITE_INDETERMINATE".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    const PIPE_ACCESS_DUPLEX: u32 = 3;
    const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
    const PIPE_READMODE_MESSAGE: u32 = 2;
    const PIPE_REJECT_REMOTE_CLIENTS: u32 = 8;
    const ERROR_PIPE_CONNECTED: u32 = 535;
    static NEXT: AtomicU64 = AtomicU64::new(1);

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateNamedPipeW(
            name: *const u16,
            open_mode: u32,
            pipe_mode: u32,
            instances: u32,
            out_size: u32,
            in_size: u32,
            timeout: u32,
            security: *mut c_void,
        ) -> *mut c_void;
        fn ConnectNamedPipe(pipe: *mut c_void, overlapped: *mut c_void) -> i32;
        fn PeekNamedPipe(
            pipe: *mut c_void,
            buffer: *mut c_void,
            size: u32,
            read: *mut u32,
            available: *mut u32,
            left: *mut u32,
        ) -> i32;
        fn ReadFile(
            file: *mut c_void,
            buffer: *mut c_void,
            size: u32,
            read: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn GetCurrentProcessId() -> u32;
    }

    fn fixture_pipe() -> (String, OwnedHandle) {
        let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            r"\\.\pipe\CatDeskBrokerClientFixture-{}-{suffix}",
            std::process::id()
        );
        let server = owned(
            unsafe {
                CreateNamedPipeW(
                    wide(&name).as_ptr(),
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    4096,
                    4096,
                    0,
                    ptr::null_mut(),
                )
            },
            "fixture create failed",
        )
        .unwrap();
        (name, server)
    }

    #[test]
    fn fixed_service_requires_running_own_process_and_same_pid() {
        let good = ServiceStatusProcess {
            service_type: SERVICE_WIN32_OWN_PROCESS,
            current_state: SERVICE_RUNNING,
            process_id: 42,
            ..ServiceStatusProcess::default()
        };
        assert!(service_matches(&good, 42));
        assert!(!service_matches(&good, 43));
        assert!(!service_matches(&good, 0));
        assert!(!service_matches(
            &ServiceStatusProcess {
                current_state: 1,
                ..good
            },
            42
        ));
        assert!(!service_matches(
            &ServiceStatusProcess {
                service_type: 0x20,
                ..good
            },
            42
        ));
        assert!(!service_matches(
            &ServiceStatusProcess {
                service_flags: 1,
                ..good
            },
            42
        ));
    }

    #[test]
    fn token_group_pointer_is_bounded_and_enabled_is_required() {
        let start = offset_of!(TokenGroupsOne, groups);
        let sid_offset = start + size_of::<SidAndAttributes>();
        let mut bytes = vec![0_u8; sid_offset + SYSTEM_SID_BYTES.len()];
        bytes[..4].copy_from_slice(&1_u32.to_le_bytes());
        bytes[sid_offset..].copy_from_slice(&SYSTEM_SID_BYTES);
        let entry = SidAndAttributes {
            sid: unsafe { bytes.as_mut_ptr().add(sid_offset).cast() },
            attributes: SE_GROUP_ENABLED,
        };
        unsafe {
            ptr::write_unaligned(
                bytes.as_mut_ptr().add(start).cast::<SidAndAttributes>(),
                entry,
            )
        };
        assert!(group_contains(&bytes, &SYSTEM_SID_BYTES, true).unwrap());
        let disabled = SidAndAttributes {
            attributes: 0,
            ..entry
        };
        unsafe {
            ptr::write_unaligned(
                bytes.as_mut_ptr().add(start).cast::<SidAndAttributes>(),
                disabled,
            )
        };
        assert!(!group_contains(&bytes, &SYSTEM_SID_BYTES, true).unwrap());
        assert!(group_contains(&bytes, &SYSTEM_SID_BYTES, false).unwrap());
        let outside = SidAndAttributes {
            sid: ptr::dangling_mut::<c_void>(),
            attributes: SE_GROUP_ENABLED,
        };
        unsafe {
            ptr::write_unaligned(
                bytes.as_mut_ptr().add(start).cast::<SidAndAttributes>(),
                outside,
            )
        };
        assert!(group_contains(&bytes, &SYSTEM_SID_BYTES, false).is_err());
        bytes[..4].copy_from_slice(&(MAX_TOKEN_GROUPS as u32 + 1).to_le_bytes());
        assert!(group_contains(&bytes, &SYSTEM_SID_BYTES, false).is_err());
    }

    #[test]
    fn isolated_same_user_squat_gets_no_approval_bytes() {
        let (name, server) = fixture_pipe();
        assert_ne!(name, FIXED_BROKER_PIPE_NAME);
        let client = open_client_pipe(&name).expect("client connects only");
        let mut reported_pid = 0;
        assert_ne!(
            unsafe { GetNamedPipeServerProcessId(client.as_raw_handle(), &mut reported_pid) },
            0
        );
        assert_eq!(reported_pid, unsafe { GetCurrentProcessId() });
        // The squatter's default current-user descriptor is not the fixed
        // SYSTEM/service/operator descriptor, even though its name is valid.
        let forged = expected_pipe_sddl("S-1-5-80-1", "S-1-5-21-1-2-3-1001").unwrap();
        assert!(!pipe_security_text(&client).unwrap().0.is_empty());
        assert!(exact_pipe_security(&client, &forged).is_err());
        let mut available = u32::MAX;
        assert_ne!(
            unsafe {
                PeekNamedPipe(
                    server.as_raw_handle(),
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    &mut available,
                    ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!(available, 0);
    }

    #[test]
    fn isolated_overlapped_transport_writes_one_message() {
        let (name, server) = fixture_pipe();
        let client = open_client_pipe(&name).expect("fixture client");
        let reader = std::thread::spawn(move || {
            let connected = unsafe { ConnectNamedPipe(server.as_raw_handle(), ptr::null_mut()) };
            assert!(connected != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED);
            let mut bytes = [0_u8; 64];
            let mut read = 0;
            assert_ne!(
                unsafe {
                    ReadFile(
                        server.as_raw_handle(),
                        bytes.as_mut_ptr().cast(),
                        bytes.len() as u32,
                        &mut read,
                        ptr::null_mut(),
                    )
                },
                0
            );
            bytes[..read as usize].to_vec()
        });
        write_one_message(&client, b"isolated-one-message").unwrap();
        assert_eq!(reader.join().unwrap(), b"isolated-one-message");
    }
}
