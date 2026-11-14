//! Closed execution seam for the reviewed T-0407 reconstruction snapshot.
//!
//! The public MCP route supplies only a review record, typed approval and the
//! PREPARE/CONFIRM/RESULT verb.  Repository, branch, paths, commit message and
//! every Git argv below are fixed by this module.

use std::{fs, path::Path, process::Command};

use sha2::{Digest, Sha256};

use super::{
    autonomous_contract::{AutonomousApprovalV1, AutonomousGitActionV1, AutonomousPolicyEngineV1},
    github_bootstrap::{TrustedGitIdentityV1, resolve_trusted_git_identity},
    github_publication::{
        FrozenPublicationManifestErrorV1, GithubPublicationCommitIntentV1,
        GithubPublicationCommitReceiptV1, GithubPublicationDescriptorV1,
        GithubPublicationJournalV1, GithubPublicationOperationV1, GithubPublicationPermitV1,
        GithubPublicationPreparedV1, GithubPublicationRecordStateV1, GithubPublicationRequestV1,
        GithubRepositoryEvidenceV1, parse_t0407_reconstruction_manifest,
        publication_commit_message,
    },
};

const TOKEN_TTL_SECONDS: u64 = 300;
/// The frozen R12 publication set contains 949 literal includes plus three
/// separately reviewed artifacts, so the exact staged-name
/// readback needs a bounded allowance above a small diagnostic prefix.
const MAX_GIT_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GitRunOutputV1 {
    pub success: bool,
    pub stdout: Vec<u8>,
}

pub(crate) trait GithubPublicationGitRunnerV1 {
    fn identity(&self) -> Result<TrustedGitIdentityV1, ()>;
    fn run(&self, identity: &TrustedGitIdentityV1, argv: &[String]) -> Result<GitRunOutputV1, ()>;
}

pub(crate) struct SystemGithubPublicationGitRunnerV1 {
    pub workspace: std::path::PathBuf,
}

impl GithubPublicationGitRunnerV1 for SystemGithubPublicationGitRunnerV1 {
    fn identity(&self) -> Result<TrustedGitIdentityV1, ()> {
        resolve_trusted_git_identity(&self.workspace).map_err(|_| ())
    }

