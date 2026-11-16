//! Contract-bound local verification and final-review artifacts.
//!
//! Commands are executed directly without a shell and only after the
//! autonomous policy accepts their exact argument vector. This module never
//! stages, commits, pushes, or changes the selected branch.

use std::process::Command;

use serde_json::json;

use super::autonomous_contract::{
    AutonomousCommandProfileV1, AutonomousGitPolicyV1, AutonomousPolicyEngineV1,
};
use super::autonomous_controller::AutonomousVerifierV1;
use super::coordinator::{VerificationStatusV1, VerificationSummaryV1};
use super::runtime::RuntimeError;

const MAX_CAPTURE_BYTES: usize = 24 * 1024;

pub struct ContractVerifierV1 {
    policy: AutonomousPolicyEngineV1,
    last_verification: Option<VerificationSummaryV1>,
}

impl ContractVerifierV1 {
    pub fn new(policy: AutonomousPolicyEngineV1) -> Result<Self, RuntimeError> {
        verify_git_identity(&policy)?;
        Ok(Self {
            policy,
            last_verification: None,
        })
    }

    fn execute_profile(
        &self,
        profile: &AutonomousCommandProfileV1,
    ) -> Result<String, RuntimeError> {
        let argv = profile.exact_argv();
        if argv.is_empty() {
            return Err(RuntimeError::Validation(
                "autonomous verification profile does not have an exact executable command".into(),
            ));
        }
        let owned = argv.iter().map(|part| part.to_string()).collect::<Vec<_>>();
        self.policy.permit_command(&owned).map_err(|_| {
            RuntimeError::Validation("verification command is outside the contract".into())
        })?;
        let output = Command::new(argv[0])
            .args(&argv[1..])
            .current_dir(&self.policy.contract().workspace)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|_| {
                RuntimeError::Provider("approved verification command could not be launched".into())
            })?;
        let text = bounded_output(&output.stdout, &output.stderr);
        if output.status.success() {
            Ok(text)
        } else {
            Err(RuntimeError::Provider(format!(
                "approved verification command failed: {}",
                bounded_failure_text(&text, 512)
            )))
        }
    }

    fn authoritative_diff(&self) -> Result<String, RuntimeError> {
        let output = Command::new("git")
            .args(["--no-pager", "diff", "--no-ext-diff", "--binary", "--"])
            .current_dir(&self.policy.contract().workspace)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|_| {
                RuntimeError::Provider("authoritative diff could not be launched".into())
            })?;
        if !output.status.success() {
            return Err(RuntimeError::Provider(
                "authoritative diff command failed".into(),
            ));
        }
        let text = bounded_output(&output.stdout, &output.stderr);
        // An empty working-tree diff is still an authoritative result; retain
        // a deterministic marker so controller completion cannot confuse it
        // with a missing capture.
        Ok(if text.trim().is_empty() {
            "authoritative-diff: clean".into()
        } else {
            text
        })
    }
}

impl AutonomousVerifierV1 for ContractVerifierV1 {
    fn verify(&mut self) -> Result<(VerificationSummaryV1, String), RuntimeError> {
        verify_git_identity(&self.policy)?;
        let mut successful_summaries = Vec::new();
        let mut failure_summaries = Vec::new();
        for profile in &self.policy.contract().verification_policy.required_commands {
            match self.execute_profile(profile) {
                Ok(summary) => successful_summaries.push(summary),
                Err(error) => {
                    // A failed profile must be diagnosable even when preceding
                    // successful profiles emitted enough bounded output to fill
                    // the durable summary. The profile identity is a closed
                    // enum serialization, and `execute_profile` has already
                    // redacted and bounded command output in `error`.
                    failure_summaries.push(failure_summary(profile, &error));
                }
            }
        }
        let (passed, summary) =
            bounded_verification_summary(failure_summaries, successful_summaries);
        let verification = VerificationSummaryV1 {
            status: if passed {
                VerificationStatusV1::Passed
            } else {
                VerificationStatusV1::Failed
            },
            command: "contract-approved verification profiles".into(),
            summary,
        };
        verify_git_identity(&self.policy)?;
        let diff = self.authoritative_diff()?;
        self.last_verification = Some(verification.clone());
        Ok((verification, diff))
    }

