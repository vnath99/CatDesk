//! Read-only self/clock witness for the proposed fixed install broker.
//!
//! A diagnostic can exist only inside the exact running SCM service process.
//! It borrows the protected root and global ledger lock, and retains service,
//! process and image handles. It cannot produce an approval observation or an
//! installation lease. The OS wall clock and the currently pinned image file
//! are not independent time or historical loaded-image-section proof.

use crate::{
    Result,
    install_broker_approval::ObjectIdentityV1,
    install_broker_client::require_fixed_service_process_identity,
    install_broker_ledger::BrokerLedgerGuard,
    install_broker_policy::BrokerProtectedPolicyV1,
    install_broker_preflight::{
        BrokerReadOnlySnapshotV1, FixedProtectedRootReadGuard, SERVICE_IMAGE, SERVICE_NAME,
        inspect_fixed_broker_read_only, protected_security_raw,
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, c_void},
    fs::File,
    io::{Read, Seek, SeekFrom},
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr,
};

const SC_MANAGER_CONNECT: u32 = 1;
const SERVICE_QUERY_STATUS: u32 = 4;
const SC_STATUS_PROCESS_INFO: u32 = 0;
const SERVICE_WIN32_OWN_PROCESS: u32 = 0x10;
const SERVICE_RUNNING: u32 = 4;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const FILE_READ_DATA: u32 = 1;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const READ_CONTROL: u32 = 0x0002_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_READ: u32 = 1;
const OPEN_EXISTING: u32 = 3;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const WAIT_TIMEOUT: u32 = 258;
const MAX_IMAGE_BYTES: u64 = 80 * 1024 * 1024;
const FILETIME_UNIX_EPOCH_100NS: u64 = 116_444_736_000_000_000;
const TICKS_PER_SECOND: u64 = 10_000_000;
const MAX_UNIX_SECONDS: u64 = 253_402_300_799; // 9999-12-31 UTC
const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ServiceStatusProcess {
    service_type: u32,
    current_state: u32,
    controls_accepted: u32,
    win32_exit_code: u32,
    service_specific_exit_code: u32,
    checkpoint: u32,
    wait_hint: u32,
    process_id: u32,
    service_flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FileTime {
    low: u32,
    high: u32,
}

impl FileTime {
    fn as_u64(self) -> u64 {
        (u64::from(self.high) << 32) | u64::from(self.low)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FileInfo {
    attributes: u32,
    creation_low: u32,
    creation_high: u32,
    access_low: u32,
    access_high: u32,
    write_low: u32,
    write_high: u32,
    volume: u32,
    size_high: u32,
    size_low: u32,
    links: u32,
    index_high: u32,
    index_low: u32,
}

impl FileInfo {
    fn length(self) -> u64 {
        (u64::from(self.size_high) << 32) | u64::from(self.size_low)
    }

    fn identity(self) -> Result<ObjectIdentityV1> {
        if self.volume == 0 || (self.index_high == 0 && self.index_low == 0) {
            return Err("WAKE_BROKER_SELF_OBJECT_ID_INVALID".into());
        }
        Ok(ObjectIdentityV1 {
            volume_serial: self.volume,
            file_index_high: self.index_high,
            file_index_low: self.index_low,
        })
    }

    fn stable_with(self, other: Self) -> bool {
        self.attributes == other.attributes
            && self.creation_low == other.creation_low
            && self.creation_high == other.creation_high
            && self.write_low == other.write_low
            && self.write_high == other.write_high
            && self.volume == other.volume
            && self.size_high == other.size_high
            && self.size_low == other.size_low
            && self.links == other.links
            && self.index_high == other.index_high
            && self.index_low == other.index_low
    }
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenSCManagerW(machine: *const u16, database: *const u16, access: u32) -> *mut c_void;
    fn OpenServiceW(manager: *mut c_void, name: *const u16, access: u32) -> *mut c_void;
    fn QueryServiceStatusEx(
        service: *mut c_void,
        level: u32,
        output: *mut u8,
        size: u32,
        needed: *mut u32,
    ) -> i32;
    fn CloseServiceHandle(service: *mut c_void) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcessId() -> u32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn GetProcessId(process: *mut c_void) -> u32;
    fn GetProcessTimes(
        process: *mut c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, millis: u32) -> u32;
    fn GetSystemTimePreciseAsFileTime(time: *mut FileTime);
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *mut c_void,
        disposition: u32,
        flags: u32,
        template: *mut c_void,
    ) -> *mut c_void;
    fn GetFileInformationByHandle(handle: *mut c_void, info: *mut FileInfo) -> i32;
}

struct ServiceHandle(*mut c_void);

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseServiceHandle(self.0) };
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain([0]).collect()
}

