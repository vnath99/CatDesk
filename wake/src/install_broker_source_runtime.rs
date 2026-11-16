//! Runtime-MAP intake over the same handle-relative broker source primitives.
//! These pins establish bytes, never release qualification or installation.
use super::*;
use crate::install_broker_approval::ArtifactV1;
use crate::install_broker_stage_writer::StageSecurityMode;
use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub(crate) enum RuntimeSourceRoleV3 {
    Candidate,
    Previous,
}

#[cfg(test)]
type PublicationHook = Box<dyn FnOnce()>;
#[cfg(test)]
thread_local! {
    static PUBLICATION_AFTER_RELEASE: std::cell::RefCell<Option<PublicationHook>> = const { std::cell::RefCell::new(None) };
    static PUBLICATION_BEFORE_RENAME: std::cell::RefCell<Option<PublicationHook>> = const { std::cell::RefCell::new(None) };
}

struct PublicationPackageV3 {
    stage_name: String,
    staged: RuntimePackagePinsV3,
    existing: Option<RuntimePackagePinsV3>,
    identity: crate::install_broker_offline_v2::FullFileIdentityV2,
    security: Vec<(String, String)>,
}

type PackageSecurityV3 = (
    crate::install_broker_offline_v2::FullFileIdentityV2,
    Vec<(String, String)>,
);

/// Exact V3 byte publication. Only a matching admitted V3 ledger attempt can
/// consume this value; the rename primitive receives retained native handles.
pub(crate) struct RuntimePackagePublicationV3 {
    root: OwnedHandle,
    versions: OwnedHandle,
    exclusive_versions: Option<OwnedHandle>,
    root_info: FileInfo,
    versions_info: FileInfo,
    root_identity: crate::install_broker_offline_v2::FullFileIdentityV2,
    versions_identity: crate::install_broker_offline_v2::FullFileIdentityV2,
    payload_sha256: String,
    envelope_sha256: String,
    packages: Vec<PublicationPackageV3>,
    previous: Option<RuntimePackagePinsV3>,
    security: StageSecurityMode,
}

