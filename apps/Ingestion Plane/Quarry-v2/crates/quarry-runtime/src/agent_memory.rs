//! Agent scratchpad memory + sensitive-data redaction (OSS-parity P2 3B).
//!
//! browser-use-style agent ergonomics: a bounded scratchpad the planner loop
//! can write observations/decisions to and reference on later steps, plus a
//! `redact_sensitive` pass so secrets in page text never reach the model or
//! logs. Pure-logic + dep-free; the `AgentLoop` consumes these (wiring mirrors
//! prior component rollouts). (Full DOM-for-LLM serialization upgrade is a
//! separate follow-up; this covers the memory + safety half of 3B.)

use std::collections::VecDeque;

/// Bounded append-only scratchpad. Oldest notes are evicted past `cap`.
#[derive(Debug)]
pub struct AgentScratchpad {
    notes: VecDeque<String>,
    cap: usize,
}

impl AgentScratchpad {
    pub fn new(cap: usize) -> Self {
        Self {
            notes: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    /// Append a note; secrets are redacted before storage.
    pub fn note(&mut self, s: impl Into<String>) {
        if self.notes.len() == self.cap {
            self.notes.pop_front();
        }
        self.notes.push_back(redact_sensitive(&s.into()));
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// Render the scratchpad for prompt injection (newest last).
    pub fn render(&self) -> String {
        self.notes.iter().cloned().collect::<Vec<_>>().join("\n")
    }
}

/// Redact obvious secrets from free text before it reaches a model or log:
/// emails, long secret-like tokens (API keys/bearer tokens), and card-like
/// digit runs. Token-based + dep-free (no regex).
pub fn redact_sensitive(text: &str) -> String {
    text.split_whitespace()
        .map(|tok| {
            // Preserve trailing punctuation so prose stays readable.
            let (core, trail) = split_trailing_punct(tok);
            let redacted = if looks_like_email(core) {
                "[redacted-email]"
            } else if looks_like_card(core) {
                "[redacted-number]"
            } else if looks_like_secret(core) {
                "[redacted-token]"
            } else {
                core
            };
            format!("{redacted}{trail}")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_trailing_punct(tok: &str) -> (&str, &str) {
    let idx = tok
        .trim_end_matches(['.', ',', ';', ':', ')', ']', '!', '?'])
        .len();
    tok.split_at(idx)
}

fn looks_like_email(t: &str) -> bool {
    if let Some(at) = t.find('@') {
        let (local, domain) = t.split_at(at);
        !local.is_empty() && domain.len() > 2 && domain.contains('.') && !t.contains("..")
    } else {
        false
    }
}

fn looks_like_secret(t: &str) -> bool {
    t.len() >= 24
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '=' | '+' | '/' | '.'))
        && t.chars().any(|c| c.is_ascii_alphabetic())
        && t.chars().any(|c| c.is_ascii_digit())
}

fn looks_like_card(t: &str) -> bool {
    let digits: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
    let non_sep = t
        .chars()
        .all(|c| c.is_ascii_digit() || c == '-' || c == ' ');
    non_sep
        && (13..=19).contains(&digits.len())
        && digits.len() == t.chars().filter(|c| *c != '-').count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratchpad_is_bounded_fifo() {
        let mut sp = AgentScratchpad::new(2);
        sp.note("a");
        sp.note("b");
        sp.note("c"); // evicts "a"
        assert_eq!(sp.len(), 2);
        assert_eq!(sp.render(), "b\nc");
    }

    #[test]
    fn redacts_email() {
        assert_eq!(
            redact_sensitive("mail me at bob@example.com please"),
            "mail me at [redacted-email] please"
        );
    }

    #[test]
    fn redacts_long_token() {
        let out = redact_sensitive("key sk-ABC123def456GHI789jkl012MNO please");
        assert!(out.contains("[redacted-token]"));
        assert!(out.contains("key") && out.contains("please"));
    }

    #[test]
    fn redacts_card_number() {
        assert_eq!(
            redact_sensitive("card 4111-1111-1111-1111 ok"),
            "card [redacted-number] ok"
        );
    }

    #[test]
    fn leaves_normal_text() {
        assert_eq!(
            redact_sensitive("the quick brown fox"),
            "the quick brown fox"
        );
    }

    #[test]
    fn scratchpad_redacts_on_store() {
        let mut sp = AgentScratchpad::new(4);
        sp.note("found token sk-ABC123def456GHI789jkl012MNO on page");
        assert!(sp.render().contains("[redacted-token]"));
    }
}
