//! Narrow bridge from autonomous local-Qwen turns to the existing CatDesk
//! integrated tool service.  This owns no filesystem or shell authority of
//! its own: every tool invocation is journaled and executed by that service.

use std::path::Path;

use serde_json::json;

use super::autonomous_contract::AutonomousPolicyEngineV1;
use super::contracts::{
    ApprovalRequirementV1, ExecutionContractV1, ExpectedArtifactKind, ExpectedArtifactV1,
    ProviderPolicyV1,
};
use super::integrated::{
    IntegratedDelegatedService, IntegratedRunConfigV1, IntegratedToolResultV1,
};
use super::runtime::{NormalizedToolCallV1, ProviderMessageV1, RuntimeError, ToolDefinitionV1};

/// Reuses `IntegratedDelegatedService` for Qwen only.  In particular, this
/// adapter deliberately excludes the job tools: an autonomous Qwen turn has
/// no shell surface and can only use the bounded read/patch/verify/diff tools.
pub struct AutonomousQwenToolLoopV1 {
    policy: AutonomousPolicyEngineV1,
    service: IntegratedDelegatedService,
    started: bool,
}

impl AutonomousQwenToolLoopV1 {
    pub fn recover(
        policy: AutonomousPolicyEngineV1,
        session_id: &str,
        task_id: &str,
    ) -> Result<Self, RuntimeError> {
        let workspace = policy.contract().workspace.clone();
        let contract = execution_contract(&policy, session_id, task_id)?;
        let root = workspace
            .join(".catdesk")
            .join("autonomy")
            .join("qwen_tools");
        let config = IntegratedRunConfigV1 {
            journal_root: root.join("journal"),
            job_root: root.join("jobs"),
            // The adapter is not used to contact Ollama; retain a loopback-only
            // value because this is an integrated-service configuration field.
            ollama_base_url: "http://127.0.0.1:11434".into(),
            model_id: policy.contract().provider_policy.routine_model.clone(),
            advisor: None,
        };
        let service = IntegratedDelegatedService::recover(&workspace, contract, config)
            .map_err(integrated_error)?;
        Ok(Self {
            policy,
            service,
            started: false,
        })
    }

    pub fn tool_definitions(&self) -> Vec<ToolDefinitionV1> {
        self.service.tool_definitions()
    }

    pub fn history(&self) -> Vec<ProviderMessageV1> {
        self.service.provider_history()
    }

    pub fn record_text(&mut self, text: String) -> Result<(), RuntimeError> {
        self.service
            .record_provider_message(ProviderMessageV1 {
                role: "assistant".into(),
                content: text,
                tool_call_id: None,
                tool_name: None,
            })
            .map_err(integrated_error)
    }

    pub async fn execute(
        &mut self,
        call: &NormalizedToolCallV1,
    ) -> Result<IntegratedToolResultV1, RuntimeError> {
        self.ensure_started()?;
        self.validate_scope(call)?;
        self.service
            .record_provider_message(ProviderMessageV1 {
                role: "assistant".into(),
                content: serde_json::to_string(
                    &json!({"tool": call.tool_name, "arguments": call.arguments}),
                )
                .map_err(|error| RuntimeError::Validation(error.to_string()))?,
                tool_call_id: Some(call.tool_call_id.as_str().into()),
                tool_name: Some(call.tool_name.clone()),
            })
            .map_err(integrated_error)?;
        self.service
            .execute_tool_call(call)
            .await
            .map_err(integrated_error)
    }

    pub fn completion_ready(&self, claim: &str) -> Result<(), RuntimeError> {
        self.service
            .completion_gate(claim)
            .map_err(integrated_error)
    }

    fn ensure_started(&mut self) -> Result<(), RuntimeError> {
        if !self.started {
            self.service.start().map_err(integrated_error)?;
            self.started = true;
        }
        Ok(())
    }

    fn validate_scope(&self, call: &NormalizedToolCallV1) -> Result<(), RuntimeError> {
        match call.tool_name.as_str() {
            "read" => self.permit_existing(required_path(&call.arguments, "path")?),
            "search" => self.permit_existing(required_path(&call.arguments, "path")?),
            "patch.preview" => {
                let operations = call
                    .arguments
                    .get("operations")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        RuntimeError::Validation("patch.preview requires operations".into())
                    })?;
                if operations.is_empty() {
                    return Err(RuntimeError::Validation(
                        "patch.preview operations are empty".into(),
                    ));
                }
                for operation in operations {
                    self.permit_existing(required_path(operation, "path")?)?;
                }
                Ok(())
            }
            "diff.actual" => {
                let paths = call
                    .arguments
                    .get("paths")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        RuntimeError::Validation(
                            "diff.actual requires explicit allowed paths".into(),
                        )
                    })?;
                if paths.is_empty() {
                    return Err(RuntimeError::Validation(
                        "diff.actual paths are empty".into(),
                    ));
                }
                for path in paths {
                    self.permit_existing(path.as_str().ok_or_else(|| {
                        RuntimeError::Validation("diff.actual path must be text".into())
                    })?)?;
                }
                Ok(())
            }
            "patch.apply" | "patch.compare" | "verify.run" => Ok(()),
            _ => Err(RuntimeError::Validation(
                "Qwen requested a tool outside the approved autonomous surface".into(),
            )),
        }
    }

    fn permit_existing(&self, relative: &str) -> Result<(), RuntimeError> {
        let candidate = self.policy.contract().workspace.join(Path::new(relative));
        self.policy.permit_path(&candidate).map_err(|_| {
            RuntimeError::Validation(
                "Qwen tool path is outside the autonomous contract allowlist".into(),
            )
        })
    }
}

