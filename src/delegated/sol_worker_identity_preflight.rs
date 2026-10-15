//! Read-only diagnostic for one fixed local Sol worker account.
//!
//! Account existence, SID/type and local-group readback do not prove that a
//! future worker process uses this account, that its descendants have exited,
//! or that it lost write authority. This module acquires no credentials and
//! grants no Sol launch or repository authority.

#![cfg(windows)]

use std::ffi::c_void;
use std::mem::size_of;
use std::os::windows::io::BorrowedHandle;
use std::ptr;

use super::sol_worker_staging_security;
use super::sol_worker_token_security::{
    ObservedRestrictedWorkerProcessTokenV1, observe_restricted_worker_process_token,
};

const LABEL: &str = "Sol worker local account preflight";
const ACCOUNT: &str = "CatDeskSolWorker";
const ADMINISTRATORS_SID: &str = "S-1-5-32-544";
const SYSTEM_SID: &str = "S-1-5-18";
const USER_INFO_LEVEL: u32 = 23;
const SERVER_INFO_LEVEL: u32 = 101;
const LG_INCLUDE_INDIRECT: u32 = 0x1;
const SV_TYPE_WORKSTATION: u32 = 0x0000_0001;
const SV_TYPE_SERVER: u32 = 0x0000_0002;
const SV_TYPE_DOMAIN_CTRL: u32 = 0x0000_0008;
const SV_TYPE_DOMAIN_BAKCTRL: u32 = 0x0000_0010;
const MAX_GROUP_BUFFER: u32 = 16 * 1024;
const MAX_ACCOUNT_BUFFER: u32 = 64 * 1024;
const MAX_GROUPS: u32 = 128;
const MAX_WIDE_CHARS: usize = 256;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const UF_ACCOUNTDISABLE: u32 = 0x0002;
const UF_LOCKOUT: u32 = 0x0010;
const UF_NORMAL_ACCOUNT: u32 = 0x0200;
const UF_ACCOUNT_TYPE_MASK: u32 = 0x3b00;
const UF_TRUSTED_FOR_DELEGATION: u32 = 0x0008_0000;
const UF_TRUSTED_TO_AUTHENTICATE_FOR_DELEGATION: u32 = 0x0100_0000;
const SID_TYPE_USER: u32 = 1;
const SID_TYPE_ALIAS: u32 = 4;
const CSTR_EQUAL: i32 = 2;

#[repr(C)]
struct UserInfo23 {
    name: *mut u16,
    full_name: *mut u16,
    comment: *mut u16,
    flags: u32,
    sid: *mut c_void,
}

#[repr(C)]
struct LocalGroupUsersInfo0 {
    name: *mut u16,
}

#[repr(C)]
struct ServerInfo101 {
    _platform_id: u32,
    _name: *mut u16,
    _version_major: u32,
    _version_minor: u32,
    server_type: u32,
    _comment: *mut u16,
}

/// Diagnostic evidence only. A real launch must separately prove an actual
/// restricted process token and denied descendant/ancestor write paths.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SolWorkerLocalAccountPreflightV1 {
    pub account_name: &'static str,
    pub local_computer: String,
    pub account_sid: String,
    pub host_sid: String,
    pub indirect_local_group_count: u32,
    account_flags: u32,
    indirect_local_groups: Vec<String>,
    pub diagnostic_only: bool,
    pub worker_process_token_proven: bool,
    pub account_privileges_proven: bool,
}

/// Read-only evidence that the fixed local account record and one opened,
/// live restricted process currently have the same TokenUser SID. Private
/// fields keep the caller from supplying a claimed account or token SID.
/// This still does not prove which executable was launched or any descendant,
/// inherited-handle, ancestor-ACL, or post-exit write boundary.
pub(crate) struct BoundFixedLocalSolWorkerProcessV1 {
    account: SolWorkerLocalAccountPreflightV1,
    observed_token: ObservedRestrictedWorkerProcessTokenV1,
}

impl BoundFixedLocalSolWorkerProcessV1 {
    pub(crate) fn process_id(&self) -> u32 {
        self.observed_token.process_id()
    }

    pub(crate) fn token_user_sid(&self) -> &str {
        self.observed_token.token_user_sid()
    }

