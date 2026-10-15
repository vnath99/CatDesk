//! Read-only inspection of the separately provisioned Wake install broker.
//!
//! The paths and service identity are compiled here, never selected by an
//! installer, candidate package, environment variable, or CLI argument. This
//! is a preflight *snapshot*, not installation authority: it does not verify
//! Authenticode, the bootstrap audit, worker-token denials, integrity labels,
//! protected approval-key policy, or a replay ledger. WakeHost's production
//! mutation hold remains independent of any result returned here.

use crate::{Result, install_broker_approval::ObjectIdentityV1};
use serde::Serialize;

pub const SERVICE_NAME: &str = "CatDeskWakeInstallBroker";
pub const SERVICE_IMAGE: &str =
    r"C:\Program Files\CatDesk\WakeInstallBrokerV1\CatDeskWakeInstallBroker.exe";
pub const SERVICE_COMMAND_LINE: &str =
    r#""C:\Program Files\CatDesk\WakeInstallBrokerV1\CatDeskWakeInstallBroker.exe" --service"#;
pub const PROTECTED_ROOT: &str = r"C:\ProgramData\CatDesk\WakeProtectedInstallV1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerReadOnlySnapshotV1 {
    pub schema_version: u32,
    pub service_name: &'static str,
    pub service_image_sha256: String,
    pub service_image: ObjectIdentityV1,
    pub service_image_security_sha256: String,
    pub protected_root: ObjectIdentityV1,
    pub service_security_sha256: String,
    pub protected_root_security_sha256: String,
    pub image_parent_security_sha256: String,
    pub disposition: &'static str,
}

#[cfg(not(windows))]
pub fn inspect_fixed_broker_read_only() -> Result<BrokerReadOnlySnapshotV1> {
    Err("WAKE_BROKER_WINDOWS_REQUIRED".into())
}

#[cfg(windows)]
mod windows {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{
        ffi::{OsStr, OsString, c_void},
        fs::File,
        io::Read,
        os::windows::{
            ffi::{OsStrExt, OsStringExt},
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        ptr,
    };

    const SC_MANAGER_CONNECT: u32 = 1;
    const SERVICE_QUERY_CONFIG: u32 = 1;
    const READ_CONTROL: u32 = 0x0002_0000;
    const SERVICE_WIN32_OWN_PROCESS: u32 = 0x10;
    const SERVICE_DEMAND_START: u32 = 3;
    const SERVICE_CONFIG_SERVICE_SID_INFO: u32 = 5;
    const SERVICE_SID_TYPE_RESTRICTED: u32 = 3;
    const OWNER_SECURITY_INFORMATION: u32 = 1;
    const GROUP_SECURITY_INFORMATION: u32 = 2;
    const DACL_SECURITY_INFORMATION: u32 = 4;
    const SECURITY_INFORMATION: u32 =
        OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    const SE_FILE_OBJECT: u32 = 1;
    const OPEN_EXISTING: u32 = 3;
    const FILE_OPEN: u32 = 1;
    const FILE_READ_DATA: u32 = 1;
    const FILE_READ_ATTRIBUTES: u32 = 0x80;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_DIRECTORY_FILE: u32 = 1;
    const FILE_NON_DIRECTORY_FILE: u32 = 0x40;
    const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x20;
    const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    const MAX_SERVICE_DESCRIPTOR: usize = 16 * 1024;
    const MAX_SERVICE_CONFIG: usize = 64 * 1024;
    const MAX_SERVICE_IMAGE: u64 = 80 * 1024 * 1024;
    const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

    #[repr(C)]
    struct ServiceConfig {
        service_type: u32,
        start_type: u32,
        error_control: u32,
        binary_path: *mut u16,
        load_order_group: *mut u16,
        tag_id: u32,
        dependencies: *mut u16,
        service_start_name: *mut u16,
        display_name: *mut u16,
    }

    #[repr(C)]
    struct ServiceSidInfo {
        service_sid_type: u32,
    }

    #[repr(C)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *mut u16,
    }

