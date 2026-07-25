use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::contracts::{
    ArtifactId, EscalationPacketV1, FinalRunResultV1, PatchId, RunId, RunState,
};
use super::events::{EventCursor, EventEnvelopeV1, poll_events};
use super::patch_engine::{
    ActualDiffArtifactV1, PatchComparisonV1, PatchProposalV1, compare_patches,
};

pub const SUPERVISOR_TOOL_NAMES: [&str; 13] = [
    "delegated_run_create",
    "delegated_run_validate",
    "delegated_run_approve_start",
    "delegated_run_start",
    "delegated_run_status",
    "delegated_run_list",
    "delegated_run_events",
    "delegated_run_get_checkpoint",
    "delegated_run_get_patch",
    "delegated_run_compare_patches",
    "delegated_run_get_diff",
    "delegated_run_cancel",
    "delegated_run_get_final_review",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundedArtifactV1 {
    pub artifact_id: ArtifactId,
    pub text: String,
    pub byte_count: usize,
    pub truncated: bool,
    pub local_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorRunRecordV1 {
    pub run_id: RunId,
    pub state: RunState,
    pub checkpoint: Option<String>,
    pub escalation: Option<EscalationPacketV1>,
    pub final_review: Option<FinalRunResultV1>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SupervisorError {
    MissingRun(String),
    MissingArtifact(String),
    MissingPatch(String),
    MissingDiff(String),
    DecisionAlreadyUsed(String),
    InvalidTransition(String),
}

#[derive(Default)]
pub struct SupervisorSurface {
    runs: BTreeMap<String, SupervisorRunRecordV1>,
    events: BTreeMap<String, Vec<EventEnvelopeV1>>,
    artifacts: BTreeMap<String, String>,
    patches: BTreeMap<String, PatchProposalV1>,
    diffs: BTreeMap<String, ActualDiffArtifactV1>,
    consumed_decisions: BTreeSet<String>,
}

impl SupervisorSurface {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tool_names() -> &'static [&'static str] {
        &SUPERVISOR_TOOL_NAMES
    }

    pub fn create_run(&mut self, run_id: RunId) {
        self.runs.insert(
            run_id.as_str().to_string(),
            SupervisorRunRecordV1 {
                run_id,
                state: RunState::Draft,
                checkpoint: None,
                escalation: None,
                final_review: None,
            },
        );
    }

    pub fn validate_run(&self, run_id: &RunId) -> Result<(), SupervisorError> {
        self.require_run(run_id).map(|_| ())
    }

    pub fn start_run(&mut self, run_id: &RunId) -> Result<(), SupervisorError> {
        let run = self.require_run_mut(run_id)?;
        if run.state != RunState::Draft && run.state != RunState::Ready {
            return Err(SupervisorError::InvalidTransition(format!(
                "cannot start from {:?}",
                run.state
            )));
        }
        run.state = RunState::Running;
        Ok(())
    }

    pub fn status(&self, run_id: &RunId) -> Result<RunState, SupervisorError> {
        Ok(self.require_run(run_id)?.state.clone())
    }

    pub fn list_runs(&self) -> Vec<RunId> {
        self.runs.values().map(|run| run.run_id.clone()).collect()
    }

    pub fn append_event(&mut self, event: EventEnvelopeV1) {
        self.events
            .entry(event.run_id.as_str().to_string())
            .or_default()
            .push(event);
    }

    pub fn events(
        &self,
        run_id: &RunId,
        cursor: EventCursor,
    ) -> Result<Vec<EventEnvelopeV1>, SupervisorError> {
        self.require_run(run_id)?;
        Ok(poll_events(
            self.events
                .get(run_id.as_str())
                .map(Vec::as_slice)
                .unwrap_or_default(),
            cursor,
        ))
    }

    pub fn set_checkpoint(
        &mut self,
        run_id: &RunId,
        checkpoint: String,
    ) -> Result<(), SupervisorError> {
        self.require_run_mut(run_id)?.checkpoint = Some(checkpoint);
        Ok(())
    }

    pub fn get_checkpoint(&self, run_id: &RunId) -> Result<Option<String>, SupervisorError> {
        Ok(self.require_run(run_id)?.checkpoint.clone())
    }

    pub fn set_escalation(
        &mut self,
        run_id: &RunId,
        escalation: EscalationPacketV1,
    ) -> Result<(), SupervisorError> {
        self.require_run_mut(run_id)?.escalation = Some(escalation);
        Ok(())
    }

    pub fn get_escalation(
        &self,
        run_id: &RunId,
    ) -> Result<Option<EscalationPacketV1>, SupervisorError> {
        Ok(self.require_run(run_id)?.escalation.clone())
    }

    pub fn consume_escalation_decision(
        &mut self,
        decision_id: &str,
    ) -> Result<(), SupervisorError> {
        if !self.consumed_decisions.insert(decision_id.to_string()) {
            return Err(SupervisorError::DecisionAlreadyUsed(decision_id.into()));
        }
        Ok(())
    }

    pub fn put_artifact(&mut self, artifact_id: ArtifactId, text: String) {
        self.artifacts
            .insert(artifact_id.as_str().to_string(), text);
    }

    pub fn get_artifact(
        &self,
        artifact_id: &ArtifactId,
        max_bytes: usize,
    ) -> Result<BoundedArtifactV1, SupervisorError> {
        let text = self
            .artifacts
            .get(artifact_id.as_str())
            .ok_or_else(|| SupervisorError::MissingArtifact(artifact_id.as_str().into()))?;
        Ok(bound_text(artifact_id.clone(), text, max_bytes))
    }

    pub fn put_patch(&mut self, patch: PatchProposalV1) {
        self.patches
            .insert(patch.patch_id.as_str().to_string(), patch);
    }

    pub fn get_patch(&self, patch_id: &PatchId) -> Result<PatchProposalV1, SupervisorError> {
        self.patches
            .get(patch_id.as_str())
            .cloned()
            .ok_or_else(|| SupervisorError::MissingPatch(patch_id.as_str().into()))
    }

    pub fn compare_patches(
        &self,
        parent_patch_id: &PatchId,
        candidate_patch_id: &PatchId,
    ) -> Result<PatchComparisonV1, SupervisorError> {
        Ok(compare_patches(
            &self.get_patch(parent_patch_id)?,
            &self.get_patch(candidate_patch_id)?,
        ))
    }

    pub fn put_diff(&mut self, diff: ActualDiffArtifactV1) {
        self.diffs.insert(diff.diff_hash.clone(), diff);
    }

    pub fn get_diff(
        &self,
        diff_hash: &str,
        max_bytes: usize,
    ) -> Result<BoundedArtifactV1, SupervisorError> {
        let diff = self
            .diffs
            .get(diff_hash)
            .ok_or_else(|| SupervisorError::MissingDiff(diff_hash.into()))?;
        Ok(bound_text(
            ArtifactId::new(diff_hash.replace(':', "_")).map_err(SupervisorError::MissingDiff)?,
            &diff.diff,
            max_bytes,
        ))
    }

    pub fn pause(&mut self, run_id: &RunId) -> Result<(), SupervisorError> {
        self.require_run_mut(run_id)?.state = RunState::Paused;
        Ok(())
    }

    pub fn resume(&mut self, run_id: &RunId) -> Result<(), SupervisorError> {
        self.require_run_mut(run_id)?.state = RunState::Running;
        Ok(())
    }

    pub fn cancel(&mut self, run_id: &RunId) -> Result<(), SupervisorError> {
        self.require_run_mut(run_id)?.state = RunState::Cancelled;
        Ok(())
    }

    pub fn set_final_review(
        &mut self,
        run_id: &RunId,
        review: FinalRunResultV1,
    ) -> Result<(), SupervisorError> {
        self.require_run_mut(run_id)?.final_review = Some(review);
        Ok(())
    }

    pub fn get_final_review(
        &self,
        run_id: &RunId,
    ) -> Result<Option<FinalRunResultV1>, SupervisorError> {
        Ok(self.require_run(run_id)?.final_review.clone())
    }

    fn require_run(&self, run_id: &RunId) -> Result<&SupervisorRunRecordV1, SupervisorError> {
        self.runs
            .get(run_id.as_str())
            .ok_or_else(|| SupervisorError::MissingRun(run_id.as_str().into()))
    }

    fn require_run_mut(
        &mut self,
        run_id: &RunId,
    ) -> Result<&mut SupervisorRunRecordV1, SupervisorError> {
        self.runs
            .get_mut(run_id.as_str())
            .ok_or_else(|| SupervisorError::MissingRun(run_id.as_str().into()))
    }
}

fn bound_text(artifact_id: ArtifactId, text: &str, max_bytes: usize) -> BoundedArtifactV1 {
    let truncated = text.len() > max_bytes;
    let mut bounded = text.to_string();
    if truncated {
        bounded.truncate(max_bytes);
        bounded.push_str("\n[truncated; full log remains local]");
    }
    BoundedArtifactV1 {
        artifact_id,
        byte_count: text.len(),
        text: bounded,
        truncated,
        local_only: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegated::contracts::{Confidence, EscalationPacketV1, TurnId};
    use crate::delegated::events::{EventPayloadV1, LifecycleEvent};
    use crate::delegated::patch_engine::{
        FileHashV1, PatchProposalV1, ReplaceOperationV1, stable_text_hash,
    };

    fn run_id() -> RunId {
        RunId::new("run-supervisor").expect("run id")
    }

    fn event(sequence: u64) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: 1,
            event_sequence: sequence,
            run_id: run_id(),
            worker_session_id: None,
            turn_id: None,
            item_id: None,
            lifecycle_event: LifecycleEvent::Delta,
            request_hash: "fnv1a64:req".into(),
            result_hash: None,
            payload: EventPayloadV1::Delta {
                text: format!("event {sequence}"),
            },
        }
        .with_result_hash()
        .expect("hash")
    }

    fn patch(id: &str, parent: Option<PatchId>, new: &str) -> PatchProposalV1 {
        PatchProposalV1 {
            schema_version: 1,
            patch_id: PatchId::new(id).expect("patch id"),
            parent_patch_id: parent,
            run_id: run_id(),
            turn_id: TurnId::new("turn-1").expect("turn id"),
            base_snapshot_hash: "fnv1a64:base".into(),
            target_paths: vec!["src/demo.txt".into()],
            expected_preimage_hashes: vec![FileHashV1 {
                path: "src/demo.txt".into(),
                hash: "fnv1a64:before".into(),
            }],
            operations: vec![ReplaceOperationV1 {
                path: "src/demo.txt".into(),
                old: "bad".into(),
                new: new.into(),
            }],
            model_rationale: "demo".into(),
            claimed_acceptance_criteria: vec!["works".into()],
        }
    }

    #[test]
    fn supervisor_tool_surface_lists_required_tools() {
        assert_eq!(SupervisorSurface::tool_names().len(), 13);
        assert!(SupervisorSurface::tool_names().contains(&"delegated_run_approve_start"));
        assert!(SupervisorSurface::tool_names().contains(&"delegated_run_events"));
        assert!(SupervisorSurface::tool_names().contains(&"delegated_run_cancel"));
        assert!(!SupervisorSurface::tool_names().contains(&"delegated_run_resume"));
        assert!(!SupervisorSurface::tool_names().contains(&"delegated_run_get_artifact"));
    }

    #[test]
    fn ordered_event_polling_is_idempotent() {
        let mut surface = SupervisorSurface::new();
        surface.create_run(run_id());
        surface.append_event(event(2));
        surface.append_event(event(1));
        let cursor = EventCursor {
            after_sequence: 0,
            limit: 2,
        };
        let first = surface.events(&run_id(), cursor.clone()).expect("events");
        let second = surface.events(&run_id(), cursor).expect("events again");
        assert_eq!(first, second);
        assert_eq!(first[0].event_sequence, 1);
        assert_eq!(first[1].event_sequence, 2);
    }

    #[test]
    fn artifact_and_diff_retrieval_are_bounded() {
        let mut surface = SupervisorSurface::new();
        let artifact_id = ArtifactId::new("artifact-log").expect("artifact id");
        surface.put_artifact(artifact_id.clone(), "x".repeat(100));
        let artifact = surface.get_artifact(&artifact_id, 10).expect("artifact");
        assert!(artifact.truncated);
        assert!(artifact.text.contains("truncated"));

        let diff = ActualDiffArtifactV1 {
            base_ref: "HEAD".into(),
            paths: vec!["src/demo.txt".into()],
            diff_hash: stable_text_hash("diff".repeat(50).as_str()),
            diff: "diff".repeat(50),
        };
        let diff_hash = diff.diff_hash.clone();
        surface.put_diff(diff);
        assert!(surface.get_diff(&diff_hash, 12).expect("diff").truncated);
    }

    #[test]
    fn patch_lineage_can_be_inspected() {
        let mut surface = SupervisorSurface::new();
        let parent = patch("patch-parent", None, "almost");
        let child = patch("patch-child", Some(parent.patch_id.clone()), "fixed");
        surface.put_patch(parent.clone());
        surface.put_patch(child.clone());
        let comparison = surface
            .compare_patches(&parent.patch_id, &child.patch_id)
            .expect("compare");
        assert_eq!(comparison.superseded_operations, 1);
    }

    #[test]
    fn escalation_decisions_cannot_be_reused() {
        let mut surface = SupervisorSurface::new();
        surface
            .consume_escalation_decision("decision-1")
            .expect("first decision");
        assert!(matches!(
            surface.consume_escalation_decision("decision-1"),
            Err(SupervisorError::DecisionAlreadyUsed(id)) if id == "decision-1"
        ));
    }

    #[test]
    fn pause_resume_cancel_and_final_review_work() {
        let mut surface = SupervisorSurface::new();
        let run_id = run_id();
        surface.create_run(run_id.clone());
        surface.start_run(&run_id).expect("start");
        surface.pause(&run_id).expect("pause");
        assert_eq!(surface.status(&run_id).expect("status"), RunState::Paused);
        surface.resume(&run_id).expect("resume");
        assert_eq!(surface.status(&run_id).expect("status"), RunState::Running);
        surface.cancel(&run_id).expect("cancel");
        assert_eq!(
            surface.status(&run_id).expect("status"),
            RunState::Cancelled
        );
    }

    #[test]
    fn checkpoint_escalation_and_final_review_are_retrievable() {
        let mut surface = SupervisorSurface::new();
        let run_id = run_id();
        surface.create_run(run_id.clone());
        surface
            .set_checkpoint(&run_id, "checkpoint text".into())
            .expect("checkpoint");
        assert_eq!(
            surface.get_checkpoint(&run_id).expect("checkpoint"),
            Some("checkpoint text".into())
        );
        surface
            .set_escalation(
                &run_id,
                EscalationPacketV1 {
                    schema_version: 1,
                    run_id: run_id.clone(),
                    blocking_question: "Question?".into(),
                    blocking_condition: "blocked".into(),
                    evidence: vec!["evidence".into()],
                    attempts: vec![],
                    changed_files: vec![],
                    tests_and_command_results: vec![],
                    options: vec![],
                    recommendation: "ask".into(),
                    confidence: Confidence::Medium,
                    current_checkpoint: "checkpoint".into(),
                },
            )
            .expect("escalation");
        assert!(
            surface
                .get_escalation(&run_id)
                .expect("escalation")
                .is_some()
        );
    }
}