    pub(crate) fn require_same_opened_process(
        &self,
        process: BorrowedHandle<'_>,
    ) -> Result<(), String> {
        self.revalidate()?;
        self.observed_token.require_same_opened_process(process)?;
        self.revalidate()
    }

    pub(crate) fn reject_stage_acl_trustees(&self, trustees: &[&str]) -> Result<(), String> {
        self.observed_token.reject_stage_acl_trustees(trustees)
    }

    pub(crate) fn revalidate(&self) -> Result<(), String> {
        self.observed_token.revalidate()?;
        let fresh = preflight_catdesk_sol_worker_local_account()?;
        require_same_account_and_token(
            &self.account,
            &fresh,
            self.observed_token.token_user_sid(),
        )?;
        self.observed_token.revalidate()
    }

    /// Recheck the fixed account around the retained process/token readback
    /// after normal exit. The live-process `revalidate` path is deliberately
    /// unavailable at this point. This grants no output or staging authority.
    pub(crate) fn require_retained_exited_identity(&self) -> Result<(), String> {
        let first = preflight_catdesk_sol_worker_local_account()?;
        require_same_account_and_token(
            &self.account,
            &first,
            self.observed_token.token_user_sid(),
        )?;
        self.observed_token.require_retained_exited_snapshot()?;
        let second = preflight_catdesk_sol_worker_local_account()?;
        require_same_account_and_token(
            &self.account,
            &second,
            self.observed_token.token_user_sid(),
        )?;
        self.observed_token.require_retained_exited_snapshot()
    }
}

fn require_same_account_and_token(
    first: &SolWorkerLocalAccountPreflightV1,
    fresh: &SolWorkerLocalAccountPreflightV1,
    token_user_sid: &str,
) -> Result<(), String> {
    if first != fresh {
        return Err(format!(
            "{LABEL} fixed account changed during process binding"
        ));
    }
    if token_user_sid != first.account_sid {
        return Err(format!(
            "{LABEL} process TokenUser is not the fixed account"
        ));
    }
    Ok(())
}

/// Bind a caller-supplied opened process to the fixed local account, using
/// independently observed primary-token readback. The caller must still
/// prove that this process is the intended launched Codex worker. This
/// dormant function performs no launch, staging, or repository write.
pub(crate) fn bind_fixed_local_sol_worker_to_opened_process(
    process: BorrowedHandle<'_>,
) -> Result<BoundFixedLocalSolWorkerProcessV1, String> {
    let account = preflight_catdesk_sol_worker_local_account()?;
    let observed_token = observe_restricted_worker_process_token(process)?;
    let fresh = preflight_catdesk_sol_worker_local_account()?;
    require_same_account_and_token(&account, &fresh, observed_token.token_user_sid())?;
    let bound = BoundFixedLocalSolWorkerProcessV1 {
        account,
        observed_token,
    };
    bound.revalidate()?;
    Ok(bound)
}

#[link(name = "netapi32")]
unsafe extern "system" {
    fn NetUserGetInfo(
        server: *const u16,
        user: *const u16,
        level: u32,
        buffer: *mut *mut u8,
    ) -> u32;
    fn NetServerGetInfo(server: *const u16, level: u32, buffer: *mut *mut u8) -> u32;
    fn NetUserGetLocalGroups(
        server: *const u16,
        user: *const u16,
        level: u32,
        flags: u32,
        buffer: *mut *mut u8,
        preferred_max_len: u32,
        entries_read: *mut u32,
        total_entries: *mut u32,
    ) -> u32;
    fn NetApiBufferSize(buffer: *mut c_void, size: *mut u32) -> u32;
    fn NetApiBufferFree(buffer: *mut c_void) -> u32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn LookupAccountSidW(
        system: *const u16,
        sid: *mut c_void,
        name: *mut u16,
        name_length: *mut u32,
        domain: *mut u16,
        domain_length: *mut u32,
        use_type: *mut u32,
    ) -> i32;
    fn ConvertStringSidToSidW(text: *const u16, sid: *mut *mut c_void) -> i32;
    fn ConvertSidToStringSidW(sid: *mut c_void, text: *mut *mut u16) -> i32;
    fn IsValidSid(sid: *mut c_void) -> i32;
    fn GetLengthSid(sid: *mut c_void) -> u32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetComputerNameW(name: *mut u16, size: *mut u32) -> i32;
    fn CompareStringOrdinal(
        first: *const u16,
        first_length: i32,
        second: *const u16,
        second_length: i32,
        ignore_case: i32,
    ) -> i32;
    fn GetLastError() -> u32;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

struct NetBuffer(*mut u8);
impl Drop for NetBuffer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { NetApiBufferFree(self.0.cast()) };
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
    value.encode_utf16().chain(Some(0)).collect()
}

