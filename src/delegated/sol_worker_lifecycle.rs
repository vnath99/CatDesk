//! Dormant Windows process-tree ownership mechanics for a future Sol launcher.
//!
//! The prepared path creates a suspended child inside an unnamed,
//! noninheritable kill-on-close Job with a creation-time Job-list attribute,
//! checks exact membership, and only then resumes it. An older same-user
//! Command diagnostic still assigns after creation and returns no capability. A
//! separate dormant route can bind the still-suspended process object to the
//! opaque fixed-account restricted-token observation before resume, then
//! consume that owner to obtain a narrow drained-Job witness. A private run
//! preparation captures the host task, one selected nonce, and pinned ledger
//! and empty output root before launch. A separate retained, digest-checked
//! executable lease is carried from a private pre-spawn transition through
//! Job drain. No production fixed-account launcher can call that route yet.
//! The witness does not prove
//! escaped descendants, inherited handles, or repository-write exclusion,
//! and no nonce, staging, or repository-use path accepts it. Consuming a
//! normally drained witness yields only an inert stopped-run candidate that
//! retains its opened-process/token evidence and exact host-held context for
//! later post-exit checks.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString, c_void};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

#[cfg(test)]
use std::cell::Cell;

use crate::reviewed_source_snapshot::query_relative_directory_names_bounded;
use crate::windows_protected_fs::{
    ProtectedDirectoryGuard, RetainedReviewedArtifact, retain_reviewed_regular_artifact,
};

use super::sol_worker_identity_preflight::BoundFixedLocalSolWorkerProcessV1;
use super::sol_worker_launch_journal::{SolLaunchBindingV1, SolLaunchReservationV1};
use super::sol_worker_nonce::{
    SolWorkerNonceAuthorityV1, issue_sol_worker_nonce, read_active_sol_worker_nonce,
};
use super::sol_worker_output::validate_sol_worker_allowed_paths;

const LABEL: &str = "Sol worker lifecycle";
const CREATE_SUSPENDED: u32 = 0x0000_0004;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
const PROC_THREAD_ATTRIBUTE_JOB_LIST: usize = 0x0002_000d;
const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
const JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION: i32 = 1;
const JOB_OBJECT_BASIC_PROCESS_ID_LIST: i32 = 3;
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
const JOB_OBJECT_LIMIT_BREAKAWAY_OK: u32 = 0x0000_0800;
const JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK: u32 = 0x0000_1000;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 0x0000_0102;
const MAX_DRAIN: Duration = Duration::from_secs(10);
const FAILED_LAUNCH_CLEANUP: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(10);
const MAX_APPROVED_EXECUTABLE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_IMAGE_PATH_WCHARS: usize = 32 * 1024;
const PROCESS_NAME_NATIVE: u32 = 1;
const CSTR_EQUAL: i32 = 2;
const MAX_LAUNCH_WCHARS: usize = 32_767;
const MAX_ATTRIBUTE_LIST_BYTES: usize = 64 * 1024;

#[repr(C)]
struct StartupInfoW {
    cb: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    x: u32,
    y: u32,
    x_size: u32,
    y_size: u32,
    x_count_chars: u32,
    y_count_chars: u32,
    fill_attribute: u32,
    flags: u32,
    show_window: u16,
    cb_reserved2: u16,
    reserved2: *mut u8,
    std_input: *mut c_void,
    std_output: *mut c_void,
    std_error: *mut c_void,
}

#[repr(C)]
struct StartupInfoExW {
    startup: StartupInfoW,
    attributes: *mut c_void,
}

#[repr(C)]
struct ProcessInformation {
    process: *mut c_void,
    thread: *mut c_void,
    process_id: u32,
    thread_id: u32,
}

#[cfg(test)]
thread_local! {
    static FORCE_JOB_TERMINATE_FAILURE: Cell<bool> = const { Cell::new(false) };
    static FORCE_JOB_ACCOUNTING_FAILURE: Cell<bool> = const { Cell::new(false) };
    static FORCE_PREPARED_OUTPUT_TAMPER_AFTER_SPAWN: Cell<bool> = const { Cell::new(false) };
}

#[repr(C)]
#[derive(Default)]
struct BasicLimits {
    process_time: i64,
    job_time: i64,
    flags: u32,
    min_working_set: usize,
    max_working_set: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[repr(C)]
#[derive(Default)]
struct IoCounters {
    read_operations: u64,
    write_operations: u64,
    other_operations: u64,
    read_bytes: u64,
    write_bytes: u64,
    other_bytes: u64,
}

#[repr(C)]
#[derive(Default)]
struct ExtendedLimits {
    basic: BasicLimits,
    io: IoCounters,
    process_memory: usize,
    job_memory: usize,
    peak_process_memory: usize,
    peak_job_memory: usize,
}

#[repr(C)]
#[derive(Default)]
struct BasicAccounting {
    total_user_time: i64,
    total_kernel_time: i64,
    period_user_time: i64,
    period_kernel_time: i64,
    total_page_fault_count: u32,
    total_processes: u32,
    active_processes: u32,
    total_terminated_processes: u32,
}

/// A fixed one-entry query buffer. When the Job is empty, the two counts are
/// zero; a longer or failed query can never be interpreted as empty.
#[repr(C)]
#[derive(Default)]
struct BasicProcessIdListOne {
    number_of_assigned_processes: u32,
    number_of_process_ids_in_list: u32,
    process_id_list: [usize; 1],
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> *mut c_void;
    fn SetInformationJobObject(job: *mut c_void, class: i32, info: *mut c_void, length: u32)
    -> i32;
    fn QueryInformationJobObject(
        job: *mut c_void,
        class: i32,
        info: *mut c_void,
        length: u32,
        returned: *mut u32,
    ) -> i32;
    fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
    fn IsProcessInJob(process: *mut c_void, job: *mut c_void, result: *mut i32) -> i32;
    fn TerminateJobObject(job: *mut c_void, exit_code: u32) -> i32;
    fn SetHandleInformation(handle: *mut c_void, mask: u32, flags: u32) -> i32;
    fn GetHandleInformation(handle: *mut c_void, flags: *mut u32) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn QueryFullProcessImageNameW(
        process: *mut c_void,
        flags: u32,
        path: *mut u16,
        length: *mut u32,
    ) -> i32;
    fn CompareStringOrdinal(
        first: *const u16,
        first_length: i32,
        second: *const u16,
        second_length: i32,
        ignore_case: i32,
    ) -> i32;
    fn InitializeProcThreadAttributeList(
        list: *mut c_void,
        count: u32,
        flags: u32,
        size: *mut usize,
    ) -> i32;
    fn UpdateProcThreadAttribute(
        list: *mut c_void,
        flags: u32,
        attribute: usize,
        value: *mut c_void,
        size: usize,
        previous: *mut c_void,
        returned: *mut usize,
    ) -> i32;
    fn DeleteProcThreadAttributeList(list: *mut c_void);
    fn CreateProcessW(
        application: *const u16,
        command_line: *mut u16,
        process_attributes: *mut c_void,
        thread_attributes: *mut c_void,
        inherit_handles: i32,
        creation_flags: u32,
        environment: *mut c_void,
        current_directory: *const u16,
        startup_info: *mut StartupInfoW,
        process_information: *mut ProcessInformation,
    ) -> i32;
    fn TerminateProcess(process: *mut c_void, exit_code: u32) -> i32;
    fn GetExitCodeProcess(process: *mut c_void, exit_code: *mut u32) -> i32;
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtResumeProcess(process: *mut c_void) -> i32;
}

/// The only holder of the unnamed Job handle. It is never passed to the
/// child; dropping it kills any members still running.
struct PrivateJob(OwnedHandle);

impl PrivateJob {
    fn create() -> Result<Self, String> {
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(format!("{LABEL} Job creation failed"));
        }
        let job = Self(unsafe { OwnedHandle::from_raw_handle(raw) });
        if unsafe { SetHandleInformation(raw, HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(format!("{LABEL} Job inheritance control failed"));
        }
        let mut limits = ExtendedLimits::default();
        limits.basic.flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                raw,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                (&mut limits as *mut ExtendedLimits).cast(),
                std::mem::size_of::<ExtendedLimits>() as u32,
            )
        } == 0
        {
            return Err(format!("{LABEL} Job limits unavailable"));
        }
        job.require_limits()?;
        Ok(job)
    }

    fn require_limits(&self) -> Result<(), String> {
        let mut flags = 0;
        if unsafe { GetHandleInformation(self.0.as_raw_handle(), &mut flags) } == 0
            || flags & HANDLE_FLAG_INHERIT != 0
        {
            return Err(format!("{LABEL} Job handle is inheritable"));
        }
        let mut limits = ExtendedLimits::default();
        if unsafe {
            QueryInformationJobObject(
                self.0.as_raw_handle(),
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                (&mut limits as *mut ExtendedLimits).cast(),
                std::mem::size_of::<ExtendedLimits>() as u32,
                std::ptr::null_mut(),
            )
        } == 0
            || limits.basic.flags != JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            || limits.basic.flags
                & (JOB_OBJECT_LIMIT_BREAKAWAY_OK | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK)
                != 0
        {
            return Err(format!("{LABEL} Job limits changed"));
        }
        Ok(())
    }

    fn assign_and_check(&self, child: &OwnedChildProcess) -> Result<(), String> {
        self.require_limits()?;
        if unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), child.as_raw_handle()) } == 0 {
            return Err(format!("{LABEL} Job assignment failed"));
        }
        self.require_exact_member(child)
    }

    fn require_exact_member(&self, child: &OwnedChildProcess) -> Result<(), String> {
        self.require_limits()?;
        let mut exact_member = 0;
        if unsafe {
            IsProcessInJob(
                child.as_raw_handle(),
                self.0.as_raw_handle(),
                &mut exact_member,
            )
        } == 0
            || exact_member != 1
        {
            return Err(format!("{LABEL} exact Job membership unavailable"));
        }
        self.require_limits()
    }

    fn terminate(&self) -> Result<(), String> {
        #[cfg(test)]
        if FORCE_JOB_TERMINATE_FAILURE.with(Cell::get) {
            return Err(format!("{LABEL} injected Job termination failure"));
        }
        self.require_limits()?;
        if unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) } == 0 {
            return Err(format!("{LABEL} Job termination failed"));
        }
        Ok(())
    }

    fn accounting(&self) -> Result<BasicAccounting, String> {
        #[cfg(test)]
        if FORCE_JOB_ACCOUNTING_FAILURE.with(Cell::get) {
            return Err(format!("{LABEL} injected Job accounting failure"));
        }
        let mut accounting = BasicAccounting::default();
        if unsafe {
            QueryInformationJobObject(
                self.0.as_raw_handle(),
                JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION,
                (&mut accounting as *mut BasicAccounting).cast(),
                std::mem::size_of::<BasicAccounting>() as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(format!("{LABEL} Job accounting unavailable"));
        }
        Ok(accounting)
    }

    fn process_list_empty(&self) -> Result<bool, String> {
        let mut list = BasicProcessIdListOne::default();
        if unsafe {
            QueryInformationJobObject(
                self.0.as_raw_handle(),
                JOB_OBJECT_BASIC_PROCESS_ID_LIST,
                (&mut list as *mut BasicProcessIdListOne).cast(),
                std::mem::size_of::<BasicProcessIdListOne>() as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(format!("{LABEL} Job process list unavailable or overfull"));
        }
        Ok(list.number_of_assigned_processes == 0 && list.number_of_process_ids_in_list == 0)
    }
}

/// The native variant is created already inside the Job. `Child` remains only
/// for the older same-user Command diagnostic. Both variants retain the exact
/// process object, including on a failed cleanup transition.
enum OwnedChildProcess {
    Command(Child),
    Native(OwnedHandle),
}

impl AsRawHandle for OwnedChildProcess {
    fn as_raw_handle(&self) -> *mut c_void {
        match self {
            Self::Command(child) => child.as_raw_handle(),
            Self::Native(process) => process.as_raw_handle(),
        }
    }
}

impl OwnedChildProcess {
    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        match self {
            Self::Command(child) => child.try_wait(),
            Self::Native(process) => {
                match unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } {
                    WAIT_TIMEOUT => Ok(None),
                    WAIT_OBJECT_0 => {
                        let mut code = 0;
                        if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
                            Err(std::io::Error::last_os_error())
                        } else {
                            Ok(Some(ExitStatus::from_raw(code)))
                        }
                    }
                    _ => Err(std::io::Error::last_os_error()),
                }
            }
        }
    }

    fn kill(&mut self) -> std::io::Result<()> {
        match self {
            Self::Command(child) => child.kill(),
            Self::Native(process) => {
                if unsafe { TerminateProcess(process.as_raw_handle(), 1) } == 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            }
        }
    }
}

