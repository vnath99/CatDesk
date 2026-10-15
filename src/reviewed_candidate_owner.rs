//! Process ownership for the optional reviewed-candidate parent launcher.
//!
//! A suspended child is assigned to a private Windows kill-on-close job before
//! any candidate code runs. The job handle stays in this process, so an owner
//! crash closes it and Windows terminates the child process tree.

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

    pub(crate) struct OwnedCandidateChild {
        child: Child,
        // Dropping this handle kills the child even if the parent crashes.
        job: KillOnCloseJob,
    }

    impl OwnedCandidateChild {
        pub(crate) fn spawn(command: &mut Command) -> Result<Self, String> {
            let job = KillOnCloseJob::create()?;
            // Keep console Ctrl+C on the parent so its bounded stdin stop
            // controls the child's graceful journal/root retirement.
            command.creation_flags(CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP);
            let mut child = command
                .spawn()
                .map_err(|_| "reviewed candidate child spawn failed")?;
            if let Err(error) = job.assign(&child) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            if unsafe { NtResumeProcess(child.as_raw_handle().cast()) } < 0 {
                drop(job);
                let _ = child.kill();
                let _ = child.wait();
                return Err("reviewed candidate child resume failed".into());
            }
            Ok(Self { child, job })
        }

        pub(crate) fn pid(&self) -> u32 {
            self.child.id()
        }

        pub(crate) fn child_mut(&mut self) -> &mut Child {
            &mut self.child
        }

        fn wait_until(&mut self, deadline: Instant) -> Result<bool, String> {
            loop {
                match self.child.try_wait() {
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
            // Close the exact job before dropping the child process handle.
            // This also covers every early-return path in the owner launcher.
            let job_closed = if self.job.close().is_ok() {
                true
            } else {
                // If CloseHandle failed while the job handle remains valid,
                // explicitly terminate its whole process tree and retry close.
                let _ = self.job.terminate();
                self.job.close().is_ok()
            };
            if !job_closed {
                let _ = self.child.kill();
            }
            if !self.wait_until(Instant::now() + Duration::from_secs(5))? {
                // Child::kill uses the retained process handle, never a PID
                // lookup. Reap it with a second bound after the fallback.
                let _ = self.child.kill();
                if !self.wait_until(Instant::now() + Duration::from_secs(2))? {
                    return Err("reviewed candidate child termination timed out".into());
                }
            }
            if !job_closed {
                return Err("reviewed candidate child job close failed".into());
            }
            Ok(())
        }

        pub(crate) fn terminate_and_wait(mut self) -> Result<(), String> {
            self.shutdown_exact()
        }
    }

    impl Drop for OwnedCandidateChild {
        fn drop(&mut self) {
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
pub(crate) use windows::OwnedCandidateChild;
