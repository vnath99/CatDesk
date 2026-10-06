//! Reusable, handle-bound Windows protected-filesystem authority.
//!
//! This module never resolves a child from a caller pathname.  A consumer
//! supplies an already-pinned parent handle through `PinnedParent`; all child
//! opens use that handle as `OBJECT_ATTRIBUTES.RootDirectory`.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use uuid::Uuid;

#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
#[cfg(windows)]
use std::os::windows::io::FromRawHandle;

/// Handle-pinned directory identity used by protected filesystem consumers.
pub(crate) struct PinnedDirectory {
    pub(crate) path: PathBuf,
    #[cfg(windows)]
    pub(crate) handle: *mut c_void,
    #[cfg(windows)]
    pub(crate) identity: (u32, u32, u32),
}

pub(crate) fn validate_protected_component(component: &str, label: &str) -> Result<(), String> {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.contains(['/', '\\', '\0'])
    {
        return Err(format!("{label} is unsafe"));
    }
    Ok(())
}

fn classify_pinned_directory(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| format!("{label} is unavailable"))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(format!("{label} is unsafe"));
    }
    #[cfg(windows)]
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!("{label} is unsafe"));
    }
    Ok(())
}

impl PinnedDirectory {
    pub(crate) fn acquire(path: &Path, label: &str) -> Result<Self, String> {
        Self::acquire_with_delete_share(path, label, false)
    }