/// Private diagnostic launcher. Its `Command` inherits the current user and
/// cannot by itself create the fixed-account worker or the drained witness.
struct OwnedSuspendedJobChild {
    // Job drops first on every early return and terminates active members.
    job: PrivateJob,
    child: Option<OwnedChildProcess>,
    // Only CreateProcessW's documented failure with no returned process handle
    // permits retrying an empty-Job observation without an original handle.
    creation_failed_without_handle: bool,
}

struct OwnedRunningJobChild {
    // Job drops first on every early return and terminates active members.
    job: PrivateJob,
    child: Option<OwnedChildProcess>,
}

/// A failed transition may be reported as clean only after the exact child
/// and Job have both been observed empty. Otherwise the caller must retain
/// the owner; a String error would release its process and Job handles.
enum ChildTransitionFailure<T> {
    Clean(String),
    Attention { reason: String, owner: T },
}

impl<T> std::fmt::Debug for ChildTransitionFailure<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Clean(reason) => formatter.debug_tuple("Clean").field(reason).finish(),
            Self::Attention { reason, .. } => {
                formatter.debug_tuple("Attention").field(reason).finish()
            }
        }
    }
}

#[derive(Debug)]
struct NormalDrainedJobV1;

enum AttentionJobChild {
    Suspended(OwnedSuspendedJobChild),
    Running(OwnedRunningJobChild),
}

/// A private, non-cloneable owner for a Sol turn with uncertain termination.
/// It carries no output, nonce-burn, staging, or repository-use capability.
/// A future service must retain it in its attention registry until a retry
/// proves the exact child terminal and the Job empty. The create-once nonce
/// remains spent even if the host process itself exits.
struct SolJobAttentionOwner {
    reason: String,
    child: AttentionJobChild,
    prepared_run: ReservedPreparedSolRunV1,
    executable: PinnedSolWorkerExecutableV1,
    bound_worker: Option<BoundFixedLocalSolWorkerProcessV1>,
}

impl std::fmt::Debug for SolJobAttentionOwner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("SolJobAttentionOwner")
            .field(&self.reason)
            .finish()
    }
}

enum PreparedSolLifecycleFailure {
    Clean(String),
    Attention(Box<SolJobAttentionOwner>),
}

impl std::fmt::Debug for PreparedSolLifecycleFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Clean(reason) => formatter.debug_tuple("Clean").field(reason).finish(),
            Self::Attention(owner) => formatter
                .debug_tuple("Attention")
                .field(&owner.reason)
                .finish(),
        }
    }
}

/// Necessary host-held context for one future fixed-account run. This type
/// selects the nonce only after checking the pinned output root is empty. It
/// does not prove the ledger ACL excludes the worker or that no other process
/// can write the output root. No live launcher or downstream handoff accepts
/// it yet.
struct PreparedSolRunV1 {
    authority: SolWorkerNonceAuthorityV1,
    nonce: String,
    ledger: ProtectedDirectoryGuard,
    output_root: ProtectedDirectoryGuard,
}

/// Only this spent, non-cloneable state can enter the prepared native spawn.
/// Losing it never permits re-reservation because the create-once ledger name
/// remains occupied, including after process restart.
struct ReservedPreparedSolRunV1 {
    prepared: PreparedSolRunV1,
    reservation: SolLaunchReservationV1,
}

impl std::ops::Deref for ReservedPreparedSolRunV1 {
    type Target = PreparedSolRunV1;

    fn deref(&self) -> &Self::Target {
        &self.prepared
    }
}

impl ReservedPreparedSolRunV1 {
    fn require_still_reserved(&self) -> Result<(), String> {
        self.prepared.require_ready()?;
        self.reservation.require_unchanged(&self.prepared.ledger)
    }

    fn require_exact(
        &self,
        executable: &PinnedSolWorkerExecutableV1,
        inputs: &PreparedSolLaunchInputsV1,
    ) -> Result<(), String> {
        self.prepared.require_ready()?;
        let binding = self.prepared.launch_binding(executable, inputs)?;
        self.reservation
            .require_exact(&self.prepared.ledger, &binding)
    }

    fn diagnose_retained_post_run_context(
        &self,
        executable: &PinnedSolWorkerExecutableV1,
    ) -> Result<(), String> {
        executable.require_original_native_path()?;
        self.reservation.diagnose_retained_objects(
            &self.prepared.ledger,
            &self.prepared.output_root,
            &executable.parent,
            &executable.component,
            &executable.reviewed.identity.object_identity,
            &executable.reviewed.identity.sha256,
        )?;
        executable.diagnose_retained_artifact()?;
        Ok(())
    }
}

/// Explicit inputs for the private same-user creation-time Job probe. A
/// future fixed-account launch must use reviewed, policy-pinned inputs and a
/// fixed-account token; constructing this value confers no such authority.
struct PreparedSolLaunchInputsV1 {
    arguments: Vec<OsString>,
    environment: BTreeMap<String, String>,
}

struct JobAttributeList {
    buffer: Vec<usize>,
    job_handle: Box<*mut c_void>,
}

impl JobAttributeList {
    fn for_job(job: &PrivateJob) -> Result<Self, String> {
        let mut size = 0;
        unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size) };
        if size == 0 || size > MAX_ATTRIBUTE_LIST_BYTES {
            return Err(format!("{LABEL} Job launch attribute size unavailable"));
        }
        let mut buffer = vec![0_usize; size.div_ceil(std::mem::size_of::<usize>())];
        let list = buffer.as_mut_ptr().cast();
        if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut size) } == 0 {
            return Err(format!(
                "{LABEL} Job launch attribute initialization failed"
            ));
        }
        let mut attributes = Self {
            buffer,
            job_handle: Box::new(job.0.as_raw_handle()),
        };
        if unsafe {
            UpdateProcThreadAttribute(
                attributes.as_mut_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST,
                attributes.job_handle.as_mut() as *mut *mut c_void as *mut c_void,
                std::mem::size_of::<*mut c_void>(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(format!("{LABEL} creation-time Job assignment unavailable"));
        }
        Ok(attributes)
    }

    fn as_mut_ptr(&mut self) -> *mut c_void {
        self.buffer.as_mut_ptr().cast()
    }
}

impl Drop for JobAttributeList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.as_mut_ptr()) };
    }
}

fn nul_terminated_wide(value: &OsStr, label: &str) -> Result<Vec<u16>, String> {
    let mut wide: Vec<u16> = value.encode_wide().collect();
    if wide.is_empty() || wide.len() >= MAX_LAUNCH_WCHARS || wide.contains(&0) {
        return Err(format!("{LABEL} {label} is invalid"));
    }
    wide.push(0);
    Ok(wide)
}

/// Windows command-line quoting for an explicit application and arguments.
/// The application is separately passed as lpApplicationName, so parsing the
/// first argument cannot select a different executable.
fn quote_windows_argument(value: &OsStr) -> Result<Vec<u16>, String> {
    let wide: Vec<u16> = value.encode_wide().collect();
    if wide.contains(&0) {
        return Err(format!("{LABEL} launch argument contains NUL"));
    }
    let quoted = wide.is_empty() || wide.iter().any(|unit| matches!(*unit, 9 | 32 | 34));
    if !quoted {
        return Ok(wide);
    }
    let mut result = vec![34];
    let mut backslashes = 0;
    for unit in wide {
        if unit == 92 {
            backslashes += 1;
            continue;
        }
        if unit == 34 {
            result.extend(std::iter::repeat_n(92, backslashes * 2 + 1));
            result.push(34);
        } else {
            result.extend(std::iter::repeat_n(92, backslashes));
            result.push(unit);
        }
        backslashes = 0;
    }
    result.extend(std::iter::repeat_n(92, backslashes * 2));
    result.push(34);
    Ok(result)
}

impl PreparedSolLaunchInputsV1 {
    fn command_line(&self, application: &OsStr) -> Result<Vec<u16>, String> {
        let mut command_line = quote_windows_argument(application)?;
        for argument in &self.arguments {
            command_line.push(32);
            command_line.extend(quote_windows_argument(argument)?);
            if command_line.len() >= MAX_LAUNCH_WCHARS {
                return Err(format!("{LABEL} command line is too long"));
            }
        }
        if command_line.len() >= MAX_LAUNCH_WCHARS {
            return Err(format!("{LABEL} command line is too long"));
        }
        command_line.push(0);
        Ok(command_line)
    }

    fn environment_block(&self) -> Result<Vec<u16>, String> {
        let mut block = Vec::new();
        let mut names = std::collections::BTreeSet::new();
        for (name, value) in &self.environment {
            let normalized = name.to_ascii_uppercase();
            if name.is_empty()
                || name.len() > 128
                || *name != normalized
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || !names.insert(normalized)
                || value.contains('\0')
            {
                return Err(format!("{LABEL} explicit environment is invalid"));
            }
            block.extend(name.encode_utf16());
            block.push(61);
            block.extend(value.encode_utf16());
            block.push(0);
            if block.len() >= MAX_LAUNCH_WCHARS {
                return Err(format!("{LABEL} explicit environment is too long"));
            }
        }
        // An empty environment still requires two terminating WCHARs.
        block.push(0);
        if self.environment.is_empty() {
            block.push(0);
        }
        if block.len() > MAX_LAUNCH_WCHARS {
            return Err(format!("{LABEL} explicit environment is too long"));
        }
        Ok(block)
    }
}

/// Launch-scoped input only. A future trusted host must supply the approved
/// digest and a pinned directory chain rooted in its immutable policy. A
/// caller-provided digest by itself is not an approval or an image-section
/// attestation. The retained file handle denies new write/delete opens while
/// this value remains alive.
struct PinnedSolWorkerExecutableV1 {
    parent: ProtectedDirectoryGuard,
    component: String,
    reviewed: RetainedReviewedArtifact,
    native_final_path: Vec<u16>,
}

impl PinnedSolWorkerExecutableV1 {
    fn pin(
        parent: ProtectedDirectoryGuard,
        component: &str,
        approved_sha256: &str,
    ) -> Result<Self, String> {
        if !parent.path().is_absolute() || !valid_executable_component(component) {
            return Err(format!("{LABEL} approved executable path is invalid"));
        }
        let reviewed = retain_reviewed_regular_artifact(
            &parent,
            component,
            approved_sha256,
            MAX_APPROVED_EXECUTABLE_BYTES,
            LABEL,
        )?;
        let native_final_path = reviewed.native_final_path(LABEL)?;
        parent.assert_stable(LABEL)?;
        Ok(Self {
            parent,
            component: component.into(),
            reviewed,
            native_final_path,
        })
    }

    fn require_original_native_path(&self) -> Result<(), String> {
        self.parent.assert_stable(LABEL)?;
        if self.reviewed.native_final_path(LABEL)? != self.native_final_path {
            return Err(format!("{LABEL} retained executable path changed"));
        }
        Ok(())
    }

