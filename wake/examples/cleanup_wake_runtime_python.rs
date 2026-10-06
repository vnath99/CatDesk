#![cfg(windows)]
use catdesk_wake::runtime;
use std::ffi::c_void;
use std::path::PathBuf;

const TH32CS_SNAPPROCESS: u32 = 0x00000002;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const PROCESS_TERMINATE: u32 = 0x0001;
const MAX_PATH: usize = 260;

#[repr(C)]
struct ProcessEntry32W {
    dw_size: u32,
    cnt_usage: u32,
    process_id: u32,
    default_heap_id: usize,
    module_id: u32,
    cnt_threads: u32,
    parent_process_id: u32,
    pri_class_base: i32,
    flags: u32,
    exe_file: [u16; MAX_PATH],
}

impl Default for ProcessEntry32W {
    fn default() -> Self {
        Self {
            dw_size: std::mem::size_of::<Self>() as u32,
            cnt_usage: 0,
            process_id: 0,
            default_heap_id: 0,
            module_id: 0,
            cnt_threads: 0,
            parent_process_id: 0,
            pri_class_base: 0,
            flags: 0,
            exe_file: [0; MAX_PATH],
        }
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut c_void;
    fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
    fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
    fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> *mut c_void;
    fn QueryFullProcessImageNameW(
        process: *mut c_void,
        flags: u32,
        name: *mut u16,
        size: *mut u32,
    ) -> i32;
    fn TerminateProcess(process: *mut c_void, exit_code: u32) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

fn full_image(pid: u32) -> Option<PathBuf> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 32768];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len) != 0;
        CloseHandle(handle);
        if !ok {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(
            &buf[..len as usize],
        )))
    }
}

fn main() -> Result<(), String> {
    let expected = runtime::default_root()?.join("runtime").join("python.exe");
    let expected =
        std::fs::canonicalize(&expected).map_err(|_| "RUNTIME_PYTHON_CANONICALIZE_FAILED")?;
    let mut matched = 0u32;
    let mut terminated = 0u32;
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot as isize == -1 {
            return Err("PROCESS_SNAPSHOT_FAILED".into());
        }
        let mut entry = ProcessEntry32W::default();
        let mut ok = Process32FirstW(snapshot, &mut entry) != 0;
        while ok {
            if entry.process_id != std::process::id() {
                if let Some(path) = full_image(entry.process_id) {
                    if path
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&expected.to_string_lossy())
                    {
                        matched += 1;
                        let handle = OpenProcess(
                            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
                            0,
                            entry.process_id,
                        );
                        if !handle.is_null() {
                            if TerminateProcess(handle, 0xC0DE) != 0 {
                                let _ = WaitForSingleObject(handle, 5000);
                                terminated += 1;
                            }
                            CloseHandle(handle);
                        }
                    }
                }
            }
            ok = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    println!("{{\"matched\":{matched},\"terminated\":{terminated}}}");
    Ok(())
}
