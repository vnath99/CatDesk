#![allow(non_snake_case)]
#[cfg(windows)]
mod win {
    use std::{
        ffi::c_void,
        mem::{size_of, zeroed},
    };
    const PROCESS_TERMINATE: u32 = 0x0001;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;
    #[repr(C)]
    struct UNICODE_STRING {
        Length: u16,
        MaximumLength: u16,
        Buffer: *const u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn TerminateProcess(handle: *mut c_void, code: u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtQueryInformationProcess(
            h: *mut c_void,
            class: u32,
            info: *mut c_void,
            len: u32,
            ret: *mut u32,
        ) -> i32;
    }
    unsafe fn cmdline(pid: u32) -> Option<String> {
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h.is_null() {
            return None;
        }
        let mut need = 0u32;
        let _ = unsafe {
            NtQueryInformationProcess(
                h,
                PROCESS_COMMAND_LINE_INFORMATION,
                std::ptr::null_mut(),
                0,
                &mut need,
            )
        };
        if need == 0 {
            unsafe { CloseHandle(h) };
            return None;
        }
        let mut buf = vec![0u8; need as usize + 16];
        let st = unsafe {
            NtQueryInformationProcess(
                h,
                PROCESS_COMMAND_LINE_INFORMATION,
                buf.as_mut_ptr() as *mut _,
                buf.len() as u32,
                &mut need,
            )
        };
        if st < 0 {
            unsafe { CloseHandle(h) };
            return None;
        }
        let u = unsafe { &*(buf.as_ptr() as *const UNICODE_STRING) };
        let s = if u.Buffer.is_null() {
            None
        } else {
            Some(String::from_utf16_lossy(unsafe {
                std::slice::from_raw_parts(u.Buffer, (u.Length / 2) as usize)
            }))
        };
        unsafe { CloseHandle(h) };
        s
    }
    pub fn terminate_exact(pid: u32) -> Result<(), String> {
        let cmd = unsafe { cmdline(pid) }.ok_or("PROCESS_UNAVAILABLE")?;
        if !cmd.contains("cargo.exe")
            || !cmd.contains("build --release --locked --target-dir target-verify/v4-daemon")
        {
            return Err(format!("PID_{pid}_COMMAND_MISMATCH"));
        }
        let h = unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        if h.is_null() {
            return Err("PROCESS_UNAVAILABLE".into());
        }
        let ok = unsafe { TerminateProcess(h, 0x4B1D) };
        unsafe { CloseHandle(h) };
        if ok == 0 {
            return Err("TERMINATE_FAILED".into());
        }
        println!("TERMINATED={pid}");
        Ok(())
    }
}
fn main() -> Result<(), String> {
    #[cfg(windows)]
    {
        win::terminate_exact(51348)?;
        win::terminate_exact(51680)?;
    }
    Ok(())
}
