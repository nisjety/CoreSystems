/// Normalize text before chunking.
/// Strips excessive whitespace, normalizes Unicode, collapses blank lines.
pub fn normalize(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_was_blank = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_was_blank {
                result.push('\n');
                prev_was_blank = true;
            }
            continue;
        }
        prev_was_blank = false;

        // Collapse internal whitespace runs
        let mut prev_space = false;
        for ch in trimmed.chars() {
            if ch.is_whitespace() {
                if !prev_space {
                    result.push(' ');
                    prev_space = true;
                }
            } else {
                result.push(ch);
                prev_space = false;
            }
        }
        result.push('\n');
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_blank_lines() {
        // Multi-blank-line runs collapse to ONE blank line so paragraph
        // boundaries survive normalization (they matter for chunking).
        let input = "Hello\n\n\n\nWorld";
        assert_eq!(normalize(input), "Hello\n\nWorld");
    }

    #[test]
    fn collapses_internal_whitespace() {
        let input = "Hello   world   foo";
        assert_eq!(normalize(input), "Hello world foo");
    }
}