fn fixed_service() -> Result<ServiceHandle> {
    let manager =
        ServiceHandle(unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT) });
    if manager.0.is_null() {
        return Err("WAKE_BROKER_SELF_SCM_UNAVAILABLE".into());
    }
    let service = ServiceHandle(unsafe {
        OpenServiceW(manager.0, wide(SERVICE_NAME).as_ptr(), SERVICE_QUERY_STATUS)
    });
    if service.0.is_null() {
        return Err("WAKE_BROKER_SELF_SERVICE_UNPROVISIONED".into());
    }
    Ok(service)
}

fn service_status(service: &ServiceHandle) -> Result<ServiceStatusProcess> {
    let mut status = ServiceStatusProcess::default();
    let mut needed = 0;
    if unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut ServiceStatusProcess).cast(),
            size_of::<ServiceStatusProcess>() as u32,
            &mut needed,
        )
    } == 0
        || needed > size_of::<ServiceStatusProcess>() as u32
    {
        return Err("WAKE_BROKER_SELF_SERVICE_STATUS_INVALID".into());
    }
    Ok(status)
}

fn require_self_service(status: ServiceStatusProcess, self_pid: u32) -> Result<()> {
    if self_pid == 0
        || status.current_state != SERVICE_RUNNING
        || status.service_type != SERVICE_WIN32_OWN_PROCESS
        || status.service_flags != 0
        || status.process_id != self_pid
    {
        return Err("WAKE_BROKER_SELF_SERVICE_MISMATCH".into());
    }
    Ok(())
}

