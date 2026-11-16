//! Dormant Windows-token admission check for a future fixed install-broker pipe.
//!
//! The broker must serialize a read and this check on the *same connected pipe
//! instance*, before decoding or acting on that request: Windows impersonates
//! the sender of the last message read. No other read, disconnect, or reconnect
//! may intervene, and this evidence must never be reused for another request.
//! The expected operator SID
//! must come from independently protected broker policy. A candidate, caller,
//! environment variable or request body must never supply that policy. An OK
//! result is only client-token evidence; the fixed pipe DACL, broker image and
//! namespace, signed approval, ledger and exact transaction remain separate.

use crate::Result;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperatorTokenEvidenceV1 {
    operator_sid_sha256: String,
    integrity_rid: u32,
    session_id: u32,
}

impl OperatorTokenEvidenceV1 {
    pub fn operator_sid_sha256(&self) -> &str {
        &self.operator_sid_sha256
    }

    pub fn integrity_rid(&self) -> u32 {
        self.integrity_rid
    }

    pub fn session_id(&self) -> u32 {
        self.session_id
    }
}

/// Identity evidence for an account delegated by protected local-update policy.
/// This carries an elevated-operator proof only when those stronger checks pass.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(windows)]
pub(crate) struct LocalUserTokenEvidenceV1 {
    user_sid_sha256: String,
    operator: Option<OperatorTokenEvidenceV1>,
}

#[cfg(windows)]
impl LocalUserTokenEvidenceV1 {
    pub(crate) fn user_sid_sha256(&self) -> &str {
        &self.user_sid_sha256
    }

    pub(crate) fn operator(&self) -> Option<&OperatorTokenEvidenceV1> {
        self.operator.as_ref()
    }
}

#[cfg(test)]
pub(crate) fn isolated_same_state_operator_fixture(sid_sha256: &str) -> OperatorTokenEvidenceV1 {
    assert_eq!(sid_sha256.len(), 64);
    OperatorTokenEvidenceV1 {
        operator_sid_sha256: sid_sha256.into(),
        integrity_rid: 0x3000,
        session_id: 1,
    }
}

#[cfg(not(windows))]
pub fn inspect_connected_pipe_operator(
    _pipe: std::os::fd::BorrowedFd<'_>,
    _protected_operator_sid: &[u8],
) -> Result<OperatorTokenEvidenceV1> {
    Err("WAKE_BROKER_WINDOWS_REQUIRED".into())
}

#[cfg(not(windows))]
pub fn inspect_current_process_operator(_: &[u8]) -> Result<OperatorTokenEvidenceV1> {
    Err("WAKE_BROKER_WINDOWS_REQUIRED".into())
}