    fn acquire_with_delete_share(
        path: &Path,
        label: &str,
        allow_delete_share: bool,
    ) -> Result<Self, String> {
        classify_pinned_directory(path, label)?;
        #[cfg(windows)]
        {
            let wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    FILE_LIST_DIRECTORY | FILE_ADD_FILE | FILE_ADD_SUBDIRECTORY,
                    FILE_SHARE_READ
                        | FILE_SHARE_WRITE
                        | if allow_delete_share {
                            FILE_SHARE_DELETE
                        } else {
                            0
                        },
                    std::ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    std::ptr::null_mut(),
                )
            };
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return Err(format!("{label} identity is unavailable"));
            }
            let information = match handle_information(handle, label) {
                Ok(information) => information,
                Err(_) => {
                    unsafe { CloseHandle(handle) };
                    return Err(format!("{label} is unsafe"));
                }
            };
            if information.file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                unsafe { CloseHandle(handle) };
                return Err(format!("{label} is unsafe"));
            }
            Ok(Self {
                path: path.to_path_buf(),
                handle,
                identity: (
                    information.volume_serial_number,
                    information.file_index_high,
                    information.file_index_low,
                ),
            })
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            Err(format!("{label} stable identity is unavailable"))
        }
    }

    pub(crate) fn open_relative(
        parent: &Self,
        component: &str,
        label: &str,
    ) -> Result<Self, String> {
        Self::open_relative_with_disposition(parent, component, FILE_OPEN, false, label)
    }

    pub(crate) fn open_optional_relative(
        parent: &Self,
        component: &str,
        label: &str,
    ) -> Result<Option<Self>, String> {
        match Self::open_relative(parent, component, label) {
            Ok(child) => Ok(Some(child)),
            Err(error) if is_exact_nt_not_found(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn create_relative(
        parent: &Self,
        component: &str,
        label: &str,
    ) -> Result<Self, String> {
        Self::open_relative_with_disposition(parent, component, FILE_CREATE, false, label)
    }

    /// Opens an existing protected child or creates it atomically beneath the
    /// already pinned parent. This is deliberately limited to a single
    /// validated component; it never provides recursive pathname authority.
    pub(crate) fn open_or_create_relative(
        parent: &Self,
        component: &str,
        label: &str,
    ) -> Result<Self, String> {
        // `FILE_OPEN_IF` is not a safe portable expression of the desired
        // operation here: the reviewed-build worker observed
        // STATUS_OBJECT_NAME_COLLISION (0xc0000035) from that one-shot
        // disposition while materializing an already-existing `scripts`
        // child. Preserve the pinned parent and discriminate only the two
        // exact benign statuses instead of treating a generic error as an
        // absence or collision.
        match Self::open_relative(parent, component, label) {
            Ok(existing) => Ok(existing),
            Err(error) if is_exact_nt_not_found(&error) => {
                match Self::create_relative(parent, component, label) {
                    Ok(created) => Ok(created),
                    Err(error) if is_exact_nt_create_collision(&error) => {
                        // A same-parent contender won after our exact open.
                        // Reopen below the same RootDirectory handle; this
                        // runs the ordinary positive directory/reparse and
                        // stable-parent validation before returning authority.
                        Self::open_relative(parent, component, label)
                    }
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }

    #[cfg(all(test, windows))]
    pub(crate) fn create_relative_with_sddl(
        parent: &Self,
        component: &str,
        sddl: &str,
        label: &str,
    ) -> Result<Self, String> {
        validate_protected_component(component, label)?;
        parent.assert_stable(label)?;
        let descriptor = security_descriptor_from_sddl(sddl, label)?;
        let result = (|| {
            let handle = nt_open_relative_with_security_descriptor_for_domain(
                parent.handle,
                component,
                FILE_LIST_DIRECTORY
                    | FILE_ADD_FILE
                    | FILE_ADD_SUBDIRECTORY
                    | FILE_READ_ATTRIBUTES
                    | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                FILE_CREATE,
                FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
                descriptor,
                Some("reviewed source snapshot"),
            )?;
            let information = handle_information(handle, label)?;
            if information.file_attributes
                & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY)
                != FILE_ATTRIBUTE_DIRECTORY
            {
                unsafe { CloseHandle(handle) };
                return Err(format!("{label} is unsafe"));
            }
            Ok(Self {
                path: parent.path.join(component),
                handle,
                identity: (
                    information.volume_serial_number,
                    information.file_index_high,
                    information.file_index_low,
                ),
            })
        })();
        unsafe { LocalFree(descriptor) };
        result
    }

    pub(crate) fn open_relative_for_delete(
        parent: &Self,
        component: &str,
        label: &str,
    ) -> Result<Self, String> {
        Self::open_relative_with_disposition(parent, component, FILE_OPEN, true, label)
    }

    pub(crate) fn open_relative_with_disposition(
        parent: &Self,
        component: &str,
        disposition: u32,
        allow_delete_share: bool,
        label: &str,
    ) -> Result<Self, String> {
        validate_protected_component(component, label)?;
        parent.assert_stable(label)?;
        #[cfg(windows)]
        {
            let handle = nt_open_relative_with_security_descriptor_for_domain(
                parent.handle,
                component,
                FILE_LIST_DIRECTORY
                    | FILE_ADD_FILE
                    | FILE_ADD_SUBDIRECTORY
                    | FILE_READ_ATTRIBUTES
                    | SYNCHRONIZE
                    | if allow_delete_share { DELETE } else { 0 },
                FILE_SHARE_READ
                    | FILE_SHARE_WRITE
                    | if allow_delete_share {
                        FILE_SHARE_DELETE
                    } else {
                        0
                    },
                disposition,
                FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
                Some("protected filesystem"),
            )?;
            let information = handle_information(handle, label)?;
            if information.file_attributes
                & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY)
                != FILE_ATTRIBUTE_DIRECTORY
            {
                unsafe { CloseHandle(handle) };
                return Err(format!("{label} is unsafe"));
            }
            Ok(Self {
                path: parent.path.join(component),
                handle,
                identity: (
                    information.volume_serial_number,
                    information.file_index_high,
                    information.file_index_low,
                ),
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (parent, component, disposition, allow_delete_share);
            Err(format!("{label} stable identity is unavailable"))
        }
    }

    pub(crate) fn try_clone(&self, label: &str) -> Result<Self, String> {
        #[cfg(windows)]
        {
            self.assert_stable(label)?;
            let mut duplicate = std::ptr::null_mut();
            let ok = unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    self.handle,
                    GetCurrentProcess(),
                    &mut duplicate,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            };
            if ok == 0 || duplicate.is_null() || duplicate == INVALID_HANDLE_VALUE {
                return Err(format!("{label} identity is unavailable"));
            }
            Ok(Self {
                path: self.path.clone(),
                handle: duplicate,
                identity: self.identity,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = self;
            Err(format!("{label} stable identity is unavailable"))
        }
    }

    pub(crate) fn assert_stable(&self, label: &str) -> Result<(), String> {
        classify_pinned_directory(&self.path, label)?;
        #[cfg(windows)]
        {
            let information = handle_information(self.handle, label)
                .map_err(|_| format!("{label} identity drifted"))?;
            if information.file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
                || self.identity
                    != (
                        information.volume_serial_number,
                        information.file_index_high,
                        information.file_index_low,
                    )
            {
                return Err(format!("{label} identity drifted"));
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for PinnedDirectory {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

pub(crate) trait PinnedParent {
    fn assert_stable(&self, label: &str) -> Result<(), String>;
    #[cfg(windows)]
    fn protected_fs_last_handle(&self) -> Result<*mut c_void, String>;
}

impl PinnedParent for PinnedDirectory {
    fn assert_stable(&self, label: &str) -> Result<(), String> {
        PinnedDirectory::assert_stable(self, label)
    }

    #[cfg(windows)]
    fn protected_fs_last_handle(&self) -> Result<*mut c_void, String> {
        self.assert_stable("protected pinned directory")?;
        Ok(self.handle)
    }
}

/// A chain of handle-pinned directory identities. Ancestors stay open while
/// each child is opened relative to the previous RootDirectory; no consumer
/// may turn a later mutable pathname into authority.
pub(crate) struct ProtectedDirectoryGuard {
    directories: Vec<PinnedDirectory>,
}

impl ProtectedDirectoryGuard {
    pub(crate) fn acquire(path: &Path, label: &str) -> Result<Self, String> {
        Ok(Self {
            directories: vec![PinnedDirectory::acquire(path, label)?],
        })
    }

    /// Adopts a directory already opened relative to an accepted pinned
    /// parent. This avoids reopening the resulting child by pathname when a
    /// consumer needs a full descendant chain for a later relative mutation.
    pub(crate) fn from_pinned_root(root: PinnedDirectory) -> Self {
        Self {
            directories: vec![root],
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.directories.last().expect("guard has root").path
    }

    pub(crate) fn assert_stable(&self, label: &str) -> Result<(), String> {
        for directory in &self.directories {
            directory.assert_stable(label)?;
        }
        Ok(())
    }

    pub(crate) fn descend_existing(&mut self, component: &str, label: &str) -> Result<(), String> {
        validate_protected_component(component, label)?;
        self.assert_stable(label)?;
        self.directories.push(PinnedDirectory::open_relative(
            self.directories.last().expect("guard has root"),
            component,
            label,
        )?);
        Ok(())
    }

    pub(crate) fn descend_optional_existing(
        &mut self,
        component: &str,
        label: &str,
    ) -> Result<bool, String> {
        validate_protected_component(component, label)?;
        self.assert_stable(label)?;
        let Some(child) = PinnedDirectory::open_optional_relative(
            self.directories.last().expect("guard has root"),
            component,
            label,
        )?
        else {
            return Ok(false);
        };
        self.directories.push(child);
        self.assert_stable(label)?;
        Ok(true)
    }

    pub(crate) fn descend_for_delete(
        &mut self,
        component: &str,
        label: &str,
    ) -> Result<(), String> {
        validate_protected_component(component, label)?;
        self.assert_stable(label)?;
        self.directories
            .push(PinnedDirectory::open_relative_for_delete(
                self.directories.last().expect("guard has root"),
                component,
                label,
            )?);
        Ok(())
    }

    pub(crate) fn create_child(&mut self, component: &str, label: &str) -> Result<(), String> {
        validate_protected_component(component, label)?;
        self.assert_stable(label)?;
        self.directories.push(PinnedDirectory::create_relative(
            self.directories.last().expect("guard has root"),
            component,
            label,
        )?);
        self.assert_stable(label)
    }

    /// Fixed-component open-or-create equivalent of `create_child`, retaining
    /// every ancestor handle. Consumers use this only for fixed product-owned
    /// roots such as an install's `versions` directory.
    pub(crate) fn descend_or_create(&mut self, component: &str, label: &str) -> Result<(), String> {
        validate_protected_component(component, label)?;
        self.assert_stable(label)?;
        self.directories
            .push(PinnedDirectory::open_or_create_relative(
                self.directories.last().expect("guard has root"),
                component,
                label,
            )?);
        self.assert_stable(label)
    }

    /// Test-only Windows feasibility support for a descriptor-at-create
    /// directory beneath an already pinned parent. The SDDL reaches
    /// NtCreateFile through OBJECT_ATTRIBUTES.SecurityDescriptor; it is not a
    /// production path-authority surface.
    #[cfg(all(test, windows))]
    pub(crate) fn create_child_with_security_descriptor(
        &mut self,
        component: &str,
        sddl: &str,
        label: &str,
    ) -> Result<(), String> {
        validate_protected_component(component, label)?;
        self.assert_stable(label)?;
        self.directories
            .push(PinnedDirectory::create_relative_with_sddl(
                self.directories.last().expect("guard has root"),
                component,
                sddl,
                label,
            )?);
        self.assert_stable(label)
    }

    pub(crate) fn create_renameable_child(
        &mut self,
        component: &str,
        label: &str,
    ) -> Result<(), String> {
        validate_protected_component(component, label)?;
        self.assert_stable(label)?;
        self.directories
            .push(PinnedDirectory::open_relative_with_disposition(
                self.directories.last().expect("guard has root"),
                component,
                FILE_CREATE,
                true,
                label,
            )?);
        self.assert_stable(label)
    }

    /// Creates a unique internally generated staging sibling. Existing stale
    /// `.stage-*` names are neither opened nor deleted, so a crash before a
    /// commit cannot wedge a retry for the same reviewed version.
    pub(crate) fn create_unique_renameable_child(&mut self, label: &str) -> Result<(), String> {
        for _ in 0..MAX_PROTECTED_EPHEMERAL_ATTEMPTS {
            let component = next_protected_ephemeral_component("stage")?;
            match self.create_renameable_child(&component, label) {
                Ok(()) => return Ok(()),
                Err(error) if error.contains("unavailable") => continue,
                Err(error) => return Err(error),
            }
        }
        Err(format!("{label} ephemeral identity is unavailable"))
    }

    pub(crate) fn try_clone(&self, label: &str) -> Result<Self, String> {
        let directories = self
            .directories
            .iter()
            .map(|directory| directory.try_clone(label))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { directories })
    }

    pub(crate) fn is_direct_child_of(&self, parent: &Self) -> bool {
        #[cfg(windows)]
        {
            self.directories.len() == parent.directories.len() + 1
                && self
                    .directories
                    .iter()
                    .zip(&parent.directories)
                    .all(|(left, right)| left.identity == right.identity)
        }
        #[cfg(not(windows))]
        {
            let _ = parent;
            false
        }
    }

    /// Returns the final handle only after validating the complete pinned
    /// chain. Snapshot-specific native operations use this exact already-open
    /// object; no caller-supplied path or root can be opened through this seam.
    #[cfg(windows)]
    pub(crate) fn last_handle(&self, label: &str) -> Result<*mut c_void, String> {
        self.assert_stable(label)?;
        Ok(self.directories.last().expect("guard has root").handle)
    }

    /// Renames the exact opened direct child under the exact pinned parent.
    /// The native mutation is rooted in already-open handles; caller-selected
    /// pathnames never become authority for the rename.
    pub(crate) fn rename_direct_child_within_parent(
        &mut self,
        parent: &Self,
        component: &str,
        label: &str,
    ) -> Result<(), String> {
        validate_protected_component(component, label)?;
        self.assert_stable(label)?;
        parent.assert_stable(label)?;
        if !self.is_direct_child_of(parent) {
            return Err(format!("{label} is unsafe"));
        }
        #[cfg(windows)]
        {
            let source = self.last_handle(label)?;
            // Hold an independently duplicated, identity-validated parent
            // chain for the full native mutation so RootDirectory cannot be
            // redirected by a mutable pathname or caller-owned handle slot.
            let destination_parent = parent.try_clone(label)?;
            let destination = destination_parent.last_handle(label)?;
            let name: Vec<u16> = component.encode_utf16().collect();
            let name_bytes = name
                .len()
                .checked_mul(2)
                .ok_or_else(|| format!("{label} is unsafe"))?;
            let total = 20_usize
                .checked_add(name_bytes)
                .ok_or_else(|| format!("{label} is unsafe"))?;
            let mut information = vec![0_u8; total];
            // FILE_RENAME_INFORMATION (x64): ReplaceIfExists/padding,
            // RootDirectory, FileNameLength, then the variable UTF-16 name.
            // ReplaceIfExists remains false because the zeroed buffer must
            // never replace an already-present destination authority.
            information[8..16].copy_from_slice(&(destination as usize).to_ne_bytes());
            information[16..20].copy_from_slice(&(name_bytes as u32).to_ne_bytes());
            for (index, unit) in name.iter().enumerate() {
                let offset = 20 + index * 2;
                information[offset..offset + 2].copy_from_slice(&unit.to_ne_bytes());
            }
            let mut status = IO_STATUS_BLOCK {
                status: 0,
                information: 0,
            };
            let result = unsafe {
                NtSetInformationFile(
                    source,
                    &mut status,
                    information.as_mut_ptr().cast(),
                    information.len() as u32,
                    FILE_RENAME_INFO_CLASS,
                )
            };
            if result < 0 {
                return Err(format!("{label} is unavailable ({result:#x})"));
            }
            self.mark_renamed_as_direct_child(parent, component, label)?;
            self.assert_stable(label)?;
            parent.assert_stable(label)?;
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = (parent, component);
            Err(format!("{label} stable identity is unavailable"))
        }
    }

    /// Records path bookkeeping only after a successful native same-parent
    /// rename. The handle identity remains authoritative and is revalidated by
    /// the caller immediately after this bookkeeping update.
    fn mark_renamed_as_direct_child(
        &mut self,
        parent: &Self,
        component: &str,
        label: &str,
    ) -> Result<(), String> {
        validate_protected_component(component, label)?;
        parent.assert_stable(label)?;
        if !self.is_direct_child_of(parent) {
            return Err(format!("{label} is unsafe"));
        }
        self.directories.last_mut().expect("guard has root").path = parent.path().join(component);
        Ok(())
    }
}

impl PinnedParent for ProtectedDirectoryGuard {
    fn assert_stable(&self, label: &str) -> Result<(), String> {
        ProtectedDirectoryGuard::assert_stable(self, label)
    }

    #[cfg(windows)]
    fn protected_fs_last_handle(&self) -> Result<*mut c_void, String> {
        self.last_handle("protected directory guard")
    }
}

/// Bounded evidence for an already-opened reviewed regular artifact.  The
/// parent is handle-pinned and the child is opened relative to that handle, so
/// neither the caller's child spelling nor a later pathname replacement can
/// become artifact authority.
#[allow(dead_code)] // Consumed by the dormant fixed activation binary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReviewedArtifactIdentity {
    pub(crate) sha256: String,
    pub(crate) byte_length: u64,
    pub(crate) object_identity: String,
}

/// A reviewed regular object whose Windows handle remains open with
/// `FILE_SHARE_READ` only.  While retained, another same-user process may
/// read the object (which Python needs) but cannot obtain write or delete
/// access.  Windows rename and delete both require delete access, so this
/// blocks pathname replacement for the lease lifetime.
///
/// The caller must keep this value alive while it uses the corresponding
/// pathname.  It deliberately does not expose the file handle for execution:
/// consumers still use their fixed pathname, with the retained handle making
/// that pathname non-replaceable.
#[allow(dead_code)] // Retained by the stable adapter launch lease.
pub(crate) struct RetainedReviewedArtifact {
    pub(crate) identity: ReviewedArtifactIdentity,
    _file: fs::File,
}

/// Bind one pre-existing reviewed artifact to its opened object identity and
/// expected SHA-256.  This is the shared protected-filesystem counterpart to
/// the reviewed-promotion evidence path; it deliberately accepts a pinned
/// parent and one component rather than a caller-selected full pathname.
#[allow(dead_code)] // The no-argument activation binary owns the production call.
pub(crate) fn bind_reviewed_regular_artifact(
    parent: &PinnedDirectory,
    component: &str,
    expected_sha256: &str,
    max_bytes: u64,
    label: &str,
) -> Result<ReviewedArtifactIdentity, String> {
    Ok(
        retain_reviewed_regular_artifact(parent, component, expected_sha256, max_bytes, label)?
            .identity,
    )
}

/// Open and bind a reviewed regular object while retaining its no-follow,
/// no-write/no-delete Windows handle.  This is the launch-bound counterpart
/// of `bind_reviewed_regular_artifact` for consumers that must subsequently
/// use a fixed pathname (for example CreateProcess and Python module import).
#[allow(dead_code)] // Retained by the stable adapter launch lease.
pub(crate) fn retain_reviewed_regular_artifact(
    parent: &PinnedDirectory,
    component: &str,
    expected_sha256: &str,
    max_bytes: u64,
    label: &str,
) -> Result<RetainedReviewedArtifact, String> {
    if max_bytes == 0
        || expected_sha256.len() != 64
        || !expected_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(format!("{label} identity is unsafe"));
    }
    parent.assert_stable(label)?;
    let mut file = open_relative_regular_file(parent, component, label)?;
    let metadata = file
        .metadata()
        .map_err(|_| format!("{label} is unavailable"))?;
    if !metadata.file_type().is_file() || metadata.len() > max_bytes {
        return Err(format!("{label} is unsafe"));
    }
    let byte_length = metadata.len();
    let mut bytes = Vec::with_capacity(byte_length as usize);
    Read::by_ref(&mut file)
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| format!("{label} is unavailable"))?;
    if bytes.len() as u64 != byte_length {
        return Err(format!("{label} changed while reading"));
    }
    #[cfg(windows)]
    let object_identity = {
        let information = handle_information(file.as_raw_handle().cast(), label)?;
        if information.file_attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY)
            != 0
        {
            return Err(format!("{label} is unsafe"));
        }
        format!(
            "{:08x}:{:08x}:{:08x}",
            information.volume_serial_number,
            information.file_index_high,
            information.file_index_low
        )
    };
    #[cfg(not(windows))]
    let object_identity = return Err(format!("{label} stable identity is unavailable"));

    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    if sha256 != expected_sha256.to_ascii_lowercase() {
        return Err(format!("{label} digest drifted"));
    }
    parent.assert_stable(label)?;
    Ok(RetainedReviewedArtifact {
        identity: ReviewedArtifactIdentity {
            sha256,
            byte_length,
            object_identity,
        },
        _file: file,
    })
}

pub(crate) fn validate_component(component: &str, label: &str) -> Result<(), String> {
    if component.is_empty()
        || component.len() > 255
        || component == "."
        || component == ".."
        || component.contains(['/', '\\', '\0'])
    {
        return Err(format!("{label} is unsafe"));
    }
    Ok(())
}

pub(crate) fn write_new_regular_in<P: PinnedParent>(
    parent: &P,
    name: &str,
    bytes: &[u8],
    label: &str,
) -> Result<(), String> {
    validate_component(name, label)?;
    parent.assert_stable(label)?;
    let mut file = create_relative_regular_file(parent, name, label)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| format!("{label} is unavailable"))?;
    if file
        .metadata()
        .map_err(|_| format!("{label} is unavailable"))?
        .len()
        != bytes.len() as u64
    {
        return Err(format!("{label} is unsafe"));
    }
    parent.assert_stable(label)
}

/// A newly-created regular file whose open handle remains authoritative until
/// it is atomically committed to another fixed child name of the same pinned
/// parent. The destination is never opened by pathname: Windows performs the
/// replacement relative to this exact parent handle.
pub(crate) struct AtomicRegularWrite<'a, P: PinnedParent> {
    parent: &'a P,
    file: fs::File,
}

const MAX_PROTECTED_EPHEMERAL_ATTEMPTS: u64 = 32;

fn next_protected_ephemeral_component(kind: &str) -> Result<String, String> {
    // `kind` is selected only by this module's fixed internal call sites. A
    // version-4 UUID is generated inside this authority boundary so a stale
    // sibling left by a crashed process stays inert even after process restart
    // or PID reuse; no caller supplies any part of the child path/name.
    if !matches!(kind, "file" | "stage") {
        return Err("protected ephemeral identity is unsafe".into());
    }
    let component = format!(".catdesk-{kind}-{}", Uuid::new_v4().simple());
    validate_component(&component, "protected ephemeral identity")?;
    Ok(component)
}

/// Creates an internally named, bounded temporary sibling under a pinned
/// parent. A crash can leave that name behind, but a later operation obtains a
/// distinct name without enumerating or deleting stale residue.
pub(crate) fn write_unique_regular_for_atomic_replace<'a, P: PinnedParent>(
    parent: &'a P,
    bytes: &[u8],
    label: &str,
) -> Result<AtomicRegularWrite<'a, P>, String> {
    for _ in 0..MAX_PROTECTED_EPHEMERAL_ATTEMPTS {
        let name = next_protected_ephemeral_component("file")?;
        match write_new_regular_for_atomic_replace(parent, &name, bytes, label) {
            Ok(write) => return Ok(write),
            // A collision is the one safe retry: the generated direct child
            // may be stale from a prior crash, never a caller-selected name.
            Err(error) if error.contains("unavailable") => continue,
            Err(error) => return Err(error),
        }
    }
    Err(format!("{label} ephemeral identity is unavailable"))
}

pub(crate) fn write_new_regular_for_atomic_replace<'a, P: PinnedParent>(
    parent: &'a P,
    name: &str,
    bytes: &[u8],
    label: &str,
) -> Result<AtomicRegularWrite<'a, P>, String> {
    validate_component(name, label)?;
    parent.assert_stable(label)?;
    let mut file = open_relative_regular_file_with_disposition_and_share(
        parent,
        name,
        FILE_CREATE,
        FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        label,
    )?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| format!("{label} is unavailable"))?;
    if file
        .metadata()
        .map_err(|_| format!("{label} is unavailable"))?
        .len()
        != bytes.len() as u64
    {
        return Err(format!("{label} is unsafe"));
    }
    parent.assert_stable(label)?;
    Ok(AtomicRegularWrite { parent, file })
}

