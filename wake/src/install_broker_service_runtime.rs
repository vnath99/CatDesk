//! Fixed Windows SCM entry point for the separately provisioned Wake broker.
//!
//! The legacy V1 transaction remains refused. The distinct same-state V3
//! operation requires its separately provisioned policy, admitted native
//! operator, signed local qualification, retained physical resources and
//! protected phase journal before a selection can change. The fixed service
//! image/key/namespace must be provisioned by an administrator; this module
//! creates none of them and accepts no caller-selected state path.

#![cfg(windows)]

use sha2::{Digest, Sha256};
use std::ffi::{OsStr, c_void};
use std::os::windows::ffi::OsStrExt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use crate::Result;
use crate::install_broker_current_observer::BrokerSelfClockPin;
use crate::install_broker_endpoint::create_fixed_broker_endpoint_from_protected_policy;
use crate::install_broker_ledger::BrokerLedgerGuard;
use crate::install_broker_pipe_io::{connect_once, write_once};
use crate::install_broker_policy::load_fixed_broker_policy_read_only;
use crate::install_broker_preflight::{SERVICE_NAME, acquire_fixed_protected_root_for_reader};
use crate::install_broker_request::read_admitted_update_frame;

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
const REPLY_DEADLINE: Duration = Duration::from_secs(10);

