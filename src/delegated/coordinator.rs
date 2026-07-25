use std::collections::BTreeSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::EXECUTION_CONTRACT_SCHEMA_VERSION;
use super::contracts::{
    ApprovalRequestV1, Confidence, EscalationPacketV1, ExecutionContractV1, FinalRunResultV1,
    RunId, RunState, SupervisorDecisionV1, validate_contract,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReadinessV1 {
    pub run_id: RunId,
    pub ready: bool,
    pub missing_approvals: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunBudgetUsageV1 {
    pub turns_used: u32,
    pub tool_calls_used: u32,
    pub elapsed: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerificationStatusV1 {
    Passed,
    Failed,
    NotConfigured,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationSummaryV1 {
    pub status: VerificationStatusV1,
    pub command: String,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalReviewPackageV1 {
    pub schema_version: u32,
    pub run_id: RunId,
    pub final_result: FinalRunResultV1,
    pub verification: VerificationSummaryV1,
    pub actual_diff_summary: String,
    pub pushed: bool,
    pub merged: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CoordinatorError {
    Validation(String),
    RunAlreadyLocked(String),
    RunNotLocked(String),
    BudgetExhausted(String),
    Approval(String),
    VerificationRequired(String),
}

#[derive(Default)]
pub struct RunCoordinator {
    locked_runs: BTreeSet<String>,
}

impl RunCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn readiness(
        &self,
        contract: &ExecutionContractV1,
    ) -> Result<RunReadinessV1, CoordinatorError> {
        validate_contract(contract).map_err(CoordinatorError::Validation)?;
        let missing_approvals = contract
            .approval_requirements
            .iter()
            .filter(|approval| approval.required)
            .map(|approval| format!("{:?}: {}", approval.kind, approval.reason))
            .collect::<Vec<_>>();
        Ok(RunReadinessV1 {
            run_id: RunId::new(contract.task_id.clone()).map_err(CoordinatorError::Validation)?,
            ready: missing_approvals.is_empty(),
            missing_approvals,
            warnings: Vec::new(),
        })
    }

    pub fn acquire_lock(&mut self, run_id: &RunId) -> Result<(), CoordinatorError> {
        if !self.locked_runs.insert(run_id.as_str().to_string()) {
            return Err(CoordinatorError::RunAlreadyLocked(run_id.as_str().into()));
        }
        Ok(())
    }

    pub fn release_lock(&mut self, run_id: &RunId) -> Result<(), CoordinatorError> {
        if !self.locked_runs.remove(run_id.as_str()) {
            return Err(CoordinatorError::RunNotLocked(run_id.as_str().into()));
        }
        Ok(())
    }

    pub fn enforce_budget(
        &self,
        contract: &ExecutionContractV1,
        usage: &RunBudgetUsageV1,
    ) -> Result<(), CoordinatorError> {
        if usage.turns_used >= contract.max_turns {
            return Err(CoordinatorError::BudgetExhausted(
                "turn budget exhausted".into(),
            ));
        }
        if usage.tool_calls_used >= contract.max_tool_calls {
            return Err(CoordinatorError::BudgetExhausted(
                "tool-call budget exhausted".into(),
            ));
        }
        if usage.elapsed >= Duration::from_secs(contract.max_elapsed_seconds) {
            return Err(CoordinatorError::BudgetExhausted(
                "elapsed-time budget exhausted".into(),
            ));
        }
        Ok(())
    }

    pub fn validate_supervisor_decision(
        &self,
        approval: &ApprovalRequestV1,
        decision: &SupervisorDecisionV1,
        current_sequence: u64,
    ) -> Result<(), CoordinatorError> {
        approval
            .validate_decision(decision, current_sequence)
            .map_err(CoordinatorError::Approval)
    }

    pub fn escalation_packet(
        &self,
        run_id: RunId,
        blocking_question: &str,
        blocking_condition: &str,
        evidence: Vec<String>,
        attempts: Vec<String>,
        recommendation: &str,
    ) -> EscalationPacketV1 {
        EscalationPacketV1 {
            schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
            run_id,
            blocking_question: blocking_question.into(),
            blocking_condition: blocking_condition.into(),
            evidence,
            attempts,
            changed_files: Vec::new(),
            tests_and_command_results: Vec::new(),
            options: vec!["continue with narrowed scope".into(), "cancel run".into()],
            recommendation: recommendation.into(),
            confidence: Confidence::Medium,
            current_checkpoint: "coordinator escalation".into(),
        }
    }

    pub fn cancel_run(
        &mut self,
        run_id: &RunId,
        reason: &str,
    ) -> Result<RunState, CoordinatorError> {
        if reason.trim().is_empty() {
            return Err(CoordinatorError::Validation(
                "cancellation reason is required".into(),
            ));
        }
        let _ = self.locked_runs.remove(run_id.as_str());
        Ok(RunState::Cancelled)
    }

    pub fn final_review_package(
        &self,
        run_id: RunId,
        result: FinalRunResultV1,
        verification: VerificationSummaryV1,
        actual_diff_summary: String,
    ) -> Result<FinalReviewPackageV1, CoordinatorError> {
        if verification.status != VerificationStatusV1::Passed {
            return Err(CoordinatorError::VerificationRequired(
                "final completion requires passed verification".into(),
            ));
        }
        if result.status != RunState::CompletedVerified {
            return Err(CoordinatorError::VerificationRequired(
                "final result must be COMPLETED_VERIFIED".into(),
            ));
        }
        if actual_diff_summary.trim().is_empty() {
            return Err(CoordinatorError::VerificationRequired(
                "actual diff summary is required".into(),
            ));
        }
        Ok(FinalReviewPackageV1 {
            schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
            run_id,
            final_result: result,
            verification,
            actual_diff_summary,
            pushed: false,
            merged: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegated::contracts::{
        ApprovalId, ApprovalRequestV1, SupervisorDecisionKind, SupervisorDecisionV1,
    };

    fn fixture_contract() -> ExecutionContractV1 {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/delegated/execution_contract_v1.json"
        ))
        .expect("fixture parses")
    }

    fn final_result(status: RunState) -> FinalRunResultV1 {
        FinalRunResultV1 {
            schema_version: 1,
            status,
            objective: "finish task".into(),
            branch: "orchestrator/v1-coding-sprint".into(),
            files_changed: vec!["src/delegated/coordinator.rs".into()],
            verification: "cargo test passed".into(),
            diff_artifacts: vec![],
            provider_history: vec!["fake".into()],
            unresolved_warnings: vec![],
            final_recommendation: "review".into(),
        }
    }

    #[test]
    fn readiness_requires_start_approval_when_contract_says_so() {
        let coordinator = RunCoordinator::new();
        let readiness = coordinator
            .readiness(&fixture_contract())
            .expect("readiness");
        assert!(!readiness.ready);
        assert!(readiness.missing_approvals[0].contains("Supervisor"));
    }

    #[test]
    fn ambiguity_escalates_with_evidence() {
        let coordinator = RunCoordinator::new();
        let packet = coordinator.escalation_packet(
            RunId::new("run-ambiguous").expect("run id"),
            "Which file should be changed?",
            "ambiguous scope",
            vec!["two matching modules".into()],
            vec!["searched src".into()],
            "ask supervisor",
        );
        assert_eq!(packet.blocking_condition, "ambiguous scope");
        assert!(packet.evidence[0].contains("two matching"));
    }

    #[test]
    fn run_locking_blocks_duplicate_active_runs() {
        let mut coordinator = RunCoordinator::new();
        let run_id = RunId::new("run-lock").expect("run id");
        coordinator.acquire_lock(&run_id).expect("first lock");
        assert!(matches!(
            coordinator.acquire_lock(&run_id),
            Err(CoordinatorError::RunAlreadyLocked(id)) if id == "run-lock"
        ));
        coordinator.release_lock(&run_id).expect("release");
    }

    #[test]
    fn budget_exhaustion_stops_safely() {
        let coordinator = RunCoordinator::new();
        let contract = fixture_contract();
        let usage = RunBudgetUsageV1 {
            turns_used: contract.max_turns,
            tool_calls_used: 0,
            elapsed: Duration::from_secs(1),
        };
        assert!(matches!(
            coordinator.enforce_budget(&contract, &usage),
            Err(CoordinatorError::BudgetExhausted(message)) if message.contains("turn")
        ));
    }

    #[test]
    fn stale_approvals_fail() {
        let coordinator = RunCoordinator::new();
        let run_id = RunId::new("run-approval").expect("run id");
        let approval = ApprovalRequestV1 {
            approval_id: ApprovalId::new("approval-1").expect("approval id"),
            run_id: run_id.clone(),
            request_hash: "fnv1a64:req".into(),
            reason: "mutating patch".into(),
            expires_at_sequence: 3,
            consumed: false,
        };
        let decision = SupervisorDecisionV1 {
            schema_version: 1,
            decision_id: "decision-1".into(),
            run_id,
            approval_id: Some(approval.approval_id.clone()),
            request_hash: approval.request_hash.clone(),
            kind: SupervisorDecisionKind::ApproveNarrowException,
            rationale: "approved".into(),
            expires_at_sequence: 3,
        };
        assert!(
            coordinator
                .validate_supervisor_decision(&approval, &decision, 4)
                .is_err()
        );
    }

    #[test]
    fn final_completion_requires_verification() {
        let coordinator = RunCoordinator::new();
        let verification = VerificationSummaryV1 {
            status: VerificationStatusV1::Failed,
            command: "cargo test".into(),
            summary: "failed".into(),
        };
        assert!(matches!(
            coordinator.final_review_package(
                RunId::new("run-final").expect("run id"),
                final_result(RunState::CompletedVerified),
                verification,
                "diff".into()
            ),
            Err(CoordinatorError::VerificationRequired(_))
        ));
    }

    #[test]
    fn final_review_never_marks_push_or_merge() {
        let coordinator = RunCoordinator::new();
        let verification = VerificationSummaryV1 {
            status: VerificationStatusV1::Passed,
            command: "cargo test".into(),
            summary: "passed".into(),
        };
        let package = coordinator
            .final_review_package(
                RunId::new("run-final").expect("run id"),
                final_result(RunState::CompletedVerified),
                verification,
                "diff --git".into(),
            )
            .expect("final package");
        assert!(!package.pushed);
        assert!(!package.merged);
    }

    #[test]
    fn cancellation_releases_lock() {
        let mut coordinator = RunCoordinator::new();
        let run_id = RunId::new("run-cancel").expect("run id");
        coordinator.acquire_lock(&run_id).expect("lock");
        assert_eq!(
            coordinator
                .cancel_run(&run_id, "user requested")
                .expect("cancel"),
            RunState::Cancelled
        );
        assert!(matches!(
            coordinator.release_lock(&run_id),
            Err(CoordinatorError::RunNotLocked(_))
        ));
    }
}
