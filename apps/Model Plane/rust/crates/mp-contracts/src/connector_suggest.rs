//! The one contract of a connected-app suggestion (Wave 3 EP-10 S8, decision D6).
//!
//! When a question needs the person's own mail, calendar or files and no connected account is
//! selected for the turn, the Ask loop may offer `suggest_connector`. The model names one provider
//! from a closed list and says in one line why; the person sees a card offering to connect it or to
//! use it for this question. The tool reads nothing and grants nothing: it only shows the card, and
//! the turn continues without that data.
//!
//! The provider list is closed on purpose. A free-text provider would let a prompt steer the card
//! to any vendor, and the card's order is the model's single pick, never a ranking. The reason is
//! model-written text that is kept in the message parts and shown to the person, so it is reduced to
//! one plain line with no address or link before it is accepted.

use serde::Deserialize;

/// The tool's name, the same in the offer, the dispatch arm and the event.
pub const TOOL_NAME: &str = "suggest_connector";

/// The providers a suggestion may name: the ones the Ask loop can read once a connection of theirs
/// is selected (WS23-S2/S3). Adding one is a contract change, never a model choice.
pub const SUGGESTABLE_PROVIDERS: &[&str] = &["google", "microsoft"];

/// Most characters of a reason, after scrubbing.
pub const MAX_REASON_CHARS: usize = 200;

/// What the model reads about the tool. It carries the whole rule, so a turn that does not offer
/// the tool carries no prompt text about it either.
pub const TOOL_DESCRIPTION: &str = "Suggest that the person connects, or selects, one of their own \
accounts for this question. Use it only when answering needs the person's own mail, calendar or files \
and you cannot see them on this turn. Call it at most once per turn, name exactly one provider, and \
never use it to compare or rank vendors. It shows the person a card and returns at once: you still \
cannot read that data on this turn, so answer without it and say what you could do with it once the \
account is connected. The reason is one short plain sentence about the person's question, with no \
names, addresses or links.";

/// The tool's argument schema, with the provider list taken from [`SUGGESTABLE_PROVIDERS`] so the
/// offer and the validator cannot disagree.
#[must_use]
pub fn tool_parameters_json() -> String {
    serde_json::json!({
        "type": "object",
        "properties": {
            "provider": {
                "type": "string",
                "enum": SUGGESTABLE_PROVIDERS,
                "description": "google for Gmail, Google Calendar and Drive; microsoft for Outlook, its calendar and OneDrive",
            },
            "reason": {
                "type": "string",
                "maxLength": MAX_REASON_CHARS,
                "description": "One short plain sentence saying why the account would help with this question",
            },
        },
        "required": ["provider", "reason"],
        "additionalProperties": false,
    })
    .to_string()
}

/// A suggestion that passed every check: a provider from the closed list and a scrubbed reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestArgs {
    pub provider: &'static str,
    pub reason: String,
}

/// Why a suggestion was refused. Each refusal reaches the model as a tool error it can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestProblem {
    /// The arguments are not an object of exactly `provider` and `reason` strings.
    Malformed,
    /// The provider is not in [`SUGGESTABLE_PROVIDERS`].
    UnknownProvider,
    /// Nothing is left of the reason once it is scrubbed.
    EmptyReason,
    /// The scrubbed reason is longer than [`MAX_REASON_CHARS`].
    ReasonTooLong,
}

impl SuggestProblem {
    /// The tool error the model reads.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Malformed => {
                "suggest_connector takes exactly two strings: 'provider' and 'reason'"
            }
            Self::UnknownProvider => {
                "suggest_connector can only suggest 'google' or 'microsoft'"
            }
            Self::EmptyReason => {
                "suggest_connector needs a short plain reason, without names, addresses or links"
            }
            Self::ReasonTooLong => "suggest_connector's reason must be at most 200 characters",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArgs {
    provider: String,
    reason: String,
}

/// Parse and check the model's arguments. The provider must match an entry exactly, so no other
/// spelling reaches the card; the reason is scrubbed first and then bounded.
///
/// # Errors
/// [`SuggestProblem`] naming the first check that failed.
pub fn parse_args(arguments_json: &str) -> Result<SuggestArgs, SuggestProblem> {
    let raw: RawArgs =
        serde_json::from_str(arguments_json).map_err(|_| SuggestProblem::Malformed)?;
    let provider = SUGGESTABLE_PROVIDERS
        .iter()
        .copied()
        .find(|known| *known == raw.provider)
        .ok_or(SuggestProblem::UnknownProvider)?;
    let reason = scrub_reason(&raw.reason);
    if reason.is_empty() {
        return Err(SuggestProblem::EmptyReason);
    }
    if reason.chars().count() > MAX_REASON_CHARS {
        return Err(SuggestProblem::ReasonTooLong);
    }
    Ok(SuggestArgs { provider, reason })
}

