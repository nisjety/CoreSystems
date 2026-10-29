//! Response-style / verbosity profiles (token-efficiency layer).
//!
//! An operator-selectable profile maps to a short system directive injected into
//! the prompt, letting a caller trade thoroughness for tokens without touching
//! reasoning or tool behaviour. `normal` (and any unknown value) injects nothing,
//! so the model's natural verbosity is the default — correctness is never traded
//! away, only presentation.

/// Map a verbosity profile to its system directive. `None` means "no directive"
/// (the `normal` default and any unknown value), so the prompt is unchanged.
#[must_use]
pub fn directive(profile: &str) -> Option<&'static str> {
    match profile.trim().to_ascii_lowercase().as_str() {
        "concise" | "brief" | "terse" => Some(
            "Response style: be concise. Answer in the fewest words that fully \
             address the request. Skip preamble, restating the question, and \
             filler; prefer short sentences and tight lists.",
        ),
        "detailed" | "thorough" | "verbose" => Some(
            "Response style: be thorough. Explain your reasoning, cover the \
             relevant edge cases, and include brief examples where they aid \
             understanding.",
        ),
        "minimal" | "caveman" => Some(
            "Response style: ultra-terse. Reply in the fewest possible words — \
             fragments and keywords over full sentences. No pleasantries, no \
             hedging, no restating. Correctness first, brevity second.",
        ),
        _ => None,
    }
}

/// Whether a profile string selects a non-default style (i.e. injects a directive).
#[must_use]
pub fn is_active(profile: &str) -> bool {
    directive(profile).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_and_unknown_inject_nothing() {
        assert!(directive("normal").is_none());
        assert!(directive("").is_none());
        assert!(directive("   ").is_none());
        assert!(directive("wat").is_none());
        assert!(!is_active("normal"));
    }

    #[test]
    fn concise_family_maps_to_concise_directive() {
        for p in ["concise", "Brief", " TERSE "] {
            let d = directive(p).unwrap_or("");
            assert!(d.contains("concise"), "profile {p:?} -> {d:?}");
        }
        assert!(is_active("concise"));
    }

    #[test]
    fn detailed_and_minimal_families_map() {
        assert!(directive("thorough").unwrap().contains("thorough"));
        assert!(directive("verbose").unwrap().contains("thorough"));
        assert!(directive("caveman").unwrap().contains("ultra-terse"));
        assert!(directive("minimal").unwrap().contains("ultra-terse"));
    }
}
