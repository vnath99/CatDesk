#[cfg(windows)]
mod win {
    use std::{
        collections::HashMap,
        ffi::c_void,
        mem::{size_of, zeroed},
    };
    const TH32CS_SNAPPROCESS: u32 = 0x00000002;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const INVALID_HANDLE_VALUE: isize = -1isize;
    #[repr(C)]
    struct PROCESSENTRY32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
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
            let mut map: HashMap<u32, (u32, String)> = HashMap::new();
            let mut e: PROCESSENTRY32W = zeroed();
            e.dw_size = size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut e);
            while ok != 0 {
                map.insert(
                    e.th32_process_id,
                    (e.th32_parent_process_id, utf16z(&e.sz_exe_file)),
                );
                ok = Process32NextW(snap, &mut e);
            }
            CloseHandle(snap);
            for start in [40512u32, 40396, 19836, 37408, 11668] {
                print!("chain {start}:");
                let mut pid = start;
                for _ in 0..8 {
                    let Some((ppid, name)) = map.get(&pid) else {
                        break;
                    };
                    let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                    let mut path = String::new();
                    if !h.is_null() {
                        let mut buf = [0u16; 32768];
                        let mut n = buf.len() as u32;
                        if QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut n) != 0 {
                            path = String::from_utf16_lossy(&buf[..n as usize]);
                        }
                        CloseHandle(h);
                    }
                    print!(" [pid={pid} ppid={ppid} name={name} path={path}]");
                    if *ppid == 0 || *ppid == pid {
                        break;
                    }
                    pid = *ppid;
                }
                println!();
            }
        }
    }
}
fn main() {
    #[cfg(windows)]
    win::run();
}
