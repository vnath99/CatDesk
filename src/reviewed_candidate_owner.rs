//! Process ownership for the optional reviewed-candidate parent launcher.
//!
//! A suspended child is assigned to a private Windows kill-on-close job before
//! any candidate code runs. An ordinary candidate remains owned until exit.
//! A reviewed reconciliation may durably prepare a persistent handoff, then
//! disarm and release the job while retaining explicit failure cleanup until
//! the final handle close.

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
    const JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION: i32 = 1;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

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

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> *mut c_void;
        fn SetInformationJobObject(
            job: *mut c_void,
            class: i32,
            information: *mut c_void,
            length: u32,
        ) -> i32;
        fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
        fn TerminateJobObject(job: *mut c_void, exit_code: u32) -> i32;
        fn QueryInformationJobObject(
            job: *mut c_void,
            class: i32,
            information: *mut c_void,
            length: u32,
            returned: *mut u32,
        ) -> i32;
        fn IsProcessInJob(process: *mut c_void, job: *mut c_void, result: *mut i32) -> i32;
        fn GetCurrentProcess() -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtResumeProcess(process: *mut c_void) -> i32;
    }

    struct KillOnCloseJob(*mut c_void);

    impl KillOnCloseJob {
        fn create() -> Result<Self, String> {
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err("reviewed candidate owner job unavailable".into());
            }
            let mut limits = ExtendedLimits::default();
            limits.basic.flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = unsafe {
                SetInformationJobObject(
                    handle,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                    (&mut limits as *mut ExtendedLimits).cast(),
                    std::mem::size_of::<ExtendedLimits>() as u32,
                )
            };
            if configured == 0 {
                unsafe { CloseHandle(handle) };
                return Err("reviewed candidate owner job unavailable".into());
            }
            Ok(Self(handle))
        }

        fn assign(&self, child: &Child) -> Result<(), String> {
            if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle().cast()) } == 0 {
                return Err("reviewed candidate child job assignment failed".into());
            }
            Ok(())
        }

        fn terminate(&self) -> Result<(), String> {
            if self.0.is_null() || unsafe { TerminateJobObject(self.0, 1) } == 0 {
                return Err("reviewed candidate child job termination failed".into());
            }
            Ok(())
        }

        fn active_processes(&self) -> Result<u32, String> {
            let mut accounting = BasicAccounting::default();
            if self.0.is_null()
                || unsafe {
                    QueryInformationJobObject(
                        self.0,
                        JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION,
                        (&mut accounting as *mut BasicAccounting).cast(),
                        std::mem::size_of::<BasicAccounting>() as u32,
                        std::ptr::null_mut(),
                    )
                } == 0
            {
                return Err("reviewed candidate Job accounting unavailable".into());
            }
            Ok(accounting.active_processes)
        }

        fn contains(&self, child: &Child) -> Result<(), String> {
            let mut member = 0;
            if self.0.is_null()
                || unsafe { IsProcessInJob(child.as_raw_handle().cast(), self.0, &mut member) } == 0
                || member != 1
            {
                return Err("reviewed candidate child Job membership unavailable".into());
            }
            Ok(())
        }

        fn disarm(&self) -> Result<(), String> {
            let mut limits = ExtendedLimits::default();
            if self.0.is_null()
                || unsafe {
                    QueryInformationJobObject(
                        self.0,
                        JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                        (&mut limits as *mut ExtendedLimits).cast(),
                        std::mem::size_of::<ExtendedLimits>() as u32,
                        std::ptr::null_mut(),
                    )
                } == 0
                || limits.basic.flags != JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            {
                return Err("reviewed candidate Job limits changed".into());
            }
            limits.basic.flags = 0;
            if unsafe {
                SetInformationJobObject(
                    self.0,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                    (&mut limits as *mut ExtendedLimits).cast(),
                    std::mem::size_of::<ExtendedLimits>() as u32,
                )
            } == 0
            {
                return Err("reviewed candidate Job release failed".into());
            }
            let mut observed = ExtendedLimits::default();
            if unsafe {
                QueryInformationJobObject(
                    self.0,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                    (&mut observed as *mut ExtendedLimits).cast(),
                    std::mem::size_of::<ExtendedLimits>() as u32,
                    std::ptr::null_mut(),
                )
            } == 0
                || observed.basic.flags != 0
            {
                return Err("reviewed candidate Job release is unproven".into());
            }
            Ok(())
        }

        fn require_disarmed(&self) -> Result<(), String> {
            let mut observed = ExtendedLimits::default();
            if self.0.is_null()
                || unsafe {
                    QueryInformationJobObject(
                        self.0,
                        JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                        (&mut observed as *mut ExtendedLimits).cast(),
                        std::mem::size_of::<ExtendedLimits>() as u32,
                        std::ptr::null_mut(),
                    )
                } == 0
                || observed.basic.flags != 0
            {
                return Err("reviewed candidate Job release is unproven".into());
            }
            Ok(())
        }

        fn close(&mut self) -> Result<(), String> {
            if self.0.is_null() {
                return Ok(());
            }
            if unsafe { CloseHandle(self.0) } == 0 {
                return Err("reviewed candidate child job close failed".into());
            }
            self.0 = std::ptr::null_mut();
            Ok(())
        }
    }

    impl Drop for KillOnCloseJob {
        fn drop(&mut self) {
            let _ = self.close();
        }
    }

    pub(crate) enum SpawnFailure {
        /// Command::spawn returned an error: no Child was handed to the owner.
        NoChildCreated,
        /// Ownership/setup failure is not authority to launch another image.
        Owner(String),
    }

    impl SpawnFailure {
        fn message(self) -> String {
            match self {
                Self::NoChildCreated => "reviewed candidate child spawn failed".into(),
                Self::Owner(message) => message,
            }
        }
    }

    pub(crate) struct OwnedCandidateChild {
        child: Option<Child>,
        // Initially kill-on-close; after disarm, explicit termination remains
        // available until the final release closes this handle.
        job: KillOnCloseJob,
        disarmed: bool,
        released: bool,
    }

    impl OwnedCandidateChild {
        pub(crate) fn spawn(command: &mut Command) -> Result<Self, String> {
            Self::spawn_classified(command).map_err(SpawnFailure::message)
        }

        pub(crate) fn spawn_classified(command: &mut Command) -> Result<Self, SpawnFailure> {
            let job = KillOnCloseJob::create().map_err(SpawnFailure::Owner)?;
            // Keep console Ctrl+C on the parent so its bounded stdin stop
            // controls the child's graceful journal/root retirement.
            command.creation_flags(CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP);
            let mut child = command.spawn().map_err(|_| SpawnFailure::NoChildCreated)?;
            if let Err(error) = job.assign(&child) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SpawnFailure::Owner(error));
            }
            if unsafe { NtResumeProcess(child.as_raw_handle().cast()) } < 0 {
                drop(job);
                let _ = child.kill();
                let _ = child.wait();
                return Err(SpawnFailure::Owner(
                    "reviewed candidate child resume failed".into(),
                ));
            }
            if let Err(error) = job.contains(&child) {
                let _ = job.terminate();
                let _ = child.wait();
                return Err(SpawnFailure::Owner(error));
            }
            Ok(Self {
                child: Some(child),
                job,
                disarmed: false,
                released: false,
            })
        }

        /// A reconciliation worker must not inherit a second, unrelated Job:
        /// releasing this private Job cannot preserve a child owned elsewhere.
        pub(crate) fn require_uncontained_owner() -> Result<(), String> {
            let mut in_job = 0;
            if unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) }
                == 0
                || in_job != 0
            {
                return Err("reviewed candidate owner has an outer Job".into());
            }
            Ok(())
        }

        pub(crate) fn pid(&self) -> u32 {
            self.child.as_ref().expect("owned child exists").id()
        }

        pub(crate) fn child_mut(&mut self) -> &mut Child {
            self.child.as_mut().expect("owned child exists")
        }

        /// Terminate the complete owned Job and prove its active process count
        /// reached zero before a caller restores any mapped release bytes.
        pub(crate) fn terminate_tree_and_confirm_quiescent(&mut self) -> Result<(), String> {
            if self.released || self.job.0.is_null() {
                return Err("reviewed candidate Job is no longer owned".into());
            }
            let termination = self.job.terminate();
            let deadline = Instant::now() + Duration::from_secs(10);
            if !self.wait_until(deadline)? {
                return Err("reviewed candidate child exit is unproven".into());
            }
            // Windows may retain a terminated process in Job accounting while
            // our Child still holds its process handle. Close that exact handle
            // before requiring zero active processes for the complete tree.
            drop(self.child.take());
            loop {
                if self.job.active_processes()? == 0 {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err("reviewed candidate Job exit is unproven".into());
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            // If TerminateJobObject itself failed but exact accounting proves
            // the tree has exited, there is no live process to restore under.
            let _ = termination;
            self.job.close()?;
            self.released = true;
            Ok(())
        }

        /// Clear kill-on-close only after the caller has durably recorded an
        /// exact pending serving handoff. Keep the Job handle for a final
        /// readback and explicit whole-tree termination on failure.
        pub(crate) fn disarm_live_retaining_job(&mut self) -> Result<(), String> {
            if self.released || self.disarmed || self.job.0.is_null() {
                return Err("reviewed candidate Job is no longer owned".into());
            }
            if self
                .child_mut()
                .try_wait()
                .map_err(|_| "reviewed candidate state unavailable")?
                .is_some()
            {
                return Err("reviewed candidate exited before Job release".into());
            }
            self.job
                .contains(self.child.as_ref().expect("owned child exists"))?;
            self.job.disarm()?;
            self.disarmed = true;
            Ok(())
        }

        /// Close the disarmed Job after a caller readback. The caller must
        /// independently prove the exact daemon is still serving after this
        /// close before recording final reconciliation success.
        pub(crate) fn release_live(&mut self) -> Result<(), String> {
            if self.released || !self.disarmed || self.job.0.is_null() {
                return Err("reviewed candidate Job release is unavailable".into());
            }
            if self
                .child_mut()
                .try_wait()
                .map_err(|_| "reviewed candidate state unavailable")?
                .is_some()
            {
                return Err("reviewed candidate exited before Job release".into());
            }
            self.job
                .contains(self.child.as_ref().expect("owned child exists"))?;
            self.job.require_disarmed()?;
            // Keep explicit termination authority until this close succeeds.
            self.job.close()?;
            self.released = true;
            Ok(())
        }

        fn wait_until(&mut self, deadline: Instant) -> Result<bool, String> {
            let Some(child) = self.child.as_mut() else {
                return Ok(true);
            };
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return Ok(true),
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Ok(None) => return Ok(false),
                    Err(_) => return Err("reviewed candidate child status unavailable".into()),
                }
            }
        }

        fn shutdown_exact(&mut self) -> Result<(), String> {
            // Terminate explicitly because a failed release may already have
            // cleared kill-on-close. Close remains the crash-safe fallback when
            // the Job is still armed.
            let termination = self.job.terminate();
            let job_closed = if self.job.close().is_ok() {
                true
            } else {
                // If CloseHandle failed while the job handle remains valid,
                // explicitly terminate its whole process tree and retry close.
                let _ = self.job.terminate();
                self.job.close().is_ok()
            };
            if !job_closed {
                if let Some(child) = self.child.as_mut() {
                    let _ = child.kill();
                }
            }
            if !self.wait_until(Instant::now() + Duration::from_secs(5))? {
                // Child::kill uses the retained process handle, never a PID
                // lookup. Reap it with a second bound after the fallback.
                if let Some(child) = self.child.as_mut() {
                    let _ = child.kill();
                }
                if !self.wait_until(Instant::now() + Duration::from_secs(2))? {
                    return Err("reviewed candidate child termination timed out".into());
                }
            }
            if !job_closed {
                return Err("reviewed candidate child job close failed".into());
            }
            if termination.is_err() {
                return Err("reviewed candidate child tree termination unproven".into());
            }
            self.released = true;
            Ok(())
        }

        pub(crate) fn terminate_and_wait(mut self) -> Result<(), String> {
            self.shutdown_exact()
        }
    }

    impl Drop for OwnedCandidateChild {
        fn drop(&mut self) {
            if self.released {
                return;
            }
            if let Err(error) = self.shutdown_exact() {
                eprintln!("reviewed candidate owner cleanup failed: {error}");
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::OwnedCandidateChild;
        use std::io::{BufRead, BufReader, Write};
        use std::process::{Command, Stdio};
        use std::time::Duration;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn WaitForSingleObject(handle: *mut std::ffi::c_void, timeout_ms: u32) -> u32;
            fn TerminateProcess(handle: *mut std::ffi::c_void, exit_code: u32) -> i32;
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }

        #[test]
        fn classified_spawn_rejection_preserves_legacy_error_without_child() {
            let missing =
                std::env::temp_dir().join(format!("catdesk-missing-{}.exe", uuid::Uuid::new_v4()));
            assert!(!missing.exists());
            assert!(matches!(
                OwnedCandidateChild::spawn_classified(&mut Command::new(&missing)),
                Err(super::SpawnFailure::NoChildCreated)
            ));
            assert_eq!(
                OwnedCandidateChild::spawn(&mut Command::new(&missing))
                    .err()
                    .unwrap(),
                "reviewed candidate child spawn failed"
            );
        }

        #[test]
        fn suspended_child_is_owned_and_terminated_with_job() {
            let mut command = Command::new(std::env::current_exe().expect("test executable"));
            command
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::sleep_child",
                ])
                .env("CATDESK_OWNER_JOB_TEST_CHILD", "1")
                .stdout(Stdio::null());
            let mut owned = OwnedCandidateChild::spawn(&mut command).expect("owned child");
            assert!(owned.pid() > 0);
            assert!(owned.child_mut().try_wait().expect("child state").is_none());
            owned.terminate_and_wait().expect("terminate owned child");
        }

        #[test]
        fn sleep_child() {
            if std::env::var_os("CATDESK_OWNER_JOB_TEST_CHILD").is_some() {
                std::thread::sleep(Duration::from_secs(60));
            }
        }

        #[test]
        fn child_with_descendant() {
            if std::env::var_os("CATDESK_OWNER_JOB_TEST_DESCENDANT").is_none() {
                return;
            }
            let mut descendant = Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::sleep_child",
                ])
                .env("CATDESK_OWNER_JOB_TEST_CHILD", "1")
                .stdout(Stdio::null())
                .spawn()
                .expect("descendant process");
            println!("DESCENDANT_PID={}", descendant.id());
            std::io::stdout().flush().expect("flush descendant PID");
            std::thread::sleep(Duration::from_secs(60));
            let _ = descendant.kill();
            let _ = descendant.wait();
        }

        fn spawn_with_descendant() -> (OwnedCandidateChild, u32) {
            let mut command = Command::new(std::env::current_exe().expect("test executable"));
            command
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::child_with_descendant",
                    "--nocapture",
                ])
                .env("CATDESK_OWNER_JOB_TEST_DESCENDANT", "1")
                .stdout(Stdio::piped());
            let mut owned = OwnedCandidateChild::spawn(&mut command).expect("owned child");
            let stdout = owned.child_mut().stdout.take().expect("child stdout");
            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).expect("child output") == 0 {
                        return;
                    }
                    if let Some(value) = line.split("DESCENDANT_PID=").nth(1) {
                        ready_tx
                            .send(value.trim().parse::<u32>().expect("descendant PID"))
                            .expect("send descendant PID");
                        return;
                    }
                }
            });
            let descendant_pid = ready_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("bounded descendant readiness");
            reader.join().expect("ready reader");
            (owned, descendant_pid)
        }

        #[test]
        fn release_after_durable_commit_preserves_child_and_descendant() {
            const PROCESS_TERMINATE: u32 = 0x0001;
            const SYNCHRONIZE: u32 = 0x0010_0000;
            const WAIT_OBJECT_0: u32 = 0;
            const WAIT_TIMEOUT: u32 = 0x0000_0102;
            let (mut owned, descendant_pid) = spawn_with_descendant();
            let child = unsafe { OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, 0, owned.pid()) };
            let descendant =
                unsafe { OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, 0, descendant_pid) };
            assert!(!child.is_null() && !descendant.is_null());
            let disarm = owned.disarm_live_retaining_job();
            let release = if disarm.is_ok() {
                owned.release_live()
            } else {
                Err("reviewed candidate disarm failed".into())
            };
            drop(owned);
            let child_outcome = unsafe { WaitForSingleObject(child, 250) };
            let descendant_outcome = unsafe { WaitForSingleObject(descendant, 250) };
            // These exact fixture handles are cleanup authority, including if
            // a release assertion below fails.
            unsafe { TerminateProcess(child, 1) };
            unsafe { TerminateProcess(descendant, 1) };
            let child_ended = unsafe { WaitForSingleObject(child, 5_000) };
            let descendant_ended = unsafe { WaitForSingleObject(descendant, 5_000) };
            unsafe { CloseHandle(child) };
            unsafe { CloseHandle(descendant) };
            disarm.expect("durable handoff disarm");
            release.expect("durable handoff release");
            assert_eq!(child_outcome, WAIT_TIMEOUT, "released child remains alive");
            assert_eq!(
                descendant_outcome, WAIT_TIMEOUT,
                "released descendant remains alive"
            );
            assert_eq!(child_ended, WAIT_OBJECT_0);
            assert_eq!(descendant_ended, WAIT_OBJECT_0);
        }

        #[test]
        fn failed_handoff_proves_entire_owned_tree_exited() {
            let (mut owned, _) = spawn_with_descendant();
            owned
                .terminate_tree_and_confirm_quiescent()
                .expect("whole Job quiescence");
            drop(owned);
        }

        #[test]
        fn failed_post_disarm_handoff_still_terminates_entire_tree() {
            let (mut owned, _) = spawn_with_descendant();
            owned
                .disarm_live_retaining_job()
                .expect("disarm while retaining cleanup authority");
            owned
                .terminate_tree_and_confirm_quiescent()
                .expect("post-disarm whole Job quiescence");
            drop(owned);
        }

        #[test]
        fn early_owner_return_reaps_child_and_descendant() {
            const PROCESS_TERMINATE: u32 = 0x0001;
            const SYNCHRONIZE: u32 = 0x0010_0000;
            const WAIT_OBJECT_0: u32 = 0;
            const WAIT_TIMEOUT: u32 = 0x0000_0102;
            let mut command = Command::new(std::env::current_exe().expect("test executable"));
            command
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::child_with_descendant",
                    "--nocapture",
                ])
                .env("CATDESK_OWNER_JOB_TEST_DESCENDANT", "1")
                .stdout(Stdio::piped());
            let mut owned = OwnedCandidateChild::spawn(&mut command).expect("owned child");
            let child_handle =
                unsafe { OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, 0, owned.pid()) };
            assert!(!child_handle.is_null(), "exact child process handle");
            let stdout = owned.child_mut().stdout.take().expect("child stdout");
            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let ready_reader = std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).expect("child output") == 0 {
                        return;
                    }
                    if let Some(value) = line.split("DESCENDANT_PID=").nth(1) {
                        ready_tx
                            .send(value.trim().parse::<u32>().expect("descendant PID"))
                            .expect("send descendant PID");
                        return;
                    }
                }
            });
            let descendant_pid = ready_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("bounded descendant readiness");
            let descendant_handle =
                unsafe { OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, 0, descendant_pid) };
            assert!(
                !descendant_handle.is_null(),
                "exact descendant process handle"
            );
            assert_eq!(
                unsafe { WaitForSingleObject(child_handle, 0) },
                WAIT_TIMEOUT
            );
            assert_eq!(
                unsafe { WaitForSingleObject(descendant_handle, 0) },
                WAIT_TIMEOUT
            );
            // This matches a readiness or listener error exit in the owner:
            // no explicit terminate_and_wait call is needed by the caller.
            drop(owned);
            ready_reader.join().expect("ready reader");
            let child_outcome = unsafe { WaitForSingleObject(child_handle, 5_000) };
            let descendant_outcome = unsafe { WaitForSingleObject(descendant_handle, 5_000) };
            if child_outcome != WAIT_OBJECT_0 {
                unsafe { TerminateProcess(child_handle, 1) };
            }
            if descendant_outcome != WAIT_OBJECT_0 {
                unsafe { TerminateProcess(descendant_handle, 1) };
            }
            unsafe { CloseHandle(child_handle) };
            unsafe { CloseHandle(descendant_handle) };
            assert_eq!(child_outcome, WAIT_OBJECT_0, "owner must reap exact child");
            assert_eq!(
                descendant_outcome, WAIT_OBJECT_0,
                "job close must terminate descendant"
            );
        }

        #[test]
        fn owner_process_helper() {
            if std::env::var_os("CATDESK_OWNER_JOB_TEST_PARENT").is_none() {
                return;
            }
            let mut command = Command::new(std::env::current_exe().expect("test executable"));
            command
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::sleep_child",
                ])
                .env("CATDESK_OWNER_JOB_TEST_CHILD", "1")
                .stdout(Stdio::null());
            let owned = OwnedCandidateChild::spawn(&mut command).expect("owned child");
            println!("CHILD_PID={}", owned.pid());
            std::io::stdout().flush().expect("flush child PID");
            std::thread::sleep(Duration::from_secs(60));
            drop(owned);
        }

        #[test]
        fn hard_owner_crash_closes_job_and_kills_exact_child() {
            const PROCESS_TERMINATE: u32 = 0x0001;
            const SYNCHRONIZE: u32 = 0x0010_0000;
            const WAIT_OBJECT_0: u32 = 0;
            const WAIT_TIMEOUT: u32 = 0x0000_0102;
            let mut owner = Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "reviewed_candidate_owner::windows::tests::owner_process_helper",
                    "--nocapture",
                ])
                .env("CATDESK_OWNER_JOB_TEST_PARENT", "1")
                .stdout(Stdio::piped())
                .spawn()
                .expect("owner helper");
            let mut reader = BufReader::new(owner.stdout.take().expect("helper stdout"));
            let mut line = String::new();
            let child_pid = loop {
                line.clear();
                if reader.read_line(&mut line).expect("helper output") == 0 {
                    let _ = owner.wait();
                    panic!("owner helper exited before child ownership was announced");
                }
                if let Some(value) = line.split("CHILD_PID=").nth(1) {
                    break value.trim().parse::<u32>().expect("child PID");
                }
            };
            let child_handle =
                unsafe { OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, 0, child_pid) };
            assert!(!child_handle.is_null(), "exact child process handle");
            let alive_before_crash = unsafe { WaitForSingleObject(child_handle, 0) };
            owner.kill().expect("hard kill owner");
            owner.wait().expect("reap owner");
            let outcome = unsafe { WaitForSingleObject(child_handle, 5_000) };
            if outcome != WAIT_OBJECT_0 {
                unsafe { TerminateProcess(child_handle, 1) };
            }
            unsafe { CloseHandle(child_handle) };
            assert_eq!(
                alive_before_crash, WAIT_TIMEOUT,
                "candidate must still be alive before owner crash"
            );
            assert_eq!(outcome, WAIT_OBJECT_0, "job close must kill child");
        }
    }
}

#[cfg(windows)]
pub(crate) use windows::{OwnedCandidateChild, SpawnFailure};