    /// Diagnostic readback through the pinned parent. This rehashes the exact
    /// named artifact and compares it with the original retained file; it
    /// does not attest the process's mapped image section.
    fn diagnose_retained_artifact(&self) -> Result<(), String> {
        self.require_original_native_path()?;
        let reopened = retain_reviewed_regular_artifact(
            &self.parent,
            &self.component,
            &self.reviewed.identity.sha256,
            MAX_APPROVED_EXECUTABLE_BYTES,
            LABEL,
        )?;
        if reopened.identity != self.reviewed.identity
            || reopened.native_final_path(LABEL)? != self.native_final_path
        {
            return Err(format!("{LABEL} retained executable changed"));
        }
        self.require_original_native_path()
    }

    fn launch_path(&self) -> PathBuf {
        self.parent.path().join(&self.component)
    }

    /// Compare two bounded NT path observations only as a fail-closed image
    /// mismatch diagnostic. The process handle must be the Job owner's exact
    /// suspended child; the query cannot prove the mapped image file object.
    fn require_same_image_path(&self, process: BorrowedHandle<'_>) -> Result<(), String> {
        self.parent.assert_stable(LABEL)?;
        let expected = self.reviewed.native_final_path(LABEL)?;
        let mut observed = vec![0_u16; MAX_IMAGE_PATH_WCHARS];
        let mut length = observed.len() as u32;
        if unsafe {
            QueryFullProcessImageNameW(
                process.as_raw_handle(),
                PROCESS_NAME_NATIVE,
                observed.as_mut_ptr(),
                &mut length,
            )
        } == 0
            || length == 0
            || length as usize >= observed.len()
            || observed[..length as usize].contains(&0)
        {
            return Err(format!("{LABEL} suspended child image path unavailable"));
        }
        observed.truncate(length as usize);
        if !same_native_image_path(&expected, &observed) {
            return Err(format!("{LABEL} suspended child image path differs"));
        }
        self.parent.assert_stable(LABEL)
    }
}

fn same_native_image_path(expected: &[u16], observed: &[u16]) -> bool {
    // Windows directories can be case-sensitive. A path-only diagnostic must
    // reject casing drift rather than accepting a potentially distinct file.
    (unsafe {
        CompareStringOrdinal(
            expected.as_ptr(),
            expected.len() as i32,
            observed.as_ptr(),
            observed.len() as i32,
            0,
        )
    }) == CSTR_EQUAL
}

fn valid_executable_component(component: &str) -> bool {
    if component.is_empty()
        || component.len() > 255
        || !component.to_ascii_lowercase().ends_with(".exe")
        || component.starts_with('.')
        || !component
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return false;
    }
    let stem = component
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

/// Consumes the run and image lease before a suspended creation-time Job
/// launch. This still inherits the host user; it cannot create a fixed-account
/// witness on this host. Uncertain cleanup returns a retained attention owner;
/// a future service must hold that owner durably before a positive launcher.
struct PreparedSuspendedSolJobChild {
    suspended: OwnedSuspendedJobChild,
    prepared_run: ReservedPreparedSolRunV1,
    executable: PinnedSolWorkerExecutableV1,
}

impl SolJobAttentionOwner {
    /// Retry only a bounded abort. This never turns an uncertain run into a
    /// deliverable or permits a second nonce. A failed retry keeps ownership.
    fn retry_abort(self: Box<Self>) -> Result<(), Box<Self>> {
        let Self {
            reason,
            child,
            prepared_run,
            executable,
            bound_worker,
        } = *self;
        let result = match child {
            AttentionJobChild::Suspended(owner) => owner
                .abort(reason)
                .map_attention_child(AttentionJobChild::Suspended),
            AttentionJobChild::Running(owner) => {
                match owner.abort_and_drain(FAILED_LAUNCH_CLEANUP) {
                    Ok(()) => return Ok(()),
                    Err(ChildTransitionFailure::Clean(cleanup)) => {
                        ChildTransitionFailure::Clean(format!("{reason}; {cleanup}"))
                    }
                    Err(ChildTransitionFailure::Attention {
                        reason: cleanup,
                        owner,
                    }) => ChildTransitionFailure::Attention {
                        reason: format!("{reason}; {cleanup}"),
                        owner: AttentionJobChild::Running(owner),
                    },
                }
            }
        };
        match result {
            ChildTransitionFailure::Clean(_) => Ok(()),
            ChildTransitionFailure::Attention { reason, owner } => Err(Box::new(Self {
                reason,
                child: owner,
                prepared_run,
                executable,
                bound_worker,
            })),
        }
    }
}

impl<T> ChildTransitionFailure<T> {
    fn map_attention_child<U>(self, into: impl FnOnce(T) -> U) -> ChildTransitionFailure<U> {
        match self {
            Self::Clean(reason) => ChildTransitionFailure::Clean(reason),
            Self::Attention { reason, owner } => ChildTransitionFailure::Attention {
                reason,
                owner: into(owner),
            },
        }
    }
}

fn prepared_failure(
    failure: ChildTransitionFailure<AttentionJobChild>,
    prepared_run: ReservedPreparedSolRunV1,
    executable: PinnedSolWorkerExecutableV1,
    bound_worker: Option<BoundFixedLocalSolWorkerProcessV1>,
) -> PreparedSolLifecycleFailure {
    match failure {
        ChildTransitionFailure::Clean(reason) => PreparedSolLifecycleFailure::Clean(reason),
        ChildTransitionFailure::Attention { reason, owner } => {
            PreparedSolLifecycleFailure::Attention(Box::new(SolJobAttentionOwner {
                reason,
                child: owner,
                prepared_run,
                executable,
                bound_worker,
            }))
        }
    }
}

impl PreparedSuspendedSolJobChild {
    fn launch(
        prepared_run: ReservedPreparedSolRunV1,
        executable: PinnedSolWorkerExecutableV1,
        inputs: &PreparedSolLaunchInputsV1,
    ) -> Result<Self, PreparedSolLifecycleFailure> {
        prepared_run
            .require_exact(&executable, inputs)
            .map_err(PreparedSolLifecycleFailure::Clean)?;
        executable
            .parent
            .assert_stable(LABEL)
            .map_err(PreparedSolLifecycleFailure::Clean)?;
        let suspended = match OwnedSuspendedJobChild::launch_suspended_at_creation(
            &prepared_run,
            &executable,
            inputs,
        ) {
            Ok(owner) => owner,
            Err(failure) => {
                return Err(prepared_failure(
                    failure.map_attention_child(AttentionJobChild::Suspended),
                    prepared_run,
                    executable,
                    None,
                ));
            }
        };
        #[cfg(test)]
        if FORCE_PREPARED_OUTPUT_TAMPER_AFTER_SPAWN.with(Cell::get) {
            std::fs::write(prepared_run.output_root.path().join("late.txt"), b"late")
                .expect("inject post-spawn output drift");
        }
        let Some(child) = suspended.child.as_ref() else {
            return Err(prepared_failure(
                suspended
                    .abort(format!("{LABEL} original child missing"))
                    .map_attention_child(AttentionJobChild::Suspended),
                prepared_run,
                executable,
                None,
            ));
        };
        let opened = unsafe { BorrowedHandle::borrow_raw(child.as_raw_handle()) };
        if let Err(error) = executable.require_same_image_path(opened) {
            return Err(prepared_failure(
                suspended
                    .abort(error)
                    .map_attention_child(AttentionJobChild::Suspended),
                prepared_run,
                executable,
                None,
            ));
        }
        if let Err(error) = prepared_run.require_exact(&executable, inputs) {
            return Err(prepared_failure(
                suspended
                    .abort(error)
                    .map_attention_child(AttentionJobChild::Suspended),
                prepared_run,
                executable,
                None,
            ));
        }
        Ok(Self {
            suspended,
            prepared_run,
            executable,
        })
    }

    /// This has no production caller. The exact child object, image path,
    /// fixed-account token and prepared run are all checked before resume.
    fn bind_and_resume_fixed(
        self,
        bound_worker: BoundFixedLocalSolWorkerProcessV1,
    ) -> Result<BoundFixedWorkerJobChild, PreparedSolLifecycleFailure> {
        let Self {
            suspended,
            prepared_run,
            executable,
        } = self;
        let Some(child) = suspended.child.as_ref() else {
            return Err(prepared_failure(
                suspended
                    .abort(format!("{LABEL} original child missing"))
                    .map_attention_child(AttentionJobChild::Suspended),
                prepared_run,
                executable,
                Some(bound_worker),
            ));
        };
        let opened = unsafe { BorrowedHandle::borrow_raw(child.as_raw_handle()) };
        if let Err(error) = bound_worker.require_same_opened_process(opened) {
            return Err(prepared_failure(
                suspended
                    .abort(error)
                    .map_attention_child(AttentionJobChild::Suspended),
                prepared_run,
                executable,
                Some(bound_worker),
            ));
        }
        if let Err(error) = executable.require_same_image_path(opened) {
            return Err(prepared_failure(
                suspended
                    .abort(error)
                    .map_attention_child(AttentionJobChild::Suspended),
                prepared_run,
                executable,
                Some(bound_worker),
            ));
        }
        if let Err(error) = prepared_run.require_still_reserved() {
            return Err(prepared_failure(
                suspended
                    .abort(error)
                    .map_attention_child(AttentionJobChild::Suspended),
                prepared_run,
                executable,
                Some(bound_worker),
            ));
        }
        let running = match suspended.resume() {
            Ok(owner) => owner,
            Err(failure) => {
                return Err(prepared_failure(
                    failure.map_attention_child(AttentionJobChild::Suspended),
                    prepared_run,
                    executable,
                    Some(bound_worker),
                ));
            }
        };
        Ok(BoundFixedWorkerJobChild {
            running,
            bound_worker,
            prepared_run,
            executable,
        })
    }
}

impl PreparedSolRunV1 {
    fn launch_binding(
        &self,
        executable: &PinnedSolWorkerExecutableV1,
        inputs: &PreparedSolLaunchInputsV1,
    ) -> Result<SolLaunchBindingV1, String> {
        self.require_ready()?;
        executable.parent.assert_stable(LABEL)?;
        executable.reviewed.native_final_path(LABEL)?;
        let application = executable.launch_path();
        let application_wide = nul_terminated_wide(application.as_os_str(), "application path")?;
        let command_line = inputs.command_line(application.as_os_str())?;
        let environment = inputs.environment_block()?;
        let directory =
            nul_terminated_wide(self.output_root.path().as_os_str(), "working directory")?;
        let mut digest = Sha256::new();
        digest.update(b"catdesk.sol.launch-inputs.v1");
        for wide in [&application_wide, &command_line, &environment, &directory] {
            digest.update((wide.len() as u64).to_le_bytes());
            for unit in wide {
                digest.update(unit.to_le_bytes());
            }
        }
        Ok(SolLaunchBindingV1 {
            authority: self.authority.clone(),
            nonce: self.nonce.clone(),
            ledger_identity: self.ledger.physical_identity(LABEL)?,
            output_identity: self.output_root.physical_identity(LABEL)?,
            executable_parent_identity: executable.parent.physical_identity(LABEL)?,
            executable_component: executable.component.clone(),
            executable_object_identity: executable.reviewed.identity.object_identity.clone(),
            executable_sha256: executable.reviewed.identity.sha256.clone(),
            launch_input_sha256: format!("{:x}", digest.finalize()),
        })
    }

    fn reserve_launch(
        self,
        executable: &PinnedSolWorkerExecutableV1,
        inputs: &PreparedSolLaunchInputsV1,
    ) -> Result<ReservedPreparedSolRunV1, String> {
        let binding = self.launch_binding(executable, inputs)?;
        let reservation = SolLaunchReservationV1::reserve(&self.ledger, binding)?;
        Ok(ReservedPreparedSolRunV1 {
            prepared: self,
            reservation,
        })
    }

