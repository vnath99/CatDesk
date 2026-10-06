//! Inert fixture used only so the delegated review harness can run its fixed Cargo verifier.
pub fn review_fixture_marker() -> &'static str {
    "T-0364-QWEN-ROUTE-R5"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_is_fixed() {
        assert_eq!(review_fixture_marker(), "T-0364-QWEN-ROUTE-R5");
    }
}