    #[repr(C)]
    struct ObjectAttributes {
        length: u32,
        root_directory: *mut c_void,
        object_name: *mut UnicodeString,
        attributes: u32,
        security_descriptor: *mut c_void,
        security_quality_of_service: *mut c_void,
    }

    #[repr(C)]
    struct IoStatusBlock {
        status: isize,
        information: usize,
    }

    #[repr(C)]
    #[derive(Default)]
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

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenSCManagerW(machine: *const u16, database: *const u16, access: u32) -> *mut c_void;
        fn OpenServiceW(manager: *mut c_void, name: *const u16, access: u32) -> *mut c_void;
        fn QueryServiceConfigW(
            service: *mut c_void,
            config: *mut ServiceConfig,
            size: u32,
            needed: *mut u32,
        ) -> i32;
        fn QueryServiceConfig2W(
            service: *mut c_void,
            level: u32,
            buffer: *mut u8,
            size: u32,
            needed: *mut u32,
        ) -> i32;
        fn QueryServiceObjectSecurity(
            service: *mut c_void,
            requested: u32,
            descriptor: *mut c_void,
            size: u32,
            needed: *mut u32,
        ) -> i32;
        fn CloseServiceHandle(handle: *mut c_void) -> i32;
        fn LookupAccountNameW(
            system: *const u16,
            name: *const u16,
            sid: *mut c_void,
            sid_size: *mut u32,
            domain: *mut u16,
            domain_size: *mut u32,
            use_type: *mut u32,
        ) -> i32;
        fn ConvertSidToStringSidW(sid: *mut c_void, result: *mut *mut u16) -> i32;
        fn ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor: *mut c_void,
            revision: u32,
            requested: u32,
            result: *mut *mut u16,
            length: *mut u32,
        ) -> i32;
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
            source: *const u16,
            revision: u32,
            descriptor: *mut *mut c_void,
            size: *mut u32,
        ) -> i32;
        fn GetSecurityInfo(
            handle: *mut c_void,
            object_type: u32,
            requested: u32,
            owner: *mut *mut c_void,
            group: *mut *mut c_void,
            dacl: *mut *mut c_void,
            sacl: *mut *mut c_void,
            descriptor: *mut *mut c_void,
        ) -> u32;
        fn GetSecurityDescriptorOwner(
            descriptor: *mut c_void,
            owner: *mut *mut c_void,
            defaulted: *mut i32,
        ) -> i32;
        fn GetSecurityDescriptorDacl(
            descriptor: *mut c_void,
            present: *mut i32,
            dacl: *mut *mut c_void,
            defaulted: *mut i32,
        ) -> i32;
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn lstrlenW(value: *const u16) -> i32;
        fn CreateFileW(
            path: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            disposition: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
        fn GetFileInformationByHandle(handle: *mut c_void, info: *mut FileInfo) -> i32;
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtCreateFile(
            handle: *mut *mut c_void,
            access: u32,
            attributes: *mut ObjectAttributes,
            status: *mut IoStatusBlock,
            allocation: *mut c_void,
            file_attributes: u32,
            share: u32,
            disposition: u32,
            options: u32,
            ea: *mut c_void,
            ea_size: u32,
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

    #[derive(Clone, Debug)]
    struct ServiceReadback {
        service_type: u32,
        start_type: u32,
        binary_path: String,
        account: String,
        sid_type: u32,
        security_sddl: String,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct SecurityReadback {
        owner: String,
        sddl: String,
    }

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn aligned_buffer(bytes: usize) -> Vec<usize> {
        vec![0; bytes.div_ceil(std::mem::size_of::<usize>())]
    }

    fn buffer_bytes(words: &[usize]) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), std::mem::size_of_val(words))
        }
    }

    fn bounded_wide(buffer: &[u8], pointer: *const u16) -> Result<String> {
        let base = buffer.as_ptr() as usize;
        let end = base + buffer.len();
        let start = pointer as usize;
        if pointer.is_null() || start < base || start >= end || !start.is_multiple_of(2) {
            return Err("WAKE_BROKER_SCM_CONFIG_INVALID".into());
        }
        let maximum = ((end - start) / 2).min(4096);
        let characters = unsafe { std::slice::from_raw_parts(pointer, maximum) };
        let Some(length) = characters.iter().position(|unit| *unit == 0) else {
            return Err("WAKE_BROKER_SCM_CONFIG_INVALID".into());
        };
        OsString::from_wide(&characters[..length])
            .into_string()
            .map_err(|_| "WAKE_BROKER_SCM_CONFIG_INVALID".into())
    }

    fn allocated_wide_with_length(pointer: *mut u16, length: usize) -> Result<String> {
        if pointer.is_null() || length == 0 || length > MAX_SERVICE_DESCRIPTOR {
            return Err("WAKE_BROKER_SECURITY_INVALID".into());
        }
        // This length is supplied by ConvertSecurityDescriptorToString...;
        // never manufacture a slice reaching beyond the returned allocation.
        let units = unsafe { std::slice::from_raw_parts(pointer, length) };
        String::from_utf16(units).map_err(|_| "WAKE_BROKER_SECURITY_INVALID".into())
    }

    fn sid_string(sid: *mut c_void) -> Result<String> {
        let mut text = ptr::null_mut();
        if sid.is_null() || unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
            return Err("WAKE_BROKER_SECURITY_INVALID".into());
        }
        // ConvertSidToStringSidW guarantees a NUL-terminated allocation.
        // Let Windows determine its actual length before forming a Rust slice.
        let length = unsafe { lstrlenW(text) };
        let result = if length <= 0 || length > 256 {
            Err("WAKE_BROKER_SECURITY_INVALID".into())
        } else {
            allocated_wide_with_length(text, length as usize)
        };
        unsafe { LocalFree(text.cast()) };
        result
    }

    fn descriptor_readback(descriptor: *mut c_void) -> Result<SecurityReadback> {
        let mut owner = ptr::null_mut();
        let mut defaulted = 0;
        let mut present = 0;
        let mut dacl = ptr::null_mut();
        if descriptor.is_null()
            || unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted) } == 0
            || owner.is_null()
            || unsafe {
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted)
            } == 0
            || present == 0
            || dacl.is_null()
        {
            return Err("WAKE_BROKER_SECURITY_INVALID".into());
        }
        let owner = sid_string(owner)?;
        let mut text = ptr::null_mut();
        let mut length = 0;
        if unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                1,
                SECURITY_INFORMATION,
                &mut text,
                &mut length,
            )
        } == 0
            || text.is_null()
            || length == 0
            || length as usize > MAX_SERVICE_DESCRIPTOR
        {
            if !text.is_null() {
                unsafe { LocalFree(text.cast()) };
            }
            return Err("WAKE_BROKER_SECURITY_INVALID".into());
        }
        let result = allocated_wide_with_length(text, length as usize);
        unsafe { LocalFree(text.cast()) };
        Ok(SecurityReadback {
            owner,
            sddl: result?,
        })
    }

    fn canonical_sddl(expected: &str) -> Result<String> {
        let mut descriptor = ptr::null_mut();
        let mut size = 0;
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(expected).as_ptr(),
                1,
                &mut descriptor,
                &mut size,
            )
        } == 0
            || descriptor.is_null()
            || size == 0
            || size as usize > MAX_SERVICE_DESCRIPTOR
        {
            return Err("WAKE_BROKER_POLICY_INVALID".into());
        }
        let result = descriptor_readback(descriptor).map(|value| value.sddl);
        unsafe { LocalFree(descriptor) };
        result
    }

    fn expected_service_sddl() -> &'static str {
        "O:SYG:SYD:P(A;;0x000F01FF;;;SY)(A;;0x00020005;;;BA)(A;;0x00020004;;;IU)"
    }

    fn expected_namespace_sddl(service_sid: &str) -> String {
        format!("O:SYG:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;{service_sid})(A;OICI;0x001200A9;;;IU)")
    }

    fn expected_image_parent_sddl(service_sid: &str) -> String {
        format!("O:SYG:SYD:P(A;OICI;FA;;;SY)(A;OICI;0x001200A9;;;{service_sid})")
    }

    fn expected_image_file_sddl(service_sid: &str) -> String {
        format!("O:SYG:SYD:P(A;;FA;;;SY)(A;;0x001200A9;;;{service_sid})")
    }

    fn assert_service(readback: &ServiceReadback) -> Result<()> {
        if readback.service_type != SERVICE_WIN32_OWN_PROCESS
            || readback.start_type != SERVICE_DEMAND_START
            || readback.binary_path != SERVICE_COMMAND_LINE
            || !readback.account.eq_ignore_ascii_case("LocalSystem")
            || readback.sid_type != SERVICE_SID_TYPE_RESTRICTED
        {
            return Err("WAKE_BROKER_SCM_CONFIG_MISMATCH".into());
        }
        if readback.security_sddl != canonical_sddl(expected_service_sddl())? {
            return Err("WAKE_BROKER_SCM_SECURITY_MISMATCH".into());
        }
        Ok(())
    }

    fn lookup_account_sid(account: &str) -> Result<String> {
        let name = wide(account);
        let mut sid_size = 0;
        let mut domain_size = 0;
        let mut use_type = 0;
        unsafe {
            LookupAccountNameW(
                ptr::null(),
                name.as_ptr(),
                ptr::null_mut(),
                &mut sid_size,
                ptr::null_mut(),
                &mut domain_size,
                &mut use_type,
            )
        };
        if sid_size == 0 || sid_size > 256 || domain_size > 256 {
            return Err("WAKE_BROKER_SERVICE_SID_UNAVAILABLE".into());
        }
        let allocated_sid = sid_size;
        let mut sid = aligned_buffer(allocated_sid as usize);
        let mut domain = vec![0u16; domain_size as usize + 1];
        if unsafe {
            LookupAccountNameW(
                ptr::null(),
                name.as_ptr(),
                sid.as_mut_ptr().cast(),
                &mut sid_size,
                domain.as_mut_ptr(),
                &mut domain_size,
                &mut use_type,
            )
        } == 0
            || sid_size == 0
            || sid_size > allocated_sid
        {
            return Err("WAKE_BROKER_SERVICE_SID_UNAVAILABLE".into());
        }
        sid_string(sid.as_mut_ptr().cast())
    }

    fn lookup_service_sid() -> Result<String> {
        let value = lookup_account_sid(&format!(r"NT SERVICE\{SERVICE_NAME}"))?;
        if !value.starts_with("S-1-5-80-") {
            return Err("WAKE_BROKER_SERVICE_SID_INVALID".into());
        }
        Ok(value)
    }

    fn service_readback() -> Result<ServiceReadback> {
        let manager =
            ServiceHandle(unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT) });
        if manager.0.is_null() {
            return Err("WAKE_BROKER_SCM_UNAVAILABLE".into());
        }
        let name = wide(SERVICE_NAME);
        let service = ServiceHandle(unsafe {
            OpenServiceW(
                manager.0,
                name.as_ptr(),
                SERVICE_QUERY_CONFIG | READ_CONTROL,
            )
        });
        if service.0.is_null() {
            return Err("WAKE_BROKER_SERVICE_UNPROVISIONED".into());
        }
        let mut needed = 0;
        unsafe { QueryServiceConfigW(service.0, ptr::null_mut(), 0, &mut needed) };
        if needed as usize > MAX_SERVICE_CONFIG
            || (needed as usize) < std::mem::size_of::<ServiceConfig>()
        {
            return Err("WAKE_BROKER_SCM_CONFIG_INVALID".into());
        }
        let mut config_words = aligned_buffer(needed as usize);
        let allocated = needed;
        if unsafe {
            QueryServiceConfigW(
                service.0,
                config_words.as_mut_ptr().cast(),
                allocated,
                &mut needed,
            )
        } == 0
            || needed > allocated
        {
            return Err("WAKE_BROKER_SCM_CONFIG_INVALID".into());
        }
        let config = unsafe { ptr::read(config_words.as_ptr().cast::<ServiceConfig>()) };
        let config_bytes = &buffer_bytes(&config_words)[..needed as usize];
        let binary_path = bounded_wide(config_bytes, config.binary_path)?;
        let account = bounded_wide(config_bytes, config.service_start_name)?;
        let mut sid_info = ServiceSidInfo {
            service_sid_type: 0,
        };
        let mut sid_needed = 0;
        if unsafe {
            QueryServiceConfig2W(
                service.0,
                SERVICE_CONFIG_SERVICE_SID_INFO,
                (&mut sid_info as *mut ServiceSidInfo).cast(),
                std::mem::size_of::<ServiceSidInfo>() as u32,
                &mut sid_needed,
            )
        } == 0
        {
            return Err("WAKE_BROKER_SCM_CONFIG_INVALID".into());
        }
        let mut security_needed = 0;
        unsafe {
            QueryServiceObjectSecurity(
                service.0,
                SECURITY_INFORMATION,
                ptr::null_mut(),
                0,
                &mut security_needed,
            )
        };
        if security_needed == 0 || security_needed as usize > MAX_SERVICE_DESCRIPTOR {
            return Err("WAKE_BROKER_SCM_SECURITY_INVALID".into());
        }
        let mut security = aligned_buffer(security_needed as usize);
        let allocated = security_needed;
        if unsafe {
            QueryServiceObjectSecurity(
                service.0,
                SECURITY_INFORMATION,
                security.as_mut_ptr().cast(),
                allocated,
                &mut security_needed,
            )
        } == 0
            || security_needed > allocated
        {
            return Err("WAKE_BROKER_SCM_SECURITY_INVALID".into());
        }
        let security_sddl = descriptor_readback(security.as_mut_ptr().cast())?.sddl;
        Ok(ServiceReadback {
            service_type: config.service_type,
            start_type: config.start_type,
            binary_path,
            account,
            sid_type: sid_info.service_sid_type,
            security_sddl,
        })
    }

    fn object_identity(info: &FileInfo) -> Result<ObjectIdentityV1> {
        if info.volume == 0 || (info.index_high == 0 && info.index_low == 0) {
            return Err("WAKE_BROKER_OBJECT_ID_INVALID".into());
        }
        Ok(ObjectIdentityV1 {
            volume_serial: info.volume,
            file_index_high: info.index_high,
            file_index_low: info.index_low,
        })
    }

    fn file_info_raw(handle: *mut c_void, directory: bool) -> Result<FileInfo> {
        let mut info = FileInfo::default();
        if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0
            || info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || (info.attributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
            || (!directory && info.links != 1)
        {
            return Err("WAKE_BROKER_OBJECT_INVALID".into());
        }
        object_identity(&info)?;
        Ok(info)
    }

    fn file_info(handle: &OwnedHandle, directory: bool) -> Result<FileInfo> {
        file_info_raw(handle.as_raw_handle(), directory)
    }

    fn read_security_raw(handle: *mut c_void) -> Result<SecurityReadback> {
        let mut descriptor = ptr::null_mut();
        if unsafe {
            GetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        } != 0
            || descriptor.is_null()
        {
            return Err("WAKE_BROKER_SECURITY_INVALID".into());
        }
        let result = descriptor_readback(descriptor);
        unsafe { LocalFree(descriptor) };
        result
    }

    fn read_security(handle: &OwnedHandle) -> Result<SecurityReadback> {
        read_security_raw(handle.as_raw_handle())
    }

    fn assert_ancestor(handle: &OwnedHandle) -> Result<()> {
        let _ = file_info(handle, true)?;
        let security = read_security(handle)?;
        // A system volume can be owned by TrustedInstaller. Resolve that
        // exact SID instead of accepting arbitrary service owners.
        if !matches!(security.owner.as_str(), "S-1-5-18" | "S-1-5-32-544") {
            let trusted_installer = lookup_account_sid(r"NT SERVICE\TrustedInstaller")?;
            if security.owner != trusted_installer {
                return Err("WAKE_BROKER_ANCESTOR_OWNER_MISMATCH".into());
            }
        }
        // Presence of a non-null DACL is checked by descriptor_readback.
        // Broader ancestor effective-access analysis remains a bootstrap gate.
        Ok(())
    }

    fn assert_leaf_policy(security: &SecurityReadback, expected_sddl: &str) -> Result<()> {
        if security.owner != "S-1-5-18" || security.sddl != canonical_sddl(expected_sddl)? {
            return Err("WAKE_BROKER_PROTECTED_ACL_MISMATCH".into());
        }
        Ok(())
    }

    fn assert_exact_leaf(handle: &OwnedHandle, expected_sddl: &str) -> Result<String> {
        let _ = file_info(handle, true)?;
        let security = read_security(handle)?;
        assert_leaf_policy(&security, expected_sddl)?;
        Ok(format!("{:x}", Sha256::digest(security.sddl.as_bytes())))
    }

    fn assert_image_file_policy(security: &SecurityReadback, expected_sddl: &str) -> Result<()> {
        if security.owner != "S-1-5-18" || security.sddl != canonical_sddl(expected_sddl)? {
            return Err("WAKE_BROKER_IMAGE_ACL_MISMATCH".into());
        }
        Ok(())
    }

    fn open_root() -> Result<OwnedHandle> {
        let root = wide(r"C:\");
        let handle = unsafe {
            CreateFileW(
                root.as_ptr(),
                FILE_READ_ATTRIBUTES | READ_CONTROL,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err("WAKE_BROKER_ROOT_UNAVAILABLE".into());
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        assert_ancestor(&handle)?;
        Ok(handle)
    }

    fn validate_component(component: &str) -> Result<()> {
        if component.is_empty()
            || matches!(component, "." | "..")
            || component.ends_with(['.', ' '])
            || component.len() > 128
            || !component
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'.' | b'-'))
        {
            return Err("WAKE_BROKER_COMPONENT_INVALID".into());
        }
        Ok(())
    }

    fn open_relative(
        parent: &OwnedHandle,
        component: &str,
        directory: bool,
    ) -> Result<OwnedHandle> {
        validate_component(component)?;
        let mut units: Vec<u16> = component.encode_utf16().collect();
        let mut name = UnicodeString {
            length: (units.len() * 2) as u16,
            maximum_length: (units.len() * 2) as u16,
            buffer: units.as_mut_ptr(),
        };
        let mut attributes = ObjectAttributes {
            length: std::mem::size_of::<ObjectAttributes>() as u32,
            root_directory: parent.as_raw_handle(),
            object_name: &mut name,
            attributes: 0x40,
            security_descriptor: ptr::null_mut(),
            security_quality_of_service: ptr::null_mut(),
        };
        let mut status = IoStatusBlock {
            status: 0,
            information: 0,
        };
        let mut handle = ptr::null_mut();
        let result = unsafe {
            NtCreateFile(
                &mut handle,
                FILE_READ_ATTRIBUTES
                    | READ_CONTROL
                    | SYNCHRONIZE
                    | if directory { 0 } else { FILE_READ_DATA },
                &mut attributes,
                &mut status,
                ptr::null_mut(),
                0,
                FILE_SHARE_READ | if directory { FILE_SHARE_WRITE } else { 0 },
                FILE_OPEN,
                FILE_SYNCHRONOUS_IO_NONALERT
                    | FILE_OPEN_REPARSE_POINT
                    | if directory {
                        FILE_DIRECTORY_FILE
                    } else {
                        FILE_NON_DIRECTORY_FILE
                    },
                ptr::null_mut(),
                0,
            )
        };
        if result < 0 || handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err("WAKE_BROKER_OBJECT_UNAVAILABLE".into());
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        file_info(&handle, directory)?;
        Ok(handle)
    }

    fn image_digest(handle: OwnedHandle, service_sid: &str) -> Result<(String, String)> {
        let before = file_info(&handle, false)?;
        let before_security = read_security(&handle)?;
        assert_image_file_policy(&before_security, &expected_image_file_sddl(service_sid))?;
        let length = (u64::from(before.size_high) << 32) | u64::from(before.size_low);
        if length == 0 || length > MAX_SERVICE_IMAGE {
            return Err("WAKE_BROKER_IMAGE_INVALID".into());
        }
        let mut file = File::from(handle);
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut read = 0u64;
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|_| "WAKE_BROKER_IMAGE_UNAVAILABLE")?;
            if count == 0 {
                break;
            }
            read += count as u64;
            if read > length {
                return Err("WAKE_BROKER_IMAGE_CHANGED".into());
            }
            hasher.update(&buffer[..count]);
        }
        if read != length {
            return Err("WAKE_BROKER_IMAGE_CHANGED".into());
        }
        let after = file_info_raw(file.as_raw_handle(), false)?;
        if before.volume != after.volume
            || before.index_high != after.index_high
            || before.index_low != after.index_low
            || before.size_high != after.size_high
            || before.size_low != after.size_low
            || before.write_high != after.write_high
            || before.write_low != after.write_low
            || before.links != after.links
            || before.attributes != after.attributes
        {
            return Err("WAKE_BROKER_IMAGE_CHANGED".into());
        }
        let after_security = read_security_raw(file.as_raw_handle())?;
        if after_security != before_security {
            return Err("WAKE_BROKER_IMAGE_CHANGED".into());
        }
        Ok((
            format!("{:x}", hasher.finalize()),
            format!("{:x}", Sha256::digest(after_security.sddl.as_bytes())),
        ))
    }

    pub(super) fn inspect() -> Result<BrokerReadOnlySnapshotV1> {
        let service = service_readback()?;
        assert_service(&service)?;
        let service_sid = lookup_service_sid()?;

        // Every child is opened relative to an already pinned parent, with no
        // delete sharing and no reparse traversal. No parent is created.
        let volume = open_root()?;
        let program_data = open_relative(&volume, "ProgramData", true)?;
        assert_ancestor(&program_data)?;
        let data_catdesk = open_relative(&program_data, "CatDesk", true)?;
        assert_ancestor(&data_catdesk)?;
        let protected = open_relative(&data_catdesk, "WakeProtectedInstallV1", true)?;
        let root_security = assert_exact_leaf(&protected, &expected_namespace_sddl(&service_sid))?;
        let protected_root = object_identity(&file_info(&protected, true)?)?;

        let program_files = open_relative(&volume, "Program Files", true)?;
        assert_ancestor(&program_files)?;
        let image_catdesk = open_relative(&program_files, "CatDesk", true)?;
        assert_ancestor(&image_catdesk)?;
        let image_parent = open_relative(&image_catdesk, "WakeInstallBrokerV1", true)?;
        let parent_security =
            assert_exact_leaf(&image_parent, &expected_image_parent_sddl(&service_sid))?;
        let image = open_relative(&image_parent, "CatDeskWakeInstallBroker.exe", false)?;
        let service_image = object_identity(&file_info(&image, false)?)?;
        let (service_image_sha256, service_image_security_sha256) =
            image_digest(image, &service_sid)?;

        Ok(BrokerReadOnlySnapshotV1 {
            schema_version: 1,
            service_name: SERVICE_NAME,
            service_image_sha256,
            service_image,
            service_image_security_sha256,
            protected_root,
            service_security_sha256: format!(
                "{:x}",
                Sha256::digest(service.security_sddl.as_bytes())
            ),
            protected_root_security_sha256: root_security,
            image_parent_security_sha256: parent_security,
            disposition: "READ_ONLY_IDENTITY_SNAPSHOT_NOT_INSTALL_AUTHORITY",
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn exact_service() -> ServiceReadback {
            ServiceReadback {
                service_type: SERVICE_WIN32_OWN_PROCESS,
                start_type: SERVICE_DEMAND_START,
                binary_path: SERVICE_COMMAND_LINE.into(),
                account: "LocalSystem".into(),
                sid_type: SERVICE_SID_TYPE_RESTRICTED,
                security_sddl: canonical_sddl(expected_service_sddl()).expect("policy"),
            }
        }

        #[test]
        fn fixed_service_policy_rejects_candidate_path_account_sid_start_and_security() {
            let exact = exact_service();
            assert!(assert_service(&exact).is_ok());
            let mut bad = exact.clone();
            bad.binary_path = r"C:\Users\worker\CatDeskWakeInstallBroker.exe".into();
            assert_eq!(
                assert_service(&bad).unwrap_err(),
                "WAKE_BROKER_SCM_CONFIG_MISMATCH"
            );
            let mut bad = exact.clone();
            bad.account = "worker".into();
            assert_eq!(
                assert_service(&bad).unwrap_err(),
                "WAKE_BROKER_SCM_CONFIG_MISMATCH"
            );
            let mut bad = exact.clone();
            bad.sid_type = 1;
            assert_eq!(
                assert_service(&bad).unwrap_err(),
                "WAKE_BROKER_SCM_CONFIG_MISMATCH"
            );
            let mut bad = exact.clone();
            bad.start_type = 2;
            assert_eq!(
                assert_service(&bad).unwrap_err(),
                "WAKE_BROKER_SCM_CONFIG_MISMATCH"
            );
            let mut bad = exact;
            bad.security_sddl =
                canonical_sddl("O:SYG:SYD:P(A;;FA;;;SY)(A;;FA;;;IU)").expect("unsafe policy");
            assert_eq!(
                assert_service(&bad).unwrap_err(),
                "WAKE_BROKER_SCM_SECURITY_MISMATCH"
            );
        }

        #[test]
        fn fixed_leaf_policies_are_distinct_and_refuse_broad_writer() {
            let sid = "S-1-5-80-12345";
            let root = canonical_sddl(&expected_namespace_sddl(sid)).expect("root policy");
            let image = canonical_sddl(&expected_image_parent_sddl(sid)).expect("image policy");
            assert_ne!(root, image);
            let broad = canonical_sddl(&format!(
                "O:SYG:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;{sid})(A;OICI;FA;;;IU)"
            ))
            .expect("broad policy");
            let protected_policy = expected_namespace_sddl(sid);
            assert!(
                assert_leaf_policy(
                    &SecurityReadback {
                        owner: "S-1-5-18".into(),
                        sddl: root,
                    },
                    &protected_policy
                )
                .is_ok()
            );
            assert_eq!(
                assert_leaf_policy(
                    &SecurityReadback {
                        owner: "S-1-5-18".into(),
                        sddl: broad,
                    },
                    &protected_policy
                )
                .unwrap_err(),
                "WAKE_BROKER_PROTECTED_ACL_MISMATCH"
            );
            assert_eq!(
                assert_leaf_policy(
                    &SecurityReadback {
                        owner: "S-1-5-32-544".into(),
                        sddl: canonical_sddl(&protected_policy).expect("policy"),
                    },
                    &protected_policy
                )
                .unwrap_err(),
                "WAKE_BROKER_PROTECTED_ACL_MISMATCH"
            );
            let image_policy = expected_image_file_sddl(sid);
            assert!(
                assert_image_file_policy(
                    &SecurityReadback {
                        owner: "S-1-5-18".into(),
                        sddl: canonical_sddl(&image_policy).expect("image policy"),
                    },
                    &image_policy
                )
                .is_ok()
            );
            assert_eq!(
                assert_image_file_policy(
                    &SecurityReadback {
                        owner: "S-1-5-18".into(),
                        sddl: canonical_sddl(&format!(
                            "O:SYG:SYD:P(A;;FA;;;SY)(A;;0x001200A9;;;{sid})(A;;FA;;;IU)"
                        ))
                        .expect("writable image"),
                    },
                    &image_policy
                )
                .unwrap_err(),
                "WAKE_BROKER_IMAGE_ACL_MISMATCH"
            );
        }

        #[test]
        fn component_open_has_no_caller_path_escape() {
            for component in ["..", "foo\\bar", "foo/bar", "", "file:stream"] {
                assert_eq!(
                    validate_component(component).unwrap_err(),
                    "WAKE_BROKER_COMPONENT_INVALID"
                );
            }
        }
    }
}

#[cfg(windows)]
pub fn inspect_fixed_broker_read_only() -> Result<BrokerReadOnlySnapshotV1> {
    windows::inspect()
}
