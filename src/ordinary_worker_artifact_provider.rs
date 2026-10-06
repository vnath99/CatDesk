//! Fixed, read-only provenance carrier for the two ordinary-worker artifacts.
//!
//! This is deliberately distinct from reviewed-main-image bootstrap/rotation
//! and from workspace-scoped reviewed-build APIs.  The fixed carrier can only
//! reopen a product-owned pair that an independently accepted producer placed
//! beneath the compiled root; it never creates, signs, or promotes artifacts.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::user_worker_release::UserWorkerReleaseManifestV1;
use crate::windows_protected_fs::{
    PinnedDirectory, ProtectedDirectoryGuard, read_optional_relative_regular,
};

const SCHEMA: u32 = 1;
const PURPOSE: &str = "catdesk-ordinary-worker-artifact-provider-v1";
const ROLE: &str = "catdesk-ordinary-worker-v1";
const PRODUCT: &str = "CatDesk";
const MAX_RECORD: u64 = 64 * 1024;
const MAX_IMAGE: u64 = 512 * 1024 * 1024;
const PROVIDER_RECORD: &str = "ordinary-worker-artifacts.v1.json";
const PREDECESSOR_IMAGE: &str = "predecessor.exe";
const CURRENT_IMAGE: &str = "current.exe";
const PREDECESSOR_ARTIFACT: &str = "ordinary-worker-predecessor-v1";
const CURRENT_ARTIFACT: &str = "ordinary-worker-current-v1";

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|_| "ordinary worker artifact serialization failed".into())
}

/// The reviewed provenance fields copied from the independently validated
/// completion and reviewed-build attestation.  There is intentionally no
/// acknowledgement boolean or reviewer prose field to reinterpret as proof.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrdinaryWorkerArtifactBindingV1 {
    generation: u64,
    artifact_id: String,
    image_sha256: String,
    image_length: u64,
    source_snapshot_id: String,
    review_session_id: String,
    review_record_id: String,
    review_authority_sha256: String,
    review_contract_hash: String,
    review_completion_sha256: String,
    review_remeasurement_digest: String,
    attestation_id: String,
    attestation_sha256: String,
    attestation_candidate_sha256: String,
    attestation_candidate_length: u64,
}

impl OrdinaryWorkerArtifactBindingV1 {
    fn validate(&self, expected_artifact: &str) -> Result<(), String> {
        if self.generation == 0
            || self.artifact_id != expected_artifact
            || self.image_length == 0
            || self.image_length > MAX_IMAGE
            || !valid_hex(&self.image_sha256)
            || !valid_hex(&self.review_authority_sha256)
            || !valid_hex(&self.review_contract_hash)
            || !valid_hex(&self.review_completion_sha256)
            || !valid_hex(&self.review_remeasurement_digest)
            || !valid_hex(&self.attestation_sha256)
            || !valid_hex(&self.attestation_candidate_sha256)
            || self.attestation_candidate_length != self.image_length
            || self.attestation_candidate_sha256 != self.image_sha256
            || !component(&self.source_snapshot_id)
            || !component(&self.review_session_id)
            || !component(&self.review_record_id)
            || !component(&self.attestation_id)
        {
            return Err("ordinary worker artifact binding is invalid".into());
        }
        Ok(())
    }

    fn release_manifest(&self) -> UserWorkerReleaseManifestV1 {
        UserWorkerReleaseManifestV1 {
            schema: 1,
            generation: self.generation,
            role: ROLE.into(),
            image_sha256: self.image_sha256.clone(),
            image_length: self.image_length,
            source_snapshot_id: self.source_snapshot_id.clone(),
            review_session_id: self.review_session_id.clone(),
            review_record_id: self.review_record_id.clone(),
            review_authority_sha256: self.review_authority_sha256.clone(),
            attestation_id: self.attestation_id.clone(),
            attestation_sha256: self.attestation_sha256.clone(),
        }
    }
}

/// Canonical product-owned record.  The fixed root is a carrier for exact
/// independently validated identities, not a generic review or signing root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrdinaryWorkerArtifactPairRecordV1 {
    schema: u32,
    product: String,
    purpose: String,
    role: String,
    predecessor: OrdinaryWorkerArtifactBindingV1,
    current: OrdinaryWorkerArtifactBindingV1,
}

