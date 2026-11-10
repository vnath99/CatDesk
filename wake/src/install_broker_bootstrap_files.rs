//! Fixed create-new filesystem half of first broker provisioning. No existing
//! namespace ACL, policy, image, selection or task state is ever rewritten.
#![cfg(windows)]
use crate::{
    Result,
    install_broker_approval::ObjectIdentityV1,
    install_broker_bootstrap::{BootstrapManifestV1, digest, validate_independent_review},
    install_broker_offline_pins::full_identity_of_handle,
    install_broker_offline_v2::FullFileIdentityV2,
    install_broker_policy::{SameStateRootPolicyV2, encode_same_state_bootstrap_policy},
    install_broker_preflight::{
        canonical_broker_image_file_security, canonical_broker_image_parent_security,
        canonical_broker_policy_security, canonical_protected_security,
    },
};
use std::{
    ffi::c_void,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Component, Path, PathBuf},
    ptr,
};

const READ: u32 = 0x0012_0081;
const WRITE: u32 = 0x0012_0083;
const INVALID_HANDLE: *mut c_void = -1isize as *mut c_void;
const NOT_FOUND: u32 = 0xc000_0034;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root: *mut c_void,
    name: *mut UnicodeString,
    attributes: u32,
    security: *mut c_void,
    qos: *mut c_void,
}
#[repr(C)]
struct IoStatus {
    status: isize,
    information: usize,
}
#[repr(C)]
#[derive(Default)]
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
#[repr(C)]
struct Acl {
    revision: u8,
    sbz1: u8,
    size: u16,
    ace_count: u16,
    sbz2: u16,
}
#[repr(C)]
struct AceHeader {
    kind: u8,
    flags: u8,
    size: u16,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Luid {
    low: u32,
    high: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LuidAttributes {
    luid: Luid,
    attributes: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TokenPrivileges {
    count: u32,
    entry: LuidAttributes,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        handle: *mut *mut c_void,
        access: u32,
        attributes: *mut ObjectAttributes,
        status: *mut IoStatus,
        allocation: *mut c_void,
        file_attributes: u32,
        share: u32,
        disposition: u32,
        options: u32,
        ea: *mut c_void,
        ea_size: u32,
    ) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileInformationByHandle(handle: *mut c_void, info: *mut FileInfo) -> i32;
    fn LocalFree(value: *mut c_void) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetLastError() -> u32;
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn GetSecurityInfo(
        handle: *mut c_void,
        kind: u32,
        information: u32,
        owner: *mut *mut c_void,
        group: *mut *mut c_void,
        dacl: *mut *mut c_void,
        sacl: *mut *mut c_void,
        descriptor: *mut *mut c_void,
    ) -> u32;
    fn ConvertSecurityDescriptorToStringSecurityDescriptorW(
        descriptor: *mut c_void,
        revision: u32,
        information: u32,
        output: *mut *mut u16,
        length: *mut u32,
    ) -> i32;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        text: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        length: *mut u32,
    ) -> i32;
    fn ConvertSidToStringSidW(sid: *mut c_void, text: *mut *mut u16) -> i32;
    fn GetAce(acl: *const Acl, index: u32, ace: *mut *mut c_void) -> i32;
    fn LookupAccountNameW(
        system: *const u16,
        name: *const u16,
        sid: *mut c_void,
        sid_size: *mut u32,
        domain: *mut u16,
        domain_size: *mut u32,
        use_type: *mut u32,
    ) -> i32;
    fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
    fn LookupPrivilegeValueW(system: *const u16, name: *const u16, luid: *mut Luid) -> i32;
    fn AdjustTokenPrivileges(
        token: *mut c_void,
        disable_all: i32,
        new: *const TokenPrivileges,
        size: u32,
        previous: *mut TokenPrivileges,
        returned: *mut u32,
    ) -> i32;
    fn GetTokenInformation(
        token: *mut c_void,
        class: u32,
        buffer: *mut c_void,
        length: u32,
        returned: *mut u32,
    ) -> i32;
}
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0) };
        }
    }
}
fn wide(v: &str) -> Vec<u16> {
    v.encode_utf16().chain(Some(0)).collect()
}
unsafe fn wide_string(p: *const u16) -> Result<String> {
    if p.is_null() {
        return Err("WAKE_BOOTSTRAP_SECURITY_INVALID".into());
    }
    let mut n = 0;
    while n < 8192 && unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    if n == 8192 {
        return Err("WAKE_BOOTSTRAP_SECURITY_INVALID".into());
    }
    String::from_utf16(unsafe { std::slice::from_raw_parts(p, n) })
        .map_err(|_| "WAKE_BOOTSTRAP_SECURITY_INVALID".into())
}
fn sid_string(p: *mut c_void) -> Result<String> {
    let mut text = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(p, &mut text) } == 0 {
        return Err("WAKE_BOOTSTRAP_SECURITY_INVALID".into());
    }
    let held = Local(text.cast());
    unsafe { wide_string(held.0.cast()) }
}
fn trusted_installer() -> Result<String> {
    let name = wide(r"NT SERVICE\TrustedInstaller");
    let mut size = 0;
    let mut domain_size = 0;
    let mut use_type = 0;
    unsafe {
        LookupAccountNameW(
            ptr::null(),
            name.as_ptr(),
            ptr::null_mut(),
            &mut size,
            ptr::null_mut(),
            &mut domain_size,
            &mut use_type,
        )
    };
    if size == 0 || size > 256 || domain_size > 256 {
        return Err("WAKE_BOOTSTRAP_TRUSTED_OWNER_UNAVAILABLE".into());
    }
    let mut sid = vec![0u8; size as usize];
    let mut domain = vec![0u16; domain_size.max(1) as usize];
    if unsafe {
        LookupAccountNameW(
            ptr::null(),
            name.as_ptr(),
            sid.as_mut_ptr().cast(),
            &mut size,
            domain.as_mut_ptr(),
            &mut domain_size,
            &mut use_type,
        )
    } == 0
    {
        return Err("WAKE_BOOTSTRAP_TRUSTED_OWNER_UNAVAILABLE".into());
    }
    sid_string(sid.as_mut_ptr().cast())
}
struct Security {
    _descriptor: Local,
    owner: String,
    sddl: String,
    dacl: *mut Acl,
}
fn descriptor_text(descriptor: *mut c_void) -> Result<String> {
    let mut text = ptr::null_mut();
    let mut length = 0;
    if unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            1,
            7,
            &mut text,
            &mut length,
        )
    } == 0
        || length == 0
        || length > 8192
    {
        return Err("WAKE_BOOTSTRAP_SECURITY_INVALID".into());
    }
    let held = Local(text.cast());
    String::from_utf16(unsafe { std::slice::from_raw_parts(held.0.cast(), length as usize) })
        .map_err(|_| "WAKE_BOOTSTRAP_SECURITY_INVALID".into())
}
fn security(file: &File) -> Result<Security> {
    let mut descriptor = ptr::null_mut();
    let mut owner = ptr::null_mut();
    let mut dacl = ptr::null_mut();
    let result = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            1,
            7,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    let descriptor = Local(descriptor);
    if result != 0 || descriptor.0.is_null() || dacl.is_null() {
        return Err("WAKE_BOOTSTRAP_SECURITY_INVALID".into());
    }
    let owner = sid_string(owner)?;
    // Match the existing canonical helpers' exact API-reported buffer length,
    // including its terminator; never compare different SDDL projections.
    let sddl = descriptor_text(descriptor.0)?;
    Ok(Security {
        _descriptor: descriptor,
        owner,
        sddl,
        dacl: dacl.cast(),
    })
}
fn safe_parent(file: &File, outer_create_only: bool) -> Result<()> {
    let security = security(file)?;
    let ti = trusted_installer()?;
    let trusted = |sid: &str| sid == "S-1-5-18" || sid == "S-1-5-32-544" || sid == ti;
    if !trusted(&security.owner) {
        return Err("WAKE_BOOTSTRAP_UNSAFE_PARENT".into());
    }
    for index in 0..unsafe { (*security.dacl).ace_count } {
        let mut ace = ptr::null_mut();
        if unsafe { GetAce(security.dacl, index as u32, &mut ace) } == 0 || ace.is_null() {
            return Err("WAKE_BOOTSTRAP_UNSAFE_PARENT".into());
        }
        let head = unsafe { &*ace.cast::<AceHeader>() };
        if head.flags & 8 != 0 || head.kind == 1 {
            continue;
        } // inherit-only/deny grants no current write
        if head.kind != 0 || head.size < 12 {
            return Err("WAKE_BOOTSTRAP_UNSAFE_PARENT".into());
        }
        let mask = unsafe { *ace.cast::<u8>().add(4).cast::<u32>() };
        let sid = sid_string(unsafe { ace.cast::<u8>().add(8) }.cast())?;
        if !trusted(&sid) && dangerous_parent_mask(mask, outer_create_only) {
            return Err("WAKE_BOOTSTRAP_UNSAFE_PARENT".into());
        }
    }
    Ok(())
}
fn dangerous_parent_mask(mask: u32, outer_create_only: bool) -> bool {
    let mut dangerous =
        0x1000_0000 | 0x4000_0000 | 0x0001_0000 | 0x0004_0000 | 0x0008_0000 | 0x0000_0040;
    // Without an exclusive metadata pin only outer CREATE is tolerated. The
    // immediate pinned parents also tolerate EA/attributes: no existing/new
    // WRITE handle can coexist, and exact relative CREATE never replaces.
    if outer_create_only {
        dangerous |= 0x110;
    }
    mask & dangerous != 0
}
fn inspect(file: &File, directory: bool) -> Result<(FullFileIdentityV2, ObjectIdentityV1, u64)> {
    let mut info = FileInfo::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
        || info.attributes & 0x400 != 0
        || (info.attributes & 0x10 != 0) != directory
        || (!directory && info.links != 1)
    {
        return Err("WAKE_BOOTSTRAP_OBJECT_INVALID".into());
    }
    let identity = full_identity_of_handle(file.as_raw_handle())?;
    Ok((
        identity,
        ObjectIdentityV1 {
            volume_serial: info.volume,
            file_index_high: info.index_high,
            file_index_low: info.index_low,
        },
        ((info.size_high as u64) << 32) | info.size_low as u64,
    ))
}
fn relative(
    parent: &File,
    name: &str,
    directory: bool,
    create: Option<&str>,
) -> Result<Option<File>> {
    relative_with_sharing(parent, name, directory, create, None)
}
fn relative_with_sharing(
    parent: &File,
    name: &str,
    directory: bool,
    create: Option<&str>,
    share_override: Option<u32>,
) -> Result<Option<File>> {
    if name.is_empty()
        || name.len() > 255
        || matches!(name, "." | "..")
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
    {
        return Err("WAKE_BOOTSTRAP_COMPONENT_INVALID".into());
    }
    let mut units: Vec<u16> = name.encode_utf16().collect();
    let mut text = UnicodeString {
        length: (units.len() * 2) as u16,
        maximum_length: (units.len() * 2) as u16,
        buffer: units.as_mut_ptr(),
    };
    let mut descriptor = ptr::null_mut();
    if let Some(sddl) = create
        && unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(sddl).as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
    {
        return Err("WAKE_BOOTSTRAP_SECURITY_INVALID".into());
    }
    let held = Local(descriptor);
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root: parent.as_raw_handle(),
        name: &mut text,
        attributes: 0x40,
        security: held.0,
        qos: ptr::null_mut(),
    };
    let mut status = IoStatus {
        status: 0,
        information: 0,
    };
    let mut handle = ptr::null_mut();
    let access = if create.is_some() {
        if directory { READ | 6 } else { WRITE }
    } else {
        READ
    };
    let share = share_override.unwrap_or(if create.is_some() || directory { 3 } else { 1 });
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &mut attributes,
            &mut status,
            ptr::null_mut(),
            0,
            share,
            if create.is_some() { 2 } else { 1 },
            (if directory { 1 } else { 0x40 }) | 0x20 | 0x200000 | 0x4000,
            ptr::null_mut(),
            0,
        )
    };
    if result < 0 {
        if result as u32 == NOT_FOUND && create.is_none() {
            return Ok(None);
        }
        if result as u32 == 0xc0000043 && share_override == Some(1) && directory && create.is_none()
        {
            return Err("WAKE_BOOTSTRAP_PARENT_BUSY".into());
        }
        return Err(if create.is_some() {
            "WAKE_BOOTSTRAP_CREATE_ONLY_PARTIAL_ATTENTION"
        } else {
            "WAKE_BOOTSTRAP_OBJECT_UNAVAILABLE"
        }
        .into());
    }
    if handle.is_null() || handle == INVALID_HANDLE {
        return Err("WAKE_BOOTSTRAP_OBJECT_UNAVAILABLE".into());
    }
    let file = unsafe { File::from_raw_handle(handle) };
    inspect(&file, directory)?;
    Ok(Some(file))
}
struct Pin {
    file: File,
    path: PathBuf,
    directory: bool,
    identity: FullFileIdentityV2,
    sddl: String,
}
impl Pin {
    fn from_file(file: File, path: PathBuf, directory: bool) -> Result<Self> {
        let (identity, _, _) = inspect(&file, directory)?;
        let sddl = security(&file)?.sddl;
        Ok(Self {
            file,
            path,
            directory,
            identity,
            sddl,
        })
    }
    fn open_root(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .access_mode(READ)
            .share_mode(3)
            .custom_flags(0x02200000)
            .open(path)
            .map_err(|_| "WAKE_BOOTSTRAP_OBJECT_UNAVAILABLE")?;
        Self::from_file(file, path.into(), true)
    }
    fn child(&self, name: &str, directory: bool) -> Result<Option<Self>> {
        relative(&self.file, name, directory, None)?
            .map(|file| Self::from_file(file, self.path.join(name), directory))
            .transpose()
    }
    fn child_no_write(&self, name: &str) -> Result<Option<Self>> {
        relative_with_sharing(&self.file, name, true, None, Some(1))?
            .map(|file| Self::from_file(file, self.path.join(name), true))
            .transpose()
    }
    fn create(&self, name: &str, directory: bool, sddl: &str) -> Result<Self> {
        let file = relative(&self.file, name, directory, Some(sddl))?
            .ok_or("WAKE_BOOTSTRAP_CREATE_ONLY_PARTIAL_ATTENTION")?;
        let result = Self::from_file(file, self.path.join(name), directory)?;
        if result.sddl != sddl || security(&result.file)?.owner != "S-1-5-18" {
            return Err("WAKE_BOOTSTRAP_CREATED_SECURITY_MISMATCH".into());
        }
        Ok(result)
    }
    fn revalidate(&self) -> Result<()> {
        if inspect(&self.file, self.directory)?.0 != self.identity
            || security(&self.file)?.sddl != self.sddl
        {
            return Err("WAKE_BOOTSTRAP_OBJECT_CHANGED".into());
        }
        let reopened = OpenOptions::new()
            .access_mode(READ)
            .share_mode(3)
            .custom_flags(0x02200000)
            .open(&self.path)
            .map_err(|_| "WAKE_BOOTSTRAP_OBJECT_CHANGED")?;
        if inspect(&reopened, self.directory)?.0 != self.identity {
            return Err("WAKE_BOOTSTRAP_OBJECT_CHANGED".into());
        }
        Ok(())
    }
    fn raw(&mut self, maximum: u64) -> Result<Vec<u8>> {
        self.revalidate()?;
        let length = inspect(&self.file, false)?.2;
        if length > maximum {
            return Err("WAKE_BOOTSTRAP_INPUT_TOO_LARGE".into());
        }
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|_| "WAKE_BOOTSTRAP_READ_FAILED")?;
        let mut raw = Vec::new();
        (&mut self.file)
            .take(maximum + 1)
            .read_to_end(&mut raw)
            .map_err(|_| "WAKE_BOOTSTRAP_READ_FAILED")?;
        if raw.len() as u64 != length {
            return Err("WAKE_BOOTSTRAP_OBJECT_CHANGED".into());
        }
        self.revalidate()?;
        Ok(raw)
    }
}
struct Input {
    pins: Vec<Pin>,
    raw: Vec<u8>,
}
impl Input {
    fn open(path: &Path, maximum: u64) -> Result<Self> {
        let value = path.to_str().ok_or("WAKE_BOOTSTRAP_PATH_INVALID")?;
        let bytes = value.as_bytes();
        if bytes.len() < 3
            || bytes.len() > 2048
            || !bytes[0].is_ascii_alphabetic()
            || bytes[1] != b':'
            || !matches!(bytes[2], b'\\' | b'/')
            || value[2..].contains(':')
            || value.chars().any(char::is_control)
            || value.split(['\\', '/']).any(|v| matches!(v, "." | ".."))
        {
            return Err("WAKE_BOOTSTRAP_PATH_INVALID".into());
        }
        let components = path
            .components()
            .filter_map(|c| {
                if let Component::Normal(v) = c {
                    Some(v.to_str().map(str::to_owned))
                } else {
                    None
                }
            })
            .collect::<Option<Vec<_>>>()
            .ok_or("WAKE_BOOTSTRAP_PATH_INVALID")?;
        if components.is_empty() {
            return Err("WAKE_BOOTSTRAP_PATH_INVALID".into());
        }
        let mut pins = vec![Pin::open_root(Path::new(&format!("{}\\", &value[..2])))?];
        for (index, name) in components.iter().enumerate() {
            let directory = index + 1 != components.len();
            let next = pins
                .last()
                .unwrap()
                .child(name, directory)?
                .ok_or("WAKE_BOOTSTRAP_INPUT_MISSING")?;
            pins.push(next);
        }
        let raw = pins.last_mut().unwrap().raw(maximum)?;
        Ok(Self { pins, raw })
    }
    fn revalidate(&self) -> Result<()> {
        for pin in &self.pins {
            pin.revalidate()?;
        }
        let pin = self.pins.last().ok_or("WAKE_BOOTSTRAP_INPUT_MISSING")?;
        let mut file = pin
            .file
            .try_clone()
            .map_err(|_| "WAKE_BOOTSTRAP_OBJECT_CHANGED")?;
        file.seek(SeekFrom::Start(0))
            .map_err(|_| "WAKE_BOOTSTRAP_OBJECT_CHANGED")?;
        let mut observed = Vec::new();
        (&mut file)
            .take(self.raw.len() as u64 + 1)
            .read_to_end(&mut observed)
            .map_err(|_| "WAKE_BOOTSTRAP_OBJECT_CHANGED")?;
        if observed != self.raw {
            return Err("WAKE_BOOTSTRAP_OBJECT_CHANGED".into());
        }
        pin.revalidate()?;
        Ok(())
    }
}
pub(crate) fn read_bootstrap_approval(path: &Path) -> Result<Vec<u8>> {
    Ok(Input::open(path, 16 * 1024)?.raw)
}

