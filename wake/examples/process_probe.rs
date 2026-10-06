#[cfg(windows)]
mod win {
    use std::{
        ffi::c_void,
        mem::{size_of, zeroed},
    };
    const TH32CS_SNAPPROCESS: u32 = 0x00000002;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
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
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn QueryFullProcessImageNameW(
            process: *mut c_void,
            flags: u32,
            buffer: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    fn utf16z(v: &[u16]) -> String {
        let n = v.iter().position(|&x| x == 0).unwrap_or(v.len());
        String::from_utf16_lossy(&v[..n])
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
                let name = utf16z(&e.szExeFile).to_ascii_lowercase();
                if [
                    "cargo.exe",
                    "rustc.exe",
                    "link.exe",
                    "catdesk.exe",
                    "catdesk-control-plane-supervisor.exe",
                ]
                .contains(&name.as_str())
                {
                    let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, e.th32ProcessID);
                    let mut path = String::new();
                    if !h.is_null() {
                        let mut buf = [0u16; 32768];
                        let mut n = buf.len() as u32;
                        if QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut n) != 0 {
                            path = String::from_utf16_lossy(&buf[..n as usize]);
                        }
                        CloseHandle(h);
                    }
                    println!(
                        "pid={}|ppid={}|name={}|path={}",
                        e.th32ProcessID, e.th32ParentProcessID, name, path
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
