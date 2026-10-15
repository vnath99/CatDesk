//! One-message receive boundary for the fixed Wake install broker.
//!
//! The caller must first connect the single fixed endpoint and serialize this
//! receive with all other reads of that pipe instance. Windows impersonates
//! the sender of the *last message read*, so admission occurs immediately
//! after the only completed ReadFile and before any decoding. A failed,
//! timed-out or oversized read never reaches token admission.
//!
//! This does not verify an approval, advance the protected ledger, or mutate
//! an install. The caller must bound connection and must not read again on
//! this instance until it has finished acting on the admitted frame.

#![cfg(windows)]

use std::os::windows::io::BorrowedHandle;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::Result;
use crate::install_broker_endpoint::FixedBrokerPipeEndpoint;
use crate::install_broker_operator::OperatorTokenEvidenceV1;
use crate::install_broker_pipe_io::read_once;
use crate::install_broker_policy::BrokerProtectedPolicyV1;

const MAX_APPROVAL_FRAME_BYTES: usize = 16 * 1024;

/// An admitted frame is still only input. The future broker must verify its
/// signature and binding against independently observed state and replay
/// history before the first install mutation.
pub struct AdmittedBrokerApprovalFrameV1 {
    approval_envelope: Vec<u8>,
    operator: OperatorTokenEvidenceV1,
}

impl AdmittedBrokerApprovalFrameV1 {
    pub fn approval_envelope(&self) -> &[u8] {
        &self.approval_envelope
    }

    pub fn operator_evidence(&self) -> &OperatorTokenEvidenceV1 {
        &self.operator
    }
}

