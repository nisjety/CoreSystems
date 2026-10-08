//! The prompt a chunk's situating context is generated from, in its versions, and
//! the hash that says whether a stored context is still the answer to it.
//!
//! ## Versions
//!
//! * [`PromptVersion::V1`] (`ctx-v1`) is the prompt the first generation of this
//!   feature shipped, byte for byte (a golden test holds it there). Every context
//!   stored so far was produced by it, so it stays the default: changing what the
//!   default produces would change what a re-embed stores without anyone choosing
//!   it.
//! * [`PromptVersion::V2`] (`ctx-v2`) is behind an environment pin or an
//!   organization's policy and ships only through the evaluation gate. It differs
//!   in three ways. The document block comes FIRST and is identical for every
//!   chunk of a window, then the chunk, then the instruction, so a provider's
//!   automatic prefix cache applies across a document's chunks (Azure caches
//!   prefixes of 1,024 tokens or more; best effort, measured not assumed). The
//!   document carries its title, source and date as attributes. The instruction is
//!   tighter: at most about 60 words, the chunk's own language, no "This chunk".
//!
//! ## What the prompt is not
//!
//! Document text is untrusted. It can say "ignore the above". The generated
//! context is never returned to a caller and only changes how that organization's
//! own chunk ranks, so an injection can at worst mislead the ranking of a tenant's
//! own content. What V2 does do is keep the document's text from closing its own
//! block early (`</document>` inside the text is neutralized) and keep the
//! attribute values from breaking out of their quotes.

use sha2::{Digest, Sha256};

/// Bumped whenever a prompt changes in a way that alters the generated context.
pub const PROMPT_V1: &str = "ctx-v1";
pub const PROMPT_V2: &str = "ctx-v2";

/// Which prompt generated (or is to generate) a context.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PromptVersion {
    V1,
    V2,
}

impl PromptVersion {
    pub const ALL: [PromptVersion; 2] = [PromptVersion::V1, PromptVersion::V2];

    /// The value stored in `knowledge_units.context_prompt_version`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1 => PROMPT_V1,
            Self::V2 => PROMPT_V2,
        }
    }

    /// Parse an environment pin or an organization policy value. Unknown values are
    /// an error, never a silent fallback: a typo must not quietly keep the default.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            PROMPT_V1 => Ok(Self::V1),
            PROMPT_V2 => Ok(Self::V2),
            other => Err(format!(
                "unknown contextual retrieval prompt version `{other}` (expected `{PROMPT_V1}` or `{PROMPT_V2}`)"
            )),
        }
    }
}

/// What the document says about itself, for the V2 document block. An empty string
/// is an absent attribute.
#[derive(Clone, Copy, Debug, Default)]
pub struct DocumentMeta<'a> {
    pub title: &'a str,
    pub source: &'a str,
    pub date: &'a str,
}

/// The longest an attribute value is carried. A title is user content.
const MAX_ATTRIBUTE_CHARS: usize = 200;

/// The V1 instruction. Deliberately constrains the model to *situating*
/// information only: a summary or an interpretation would add tokens that compete
/// with the chunk's own terms in the embedding, which is the opposite of the goal.
fn build_prompt_v1(document_text: &str, chunk_text: &str) -> String {
    format!(
        "Here is a document:\n\n<document>\n{document_text}\n</document>\n\n\
         Here is a chunk taken from that document:\n\n<chunk>\n{chunk_text}\n</chunk>\n\n\
         Give one or two short sentences that situate this chunk within the \
         document, so the chunk can be found by search. Name the concrete \
         entities, dates, sections, or subjects the chunk refers to only \
         implicitly. Do not summarise the chunk, do not interpret it, and do \
         not add anything that is not in the document. Reply with the situating \
         sentences alone and nothing else."
    )
}

/// The V2 instruction. English on purpose (the model follows it in any language)
/// and explicit about the chunk's language, because a Norwegian chunk situated by
/// an English sentence embeds worse than one situated in Norwegian.
const INSTRUCTION_V2: &str = "Write one or two sentences, at most 60 words, that situate the chunk within the document so that a search can find it. \
Write in the same language as the chunk. \
Start directly with the content; never start with \"This chunk\", \"The chunk\" or \"This section\" (or the same phrase in the chunk's language). \
Name the subject, section, entities and dates that the chunk only implies. \
Do not summarise or interpret the chunk, and do not add anything that is not in the document. \
Reply with the sentences alone.";

