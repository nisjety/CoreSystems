//! Checking what the model said before it is stored.
//!
//! The model's answer becomes data in three places: it is prepended to the chunk
//! that is embedded, it is written to `knowledge_units.chunk_context`, and the
//! keyword indexes tokenize it. An answer that is empty, markup, an essay, or cut
//! off mid-word degrades all three, and until now the only check was "non-empty
//! after trim", with an empty answer silently becoming "no context" and the stop
//! reason ignored altogether.
//!
//! Every rejection here is an explicit outcome with a content-free class; nothing
//! falls back quietly.
//!
//! ## The stop reason
//!
//! inference-core returns the provider's own finish reason. For Azure OpenAI that
//! is the raw `finish_reason` of the chat completion: `stop` for a complete answer,
//! `length` when the output cap cut it off, `content_filter` when the provider's
//! filter intervened. The Anthropic vocabulary (`end_turn`, `max_tokens`) is
//! accepted too, because the route is the Model Plane's to choose. An earlier design
//! required `end_turn`, which no Azure answer has ever carried, and would have
//! rejected every real context.
//!
//! An unknown reason (`tool_calls`, a future one, an empty one) is rejected: the
//! answer might be cut off and nothing says it is not.
//!
//! The model's output is untrusted text. It is only ever normalized and bounded
//! here; it is never executed, rendered as markup, or returned to a caller.

use super::outcome::{ContextOutcome, ErrorClass};

/// The shortest context kept. Below this the model said nothing a search could
/// use ("N/A", "Se dokumentet.").
pub const MIN_CHARS: usize = 12;
/// The longest context kept. The prompt asks for one or two sentences, at most
/// about 60 words; an answer far past that is an essay that would crowd the
/// chunk's own terms out of its embedding.
pub const MAX_CHARS: usize = 600;
/// When the output cap cut the answer off, it is shortened to its last full
/// sentence only if that sentence is at least this long; otherwise there is
/// nothing worth keeping.
pub const MIN_TRUNCATED_CHARS: usize = 20;

/// What a provider's finish reason says about the answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopKind {
    /// The model finished.
    Complete,
    /// The output cap cut the answer off.
    Truncated,
    /// The provider's content filter intervened.
    ContentFilter,
    /// Anything else, including empty.
    Unknown,
}

/// Classify a finish reason. Case and surrounding whitespace do not matter.
pub fn classify_stop_reason(raw: &str) -> StopKind {
    match raw.trim().to_ascii_lowercase().as_str() {
        "stop" | "end_turn" | "stop_sequence" => StopKind::Complete,
        "length" | "max_tokens" => StopKind::Truncated,
        "content_filter" | "refusal" => StopKind::ContentFilter,
        _ => StopKind::Unknown,
    }
}

/// An answer that passed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accepted {
    pub text: String,
    /// The output cap cut the answer off and it was shortened to its last full
    /// sentence.
    pub cut_at_sentence: bool,
    /// It opens with a phrase the prompt forbids. Counted, not rejected: a usable
    /// context with a poor opening is better stored than lost.
    pub meta_opening: bool,
}

/// Why an answer was not kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    Empty,
    TooShort,
    TooLong,
    /// The finish reason is not one this code knows to be complete.
    UnknownStopReason,
    /// The answer was cut off and holds no full sentence of useful length.
    TruncatedWithoutSentence,
    /// The provider's filter intervened. The text, if any, is not used.
    ContentFilter,
}

impl Rejection {
    pub const fn error_class(self) -> ErrorClass {
        match self {
            Self::ContentFilter => ErrorClass::ContentFilter,
            Self::Empty
            | Self::TooShort
            | Self::TooLong
            | Self::UnknownStopReason
            | Self::TruncatedWithoutSentence => ErrorClass::InvalidOutput,
        }
    }

    /// What the chunk's outcome is. A content-filter refusal is final and never
    /// retried (the same document would be refused again, and every attempt is
    /// paid for). Anything else is worth another try later: the output of a model
    /// is not bit-exact even at temperature 0.
    pub fn outcome(self) -> ContextOutcome {
        match self {
            Self::ContentFilter => ContextOutcome::permanent(ErrorClass::ContentFilter),
            other => ContextOutcome::transient(other.error_class(), None),
        }
    }
}

