//! Dependency-free, read-only stable wake readiness contract.
//!
//! This module is compiled by both the CatDesk process and the standalone
//! `catdesk-stable-wake-host` binary.  It deliberately has no daemon, MCP,
//! release, browser, provider, or host-install authority.

use reqwest::Url;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

pub(crate) const STABLE_PROJECT_ID: &str = "catdesk";
const SCHEMA_VERSION: u32 = 1;
const MAX_INBOX_RECORDS: usize = 512;
const MAX_INBOX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_REFERENCE_BYTES: usize = 1024;
const MAX_IDENTITY_BYTES: usize = 128;
const MAX_WAKE_CONFIG_BYTES: u64 = 16 * 1024;
const MAX_WAKE_TARGET_BYTES: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum ReviewState {
    Draft,
    Queued,
    Running,
    Verifying,
    WaitingForChatgpt,
    WaitingForUser,
    RateLimited,
    Paused,
    CompletedVerified,
    Blocked,
    Failed,
    Cancelled,
    LeaseExpired,
    CreditBudgetExhausted,
    RecoveringAfterRestart,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewInboxRecord {
    pub(crate) schema_version: u32,
    pub(crate) record_id: String,
    pub(crate) project_id: String,
    pub(crate) session_id: String,
    pub(crate) state: ReviewState,
    pub(crate) next_action: String,
    pub(crate) reference: String,
    pub(crate) created_at_unix: u64,
    pub(crate) unread: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EventClass {
    Pending(ReviewInboxRecord),
    Stale(ReviewInboxRecord),
}

/// Bounded, non-secret evidence.  `target_sha256` is never a conversation URL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct StableWakeReadiness {
    pub(crate) pending_count: usize,
    pub(crate) stale_count: usize,
    pub(crate) discovery_available: bool,
    pub(crate) target_available: bool,
    pub(crate) target_sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProtectedWakeTargetIdentity {
    pub(crate) target_sha256: String,
}

/// Read and classify the only canonical durable inbox.  This never mutates it.
pub(crate) fn discover(workspace_root: &Path, project: &str) -> Result<Vec<EventClass>, String> {
    let records = canonical_inbox_records(workspace_root, project)?;

    let mut unique = BTreeMap::new();
    for record in records {
        if let Some(previous) = unique.insert(record.record_id.clone(), record.clone())
            && previous != record
        {
            return Err("stable wake inbox conflict".into());
        }
    }

    Ok(unique
        .into_values()
        .map(|record| {
            if record.unread && is_actionable(&record) {
                EventClass::Pending(record)
            } else {
                EventClass::Stale(record)
            }
        })
        .collect())
}

/// Load the fixed canonical inbox with the same containment, bounded parsing,
/// schema, identity, reference, and exact-project checks as `discover`.
/// It deliberately retains raw cardinality so a caller with stronger safety
/// needs (terminal history retirement) can require exactly one durable record.
pub(crate) fn canonical_inbox_records(
    workspace_root: &Path,
    project: &str,
) -> Result<Vec<ReviewInboxRecord>, String> {
    validate_slug(project)?;
    let workspace_root = canonical_directory(workspace_root, "stable wake root unavailable")?;
    let catdesk_root = canonical_child_directory(&workspace_root, ".catdesk")?;
    let autonomy_root = canonical_child_directory(&catdesk_root, "autonomy")?;
    let inbox = canonical_regular_child(&autonomy_root, "review-inbox.json")?;
    let bytes = read_bounded_regular_file(&inbox, MAX_INBOX_BYTES, "stable wake inbox")?;
    let records: Vec<ReviewInboxRecord> =
        serde_json::from_slice(&bytes).map_err(|_| "stable wake inbox malformed".to_string())?;
    if records.len() > MAX_INBOX_RECORDS {
        return Err("stable wake inbox count exceeded".into());
    }

    for record in &records {
        validate_review_record(record)?;
        if record.project_id != project {
            return Err("stable wake project refused".into());
        }
    }
    Ok(records)
}

/// Evaluate both durable prerequisites.  The inbox remains independently
/// readable when the exact target config is unavailable, while the final host
/// result stays fail-closed unless both are available.
#[allow(dead_code)] // Used by the CatDesk binary; this module is also compiled by the host binary.
pub(crate) fn workspace_readiness(workspace_root: &Path) -> StableWakeReadiness {
    let target = read_protected_wake_target(workspace_root)
        .and_then(|identity| verify_protected_wake_target(workspace_root, &identity.target_sha256));
    let events = discover(workspace_root, STABLE_PROJECT_ID);
    match (events, target) {
        (Ok(events), Ok(target)) => readiness(events, true, Some(target.target_sha256)),
        (Ok(events), Err(_)) => readiness(events, false, None),
        (Err(_), Ok(target)) => StableWakeReadiness {
            pending_count: 0,
            stale_count: 0,
            discovery_available: false,
            target_available: true,
            target_sha256: Some(target.target_sha256),
        },
        (Err(_), Err(_)) => StableWakeReadiness {
            pending_count: 0,
            stale_count: 0,
            discovery_available: false,
            target_available: false,
            target_sha256: None,
        },
    }
}

/// Strict final readiness for the standalone host.  The optional opaque digest
/// may only constrain the configured target; it cannot select a replacement.
#[allow(dead_code)] // Used by the standalone host; this module is also compiled by CatDesk.
pub(crate) fn evaluate_host(
    workspace_root: &Path,
    expected_target_sha256: Option<&str>,
) -> Result<StableWakeReadiness, String> {
    let events = discover(workspace_root, STABLE_PROJECT_ID)?;
    let target = match expected_target_sha256 {
        Some(expected) => verify_protected_wake_target(workspace_root, expected)?,
        None => {
            let first = read_protected_wake_target(workspace_root)?;
            verify_protected_wake_target(workspace_root, &first.target_sha256)?
        }
    };
    Ok(readiness(events, true, Some(target.target_sha256)))
}

fn readiness(
    events: Vec<EventClass>,
    target_available: bool,
    target_sha256: Option<String>,
) -> StableWakeReadiness {
    StableWakeReadiness {
        pending_count: events
            .iter()
            .filter(|event| matches!(event, EventClass::Pending(_)))
            .count(),
        stale_count: events
            .iter()
            .filter(|event| matches!(event, EventClass::Stale(_)))
            .count(),
        discovery_available: true,
        target_available,
        target_sha256,
    }
}

pub(crate) fn read_protected_wake_target(
    workspace: &Path,
) -> Result<ProtectedWakeTargetIdentity, String> {
    let config_path = protected_config_path(workspace)?;
    let bytes =
        read_bounded_regular_file(&config_path, MAX_WAKE_CONFIG_BYTES, "wake configuration")?;
    let config = parse_wake_config(&bytes)?;
    let target = config
        .get("conversation_url")
        .and_then(Value::as_str)
        .ok_or_else(|| "wake configuration is invalid".to_string())?;
    let canonical =
        canonical_wake_target(target).map_err(|_| "wake configuration is invalid".to_string())?;
    if canonical != target {
        return Err("wake configuration is invalid".into());
    }
    Ok(ProtectedWakeTargetIdentity {
        target_sha256: sha256(&canonical),
    })
}

pub(crate) fn verify_protected_wake_target(
    workspace: &Path,
    expected_target_sha256: &str,
) -> Result<ProtectedWakeTargetIdentity, String> {
    if !is_sha256_hex(expected_target_sha256) {
        return Err("wake target binding is invalid".into());
    }
    let identity = read_protected_wake_target(workspace)?;
    if identity.target_sha256 != expected_target_sha256.to_ascii_lowercase() {
        return Err("wake target binding drifted".into());
    }
    Ok(identity)
}

pub(crate) fn canonical_wake_target(value: &str) -> Result<String, String> {
    if value.is_empty()
        || value.len() > MAX_WAKE_TARGET_BYTES
        || !value.is_ascii()
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err("wake target is invalid".into());
    }
    let parsed = Url::parse(value).map_err(|_| "wake target is invalid".to_string())?;
    if parsed.scheme() != "https"
        || !matches!(parsed.host_str(), Some("chatgpt.com" | "chat.openai.com"))
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("wake target is invalid".into());
    }
    let valid_id = |value: &str| is_slug(value, 200);
    let valid_conversation_id = |value: &str| {
        valid_id(value) || (value.len() <= 200 && value.strip_prefix("WEB:").is_some_and(valid_id))
    };
    let segments = parsed
        .path_segments()
        .ok_or_else(|| "wake target is invalid".to_string())?
        .collect::<Vec<_>>();
    let path = match segments.as_slice() {
        ["c", conversation] if valid_conversation_id(conversation) => {
            format!("/c/{conversation}")
        }
        ["g", project, "c", conversation]
            if valid_id(project) && valid_conversation_id(conversation) =>
        {
            format!("/g/{project}/c/{conversation}")
        }
        _ => return Err("wake target is invalid".into()),
    };
    Ok(format!(
        "https://{}{}",
        parsed.host_str().expect("validated host"),
        path
    ))
}

