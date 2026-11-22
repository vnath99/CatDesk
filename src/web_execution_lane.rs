//! Serial admission for linked Web tasks, stored beside their work
//! ledgers. Mutations share the existing registration lock with work closure.
use super::*;

const MAX_WAITING: usize = 128;

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

    fn valid(&self) -> bool {
        valid_request(&self.project_id)
            && valid_request(&self.request_id)
            && self.event_id == expected_event(&self.project_id, &self.request_id)
            && valid_request(&self.assignment.task_id)
            && sha256(&self.assignment.assignment_sha256)
            && sha256(&self.assignment.scope_sha256)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Lane {
    schema_version: u32,
    owner: Option<Owner>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    waiting: Vec<Owner>,
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
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WEB_WORK_LANE_UNAVAILABLE")?;
    if bytes.len() > MAX_BYTES {
        return Err("WEB_WORK_LANE_INVALID".into());
    }
    let lane: Lane = serde_json::from_slice(&bytes).map_err(|_| "WEB_WORK_LANE_INVALID")?;
    let mut events = std::collections::BTreeSet::new();
    if lane.schema_version != 1
        || lane.waiting.len() > MAX_WAITING
        || lane
            .owner
            .iter()
            .chain(&lane.waiting)
            .any(|owner| !owner.valid() || !events.insert(&owner.event_id))
    {
        return Err("WEB_WORK_LANE_INVALID".into());
    }
    Ok(Some(lane))
}

fn save(workspace: &Path, lane: &Lane) -> Result<(), String> {
    let bytes = serde_json::to_vec(lane).map_err(|_| "WEB_WORK_LANE_INVALID")?;
    write_work_state(&path(workspace), &bytes)?;
    if load(workspace)?.as_ref() != Some(lane) {
        return Err("WEB_WORK_LANE_READBACK_CHANGED".into());
    }
    Ok(())
}

fn next_waiting<'a>(workspace: &Path, waiting: &'a [Owner]) -> Result<Option<&'a Owner>, String> {
    for candidate in waiting {
        if candidate.open(workspace)? {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
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
        match next_waiting(workspace, &lane.waiting)? {
            Some(next) if next.project_id == project => {
                Ok(json!({"status":"NEXT_IN_QUEUE","requestId":next.request_id}))
            }
            Some(_) => Ok(json!({"status":"QUEUED_BEHIND_OTHER_PROJECT"})),
            None => Ok(json!({"status":"AVAILABLE_FOR_SELECTION"})),
        }
    })();
    observed.unwrap_or_else(|reason| json!({"status":"ATTENTION","reason":reason}))
}

/// Called only after exact effect authorization, under the existing project
/// lease and registration lock. Record contention without journaling a tool
/// call; the same call ID may be used after selection.
pub(crate) fn admit_owner(
    workspace: &Path,
    project: &str,
    record: &WorkRecord,
    check_fresh_call: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let Some(mut lane) = load(workspace)? else {
        return Ok(());
    };
    if record.phase != "OPEN" {
        return Err("WEB_WORK_REQUEST_CLOSED".into());
    }
    let requested = Owner::new(project, record)?;
    if lane.owner.as_ref() == Some(&requested) {
        return Ok(());
    }
    check_fresh_call()?;
    let occupied = match &lane.owner {
        Some(owner) => owner.open(workspace)?,
        None => false,
    };
    // A closed waiter is already settled by normal terminal resolution. An
    // absent or conflicting record fails closed instead of dropping its slot.
    let mut waiting = Vec::with_capacity(lane.waiting.len());
    for candidate in &lane.waiting {
        if candidate.open(workspace)? {
            waiting.push(candidate.clone());
        }
    }
    let can_enter = !occupied && waiting.first().is_none_or(|next| next == &requested);
    if can_enter {
        waiting.retain(|candidate| candidate != &requested);
        lane.owner = Some(requested);
    } else if !waiting.contains(&requested) {
        if waiting.len() == MAX_WAITING {
            return Err("WEB_WORK_LANE_QUEUE_FULL".into());
        }
        waiting.push(requested);
    } else if waiting == lane.waiting {
        return Err("WEB_WORK_EXECUTION_QUEUED".into());
    }
    lane.waiting = waiting;
    save(workspace, &lane)?;
    if can_enter {
        Ok(())
    } else {
        Err("WEB_WORK_EXECUTION_QUEUED".into())
    }
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
    let mut next = lane.unwrap_or(Lane {
        schema_version: 1,
        owner: None,
        waiting: vec![],
    });
    if select {
        next.waiting.retain(|candidate| candidate != &requested);
    }
    next.owner = select.then_some(requested);
    save(workspace, &next)?;
    Ok(
        json!({"status":if select {"ACTIVE"} else {"YIELDED"}, "projectId":project,
        "requestId":record.request_id,"taskRemainsOpen":true}),
    )
}
