//! Dormant Windows process-tree ownership mechanics for a future Sol launcher.
//!
//! This private owner creates a suspended child, assigns it to an unnamed,
//! noninheritable kill-on-close Job, checks exact membership, and only then
//! resumes it. The same-user diagnostic route returns no capability. A
//! separate dormant route can bind the still-suspended process object to the
//! opaque fixed-account restricted-token observation before resume, then
//! consume that owner to obtain a narrow drained-Job witness. A private run
//! preparation captures the host task, one selected nonce, and pinned ledger
//! and empty output root before launch. A separate retained, digest-checked
//! executable lease is carried from a private pre-spawn transition through
//! Job drain. No production fixed-account launcher can call that route yet.
//! The witness does not prove
//! escaped descendants, inherited handles, or repository-write exclusion,
//! and no nonce, staging, or repository-use path accepts it.

use std::ffi::c_void;
use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use crate::reviewed_source_snapshot::query_relative_directory_names_bounded;
use crate::windows_protected_fs::{
    ProtectedDirectoryGuard, RetainedReviewedArtifact, retain_reviewed_regular_artifact,
};

use super::sol_worker_identity_preflight::BoundFixedLocalSolWorkerProcessV1;
use super::sol_worker_nonce::{
    SolWorkerNonceAuthorityV1, issue_sol_worker_nonce, read_active_sol_worker_nonce,
};
use super::sol_worker_output::validate_sol_worker_allowed_paths;