impl RuntimePackagePublicationV3 {
    pub(crate) fn prepare(
        root: &std::path::Path,
        payload: &crate::install_broker_approval::same_state::SameStateRootSelectionApprovalV3,
        envelope_sha256: &str,
        ledger: &crate::install_broker_ledger::AdmittedSameStateLedgerLeaseV3<'_, '_>,
        policy: &crate::install_broker_policy::BrokerProtectedPolicyV1,
    ) -> Result<Self> {
        use crate::install_broker_offline_pins::full_identity_of_handle;
        let security = StageSecurityMode::from_same_state_policy(policy)?;
        if policy.same_state_resource_v2()?.identity != payload.state_snapshot.root {
            return Err("WAKE_SAME_STATE_PUBLICATION_BINDING_INVALID".into());
        }
        if payload.operation != "SELECT_SAME_STATE_RELEASE"
            || envelope_sha256.len() != 64
            || !envelope_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("WAKE_SAME_STATE_PUBLICATION_BINDING_INVALID".into());
        }
        // Retained signed records keep their historical commitment domain.
        // Delegated local updates use a separate, explicitly unsigned domain.
        let mut signed_payload = if ledger.binding().local_user_authorized() {
            b"CatDeskWakeLocalUserSelectionV1\0".to_vec()
        } else {
            b"CatDeskWakeSameStateRootSelectionApprovalV3\0".to_vec()
        };
        signed_payload.extend(
            serde_json::to_vec(payload)
                .map_err(|_| "WAKE_SAME_STATE_PUBLICATION_BINDING_INVALID")?,
        );
        let payload_sha256 = format!("{:x}", Sha256::digest(signed_payload));
        if ledger.phase() != crate::install_broker_ledger::InstallPhaseV1::Staged
            || ledger.binding().is_rollback()
            || ledger.publication_attempt_present()?
            || ledger.binding().signed_payload_sha256 != payload_sha256
            || ledger.binding().signed_envelope_sha256 != envelope_sha256
            || ledger.binding().state_root != payload.state_snapshot.root
        {
            return Err("WAKE_SAME_STATE_PUBLICATION_ATTEMPT_ATTENTION".into());
        }
        let root = open_path_directory_with_sharing(root.as_os_str(), FILE_SHARE_READ)?;
        if full_identity_of_handle(root.as_raw_handle())? != payload.state_snapshot.root {
            return Err("WAKE_SAME_STATE_PUBLICATION_BINDING_INVALID".into());
        }
        let versions = open_relative(&root, "versions", true)?;
        let exclusive_versions =
            open_relative_object(&root, "versions", true, false, FILE_SHARE_READ)?;
        let mut result = Self {
            root_info: info(root.as_raw_handle(), true)?,
            versions_info: info(versions.as_raw_handle(), true)?,
            root_identity: full_identity_of_handle(root.as_raw_handle())?,
            versions_identity: full_identity_of_handle(versions.as_raw_handle())?,
            root,
            versions,
            exclusive_versions: Some(exclusive_versions),
            payload_sha256,
            envelope_sha256: envelope_sha256.into(),
            packages: Vec::new(),
            previous: None,
            security,
        };
        let allowed = [
            payload.candidate.manifest.package_id.as_str(),
            payload
                .rollback_authority
                .release()
                .manifest
                .package_id
                .as_str(),
        ];
        for version in [
            &payload.candidate.manifest.version,
            &payload.rollback_authority.release().manifest.version,
        ] {
            result.require_version_policy(version, &allowed)?;
        }
        for (role, release) in [
            ("previous", payload.rollback_authority.release()),
            ("candidate", &payload.candidate),
        ] {
            let names = crate::install_broker_reader::directory_inventory(&result.versions, 4096)?;
            let existing = if names.contains(&release.manifest.package_id) {
                let pins = RuntimePackagePinsV3::open(
                    result
                        .root
                        .try_clone()
                        .map_err(|_| "WAKE_SAME_STATE_PUBLICATION_ATTENTION")?,
                    &["versions", &release.manifest.package_id],
                )?;
                if pins.manifest() != &release.manifest
                    || pins.runtime_manifest_sha256() != release.runtime_manifest_sha256
                {
                    return Err("HOST_INSTALL_ARTIFACT_IDENTITY_CONFLICT".into());
                }
                Some(pins)
            } else {
                None
            };
            if role == "previous" && existing.is_some() {
                result.previous = existing;
                continue;
            }
            let stage_name = format!(".staging-v3-{}-{role}", &envelope_sha256[..32]);
            let staged = RuntimePackagePinsV3::open(
                result
                    .root
                    .try_clone()
                    .map_err(|_| "WAKE_SAME_STATE_PUBLICATION_ATTENTION")?,
                &["versions", &stage_name],
            )?;
            if staged.manifest() != &release.manifest
                || staged.runtime_manifest_sha256() != release.runtime_manifest_sha256
            {
                return Err("WAKE_SAME_STATE_PUBLISHED_PACKAGE_MISMATCH".into());
            }
            let identity = full_identity_of_handle(
                staged
                    .ancestors
                    .last()
                    .ok_or("WAKE_SAME_STATE_PUBLICATION_ATTENTION")?
                    .as_raw_handle(),
            )?;
            let (security_identity, security) =
                package_security(&result.versions, &stage_name, &result.security)?;
            if security_identity != identity {
                return Err("WAKE_SAME_STATE_PUBLICATION_OBJECT_CHANGED".into());
            }
            result.packages.push(PublicationPackageV3 {
                stage_name,
                staged,
                existing,
                identity,
                security,
            });
        }
        result.revalidate_parents()?;
        Ok(result)
    }

    fn revalidate_parents(&self) -> Result<()> {
        use crate::install_broker_offline_pins::full_identity_of_handle;
        if !same_object(&self.root_info, &info(self.root.as_raw_handle(), true)?)
            || !same_object(
                &self.versions_info,
                &info(self.versions.as_raw_handle(), true)?,
            )
            || full_identity_of_handle(self.root.as_raw_handle())? != self.root_identity
            || full_identity_of_handle(self.versions.as_raw_handle())? != self.versions_identity
        {
            return Err("WAKE_SAME_STATE_PUBLICATION_PARENT_CHANGED".into());
        }
        let reopened = open_relative(&self.root, "versions", true)?;
        if !same_object(&self.versions_info, &info(reopened.as_raw_handle(), true)?)
            || full_identity_of_handle(reopened.as_raw_handle())? != self.versions_identity
        {
            return Err("WAKE_SAME_STATE_PUBLICATION_PARENT_CHANGED".into());
        }
        Ok(())
    }

    fn require_version_policy(&self, version: &str, allowed: &[&str]) -> Result<()> {
        for name in crate::install_broker_reader::directory_inventory(&self.versions, 4096)? {
            if name.starts_with(".staging-") {
                continue;
            }
            let directory = open_relative(&self.versions, &name, true)?;
            let entries = crate::install_broker_reader::directory_inventory(&directory, 4096)?;
            if !entries.contains(MANIFEST_NAME) {
                continue;
            }
            let mut file = File::from(open_relative(&directory, MANIFEST_NAME, false)?);
            let (raw, _) = read_manifest(&mut file)?;
            let Ok(manifest) = serde_json::from_slice::<Value>(&raw) else {
                continue;
            };
            if manifest["version"].as_str() == Some(version) && !allowed.contains(&name.as_str()) {
                return Err("HOST_INSTALL_VERSION_IDENTITY_CONFLICT".into());
            }
        }
        Ok(())
    }

    pub(crate) fn publish(
        mut self,
        ledger: &crate::install_broker_ledger::AdmittedSameStateLedgerLeaseV3<'_, '_>,
    ) -> Result<Vec<RuntimePackagePinsV3>> {
        use crate::install_broker_offline_pins::full_identity_of_handle;
        use crate::install_broker_stage_writer::{
            open_publication_child, rename_candidate_noreplace,
        };
        if ledger.phase() != crate::install_broker_ledger::InstallPhaseV1::Staged
            || ledger.binding().is_rollback()
            || !ledger.publication_attempt_present()?
            || ledger.binding().signed_payload_sha256 != self.payload_sha256
            || ledger.binding().signed_envelope_sha256 != self.envelope_sha256
            || full_identity_of_handle(self.root.as_raw_handle())? != ledger.binding().state_root
        {
            return Err("WAKE_SAME_STATE_PUBLICATION_BINDING_INVALID".into());
        }
        self.revalidate_parents()?;
        // A durable attempt precedes the parent's sharing handoff. A separate
        // no-DELETE versions handle stays retained; there is no pathname reopen.
        drop(self.exclusive_versions.take());
        let mut published = Vec::new();
        if let Some(mut previous) = self.previous.take() {
            previous.revalidate()?;
            published.push(previous);
        }
        for mut package in std::mem::take(&mut self.packages) {
            self.revalidate_parents()?;
            package.staged.revalidate()?;
            if let Some(mut existing) = package.existing.take() {
                existing.revalidate()?;
                published.push(existing);
                continue;
            }
            let expected = package.staged.manifest.clone();
            let raw_manifest = package.staged.runtime_manifest.clone();
            let byte_identities = std::iter::once(&package.staged.manifest_file)
                .chain(package.staged.artifacts.iter())
                .map(|file| full_identity_of_handle(file.as_raw_handle()))
                .collect::<Result<Vec<_>>>()?;
            let RuntimePackagePinsV3 {
                ancestors,
                mut manifest_file,
                mut artifacts,
                ..
            } = package.staged;
            // Protected ownership/DACLs existed at FILE_CREATE. Release only
            // directory read pins to acquire the exact no-delete-share source.
            drop(ancestors);
            #[cfg(test)]
            if let Some(hook) = PUBLICATION_AFTER_RELEASE.with(|slot| slot.borrow_mut().take()) {
                hook();
            }
            let source = open_publication_child(&self.versions, &package.stage_name, true)?;
            if full_identity_of_handle(source.as_raw_handle())? != package.identity {
                return Err("WAKE_SAME_STATE_PUBLICATION_OBJECT_CHANGED".into());
            }
            let read_directory = open_relative_object_secured(
                &self.versions,
                &package.stage_name,
                true,
                false,
                7,
                None,
                true,
            )?;
            let (security_identity, security) =
                package_security(&self.versions, &package.stage_name, &self.security)?;
            if security_identity != package.identity || security != package.security {
                return Err("WAKE_BROKER_PUBLICATION_ACL_MISMATCH".into());
            }
            require_publication_bytes(
                &read_directory,
                &mut manifest_file,
                &mut artifacts,
                &expected,
                &raw_manifest,
            )?;
            self.revalidate_parents()?;
            // NTFS refuses a directory rename while descendant files remain
            // open, regardless of their delete sharing. Only after the durable
            // attempt and exact DELETE/identity/ACL handoff may these close.
            // The protected stage denies untrusted writes and DELETE_CHILD;
            // the source handle denies deletion via its writable parent.
            drop(manifest_file);
            drop(artifacts);
            #[cfg(test)]
            if let Some(hook) = PUBLICATION_BEFORE_RENAME.with(|slot| slot.borrow_mut().take()) {
                hook();
            }
            rename_candidate_noreplace(&source, &self.versions, &expected.package_id)?;
            if full_identity_of_handle(source.as_raw_handle())? != package.identity {
                return Err("WAKE_SAME_STATE_PUBLICATION_OBJECT_CHANGED".into());
            }
            let moved = open_relative_object(&self.versions, &expected.package_id, true, false, 7)?;
            if full_identity_of_handle(moved.as_raw_handle())? != package.identity {
                return Err("WAKE_SAME_STATE_PUBLICATION_OBJECT_CHANGED".into());
            }
            let (security_identity, security) =
                package_security(&self.versions, &expected.package_id, &self.security)?;
            if security_identity != package.identity || security != package.security {
                return Err("WAKE_BROKER_PUBLICATION_ACL_MISMATCH".into());
            }
            let mut manifest_file = File::from(open_relative(&moved, MANIFEST_NAME, false)?);
            let mut artifacts: [File; 4] = ARTIFACT_NAMES
                .iter()
                .map(|name| open_relative(&moved, name, false).map(File::from))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .map_err(|_| "WAKE_SAME_STATE_PUBLICATION_ATTENTION")?;
            let moved_byte_identities = std::iter::once(&manifest_file)
                .chain(artifacts.iter())
                .map(|file| full_identity_of_handle(file.as_raw_handle()))
                .collect::<Result<Vec<_>>>()?;
            if moved_byte_identities != byte_identities {
                return Err("WAKE_SAME_STATE_PUBLICATION_OBJECT_CHANGED".into());
            }
            require_publication_bytes(
                &moved,
                &mut manifest_file,
                &mut artifacts,
                &expected,
                &raw_manifest,
            )?;
            drop(moved);
            drop(read_directory);
            self.revalidate_parents()?;
            let pins = RuntimePackagePinsV3::open_with_publication(
                self.root
                    .try_clone()
                    .map_err(|_| "WAKE_SAME_STATE_PUBLICATION_ATTENTION")?,
                &["versions", &expected.package_id],
                Some(PublicationDirectoryPinV3 {
                    source,
                    identity: package.identity,
                    security: self.security.clone(),
                    expected_security: package.security,
                }),
            )?;
            if pins.manifest() != &expected || pins.runtime_manifest_bytes() != raw_manifest {
                return Err("WAKE_SAME_STATE_PUBLISHED_PACKAGE_MISMATCH".into());
            }
            published.push(pins);
        }
        self.revalidate_parents()?;
        Ok(published)
    }

    #[cfg(test)]
    pub(crate) fn fixture_use_proven_destination(&mut self) -> Result<()> {
        use crate::install_broker_offline_pins::full_identity_of_handle;
        self.revalidate_parents()?;
        let destination = crate::install_broker_stage_writer::open_publication_child(
            &self.root, "versions", false,
        )?;
        if full_identity_of_handle(destination.as_raw_handle())? != self.versions_identity {
            return Err("WAKE_SAME_STATE_PUBLICATION_PARENT_CHANGED".into());
        }
        self.versions = destination;
        self.revalidate_parents()
    }
    #[cfg(test)]
    pub(crate) fn fixture_hook(after_release: bool, hook: PublicationHook) {
        let slot = if after_release {
            &PUBLICATION_AFTER_RELEASE
        } else {
            &PUBLICATION_BEFORE_RENAME
        };
        slot.with(|slot| {
            assert!(slot.borrow().is_none());
            *slot.borrow_mut() = Some(hook);
        });
    }
    #[cfg(test)]
    pub(crate) fn fixture_clear_hooks() {
        PUBLICATION_AFTER_RELEASE.with(|slot| {
            slot.borrow_mut().take();
        });
        PUBLICATION_BEFORE_RENAME.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

fn package_security(
    versions: &OwnedHandle,
    name: &str,
    security: &StageSecurityMode,
) -> Result<PackageSecurityV3> {
    use crate::install_broker_offline_pins::full_identity_of_handle;
    let directory = open_relative_object_secured(versions, name, true, false, 7, None, true)?;
    let identity = full_identity_of_handle(directory.as_raw_handle())?;
    let mut observed = vec![security.readback(&directory, true)?];
    for leaf in std::iter::once(MANIFEST_NAME).chain(ARTIFACT_NAMES) {
        let file = open_relative_object_secured(&directory, leaf, false, false, 1, None, true)?;
        observed.push(security.readback(&file, false)?);
    }
    Ok((identity, observed))
}

fn require_publication_bytes(
    directory: &OwnedHandle,
    manifest: &mut File,
    artifacts: &mut [File; 4],
    expected: &PackageManifestV1,
    raw_manifest: &[u8],
) -> Result<()> {
    use crate::install_broker_offline_pins::full_identity_of_handle;
    // Only this publication readback shares DELETE with our retained exact
    // rename-capable directory handle. Ordinary source readers stay unchanged.
    inventory_with_sharing(
        directory,
        None,
        &[
            MANIFEST_NAME,
            ARTIFACT_NAMES[0],
            ARTIFACT_NAMES[1],
            ARTIFACT_NAMES[2],
            ARTIFACT_NAMES[3],
        ],
        7,
    )?;
    let (raw, observed) = read_manifest(manifest)?;
    let reopened = open_relative(directory, MANIFEST_NAME, false)?;
    if raw != raw_manifest
        || !same_object(&observed, &info(reopened.as_raw_handle(), false)?)
        || full_identity_of_handle(manifest.as_raw_handle())?
            != full_identity_of_handle(reopened.as_raw_handle())?
    {
        return Err("WAKE_SAME_STATE_PUBLISHED_PACKAGE_MISMATCH".into());
    }
    for (index, file) in artifacts.iter_mut().enumerate() {
        let artifact = &expected.artifacts[index];
        let observed = hash_artifact(file, artifact.byte_length, &artifact.sha256)?;
        let reopened = open_relative(directory, &artifact.name, false)?;
        if !same_object(&observed, &info(reopened.as_raw_handle(), false)?)
            || full_identity_of_handle(file.as_raw_handle())?
                != full_identity_of_handle(reopened.as_raw_handle())?
        {
            return Err("WAKE_SAME_STATE_PUBLICATION_OBJECT_CHANGED".into());
        }
    }
    Ok(())
}

/// One fresh same-state stage. Directory identities and no-delete/write-share
/// handles remain owned through every relative create and package readback.
/// This byte writer is internal; only the admitted runtime chooses its root.
pub(crate) struct RuntimePackageStageV3 {
    directories: [OwnedHandle; 3],
    identities: [FileInfo; 3],
    name: String,
    security: StageSecurityMode,
}

impl RuntimePackageStageV3 {
    pub(crate) fn create(
        root: &std::path::Path,
        name: &str,
        security: StageSecurityMode,
    ) -> Result<Self> {
        if !name.starts_with(".staging-v3-") || !valid_component(name) {
            return Err("WAKE_SAME_STATE_STAGE_ATTENTION".into());
        }
        let root = open_path_directory_with_sharing(root.as_os_str(), FILE_SHARE_READ)?;
        let versions = open_relative_object(&root, "versions", true, false, FILE_SHARE_READ)?;
        // FILE_CREATE returns the new directory's retained handle atomically;
        // there is no create_dir -> open race or absolute child pathname.
        let descriptor = security.creation_descriptor(true)?;
        let stage = open_relative_object_secured(
            &versions,
            name,
            true,
            true,
            FILE_SHARE_READ,
            descriptor.as_ref(),
            true,
        )?;
        security.readback(&stage, true)?;
        let directories = [root, versions, stage];
        let identities = directories
            .iter()
            .map(|handle| info(handle.as_raw_handle(), true))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .map_err(|_| "WAKE_SAME_STATE_STAGE_ATTENTION")?;
        let stage = Self {
            directories,
            identities,
            name: name.into(),
            security,
        };
        stage.revalidate()?;
        Ok(stage)
    }

    pub(crate) fn revalidate(&self) -> Result<()> {
        self.security.readback(&self.directories[2], true)?;
        for (handle, expected) in self.directories.iter().zip(&self.identities) {
            if !same_object(expected, &info(handle.as_raw_handle(), true)?) {
                return Err("WAKE_SAME_STATE_STAGE_ATTENTION".into());
            }
        }
        for (index, name) in ["versions", self.name.as_str()].into_iter().enumerate() {
            let reopened =
                open_relative_object(&self.directories[index], name, true, false, FILE_SHARE_READ)?;
            if !same_object(
                &self.identities[index + 1],
                &info(reopened.as_raw_handle(), true)?,
            ) {
                return Err("WAKE_SAME_STATE_STAGE_ATTENTION".into());
            }
        }
        Ok(())
    }

    pub(crate) fn copy_package(&self, source: &mut RuntimePackagePinsV3) -> Result<()> {
        use std::io::Write;
        source.revalidate()?;
        for index in 0..4 {
            self.revalidate()?;
            let description = source.manifest().artifacts[index].clone();
            let descriptor = self.security.creation_descriptor(false)?;
            let mut target = File::from(open_relative_object_secured(
                &self.directories[2],
                &description.name,
                false,
                true,
                0,
                descriptor.as_ref(),
                true,
            )?);
            self.security.readback(&target, false)?;
            let input = source.artifact_mut(index);
            input
                .seek(SeekFrom::Start(0))
                .map_err(|_| "WAKE_SAME_STATE_STAGE_ATTENTION")?;
            let copied = std::io::copy(&mut input.take(description.byte_length + 1), &mut target)
                .map_err(|_| "WAKE_SAME_STATE_STAGE_ATTENTION")?;
            if copied != description.byte_length {
                return Err("WAKE_SAME_STATE_STAGE_ATTENTION".into());
            }
            target
                .sync_all()
                .map_err(|_| "WAKE_SAME_STATE_STAGE_ATTENTION")?;
            hash_artifact(&mut target, description.byte_length, &description.sha256)?;
            self.security.readback(&target, false)?;
        }
        self.revalidate()?;
        let descriptor = self.security.creation_descriptor(false)?;
        let mut manifest = File::from(open_relative_object_secured(
            &self.directories[2],
            MANIFEST_NAME,
            false,
            true,
            0,
            descriptor.as_ref(),
            true,
        )?);
        self.security.readback(&manifest, false)?;
        manifest
            .write_all(source.runtime_manifest_bytes())
            .and_then(|_| manifest.sync_all())
            .map_err(|_| "WAKE_SAME_STATE_STAGE_ATTENTION")?;
        self.security.readback(&manifest, false)?;
        drop(manifest);
        source.revalidate()?;
        self.revalidate()
    }

    #[cfg(test)]
    pub(crate) fn fixture_relative_create_after_metadata_change(&self) -> Result<()> {
        // Exercise the actual leaf-create primitive without a fresh directory
        // check: an inert fixture can model mutation immediately after it.
        use std::io::Write;
        let descriptor = self.security.creation_descriptor(false)?;
        let mut file = File::from(open_relative_object_secured(
            &self.directories[2],
            ARTIFACT_NAMES[2],
            false,
            true,
            0,
            descriptor.as_ref(),
            true,
        )?);
        file.write_all(b"inert relative-create sentinel")
            .and_then(|_| file.sync_all())
            .map_err(|_| "WAKE_SAME_STATE_STAGE_ATTENTION".into())
    }
}

struct PublicationDirectoryPinV3 {
    source: OwnedHandle,
    identity: crate::install_broker_offline_v2::FullFileIdentityV2,
    security: StageSecurityMode,
    expected_security: Vec<(String, String)>,
}

pub(crate) struct RuntimePackagePinsV3 {
    ancestors: Vec<OwnedHandle>,
    chain: Vec<String>,
    directory_info: Vec<FileInfo>,
    manifest_file: File,
    manifest_info: FileInfo,
    runtime_manifest: Vec<u8>,
    manifest: PackageManifestV1,
    artifacts: [File; 4],
    artifact_info: [FileInfo; 4],
    publication: Option<PublicationDirectoryPinV3>,
}

impl RuntimePackagePinsV3 {
    pub(crate) fn fixed(role: RuntimeSourceRoleV3) -> Result<Self> {
        let role = match role {
            RuntimeSourceRoleV3::Candidate => "candidate",
            RuntimeSourceRoleV3::Previous => "previous",
        };
        Self::open(
            open_path_directory(OsStr::new(r"C:\"))?,
            &["ProgramData", "CatDesk", "WakeSubmissionV1", role],
        )
    }
    pub(crate) fn installed(root: &std::path::Path, id: &str) -> Result<Self> {
        if !valid_component(id) {
            return Err("WAKE_SAME_STATE_PACKAGE_INVALID".into());
        }
        Self::open(open_path_directory(root.as_os_str())?, &["versions", id])
    }
    #[cfg(test)]
    pub(crate) fn isolated(root: &std::path::Path, role: RuntimeSourceRoleV3) -> Result<Self> {
        let temp =
            std::fs::canonicalize(std::env::temp_dir()).map_err(|_| "FIXTURE_ROOT_INVALID")?;
        let root = std::fs::canonicalize(root).map_err(|_| "FIXTURE_ROOT_INVALID")?;
        if !root.starts_with(&temp) || root == temp {
            return Err("FIXTURE_ROOT_INVALID".into());
        }
        let role = match role {
            RuntimeSourceRoleV3::Candidate => "candidate",
            RuntimeSourceRoleV3::Previous => "previous",
        };
        Self::open(open_path_directory(root.as_os_str())?, &[role])
    }
    fn open(root: OwnedHandle, chain: &[&str]) -> Result<Self> {
        Self::open_with_publication(root, chain, None)
    }

    fn open_with_publication(
        root: OwnedHandle,
        chain: &[&str],
        publication: Option<PublicationDirectoryPinV3>,
    ) -> Result<Self> {
        let mut ancestors = vec![root];
        let mut directory_info = vec![info(ancestors[0].as_raw_handle(), true)?];
        for (index, component) in chain.iter().enumerate() {
            if !valid_component(component) {
                return Err("WAKE_SAME_STATE_PACKAGE_INVALID".into());
            }
            let parent = ancestors.last().ok_or("WAKE_SAME_STATE_PACKAGE_INVALID")?;
            inventory(parent, Some(component), &[component])?;
            // Only the final package reader shares DELETE, and only while our
            // separately retained exact source capability denies other DELETE
            // opens. Root/versions and ordinary package readers are unchanged.
            let child = if publication.is_some() && index + 1 == chain.len() {
                open_relative_object(parent, component, true, false, 7)?
            } else {
                open_relative(parent, component, true)?
            };
            let child_info = info(child.as_raw_handle(), true)?;
            directory_info.push(child_info);
            ancestors.push(child);
        }
        let directory = ancestors.last().ok_or("WAKE_SAME_STATE_PACKAGE_INVALID")?;
        inventory_with_sharing(
            directory,
            None,
            &[
                MANIFEST_NAME,
                ARTIFACT_NAMES[0],
                ARTIFACT_NAMES[1],
                ARTIFACT_NAMES[2],
                ARTIFACT_NAMES[3],
            ],
            if publication.is_some() {
                7
            } else {
                FILE_SHARE_READ | FILE_SHARE_WRITE
            },
        )?;
        let mut manifest_file = File::from(open_relative(directory, MANIFEST_NAME, false)?);
        let (runtime_manifest, manifest_info) = read_manifest(&mut manifest_file)?;
        let map: Value = serde_json::from_slice(&runtime_manifest)
            .map_err(|_| "WAKE_SAME_STATE_RUNTIME_MANIFEST_INVALID")?;
        if map["schemaVersion"] != 1
            || map["protocolVersion"] != 1
            || map["artifacts"].as_object().is_none_or(|v| v.len() != 4)
        {
            return Err("WAKE_SAME_STATE_RUNTIME_MANIFEST_INVALID".into());
        }
        let version = map["version"]
            .as_str()
            .ok_or("WAKE_SAME_STATE_RUNTIME_MANIFEST_INVALID")?
            .to_owned();
        let mut files = Vec::new();
        let mut descriptions = Vec::new();
        let mut observed = Vec::new();
        for (index, name) in ARTIFACT_NAMES.into_iter().enumerate() {
            let mut file = File::from(open_relative(directory, name, false)?);
            let before = info(file.as_raw_handle(), false)?;
            let maximum = if index < 2 {
                80 * 1024 * 1024
            } else {
                1024 * 1024
            };
            if size(&before) == 0 || size(&before) > maximum {
                return Err("WAKE_SAME_STATE_PACKAGE_SIZE_INVALID".into());
            }
            let digest = map["artifacts"][name]
                .as_str()
                .ok_or("WAKE_SAME_STATE_RUNTIME_MANIFEST_INVALID")?
                .to_owned();
            let after = hash_artifact(&mut file, size(&before), &digest)?;
            descriptions.push(ArtifactV1 {
                name: name.into(),
                byte_length: size(&after),
                sha256: digest,
            });
            observed.push(after);
            files.push(file);
        }
        let descriptions: [ArtifactV1; 4] = descriptions
            .try_into()
            .map_err(|_| "WAKE_SAME_STATE_PACKAGE_INVALID")?;
        let package_id = format!(
            "{}-{}-{}",
            version,
            &descriptions[0].sha256[..12],
            &descriptions[1].sha256[..12]
        );
        let manifest = PackageManifestV1 {
            schema_version: 1,
            package_id,
            version,
            artifacts: descriptions,
        };
        validate_manifest(&manifest)?;
        let mut result = Self {
            ancestors,
            chain: chain.iter().map(|v| (*v).into()).collect(),
            directory_info,
            manifest_file,
            manifest_info,
            runtime_manifest,
            manifest,
            artifacts: files
                .try_into()
                .map_err(|_| "WAKE_SAME_STATE_PACKAGE_INVALID")?,
            artifact_info: observed
                .try_into()
                .map_err(|_| "WAKE_SAME_STATE_PACKAGE_INVALID")?,
            publication,
        };
        result.revalidate()?;
        Ok(result)
    }
    pub(crate) fn manifest(&self) -> &PackageManifestV1 {
        &self.manifest
    }
    pub(crate) fn manifest_sha256(&self) -> Result<String> {
        Ok(format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&self.manifest)
                    .map_err(|_| "WAKE_SAME_STATE_PACKAGE_INVALID")?
            )
        ))
    }
    pub(crate) fn runtime_manifest_sha256(&self) -> String {
        format!("{:x}", Sha256::digest(&self.runtime_manifest))
    }
    pub(crate) fn runtime_manifest_bytes(&self) -> &[u8] {
        &self.runtime_manifest
    }
    pub(crate) fn artifact_mut(&mut self, index: usize) -> &mut File {
        &mut self.artifacts[index]
    }
    pub(crate) fn pointer(&self) -> Value {
        json!({"schemaVersion":1,"directory":self.manifest.package_id,
        "hostSha256":self.manifest.artifacts[0].sha256,"binagotchySha256":self.manifest.artifacts[1].sha256,
        "adapterSha256":self.manifest.artifacts[2].sha256,"wakeBridgeSha256":self.manifest.artifacts[3].sha256})
    }
    pub(crate) fn revalidate(&mut self) -> Result<()> {
        if let Some(publication) = &self.publication {
            use crate::install_broker_offline_pins::full_identity_of_handle;
            let versions = self
                .ancestors
                .get(
                    self.ancestors
                        .len()
                        .checked_sub(2)
                        .ok_or("WAKE_SAME_STATE_PACKAGE_INVALID")?,
                )
                .ok_or("WAKE_SAME_STATE_PACKAGE_INVALID")?;
            let name = self.chain.last().ok_or("WAKE_SAME_STATE_PACKAGE_INVALID")?;
            let (actual_identity, actual_security) =
                package_security(versions, name, &publication.security)?;
            if full_identity_of_handle(publication.source.as_raw_handle())? != publication.identity
                || actual_identity != publication.identity
                || actual_security != publication.expected_security
            {
                return Err("WAKE_SAME_STATE_PUBLICATION_OBJECT_CHANGED".into());
            }
        }
        for (index, handle) in self.ancestors.iter().enumerate() {
            let fresh = info(handle.as_raw_handle(), true)?;
            if !same_object(&self.directory_info[index], &fresh) {
                return Err("WAKE_SAME_STATE_PACKAGE_CHANGED".into());
            }
            if index > 0 {
                let reopened = if self.publication.is_some() && index + 1 == self.ancestors.len() {
                    open_relative_object(
                        &self.ancestors[index - 1],
                        &self.chain[index - 1],
                        true,
                        false,
                        7,
                    )?
                } else {
                    open_relative(&self.ancestors[index - 1], &self.chain[index - 1], true)?
                };
                if !same_object(&fresh, &info(reopened.as_raw_handle(), true)?) {
                    return Err("WAKE_SAME_STATE_PACKAGE_CHANGED".into());
                }
            }
        }
        let directory = self
            .ancestors
            .last()
            .ok_or("WAKE_SAME_STATE_PACKAGE_INVALID")?;
        inventory_with_sharing(
            directory,
            None,
            &[
                MANIFEST_NAME,
                ARTIFACT_NAMES[0],
                ARTIFACT_NAMES[1],
                ARTIFACT_NAMES[2],
                ARTIFACT_NAMES[3],
            ],
            if self.publication.is_some() {
                7
            } else {
                FILE_SHARE_READ | FILE_SHARE_WRITE
            },
        )?;
        let (bytes, observed) = read_manifest(&mut self.manifest_file)?;
        if bytes != self.runtime_manifest || !stable(&observed, &self.manifest_info) {
            return Err("WAKE_SAME_STATE_PACKAGE_CHANGED".into());
        }
        let reopened = open_relative(directory, MANIFEST_NAME, false)?;
        if !same_object(&observed, &info(reopened.as_raw_handle(), false)?) {
            return Err("WAKE_SAME_STATE_PACKAGE_CHANGED".into());
        }
        for (index, name) in ARTIFACT_NAMES.iter().enumerate() {
            let expected = &self.manifest.artifacts[index];
            let observed = hash_artifact(
                &mut self.artifacts[index],
                expected.byte_length,
                &expected.sha256,
            )?;
            let reopened = open_relative(directory, name, false)?;
            if !stable(&observed, &self.artifact_info[index])
                || !same_object(&observed, &info(reopened.as_raw_handle(), false)?)
            {
                return Err("WAKE_SAME_STATE_PACKAGE_CHANGED".into());
            }
        }
        Ok(())
    }
}
