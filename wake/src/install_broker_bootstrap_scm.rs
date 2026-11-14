//! Create-only SCM portion of the explicitly approved first broker bootstrap.
//! No service is started, deleted, replaced or repaired here. A partial create
//! stays quarantined; the caller must retain its admitted bootstrap and pins.
#![cfg(windows)]

use crate::{
    Result,
    install_broker_preflight::{
        SERVICE_COMMAND_LINE, SERVICE_NAME, canonical_broker_service_security,
        fixed_broker_service_sid_for_endpoint,
    },
};
use std::{ffi::c_void, mem::size_of, ptr};

const SC_MANAGER_CONNECT: u32 = 1;
const SC_MANAGER_CREATE_SERVICE: u32 = 2;
const SERVICE_QUERY_CONFIG: u32 = 1;
const SERVICE_CHANGE_CONFIG: u32 = 2;
const SERVICE_QUERY_STATUS: u32 = 4;
const SERVICE_START: u32 = 0x10;
const READ_CONTROL: u32 = 0x0002_0000;
const WRITE_DAC: u32 = 0x0004_0000;
const WRITE_OWNER: u32 = 0x0008_0000;
const SERVICE_WIN32_OWN_PROCESS: u32 = 0x10;
const SERVICE_DEMAND_START: u32 = 3;
const SERVICE_ERROR_NORMAL: u32 = 1;
const SERVICE_CONFIG_SERVICE_SID_INFO: u32 = 5;
const SERVICE_SID_TYPE_RESTRICTED: u32 = 3;
const SERVICE_STOPPED: u32 = 1;
const ERROR_SERVICE_DOES_NOT_EXIST: u32 = 1060;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const SECURITY_INFORMATION: u32 = 7;
const PROTECTED_DACL_SECURITY_INFORMATION: u32 = 0x8000_0000;
const SE_SERVICE: u32 = 5;
const MAX_SECURITY: usize = 16 * 1024;
const MAX_CONFIG: usize = 64 * 1024;

#[repr(C)]
struct ServiceConfig {
    service_type: u32,
    start_type: u32,
    error_control: u32,
    binary_path: *mut u16,
    load_order_group: *mut u16,
    tag_id: u32,
    dependencies: *mut u16,
    account: *mut u16,
    display_name: *mut u16,
}

#[repr(C)]
#[derive(Default)]
struct ServiceStatus {
    service_type: u32,
    state: u32,
    controls_accepted: u32,
    exit_code: u32,
    specific_exit_code: u32,
    checkpoint: u32,
    wait_hint: u32,
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenSCManagerW(machine: *const u16, database: *const u16, access: u32) -> *mut c_void;
    fn OpenServiceW(manager: *mut c_void, name: *const u16, access: u32) -> *mut c_void;
    fn CreateServiceW(
        manager: *mut c_void,
        name: *const u16,
        display: *const u16,
        access: u32,
        service_type: u32,
        start_type: u32,
        error_control: u32,
        binary: *const u16,
        group: *const u16,
        tag: *mut u32,
        dependencies: *const u16,
        account: *const u16,
        password: *const u16,
    ) -> *mut c_void;
    fn ChangeServiceConfig2W(service: *mut c_void, level: u32, info: *mut c_void) -> i32;
    fn QueryServiceConfigW(
        service: *mut c_void,
        config: *mut ServiceConfig,
        size: u32,
        needed: *mut u32,
    ) -> i32;
    fn QueryServiceConfig2W(
        service: *mut c_void,
        level: u32,
        info: *mut u8,
        size: u32,
        needed: *mut u32,
    ) -> i32;
    fn QueryServiceStatus(service: *mut c_void, status: *mut ServiceStatus) -> i32;
    fn CloseServiceHandle(service: *mut c_void) -> i32;
    fn QueryServiceObjectSecurity(
        service: *mut c_void,
        requested: u32,
        descriptor: *mut c_void,
        length: u32,
        needed: *mut u32,
    ) -> i32;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        text: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        size: *mut u32,
    ) -> i32;
    fn ConvertSecurityDescriptorToStringSecurityDescriptorW(
        descriptor: *mut c_void,
        revision: u32,
        requested: u32,
        text: *mut *mut u16,
        length: *mut u32,
    ) -> i32;
    fn GetSecurityDescriptorOwner(
        descriptor: *mut c_void,
        owner: *mut *mut c_void,
        defaulted: *mut i32,
    ) -> i32;
    fn GetSecurityDescriptorGroup(
        descriptor: *mut c_void,
        group: *mut *mut c_void,
        defaulted: *mut i32,
    ) -> i32;
    fn GetSecurityDescriptorDacl(
        descriptor: *mut c_void,
        present: *mut i32,
        dacl: *mut *mut c_void,
        defaulted: *mut i32,
    ) -> i32;
    fn SetSecurityInfo(
        handle: *mut c_void,
        object_type: u32,
        information: u32,
        owner: *mut c_void,
        group: *mut c_void,
        dacl: *mut c_void,
        sacl: *mut c_void,
    ) -> u32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetLastError() -> u32;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
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

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn aligned(size: usize) -> Vec<usize> {
    vec![0; size.div_ceil(size_of::<usize>())]
}

