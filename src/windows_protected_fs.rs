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

#[cfg(all(test, windows))]
thread_local! {
    static REOPEN_DELETE_TEST_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        std::cell::RefCell::new(None);
}

#[cfg(all(test, windows))]
pub(crate) fn replace_reopen_delete_test_hook(hook: Option<Box<dyn FnOnce()>>) {
    REOPEN_DELETE_TEST_HOOK.with(|slot| *slot.borrow_mut() = hook);
}

#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::io::FromRawHandle;
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, OwnedHandle};

/// Handle-pinned directory identity used by protected filesystem consumers.
pub(crate) struct PinnedDirectory {
    pub(crate) path: PathBuf,
    #[cfg(windows)]
    pub(crate) handle: OwnedHandle,
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
            let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
            let information = handle_information(handle.as_raw_handle(), label)
                .map_err(|_| format!("{label} is unsafe"))?;
            if information.file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
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

    #[cfg(windows)]
    fn create_relative_with_sddl(
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
                parent.handle.as_raw_handle(),
                component,
                FILE_LIST_DIRECTORY
                    | FILE_ADD_FILE
                    | FILE_ADD_SUBDIRECTORY
                    | FILE_READ_ATTRIBUTES
                    | READ_CONTROL
                    | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                FILE_CREATE,
                FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
                descriptor,
                Some("reviewed source snapshot"),
            )?;
            let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
            let information = handle_information(handle.as_raw_handle(), label)?;
            if information.file_attributes
                & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY)
                != FILE_ATTRIBUTE_DIRECTORY
            {
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
        Self::open_relative_with_security(
            parent,
            component,
            disposition,
            allow_delete_share,
            false,
            label,
        )
    }

    #[allow(dead_code)] // The owner journal reader is absent from standalone helper binaries.
    pub(crate) fn open_relative_for_security(
        parent: &Self,
        component: &str,
        label: &str,
    ) -> Result<Self, String> {
        Self::open_relative_with_security(parent, component, FILE_OPEN, false, true, label)
    }

    fn open_relative_with_security(
        parent: &Self,
        component: &str,
        disposition: u32,
        allow_delete_share: bool,
        read_control: bool,
        label: &str,
    ) -> Result<Self, String> {
        validate_protected_component(component, label)?;
        parent.assert_stable(label)?;
        #[cfg(windows)]
        {
            let handle = nt_open_relative_with_security_descriptor_for_domain(
                parent.handle.as_raw_handle(),
                component,
                FILE_LIST_DIRECTORY
                    | FILE_ADD_FILE
                    | FILE_ADD_SUBDIRECTORY
                    | FILE_READ_ATTRIBUTES
                    | SYNCHRONIZE
                    | if read_control { READ_CONTROL } else { 0 }
                    | if allow_delete_share {
                        DELETE | READ_CONTROL
                    } else {
                        0
                    },
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
            let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
            let information = handle_information(handle.as_raw_handle(), label)?;
            if information.file_attributes
                & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY)
                != FILE_ATTRIBUTE_DIRECTORY
            {
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
            let _ = (
                parent,
                component,
                disposition,
                allow_delete_share,
                read_control,
            );
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
                    self.handle.as_raw_handle(),
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
                handle: unsafe { OwnedHandle::from_raw_handle(duplicate) },
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
            let information = handle_information(self.handle.as_raw_handle(), label)
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
        Ok(self.handle.as_raw_handle())
    }
}

/// A chain of handle-pinned directory identities. Ancestors stay open while
/// each child is opened relative to the previous RootDirectory; no consumer
/// may turn a later mutable pathname into authority.
pub(crate) struct ProtectedDirectoryGuard {
    directories: Vec<PinnedDirectory>,
}

#[cfg(windows)]
fn fixed_fixture_owner_sddl(sid: &str, label: &str) -> Result<String, String> {
    let parts = sid
        .strip_prefix("S-1-")
        .ok_or_else(|| format!("{label} owner SID is unsafe"))?;
    if sid.len() > 256
        || parts.is_empty()
        || parts
            .split('-')
            .any(|segment| segment.is_empty() || !segment.bytes().all(|byte| byte.is_ascii_digit()))
        || sid == "S-1-5-18"
    {
        return Err(format!("{label} owner SID is unsafe"));
    }
    // No inherited write ACEs; only the actual interactive token user and
    // LocalSystem receive explicit permissions. User ownership is not a
    // same-user isolation sandbox.
    Ok(format!(
        "O:{sid}G:{sid}D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"
    ))
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

    /// Clone only the pinned parent of one direct child. This is used for a
    /// post-disposition absence check without retaining a second child handle.
    #[cfg(windows)]
    pub(crate) fn direct_parent_guard(&self, label: &str) -> Result<Self, String> {
        if self.directories.len() != 2 {
            return Err(format!("{label} is not a direct child"));
        }
        self.assert_stable(label)?;
        Ok(Self::from_pinned_root(
            self.directories[0].try_clone(label)?,
        ))
    }

    /// Replace a pinned direct-child handle with a DELETE-capable handle
    /// under the same pinned parent. The old handle prevents opening DELETE,
    /// so it is dropped first; the reopened volume/file index must match or
    /// no deletion authority is returned. A concurrent namespace replacement
    /// can only make this fail closed.
    #[cfg(windows)]
    pub(crate) fn reopen_direct_child_for_delete(mut self, label: &str) -> Result<Self, String> {
        if self.directories.len() != 2 {
            return Err(format!("{label} is not a direct child"));
        }
        self.assert_stable(label)?;
        let child = self.directories.pop().expect("checked child count");
        let expected = child.identity;
        let name = child
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{label} child name unavailable"))?
            .to_owned();
        drop(child);
        #[cfg(test)]
        REOPEN_DELETE_TEST_HOOK.with(|slot| {
            if let Some(hook) = slot.borrow_mut().take() {
                hook();
            }
        });
        let reopened = PinnedDirectory::open_relative_for_delete(
            self.directories.last().expect("checked parent count"),
            &name,
            label,
        )?;
        if reopened.identity != expected {
            return Err(format!("{label} child identity drifted"));
        }
        self.directories.push(reopened);
        self.assert_stable(label)?;
        Ok(self)
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

    #[allow(dead_code)] // Used by the dormant fixture journal crash reader.
    pub(crate) fn descend_existing_for_security(
        &mut self,
        component: &str,
        label: &str,
    ) -> Result<(), String> {
        validate_protected_component(component, label)?;
        self.assert_stable(label)?;
        self.directories
            .push(PinnedDirectory::open_relative_for_security(
                self.directories.last().expect("guard has root"),
                component,
                label,
            )?);
        self.assert_stable(label)
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

    /// Fixed-policy disposable source directory creator. This API accepts no
    /// SDDL or SID from a caller: only an OS-token-derived ordinary interactive
    /// principal, under the canonical OS temp directory, can create one fresh
    /// CatDesk-generated direct child. It does not approve later deletion.
    #[cfg(windows)]
    pub(crate) fn create_owner_only_fixture_child(
        &mut self,
        component: &str,
        os_derived_interactive_sid: &str,
        label: &str,
    ) -> Result<(), String> {
        self.create_owner_only_generated_child(
            component,
            "catdesk-dev-fixture-",
            os_derived_interactive_sid,
            label,
        )
    }

    /// Separate generated owner-only directory for a write-ahead fixture
    /// journal. It is not a candidate source root or a cleanup authority.
    #[cfg(windows)]
    pub(crate) fn create_owner_only_fixture_journal_child(
        &mut self,
        component: &str,
        os_derived_interactive_sid: &str,
        label: &str,
    ) -> Result<(), String> {
        self.create_owner_only_generated_child(
            component,
            "catdesk-fixture-journal-",
            os_derived_interactive_sid,
            label,
        )
    }

    #[cfg(windows)]
    fn create_owner_only_generated_child(
        &mut self,
        component: &str,
        prefix: &str,
        os_derived_interactive_sid: &str,
        label: &str,
    ) -> Result<(), String> {
        let temp = fs::canonicalize(std::env::temp_dir())
            .map_err(|_| format!("{label} temp authority unavailable"))?;
        if self.path() != temp {
            return Err(format!("{label} parent is outside OS temp"));
        }
        let Some(id) = component.strip_prefix(prefix) else {
            return Err(format!("{label} component outside generated scope"));
        };
        let parsed =
            Uuid::parse_str(id).map_err(|_| format!("{label} generated identity invalid"))?;
        if parsed.simple().to_string() != id {
            return Err(format!("{label} generated identity invalid"));
        }
        // The only in-tree production caller obtains this SID from the fixed
        // process-token routine. Keep generic filesystem code independent of
        // the supervisor module: standalone utility bins import this module
        // but deliberately do not import the supervisor principal logic.
        let sddl = fixed_fixture_owner_sddl(os_derived_interactive_sid, label)?;
        self.assert_stable(label)?;
        let pinned = PinnedDirectory::create_relative_with_sddl(
            self.directories.last().expect("guard has root"),
            component,
            &sddl,
            label,
        )?;
        self.directories.push(pinned);
        self.assert_stable(label)
    }

    /// Compare the real OWNER/GROUP/DACL read directly from the pinned child
    /// HANDLE with the exact policy used at creation. A matching pathname or
    /// well-formed generated name is never sufficient identity evidence.
    #[cfg(windows)]
    pub(crate) fn verify_owner_only_fixture_descriptor(
        &self,
        os_derived_interactive_sid: &str,
        label: &str,
    ) -> Result<(), String> {
        self.verify_owner_only_generated_descriptor(
            "catdesk-dev-fixture-",
            os_derived_interactive_sid,
            label,
        )
    }

    #[cfg(windows)]
    pub(crate) fn verify_owner_only_fixture_journal_descriptor(
        &self,
        os_derived_interactive_sid: &str,
        label: &str,
    ) -> Result<(), String> {
        self.verify_owner_only_generated_descriptor(
            "catdesk-fixture-journal-",
            os_derived_interactive_sid,
            label,
        )
    }

    #[cfg(windows)]
    fn verify_owner_only_generated_descriptor(
        &self,
        prefix: &str,
        os_derived_interactive_sid: &str,
        label: &str,
    ) -> Result<(), String> {
        let temp = fs::canonicalize(std::env::temp_dir())
            .map_err(|_| format!("{label} temp authority unavailable"))?;
        if self.directories.len() != 2 || self.directories[0].path != temp {
            return Err(format!("{label} owner scope is invalid"));
        }
        let Some(name) = self.path().file_name().and_then(|name| name.to_str()) else {
            return Err(format!("{label} owner name is invalid"));
        };
        let Some(id) = name.strip_prefix(prefix) else {
            return Err(format!("{label} owner name is invalid"));
        };
        if self.path().parent() != Some(temp.as_path())
            || Uuid::parse_str(id)
                .map(|id| id.simple().to_string())
                .ok()
                .as_deref()
                != Some(id)
        {
            return Err(format!("{label} owner name is invalid"));
        }
        let expected = fixed_fixture_owner_sddl(os_derived_interactive_sid, label)?;
        self.assert_stable(label)?;
        let actual = security_descriptor_sddl_from_handle(self.last_handle(label)?, label)?;
        if !fixed_fixture_descriptor_matches(&actual, &expected) {
            return Err(format!(
                "{label} owner DACL is unverified ({})",
                fixture_acl_safe_shape(&actual, os_derived_interactive_sid)
            ));
        }
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
        Ok(self
            .directories
            .last()
            .expect("guard has root")
            .handle
            .as_raw_handle())
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
pub(crate) fn retain_reviewed_regular_artifact<P: PinnedParent>(
    parent: &P,
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

/// Reopen a previously retained reviewed file for a future owner-bound
/// retirement transaction. The caller must first release its read lease,
/// retain the pinned parent, and compare every expected child before applying
/// disposition to any handle. This function itself never deletes a file.
#[cfg(windows)]
#[allow(dead_code)] // B3B2B2B owner transaction is not connected to this primitive yet.
pub(crate) fn open_reviewed_regular_for_delete<P: PinnedParent>(
    parent: &P,
    component: &str,
    expected: &ReviewedArtifactIdentity,
    max_bytes: u64,
    label: &str,
) -> Result<fs::File, String> {
    if max_bytes == 0
        || expected.byte_length > max_bytes
        || expected.sha256.len() != 64
        || !expected.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || expected.object_identity.len() != 26
    {
        return Err(format!("{label} identity is unsafe"));
    }
    validate_component(component, label)?;
    parent.assert_stable(label)?;
    let mut file = open_relative_regular_file_with_disposition_and_share(
        parent,
        component,
        FILE_OPEN,
        FILE_READ_DATA | FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE,
        FILE_SHARE_READ,
        label,
    )?;
    let information = handle_information(file.as_raw_handle().cast(), label)?;
    let identity = format!(
        "{:08x}:{:08x}:{:08x}",
        information.volume_serial_number, information.file_index_high, information.file_index_low
    );
    if information.file_attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0
        || information.number_of_links != 1
        || identity != expected.object_identity
    {
        return Err(format!("{label} identity drifted"));
    }
    let length = file
        .metadata()
        .map_err(|_| format!("{label} is unavailable"))?
        .len();
    if length != expected.byte_length {
        return Err(format!("{label} identity drifted"));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    Read::by_ref(&mut file)
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| format!("{label} is unavailable"))?;
    if bytes.len() as u64 != length
        || format!("{:x}", Sha256::digest(&bytes)) != expected.sha256.to_ascii_lowercase()
    {
        return Err(format!("{label} digest drifted"));
    }
    parent.assert_stable(label)?;
    Ok(file)
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

/// Only use for disposable fixture data. A single opened, no-follow child
/// handle is pinned throughout the read and must have no other hardlink name.
pub(crate) fn read_relative_regular_unique<P: PinnedParent>(
    parent: &P,
    name: &str,
    limit: u64,
    label: &str,
) -> Result<Vec<u8>, String> {
    validate_component(name, label)?;
    parent.assert_stable(label)?;
    let mut file = open_relative_regular_file(parent, name, label)?;
    #[cfg(windows)]
    {
        let info = handle_information(file.as_raw_handle().cast(), label)?;
        if info.number_of_links != 1 {
            return Err(format!("{label} is unsafe"));
        }
    }
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
            dispose_opened_object(file.as_raw_handle().cast(), label)?;
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

/// Apply native delete disposition to one already opened DELETE-authorized
/// Windows object. The caller must independently prove ownership, type,
/// identity, contents, and directory inventory before invoking this primitive.
/// It accepts no pathname and never enumerates or opens another object.
#[cfg(windows)]
pub(crate) fn dispose_opened_object(handle: *mut c_void, label: &str) -> Result<(), String> {
    const FILE_DISPOSITION_INFORMATION_EX_CLASS: u32 = 64;
    const FILE_DISPOSITION_DELETE: u32 = 1;
    const STATUS_INVALID_INFO_CLASS: i32 = 0xc000_0003_u32 as i32;
    const STATUS_INVALID_PARAMETER: i32 = 0xc000_000d_u32 as i32;
    let mut status = IO_STATUS_BLOCK {
        status: 0,
        information: 0,
    };
    let mut flags = FILE_DISPOSITION_DELETE;
    let extended = unsafe {
        NtSetInformationFile(
            handle,
            &mut status,
            (&mut flags as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
            FILE_DISPOSITION_INFORMATION_EX_CLASS,
        )
    };
    if extended >= 0 {
        return Ok(());
    }
    if extended != STATUS_INVALID_INFO_CLASS && extended != STATUS_INVALID_PARAMETER {
        return Err(format!("{label} deletion is unavailable"));
    }
    let mut delete = 1_u8;
    let fallback = unsafe {
        NtSetInformationFile(
            handle,
            &mut status,
            (&mut delete as *mut u8).cast(),
            1,
            FILE_DISPOSITION_INFO_CLASS,
        )
    };
    if fallback < 0 {
        return Err(format!("{label} deletion is unavailable"));
    }
    Ok(())
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
#[cfg(windows)]
const READ_CONTROL: u32 = 0x0002_0000;
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
#[link(name = "advapi32")]
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
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        definition: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        descriptor_size: *mut u32,
    ) -> i32;
    fn GetSecurityInfo(
        handle: *mut c_void,
        object_type: u32,
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
        security_information: u32,
        string_descriptor: *mut *mut u16,
        length: *mut u32,
    ) -> i32;
    fn GetSecurityDescriptorOwner(
        security_descriptor: *mut c_void,
        owner: *mut *mut c_void,
        owner_defaulted: *mut i32,
    ) -> i32;
    fn EqualSid(first: *mut c_void, second: *mut c_void) -> i32;
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

/// Compare SDDL principals by their actual Windows SID bytes. The Windows
/// serializer can emit a local account alias such as `LA` for a token SID
/// supplied numerically at creation. Parse each principal through the OS's
/// full descriptor parser, then compare OWNER SIDs; no textual shortcut, RID
/// suffix guess, group membership, or additional trustee is accepted.
#[cfg(windows)]
fn fixture_principals_equal(left: &str, right: &str) -> bool {
    fn owner_from_principal(principal: &str) -> Option<(*mut c_void, *mut c_void)> {
        if principal.is_empty()
            || principal.len() > 256
            || !principal
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return None;
        }
        let descriptor = security_descriptor_from_sddl(
            &format!("O:{principal}G:SYD:P(A;;FA;;;SY)"),
            "fixture SID equivalence",
        )
        .ok()?;
        let mut owner = std::ptr::null_mut();
        let mut owner_defaulted = 0;
        let ok =
            unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut owner_defaulted) };
        if ok == 0 || owner.is_null() {
            unsafe { LocalFree(descriptor) };
            return None;
        }
        Some((descriptor, owner))
    }
    let Some((left_descriptor, left_sid)) = owner_from_principal(left) else {
        return false;
    };
    let right_sid = owner_from_principal(right);
    let matches = right_sid
        .as_ref()
        .is_some_and(|(_, right_sid)| unsafe { EqualSid(left_sid, *right_sid) != 0 });
    if let Some((descriptor, _)) = right_sid {
        unsafe { LocalFree(descriptor) };
    }
    unsafe { LocalFree(left_descriptor) };
    matches
}

/// Exact ACL policy equivalence. Canonical Windows SDDL may include the
/// auto-inherited AI flag even for a protected DACL. No unknown ACEs, flags,
/// principals, or reordered broad permissions are tolerated.
#[cfg(windows)]
fn fixture_acl_safe_shape(actual: &str, owner_sid: &str) -> String {
    // Bounded diagnostics only: never expose owner, group, or foreign SIDs.
    let owner_matches = actual
        .strip_prefix("O:")
        .and_then(|s| s.split("G:").next())
        .is_some_and(|s| fixture_principals_equal(s, owner_sid));
    let dacl = actual.split_once("D:").map(|(_, d)| d).unwrap_or("");
    let flags = dacl.split('(').next().unwrap_or("");
    let mut entries = Vec::new();
    for fragment in dacl.split('(').skip(1).take(4) {
        let fields = fragment
            .split(')')
            .next()
            .unwrap_or("")
            .split(';')
            .collect::<Vec<_>>();
        let principal = match fields.get(5).copied().unwrap_or("") {
            "SY" | "S-1-5-18" => "SYSTEM",
            sid if fixture_principals_equal(sid, owner_sid) => "OWNER",
            _ => "OTHER",
        };
        let ty = fields.first().copied().unwrap_or("?");
        let ace_flags = fields.get(1).copied().unwrap_or("?");
        let rights = fields.get(2).copied().unwrap_or("?");
        entries.push(format!("{ty}/{ace_flags}/{rights}/{principal}"));
    }
    format!(
        "owner_matches={owner_matches}, dacl_flags={flags}, ace_count={}, entries={}",
        dacl.matches('(').count(),
        entries.join(",")
    )
}

#[cfg(windows)]
fn fixed_fixture_descriptor_matches(actual: &str, expected: &str) -> bool {
    // SDDL string serialization is not stable across Windows versions:
    // readback may re-order ACEs, add the auto-inheritance flag, map FA to a
    // numeric FILE_ALL_ACCESS mask, and retain an OS-selected primary group.
    // Compare the actual *security-relevant* owner and protected DACL shape,
    // never downgrade to string containment or accept additional ACEs.
    fn normalized(value: &str) -> Option<(String, Vec<(String, String)>)> {
        let value = value.trim_end_matches('\0');
        if value.contains('\0') || !value.starts_with("O:") {
            return None;
        }
        let d = value.find("D:")?;
        let owner_section = &value[2..d];
        let owner = owner_section.split("G:").next()?;
        if owner.is_empty() || owner.contains(['(', ')', ';']) {
            return None;
        }
        let dacl = &value[d + 2..];
        let pos = dacl.find('(')?;
        if !matches!(&dacl[..pos], "P" | "PAI" | "PAR" | "PAIAR" | "PARAI") {
            return None;
        }
        let mut cursor = &dacl[pos..];
        let mut aces = Vec::new();
        while !cursor.is_empty() {
            cursor = cursor.strip_prefix('(')?;
            let end = cursor.find(')')?;
            let ace = &cursor[..end];
            cursor = &cursor[end + 1..];
            let fields = ace.split(';').collect::<Vec<_>>();
            if fields.len() != 6
                || fields[0] != "A"
                || !matches!(fields[1], "OICI" | "CIOI")
                || !fields[3].is_empty()
                || !fields[4].is_empty()
            {
                return None;
            }
            let mask = if fields[2] == "FA" || fields[2].eq_ignore_ascii_case("0x001f01ff") {
                "FA"
            } else {
                return None;
            };
            let sid = if fields[5] == "S-1-5-18" {
                "SY"
            } else {
                fields[5]
            };
            if sid.is_empty() {
                return None;
            }
            aces.push((sid.to_owned(), mask.to_owned()));
            if aces.len() > 2 {
                return None;
            }
        }
        aces.sort();
        Some((owner.to_owned(), aces))
    }
    let Some((expected_owner, expected_aces)) = normalized(expected) else {
        return false;
    };
    let Some((actual_owner, actual_aces)) = normalized(actual) else {
        return false;
    };
    if !fixture_principals_equal(&actual_owner, &expected_owner)
        || actual_aces.len() != expected_aces.len()
    {
        return false;
    }
    // Pair every required ACE with a DISTINCT exact-SID/mask match. A
    // duplicate owner ACE may never stand in for the required SYSTEM ACE.
    let mut unmatched = vec![true; actual_aces.len()];
    for (expected_sid, expected_mask) in expected_aces {
        let Some(index) = actual_aces
            .iter()
            .enumerate()
            .find_map(|(index, (sid, mask))| {
                (unmatched[index]
                    && mask == &expected_mask
                    && fixture_principals_equal(sid, &expected_sid))
                .then_some(index)
            })
        else {
            return false;
        };
        unmatched[index] = false;
    }
    true
}

/// Query only the *already opened* directory HANDLE; never GetFileSecurityW
/// by pathname. No authority is obtained by accepting a caller-created path.
#[cfg(windows)]
fn security_descriptor_sddl_from_handle(
    handle: *mut c_void,
    label: &str,
) -> Result<String, String> {
    const SE_FILE_OBJECT: u32 = 1;
    const SECURITY_INFORMATION: u32 = 0x0000_0001 | 0x0000_0002 | 0x0000_0004;
    let mut descriptor: *mut c_void = std::ptr::null_mut();
    let result = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if result != 0 || descriptor.is_null() {
        if !descriptor.is_null() {
            unsafe { LocalFree(descriptor) };
        }
        return Err(format!("{label} owner security descriptor unavailable"));
    }
    let mut sddl: *mut u16 = std::ptr::null_mut();
    let mut length = 0u32;
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            1,
            SECURITY_INFORMATION,
            &mut sddl,
            &mut length,
        )
    };
    let text = if converted == 0 || sddl.is_null() || length == 0 || length > 8192 {
        Err(format!("{label} owner security descriptor unavailable"))
    } else {
        let wide = unsafe { std::slice::from_raw_parts(sddl, length as usize) };
        String::from_utf16(wide).map_err(|_| format!("{label} owner SDDL invalid"))
    };
    if !sddl.is_null() {
        unsafe { LocalFree(sddl.cast()) };
    }
    unsafe { LocalFree(descriptor) };
    text
}

#[cfg(windows)]
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

    #[cfg(windows)]
    #[test]
    fn reviewed_delete_handle_requires_exact_identity_and_digest() {
        let dir = std::env::temp_dir().join(format!(
            "catdesk-reviewed-delete-{}",
            Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&dir).expect("create test-owned root");
        let leaf = "candidate.txt";
        let bytes = b"reviewed exact bytes";
        std::fs::write(dir.join(leaf), bytes).expect("create test file");
        let parent = PinnedDirectory::acquire(&dir, "test reviewed delete").expect("pin root");
        let digest = format!("{:x}", Sha256::digest(bytes));
        let retained = retain_reviewed_regular_artifact(
            &parent,
            leaf,
            &digest,
            512 * 1024,
            "test reviewed delete",
        )
        .expect("retain exact file");
        let expected = retained.identity.clone();
        assert!(
            open_reviewed_regular_for_delete(
                &parent,
                leaf,
                &expected,
                512 * 1024,
                "test reviewed delete"
            )
            .is_err(),
            "active read lease must block DELETE access"
        );
        drop(retained);
        let mut wrong = expected.clone();
        wrong.object_identity = "00000000:00000000:00000000".into();
        assert!(
            open_reviewed_regular_for_delete(
                &parent,
                leaf,
                &wrong,
                512 * 1024,
                "test reviewed delete"
            )
            .is_err()
        );
        wrong = expected.clone();
        wrong.sha256 = "0".repeat(64);
        assert!(
            open_reviewed_regular_for_delete(
                &parent,
                leaf,
                &wrong,
                512 * 1024,
                "test reviewed delete"
            )
            .is_err()
        );
        let opened = open_reviewed_regular_for_delete(
            &parent,
            leaf,
            &expected,
            512 * 1024,
            "test reviewed delete",
        )
        .expect("exact reviewed object opens for delete");
        assert!(
            std::fs::write(dir.join(leaf), b"replacement").is_err(),
            "the delete handle must prevent later write access"
        );
        dispose_opened_object(opened.as_raw_handle().cast(), "test reviewed delete")
            .expect("dispose exact test-owned file");
        drop(opened);
        assert!(!dir.join(leaf).exists());
        drop(parent);
        std::fs::remove_dir(&dir).expect("remove empty test root");
    }

    #[cfg(windows)]
    #[test]
    fn reviewed_delete_handle_refuses_hardlink_and_changed_bytes() {
        let dir = std::env::temp_dir().join(format!(
            "catdesk-reviewed-delete-negative-{}",
            Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&dir).expect("create test-owned root");
        let leaf = "candidate.txt";
        let bytes = b"reviewed bytes";
        std::fs::write(dir.join(leaf), bytes).expect("create test file");
        let parent = PinnedDirectory::acquire(&dir, "test reviewed delete").expect("pin root");
        let digest = format!("{:x}", Sha256::digest(bytes));
        let retained = retain_reviewed_regular_artifact(
            &parent,
            leaf,
            &digest,
            512 * 1024,
            "test reviewed delete",
        )
        .expect("retain exact file");
        let expected = retained.identity.clone();
        drop(retained);
        std::fs::hard_link(dir.join(leaf), dir.join("alias.txt"))
            .expect("add hardlink inside test root");
        assert!(
            open_reviewed_regular_for_delete(
                &parent,
                leaf,
                &expected,
                512 * 1024,
                "test reviewed delete"
            )
            .is_err(),
            "multi-link object must not be disposable"
        );
        std::fs::remove_file(dir.join("alias.txt")).expect("remove test alias");
        std::fs::write(dir.join(leaf), b"changed bytes!").expect("change test file");
        assert!(
            open_reviewed_regular_for_delete(
                &parent,
                leaf,
                &expected,
                512 * 1024,
                "test reviewed delete"
            )
            .is_err(),
            "changed bytes must not be disposable"
        );
        drop(parent);
        std::fs::remove_file(dir.join(leaf)).expect("remove test file");
        std::fs::remove_dir(&dir).expect("remove empty test root");
    }

    #[cfg(windows)]
    #[test]
    fn fixture_owner_acl_readback_rejects_unreviewed_aces_and_flags() {
        let sid = "S-1-5-21-111-222-333-1001";
        let expected = fixed_fixture_owner_sddl(sid, "fixture").unwrap();
        assert!(fixed_fixture_descriptor_matches(&expected, &expected));
        assert!(fixed_fixture_descriptor_matches(
            &expected.replacen("D:P", "D:PAI", 1),
            &expected
        ));
        // Windows may normalize ACE order and symbolic/full file mask while
        // preserving precisely the same two allowed trustees and rights.
        let canonicalized = format!("O:{sid}G:SYD:PAI(A;OICI;0x001f01ff;;;SY)(A;OICI;FA;;;{sid})");
        assert!(fixed_fixture_descriptor_matches(&canonicalized, &expected));
        // GitHub Windows runners may serialize a recognized SID as its SDDL
        // abbreviation instead of the numeric string supplied at creation.
        let administrators = "S-1-5-32-544";
        let administrative_policy = fixed_fixture_owner_sddl(administrators, "fixture").unwrap();
        let administrative_alias = "O:BAG:SYD:P(A;OICI;FA;;;BA)(A;OICI;FA;;;SY)";
        assert!(fixture_principals_equal("BA", administrators));
        assert!(!fixture_principals_equal("BU", administrators));
        assert!(fixed_fixture_descriptor_matches(
            administrative_alias,
            &administrative_policy
        ));
        assert!(!fixed_fixture_descriptor_matches(
            "O:BAG:SYD:P(A;OICI;FA;;;BU)(A;OICI;FA;;;SY)",
            &administrative_policy
        ));
        assert!(!fixed_fixture_descriptor_matches(
            "O:BAG:SYD:P(A;OICI;FA;;;BA)(A;OICI;FA;;;BA)",
            &administrative_policy
        ));
        for extra in [
            format!("{expected}(A;;FA;;;WD)"),
            expected.replacen("D:P", "D:", 1),
            expected.replacen(";;;SY)", ";;;BU)", 1),
            expected.replacen("O:", "O:S-1-5-18G:", 1),
        ] {
            assert!(!fixed_fixture_descriptor_matches(&extra, &expected));
        }
    }

    #[cfg(windows)]
    #[test]
    fn fixture_owner_acl_is_fixed_and_rejects_sid_injection() {
        let sid = "S-1-5-21-111-222-333-1001";
        let acl = fixed_fixture_owner_sddl(sid, "fixture").unwrap();
        assert_eq!(
            acl,
            format!("O:{sid}G:{sid}D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)")
        );
        assert!(!acl.contains(";;;WD"));
        assert!(!acl.contains(";;;BU"));
        assert!(!acl.contains(";;;AU"));
        for invalid in [
            "",
            "S-1-5-18",
            "S-1-5-21-1);(A;;FA;;;WD",
            "S-1-5-21--2",
            "S-1-5-21-abc",
        ] {
            assert!(fixed_fixture_owner_sddl(invalid, "fixture").is_err());
        }
    }

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
        assert_eq!(count(shared, "pub(crate) handle: OwnedHandle,"), 1);
        assert_eq!(count(snapshot, "pub(crate) handle: OwnedHandle,"), 0);
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
