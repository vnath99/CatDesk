//! Durable, fail-closed MCP supervisor operations for autonomous Codex work.
//!
//! This surface persists only contract-governed control-plane facts. Operator
//! configuration such as the direct Codex executable remains outside MCP.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::core_host_gate_approval::{
    CORE_HOST_GATE_APPROVAL_PURPOSE, CORE_HOST_GATE_APPROVAL_REVIEW_ARTIFACT_ID,
    CoreHostGateApprovalRequestV1, canonical_request_sha256, parse_review_artifact,
    review_artifact_binds_request, validate_request,
};
use crate::daemon_reload_approval::{
    DAEMON_RELOAD_APPROVAL_PRODUCT, DAEMON_RELOAD_APPROVAL_PROJECT, DAEMON_RELOAD_APPROVAL_PURPOSE,
    DAEMON_RELOAD_APPROVAL_REVIEW_ARTIFACT_ID, DAEMON_RELOAD_APPROVAL_SCHEMA_VERSION,
    DaemonReloadApprovalRequestV1, DaemonReloadReviewAuthorityV1,
    canonical_request_sha256 as canonical_daemon_reload_request_sha256,
    parse_review_artifact as parse_daemon_reload_review_artifact,
    review_artifact_binds_request as daemon_reload_artifact_binds_request,
    validate_authority as validate_daemon_reload_authority,
    validate_request as validate_daemon_reload_request,
};
use crate::ordinary_worker_pair_approval::{
    ORDINARY_WORKER_PAIR_APPROVAL_PURPOSE, ORDINARY_WORKER_PAIR_APPROVAL_REVIEW_ARTIFACT_ID,
    OrdinaryWorkerPairApprovalRequestV1, OrdinaryWorkerPairDescriptorV1,
    canonical_pair_request_sha256,
    parse_review_artifact as parse_ordinary_worker_pair_review_artifact,
    review_artifact_binds_request as ordinary_worker_pair_artifact_binds_request,
    validate_pair_request as validate_ordinary_worker_pair_request,
};
use crate::reviewed_build::validate_producer_attestation;
use crate::reviewed_source_snapshot::{
    ReviewedSourceBaselineObservationV1, ReviewedSourceCurrentOutputV1,
    ReviewedSourceSnapshotExpectedV1, validate_committed_snapshot,
};
use crate::task_queue;

use super::autonomous_contract::{
    AutonomousDevelopmentContractV1, AutonomousPolicyEngineV1, runtime_capability_manifest,
};
use super::autonomous_controller::{
    RestartTaskReconciliationV1, reconcile_restart_task_state, safe_task_output_hash,
};
use super::autonomy_projects::{
    AutonomousProjectRegistryStoreV1, AutonomousProjectV1, PROJECT_REGISTRY_SCHEMA_VERSION,
};
use super::autonomy_state::{
    AUTONOMY_STATE_SCHEMA_VERSION, AutonomousPlanQueueV1, AutonomousPlannedTaskV1,
    AutonomousProviderRouteV1, AutonomousQueueTaskStateV1, AutonomousQueueTaskV1,
    AutonomousQueueV1, AutonomousSessionSnapshotV1, AutonomousSessionStateV1,
    AutonomousStateStoreV1, AutonomousTaskOutputObservationV1, AutonomousWakeModeV1,
    validate_exact_task_graph_materialization,
};
use super::github_bootstrap::{GithubBootstrapStore, SystemGithubBootstrapRunner};
use super::github_publication::{
    CURRENT_GITHUB_PUBLICATION_AUTHORITY_PATH, parse_current_github_publication_authority,
    parse_github_publication_descriptor,
};
use super::github_publication_executor::{
    PublicationReviewAuthorityV1, SystemGithubPublicationGitRunnerV1,
    confirm_local_commit as confirm_github_publication_local_commit,
    prepare as prepare_github_publication, push as push_github_publication,
    reconcile as reconcile_github_publication,
};

pub const AUTONOMY_MCP_TOOL_NAMES: [&str; 42] = [
    "autonomy_contract_create",
    "autonomy_contract_validate",
    "autonomy_runtime_capabilities",
    "autonomy_contract_approve",
    "autonomy_session_start",
    "autonomy_session_status",
    "autonomy_session_events",
    "autonomy_session_reply",
    "autonomy_session_claim_direct_work",
    "autonomy_session_finalize_direct_work",
    "autonomy_session_pause",
    "autonomy_session_resume",
    "autonomy_session_cancel",
    "autonomy_session_renew_lease",
    "autonomy_session_get_checkpoint",
    "autonomy_session_get_diff",
    "autonomy_session_get_escalation",
    "autonomy_session_get_final_review",
    "autonomy_session_list",
    "provider_status",
    "autonomy_queue_status",
    "autonomy_execution_accounting",
    "autonomy_work_time_report",
    "autonomy_wake_policy_get",
    "autonomy_wake_policy_set",
    "autonomy_ticket_audit",
    "autonomy_session_supersede",
    "autonomy_review_inbox_list",
    "autonomy_review_inbox_ack",
    "autonomy_project_registry_read",
    "autonomy_project_registry_bind",
    "autonomy_project_registry_preflight",
    "autonomy_project_registry_confirm",
    "autonomy_project_registry_chat_target_bind",
    "autonomy_project_thread_adoption_preflight",
    "autonomy_project_thread_adoption_confirm",
    "catdesk_codex_goal_resume",
    "catdesk_daemon_reload",
    "catdesk_release_recovery",
    "catdesk_reviewed_build",
    "catdesk_reviewed_build_promotion",
    "catdesk_github_publication",
];

/// Only bounded approval evidence is retained by the promotion preflight.
/// The digest is recomputed from durable state immediately before the worker
/// authorization is issued, preventing a review/completion TOCTOU.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewedPromotionReviewAuthorityEvidenceV1 {
    record_id: String,
    session_id: String,
    project_id: String,
    state: String,
    next_action: String,
    reference: String,
    approved_contract_hash: String,
    logical_task_id: String,
    completion_verification: String,
    final_review_sha256: String,
    completion_artifact_ids: Vec<String>,
    baseline_observations: Vec<AutonomousTaskOutputObservationV1>,
    current_observations: Vec<AutonomousTaskOutputObservationV1>,
}

#[derive(Clone, Debug)]
struct ReviewedPromotionReviewAuthorityV1 {
    session_id: String,
    record_id: String,
    digest: String,
    approved_contract_hash: String,
    completion_final_review_sha256: String,
    snapshot_expected: ReviewedSourceSnapshotExpectedV1,
}

/// Purpose-separated identity derived only after the existing acknowledged
/// independent-review remeasurement succeeds. It deliberately carries no
/// source-snapshot authority and is not an approval by itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CoreHostGateReviewAuthorityV1 {
    pub(crate) schema_version: u32,
    pub(crate) product: String,
    pub(crate) project_id: String,
    pub(crate) purpose: String,
    pub(crate) request_sha256: String,
    pub(crate) review_record_id: String,
    pub(crate) review_session_id: String,
    pub(crate) review_contract_hash: String,
    pub(crate) review_completion_sha256: String,
    pub(crate) remeasurement_digest: String,
    pub(crate) authority_sha256: String,
}

/// Purpose-separated identity for one exact ordinary-worker pair request.
/// It has no artifact bytes, write, activation, or main-image authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OrdinaryWorkerPairReviewAuthorityV1 {
    pub(crate) schema_version: u32,
    pub(crate) product: String,
    pub(crate) project_id: String,
    pub(crate) purpose: String,
    pub(crate) request_sha256: String,
    pub(crate) review_record_id: String,
    pub(crate) review_session_id: String,
    pub(crate) review_contract_hash: String,
    pub(crate) review_completion_sha256: String,
    pub(crate) remeasurement_digest: String,
    pub(crate) authority_sha256: String,
}

/// Closed read-only bridge for T-0311. No MCP tool exposes it; future code can
/// only obtain a purpose-bound identity after the acknowledged-review path
/// revalidates completion and current attributed output state.
pub(crate) fn resolve_core_host_gate_review_authority(
    workspace: &Path,
    record_id: &str,
    request: &CoreHostGateApprovalRequestV1,
) -> Result<CoreHostGateReviewAuthorityV1, String> {
    AutonomousSupervisorV1::open(workspace)?
        .resolve_core_host_gate_review_authority(record_id, request)
}

/// Closed read-only bridge for the T-0387 pair-approval domain. No MCP tool
/// exposes it; only a current immutable reviewed completion artifact can bind
/// a request before downstream code receives this identity.
pub(crate) fn resolve_ordinary_worker_pair_review_authority(
    workspace: &Path,
    record_id: &str,
    request: &OrdinaryWorkerPairApprovalRequestV1,
) -> Result<OrdinaryWorkerPairReviewAuthorityV1, String> {
    AutonomousSupervisorV1::open(workspace)?
        .resolve_ordinary_worker_pair_review_authority(record_id, request)
}

pub fn is_autonomy_mcp_tool(name: &str) -> bool {
    AUTONOMY_MCP_TOOL_NAMES.contains(&name)
}

pub fn tool_schemas() -> Vec<Value> {
    AUTONOMY_MCP_TOOL_NAMES
        .iter()
        .map(|name| {
            let read_only = matches!(
                *name,
                "autonomy_contract_validate"
                    | "autonomy_runtime_capabilities"
                    | "autonomy_session_status"
                    | "autonomy_session_events"
                    | "autonomy_session_get_checkpoint"
                    | "autonomy_session_get_diff"
                    | "autonomy_session_get_escalation"
                    | "autonomy_session_get_final_review"
                    | "autonomy_session_list"
                    | "provider_status"
                    | "autonomy_queue_status"
                    | "autonomy_execution_accounting"
                    | "autonomy_work_time_report"
                    | "autonomy_wake_policy_get"
                    | "autonomy_ticket_audit"
                    | "autonomy_review_inbox_list"
                    | "autonomy_project_registry_read"
            );
            let mut input_schema = json!({
                    "type": "object",
                    "properties": {
                        "contract": { "type": "object" },
                        "sessionId": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" },
                        "expectedStateVersion": { "type": "integer", "minimum": 0 },
                        "idempotencyKey": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" },
                        "approvalId": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" },
                        "escalationId": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" },
                        "decisionHash": { "type": "string", "minLength": 1, "maxLength": 256 },
                        "afterSequence": { "type": "integer", "minimum": 0 },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
                        "maxBytes": { "type": "integer", "minimum": 1, "maximum": 24000 },
                        "decision": { "type": "string", "minLength": 1, "maxLength": 4000 },
                        "constraints": { "type": "array", "maxItems": 20, "items": {"type":"string","maxLength":512} },
                        "newExpiryUnix": { "type": "integer", "minimum": 1 }
                        ,"projectId": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" }
                        ,"workspace": { "type": "string", "minLength": 1, "maxLength": 4096 }
                        ,"conversationUrl": { "type": "string", "minLength": 1, "maxLength": 512 }
                        ,"expectedCurrentTargetSha256": { "type": "string", "pattern": "^[A-Fa-f0-9]{64}$" }
                        ,"afterRecordId": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" }
                        ,"recordId": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" }
                        ,"threadId": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" }
                        ,"gitIdentity": { "type": "string", "minLength": 1, "maxLength": 1024 }
                        ,"verificationProfile": { "type": "string", "minLength": 1, "maxLength": 256 }
                        ,"resolutionEvidence": { "type": "string", "enum": ["exact-unowned-direct-input"] }
                        ,"buildPath": { "type": "string", "minLength": 1, "maxLength": 4096 }
                        ,"expectedSha256": { "type": "string", "pattern": "^[A-Fa-f0-9]{64}$" }
                        ,"dryRun": { "type": "boolean" }
                        ,"confirmationToken": { "type": "string", "minLength": 1, "maxLength": 256 }
                        ,"candidateHandle": { "type": "string", "minLength": 1, "maxLength": 128 }
                        ,"startUnix": { "type": "integer", "minimum": 1 }
                        ,"endUnix": { "type": "integer", "minimum": 1 }
                        ,"afterSessionId": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" }
                        ,"supersededSessionId": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" }
                        ,"supersedingSessionId": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" }
                        ,"expectedGeneration": { "type": "integer", "minimum": 0 }
                        ,"mode": { "type": "string", "enum": ["MANUAL_OFF","INDEFINITE","THROUGH_TASK","UNTIL_PROVIDER_EXHAUSTED"] }
                        ,"terminalTaskId": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" }
                        ,"taskId": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" }
                        ,"action": { "type": "string", "enum": ["PREFLIGHT","CONFIRM","RESULT"] }
                    }
                });
            if *name == "catdesk_codex_goal_resume" {
                input_schema = json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["titles"],
                    "properties": {
                        "titles": {
                            "type": "array",
                            "minItems": 1,
                            "maxItems": 8,
                            "uniqueItems": true,
                            "items": {"type":"string","minLength":1,"maxLength":512}
                        }
                    }
                });
            }
            if *name == "autonomy_project_registry_bind" {
                input_schema["oneOf"] = json!([
                    {"required":["projectId","threadId","gitIdentity","verificationProfile","resolutionEvidence"], "not":{"anyOf":[{"required":["conversationUrl"]},{"required":["expectedCurrentTargetSha256"]},{"required":["decision"]},{"required":["expectedSha256"]}]}},
                    {"required":["projectId","conversationUrl"], "not":{"anyOf":[{"required":["threadId"]},{"required":["gitIdentity"]},{"required":["verificationProfile"]},{"required":["resolutionEvidence"]},{"required":["workspace"]}]}},
                    {"required":["projectId","decision","expectedSha256"],"properties":{"decision":{"pattern":"^CHAT_TARGET_URL=\\S+$"}}, "not":{"anyOf":[{"required":["conversationUrl"]},{"required":["expectedCurrentTargetSha256"]},{"required":["threadId"]},{"required":["gitIdentity"]},{"required":["verificationProfile"]},{"required":["resolutionEvidence"]},{"required":["workspace"]}]}},
                    {"required":["projectId","decision","expectedSha256"],"properties":{"decision":{"pattern":"^DESIGNATED_CHAT_TARGET_URL=\\S+$"}}, "not":{"anyOf":[{"required":["conversationUrl"]},{"required":["expectedCurrentTargetSha256"]},{"required":["threadId"]},{"required":["gitIdentity"]},{"required":["verificationProfile"]},{"required":["resolutionEvidence"]},{"required":["workspace"]}]}},
                    {"required":["projectId","decision"],"properties":{"decision":{"pattern":"^GITHUB_BOOTSTRAP_PREFLIGHT_WORKSPACE=\\S+$"}}, "not":{"anyOf":[{"required":["expectedSha256"]},{"required":["conversationUrl"]},{"required":["threadId"]},{"required":["gitIdentity"]},{"required":["verificationProfile"]},{"required":["resolutionEvidence"]},{"required":["workspace"]},{"required":["confirmationToken"]},{"required":["candidateHandle"]}]}},
                    {"required":["projectId","decision","confirmationToken"],"properties":{"decision":{"const":"GITHUB_BOOTSTRAP_CONFIRM"}}, "not":{"anyOf":[{"required":["expectedSha256"]},{"required":["conversationUrl"]},{"required":["threadId"]},{"required":["gitIdentity"]},{"required":["verificationProfile"]},{"required":["resolutionEvidence"]},{"required":["workspace"]},{"required":["candidateHandle"]}]}},
                    {"required":["projectId","decision","confirmationToken"],"properties":{"decision":{"const":"GITHUB_BOOTSTRAP_RECOVER"}}, "not":{"anyOf":[{"required":["expectedSha256"]},{"required":["conversationUrl"]},{"required":["threadId"]},{"required":["gitIdentity"]},{"required":["verificationProfile"]},{"required":["resolutionEvidence"]},{"required":["workspace"]},{"required":["candidateHandle"]}]}},
                    {"required":["decision"],"properties":{"decision":{"pattern":"^PROJECT_ORIGIN_WORKSPACE=.+$"}},"not":{"anyOf":[{"required":["projectId"]},{"required":["expectedSha256"]},{"required":["conversationUrl"]},{"required":["threadId"]},{"required":["gitIdentity"]},{"required":["verificationProfile"]},{"required":["resolutionEvidence"]},{"required":["workspace"]}]}},
                    {"required":["projectId","decision","gitIdentity","verificationProfile"],"properties":{"decision":{"pattern":"^PROJECT_REGISTRATION_PREFLIGHT_WORKSPACE=.+$"}},"not":{"anyOf":[{"required":["expectedSha256"]},{"required":["conversationUrl"]},{"required":["threadId"]},{"required":["resolutionEvidence"]},{"required":["workspace"]}]}},
                    {"required":["decision","confirmationToken"],"properties":{"decision":{"const":"PROJECT_REGISTRATION_CONFIRM"}},"not":{"anyOf":[{"required":["expectedSha256"]},{"required":["conversationUrl"]},{"required":["threadId"]},{"required":["gitIdentity"]},{"required":["verificationProfile"]},{"required":["resolutionEvidence"]},{"required":["workspace"]}]}},
                    {"required":["projectId","decision"],"properties":{"decision":{"pattern":"^PROJECT_THREAD_ADOPTION_PREFLIGHT_WORKSPACE=.+$"}},"not":{"anyOf":[{"required":["expectedSha256"]},{"required":["conversationUrl"]},{"required":["threadId"]},{"required":["gitIdentity"]},{"required":["verificationProfile"]},{"required":["resolutionEvidence"]},{"required":["workspace"]}]}},
                    {"required":["decision","confirmationToken","candidateHandle"],"properties":{"decision":{"const":"PROJECT_THREAD_ADOPTION_CONFIRM"}},"not":{"anyOf":[{"required":["expectedSha256"]},{"required":["conversationUrl"]},{"required":["threadId"]},{"required":["gitIdentity"]},{"required":["verificationProfile"]},{"required":["resolutionEvidence"]},{"required":["workspace"]}]}},
                    {"required":["projectId","decision"],"properties":{"decision":{"pattern":"^CHAT_TARGET_INIT_URL=\\S+$"}},"not":{"anyOf":[{"required":["expectedSha256"]},{"required":["conversationUrl"]},{"required":["threadId"]},{"required":["gitIdentity"]},{"required":["verificationProfile"]},{"required":["resolutionEvidence"]},{"required":["workspace"]}]}}
                ]);
            }
            if *name == "catdesk_release_recovery" {
                input_schema = json!({
                    "type": "object",
                    "additionalProperties": false,
                    "maxProperties": 0
                });
            }
            if *name == "autonomy_runtime_capabilities" {
                input_schema = json!({
                    "type": "object",
                    "additionalProperties": false,
                    "maxProperties": 0
                });
            }
            if *name == "catdesk_daemon_reload" {
                input_schema = json!({"type":"object","oneOf":[
                    {
                        "required":["action","buildPath","expectedSha256","recordId"],
                        "properties":{
                            "action":{"const":"PREFLIGHT"},
                            "buildPath":{"type":"string","minLength":1,"maxLength":4096},
                            "expectedSha256":{"type":"string","pattern":"^[A-Fa-f0-9]{64}$"},
                            "recordId":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,128}$"}
                        },
                        "additionalProperties":false
                    },
                    {
                        "required":["action","buildPath","expectedSha256","confirmationToken"],
                        "properties":{
                            "action":{"const":"CONFIRM"},
                            "buildPath":{"type":"string","minLength":1,"maxLength":4096},
                            "expectedSha256":{"type":"string","pattern":"^[A-Fa-f0-9]{64}$"},
                            "confirmationToken":{"type":"string","minLength":1,"maxLength":256}
                        },
                        "additionalProperties":false
                    },
                    {
                        "required":["action"],
                        "properties":{"action":{"const":"RESULT"}},
                        "additionalProperties":false
                    }
                ]});
            }
            if *name == "catdesk_reviewed_build" {
                input_schema = json!({"type":"object","oneOf":[
                    {"required":["action","recordId"],"properties":{"action":{"enum":["PREPARE","PREFLIGHT"]},"recordId":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,128}$"}},"additionalProperties":false},
                    {"required":["action","confirmationToken"],"properties":{"action":{"const":"CONFIRM"},"confirmationToken":{"type":"string","minLength":1,"maxLength":256}},"additionalProperties":false},
                    {"required":["action"],"properties":{"action":{"const":"RESULT"}},"additionalProperties":false}
                ]});
            }
            if *name == "catdesk_reviewed_build_promotion" {
                input_schema = json!({"type":"object","oneOf":[
                    {"required":["action","buildPath","expectedSha256","recordId"],"properties":{"action":{"const":"PREFLIGHT"}},"additionalProperties":false},
                    {"required":["action","buildPath","expectedSha256","confirmationToken"],"properties":{"action":{"const":"CONFIRM"}},"additionalProperties":false},
                    {"required":["action"],"properties":{"action":{"const":"RESULT"}},"additionalProperties":false}
                ]});
            }
            if *name == "catdesk_github_publication" {
                input_schema = json!({"type":"object","oneOf":[
                    {"required":["action","recordId","approval"],"properties":{"action":{"const":"PREPARE"},"recordId":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,128}$"},"approval":{"type":"object"}},"additionalProperties":false},
                    {"required":["action","confirmationToken"],"properties":{"action":{"const":"CONFIRM"},"confirmationToken":{"type":"string","pattern":"^gpub-[A-Fa-f0-9]{64}$"}},"additionalProperties":false},
                    {"required":["action","confirmationToken"],"properties":{"action":{"const":"RESULT"},"confirmationToken":{"type":"string","pattern":"^gpub-[A-Fa-f0-9]{64}$"}},"additionalProperties":false}
                ]});
            }
            json!({
                "name": name,
                "title": name.replace('_', " "),
                "description": "CatDesk autonomous-development control-plane operation. Operator executable and authentication configuration are never accepted through MCP.",
                "inputSchema": input_schema,
                "annotations": {"readOnlyHint":read_only,"openWorldHint":false,"destructiveHint":matches!(*name,"autonomy_session_cancel" | "catdesk_daemon_reload")}
            })
        })
        .collect()
}

pub fn handle_tool(name: &str, args: Value, workspace: &Path) -> Result<Value, String> {
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "workspace canonicalization failed".to_string())?;
    if name == "autonomy_runtime_capabilities" {
        require_empty_capability_args(&args)?;
        return Ok(runtime_capability_manifest());
    }
    if name == "autonomy_contract_validate" && is_capability_query(&args)? {
        return Ok(runtime_capability_manifest());
    }
    let supervisor = AutonomousSupervisorV1::open(&workspace)?;
    match name {
        "autonomy_contract_create" => {
            let contract: AutonomousDevelopmentContractV1 =
                serde_json::from_value(required(&args, "contract")?.clone())
                    .map_err(|_| "autonomous contract schema is invalid".to_string())?;
            supervisor.create_contract(contract)
        }
        "autonomy_contract_validate" => supervisor.validate_contract(session_id(&args)?),
        "autonomy_contract_approve" => supervisor.approve(
            session_id(&args)?,
            expected_state_version(&args)?,
            required_str(&args, "idempotencyKey")?,
            required_str(&args, "approvalId")?,
            required_str(&args, "decisionHash")?,
        ),
        "autonomy_session_start" => supervisor.start(
            session_id(&args)?,
            expected_state_version(&args)?,
            required_str(&args, "idempotencyKey")?,
        ),
        "autonomy_session_status" => supervisor.status(session_id(&args)?),
        "autonomy_session_events" => supervisor.events(
            session_id(&args)?,
            optional_u64(&args, "afterSequence").unwrap_or(0),
            optional_u64(&args, "limit").unwrap_or(50).min(100) as usize,
            optional_u64(&args, "maxBytes")
                .unwrap_or(24_000)
                .min(24_000) as usize,
        ),
        "autonomy_session_pause" => supervisor.set_state(
            session_id(&args)?,
            expected_state_version(&args)?,
            required_str(&args, "idempotencyKey")?,
            AutonomousSessionStateV1::Paused,
            "session_paused",
        ),
        "autonomy_session_resume" => supervisor.set_state(
            session_id(&args)?,
            expected_state_version(&args)?,
            required_str(&args, "idempotencyKey")?,
            AutonomousSessionStateV1::Queued,
            "session_resumed",
        ),
        "autonomy_session_cancel" => supervisor.cancel(
            session_id(&args)?,
            expected_state_version(&args)?,
            required_str(&args, "idempotencyKey")?,
        ),
        "autonomy_session_renew_lease" => supervisor.renew_lease(
            session_id(&args)?,
            expected_state_version(&args)?,
            required_str(&args, "idempotencyKey")?,
            required_u64(&args, "newExpiryUnix")?,
        ),
        "autonomy_session_get_checkpoint" => supervisor.status(session_id(&args)?),
        "autonomy_session_get_escalation" => supervisor.escalation(session_id(&args)?),
        "autonomy_session_get_diff" => supervisor.diff(session_id(&args)?),
        "autonomy_session_get_final_review" => supervisor.final_review(session_id(&args)?),
        "autonomy_session_reply" => supervisor.reply(
            session_id(&args)?,
            expected_state_version(&args)?,
            required_str(&args, "idempotencyKey")?,
            required_str(&args, "escalationId")?,
            required_str(&args, "decisionHash")?,
            required_str(&args, "decision")?,
            string_list(&args, "constraints")?,
        ),
        "autonomy_session_claim_direct_work" => supervisor.request_direct_work_claim(
            session_id(&args)?,
            expected_state_version(&args)?,
            required_str(&args, "idempotencyKey")?,
        ),
        "autonomy_session_finalize_direct_work" => supervisor.request_direct_work_finalization(
            session_id(&args)?,
            expected_state_version(&args)?,
            required_str(&args, "idempotencyKey")?,
        ),
        "autonomy_session_list" => supervisor.list(),
        "autonomy_queue_status" => supervisor.queue(session_id(&args)?),
        "autonomy_execution_accounting" => supervisor.execution_accounting(session_id(&args)?),
        "autonomy_work_time_report" => supervisor.work_time_report(
            required_u64(&args, "startUnix")?,
            required_u64(&args, "endUnix")?,
            args.get("projectId").and_then(Value::as_str),
            args.get("sessionId").and_then(Value::as_str),
            args.get("taskId").and_then(Value::as_str),
            optional_u64(&args, "limit").unwrap_or(50) as usize,
            optional_u64(&args, "maxBytes").unwrap_or(24_000) as usize,
        ),
        "autonomy_wake_policy_get" => supervisor.wake_policy_get(),
        "autonomy_wake_policy_set" => supervisor.wake_policy_set(
            required_u64(&args, "expectedGeneration")?,
            required_str(&args, "mode")?,
            args.get("terminalTaskId").and_then(Value::as_str),
        ),
        "autonomy_ticket_audit" => supervisor.ticket_audit(
            required_u64(&args, "startUnix")?,
            required_u64(&args, "endUnix")?,
            args.get("projectId").and_then(Value::as_str),
            args.get("afterSessionId").and_then(Value::as_str),
            optional_u64(&args, "limit").unwrap_or(50) as usize,
            optional_u64(&args, "maxBytes").unwrap_or(24_000) as usize,
        ),
        "autonomy_session_supersede" => supervisor.record_supersession(
            required_str(&args, "supersededSessionId")?,
            required_str(&args, "supersedingSessionId")?,
        ),
        "autonomy_review_inbox_list" => supervisor.review_inbox(
            args.get("projectId").and_then(Value::as_str),
            args.get("afterRecordId").and_then(Value::as_str),
            optional_u64(&args, "limit").unwrap_or(50).min(100) as usize,
        ),
        "autonomy_review_inbox_ack" => {
            supervisor.ack_review_inbox(required_str(&args, "recordId")?)
        }
        "autonomy_project_registry_read" => supervisor.project_registry_read(),
        "autonomy_project_registry_bind" => supervisor.project_registry_bind(&args),
        "autonomy_project_registry_preflight" => supervisor.project_registry_preflight(
            required_str(&args, "projectId")?,
            required_str(&args, "workspace")?,
            required_str(&args, "gitIdentity")?,
            required_str(&args, "verificationProfile")?,
        ),
        "autonomy_project_registry_confirm" => {
            supervisor.project_registry_confirm(required_str(&args, "confirmationToken")?)
        }
        "autonomy_project_registry_chat_target_bind" => supervisor
            .project_registry_chat_target_bind(
                required_str(&args, "projectId")?,
                required_str(&args, "conversationUrl")?,
                args.get("expectedCurrentTargetSha256")
                    .and_then(Value::as_str),
            ),
        "autonomy_project_thread_adoption_preflight" => supervisor
            .project_thread_adoption_preflight(
                required_str(&args, "projectId")?,
                required_str(&args, "workspace")?,
            ),
        "autonomy_project_thread_adoption_confirm" => supervisor.project_thread_adoption_confirm(
            required_str(&args, "confirmationToken")?,
            required_str(&args, "candidateHandle")?,
        ),
        "catdesk_codex_goal_resume" => {
            let titles = args
                .get("titles")
                .and_then(Value::as_array)
                .ok_or_else(|| "Codex Goal resume titles are required".to_string())?
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| "Codex Goal resume title must be a string".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            super::autonomy_runtime::host_resume_codex_goals_by_title(
                &workspace,
                &titles,
                supervisor_now_unix(),
            )
            .map_err(|_| "Codex Goal resume host operation failed".to_string())
        }
        "catdesk_daemon_reload" => supervisor.daemon_reload(&args),
        "catdesk_release_recovery" => supervisor.canonical_release_recovery(&args),
        "catdesk_reviewed_build" => supervisor.reviewed_build(&args),
        "catdesk_reviewed_build_promotion" => supervisor.reviewed_build_promotion(&args),
        "catdesk_github_publication" => supervisor.github_publication(&args),
        "provider_status" => Ok(json!({
            "providerId": "codex-cli",
            "status": "OPERATOR_CONFIGURATION_REQUIRED",
            "detail": "MCP cannot select an executable or read authentication files."
        })),
        _ => Err("unknown autonomous supervisor tool".into()),
    }
}