impl OrdinaryWorkerArtifactPairRecordV1 {
    fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA
            || self.product != PRODUCT
            || self.purpose != PURPOSE
            || self.role != ROLE
        {
            return Err("ordinary worker artifact provider domain is invalid".into());
        }
        self.predecessor.validate(PREDECESSOR_ARTIFACT)?;
        self.current.validate(CURRENT_ARTIFACT)?;
        if self.predecessor.generation >= self.current.generation
            || self.predecessor.image_sha256 == self.current.image_sha256
        {
            return Err("ordinary worker artifact pair relation is invalid".into());
        }
        Ok(())
    }
}

/// Fully reopened immutable artifact. The bytes are crate-private so only a
/// later reviewed zero-choice bootstrap boundary may consume them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FixedOrdinaryWorkerArtifactV1 {
    pub(crate) manifest: UserWorkerReleaseManifestV1,
    pub(crate) image: Vec<u8>,
    pub(crate) review_contract_hash: String,
    pub(crate) review_completion_sha256: String,
    pub(crate) review_remeasurement_digest: String,
}

/// Exactly two artifacts, in rollback order. This type has no optional or
/// caller-selectable members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FixedOrdinaryWorkerArtifactPairV1 {
    pub(crate) predecessor: FixedOrdinaryWorkerArtifactV1,
    pub(crate) current: FixedOrdinaryWorkerArtifactV1,
}

pub(crate) fn fixed_ordinary_worker_artifact_provider_root() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\ProgramData\CatDesk\ReviewedOrdinaryWorkerArtifactsV1")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/var/lib/catdesk/reviewed-ordinary-worker-artifacts-v1")
    }
}

/// Zero-choice production reader. It never creates the fixed root; missing,
/// malformed, reparse-substituted, or noncanonical content is unavailable.
pub(crate) fn read_fixed_ordinary_worker_artifact_pair()
-> Result<FixedOrdinaryWorkerArtifactPairV1, String> {
    let root = provider_root(&fixed_ordinary_worker_artifact_provider_root())?;
    read_pair_from_root(&root)
}

fn provider_root(root: &Path) -> Result<ProtectedDirectoryGuard, String> {
    #[cfg(windows)]
    {
        if root == fixed_ordinary_worker_artifact_provider_root() {
            let program_data = PinnedDirectory::acquire(
                Path::new(r"C:\ProgramData"),
                "ordinary worker artifacts ProgramData parent",
            )?;
            let mut guard = ProtectedDirectoryGuard::from_pinned_root(program_data);
            for component in ["CatDesk", "ReviewedOrdinaryWorkerArtifactsV1"] {
                if !guard.descend_optional_existing(component, "ordinary worker artifacts root")? {
                    return Err("ordinary worker artifacts are unavailable".into());
                }
            }
            return Ok(guard);
        }
        #[cfg(test)]
        return ProtectedDirectoryGuard::acquire(root, "ordinary worker artifact test root");
        #[cfg(not(test))]
        return Err("ordinary worker artifacts are unavailable".into());
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        Err("ordinary worker artifacts are unavailable".into())
    }
}

fn read_pair_from_root(
    root: &ProtectedDirectoryGuard,
) -> Result<FixedOrdinaryWorkerArtifactPairV1, String> {
    let record_bytes = read_optional_relative_regular(
        root,
        PROVIDER_RECORD,
        MAX_RECORD,
        "ordinary worker artifact record",
    )?
    .ok_or_else(|| "ordinary worker artifacts are unavailable".to_string())?;
    let record: OrdinaryWorkerArtifactPairRecordV1 = serde_json::from_slice(&record_bytes)
        .map_err(|_| "ordinary worker artifact record is invalid".to_string())?;
    if canonical(&record)? != record_bytes {
        return Err("ordinary worker artifact record is noncanonical".into());
    }
    record.validate()?;
    let predecessor = read_artifact(root, &record.predecessor, PREDECESSOR_IMAGE)?;
    let current = read_artifact(root, &record.current, CURRENT_IMAGE)?;
    if predecessor.manifest.generation >= current.manifest.generation
        || predecessor.manifest.image_sha256 == current.manifest.image_sha256
    {
        return Err("ordinary worker artifact pair relation is invalid".into());
    }
    Ok(FixedOrdinaryWorkerArtifactPairV1 {
        predecessor,
        current,
    })
}

