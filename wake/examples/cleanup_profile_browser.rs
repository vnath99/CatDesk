#![cfg(windows)]
use catdesk_wake::runtime;
use std::ffi::c_void;

const TH32CS_SNAPPROCESS: u32 = 2;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const PROCESS_TERMINATE: u32 = 1;
const MAX_PATH: usize = 260;

#[repr(C)]
struct ProcessEntry32W {
    dw_size: u32,
    cnt_usage: u32,
    pid: u32,
    heap: usize,
    module: u32,
    threads: u32,
    parent: u32,
    pri: i32,
    flags: u32,
    exe: [u16; MAX_PATH],
}
impl Default for ProcessEntry32W {
    fn default() -> Self {
        Self {
            dw_size: std::mem::size_of::<Self>() as u32,
            cnt_usage: 0,
            pid: 0,
            heap: 0,
            module: 0,
            threads: 0,
            parent: 0,
            pri: 0,
            flags: 0,
            exe: [0; MAX_PATH],
        }
    }
}
#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
    fn Process32FirstW(s: *mut c_void, e: *mut ProcessEntry32W) -> i32;
    fn Process32NextW(s: *mut c_void, e: *mut ProcessEntry32W) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn TerminateProcess(h: *mut c_void, code: u32) -> i32;
    fn WaitForSingleObject(h: *mut c_void, ms: u32) -> u32;
    fn CloseHandle(h: *mut c_void) -> i32;
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
fn exe_name(entry: &ProcessEntry32W) -> String {
    let end = entry.exe.iter().position(|x| *x == 0).unwrap_or(MAX_PATH);
    String::from_utf16_lossy(&entry.exe[..end])
}
fn command_line(h: *mut c_void) -> Option<String> {
    unsafe {
        let mut needed = 0u32;
        let _ = NtQueryInformationProcess(h, 60, std::ptr::null_mut(), 0, &mut needed);
        if !(16..=1024 * 1024).contains(&needed) {
            return None;
        }
        let mut buf = vec![0u8; needed as usize + 2];
        if NtQueryInformationProcess(h, 60, buf.as_mut_ptr().cast(), needed, &mut needed) < 0 {
            return None;
        }
        let us = &*(buf.as_ptr() as *const UnicodeString);
        if us.buffer.is_null() {
            return None;
        }
        Some(String::from_utf16_lossy(std::slice::from_raw_parts(
            us.buffer,
            (us.length / 2) as usize,
        )))
    }
}
fn normalized(s: &str) -> String {
    s.replace("\\\\?\\", "").to_ascii_lowercase()
}
fn main() -> Result<(), String> {
    let profile = runtime::default_root()?.join("browser-profile");
    let needle = normalized(&profile.to_string_lossy());
    let mut matched = 0;
    let mut terminated = 0;
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap as isize == -1 {
            return Err("SNAPSHOT_FAILED".into());
        }
        let mut e = ProcessEntry32W::default();
        let mut ok = Process32FirstW(snap, &mut e) != 0;
        while ok {
            let name = exe_name(&e).to_ascii_lowercase();
            if matches!(
                name.as_str(),
                "chrome.exe" | "msedge.exe" | "chromium.exe" | "uc_driver.exe" | "chromedriver.exe"
            ) {
                let h = OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
                    0,
                    e.pid,
                );
                if !h.is_null() {
                    if let Some(cmd) = command_line(h)
                        && normalized(&cmd).contains(&needle)
                    {
                        matched += 1;
                        if TerminateProcess(h, 0xC0DE) != 0 {
                            let _ = WaitForSingleObject(h, 5000);
                            terminated += 1;
                        }
                    }
                    CloseHandle(h);
                }
            }
            ok = Process32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
    }
    println!("{{\"matched\":{matched},\"terminated\":{terminated}}}");
    Ok(())
}
