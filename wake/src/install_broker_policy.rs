//! Dormant read-only loader for the separately installed Wake broker policy.
//!
//! Only the compiled `C:\ProgramData\CatDesk\WakeProtectedInstallV1` root and
//! fixed `broker-policy.json` name are accepted in production. The policy file
//! must be broker-only writable, canonical and bounded, and physically bound
//! to its root and own file object. Root and file handles stay pinned without
//! write/delete sharing for the value's lifetime; use sites recheck them.
//! This proves a snapshot of installed bytes, not that an independent offline
//! authority approved the bootstrap, nor that a service or worker token is
//! correctly provisioned. It grants no install or launch authority.

use crate::{
    Result,
    install_broker_approval::{
        BrokerObservedStateV1, ObjectIdentityV1, VerifiedApprovalV1, verify_approval,
    },
    install_broker_operator::{OperatorTokenEvidenceV1, inspect_connected_pipe_operator},
    install_broker_preflight::{
        FixedProtectedRootReadGuard, acquire_fixed_protected_root_for_reader,
        canonical_broker_policy_security, canonical_protected_security, protected_security_raw,
    },
};
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, c_void},
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
    },
    ptr,
    sync::Mutex,
};

pub const FIXED_BROKER_POLICY_FILE: &str = "broker-policy.json";
const POLICY_PURPOSE: &str = "catdesk-wake-install-broker-policy";
const SYSTEM_SID: &str = "S-1-5-18";
const MAX_POLICY_BYTES: usize = 4096;
const MAX_SID_BYTES: usize = 256;
const MAX_SID_CHARS: usize = 128;
const FILE_OPEN: u32 = 1;
const FILE_READ_DATA: u32 = 1;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const READ_CONTROL: u32 = 0x0002_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_READ: u32 = 1;
const FILE_NON_DIRECTORY_FILE: u32 = 0x40;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x20;
const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: *mut c_void,
    object_name: *mut UnicodeString,
    attributes: u32,
    security_descriptor: *mut c_void,
    security_quality_of_service: *mut c_void,
}

#[repr(C)]
struct IoStatusBlock {
    status: isize,
    information: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
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

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        handle: *mut *mut c_void,
        access: u32,
        attributes: *mut ObjectAttributes,
        status: *mut IoStatusBlock,
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
    fn lstrlenW(value: *const u16) -> i32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertStringSidToSidW(text: *const u16, sid: *mut *mut c_void) -> i32;
    fn ConvertSidToStringSidW(sid: *mut c_void, text: *mut *mut u16) -> i32;
    fn IsValidSid(sid: *mut c_void) -> i32;
    fn GetLengthSid(sid: *mut c_void) -> u32;
    fn LookupAccountSidW(
        system: *const u16,
        sid: *mut c_void,
        account: *mut u16,
        account_length: *mut u32,
        domain: *mut u16,
        domain_length: *mut u32,
        use_type: *mut u32,
    ) -> i32;
}

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0) };
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyWireV1 {
    schema_version: u32,
    purpose: String,
    protected_root: ObjectIdentityV1,
    policy_file: ObjectIdentityV1,
    service_sid: String,
    operator_sid: String,
    approval_public_key_hex: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExactSecurity {
    owner: String,
    sddl: String,
}

enum RootPin {
    Fixed(FixedProtectedRootReadGuard),
    #[cfg(test)]
    Fixture(OwnedHandle),
}

impl RootPin {
    fn handle(&self) -> &OwnedHandle {
        match self {
            Self::Fixed(guard) => guard.root(),
            #[cfg(test)]
            Self::Fixture(handle) => handle,
        }
    }
}

/// Opaque pinned policy. No caller path, SID, key or JSON field is accepted by
/// its public constructor. Keep this object alive while consuming its values.
pub struct BrokerProtectedPolicyV1 {
    root: RootPin,
    file: Mutex<File>,
    root_id: ObjectIdentityV1,
    file_id: ObjectIdentityV1,
    original_bytes: Vec<u8>,
    root_security: ExactSecurity,
    file_security: ExactSecurity,
    service_sid: String,
    operator_sid: String,
    operator_sid_bytes: Vec<u8>,
    approval_key: VerifyingKey,
}

/// Opaque proof that this envelope was verified by the pinned, independently
/// protected broker key. A standalone `VerifiedApprovalV1` from a caller key
/// cannot be passed to the production ledger or stage writer.
pub struct PolicyVerifiedApprovalV1<'p> {
    policy: &'p BrokerProtectedPolicyV1,
    verified: VerifiedApprovalV1,
    readback: BrokerProtectedPolicyReadbackV1,
}

impl<'p> PolicyVerifiedApprovalV1<'p> {
    pub(crate) fn verified(&self) -> &VerifiedApprovalV1 {
        &self.verified
    }

