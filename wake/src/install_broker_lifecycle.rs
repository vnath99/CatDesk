//! Explicit native operator start of the existing fixed one-request broker.
//! This starts no install transaction and creates or configures no resource.
use crate::{
    Result,
    install_broker_offline_pins::OfflineObjectPinV2,
    install_broker_policy::{BrokerProtectedPolicyV1, load_fixed_broker_policy_read_only},
    install_broker_preflight::{
        SERVICE_IMAGE, SERVICE_NAME, inspect_fixed_broker_with_policy,
        verify_fixed_broker_service_handle,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    mem::size_of,
    path::Path,
    ptr, thread,
    time::{Duration, Instant},
};

const SERVICE_STOPPED: u32 = 1;
const SERVICE_START_PENDING: u32 = 2;
const SERVICE_RUNNING: u32 = 4;
const POLL_SECONDS: u64 = 15;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct ServiceStatus {
    service_type: u32,
    current_state: u32,
    controls_accepted: u32,
    win32_exit_code: u32,
    service_specific_exit_code: u32,
    check_point: u32,
    wait_hint: u32,
    process_id: u32,
    service_flags: u32,
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenSCManagerW(machine: *const u16, database: *const u16, access: u32) -> *mut c_void;
    fn OpenServiceW(manager: *mut c_void, name: *const u16, access: u32) -> *mut c_void;
    fn StartServiceW(service: *mut c_void, count: u32, args: *const *const u16) -> i32;
    fn QueryServiceStatusEx(
        service: *mut c_void,
        level: u32,
        buffer: *mut u8,
        size: u32,
        needed: *mut u32,
    ) -> i32;
    fn CloseServiceHandle(service: *mut c_void) -> i32;
}

struct ServiceHandle(*mut c_void);
impl Drop for ServiceHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseServiceHandle(self.0) };
        }
    }
}

fn require_start_authorized(allowed: bool) -> Result<()> {
    if !allowed {
        return Err("WAKE_BROKER_START_NOT_AUTHORIZED".into());
    }
    Ok(())
}

fn require_start_mode_authorized(
    start_allowed: bool,
    local_update: bool,
    local_updates_enrolled: bool,
) -> Result<()> {
    require_start_authorized(start_allowed)?;
    if local_update && !local_updates_enrolled {
        return Err("WAKE_LOCAL_UPDATE_POLICY_REQUIRED".into());
    }
    Ok(())
}

fn current_start_caller(policy: &BrokerProtectedPolicyV1, local_update: bool) -> Result<String> {
    if local_update {
        Ok(policy
            .current_process_local_updater()?
            .user_sid_sha256()
            .into())
    } else {
        Ok(policy
            .current_process_operator()?
            .operator_sid_sha256()
            .into())
    }
}

fn valid_status(status: ServiceStatus) -> Result<u32> {
    if status.service_type != 0x10
        || status.service_flags != 0
        || (status.current_state == SERVICE_STOPPED && status.process_id != 0)
        || (status.current_state == SERVICE_RUNNING && status.process_id == 0)
        || !matches!(
            status.current_state,
            SERVICE_STOPPED | SERVICE_START_PENDING | SERVICE_RUNNING
        )
    {
        return Err("WAKE_BROKER_START_STATUS_INVALID".into());
    }
    Ok(status.current_state)
}

fn status(service: &ServiceHandle) -> Result<ServiceStatus> {
    let mut value = ServiceStatus::default();
    let mut needed = 0;
    if unsafe {
        QueryServiceStatusEx(
            service.0,
            0,
            (&mut value as *mut ServiceStatus).cast(),
            size_of::<ServiceStatus>() as u32,
            &mut needed,
        )
    } == 0
        || needed > size_of::<ServiceStatus>() as u32
    {
        return Err("WAKE_BROKER_START_STATUS_UNAVAILABLE".into());
    }
    valid_status(value)?;
    Ok(value)
}

/// Only this explicit native command may start the fixed demand-start broker.
/// The fixed polling window does not cancel a synchronous Windows SCM call;
/// the qualified enclosing operator owner supplies the hard execution bound.
pub fn start_fixed_broker_once() -> Result<Value> {
    start_fixed_broker(false)
}

/// Start only the existing fixed one-request service under enrolled local-user
/// update policy. This supplies no install, selection or maintenance authority.
pub fn start_fixed_broker_for_local_update() -> Result<Value> {
    start_fixed_broker(true)
}

