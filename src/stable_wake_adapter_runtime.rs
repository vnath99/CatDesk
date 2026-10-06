//! Durable, reviewed browser-adapter runtime authority.
//!
//! This is deliberately a reader/preflight boundary. Host provisioning is a
//! separately reviewed operation: this module never copies an interpreter,
//! writes a descriptor, or selects a fallback Python/venv. At runtime the
//! only accepted authority is the fixed stable root below the protected wake
//! bridge directory.

use crate::windows_protected_fs::{
    PinnedDirectory, RetainedReviewedArtifact, ReviewedArtifactIdentity,
    bind_reviewed_regular_artifact, read_relative_regular, retain_reviewed_regular_artifact,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};

const RUNTIME_ROOT_COMPONENT: &str = "stable-runtime-v1";
const DESCRIPTOR_COMPONENT: &str = "adapter-runtime.json";
const OWNER_COMPONENT: &str = "catdesk-stable-wake-owner.exe";
const INTERPRETER_COMPONENT: &str = "python.exe";
const ADAPTER_COMPONENT: &str = "stable_wake_browser_adapter.py";
const BROWSER_PRIMITIVES_COMPONENT: &str = "wake_bridge.py";
const REVIEWED_CONTROL_COMPONENT: &str = "reviewed-build-control";
const REVIEWED_ARTIFACTS_COMPONENT: &str = "wake-owner-artifacts";
const REVIEWED_EVIDENCE_COMPONENT: &str = "reviewed-artifacts.json";
const DESCRIPTOR_SCHEMA_VERSION: u32 = 1;
const RUNTIME_VERSION: u32 = 1;
const REVIEWED_EVIDENCE_SCHEMA_VERSION: u32 = 2;
const MAX_DESCRIPTOR_BYTES: u64 = 2 * 1024;
const MAX_REVIEWED_EVIDENCE_BYTES: u64 = 2 * 1024;
// A reviewed self-contained interpreter may be larger than the ordinary
// adapter scripts; keep a fixed bounded allowance for that artifact while
// still refusing unbounded runtime material.
const MAX_RUNTIME_ARTIFACT_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewedRuntimeEvidenceV2 {
    schema_version: u32,
    runtime_descriptor_sha256: String,
    provenance_sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AdapterRuntimeDescriptorV1 {
    schema_version: u32,
    runtime_version: u32,
    owner_sha256: String,
    interpreter_sha256: String,
    adapter_sha256: String,
    browser_primitives_sha256: String,
}

/// Bounded, non-secret runtime identity. The artifact digests are retained
/// only for immediate revalidation before process launch; no target/profile,
/// credential, path input, or runtime fallback is represented here.
pub(crate) struct DurableAdapterRuntime {
    // Retain every ancestor named by the fixed runtime path. These directory
    // handles deny delete sharing, preventing a parent/root rename during a
    // pathname-based CreateProcess or Python import.
    workspace: PinnedDirectory,
    catdesk: PinnedDirectory,
    wake_bridge: PinnedDirectory,
    root: PinnedDirectory,
    descriptor_sha256: String,
    descriptor: RuntimeDescriptorIdentity,
}

/// Launch-scoped authority for the exact pathnames consumed by CreateProcess
/// and Python import. The retained file handles allow read sharing only;
/// Windows consequently denies write, delete, and rename replacement until
/// the lease is released after child completion.
pub(crate) struct RuntimeLaunchLease<'runtime> {
    runtime: &'runtime DurableAdapterRuntime,
    _interpreter: RetainedReviewedArtifact,
    _adapter: RetainedReviewedArtifact,
    _browser_primitives: RetainedReviewedArtifact,
}

