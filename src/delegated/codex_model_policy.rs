//! Exact managed Codex model profiles. A contract selects a model explicitly;
//! the effort is fixed for that model. App-server metadata gates launch;
//! completed native-turn metadata independently gates review.

use serde::{Deserialize, Serialize};

pub const LEGACY_TERRA_MODEL: &str = "gpt-5.6-terra";
pub const SOL_MEDIUM_MODEL: &str = "gpt-6.1-sol";

/// Bounded metadata observed in the Codex-owned rollout for one completed
/// native turn. This is separate from the model requested on the CLI command
/// line and from mutable app-server thread settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliTurnProfileEvidenceV1 {
    pub codex_thread_id: String,
    pub codex_turn_id: String,
    pub model_id: String,
    pub reasoning_effort: String,
    /// SHA-256 of a bounded native task-start/context/complete metadata
    /// projection, excluding transcript text and authentication material.
    pub evidence_sha256: String,
}

impl CodexCliTurnProfileEvidenceV1 {
    pub fn matches(&self, expected_thread_id: &str, expected_model_id: &str) -> bool {
        self.codex_thread_id == expected_thread_id
            && !self.codex_turn_id.is_empty()
            && self.codex_turn_id.len() <= 128
            && self
                .codex_turn_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && self.evidence_sha256.len() == 64
            && self
                .evidence_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            && exact_profile_matches(expected_model_id, &self.model_id, &self.reasoning_effort)
    }
}

pub fn required_effort(model: &str) -> Option<&'static str> {
    match model {
        LEGACY_TERRA_MODEL => Some("high"),
        SOL_MEDIUM_MODEL => Some("medium"),
        _ => None,
    }
}

pub fn exact_profile_matches(
    expected_model: &str,
    observed_model: &str,
    observed_effort: &str,
) -> bool {
    required_effort(expected_model) == Some(observed_effort) && expected_model == observed_model
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_exact_and_never_silently_downgrade() {
        assert!(exact_profile_matches(
            LEGACY_TERRA_MODEL,
            LEGACY_TERRA_MODEL,
            "high"
        ));
        assert!(exact_profile_matches(
            SOL_MEDIUM_MODEL,
            SOL_MEDIUM_MODEL,
            "medium"
        ));
        assert!(!exact_profile_matches(
            SOL_MEDIUM_MODEL,
            LEGACY_TERRA_MODEL,
            "high"
        ));
        assert!(!exact_profile_matches(
            SOL_MEDIUM_MODEL,
            SOL_MEDIUM_MODEL,
            "high"
        ));
        assert!(!exact_profile_matches("unknown", "unknown", "medium"));
    }
}
