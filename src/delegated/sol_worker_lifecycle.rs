//! Dormant Windows process-tree ownership mechanics for a future Sol launcher.
//!
//! This private owner creates a suspended child, assigns it to an unnamed,
//! noninheritable kill-on-close Job, checks exact membership, and only then
//! resumes it. The same-user diagnostic route returns no capability. A
//! separate dormant route can bind the still-suspended process object to the
//! opaque fixed-account restricted-token observation before resume, then
//! consume that owner to obtain a narrow drained-Job witness. No production
//! fixed-account launcher can call that route yet. The witness does not prove
//! escaped descendants, inherited handles, or repository-write exclusion,
//! and no nonce, staging, or repository-use path accepts it.

use std::ffi::c_void;
use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use super::sol_worker_identity_preflight::BoundFixedLocalSolWorkerProcessV1;

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

/// Only a still-suspended, exact Job member whose opened process was bound to
/// the fixed local account can reach this state. This is private while a
/// trusted fixed-account launcher and host-held run/output-root binding are
/// absent.
struct BoundFixedWorkerJobChild {
    running: OwnedRunningJobChild,
    bound_worker: BoundFixedLocalSolWorkerProcessV1,
}

/// A necessary-condition observation: the bound original process and all
/// members observed in its retained Job have drained. It says nothing about
/// processes launched outside that Job or handles held by other processes.
/// Private fields and the lack of `Clone` prevent a caller-created claim.
pub(crate) struct DrainedBoundFixedWorkerJobV1 {
    _bound_worker: BoundFixedLocalSolWorkerProcessV1,
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

    /// Bind the real opened process object while it is still suspended. The
    /// R20 observation also checks the live fixed-account restricted token.
    /// On any failure the suspended original process is terminated and no
    /// bound or drained value is returned.
    fn bind_and_resume_fixed(
        self,
        bound_worker: BoundFixedLocalSolWorkerProcessV1,
    ) -> Result<BoundFixedWorkerJobChild, String> {
        let Some(child) = self.child.as_ref() else {
            return Err(self.abort(format!("{LABEL} original child missing")));
        };
        let opened = unsafe { BorrowedHandle::borrow_raw(child.as_raw_handle()) };
        if let Err(error) = bound_worker.require_same_opened_process(opened) {
            return Err(self.abort(error));
        }
        let running = self.resume()?;
        Ok(BoundFixedWorkerJobChild {
            running,
            bound_worker,
        })
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    use super::super::sol_worker_identity_preflight::bind_fixed_local_sol_worker_to_opened_process;
    use super::super::sol_worker_token_security::require_same_opened_process_object;

    const HELPER: &str = "delegated::sol_worker_lifecycle::tests::sleep_helper";

    fn helper_command(level: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().expect("test executable"));
        command
            .args(["--ignored", "--exact", HELPER])
            .env("CATDESK_SOL_LIFECYCLE_HELPER_LEVEL", level)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
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
            OwnedSuspendedJobChild,
            BoundFixedLocalSolWorkerProcessV1,
        ) -> Result<BoundFixedWorkerJobChild, String> =
            OwnedSuspendedJobChild::bind_and_resume_fixed;
        let _: fn(
            BoundFixedWorkerJobChild,
            Duration,
        ) -> Result<DrainedBoundFixedWorkerJobV1, String> = BoundFixedWorkerJobChild::finish;
        let _: fn(OwnedRunningJobChild, Duration) -> Result<(), String> =
            OwnedRunningJobChild::finish;
    }

    #[test]
    fn same_user_diagnostic_child_cannot_become_fixed_worker_or_supply_another_handle() {
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
        assert_eq!(first.abort("diagnostic only".into()), "diagnostic only");
        assert_eq!(second.abort("diagnostic only".into()), "diagnostic only");
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
