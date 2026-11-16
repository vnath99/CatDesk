//! Ephemeral read-only attestation access for the enrolled local updater.
//! Only the fixed restricted SYSTEM service may publish these two grants.
//! No request supplies an object handle, SID, access mask or security document.

use crate::{
    Result, install_broker_client::require_fixed_service_process_identity,
    install_broker_policy::BrokerProtectedPolicyV1,
    install_broker_preflight::fixed_broker_service_sid_for_endpoint,
};
use std::{
    ffi::{OsStr, c_void},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr,
};

const PROCESS_READBACK: u32 = 0x0010_1000; // SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION
const TOKEN_QUERY: u32 = 8;
const READ_CONTROL: u32 = 0x0002_0000;
const WRITE_DAC: u32 = 0x0004_0000;
const SE_KERNEL_OBJECT: u32 = 6;
const DACL_INFORMATION: u32 = 4;
const OWNER_GROUP_DACL_INFORMATION: u32 = 7;
const PROTECTED_DACL_INFORMATION: u32 = 0x8000_0000;
const SE_DACL_PRESENT: u16 = 4;
const SE_DACL_PROTECTED: u16 = 0x1000;
const MAX_SECURITY_BYTES: usize = 64 * 1024;
const MAX_ACES: usize = 256;
const ERROR: &str = "WAKE_LOCAL_ATTESTATION_ACCESS_INVALID";

#[repr(C)]
struct TrusteeW {
    multiple_trustee: *mut c_void,
    multiple_operation: u32,
    form: u32,
    trustee_type: u32,
    name: *mut u16,
}

#[repr(C)]
struct ExplicitAccessW {
    permissions: u32,
    mode: u32,
    inheritance: u32,
    trustee: TrusteeW,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn DuplicateHandle(
        source_process: *mut c_void,
        source: *mut c_void,
        target_process: *mut c_void,
        target: *mut *mut c_void,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    fn LocalFree(value: *mut c_void) -> *mut c_void;
    fn LocalSize(value: *mut c_void) -> usize;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
    fn ConvertStringSidToSidW(value: *const u16, sid: *mut *mut c_void) -> i32;
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
    fn SetSecurityInfo(
        handle: *mut c_void,
        object_type: u32,
        information: u32,
        owner: *mut c_void,
        group: *mut c_void,
        dacl: *mut c_void,
        sacl: *mut c_void,
    ) -> u32;
    fn GetSecurityDescriptorLength(descriptor: *mut c_void) -> u32;
    fn GetSecurityDescriptorControl(
        descriptor: *mut c_void,
        control: *mut u16,
        revision: *mut u32,
    ) -> i32;
    fn SetEntriesInAclW(
        count: u32,
        entries: *const ExplicitAccessW,
        old_acl: *mut c_void,
        new_acl: *mut *mut c_void,
    ) -> u32;
}

struct LocalAllocation(*mut c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0) };
        }
    }
}

fn owned(raw: *mut c_void) -> Result<OwnedHandle> {
    if raw.is_null() || raw == -1isize as *mut c_void {
        Err(ERROR.into())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
    }
}

fn user_sid(value: &str) -> Result<LocalAllocation> {
    let components = value.split('-').collect::<Vec<_>>();
    if components.len() != 8
        || components[..4] != ["S", "1", "5", "21"]
        || components[4..].iter().any(|part| {
            part.is_empty()
                || part.len() > 10
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || part.parse::<u32>().is_err()
        })
    {
        return Err(ERROR.into());
    }
    let wide = OsStr::new(value)
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let mut raw = ptr::null_mut();
    let status = unsafe { ConvertStringSidToSidW(wide.as_ptr(), &mut raw) };
    let allocation = LocalAllocation(raw);
    if status == 0 || allocation.0.is_null() {
        return Err(ERROR.into());
    }
    Ok(allocation)
}

fn sid_bytes(bytes: &[u8]) -> Result<&[u8]> {
    let head = bytes.get(..8).ok_or(ERROR)?;
    let count = usize::from(head[1]);
    if head[0] != 1 || count == 0 || count > 15 {
        return Err(ERROR.into());
    }
    bytes.get(..8 + count * 4).ok_or_else(|| ERROR.into())
}

fn allocation_bytes(allocation: &LocalAllocation) -> Result<&[u8]> {
    let length = unsafe { LocalSize(allocation.0) };
    if allocation.0.is_null() || !(8..=MAX_SECURITY_BYTES).contains(&length) {
        return Err(ERROR.into());
    }
    Ok(unsafe { std::slice::from_raw_parts(allocation.0.cast::<u8>(), length) })
}