    fn run(&self, identity: &TrustedGitIdentityV1, argv: &[String]) -> Result<GitRunOutputV1, ()> {
        let output = Command::new(&identity.canonical)
            .args(argv)
            .current_dir(&self.workspace)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .map_err(|_| ())?;
        if output.stdout.len() > MAX_GIT_OUTPUT_BYTES {
            return Err(());
        }
        Ok(GitRunOutputV1 {
            success: output.status.success(),
            stdout: output.stdout,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PublicationReviewAuthorityV1 {
    pub record_id: String,
    pub digest: String,
    pub descriptor: GithubPublicationDescriptorV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PublicationPrepareOutcomeV1 {
    pub confirmation_token: String,
    pub fingerprint: String,
    pub expires_at_unix: u64,
    pub include_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PublicationExecutorErrorV1 {
    Manifest,
    Repository,
    StagedState,
    ReviewAuthority,
    Approval,
    Token,
    Expired,
    EvidenceDrift,
    Commit,
    Dispatch,
    Reconciliation,
    RecoveryRequired,
}

impl PublicationExecutorErrorV1 {
    pub(crate) const fn reason_code(self) -> &'static str {
        match self {
            Self::Manifest => "GITHUB_PUBLICATION_MANIFEST_REFUSED",
            Self::Repository => "GITHUB_PUBLICATION_REPOSITORY_REFUSED",
            Self::StagedState => "GITHUB_PUBLICATION_STAGED_STATE_REFUSED",
            Self::ReviewAuthority => "GITHUB_PUBLICATION_REVIEW_AUTHORITY_REFUSED",
            Self::Approval => "GITHUB_PUBLICATION_APPROVAL_REFUSED",
            Self::Token => "GITHUB_PUBLICATION_CONFIRMATION_TOKEN_REFUSED",
            Self::Expired => "GITHUB_PUBLICATION_CONFIRMATION_EXPIRED",
            Self::EvidenceDrift => "GITHUB_PUBLICATION_EVIDENCE_DRIFT",
            Self::Commit => "GITHUB_PUBLICATION_LOCAL_COMMIT_FAILED",
            Self::Dispatch => "GITHUB_PUBLICATION_REMOTE_OUTCOME_UNKNOWN",
            Self::Reconciliation => "GITHUB_PUBLICATION_REMOTE_RECONCILIATION_REQUIRED",
            Self::RecoveryRequired => "GITHUB_PUBLICATION_RECOVERY_REQUIRED",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProbeV1 {
    repository: GithubRepositoryEvidenceV1,
    paths: Vec<String>,
    fingerprint: String,
}

pub(crate) fn prepare<R: GithubPublicationGitRunnerV1>(
    workspace: &Path,
    policy: &AutonomousPolicyEngineV1,
    journal: &mut GithubPublicationJournalV1,
    review: &PublicationReviewAuthorityV1,
    approval: AutonomousApprovalV1,
    runner: &R,
    now_unix: u64,
) -> Result<PublicationPrepareOutcomeV1, PublicationExecutorErrorV1> {
    if !valid_review(review) {
        return Err(PublicationExecutorErrorV1::ReviewAuthority);
    }
    policy
        .permit_git_action(AutonomousGitActionV1::LocalCommit)
        .map_err(|_| PublicationExecutorErrorV1::Approval)?;
    let git = runner
        .identity()
        .map_err(|_| PublicationExecutorErrorV1::Repository)?;
    let probe = probe(workspace, runner, &git, &review.descriptor, true)?;
    let request = GithubPublicationRequestV1 {
        operation: GithubPublicationOperationV1::PushFeatureBranch,
        repository: probe.repository.clone(),
        approval,
    };
    let permit = journal
        .authorize(policy, &request, now_unix)
        .map_err(|_| PublicationExecutorErrorV1::Approval)?;
    let token = confirmation_token(&permit, &probe.fingerprint, now_unix);
    let prepared = GithubPublicationPreparedV1 {
        schema_version: 1,
        idempotency_key: permit.idempotency_key().into(),
        intent_hash: permit.intent_hash().into(),
        review_record_id: review.record_id.clone(),
        review_digest: review.digest.clone(),
        manifest_path: review.descriptor.manifest_path.clone(),
        manifest_sha256: review.descriptor.manifest_sha256.clone(),
        manifest_include_count: review.descriptor.include_count,
        archival_exclusion_count: review.descriptor.archival_exclusion_count,
        runtime_generated_exclusion_count: review.descriptor.runtime_generated_exclusion_count,
        supplemental_paths: review.descriptor.supplemental_paths.clone(),
        evidence_fingerprint: probe.fingerprint.clone(),
        expires_at_unix: now_unix.saturating_add(TOKEN_TTL_SECONDS),
        repository: probe.repository,
        git_executable_fingerprint: git.fingerprint,
        git_selected_slot: git.slot,
        git_selected_identity: git.canonical.to_string_lossy().into_owned(),
        local_commit_intent: None,
        local_commit_receipt: None,
        push_expected_commit: None,
    };
    journal
        .bind_prepared(token.clone(), &permit, prepared)
        .map_err(|_| PublicationExecutorErrorV1::RecoveryRequired)?;
    Ok(PublicationPrepareOutcomeV1 {
        confirmation_token: token,
        fingerprint: probe.fingerprint,
        expires_at_unix: now_unix.saturating_add(TOKEN_TTL_SECONDS),
        include_count: probe.paths.len(),
    })
}

/// Stage the frozen list and bind its tree. The caller persists this intent
/// before execute_local_commit can create a commit.
pub(crate) fn begin_local_commit<R: GithubPublicationGitRunnerV1>(
    workspace: &Path,
    journal: &mut GithubPublicationJournalV1,
    token: &str,
    review: &PublicationReviewAuthorityV1,
    runner: &R,
    now_unix: u64,
) -> Result<GithubPublicationPermitV1, PublicationExecutorErrorV1> {
    let (prepared, permit) = journal
        .prepared(token)
        .map_err(|_| PublicationExecutorErrorV1::Token)?;
    if journal
        .state_for(&permit)
        .map_err(|_| PublicationExecutorErrorV1::RecoveryRequired)?
        != GithubPublicationRecordStateV1::Prepared
    {
        return Err(PublicationExecutorErrorV1::RecoveryRequired);
    }
    if prepared.expires_at_unix <= now_unix {
        return Err(PublicationExecutorErrorV1::Expired);
    }
    if prepared.review_record_id != review.record_id || prepared.review_digest != review.digest {
        return Err(PublicationExecutorErrorV1::ReviewAuthority);
    }
    let git = runner
        .identity()
        .map_err(|_| PublicationExecutorErrorV1::Repository)?;
    if git.fingerprint != prepared.git_executable_fingerprint
        || git.slot != prepared.git_selected_slot
        || git.canonical.to_string_lossy() != prepared.git_selected_identity
    {
        return Err(PublicationExecutorErrorV1::EvidenceDrift);
    }
    let descriptor = GithubPublicationDescriptorV1 {
        manifest_path: prepared.manifest_path.clone(),
        manifest_sha256: prepared.manifest_sha256.clone(),
        include_count: prepared.manifest_include_count,
        archival_exclusion_count: prepared.archival_exclusion_count,
        runtime_generated_exclusion_count: prepared.runtime_generated_exclusion_count,
        supplemental_paths: prepared.supplemental_paths.clone(),
    };
    // A PREPARE-authorized staged subset remains permissible here.  Each
    // approved literal is added below, after which the final staged set must
    // equal the whole approved set exactly.
    let probe = probe(workspace, runner, &git, &descriptor, true)?;
    if probe.repository != prepared.repository || probe.fingerprint != prepared.evidence_fingerprint
    {
        return Err(PublicationExecutorErrorV1::EvidenceDrift);
    }
    for path in &probe.paths {
        run_ok(
            &git,
            runner,
            &["add", "--", path],
            PublicationExecutorErrorV1::Commit,
        )?;
    }
    let staged = read_lines(runner, &git, &["diff", "--cached", "--name-only"])?;
    if staged != probe.paths {
        return Err(PublicationExecutorErrorV1::StagedState);
    }
    let tree = read_one(runner, &git, &["write-tree"])?;
    if !full_object_id(&tree) || !full_object_id(&prepared.repository.head_commit) {
        return Err(PublicationExecutorErrorV1::Commit);
    }
    let intent = GithubPublicationCommitIntentV1 {
        parent_commit: prepared.repository.head_commit.clone(),
        tree,
        paths: probe.paths,
        message: publication_commit_message(token),
        accepted_at_unix: now_unix,
    };
    journal
        .begin_local_commit(token, intent)
        .map_err(|_| PublicationExecutorErrorV1::RecoveryRequired)?;
    Ok(permit)
}

pub(crate) fn execute_local_commit<R: GithubPublicationGitRunnerV1>(
    journal: &mut GithubPublicationJournalV1,
    token: &str,
    runner: &R,
) -> Result<(), PublicationExecutorErrorV1> {
    journal
        .validate()
        .map_err(|_| PublicationExecutorErrorV1::RecoveryRequired)?;
    let (prepared, permit) = journal
        .prepared(token)
        .map_err(|_| PublicationExecutorErrorV1::Token)?;
    if journal
        .state_for(&permit)
        .map_err(|_| PublicationExecutorErrorV1::RecoveryRequired)?
        != GithubPublicationRecordStateV1::LocalCommitOutcomeUnknown
    {
        return Err(PublicationExecutorErrorV1::RecoveryRequired);
    }
    let intent = prepared
        .local_commit_intent
        .as_ref()
        .ok_or(PublicationExecutorErrorV1::RecoveryRequired)?;
    let git = bound_git(prepared, runner)?;
    require_repository(prepared, runner, &git)?;
    if read_one(runner, &git, &["rev-parse", "HEAD"])? != intent.parent_commit
        || read_one(runner, &git, &["write-tree"])? != intent.tree
    {
        return Err(PublicationExecutorErrorV1::EvidenceDrift);
    }
    run_ok(
        &git,
        runner,
        &["commit", "--no-gpg-sign", "-m", &intent.message],
        PublicationExecutorErrorV1::Commit,
    )?;
    let head = read_one(runner, &git, &["rev-parse", "HEAD"])?;
    let receipt = verify_local_commit(prepared, &head, runner, &git)?;
    journal
        .record_local_commit(token, receipt)
        .map_err(|_| PublicationExecutorErrorV1::RecoveryRequired)
}

/// The caller persists the exact push SHA and dispatch phase before push.
pub(crate) fn begin_push<R: GithubPublicationGitRunnerV1>(
    journal: &mut GithubPublicationJournalV1,
    token: &str,
    runner: &R,
) -> Result<(), PublicationExecutorErrorV1> {
    journal
        .validate()
        .map_err(|_| PublicationExecutorErrorV1::RecoveryRequired)?;
    let (prepared, _) = journal
        .prepared(token)
        .map_err(|_| PublicationExecutorErrorV1::Token)?;
    let git = bound_git(prepared, runner)?;
    require_repository(prepared, runner, &git)?;
    let receipt = prepared
        .local_commit_receipt
        .as_ref()
        .ok_or(PublicationExecutorErrorV1::RecoveryRequired)?;
    verify_local_commit(prepared, &receipt.commit, runner, &git)?;
    journal
        .begin_exact_push(token)
        .map_err(|_| PublicationExecutorErrorV1::RecoveryRequired)
}

pub(crate) fn push<R: GithubPublicationGitRunnerV1>(
    prepared: &GithubPublicationPreparedV1,
    runner: &R,
) -> Result<(), PublicationExecutorErrorV1> {
    let git = bound_git(prepared, runner)?;
    require_repository(prepared, runner, &git)?;
    let expected = prepared
        .push_expected_commit
        .as_ref()
        .filter(|expected| {
            prepared.local_commit_receipt.as_ref().map(|r| &r.commit) == Some(*expected)
        })
        .ok_or(PublicationExecutorErrorV1::RecoveryRequired)?;
    verify_local_commit(prepared, expected, runner, &git)?;
    let reference = format!(
        "{expected}:refs/heads/{}",
        prepared.repository.current_branch
    );
    run_ok(
        &git,
        runner,
        &["push", "origin", &reference],
        PublicationExecutorErrorV1::Dispatch,
    )
}

pub(crate) fn reconcile<R: GithubPublicationGitRunnerV1>(
    journal: &mut GithubPublicationJournalV1,
    token: &str,
    runner: &R,
) -> Result<&'static str, PublicationExecutorErrorV1> {
    journal
        .validate()
        .map_err(|_| PublicationExecutorErrorV1::RecoveryRequired)?;
    let (prepared, permit) = journal
        .prepared(token)
        .map_err(|_| PublicationExecutorErrorV1::Token)?;
    let git = bound_git(prepared, runner)?;
    require_repository(prepared, runner, &git)?;
    let state = journal
        .state_for(&permit)
        .map_err(|_| PublicationExecutorErrorV1::RecoveryRequired)?;
    if state == GithubPublicationRecordStateV1::LocalCommitOutcomeUnknown {
        let head = read_one(runner, &git, &["rev-parse", "HEAD"])?;
        let receipt = verify_local_commit(prepared, &head, runner, &git)?;
        journal
            .record_local_commit(token, receipt)
            .map_err(|_| PublicationExecutorErrorV1::RecoveryRequired)?;
        return Ok("LOCAL_COMMIT_CONFIRMED");
    }
    if state == GithubPublicationRecordStateV1::LocalCommitConfirmed {
        let receipt = prepared
            .local_commit_receipt
            .as_ref()
            .ok_or(PublicationExecutorErrorV1::RecoveryRequired)?;
        verify_local_commit(prepared, &receipt.commit, runner, &git)?;
        return Ok("LOCAL_COMMIT_CONFIRMED");
    }
    if !matches!(
        state,
        GithubPublicationRecordStateV1::RemoteOutcomeUnknown
            | GithubPublicationRecordStateV1::Confirmed
            | GithubPublicationRecordStateV1::ReconciledNotApplied
    ) {
        return Err(PublicationExecutorErrorV1::RecoveryRequired);
    }
    let expected = prepared
        .push_expected_commit
        .as_ref()
        .filter(|expected| {
            prepared.local_commit_receipt.as_ref().map(|r| &r.commit) == Some(*expected)
        })
        .ok_or(PublicationExecutorErrorV1::RecoveryRequired)?;
    verify_local_commit(prepared, expected, runner, &git)?;
    let remote = read_remote_head(runner, &git, &prepared.repository.current_branch);
    match remote {
        Ok(value) if value == *expected => {
            if state == GithubPublicationRecordStateV1::RemoteOutcomeUnknown {
                journal
                    .mark_remote_confirmed(&permit)
                    .map_err(|_| PublicationExecutorErrorV1::RecoveryRequired)?;
            } else if state != GithubPublicationRecordStateV1::Confirmed {
                return Err(PublicationExecutorErrorV1::Reconciliation);
            }
            Ok("CONFIRMED")
        }
        Ok(value)
            if value.is_empty()
                && state == GithubPublicationRecordStateV1::RemoteOutcomeUnknown =>
        {
            journal
                .mark_remote_not_applied(&permit)
                .map_err(|_| PublicationExecutorErrorV1::RecoveryRequired)?;
            Ok("RECONCILED_NOT_APPLIED")
        }
        _ => Err(PublicationExecutorErrorV1::Reconciliation),
    }
}

fn bound_git<R: GithubPublicationGitRunnerV1>(
    prepared: &GithubPublicationPreparedV1,
    runner: &R,
) -> Result<TrustedGitIdentityV1, PublicationExecutorErrorV1> {
    let git = runner
        .identity()
        .map_err(|_| PublicationExecutorErrorV1::Repository)?;
    if git.fingerprint != prepared.git_executable_fingerprint
        || git.slot != prepared.git_selected_slot
        || git.canonical.to_string_lossy() != prepared.git_selected_identity
    {
        return Err(PublicationExecutorErrorV1::EvidenceDrift);
    }
    Ok(git)
}

fn require_repository<R: GithubPublicationGitRunnerV1>(
    prepared: &GithubPublicationPreparedV1,
    runner: &R,
    git: &TrustedGitIdentityV1,
) -> Result<(), PublicationExecutorErrorV1> {
    if read_one(runner, git, &["config", "--get", "remote.origin.url"])?
        != prepared.repository.origin
        || read_one(runner, git, &["branch", "--show-current"])?
            != prepared.repository.current_branch
    {
        return Err(PublicationExecutorErrorV1::EvidenceDrift);
    }
    Ok(())
}

fn verify_local_commit<R: GithubPublicationGitRunnerV1>(
    prepared: &GithubPublicationPreparedV1,
    commit: &str,
    runner: &R,
    git: &TrustedGitIdentityV1,
) -> Result<GithubPublicationCommitReceiptV1, PublicationExecutorErrorV1> {
    let intent = prepared
        .local_commit_intent
        .as_ref()
        .ok_or(PublicationExecutorErrorV1::RecoveryRequired)?;
    if !full_object_id(commit) || commit == intent.parent_commit {
        return Err(PublicationExecutorErrorV1::Reconciliation);
    }
    let output = runner
        .run(
            git,
            &[
                "--no-replace-objects".into(),
                "cat-file".into(),
                "commit".into(),
                commit.into(),
            ],
        )
        .map_err(|_| PublicationExecutorErrorV1::Reconciliation)?;
    if !output.success || output.stdout.len() > MAX_GIT_OUTPUT_BYTES {
        return Err(PublicationExecutorErrorV1::Reconciliation);
    }
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| PublicationExecutorErrorV1::Reconciliation)?;
    let (headers, message) = text
        .split_once("\n\n")
        .ok_or(PublicationExecutorErrorV1::Reconciliation)?;
    let trees: Vec<_> = headers
        .lines()
        .filter_map(|line| line.strip_prefix("tree "))
        .collect();
    let parents: Vec<_> = headers
        .lines()
        .filter_map(|line| line.strip_prefix("parent "))
        .collect();
    if trees != [intent.tree.as_str()]
        || parents != [intent.parent_commit.as_str()]
        || message != intent.message
    {
        return Err(PublicationExecutorErrorV1::Reconciliation);
    }
    let paths = read_lines(
        runner,
        git,
        &[
            "--no-replace-objects",
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "--no-renames",
            "-r",
            &intent.parent_commit,
            commit,
            "--",
        ],
    )?;
    if paths != intent.paths {
        return Err(PublicationExecutorErrorV1::Reconciliation);
    }
    Ok(GithubPublicationCommitReceiptV1 {
        commit: commit.into(),
        parent_commit: intent.parent_commit.clone(),
        tree: intent.tree.clone(),
        message: message.into(),
    })
}

fn full_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn probe<R: GithubPublicationGitRunnerV1>(
    workspace: &Path,
    runner: &R,
    git: &TrustedGitIdentityV1,
    descriptor: &GithubPublicationDescriptorV1,
    allow_approved_pre_staged: bool,
) -> Result<ProbeV1, PublicationExecutorErrorV1> {
    let manifest = fs::read(workspace.join(&descriptor.manifest_path))
        .map_err(|_| PublicationExecutorErrorV1::Manifest)?;
    let manifest_sha = sha256(&manifest);
    if manifest_sha != descriptor.manifest_sha256 {
        return Err(PublicationExecutorErrorV1::Manifest);
    }
    let parsed = parse_t0407_reconstruction_manifest(&manifest).map_err(map_manifest_error)?;
    if parsed.includes.len() != descriptor.include_count
        || parsed.archival_binary_exclusions != descriptor.archival_exclusion_count
        || parsed.runtime_generated_exclusions != descriptor.runtime_generated_exclusion_count
    {
        return Err(PublicationExecutorErrorV1::Manifest);
    }
    let expected_path_count = parsed
        .includes
        .len()
        .checked_add(1)
        .and_then(|count| count.checked_add(descriptor.supplemental_paths.len()))
        .ok_or(PublicationExecutorErrorV1::Manifest)?;
    let mut paths = parsed
        .includes
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    paths.push(descriptor.manifest_path.clone());
    paths.extend(descriptor.supplemental_paths.clone());
    paths.sort();
    paths.dedup();
    if paths.len() != expected_path_count {
        return Err(PublicationExecutorErrorV1::Manifest);
    }
    for entry in &parsed.includes {
        validate_manifest_entry(workspace, entry)?;
    }
    for path in &paths {
        let candidate = workspace.join(path);
        let canonical = candidate
            .canonicalize()
            .map_err(|_| PublicationExecutorErrorV1::Manifest)?;
        if !canonical.starts_with(workspace) || !canonical.is_file() {
            return Err(PublicationExecutorErrorV1::Manifest);
        }
    }
    let origin = read_one(runner, git, &["config", "--get", "remote.origin.url"])?;
    let branch = read_one(runner, git, &["branch", "--show-current"])?;
    let head = read_one(runner, git, &["rev-parse", "HEAD"])?;
    if origin.is_empty() || branch.is_empty() || !valid_commit(&head) {
        return Err(PublicationExecutorErrorV1::Repository);
    }
    let staged = read_lines(runner, git, &["diff", "--cached", "--name-only"])?;
    if (!allow_approved_pre_staged && !staged.is_empty())
        || staged.iter().any(|path| !paths.contains(path))
    {
        return Err(PublicationExecutorErrorV1::StagedState);
    }
    let fingerprint = sha256(
        format!(
            "{origin}\n{branch}\n{head}\n{}\n{}",
            review_paths_digest(workspace, &paths)?,
            descriptor.manifest_sha256
        )
        .as_bytes(),
    );
    Ok(ProbeV1 {
        repository: GithubRepositoryEvidenceV1 {
            origin,
            current_branch: branch,
            head_commit: head,
        },
        paths,
        fingerprint,
    })
}

fn validate_manifest_entry(
    workspace: &Path,
    entry: &super::github_publication::FrozenPublicationManifestEntryV1,
) -> Result<(), PublicationExecutorErrorV1> {
    let bytes =
        fs::read(workspace.join(&entry.path)).map_err(|_| PublicationExecutorErrorV1::Manifest)?;
    if bytes.len() as u64 != entry.byte_length || sha256(&bytes) != entry.sha256 {
        return Err(PublicationExecutorErrorV1::EvidenceDrift);
    }
    Ok(())
}

fn review_paths_digest(
    workspace: &Path,
    paths: &[String],
) -> Result<String, PublicationExecutorErrorV1> {
    let mut hasher = Sha256::new();
    for path in paths {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(
            fs::read(workspace.join(path)).map_err(|_| PublicationExecutorErrorV1::Manifest)?,
        );
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn read_one<R: GithubPublicationGitRunnerV1>(
    runner: &R,
    git: &TrustedGitIdentityV1,
    args: &[&str],
) -> Result<String, PublicationExecutorErrorV1> {
    let output = runner
        .run(
            git,
            &args
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
        .map_err(|_| PublicationExecutorErrorV1::Repository)?;
    if !output.success || output.stdout.len() > MAX_GIT_OUTPUT_BYTES {
        return Err(PublicationExecutorErrorV1::Repository);
    }
    let value = std::str::from_utf8(&output.stdout)
        .map_err(|_| PublicationExecutorErrorV1::Repository)?
        .trim();
    if value.len() > 1024 || value.contains('\r') || value.contains('\n') {
        return Err(PublicationExecutorErrorV1::Repository);
    }
    Ok(value.into())
}

fn read_lines<R: GithubPublicationGitRunnerV1>(
    runner: &R,
    git: &TrustedGitIdentityV1,
    args: &[&str],
) -> Result<Vec<String>, PublicationExecutorErrorV1> {
    let output = runner
        .run(
            git,
            &args
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
        .map_err(|_| PublicationExecutorErrorV1::StagedState)?;
    if !output.success || output.stdout.len() > MAX_GIT_OUTPUT_BYTES {
        return Err(PublicationExecutorErrorV1::StagedState);
    }
    let text =
        std::str::from_utf8(&output.stdout).map_err(|_| PublicationExecutorErrorV1::StagedState)?;
    let mut lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
    if lines
        .iter()
        .any(|line| line.is_empty() || line.len() > 512 || line.contains('\\'))
    {
        return Err(PublicationExecutorErrorV1::StagedState);
    }
    lines.sort();
    lines.dedup();
    Ok(lines)
}

fn read_remote_head<R: GithubPublicationGitRunnerV1>(
    runner: &R,
    git: &TrustedGitIdentityV1,
    branch: &str,
) -> Result<String, PublicationExecutorErrorV1> {
    let output = runner
        .run(
            git,
            &[
                "ls-remote".into(),
                "--heads".into(),
                "origin".into(),
                branch.into(),
            ],
        )
        .map_err(|_| PublicationExecutorErrorV1::Reconciliation)?;
    if !output.success || output.stdout.len() > MAX_GIT_OUTPUT_BYTES {
        return Err(PublicationExecutorErrorV1::Reconciliation);
    }
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| PublicationExecutorErrorV1::Reconciliation)?
        .trim();
    if text.is_empty() {
        return Ok(String::new());
    }
    let (head, reference) = text
        .split_once('\t')
        .ok_or(PublicationExecutorErrorV1::Reconciliation)?;
    if reference != format!("refs/heads/{branch}") || !valid_commit(head) {
        return Err(PublicationExecutorErrorV1::Reconciliation);
    }
    Ok(head.into())
}

fn run_ok<R: GithubPublicationGitRunnerV1>(
    git: &TrustedGitIdentityV1,
    runner: &R,
    args: &[&str],
    error: PublicationExecutorErrorV1,
) -> Result<(), PublicationExecutorErrorV1> {
    let output = runner
        .run(
            git,
            &args
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
        .map_err(|_| error)?;
    if output.success && output.stdout.len() <= MAX_GIT_OUTPUT_BYTES {
        Ok(())
    } else {
        Err(error)
    }
}

fn valid_review(review: &PublicationReviewAuthorityV1) -> bool {
    !review.record_id.is_empty()
        && review.record_id.len() <= 128
        && review.digest.len() == 64
        && review.digest.bytes().all(|b| b.is_ascii_hexdigit())
}
fn valid_commit(value: &str) -> bool {
    (7..=64).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn confirmation_token(permit: &GithubPublicationPermitV1, fingerprint: &str, now: u64) -> String {
    format!(
        "gpub-{}",
        sha256(
            format!(
                "{}:{}:{fingerprint}:{now}",
                permit.idempotency_key(),
                permit.intent_hash()
            )
            .as_bytes()
        )
    )
}
fn map_manifest_error(_: FrozenPublicationManifestErrorV1) -> PublicationExecutorErrorV1 {
    PublicationExecutorErrorV1::Manifest
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::{BTreeMap, VecDeque},
        path::PathBuf,
        sync::Mutex,
    };

    struct Fake {
        calls: Mutex<Vec<Vec<String>>>,
        replies: Mutex<VecDeque<GitRunOutputV1>>,
    }
    impl Fake {
        fn new(replies: Vec<GitRunOutputV1>) -> Self {
            Self {
                calls: Mutex::new(vec![]),
                replies: Mutex::new(replies.into()),
            }
        }
    }
    impl GithubPublicationGitRunnerV1 for Fake {
        fn identity(&self) -> Result<TrustedGitIdentityV1, ()> {
            Ok(TrustedGitIdentityV1 {
                canonical: Path::new(r"C:\trusted\git.exe").into(),
                fingerprint: "b".repeat(64),
                slot: 0,
            })
        }
        fn run(
            &self,
            _identity: &TrustedGitIdentityV1,
            argv: &[String],
        ) -> Result<GitRunOutputV1, ()> {
            self.calls.lock().unwrap().push(argv.into());
            self.replies.lock().unwrap().pop_front().ok_or(())
        }
    }
    #[test]
    fn fixed_push_argv_has_no_force_flag() {
        let prepared = prepared_fixture("feature");
        let intent = prepared.local_commit_intent.as_ref().unwrap();
        let fake = Fake::new(vec![
            ok_bytes(format!("{}\n", prepared.repository.origin)),
            ok_bytes("feature\n"),
            ok_bytes(format!(
                "tree {}\nparent {}\n\n{}",
                intent.tree, intent.parent_commit, intent.message
            )),
            ok_bytes(format!("{}\n", intent.paths.join("\n"))),
            ok_bytes(""),
        ]);
        push(&prepared, &fake).expect("push");
        assert_eq!(
            fake.calls.lock().unwrap().last().unwrap(),
            &vec![
                "push".to_string(),
                "origin".into(),
                format!("{}:refs/heads/feature", "1".repeat(40))
            ]
        );
    }
    #[test]
    fn confirmation_token_is_bounded_and_redacted() {
        let permit = GithubPublicationPermitV1::for_test("approval-1", "fnv1a64:0123456789abcdef");
        let token = confirmation_token(&permit, &"a".repeat(64), 7);
        assert_eq!(token.len(), 69);
        assert!(token.starts_with("gpub-"));
        assert!(!token.contains("origin"));
    }
    #[test]
    fn remote_ref_parser_accepts_only_the_exact_feature_branch() {
        let fake = Fake::new(vec![GitRunOutputV1 {
            success: true,
            stdout: b"0123456789abcdef\trefs/heads/feature\n".to_vec(),
        }]);
        assert_eq!(
            read_remote_head(&fake, &fake.identity().unwrap(), "feature").expect("remote ref"),
            "0123456789abcdef"
        );
        let wrong = Fake::new(vec![GitRunOutputV1 {
            success: true,
            stdout: b"0123456789abcdef\trefs/heads/main\n".to_vec(),
        }]);
        assert_eq!(
            read_remote_head(&wrong, &wrong.identity().unwrap(), "feature")
                .expect_err("wrong branch"),
            PublicationExecutorErrorV1::Reconciliation
        );
    }

    #[test]
    fn one_byte_manifest_entry_drift_is_refused_before_staging() {
        let (workspace, descriptor) = fixture_workspace();
        let manifest = fs::read(workspace.join(&descriptor.manifest_path)).expect("manifest");
        let mut entry = parse_t0407_reconstruction_manifest(&manifest)
            .expect("manifest")
            .includes
            .into_iter()
            .find(|entry| entry.path == "fixture/approved.txt")
            .expect("fixture included");
        entry.byte_length = entry.byte_length.saturating_add(1);
        assert_eq!(
            validate_manifest_entry(&workspace, &entry).expect_err("one byte drift"),
            PublicationExecutorErrorV1::EvidenceDrift
        );
    }

    #[test]
    fn manifest_entries_cannot_overlap_reviewed_supplemental_paths() {
        let (workspace, mut descriptor) = fixture_workspace();
        let manifest_path = workspace.join(&descriptor.manifest_path);
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).expect("manifest bytes"))
                .expect("manifest json");
        let descriptor_path = super::super::github_publication::T0407_R12_DESCRIPTOR_PATH;
        let descriptor_bytes =
            fs::read(workspace.join(descriptor_path)).expect("descriptor supplemental fixture");
        manifest["entries"]
            .as_array_mut()
            .expect("manifest entries")
            .push(serde_json::json!({
                "path": descriptor_path,
                "sha256": sha256(&descriptor_bytes),
                "byteLength": descriptor_bytes.len(),
                "classification": "INCLUDE_RECONSTRUCTION",
                "rationale": "overlap must be rejected"
            }));
        let bytes = serde_json::to_vec(&manifest).expect("updated manifest");
        fs::write(&manifest_path, &bytes).expect("updated manifest write");
        descriptor.manifest_sha256 = sha256(&bytes);
        descriptor.include_count += 1;

        let runner = Fake::new(vec![]);
        assert_eq!(
            probe(
                &workspace,
                &runner,
                &runner.identity().expect("identity"),
                &descriptor,
                true
            )
            .expect_err("manifest/supplemental overlap must fail closed"),
            PublicationExecutorErrorV1::Manifest
        );
        assert!(
            runner.calls.lock().expect("calls").is_empty(),
            "overlap must be rejected before any Git observation or mutation"
        );
    }

    #[test]
    fn approved_pre_staged_subset_is_observed_but_unrelated_path_is_refused() {
        let (workspace, descriptor) = fixture_workspace();
        for (staged, expected) in [
            (b"fixture/approved.txt\n".to_vec(), Ok(())),
            (
                b"unrelated-local.txt\n".to_vec(),
                Err(PublicationExecutorErrorV1::StagedState),
            ),
        ] {
            let runner = Fake::new(vec![
                GitRunOutputV1 {
                    success: true,
                    stdout: b"https://github.example.invalid/catdesk.git\n".to_vec(),
                },
                GitRunOutputV1 {
                    success: true,
                    stdout: b"feature-publication\n".to_vec(),
                },
                GitRunOutputV1 {
                    success: true,
                    stdout: b"abcdef0123456789\n".to_vec(),
                },
                GitRunOutputV1 {
                    success: true,
                    stdout: staged,
                },
            ]);
            match expected {
                Ok(()) => assert!(
                    probe(
                        &workspace,
                        &runner,
                        &runner.identity().expect("identity"),
                        &descriptor,
                        true
                    )
                    .is_ok()
                ),
                Err(error) => assert_eq!(
                    probe(
                        &workspace,
                        &runner,
                        &runner.identity().expect("identity"),
                        &descriptor,
                        true
                    )
                    .expect_err("unrelated staged path"),
                    error
                ),
            }
        }
    }

    #[test]
    fn trusted_git_identity_drift_refuses_push() {
        let fake = Fake::new(vec![]);
        let mut prepared = prepared_fixture("feature");
        prepared.git_executable_fingerprint = "a".repeat(64);
        assert_eq!(
            push(&prepared, &fake).expect_err("fingerprint drift"),
            PublicationExecutorErrorV1::EvidenceDrift
        );
        assert!(fake.calls.lock().expect("calls").is_empty());
    }

    struct LocalGitRunner {
        workspace: PathBuf,
        git: TrustedGitIdentityV1,
    }

    impl GithubPublicationGitRunnerV1 for LocalGitRunner {
        fn identity(&self) -> Result<TrustedGitIdentityV1, ()> {
            Ok(self.git.clone())
        }

        fn run(
            &self,
            identity: &TrustedGitIdentityV1,
            argv: &[String],
        ) -> Result<GitRunOutputV1, ()> {
            assert!(
                !argv.iter().any(|arg| arg == "push"),
                "fixture must never push"
            );
            let mut command = Command::new(&identity.canonical);
            command
                .args(argv)
                .current_dir(&self.workspace)
                .env_clear()
                .env("HOME", &self.workspace)
                .env("USERPROFILE", &self.workspace)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env(
                    "GIT_CONFIG_GLOBAL",
                    self.workspace.join(".fixture-no-global-config"),
                )
                .env("GIT_ALLOW_PROTOCOL", "file")
                .env("GIT_TERMINAL_PROMPT", "0");
            if let Some(system_root) = std::env::var_os("SystemRoot") {
                command.env("SystemRoot", system_root);
            }
            let output = command
                .stdin(std::process::Stdio::null())
                .output()
                .map_err(|_| ())?;
            if !output.status.success() {
                eprintln!(
                    "owned fixture Git {argv:?}: {}",
                    String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(2048)])
                );
            }
            Ok(GitRunOutputV1 {
                success: output.status.success(),
                stdout: output.stdout,
            })
        }
    }

    struct RealPublication {
        workspace: PathBuf,
        remote: PathBuf,
        runner: LocalGitRunner,
        review: PublicationReviewAuthorityV1,
        journal: GithubPublicationJournalV1,
        token: String,
    }

    impl RealPublication {
        fn new() -> Self {
            let (workspace, descriptor) = fixture_workspace();
            let runner = LocalGitRunner {
                git: resolve_trusted_git_identity(&workspace).expect("trusted fixture Git"),
                workspace: workspace.clone(),
            };
            let git = runner.identity().expect("trusted Git");
            let command = |args: &[&str]| {
                run_ok(&git, &runner, args, PublicationExecutorErrorV1::Repository)
                    .expect("fixture Git");
            };
            command(&["init", "-b", "feature-publication"]);
            command(&["config", "user.name", "CatDesk fixture"]);
            command(&["config", "user.email", "fixture@example.invalid"]);
            command(&["config", "core.autocrlf", "false"]);
            command(&["config", "core.hooksPath", ".fixture-empty-hooks"]);
            fs::write(workspace.join("base.txt"), b"base\n").unwrap();
            command(&["add", "--", "base.txt"]);
            command(&["commit", "--no-gpg-sign", "-m", "fixture base"]);
            let remote = workspace.join(".fixture-remote.git");
            command(&["clone", "--bare", ".", ".fixture-remote.git"]);
            command(&["remote", "add", "origin", &fixture_git_path(&remote)]);
            let observed = probe(&workspace, &runner, &git, &descriptor, true).unwrap();
            let permit =
                GithubPublicationPermitV1::for_test("approval-1", "fnv1a64:0123456789abcdef");
            let token = confirmation_token(&permit, &observed.fingerprint, 10);
            let mut prepared = prepared_fixture("feature-publication");
            prepared.manifest_path = descriptor.manifest_path.clone();
            prepared.manifest_sha256 = descriptor.manifest_sha256.clone();
            prepared.supplemental_paths = descriptor.supplemental_paths.clone();
            prepared.repository = observed.repository;
            prepared.evidence_fingerprint = observed.fingerprint;
            prepared.git_executable_fingerprint = git.fingerprint;
            prepared.git_selected_slot = git.slot;
            prepared.git_selected_identity = git.canonical.to_string_lossy().into_owned();
            prepared.local_commit_intent = None;
            prepared.local_commit_receipt = None;
            prepared.push_expected_commit = None;
            let mut journal = GithubPublicationJournalV1 {
                schema_version: 1,
                records: BTreeMap::from([(
                    "approval-1".into(),
                    super::super::github_publication::GithubPublicationRecordV1 {
                        schema_version: 1,
                        intent_hash: permit.intent_hash().into(),
                        state: GithubPublicationRecordStateV1::Prepared,
                    },
                )]),
                prepared: BTreeMap::new(),
            };
            journal
                .bind_prepared(token.clone(), &permit, prepared)
                .unwrap();
            Self {
                workspace,
                remote,
                runner,
                journal,
                token,
                review: PublicationReviewAuthorityV1 {
                    record_id: "review-1".into(),
                    digest: "a".repeat(64),
                    descriptor,
                },
            }
        }

        fn git(&self, args: &[&str]) -> String {
            let identity = self.runner.identity().unwrap();
            let output = self
                .runner
                .run(
                    &identity,
                    &args.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
                )
                .unwrap();
            assert!(output.success, "fixture command failed: {args:?}");
            String::from_utf8(output.stdout).unwrap().trim().into()
        }

        fn persist(&self) {
            fs::write(
                self.workspace.join(".fixture-journal.json"),
                serde_json::to_vec(&self.journal).unwrap(),
            )
            .unwrap();
        }

        fn reload(&mut self) {
            self.journal = serde_json::from_slice(
                &fs::read(self.workspace.join(".fixture-journal.json")).unwrap(),
            )
            .unwrap();
        }

        fn begin(&mut self) {
            begin_local_commit(
                &self.workspace,
                &mut self.journal,
                &self.token,
                &self.review,
                &self.runner,
                11,
            )
            .unwrap();
            self.persist();
        }

        fn commit(&mut self) -> String {
            execute_local_commit(&mut self.journal, &self.token, &self.runner).unwrap();
            self.git(&["rev-parse", "HEAD"])
        }

        fn set_remote(&self, commit: &str) {
            let remote = fixture_git_path(&self.remote);
            let workspace = fixture_git_path(&self.workspace);
            self.git(&[
                "--git-dir",
                &remote,
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                &workspace,
                commit,
            ]);
            self.git(&[
                "--git-dir",
                &remote,
                "update-ref",
                "refs/heads/feature-publication",
                commit,
            ]);
        }

        fn state(&self) -> GithubPublicationRecordStateV1 {
            self.journal.records["approval-1"].state.clone()
        }
    }

    impl Drop for RealPublication {
        fn drop(&mut self) {
            let temp = std::env::temp_dir().canonicalize().unwrap();
            assert!(
                self.workspace.starts_with(temp)
                    && self
                        .workspace
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with("catdesk-github-publication-executor-fixture-")
            );
            #[cfg(windows)]
            clear_fixture_readonly(&self.workspace);
            fs::remove_dir_all(&self.workspace).expect("owned publication fixture cleanup");
        }
    }

    fn fixture_git_path(path: &Path) -> String {
        let path = path.to_str().expect("UTF-8 owned fixture path");
        path.strip_prefix(r"\\?\").unwrap_or(path).to_owned()
    }

    #[cfg(windows)]
    fn clear_fixture_readonly(directory: &Path) {
        use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn SetFileAttributesW(path: *const u16, attributes: u32) -> i32;
        }
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            assert_eq!(
                metadata.file_attributes() & 0x400,
                0,
                "owned fixture reparse"
            );
            if metadata.is_dir() {
                clear_fixture_readonly(&path);
            } else if metadata.file_attributes() & 1 != 0 {
                let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                assert_ne!(
                    unsafe { SetFileAttributesW(wide.as_ptr(), metadata.file_attributes() & !1) },
                    0
                );
            }
        }
    }

    #[test]
    fn publication_precommit_crash_keeps_intent_and_never_replays_commit() {
        let mut f = RealPublication::new();
        let original = f.git(&["rev-parse", "HEAD"]);
        f.begin();
        f.reload();
        assert_eq!(
            f.state(),
            GithubPublicationRecordStateV1::LocalCommitOutcomeUnknown
        );
        assert!(reconcile(&mut f.journal, &f.token, &f.runner).is_err());
        assert!(
            begin_local_commit(
                &f.workspace,
                &mut f.journal,
                &f.token,
                &f.review,
                &f.runner,
                12
            )
            .is_err()
        );
        assert_eq!(f.git(&["rev-parse", "HEAD"]), original);
        assert!(
            f.journal
                .prepared(&f.token)
                .unwrap()
                .0
                .local_commit_receipt
                .is_none()
        );
    }

    #[test]
    fn publication_postcommit_crash_recovers_only_the_exact_commit_receipt() {
        let mut f = RealPublication::new();
        f.begin();
        let committed = f.commit();
        f.reload(); // Drop the unpersisted receipt to reproduce the crash window.
        assert_eq!(
            reconcile(&mut f.journal, &f.token, &f.runner).unwrap(),
            "LOCAL_COMMIT_CONFIRMED"
        );
        assert_eq!(
            f.journal
                .prepared(&f.token)
                .unwrap()
                .0
                .local_commit_receipt
                .as_ref()
                .unwrap()
                .commit,
            committed
        );
        f.persist();
        f.reload();
        begin_push(&mut f.journal, &f.token, &f.runner).unwrap();
        assert_eq!(
            f.state(),
            GithubPublicationRecordStateV1::RemoteOutcomeUnknown
        );
        assert_eq!(
            f.journal
                .prepared(&f.token)
                .unwrap()
                .0
                .push_expected_commit
                .as_deref(),
            Some(committed.as_str())
        );
    }

    #[test]
    fn publication_reconcile_rejects_wrong_tree_parent_message_and_path_scope() {
        for mismatch in ["tree", "parent", "message", "paths"] {
            let mut f = RealPublication::new();
            f.begin();
            let message = f
                .journal
                .prepared(&f.token)
                .unwrap()
                .0
                .local_commit_intent
                .as_ref()
                .unwrap()
                .message
                .clone();
            if mismatch == "tree" {
                fs::write(
                    f.workspace.join("fixture/approved.txt"),
                    b"unreviewed change",
                )
                .unwrap();
                f.git(&["add", "--", "fixture/approved.txt"]);
            }
            if mismatch == "parent" {
                f.git(&["commit", "--no-gpg-sign", "-m", "unrelated parent"]);
            }
            let actual_message = if mismatch == "message" {
                "unrelated message"
            } else {
                &message
            };
            f.git(&[
                "commit",
                "--no-gpg-sign",
                "--allow-empty",
                "-m",
                actual_message,
            ]);
            if mismatch == "paths" {
                let paths = &mut f
                    .journal
                    .prepared
                    .get_mut(&f.token)
                    .unwrap()
                    .local_commit_intent
                    .as_mut()
                    .unwrap()
                    .paths;
                let index = paths
                    .iter()
                    .position(|p| p == "fixture/approved.txt")
                    .unwrap();
                paths[index] = "fixture/wrong.txt".into();
            }
            assert!(
                reconcile(&mut f.journal, &f.token, &f.runner).is_err(),
                "{mismatch}"
            );
            assert_eq!(
                f.state(),
                GithubPublicationRecordStateV1::LocalCommitOutcomeUnknown
            );
        }
    }

    #[test]
    fn publication_remote_reconciliation_uses_saved_commit_independently_of_current_head() {
        let mut f = RealPublication::new();
        f.begin();
        let committed = f.commit();
        f.persist();
        begin_push(&mut f.journal, &f.token, &f.runner).unwrap();
        f.persist();
        f.git(&[
            "commit",
            "--no-gpg-sign",
            "--allow-empty",
            "-m",
            "later unrelated commit",
        ]);
        let later = f.git(&["rev-parse", "HEAD"]);
        f.set_remote(&later);
        assert!(reconcile(&mut f.journal, &f.token, &f.runner).is_err());
        assert_eq!(
            f.state(),
            GithubPublicationRecordStateV1::RemoteOutcomeUnknown
        );
        f.set_remote(&committed);
        assert_eq!(
            reconcile(&mut f.journal, &f.token, &f.runner).unwrap(),
            "CONFIRMED"
        );
        assert_eq!(f.git(&["rev-parse", "HEAD"]), later);
    }

    #[test]
    fn publication_legacy_unknown_without_commit_proof_stays_held() {
        let mut f = RealPublication::new();
        let (_, permit) = f.journal.prepared(&f.token).unwrap();
        f.journal.mark_remote_dispatch_started(&permit).unwrap();
        f.persist();
        f.reload();
        let before = f.journal.clone();
        assert_eq!(
            reconcile(&mut f.journal, &f.token, &f.runner),
            Err(PublicationExecutorErrorV1::RecoveryRequired)
        );
        assert_eq!(f.journal, before);
        assert!(begin_push(&mut f.journal, &f.token, &f.runner).is_err());
    }

    #[test]
    fn publication_unrelated_staged_file_is_refused_before_commit_intent() {
        let mut f = RealPublication::new();
        fs::write(f.workspace.join("unrelated.txt"), b"unrelated").unwrap();
        f.git(&["add", "--", "unrelated.txt"]);
        let before = f.journal.clone();
        assert_eq!(
            begin_local_commit(
                &f.workspace,
                &mut f.journal,
                &f.token,
                &f.review,
                &f.runner,
                11
            ),
            Err(PublicationExecutorErrorV1::StagedState)
        );
        assert_eq!(f.journal, before);
    }

    fn fixture_workspace() -> (PathBuf, GithubPublicationDescriptorV1) {
        let workspace = std::env::temp_dir().join(format!(
            "catdesk-github-publication-executor-fixture-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(workspace.join("fixture")).expect("fixture directory");
        let approved = workspace.join("fixture/approved.txt");
        fs::write(&approved, b"fixture").expect("approved fixture");

        let path = super::super::github_publication::T0407_R12_MANIFEST_PATH;
        let manifest_path = workspace.join(path);
        fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
            .expect("manifest directory");
        let mut entries = vec![
            serde_json::json!({
                "path": "fixture/approved.txt",
                "sha256": sha256(b"fixture"),
                "byteLength": 7,
                "classification": "INCLUDE_RECONSTRUCTION",
                "rationale": "fixture include",
            }),
            serde_json::json!({
                "path": "src/daemon-reload-approval-request-v1.json",
                "sha256": "0".repeat(64),
                "byteLength": 0,
                "classification": "EXCLUDE_RUNTIME_GENERATED",
                "rationale": "fixture runtime exclusion",
            }),
        ];
        entries.extend((0..12).map(|index| {
            serde_json::json!({
                "path": format!("history/archive-{index}.zip"),
                "sha256": "0".repeat(64),
                "byteLength": 0,
                "classification": "EXCLUDE_ARCHIVAL_BINARY",
                "rationale": "fixture archival exclusion",
            })
        }));
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "entries": entries,
        });
        let bytes = serde_json::to_vec(&manifest).expect("fixture manifest");
        fs::write(&manifest_path, &bytes).expect("write manifest");

        for supplemental in [
            super::super::github_publication::T0407_R12_DESCRIPTOR_PATH,
            super::super::github_publication::T0407_R12_REVIEW_PATH,
        ] {
            let supplemental_path = workspace.join(supplemental);
            fs::create_dir_all(supplemental_path.parent().expect("supplemental parent"))
                .expect("supplemental directory");
            fs::write(supplemental_path, b"fixture supplemental").expect("supplemental");
        }

        let workspace = workspace
            .canonicalize()
            .expect("canonical fixture workspace");
        let parsed = parse_t0407_reconstruction_manifest(&bytes).expect("parse manifest");
        let descriptor = GithubPublicationDescriptorV1 {
            manifest_path: path.into(),
            manifest_sha256: sha256(&bytes),
            include_count: parsed.includes.len(),
            archival_exclusion_count: parsed.archival_binary_exclusions,
            runtime_generated_exclusion_count: parsed.runtime_generated_exclusions,
            supplemental_paths: vec![
                super::super::github_publication::T0407_R12_DESCRIPTOR_PATH.into(),
                super::super::github_publication::T0407_R12_REVIEW_PATH.into(),
            ],
        };
        (workspace, descriptor)
    }

    fn prepared_fixture(branch: &str) -> GithubPublicationPreparedV1 {
        let intent = GithubPublicationCommitIntentV1 {
            parent_commit: "0".repeat(40),
            tree: "2".repeat(40),
            paths: vec![
                "docs/a.md".into(),
                "docs/b.md".into(),
                super::super::github_publication::T0407_R12_MANIFEST_PATH.into(),
                "fixture/approved.txt".into(),
            ],
            message: publication_commit_message(&format!("gpub-{}", "0".repeat(64))),
            accepted_at_unix: 11,
        };
        GithubPublicationPreparedV1 {
            schema_version: 1,
            idempotency_key: "approval-1".into(),
            intent_hash: "fnv1a64:0123456789abcdef".into(),
            review_record_id: "review-1".into(),
            review_digest: "a".repeat(64),
            manifest_path: super::super::github_publication::T0407_R12_MANIFEST_PATH.into(),
            manifest_sha256: "b".repeat(64),
            manifest_include_count: 1,
            archival_exclusion_count: 12,
            runtime_generated_exclusion_count: 1,
            supplemental_paths: vec!["docs/a.md".into(), "docs/b.md".into()],
            evidence_fingerprint: "c".repeat(64),
            expires_at_unix: 20,
            repository: GithubRepositoryEvidenceV1 {
                origin: "https://example.invalid/repo.git".into(),
                current_branch: branch.into(),
                head_commit: intent.parent_commit.clone(),
            },
            git_executable_fingerprint: "b".repeat(64),
            git_selected_slot: 0,
            git_selected_identity: r"C:\trusted\git.exe".into(),
            local_commit_receipt: Some(GithubPublicationCommitReceiptV1 {
                commit: "1".repeat(40),
                parent_commit: intent.parent_commit.clone(),
                tree: intent.tree.clone(),
                message: intent.message.clone(),
            }),
            local_commit_intent: Some(intent),
            push_expected_commit: Some("1".repeat(40)),
        }
    }

    fn ok_bytes(value: impl Into<String>) -> GitRunOutputV1 {
        GitRunOutputV1 {
            success: true,
            stdout: value.into().into_bytes(),
        }
    }
}