struct TargetParent {
    base: Pin,
    catdesk: Option<Pin>,
    leaf: &'static str,
}
impl TargetParent {
    fn observe(root: &Pin, name: &str, leaf: &'static str) -> Result<Self> {
        let base = root
            .child_no_write(name)?
            .ok_or("WAKE_BOOTSTRAP_PARENT_MISSING")?;
        safe_parent(&base.file, false)?;
        let catdesk = base.child_no_write("CatDesk")?;
        if let Some(pin) = &catdesk {
            safe_parent(&pin.file, false)?;
        }
        let result = Self {
            base,
            catdesk,
            leaf,
        };
        result.absent()?;
        Ok(result)
    }
    fn absent(&self) -> Result<()> {
        self.base.revalidate()?;
        if let Some(parent) = &self.catdesk {
            parent.revalidate()?;
            if parent.child(self.leaf, true)?.is_some() {
                return Err("WAKE_BOOTSTRAP_ALREADY_OR_PARTIALLY_PROVISIONED".into());
            }
        } else if self.base.child("CatDesk", true)?.is_some() {
            return Err("WAKE_BOOTSTRAP_PARENT_CHANGED".into());
        }
        Ok(())
    }
    fn create(&mut self, sddl: &str) -> Result<Pin> {
        self.absent()?;
        if self.catdesk.is_none() {
            self.catdesk = Some(self.base.create("CatDesk", true, sddl)?);
        }
        self.catdesk.as_ref().unwrap().create(self.leaf, true, sddl)
    }
}
pub(crate) struct FixedBootstrapFiles {
    root: Pin,
    data: TargetParent,
    image: TargetParent,
    installer: Input,
    broker: Input,
    review: Input,
    state: Vec<Pin>,
    state_hashes: [String; 3],
}
impl FixedBootstrapFiles {
    pub(crate) fn observe(m: &BootstrapManifestV1) -> Result<Self> {
        let installer = Input::open(
            &std::env::current_exe().map_err(|_| "WAKE_BOOTSTRAP_INSTALLER_UNAVAILABLE")?,
            80 * 1024 * 1024,
        )?;
        if installer.raw.len() as u64 != m.installer.byte_length
            || digest(&installer.raw) != m.installer.sha256
        {
            return Err("WAKE_BOOTSTRAP_INSTALLER_IDENTITY_MISMATCH".into());
        }
        let broker = Input::open(Path::new(&m.broker_artifact.path), 80 * 1024 * 1024)?;
        let review = Input::open(Path::new(&m.independent_review.path), 64 * 1024)?;
        if broker.raw.len() as u64 != m.broker_artifact.byte_length
            || digest(&broker.raw) != m.broker_artifact.sha256
            || review.raw.len() as u64 != m.independent_review.byte_length
            || digest(&review.raw) != m.independent_review.sha256
        {
            return Err("WAKE_BOOTSTRAP_INPUT_IDENTITY_MISMATCH".into());
        }
        validate_independent_review(&review.raw, m)?;
        let state_root = Pin::open_root(Path::new(&m.state_root.path))?;
        if state_root.identity != m.state_root.identity {
            return Err("WAKE_BOOTSTRAP_STATE_ROOT_MISMATCH".into());
        }
        // Pin each ancestor of the existing root with no reparse traversal.
        let mut state = Vec::new();
        let mut paths = Path::new(&m.state_root.path)
            .ancestors()
            .collect::<Vec<_>>();
        paths.reverse();
        for path in paths {
            state.push(Pin::open_root(path)?);
        }
        if state.last().is_none_or(|pin| {
            pin.identity != m.state_root.identity || pin.identity != state_root.identity
        }) {
            return Err("WAKE_BOOTSTRAP_STATE_ROOT_MISMATCH".into());
        }
        let mut hashes = Vec::new();
        for (name, expected) in [
            ("current.json", &m.state_root.current_sha256),
            ("control.json", &m.state_root.control_sha256),
            ("config.json", &m.state_root.config_sha256),
        ] {
            let mut pin = state_root
                .child(name, false)?
                .ok_or("WAKE_BOOTSTRAP_STATE_MISSING")?;
            let raw = pin.raw(64 * 1024)?;
            if digest(&raw) != *expected {
                return Err("WAKE_BOOTSTRAP_STATE_CHANGED".into());
            }
            if name == "control.json" {
                let v: serde_json::Value =
                    serde_json::from_slice(&raw).map_err(|_| "WAKE_BOOTSTRAP_STATE_INVALID")?;
                if v["schemaVersion"] != 1 || v["desired"] != "STOPPED" {
                    return Err("WAKE_BOOTSTRAP_STOPPED_REQUIRED".into());
                }
            }
            hashes.push(expected.clone());
            state.push(pin);
        }
        let root = Pin::open_root(Path::new(r"C:\"))?;
        safe_parent(&root.file, true)?;
        let data = TargetParent::observe(&root, "ProgramData", "WakeProtectedInstallV1")?;
        let image = TargetParent::observe(&root, "Program Files", "WakeInstallBrokerV1")?;
        Ok(Self {
            root,
            data,
            image,
            installer,
            broker,
            review,
            state,
            state_hashes: hashes
                .try_into()
                .map_err(|_| "WAKE_BOOTSTRAP_STATE_INVALID")?,
        })
    }
    pub(crate) fn revalidate_state_and_sources(&self) -> Result<()> {
        self.root.revalidate()?;
        self.installer.revalidate()?;
        self.broker.revalidate()?;
        self.review.revalidate()?;
        for pin in &self.state {
            pin.revalidate()?;
        }
        for (pin, expected) in self.state[self.state.len() - 3..]
            .iter()
            .zip(&self.state_hashes)
        {
            let mut file = pin
                .file
                .try_clone()
                .map_err(|_| "WAKE_BOOTSTRAP_STATE_CHANGED")?;
            file.seek(SeekFrom::Start(0))
                .map_err(|_| "WAKE_BOOTSTRAP_STATE_CHANGED")?;
            let mut raw = Vec::new();
            (&mut file)
                .take(64 * 1024 + 1)
                .read_to_end(&mut raw)
                .map_err(|_| "WAKE_BOOTSTRAP_STATE_CHANGED")?;
            if digest(&raw) != *expected {
                return Err("WAKE_BOOTSTRAP_STATE_CHANGED".into());
            }
        }
        Ok(())
    }
    pub(crate) fn revalidate_absent(&self) -> Result<()> {
        self.revalidate_state_and_sources()?;
        self.data.absent()?;
        self.image.absent()
    }
    pub(crate) fn create_fixed(
        &mut self,
        service_sid: &str,
        m: &BootstrapManifestV1,
    ) -> Result<CreatedBootstrapFiles> {
        self.revalidate_absent()?;
        let protected_sddl = canonical_protected_security(service_sid, true)?;
        let image_sddl = canonical_broker_image_parent_security(service_sid)?;
        let protected = self.data.create(&protected_sddl)?;
        let image = self.image.create(&image_sddl)?;
        let root_id = inspect(&protected.file, true)?.1;
        let policy_sddl = canonical_broker_policy_security(service_sid)?;
        let mut policy = protected.create("broker-policy.json", false, &policy_sddl)?;
        let policy_id = inspect(&policy.file, false)?.1;
        let bytes = encode_same_state_bootstrap_policy(
            root_id,
            policy_id,
            service_sid,
            &m.operator_sid,
            &m.approval_public_key_hex,
            &m.reviewer_identity,
            SameStateRootPolicyV2 {
                path: m.state_root.path.clone(),
                identity: m.state_root.identity.clone(),
                allow_reviewed_reconstruction: m.allow_reviewed_reconstruction,
            },
            m.allow_operator_service_start,
        )?;
        policy
            .file
            .write_all(&bytes)
            .and_then(|()| policy.file.sync_all())
            .map_err(|_| "WAKE_BOOTSTRAP_PARTIAL_WRITE_ATTENTION")?;
        let policy_identity = policy.identity.clone();
        drop(policy);
        let mut policy = protected
            .child("broker-policy.json", false)?
            .ok_or("WAKE_BOOTSTRAP_PARTIAL_WRITE_ATTENTION")?;
        if policy.identity != policy_identity || policy.raw(4096)? != bytes {
            return Err("WAKE_BOOTSTRAP_PARTIAL_WRITE_ATTENTION".into());
        }
        let mut installed = image.create(
            "CatDeskWakeInstallBroker.exe",
            false,
            &canonical_broker_image_file_security(service_sid)?,
        )?;
        installed
            .file
            .write_all(&self.broker.raw)
            .and_then(|()| installed.file.sync_all())
            .map_err(|_| "WAKE_BOOTSTRAP_PARTIAL_WRITE_ATTENTION")?;
        let installed_id = installed.identity.clone();
        drop(installed);
        let mut installed = image
            .child("CatDeskWakeInstallBroker.exe", false)?
            .ok_or("WAKE_BOOTSTRAP_PARTIAL_WRITE_ATTENTION")?;
        if installed.identity != installed_id
            || digest(&installed.raw(80 * 1024 * 1024)?) != m.broker_artifact.sha256
        {
            return Err("WAKE_BOOTSTRAP_PARTIAL_WRITE_ATTENTION".into());
        }
        let lock = protected.create(
            "install-ledger.lock",
            false,
            &canonical_protected_security(service_sid, false)?,
        )?;
        lock.file
            .sync_all()
            .map_err(|_| "WAKE_BOOTSTRAP_PARTIAL_WRITE_ATTENTION")?;
        for pin in [&protected, &image] {
            let handle: OwnedHandle = pin
                .file
                .try_clone()
                .map_err(|_| "WAKE_BOOTSTRAP_DIRECTORY_FLUSH_UNPROVEN")?
                .into();
            crate::install_broker_durability::flush_directory_handle(
                &handle,
                &inspect(&pin.file, true)?.1,
                crate::install_broker_durability::ParentFlushPoint::StageLeafEntries,
            )?;
        }
        self.revalidate_state_and_sources()?;
        Ok(CreatedBootstrapFiles {
            pins: vec![protected, image, policy, installed, lock],
            policy_sha256: digest(&bytes),
        })
    }
}
pub(crate) struct CreatedBootstrapFiles {
    pins: Vec<Pin>,
    policy_sha256: String,
}
impl CreatedBootstrapFiles {
    pub(crate) fn policy_sha256(&self) -> &str {
        &self.policy_sha256
    }
    pub(crate) fn revalidate(&self) -> Result<()> {
        for pin in &self.pins {
            pin.revalidate()?;
        }
        Ok(())
    }
}

pub(crate) struct RestorePrivilege {
    token: OwnedHandle,
    previous: TokenPrivileges,
}
impl RestorePrivilege {
    fn restore_or_abort(token: &OwnedHandle, previous: &TokenPrivileges) {
        let restored = unsafe {
            AdjustTokenPrivileges(
                token.as_raw_handle(),
                0,
                previous,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if restored == 0 || unsafe { GetLastError() } != 0 {
            std::process::abort();
        }
    }
    fn token(access: u32) -> Result<OwnedHandle> {
        let mut token = ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut token) } == 0
            || token.is_null()
        {
            return Err("WAKE_BOOTSTRAP_RESTORE_PRIVILEGE_UNAVAILABLE".into());
        }
        Ok(unsafe { OwnedHandle::from_raw_handle(token) })
    }
    fn luid() -> Result<Luid> {
        let mut luid = Luid::default();
        if unsafe {
            LookupPrivilegeValueW(ptr::null(), wide("SeRestorePrivilege").as_ptr(), &mut luid)
        } == 0
        {
            return Err("WAKE_BOOTSTRAP_RESTORE_PRIVILEGE_UNAVAILABLE".into());
        }
        Ok(luid)
    }
    pub(crate) fn check_available() -> Result<()> {
        let token = Self::token(8)?;
        let wanted = Self::luid()?;
        let mut needed = 0;
        unsafe { GetTokenInformation(token.as_raw_handle(), 3, ptr::null_mut(), 0, &mut needed) };
        if !(4..=65536).contains(&needed) {
            return Err("WAKE_BOOTSTRAP_RESTORE_PRIVILEGE_UNAVAILABLE".into());
        }
        let mut buffer = vec![0u32; (needed as usize).div_ceil(4)];
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                3,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        } == 0
        {
            return Err("WAKE_BOOTSTRAP_RESTORE_PRIVILEGE_UNAVAILABLE".into());
        }
        let count = buffer[0] as usize;
        if count > 256 || 4 + count * std::mem::size_of::<LuidAttributes>() > needed as usize {
            return Err("WAKE_BOOTSTRAP_RESTORE_PRIVILEGE_UNAVAILABLE".into());
        }
        let entries = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().cast::<u8>().add(4).cast::<LuidAttributes>(),
                count,
            )
        };
        if !entries.iter().any(|v| {
            v.luid.low == wanted.low && v.luid.high == wanted.high && v.attributes & 4 == 0
        }) {
            return Err("WAKE_BOOTSTRAP_RESTORE_PRIVILEGE_UNAVAILABLE".into());
        }
        Ok(())
    }
    pub(crate) fn acquire() -> Result<Self> {
        Self::check_available()?;
        let token = Self::token(0x28)?;
        let next = TokenPrivileges {
            count: 1,
            entry: LuidAttributes {
                luid: Self::luid()?,
                attributes: 2,
            },
        };
        let mut previous = TokenPrivileges::default();
        let mut returned = 0;
        let changed = unsafe {
            AdjustTokenPrivileges(
                token.as_raw_handle(),
                0,
                &next,
                std::mem::size_of::<TokenPrivileges>() as u32,
                &mut previous,
                &mut returned,
            )
        };
        let error = unsafe { GetLastError() };
        if changed == 0 || error != 0 {
            if changed != 0 {
                Self::restore_or_abort(&token, &previous);
            }
            return Err("WAKE_BOOTSTRAP_RESTORE_PRIVILEGE_UNAVAILABLE".into());
        }
        Ok(Self { token, previous })
    }
}
impl Drop for RestorePrivilege {
    fn drop(&mut self) {
        Self::restore_or_abort(&self.token, &self.previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bootstrap_native_sddl_roundtrip_matches_existing_exact_canonical_helpers() {
        let service = "S-1-5-80-1-2-3-4-5";
        for expected in [
            canonical_protected_security(service, true).unwrap(),
            canonical_protected_security(service, false).unwrap(),
            canonical_broker_policy_security(service).unwrap(),
            canonical_broker_image_parent_security(service).unwrap(),
            canonical_broker_image_file_security(service).unwrap(),
        ] {
            let mut descriptor = ptr::null_mut();
            assert_ne!(
                unsafe {
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(
                        wide(&expected).as_ptr(),
                        1,
                        &mut descriptor,
                        ptr::null_mut(),
                    )
                },
                0
            );
            let held = Local(descriptor);
            assert_eq!(descriptor_text(held.0).unwrap(), expected);
        }
    }
    #[test]
    fn bootstrap_parent_access_distinguishes_outer_create_only_from_namespace_writes() {
        assert!(!dangerous_parent_mask(6, true));
        assert!(!dangerous_parent_mask(0x116, false));
        for mask in [0x10000, 0x40000, 0x80000, 0x40, 0x10000000, 0x40000000] {
            assert!(dangerous_parent_mask(mask, true));
            assert!(dangerous_parent_mask(mask, false));
        }
        for mask in [0x10, 0x100] {
            assert!(dangerous_parent_mask(mask, true));
            assert!(!dangerous_parent_mask(mask, false));
        }
        assert!(!dangerous_parent_mask(0x001200a9, false));
    }
    #[test]
    fn bootstrap_native_relative_create_is_once_only_and_retains_existing_bytes() {
        let root = std::env::temp_dir().join(format!(
            "catdesk-bootstrap-relative-{}-{}",
            std::process::id(),
            crate::store::now()
        ));
        std::fs::create_dir(&root).unwrap();
        let held = OpenOptions::new()
            .access_mode(READ)
            .share_mode(1)
            .custom_flags(0x02200000)
            .open(&root)
            .unwrap();
        let parent = Pin::from_file(held, root.clone(), true).unwrap();
        assert!(
            OpenOptions::new()
                .access_mode(WRITE)
                .share_mode(3)
                .custom_flags(0x02200000)
                .open(&root)
                .is_err(),
            "retained no-WRITE parent pin refuses metadata writers"
        );
        let owner = security(&parent.file).unwrap().owner;
        let sddl = format!("D:P(A;;FA;;;{owner})");
        let mut file = relative(&parent.file, "frozen.bin", false, Some(&sddl))
            .unwrap()
            .unwrap();
        file.write_all(b"original must survive").unwrap();
        file.sync_all().unwrap();
        drop(file);
        assert!(relative(&parent.file, "frozen.bin", false, Some(&sddl)).is_err());
        assert_eq!(
            std::fs::read(root.join("frozen.bin")).unwrap(),
            b"original must survive"
        );
        assert!(relative(&parent.file, "..", true, None).is_err());
        drop(parent);
        std::fs::remove_file(root.join("frozen.bin")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn bootstrap_native_input_pins_reject_writes_and_unsafe_input_paths() {
        let root = std::env::temp_dir().join(format!(
            "catdesk-bootstrap-input-{}-{}",
            std::process::id(),
            crate::store::now()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("approved.json");
        std::fs::write(&path, b"frozen").unwrap();
        let pin = Input::open(&path, 16).unwrap();
        assert_eq!(pin.raw, b"frozen");
        assert!(std::fs::write(&path, b"changed").is_err());
        pin.revalidate().unwrap();
        assert!(Input::open(Path::new(r"\\server\share\file"), 16).is_err());
        assert!(Input::open(Path::new(r"C:\folder\..\file"), 16).is_err());
        drop(pin);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
