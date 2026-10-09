use std::ffi::c_void;

const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const PROCESS_TERMINATE: u32 = 1;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}

#[link(name = "kernel32")]
unsafe extern "system" {
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

fn command_line(h: *mut c_void) -> Option<String> {
    unsafe {
        let mut needed = 0u32;
        let _ = NtQueryInformationProcess(h, 60, std::ptr::null_mut(), 0, &mut needed);
        if !(16..=1024 * 1024).contains(&needed) {
            return None;
        }
        let mut buffer = vec![0u8; needed as usize + 2];
        if NtQueryInformationProcess(h, 60, buffer.as_mut_ptr().cast(), needed, &mut needed) < 0 {
            return None;
        }
        let value = &*(buffer.as_ptr() as *const UnicodeString);
        if value.buffer.is_null() {
            return None;
        }
        Some(String::from_utf16_lossy(std::slice::from_raw_parts(
            value.buffer,
            (value.length / 2) as usize,
        )))
    }
}

fn exact_diagnostic_identity(command: &str) -> bool {
    let normalized = command
        .trim_matches('"')
        .replace('\\', "/")
        .to_ascii_lowercase();
    normalized.ends_with("wake/target/debug/examples/profile_auth_diagnostic.exe")
        || normalized.ends_with("wake/target/release/examples/profile_auth_diagnostic.exe")
}

fn main() -> Result<(), String> {
    let pid = std::env::args()
        .nth(1)
        .ok_or("PID_REQUIRED")?
        .parse::<u32>()
        .map_err(|_| "PID_INVALID")?;
    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            0,
            pid,
        );
        if handle.is_null() {
            println!("ALREADY_GONE");
            return Ok(());
        }
        let observed = command_line(handle).unwrap_or_default();
        if !exact_diagnostic_identity(&observed) {
            CloseHandle(handle);
            return Err("PROCESS_IDENTITY_MISMATCH".into());
        }
        if TerminateProcess(handle, 0xC0DE) == 0 {
            CloseHandle(handle);
            return Err("TERMINATE_FAILED".into());
        }
        let _ = WaitForSingleObject(handle, 5000);
        CloseHandle(handle);
        println!("TERMINATED_EXACT_STALE_DIAGNOSTIC");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::exact_diagnostic_identity;

    #[test]
    fn cleanup_accepts_only_exact_profile_auth_diagnostic_paths() {
        assert!(exact_diagnostic_identity(
            r#"wake\target\release\examples\profile_auth_diagnostic.exe"#
        ));
        assert!(exact_diagnostic_identity(
            r#"C:\repo\wake\target\debug\examples\profile_auth_diagnostic.exe"#
        ));
        assert!(!exact_diagnostic_identity(
            r#"C:\repo\wake\target\release\examples\CatDeskWakeHost.exe"#
        ));
        assert!(!exact_diagnostic_identity(
            r#"C:\repo\wake\target\release\examples\profile_auth_diagnostic.exe --other"#
        ));
    }
}
