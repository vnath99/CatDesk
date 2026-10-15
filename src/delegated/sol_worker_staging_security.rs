//! Exact Windows security readback for dormant Sol host staging directories.
//!
//! All descriptor queries use the already opened directory handles. The
//! caller must separately prove that `worker_token_user_sid` came from the
//! restricted worker's actual token, and that the worker lacks privilege or
//! another identity capable of changing ownership or DACLs. This module does
//! not create that worker, stop its descendants, or authorize Sol launch.

#[cfg(not(windows))]
use crate::windows_protected_fs::ProtectedDirectoryGuard;

const LABEL: &str = "Sol worker staging security";

#[cfg(not(windows))]
pub(super) fn verify_private_root(
    _root: &ProtectedDirectoryGuard,
    _worker_token_user_sid: &str,
) -> Result<(), String> {
    Err(format!("{LABEL} requires Windows"))
}

#[cfg(not(windows))]
pub(super) fn verify_private_child(
    _root: &ProtectedDirectoryGuard,
    _child: &ProtectedDirectoryGuard,
    _worker_token_user_sid: &str,
) -> Result<(), String> {
    Err(format!("{LABEL} requires Windows"))
}

#[cfg(not(windows))]
pub(super) fn same_opened_directory(
    _created: &ProtectedDirectoryGuard,
    _security_reopen: &ProtectedDirectoryGuard,
) -> Result<(), String> {
    Err(format!("{LABEL} requires Windows"))
}

#[cfg(windows)]
mod windows {
    use std::ffi::{OsStr, c_void};
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    use crate::windows_protected_fs::{ProtectedDirectoryGuard, handle_information};

    use super::LABEL;

    const OWNER_SECURITY_INFORMATION: u32 = 0x0000_0001;
    const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
    const SE_FILE_OBJECT: u32 = 1;
    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_USER: u32 = 1;
    const SE_DACL_PRESENT: u16 = 0x0004;
    const SE_DACL_PROTECTED: u16 = 0x1000;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    const FILE_ALL_ACCESS: u32 = 0x001f01ff;
    const OBJECT_INHERIT_ACE: u8 = 0x01;
    const CONTAINER_INHERIT_ACE: u8 = 0x02;
    const INHERITED_ACE: u8 = 0x10;
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const ACL_SIZE_INFORMATION_CLASS: u32 = 2;

    fn acceptable_dacl_control(control: u16, require_protected: bool) -> bool {
        control & SE_DACL_PRESENT != 0 && (!require_protected || control & SE_DACL_PROTECTED != 0)
    }

    #[repr(C)]
    struct SidAndAttributes {
        sid: *mut c_void,
        attributes: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct AclSizeInformation {
        ace_count: u32,
        acl_bytes_in_use: u32,
        acl_bytes_free: u32,
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
        fn GetTokenInformation(
            token: *mut c_void,
            class: u32,
            buffer: *mut c_void,
            length: u32,
            returned: *mut u32,
        ) -> i32;
        fn ConvertStringSidToSidW(text: *const u16, sid: *mut *mut c_void) -> i32;
        fn ConvertSidToStringSidW(sid: *mut c_void, text: *mut *mut u16) -> i32;
        fn EqualSid(left: *mut c_void, right: *mut c_void) -> i32;
        fn GetLengthSid(sid: *mut c_void) -> u32;
        fn IsValidSid(sid: *mut c_void) -> i32;
        fn GetSecurityInfo(
            handle: *mut c_void,
            object_type: u32,
            security_information: u32,
            owner: *mut *mut c_void,
            group: *mut *mut c_void,
            dacl: *mut *mut c_void,
            sacl: *mut *mut c_void,
            descriptor: *mut *mut c_void,
        ) -> u32;
        fn IsValidSecurityDescriptor(descriptor: *mut c_void) -> i32;
        fn GetSecurityDescriptorControl(
            descriptor: *mut c_void,
            control: *mut u16,
            revision: *mut u32,
        ) -> i32;
        fn GetSecurityDescriptorDacl(
            descriptor: *mut c_void,
            present: *mut i32,
            dacl: *mut *mut c_void,
            defaulted: *mut i32,
        ) -> i32;
        fn IsValidAcl(acl: *mut c_void) -> i32;
        fn GetAclInformation(
            acl: *mut c_void,
            information: *mut c_void,
            length: u32,
            class: u32,
        ) -> i32;
        fn GetAce(acl: *mut c_void, index: u32, ace: *mut *mut c_void) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    struct LocalAllocation(*mut c_void);
    impl Drop for LocalAllocation {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { LocalFree(self.0) };
            }
        }
    }