fn bounded_part(allocation: &[u8], raw: *mut c_void) -> Result<&[u8]> {
    let offset = raw
        .addr()
        .checked_sub(allocation.as_ptr().addr())
        .ok_or(ERROR)?;
    allocation.get(offset..).ok_or_else(|| ERROR.into())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SecuritySnapshot {
    owner: Vec<u8>,
    group: Option<Vec<u8>>,
    control: u16,
    acl: Vec<u8>,
}

fn acl_bytes(bytes: &[u8]) -> Result<&[u8]> {
    let head = bytes.get(..8).ok_or(ERROR)?;
    let length = usize::from(u16::from_le_bytes([head[2], head[3]]));
    if length < 8 || !length.is_multiple_of(4) {
        return Err(ERROR.into());
    }
    bytes.get(..length).ok_or_else(|| ERROR.into())
}

fn snapshot(handle: &OwnedHandle) -> Result<SecuritySnapshot> {
    let mut owner = ptr::null_mut();
    let mut group = ptr::null_mut();
    let mut dacl = ptr::null_mut();
    let mut descriptor = ptr::null_mut();
    let status = unsafe {
        GetSecurityInfo(
            handle.as_raw_handle(),
            SE_KERNEL_OBJECT,
            OWNER_GROUP_DACL_INFORMATION,
            &mut owner,
            &mut group,
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    let allocation = LocalAllocation(descriptor);
    #[cfg(test)]
    eprintln!(
        "LOCAL_ACCESS_SECURITY_METADATA status={status} ownerPresent={} groupPresent={} daclPresent={} descriptorPresent={} allocationBytes={}",
        !owner.is_null(),
        !group.is_null(),
        !dacl.is_null(),
        !allocation.0.is_null(),
        if allocation.0.is_null() {
            0
        } else {
            unsafe { LocalSize(allocation.0) }
        },
    );
    if status != 0 || owner.is_null() || dacl.is_null() {
        return Err(ERROR.into());
    }
    let allocated = allocation_bytes(&allocation)?;
    let length = unsafe { GetSecurityDescriptorLength(allocation.0) } as usize;
    let bytes = allocated
        .get(..length)
        .filter(|bytes| bytes.len() >= 20)
        .ok_or(ERROR)?;
    let mut control = 0;
    let mut revision = 0;
    if unsafe { GetSecurityDescriptorControl(allocation.0, &mut control, &mut revision) } == 0
        || revision != 1
        || control & SE_DACL_PRESENT == 0
    {
        return Err(ERROR.into());
    }
    Ok(SecuritySnapshot {
        owner: sid_bytes(bounded_part(bytes, owner)?)?.to_vec(),
        // A valid kernel descriptor may omit its primary group. Preserve that
        // absence distinctly; a present group still requires the full bounds.
        group: if group.is_null() {
            None
        } else {
            Some(sid_bytes(bounded_part(bytes, group)?)?.to_vec())
        },
        control,
        acl: acl_bytes(bounded_part(bytes, dacl)?)?.to_vec(),
    })
}

fn aces(acl: &[u8]) -> Result<Vec<&[u8]>> {
    if acl_bytes(acl)? != acl || !matches!(acl[0], 2 | 4) {
        return Err(ERROR.into());
    }
    let count = usize::from(u16::from_le_bytes([acl[4], acl[5]]));
    if count > MAX_ACES {
        return Err(ERROR.into());
    }
    let mut offset = 8usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let header = acl
            .get(offset..offset.checked_add(4).ok_or(ERROR)?)
            .ok_or(ERROR)?;
        let length = usize::from(u16::from_le_bytes([header[2], header[3]]));
        if length < 16 || !length.is_multiple_of(4) || !matches!(header[0], 0 | 1) {
            // Refuse unfamiliar object/callback ACL geometry before changing it.
            return Err(ERROR.into());
        }
        let entry = acl
            .get(offset..offset.checked_add(length).ok_or(ERROR)?)
            .ok_or(ERROR)?;
        if sid_bytes(&entry[8..])?.len() + 8 != entry.len() {
            return Err(ERROR.into());
        }
        entries.push(entry);
        offset += length;
    }
    Ok(entries)
}

fn check_delta(before: &[u8], after: &[u8], sid: &[u8], permissions: u32) -> Result<()> {
    if !matches!(permissions, PROCESS_READBACK | TOKEN_QUERY) || before.first() != after.first() {
        return Err(ERROR.into());
    }
    let split = |acl| -> Result<(Vec<Vec<u8>>, Vec<u32>)> {
        let mut others = Vec::new();
        let mut target = Vec::new();
        for ace in aces(acl)? {
            if &ace[8..] == sid {
                let mask = u32::from_le_bytes(ace[4..8].try_into().map_err(|_| ERROR)?);
                if ace[0] != 0 || ace[1] != 0 || mask & !permissions != 0 {
                    return Err(ERROR.into());
                }
                target.push(mask);
            } else {
                others.push(ace.to_vec());
            }
        }
        Ok((others, target))
    };
    let (before_others, before_target) = split(before)?;
    let (after_others, after_target) = split(after)?;
    if before_target.len() > 1 || before_others != after_others || after_target != [permissions] {
        return Err(ERROR.into());
    }
    Ok(())
}

struct PreparedGrant {
    before: SecuritySnapshot,
    allocation: LocalAllocation,
    acl: Vec<u8>,
}

fn prepare(handle: &OwnedHandle, sid: &LocalAllocation, permissions: u32) -> Result<PreparedGrant> {
    let before = snapshot(handle)?;
    let target = sid_bytes(allocation_bytes(sid)?)?;
    // Validate the old geometry and target ACEs before invoking the ACL merger.
    // A duplicate, denied, inherited or broader exact-user ACE is not repaired.
    let mut existing_target = 0;
    for ace in aces(&before.acl)? {
        if &ace[8..] == target {
            existing_target += 1;
            let mask = u32::from_le_bytes(ace[4..8].try_into().map_err(|_| ERROR)?);
            if ace[0] != 0 || ace[1] != 0 || mask & !permissions != 0 || existing_target > 1 {
                return Err(ERROR.into());
            }
        }
    }
    let mut old_words = before
        .acl
        .chunks_exact(4)
        .map(|word| u32::from_ne_bytes(word.try_into().expect("four-byte ACL word")))
        .collect::<Vec<_>>();
    let entry = ExplicitAccessW {
        permissions,
        mode: 1, // GRANT_ACCESS, never SET_ACCESS/revoke existing rights.
        inheritance: 0,
        trustee: TrusteeW {
            multiple_trustee: ptr::null_mut(),
            multiple_operation: 0,
            form: 0,         // TRUSTEE_IS_SID
            trustee_type: 1, // TRUSTEE_IS_USER
            name: sid.0.cast(),
        },
    };
    let mut raw = ptr::null_mut();
    let status = unsafe { SetEntriesInAclW(1, &entry, old_words.as_mut_ptr().cast(), &mut raw) };
    let allocation = LocalAllocation(raw);
    if status != 0 {
        return Err(ERROR.into());
    }
    let acl = acl_bytes(allocation_bytes(&allocation)?)?.to_vec();
    check_delta(&before.acl, &acl, target, permissions)?;
    Ok(PreparedGrant {
        before,
        allocation,
        acl,
    })
}

fn apply(handle: &OwnedHandle, prepared: PreparedGrant) -> Result<()> {
    if snapshot(handle)? != prepared.before {
        return Err(ERROR.into());
    }
    let information = DACL_INFORMATION
        | if prepared.before.control & SE_DACL_PROTECTED != 0 {
            PROTECTED_DACL_INFORMATION
        } else {
            0
        };
    // Neither owner/group nor SACL/mandatory labels are ever supplied or set.
    if unsafe {
        SetSecurityInfo(
            handle.as_raw_handle(),
            SE_KERNEL_OBJECT,
            information,
            ptr::null_mut(),
            ptr::null_mut(),
            prepared.allocation.0,
            ptr::null_mut(),
        )
    } != 0
    {
        return Err(ERROR.into());
    }
    let after = snapshot(handle)?;
    if after.owner != prepared.before.owner
        || after.group != prepared.before.group
        || after.control != prepared.before.control
        || after.acl != prepared.acl
    {
        return Err(ERROR.into());
    }
    Ok(())
}

/// Establish only ephemeral observation rights for the protected enrolled user.
/// A partial failure prevents endpoint publication; service exit destroys these
/// process/token objects. This never updates a persistent ACL or caller token.
pub(crate) fn authorize_enrolled_client_readback(policy: &BrokerProtectedPolicyV1) -> Result<()> {
    if !policy.local_user_updates_enabled()? {
        return Err("WAKE_LOCAL_UPDATE_POLICY_REQUIRED".into());
    }
    let (service_sid, enrolled_sid) = policy.checked_endpoint_sids()?;
    if service_sid != fixed_broker_service_sid_for_endpoint()? {
        return Err(ERROR.into());
    }
    let sid = user_sid(enrolled_sid)?;
    let current = unsafe { GetCurrentProcess() };
    let mut raw = ptr::null_mut();
    if unsafe {
        DuplicateHandle(
            current,
            current,
            current,
            &mut raw,
            PROCESS_READBACK | READ_CONTROL | WRITE_DAC,
            0,
            0,
        )
    } == 0
    {
        return Err(ERROR.into());
    }
    let process = owned(raw)?;
    require_fixed_service_process_identity(&process, service_sid)?;
    let mut raw_token = ptr::null_mut();
    if unsafe {
        OpenProcessToken(
            process.as_raw_handle(),
            TOKEN_QUERY | READ_CONTROL | WRITE_DAC,
            &mut raw_token,
        )
    } == 0
    {
        return Err(ERROR.into());
    }
    let token = owned(raw_token)?;
    let process_grant = prepare(&process, &sid, PROCESS_READBACK)?;
    let token_grant = prepare(&token, &sid, TOKEN_QUERY)?;
    policy.revalidate()?;
    require_fixed_service_process_identity(&process, service_sid)?;
    apply(&process, process_grant)?;
    policy.revalidate()?;
    apply(&token, token_grant)?;
    require_fixed_service_process_identity(&process, service_sid)?;
    policy.revalidate()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn DuplicateTokenEx(
            existing: *mut c_void,
            access: u32,
            attributes: *mut c_void,
            impersonation_level: u32,
            token_type: u32,
            token: *mut *mut c_void,
        ) -> i32;
    }

    struct TokenFixture {
        original: OwnedHandle,
        original_security: SecuritySnapshot,
        isolated: OwnedHandle,
    }

    impl TokenFixture {
        fn new() -> Self {
            let mut raw = ptr::null_mut();
            // The original token has no WRITE_DAC handle. Only a fresh,
            // unassigned duplicate is modified; it is never impersonated or
            // assigned to any process/thread and launches no process.
            assert_ne!(
                unsafe {
                    OpenProcessToken(
                        GetCurrentProcess(),
                        TOKEN_QUERY | READ_CONTROL | 2,
                        &mut raw,
                    )
                },
                0
            );
            let original = owned(raw).unwrap();
            let original_security = snapshot(&original).unwrap();
            let mut duplicate = ptr::null_mut();
            assert_ne!(
                unsafe {
                    DuplicateTokenEx(
                        original.as_raw_handle(),
                        TOKEN_QUERY | READ_CONTROL | WRITE_DAC,
                        ptr::null_mut(),
                        2,
                        1,
                        &mut duplicate,
                    )
                },
                0
            );
            Self {
                original,
                original_security,
                isolated: owned(duplicate).unwrap(),
            }
        }

        fn assert_original_unchanged(&self) {
            assert_eq!(snapshot(&self.original).unwrap(), self.original_security);
        }
    }

    #[test]
    fn local_access_disposable_token_checks_exact_acl_geometry_for_both_read_masks() {
        let sid = user_sid("S-1-5-21-11-22-33-1001").unwrap();
        let target = sid_bytes(allocation_bytes(&sid).unwrap()).unwrap();
        // PROCESS_READBACK tests only prepared ACL geometry, without applying
        // process-specific rights to a token object. Actual process-object and
        // medium-user SYSTEM compatibility require separate qualification.
        for mask in [PROCESS_READBACK, TOKEN_QUERY] {
            let fixture = TokenFixture::new();
            let object = &fixture.isolated;
            let before = snapshot(object).unwrap();
            let plan = prepare(object, &sid, mask).unwrap();
            check_delta(&before.acl, &plan.acl, target, mask).unwrap();
            if mask == PROCESS_READBACK {
                assert_eq!(snapshot(object).unwrap(), before);
                fixture.assert_original_unchanged();
                continue;
            }
            apply(object, plan).unwrap();
            let after = snapshot(object).unwrap();
            assert_eq!(before.owner, after.owner);
            assert_eq!(before.group, after.group);
            assert_eq!(before.control, after.control);
            check_delta(&before.acl, &after.acl, target, mask).unwrap();
            apply(object, prepare(object, &sid, mask).unwrap()).unwrap();
            assert_eq!(snapshot(object).unwrap(), after);
            let mut changed_group = prepare(object, &sid, mask).unwrap();
            changed_group.before.group = if after.group.is_none() {
                Some(target.to_vec())
            } else {
                None
            };
            assert!(apply(object, changed_group).is_err());
            assert_eq!(snapshot(object).unwrap(), after);
            fixture.assert_original_unchanged();
        }
    }

    #[test]
    fn local_access_unassigned_duplicate_token_readback_does_not_modify_source_token() {
        let fixture = TokenFixture::new();
        let sid = user_sid("S-1-5-21-11-22-33-1001").unwrap();
        let plan = prepare(&fixture.isolated, &sid, TOKEN_QUERY).unwrap();
        let original_acl = plan.before.acl.clone();
        apply(&fixture.isolated, plan).unwrap();
        check_delta(
            &original_acl,
            &snapshot(&fixture.isolated).unwrap().acl,
            sid_bytes(allocation_bytes(&sid).unwrap()).unwrap(),
            TOKEN_QUERY,
        )
        .unwrap();
        fixture.assert_original_unchanged();
    }

    #[test]
    fn local_access_stale_plan_refuses_before_overwriting_another_owned_acl_change() {
        let fixture = TokenFixture::new();
        let object = &fixture.isolated;
        let first = user_sid("S-1-5-21-11-22-33-1001").unwrap();
        let second = user_sid("S-1-5-21-11-22-33-1002").unwrap();
        let stale = prepare(object, &first, TOKEN_QUERY).unwrap();
        apply(object, prepare(object, &second, TOKEN_QUERY).unwrap()).unwrap();
        let after = snapshot(object).unwrap();
        assert!(apply(object, stale).is_err());
        assert_eq!(snapshot(object).unwrap(), after);
        fixture.assert_original_unchanged();
    }

    #[test]
    fn local_access_refuses_nonuser_sid_and_extra_rights_before_mutation() {
        for value in [
            "S-1-5-18",
            "S-1-5-4",
            "S-1-5-32-544",
            "S-1-5-80-1-2-3-4-5",
            "S-1-5-21-1-2-3-4294967296",
            "S-1-5-21-1-2-3-4\0suffix",
        ] {
            assert!(user_sid(value).is_err());
        }
        let fixture = TokenFixture::new();
        let object = &fixture.isolated;
        let before = snapshot(object).unwrap();
        let sid = user_sid("S-1-5-21-11-22-33-1001").unwrap();
        let target = sid_bytes(allocation_bytes(&sid).unwrap()).unwrap();
        let plan = prepare(object, &sid, TOKEN_QUERY).unwrap();
        let mut bad = plan.acl.clone();
        let target_ace = aces(&bad)
            .unwrap()
            .into_iter()
            .find(|ace| &ace[8..] == target)
            .unwrap();
        let offset = target_ace.as_ptr().addr() - bad.as_ptr().addr();
        bad[offset + 4..offset + 8].copy_from_slice(&(TOKEN_QUERY | 2).to_le_bytes());
        assert!(check_delta(&before.acl, &bad, target, TOKEN_QUERY).is_err());
        assert!(check_delta(&before.acl, &plan.acl, target, TOKEN_QUERY | WRITE_DAC).is_err());
        let wrong = user_sid("S-1-5-21-11-22-33-1002").unwrap();
        assert!(
            check_delta(
                &before.acl,
                &plan.acl,
                sid_bytes(allocation_bytes(&wrong).unwrap()).unwrap(),
                TOKEN_QUERY
            )
            .is_err()
        );
        assert_eq!(snapshot(object).unwrap(), before);
        fixture.assert_original_unchanged();
    }

    #[test]
    fn local_access_refuses_malformed_acl_and_preserved_ace_changes() {
        let fixture = TokenFixture::new();
        let object = &fixture.isolated;
        let before = snapshot(object).unwrap();
        let sid = user_sid("S-1-5-21-11-22-33-1001").unwrap();
        let target = sid_bytes(allocation_bytes(&sid).unwrap()).unwrap();
        let plan = prepare(object, &sid, PROCESS_READBACK).unwrap();
        for variant in 0..4 {
            let mut changed = plan.acl.clone();
            match variant {
                0 => changed.truncate(7),
                1 => changed[4..6].copy_from_slice(&257u16.to_le_bytes()),
                2 => changed[8] = 5,
                _ => {
                    let ace = aces(&changed)
                        .unwrap()
                        .into_iter()
                        .find(|ace| &ace[8..] != target)
                        .unwrap();
                    let offset = ace.as_ptr().addr() - changed.as_ptr().addr();
                    changed[offset + 4] ^= 1;
                }
            }
            assert!(check_delta(&before.acl, &changed, target, PROCESS_READBACK).is_err());
        }
        assert_eq!(snapshot(object).unwrap(), before);
        fixture.assert_original_unchanged();
    }
}