fn build_prompt_v2(meta: &DocumentMeta<'_>, window_text: &str, chunk_text: &str) -> String {
    let mut attributes = String::new();
    for (name, value) in [
        ("title", meta.title),
        ("source", meta.source),
        ("date", meta.date),
    ] {
        let value = attribute_value(value);
        if !value.is_empty() {
            attributes.push_str(&format!(" {name}=\"{value}\""));
        }
    }
    format!(
        "<document{attributes}>\n{}\n</document>\n\n<chunk>\n{}\n</chunk>\n\n{INSTRUCTION_V2}",
        neutralize_closing_tags(window_text),
        neutralize_closing_tags(chunk_text),
    )
}

/// The prompt for one chunk.
///
/// `window_text` is the part of the document the model reads (the whole document,
/// or one section of a large one with its header). It must be the same string for
/// every chunk that shares the window: for V2 that is what makes the block a cacheable
/// prefix.
pub fn build_prompt(
    version: PromptVersion,
    meta: &DocumentMeta<'_>,
    window_text: &str,
    chunk_text: &str,
) -> String {
    match version {
        PromptVersion::V1 => build_prompt_v1(window_text, chunk_text),
        PromptVersion::V2 => build_prompt_v2(meta, window_text, chunk_text),
    }
}

/// An attribute value that cannot break out of its quotes: control characters and
/// angle brackets removed, double quotes replaced, whitespace collapsed, bounded
/// by characters (never bytes: titles are Norwegian-first).
fn attribute_value(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|character| match character {
            '"' => '\'',
            '<' | '>' => ' ',
            character if character.is_control() => ' ',
            character => character,
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(MAX_ATTRIBUTE_CHARS).collect()
}

/// Keep text from closing the block it sits in. `</document` and `</chunk` (any
/// case) become `&lt;/document` and `&lt;/chunk`; everything else is untouched.
fn neutralize_closing_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("</") {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 2..];
        let closes_a_block = ["document", "chunk"].iter().any(|name| {
            tail.get(..name.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(name))
        });
        out.push_str(if closes_a_block { "&lt;/" } else { "</" });
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Joins the generated context to the chunk to form the text that gets embedded.
///
/// A blank line separator, and context first: the retrieval query matches against
/// the whole string, and leading context is what makes an otherwise-anonymous
/// chunk match an entity-bearing query.
pub fn compose_contextualized(context: &str, chunk_text: &str) -> String {
    format!("{}\n\n{}", context.trim(), chunk_text)
}

/// What a stored context was generated from, as a lowercase hex sha256.
///
/// It decides reuse and staleness: a context whose hash matches the inputs now is
/// still the answer and is not paid for again (a redelivered message, a re-embed of
/// unchanged content, a reconciler pass), and one whose hash differs is stale. The
/// inputs are the prompt version, the configured model alias, the window the model
/// read, the document's title and the chunk's text. The document's source and date
/// are in the V2 prompt but not in the hash: a refreshed timestamp does not change
/// what the chunk is about.
///
/// The encoding is a tag, then each field as an 8-byte little-endian length and its
/// bytes, so that `("ab", "c")` and `("a", "bc")` cannot collide. The tag names the
/// layout; a different layout is a different tag, never a silent change.
pub fn input_hash(
    version: PromptVersion,
    model: &str,
    window_text: &str,
    title: &str,
    chunk_text: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ctx-input-v1");
    for field in [version.as_str(), model, window_text, title, chunk_text] {
        hasher.update((field.len() as u64).to_le_bytes());
        hasher.update(field.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// `sha256` of a context exactly as it is stored, lowercase hex: the same bytes
/// the database hashes with `context_text_hash()` (`encode(sha256(convert_to(text,
/// 'UTF8')), 'hex')`), and what `embedded_context_hash` is compared against. It does
/// not trim: a validated context is already normalized, so the stored string and the
/// hashed string are one string, and hashing something else would read as an
/// incoherent vector.
pub fn stored_context_hash(context: &str) -> String {
    format!("{:x}", Sha256::digest(context.as_bytes()))
}

#[cfg(test)]
mod tests;