    struct OwnedToken(*mut c_void);
    impl Drop for OwnedToken {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    fn numeric_sid(value: &str) -> bool {
        value.len() <= 256
            && value.starts_with("S-1-")
            && value[4..]
                .split('-')
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }

    fn sid_from_numeric(value: &str) -> Result<LocalAllocation, String> {
        if !numeric_sid(value) {
            return Err(format!("{LABEL} worker SID is invalid"));
        }
        let wide = OsStr::new(value)
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut sid = ptr::null_mut();
        if unsafe { ConvertStringSidToSidW(wide.as_ptr(), &mut sid) } == 0 || sid.is_null() {
            return Err(format!("{LABEL} worker SID is invalid"));
        }
        let allocation = LocalAllocation(sid);
        if unsafe { IsValidSid(sid) } == 0 {
            return Err(format!("{LABEL} worker SID is invalid"));
        }
        Ok(allocation)
    }

    // Validate the complete SID extent before allowing the Win32 SID routines
    // to inspect an untrusted pointer returned inside a variable-sized buffer.
    fn bounded_sid_length(
        sid: *mut c_void,
        region_start: usize,
        region_end: usize,
    ) -> Option<usize> {
        let sid_start = sid as usize;
        if sid_start < region_start || sid_start.checked_add(8)? > region_end {
            return None;
        }
        let sub_authorities = unsafe { *sid.cast::<u8>().add(1) } as usize;
        let sid_size = 8_usize.checked_add(4_usize.checked_mul(sub_authorities)?)?;
        (sid_start.checked_add(sid_size)? <= region_end).then_some(sid_size)
    }

