//! Exact managed Codex model profiles. A contract selects a model explicitly;
//! the effort is fixed for that model and must be read back from app-server
//! metadata before any mutating turn.

pub const LEGACY_TERRA_MODEL: &str = "gpt-5.6-terra";
pub const SOL_MEDIUM_MODEL: &str = "gpt-6.1-sol";

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
