//! Shared identifier formatting helpers.

/// Sanitize text into a lowercase ASCII identifier component.
///
/// ASCII alphanumeric characters are retained and lowercased. Other characters
/// become underscores, then leading and trailing underscores are removed. The
/// function returns `None` when no identifier component remains after
/// sanitization so callers can apply a domain-specific fallback.
pub fn sanitize_ascii_identifier_component(value: &str) -> Option<String> {
    let sanitized = value
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();

    let trimmed = sanitized.trim_matches('_');

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_ascii_identifier_component;

    #[test]
    fn sanitize_ascii_identifier_component_lowercases_and_replaces_separators() {
        assert_eq!(
            sanitize_ascii_identifier_component(" ES Futures Globex "),
            Some("es_futures_globex".to_string())
        );
    }

    #[test]
    fn sanitize_ascii_identifier_component_reports_empty_identifier() {
        assert_eq!(sanitize_ascii_identifier_component(" - "), None);
    }
}
