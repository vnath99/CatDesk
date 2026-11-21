//! Fixed project connections through the existing selected-development owner.
//! No external listener, tunnel, browser operation or production authority.
use super::*;
use std::fs::{File, OpenOptions};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectOptions {
    pub selection: crate::project_mcp::Selection,
    pub assignment: Option<PathBuf>,
    pub maximum_seconds: u64,
}

pub(crate) fn parse_project_options(args: &[String]) -> Result<ProjectOptions, String> {
    let usage = "DEV_PROJECT_ARGUMENTS_INVALID: run-project-backend --project-id <id> --project-target-generation <n> --project-target-sha256 <digest> --max-runtime-seconds <1..180> [--project-task-contract <absolute-file>]";
    if !matches!(args.len(), 11 | 13)
        || args[..4]
            != [
                "operator",
                "web-dev-runtime",
                "run-project-backend",
                "--project-id",
            ]
        || args[5] != "--project-target-generation"
        || args[7] != "--project-target-sha256"
        || args[9] != "--max-runtime-seconds"
        || !catdesk_wake::protocol::valid_id(&args[4])
        || args[8].len() != 64
        || !args[8]
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(usage.into());
    }
    let target_generation = args[6]
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or(usage)?;
    let maximum_seconds = args[10]
        .parse::<u64>()
        .ok()
        .filter(|n| (1..=180).contains(n))
        .ok_or(usage)?;
    let assignment = if args.len() == 13 {
        if args[11] != "--project-task-contract"
            || args[12].len() > 4096
            || args[12].chars().any(char::is_control)
            || !Path::new(&args[12]).is_absolute()
        {
            return Err(usage.into());
        }
        Some(PathBuf::from(&args[12]))
    } else {
        None
    };
    Ok(ProjectOptions {
        selection: crate::project_mcp::Selection {
            project_id: args[4].clone(),
            target_generation,
            target_sha256: args[8].clone(),
        },
        assignment,
        maximum_seconds,
    })
}

pub(crate) async fn run_project(
    workspace: &Path,
    options: ProjectOptions,
) -> Result<Value, String> {
    let token = process_auth(std::env::var("CATDESK_MCP_AUTH_TOKEN").ok())?;
    let workspace = workspace.to_path_buf();
    tokio::task::spawn_blocking(move || run_owned(&workspace, options, &token))
        .await
        .map_err(|_| "DEV_BACKEND_OWNER_FAILED")?
}

