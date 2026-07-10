use crate::command;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

const CATDESK_DIR: &str = ".catdesk";
const CURRENT_PLAN_FILE: &str = "current_plan.md";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentPlanOutput {
    pub path: String,
    pub plan_required: bool,
    pub text: String,
}

impl CurrentPlanOutput {
    pub fn render_text(&self) -> String {
        format!(
            "path: {}\nplan_required: {}\n\n{}",
            self.path, self.plan_required, self.text
        )
    }
}

fn workspace_root_path(workspace_root: &str) -> Result<PathBuf, String> {
    Path::new(workspace_root)
        .canonicalize()
        .map(command::normalize_windows_verbatim_path)
        .map_err(|e| e.to_string())
}

fn tool_path_string(path: &Path) -> String {
    let path = path.display().to_string();
    #[cfg(windows)]
    {
        path.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        path
    }
}

fn to_workspace_relative(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".into(),
        Ok(rel) => tool_path_string(rel),
        Err(_) => tool_path_string(path),
    }
}

fn plan_path(root: &Path) -> PathBuf {
    root.join(CATDESK_DIR).join(CURRENT_PLAN_FILE)
}

fn normalize_markdown(text: &str) -> String {
    let mut text = text.replace("\r\n", "\n").replace('\r', "\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn parse_plan_required(text: &str) -> bool {
    text.lines()
        .find_map(|line| line.strip_prefix("plan_required:"))
        .map(|value| value.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub fn update(
    workspace_root: &str,
    plan: &str,
    plan_required: bool,
) -> Result<CurrentPlanOutput, String> {
    let root = workspace_root_path(workspace_root)?;
    let path = plan_path(&root);
    let parent = path
        .parent()
        .ok_or_else(|| "failed to resolve .catdesk directory".to_string())?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let text = format!(
        "# Current Plan\n\nplan_required: {}\n\n## Plan\n\n{}",
        plan_required,
        normalize_markdown(plan.trim())
    );
    fs::write(&path, &text).map_err(|e| e.to_string())?;
    Ok(CurrentPlanOutput {
        path: to_workspace_relative(&root, &path),
        plan_required,
        text,
    })
}

pub fn read(workspace_root: &str) -> Result<CurrentPlanOutput, String> {
    let root = workspace_root_path(workspace_root)?;
    let path = plan_path(&root);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            "# Current Plan\n\nplan_required: false\n\n## Plan\n\n_No current plan recorded._\n"
                .to_string()
        }
        Err(e) => return Err(e.to_string()),
    };
    Ok(CurrentPlanOutput {
        path: to_workspace_relative(&root, &path),
        plan_required: parse_plan_required(&text),
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_workspace(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("catdesk-planning-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn update_and_read_current_plan() {
        let workspace = test_workspace("current-plan");
        fs::create_dir_all(&workspace).expect("create workspace");

        let output = update(&workspace.to_string_lossy(), "1. Inspect\n2. Build", true)
            .expect("update plan");
        assert_eq!(output.path, ".catdesk/current_plan.md");
        assert!(output.plan_required);

        let read_output = read(&workspace.to_string_lossy()).expect("read plan");
        assert!(read_output.plan_required);
        assert!(read_output.text.contains("1. Inspect"));

        let _ = fs::remove_dir_all(workspace);
    }
}