/// Backwards-compatible capability discovery uses only the pre-existing
/// generic `action` field. It is intentionally one exact field and cannot be
/// combined with session, contract, or any other validation authority.
fn is_capability_query(args: &Value) -> Result<bool, String> {
    let object = args
        .as_object()
        .ok_or_else(|| "autonomy capability query is invalid".to_string())?;
    let Some(action) = object.get("action") else {
        return Ok(false);
    };
    if object.len() != 1 || action.as_str() != Some("RESULT") {
        return Err("autonomy capability query may not be mixed with validation authority".into());
    }
    Ok(true)
}

fn require_empty_capability_args(args: &Value) -> Result<(), String> {
    if args.as_object().is_some_and(|object| object.is_empty()) {
        Ok(())
    } else {
        Err("autonomy runtime capabilities accepts no arguments".into())
    }
}

struct AutonomousSupervisorV1 {
    store: AutonomousStateStoreV1,
    workspace: PathBuf,
}

impl AutonomousSupervisorV1 {
    fn open(workspace: &Path) -> Result<Self, String> {
        Ok(Self {
            store: AutonomousStateStoreV1::open(workspace.join(".catdesk").join("autonomy"))
                .map_err(|_| "autonomy state store could not be opened".to_string())?,
            workspace: workspace.into(),
        })
    }

    fn project_registry(&self) -> Result<AutonomousProjectRegistryStoreV1, String> {
        let store = AutonomousProjectRegistryStoreV1::open(
            self.workspace.join(".catdesk").join("projects"),
        )
        .map_err(|_| "project registry could not be opened".to_string())?;
        store
            .initialize(1, 1)
            .map_err(|_| "project registry initialization failed".to_string())?;
        Ok(store)
    }

    fn github_bootstrap_store(&self) -> GithubBootstrapStore {
        GithubBootstrapStore::open(self.workspace.join(".catdesk").join("github-bootstrap"))
    }

    fn project_registry_read(&self) -> Result<Value, String> {
        let registry = self
            .project_registry()?
            .load_registry()
            .map_err(|_| "project registry is unavailable".to_string())?;
        Ok(json!({
            "schemaVersion": registry.schema_version,
            "maximumCodexWorkers": registry.maximum_codex_workers,
            "maximumQwenWorkers": registry.maximum_qwen_workers,
            "projects": registry.projects,
        }))
    }

    /// The app-server resolver remains the authority for observing a thread.
    /// This bounded control-plane operation only persists an already verified
    /// exact, non-owned, direct-input result for the canonical MCP workspace.
    fn project_registry_bind(&self, args: &Value) -> Result<Value, String> {
        let has_target_fields = args.get("conversationUrl").is_some()
            || args.get("expectedCurrentTargetSha256").is_some();
        let has_cached_target_fields =
            args.get("decision").is_some() || args.get("expectedSha256").is_some();
        if has_target_fields && has_cached_target_fields {
            return Err("chat-target binding modes may not be mixed".into());
        }
        if has_target_fields {
            return self.project_registry_bind_chat_target(args);
        }
        if has_cached_target_fields {
            return self.project_registry_bind_cached_control(args);
        }
        self.project_registry_bind_thread(
            required_str(args, "projectId")?,
            required_str(args, "threadId")?,
            required_str(args, "gitIdentity")?,
            required_str(args, "verificationProfile")?,
            required_str(args, "resolutionEvidence")?,
        )
    }

    /// Existing direct thread-binding behavior. Keep this path untouched when
    /// no chat-target fields are supplied.
    fn project_registry_bind_thread(
        &self,
        project_id: &str,
        thread_id: &str,
        git_identity: &str,
        verification_profile: &str,
        resolution_evidence: &str,
    ) -> Result<Value, String> {
        if resolution_evidence != "exact-unowned-direct-input" {
            return Err(
                "project thread binding requires exact unowned direct-input evidence".into(),
            );
        }
        let store = self.project_registry()?;
        let registry = store
            .load_registry()
            .map_err(|_| "project registry is unavailable".to_string())?;
        if registry.schema_version != PROJECT_REGISTRY_SCHEMA_VERSION {
            return Err("project registry schema is unsupported".into());
        }
        if let Some(existing) = registry
            .projects
            .iter()
            .find(|project| project.project_id == project_id)
        {
            if existing.workspace != self.workspace {
                return Err("project id is registered to a different workspace".into());
            }
        } else {
            if registry
                .projects
                .iter()
                .any(|project| project.codex_thread_id.as_deref() == Some(thread_id))
            {
                return Err("Codex thread is already bound to another project".into());
            }
            store
                .register_project(AutonomousProjectV1 {
                    project_id: project_id.into(),
                    workspace: self.workspace.clone(),
                    git_identity: git_identity.into(),
                    verification_profile: verification_profile.into(),
                    codex_thread_id: None,
                    chatgpt_target_url: None,
                    chatgpt_target_sha256: None,
                })
                .map_err(|_| "project registration was rejected".to_string())?;
        }
        store
            .bind_codex_thread(project_id, thread_id)
            .map_err(|_| "project thread binding was rejected".to_string())?;
        let project = store
            .load_registry()
            .map_err(|_| "project registry is unavailable".to_string())?
            .projects
            .into_iter()
            .find(|project| project.project_id == project_id)
            .ok_or_else(|| "project binding was not persisted".to_string())?;
        if project.workspace != self.workspace
            || project.codex_thread_id.as_deref() != Some(thread_id)
        {
            return Err("project binding verification failed".into());
        }
        Ok(json!({
            "projectId": project.project_id,
            "workspace": project.workspace,
            "codexThreadId": project.codex_thread_id,
            "bindingEvidence": resolution_evidence,
        }))
    }

