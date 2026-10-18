//! Windows object identity for a reviewed Wake installation handoff.
//!
//! A displayed path is insufficient here: the installed WakeHost and an
//! installer launched in another package context can resolve the same path to
//! different directories. The running host pins its root directory. An
//! installer must find both that directory and the singleton host lock among
//! the *other process's* open handles, then pin the corresponding local objects
//! against deletion for the entire mutation.

use crate::{Result, store::HostLease};
use std::path::Path;

#[cfg(windows)]
mod windows {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{
        ffi::c_void,
        fs::File,
        io::Read,
        marker::PhantomData,
        os::windows::{
            ffi::{OsStrExt, OsStringExt},
            io::FromRawHandle,
        },
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread::{self, JoinHandle},
        time::{Duration, Instant},
    };

    const INVALID_HANDLE: *mut c_void = -1_isize as *mut c_void;
    const OPEN_EXISTING: u32 = 3;
    const SHARE_READ_WRITE: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_READ_WRITE: u32 = 0xc000_0000;
    const PROCESS_DUP_HANDLE: u32 = 0x40;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const DUPLICATE_SAME_ACCESS: u32 = 2;
    const FILE_TYPE_DISK: u32 = 1;
    const WAIT_TIMEOUT: u32 = 0x102;
    const SYSTEM_EXTENDED_HANDLE_INFORMATION: u32 = 64;
    const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xc000_0004_u32 as i32;
    const MAX_HANDLE_TABLE_BYTES: usize = 128 * 1024 * 1024;
    const OWNER_PROOF_REQUEST: &[u8; 8] = b"CDWAUTH1";
    const OWNER_PROOF_RESPONSE: &[u8; 8] = b"CDWPROOF";
    const OWNER_PROOF_REQUEST_BYTES: usize = 40;
    const OWNER_PROOF_RESPONSE_BYTES: usize = 64;
    const OWNER_PROOF_WAIT: Duration = Duration::from_secs(2);
    const PIPE_ACCESS_DUPLEX: u32 = 3;
    const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
    const PIPE_NOWAIT: u32 = 1;
    const PIPE_REJECT_REMOTE_CLIENTS: u32 = 8;
    const ERROR_PIPE_CONNECTED: u32 = 535;
    const ERROR_PIPE_LISTENING: u32 = 536;
    const ERROR_NO_DATA: u32 = 232;
    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 2;

    #[repr(C)]
    #[derive(Clone, Copy, Default, Eq, PartialEq)]
    struct FileInfo {
        attributes: u32,
        creation_low: u32,
        creation_high: u32,
        access_low: u32,
        access_high: u32,
        write_low: u32,
        write_high: u32,
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct ObjectId {
        volume: u32,
        index_high: u32,
        index_low: u32,
    }

    impl From<FileInfo> for ObjectId {
        fn from(info: FileInfo) -> Self {
            Self {
                volume: info.volume,
                index_high: info.index_high,
                index_low: info.index_low,
            }
        }
    }

    unsafe extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            disposition: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
        fn GetFileInformationByHandle(handle: *mut c_void, info: *mut FileInfo) -> i32;
        fn GetFileType(handle: *mut c_void) -> u32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetCurrentProcess() -> *mut c_void;
        fn GetCurrentProcessId() -> u32;
        fn QueryFullProcessImageNameW(
            process: *mut c_void,
            flags: u32,
            image: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn DuplicateHandle(
            source: *mut c_void,
            handle: *mut c_void,
            target: *mut c_void,
            copy: *mut *mut c_void,
            access: u32,
            inherit: i32,
            options: u32,
        ) -> i32;
        fn WaitForSingleObject(handle: *mut c_void, millis: u32) -> u32;
        fn NtQuerySystemInformation(
            kind: u32,
            buffer: *mut c_void,
            length: u32,
            needed: *mut u32,
        ) -> i32;
        fn CreateNamedPipeW(
            name: *const u16,
            open_mode: u32,
            pipe_mode: u32,
            max_instances: u32,
            out_buffer: u32,
            in_buffer: u32,
            default_timeout: u32,
            security: *mut c_void,
        ) -> *mut c_void;
        fn ConnectNamedPipe(pipe: *mut c_void, overlapped: *mut c_void) -> i32;
        fn DisconnectNamedPipe(pipe: *mut c_void) -> i32;
        fn GetNamedPipeServerProcessId(pipe: *mut c_void, pid: *mut u32) -> i32;
        fn PeekNamedPipe(
            pipe: *mut c_void,
            buffer: *mut c_void,
            size: u32,
            read: *mut u32,
            available: *mut u32,
            left: *mut u32,
        ) -> i32;
        fn ReadFile(
            file: *mut c_void,
            buffer: *mut c_void,
            size: u32,
            read: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn WriteFile(
            file: *mut c_void,
            buffer: *const c_void,
            size: u32,
            written: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn GetLastError() -> u32;
    }
    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(algorithm: *mut c_void, buffer: *mut u8, size: u32, flags: u32) -> i32;
    }

    fn open_pinned(path: &Path, directory: bool) -> Result<File> {
        open_pinned_with_access(
            path,
            directory,
            if directory { 0 } else { GENERIC_READ_WRITE },
        )
    }

    fn open_pinned_with_access(path: &Path, directory: bool, access: u32) -> Result<File> {
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        let flags = FILE_FLAG_OPEN_REPARSE_POINT
            | if directory {
                FILE_FLAG_BACKUP_SEMANTICS
            } else {
                0
            };
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                access,
                SHARE_READ_WRITE, // no FILE_SHARE_DELETE: pathname cannot be retargeted
                std::ptr::null_mut(),
                OPEN_EXISTING,
                flags,
                std::ptr::null_mut(),
            )
        };
        if handle.is_null() || handle == INVALID_HANDLE {
            return Err("WAKE_STORE_AUTHORITY_OBJECT_UNAVAILABLE".into());
        }
        Ok(unsafe { File::from_raw_handle(handle) })
    }

