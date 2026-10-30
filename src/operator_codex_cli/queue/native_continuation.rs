//! A proved Web continuation can carry unexecuted native work. Keep the source
//! job/receipts intact. Prepare a dormant child, pause/link its parent, then
//! activate the child. A missing committed child is evidence loss, not resend
//! authority. No extra authorization store, browser action or native launch.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativeContinuationLink {
    origin_task_id: String,
    origin_request_sha256: String,
    source_event_id: String,
    target_generation: u64,
    target_sha256: String,
    task_id: String,
    request_sha256: String,
    created_at_unix: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativeContinuationOrigin {
    parent_task_id: String,
    parent_request_sha256: String,
    link: NativeContinuationLink,
    pub(super) activated: bool,
}

fn child_id(parent: &str, link: &NativeContinuationLink) -> String {
    let bytes = serde_json::to_vec(&(
        "native-continuation-v1",
        parent,
        &link.origin_task_id,
        &link.origin_request_sha256,
        &link.source_event_id,
        link.target_generation,
        &link.target_sha256,
    ))
    .expect("native continuation identity");
    format!("native-next-{:x}", Sha256::digest(bytes))
}

impl NativeContinuationLink {
    fn valid_fields(&self, parent_id: &str) -> bool {
        valid_slug(&self.origin_task_id).is_ok()
            && valid_slug(parent_id).is_ok()
            && catdesk_wake::protocol::valid_id(&self.source_event_id)
            && self.source_event_id.len() <= 128
            && self.target_generation > 0
            && self.created_at_unix > 0
            && self.task_id == child_id(parent_id, self)
            && [
                &self.origin_request_sha256,
                &self.target_sha256,
                &self.request_sha256,
            ]
            .iter()
            .all(|hash| valid_sha256_hex(hash) && **hash == hash.to_ascii_lowercase())
    }
    pub(super) fn validate(&self, parent: &QueueJob) -> Result<(), String> {
        if parent.state != QueueState::Paused
            || !self.valid_fields(&parent.task_id)
            || dormant_child(parent, self).request_sha256() != self.request_sha256
        {
            return Err("WEB_TASK_NATIVE_CONTINUATION_INVALID".into());
        }
        Ok(())
    }
}

impl NativeContinuationOrigin {
    pub(super) fn validate(&self, child: &QueueJob) -> Result<(), String> {
        if !self.link.valid_fields(&self.parent_task_id)
            || !valid_sha256_hex(&self.parent_request_sha256)
            || self.link.task_id != child.task_id
            || self.link.request_sha256 != child.request_sha256()
            || !self.activated && (child.state != QueueState::Paused || !empty_effects(child))
        {
            return Err("WEB_TASK_NATIVE_CONTINUATION_INVALID".into());
        }
        Ok(())
    }
}

fn empty_effects(job: &QueueJob) -> bool {
    job.attempt.is_none()
        && job.receipt_id.is_none()
        && job.review_candidate_sha256.is_none()
        && job.verification_sha256.is_none()
        && job.review_package_sha256.is_none()
        && job.advisory_outcome_sha256.is_none()
}

fn no_native_effects(queue: &QueueStore, job: &QueueJob) -> Result<(), String> {
    if !matches!(job.state, QueueState::Ready | QueueState::Paused)
        || job.review_candidate_sha256.is_some()
        || job.verification_sha256.is_some()
        || job.review_package_sha256.is_some()
        || job.advisory_outcome_sha256.is_some()
    {
        return Err("WEB_TASK_NATIVE_CONTINUATION_EFFECTS_UNPROVEN".into());
    }
    if empty_effects(job) {
        return Ok(());
    }
    let attempt = job
        .attempt
        .as_ref()
        .ok_or("WEB_TASK_NATIVE_CONTINUATION_EFFECTS_UNPROVEN")?;
    if !attempt.marker_binding_is_valid() || job.receipt_id.as_deref() != Some(&attempt.attempt_id)
    {
        return Err("WEB_TASK_NATIVE_CONTINUATION_EFFECTS_UNPROVEN".into());
    }
    let receipt = queue.read_receipt(&attempt.attempt_id)?;
    if receipt.project_id != job.binding.project_id
        || receipt.task_id != job.task_id
        || receipt.attempt_id != attempt.attempt_id
        || receipt.evidence_level != "CHILD_ABSENT_BEFORE_SPAWN"
        || receipt.child_exit_observed
        || receipt.exit_code.is_some()
        || !receipt.binding_still_matches
    {
        return Err("WEB_TASK_NATIVE_CONTINUATION_EFFECTS_UNPROVEN".into());
    }
    Ok(())
}

