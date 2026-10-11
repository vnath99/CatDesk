//! T0478 R3B4-A: schema checks for a *candidate* source snapshot manifest.
//! Syntax and byte-hash checks are NOT proof that a Git commit is trusted.
//! A subsequent reviewed producer must bind the source commit to GitHub
//! authority and copy only tracked Git-object bytes into a disposable root.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(crate) const MAX_MANIFEST_BYTES: usize = 16 * 1024;
pub(crate) const MAX_FILE_BYTES: u64 = 512 * 1024;
pub(crate) const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const MAX_FILES: usize = 32;
pub(crate) const SOURCE_REPOSITORY: &str = "https://github.com/vnath99/CatDesk.git";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SnapshotFile {
    pub fixture_leaf: String,
    pub source_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SnapshotManifest {
    pub schema_version: u32,
    pub source_repository: String,
    pub source_commit: String,
    pub snapshot_id: String,
    pub files: Vec<SnapshotFile>,
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// R3B4-A supports only these two reviewed source-to-flat-leaf mappings.
/// Widening the scope requires a separate review of the Git-object producer.
fn fixed_source_path(leaf: &str) -> Option<&'static str> {
    match leaf {
        "Cargo.toml" => Some("Cargo.toml"),
        "main.rs" => Some("src/main.rs"),
        _ => None,
    }
}

/// Parse one canonical JSON manifest and enforce a deterministic narrow scope.
/// This has NO side effects and confers NO authority to read a local Git tree.
pub(crate) fn parse_manifest_json(bytes: &[u8]) -> Result<SnapshotManifest, String> {
    if bytes.is_empty() || bytes.len() > MAX_MANIFEST_BYTES {
        return Err("snapshot manifest byte limit invalid".into());
    }
    let manifest: SnapshotManifest =
        serde_json::from_slice(bytes).map_err(|_| "snapshot manifest JSON invalid")?;
    validate_manifest(&manifest)?;
    let canonical =
        serde_json::to_vec(&manifest).map_err(|_| "snapshot manifest encoding invalid")?;
    if bytes != canonical {
        return Err("snapshot manifest must use canonical JSON encoding".into());
    }
    Ok(manifest)
}

pub(crate) fn validate_manifest(manifest: &SnapshotManifest) -> Result<(), String> {
    if manifest.schema_version != 1
        || manifest.source_repository != SOURCE_REPOSITORY
        || !is_lower_hex(&manifest.source_commit, 40)
    {
        return Err("snapshot source identity invalid".into());
    }
    let snapshot_id = Uuid::parse_str(&manifest.snapshot_id).map_err(|_| "snapshot id invalid")?;
    if snapshot_id.to_string() != manifest.snapshot_id {
        return Err("snapshot id not canonical".into());
    }
    if manifest.files.is_empty() || manifest.files.len() > MAX_FILES {
        return Err("snapshot file count invalid".into());
    }
    let mut seen_sources = BTreeSet::new();
    let mut previous_leaf: Option<&str> = None;
    let mut total = 0u64;
    for file in &manifest.files {
        if file.fixture_leaf.is_empty()
            || file.fixture_leaf.len() > 128
            || file.fixture_leaf.starts_with('.')
            || file.fixture_leaf.eq_ignore_ascii_case("config.toml")
            || !file
                .fixture_leaf
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
            || fixed_source_path(&file.fixture_leaf) != Some(file.source_path.as_str())
        {
            return Err("snapshot file mapping denied".into());
        }
        if previous_leaf.is_some_and(|prior| prior >= file.fixture_leaf.as_str())
            || !seen_sources.insert(&file.source_path)
        {
            return Err("snapshot files must be unique and sorted".into());
        }
        previous_leaf = Some(&file.fixture_leaf);
        if file.size_bytes > MAX_FILE_BYTES || !is_lower_hex(&file.sha256, 64) {
            return Err("snapshot file size or digest invalid".into());
        }
        total = total
            .checked_add(file.size_bytes)
            .ok_or("snapshot aggregate size invalid")?;
        if total > MAX_TOTAL_BYTES {
            return Err("snapshot aggregate size invalid".into());
        }
    }
    Ok(())
}

/// Compare bytes that the future trusted fixture reader obtains from ONE
/// no-follow regular file handle. This never opens a path or calls Git.
pub(crate) fn verify_file_bytes(entry: &SnapshotFile, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 != entry.size_bytes || bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("snapshot file length mismatch".into());
    }
    let digest = format!("{:x}", Sha256::digest(bytes));
    if digest != entry.sha256 {
        return Err("snapshot file digest mismatch".into());
    }
    Ok(())
}

/// Bytes supplied by a future trusted Git-object reader. This type itself
/// carries NO proof of object origin, GitHub review or runner identity.
pub(crate) struct CandidateBlob<'a> {
    pub source_path: &'a str,
    pub bytes: &'a [u8],
}

