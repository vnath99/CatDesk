#![allow(non_snake_case)]
#[cfg(windows)]
mod win {
    use std::{
        ffi::c_void,
        mem::{size_of, zeroed},
    };
    const TH32CS_SNAPPROCESS: u32 = 0x00000002;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const INVALID_HANDLE_VALUE: isize = -1isize;
    const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;
    #[repr(C)]
    struct PROCESSENTRY32W {
        dwSize: u32,
        cntUsage: u32,
        th32ProcessID: u32,
        th32DefaultHeapID: usize,
        th32ModuleID: u32,
        cntThreads: u32,
        th32ParentProcessID: u32,
        pcPriClassBase: i32,
        dwFlags: u32,
        szExeFile: [u16; 260],
    }
    #[repr(C)]
    struct UNICODE_STRING {
        Length: u16,
        MaximumLength: u16,
        Buffer: *const u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
        fn Process32FirstW(snapshot: *mut c_void, entry: *mut PROCESSENTRY32W) -> i32;
        fn Process32NextW(snapshot: *mut c_void, entry: *mut PROCESSENTRY32W) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
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
    fn utf16z(v: &[u16]) -> String {
        let n = v.iter().position(|&x| x == 0).unwrap_or(v.len());
        String::from_utf16_lossy(&v[..n])
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
        let u = &*(buf.as_ptr() as *const UNICODE_STRING);
        let s = if u.Buffer.is_null() {
            None
        } else {
            Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                u.Buffer,
                (u.Length / 2) as usize,
            )))
        };
        unsafe { CloseHandle(h) };
        s
    }
    fn selected(cmd: &str) -> Vec<String> {
        cmd.split_whitespace()
            .filter_map(|part| {
                let l = part.to_ascii_lowercase();
                if l.contains("catdeskwake")
                    || l.contains("adapter.py")
                    || l.starts_with("--user-data-dir")
                    || l.starts_with("--profile-directory")
                    || l.starts_with("--remote-debugging-port")
                {
                    Some(part.trim_matches('"').to_string())
                } else {
                    None
                }
            })
            .collect()
    }
    pub fn run() {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap as isize == INVALID_HANDLE_VALUE {
                return;
            }
            let mut e: PROCESSENTRY32W = zeroed();
            e.dwSize = size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut e);
            while ok != 0 {
                let name = utf16z(&e.szExeFile);
                let lower = name.to_ascii_lowercase();
                if lower == "chrome.exe" || lower == "python.exe" || lower == "catdeskwakehost.exe"
                {
                    let c = cmdline(e.th32ProcessID).unwrap_or_default();
                    println!(
                        "pid={}|ppid={}|name={}|selected={}",
                        e.th32ProcessID,
                        e.th32ParentProcessID,
                        name,
                        selected(&c).join(" ")
                    );
                }
                ok = Process32NextW(snap, &mut e);
            }
            CloseHandle(snap);
        }
    }
}
fn main() {
    #[cfg(windows)]
    win::run();
}
