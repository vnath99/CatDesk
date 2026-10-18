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
                "review_ready" | "engineering_attention" | "test"
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
        Ok(())
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
