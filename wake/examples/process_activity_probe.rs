#![allow(non_snake_case)]
#[cfg(windows)]
mod win {
    use std::{ffi::c_void, thread, time::Duration};
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct FILETIME {
        dwLowDateTime: u32,
        dwHighDateTime: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetProcessTimes(
            h: *mut c_void,
            create: *mut FILETIME,
            exit: *mut FILETIME,
            kernel: *mut FILETIME,
            user: *mut FILETIME,
        ) -> i32;
        fn CloseHandle(h: *mut c_void) -> i32;
    }
    fn ticks(ft: FILETIME) -> u64 {
        ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
    }
    unsafe fn cpu(pid: u32) -> Option<(u64, u64)> {
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h.is_null() {
            return None;
        }
        let mut c = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut e = c;
        let mut k = c;
        let mut u = c;
        let ok = unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) };
        unsafe { CloseHandle(h) };
        if ok == 0 {
            None
        } else {
            Some((ticks(k), ticks(u)))
        }
    }
    pub fn run(pid: u32) {
        let a = unsafe { cpu(pid) };
        thread::sleep(Duration::from_secs(2));
        let b = unsafe { cpu(pid) };
        println!("PID={pid}|A={a:?}|B={b:?}");
        if let (Some((ak, au)), Some((bk, bu))) = (a, b) {
            println!("DELTA_100NS={}", (bk - ak) + (bu - au));
        }
    }
}
fn main() {
    #[cfg(windows)]
    {
        win::run(20384);
        win::run(36688);
    }
}
