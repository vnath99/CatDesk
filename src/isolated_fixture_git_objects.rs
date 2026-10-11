//! R3B4-B1: fixed read-only Git object byte retrieval for candidate fixtures.
//! A trusted commit/branch/CI witness is NOT supplied by this module.
//! Nothing here copies a working tree, publishes a manifest, starts a fixture,
//! or grants a new MCP, process-control, shell or production permission.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use crate::delegated::github_bootstrap::{TrustedGitIdentityV1, resolve_trusted_git_identity};
use crate::isolated_fixture_manifest::{
    CandidateBlob, MAX_FILE_BYTES, SnapshotManifest, assemble_candidate_manifest,
};

const MAX_PROBE_BYTES: usize = 32;
const GIT_OBJECT_TIMEOUT: Duration = Duration::from_secs(12);
const FIXED_SOURCES: [&str; 2] = ["Cargo.toml", "src/main.rs"];

fn lowercase_commit_sha(commit: &str) -> bool {
    commit.len() == 40
        && commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn fixed_object_spec(commit: &str, path: &str) -> Result<String, String> {
    if !lowercase_commit_sha(commit) || !FIXED_SOURCES.contains(&path) {
        return Err("candidate Git object request not in fixed scope".into());
    }
    Ok(format!("{commit}:{path}"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GitBlobPhase {
    Type,
    Size,
    Bytes,
}

fn fixed_cat_file_args(
    commit: &str,
    path: &str,
    phase: GitBlobPhase,
) -> Result<Vec<String>, String> {
    let object = fixed_object_spec(commit, path)?;
    let mut args = vec!["--no-replace-objects".into(), "cat-file".into()];
    match phase {
        GitBlobPhase::Type => args.push("-t".into()),
        GitBlobPhase::Size => args.push("-s".into()),
        GitBlobPhase::Bytes => args.push("blob".into()),
    }
    args.push(object);
    Ok(args)
}

/// Validate one complete 3-step result for a fixed commit:path object.
/// The runner will be separately responsible for byte limits, timeout and
/// executable identity. This does not assert a reviewed GitHub source.
fn verified_candidate_blob(kind: &[u8], size: &[u8], content: Vec<u8>) -> Result<Vec<u8>, String> {
    if kind != b"blob\n" {
        return Err("candidate Git object is not a blob".into());
    }
    let size_text = std::str::from_utf8(size).map_err(|_| "candidate Git object size invalid")?;
    if size_text.len() > MAX_PROBE_BYTES
        || !size_text.ends_with('\n')
        || size_text.trim_end_matches('\n').is_empty()
        || !size_text
            .trim_end_matches('\n')
            .bytes()
            .all(|b| b.is_ascii_digit())
    {
        return Err("candidate Git object size invalid".into());
    }
    let expected: u64 = size_text
        .trim_end_matches('\n')
        .parse()
        .map_err(|_| "candidate Git object size invalid")?;
    if expected > MAX_FILE_BYTES || content.len() as u64 != expected {
        return Err("candidate Git object byte count mismatch".into());
    }
    if std::str::from_utf8(&content).is_err() || content.contains(&0) {
        return Err("candidate Git object must be UTF-8 text".into());
    }
    Ok(content)
}

/// Closed, bounded, noninteractive Git plumbing. No shell or PATH lookup.
/// The canonical program comes from CatDesk's existing fixed-slot selector.
async fn read_git_output(
    workspace: &Path,
    trusted: &TrustedGitIdentityV1,
    args: &[String],
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let check = resolve_trusted_git_identity(workspace)
        .map_err(|_| "trusted Git executable unavailable")?;
    if &check != trusted {
        return Err("trusted Git executable identity drifted".into());
    }
    let mut child = Command::new(&trusted.canonical)
        .args(args)
        .current_dir(workspace)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "candidate Git object process unavailable")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("candidate Git object output unavailable")?;
    let task = async {
        let mut output = Vec::new();
        stdout
            .take((max_bytes as u64) + 1)
            .read_to_end(&mut output)
            .await
            .map_err(|_| "candidate Git output read failed")?;
        if output.len() > max_bytes {
            return Err("candidate Git output exceeded fixed bound".into());
        }
        let status = child
            .wait()
            .await
            .map_err(|_| "candidate Git object process wait failed")?;
        if !status.success() {
            return Err("candidate Git object request refused".into());
        }
        Ok(output)
    };
    timeout(GIT_OBJECT_TIMEOUT, task)
        .await
        .map_err(|_| "candidate Git object request timed out")?
}

/// Retrieve ONLY the two fixed Git commit tree blob objects from a local Git
/// repository, never mutable checked-out files. This is a *candidate* snapshot:
/// independent GitHub branch+CI attestation must be validated before any copy
/// or MCP serving. No caller-selectable source path or command accepted.
pub(crate) async fn read_candidate_git_objects(
    workspace: &Path,
    source_commit: &str,
    snapshot_id: &str,
) -> Result<(SnapshotManifest, Vec<(String, Vec<u8>)>), String> {
    if !lowercase_commit_sha(source_commit) {
        return Err("candidate Git source commit invalid".into());
    }
    let git = resolve_trusted_git_identity(workspace)
        .map_err(|_| "trusted Git executable unavailable")?;
    let mut candidates = Vec::with_capacity(FIXED_SOURCES.len());
    for source_path in FIXED_SOURCES {
        let kind = read_git_output(
            workspace,
            &git,
            &fixed_cat_file_args(source_commit, source_path, GitBlobPhase::Type)?,
            MAX_PROBE_BYTES,
        )
        .await?;
        let size = read_git_output(
            workspace,
            &git,
            &fixed_cat_file_args(source_commit, source_path, GitBlobPhase::Size)?,
            MAX_PROBE_BYTES,
        )
        .await?;
        // Read one extra byte to refuse unexpectedly large output.
        let data = read_git_output(
            workspace,
            &git,
            &fixed_cat_file_args(source_commit, source_path, GitBlobPhase::Bytes)?,
            MAX_FILE_BYTES as usize,
        )
        .await?;
        candidates.push((
            source_path.to_string(),
            verified_candidate_blob(&kind, &size, data)?,
        ));
    }
    let inputs = candidates
        .iter()
        .map(|(source_path, bytes)| CandidateBlob { source_path, bytes })
        .collect::<Vec<_>>();
    let manifest = assemble_candidate_manifest(source_commit, snapshot_id, &inputs)?;
    Ok((manifest, candidates))
}

#[cfg(test)]
mod tests {
    use super::*;
    const SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";

    #[test]
    fn fixed_commands_only_and_no_caller_source_path() {
        for source in FIXED_SOURCES {
            let spec = format!("{SHA}:{source}");
            assert_eq!(
                fixed_cat_file_args(SHA, source, GitBlobPhase::Type).unwrap(),
                ["--no-replace-objects", "cat-file", "-t", &spec]
            );
            assert_eq!(
                fixed_cat_file_args(SHA, source, GitBlobPhase::Size).unwrap(),
                ["--no-replace-objects", "cat-file", "-s", &spec]
            );
            assert_eq!(
                fixed_cat_file_args(SHA, source, GitBlobPhase::Bytes).unwrap(),
                ["--no-replace-objects", "cat-file", "blob", &spec]
            );
        }
        for path in [
            "../Cargo.toml",
            "src/main.rs/../.env",
            ".catdesk/config.toml",
            "src\\main.rs",
            "README.md",
            "C:/private",
        ] {
            assert!(fixed_cat_file_args(SHA, path, GitBlobPhase::Bytes).is_err());
        }
        assert!(fixed_cat_file_args("HEAD", "Cargo.toml", GitBlobPhase::Bytes).is_err());
        assert!(
            fixed_cat_file_args("a".repeat(40).as_str(), "Cargo.toml", GitBlobPhase::Bytes)
                .is_err()
        );
    }

    #[test]
    fn candidate_blob_rejects_wrong_kind_size_and_content() {
        assert_eq!(
            verified_candidate_blob(b"blob\n", b"4\n", b"rust".to_vec()).unwrap(),
            b"rust"
        );
        for wrong_type in [b"tree\n".as_slice(), b"commit\n", b"tag\n", b"blob"] {
            assert!(verified_candidate_blob(wrong_type, b"4\n", b"rust".to_vec()).is_err());
        }
        for wrong_size in [
            b"5\n".as_slice(),
            b"-1\n",
            b"0x4\n",
            b"4\n4\n",
            b"1".repeat(40).as_slice(),
        ] {
            assert!(verified_candidate_blob(b"blob\n", wrong_size, b"rust".to_vec()).is_err());
        }
        assert!(verified_candidate_blob(b"blob\n", b"524289\n", vec![b'a'; 4]).is_err());
        assert!(verified_candidate_blob(b"blob\n", b"4\n", b"r\0st".to_vec()).is_err());
        assert!(verified_candidate_blob(b"blob\n", b"2\n", vec![0xff, 0xff]).is_err());
    }
}