fn same_wide_case_insensitive(left: &str, right: &str) -> bool {
    let left = left.encode_utf16().collect::<Vec<_>>();
    let right = right.encode_utf16().collect::<Vec<_>>();
    unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            1,
        ) == CSTR_EQUAL
    }
}

fn local_computer_name() -> Result<String, String> {
    let mut name = [0_u16; MAX_WIDE_CHARS];
    let mut size = name.len() as u32;
    if unsafe { GetComputerNameW(name.as_mut_ptr(), &mut size) } == 0
        || size == 0
        || size as usize >= name.len()
    {
        return Err(format!("{LABEL} local computer name unavailable"));
    }
    String::from_utf16(&name[..size as usize])
        .map_err(|_| format!("{LABEL} local computer name invalid"))
}

fn is_local_sam_server_type(server_type: u32) -> bool {
    server_type & (SV_TYPE_WORKSTATION | SV_TYPE_SERVER) != 0
        && server_type & (SV_TYPE_DOMAIN_CTRL | SV_TYPE_DOMAIN_BAKCTRL) == 0
}

fn require_local_sam_host() -> Result<(), String> {
    let mut raw = ptr::null_mut();
    let status = unsafe { NetServerGetInfo(ptr::null(), SERVER_INFO_LEVEL, &mut raw) };
    let allocation = NetBuffer(raw);
    if status != 0 {
        return Err(format!("{LABEL} local server role unavailable ({status})"));
    }
    let length = net_buffer_size(allocation.0, MAX_ACCOUNT_BUFFER)?;
    if length < size_of::<ServerInfo101>() {
        return Err(format!("{LABEL} local server role truncated"));
    }
    let record = unsafe { ptr::read_unaligned(allocation.0.cast::<ServerInfo101>()) };
    if !is_local_sam_server_type(record.server_type) {
        return Err(format!(
            "{LABEL} host is not a local-SAM member/workstation"
        ));
    }
    Ok(())
}

fn net_buffer_size(buffer: *mut u8, limit: u32) -> Result<usize, String> {
    if buffer.is_null() {
        return Err(format!("{LABEL} account query returned no buffer"));
    }
    let mut size = 0_u32;
    if unsafe { NetApiBufferSize(buffer.cast(), &mut size) } != 0 || size == 0 || size > limit {
        return Err(format!("{LABEL} account query buffer is unbounded"));
    }
    let size = size as usize;
    if (buffer as usize).checked_add(size).is_none() {
        return Err(format!("{LABEL} account query buffer address overflow"));
    }
    Ok(size)
}

fn bounded_tail(
    buffer_start: usize,
    buffer_len: usize,
    prefix_len: usize,
) -> Result<(usize, usize), String> {
    if prefix_len > buffer_len || buffer_start.checked_add(buffer_len).is_none() {
        return Err(format!("{LABEL} group buffer extent invalid"));
    }
    let tail_start = buffer_start
        .checked_add(prefix_len)
        .ok_or_else(|| format!("{LABEL} group buffer address overflow"))?;
    Ok((tail_start, buffer_len - prefix_len))
}

