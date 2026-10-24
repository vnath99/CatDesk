//! Private browser excerpt ingestion. Public output contains counts only.
use crate::web_conversation_context::{
    BrowserCoverageObservation, ConversationKey, PressureStatus, VisibleSourceKind, WebContextStore,
};
use catdesk_wake::protocol::digest;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    messages: Vec<Message>,
    truncated: bool,
    coverage: Coverage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Coverage {
    selector_tier: String,
    visible_role_count: u64,
    count_overflow: bool,
    nested_collapsed: u64,
    keyed_collapsed: u64,
    omitted_before_window: u64,
    returned: u64,
    identity_missing: u64,
    identity_uncertain: u64,
    revision_incomplete: u64,
    text_incomplete: u64,
    anchor_stability: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Message {
    role: String,
    text: String,
    visible_units: u64,
    text_complete: bool,
    /// New adapters distinguish real clipped body text from a controls-only
    /// diagnostic fallback. Older incomplete snapshots remain uncounted.
    #[serde(default)]
    body_text_observed: bool,
    turn_hash: Option<String>,
    role_turn_hash: Option<String>,
    message_hash: Option<String>,
    identity_basis: String,
    revision_hash: Option<String>,
    revision_complete: bool,
}

fn sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|v| v.is_ascii_hexdigit() && !v.is_ascii_uppercase())
}

fn recorded_or_at_limit(
    result: Result<PressureStatus, String>,
    uncounted: &mut usize,
) -> Result<bool, String> {
    match result {
        Ok(_) => Ok(true),
        Err(reason) if reason == "WEB_CONTEXT_EVENT_LIMIT" => {
            // Retain every existing event/revision and disclose the accounting
            // gap. An already-due policy can still request a guarded handoff;
            // saturation itself is never treated as token-pressure evidence.
            *uncounted += 1;
            Ok(false)
        }
        Err(reason) => Err(reason),
    }
}

