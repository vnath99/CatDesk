//! Bounded, one-shot overlapped I/O for the fixed broker pipe.
//!
//! A pending operation owns its event, OVERLAPPED and read buffer until the
//! kernel has completed it. On an unresponsive cancellation those allocations
//! are deliberately retained until process exit; a service must stop after
//! such a refusal, never reuse the pipe or start a second request.

#![cfg(windows)]

use std::ffi::c_void;
use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::Result;

const ERROR_IO_PENDING: i32 = 997;
const ERROR_PIPE_CONNECTED: i32 = 535;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 258;
const CANCEL_DRAIN_MS: u32 = 2_000;

#[repr(C)]
#[derive(Default)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: *mut c_void,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateEventW(
        attributes: *mut c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> *mut c_void;
    fn ConnectNamedPipe(pipe: *mut c_void, overlapped: *mut c_void) -> i32;
    fn ReadFile(
        file: *mut c_void,
        buffer: *mut c_void,
        length: u32,
        read: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn GetLastError() -> u32;
    fn GetOverlappedResult(
        file: *mut c_void,
        overlapped: *mut Overlapped,
        transferred: *mut u32,
        wait: i32,
    ) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn CancelIoEx(file: *mut c_void, overlapped: *mut Overlapped) -> i32;
}

struct PendingIo {
    event: OwnedHandle,
    overlapped: Box<Overlapped>,
    buffer: Vec<u8>,
    transferred: Box<u32>,
}

impl PendingIo {
    fn new(buffer_size: usize) -> Result<Self> {
        let raw = unsafe { CreateEventW(ptr::null_mut(), 1, 0, ptr::null()) };
        if raw.is_null() {
            return Err("WAKE_BROKER_PIPE_EVENT_FAILED".into());
        }
        let event = unsafe { OwnedHandle::from_raw_handle(raw) };
        let overlapped = Box::new(Overlapped {
            event: event.as_raw_handle(),
            ..Overlapped::default()
        });
        Ok(Self {
            event,
            overlapped,
            buffer: vec![0; buffer_size],
            transferred: Box::new(0),
        })
    }

    fn raw_overlapped(&mut self) -> *mut Overlapped {
        self.overlapped.as_mut()
    }

    fn cancel_and_retire(self, pipe: BorrowedHandle<'_>) {
        unsafe {
            CancelIoEx(
                pipe.as_raw_handle(),
                self.overlapped.as_ref() as *const _ as *mut _,
            )
        };
        if unsafe { WaitForSingleObject(self.event.as_raw_handle(), CANCEL_DRAIN_MS) }
            == WAIT_OBJECT_0
        {
            let mut transferred = 0;
            unsafe {
                GetOverlappedResult(
                    pipe.as_raw_handle(),
                    self.overlapped.as_ref() as *const _ as *mut _,
                    &mut transferred,
                    0,
                )
            };
        } else {
            // The kernel may still reference OVERLAPPED and the read buffer.
            // One-shot service process exit reclaims these after handle close.
            std::mem::forget(self);
        }
    }

    fn wait(
        mut self,
        pipe: BorrowedHandle<'_>,
        started: Instant,
        deadline: Duration,
        stop: &AtomicBool,
    ) -> Result<(Vec<u8>, u32)> {
        loop {
            if stop.load(Ordering::Acquire) || started.elapsed() >= deadline {
                self.cancel_and_retire(pipe);
                return Err("WAKE_BROKER_PIPE_DEADLINE_OR_STOP".into());
            }
            let remaining = deadline.saturating_sub(started.elapsed());
            let slice = remaining.as_millis().clamp(1, 100) as u32;
            match unsafe { WaitForSingleObject(self.event.as_raw_handle(), slice) } {
                WAIT_OBJECT_0 => {
                    let mut transferred = 0;
                    if unsafe {
                        GetOverlappedResult(
                            pipe.as_raw_handle(),
                            self.raw_overlapped(),
                            &mut transferred,
                            0,
                        )
                    } == 0
                    {
                        return Err("WAKE_BROKER_PIPE_OPERATION_FAILED".into());
                    }
                    return Ok((self.buffer, transferred));
                }
                WAIT_TIMEOUT => continue,
                _ => {
                    self.cancel_and_retire(pipe);
                    return Err("WAKE_BROKER_PIPE_WAIT_FAILED".into());
                }
            }
        }
    }
}