fn read_once_then_admit<T>(
    pipe: BorrowedHandle<'_>,
    deadline: Duration,
    stop: &AtomicBool,
    admit: impl FnOnce(BorrowedHandle<'_>) -> Result<T>,
) -> Result<(Vec<u8>, T)> {
    // One extra byte distinguishes the maximum valid frame from an oversized
    // complete message. ERROR_MORE_DATA is a refusal, never a continuation.
    let bytes = read_once(pipe, MAX_APPROVAL_FRAME_BYTES, deadline, stop)?;
    // Do not decode, revalidate policy, disconnect, or make another pipe read
    // between ReadFile and connected-client impersonation.
    let evidence = admit(pipe)?;
    Ok((bytes, evidence))
}

/// Receives exactly one message from an already connected fixed pipe and
/// immediately checks the connected sender against the pinned protected
/// operator policy. This grants no install or response authority.
pub fn read_admitted_approval_frame(
    endpoint: &mut FixedBrokerPipeEndpoint,
    policy: &BrokerProtectedPolicyV1,
    deadline: Duration,
    stop: &AtomicBool,
) -> Result<AdmittedBrokerApprovalFrameV1> {
    policy.revalidate()?;
    let (approval_envelope, operator) =
        read_once_then_admit(endpoint.as_handle(), deadline, stop, |pipe| {
            policy.inspect_connected_operator_immediately_after_read(pipe)
        })?;
    policy.revalidate()?;
    Ok(AdmittedBrokerApprovalFrameV1 {
        approval_envelope,
        operator,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_broker_pipe_io::connect_once;
    use std::ffi::{OsStr, c_void};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle};
    use std::ptr;
    use std::sync::atomic::{AtomicU64, Ordering};

    const PIPE_ACCESS_DUPLEX: u32 = 3;
    const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
    const PIPE_TYPE_MESSAGE: u32 = 4;
    const PIPE_READMODE_MESSAGE: u32 = 2;
    const OPEN_EXISTING: u32 = 3;
    const GENERIC_READ_WRITE: u32 = 0xc000_0000;
    const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;
    static NEXT: AtomicU64 = AtomicU64::new(1);

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateNamedPipeW(
            name: *const u16,
            open_mode: u32,
            pipe_mode: u32,
            max_instances: u32,
            out_buffer: u32,
            in_buffer: u32,
            timeout: u32,
            security: *mut c_void,
        ) -> *mut c_void;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            disposition: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
        fn WriteFile(
            file: *mut c_void,
            buffer: *const c_void,
            size: u32,
            written: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn GetCurrentProcessId() -> u32;
    }

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value).encode_wide().chain([0]).collect()
    }

    fn fixture(message: Vec<u8>) -> (OwnedHandle, std::thread::JoinHandle<()>) {
        let name = format!(
            r"\\.\pipe\CatDeskWakeBrokerFrameFixture-{}-{}",
            unsafe { GetCurrentProcessId() },
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let raw = unsafe {
            CreateNamedPipeW(
                wide(&name).as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE,
                1,
                32 * 1024,
                32 * 1024,
                0,
                ptr::null_mut(),
            )
        };
        assert!(!raw.is_null() && raw != INVALID_HANDLE_VALUE);
        let server = unsafe { OwnedHandle::from_raw_handle(raw) };
        let client = std::thread::spawn(move || {
            let raw = unsafe {
                CreateFileW(
                    wide(&name).as_ptr(),
                    GENERIC_READ_WRITE,
                    0,
                    ptr::null_mut(),
                    OPEN_EXISTING,
                    0,
                    ptr::null_mut(),
                )
            };
            assert!(!raw.is_null() && raw != INVALID_HANDLE_VALUE);
            let client = unsafe { OwnedHandle::from_raw_handle(raw) };
            if message.is_empty() {
                std::thread::sleep(Duration::from_millis(100));
            } else {
                let mut written = 0_u32;
                assert_ne!(
                    unsafe {
                        WriteFile(
                            client.as_raw_handle(),
                            message.as_ptr().cast(),
                            message.len() as u32,
                            &mut written,
                            ptr::null_mut(),
                        )
                    },
                    0
                );
                assert_eq!(written as usize, message.len());
            }
        });
        connect_once(
            server.as_handle(),
            Duration::from_secs(2),
            &AtomicBool::new(false),
        )
        .expect("fixture connection");
        (server, client)
    }

    #[test]
    fn one_complete_message_is_admitted_once_before_any_decoding() {
        let (server, client) = fixture(b"signed-envelope".to_vec());
        let mut calls = 0;
        let (bytes, evidence) = read_once_then_admit(
            server.as_handle(),
            Duration::from_secs(2),
            &AtomicBool::new(false),
            |_| {
                calls += 1;
                Ok(7_u32)
            },
        )
        .expect("single frame");
        assert_eq!(bytes, b"signed-envelope");
        assert_eq!(evidence, 7);
        assert_eq!(calls, 1);
        drop(server);
        client.join().expect("client");
    }

    #[test]
    fn oversized_message_never_reaches_operator_admission() {
        let (server, client) = fixture(vec![b'x'; MAX_APPROVAL_FRAME_BYTES + 2]);
        let mut calls = 0;
        let result = read_once_then_admit(
            server.as_handle(),
            Duration::from_secs(2),
            &AtomicBool::new(false),
            |_| {
                calls += 1;
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(calls, 0);
        drop(server);
        client.join().expect("client");
    }

    #[test]
    fn operator_denial_discards_read_frame() {
        let (server, client) = fixture(b"signed-envelope".to_vec());
        let mut calls = 0;
        let result: Result<(Vec<u8>, ())> = read_once_then_admit(
            server.as_handle(),
            Duration::from_secs(2),
            &AtomicBool::new(false),
            |_| {
                calls += 1;
                Err("WAKE_BROKER_OPERATOR_DENIED".into())
            },
        );
        assert_eq!(result.unwrap_err(), "WAKE_BROKER_OPERATOR_DENIED");
        assert_eq!(calls, 1);
        drop(server);
        client.join().expect("client");
    }

    #[test]
    fn read_deadline_and_stop_never_reach_operator_admission() {
        let (server, client) = fixture(Vec::new());
        let mut calls = 0;
        let result = read_once_then_admit(
            server.as_handle(),
            Duration::from_millis(20),
            &AtomicBool::new(false),
            |_| {
                calls += 1;
                Ok(())
            },
        );
        assert_eq!(result.unwrap_err(), "WAKE_BROKER_PIPE_DEADLINE_OR_STOP");
        assert_eq!(calls, 0);
        drop(server);
        client.join().expect("client");

        let (server, client) = fixture(Vec::new());
        let stop = AtomicBool::new(true);
        let result =
            read_once_then_admit(server.as_handle(), Duration::from_secs(1), &stop, |_| {
                calls += 1;
                Ok(())
            });
        assert_eq!(result.unwrap_err(), "WAKE_BROKER_PIPE_DEADLINE_OR_STOP");
        assert_eq!(calls, 0);
        drop(server);
        client.join().expect("client");
    }
}
