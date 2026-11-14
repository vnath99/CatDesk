//! First conversation in the existing project lock, browser admission and Wake CAS.
//! It is not rollover and cannot supply predecessor or native execution authority.
use super::*;

const PURPOSE: &str = "INITIAL_PROJECT_CONVERSATION";
const FILE: &str = "initial-conversation.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InitialContext {
    schema_version: u32,
    project_id: String,
    task_id: String,
    objective: String,
    #[serde(default)]
    verified_accomplishments: Vec<String>,
    #[serde(default)]
    decisions: Vec<String>,
    #[serde(default)]
    blockers: Vec<String>,
    next_actions: Vec<String>,
    #[serde(default)]
    owned_paths: Vec<String>,
    #[serde(default)]
    declared_source_commit: Option<String>,
}

impl InitialContext {
    fn validate(&self, project: &str) -> Result<(), String> {
        let text = |value: &str| {
            !value.trim().is_empty()
                && value.len() <= 1024
                && !value
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        };
        if self.schema_version != 1
            || self.project_id != project
            || !valid_id(&self.task_id)
            || !text(&self.objective)
            || self.next_actions.is_empty()
            || [
                &self.verified_accomplishments,
                &self.decisions,
                &self.blockers,
                &self.next_actions,
                &self.owned_paths,
            ]
            .iter()
            .any(|values| values.len() > 16 || values.iter().any(|value| !text(value)))
            || self.declared_source_commit.as_ref().is_some_and(|value| {
                value.len() != 40
                    || !value
                        .bytes()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            })
            || serde_json::to_vec(self)
                .map_err(|_| "WEB_INITIAL_CONTEXT_INVALID")?
                .len()
                > 4096
        {
            return Err("WEB_INITIAL_CONTEXT_INVALID".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InitialConversation {
    schema_version: u32,
    purpose: String,
    project_id: String,
    intent_id: String,
    registration: AutonomousProjectV1,
    registration_digest: String,
    context: InitialContext,
    phase: Phase,
    progress: Option<runtime::BootstrapProgress>,
    message_digest: String,
    receipt: Option<runtime::BootstrapReceipt>,
    created_utc: u64,
}

fn registration_digest(value: &AutonomousProjectV1) -> Result<String, String> {
    serde_json::to_string(value)
        .map(|value| digest(&value))
        .map_err(|_| "WEB_INITIAL_REGISTRATION_INVALID".into())
}

impl InitialConversation {
    fn handoff(&self) -> Result<String, String> {
        Ok(format!(
            "{}@CatDesk-Local-Tunnel-v5\n{}",
            runtime::APP_JSON_BOOTSTRAP_HANDOFF_PREFIX,
            serde_json::to_string(&json!({"schemaVersion":1,"purpose":PURPOSE,
                "projectId":self.project_id,"workspace":self.registration.workspace,
                "nativeThreadId":self.registration.codex_thread_id,
                "verificationProfile":self.registration.verification_profile,
                "context":self.context,"authority":"CONTEXT_ONLY_WAIT_FOR_EXPLICIT_TASK_GRANT"}))
            .map_err(|_| "WEB_INITIAL_CONTEXT_INVALID")?
        ))
    }

    fn identity(&self) -> admission::Identity {
        admission::Identity {
            purpose: admission::Purpose::InitialConversation,
            project_id: self.project_id.clone(),
            intent_id: self.intent_id.clone(),
            source_generation: 0,
            source_digest: self.registration_digest.clone(),
            message_digest: self.message_digest.clone(),
        }
    }

    fn receipt_target(&self, completion: &str) -> Result<Target, String> {
        let receipt = self
            .receipt
            .as_ref()
            .ok_or("WEB_INITIAL_RECEIPT_REQUIRED")?;
        runtime::validate_initialization_receipt(
            receipt,
            &self.project_id,
            &self.intent_id,
            &self.registration_digest,
            &self.handoff()?,
            completion,
        )?;
        Target::new(1, receipt.destination_url.clone())
    }

    fn validate(&self, project: &str) -> Result<(), String> {
        self.context.validate(project)?;
        if self.schema_version != 1
            || self.purpose != PURPOSE
            || self.project_id != project
            || !valid_id(&self.intent_id)
            || self.registration.project_id != project
            || self.registration.chatgpt_target_url.is_some()
            || self.registration.chatgpt_target_sha256.is_some()
            || !self.registration.chatgpt_target_history.is_empty()
            || self.created_utc == 0
            || registration_digest(&self.registration)? != self.registration_digest
            || runtime::initialization_message_digest(
                project,
                &self.intent_id,
                &self.registration_digest,
                &self.handoff()?,
            )? != self.message_digest
        {
            return Err("WEB_INITIAL_JOURNAL_INVALID".into());
        }
        match self.phase {
            Phase::Prepared if self.progress.is_none() && self.receipt.is_none() => {}
            Phase::Submitting if self.progress.is_some() && self.receipt.is_none() => {}
            Phase::ResponsePending
                if self.progress == Some(runtime::BootstrapProgress::SendAttempted) =>
            {
                self.receipt_target("RESPONSE_PENDING")?;
            }
            Phase::Created | Phase::Bound
                if self.progress == Some(runtime::BootstrapProgress::SendAttempted) =>
            {
                self.receipt_target("RESPONSE_COMPLETED")?;
            }
            _ => return Err("WEB_INITIAL_JOURNAL_INVALID".into()),
        }
        Ok(())
    }
}

fn load(store: &ProjectStore) -> Result<Option<InitialConversation>, String> {
    let path = store.root.join(FILE);
    if !path
        .try_exists()
        .map_err(|_| "WEB_INITIAL_JOURNAL_UNAVAILABLE")?
    {
        return Ok(None);
    }
    plain(&path, false)?;
    if fs::metadata(&path)
        .map_err(|_| "WEB_INITIAL_JOURNAL_UNAVAILABLE")?
        .len()
        > 24 * 1024
    {
        return Err("WEB_INITIAL_JOURNAL_INVALID".into());
    }
    let value: InitialConversation = read(&path).map_err(|_| "WEB_INITIAL_JOURNAL_INVALID")?;
    value.validate(
        store
            .root
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or("WEB_PROJECT_INVALID")?,
    )?;
    Ok(Some(value))
}

fn save(store: &ProjectStore, value: &InitialConversation) -> Result<(), String> {
    value.validate(&value.project_id)?;
    let path = store.root.join(FILE);
    if path
        .try_exists()
        .map_err(|_| "WEB_INITIAL_JOURNAL_UNAVAILABLE")?
    {
        plain(&path, false)?;
    }
    atomic(&path, value)
}

pub(super) fn ensure_ready(store: &ProjectStore) -> Result<(), String> {
    if load(store)?.is_some_and(|value| value.phase != Phase::Bound) {
        return Err("WEB_PROJECT_INITIALIZATION_PENDING".into());
    }
    Ok(())
}

fn summary(record: &InitialConversation) -> Value {
    json!({"action":"web_project_initialize","purpose":PURPOSE,"projectId":record.project_id,
        "intentId":record.intent_id,"phase":record.phase,"progress":record.progress,
        "predecessor":null,"registrationDigest":record.registration_digest,
        "messageDigest":record.message_digest,"contextBound":record.phase==Phase::Bound,
        "nativeOwnerChanged":false,"executionGranted":false,
        "submissionOutcomeUnknown":record.phase==Phase::Submitting && record.progress==Some(runtime::BootstrapProgress::SendAttempted),
        "destination":record.receipt.as_ref().map(|receipt|&receipt.destination_url)})
}

fn unbound(workspace: &Path, wake: &Store, record: &InitialConversation) -> Result<(), String> {
    if registered_project(workspace, &record.project_id)? != record.registration
        || wake.config()?.targets.contains_key(&record.project_id)
    {
        return Err("WEB_INITIAL_REGISTRATION_CHANGED".into());
    }
    let work = WorkLedger::status(workspace, &record.project_id);
    if work["status"] == "ATTENTION" || work.get("active").is_some_and(|value| !value.is_null()) {
        return Err("WEB_INITIAL_PROJECT_WORK_PENDING".into());
    }
    Ok(())
}

pub(crate) fn prepare(
    workspace: &Path,
    project: &str,
    intent: &str,
    input: InitialContext,
) -> Result<Value, String> {
    input.validate(project)?;
    if !valid_id(intent) {
        return Err("WEB_INITIAL_INTENT_INVALID".into());
    }
    let store = ProjectStore::open(workspace, project)?;
    if let Some(existing) = load(&store)? {
        if existing.intent_id != intent || existing.context != input {
            return Err("WEB_INITIAL_EXISTING_INTENT_REQUIRES_RESUME".into());
        }
        return Ok(summary(&existing));
    }
    if store.active()?.is_some() || store.handoff_request()?.is_some() {
        return Err("WEB_INITIAL_ROLLOVER_HISTORY_PRESENT".into());
    }
    let registration = resolve_registered_project_from_control_root(workspace, project)
        .map_err(|_| "WEB_INITIAL_REGISTERED_SCOPE_REQUIRED")?
        .project;
    let wake = Store::open(&crate::wake_protocol_client::wake_root_for_workspace(
        workspace,
    )?)?;
    let mut record = InitialConversation {
        schema_version: 1,
        purpose: PURPOSE.into(),
        project_id: project.into(),
        intent_id: intent.into(),
        registration_digest: registration_digest(&registration)?,
        registration,
        context: input,
        phase: Phase::Prepared,
        progress: None,
        message_digest: String::new(),
        receipt: None,
        created_utc: now(),
    };
    unbound(workspace, &wake, &record)?;
    record.message_digest = runtime::initialization_message_digest(
        project,
        intent,
        &record.registration_digest,
        &record.handoff()?,
    )?;
    save(&store, &record)?;
    Ok(summary(&record))
}

pub(crate) fn status(workspace: &Path, project: &str) -> Result<Value, String> {
    if !valid_id(project) {
        return Err("WEB_PROJECT_INVALID".into());
    }
    let path = workspace
        .join(".catdesk/web-rollovers")
        .join(project)
        .join(FILE);
    if !path
        .try_exists()
        .map_err(|_| "WEB_INITIAL_JOURNAL_UNAVAILABLE")?
    {
        return Ok(
            json!({"action":"web_project_initialize","projectId":project,"phase":"UNPREPARED","readOnly":true}),
        );
    }
    let store = ProjectStore::open_existing(workspace, project)?;
    Ok(summary(
        &load(&store)?.ok_or("WEB_INITIAL_INTENT_REQUIRED")?,
    ))
}

pub(super) fn queued_eligible(workspace: &Path, identity: &admission::Identity) -> bool {
    let store = match ProjectStore::open_existing(workspace, &identity.project_id) {
        Ok(store) => store,
        Err(reason) if reason == "WEB_ROLLOVER_PROJECT_BUSY" => return true,
        Err(_) => return false,
    };
    let Ok(Some(record)) = load(&store) else {
        return false;
    };
    if record.identity() != *identity
        || !(record.phase == Phase::Prepared
            || (record.phase == Phase::Submitting
                && record.progress != Some(runtime::BootstrapProgress::SendAttempted)))
    {
        return false;
    }
    crate::wake_protocol_client::wake_root_for_workspace(workspace)
        .and_then(|root| Store::open(&root))
        .and_then(|wake| unbound(workspace, &wake, &record))
        .is_ok()
}

fn meter(workspace: &Path, record: &InitialConversation, target: &Target) -> Result<(), String> {
    let meter = WebContextStore::open_nonblocking(workspace.join(".catdesk/web-context"))?;
    let key = key(&record.project_id, target);
    meter.bind_observed_target(key.clone(), None)?;
    let message = runtime::compose_initialization_message(
        &record.project_id,
        &record.intent_id,
        &record.registration_digest,
        &record.handoff()?,
    )?;
    let answer = runtime::initialization_acknowledgement(
        &record.project_id,
        &record.intent_id,
        &record.registration_digest,
    );
    for (prefix, kind, text) in [
        ("initial-user", VisibleSourceKind::UserMessage, message),
        (
            "initial-assistant",
            VisibleSourceKind::AssistantMessage,
            answer,
        ),
    ] {
        let id = format!("{prefix}-{}", digest(&record.intent_id));
        meter.record_observation(
            &key,
            Observation {
                event_id: &id,
                revision: 1,
                dedup_key: &id,
                source_kind: kind,
                visible_text: Some(&text),
                observed_at_unix: now(),
            },
        )?;
    }
    Ok(())
}

pub(crate) fn execute(
    workspace: &Path,
    project: &str,
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<Value, String> {
    keep_running()?;
    let store = ProjectStore::open_existing(workspace, project)?;
    let mut record = load(&store)?.ok_or("WEB_INITIAL_INTENT_REQUIRED")?;
    if record.phase == Phase::Bound {
        return Ok(summary(&record));
    }
    let wake = Store::open(&crate::wake_protocol_client::wake_root_for_workspace(
        workspace,
    )?)?;
    if matches!(record.phase, Phase::Prepared | Phase::Submitting) {
        if record.progress == Some(runtime::BootstrapProgress::SendAttempted) {
            return Err("WEB_INITIAL_SUBMISSION_UNKNOWN_NO_REPLAY".into());
        }
        unbound(workspace, &wake, &record)?;
        let Some(creation) = admission::try_acquire(wake.root(), record.identity(), |head| {
            queued_creation_eligible(workspace, head)
        })?
        else {
            let mut result = summary(&record);
            result["admission"] = json!("QUEUED");
            return Ok(result);
        };
        let registration = record.registration_digest.clone();
        let intent = record.intent_id.clone();
        let handoff = record.handoff()?;
        let resume = record.progress;
        let receipt = runtime::initialize_conversation_guarded(
            &wake,
            project,
            &intent,
            &registration,
            &handoff,
            resume,
            runtime::BootstrapCallbacks {
                keep_running,
                checkpoint: &mut |checkpoint| {
                    keep_running()?;
                    unbound(workspace, &wake, &record)?;
                    if let runtime::BootstrapCheckpoint::Progress(stage) = checkpoint {
                        record.phase = Phase::Submitting;
                        record.progress = Some(record.progress.map_or(stage, |old| old.max(stage)));
                        save(&store, &record)?;
                    }
                    Ok(())
                },
            },
        )?;
        record.receipt = Some(receipt);
        record.phase = Phase::ResponsePending;
        save(&store, &record)?;
        creation.creation_recorded()?;
    }
    if record.phase == Phase::ResponsePending {
        unbound(workspace, &wake, &record)?;
        let Some(receipt) = runtime::initialization_probe_completion_guarded(
            &wake,
            &record.registration_digest,
            record
                .receipt
                .as_ref()
                .ok_or("WEB_INITIAL_RECEIPT_REQUIRED")?,
            &record.handoff()?,
            keep_running,
        )?
        else {
            return Ok(summary(&record));
        };
        record.receipt = Some(receipt);
        record.phase = Phase::Created;
        save(&store, &record)?;
    }
    bind_completed(workspace, &store, &wake, &mut record, keep_running)?;
    Ok(summary(&record))
}

fn bind_completed(
    workspace: &Path,
    store: &ProjectStore,
    wake: &Store,
    record: &mut InitialConversation,
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    record.validate(&record.project_id)?;
    if record.phase != Phase::Created {
        return Err("WEB_INITIAL_COMPLETED_RESPONSE_REQUIRED".into());
    }
    let project = record.project_id.as_str();
    let target = record.receipt_target("RESPONSE_COMPLETED")?;
    let current = registered_project(workspace, project)?;
    let mut expected = record.registration.clone();
    expected.chatgpt_target_url = Some(target.url.clone());
    expected.chatgpt_target_sha256 = Some(target.digest.clone());
    if current != record.registration && current != expected {
        return Err("WEB_INITIAL_REGISTRATION_CHANGED".into());
    }
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &target.url,
    )?;
    keep_running()?;
    if !matches!(paired(workspace,project),Ok(paired) if paired==target) {
        mcp::operator_bind_project_web_target_at_root(
            workspace,
            Some(wake.root()),
            project,
            &target.url,
            None,
            0,
        )
        .map_err(|_| "WEB_INITIAL_PAIRED_COMMIT_REQUIRED")?;
    }
    if registered_project(workspace, project)? != expected || paired(workspace, project)? != target
    {
        return Err("WEB_INITIAL_PAIRED_COMMIT_UNVERIFIED".into());
    }
    meter(workspace, record, &target)?;
    record.phase = Phase::Bound;
    save(store, record)?;
    Ok(())
}

#[cfg(all(test, windows))]
mod first_conversation_tests {
    use super::*;

    fn fixture() -> (PathBuf, PathBuf, PathBuf) {
        let (parent, control, wake, _, _) = super::super::tests::paired_handoff_fixture();
        let registry =
            AutonomousProjectRegistryStoreV1::open(control.join(".catdesk/projects")).unwrap();
        for project in ["first", "second"] {
            let path = parent.join(project);
            fs::create_dir(&path).unwrap();
            for args in [
                vec!["init", "-q"],
                vec![
                    "remote",
                    "add",
                    "origin",
                    "https://example.test/initial.git",
                ],
            ] {
                assert!(
                    std::process::Command::new("git")
                        .arg("-C")
                        .arg(&path)
                        .args(args)
                        .status()
                        .unwrap()
                        .success()
                );
            }
            registry
                .register_project(AutonomousProjectV1 {
                    project_id: project.into(),
                    workspace: path.canonicalize().unwrap(),
                    git_identity: "https://example.test/initial.git".into(),
                    verification_profile: "web_only_v1".into(),
                    codex_thread_id: None,
                    chatgpt_target_url: None,
                    chatgpt_target_sha256: None,
                    chatgpt_target_history: vec![],
                })
                .unwrap();
        }
        (parent, control, wake)
    }

    fn context(project: &str) -> InitialContext {
        serde_json::from_value(json!({"schemaVersion":1,"projectId":project,"taskId":"owned-task",
            "objective":"Initialize context only","nextActions":["Wait for an explicit scoped native task grant"],
            "ownedPaths":["src/example.rs"]})).unwrap()
    }

    fn completed(record: &mut InitialConversation) {
        let url = "https://chatgpt.com/c/source-fixture-only-initial";
        record.phase = Phase::Created;
        record.progress = Some(runtime::BootstrapProgress::SendAttempted);
        record.receipt = Some(runtime::BootstrapReceipt {
            schema_version: 2,
            project_id: record.project_id.clone(),
            intent_id: record.intent_id.clone(),
            source_generation: 0,
            source_digest: record.registration_digest.clone(),
            destination_url: url.into(),
            destination_digest: digest(url),
            message_digest: record.message_digest.clone(),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
            completion: "RESPONSE_COMPLETED".into(),
        });
    }

    #[test]
    fn first_conversation_prepare_is_durable_scoped_and_does_not_bind_or_grant_work() {
        let (_parent, control, wake) = fixture();
        let registry_path = control.join(".catdesk/projects/projects.json");
        let wake_path = wake.join("config.json");
        let old_registry = fs::read(&registry_path).unwrap();
        let old_wake = fs::read(&wake_path).unwrap();
        let result = prepare(&control, "first", "initial-first", context("first")).unwrap();
        assert_eq!(result["phase"], "PREPARED");
        assert_eq!(result["predecessor"], Value::Null);
        assert_eq!(result["executionGranted"], false);
        let store = ProjectStore::open_existing(&control, "first").unwrap();
        let before = fs::read(store.root.join(FILE)).unwrap();
        let record = load(&store).unwrap().unwrap();
        assert!(record.registration.chatgpt_target_history.is_empty());
        assert_eq!(record.identity().source_generation, 0);
        assert!(
            record
                .handoff()
                .unwrap()
                .contains("CONTEXT_ONLY_WAIT_FOR_EXPLICIT_TASK_GRANT")
        );
        assert_eq!(
            ensure_ready(&store).unwrap_err(),
            "WEB_PROJECT_INITIALIZATION_PENDING"
        );
        drop(store);
        prepare(&control, "first", "initial-first", context("first")).unwrap();
        let store = ProjectStore::open_existing(&control, "first").unwrap();
        assert_eq!(fs::read(store.root.join(FILE)).unwrap(), before);
        drop(store);
        assert!(prepare(&control, "first", "different", context("first")).is_err());
        assert!(prepare(&control, "first", "initial-first", context("second")).is_err());
        assert_eq!(fs::read(registry_path).unwrap(), old_registry);
        assert_eq!(fs::read(wake_path).unwrap(), old_wake);
    }

    #[test]
    fn first_conversation_unknown_send_cannot_replay_or_starve_an_independent_project() {
        let (_parent, control, wake_root) = fixture();
        prepare(&control, "first", "initial-first", context("first")).unwrap();
        prepare(&control, "second", "initial-second", context("second")).unwrap();
        let first = ProjectStore::open_existing(&control, "first").unwrap();
        let mut record = load(&first).unwrap().unwrap();
        let ticket = admission::try_acquire(&wake_root, record.identity(), |_| true)
            .unwrap()
            .unwrap();
        record.phase = Phase::Submitting;
        record.progress = Some(runtime::BootstrapProgress::SendAttempted);
        save(&first, &record).unwrap();
        let before = fs::read(first.root.join(FILE)).unwrap();
        drop(first);
        assert_eq!(
            execute(&control, "first", &|| Ok(())).unwrap_err(),
            "WEB_INITIAL_SUBMISSION_UNKNOWN_NO_REPLAY"
        );
        assert_eq!(
            execute(&control, "second", &|| Ok(())).unwrap()["admission"],
            "QUEUED"
        );
        drop(ticket);
        let second = ProjectStore::open_existing(&control, "second").unwrap();
        let second_record = load(&second).unwrap().unwrap();
        drop(second);
        admission::try_acquire(&wake_root, second_record.identity(), |head| {
            queued_creation_eligible(&control, head)
        })
        .unwrap()
        .unwrap()
        .creation_recorded()
        .unwrap();
        let first = ProjectStore::open_existing(&control, "first").unwrap();
        assert_eq!(fs::read(first.root.join(FILE)).unwrap(), before);
        assert!(paired(&control, "alpha").is_ok());
        assert!(paired(&control, "beta").is_ok());
    }

    #[test]
    fn first_conversation_completed_pair_commit_recovers_wake_only_and_context_reconciliation() {
        let (_parent, control, wake_root) = fixture();
        let wake = Store::open(&wake_root).unwrap();
        prepare(&control, "first", "initial-first", context("first")).unwrap();
        let store = ProjectStore::open_existing(&control, "first").unwrap();
        let mut record = load(&store).unwrap().unwrap();
        assert!(bind_completed(&control, &store, &wake, &mut record, &|| Ok(())).is_err());
        completed(&mut record);
        save(&store, &record).unwrap();
        let sibling = registered_project(&control, "second").unwrap();
        let native = record.registration.codex_thread_id.clone();
        wake.set_target(
            "first",
            0,
            &record.receipt.as_ref().unwrap().destination_url,
        )
        .unwrap(); // Crash after Wake CAS, before registry CAS.
        bind_completed(&control, &store, &wake, &mut record, &|| Ok(())).unwrap();
        assert_eq!(record.phase, Phase::Bound);
        let target = paired(&control, "first").unwrap();
        assert_eq!(target.generation, 1);
        let row = registered_project(&control, "first").unwrap();
        assert_eq!(row.codex_thread_id, native);
        assert!(row.chatgpt_target_history.is_empty());
        assert_eq!(registered_project(&control, "second").unwrap(), sibling);
        let context_root = control.join(".catdesk/web-context");
        let meter = WebContextStore::open_nonblocking(&context_root).unwrap();
        let before = serde_json::to_value(meter.pressure(&key("first", &target)).unwrap()).unwrap();
        drop(meter);
        record.phase = Phase::Created;
        save(&store, &record).unwrap(); // Crash after paired binding/meter, before BOUND journal save.
        bind_completed(&control, &store, &wake, &mut record, &|| Ok(())).unwrap();
        ensure_ready(&store).unwrap();
        drop(store);
        let meter = WebContextStore::open_nonblocking(&context_root).unwrap();
        assert_eq!(
            serde_json::to_value(meter.pressure(&key("first", &target)).unwrap()).unwrap(),
            before
        );
        drop(meter);
        assert_eq!(
            execute(&control, "first", &|| Ok(())).unwrap()["phase"],
            "BOUND"
        );
    }

    #[test]
    fn first_conversation_context_and_journal_reject_mixed_identities_and_predecessors() {
        let mut input = context("first");
        input.next_actions.clear();
        assert!(input.validate("first").is_err());
        input = context("first");
        input.objective = "x".repeat(4097);
        assert!(input.validate("first").is_err());
        let (_parent, control, _wake) = fixture();
        prepare(&control, "first", "initial-first", context("first")).unwrap();
        let store = ProjectStore::open_existing(&control, "first").unwrap();
        let record = load(&store).unwrap().unwrap();
        for case in 0..4 {
            let mut changed = record.clone();
            match case {
                0 => changed
                    .registration
                    .chatgpt_target_history
                    .push("https://chatgpt.com/c/foreign".into()),
                1 => changed.project_id = "second".into(),
                2 => changed.message_digest = "f".repeat(64),
                _ => changed.registration.workspace = control.clone(),
            }
            assert!(changed.validate("first").is_err());
        }
    }
}
