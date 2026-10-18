//! Private browser excerpt ingestion. Public output contains counts only.
use crate::web_conversation_context::{ConversationKey, VisibleSourceKind, WebContextStore};
use catdesk_wake::protocol::digest;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    messages: Vec<Message>,
    truncated: bool,
    coverage: Value,
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
    if snapshot.messages.len() > 16
        || snapshot
            .coverage
            .get("anchorStability")
            .and_then(Value::as_str)
            != Some("UNVERIFIED")
        || snapshot.coverage.get("returned").and_then(Value::as_u64)
            != Some(snapshot.messages.len() as u64)
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
    let mut recorded = 0;
    let mut uncertain = 0;
    // A recent rendered excerpt cannot establish unseen history, attachments,
    // provider context or cross-document anchor stability. Reserve once per chat,
    // never once per polling run. The reserve remains explicitly conservative.
    confirm_target()?;
    store.record_browser_observation(
        key,
        "browser-unobserved-context",
        VisibleSourceKind::UnknownAttachment,
        None,
        observed_at,
    )?;
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
            // share the configurable whole-chat reserve; transient clipping
            // must not add a permanent reserve on every newly streamed message.
        }
    }
    confirm_target()?;
    Ok(
        json!({"action":"web_context_capture","projectId":key.project_id,
        "recordedMessages":recorded,"uncertainMessages":uncertain,
        "excerptTruncated":snapshot.truncated,"anchorStability":"UNVERIFIED",
        "coverage":"RECENT_VISIBLE_EXCERPT_WITH_UNKNOWN_CONTEXT_RESERVE",
        "pressure":store.pressure(key)?,"advisory":true,"originAuthenticated":false,
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
            "coverage":{"anchorStability":"UNVERIFIED","returned":1}})
    }
    #[test]
    fn browser_excerpt_replay_revision_and_unknown_reserve_do_not_accumulate_snapshots() {
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
        assert_eq!(incomplete["pressure"], changed["pressure"]);
        for placeholder in [" \n\t", "This response\ncouldn’t load"] {
            let unreadable =
                ingest(&store, &key, input(&key, placeholder, true), 5, || Ok(())).unwrap();
            assert_eq!(unreadable["recordedMessages"], 0);
            assert_eq!(unreadable["pressure"], changed["pressure"]);
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
}
