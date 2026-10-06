#![allow(non_snake_case)]
#[cfg(windows)]
mod win {
    use std::{
        collections::{HashMap, VecDeque},
        ffi::c_void,
        mem::{size_of, zeroed},
    };
    const TH32CS_SNAPPROCESS: u32 = 0x00000002;
    const INVALID_HANDLE_VALUE: isize = -1isize;
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
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
        fn Process32FirstW(snapshot: *mut c_void, entry: *mut PROCESSENTRY32W) -> i32;
        fn Process32NextW(snapshot: *mut c_void, entry: *mut PROCESSENTRY32W) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    fn utf16z(v: &[u16]) -> String {
        let n = v.iter().position(|&x| x == 0).unwrap_or(v.len());
        String::from_utf16_lossy(&v[..n])
    }
    pub fn run(root: u32) {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap as isize == INVALID_HANDLE_VALUE {
                return;
            }
            let mut children: HashMap<u32, Vec<(u32, String)>> = HashMap::new();
            let mut e: PROCESSENTRY32W = zeroed();
            e.dwSize = size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut e);
            while ok != 0 {
                children
                    .entry(e.th32ParentProcessID)
                    .or_default()
                    .push((e.th32ProcessID, utf16z(&e.szExeFile)));
                ok = Process32NextW(snap, &mut e);
            }
            CloseHandle(snap);
            let mut q = VecDeque::from([(root, 0usize)]);
            while let Some((pid, depth)) = q.pop_front() {
                if let Some(v) = children.get(&pid) {
                    for (child, name) in v {
                        println!(
                            "{}pid={}|ppid={}|name={}",
                            "  ".repeat(depth),
                            child,
                            pid,
                            name
                        );
                        q.push_back((*child, depth + 1));
                    }
                }
            }
        }
    }
}
fn main() {
    #[cfg(windows)]
    win::run(51292);
}