fn dormant_child(parent: &QueueJob, link: &NativeContinuationLink) -> QueueJob {
    let mut child = parent.clone();
    child.task_id = link.task_id.clone();
    child.binding.target_sha256 = Some(link.target_sha256.clone());
    child.state = QueueState::Paused;
    child.created_at_unix = link.created_at_unix;
    child.attempt = None;
    child.receipt_id = None;
    child.review_candidate_sha256 = None;
    child.verification_sha256 = None;
    child.review_package_sha256 = None;
    child.advisory_outcome_sha256 = None;
    child.native_continuation = None;
    child.native_origin = None;
    child.native_origin = Some(NativeContinuationOrigin {
        parent_task_id: parent.task_id.clone(),
        parent_request_sha256: parent.request_sha256(),
        link: link.clone(),
        activated: false,
    });
    child
}

fn check_link(
    queue: &QueueStore,
    parent: &QueueJob,
    link: &NativeContinuationLink,
    root: &native_dispatch::NativeQueueSelection,
    source_event: Option<&str>,
) -> Result<QueueJob, String> {
    link.validate(parent)?;
    no_native_effects(queue, parent)?;
    if link.origin_task_id != root.task_id
        || link.origin_request_sha256 != root.request_sha256
        || source_event.is_some_and(|event| event != link.source_event_id)
    {
        return Err("WEB_TASK_NATIVE_CONTINUATION_SCOPE_CHANGED".into());
    }
    let child = queue
        .load_job(&parent.binding.project_id, &link.task_id)?
        .ok_or("WEB_TASK_NATIVE_CONTINUATION_RECORD_MISSING_NO_REPLAY")?;
    let expected = dormant_child(parent, link);
    if child.request_sha256() != expected.request_sha256()
        || child.native_origin.as_ref().is_none_or(|origin| {
            let mut expected_origin = expected.native_origin.clone().unwrap();
            expected_origin.activated = origin.activated;
            *origin != expected_origin
        })
    {
        return Err("WEB_TASK_NATIVE_CONTINUATION_SCOPE_CHANGED".into());
    }
    Ok(child)
}

pub(super) fn follow(
    queue: &QueueStore,
    project: &str,
    root: &native_dispatch::NativeQueueSelection,
    source_event: Option<&str>,
) -> Result<(QueueJob, Vec<String>), String> {
    let mut job = queue
        .load_job(project, &root.task_id)?
        .ok_or("WEB_TASK_NATIVE_TASK_UNAVAILABLE")?;
    if job.request_sha256() != root.request_sha256 {
        return Err("WEB_TASK_NATIVE_REQUEST_CHANGED".into());
    }
    let mut lineage = Vec::new();
    for _ in 0..64 {
        if lineage.contains(&job.task_id) {
            break;
        }
        lineage.push(job.task_id.clone());
        let Some(link) = job.native_continuation.as_ref() else {
            return Ok((job, lineage));
        };
        job = check_link(queue, &job, link, root, source_event)?;
    }
    Err("WEB_TASK_NATIVE_CONTINUATION_CHAIN_INVALID".into())
}

fn has_capacity(queue: &QueueStore) -> Result<(), String> {
    let mut records = 0;
    for (index, entry) in fs::read_dir(queue.root.join("jobs"))
        .map_err(|_| "CODEX_QUEUE: jobs unavailable")?
        .enumerate()
    {
        let entry = entry.map_err(|_| "CODEX_QUEUE: jobs unavailable")?;
        if index >= MAX_DIR_ENTRIES {
            return Err("CODEX_QUEUE: directory entry limit reached".into());
        }
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            records += 1;
        }
    }
    if records >= MAX_JOBS {
        return Err("WEB_TASK_NATIVE_CONTINUATION_CAPACITY".into());
    }
    Ok(())
}

