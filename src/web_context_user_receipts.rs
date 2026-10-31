//! Receipt-checked, content-free accounting in the existing context ledger.
//! Correlation is advisory and never authorizes delivery or a browser action.
use super::*;
use catdesk_wake::{protocol, store::Receipt};

pub(super) fn canonical_user(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn tagged_id(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix).is_some_and(|tail| {
        tail.len() == 64
            && tail
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
pub(super) fn is_browser_id(id: &str) -> bool {
    tagged_id(id, "browser-")
}

pub(super) fn reject_reserved(id: &str) -> Result<(), String> {
    if id.starts_with("delivered-user:") {
        Err("WEB_CONTEXT_USER_RECEIPT_REQUIRED".into())
    } else {
        Ok(())
    }
}

impl WebContextStore {
    /// Reuse retained event identities as the incremental accounting cursor.
    /// No second journal or per-project scheduler state is needed.
    pub(crate) fn delivered_user_ids(
        &self,
        key: &ConversationKey,
    ) -> Result<BTreeSet<String>, String> {
        key.validate()?;
        let _lock = self.read_lock()?;
        let project = self.read_project(&key.project_id)?;
        if !project.history.contains(key) {
            return Err("WEB_CONTEXT_TARGET_NOT_RECORDED".into());
        }
        Ok(self
            .read_ledger(key)?
            .events
            .into_iter()
            .filter(|event| {
                tagged_id(&event.event_id, "delivered-user:")
                    && event.source_kind == VisibleSourceKind::UserMessage
            })
            .map(|event| event.event_id)
            .collect())
    }

    /// The caller must establish the durable SENT receipt independently. This
    /// validates its target/message witnesses, stores only hashes/counts, and
    /// may finish on an already-known predecessor without reactivating it.
    pub(crate) fn record_delivered_user(
        &self,
        key: &ConversationKey,
        message: &str,
        receipt: &Receipt,
    ) -> Result<PressureStatus, String> {
        key.validate()?;
        if message.is_empty() || message.len() > MAX_VISIBLE_TEXT_BYTES {
            return Err("WEB_CONTEXT_USER_RECEIPT_INVALID".into());
        }
        let text = canonical_user(message);
        if receipt.schema_version != 1
            || !protocol::valid_id(&receipt.event_id)
            || receipt.target_generation != key.target_generation
            || receipt.target_digest != key.target_sha256()
            || receipt.message_digest != protocol::digest(&text)
            || receipt.sent_utc == 0
            || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
            || text.is_empty()
        {
            return Err("WEB_CONTEXT_USER_RECEIPT_INVALID".into());
        }
        let id = format!("delivered-user:{}", sha256(receipt.event_id.as_bytes()));
        let event = observation_metadata(Observation {
            event_id: &id,
            revision: 1,
            dedup_key: &id,
            source_kind: VisibleSourceKind::UserMessage,
            visible_text: Some(&text),
            observed_at_unix: receipt.sent_utc,
        })?;
        self.record_metadata_batch(key, vec![event], true)
    }
}

pub(super) struct Correlation<'a> {
    pub aliases: BTreeMap<&'a str, String>,
    pub known: usize,
    pub paired: usize,
    pub ambiguous: usize,
    pub possible_overlap_tokens: u64,
}

pub(super) fn correlate(events: &[EventMetadata]) -> Correlation<'_> {
    let mut groups = BTreeMap::<&str, (Vec<&EventMetadata>, Vec<&EventMetadata>)>::new();
    let mut result = Correlation {
        aliases: BTreeMap::new(),
        known: 0,
        paired: 0,
        ambiguous: 0,
        possible_overlap_tokens: 0,
    };
    for event in events {
        if event.source_kind != VisibleSourceKind::UserMessage
            || event.excerpt_only
            || event.dedup_key != event.event_id
        {
            continue;
        }
        let known = tagged_id(&event.event_id, "delivered-user:");
        if !known && !is_browser_id(&event.event_id) {
            continue;
        }
        let Some(hash) = event.content_sha256.as_deref() else {
            continue;
        };
        let group = groups.entry(hash).or_default();
        if known {
            group.0.push(event);
            result.known += 1;
        } else {
            group.1.push(event);
        }
    }
    for (known, browser) in groups.values() {
        if known.len() == 1 && browser.len() == 1 {
            result
                .aliases
                .insert(browser[0].event_id.as_str(), known[0].dedup_key.clone());
            result.paired += 1;
        } else if !known.is_empty() && !browser.is_empty() {
            // Equal text in distinct messages is not sufficient identity.
            result.ambiguous += 1;
            let tokens = |events: &[&EventMetadata]| {
                events.iter().fold(0u64, |sum, event| {
                    sum.saturating_add(event.estimated_tokens.unwrap_or(0))
                })
            };
            result.possible_overlap_tokens = result
                .possible_overlap_tokens
                .saturating_add(tokens(known).min(tokens(browser)));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (WebContextStore, ConversationKey) {
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("user-receipt-meter-{}", Uuid::new_v4()));
        let store = WebContextStore::open(root).unwrap();
        let key = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: "https://chatgpt.com/c/known-user-alpha".into(),
            target_generation: 1,
        };
        store.bind_observed_target(key.clone(), None).unwrap();
        (store, key)
    }
    fn receipt(key: &ConversationKey, id: &str, text: &str) -> Receipt {
        Receipt {
            schema_version: 1,
            event_id: id.into(),
            target_generation: key.target_generation,
            target_digest: key.target_sha256(),
            message_digest: protocol::digest(&canonical_user(text)),
            sent_utc: 10,
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        }
    }
    fn browser(
        store: &WebContextStore,
        key: &ConversationKey,
        id: &str,
        text: &str,
    ) -> PressureStatus {
        store
            .record_browser_observation(
                key,
                &format!("browser-{}", sha256(id.as_bytes())),
                VisibleSourceKind::UserMessage,
                Some(text),
                20,
            )
            .unwrap()
    }

    #[test]
    fn known_user_receipt_then_browser_counts_once_without_secret_text_or_replay_growth() {
        let (store, key) = fixture();
        let message = "Exact private context: opaque-demo-secret-value\n next step";
        let r = receipt(&key, "known-context", message);
        let first = store.record_delivered_user(&key, message, &r).unwrap();
        assert_eq!(first.known_delivered_user_messages, 1);
        assert_eq!(first.paired_delivered_user_messages, 0);
        let paired = browser(
            &store,
            &key,
            "unique-message",
            "Exact private context: opaque-demo-secret-value\t next step",
        );
        assert_eq!(paired.visible_unique_tokens, first.visible_unique_tokens);
        assert_eq!(paired.paired_delivered_user_messages, 1);
        let ledger = store.ledger_path(&key);
        let bytes = fs::read(&ledger).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("opaque-demo-secret-value"));
        assert!(!String::from_utf8_lossy(&bytes).contains("Exact private context"));
        assert_eq!(
            store.record_delivered_user(&key, message, &r).unwrap(),
            paired
        );
        assert_eq!(fs::read(&ledger).unwrap(), bytes);
        let cold = WebContextStore::open(&store.root).unwrap();
        assert_eq!(cold.pressure(&key).unwrap(), paired);
    }

    #[test]
    fn known_user_browser_first_and_parallel_projects_remain_independent() {
        let (store, alpha) = fixture();
        let beta = ConversationKey {
            project_id: "beta".into(),
            conversation_url: "https://chatgpt.com/c/known-user-beta".into(),
            target_generation: 7,
        };
        store.bind_observed_target(beta.clone(), None).unwrap();
        let first = browser(&store, &alpha, "one", "Same visible text");
        let alpha_status = store
            .record_delivered_user(
                &alpha,
                "Same visible text",
                &receipt(&alpha, "alpha-context", "Same visible text"),
            )
            .unwrap();
        assert_eq!(alpha_status.paired_delivered_user_messages, 1);
        assert_eq!(
            alpha_status.visible_unique_tokens,
            first.visible_unique_tokens
        );
        let beta_status = store
            .record_delivered_user(
                &beta,
                "Same visible text",
                &receipt(&beta, "beta-context", "Same visible text"),
            )
            .unwrap();
        assert_eq!(beta_status.known_delivered_user_messages, 1);
        assert_eq!(beta_status.paired_delivered_user_messages, 0);
        assert_eq!(store.pressure(&alpha).unwrap(), alpha_status);
    }

    #[test]
    fn equal_text_in_multiple_browser_messages_is_not_collapsed_into_one_turn() {
        let (store, key) = fixture();
        let text = "Repeated equal body";
        let known = store
            .record_delivered_user(&key, text, &receipt(&key, "one-receipt", text))
            .unwrap();
        browser(&store, &key, "first-dom-message", text);
        let ambiguous = browser(&store, &key, "second-dom-message", text);
        assert_eq!(ambiguous.ambiguous_delivered_user_groups, 1);
        assert_eq!(ambiguous.paired_delivered_user_messages, 0);
        assert_eq!(
            ambiguous.visible_unique_tokens,
            2 * known.visible_unique_tokens
        );
        assert_eq!(
            ambiguous.estimated_upper_tokens,
            3 * known.visible_unique_tokens
        );
        assert!(!ambiguous.coverage_known);
        assert_eq!(ambiguous.pressure_assessment, "UNKNOWN_OR_INCOMPLETE");
    }

    #[test]
    fn browser_revision_that_changes_body_breaks_only_its_receipt_correlation() {
        let (store, key) = fixture();
        let text = "immutable delivered task context";
        let known = store
            .record_delivered_user(&key, text, &receipt(&key, "one-receipt", text))
            .unwrap();
        assert_eq!(
            browser(&store, &key, "same-dom-message", text).paired_delivered_user_messages,
            1
        );
        let changed = browser(&store, &key, "same-dom-message", "a different current body");
        assert_eq!(changed.paired_delivered_user_messages, 0);
        assert_eq!(
            changed.visible_unique_tokens,
            known.visible_unique_tokens + estimate_tokens("a different current body")
        );
        assert_eq!(
            browser(&store, &key, "same-dom-message", "a different current body"),
            changed
        );
    }

    #[test]
    fn unknown_wrong_target_or_changed_message_receipts_never_change_the_ledger() {
        let (store, key) = fixture();
        let text = "Known task body";
        let good = receipt(&key, "one-receipt", text);
        let path = store.ledger_path(&key);
        let before = fs::read(&path).unwrap();
        for field in ["evidence", "generation", "target", "message", "event"] {
            let mut bad = good.clone();
            match field {
                "evidence" => bad.evidence = "UNKNOWN_SUBMISSION".into(),
                "generation" => bad.target_generation += 1,
                "target" => bad.target_digest = "0".repeat(64),
                "message" => bad.message_digest = "0".repeat(64),
                _ => bad.event_id = "../foreign".into(),
            }
            assert_eq!(
                store.record_delivered_user(&key, text, &bad).unwrap_err(),
                "WEB_CONTEXT_USER_RECEIPT_INVALID"
            );
            assert_eq!(fs::read(&path).unwrap(), before);
        }
    }

    #[test]
    fn excerpts_and_generic_caller_messages_never_gain_receipt_identity() {
        let (store, key) = fixture();
        let text = "some known body";
        store
            .record_delivered_user(&key, text, &receipt(&key, "one-receipt", text))
            .unwrap();
        let id = format!("browser-{}", sha256(b"clipped"));
        let clipped = store
            .record_browser_excerpt_observation(&key, &id, VisibleSourceKind::UserMessage, text, 20)
            .unwrap();
        assert_eq!(clipped.paired_delivered_user_messages, 0);
        assert_eq!(clipped.excerpt_only_messages, 1);
        let generic = store
            .record_observation(
                &key,
                Observation {
                    event_id: "operator-input",
                    revision: 1,
                    dedup_key: "operator-input",
                    source_kind: VisibleSourceKind::UserMessage,
                    visible_text: Some(text),
                    observed_at_unix: 20,
                },
            )
            .unwrap();
        assert_eq!(generic.paired_delivered_user_messages, 0);
        assert_eq!(generic.visible_unique_tokens, 3 * estimate_tokens(text));
    }

    #[test]
    fn receipt_accounting_can_finish_on_known_history_without_reactivating_it() {
        let (store, old) = fixture();
        let next = ConversationKey {
            project_id: old.project_id.clone(),
            conversation_url: "https://chatgpt.com/c/known-user-replacement".into(),
            target_generation: 2,
        };
        store
            .bind_observed_target(next.clone(), Some(&old))
            .unwrap();
        let p = store
            .record_delivered_user(
                &old,
                "delivered before rollover",
                &receipt(&old, "old-event", "delivered before rollover"),
            )
            .unwrap();
        assert!(!p.is_current);
        assert_eq!(store.active_key("alpha").unwrap(), Some(next.clone()));
        let unrecorded = ConversationKey {
            target_generation: 3,
            ..next
        };
        assert_eq!(
            store
                .record_delivered_user(
                    &unrecorded,
                    "unknown chat",
                    &receipt(&unrecorded, "unknown", "unknown chat")
                )
                .unwrap_err(),
            "WEB_CONTEXT_OLD_GENERATION"
        );
    }

    #[test]
    fn legacy_browser_record_is_not_rewritten_until_a_new_complete_capture() {
        let (store, key) = fixture();
        let text = "paragraph one\n\nparagraph two";
        let id = format!("browser-{}", sha256(b"legacy-message"));
        store
            .record_observation(
                &key,
                Observation {
                    event_id: &id,
                    revision: 1,
                    dedup_key: &id,
                    source_kind: VisibleSourceKind::UserMessage,
                    visible_text: Some(text),
                    observed_at_unix: 5,
                },
            )
            .unwrap();
        let known = store
            .record_delivered_user(&key, text, &receipt(&key, "one-receipt", text))
            .unwrap();
        assert_eq!(known.paired_delivered_user_messages, 0);
        let p = store
            .record_browser_observation(&key, &id, VisibleSourceKind::UserMessage, Some(text), 20)
            .unwrap();
        assert_eq!(p.paired_delivered_user_messages, 1);
        assert_eq!(p.recorded_events, 2);
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(store.ledger_path(&key)).unwrap()).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert!(
            !value["events"][0]
                .as_object()
                .unwrap()
                .contains_key("receipt")
        );
    }

    #[test]
    fn generic_observations_cannot_forge_or_alias_the_receipt_namespace() {
        let (store, key) = fixture();
        let id = format!("delivered-user:{}", sha256(b"fake"));
        let path = store.ledger_path(&key);
        let before = fs::read(&path).unwrap();
        for (event_id, dedup_key) in [(&*id, "generic"), ("generic", &*id)] {
            let make = || Observation {
                event_id,
                revision: 1,
                dedup_key,
                source_kind: VisibleSourceKind::UserMessage,
                visible_text: Some("not a receipt"),
                observed_at_unix: 10,
            };
            assert_eq!(
                store.record_observation(&key, make()).unwrap_err(),
                "WEB_CONTEXT_USER_RECEIPT_REQUIRED"
            );
            assert_eq!(
                store.record_observation_batch(&key, [make()]).unwrap_err(),
                "WEB_CONTEXT_USER_RECEIPT_REQUIRED"
            );
            assert_eq!(
                validate_observation(make()).unwrap_err(),
                "WEB_CONTEXT_USER_RECEIPT_REQUIRED"
            );
            assert_eq!(fs::read(&path).unwrap(), before);
        }
    }

    #[test]
    fn whitespace_normalization_cannot_bypass_the_original_browser_input_budget() {
        let (store, key) = fixture();
        let text = " ".repeat(MAX_VISIBLE_TEXT_BYTES + 1);
        let id = format!("browser-{}", sha256(b"oversize-body"));
        let before = fs::read(store.ledger_path(&key)).unwrap();
        assert_eq!(
            store
                .record_browser_observation(
                    &key,
                    &id,
                    VisibleSourceKind::UserMessage,
                    Some(&text),
                    20
                )
                .unwrap_err(),
            "WEB_CONTEXT_EVENT_CONTENT_OVERSIZED"
        );
        assert_eq!(fs::read(store.ledger_path(&key)).unwrap(), before);
    }

    #[test]
    fn ambiguous_overlap_alone_cannot_trigger_an_observed_pressure_rollover() {
        let (store, key) = fixture();
        let text = "The same body in two distinct turns";
        let tokens = estimate_tokens(text);
        store
            .configure_policy(
                "alpha",
                ContextPressurePolicy {
                    configured_budget_tokens: 3 * tokens,
                    safety_headroom_tokens: 0,
                    unknown_source_reserve_tokens: 0,
                    automatic_rollover_enabled: true,
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .record_delivered_user(&key, text, &receipt(&key, "one-receipt", text))
            .unwrap();
        browser(&store, &key, "first", text);
        let p = browser(&store, &key, "second", text);
        assert_eq!(p.visible_unique_tokens, 2 * tokens);
        assert_eq!(p.estimated_upper_tokens, 3 * tokens);
        assert!(!p.near_capacity);
        assert_eq!(p.ambiguous_delivered_user_groups, 1);
        assert!(
            store
                .request_rollover_if_due(&key, &key.target_sha256(), "source-task", true, 30)
                .unwrap()
                .is_none()
        );
    }
}
