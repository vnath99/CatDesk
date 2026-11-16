use crate::{PROTOCOL_VERSION, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// Typed Wake intent metadata lives in the existing audit-reference namespace.
/// Events without these references retain their original JSON and digests.
const WAKE_METADATA_NAMESPACE: &str = "catdesk.wake.";
const WAKE_METADATA_V1: &str = "catdesk.wake.v1:";
pub const MAX_WAKE_TTL_SECONDS: u64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WakePurpose {
    ManualEndTurn,
    NewTask,
    Rollover,
    PeriodicHealth,
}

impl WakePurpose {
    fn wire_value(self) -> &'static str {
        match self {
            Self::ManualEndTurn => "MANUAL_END_TURN",
            Self::NewTask => "NEW_TASK",
            Self::Rollover => "ROLLOVER",
            Self::PeriodicHealth => "PERIODIC_HEALTH",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "MANUAL_END_TURN" => Ok(Self::ManualEndTurn),
            "NEW_TASK" => Ok(Self::NewTask),
            "ROLLOVER" => Ok(Self::Rollover),
            "PERIODIC_HEALTH" => Ok(Self::PeriodicHealth),
            _ => Err("QUEUE_WAKE_METADATA_INVALID".into()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WakeMetadata {
    pub purpose: WakePurpose,
    pub source_event_id: String,
    pub source_turn_id: String,
    /// Scheduling hint only; it does not override per-project FIFO or fairness.
    pub priority: u8,
    /// Absolute Unix seconds. TTL is measured from the immutable received time.
    pub expires_utc: u64,
}

impl WakeMetadata {
    pub fn validate(&self, created_utc: u64) -> Result<()> {
        if created_utc == 0
            || !valid_id(&self.source_event_id)
            || !valid_id(&self.source_turn_id)
            || self.expires_utc <= created_utc
            || self.expires_utc - created_utc > MAX_WAKE_TTL_SECONDS
        {
            return Err("QUEUE_WAKE_METADATA_INVALID".into());
        }
        Ok(())
    }

    /// Pass these references to the existing immutable, target-bound producer.
    /// All five are required together; partial metadata is never accepted.
    pub fn audit_references(&self, created_utc: u64) -> Result<Vec<String>> {
        self.validate(created_utc)?;
        Ok([
            ("purpose", self.purpose.wire_value().to_owned()),
            ("source-event", self.source_event_id.clone()),
            ("source-turn", self.source_turn_id.clone()),
            ("priority", self.priority.to_string()),
            ("expires-utc", self.expires_utc.to_string()),
        ]
        .into_iter()
        .map(|(key, value)| format!("{WAKE_METADATA_V1}{key}:{value}"))
        .collect())
    }
}

/// Accept only an exact conversation URL, including a ChatGPT Project route
/// and the reviewed uppercase WEB: conversation compatibility spelling. No
/// redirects, query strings, ports or URL credentials.
pub fn validate_target(url: &str) -> Result<()> {
    let path = url
        .strip_prefix("https://chatgpt.com/")
        .or_else(|| url.strip_prefix("https://chat.openai.com/"))
        .ok_or("TARGET_INVALID_URL")?;
    let parts = path.split('/').collect::<Vec<_>>();
    let conversation = match parts.as_slice() {
        ["c", conversation] => *conversation,
        ["g", project, "c", conversation] if valid_id(project) => *conversation,
        _ => return Err("TARGET_INVALID_URL".into()),
    };
    let suffix = conversation.strip_prefix("WEB:").unwrap_or(conversation);
    if url.len() > 512 || conversation.len() > 200 || !valid_id(suffix) {
        return Err("TARGET_INVALID_URL".into());
    }
    Ok(())
}

#[cfg(test)]
mod target_tests {
    use super::validate_target;

    #[test]
    fn project_conversation_routes_are_exact_and_credential_free() {
        for url in [
            "https://chatgpt.com/c/turn-1",
            "https://chatgpt.com/g/g-p-123/c/turn-1",
            "https://chat.openai.com/g/g-p-123/c/WEB:turn-1",
        ] {
            assert!(validate_target(url).is_ok(), "{url}");
        }
        for url in [
            "https://chatgpt.com/g/project/c/",
            "https://chatgpt.com/g/project/c/turn-1/",
            "https://chatgpt.com/g/project/c/turn-1?query=1",
            "https://chatgpt.com/g/WEB:project/c/turn-1",
            "https://user@chatgpt.com/g/project/c/turn-1",
            "https://chatgpt.com:443/g/project/c/turn-1",
            "https://other.example/g/project/c/turn-1",
        ] {
            assert!(validate_target(url).is_err(), "{url}");
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub schema_version: u32,
    pub event_id: String,
    pub project_id: String,
    pub event_type: String,
    /// Unix seconds UTC. Integers avoid local timezone and floating point ambiguity.
    pub created_utc: u64,
    pub message: String,
    pub target_generation: u64,
    #[serde(default)]
    pub audit_references: Vec<String>,
}

impl Event {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PROTOCOL_VERSION {
            return Err("QUEUE_PROTOCOL_UNSUPPORTED".into());
        }
        if !valid_id(&self.event_id)
            || !valid_id(&self.project_id)
            || !matches!(
                self.event_type.as_str(),
                "review_ready" | "engineering_attention" | "source_review" | "test"
            )
            || self.created_utc == 0
            || self.target_generation == 0
            || self.message.is_empty()
            || self
                .message
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                != self.message
            || self.message.len() > 2048
            || self.message.chars().any(|c| c.is_control())
            || self.audit_references.len() > 8
            || self.audit_references.iter().any(|s| {
                s.is_empty()
                    || s.len() > 256
                    || s.chars().any(|c| c.is_control())
                    || s.contains("://")
            })
        {
            return Err("QUEUE_EVENT_INVALID".into());
        }
        self.wake_metadata()?;
        Ok(())
    }

    pub fn wake_metadata(&self) -> Result<Option<WakeMetadata>> {
        let mut fields = std::collections::BTreeMap::new();
        for reference in &self.audit_references {
            if !reference.starts_with(WAKE_METADATA_NAMESPACE) {
                continue;
            }
            let (key, value) = reference
                .strip_prefix(WAKE_METADATA_V1)
                .and_then(|reference| reference.split_once(':'))
                .ok_or("QUEUE_WAKE_METADATA_INVALID")?;
            if !matches!(
                key,
                "purpose" | "source-event" | "source-turn" | "priority" | "expires-utc"
            ) || fields.insert(key, value).is_some()
            {
                return Err("QUEUE_WAKE_METADATA_INVALID".into());
            }
        }
        if fields.is_empty() {
            return Ok(None);
        }
        if fields.len() != 5 {
            return Err("QUEUE_WAKE_METADATA_INVALID".into());
        }
        let priority = fields["priority"]
            .parse::<u8>()
            .map_err(|_| "QUEUE_WAKE_METADATA_INVALID")?;
        let expires_utc = fields["expires-utc"]
            .parse::<u64>()
            .map_err(|_| "QUEUE_WAKE_METADATA_INVALID")?;
        if priority.to_string() != fields["priority"]
            || expires_utc.to_string() != fields["expires-utc"]
        {
            return Err("QUEUE_WAKE_METADATA_INVALID".into());
        }
        let metadata = WakeMetadata {
            purpose: WakePurpose::parse(fields["purpose"])?,
            source_event_id: fields["source-event"].into(),
            source_turn_id: fields["source-turn"].into(),
            priority,
            expires_utc,
        };
        metadata.validate(self.created_utc)?;
        Ok(Some(metadata))
    }

    /// Add metadata without replacing ordinary evidence or rewriting an intent.
    pub fn with_wake_metadata(mut self, metadata: WakeMetadata) -> Result<Self> {
        if let Some(existing) = self.wake_metadata()? {
            if existing != metadata {
                return Err("QUEUE_WAKE_METADATA_CONFLICT".into());
            }
        } else {
            self.audit_references
                .extend(metadata.audit_references(self.created_utc)?);
        }
        self.validate()?;
        Ok(self)
    }

    /// Expiration forbids a new submission, never reconciliation of a prior one.
    pub fn is_expired_at(&self, observed_utc: u64) -> Result<bool> {
        Ok(self
            .wake_metadata()?
            .is_some_and(|metadata| observed_utc >= metadata.expires_utc))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub generation: u64,
    pub url: String,
    pub digest: String,
}

impl Target {
    pub fn new(generation: u64, url: String) -> Result<Self> {
        validate_target(&url)?;
        if generation == 0 {
            return Err("CONFIG_GENERATION_INVALID".into());
        }
        Ok(Self {
            generation,
            digest: digest(&url),
            url,
        })
    }
    pub fn validate(&self) -> Result<()> {
        if Self::new(self.generation, self.url.clone())? != *self {
            return Err("CONFIG_DIGEST_MISMATCH".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod wake_metadata_tests {
    use super::*;

    const LEGACY_EVENT: &str = r#"{"schemaVersion":1,"eventId":"legacy-event","projectId":"alpha","eventType":"test","createdUtc":100,"message":"Review alpha","targetGeneration":1,"auditReferences":["review-receipt-1"]}"#;

    fn event() -> Event {
        serde_json::from_str(LEGACY_EVENT).unwrap()
    }

    fn metadata() -> WakeMetadata {
        WakeMetadata {
            purpose: WakePurpose::ManualEndTurn,
            source_event_id: "source-event-1".into(),
            source_turn_id: "source-turn-1".into(),
            priority: 5,
            expires_utc: 160,
        }
    }

    #[test]
    fn wake_metadata_absence_preserves_legacy_event_wire_bytes_and_digest() {
        let event = event();
        event.validate().unwrap();
        assert_eq!(event.wake_metadata().unwrap(), None);
        assert!(!event.is_expired_at(u64::MAX).unwrap());
        let wire = serde_json::to_string(&event).unwrap();
        assert_eq!(wire, LEGACY_EVENT);
        assert_eq!(
            digest(&wire),
            "2d6c490be50d99a2d328a25b6047ab7f5ea3bbe4ba80f32b99dc113636bbf156"
        );
    }

    #[test]
    fn wake_metadata_round_trips_all_purposes_and_has_exact_expiry_boundary() {
        for purpose in [
            WakePurpose::ManualEndTurn,
            WakePurpose::NewTask,
            WakePurpose::Rollover,
            WakePurpose::PeriodicHealth,
        ] {
            let metadata = WakeMetadata {
                purpose,
                ..metadata()
            };
            let enriched = event().with_wake_metadata(metadata.clone()).unwrap();
            assert_eq!(enriched.audit_references[0], "review-receipt-1");
            assert_eq!(enriched.audit_references.len(), 6);
            assert_eq!(enriched.wake_metadata().unwrap(), Some(metadata.clone()));
            assert!(!enriched.is_expired_at(159).unwrap());
            assert!(enriched.is_expired_at(160).unwrap());
            let wire = serde_json::to_string(&enriched).unwrap();
            let decoded: Event = serde_json::from_str(&wire).unwrap();
            decoded.validate().unwrap();
            assert_eq!(decoded, enriched);
            assert_eq!(
                enriched.clone().with_wake_metadata(metadata).unwrap(),
                enriched
            );
        }
    }

    #[test]
    fn wake_metadata_refuses_partial_duplicate_unknown_or_noncanonical_fields() {
        let valid = metadata().audit_references(100).unwrap();
        let mut cases = Vec::new();
        cases.push(valid[..4].to_vec());
        let mut duplicate = valid.clone();
        duplicate.push(valid[0].clone());
        cases.push(duplicate);
        for replacement in [
            "catdesk.wake.v2:purpose:MANUAL_END_TURN",
            "catdesk.wake.v1:other:MANUAL_END_TURN",
            "catdesk.wake.v1:purpose:manual_end_turn",
            "catdesk.wake.v1:purpose:UNKNOWN",
        ] {
            let mut references = valid.clone();
            references[0] = replacement.into();
            cases.push(references);
        }
        for (index, key, values) in [
            (1, "source-event", vec!["", "event:foreign", "event space"]),
            (2, "source-turn", vec!["", "turn/foreign", "turn space"]),
            (3, "priority", vec!["-1", "+1", "01", "256"]),
            (
                4,
                "expires-utc",
                vec!["0", "100", "+160", "0160", "18446744073709551616"],
            ),
        ] {
            for value in values {
                let mut references = valid.clone();
                references[index] = format!("{WAKE_METADATA_V1}{key}:{value}");
                cases.push(references);
            }
        }
        for references in cases {
            let invalid = Event {
                audit_references: references,
                ..event()
            };
            assert_eq!(
                invalid.validate().unwrap_err(),
                "QUEUE_WAKE_METADATA_INVALID",
                "{:?}",
                invalid.audit_references
            );
        }
    }

    #[test]
    fn wake_metadata_ttl_and_identity_are_bounded_without_overflow() {
        let mut metadata = metadata();
        metadata.expires_utc = 100 + MAX_WAKE_TTL_SECONDS;
        metadata.validate(100).unwrap();
        metadata.expires_utc += 1;
        assert!(metadata.validate(100).is_err());
        metadata.expires_utc = u64::MAX;
        metadata.validate(u64::MAX - MAX_WAKE_TTL_SECONDS).unwrap();
        assert!(metadata.validate(u64::MAX).is_err());
        metadata.source_event_id = "a".repeat(201);
        assert!(metadata.validate(u64::MAX - 1).is_err());
        assert!(metadata.validate(0).is_err());
    }

    #[test]
    fn wake_metadata_cannot_rewrite_an_existing_intent_or_overflow_audit_capacity() {
        let enriched = event().with_wake_metadata(metadata()).unwrap();
        let changed = WakeMetadata {
            priority: 6,
            ..metadata()
        };
        assert_eq!(
            enriched.with_wake_metadata(changed).unwrap_err(),
            "QUEUE_WAKE_METADATA_CONFLICT"
        );
        let full = Event {
            audit_references: (1..=4).map(|id| format!("existing-{id}")).collect(),
            ..event()
        };
        assert_eq!(
            full.with_wake_metadata(metadata()).unwrap_err(),
            "QUEUE_EVENT_INVALID"
        );
    }
}