fn service_absent(manager: &ServiceHandle) -> Result<()> {
    let service = ServiceHandle(unsafe {
        OpenServiceW(manager.0, wide(SERVICE_NAME).as_ptr(), SERVICE_QUERY_STATUS)
    });
    if !service.0.is_null() {
        return Err("WAKE_BOOTSTRAP_SERVICE_ALREADY_EXISTS".into());
    }
    if unsafe { GetLastError() } != ERROR_SERVICE_DOES_NOT_EXIST {
        return Err("WAKE_BOOTSTRAP_SERVICE_ABSENCE_UNPROVEN".into());
    }
    Ok(())
}

/// Keep the fixed SCM connection alive from absence check through create.
/// The admitted coordinator checks token, privilege, inputs and filesystem
/// absence before consuming this object; this type itself grants no approval.
pub(crate) struct BootstrapScmAbsent {
    manager: ServiceHandle,
}

impl BootstrapScmAbsent {
    pub(crate) fn observe() -> Result<Self> {
        let manager = ServiceHandle(unsafe {
            OpenSCManagerW(
                ptr::null(),
                ptr::null(),
                SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE,
            )
        });
        if manager.0.is_null() {
            return Err("WAKE_BOOTSTRAP_SCM_CREATE_ACCESS_REQUIRED".into());
        }
        service_absent(&manager)?;
        Ok(Self { manager })
    }

    pub(crate) fn revalidate_absent(&self) -> Result<()> {
        service_absent(&self.manager)
    }

    pub(crate) fn create_stopped(
        self,
        operator_sid: &str,
        allow_operator_service_start: bool,
    ) -> Result<BootstrapScmCreated> {
        // Validate both exact final security documents before the first write.
        // A query-only one-shot service would have no supported later owner.
        if !allow_operator_service_start {
            return Err("WAKE_BOOTSTRAP_START_OWNER_REQUIRED".into());
        }
        let initial_security = canonical_broker_service_security(None)?;
        let final_security = canonical_broker_service_security(Some(operator_sid))?;
        self.revalidate_absent()?;
        let service = ServiceHandle(unsafe {
            CreateServiceW(
                self.manager.0,
                wide(SERVICE_NAME).as_ptr(),
                wide(SERVICE_NAME).as_ptr(),
                SERVICE_QUERY_CONFIG
                    | SERVICE_CHANGE_CONFIG
                    | SERVICE_QUERY_STATUS
                    | SERVICE_START
                    | READ_CONTROL
                    | WRITE_DAC
                    | WRITE_OWNER,
                SERVICE_WIN32_OWN_PROCESS,
                SERVICE_DEMAND_START,
                SERVICE_ERROR_NORMAL,
                wide(SERVICE_COMMAND_LINE).as_ptr(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null(),
                wide("LocalSystem").as_ptr(),
                ptr::null(),
            )
        });
        if service.0.is_null() {
            return Err("WAKE_BOOTSTRAP_SERVICE_CREATE_FAILED".into());
        }
        // Never delete on an uncertain later error. Seal start permission
        // immediately; retained rights remain valid through final readback.
        set_security(&service, &initial_security)?;
        let mut sid_type = SERVICE_SID_TYPE_RESTRICTED;
        if unsafe {
            ChangeServiceConfig2W(
                service.0,
                SERVICE_CONFIG_SERVICE_SID_INFO,
                (&mut sid_type as *mut u32).cast(),
            )
        } == 0
        {
            return Err("WAKE_BOOTSTRAP_SCM_PARTIAL_QUARANTINED".into());
        }
        let service_sid = fixed_broker_service_sid_for_endpoint()?;
        let result = BootstrapScmCreated {
            _manager: self.manager,
            service,
            service_sid,
            initial_security,
            final_security,
            finalized: false,
        };
        result.revalidate_stopped()?;
        Ok(result)
    }
}

pub(crate) struct BootstrapScmCreated {
    _manager: ServiceHandle,
    service: ServiceHandle,
    service_sid: String,
    initial_security: String,
    final_security: String,
    finalized: bool,
}

impl BootstrapScmCreated {
    pub(crate) fn service_sid(&self) -> &str {
        &self.service_sid
    }