impl RuntimeLaunchLease<'_> {
    pub(crate) fn interpreter_path(&self) -> PathBuf {
        self.runtime.root.path.join(INTERPRETER_COMPONENT)
    }

    pub(crate) fn adapter_path(&self) -> PathBuf {
        self.runtime.root.path.join(ADAPTER_COMPONENT)
    }

    /// Explicitly consume this lease only after child exit/output collection,
    /// avoiding an accidental early drop before Python imports its primitive.
    pub(crate) fn release_after_child_exit(self) {}
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RuntimeDescriptorIdentity {
    owner_sha256: String,
    interpreter_sha256: String,
    adapter_sha256: String,
    browser_primitives_sha256: String,
}

impl DurableAdapterRuntime {
    /// Opens only the fixed durable root. Missing, stale, redirected, or
    /// unreviewed runtime material is an error; there is no PATH, `py`, venv,
    /// source-script, release, daemon, MCP, manifest, or LKG fallback.
    pub(crate) fn open(workspace: &Path) -> Result<Self, String> {
        let workspace = PinnedDirectory::acquire(workspace, "stable wake runtime workspace")?;
        let catdesk =
            PinnedDirectory::open_relative(&workspace, ".catdesk", "stable wake runtime")?;
        let wake_bridge =
            PinnedDirectory::open_relative(&catdesk, "wake-bridge", "stable wake runtime")?;
        let root = PinnedDirectory::open_relative(
            &wake_bridge,
            RUNTIME_ROOT_COMPONENT,
            "stable wake durable runtime",
        )?;
        let descriptor_sha256 = read_reviewed_descriptor_sha256(&catdesk)?;
        let descriptor_artifact = bind_reviewed_regular_artifact(
            &root,
            DESCRIPTOR_COMPONENT,
            &descriptor_sha256,
            MAX_DESCRIPTOR_BYTES,
            "stable wake runtime descriptor",
        )?;
        let descriptor = parse_descriptor(&read_relative_regular(
            &root,
            DESCRIPTOR_COMPONENT,
            MAX_DESCRIPTOR_BYTES,
            "stable wake runtime descriptor",
        )?)?;
        if descriptor_artifact.sha256 != descriptor_sha256 {
            return Err("stable wake runtime descriptor drifted".into());
        }
        let runtime = Self {
            workspace,
            catdesk,
            wake_bridge,
            root,
            descriptor_sha256,
            descriptor,
        };
        runtime.revalidate()?;
        Ok(runtime)
    }

