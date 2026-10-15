//! Dormant constructor and readback for the fixed Wake install-broker pipe.
//!
//! The operator SID comes only from the pinned, independently protected
//! `BrokerProtectedPolicyV1` loader. The endpoint's security readback is not
//! an install authorization. A future service must prove its image/token,
//! serialize each request
//! read with connected-client impersonation, and verify signed approval and
//! transaction state. This module does not start a service or alter the
//! protected package namespace.
//! A same-user process can squat the pipe name before service startup; the
//! first-instance flag makes the service fail closed, but a future client must
//! authenticate the connected server's process/image/token before sending an
//! approval. The pipe name and owner SID alone are not server identity.

use crate::{
    Result,
    install_broker_policy::BrokerProtectedPolicyV1,
    install_broker_preflight::{
        fixed_broker_service_sid_for_endpoint, inspect_fixed_broker_read_only,
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, c_void},
    os::windows::{
        ffi::OsStrExt,
        io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
    },
    ptr,
};

pub const FIXED_BROKER_PIPE_NAME: &str = r"\\.\pipe\CatDeskWakeInstallBrokerV1";
const SYSTEM_SID: &str = "S-1-5-18";
const PIPE_ACCESS_DUPLEX: u32 = 3;
const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
const PIPE_TYPE_MESSAGE: u32 = 4;
const PIPE_READMODE_MESSAGE: u32 = 2;
const PIPE_REJECT_REMOTE_CLIENTS: u32 = 8;
const PIPE_SERVER_END: u32 = 1;
const HANDLE_FLAG_INHERIT: u32 = 1;
const SERVER_FULL_ACCESS: u32 = 0x001f_01ff;
// Data read/write, read attributes (needed by CreateFileW), READ_CONTROL
// (connected-client descriptor readback), and synchronize.
// In particular, exclude 0x4 FILE_CREATE_PIPE_INSTANCE and every generic
// write/full-access right. An isolated Windows test exercises both exchange
// and rejection of a competing second server instance under this trustee.
const OPERATOR_CLIENT_ACCESS: u32 = 0x0012_0083;
const FILE_CREATE_PIPE_INSTANCE: u32 = 4;
const OWNER_SECURITY_INFORMATION: u32 = 1;
const DACL_SECURITY_INFORMATION: u32 = 4;
const SE_KERNEL_OBJECT: u32 = 6;
const SE_DACL_PRESENT: u16 = 4;
const SE_DACL_PROTECTED: u16 = 0x1000;
const MAX_SID_CHARS: usize = 128;
const MAX_SECURITY_CHARS: usize = 2048;
const MAX_SECURITY_BYTES: u32 = 4096;
const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

#[repr(C)]
struct SecurityAttributes {
    length: u32,
    descriptor: *mut c_void,
    inherit_handle: i32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateNamedPipeW(
        name: *const u16,
        open_mode: u32,
        pipe_mode: u32,
        max_instances: u32,
        out_buffer: u32,
        in_buffer: u32,
        default_timeout: u32,
        security: *mut c_void,
    ) -> *mut c_void;
    fn GetNamedPipeInfo(
        pipe: *mut c_void,
        flags: *mut u32,
        out_size: *mut u32,
        in_size: *mut u32,
        max_instances: *mut u32,
    ) -> i32;
    fn GetHandleInformation(handle: *mut c_void, flags: *mut u32) -> i32;
    fn LocalFree(value: *mut c_void) -> *mut c_void;
    fn lstrlenW(value: *const u16) -> i32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        text: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        size: *mut u32,
    ) -> i32;
    fn ConvertSecurityDescriptorToStringSecurityDescriptorW(
        descriptor: *mut c_void,
        revision: u32,
        information: u32,
        text: *mut *mut u16,
        length: *mut u32,
    ) -> i32;
    fn ConvertStringSidToSidW(text: *const u16, sid: *mut *mut c_void) -> i32;
    fn ConvertSidToStringSidW(sid: *mut c_void, text: *mut *mut u16) -> i32;
    fn LookupAccountSidW(
        system: *const u16,
        sid: *mut c_void,
        account: *mut u16,
        account_length: *mut u32,
        domain: *mut u16,
        domain_length: *mut u32,
        use_type: *mut u32,
    ) -> i32;
    fn IsValidSid(sid: *mut c_void) -> i32;
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
}

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0) };
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixedBrokerEndpointReadbackV1 {
    pub schema_version: u32,
    pub pipe_name: String,
    pub owner_sid: String,
    pub service_sid_sha256: String,
    pub operator_sid_sha256: String,
    pub security_descriptor_sha256: String,
    pub message_pipe: bool,
    pub one_instance: bool,
    pub remote_clients_rejected_by_constructor: bool,
    pub disposition: &'static str,
}