    /// A narrow alternate shape on the already-exposed bind operation. It is
    /// intentionally not a general project patch: only the reviewed target
    /// CAS fields are accepted and all thread/workspace/profile fields fail.
    fn project_registry_bind_chat_target(&self, args: &Value) -> Result<Value, String> {
        let object = args
            .as_object()
            .ok_or_else(|| "project registry binding arguments are invalid".to_string())?;
        if object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "projectId" | "conversationUrl" | "expectedCurrentTargetSha256"
            )
        }) {
            return Err("chat-target binding rejects unrelated fields".into());
        }
        let project_id = required_str(args, "projectId")?;
        let conversation_url = required_str(args, "conversationUrl")?;
        let store = self.project_registry()?;
        store
            .project_for_workspace(project_id, &self.workspace)
            .map_err(|_| "project id is registered to a different workspace".to_string())?;
        let project = store
            .bind_project_chat_target(
                project_id,
                conversation_url,
                args.get("expectedCurrentTargetSha256")
                    .and_then(Value::as_str),
            )
            .map_err(|_| "project ChatGPT target binding was rejected".to_string())?;
        Ok(json!({
            "projectId": project.project_id,
            "targetSha256": project.chatgpt_target_sha256,
        }))
    }

    /// Compatibility control shapes for an already-open connector that cannot
    /// send newer field names. `decision` is never a generic command channel:
    /// each literal/prefix has a closed field allowlist and delegates to the
    /// reviewed control-plane operation that owns the authority decision.
    fn project_registry_bind_cached_control(&self, args: &Value) -> Result<Value, String> {
        let decision = cached_bind_decision(args)?;
        if let Some(workspace) = decision.strip_prefix("GITHUB_BOOTSTRAP_PREFLIGHT_WORKSPACE=") {
            cached_bind_keys(args, &["projectId", "decision"])?;
            let preflight = self
                .github_bootstrap_store()
                .preflight(
                    required_str(args, "projectId")?,
                    workspace,
                    supervisor_now_unix(),
                    &SystemGithubBootstrapRunner,
                )
                .map_err(|failure| failure.reason_code().to_string())?;
            return Ok(json!({
                "projectId": preflight.project_id,
                "confirmationToken": preflight.confirmation_token,
                "expiresAtUnix": preflight.expires_at_unix,
                "owner": "vnath99",
                "origin": preflight.policy_origin,
                "branch": preflight.branch,
                "head": preflight.head,
                "workingTreeDirty": preflight.working_tree_dirty,
                "workingTreeChangeCount": preflight.working_tree_change_count,
            }));
        }
        if decision == "GITHUB_BOOTSTRAP_CONFIRM" {
            cached_bind_keys(args, &["projectId", "decision", "confirmationToken"])?;
            let outcome = self
                .github_bootstrap_store()
                .confirm(
                    required_str(args, "projectId")?,
                    required_str(args, "confirmationToken")?,
                    supervisor_now_unix(),
                    &SystemGithubBootstrapRunner,
                )
                .map_err(|failure| failure.reason_code().to_string())?;
            return Ok(json!({
                "projectId": outcome.project_id,
                "owner": outcome.owner,
                "repository": outcome.repository,
                "origin": outcome.origin,
                "branch": outcome.branch,
                "head": outcome.head,
                "workingTreeDirty": outcome.working_tree_dirty,
                "workingTreeChangeCount": outcome.working_tree_change_count,
                "completedStages": outcome.stages,
            }));
        }
        if decision == "GITHUB_BOOTSTRAP_RECOVER" {
            cached_bind_keys(args, &["projectId", "decision", "confirmationToken"])?;
            let outcome = self
                .github_bootstrap_store()
                .recover(
                    required_str(args, "projectId")?,
                    required_str(args, "confirmationToken")?,
                    supervisor_now_unix(),
                    &SystemGithubBootstrapRunner,
                )
                .map_err(|failure| failure.reason_code().to_string())?;
            return Ok(json!({
                "projectId": outcome.project_id,
                "owner": outcome.owner,
                "repository": outcome.repository,
                "origin": outcome.origin,
                "branch": outcome.branch,
                "head": outcome.head,
                "workingTreeDirty": outcome.working_tree_dirty,
                "workingTreeChangeCount": outcome.working_tree_change_count,
                "completedStages": outcome.stages,
            }));
        }
        if let Some(conversation_url) = decision.strip_prefix("DESIGNATED_CHAT_TARGET_URL=") {
            cached_bind_keys(args, &["projectId", "decision", "expectedSha256"])?;
            let project_id = required_str(args, "projectId")?;
            if project_id != "catdesk" {
                return Err(
                    "designated ChatGPT target update is available only for project catdesk".into(),
                );
            }
            validate_cached_conversation_url(conversation_url)?;
            let target = crate::mcp::operator_update_designated_chat_target(
                &self.workspace,
                conversation_url,
                required_str(args, "expectedSha256")?,
            )
            .map_err(|_| "designated ChatGPT target update was rejected".to_string())?;
            return Ok(json!({
                "projectId": project_id,
                "targetSha256": target.sha256,
                "targetUrl": target.url,
            }));
        }
        if let Some(conversation_url) = decision.strip_prefix("CHAT_TARGET_URL=") {
            cached_bind_keys(args, &["projectId", "decision", "expectedSha256"])?;
            return self.project_registry_bind_cached_chat_target(
                required_str(args, "projectId")?,
                conversation_url,
                required_str(args, "expectedSha256")?,
            );
        }
        if let Some(workspace) = decision.strip_prefix("PROJECT_ORIGIN_WORKSPACE=") {
            cached_bind_keys(args, &["decision"])?;
            return match project_origin_workspace_probe(workspace) {
                Ok((workspace, git_identity)) => Ok(json!({
                    "accepted": true,
                    "identityKind": "GIT_REMOTE_ORIGIN",
                    "workspace": workspace,
                    "gitIdentity": git_identity,
                })),
                Err(failure) => Ok(json!({
                    "accepted": false,
                    "reasonCode": failure.reason_code(),
                })),
            };
        }
        if let Some(workspace) = decision.strip_prefix("PROJECT_REGISTRATION_PREFLIGHT_WORKSPACE=")
        {
            cached_bind_keys(
                args,
                &[
                    "projectId",
                    "decision",
                    "gitIdentity",
                    "verificationProfile",
                ],
            )?;
            return self.project_registry_preflight(
                required_str(args, "projectId")?,
                workspace,
                required_str(args, "gitIdentity")?,
                required_str(args, "verificationProfile")?,
            );
        }
        if decision == "PROJECT_REGISTRATION_CONFIRM" {
            cached_bind_keys(args, &["projectId", "decision", "confirmationToken"])?;
            let requested_project_id = cached_optional_project_id(args)?;
            let confirmed =
                self.project_registry_confirm(required_str(args, "confirmationToken")?)?;
            if requested_project_id.is_some_and(|project_id| {
                confirmed.get("projectId").and_then(Value::as_str) != Some(project_id)
            }) {
                return Err("project registration confirmation projectId did not match".into());
            }
            return Ok(confirmed);
        }
        if let Some(workspace) =
            decision.strip_prefix("PROJECT_THREAD_ADOPTION_PREFLIGHT_WORKSPACE=")
        {
            cached_bind_keys(args, &["projectId", "decision"])?;
            return self
                .project_thread_adoption_preflight(required_str(args, "projectId")?, workspace);
        }
        if decision == "PROJECT_THREAD_ADOPTION_CONFIRM" {
            cached_bind_keys(
                args,
                &[
                    "projectId",
                    "decision",
                    "confirmationToken",
                    "candidateHandle",
                ],
            )?;
            let requested_project_id = cached_optional_project_id(args)?;
            let confirmed = self.project_thread_adoption_confirm(
                required_str(args, "confirmationToken")?,
                required_str(args, "candidateHandle")?,
            )?;
            if requested_project_id.is_some_and(|project_id| {
                confirmed.get("projectId").and_then(Value::as_str) != Some(project_id)
            }) {
                return Err("thread adoption confirmation projectId did not match".into());
            }
            return Ok(confirmed);
        }
        if let Some(conversation_url) = decision.strip_prefix("CHAT_TARGET_INIT_URL=") {
            cached_bind_keys(args, &["projectId", "decision"])?;
            return self.project_registry_bind_cached_chat_target_initialize(
                required_str(args, "projectId")?,
                conversation_url,
            );
        }
        Err("cached project registry decision is not a supported exact control form".into())
    }

    fn project_registry_bind_cached_chat_target(
        &self,
        project_id: &str,
        conversation_url: &str,
        expected: &str,
    ) -> Result<Value, String> {
        validate_cached_conversation_url(conversation_url)?;
        let project = self
            .project_registry()?
            .bind_project_chat_target(project_id, conversation_url, Some(expected))
            .map_err(|_| "cached project ChatGPT target binding was rejected".to_string())?;
        Ok(json!({"projectId": project.project_id, "targetSha256": project.chatgpt_target_sha256}))
    }

    fn project_registry_bind_cached_chat_target_initialize(
        &self,
        project_id: &str,
        conversation_url: &str,
    ) -> Result<Value, String> {
        validate_cached_conversation_url(conversation_url)?;
        let project = self
            .project_registry()?
            .initialize_project_chat_target(project_id, conversation_url)
            .map_err(|_| "project ChatGPT target initialization was rejected".to_string())?;
        Ok(json!({"projectId": project.project_id, "targetSha256": project.chatgpt_target_sha256}))
    }

    fn project_registry_preflight(
        &self,
        project_id: &str,
        workspace: &str,
        git_identity: &str,
        verification_profile: &str,
    ) -> Result<Value, String> {
        let workspace = verified_external_git_workspace(workspace, git_identity)?;
        let evidence = super::autonomy_runtime::host_observe_registration_thread(
            &workspace,
            git_identity,
            supervisor_now_unix(),
        )
        .map_err(|_| {
            "authoritative exact-CWD Codex registration evidence is unavailable".to_string()
        })?;
        let preflight = self
            .project_registry()?
            .preflight_project_registration(
                project_id,
                verification_profile,
                evidence,
                supervisor_now_unix(),
            )
            .map_err(|_| "project registration preflight was rejected".to_string())?;
        Ok(json!({
            "confirmationToken": preflight.confirmation_token,
            "confirmationFingerprint": preflight.confirmation_fingerprint,
            "expiresAtUnix": preflight.expires_at_unix,
            "projectId": preflight.project_id,
            "workspace": preflight.evidence.workspace,
            "gitIdentity": preflight.evidence.git_identity,
            "codexThreadId": preflight.evidence.codex_thread_id,
            "model": preflight.evidence.selected_model,
            "reasoningEffort": preflight.evidence.reasoning_effort,
        }))
    }

    fn project_registry_confirm(&self, confirmation_token: &str) -> Result<Value, String> {
        let store = self.project_registry()?;
        let preflight = store
            .registration_preflight(confirmation_token)
            .map_err(|_| "project registration confirmation is unavailable".to_string())?;
        let workspace = verified_external_git_workspace(
            &preflight.evidence.workspace.to_string_lossy(),
            &preflight.evidence.git_identity,
        )?;
        let evidence = super::autonomy_runtime::host_observe_registration_thread(
            &workspace,
            &preflight.evidence.git_identity,
            supervisor_now_unix(),
        )
        .map_err(|_| {
            "authoritative exact-CWD Codex confirmation evidence is unavailable".to_string()
        })?;
        let project = store
            .confirm_project_registration(confirmation_token, &evidence, supervisor_now_unix())
            .map_err(|_| "project registration confirmation was rejected".to_string())?;
        Ok(json!({
            "projectId": project.project_id,
            "workspace": project.workspace,
            "gitIdentity": project.git_identity,
            "verificationProfile": project.verification_profile,
            "codexThreadId": project.codex_thread_id,
        }))
    }

    fn project_registry_chat_target_bind(
        &self,
        project_id: &str,
        conversation_url: &str,
        expected_current_target_sha256: Option<&str>,
    ) -> Result<Value, String> {
        let store = self.project_registry()?;
        store
            .project_for_workspace(project_id, &self.workspace)
            .map_err(|_| "project id is registered to a different workspace".to_string())?;
        let project = store
            .bind_project_chat_target(project_id, conversation_url, expected_current_target_sha256)
            .map_err(|_| "project ChatGPT target binding was rejected".to_string())?;
        Ok(json!({
            "projectId": project.project_id,
            "chatgptTargetUrl": project.chatgpt_target_url,
            "chatgptTargetSha256": project.chatgpt_target_sha256,
        }))
    }

    fn project_thread_adoption_preflight(
        &self,
        project_id: &str,
        workspace: &str,
    ) -> Result<Value, String> {
        let workspace = PathBuf::from(workspace)
            .canonicalize()
            .map_err(|_| "adoption workspace canonicalization failed".to_string())?;
        let store = self.project_registry()?;
        let registry = store
            .load_registry()
            .map_err(|_| "project registry is unavailable".to_string())?;
        let project = registry
            .projects
            .iter()
            .find(|project| project.project_id == project_id)
            .ok_or_else(|| "project is not registered".to_string())?;
        if project.workspace != workspace || project.codex_thread_id.is_some() {
            return Err("project is not eligible for existing-thread adoption".into());
        }
        let candidates = super::autonomy_runtime::host_discover_project_thread_adoption_candidates(
            &workspace,
            &project.git_identity,
            supervisor_now_unix(),
        )
        .map_err(|_| {
            "authoritative existing-thread adoption metadata is unavailable".to_string()
        })?;
        let preflight = store
            .preflight_thread_adoption(
                project_id,
                &workspace,
                candidates
                    .into_iter()
                    .map(|candidate| (candidate.title, candidate.evidence))
                    .collect(),
                supervisor_now_unix(),
            )
            .map_err(|_| "existing-thread adoption preflight was rejected".to_string())?;
        Ok(json!({
            "confirmationToken": preflight.confirmation_token,
            "projectId": preflight.project_id,
            "workspace": preflight.workspace,
            "expiresAtUnix": preflight.expires_at_unix,
            "candidates": preflight.candidates.into_iter().map(|candidate| json!({
                "candidateHandle": candidate.candidate_handle,
                "title": candidate.title,
                "threadFingerprint": candidate.thread_fingerprint,
                "cwdFingerprint": candidate.cwd_fingerprint,
                "model": candidate.evidence.selected_model,
                "reasoningEffort": candidate.evidence.reasoning_effort,
            })).collect::<Vec<_>>(),
        }))
    }

    fn project_thread_adoption_confirm(
        &self,
        confirmation_token: &str,
        candidate_handle: &str,
    ) -> Result<Value, String> {
        let store = self.project_registry()?;
        let preflight = store
            .load_thread_adoption_preflight(confirmation_token)
            .map_err(|_| "existing-thread adoption confirmation is unavailable".to_string())?;
        let candidate = preflight
            .candidates
            .iter()
            .find(|candidate| candidate.candidate_handle == candidate_handle)
            .ok_or_else(|| "existing-thread adoption candidate is unavailable".to_string())?;
        let evidence = super::autonomy_runtime::host_confirm_project_thread_adoption(
            &preflight.workspace,
            &candidate.evidence.git_identity,
            &candidate.evidence.codex_thread_id,
            supervisor_now_unix(),
        )
        .map_err(|_| {
            "authoritative existing-thread adoption confirmation metadata is unavailable"
                .to_string()
        })?;
        let project = store
            .confirm_thread_adoption(
                confirmation_token,
                candidate_handle,
                &evidence,
                supervisor_now_unix(),
            )
            .map_err(|_| "existing-thread adoption confirmation was rejected".to_string())?;
        Ok(
            json!({"projectId": project.project_id, "workspace": project.workspace, "codexThreadId": project.codex_thread_id}),
        )
    }

    fn daemon_reload(&self, args: &Value) -> Result<Value, String> {
        let action = required_str(args, "action")?;
        let allowed: &[&str] = match action {
            "PREFLIGHT" => &["action", "buildPath", "expectedSha256", "recordId"],
            "CONFIRM" => &["action", "buildPath", "expectedSha256", "confirmationToken"],
            "RESULT" => &["action"],
            _ => return Err("CatDesk daemon reload action is invalid".into()),
        };
        let object = args
            .as_object()
            .ok_or_else(|| "CatDesk daemon reload request is invalid".to_string())?;
        if object.len() != allowed.len()
            || object.keys().any(|key| !allowed.contains(&key.as_str()))
        {
            return Err("CatDesk daemon reload request is invalid".into());
        }

        let active_mutation = self
            .store
            .list_sessions()
            .map_err(|_| "autonomy sessions could not be inspected before reload".to_string())?
            .into_iter()
            .any(|snapshot| {
                snapshot.active
                    && matches!(
                        snapshot.state,
                        AutonomousSessionStateV1::Running
                            | AutonomousSessionStateV1::Verifying
                            | AutonomousSessionStateV1::RecoveringAfterRestart
                    )
            });

        if action == "RESULT" {
            let preflight = crate::daemon_reload::read_reload_preflight(&self.workspace).ok();
            let preflight_present = preflight.is_some();
            let state = if active_mutation {
                "BLOCKED_ACTIVE_MUTATION"
            } else if preflight_present {
                "PREFLIGHT_PRESENT"
            } else {
                "READY_FOR_PREFLIGHT"
            };
            let next_action = if active_mutation {
                "WAIT_FOR_ACTIVE_MUTATION"
            } else if preflight_present {
                "CONFIRM_OR_REFRESH_PREFLIGHT"
            } else {
                "PREFLIGHT"
            };
            return Ok(json!({
                "state": state,
                "activeMutation": active_mutation,
                "preflightPresent": preflight_present,
                "nextAction": next_action,
                "tunnelAction": "none-external-tunnel-untouched"
            }));
        }

        if active_mutation {
            return Err(
                "CatDesk daemon reload is blocked while autonomous mutation or verification is active"
                    .into(),
            );
        }

        let build_path = PathBuf::from(required_str(args, "buildPath")?);
        let expected_sha256 = required_str(args, "expectedSha256")?;
        let port = std::env::var("PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(3200);
        let measurement = crate::daemon_reload::measure_reload_candidate(
            &self.workspace,
            &build_path,
            Some(expected_sha256),
        )?;
        let request = DaemonReloadApprovalRequestV1 {
            schema_version: DAEMON_RELOAD_APPROVAL_SCHEMA_VERSION,
            product: DAEMON_RELOAD_APPROVAL_PRODUCT.into(),
            project_id: DAEMON_RELOAD_APPROVAL_PROJECT.into(),
            purpose: DAEMON_RELOAD_APPROVAL_PURPOSE.into(),
            candidate_relative_path: measurement.candidate_relative_path.clone(),
            candidate_sha256: measurement.candidate_sha256.clone(),
            candidate_length: measurement.candidate_length,
        };
        validate_daemon_reload_request(&request)
            .map_err(|_| "CatDesk daemon reload candidate authority is invalid".to_string())?;

        if action == "PREFLIGHT" {
            let record_id = required_str(args, "recordId")?;
            let authority = self.resolve_daemon_reload_review_authority(record_id, &request)?;

            // Review resolution reads several durable files. Re-measure the exact
            // candidate afterwards so the review cannot be raced against candidate
            // substitution between measurement and preflight persistence.
            let after_review = crate::daemon_reload::measure_reload_candidate(
                &self.workspace,
                &build_path,
                Some(&request.candidate_sha256),
            )?;
            if after_review.candidate_relative_path != request.candidate_relative_path
                || after_review.candidate_sha256 != request.candidate_sha256
                || after_review.candidate_length != request.candidate_length
            {
                return Err(
                    "CatDesk daemon reload candidate changed while review authority was remeasured"
                        .into(),
                );
            }

            let review_binding = crate::daemon_reload::ReloadReviewBindingV1 {
                schema_version: 1,
                review_record_id: authority.review_record_id.clone(),
                review_session_id: authority.review_session_id.clone(),
                request_sha256: authority.request_sha256.clone(),
                authority_sha256: authority.authority_sha256.clone(),
            };
            let preflight = crate::daemon_reload::prepare_reviewed_reload(
                &self.workspace,
                &build_path,
                Some(&request.candidate_sha256),
                port,
                review_binding,
            )?;
            let relative = preflight
                .replacement_path
                .strip_prefix(&self.workspace)
                .unwrap_or(&preflight.replacement_path)
                .to_string_lossy()
                .into_owned();
            return Ok(json!({
                "state": "PREFLIGHT_READY",
                "accepted": false,
                "action": "PREFLIGHT",
                "replacement": relative,
                "expectedSha256": preflight.expected_sha256,
                "confirmationToken": preflight.confirmation_token,
                "expiresAtUnix": preflight.expires_at_unix,
                "oldPid": preflight.old_pid,
                "mcpPort": preflight.port,
                "reviewRecordId": authority.review_record_id,
                "reloadRequestSha256": authority.request_sha256,
                "reloadAuthoritySha256": authority.authority_sha256,
                "tunnelAction": "none-external-tunnel-untouched"
            }));
        }

        let persisted = crate::daemon_reload::read_reload_preflight(&self.workspace)?;
        let persisted_binding = persisted.review_binding.clone().ok_or_else(|| {
            "CatDesk daemon reload confirmation requires reviewed preflight authority".to_string()
        })?;
        let authority = self.resolve_daemon_reload_review_authority(
            &persisted_binding.review_record_id,
            &request,
        )?;
        let review_binding = crate::daemon_reload::ReloadReviewBindingV1 {
            schema_version: 1,
            review_record_id: authority.review_record_id.clone(),
            review_session_id: authority.review_session_id.clone(),
            request_sha256: authority.request_sha256.clone(),
            authority_sha256: authority.authority_sha256.clone(),
        };
        if review_binding != persisted_binding {
            return Err("CatDesk daemon reload review authority changed after preflight".into());
        }
        let confirmation_token = required_str(args, "confirmationToken")?;
        let preflight = crate::daemon_reload::execute_reviewed_reload(
            &self.workspace,
            &build_path,
            expected_sha256,
            confirmation_token,
            port,
            &review_binding,
        )?;
        Ok(json!({
            "state": "RELOAD_SCHEDULED",
            "accepted": true,
            "action": "CONFIRM",
            "replacementSha256": preflight.expected_sha256,
            "oldPid": preflight.old_pid,
            "mcpPort": preflight.port,
            "reviewRecordId": authority.review_record_id,
            "reloadRequestSha256": authority.request_sha256,
            "reloadAuthoritySha256": authority.authority_sha256,
            "handoff": "native-detached-helper-started",
            "tunnelAction": "none-external-tunnel-untouched",
            "expectedInterruption": "brief local MCP disconnect while replacement takes port"
        }))
    }

    fn resolve_reviewed_promotion_review_authority(
        &self,
        record_id: &str,
    ) -> Result<ReviewedPromotionReviewAuthorityV1, String> {
        if record_id.is_empty() || record_id.len() > 128 {
            return Err("independent review authority is unavailable".into());
        }
        let records = self
            .store
            .all_review_inbox()
            .map_err(|_| "independent review authority is unavailable".to_string())?
            .into_iter()
            .filter(|record| record.record_id == record_id)
            .collect::<Vec<_>>();
        if records.len() != 1 {
            return Err("independent review authority is unavailable".into());
        }
        let record = &records[0];
        if record.project_id != "catdesk"
            || record.unread
            || record.state != AutonomousSessionStateV1::CompletedVerified
            || record.next_action != "independent_final_review"
            || record.reference != "artifacts/completion.json"
        {
            return Err("independent review authority is unavailable".into());
        }
        let snapshot = self
            .store
            .load_session(&record.session_id)
            .map_err(|_| "independent review authority is unavailable".to_string())?;
        let task_id = snapshot
            .current_task_id
            .as_deref()
            .ok_or_else(|| "independent review authority is unavailable".to_string())?;
        if snapshot.active || snapshot.state != AutonomousSessionStateV1::CompletedVerified {
            return Err("independent review authority is unavailable".into());
        }
        let contract = self
            .store
            .load_contract(&record.session_id)
            .map_err(|_| "independent review authority is unavailable".to_string())?;
        let contract_hash = contract
            .decision_hash()
            .map_err(|_| "independent review authority is unavailable".to_string())?;
        if contract.project_id != "catdesk"
            || snapshot.approved_contract_hash.as_deref() != Some(contract_hash.as_str())
        {
            return Err("independent review authority is unavailable".into());
        }
        let completion_artifact_ids =
            reviewed_promotion_completion_artifact_ids(&contract, task_id)
                .map_err(|_| "independent review authority is unavailable".to_string())?;
        if completion_artifact_ids.is_empty() {
            return Err("independent review authority is unavailable".into());
        }
        let completion = self
            .store
            .load_completion_artifacts(&record.session_id)
            .map_err(|_| "independent review authority is unavailable".to_string())?;
        if completion.verification.status
            != crate::delegated::coordinator::VerificationStatusV1::Passed
            || completion.final_review.trim().is_empty()
        {
            return Err("independent review authority is unavailable".into());
        }
        let baseline = self
            .store
            .load_task_output_baseline(&record.session_id, task_id)
            .map_err(|_| "independent review authority is unavailable".to_string())?
            .ok_or_else(|| "independent review authority is unavailable".to_string())?;
        if baseline.session_id != record.session_id
            || baseline.task_id != task_id
            || baseline.approved_contract_hash != contract_hash
            || baseline.completion_artifact_ids != completion_artifact_ids
            || baseline.observations.len() != completion_artifact_ids.len()
        {
            return Err("independent review authority is unavailable".into());
        }
        let policy = AutonomousPolicyEngineV1::new(contract.clone())
            .map_err(|_| "independent review authority is unavailable".to_string())?;
        let mut current_observations = Vec::with_capacity(completion_artifact_ids.len());
        for artifact_id in &completion_artifact_ids {
            let observations = baseline
                .observations
                .iter()
                .filter(|observation| observation.artifact_id == *artifact_id)
                .collect::<Vec<_>>();
            if observations.len() != 1 {
                return Err("independent review authority is unavailable".into());
            }
            let before = observations[0];
            let now = safe_task_output_hash(&policy, artifact_id)
                .map_err(|_| "independent review authority is unavailable".to_string())?;
            let attributed = match (
                before.state.as_str(),
                before.sha256.as_deref(),
                now.as_deref(),
            ) {
                ("ABSENT", None, Some(_)) => true,
                ("PRESENT_SHA256", Some(previous), Some(current)) => previous != current,
                _ => false,
            };
            if !attributed {
                return Err("independent review authority is unavailable".into());
            }
            current_observations.push(AutonomousTaskOutputObservationV1 {
                artifact_id: artifact_id.clone(),
                state: "PRESENT_SHA256".into(),
                sha256: now,
            });
        }
        let mut final_review = Sha256::new();
        final_review.update(completion.final_review.as_bytes());
        let evidence = ReviewedPromotionReviewAuthorityEvidenceV1 {
            record_id: record.record_id.clone(),
            session_id: record.session_id.clone(),
            project_id: record.project_id.clone(),
            state: "COMPLETED_VERIFIED".into(),
            next_action: record.next_action.clone(),
            reference: record.reference.clone(),
            approved_contract_hash: contract_hash,
            logical_task_id: task_id.into(),
            completion_verification: "PASSED".into(),
            final_review_sha256: format!("{:x}", final_review.finalize()),
            completion_artifact_ids,
            baseline_observations: baseline.observations,
            current_observations,
        };
        let serialized = serde_json::to_vec(&evidence)
            .map_err(|_| "independent review authority is unavailable".to_string())?;
        let mut digest = Sha256::new();
        digest.update(serialized);
        let snapshot_expected = ReviewedSourceSnapshotExpectedV1 {
            session_id: record.session_id.clone(),
            project_id: record.project_id.clone(),
            approved_contract_hash: evidence.approved_contract_hash.clone(),
            logical_task_id: evidence.logical_task_id.clone(),
            completion_artifact_ids: evidence.completion_artifact_ids.clone(),
            current_outputs: evidence
                .current_observations
                .iter()
                .map(|observation| {
                    observation
                        .sha256
                        .clone()
                        .map(|sha256| ReviewedSourceCurrentOutputV1 {
                            artifact_id: observation.artifact_id.clone(),
                            sha256,
                        })
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| "independent review authority is unavailable".to_string())?,
            baseline_observations: evidence
                .baseline_observations
                .iter()
                .map(|observation| ReviewedSourceBaselineObservationV1 {
                    artifact_id: observation.artifact_id.clone(),
                    state: observation.state.clone(),
                    sha256: observation.sha256.clone(),
                })
                .collect(),
        };
        Ok(ReviewedPromotionReviewAuthorityV1 {
            session_id: record.session_id.clone(),
            record_id: record.record_id.clone(),
            digest: format!("{:x}", digest.finalize()),
            approved_contract_hash: evidence.approved_contract_hash.clone(),
            completion_final_review_sha256: evidence.final_review_sha256.clone(),
            snapshot_expected,
        })
    }

    fn resolve_daemon_reload_review_authority(
        &self,
        record_id: &str,
        request: &DaemonReloadApprovalRequestV1,
    ) -> Result<DaemonReloadReviewAuthorityV1, String> {
        validate_daemon_reload_request(request)
            .map_err(|_| "daemon reload review authority is unavailable".to_string())?;
        let reviewed = self.resolve_reviewed_promotion_review_authority(record_id)?;
        let expected_artifacts = reviewed
            .snapshot_expected
            .current_outputs
            .iter()
            .filter(|output| output.artifact_id == DAEMON_RELOAD_APPROVAL_REVIEW_ARTIFACT_ID)
            .collect::<Vec<_>>();
        if expected_artifacts.len() != 1 {
            return Err("daemon reload review authority is unavailable".into());
        }
        let contract = self
            .store
            .load_contract(&reviewed.session_id)
            .map_err(|_| "daemon reload review authority is unavailable".to_string())?;
        let policy = AutonomousPolicyEngineV1::new(contract)
            .map_err(|_| "daemon reload review authority is unavailable".to_string())?;
        let expected_sha256 = &expected_artifacts[0].sha256;
        let before = safe_task_output_hash(&policy, DAEMON_RELOAD_APPROVAL_REVIEW_ARTIFACT_ID)
            .map_err(|_| "daemon reload review authority is unavailable".to_string())?;
        if before.as_deref() != Some(expected_sha256.as_str()) {
            return Err("daemon reload review authority is unavailable".into());
        }
        let artifact_path = self
            .workspace
            .join(DAEMON_RELOAD_APPROVAL_REVIEW_ARTIFACT_ID);
        let bytes = std::fs::read(artifact_path)
            .map_err(|_| "daemon reload review authority is unavailable".to_string())?;
        let mut artifact_digest = Sha256::new();
        artifact_digest.update(&bytes);
        if format!("{:x}", artifact_digest.finalize()) != *expected_sha256
            || safe_task_output_hash(&policy, DAEMON_RELOAD_APPROVAL_REVIEW_ARTIFACT_ID)
                .map_err(|_| "daemon reload review authority is unavailable".to_string())?
                .as_deref()
                != Some(expected_sha256.as_str())
        {
            return Err("daemon reload review authority is unavailable".into());
        }
        let artifact = parse_daemon_reload_review_artifact(&bytes)
            .map_err(|_| "daemon reload review authority is unavailable".to_string())?;
        daemon_reload_artifact_binds_request(&artifact, request)
            .map_err(|_| "daemon reload review authority is unavailable".to_string())?;
        let authority = DaemonReloadReviewAuthorityV1 {
            schema_version: DAEMON_RELOAD_APPROVAL_SCHEMA_VERSION,
            product: DAEMON_RELOAD_APPROVAL_PRODUCT.into(),
            project_id: DAEMON_RELOAD_APPROVAL_PROJECT.into(),
            purpose: DAEMON_RELOAD_APPROVAL_PURPOSE.into(),
            request_sha256: canonical_daemon_reload_request_sha256(request)
                .map_err(|_| "daemon reload review authority is unavailable".to_string())?,
            review_record_id: reviewed.record_id,
            review_session_id: reviewed.session_id,
            review_contract_hash: reviewed.approved_contract_hash,
            review_completion_sha256: reviewed.completion_final_review_sha256,
            remeasurement_digest: reviewed.digest,
            authority_sha256: String::new(),
        };
        let serialized = serde_json::to_vec(&authority)
            .map_err(|_| "daemon reload review authority is unavailable".to_string())?;
        let mut digest = Sha256::new();
        digest.update(DAEMON_RELOAD_APPROVAL_PURPOSE.as_bytes());
        digest.update(serialized);
        let authority = DaemonReloadReviewAuthorityV1 {
            authority_sha256: format!("{:x}", digest.finalize()),
            ..authority
        };
        validate_daemon_reload_authority(&authority, request)
            .map_err(|_| "daemon reload review authority is unavailable".to_string())?;
        Ok(authority)
    }

    fn resolve_core_host_gate_review_authority(
        &self,
        record_id: &str,
        request: &CoreHostGateApprovalRequestV1,
    ) -> Result<CoreHostGateReviewAuthorityV1, String> {
        validate_request(request)
            .map_err(|_| "core host gate review authority is unavailable".to_string())?;
        // The generic acknowledged-completion remeasurement is necessary but
        // insufficient. The exact fixed artifact below must also be a current
        // immutable completion output and bind this exact typed request.
        let reviewed = self.resolve_reviewed_promotion_review_authority(record_id)?;
        let expected_artifacts = reviewed
            .snapshot_expected
            .current_outputs
            .iter()
            .filter(|output| output.artifact_id == CORE_HOST_GATE_APPROVAL_REVIEW_ARTIFACT_ID)
            .collect::<Vec<_>>();
        if expected_artifacts.len() != 1 {
            return Err("core host gate review authority is unavailable".into());
        }
        let contract = self
            .store
            .load_contract(&reviewed.session_id)
            .map_err(|_| "core host gate review authority is unavailable".to_string())?;
        let policy = AutonomousPolicyEngineV1::new(contract)
            .map_err(|_| "core host gate review authority is unavailable".to_string())?;
        let expected_sha256 = &expected_artifacts[0].sha256;
        let before = safe_task_output_hash(&policy, CORE_HOST_GATE_APPROVAL_REVIEW_ARTIFACT_ID)
            .map_err(|_| "core host gate review authority is unavailable".to_string())?;
        if before.as_deref() != Some(expected_sha256.as_str()) {
            return Err("core host gate review authority is unavailable".into());
        }
        let artifact_path = self
            .workspace
            .join(CORE_HOST_GATE_APPROVAL_REVIEW_ARTIFACT_ID);
        let bytes = std::fs::read(artifact_path)
            .map_err(|_| "core host gate review authority is unavailable".to_string())?;
        let mut artifact_digest = Sha256::new();
        artifact_digest.update(&bytes);
        if format!("{:x}", artifact_digest.finalize()) != *expected_sha256
            || safe_task_output_hash(&policy, CORE_HOST_GATE_APPROVAL_REVIEW_ARTIFACT_ID)
                .map_err(|_| "core host gate review authority is unavailable".to_string())?
                .as_deref()
                != Some(expected_sha256.as_str())
        {
            return Err("core host gate review authority is unavailable".into());
        }
        let artifact = parse_review_artifact(&bytes)
            .map_err(|_| "core host gate review authority is unavailable".to_string())?;
        review_artifact_binds_request(&artifact, request)
            .map_err(|_| "core host gate review authority is unavailable".to_string())?;
        let authority = CoreHostGateReviewAuthorityV1 {
            schema_version: 1,
            product: "CatDesk".into(),
            project_id: "catdesk".into(),
            purpose: CORE_HOST_GATE_APPROVAL_PURPOSE.into(),
            request_sha256: canonical_request_sha256(request)
                .map_err(|_| "core host gate review authority is unavailable".to_string())?,
            review_record_id: reviewed.record_id,
            review_session_id: reviewed.session_id,
            review_contract_hash: reviewed.approved_contract_hash,
            review_completion_sha256: reviewed.completion_final_review_sha256,
            remeasurement_digest: reviewed.digest,
            authority_sha256: String::new(),
        };
        let serialized = serde_json::to_vec(&authority)
            .map_err(|_| "independent review authority is unavailable".to_string())?;
        let mut digest = Sha256::new();
        digest.update(CORE_HOST_GATE_APPROVAL_PURPOSE.as_bytes());
        digest.update(serialized);
        Ok(CoreHostGateReviewAuthorityV1 {
            authority_sha256: format!("{:x}", digest.finalize()),
            ..authority
        })
    }

    fn resolve_ordinary_worker_pair_review_authority(
        &self,
        record_id: &str,
        request: &OrdinaryWorkerPairApprovalRequestV1,
    ) -> Result<OrdinaryWorkerPairReviewAuthorityV1, String> {
        validate_ordinary_worker_pair_request(request)
            .map_err(|_| "ordinary worker pair review authority is unavailable".to_string())?;
        // Generic acknowledged-completion remeasurement is necessary but never
        // sufficient: one exact immutable output must bind this exact request.
        let reviewed = self.resolve_reviewed_promotion_review_authority(record_id)?;
        let expected_artifacts = reviewed
            .snapshot_expected
            .current_outputs
            .iter()
            .filter(|output| output.artifact_id == ORDINARY_WORKER_PAIR_APPROVAL_REVIEW_ARTIFACT_ID)
            .collect::<Vec<_>>();
        if expected_artifacts.len() != 1 {
            return Err("ordinary worker pair review authority is unavailable".into());
        }
        let contract = self
            .store
            .load_contract(&reviewed.session_id)
            .map_err(|_| "ordinary worker pair review authority is unavailable".to_string())?;
        let policy = AutonomousPolicyEngineV1::new(contract)
            .map_err(|_| "ordinary worker pair review authority is unavailable".to_string())?;
        let expected_sha256 = &expected_artifacts[0].sha256;
        let measured =
            safe_task_output_hash(&policy, ORDINARY_WORKER_PAIR_APPROVAL_REVIEW_ARTIFACT_ID)
                .map_err(|_| "ordinary worker pair review authority is unavailable".to_string())?;
        if measured.as_deref() != Some(expected_sha256.as_str()) {
            return Err("ordinary worker pair review authority is unavailable".into());
        }
        let bytes = std::fs::read(
            self.workspace
                .join(ORDINARY_WORKER_PAIR_APPROVAL_REVIEW_ARTIFACT_ID),
        )
        .map_err(|_| "ordinary worker pair review authority is unavailable".to_string())?;
        let mut artifact_digest = Sha256::new();
        artifact_digest.update(&bytes);
        if format!("{:x}", artifact_digest.finalize()) != *expected_sha256
            || safe_task_output_hash(&policy, ORDINARY_WORKER_PAIR_APPROVAL_REVIEW_ARTIFACT_ID)
                .map_err(|_| "ordinary worker pair review authority is unavailable".to_string())?
                .as_deref()
                != Some(expected_sha256.as_str())
        {
            return Err("ordinary worker pair review authority is unavailable".into());
        }
        let artifact = parse_ordinary_worker_pair_review_artifact(&bytes)
            .map_err(|_| "ordinary worker pair review authority is unavailable".to_string())?;
        ordinary_worker_pair_artifact_binds_request(&artifact, request)
            .map_err(|_| "ordinary worker pair review authority is unavailable".to_string())?;
        self.revalidate_ordinary_worker_pair_descriptor(&request.predecessor)?;
        self.revalidate_ordinary_worker_pair_descriptor(&request.current)?;
        let authority = OrdinaryWorkerPairReviewAuthorityV1 {
            schema_version: 1,
            product: "CatDesk".into(),
            project_id: "catdesk".into(),
            purpose: ORDINARY_WORKER_PAIR_APPROVAL_PURPOSE.into(),
            request_sha256: canonical_pair_request_sha256(request)
                .map_err(|_| "ordinary worker pair review authority is unavailable".to_string())?,
            review_record_id: reviewed.record_id,
            review_session_id: reviewed.session_id,
            review_contract_hash: reviewed.approved_contract_hash,
            review_completion_sha256: reviewed.completion_final_review_sha256,
            remeasurement_digest: reviewed.digest,
            authority_sha256: String::new(),
        };
        let serialized = serde_json::to_vec(&authority)
            .map_err(|_| "ordinary worker pair review authority is unavailable".to_string())?;
        let mut digest = Sha256::new();
        digest.update(ORDINARY_WORKER_PAIR_APPROVAL_PURPOSE.as_bytes());
        digest.update(serialized);
        Ok(OrdinaryWorkerPairReviewAuthorityV1 {
            authority_sha256: format!("{:x}", digest.finalize()),
            ..authority
        })
    }

    fn revalidate_ordinary_worker_pair_descriptor(
        &self,
        descriptor: &OrdinaryWorkerPairDescriptorV1,
    ) -> Result<(), String> {
        // A descriptor cannot borrow the pair-approval review itself. Its
        // reviewed source/build provenance must remeasure independently before
        // the pair approval may bind it.
        let reviewed = self
            .resolve_reviewed_promotion_review_authority(&descriptor.review_record_id)
            .map_err(|_| "ordinary worker pair descriptor authority is unavailable".to_string())?;
        if reviewed.session_id != descriptor.review_session_id
            || reviewed.record_id != descriptor.review_record_id
            || reviewed.digest != descriptor.review_authority_sha256
            || reviewed.approved_contract_hash != descriptor.review_contract_hash
            || reviewed.completion_final_review_sha256 != descriptor.review_completion_sha256
            || reviewed.digest != descriptor.review_remeasurement_digest
        {
            return Err("ordinary worker pair descriptor authority is unavailable".into());
        }
        let snapshot = validate_committed_snapshot(&self.workspace, &reviewed.snapshot_expected)
            .map_err(|_| "ordinary worker pair descriptor authority is unavailable".to_string())?;
        if snapshot.snapshot_id != descriptor.source_snapshot_id {
            return Err("ordinary worker pair descriptor authority is unavailable".into());
        }
        let candidate_path = format!(
            "target/reviewed-builds/{}/catdesk.exe",
            descriptor.attestation_id
        );
        let attestation = validate_producer_attestation(
            &self.workspace,
            &descriptor.review_session_id,
            &descriptor.review_record_id,
            &descriptor.review_authority_sha256,
            &reviewed.snapshot_expected,
            &candidate_path,
            &descriptor.image_sha256,
        )
        .map_err(|_| "ordinary worker pair descriptor authority is unavailable".to_string())?;
        if attestation.build_attempt_id != descriptor.attestation_id
            || attestation.attestation_digest != descriptor.attestation_sha256
            || attestation.candidate_sha256 != descriptor.image_sha256
            || attestation.candidate_length != descriptor.image_length
        {
            return Err("ordinary worker pair descriptor authority is unavailable".into());
        }
        Ok(())
    }

    fn reviewed_build(&self, args: &Value) -> Result<Value, String> {
        let object = args
            .as_object()
            .ok_or_else(|| "reviewed build request is invalid".to_string())?;
        let action = required_str(args, "action")?;
        let allowed: &[&str] = match action {
            "PREPARE" | "PREFLIGHT" => &["action", "recordId"],
            "CONFIRM" => &["action", "confirmationToken"],
            "RESULT" => &["action"],
            _ => return Err("reviewed build request is invalid".into()),
        };
        if object.len() != allowed.len()
            || object.keys().any(|key| !allowed.contains(&key.as_str()))
        {
            return Err("reviewed build request is invalid".into());
        }
        if action == "RESULT" {
            let result = crate::reviewed_build::reviewed_build_result(&self.workspace)
                .map_err(|_| "reviewed build result is unavailable".to_string())?;
            return Ok(
                json!({"state":result.state,"tunnelAction":"none-external-tunnel-untouched"}),
            );
        }
        if matches!(action, "PREPARE" | "PREFLIGHT") {
            let authority =
                self.resolve_reviewed_promotion_review_authority(required_str(args, "recordId")?)?;
            let (token, outcome) = crate::reviewed_build::prepare_reviewed_build(
                &self.workspace,
                &authority.record_id,
                &authority.digest,
                &authority.snapshot_expected,
            )
            .map_err(|_| "reviewed build preparation was rejected".to_string())?;
            return Ok(
                json!({"state":outcome.state,"confirmationToken":token,"tunnelAction":"none-external-tunnel-untouched"}),
            );
        }
        let token = required_str(args, "confirmationToken")?;
        let workspace = self.workspace.clone();
        let outcome =
            crate::reviewed_build::confirm_reviewed_build(&workspace, token, |attempt, owner| {
                let current = std::env::current_exe()
                    .map_err(|_| "REVIEWED_BUILD_HELPER_UNAVAILABLE".to_string())?;
                let mut command = std::process::Command::new(current);
                command
                    .arg(crate::reviewed_build::REVIEWED_BUILD_WORKER_FLAG)
                    .arg("--workspace")
                    .arg(&workspace)
                    .arg("--attempt")
                    .arg(&attempt.build_attempt_id)
                    .arg("--owner")
                    .arg(owner)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                command
                    .spawn()
                    .map(|_| ())
                    .map_err(|_| "REVIEWED_BUILD_HELPER_UNAVAILABLE".into())
            })
            .map_err(|_| "reviewed build confirmation was rejected".to_string())?;
        Ok(json!({"state":outcome.state,"tunnelAction":"none-external-tunnel-untouched"}))
    }

    fn reviewed_build_promotion(&self, args: &Value) -> Result<Value, String> {
        let object = args
            .as_object()
            .ok_or_else(|| "reviewed promotion request is invalid".to_string())?;
        let action = required_str(args, "action")?;
        let expected_keys: &[&str] = match action {
            "PREFLIGHT" => &["action", "buildPath", "expectedSha256", "recordId"],
            "CONFIRM" => &["action", "buildPath", "expectedSha256", "confirmationToken"],
            "RESULT" => &["action"],
            _ => return Err("reviewed promotion action is invalid".into()),
        };
        if object.len() != expected_keys.len()
            || object
                .keys()
                .any(|key| !expected_keys.contains(&key.as_str()))
        {
            return Err("reviewed promotion request is invalid".into());
        }
        if action == "RESULT" {
            let result = crate::daemon_reload::reviewed_promotion_result(&self.workspace)
                .map_err(|_| "reviewed promotion result is unavailable".to_string())?;
            return Ok(match result {
                Some(result) => {
                    json!({"state":result.state,"authorizationGeneration":result.authorization_generation,"candidateSha256":result.candidate_sha256,"tunnelAction":"none-external-tunnel-untouched"})
                }
                None => {
                    json!({"state":"RESULT_UNAVAILABLE_OR_PENDING","tunnelAction":"none-external-tunnel-untouched"})
                }
            });
        }
        let candidate = PathBuf::from(required_str(args, "buildPath")?);
        let expected = required_str(args, "expectedSha256")?;
        if action == "PREFLIGHT" {
            let record_id = required_str(args, "recordId")?;
            let review_authority = self.resolve_reviewed_promotion_review_authority(record_id)?;
            let preflight = crate::daemon_reload::prepare_reviewed_promotion(
                &self.workspace,
                &candidate,
                expected,
                &review_authority.session_id,
                &review_authority.record_id,
                &review_authority.digest,
                &review_authority.snapshot_expected,
            )
            .map_err(|_| "reviewed promotion preflight was rejected".to_string())?;
            return Ok(
                json!({"state":"PREFLIGHT_READY","confirmationToken":preflight.confirmation_token,"expiresAtUnix":preflight.expires_at_unix,"candidateSha256":preflight.authorization.candidate_sha256,"tunnelAction":"none-external-tunnel-untouched"}),
            );
        }
        let bound = crate::daemon_reload::reviewed_promotion_preflight_authority(&self.workspace)
            .map_err(|_| "reviewed promotion confirmation was rejected".to_string())?;
        let review_authority = self
            .resolve_reviewed_promotion_review_authority(&bound.review_record_id)
            .map_err(|_| "reviewed promotion confirmation was rejected".to_string())?;
        if review_authority.session_id != bound.review_session_id
            || review_authority.record_id != bound.review_record_id
            || review_authority.digest != bound.review_record_sha256
        {
            return Err("reviewed promotion confirmation was rejected".into());
        }
        let outcome = crate::daemon_reload::confirm_reviewed_promotion(
            &self.workspace,
            &candidate,
            expected,
            required_str(args, "confirmationToken")?,
        )
        .map_err(|_| "reviewed promotion confirmation was rejected".to_string())?;
        let state = match outcome.state {
            "NEWLY_SCHEDULED" => "PROMOTION_SCHEDULED",
            "WORKER_OWNED_PENDING" => "PROMOTION_IN_PROGRESS",
            "CLAIMED_PENDING_UNPROVEN" => "PROMOTION_CLAIMED_PENDING_UNPROVEN",
            "PROMOTION_COMPLETED" => "PROMOTION_COMPLETED",
            "PROMOTION_FAILED_OR_AMBIGUOUS" => "PROMOTION_FAILED_OR_AMBIGUOUS",
            _ => return Err("reviewed promotion confirmation was rejected".into()),
        };
        Ok(
            json!({"state":state,"authorizationGeneration":outcome.authorization_generation,"candidateSha256":outcome.candidate_sha256,"tunnelAction":"none-external-tunnel-untouched"}),
        )
    }

    /// The sole publication surface. It accepts no repository, branch, path,
    /// executable, commit-message or command selection from MCP callers.
    fn github_publication(&self, args: &Value) -> Result<Value, String> {
        let object = args
            .as_object()
            .ok_or_else(|| "GitHub publication request is invalid".to_string())?;
        let action = required_str(args, "action")?;
        let expected: &[&str] = match action {
            "PREPARE" => &["action", "recordId", "approval"],
            "CONFIRM" | "RESULT" => &["action", "confirmationToken"],
            _ => return Err("GitHub publication action is invalid".into()),
        };
        if object.len() != expected.len()
            || object.keys().any(|key| !expected.contains(&key.as_str()))
        {
            return Err("GitHub publication request is invalid".into());
        }
        let runner = SystemGithubPublicationGitRunnerV1 {
            workspace: self.workspace.clone(),
        };
        if action == "PREPARE" {
            let reviewed = self
                .resolve_reviewed_promotion_review_authority(required_str(args, "recordId")?)
                .map_err(|_| "GitHub publication review authority is unavailable".to_string())?;
            let policy = self.load_policy(&reviewed.session_id)?;
            let mut snapshot = self.snapshot(&reviewed.session_id)?;
            let approval = serde_json::from_value(args["approval"].clone())
                .map_err(|_| "GitHub publication approval is invalid".to_string())?;
            let publication_review = self.publication_review_authority(&reviewed)?;
            let outcome = prepare_github_publication(
                &self.workspace,
                &policy,
                &mut snapshot.github_publication_journal,
                &publication_review,
                approval,
                &runner,
                now_unix(),
            )
            .map_err(|error| error.reason_code().to_string())?;
            self.store
                .save_session(&snapshot)
                .map_err(|_| "GitHub publication preparation could not be persisted".to_string())?;
            return Ok(json!({
                "state":"PREPARED",
                "confirmationToken":outcome.confirmation_token,
                "fingerprint":outcome.fingerprint,
                "expiresAtUnix":outcome.expires_at_unix,
                "includeCount":outcome.include_count,
                "detail":"bounded GitHub publication evidence prepared"
            }));
        }
        let token = required_str(args, "confirmationToken")?;
        let mut snapshot = self.find_github_publication_snapshot(token)?;
        let (prepared, _) = snapshot
            .github_publication_journal
            .prepared(token)
            .map_err(|_| "GitHub publication confirmation is unavailable".to_string())?;
        let prepared_record_id = prepared.review_record_id.clone();
        let prepared_review_digest = prepared.review_digest.clone();
        let reviewed = self
            .resolve_reviewed_promotion_review_authority(&prepared_record_id)
            .map_err(|_| "GitHub publication review authority is unavailable".to_string())?;
        if reviewed.session_id != snapshot.session_id || reviewed.digest != prepared_review_digest {
            return Err("GitHub publication review authority is unavailable".into());
        }
        let review = self.publication_review_authority(&reviewed)?;
        if action == "RESULT" {
            let state = reconcile_github_publication(
                &mut snapshot.github_publication_journal,
                token,
                &runner,
            )
            .map_err(|error| error.reason_code().to_string())?;
            self.store.save_session(&snapshot).map_err(|_| {
                "GitHub publication reconciliation could not be persisted".to_string()
            })?;
            return Ok(
                json!({"state":state,"detail":"bounded remote-ref reconciliation completed"}),
            );
        }
        let permit = confirm_github_publication_local_commit(
            &self.workspace,
            &snapshot.github_publication_journal,
            token,
            &review,
            &runner,
            now_unix(),
        )
        .map_err(|error| error.reason_code().to_string())?;
        // This durable write intentionally precedes the only remote dispatch.
        snapshot
            .github_publication_journal
            .mark_remote_dispatch_started(&permit)
            .map_err(|_| "GitHub publication journal transition was rejected".to_string())?;
        self.store
            .save_session(&snapshot)
            .map_err(|_| "GitHub publication remote intent could not be persisted".to_string())?;
        let prepared_for_push = snapshot
            .github_publication_journal
            .prepared(token)
            .map_err(|_| "GitHub publication confirmation is unavailable".to_string())?
            .0
            .clone();
        if push_github_publication(&prepared_for_push, &runner).is_err() {
            return Ok(
                json!({"state":"REMOTE_OUTCOME_UNKNOWN","detail":"reconciliation required before any retry"}),
            );
        }
        match reconcile_github_publication(&mut snapshot.github_publication_journal, token, &runner)
        {
            Ok(state) => {
                self.store.save_session(&snapshot).map_err(|_| {
                    "GitHub publication reconciliation could not be persisted".to_string()
                })?;
                Ok(json!({"state":state,"detail":"bounded remote-ref reconciliation completed"}))
            }
            Err(_) => Ok(
                json!({"state":"REMOTE_OUTCOME_UNKNOWN","detail":"reconciliation required before any retry"}),
            ),
        }
    }

    fn find_github_publication_snapshot(
        &self,
        token: &str,
    ) -> Result<AutonomousSessionSnapshotV1, String> {
        let matching = self
            .store
            .list_sessions()
            .map_err(|_| "GitHub publication journal is unavailable".to_string())?
            .into_iter()
            .filter(|snapshot| snapshot.github_publication_journal.prepared(token).is_ok())
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err("GitHub publication confirmation is unavailable".into());
        }
        Ok(matching.into_iter().next().expect("exactly one match"))
    }

    fn publication_review_authority(
        &self,
        reviewed: &ReviewedPromotionReviewAuthorityV1,
    ) -> Result<PublicationReviewAuthorityV1, String> {
        let pointer_outputs = reviewed
            .snapshot_expected
            .current_outputs
            .iter()
            .filter(|output| output.artifact_id == CURRENT_GITHUB_PUBLICATION_AUTHORITY_PATH)
            .collect::<Vec<_>>();
        if pointer_outputs.len() != 1 {
            return Err("GitHub publication authority pointer is unavailable".into());
        }
        let pointer_bytes = std::fs::read(
            self.workspace
                .join(CURRENT_GITHUB_PUBLICATION_AUTHORITY_PATH),
        )
        .map_err(|_| "GitHub publication authority pointer is unavailable".to_string())?;
        let pointer_sha256 = format!("{:x}", Sha256::digest(&pointer_bytes));
        if pointer_sha256 != pointer_outputs[0].sha256 {
            return Err("GitHub publication authority pointer is unavailable".into());
        }
        let pointer = parse_current_github_publication_authority(&pointer_bytes)
            .map_err(|_| "GitHub publication authority pointer is unavailable".to_string())?
            .ok_or_else(|| "GitHub publication authority pointer is unbound".to_string())?;

        let descriptor_outputs = reviewed
            .snapshot_expected
            .current_outputs
            .iter()
            .filter(|output| output.artifact_id == pointer.descriptor_path)
            .collect::<Vec<_>>();
        if descriptor_outputs.len() != 1 {
            return Err("GitHub publication descriptor authority is unavailable".into());
        }
        let descriptor_bytes = std::fs::read(self.workspace.join(&pointer.descriptor_path))
            .map_err(|_| "GitHub publication descriptor authority is unavailable".to_string())?;
        let descriptor_sha256 = format!("{:x}", Sha256::digest(&descriptor_bytes));
        if descriptor_sha256 != pointer.descriptor_sha256
            || descriptor_sha256 != descriptor_outputs[0].sha256
        {
            return Err("GitHub publication descriptor authority is unavailable".into());
        }
        let descriptor =
            parse_github_publication_descriptor(&descriptor_bytes, &pointer.descriptor_path)
                .map_err(|_| {
                    "GitHub publication descriptor authority is unavailable".to_string()
                })?;
        if descriptor.supplemental_paths.len() != 3
            || descriptor.supplemental_paths[2] != CURRENT_GITHUB_PUBLICATION_AUTHORITY_PATH
        {
            return Err("GitHub publication descriptor authority is unavailable".into());
        }

        let manifest_outputs = reviewed
            .snapshot_expected
            .current_outputs
            .iter()
            .filter(|output| output.artifact_id == descriptor.manifest_path)
            .collect::<Vec<_>>();
        if manifest_outputs.len() != 1 {
            return Err("GitHub publication manifest authority is unavailable".into());
        }
        let manifest_bytes = std::fs::read(self.workspace.join(&descriptor.manifest_path))
            .map_err(|_| "GitHub publication manifest authority is unavailable".to_string())?;
        let manifest_sha256 = format!("{:x}", Sha256::digest(&manifest_bytes));
        if manifest_sha256 != descriptor.manifest_sha256
            || manifest_sha256 != manifest_outputs[0].sha256
        {
            return Err("GitHub publication manifest authority is unavailable".into());
        }

        for path in &descriptor.supplemental_paths {
            let outputs = reviewed
                .snapshot_expected
                .current_outputs
                .iter()
                .filter(|output| output.artifact_id == *path)
                .collect::<Vec<_>>();
            if outputs.len() != 1 {
                return Err("GitHub publication supplemental authority is unavailable".into());
            }
            let bytes = std::fs::read(self.workspace.join(path)).map_err(|_| {
                "GitHub publication supplemental authority is unavailable".to_string()
            })?;
            let sha256 = format!("{:x}", Sha256::digest(&bytes));
            if sha256 != outputs[0].sha256 {
                return Err("GitHub publication supplemental authority is unavailable".into());
            }
        }

        Ok(PublicationReviewAuthorityV1 {
            record_id: reviewed.record_id.clone(),
            digest: reviewed.digest.clone(),
            descriptor,
        })
    }

    /// The first-class recovery surface intentionally accepts no caller data.
    /// Its detached helper invokes only the project-owned public lifecycle
    /// facade, which retains sole authority over canonical pair repair,
    /// prior-known-good restore, process identity, and official runtime
    /// reattachment.
    fn canonical_release_recovery(&self, args: &Value) -> Result<Value, String> {
        if args.as_object().is_none_or(|object| !object.is_empty()) {
            return Err("CatDesk canonical recovery accepts no arguments".into());
        }
        let active_mutation = self
            .store
            .list_sessions()
            .map_err(|_| "autonomy sessions could not be inspected before recovery".to_string())?
            .into_iter()
            .any(|snapshot| {
                snapshot.active
                    && matches!(
                        snapshot.state,
                        AutonomousSessionStateV1::Running
                            | AutonomousSessionStateV1::Verifying
                            | AutonomousSessionStateV1::RecoveringAfterRestart
                    )
            });
        if active_mutation {
            return Err("CatDesk canonical recovery is blocked while autonomous mutation or verification is active".into());
        }
        let state = crate::daemon_reload::schedule_canonical_recovery(&self.workspace)
            .map_err(|_| "CatDesk canonical recovery requires operator attention".to_string())?;
        Ok(json!({
            "state": state.status,
            "attemptGeneration": state.generation,
            "tunnelAction": "NONE_EXTERNAL_RUNTIME_OWNERSHIP_PRESERVED"
        }))
    }

    fn create_contract(&self, contract: AutonomousDevelopmentContractV1) -> Result<Value, String> {
        let policy = AutonomousPolicyEngineV1::new(contract.clone())
            .map_err(|_| "autonomous contract policy rejected the request".to_string())?;
        if policy.contract().workspace.canonicalize().ok().as_deref() != Some(&self.workspace) {
            return Err("autonomous contract workspace must match the MCP workspace".into());
        }
        let session_id = contract.contract_id.clone();
        let graph = if contract.task_graph.is_empty() {
            vec![(
                contract.task_id.clone(),
                1,
                Vec::new(),
                contract.ordered_steps.clone(),
                contract.completion_artifact_ids.clone(),
            )]
        } else {
            contract
                .task_graph
                .iter()
                .map(|task| {
                    (
                        task.task_id.clone(),
                        task.priority,
                        task.depends_on.clone(),
                        task.acceptance_criteria.clone(),
                        task.completion_artifact_ids.clone(),
                    )
                })
                .collect()
        };
        let queue = AutonomousQueueV1::new(
            graph
                .iter()
                .map(
                    |(task_id, priority, depends_on, _, _)| AutonomousQueueTaskV1 {
                        task_id: task_id.clone(),
                        priority: *priority,
                        depends_on: depends_on.clone(),
                        state: AutonomousQueueTaskStateV1::Ready,
                    },
                )
                .collect(),
        )
        .map_err(|_| "autonomous queue could not be created".to_string())?;
        let mut snapshot = self
            .store
            .create_session(&session_id, queue)
            .map_err(|_| "autonomous session already exists or could not be created".to_string())?;
        if contract.starts_on_routine_provider() {
            snapshot.provider_route = AutonomousProviderRouteV1::QwenFallbackActive;
            if self.store.save_session(&snapshot).is_err() {
                let _ = self.store.remove_unapproved_session(&session_id);
                return Err("autonomous routine-provider route could not be persisted".into());
            }
        }
        if self.store.save_contract(&session_id, &contract).is_err() {
            let _ = self.store.remove_unapproved_session(&session_id);
            return Err("autonomous contract could not be persisted".into());
        }
        if self
            .store
            .write_plan_queue(
                &session_id,
                &AutonomousPlanQueueV1 {
                    schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
                    tasks: graph
                        .iter()
                        .map(
                            |(
                                task_id,
                                _priority,
                                depends_on,
                                acceptance_criteria,
                                completion_artifact_ids,
                            )| {
                                AutonomousPlannedTaskV1 {
                                    task_id: task_id.clone(),
                                    depends_on: depends_on.clone(),
                                    acceptance_criteria: acceptance_criteria.clone(),
                                    allowed_paths: contract
                                        .allowed_paths
                                        .iter()
                                        .map(|path| path.to_string_lossy().into_owned())
                                        .collect(),
                                    verification_profile: contract
                                        .verification_policy
                                        .profile
                                        .clone(),
                                    preferred_worker_policy:
                                        "codex-preferred-local-qwen-continuation".into(),
                                    escalation_conditions: vec![
                                        "architecture ambiguity".into(),
                                        "scope expansion".into(),
                                    ],
                                    completion_artifact_ids: completion_artifact_ids.clone(),
                                    provider_provenance: vec!["codex-cli".into()],
                                }
                            },
                        )
                        .collect(),
                },
            )
            .is_err()
        {
            let _ = self.store.remove_unapproved_session(&session_id);
            return Err("autonomous plan queue could not be persisted".into());
        }
        if self
            .store
            .append_event(
                &session_id,
                "contract_created",
                "contract persisted and awaiting approval",
            )
            .is_err()
        {
            let _ = self.store.remove_unapproved_session(&session_id);
            return Err("autonomous event could not be persisted".into());
        }
        Ok(json!({"sessionId":session_id,"contractHash":policy.contract_hash(),"state":"DRAFT"}))
    }

    fn validate_contract(&self, session_id: &str) -> Result<Value, String> {
        let contract = self.load_policy(session_id)?;
        self.validate_graph_materialization(session_id, &contract)?;
        Ok(json!({"valid":true,"contractHash":contract.contract_hash()}))
    }

    fn approve(
        &self,
        session_id: &str,
        expected: u64,
        key: &str,
        approval_id: &str,
        decision_hash: &str,
    ) -> Result<Value, String> {
        let policy = self.load_policy(session_id)?;
        self.validate_graph_materialization(session_id, &policy)?;
        if decision_hash != policy.contract_hash() {
            return Err("approval decision hash does not match the persisted contract".into());
        }
        self.mutate(session_id, expected, key, "approve", |snapshot| {
            snapshot.approved_contract_hash = Some(decision_hash.into());
            snapshot.state = AutonomousSessionStateV1::Queued;
            snapshot.active = true;
            let _ = approval_id;
            Ok(())
        })
    }

    fn start(&self, session_id: &str, expected: u64, key: &str) -> Result<Value, String> {
        let policy = self.load_policy(session_id)?;
        self.validate_graph_materialization(session_id, &policy)?;
        let result = self.mutate(session_id, expected, key, "start", |snapshot| {
            if policy.contract().autonomy_lease.start_approval_required
                && snapshot.approved_contract_hash.as_deref() != Some(policy.contract_hash())
            {
                return Err("start approval is required for this autonomous contract".into());
            }
            if !matches!(
                snapshot.state,
                AutonomousSessionStateV1::Queued | AutonomousSessionStateV1::RecoveringAfterRestart
            ) {
                return Err("autonomy session is not ready to start".into());
            }
            // The controller tick is deliberately started by CatDesk's local runtime,
            // not by MCP-provided executable or environment values.
            snapshot.state = AutonomousSessionStateV1::Queued;
            snapshot.active = true;
            Ok(())
        })?;
        self.reconcile_queued_restart_task(session_id, result)
    }

    fn status(&self, session_id: &str) -> Result<Value, String> {
        let snapshot = self.snapshot(session_id)?;
        Ok(json!({
            "sessionId": snapshot.session_id,
            "state": snapshot.state,
            "stateVersion": snapshot.last_event_sequence,
            "providerThreadCaptured": snapshot.provider_thread_id.is_some(),
            "retryNotBeforeUnix": snapshot.retry_not_before_unix,
            "repairAttempts": snapshot.repair_attempts,
            "providerTurnCount": snapshot.provider_turn_count,
            "active": snapshot.active
        }))
    }

    fn events(
        &self,
        session_id: &str,
        after: u64,
        limit: usize,
        max_bytes: usize,
    ) -> Result<Value, String> {
        let events = self
            .store
            .poll_events(session_id, after)
            .map_err(|_| "autonomy events are unavailable".to_string())?;
        let mut used = 0usize;
        let mut selected = Vec::new();
        for event in events {
            let item = serde_json::to_value(&event)
                .map_err(|_| "autonomy event serialization failed".to_string())?;
            let bytes = item.to_string().len();
            if selected.len() >= limit || used.saturating_add(bytes) > max_bytes {
                break;
            }
            used += bytes;
            selected.push(item);
        }
        let next = selected
            .last()
            .and_then(|event| event.get("sequence"))
            .and_then(Value::as_u64)
            .unwrap_or(after);
        Ok(
            json!({"events":selected,"nextSequence":next,"hasMore":false,"sessionState":self.snapshot(session_id)?.state}),
        )
    }

    fn set_state(
        &self,
        session_id: &str,
        expected: u64,
        key: &str,
        state: AutonomousSessionStateV1,
        event: &str,
    ) -> Result<Value, String> {
        let requeue_requested = state == AutonomousSessionStateV1::Queued;
        let mut direct_chatgpt_resume = false;
        if requeue_requested {
            let snapshot = self.snapshot(session_id)?;
            let policy = self.load_policy(session_id)?;
            if snapshot.state == AutonomousSessionStateV1::WaitingForChatgpt
                && snapshot.current_task_id.as_ref().is_some_and(|task_id| {
                    policy
                        .contract()
                        .task_graph
                        .iter()
                        .any(|task| task.task_id == *task_id && task.planner_gate.is_some())
                })
            {
                return Err("planned ChatGPT gate must be requeued by its matching reply".into());
            }
            let direct_chatgpt_candidate = snapshot.state == AutonomousSessionStateV1::Paused
                && snapshot.provider_route == AutonomousProviderRouteV1::WaitingForChatgpt
                && snapshot.provider_turn_count > 0
                && snapshot.provider_thread_id.is_none()
                && snapshot.provider_handle_id.is_none()
                && snapshot.current_task_id.is_some();
            if direct_chatgpt_candidate {
                let task_id = snapshot
                    .current_task_id
                    .as_deref()
                    .expect("direct ChatGPT candidate requires a current task");
                let queue = self.store.load_queue(session_id).map_err(|_| {
                    "paused direct ChatGPT ownership queue evidence is unavailable".to_string()
                })?;
                if !queue.tasks.iter().any(|task| {
                    task.task_id == task_id
                        && task.state == AutonomousQueueTaskStateV1::WorkerRunning
                }) {
                    return Err(
                        "paused direct ChatGPT ownership queue evidence is inconsistent".into(),
                    );
                }
                direct_chatgpt_resume = true;
            }
        }
        let target_state = if direct_chatgpt_resume {
            AutonomousSessionStateV1::WaitingForChatgpt
        } else {
            state
        };
        let result = self.mutate(session_id, expected, key, event, |snapshot| {
            if snapshot.state.is_terminal() {
                return Err("terminal autonomous session cannot be changed".into());
            }
            snapshot.state = target_state;
            snapshot.active = true;
            Ok(())
        })?;
        if requeue_requested && !direct_chatgpt_resume {
            self.reconcile_queued_restart_task(session_id, result)
        } else {
            Ok(result)
        }
    }

    /// Start/resume reports a bounded pending state when durable ownership is
    /// not sufficient to requeue a stale worker. The runtime invokes the same
    /// primitive before host/provider preflight, so this response can never
    /// become a hidden provider-launch permit.
    fn reconcile_queued_restart_task(
        &self,
        session_id: &str,
        mut result: Value,
    ) -> Result<Value, String> {
        match reconcile_restart_task_state(&self.store, session_id)
            .map_err(|_| "restart task reconciliation is unavailable".to_string())?
        {
            RestartTaskReconciliationV1::Pending => {
                if let Some(object) = result.as_object_mut() {
                    object.insert(
                        "restartReconciliation".into(),
                        Value::String("PENDING_RECOVERY".into()),
                    );
                }
                Ok(result)
            }
            RestartTaskReconciliationV1::Requeued => {
                if let Some(object) = result.as_object_mut() {
                    object.insert(
                        "restartReconciliation".into(),
                        Value::String("RECONCILED_READY".into()),
                    );
                }
                Ok(result)
            }
            RestartTaskReconciliationV1::NotApplicable
            | RestartTaskReconciliationV1::AlreadyReady => Ok(result),
        }
    }

    fn cancel(&self, session_id: &str, expected: u64, key: &str) -> Result<Value, String> {
        self.mutate(session_id, expected, key, "cancel", |snapshot| {
            snapshot.cancellation_requested = true;
            snapshot.state = AutonomousSessionStateV1::Cancelled;
            snapshot.active = false;
            Ok(())
        })
    }

    fn renew_lease(
        &self,
        session_id: &str,
        expected: u64,
        key: &str,
        new_expiry: u64,
    ) -> Result<Value, String> {
        let mut policy = self
            .store
            .load_contract(session_id)
            .map_err(|_| "autonomous contract is unavailable".to_string())?;
        policy
            .autonomy_lease
            .renew(now_unix(), new_expiry)
            .map_err(|_| "autonomy lease renewal was rejected".to_string())?;
        self.mutate(session_id, expected, key, "renew_lease", |_| Ok(()))?;
        self.store
            .save_contract(session_id, &policy)
            .map_err(|_| "renewed contract could not be persisted".to_string())
            .map(|_| json!({"sessionId":session_id,"renewed":true}))
    }

    fn request_direct_work_claim(
        &self,
        session_id: &str,
        expected: u64,
        key: &str,
    ) -> Result<Value, String> {
        self.mutate(
            session_id,
            expected,
            key,
            "direct_work_claim_requested",
            |snapshot| {
                if snapshot.state != AutonomousSessionStateV1::Queued
                    || !snapshot.active
                    || snapshot.current_task_id.is_some()
                    || snapshot.provider_turn_count != 0
                    || snapshot.approved_contract_hash.is_none()
                {
                    return Err(
                        "direct work claim requires one approved, never-started QUEUED session"
                            .into(),
                    );
                }
                Ok(())
            },
        )
    }

    fn request_direct_work_finalization(
        &self,
        session_id: &str,
        expected: u64,
        key: &str,
    ) -> Result<Value, String> {
        self.mutate(
            session_id,
            expected,
            key,
            "direct_work_finalize_requested",
            |snapshot| {
                if snapshot.state != AutonomousSessionStateV1::WaitingForChatgpt
                    || !snapshot.active
                    || snapshot.current_task_id.is_none()
                    || snapshot.provider_turn_count == 0
                {
                    return Err(
                        "direct work finalization requires one already-started WAITING_FOR_CHATGPT task"
                            .into(),
                    );
                }
                Ok(())
            },
        )
    }

    fn reply(
        &self,
        session_id: &str,
        expected: u64,
        key: &str,
        escalation_id: &str,
        decision_hash: &str,
        decision: &str,
        constraints: Vec<String>,
    ) -> Result<Value, String> {
        if !valid_slug(key) {
            return Err("idempotency key is invalid".into());
        }
        let _lock = self
            .store
            .acquire_lock(session_id, "autonomy-mcp")
            .map_err(|_| "autonomy session is busy".to_string())?;
        let mut snapshot = self.snapshot(session_id)?;
        let action_hash = format!("planner_reply:{expected}");
        if let Some(existing) = snapshot.consumed_idempotency_keys.get(key) {
            if existing == &action_hash {
                return self.status(session_id);
            }
            return Err("idempotency key was previously used for a different action".into());
        }
        if snapshot.last_event_sequence != expected {
            return Err("expected state version does not match persisted state".into());
        }
        if snapshot.state != AutonomousSessionStateV1::WaitingForChatgpt {
            return Err("autonomy session is not awaiting a planner reply".into());
        }
        let policy = self.load_policy(session_id)?;
        let packet = self
            .store
            .load_escalation(session_id)
            .map_err(|_| "current autonomous escalation is unavailable".to_string())?;
        if packet.escalation_id != escalation_id {
            return Err("planner reply does not match the current escalation".into());
        }
        let planned_gate_task = if packet.reason == "planned_architecture_decision_required" {
            let task_id = snapshot
                .current_task_id
                .as_deref()
                .ok_or_else(|| "planned gate has no current task".to_string())?;
            if !policy
                .contract()
                .task_graph
                .iter()
                .any(|task| task.task_id == task_id && task.planner_gate.is_some())
            {
                return Err("planned gate does not match the approved task graph".into());
            }
            Some(task_id.to_owned())
        } else {
            None
        };
        self.store
            .write_planner_reply(
                session_id,
                escalation_id,
                decision_hash,
                decision,
                &constraints,
            )
            .map_err(|_| "bounded planner reply could not be persisted".to_string())?;
        if let Some(task_id) = planned_gate_task {
            self.store
                .satisfy_planner_gate(session_id, &task_id)
                .map_err(|_| "planner gate could not be persisted".to_string())?;
        }
        snapshot.state = AutonomousSessionStateV1::Queued;
        snapshot.active = true;
        snapshot
            .consumed_idempotency_keys
            .insert(key.into(), action_hash);
        self.store
            .save_session(&snapshot)
            .map_err(|_| "autonomy state could not be persisted".to_string())?;
        self.store
            .append_event(session_id, "planner_reply", "autonomous MCP state change")
            .map_err(|_| "autonomy event could not be persisted".to_string())?;
        self.status(session_id)
    }

    fn escalation(&self, session_id: &str) -> Result<Value, String> {
        match self.store.load_escalation(session_id) {
            Ok(packet) => {
                serde_json::to_value(packet).map_err(|_| "escalation serialization failed".into())
            }
            Err(_) => Ok(json!({"available":false})),
        }
    }

    fn diff(&self, session_id: &str) -> Result<Value, String> {
        if self.snapshot(session_id)?.state != AutonomousSessionStateV1::CompletedVerified {
            return Ok(json!({
                "available": false,
                "reason": "authoritative diff is unavailable until verified completion"
            }));
        }
        match self.store.load_completion_artifacts(session_id) {
            Ok(artifacts) => Ok(bounded_artifact_response(
                "authoritativeDiff",
                &artifacts.authoritative_diff,
            )),
            Err(_) => Ok(json!({
                "available": false,
                "reason": "authoritative diff is unavailable until verified completion"
            })),
        }
    }

    fn final_review(&self, session_id: &str) -> Result<Value, String> {
        if self.snapshot(session_id)?.state != AutonomousSessionStateV1::CompletedVerified {
            return Ok(json!({
                "available": false,
                "reason": "final review is unavailable until verified completion"
            }));
        }
        match self.store.load_completion_artifacts(session_id) {
            Ok(artifacts) => Ok(json!({
                "available": true,
                "finalReview": artifacts.final_review,
                "verification": artifacts.verification
            })),
            Err(_) => Ok(json!({
                "available": false,
                "reason": "final review is unavailable until verified completion"
            })),
        }
    }

    fn queue(&self, session_id: &str) -> Result<Value, String> {
        let queue = self
            .store
            .load_queue(session_id)
            .map_err(|_| "autonomy queue is unavailable".to_string())?;
        let plan = self.store.load_plan_queue(session_id).ok();
        Ok(json!({"executionQueue":queue,"plan":plan}))
    }

    /// Bounded accounting summary intended for a normal ChatGPT status
    /// answer.  The durable ledger remains the source of truth; this surface
    /// avoids requiring callers to parse provider diagnostics.
    fn execution_accounting(&self, session_id: &str) -> Result<Value, String> {
        let records = self
            .store
            .execution_accounting_store()
            .map_err(|_| "execution accounting is unavailable".to_string())?
            .list(Some(session_id))
            .map_err(|_| "execution accounting is unavailable".to_string())?;
        let elapsed_millis = records
            .iter()
            .filter_map(|record| record.elapsed_millis)
            .sum::<u128>();
        let provider_turns = records
            .iter()
            .map(|record| u64::from(record.provider_turns))
            .sum::<u64>();
        let tool_calls = records
            .iter()
            .map(|record| u64::from(record.catdesk_tool_calls))
            .sum::<u64>();
        let retries = records
            .iter()
            .map(|record| u64::from(record.retries))
            .sum::<u64>();
        let repairs = records
            .iter()
            .map(|record| u64::from(record.repair_cycles))
            .sum::<u64>();
        Ok(json!({
            "sessionId": session_id,
            "records": records,
            "aggregate": {
                "elapsedMillis": elapsed_millis,
                "providerTurns": provider_turns,
                "catdeskToolCalls": tool_calls,
                "retries": retries,
                "repairCycles": repairs,
                "creditUsagePolicy": "UNKNOWN_NOT_CAPTURED unless authoritative comparable snapshots explicitly support a delta"
            }
        }))
    }

    fn work_time_report(
        &self,
        start_unix: u64,
        end_unix: u64,
        project_id: Option<&str>,
        session_id: Option<&str>,
        task_id: Option<&str>,
        limit: usize,
        max_bytes: usize,
    ) -> Result<Value, String> {
        const MAX_WINDOW_SECONDS: u64 = 31 * 24 * 60 * 60;
        const MAX_ROWS: usize = 100;
        const MAX_BYTES: usize = 24_000;
        if start_unix == 0
            || end_unix <= start_unix
            || end_unix.saturating_sub(start_unix) > MAX_WINDOW_SECONDS
            || limit == 0
            || limit > MAX_ROWS
            || max_bytes == 0
            || max_bytes > MAX_BYTES
            || project_id.is_some_and(|value| !valid_slug(value))
            || session_id.is_some_and(|value| !valid_slug(value))
            || task_id.is_some_and(|value| !valid_slug(value))
        {
            return Err("autonomy work time report window or filters are invalid".into());
        }
        let report = self
            .store
            .execution_accounting_store()
            .map_err(|_| "execution accounting is unavailable".to_string())?
            .work_time_report(
                u128::from(start_unix) * 1_000,
                u128::from(end_unix) * 1_000,
                project_id,
                session_id,
                task_id,
                limit,
                u128::from(now_unix()) * 1_000,
            )
            .map_err(|_| "execution accounting is unavailable or invalid".to_string())?;
        let mut value = serde_json::to_value(report)
            .map_err(|_| "execution accounting report serialization failed".to_string())?;
        if let Some(entries) = value.get_mut("entries").and_then(Value::as_array_mut) {
            for entry in entries {
                let Some(session_id) = entry.get("sessionId").and_then(Value::as_str) else {
                    continue;
                };
                let current_state = self
                    .store
                    .load_session(session_id)
                    .ok()
                    .and_then(|snapshot| serde_json::to_value(snapshot.state).ok())
                    .unwrap_or_else(|| Value::String("UNKNOWN_NOT_LOADED".into()));
                if let Some(entry) = entry.as_object_mut() {
                    entry.insert("currentState".into(), current_state);
                }
            }
        }
        if serde_json::to_vec(&value)
            .map_err(|_| "execution accounting report serialization failed".to_string())?
            .len()
            > max_bytes
        {
            return Err("autonomy work time report exceeds byte limit".into());
        }
        Ok(value)
    }

    fn wake_policy_get(&self) -> Result<Value, String> {
        let policy = self
            .store
            .load_wake_policy()
            .map_err(|_| "autonomy wake policy is unavailable or invalid".to_string())?;
        Ok(
            json!({"policy": policy, "readiness": if super::autonomy_runtime::wake_policy_enablement_ready(&self.workspace) { "READY" } else { "NOT_READY" }}),
        )
    }

    fn wake_policy_set(
        &self,
        expected_generation: u64,
        mode: &str,
        terminal_task_id: Option<&str>,
    ) -> Result<Value, String> {
        let mode = match mode {
            "MANUAL_OFF" => AutonomousWakeModeV1::ManualOff,
            "INDEFINITE" => AutonomousWakeModeV1::Indefinite,
            "THROUGH_TASK" => AutonomousWakeModeV1::ThroughTask,
            "UNTIL_PROVIDER_EXHAUSTED" => AutonomousWakeModeV1::UntilProviderExhausted,
            _ => return Err("autonomy wake mode is invalid".into()),
        };
        if mode != AutonomousWakeModeV1::ManualOff
            && !super::autonomy_runtime::wake_policy_enablement_ready(&self.workspace)
        {
            return Err("autonomy wake enablement prerequisites are not ready".into());
        }
        let policy = self
            .store
            .set_wake_policy(expected_generation, mode, terminal_task_id, now_unix())
            .map_err(|_| "autonomy wake policy update was rejected".to_string())?;
        Ok(json!({"policy": policy}))
    }

    /// Read-only, bounded execution audit. Accounting and review timestamps
    /// establish window overlap; absent timing is reported as incomplete
    /// evidence instead of being reconstructed from chat or filenames.
    fn ticket_audit(
        &self,
        start_unix: u64,
        end_unix: u64,
        project_id: Option<&str>,
        after_session_id: Option<&str>,
        limit: usize,
        max_bytes: usize,
    ) -> Result<Value, String> {
        const MAX_WINDOW_SECONDS: u64 = 31 * 24 * 60 * 60;
        const MAX_ROWS: usize = 100;
        const MAX_BYTES: usize = 24_000;
        if start_unix == 0
            || end_unix <= start_unix
            || end_unix.saturating_sub(start_unix) > MAX_WINDOW_SECONDS
            || limit == 0
            || limit > MAX_ROWS
            || max_bytes == 0
            || max_bytes > MAX_BYTES
            || project_id.is_some_and(|value| !valid_slug(value))
            || after_session_id.is_some_and(|value| !valid_slug(value))
        {
            return Err("autonomy ticket audit window or pagination is invalid".into());
        }
        let start_millis = u128::from(start_unix) * 1_000;
        let end_millis = u128::from(end_unix) * 1_000;
        let accounting = self
            .store
            .execution_accounting_store()
            .map_err(|_| "autonomy execution accounting is unavailable".to_string())?
            .list(None)
            .map_err(|_| "autonomy execution accounting is unavailable".to_string())?;
        let reviews = self
            .store
            .all_review_inbox()
            .map_err(|_| "autonomy review evidence is unavailable".to_string())?;
        let superseded = self
            .store
            .list_session_supersessions()
            .map_err(|_| "autonomy supersession evidence is unavailable".to_string())?
            .into_iter()
            .map(|relation| {
                (
                    relation.superseded_session_id,
                    relation.superseding_session_id,
                )
            })
            .collect::<BTreeMap<_, _>>();
        // Markdown queue data is descriptive only. It cannot add an audit row
        // or establish that work occurred in the requested window.
        let queue_tasks = task_queue::read(&self.workspace.to_string_lossy())
            .ok()
            .map(|queue| {
                queue
                    .tasks
                    .into_iter()
                    .map(|task| {
                        let text = task.text.chars().take(512).collect::<String>();
                        (task.id, json!({"done":task.done,"text":text}))
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        let mut rows = Vec::new();
        let mut incomplete_evidence_count = 0_u64;
        for snapshot in self
            .store
            .list_sessions()
            .map_err(|_| "autonomy session list is unavailable".to_string())?
        {
            let contract = self.store.load_contract(&snapshot.session_id).ok();
            let Some(contract) = contract else {
                incomplete_evidence_count = incomplete_evidence_count.saturating_add(1);
                continue;
            };
            if project_id.is_some_and(|project| contract.project_id != project) {
                continue;
            }
            let session_records = accounting
                .iter()
                .filter(|record| record.catdesk_session_id == snapshot.session_id)
                .collect::<Vec<_>>();
            let session_reviews = reviews
                .iter()
                .filter(|record| record.session_id == snapshot.session_id)
                .collect::<Vec<_>>();
            let first_millis = session_records
                .iter()
                .map(|record| record.started_at_unix_millis)
                .chain(
                    session_reviews
                        .iter()
                        .map(|record| u128::from(record.created_at_unix) * 1_000),
                )
                .min();
            let last_millis = session_records
                .iter()
                .map(|record| {
                    record
                        .ended_at_unix_millis
                        .unwrap_or(record.started_at_unix_millis)
                })
                .chain(
                    session_reviews
                        .iter()
                        .map(|record| u128::from(record.created_at_unix) * 1_000),
                )
                .max();
            let overlaps = first_millis
                .zip(last_millis)
                .is_some_and(|(first, last)| first <= end_millis && last >= start_millis);
            if !overlaps {
                if first_millis.is_none() {
                    incomplete_evidence_count = incomplete_evidence_count.saturating_add(1);
                }
                continue;
            }
            let artifacts = self
                .store
                .load_completion_artifacts(&snapshot.session_id)
                .ok();
            let has_completed_review = session_reviews
                .iter()
                .any(|record| record.state == AutonomousSessionStateV1::CompletedVerified);
            let review_state = if has_completed_review {
                if session_reviews.iter().any(|record| record.unread) {
                    "PENDING_INDEPENDENT_REVIEW"
                } else {
                    "ACKNOWLEDGED"
                }
            } else {
                "NO_COMPLETION_REVIEW_RECORD"
            };
            let missing_completion_evidence = snapshot.state
                == AutonomousSessionStateV1::CompletedVerified
                && artifacts.is_none();
            let status = if superseded.contains_key(&snapshot.session_id) {
                "SUPERSEDED"
            } else if missing_completion_evidence {
                "UNKNOWN_INCOMPLETE_EVIDENCE"
            } else {
                match snapshot.state {
                    AutonomousSessionStateV1::CompletedVerified
                        if review_state == "ACKNOWLEDGED" =>
                    {
                        "COMPLETED_REVIEWED"
                    }
                    AutonomousSessionStateV1::CompletedVerified => "COMPLETED_UNREVIEWED",
                    AutonomousSessionStateV1::WaitingForChatgpt
                    | AutonomousSessionStateV1::WaitingForUser
                    | AutonomousSessionStateV1::Paused
                    | AutonomousSessionStateV1::RateLimited
                    | AutonomousSessionStateV1::Blocked
                    | AutonomousSessionStateV1::CreditBudgetExhausted => "WAITING",
                    AutonomousSessionStateV1::Failed | AutonomousSessionStateV1::LeaseExpired => {
                        "FAILED"
                    }
                    AutonomousSessionStateV1::Cancelled => "CANCELLED",
                    AutonomousSessionStateV1::Draft
                    | AutonomousSessionStateV1::Queued
                    | AutonomousSessionStateV1::Running
                    | AutonomousSessionStateV1::Verifying
                    | AutonomousSessionStateV1::RecoveringAfterRestart => "RUNNING_QUEUED",
                }
            };
            let verification_outcome = artifacts
                .as_ref()
                .map(|artifact| format!("{:?}", artifact.verification.status))
                .or_else(|| {
                    session_records
                        .iter()
                        .rev()
                        .find_map(|record| record.verification_result.clone())
                });
            let attention = self
                .store
                .load_escalation(&snapshot.session_id)
                .ok()
                .map(|packet| packet.reason);
            let task_id = contract.task_id.clone();
            let queue_metadata = queue_tasks.get(&task_id).cloned();
            let references = artifacts.as_ref().map(|_| {
                json!({
                    "completionArtifact": format!("sessions/{}/artifacts/completion.json", snapshot.session_id),
                    "authoritativeDiff": format!("sessions/{}/artifacts/completion.json#authoritativeDiff", snapshot.session_id),
                    "finalReview": format!("sessions/{}/artifacts/completion.json#finalReview", snapshot.session_id),
                })
            });
            rows.push(json!({
                "taskId": task_id,
                "queueMetadata": queue_metadata,
                "sessionId": snapshot.session_id,
                "projectId": contract.project_id,
                "firstActivityUnix": first_millis.map(|value| (value / 1_000) as u64),
                "lastActivityUnix": last_millis.map(|value| (value / 1_000) as u64),
                "sessionState": snapshot.state,
                "auditStatus": status,
                "independentReview": review_state,
                "verifiedChangeSummary": artifacts.as_ref().map(|_| "completion artifact recorded"),
                "verificationOutcome": verification_outcome,
                "unresolvedAttention": attention,
                "references": references,
                "supersededBySessionId": superseded.get(&snapshot.session_id),
                "incompleteEvidence": missing_completion_evidence,
            }));
        }
        rows.sort_by(|left, right| {
            left.get("firstActivityUnix")
                .and_then(Value::as_u64)
                .cmp(&right.get("firstActivityUnix").and_then(Value::as_u64))
                .then_with(|| {
                    left.get("sessionId")
                        .and_then(Value::as_str)
                        .cmp(&right.get("sessionId").and_then(Value::as_str))
                })
        });
        let start = after_session_id
            .and_then(|cursor| {
                rows.iter()
                    .position(|row| row.get("sessionId").and_then(Value::as_str) == Some(cursor))
            })
            .map_or(0, |index| index.saturating_add(1));
        let mut page = Vec::new();
        let mut used_bytes = 0_usize;
        for row in rows.iter().skip(start).take(limit) {
            let row_bytes = serde_json::to_vec(row)
                .map_err(|_| "autonomy ticket audit serialization failed")?
                .len();
            if page.is_empty() && row_bytes > max_bytes {
                return Err("autonomy ticket audit row exceeds response bound".into());
            }
            if used_bytes.saturating_add(row_bytes) > max_bytes {
                break;
            }
            used_bytes = used_bytes.saturating_add(row_bytes);
            page.push(row.clone());
        }
        let has_more = rows.get(start + page.len()).is_some();
        let next_session_id = has_more
            .then(|| page.last())
            .flatten()
            .and_then(|row| row.get("sessionId"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let truncated = next_session_id.is_some();
        Ok(json!({
            "startUnix": start_unix,
            "endUnix": end_unix,
            "projectId": project_id,
            "records": page,
            "nextSessionId": next_session_id,
            "truncated": truncated,
            "incompleteEvidenceCount": incomplete_evidence_count,
            "redactionPolicy": "NO_TRANSCRIPTS_CREDENTIALS_BROWSER_PROFILE_TUNNEL_OR_RAW_ARTIFACT_CONTENT",
        }))
    }

    fn record_supersession(
        &self,
        superseded_session_id: &str,
        superseding_session_id: &str,
    ) -> Result<Value, String> {
        if !valid_slug(superseded_session_id) || !valid_slug(superseding_session_id) {
            return Err("autonomous supersession session id is invalid".into());
        }
        let relation = self
            .store
            .record_session_supersession(superseded_session_id, superseding_session_id, now_unix())
            .map_err(|_| "autonomous supersession relation was rejected".to_string())?;
        Ok(json!({"relation": relation}))
    }

    fn review_inbox(
        &self,
        project_id: Option<&str>,
        after_record_id: Option<&str>,
        limit: usize,
    ) -> Result<Value, String> {
        let records = self
            .store
            .list_review_inbox(project_id, after_record_id, limit)
            .map_err(|_| "autonomy review inbox is unavailable or invalid".to_string())?;
        let next_record_id = records.last().map(|record| record.record_id.clone());
        Ok(
            json!({"records":records,"nextRecordId":next_record_id,"unsolicitedPushAvailable":false}),
        )
    }

    fn ack_review_inbox(&self, record_id: &str) -> Result<Value, String> {
        let record = self
            .store
            .acknowledge_review_inbox(record_id)
            .map_err(|_| "autonomy review inbox acknowledgement was rejected".to_string())?;
        Ok(json!({"record":record,"acknowledged":true}))
    }

    fn list(&self) -> Result<Value, String> {
        let sessions = self
            .store
            .list_sessions()
            .map_err(|_| "autonomy session list is unavailable".to_string())?
            .into_iter()
            .map(|snapshot| {
                json!({
                    "sessionId": snapshot.session_id,
                    "state": snapshot.state,
                    "stateVersion": snapshot.last_event_sequence,
                    "active": snapshot.active
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({"sessions":sessions}))
    }

    fn load_policy(&self, session_id: &str) -> Result<AutonomousPolicyEngineV1, String> {
        AutonomousPolicyEngineV1::new(
            self.store
                .load_contract(session_id)
                .map_err(|_| "autonomous contract is unavailable".to_string())?,
        )
        .map_err(|_| "persisted autonomous contract is invalid".to_string())
    }

    fn validate_graph_materialization(
        &self,
        session_id: &str,
        policy: &AutonomousPolicyEngineV1,
    ) -> Result<(), String> {
        let graph = &policy.contract().task_graph;
        if graph.is_empty() {
            return Ok(());
        }
        let queue = self
            .store
            .load_queue(session_id)
            .map_err(|_| "approved graph queue is unavailable".to_string())?;
        let plan = self
            .store
            .load_plan_queue(session_id)
            .map_err(|_| "approved graph plan is unavailable".to_string())?;
        validate_exact_task_graph_materialization(policy.contract(), &queue, &plan)
            .map_err(|_| "approved graph materialization does not match its contract".into())
    }

    fn snapshot(&self, session_id: &str) -> Result<AutonomousSessionSnapshotV1, String> {
        self.store
            .load_session(session_id)
            .map_err(|_| "autonomous session is unavailable".into())
    }

    fn mutate<F>(
        &self,
        session_id: &str,
        expected: u64,
        key: &str,
        action: &str,
        mutate: F,
    ) -> Result<Value, String>
    where
        F: FnOnce(&mut AutonomousSessionSnapshotV1) -> Result<(), String>,
    {
        if !valid_slug(key) {
            return Err("idempotency key is invalid".into());
        }
        let _lock = self
            .store
            .acquire_lock(session_id, "autonomy-mcp")
            .map_err(|_| "autonomy session is busy".to_string())?;
        let mut snapshot = self.snapshot(session_id)?;
        let action_hash = format!("{action}:{expected}");
        if let Some(existing) = snapshot.consumed_idempotency_keys.get(key) {
            if existing == &action_hash {
                return self.status(session_id);
            }
            return Err("idempotency key was previously used for a different action".into());
        }
        if snapshot.last_event_sequence != expected {
            return Err("expected state version does not match persisted state".into());
        }
        mutate(&mut snapshot)?;
        snapshot
            .consumed_idempotency_keys
            .insert(key.into(), action_hash);
        self.store
            .save_session(&snapshot)
            .map_err(|_| "autonomy state could not be persisted".to_string())?;
        self.store
            .append_event(session_id, action, "autonomous MCP state change")
            .map_err(|_| "autonomy event could not be persisted".to_string())?;
        self.status(session_id)
    }
}

fn required<'a>(args: &'a Value, name: &str) -> Result<&'a Value, String> {
    args.get(name).ok_or_else(|| format!("{name} is required"))
}
fn required_str<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    required(args, name)?
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{name} must be a non-empty string"))
}

fn cached_bind_decision(args: &Value) -> Result<&str, String> {
    let decision = required_str(args, "decision")?;
    if decision.len() > 4_096 || decision.trim() != decision {
        return Err("cached project registry decision is not exact".into());
    }
    Ok(decision)
}

fn cached_bind_keys(args: &Value, allowed: &[&str]) -> Result<(), String> {
    let object = args
        .as_object()
        .ok_or_else(|| "project registry binding arguments are invalid".to_string())?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("cached project registry control rejects unrelated fields".into());
    }
    Ok(())
}

fn cached_optional_project_id(args: &Value) -> Result<Option<&str>, String> {
    let Some(_) = args.get("projectId") else {
        return Ok(None);
    };
    let project_id = required_str(args, "projectId")?;
    if project_id.len() > 128 || !valid_slug(project_id) {
        return Err("cached project registry projectId is invalid".into());
    }
    Ok(Some(project_id))
}

fn validate_cached_conversation_url(conversation_url: &str) -> Result<(), String> {
    if conversation_url.is_empty()
        || conversation_url.len() > 512
        || conversation_url.trim() != conversation_url
        || super::autonomy_projects::canonical_project_chat_target(conversation_url)
            .ok()
            .as_deref()
            != Some(conversation_url)
    {
        return Err("cached project ChatGPT target URL is invalid".into());
    }
    Ok(())
}

fn required_u64(args: &Value, name: &str) -> Result<u64, String> {
    required(args, name)?
        .as_u64()
        .ok_or_else(|| format!("{name} must be an integer"))
}
fn optional_u64(args: &Value, name: &str) -> Option<u64> {
    args.get(name).and_then(Value::as_u64)
}
fn supervisor_now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

/// Bounded Git-only identity inspection for a candidate external workspace.
/// It deliberately does not evaluate caller commands or read repository files.
fn verified_external_git_workspace(
    candidate: &str,
    expected_origin: &str,
) -> Result<PathBuf, String> {
    if expected_origin.is_empty() || expected_origin.len() > 1_024 {
        return Err("external project Git identity is unbounded".into());
    }
    let (workspace, origin) = project_origin_workspace_probe(candidate)
        .map_err(|failure| failure.reason_code().to_string())?;
    if origin != expected_origin {
        return Err("candidate Git origin does not match expected identity".into());
    }
    Ok(workspace)
}

/// Shared, bounded Git-only metadata probe. It accepts only an absolute,
/// non-reparse exact repository root and never writes registry or repository
/// state. Registration reuses this exact root/origin observation before it
/// compares the caller's expected identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectOriginProbeFailure {
    WorkspaceUnbounded,
    WorkspaceNotAbsolute,
    WorkspaceUnavailable,
    WorkspaceReparseOrSymlink,
    WorkspaceCanonicalizationFailed,
    NotGitRoot,
    NestedGitRoot,
    OriginMissing,
    GitExecutableUnavailable,
    GitCommandFailed,
    GitMetadataOversized,
    GitMetadataNonUtf8,
    GitMetadataInvalid,
    GitTopLevelCanonicalizationFailed,
}

impl ProjectOriginProbeFailure {
    const fn reason_code(self) -> &'static str {
        match self {
            Self::WorkspaceUnbounded => "WORKSPACE_UNBOUNDED",
            Self::WorkspaceNotAbsolute => "WORKSPACE_NOT_ABSOLUTE",
            Self::WorkspaceUnavailable => "WORKSPACE_UNAVAILABLE",
            Self::WorkspaceReparseOrSymlink => "WORKSPACE_REPARSE_OR_SYMLINK",
            Self::WorkspaceCanonicalizationFailed => "WORKSPACE_CANONICALIZATION_FAILED",
            Self::NotGitRoot => "NOT_GIT_ROOT",
            Self::NestedGitRoot => "NESTED_GIT_ROOT",
            Self::OriginMissing => "ORIGIN_MISSING",
            Self::GitExecutableUnavailable => "GIT_EXECUTABLE_UNAVAILABLE",
            Self::GitCommandFailed => "GIT_COMMAND_FAILED",
            Self::GitMetadataOversized => "GIT_METADATA_OVERSIZED",
            Self::GitMetadataNonUtf8 => "GIT_METADATA_NON_UTF8",
            Self::GitMetadataInvalid => "GIT_METADATA_INVALID",
            Self::GitTopLevelCanonicalizationFailed => "GIT_TOPLEVEL_CANONICALIZATION_FAILED",
        }
    }
}

impl fmt::Display for ProjectOriginProbeFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.reason_code())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GitMetadataStage {
    TopLevel,
    Origin,
}

/// Fixed Git-only probe for the external-project identity gate. Errors retain
/// their internal stage classification but the public dispatcher emits only a
/// stable reason code; no stderr, repository content, or command output is
/// surfaced on rejection.
fn project_origin_workspace_probe(
    candidate: &str,
) -> Result<(PathBuf, String), ProjectOriginProbeFailure> {
    if candidate.is_empty() || candidate.len() > 4_096 {
        return Err(ProjectOriginProbeFailure::WorkspaceUnbounded);
    }
    let raw = PathBuf::from(candidate);
    if !raw.is_absolute() {
        return Err(ProjectOriginProbeFailure::WorkspaceNotAbsolute);
    }
    let metadata = std::fs::symlink_metadata(&raw)
        .map_err(|_| ProjectOriginProbeFailure::WorkspaceUnavailable)?;
    if path_is_reparse_or_symlink(&metadata) {
        return Err(ProjectOriginProbeFailure::WorkspaceReparseOrSymlink);
    }
    let workspace = raw
        .canonicalize()
        .map_err(|_| ProjectOriginProbeFailure::WorkspaceCanonicalizationFailed)?;
    let has_git_marker = std::fs::symlink_metadata(workspace.join(".git")).is_ok();
    let top_level = match bounded_git_metadata(
        &workspace,
        GitMetadataStage::TopLevel,
        ["rev-parse", "--show-toplevel"],
    ) {
        Err(ProjectOriginProbeFailure::GitCommandFailed) if !has_git_marker => {
            return Err(ProjectOriginProbeFailure::NotGitRoot);
        }
        other => other?,
    };
    let git_root = PathBuf::from(top_level)
        .canonicalize()
        .map_err(|_| ProjectOriginProbeFailure::GitTopLevelCanonicalizationFailed)?;
    if git_root != workspace {
        return Err(ProjectOriginProbeFailure::NestedGitRoot);
    }
    let origin = bounded_git_metadata(
        &workspace,
        GitMetadataStage::Origin,
        ["config", "--get", "remote.origin.url"],
    )?;
    Ok((workspace, origin))
}

fn path_is_reparse_or_symlink(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}
fn bounded_git_metadata<const N: usize>(
    workspace: &Path,
    stage: GitMetadataStage,
    args: [&str; N],
) -> Result<String, ProjectOriginProbeFailure> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(args)
        .output()
        .map_err(|_| ProjectOriginProbeFailure::GitExecutableUnavailable)?;
    validate_bounded_git_metadata(
        stage,
        output.status.success(),
        &output.stdout,
        output.stderr.len(),
    )
}

fn validate_bounded_git_metadata(
    stage: GitMetadataStage,
    command_succeeded: bool,
    stdout: &[u8],
    stderr_len: usize,
) -> Result<String, ProjectOriginProbeFailure> {
    if !command_succeeded {
        return Err(match stage {
            GitMetadataStage::TopLevel => ProjectOriginProbeFailure::GitCommandFailed,
            GitMetadataStage::Origin => ProjectOriginProbeFailure::OriginMissing,
        });
    }
    if stdout.len() > 4_096 || stderr_len > 4_096 {
        return Err(ProjectOriginProbeFailure::GitMetadataOversized);
    }
    let value = String::from_utf8(stdout.to_vec())
        .map_err(|_| ProjectOriginProbeFailure::GitMetadataNonUtf8)?;
    let value = value.trim();
    if value.is_empty() || value.len() > 4_096 || value.contains('\0') {
        return Err(ProjectOriginProbeFailure::GitMetadataInvalid);
    }
    Ok(value.into())
}
fn string_list(args: &Value, name: &str) -> Result<Vec<String>, String> {
    let Some(value) = args.get(name) else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| format!("{name} must be an array"))?;
    if values.len() > 20 {
        return Err(format!("{name} exceeds the bounded item count"));
    }
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.trim().is_empty() && value.len() <= 512)
                .map(str::to_string)
                .ok_or_else(|| format!("{name} contains an invalid item"))
        })
        .collect()
}
fn bounded_artifact_response(name: &str, value: &str) -> Value {
    const MAX_RESPONSE_BYTES: usize = 24_000;
    let mut bounded = value.to_string();
    let truncated = if bounded.len() > MAX_RESPONSE_BYTES {
        bounded.truncate(MAX_RESPONSE_BYTES);
        true
    } else {
        false
    };
    json!({"available":true, name:bounded, "truncated":truncated})
}
fn session_id(args: &Value) -> Result<&str, String> {
    let value = required_str(args, "sessionId")?;
    if valid_slug(value) {
        Ok(value)
    } else {
        Err("sessionId is invalid".into())
    }
}
fn expected_state_version(args: &Value) -> Result<u64, String> {
    required_u64(args, "expectedStateVersion")
}
fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

