//! Fixed Windows SCM entry point for the separately provisioned Wake broker.
//!
//! This is a real one-shot service process boundary, but it is deliberately
//! unable to mutate an install. It refuses after a bounded, operator-admitted
//! frame because no trusted dev.84 connected-owner observation or independent
//! exact-source CI fetch is available to the transaction yet. The fixed
//! service/namespace must be separately signed and provisioned by an
//! administrator; this module never creates either.

#![cfg(windows)]

use std::ffi::{OsStr, c_void};
use std::os::windows::ffi::OsStrExt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use crate::Result;
use crate::install_broker_current_observer::BrokerSelfClockPin;
use crate::install_broker_endpoint::create_fixed_broker_endpoint_from_protected_policy;
use crate::install_broker_ledger::BrokerLedgerGuard;
use crate::install_broker_pipe_io::connect_once;
use crate::install_broker_policy::load_fixed_broker_policy_read_only;
use crate::install_broker_preflight::{SERVICE_NAME, acquire_fixed_protected_root_for_reader};
use crate::install_broker_request::read_admitted_approval_frame;

const SERVICE_WIN32_OWN_PROCESS: u32 = 0x10;
const SERVICE_STOPPED: u32 = 1;
const SERVICE_START_PENDING: u32 = 2;
const SERVICE_STOP_PENDING: u32 = 3;
const SERVICE_RUNNING: u32 = 4;
const SERVICE_ACCEPT_STOP: u32 = 1;
const SERVICE_ACCEPT_SHUTDOWN: u32 = 4;
const SERVICE_CONTROL_STOP: u32 = 1;
const SERVICE_CONTROL_SHUTDOWN: u32 = 5;
const ERROR_SERVICE_SPECIFIC_ERROR: u32 = 1066;
const SERVICE_REFUSAL_CODE: u32 = 1;
const CONNECT_DEADLINE: Duration = Duration::from_secs(120);
const READ_DEADLINE: Duration = Duration::from_secs(10);

static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
static STATUS_HANDLE: AtomicUsize = AtomicUsize::new(0);
static STATUS_GATE: Mutex<()> = Mutex::new(());

#[repr(C)]
struct ServiceTableEntry {
    service_name: *mut u16,
    service_main: Option<unsafe extern "system" fn(u32, *mut *mut u16)>,
}

#[repr(C)]
struct ServiceStatus {
    service_type: u32,
    current_state: u32,
    controls_accepted: u32,
    win32_exit_code: u32,
    service_specific_exit_code: u32,
    checkpoint: u32,
    wait_hint: u32,
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn StartServiceCtrlDispatcherW(table: *const ServiceTableEntry) -> i32;
    fn RegisterServiceCtrlHandlerExW(
        name: *const u16,
        handler: unsafe extern "system" fn(u32, u32, *mut c_void, *mut c_void) -> u32,
        context: *mut c_void,
    ) -> *mut c_void;
    fn SetServiceStatus(handle: *mut c_void, status: *const ServiceStatus) -> i32;
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain([0]).collect()
}

fn service_status(state: u32, failure: bool) -> ServiceStatus {
    ServiceStatus {
        service_type: SERVICE_WIN32_OWN_PROCESS,
        current_state: state,
        controls_accepted: if state == SERVICE_RUNNING {
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        } else {
            0
        },
        win32_exit_code: if state == SERVICE_STOPPED && failure {
            ERROR_SERVICE_SPECIFIC_ERROR
        } else {
            0
        },
        service_specific_exit_code: if state == SERVICE_STOPPED && failure {
            SERVICE_REFUSAL_CODE
        } else {
            0
        },
        checkpoint: if state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING {
            1
        } else {
            0
        },
        wait_hint: if state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING {
            10_000
        } else {
            0
        },
    }
}

fn report_locked(state: u32, failure: bool) -> Result<()> {
    let handle = STATUS_HANDLE.load(Ordering::Acquire) as *mut c_void;
    if handle.is_null() || unsafe { SetServiceStatus(handle, &service_status(state, failure)) } == 0
    {
        return Err("WAKE_BROKER_SERVICE_STATUS_FAILED".into());
    }
    Ok(())
}

fn report(state: u32, failure: bool) -> Result<()> {
    let _gate = STATUS_GATE
        .lock()
        .map_err(|_| "WAKE_BROKER_SERVICE_STATUS_FAILED")?;
    report_locked(state, failure)
}

fn report_running_if_active() -> Result<()> {
    let _gate = STATUS_GATE
        .lock()
        .map_err(|_| "WAKE_BROKER_SERVICE_STATUS_FAILED")?;
    if STOP_REQUESTED.load(Ordering::Acquire) {
        return Err("WAKE_BROKER_SERVICE_STOP_REQUESTED".into());
    }
    report_locked(SERVICE_RUNNING, false)
}

unsafe extern "system" fn control(
    command: u32,
    _event_type: u32,
    _event_data: *mut c_void,
    _context: *mut c_void,
) -> u32 {
    if command == SERVICE_CONTROL_STOP || command == SERVICE_CONTROL_SHUTDOWN {
        if let Ok(_gate) = STATUS_GATE.lock() {
            STOP_REQUESTED.store(true, Ordering::Release);
            let _ = report_locked(SERVICE_STOP_PENDING, false);
        } else {
            STOP_REQUESTED.store(true, Ordering::Release);
        }
    }
    0
}

