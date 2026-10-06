use std::{collections::HashSet, ffi::c_void};

const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const PROCESS_TERMINATE: u32 = 1;
const TH32CS_SNAPPROCESS: u32 = 2;
const MAX_PATH: usize = 260;

#[repr(C)]
#[derive(Clone, Copy)]
struct FileTime {
    low: u32,
    high: u32,
}
#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}
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

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn GetProcessTimes(
        process: *mut c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn TerminateProcess(process: *mut c_void, code: u32) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, ms: u32) -> u32;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
    fn Process32FirstW(s: *mut c_void, e: *mut ProcessEntry32W) -> i32;
    fn Process32NextW(s: *mut c_void, e: *mut ProcessEntry32W) -> i32;
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
fn ticks(v: FileTime) -> u64 {
    ((v.high as u64) << 32) | v.low as u64
}
fn creation_unix(h: *mut c_void) -> Result<u64, String> {
    unsafe {
        let mut c = FileTime { low: 0, high: 0 };
        let mut e = c;
        let mut k = c;
        let mut u = c;
        if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) == 0 {
            return Err("PROCESS_TIMES_UNAVAILABLE".into());
        }
        Ok(ticks(c).saturating_sub(116_444_736_000_000_000) / 10_000_000)
    }
}
fn command_line(h: *mut c_void) -> Result<String, String> {
    unsafe {
        let mut n = 0u32;
        let _ = NtQueryInformationProcess(h, 60, std::ptr::null_mut(), 0, &mut n);
        if n < 16 || n > 1024 * 1024 {
            return Err("COMMAND_LINE_UNAVAILABLE".into());
        }
        let mut b = vec![0u8; n as usize + 2];
        if NtQueryInformationProcess(h, 60, b.as_mut_ptr().cast(), n, &mut n) < 0 {
            return Err("COMMAND_LINE_UNAVAILABLE".into());
        }
        let us = &*(b.as_ptr() as *const UnicodeString);
        if us.buffer.is_null() {
            return Err("COMMAND_LINE_UNAVAILABLE".into());
        }
        Ok(String::from_utf16_lossy(std::slice::from_raw_parts(
            us.buffer,
            (us.length / 2) as usize,
        )))
    }
}
fn has_children(pid: u32) -> Result<bool, String> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap as isize == -1 {
            return Err("SNAPSHOT_FAILED".into());
        }
        let mut e = ProcessEntry32W::default();
        let mut ok = Process32FirstW(snap, &mut e) != 0;
        let mut children = HashSet::new();
        while ok {
            if e.parent == pid {
                children.insert(e.pid);
            }
            ok = Process32NextW(snap, &mut e) != 0
        }
        CloseHandle(snap);
        Ok(!children.is_empty())
    }
}
fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let pid = args
        .next()
        .ok_or("PID_REQUIRED")?
        .parse::<u32>()
        .map_err(|_| "PID_INVALID")?;
    let expected = args
        .next()
        .ok_or("CREATED_UNIX_REQUIRED")?
        .parse::<u64>()
        .map_err(|_| "CREATED_UNIX_INVALID")?;
    unsafe {
        let h = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            0,
            pid,
        );
        if h.is_null() {
            println!("ALREADY_GONE");
            return Ok(());
        }
        let created = creation_unix(h)?;
        if created != expected {
            CloseHandle(h);
            return Err("PROCESS_CREATION_MISMATCH".into());
        }
        let cmd = command_line(h)?.replace('/', "\\").to_ascii_lowercase();
        if !cmd.contains("powershell.exe") || !cmd.ends_with(r#"-command .\wake\install.ps1"#) {
            CloseHandle(h);
            return Err("PROCESS_COMMAND_MISMATCH".into());
        }
        if has_children(pid)? {
            CloseHandle(h);
            return Err("PROCESS_HAS_CHILDREN".into());
        }
        if TerminateProcess(h, 0xC0DE) == 0 {
            CloseHandle(h);
            return Err("TERMINATE_FAILED".into());
        }
        let _ = WaitForSingleObject(h, 5000);
        CloseHandle(h);
        println!("TERMINATED_EXACT_STALE_WAKE_INSTALLER|pid={pid}|created={created}");
        Ok(())
    }
}