    pub(crate) fn policy(&self) -> &'p BrokerProtectedPolicyV1 {
        self.policy
    }

    pub(crate) fn readback_snapshot(&self) -> &BrokerProtectedPolicyReadbackV1 {
        &self.readback
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerProtectedPolicyReadbackV1 {
    pub schema_version: u32,
    pub protected_root: ObjectIdentityV1,
    pub policy_file: ObjectIdentityV1,
    pub policy_sha256: String,
    pub service_sid_sha256: String,
    pub operator_sid_sha256: String,
    pub approval_public_key_sha256: String,
    pub disposition: &'static str,
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain([0]).collect()
}

fn object_identity(info: &FileInfo) -> ObjectIdentityV1 {
    ObjectIdentityV1 {
        volume_serial: info.volume,
        file_index_high: info.index_high,
        file_index_low: info.index_low,
    }
}

fn valid_identity(identity: &ObjectIdentityV1) -> bool {
    identity.volume_serial != 0 && (identity.file_index_high != 0 || identity.file_index_low != 0)
}

fn validate_file_info(info: FileInfo, directory: bool) -> Result<FileInfo> {
    if info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (info.attributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || (!directory && info.links != 1)
        || info.volume == 0
        || (info.index_high == 0 && info.index_low == 0)
    {
        return Err("WAKE_BROKER_POLICY_OBJECT_INVALID".into());
    }
    Ok(info)
}

fn file_info(handle: *mut c_void, directory: bool) -> Result<FileInfo> {
    let mut info = FileInfo::default();
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        return Err("WAKE_BROKER_POLICY_OBJECT_INVALID".into());
    }
    validate_file_info(info, directory)
}

fn stable_file_info(before: &FileInfo, after: &FileInfo) -> bool {
    before.attributes == after.attributes
        && before.creation_low == after.creation_low
        && before.creation_high == after.creation_high
        && before.write_low == after.write_low
        && before.write_high == after.write_high
        && before.volume == after.volume
        && before.size_high == after.size_high
        && before.size_low == after.size_low
        && before.links == after.links
        && before.index_high == after.index_high
        && before.index_low == after.index_low
}

fn security(handle: *mut c_void) -> Result<ExactSecurity> {
    let (owner, sddl) = protected_security_raw(handle)?;
    Ok(ExactSecurity { owner, sddl })
}

fn assert_security(handle: *mut c_void, expected: &ExactSecurity) -> Result<()> {
    if security(handle)? != *expected {
        return Err("WAKE_BROKER_POLICY_ACL_MISMATCH".into());
    }
    Ok(())
}

fn open_policy_relative(root: &OwnedHandle) -> Result<OwnedHandle> {
    let mut units: Vec<u16> = FIXED_BROKER_POLICY_FILE.encode_utf16().collect();
    let mut name = UnicodeString {
        length: (units.len() * 2) as u16,
        maximum_length: (units.len() * 2) as u16,
        buffer: units.as_mut_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: root.as_raw_handle(),
        object_name: &mut name,
        attributes: 0x40,
        security_descriptor: ptr::null_mut(),
        security_quality_of_service: ptr::null_mut(),
    };
    let mut status = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut raw = ptr::null_mut();
    let result = unsafe {
        NtCreateFile(
            &mut raw,
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            &mut attributes,
            &mut status,
            ptr::null_mut(),
            0,
            FILE_SHARE_READ, // keep write/delete refused for the pin lifetime
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
            ptr::null_mut(),
            0,
        )
    };
    if result < 0 || raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err("WAKE_BROKER_POLICY_UNAVAILABLE".into());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    file_info(handle.as_raw_handle(), false)?;
    Ok(handle)
}

fn read_bounded(file: &mut File, expected: &ExactSecurity) -> Result<(Vec<u8>, FileInfo)> {
    let before = file_info(file.as_raw_handle(), false)?;
    assert_security(file.as_raw_handle(), expected)?;
    let length = (u64::from(before.size_high) << 32) | u64::from(before.size_low);
    if length == 0 || length > MAX_POLICY_BYTES as u64 {
        return Err("WAKE_BROKER_POLICY_SIZE_INVALID".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "WAKE_BROKER_POLICY_READ_FAILED")?;
    let mut bytes = Vec::new();
    file.take(MAX_POLICY_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WAKE_BROKER_POLICY_READ_FAILED")?;
    if bytes.len() as u64 != length {
        return Err("WAKE_BROKER_POLICY_OBJECT_CHANGED".into());
    }
    let after = file_info(file.as_raw_handle(), false)?;
    if !stable_file_info(&before, &after) || security(file.as_raw_handle())? != *expected {
        return Err("WAKE_BROKER_POLICY_OBJECT_CHANGED".into());
    }
    Ok((bytes, before))
}

fn decode_key(value: &str) -> Result<VerifyingKey> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("WAKE_BROKER_POLICY_KEY_INVALID".into());
    }
    let mut bytes = [0u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(pair).map_err(|_| "WAKE_BROKER_POLICY_KEY_INVALID")?;
        bytes[index] =
            u8::from_str_radix(text, 16).map_err(|_| "WAKE_BROKER_POLICY_KEY_INVALID")?;
    }
    let key = VerifyingKey::from_bytes(&bytes).map_err(|_| "WAKE_BROKER_POLICY_KEY_INVALID")?;
    if key.is_weak() {
        return Err("WAKE_BROKER_POLICY_KEY_INVALID".into());
    }
    Ok(key)
}

fn sid_text(sid: *mut c_void) -> Result<String> {
    if sid.is_null()
        || unsafe { IsValidSid(sid) } == 0
        || unsafe { GetLengthSid(sid) } == 0
        || unsafe { GetLengthSid(sid) } as usize > MAX_SID_BYTES
    {
        return Err("WAKE_BROKER_POLICY_OPERATOR_INVALID".into());
    }
    let mut raw = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut raw) } == 0 || raw.is_null() {
        return Err("WAKE_BROKER_POLICY_OPERATOR_INVALID".into());
    }
    let text = LocalAllocation(raw.cast());
    let length = unsafe { lstrlenW(text.0.cast()) };
    if length <= 0 || length as usize > MAX_SID_CHARS {
        return Err("WAKE_BROKER_POLICY_OPERATOR_INVALID".into());
    }
    let units = unsafe { std::slice::from_raw_parts(text.0.cast::<u16>(), length as usize) };
    String::from_utf16(units).map_err(|_| "WAKE_BROKER_POLICY_OPERATOR_INVALID".into())
}