pub(crate) fn continue_native_queue_selection(
    control: &Path,
    project_id: &str,
    root: &native_dispatch::NativeQueueSelection,
    lease: &crate::operator_facade::WebWorkLease,
    original: &catdesk_wake::protocol::Target,
    current: &catdesk_wake::protocol::Target,
    source_event: &str,
) -> Result<native_dispatch::NativeQueueSelection, String> {
    continue_selection(
        control,
        project_id,
        root,
        ContinuationProof {
            lease,
            original,
            current,
            source_event,
        },
        |_| Ok(()),
    )
}

struct ContinuationProof<'a> {
    lease: &'a crate::operator_facade::WebWorkLease,
    original: &'a catdesk_wake::protocol::Target,
    current: &'a catdesk_wake::protocol::Target,
    source_event: &'a str,
}

fn continue_selection(
    control: &Path,
    project_id: &str,
    root: &native_dispatch::NativeQueueSelection,
    proof: ContinuationProof<'_>,
    checkpoint: impl Fn(&str) -> Result<(), String>,
) -> Result<native_dispatch::NativeQueueSelection, String> {
    let ContinuationProof {
        lease,
        original,
        current,
        source_event,
    } = proof;
    root.validate()?;
    let controller = resolve_registered_project_from_control_root(control, "catdesk")
        .map_err(|_| "WEB_TASK_NATIVE_CONTROL_UNAVAILABLE")?;
    let project = resolve_registered_project_from_control_root(control, project_id)
        .map_err(|_| "WEB_TASK_NATIVE_PROJECT_UNAVAILABLE")?;
    let queue =
        native_dispatch::selected_store(&controller.control_workspace, &controller.project)?;
    let _lock = queue.lock()?;
    let (mut job, _) = follow(&queue, project_id, root, Some(source_event))?;
    let current_hash = project_binding_sha256(&project.project)?;
    let needs_continuation = root.binding_sha256 != current_hash || job.task_id != root.task_id;
    if needs_continuation {
        lease.check_native_continuation(control, project_id, original, current, source_event)?;
    }
    if project.project.chatgpt_target_sha256.as_deref() != Some(&current.digest)
        || project.project.chatgpt_target_url.as_deref() != Some(&current.url)
    {
        return Err("WEB_TASK_NATIVE_CONTINUATION_SCOPE_CHANGED".into());
    }
    let mut native_scope = job.binding.clone();
    native_scope.target_sha256 = project.project.chatgpt_target_sha256.clone();
    if native_scope != QueueBinding::from_project(&project.project)? {
        return Err("WEB_TASK_NATIVE_CONTINUATION_SCOPE_CHANGED".into());
    }
    if job.binding.target_sha256 != Some(current.digest.clone())
        && (job.state == QueueState::Ready
            || job.native_origin.as_ref().is_some_and(|o| !o.activated))
    {
        lease.check_native_continuation(control, project_id, original, current, source_event)?;
        no_native_effects(&queue, &job)?;
        let mut link = NativeContinuationLink {
            origin_task_id: root.task_id.clone(),
            origin_request_sha256: root.request_sha256.clone(),
            source_event_id: source_event.into(),
            target_generation: current.generation,
            target_sha256: current.digest.clone(),
            task_id: String::new(),
            request_sha256: String::new(),
            created_at_unix: unix_now()?,
        };
        link.task_id = child_id(&job.task_id, &link);
        let existing = queue.load_job(project_id, &link.task_id)?;
        if let Some(child) = &existing {
            link.created_at_unix = child.created_at_unix;
        }
        link.request_sha256 = dormant_child(&job, &link).request_sha256();
        let child = dormant_child(&job, &link);
        if let Some(existing) = existing {
            if existing != child {
                return Err("WEB_TASK_NATIVE_CONTINUATION_CONFLICT".into());
            }
        } else {
            has_capacity(&queue)?;
            queue.save_job(&child)?; // Dormant: no controller may execute it yet.
        }
        checkpoint("DORMANT_CHILD_SAVED")?;
        job.state = QueueState::Paused;
        job.native_continuation = Some(link.clone());
        queue.save_job(&job)?;
        checkpoint("PARENT_LINK_SAVED")?;
        job = check_link(&queue, &job, &link, root, Some(source_event))?;
    }
    if job.native_origin.as_ref().is_some_and(|o| !o.activated) && job.native_continuation.is_none()
    {
        lease.check_native_continuation(control, project_id, original, current, source_event)?;
        no_native_effects(&queue, &job)?;
        if job.binding.target_sha256.as_deref() != Some(&current.digest) {
            return Err("WEB_TASK_NATIVE_CONTINUATION_SCOPE_CHANGED".into());
        }
        job.native_origin.as_mut().unwrap().activated = true;
        job.state = QueueState::Ready;
        queue.save_job(&job)?;
        checkpoint("CHILD_ACTIVATED")?;
    }
    let request_sha256 = job.request_sha256();
    Ok(native_dispatch::NativeQueueSelection {
        task_id: job.task_id,
        binding_sha256: current_hash,
        request_sha256,
        maximum_seconds: root.maximum_seconds,
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use catdesk_wake::{protocol::Target, store::Store};
    use native_dispatch::{NativeQueueFixture, NativeQueueSelection};

    struct Fixture {
        control: PathBuf,
        wake: Store,
        event: String,
        selected: NativeQueueSelection,
        _native: NativeQueueFixture,
    }
    impl Fixture {
        fn new() -> Self {
            let (control, _, wake, _, event) =
                crate::web_work::tests::assignment_fixture_for("alpha");
            let path = control.join(".catdesk/projects/projects.json");
            let mut registry: crate::delegated::autonomy_projects::AutonomousProjectRegistryV1 =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            registry
                .projects
                .iter_mut()
                .find(|p| p.project_id == "alpha")
                .unwrap()
                .codex_thread_id = Some(Uuid::new_v4().to_string());
            fs::write(path, serde_json::to_vec(&registry).unwrap()).unwrap();
            let native = native_dispatch::native_queue_fixture(&control, "alpha");
            let selected =
                native_dispatch::freeze_native_queue_selection(&control, "alpha", "native-fixture")
                    .unwrap();
            Self {
                control,
                wake,
                event: event.event_id,
                selected,
                _native: native,
            }
        }
        fn queue(&self) -> QueueStore {
            let controller =
                resolve_registered_project_from_control_root(&self.control, "catdesk").unwrap();
            native_dispatch::selected_store(&self.control, &controller.project).unwrap()
        }
        fn rollover(&self) -> (Target, Target) {
            crate::operator_facade::native_continuation_fixture(
                &self.control,
                &self.wake,
                "alpha",
                &self.event,
            )
        }
        fn resume(
            &self,
            source: &Target,
            current: &Target,
            stop: Option<&str>,
        ) -> Result<NativeQueueSelection, String> {
            let lease = crate::operator_facade::acquire_web_work_lease(&self.control, "alpha")?;
            continue_selection(
                &self.control,
                "alpha",
                &self.selected,
                ContinuationProof {
                    lease: &lease,
                    original: source,
                    current,
                    source_event: &self.event,
                },
                |phase| {
                    if Some(phase) == stop {
                        Err("INJECTED_METADATA_INTERRUPTION".into())
                    } else {
                        Ok(())
                    }
                },
            )
        }
    }

    #[test]
    fn native_continuation_recovers_each_metadata_boundary_once() {
        for phase in [
            "DORMANT_CHILD_SAVED",
            "PARENT_LINK_SAVED",
            "CHILD_ACTIVATED",
        ] {
            let f = Fixture::new();
            let original = f
                .queue()
                .load_job("alpha", &f.selected.task_id)
                .unwrap()
                .unwrap();
            let (source, current) = f.rollover();
            assert_eq!(
                f.resume(&source, &current, Some(phase)).unwrap_err(),
                "INJECTED_METADATA_INTERRUPTION"
            );
            let cold = f.queue();
            let pending = cold.jobs().unwrap();
            assert_eq!(pending.len(), 2);
            let child = pending
                .iter()
                .find(|j| j.task_id != original.task_id)
                .unwrap();
            assert_eq!(
                child.state,
                if phase == "CHILD_ACTIVATED" {
                    QueueState::Ready
                } else {
                    QueueState::Paused
                }
            );
            let selected = f.resume(&source, &current, None).unwrap();
            assert_eq!(selected, f.resume(&source, &current, None).unwrap());
            let (leaf, lineage) = follow(&cold, "alpha", &f.selected, Some(&f.event)).unwrap();
            assert_eq!(
                lineage,
                vec![original.task_id.clone(), selected.task_id.clone()]
            );
            assert_eq!(leaf.state, QueueState::Ready);
            assert_eq!(leaf.prompt, original.prompt);
            assert_eq!(leaf.model, original.model);
            assert_eq!(leaf.effort, original.effort);
            assert_eq!(leaf.windows_sandbox, original.windows_sandbox);
            assert_eq!(leaf.binding.workspace, original.binding.workspace);
            assert_eq!(leaf.binding.thread_id, original.binding.thread_id);
            assert_eq!(leaf.binding.target_sha256, Some(current.digest));
            assert_eq!(selected.maximum_seconds, f.selected.maximum_seconds);
            let parent = cold.load_job("alpha", &original.task_id).unwrap().unwrap();
            assert_eq!(parent.state, QueueState::Paused);
            assert_eq!(parent.request_sha256(), original.request_sha256());
            assert_eq!(cold.jobs().unwrap().len(), 2);
        }
    }

    #[test]
    fn native_continuation_missing_committed_child_never_recreates_it() {
        let f = Fixture::new();
        let (source, current) = f.rollover();
        assert!(
            f.resume(&source, &current, Some("PARENT_LINK_SAVED"))
                .is_err()
        );
        let queue = f.queue();
        let parent = queue
            .load_job("alpha", &f.selected.task_id)
            .unwrap()
            .unwrap();
        let child = &parent.native_continuation.as_ref().unwrap().task_id;
        fs::remove_file(queue.job_path("alpha", child)).unwrap();
        assert_eq!(
            f.resume(&source, &current, None).unwrap_err(),
            "WEB_TASK_NATIVE_CONTINUATION_RECORD_MISSING_NO_REPLAY"
        );
        assert!(queue.load_job("alpha", child).unwrap().is_none());
    }

    #[test]
    fn native_continuation_refuses_changed_native_scope_and_corrupt_dormant_child() {
        let f = Fixture::new();
        let (source, current) = f.rollover();
        let queue = f.queue();
        let before = fs::read(queue.job_path("alpha", &f.selected.task_id)).unwrap();
        assert!(
            f.resume(&source, &current, Some("DORMANT_CHILD_SAVED"))
                .is_err()
        );
        let child = queue
            .jobs()
            .unwrap()
            .into_iter()
            .find(|j| j.task_id != f.selected.task_id)
            .unwrap();
        let child_path = queue.job_path("alpha", &child.task_id);
        let mut invalid: Value = serde_json::from_slice(&fs::read(&child_path).unwrap()).unwrap();
        invalid["nativeOrigin"]["link"]["sourceEventId"] = json!("foreign-source-event");
        let invalid = serde_json::to_vec(&invalid).unwrap();
        fs::write(&child_path, &invalid).unwrap();
        assert!(f.resume(&source, &current, None).is_err());
        assert_eq!(fs::read(&child_path).unwrap(), invalid);
        assert_eq!(
            fs::read(queue.job_path("alpha", &f.selected.task_id)).unwrap(),
            before
        );

        let f = Fixture::new();
        let (source, current) = f.rollover();
        let lease = crate::operator_facade::acquire_web_work_lease(&f.control, "alpha").unwrap();
        let path = f.control.join(".catdesk/projects/projects.json");
        let mut registry: crate::delegated::autonomy_projects::AutonomousProjectRegistryV1 =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        registry
            .projects
            .iter_mut()
            .find(|p| p.project_id == "alpha")
            .unwrap()
            .codex_thread_id = Some(Uuid::new_v4().to_string());
        fs::write(path, serde_json::to_vec(&registry).unwrap()).unwrap();
        assert_eq!(
            continue_native_queue_selection(
                &f.control,
                "alpha",
                &f.selected,
                &lease,
                &source,
                &current,
                &f.event
            )
            .unwrap_err(),
            "WEB_TASK_NATIVE_CONTINUATION_SCOPE_CHANGED"
        );
        assert_eq!(f.queue().jobs().unwrap().len(), 1);
    }

    #[test]
    fn native_continuation_preserves_proved_no_child_receipt_and_rejects_active_claim() {
        let f = Fixture::new();
        let queue = f.queue();
        let project = resolve_registered_project_from_control_root(&f.control, "alpha").unwrap();
        let scope = CodexScope {
            project_id: "alpha".into(),
            workspace: project.project.workspace,
        };
        let pins = Some((
            &scope,
            f.selected.task_id.as_str(),
            f.selected.binding_sha256.as_str(),
            Some(f.selected.request_sha256.as_str()),
        ));
        let claimed = claim_selected(&f.control, &queue, pins).unwrap().unwrap();
        let id = claimed.attempt.as_ref().unwrap().attempt_id.clone();
        let before_claim = fs::read(queue.job_path("alpha", &claimed.task_id)).unwrap();
        let (source, current) = f.rollover();
        let unchanged = f.resume(&source, &current, None).unwrap();
        assert_eq!(unchanged.task_id, f.selected.task_id);
        assert_eq!(
            fs::read(queue.job_path("alpha", &claimed.task_id)).unwrap(),
            before_claim
        );
        // Finish the simulated claim against its original target: no child
        // existed. Binding changed meanwhile, so recovery cannot infer that
        // the old receipt was a valid prelaunch refusal for the new target.
        complete_attempt(
            &f.control,
            &queue,
            &claimed,
            TurnResult::before_spawn("FIXTURE_NO_CHILD"),
        )
        .unwrap();
        assert_eq!(
            f.resume(&source, &current, None).unwrap().task_id,
            f.selected.task_id
        );
        assert_eq!(queue.jobs().unwrap().len(), 1);
        assert!(!queue.read_receipt(&id).unwrap().binding_still_matches);
        let mut job = queue
            .load_job("alpha", &f.selected.task_id)
            .unwrap()
            .unwrap();
        job.state = QueueState::Ready; // An operator state edit is not no-effect proof.
        queue.save_job(&job).unwrap();
        assert!(f.resume(&source, &current, None).is_err());
        assert_eq!(queue.jobs().unwrap().len(), 1);

        let f = Fixture::new();
        let queue = f.queue();
        let project = resolve_registered_project_from_control_root(&f.control, "alpha").unwrap();
        let scope = CodexScope {
            project_id: "alpha".into(),
            workspace: project.project.workspace,
        };
        let claimed = claim_selected(
            &f.control,
            &queue,
            Some((
                &scope,
                f.selected.task_id.as_str(),
                f.selected.binding_sha256.as_str(),
                Some(f.selected.request_sha256.as_str()),
            )),
        )
        .unwrap()
        .unwrap();
        let id = claimed.attempt.as_ref().unwrap().attempt_id.clone();
        complete_attempt(
            &f.control,
            &queue,
            &claimed,
            TurnResult::before_spawn("FIXTURE_NO_CHILD"),
        )
        .unwrap();
        let receipt_path = queue.root.join("receipts").join(format!("{id}.json"));
        let original_receipt = fs::read(&receipt_path).unwrap();
        assert!(queue.read_receipt(&id).unwrap().binding_still_matches);
        let (source, current) = f.rollover();
        let selected = f.resume(&source, &current, None).unwrap();
        assert_ne!(selected.task_id, f.selected.task_id);
        assert_eq!(fs::read(&receipt_path).unwrap(), original_receipt);
        assert_eq!(queue.read_receipt(&id).unwrap().task_id, f.selected.task_id);
        assert!(
            queue
                .load_job("alpha", &selected.task_id)
                .unwrap()
                .unwrap()
                .attempt
                .is_none()
        );
    }

    #[test]
    fn native_continuation_terminal_and_operator_paused_jobs_remain_unchanged() {
        for state in [
            QueueState::Paused,
            QueueState::NativeExitedZero,
            QueueState::Failed,
            QueueState::Unknown,
            QueueState::Attention,
        ] {
            let f = Fixture::new();
            let queue = f.queue();
            let mut job = queue
                .load_job("alpha", &f.selected.task_id)
                .unwrap()
                .unwrap();
            job.state = state;
            queue.save_job(&job).unwrap();
            let before = fs::read(queue.job_path("alpha", &job.task_id)).unwrap();
            let (source, current) = f.rollover();
            let effective = f.resume(&source, &current, None).unwrap();
            assert_eq!(effective.task_id, f.selected.task_id);
            assert_eq!(
                fs::read(queue.job_path("alpha", &job.task_id)).unwrap(),
                before
            );
            assert_eq!(queue.jobs().unwrap().len(), 1);
        }
    }

    #[test]
    fn native_continuation_requires_durable_proof_and_exact_source_scope() {
        let f = Fixture::new();
        let source = f.wake.config().unwrap().targets["alpha"].clone();
        let current = f
            .wake
            .set_target(
                "alpha",
                source.generation,
                "https://chatgpt.com/c/manual-fixture-target",
            )
            .unwrap();
        let path = f.control.join(".catdesk/projects/projects.json");
        let mut registry: crate::delegated::autonomy_projects::AutonomousProjectRegistryV1 =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let p = registry
            .projects
            .iter_mut()
            .find(|p| p.project_id == "alpha")
            .unwrap();
        p.chatgpt_target_url = Some(current.url.clone());
        p.chatgpt_target_sha256 = Some(current.digest.clone());
        p.chatgpt_target_history.push(current.url.clone());
        fs::write(path, serde_json::to_vec(&registry).unwrap()).unwrap();
        let before = f.queue().jobs().unwrap();
        assert_eq!(
            f.resume(&source, &current, None).unwrap_err(),
            "WEB_WORK_CONTINUATION_SOURCE_REQUIRED"
        );
        assert_eq!(f.queue().jobs().unwrap(), before);
        let (source, current) = f.rollover();
        let lease = crate::operator_facade::acquire_web_work_lease(&f.control, "alpha").unwrap();
        assert!(
            continue_native_queue_selection(
                &f.control,
                "alpha",
                &f.selected,
                &lease,
                &source,
                &current,
                "wrong-event"
            )
            .is_err()
        );
        assert_eq!(f.queue().jobs().unwrap(), before);
    }

    #[test]
    fn native_continuation_follows_multiple_rollovers_and_dormant_interruption() {
        let f = Fixture::new();
        let (original, first) = f.rollover();
        assert!(
            f.resume(&original, &first, Some("PARENT_LINK_SAVED"))
                .is_err()
        );
        let (_, second) = f.rollover();
        let selected = f.resume(&original, &second, None).unwrap();
        let (leaf, lineage) = follow(&f.queue(), "alpha", &f.selected, Some(&f.event)).unwrap();
        assert_eq!(lineage.len(), 3);
        assert_eq!(leaf.task_id, selected.task_id);
        assert_eq!(leaf.binding.target_sha256, Some(second.digest));
        assert_eq!(
            f.queue()
                .jobs()
                .unwrap()
                .iter()
                .filter(|j| j.state == QueueState::Ready)
                .count(),
            1
        );
        assert_eq!(
            f.queue()
                .jobs()
                .unwrap()
                .iter()
                .filter(|j| j.native_origin.as_ref().is_some_and(|o| !o.activated))
                .count(),
            1
        );
        assert!(follow(&f.queue(), "alpha", &f.selected, Some("foreign-event")).is_err());
    }
}
