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
    install_broker_offline_pins::{OfflineMigrationPinsV2, full_identity_of_handle},
    install_broker_offline_v2::{
        FullFileIdentityV2, OfflineMigrationObservationV2, OfflinePairReadbackV2,
        verify_offline_pair_read_only,
    },
    install_broker_operator::{OperatorTokenEvidenceV1, inspect_connected_pipe_operator},
    install_broker_preflight::{
        FixedProtectedRootReadGuard, acquire_fixed_protected_root_for_reader,
        canonical_broker_policy_security, canonical_protected_security,
        inspect_fixed_broker_read_only, protected_security_raw,
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
    fn fixed_full_ids(&self) -> Result<(FullFileIdentityV2, FullFileIdentityV2)> {
        let root = full_identity_of_handle(self.root.handle().as_raw_handle())?;
        let file = self
            .file
            .lock()
            .map_err(|_| "WAKE_BROKER_POLICY_READ_FAILED")?;
        let policy = full_identity_of_handle(file.as_raw_handle())?;
        Ok((root, policy))
    }

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

    /// Compare two V2 signatures using the protected policy key, fixed service
    /// preflight and retained full FileIdInfo role pins. The old-store path
    /// selection and closed-inventory claims still need independent proof:
    /// this returns no approval, ledger lease, owner proof or install capability.
    pub fn inspect_offline_pair_read_only(
        &self,
        anchor_wire: &[u8],
        approval_wire: &[u8],
        observed: &OfflineMigrationObservationV2,
        pins: &OfflineMigrationPinsV2,
    ) -> Result<OfflinePairReadbackV2> {
        let before = self.readback()?;
        let fixed_full_ids = self.fixed_full_ids()?;
        let service = inspect_fixed_broker_read_only()?;
        pins.revalidate_matching(observed)?;
        if observed.target_protected_root.legacy != before.protected_root
            || observed.target_protected_root.full != fixed_full_ids.0
            || observed.target_protected_root.full != service.protected_root_full
            || observed.broker_policy_file.legacy != before.policy_file
            || observed.broker_policy_file.full != fixed_full_ids.1
            || observed.broker_policy_sha256 != before.policy_sha256
            || observed.broker_service_image.legacy != service.service_image
            || observed.broker_service_image.full != service.service_image_full
            || observed.broker_service_image_sha256 != service.service_image_sha256
            || service.protected_root != before.protected_root
        {
            return Err("WAKE_OFFLINE_PROTECTED_POLICY_MISMATCH".into());
        }
        let result = verify_offline_pair_read_only(
            anchor_wire,
            approval_wire,
            &self.approval_key,
            observed,
        )?;
        if self.readback()? != before
            || self.fixed_full_ids()? != fixed_full_ids
            || inspect_fixed_broker_read_only()? != service
            || result.verification_key_sha256 != before.approval_public_key_sha256
        {
            return Err("WAKE_OFFLINE_PROTECTED_POLICY_CHANGED".into());
        }
        pins.revalidate_matching(observed)?;
        Ok(result)
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
pub(crate) mod tests {
    use super::*;
    use crate::install_broker_approval::{
        ArtifactV1, BrokerObservedStateV1, tests as approval_test,
    };
    use crate::install_broker_candidate_stage::inspect_stage_fixture;
    use crate::install_broker_durability::{ParentFlushPoint, fail_next_flush};
    use crate::install_broker_ledger::{
        BrokerLedgerGuard, InstallPhaseV1, LedgerExpectationV1, OriginalDesiredStateV1,
        StageBindingV1,
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

    /// Build only an isolated test policy in an already pinned temporary root.
    /// The production loader still accepts only the compiled protected path.
    pub(crate) fn load_fixture_policy_for_existing_root(
        path: &std::path::Path,
        root: &OwnedHandle,
        root_id: &ObjectIdentityV1,
    ) -> Result<BrokerProtectedPolicyV1> {
        let policy_path = path.join(FIXED_BROKER_POLICY_FILE);
        std::fs::write(&policy_path, b"initial").expect("initial fixture policy");
        let file = open_policy_relative(root)?;
        let file_id = object_identity(&file_info(file.as_raw_handle(), false)?);
        let root_security = security(root.as_raw_handle())?;
        let file_security = security(file.as_raw_handle())?;
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
        std::fs::write(
            &policy_path,
            serde_json::to_vec(&wire).expect("policy wire"),
        )
        .expect("canonical fixture policy");
        let pinned_root = root
            .as_handle()
            .try_clone_to_owned()
            .expect("fixture root clone");
        load_pinned(
            RootPin::Fixture(pinned_root),
            root_id.clone(),
            service_sid,
            root_security,
            file_security,
        )
    }

    const RESUME_ARTIFACTS: [&str; 4] = [
        "CatDeskWakeHost.exe",
        "CatDeskBinagotchy.exe",
        "adapter.py",
        "wake_bridge.py",
    ];
    const RESUME_BLOBS: [&[u8]; 4] = [
        b"host-binary",
        b"gui-binary",
        b"adapter-code",
        b"bridge-code",
    ];

    fn resume_fixture() -> (
        Fixture,
        BrokerProtectedPolicyV1,
        Vec<u8>,
        BrokerObservedStateV1,
        LedgerExpectationV1,
        Vec<u8>,
    ) {
        let fixture = Fixture::new();
        std::fs::write(fixture.path.join("install-ledger.lock"), b"").expect("global lock");
        std::fs::create_dir(fixture.path.join("staging")).expect("staging directory");
        std::fs::create_dir(fixture.path.join("packages")).expect("package directory");
        let policy = fixture.load().expect("pinned fixture policy");
        let (mut payload, mut observed, key) = approval_test::fixture();
        payload.target_protected_root = fixture.root_id.clone();
        observed.target_protected_root = fixture.root_id.clone();
        payload.manifest.artifacts = std::array::from_fn(|index| ArtifactV1 {
            name: RESUME_ARTIFACTS[index].into(),
            byte_length: RESUME_BLOBS[index].len() as u64,
            sha256: format!("{:x}", Sha256::digest(RESUME_BLOBS[index])),
        });
        payload.manifest.package_id = format!(
            "{}-{}-{}",
            payload.manifest.version,
            &payload.manifest.artifacts[0].sha256[..12],
            &payload.manifest.artifacts[1].sha256[..12]
        );
        let manifest = serde_json::to_vec(&payload.manifest).expect("canonical manifest");
        payload.manifest_sha256 = format!("{:x}", Sha256::digest(&manifest));
        let envelope = approval_test::envelope(&payload, &key);
        let expected = LedgerExpectationV1 {
            now_utc_seconds: observed.now_utc_seconds,
            next_install_generation: payload.install_generation,
            predecessor_package_id: payload.predecessor_package_id,
            predecessor_pointer: payload.predecessor_pointer,
            protected_root: fixture.root_id.clone(),
            original_desired_state: OriginalDesiredStateV1::Running,
        };
        (fixture, policy, envelope, observed, expected, manifest)
    }

    fn cleanup_resume_fixture(fixture: Fixture, policy: BrokerProtectedPolicyV1) {
        drop(policy);
        assert!(fixture.path.starts_with(std::env::temp_dir()));
        let reopened_raw = unsafe {
            CreateFileW(
                wide(fixture.path.to_str().expect("test path UTF-8")).as_ptr(),
                FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        assert!(!reopened_raw.is_null() && reopened_raw != INVALID_HANDLE_VALUE);
        let reopened = unsafe { OwnedHandle::from_raw_handle(reopened_raw) };
        assert_eq!(
            object_identity(&file_info(reopened.as_raw_handle(), true).expect("fixture root")),
            fixture.root_id
        );
        assert_eq!(
            object_identity(&file_info(fixture.root.as_raw_handle(), true).expect("pinned root")),
            fixture.root_id
        );
        drop(reopened);
        for phase in ["intent", "stage-started", "staged", "publication-attempt"] {
            let path = fixture.path.join(format!(
                "install-transaction-00000000000000000002-{phase}.json"
            ));
            if path.exists() {
                std::fs::remove_file(path).expect("remove exact fixture record");
            }
        }
        let staging = fixture.path.join("staging");
        let candidate = staging.join("candidate");
        if candidate.exists() {
            for name in [
                "manifest.json",
                RESUME_ARTIFACTS[0],
                RESUME_ARTIFACTS[1],
                RESUME_ARTIFACTS[2],
                RESUME_ARTIFACTS[3],
            ] {
                let path = candidate.join(name);
                if path.exists() {
                    std::fs::remove_file(path).expect("remove exact fixture stage leaf");
                }
            }
            std::fs::remove_dir(candidate).expect("remove empty fixture candidate");
        }
        std::fs::remove_dir(staging).expect("remove empty fixture staging");
        std::fs::remove_dir(fixture.path.join("packages"))
            .expect("remove empty fixture package directory");
        std::fs::remove_file(fixture.path.join("install-ledger.lock"))
            .expect("remove fixture lock");
        fixture.cleanup();
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
    fn offline_full_ids_come_from_the_fixed_policy_handles() {
        let fixture = Fixture::new();
        let policy = fixture.load().expect("pinned policy");
        let (root_full, policy_full) = policy.fixed_full_ids().expect("full fixed IDs");
        assert_eq!(
            root_full,
            full_identity_of_handle(fixture.root.as_raw_handle()).expect("fixture root full ID")
        );
        let policy_file = open_policy_relative(&fixture.root).expect("fixed policy leaf");
        assert_eq!(
            policy_full,
            full_identity_of_handle(policy_file.as_raw_handle()).expect("policy full ID")
        );
        assert_ne!(root_full, policy_full);
        drop(policy_file);
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
            .accept_with_observation_fixture(&policy, &envelope, |root| {
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
    fn exact_ci_failure_does_not_create_intent() {
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
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("global lock");
        let error = guard
            .accept_with_observation_ci_rejection_fixture(&policy, &envelope, |_| {
                Ok((observed, expectation))
            })
            .err()
            .expect("CI failure");
        assert_eq!(error, "WAKE_BROKER_CI_FIXTURE_DENIED");
        assert!(
            !fixture
                .path
                .join("install-transaction-00000000000000000002-intent.json")
                .exists()
        );
        drop(policy);
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
            .accept_with_observation_fixture(&first_policy, &first_envelope, |_| {
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
                .accept_with_observation_fixture(&rotated_policy, &rotated_envelope, |_| {
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
    fn guarded_resume_is_existing_only_policy_bound_and_checks_current_observation() {
        let (fixture, policy, envelope, observed, expected, _) = resume_fixture();
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("global lock");
        assert_eq!(
            guard
                .inspect_exact_resume_fixture(&policy, &envelope, 2, |_| {
                    panic!("missing intent must not invoke observer")
                })
                .unwrap_err(),
            "WAKE_LEDGER_NO_PRIOR_INTENT"
        );
        drop(guard);
        assert!(
            !fixture
                .path
                .join("install-transaction-00000000000000000002-intent.json")
                .exists()
        );

        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("acceptance lock");
        let (lease, approved) = guard
            .accept_with_observation_fixture(&policy, &envelope, |_| {
                Ok((observed.clone(), expected.clone()))
            })
            .expect("original policy-bound INTENT");
        drop(lease);
        drop(approved);

        let mut later = observed.clone();
        later.now_utc_seconds = 2_001; // Original approval is expired now.
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("resume lock");
        let diagnosis = guard
            .inspect_exact_resume_fixture(&policy, &envelope, 2, |root| {
                assert!(BrokerLedgerGuard::acquire(root).is_err());
                Ok((later.clone(), OriginalDesiredStateV1::Running))
            })
            .expect("exact expired approval readback");
        assert_eq!(diagnosis.phase, "intent");
        assert!(diagnosis.stage.is_none());
        assert!(!diagnosis.ledger_stage_binding_matched);
        assert_eq!(
            diagnosis.disposition,
            "INTENT_STAGING_EMPTY_NO_INSTALL_AUTHORITY"
        );
        assert!(
            !fixture
                .path
                .join("install-transaction-00000000000000000002-stage-started.json")
                .exists()
        );
        let future = fixture
            .path
            .join("install-transaction-00000000000000000003-intent.json");
        std::fs::write(&future, b"future fixture").expect("later-generation residue");
        assert_eq!(
            guard
                .inspect_exact_resume_fixture(&policy, &envelope, 2, |_| {
                    panic!("later generation must stop before observation")
                })
                .unwrap_err(),
            "WAKE_LEDGER_STALE_GENERATION"
        );
        std::fs::remove_file(future).expect("remove exact later-generation fixture");
        let orphan = fixture.path.join("staging").join("candidate");
        std::fs::create_dir(&orphan).expect("orphan candidate fixture");
        assert!(
            guard
                .inspect_exact_resume_fixture(&policy, &envelope, 2, |_| {
                    Ok((later.clone(), OriginalDesiredStateV1::Running))
                })
                .is_err()
        );
        std::fs::remove_dir(orphan).expect("remove empty orphan candidate");

        let mut drifted = later.clone();
        drifted.predecessor_pointer.file_index_low += 1;
        assert_eq!(
            guard
                .inspect_exact_resume_fixture(&policy, &envelope, 2, |_| {
                    Ok((drifted, OriginalDesiredStateV1::Running))
                })
                .unwrap_err(),
            "WAKE_LEDGER_RESUME_PHYSICAL_MISMATCH"
        );
        assert_eq!(
            guard
                .inspect_exact_resume_fixture(&policy, &envelope, 2, |_| {
                    Ok((later.clone(), OriginalDesiredStateV1::Paused))
                })
                .unwrap_err(),
            "WAKE_LEDGER_RESUME_PHYSICAL_MISMATCH"
        );
        let mut rollback = later.clone();
        rollback.now_utc_seconds = observed.now_utc_seconds - 1;
        assert_eq!(
            guard
                .inspect_exact_resume_fixture(&policy, &envelope, 2, |_| {
                    Ok((rollback, OriginalDesiredStateV1::Running))
                })
                .unwrap_err(),
            "WAKE_LEDGER_CLOCK_ROLLBACK"
        );
        let other_key = SigningKey::from_bytes(&[8u8; 32]);
        let (mut same_payload, _, _) = approval_test::fixture();
        same_payload.target_protected_root = fixture.root_id.clone();
        // A different valid signature for a different key cannot impersonate
        // the original protected policy, regardless of payload shape.
        let other_envelope = approval_test::envelope(&same_payload, &other_key);
        assert_eq!(
            guard
                .inspect_exact_resume_fixture(&policy, &other_envelope, 2, |_| {
                    panic!("wrong-key approval must stop before observation")
                })
                .unwrap_err(),
            "WAKE_APPROVAL_SIGNATURE_INVALID"
        );
        drop(guard);
        cleanup_resume_fixture(fixture, policy);
    }

    #[test]
    fn guarded_resume_rejects_same_payload_after_policy_key_rotation() {
        let (fixture, policy, envelope, observed, expected, _) = resume_fixture();
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("acceptance lock");
        let (lease, approved) = guard
            .accept_with_observation_fixture(&policy, &envelope, |_| {
                Ok((observed.clone(), expected.clone()))
            })
            .expect("original INTENT");
        drop(lease);
        drop(approved);
        drop(policy);

        let parsed: serde_json::Value = serde_json::from_slice(&envelope).expect("original wire");
        let payload: crate::install_broker_approval::WakeInstallApprovalV1 =
            serde_json::from_value(parsed["payload"].clone()).expect("same payload");
        let rotated_key = SigningKey::from_bytes(&[8u8; 32]);
        let rotated_envelope = approval_test::envelope(&payload, &rotated_key);
        let mut rotated_wire = fixture.wire.clone();
        rotated_wire.approval_public_key_hex = rotated_key
            .verifying_key()
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        fixture.write_wire(&rotated_wire);
        let rotated_policy = fixture.load().expect("new pinned policy snapshot");
        let mut later = observed;
        later.now_utc_seconds = 2_001;
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("resume lock");
        assert_eq!(
            guard
                .inspect_exact_resume_fixture(&rotated_policy, &rotated_envelope, 2, |_| {
                    Ok((later, OriginalDesiredStateV1::Running))
                })
                .unwrap_err(),
            "WAKE_LEDGER_APPROVAL_CONFLICT"
        );
        drop(guard);
        cleanup_resume_fixture(fixture, rotated_policy);
    }

    #[test]
    fn guarded_resume_reports_stage_started_attention_and_matches_staged_ids() {
        let (fixture, policy, envelope, observed, expected, manifest) = resume_fixture();
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("acceptance lock");
        let (mut lease, approved) = guard
            .accept_with_observation_fixture(&policy, &envelope, |_| {
                Ok((observed.clone(), expected.clone()))
            })
            .expect("original INTENT");
        fail_next_flush(ParentFlushPoint::StageStarted);
        assert_eq!(
            lease.record_stage_started().unwrap_err(),
            "WAKE_BROKER_PARENT_FLUSH_FAILED"
        );
        drop(lease);
        drop(approved);

        let mut later = observed.clone();
        later.now_utc_seconds = 2_001;
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("marker resume lock");
        let attention = guard
            .inspect_exact_resume_fixture(&policy, &envelope, 2, |_| {
                Ok((later.clone(), OriginalDesiredStateV1::Running))
            })
            .expect("marker is diagnosable without candidate");
        assert_eq!(attention.phase, "stage-started");
        assert!(attention.stage.is_none());
        assert_eq!(
            attention.disposition,
            "STAGE_STARTED_ATTENTION_NO_INSTALL_AUTHORITY"
        );
        let partial = fixture.path.join("staging").join("candidate");
        std::fs::create_dir(&partial).expect("partial candidate fixture");
        std::fs::write(partial.join(RESUME_ARTIFACTS[0]), RESUME_BLOBS[0])
            .expect("first partial artifact");
        let still_attention = guard
            .inspect_exact_resume_fixture(&policy, &envelope, 2, |_| {
                Ok((later.clone(), OriginalDesiredStateV1::Running))
            })
            .expect("partial candidate remains attention");
        assert_eq!(still_attention.phase, "stage-started");
        assert_eq!(
            std::fs::read(partial.join(RESUME_ARTIFACTS[0])).expect("partial bytes retained"),
            RESUME_BLOBS[0]
        );
        std::fs::remove_file(partial.join(RESUME_ARTIFACTS[0]))
            .expect("remove exact partial artifact");
        std::fs::remove_dir(partial).expect("remove empty partial candidate");
        drop(guard);

        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("stage fixture lock");
        let (mut lease, approved) = guard
            .accept_with_observation_fixture(&policy, &envelope, |_| {
                Ok((observed.clone(), expected.clone()))
            })
            .expect("exact marker reopen");
        let candidate = fixture.path.join("staging").join("candidate");
        std::fs::create_dir(&candidate).expect("fixture candidate");
        for (name, bytes) in RESUME_ARTIFACTS.into_iter().zip(RESUME_BLOBS) {
            std::fs::write(candidate.join(name), bytes).expect("fixture artifact");
        }
        std::fs::write(candidate.join("manifest.json"), manifest).expect("fixture manifest");
        let snapshot = inspect_stage_fixture(&fixture.root, &fixture.root_id, approved.verified())
            .expect("exact fixture stage bytes");
        let binding = StageBindingV1 {
            staging_directory: snapshot.staging_directory,
            packages_directory: None,
            candidate_directory: snapshot.candidate_directory,
            manifest_file: snapshot.manifest_file,
            artifacts: std::array::from_fn(|index| snapshot.artifacts[index].object.clone()),
        };
        lease
            .record_staged_fixture(binding)
            .expect("test-only ledger STAGED");
        drop(lease);
        drop(approved);

        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("staged resume lock");
        let staged = guard
            .inspect_exact_resume_fixture(&policy, &envelope, 2, |_| {
                Ok((later.clone(), OriginalDesiredStateV1::Running))
            })
            .expect("exact physical STAGED readback");
        assert_eq!(staged.phase, "staged");
        assert!(staged.ledger_stage_binding_matched);
        assert!(!staged.packages_parent_object_verified);
        assert!(
            !staged
                .stage
                .expect("stage readback")
                .stage_creation_and_ledger_binding_proven
        );
        std::fs::write(candidate.join("adapter.py"), b"adapter-codz")
            .expect("same-size tamper fixture");
        assert!(
            guard
                .inspect_exact_resume_fixture(&policy, &envelope, 2, |_| {
                    Ok((later.clone(), OriginalDesiredStateV1::Running))
                })
                .is_err()
        );
        std::fs::write(candidate.join("adapter.py"), RESUME_BLOBS[2])
            .expect("restore fixture bytes");
        std::fs::write(candidate.join("unexpected"), b"x").expect("foreign entry");
        assert!(
            guard
                .inspect_exact_resume_fixture(&policy, &envelope, 2, |_| {
                    Ok((later.clone(), OriginalDesiredStateV1::Running))
                })
                .is_err()
        );
        std::fs::remove_file(candidate.join("unexpected")).expect("remove foreign fixture entry");
        // Replace one file with identical signed bytes while the old physical
        // object still exists under a different fixed fixture name.
        let displaced = candidate.join("displaced-adapter.py");
        std::fs::rename(candidate.join("adapter.py"), &displaced)
            .expect("displace one fixture file");
        std::fs::write(candidate.join("adapter.py"), RESUME_BLOBS[2])
            .expect("replacement artifact bytes");
        std::fs::remove_file(displaced).expect("remove displaced fixture file");
        assert_eq!(
            guard
                .inspect_exact_resume_fixture(&policy, &envelope, 2, |_| {
                    Ok((later.clone(), OriginalDesiredStateV1::Running))
                })
                .unwrap_err(),
            "WAKE_LEDGER_STAGE_BINDING_INVALID"
        );
        drop(guard);
        cleanup_resume_fixture(fixture, policy);
    }

    #[test]
    fn guarded_publication_seam_distinguishes_exact_move_and_ambiguous_restart() {
        use crate::install_broker_reader::{
            pin_published_package_fixture, publication_fixture_policy_before_rename,
        };

        let (fixture, policy, envelope, observed, expected, manifest) = resume_fixture();
        let candidate = fixture.path.join("staging").join("candidate");
        let pointer = fixture.path.join("current.json");
        std::fs::write(&pointer, b"unchanged predecessor pointer")
            .expect("fixture predecessor pointer");
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("fixture ledger lock");
        let (mut lease, approved) = guard
            .accept_with_observation_fixture(&policy, &envelope, |_| {
                Ok((observed.clone(), expected.clone()))
            })
            .expect("fixture INTENT");
        let package = fixture
            .path
            .join("packages")
            .join(&approved.verified().payload().manifest.package_id);
        let fixture_root = fixture.path.canonicalize().expect("fixture root path");
        let fixture_staging = fixture
            .path
            .join("staging")
            .canonicalize()
            .expect("fixture staging parent path");
        let fixture_packages = fixture
            .path
            .join("packages")
            .canonicalize()
            .expect("fixture packages parent path");
        let exact_package_path = fixture_packages.join(
            package
                .file_name()
                .expect("signed fixture package component"),
        );
        let checked_rename =
            |source: &std::path::Path,
             destination: &std::path::Path,
             expected_source_parent: &std::path::Path,
             expected_destination_parent: &std::path::Path| {
                assert_eq!(
                    source
                        .parent()
                        .expect("rename source parent")
                        .canonicalize()
                        .expect("resolved rename source parent"),
                    expected_source_parent
                );
                assert_eq!(
                    destination
                        .parent()
                        .expect("rename destination parent")
                        .canonicalize()
                        .expect("resolved rename destination parent"),
                    expected_destination_parent
                );
                std::fs::rename(source, destination).expect("contained fixture rename");
            };
        let remove_exact_package = |path: &std::path::Path| {
            assert_eq!(
                path.parent()
                    .expect("package parent")
                    .canonicalize()
                    .expect("resolved package parent"),
                fixture_packages
            );
            assert_eq!(path, package);
            for name in [
                "manifest.json",
                RESUME_ARTIFACTS[0],
                RESUME_ARTIFACTS[1],
                RESUME_ARTIFACTS[2],
                RESUME_ARTIFACTS[3],
            ] {
                // Refuse a junction or symlink substituted at the package
                // name before each leaf deletion. The resolved directory
                // must remain the fixture's exact final package directory.
                assert_eq!(
                    path.canonicalize().expect("resolved fixture package"),
                    exact_package_path
                );
                std::fs::remove_file(path.join(name)).expect("remove exact fixture package leaf");
            }
            assert_eq!(
                path.canonicalize().expect("resolved fixture package"),
                exact_package_path
            );
            std::fs::remove_dir(path).expect("remove empty fixture package directory");
        };
        lease.record_stage_started().expect("stage marker");
        std::fs::create_dir(&candidate).expect("candidate slot");
        for (name, bytes) in RESUME_ARTIFACTS.into_iter().zip(RESUME_BLOBS) {
            std::fs::write(candidate.join(name), bytes).expect("signed fixture artifact");
        }
        std::fs::write(candidate.join("manifest.json"), manifest)
            .expect("canonical fixture manifest");
        let snapshot = inspect_stage_fixture(&fixture.root, &fixture.root_id, approved.verified())
            .expect("exact stage readback");
        let packages_path = fixture.path.join("packages");
        let packages_raw = unsafe {
            CreateFileW(
                wide(packages_path.to_str().expect("fixture packages UTF-8")).as_ptr(),
                FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        assert!(!packages_raw.is_null() && packages_raw != INVALID_HANDLE_VALUE);
        let packages_pin = unsafe { OwnedHandle::from_raw_handle(packages_raw) };
        let packages_id = object_identity(
            &file_info(packages_pin.as_raw_handle(), true).expect("packages parent identity"),
        );
        drop(packages_pin);
        let stage = StageBindingV1 {
            staging_directory: snapshot.staging_directory,
            packages_directory: Some(packages_id),
            candidate_directory: snapshot.candidate_directory,
            manifest_file: snapshot.manifest_file,
            artifacts: std::array::from_fn(|index| snapshot.artifacts[index].object.clone()),
        };
        let publication_policy =
            publication_fixture_policy_before_rename(&fixture.root, approved.verified(), &stage)
                .expect("stage ACL and package-parent fixture policy");
        let verified = approved.verified().clone();
        let stage_for_pin = stage.clone();
        lease
            .record_staged_fixture(stage)
            .expect("durable fixture STAGED");
        drop(lease);
        drop(approved);

        let ledger_before: Vec<Vec<u8>> = ["intent", "stage-started", "staged"]
            .into_iter()
            .map(|phase| {
                std::fs::read(fixture.path.join(format!(
                    "install-transaction-00000000000000000002-{phase}.json"
                )))
                .expect("fixture ledger bytes")
            })
            .collect();
        let pointer_before = std::fs::read(&pointer).expect("pointer bytes");
        let mut later = observed.clone();
        later.now_utc_seconds = 2_001;
        {
            let pin_moved = || {
                pin_published_package_fixture(
                    &fixture.root,
                    &fixture.root_id,
                    &verified,
                    &stage_for_pin,
                    &publication_policy,
                )
            };
            let inspect = || {
                let guard = BrokerLedgerGuard::acquire(&fixture.root)?;
                guard.inspect_publication_seam_fixture(
                    &policy,
                    &envelope,
                    2,
                    |_| Ok((later.clone(), OriginalDesiredStateV1::Running)),
                    &publication_policy,
                )
            };
            let staged = inspect().expect("intact stage diagnosis");
            assert_eq!(staged.phase, "staged");
            assert!(staged.ledger_stage_binding_matched);
            assert!(!staged.published_package_binding_matched);
            assert!(staged.staging_parent_object_verified);
            assert!(staged.packages_parent_object_verified);
            assert!(staged.stage.is_some());

            // Model a physical rename that completed before the PUBLISHED
            // record. This fixture operation is never a production writer.
            checked_rename(&candidate, &package, &fixture_staging, &fixture_packages);
            let moved = inspect().expect("exact orphan package diagnosis");
            assert_eq!(moved.phase, "staged");
            assert!(moved.stage.is_none());
            assert!(!moved.ledger_stage_binding_matched);
            assert!(moved.published_package_binding_matched);
            assert!(moved.staging_parent_object_verified);
            assert!(moved.packages_parent_object_verified);
            assert_eq!(
                moved.disposition,
                "STAGED_PACKAGE_MOVED_PUBLISHED_RECORD_MISSING_ATTENTION_NO_INSTALL_AUTHORITY"
            );
            let pin = pin_moved().expect("retained exact moved package pin");
            assert_eq!(
                pin.snapshot().package_directory,
                stage_for_pin.candidate_directory
            );
            assert!(std::fs::write(package.join("adapter.py"), b"adapter-codz").is_err());
            let blocked = fixture.path.join("packages").join("blocked-fixture");
            assert_eq!(
                blocked
                    .parent()
                    .expect("blocked rename parent")
                    .canonicalize()
                    .expect("resolved blocked rename parent"),
                fixture_packages
            );
            assert_eq!(
                package
                    .parent()
                    .expect("package rename parent")
                    .canonicalize()
                    .expect("resolved package rename parent"),
                fixture_packages
            );
            assert!(std::fs::rename(&package, &blocked).is_err());
            pin.revalidate(&fixture.root, &fixture.root_id)
                .expect("reopened exact package while pins retained");
            drop(pin);
            assert!(
                !fixture
                    .path
                    .join("install-transaction-00000000000000000002-published.json")
                    .exists()
            );

            std::fs::create_dir(&candidate).expect("ambiguous second slot");
            assert_eq!(
                inspect().unwrap_err(),
                "WAKE_PROTECTED_PUBLICATION_LOCATION_INVALID"
            );
            assert!(pin_moved().is_err());
            std::fs::remove_dir(&candidate).expect("remove exact extra slot");

            let displaced = fixture.path.join("packages").join("displaced-fixture");
            checked_rename(&package, &displaced, &fixture_packages, &fixture_packages);
            assert_eq!(
                inspect().unwrap_err(),
                "WAKE_PROTECTED_PUBLICATION_LOCATION_INVALID"
            );
            checked_rename(&displaced, &package, &fixture_packages, &fixture_packages);

            let artifact = package.join("adapter.py");
            std::fs::write(&artifact, b"adapter-codz").expect("same-size drift");
            assert_eq!(
                inspect().unwrap_err(),
                "WAKE_PROTECTED_READER_HASH_MISMATCH"
            );
            assert!(pin_moved().is_err());
            std::fs::write(&artifact, RESUME_BLOBS[2]).expect("restore signed bytes");
            std::fs::write(package.join("unexpected"), b"x").expect("extra package entry");
            assert_eq!(
                inspect().unwrap_err(),
                "WAKE_PROTECTED_READER_INVENTORY_MISMATCH"
            );
            std::fs::remove_file(package.join("unexpected")).expect("remove extra entry");

            checked_rename(&package, &displaced, &fixture_packages, &fixture_packages);
            std::fs::create_dir(&package).expect("same-byte replacement directory");
            for name in [
                "manifest.json",
                RESUME_ARTIFACTS[0],
                RESUME_ARTIFACTS[1],
                RESUME_ARTIFACTS[2],
                RESUME_ARTIFACTS[3],
            ] {
                std::fs::copy(displaced.join(name), package.join(name)).expect("same-byte copy");
            }
            assert_eq!(
                inspect().unwrap_err(),
                "WAKE_PROTECTED_PUBLICATION_BINDING_INVALID"
            );
            assert!(pin_moved().is_err());
            remove_exact_package(&package);
            checked_rename(&displaced, &package, &fixture_packages, &fixture_packages);

            let saved_staging = fixture.path.join("staging-saved-fixture");
            checked_rename(
                &fixture.path.join("staging"),
                &saved_staging,
                &fixture_root,
                &fixture_root,
            );
            std::fs::create_dir(fixture.path.join("staging"))
                .expect("same-name replacement staging parent");
            assert_eq!(
                inspect().unwrap_err(),
                "WAKE_PROTECTED_PUBLICATION_BINDING_INVALID"
            );
            assert!(pin_moved().is_err());
            std::fs::remove_dir(fixture.path.join("staging"))
                .expect("remove replacement staging parent");
            checked_rename(
                &saved_staging,
                &fixture.path.join("staging"),
                &fixture_root,
                &fixture_root,
            );
            inspect().expect("original physical package restored");

            let saved_packages = fixture.path.join("packages-saved-fixture");
            checked_rename(
                &packages_path,
                &saved_packages,
                &fixture_root,
                &fixture_root,
            );
            std::fs::create_dir(&packages_path).expect("same-name packages parent replacement");
            let saved_packages_parent = saved_packages
                .canonicalize()
                .expect("resolved displaced packages parent");
            let saved_package = saved_packages.join(
                package
                    .file_name()
                    .expect("signed fixture package component"),
            );
            checked_rename(
                &saved_package,
                &package,
                &saved_packages_parent,
                &fixture_packages,
            );
            assert_eq!(
                inspect().unwrap_err(),
                "WAKE_PROTECTED_PUBLICATION_BINDING_INVALID"
            );
            assert!(pin_moved().is_err());
            checked_rename(
                &package,
                &saved_package,
                &fixture_packages,
                &saved_packages_parent,
            );
            std::fs::remove_dir(&packages_path).expect("remove empty replacement packages");
            checked_rename(
                &saved_packages,
                &packages_path,
                &fixture_root,
                &fixture_root,
            );
            inspect().expect("original packages parent restored");

            let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("fixture phase lock");
            let (mut lease, _approved) = guard
                .accept_with_observation_fixture(&policy, &envelope, |_| {
                    Ok((observed.clone(), expected.clone()))
                })
                .expect("exact fixture replay");
            checked_rename(&package, &candidate, &fixture_packages, &fixture_staging);
            lease
                .record_publication_attempt_fixture()
                .expect("fixture pre-rename attempt marker");
            checked_rename(&candidate, &package, &fixture_staging, &fixture_packages);
            lease
                .advance(InstallPhaseV1::Published)
                .expect("fixture PUBLISHED record");
            drop(lease);
            let published = inspect().expect("recorded published package diagnosis");
            assert_eq!(published.phase, "published");
            assert!(published.published_package_binding_matched);
            assert!(published.packages_parent_object_verified);
            assert!(published.publication_attempt_record_present);
            assert!(!published.ledger_stage_binding_matched);
            assert_eq!(
                published.disposition,
                "PUBLISHED_PACKAGE_IDS_MATCH_NO_INSTALL_AUTHORITY"
            );
            checked_rename(&package, &candidate, &fixture_packages, &fixture_staging);
            assert_eq!(
                inspect().unwrap_err(),
                "WAKE_BROKER_PUBLICATION_PHASE_MISMATCH"
            );
            assert!(pin_moved().is_err());
            checked_rename(&candidate, &package, &fixture_staging, &fixture_packages);
            inspect().expect("exact published location restored");
            let pin = pin_moved().expect("retain package for ACL drift check");
            set_file_dacl(&artifact, "D:P(A;;FA;;;SY)(A;;FA;;;WD)");
            assert_eq!(
                pin.revalidate(&fixture.root, &fixture.root_id).unwrap_err(),
                "WAKE_PROTECTED_READER_ACL_MISMATCH"
            );
            drop(pin);
            assert_eq!(inspect().unwrap_err(), "WAKE_PROTECTED_READER_ACL_MISMATCH");
            assert!(pin_moved().is_err());
        }
        let ledger_after: Vec<Vec<u8>> = ["intent", "stage-started", "staged"]
            .into_iter()
            .map(|phase| {
                std::fs::read(fixture.path.join(format!(
                    "install-transaction-00000000000000000002-{phase}.json"
                )))
                .expect("unchanged ledger bytes")
            })
            .collect();
        assert_eq!(ledger_after, ledger_before);
        assert_eq!(
            std::fs::read(&pointer).expect("unchanged pointer"),
            pointer_before
        );
        assert!(
            !fixture
                .path
                .join("install-transaction-00000000000000000003-intent.json")
                .exists()
        );
        std::fs::remove_file(pointer).expect("remove fixture pointer");
        std::fs::remove_file(
            fixture
                .path
                .join("install-transaction-00000000000000000002-published.json"),
        )
        .expect("remove fixture PUBLISHED record");
        remove_exact_package(&package);
        cleanup_resume_fixture(fixture, policy);
    }

    #[test]
    fn guarded_staged_attempt_marker_reports_attention_without_retry_authority() {
        use crate::install_broker_reader::publication_fixture_policy_before_rename;

        let (fixture, policy, envelope, observed, expected, manifest) = resume_fixture();
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("fixture ledger lock");
        let (mut lease, approved) = guard
            .accept_with_observation_fixture(&policy, &envelope, |_| {
                Ok((observed.clone(), expected.clone()))
            })
            .expect("fixture INTENT");
        lease.record_stage_started().expect("stage marker");
        let candidate = fixture.path.join("staging").join("candidate");
        std::fs::create_dir(&candidate).expect("fixed candidate slot");
        for (name, bytes) in RESUME_ARTIFACTS.into_iter().zip(RESUME_BLOBS) {
            std::fs::write(candidate.join(name), bytes).expect("signed fixture artifact");
        }
        std::fs::write(candidate.join("manifest.json"), manifest)
            .expect("canonical fixture manifest");
        let snapshot = inspect_stage_fixture(&fixture.root, &fixture.root_id, approved.verified())
            .expect("exact stage readback");
        let packages_path = fixture.path.join("packages");
        let packages_raw = unsafe {
            CreateFileW(
                wide(packages_path.to_str().expect("fixture packages UTF-8")).as_ptr(),
                FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        assert!(!packages_raw.is_null() && packages_raw != INVALID_HANDLE_VALUE);
        let packages_pin = unsafe { OwnedHandle::from_raw_handle(packages_raw) };
        let packages_id = object_identity(
            &file_info(packages_pin.as_raw_handle(), true).expect("packages parent identity"),
        );
        drop(packages_pin);
        let stage = StageBindingV1 {
            staging_directory: snapshot.staging_directory,
            candidate_directory: snapshot.candidate_directory,
            manifest_file: snapshot.manifest_file,
            artifacts: std::array::from_fn(|index| snapshot.artifacts[index].object.clone()),
            packages_directory: Some(packages_id),
        };
        let publication_policy =
            publication_fixture_policy_before_rename(&fixture.root, approved.verified(), &stage)
                .expect("fixed stage and packages policy");
        lease.record_staged_fixture(stage).expect("durable STAGED");
        lease
            .record_publication_attempt_fixture()
            .expect("durable publication attempt");
        drop(lease);
        drop(approved);

        let mut later = observed.clone();
        later.now_utc_seconds = 2_001;
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("diagnostic lock");
        let readback = guard
            .inspect_publication_seam_fixture(
                &policy,
                &envelope,
                2,
                |_| Ok((later, OriginalDesiredStateV1::Running)),
                &publication_policy,
            )
            .expect("marked STAGED diagnosis");
        assert_eq!(readback.phase, "staged");
        assert!(readback.ledger_stage_binding_matched);
        assert!(readback.packages_parent_object_verified);
        assert!(readback.publication_attempt_record_present);
        assert_eq!(
            readback.disposition,
            "STAGED_PUBLICATION_ATTEMPT_ATTENTION_NO_INSTALL_AUTHORITY"
        );
        drop(guard);
        let guard = BrokerLedgerGuard::acquire(&fixture.root).expect("retry lock");
        assert_eq!(
            guard
                .accept_with_observation_fixture(&policy, &envelope, |_| {
                    Ok((observed.clone(), expected.clone()))
                })
                .err()
                .unwrap(),
            "WAKE_BROKER_PUBLICATION_ATTEMPT_ATTENTION"
        );
        assert!(
            !fixture
                .path
                .join("install-transaction-00000000000000000002-published.json")
                .exists()
        );
        cleanup_resume_fixture(fixture, policy);
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