/// Convert *already supplied* candidate bytes to a deterministic manifest.
/// This cannot read a repository or claim its caller supplied trusted blobs.
pub(crate) fn assemble_candidate_manifest(
    source_commit: &str,
    snapshot_id: &str,
    blobs: &[CandidateBlob<'_>],
) -> Result<SnapshotManifest, String> {
    if blobs.is_empty() || blobs.len() > MAX_FILES {
        return Err("candidate source count invalid".into());
    }
    let mut files = Vec::with_capacity(blobs.len());
    let mut total = 0u64;
    for blob in blobs {
        let leaf = match blob.source_path {
            "Cargo.toml" => "Cargo.toml",
            "src/main.rs" => "main.rs",
            _ => return Err("candidate Git source mapping denied".into()),
        };
        let size = blob.bytes.len() as u64;
        if size > MAX_FILE_BYTES
            || std::str::from_utf8(blob.bytes).is_err()
            || blob.bytes.contains(&0)
        {
            return Err("candidate Git source is not bounded UTF-8 text".into());
        }
        total = total
            .checked_add(size)
            .ok_or("candidate source aggregate size invalid")?;
        if total > MAX_TOTAL_BYTES {
            return Err("candidate source aggregate size invalid".into());
        }
        files.push(SnapshotFile {
            fixture_leaf: leaf.into(),
            source_path: blob.source_path.into(),
            size_bytes: size,
            sha256: format!("{:x}", Sha256::digest(blob.bytes)),
        });
    }
    files.sort_by(|a, b| a.fixture_leaf.cmp(&b.fixture_leaf));
    let result = SnapshotManifest {
        schema_version: 1,
        source_repository: SOURCE_REPOSITORY.into(),
        source_commit: source_commit.into(),
        snapshot_id: snapshot_id.into(),
        files,
    };
    validate_manifest(&result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn manifest() -> SnapshotManifest {
        let data = b"sample cargo";
        SnapshotManifest {
            schema_version: 1,
            source_repository: SOURCE_REPOSITORY.into(),
            source_commit: "abcdef0123456789abcdef0123456789abcdef01".into(),
            snapshot_id: "8e1ae4ac-3d5f-46b3-9d5f-1e16bbf4f3e5".into(),
            files: vec![SnapshotFile {
                fixture_leaf: "Cargo.toml".into(),
                source_path: "Cargo.toml".into(),
                size_bytes: data.len() as u64,
                sha256: format!("{:x}", Sha256::digest(data)),
            }],
        }
    }

    #[test]
    fn candidate_bytes_produce_deterministic_schema_without_git_access() {
        let baseline = manifest();
        let blobs = [
            CandidateBlob {
                source_path: "src/main.rs",
                bytes: b"fn main() {}",
            },
            CandidateBlob {
                source_path: "Cargo.toml",
                bytes: b"sample cargo",
            },
        ];
        let built =
            assemble_candidate_manifest(&baseline.source_commit, &baseline.snapshot_id, &blobs)
                .expect("two scoped UTF-8 candidate blobs");
        assert_eq!(built.files.len(), 2);
        assert_eq!(built.files[0].fixture_leaf, "Cargo.toml");
        assert_eq!(built.files[1].fixture_leaf, "main.rs");
        assert!(verify_file_bytes(&built.files[0], blobs[1].bytes).is_ok());
        assert!(verify_file_bytes(&built.files[1], blobs[0].bytes).is_ok());
        let canonical = serde_json::to_vec(&built).unwrap();
        assert_eq!(parse_manifest_json(&canonical).unwrap(), built);
    }

    #[test]
    fn candidate_blob_input_fails_closed_on_unknown_duplicate_or_binary() {
        let m = manifest();
        let commit = &m.source_commit;
        let id = &m.snapshot_id;
        for source_path in ["../Cargo.toml", "secret.txt", "config.toml", "src\\main.rs"] {
            assert!(
                assemble_candidate_manifest(
                    commit,
                    id,
                    &[CandidateBlob {
                        source_path,
                        bytes: b"valid"
                    }],
                )
                .is_err(),
                "candidate path {source_path}"
            );
        }
        let duplicate = [
            CandidateBlob {
                source_path: "Cargo.toml",
                bytes: b"one",
            },
            CandidateBlob {
                source_path: "Cargo.toml",
                bytes: b"two",
            },
        ];
        assert!(assemble_candidate_manifest(commit, id, &duplicate).is_err());
        let invalid_sources: [&[u8]; 3] = [b"bad\0bytes", b"\xff", b"\xc0\xaf"];
        for data in invalid_sources {
            assert!(
                assemble_candidate_manifest(
                    commit,
                    id,
                    &[CandidateBlob {
                        source_path: "Cargo.toml",
                        bytes: data
                    }],
                )
                .is_err()
            );
        }
        let oversized = vec![b'A'; MAX_FILE_BYTES as usize + 1];
        assert!(
            assemble_candidate_manifest(
                commit,
                id,
                &[CandidateBlob {
                    source_path: "Cargo.toml",
                    bytes: &oversized
                }],
            )
            .is_err()
        );
        assert!(assemble_candidate_manifest(commit, id, &[]).is_err());
        assert!(
            assemble_candidate_manifest(
                "unverified-commit",
                id,
                &[CandidateBlob {
                    source_path: "Cargo.toml",
                    bytes: b"valid"
                }],
            )
            .is_err()
        );
    }

    #[test]
    fn valid_canonical_manifest_and_bytes() {
        let m = manifest();
        let bytes = serde_json::to_vec(&m).unwrap();
        assert_eq!(parse_manifest_json(&bytes).unwrap(), m);
        assert!(verify_file_bytes(&m.files[0], b"sample cargo").is_ok());
        assert!(verify_file_bytes(&m.files[0], b"sample cargO").is_err());
        assert!(verify_file_bytes(&m.files[0], b"short").is_err());
        assert!(parse_manifest_json(b"  {").is_err());
        assert!(parse_manifest_json(&vec![b'X'; MAX_MANIFEST_BYTES + 1]).is_err());
        let pretty = serde_json::to_vec_pretty(&m).unwrap();
        assert!(parse_manifest_json(&pretty).is_err());
    }

    #[test]
    fn rejects_unknown_fields_and_missing_commit() {
        let m = manifest();
        let mut value = serde_json::to_value(&m).unwrap();
        value["unexpected"] = json!(true);
        assert!(parse_manifest_json(&serde_json::to_vec(&value).unwrap()).is_err());
        let mut value = serde_json::to_value(&m).unwrap();
        value.as_object_mut().unwrap().remove("sourceCommit");
        assert!(parse_manifest_json(&serde_json::to_vec(&value).unwrap()).is_err());
        let mut value = serde_json::to_value(&m).unwrap();
        value["files"][0]["extra"] = json!("unexpected");
        assert!(parse_manifest_json(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn rejects_untrusted_source_identity() {
        let mut m = manifest();
        for invalid in ["", "A".repeat(40).as_str(), "zzzz", "0".repeat(41).as_str()] {
            m.source_commit = invalid.into();
            assert!(validate_manifest(&m).is_err());
        }
        m = manifest();
        m.source_repository = "https://github.com/example/CatDesk.git".into();
        assert!(validate_manifest(&m).is_err());
        m = manifest();
        m.snapshot_id = "../fixture".into();
        assert!(validate_manifest(&m).is_err());
        m = manifest();
        m.snapshot_id = m.snapshot_id.to_uppercase();
        assert!(validate_manifest(&m).is_err());
        m = manifest();
        m.schema_version = 2;
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn denies_unsafe_or_unmapped_names() {
        for invalid in [
            "../Cargo.toml",
            "/Cargo.toml",
            "C:\\secret",
            ".env",
            "config.toml",
            "CON",
            "src/main.rs",
            "README.md",
            "main.rs.",
        ] {
            let mut m = manifest();
            m.files[0].fixture_leaf = invalid.into();
            assert!(validate_manifest(&m).is_err(), "name {invalid}");
        }
        let mut m = manifest();
        m.files[0].source_path = "src/main.rs".into();
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn rejects_duplicate_unsorted_and_oversized_files() {
        let mut m = manifest();
        m.files.push(m.files[0].clone());
        assert!(validate_manifest(&m).is_err());
        let mut m = manifest();
        m.files[0].fixture_leaf = "main.rs".into();
        m.files[0].source_path = "src/main.rs".into();
        m.files.push(SnapshotFile {
            fixture_leaf: "Cargo.toml".into(),
            source_path: "Cargo.toml".into(),
            size_bytes: 0,
            sha256: "0".repeat(64),
        });
        assert!(validate_manifest(&m).is_err(), "unsorted");
        let mut m = manifest();
        m.files[0].size_bytes = MAX_FILE_BYTES + 1;
        assert!(validate_manifest(&m).is_err());
        let mut m = manifest();
        m.files[0].sha256 = "A".repeat(64);
        assert!(validate_manifest(&m).is_err());
        let mut m = manifest();
        m.files[0].sha256 = "bad".into();
        assert!(validate_manifest(&m).is_err());
        let mut m = manifest();
        m.files.clear();
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn rejects_duplicate_json_keys() {
        // serde JSON alone uses last-key-wins, so a parser that roundtrips
        // into canonical bytes must reject ambiguous duplicate keys.
        let m = manifest();
        let mut canonical = String::from_utf8(serde_json::to_vec(&m).unwrap()).unwrap();
        canonical.insert_str(1, "\"schemaVersion\":1,");
        assert!(parse_manifest_json(canonical.as_bytes()).is_err());
        let value: Value = serde_json::from_slice(&serde_json::to_vec(&m).unwrap()).unwrap();
        assert_eq!(value["schemaVersion"], 1);
    }
}