fn user_sid_bytes(value: &str) -> Result<Vec<u8>> {
    if value.len() > MAX_SID_CHARS
        || !value.starts_with("S-1-")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'S' || byte == b'-')
    {
        return Err("WAKE_BROKER_POLICY_OPERATOR_INVALID".into());
    }
    let mut raw = ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(wide(value).as_ptr(), &mut raw) } == 0 || raw.is_null() {
        return Err("WAKE_BROKER_POLICY_OPERATOR_INVALID".into());
    }
    let sid = LocalAllocation(raw);
    if sid_text(sid.0)? != value || value == SYSTEM_SID {
        return Err("WAKE_BROKER_POLICY_OPERATOR_INVALID".into());
    }
    let length = unsafe { GetLengthSid(sid.0) } as usize;
    if length == 0 || length > MAX_SID_BYTES {
        return Err("WAKE_BROKER_POLICY_OPERATOR_INVALID".into());
    }
    let mut account_length = 0;
    let mut domain_length = 0;
    let mut use_type = 0;
    unsafe {
        LookupAccountSidW(
            ptr::null(),
            sid.0,
            ptr::null_mut(),
            &mut account_length,
            ptr::null_mut(),
            &mut domain_length,
            &mut use_type,
        )
    };
    if account_length == 0 || account_length > 256 || domain_length > 256 {
        return Err("WAKE_BROKER_POLICY_OPERATOR_INVALID".into());
    }
    let mut account = vec![0u16; account_length as usize];
    let mut domain = vec![0u16; domain_length.max(1) as usize];
    if unsafe {
        LookupAccountSidW(
            ptr::null(),
            sid.0,
            account.as_mut_ptr(),
            &mut account_length,
            domain.as_mut_ptr(),
            &mut domain_length,
            &mut use_type,
        )
    } == 0
        || use_type != 1
    // SidTypeUser, never a group or logon SID
    {
        return Err("WAKE_BROKER_POLICY_OPERATOR_INVALID".into());
    }
    let bytes = unsafe { std::slice::from_raw_parts(sid.0.cast::<u8>(), length) };
    Ok(bytes.to_vec())
}

fn parse_policy(
    bytes: &[u8],
    root_id: &ObjectIdentityV1,
    file_id: &ObjectIdentityV1,
    service_sid: &str,
) -> Result<(PolicyWireV1, VerifyingKey, Vec<u8>)> {
    let wire: PolicyWireV1 =
        serde_json::from_slice(bytes).map_err(|_| "WAKE_BROKER_POLICY_ENCODING_INVALID")?;
    if serde_json::to_vec(&wire).map_err(|_| "WAKE_BROKER_POLICY_ENCODING_INVALID")? != bytes {
        return Err("WAKE_BROKER_POLICY_ENCODING_INVALID".into());
    }
    if wire.schema_version != 1
        || wire.purpose != POLICY_PURPOSE
        || !valid_identity(&wire.protected_root)
        || !valid_identity(&wire.policy_file)
        || wire.protected_root != *root_id
        || wire.policy_file != *file_id
        || wire.policy_file.volume_serial != wire.protected_root.volume_serial
        || wire.service_sid != service_sid
        || !wire.service_sid.starts_with("S-1-5-80-")
    {
        return Err("WAKE_BROKER_POLICY_BINDING_INVALID".into());
    }
    let key = decode_key(&wire.approval_public_key_hex)?;
    let operator_sid_bytes = user_sid_bytes(&wire.operator_sid)?;
    Ok((wire, key, operator_sid_bytes))
}