impl<P: PinnedParent> AtomicRegularWrite<'_, P> {
    /// Atomically replaces exactly one direct child name under the same pinned
    /// parent. The temporary source stays open with DELETE access for the
    /// complete write/flush/rename boundary. On Windows a rename replaces the
    /// directory entry itself; it does not resolve a destination symlink.
    pub(crate) fn commit_replace(self, destination: &str, label: &str) -> Result<(), String> {
        validate_component(destination, label)?;
        self.parent.assert_stable(label)?;
        self.file
            .sync_all()
            .map_err(|_| format!("{label} is unavailable"))?;
        #[cfg(windows)]
        {
            let destination_parent = self.parent.protected_fs_last_handle()?;
            let name: Vec<u16> = destination.encode_utf16().collect();
            let name_bytes = name
                .len()
                .checked_mul(2)
                .ok_or_else(|| format!("{label} is unsafe"))?;
            let total = 20_usize
                .checked_add(name_bytes)
                .ok_or_else(|| format!("{label} is unsafe"))?;
            let mut information = vec![0_u8; total];
            // FILE_RENAME_INFORMATION: ReplaceIfExists=true, RootDirectory,
            // FileNameLength, then the direct-child UTF-16 name. No caller
            // pathname or destination handle can redirect the operation.
            information[0] = 1;
            information[8..16].copy_from_slice(&(destination_parent as usize).to_ne_bytes());
            information[16..20].copy_from_slice(&(name_bytes as u32).to_ne_bytes());
            for (index, unit) in name.iter().enumerate() {
                let offset = 20 + index * 2;
                information[offset..offset + 2].copy_from_slice(&unit.to_ne_bytes());
            }
            let mut status = IO_STATUS_BLOCK {
                status: 0,
                information: 0,
            };
            let result = unsafe {
                NtSetInformationFile(
                    self.file.as_raw_handle().cast(),
                    &mut status,
                    information.as_mut_ptr().cast(),
                    information.len() as u32,
                    FILE_RENAME_INFO_CLASS,
                )
            };
            if result < 0 {
                return Err(format!("{label} is unavailable ({result:#x})"));
            }
        }
        #[cfg(not(windows))]
        {
            let _ = destination;
            return Err(format!("{label} stable identity is unavailable"));
        }
        self.parent.assert_stable(label)
    }
}

