//! Runtime-MAP intake over the same handle-relative broker source primitives.
//! These pins establish bytes, never release qualification or installation.
use super::*;
use crate::install_broker_approval::ArtifactV1;
use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub(crate) enum RuntimeSourceRoleV3 {
    Candidate,
    Previous,
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
        let mut ancestors = vec![root];
        let mut directory_info = vec![info(ancestors[0].as_raw_handle(), true)?];
        for component in chain {
            if !valid_component(component) {
                return Err("WAKE_SAME_STATE_PACKAGE_INVALID".into());
            }
            let parent = ancestors.last().ok_or("WAKE_SAME_STATE_PACKAGE_INVALID")?;
            inventory(parent, Some(component), &[component])?;
            let child = open_relative(parent, component, true)?;
            let child_info = info(child.as_raw_handle(), true)?;
            directory_info.push(child_info);
            ancestors.push(child);
        }
        let directory = ancestors.last().ok_or("WAKE_SAME_STATE_PACKAGE_INVALID")?;
        inventory(
            directory,
            None,
            &[
                MANIFEST_NAME,
                ARTIFACT_NAMES[0],
                ARTIFACT_NAMES[1],
                ARTIFACT_NAMES[2],
                ARTIFACT_NAMES[3],
            ],
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
        for (index, handle) in self.ancestors.iter().enumerate() {
            let fresh = info(handle.as_raw_handle(), true)?;
            if !same_object(&self.directory_info[index], &fresh) {
                return Err("WAKE_SAME_STATE_PACKAGE_CHANGED".into());
            }
            if index > 0 {
                let reopened =
                    open_relative(&self.ancestors[index - 1], &self.chain[index - 1], true)?;
                if !same_object(&fresh, &info(reopened.as_raw_handle(), true)?) {
                    return Err("WAKE_SAME_STATE_PACKAGE_CHANGED".into());
                }
            }
        }
        let directory = self
            .ancestors
            .last()
            .ok_or("WAKE_SAME_STATE_PACKAGE_INVALID")?;
        inventory(
            directory,
            None,
            &[
                MANIFEST_NAME,
                ARTIFACT_NAMES[0],
                ARTIFACT_NAMES[1],
                ARTIFACT_NAMES[2],
                ARTIFACT_NAMES[3],
            ],
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