    fn info(file: &File) -> Result<FileInfo> {
        use std::os::windows::io::AsRawHandle;
        let mut info = FileInfo::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err("WAKE_STORE_AUTHORITY_OBJECT_INFO_UNAVAILABLE".into());
        }
        Ok(info)
    }

    fn validate_info(info: FileInfo, directory: bool) -> Result<ObjectId> {
        if info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || (info.attributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
            || (!directory && info.links != 1)
        {
            return Err("WAKE_STORE_AUTHORITY_OBJECT_INVALID".into());
        }
        Ok(info.into())
    }

    fn fingerprint(root: ObjectId, lock: ObjectId) -> String {
        let payload = format!(
            "catdesk-wake-install-store-v1\0{:08x}:{:08x}:{:08x}/{:08x}:{:08x}:{:08x}",
            root.volume,
            root.index_high,
            root.index_low,
            lock.volume,
            lock.index_high,
            lock.index_low
        );
        format!("{:x}", Sha256::digest(payload.as_bytes()))
    }

    fn pipe_name(pid: u32, root: ObjectId, lock: ObjectId) -> String {
        format!(
            r"\\.\pipe\CatDeskWakeAuthorityV1-{pid}-{}",
            &fingerprint(root, lock)[..24]
        )
    }

    fn wire_identity(root: ObjectId, lock: ObjectId) -> [u8; 24] {
        let mut bytes = [0u8; 24];
        for (index, value) in [
            root.volume,
            root.index_high,
            root.index_low,
            lock.volume,
            lock.index_high,
            lock.index_low,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// A host may offer this endpoint only while the HostLease acquired by
    /// Store::host_lock is alive. The kernel identifies the pipe server PID;
    /// the nonce prevents reuse of a prior response. Opening host.lock alone
    /// cannot answer the challenge from the reviewed WakeHost process.
    pub struct HostOwnerProofServer<'a> {
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
        _authority: PhantomData<(&'a HostRootPin, &'a HostLease)>,
    }

    impl<'a> HostOwnerProofServer<'a> {
        pub fn start(root: &Path, pin: &'a HostRootPin, lease: &'a HostLease) -> Result<Self> {
            let root_id = validate_info(info(&pin._root)?, true)?;
            let lock_id = validate_info(info(lease.lock_file())?, false)?;
            let path_root = open_pinned(root, true)?;
            let path_lock = open_pinned(&root.join("host.lock"), false)?;
            if validate_info(info(&path_root)?, true)? != root_id
                || validate_info(info(&path_lock)?, false)? != lock_id
            {
                return Err("WAKE_STORE_AUTHORITY_HOST_OBJECT_CHANGED".into());
            }
            let name = pipe_name(unsafe { GetCurrentProcessId() }, root_id, lock_id);
            let wide = std::ffi::OsStr::new(&name)
                .encode_wide()
                .chain([0])
                .collect::<Vec<_>>();
            let raw = unsafe {
                CreateNamedPipeW(
                    wide.as_ptr(),
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    128,
                    128,
                    0,
                    std::ptr::null_mut(),
                )
            };
            if raw.is_null() || raw == INVALID_HANDLE {
                return Err("WAKE_STORE_AUTHORITY_HOST_PROOF_UNAVAILABLE".into());
            }
            let pipe = unsafe { File::from_raw_handle(raw) };
            let stop = Arc::new(AtomicBool::new(false));
            let thread_stop = Arc::clone(&stop);
            let thread =
                thread::spawn(move || serve_owner_proof(pipe, root_id, lock_id, &thread_stop));
            Ok(Self {
                stop,
                thread: Some(thread),
                _authority: PhantomData,
            })
        }
    }

    impl Drop for HostOwnerProofServer<'_> {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn serve_owner_proof(pipe: File, root: ObjectId, lock: ObjectId, stop: &AtomicBool) {
        use std::os::windows::io::AsRawHandle;
        let raw = pipe.as_raw_handle();
        while !stop.load(Ordering::Acquire) {
            let connected = unsafe { ConnectNamedPipe(raw, std::ptr::null_mut()) } != 0
                || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
            if !connected {
                let error = unsafe { GetLastError() };
                if error != ERROR_PIPE_LISTENING && error != ERROR_NO_DATA {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            let mut request = [0u8; OWNER_PROOF_REQUEST_BYTES];
            let mut count = 0usize;
            let deadline = Instant::now() + OWNER_PROOF_WAIT;
            while count < request.len()
                && !stop.load(Ordering::Acquire)
                && Instant::now() < deadline
            {
                let mut read = 0u32;
                let result = unsafe {
                    ReadFile(
                        raw,
                        request[count..].as_mut_ptr().cast(),
                        (request.len() - count) as u32,
                        &mut read,
                        std::ptr::null_mut(),
                    )
                };
                if result != 0 && read > 0 {
                    count += read as usize;
                } else if result != 0 || unsafe { GetLastError() } == ERROR_NO_DATA {
                    thread::sleep(Duration::from_millis(10));
                } else {
                    break;
                }
            }
            if count == request.len()
                && &request[..8] == OWNER_PROOF_REQUEST
                && !stop.load(Ordering::Acquire)
            {
                let mut response = [0u8; OWNER_PROOF_RESPONSE_BYTES];
                response[..8].copy_from_slice(OWNER_PROOF_RESPONSE);
                response[8..40].copy_from_slice(&request[8..40]);
                response[40..].copy_from_slice(&wire_identity(root, lock));
                let mut written = 0u32;
                let sent = unsafe {
                    WriteFile(
                        raw,
                        response.as_ptr().cast(),
                        response.len() as u32,
                        &mut written,
                        std::ptr::null_mut(),
                    )
                };
                if sent != 0 && written as usize == response.len() {
                    // Keep the server endpoint connected until the client has
                    // consumed the response. DisconnectNamedPipe otherwise
                    // discards unread pipe bytes on Windows.
                    let ack_deadline = Instant::now() + OWNER_PROOF_WAIT;
                    while !stop.load(Ordering::Acquire) && Instant::now() < ack_deadline {
                        let mut ack = 0u8;
                        let mut ack_read = 0u32;
                        let received = unsafe {
                            ReadFile(
                                raw,
                                (&mut ack as *mut u8).cast(),
                                1,
                                &mut ack_read,
                                std::ptr::null_mut(),
                            )
                        };
                        if received != 0 && ack_read == 1 {
                            break;
                        }
                        if received == 0 && unsafe { GetLastError() } != ERROR_NO_DATA {
                            break;
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
            let _ = unsafe { DisconnectNamedPipe(raw) };
        }
    }

    fn owner_unproven(stage: &str) -> String {
        #[cfg(feature = "test-support")]
        {
            format!("WAKE_STORE_AUTHORITY_LOCK_OWNER_UNPROVEN_{stage}")
        }
        #[cfg(not(feature = "test-support"))]
        {
            let _ = stage;
            "WAKE_STORE_AUTHORITY_LOCK_OWNER_UNPROVEN".into()
        }
    }

    fn prove_owner(process: &File, pid: u32, root: ObjectId, lock: ObjectId) -> Result<()> {
        use std::os::windows::io::AsRawHandle;
        let name = pipe_name(pid, root, lock);
        let wide = std::ffi::OsStr::new(&name)
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        let deadline = Instant::now() + OWNER_PROOF_WAIT;
        let pipe = loop {
            let raw = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    GENERIC_READ_WRITE,
                    0,
                    std::ptr::null_mut(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                )
            };
            if !raw.is_null() && raw != INVALID_HANDLE {
                break unsafe { File::from_raw_handle(raw) };
            }
            if Instant::now() >= deadline
                || unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } != WAIT_TIMEOUT
            {
                return Err(owner_unproven("PIPE_OPEN"));
            }
            thread::sleep(Duration::from_millis(10));
        };
        let mut server_pid = 0u32;
        if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut server_pid) } == 0
            || server_pid != pid
        {
            return Err(owner_unproven("PIPE_PID"));
        }
        let mut nonce = [0u8; 32];
        if unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                nonce.as_mut_ptr(),
                nonce.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        } != 0
        {
            return Err("WAKE_STORE_AUTHORITY_NONCE_UNAVAILABLE".into());
        }
        let mut request = [0u8; OWNER_PROOF_REQUEST_BYTES];
        request[..8].copy_from_slice(OWNER_PROOF_REQUEST);
        request[8..].copy_from_slice(&nonce);
        let mut written = 0u32;
        if unsafe {
            WriteFile(
                pipe.as_raw_handle(),
                request.as_ptr().cast(),
                request.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            )
        } == 0
            || written as usize != request.len()
        {
            return Err(owner_unproven("PIPE_WRITE"));
        }
        loop {
            let mut available = 0u32;
            if unsafe {
                PeekNamedPipe(
                    pipe.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(owner_unproven("PIPE_PEEK"));
            }
            if available as usize >= OWNER_PROOF_RESPONSE_BYTES {
                break;
            }
            if Instant::now() >= deadline {
                return Err(owner_unproven("PIPE_TIMEOUT"));
            }
            thread::sleep(Duration::from_millis(10));
        }
        let mut response = [0u8; OWNER_PROOF_RESPONSE_BYTES];
        let mut read = 0u32;
        if unsafe {
            ReadFile(
                pipe.as_raw_handle(),
                response.as_mut_ptr().cast(),
                response.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        } == 0
            || read as usize != response.len()
            || &response[..8] != OWNER_PROOF_RESPONSE
            || response[8..40] != nonce
            || response[40..] != wire_identity(root, lock)
            || unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } != WAIT_TIMEOUT
        {
            return Err(owner_unproven("PIPE_RESPONSE"));
        }
        let ack = *b"A";
        let mut ack_written = 0u32;
        let _ = unsafe {
            WriteFile(
                pipe.as_raw_handle(),
                ack.as_ptr().cast(),
                1,
                &mut ack_written,
                std::ptr::null_mut(),
            )
        };
        Ok(())
    }

    fn display_path(path: &Path) -> String {
        let path = path.to_string_lossy().replace('/', "\\");
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_lowercase()
    }

    fn process_image(process: &File) -> Result<PathBuf> {
        use std::os::windows::io::AsRawHandle;
        let mut image = vec![0u16; 32_768];
        let mut size = image.len() as u32;
        if unsafe {
            QueryFullProcessImageNameW(process.as_raw_handle(), 0, image.as_mut_ptr(), &mut size)
        } == 0
            || size == 0
            || size as usize >= image.len()
        {
            return Err("WAKE_STORE_AUTHORITY_PROCESS_IMAGE_UNAVAILABLE".into());
        }
        Ok(PathBuf::from(std::ffi::OsString::from_wide(
            &image[..size as usize],
        )))
    }

    fn reviewed_install_directory(pointer: &serde_json::Value) -> Result<&str> {
        let id = pointer
            .get("directory")
            .and_then(serde_json::Value::as_str)
            .ok_or("WAKE_STORE_AUTHORITY_POINTER_INVALID")?;
        let Some(suffix) = id.strip_prefix("1.0.0-dev.") else {
            return Err("WAKE_STORE_AUTHORITY_POINTER_INVALID".into());
        };
        let mut parts = suffix.split('-');
        let version = parts.next().unwrap_or("");
        let host_hash = parts.next().unwrap_or("");
        let gui_hash = parts.next().unwrap_or("");
        if pointer
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
            || version.is_empty()
            || !version.bytes().all(|byte| byte.is_ascii_digit())
            || host_hash.len() != 12
            || !host_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            || gui_hash.len() != 12
            || !gui_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            || parts.next().is_some()
        {
            return Err("WAKE_STORE_AUTHORITY_POINTER_INVALID".into());
        }
        Ok(id)
    }

    fn verify_host_image(root: &Path, process: &File) -> Result<()> {
        let actual = process_image(process)?;
        #[cfg(feature = "test-support")]
        if isolated_test_root(root) {
            let expected = std::env::current_exe()
                .map_err(|_| "WAKE_STORE_AUTHORITY_PROCESS_IMAGE_UNAVAILABLE")?;
            return if display_path(&actual) == display_path(&expected) {
                Ok(())
            } else {
                Err("WAKE_STORE_AUTHORITY_PROCESS_IMAGE_MISMATCH".into())
            };
        }

        let mut pointer_file =
            open_pinned_with_access(&root.join("current.json"), false, GENERIC_READ)
                .map_err(|_| "WAKE_STORE_AUTHORITY_POINTER_UNAVAILABLE")?;
        validate_info(info(&pointer_file)?, false)
            .map_err(|_| "WAKE_STORE_AUTHORITY_POINTER_INVALID")?;
        let mut pointer_bytes = Vec::new();
        pointer_file
            .by_ref()
            .take(65_537)
            .read_to_end(&mut pointer_bytes)
            .map_err(|_| "WAKE_STORE_AUTHORITY_POINTER_UNAVAILABLE")?;
        if pointer_bytes.len() > 65_536 {
            return Err("WAKE_STORE_AUTHORITY_POINTER_INVALID".into());
        }
        let pointer: serde_json::Value = serde_json::from_slice(&pointer_bytes)
            .map_err(|_| "WAKE_STORE_AUTHORITY_POINTER_INVALID")?;
        let id = reviewed_install_directory(&pointer)?;
        let expected_hash = pointer
            .get("hostSha256")
            .and_then(serde_json::Value::as_str)
            .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or("WAKE_STORE_AUTHORITY_POINTER_INVALID")?;
        let install = root.join("versions").join(id);
        let install_dir = open_pinned(&install, true)
            .map_err(|_| "WAKE_STORE_AUTHORITY_PROCESS_IMAGE_UNAVAILABLE")?;
        validate_info(info(&install_dir)?, true)
            .map_err(|_| "WAKE_STORE_AUTHORITY_PROCESS_IMAGE_INVALID")?;
        let expected = install.join("CatDeskWakeHost.exe");
        if display_path(&actual) != display_path(&expected) {
            return Err("WAKE_STORE_AUTHORITY_PROCESS_IMAGE_MISMATCH".into());
        }
        let mut image_file = open_pinned_with_access(&expected, false, GENERIC_READ)
            .map_err(|_| "WAKE_STORE_AUTHORITY_PROCESS_IMAGE_UNAVAILABLE")?;
        validate_info(info(&image_file)?, false)
            .map_err(|_| "WAKE_STORE_AUTHORITY_PROCESS_IMAGE_INVALID")?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 65_536];
        let mut size = 0u64;
        loop {
            let count = image_file
                .read(&mut buffer)
                .map_err(|_| "WAKE_STORE_AUTHORITY_PROCESS_IMAGE_UNAVAILABLE")?;
            if count == 0 {
                break;
            }
            size += count as u64;
            if size > 256 * 1024 * 1024 {
                return Err("WAKE_STORE_AUTHORITY_PROCESS_IMAGE_INVALID".into());
            }
            hasher.update(&buffer[..count]);
        }
        if !format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(expected_hash) {
            return Err("WAKE_STORE_AUTHORITY_PROCESS_IMAGE_HASH_MISMATCH".into());
        }
        Ok(())
    }

    #[cfg(feature = "test-support")]
    fn isolated_test_root(root: &Path) -> bool {
        let Some(parent) = std::env::var_os("CATDESK_WAKE_TEST_ONLY_ROOT_PARENT") else {
            return false;
        };
        let (Ok(trusted), Ok(root), Ok(temporary)) = (
            std::fs::canonicalize(parent),
            std::fs::canonicalize(root),
            std::fs::canonicalize(std::env::temp_dir()),
        ) else {
            return false;
        };
        trusted.parent() == Some(temporary.as_path())
            && trusted.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("catdesk-wake-install-authority-test-")
            })
            && root == trusted.join("CatDeskWake")
    }

    /// Installation mutation remains disabled until a protected broker owns
    /// the install namespace and verifies an independent package approval.
    /// The owner challenge proves the host, not the caller. This gate prevents
    /// this CLI from treating self-consistent caller inputs as approval; it
    /// does not protect the current user-owned store against direct writes.
    pub fn require_install_mutation_broker(root: &Path) -> Result<()> {
        #[cfg(feature = "test-support")]
        if isolated_test_root(root) {
            return Ok(());
        }
        let _ = root;
        Err("WAKE_INSTALL_BROKER_UNPROVISIONED".into())
    }

    fn handle_table() -> Result<Vec<u8>> {
        let mut size = 1024 * 1024;
        loop {
            if size > MAX_HANDLE_TABLE_BYTES {
                return Err("WAKE_STORE_AUTHORITY_HANDLE_TABLE_TOO_LARGE".into());
            }
            let mut bytes = vec![0u8; size];
            let mut needed = 0u32;
            let status = unsafe {
                NtQuerySystemInformation(
                    SYSTEM_EXTENDED_HANDLE_INFORMATION,
                    bytes.as_mut_ptr().cast(),
                    size as u32,
                    &mut needed,
                )
            };
            if status == 0 {
                return Ok(bytes);
            }
            if status != STATUS_INFO_LENGTH_MISMATCH {
                return Err("WAKE_STORE_AUTHORITY_HANDLE_QUERY_FAILED".into());
            }
            size = size.saturating_mul(2).max(needed as usize);
        }
    }

    fn read_u64(bytes: &[u8], offset: usize) -> Result<u64> {
        let range = bytes
            .get(offset..offset.saturating_add(8))
            .ok_or("WAKE_STORE_AUTHORITY_HANDLE_COUNT_INVALID")?;
        Ok(u64::from_ne_bytes(range.try_into().map_err(
            |_| "WAKE_STORE_AUTHORITY_HANDLE_COUNT_INVALID",
        )?))
    }

    fn other_process_has_both(
        process: &File,
        pid: u32,
        root: ObjectId,
        lock: ObjectId,
    ) -> Result<()> {
        use std::os::windows::io::AsRawHandle;
        let table = handle_table()?;
        let count = read_u64(&table, 0)? as usize;
        if count > (table.len().saturating_sub(16)) / 40 {
            return Err("WAKE_STORE_AUTHORITY_HANDLE_COUNT_INVALID".into());
        }
        let mut root_matches = 0usize;
        let mut lock_matches = 0usize;
        for index in 0..count {
            let offset = 16 + index * 40;
            if read_u64(&table, offset + 8)? != u64::from(pid) {
                continue;
            }
            let raw_handle = read_u64(&table, offset + 16)? as usize as *mut c_void;
            let mut copy = std::ptr::null_mut();
            let duplicated = unsafe {
                DuplicateHandle(
                    process.as_raw_handle(),
                    raw_handle,
                    GetCurrentProcess(),
                    &mut copy,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            };
            if duplicated == 0 || copy.is_null() || copy == INVALID_HANDLE {
                continue;
            }
            let duplicate = unsafe { File::from_raw_handle(copy) };
            if unsafe { GetFileType(duplicate.as_raw_handle()) } != FILE_TYPE_DISK {
                continue;
            }
            let Ok(held) = info(&duplicate) else {
                continue;
            };
            if ObjectId::from(held) == root && held.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                root_matches += 1;
            }
            if ObjectId::from(held) == lock && held.attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
                lock_matches += 1;
            }
        }
        if root_matches != 1 {
            return Err(if root_matches == 0 {
                "WAKE_STORE_AUTHORITY_LIVE_ROOT_UNPROVEN"
            } else {
                "WAKE_STORE_AUTHORITY_LIVE_ROOT_AMBIGUOUS"
            }
            .into());
        }
        if lock_matches != 1 {
            return Err(if lock_matches == 0 {
                "WAKE_STORE_AUTHORITY_HELD_LOCK_NOT_FOUND"
            } else {
                "WAKE_STORE_AUTHORITY_LOCK_AMBIGUOUS"
            }
            .into());
        }
        if unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } != WAIT_TIMEOUT {
            return Err("WAKE_STORE_AUTHORITY_PROCESS_CHANGED".into());
        }
        Ok(())
    }

    pub struct HostRootPin {
        _root: File,
    }

    impl HostRootPin {
        pub fn acquire(root: &Path) -> Result<Self> {
            let held = open_pinned(root, true)?;
            validate_info(info(&held)?, true)?;
            Ok(Self { _root: held })
        }
    }

    pub struct InstallStoreGuard {
        _root: File,
        _lock: File,
        _versions: Option<File>,
        process: File,
        path: PathBuf,
        root_id: ObjectId,
        lock_id: ObjectId,
        versions_id: Option<ObjectId>,
        pid: u32,
    }

    impl InstallStoreGuard {
        /// Read-only proof of the process currently holding this exact store.
        /// The fingerprint is derived from freshly pinned objects rather than
        /// supplied by the caller; this grants no install mutation authority.
        pub fn acquire_current_for_probe(root: &Path, pid: u32) -> Result<Self> {
            let root_file = open_pinned(root, true)?;
            let lock_file = open_pinned(&root.join("host.lock"), false)?;
            let expected = fingerprint(
                validate_info(info(&root_file)?, true)?,
                validate_info(info(&lock_file)?, false)?,
            );
            Self::acquire_inner(root, pid, &expected, false)
        }

        pub fn acquire(root: &Path, pid: u32, expected: &str) -> Result<Self> {
            Self::acquire_inner(root, pid, expected, true)
        }

        pub fn acquire_for_probe(root: &Path, pid: u32, expected: &str) -> Result<Self> {
            Self::acquire_inner(root, pid, expected, false)
        }

        fn acquire_inner(
            root: &Path,
            pid: u32,
            expected: &str,
            require_versions: bool,
        ) -> Result<Self> {
            if pid == 0 || pid == unsafe { GetCurrentProcessId() } {
                return Err("WAKE_STORE_AUTHORITY_PROCESS_INVALID".into());
            }
            if expected.len() != 64 || !expected.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err("WAKE_STORE_AUTHORITY_FINGERPRINT_INVALID".into());
            }
            let root_file = open_pinned(root, true)?;
            let root_id = validate_info(info(&root_file)?, true)?;
            let lock_file = open_pinned(&root.join("host.lock"), false)?;
            let lock_id = validate_info(info(&lock_file)?, false)?;
            let versions_file = if require_versions {
                Some(
                    open_pinned(&root.join("versions"), true)
                        .map_err(|_| "WAKE_STORE_AUTHORITY_VERSIONS_UNAVAILABLE")?,
                )
            } else {
                None
            };
            let versions_id = versions_file
                .as_ref()
                .map(|file| {
                    validate_info(info(file)?, true)
                        .map_err(|_| "WAKE_STORE_AUTHORITY_VERSIONS_INVALID".to_string())
                })
                .transpose()?;
            match lock_file.try_lock() {
                Ok(()) => {
                    let _ = lock_file.unlock();
                    return Err("WAKE_STORE_AUTHORITY_LOCK_NOT_HELD".into());
                }
                Err(std::fs::TryLockError::WouldBlock) => {}
                Err(std::fs::TryLockError::Error(_)) => {
                    return Err("WAKE_STORE_AUTHORITY_LOCK_STATE_UNAVAILABLE".into());
                }
            }
            if !fingerprint(root_id, lock_id).eq_ignore_ascii_case(expected) {
                return Err("WAKE_STORE_AUTHORITY_FINGERPRINT_MISMATCH".into());
            }
            let raw_process = unsafe {
                OpenProcess(
                    PROCESS_DUP_HANDLE | PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                    0,
                    pid,
                )
            };
            if raw_process.is_null() || raw_process == INVALID_HANDLE {
                return Err("WAKE_STORE_AUTHORITY_PROCESS_UNAVAILABLE".into());
            }
            let process = unsafe { File::from_raw_handle(raw_process) };
            other_process_has_both(&process, pid, root_id, lock_id)?;
            Ok(Self {
                _root: root_file,
                _lock: lock_file,
                _versions: versions_file,
                process,
                path: root.to_path_buf(),
                root_id,
                lock_id,
                versions_id,
                pid,
            })
        }

        /// A byte-range lock conflict alone cannot identify its owner. The
        /// reviewed host must answer a fresh challenge over a pipe whose
        /// server PID is supplied by the Windows kernel. Older hosts without
        /// this protocol remain fail closed.
        pub fn require_lock_owner_proof(&self) -> Result<()> {
            self.recheck_live()?;
            prove_owner(&self.process, self.pid, self.root_id, self.lock_id)?;
            verify_host_image(&self.path, &self.process)?;
            self.recheck_live()
        }

        pub fn recheck_live(&self) -> Result<()> {
            self.recheck_path()?;
            other_process_has_both(&self.process, self.pid, self.root_id, self.lock_id)
        }

        pub fn recheck_path(&self) -> Result<()> {
            let root = open_pinned(&self.path, true)?;
            let lock = open_pinned(&self.path.join("host.lock"), false)?;
            if validate_info(info(&root)?, true)? != self.root_id
                || validate_info(info(&lock)?, false)? != self.lock_id
            {
                return Err("WAKE_STORE_AUTHORITY_OBJECT_CHANGED".into());
            }
            if let Some(versions_id) = self.versions_id {
                let versions = open_pinned(&self.path.join("versions"), true)
                    .map_err(|_| "WAKE_STORE_AUTHORITY_VERSIONS_UNAVAILABLE")?;
                if validate_info(info(&versions)?, true)
                    .map_err(|_| "WAKE_STORE_AUTHORITY_VERSIONS_INVALID")?
                    != versions_id
                {
                    return Err("WAKE_STORE_AUTHORITY_VERSIONS_CHANGED".into());
                }
            }
            Ok(())
        }

        /// Verify that the newly started owner has inherited the same two
        /// physical objects after the old owner exits during activation.
        pub fn recheck_owner(&self, pid: u32) -> Result<()> {
            if pid == 0 || pid == unsafe { GetCurrentProcessId() } {
                return Err("WAKE_STORE_AUTHORITY_PROCESS_INVALID".into());
            }
            self.recheck_path()?;
            match self._lock.try_lock() {
                Ok(()) => {
                    let _ = self._lock.unlock();
                    return Err("WAKE_STORE_AUTHORITY_LOCK_NOT_HELD".into());
                }
                Err(std::fs::TryLockError::WouldBlock) => {}
                Err(std::fs::TryLockError::Error(_)) => {
                    return Err("WAKE_STORE_AUTHORITY_LOCK_STATE_UNAVAILABLE".into());
                }
            }
            let raw_process = unsafe {
                OpenProcess(
                    PROCESS_DUP_HANDLE | PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                    0,
                    pid,
                )
            };
            if raw_process.is_null() || raw_process == INVALID_HANDLE {
                return Err("WAKE_STORE_AUTHORITY_PROCESS_UNAVAILABLE".into());
            }
            let process = unsafe { File::from_raw_handle(raw_process) };
            other_process_has_both(&process, pid, self.root_id, self.lock_id)?;
            prove_owner(&process, pid, self.root_id, self.lock_id)?;
            verify_host_image(&self.path, &process)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn fingerprint_is_stable_and_distinguishes_root_and_lock() {
            let one = ObjectId {
                volume: 1,
                index_high: 2,
                index_low: 3,
            };
            let two = ObjectId {
                volume: 1,
                index_high: 2,
                index_low: 4,
            };
            assert_eq!(fingerprint(one, two), fingerprint(one, two));
            assert_ne!(fingerprint(one, two), fingerprint(two, one));
        }

        #[test]
        fn guard_rejects_self_process_before_object_or_store_mutation() {
            assert_eq!(
                InstallStoreGuard::acquire(
                    Path::new("Z:\\nonexistent"),
                    std::process::id(),
                    &"0".repeat(64)
                )
                .err()
                .unwrap(),
                "WAKE_STORE_AUTHORITY_PROCESS_INVALID"
            );
        }

        #[test]
        fn install_mutation_requires_protected_broker() {
            assert_eq!(
                require_install_mutation_broker(Path::new(r"Z:\nonexistent")).unwrap_err(),
                "WAKE_INSTALL_BROKER_UNPROVISIONED"
            );
        }
    }
}