    fn current_token_user() -> Result<(OwnedToken, Vec<u8>, *mut c_void), String> {
        let mut token = ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0
            || token.is_null()
        {
            return Err(format!("{LABEL} host token unavailable"));
        }
        let token = OwnedToken(token);
        let mut size = 0_u32;
        unsafe { GetTokenInformation(token.0, TOKEN_USER, ptr::null_mut(), 0, &mut size) };
        if size < std::mem::size_of::<SidAndAttributes>() as u32 || size > 64 * 1024 {
            return Err(format!("{LABEL} host token invalid"));
        }
        let mut bytes = vec![0_u8; size as usize];
        let mut returned = 0_u32;
        if unsafe {
            GetTokenInformation(
                token.0,
                TOKEN_USER,
                bytes.as_mut_ptr().cast(),
                size,
                &mut returned,
            )
        } == 0
            || returned != size
        {
            return Err(format!("{LABEL} host token unavailable"));
        }
        let user = unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<SidAndAttributes>()) };
        let region_start = bytes.as_ptr() as usize;
        let region_end = region_start
            .checked_add(returned as usize)
            .ok_or_else(|| format!("{LABEL} host token invalid"))?;
        let sid_size = bounded_sid_length(user.sid, region_start, region_end)
            .ok_or_else(|| format!("{LABEL} host token invalid"))?;
        if unsafe { IsValidSid(user.sid) } == 0
            || unsafe { GetLengthSid(user.sid) } as usize != sid_size
        {
            return Err(format!("{LABEL} host token invalid"));
        }
        Ok((token, bytes, user.sid))
    }

    fn same_sid(left: *mut c_void, right: *mut c_void) -> bool {
        unsafe { EqualSid(left, right) != 0 }
    }

    fn opened_identity(guard: &ProtectedDirectoryGuard) -> Result<(u32, u32, u32), String> {
        guard.assert_stable(LABEL)?;
        let handle = guard.last_handle(LABEL)?;
        let info = handle_information(handle, LABEL)?;
        if info.file_attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)
            != FILE_ATTRIBUTE_DIRECTORY
        {
            return Err(format!(
                "{LABEL} opened object is not an ordinary directory"
            ));
        }
        Ok((
            info.volume_serial_number,
            info.file_index_high,
            info.file_index_low,
        ))
    }

    fn audit_descriptor(
        guard: &ProtectedDirectoryGuard,
        host_sid: *mut c_void,
        system_sid: *mut c_void,
        require_protected: bool,
    ) -> Result<(), String> {
        let before = opened_identity(guard)?;
        let handle = guard.last_handle(LABEL)?;
        let mut owner = ptr::null_mut();
        let mut dacl = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        let status = unsafe {
            GetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != 0 || descriptor.is_null() {
            if !descriptor.is_null() {
                unsafe { LocalFree(descriptor) };
            }
            return Err(format!("{LABEL} pinned ACL readback unavailable"));
        }
        let descriptor = LocalAllocation(descriptor);
        if unsafe { IsValidSecurityDescriptor(descriptor.0) } == 0
            || owner.is_null()
            || unsafe { IsValidSid(owner) } == 0
            || !same_sid(owner, host_sid)
        {
            return Err(format!("{LABEL} owner differs from host token"));
        }
        let mut control = 0_u16;
        let mut revision = 0_u32;
        if unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) } == 0
            || !acceptable_dacl_control(control, require_protected)
        {
            return Err(format!("{LABEL} DACL control is unsafe"));
        }
        let mut present = 0_i32;
        let mut defaulted = 0_i32;
        let mut descriptor_dacl = ptr::null_mut();
        if unsafe {
            GetSecurityDescriptorDacl(
                descriptor.0,
                &mut present,
                &mut descriptor_dacl,
                &mut defaulted,
            )
        } == 0
            || present == 0
            || defaulted != 0
            || descriptor_dacl.is_null()
            || dacl != descriptor_dacl
            || unsafe { IsValidAcl(dacl) } == 0
        {
            return Err(format!("{LABEL} DACL is absent or unsafe"));
        }
        let mut information = AclSizeInformation::default();
        if unsafe {
            GetAclInformation(
                dacl,
                (&mut information as *mut AclSizeInformation).cast(),
                std::mem::size_of::<AclSizeInformation>() as u32,
                ACL_SIZE_INFORMATION_CLASS,
            )
        } == 0
            || !(8..=8192).contains(&information.acl_bytes_in_use)
            || information.ace_count != if same_sid(host_sid, system_sid) { 1 } else { 2 }
        {
            return Err(format!("{LABEL} DACL entries differ from host policy"));
        }
        let acl_start = dacl as usize;
        let acl_end = acl_start
            .checked_add(information.acl_bytes_in_use as usize)
            .ok_or_else(|| format!("{LABEL} DACL entry is unsafe"))?;
        let first_ace = acl_start
            .checked_add(8)
            .ok_or_else(|| format!("{LABEL} DACL entry is unsafe"))?;
        let mut host_seen = false;
        let mut system_seen = false;
        for index in 0..information.ace_count {
            let mut ace = ptr::null_mut();
            if unsafe { GetAce(dacl, index, &mut ace) } == 0 || ace.is_null() {
                return Err(format!("{LABEL} DACL entry unavailable"));
            }
            let ace_start = ace as usize;
            if ace_start < first_ace || ace_start.checked_add(8).is_none_or(|end| end > acl_end) {
                return Err(format!("{LABEL} DACL entry is unsafe"));
            }
            let bytes = ace.cast::<u8>();
            let kind = unsafe { *bytes };
            let flags = unsafe { *bytes.add(1) };
            let size = unsafe { ptr::read_unaligned(bytes.add(2).cast::<u16>()) } as usize;
            let mask = unsafe { ptr::read_unaligned(bytes.add(4).cast::<u32>()) };
            let ace_end = ace_start
                .checked_add(size)
                .ok_or_else(|| format!("{LABEL} DACL entry is unsafe"))?;
            if size < 16 || ace_end > acl_end {
                return Err(format!("{LABEL} DACL entry is unsafe"));
            }
            let sid_bytes = unsafe { bytes.add(8) };
            let sid_size = bounded_sid_length(sid_bytes.cast(), ace_start + 8, ace_end)
                .ok_or_else(|| format!("{LABEL} DACL entry is unsafe"))?;
            if size != 8 + sid_size {
                return Err(format!("{LABEL} DACL entry is unsafe"));
            }
            let sid = sid_bytes.cast::<c_void>();
            if unsafe { IsValidSid(sid) } == 0 {
                return Err(format!("{LABEL} DACL trustee is invalid"));
            }
            let sid_len = unsafe { GetLengthSid(sid) } as usize;
            let required_flags = OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE;
            let flags_accepted = flags == required_flags
                || (!require_protected && flags == (required_flags | INHERITED_ACE));
            if kind != ACCESS_ALLOWED_ACE_TYPE
                || !flags_accepted
                || mask != FILE_ALL_ACCESS
                || sid_len == 0
                || sid_len != sid_size
            {
                return Err(format!("{LABEL} DACL entry is unsafe"));
            }
            if same_sid(sid, host_sid) && !host_seen {
                host_seen = true;
            } else if same_sid(sid, system_sid) && !system_seen {
                system_seen = true;
            } else {
                return Err(format!("{LABEL} DACL trustee is unsafe"));
            }
        }
        if !host_seen || (!same_sid(host_sid, system_sid) && !system_seen) {
            return Err(format!("{LABEL} host DACL entry missing"));
        }
        if opened_identity(guard)? != before {
            return Err(format!(
                "{LABEL} opened identity changed during ACL readback"
            ));
        }
        Ok(())
    }

    fn audit(
        guard: &ProtectedDirectoryGuard,
        worker_token_user_sid: &str,
        require_protected: bool,
    ) -> Result<(), String> {
        let worker = sid_from_numeric(worker_token_user_sid)?;
        let system = sid_from_numeric("S-1-5-18")?;
        let (_token, _bytes, host_sid) = current_token_user()?;
        if same_sid(worker.0, host_sid) || same_sid(worker.0, system.0) {
            return Err(format!("{LABEL} worker is not excluded from host trustees"));
        }
        audit_descriptor(guard, host_sid, system.0, require_protected)
    }

    pub(crate) fn verify_private_root(
        root: &ProtectedDirectoryGuard,
        worker_token_user_sid: &str,
    ) -> Result<(), String> {
        audit(root, worker_token_user_sid, true)
    }

    pub(crate) fn verify_private_child(
        root: &ProtectedDirectoryGuard,
        child: &ProtectedDirectoryGuard,
        worker_token_user_sid: &str,
    ) -> Result<(), String> {
        if !child.is_direct_child_of(root) {
            return Err(format!("{LABEL} child is outside pinned root"));
        }
        verify_private_root(root, worker_token_user_sid)?;
        audit(child, worker_token_user_sid, false)?;
        verify_private_root(root, worker_token_user_sid)
    }

    pub(crate) fn same_opened_directory(
        created: &ProtectedDirectoryGuard,
        security_reopen: &ProtectedDirectoryGuard,
    ) -> Result<(), String> {
        if opened_identity(created)? != opened_identity(security_reopen)? {
            return Err(format!("{LABEL} created child identity changed"));
        }
        Ok(())
    }

    pub(crate) fn current_host_sid() -> Result<String, String> {
        let (_token, _bytes, sid) = current_token_user()?;
        let mut text = ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
            return Err(format!("{LABEL} host SID unavailable"));
        }
        let allocation = LocalAllocation(text.cast());
        let mut length = 0usize;
        while length < 256 && unsafe { *text.add(length) } != 0 {
            length += 1;
        }
        if length == 0 || length == 256 {
            return Err(format!("{LABEL} host SID invalid"));
        }
        String::from_utf16(unsafe {
            std::slice::from_raw_parts(allocation.0.cast::<u16>(), length)
        })
        .map_err(|_| format!("{LABEL} host SID invalid"))
    }

    #[cfg(test)]
    #[test]
    fn unprotected_or_missing_root_dacl_control_is_rejected() {
        assert!(!acceptable_dacl_control(0, true));
        assert!(!acceptable_dacl_control(SE_DACL_PRESENT, true));
        assert!(acceptable_dacl_control(
            SE_DACL_PRESENT | SE_DACL_PROTECTED,
            true,
        ));
    }

    #[cfg(test)]
    #[test]
    fn sid_extent_must_fit_returned_buffer_before_win32_validation() {
        let mut bytes = [0_u8; 16];
        let start = bytes.as_mut_ptr() as usize;
        let end = start + bytes.len();
        bytes[1] = 2;
        assert_eq!(
            bounded_sid_length(bytes.as_mut_ptr().cast(), start, end),
            Some(16)
        );
        assert_eq!(
            bounded_sid_length(bytes.as_mut_ptr().cast(), start, end - 1),
            None
        );
        bytes[1] = 3;
        assert_eq!(
            bounded_sid_length(bytes.as_mut_ptr().cast(), start, end),
            None
        );
        assert_eq!(bounded_sid_length(end as *mut c_void, start, end), None);
    }
}

#[cfg(windows)]
pub(super) use windows::{
    current_host_sid, same_opened_directory, verify_private_child, verify_private_root,
};