/// The reason as one plain line: control and format characters dropped, every run of whitespace
/// one space, and any word that is an address or a link left out. The card renders the line as
/// text, and the message parts keep it, so neither a mail address nor a link the model copied from
/// the conversation is shown or stored again.
#[must_use]
pub fn scrub_reason(raw: &str) -> String {
    let plain: String = raw
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control() && !is_format_char(*c))
        .collect();
    plain
        .split(' ')
        .filter(|word| !word.is_empty() && !is_address_or_link(word))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Invisible characters that can reorder or hide text on the card: bidi controls, zero-width
/// characters and the byte-order mark.
fn is_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}'
    )
}

fn is_address_or_link(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    word.contains('@')
        || lower.contains("://")
        || lower.starts_with("www.")
        || lower.starts_with("mailto:")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(provider: &str, reason: &str) -> String {
        serde_json::json!({ "provider": provider, "reason": reason }).to_string()
    }

    #[test]
    fn the_provider_list_is_closed_and_exact() {
        assert_eq!(SUGGESTABLE_PROVIDERS, &["google", "microsoft"]);
        for provider in SUGGESTABLE_PROVIDERS {
            let parsed = parse_args(&args(provider, "Your mail would answer this.")).expect("valid");
            assert_eq!(parsed.provider, *provider);
        }
        for other in ["Google", "slack", "notion", "", " google", "google,microsoft"] {
            assert_eq!(
                parse_args(&args(other, "Your mail would answer this.")),
                Err(SuggestProblem::UnknownProvider),
                "{other:?}"
            );
        }
    }

    #[test]
    fn the_schema_offers_exactly_the_closed_list() {
        let schema: serde_json::Value =
            serde_json::from_str(&tool_parameters_json()).expect("schema is JSON");
        assert_eq!(
            schema["properties"]["provider"]["enum"],
            serde_json::json!(["google", "microsoft"])
        );
        assert_eq!(schema["properties"]["reason"]["maxLength"], MAX_REASON_CHARS);
        assert_eq!(schema["required"], serde_json::json!(["provider", "reason"]));
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn malformed_arguments_are_refused() {
        for bad in [
            "",
            "not json",
            "[]",
            r#"{"provider":"google"}"#,
            r#"{"reason":"x"}"#,
            r#"{"provider":1,"reason":"x"}"#,
            r#"{"provider":"google","reason":"x","rank":1}"#,
        ] {
            assert_eq!(parse_args(bad), Err(SuggestProblem::Malformed), "{bad:?}");
        }
    }

    #[test]
    fn the_reason_is_one_plain_line() {
        assert_eq!(
            scrub_reason("  Your\nmail\t\tfrom   yesterday\r\nwould help. "),
            "Your mail from yesterday would help."
        );
        assert_eq!(
            scrub_reason("Your\u{202E}mail\u{200B} would\u{0007} help"),
            "Yourmail would help"
        );
    }

    #[test]
    fn addresses_and_links_are_left_out_of_the_reason() {
        assert_eq!(
            scrub_reason("Mail from kari@example.com at https://mail.google.com/x would help"),
            "Mail from at would help"
        );
        assert_eq!(
            scrub_reason("See www.example.com or mailto:kari"),
            "See or"
        );
    }

    #[test]
    fn the_reason_is_bounded_after_scrubbing() {
        let at_limit = "a".repeat(MAX_REASON_CHARS);
        assert_eq!(
            parse_args(&args("google", &at_limit)).map(|parsed| parsed.reason),
            Ok(at_limit.clone())
        );
        let over = "a".repeat(MAX_REASON_CHARS + 1);
        assert_eq!(parse_args(&args("google", &over)), Err(SuggestProblem::ReasonTooLong));
        // Whitespace the scrub folds away does not count against the limit.
        let padded = format!("{at_limit}\n\n\n   ");
        assert!(parse_args(&args("google", &padded)).is_ok());
        // The limit counts characters, not bytes.
        let wide = "å".repeat(MAX_REASON_CHARS);
        assert!(parse_args(&args("microsoft", &wide)).is_ok());
    }

    #[test]
    fn a_reason_with_nothing_left_is_refused() {
        for empty in ["", "   ", "\n\t", "kari@example.com", "https://example.com"] {
            assert_eq!(
                parse_args(&args("google", empty)),
                Err(SuggestProblem::EmptyReason),
                "{empty:?}"
            );
        }
    }

    #[test]
    fn the_description_states_the_rules() {
        for rule in ["at most once per turn", "never use it to compare or rank", "answer without it"] {
            assert!(TOOL_DESCRIPTION.contains(rule), "{rule}");
        }
    }
}
