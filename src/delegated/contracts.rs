use serde::{Deserialize, Serialize};

use super::{EXECUTION_CONTRACT_SCHEMA_VERSION, EXECUTION_PROTOCOL_VERSION};

const MAX_OBJECTIVE_CHARS: usize = 8_000;
const MAX_PATH_CHARS: usize = 260;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, String> {
                let value = value.into();
                validate_identifier(stringify!($name), &value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

id_type!(RunId);
id_type!(WorkerSessionId);
id_type!(TurnId);
id_type!(ItemId);
id_type!(ToolCallId);
id_type!(ApprovalId);
id_type!(ArtifactId);
id_type!(PatchId);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RunState {
    Draft,
    AwaitingApproval,
    Ready,
    Starting,
    Running,
    Paused,
    NeedsSupervisor,
    CancelRequested,
    Verifying,
    CompletedVerified,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionContractV1 {
    pub schema_version: u32,
    pub task_id: String,
    pub objective: String,
    pub workspace: String,
    pub feature_branch: String,
    pub allowed_paths: Vec<String>,
    pub forbidden_paths: Vec<String>,
    pub allowed_command_profiles: Vec<String>,
    pub ordered_steps: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub retry_budget: u32,
    pub max_turns: u32,
    pub max_tool_calls: u32,
    pub max_elapsed_seconds: u64,
    pub provider_policy: ProviderPolicyV1,
    pub escalation_conditions: Vec<String>,
    pub approval_requirements: Vec<ApprovalRequirementV1>,
    pub expected_artifacts: Vec<ExpectedArtifactV1>,
    pub verification_profile: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPolicyV1 {
    pub primary_provider_id: String,
    pub primary_model_id: String,
    pub fallback_provider_ids: Vec<String>,
    pub require_tool_calls: bool,
    pub allow_paid_fallbacks: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ApprovalRequirementKind {
    RunStart,
    ToolException,
    UnrestrictedShell,
    GitPush,
    Merge,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequirementV1 {
    pub kind: ApprovalRequirementKind,
    pub required: bool,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExpectedArtifactKind {
    Diff,
    Verification,
    Escalation,
    FinalReview,
    Log,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedArtifactV1 {
    pub kind: ExpectedArtifactKind,
    pub name: String,
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolHandshakeV1 {
    pub protocol_version: String,
    pub contract_schema_version: u32,
    pub worker_runtime: String,
    pub supports_delta_events: bool,
    pub supports_event_cursor: bool,
}

impl ProtocolHandshakeV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.protocol_version != EXECUTION_PROTOCOL_VERSION {
            return Err(format!(
                "unsupported protocol_version `{}`; expected `{}`",
                self.protocol_version, EXECUTION_PROTOCOL_VERSION
            ));
        }
        if self.contract_schema_version != EXECUTION_CONTRACT_SCHEMA_VERSION {
            return Err(format!(
                "unsupported contract_schema_version `{}`; expected `{}`",
                self.contract_schema_version, EXECUTION_CONTRACT_SCHEMA_VERSION
            ));
        }
        validate_non_empty("worker_runtime", &self.worker_runtime)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EscalationPacketV1 {
    pub schema_version: u32,
    pub run_id: RunId,
    pub blocking_question: String,
    pub blocking_condition: String,
    pub evidence: Vec<String>,
    pub attempts: Vec<String>,
    pub changed_files: Vec<String>,
    pub tests_and_command_results: Vec<String>,
    pub options: Vec<String>,
    pub recommendation: String,
    pub confidence: Confidence,
    pub current_checkpoint: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SupervisorDecisionKind {
    Continue,
    ModifyPlan,
    ApproveNarrowException,
    Rollback,
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorDecisionV1 {
    pub schema_version: u32,
    pub decision_id: String,
    pub run_id: RunId,
    pub approval_id: Option<ApprovalId>,
    pub request_hash: String,
    pub kind: SupervisorDecisionKind,
    pub rationale: String,
    pub expires_at_sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequestV1 {
    pub approval_id: ApprovalId,
    pub run_id: RunId,
    pub request_hash: String,
    pub reason: String,
    pub expires_at_sequence: u64,
    pub consumed: bool,
}

impl ApprovalRequestV1 {
    pub fn validate_decision(
        &self,
        decision: &SupervisorDecisionV1,
        current_sequence: u64,
    ) -> Result<(), String> {
        if decision.schema_version != EXECUTION_CONTRACT_SCHEMA_VERSION {
            return Err("unsupported supervisor decision schema_version".into());
        }
        if decision.run_id != self.run_id {
            return Err("supervisor decision is for a different run".into());
        }
        if decision.approval_id.as_ref() != Some(&self.approval_id) {
            return Err("supervisor decision is not tied to this approval request".into());
        }
        if decision.request_hash != self.request_hash {
            return Err("supervisor decision request_hash does not match approval request".into());
        }
        if self.consumed {
            return Err("approval request was already consumed".into());
        }
        if current_sequence > self.expires_at_sequence
            || current_sequence > decision.expires_at_sequence
        {
            return Err("supervisor decision or approval request is stale".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalRunResultV1 {
    pub schema_version: u32,
    pub status: RunState,
    pub objective: String,
    pub branch: String,
    pub files_changed: Vec<String>,
    pub verification: String,
    pub diff_artifacts: Vec<ArtifactId>,
    pub provider_history: Vec<String>,
    pub unresolved_warnings: Vec<String>,
    pub final_recommendation: String,
}

pub fn stable_hash<T: Serialize>(value: &T) -> Result<String, String> {
    let json = serde_json::to_string(value).map_err(|err| err.to_string())?;
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in json.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok(format!("fnv1a64:{hash:016x}"))
}

pub fn validate_contract(contract: &ExecutionContractV1) -> Result<(), String> {
    if contract.schema_version != EXECUTION_CONTRACT_SCHEMA_VERSION {
        return Err(format!(
            "unsupported schema_version `{}`; expected `{}`",
            contract.schema_version, EXECUTION_CONTRACT_SCHEMA_VERSION
        ));
    }
    validate_non_empty("task_id", &contract.task_id)?;
    validate_non_empty("objective", &contract.objective)?;
    if contract.objective.len() > MAX_OBJECTIVE_CHARS {
        return Err("objective is too large".into());
    }
    validate_non_empty("workspace", &contract.workspace)?;
    validate_branch_name(&contract.feature_branch)?;
    validate_non_empty_list("allowed_paths", &contract.allowed_paths)?;
    validate_non_empty_list(
        "allowed_command_profiles",
        &contract.allowed_command_profiles,
    )?;
    validate_non_empty_list("ordered_steps", &contract.ordered_steps)?;
    validate_non_empty_list("acceptance_criteria", &contract.acceptance_criteria)?;
    validate_non_empty("verification_profile", &contract.verification_profile)?;
    validate_positive_budget("retry_budget", contract.retry_budget as u64)?;
    validate_positive_budget("max_turns", contract.max_turns as u64)?;
    validate_positive_budget("max_tool_calls", contract.max_tool_calls as u64)?;
    validate_positive_budget("max_elapsed_seconds", contract.max_elapsed_seconds)?;
    contract.provider_policy.validate()?;
    for path in &contract.allowed_paths {
        validate_contract_path(path, false)?;
    }
    for path in &contract.forbidden_paths {
        validate_contract_path(path, true)?;
    }
    for path in contract.changed_file_candidates() {
        validate_contract_path(path, false)?;
    }
    Ok(())
}

impl ExecutionContractV1 {
    fn changed_file_candidates(&self) -> impl Iterator<Item = &String> {
        std::iter::empty()
    }

    pub fn request_hash(&self) -> Result<String, String> {
        validate_contract(self)?;
        stable_hash(self)
    }

    pub fn ensure_same_immutable_intent(
        &self,
        original: &ExecutionContractV1,
    ) -> Result<(), String> {
        if self.schema_version != original.schema_version
            || self.task_id != original.task_id
            || self.objective != original.objective
            || self.workspace != original.workspace
            || self.feature_branch != original.feature_branch
            || self.allowed_paths != original.allowed_paths
            || self.forbidden_paths != original.forbidden_paths
            || self.acceptance_criteria != original.acceptance_criteria
        {
            return Err(
                "worker output attempted to alter immutable execution contract intent".into(),
            );
        }
        Ok(())
    }
}

impl ProviderPolicyV1 {
    fn validate(&self) -> Result<(), String> {
        validate_non_empty("primary_provider_id", &self.primary_provider_id)?;
        validate_non_empty("primary_model_id", &self.primary_model_id)?;
        for provider in &self.fallback_provider_ids {
            validate_non_empty("fallback_provider_id", provider)?;
        }
        Ok(())
    }
}

pub fn validate_branch_name(branch: &str) -> Result<(), String> {
    let branch = branch.trim();
    if branch.is_empty()
        || branch.starts_with('-')
        || branch.ends_with('/')
        || branch.contains("..")
        || branch.contains("//")
    {
        return Err("invalid feature branch".into());
    }
    if !branch
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.'))
    {
        return Err(
            "feature branch may contain only ASCII letters, numbers, '/', '-', '_', and '.'".into(),
        );
    }
    Ok(())
}

fn validate_contract_path(path: &str, allow_forbidden_protected_path: bool) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_PATH_CHARS {
        return Err(format!("invalid contract path `{path}`"));
    }
    if trimmed.starts_with('-')
        || trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.contains(':')
        || trimmed.contains('\0')
        || trimmed
            .split(['/', '\\'])
            .any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(format!(
            "contract path must be a relative contained path: `{path}`"
        ));
    }
    let normalized = trimmed.replace('\\', "/").to_ascii_lowercase();
    if !allow_forbidden_protected_path && (normalized == ".git" || normalized.starts_with(".git/"))
    {
        return Err("contract paths may not target .git".into());
    }
    Ok(())
}

fn validate_identifier(label: &str, value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > 120
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':' | '.'))
    {
        return Err(format!(
            "{label} must be a conservative non-empty identifier"
        ));
    }
    Ok(())
}

fn validate_non_empty(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    Ok(())
}

fn validate_non_empty_list(label: &str, values: &[String]) -> Result<(), String> {
    if values.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    for value in values {
        validate_non_empty(label, value)?;
    }
    Ok(())
}

fn validate_positive_budget(label: &str, value: u64) -> Result<(), String> {
    if value == 0 {
        return Err(format!("{label} must be greater than zero"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_contract() -> ExecutionContractV1 {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/delegated/execution_contract_v1.json"
        ))
        .expect("fixture parses")
    }

    #[test]
    fn execution_contract_round_trips_and_validates() {
        let contract = fixture_contract();
        validate_contract(&contract).expect("valid contract");
        let json = serde_json::to_string_pretty(&contract).expect("serialize");
        let reparsed: ExecutionContractV1 = serde_json::from_str(&json).expect("reparse");
        assert_eq!(contract, reparsed);
        assert_eq!(
            contract.request_hash().expect("hash"),
            reparsed.request_hash().expect("hash")
        );
    }

    #[test]
    fn unknown_schema_versions_are_rejected() {
        let mut contract = fixture_contract();
        contract.schema_version = 2;
        assert!(
            validate_contract(&contract)
                .unwrap_err()
                .contains("unsupported schema_version")
        );
    }

    #[test]
    fn validates_paths_budgets_and_branch_names() {
        let mut contract = fixture_contract();
        contract.allowed_paths.push("../secret".into());
        assert!(validate_contract(&contract).is_err());

        let mut contract = fixture_contract();
        contract.max_turns = 0;
        assert!(validate_contract(&contract).is_err());

        let mut contract = fixture_contract();
        contract.feature_branch = "bad branch".into();
        assert!(validate_contract(&contract).is_err());
    }

    #[test]
    fn worker_output_cannot_change_immutable_objective() {
        let original = fixture_contract();
        let mut echoed = original.clone();
        echoed.objective = "Do something else".into();
        assert!(echoed.ensure_same_immutable_intent(&original).is_err());
    }

    #[test]
    fn approvals_are_tied_to_one_run_request_and_expiration() {
        let run_id = RunId::new("run-t0013").expect("run id");
        let approval = ApprovalRequestV1 {
            approval_id: ApprovalId::new("approval-1").expect("approval id"),
            run_id: run_id.clone(),
            request_hash: "fnv1a64:abc".into(),
            reason: "Need narrow exception".into(),
            expires_at_sequence: 10,
            consumed: false,
        };
        let decision = SupervisorDecisionV1 {
            schema_version: 1,
            decision_id: "decision-1".into(),
            run_id,
            approval_id: Some(approval.approval_id.clone()),
            request_hash: approval.request_hash.clone(),
            kind: SupervisorDecisionKind::ApproveNarrowException,
            rationale: "Approved for one path".into(),
            expires_at_sequence: 10,
        };
        assert!(approval.validate_decision(&decision, 9).is_ok());
        assert!(approval.validate_decision(&decision, 11).is_err());
    }
}
