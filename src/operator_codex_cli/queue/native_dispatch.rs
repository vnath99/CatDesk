//! Immutable task selection for the existing Web execution grant. These bytes
//! are not authorization by themselves; the Web key/journal admits dispatch.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NativeQueueSelection {
    pub(crate) task_id: String,
    pub(crate) binding_sha256: String,
    pub(crate) request_sha256: String,
    pub(crate) maximum_seconds: u64,
}

impl NativeQueueSelection {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !cfg!(windows) {
            return Err("WEB_TASK_NATIVE_PLATFORM_UNQUALIFIED".into());
        }
        valid_slug(&self.task_id)?;
        if !(60..=43_200).contains(&self.maximum_seconds)
            || [&self.binding_sha256, &self.request_sha256]
                .iter()
                .any(|v| !valid_sha256_hex(v) || **v != v.to_ascii_lowercase())
        {
            return Err("WEB_TASK_NATIVE_SELECTION_INVALID".into());
        }
        Ok(())
    }
}

pub(super) fn selected_store(
    _control: &Path,
    controller: &AutonomousProjectV1,
) -> Result<QueueStore, String> {
    #[cfg(all(test, windows))]
    if let Some((root, _)) = fixture_entry(_control) {
        return Ok(QueueStore { root });
    }
    QueueStore::open_read_only(controller)?
        .ok_or_else(|| "WEB_TASK_NATIVE_QUEUE_UNAVAILABLE".into())
}

pub(crate) fn freeze_native_queue_selection(
    control: &Path,
    project_id: &str,
    task_id: &str,
) -> Result<NativeQueueSelection, String> {
    if !cfg!(windows) {
        return Err("WEB_TASK_NATIVE_PLATFORM_UNQUALIFIED".into());
    }
    valid_slug(task_id)?;
    let controller = resolve_registered_project_from_control_root(control, "catdesk")
        .map_err(|_| "WEB_TASK_NATIVE_CONTROL_UNAVAILABLE")?;
    let project = resolve_registered_project_from_control_root(control, project_id)
        .map_err(|_| "WEB_TASK_NATIVE_PROJECT_UNAVAILABLE")?;
    let queue = selected_store(&controller.control_workspace, &controller.project)?;
    let _lock = queue.lock()?;
    let job = queue
        .load_job(project_id, task_id)?
        .ok_or("WEB_TASK_NATIVE_TASK_UNAVAILABLE")?;
    if job.state != QueueState::Ready || !job.binding.matches(&project.project) {
        return Err("WEB_TASK_NATIVE_TASK_NOT_READY_OR_CHANGED".into());
    }
    #[cfg(all(test, windows))]
    let maximum_seconds = if fixture_entry(control).is_some() {
        60
    } else {
        turn_timeout()?.as_secs()
    };
    #[cfg(not(all(test, windows)))]
    let maximum_seconds = turn_timeout()?.as_secs();
    Ok(NativeQueueSelection {
        task_id: task_id.into(),
        binding_sha256: project_binding_sha256(&project.project)?,
        request_sha256: job.request_sha256(),
        maximum_seconds,
    })
}

/// Metadata only. The caller retains the paired-target/registration transaction
/// and has revalidated this exact Git-prepared project/controller snapshot.
pub(crate) fn check_native_queue_selection_prepared(
    prepared: &crate::delegated::autonomy_projects::PreparedProjectScopeV1,
    selected: &NativeQueueSelection,
) -> Result<(), String> {
    selected.validate()?;
    let project = prepared.scope();
    let queue = selected_store(&project.control_workspace, prepared.controller())?;
    let _lock = queue.lock()?;
    if project_binding_sha256(&project.project)? != selected.binding_sha256 {
        return Err("CODEX_QUEUE_SCOPE: exact dispatch binding changed".into());
    }
    let scope = CodexScope {
        project_id: project.project.project_id.clone(),
        workspace: project.project.workspace.clone(),
    };
    dispatch_jobs(
        &queue,
        Some((
            &scope,
            &selected.task_id,
            &selected.binding_sha256,
            Some(&selected.request_sha256),
        )),
    )?;
    Ok(())
}

pub(crate) fn native_queue_selection_status(
    control: &Path,
    project_id: &str,
    selected: &NativeQueueSelection,
) -> Result<Value, String> {
    selected.validate()?;
    let controller = resolve_registered_project_from_control_root(control, "catdesk")
        .map_err(|_| "WEB_TASK_NATIVE_CONTROL_UNAVAILABLE")?;
    let project = resolve_registered_project_from_control_root(control, project_id)
        .map_err(|_| "WEB_TASK_NATIVE_PROJECT_UNAVAILABLE")?;
    let queue = selected_store(&controller.control_workspace, &controller.project)?;
    let (job, lineage) = super::native_continuation::follow(&queue, project_id, selected, None)?;
    let receipt = job
        .receipt_id
        .as_deref()
        .map(|id| -> Result<QueueReceipt, String> {
            let receipt = queue.read_receipt(id)?;
            if receipt.project_id != project_id
                || receipt.task_id != job.task_id
                || job
                    .attempt
                    .as_ref()
                    .is_none_or(|attempt| attempt.attempt_id != receipt.attempt_id)
            {
                return Err("WEB_TASK_NATIVE_RECEIPT_MISMATCH".into());
            }
            Ok(receipt)
        })
        .transpose()?;
    Ok(
        json!({"job":job.summary(),"receipt":receipt,"snapshotOnly":true,
        "originTaskId":selected.task_id,"lineage":lineage,
        "bindingStillMatches":job.binding.matches(&project.project),
        "originalBindingStillMatches":project_binding_sha256(&project.project)? == selected.binding_sha256,
        "processLiveness":"NOT_INFERRED_FROM_CLAIM_METADATA","taskCompletionVerified":false,
        "nativeTurnStarted":false,"journalChanged":false}),
    )
}