fn bounded_wide_string(
    value: *const u16,
    buffer_start: usize,
    buffer_len: usize,
) -> Result<String, String> {
    let start = value as usize;
    let end = buffer_start
        .checked_add(buffer_len)
        .ok_or_else(|| format!("{LABEL} string buffer overflow"))?;
    if start < buffer_start || start.checked_add(2).is_none_or(|x| x > end) {
        return Err(format!("{LABEL} string outside account buffer"));
    }
    let mut units = Vec::new();
    for index in 0..MAX_WIDE_CHARS {
        let offset = index
            .checked_mul(2)
            .and_then(|n| start.checked_add(n))
            .ok_or_else(|| format!("{LABEL} string overflow"))?;
        if offset.checked_add(2).is_none_or(|x| x > end) {
            return Err(format!("{LABEL} string exceeds account buffer"));
        }
        let unit = unsafe { ptr::read_unaligned((offset as *const u8).cast::<u16>()) };
        if unit == 0 {
            if units.is_empty() {
                return Err(format!("{LABEL} empty account string"));
            }
            return String::from_utf16(&units)
                .map_err(|_| format!("{LABEL} account string invalid"));
        }
        units.push(unit);
    }
    Err(format!("{LABEL} account string too long"))
}

fn bounded_sid_text(
    sid: *mut c_void,
    buffer_start: usize,
    buffer_len: usize,
) -> Result<String, String> {
    let start = sid as usize;
    let end = buffer_start
        .checked_add(buffer_len)
        .ok_or_else(|| format!("{LABEL} SID buffer overflow"))?;
    if start < buffer_start || start.checked_add(8).is_none_or(|x| x > end) {
        return Err(format!("{LABEL} SID outside account buffer"));
    }
    let count = unsafe { *sid.cast::<u8>().add(1) } as usize;
    let length = 8_usize
        .checked_add(
            4_usize
                .checked_mul(count)
                .ok_or_else(|| format!("{LABEL} SID invalid"))?,
        )
        .ok_or_else(|| format!("{LABEL} SID invalid"))?;
    if start.checked_add(length).is_none_or(|x| x > end)
        || unsafe { IsValidSid(sid) } == 0
        || unsafe { GetLengthSid(sid) } as usize != length
    {
        return Err(format!("{LABEL} SID invalid"));
    }
    let mut text = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
        return Err(format!("{LABEL} SID conversion failed"));
    }
    let allocation = LocalAllocation(text.cast());
    // ConvertSidToStringSidW returns a NUL-terminated LocalAlloc string.
    let mut units = Vec::new();
    for index in 0..MAX_WIDE_CHARS {
        let unit = unsafe { *text.add(index) };
        if unit == 0 {
            return String::from_utf16(&units).map_err(|_| format!("{LABEL} SID text invalid"));
        }
        units.push(unit);
    }
    drop(allocation);
    Err(format!("{LABEL} SID text too long"))
}

fn local_account_record() -> Result<(String, u32), String> {
    let user = wide(ACCOUNT);
    let mut raw = ptr::null_mut();
    let status = unsafe { NetUserGetInfo(ptr::null(), user.as_ptr(), USER_INFO_LEVEL, &mut raw) };
    let allocation = NetBuffer(raw);
    if status != 0 {
        return Err(format!(
            "{LABEL} fixed local account unavailable ({status})"
        ));
    }
    let length = net_buffer_size(allocation.0, MAX_ACCOUNT_BUFFER)?;
    if length < size_of::<UserInfo23>() {
        return Err(format!("{LABEL} local account record truncated"));
    }
    let record = unsafe { ptr::read_unaligned(allocation.0.cast::<UserInfo23>()) };
    let account_name = bounded_wide_string(record.name, allocation.0 as usize, length)?;
    if !same_wide_case_insensitive(&account_name, ACCOUNT) {
        return Err(format!("{LABEL} account name is ambiguous"));
    }
    let sid = bounded_sid_text(record.sid, allocation.0 as usize, length)?;
    Ok((sid, record.flags))
}

