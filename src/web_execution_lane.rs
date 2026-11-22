//! Explicit serial admission for linked Web tasks, stored beside their work
//! ledgers. Mutations share the existing registration lock with work closure.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Owner {
    project_id: String,
    request_id: String,
    event_id: String,
    assignment: WorkTaskAssignment,
}

impl Owner {
    fn new(project: &str, record: &WorkRecord) -> Result<Self, String> {
        Ok(Self {
            project_id: project.into(),
            request_id: record.request_id.clone(),
            event_id: record.event_id.clone(),
            assignment: record
                .task_assignment
                .clone()
                .ok_or("WEB_WORK_ASSIGNMENT_REQUIRED")?,
        })
    }

    fn open(&self, workspace: &Path) -> Result<bool, String> {
        let record = WorkLedger::open_read_only(workspace, &self.project_id)?
            .record(&self.request_id)?
            .ok_or("WEB_WORK_LANE_OWNER_UNPROVEN")?;
        if Self::new(&self.project_id, &record)? != *self {
            return Err("WEB_WORK_LANE_OWNER_UNPROVEN".into());
        }
        Ok(record.phase == "OPEN")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Lane {
    schema_version: u32,
    owner: Option<Owner>,
}

fn path(workspace: &Path) -> PathBuf {
    workspace.join(".catdesk/web-work/.execution-lane.json")
}

fn load(workspace: &Path) -> Result<Option<Lane>, String> {
    let path = path(workspace);
    let parent = path.parent().ok_or("WEB_WORK_LANE_UNAVAILABLE")?;
    for directory in [parent.parent().ok_or("WEB_WORK_LANE_UNAVAILABLE")?, parent] {
        match fs::symlink_metadata(directory) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            _ => plain(directory, true).map_err(|_| "WEB_WORK_LANE_UNAVAILABLE")?,
        }
    }
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        _ => plain(&path, false).map_err(|_| "WEB_WORK_LANE_UNAVAILABLE")?,
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "WEB_WORK_LANE_UNAVAILABLE")?
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|_| "WEB_WORK_LANE_UNAVAILABLE")?;
    if bytes.len() > 8192 {
        return Err("WEB_WORK_LANE_INVALID".into());
    }
    let lane: Lane = serde_json::from_slice(&bytes).map_err(|_| "WEB_WORK_LANE_INVALID")?;
    if lane.schema_version != 1
        || lane.owner.as_ref().is_some_and(|owner| {
            !valid_request(&owner.project_id)
                || !valid_request(&owner.request_id)
                || owner.event_id != expected_event(&owner.project_id, &owner.request_id)
                || !valid_request(&owner.assignment.task_id)
                || !sha256(&owner.assignment.assignment_sha256)
                || !sha256(&owner.assignment.scope_sha256)
        })
    {
        return Err("WEB_WORK_LANE_INVALID".into());
    }
    Ok(Some(lane))
}

pub(crate) fn status(workspace: &Path, project: &str) -> Value {
    let observed = (|| {
        let Some(lane) = load(workspace)? else {
            return Ok::<_, String>(json!({"status":"NOT_ENABLED"}));
        };
        if let Some(owner) = lane.owner
            && owner.open(workspace)?
        {
            return Ok(if owner.project_id == project {
                json!({"status":"ACTIVE","requestId":owner.request_id})
            } else if let Some(queued) =
                WorkLedger::open_read_only(workspace, project)?.active_record()?
                && queued.task_assignment.is_some()
            {
                json!({"status":"QUEUED","requestId":queued.request_id})
            } else {
                json!({"status":"HELD_BY_OTHER_PROJECT"})
            });
        }
        Ok(json!({"status":"AVAILABLE_FOR_SELECTION"}))
    })();
    observed.unwrap_or_else(|reason| json!({"status":"ATTENTION","reason":reason}))
}

pub(crate) fn is_enabled(workspace: &Path) -> Result<bool, String> {
    Ok(load(workspace)?.is_some())
}

/// An unselected project keeps its original OPEN record. Reads, review work
/// and native Codex sessions do not enter this direct-Web effect gate.
pub(crate) fn require_owner(
    workspace: &Path,
    project: &str,
    record: &WorkRecord,
) -> Result<(), String> {
    let Some(lane) = load(workspace)? else {
        return Ok(());
    };
    let requested = Owner::new(project, record)?;
    if record.phase == "OPEN" && lane.owner.as_ref() == Some(&requested) {
        return Ok(());
    }
    Err("WEB_WORK_EXECUTION_QUEUED".into())
}

/// The caller retains the project lease and registration lock, and has checked
/// the exact requested record and the absence of unresolved work when yielding.
pub(crate) fn transition(
    workspace: &Path,
    project: &str,
    record: &WorkRecord,
    select: bool,
) -> Result<Value, String> {
    if record.phase != "OPEN" {
        return Err("WEB_WORK_REQUEST_CLOSED".into());
    }
    let requested = Owner::new(project, record)?;
    let lane = load(workspace)?;
    let current = lane.as_ref().and_then(|state| state.owner.as_ref());
    if select {
        if current == Some(&requested) {
            return Ok(
                json!({"status":"ACTIVE","projectId":project,"requestId":record.request_id}),
            );
        }
        if let Some(current) = current
            && current.open(workspace)?
        {
            return Err("WEB_WORK_LANE_ALREADY_OWNED".into());
        }
    } else if current != Some(&requested) {
        return Err("WEB_WORK_LANE_OWNER_CHANGED".into());
    }
    let next = Lane {
        schema_version: 1,
        owner: select.then_some(requested),
    };
    let bytes = serde_json::to_vec(&next).map_err(|_| "WEB_WORK_LANE_INVALID")?;
    write_work_state(&path(workspace), &bytes)?;
    if load(workspace)?.as_ref() != Some(&next) {
        return Err("WEB_WORK_LANE_READBACK_CHANGED".into());
    }
    Ok(
        json!({"status":if select {"ACTIVE"} else {"YIELDED"}, "projectId":project,
        "requestId":record.request_id,"taskRemainsOpen":true}),
    )
}
