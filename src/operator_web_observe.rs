//! Private browser excerpt ingestion. Public output contains counts only.
use crate::web_conversation_context::{
    BrowserCoverageObservation, ConversationKey, VisibleSourceKind, WebContextStore,
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
            "AUTHOR_ROLE" | "ARTICLE_ROLE" | "NONE"
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
    let mut uncertain = 0;
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
            store.record_browser_observation(
                key,
                &format!("browser-{id}"),
                role,
                Some(&message.text),
                observed_at,
            )?;
            recorded += 1;
        } else {
            uncertain += 1;
            // Keep the previous complete revision, if any. Incomplete units
            // set durable coverage uncertainty. Transient clipping must not
            // add an invented token reserve on every newly streamed message.
        }
    }
    confirm_target()?;
    let pressure = store.record_browser_coverage(
        key,
        BrowserCoverageObservation {
            initial_history_gap: true,
            identity_gap: identity_missing > 0
                || identity_uncertain > 0
                || coverage.nested_collapsed > 0
                || coverage.keyed_collapsed > 0,
            text_gap: text_incomplete > 0 || revision_incomplete > 0 || uncertain > 0,
            overflow_gap: snapshot.truncated
                || coverage.count_overflow
                || coverage.omitted_before_window > 0,
        },
    )?;
    Ok(
        json!({"action":"web_context_capture","projectId":key.project_id,
        "recordedMessages":recorded,"uncertainMessages":uncertain,
        "excerptTruncated":snapshot.truncated,"anchorStability":"UNVERIFIED",
        "coverage":"RECENT_VISIBLE_EXCERPT_WITH_UNKNOWN_HISTORY",
        "pressure":pressure,"advisory":true,"originAuthenticated":false,
        "browserBindingVerified":true,"rawTextPersisted":false,"userMessageSent":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(first["pressure"], replay["pressure"]);
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
            assert_eq!(unreadable["pressure"], incomplete["pressure"]);
        }
        assert!(
            ingest(&store, &key, input(&key, "new text", true), 5, || Err(
                "TARGET_CHANGED".into()
            ))
            .is_err()
        );
        assert_eq!(
            store.pressure(&key).unwrap(),
            serde_json::from_value(incomplete["pressure"].clone()).unwrap()
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
        assert_eq!(first["pressure"], replay["pressure"]);
        let vanished = json!({"messages":[],"truncated":false,
            "coverage":{"selectorTier":"NONE","visibleRoleCount":0,
                "countOverflow":false,"nestedCollapsed":0,"keyedCollapsed":0,
                "omittedBeforeWindow":0,"returned":0,"identityMissing":0,
                "identityUncertain":0,"revisionIncomplete":0,
                "textIncomplete":0,"anchorStability":"UNVERIFIED"}});
        let later = ingest(&store, &key, vanished, 3, || Ok(())).unwrap();
        assert_eq!(later["pressure"], first["pressure"]);
        assert_eq!(later["recordedMessages"], 0);
        let mut impossible_full = input(&key, "only the newest rendered message", true);
        impossible_full["coverage"]["anchorStability"] = json!("FULL_HISTORY_VERIFIED");
        assert_eq!(
            ingest(&store, &key, impossible_full, 4, || Ok(())).unwrap_err(),
            "WEB_SNAPSHOT_INVALID"
        );
        assert_eq!(
            store.pressure(&key).unwrap(),
            serde_json::from_value(first["pressure"].clone()).unwrap()
        );
    }
}
