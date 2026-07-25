use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::EXECUTION_CONTRACT_SCHEMA_VERSION;
use super::contracts::{ExecutionContractV1, PatchId, RunId, TurnId, validate_contract};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchProposalV1 {
    pub schema_version: u32,
    pub patch_id: PatchId,
    pub parent_patch_id: Option<PatchId>,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub base_snapshot_hash: String,
    pub target_paths: Vec<String>,
    pub expected_preimage_hashes: Vec<FileHashV1>,
    pub operations: Vec<ReplaceOperationV1>,
    pub model_rationale: String,
    pub claimed_acceptance_criteria: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHashV1 {
    pub path: String,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceOperationV1 {
    pub path: String,
    pub old: String,
    pub new: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PatchPreviewStatus {
    Accepted,
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchPreviewV1 {
    pub patch_id: PatchId,
    pub status: PatchPreviewStatus,
    pub target_paths: Vec<String>,
    pub preview_diff: String,
    pub rejection_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PatchApplyStatus {
    Applied,
    Rejected,
    Conflicted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchApplyResultV1 {
    pub patch_id: PatchId,
    pub status: PatchApplyStatus,
    pub actual_paths_changed: Vec<String>,
    pub before_hashes: Vec<FileHashV1>,
    pub after_hashes: Vec<FileHashV1>,
    pub actual_diff: String,
    pub rejection_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchComparisonV1 {
    pub parent_patch_id: PatchId,
    pub candidate_patch_id: PatchId,
    pub files_added: Vec<String>,
    pub files_removed: Vec<String>,
    pub files_modified: Vec<String>,
    pub superseded_operations: usize,
    pub net_operation_delta: isize,
    pub scope_warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActualDiffArtifactV1 {
    pub base_ref: String,
    pub paths: Vec<String>,
    pub diff_hash: String,
    pub diff: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PatchError {
    Io(String),
    Validation(String),
    OutOfScope(String),
    StaleBase(String),
    Conflict(String),
    Git(String),
    FalseSuccess(String),
}

pub struct PatchEngine<'a> {
    workspace_root: &'a Path,
    contract: &'a ExecutionContractV1,
}

impl<'a> PatchEngine<'a> {
    pub fn new(
        workspace_root: &'a Path,
        contract: &'a ExecutionContractV1,
    ) -> Result<Self, PatchError> {
        validate_contract(contract).map_err(PatchError::Validation)?;
        Ok(Self {
            workspace_root,
            contract,
        })
    }

    pub fn preview(&self, proposal: &PatchProposalV1) -> Result<PatchPreviewV1, PatchError> {
        self.validate_proposal(proposal)?;
        let before_after = self.compute_before_after(proposal)?;
        Ok(PatchPreviewV1 {
            patch_id: proposal.patch_id.clone(),
            status: PatchPreviewStatus::Accepted,
            target_paths: proposal.target_paths.clone(),
            preview_diff: render_preview_diff(&before_after),
            rejection_reason: None,
        })
    }

    pub fn apply(&self, proposal: &PatchProposalV1) -> Result<PatchApplyResultV1, PatchError> {
        self.preview(proposal)?;
        let before_after = self.compute_before_after(proposal)?;
        let mut before_hashes = Vec::new();
        let mut after_hashes = Vec::new();
        for file in &before_after {
            before_hashes.push(FileHashV1 {
                path: file.path.clone(),
                hash: stable_text_hash(&file.before),
            });
            fs::write(
                contained_path(self.workspace_root, &file.path)?,
                &file.after,
            )?;
            after_hashes.push(FileHashV1 {
                path: file.path.clone(),
                hash: stable_text_hash(&file.after),
            });
        }
        Ok(PatchApplyResultV1 {
            patch_id: proposal.patch_id.clone(),
            status: PatchApplyStatus::Applied,
            actual_paths_changed: before_after.iter().map(|file| file.path.clone()).collect(),
            before_hashes,
            after_hashes,
            actual_diff: self.actual_git_diff(&proposal.target_paths)?.diff,
            rejection_reason: None,
        })
    }

    pub fn actual_git_diff(&self, paths: &[String]) -> Result<ActualDiffArtifactV1, PatchError> {
        let mut command = Command::new("git");
        command.arg("diff").arg("--");
        for path in paths {
            validate_relative_path(path)?;
            command.arg(path);
        }
        let output = command
            .current_dir(self.workspace_root)
            .output()
            .map_err(|error| PatchError::Git(error.to_string()))?;
        if !output.status.success() {
            return Err(PatchError::Git(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }
        let diff = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(ActualDiffArtifactV1 {
            base_ref: "HEAD".into(),
            paths: paths.to_vec(),
            diff_hash: stable_text_hash(&diff),
            diff,
        })
    }

    fn validate_proposal(&self, proposal: &PatchProposalV1) -> Result<(), PatchError> {
        if proposal.schema_version != EXECUTION_CONTRACT_SCHEMA_VERSION {
            return Err(PatchError::Validation("unsupported patch schema".into()));
        }
        if proposal.operations.is_empty() || proposal.target_paths.is_empty() {
            return Err(PatchError::Validation("patch has no operations".into()));
        }
        for path in &proposal.target_paths {
            validate_relative_path(path)?;
            if !self.path_allowed(path) {
                return Err(PatchError::OutOfScope(path.clone()));
            }
        }
        for operation in &proposal.operations {
            validate_relative_path(&operation.path)?;
            if !proposal.target_paths.contains(&operation.path) {
                return Err(PatchError::OutOfScope(operation.path.clone()));
            }
            if operation.old == operation.new {
                return Err(PatchError::Validation(
                    "replace operation old and new text must differ".into(),
                ));
            }
        }
        for expected in &proposal.expected_preimage_hashes {
            let text = fs::read_to_string(contained_path(self.workspace_root, &expected.path)?)?;
            let actual = stable_text_hash(&text);
            if actual != expected.hash {
                return Err(PatchError::StaleBase(expected.path.clone()));
            }
        }
        Ok(())
    }

    fn compute_before_after(
        &self,
        proposal: &PatchProposalV1,
    ) -> Result<Vec<FileBeforeAfter>, PatchError> {
        let mut files = Vec::new();
        for path in &proposal.target_paths {
            let mut text = fs::read_to_string(contained_path(self.workspace_root, path)?)?;
            let before = text.clone();
            for operation in proposal.operations.iter().filter(|op| &op.path == path) {
                let count = text.matches(&operation.old).count();
                if count != 1 {
                    return Err(PatchError::Conflict(format!(
                        "{} matched {} times in {}",
                        operation.old, count, path
                    )));
                }
                text = text.replacen(&operation.old, &operation.new, 1);
            }
            files.push(FileBeforeAfter {
                path: path.clone(),
                before,
                after: text,
            });
        }
        Ok(files)
    }

    fn path_allowed(&self, path: &str) -> bool {
        let normalized = path.replace('\\', "/");
        if self
            .contract
            .forbidden_paths
            .iter()
            .any(|forbidden| path_is_under(&normalized, forbidden))
        {
            return false;
        }
        self.contract
            .allowed_paths
            .iter()
            .any(|allowed| path_is_under(&normalized, allowed))
    }
}

pub fn compare_patches(parent: &PatchProposalV1, candidate: &PatchProposalV1) -> PatchComparisonV1 {
    let parent_files = parent.target_paths.iter().cloned().collect::<BTreeSet<_>>();
    let candidate_files = candidate
        .target_paths
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let files_added = candidate_files
        .difference(&parent_files)
        .cloned()
        .collect::<Vec<_>>();
    let files_removed = parent_files
        .difference(&candidate_files)
        .cloned()
        .collect::<Vec<_>>();
    let files_modified = candidate_files
        .intersection(&parent_files)
        .cloned()
        .collect::<Vec<_>>();
    let superseded_operations = parent
        .operations
        .iter()
        .filter(|old| {
            candidate
                .operations
                .iter()
                .any(|new| new.path == old.path && new.old == old.old && new.new != old.new)
        })
        .count();
    let scope_warnings = candidate
        .operations
        .iter()
        .filter(|operation| !candidate.target_paths.contains(&operation.path))
        .map(|operation| format!("operation outside target paths: {}", operation.path))
        .collect();
    PatchComparisonV1 {
        parent_patch_id: parent.patch_id.clone(),
        candidate_patch_id: candidate.patch_id.clone(),
        files_added,
        files_removed,
        files_modified,
        superseded_operations,
        net_operation_delta: candidate.operations.len() as isize - parent.operations.len() as isize,
        scope_warnings,
    }
}

pub fn verify_model_completion_claim(
    model_claim: &str,
    tests_passed: bool,
    actual_diff: &ActualDiffArtifactV1,
) -> Result<(), PatchError> {
    if model_claim.trim().is_empty() {
        return Err(PatchError::FalseSuccess("empty completion claim".into()));
    }
    if !tests_passed {
        return Err(PatchError::FalseSuccess(
            "model completion claim rejected because tests did not pass".into(),
        ));
    }
    if actual_diff.diff.trim().is_empty() {
        return Err(PatchError::FalseSuccess(
            "model completion claim rejected because actual diff is empty".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct FileBeforeAfter {
    path: String,
    before: String,
    after: String,
}

fn render_preview_diff(files: &[FileBeforeAfter]) -> String {
    let mut diff = String::new();
    for file in files {
        diff.push_str(&format!("--- a/{}\n+++ b/{}\n", file.path, file.path));
        diff.push_str("@@ structured-replace @@\n");
        diff.push_str(&format!("-{}\n", file.before.trim_end()));
        diff.push_str(&format!("+{}\n", file.after.trim_end()));
    }
    diff
}

fn path_is_under(path: &str, root: &str) -> bool {
    let root = root.replace('\\', "/");
    path == root || path.starts_with(&format!("{root}/"))
}

fn validate_relative_path(path: &str) -> Result<(), PatchError> {
    let relative = Path::new(path);
    if path.trim().is_empty()
        || relative.is_absolute()
        || path.contains('\0')
        || path.contains(':')
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(PatchError::OutOfScope(path.into()));
    }
    Ok(())
}

fn contained_path(root: &Path, relative_path: &str) -> Result<PathBuf, PatchError> {
    validate_relative_path(relative_path)?;
    let root = root.canonicalize()?;
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let canonical_parent = path
        .parent()
        .ok_or_else(|| PatchError::OutOfScope(relative_path.into()))?
        .canonicalize()?;
    if !canonical_parent.starts_with(&root) {
        return Err(PatchError::OutOfScope(relative_path.into()));
    }
    Ok(path)
}

pub fn stable_text_hash(text: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}")
}

impl From<std::io::Error> for PatchError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workspace(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("catdesk-patch-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).expect("clear temp workspace");
        }
        fs::create_dir_all(root.join("src")).expect("create src");
        Command::new("git")
            .arg("init")
            .current_dir(&root)
            .output()
            .expect("git init");
        root
    }

    fn commit_all(root: &Path) {
        Command::new("git")
            .arg("add")
            .arg(".")
            .current_dir(root)
            .output()
            .expect("git add");
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=CatDesk Test",
                "-c",
                "user.email=catdesk@example.invalid",
                "commit",
                "-m",
                "baseline",
            ])
            .current_dir(root)
            .output()
            .expect("git commit");
        assert!(
            output.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn contract(root: &Path) -> ExecutionContractV1 {
        let mut contract: ExecutionContractV1 = serde_json::from_str(include_str!(
            "../../tests/fixtures/delegated/execution_contract_v1.json"
        ))
        .expect("fixture parses");
        contract.task_id = "run-t0017".into();
        contract.workspace = root.display().to_string();
        contract.allowed_paths = vec!["src".into()];
        contract.forbidden_paths = vec![".git".into(), "target".into()];
        contract
    }

    fn proposal(
        root: &Path,
        patch_id: &str,
        parent: Option<PatchId>,
        old: &str,
        new: &str,
    ) -> PatchProposalV1 {
        let path = "src/bug.txt";
        let text = fs::read_to_string(root.join(path)).expect("read bug file");
        PatchProposalV1 {
            schema_version: 1,
            patch_id: PatchId::new(patch_id).expect("patch id"),
            parent_patch_id: parent,
            run_id: RunId::new("run-t0017").expect("run id"),
            turn_id: TurnId::new("turn-1").expect("turn id"),
            base_snapshot_hash: stable_text_hash(&text),
            target_paths: vec![path.into()],
            expected_preimage_hashes: vec![FileHashV1 {
                path: path.into(),
                hash: stable_text_hash(&text),
            }],
            operations: vec![ReplaceOperationV1 {
                path: path.into(),
                old: old.into(),
                new: new.into(),
            }],
            model_rationale: "fix disposable bug".into(),
            claimed_acceptance_criteria: vec!["test passes".into()],
        }
    }

    #[test]
    fn patch_preview_and_apply_records_actual_diff() {
        let root = temp_workspace("apply");
        fs::write(root.join("src/bug.txt"), "answer=41\n").expect("write bug");
        commit_all(&root);
        let contract = contract(&root);
        let engine = PatchEngine::new(&root, &contract).expect("engine");
        let proposal = proposal(&root, "patch-1", None, "answer=41", "answer=42");
        let preview = engine.preview(&proposal).expect("preview");
        assert_eq!(preview.status, PatchPreviewStatus::Accepted);
        assert!(preview.preview_diff.contains("answer=42"));
        let result = engine.apply(&proposal).expect("apply");
        assert_eq!(result.status, PatchApplyStatus::Applied);
        assert!(result.actual_diff.contains("answer=42"));
        assert_ne!(result.before_hashes[0].hash, result.after_hashes[0].hash);
    }

    #[test]
    fn stale_base_patch_is_rejected() {
        let root = temp_workspace("stale");
        fs::write(root.join("src/bug.txt"), "answer=41\n").expect("write bug");
        commit_all(&root);
        let contract = contract(&root);
        let engine = PatchEngine::new(&root, &contract).expect("engine");
        let mut proposal = proposal(&root, "patch-stale", None, "answer=41", "answer=42");
        proposal.expected_preimage_hashes[0].hash = "fnv1a64:stale".into();
        assert!(matches!(
            engine.preview(&proposal),
            Err(PatchError::StaleBase(path)) if path == "src/bug.txt"
        ));
    }

    #[test]
    fn out_of_scope_patch_is_blocked() {
        let root = temp_workspace("scope");
        fs::write(root.join("src/bug.txt"), "answer=41\n").expect("write bug");
        fs::write(root.join("secret.txt"), "secret\n").expect("write secret");
        commit_all(&root);
        let contract = contract(&root);
        let engine = PatchEngine::new(&root, &contract).expect("engine");
        let mut proposal = proposal(&root, "patch-scope", None, "secret", "public");
        proposal.target_paths = vec!["secret.txt".into()];
        proposal.operations[0].path = "secret.txt".into();
        proposal.expected_preimage_hashes = vec![FileHashV1 {
            path: "secret.txt".into(),
            hash: stable_text_hash("secret\n"),
        }];
        assert!(matches!(
            engine.preview(&proposal),
            Err(PatchError::OutOfScope(path)) if path == "secret.txt"
        ));
    }

    #[test]
    fn patch_revision_compares_to_parent() {
        let root = temp_workspace("compare");
        fs::write(root.join("src/bug.txt"), "answer=41\n").expect("write bug");
        commit_all(&root);
        let parent = proposal(&root, "patch-parent", None, "answer=41", "answer=43");
        let candidate = proposal(
            &root,
            "patch-child",
            Some(parent.patch_id.clone()),
            "answer=41",
            "answer=42",
        );
        let comparison = compare_patches(&parent, &candidate);
        assert_eq!(comparison.parent_patch_id, parent.patch_id);
        assert_eq!(comparison.candidate_patch_id, candidate.patch_id);
        assert_eq!(comparison.superseded_operations, 1);
        assert_eq!(comparison.files_modified, vec!["src/bug.txt"]);
    }

    #[test]
    fn model_cannot_self_declare_verified_success() {
        let artifact = ActualDiffArtifactV1 {
            base_ref: "HEAD".into(),
            paths: vec!["src/bug.txt".into()],
            diff_hash: stable_text_hash(""),
            diff: String::new(),
        };
        assert!(matches!(
            verify_model_completion_claim("done", true, &artifact),
            Err(PatchError::FalseSuccess(message)) if message.contains("actual diff")
        ));
        let artifact = ActualDiffArtifactV1 {
            diff: "diff --git a/src/bug.txt b/src/bug.txt".into(),
            diff_hash: stable_text_hash("diff"),
            ..artifact
        };
        assert!(matches!(
            verify_model_completion_claim("done", false, &artifact),
            Err(PatchError::FalseSuccess(message)) if message.contains("tests")
        ));
        assert!(verify_model_completion_claim("done", true, &artifact).is_ok());
    }

    #[test]
    fn disposable_bug_fix_cycle_completes_after_repair() {
        let root = temp_workspace("cycle");
        fs::write(root.join("src/bug.txt"), "answer=41\n").expect("write bug");
        commit_all(&root);
        let contract = contract(&root);
        let engine = PatchEngine::new(&root, &contract).expect("engine");

        let bad = proposal(&root, "patch-bad", None, "answer=41", "answer=43");
        engine.apply(&bad).expect("apply bad patch");
        let test_passed = fs::read_to_string(root.join("src/bug.txt"))
            .expect("read")
            .contains("answer=42");
        assert!(!test_passed);

        let repaired = proposal(
            &root,
            "patch-good",
            Some(bad.patch_id.clone()),
            "answer=43",
            "answer=42",
        );
        let comparison = compare_patches(&bad, &repaired);
        assert_eq!(comparison.files_modified, vec!["src/bug.txt"]);
        let result = engine.apply(&repaired).expect("apply repaired patch");
        let test_passed = fs::read_to_string(root.join("src/bug.txt"))
            .expect("read")
            .contains("answer=42");
        assert!(test_passed);
        let diff = ActualDiffArtifactV1 {
            base_ref: "HEAD".into(),
            paths: result.actual_paths_changed,
            diff_hash: stable_text_hash(&result.actual_diff),
            diff: result.actual_diff,
        };
        verify_model_completion_claim("fixed", test_passed, &diff).expect("verified");
    }
}
