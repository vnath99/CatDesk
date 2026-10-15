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

#[cfg(not(windows))]
pub fn inspect_connected_pipe_operator(
    _pipe: std::os::fd::BorrowedFd<'_>,
    _protected_operator_sid: &[u8],
) -> Result<OperatorTokenEvidenceV1> {
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
    const TOKEN_IMPERSONATION_LEVEL: u32 = 9;
    const TOKEN_SESSION_ID: u32 = 12;
    const TOKEN_ELEVATION: u32 = 20;
    const TOKEN_HAS_RESTRICTIONS: u32 = 21;
    const TOKEN_INTEGRITY_LEVEL: u32 = 25;
    const TOKEN_IS_APP_CONTAINER: u32 = 29;
    const ERROR_NO_TOKEN: u32 = 1008;
    const MAX_TOKEN_INFORMATION: usize = 4096;
    const MAX_SID_BYTES: usize = 68;
    const SECURITY_MANDATORY_HIGH_RID: u32 = 0x3000;
    const MANDATORY_LABEL_AUTHORITY: [u8; 6] = [0, 0, 0, 0, 0, 16];
    const LOCAL_SYSTEM_SID: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];

    #[repr(C)]
    struct SidAndAttributes {
        sid: *mut c_void,
        attributes: u32,
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
    }

    unsafe extern "system" {
        fn GetCurrentThread() -> *mut c_void;
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

    #[derive(Clone, Debug)]
    struct TokenFacts {
        user_sid: Vec<u8>,
        integrity_rid: u32,
        session_id: u32,
        elevated: bool,
        impersonation_level: u32,
        app_container: bool,
        restricted: bool,
    }

    /// Reads an impersonated pipe-client token, not a process PID or a JSON
    /// identity claim. The caller must retain the same pipe instance for the
    /// full request and must not cache this result for another connection.
    pub fn inspect_connected_pipe_operator(
        pipe: BorrowedHandle<'_>,
        protected_operator_sid: &[u8],
    ) -> Result<OperatorTokenEvidenceV1> {
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
        let facts = token_facts(token.as_raw_handle())?;
        authorize_facts(&facts, protected_operator_sid)
    }

    fn query_token_blob(token: *mut c_void, class: u32) -> Result<Vec<u8>> {
        let mut needed = 0_u32;
        unsafe { GetTokenInformation(token, class, ptr::null_mut(), 0, &mut needed) };
        if needed == 0 || needed as usize > MAX_TOKEN_INFORMATION {
            return Err("WAKE_BROKER_OPERATOR_TOKEN_INVALID".into());
        }
        let mut bytes = vec![0_u8; needed as usize];
        let mut returned = 0_u32;
        if unsafe {
            GetTokenInformation(
                token,
                class,
                bytes.as_mut_ptr().cast(),
                needed,
                &mut returned,
            )
        } == 0
            || returned == 0
            || returned > needed
        {
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
        let offset = header
            .sid
            .addr()
            .checked_sub(bytes.as_ptr().addr())
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

    fn token_facts(token: *mut c_void) -> Result<TokenFacts> {
        let user_sid = embedded_sid(&query_token_blob(token, TOKEN_USER)?)?.to_vec();
        let integrity_rid = integrity_rid(embedded_sid(&query_token_blob(
            token,
            TOKEN_INTEGRITY_LEVEL,
        )?)?)?;
        let session_id = token_u32(token, TOKEN_SESSION_ID)?;
        let elevated = token_u32(token, TOKEN_ELEVATION)? == 1;
        let impersonation_level = token_u32(token, TOKEN_IMPERSONATION_LEVEL)?;
        let app_container = token_u32(token, TOKEN_IS_APP_CONTAINER)? != 0;
        // TokenHasRestrictions also covers disabled SIDs and deleted privileges.
        // IsTokenRestricted returns FALSE on API errors, which would fail open.
        let restricted = token_has_restrictions(token)?;
        Ok(TokenFacts {
            user_sid,
            integrity_rid,
            session_id,
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
        valid_sid(protected_operator_sid)?;
        if protected_operator_sid == LOCAL_SYSTEM_SID
            || facts.user_sid != protected_operator_sid
            || facts.integrity_rid < SECURITY_MANDATORY_HIGH_RID
            || facts.session_id == 0
            || !facts.elevated
            || !(2..=3).contains(&facts.impersonation_level)
            || facts.app_container
            || facts.restricted
        {
            return Err("WAKE_BROKER_OPERATOR_TOKEN_DENIED".into());
        }
        Ok(OperatorTokenEvidenceV1 {
            operator_sid_sha256: format!("{:x}", Sha256::digest(&facts.user_sid)),
            integrity_rid: facts.integrity_rid,
            session_id: facts.session_id,
        })
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
                elevated: true,
                impersonation_level: 2,
                app_container: false,
                restricted: false,
            }
        }

        #[test]
        fn exact_elevated_high_interactive_operator_is_admissible() {
            let evidence = authorize_facts(&allowed(), &USER).unwrap();
            assert_eq!(evidence.integrity_rid, SECURITY_MANDATORY_HIGH_RID);
            assert_eq!(evidence.session_id, 1);
            assert_eq!(evidence.operator_sid_sha256.len(), 64);
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
            facts.impersonation_level = 1;
            assert!(authorize_facts(&facts, &USER).is_err());
            facts.impersonation_level = 4;
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
                        0,
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
            release_tx.send(()).unwrap();
            client.join().unwrap();
            assert_eq!(wrong.unwrap_err(), "WAKE_BROKER_OPERATOR_TOKEN_DENIED");
            assert!(own.is_ok() || own.unwrap_err() == "WAKE_BROKER_OPERATOR_TOKEN_DENIED");

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
pub use windows::inspect_connected_pipe_operator;