fn process_creation(process: &OwnedHandle) -> Result<u64> {
    let mut creation = FileTime::default();
    let mut exit = FileTime::default();
    let mut kernel = FileTime::default();
    let mut user = FileTime::default();
    if unsafe {
        GetProcessTimes(
            process.as_raw_handle(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
        || creation.as_u64() == 0
    {
        return Err("WAKE_BROKER_SELF_PROCESS_INVALID".into());
    }
    Ok(creation.as_u64())
}

fn require_live_process(process: &OwnedHandle, pid: u32, creation: u64) -> Result<()> {
    if unsafe { GetProcessId(process.as_raw_handle()) } != pid
        || unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } != WAIT_TIMEOUT
        || process_creation(process)? != creation
    {
        return Err("WAKE_BROKER_SELF_PROCESS_CHANGED".into());
    }
    Ok(())
}

fn pin_process(pid: u32) -> Result<(OwnedHandle, u64)> {
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err("WAKE_BROKER_SELF_PROCESS_UNAVAILABLE".into());
    }
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    let creation = process_creation(&process)?;
    require_live_process(&process, pid, creation)?;
    Ok((process, creation))
}

fn file_info(handle: *mut c_void, directory: bool) -> Result<FileInfo> {
    let mut info = FileInfo::default();
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0
        || (info.attributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (!directory && info.links != 1)
    {
        return Err("WAKE_BROKER_SELF_OBJECT_INVALID".into());
    }
    info.identity()?;
    Ok(info)
}

fn open_fixed_image() -> Result<File> {
    // The separate fixed-name handle corroborates the preflight physical ID,
    // bytes and ACL. Image ancestor handles are not retained by this probe,
    // so this read-only pin cannot authorize a later path-based launch.
    let raw = unsafe {
        CreateFileW(
            wide(SERVICE_IMAGE).as_ptr(),
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            FILE_SHARE_READ,
            ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err("WAKE_BROKER_SELF_IMAGE_UNAVAILABLE".into());
    }
    let file = unsafe { File::from_raw_handle(raw) };
    file_info(file.as_raw_handle(), false)?;
    Ok(file)
}

fn pinned_image_readback(file: &mut File) -> Result<(FileInfo, String, String)> {
    let before = file_info(file.as_raw_handle(), false)?;
    if before.length() == 0 || before.length() > MAX_IMAGE_BYTES {
        return Err("WAKE_BROKER_SELF_IMAGE_INVALID".into());
    }
    let (owner, sddl) = protected_security_raw(file.as_raw_handle())?;
    if owner != "S-1-5-18" {
        return Err("WAKE_BROKER_SELF_IMAGE_SECURITY_MISMATCH".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_BROKER_SELF_IMAGE_READ_FAILED")?;
    let mut bytes = Vec::with_capacity(before.length() as usize);
    (&mut *file)
        .take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WAKE_BROKER_SELF_IMAGE_READ_FAILED")?;
    let after = file_info(file.as_raw_handle(), false)?;
    let after_security = protected_security_raw(file.as_raw_handle())?;
    if !before.stable_with(after)
        || bytes.len() as u64 != before.length()
        || after_security != (owner, sddl.clone())
    {
        return Err("WAKE_BROKER_SELF_IMAGE_CHANGED".into());
    }
    Ok((
        before,
        format!("{:x}", Sha256::digest(bytes)),
        format!("{:x}", Sha256::digest(sddl.as_bytes())),
    ))
}

fn unix_seconds(filetime: u64) -> Result<u64> {
    let ticks = filetime
        .checked_sub(FILETIME_UNIX_EPOCH_100NS)
        .ok_or("WAKE_BROKER_SELF_CLOCK_INVALID")?;
    let seconds = ticks / TICKS_PER_SECOND;
    if seconds > MAX_UNIX_SECONDS {
        return Err("WAKE_BROKER_SELF_CLOCK_INVALID".into());
    }
    Ok(seconds)
}

fn checked_clock_seconds(filetime: u64, creation: u64, floor: u64, prior: u64) -> Result<u64> {
    let now = unix_seconds(filetime)?;
    if now == 0 || filetime < creation || now < floor || now < prior {
        return Err("WAKE_BROKER_SELF_CLOCK_ROLLBACK".into());
    }
    Ok(now)
}

fn system_time_filetime() -> u64 {
    let mut time = FileTime::default();
    unsafe { GetSystemTimePreciseAsFileTime(&mut time) };
    time.as_u64()
}

/// Copied readback only. It never conveys handles, a verified predecessor or
/// permission to call the broker ledger's acceptance or resume path.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerSelfClockReadbackV1 {
    pub schema_version: u32,
    pub service_name: &'static str,
    pub service_pid: u32,
    pub service_creation_time_100ns: u64,
    pub sampled_at_utc_seconds: u64,
    pub protected_ledger_clock_floor_utc_seconds: Option<u64>,
    pub protected_root: ObjectIdentityV1,
    pub service_image: ObjectIdentityV1,
    pub service_image_sha256: String,
    pub disposition: &'static str,
}

/// Read-only retained pins. The borrowed ledger guard keeps its global lock
/// and the fixed root guard keeps its no-delete-share ancestor chain open.
pub struct BrokerSelfClockPin<'root, 'guard, 'policy> {
    fixed_root: &'root FixedProtectedRootReadGuard,
    ledger: &'guard BrokerLedgerGuard<'root>,
    policy: &'policy BrokerProtectedPolicyV1,
    service: ServiceHandle,
    process: OwnedHandle,
    image: File,
    snapshot: BrokerReadOnlySnapshotV1,
    image_info: FileInfo,
    service_pid: u32,
    process_creation: u64,
    clock_floor: u64,
    last_filetime_100ns: u64,
    last_utc_seconds: u64,
}

impl<'root, 'guard, 'policy> BrokerSelfClockPin<'root, 'guard, 'policy> {
    /// This constructor has no caller-selected process, path or clock. It can
    /// succeed only inside the fixed running service process, with the exact
    /// protected root and policy already pinned under the ledger's global lock.
    pub fn acquire(
        fixed_root: &'root FixedProtectedRootReadGuard,
        ledger: &'guard BrokerLedgerGuard<'root>,
        policy: &'policy BrokerProtectedPolicyV1,
    ) -> Result<Self> {
        let policy_root = policy.readback()?.protected_root;
        if policy_root != *fixed_root.root_identity()
            || file_info(ledger.root().as_raw_handle(), true)?.identity()? != policy_root
        {
            return Err("WAKE_BROKER_SELF_ROOT_MISMATCH".into());
        }
        let snapshot = inspect_fixed_broker_read_only()?;
        if snapshot.protected_root != policy_root {
            return Err("WAKE_BROKER_SELF_ROOT_MISMATCH".into());
        }
        let self_pid = unsafe { GetCurrentProcessId() };
        let service = fixed_service()?;
        require_self_service(service_status(&service)?, self_pid)?;
        let (process, process_creation) = pin_process(self_pid)?;
        require_fixed_service_process_identity(&process, fixed_root.service_sid())?;
        let mut image = open_fixed_image()?;
        let (image_info, digest, security_digest) = pinned_image_readback(&mut image)?;
        if image_info.identity()? != snapshot.service_image
            || digest != snapshot.service_image_sha256
            || security_digest != snapshot.service_image_security_sha256
        {
            return Err("WAKE_BROKER_SELF_IMAGE_MISMATCH".into());
        }
        let clock_floor = ledger.latest_accepted_at_utc_seconds(policy)?.unwrap_or(0);
        let mut pin = Self {
            fixed_root,
            ledger,
            policy,
            service,
            process,
            image,
            snapshot,
            image_info,
            service_pid: self_pid,
            process_creation,
            clock_floor,
            last_filetime_100ns: 0,
            last_utc_seconds: 0,
        };
        pin.readback()?;
        Ok(pin)
    }

    /// Re-sample time only while all pins and the global lock remain live.
    /// No readback is usable as `BrokerObservedStateV1` or an install lease.
    pub fn readback(&mut self) -> Result<BrokerSelfClockReadbackV1> {
        let policy_root = self.policy.readback()?.protected_root;
        if policy_root != *self.fixed_root.root_identity()
            || file_info(self.fixed_root.root().as_raw_handle(), true)?.identity()? != policy_root
            || file_info(self.ledger.root().as_raw_handle(), true)?.identity()? != policy_root
            || self
                .ledger
                .latest_accepted_at_utc_seconds(self.policy)?
                .unwrap_or(0)
                != self.clock_floor
        {
            return Err("WAKE_BROKER_SELF_ROOT_OR_LEDGER_CHANGED".into());
        }
        require_self_service(service_status(&self.service)?, self.service_pid)?;
        require_live_process(&self.process, self.service_pid, self.process_creation)?;
        require_fixed_service_process_identity(&self.process, self.fixed_root.service_sid())?;
        let before_raw = system_time_filetime();
        if before_raw < self.last_filetime_100ns {
            return Err("WAKE_BROKER_SELF_CLOCK_ROLLBACK".into());
        }
        let before = checked_clock_seconds(
            before_raw,
            self.process_creation,
            self.clock_floor,
            self.last_utc_seconds,
        )?;
        let snapshot = inspect_fixed_broker_read_only()?;
        let (image_info, digest, security_digest) = pinned_image_readback(&mut self.image)?;
        if snapshot != self.snapshot
            || !self.image_info.stable_with(image_info)
            || image_info.identity()? != self.snapshot.service_image
            || digest != self.snapshot.service_image_sha256
            || security_digest != self.snapshot.service_image_security_sha256
        {
            return Err("WAKE_BROKER_SELF_IMAGE_OR_PREFLIGHT_CHANGED".into());
        }
        require_self_service(service_status(&self.service)?, self.service_pid)?;
        require_live_process(&self.process, self.service_pid, self.process_creation)?;
        require_fixed_service_process_identity(&self.process, self.fixed_root.service_sid())?;
        let after_raw = system_time_filetime();
        let after =
            checked_clock_seconds(after_raw, self.process_creation, self.clock_floor, before)?;
        if after_raw < before_raw {
            return Err("WAKE_BROKER_SELF_CLOCK_ROLLBACK".into());
        }
        self.last_filetime_100ns = after_raw;
        self.last_utc_seconds = after;
        Ok(BrokerSelfClockReadbackV1 {
            schema_version: 1,
            service_name: SERVICE_NAME,
            service_pid: self.service_pid,
            service_creation_time_100ns: self.process_creation,
            sampled_at_utc_seconds: after,
            protected_ledger_clock_floor_utc_seconds: (self.clock_floor != 0)
                .then_some(self.clock_floor),
            protected_root: policy_root,
            service_image: self.snapshot.service_image.clone(),
            service_image_sha256: self.snapshot.service_image_sha256.clone(),
            disposition: "READ_ONLY_SELF_CLOCK_PIN_NO_INSTALL_AUTHORITY",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn running(pid: u32) -> ServiceStatusProcess {
        ServiceStatusProcess {
            service_type: SERVICE_WIN32_OWN_PROCESS,
            current_state: SERVICE_RUNNING,
            process_id: pid,
            ..ServiceStatusProcess::default()
        }
    }

    #[test]
    fn fixed_service_status_must_name_this_running_own_process() {
        let pid = unsafe { GetCurrentProcessId() };
        assert!(require_self_service(running(pid), pid).is_ok());
        let mut cases = Vec::new();
        cases.push(running(0));
        cases.push(running(pid.wrapping_add(1)));
        for state in [1, 2, 3, 5, 6, 7] {
            cases.push(ServiceStatusProcess {
                current_state: state,
                ..running(pid)
            });
        }
        cases.push(ServiceStatusProcess {
            service_type: 0x20, // shared process
            ..running(pid)
        });
        cases.push(ServiceStatusProcess {
            service_flags: 1, // system process
            ..running(pid)
        });
        for status in cases {
            assert_eq!(
                require_self_service(status, pid).unwrap_err(),
                "WAKE_BROKER_SELF_SERVICE_MISMATCH"
            );
        }
        assert_eq!(
            require_self_service(running(pid), 0).unwrap_err(),
            "WAKE_BROKER_SELF_SERVICE_MISMATCH"
        );
    }

    #[test]
    fn process_handle_rejects_wrong_pid_creation_and_exit() {
        let pid = unsafe { GetCurrentProcessId() };
        let (process, creation) = pin_process(pid).expect("pin current process");
        require_live_process(&process, pid, creation).expect("live exact process");
        assert_eq!(
            require_live_process(&process, pid.wrapping_add(1), creation).unwrap_err(),
            "WAKE_BROKER_SELF_PROCESS_CHANGED"
        );
        assert_eq!(
            require_live_process(&process, pid, creation.wrapping_add(1)).unwrap_err(),
            "WAKE_BROKER_SELF_PROCESS_CHANGED"
        );
        assert_eq!(
            pin_process(0).err().unwrap(),
            "WAKE_BROKER_SELF_PROCESS_UNAVAILABLE"
        );

        // `more` blocks on the owned pipe until stdin closes, giving the test
        // a deterministic child process object without a filesystem fixture.
        let mut child = Command::new("cmd")
            .args(["/C", "more"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn isolated child");
        let child_pid = child.id();
        let (child_process, child_creation) = pin_process(child_pid).expect("pin child");
        drop(child.stdin.take());
        child.wait().expect("child exits after pipe closes");
        assert_eq!(
            require_live_process(&child_process, child_pid, child_creation).unwrap_err(),
            "WAKE_BROKER_SELF_PROCESS_CHANGED"
        );
    }

    #[test]
    fn utc_filetime_conversion_and_rollback_are_bounded() {
        let epoch = FILETIME_UNIX_EPOCH_100NS;
        assert_eq!(
            unix_seconds(epoch - 1).unwrap_err(),
            "WAKE_BROKER_SELF_CLOCK_INVALID"
        );
        assert_eq!(unix_seconds(epoch).unwrap(), 0);
        assert_eq!(unix_seconds(epoch + TICKS_PER_SECOND - 1).unwrap(), 0);
        assert_eq!(unix_seconds(epoch + TICKS_PER_SECOND).unwrap(), 1);
        assert_eq!(
            unix_seconds(u64::MAX).unwrap_err(),
            "WAKE_BROKER_SELF_CLOCK_INVALID"
        );
        let creation = epoch + 2 * TICKS_PER_SECOND;
        let sample = epoch + 3 * TICKS_PER_SECOND;
        assert_eq!(checked_clock_seconds(sample, creation, 2, 2).unwrap(), 3);
        for (raw, floor, prior) in [
            (creation - 1, 0, 0),
            (sample, 4, 0),
            (sample, 0, 4),
            (epoch, 0, 0),
        ] {
            assert_eq!(
                checked_clock_seconds(raw, creation, floor, prior).unwrap_err(),
                "WAKE_BROKER_SELF_CLOCK_ROLLBACK"
            );
        }
    }
}