    fn prepare(
        ledger: ProtectedDirectoryGuard,
        output_root: ProtectedDirectoryGuard,
        authority: SolWorkerNonceAuthorityV1,
    ) -> Result<Self, String> {
        validate_sol_worker_allowed_paths(&authority.allowed_paths)?;
        ledger.assert_stable(LABEL)?;
        Self::require_empty_output_root(&output_root)?;
        let nonce = issue_sol_worker_nonce(&ledger, &authority)?;
        let prepared = Self {
            authority,
            nonce,
            ledger,
            output_root,
        };
        // A failed check after selection leaves this turn in attention. The
        // create-once active record prevents a replacement nonce on retry.
        prepared.require_ready()?;
        Ok(prepared)
    }

    fn require_empty_output_root(root: &ProtectedDirectoryGuard) -> Result<(), String> {
        root.assert_stable(LABEL)?;
        let entries = query_relative_directory_names_bounded(root, 1)?;
        root.assert_stable(LABEL)?;
        if !entries.is_empty() {
            return Err(format!("{LABEL} output root is not empty"));
        }
        Ok(())
    }

    fn require_ready(&self) -> Result<(), String> {
        self.ledger.assert_stable(LABEL)?;
        Self::require_empty_output_root(&self.output_root)?;
        if read_active_sol_worker_nonce(&self.ledger, &self.authority)? != self.nonce {
            return Err(format!("{LABEL} selected nonce changed"));
        }
        self.ledger.assert_stable(LABEL)?;
        Self::require_empty_output_root(&self.output_root)
    }
}

/// Only a still-suspended, exact Job member whose opened process was bound to
/// the fixed local account can reach this state. This is private while a
/// trusted fixed-account launcher and complete write-authority proof are
/// absent.
struct BoundFixedWorkerJobChild {
    running: OwnedRunningJobChild,
    bound_worker: BoundFixedLocalSolWorkerProcessV1,
    prepared_run: ReservedPreparedSolRunV1,
    executable: PinnedSolWorkerExecutableV1,
}

/// A necessary-condition observation: the bound original process and all
/// members observed in its retained Job have drained. It says nothing about
/// processes launched outside that Job or handles held by other processes.
/// Private fields and the lack of `Clone` prevent a caller-created claim.
pub(crate) struct DrainedBoundFixedWorkerJobV1 {
    _bound_worker: BoundFixedLocalSolWorkerProcessV1,
    _prepared_run: ReservedPreparedSolRunV1,
    _executable: PinnedSolWorkerExecutableV1,
}

/// An inert post-drain handoff. This is constructed only by consuming the
/// normal, bound Job-drain witness. The retained fixed-worker binding includes
/// the exact opened process and its pre-resume restricted-token observation;
/// the run and executable leases keep the selected task, nonce, pinned roots,
/// launch reservation and reviewed image together. The live-process
/// `revalidate` method cannot be used after normal exit. A future boundary
/// must prove post-exit identity and loss of all write authority before it may
/// consume output. This type exposes no burn, stage or repository-use method.
pub(crate) struct StoppedBoundFixedWorkerCandidateV1 {
    _bound_worker: BoundFixedLocalSolWorkerProcessV1,
    _prepared_run: ReservedPreparedSolRunV1,
    _executable: PinnedSolWorkerExecutableV1,
}

impl StoppedBoundFixedWorkerCandidateV1 {
    /// Diagnostic only. This checks the original exited process and retained
    /// restricted token against the fixed local account, but cannot prove no
    /// outside-Job process or writable handle survived the normal drain.
    pub(crate) fn diagnose_retained_exited_identity(&self) -> Result<(), String> {
        self._bound_worker.require_retained_exited_identity()
    }

    /// Diagnostic only. This checks retained host launch objects and the
    /// selected reservation after output may have been written. It does not
    /// attest output contents or grant staging or repository authority.
    pub(crate) fn diagnose_retained_host_context(&self) -> Result<(), String> {
        self._prepared_run
            .diagnose_retained_post_run_context(&self._executable)
    }
}

impl DrainedBoundFixedWorkerJobV1 {
    pub(crate) fn into_stopped_candidate(self) -> StoppedBoundFixedWorkerCandidateV1 {
        StoppedBoundFixedWorkerCandidateV1 {
            _bound_worker: self._bound_worker,
            _prepared_run: self._prepared_run,
            _executable: self._executable,
        }
    }
}