fn read_artifact(
    root: &ProtectedDirectoryGuard,
    binding: &OrdinaryWorkerArtifactBindingV1,
    image_name: &str,
) -> Result<FixedOrdinaryWorkerArtifactV1, String> {
    let image =
        read_optional_relative_regular(root, image_name, MAX_IMAGE, "ordinary worker image")?
            .ok_or_else(|| "ordinary worker image is unavailable".to_string())?;
    if image.len() as u64 != binding.image_length || digest(&image) != binding.image_sha256 {
        return Err("ordinary worker image binding is invalid".into());
    }
    let manifest = binding.release_manifest();
    manifest.validate()?;
    Ok(FixedOrdinaryWorkerArtifactV1 {
        manifest,
        image,
        review_contract_hash: binding.review_contract_hash.clone(),
        review_completion_sha256: binding.review_completion_sha256.clone(),
        review_remeasurement_digest: binding.review_remeasurement_digest.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT: AtomicU64 = AtomicU64::new(1);
    const T0382_UPSTREAM_AUTHORITY_MISSING: &str = "UPSTREAM_ARTIFACT_AUTHORITY_MISSING";

    /// This test-only classifier records the T-0382 decision without creating
    /// a production provisioner input or a caller-selectable authority.
    fn classify_t0382_reader_only_authority(has_accepted_fixed_pair_source: bool) -> &'static str {
        if has_accepted_fixed_pair_source {
            "FIXED_PAIR_PROVISIONER_READY"
        } else {
            T0382_UPSTREAM_AUTHORITY_MISSING
        }
    }

    struct TestRoot(PathBuf);
    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn root() -> TestRoot {
        let root = std::env::temp_dir().join(format!(
            "catdesk-t0381-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        TestRoot(root)
    }

    fn binding(
        generation: u64,
        artifact_id: &str,
        image: &[u8],
    ) -> OrdinaryWorkerArtifactBindingV1 {
        OrdinaryWorkerArtifactBindingV1 {
            generation,
            artifact_id: artifact_id.into(),
            image_sha256: digest(image),
            image_length: image.len() as u64,
            source_snapshot_id: format!("snapshot-{generation}"),
            review_session_id: format!("session-{generation}"),
            review_record_id: format!("record-{generation}"),
            review_authority_sha256: "a".repeat(64),
            review_contract_hash: "b".repeat(64),
            review_completion_sha256: "c".repeat(64),
            review_remeasurement_digest: "d".repeat(64),
            attestation_id: format!("attestation-{generation}"),
            attestation_sha256: "e".repeat(64),
            attestation_candidate_sha256: digest(image),
            attestation_candidate_length: image.len() as u64,
        }
    }

    fn record(
        predecessor: OrdinaryWorkerArtifactBindingV1,
        current: OrdinaryWorkerArtifactBindingV1,
    ) -> OrdinaryWorkerArtifactPairRecordV1 {
        OrdinaryWorkerArtifactPairRecordV1 {
            schema: SCHEMA,
            product: PRODUCT.into(),
            purpose: PURPOSE.into(),
            role: ROLE.into(),
            predecessor,
            current,
        }
    }

    fn write_pair(
        root: &Path,
        record: &OrdinaryWorkerArtifactPairRecordV1,
        predecessor: &[u8],
        current: &[u8],
    ) {
        fs::write(root.join(PROVIDER_RECORD), canonical(record).unwrap()).unwrap();
        fs::write(root.join(PREDECESSOR_IMAGE), predecessor).unwrap();
        fs::write(root.join(CURRENT_IMAGE), current).unwrap();
    }

    fn read_test(root: &Path) -> Result<FixedOrdinaryWorkerArtifactPairV1, String> {
        read_pair_from_root(&provider_root(root)?)
    }

    #[test]
    fn fixed_provider_reopens_exact_two_artifacts_and_preserves_provenance() {
        let root = root();
        let predecessor = b"reviewed predecessor";
        let current = b"reviewed current";
        let record = record(
            binding(7, PREDECESSOR_ARTIFACT, predecessor),
            binding(8, CURRENT_ARTIFACT, current),
        );
        write_pair(&root.0, &record, predecessor, current);
        let pair = read_test(&root.0).unwrap();
        assert_eq!(pair.predecessor.image, predecessor);
        assert_eq!(pair.current.image, current);
        assert_eq!(pair.predecessor.manifest.generation, 7);
        assert_eq!(pair.current.manifest.generation, 8);
        assert_eq!(pair.current.review_contract_hash, "b".repeat(64));
        assert_eq!(pair.current.review_completion_sha256, "c".repeat(64));
        assert_eq!(pair.current.review_remeasurement_digest, "d".repeat(64));
    }

    #[test]
    fn provider_refuses_missing_identical_stale_tampered_and_role_confused_pairs() {
        let root = root();
        let predecessor = b"reviewed predecessor";
        let current = b"reviewed current";
        let mut valid = record(
            binding(7, PREDECESSOR_ARTIFACT, predecessor),
            binding(8, CURRENT_ARTIFACT, current),
        );
        write_pair(&root.0, &valid, predecessor, current);
        fs::remove_file(root.0.join(PREDECESSOR_IMAGE)).unwrap();
        assert!(read_test(&root.0).is_err());
        write_pair(&root.0, &valid, predecessor, current);
        valid.current.generation = 7;
        write_pair(&root.0, &valid, predecessor, current);
        assert!(read_test(&root.0).is_err());
        valid = record(
            binding(7, PREDECESSOR_ARTIFACT, predecessor),
            binding(8, CURRENT_ARTIFACT, predecessor),
        );
        write_pair(&root.0, &valid, predecessor, predecessor);
        assert!(read_test(&root.0).is_err());
        valid = record(
            binding(7, PREDECESSOR_ARTIFACT, predecessor),
            binding(8, CURRENT_ARTIFACT, current),
        );
        write_pair(&root.0, &valid, predecessor, b"tampered");
        assert!(read_test(&root.0).is_err());
        valid.current.artifact_id = PREDECESSOR_ARTIFACT.into();
        write_pair(&root.0, &valid, predecessor, current);
        assert!(read_test(&root.0).is_err());
        valid = record(
            binding(7, PREDECESSOR_ARTIFACT, predecessor),
            binding(8, CURRENT_ARTIFACT, current),
        );
        valid.current.review_authority_sha256 = "not-a-digest".into();
        write_pair(&root.0, &valid, predecessor, current);
        assert!(read_test(&root.0).is_err());
        valid = record(
            binding(7, PREDECESSOR_ARTIFACT, predecessor),
            binding(8, CURRENT_ARTIFACT, current),
        );
        valid.current.attestation_candidate_sha256 = "f".repeat(64);
        write_pair(&root.0, &valid, predecessor, current);
        assert!(read_test(&root.0).is_err());

        write_pair(
            &root.0,
            &record(
                binding(7, PREDECESSOR_ARTIFACT, predecessor),
                binding(8, CURRENT_ARTIFACT, current),
            ),
            predecessor,
            current,
        );
        let replacement = root.0.join("outside.exe");
        fs::write(&replacement, current).unwrap();
        fs::remove_file(root.0.join(CURRENT_IMAGE)).unwrap();
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(&replacement, root.0.join(CURRENT_IMAGE)).is_ok() {
            assert!(read_test(&root.0).is_err());
        }
    }

    #[test]
    fn provider_is_canonical_no_choice_and_has_no_live_mutation_surface() {
        let root = root();
        let predecessor = b"reviewed predecessor";
        let current = b"reviewed current";
        let record = record(
            binding(7, PREDECESSOR_ARTIFACT, predecessor),
            binding(8, CURRENT_ARTIFACT, current),
        );
        fs::write(root.0.join(PROVIDER_RECORD), b"{\n}").unwrap();
        assert!(read_test(&root.0).is_err());
        write_pair(&root.0, &record, predecessor, current);
        let source = include_str!("ordinary_worker_artifact_provider.rs");
        let forbidden = [
            ["open", "fixed", "pipe", "client"].concat(),
            ["Register", "Backend"].concat(),
            ["wake", "_"].concat(),
            ["brow", "ser"].concat(),
            ["tun", "nel"].concat(),
            ["Create", "Process"].concat(),
            ["Program", " Files"].concat(),
            ["run", "as"].concat(),
            ["git", " push"].concat(),
        ];
        for forbidden in forbidden {
            assert!(
                !source.contains(&forbidden),
                "unexpected authority: {forbidden}"
            );
        }
        assert!(
            !source.contains("pub(crate) fn read_fixed_ordinary_worker_artifact_pair(\n    root")
        );
        assert!(read_test(&root.0).is_ok());
    }

    #[test]
    fn t0382_reader_only_authority_is_upstream_artifact_authority_missing() {
        // T-0381 has a fixed reader but no accepted producer or immutable pair
        // source. Treating the carrier itself as a producer would manufacture
        // the provenance that it is supposed to validate.
        assert_eq!(
            classify_t0382_reader_only_authority(false),
            T0382_UPSTREAM_AUTHORITY_MISSING
        );
        let source = include_str!("ordinary_worker_artifact_provider.rs");
        for producer_primitive in [
            [
                "write", "_unique", "_regular", "_for", "_atomic", "_replace",
            ]
            .concat(),
            ["commit", "_replace"].concat(),
            ["create", "_child"].concat(),
        ] {
            assert!(
                !source.contains(&producer_primitive),
                "T-0381 reader unexpectedly provisions artifacts"
            );
        }
    }
}