pub(super) fn ingest(
    store: &WebContextStore,
    key: &ConversationKey,
    value: Value,
    observed_at: u64,
    mut confirm_target: impl FnMut() -> Result<(), String>,
) -> Result<Value, String> {
    let snapshot: Snapshot = serde_json::from_value(value).map_err(|_| "WEB_SNAPSHOT_INVALID")?;
    let coverage = &snapshot.coverage;
    if snapshot.messages.len() > 16
        || !matches!(
            coverage.selector_tier.as_str(),
            "AUTHOR_ROLE" | "ARTICLE_ROLE" | "SEMANTIC_UNIT" | "NONE"
        )
        || (coverage.selector_tier == "NONE" && !snapshot.messages.is_empty())
        || coverage.anchor_stability != "UNVERIFIED"
        || coverage.visible_role_count > 1024
        || coverage.nested_collapsed > 1024
        || coverage.keyed_collapsed > 1024
        || (coverage.count_overflow && coverage.visible_role_count != 1024)
        || coverage.returned != snapshot.messages.len() as u64
        || coverage.visible_role_count < coverage.returned
        || coverage.omitted_before_window != coverage.visible_role_count - coverage.returned
        || (coverage.count_overflow
            || coverage.omitted_before_window > 0
            || coverage.text_incomplete > 0)
            && !snapshot.truncated
    {
        return Err("WEB_SNAPSHOT_INVALID".into());
    }
    // Validate the whole bounded response before mutating the advisory ledger.
    let mut ids = BTreeSet::new();
    for message in &snapshot.messages {
        if !matches!(message.role.as_str(), "USER" | "ASSISTANT")
            || !matches!(
                message.identity_basis.as_str(),
                "UNIT_KEY" | "MESSAGE_ID" | "ROLE_TURN_ONLY" | "MISSING"
            )
            || message.text.len() > 8192
            || message.visible_units > 1_000_000
            || [
                &message.turn_hash,
                &message.role_turn_hash,
                &message.message_hash,
                &message.revision_hash,
            ]
            .iter()
            .any(|hash| hash.as_ref().is_some_and(|hash| !sha(hash)))
            || message
                .message_hash
                .as_ref()
                .is_some_and(|hash| !ids.insert(hash.clone()))
            || (matches!(message.identity_basis.as_str(), "UNIT_KEY" | "MESSAGE_ID")
                != message.message_hash.is_some())
            || (message.revision_complete != message.revision_hash.is_some())
        {
            return Err("WEB_SNAPSHOT_INVALID".into());
        }
        if message.text_complete && message.revision_complete {
            let expected = digest(&format!(
                "catdesk-snapshot-revision-v1\0{}\0{}",
                key.target_sha256(),
                message.text
            ));
            if message.revision_hash.as_deref() != Some(expected.as_str()) {
                return Err("WEB_SNAPSHOT_REVISION_INVALID".into());
            }
        }
    }
    let identity_missing = snapshot
        .messages
        .iter()
        .filter(|message| message.identity_basis == "MISSING")
        .count() as u64;
    let identity_uncertain = snapshot
        .messages
        .iter()
        .filter(|message| message.identity_basis == "ROLE_TURN_ONLY")
        .count() as u64;
    let revision_incomplete = snapshot
        .messages
        .iter()
        .filter(|message| !message.revision_complete)
        .count() as u64;
    let text_incomplete = snapshot
        .messages
        .iter()
        .filter(|message| !message.text_complete)
        .count() as u64;
    if coverage.identity_missing != identity_missing
        || coverage.identity_uncertain != identity_uncertain
        || coverage.revision_incomplete != revision_incomplete
        || coverage.text_incomplete != text_incomplete
    {
        return Err("WEB_SNAPSHOT_INVALID".into());
    }
    let mut recorded = 0;
    let mut excerpts = 0;
    let mut uncertain = 0;
    let mut uncounted_at_limit = 0;
    confirm_target()?;
    for message in &snapshot.messages {
        confirm_target()?;
        let normalized = message
            .text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .replace(['’', '‘'], "'")
            .to_lowercase();
        let load_error = normalized.contains("this response couldn't load");
        let complete = message.text_complete
            && message.revision_complete
            && !normalized.is_empty()
            && !load_error
            && matches!(message.identity_basis.as_str(), "UNIT_KEY" | "MESSAGE_ID");
        if let Some(id) = message.message_hash.as_ref().filter(|_| complete) {
            let role = if message.role == "USER" {
                VisibleSourceKind::UserMessage
            } else {
                VisibleSourceKind::AssistantMessage
            };
            if recorded_or_at_limit(
                store.record_browser_observation(
                    key,
                    &format!("browser-{id}"),
                    role,
                    Some(&message.text),
                    observed_at,
                ),
                &mut uncounted_at_limit,
            )? {
                recorded += 1;
            }
        } else {
            uncertain += 1;
            if message.body_text_observed
                && !normalized.is_empty()
                && !load_error
                && let Some(id) = &message.message_hash
            {
                let role = if message.role == "USER" {
                    VisibleSourceKind::UserMessage
                } else {
                    VisibleSourceKind::AssistantMessage
                };
                if recorded_or_at_limit(
                    store.record_browser_excerpt_observation(
                        key,
                        &format!("browser-{id}"),
                        role,
                        &message.text,
                        observed_at,
                    ),
                    &mut uncounted_at_limit,
                )? {
                    excerpts += 1;
                }
            }
            // Coverage stays uncertain. Missing/ambiguous identity, UI-only
            // fallback and load errors never count as message body content.
        }
    }
    confirm_target()?;
    let pressure = store.record_browser_coverage(
        key,
        BrowserCoverageObservation {
            observed_at_unix: observed_at,
            initial_history_gap: true,
            identity_gap: identity_missing > 0
                || identity_uncertain > 0
                || coverage.nested_collapsed > 0
                || coverage.keyed_collapsed > 0,
            text_gap: text_incomplete > 0
                || revision_incomplete > 0
                || uncertain > 0
                || uncounted_at_limit > 0,
            overflow_gap: snapshot.truncated
                || coverage.count_overflow
                || coverage.omitted_before_window > 0
                || uncounted_at_limit > 0,
        },
    )?;
    Ok(
        json!({"action":"web_context_capture","projectId":key.project_id,
        "recordedMessages":recorded,"excerptMessages":excerpts,"uncertainMessages":uncertain,
        "uncountedMessagesAtLimit":uncounted_at_limit,"observationLimitReached":uncounted_at_limit>0,
        "excerptTruncated":snapshot.truncated,"anchorStability":"UNVERIFIED",
        "coverage":"RECENT_VISIBLE_EXCERPT_WITH_UNKNOWN_HISTORY",
        "pressure":pressure,"advisory":true,"originAuthenticated":false,
        "browserBindingVerified":true,"rawTextPersisted":false,"userMessageSent":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pressure_at(capture: &Value, observed_at: u64) -> Value {
        let mut pressure = capture["pressure"].clone();
        pressure["browserCoverage"]["lastObservedAtUnix"] = json!(observed_at);
        pressure
    }
    fn input(key: &ConversationKey, text: &str, complete: bool) -> Value {
        json!({"messages":[{"role":"ASSISTANT","text":text,"visibleUnits":text.len(),
            "textComplete":complete,"turnHash":null,"roleTurnHash":null,
            "messageHash":digest("stable-message"),"identityBasis":"MESSAGE_ID",
            "revisionHash":digest(&format!("catdesk-snapshot-revision-v1\0{}\0{}",key.target_sha256(),text)),
            "revisionComplete":true}],"truncated":!complete,
            "coverage":{"selectorTier":"AUTHOR_ROLE","visibleRoleCount":1,
                "countOverflow":false,"nestedCollapsed":0,"keyedCollapsed":0,
                "omittedBeforeWindow":0,"returned":1,"identityMissing":0,
                "identityUncertain":0,"revisionIncomplete":0,
                "textIncomplete":u64::from(!complete),"anchorStability":"UNVERIFIED"}})
    }

    fn excerpt(key: &ConversationKey, text: &str) -> Value {
        let mut value = input(key, text, false);
        value["messages"][0]["bodyTextObserved"] = json!(true);
        value["messages"][0]["visibleUnits"] = json!(20_000);
        value["messages"][0]["revisionComplete"] = json!(false);
        value["messages"][0]["revisionHash"] = Value::Null;
        value["coverage"]["revisionIncomplete"] = json!(1);
        value
    }

    fn fill_observation_ledger(store: &WebContextStore, key: &ConversationKey, count: usize) {
        use crate::web_conversation_context::Observation;
        let ids: Vec<_> = (0..count).map(|n| format!("bounded-event-{n}")).collect();
        for batch in ids.chunks(512) {
            store
                .record_observation_batch(
                    key,
                    batch.iter().map(|id| Observation {
                        event_id: id,
                        revision: 1,
                        dedup_key: id,
                        source_kind: VisibleSourceKind::ToolResult,
                        visible_text: Some("x"),
                        observed_at_unix: 1,
                    }),
                )
                .unwrap();
        }
    }

    #[test]
    fn saturated_snapshot_preserves_known_revisions_and_due_handoff_readiness() {
        use crate::web_conversation_context::ContextPressurePolicy;
        let root = std::env::temp_dir().join(format!("web-observe-limit-{}", uuid::Uuid::new_v4()));
        let store = WebContextStore::open(&root).unwrap();
        let key = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: "https://chatgpt.com/c/limit-alpha".into(),
            target_generation: 1,
        };
        store.bind_observed_target(key.clone(), None).unwrap();
        store
            .configure_policy(
                "alpha",
                ContextPressurePolicy {
                    configured_budget_tokens: 6000,
                    safety_headroom_tokens: 2000,
                    unknown_source_reserve_tokens: 8,
                    automatic_rollover_enabled: true,
                    ..Default::default()
                },
            )
            .unwrap();
        ingest(&store, &key, input(&key, "original body", true), 1, || {
            Ok(())
        })
        .unwrap();
        fill_observation_ledger(&store, &key, 4095);
        let before = store.pressure(&key).unwrap();
        assert_eq!(before.recorded_events, 4096);
        assert!(before.near_capacity && store.rollover_readiness(&key).unwrap().1);
        let mut capture = input(&key, "known body updated at the limit", true);
        let known = capture["messages"][0].clone();
        let mut new = input(
            &key,
            "unrecorded new body must remain explicitly unknown",
            true,
        )["messages"][0]
            .clone();
        new["messageHash"] = json!(digest("new-message-at-limit"));
        capture["messages"] = json!([new, known]);
        capture["coverage"]["returned"] = json!(2);
        capture["coverage"]["visibleRoleCount"] = json!(2);
        let result = ingest(&store, &key, capture.clone(), 2, || Ok(()))
            .expect("saturated capture must retain due handoff readiness");
        assert_eq!(result["recordedMessages"], 1);
        assert_eq!(result["uncountedMessagesAtLimit"], 1);
        assert_eq!(result["observationLimitReached"], true);
        assert_eq!(result["pressure"]["recordedEvents"], 4096);
        assert_eq!(result["pressure"]["observationLimitReached"], true);
        assert_eq!(result["pressure"]["browserCoverage"]["overflowGap"], true);
        assert_eq!(result["pressure"]["browserCoverage"]["textGap"], true);
        assert_eq!(
            result["pressure"]["pressureAssessment"],
            "UNKNOWN_OR_INCOMPLETE"
        );
        let retained = store.pressure(&key).unwrap();
        assert!(retained.visible_unique_tokens > before.visible_unique_tokens);
        let replay = ingest(&store, &key, capture.clone(), 3, || Ok(())).unwrap();
        assert_eq!(
            replay["pressure"]["visibleUniqueTokens"],
            result["pressure"]["visibleUniqueTokens"]
        );
        assert_eq!(
            store.pressure(&key).unwrap().last_observed_event_at_unix,
            Some(2)
        );
        assert_eq!(
            ingest(&store, &key, capture, 4, || Err("TARGET_CHANGED".into())).unwrap_err(),
            "TARGET_CHANGED"
        );
        assert_eq!(
            store
                .pressure(&key)
                .unwrap()
                .browser_coverage
                .last_observed_at_unix,
            Some(3)
        );
        assert!(
            store
                .request_rollover_if_due(&key, &key.target_sha256(), "source-task", false, 4)
                .unwrap()
                .is_none()
        );
        let mut clipped = excerpt(&key, "unrecorded clipped body at the metadata limit");
        clipped["messages"][0]["messageHash"] = json!(digest("new-excerpt-at-limit"));
        let uncounted_excerpt = ingest(&store, &key, clipped, 4, || Ok(())).unwrap();
        assert_eq!(uncounted_excerpt["excerptMessages"], 0);
        assert_eq!(uncounted_excerpt["uncountedMessagesAtLimit"], 1);
        assert_eq!(
            uncounted_excerpt["pressure"]["visibleUniqueTokens"],
            result["pressure"]["visibleUniqueTokens"]
        );
        let intent = store
            .request_rollover_if_due(&key, &key.target_sha256(), "source-task", true, 4)
            .unwrap()
            .unwrap();
        assert_eq!(intent.trigger_reason, "OBSERVED_PRESSURE");
        let reader = WebContextStore::open_read_only(&root).unwrap();
        assert_eq!(
            reader.pressure(&key).unwrap().rollover_intent,
            Some(intent.clone())
        );
        assert_eq!(
            store
                .request_rollover_if_due(&key, &key.target_sha256(), "source-task", true, 5)
                .unwrap(),
            Some(intent)
        );
    }

    #[test]
    fn saturated_low_pressure_never_invents_tokens_or_blocks_sibling_capture() {
        use crate::web_conversation_context::ContextPressurePolicy;
        let root = std::env::temp_dir().join(format!(
            "web-observe-limit-sibling-{}",
            uuid::Uuid::new_v4()
        ));
        let store = WebContextStore::open(&root).unwrap();
        let alpha = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: "https://chatgpt.com/c/limit-low-alpha".into(),
            target_generation: 1,
        };
        let beta = ConversationKey {
            project_id: "beta".into(),
            conversation_url: "https://chatgpt.com/c/limit-low-beta".into(),
            target_generation: 1,
        };
        for key in [&alpha, &beta] {
            store.bind_observed_target(key.clone(), None).unwrap();
            store
                .configure_policy(
                    &key.project_id,
                    ContextPressurePolicy {
                        automatic_rollover_enabled: true,
                        incomplete_history_rollover: false,
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        fill_observation_ledger(&store, &alpha, 4096);
        let before = store.pressure(&alpha).unwrap();
        assert!(!before.near_capacity);
        assert!(before.observation_limit_reached);
        let result = ingest(
            &store,
            &alpha,
            input(&alpha, "not recorded due to metadata limit", true),
            2,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(result["recordedMessages"], 0);
        assert_eq!(result["uncountedMessagesAtLimit"], 1);
        assert_eq!(
            result["pressure"]["visibleUniqueTokens"],
            before.visible_unique_tokens
        );
        assert_eq!(
            result["pressure"]["estimatedUpperTokens"],
            before.estimated_upper_tokens
        );
        assert!(!store.rollover_readiness(&alpha).unwrap().1);
        assert!(
            store
                .request_rollover_if_due(&alpha, &alpha.target_sha256(), "source-task", true, 3)
                .unwrap()
                .is_none()
        );
        let sibling = ingest(
            &store,
            &beta,
            input(&beta, "sibling independently makes progress", true),
            3,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(sibling["recordedMessages"], 1);
        assert_eq!(sibling["uncountedMessagesAtLimit"], 0);
        assert_eq!(sibling["observationLimitReached"], false);
        assert_eq!(sibling["pressure"]["recordedEvents"], 1);
        assert!(sibling["pressure"].get("observationLimitReached").is_none());
        assert_eq!(sibling["pressure"]["browserCoverage"]["overflowGap"], false);
        assert_eq!(store.pressure(&alpha).unwrap().recorded_events, 4096);
        assert!(
            WebContextStore::open_read_only(&root)
                .unwrap()
                .pressure(&beta)
                .unwrap()
                .visible_unique_tokens
                > 0
        );
    }

    #[test]
    fn clipped_body_counts_once_without_becoming_complete_history() {
        let root = std::env::temp_dir().join(format!("web-excerpt-{}", uuid::Uuid::new_v4()));
        let store = WebContextStore::open(&root).unwrap();
        let key = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: "https://chatgpt.com/c/excerpt".into(),
            target_generation: 1,
        };
        store.bind_observed_target(key.clone(), None).unwrap();
        let small = "private clipped body ".repeat(12);
        let first = ingest(&store, &key, excerpt(&key, &small), 1, || Ok(())).unwrap();
        assert_eq!(first["recordedMessages"], 0);
        assert_eq!(first["excerptMessages"], 1);
        assert_eq!(first["uncertainMessages"], 1);
        let first_count = first["pressure"]["visibleUniqueTokens"].as_u64().unwrap();
        assert!(first_count > 0);
        assert_eq!(first["pressure"]["recordedEvents"], 1);
        assert_eq!(first["pressure"]["excerptOnlyMessages"], 1);
        assert_eq!(
            first["pressure"]["pressureAssessment"],
            "UNKNOWN_OR_INCOMPLETE"
        );
        assert_eq!(first["pressure"]["browserCoverage"]["textGap"], true);
        let replay = ingest(&store, &key, excerpt(&key, &small), 2, || Ok(())).unwrap();
        assert_eq!(pressure_at(&first, 2), replay["pressure"]);
        let changed_tail = "another clipped tail ".repeat(2);
        let smaller = ingest(&store, &key, excerpt(&key, &changed_tail), 3, || Ok(())).unwrap();
        assert_eq!(pressure_at(&first, 3), smaller["pressure"]);
        let large = "private clipped body ".repeat(60);
        let grown = ingest(&store, &key, excerpt(&key, &large), 4, || Ok(())).unwrap();
        assert!(grown["pressure"]["visibleUniqueTokens"].as_u64().unwrap() > first_count);
        assert_eq!(grown["pressure"]["recordedEvents"], 1);
        let reopened = WebContextStore::open_read_only(&root).unwrap();
        assert_eq!(
            reopened.pressure(&key).unwrap(),
            serde_json::from_value(grown["pressure"].clone()).unwrap()
        );
        let complete = ingest(
            &store,
            &key,
            input(&key, "final edited answer", true),
            5,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(complete["pressure"]["excerptOnlyMessages"], 0);
        assert_eq!(complete["pressure"]["recordedEvents"], 1);
        assert!(
            complete["pressure"]["visibleUniqueTokens"]
                .as_u64()
                .unwrap()
                < first_count
        );
        assert_eq!(complete["pressure"]["browserCoverage"]["textGap"], true);
        let clipped_again = ingest(&store, &key, excerpt(&key, "tail"), 6, || Ok(())).unwrap();
        assert_eq!(pressure_at(&complete, 6), clipped_again["pressure"]);
        // Neither the response nor the on-disk metadata retains body text.
        for file in std::fs::read_dir(root.join("conversations")).unwrap() {
            let text = std::fs::read_to_string(file.unwrap().path()).unwrap();
            assert!(!text.contains("private clipped body"));
            assert!(!text.contains("final edited answer"));
        }
        assert!(!grown.to_string().contains("private clipped body"));
    }

    #[test]
    fn only_identified_body_excerpts_count_and_target_change_stops_ingestion() {
        let root = std::env::temp_dir().join(format!("web-excerpt-veto-{}", uuid::Uuid::new_v4()));
        let store = WebContextStore::open(&root).unwrap();
        let key = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: "https://chatgpt.com/c/excerpt-veto".into(),
            target_generation: 1,
        };
        store.bind_observed_target(key.clone(), None).unwrap();
        for mut value in [
            excerpt(&key, "controls diagnostic"),
            excerpt(&key, "unidentified body"),
            excerpt(&key, "This response couldn’t load"),
            excerpt(&key, " \n\t"),
        ] {
            if value["messages"][0]["text"] == "controls diagnostic" {
                value["messages"][0]["bodyTextObserved"] = json!(false);
            }
            if value["messages"][0]["text"] == "unidentified body" {
                value["messages"][0]["identityBasis"] = json!("ROLE_TURN_ONLY");
                value["messages"][0]["messageHash"] = Value::Null;
                value["coverage"]["identityUncertain"] = json!(1);
            }
            let result = ingest(&store, &key, value, 1, || Ok(())).unwrap();
            assert_eq!(result["pressure"]["visibleUniqueTokens"], 0);
            assert_eq!(result["excerptMessages"], 0);
        }
        // Old adapters do not attest a diagnostic fallback as body text.
        let legacy = ingest(
            &store,
            &key,
            input(&key, "legacy clipped text", false),
            2,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(legacy["pressure"]["recordedEvents"], 0);
        assert_eq!(
            ingest(&store, &key, excerpt(&key, "real body"), 3, || Err(
                "TARGET_CHANGED".into()
            ))
            .unwrap_err(),
            "TARGET_CHANGED"
        );
        assert_eq!(store.pressure(&key).unwrap().recorded_events, 0);
    }

    #[test]
    fn excerpt_pressure_is_per_conversation_and_can_reach_the_configured_trigger() {
        use crate::web_conversation_context::ContextPressurePolicy;
        let root =
            std::env::temp_dir().join(format!("web-excerpt-pressure-{}", uuid::Uuid::new_v4()));
        let store = WebContextStore::open(&root).unwrap();
        let alpha = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: "https://chatgpt.com/c/excerpt-alpha".into(),
            target_generation: 1,
        };
        let beta = ConversationKey {
            project_id: "beta".into(),
            conversation_url: "https://chatgpt.com/c/excerpt-beta".into(),
            target_generation: 1,
        };
        for key in [&alpha, &beta] {
            store.bind_observed_target(key.clone(), None).unwrap();
        }
        let beta_before = store.pressure(&beta).unwrap();
        store
            .configure_policy(
                "alpha",
                ContextPressurePolicy {
                    configured_budget_tokens: 128,
                    safety_headroom_tokens: 32,
                    unknown_source_reserve_tokens: 0,
                    automatic_rollover_enabled: true,
                    ..Default::default()
                },
            )
            .unwrap();
        ingest(
            &store,
            &alpha,
            excerpt(&alpha, &"observed body excerpt ".repeat(90)),
            1,
            || Ok(()),
        )
        .unwrap();
        let (pressure, due) = store.rollover_readiness(&alpha).unwrap();
        assert!(pressure.near_capacity && due);
        assert!(!pressure.coverage_known);
        assert_eq!(pressure.unknown_source_count, 0);
        assert_eq!(store.pressure(&beta).unwrap(), beta_before);
        let next = ConversationKey {
            conversation_url: "https://chatgpt.com/c/excerpt-alpha-next".into(),
            target_generation: 2,
            ..alpha.clone()
        };
        store
            .bind_observed_target(next.clone(), Some(&alpha))
            .unwrap();
        assert_eq!(store.pressure(&next).unwrap().visible_unique_tokens, 0);
        assert_eq!(store.pressure(&next).unwrap().excerpt_only_messages, 0);
        assert_eq!(
            store.pressure(&alpha).unwrap().visible_unique_tokens,
            pressure.visible_unique_tokens
        );
        assert!(
            store
                .record_browser_excerpt_observation(
                    &alpha,
                    "old",
                    VisibleSourceKind::AssistantMessage,
                    "body",
                    2
                )
                .is_err()
        );
    }
    #[test]
    fn browser_excerpt_replay_and_revision_do_not_accumulate_snapshots() {
        let root = std::env::temp_dir().join(format!("web-observe-{}", uuid::Uuid::new_v4()));
        let store = WebContextStore::open(root).unwrap();
        let key = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: "https://chatgpt.com/c/alpha".into(),
            target_generation: 1,
        };
        store.bind_observed_target(key.clone(), None).unwrap();
        let first = ingest(
            &store,
            &key,
            input(&key, "private short response", true),
            1,
            || Ok(()),
        )
        .unwrap();
        let replay = ingest(
            &store,
            &key,
            input(&key, "private short response", true),
            2,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(
            first["pressure"]["browserCoverage"]["lastObservedAtUnix"],
            1
        );
        assert_eq!(pressure_at(&first, 2), replay["pressure"]);
        let mut current_dom = input(&key, "private short response", true);
        current_dom["coverage"]["selectorTier"] = json!("SEMANTIC_UNIT");
        let current_dom = ingest(&store, &key, current_dom, 2, || Ok(())).unwrap();
        assert_eq!(pressure_at(&first, 2), current_dom["pressure"]);
        assert_eq!(current_dom["recordedMessages"], 1);
        assert!(!first.to_string().contains("private short response"));
        assert_eq!(first["recordedMessages"], 1);
        let changed = ingest(
            &store,
            &key,
            input(
                &key,
                "private longer response with several more words",
                true,
            ),
            3,
            || Ok(()),
        )
        .unwrap();
        assert_ne!(first["pressure"], changed["pressure"]);
        let incomplete = ingest(&store, &key, input(&key, "clipped", false), 4, || Ok(())).unwrap();
        assert_eq!(incomplete["recordedMessages"], 0);
        assert_eq!(incomplete["uncertainMessages"], 1);
        assert_eq!(
            incomplete["pressure"]["visibleUniqueTokens"],
            changed["pressure"]["visibleUniqueTokens"]
        );
        assert_eq!(
            incomplete["pressure"]["pressureAssessment"],
            "UNKNOWN_OR_INCOMPLETE"
        );
        assert_eq!(incomplete["pressure"]["browserCoverage"]["textGap"], true);
        for placeholder in [" \n\t", "This response\ncouldn’t load"] {
            let unreadable =
                ingest(&store, &key, input(&key, placeholder, true), 5, || Ok(())).unwrap();
            assert_eq!(unreadable["recordedMessages"], 0);
            assert_eq!(unreadable["pressure"], pressure_at(&incomplete, 5));
        }
        assert!(
            ingest(&store, &key, input(&key, "new text", true), 5, || Err(
                "TARGET_CHANGED".into()
            ))
            .is_err()
        );
        assert_eq!(
            store.pressure(&key).unwrap(),
            serde_json::from_value(pressure_at(&incomplete, 5)).unwrap()
        );
    }

    #[test]
    fn late_first_large_chat_and_disappearing_dom_remain_unknown_without_erasing_tokens() {
        let root = std::env::temp_dir().join(format!("web-observe-{}", uuid::Uuid::new_v4()));
        let store = WebContextStore::open(root).unwrap();
        let key = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: "https://chatgpt.com/c/late-chat".into(),
            target_generation: 1,
        };
        store.bind_observed_target(key.clone(), None).unwrap();
        let mut late = input(&key, "only the newest rendered message", true);
        late["truncated"] = json!(true);
        late["coverage"]["visibleRoleCount"] = json!(1024);
        late["coverage"]["countOverflow"] = json!(true);
        late["coverage"]["omittedBeforeWindow"] = json!(1023);
        let first = ingest(&store, &key, late.clone(), 1, || Ok(())).unwrap();
        assert_eq!(first["pressure"]["coverageKnown"], false);
        assert_eq!(
            first["pressure"]["pressureAssessment"],
            "UNKNOWN_OR_INCOMPLETE"
        );
        assert_eq!(
            first["pressure"]["browserCoverage"]["initialHistoryGap"],
            true
        );
        assert_eq!(first["pressure"]["browserCoverage"]["overflowGap"], true);
        assert_eq!(first["pressure"]["unknownSourceCount"], 0);
        let replay = ingest(&store, &key, late, 2, || Ok(())).unwrap();
        assert_eq!(pressure_at(&first, 2), replay["pressure"]);
        let vanished = json!({"messages":[],"truncated":false,
            "coverage":{"selectorTier":"NONE","visibleRoleCount":0,
                "countOverflow":false,"nestedCollapsed":0,"keyedCollapsed":0,
                "omittedBeforeWindow":0,"returned":0,"identityMissing":0,
                "identityUncertain":0,"revisionIncomplete":0,
                "textIncomplete":0,"anchorStability":"UNVERIFIED"}});
        let later = ingest(&store, &key, vanished, 3, || Ok(())).unwrap();
        assert_eq!(later["pressure"], pressure_at(&first, 3));
        assert_eq!(later["recordedMessages"], 0);
        let mut impossible_full = input(&key, "only the newest rendered message", true);
        impossible_full["coverage"]["anchorStability"] = json!("FULL_HISTORY_VERIFIED");
        assert_eq!(
            ingest(&store, &key, impossible_full, 4, || Ok(())).unwrap_err(),
            "WEB_SNAPSHOT_INVALID"
        );
        assert_eq!(
            store.pressure(&key).unwrap(),
            serde_json::from_value(pressure_at(&first, 3)).unwrap()
        );
    }
}
