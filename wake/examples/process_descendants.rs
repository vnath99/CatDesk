use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
};

const TH32CS_SNAPPROCESS: u32 = 2;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
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
fn name(e: &ProcessEntry32W) -> String {
    let end = e.exe.iter().position(|v| *v == 0).unwrap_or(MAX_PATH);
    String::from_utf16_lossy(&e.exe[..end])
}
fn cmd(pid: u32) -> String {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return "<unavailable>".into();
        }
        let mut n = 0u32;
        let _ = NtQueryInformationProcess(h, 60, std::ptr::null_mut(), 0, &mut n);
        if !(16..=1024 * 1024).contains(&n) {
            CloseHandle(h);
            return "<unavailable>".into();
        }
        let mut b = vec![0u8; n as usize + 2];
        if NtQueryInformationProcess(h, 60, b.as_mut_ptr().cast(), n, &mut n) < 0 {
            CloseHandle(h);
            return "<unavailable>".into();
        }
        let u = &*(b.as_ptr() as *const UnicodeString);
        let out = if u.buffer.is_null() {
            "<unavailable>".into()
        } else {
            String::from_utf16_lossy(std::slice::from_raw_parts(
                u.buffer,
                (u.length / 2) as usize,
            ))
        };
        CloseHandle(h);
        out.chars().take(1200).collect()
    }
}
fn main() -> Result<(), String> {
    let root = std::env::args()
        .nth(1)
        .ok_or("PID_REQUIRED")?
        .parse::<u32>()
        .map_err(|_| "PID_INVALID")?;
    let mut rows = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap as isize == -1 {
            return Err("SNAPSHOT_FAILED".into());
        }
        let mut e = ProcessEntry32W::default();
        let mut ok = Process32FirstW(snap, &mut e) != 0;
        while ok {
            rows.push((e.pid, e.parent, name(&e)));
            ok = Process32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
    }
    let mut by_parent: HashMap<u32, Vec<(u32, String)>> = HashMap::new();
    for (pid, ppid, n) in rows {
        by_parent.entry(ppid).or_default().push((pid, n));
    }
    let mut frontier = vec![root];
    let mut seen = HashSet::new();
    println!("ROOT|pid={root}|cmd={}", cmd(root));
    while let Some(parent) = frontier.pop() {
        if !seen.insert(parent) {
            continue;
        }
        if let Some(children) = by_parent.get(&parent) {
            for (pid, n) in children {
                println!(
                    "CHILD|ppid={}|pid={}|name={}|cmd={}",
                    parent,
                    pid,
                    n,
                    cmd(*pid)
                );
                frontier.push(*pid);
            }
        }
    }
    Ok(())
}