    /// Validate every fixed artifact and retain launch-bound no-follow handles
    /// for the interpreter, adapter, and Python-imported browser primitive.
    /// Callers must retain the returned lease through child completion.
    pub(crate) fn prepare_launch(&self) -> Result<RuntimeLaunchLease<'_>, String> {
        self.workspace
            .assert_stable("stable wake runtime workspace")?;
        self.catdesk.assert_stable("stable wake runtime")?;
        self.wake_bridge.assert_stable("stable wake runtime")?;
        self.root.assert_stable("stable wake durable runtime")?;
        if read_reviewed_descriptor_sha256(&self.catdesk)? != self.descriptor_sha256 {
            return Err("stable wake runtime descriptor drifted".into());
        }
        let descriptor_artifact = bind_reviewed_regular_artifact(
            &self.root,
            DESCRIPTOR_COMPONENT,
            &self.descriptor_sha256,
            MAX_DESCRIPTOR_BYTES,
            "stable wake runtime descriptor",
        )?;
        let current = parse_descriptor(&read_relative_regular(
            &self.root,
            DESCRIPTOR_COMPONENT,
            MAX_DESCRIPTOR_BYTES,
            "stable wake runtime descriptor",
        )?)?;
        if descriptor_artifact.sha256 != self.descriptor_sha256 || current != self.descriptor {
            return Err("stable wake runtime descriptor drifted".into());
        }
        let _owner = self.bind(
            OWNER_COMPONENT,
            &current.owner_sha256,
            "stable wake runtime owner",
        )?;
        let interpreter = self.retain(
            INTERPRETER_COMPONENT,
            &current.interpreter_sha256,
            "stable wake runtime interpreter",
        )?;
        let adapter = self.retain(
            ADAPTER_COMPONENT,
            &current.adapter_sha256,
            "stable wake runtime adapter",
        )?;
        let browser_primitives = self.retain(
            BROWSER_PRIMITIVES_COMPONENT,
            &current.browser_primitives_sha256,
            "stable wake runtime browser primitives",
        )?;
        self.workspace
            .assert_stable("stable wake runtime workspace")?;
        self.catdesk.assert_stable("stable wake runtime")?;
        self.wake_bridge.assert_stable("stable wake runtime")?;
        self.root.assert_stable("stable wake durable runtime")?;
        Ok(RuntimeLaunchLease {
            runtime: self,
            _interpreter: interpreter,
            _adapter: adapter,
            _browser_primitives: browser_primitives,
        })
    }

    /// Preflight compatibility check. A pathname-based process launch must
    /// call `prepare_launch` instead so its reviewed handles remain retained.
    pub(crate) fn revalidate(&self) -> Result<(), String> {
        self.prepare_launch().map(|_| ())
    }

    fn bind(
        &self,
        component: &str,
        sha256: &str,
        label: &str,
    ) -> Result<ReviewedArtifactIdentity, String> {
        bind_reviewed_regular_artifact(
            &self.root,
            component,
            sha256,
            MAX_RUNTIME_ARTIFACT_BYTES,
            label,
        )
    }

    fn retain(
        &self,
        component: &str,
        sha256: &str,
        label: &str,
    ) -> Result<RetainedReviewedArtifact, String> {
        retain_reviewed_regular_artifact(
            &self.root,
            component,
            sha256,
            MAX_RUNTIME_ARTIFACT_BYTES,
            label,
        )
    }

    /// Planning-only fixed layout for a separately authorized host provision.
    /// It intentionally does not create, copy, repair, or select any runtime.
    #[allow(dead_code)] // The fixed host provisioning procedure is separately gated.
    pub(crate) fn host_provisioning_components() -> [&'static str; 5] {
        [
            DESCRIPTOR_COMPONENT,
            OWNER_COMPONENT,
            INTERPRETER_COMPONENT,
            ADAPTER_COMPONENT,
            BROWSER_PRIMITIVES_COMPONENT,
        ]
    }
}

fn read_reviewed_descriptor_sha256(catdesk: &PinnedDirectory) -> Result<String, String> {
    let reviewed = PinnedDirectory::open_relative(
        catdesk,
        REVIEWED_CONTROL_COMPONENT,
        "stable wake reviewed runtime evidence",
    )?;
    let artifacts = PinnedDirectory::open_relative(
        &reviewed,
        REVIEWED_ARTIFACTS_COMPONENT,
        "stable wake reviewed runtime evidence",
    )?;
    let bytes = read_relative_regular(
        &artifacts,
        REVIEWED_EVIDENCE_COMPONENT,
        MAX_REVIEWED_EVIDENCE_BYTES,
        "stable wake reviewed runtime evidence",
    )?;
    let evidence: ReviewedRuntimeEvidenceV2 = serde_json::from_slice(&bytes)
        .map_err(|_| "stable wake reviewed runtime evidence invalid".to_string())?;
    if evidence.schema_version != REVIEWED_EVIDENCE_SCHEMA_VERSION
        || !sha256(&evidence.runtime_descriptor_sha256)
        || !sha256(&evidence.provenance_sha256)
    {
        return Err("stable wake reviewed runtime evidence invalid".into());
    }
    Ok(evidence.runtime_descriptor_sha256)
}