fn required_path<'a>(value: &'a serde_json::Value, field: &str) -> Result<&'a str, RuntimeError> {
    value
        .get(field)
        .and_then(|v| v.as_str())
        .filter(|path| !path.is_empty())
        .ok_or_else(|| RuntimeError::Validation(format!("Qwen tool requires {field}")))
}

fn execution_contract(
    policy: &AutonomousPolicyEngineV1,
    session_id: &str,
    task_id: &str,
) -> Result<ExecutionContractV1, RuntimeError> {
    let contract = policy.contract();
    let relative = |path: &std::path::PathBuf| -> Result<String, RuntimeError> {
        path.strip_prefix(&contract.workspace)
            .map_err(|_| RuntimeError::Validation("autonomous path escaped workspace".into()))
            .map(|p| {
                let path = p.to_string_lossy().replace('\\', "/");
                if path.is_empty() { ".".into() } else { path }
            })
    };
    Ok(ExecutionContractV1 {
        schema_version: super::EXECUTION_CONTRACT_SCHEMA_VERSION,
        task_id: format!("qwen-{session_id}-{task_id}"),
        objective: contract.objective.clone(),
        workspace: contract.workspace.to_string_lossy().into_owned(),
        feature_branch: contract.feature_branch.clone(),
        allowed_paths: contract
            .allowed_paths
            .iter()
            .map(&relative)
            .collect::<Result<_, _>>()?,
        forbidden_paths: contract
            .forbidden_paths
            .iter()
            .map(&relative)
            .collect::<Result<_, _>>()?,
        allowed_command_profiles: contract
            .allowed_command_profiles
            .iter()
            .map(|p| p.exact_argv().join(" "))
            .collect(),
        ordered_steps: contract.ordered_steps.clone(),
        acceptance_criteria: vec![
            "verification passes".into(),
            "authoritative diff is captured".into(),
        ],
        retry_budget: contract.verification_policy.max_repair_cycles.max(1),
        max_turns: contract.autonomy_lease.maximum_provider_turns,
        max_tool_calls: contract.autonomy_lease.maximum_tool_calls,
        max_elapsed_seconds: contract.autonomy_lease.maximum_total_elapsed_seconds,
        provider_policy: ProviderPolicyV1 {
            primary_provider_id: "ollama".into(),
            primary_model_id: contract.provider_policy.routine_model.clone(),
            fallback_provider_ids: Vec::new(),
            require_tool_calls: true,
            allow_paid_fallbacks: false,
        },
        advisor_policy: None,
        escalation_conditions: vec!["autonomous policy rejection".into()],
        approval_requirements: vec![ApprovalRequirementV1 {
            kind: super::contracts::ApprovalRequirementKind::RunStart,
            required: false,
            reason: "autonomous task already approved".into(),
        }],
        expected_artifacts: vec![
            ExpectedArtifactV1 {
                kind: ExpectedArtifactKind::Diff,
                name: "authoritative diff".into(),
                required: true,
            },
            ExpectedArtifactV1 {
                kind: ExpectedArtifactKind::Verification,
                name: "verification".into(),
                required: true,
            },
        ],
        verification_profile: contract.verification_policy.profile.clone(),
    })
}