fn lookup_sid_identity(sid: *mut c_void) -> Result<(String, String, u32), String> {
    let mut name_len = 0_u32;
    let mut domain_len = 0_u32;
    let mut use_type = 0_u32;
    let first = unsafe {
        LookupAccountSidW(
            ptr::null(),
            sid,
            ptr::null_mut(),
            &mut name_len,
            ptr::null_mut(),
            &mut domain_len,
            &mut use_type,
        )
    };
    if first != 0
        || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER
        || name_len == 0
        || domain_len == 0
        || name_len as usize > MAX_WIDE_CHARS
        || domain_len as usize > MAX_WIDE_CHARS
    {
        return Err(format!("{LABEL} SID reverse lookup unavailable"));
    }
    let mut name = vec![0_u16; name_len as usize];
    let mut domain = vec![0_u16; domain_len as usize];
    if unsafe {
        LookupAccountSidW(
            ptr::null(),
            sid,
            name.as_mut_ptr(),
            &mut name_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut use_type,
        )
    } == 0
        || name_len == 0
        || domain_len == 0
        || name_len as usize > name.len()
        || domain_len as usize > domain.len()
    {
        return Err(format!("{LABEL} SID reverse lookup changed"));
    }
    let name = String::from_utf16(&name[..name_len as usize])
        .map_err(|_| format!("{LABEL} account name invalid"))?;
    let domain = String::from_utf16(&domain[..domain_len as usize])
        .map_err(|_| format!("{LABEL} account domain invalid"))?;
    if name.is_empty() || domain.is_empty() || name.contains('\0') || domain.contains('\0') {
        return Err(format!("{LABEL} SID reverse lookup invalid"));
    }
    Ok((name, domain, use_type))
}

fn sid_from_numeric(value: &str) -> Result<LocalAllocation, String> {
    let text = wide(value);
    let mut sid = ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(text.as_ptr(), &mut sid) } == 0 || sid.is_null() {
        return Err(format!("{LABEL} fixed SID unavailable"));
    }
    Ok(LocalAllocation(sid))
}

fn reverse_lookup_numeric_sid(value: &str) -> Result<(String, String, u32), String> {
    let sid = sid_from_numeric(value)?;
    lookup_sid_identity(sid.0)
}

fn indirect_local_groups(computer: &str) -> Result<Vec<String>, String> {
    // Bind enumeration to the same local account domain used in SID readback.
    let user = wide(&format!("{computer}\\{ACCOUNT}"));
    let mut raw = ptr::null_mut();
    let mut entries = 0_u32;
    let mut total = 0_u32;
    let status = unsafe {
        NetUserGetLocalGroups(
            ptr::null(),
            user.as_ptr(),
            0,
            LG_INCLUDE_INDIRECT,
            &mut raw,
            MAX_GROUP_BUFFER,
            &mut entries,
            &mut total,
        )
    };
    let allocation = NetBuffer(raw);
    if status != 0 || entries != total || entries > MAX_GROUPS {
        return Err(format!(
            "{LABEL} local group membership incomplete ({status})"
        ));
    }
    if entries == 0 {
        return Ok(Vec::new());
    }
    let length = net_buffer_size(allocation.0, MAX_GROUP_BUFFER)?;
    let records_len = (entries as usize)
        .checked_mul(size_of::<LocalGroupUsersInfo0>())
        .ok_or_else(|| format!("{LABEL} group count invalid"))?;
    let (names_start, names_len) = bounded_tail(allocation.0 as usize, length, records_len)?;
    let mut groups = Vec::with_capacity(entries as usize);
    for index in 0..entries as usize {
        let entry = unsafe {
            ptr::read_unaligned(
                allocation
                    .0
                    .add(index * size_of::<LocalGroupUsersInfo0>())
                    .cast::<LocalGroupUsersInfo0>(),
            )
        };
        let name = bounded_wide_string(entry.name, names_start, names_len)?;
        groups.push(name);
    }
    Ok(groups)
}

fn assess_identity(
    account_sid: &str,
    host_sid: &str,
    flags: u32,
    local_groups: &[String],
    administrators_name: &str,
) -> Result<(), String> {
    if account_sid == host_sid || account_sid == SYSTEM_SID {
        return Err(format!("{LABEL} account is host or SYSTEM"));
    }
    if flags & UF_ACCOUNT_TYPE_MASK != UF_NORMAL_ACCOUNT
        || flags
            & (UF_ACCOUNTDISABLE
                | UF_LOCKOUT
                | UF_TRUSTED_FOR_DELEGATION
                | UF_TRUSTED_TO_AUTHENTICATE_FOR_DELEGATION)
            != 0
    {
        return Err(format!("{LABEL} account is not an enabled normal user"));
    }
    if local_groups
        .iter()
        .any(|group| same_wide_case_insensitive(group, administrators_name))
    {
        return Err(format!("{LABEL} account belongs to Administrators"));
    }
    Ok(())
}