pub(crate) async fn dispatch_native_queue_selection(
    control: &Path,
    project_id: &str,
    selected: &NativeQueueSelection,
) -> Result<Value, String> {
    selected.validate()?;
    let controller = resolve_registered_project_from_control_root(control, "catdesk")
        .map_err(|_| "WEB_TASK_NATIVE_CONTROL_UNAVAILABLE")?;
    let project = resolve_registered_project_from_control_root(control, project_id)
        .map_err(|_| "WEB_TASK_NATIVE_PROJECT_UNAVAILABLE")?;
    let scope = CodexScope {
        project_id: project_id.into(),
        workspace: project.project.workspace,
    };
    let pins = Some((
        &scope,
        selected.task_id.as_str(),
        selected.binding_sha256.as_str(),
        Some(selected.request_sha256.as_str()),
    ));
    exact_dispatch_binding(control, pins)?;
    let queue = selected_store(&controller.control_workspace, &controller.project)?;
    let ready = {
        let _lock = queue.lock()?;
        dispatch_jobs(&queue, pins)?
            .iter()
            .any(|j| j.state == QueueState::Ready)
    };
    if !ready {
        return Ok(
            json!({"action":"codex_queue_dispatch","status":"no_ready_tasks","nativeTurnStarted":false}),
        );
    }
    #[cfg(all(test, windows))]
    let cli = if let Some((_, image)) = fixture_entry(control) {
        preflight_selected_cli(image, None, control).await
    } else {
        preflight_native_cli(control).await
    };
    #[cfg(not(all(test, windows)))]
    let cli = preflight_native_cli(control).await;
    let cli = match cli {
        Ok(cli) => cli,
        Err(reason) => {
            return Ok(json!({"action":"codex_queue_dispatch",
            "status":"prelaunch_refused","reason":reason,"nativeTurnStarted":false}));
        }
    };
    let Some(job) = claim_selected(control, &queue, pins)? else {
        return Ok(
            json!({"action":"codex_queue_dispatch","status":"no_ready_tasks","nativeTurnStarted":false}),
        );
    };
    let turn = run_queued_turn(
        &job,
        control,
        &cli,
        Duration::from_secs(selected.maximum_seconds),
    )
    .await
    .with_cli(&cli);
    complete_attempt(control, &queue, &job, turn)
}

#[cfg(all(test, windows))]
type FixtureMap = std::collections::BTreeMap<PathBuf, (PathBuf, PathBuf)>;
#[cfg(all(test, windows))]
static FIXTURES: std::sync::OnceLock<std::sync::Mutex<FixtureMap>> = std::sync::OnceLock::new();
#[cfg(all(test, windows))]
fn fixture_entry(control: &Path) -> Option<(PathBuf, PathBuf)> {
    FIXTURES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .get(control)
        .cloned()
}
#[cfg(all(test, windows))]
pub(crate) struct NativeQueueFixture {
    control: PathBuf,
    owns_map: bool,
}
#[cfg(all(test, windows))]
impl NativeQueueFixture {
    pub(crate) fn image(&self) -> PathBuf {
        fixture_entry(&self.control).unwrap().1
    }
    pub(crate) fn job_file(&self, project_id: &str) -> PathBuf {
        let root = fixture_entry(&self.control).unwrap().0;
        QueueStore { root }.job_path(project_id, "native-fixture")
    }
}
#[cfg(all(test, windows))]
impl Drop for NativeQueueFixture {
    fn drop(&mut self) {
        if self.owns_map {
            FIXTURES
                .get()
                .unwrap()
                .lock()
                .unwrap()
                .remove(&self.control);
        }
    }
}
#[cfg(all(test, windows))]
pub(crate) fn native_queue_fixture(control: &Path, project_id: &str) -> NativeQueueFixture {
    let control = control.canonicalize().unwrap();
    let project = resolve_registered_project_from_control_root(&control, project_id).unwrap();
    let (queue, owns_map) = if let Some((root, _)) = fixture_entry(&control) {
        (QueueStore { root }, false)
    } else {
        let base = control
            .parent()
            .unwrap()
            .join(format!("native-queue-{}", Uuid::new_v4()));
        fs::create_dir(&base).unwrap();
        let queue = QueueStore::open_at(base.join("queue")).unwrap();
        let cli = super::tests::fake_native_cli(&base);
        assert!(
            FIXTURES
                .get_or_init(Default::default)
                .lock()
                .unwrap()
                .insert(control.clone(), (queue.root.clone(), cli))
                .is_none()
        );
        (queue, true)
    };
    let job = QueueJob::new(
        "native-fixture".into(),
        &project.project,
        "native fixture task".into(),
        None,
        None,
    )
    .unwrap();
    queue.save_job(&job).unwrap();
    NativeQueueFixture { control, owns_map }
}
