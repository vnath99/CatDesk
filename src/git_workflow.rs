use crate::command;
use crate::verification;
use serde::Serialize;
use std::path::{Path, PathBuf};

const GIT_TIMEOUT_MS: u64 = 120_000;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusSummary {
    pub branch: String,
    pub clean: bool,
    pub warn_on_main: bool,
    pub raw: String,
    pub summary: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffSummary {
    pub name_status: String,
    pub stat: String,
    pub summary: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommandOutput {
    pub success: bool,
    pub summary: String,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitVerifiedOutput {
    pub success: bool,
    pub verification_success: bool,
    pub verification_summary: String,
    pub commit: GitCommandOutput,
}

fn workspace_root_path(workspace_root: &str) -> Result<PathBuf, String> {
    Path::new(workspace_root)
        .canonicalize()
        .map(command::normalize_windows_verbatim_path)
        .map_err(|e| e.to_string())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn validate_branch_name(branch: &str) -> Result<(), String> {
    let branch = branch.trim();
    if branch.is_empty() || branch.starts_with('-') || branch.contains("..") {
        return Err("Invalid branch name".into());
    }
    if !branch
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.'))
    {
        return Err(
            "Branch names may contain only ASCII letters, numbers, '/', '-', '_', and '.'".into(),
        );
    }
    Ok(())
}

fn current_branch_from_status(raw: &str) -> String {
    raw.lines()
        .next()
        .and_then(|line| line.strip_prefix("## "))
        .map(|line| {
            line.strip_prefix("No commits yet on ")
                .unwrap_or(line)
                .split(['.', ' '])
                .next()
                .unwrap_or(line)
                .to_string()
        })
        .filter(|branch| !branch.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn status_summary_text(branch: &str, clean: bool, warn_on_main: bool, raw: &str) -> String {
    let mut summary = format!(
        "branch: {branch}\nclean: {}\n",
        if clean { "yes" } else { "no" }
    );
    if warn_on_main {
        summary
            .push_str("warning: currently on main; create a feature branch before committing.\n");
    }
    if !raw.trim().is_empty() {
        summary.push_str("\n");
        summary.push_str(raw.trim());
    }
    summary
}

pub async fn status_summary(workspace_root: &str) -> Result<GitStatusSummary, String> {
    let root = workspace_root_path(workspace_root)?;
    let result = command::run_command("git status --short --branch", &root, GIT_TIMEOUT_MS).await;
    if !result.success {
        return Err(command::format_result(&result));
    }
    let raw = result.stdout;
    let branch = current_branch_from_status(&raw);
    let clean = raw.lines().count() <= 1;
    let warn_on_main = matches!(branch.as_str(), "main" | "master");
    let summary = status_summary_text(&branch, clean, warn_on_main, &raw);
    Ok(GitStatusSummary {
        branch,
        clean,
        warn_on_main,
        raw,
        summary,
    })
}

pub async fn create_feature_branch(
    workspace_root: &str,
    branch: &str,
) -> Result<GitCommandOutput, String> {
    validate_branch_name(branch)?;
    let root = workspace_root_path(workspace_root)?;
    let command_line = format!("git switch -c {}", shell_quote(branch.trim()));
    let result = command::run_command(&command_line, &root, GIT_TIMEOUT_MS).await;
    Ok(GitCommandOutput {
        success: result.success,
        summary: command::format_result(&result),
        stdout: result.stdout,
        stderr: result.stderr,
    })
}

pub async fn diff_summary(workspace_root: &str) -> Result<GitDiffSummary, String> {
    let root = workspace_root_path(workspace_root)?;
    let name_status = command::run_command("git diff --name-status", &root, GIT_TIMEOUT_MS).await;
    let stat = command::run_command("git diff --stat", &root, GIT_TIMEOUT_MS).await;
    if !name_status.success {
        return Err(command::format_result(&name_status));
    }
    if !stat.success {
        return Err(command::format_result(&stat));
    }
    let summary = if name_status.stdout.trim().is_empty() {
        "No unstaged diff.".to_string()
    } else {
        format!(
            "changed files:\n{}\n\nstat:\n{}",
            name_status.stdout.trim(),
            stat.stdout.trim()
        )
    };
    Ok(GitDiffSummary {
        name_status: name_status.stdout,
        stat: stat.stdout,
        summary,
    })
}

pub async fn commit_verified_changes(
    workspace_root: &str,
    message: &str,
    allow_failed_verification: bool,
) -> Result<GitCommitVerifiedOutput, String> {
    if message.trim().is_empty() {
        return Err("Commit message must not be empty".into());
    }
    let root = workspace_root_path(workspace_root)?;
    let verification = verification::verify_project(workspace_root).await?;
    let verification_summary = verification.render_text();
    if !verification.success && !allow_failed_verification {
        return Ok(GitCommitVerifiedOutput {
            success: false,
            verification_success: false,
            verification_summary,
            commit: GitCommandOutput {
                success: false,
                summary: "verification failed; commit was not created".into(),
                stdout: String::new(),
                stderr: String::new(),
            },
        });
    }

    let add = command::run_command("git add -A", &root, GIT_TIMEOUT_MS).await;
    if !add.success {
        return Err(command::format_result(&add));
    }
    let commit_line = format!("git commit -m {}", shell_quote(message.trim()));
    let commit = command::run_command(&commit_line, &root, GIT_TIMEOUT_MS).await;
    Ok(GitCommitVerifiedOutput {
        success: commit.success,
        verification_success: verification.success,
        verification_summary,
        commit: GitCommandOutput {
            success: commit.success,
            summary: command::format_result(&commit),
            stdout: commit.stdout,
            stderr: commit.stderr,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_and_warns_on_main() {
        let raw = "## main...origin/main\n M src/main.rs\n";
        let branch = current_branch_from_status(raw);
        assert_eq!(branch, "main");
        assert_eq!(
            current_branch_from_status("## No commits yet on main\n"),
            "main"
        );
        let summary = status_summary_text(&branch, false, true, raw);
        assert!(summary.contains("warning: currently on main"));
    }

    #[test]
    fn validates_feature_branch_names() {
        assert!(validate_branch_name("feature/git-workflow").is_ok());
        assert!(validate_branch_name("-bad").is_err());
        assert!(validate_branch_name("bad name").is_err());
        assert!(validate_branch_name("bad..name").is_err());
    }
}