/// Zero-argument, read-only local-SAM preflight. `Ok` means only that these
/// current account records passed; it never authorizes launch or staging.
pub(crate) fn preflight_catdesk_sol_worker_local_account()
-> Result<SolWorkerLocalAccountPreflightV1, String> {
    let computer = local_computer_name()?;
    require_local_sam_host()?;
    let (account_sid, flags) = local_account_record()?;
    let (name, domain, use_type) = reverse_lookup_numeric_sid(&account_sid)?;
    if use_type != SID_TYPE_USER
        || !same_wide_case_insensitive(&name, ACCOUNT)
        || !same_wide_case_insensitive(&domain, &computer)
    {
        return Err(format!("{LABEL} SID is not the fixed local user"));
    }
    let (administrators_name, _, administrators_type) =
        reverse_lookup_numeric_sid(ADMINISTRATORS_SID)?;
    if administrators_type != SID_TYPE_ALIAS {
        return Err(format!("{LABEL} Administrators alias unavailable"));
    }
    let host_sid = sol_worker_staging_security::current_host_sid()?;
    let mut groups = indirect_local_groups(&computer)?;
    assess_identity(
        &account_sid,
        &host_sid,
        flags,
        &groups,
        &administrators_name,
    )?;
    // Detect simple rename/replacement races without claiming a stable account
    // transaction. The actual process token remains the later authority.
    let (again_sid, again_flags) = local_account_record()?;
    if again_sid != account_sid || again_flags != flags {
        return Err(format!("{LABEL} local account changed during readback"));
    }
    groups.sort_unstable();
    Ok(SolWorkerLocalAccountPreflightV1 {
        account_name: ACCOUNT,
        local_computer: computer,
        account_sid,
        host_sid,
        indirect_local_group_count: groups.len() as u32,
        account_flags: flags,
        indirect_local_groups: groups,
        diagnostic_only: true,
        worker_process_token_proven: false,
        account_privileges_proven: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
    }

    #[test]
    fn real_current_host_process_never_binds_as_fixed_worker() {
        let process = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess()) };
        assert!(bind_fixed_local_sol_worker_to_opened_process(process).is_err());
    }

    fn fixture_account() -> SolWorkerLocalAccountPreflightV1 {
        SolWorkerLocalAccountPreflightV1 {
            account_name: ACCOUNT,
            local_computer: "FIXTURE".into(),
            account_sid: "S-1-5-21-1-2-3-1001".into(),
            host_sid: "S-1-5-21-1-2-3-1000".into(),
            indirect_local_group_count: 1,
            account_flags: UF_NORMAL_ACCOUNT,
            indirect_local_groups: vec!["Users".into()],
            diagnostic_only: true,
            worker_process_token_proven: false,
            account_privileges_proven: false,
        }
    }

    #[test]
    fn process_token_user_must_match_fixed_account_sid() {
        let account = fixture_account();
        assert!(require_same_account_and_token(&account, &account, &account.account_sid).is_ok());
        assert!(require_same_account_and_token(&account, &account, &account.host_sid).is_err());
        assert!(require_same_account_and_token(&account, &account, SYSTEM_SID).is_err());
    }

    #[test]
    fn fixed_account_change_invalidates_process_binding() {
        let account = fixture_account();
        let mut changed = account.clone();
        changed.account_sid = "S-1-5-21-1-2-3-2001".into();
        assert!(require_same_account_and_token(&account, &changed, &account.account_sid).is_err());
        changed = account.clone();
        changed.indirect_local_group_count += 1;
        assert!(require_same_account_and_token(&account, &changed, &account.account_sid).is_err());
        changed = account.clone();
        changed.indirect_local_groups[0] = "Remote Desktop Users".into();
        assert!(require_same_account_and_token(&account, &changed, &account.account_sid).is_err());
        changed = account.clone();
        changed.account_flags |= 0x10000;
        assert!(require_same_account_and_token(&account, &changed, &account.account_sid).is_err());
        changed = account.clone();
        changed.local_computer = "OTHER".into();
        assert!(require_same_account_and_token(&account, &changed, &account.account_sid).is_err());
    }

    #[test]
    fn real_fixed_account_preflight_is_diagnostic_or_fails_closed() {
        match preflight_catdesk_sol_worker_local_account() {
            Ok(result) => {
                assert_eq!(result.account_name, ACCOUNT);
                assert!(result.diagnostic_only);
                assert!(!result.worker_process_token_proven);
                assert!(!result.account_privileges_proven);
                eprintln!("{LABEL}: fixed account record passed diagnostic readback only");
            }
            Err(reason) => {
                assert!(reason.starts_with(LABEL));
                assert!(reason.len() <= 160);
                eprintln!("{reason}");
            }
        }
    }

    #[test]
    fn real_current_host_sid_is_rejected_as_worker_account() {
        let host = sol_worker_staging_security::current_host_sid().expect("real host SID");
        let (name, domain, _) = reverse_lookup_numeric_sid(&host).expect("reverse lookup host");
        assert!(!name.is_empty() && !domain.is_empty());
        let (admin_name, _, admin_type) =
            reverse_lookup_numeric_sid(ADMINISTRATORS_SID).expect("reverse lookup Administrators");
        assert!(!admin_name.is_empty());
        assert_eq!(admin_type, SID_TYPE_ALIAS);
        assert!(assess_identity(&host, &host, UF_NORMAL_ACCOUNT, &[], "Administrators").is_err());
    }

    #[test]
    fn bounded_fixture_rejects_missing_terminator_and_outside_pointer() {
        let units = [b'A' as u16; 4];
        let start = units.as_ptr() as usize;
        assert!(bounded_wide_string(units.as_ptr(), start, size_of::<[u16; 4]>()).is_err());
        assert!(bounded_wide_string(start.wrapping_sub(2) as *const u16, start, 8).is_err());
        let mut sid = [0_u8; 8];
        sid[1] = 1;
        let sid_start = sid.as_mut_ptr() as usize;
        assert!(bounded_sid_text(sid.as_mut_ptr().cast(), sid_start, sid.len()).is_err());
        assert!(bounded_tail(usize::MAX - 1, 4, 2).is_err());
        assert!(bounded_tail(8, 4, 5).is_err());
    }

    #[test]
    fn fixture_rejects_administrator_and_non_user_flags() {
        let sid = "S-1-5-21-1-2-3-1001";
        let host = "S-1-5-21-1-2-3-1000";
        assert!(assess_identity(sid, host, UF_NORMAL_ACCOUNT, &[], "Administrators").is_ok());
        assert!(
            assess_identity(
                sid,
                host,
                UF_NORMAL_ACCOUNT,
                &["administrators".into()],
                "Administrators"
            )
            .is_err()
        );
        assert!(
            assess_identity(
                sid,
                host,
                UF_NORMAL_ACCOUNT | UF_ACCOUNTDISABLE,
                &[],
                "Administrators"
            )
            .is_err()
        );
        assert!(assess_identity(sid, host, 0x1000, &[], "Administrators").is_err());
        assert!(
            assess_identity(
                sid,
                host,
                UF_NORMAL_ACCOUNT | UF_TRUSTED_FOR_DELEGATION,
                &[],
                "Administrators"
            )
            .is_err()
        );
        assert!(
            assess_identity(
                sid,
                host,
                UF_NORMAL_ACCOUNT | UF_TRUSTED_TO_AUTHENTICATE_FOR_DELEGATION,
                &[],
                "Administrators"
            )
            .is_err()
        );
        assert!(
            assess_identity(SYSTEM_SID, host, UF_NORMAL_ACCOUNT, &[], "Administrators").is_err()
        );
    }

    #[test]
    fn fixture_rejects_domain_controller_role() {
        assert!(is_local_sam_server_type(0x0000_0001));
        assert!(is_local_sam_server_type(0x0000_0002));
        assert!(!is_local_sam_server_type(0));
        assert!(!is_local_sam_server_type(0x0000_0004));
        assert!(!is_local_sam_server_type(SV_TYPE_DOMAIN_CTRL));
        assert!(!is_local_sam_server_type(SV_TYPE_DOMAIN_BAKCTRL));
    }
}