fn load_pinned(
    root: RootPin,
    root_id: ObjectIdentityV1,
    service_sid: String,
    root_security: ExactSecurity,
    file_security: ExactSecurity,
) -> Result<BrokerProtectedPolicyV1> {
    let root_info = file_info(root.handle().as_raw_handle(), true)?;
    if object_identity(&root_info) != root_id {
        return Err("WAKE_BROKER_POLICY_ROOT_CHANGED".into());
    }
    assert_security(root.handle().as_raw_handle(), &root_security)?;
    let handle = open_policy_relative(root.handle())?;
    let mut file = File::from(handle);
    let (bytes, info) = read_bounded(&mut file, &file_security)?;
    let file_id = object_identity(&info);
    if file_id.volume_serial != root_id.volume_serial {
        return Err("WAKE_BROKER_POLICY_OBJECT_INVALID".into());
    }
    let (wire, approval_key, operator_sid_bytes) =
        parse_policy(&bytes, &root_id, &file_id, &service_sid)?;
    let value = BrokerProtectedPolicyV1 {
        root,
        file: Mutex::new(file),
        root_id,
        file_id,
        original_bytes: bytes,
        root_security,
        file_security,
        service_sid,
        operator_sid: wire.operator_sid,
        operator_sid_bytes,
        approval_key,
    };
    value.revalidate()?;
    Ok(value)
}

impl BrokerProtectedPolicyV1 {
    pub fn revalidate(&self) -> Result<()> {
        let root = self.root.handle();
        if object_identity(&file_info(root.as_raw_handle(), true)?) != self.root_id {
            return Err("WAKE_BROKER_POLICY_ROOT_CHANGED".into());
        }
        assert_security(root.as_raw_handle(), &self.root_security)?;
        let mut original = self
            .file
            .lock()
            .map_err(|_| "WAKE_BROKER_POLICY_READ_FAILED")?;
        let (bytes, info) = read_bounded(&mut original, &self.file_security)?;
        if bytes != self.original_bytes || object_identity(&info) != self.file_id {
            return Err("WAKE_BROKER_POLICY_OBJECT_CHANGED".into());
        }
        let reopened = open_policy_relative(root)?;
        let mut reopened = File::from(reopened);
        let (reopened_bytes, reopened_info) = read_bounded(&mut reopened, &self.file_security)?;
        if reopened_bytes != self.original_bytes
            || object_identity(&reopened_info) != self.file_id
            || object_identity(&file_info(root.as_raw_handle(), true)?) != self.root_id
            || security(root.as_raw_handle())? != self.root_security
        {
            return Err("WAKE_BROKER_POLICY_OBJECT_CHANGED".into());
        }
        Ok(())
    }

    pub fn readback(&self) -> Result<BrokerProtectedPolicyReadbackV1> {
        self.revalidate()?;
        Ok(BrokerProtectedPolicyReadbackV1 {
            schema_version: 1,
            protected_root: self.root_id.clone(),
            policy_file: self.file_id.clone(),
            policy_sha256: format!("{:x}", Sha256::digest(&self.original_bytes)),
            service_sid_sha256: format!("{:x}", Sha256::digest(self.service_sid.as_bytes())),
            operator_sid_sha256: format!("{:x}", Sha256::digest(&self.operator_sid_bytes)),
            approval_public_key_sha256: format!(
                "{:x}",
                Sha256::digest(self.approval_key.as_bytes())
            ),
            disposition: "PINNED_POLICY_READBACK_NOT_INSTALL_AUTHORITY",
        })
    }

    pub(crate) fn checked_endpoint_sids(&self) -> Result<(&str, &str)> {
        self.revalidate()?;
        Ok((&self.service_sid, &self.operator_sid))
    }

    /// Use only immediately after the final request `ReadFile` on this same
    /// connected pipe instance. This makes no policy I/O before Windows
    /// impersonates the sender of that last message. The caller must have
    /// revalidated this pinned policy before the read and revalidate again
    /// after admission, without another pipe read in between.
    pub(crate) fn inspect_connected_operator_immediately_after_read(
        &self,
        pipe: BorrowedHandle<'_>,
    ) -> Result<OperatorTokenEvidenceV1> {
        inspect_connected_pipe_operator(pipe, &self.operator_sid_bytes)
    }

    /// Verify only with the independently installed key read from this pinned
    /// policy. This does not independently prove CI, stage bytes or ledger.
    pub fn verify_approval(
        &self,
        envelope: &[u8],
        observed: &BrokerObservedStateV1,
    ) -> Result<VerifiedApprovalV1> {
        self.revalidate()?;
        let approval = verify_approval(envelope, &self.approval_key, observed);
        self.revalidate()?;
        approval
    }

    /// Production transaction proof. Keep this value and its pinned policy
    /// live throughout INTENT and stage creation; revalidation is repeated by
    /// each consumer before mutating state.
    pub fn verify_approval_for_transaction(
        &self,
        envelope: &[u8],
        observed: &BrokerObservedStateV1,
    ) -> Result<PolicyVerifiedApprovalV1<'_>> {
        let readback = self.readback()?;
        let verified = self.verify_approval(envelope, observed)?;
        if self.readback()? != readback
            || verified.verification_key_sha256() != readback.approval_public_key_sha256
        {
            return Err("WAKE_BROKER_POLICY_CHANGED".into());
        }
        Ok(PolicyVerifiedApprovalV1 {
            policy: self,
            verified,
            readback,
        })
    }
}