    pub(crate) fn revalidate_stopped(&self) -> Result<()> {
        stopped_config(&self.service)?;
        if fixed_broker_service_sid_for_endpoint()? != self.service_sid
            || service_security(&self.service)?
                != if self.finalized {
                    &self.final_security
                } else {
                    &self.initial_security
                }
                .as_str()
        {
            return Err("WAKE_BOOTSTRAP_SCM_CHANGED_QUARANTINED".into());
        }
        Ok(())
    }

    pub(crate) fn finalize_security(&mut self) -> Result<()> {
        self.revalidate_stopped()?;
        set_security(&self.service, &self.final_security)?;
        self.finalized = true;
        self.revalidate_stopped()
    }
}

fn set_security(service: &ServiceHandle, expected: &str) -> Result<()> {
    let mut descriptor = ptr::null_mut();
    let mut length = 0;
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(expected).as_ptr(),
            1,
            &mut descriptor,
            &mut length,
        )
    };
    let allocation = LocalAllocation(descriptor);
    if converted == 0 || allocation.0.is_null() {
        return Err("WAKE_BOOTSTRAP_SCM_SECURITY_ENCODING_INVALID".into());
    }
    if length == 0 || length as usize > MAX_SECURITY {
        return Err("WAKE_BOOTSTRAP_SCM_SECURITY_ENCODING_INVALID".into());
    }
    let mut owner = ptr::null_mut();
    let mut group = ptr::null_mut();
    let mut dacl = ptr::null_mut();
    let mut defaulted = 0;
    let mut present = 0;
    if unsafe { GetSecurityDescriptorOwner(allocation.0, &mut owner, &mut defaulted) } == 0
        || unsafe { GetSecurityDescriptorGroup(allocation.0, &mut group, &mut defaulted) } == 0
        || unsafe {
            GetSecurityDescriptorDacl(allocation.0, &mut present, &mut dacl, &mut defaulted)
        } == 0
        || owner.is_null()
        || group.is_null()
        || present == 0
        || dacl.is_null()
    {
        return Err("WAKE_BOOTSTRAP_SCM_SECURITY_ENCODING_INVALID".into());
    }
    if unsafe {
        SetSecurityInfo(
            service.0,
            SE_SERVICE,
            SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            owner,
            group,
            dacl,
            ptr::null_mut(),
        )
    } != 0
        || service_security(service)? != expected
    {
        return Err("WAKE_BOOTSTRAP_SCM_SECURITY_QUARANTINED".into());
    }
    Ok(())
}

fn service_security(service: &ServiceHandle) -> Result<String> {
    let mut needed = 0;
    if unsafe {
        QueryServiceObjectSecurity(
            service.0,
            SECURITY_INFORMATION,
            ptr::null_mut(),
            0,
            &mut needed,
        )
    } != 0
        || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER
        || needed == 0
        || needed as usize > MAX_SECURITY
    {
        return Err("WAKE_BOOTSTRAP_SCM_SECURITY_UNAVAILABLE".into());
    }
    let capacity = needed;
    let mut bytes = aligned(capacity as usize);
    if unsafe {
        QueryServiceObjectSecurity(
            service.0,
            SECURITY_INFORMATION,
            bytes.as_mut_ptr().cast(),
            capacity,
            &mut needed,
        )
    } == 0
        || needed == 0
        || needed > capacity
    {
        return Err("WAKE_BOOTSTRAP_SCM_SECURITY_UNAVAILABLE".into());
    }
    descriptor_text(bytes.as_mut_ptr().cast())
}

fn descriptor_text(descriptor: *mut c_void) -> Result<String> {
    let mut text = ptr::null_mut();
    let mut chars = 0;
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            1,
            SECURITY_INFORMATION,
            &mut text,
            &mut chars,
        )
    };
    let allocation = LocalAllocation(text.cast());
    if converted == 0 || allocation.0.is_null() {
        return Err("WAKE_BOOTSTRAP_SCM_SECURITY_UNAVAILABLE".into());
    }
    if chars == 0 || chars as usize > MAX_SECURITY / 2 {
        return Err("WAKE_BOOTSTRAP_SCM_SECURITY_UNAVAILABLE".into());
    }
    let raw = unsafe { std::slice::from_raw_parts(allocation.0.cast::<u16>(), chars as usize) };
    // Match the existing canonical descriptor helper's API-reported length;
    // never guess whether a particular Windows conversion includes the NUL.
    String::from_utf16(raw).map_err(|_| "WAKE_BOOTSTRAP_SCM_SECURITY_UNAVAILABLE".into())
}

