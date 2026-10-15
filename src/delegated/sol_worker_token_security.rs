//! Dormant readback of an opened Windows worker process's primary token.
//!
//! This observes the process object supplied by the trusted host; it does not
//! establish that the host launched the intended Codex process, that its
//! descendants exited, or that inherited handles and parent ACLs prevent
//! writes. No live Sol launch path calls this module.

#![cfg(windows)]

use std::ffi::c_void;
use std::mem::{offset_of, size_of};
use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::ptr;

const LABEL: &str = "Sol worker token security";
const TOKEN_QUERY: u32 = 0x0008;
const TOKEN_USER: u32 = 1;
const TOKEN_GROUPS: u32 = 2;
const TOKEN_PRIVILEGES: u32 = 3;
const TOKEN_STATISTICS: u32 = 10;
const TOKEN_RESTRICTED_SIDS: u32 = 11;
const TOKEN_HAS_RESTRICTIONS: u32 = 21;
const TOKEN_INTEGRITY_LEVEL: u32 = 25;
const TOKEN_MANDATORY_POLICY: u32 = 27;
const TOKEN_PRIMARY: u32 = 1;
const LOW_INTEGRITY_RID: u32 = 0x1000;
const MANDATORY_NO_WRITE_UP: u32 = 0x1;
const DUPLICATE_SAME_ACCESS: u32 = 0x2;
const SE_GROUP_ENABLED: u32 = 0x4;
const SE_GROUP_USE_FOR_DENY_ONLY: u32 = 0x10;
const MAX_TOKEN_INFORMATION: u32 = 64 * 1024;
const MAX_SIDS: usize = 256;
const MAX_PRIVILEGES: usize = 256;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Luid {
    low: u32,
    high: i32,
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
    first: SidAndAttributes,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LuidAndAttributes {
    luid: Luid,
    attributes: u32,
}

#[repr(C)]
struct TokenPrivilegesOne {
    count: u32,
    first: LuidAndAttributes,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TokenStatistics {
    token_id: Luid,
    authentication_id: Luid,
    expiration_time: i64,
    token_type: u32,
    impersonation_level: u32,
    dynamic_charged: u32,
    dynamic_available: u32,
    group_count: u32,
    privilege_count: u32,
    modified_id: Luid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TokenGroupV1 {
    pub sid: String,
    pub attributes: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TokenSnapshot {
    token_id: Luid,
    modified_id: Luid,
    user_sid: String,
    groups: Vec<TokenGroupV1>,
    restricting_sids: Vec<TokenGroupV1>,
    privileges: Vec<LuidAndAttributes>,
    has_restrictions: bool,
    integrity_rid: u32,
    mandatory_policy: u32,
}

/// A pinned process and token readback. Private fields prevent caller-created
/// identity claims. The snapshot is rechecked against both opened handles
/// before a staging ACL may use it.
pub(crate) struct ObservedRestrictedWorkerProcessTokenV1 {
    process: OwnedHandle,
    token: OwnedHandle,
    process_id: u32,
    snapshot: TokenSnapshot,
}

impl ObservedRestrictedWorkerProcessTokenV1 {
    pub(crate) fn process_id(&self) -> u32 {
        self.process_id
    }

    pub(crate) fn token_user_sid(&self) -> &str {
        &self.snapshot.user_sid
    }

    pub(crate) fn enabled_groups(&self) -> impl Iterator<Item = &TokenGroupV1> {
        self.snapshot
            .groups
            .iter()
            .filter(|group| group.attributes & SE_GROUP_ENABLED != 0)
    }

    pub(crate) fn integrity_rid(&self) -> u32 {
        self.snapshot.integrity_rid
    }

    pub(crate) fn revalidate(&self) -> Result<(), String> {
        if unsafe { GetProcessId(self.process.as_raw_handle()) } != self.process_id {
            return Err(format!("{LABEL} opened process identity changed"));
        }
        let fresh_token = open_process_token(&self.process)?;
        let fresh = read_snapshot(&fresh_token)?;
        let original = read_snapshot(&self.token)?;
        if fresh != self.snapshot || original != self.snapshot {
            return Err(format!("{LABEL} process token changed"));
        }
        enforce_restricted_policy(&fresh)
    }

    /// Reject every token group that could become an ACL trustee, including
    /// disabled groups that a later token adjustment might enable. Restricting
    /// SIDs are included conservatively.
    pub(crate) fn reject_stage_acl_trustees(&self, trustees: &[&str]) -> Result<(), String> {
        self.revalidate()?;
        reject_snapshot_stage_acl_trustees(&self.snapshot, trustees)
    }
}

fn reject_snapshot_stage_acl_trustees(
    snapshot: &TokenSnapshot,
    trustees: &[&str],
) -> Result<(), String> {
    for trustee in trustees {
        if snapshot.user_sid == *trustee
            || snapshot
                .groups
                .iter()
                .chain(snapshot.restricting_sids.iter())
                .any(|group| group.sid == *trustee)
        {
            return Err(format!("{LABEL} worker identity matches stage ACL trustee"));
        }
    }
    Ok(())
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
    fn IsValidSid(sid: *mut c_void) -> i32;
    fn GetLengthSid(sid: *mut c_void) -> u32;
    fn ConvertSidToStringSidW(sid: *mut c_void, text: *mut *mut u16) -> i32;
    fn LookupPrivilegeValueW(system: *const u16, name: *const u16, luid: *mut Luid) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn GetProcessId(process: *mut c_void) -> u32;
    fn DuplicateHandle(
        source_process: *mut c_void,
        source: *mut c_void,
        target_process: *mut c_void,
        target: *mut *mut c_void,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

fn owned_handle(raw: *mut c_void) -> Result<OwnedHandle, String> {
    if raw.is_null() || raw as isize == -1 {
        return Err(format!("{LABEL} invalid opened handle"));
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

fn open_process_token(process: &OwnedHandle) -> Result<OwnedHandle, String> {
    let mut raw = ptr::null_mut();
    if unsafe { OpenProcessToken(process.as_raw_handle(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(format!("{LABEL} process token unavailable"));
    }
    owned_handle(raw)
}

/// The host must supply a live borrowed handle to the intended worker process
/// with PROCESS_QUERY_LIMITED_INFORMATION. This observes its actual primary
/// token, but the caller still must bind the process handle to its launch and
/// prove no descendant or inherited handle retains write authority.
pub(crate) fn observe_restricted_worker_process_token(
    process: BorrowedHandle<'_>,
) -> Result<ObservedRestrictedWorkerProcessTokenV1, String> {
    let mut raw = ptr::null_mut();
    if unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            process.as_raw_handle(),
            GetCurrentProcess(),
            &mut raw,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(format!("{LABEL} process handle duplication failed"));
    }
    let process = owned_handle(raw)?;
    let process_id = unsafe { GetProcessId(process.as_raw_handle()) };
    if process_id == 0 {
        return Err(format!("{LABEL} opened handle is not a process"));
    }
    let token = open_process_token(&process)?;
    let snapshot = read_snapshot(&token)?;
    enforce_restricted_policy(&snapshot)?;
    let observed = ObservedRestrictedWorkerProcessTokenV1 {
        process,
        token,
        process_id,
        snapshot,
    };
    observed.revalidate()?;
    Ok(observed)
}

fn token_information(token: &OwnedHandle, class: u32, minimum: usize) -> Result<Vec<u8>, String> {
    let mut size = 0_u32;
    unsafe { GetTokenInformation(token.as_raw_handle(), class, ptr::null_mut(), 0, &mut size) };
    if size < minimum as u32 || size > MAX_TOKEN_INFORMATION {
        return Err(format!("{LABEL} token class {class} has invalid length"));
    }
    let mut bytes = vec![0_u8; size as usize];
    let mut returned = 0_u32;
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            bytes.as_mut_ptr().cast(),
            size,
            &mut returned,
        )
    } == 0
        || returned != size
    {
        return Err(format!("{LABEL} token class {class} readback failed"));
    }
    Ok(bytes)
}

fn decode_has_restrictions(bytes: [u8; 4], returned: u32) -> Result<bool, String> {
    // The published class-21 contract says DWORD, while current Windows can
    // report and write one byte. The caller always supplies four zeroed bytes.
    match returned {
        1 if bytes[1..] == [0, 0, 0] => Ok(bytes[0] != 0),
        4 => Ok(u32::from_ne_bytes(bytes) != 0),
        _ => Err(format!("{LABEL} TokenHasRestrictions length invalid")),
    }
}

fn read_has_restrictions(token: &OwnedHandle) -> Result<bool, String> {
    let mut bytes = [0_u8; 4];
    let mut returned = 0_u32;
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
        return Err(format!("{LABEL} TokenHasRestrictions unavailable"));
    }
    decode_has_restrictions(bytes, returned)
}

fn bounded_sid(sid: *mut c_void, bytes: &[u8], minimum_offset: usize) -> Result<String, String> {
    let start = bytes.as_ptr() as usize;
    let end = start
        .checked_add(bytes.len())
        .ok_or_else(|| format!("{LABEL} SID buffer overflow"))?;
    let sid_start = sid as usize;
    let first_sid_byte = start
        .checked_add(minimum_offset)
        .ok_or_else(|| format!("{LABEL} SID buffer overflow"))?;
    if sid_start < first_sid_byte || sid_start.checked_add(8).is_none_or(|x| x > end) {
        return Err(format!("{LABEL} SID outside token buffer"));
    }
    let count = unsafe { *sid.cast::<u8>().add(1) } as usize;
    let length = 8_usize
        .checked_add(
            4_usize
                .checked_mul(count)
                .ok_or_else(|| format!("{LABEL} SID invalid"))?,
        )
        .ok_or_else(|| format!("{LABEL} SID invalid"))?;
    if sid_start.checked_add(length).is_none_or(|x| x > end)
        || unsafe { IsValidSid(sid) } == 0
        || unsafe { GetLengthSid(sid) } as usize != length
    {
        return Err(format!("{LABEL} SID invalid"));
    }
    let mut text = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
        return Err(format!("{LABEL} SID conversion failed"));
    }
    let mut units = 0usize;
    while units < 256 && unsafe { *text.add(units) } != 0 {
        units += 1;
    }
    let result = if units == 0 || units == 256 {
        Err(format!("{LABEL} SID text invalid"))
    } else {
        String::from_utf16(unsafe { std::slice::from_raw_parts(text, units) })
            .map_err(|_| format!("{LABEL} SID text invalid"))
    };
    unsafe { LocalFree(text.cast()) };
    result
}

fn sid_and_attributes_at(bytes: &[u8], offset: usize) -> Result<SidAndAttributes, String> {
    if offset
        .checked_add(size_of::<SidAndAttributes>())
        .is_none_or(|end| end > bytes.len())
    {
        return Err(format!("{LABEL} SID entry truncated"));
    }
    Ok(unsafe { ptr::read_unaligned(bytes.as_ptr().add(offset).cast::<SidAndAttributes>()) })
}

fn read_user(token: &OwnedHandle) -> Result<String, String> {
    let bytes = token_information(token, TOKEN_USER, size_of::<SidAndAttributes>())?;
    let user = sid_and_attributes_at(&bytes, 0)?;
    bounded_sid(user.sid, &bytes, size_of::<SidAndAttributes>())
}

fn parse_groups(bytes: &[u8]) -> Result<Vec<TokenGroupV1>, String> {
    if bytes.len() < size_of::<u32>() {
        return Err(format!("{LABEL} group count missing"));
    }
    let count = unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<u32>()) } as usize;
    if count > MAX_SIDS {
        return Err(format!("{LABEL} too many token SIDs"));
    }
    let offset = offset_of!(TokenGroupsOne, first);
    let records_end = offset
        .checked_add(
            count
                .checked_mul(size_of::<SidAndAttributes>())
                .ok_or_else(|| format!("{LABEL} group count invalid"))?,
        )
        .ok_or_else(|| format!("{LABEL} group count invalid"))?;
    if count > 0 && records_end > bytes.len() {
        return Err(format!("{LABEL} group records truncated"));
    }
    let mut groups = Vec::with_capacity(count);
    for index in 0..count {
        let entry = sid_and_attributes_at(bytes, offset + index * size_of::<SidAndAttributes>())?;
        if entry.attributes & SE_GROUP_ENABLED != 0
            && entry.attributes & SE_GROUP_USE_FOR_DENY_ONLY != 0
        {
            return Err(format!("{LABEL} contradictory group attributes"));
        }
        groups.push(TokenGroupV1 {
            sid: bounded_sid(entry.sid, bytes, records_end)?,
            attributes: entry.attributes,
        });
    }
    Ok(groups)
}

fn parse_privileges(bytes: &[u8]) -> Result<Vec<LuidAndAttributes>, String> {
    if bytes.len() < size_of::<u32>() {
        return Err(format!("{LABEL} privilege count missing"));
    }
    let count = unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<u32>()) } as usize;
    if count > MAX_PRIVILEGES {
        return Err(format!("{LABEL} too many privileges"));
    }
    let offset = offset_of!(TokenPrivilegesOne, first);
    let end = offset
        .checked_add(
            count
                .checked_mul(size_of::<LuidAndAttributes>())
                .ok_or_else(|| format!("{LABEL} privilege count invalid"))?,
        )
        .ok_or_else(|| format!("{LABEL} privilege count invalid"))?;
    if count > 0 && end > bytes.len() {
        return Err(format!("{LABEL} privilege records truncated"));
    }
    let mut privileges = Vec::with_capacity(count);
    for index in 0..count {
        privileges.push(unsafe {
            ptr::read_unaligned(
                bytes
                    .as_ptr()
                    .add(offset + index * size_of::<LuidAndAttributes>())
                    .cast::<LuidAndAttributes>(),
            )
        });
    }
    Ok(privileges)
}

fn read_statistics(token: &OwnedHandle) -> Result<TokenStatistics, String> {
    let bytes = token_information(token, TOKEN_STATISTICS, size_of::<TokenStatistics>())?;
    Ok(unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<TokenStatistics>()) })
}