    fn final_review(
        &mut self,
        verification: &VerificationSummaryV1,
        authoritative_diff: &str,
    ) -> Result<String, RuntimeError> {
        if verification.status != VerificationStatusV1::Passed
            || authoritative_diff.trim().is_empty()
        {
            return Err(RuntimeError::Validation(
                "final review requires passing verification and authoritative diff".into(),
            ));
        }
        serde_json::to_string(&json!({
            "schemaVersion": 1,
            "verificationStatus": "PASSED",
            "diffBytes": authoritative_diff.len(),
            "automaticGitPublication": false,
            "result": "reviewed locally"
        }))
        .map_err(|_| RuntimeError::Validation("final review serialization failed".into()))
    }
}

fn failure_summary(profile: &AutonomousCommandProfileV1, error: &RuntimeError) -> String {
    let profile_name =
        serde_json::to_string(profile).expect("command-profile serialization is infallible");
    format!("verification failure [{profile_name}]: {error:?}")
}

fn bounded_verification_summary(
    mut failure_summaries: Vec<String>,
    successful_summaries: Vec<String>,
) -> (bool, String) {
    let passed = failure_summaries.is_empty();
    failure_summaries.extend(successful_summaries);
    (passed, bounded_text(&failure_summaries.join("\n"), 2_048))
}

fn verify_git_identity(policy: &AutonomousPolicyEngineV1) -> Result<(), RuntimeError> {
    let contract = policy.contract();
    let branch = run_git(policy, ["branch", "--show-current"])?;
    if contract.feature_branch == "HEAD" && contract.base_branch == "HEAD" {
        // Explicit detached direct work cannot create or publish Git history.
        // Unlike named-branch work, ancestry is insufficient: its complete
        // immutable source commit must still be selected at verification.
        let git_read_only = matches!(
            &contract.git_policy,
            AutonomousGitPolicyV1 {
                create_branch: false,
                create_worktree: false,
                local_commits: false,
                push_feature_branch: false,
                open_pull_request: false,
                merge: false,
                force_push: false,
                modify_main: false,
            }
        );
        if !contract.is_direct_chatgpt()
            || !git_read_only
            || !branch.trim().is_empty()
            || contract.base_commit.len() != 40
            || !contract
                .base_commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(RuntimeError::Validation(
                "autonomous detached verification requires an exact read-only Git contract".into(),
            ));
        }
        if run_git(policy, ["rev-parse", "--verify", "HEAD"])?.trim() != contract.base_commit {
            return Err(RuntimeError::Validation(
                "autonomous detached verification requires the exact base commit".into(),
            ));
        }
        run_git(
            policy,
            [
                "diff",
                "--quiet",
                "--no-ext-diff",
                "--no-textconv",
                "--ignore-submodules=none",
                "HEAD",
                "--",
            ],
        )
        .map_err(|_| {
            RuntimeError::Validation(
                "autonomous detached verification requires unchanged tracked source".into(),
            )
        })?;
    } else if branch.trim() != contract.feature_branch || branch.trim() == "main" {
        return Err(RuntimeError::Validation(
            "autonomous verifier requires the contract feature branch".into(),
        ));
    }
    let origin = run_git(policy, ["remote", "get-url", "origin"])?;
    if origin.trim() != policy.contract().expected_origin {
        return Err(RuntimeError::Validation(
            "autonomous verifier origin does not match the contract".into(),
        ));
    }
    let output = Command::new("git")
        .args([
            "merge-base",
            "--is-ancestor",
            &policy.contract().base_commit,
            "HEAD",
        ])
        .current_dir(&policy.contract().workspace)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|_| RuntimeError::Provider("Git base check could not be launched".into()))?;
    if !output.status.success() {
        return Err(RuntimeError::Validation(
            "autonomous verifier base commit is not an ancestor of HEAD".into(),
        ));
    }
    Ok(())
}

fn run_git<const N: usize>(
    policy: &AutonomousPolicyEngineV1,
    args: [&str; N],
) -> Result<String, RuntimeError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(&policy.contract().workspace)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|_| RuntimeError::Provider("Git identity command could not be launched".into()))?;
    if !output.status.success() {
        return Err(RuntimeError::Validation(
            "Git identity command did not succeed".into(),
        ));
    }
    Ok(bounded_output(&output.stdout, &output.stderr))
}

