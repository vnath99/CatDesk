//! Same Windows kill-on-close job policy used by CatDesk's existing facade.
#[cfg(windows)]
pub struct Job(*mut std::ffi::c_void);
#[cfg(windows)]
impl Job {
    pub fn assign(child: &std::process::Child) -> crate::Result<Self> {
        use std::os::windows::io::AsRawHandle;
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err("BROWSER_JOB_UNAVAILABLE".into());
        }
        let mut limits = Extended::default();
        limits.basic.flags = 0x2000;
        let valid = unsafe {
            SetInformationJobObject(
                handle,
                9,
                &mut limits as *mut _ as *mut _,
                std::mem::size_of::<Extended>() as u32,
            )
        } != 0
            && unsafe { AssignProcessToJobObject(handle, child.as_raw_handle().cast()) } != 0;
        if !valid {
            unsafe {
                CloseHandle(handle);
            }
            return Err("BROWSER_JOB_ASSIGNMENT_FAILED".into());
        }
        Ok(Self(handle))
    }

    /// Account for every process still attached to this exact owned browser
    /// Job. A helper exit alone does not prove its browser descendants exited.
    pub fn active_processes(&self) -> crate::Result<u32> {
        let mut accounting = BasicAccounting::default();
        if unsafe {
            QueryInformationJobObject(
                self.0,
                1,
                (&mut accounting as *mut BasicAccounting).cast(),
                std::mem::size_of::<BasicAccounting>() as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err("BROWSER_JOB_ACCOUNTING_UNAVAILABLE".into());
        }
        Ok(accounting.active_processes)
    }

    pub fn wait_empty(&self, wait: std::time::Duration) -> crate::Result<()> {
        let deadline = std::time::Instant::now() + wait;
        loop {
            if self.active_processes()? == 0 {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err("BROWSER_JOB_EXIT_UNPROVEN".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    pub fn terminate(&self) -> crate::Result<()> {
        if unsafe { TerminateJobObject(self.0, 1) } == 0 {
            return Err("BROWSER_JOB_TERMINATION_UNAVAILABLE".into());
        }
        Ok(())
    }

    /// Only terminate the retained private Job after the helper's normal
    /// browser cleanup has had a bounded grace period.
    pub fn terminate_and_wait_empty(&self, wait: std::time::Duration) -> crate::Result<()> {
        let terminated = self.terminate().is_ok();
        self.wait_empty(wait)?;
        if terminated || self.active_processes()? == 0 {
            Ok(())
        } else {
            Err("BROWSER_JOB_EXIT_UNPROVEN".into())
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    #[test]
    fn retained_private_job_accounts_and_terminates_exact_child() {
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$null = [Console]::In.ReadLine()",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000);
        let mut child = command.spawn().expect("isolated PowerShell child");
        let job = match Job::assign(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("private Job assignment: {error}");
            }
        };
        assert!(job.active_processes().unwrap() > 0);
        job.terminate_and_wait_empty(Duration::from_secs(10))
            .expect("exact private Job must reach zero active processes");
        child.wait().expect("owned child reaped");
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct Basic {
    process_time: i64,
    job_time: i64,
    flags: u32,
    minimum: usize,
    maximum: usize,
    active: u32,
    affinity: usize,
    priority: u32,
    scheduling: u32,
}
#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct Extended {
    basic: Basic,
    io: [u64; 6],
    process_memory: usize,
    job_memory: usize,
    peak_process: usize,
    peak_job: usize,
}
#[cfg(windows)]
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
#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(
        attributes: *const std::ffi::c_void,
        name: *const u16,
    ) -> *mut std::ffi::c_void;
    fn SetInformationJobObject(
        job: *mut std::ffi::c_void,
        class: i32,
        information: *mut std::ffi::c_void,
        length: u32,
    ) -> i32;
    fn AssignProcessToJobObject(job: *mut std::ffi::c_void, process: *mut std::ffi::c_void) -> i32;
    fn QueryInformationJobObject(
        job: *mut std::ffi::c_void,
        class: i32,
        information: *mut std::ffi::c_void,
        length: u32,
        returned: *mut u32,
    ) -> i32;
    fn TerminateJobObject(job: *mut std::ffi::c_void, exit_code: u32) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}
#[cfg(not(windows))]
pub struct Job;
#[cfg(not(windows))]
impl Job {
    pub fn assign(_: &std::process::Child) -> crate::Result<Self> {
        Err("BROWSER_PLATFORM_UNSUPPORTED".into())
    }
    pub fn active_processes(&self) -> crate::Result<u32> {
        Err("BROWSER_PLATFORM_UNSUPPORTED".into())
    }
    pub fn wait_empty(&self, _: std::time::Duration) -> crate::Result<()> {
        Err("BROWSER_PLATFORM_UNSUPPORTED".into())
    }
    pub fn terminate(&self) -> crate::Result<()> {
        Err("BROWSER_PLATFORM_UNSUPPORTED".into())
    }
    pub fn terminate_and_wait_empty(&self, _: std::time::Duration) -> crate::Result<()> {
        Err("BROWSER_PLATFORM_UNSUPPORTED".into())
    }
}