/// Phrases the V2 prompt forbids an answer to open with, lowercased. English, and
/// the Norwegian equivalents, since the corpus is Norwegian-first.
const META_OPENINGS: [&str; 12] = [
    "this chunk",
    "the chunk",
    "this section",
    "this passage",
    "this excerpt",
    "this part",
    "denne delen",
    "dette avsnittet",
    "denne seksjonen",
    "dette utdraget",
    "denne teksten",
    "denne biten",
];

/// Validate and normalize one answer.
pub fn validate(content: &str, stop_reason: &str) -> Result<Accepted, Rejection> {
    let stop = classify_stop_reason(stop_reason);
    if stop == StopKind::ContentFilter {
        return Err(Rejection::ContentFilter);
    }
    if stop == StopKind::Unknown {
        return Err(Rejection::UnknownStopReason);
    }

    let mut text = normalize(content);
    if text.is_empty() {
        return Err(Rejection::Empty);
    }

    let mut cut_at_sentence = false;
    if stop == StopKind::Truncated {
        match last_full_sentence(&text) {
            Some(end) => {
                if end < text.len() {
                    text.truncate(end);
                    cut_at_sentence = true;
                }
            }
            None => return Err(Rejection::TruncatedWithoutSentence),
        }
    }

    let length = text.chars().count();
    if length < MIN_CHARS {
        return Err(Rejection::TooShort);
    }
    if length > MAX_CHARS {
        return Err(Rejection::TooLong);
    }
    let meta_opening = has_meta_opening(&text);
    Ok(Accepted {
        text,
        cut_at_sentence,
        meta_opening,
    })
}

/// Markup removed, whitespace collapsed to single spaces, control and zero-width
/// characters dropped, ends trimmed.
fn normalize(raw: &str) -> String {
    let stripped = strip_tags(raw);
    let mut out = String::with_capacity(stripped.len());
    let mut gap = false;
    for character in stripped.chars() {
        if character.is_whitespace() {
            gap = !out.is_empty();
            continue;
        }
        if character.is_control() || is_invisible(character) {
            continue;
        }
        if gap {
            out.push(' ');
            gap = false;
        }
        out.push(character);
    }
    out
}

/// Zero-width and byte-order characters: invisible, and a way to make two
/// identical-looking contexts hash differently.
fn is_invisible(character: char) -> bool {
    matches!(character, '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}')
}

/// Remove anything that reads as an HTML or XML tag (`<b>`, `</document>`,
/// `<!-- -->`). A bare `<` that is not followed by a tag-like start and a closing
/// `>` ("x < 5 and y > 3") is text and stays. A removed tag leaves a space so the
/// words either side do not run together.
fn strip_tags(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let tag_like = after
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || matches!(first, '/' | '!' | '?'));
        match (tag_like, after.find('>')) {
            (true, Some(close)) => {
                out.push(' ');
                rest = &after[close + 1..];
            }
            _ => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The byte offset just past the last sentence terminator that ends a sentence of
/// at least [`MIN_TRUNCATED_CHARS`] characters. A terminator ends a sentence when
/// it is followed by whitespace or the end of the text, so "3.5" and "kl.14" do not
/// count.
fn last_full_sentence(text: &str) -> Option<usize> {
    let mut last = None;
    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if !matches!(character, '.' | '!' | '?' | '…') {
            continue;
        }
        let ends_sentence = characters
            .peek()
            .is_none_or(|(_, next)| next.is_whitespace());
        if ends_sentence {
            last = Some(index + character.len_utf8());
        }
    }
    last.filter(|end| text[..*end].chars().count() >= MIN_TRUNCATED_CHARS)
}

fn has_meta_opening(text: &str) -> bool {
    let head: String = text.chars().take(24).collect::<String>().to_lowercase();
    META_OPENINGS
        .iter()
        .any(|opening| head.starts_with(opening))
}

#[cfg(test)]
mod tests;