fn expected_scope(
    workspace: &Path,
    options: &ProjectOptions,
) -> Result<(Value, Option<File>), String> {
    let registered =
        crate::delegated::autonomy_projects::resolve_registered_project_from_control_root(
            workspace,
            &options.selection.project_id,
        )
        .map_err(|_| "DEV_PROJECT_REGISTRATION_UNAVAILABLE")?;
    let target =
        crate::mcp::operator_read_project_web_target(workspace, &options.selection.project_id)
            .map_err(|_| "DEV_PROJECT_TARGET_UNAVAILABLE")?
            .ok_or("DEV_PROJECT_TARGET_UNBOUND")?;
    if target.generation != options.selection.target_generation
        || target.sha256 != options.selection.target_sha256
    {
        return Err("DEV_PROJECT_BINDING_CHANGED".into());
    }
    let mut expected = json!({"projectId":options.selection.project_id,
        "targetGeneration":options.selection.target_generation,"targetSha256":options.selection.target_sha256,
        "executionWorkspace":registered.project.workspace,"nativeWriterClaimed":false,"assignedTask":null});
    let pin = if let Some(path) = &options.assignment {
        use std::os::windows::fs::OpenOptionsExt;
        let pin = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(path)
            .map_err(|_| "DEV_PROJECT_ASSIGNMENT_UNAVAILABLE")?;
        let bytes = crate::project_mcp_task::read_assignment_bytes(path)?;
        let assignment: Value =
            serde_json::from_slice(&bytes).map_err(|_| "DEV_PROJECT_ASSIGNMENT_INVALID")?;
        let task_id = assignment["contract"]["taskId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("DEV_PROJECT_ASSIGNMENT_INVALID")?;
        let root = assignment["contract"]["workspace"]
            .as_str()
            .map(Path::new)
            .and_then(|p| p.canonicalize().ok())
            .ok_or("DEV_PROJECT_ASSIGNMENT_INVALID")?;
        if assignment["projectId"] != options.selection.project_id {
            return Err("DEV_PROJECT_ASSIGNMENT_INVALID".into());
        }
        expected["assignedTask"] = json!({"taskId":task_id,"executionWorkspace":root,
            "registeredWorkspace":registered.project.workspace,"nativeWriterClaimed":false});
        Some(pin)
    } else {
        None
    };
    Ok((expected, pin))
}

pub(super) fn verify_scope(actual: &Value, expected: &Value) -> Result<(), String> {
    for field in [
        "projectId",
        "targetGeneration",
        "targetSha256",
        "executionWorkspace",
        "nativeWriterClaimed",
    ] {
        if actual[field] != expected[field] || actual[field].is_null() {
            return Err("DEV_PROJECT_SCOPE_CHANGED".into());
        }
    }
    if expected["assignedTask"].is_null() {
        if !actual["assignedTask"].is_null() || actual["access"] != "READ_ONLY" {
            return Err("DEV_PROJECT_SCOPE_CHANGED".into());
        }
    } else {
        for field in [
            "taskId",
            "executionWorkspace",
            "registeredWorkspace",
            "nativeWriterClaimed",
        ] {
            if actual["assignedTask"][field] != expected["assignedTask"][field]
                || actual["assignedTask"][field].is_null()
            {
                return Err("DEV_PROJECT_SCOPE_CHANGED".into());
            }
        }
        if !matches!(
            actual["access"].as_str(),
            Some(
                "READ_ONLY_INSPECTION_WITH_ASSIGNED_TASK_PATCH_TOOLS"
                    | "READ_ONLY_INSPECTION_WITH_ASSIGNED_TASK_PATCH_AND_COMMAND_TOOLS"
            )
        ) {
            return Err("DEV_PROJECT_SCOPE_CHANGED".into());
        }
    }
    Ok(())
}

fn run_owned(workspace: &Path, options: ProjectOptions, token: &str) -> Result<Value, String> {
    if !(1..=180).contains(&options.maximum_seconds) {
        return Err("DEV_BACKEND_BUDGET_INVALID".into());
    }
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "DEV_WORKSPACE_UNAVAILABLE")?;
    // Refuse an unregistered/stale request before selection can attempt any
    // normal stopped-package recovery or initialize its backend lease.
    let (expected, _assignment_pin) = expected_scope(&workspace, &options)?;
    let selection = development_runtime::select_project_backend(&workspace)?;
    // Shared listeners cannot rotate one another's selected image on failure.
    // The normal exclusive STOPPED recovery command remains available afterward.
    attempt_backend(
        &selection,
        options.maximum_seconds,
        token,
        None,
        Some((&options, &expected)),
    )
    .map_err(|failure| failure.code)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_project_refuses_before_backend_selection_or_state_initialization() {
        let root =
            std::env::temp_dir().join(format!("dev-project-admission-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let options = ProjectOptions {
            selection: crate::project_mcp::Selection {
                project_id: "unknown".into(),
                target_generation: 1,
                target_sha256: "a".repeat(64),
            },
            assignment: None,
            maximum_seconds: 1,
        };
        assert_eq!(
            run_owned(&root, options, &"x".repeat(64)).unwrap_err(),
            "DEV_PROJECT_REGISTRATION_UNAVAILABLE"
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn project_backend_parser_refuses_missing_stale_or_unbounded_scope() {
        let args: Vec<_> = [
            "operator",
            "web-dev-runtime",
            "run-project-backend",
            "--project-id",
            "alpha",
            "--project-target-generation",
            "7",
            "--project-target-sha256",
            &"a".repeat(64),
            "--max-runtime-seconds",
            "9",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let parsed = parse_project_options(&args).unwrap();
        assert_eq!(parsed.selection.project_id, "alpha");
        assert_eq!(parsed.maximum_seconds, 9);
        for (index, value) in [
            (4, "../beta"),
            (6, "0"),
            (8, "bad"),
            (10, "0"),
            (10, "181"),
            (9, "--port"),
        ] {
            let mut bad = args.clone();
            bad[index] = value.into();
            assert!(parse_project_options(&bad).is_err());
        }
        for length in 0..args.len() {
            assert!(parse_project_options(&args[..length]).is_err());
        }
        let mut assigned = args.clone();
        assigned.extend([
            "--project-task-contract".into(),
            std::env::temp_dir()
                .join("task.json")
                .to_string_lossy()
                .into_owned(),
        ]);
        assert!(
            parse_project_options(&assigned)
                .unwrap()
                .assignment
                .is_some()
        );
        assigned[12] = "relative-task.json".into();
        assert!(parse_project_options(&assigned).is_err());
        let mut extra = args;
        extra.extend(["--port".into(), "3200".into()]);
        assert!(parse_project_options(&extra).is_err());
    }
}