fn run_one_service_submission() -> Result<()> {
    // SCM is already RUNNING. Its initialization lock can block the fixed
    // preflight's own SCM read if we perform that read while START_PENDING.
    let root = acquire_fixed_protected_root_for_reader()?;
    let policy = load_fixed_broker_policy_read_only()?;
    if policy.readback()?.protected_root != *root.root_identity() {
        return Err("WAKE_BROKER_SERVICE_PROTECTED_ROOT_MISMATCH".into());
    }
    let ledger = BrokerLedgerGuard::acquire(root.root())?;
    let mut clock = BrokerSelfClockPin::acquire(&root, &ledger, &policy)?;
    clock.readback()?;
    let mut endpoint = create_fixed_broker_endpoint_from_protected_policy(&policy)?;
    connect_once(endpoint.as_handle(), CONNECT_DEADLINE, &STOP_REQUESTED)?;
    let frame =
        read_admitted_approval_frame(&mut endpoint, &policy, READ_DEADLINE, &STOP_REQUESTED)?;
    // Keep every pin and the global lock live after the admitted last read.
    clock.readback()?;
    policy.revalidate()?;
    if frame.approval_envelope().is_empty() {
        return Err("WAKE_BROKER_REQUEST_SIZE_INVALID".into());
    }
    // No supplied observation, requested path, CI claim, or legacy pathname
    // is converted into a transaction capability here. A later source change
    // must provide independent exact CI and predecessor ownership first.
    Err("WAKE_BROKER_TRANSACTION_GATES_UNAVAILABLE".into())
}

fn service_start_arguments_valid(argc: u32, argv: *mut *mut u16) -> bool {
    if argc == 0 {
        return true;
    }
    if argc != 1 || argv.is_null() {
        return false;
    }
    // SCM may pass the fixed service name as argument zero, with no further
    // start parameters. Compare only the exact bounded name and terminator.
    let value = unsafe { *argv };
    if value.is_null() {
        return false;
    }
    wide(SERVICE_NAME)
        .iter()
        .enumerate()
        .all(|(index, expected)| unsafe { *value.add(index) } == *expected)
}

unsafe extern "system" fn service_main(argc: u32, argv: *mut *mut u16) {
    let name = wide(SERVICE_NAME);
    let handle =
        unsafe { RegisterServiceCtrlHandlerExW(name.as_ptr(), control, std::ptr::null_mut()) };
    if handle.is_null() {
        return;
    }
    STATUS_HANDLE.store(handle as usize, Ordering::Release);
    let result = if !service_start_arguments_valid(argc, argv)
        || report(SERVICE_START_PENDING, false).is_err()
        || report_running_if_active().is_err()
    {
        Err("WAKE_BROKER_SERVICE_START_INVALID".to_string())
    } else {
        run_one_service_submission()
    };
    if let Ok(_gate) = STATUS_GATE.lock() {
        let _ = report_locked(SERVICE_STOPPED, result.is_err());
        STATUS_HANDLE.store(0, Ordering::Release);
    }
}

/// The binary accepts exactly the compiled `--service` command line and
/// enters the Windows Service Control Manager. There is no console/debug
/// fallback, arbitrary service name or caller-supplied configuration.
pub fn run_fixed_service_binary() -> Result<()> {
    let mut args = std::env::args_os();
    let _image = args.next();
    if args.next().as_deref() != Some(OsStr::new("--service")) || args.next().is_some() {
        return Err("WAKE_BROKER_SERVICE_ARGUMENTS_INVALID".into());
    }
    let mut name = wide(SERVICE_NAME);
    let table = [
        ServiceTableEntry {
            service_name: name.as_mut_ptr(),
            service_main: Some(service_main),
        },
        ServiceTableEntry {
            service_name: std::ptr::null_mut(),
            service_main: None,
        },
    ];
    if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
        return Err("WAKE_BROKER_SERVICE_DISPATCHER_UNAVAILABLE".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_never_reports_successful_install_authority() {
        let running = service_status(SERVICE_RUNNING, false);
        assert_eq!(
            running.controls_accepted,
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        );
        assert_eq!(running.win32_exit_code, 0);
        let refused = service_status(SERVICE_STOPPED, true);
        assert_eq!(refused.controls_accepted, 0);
        assert_eq!(refused.win32_exit_code, ERROR_SERVICE_SPECIFIC_ERROR);
        assert_eq!(refused.service_specific_exit_code, SERVICE_REFUSAL_CODE);
    }

    #[test]
    fn bounded_one_shot_deadlines_are_nonzero() {
        assert!(CONNECT_DEADLINE > READ_DEADLINE);
        assert!(READ_DEADLINE > Duration::ZERO);
    }

    #[test]
    fn scm_start_parameters_allow_only_no_arguments_or_fixed_service_name() {
        assert!(service_start_arguments_valid(0, std::ptr::null_mut()));
        let mut fixed = wide(SERVICE_NAME);
        let mut fixed_ptr = fixed.as_mut_ptr();
        assert!(service_start_arguments_valid(1, &mut fixed_ptr));
        assert!(!service_start_arguments_valid(2, &mut fixed_ptr));
        assert!(!service_start_arguments_valid(1, std::ptr::null_mut()));
        let mut foreign = wide("OtherService");
        let mut foreign_ptr = foreign.as_mut_ptr();
        assert!(!service_start_arguments_valid(1, &mut foreign_ptr));
    }
}