fn bounded_output(stdout: &[u8], stderr: &[u8]) -> String {
    let mut combined = String::from_utf8_lossy(stdout).into_owned();
    if !stderr.is_empty() {
        combined.push('\n');
        combined.push_str(&String::from_utf8_lossy(stderr));
    }
    bounded_text(&redact_text(&combined), MAX_CAPTURE_BYTES)
}

fn bounded_text(value: &str, limit: usize) -> String {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut text = value[..end].to_string();
    if value.len() > end {
        text.push_str("...");
    }
    text
}

fn bounded_failure_text(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    const MARKER: &str = "\n...<verification output omitted>...\n";
    let available = limit.saturating_sub(MARKER.len());
    let head_budget = available / 3;
    let tail_budget = available.saturating_sub(head_budget);

    let mut head_end = head_budget.min(value.len());
    while !value.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = value.len().saturating_sub(tail_budget);
    while tail_start < value.len() && !value.is_char_boundary(tail_start) {
        tail_start += 1;
    }

    format!("{}{}{}", &value[..head_end], MARKER, &value[tail_start..])
}

fn redact_text(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if ["token=", "password=", "secret=", "api_key=", "apikey="]
                .iter()
                .any(|marker| lower.contains(marker))
            {
                "<redacted-sensitive-output>"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::super::autonomous_contract::AutonomousDevelopmentContractV1;
    use super::*;

    struct GitFixture {
        root: std::path::PathBuf,
        temp_root: std::path::PathBuf,
        commit: String,
    }

    impl GitFixture {
        fn new() -> Self {
            let temp_root = std::env::temp_dir().canonicalize().expect("temporary root");
            let root = temp_root.join(format!("catdesk-detached-review-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&root).expect("new temporary Git repository");
            let mut fixture = Self {
                root,
                temp_root,
                commit: String::new(),
            };
            fixture.git(&["init", "-q", "--initial-branch=codex/review-fixture"]);
            fixture.git(&[
                "remote",
                "add",
                "origin",
                "https://example.invalid/review.git",
            ]);
            std::fs::write(fixture.root.join("source.txt"), "review baseline\n").expect("source");
            fixture.git(&["add", "source.txt"]);
            fixture.git(&["commit", "-q", "--allow-empty", "-m", "fixture base"]);
            fixture.commit = fixture.git(&["rev-parse", "HEAD"]).trim().into();
            fixture
        }

        fn git(&self, args: &[&str]) -> String {
            let output = Command::new("git")
                .args([
                    "-c",
                    "user.name=Review Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                ])
                .arg("-c")
                .arg(format!(
                    "core.hooksPath={}",
                    self.root.join("no-hooks").display()
                ))
                .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
                .args(args)
                .current_dir(&self.root)
                .stdin(std::process::Stdio::null())
                .output()
                .expect("fixture Git process");
            assert!(output.status.success(), "fixture Git failed: {:?}", args);
            String::from_utf8(output.stdout).expect("fixture Git UTF-8")
        }

        fn contract(&self, detached: bool) -> AutonomousDevelopmentContractV1 {
            let branch = if detached {
                "HEAD"
            } else {
                "codex/review-fixture"
            };
            serde_json::from_value(json!({
                "schemaVersion":1, "contractId":"detached-review", "taskId":"source-assessment",
                "projectId":"fixture", "mode":"chatgpt_web_direct_autonomous",
                "objective":"Read source and retain only an authorized review report",
                "workspace":self.root, "baseBranch":branch, "featureBranch":branch,
                "baseCommit":self.commit, "expectedOrigin":"https://example.invalid/review.git",
                "orderedSteps":["review"], "allowedPaths":[self.root], "forbiddenPaths":[],
                "allowedCommandProfiles":["GIT_STATUS","GIT_DIFF"],
                "gitPolicy":{"createBranch":false,"createWorktree":false,"localCommits":false,
                    "pushFeatureBranch":false,"openPullRequest":false,"merge":false,"forcePush":false,"modifyMain":false},
                "providerPolicy":{"primaryProvider":"chatgpt-web","primaryModel":"conversation-selected",
                    "routineProvider":"none","routineModel":"none","allowPaidFallback":false,"allowCloudFallback":false},
                "verificationPolicy":{"profile":"source-review","requiredCommands":["GIT_STATUS","GIT_DIFF"],
                    "maxRepairCycles":1,"requireAuthoritativeDiff":true,"requireFinalReview":true},
                "rateLimitPolicy":{"automaticPause":true,"automaticResume":true,"initialBackoffSeconds":1,
                    "maximumBackoffSeconds":2,"maximumRateLimitedSeconds":5,"honorProviderRetryAfter":true},
                "autonomyLease":{"startApprovalRequired":true,"renewalAllowed":false,"issuedAtUnix":10,
                    "expiresAtUnix":1000,"maximumTotalElapsedSeconds":90,"maximumProviderTurns":2,
                    "maximumToolCalls":8,"maximumConsecutiveFailures":1,"maximumRepairCycles":1},
                "hardStopConditions":["CANCELLATION_REQUESTED"], "completionArtifactIds":["review.json"]
            })).expect("fixture contract")
        }

        fn detached(&self) {
            self.git(&["checkout", "-q", "--detach", &self.commit]);
            assert!(self.git(&["branch", "--show-current"]).trim().is_empty());
        }
    }

    impl Drop for GitFixture {
        fn drop(&mut self) {
            // Delete only our still-identical immediate child of the canonical
            // temporary directory; never follow a replaced fixture root.
            let resolved = self.root.canonicalize().expect("fixture root still exists");
            assert_eq!(resolved, self.root);
            assert_eq!(resolved.parent(), Some(self.temp_root.as_path()));
            let cleanup = std::fs::remove_dir_all(&resolved);
            if !std::thread::panicking() {
                cleanup.expect("remove owned temporary Git repository");
            }
        }
    }

    fn admitted(contract: AutonomousDevelopmentContractV1) -> bool {
        AutonomousPolicyEngineV1::new(contract)
            .is_ok_and(|policy| ContractVerifierV1::new(policy).is_ok())
    }

    #[test]
    fn exact_detached_direct_read_only_git_contract_verifies_without_branch_changes() {
        let fixture = GitFixture::new();
        fixture.detached();
        let policy = AutonomousPolicyEngineV1::new(fixture.contract(true)).expect("valid policy");
        let mut verifier = ContractVerifierV1::new(policy).expect("exact detached source admitted");
        let (result, _) = verifier.verify().expect("read-only verification");
        assert_eq!(result.status, VerificationStatusV1::Passed);
        assert!(fixture.git(&["branch", "--show-current"]).trim().is_empty());
        assert_eq!(fixture.git(&["rev-parse", "HEAD"]).trim(), fixture.commit);
    }

    #[test]
    fn detached_contract_refuses_every_git_mutation_permission() {
        let fixture = GitFixture::new();
        fixture.detached();
        for flag in [
            "createBranch",
            "createWorktree",
            "localCommits",
            "pushFeatureBranch",
            "openPullRequest",
            "merge",
            "forcePush",
            "modifyMain",
        ] {
            let mut value = serde_json::to_value(fixture.contract(true)).expect("contract JSON");
            value["gitPolicy"][flag] = json!(true);
            let contract = serde_json::from_value(value).expect("modified fixture contract");
            assert!(!admitted(contract), "detached contract must refuse {flag}");
        }
    }

    #[test]
    fn detached_contract_refuses_wrong_commit_origin_and_abbreviated_identity() {
        let fixture = GitFixture::new();
        fixture.detached();
        let mut contract = fixture.contract(true);
        contract.base_commit = "0".repeat(40);
        assert!(!admitted(contract));
        let mut contract = fixture.contract(true);
        contract.base_commit = fixture.commit[..7].into();
        assert!(!admitted(contract));
        let mut contract = fixture.contract(true);
        contract.expected_origin = "https://example.invalid/other.git".into();
        assert!(!admitted(contract));
    }

    #[test]
    fn detached_contract_requires_explicit_head_descriptors_and_direct_provider() {
        let fixture = GitFixture::new();
        fixture.detached();
        let mut contract = fixture.contract(true);
        contract.base_branch = "main".into();
        assert!(!admitted(contract));
        let mut contract = fixture.contract(true);
        contract.mode = "chatgpt_web_codex_autonomous".into();
        contract.provider_policy.primary_provider = "codex-cli".into();
        contract.provider_policy.primary_model = "gpt-5.6-terra".into();
        contract.provider_policy.routine_provider = "ollama".into();
        contract.provider_policy.routine_model = "qwen3.6:35b-a3b".into();
        assert!(!admitted(contract));
    }

    #[test]
    fn named_branch_rules_still_require_the_exact_feature_branch_and_refuse_main() {
        let fixture = GitFixture::new();
        assert!(admitted(fixture.contract(false)));
        assert!(!admitted(fixture.contract(true)));
        let mut contract = fixture.contract(false);
        contract.feature_branch = "codex/wrong".into();
        assert!(!admitted(contract));
        fixture.git(&["checkout", "-q", "-b", "main"]);
        let mut contract = fixture.contract(false);
        contract.feature_branch = "main".into();
        assert!(!admitted(contract));
    }

    #[test]
    fn verification_rechecks_detached_commit_and_branch_after_construction() {
        let fixture = GitFixture::new();
        fixture.detached();
        let policy = AutonomousPolicyEngineV1::new(fixture.contract(true)).expect("valid policy");
        let mut verifier = ContractVerifierV1::new(policy).expect("initial identity");
        fixture.git(&[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "unexpected advancement",
        ]);
        assert!(verifier.verify().is_err());
        fixture.detached();
        let policy = AutonomousPolicyEngineV1::new(fixture.contract(true)).expect("valid policy");
        let mut verifier = ContractVerifierV1::new(policy).expect("initial identity");
        fixture.git(&["checkout", "-q", "-b", "codex/drift"]);
        assert!(verifier.verify().is_err());
    }

    #[test]
    fn detached_review_refuses_tracked_changes_and_allows_a_new_untracked_report() {
        let fixture = GitFixture::new();
        fixture.detached();
        std::fs::write(fixture.root.join("source.txt"), "changed\n").expect("fixture change");
        assert!(!admitted(fixture.contract(true)));
        fixture.git(&["add", "source.txt"]);
        assert!(!admitted(fixture.contract(true)));
        fixture.git(&["reset", "--hard", "-q", &fixture.commit]);
        let policy = AutonomousPolicyEngineV1::new(fixture.contract(true)).expect("valid policy");
        let mut verifier = ContractVerifierV1::new(policy).expect("immutable source");
        std::fs::write(fixture.root.join("review.json"), "{}\n").expect("new report");
        assert_eq!(
            verifier.verify().expect("report verification").0.status,
            VerificationStatusV1::Passed
        );
        std::fs::write(fixture.root.join("source.txt"), "later change\n").expect("fixture drift");
        assert!(verifier.verify().is_err());
    }

    #[test]
    fn output_is_bounded_on_utf8_boundaries() {
        assert_eq!(bounded_text("abc", 2), "ab...");
        assert_eq!(bounded_text("a\u{00E9}", 2), "a...");
    }

    #[test]
    fn output_redaction_removes_common_assignment_values() {
        assert_eq!(
            redact_text("token=synthetic-value"),
            "<redacted-sensitive-output>"
        );
        assert_eq!(redact_text("ordinary output"), "ordinary output");
    }

    #[test]
    fn failure_summary_precedes_large_successful_output_before_truncation() {
        let large_success = "successful profile output\n".repeat(256);
        let failure = failure_summary(
            &AutonomousCommandProfileV1::GitDiff,
            &RuntimeError::Provider("exact late failure".into()),
        );
        let (passed, summary) = bounded_verification_summary(vec![failure], vec![large_success]);

        assert!(!passed);
        assert!(
            summary.starts_with(
                "verification failure [\"GIT_DIFF\"]: Provider(\"exact late failure\")"
            )
        );
        assert!(summary.len() <= 2_051, "the bounded summary remains capped");
        assert!(summary.ends_with("..."), "large success remains truncated");
    }

    #[test]
    fn failed_profile_output_retains_bounded_head_and_tail() {
        let value = format!(
            "head:{}{}tail:failures: exact_late_test\n",
            "a".repeat(400),
            "é".repeat(200)
        );
        let bounded = bounded_failure_text(&value, 512);

        assert!(bounded.starts_with("head:"));
        assert!(bounded.contains("...<verification output omitted>..."));
        assert!(bounded.ends_with("tail:failures: exact_late_test\n"));
        assert!(bounded.len() <= 512);
        assert!(std::str::from_utf8(bounded.as_bytes()).is_ok());
    }

    #[test]
    fn successful_profile_summaries_retain_their_original_order() {
        let (passed, summary) = bounded_verification_summary(
            Vec::new(),
            vec!["fmt passed".to_string(), "test passed".to_string()],
        );

        assert!(passed);
        assert_eq!(summary, "fmt passed\ntest passed");
    }
}