/// Zero-argument fixed-root loader. It performs no file or service mutation.
pub fn load_fixed_broker_policy_read_only() -> Result<BrokerProtectedPolicyV1> {
    let guard = acquire_fixed_protected_root_for_reader()?;
    let root_id = guard.root_identity().clone();
    let service_sid = guard.service_sid().to_owned();
    let root_security = ExactSecurity {
        owner: SYSTEM_SID.into(),
        sddl: canonical_protected_security(&service_sid, true)?,
    };
    let file_security = ExactSecurity {
        owner: SYSTEM_SID.into(),
        sddl: canonical_broker_policy_security(&service_sid)?,
    };
    load_pinned(
        RootPin::Fixed(guard),
        root_id,
        service_sid,
        root_security,
        file_security,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_broker_approval::tests as approval_test;
    use crate::install_broker_ledger::{
        BrokerLedgerGuard, InstallPhaseV1, LedgerExpectationV1, OriginalDesiredStateV1,
    };
    use ed25519_dalek::SigningKey;
    use std::{
        os::windows::io::AsHandle,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    const FILE_SHARE_WRITE: u32 = 2;
    const FILE_WRITE_DATA: u32 = 2;
    const DACL_SECURITY_INFORMATION: u32 = 4;
    const ERROR_ACCESS_DENIED: u32 = 5;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    static NEXT: AtomicU64 = AtomicU64::new(1);

    #[link(name = "kernel32")]
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
        fn GetCurrentProcess() -> *mut c_void;
        fn GetLastError() -> u32;
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
        fn GetTokenInformation(
            token: *mut c_void,
            class: u32,
            info: *mut c_void,
            length: u32,
            returned: *mut u32,
        ) -> i32;
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text: *const u16,
            revision: u32,
            descriptor: *mut *mut c_void,
            size: *mut u32,
        ) -> i32;
        fn SetFileSecurityW(path: *const u16, information: u32, descriptor: *mut c_void) -> i32;
    }

    fn set_file_dacl(path: &std::path::Path, dacl: &str) {
        let mut raw = ptr::null_mut();
        let mut size = 0;
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide(dacl).as_ptr(),
                    1,
                    &mut raw,
                    &mut size,
                )
            },
            0
        );
        assert!(!raw.is_null() && size > 0);
        let descriptor = LocalAllocation(raw);
        assert_ne!(
            unsafe {
                SetFileSecurityW(
                    wide(path.to_str().expect("test path UTF-8")).as_ptr(),
                    DACL_SECURITY_INFORMATION,
                    descriptor.0,
                )
            },
            0
        );
    }

    fn current_user_sid() -> String {
        #[repr(C)]
        struct SidAndAttributes {
            sid: *mut c_void,
            attributes: u32,
        }
        let mut raw = ptr::null_mut();
        assert_ne!(
            unsafe { OpenProcessToken(GetCurrentProcess(), 8, &mut raw) },
            0
        );
        let token = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut required = 0;
        unsafe { GetTokenInformation(token.as_raw_handle(), 1, ptr::null_mut(), 0, &mut required) };
        assert!((std::mem::size_of::<SidAndAttributes>() as u32..=512).contains(&required));
        let mut aligned = vec![0usize; (required as usize).div_ceil(std::mem::size_of::<usize>())];
        let mut returned = required;
        assert_ne!(
            unsafe {
                GetTokenInformation(
                    token.as_raw_handle(),
                    1,
                    aligned.as_mut_ptr().cast(),
                    required,
                    &mut returned,
                )
            },
            0
        );
        assert!(returned >= std::mem::size_of::<SidAndAttributes>() as u32);
        assert!(returned <= required);
        let entry = unsafe { ptr::read_unaligned(aligned.as_ptr().cast::<SidAndAttributes>()) };
        let start = aligned.as_ptr().addr();
        let end = start + returned as usize;
        let sid_start = entry.sid.addr();
        assert!(sid_start >= start && sid_start + 8 <= end);
        let sid_length = unsafe { GetLengthSid(entry.sid) } as usize;
        assert!(sid_length <= MAX_SID_BYTES && sid_start + sid_length <= end);
        sid_text(entry.sid).expect("current token user SID")
    }

    struct Fixture {
        path: PathBuf,
        root: OwnedHandle,
        root_id: ObjectIdentityV1,
        service_sid: String,
        root_security: ExactSecurity,
        file_security: ExactSecurity,
        wire: PolicyWireV1,
    }

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "catdesk-broker-policy-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).expect("isolated policy directory");
            std::fs::write(path.join(FIXED_BROKER_POLICY_FILE), b"initial")
                .expect("initial policy file");
            let raw = unsafe {
                CreateFileW(
                    wide(path.to_str().expect("test path UTF-8")).as_ptr(),
                    FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    ptr::null_mut(),
                )
            };
            assert!(!raw.is_null() && raw != INVALID_HANDLE_VALUE);
            let root = unsafe { OwnedHandle::from_raw_handle(raw) };
            let root_id = object_identity(&file_info(root.as_raw_handle(), true).unwrap());
            let file = open_policy_relative(&root).expect("policy pin");
            let file_id = object_identity(&file_info(file.as_raw_handle(), false).unwrap());
            let root_security = security(root.as_raw_handle()).expect("root ACL");
            let file_security = security(file.as_raw_handle()).expect("file ACL");
            drop(file);
            let service_sid = "S-1-5-80-1-2-3-4-5".to_owned();
            let key = SigningKey::from_bytes(&[7u8; 32]).verifying_key();
            let wire = PolicyWireV1 {
                schema_version: 1,
                purpose: POLICY_PURPOSE.into(),
                protected_root: root_id.clone(),
                policy_file: file_id,
                service_sid: service_sid.clone(),
                operator_sid: current_user_sid(),
                approval_public_key_hex: key
                    .as_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            };
            let fixture = Self {
                path,
                root,
                root_id,
                service_sid,
                root_security,
                file_security,
                wire,
            };
            fixture.write_wire(&fixture.wire);
            fixture
        }

        fn write_wire(&self, wire: &PolicyWireV1) {
            std::fs::write(
                self.path.join(FIXED_BROKER_POLICY_FILE),
                serde_json::to_vec(wire).expect("canonical wire"),
            )
            .expect("write fixture policy");
        }

        fn load(&self) -> Result<BrokerProtectedPolicyV1> {
            let root = self
                .root
                .as_handle()
                .try_clone_to_owned()
                .expect("root clone");
            load_pinned(
                RootPin::Fixture(root),
                self.root_id.clone(),
                self.service_sid.clone(),
                self.root_security.clone(),
                self.file_security.clone(),
            )
        }

        fn cleanup(self) {
            drop(self.root);
            std::fs::remove_file(self.path.join(FIXED_BROKER_POLICY_FILE))
                .expect("remove fixture policy");
            std::fs::remove_dir(self.path).expect("remove fixture root");
        }
    }

    #[test]
    fn exact_pinned_policy_loads_key_and_user_identity() {
        let fixture = Fixture::new();
        let policy = fixture.load().expect("valid pinned policy");
        let readback = policy.readback().expect("stable readback");
        assert_eq!(readback.protected_root, fixture.root_id);
        assert_eq!(readback.policy_file, fixture.wire.policy_file);
        assert_eq!(readback.policy_sha256.len(), 64);
        assert_eq!(
            readback.disposition,
            "PINNED_POLICY_READBACK_NOT_INSTALL_AUTHORITY"
        );
        assert_eq!(
            policy.checked_endpoint_sids().unwrap().1,
            fixture.wire.operator_sid
        );
        drop(policy);
        fixture.cleanup();
    }

    #[test]
    fn transaction_proof_uses_only_the_pinned_policy_key() {
        let fixture = Fixture::new();
        let policy = fixture.load().expect("pinned policy");
        let (mut payload, mut observed, key) = approval_test::fixture();
        payload.target_protected_root = fixture.root_id.clone();
        observed.target_protected_root = fixture.root_id.clone();
        let envelope = approval_test::envelope(&payload, &key);
        let approved = policy
            .verify_approval_for_transaction(&envelope, &observed)
            .expect("policy key verifies transaction");
        assert_eq!(
            approved.verified().payload().approval_id,
            payload.approval_id
        );
        assert_eq!(
            approved.policy().readback().unwrap().protected_root,
            fixture.root_id
        );
        let unrelated = SigningKey::from_bytes(&[8u8; 32]);
        let self_signed = approval_test::envelope(&payload, &unrelated);
        assert_eq!(
            policy
                .verify_approval_for_transaction(&self_signed, &observed)
                .err()
                .unwrap(),
            "WAKE_APPROVAL_SIGNATURE_INVALID"
        );
        drop(approved);
        drop(policy);
        fixture.cleanup();
    }

    #[test]
    fn transaction_observation_runs_with_global_lock_and_pinned_policy() {
        let fixture = Fixture::new();
        std::fs::write(fixture.path.join("install-ledger.lock"), b"").expect("fixture lock");
        let policy = fixture.load().expect("pinned policy");
        let (mut payload, mut observed, key) = approval_test::fixture();
        payload.target_protected_root = fixture.root_id.clone();
        observed.target_protected_root = fixture.root_id.clone();
        let expectation = LedgerExpectationV1 {
            now_utc_seconds: observed.now_utc_seconds,
            next_install_generation: payload.install_generation,
            predecessor_package_id: payload.predecessor_package_id.clone(),
            predecessor_pointer: payload.predecessor_pointer.clone(),
            protected_root: fixture.root_id.clone(),
            original_desired_state: OriginalDesiredStateV1::Running,
        };
        let envelope = approval_test::envelope(&payload, &key);
        let mut observed_under_lock = false;
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("global lock");
        let (lease, approved) = guard
            .accept_with_observation(&policy, &envelope, |root| {
                observed_under_lock = true;
                assert!(BrokerLedgerGuard::acquire(root).is_err());
                Ok((observed.clone(), expectation.clone()))
            })
            .expect("policy-bound intent");
        assert!(observed_under_lock);
        assert_eq!(lease.phase(), InstallPhaseV1::Intent);
        assert_eq!(approved.verified().signed_payload_sha256().len(), 64);
        drop(lease);
        drop(approved);
        drop(policy);
        std::fs::remove_file(
            fixture
                .path
                .join("install-transaction-00000000000000000002-intent.json"),
        )
        .expect("remove fixture intent");
        std::fs::remove_file(fixture.path.join("install-ledger.lock"))
            .expect("remove fixture lock");
        fixture.cleanup();
    }

    #[test]
    fn same_payload_rotated_policy_key_cannot_resume_original_intent() {
        let fixture = Fixture::new();
        std::fs::write(fixture.path.join("install-ledger.lock"), b"").expect("fixture lock");
        let first_policy = fixture.load().expect("first protected policy");
        let (mut payload, mut observed, first_key) = approval_test::fixture();
        payload.target_protected_root = fixture.root_id.clone();
        observed.target_protected_root = fixture.root_id.clone();
        let expectation = LedgerExpectationV1 {
            now_utc_seconds: observed.now_utc_seconds,
            next_install_generation: payload.install_generation,
            predecessor_package_id: payload.predecessor_package_id.clone(),
            predecessor_pointer: payload.predecessor_pointer.clone(),
            protected_root: fixture.root_id.clone(),
            original_desired_state: OriginalDesiredStateV1::Running,
        };
        let first_envelope = approval_test::envelope(&payload, &first_key);
        let (first_lease, first_approved) = BrokerLedgerGuard::acquire(&fixture.root)
            .expect("global lock")
            .accept_with_observation(&first_policy, &first_envelope, |_| {
                Ok((observed.clone(), expectation.clone()))
            })
            .expect("first protected intent");
        let first_payload_digest = first_approved.verified().signed_payload_sha256().to_owned();
        let first_envelope_digest = first_approved
            .verified()
            .signed_envelope_sha256()
            .to_owned();
        let first_policy_file = first_approved.readback_snapshot().policy_file.clone();
        drop(first_lease);
        drop(first_approved);
        drop(first_policy);

        let rotated_key = SigningKey::from_bytes(&[8u8; 32]);
        let mut rotated_wire = fixture.wire.clone();
        rotated_wire.approval_public_key_hex = rotated_key
            .verifying_key()
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        fixture.write_wire(&rotated_wire);
        let rotated_policy = fixture.load().expect("rotated protected policy");
        let rotated_envelope = approval_test::envelope(&payload, &rotated_key);
        let rotated_proof = rotated_policy
            .verify_approval_for_transaction(&rotated_envelope, &observed)
            .expect("new policy accepts its own signature");
        assert_eq!(
            rotated_proof.verified().signed_payload_sha256(),
            first_payload_digest
        );
        assert_ne!(
            rotated_proof.verified().signed_envelope_sha256(),
            first_envelope_digest
        );
        assert_eq!(
            rotated_proof.readback_snapshot().policy_file,
            first_policy_file
        );
        assert_eq!(
            BrokerLedgerGuard::acquire(&fixture.root)
                .expect("global lock")
                .accept_with_observation(&rotated_policy, &rotated_envelope, |_| {
                    Ok((observed.clone(), expectation.clone()))
                })
                .err()
                .unwrap(),
            "WAKE_LEDGER_APPROVAL_CONFLICT"
        );
        assert!(
            !fixture
                .path
                .join("install-transaction-00000000000000000002-staged.json")
                .exists()
        );
        drop(rotated_proof);
        drop(rotated_policy);
        std::fs::remove_file(
            fixture
                .path
                .join("install-transaction-00000000000000000002-intent.json"),
        )
        .expect("remove original intent");
        std::fs::remove_file(fixture.path.join("install-ledger.lock"))
            .expect("remove fixture lock");
        fixture.cleanup();
    }

    #[test]
    fn canonical_encoding_and_size_are_bounded() {
        let fixture = Fixture::new();
        let pretty = serde_json::to_vec_pretty(&fixture.wire).unwrap();
        std::fs::write(fixture.path.join(FIXED_BROKER_POLICY_FILE), pretty).unwrap();
        assert_eq!(
            fixture.load().err().unwrap(),
            "WAKE_BROKER_POLICY_ENCODING_INVALID"
        );
        let duplicate = serde_json::to_string(&fixture.wire).unwrap().replacen(
            "\"schemaVersion\":1,",
            "\"schemaVersion\":1,\"schemaVersion\":1,",
            1,
        );
        std::fs::write(fixture.path.join(FIXED_BROKER_POLICY_FILE), duplicate).unwrap();
        assert_eq!(
            fixture.load().err().unwrap(),
            "WAKE_BROKER_POLICY_ENCODING_INVALID"
        );
        let extra =
            serde_json::to_string(&fixture.wire)
                .unwrap()
                .replacen('}', ",\"extra\":true}", 1);
        std::fs::write(fixture.path.join(FIXED_BROKER_POLICY_FILE), extra).unwrap();
        assert_eq!(
            fixture.load().err().unwrap(),
            "WAKE_BROKER_POLICY_ENCODING_INVALID"
        );
        std::fs::write(
            fixture.path.join(FIXED_BROKER_POLICY_FILE),
            vec![b'x'; MAX_POLICY_BYTES + 1],
        )
        .unwrap();
        assert_eq!(
            fixture.load().err().unwrap(),
            "WAKE_BROKER_POLICY_SIZE_INVALID"
        );
        fixture.cleanup();
    }

    #[test]
    fn purpose_root_file_and_service_binding_fail_closed() {
        let fixture = Fixture::new();
        let mut wrong = fixture.wire.clone();
        wrong.purpose = "catdesk-main-image-policy".into();
        fixture.write_wire(&wrong);
        assert_eq!(
            fixture.load().err().unwrap(),
            "WAKE_BROKER_POLICY_BINDING_INVALID"
        );
        wrong = fixture.wire.clone();
        wrong.schema_version = 2;
        fixture.write_wire(&wrong);
        assert_eq!(
            fixture.load().err().unwrap(),
            "WAKE_BROKER_POLICY_BINDING_INVALID"
        );
        wrong = fixture.wire.clone();
        wrong.protected_root.file_index_low ^= 1;
        fixture.write_wire(&wrong);
        assert_eq!(
            fixture.load().err().unwrap(),
            "WAKE_BROKER_POLICY_BINDING_INVALID"
        );
        wrong = fixture.wire.clone();
        wrong.policy_file.file_index_low ^= 1;
        fixture.write_wire(&wrong);
        assert_eq!(
            fixture.load().err().unwrap(),
            "WAKE_BROKER_POLICY_BINDING_INVALID"
        );
        wrong = fixture.wire.clone();
        wrong.service_sid = "S-1-5-80-9-9-9-9-9".into();
        fixture.write_wire(&wrong);
        assert_eq!(
            fixture.load().err().unwrap(),
            "WAKE_BROKER_POLICY_BINDING_INVALID"
        );
        fixture.cleanup();
    }

    #[test]
    fn weak_key_and_group_operator_are_rejected() {
        let fixture = Fixture::new();
        let mut wrong = fixture.wire.clone();
        wrong.approval_public_key_hex = "00".repeat(32);
        fixture.write_wire(&wrong);
        assert_eq!(
            fixture.load().err().unwrap(),
            "WAKE_BROKER_POLICY_KEY_INVALID"
        );
        wrong = fixture.wire.clone();
        wrong.operator_sid = "S-1-5-32-545".into();
        fixture.write_wire(&wrong);
        assert_eq!(
            fixture.load().err().unwrap(),
            "WAKE_BROKER_POLICY_OPERATOR_INVALID"
        );
        fixture.cleanup();
    }

    #[test]
    fn hardlink_and_acl_mismatch_are_denied() {
        let fixture = Fixture::new();
        let extra = fixture.path.join("outside-policy-link.json");
        std::fs::hard_link(fixture.path.join(FIXED_BROKER_POLICY_FILE), &extra).unwrap();
        assert_eq!(
            fixture.load().err().unwrap(),
            "WAKE_BROKER_POLICY_OBJECT_INVALID"
        );
        std::fs::remove_file(extra).unwrap();
        let mut expected = fixture.file_security.clone();
        expected.owner = SYSTEM_SID.into();
        if expected == fixture.file_security {
            expected.sddl.push('X');
        }
        let root = fixture.root.as_handle().try_clone_to_owned().unwrap();
        assert_eq!(
            load_pinned(
                RootPin::Fixture(root),
                fixture.root_id.clone(),
                fixture.service_sid.clone(),
                fixture.root_security.clone(),
                expected,
            )
            .err()
            .unwrap(),
            "WAKE_BROKER_POLICY_ACL_MISMATCH"
        );
        fixture.cleanup();
    }

    #[test]
    fn reparse_metadata_is_never_accepted_as_policy_file() {
        let info = FileInfo {
            attributes: FILE_ATTRIBUTE_REPARSE_POINT,
            volume: 1,
            links: 1,
            index_low: 1,
            ..FileInfo::default()
        };
        assert_eq!(
            validate_file_info(info, false).unwrap_err(),
            "WAKE_BROKER_POLICY_OBJECT_INVALID"
        );
    }

    #[test]
    fn policy_pin_refuses_write_delete_and_rechecks_exact_bytes() {
        let fixture = Fixture::new();
        let policy = fixture.load().unwrap();
        let path = fixture.path.join(FIXED_BROKER_POLICY_FILE);
        assert!(std::fs::write(&path, b"replacement").is_err());
        assert!(std::fs::rename(&path, fixture.path.join("renamed.json")).is_err());
        policy.revalidate().unwrap();
        drop(policy);
        fixture.cleanup();
    }

    #[test]
    fn interactive_read_only_policy_dacl_allows_read_and_denies_write() {
        let fixture = Fixture::new();
        let path = fixture.path.join(FIXED_BROKER_POLICY_FILE);
        set_file_dacl(
            &path,
            &format!(
                "D:P(A;;FA;;;SY)(A;;FA;;;{})(A;;0x00120089;;;IU)",
                fixture.service_sid
            ),
        );
        let read = open_policy_relative(&fixture.root).expect("interactive policy read");
        drop(read);
        let raw = unsafe {
            CreateFileW(
                wide(path.to_str().expect("test path UTF-8")).as_ptr(),
                FILE_WRITE_DATA,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        assert_eq!(raw, INVALID_HANDLE_VALUE);
        assert_eq!(unsafe { GetLastError() }, ERROR_ACCESS_DENIED);
        set_file_dacl(
            &path,
            &format!("D:P(A;;FA;;;SY)(A;;FA;;;{})", current_user_sid()),
        );
        fixture.cleanup();
    }
}