/// A create-once regular file opened with Windows delete-on-close semantics.
/// It supplies crash-safe lock lifetime without any stale-lock pathname delete
/// recovery: a pre-existing or substituted lock is simply refused.
pub(crate) struct DeleteOnCloseRegularLock {
    _file: fs::File,
}

pub(crate) fn create_delete_on_close_regular_lock<P: PinnedParent>(
    parent: &P,
    name: &str,
    label: &str,
) -> Result<DeleteOnCloseRegularLock, String> {
    validate_component(name, label)?;
    parent.assert_stable(label)?;
    #[cfg(windows)]
    {
        let handle = nt_open_relative(
            parent.protected_fs_last_handle()?,
            name,
            FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_CREATE,
            FILE_NON_DIRECTORY_FILE
                | FILE_SYNCHRONOUS_IO_NONALERT
                | FILE_OPEN_REPARSE_POINT
                | FILE_DELETE_ON_CLOSE,
        )?;
        let attrs = handle_attributes(handle, label)?;
        if attrs & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
            unsafe { CloseHandle(handle) };
            return Err(format!("{label} is unsafe"));
        }
        parent.assert_stable(label)?;
        Ok(DeleteOnCloseRegularLock {
            _file: unsafe { fs::File::from_raw_handle(handle) },
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (parent, name);
        Err(format!("{label} stable identity is unavailable"))
    }
}

pub(crate) fn read_relative_regular<P: PinnedParent>(
    parent: &P,
    name: &str,
    limit: u64,
    label: &str,
) -> Result<Vec<u8>, String> {
    validate_component(name, label)?;
    parent.assert_stable(label)?;
    let mut file = open_relative_regular_file(parent, name, label)?;
    let length = file
        .metadata()
        .map_err(|_| format!("{label} is unavailable"))?
        .len();
    if length > limit {
        return Err(format!("{label} is unsafe"));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    Read::by_ref(&mut file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| format!("{label} is unavailable"))?;
    if bytes.len() as u64 != length {
        return Err(format!("{label} changed while reading"));
    }
    parent.assert_stable(label)?;
    Ok(bytes)
}

/// Reads one existing protected child, distinguishing the one safe absence
/// outcome from every unsafe/no-follow/identity failure. This is used for
/// fixed state records where an absent initial file is valid but a substituted
/// or malformed object is not.
pub(crate) fn read_optional_relative_regular<P: PinnedParent>(
    parent: &P,
    name: &str,
    limit: u64,
    label: &str,
) -> Result<Option<Vec<u8>>, String> {
    validate_component(name, label)?;
    parent.assert_stable(label)?;
    let mut file = match open_optional_relative_regular_file(parent, name, label)? {
        Some(file) => file,
        None => return Ok(None),
    };
    let length = file
        .metadata()
        .map_err(|_| format!("{label} is unavailable"))?
        .len();
    if length > limit {
        return Err(format!("{label} is unsafe"));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    Read::by_ref(&mut file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| format!("{label} is unavailable"))?;
    if bytes.len() as u64 != length {
        return Err(format!("{label} changed while reading"));
    }
    parent.assert_stable(label)?;
    Ok(Some(bytes))
}

/// Removes one exact existing regular child only after reading that same
/// no-follow handle and comparing all of its bounded bytes.  This is the
/// narrow deletion building block for fixed receipt rollback: callers still
/// select their fixed name and expected bytes in their own closed authority;
/// this shared primitive never enumerates a directory or follows a path.
pub(crate) fn remove_exact_relative_regular_with_bytes<P: PinnedParent>(
    parent: &P,
    name: &str,
    expected: &[u8],
    label: &str,
) -> Result<(), String> {
    validate_component(name, label)?;
    if expected.len() > 64 * 1024 {
        return Err(format!("{label} is unsafe"));
    }
    parent.assert_stable(label)?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;

        {
            let mut file = open_relative_regular_file_with_disposition_and_share(
                parent,
                name,
                FILE_OPEN,
                FILE_READ_DATA | FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                label,
            )?;
            let length = file
                .metadata()
                .map_err(|_| format!("{label} is unavailable"))?
                .len();
            if length != expected.len() as u64 {
                return Err(format!("{label} identity is unsafe"));
            }
            let mut actual = Vec::with_capacity(expected.len());
            Read::by_ref(&mut file)
                .take(expected.len() as u64 + 1)
                .read_to_end(&mut actual)
                .map_err(|_| format!("{label} is unavailable"))?;
            if actual != expected {
                return Err(format!("{label} identity is unsafe"));
            }
            parent.assert_stable(label)?;
            let mut status = IO_STATUS_BLOCK {
                status: 0,
                information: 0,
            };
            let mut delete = 1_u8; // FILE_DISPOSITION_INFORMATION::DeleteFile
            let result = unsafe {
                NtSetInformationFile(
                    file.as_raw_handle().cast(),
                    &mut status,
                    (&mut delete as *mut u8).cast(),
                    1,
                    FILE_DISPOSITION_INFO_CLASS,
                )
            };
            if result < 0 {
                return Err(format!("{label} is unavailable ({result:#x})"));
            }
            // Closing this exact handle finalizes the disposition before the
            // postcondition below reopens the fixed direct child.
        }
        parent.assert_stable(label)?;
        if read_optional_relative_regular(parent, name, expected.len() as u64, label)?.is_some() {
            return Err(format!("{label} identity drifted during removal"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (parent, name, expected);
        Err(format!("{label} stable identity is unavailable"))
    }
}

fn is_exact_nt_not_found(error: &str) -> bool {
    // `nt_open_relative_with_security_descriptor_for_domain` is the only
    // source of this fixed NTSTATUS rendering. Do not treat generic I/O or a
    // reparse/type error as absence.
    error.ends_with("(0xc0000034)")
}

fn is_exact_nt_create_collision(error: &str) -> bool {
    // Do not promote an arbitrary unavailable/open error into a second open.
    // This is only the native STATUS_OBJECT_NAME_COLLISION returned by an
    // exact FILE_CREATE beneath the already-pinned RootDirectory parent.
    error.ends_with("(0xc0000035)")
}

pub(crate) fn create_relative_regular_file<P: PinnedParent>(
    parent: &P,
    name: &str,
    label: &str,
) -> Result<fs::File, String> {
    open_relative_regular_file_with_disposition(
        parent,
        name,
        FILE_CREATE,
        FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        label,
    )
}

pub(crate) fn open_relative_regular_file<P: PinnedParent>(
    parent: &P,
    name: &str,
    label: &str,
) -> Result<fs::File, String> {
    open_relative_regular_file_with_disposition(
        parent,
        name,
        FILE_OPEN,
        FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        label,
    )
}

fn open_relative_regular_file_with_disposition<P: PinnedParent>(
    parent: &P,
    name: &str,
    disposition: u32,
    access: u32,
    label: &str,
) -> Result<fs::File, String> {
    open_relative_regular_file_with_disposition_and_share(
        parent,
        name,
        disposition,
        access,
        FILE_SHARE_READ,
        label,
    )
}

fn open_relative_regular_file_with_disposition_and_share<P: PinnedParent>(
    parent: &P,
    name: &str,
    disposition: u32,
    access: u32,
    share: u32,
    label: &str,
) -> Result<fs::File, String> {
    validate_component(name, label)?;
    parent.assert_stable(label)?;
    #[cfg(windows)]
    {
        let handle = nt_open_relative(
            parent.protected_fs_last_handle()?,
            name,
            access,
            share,
            disposition,
            FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
        )?;
        let attrs = handle_attributes(handle, label)?;
        if attrs & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
            unsafe { CloseHandle(handle) };
            return Err(format!("{label} is unsafe"));
        }
        Ok(unsafe { fs::File::from_raw_handle(handle) })
    }
    #[cfg(not(windows))]
    {
        let _ = (parent, name, disposition, access, share);
        Err(format!("{label} stable identity is unavailable"))
    }
}

/// The only optional-file open in the protected filesystem API.  It preserves
/// the precise NT missing-name result long enough to distinguish a safe absent
/// initial record from a reparse, type, access, or identity error.
fn open_optional_relative_regular_file<P: PinnedParent>(
    parent: &P,
    name: &str,
    label: &str,
) -> Result<Option<fs::File>, String> {
    #[cfg(windows)]
    {
        let handle = match nt_open_relative_with_error_domain(
            parent.protected_fs_last_handle()?,
            name,
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_READ,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
            "protected optional filesystem",
        ) {
            Ok(handle) => handle,
            Err(error) if is_exact_nt_not_found(&error) => return Ok(None),
            Err(error) => return Err(error),
        };
        let attrs = handle_attributes(handle, label)?;
        if attrs & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
            unsafe { CloseHandle(handle) };
            return Err(format!("{label} is unsafe"));
        }
        Ok(Some(unsafe { fs::File::from_raw_handle(handle) }))
    }
    #[cfg(not(windows))]
    {
        let _ = (parent, name, label);
        Err("protected filesystem authority is unavailable".into())
    }
}

const FILE_READ_DATA: u32 = 0x0001;
const FILE_WRITE_DATA: u32 = 0x0002;
const FILE_READ_ATTRIBUTES: u32 = 0x0080;
const FILE_LIST_DIRECTORY: u32 = 0x0001;
const FILE_ADD_FILE: u32 = 0x0002;
const FILE_ADD_SUBDIRECTORY: u32 = 0x0004;
const DELETE: u32 = 0x0001_0000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_READ: u32 = 0x0000_0001;
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
const FILE_SHARE_DELETE: u32 = 0x0000_0004;
const FILE_OPEN: u32 = 1;
const FILE_CREATE: u32 = 2;
const OPEN_EXISTING: u32 = 3;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;
const FILE_DIRECTORY_FILE: u32 = 0x0000_0001;
const FILE_NON_DIRECTORY_FILE: u32 = 0x0000_0040;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x0000_0020;
const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_DELETE_ON_CLOSE: u32 = 0x0000_1000;
const FILE_RENAME_INFO_CLASS: u32 = 10; // native FILE_INFORMATION_CLASS::FileRenameInformation
const FILE_DISPOSITION_INFO_CLASS: u32 = 13; // FileDispositionInformation
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

#[cfg(windows)]
const INVALID_HANDLE_VALUE: *mut c_void = -1_isize as *mut c_void;
#[cfg(windows)]
#[repr(C)]
pub(crate) struct UNICODE_STRING {
    pub(crate) length: u16,
    pub(crate) maximum_length: u16,
    pub(crate) buffer: *mut u16,
}
#[cfg(windows)]
#[repr(C)]
pub(crate) struct OBJECT_ATTRIBUTES {
    pub(crate) length: u32,
    pub(crate) root_directory: *mut c_void,
    pub(crate) object_name: *mut UNICODE_STRING,
    pub(crate) attributes: u32,
    pub(crate) security_descriptor: *mut c_void,
    pub(crate) security_quality_of_service: *mut c_void,
}
#[cfg(windows)]
#[repr(C)]
pub(crate) struct IO_STATUS_BLOCK {
    pub(crate) status: isize,
    pub(crate) information: usize,
}
#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
pub(crate) struct BY_HANDLE_FILE_INFORMATION {
    pub(crate) file_attributes: u32,
    pub(crate) creation_time_low: u32,
    pub(crate) creation_time_high: u32,
    pub(crate) last_access_time_low: u32,
    pub(crate) last_access_time_high: u32,
    pub(crate) last_write_time_low: u32,
    pub(crate) last_write_time_high: u32,
    pub(crate) volume_serial_number: u32,
    pub(crate) file_size_high: u32,
    pub(crate) file_size_low: u32,
    pub(crate) number_of_links: u32,
    pub(crate) file_index_high: u32,
    pub(crate) file_index_low: u32,
}
#[cfg(windows)]
unsafe extern "system" {
    fn CreateFileW(
        file_name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *mut c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: *mut c_void,
    ) -> *mut c_void;
    fn NtCreateFile(
        file_handle: *mut *mut c_void,
        desired_access: u32,
        object_attributes: *mut OBJECT_ATTRIBUTES,
        io_status_block: *mut IO_STATUS_BLOCK,
        allocation_size: *mut c_void,
        file_attributes: u32,
        share_access: u32,
        create_disposition: u32,
        create_options: u32,
        ea_buffer: *mut c_void,
        ea_length: u32,
    ) -> i32;
    fn GetFileInformationByHandle(
        handle: *mut c_void,
        information: *mut BY_HANDLE_FILE_INFORMATION,
    ) -> i32;
    fn NtSetInformationFile(
        file_handle: *mut c_void,
        io_status_block: *mut IO_STATUS_BLOCK,
        file_information: *mut c_void,
        length: u32,
        file_information_class: u32,
    ) -> i32;
    pub(crate) fn CloseHandle(handle: *mut c_void) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn DuplicateHandle(
        source_process: *mut c_void,
        source_handle: *mut c_void,
        target_process: *mut c_void,
        target_handle: *mut *mut c_void,
        desired_access: u32,
        inherit_handle: i32,
        options: u32,
    ) -> i32;
    #[cfg(test)]
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        definition: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        descriptor_size: *mut u32,
    ) -> i32;
    #[cfg(test)]
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}
#[cfg(windows)]
fn handle_attributes(handle: *mut c_void, label: &str) -> Result<u32, String> {
    Ok(handle_information(handle, label)?.file_attributes)
}
#[cfg(windows)]
pub(crate) fn handle_information(
    handle: *mut c_void,
    label: &str,
) -> Result<BY_HANDLE_FILE_INFORMATION, String> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        return Err(format!("{label} identity is unavailable"));
    }
    Ok(info)
}
#[cfg(windows)]
fn nt_open_relative(
    parent: *mut c_void,
    component: &str,
    access: u32,
    share: u32,
    disposition: u32,
    options: u32,
) -> Result<*mut c_void, String> {
    nt_open_relative_with_security_descriptor_for_domain(
        parent,
        component,
        access,
        share,
        disposition,
        options,
        std::ptr::null_mut(),
        None,
    )
}