/// Fixed W13-compatible wake text. This is data-only and never submits it.
#[allow(dead_code)] // Used by delivery state; core is also compiled by the host binary.
pub(crate) fn wake_message(record_id: &str) -> String {
    format!(
        "CatDesk review record {record_id}: This is an automated CatDesk wake event. Acknowledge or claim this triggering CatDesk review event before proceeding. Take stock of the current project status and where you last left off. Ensure the previous task and review bundle have been completed and reviewed. Continue working through the broad implementation phases of the project by dividing them into appropriately bounded tickets/tasks. For the next ticket, create a detailed technical design and implementation plan for Codex, send those instructions through CatDesk, and request a review bundle so the implementation can be verified. If the previous work is incomplete or issues remain, continue the fix/review cycle until major bugs are no longer an issue. Do not ask the user to relay prompts between agents. Involve the user only for genuine operator-only decisions."
    )
}

#[allow(dead_code)] // Used by delivery state; core is also compiled by the host binary.
pub(crate) fn wake_message_sha256(record_id: &str) -> String {
    let normalized = wake_message(record_id)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    sha256(&normalized)
}

pub(crate) fn is_actionable(record: &ReviewInboxRecord) -> bool {
    matches!(
        (&record.state, record.next_action.as_str()),
        (ReviewState::CompletedVerified, "independent_final_review")
            | (ReviewState::WaitingForChatgpt, "chatgpt_decision_required")
    )
}

