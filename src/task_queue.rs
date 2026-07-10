use crate::command;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

const CATDESK_DIR: &str = ".catdesk";
const TODO_FILE: &str = "todo.md";
const DEFAULT_TODO_TEXT: &str = "# Todo\n\n- [ ] Capture project follow-up work here.\n";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskItem {
    pub index: usize,
    pub done: bool,
    pub text: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQueueOutput {
    pub path: String,
    pub total: usize,
    pub open: usize,
    pub done: usize,
    pub tasks: Vec<TaskItem>,
    pub text: String,
}

impl TaskQueueOutput {
    pub fn render_text(&self) -> String {
        let mut out = format!(
            "path: {}\ntotal: {}\nopen: {}\ndone: {}\n",
            self.path, self.total, self.open, self.done
        );
        if self.tasks.is_empty() {
            out.push_str("\n_No tasks recorded._\n");
        } else {
            out.push_str("\n## Tasks\n");
            for task in &self.tasks {
                let marker = if task.done { "x" } else { " " };
                out.push_str(&format!("{}. [{}] {}\n", task.index, marker, task.text));
            }
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

fn todo_path(root: &Path) -> PathBuf {
    root.join(CATDESK_DIR).join(TODO_FILE)
}

fn normalize_markdown(text: &str) -> String {
    let mut text = text.replace("\r\n", "\n").replace('\r', "\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn ensure_todo_file(root: &Path) -> Result<PathBuf, String> {
    let path = todo_path(root);
    let parent = path
        .parent()
        .ok_or_else(|| "failed to resolve .catdesk directory".to_string())?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    if !path.exists() {
        fs::write(&path, DEFAULT_TODO_TEXT).map_err(|e| e.to_string())?;
    }
    Ok(path)
}

fn parse_checkbox_line(line: &str) -> Option<(bool, String)> {
    let trimmed = line.trim_start();
    for prefix in ["- [ ] ", "* [ ] ", "- [x] ", "- [X] ", "* [x] ", "* [X] "] {
        if let Some(task) = trimmed.strip_prefix(prefix) {
            return Some((
                prefix.contains('x') || prefix.contains('X'),
                task.to_string(),
            ));
        }
    }
    None
}

fn parse_tasks(text: &str) -> Vec<TaskItem> {
    let mut tasks = Vec::new();
    for line in text.lines() {
        if let Some((done, task_text)) = parse_checkbox_line(line) {
            tasks.push(TaskItem {
                index: tasks.len() + 1,
                done,
                text: task_text,
            });
        }
    }
    tasks
}

fn output_from_text(root: &Path, path: &Path, text: String) -> TaskQueueOutput {
    let tasks = parse_tasks(&text);
    let done = tasks.iter().filter(|task| task.done).count();
    TaskQueueOutput {
        path: to_workspace_relative(root, path),
        total: tasks.len(),
        open: tasks.len().saturating_sub(done),
        done,
        tasks,
        text,
    }
}

pub fn read(workspace_root: &str) -> Result<TaskQueueOutput, String> {
    let root = workspace_root_path(workspace_root)?;
    let path = ensure_todo_file(&root)?;
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(output_from_text(&root, &path, text))
}

pub fn add(workspace_root: &str, tasks: &[String]) -> Result<TaskQueueOutput, String> {
    let root = workspace_root_path(workspace_root)?;
    let path = ensure_todo_file(&root)?;
    let mut text = normalize_markdown(&fs::read_to_string(&path).map_err(|e| e.to_string())?);
    if !text.ends_with("\n\n") {
        text.push('\n');
    }
    for task in tasks
        .iter()
        .map(|task| task.trim())
        .filter(|task| !task.is_empty())
    {
        text.push_str("- [ ] ");
        text.push_str(task);
        text.push('\n');
    }
    fs::write(&path, &text).map_err(|e| e.to_string())?;
    Ok(output_from_text(&root, &path, text))
}

pub fn set_status(
    workspace_root: &str,
    index: usize,
    done: bool,
) -> Result<TaskQueueOutput, String> {
    if index == 0 {
        return Err("index must be 1 or greater".into());
    }
    let root = workspace_root_path(workspace_root)?;
    let path = ensure_todo_file(&root)?;
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut seen = 0usize;
    let mut updated = false;
    let mut next = String::new();
    for line in text.lines() {
        if let Some((_, task_text)) = parse_checkbox_line(line) {
            seen += 1;
            if seen == index {
                let indent_len = line.len().saturating_sub(line.trim_start().len());
                next.push_str(&line[..indent_len]);
                next.push_str(if done { "- [x] " } else { "- [ ] " });
                next.push_str(&task_text);
                next.push('\n');
                updated = true;
                continue;
            }
        }
        next.push_str(line);
        next.push('\n');
    }
    if !updated {
        return Err(format!("No task found at index {index}"));
    }
    fs::write(&path, &next).map_err(|e| e.to_string())?;
    Ok(output_from_text(&root, &path, next))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_workspace(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("catdesk-task-queue-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn add_and_complete_tasks_in_markdown_todo() {
        let workspace = test_workspace("todo");
        fs::create_dir_all(&workspace).expect("create workspace");

        let added = add(
            &workspace.to_string_lossy(),
            &[
                "Write task queue tests".to_string(),
                "Wire MCP tools".to_string(),
            ],
        )
        .expect("add tasks");
        assert_eq!(added.open, 3);
        assert!(added.text.contains("- [ ] Write task queue tests"));

        let completed = set_status(&workspace.to_string_lossy(), 2, true).expect("complete task");
        assert_eq!(completed.done, 1);
        assert!(completed.text.contains("- [x] Write task queue tests"));

        let _ = fs::remove_dir_all(workspace);
    }
}
