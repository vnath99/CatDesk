//! Contract-bound local verification and final-review artifacts.
//!
//! Commands are executed directly without a shell and only after the
//! autonomous policy accepts their exact argument vector. This module never
//! stages, commits, pushes, or changes the selected branch.

use std::process::Command;

use serde_json::json;

use super::autonomous_contract::{AutonomousCommandProfileV1, AutonomousPolicyEngineV1};
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
    let branch = run_git(policy, ["branch", "--show-current"])?;
    if branch.trim() != policy.contract().feature_branch || branch.trim() == "main" {
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
    use super::*;

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
