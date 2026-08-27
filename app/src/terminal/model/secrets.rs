pub use warp_terminal::model::secrets::*;
use std::sync::Arc;

use warpui::elements::SecretRange;

/// Returns the ranges of detected secrets in the given text along with their SecretLevel.
pub fn find_secrets_in_text_with_levels(text: &str) -> Vec<(SecretRange, SecretLevel)> {
    let secrets_regex: Arc<SecretsRegex> = { SECRETS_REGEX.lock().clone() };

    find_secrets_in_text_with_levels_using_regex(text, &secrets_regex)
}

pub const SECRET_REDACTION_REPLACEMENT_CHARACTER: &str = "*";

/// Returns the ranges of detected secrets in the given text.
pub fn find_secrets_in_text(text: &str) -> Vec<SecretRange> {
    find_secrets_in_text_with_levels(text)
        .into_iter()
        .map(|(range, _level)| range)
        .collect()
}

/// Redact all detected secrets in-place within the given string.
pub fn redact_secrets(input: &mut String) {
    let mut secrets: Vec<_> = find_secrets_in_text(input)
        .into_iter()
        .map(|r| r.byte_range)
        .collect();
    // Replace from the end to preserve indices
    secrets.sort_by_key(|range| range.start);
    for range in secrets.into_iter().rev() {
        let replacement =
            SECRET_REDACTION_REPLACEMENT_CHARACTER.repeat(range.end.saturating_sub(range.start));
        input.replace_range(range.start..range.end, &replacement);
    }
}


