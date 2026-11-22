//! Same Windows kill-on-close job policy used by CatDesk's existing facade.
#[cfg(windows)]
use std::os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle};
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
const KILL_ON_CLOSE: u32 = 0x2000;
#[cfg(windows)]
const ACTIVE_PROCESS_LIMIT: u32 = 0x0008;
#[cfg(windows)]
const MAX_DRAIN_MEMBERS: usize = 1024;

#[cfg(windows)]
pub struct Job {
    handle: *mut std::ffi::c_void,
    root: OwnedHandle,
}
#[cfg(windows)]
impl Job {
    pub fn assign(child: &std::process::Child) -> crate::Result<Self> {
        let root = child
            .as_handle()
            .try_clone_to_owned()
            .map_err(|_| "BROWSER_JOB_UNAVAILABLE")?;
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err("BROWSER_JOB_UNAVAILABLE".into());
        }
        let mut limits = Extended::default();
        limits.basic.flags = KILL_ON_CLOSE;
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
        Ok(Self { handle, root })
    }

    /// Account for every process still attached to this exact owned browser
    /// Job. A helper exit alone does not prove its browser descendants exited.
    pub fn active_processes(&self) -> crate::Result<u32> {
        let mut accounting = BasicAccounting::default();
        if unsafe {
            QueryInformationJobObject(
                self.handle,
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

    pub fn wait_empty(&self, wait: Duration) -> crate::Result<()> {
        let deadline = Instant::now() + wait;
        loop {
            if self.active_processes()? == 0 {
                return Ok(());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("BROWSER_JOB_EXIT_UNPROVEN".into());
            }
            std::thread::sleep(remaining.min(Duration::from_millis(50)));
        }
    }

    pub fn terminate(&self) -> crate::Result<()> {
        if unsafe { TerminateJobObject(self.handle, 1) } == 0 {
            return Err("BROWSER_JOB_TERMINATION_UNAVAILABLE".into());
        }
        Ok(())
    }

    /// Only terminate the retained private Job after the helper's normal
    /// browser cleanup has had a bounded grace period.
    pub fn terminate_and_wait_empty(&self, wait: Duration) -> crate::Result<()> {
        let deadline = Instant::now() + wait;
        let members = self.capture_drain_members();
        // Also request cleanup on proof failure. The caller retains ownership
        // unless both the process handles and the Job have drained.
        let _ = self.terminate();
        let members = members?;
        for member in std::iter::once(&self.root).chain(&members) {
            let timeout = deadline
                .saturating_duration_since(Instant::now())
                .as_millis();
            let timeout = timeout.min(u128::from(u32::MAX - 1)) as u32;
            if unsafe { WaitForSingleObject(member.as_raw_handle().cast(), timeout) } != 0 {
                return Err("BROWSER_JOB_EXIT_UNPROVEN".into());
            }
        }
        self.wait_empty(deadline.saturating_duration_since(Instant::now()))
    }

    fn capture_drain_members(&self) -> crate::Result<Vec<OwnedHandle>> {
        // The job is private. With the ceiling at one, a remaining member
        // cannot add a child while we collect the complete membership list.
        let mut limits = Extended::default();
        limits.basic.flags = KILL_ON_CLOSE | ACTIVE_PROCESS_LIMIT;
        limits.basic.active = 1;
        if unsafe {
            SetInformationJobObject(
                self.handle,
                9,
                (&mut limits as *mut Extended).cast(),
                std::mem::size_of::<Extended>() as u32,
            )
        } == 0
        {
            return Err("BROWSER_JOB_EXIT_UNPROVEN".into());
        }
        let mut list = BasicProcessList {
            assigned: 0,
            count: 0,
            ids: [0; MAX_DRAIN_MEMBERS],
        };
        if unsafe {
            QueryInformationJobObject(
                self.handle,
                3,
                (&mut list as *mut BasicProcessList).cast(),
                std::mem::size_of::<BasicProcessList>() as u32,
                std::ptr::null_mut(),
            )
        } == 0
            || list.count != list.assigned
            || list.count as usize > list.ids.len()
        {
            return Err("BROWSER_JOB_EXIT_UNPROVEN".into());
        }
        let mut handles = Vec::with_capacity(list.count as usize);
        for &id in &list.ids[..list.count as usize] {
            let pid = u32::try_from(id).map_err(|_| "BROWSER_JOB_EXIT_UNPROVEN")?;
            if pid == 0 {
                return Err("BROWSER_JOB_EXIT_UNPROVEN".into());
            }
            const SYNCHRONIZE: u32 = 0x0010_0000;
            const QUERY_LIMITED_INFORMATION: u32 = 0x1000;
            let raw = unsafe { OpenProcess(SYNCHRONIZE | QUERY_LIMITED_INFORMATION, 0, pid) };
            if raw.is_null() {
                return Err("BROWSER_JOB_EXIT_UNPROVEN".into());
            }
            let process = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
            let mut member = 0;
            if (unsafe { IsProcessInJob(raw, self.handle, &mut member) } == 0 || member != 1)
                && unsafe { WaitForSingleObject(raw, 0) } != 0
            {
                return Err("BROWSER_JOB_EXIT_UNPROVEN".into());
            }
            handles.push(process);
        }
        Ok(handles)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    #[test]
    fn retained_private_job_accounts_and_terminates_exact_child() {
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "if ([Console]::In.ReadLine() -cne 'start') { exit 8 }",
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

    #[test]
    fn drain_snapshot_prevents_a_new_descendant_from_starting() {
        let script = r#"
            $ErrorActionPreference = 'Stop'
            if ([Console]::In.ReadLine() -cne 'start') { exit 8 }
            $start = [Diagnostics.ProcessStartInfo]::new()
            $start.FileName = 'powershell.exe'
            $start.Arguments = '-NoProfile -NonInteractive -Command "exit 0"'
            $start.UseShellExecute = $false
            $start.CreateNoWindow = $true
            try { [Diagnostics.Process]::Start($start) | Out-Null } catch { exit 0 }
            exit 9
        "#;
        let mut helper = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
            .expect("private Job fixture helper");
        let job = Job::assign(&helper).expect("contained fixture helper");
        let members = job.capture_drain_members().expect("drain membership proof");
        assert_eq!(members.len(), 1);
        helper.stdin.take().unwrap().write_all(b"start\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let exit = loop {
            if let Some(exit) = helper.try_wait().unwrap() {
                break Some(exit);
            }
            if Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        job.terminate_and_wait_empty(Duration::from_secs(5))
            .unwrap();
        assert!(exit.expect("fixture helper must finish").success());
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
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
#[repr(C)]
struct BasicProcessList {
    assigned: u32,
    count: u32,
    ids: [usize; MAX_DRAIN_MEMBERS],
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
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
    fn IsProcessInJob(
        process: *mut std::ffi::c_void,
        job: *mut std::ffi::c_void,
        result: *mut i32,
    ) -> i32;
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
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
