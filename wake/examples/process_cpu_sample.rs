use std::ffi::c_void;
use std::thread;
use std::time::Duration;

const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

#[repr(C)]
#[derive(Clone, Copy)]
struct FileTime {
    low: u32,
    high: u32,
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
    fn CloseHandle(handle: *mut c_void) -> i32;
}

fn ticks(value: FileTime) -> u64 {
    ((value.high as u64) << 32) | value.low as u64
}

fn sample(handle: *mut c_void) -> Result<(u64, u64, u64), String> {
    unsafe {
        let mut creation = FileTime { low: 0, high: 0 };
        let mut exit = creation;
        let mut kernel = creation;
        let mut user = creation;
        if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) == 0 {
            return Err("PROCESS_TIMES_UNAVAILABLE".into());
        }
        Ok((ticks(creation), ticks(kernel), ticks(user)))
    }
}

fn main() -> Result<(), String> {
    let pid = std::env::args()
        .nth(1)
        .ok_or("PID_REQUIRED")?
        .parse::<u32>()
        .map_err(|_| "PID_INVALID")?;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return Err("PROCESS_UNAVAILABLE".into());
        }
        let first = sample(handle)?;
        thread::sleep(Duration::from_secs(2));
        let second = sample(handle)?;
        CloseHandle(handle);
        let unix_creation = first.0.saturating_sub(116_444_736_000_000_000) / 10_000_000;
        let kernel_ms = second.1.saturating_sub(first.1) / 10_000;
        let user_ms = second.2.saturating_sub(first.2) / 10_000;
        println!(
            "PID={pid} CREATED_UNIX={unix_creation} KERNEL_DELTA_MS={kernel_ms} USER_DELTA_MS={user_ms}"
        );
        Ok(())
    }
}