/// Opens one component under an already pinned handle while retaining the
/// caller's fixed error vocabulary. The parent handle, not a pathname, is the
/// authority boundary.
#[cfg(windows)]
pub(crate) fn nt_open_relative_with_error_domain(
    parent: *mut c_void,
    component: &str,
    access: u32,
    share: u32,
    disposition: u32,
    options: u32,
    error_domain: &str,
) -> Result<*mut c_void, String> {
    nt_open_relative_with_security_descriptor_for_domain(
        parent,
        component,
        access,
        share,
        disposition,
        options,
        std::ptr::null_mut(),
        Some(error_domain),
    )
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)] // Mirrors the fixed NtCreateFile authority tuple.
fn nt_open_relative_with_security_descriptor_for_domain(
    parent: *mut c_void,
    component: &str,
    access: u32,
    share: u32,
    disposition: u32,
    options: u32,
    security_descriptor: *mut c_void,
    error_domain: Option<&str>,
) -> Result<*mut c_void, String> {
    let mut wide: Vec<u16> = component.encode_utf16().collect();
    if wide.is_empty() || wide.len() > u16::MAX as usize / 2 {
        return Err(match error_domain {
            Some(error_domain) => format!("{error_domain} child identity is unsafe"),
            None => "protected filesystem child identity is unsafe".into(),
        });
    }
    let mut name = UNICODE_STRING {
        length: (wide.len() * 2) as u16,
        maximum_length: (wide.len() * 2) as u16,
        buffer: wide.as_mut_ptr(),
    };
    let mut attributes = OBJECT_ATTRIBUTES {
        length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        root_directory: parent,
        object_name: &mut name,
        attributes: 0x40,
        security_descriptor,
        security_quality_of_service: std::ptr::null_mut(),
    };
    let mut status = IO_STATUS_BLOCK {
        status: 0,
        information: 0,
    };
    let mut handle = std::ptr::null_mut();
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &mut attributes,
            &mut status,
            std::ptr::null_mut(),
            0,
            share,
            disposition,
            options,
            std::ptr::null_mut(),
            0,
        )
    };
    if result < 0 || handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(match error_domain {
            Some(error_domain) => {
                format!("{error_domain} child {component} is unavailable ({result:#x})")
            }
            None => "protected filesystem child is unavailable".into(),
        });
    }
    Ok(handle)
}