#[cfg(windows)]
mod windows {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{
        ffi::c_void,
        os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
        ptr,
    };

    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_USER: u32 = 1;
    const TOKEN_GROUPS: u32 = 2;
    const TOKEN_TYPE: u32 = 8;
    const TOKEN_IMPERSONATION_LEVEL: u32 = 9;
    const TOKEN_SESSION_ID: u32 = 12;
    const TOKEN_ELEVATION: u32 = 20;
    const TOKEN_HAS_RESTRICTIONS: u32 = 21;
    const TOKEN_INTEGRITY_LEVEL: u32 = 25;
    const TOKEN_IS_APP_CONTAINER: u32 = 29;
    const ERROR_NO_TOKEN: u32 = 1008;
    const MAX_TOKEN_INFORMATION: usize = 4096;
    const MAX_SID_BYTES: usize = 68;
    const MAX_TOKEN_GROUPS: usize = 256;
    const SE_GROUP_ENABLED: u32 = 0x0000_0004;
    const SE_GROUP_USE_FOR_DENY_ONLY: u32 = 0x0000_0010;
    const INTERACTIVE_SID: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 5, 4, 0, 0, 0];
    const SECURITY_MANDATORY_HIGH_RID: u32 = 0x3000;
    const SECURITY_MANDATORY_MEDIUM_RID: u32 = 0x2000;
    const MANDATORY_LABEL_AUTHORITY: [u8; 6] = [0, 0, 0, 0, 0, 16];
    const LOCAL_SYSTEM_SID: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];

    #[repr(C)]
    struct SidAndAttributes {
        sid: *mut c_void,
        attributes: u32,
    }

    #[repr(C)]
    struct TokenGroupsHeader {
        group_count: u32,
        groups: [SidAndAttributes; 1],
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn ImpersonateNamedPipeClient(pipe: *mut c_void) -> i32;
        fn RevertToSelf() -> i32;
        fn OpenThreadToken(
            thread: *mut c_void,
            access: u32,
            open_as_self: i32,
            token: *mut *mut c_void,
        ) -> i32;
        fn GetTokenInformation(
            token: *mut c_void,
            class: u32,
            buffer: *mut c_void,
            size: u32,
            returned: *mut u32,
        ) -> i32;
        fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
    }

    unsafe extern "system" {
        fn GetCurrentThread() -> *mut c_void;
        fn GetCurrentProcess() -> *mut c_void;
        fn GetLastError() -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    struct RevertGuard;

    impl Drop for RevertGuard {
        fn drop(&mut self) {
            // Continuing a privileged service thread under a client token (or
            // an earlier token) is unsafe. Terminate if Windows cannot revert.
            if unsafe { RevertToSelf() } == 0 {
                std::process::abort();
            }
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct TokenFacts {
        user_sid: Vec<u8>,
        integrity_rid: u32,
        session_id: u32,
        interactive_enabled: bool,
        elevated: bool,
        impersonation_level: Option<u32>,
        app_container: bool,
        restricted: bool,
    }

    mod uac {
        include!("install_broker_operator_uac.rs");
    }

    /// Reads an impersonated pipe-client token, not a process PID or a JSON
    /// identity claim. The caller must retain the same pipe instance for the
    /// full request and must not cache this result for another connection.
    pub fn inspect_connected_pipe_operator(
        pipe: BorrowedHandle<'_>,
        protected_operator_sid: &[u8],
    ) -> Result<OperatorTokenEvidenceV1> {
        inspect_pipe_client(pipe, protected_operator_sid, |_, facts, sid| {
            authorize_facts(facts, sid)
        })
    }

    pub(crate) fn inspect_connected_pipe_local_user(
        pipe: BorrowedHandle<'_>,
        protected_user_sid: &[u8],
    ) -> Result<LocalUserTokenEvidenceV1> {
        inspect_pipe_client(pipe, protected_user_sid, |token, facts, sid| {
            authorize_local_user_token(token, facts, sid, false)
        })
    }

    fn inspect_pipe_client<T>(
        pipe: BorrowedHandle<'_>,
        protected_operator_sid: &[u8],
        authorize: impl FnOnce(&OwnedHandle, &TokenFacts, &[u8]) -> Result<T>,
    ) -> Result<T> {
        valid_sid(protected_operator_sid)?;
        if protected_operator_sid == LOCAL_SYSTEM_SID {
            return Err("WAKE_BROKER_OPERATOR_POLICY_INVALID".into());
        }

        // Never unknowingly restore a prior impersonation context. Service
        // request threads must enter this operation as their own identity.
        let mut prior = ptr::null_mut();
        let had_prior =
            unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut prior) } != 0;
        if had_prior {
            unsafe { CloseHandle(prior) };
            return Err("WAKE_BROKER_THREAD_ALREADY_IMPERSONATING".into());
        }
        if unsafe { GetLastError() } != ERROR_NO_TOKEN {
            return Err("WAKE_BROKER_THREAD_TOKEN_UNAVAILABLE".into());
        }
        if unsafe { ImpersonateNamedPipeClient(pipe.as_raw_handle()) } == 0 {
            return Err("WAKE_BROKER_OPERATOR_IMPERSONATION_FAILED".into());
        }
        let _revert = RevertGuard;
        let mut raw_token = ptr::null_mut();
        if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut raw_token) } == 0
            || raw_token.is_null()
        {
            return Err("WAKE_BROKER_OPERATOR_TOKEN_UNAVAILABLE".into());
        }
        let token = unsafe { OwnedHandle::from_raw_handle(raw_token) };
        let facts = token_facts(token.as_raw_handle(), false)?;
        authorize(&token, &facts, protected_operator_sid)
    }

    /// This grants only current-process identity evidence. It never elevates,
    /// enables privileges, creates a service or authorizes an installation.
    pub fn inspect_current_process_operator(
        protected_operator_sid: &[u8],
    ) -> Result<OperatorTokenEvidenceV1> {
        inspect_current_process(protected_operator_sid, |_, facts, sid| {
            authorize_token_facts(facts, sid, true)
        })
    }

    pub(crate) fn inspect_current_process_local_user(
        protected_user_sid: &[u8],
    ) -> Result<LocalUserTokenEvidenceV1> {
        inspect_current_process(protected_user_sid, |token, facts, sid| {
            authorize_local_user_token(token, facts, sid, true)
        })
    }

    fn inspect_current_process<T>(
        protected_operator_sid: &[u8],
        authorize: impl FnOnce(&OwnedHandle, &TokenFacts, &[u8]) -> Result<T>,
    ) -> Result<T> {
        valid_sid(protected_operator_sid)?;
        let mut prior = ptr::null_mut();
        if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut prior) } != 0 {
            unsafe { CloseHandle(prior) };
            return Err("WAKE_BROKER_THREAD_ALREADY_IMPERSONATING".into());
        }
        if unsafe { GetLastError() } != ERROR_NO_TOKEN {
            return Err("WAKE_BROKER_THREAD_TOKEN_UNAVAILABLE".into());
        }
        let mut raw = ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0
            || raw.is_null()
        {
            return Err("WAKE_BROKER_OPERATOR_TOKEN_UNAVAILABLE".into());
        }
        let token = unsafe { OwnedHandle::from_raw_handle(raw) };
        let facts = token_facts(token.as_raw_handle(), true)?;
        authorize(&token, &facts, protected_operator_sid)
    }

    fn query_token_blob(token: *mut c_void, class: u32) -> Result<Vec<u8>> {
        let mut needed = 0_u32;
        let _size_result =
            unsafe { GetTokenInformation(token, class, ptr::null_mut(), 0, &mut needed) };
        if needed == 0 || needed as usize > MAX_TOKEN_INFORMATION {
            #[cfg(test)]
            uac::diagnostic_query_failure(
                class,
                "SIZE_UNQUALIFIED",
                if _size_result == 0 {
                    unsafe { GetLastError() }
                } else {
                    0
                },
                needed,
                0,
            );
            return Err("WAKE_BROKER_OPERATOR_TOKEN_INVALID".into());
        }
        let mut bytes = vec![0_u8; needed as usize];
        let mut returned = 0_u32;
        let query_result = unsafe {
            GetTokenInformation(
                token,
                class,
                bytes.as_mut_ptr().cast(),
                needed,
                &mut returned,
            )
        };
        if query_result == 0 || returned == 0 || returned > needed {
            #[cfg(test)]
            uac::diagnostic_query_failure(
                class,
                if query_result == 0 {
                    "DATA_QUERY_FAILED"
                } else {
                    "DATA_LENGTH_INVALID"
                },
                if query_result == 0 {
                    unsafe { GetLastError() }
                } else {
                    0
                },
                needed,
                returned,
            );
            return Err("WAKE_BROKER_OPERATOR_TOKEN_INVALID".into());
        }
        bytes.truncate(returned as usize);
        Ok(bytes)
    }

    fn token_u32(token: *mut c_void, class: u32) -> Result<u32> {
        let bytes = query_token_blob(token, class)?;
        let four: [u8; 4] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| "WAKE_BROKER_OPERATOR_TOKEN_INVALID")?;
        Ok(u32::from_le_bytes(four))
    }

    fn token_has_restrictions(token: *mut c_void) -> Result<bool> {
        // Documentation describes a DWORD, but current Windows can report a
        // one-byte ReturnLength for this class. Supply four bytes and accept
        // only those two exact lengths; any nonzero returned value is a denial.
        let mut bytes = [0xa5_u8; 4];
        let mut returned = 0_u32;
        if unsafe {
            GetTokenInformation(
                token,
                TOKEN_HAS_RESTRICTIONS,
                bytes.as_mut_ptr().cast(),
                bytes.len() as u32,
                &mut returned,
            )
        } == 0
        {
            return Err("WAKE_BROKER_OPERATOR_TOKEN_INVALID".into());
        }
        match returned {
            1 => Ok(bytes[0] != 0),
            4 => Ok(u32::from_le_bytes(bytes) != 0),
            _ => Err("WAKE_BROKER_OPERATOR_TOKEN_INVALID".into()),
        }
    }

    fn embedded_sid(bytes: &[u8]) -> Result<&[u8]> {
        if bytes.len() < std::mem::size_of::<SidAndAttributes>() {
            return Err("WAKE_BROKER_OPERATOR_TOKEN_INVALID".into());
        }
        // GetTokenInformation may write this C header into byte-aligned Rust
        // storage. Read it unaligned and prove its pointer is inside that very
        // returned buffer before ever dereferencing a SID byte.
        let header = unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<SidAndAttributes>()) };
        embedded_sid_at(bytes, header.sid, 0)
    }

    fn embedded_sid_at(bytes: &[u8], sid: *mut c_void, minimum_offset: usize) -> Result<&[u8]> {
        let offset = sid
            .addr()
            .checked_sub(bytes.as_ptr().addr())
            .filter(|offset| *offset >= minimum_offset)
            .ok_or("WAKE_BROKER_OPERATOR_TOKEN_INVALID")?;
        let header_end = offset
            .checked_add(8)
            .ok_or("WAKE_BROKER_OPERATOR_TOKEN_INVALID")?;
        let sid_header = bytes
            .get(offset..header_end)
            .ok_or("WAKE_BROKER_OPERATOR_TOKEN_INVALID")?;
        let length = 8 + usize::from(sid_header[1]) * 4;
        if length > MAX_SID_BYTES {
            return Err("WAKE_BROKER_OPERATOR_TOKEN_INVALID".into());
        }
        let sid_end = offset
            .checked_add(length)
            .ok_or("WAKE_BROKER_OPERATOR_TOKEN_INVALID")?;
        let sid = bytes
            .get(offset..sid_end)
            .ok_or("WAKE_BROKER_OPERATOR_TOKEN_INVALID")?;
        valid_sid(sid)?;
        Ok(sid)
    }

    fn interactive_group_enabled(bytes: &[u8]) -> Result<bool> {
        if bytes.len() < std::mem::size_of::<u32>() || bytes.len() > MAX_TOKEN_INFORMATION {
            return Err("WAKE_BROKER_OPERATOR_TOKEN_INVALID".into());
        }
        let count = u32::from_le_bytes(bytes[..4].try_into().expect("four bytes")) as usize;
        if count > MAX_TOKEN_GROUPS {
            return Err("WAKE_BROKER_OPERATOR_TOKEN_INVALID".into());
        }
        if count == 0 {
            return Ok(false);
        }
        // TOKEN_GROUPS has native padding before its SID_AND_ATTRIBUTES array.
        // The byte allocation itself need not be aligned; copy each header
        // unaligned, and resolve all SID pointers against the WHOLE result.
        let start = std::mem::offset_of!(TokenGroupsHeader, groups);
        let stride = std::mem::size_of::<SidAndAttributes>();
        let table_end = count
            .checked_mul(stride)
            .and_then(|length| start.checked_add(length))
            .filter(|end| *end <= bytes.len())
            .ok_or("WAKE_BROKER_OPERATOR_TOKEN_INVALID")?;
        let mut enabled = false;
        for index in 0..count {
            // Checked table_end bounds every entry, without following a raw
            // SID pointer or interpreting byte-aligned storage as a reference.
            let offset = start + index * stride;
            let group = unsafe {
                ptr::read_unaligned(bytes.as_ptr().add(offset).cast::<SidAndAttributes>())
            };
            if group.sid.addr() % std::mem::align_of::<u32>() != 0 {
                return Err("WAKE_BROKER_OPERATOR_TOKEN_INVALID".into());
            }
            let sid = embedded_sid_at(bytes, group.sid, table_end)
                .map_err(|_| "WAKE_BROKER_OPERATOR_TOKEN_INVALID")?;
            if sid == INTERACTIVE_SID
                && group.attributes & SE_GROUP_ENABLED != 0
                && group.attributes & SE_GROUP_USE_FOR_DENY_ONLY == 0
            {
                enabled = true;
            }
        }
        Ok(enabled)
    }

    fn valid_sid(sid: &[u8]) -> Result<()> {
        if sid.len() < 12 || sid.len() > MAX_SID_BYTES || sid[0] != 1 {
            return Err("WAKE_BROKER_OPERATOR_POLICY_INVALID".into());
        }
        let count = usize::from(sid[1]);
        if count == 0 || count > 15 || sid.len() != 8 + count * 4 {
            return Err("WAKE_BROKER_OPERATOR_POLICY_INVALID".into());
        }
        Ok(())
    }

    fn integrity_rid(sid: &[u8]) -> Result<u32> {
        valid_sid(sid)?;
        if sid[2..8] != MANDATORY_LABEL_AUTHORITY {
            return Err("WAKE_BROKER_OPERATOR_TOKEN_INVALID".into());
        }
        let last = &sid[sid.len() - 4..];
        Ok(u32::from_le_bytes(last.try_into().expect("four bytes")))
    }

    fn token_facts(token: *mut c_void, primary: bool) -> Result<TokenFacts> {
        if token_u32(token, TOKEN_TYPE)? != if primary { 1 } else { 2 } {
            return Err("WAKE_BROKER_OPERATOR_TOKEN_INVALID".into());
        }
        let user_sid = embedded_sid(&query_token_blob(token, TOKEN_USER)?)?.to_vec();
        let integrity_rid = integrity_rid(embedded_sid(&query_token_blob(
            token,
            TOKEN_INTEGRITY_LEVEL,
        )?)?)?;
        let session_id = token_u32(token, TOKEN_SESSION_ID)?;
        let interactive_enabled =
            interactive_group_enabled(&query_token_blob(token, TOKEN_GROUPS)?)?;
        let elevated = token_u32(token, TOKEN_ELEVATION)? == 1;
        let impersonation_level = if primary {
            None
        } else {
            Some(token_u32(token, TOKEN_IMPERSONATION_LEVEL)?)
        };
        let app_container = token_u32(token, TOKEN_IS_APP_CONTAINER)? != 0;
        // TokenHasRestrictions also covers disabled SIDs and deleted privileges.
        // IsTokenRestricted returns FALSE on API errors, which would fail open.
        let restricted = token_has_restrictions(token)?;
        Ok(TokenFacts {
            user_sid,
            integrity_rid,
            session_id,
            interactive_enabled,
            elevated,
            impersonation_level,
            app_container,
            restricted,
        })
    }

    fn authorize_facts(
        facts: &TokenFacts,
        protected_operator_sid: &[u8],
    ) -> Result<OperatorTokenEvidenceV1> {
        authorize_token_facts(facts, protected_operator_sid, false)
    }

    fn authorize_token_facts(
        facts: &TokenFacts,
        protected_operator_sid: &[u8],
        primary: bool,
    ) -> Result<OperatorTokenEvidenceV1> {
        require_user_facts(
            facts,
            protected_operator_sid,
            primary,
            SECURITY_MANDATORY_HIGH_RID,
            true,
        )?;
        Ok(OperatorTokenEvidenceV1 {
            operator_sid_sha256: format!("{:x}", Sha256::digest(&facts.user_sid)),
            integrity_rid: facts.integrity_rid,
            session_id: facts.session_id,
        })
    }

    fn authorize_local_user_facts(
        facts: &TokenFacts,
        protected_user_sid: &[u8],
        primary: bool,
    ) -> Result<LocalUserTokenEvidenceV1> {
        require_user_facts(
            facts,
            protected_user_sid,
            primary,
            SECURITY_MANDATORY_MEDIUM_RID,
            false,
        )?;
        Ok(LocalUserTokenEvidenceV1 {
            user_sid_sha256: format!("{:x}", Sha256::digest(&facts.user_sid)),
            operator: authorize_token_facts(facts, protected_user_sid, primary).ok(),
        })
    }

    fn authorize_local_user_token(
        token: &OwnedHandle,
        facts: &TokenFacts,
        protected_user_sid: &[u8],
        primary: bool,
    ) -> Result<LocalUserTokenEvidenceV1> {
        if !facts.restricted {
            return authorize_local_user_facts(facts, protected_user_sid, primary);
        }
        let identity = require_user_identity_facts(
            facts,
            protected_user_sid,
            primary,
            SECURITY_MANDATORY_MEDIUM_RID,
            false,
        );
        #[cfg(test)]
        if identity.is_err() {
            eprintln!(
                "CANONICAL_UAC_ACTOR_IDENTITY sidMatch={} integrityAllowed={} sessionAllowed={} interactiveAllowed={} actorLevelAllowed={} appContainerAllowed={}",
                facts.user_sid == protected_user_sid,
                facts.integrity_rid >= SECURITY_MANDATORY_MEDIUM_RID,
                facts.session_id != 0,
                facts.interactive_enabled,
                if primary {
                    facts.impersonation_level.is_none()
                } else {
                    facts
                        .impersonation_level
                        .is_some_and(|level| (2..=3).contains(&level))
                },
                !facts.app_container
            );
        }
        identity?;
        let _canonical = uac::verify(token, facts)?;
        Ok(LocalUserTokenEvidenceV1 {
            user_sid_sha256: format!("{:x}", Sha256::digest(&facts.user_sid)),
            // Original actor facts only; a filtered caller and metadata-only
            // linked witnesses can never provide elevated operator evidence.
            operator: authorize_token_facts(facts, protected_user_sid, primary).ok(),
        })
    }

    fn require_user_facts(
        facts: &TokenFacts,
        protected_operator_sid: &[u8],
        primary: bool,
        minimum_integrity: u32,
        require_elevation: bool,
    ) -> Result<()> {
        require_user_identity_facts(
            facts,
            protected_operator_sid,
            primary,
            minimum_integrity,
            require_elevation,
        )?;
        if facts.restricted {
            return Err("WAKE_BROKER_OPERATOR_TOKEN_DENIED".into());
        }
        Ok(())
    }

    fn require_user_identity_facts(
        facts: &TokenFacts,
        protected_operator_sid: &[u8],
        primary: bool,
        minimum_integrity: u32,
        require_elevation: bool,
    ) -> Result<()> {
        valid_sid(protected_operator_sid)?;
        if protected_operator_sid == LOCAL_SYSTEM_SID
            || facts.user_sid != protected_operator_sid
            || facts.integrity_rid < minimum_integrity
            || facts.session_id == 0
            || !facts.interactive_enabled
            || (require_elevation && !facts.elevated)
            || if primary {
                facts.impersonation_level.is_some()
            } else {
                !facts
                    .impersonation_level
                    .is_some_and(|level| (2..=3).contains(&level))
            }
            || facts.app_container
        {
            return Err("WAKE_BROKER_OPERATOR_TOKEN_DENIED".into());
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{
            ffi::OsStr,
            os::windows::{ffi::OsStrExt, io::AsHandle},
            sync::atomic::{AtomicU32, Ordering},
        };

        const INVALID_HANDLE: *mut c_void = -1_isize as *mut c_void;
        static NEXT_PIPE: AtomicU32 = AtomicU32::new(1);

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
            fn ConnectNamedPipe(pipe: *mut c_void, overlapped: *mut c_void) -> i32;
            fn CreateFileW(
                path: *const u16,
                access: u32,
                share: u32,
                security: *mut c_void,
                creation: u32,
                flags: u32,
                template: *mut c_void,
            ) -> *mut c_void;
            fn ReadFile(
                file: *mut c_void,
                buffer: *mut c_void,
                len: u32,
                read: *mut u32,
                overlapped: *mut c_void,
            ) -> i32;
            fn WriteFile(
                file: *mut c_void,
                buffer: *const c_void,
                len: u32,
                written: *mut u32,
                overlapped: *mut c_void,
            ) -> i32;
            fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
            fn GetCurrentProcess() -> *mut c_void;
            fn GetCurrentProcessId() -> u32;
        }

        const USER: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0];
        const OTHER: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 5, 22, 0, 0, 0];

        fn allowed() -> TokenFacts {
            TokenFacts {
                user_sid: USER.to_vec(),
                integrity_rid: SECURITY_MANDATORY_HIGH_RID,
                session_id: 1,
                interactive_enabled: true,
                elevated: true,
                impersonation_level: Some(2),
                app_container: false,
                restricted: false,
            }
        }

        pub(super) fn groups_blob(groups: &[(&[u8], u32)], prefix: usize) -> Vec<u8> {
            let table_start = std::mem::offset_of!(TokenGroupsHeader, groups);
            let stride = std::mem::size_of::<SidAndAttributes>();
            let sid_start = (prefix + table_start + groups.len() * stride + 3) & !3;
            let length = sid_start + groups.iter().map(|(sid, _)| sid.len()).sum::<usize>();
            let mut bytes = vec![0_u8; length];
            bytes[prefix..prefix + 4].copy_from_slice(&(groups.len() as u32).to_le_bytes());
            let mut next_sid = sid_start;
            for (index, (sid, attributes)) in groups.iter().enumerate() {
                bytes[next_sid..next_sid + sid.len()].copy_from_slice(sid);
                let header = SidAndAttributes {
                    sid: unsafe { bytes.as_mut_ptr().add(next_sid).cast() },
                    attributes: *attributes,
                };
                unsafe {
                    ptr::write_unaligned(
                        bytes
                            .as_mut_ptr()
                            .add(prefix + table_start + index * stride)
                            .cast(),
                        header,
                    );
                }
                next_sid += sid.len();
            }
            bytes
        }

        #[test]
        fn token_groups_enabled_interactive_sid_uses_whole_returned_buffer() {
            let bytes = groups_blob(
                &[
                    (&USER, SE_GROUP_ENABLED),
                    (&INTERACTIVE_SID, SE_GROUP_ENABLED),
                ],
                0,
            );
            assert!(interactive_group_enabled(&bytes).unwrap());
        }

        #[test]
        fn token_groups_disabled_deny_only_and_absent_interactive_are_not_authority() {
            for attributes in [
                0,
                SE_GROUP_USE_FOR_DENY_ONLY,
                SE_GROUP_ENABLED | SE_GROUP_USE_FOR_DENY_ONLY,
            ] {
                let bytes = groups_blob(&[(&INTERACTIVE_SID, attributes)], 0);
                assert!(!interactive_group_enabled(&bytes).unwrap());
            }
            assert!(
                !interactive_group_enabled(&groups_blob(&[(&USER, SE_GROUP_ENABLED)], 0)).unwrap()
            );
            assert!(!interactive_group_enabled(&0_u32.to_le_bytes()).unwrap());
        }

        #[test]
        fn token_groups_unaligned_header_is_copied_without_aligned_references() {
            let bytes = groups_blob(&[(&INTERACTIVE_SID, SE_GROUP_ENABLED)], 1);
            assert!(interactive_group_enabled(&bytes[1..]).unwrap());
        }

        #[test]
        fn token_groups_reject_out_of_buffer_table_alias_and_unaligned_sid() {
            for case in 0..4 {
                let mut bytes = groups_blob(&[(&INTERACTIVE_SID, SE_GROUP_ENABLED)], 0);
                let pointer = match case {
                    0 => bytes.as_mut_ptr().wrapping_sub(1),
                    1 => bytes.as_mut_ptr().wrapping_add(bytes.len()),
                    2 => bytes.as_mut_ptr(),
                    _ => bytes.as_mut_ptr().wrapping_add(bytes.len() - 11),
                };
                let header = SidAndAttributes {
                    sid: pointer.cast(),
                    attributes: SE_GROUP_ENABLED,
                };
                unsafe {
                    ptr::write_unaligned(
                        bytes
                            .as_mut_ptr()
                            .add(std::mem::offset_of!(TokenGroupsHeader, groups))
                            .cast(),
                        header,
                    );
                }
                assert!(interactive_group_enabled(&bytes).is_err(), "case {case}");
            }
        }

        #[test]
        fn token_groups_reject_excessive_counts_truncated_tables_and_sid_headers() {
            for count in [u32::MAX, (MAX_TOKEN_GROUPS + 1) as u32] {
                assert!(interactive_group_enabled(&count.to_le_bytes()).is_err());
            }
            let mut bytes = groups_blob(&[(&INTERACTIVE_SID, SE_GROUP_ENABLED)], 0);
            bytes[..4].copy_from_slice(&2_u32.to_le_bytes());
            assert!(interactive_group_enabled(&bytes).is_err());
            let mut bytes = groups_blob(&[(&INTERACTIVE_SID, SE_GROUP_ENABLED)], 0);
            bytes.truncate(bytes.len() - 5);
            assert!(interactive_group_enabled(&bytes).is_err());
            assert!(interactive_group_enabled(&vec![0; MAX_TOKEN_INFORMATION + 1]).is_err());
        }

        #[test]
        fn token_groups_validate_later_entries_even_after_an_enabled_match() {
            let mut bytes = groups_blob(
                &[
                    (&INTERACTIVE_SID, SE_GROUP_ENABLED),
                    (&USER, SE_GROUP_ENABLED),
                ],
                0,
            );
            let offset = std::mem::offset_of!(TokenGroupsHeader, groups)
                + std::mem::size_of::<SidAndAttributes>();
            let header = SidAndAttributes {
                sid: ptr::null_mut(),
                attributes: SE_GROUP_ENABLED,
            };
            unsafe { ptr::write_unaligned(bytes.as_mut_ptr().add(offset).cast(), header) };
            assert!(interactive_group_enabled(&bytes).is_err());
        }

        #[test]
        fn noninteractive_nonzero_session_is_denied_for_pipe_and_primary() {
            let mut facts = allowed();
            facts.interactive_enabled = false;
            assert_eq!(
                authorize_facts(&facts, &USER).unwrap_err(),
                "WAKE_BROKER_OPERATOR_TOKEN_DENIED"
            );
            facts.impersonation_level = None;
            assert_eq!(
                authorize_token_facts(&facts, &USER, true).unwrap_err(),
                "WAKE_BROKER_OPERATOR_TOKEN_DENIED"
            );
        }

        #[test]
        fn exact_elevated_high_interactive_operator_is_admissible() {
            let evidence = authorize_facts(&allowed(), &USER).unwrap();
            assert_eq!(evidence.integrity_rid, SECURITY_MANDATORY_HIGH_RID);
            assert_eq!(evidence.session_id, 1);
            assert_eq!(evidence.operator_sid_sha256.len(), 64);
        }

        #[test]
        fn local_update_medium_interactive_user_does_not_gain_operator_authority() {
            let mut facts = allowed();
            facts.integrity_rid = SECURITY_MANDATORY_MEDIUM_RID;
            facts.elevated = false;
            let user = authorize_local_user_facts(&facts, &USER, false).unwrap();
            assert_eq!(user.user_sid_sha256().len(), 64);
            assert!(user.operator().is_none());
            assert!(authorize_facts(&facts, &USER).is_err());
            facts.impersonation_level = None;
            assert!(authorize_local_user_facts(&facts, &USER, true).is_ok());
            assert!(authorize_local_user_facts(&facts, &USER, false).is_err());
            assert!(
                authorize_local_user_facts(&allowed(), &USER, false)
                    .unwrap()
                    .operator()
                    .is_some()
            );
        }

        #[test]
        fn local_update_refuses_other_account_low_restricted_and_noninteractive_tokens() {
            for case in 0..9 {
                let mut facts = allowed();
                match case {
                    0 => facts.user_sid = OTHER.to_vec(),
                    1 => facts.integrity_rid = 0x1000,
                    2 => facts.session_id = 0,
                    3 => facts.interactive_enabled = false,
                    4 => facts.app_container = true,
                    5 => facts.restricted = true,
                    6 => facts.impersonation_level = Some(1),
                    7 => facts.impersonation_level = Some(4),
                    _ => facts.impersonation_level = None,
                }
                assert!(
                    authorize_local_user_facts(&facts, &USER, false).is_err(),
                    "case {case}"
                );
            }
            assert!(authorize_local_user_facts(&allowed(), &LOCAL_SYSTEM_SID, false).is_err());
        }

        #[test]
        fn current_primary_operator_reuses_identity_gates_without_impersonation_level_claim() {
            let mut facts = allowed();
            facts.impersonation_level = None;
            assert!(authorize_token_facts(&facts, &USER, true).is_ok());
            assert!(authorize_facts(&facts, &USER).is_err());
            assert!(authorize_token_facts(&allowed(), &USER, true).is_err());
            assert!(authorize_token_facts(&facts, &OTHER, true).is_err());
            for case in 0..4 {
                let mut denied = facts.clone();
                match case {
                    0 => denied.elevated = false,
                    1 => denied.integrity_rid = 0x2000,
                    2 => denied.session_id = 0,
                    _ => denied.restricted = true,
                }
                assert!(authorize_token_facts(&denied, &USER, true).is_err());
            }
            facts.app_container = true;
            assert!(authorize_token_facts(&facts, &USER, true).is_err());
        }

        #[test]
        fn different_user_and_system_policy_are_denied() {
            assert!(authorize_facts(&allowed(), &OTHER).is_err());
            assert!(authorize_facts(&allowed(), &LOCAL_SYSTEM_SID).is_err());
        }

        #[test]
        fn low_integrity_unelevated_and_session_zero_are_denied() {
            let mut facts = allowed();
            facts.integrity_rid = 0x2000;
            assert!(authorize_facts(&facts, &USER).is_err());
            facts = allowed();
            facts.elevated = false;
            assert!(authorize_facts(&facts, &USER).is_err());
            facts = allowed();
            facts.session_id = 0;
            assert!(authorize_facts(&facts, &USER).is_err());
        }

        #[test]
        fn app_container_and_restricted_token_are_denied() {
            let mut facts = allowed();
            facts.app_container = true;
            assert!(authorize_facts(&facts, &USER).is_err());
            facts = allowed();
            facts.restricted = true;
            assert!(authorize_facts(&facts, &USER).is_err());
        }

        #[test]
        fn identification_only_impersonation_is_denied() {
            let mut facts = allowed();
            facts.impersonation_level = Some(1);
            assert!(authorize_facts(&facts, &USER).is_err());
            facts.impersonation_level = Some(4);
            assert!(authorize_facts(&facts, &USER).is_err());
        }

        #[test]
        fn malformed_policy_and_integrity_authority_are_denied() {
            assert!(authorize_facts(&allowed(), &[1, 0, 0]).is_err());
            let mut wrong_integrity_sid = USER;
            wrong_integrity_sid[8..12].copy_from_slice(&0x3000_u32.to_le_bytes());
            assert!(integrity_rid(&wrong_integrity_sid).is_err());
        }

        #[test]
        fn connected_pipe_uses_the_client_token_and_reverts() {
            connected_pipe_with_qos(0x0010_0000 | 0x0002_0000, true);
        }

        #[test]
        fn connected_pipe_effective_only_context_does_not_gain_canonical_uac_admission() {
            connected_pipe_with_qos(0x0010_0000 | 0x0002_0000 | 0x0008_0000, false);
        }

        fn connected_pipe_with_qos(client_qos: u32, require_complete_profile: bool) {
            let name = format!(
                r"\\.\pipe\CatDeskWakeBrokerOperatorFixture-{}-{}",
                unsafe { GetCurrentProcessId() },
                NEXT_PIPE.fetch_add(1, Ordering::Relaxed)
            );
            let wide = OsStr::new(&name)
                .encode_wide()
                .chain([0])
                .collect::<Vec<_>>();
            let raw_server = unsafe {
                CreateNamedPipeW(
                    wide.as_ptr(),
                    3 | 0x0008_0000, // duplex, first instance
                    8,               // reject remote clients
                    1,
                    128,
                    128,
                    0,
                    ptr::null_mut(),
                )
            };
            assert!(!raw_server.is_null() && raw_server != INVALID_HANDLE);
            let server = unsafe { OwnedHandle::from_raw_handle(raw_server) };
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let client = std::thread::spawn(move || {
                let raw_client = unsafe {
                    CreateFileW(
                        wide.as_ptr(),
                        0xc000_0000,
                        0,
                        ptr::null_mut(),
                        3,
                        // Match the production client's SQOS bits. I/O stays
                        // synchronous in this one-message fixture only.
                        client_qos,
                        ptr::null_mut(),
                    )
                };
                assert!(!raw_client.is_null() && raw_client != INVALID_HANDLE);
                let client = unsafe { OwnedHandle::from_raw_handle(raw_client) };
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
                release_rx.recv().unwrap();
            });
            let connected = unsafe { ConnectNamedPipe(server.as_raw_handle(), ptr::null_mut()) };
            assert!(connected != 0 || unsafe { GetLastError() } == 535);
            let mut byte = 0_u8;
            let mut read = 0_u32;
            assert_ne!(
                unsafe {
                    ReadFile(
                        server.as_raw_handle(),
                        (&mut byte as *mut u8).cast(),
                        1,
                        &mut read,
                        ptr::null_mut(),
                    )
                },
                0
            );
            assert_eq!((byte, read), (b'x', 1));

            let mut own_raw_token = ptr::null_mut();
            assert_ne!(
                unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut own_raw_token) },
                0
            );
            let own_token = unsafe { OwnedHandle::from_raw_handle(own_raw_token) };
            let own_sid =
                embedded_sid(&query_token_blob(own_token.as_raw_handle(), TOKEN_USER).unwrap())
                    .unwrap()
                    .to_vec();
            let mut wrong_sid = own_sid.clone();
            *wrong_sid.last_mut().unwrap() ^= 1;
            let wrong = inspect_connected_pipe_operator(server.as_handle(), &wrong_sid);
            let own = inspect_connected_pipe_operator(server.as_handle(), &own_sid);
            let local_wrong = inspect_connected_pipe_local_user(server.as_handle(), &wrong_sid);
            let local_own = inspect_connected_pipe_local_user(server.as_handle(), &own_sid);
            let primary_local = inspect_current_process_local_user(&own_sid);
            release_tx.send(()).unwrap();
            client.join().unwrap();
            assert_eq!(wrong.unwrap_err(), "WAKE_BROKER_OPERATOR_TOKEN_DENIED");
            assert!(own.is_ok() || own.unwrap_err() == "WAKE_BROKER_OPERATOR_TOKEN_DENIED");
            assert_eq!(
                local_wrong.unwrap_err(),
                "WAKE_BROKER_OPERATOR_TOKEN_DENIED"
            );
            let actual_facts = token_facts(own_token.as_raw_handle(), true).unwrap();
            if require_user_identity_facts(
                &actual_facts,
                &own_sid,
                true,
                SECURITY_MANDATORY_MEDIUM_RID,
                false,
            )
            .is_ok()
            {
                assert!(
                    primary_local.is_ok(),
                    "actual primary local admission code={}",
                    primary_local
                        .as_ref()
                        .err()
                        .map(String::as_str)
                        .unwrap_or("NONE")
                );
                if require_complete_profile {
                    assert!(
                        local_own.is_ok(),
                        "actual one-read pipe local admission code={}; primaryCode={}",
                        local_own
                            .as_ref()
                            .err()
                            .map(String::as_str)
                            .unwrap_or("NONE"),
                        primary_local
                            .as_ref()
                            .err()
                            .map(String::as_str)
                            .unwrap_or("NONE")
                    );
                    if actual_facts.restricted {
                        eprintln!("CANONICAL_UAC_ONE_READ_PIPE_PROFILE_MATCHED=true");
                    }
                } else if actual_facts.restricted {
                    assert!(
                        local_own.is_err(),
                        "effective-only context must not gain canonical admission"
                    );
                    eprintln!(
                        "CANONICAL_UAC_EFFECTIVE_ONLY_RESULT code={}",
                        local_own
                            .as_ref()
                            .err()
                            .map(String::as_str)
                            .unwrap_or("NONE")
                    );
                    eprintln!("CANONICAL_UAC_EFFECTIVE_ONLY_REFUSED=true");
                }
            }
            if require_complete_profile {
                assert_eq!(local_own.is_ok(), primary_local.is_ok());
            }
            if let Ok(local) = local_own {
                assert_eq!(
                    local.user_sid_sha256(),
                    primary_local.unwrap().user_sid_sha256()
                );
            }

            // A completed admission check must not leave this test thread
            // impersonating the connected client.
            let mut leftover = ptr::null_mut();
            assert_eq!(
                unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut leftover) },
                0
            );
            assert_eq!(unsafe { GetLastError() }, ERROR_NO_TOKEN);
        }
    }
}

#[cfg(windows)]
pub use windows::{inspect_connected_pipe_operator, inspect_current_process_operator};

#[cfg(windows)]
pub(crate) use windows::{inspect_connected_pipe_local_user, inspect_current_process_local_user};

#[cfg(windows)]
pub(crate) fn admit_bootstrap_operator(expected_sid: &str) -> Result<OperatorTokenEvidenceV1> {
    let sid = crate::install_broker_policy::user_sid_bytes(expected_sid)?;
    inspect_current_process_operator(&sid)
}