fn validate_review_record(record: &ReviewInboxRecord) -> Result<(), String> {
    if record.schema_version != SCHEMA_VERSION
        || contains_secret_marker(&record.reference)
        || !is_slug(&record.record_id, MAX_IDENTITY_BYTES)
        || !is_slug(&record.project_id, MAX_IDENTITY_BYTES)
        || !is_slug(&record.session_id, MAX_IDENTITY_BYTES)
        || !is_slug(&record.next_action, MAX_IDENTITY_BYTES)
    {
        return Err("stable wake inbox unsafe".into());
    }
    validate_reference(&record.reference)
}

fn validate_reference(reference: &str) -> Result<(), String> {
    if reference.is_empty()
        || reference.len() > MAX_REFERENCE_BYTES
        || reference.bytes().any(|byte| byte == 0)
        || reference.contains('\\')
    {
        return Err("stable wake reference unsafe".into());
    }
    let path = Path::new(reference);
    if path.is_absolute()
        || looks_prefixed(reference)
        || path.components().next().is_none()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("stable wake reference unsafe".into());
    }
    Ok(())
}

fn protected_config_path(workspace: &Path) -> Result<PathBuf, String> {
    let workspace = canonical_directory(workspace, "wake configuration is unavailable")?;
    let control = canonical_child_directory(&workspace, ".catdesk")
        .map_err(|_| "wake configuration is unavailable".to_string())?;
    let wake_root = canonical_child_directory(&control, "wake-bridge")
        .map_err(|_| "wake configuration is unavailable".to_string())?;
    canonical_regular_child(&wake_root, "config.json").map_err(|error| {
        if error == "stable wake inbox unsafe" {
            "wake configuration is unsafe".into()
        } else {
            "wake configuration is unavailable".into()
        }
    })
}

