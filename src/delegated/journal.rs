use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::EXECUTION_CONTRACT_SCHEMA_VERSION;
use super::contracts::{
    ArtifactId, ExecutionContractV1, PatchId, RunId, RunState, ToolCallId, TurnId,
    validate_contract,
};
use super::events::{EventCursor, EventEnvelopeV1, poll_events, validate_append_order};
use super::state_machine::is_terminal;

#[derive(Clone, Debug)]
pub struct DelegatedJournal {
    root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSnapshotV1 {
    pub schema_version: u32,
    pub run_id: RunId,
    pub state: RunState,
    pub contract_hash: String,
    pub last_event_sequence: u64,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ToolCallStatus {
    Requested,
    PolicyAllowed,
    ApprovalRequired,
    Approved,
    Executing,
    Completed,
    Failed,
    OutcomeUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ToolMutationKind {
    ReadOnly,
    Mutating,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecordV1 {
    pub schema_version: u32,
    pub run_id: RunId,
    pub tool_call_id: ToolCallId,
    pub tool_name: String,
    pub request_hash: String,
    pub arguments_hash: String,
    pub mutation_kind: ToolMutationKind,
    pub status: ToolCallStatus,
    pub result_hash: Option<String>,
    pub outcome_summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PatchApplicationStatus {
    Applied,
    Rejected,
    Conflicted,
    OutcomeUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchProposalRecordV1 {
    pub schema_version: u32,
    pub patch_id: PatchId,
    pub parent_patch_id: Option<PatchId>,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub base_snapshot_hash: String,
    pub target_paths: Vec<String>,
    pub expected_preimage_hashes: Vec<String>,
    pub proposed_diff_hash: String,
    pub model_rationale: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchApplicationRecordV1 {
    pub schema_version: u32,
    pub patch_id: PatchId,
    pub tool_call_id: ToolCallId,
    pub status: PatchApplicationStatus,
    pub actual_paths_changed: Vec<String>,
    pub before_hashes: Vec<String>,
    pub after_hashes: Vec<String>,
    pub result_hash: String,
    pub diff_artifact_id: ArtifactId,
}

#[derive(Debug, PartialEq, Eq)]
pub enum JournalError {
    Io(String),
    Serde(String),
    Validation(String),
    DuplicateRun(String),
    RunNotFound(String),
    DuplicateToolCall(String),
    MissingToolCall(String),
    InvalidToolTransition {
        tool_call_id: String,
        from: ToolCallStatus,
        to: ToolCallStatus,
    },
    CompletedToolCallWillNotReplay(String),
    OutcomeUnknownRequiresSupervisor(String),
}

impl DelegatedJournal {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, JournalError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn create_run(
        &self,
        contract: &ExecutionContractV1,
    ) -> Result<RunSnapshotV1, JournalError> {
        validate_contract(contract).map_err(JournalError::Validation)?;
        let run_id = RunId::new(contract.task_id.clone()).map_err(JournalError::Validation)?;
        let run_dir = self.run_dir(&run_id)?;
        if run_dir.join("state.json").exists() {
            return Err(JournalError::DuplicateRun(run_id.as_str().to_string()));
        }
        fs::create_dir_all(&run_dir)?;

        let contract_hash = contract.request_hash().map_err(JournalError::Validation)?;
        let snapshot = RunSnapshotV1 {
            schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
            run_id,
            state: RunState::Ready,
            contract_hash,
            last_event_sequence: 0,
            active: true,
        };

        write_json_atomic(&run_dir.join("contract.json"), contract)?;
        write_json_atomic(&run_dir.join("state.json"), &snapshot)?;
        write_json_atomic(
            &run_dir.join("tool_calls.json"),
            &BTreeMap::<String, ToolCallRecordV1>::new(),
        )?;
        write_json_atomic(
            &run_dir.join("patch_proposals.json"),
            &Vec::<PatchProposalRecordV1>::new(),
        )?;
        write_json_atomic(
            &run_dir.join("patch_applications.json"),
            &Vec::<PatchApplicationRecordV1>::new(),
        )?;
        Ok(snapshot)
    }

    pub fn load_run(&self, run_id: &RunId) -> Result<RunSnapshotV1, JournalError> {
        read_json(&self.run_dir(run_id)?.join("state.json"))
    }

    pub fn update_run_state(
        &self,
        run_id: &RunId,
        state: RunState,
    ) -> Result<RunSnapshotV1, JournalError> {
        let mut snapshot = self.load_run(run_id)?;
        snapshot.active = !is_terminal(&state);
        snapshot.state = state;
        write_json_atomic(&self.run_dir(run_id)?.join("state.json"), &snapshot)?;
        Ok(snapshot)
    }

    pub fn append_event(&self, event: &EventEnvelopeV1) -> Result<(), JournalError> {
        let run_dir = self.run_dir(&event.run_id)?;
        ensure_run_dir_exists(&run_dir, &event.run_id)?;
        let events_path = run_dir.join("events.jsonl");
        let events = read_jsonl::<EventEnvelopeV1>(&events_path)?;
        let last_sequence = events.last().map(|event| event.event_sequence);
        validate_append_order(last_sequence, event).map_err(JournalError::Validation)?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)?;
        serde_json::to_writer(&mut file, event)?;
        file.write_all(b"\n")?;
        file.sync_data()?;

        let mut snapshot = self.load_run(&event.run_id)?;
        snapshot.last_event_sequence = event.event_sequence;
        write_json_atomic(&run_dir.join("state.json"), &snapshot)?;
        Ok(())
    }

    pub fn poll_events(
        &self,
        run_id: &RunId,
        cursor: EventCursor,
    ) -> Result<Vec<EventEnvelopeV1>, JournalError> {
        let events = read_jsonl::<EventEnvelopeV1>(&self.run_dir(run_id)?.join("events.jsonl"))?;
        Ok(poll_events(&events, cursor))
    }

    pub fn restore_active_runs(&self) -> Result<Vec<RunSnapshotV1>, JournalError> {
        let mut runs = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let state_path = entry.path().join("state.json");
            if !state_path.exists() {
                continue;
            }
            let snapshot: RunSnapshotV1 = read_json(&state_path)?;
            if snapshot.active && !is_terminal(&snapshot.state) {
                runs.push(snapshot);
            }
        }
        runs.sort_by(|left, right| left.run_id.as_str().cmp(right.run_id.as_str()));
        Ok(runs)
    }

    pub fn record_tool_call_requested(&self, record: ToolCallRecordV1) -> Result<(), JournalError> {
        if record.status != ToolCallStatus::Requested {
            return Err(JournalError::Validation(
                "new tool-call records must start as REQUESTED".into(),
            ));
        }
        ensure_run_dir_exists(&self.run_dir(&record.run_id)?, &record.run_id)?;
        let mut calls = self.load_tool_calls(&record.run_id)?;
        let key = record.tool_call_id.as_str().to_string();
        if calls.contains_key(&key) {
            return Err(JournalError::DuplicateToolCall(key));
        }
        calls.insert(key, record);
        self.write_tool_calls(calls.values())
    }

    pub fn transition_tool_call(
        &self,
        run_id: &RunId,
        tool_call_id: &ToolCallId,
        next: ToolCallStatus,
        result_hash: Option<String>,
        outcome_summary: Option<String>,
    ) -> Result<ToolCallRecordV1, JournalError> {
        let mut calls = self.load_tool_calls(run_id)?;
        let key = tool_call_id.as_str().to_string();
        let mut record = calls
            .remove(&key)
            .ok_or_else(|| JournalError::MissingToolCall(key.clone()))?;

        guard_replay(tool_call_id, &record.status)?;
        if !is_valid_tool_transition(&record.status, &next) {
            return Err(JournalError::InvalidToolTransition {
                tool_call_id: key,
                from: record.status,
                to: next,
            });
        }
        record.status = next;
        if let Some(hash) = result_hash {
            record.result_hash = Some(hash);
        }
        if let Some(summary) = outcome_summary {
            record.outcome_summary = Some(summary);
        }
        calls.insert(record.tool_call_id.as_str().to_string(), record.clone());
        self.write_tool_calls(calls.values())?;
        Ok(record)
    }

    pub fn load_tool_calls(
        &self,
        run_id: &RunId,
    ) -> Result<BTreeMap<String, ToolCallRecordV1>, JournalError> {
        let run_dir = self.run_dir(run_id)?;
        let records: BTreeMap<String, ToolCallRecordV1> =
            read_json(&run_dir.join("tool_calls.json"))?;
        Ok(records)
    }

    pub fn record_patch_proposal(
        &self,
        proposal: PatchProposalRecordV1,
    ) -> Result<(), JournalError> {
        ensure_run_dir_exists(&self.run_dir(&proposal.run_id)?, &proposal.run_id)?;
        let mut proposals = self.load_patch_proposals(&proposal.run_id)?;
        if proposals
            .iter()
            .any(|existing| existing.patch_id == proposal.patch_id)
        {
            return Err(JournalError::Validation("duplicate patch proposal".into()));
        }
        if let Some(parent_patch_id) = proposal.parent_patch_id.as_ref()
            && !proposals
                .iter()
                .any(|existing| &existing.patch_id == parent_patch_id)
        {
            return Err(JournalError::Validation(
                "parent patch proposal is missing".into(),
            ));
        }
        proposals.push(proposal);
        write_json_atomic(
            &self
                .run_dir(&proposals[0].run_id)?
                .join("patch_proposals.json"),
            &proposals,
        )
    }

    pub fn load_patch_proposals(
        &self,
        run_id: &RunId,
    ) -> Result<Vec<PatchProposalRecordV1>, JournalError> {
        read_json(&self.run_dir(run_id)?.join("patch_proposals.json"))
    }

    pub fn record_patch_application(
        &self,
        run_id: &RunId,
        application: PatchApplicationRecordV1,
    ) -> Result<(), JournalError> {
        ensure_run_dir_exists(&self.run_dir(run_id)?, run_id)?;
        if !self
            .load_patch_proposals(run_id)?
            .iter()
            .any(|proposal| proposal.patch_id == application.patch_id)
        {
            return Err(JournalError::Validation(
                "patch application references an unknown proposal".into(),
            ));
        }
        let mut applications = self.load_patch_applications(run_id)?;
        applications.push(application);
        write_json_atomic(
            &self.run_dir(run_id)?.join("patch_applications.json"),
            &applications,
        )
    }

    pub fn load_patch_applications(
        &self,
        run_id: &RunId,
    ) -> Result<Vec<PatchApplicationRecordV1>, JournalError> {
        read_json(&self.run_dir(run_id)?.join("patch_applications.json"))
    }

    fn run_dir(&self, run_id: &RunId) -> Result<PathBuf, JournalError> {
        Ok(self.root.join(safe_run_segment(run_id)?))
    }

    fn write_tool_calls<'a>(
        &self,
        records: impl IntoIterator<Item = &'a ToolCallRecordV1>,
    ) -> Result<(), JournalError> {
        let mut calls = BTreeMap::new();
        let mut run_id = None;
        for record in records {
            run_id = Some(record.run_id.clone());
            calls.insert(record.tool_call_id.as_str().to_string(), record.clone());
        }
        let run_id = run_id.ok_or_else(|| JournalError::Validation("missing run_id".into()))?;
        write_json_atomic(&self.run_dir(&run_id)?.join("tool_calls.json"), &calls)
    }
}

fn is_valid_tool_transition(from: &ToolCallStatus, to: &ToolCallStatus) -> bool {
    use ToolCallStatus::*;
    match (from, to) {
        (Requested, PolicyAllowed | ApprovalRequired | Failed) => true,
        (PolicyAllowed, ApprovalRequired | Executing | Failed) => true,
        (ApprovalRequired, Approved | Failed) => true,
        (Approved, Executing | Failed) => true,
        (Executing, Completed | Failed | OutcomeUnknown) => true,
        (Completed | Failed | OutcomeUnknown, _) => false,
        _ => false,
    }
}

fn guard_replay(tool_call_id: &ToolCallId, status: &ToolCallStatus) -> Result<(), JournalError> {
    match status {
        ToolCallStatus::Completed => Err(JournalError::CompletedToolCallWillNotReplay(
            tool_call_id.as_str().to_string(),
        )),
        ToolCallStatus::OutcomeUnknown => Err(JournalError::OutcomeUnknownRequiresSupervisor(
            tool_call_id.as_str().to_string(),
        )),
        _ => Ok(()),
    }
}

fn safe_run_segment(run_id: &RunId) -> Result<String, JournalError> {
    let hash = super::contracts::stable_hash(run_id).map_err(JournalError::Validation)?;
    let suffix = hash
        .strip_prefix("fnv1a64:")
        .ok_or_else(|| JournalError::Validation("unexpected run hash format".into()))?;
    let mut segment = run_id
        .as_str()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    segment.truncate(48);
    Ok(format!("{segment}-{suffix}"))
}

fn ensure_run_dir_exists(run_dir: &Path, run_id: &RunId) -> Result<(), JournalError> {
    if !run_dir.is_dir() {
        return Err(JournalError::RunNotFound(run_id.as_str().to_string()));
    }
    Ok(())
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), JournalError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("tmp");
    {
        let mut file = File::create(&tmp_path)?;
        serde_json::to_writer_pretty(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_data()?;
    }
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, JournalError> {
    let file = File::open(path)?;
    serde_json::from_reader(file).map_err(Into::into)
}

fn read_jsonl<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>, JournalError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(path)?;
    let mut values = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        values.push(serde_json::from_str(&line)?);
    }
    Ok(values)
}

impl From<std::io::Error> for JournalError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl From<serde_json::Error> for JournalError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serde(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegated::events::{EventPayloadV1, LifecycleEvent};

    fn fixture_contract() -> ExecutionContractV1 {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/delegated/execution_contract_v1.json"
        ))
        .expect("fixture parses")
    }

    fn temp_journal(name: &str) -> DelegatedJournal {
        let root = std::env::temp_dir().join(format!("catdesk-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).expect("clear old temp journal");
        }
        DelegatedJournal::open(root).expect("journal opens")
    }

    fn create_fixture_run(journal: &DelegatedJournal) -> RunId {
        journal
            .create_run(&fixture_contract())
            .expect("run created")
            .run_id
    }

    fn tool_call(run_id: &RunId, id: &str, mutation_kind: ToolMutationKind) -> ToolCallRecordV1 {
        ToolCallRecordV1 {
            schema_version: 1,
            run_id: run_id.clone(),
            tool_call_id: ToolCallId::new(id).expect("tool id"),
            tool_name: "patch.apply".into(),
            request_hash: "fnv1a64:req".into(),
            arguments_hash: "fnv1a64:args".into(),
            mutation_kind,
            status: ToolCallStatus::Requested,
            result_hash: None,
            outcome_summary: None,
        }
    }

    #[test]
    fn duplicate_run_creation_does_not_overwrite_journal() {
        let journal = temp_journal("journal-duplicate-run");
        let contract = fixture_contract();
        let run_id = journal.create_run(&contract).expect("run created").run_id;
        let tool_call_id = ToolCallId::new("tc-preserved").expect("tool id");
        journal
            .record_tool_call_requested(tool_call(
                &run_id,
                tool_call_id.as_str(),
                ToolMutationKind::Mutating,
            ))
            .expect("tool record");

        assert!(matches!(
            journal.create_run(&contract),
            Err(JournalError::DuplicateRun(id)) if id == contract.task_id
        ));
        assert!(
            journal
                .load_tool_calls(&run_id)
                .expect("load tool calls")
                .contains_key(tool_call_id.as_str())
        );
    }

    #[test]
    fn duplicate_tool_call_ids_fail() {
        let journal = temp_journal("journal-duplicate-tool-call");
        let run_id = create_fixture_run(&journal);
        let record = tool_call(&run_id, "tc-1", ToolMutationKind::Mutating);

        journal
            .record_tool_call_requested(record.clone())
            .expect("first record succeeds");
        assert!(matches!(
            journal.record_tool_call_requested(record),
            Err(JournalError::DuplicateToolCall(id)) if id == "tc-1"
        ));
    }

    #[test]
    fn completed_mutations_are_not_replayed_after_restart() {
        let journal = temp_journal("journal-no-replay");
        let run_id = create_fixture_run(&journal);
        let tool_call_id = ToolCallId::new("tc-mutating").expect("tool id");
        journal
            .record_tool_call_requested(tool_call(
                &run_id,
                tool_call_id.as_str(),
                ToolMutationKind::Mutating,
            ))
            .expect("record call");
        journal
            .transition_tool_call(
                &run_id,
                &tool_call_id,
                ToolCallStatus::PolicyAllowed,
                None,
                None,
            )
            .expect("allow call");
        journal
            .transition_tool_call(
                &run_id,
                &tool_call_id,
                ToolCallStatus::Executing,
                None,
                None,
            )
            .expect("execute call");
        journal
            .transition_tool_call(
                &run_id,
                &tool_call_id,
                ToolCallStatus::Completed,
                Some("fnv1a64:result".into()),
                Some("patch applied".into()),
            )
            .expect("complete call");

        let reopened = DelegatedJournal::open(journal.root.clone()).expect("reopen journal");
        assert!(matches!(
            reopened.transition_tool_call(
                &run_id,
                &tool_call_id,
                ToolCallStatus::Executing,
                None,
                None
            ),
            Err(JournalError::CompletedToolCallWillNotReplay(id)) if id == "tc-mutating"
        ));
    }

    #[test]
    fn outcome_unknown_requires_supervisor_after_restart() {
        let journal = temp_journal("journal-outcome-unknown");
        let run_id = create_fixture_run(&journal);
        let tool_call_id = ToolCallId::new("tc-unknown").expect("tool id");
        journal
            .record_tool_call_requested(tool_call(
                &run_id,
                tool_call_id.as_str(),
                ToolMutationKind::Mutating,
            ))
            .expect("record call");
        journal
            .transition_tool_call(
                &run_id,
                &tool_call_id,
                ToolCallStatus::PolicyAllowed,
                None,
                None,
            )
            .expect("allow call");
        journal
            .transition_tool_call(
                &run_id,
                &tool_call_id,
                ToolCallStatus::Executing,
                None,
                None,
            )
            .expect("execute call");
        journal
            .transition_tool_call(
                &run_id,
                &tool_call_id,
                ToolCallStatus::OutcomeUnknown,
                None,
                Some("process exited before result persisted".into()),
            )
            .expect("mark unknown");

        let reopened = DelegatedJournal::open(journal.root.clone()).expect("reopen journal");
        assert!(matches!(
            reopened.transition_tool_call(
                &run_id,
                &tool_call_id,
                ToolCallStatus::Executing,
                None,
                None
            ),
            Err(JournalError::OutcomeUnknownRequiresSupervisor(id)) if id == "tc-unknown"
        ));
    }

    #[test]
    fn patch_and_result_hashes_survive_restart() {
        let journal = temp_journal("journal-patches");
        let run_id = create_fixture_run(&journal);
        let patch_id = PatchId::new("patch-1").expect("patch id");
        let tool_call_id = ToolCallId::new("tc-patch").expect("tool id");
        journal
            .record_patch_proposal(PatchProposalRecordV1 {
                schema_version: 1,
                patch_id: patch_id.clone(),
                parent_patch_id: None,
                run_id: run_id.clone(),
                turn_id: TurnId::new("turn-1").expect("turn id"),
                base_snapshot_hash: "fnv1a64:base".into(),
                target_paths: vec!["src/delegated/journal.rs".into()],
                expected_preimage_hashes: vec!["fnv1a64:before".into()],
                proposed_diff_hash: "fnv1a64:proposal".into(),
                model_rationale: "exercise durable patch lineage".into(),
            })
            .expect("proposal stored");
        journal
            .record_patch_application(
                &run_id,
                PatchApplicationRecordV1 {
                    schema_version: 1,
                    patch_id,
                    tool_call_id,
                    status: PatchApplicationStatus::Applied,
                    actual_paths_changed: vec!["src/delegated/journal.rs".into()],
                    before_hashes: vec!["fnv1a64:before".into()],
                    after_hashes: vec!["fnv1a64:after".into()],
                    result_hash: "fnv1a64:result".into(),
                    diff_artifact_id: ArtifactId::new("artifact-diff").expect("artifact id"),
                },
            )
            .expect("application stored");

        let reopened = DelegatedJournal::open(journal.root.clone()).expect("reopen journal");
        assert_eq!(
            reopened
                .load_patch_proposals(&run_id)
                .expect("load proposals")[0]
                .proposed_diff_hash,
            "fnv1a64:proposal"
        );
        assert_eq!(
            reopened
                .load_patch_applications(&run_id)
                .expect("load applications")[0]
                .result_hash,
            "fnv1a64:result"
        );
    }

    #[test]
    fn patch_application_requires_known_proposal() {
        let journal = temp_journal("journal-patch-lineage");
        let run_id = create_fixture_run(&journal);
        let result = journal.record_patch_application(
            &run_id,
            PatchApplicationRecordV1 {
                schema_version: 1,
                patch_id: PatchId::new("missing-patch").expect("patch id"),
                tool_call_id: ToolCallId::new("tc-patch").expect("tool id"),
                status: PatchApplicationStatus::Applied,
                actual_paths_changed: vec!["src/delegated/journal.rs".into()],
                before_hashes: vec!["fnv1a64:before".into()],
                after_hashes: vec!["fnv1a64:after".into()],
                result_hash: "fnv1a64:result".into(),
                diff_artifact_id: ArtifactId::new("artifact-diff").expect("artifact id"),
            },
        );

        assert!(
            matches!(result, Err(JournalError::Validation(message)) if message.contains("unknown proposal"))
        );
    }

    #[test]
    fn child_patch_requires_existing_parent() {
        let journal = temp_journal("journal-parent-patch");
        let run_id = create_fixture_run(&journal);
        let result = journal.record_patch_proposal(PatchProposalRecordV1 {
            schema_version: 1,
            patch_id: PatchId::new("patch-child").expect("patch id"),
            parent_patch_id: Some(PatchId::new("patch-missing").expect("patch id")),
            run_id,
            turn_id: TurnId::new("turn-1").expect("turn id"),
            base_snapshot_hash: "fnv1a64:base".into(),
            target_paths: vec!["src/delegated/journal.rs".into()],
            expected_preimage_hashes: vec!["fnv1a64:before".into()],
            proposed_diff_hash: "fnv1a64:proposal".into(),
            model_rationale: "exercise parent lineage".into(),
        });

        assert!(
            matches!(result, Err(JournalError::Validation(message)) if message.contains("parent patch"))
        );
    }

    #[test]
    fn active_runs_restore_after_restart() {
        let journal = temp_journal("journal-active-runs");
        let run_id = create_fixture_run(&journal);
        journal
            .update_run_state(&run_id, RunState::Running)
            .expect("mark running");

        let reopened = DelegatedJournal::open(journal.root.clone()).expect("reopen journal");
        let active_runs = reopened.restore_active_runs().expect("restore active");
        assert_eq!(active_runs.len(), 1);
        assert_eq!(active_runs[0].run_id, run_id);

        reopened
            .update_run_state(&run_id, RunState::CompletedVerified)
            .expect("mark terminal");
        assert!(
            reopened
                .restore_active_runs()
                .expect("restore active")
                .is_empty()
        );
    }

    #[test]
    fn event_cursor_survives_restart() {
        let journal = temp_journal("journal-events");
        let run_id = create_fixture_run(&journal);
        let event = EventEnvelopeV1 {
            schema_version: 1,
            event_sequence: 1,
            run_id: run_id.clone(),
            worker_session_id: None,
            turn_id: None,
            item_id: None,
            lifecycle_event: LifecycleEvent::Started,
            request_hash: "fnv1a64:req".into(),
            result_hash: None,
            payload: EventPayloadV1::RunStateChanged {
                state: RunState::Running,
            },
        }
        .with_result_hash()
        .expect("hash event");
        journal.append_event(&event).expect("append event");

        let reopened = DelegatedJournal::open(journal.root.clone()).expect("reopen journal");
        let events = reopened
            .poll_events(
                &run_id,
                EventCursor {
                    after_sequence: 0,
                    limit: 10,
                },
            )
            .expect("poll events");
        assert_eq!(events, vec![event]);
        assert_eq!(
            reopened
                .load_run(&run_id)
                .expect("load run")
                .last_event_sequence,
            1
        );
    }
}