fn parse_descriptor(bytes: &[u8]) -> Result<RuntimeDescriptorIdentity, String> {
    let descriptor: AdapterRuntimeDescriptorV1 = serde_json::from_slice(bytes)
        .map_err(|_| "stable wake runtime descriptor invalid".to_string())?;
    if descriptor.schema_version != DESCRIPTOR_SCHEMA_VERSION
        || descriptor.runtime_version != RUNTIME_VERSION
        || !sha256(&descriptor.owner_sha256)
        || !sha256(&descriptor.interpreter_sha256)
        || !sha256(&descriptor.adapter_sha256)
        || !sha256(&descriptor.browser_primitives_sha256)
    {
        return Err("stable wake runtime descriptor invalid".into());
    }
    Ok(RuntimeDescriptorIdentity {
        owner_sha256: descriptor.owner_sha256,
        interpreter_sha256: descriptor.interpreter_sha256,
        adapter_sha256: descriptor.adapter_sha256,
        browser_primitives_sha256: descriptor.browser_primitives_sha256,
    })
}

fn sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{fs, process::Command};
    use uuid::Uuid;

    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn fixture(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("catdesk-runtime-{label}-{}", Uuid::new_v4()));
        let runtime = root.join(".catdesk/wake-bridge/stable-runtime-v1");
        fs::create_dir_all(&runtime).unwrap();
        fs::create_dir_all(root.join(".catdesk/reviewed-build-control/wake-owner-artifacts"))
            .unwrap();
        let owner = b"owner fixture";
        let interpreter = b"durable interpreter fixture";
        let adapter = b"durable adapter fixture";
        let bridge = b"durable browser primitives fixture";
        fs::write(runtime.join(OWNER_COMPONENT), owner).unwrap();
        fs::write(runtime.join(INTERPRETER_COMPONENT), interpreter).unwrap();
        fs::write(runtime.join(ADAPTER_COMPONENT), adapter).unwrap();
        fs::write(runtime.join(BROWSER_PRIMITIVES_COMPONENT), bridge).unwrap();
        refresh_reviewed_metadata(&root);
        root
    }

    fn refresh_reviewed_metadata(root: &Path) {
        let runtime = root.join(".catdesk/wake-bridge/stable-runtime-v1");
        let owner = fs::read(runtime.join(OWNER_COMPONENT)).unwrap();
        let interpreter = fs::read(runtime.join(INTERPRETER_COMPONENT)).unwrap();
        let adapter = fs::read(runtime.join(ADAPTER_COMPONENT)).unwrap();
        let bridge = fs::read(runtime.join(BROWSER_PRIMITIVES_COMPONENT)).unwrap();
        let descriptor = format!(
            r#"{{"schemaVersion":1,"runtimeVersion":1,"ownerSha256":"{}","interpreterSha256":"{}","adapterSha256":"{}","browserPrimitivesSha256":"{}"}}"#,
            digest(&owner),
            digest(&interpreter),
            digest(&adapter),
            digest(&bridge),
        );
        fs::write(runtime.join(DESCRIPTOR_COMPONENT), &descriptor).unwrap();
        let evidence = format!(
            r#"{{"schemaVersion":2,"runtimeDescriptorSha256":"{}","provenanceSha256":"{}"}}"#,
            digest(descriptor.as_bytes()),
            "a".repeat(64),
        );
        fs::write(
            root.join(
                ".catdesk/reviewed-build-control/wake-owner-artifacts/reviewed-artifacts.json",
            ),
            evidence,
        )
        .unwrap();
    }

    #[test]
    fn durable_runtime_binds_only_fixed_reviewed_components() {
        let root = fixture("valid");
        let runtime = DurableAdapterRuntime::open(&root).expect("durable fixture opens");
        let lease = runtime.prepare_launch().expect("launch lease opens");
        assert!(lease.interpreter_path().ends_with(INTERPRETER_COMPONENT));
        assert!(lease.adapter_path().ends_with(ADAPTER_COMPONENT));
        lease.release_after_child_exit();
        assert_eq!(
            DurableAdapterRuntime::host_provisioning_components(),
            [
                DESCRIPTOR_COMPONENT,
                OWNER_COMPONENT,
                INTERPRETER_COMPONENT,
                ADAPTER_COMPONENT,
                BROWSER_PRIMITIVES_COMPONENT,
            ]
        );
        drop(runtime);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_missing_corrupt_stale_or_replaced_fails_without_fallback() {
        let missing =
            std::env::temp_dir().join(format!("catdesk-runtime-missing-{}", Uuid::new_v4()));
        fs::create_dir_all(&missing).unwrap();
        assert!(DurableAdapterRuntime::open(&missing).is_err());
        fs::remove_dir_all(&missing).unwrap();

        let root = fixture("adversarial");
        let runtime = root.join(".catdesk/wake-bridge/stable-runtime-v1");
        fs::write(runtime.join(DESCRIPTOR_COMPONENT), b"not-json").unwrap();
        assert!(DurableAdapterRuntime::open(&root).is_err());

        let root = fixture("stale");
        let descriptor = root.join(".catdesk/wake-bridge/stable-runtime-v1/adapter-runtime.json");
        let bytes = fs::read_to_string(&descriptor)
            .unwrap()
            .replace("\"runtimeVersion\":1", "\"runtimeVersion\":2");
        fs::write(&descriptor, bytes).unwrap();
        assert!(DurableAdapterRuntime::open(&root).is_err());
        fs::remove_dir_all(root).unwrap();

        let root = fixture("replacement");
        let adapter = root
            .join(".catdesk/wake-bridge/stable-runtime-v1")
            .join(ADAPTER_COMPONENT);
        let runtime = DurableAdapterRuntime::open(&root).unwrap();
        fs::write(adapter, b"replacement").unwrap();
        assert!(runtime.revalidate().is_err());
        drop(runtime);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removed_interpreter_and_release_daemon_mcp_changes_do_not_select_fallbacks() {
        let root = fixture("independent");
        let interpreter = root
            .join(".catdesk/wake-bridge/stable-runtime-v1")
            .join(INTERPRETER_COMPONENT);
        let runtime = DurableAdapterRuntime::open(&root).unwrap();
        fs::create_dir_all(root.join("target/release")).unwrap();
        fs::write(root.join("target/release/catdesk.exe"), b"replaced").unwrap();
        fs::create_dir_all(root.join(".catdesk/reviewed-release")).unwrap();
        fs::write(root.join(".catdesk/reviewed-release/manifest.json"), b"bad").unwrap();
        assert!(
            runtime.revalidate().is_ok(),
            "runtime ignores release/daemon/MCP/LKG artifacts"
        );
        fs::remove_file(interpreter).unwrap();
        assert!(
            runtime.revalidate().is_err(),
            "missing interpreter must not use a venv, PATH, or py fallback"
        );
        drop(runtime);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn launch_lease_blocks_artifact_replacement_until_child_boundary() {
        const CHILD_ROOT: &str = "CATDESK_RUNTIME_LEASE_CHILD_ROOT";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let runtime_root = PathBuf::from(root);
            for artifact in [
                runtime_root.join(INTERPRETER_COMPONENT),
                runtime_root.join(ADAPTER_COMPONENT),
                runtime_root.join(BROWSER_PRIMITIVES_COMPONENT),
            ] {
                let moved = artifact.with_extension("child-replacement");
                assert!(fs::write(&artifact, b"child-replacement").is_err());
                assert!(fs::rename(&artifact, moved).is_err());
                assert!(fs::remove_file(artifact).is_err());
            }
            let moved_runtime = runtime_root.with_file_name("stable-runtime-v1-child-moved");
            assert!(fs::rename(&runtime_root, moved_runtime).is_err());
            return;
        }

        let root = fixture("launch-lease");
        let runtime_root = root.join(".catdesk/wake-bridge/stable-runtime-v1");
        let artifacts = [
            runtime_root.join(INTERPRETER_COMPONENT),
            runtime_root.join(ADAPTER_COMPONENT),
            runtime_root.join(BROWSER_PRIMITIVES_COMPONENT),
        ];
        let runtime = DurableAdapterRuntime::open(&root).unwrap();
        let lease = runtime.prepare_launch().unwrap();

        let status = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("stable_wake_adapter_runtime::tests::launch_lease_blocks_artifact_replacement_until_child_boundary")
            .env(CHILD_ROOT, &runtime_root)
            .status()
            .unwrap();
        assert!(status.success(), "child replacement attempt must be denied");

        lease.release_after_child_exit();
        for artifact in &artifacts {
            fs::write(artifact, b"changed-after-lease").unwrap();
        }
        let moved_runtime = runtime_root.with_file_name("stable-runtime-v1-moved");
        drop(runtime);
        fs::rename(&runtime_root, &moved_runtime).unwrap();
        fs::rename(&moved_runtime, &runtime_root).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn launch_lease_process_executes_and_reads_only_reviewed_fixture_objects() {
        const CHILD_ROOT: &str = "CATDESK_RUNTIME_LEASE_EXECUTION_CHILD_ROOT";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let runtime_root = PathBuf::from(root);
            let descriptor =
                parse_descriptor(&fs::read(runtime_root.join(DESCRIPTOR_COMPONENT)).unwrap())
                    .unwrap();
            let child_exe = std::env::current_exe().unwrap();
            assert_eq!(
                fs::canonicalize(&child_exe).unwrap(),
                fs::canonicalize(runtime_root.join(INTERPRETER_COMPONENT)).unwrap(),
                "the child must be the lease-selected reviewed interpreter"
            );
            assert_eq!(
                digest(&fs::read(&child_exe).unwrap()),
                descriptor.interpreter_sha256
            );
            assert_eq!(
                digest(&fs::read(runtime_root.join(ADAPTER_COMPONENT)).unwrap()),
                descriptor.adapter_sha256
            );
            assert_eq!(
                digest(&fs::read(runtime_root.join(BROWSER_PRIMITIVES_COMPONENT)).unwrap()),
                descriptor.browser_primitives_sha256
            );
            return;
        }

        let root = fixture("launch-execution");
        let runtime_root = root.join(".catdesk/wake-bridge/stable-runtime-v1");
        fs::copy(
            std::env::current_exe().unwrap(),
            runtime_root.join(INTERPRETER_COMPONENT),
        )
        .unwrap();
        refresh_reviewed_metadata(&root);

        let runtime = DurableAdapterRuntime::open(&root).unwrap();
        let lease = runtime.prepare_launch().unwrap();
        let status = Command::new(lease.interpreter_path())
            .arg("--exact")
            .arg("stable_wake_adapter_runtime::tests::launch_lease_process_executes_and_reads_only_reviewed_fixture_objects")
            .env(CHILD_ROOT, &runtime_root)
            .status()
            .unwrap();
        assert!(
            status.success(),
            "lease-selected reviewed process must succeed"
        );
        lease.release_after_child_exit();
        drop(runtime);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn reparse_redirected_runtime_root_is_refused_when_symlink_fixture_is_available() {
        let root = fixture("reparse");
        let runtime = root.join(".catdesk/wake-bridge/stable-runtime-v1");
        let replacement = root.join(".catdesk/wake-bridge/stable-runtime-replacement");
        fs::rename(&runtime, &replacement).unwrap();
        if std::os::windows::fs::symlink_dir(&replacement, &runtime).is_err() {
            fs::rename(&replacement, &runtime).unwrap();
            fs::remove_dir_all(root).unwrap();
            return;
        }
        assert!(DurableAdapterRuntime::open(&root).is_err());
        fs::remove_dir(&runtime).unwrap();
        fs::rename(&replacement, &runtime).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_has_no_path_or_repository_venv_fallback_authority() {
        let source = include_str!("stable_wake_adapter_runtime.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        for forbidden in [
            "Command::new",
            "std::env::var",
            "venv/Scripts",
            "target/release",
        ] {
            assert!(
                !production.contains(forbidden),
                "runtime must not contain {forbidden} fallback authority"
            );
        }
        assert!(production.contains("RuntimeLaunchLease"));
        assert!(production.contains("prepare_launch"));
        assert!(production.contains("retain_reviewed_regular_artifact"));
    }
}