fn canonical_directory(path: &Path, unavailable: &str) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| unavailable.to_string())?;
    if !metadata.is_dir() || path_is_reparse_or_symlink(&metadata) {
        return Err("stable wake inbox unsafe".into());
    }
    path.canonicalize().map_err(|_| unavailable.to_string())
}

fn canonical_child_directory(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let candidate = parent.join(name);
    let metadata = fs::symlink_metadata(&candidate)
        .map_err(|_| "stable wake inbox unavailable".to_string())?;
    if !metadata.is_dir() || path_is_reparse_or_symlink(&metadata) {
        return Err("stable wake inbox unsafe".into());
    }
    let canonical = candidate
        .canonicalize()
        .map_err(|_| "stable wake inbox unavailable".to_string())?;
    if !canonical.starts_with(parent) {
        return Err("stable wake inbox unsafe".into());
    }
    Ok(canonical)
}

fn canonical_regular_child(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let candidate = parent.join(name);
    let metadata = fs::symlink_metadata(&candidate)
        .map_err(|_| "stable wake inbox unavailable".to_string())?;
    if !metadata.is_file() || path_is_reparse_or_symlink(&metadata) {
        return Err("stable wake inbox unsafe".into());
    }
    let canonical = candidate
        .canonicalize()
        .map_err(|_| "stable wake inbox unavailable".to_string())?;
    if !canonical.starts_with(parent) {
        return Err("stable wake inbox unsafe".into());
    }
    Ok(canonical)
}

fn read_bounded_regular_file(path: &Path, max_bytes: u64, label: &str) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|_| format!("{label} is unavailable"))?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(unsafe_input_error(label));
    }
    let bytes = fs::read(path).map_err(|_| format!("{label} is unavailable"))?;
    if bytes.len() as u64 > max_bytes {
        return Err(unsafe_input_error(label));
    }
    Ok(bytes)
}

fn unsafe_input_error(label: &str) -> String {
    if label == "stable wake inbox" {
        "stable wake inbox unsafe".into()
    } else {
        format!("{label} is unsafe")
    }
}

fn path_is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    false
}

fn sha256(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_slug(value: &str) -> Result<(), String> {
    if !is_slug(value, MAX_IDENTITY_BYTES) {
        return Err("stable wake identity unsafe".into());
    }
    Ok(())
}

fn is_slug(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn looks_prefixed(value: &str) -> bool {
    value.len() >= 2 && value.as_bytes()[0].is_ascii_alphabetic() && value.as_bytes()[1] == b':'
}

fn contains_secret_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "token=",
        "password=",
        "secret=",
        "api_key=",
        "authorization:",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

struct StrictWakeConfigObject(Map<String, Value>);

impl<'de> Deserialize<'de> for StrictWakeConfigObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = StrictWakeConfigObject;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an unambiguous JSON object")
            }
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = Map::new();
                while let Some((name, value)) = map.next_entry::<String, Value>()? {
                    if values.insert(name.clone(), value).is_some() {
                        return Err(de::Error::custom("duplicate wake configuration field"));
                    }
                }
                Ok(StrictWakeConfigObject(values))
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}