#[cfg(windows)]
const DUPLICATE_SAME_ACCESS: u32 = 0x0000_0002;

#[cfg(all(test, windows))]
fn security_descriptor_from_sddl(sddl: &str, label: &str) -> Result<*mut c_void, String> {
    if sddl.is_empty() || sddl.len() > 8 * 1024 || sddl.contains('\0') {
        return Err(format!("{label} security descriptor is unsafe"));
    }
    let wide = std::ffi::OsString::from(sddl)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut descriptor = std::ptr::null_mut();
    let mut size = 0u32;
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            1,
            &mut descriptor,
            &mut size,
        )
    };
    if ok == 0 || descriptor.is_null() || size == 0 || size > 64 * 1024 {
        if !descriptor.is_null() {
            unsafe { LocalFree(descriptor) };
        }
        return Err(format!("{label} security descriptor is unavailable"));
    }
    Ok(descriptor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_module_has_no_snapshot_reverse_dependency() {
        let source = include_str!("windows_protected_fs.rs");
        let forbidden = ["crate", "reviewed_source_snapshot"].join("::");
        assert!(!source.contains(&forbidden));
        assert!(source.contains("OBJECT_ATTRIBUTES"));
        assert!(source.contains("RootDirectory"));
        assert!(source.contains("NtCreateFile"));
    }

    #[test]
    fn protected_ephemeral_names_are_restart_independent_and_caller_unselectable() {
        let first_file = next_protected_ephemeral_component("file").expect("file identity");
        let second_file = next_protected_ephemeral_component("file").expect("second identity");
        let stage = next_protected_ephemeral_component("stage").expect("stage identity");

        assert_ne!(first_file, second_file);
        assert!(first_file.starts_with(".catdesk-file-"));
        assert!(stage.starts_with(".catdesk-stage-"));
        assert!(first_file.len() <= 64);
        assert!(stage.len() <= 64);
        validate_component(&first_file, "test ephemeral file").expect("safe file component");
        validate_component(&stage, "test ephemeral stage").expect("safe stage component");
        Uuid::parse_str(first_file.trim_start_matches(".catdesk-file-"))
            .expect("UUID-backed file identity");
        Uuid::parse_str(stage.trim_start_matches(".catdesk-stage-"))
            .expect("UUID-backed stage identity");
        assert!(next_protected_ephemeral_component("caller-selected").is_err());

        let source = include_str!("windows_protected_fs.rs");
        let legacy_counter = ["PROTECTED_EPHEMERAL", "_COUNTER"].concat();
        let legacy_pid = ["std::process::", "id()"].concat();
        assert!(!source.contains(&legacy_counter));
        assert!(!source.contains(&legacy_pid));
    }

    #[test]
    fn pinned_directory_and_guard_have_one_way_shared_ownership() {
        let shared = include_str!("windows_protected_fs.rs");
        let snapshot = include_str!("reviewed_source_snapshot.rs");
        let count = |source: &str, declaration: &str| {
            source
                .lines()
                .filter(|line| line.trim_start().starts_with(declaration))
                .count()
        };
        assert_eq!(count(shared, "pub(crate) struct PinnedDirectory"), 1);
        assert_eq!(count(snapshot, "pub(crate) struct PinnedDirectory"), 0);
        assert_eq!(count(shared, "impl PinnedDirectory {"), 1);
        assert_eq!(count(snapshot, "impl PinnedDirectory {"), 0);
        assert_eq!(count(shared, "impl Drop for PinnedDirectory {"), 1);
        assert_eq!(count(snapshot, "impl Drop for PinnedDirectory {"), 0);
        assert_eq!(
            count(shared, "pub(crate) struct ProtectedDirectoryGuard"),
            1
        );
        assert_eq!(
            count(snapshot, "pub(crate) struct ProtectedDirectoryGuard"),
            0
        );
        assert_eq!(count(shared, "impl ProtectedDirectoryGuard {"), 1);
        assert_eq!(count(snapshot, "impl ProtectedDirectoryGuard {"), 0);
        assert_eq!(
            count(shared, "impl PinnedParent for ProtectedDirectoryGuard {"),
            1
        );
        assert_eq!(
            count(snapshot, "impl PinnedParent for ProtectedDirectoryGuard {"),
            0
        );
        assert!(snapshot.contains("ProtectedDirectoryGuard"));
        assert!(shared.contains("create_child_with_security_descriptor"));
        assert!(shared.contains("rename_direct_child_within_parent"));
        assert!(shared.contains("FILE_RENAME_INFO_CLASS"));
        assert!(shared.contains("mark_renamed_as_direct_child"));
        assert!(snapshot.contains(".rename_direct_child_within_parent("));
        assert!(!snapshot.contains("FILE_RENAME_INFO_CLASS"));
        assert!(!snapshot.contains("mark_renamed_as_direct_child"));
        assert!(!snapshot.contains(".directories"));
    }

    #[cfg(windows)]
    #[test]
    fn protected_directory_guard_preserves_pinned_chain_clone_and_child_identity() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("catdesk-protected-guard-{nonce}"));
        fs::create_dir_all(&root).expect("fixture root");
        {
            let root_guard =
                ProtectedDirectoryGuard::acquire(&root, "protected guard fixture").expect("root");
            root_guard
                .assert_stable("protected guard fixture")
                .expect("stable root");

            let mut created = root_guard
                .try_clone("protected guard fixture")
                .expect("clone");
            created
                .create_child("created", "protected guard fixture")
                .expect("create child");
            assert!(created.is_direct_child_of(&root_guard));
            created
                .assert_stable("protected guard fixture")
                .expect("stable child");
            let cloned_child = created
                .try_clone("protected guard fixture")
                .expect("clone child");
            assert_eq!(cloned_child.path(), created.path());
            cloned_child
                .assert_stable("protected guard fixture")
                .expect("stable cloned child");
            drop(cloned_child);
            drop(created);

            let mut delete_child = root_guard
                .try_clone("protected guard fixture")
                .expect("clone");
            delete_child
                .descend_for_delete("created", "protected guard fixture")
                .expect("open delete child");
            assert!(delete_child.is_direct_child_of(&root_guard));
            delete_child
                .assert_stable("protected guard fixture")
                .expect("stable delete child");

            let mut renameable = root_guard
                .try_clone("protected guard fixture")
                .expect("clone");
            renameable
                .create_renameable_child("renameable", "protected guard fixture")
                .expect("create renameable child");
            assert!(renameable.is_direct_child_of(&root_guard));
            renameable
                .assert_stable("protected guard fixture")
                .expect("stable renameable child");
        }
        fs::remove_dir_all(root).expect("fixture cleanup");
    }

    #[cfg(windows)]
    #[test]
    fn protected_directory_guard_rename_is_pinned_same_parent_and_fail_closed() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("catdesk-protected-rename-{nonce}"));
        let other_root =
            std::env::temp_dir().join(format!("catdesk-protected-rename-other-{nonce}"));
        fs::create_dir_all(&root).expect("fixture root");
        fs::create_dir_all(&other_root).expect("other fixture root");
        let outside_sentinel = other_root.join("outside-sentinel.txt");
        fs::write(&outside_sentinel, b"unchanged").expect("outside sentinel");

        {
            let root_guard =
                ProtectedDirectoryGuard::acquire(&root, "protected rename fixture").expect("root");
            let other_guard =
                ProtectedDirectoryGuard::acquire(&other_root, "protected rename other fixture")
                    .expect("other root");
            let mut source = root_guard
                .try_clone("protected rename fixture")
                .expect("clone root");
            source
                .create_renameable_child("staging", "protected rename fixture")
                .expect("create staging");

            source
                .rename_direct_child_within_parent(
                    &root_guard,
                    "committed",
                    "protected rename fixture",
                )
                .expect("same-parent rename");
            let committed = root.join("committed");
            assert_eq!(source.path(), committed.as_path());
            assert!(committed.is_dir());
            assert!(!root.join("staging").exists());
            source
                .assert_stable("protected rename fixture")
                .expect("renamed identity remains stable");

            let invalid = source.rename_direct_child_within_parent(
                &root_guard,
                "..\\outside",
                "protected rename fixture",
            );
            assert!(invalid.is_err());
            assert!(committed.is_dir());

            let wrong_parent = source.rename_direct_child_within_parent(
                &other_guard,
                "escaped",
                "protected rename fixture",
            );
            assert!(wrong_parent.is_err());
            assert!(committed.is_dir());
            assert!(!other_root.join("escaped").exists());

            fs::create_dir(root.join("occupied")).expect("occupied destination");
            let occupied = source.rename_direct_child_within_parent(
                &root_guard,
                "occupied",
                "protected rename fixture",
            );
            assert!(occupied.is_err());
            assert!(committed.is_dir());
            assert!(root.join("occupied").is_dir());
            assert_eq!(
                fs::read(&outside_sentinel).expect("outside sentinel readback"),
                b"unchanged"
            );
        }

        fs::remove_dir_all(root).expect("fixture cleanup");
        fs::remove_dir_all(other_root).expect("other fixture cleanup");
    }

    #[cfg(windows)]
    #[test]
    fn descend_or_create_uses_real_open_existing_then_fresh_create_and_refuses_file_or_reparse() {
        let nonce = Uuid::new_v4();
        let root = std::env::temp_dir().join(format!("catdesk-open-create-{nonce}"));
        let outside = std::env::temp_dir().join(format!("catdesk-open-create-outside-{nonce}"));
        fs::create_dir(&root).expect("fixture root");
        fs::create_dir(&outside).expect("outside root");
        {
            let root_guard =
                ProtectedDirectoryGuard::acquire(&root, "open-or-create fixture").expect("root");

            // Fresh creation and a second ordinary production call both reach
            // `ProtectedDirectoryGuard::descend_or_create` and therefore the
            // exact shared helper used by reviewed_source_snapshot.
            let mut fresh = root_guard
                .try_clone("open-or-create fixture")
                .expect("clone fresh");
            fresh
                .descend_or_create("bytes", "open-or-create fixture")
                .expect("fresh create");
            fresh
                .descend_or_create("scripts", "open-or-create fixture")
                .expect("fresh nested create");
            fresh
                .assert_stable("open-or-create fixture")
                .expect("stable fresh");
            drop(fresh);
            let mut existing = root_guard
                .try_clone("open-or-create fixture")
                .expect("clone existing");
            existing
                .descend_or_create("bytes", "open-or-create fixture")
                .expect("open existing");
            existing
                .descend_or_create("scripts", "open-or-create fixture")
                .expect("open existing nested child");
            existing
                .assert_stable("open-or-create fixture")
                .expect("stable existing");

            fs::write(root.join("file-child"), b"not a directory").expect("file collision");
            let mut file = root_guard
                .try_clone("open-or-create fixture")
                .expect("clone file");
            assert!(
                file.descend_or_create("file-child", "open-or-create fixture")
                    .is_err()
            );

            if std::os::windows::fs::symlink_dir(&outside, root.join("redirect")).is_ok() {
                let mut link = root_guard
                    .try_clone("open-or-create fixture")
                    .expect("clone link");
                assert!(
                    link.descend_or_create("redirect", "open-or-create fixture")
                        .is_err()
                );
            }
        }
        let _ = fs::remove_file(root.join("redirect"));
        fs::remove_dir_all(root).expect("fixture cleanup");
        fs::remove_dir_all(outside).expect("outside cleanup");
    }

    #[test]
    fn open_or_create_status_discrimination_is_exact_and_fail_closed() {
        assert!(is_exact_nt_not_found(
            "protected filesystem child bytes is unavailable (0xc0000034)"
        ));
        assert!(is_exact_nt_create_collision(
            "protected filesystem child bytes is unavailable (0xc0000035)"
        ));
        for ambiguous in [
            "protected filesystem child bytes is unavailable (0xc0000034) trailing",
            "protected filesystem child bytes is unavailable (0xc0000035) trailing",
            "protected filesystem child bytes is unavailable (0xc0000022)",
            "arbitrary io failure",
        ] {
            assert!(!is_exact_nt_not_found(ambiguous));
            assert!(!is_exact_nt_create_collision(ambiguous));
        }
    }
}
