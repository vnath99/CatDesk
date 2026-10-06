//! In-process read-only adapter for the standalone stable wake core.
//!
//! The independently invokable host and the existing status observation use
//! the same canonical inbox and protected target implementation.

pub(crate) use crate::stable_wake_core::workspace_readiness;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_owns_no_second_event_authority() {
        let source = include_str!("stable_wake_bootstrap.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        assert!(production.contains("stable_wake_core"));
        assert!(!production.contains("stable-wake/review-events"));
    }

    #[test]
    fn production_status_source_invokes_the_read_only_adapter() {
        let state_source = include_str!("state.rs");
        assert!(state_source.contains("stable_wake_bootstrap::workspace_readiness"));
        assert!(state_source.contains("\"stableWake\""));
    }

    #[test]
    fn core_adapter_exposes_the_production_readiness_entry() {
        let _ = workspace_readiness;
    }
}
