use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::context::{
    CheckpointSummaryV1, DisclosurePolicy, ProviderHandoffContextV1,
    provider_handoff_from_checkpoint,
};
use super::contracts::EscalationPacketV1;
use super::coordinator::RunCoordinator;
use super::journal::{ToolCallRecordV1, ToolCallStatus};
use super::runtime::{ProviderCapabilitiesV1, ProviderType};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderAvailabilityV1 {
    Available,
    Unavailable,
    RateLimited,
    TimedOut,
    MalformedOutput,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigV1 {
    pub provider_id: String,
    pub capabilities: ProviderCapabilitiesV1,
    pub availability: ProviderAvailabilityV1,
    pub credential_env_var: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRoutingPolicyV1 {
    pub primary_provider_id: String,
    pub fallback_provider_ids: Vec<String>,
    pub disclosure_policy: DisclosurePolicy,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProviderRoutingError {
    MissingProvider(String),
    TransitionNotAllowed(String),
    RemoteDisclosureNotAllowed(String),
    AllProvidersUnavailable(Box<EscalationPacketV1>),
}

#[derive(Default)]
pub struct ProviderRegistryV1 {
    providers: BTreeMap<String, ProviderConfigV1>,
}

impl ProviderRegistryV1 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, provider: ProviderConfigV1) {
        self.providers
            .insert(provider.provider_id.clone(), provider);
    }

    pub fn select_replacement(
        &self,
        policy: &ProviderRoutingPolicyV1,
        current_provider_id: &str,
        checkpoint: CheckpointSummaryV1,
        tool_calls: &[ToolCallRecordV1],
    ) -> Result<ProviderHandoffContextV1, ProviderRoutingError> {
        let mut candidates = Vec::new();
        candidates.push(policy.primary_provider_id.clone());
        candidates.extend(policy.fallback_provider_ids.clone());
        candidates.retain(|provider| provider != current_provider_id);

        for candidate in candidates {
            let provider = self
                .providers
                .get(&candidate)
                .ok_or_else(|| ProviderRoutingError::MissingProvider(candidate.clone()))?;
            if provider.availability != ProviderAvailabilityV1::Available {
                continue;
            }
            validate_transition(policy, current_provider_id, provider)?;
            let pending_tool_call_ids = tool_calls
                .iter()
                .filter(|record| {
                    !matches!(
                        record.status,
                        ToolCallStatus::Completed | ToolCallStatus::Failed
                    )
                })
                .map(|record| record.tool_call_id.clone())
                .collect();
            return Ok(provider_handoff_from_checkpoint(
                current_provider_id,
                &candidate,
                checkpoint,
                Vec::new(),
                pending_tool_call_ids,
                policy.disclosure_policy.clone(),
            ));
        }

        let coordinator = RunCoordinator::new();
        Err(ProviderRoutingError::AllProvidersUnavailable(Box::new(
            coordinator.escalation_packet(
                checkpoint.run_id,
                "No provider is currently available.",
                "all providers unavailable",
                self.providers
                    .values()
                    .map(|provider| {
                        format!("{}: {:?}", provider.provider_id, provider.availability)
                    })
                    .collect(),
                vec![format!("tried replacement after {current_provider_id}")],
                "pause and ask supervisor",
            ),
        )))
    }

    pub fn redacted_provider_log(&self) -> Vec<String> {
        self.providers
            .values()
            .map(|provider| {
                format!(
                    "{} {:?} credential_env_var={}",
                    provider.provider_id,
                    provider.availability,
                    provider.credential_env_var.as_deref().unwrap_or("<none>")
                )
            })
            .collect()
    }
}

fn validate_transition(
    policy: &ProviderRoutingPolicyV1,
    from_provider_id: &str,
    to: &ProviderConfigV1,
) -> Result<(), ProviderRoutingError> {
    if from_provider_id == to.provider_id {
        return Err(ProviderRoutingError::TransitionNotAllowed(
            "provider transition cannot target itself".into(),
        ));
    }
    if matches!(
        to.capabilities.provider_type,
        ProviderType::RemoteApi | ProviderType::Browser
    ) && policy.disclosure_policy != DisclosurePolicy::RemoteAllowed
    {
        return Err(ProviderRoutingError::RemoteDisclosureNotAllowed(
            to.provider_id.clone(),
        ));
    }
    Ok(())
}

pub fn fake_provider_config(
    provider_id: &str,
    provider_type: ProviderType,
    availability: ProviderAvailabilityV1,
) -> ProviderConfigV1 {
    ProviderConfigV1 {
        provider_id: provider_id.into(),
        capabilities: ProviderCapabilitiesV1 {
            provider_id: provider_id.into(),
            provider_type,
            supports_streaming: true,
            supports_native_tool_calls: true,
            supports_session_continuity: true,
            supports_cancellation: true,
            context_limit: 64 * 1024,
            output_limit: 16 * 1024,
        },
        availability,
        credential_env_var: Some(format!("{}_API_KEY", provider_id.to_ascii_uppercase())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegated::context::CheckpointSummaryV1;
    use crate::delegated::contracts::{RunId, ToolCallId};
    use crate::delegated::journal::ToolMutationKind;

    fn checkpoint() -> CheckpointSummaryV1 {
        CheckpointSummaryV1 {
            run_id: RunId::new("run-routing").expect("run id"),
            objective: "route provider".into(),
            allowed_paths: vec!["src".into()],
            forbidden_paths: vec![".git".into()],
            acceptance_criteria: vec!["handoff succeeds".into()],
            current_step: "provider switch".into(),
            latest_checkpoint: "ollama unavailable".into(),
            unresolved_failure: Some("rate limit".into()),
            remaining_turns: 3,
            remaining_tool_calls: 4,
            recent_artifact_ids: vec![],
        }
    }

    fn policy(disclosure_policy: DisclosurePolicy) -> ProviderRoutingPolicyV1 {
        ProviderRoutingPolicyV1 {
            primary_provider_id: "ollama".into(),
            fallback_provider_ids: vec!["fake-api".into(), "fake-browser".into()],
            disclosure_policy,
        }
    }

    fn tool_record(id: &str, status: ToolCallStatus) -> ToolCallRecordV1 {
        ToolCallRecordV1 {
            schema_version: 1,
            run_id: RunId::new("run-routing").expect("run id"),
            tool_call_id: ToolCallId::new(id).expect("tool id"),
            tool_name: "read".into(),
            request_hash: "fnv1a64:req".into(),
            arguments_hash: "fnv1a64:args".into(),
            mutation_kind: ToolMutationKind::ReadOnly,
            status,
            result_hash: None,
            outcome_summary: None,
        }
    }

    #[test]
    fn simulated_provider_replacement_succeeds() {
        let mut registry = ProviderRegistryV1::new();
        registry.register(fake_provider_config(
            "ollama",
            ProviderType::LocalApi,
            ProviderAvailabilityV1::Unavailable,
        ));
        registry.register(fake_provider_config(
            "fake-api",
            ProviderType::RemoteApi,
            ProviderAvailabilityV1::Available,
        ));
        let handoff = registry
            .select_replacement(
                &policy(DisclosurePolicy::RemoteAllowed),
                "ollama",
                checkpoint(),
                &[],
            )
            .expect("handoff");
        assert_eq!(handoff.from_provider_id, "ollama");
        assert_eq!(handoff.to_provider_id, "fake-api");
    }

    #[test]
    fn completed_tool_calls_are_not_repeated_during_switch() {
        let mut registry = ProviderRegistryV1::new();
        registry.register(fake_provider_config(
            "fake-api",
            ProviderType::RemoteApi,
            ProviderAvailabilityV1::Available,
        ));
        let handoff = registry
            .select_replacement(
                &policy(DisclosurePolicy::RemoteAllowed),
                "ollama",
                checkpoint(),
                &[
                    tool_record("tc-complete", ToolCallStatus::Completed),
                    tool_record("tc-pending", ToolCallStatus::Executing),
                ],
            )
            .expect("handoff");
        assert_eq!(handoff.pending_tool_call_ids.len(), 1);
        assert_eq!(handoff.pending_tool_call_ids[0].as_str(), "tc-pending");
    }

    #[test]
    fn all_providers_unavailable_escalates() {
        let mut registry = ProviderRegistryV1::new();
        registry.register(fake_provider_config(
            "fake-api",
            ProviderType::RemoteApi,
            ProviderAvailabilityV1::RateLimited,
        ));
        registry.register(fake_provider_config(
            "fake-browser",
            ProviderType::Browser,
            ProviderAvailabilityV1::Unavailable,
        ));
        let result = registry.select_replacement(
            &policy(DisclosurePolicy::RemoteAllowed),
            "ollama",
            checkpoint(),
            &[],
        );
        assert!(matches!(
            result,
            Err(ProviderRoutingError::AllProvidersUnavailable(packet))
                if packet.blocking_condition == "all providers unavailable"
        ));
    }

    #[test]
    fn browser_state_does_not_leak_into_handoff_schema() {
        let handoff = provider_handoff_from_checkpoint(
            "ollama",
            "fake-browser",
            checkpoint(),
            Vec::new(),
            Vec::new(),
            DisclosurePolicy::RemoteAllowed,
        );
        let json = serde_json::to_string(&handoff).expect("serialize");
        assert!(!json.contains("cookie"));
        assert!(!json.contains("selector"));
        assert!(!json.contains("browserProfile"));
    }

    #[test]
    fn remote_or_browser_transition_requires_disclosure_policy() {
        let mut registry = ProviderRegistryV1::new();
        registry.register(fake_provider_config(
            "fake-api",
            ProviderType::RemoteApi,
            ProviderAvailabilityV1::Unavailable,
        ));
        registry.register(fake_provider_config(
            "fake-browser",
            ProviderType::Browser,
            ProviderAvailabilityV1::Available,
        ));
        let result = registry.select_replacement(
            &policy(DisclosurePolicy::LocalOnly),
            "ollama",
            checkpoint(),
            &[],
        );
        assert!(matches!(
            result,
            Err(ProviderRoutingError::RemoteDisclosureNotAllowed(provider))
                if provider == "fake-browser"
        ));
    }

    #[test]
    fn secrets_remain_as_env_var_names_only() {
        let mut registry = ProviderRegistryV1::new();
        registry.register(fake_provider_config(
            "fake-api",
            ProviderType::RemoteApi,
            ProviderAvailabilityV1::Available,
        ));
        let log = registry.redacted_provider_log().join("\n");
        assert!(log.contains("FAKE-API_API_KEY"));
        assert!(!log.contains("sk-"));
        assert!(!log.contains("secret-value"));
    }
}