/// Owns one fixed pipe instance. Dropping it closes the endpoint.
pub struct FixedBrokerPipeEndpoint {
    handle: OwnedHandle,
    readback: FixedBrokerEndpointReadbackV1,
}

impl FixedBrokerPipeEndpoint {
    pub fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }

    pub fn readback(&self) -> &FixedBrokerEndpointReadbackV1 {
        &self.readback
    }
}

enum EndpointName {
    Fixed,
    #[cfg(test)]
    Fixture(String),
}

impl EndpointName {
    fn value(&self) -> &str {
        match self {
            Self::Fixed => FIXED_BROKER_PIPE_NAME,
            #[cfg(test)]
            Self::Fixture(name) => name,
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain([0]).collect()
}

fn allocated_text(
    pointer: *const u16,
    reported_length: Option<usize>,
    max: usize,
) -> Result<String> {
    if pointer.is_null() {
        return Err("WAKE_BROKER_ENDPOINT_SECURITY_INVALID".into());
    }
    // Both conversion APIs return NUL-terminated LocalAlloc text. The SID
    // conversion omits a length, so ask Windows for its actual length before
    // materializing a Rust slice; the descriptor conversion supplies one.
    let length = match reported_length {
        Some(value) => value,
        None => {
            let value = unsafe { lstrlenW(pointer) };
            if value <= 0 {
                return Err("WAKE_BROKER_ENDPOINT_SECURITY_INVALID".into());
            }
            value as usize
        }
    };
    if length == 0 || length > max {
        return Err("WAKE_BROKER_ENDPOINT_SECURITY_INVALID".into());
    }
    let units = unsafe { std::slice::from_raw_parts(pointer, length) };
    String::from_utf16(units).map_err(|_| "WAKE_BROKER_ENDPOINT_SECURITY_INVALID".into())
}

fn sid_text(sid: *mut c_void) -> Result<String> {
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 || unsafe { GetLengthSid(sid) } > 256 {
        return Err("WAKE_BROKER_ENDPOINT_SECURITY_INVALID".into());
    }
    let mut raw = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut raw) } == 0 || raw.is_null() {
        return Err("WAKE_BROKER_ENDPOINT_SECURITY_INVALID".into());
    }
    let allocation = LocalAllocation(raw.cast());
    allocated_text(allocation.0.cast(), None, MAX_SID_CHARS)
}

fn validate_sid(value: &str) -> Result<()> {
    if value.len() > MAX_SID_CHARS
        || !value.starts_with("S-1-")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'S' || byte == b'-')
    {
        return Err("WAKE_BROKER_ENDPOINT_POLICY_INVALID".into());
    }
    let mut raw = ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(wide(value).as_ptr(), &mut raw) } == 0 || raw.is_null() {
        return Err("WAKE_BROKER_ENDPOINT_POLICY_INVALID".into());
    }
    let sid = LocalAllocation(raw);
    if sid_text(sid.0)? != value {
        return Err("WAKE_BROKER_ENDPOINT_POLICY_INVALID".into());
    }
    Ok(())
}