fn read_integrity(token: &OwnedHandle) -> Result<u32, String> {
    let bytes = token_information(token, TOKEN_INTEGRITY_LEVEL, size_of::<SidAndAttributes>())?;
    let label = sid_and_attributes_at(&bytes, 0)?;
    let sid = bounded_sid(label.sid, &bytes, size_of::<SidAndAttributes>())?;
    sid.strip_prefix("S-1-16-")
        .and_then(|rid| rid.parse::<u32>().ok())
        .ok_or_else(|| format!("{LABEL} integrity SID invalid"))
}

fn read_snapshot(token: &OwnedHandle) -> Result<TokenSnapshot, String> {
    let before = read_statistics(token)?;
    if before.token_type != TOKEN_PRIMARY {
        return Err(format!("{LABEL} process token is not primary"));
    }
    let user_sid = read_user(token)?;
    let groups = parse_groups(&token_information(token, TOKEN_GROUPS, 4)?)?;
    let restricting_sids = parse_groups(&token_information(token, TOKEN_RESTRICTED_SIDS, 4)?)?;
    let privileges = parse_privileges(&token_information(token, TOKEN_PRIVILEGES, 4)?)?;
    let has_restrictions = read_has_restrictions(token)?;
    let integrity_rid = read_integrity(token)?;
    let policy_bytes = token_information(token, TOKEN_MANDATORY_POLICY, 4)?;
    let mandatory_policy = unsafe { ptr::read_unaligned(policy_bytes.as_ptr().cast::<u32>()) };
    let after = read_statistics(token)?;
    if before.token_id != after.token_id
        || before.modified_id != after.modified_id
        || before.group_count as usize != groups.len()
        || before.privilege_count as usize != privileges.len()
    {
        return Err(format!("{LABEL} token changed during inspection"));
    }
    Ok(TokenSnapshot {
        token_id: before.token_id,
        modified_id: before.modified_id,
        user_sid,
        groups,
        restricting_sids,
        privileges,
        has_restrictions,
        integrity_rid,
        mandatory_policy,
    })
}