fn after_pending_hook(
    io: PendingIo,
    pipe: BorrowedHandle<'_>,
    on_pending: impl FnOnce() -> Result<()>,
) -> Result<PendingIo> {
    match catch_unwind(AssertUnwindSafe(on_pending)) {
        Ok(Ok(())) => Ok(io),
        Ok(Err(error)) => {
            io.cancel_and_retire(pipe);
            Err(error)
        }
        Err(panic) => {
            io.cancel_and_retire(pipe);
            resume_unwind(panic)
        }
    }
}

/// Connect the one-instance fixed server endpoint or refuse at the deadline.
pub(crate) fn connect_once(
    pipe: BorrowedHandle<'_>,
    deadline: Duration,
    stop: &AtomicBool,
) -> Result<()> {
    connect_once_with_pending_hook(pipe, deadline, stop, || Ok(()))
}

fn connect_once_with_pending_hook(
    pipe: BorrowedHandle<'_>,
    deadline: Duration,
    stop: &AtomicBool,
    on_pending: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if deadline.is_zero() || stop.load(Ordering::Acquire) {
        return Err("WAKE_BROKER_PIPE_DEADLINE_OR_STOP".into());
    }
    let started = Instant::now();
    let mut io = PendingIo::new(0)?;
    if unsafe { ConnectNamedPipe(pipe.as_raw_handle(), io.raw_overlapped().cast()) } != 0 {
        return Ok(());
    }
    match unsafe { GetLastError() } as i32 {
        ERROR_PIPE_CONNECTED => Ok(()),
        ERROR_IO_PENDING => {
            let io = after_pending_hook(io, pipe, on_pending)?;
            io.wait(pipe, started, deadline, stop).map(|_| ())
        }
        _ => Err("WAKE_BROKER_PIPE_CONNECT_FAILED".into()),
    }
}

/// Read a single complete message. ERROR_MORE_DATA is a refusal; the caller
/// must not continue reading before impersonating the client of this frame.
pub(crate) fn read_once(
    pipe: BorrowedHandle<'_>,
    max_bytes: usize,
    deadline: Duration,
    stop: &AtomicBool,
) -> Result<Vec<u8>> {
    read_once_with_pending_hook(pipe, max_bytes, deadline, stop, || Ok(()))
}

