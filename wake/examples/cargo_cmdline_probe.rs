#![allow(non_snake_case)]
#[cfg(windows)]
mod win {
    use std::{
        ffi::c_void,
        mem::{size_of, zeroed},
    };
    const TH32CS_SNAPPROCESS: u32 = 0x00000002;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const PROCESS_VM_READ: u32 = 0x0010;
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
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut need = 0u32;
        let _ = NtQueryInformationProcess(
            h,
            PROCESS_COMMAND_LINE_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut need,
        );
        if need == 0 {
            CloseHandle(h);
            return None;
        }
        let mut buf = vec![0u8; need as usize + 16];
        let st = NtQueryInformationProcess(
            h,
            PROCESS_COMMAND_LINE_INFORMATION,
            buf.as_mut_ptr() as *mut _,
            buf.len() as u32,
            &mut need,
        );
        if st < 0 {
            CloseHandle(h);
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
        CloseHandle(h);
        s
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
                if name.eq_ignore_ascii_case("cargo.exe")
                    || name.eq_ignore_ascii_case("rustc.exe")
                    || name.eq_ignore_ascii_case("link.exe")
                {
                    println!(
                        "pid={}|ppid={}|name={}|cmd={}",
                        e.th32ProcessID,
                        e.th32ParentProcessID,
                        name,
                        cmdline(e.th32ProcessID).unwrap_or_default()
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
