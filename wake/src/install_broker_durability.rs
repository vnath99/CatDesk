//! Parent-directory metadata flush for the dormant protected install journal.
//!
//! File `sync_all` covers a record's bytes, not the parent entry naming it.
//! The service must still establish a protected local filesystem and validate
//! sudden-power-loss behavior before this source can gate an installation.

use crate::{Result, install_broker_approval::ObjectIdentityV1};
use std::{
    ffi::c_void,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};

const FILE_OPEN: u32 = 1;
const FILE_ADD_FILE: u32 = 2;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_READ: u32 = 1;
const FILE_SHARE_WRITE: u32 = 2;
const FILE_DIRECTORY_FILE: u32 = 1;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x20;
const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ParentFlushPoint {
    Intent,
    StageStarted,
    StageCandidateEntry,
    StageLeafEntries,
    Staged,
    PublicationAttempt,
    LaterLedgerPhase,
}

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
    fn NtFlushBuffersFileEx(
        handle: *mut c_void,
        flags: u32,
        parameters: *mut c_void,
        parameters_size: u32,
        status: *mut IoStatusBlock,
    ) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileInformationByHandle(handle: *mut c_void, info: *mut FileInfo) -> i32;
}

fn identity(handle: &OwnedHandle) -> Result<ObjectIdentityV1> {
    let mut info = FileInfo::default();
    if unsafe { GetFileInformationByHandle(handle.as_raw_handle(), &mut info) } == 0
        || info.attributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || info.volume == 0
        || (info.index_high == 0 && info.index_low == 0)
    {
        return Err("WAKE_BROKER_PARENT_DIRECTORY_CHANGED".into());
    }
    Ok(ObjectIdentityV1 {
        volume_serial: info.volume,
        file_index_high: info.index_high,
        file_index_low: info.index_low,
    })
}

fn reopen_self_for_flush(pinned: &OwnedHandle) -> Result<OwnedHandle> {
    // An empty relative name is the pinned directory itself, not a caller
    // pathname. Reopening obtains the write right needed for metadata flush
    // without widening the long-lived readback handle's rights.
    let mut name = UnicodeString {
        length: 0,
        maximum_length: 0,
        buffer: ptr::null_mut(),
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: pinned.as_raw_handle(),
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
            FILE_ADD_FILE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &mut attributes,
            &mut status,
            ptr::null_mut(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE, // deny delete/rename
            FILE_OPEN,
            FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
            ptr::null_mut(),
            0,
        )
    };
    if result != 0 || raw.is_null() || raw == INVALID_HANDLE_VALUE {
        if !raw.is_null() && raw != INVALID_HANDLE_VALUE {
            drop(unsafe { OwnedHandle::from_raw_handle(raw) });
        }
        return Err("WAKE_BROKER_PARENT_FLUSH_UNAVAILABLE".into());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

/// Synchronize the metadata of the exact pinned parent directory. An error is
/// a hard transaction stop, including unsupported filesystem/API behavior.
pub(crate) fn flush_pinned_directory(
    pinned: &OwnedHandle,
    expected: &ObjectIdentityV1,
    point: ParentFlushPoint,
) -> Result<()> {
    if identity(pinned)? != *expected {
        return Err("WAKE_BROKER_PARENT_DIRECTORY_CHANGED".into());
    }
    let write_handle = reopen_self_for_flush(pinned)?;
    if identity(&write_handle)? != *expected {
        return Err("WAKE_BROKER_PARENT_DIRECTORY_CHANGED".into());
    }
    #[cfg(test)]
    if FAIL_NEXT.with(|failure| {
        if failure.get() == Some(point) {
            failure.set(None);
            true
        } else {
            false
        }
    }) {
        return Err("WAKE_BROKER_PARENT_FLUSH_FAILED".into());
    }
    #[cfg(not(test))]
    let _ = point;
    let mut status = IoStatusBlock {
        status: 0,
        information: 0,
    };
    // Normal flush (flags 0) includes file metadata and storage cache sync.
    // FILE_DATA_SYNC_ONLY is invalid for directory handles.
    if unsafe {
        NtFlushBuffersFileEx(
            write_handle.as_raw_handle(),
            0,
            ptr::null_mut(),
            0,
            &mut status,
        )
    } != 0
        || status.status != 0
    {
        return Err("WAKE_BROKER_PARENT_FLUSH_FAILED".into());
    }
    if identity(pinned)? != *expected || identity(&write_handle)? != *expected {
        return Err("WAKE_BROKER_PARENT_DIRECTORY_CHANGED".into());
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    static FAIL_NEXT: std::cell::Cell<Option<ParentFlushPoint>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn fail_next_flush(point: ParentFlushPoint) {
    FAIL_NEXT.with(|failure| {
        assert!(
            failure.get().is_none(),
            "prior flush failure was not consumed"
        );
        failure.set(Some(point));
    });
}