fn parse_wake_config(bytes: &[u8]) -> Result<Map<String, Value>, String> {
    if bytes.len() > MAX_WAKE_CONFIG_BYTES as usize {
        return Err("wake configuration is invalid".into());
    }
    let object = serde_json::from_slice::<StrictWakeConfigObject>(bytes)
        .ok()
        .map(|value| value.0)
        .ok_or_else(|| "wake configuration is invalid".to_string())?;
    if object.is_empty()
        || !object
            .get("profile_dir")
            .and_then(Value::as_str)
            .is_some_and(|value| {
                !value.is_empty()
                    && value.len() <= 1024
                    && !value.bytes().any(|byte| byte.is_ascii_control())
            })
    {
        return Err("wake configuration is invalid".into());
    }
    let current = object
        .get("conversation_url")
        .and_then(Value::as_str)
        .ok_or_else(|| "wake configuration is invalid".to_string())?;
    canonical_wake_target(current).map_err(|_| "wake configuration is invalid".to_string())?;
    for name in [
        "ui_ready_timeout_seconds",
        "send_confirmation_timeout_seconds",
        "ui_poll_interval_seconds",
        "debounce_seconds",
    ] {
        if let Some(value) = object.get(name)
            && !value
                .as_f64()
                .is_some_and(|value| value.is_finite() && value > 0.0 && value <= 3_600.0)
        {
            return Err("wake configuration is invalid".into());
        }
    }
    Ok(object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn workspace(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("catdesk-stable-core-{name}-{nonce}"));
        fs::create_dir_all(root.join(".catdesk/autonomy")).expect("autonomy root");
        fs::create_dir_all(root.join(".catdesk/wake-bridge")).expect("wake root");
        root
    }

    fn record(id: &str) -> ReviewInboxRecord {
        ReviewInboxRecord {
            schema_version: SCHEMA_VERSION,
            record_id: id.into(),
            project_id: STABLE_PROJECT_ID.into(),
            session_id: "session_1".into(),
            state: ReviewState::CompletedVerified,
            next_action: "independent_final_review".into(),
            reference: "artifacts/completion.json".into(),
            created_at_unix: 1,
            unread: true,
        }
    }

    fn inbox(root: &Path) -> PathBuf {
        root.join(".catdesk/autonomy/review-inbox.json")
    }

    fn config(root: &Path) -> PathBuf {
        root.join(".catdesk/wake-bridge/config.json")
    }

    fn write_inbox(root: &Path, records: &[ReviewInboxRecord]) {
        fs::write(
            inbox(root),
            serde_json::to_vec(records).expect("serialize inbox"),
        )
        .expect("write inbox");
    }

    fn write_target(root: &Path, target: &str) {
        fs::write(
            config(root),
            serde_json::json!({
                "conversation_url": target,
                "profile_dir": ".catdesk/wake-bridge/browser-profile",
            })
            .to_string(),
        )
        .expect("write target");
    }

    #[test]
    fn canonical_nested_record_is_pending_and_read_only() {
        let root = workspace("nested");
        write_inbox(&root, &[record("review_1")]);
        let before = fs::read(inbox(&root)).unwrap();
        assert!(matches!(
            discover(&root, STABLE_PROJECT_ID).unwrap().as_slice(),
            [EventClass::Pending(_)]
        ));
        assert_eq!(fs::read(inbox(&root)).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_waiting_and_exact_duplicate_classification_is_deterministic() {
        let root = workspace("classification");
        let pending = record("review_1");
        let mut waiting = record("review_2");
        waiting.state = ReviewState::WaitingForChatgpt;
        waiting.next_action = "chatgpt_decision_required".into();
        let mut stale = record("review_3");
        stale.unread = false;
        write_inbox(&root, &[pending.clone(), pending, waiting, stale]);
        let events = discover(&root, STABLE_PROJECT_ID).unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, EventClass::Pending(_)))
                .count(),
            2
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, EventClass::Stale(_)))
                .count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn semantic_duplicate_conflict_fails_closed() {
        let root = workspace("conflict");
        let valid = record("review_1");
        let mut conflict = valid.clone();
        conflict.created_at_unix = 2;
        write_inbox(&root, &[valid, conflict]);
        assert_eq!(
            discover(&root, STABLE_PROJECT_ID),
            Err("stable wake inbox conflict".into())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_schema_malformed_and_count_fail_closed() {
        let root = workspace("validation");
        let mut wrong_project = record("review_1");
        wrong_project.project_id = "other".into();
        write_inbox(&root, &[wrong_project]);
        assert_eq!(
            discover(&root, STABLE_PROJECT_ID),
            Err("stable wake project refused".into())
        );
        let mut wrong_schema = record("review_1");
        wrong_schema.schema_version = 2;
        write_inbox(&root, &[wrong_schema]);
        assert_eq!(
            discover(&root, STABLE_PROJECT_ID),
            Err("stable wake inbox unsafe".into())
        );
        fs::write(inbox(&root), b"not-json").unwrap();
        assert_eq!(
            discover(&root, STABLE_PROJECT_ID),
            Err("stable wake inbox malformed".into())
        );
        let records: Vec<_> = (0..=MAX_INBOX_RECORDS)
            .map(|index| record(&format!("review_{index}")))
            .collect();
        write_inbox(&root, &records);
        assert_eq!(
            discover(&root, STABLE_PROJECT_ID),
            Err("stable wake inbox count exceeded".into())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsafe_reference_identity_and_oversized_inbox_fail_closed() {
        let root = workspace("unsafe");
        for reference in [
            "",
            "../outside",
            "/rooted",
            "C:\\outside",
            "artifacts\\outside",
        ] {
            let mut unsafe_record = record("review_1");
            unsafe_record.reference = reference.into();
            write_inbox(&root, &[unsafe_record]);
            assert!(discover(&root, STABLE_PROJECT_ID).is_err());
        }
        let mut bad_identity = record("review_1");
        bad_identity.record_id = "bad identity".into();
        write_inbox(&root, &[bad_identity]);
        assert_eq!(
            discover(&root, STABLE_PROJECT_ID),
            Err("stable wake inbox unsafe".into())
        );
        fs::write(inbox(&root), vec![b'x'; MAX_INBOX_BYTES as usize + 1]).unwrap();
        assert_eq!(
            discover(&root, STABLE_PROJECT_ID),
            Err("stable wake inbox unsafe".into())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn protected_target_is_exact_read_only_and_drift_refuses() {
        let root = workspace("target");
        let web_target = "https://chatgpt.com/c/WEB:1229263e-88d1-41a1-8ce2-b4aa97fbcb0f";
        assert_eq!(
            canonical_wake_target(web_target).expect("WEB-prefixed wake target"),
            web_target
        );
        for invalid in [
            "https://chatgpt.com/c/WEB:",
            "https://chatgpt.com/c/web:thread",
            "https://chatgpt.com/c/WEB:thread:extra",
            "https://chatgpt.com/g/WEB:project/c/thread",
        ] {
            assert!(canonical_wake_target(invalid).is_err(), "{invalid}");
        }
        write_target(&root, web_target);
        let before = fs::read(config(&root)).unwrap();
        let identity = read_protected_wake_target(&root).unwrap();
        assert_eq!(fs::read(config(&root)).unwrap(), before);
        assert!(verify_protected_wake_target(&root, &identity.target_sha256).is_ok());
        write_target(&root, "https://chatgpt.com/c/drifted-thread");
        assert_eq!(
            verify_protected_wake_target(&root, &identity.target_sha256),
            Err("wake target binding drifted".into())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn readiness_does_not_depend_on_runtime_artifacts() {
        let root = workspace("runtime");
        write_inbox(&root, &[record("review_1")]);
        write_target(&root, "https://chatgpt.com/c/exact-thread");
        let expected = workspace_readiness(&root);
        fs::create_dir_all(root.join("target/release")).unwrap();
        fs::write(root.join("target/release/catdesk.exe"), b"replacement").unwrap();
        fs::create_dir_all(root.join(".catdesk/reviewed-release")).unwrap();
        fs::write(root.join(".catdesk/reviewed-release/manifest.json"), b"bad").unwrap();
        assert_eq!(workspace_readiness(&root), expected);
        fs::remove_dir_all(root).unwrap();
    }
}
