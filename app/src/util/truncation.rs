/// Truncate text from the end with ellipsis if it exceeds max_length.
/// Properly handles UTF-8 character boundaries to avoid panics.
pub fn truncate_from_end(text: &str, max_length: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_length {
        text.to_string()
    } else {
        let chars_to_take = max_length.saturating_sub(1);
        let truncated: String = text.chars().take(chars_to_take).collect();
        format!("{truncated}…")
    }
}

/// Truncate text from the beginning with ellipsis if it exceeds max_length.
/// Properly handles UTF-8 character boundaries to avoid panics.
pub fn truncate_from_beginning(text: &str, max_length: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_length {
        text.to_string()
    } else {
        let chars_to_take = max_length.saturating_sub(1);
        let truncated: String = text.chars().skip(char_count - chars_to_take).collect();
        format!("…{truncated}")
    }
}

/// Safely truncate a string at the given byte index, ensuring we don't split UTF-8 characters
pub fn safe_truncate(s: &mut String, new_len: usize) {
    if new_len >= s.len() {
        return;
    }
    let safe_len = floor_char_boundary(s, new_len);
    s.truncate(safe_len);
}

/// Find the largest valid character boundary at or before the given byte index
pub fn floor_char_boundary(original_string: &str, idx: usize) -> usize {
    if idx >= original_string.len() {
        original_string.len()
    } else {
        let mut curr = idx;
        while curr > 0 && !original_string.is_char_boundary(curr) {
            curr -= 1;
        }
        curr
    }
}