fn change_notify_luid() -> Result<Luid, String> {
    let name = "SeChangeNotifyPrivilege"
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut luid = Luid { low: 0, high: 0 };
    if unsafe { LookupPrivilegeValueW(ptr::null(), name.as_ptr(), &mut luid) } == 0 {
        return Err(format!("{LABEL} privilege lookup failed"));
    }
    Ok(luid)
}

fn enforce_restricted_policy(snapshot: &TokenSnapshot) -> Result<(), String> {
    if !snapshot.has_restrictions || snapshot.restricting_sids.is_empty() {
        return Err(format!("{LABEL} token has no restricting SIDs"));
    }
    if snapshot.integrity_rid > LOW_INTEGRITY_RID
        || snapshot.mandatory_policy & MANDATORY_NO_WRITE_UP == 0
    {
        return Err(format!("{LABEL} token integrity is too broad"));
    }
    let allowed = change_notify_luid()?;
    if snapshot
        .privileges
        .iter()
        .any(|entry| entry.luid != allowed)
    {
        return Err(format!("{LABEL} token retains a non-allowlisted privilege"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN_DUPLICATE: u32 = 0x0002;
    const TOKEN_ADJUST_DEFAULT: u32 = 0x0080;
    const DISABLE_MAX_PRIVILEGE: u32 = 0x1;
    const SE_GROUP_INTEGRITY: u32 = 0x20;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn CreateRestrictedToken(
            existing: *mut c_void,
            flags: u32,
            disable_count: u32,
            disable: *const SidAndAttributes,
            delete_count: u32,
            delete: *const LuidAndAttributes,
            restrict_count: u32,
            restrict: *const SidAndAttributes,
            new: *mut *mut c_void,
        ) -> i32;
        fn SetTokenInformation(
            token: *mut c_void,
            class: u32,
            info: *mut c_void,
            length: u32,
        ) -> i32;
        fn ConvertStringSidToSidW(text: *const u16, sid: *mut *mut c_void) -> i32;
    }

    struct TestSid(*mut c_void);
    impl Drop for TestSid {
        fn drop(&mut self) {
            unsafe { LocalFree(self.0) };
        }
    }

    fn test_sid(value: &str) -> TestSid {
        let text = value.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut sid = ptr::null_mut();
        assert_ne!(
            unsafe { ConvertStringSidToSidW(text.as_ptr(), &mut sid) },
            0
        );
        assert!(!sid.is_null());
        TestSid(sid)
    }

    #[test]
    fn real_low_restricted_token_has_expected_os_fields() {
        let mut raw = ptr::null_mut();
        assert_ne!(
            unsafe {
                OpenProcessToken(
                    GetCurrentProcess(),
                    TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ADJUST_DEFAULT,
                    &mut raw,
                )
            },
            0
        );
        let host = owned_handle(raw).expect("host token handle");
        let everyone = test_sid("S-1-1-0");
        let restricting = SidAndAttributes {
            sid: everyone.0,
            attributes: 0,
        };
        let mut restricted_raw = ptr::null_mut();
        assert_ne!(
            unsafe {
                CreateRestrictedToken(
                    host.as_raw_handle(),
                    DISABLE_MAX_PRIVILEGE,
                    0,
                    ptr::null(),
                    0,
                    ptr::null(),
                    1,
                    &restricting,
                    &mut restricted_raw,
                )
            },
            0
        );
        let restricted = owned_handle(restricted_raw).expect("restricted token handle");
        let low = test_sid("S-1-16-4096");
        let mut label = SidAndAttributes {
            sid: low.0,
            attributes: SE_GROUP_INTEGRITY,
        };
        assert_ne!(
            unsafe {
                SetTokenInformation(
                    restricted.as_raw_handle(),
                    TOKEN_INTEGRITY_LEVEL,
                    (&mut label as *mut SidAndAttributes).cast(),
                    (size_of::<SidAndAttributes>() as u32) + GetLengthSid(low.0),
                )
            },
            0
        );
        let snapshot = read_snapshot(&restricted).expect("read real restricted token");
        assert!(snapshot.has_restrictions);
        assert_eq!(snapshot.restricting_sids.len(), 1);
        assert_eq!(snapshot.integrity_rid, LOW_INTEGRITY_RID);
        enforce_restricted_policy(&snapshot).expect("restricted token policy");
        let host_sid = read_snapshot(&host).expect("host identity").user_sid;
        assert!(reject_snapshot_stage_acl_trustees(&snapshot, &[&host_sid]).is_err());
    }

    #[test]
    fn current_process_is_not_a_restricted_worker_process() {
        let process = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess()) };
        assert!(observe_restricted_worker_process_token(process).is_err());
    }

    #[test]
    fn actual_host_token_fields_are_derived_but_not_accepted_as_worker() {
        let process = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess()) };
        let mut raw = ptr::null_mut();
        assert_ne!(
            unsafe { OpenProcessToken(process.as_raw_handle(), TOKEN_QUERY, &mut raw) },
            0
        );
        let token = owned_handle(raw).expect("host token handle");
        let observed = read_snapshot(&token).expect("derive actual token fields");
        assert!(observed.user_sid.starts_with("S-1-"));
        assert!(!observed.groups.is_empty());
        assert!(observed.integrity_rid >= LOW_INTEGRITY_RID);
        assert!(enforce_restricted_policy(&observed).is_err());
        let token_as_process = unsafe { BorrowedHandle::borrow_raw(token.as_raw_handle()) };
        assert!(observe_restricted_worker_process_token(token_as_process).is_err());
    }

    #[test]
    fn invalid_group_and_sid_bounds_fail_before_win32_sid_calls() {
        let mut bytes = [0_u8; 32];
        bytes[..4].copy_from_slice(&1_u32.to_ne_bytes());
        assert!(parse_groups(&bytes).is_err());
        let sid = unsafe { bytes.as_mut_ptr().add(bytes.len() - 4) }.cast();
        assert!(bounded_sid(sid, &bytes, 8).is_err());
    }

    #[test]
    fn extra_privilege_and_missing_restriction_fail_policy() {
        let snapshot = TokenSnapshot {
            token_id: Luid { low: 1, high: 0 },
            modified_id: Luid { low: 1, high: 0 },
            user_sid: "S-1-5-21-1".into(),
            groups: Vec::new(),
            restricting_sids: Vec::new(),
            privileges: Vec::new(),
            has_restrictions: false,
            integrity_rid: LOW_INTEGRITY_RID,
            mandatory_policy: MANDATORY_NO_WRITE_UP,
        };
        assert!(enforce_restricted_policy(&snapshot).is_err());
        let mut changed = snapshot;
        changed.restricting_sids.push(TokenGroupV1 {
            sid: "S-1-1-0".into(),
            attributes: SE_GROUP_ENABLED,
        });
        assert!(enforce_restricted_policy(&changed).is_err());
        changed.has_restrictions = true;
        changed.privileges.push(LuidAndAttributes {
            luid: Luid {
                low: u32::MAX,
                high: i32::MAX,
            },
            attributes: 0,
        });
        assert!(enforce_restricted_policy(&changed).is_err());
        changed.privileges.clear();
        changed.integrity_rid = LOW_INTEGRITY_RID + 1;
        assert!(enforce_restricted_policy(&changed).is_err());
        changed.integrity_rid = LOW_INTEGRITY_RID;
        changed.mandatory_policy = 0;
        assert!(enforce_restricted_policy(&changed).is_err());
    }

    #[test]
    fn token_has_restrictions_readback_accepts_only_exact_boolean_shapes() {
        assert!(decode_has_restrictions([1, 0, 0, 0], 1).unwrap());
        assert!(decode_has_restrictions([1, 0, 0, 0], 4).unwrap());
        assert!(!decode_has_restrictions([0, 0, 0, 0], 1).unwrap());
        assert!(decode_has_restrictions([1, 1, 0, 0], 1).is_err());
        assert!(decode_has_restrictions([1, 0, 0, 0], 2).is_err());
        assert!(decode_has_restrictions([1, 0, 0, 0], 0).is_err());
    }

    #[test]
    fn all_worker_group_states_are_rejected_as_stage_trustees() {
        let mut snapshot = TokenSnapshot {
            token_id: Luid { low: 1, high: 0 },
            modified_id: Luid { low: 1, high: 0 },
            user_sid: "S-1-5-21-1".into(),
            groups: vec![TokenGroupV1 {
                sid: "S-1-5-21-2".into(),
                attributes: 0,
            }],
            restricting_sids: vec![TokenGroupV1 {
                sid: "S-1-1-0".into(),
                attributes: SE_GROUP_ENABLED,
            }],
            privileges: Vec::new(),
            has_restrictions: true,
            integrity_rid: LOW_INTEGRITY_RID,
            mandatory_policy: MANDATORY_NO_WRITE_UP,
        };
        assert!(reject_snapshot_stage_acl_trustees(&snapshot, &["S-1-5-21-1"]).is_err());
        assert!(reject_snapshot_stage_acl_trustees(&snapshot, &["S-1-5-21-2"]).is_err());
        assert!(reject_snapshot_stage_acl_trustees(&snapshot, &["S-1-1-0"]).is_err());
        assert!(reject_snapshot_stage_acl_trustees(&snapshot, &["S-1-5-18"]).is_ok());
        snapshot.groups[0].attributes = SE_GROUP_USE_FOR_DENY_ONLY;
        assert!(reject_snapshot_stage_acl_trustees(&snapshot, &["S-1-5-21-2"]).is_err());
    }
}