fn start_fixed_broker(local_update: bool) -> Result<Value> {
    let policy = load_fixed_broker_policy_read_only()?;
    require_start_mode_authorized(
        policy.operator_service_start_sid()?.is_some(),
        local_update,
        local_update && policy.local_user_updates_enabled()?,
    )?;
    let caller_sid_sha256 = current_start_caller(&policy, local_update)?;
    let before = inspect_fixed_broker_with_policy(&policy)?;
    let image_path = Path::new(SERVICE_IMAGE);
    let _image_parents = image_path
        .ancestors()
        .skip(1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(OfflineObjectPinV2::same_state_directory)
        .collect::<Result<Vec<_>>>()?;
    let mut image = OfflineObjectPinV2::open(image_path, false)?;
    if image.identity().full != before.service_image_full
        || format!(
            "{:x}",
            Sha256::digest(image.read_bounded(80 * 1024 * 1024)?)
        ) != before.service_image_sha256
        || inspect_fixed_broker_with_policy(&policy)? != before
    {
        return Err("WAKE_BROKER_START_AUTHORITY_CHANGED".into());
    }
    let manager = ServiceHandle(unsafe { OpenSCManagerW(ptr::null(), ptr::null(), 1) });
    if manager.0.is_null() {
        return Err("WAKE_BROKER_START_SCM_UNAVAILABLE".into());
    }
    let name = SERVICE_NAME.encode_utf16().chain([0]).collect::<Vec<_>>();
    // START plus query/readback only; never STOP/config/DACL/owner/delete.
    let service = ServiceHandle(unsafe { OpenServiceW(manager.0, name.as_ptr(), 0x0002_0015) });
    if service.0.is_null() {
        return Err("WAKE_BROKER_START_HANDLE_UNAVAILABLE".into());
    }
    verify_fixed_broker_service_handle(service.0, &policy)?;
    let initial = status(&service)?;
    let started = match valid_status(initial)? {
        SERVICE_STOPPED => {
            current_start_caller(&policy, local_update)?;
            if unsafe { StartServiceW(service.0, 0, ptr::null()) } == 0 {
                return Err("WAKE_BROKER_START_FAILED_NO_RETRY".into());
            }
            true
        }
        SERVICE_RUNNING => false,
        _ => return Err("WAKE_BROKER_START_EXISTING_OWNER_PENDING".into()),
    };
    let deadline = Instant::now() + Duration::from_secs(POLL_SECONDS);
    let current = loop {
        if Instant::now() >= deadline {
            return Err("WAKE_BROKER_START_STATUS_TIMEOUT_NO_RETRY".into());
        }
        let current = status(&service)?;
        if valid_status(current)? == SERVICE_RUNNING {
            break current;
        }
        if current.current_state != SERVICE_START_PENDING {
            return Err("WAKE_BROKER_START_DID_NOT_REACH_RUNNING".into());
        }
        thread::sleep(Duration::from_millis(50));
    };
    verify_fixed_broker_service_handle(service.0, &policy)?;
    image.revalidate_path(image_path)?;
    if inspect_fixed_broker_with_policy(&policy)? != before {
        return Err("WAKE_BROKER_START_AUTHORITY_CHANGED".into());
    }
    let mut result = json!({"action":"start_fixed_install_broker", "status":"SCM_RUNNING_ONLY",
        "serviceName":SERVICE_NAME,"serviceStarted":started,"processId":current.process_id,
        "statusPollMaximumSeconds":POLL_SECONDS,"operatorSidSha256":caller_sid_sha256,
        "serviceImageSha256":before.service_image_sha256,"installationAttempted":false,
        "settingsChanged":false,"approvalSubmitted":false,"transactionReadyVerified":false});
    if local_update {
        let fields = result.as_object_mut().expect("fixed result object");
        let user_sid = fields
            .remove("operatorSidSha256")
            .expect("fixed caller digest");
        fields.insert("userSidSha256".into(), user_sid);
        fields.insert("authorization".into(), "LOCAL_USER_POLICY".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(current_state: u32, process_id: u32) -> ServiceStatus {
        ServiceStatus {
            service_type: 0x10,
            current_state,
            process_id,
            ..Default::default()
        }
    }
    #[test]
    fn lifecycle_start_requires_explicit_policy_authorization() {
        assert_eq!(
            require_start_authorized(false).unwrap_err(),
            "WAKE_BROKER_START_NOT_AUTHORIZED"
        );
        assert!(require_start_authorized(true).is_ok());
    }
    #[test]
    fn lifecycle_local_start_requires_both_enrollment_and_existing_start_permission() {
        assert_eq!(
            require_start_mode_authorized(false, true, true).unwrap_err(),
            "WAKE_BROKER_START_NOT_AUTHORIZED"
        );
        assert_eq!(
            require_start_mode_authorized(true, true, false).unwrap_err(),
            "WAKE_LOCAL_UPDATE_POLICY_REQUIRED"
        );
        assert!(require_start_mode_authorized(true, true, true).is_ok());
        assert!(require_start_mode_authorized(true, false, false).is_ok());
        assert!(require_start_mode_authorized(false, false, true).is_err());
    }
    #[test]
    fn lifecycle_start_status_refuses_unknown_shared_and_inconsistent_owner() {
        assert_eq!(
            valid_status(state(SERVICE_STOPPED, 0)).unwrap(),
            SERVICE_STOPPED
        );
        assert_eq!(
            valid_status(state(SERVICE_RUNNING, 7)).unwrap(),
            SERVICE_RUNNING
        );
        assert!(valid_status(state(SERVICE_STOPPED, 7)).is_err());
        assert!(valid_status(state(SERVICE_RUNNING, 0)).is_err());
        assert!(valid_status(state(3, 7)).is_err());
        let mut changed = state(SERVICE_RUNNING, 7);
        changed.service_type = 0x20;
        assert!(valid_status(changed).is_err());
        changed = state(SERVICE_RUNNING, 7);
        changed.service_flags = 1;
        assert!(valid_status(changed).is_err());
    }
}
