use crate::command;
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_VERIFY_TIMEOUT_MS: u64 = 120_000;
const MAX_SUMMARY_CHARS: usize = 1_200;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationCommand {
    pub ecosystem: String,
    pub command: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResult {
    pub ecosystem: String,
    pub command: String,
    pub success: bool,
    pub elapsed_ms: u64,
    pub summary: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyProjectOutput {
    pub success: bool,
    pub commands: Vec<VerificationResult>,
    pub skipped: Vec<String>,
}

impl VerifyProjectOutput {
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("success: {}\n", self.success));
        out.push_str(&format!("commands: {}\n", self.commands.len()));
        if !self.skipped.is_empty() {
            out.push_str("\nskipped:\n");
            for item in &self.skipped {
                out.push_str("- ");
                out.push_str(item);
                out.push('\n');
            }
        }
        for result in &self.commands {
            out.push_str(&format!(
                "\n## {} - {}\nstatus: {}\nelapsed_ms: {}\n\n{}\n",
                result.ecosystem,
                result.command,
                if result.success { "passed" } else { "failed" },
                result.elapsed_ms,
                result.summary
            ));
        }
        out
    }
}

fn workspace_root_path(workspace_root: &str) -> Result<PathBuf, String> {
    Path::new(workspace_root)
        .canonicalize()
        .map(command::normalize_windows_verbatim_path)
        .map_err(|e| e.to_string())
}

fn has_python_files(root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .path()
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "py")
    })
}

fn package_script_exists(root: &Path, script: &str) -> bool {
    let Ok(text) = fs::read_to_string(root.join("package.json")) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    value
        .get("scripts")
        .and_then(Value::as_object)
        .is_some_and(|scripts| scripts.contains_key(script))
}

pub fn verification_plan(
    workspace_root: &str,
) -> Result<(Vec<VerificationCommand>, Vec<String>), String> {
    let root = workspace_root_path(workspace_root)?;
    let mut commands = Vec::new();
    let mut skipped = Vec::new();

    if root.join("Cargo.toml").is_file() {
        commands.extend([
            VerificationCommand {
                ecosystem: "Rust".into(),
                command: "cargo fmt --check".into(),
            },
            VerificationCommand {
                ecosystem: "Rust".into(),
                command: "cargo test".into(),
            },
            VerificationCommand {
                ecosystem: "Rust".into(),
                command: "cargo build".into(),
            },
        ]);
    } else {
        skipped.push("Rust: Cargo.toml not found".into());
    }

    if root.join("package.json").is_file() {
        for (script, command) in [
            ("test", "npm test"),
            ("build", "npm run build"),
            ("lint", "npm run lint"),
        ] {
            if package_script_exists(&root, script) {
                commands.push(VerificationCommand {
                    ecosystem: "Node".into(),
                    command: command.into(),
                });
            } else {
                skipped.push(format!("Node: package.json has no `{script}` script"));
            }
        }
    } else {
        skipped.push("Node: package.json not found".into());
    }

    if root.join("pyproject.toml").is_file()
        || root.join("setup.py").is_file()
        || root.join("requirements.txt").is_file()
        || has_python_files(&root)
    {
        commands.extend([
            VerificationCommand {
                ecosystem: "Python".into(),
                command: "pytest".into(),
            },
            VerificationCommand {
                ecosystem: "Python".into(),
                command: "ruff check .".into(),
            },
            VerificationCommand {
                ecosystem: "Python".into(),
                command: "mypy .".into(),
            },
        ]);
    } else {
        skipped.push("Python: no Python project files found".into());
    }

    Ok((commands, skipped))
}

fn summarize_output(stdout: &str, stderr: &str) -> String {
    let mut combined = String::new();
    if !stdout.trim().is_empty() {
        combined.push_str("stdout:\n");
        combined.push_str(stdout.trim());
    }
    if !stderr.trim().is_empty() {
        if !combined.is_empty() {
            combined.push_str("\n\n");
        }
        combined.push_str("stderr:\n");
        combined.push_str(stderr.trim());
    }
    if combined.is_empty() {
        combined.push_str("(no output)");
    }
    if combined.chars().count() <= MAX_SUMMARY_CHARS {
        return combined;
    }
    let mut out = combined
        .chars()
        .take(MAX_SUMMARY_CHARS.saturating_sub(32))
        .collect::<String>();
    out.push_str("\n[output truncated]");
    out
}

pub async fn verify_project(workspace_root: &str) -> Result<VerifyProjectOutput, String> {
    let root = workspace_root_path(workspace_root)?;
    let (commands, skipped) = verification_plan(workspace_root)?;
    let mut results = Vec::new();

    for command_to_run in commands {
        let result =
            command::run_command(&command_to_run.command, &root, DEFAULT_VERIFY_TIMEOUT_MS).await;
        results.push(VerificationResult {
            ecosystem: command_to_run.ecosystem,
            command: command_to_run.command,
            success: result.success,
            elapsed_ms: result.elapsed_ms,
            summary: summarize_output(&result.stdout, &result.stderr),
        });
    }

    Ok(VerifyProjectOutput {
        success: results.iter().all(|result| result.success),
        commands: results,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_workspace(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("catdesk-verification-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn verification_plan_detects_rust_node_and_python_commands() {
        let workspace = test_workspace("plan");
        fs::create_dir_all(&workspace).expect("create workspace");
        fs::write(workspace.join("Cargo.toml"), "[package]\nname = \"demo\"\n")
            .expect("write cargo");
        fs::write(
            workspace.join("package.json"),
            r#"{"scripts":{"test":"echo test","build":"echo build"}}"#,
        )
        .expect("write package");
        fs::write(
            workspace.join("pyproject.toml"),
            "[project]\nname = \"demo\"\n",
        )
        .expect("write pyproject");

        let (commands, skipped) =
            verification_plan(&workspace.to_string_lossy()).expect("build plan");
        let command_names = commands
            .iter()
            .map(|command| command.command.as_str())
            .collect::<Vec<_>>();
        assert!(command_names.contains(&"cargo fmt --check"));
        assert!(command_names.contains(&"cargo test"));
        assert!(command_names.contains(&"cargo build"));
        assert!(command_names.contains(&"npm test"));
        assert!(command_names.contains(&"npm run build"));
        assert!(command_names.contains(&"pytest"));
        assert!(command_names.contains(&"ruff check ."));
        assert!(command_names.contains(&"mypy ."));
        assert!(skipped.iter().any(|item| item.contains("lint")));

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn summarize_output_truncates_long_output() {
        let summary = summarize_output(&"a".repeat(MAX_SUMMARY_CHARS + 100), "");
        assert!(summary.contains("[output truncated]"));
        assert!(summary.len() < MAX_SUMMARY_CHARS + 64);
    }
}
