//! Explicit, advisory context controls for the current paired Web target.
//! A project scope supplied in a request does not identify its ChatGPT caller.

use crate::{
    mcp,
    web_conversation_context::{
        self, ContextPressurePolicy, ConversationKey, Observation, VisibleSourceKind,
        WebContextStore,
    },
};
use catdesk_wake::{protocol::valid_id, store::now};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigureRequest {
    project_id: String,
    expected_target_generation: u64,
    expected_target_sha256: String,
    policy: PolicyInput,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyInput {
    configured_budget_tokens: u64,
    safety_headroom_tokens: u64,
    unknown_source_reserve_tokens: u64,
    automatic_rollover_enabled: bool,
    automatic_observation_enabled: bool,
    incomplete_history_rollover: bool,
}

impl PolicyInput {
    fn into_policy(self) -> ContextPressurePolicy {
        ContextPressurePolicy {
            configured_budget_tokens: self.configured_budget_tokens,
            safety_headroom_tokens: self.safety_headroom_tokens,
            unknown_source_reserve_tokens: self.unknown_source_reserve_tokens,
            automatic_rollover_enabled: self.automatic_rollover_enabled,
            automatic_observation_enabled: self.automatic_observation_enabled,
            incomplete_history_rollover: self.incomplete_history_rollover,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ObserveRequest {
    project_id: String,
    expected_target_generation: u64,
    expected_target_sha256: String,
    event: ContextEvent,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextEvent {
    event_id: String,
    revision: u64,
    dedup_key: String,
    source_kind: VisibleSourceKind,
    visible_text: Option<String>,
}

impl ContextEvent {
    fn observation(&self, observed_at: u64) -> Observation<'_> {
        Observation {
            event_id: &self.event_id,
            revision: self.revision,
            dedup_key: &self.dedup_key,
            source_kind: self.source_kind,
            visible_text: self.visible_text.as_deref(),
            observed_at_unix: observed_at,
        }
    }
}

fn validate_scope(project: &str, generation: u64, digest: &str) -> Result<(), String> {
    if project.len() > 128
        || !valid_id(project)
        || generation == 0
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("WEB_CONTEXT_ARGUMENTS_INVALID".into());
    }
    Ok(())
}

fn key(target: &mcp::ProjectWebTargetV1) -> ConversationKey {
    ConversationKey {
        project_id: target.project_id.clone(),
        conversation_url: target.url.clone(),
        target_generation: target.generation,
    }
}

pub(crate) fn configure(workspace: &Path, args: Value) -> Result<Value, String> {
    let request: ConfigureRequest =
        serde_json::from_value(args).map_err(|_| "WEB_CONTEXT_ARGUMENTS_INVALID")?;
    validate_scope(
        &request.project_id,
        request.expected_target_generation,
        &request.expected_target_sha256,
    )?;
    let policy = request.policy.into_policy();
    policy.validate()?;
    mcp::with_current_project_web_target(
        workspace,
        &request.project_id,
        request.expected_target_generation,
        &request.expected_target_sha256,
        |control, target| {
            let store = WebContextStore::open(control.join(".catdesk/web-context"))?;
            let key = key(target);
            let previous = store.active_key(&request.project_id)?;
            store.bind_observed_target(key.clone(), previous.as_ref())?;
            store.configure_policy(&request.project_id, policy)?;
            Ok(
                json!({"action":"web_context_configure","projectId":request.project_id,
            "pressure":store.pressure(&key)?,"advisory":true,"originAuthenticated":false,
            "pairedTargetVerified":true,"browserAction":"NONE","hostStarted":false}),
            )
        },
    )
}

pub(crate) fn observe(workspace: &Path, args: Value) -> Result<Value, String> {
    let request: ObserveRequest =
        serde_json::from_value(args).map_err(|_| "WEB_CONTEXT_ARGUMENTS_INVALID")?;
    validate_scope(
        &request.project_id,
        request.expected_target_generation,
        &request.expected_target_sha256,
    )?;
    if request
        .event
        .visible_text
        .as_ref()
        .is_some_and(|text| text.len() > 64 * 1024)
    {
        return Err("WEB_CONTEXT_ARGUMENTS_INVALID".into());
    }
    let observed_at = now();
    // Validate before opening state, including existing event/revision rules.
    web_conversation_context::validate_observation(request.event.observation(observed_at))?;
    mcp::with_current_project_web_target(
        workspace,
        &request.project_id,
        request.expected_target_generation,
        &request.expected_target_sha256,
        |control, target| {
            let store = WebContextStore::open(control.join(".catdesk/web-context"))?;
            let key = key(target);
            let previous = store.active_key(&request.project_id)?;
            store.bind_observed_target(key.clone(), previous.as_ref())?;
            let pressure =
                store.record_observation(&key, request.event.observation(observed_at))?;
            Ok(
                json!({"action":"web_context_observe","projectId":request.project_id,
            "eventId":request.event.event_id,"pressure":pressure,"advisory":true,
            "observationSource":"CALLER_REPORTED","originAuthenticated":false,
            "pairedTargetVerified":true,"rawTextPersisted":false,"browserAction":"NONE","hostStarted":false}),
            )
        },
    )
}

pub(crate) fn tool_schemas() -> [(&'static str, &'static str, &'static str, Value); 2] {
    [
        (
            "catdesk_web_context_configure",
            "Configure one project's context policy",
            "Set the existing advisory context policy for the exact paired project generation and URL digest from registry Web status. Automatic observation and rollover are explicit opt-ins affecting later work by an already authorized running worker. The operation starts no browser or host and does not identify the calling ChatGPT conversation.",
            json!({"type":"object","additionalProperties":false,"properties":{
            "projectId":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,128}$"},
            "expectedTargetGeneration":{"type":"integer","minimum":1},
            "expectedTargetSha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
            "policy":{"type":"object","additionalProperties":false,"properties":{
                "configuredBudgetTokens":{"type":"integer","minimum":1,"maximum":2000000},
                "safetyHeadroomTokens":{"type":"integer","minimum":0,"description":"Must be smaller than the configured budget"},
                "unknownSourceReserveTokens":{"type":"integer","minimum":0,"description":"Must not exceed the configured budget"},
                "automaticRolloverEnabled":{"type":"boolean"},
                "automaticObservationEnabled":{"type":"boolean"},
                "incompleteHistoryRollover":{"type":"boolean"}},
                "required":["configuredBudgetTokens","safetyHeadroomTokens","unknownSourceReserveTokens","automaticRolloverEnabled","automaticObservationEnabled","incompleteHistoryRollover"]}},
            "required":["projectId","expectedTargetGeneration","expectedTargetSha256","policy"]}),
        ),
        (
            "catdesk_web_context_observe",
            "Record one advisory context observation",
            "Record bounded caller-reported visible text or unknown-attachment metadata for the exact paired project conversation. Use stable event/dedup IDs and revisions for retries and streaming updates. Only hashes and token counts persist. Scope is explicit caller-reported data, not authenticated ChatGPT origin; no global traffic attribution, browser capture, or delivery authority is created.",
            json!({"type":"object","additionalProperties":false,"properties":{
            "projectId":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,128}$"},
            "expectedTargetGeneration":{"type":"integer","minimum":1},
            "expectedTargetSha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
            "event":{"type":"object","additionalProperties":false,"properties":{
                "eventId":{"type":"string","pattern":"^[A-Za-z0-9_.:-]{1,200}$"},
                "revision":{"type":"integer","minimum":1},
                "dedupKey":{"type":"string","pattern":"^[A-Za-z0-9_.:-]{1,200}$"},
                "sourceKind":{"type":"string","enum":["USER_MESSAGE","ASSISTANT_MESSAGE","TOOL_RESULT","TOOL_ARGUMENTS","HANDOFF","UNKNOWN_ATTACHMENT"]},
                "visibleText":{"type":["string","null"],"maxLength":65536,"description":"UTF-8 bytes also limited to 65536; null only for UNKNOWN_ATTACHMENT"}},
                "required":["eventId","revision","dedupKey","sourceKind","visibleText"]}},
            "required":["projectId","expectedTargetGeneration","expectedTargetSha256","event"]}),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_controls_reject_invalid_payloads_before_state_creation() {
        let root =
            std::env::temp_dir().join(format!("web-context-invalid-{}", uuid::Uuid::new_v4()));
        let valid = json!({"projectId":"alpha","expectedTargetGeneration":1,
            "expectedTargetSha256":"a".repeat(64),"policy":ContextPressurePolicy::default()});
        for (field, value) in [
            ("workspace", json!("C:/private")),
            ("conversationUrl", json!("https://chatgpt.com/c/other")),
            ("projectId", json!("../other")),
            ("expectedTargetGeneration", json!(0)),
            ("expectedTargetSha256", json!("A".repeat(64))),
        ] {
            let mut args = valid.clone();
            args[field] = value;
            assert!(configure(&root, args).is_err());
            assert!(!root.exists());
        }
        let mut invalid_policy = valid.clone();
        invalid_policy["policy"]["safetyHeadroomTokens"] = json!(96000);
        assert!(configure(&root, invalid_policy).is_err());
        let mut unknown_policy = valid.clone();
        unknown_policy["policy"]["conversationUrl"] = json!("C:/private-secret");
        assert!(configure(&root, unknown_policy).is_err());
        let event = json!({"eventId":"tool-1","revision":1,"dedupKey":"tool-1","sourceKind":"TOOL_RESULT","visibleText":"private text"});
        for (field, value) in [
            ("eventId", json!("../other")),
            ("revision", json!(0)),
            ("sourceKind", json!("OTHER")),
            ("visibleText", json!(null)),
            ("visibleText", json!("é".repeat(32769))),
            ("observedAtUnix", json!(100)),
        ] {
            let mut args = json!({"projectId":"alpha","expectedTargetGeneration":1,"expectedTargetSha256":"a".repeat(64),"event":event.clone()});
            args["event"][field] = value;
            assert!(observe(&root, args).is_err());
            assert!(!root.exists());
        }
    }
}