/// The original process handle is the cleanup target. The diagnostic Command
/// path can fail before Job assignment; the prepared native path starts in
/// the Job. Neither may infer terminal state from Job close alone.
fn terminate_original_child_bounded(child: &mut OwnedChildProcess) -> Result<(), String> {
    match unsafe { WaitForSingleObject(child.as_raw_handle(), 0) } {
        WAIT_OBJECT_0 => {
            return if child
                .try_wait()
                .map_err(|_| format!("{LABEL} original child status unavailable"))?
                .is_some()
            {
                Ok(())
            } else {
                Err(format!("{LABEL} original child status is not terminal"))
            };
        }
        WAIT_TIMEOUT => {}
        _ => return Err(format!("{LABEL} original child state unavailable")),
    }
    // TerminateJobObject may have initiated asynchronous teardown already.
    // TerminateProcess can then fail while the exact child is still becoming
    // signaled; only the bounded wait below decides whether cleanup proved it.
    let kill_failed = child.kill().is_err();
    let deadline = Instant::now() + FAILED_LAUNCH_CLEANUP;
    loop {
        match unsafe { WaitForSingleObject(child.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => {
                return if child
                    .try_wait()
                    .map_err(|_| format!("{LABEL} original child status unavailable"))?
                    .is_some()
                {
                    Ok(())
                } else {
                    Err(format!("{LABEL} original child status is not terminal"))
                };
            }
            WAIT_TIMEOUT if Instant::now() < deadline => std::thread::sleep(POLL),
            WAIT_TIMEOUT => {
                return Err(format!(
                    "{LABEL} original child termination {}",
                    if kill_failed { "failed" } else { "timed out" }
                ));
            }
            _ => return Err(format!("{LABEL} original child wait unavailable")),
        }
    }
}

fn wait_original_child_until(
    child: &mut OwnedChildProcess,
    deadline: Instant,
) -> Result<Option<ExitStatus>, String> {
    loop {
        match unsafe { WaitForSingleObject(child.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => {
                return child
                    .try_wait()
                    .map_err(|_| format!("{LABEL} original child status unavailable"))?
                    .map(Some)
                    .ok_or_else(|| format!("{LABEL} original child status is not terminal"));
            }
            WAIT_TIMEOUT if Instant::now() < deadline => std::thread::sleep(POLL),
            WAIT_TIMEOUT => return Ok(None),
            _ => return Err(format!("{LABEL} original child wait unavailable")),
        }
    }
}

fn job_empty_until(
    job: &PrivateJob,
    deadline: Instant,
    require_member: bool,
) -> Result<bool, String> {
    loop {
        job.require_limits()?;
        let state = job.accounting()?;
        if (!require_member || state.total_processes >= 1)
            && state.active_processes == 0
            && job.process_list_empty()?
        {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(POLL);
    }
}

impl OwnedSuspendedJobChild {
    fn launch_suspended_at_creation(
        prepared_run: &ReservedPreparedSolRunV1,
        executable: &PinnedSolWorkerExecutableV1,
        inputs: &PreparedSolLaunchInputsV1,
    ) -> Result<Self, ChildTransitionFailure<Self>> {
        let application = executable.launch_path();
        let application_wide = nul_terminated_wide(application.as_os_str(), "application path")
            .map_err(ChildTransitionFailure::Clean)?;
        let mut command_line = inputs
            .command_line(application.as_os_str())
            .map_err(ChildTransitionFailure::Clean)?;
        let mut environment = inputs
            .environment_block()
            .map_err(ChildTransitionFailure::Clean)?;
        let current_directory = nul_terminated_wide(
            prepared_run.output_root.path().as_os_str(),
            "working directory",
        )
        .map_err(ChildTransitionFailure::Clean)?;
        let job = PrivateJob::create().map_err(ChildTransitionFailure::Clean)?;
        let mut attributes =
            JobAttributeList::for_job(&job).map_err(ChildTransitionFailure::Clean)?;
        prepared_run
            .require_exact(executable, inputs)
            .map_err(ChildTransitionFailure::Clean)?;
        executable
            .parent
            .assert_stable(LABEL)
            .map_err(ChildTransitionFailure::Clean)?;
        job.require_limits()
            .map_err(ChildTransitionFailure::Clean)?;
        let mut startup: StartupInfoExW = unsafe { std::mem::zeroed() };
        startup.startup.cb = std::mem::size_of::<StartupInfoExW>() as u32;
        startup.attributes = attributes.as_mut_ptr();
        let mut info: ProcessInformation = unsafe { std::mem::zeroed() };
        // lpApplicationName is the pinned executable's exact path. No handles
        // are inherited, even if another host handle is marked inheritable.
        let created = unsafe {
            CreateProcessW(
                application_wide.as_ptr(),
                command_line.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                CREATE_SUSPENDED
                    | CREATE_NO_WINDOW
                    | CREATE_UNICODE_ENVIRONMENT
                    | EXTENDED_STARTUPINFO_PRESENT,
                environment.as_mut_ptr().cast(),
                current_directory.as_ptr(),
                &mut startup.startup,
                &mut info,
            )
        };
        drop(attributes);
        let thread =
            (!info.thread.is_null()).then(|| unsafe { OwnedHandle::from_raw_handle(info.thread) });
        let child = (!info.process.is_null()).then(|| {
            OwnedChildProcess::Native(unsafe { OwnedHandle::from_raw_handle(info.process) })
        });
        drop(thread);
        let creation_failed_without_handle = created == 0 && child.is_none();
        let owner = Self {
            job,
            child,
            creation_failed_without_handle,
        };
        if created == 0 {
            if owner.child.is_some() {
                return Err(owner.abort(format!("{LABEL} creation-time Job launch failed")));
            }
            return match job_empty_until(&owner.job, Instant::now() + FAILED_LAUNCH_CLEANUP, false)
            {
                Ok(true) => Err(ChildTransitionFailure::Clean(format!(
                    "{LABEL} creation-time Job launch failed"
                ))),
                _ => Err(ChildTransitionFailure::Attention {
                    reason: format!("{LABEL} failed launch Job state unavailable"),
                    owner,
                }),
            };
        }
        if owner.child.is_none() || info.thread.is_null() {
            // Win32 promises both handles on success. If that contract is
            // violated, an absent process handle cannot prove exact-child
            // termination: abort retains the Job in Attention even when its
            // termination request succeeds. A missing thread handle permits
            // clean failure only after exact process and Job drain.
            return Err(owner.abort(format!("{LABEL} created process handles unavailable")));
        }
        if let Err(error) = owner
            .job
            .require_exact_member(owner.child.as_ref().expect("created process handle"))
        {
            return Err(owner.abort(error));
        }
        Ok(owner)
    }

    fn launch_suspended(command: &mut Command) -> Result<Self, ChildTransitionFailure<Self>> {
        let job = PrivateJob::create().map_err(ChildTransitionFailure::Clean)?;
        command.creation_flags(CREATE_SUSPENDED);
        let child = command.spawn().map_err(|_| {
            ChildTransitionFailure::Clean(format!("{LABEL} suspended launch failed"))
        })?;
        let owner = Self {
            job,
            child: Some(OwnedChildProcess::Command(child)),
            creation_failed_without_handle: false,
        };
        if let Err(error) = owner
            .job
            .assign_and_check(owner.child.as_ref().expect("just assigned child"))
        {
            return Err(owner.abort(error));
        }
        Ok(owner)
    }

    fn launch(command: &mut Command) -> Result<OwnedRunningJobChild, ChildTransitionFailure<Self>> {
        Self::launch_suspended(command)?.resume()
    }

    fn abort(mut self, error: String) -> ChildTransitionFailure<Self> {
        let mut reasons = vec![error];
        if let Err(job_error) = self.job.terminate() {
            reasons.push(job_error);
        }
        match self.child.as_mut() {
            Some(child) => {
                if let Err(child_error) = terminate_original_child_bounded(child) {
                    reasons.push(child_error);
                }
            }
            None if self.creation_failed_without_handle => {}
            None => reasons.push(format!("{LABEL} original child missing")),
        }
        if reasons.len() > 1 {
            return ChildTransitionFailure::Attention {
                reason: reasons.join("; "),
                owner: self,
            };
        }
        match job_empty_until(&self.job, Instant::now() + FAILED_LAUNCH_CLEANUP, false) {
            Ok(true) => {}
            Ok(false) => reasons.push(format!("{LABEL} Job still has active processes")),
            Err(job_error) => reasons.push(job_error),
        }
        if reasons.len() == 1 {
            ChildTransitionFailure::Clean(reasons.remove(0))
        } else {
            ChildTransitionFailure::Attention {
                reason: reasons.join("; "),
                owner: self,
            }
        }
    }

    fn resume(self) -> Result<OwnedRunningJobChild, ChildTransitionFailure<Self>> {
        let Some(child) = self.child.as_ref() else {
            return Err(self.abort(format!("{LABEL} original child missing")));
        };
        if let Err(error) = self.job.require_exact_member(child) {
            return Err(self.abort(error));
        }
        if unsafe { NtResumeProcess(child.as_raw_handle()) } < 0 {
            return Err(self.abort(format!("{LABEL} child resume failed")));
        }
        let Self { job, child, .. } = self;
        Ok(OwnedRunningJobChild { job, child })
    }
}

impl OwnedRunningJobChild {
    /// Abort the exact child and all Job members, retaining every handle if
    /// termination or drain cannot be proved. This path grants no witness.
    fn abort_and_drain(mut self, timeout: Duration) -> Result<(), ChildTransitionFailure<Self>> {
        if timeout.is_zero() || timeout > MAX_DRAIN {
            return Err(ChildTransitionFailure::Attention {
                reason: format!("{LABEL} drain deadline invalid"),
                owner: self,
            });
        }
        let deadline = Instant::now() + timeout;
        let mut reasons = Vec::new();
        if let Err(error) = self.job.terminate() {
            reasons.push(error);
            if let Some(child) = self.child.as_mut() {
                if let Err(child_error) = terminate_original_child_bounded(child) {
                    reasons.push(child_error);
                }
            }
            return Err(ChildTransitionFailure::Attention {
                reason: reasons.join("; "),
                owner: self,
            });
        }
        match self.child.as_mut() {
            Some(child) => match wait_original_child_until(child, deadline) {
                Ok(Some(_)) => {}
                Ok(None) => reasons.push(format!("{LABEL} original child still running")),
                Err(error) => reasons.push(error),
            },
            None => reasons.push(format!("{LABEL} original child missing")),
        }
        match job_empty_until(&self.job, deadline, true) {
            Ok(true) => {}
            Ok(false) => reasons.push(format!("{LABEL} Job still has active processes")),
            Err(error) => reasons.push(error),
        }
        if reasons.is_empty() {
            Ok(())
        } else {
            Err(ChildTransitionFailure::Attention {
                reason: reasons.join("; "),
                owner: self,
            })
        }
    }

    fn failed_normal_completion(self, reason: String) -> ChildTransitionFailure<Self> {
        match self.abort_and_drain(FAILED_LAUNCH_CLEANUP) {
            Ok(()) => ChildTransitionFailure::Clean(reason),
            Err(ChildTransitionFailure::Attention {
                reason: cleanup,
                owner,
            }) => ChildTransitionFailure::Attention {
                reason: format!("{reason}; {cleanup}"),
                owner,
            },
            Err(ChildTransitionFailure::Clean(cleanup)) => {
                ChildTransitionFailure::Clean(format!("{reason}; {cleanup}"))
            }
        }
    }

    /// A normal witness requires the exact original process to exit zero and
    /// the complete retained Job to empty without first killing its members.
    /// Failure triggers a bounded abort; uncertain cleanup retains ownership.
    fn finish_normal(
        mut self,
        timeout: Duration,
    ) -> Result<NormalDrainedJobV1, ChildTransitionFailure<Self>> {
        if timeout.is_zero() || timeout > MAX_DRAIN {
            return Err(ChildTransitionFailure::Attention {
                reason: format!("{LABEL} normal completion deadline invalid"),
                owner: self,
            });
        }
        let deadline = Instant::now() + timeout;
        let status = match self.child.as_mut() {
            Some(child) => match wait_original_child_until(child, deadline) {
                Ok(Some(status)) => status,
                Ok(None) => {
                    return Err(self.failed_normal_completion(format!(
                        "{LABEL} normal original child timed out"
                    )));
                }
                Err(error) => return Err(self.failed_normal_completion(error)),
            },
            None => {
                return Err(
                    self.failed_normal_completion(format!("{LABEL} original child missing"))
                );
            }
        };
        if !status.success() {
            return Err(self.failed_normal_completion(format!(
                "{LABEL} original child exited unsuccessfully"
            )));
        }
        match job_empty_until(&self.job, deadline, true) {
            Ok(true) => Ok(NormalDrainedJobV1),
            Ok(false) => Err(self.failed_normal_completion(format!(
                "{LABEL} Job did not drain after normal child exit"
            ))),
            Err(error) => Err(self.failed_normal_completion(error)),
        }
    }
}

impl BoundFixedWorkerJobChild {
    /// The process-token observation is checked before resume because a
    /// normally completed worker has exited here. This accepts only a zero
    /// exit and empty Job; failure or forced abort mints no drained witness.
    fn finish(
        self,
        timeout: Duration,
    ) -> Result<DrainedBoundFixedWorkerJobV1, PreparedSolLifecycleFailure> {
        let Self {
            running,
            bound_worker,
            prepared_run,
            executable,
        } = self;
        if let Err(failure) = running.finish_normal(timeout) {
            return Err(prepared_failure(
                failure.map_attention_child(AttentionJobChild::Running),
                prepared_run,
                executable,
                Some(bound_worker),
            ));
        }
        Ok(DrainedBoundFixedWorkerJobV1 {
            _bound_worker: bound_worker,
            _prepared_run: prepared_run,
            _executable: executable,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Stdio;
    use uuid::Uuid;

    use super::super::sol_worker_identity_preflight::bind_fixed_local_sol_worker_to_opened_process;
    use super::super::sol_worker_launch_journal::reopen_attention;
    use super::super::sol_worker_token_security::require_same_opened_process_object;

    const HELPER: &str = "delegated::sol_worker_lifecycle::tests::sleep_helper";

    fn assert_clean_failure<T>(failure: ChildTransitionFailure<T>, expected: &str) {
        match failure {
            ChildTransitionFailure::Clean(reason) => assert_eq!(reason, expected),
            ChildTransitionFailure::Attention { reason, .. } => {
                panic!("expected proved cleanup, received attention: {reason}")
            }
        }
    }

    fn launch_prepared(
        prepared: PreparedSolRunV1,
        executable: PinnedSolWorkerExecutableV1,
        inputs: &PreparedSolLaunchInputsV1,
    ) -> Result<PreparedSuspendedSolJobChild, PreparedSolLifecycleFailure> {
        let reserved = prepared
            .reserve_launch(&executable, inputs)
            .map_err(PreparedSolLifecycleFailure::Clean)?;
        PreparedSuspendedSolJobChild::launch(reserved, executable, inputs)
    }

    struct PreparationFixture {
        base: PathBuf,
        ledger_path: PathBuf,
        output_path: PathBuf,
        authority: SolWorkerNonceAuthorityV1,
    }

    impl PreparationFixture {
        fn new() -> Self {
            let base =
                std::env::temp_dir().join(format!("catdesk-sol-prepared-{}", Uuid::new_v4()));
            let ledger_path = base.join("ledger");
            let output_path = base.join("output");
            fs::create_dir(&base).expect("fixture base");
            fs::create_dir(&ledger_path).expect("fixture ledger");
            fs::create_dir(&output_path).expect("fixture output");
            Self {
                base,
                ledger_path,
                output_path,
                authority: SolWorkerNonceAuthorityV1 {
                    session_id: "session-1".into(),
                    task_id: "T-0483".into(),
                    host_turn_id: "host-turn-1".into(),
                    contract_hash: "b".repeat(64),
                    baseline_commit: "a".repeat(40),
                    allowed_paths: BTreeSet::from(["src/lib.rs".into()]),
                },
            }
        }

        fn prepare_with(
            &self,
            authority: SolWorkerNonceAuthorityV1,
        ) -> Result<PreparedSolRunV1, String> {
            let ledger = ProtectedDirectoryGuard::acquire(&self.ledger_path, LABEL)?;
            let output = ProtectedDirectoryGuard::acquire(&self.output_path, LABEL)?;
            PreparedSolRunV1::prepare(ledger, output, authority)
        }

        fn prepare(&self) -> Result<PreparedSolRunV1, String> {
            self.prepare_with(self.authority.clone())
        }

        fn cleanup(self) {
            for entry in fs::read_dir(&self.ledger_path).expect("list fixture ledger") {
                fs::remove_file(entry.expect("ledger entry").path()).expect("remove ledger record");
            }
            assert_eq!(
                fs::read_dir(&self.output_path)
                    .expect("list fixture output")
                    .count(),
                0,
                "fixture output must be empty at cleanup"
            );
            fs::remove_dir(&self.output_path).expect("remove fixture output");
            fs::remove_dir(&self.ledger_path).expect("remove fixture ledger");
            fs::remove_dir(&self.base).expect("remove fixture base");
        }
    }

    fn configure_helper_command(command: &mut Command, level: &str) {
        command
            .args(["--ignored", "--exact", HELPER])
            .env("CATDESK_SOL_LIFECYCLE_HELPER_LEVEL", level)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    }

    fn helper_command(level: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().expect("test executable"));
        configure_helper_command(&mut command, level);
        command
    }

    fn prepared_helper_inputs(level: &str) -> PreparedSolLaunchInputsV1 {
        PreparedSolLaunchInputsV1 {
            arguments: ["--ignored", "--exact", HELPER]
                .into_iter()
                .map(OsString::from)
                .collect(),
            environment: BTreeMap::from([(
                "CATDESK_SOL_LIFECYCLE_HELPER_LEVEL".into(),
                level.into(),
            )]),
        }
    }

    fn pin_current_test_executable() -> PinnedSolWorkerExecutableV1 {
        let path = std::env::current_exe().expect("test executable");
        let approved_sha256 = format!("{:x}", Sha256::digest(fs::read(&path).expect("read exe")));
        let parent = ProtectedDirectoryGuard::acquire(path.parent().expect("exe parent"), LABEL)
            .expect("pin exe parent");
        PinnedSolWorkerExecutableV1::pin(
            parent,
            path.file_name()
                .expect("exe name")
                .to_str()
                .expect("exe UTF-8"),
            &approved_sha256,
        )
        .expect("pin approved diagnostic executable")
    }

    #[test]
    #[ignore]
    fn sleep_helper() {
        let Ok(level) = std::env::var("CATDESK_SOL_LIFECYCLE_HELPER_LEVEL") else {
            return;
        };
        if level == "failure" {
            std::process::exit(7);
        } else if level == "inherit-check" {
            let raw = std::env::var("CATDESK_SOL_INHERIT_MARKER")
                .expect("explicit marker value")
                .parse::<usize>()
                .expect("numeric marker handle") as *mut c_void;
            let mut flags = 0;
            if unsafe { GetHandleInformation(raw, &mut flags) } != 0 {
                std::process::exit(7);
            }
        } else if level == "argv-check" {
            let expected =
                std::env::var_os("CATDESK_SOL_EXPECTED_ARG").expect("explicit expected argv value");
            let mut args = std::env::args_os();
            let actual = args.find(|arg| arg == "--skip").and_then(|_| args.next());
            if actual.as_ref() != Some(&expected) {
                std::process::exit(7);
            }
        } else if level == "parent" {
            let mut descendant = helper_command("descendant")
                .spawn()
                .expect("spawn descendant");
            std::thread::sleep(Duration::from_secs(20));
            let _ = descendant.wait();
        } else if level == "descendant" {
            std::thread::sleep(Duration::from_secs(20));
        }
    }

    #[test]
    fn normal_success_drains_without_forced_job_termination() {
        let mut command = helper_command("success");
        let owner = OwnedSuspendedJobChild::launch(&mut command).expect("launch in exact Job");
        FORCE_JOB_TERMINATE_FAILURE.with(|slot| slot.set(true));
        let result = owner.finish_normal(Duration::from_secs(5));
        FORCE_JOB_TERMINATE_FAILURE.with(|slot| slot.set(false));
        result.expect("normal zero exit and empty Job without forced termination");
    }

    #[test]
    fn nonzero_exit_cannot_mint_normal_drained_witness() {
        let mut command = helper_command("failure");
        let owner = OwnedSuspendedJobChild::launch(&mut command).expect("launch in exact Job");
        match owner.finish_normal(Duration::from_secs(5)) {
            Err(ChildTransitionFailure::Clean(reason)) => {
                assert!(reason.contains("exited unsuccessfully"));
            }
            Err(ChildTransitionFailure::Attention { reason, .. }) => {
                panic!("unexpected uncertain drain: {reason}")
            }
            Ok(_) => panic!("failed worker cannot mint normal witness"),
        }
    }

    #[test]
    fn timeout_with_failed_termination_retains_exact_running_owner() {
        let mut command = helper_command("descendant");
        let owner = OwnedSuspendedJobChild::launch(&mut command).expect("launch in exact Job");
        FORCE_JOB_TERMINATE_FAILURE.with(|slot| slot.set(true));
        let failure = owner
            .finish_normal(Duration::from_millis(1))
            .expect_err("sleeping child must not complete normally");
        FORCE_JOB_TERMINATE_FAILURE.with(|slot| slot.set(false));
        let owner = match failure {
            ChildTransitionFailure::Attention { reason, owner } => {
                assert!(reason.contains("timed out"));
                assert!(reason.contains("injected Job termination failure"));
                assert!(owner.child.is_some(), "exact child handle remains owned");
                owner
            }
            ChildTransitionFailure::Clean(reason) => {
                panic!("termination uncertainty lost its owner: {reason}")
            }
        };
        owner
            .abort_and_drain(Duration::from_secs(5))
            .expect("retry abort proves exact child and Job drain");
    }

    #[test]
    fn failed_job_readback_retains_owner_after_child_exit() {
        let mut command = helper_command("success");
        let owner = OwnedSuspendedJobChild::launch(&mut command).expect("launch in exact Job");
        FORCE_JOB_ACCOUNTING_FAILURE.with(|slot| slot.set(true));
        let failure = owner
            .finish_normal(Duration::from_secs(5))
            .expect_err("unavailable Job readback must reject witness");
        FORCE_JOB_ACCOUNTING_FAILURE.with(|slot| slot.set(false));
        let owner = match failure {
            ChildTransitionFailure::Attention { reason, owner } => {
                assert!(reason.contains("injected Job accounting failure"));
                assert!(owner.child.is_some());
                owner
            }
            ChildTransitionFailure::Clean(reason) => {
                panic!("Job readback uncertainty lost its owner: {reason}")
            }
        };
        owner
            .abort_and_drain(Duration::from_secs(5))
            .expect("retry obtains complete Job readback");
    }

    #[test]
    fn prepared_launch_failure_retains_turn_and_image_lease_in_attention() {
        let fixture = PreparationFixture::new();
        let prepared = fixture.prepare().expect("prepare one host turn");
        let selected = prepared.nonce.clone();
        let executable = pin_current_test_executable();
        let expected_image = executable.reviewed.identity.sha256.clone();
        let added = fixture.output_path.join("late.txt");
        FORCE_JOB_TERMINATE_FAILURE.with(|slot| slot.set(true));
        FORCE_PREPARED_OUTPUT_TAMPER_AFTER_SPAWN.with(|slot| slot.set(true));
        let result = launch_prepared(prepared, executable, &prepared_helper_inputs("descendant"));
        FORCE_PREPARED_OUTPUT_TAMPER_AFTER_SPAWN.with(|slot| slot.set(false));
        FORCE_JOB_TERMINATE_FAILURE.with(|slot| slot.set(false));
        let attention = match result {
            Err(PreparedSolLifecycleFailure::Attention(owner)) => owner,
            Err(PreparedSolLifecycleFailure::Clean(reason)) => {
                panic!("ambiguous cleanup lost prepared run: {reason}")
            }
            Ok(_) => panic!("changed output must reject prepared launch"),
        };
        assert_eq!(attention.prepared_run.nonce, selected);
        assert_eq!(
            attention.executable.reviewed.identity.sha256,
            expected_image
        );
        assert!(matches!(&attention.child, AttentionJobChild::Suspended(_)));
        attention
            .retry_abort()
            .expect("retry proves terminal suspended child and empty Job");
        fs::remove_file(added).expect("remove exact test output");
        fixture.cleanup();
    }

    #[test]
    fn exact_job_membership_precedes_resume_and_finish_drains_child() {
        let mut command = helper_command("descendant");
        let owner = OwnedSuspendedJobChild::launch(&mut command).expect("launch in exact Job");
        let child = owner.child.as_ref().expect("original child");
        let mut exact_member = 0;
        assert_ne!(
            unsafe {
                IsProcessInJob(
                    child.as_raw_handle(),
                    owner.job.0.as_raw_handle(),
                    &mut exact_member,
                )
            },
            0
        );
        assert_eq!(exact_member, 1);
        assert!(!owner.job.process_list_empty().expect("one Job member"));
        owner
            .abort_and_drain(Duration::from_secs(5))
            .expect("abort and drain Job");
    }

    #[test]
    fn fixed_worker_drain_type_requires_pre_resume_process_binding() {
        let _: fn(
            PreparedSuspendedSolJobChild,
            BoundFixedLocalSolWorkerProcessV1,
        ) -> Result<BoundFixedWorkerJobChild, PreparedSolLifecycleFailure> =
            PreparedSuspendedSolJobChild::bind_and_resume_fixed;
        let _: fn(
            BoundFixedWorkerJobChild,
            Duration,
        ) -> Result<DrainedBoundFixedWorkerJobV1, PreparedSolLifecycleFailure> =
            BoundFixedWorkerJobChild::finish;
        let _: fn(
            OwnedRunningJobChild,
            Duration,
        )
            -> Result<NormalDrainedJobV1, ChildTransitionFailure<OwnedRunningJobChild>> =
            OwnedRunningJobChild::finish_normal;
        let _: fn(DrainedBoundFixedWorkerJobV1) -> StoppedBoundFixedWorkerCandidateV1 =
            DrainedBoundFixedWorkerJobV1::into_stopped_candidate;
        let _: fn(&StoppedBoundFixedWorkerCandidateV1) -> Result<(), String> =
            StoppedBoundFixedWorkerCandidateV1::diagnose_retained_exited_identity;
        let _: fn(&StoppedBoundFixedWorkerCandidateV1) -> Result<(), String> =
            StoppedBoundFixedWorkerCandidateV1::diagnose_retained_host_context;
    }

    #[test]
    fn reviewed_executable_lease_rejects_bad_inputs_and_blocks_replacement() {
        let fixture = PreparationFixture::new();
        let path = fixture.base.join("approved.exe");
        fs::write(&path, b"diagnostic fixture only").expect("fixture executable bytes");
        let digest = format!("{:x}", Sha256::digest(b"diagnostic fixture only"));
        let parent = ProtectedDirectoryGuard::acquire(&fixture.base, LABEL).expect("pin parent");
        assert!(
            PinnedSolWorkerExecutableV1::pin(
                parent.try_clone(LABEL).expect("clone parent"),
                "approved.exe",
                &"0".repeat(64),
            )
            .is_err()
        );
        assert!(
            PinnedSolWorkerExecutableV1::pin(
                parent.try_clone(LABEL).expect("clone parent"),
                "../approved.exe",
                &digest,
            )
            .is_err()
        );
        assert!(
            PinnedSolWorkerExecutableV1::pin(
                parent.try_clone(LABEL).expect("clone parent"),
                "approved.cmd",
                &digest,
            )
            .is_err()
        );
        assert!(
            PinnedSolWorkerExecutableV1::pin(
                parent.try_clone(LABEL).expect("clone parent"),
                "CON.exe",
                &digest,
            )
            .is_err()
        );
        assert!(
            PinnedSolWorkerExecutableV1::pin(
                parent.try_clone(LABEL).expect("clone parent"),
                "approved:stream.exe",
                &digest,
            )
            .is_err()
        );
        let lease = PinnedSolWorkerExecutableV1::pin(parent, "approved.exe", &digest)
            .expect("retain fixture executable");
        assert!(
            fs::OpenOptions::new().write(true).open(&path).is_err(),
            "retained handle denies a new writer"
        );
        lease
            .diagnose_retained_artifact()
            .expect("original artifact can be rehashed through pinned parent");
        let renamed = fixture.base.join("renamed.exe");
        assert!(fs::rename(&path, &renamed).is_err());
        drop(lease);
        fs::rename(&path, &renamed).expect("rename after lease release");
        fs::remove_file(renamed).expect("remove fixture executable");
        fixture.cleanup();
    }

    #[test]
    fn native_image_path_comparison_rejects_case_twin() {
        let expected = "\\Device\\HarddiskVolume1\\codex.exe"
            .encode_utf16()
            .collect::<Vec<_>>();
        let same = expected.clone();
        let case_twin = "\\Device\\HarddiskVolume1\\CODEX.exe"
            .encode_utf16()
            .collect::<Vec<_>>();
        assert!(same_native_image_path(&expected, &same));
        assert!(!same_native_image_path(&expected, &case_twin));
    }

    #[test]
    fn prepared_child_owns_run_and_image_path_before_fixed_bind_attempt() {
        let fixture = PreparationFixture::new();
        let prepared = fixture.prepare().expect("prepare host turn");
        let selected = prepared.nonce.clone();
        let executable = pin_current_test_executable();
        let owner = launch_prepared(prepared, executable, &prepared_helper_inputs("descendant"))
            .expect("launch same-user diagnostic from retained image");
        assert_eq!(owner.prepared_run.nonce, selected);
        let child = owner.suspended.child.as_ref().expect("original child");
        let opened = unsafe { BorrowedHandle::borrow_raw(child.as_raw_handle()) };
        owner
            .executable
            .require_same_image_path(opened)
            .expect("exact child reports retained image path");
        assert!(bind_fixed_local_sol_worker_to_opened_process(opened).is_err());
        let PreparedSuspendedSolJobChild {
            suspended,
            prepared_run,
            executable,
        } = owner;
        assert_clean_failure(suspended.abort("diagnostic only".into()), "diagnostic only");
        drop(prepared_run);
        drop(executable);
        fixture.cleanup();
    }

    #[test]
    fn prepared_native_child_starts_in_job_and_finishes_without_forced_termination() {
        let fixture = PreparationFixture::new();
        let prepared = fixture.prepare().expect("prepare one run");
        let executable = pin_current_test_executable();
        let owner = launch_prepared(prepared, executable, &prepared_helper_inputs("success"))
            .expect("creation-time Job launch");
        let child = owner.suspended.child.as_ref().expect("retained child");
        assert!(matches!(child, OwnedChildProcess::Native(_)));
        owner
            .suspended
            .job
            .require_exact_member(child)
            .expect("exact membership before any resume");
        assert_eq!(
            unsafe { WaitForSingleObject(child.as_raw_handle(), 0) },
            WAIT_TIMEOUT,
            "original process remains suspended"
        );
        assert!(
            owner
                .suspended
                .job
                .accounting()
                .expect("Job accounting")
                .total_processes
                >= 1
        );
        let PreparedSuspendedSolJobChild {
            suspended,
            prepared_run,
            executable,
        } = owner;
        let running = suspended.resume().expect("resume exact member");
        FORCE_JOB_TERMINATE_FAILURE.with(|slot| slot.set(true));
        let result = running.finish_normal(Duration::from_secs(5));
        FORCE_JOB_TERMINATE_FAILURE.with(|slot| slot.set(false));
        result.expect("zero exit and empty Job without forced termination");
        drop(prepared_run);
        drop(executable);
        let reopened = ProtectedDirectoryGuard::acquire(&fixture.ledger_path, LABEL)
            .expect("reopen after native child exit");
        assert!(
            reopen_attention(&reopened, &fixture.authority).contains("ATTENTION"),
            "a completed diagnostic child has no terminal handoff protocol"
        );
        drop(reopened);
        fixture.cleanup();
    }

    #[test]
    fn prepared_native_launch_does_not_inherit_other_host_handles() {
        let fixture = PreparationFixture::new();
        let marker_path = fixture.base.join("inheritable-marker.txt");
        fs::write(&marker_path, b"marker").expect("marker fixture");
        let marker = fs::File::open(&marker_path).expect("open marker");
        assert_ne!(
            unsafe {
                SetHandleInformation(
                    marker.as_raw_handle(),
                    HANDLE_FLAG_INHERIT,
                    HANDLE_FLAG_INHERIT,
                )
            },
            0
        );
        let mut inputs = prepared_helper_inputs("inherit-check");
        inputs.environment.insert(
            "CATDESK_SOL_INHERIT_MARKER".into(),
            (marker.as_raw_handle() as usize).to_string(),
        );
        let owner = launch_prepared(
            fixture.prepare().expect("prepare one run"),
            pin_current_test_executable(),
            &inputs,
        )
        .expect("creation-time Job launch");
        let PreparedSuspendedSolJobChild {
            suspended,
            prepared_run,
            executable,
        } = owner;
        suspended
            .resume()
            .expect("resume exact member")
            .finish_normal(Duration::from_secs(5))
            .expect("child could not use parent's inheritable handle");
        drop(prepared_run);
        drop(executable);
        assert_ne!(
            unsafe { SetHandleInformation(marker.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) },
            0
        );
        drop(marker);
        fs::remove_file(marker_path).expect("remove marker fixture");
        fixture.cleanup();
    }

    #[test]
    fn prepared_native_launch_round_trips_windows_quoted_argv() {
        // --skip accepts an arbitrary filter while leaving the exact ignored
        // helper selected. The helper compares the argv value received after
        // CreateProcessW, rather than only checking our formatter's output.
        for expected in [r"C:\path with spaces\\", r#"embedded "quote" value"#] {
            let fixture = PreparationFixture::new();
            let mut inputs = prepared_helper_inputs("argv-check");
            inputs
                .arguments
                .extend([OsString::from("--skip"), OsString::from(expected)]);
            inputs
                .environment
                .insert("CATDESK_SOL_EXPECTED_ARG".into(), expected.into());
            let owner = launch_prepared(
                fixture.prepare().expect("prepare one run"),
                pin_current_test_executable(),
                &inputs,
            )
            .expect("native launch with quoted argv");
            let PreparedSuspendedSolJobChild {
                suspended,
                prepared_run,
                executable,
            } = owner;
            suspended
                .resume()
                .expect("resume exact Job member")
                .finish_normal(Duration::from_secs(5))
                .expect("helper received exact quoted argv and exited zero");
            drop(prepared_run);
            drop(executable);
            fixture.cleanup();
        }
    }

    #[test]
    fn prepared_native_descendant_remains_in_creation_time_job() {
        let fixture = PreparationFixture::new();
        let owner = launch_prepared(
            fixture.prepare().expect("prepare one run"),
            pin_current_test_executable(),
            &prepared_helper_inputs("parent"),
        )
        .expect("creation-time Job launch");
        let PreparedSuspendedSolJobChild {
            suspended,
            prepared_run,
            executable,
        } = owner;
        let running = suspended.resume().expect("resume original member");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let state = running.job.accounting().expect("retained Job accounting");
            if state.total_processes >= 2 && state.active_processes >= 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "native child descendant did not join Job"
            );
            std::thread::sleep(POLL);
        }
        running
            .abort_and_drain(Duration::from_secs(5))
            .expect("terminate both Job members and retain no witness");
        drop(prepared_run);
        drop(executable);
        fixture.cleanup();
    }

    #[test]
    fn failed_creation_time_spawn_proves_empty_job_before_clean_failure() {
        let fixture = PreparationFixture::new();
        let fake_path = fixture.base.join("fake.exe");
        let fake_bytes = b"not a Windows executable";
        fs::write(&fake_path, fake_bytes).expect("fake image fixture");
        let digest = format!("{:x}", Sha256::digest(fake_bytes));
        let parent = ProtectedDirectoryGuard::acquire(&fixture.base, LABEL).expect("pin parent");
        let fake = PinnedSolWorkerExecutableV1::pin(parent, "fake.exe", &digest)
            .expect("pin diagnostic invalid image");
        let result = launch_prepared(
            fixture.prepare().expect("prepare one run"),
            fake,
            &prepared_helper_inputs("success"),
        );
        match result {
            Err(PreparedSolLifecycleFailure::Clean(reason)) => {
                assert!(reason.contains("creation-time Job launch failed"));
            }
            Err(PreparedSolLifecycleFailure::Attention(owner)) => {
                panic!("unexpected uncertain native create: {}", owner.reason)
            }
            Ok(_) => panic!("invalid image unexpectedly created a child"),
        }
        fs::remove_file(fake_path).expect("remove exact fake image");
        assert!(fixture.prepare().is_err(), "selected nonce stays spent");
        fixture.cleanup();
    }

    #[test]
    fn failed_native_create_with_unavailable_job_readback_retains_retry_owner() {
        let fixture = PreparationFixture::new();
        let fake_path = fixture.base.join("fake.exe");
        let fake_bytes = b"not a Windows executable";
        fs::write(&fake_path, fake_bytes).expect("fake image fixture");
        let digest = format!("{:x}", Sha256::digest(fake_bytes));
        let parent = ProtectedDirectoryGuard::acquire(&fixture.base, LABEL).expect("pin parent");
        let fake = PinnedSolWorkerExecutableV1::pin(parent, "fake.exe", &digest)
            .expect("pin diagnostic invalid image");
        let selected = fixture.prepare().expect("prepare one run");
        let nonce = selected.nonce.clone();
        FORCE_JOB_ACCOUNTING_FAILURE.with(|slot| slot.set(true));
        let result = launch_prepared(selected, fake, &prepared_helper_inputs("success"));
        FORCE_JOB_ACCOUNTING_FAILURE.with(|slot| slot.set(false));
        let attention = match result {
            Err(PreparedSolLifecycleFailure::Attention(owner)) => owner,
            Err(PreparedSolLifecycleFailure::Clean(reason)) => {
                panic!("uncertain Job readback lost owner: {reason}")
            }
            Ok(_) => panic!("invalid image unexpectedly created a child"),
        };
        assert_eq!(attention.prepared_run.nonce, nonce);
        assert!(matches!(
            &attention.child,
            AttentionJobChild::Suspended(owner)
                if owner.creation_failed_without_handle && owner.child.is_none()
        ));
        attention.retry_abort().expect("retry proves empty Job");
        fs::remove_file(fake_path).expect("remove exact fake image");
        fixture.cleanup();
    }

    #[test]
    fn native_launch_rejects_invalid_arguments_and_environment_before_spawn() {
        let invalid_argument = PreparedSolLaunchInputsV1 {
            arguments: vec![OsString::from("bad\0argument")],
            environment: BTreeMap::new(),
        };
        assert!(
            invalid_argument
                .command_line(OsStr::new("approved.exe"))
                .is_err()
        );
        let invalid_environment = PreparedSolLaunchInputsV1 {
            arguments: Vec::new(),
            environment: BTreeMap::from([("PATH=OTHER".into(), "value".into())]),
        };
        assert!(invalid_environment.environment_block().is_err());
        let duplicate_case = PreparedSolLaunchInputsV1 {
            arguments: Vec::new(),
            environment: BTreeMap::from([
                ("CATDESK_TEST".into(), "first".into()),
                ("catdesk_test".into(), "second".into()),
            ]),
        };
        assert!(duplicate_case.environment_block().is_err());
        let quoted =
            quote_windows_argument(OsStr::new("a b\\\"c")).expect("quote explicit argument");
        assert_eq!(
            String::from_utf16(&quoted).expect("UTF-16"),
            "\"a b\\\\\\\"c\""
        );
    }

    #[test]
    fn different_retained_image_path_rejects_exact_suspended_child() {
        let fixture = PreparationFixture::new();
        let other = fixture.base.join("other.exe");
        fs::write(&other, b"different diagnostic image").expect("other image bytes");
        let digest = format!("{:x}", Sha256::digest(b"different diagnostic image"));
        let parent = ProtectedDirectoryGuard::acquire(&fixture.base, LABEL).expect("pin parent");
        let lease = PinnedSolWorkerExecutableV1::pin(parent, "other.exe", &digest)
            .expect("pin other image");
        let mut command = helper_command("descendant");
        let owner = OwnedSuspendedJobChild::launch_suspended(&mut command)
            .expect("launch current test executable suspended");
        let child = owner.child.as_ref().expect("original child");
        let opened = unsafe { BorrowedHandle::borrow_raw(child.as_raw_handle()) };
        assert!(lease.require_same_image_path(opened).is_err());
        assert_clean_failure(owner.abort("image mismatch".into()), "image mismatch");
        drop(lease);
        fs::remove_file(other).expect("remove other image");
        fixture.cleanup();
    }

    #[test]
    fn prepared_run_selects_one_nonce_and_retains_pinned_host_context() {
        let fixture = PreparationFixture::new();
        let prepared = fixture.prepare().expect("prepare one host turn");
        assert_eq!(prepared.authority, fixture.authority);
        assert_eq!(prepared.ledger.path(), fixture.ledger_path);
        assert_eq!(prepared.output_root.path(), fixture.output_path);
        assert_eq!(
            read_active_sol_worker_nonce(&prepared.ledger, &prepared.authority)
                .expect("read selected nonce"),
            prepared.nonce
        );
        prepared.require_ready().expect("still ready before resume");
        drop(prepared);
        assert!(
            fixture.prepare().is_err(),
            "a second preparation cannot select a replacement nonce"
        );
        fixture.cleanup();
    }

    #[test]
    fn reserved_prepared_launch_rejects_changed_inputs_before_child_creation() {
        let fixture = PreparationFixture::new();
        let prepared = fixture.prepare().expect("prepare turn");
        let executable = pin_current_test_executable();
        let inputs = prepared_helper_inputs("success");
        let reserved = prepared
            .reserve_launch(&executable, &inputs)
            .expect("durable reservation");
        assert!(reopen_attention(&reserved.ledger, &reserved.authority).contains("ATTENTION"));
        let mut changed = prepared_helper_inputs("success");
        changed.arguments.push(OsString::from("different"));
        FORCE_PREPARED_OUTPUT_TAMPER_AFTER_SPAWN.with(|slot| slot.set(true));
        let result = PreparedSuspendedSolJobChild::launch(reserved, executable, &changed);
        FORCE_PREPARED_OUTPUT_TAMPER_AFTER_SPAWN.with(|slot| slot.set(false));
        assert!(matches!(result, Err(PreparedSolLifecycleFailure::Clean(_))));
        assert!(
            !fixture.output_path.join("late.txt").exists(),
            "native create must not run"
        );
        assert!(
            fixture.prepare().is_err(),
            "reservation and nonce cannot be replayed"
        );
        fixture.cleanup();
    }

    #[test]
    fn retained_post_run_context_accepts_output_but_rejects_object_and_record_drift() {
        let fixture = PreparationFixture::new();
        let prepared = fixture.prepare().expect("prepare selected turn");
        let mut executable = pin_current_test_executable();
        let reserved = prepared
            .reserve_launch(&executable, &prepared_helper_inputs("success"))
            .expect("durable reservation");
        let output_file = fixture.output_path.join("worker-output.txt");
        fs::write(&output_file, b"diagnostic output").expect("populate output after reserve");
        assert!(reserved.require_still_reserved().is_err());
        reserved
            .diagnose_retained_post_run_context(&executable)
            .expect("retained host context survives output creation");

        let reservation = &reserved.reservation;
        let ledger = &reserved.ledger;
        let output = &reserved.output_root;
        let parent = &executable.parent;
        let component = &executable.component;
        let object = &executable.reviewed.identity.object_identity;
        let digest = &executable.reviewed.identity.sha256;
        assert!(
            reservation
                .diagnose_retained_objects(ledger, ledger, parent, component, object, digest)
                .is_err(),
            "a different retained output directory must fail"
        );
        assert!(
            reservation
                .diagnose_retained_objects(ledger, output, output, component, object, digest)
                .is_err(),
            "a different executable parent must fail"
        );
        assert!(
            reservation
                .diagnose_retained_objects(ledger, output, parent, "other.exe", object, digest)
                .is_err(),
            "a different executable component must fail"
        );
        assert!(
            reservation
                .diagnose_retained_objects(
                    ledger,
                    output,
                    parent,
                    component,
                    "other-object",
                    digest
                )
                .is_err(),
            "a different executable object must fail"
        );
        assert!(
            reservation
                .diagnose_retained_objects(
                    ledger,
                    output,
                    parent,
                    component,
                    object,
                    &"0".repeat(64)
                )
                .is_err(),
            "a different executable digest must fail"
        );

        let original_native_path = executable.native_final_path.clone();
        executable.native_final_path = "different native path".encode_utf16().collect();
        assert!(
            reserved
                .diagnose_retained_post_run_context(&executable)
                .is_err(),
            "a changed native path observation must fail"
        );
        executable.native_final_path = original_native_path;

        let alternate_path = fixture.base.join("alternate.exe");
        let alternate_bytes = b"other diagnostic executable";
        fs::write(&alternate_path, alternate_bytes).expect("alternate executable fixture");
        let alternate_parent =
            ProtectedDirectoryGuard::acquire(&fixture.base, LABEL).expect("pin alternate parent");
        let alternate = PinnedSolWorkerExecutableV1::pin(
            alternate_parent,
            "alternate.exe",
            &format!("{:x}", Sha256::digest(alternate_bytes)),
        )
        .expect("pin alternate executable");
        assert!(
            reserved
                .diagnose_retained_post_run_context(&alternate)
                .is_err(),
            "a different retained executable cannot satisfy the launch record"
        );
        drop(alternate);
        fs::remove_file(alternate_path).expect("remove exact alternate executable");

        let active = fs::read_dir(&fixture.ledger_path)
            .expect("list nonce ledger")
            .map(|entry| entry.expect("ledger entry").path())
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("active-"))
            })
            .expect("active nonce record");
        let active_bytes = fs::read(&active).expect("read active nonce");
        fs::write(&active, b"{").expect("tamper selected nonce");
        assert!(
            reserved
                .diagnose_retained_post_run_context(&executable)
                .is_err(),
            "selected nonce drift must fail"
        );
        fs::write(&active, active_bytes).expect("restore exact selected nonce fixture");

        let record = fixture
            .ledger_path
            .join(format!("launch-started-{}.json", reserved.nonce));
        let mut bytes = fs::read(&record).expect("read reservation");
        bytes[0] ^= 1;
        fs::write(&record, bytes).expect("tamper reservation");
        assert!(
            reserved
                .diagnose_retained_post_run_context(&executable)
                .is_err()
        );
        fs::remove_file(output_file).expect("remove exact diagnostic output");
        drop(reserved);
        drop(executable);
        fixture.cleanup();
    }

    #[test]
    fn invalid_launch_input_cannot_create_a_reservation_or_child() {
        let fixture = PreparationFixture::new();
        let prepared = fixture.prepare().expect("prepare selected turn");
        let executable = pin_current_test_executable();
        let inputs = PreparedSolLaunchInputsV1 {
            arguments: vec![OsString::from("invalid\0argument")],
            environment: BTreeMap::new(),
        };
        assert!(prepared.reserve_launch(&executable, &inputs).is_err());
        assert!(
            !fs::read_dir(&fixture.ledger_path)
                .expect("list ledger")
                .any(|entry| entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with("launch-started-")),
            "invalid inputs must stop before the create-once launch record"
        );
        let ledger =
            ProtectedDirectoryGuard::acquire(&fixture.ledger_path, LABEL).expect("reopen ledger");
        assert!(reopen_attention(&ledger, &fixture.authority).contains("ATTENTION"));
        drop(ledger);
        drop(executable);
        fixture.cleanup();
    }

    #[test]
    fn preparation_rejects_preexisting_output_and_unsafe_allowlist_before_nonce_issue() {
        let fixture = PreparationFixture::new();
        fs::write(fixture.output_path.join("preexisting.txt"), b"old").expect("preexisting output");
        assert!(fixture.prepare().is_err());
        assert_eq!(
            fs::read_dir(&fixture.ledger_path)
                .expect("list empty ledger")
                .count(),
            0
        );
        fs::remove_file(fixture.output_path.join("preexisting.txt"))
            .expect("remove preexisting output");

        for paths in [
            BTreeSet::from(["../src/lib.rs".into()]),
            BTreeSet::from(["CON/file.rs".into()]),
            BTreeSet::from(["src/lib.rs".into(), "src/LIB.rs".into()]),
        ] {
            let mut authority = fixture.authority.clone();
            authority.allowed_paths = paths;
            assert!(fixture.prepare_with(authority).is_err());
            assert_eq!(
                fs::read_dir(&fixture.ledger_path)
                    .expect("list empty ledger")
                    .count(),
                0,
                "invalid allowlist must not select a nonce"
            );
        }
        fixture.cleanup();
    }

    #[test]
    fn preparation_recheck_detects_output_and_ledger_tamper() {
        let fixture = PreparationFixture::new();
        let prepared = fixture.prepare().expect("prepare one host turn");
        let added = fixture.output_path.join("added.txt");
        fs::write(&added, b"late").expect("add output before resume");
        assert!(prepared.require_ready().is_err());
        fs::remove_file(added).expect("remove late output");
        prepared.require_ready().expect("output empty again");

        let issued = fs::read_dir(&fixture.ledger_path)
            .expect("list ledger")
            .map(|entry| entry.expect("ledger entry").path())
            .find(|path| {
                path.file_name()
                    .expect("ledger name")
                    .to_string_lossy()
                    .starts_with("issued-")
            })
            .expect("issued record");
        fs::write(issued, b"tampered").expect("tamper diagnostic ledger");
        assert!(prepared.require_ready().is_err());
        drop(prepared);
        fixture.cleanup();
    }

    #[test]
    fn preparation_rejects_ledger_as_output_and_leaves_selected_turn_in_attention() {
        let fixture = PreparationFixture::new();
        let ledger =
            ProtectedDirectoryGuard::acquire(&fixture.ledger_path, LABEL).expect("pin ledger");
        let output = ProtectedDirectoryGuard::acquire(&fixture.ledger_path, LABEL)
            .expect("pin same directory as output");
        assert!(
            PreparedSolRunV1::prepare(ledger, output, fixture.authority.clone()).is_err(),
            "journal writes make the output root nonempty"
        );
        let ledger = ProtectedDirectoryGuard::acquire(&fixture.ledger_path, LABEL)
            .expect("reopen ledger for diagnostic read");
        assert!(read_active_sol_worker_nonce(&ledger, &fixture.authority).is_ok());
        drop(ledger);
        assert!(
            fixture.prepare().is_err(),
            "selected turn cannot be reissued"
        );
        fixture.cleanup();
    }

    #[test]
    fn same_user_diagnostic_child_cannot_become_fixed_worker_or_supply_another_handle() {
        let fixture = PreparationFixture::new();
        let prepared = fixture
            .prepare()
            .expect("host context alone is not a process bind");
        let mut first_command = helper_command("descendant");
        let mut second_command = helper_command("descendant");
        let first = OwnedSuspendedJobChild::launch_suspended(&mut first_command)
            .expect("first suspended diagnostic child");
        let second = OwnedSuspendedJobChild::launch_suspended(&mut second_command)
            .expect("second suspended diagnostic child");
        let first_handle = unsafe {
            BorrowedHandle::borrow_raw(first.child.as_ref().expect("first child").as_raw_handle())
        };
        let second_handle = unsafe {
            BorrowedHandle::borrow_raw(second.child.as_ref().expect("second child").as_raw_handle())
        };
        require_same_opened_process_object(first_handle, first_handle)
            .expect("same opened process object");
        assert!(require_same_opened_process_object(first_handle, second_handle).is_err());
        assert!(bind_fixed_local_sol_worker_to_opened_process(first_handle).is_err());
        prepared
            .require_ready()
            .expect("host context remains prepared");
        assert_clean_failure(first.abort("diagnostic only".into()), "diagnostic only");
        assert_clean_failure(second.abort("diagnostic only".into()), "diagnostic only");
        drop(prepared);
        fixture.cleanup();
    }

    #[test]
    fn descendant_is_in_same_job_and_finish_drains_entire_job() {
        let mut command = helper_command("parent");
        let owner = OwnedSuspendedJobChild::launch(&mut command).expect("launch in exact Job");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let state = owner.job.accounting().expect("Job accounting");
            if state.total_processes >= 2 && state.active_processes >= 2 {
                break;
            }
            assert!(Instant::now() < deadline, "descendant never joined Job");
            std::thread::sleep(POLL);
        }
        owner
            .abort_and_drain(Duration::from_secs(5))
            .expect("drain child and descendant");
    }

    #[test]
    fn retained_observer_process_handle_does_not_fake_an_active_job_member() {
        let mut command = helper_command("descendant");
        let owner = OwnedSuspendedJobChild::launch(&mut command).expect("launch in exact Job");
        let opened = unsafe {
            BorrowedHandle::borrow_raw(
                owner
                    .child
                    .as_ref()
                    .expect("original child")
                    .as_raw_handle(),
            )
        };
        let observer = opened.try_clone_to_owned().expect("retain opened process");
        owner
            .abort_and_drain(Duration::from_secs(5))
            .expect("drain despite retained process observation");
        drop(observer);
    }

    #[test]
    fn invalid_deadline_cannot_return_a_drain_observation() {
        let mut command = helper_command("descendant");
        let owner = OwnedSuspendedJobChild::launch(&mut command).expect("launch in exact Job");
        let owner = match owner.abort_and_drain(Duration::ZERO) {
            Err(ChildTransitionFailure::Attention { owner, .. }) => owner,
            _ => panic!("invalid deadline must retain its exact Job owner"),
        };
        owner
            .abort_and_drain(Duration::from_secs(5))
            .expect("retry abort drains exact Job");
    }

    #[test]
    fn suspended_child_cleanup_uses_its_original_handle_without_running_it() {
        let mut command = helper_command("descendant");
        command.creation_flags(CREATE_SUSPENDED);
        let child = command.spawn().expect("suspended test child");
        let mut child = OwnedChildProcess::Command(child);
        terminate_original_child_bounded(&mut child).expect("bounded exact-handle cleanup");
        assert_eq!(
            unsafe { WaitForSingleObject(child.as_raw_handle(), 0) },
            WAIT_OBJECT_0
        );
        assert!(child.try_wait().expect("terminal status").is_some());
    }
}