fn read_once_with_pending_hook(
    pipe: BorrowedHandle<'_>,
    max_bytes: usize,
    deadline: Duration,
    stop: &AtomicBool,
    on_pending: impl FnOnce() -> Result<()>,
) -> Result<Vec<u8>> {
    if deadline.is_zero() || stop.load(Ordering::Acquire) || max_bytes > u32::MAX as usize - 1 {
        return Err("WAKE_BROKER_PIPE_DEADLINE_OR_STOP".into());
    }
    let started = Instant::now();
    let mut io = PendingIo::new(max_bytes + 1)?;
    // Keep lpNumberOfBytesRead in the same retained allocation set as the
    // OVERLAPPED and buffer. Windows may still touch it after a bounded
    // cancellation return; on immediate success it supplies the byte count.
    let complete = unsafe {
        ReadFile(
            pipe.as_raw_handle(),
            io.buffer.as_mut_ptr().cast(),
            io.buffer.len() as u32,
            io.transferred.as_mut(),
            io.raw_overlapped().cast(),
        )
    } != 0;
    let (mut buffer, transferred) = if complete {
        let transferred = *io.transferred;
        (io.buffer, transferred)
    } else if unsafe { GetLastError() } as i32 == ERROR_IO_PENDING {
        let io = after_pending_hook(io, pipe, on_pending)?;
        io.wait(pipe, started, deadline, stop)?
    } else {
        return Err("WAKE_BROKER_PIPE_READ_FAILED".into());
    };
    if transferred == 0 || transferred as usize > max_bytes {
        return Err("WAKE_BROKER_PIPE_MESSAGE_SIZE_INVALID".into());
    }
    buffer.truncate(transferred as usize);
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsHandle;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;
    use std::sync::mpsc;

    const PIPE_ACCESS_DUPLEX: u32 = 3;
    const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
    const PIPE_TYPE_MESSAGE: u32 = 4;
    const PIPE_READMODE_MESSAGE: u32 = 2;
    const GENERIC_READ_WRITE: u32 = 0xc000_0000;
    const OPEN_EXISTING: u32 = 3;
    const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;
    static NEXT: AtomicU64 = AtomicU64::new(1);

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateNamedPipeW(
            name: *const u16,
            mode: u32,
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
            length: u32,
            written: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn GetCurrentProcessId() -> u32;
    }

    fn fixture() -> (Vec<u16>, OwnedHandle) {
        let name = format!(
            r"\\.\pipe\CatDeskWakeBrokerDeadlineFixture-{}-{}",
            unsafe { GetCurrentProcessId() },
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let name: Vec<u16> = OsStr::new(&name).encode_wide().chain([0]).collect();
        let raw = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE,
                1,
                4096,
                4096,
                0,
                ptr::null_mut(),
            )
        };
        assert!(!raw.is_null() && raw != INVALID_HANDLE_VALUE);
        (name, unsafe { OwnedHandle::from_raw_handle(raw) })
    }

    fn connected_fixture() -> (OwnedHandle, OwnedHandle) {
        let (name, server) = fixture();
        let raw_client = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ_WRITE,
                0,
                ptr::null_mut(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        assert!(!raw_client.is_null() && raw_client != INVALID_HANDLE_VALUE);
        let client = unsafe { OwnedHandle::from_raw_handle(raw_client) };
        connect_once(
            server.as_handle(),
            Duration::from_secs(2),
            &AtomicBool::new(false),
        )
        .expect("client connected");
        (server, client)
    }

    #[test]
    fn connect_deadline_refuses_without_client() {
        let (_, pipe) = fixture();
        let started = Instant::now();
        assert_eq!(
            connect_once(
                pipe.as_handle(),
                Duration::from_millis(20),
                &AtomicBool::new(false)
            )
            .unwrap_err(),
            "WAKE_BROKER_PIPE_DEADLINE_OR_STOP"
        );
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn stop_after_pending_connect_cancels_and_closes_one_shot_pipe() {
        let (name, pipe) = fixture();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (pending_tx, pending_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let started = Instant::now();
            let result = connect_once_with_pending_hook(
                pipe.as_handle(),
                Duration::from_secs(10),
                &worker_stop,
                || {
                    pending_tx
                        .send(())
                        .map_err(|_| "FIXTURE_PENDING_CONNECT_CHANNEL_CLOSED")?;
                    continue_rx
                        .recv_timeout(Duration::from_secs(2))
                        .map_err(|_| "FIXTURE_PENDING_CONNECT_RELEASE_MISSING")?;
                    Ok(())
                },
            );
            drop(pipe); // The canceled instance is never offered for reuse.
            (result, started.elapsed())
        });
        pending_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("connect must be pending without a client");
        stop.store(true, Ordering::Release);
        continue_tx.send(()).expect("release worker");
        let (result, elapsed) = worker.join().expect("worker exits");
        assert_eq!(result.unwrap_err(), "WAKE_BROKER_PIPE_DEADLINE_OR_STOP");
        assert!(elapsed < Duration::from_secs(4));
        let late_client = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ_WRITE,
                0,
                ptr::null_mut(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        assert_eq!(late_client, INVALID_HANDLE_VALUE);
    }

    #[test]
    fn stop_after_pending_read_cancels_without_accepting_late_bytes() {
        let (server, client) = connected_fixture();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (pending_tx, pending_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let started = Instant::now();
            let result = read_once_with_pending_hook(
                server.as_handle(),
                64,
                Duration::from_secs(10),
                &worker_stop,
                || {
                    pending_tx
                        .send(())
                        .map_err(|_| "FIXTURE_PENDING_READ_CHANNEL_CLOSED")?;
                    continue_rx
                        .recv_timeout(Duration::from_secs(2))
                        .map_err(|_| "FIXTURE_PENDING_READ_RELEASE_MISSING")?;
                    Ok(())
                },
            );
            drop(server); // No second read or client admission follows cancel.
            (result, started.elapsed())
        });
        pending_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("read must be pending without client bytes");
        stop.store(true, Ordering::Release);
        continue_tx.send(()).expect("release worker");
        let (result, elapsed) = worker.join().expect("worker exits");
        assert_eq!(result.unwrap_err(), "WAKE_BROKER_PIPE_DEADLINE_OR_STOP");
        assert!(elapsed < Duration::from_secs(4));
        let mut written = 0;
        assert_eq!(
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
        assert_eq!(written, 0);
    }

    #[test]
    fn pending_connect_hook_panic_retires_kernel_operation_before_unwind() {
        let (name, server) = fixture();
        let started = Instant::now();
        let caught = catch_unwind(AssertUnwindSafe(|| {
            connect_once_with_pending_hook(
                server.as_handle(),
                Duration::from_secs(10),
                &AtomicBool::new(false),
                || -> Result<()> { panic!("injected pending connect hook panic") },
            )
        }));
        assert!(caught.is_err());
        assert!(started.elapsed() < Duration::from_secs(4));
        drop(server);
        let late_client = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ_WRITE,
                0,
                ptr::null_mut(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        assert_eq!(late_client, INVALID_HANDLE_VALUE);
    }

    #[test]
    fn pending_read_hook_error_retires_kernel_operation_before_return() {
        let (server, client) = connected_fixture();
        let started = Instant::now();
        assert_eq!(
            read_once_with_pending_hook(
                server.as_handle(),
                64,
                Duration::from_secs(10),
                &AtomicBool::new(false),
                || Err("INJECTED_PENDING_READ_HOOK_ERROR".into()),
            )
            .unwrap_err(),
            "INJECTED_PENDING_READ_HOOK_ERROR"
        );
        assert!(started.elapsed() < Duration::from_secs(4));
        drop(server);
        let mut written = 0;
        assert_eq!(
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
        assert_eq!(written, 0);
    }

    #[test]
    fn queued_message_read_gets_synchronous_completion_byte_count() {
        let (server, client) = connected_fixture();
        let message = b"queued-approval";
        let mut written = 0;
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
        let bytes = read_once_with_pending_hook(
            server.as_handle(),
            64,
            Duration::from_secs(2),
            &AtomicBool::new(false),
            || Err("QUEUED_READ_MUST_COMPLETE_SYNCHRONOUSLY".into()),
        )
        .expect("queued message");
        assert_eq!(bytes, message);
    }

    #[test]
    fn pending_read_gets_completion_byte_count_only_after_client_write() {
        let (server, client) = connected_fixture();
        let (pending_tx, pending_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            read_once_with_pending_hook(
                server.as_handle(),
                64,
                Duration::from_secs(2),
                &AtomicBool::new(false),
                || {
                    pending_tx
                        .send(())
                        .map_err(|_| "FIXTURE_PENDING_READ_CHANNEL_CLOSED")?;
                    continue_rx
                        .recv_timeout(Duration::from_secs(2))
                        .map_err(|_| "FIXTURE_PENDING_READ_RELEASE_MISSING")?;
                    Ok(())
                },
            )
        });
        pending_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("read pending before client write");
        let message = b"pending-approval";
        let mut written = 0;
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
        continue_tx.send(()).expect("release pending read");
        let bytes = worker
            .join()
            .expect("worker exits")
            .expect("read completes");
        assert_eq!(bytes, message);
    }
}