const LABEL: &str = "Sol worker lifecycle";
const CREATE_SUSPENDED: u32 = 0x0000_0004;
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

    fn assign_and_check(&self, child: &Child) -> Result<(), String> {
        self.require_limits()?;
        if unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), child.as_raw_handle()) } == 0 {
            return Err(format!("{LABEL} Job assignment failed"));
        }
        self.require_exact_member(child)
    }

    fn require_exact_member(&self, child: &Child) -> Result<(), String> {
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
        self.require_limits()?;
        if unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) } == 0 {
            return Err(format!("{LABEL} Job termination failed"));
        }
        Ok(())
    }

    fn accounting(&self) -> Result<BasicAccounting, String> {
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

/// Private diagnostic launcher. Its `Command` inherits the current user and
/// cannot by itself create the fixed-account worker or the drained witness.
struct OwnedSuspendedJobChild {
    // Job drops first on every early return and terminates active members.
    job: PrivateJob,
    child: Option<Child>,
}

struct OwnedRunningJobChild {
    // Job drops first on every early return and terminates active members.
    job: PrivateJob,
    child: Option<Child>,
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

/// Launch-scoped input only. A future trusted host must supply the approved
/// digest and a pinned directory chain rooted in its immutable policy. A
/// caller-provided digest by itself is not an approval or an image-section
/// attestation. The retained file handle denies new write/delete opens while
/// this value remains alive.
struct PinnedSolWorkerExecutableV1 {
    parent: ProtectedDirectoryGuard,
    component: String,
    reviewed: RetainedReviewedArtifact,
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
        reviewed.native_final_path(LABEL)?;
        parent.assert_stable(LABEL)?;
        Ok(Self {
            parent,
            component: component.into(),
            reviewed,
        })
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

/// Consumes the run and image lease before constructing the child command.
/// The current `Command` path inherits the host user and remains diagnostic;
/// it cannot create a fixed-account witness on this host. An error can release
/// the lease without a proved terminal child when bounded cleanup fails. A
/// future positive launcher needs a retained attention owner for that case.
struct PreparedSuspendedSolJobChild {
    suspended: OwnedSuspendedJobChild,
    prepared_run: PreparedSolRunV1,
    executable: PinnedSolWorkerExecutableV1,
}

impl PreparedSuspendedSolJobChild {
    fn launch<F>(
        prepared_run: PreparedSolRunV1,
        executable: PinnedSolWorkerExecutableV1,
        configure: F,
    ) -> Result<Self, String>
    where
        F: FnOnce(&mut Command),
    {
        prepared_run.require_ready()?;
        executable.parent.assert_stable(LABEL)?;
        let mut command = Command::new(executable.launch_path());
        configure(&mut command);
        let suspended = OwnedSuspendedJobChild::launch_suspended(&mut command)?;
        let Some(child) = suspended.child.as_ref() else {
            return Err(suspended.abort(format!("{LABEL} original child missing")));
        };
        let opened = unsafe { BorrowedHandle::borrow_raw(child.as_raw_handle()) };
        if let Err(error) = executable.require_same_image_path(opened) {
            return Err(suspended.abort(error));
        }
        if let Err(error) = prepared_run.require_ready() {
            return Err(suspended.abort(error));
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
    ) -> Result<BoundFixedWorkerJobChild, String> {
        let Self {
            suspended,
            prepared_run,
            executable,
        } = self;
        let Some(child) = suspended.child.as_ref() else {
            return Err(suspended.abort(format!("{LABEL} original child missing")));
        };
        let opened = unsafe { BorrowedHandle::borrow_raw(child.as_raw_handle()) };
        if let Err(error) = bound_worker.require_same_opened_process(opened) {
            return Err(suspended.abort(error));
        }
        if let Err(error) = executable.require_same_image_path(opened) {
            return Err(suspended.abort(error));
        }
        if let Err(error) = prepared_run.require_ready() {
            return Err(suspended.abort(error));
        }
        let running = suspended.resume()?;
        Ok(BoundFixedWorkerJobChild {
            running,
            bound_worker,
            prepared_run,
            executable,
        })
    }
}

impl PreparedSolRunV1 {
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
    prepared_run: PreparedSolRunV1,
    executable: PinnedSolWorkerExecutableV1,
}

/// A necessary-condition observation: the bound original process and all
/// members observed in its retained Job have drained. It says nothing about
/// processes launched outside that Job or handles held by other processes.
/// Private fields and the lack of `Clone` prevent a caller-created claim.
pub(crate) struct DrainedBoundFixedWorkerJobV1 {
    _bound_worker: BoundFixedLocalSolWorkerProcessV1,
    _prepared_run: PreparedSolRunV1,
    _executable: PinnedSolWorkerExecutableV1,
}

/// The original process handle is the cleanup target. A failed assignment
/// leaves the process suspended and outside the Job, so Job close is not a
/// substitute for an exact-handle termination attempt. Never call blocking
/// `Child::wait` after ignoring a termination failure.
fn terminate_original_child_bounded(child: &mut Child) -> Result<(), String> {
    match unsafe { WaitForSingleObject(child.as_raw_handle(), 0) } {
        WAIT_OBJECT_0 => return Ok(()),
        WAIT_TIMEOUT => {}
        _ => return Err(format!("{LABEL} original child state unavailable")),
    }
    if child.kill().is_err()
        && unsafe { WaitForSingleObject(child.as_raw_handle(), 0) } != WAIT_OBJECT_0
    {
        return Err(format!("{LABEL} original child termination failed"));
    }
    let deadline = Instant::now() + FAILED_LAUNCH_CLEANUP;
    loop {
        match unsafe { WaitForSingleObject(child.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => return Ok(()),
            WAIT_TIMEOUT if Instant::now() < deadline => std::thread::sleep(POLL),
            WAIT_TIMEOUT => return Err(format!("{LABEL} original child termination timed out")),
            _ => return Err(format!("{LABEL} original child wait unavailable")),
        }
    }
}

impl OwnedSuspendedJobChild {
    fn launch_suspended(command: &mut Command) -> Result<Self, String> {
        let job = PrivateJob::create()?;
        command.creation_flags(CREATE_SUSPENDED);
        let mut child = command
            .spawn()
            .map_err(|_| format!("{LABEL} suspended launch failed"))?;
        if let Err(error) = job.assign_and_check(&child) {
            return match terminate_original_child_bounded(&mut child) {
                Ok(()) => Err(error),
                Err(cleanup) => Err(format!("{error}; {cleanup}")),
            };
        }
        Ok(Self {
            job,
            child: Some(child),
        })
    }

    fn launch(command: &mut Command) -> Result<OwnedRunningJobChild, String> {
        Self::launch_suspended(command)?.resume()
    }

    fn abort(mut self, error: String) -> String {
        let job_error = self.job.terminate().err();
        let child_error = self
            .child
            .take()
            .and_then(|mut child| terminate_original_child_bounded(&mut child).err());
        match (job_error, child_error) {
            (None, None) => error,
            (Some(job), None) => format!("{error}; {job}"),
            (None, Some(child)) => format!("{error}; {child}"),
            (Some(job), Some(child)) => format!("{error}; {job}; {child}"),
        }
    }

    fn resume(self) -> Result<OwnedRunningJobChild, String> {
        let Some(child) = self.child.as_ref() else {
            return Err(self.abort(format!("{LABEL} original child missing")));
        };
        if let Err(error) = self.job.require_exact_member(child) {
            return Err(self.abort(error));
        }
        if unsafe { NtResumeProcess(child.as_raw_handle()) } < 0 {
            return Err(self.abort(format!("{LABEL} child resume failed")));
        }
        let Self { job, child } = self;
        Ok(OwnedRunningJobChild { job, child })
    }
}

impl OwnedRunningJobChild {
    /// Consume the owner and return a diagnostic only. The child is waited
    /// on through its original handle; its handle is then released before
    /// checking the retained Job's active-process count and bounded process
    /// ID list. Nonzero counts, unavailable queries or deadline expiry fail
    /// closed.
    fn finish(mut self, timeout: Duration) -> Result<(), String> {
        if timeout.is_zero() || timeout > MAX_DRAIN {
            return Err(format!("{LABEL} drain deadline invalid"));
        }
        let deadline = Instant::now() + timeout;
        self.job.terminate()?;
        let mut child = self
            .child
            .take()
            .ok_or_else(|| format!("{LABEL} original child missing"))?;
        loop {
            match unsafe { WaitForSingleObject(child.as_raw_handle(), 0) } {
                WAIT_OBJECT_0 => break,
                WAIT_TIMEOUT if Instant::now() < deadline => std::thread::sleep(POLL),
                WAIT_TIMEOUT => return Err(format!("{LABEL} original child still running")),
                _ => return Err(format!("{LABEL} original child wait unavailable")),
            }
        }
        if child
            .try_wait()
            .map_err(|_| format!("{LABEL} original child status unavailable"))?
            .is_none()
        {
            return Err(format!("{LABEL} original child status is not terminal"));
        }
        drop(child);
        loop {
            self.job.require_limits()?;
            let state = self.job.accounting()?;
            if state.total_processes >= 1
                && state.active_processes == 0
                && self.job.process_list_empty()?
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!("{LABEL} Job still has active processes"));
            }
            std::thread::sleep(POLL);
        }
    }
}

impl BoundFixedWorkerJobChild {
    /// A normal worker may already have exited at this point, so the live
    /// token observation is intentionally checked before resume, not here.
    /// This only drains the original and Job-associated processes.
    fn finish(self, timeout: Duration) -> Result<DrainedBoundFixedWorkerJobV1, String> {
        self.running.finish(timeout)?;
        Ok(DrainedBoundFixedWorkerJobV1 {
            _bound_worker: self.bound_worker,
            _prepared_run: self.prepared_run,
            _executable: self.executable,
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
    use super::super::sol_worker_token_security::require_same_opened_process_object;

    const HELPER: &str = "delegated::sol_worker_lifecycle::tests::sleep_helper";

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
        if level == "parent" {
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
        owner.finish(Duration::from_secs(5)).expect("drain Job");
    }

    #[test]
    fn fixed_worker_drain_type_requires_pre_resume_process_binding() {
        let _: fn(
            PreparedSuspendedSolJobChild,
            BoundFixedLocalSolWorkerProcessV1,
        ) -> Result<BoundFixedWorkerJobChild, String> =
            PreparedSuspendedSolJobChild::bind_and_resume_fixed;
        let _: fn(
            BoundFixedWorkerJobChild,
            Duration,
        ) -> Result<DrainedBoundFixedWorkerJobV1, String> = BoundFixedWorkerJobChild::finish;
        let _: fn(OwnedRunningJobChild, Duration) -> Result<(), String> =
            OwnedRunningJobChild::finish;
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
        let owner = PreparedSuspendedSolJobChild::launch(prepared, executable, |command| {
            configure_helper_command(command, "descendant");
        })
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
        assert_eq!(suspended.abort("diagnostic only".into()), "diagnostic only");
        drop(prepared_run);
        drop(executable);
        fixture.cleanup();
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
        assert_eq!(owner.abort("image mismatch".into()), "image mismatch");
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
        assert_eq!(first.abort("diagnostic only".into()), "diagnostic only");
        assert_eq!(second.abort("diagnostic only".into()), "diagnostic only");
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
            .finish(Duration::from_secs(5))
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
            .finish(Duration::from_secs(5))
            .expect("drain despite retained process observation");
        drop(observer);
    }

    #[test]
    fn invalid_deadline_cannot_return_a_drain_observation() {
        let mut command = helper_command("descendant");
        let owner = OwnedSuspendedJobChild::launch(&mut command).expect("launch in exact Job");
        assert!(owner.finish(Duration::ZERO).is_err());
    }

    #[test]
    fn suspended_child_cleanup_uses_its_original_handle_without_running_it() {
        let mut command = helper_command("descendant");
        command.creation_flags(CREATE_SUSPENDED);
        let mut child = command.spawn().expect("suspended test child");
        terminate_original_child_bounded(&mut child).expect("bounded exact-handle cleanup");
        assert_eq!(
            unsafe { WaitForSingleObject(child.as_raw_handle(), 0) },
            WAIT_OBJECT_0
        );
        assert!(child.try_wait().expect("terminal status").is_some());
    }
}
