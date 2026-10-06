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
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}
#[cfg(not(windows))]
pub struct Job;
#[cfg(not(windows))]
impl Job {
    pub fn assign(_: &std::process::Child) -> crate::Result<Self> {
        Err("BROWSER_PLATFORM_UNSUPPORTED".into())
    }
}