fn require_user_account_sid(value: &str) -> Result<()> {
    validate_sid(value)?;
    let mut raw = ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(wide(value).as_ptr(), &mut raw) } == 0 || raw.is_null() {
        return Err("WAKE_BROKER_ENDPOINT_POLICY_INVALID".into());
    }
    let sid = LocalAllocation(raw);
    let mut account_length = 0;
    let mut domain_length = 0;
    let mut use_type = 0;
    unsafe {
        LookupAccountSidW(
            ptr::null(),
            sid.0,
            ptr::null_mut(),
            &mut account_length,
            ptr::null_mut(),
            &mut domain_length,
            &mut use_type,
        )
    };
    if account_length == 0 || account_length > 256 || domain_length > 256 {
        return Err("WAKE_BROKER_ENDPOINT_POLICY_INVALID".into());
    }
    let mut account = vec![0u16; account_length as usize];
    let mut domain = vec![0u16; domain_length.max(1) as usize];
    if unsafe {
        LookupAccountSidW(
            ptr::null(),
            sid.0,
            account.as_mut_ptr(),
            &mut account_length,
            domain.as_mut_ptr(),
            &mut domain_length,
            &mut use_type,
        )
    } == 0
        || use_type != 1
    // SidTypeUser, never a group/alias/logon SID
    {
        return Err("WAKE_BROKER_ENDPOINT_POLICY_INVALID".into());
    }
    Ok(())
}

fn expected_sddl(owner: &str, service: &str, operator: &str) -> Result<String> {
    for sid in [owner, service, operator] {
        validate_sid(sid)?;
    }
    // A client ACE must not include FILE_APPEND_DATA (bit 0x4): on named
    // pipes that same bit is FILE_CREATE_PIPE_INSTANCE. Broad generic write
    // or full access would let an operator create a competing server.
    if operator == service
        || operator == SYSTEM_SID
        || OPERATOR_CLIENT_ACCESS & FILE_CREATE_PIPE_INSTANCE != 0
        || !service.starts_with("S-1-5-")
    {
        return Err("WAKE_BROKER_ENDPOINT_POLICY_INVALID".into());
    }
    Ok(format!(
        "O:{owner}D:P(A;;0x{SERVER_FULL_ACCESS:08X};;;{SYSTEM_SID})(A;;0x{SERVER_FULL_ACCESS:08X};;;{service})(A;;0x{OPERATOR_CLIENT_ACCESS:08X};;;{operator})"
    ))
}

fn descriptor_from_sddl(value: &str) -> Result<LocalAllocation> {
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
        || size > MAX_SECURITY_BYTES
    {
        if !raw.is_null() {
            unsafe { LocalFree(raw) };
        }
        return Err("WAKE_BROKER_ENDPOINT_POLICY_INVALID".into());
    }
    Ok(LocalAllocation(raw))
}

fn descriptor_sddl(descriptor: *mut c_void) -> Result<String> {
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
        return Err("WAKE_BROKER_ENDPOINT_SECURITY_INVALID".into());
    }
    let allocation = LocalAllocation(raw.cast());
    allocated_text(
        allocation.0.cast(),
        Some(length as usize),
        MAX_SECURITY_CHARS,
    )
}

