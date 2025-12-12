// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Parsing utilities for kernel parameters.

/// Parse a boolean value from kernel parameter
///
/// Accepts various boolean representations:
/// - "on", "true", "1", "yes" → true
/// - Everything else → false
///
/// # Examples
///
/// ```
/// use nvrc::config::parser::parse_boolean;
///
/// assert!(parse_boolean("on"));
/// assert!(parse_boolean("true"));
/// assert!(parse_boolean("1"));
/// assert!(parse_boolean("yes"));
/// assert!(!parse_boolean("off"));
/// assert!(!parse_boolean("false"));
/// ```
pub fn parse_boolean(s: &str) -> bool {
    matches!(s.to_ascii_lowercase().as_str(), "on" | "true" | "1" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_boolean_true_values() {
        assert!(parse_boolean("on"));
        assert!(parse_boolean("true"));
        assert!(parse_boolean("1"));
        assert!(parse_boolean("yes"));
        assert!(parse_boolean("ON"));
        assert!(parse_boolean("True"));
        assert!(parse_boolean("YES"));
    }

    #[test]
    fn test_parse_boolean_false_values() {
        assert!(!parse_boolean("off"));
        assert!(!parse_boolean("false"));
        assert!(!parse_boolean("0"));
        assert!(!parse_boolean("no"));
        assert!(!parse_boolean("invalid"));
        assert!(!parse_boolean(""));
    }
}
