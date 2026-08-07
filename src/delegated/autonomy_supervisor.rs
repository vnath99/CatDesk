//! Durable, fail-closed MCP supervisor operations for autonomous Codex work.
//!
//! This surface persists only contract-governed control-plane facts. Operator
//! configuration such as the direct Codex executable remains outside MCP.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::autonomous_contract::{AutonomousDevelopmentContractV1, AutonomousPolicyEngineV1};
use super::autonomy_state::{
    AutonomousQueueTaskStateV1, AutonomousQueueTaskV1, AutonomousQueueV1,
    AutonomousSessionSnapshotV1, AutonomousSessionStateV1, AutonomousStateStoreV1,
};

pub const AUTONOMY_MCP_TOOL_NAMES: [&str; 18] = [
    "autonomy_contract_create",
    "autonomy_contract_validate",
    "autonomy_contract_approve",
    "autonomy_session_start",
    "autonomy_session_status",
    "autonomy_session_events",
    "autonomy_session_reply",
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
];

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
                    | "autonomy_session_status"
                    | "autonomy_session_events"
                    | "autonomy_session_get_checkpoint"
                    | "autonomy_session_get_diff"
                    | "autonomy_session_get_escalation"
                    | "autonomy_session_get_final_review"
                    | "autonomy_session_list"
                    | "provider_status"
                    | "autonomy_queue_status"
            );
            json!({
                "name": name,
                "title": name.replace('_', " "),
                "description": "CatDesk autonomous-development control-plane operation. Operator executable and authentication configuration are never accepted through MCP.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "contract": { "type": "object" },
                        "sessionId": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" },
                        "expectedStateVersion": { "type": "integer", "minimum": 0 },
                        "idempotencyKey": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" },
                        "approvalId": { "type": "string", "pattern": "^[A-Za-z0-9_-]{1,128}$" },
                        "decisionHash": { "type": "string", "minLength": 1, "maxLength": 256 },
                        "afterSequence": { "type": "integer", "minimum": 0 },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
                        "maxBytes": { "type": "integer", "minimum": 1, "maximum": 24000 },
                        "decision": { "type": "string", "minLength": 1, "maxLength": 4000 },
                        "constraints": { "type": "array", "maxItems": 20, "items": {"type":"string","maxLength":512} },
                        "newExpiryUnix": { "type": "integer", "minimum": 1 }
                    }
                },
                "annotations": {"readOnlyHint":read_only,"openWorldHint":false,"destructiveHint":matches!(*name,"autonomy_session_cancel")}
            })
        })
        .collect()
}

pub fn handle_tool(name: &str, args: Value, workspace: &Path) -> Result<Value, String> {
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "workspace canonicalization failed".to_string())?;
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
        "autonomy_session_get_diff" | "autonomy_session_get_final_review" => Ok(json!({
            "available": false,
            "reason": "artifact is unavailable until the controller reaches verified completion"
        })),
        "autonomy_session_reply" => supervisor.reply(
            session_id(&args)?,
            expected_state_version(&args)?,
            required_str(&args, "idempotencyKey")?,
            required_str(&args, "decisionHash")?,
        ),
        "autonomy_session_list" => supervisor.list(),
        "autonomy_queue_status" => supervisor.queue(session_id(&args)?),
        "provider_status" => Ok(json!({
            "providerId": "codex-cli",
            "status": "OPERATOR_CONFIGURATION_REQUIRED",
            "detail": "MCP cannot select an executable or read authentication files."
        })),
        _ => Err("unknown autonomous supervisor tool".into()),
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

    fn create_contract(&self, contract: AutonomousDevelopmentContractV1) -> Result<Value, String> {
        let policy = AutonomousPolicyEngineV1::new(contract.clone())
            .map_err(|_| "autonomous contract policy rejected the request".to_string())?;
        if policy.contract().workspace.canonicalize().ok().as_deref() != Some(&self.workspace) {
            return Err("autonomous contract workspace must match the MCP workspace".into());
        }
        let session_id = contract.contract_id.clone();
        let queue = AutonomousQueueV1::new(vec![AutonomousQueueTaskV1 {
            task_id: contract.task_id.clone(),
            priority: 1,
            depends_on: Vec::new(),
            state: AutonomousQueueTaskStateV1::Ready,
        }])
        .map_err(|_| "autonomous queue could not be created".to_string())?;
        self.store
            .create_session(&session_id, queue)
            .map_err(|_| "autonomous session already exists or could not be created".to_string())?;
        self.store
            .save_contract(&session_id, &contract)
            .map_err(|_| "autonomous contract could not be persisted".to_string())?;
        self.store
            .append_event(
                &session_id,
                "contract_created",
                "contract persisted and awaiting approval",
            )
            .map_err(|_| "autonomous event could not be persisted".to_string())?;
        Ok(json!({"sessionId":session_id,"contractHash":policy.contract_hash(),"state":"DRAFT"}))
    }

    fn validate_contract(&self, session_id: &str) -> Result<Value, String> {
        let contract = self.load_policy(session_id)?;
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
        self.mutate(session_id, expected, key, "start", |snapshot| {
            if policy.contract().autonomy_lease.start_approval_required
                && snapshot.approved_contract_hash.as_deref() != Some(policy.contract_hash())
            {
                return Err("start approval is required for this autonomous contract".into());
            }
            if snapshot.state != AutonomousSessionStateV1::Queued {
                return Err("autonomy session is not ready to start".into());
            }
            // The controller tick is deliberately started by CatDesk's local runtime,
            // not by MCP-provided executable or environment values.
            snapshot.state = AutonomousSessionStateV1::Queued;
            snapshot.active = true;
            Ok(())
        })
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
        self.mutate(session_id, expected, key, event, |snapshot| {
            if snapshot.state.is_terminal() {
                return Err("terminal autonomous session cannot be changed".into());
            }
            snapshot.state = state;
            snapshot.active = true;
            Ok(())
        })
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

    fn reply(
        &self,
        session_id: &str,
        expected: u64,
        key: &str,
        decision_hash: &str,
    ) -> Result<Value, String> {
        if decision_hash.trim().is_empty() {
            return Err("planner decision hash is required".into());
        }
        self.mutate(session_id, expected, key, "planner_reply", |snapshot| {
            if snapshot.state != AutonomousSessionStateV1::WaitingForChatgpt {
                return Err("autonomy session is not awaiting a planner reply".into());
            }
            snapshot.state = AutonomousSessionStateV1::Queued;
            snapshot.active = true;
            Ok(())
        })
    }

    fn escalation(&self, session_id: &str) -> Result<Value, String> {
        match self.store.load_escalation(session_id) {
            Ok(packet) => {
                serde_json::to_value(packet).map_err(|_| "escalation serialization failed".into())
            }
            Err(_) => Ok(json!({"available":false})),
        }
    }

    fn queue(&self, session_id: &str) -> Result<Value, String> {
        serde_json::to_value(
            self.store
                .load_queue(session_id)
                .map_err(|_| "autonomy queue is unavailable".to_string())?,
        )
        .map_err(|_| "autonomy queue serialization failed".into())
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
fn required_u64(args: &Value, name: &str) -> Result<u64, String> {
    required(args, name)?
        .as_u64()
        .ok_or_else(|| format!("{name} must be an integer"))
}
fn optional_u64(args: &Value, name: &str) -> Option<u64> {
    args.get(name).and_then(Value::as_u64)
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
                primary_model: "default".into(),
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
        }
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
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