#[cfg(windows)]
pub use windows::{
    HostOwnerProofServer, HostRootPin, InstallStoreGuard, require_install_mutation_broker,
};

#[cfg(not(windows))]
pub fn require_install_mutation_broker(_root: &Path) -> Result<()> {
    Err("WAKE_INSTALL_BROKER_UNPROVISIONED".into())
}

#[cfg(not(windows))]
pub struct HostRootPin;
#[cfg(not(windows))]
impl HostRootPin {
    pub fn acquire(_root: &Path) -> Result<Self> {
        Ok(Self)
    }
}

#[cfg(not(windows))]
pub struct HostOwnerProofServer<'a> {
    _authority: std::marker::PhantomData<(&'a HostRootPin, &'a HostLease)>,
}
#[cfg(not(windows))]
impl<'a> HostOwnerProofServer<'a> {
    pub fn start(_root: &Path, _pin: &'a HostRootPin, _lease: &'a HostLease) -> Result<Self> {
        Ok(Self {
            _authority: std::marker::PhantomData,
        })
    }
}

#[cfg(not(windows))]
pub struct InstallStoreGuard;
#[cfg(not(windows))]
impl InstallStoreGuard {
    pub fn acquire_current_for_probe(_root: &Path, _pid: u32) -> Result<Self> {
        Err("WAKE_STORE_AUTHORITY_PLATFORM_INVALID".into())
    }
    pub fn acquire(_root: &Path, _pid: u32, _expected: &str) -> Result<Self> {
        Err("WAKE_STORE_AUTHORITY_PLATFORM_INVALID".into())
    }
    pub fn acquire_for_probe(_root: &Path, _pid: u32, _expected: &str) -> Result<Self> {
        Err("WAKE_STORE_AUTHORITY_PLATFORM_INVALID".into())
    }
    pub fn recheck_live(&self) -> Result<()> {
        Err("WAKE_STORE_AUTHORITY_PLATFORM_INVALID".into())
    }
    pub fn recheck_path(&self) -> Result<()> {
        Err("WAKE_STORE_AUTHORITY_PLATFORM_INVALID".into())
    }
    pub fn recheck_owner(&self, _pid: u32) -> Result<()> {
        Err("WAKE_STORE_AUTHORITY_PLATFORM_INVALID".into())
    }
    pub fn require_lock_owner_proof(&self) -> Result<()> {
        Err("WAKE_STORE_AUTHORITY_LOCK_OWNER_UNPROVEN".into())
    }
}