/// Promotion authority is deliberately stricter than general legacy
/// completion compatibility: a DAG task must name its approved graph member,
/// never inherit the contract-level legacy artifact list.
fn reviewed_promotion_completion_artifact_ids(
    contract: &AutonomousDevelopmentContractV1,
    task_id: &str,
) -> Result<Vec<String>, ()> {
    if contract.task_graph.is_empty() {
        return (task_id == contract.task_id)
            .then(|| contract.completion_artifact_ids.clone())
            .ok_or(());
    }
    let matches = contract
        .task_graph
        .iter()
        .filter(|task| task.task_id == task_id)
        .collect::<Vec<_>>();
    (matches.len() == 1)
        .then(|| matches[0].completion_artifact_ids.clone())
        .ok_or(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn contract(root: &Path) -> AutonomousDevelopmentContractV1 {
        use super::super::autonomous_contract::{
            AutonomousCommandProfileV1, AutonomousGitPolicyV1, AutonomousHardStopV1,
            AutonomousProviderPolicyV1, AutonomousRateLimitPolicyV1,
            AutonomousVerificationPolicyV1, AutonomyLeaseV1,
        };
        AutonomousDevelopmentContractV1 {
            schema_version: 1,
            contract_id: "session-1".into(),
            task_id: "work".into(),
            project_id: "project".into(),
            mode: "chatgpt_web_codex_autonomous".into(),
            objective: "test".into(),
            workspace: root.into(),
            base_branch: "main".into(),
            feature_branch: "feature-test".into(),
            base_commit: "1234567".into(),
            expected_origin: "https://example.invalid/repo.git".into(),
            ordered_steps: vec!["work".into()],
            allowed_paths: vec![root.join("src")],
            forbidden_paths: vec![],
            allowed_command_profiles: vec![AutonomousCommandProfileV1::CargoTest],
            git_policy: AutonomousGitPolicyV1 {
                create_branch: true,
                create_worktree: true,
                local_commits: false,
                push_feature_branch: false,
                open_pull_request: false,
                merge: false,
                force_push: false,
                modify_main: false,
            },
            provider_policy: AutonomousProviderPolicyV1 {
                primary_provider: "codex-cli".into(),
                primary_model: "gpt-5.6-terra".into(),
                routine_provider: "ollama".into(),
                routine_model: "qwen".into(),
                allow_paid_fallback: false,
                allow_cloud_fallback: false,
            },
            verification_policy: AutonomousVerificationPolicyV1 {
                profile: "test".into(),
                required_commands: vec![],
                max_repair_cycles: 1,
                require_authoritative_diff: true,
                require_final_review: true,
            },
            rate_limit_policy: AutonomousRateLimitPolicyV1 {
                automatic_pause: true,
                automatic_resume: true,
                initial_backoff_seconds: 1,
                maximum_backoff_seconds: 2,
                maximum_rate_limited_seconds: 3,
                honor_provider_retry_after: true,
            },
            autonomy_lease: AutonomyLeaseV1 {
                start_approval_required: true,
                renewal_allowed: true,
                issued_at_unix: 1,
                expires_at_unix: 100,
                maximum_total_elapsed_seconds: 99,
                maximum_provider_turns: 2,
                maximum_tool_calls: 2,
                maximum_consecutive_failures: 1,
                maximum_repair_cycles: 1,
            },
            hard_stop_conditions: vec![AutonomousHardStopV1::CancellationRequested],
            completion_artifact_ids: Vec::new(),
            task_graph: Vec::new(),
        }
    }

    #[test]
    fn contract_create_persists_explicit_routine_provider_route_without_changing_default() {
        let root =
            std::env::temp_dir().join(format!("catdesk-autonomy-routine-route-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");

        let mut routine = contract(&root);
        routine.contract_id = "session-routine".into();
        routine.mode = "chatgpt_web_qwen_autonomous".into();
        handle_tool(
            "autonomy_contract_create",
            json!({"contract":routine}),
            &root,
        )
        .expect("routine contract");

        let mut primary = contract(&root);
        primary.contract_id = "session-primary".into();
        handle_tool(
            "autonomy_contract_create",
            json!({"contract":primary}),
            &root,
        )
        .expect("primary contract");

        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        assert_eq!(
            store
                .load_session("session-routine")
                .expect("routine session")
                .provider_route,
            AutonomousProviderRouteV1::QwenFallbackActive
        );
        assert_eq!(
            store
                .load_session("session-primary")
                .expect("primary session")
                .provider_route,
            AutonomousProviderRouteV1::CodexPreferred
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn approval_is_idempotent_and_start_requires_operator_runtime() {
        let root = std::env::temp_dir().join(format!("catdesk-autonomy-mcp-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        let create = handle_tool(
            "autonomy_contract_create",
            json!({"contract":contract(&root)}),
            &root,
        )
        .expect("create");
        let hash = create
            .get("contractHash")
            .and_then(Value::as_str)
            .expect("hash")
            .to_string();
        let approval=handle_tool("autonomy_contract_approve",json!({"sessionId":"session-1","expectedStateVersion":1,"idempotencyKey":"approve-1","approvalId":"approval-1","decisionHash":hash}),&root).expect("approve");
        assert_eq!(
            approval.get("state").and_then(Value::as_str),
            Some("QUEUED")
        );
        let replay = handle_tool(
            "autonomy_contract_approve",
            json!({"sessionId":"session-1","expectedStateVersion":1,"idempotencyKey":"approve-1","approvalId":"approval-1","decisionHash":hash}),
            &root,
        )
        .expect("idempotent replay");
        assert_eq!(replay.get("state").and_then(Value::as_str), Some("QUEUED"));
        let start = handle_tool(
            "autonomy_session_start",
            json!({"sessionId":"session-1","expectedStateVersion":2,"idempotencyKey":"start-1"}),
            &root,
        )
        .expect("start");
        assert_eq!(start.get("state").and_then(Value::as_str), Some("QUEUED"));
        let list = handle_tool("autonomy_session_list", json!({}), &root).expect("list");
        assert_eq!(
            list.get("sessions")
                .and_then(Value::as_array)
                .expect("sessions")
                .len(),
            1
        );
        let accounting = handle_tool(
            "autonomy_execution_accounting",
            json!({"sessionId":"session-1"}),
            &root,
        )
        .expect("accounting");
        assert_eq!(
            accounting
                .get("aggregate")
                .and_then(|aggregate| aggregate.get("providerTurns"))
                .and_then(Value::as_u64),
            Some(0)
        );
    }

    #[test]
    fn runtime_capabilities_are_read_only_and_compatibility_query_is_fail_closed() {
        let root = std::env::temp_dir().join(format!("catdesk-capabilities-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        assert!(!root.join(".catdesk").exists());

        let first_class = handle_tool("autonomy_runtime_capabilities", json!({}), &root)
            .expect("first-class capabilities");
        let compatibility = handle_tool(
            "autonomy_contract_validate",
            json!({"action":"RESULT"}),
            &root,
        )
        .expect("compatibility capabilities");
        assert_eq!(first_class, runtime_capability_manifest());
        assert_eq!(compatibility, first_class);
        assert!(!root.join(".catdesk").exists(), "query must not open state");
        assert!(
            handle_tool(
                "autonomy_runtime_capabilities",
                json!({"sessionId":"session-1"}),
                &root,
            )
            .is_err()
        );
        assert!(
            handle_tool(
                "autonomy_contract_validate",
                json!({"action":"RESULT","sessionId":"session-1"}),
                &root,
            )
            .is_err()
        );
        assert!(
            handle_tool(
                "autonomy_contract_validate",
                json!({"action":"RESULT","contract":contract(&root)}),
                &root,
            )
            .is_err()
        );
        assert!(
            handle_tool(
                "autonomy_contract_validate",
                json!({"action":"PREFLIGHT"}),
                &root,
            )
            .is_err()
        );

        handle_tool(
            "autonomy_contract_create",
            json!({"contract":contract(&root)}),
            &root,
        )
        .expect("legacy create");
        let validated = handle_tool(
            "autonomy_contract_validate",
            json!({"sessionId":"session-1"}),
            &root,
        )
        .expect("legacy validation remains unchanged");
        assert_eq!(validated.get("valid").and_then(Value::as_bool), Some(true));

        let schema = tool_schemas()
            .into_iter()
            .find(|schema| {
                schema.get("name").and_then(Value::as_str) == Some("autonomy_runtime_capabilities")
            })
            .expect("capability schema");
        assert_eq!(
            schema
                .get("annotations")
                .and_then(|value| value.get("readOnlyHint"))
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            schema
                .get("inputSchema")
                .and_then(|value| value.get("maxProperties"))
                .and_then(Value::as_u64),
            Some(0)
        );
    }

    #[test]
    fn start_and_resume_share_interrupted_worker_reconciliation_without_launching() {
        use super::super::autonomy_accounting::{ActivityActorV1, ActivityEvidenceV1};

        let root = std::env::temp_dir().join(format!("catdesk-restart-mcp-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        let create = handle_tool(
            "autonomy_contract_create",
            json!({"contract":contract(&root)}),
            &root,
        )
        .expect("create");
        let hash = create
            .get("contractHash")
            .and_then(Value::as_str)
            .expect("hash")
            .to_string();
        handle_tool(
            "autonomy_contract_approve",
            json!({"sessionId":"session-1","expectedStateVersion":1,"idempotencyKey":"approve","approvalId":"approval","decisionHash":hash}),
            &root,
        )
        .expect("approve");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let mut snapshot = store.load_session("session-1").expect("snapshot");
        snapshot.current_task_id = Some("work".into());
        snapshot.provider_thread_id = Some("canonical-thread".into());
        snapshot.expected_codex_thread_id = Some("canonical-thread".into());
        snapshot.provider_handle_id = Some("interrupted-handle".into());
        snapshot.provider_turn_count = 1;
        store.save_session(&snapshot).expect("state");
        let mut queue = store.load_queue("session-1").expect("queue");
        queue.tasks[0].state = AutonomousQueueTaskStateV1::WorkerRunning;
        store.save_queue("session-1", &queue).expect("queue");
        let accounting = store.execution_accounting_store().expect("accounting");
        accounting
            .record_task_started(
                "project",
                "work",
                "session-1",
                Some("canonical-thread"),
                "codex-cli",
                Some("model"),
                None,
                1,
            )
            .expect("record");
        accounting
            .activity_started(
                "session-1",
                ActivityActorV1::CodexProviderActive,
                ActivityEvidenceV1::ProviderLifecycle,
                2,
            )
            .expect("span");
        accounting
            .interrupt_open_spans("session-1")
            .expect("interrupted");

        let expected = store
            .load_session("session-1")
            .expect("state")
            .last_event_sequence;
        let start = handle_tool(
            "autonomy_session_start",
            json!({"sessionId":"session-1","expectedStateVersion":expected,"idempotencyKey":"restart-start"}),
            &root,
        )
        .expect("start reconciles");
        assert_eq!(
            start.get("restartReconciliation").and_then(Value::as_str),
            Some("RECONCILED_READY")
        );
        assert_eq!(
            store
                .load_session("session-1")
                .expect("state")
                .provider_turn_count,
            1,
            "supervisor reconciliation never starts a provider"
        );

        let mut snapshot = store.load_session("session-1").expect("state");
        snapshot.state = AutonomousSessionStateV1::Paused;
        store.save_session(&snapshot).expect("pause fixture");
        let mut queue = store.load_queue("session-1").expect("queue");
        queue.tasks[0].state = AutonomousQueueTaskStateV1::WorkerRunning;
        store
            .save_queue("session-1", &queue)
            .expect("queue fixture");
        let expected = store
            .load_session("session-1")
            .expect("state")
            .last_event_sequence;
        let resume = handle_tool(
            "autonomy_session_resume",
            json!({"sessionId":"session-1","expectedStateVersion":expected,"idempotencyKey":"restart-resume"}),
            &root,
        )
        .expect("resume reconciles");
        assert_eq!(
            resume.get("restartReconciliation").and_then(Value::as_str),
            Some("RECONCILED_READY")
        );
        assert_eq!(
            store
                .load_session("session-1")
                .expect("state")
                .provider_turn_count,
            1
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resume_preserves_paused_direct_chatgpt_ownership_for_finalization() {
        let root =
            std::env::temp_dir().join(format!("catdesk-direct-resume-mcp-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        let create = handle_tool(
            "autonomy_contract_create",
            json!({"contract":contract(&root)}),
            &root,
        )
        .expect("create");
        let hash = create
            .get("contractHash")
            .and_then(Value::as_str)
            .expect("hash")
            .to_string();
        handle_tool(
            "autonomy_contract_approve",
            json!({"sessionId":"session-1","expectedStateVersion":1,"idempotencyKey":"approve-direct-resume","approvalId":"approval","decisionHash":hash}),
            &root,
        )
        .expect("approve");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let mut snapshot = store.load_session("session-1").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Paused;
        snapshot.current_task_id = Some("work".into());
        snapshot.provider_thread_id = None;
        snapshot.provider_handle_id = None;
        snapshot.provider_route = AutonomousProviderRouteV1::WaitingForChatgpt;
        snapshot.provider_turn_count = 1;
        store.save_session(&snapshot).expect("direct pause fixture");
        let mut queue = store.load_queue("session-1").expect("queue");
        queue.tasks[0].state = AutonomousQueueTaskStateV1::WorkerRunning;
        store
            .save_queue("session-1", &queue)
            .expect("queue fixture");

        let expected = store
            .load_session("session-1")
            .expect("state")
            .last_event_sequence;
        let resumed = handle_tool(
            "autonomy_session_resume",
            json!({"sessionId":"session-1","expectedStateVersion":expected,"idempotencyKey":"direct-resume"}),
            &root,
        )
        .expect("direct ChatGPT resume");
        assert_eq!(
            resumed.get("state").and_then(Value::as_str),
            Some("WAITING_FOR_CHATGPT")
        );
        assert!(resumed.get("restartReconciliation").is_none());
        let resumed_snapshot = store.load_session("session-1").expect("resumed state");
        assert_eq!(resumed_snapshot.current_task_id.as_deref(), Some("work"));
        assert_eq!(resumed_snapshot.provider_turn_count, 1);
        assert_eq!(
            resumed_snapshot.provider_route,
            AutonomousProviderRouteV1::WaitingForChatgpt
        );
        assert!(resumed_snapshot.provider_thread_id.is_none());
        assert!(resumed_snapshot.provider_handle_id.is_none());
        assert_eq!(
            store.load_queue("session-1").expect("queue").tasks[0].state,
            AutonomousQueueTaskStateV1::WorkerRunning
        );

        handle_tool(
            "autonomy_session_finalize_direct_work",
            json!({
                "sessionId":"session-1",
                "expectedStateVersion":resumed_snapshot.last_event_sequence,
                "idempotencyKey":"direct-resume-finalize"
            }),
            &root,
        )
        .expect("direct finalization remains available after resume");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resume_refuses_inconsistent_paused_direct_chatgpt_queue_without_mutation() {
        let root =
            std::env::temp_dir().join(format!("catdesk-direct-resume-refusal-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        let create = handle_tool(
            "autonomy_contract_create",
            json!({"contract":contract(&root)}),
            &root,
        )
        .expect("create");
        let hash = create
            .get("contractHash")
            .and_then(Value::as_str)
            .expect("hash")
            .to_string();
        handle_tool(
            "autonomy_contract_approve",
            json!({"sessionId":"session-1","expectedStateVersion":1,"idempotencyKey":"approve-direct-resume-refusal","approvalId":"approval","decisionHash":hash}),
            &root,
        )
        .expect("approve");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let mut snapshot = store.load_session("session-1").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Paused;
        snapshot.current_task_id = Some("work".into());
        snapshot.provider_thread_id = None;
        snapshot.provider_handle_id = None;
        snapshot.provider_route = AutonomousProviderRouteV1::WaitingForChatgpt;
        snapshot.provider_turn_count = 1;
        store.save_session(&snapshot).expect("direct pause fixture");

        let before = store.load_session("session-1").expect("before");
        let queue_before = store.load_queue("session-1").expect("queue before");
        assert_eq!(
            queue_before.tasks[0].state,
            AutonomousQueueTaskStateV1::Ready
        );
        let result = handle_tool(
            "autonomy_session_resume",
            json!({
                "sessionId":"session-1",
                "expectedStateVersion":before.last_event_sequence,
                "idempotencyKey":"direct-resume-refusal"
            }),
            &root,
        );
        assert_eq!(
            result.expect_err("inconsistent direct ownership must refuse resume"),
            "paused direct ChatGPT ownership queue evidence is inconsistent"
        );
        let after = store.load_session("session-1").expect("after");
        assert_eq!(after.state, AutonomousSessionStateV1::Paused);
        assert_eq!(after.last_event_sequence, before.last_event_sequence);
        assert_eq!(
            store.load_queue("session-1").expect("queue after").tasks[0].state,
            AutonomousQueueTaskStateV1::Ready
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reviewed_promotion_dag_authority_never_falls_back_to_legacy_outputs() {
        let root = std::env::temp_dir().join(format!("catdesk-promotion-dag-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        let mut approved = contract(&root);
        approved.task_id = "legacy".into();
        approved.completion_artifact_ids = vec!["src/legacy.rs".into()];
        approved.task_graph = vec![super::super::autonomous_contract::AutonomousTaskSpecV1 {
            task_id: "reviewed".into(),
            priority: 1,
            depends_on: Vec::new(),
            acceptance_criteria: vec!["done".into()],
            planner_gate: None,
            completion_artifact_ids: vec!["src/reviewed.rs".into()],
        }];
        assert_eq!(
            reviewed_promotion_completion_artifact_ids(&approved, "reviewed"),
            Ok(vec!["src/reviewed.rs".into()])
        );
        assert!(reviewed_promotion_completion_artifact_ids(&approved, "legacy").is_err());
        assert!(reviewed_promotion_completion_artifact_ids(&approved, "missing").is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reviewed_promotion_authority_revalidates_completion_output_evidence() {
        use crate::core_host_gate_approval::{
            CORE_HOST_GATE_APPROVAL_PRODUCT, CORE_HOST_GATE_APPROVAL_PROJECT,
            CORE_HOST_GATE_APPROVAL_SCHEMA_VERSION, CoreHostGateRuntimeIdentityV1, CoreHostGateV1,
            canonical_review_artifact_bytes, review_artifact_for_request,
        };
        use crate::delegated::coordinator::{VerificationStatusV1, VerificationSummaryV1};

        let root =
            std::env::temp_dir().join(format!("catdesk-promotion-review-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        let mut approved = contract(&root);
        approved.project_id = "catdesk".into();
        approved.completion_artifact_ids = vec![
            "src/result.txt".into(),
            CORE_HOST_GATE_APPROVAL_REVIEW_ARTIFACT_ID.into(),
        ];
        let gate_request = CoreHostGateApprovalRequestV1 {
            schema_version: CORE_HOST_GATE_APPROVAL_SCHEMA_VERSION,
            product: CORE_HOST_GATE_APPROVAL_PRODUCT.into(),
            project_id: CORE_HOST_GATE_APPROVAL_PROJECT.into(),
            purpose: CORE_HOST_GATE_APPROVAL_PURPOSE.into(),
            gate: CoreHostGateV1::T0223,
            observation_id: "observation-one".into(),
            observation_sha256: "a".repeat(64),
            t0224_session_id: "wake-session".into(),
            t0224_review_record_id: "wake-record".into(),
            current_target_sha256: "b".repeat(64),
            runtime: CoreHostGateRuntimeIdentityV1 {
                build_identity: "build-one".into(),
                generation: 1,
                host_session_id: "host-session".into(),
            },
            issued_at_unix: 1,
            revision: 1,
        };
        let gate_artifact = canonical_review_artifact_bytes(
            &review_artifact_for_request(gate_request.clone()).expect("artifact"),
        )
        .expect("artifact bytes");
        handle_tool(
            "autonomy_contract_create",
            json!({"contract":approved.clone()}),
            &root,
        )
        .expect("contract");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let contract_hash = approved.decision_hash().expect("contract hash");
        let mut snapshot = store.load_session("session-1").expect("session");
        snapshot.active = false;
        snapshot.state = AutonomousSessionStateV1::CompletedVerified;
        snapshot.current_task_id = Some("work".into());
        snapshot.approved_contract_hash = Some(contract_hash.clone());
        store.save_session(&snapshot).expect("completed session");
        store
            .write_completion_artifacts(
                "session-1",
                &VerificationSummaryV1 {
                    status: VerificationStatusV1::Passed,
                    command: "cargo test".into(),
                    summary: "passed".into(),
                },
                "bounded diff",
                "bounded final review",
            )
            .expect("completion");
        store
            .save_task_output_baseline(
                "session-1",
                &super::super::autonomy_state::AutonomousTaskOutputBaselineV1 {
                    schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
                    session_id: "session-1".into(),
                    task_id: "work".into(),
                    approved_contract_hash: contract_hash,
                    completion_artifact_ids: approved.completion_artifact_ids.clone(),
                    observations: approved
                        .completion_artifact_ids
                        .iter()
                        .map(|artifact_id| AutonomousTaskOutputObservationV1 {
                            artifact_id: artifact_id.clone(),
                            state: "ABSENT".into(),
                            sha256: None,
                        })
                        .collect(),
                },
            )
            .expect("baseline");
        std::fs::write(root.join("src/result.txt"), "first attributed output").expect("output");
        std::fs::write(
            root.join(CORE_HOST_GATE_APPROVAL_REVIEW_ARTIFACT_ID),
            &gate_artifact,
        )
        .expect("typed artifact");
        let record = store
            .emit_review_inbox_record(
                "session-1",
                "catdesk",
                AutonomousSessionStateV1::CompletedVerified,
                "independent_final_review",
                "artifacts/completion.json",
                1,
            )
            .expect("record");
        let supervisor = AutonomousSupervisorV1::open(&root).expect("supervisor");
        assert!(
            supervisor
                .resolve_core_host_gate_review_authority(&record.record_id, &gate_request)
                .is_err(),
            "an unread ordinary final-review record is workflow state, not host approval authority"
        );
        store
            .acknowledge_review_inbox(&record.record_id)
            .expect("acknowledge");
        let first = supervisor
            .resolve_reviewed_promotion_review_authority(&record.record_id)
            .expect("authority");
        let mut changed_request = gate_request.clone();
        changed_request.gate = CoreHostGateV1::T0222;
        assert!(
            supervisor
                .resolve_core_host_gate_review_authority(&record.record_id, &changed_request)
                .is_err(),
            "a generic acknowledged completion cannot authorize a different host request"
        );
        let artifact_path = root.join(CORE_HOST_GATE_APPROVAL_REVIEW_ARTIFACT_ID);
        std::fs::remove_file(&artifact_path).expect("remove typed artifact");
        assert!(
            supervisor
                .resolve_core_host_gate_review_authority(&record.record_id, &gate_request)
                .is_err(),
            "the exact typed artifact must remain a current reviewed completion output"
        );
        std::fs::write(&artifact_path, &gate_artifact).expect("restore typed artifact");
        let host_first = supervisor
            .resolve_core_host_gate_review_authority(&record.record_id, &gate_request)
            .expect("purpose-separated host authority");
        assert_eq!(host_first.purpose, CORE_HOST_GATE_APPROVAL_PURPOSE);
        assert_ne!(host_first.authority_sha256, first.digest);
        std::fs::write(root.join("src/result.txt"), "changed attributed output").expect("drift");
        let second = supervisor
            .resolve_reviewed_promotion_review_authority(&record.record_id)
            .expect("authority after output change");
        assert_ne!(
            first.digest, second.digest,
            "current output hash is authority evidence"
        );
        let host_second = supervisor
            .resolve_core_host_gate_review_authority(&record.record_id, &gate_request)
            .expect("host authority after output change");
        assert_ne!(host_first.authority_sha256, host_second.authority_sha256);
        std::fs::remove_file(root.join("src/result.txt")).expect("remove output");
        assert!(
            supervisor
                .resolve_reviewed_promotion_review_authority(&record.record_id)
                .is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn daemon_reload_review_authority_requires_exact_current_candidate_artifact() {
        use crate::daemon_reload_approval::{
            DAEMON_RELOAD_APPROVAL_PRODUCT, DAEMON_RELOAD_APPROVAL_PROJECT,
            DAEMON_RELOAD_APPROVAL_PURPOSE, DAEMON_RELOAD_APPROVAL_REVIEW_ARTIFACT_ID,
            DAEMON_RELOAD_APPROVAL_SCHEMA_VERSION, DaemonReloadApprovalRequestV1,
            canonical_review_artifact_bytes as canonical_reload_review_artifact_bytes,
            review_artifact_for_request as reload_review_artifact_for_request,
        };
        use crate::delegated::coordinator::{VerificationStatusV1, VerificationSummaryV1};

        let root = std::env::temp_dir().join(format!("catdesk-reload-review-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        let mut approved = contract(&root);
        approved.project_id = "catdesk".into();
        approved.completion_artifact_ids = vec![
            "src/result.txt".into(),
            DAEMON_RELOAD_APPROVAL_REVIEW_ARTIFACT_ID.into(),
        ];
        let request_a = DaemonReloadApprovalRequestV1 {
            schema_version: DAEMON_RELOAD_APPROVAL_SCHEMA_VERSION,
            product: DAEMON_RELOAD_APPROVAL_PRODUCT.into(),
            project_id: DAEMON_RELOAD_APPROVAL_PROJECT.into(),
            purpose: DAEMON_RELOAD_APPROVAL_PURPOSE.into(),
            candidate_relative_path: r"target-verify\candidate-a\catdesk.exe".into(),
            candidate_sha256: "a".repeat(64),
            candidate_length: 123,
        };
        let artifact_a = canonical_reload_review_artifact_bytes(
            &reload_review_artifact_for_request(request_a.clone()).expect("artifact"),
        )
        .expect("artifact bytes");
        handle_tool(
            "autonomy_contract_create",
            json!({"contract":approved.clone()}),
            &root,
        )
        .expect("contract");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let contract_hash = approved.decision_hash().expect("contract hash");
        let mut snapshot = store.load_session("session-1").expect("session");
        snapshot.active = false;
        snapshot.state = AutonomousSessionStateV1::CompletedVerified;
        snapshot.current_task_id = Some("work".into());
        snapshot.approved_contract_hash = Some(contract_hash.clone());
        store.save_session(&snapshot).expect("completed session");
        store
            .write_completion_artifacts(
                "session-1",
                &VerificationSummaryV1 {
                    status: VerificationStatusV1::Passed,
                    command: "cargo test".into(),
                    summary: "passed".into(),
                },
                "bounded diff",
                "bounded final review",
            )
            .expect("completion");
        store
            .save_task_output_baseline(
                "session-1",
                &super::super::autonomy_state::AutonomousTaskOutputBaselineV1 {
                    schema_version: AUTONOMY_STATE_SCHEMA_VERSION,
                    session_id: "session-1".into(),
                    task_id: "work".into(),
                    approved_contract_hash: contract_hash,
                    completion_artifact_ids: approved.completion_artifact_ids.clone(),
                    observations: approved
                        .completion_artifact_ids
                        .iter()
                        .map(|artifact_id| AutonomousTaskOutputObservationV1 {
                            artifact_id: artifact_id.clone(),
                            state: "ABSENT".into(),
                            sha256: None,
                        })
                        .collect(),
                },
            )
            .expect("baseline");
        std::fs::write(root.join("src/result.txt"), "reload review output").expect("output");
        std::fs::write(
            root.join(DAEMON_RELOAD_APPROVAL_REVIEW_ARTIFACT_ID),
            &artifact_a,
        )
        .expect("typed artifact");
        let record = store
            .emit_review_inbox_record(
                "session-1",
                "catdesk",
                AutonomousSessionStateV1::CompletedVerified,
                "independent_final_review",
                "artifacts/completion.json",
                1,
            )
            .expect("record");
        let supervisor = AutonomousSupervisorV1::open(&root).expect("supervisor");
        assert!(
            supervisor
                .resolve_daemon_reload_review_authority(&record.record_id, &request_a)
                .is_err(),
            "unread review must not authorize daemon reload"
        );
        store
            .acknowledge_review_inbox(&record.record_id)
            .expect("acknowledge");
        let authority_a = supervisor
            .resolve_daemon_reload_review_authority(&record.record_id, &request_a)
            .expect("candidate A authority");
        assert_eq!(authority_a.purpose, DAEMON_RELOAD_APPROVAL_PURPOSE);

        let mut request_b = request_a.clone();
        request_b.candidate_relative_path = r"target-verify\candidate-b\catdesk.exe".into();
        assert!(
            supervisor
                .resolve_daemon_reload_review_authority(&record.record_id, &request_b)
                .is_err(),
            "a valid acknowledged review for candidate A must not authorize candidate B"
        );
        request_b = request_a.clone();
        request_b.candidate_sha256 = "b".repeat(64);
        assert!(
            supervisor
                .resolve_daemon_reload_review_authority(&record.record_id, &request_b)
                .is_err(),
            "a valid acknowledged review for one hash must not authorize another"
        );
        std::fs::remove_file(root.join(DAEMON_RELOAD_APPROVAL_REVIEW_ARTIFACT_ID))
            .expect("remove typed artifact");
        assert!(
            supervisor
                .resolve_daemon_reload_review_authority(&record.record_id, &request_a)
                .is_err(),
            "the exact typed artifact must remain a current reviewed completion output"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn project_registry_binding_is_canonical_evidence_gated_and_durable() {
        let root = std::env::temp_dir().join(format!("catdesk-project-bind-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("workspace");
        assert!(
            handle_tool(
                "autonomy_project_registry_bind",
                json!({
                    "projectId":"catdesk",
                    "threadId":"thread-canonical-1",
                    "gitIdentity":"https://example.test/catdesk.git",
                    "verificationProfile":"rust_full",
                    "resolutionEvidence":"title-only"
                }),
                &root,
            )
            .is_err()
        );
        let bound = handle_tool(
            "autonomy_project_registry_bind",
            json!({
                "projectId":"catdesk",
                "threadId":"thread-canonical-1",
                "gitIdentity":"https://example.test/catdesk.git",
                "verificationProfile":"rust_full",
                "resolutionEvidence":"exact-unowned-direct-input"
            }),
            &root,
        )
        .expect("bind");
        assert_eq!(
            bound.get("codexThreadId").and_then(Value::as_str),
            Some("thread-canonical-1")
        );
        let registry =
            handle_tool("autonomy_project_registry_read", json!({}), &root).expect("registry read");
        let projects = registry
            .get("projects")
            .and_then(Value::as_array)
            .expect("projects");
        let canonical_root = root
            .canonicalize()
            .expect("canonical root")
            .to_string_lossy()
            .into_owned();
        assert_eq!(projects.len(), 1);
        assert_eq!(
            projects[0].get("workspace").and_then(Value::as_str),
            Some(canonical_root.as_str())
        );
    }

    #[test]
    fn exposed_registry_bind_supports_only_exclusive_target_cas_mode() {
        let root = std::env::temp_dir().join(format!("catdesk-project-target-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("workspace");
        let legacy = json!({"projectId":"catdesk","threadId":"thread-canonical-1","gitIdentity":"https://example.test/catdesk.git","verificationProfile":"rust_full","resolutionEvidence":"exact-unowned-direct-input"});
        let bound =
            handle_tool("autonomy_project_registry_bind", legacy, &root).expect("legacy bind");
        assert_eq!(
            bound.get("codexThreadId").and_then(Value::as_str),
            Some("thread-canonical-1")
        );
        let target = handle_tool("autonomy_project_registry_bind", json!({
            "projectId":"catdesk", "conversationUrl":"https://chatgpt.com/g/project-1/c/conversation-1"
        }), &root).expect("target CAS");
        let digest = target
            .get("targetSha256")
            .and_then(Value::as_str)
            .expect("digest")
            .to_owned();
        assert_eq!(target.get("chatgptTargetUrl"), None);
        assert!(handle_tool("autonomy_project_registry_bind", json!({
            "projectId":"catdesk", "conversationUrl":"https://chatgpt.com/c/other", "expectedCurrentTargetSha256":"0".repeat(64)
        }), &root).is_err());
        assert!(handle_tool("autonomy_project_registry_bind", json!({
            "projectId":"catdesk", "conversationUrl":"https://chatgpt.com/c/other", "expectedCurrentTargetSha256":digest, "threadId":"thread-canonical-1"
        }), &root).is_err());
        for invalid in [
            json!({"projectId":"catdesk", "expectedCurrentTargetSha256":"0".repeat(64)}),
            json!({"projectId":"catdesk", "conversationUrl":"https://chatgpt.com/c/other?query=1"}),
            json!({"projectId":"missing", "conversationUrl":"https://chatgpt.com/c/other"}),
            json!({"projectId":"catdesk", "conversationUrl":"https://chatgpt.com/c/other", "workspace":"elsewhere"}),
            json!({"projectId":"catdesk", "conversationUrl":"https://user@chatgpt.com/c/other"}),
        ] {
            assert!(handle_tool("autonomy_project_registry_bind", invalid, &root).is_err());
        }
        let registry =
            handle_tool("autonomy_project_registry_read", json!({}), &root).expect("registry");
        let project = &registry["projects"][0];
        assert_eq!(project["codexThreadId"], "thread-canonical-1");
        assert_eq!(project["chatgptTargetSha256"], digest);
        let schema = tool_schemas()
            .into_iter()
            .find(|schema| schema["name"] == "autonomy_project_registry_bind")
            .expect("schema");
        let alternatives = schema["inputSchema"]["oneOf"]
            .as_array()
            .expect("closed-world registry-bind alternatives");
        assert_eq!(alternatives.len(), 13);
        let paired_target = alternatives
            .iter()
            .find(|alternative| {
                alternative["properties"]["decision"]["pattern"]
                    == "^DESIGNATED_CHAT_TARGET_URL=\\S+$"
            })
            .expect("paired designated-target CAS alternative");
        assert_eq!(
            paired_target["required"],
            json!(["projectId", "decision", "expectedSha256"])
        );
    }

    #[test]
    fn target_binding_cannot_mutate_a_registered_sibling_project() {
        let root = std::env::temp_dir().join(format!("catdesk-project-target-{}", Uuid::new_v4()));
        let sibling = root
            .parent()
            .expect("temp parent")
            .join(format!("catdesk-project-sibling-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("workspace");
        std::fs::create_dir_all(&sibling).expect("sibling workspace");
        handle_tool(
            "autonomy_project_registry_bind",
            json!({
                "projectId":"catdesk",
                "threadId":"thread-canonical-1",
                "gitIdentity":"https://example.test/catdesk.git",
                "verificationProfile":"rust_full",
                "resolutionEvidence":"exact-unowned-direct-input"
            }),
            &root,
        )
        .expect("root registration");
        let store = AutonomousProjectRegistryStoreV1::open(root.join(".catdesk").join("projects"))
            .expect("registry");
        store
            .register_project(AutonomousProjectV1 {
                project_id: "sibling".into(),
                workspace: sibling.canonicalize().expect("canonical sibling"),
                git_identity: "https://example.test/sibling.git".into(),
                verification_profile: "rust_full".into(),
                codex_thread_id: Some("thread-sibling-1".into()),
                chatgpt_target_url: None,
                chatgpt_target_sha256: None,
            })
            .expect("sibling registration");

        assert!(
            handle_tool(
                "autonomy_project_registry_chat_target_bind",
                json!({
                    "projectId":"sibling",
                    "conversationUrl":"https://chatgpt.com/c/sibling-target"
                }),
                &root,
            )
            .is_err(),
            "the root-bound supervisor must not mutate a sibling target"
        );
        let registry = store.load_registry().expect("registry readback");
        let sibling_project = registry
            .projects
            .iter()
            .find(|project| project.project_id == "sibling")
            .expect("sibling record");
        assert!(sibling_project.chatgpt_target_url.is_none());
        assert!(sibling_project.chatgpt_target_sha256.is_none());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(sibling);
    }

    #[test]
    fn cached_connector_target_cas_requires_exact_fixed_decision_shape() {
        let root = std::env::temp_dir().join(format!("catdesk-cached-target-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("workspace");
        handle_tool("autonomy_project_registry_bind", json!({"projectId":"catdesk","threadId":"thread-cached-1","gitIdentity":"https://example.test/catdesk.git","verificationProfile":"rust_full","resolutionEvidence":"exact-unowned-direct-input"}), &root).expect("legacy");
        let initial = handle_tool(
            "autonomy_project_registry_bind",
            json!({"projectId":"catdesk","conversationUrl":"https://chatgpt.com/c/initial"}),
            &root,
        )
        .expect("initial target");
        let digest = initial["targetSha256"].as_str().expect("digest").to_owned();
        let cached = handle_tool("autonomy_project_registry_bind", json!({
            "projectId":"catdesk", "decision":"CHAT_TARGET_URL=https://chatgpt.com/g/project-1/c/current", "expectedSha256":digest
        }), &root).expect("cached target");
        assert!(cached["targetSha256"].as_str().is_some());
        for invalid in [
            json!({"projectId":"catdesk","decision":"CHAT_TARGET_URL=https://chatgpt.com/c/current ","expectedSha256":cached["targetSha256"]}),
            json!({"projectId":"catdesk","decision":"xCHAT_TARGET_URL=https://chatgpt.com/c/current","expectedSha256":cached["targetSha256"]}),
            json!({"projectId":"catdesk","decision":"CHAT_TARGET_URL=https://chatgpt.com/c/current extra","expectedSha256":cached["targetSha256"]}),
            json!({"projectId":"catdesk","decision":"CHAT_TARGET_URL=https://chatgpt.com/c/current?query=1","expectedSha256":cached["targetSha256"]}),
            json!({"projectId":"catdesk","decision":"CHAT_TARGET_URL=https://chatgpt.com/c/current","expectedSha256":"0".repeat(64)}),
            json!({"projectId":"catdesk","decision":"CHAT_TARGET_URL=https://chatgpt.com/c/current","expectedSha256":cached["targetSha256"],"threadId":"thread-cached-1"}),
            json!({"projectId":"catdesk","decision":"CHAT_TARGET_URL=https://chatgpt.com/c/current","expectedSha256":cached["targetSha256"],"conversationUrl":"https://chatgpt.com/c/other"}),
        ] {
            assert!(handle_tool("autonomy_project_registry_bind", invalid, &root).is_err());
        }
        let registry =
            handle_tool("autonomy_project_registry_read", json!({}), &root).expect("registry");
        assert_eq!(registry["projects"][0]["codexThreadId"], "thread-cached-1");
        assert_eq!(
            registry["projects"][0]["chatgptTargetUrl"],
            "https://chatgpt.com/g/project-1/c/current"
        );
    }

    #[test]
    fn cached_designated_chat_target_cas_keeps_catdesk_registry_and_wake_target_coherent() {
        let root = std::env::temp_dir().join(format!(
            "catdesk-cached-designated-target-{}",
            Uuid::new_v4()
        ));
        let wake_root = root.join(".catdesk").join("wake-bridge");
        std::fs::create_dir_all(&wake_root).expect("wake root");
        let wake_config = wake_root.join("config.json");
        std::fs::write(
            &wake_config,
            serde_json::to_vec_pretty(&json!({
                "schema_version": 4,
                "conversation_url": "https://chatgpt.com/c/previous-thread",
                "profile_dir": ".catdesk/wake-bridge/browser-profile",
                "ui_ready_timeout_seconds": 20.0,
                "send_confirmation_timeout_seconds": 15.0,
                "ui_poll_interval_seconds": 0.25,
                "debounce_seconds": 1
            }))
            .expect("wake config json"),
        )
        .expect("wake config");

        handle_tool(
            "autonomy_project_registry_bind",
            json!({
                "projectId":"catdesk",
                "threadId":"thread-designated-1",
                "gitIdentity":"https://example.test/catdesk.git",
                "verificationProfile":"rust_full",
                "resolutionEvidence":"exact-unowned-direct-input"
            }),
            &root,
        )
        .expect("legacy project binding");
        let initial = handle_tool(
            "autonomy_project_registry_bind",
            json!({
                "projectId":"catdesk",
                "conversationUrl":"https://chatgpt.com/c/previous-thread"
            }),
            &root,
        )
        .expect("initial project target");
        let digest = initial["targetSha256"].as_str().expect("initial digest");

        let updated = handle_tool(
            "autonomy_project_registry_bind",
            json!({
                "projectId":"catdesk",
                "decision":"DESIGNATED_CHAT_TARGET_URL=https://chatgpt.com/c/current-thread",
                "expectedSha256":digest
            }),
            &root,
        )
        .expect("paired target update");
        assert_eq!(updated["targetUrl"], "https://chatgpt.com/c/current-thread");

        let registry = handle_tool("autonomy_project_registry_read", json!({}), &root)
            .expect("registry readback");
        assert_eq!(
            registry["projects"][0]["chatgptTargetUrl"],
            "https://chatgpt.com/c/current-thread"
        );
        let wake: Value = serde_json::from_slice(&std::fs::read(&wake_config).expect("wake bytes"))
            .expect("wake json");
        assert_eq!(
            wake["conversation_url"],
            "https://chatgpt.com/c/current-thread"
        );

        assert!(
            handle_tool(
                "autonomy_project_registry_bind",
                json!({
                    "projectId":"catdesk",
                    "decision":"DESIGNATED_CHAT_TARGET_URL=https://chatgpt.com/c/stale-write",
                    "expectedSha256":digest
                }),
                &root,
            )
            .is_err()
        );
        assert!(
            handle_tool(
                "autonomy_project_registry_bind",
                json!({
                    "projectId":"other-project",
                    "decision":"DESIGNATED_CHAT_TARGET_URL=https://chatgpt.com/c/other",
                    "expectedSha256":updated["targetSha256"]
                }),
                &root,
            )
            .is_err()
        );

        let registry_after = handle_tool("autonomy_project_registry_read", json!({}), &root)
            .expect("registry final readback");
        assert_eq!(
            registry_after["projects"][0]["chatgptTargetUrl"],
            "https://chatgpt.com/c/current-thread"
        );
        let wake_after: Value =
            serde_json::from_slice(&std::fs::read(&wake_config).expect("wake final bytes"))
                .expect("wake final json");
        assert_eq!(
            wake_after["conversation_url"],
            "https://chatgpt.com/c/current-thread"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cached_control_modes_are_closed_and_target_init_is_one_time() {
        let root = std::env::temp_dir().join(format!("catdesk-cached-control-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("workspace");
        handle_tool(
            "autonomy_project_registry_bind",
            json!({
                "projectId":"catdesk",
                "threadId":"thread-cached-control-1",
                "gitIdentity":"https://example.test/catdesk.git",
                "verificationProfile":"rust_full",
                "resolutionEvidence":"exact-unowned-direct-input"
            }),
            &root,
        )
        .expect("legacy bind");

        let initialized = handle_tool(
            "autonomy_project_registry_bind",
            json!({
                "projectId":"catdesk",
                "decision":"CHAT_TARGET_INIT_URL=https://chatgpt.com/g/project-1/c/initial"
            }),
            &root,
        )
        .expect("one-time target initialization");
        assert_eq!(initialized["projectId"], "catdesk");
        assert!(initialized["targetSha256"].as_str().is_some());

        for invalid in [
            json!({"projectId":"catdesk","decision":"CHAT_TARGET_INIT_URL=https://chatgpt.com/c/initial?query=1"}),
            json!({"projectId":"catdesk","decision":"CHAT_TARGET_INIT_URL=https://chatgpt.com/c/second"}),
            json!({"projectId":"catdesk","decision":" CHAT_TARGET_INIT_URL=https://chatgpt.com/c/second"}),
            json!({"projectId":"catdesk","decision":"CHAT_TARGET_INIT_URL=https://chatgpt.com/c/second","expectedSha256":"0".repeat(64)}),
            json!({"projectId":"catdesk","decision":"PROJECT_REGISTRATION_CONFIRM","confirmationToken":"token","threadId":"raw-thread"}),
            json!({"projectId":7,"decision":"PROJECT_REGISTRATION_CONFIRM","confirmationToken":"token"}),
            json!({"decision":"PROJECT_ORIGIN_WORKSPACE=C:\\not-a-repository","projectId":"catdesk"}),
            json!({"projectId":"catdesk","decision":"PROJECT_THREAD_ADOPTION_CONFIRM","confirmationToken":"token","candidateHandle":"opaque","threadId":"raw-thread"}),
            json!({"projectId":"catdesk","decision":"GITHUB_BOOTSTRAP_PREFLIGHT_WORKSPACE=C:\\Users\\Volap\\OneDrive\\Desktop\\Projects\\CatDesk-codex-loop","gitIdentity":"injected"}),
            json!({"projectId":"catdesk","decision":"GITHUB_BOOTSTRAP_CONFIRM","confirmationToken":"token","expectedSha256":"0".repeat(64)}),
            json!({"projectId":"BYOVD_DRIVER_PIPELINE","decision":" GITHUB_BOOTSTRAP_CONFIRM","confirmationToken":"token"}),
            json!({"projectId":"BYOVD_DRIVER_PIPELINE","decision":"GITHUB_BOOTSTRAP_RECOVER","confirmationToken":"token","workspace":"C:\\injected"}),
            json!({"projectId":"BYOVD_DRIVER_PIPELINE","decision":" GITHUB_BOOTSTRAP_RECOVER","confirmationToken":"token"}),
        ] {
            assert!(handle_tool("autonomy_project_registry_bind", invalid, &root).is_err());
        }
        let origin_rejection = handle_tool(
            "autonomy_project_registry_bind",
            json!({"decision":"PROJECT_ORIGIN_WORKSPACE=relative-path"}),
            &root,
        )
        .expect("read-only origin rejection");
        assert_eq!(origin_rejection["accepted"], false);
        assert_eq!(origin_rejection["reasonCode"], "WORKSPACE_NOT_ABSOLUTE");
        let catdesk_bootstrap = handle_tool(
            "autonomy_project_registry_bind",
            json!({"projectId":"catdesk","decision":"GITHUB_BOOTSTRAP_PREFLIGHT_WORKSPACE=C:\\Users\\Volap\\OneDrive\\Desktop\\Projects\\CatDesk-codex-loop"}),
            &root,
        );
        assert!(catdesk_bootstrap.is_err());
        let registry =
            handle_tool("autonomy_project_registry_read", json!({}), &root).expect("registry");
        assert_eq!(
            registry["projects"][0]["chatgptTargetUrl"],
            "https://chatgpt.com/g/project-1/c/initial"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn origin_probe_uses_exact_root_origin_and_never_touches_registry() {
        let root = std::env::temp_dir().join(format!("catdesk-origin-probe-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("nested")).expect("workspace");
        for args in [
            &["init"][..],
            &[
                "remote",
                "add",
                "origin",
                "https://example.test/external.git",
            ][..],
        ] {
            let status = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .expect("git available for bounded fixture");
            assert!(status.success());
        }
        let root_text = root
            .canonicalize()
            .expect("canonical root")
            .to_string_lossy()
            .into_owned();
        let result = project_origin_workspace_probe(&root_text).expect("exact root origin");
        assert_eq!(result.1, "https://example.test/external.git");
        assert!(
            verified_external_git_workspace(&root_text, "https://wrong.example/external.git")
                .is_err()
        );
        assert!(project_origin_workspace_probe(&root.join("nested").to_string_lossy()).is_err());
        assert!(project_origin_workspace_probe("relative-path").is_err());
        assert!(
            !root.join(".catdesk").exists(),
            "probe must not persist state"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn origin_probe_reason_codes_are_stable_and_non_secret() {
        let root = std::env::temp_dir().join(format!("catdesk-origin-reasons-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("nested")).expect("workspace");
        let root_text = root
            .canonicalize()
            .expect("canonical root")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            project_origin_workspace_probe(&root_text)
                .expect_err("plain directory is not Git")
                .reason_code(),
            "NOT_GIT_ROOT"
        );
        std::fs::create_dir(root.join(".git")).expect("corrupt Git marker fixture");
        assert_eq!(
            project_origin_workspace_probe(&root_text)
                .expect_err("corrupt Git marker command failure")
                .reason_code(),
            "GIT_COMMAND_FAILED"
        );
        std::fs::remove_dir(root.join(".git")).expect("remove corrupt Git marker");
        let status = Command::new("git")
            .arg("-C")
            .arg(&root)
            .arg("init")
            .status()
            .expect("git available for bounded fixture");
        assert!(status.success());
        assert_eq!(
            project_origin_workspace_probe(&root_text)
                .expect_err("origin is absent")
                .reason_code(),
            "ORIGIN_MISSING"
        );
        assert_eq!(
            project_origin_workspace_probe(&root.join("nested").to_string_lossy())
                .expect_err("nested root")
                .reason_code(),
            "NESTED_GIT_ROOT"
        );
        let response = handle_tool(
            "autonomy_project_registry_bind",
            json!({"decision":format!("PROJECT_ORIGIN_WORKSPACE={root_text}")}),
            &root,
        )
        .expect("read-only diagnostic response");
        assert_eq!(response["accepted"], false);
        assert_eq!(response["reasonCode"], "ORIGIN_MISSING");
        assert!(response.get("stderr").is_none());
        assert!(response.get("detail").is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_git_metadata_rejects_failure_oversize_and_non_utf8_without_output() {
        assert_eq!(
            validate_bounded_git_metadata(GitMetadataStage::TopLevel, false, b"ignored", 0)
                .expect_err("failed top-level command")
                .reason_code(),
            "GIT_COMMAND_FAILED"
        );
        assert_eq!(
            validate_bounded_git_metadata(GitMetadataStage::Origin, false, b"ignored", 0)
                .expect_err("missing origin")
                .reason_code(),
            "ORIGIN_MISSING"
        );
        assert_eq!(
            validate_bounded_git_metadata(GitMetadataStage::Origin, true, &vec![b'x'; 4_097], 0,)
                .expect_err("oversized stdout")
                .reason_code(),
            "GIT_METADATA_OVERSIZED"
        );
        assert_eq!(
            validate_bounded_git_metadata(GitMetadataStage::Origin, true, &[0xff], 0)
                .expect_err("non UTF-8")
                .reason_code(),
            "GIT_METADATA_NON_UTF8"
        );
        assert_eq!(
            ProjectOriginProbeFailure::GitExecutableUnavailable.reason_code(),
            "GIT_EXECUTABLE_UNAVAILABLE"
        );
    }

    #[test]
    fn reviewed_build_schema_is_closed_and_accepts_minimal_preflight() {
        let schema = tool_schemas()
            .into_iter()
            .find(|schema| schema["name"] == "catdesk_reviewed_build")
            .expect("reviewed build schema");
        let input = &schema["inputSchema"];
        assert_eq!(input["type"], "object");
        assert_eq!(input["oneOf"][0]["additionalProperties"], false);
        assert_eq!(input["oneOf"][0]["required"], json!(["action", "recordId"]));
        assert_eq!(
            input["oneOf"][0]["properties"]["action"]["enum"],
            json!(["PREPARE", "PREFLIGHT"])
        );
        assert_eq!(
            input["oneOf"][1]["required"],
            json!(["action", "confirmationToken"])
        );
        assert_eq!(input["oneOf"][2]["properties"]["action"]["const"], "RESULT");
    }

    #[test]
    fn github_publication_schema_is_closed_to_prepare_confirm_and_result() {
        assert!(AUTONOMY_MCP_TOOL_NAMES.contains(&"catdesk_github_publication"));
        let schema = tool_schemas()
            .into_iter()
            .find(|schema| schema["name"] == "catdesk_github_publication")
            .expect("publication schema");
        let input = &schema["inputSchema"];
        assert_eq!(
            input["oneOf"][0]["required"],
            json!(["action", "recordId", "approval"])
        );
        assert_eq!(input["oneOf"][0]["additionalProperties"], false);
        assert_eq!(
            input["oneOf"][1]["required"],
            json!(["action", "confirmationToken"])
        );
        assert_eq!(input["oneOf"][2]["properties"]["action"]["const"], "RESULT");
        assert!(input.to_string().contains("gpub-"));
        assert!(!input.to_string().contains("branch"));
        assert!(!input.to_string().contains("origin"));
    }

    #[test]
    fn daemon_reload_is_exposed_but_blocked_during_active_mutation() {
        assert!(AUTONOMY_MCP_TOOL_NAMES.contains(&"catdesk_daemon_reload"));
        let schema = tool_schemas()
            .into_iter()
            .find(|schema| {
                schema.get("name").and_then(Value::as_str) == Some("catdesk_daemon_reload")
            })
            .expect("reload schema");
        assert_eq!(
            schema
                .get("annotations")
                .and_then(|annotations| annotations.get("destructiveHint"))
                .and_then(Value::as_bool),
            Some(true)
        );

        let root = std::env::temp_dir().join(format!("catdesk-reload-mcp-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        handle_tool(
            "autonomy_contract_create",
            json!({"contract":contract(&root)}),
            &root,
        )
        .expect("create");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let mut snapshot = store.load_session("session-1").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Running;
        snapshot.active = true;
        store.save_session(&snapshot).expect("running state");
        let error = handle_tool(
            "catdesk_daemon_reload",
            json!({
                "buildPath":"missing.exe",
                "expectedSha256":"0".repeat(64),
                "dryRun":true
            }),
            &root,
        )
        .expect_err("active mutation must block reload");
        assert!(error.contains("blocked while autonomous mutation"));
    }

    #[test]
    fn daemon_reload_result_is_read_only_and_reports_active_blocker() {
        let root =
            std::env::temp_dir().join(format!("catdesk-daemon-reload-result-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        handle_tool(
            "autonomy_contract_create",
            json!({"contract":contract(&root)}),
            &root,
        )
        .expect("create");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let mut snapshot = store.load_session("session-1").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Running;
        snapshot.active = true;
        store.save_session(&snapshot).expect("running state");

        let result = handle_tool("catdesk_daemon_reload", json!({"action":"RESULT"}), &root)
            .expect("read-only reload result");
        assert_eq!(
            result.get("state").and_then(Value::as_str),
            Some("BLOCKED_ACTIVE_MUTATION")
        );
        assert_eq!(
            result.get("activeMutation").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            result.get("preflightPresent").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            result.get("tunnelAction").and_then(Value::as_str),
            Some("none-external-tunnel-untouched")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn daemon_reload_schema_is_closed_to_reviewed_preflight_and_confirm() {
        let schema = tool_schemas()
            .into_iter()
            .find(|schema| schema["name"] == "catdesk_daemon_reload")
            .expect("reload schema");
        let input = &schema["inputSchema"];
        assert_eq!(input["type"], "object");
        assert_eq!(input["oneOf"].as_array().map(Vec::len), Some(3));
        assert_eq!(input["oneOf"][2]["required"], json!(["action"]));
        assert_eq!(input["oneOf"][2]["properties"]["action"]["const"], "RESULT");
        assert_eq!(
            input["oneOf"][0]["required"],
            json!(["action", "buildPath", "expectedSha256", "recordId"])
        );
        assert_eq!(
            input["oneOf"][0]["properties"]["action"]["const"],
            "PREFLIGHT"
        );
        assert_eq!(input["oneOf"][0]["additionalProperties"], false);
        assert_eq!(
            input["oneOf"][1]["required"],
            json!(["action", "buildPath", "expectedSha256", "confirmationToken"])
        );
        assert_eq!(
            input["oneOf"][1]["properties"]["action"]["const"],
            "CONFIRM"
        );
        assert_eq!(input["oneOf"][1]["additionalProperties"], false);
        let serialized = serde_json::to_string(input).expect("schema JSON");
        for forbidden in ["dryRun", "decision", "\"RESULT\""] {
            assert!(
                !serialized.contains(forbidden),
                "reload schema must not expose legacy field/value {forbidden}"
            );
        }
    }

    #[test]
    fn daemon_reload_handler_rejects_legacy_and_unrelated_shapes_when_idle() {
        let root = std::env::temp_dir().join(format!("catdesk-reload-closed-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        for args in [
            json!({
                "buildPath":"candidate.exe",
                "expectedSha256":"0".repeat(64),
                "dryRun":true
            }),
            json!({"decision":"CATDESK_CANONICAL_RECOVERY"}),
            json!({"action":"RESULT"}),
            json!({
                "action":"PREFLIGHT",
                "buildPath":"candidate.exe",
                "expectedSha256":"0".repeat(64),
                "recordId":"review-record",
                "dryRun":true
            }),
        ] {
            assert!(
                handle_tool("catdesk_daemon_reload", args, &root).is_err(),
                "legacy/unrelated reload shape must fail closed"
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn canonical_recovery_surface_and_cached_bridge_are_closed_before_execution() {
        assert!(AUTONOMY_MCP_TOOL_NAMES.contains(&"catdesk_release_recovery"));
        let schema = tool_schemas()
            .into_iter()
            .find(|schema| {
                schema.get("name").and_then(Value::as_str) == Some("catdesk_release_recovery")
            })
            .expect("recovery schema");
        assert_eq!(
            schema["inputSchema"]["additionalProperties"].as_bool(),
            Some(false)
        );
        assert_eq!(schema["inputSchema"]["maxProperties"].as_u64(), Some(0));

        let root = std::env::temp_dir().join(format!("catdesk-recovery-mcp-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("workspace");
        for args in [
            json!({"script":"evil.ps1"}),
            json!({"path":"C:/outside"}),
            json!({"command":"whoami"}),
            json!({"tunnelId":"external"}),
        ] {
            assert!(handle_tool("catdesk_release_recovery", args, &root).is_err());
        }
        for args in [
            json!({"decision":"CATDESK_CANONICAL_RECOVERY", "dryRun":false}),
            json!({"decision":"CATDESK_CANONICAL_RECOVERY", "buildPath":"x.exe"}),
            json!({"decision":"OTHER"}),
        ] {
            assert!(handle_tool("catdesk_daemon_reload", args, &root).is_err());
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn direct_work_claim_request_is_approved_queued_bound_and_idempotent() {
        let root =
            std::env::temp_dir().join(format!("catdesk-direct-claim-mcp-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        handle_tool(
            "autonomy_contract_create",
            json!({"contract":contract(&root)}),
            &root,
        )
        .expect("create");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let mut snapshot = store.load_session("session-1").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::Queued;
        snapshot.active = true;
        snapshot.approved_contract_hash = Some("approved-contract".into());
        store.save_session(&snapshot).expect("queued state");
        let expected = snapshot.last_event_sequence;
        let request = json!({
            "sessionId":"session-1",
            "expectedStateVersion":expected,
            "idempotencyKey":"direct-claim-1"
        });
        let first = handle_tool("autonomy_session_claim_direct_work", request.clone(), &root)
            .expect("direct claim request");
        assert_eq!(first.get("state").and_then(Value::as_str), Some("QUEUED"));
        handle_tool("autonomy_session_claim_direct_work", request, &root)
            .expect("idempotent direct claim replay");
        assert_eq!(
            store
                .poll_events("session-1", 0)
                .expect("events")
                .iter()
                .filter(|event| event.kind == "direct_work_claim_requested")
                .count(),
            1
        );

        let mut invalid = store.load_session("session-1").expect("invalid fixture");
        invalid.state = AutonomousSessionStateV1::WaitingForChatgpt;
        invalid.current_task_id = Some("work".into());
        invalid.provider_turn_count = 1;
        store.save_session(&invalid).expect("owned state");
        let expected = invalid.last_event_sequence;
        assert!(
            handle_tool(
                "autonomy_session_claim_direct_work",
                json!({
                    "sessionId":"session-1",
                    "expectedStateVersion":expected,
                    "idempotencyKey":"direct-claim-invalid"
                }),
                &root,
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn direct_work_finalize_request_is_waiting_bound_and_idempotent() {
        let root =
            std::env::temp_dir().join(format!("catdesk-direct-finalize-mcp-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        handle_tool(
            "autonomy_contract_create",
            json!({"contract":contract(&root)}),
            &root,
        )
        .expect("create");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let mut snapshot = store.load_session("session-1").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::WaitingForChatgpt;
        snapshot.active = true;
        snapshot.current_task_id = Some("work".into());
        snapshot.provider_turn_count = 1;
        store.save_session(&snapshot).expect("waiting state");
        let expected = snapshot.last_event_sequence;
        let request = json!({
            "sessionId":"session-1",
            "expectedStateVersion":expected,
            "idempotencyKey":"direct-finalize-1"
        });
        let first = handle_tool(
            "autonomy_session_finalize_direct_work",
            request.clone(),
            &root,
        )
        .expect("direct finalize request");
        assert_eq!(
            first.get("state").and_then(Value::as_str),
            Some("WAITING_FOR_CHATGPT")
        );
        handle_tool("autonomy_session_finalize_direct_work", request, &root)
            .expect("idempotent direct finalize replay");
        assert_eq!(
            store
                .poll_events("session-1", 0)
                .expect("events")
                .iter()
                .filter(|event| event.kind == "direct_work_finalize_requested")
                .count(),
            1
        );

        let mut invalid = store.load_session("session-1").expect("invalid fixture");
        invalid.state = AutonomousSessionStateV1::Queued;
        store.save_session(&invalid).expect("queued state");
        let expected = invalid.last_event_sequence;
        assert!(
            handle_tool(
                "autonomy_session_finalize_direct_work",
                json!({
                    "sessionId":"session-1",
                    "expectedStateVersion":expected,
                    "idempotencyKey":"direct-finalize-invalid"
                }),
                &root,
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn planner_reply_is_persisted_and_completion_artifacts_are_bounded_for_mcp() {
        let root = std::env::temp_dir().join(format!("catdesk-autonomy-mcp-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        handle_tool(
            "autonomy_contract_create",
            json!({"contract":contract(&root)}),
            &root,
        )
        .expect("create");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let mut snapshot = store.load_session("session-1").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::WaitingForChatgpt;
        snapshot.active = true;
        store.save_session(&snapshot).expect("waiting state");
        let escalation = store
            .write_escalation(
                "session-1",
                AutonomousSessionStateV1::WaitingForChatgpt,
                "provider_terminal_error",
            )
            .expect("escalation");
        handle_tool(
            "autonomy_session_reply",
            json!({
                "sessionId":"session-1",
                "expectedStateVersion":1,
                "idempotencyKey":"reply-1",
                "escalationId":escalation.escalation_id,
                "decisionHash":"decision-hash",
                "decision":"choose the bounded repair",
                "constraints":["preserve policy"]
            }),
            &root,
        )
        .expect("reply");
        assert_eq!(
            store
                .load_planner_reply("session-1")
                .expect("stored reply")
                .decision,
            "choose the bounded repair"
        );

        let verification = crate::delegated::coordinator::VerificationSummaryV1 {
            status: crate::delegated::coordinator::VerificationStatusV1::Passed,
            command: "cargo test".into(),
            summary: "passed".into(),
        };
        store
            .write_completion_artifacts(
                "session-1",
                &verification,
                &format!("{}tail", "d".repeat(24_100)),
                "final review",
            )
            .expect("artifacts");
        let mut completed = store.load_session("session-1").expect("queued state");
        completed.state = AutonomousSessionStateV1::CompletedVerified;
        completed.active = false;
        store.save_session(&completed).expect("completed state");
        let diff = handle_tool(
            "autonomy_session_get_diff",
            json!({"sessionId":"session-1"}),
            &root,
        )
        .expect("diff");
        assert_eq!(diff.get("available").and_then(Value::as_bool), Some(true));
        assert_eq!(diff.get("truncated").and_then(Value::as_bool), Some(true));
        let review = handle_tool(
            "autonomy_session_get_final_review",
            json!({"sessionId":"session-1"}),
            &root,
        )
        .expect("review");
        assert_eq!(
            review.get("finalReview").and_then(Value::as_str),
            Some("final review")
        );
    }

    #[test]
    fn rejected_planner_replies_leave_no_durable_launch_permit() {
        let root =
            std::env::temp_dir().join(format!("catdesk-autonomy-reply-reject-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        handle_tool(
            "autonomy_contract_create",
            json!({"contract":contract(&root)}),
            &root,
        )
        .expect("create");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let mut snapshot = store.load_session("session-1").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::WaitingForChatgpt;
        snapshot.active = true;
        store.save_session(&snapshot).expect("waiting state");
        let escalation = store
            .write_escalation(
                "session-1",
                AutonomousSessionStateV1::WaitingForChatgpt,
                "provider_terminal_error",
            )
            .expect("escalation");

        for (expected, escalation_id) in [
            (0, escalation.escalation_id.as_str()),
            (1, "wrong-escalation"),
        ] {
            assert!(
                handle_tool(
                    "autonomy_session_reply",
                    json!({
                        "sessionId":"session-1",
                        "expectedStateVersion":expected,
                        "idempotencyKey":format!("reject-{expected}"),
                        "escalationId":escalation_id,
                        "decisionHash":"decision-hash",
                        "decision":"do not launch",
                        "constraints":[]
                    }),
                    &root,
                )
                .is_err()
            );
            let current = store
                .load_session("session-1")
                .expect("state remains waiting");
            assert_eq!(current.state, AutonomousSessionStateV1::WaitingForChatgpt);
            assert!(store.load_planner_reply("session-1").is_err());
            assert_eq!(current.provider_turn_count, 0);
        }
    }

    #[test]
    fn planner_reply_replay_is_durable_once_and_requeues_once() {
        let root =
            std::env::temp_dir().join(format!("catdesk-autonomy-reply-replay-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        handle_tool(
            "autonomy_contract_create",
            json!({"contract":contract(&root)}),
            &root,
        )
        .expect("create");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let mut snapshot = store.load_session("session-1").expect("snapshot");
        snapshot.state = AutonomousSessionStateV1::WaitingForChatgpt;
        snapshot.active = true;
        store.save_session(&snapshot).expect("waiting state");
        let escalation = store
            .write_escalation(
                "session-1",
                AutonomousSessionStateV1::WaitingForChatgpt,
                "provider_terminal_error",
            )
            .expect("escalation");
        let request = json!({
            "sessionId":"session-1",
            "expectedStateVersion":1,
            "idempotencyKey":"reply-replay",
            "escalationId":escalation.escalation_id,
            "decisionHash":"decision-hash",
            "decision":"resume the approved task",
            "constraints":["preserve graph"]
        });
        handle_tool("autonomy_session_reply", request.clone(), &root).expect("first reply");
        handle_tool("autonomy_session_reply", request, &root).expect("replay");
        assert_eq!(
            store.load_session("session-1").expect("queued state").state,
            AutonomousSessionStateV1::Queued
        );
        assert_eq!(
            store
                .poll_events("session-1", 0)
                .expect("events")
                .iter()
                .filter(|event| event.kind == "planner_reply")
                .count(),
            1
        );
    }

    #[test]
    fn ticket_audit_joins_bounded_execution_review_and_explicit_supersession_evidence() {
        use crate::delegated::coordinator::{VerificationStatusV1, VerificationSummaryV1};

        let root = std::env::temp_dir().join(format!("catdesk-ticket-audit-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        let mut first = contract(&root);
        first.contract_id = "session-one".into();
        first.task_id = "ticket-one".into();
        let mut second = contract(&root);
        second.contract_id = "session-two".into();
        second.task_id = "ticket-one".into();
        handle_tool("autonomy_contract_create", json!({"contract":first}), &root).expect("first");
        handle_tool(
            "autonomy_contract_create",
            json!({"contract":second}),
            &root,
        )
        .expect("second");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let accounting = store.execution_accounting_store().expect("accounting");
        accounting
            .record_task_started(
                "project",
                "ticket-one",
                "session-one",
                None,
                "codex-cli",
                None,
                None,
                10_000,
            )
            .expect("first started");
        accounting
            .finish_task("session-one", Some("diff"), Some("review"), 12_000)
            .expect("first ended");
        accounting
            .record_task_started(
                "project",
                "ticket-one",
                "session-two",
                None,
                "codex-cli",
                None,
                None,
                15_000,
            )
            .expect("second started");
        let mut completed = store
            .load_session("session-one")
            .expect("completed snapshot");
        completed.state = AutonomousSessionStateV1::CompletedVerified;
        completed.active = false;
        store.save_session(&completed).expect("completed state");
        store
            .write_completion_artifacts(
                "session-one",
                &VerificationSummaryV1 {
                    status: VerificationStatusV1::Passed,
                    command: "cargo test".into(),
                    summary: "passed".into(),
                },
                "diff",
                "review",
            )
            .expect("completion evidence");
        let review = store
            .emit_review_inbox_record(
                "session-one",
                "project",
                AutonomousSessionStateV1::CompletedVerified,
                "independent_final_review",
                "completion artifact",
                12,
            )
            .expect("review");
        store
            .acknowledge_review_inbox(&review.record_id)
            .expect("acknowledged");

        let report = handle_tool(
            "autonomy_ticket_audit",
            json!({"startUnix":5,"endUnix":20,"projectId":"project","limit":1,"maxBytes":24000}),
            &root,
        )
        .expect("audit");
        assert_eq!(report.get("truncated").and_then(Value::as_bool), Some(true));
        let first_row = report
            .get("records")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .expect("first row");
        assert_eq!(
            first_row.get("auditStatus").and_then(Value::as_str),
            Some("COMPLETED_REVIEWED")
        );
        assert_eq!(
            first_row
                .get("verifiedChangeSummary")
                .and_then(Value::as_str),
            Some("completion artifact recorded")
        );
        assert!(first_row.get("references").is_some());
        assert!(
            handle_tool(
                "autonomy_ticket_audit",
                json!({"startUnix":20,"endUnix":5}),
                &root
            )
            .is_err()
        );

        handle_tool(
            "autonomy_session_supersede",
            json!({"supersededSessionId":"session-one","supersedingSessionId":"session-two"}),
            &root,
        )
        .expect("explicit supersession");
        let report = handle_tool(
            "autonomy_ticket_audit",
            json!({"startUnix":5,"endUnix":20,"projectId":"project"}),
            &root,
        )
        .expect("superseded audit");
        let rows = report
            .get("records")
            .and_then(Value::as_array)
            .expect("rows");
        assert_eq!(rows.len(), 2, "multiple sessions remain distinct");
        assert_eq!(
            rows[0].get("auditStatus").and_then(Value::as_str),
            Some("SUPERSEDED")
        );
        assert_eq!(
            rows[1].get("auditStatus").and_then(Value::as_str),
            Some("RUNNING_QUEUED")
        );
        assert_eq!(
            report.get("redactionPolicy").and_then(Value::as_str),
            Some("NO_TRANSCRIPTS_CREDENTIALS_BROWSER_PROFILE_TUNNEL_OR_RAW_ARTIFACT_CONTENT")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn work_time_report_is_bounded_read_only_and_reports_unknown_chatgpt() {
        let root =
            std::env::temp_dir().join(format!("catdesk-work-time-report-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let accounting = store.execution_accounting_store().expect("accounting");
        accounting
            .record_task_started(
                "project",
                "task",
                "session",
                None,
                "codex-cli",
                None,
                None,
                1_000,
            )
            .expect("start");
        accounting
            .activity_started(
                "session",
                super::super::autonomy_accounting::ActivityActorV1::CodexProviderActive,
                super::super::autonomy_accounting::ActivityEvidenceV1::ProviderLifecycle,
                1_100,
            )
            .expect("provider");
        accounting
            .finish_task("session", None, None, 2_000)
            .expect("finish");
        let report = handle_tool(
            "autonomy_work_time_report",
            json!({"startUnix":1,"endUnix":3,"projectId":"project","limit":10,"maxBytes":24000}),
            &root,
        )
        .expect("report");
        assert_eq!(
            report
                .pointer("/entries/0/chatgptWebStatus")
                .and_then(Value::as_str),
            Some("UNKNOWN_NOT_OBSERVABLE")
        );
        assert!(
            handle_tool(
                "autonomy_work_time_report",
                json!({"startUnix":3,"endUnix":1}),
                &root,
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ticket_audit_distinguishes_unreviewed_waiting_failed_cancelled_and_incomplete_evidence() {
        use crate::delegated::coordinator::{VerificationStatusV1, VerificationSummaryV1};

        let root =
            std::env::temp_dir().join(format!("catdesk-ticket-audit-status-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("workspace");
        let store =
            AutonomousStateStoreV1::open(root.join(".catdesk").join("autonomy")).expect("store");
        let accounting = store.execution_accounting_store().expect("accounting");
        for (session_id, state, at_millis, artifacts) in [
            (
                "audit-unreviewed",
                AutonomousSessionStateV1::CompletedVerified,
                10_000_u128,
                true,
            ),
            (
                "audit-waiting",
                AutonomousSessionStateV1::WaitingForChatgpt,
                11_000,
                false,
            ),
            (
                "audit-failed",
                AutonomousSessionStateV1::Failed,
                12_000,
                false,
            ),
            (
                "audit-cancelled",
                AutonomousSessionStateV1::Cancelled,
                13_000,
                false,
            ),
            (
                "audit-incomplete",
                AutonomousSessionStateV1::CompletedVerified,
                14_000,
                false,
            ),
            (
                "audit-outside",
                AutonomousSessionStateV1::Running,
                90_000,
                false,
            ),
        ] {
            let mut item = contract(&root);
            item.contract_id = session_id.into();
            item.task_id = "ticket-status".into();
            handle_tool("autonomy_contract_create", json!({"contract":item}), &root)
                .expect("contract");
            accounting
                .record_task_started(
                    "project",
                    "ticket-status",
                    session_id,
                    None,
                    "codex-cli",
                    None,
                    None,
                    at_millis,
                )
                .expect("started");
            let mut snapshot = store.load_session(session_id).expect("snapshot");
            snapshot.active = state == AutonomousSessionStateV1::WaitingForChatgpt
                || state == AutonomousSessionStateV1::Running;
            snapshot.state = state;
            store.save_session(&snapshot).expect("state");
            if artifacts {
                store
                    .write_completion_artifacts(
                        session_id,
                        &VerificationSummaryV1 {
                            status: VerificationStatusV1::Passed,
                            command: "cargo test".into(),
                            summary: "passed".into(),
                        },
                        "diff",
                        "review",
                    )
                    .expect("artifact");
            }
        }
        let report = handle_tool(
            "autonomy_ticket_audit",
            json!({"startUnix":5,"endUnix":20,"projectId":"project"}),
            &root,
        )
        .expect("audit");
        let states = report
            .get("records")
            .and_then(Value::as_array)
            .expect("rows")
            .iter()
            .map(|row| {
                (
                    row.get("sessionId")
                        .and_then(Value::as_str)
                        .expect("session"),
                    row.get("auditStatus")
                        .and_then(Value::as_str)
                        .expect("status"),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            states.get("audit-unreviewed"),
            Some(&"COMPLETED_UNREVIEWED")
        );
        assert_eq!(states.get("audit-waiting"), Some(&"WAITING"));
        assert_eq!(states.get("audit-failed"), Some(&"FAILED"));
        assert_eq!(states.get("audit-cancelled"), Some(&"CANCELLED"));
        assert_eq!(
            states.get("audit-incomplete"),
            Some(&"UNKNOWN_INCOMPLETE_EVIDENCE")
        );
        assert!(!states.contains_key("audit-outside"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn wake_policy_defaults_indefinite_and_off_is_cas_safe_without_enablement() {
        let root = std::env::temp_dir().join(format!("catdesk-wake-policy-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("workspace");
        let initial =
            handle_tool("autonomy_wake_policy_get", json!({}), &root).expect("default policy");
        assert_eq!(
            initial.pointer("/policy/mode").and_then(Value::as_str),
            Some("INDEFINITE")
        );
        assert_eq!(
            initial
                .pointer("/policy/generation")
                .and_then(Value::as_u64),
            Some(0)
        );
        let off = handle_tool(
            "autonomy_wake_policy_set",
            json!({"expectedGeneration":0,"mode":"MANUAL_OFF"}),
            &root,
        )
        .expect("off always allowed");
        assert_eq!(
            off.pointer("/policy/generation").and_then(Value::as_u64),
            Some(1)
        );
        assert!(
            handle_tool(
                "autonomy_wake_policy_set",
                json!({"expectedGeneration":0,"mode":"MANUAL_OFF"}),
                &root,
            )
            .is_err()
        );
        assert!(
            handle_tool(
                "autonomy_wake_policy_set",
                json!({"expectedGeneration":1,"mode":"INDEFINITE"}),
                &root,
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