fn integrated_error(error: super::integrated::IntegratedError) -> RuntimeError {
    RuntimeError::Provider(format!(
        "CatDesk Qwen tool loop rejected or failed the request: {error:?}"
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::delegated::autonomous_contract::{
        AutonomousCommandProfileV1, AutonomousDevelopmentContractV1, AutonomousGitPolicyV1,
        AutonomousHardStopV1, AutonomousProviderPolicyV1, AutonomousRateLimitPolicyV1,
        AutonomousVerificationPolicyV1, AutonomyLeaseV1,
    };
    use crate::delegated::contracts::ToolCallId;

    fn call(id: &str, tool: &str, arguments: serde_json::Value) -> NormalizedToolCallV1 {
        NormalizedToolCallV1 {
            tool_call_id: ToolCallId::new(id).expect("tool id"),
            tool_name: tool.into(),
            arguments,
            arguments_hash: format!("test-{id}"),
        }
    }

    fn git(root: &std::path::Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git");
        assert!(
            output.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    async fn deterministic_qwen_tool_path_executes_read_preview_apply_verify_and_diff() {
        let root = std::env::temp_dir().join(format!("catdesk-qwen-tools-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("src")).expect("src");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"qwen_fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .expect("cargo");
        fs::write(root.join("src/lib.rs"), "pub fn answer() -> u32 {\n    41\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn answer_is_42() {\n        assert_eq!(answer(), 42);\n    }\n}\n").expect("source");
        git(&root, &["init"]);
        git(&root, &["add", "."]);
        git(
            &root,
            &[
                "-c",
                "user.name=CatDesk",
                "-c",
                "user.email=catdesk@example.invalid",
                "commit",
                "-m",
                "fixture",
            ],
        );
        let contract = AutonomousDevelopmentContractV1 {
            schema_version: 1,
            contract_id: "qwen-proof".into(),
            task_id: "qwen-proof-task".into(),
            project_id: "catdesk".into(),
            mode: "chatgpt_web_codex_autonomous".into(),
            objective: "repair fixture".into(),
            workspace: root.clone(),
            base_branch: "main".into(),
            feature_branch: "feature-qwen".into(),
            base_commit: "base000".into(),
            expected_origin: "https://example.invalid/qwen.git".into(),
            ordered_steps: vec!["read patch verify diff".into()],
            allowed_paths: vec![root.clone()],
            forbidden_paths: vec![root.join(".git")],
            allowed_command_profiles: vec![AutonomousCommandProfileV1::CargoTest],
            git_policy: AutonomousGitPolicyV1 {
                create_branch: false,
                create_worktree: false,
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
                routine_model: "qwen-test".into(),
                allow_paid_fallback: false,
                allow_cloud_fallback: false,
            },
            verification_policy: AutonomousVerificationPolicyV1 {
                profile: "cargo".into(),
                required_commands: vec![AutonomousCommandProfileV1::CargoTest],
                max_repair_cycles: 1,
                require_authoritative_diff: true,
                require_final_review: true,
            },
            rate_limit_policy: AutonomousRateLimitPolicyV1 {
                automatic_pause: true,
                automatic_resume: true,
                initial_backoff_seconds: 1,
                maximum_backoff_seconds: 2,
                maximum_rate_limited_seconds: 10,
                honor_provider_retry_after: true,
            },
            autonomy_lease: AutonomyLeaseV1 {
                start_approval_required: false,
                renewal_allowed: false,
                issued_at_unix: 1,
                expires_at_unix: u64::MAX,
                maximum_total_elapsed_seconds: 120,
                maximum_provider_turns: 8,
                maximum_tool_calls: 8,
                maximum_consecutive_failures: 2,
                maximum_repair_cycles: 1,
            },
            hard_stop_conditions: vec![AutonomousHardStopV1::CancellationRequested],
            completion_artifact_ids: Vec::new(),
            task_graph: Vec::new(),
        };
        let policy = AutonomousPolicyEngineV1::new(contract).expect("policy");
        let execution_contract = execution_contract(&policy, "handoff-session", "work")
            .expect("workspace-root execution contract");
        assert_eq!(execution_contract.allowed_paths, ["."]);
        assert_eq!(execution_contract.forbidden_paths, [".git"]);
        let mut loop_ = AutonomousQwenToolLoopV1::recover(policy, "handoff-session", "work")
            .expect("tool loop");
        let names = loop_
            .tool_definitions()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert!(
            names.contains(&"read".into())
                && names.contains(&"patch.preview".into())
                && names.contains(&"verify.run".into())
        );
        assert!(!names.iter().any(|name| name.starts_with("job.")));
        loop_
            .execute(&call("qwen-read", "read", json!({"path":"src/lib.rs"})))
            .await
            .expect("read");
        assert!(
            loop_
                .execute(&call(
                    "qwen-read-forbidden",
                    "read",
                    json!({"path":".git/config"})
                ))
                .await
                .is_err(),
            "forbidden paths must remain rejected under workspace-root scope"
        );
        assert!(
            loop_
                .execute(&call(
                    "qwen-read-outside",
                    "read",
                    json!({"path":"../outside.txt"})
                ))
                .await
                .is_err(),
            "out-of-workspace paths must remain rejected under workspace-root scope"
        );
        loop_.execute(&call("qwen-preview", "patch.preview", json!({"patchId":"fix-answer", "operations":[{"path":"src/lib.rs", "old":"pub fn answer() -> u32 {\n    41\n}", "new":"pub fn answer() -> u32 {\n    42\n}"}]}))).await.expect("preview");
        loop_
            .execute(&call(
                "qwen-apply",
                "patch.apply",
                json!({"patchId":"fix-answer"}),
            ))
            .await
            .expect("apply");
        let verify = loop_
            .execute(&call("qwen-verify", "verify.run", json!({"timeout":30000})))
            .await
            .expect("verify");
        assert_eq!(verify.summary, "Passed", "{}", verify.bounded_text);
        loop_
            .execute(&call(
                "qwen-diff",
                "diff.actual",
                json!({"paths":["src/lib.rs"]}),
            ))
            .await
            .expect("diff");
        loop_
            .completion_ready("fixed and verified")
            .expect("completion gate");
        assert!(
            loop_
                .execute(&call(
                    "qwen-apply",
                    "patch.apply",
                    json!({"patchId":"fix-answer"})
                ))
                .await
                .is_err(),
            "completed mutation must not replay"
        );
    }
}