fn bounded_config_text(
    pointer: *const u16,
    buffer: &[usize],
    byte_length: usize,
) -> Result<String> {
    if byte_length > std::mem::size_of_val(buffer) {
        return Err("WAKE_BOOTSTRAP_SCM_CONFIG_INVALID".into());
    }
    let start = buffer.as_ptr() as usize;
    let address = pointer as usize;
    let end = start
        .checked_add(byte_length)
        .ok_or("WAKE_BOOTSTRAP_SCM_CONFIG_INVALID")?;
    if address < start || address >= end || !address.is_multiple_of(2) {
        return Err("WAKE_BOOTSTRAP_SCM_CONFIG_INVALID".into());
    }
    let raw = unsafe { std::slice::from_raw_parts(pointer, (end - address) / 2) };
    let length = raw
        .iter()
        .position(|value| *value == 0)
        .ok_or("WAKE_BOOTSTRAP_SCM_CONFIG_INVALID")?;
    String::from_utf16(&raw[..length]).map_err(|_| "WAKE_BOOTSTRAP_SCM_CONFIG_INVALID".into())
}

fn stopped_config(service: &ServiceHandle) -> Result<()> {
    let mut needed = 0;
    if unsafe { QueryServiceConfigW(service.0, ptr::null_mut(), 0, &mut needed) } != 0
        || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER
        || (needed as usize) < size_of::<ServiceConfig>()
        || needed as usize > MAX_CONFIG
    {
        return Err("WAKE_BOOTSTRAP_SCM_CONFIG_INVALID".into());
    }
    let capacity = needed;
    let mut bytes = aligned(capacity as usize);
    if unsafe { QueryServiceConfigW(service.0, bytes.as_mut_ptr().cast(), capacity, &mut needed) }
        == 0
        || needed > capacity
        || (needed as usize) < size_of::<ServiceConfig>()
    {
        return Err("WAKE_BOOTSTRAP_SCM_CONFIG_INVALID".into());
    }
    let config = unsafe { &*bytes.as_ptr().cast::<ServiceConfig>() };
    let image = bounded_config_text(config.binary_path, &bytes, needed as usize)?;
    let account = bounded_config_text(config.account, &bytes, needed as usize)?;
    let mut sid_type = 0u32;
    let mut status = ServiceStatus::default();
    if config.service_type != SERVICE_WIN32_OWN_PROCESS
        || config.start_type != SERVICE_DEMAND_START
        || config.error_control != SERVICE_ERROR_NORMAL
        || image != SERVICE_COMMAND_LINE
        || !account.eq_ignore_ascii_case("LocalSystem")
        || unsafe {
            QueryServiceConfig2W(
                service.0,
                SERVICE_CONFIG_SERVICE_SID_INFO,
                (&mut sid_type as *mut u32).cast(),
                size_of::<u32>() as u32,
                &mut needed,
            )
        } == 0
        || sid_type != SERVICE_SID_TYPE_RESTRICTED
        || unsafe { QueryServiceStatus(service.0, &mut status) } == 0
        || status.service_type != SERVICE_WIN32_OWN_PROCESS
        || status.state != SERVICE_STOPPED
    {
        return Err("WAKE_BOOTSTRAP_SCM_CHANGED_QUARANTINED".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_scm_configuration_strings_are_bounded_to_returned_buffer() {
        let mut buffer = aligned(64);
        let bytes = buffer.as_mut_ptr().cast::<u16>();
        for (offset, value) in wide("LocalSystem").iter().enumerate() {
            unsafe { *bytes.add(offset) = *value };
        }
        assert_eq!(
            bounded_config_text(bytes, &buffer, 64).unwrap(),
            "LocalSystem"
        );
        assert!(bounded_config_text(ptr::null(), &buffer, 64).is_err());
        assert!(bounded_config_text((bytes as usize + 1) as *const u16, &buffer, 64).is_err());
        assert!(bounded_config_text(unsafe { bytes.add(32) }, &buffer, 64).is_err());
        assert!(bounded_config_text(bytes, &buffer, 2).is_err());
        assert!(bounded_config_text(bytes, &buffer, 65).is_err());
        assert!(bounded_config_text(unsafe { bytes.add(16) }, &buffer, 24).is_err());
    }

    #[test]
    fn bootstrap_scm_native_descriptor_roundtrip_matches_existing_canonical_policy() {
        let expected = canonical_broker_service_security(None).unwrap();
        let mut descriptor = ptr::null_mut();
        let mut size = 0;
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide(&expected).as_ptr(),
                    1,
                    &mut descriptor,
                    &mut size,
                )
            },
            0
        );
        let allocation = LocalAllocation(descriptor);
        assert!(!allocation.0.is_null());
        assert_eq!(descriptor_text(allocation.0).unwrap(), expected);
    }
}