fn is_same_state_selection_request(bytes: &[u8]) -> Result<bool> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "WAKE_BROKER_REQUEST_ENCODING_INVALID")?;
    if value["payload"]["schemaVersion"] != 3 {
        return Ok(false);
    }
    if !matches!(
        value["payload"]["operation"].as_str(),
        Some("SELECT_SAME_STATE_RELEASE" | "ROLLBACK_SAME_STATE_RELEASE")
    ) {
        return Err("WAKE_BROKER_REQUEST_OPERATION_UNSUPPORTED".into());
    }
    Ok(true)
}

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
    // The enrolled account must still attest the actual restricted SYSTEM
    // process and token. Establish only those ephemeral read rights before
    // exposing the endpoint; partial failure exits this one-request service.
    if policy.local_user_updates_enabled()? {
        crate::install_broker_local_access::authorize_enrolled_client_readback(&policy)?;
    }
    let mut endpoint = create_fixed_broker_endpoint_from_protected_policy(&policy)?;
    connect_once(endpoint.as_handle(), CONNECT_DEADLINE, &STOP_REQUESTED)?;
    let frame = read_admitted_update_frame(&mut endpoint, &policy, READ_DEADLINE, &STOP_REQUESTED)?;
    // Keep every pin and the global lock live after the admitted last read.
    let observed = clock.readback()?;
    policy.revalidate()?;
    if frame.approval_envelope().is_empty() {
        return Err("WAKE_BROKER_REQUEST_SIZE_INVALID".into());
    }
    if is_same_state_selection_request(frame.approval_envelope())? {
        // The operator token belongs to this single completed read. Policy
        // derives the sole state path; no request path or observer is trusted.
        let envelope_sha256 = format!("{:x}", Sha256::digest(frame.approval_envelope()));
        let admission =
            ledger.same_state_admission_v3(&envelope_sha256, observed.sampled_at_utc_seconds)?;
        let purpose: serde_json::Value = serde_json::from_slice(frame.approval_envelope())
            .map_err(|_| "WAKE_BROKER_REQUEST_ENCODING_INVALID")?;
        let operation = purpose["payload"]["operation"]
            .as_str()
            .ok_or("WAKE_BROKER_REQUEST_OPERATION_UNSUPPORTED")?;
        let pins = crate::runtime::SameStateReadPinsV3::observe_for_operation(
            &policy, &admission, operation,
        )?;
        let grant = policy.verify_same_state_selection_v3(&frame, pins, &admission)?;
        let resources = crate::runtime::SameStateInstallResources::acquire(grant)?;
        // acceptedUTC is the protected signature-verification sample, as in
        // V1. A separate final sample must still be in the fresh approval's
        // validity window immediately before INTENT; resume keeps originalUTC.
        let final_clock = clock.readback()?;
        resources
            .grant()
            .require_acceptance_time(final_clock.sampled_at_utc_seconds)?;
        resources.grant().revalidate()?;
        let original = resources.grant().original_selection_bytes()?;
        drop(clock); // global lock is moved, never released, into the lease.
        let mut transaction =
            ledger.begin_same_state_v3(resources.grant(), &admission, &original)?;
        let outcome = resources.commit_selection(&mut transaction)?;
        let complete_sha256 = transaction.complete_record_sha256()?.to_owned();
        let binding = transaction.binding();
        let mut reply = serde_json::json!({"schemaVersion":3,"operation":operation,
            "approvalId":binding.approval_id,"installGeneration":binding.install_generation,"phase":"COMPLETE",
            "signedEnvelopeSha256":binding.signed_envelope_sha256,"selectedSha256":binding.planned_selection_sha256,
            "completeRecordSha256":complete_sha256,"runtimeState":"STOPPED",
            "browserOrWakeStarted":false,"liveAcceptanceVerified":false});
        if binding.local_user_authorized() {
            let object = reply
                .as_object_mut()
                .ok_or("WAKE_SAME_STATE_RESULT_ENCODING_INVALID")?;
            object.remove("signedEnvelopeSha256");
            object.insert("authorization".into(), "LOCAL_USER_POLICY".into());
            object.insert(
                "requestSha256".into(),
                binding.signed_envelope_sha256.clone().into(),
            );
        }
        if operation == "ROLLBACK_SAME_STATE_RELEASE" {
            reply["failedCurrentImages"] = outcome["failedCurrentImages"].clone();
        }
        let payload: serde_json::Value =
            serde_json::from_slice::<serde_json::Value>(frame.approval_envelope())
                .map_err(|_| "WAKE_BROKER_REQUEST_ENCODING_INVALID")?["payload"]
                .clone();
        let reply_matches = if binding.local_user_authorized() {
            crate::install_broker_client::local_user_complete_reply_matches(
                &reply,
                &payload,
                &envelope_sha256,
            )
        } else {
            crate::install_broker_client::same_state_complete_reply_matches(
                &reply,
                &payload,
                &envelope_sha256,
            )
        };
        if !reply_matches {
            return Err("WAKE_SAME_STATE_RESULT_BINDING_INVALID".into());
        }
        // A lost reply never rolls back a durable COMPLETE or permits a new
        // approval. Recovery inspects the same protected transaction first.
        let raw =
            serde_json::to_vec(&reply).map_err(|_| "WAKE_SAME_STATE_RESULT_ENCODING_INVALID")?;
        write_once(
            endpoint.as_handle(),
            &raw,
            8192,
            REPLY_DEADLINE,
            &STOP_REQUESTED,
        )?;
        return Ok(());
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
    fn same_state_dispatch_is_versioned_without_reclassifying_legacy_requests() {
        assert!(
            !is_same_state_selection_request(
                br#"{"payload":{"schemaVersion":1,"operation":"INSTALL"}}"#
            )
            .unwrap()
        );
        assert!(
            is_same_state_selection_request(
                br#"{"payload":{"schemaVersion":3,"operation":"SELECT_SAME_STATE_RELEASE"}}"#
            )
            .unwrap()
        );
        assert!(
            is_same_state_selection_request(
                br#"{"payload":{"schemaVersion":3,"operation":"ROLLBACK_SAME_STATE_RELEASE"}}"#
            )
            .unwrap()
        );
        assert!(
            is_same_state_selection_request(
                br#"{"payload":{"schemaVersion":3,"operation":"OTHER"}}"#
            )
            .is_err()
        );
        assert!(is_same_state_selection_request(b"not json").is_err());
    }

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