fn readback_security(handle: &OwnedHandle, expected: &str, owner_sid: &str) -> Result<String> {
    let mut owner = ptr::null_mut();
    let mut dacl = ptr::null_mut();
    let mut descriptor = ptr::null_mut();
    if unsafe {
        GetSecurityInfo(
            handle.as_raw_handle(),
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
        return Err("WAKE_BROKER_ENDPOINT_SECURITY_INVALID".into());
    }
    let allocation = LocalAllocation(descriptor);
    let mut control = 0;
    let mut revision = 0;
    if unsafe { GetSecurityDescriptorControl(allocation.0, &mut control, &mut revision) } == 0
        || control & (SE_DACL_PRESENT | SE_DACL_PROTECTED) != SE_DACL_PRESENT | SE_DACL_PROTECTED
        || sid_text(owner)? != owner_sid
    {
        return Err("WAKE_BROKER_ENDPOINT_SECURITY_MISMATCH".into());
    }
    let actual = descriptor_sddl(allocation.0)?;
    let canonical_expected = descriptor_sddl(descriptor_from_sddl(expected)?.0)?;
    if actual != canonical_expected {
        return Err("WAKE_BROKER_ENDPOINT_SECURITY_MISMATCH".into());
    }
    Ok(actual)
}

fn create_endpoint(
    name: EndpointName,
    owner_sid: &str,
    service_sid: &str,
    operator_sid: &str,
) -> Result<FixedBrokerPipeEndpoint> {
    let sddl = expected_sddl(owner_sid, service_sid, operator_sid)?;
    let descriptor = descriptor_from_sddl(&sddl)?;
    let mut security = SecurityAttributes {
        length: std::mem::size_of::<SecurityAttributes>() as u32,
        descriptor: descriptor.0,
        inherit_handle: 0,
    };
    let name_value = name.value();
    let raw = unsafe {
        CreateNamedPipeW(
            wide(name_value).as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            4096,
            0,
            (&mut security as *mut SecurityAttributes).cast(),
        )
    };
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err("WAKE_BROKER_ENDPOINT_CREATE_REFUSED".into());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let actual_sddl = readback_security(&handle, &sddl, owner_sid)?;
    let mut handle_flags = 0;
    if unsafe { GetHandleInformation(handle.as_raw_handle(), &mut handle_flags) } == 0
        || handle_flags & HANDLE_FLAG_INHERIT != 0
    {
        return Err("WAKE_BROKER_ENDPOINT_HANDLE_INHERITABLE".into());
    }
    let mut flags = 0;
    let mut out_size = 0;
    let mut in_size = 0;
    let mut max_instances = 0;
    if unsafe {
        GetNamedPipeInfo(
            handle.as_raw_handle(),
            &mut flags,
            &mut out_size,
            &mut in_size,
            &mut max_instances,
        )
    } == 0
        || flags & (PIPE_SERVER_END | PIPE_TYPE_MESSAGE) != PIPE_SERVER_END | PIPE_TYPE_MESSAGE
        || max_instances != 1
    {
        return Err("WAKE_BROKER_ENDPOINT_MODE_MISMATCH".into());
    }
    Ok(FixedBrokerPipeEndpoint {
        handle,
        readback: FixedBrokerEndpointReadbackV1 {
            schema_version: 1,
            pipe_name: name_value.into(),
            owner_sid: owner_sid.into(),
            service_sid_sha256: format!("{:x}", Sha256::digest(service_sid.as_bytes())),
            operator_sid_sha256: format!("{:x}", Sha256::digest(operator_sid.as_bytes())),
            security_descriptor_sha256: format!("{:x}", Sha256::digest(actual_sddl.as_bytes())),
            message_pipe: true,
            one_instance: true,
            // GetNamedPipeInfo does not expose this flag. It is the fixed
            // CreateNamedPipeW argument above, not independent readback.
            remote_clients_rejected_by_constructor: true,
            disposition: "ENDPOINT_SECURITY_READBACK_NOT_INSTALL_AUTHORITY",
        },
    })
}

/// Construct the compiled one-instance endpoint only from the fixed pinned
/// protected policy. It never accepts a caller-selected SID or pipe name.
pub fn create_fixed_broker_endpoint_from_protected_policy(
    policy: &BrokerProtectedPolicyV1,
) -> Result<FixedBrokerPipeEndpoint> {
    inspect_fixed_broker_read_only()?;
    let expected_service_sid = fixed_broker_service_sid_for_endpoint()?;
    let (service_sid, operator_sid) = policy.checked_endpoint_sids()?;
    if !service_sid.starts_with("S-1-5-80-") || service_sid != expected_service_sid {
        return Err("WAKE_BROKER_ENDPOINT_POLICY_INVALID".into());
    }
    require_user_account_sid(operator_sid)?;
    let endpoint = create_endpoint(EndpointName::Fixed, SYSTEM_SID, service_sid, operator_sid)?;
    policy.revalidate()?;
    Ok(endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcessId() -> u32;
        fn GetLastError() -> u32;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            disposition: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
        fn ConnectNamedPipe(pipe: *mut c_void, overlapped: *mut c_void) -> i32;
        fn ReadFile(
            file: *mut c_void,
            buffer: *mut c_void,
            length: u32,
            read: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn WriteFile(
            file: *mut c_void,
            buffer: *const c_void,
            length: u32,
            written: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
    }

    fn fixture_name() -> EndpointName {
        EndpointName::Fixture(format!(
            r"\\.\pipe\CatDeskWakeBrokerEndpointFixture-{}-{}",
            unsafe { GetCurrentProcessId() },
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn current_user_sid() -> String {
        // A fixture may run without elevation. The current account can own
        // its own ephemeral pipe; production always requires SYSTEM owner.
        use std::mem::size_of;
        #[repr(C)]
        struct SidAndAttributes {
            sid: *mut c_void,
            attributes: u32,
        }
        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
            fn GetTokenInformation(
                token: *mut c_void,
                class: u32,
                info: *mut c_void,
                length: u32,
                returned: *mut u32,
            ) -> i32;
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut c_void;
        }
        let mut raw = ptr::null_mut();
        assert_ne!(
            unsafe { OpenProcessToken(GetCurrentProcess(), 8, &mut raw) },
            0
        );
        let token = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut required = 0;
        unsafe { GetTokenInformation(token.as_raw_handle(), 1, ptr::null_mut(), 0, &mut required) };
        assert!((size_of::<SidAndAttributes>() as u32..=512).contains(&required));
        let mut aligned = vec![0usize; (required as usize).div_ceil(size_of::<usize>())];
        let mut returned = required;
        assert_ne!(
            unsafe {
                GetTokenInformation(
                    token.as_raw_handle(),
                    1,
                    aligned.as_mut_ptr().cast(),
                    required,
                    &mut returned,
                )
            },
            0
        );
        assert!(returned <= required);
        let entry = unsafe { ptr::read_unaligned(aligned.as_ptr().cast::<SidAndAttributes>()) };
        sid_text(entry.sid).expect("current token SID")
    }

    #[test]
    fn exact_isolated_pipe_owner_and_dacl_read_back() {
        let owner = current_user_sid();
        let service = owner.clone();
        let operator = "S-1-5-21-1-2-3-1001";
        let endpoint = create_endpoint(fixture_name(), &owner, &service, operator)
            .expect("isolated endpoint creation");
        assert_eq!(endpoint.readback().owner_sid, owner);
        assert!(endpoint.readback().message_pipe);
        assert!(endpoint.readback().one_instance);
        assert!(endpoint.readback().remote_clients_rejected_by_constructor);
        assert_eq!(endpoint.readback().security_descriptor_sha256.len(), 64);
        assert_eq!(
            endpoint.readback().disposition,
            "ENDPOINT_SECURITY_READBACK_NOT_INSTALL_AUTHORITY"
        );
    }

    #[test]
    fn wrong_operator_or_owner_sid_is_refused_by_readback() {
        let owner = current_user_sid();
        let service = owner.clone();
        let operator = "S-1-5-21-1-2-3-1001";
        let endpoint = create_endpoint(fixture_name(), &owner, &service, operator).unwrap();
        let wrong_operator = "S-1-5-21-1-2-3-1002";
        let wrong_sddl = expected_sddl(&owner, &service, wrong_operator).unwrap();
        assert_eq!(
            readback_security(&endpoint.handle, &wrong_sddl, &owner).unwrap_err(),
            "WAKE_BROKER_ENDPOINT_SECURITY_MISMATCH"
        );
        let exact_sddl = expected_sddl(&owner, &service, operator).unwrap();
        assert_eq!(
            readback_security(&endpoint.handle, &exact_sddl, SYSTEM_SID).unwrap_err(),
            "WAKE_BROKER_ENDPOINT_SECURITY_MISMATCH"
        );
    }

    #[test]
    fn operator_cannot_receive_pipe_create_instance_bit() {
        assert_eq!(OPERATOR_CLIENT_ACCESS & FILE_CREATE_PIPE_INSTANCE, 0);
        assert!(expected_sddl(SYSTEM_SID, "S-1-5-80-1-2-3-4-5", SYSTEM_SID).is_err());
        assert!(expected_sddl(SYSTEM_SID, "S-1-5-80-1-2-3-4-5", "S-1-5-80-1-2-3-4-5").is_err());
        assert!(validate_sid("S-1-5-021-1").is_err());
        assert!(require_user_account_sid(&current_user_sid()).is_ok());
        assert!(require_user_account_sid("S-1-5-32-545").is_err());
    }

    #[test]
    fn first_instance_refuses_existing_pipe_name() {
        let owner = current_user_sid();
        let service = owner.clone();
        let operator = "S-1-5-21-1-2-3-1001";
        let name = fixture_name();
        let value = name.value().to_string();
        let first = create_endpoint(name, &owner, &service, operator).unwrap();
        assert_eq!(
            create_endpoint(EndpointName::Fixture(value), &owner, &service, operator)
                .err()
                .expect("second first-instance must fail"),
            "WAKE_BROKER_ENDPOINT_CREATE_REFUSED"
        );
        drop(first);
    }

    #[test]
    fn operator_only_client_can_exchange_bytes_but_cannot_create_pipe_instance() {
        let owner = current_user_sid();
        let service = "S-1-5-80-1-2-3-4-5";
        let name = fixture_name();
        let sddl = expected_sddl(&owner, service, &owner).unwrap();
        let descriptor = descriptor_from_sddl(&sddl).unwrap();
        let mut security = SecurityAttributes {
            length: std::mem::size_of::<SecurityAttributes>() as u32,
            descriptor: descriptor.0,
            inherit_handle: 0,
        };
        // The test allows two instances so a second-server failure measures
        // the DACL's missing create-instance right, not the instance limit.
        let raw_server = unsafe {
            CreateNamedPipeW(
                wide(name.value()).as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS,
                2,
                4096,
                4096,
                0,
                (&mut security as *mut SecurityAttributes).cast(),
            )
        };
        assert!(!raw_server.is_null() && raw_server != INVALID_HANDLE_VALUE);
        let server = unsafe { OwnedHandle::from_raw_handle(raw_server) };
        readback_security(&server, &sddl, &owner).unwrap();

        let raw_client = unsafe {
            CreateFileW(
                wide(name.value()).as_ptr(),
                OPERATOR_CLIENT_ACCESS,
                0,
                ptr::null_mut(),
                3,
                0,
                ptr::null_mut(),
            )
        };
        assert!(
            !raw_client.is_null() && raw_client != INVALID_HANDLE_VALUE,
            "narrow client open error {}",
            unsafe { GetLastError() }
        );
        let client = unsafe { OwnedHandle::from_raw_handle(raw_client) };
        let connected = unsafe { ConnectNamedPipe(server.as_raw_handle(), ptr::null_mut()) };
        assert!(connected != 0 || unsafe { GetLastError() } == 535);
        let mut written = 0;
        assert_ne!(
            unsafe {
                WriteFile(
                    client.as_raw_handle(),
                    b"x".as_ptr().cast(),
                    1,
                    &mut written,
                    ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!(written, 1);
        let mut server_byte = 0u8;
        let mut read = 0;
        assert_ne!(
            unsafe {
                ReadFile(
                    server.as_raw_handle(),
                    (&mut server_byte as *mut u8).cast(),
                    1,
                    &mut read,
                    ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!((server_byte, read), (b'x', 1));
        assert_ne!(
            unsafe {
                WriteFile(
                    server.as_raw_handle(),
                    b"y".as_ptr().cast(),
                    1,
                    &mut written,
                    ptr::null_mut(),
                )
            },
            0
        );
        let mut client_byte = 0u8;
        assert_ne!(
            unsafe {
                ReadFile(
                    client.as_raw_handle(),
                    (&mut client_byte as *mut u8).cast(),
                    1,
                    &mut read,
                    ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!((client_byte, read), (b'y', 1));

        let competing = unsafe {
            CreateNamedPipeW(
                wide(name.value()).as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS,
                2,
                4096,
                4096,
                0,
                (&mut security as *mut SecurityAttributes).cast(),
            )
        };
        assert_eq!(competing, INVALID_HANDLE_VALUE);
        assert_eq!(unsafe { GetLastError() }, 5); // ERROR_ACCESS_DENIED
    }
}
