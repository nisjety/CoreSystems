//! Choosing the part of a document the model reads for one chunk.
//!
//! A chunk is situated by what surrounds it, so the model has to read the
//! document. It cannot always read all of it: a document is arbitrary user
//! content and can be megabytes, and a prompt that size would either exceed the
//! model's context or cost hundreds of times what a normal document does. The old
//! code took the first N characters, which situated every chunk after the first
//! N characters of a long document by a part of the document it was not in.
//!
//! * A document of at most `window_chars` characters (32,000 by default, about
//!   8-10 thousand tokens) is the window, whole.
//! * A longer one is cut into fixed sections of `window_chars` characters that
//!   overlap by an eighth (4,000). A chunk goes to the section where it sits
//!   farthest from a cut edge, so the model reads it with context on both sides.
//!   Every section starts with the same header (the document's first sixteenth,
//!   2,000 characters, which usually names what the document is) unless the section
//!   already contains it, and marks where text was left out.
//!
//! Every chunk of a section gets the SAME window text. That is deliberate: the
//! window is the front of the prompt, and an identical prefix is what a provider's
//! automatic prefix cache needs to apply across the chunks of a document.
//!
//! The document's title is not part of the window text. The `ctx-v2` prompt carries
//! it as an attribute of the document block, in front of the window and the same for
//! every chunk (`ctx-v1`, held byte for byte to what shipped, has no place for it),
//! and the input hash takes it as its own field.
//!
//! ## Characters, never bytes
//!
//! The corpus is Norwegian-first, so a byte offset can fall inside an `ø`. Every
//! offset here is a character index, and every byte offset that is used to slice
//! comes from `char_indices`, so a slice cannot split a character. A character
//! index is turned into a byte offset through a sparse grid (one entry per 256
//! characters) rather than a table of every character: a multi-megabyte document
//! costs a few kilobytes of index, not eight bytes per character.
//!
//! ## Finding the chunk
//!
//! The chunk's text is searched for in the document. When it occurs more than once
//! (boilerplate repeats) the occurrence nearest the chunk's proportional position
//! wins. When it does not occur at all (a chunker that normalizes whitespace) the
//! proportional position itself is used, which is deterministic: the same document,
//! the same chunk and the same index always get the same window.

use std::borrow::Cow;
use std::sync::OnceLock;

/// The smallest window accepted. Below this a section's header and overlap are
/// too small to situate anything.
pub const MIN_WINDOW_CHARS: usize = 8_000;

/// One grid entry per this many characters.
const GRID: usize = 256;
/// Where text was left out of a window.
const ELIDED: &str = "[...]";
/// More occurrences than this are not compared: a chunk this repetitive carries no
/// position information, and a search that long is not worth its cost.
const MAX_OCCURRENCES: usize = 64;
/// Where a chunk is assumed to start, per index, when neither its text nor the
/// document's chunk count is known. Only ever a last resort.
const ASSUMED_CHUNK_CHARS: usize = 900;

/// The window size, from which the overlap and the header follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowConfig {
    window_chars: usize,
}

impl WindowConfig {
    pub fn new(window_chars: usize) -> Result<Self, String> {
        if window_chars < MIN_WINDOW_CHARS {
            return Err(format!(
                "CONTEXTUAL_RETRIEVAL_WINDOW_CHARS must be at least {MIN_WINDOW_CHARS}, got {window_chars}"
            ));
        }
        Ok(Self { window_chars })
    }

    pub const fn window_chars(&self) -> usize {
        self.window_chars
    }

    /// How far consecutive sections overlap: an eighth of the window (4,000 of
    /// 32,000).
    pub const fn overlap_chars(&self) -> usize {
        self.window_chars / 8
    }

    /// How much of the document's start every section carries: a sixteenth (2,000
    /// of 32,000).
    pub const fn header_chars(&self) -> usize {
        self.window_chars / 16
    }

    const fn stride(&self) -> usize {
        self.window_chars - self.overlap_chars()
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            window_chars: 32_000,
        }
    }
}

/// What kind of window a chunk got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    /// The whole document.
    Whole,
    /// One section of a long document, with its header.
    Section,
    /// A smaller window around the chunk, built when the provider said the input
    /// was too long.
    Shrunk,
}

/// The text the model reads for a chunk.
#[derive(Clone, Debug)]
pub struct Window<'a> {
    /// Identifies the window within its document: chunks with the same id share
    /// the same text. `0` for a whole document.
    pub id: usize,
    pub kind: WindowKind,
    pub text: Cow<'a, str>,
}

struct Section {
    start: usize,
    end: usize,
    text: OnceLock<String>,
}

/// A document, indexed for windowing. Cheap to build (one pass), cheap to keep.
pub struct WindowedDocument<'a> {
    text: &'a str,
    total_chars: usize,
    /// `grid[i]` is the byte offset of character `i * GRID`.
    grid: Vec<usize>,
    cfg: WindowConfig,
    /// Empty when the whole document is the window.
    sections: Vec<Section>,
}

struct Located {
    start: usize,
    len: usize,
}

impl<'a> WindowedDocument<'a> {
    pub fn new(text: &'a str, cfg: WindowConfig) -> Self {
        let mut grid = Vec::with_capacity(text.len() / GRID + 2);
        let mut total_chars = 0usize;
        for (char_index, (byte, _)) in text.char_indices().enumerate() {
            if char_index % GRID == 0 {
                grid.push(byte);
            }
            total_chars = char_index + 1;
        }
        if grid.is_empty() {
            grid.push(0);
        }

        let mut sections = Vec::new();
        if total_chars > cfg.window_chars() {
            let mut start = 0usize;
            loop {
                let end = (start + cfg.window_chars()).min(total_chars);
                sections.push(Section {
                    start,
                    end,
                    text: OnceLock::new(),
                });
                if end == total_chars {
                    break;
                }
                start += cfg.stride();
            }
        }
        Self {
            text,
            total_chars,
            grid,
            cfg,
            sections,
        }
    }

    pub fn total_chars(&self) -> usize {
        self.total_chars
    }

    /// True when the whole document is the window.
    pub fn is_whole(&self) -> bool {
        self.sections.is_empty()
    }

    pub fn section_count(&self) -> usize {
        self.sections.len().max(1)
    }

    /// The window for one chunk.
    ///
    /// `total_chunks` is how many chunks the document has, when the caller knows:
    /// it makes the proportional position (used only when the chunk's text cannot
    /// be found) exact. Without it the position is an estimate from the index.
    pub fn window_for(
        &self,
        chunk_text: &str,
        chunk_index: i32,
        total_chunks: Option<usize>,
    ) -> Window<'_> {
        if self.is_whole() {
            return Window {
                id: 0,
                kind: WindowKind::Whole,
                text: Cow::Borrowed(self.text),
            };
        }
        let located = self.locate(chunk_text, chunk_index, total_chunks);
        let id = self.section_for(&located);
        let section = &self.sections[id];
        let text = section.text.get_or_init(|| self.section_text(section));
        Window {
            id,
            kind: WindowKind::Section,
            text: Cow::Borrowed(text.as_str()),
        }
    }

    /// A window of half the size, centred on the chunk, for the one retry a
    /// provider's "input too long" earns. `None` when the document is no larger
    /// than that already: shrinking would change nothing.
    pub fn shrunk_for(
        &self,
        chunk_text: &str,
        chunk_index: i32,
        total_chunks: Option<usize>,
    ) -> Option<Window<'_>> {
        let size = self.cfg.window_chars() / 2;
        if self.total_chars <= size {
            return None;
        }
        let located = self.locate(chunk_text, chunk_index, total_chunks);
        let lead = size.saturating_sub(located.len) / 2;
        let start = located
            .start
            .saturating_sub(lead)
            .min(self.total_chars - size);
        let end = start + size;
        let mut text =
            String::with_capacity(self.byte_of_char(end) - self.byte_of_char(start) + 16);
        if start > 0 {
            text.push_str(ELIDED);
            text.push_str("\n\n");
        }
        text.push_str(self.slice(start, end));
        if end < self.total_chars {
            text.push_str("\n\n");
            text.push_str(ELIDED);
        }
        Some(Window {
            id: usize::MAX,
            kind: WindowKind::Shrunk,
            text: Cow::Owned(text),
        })
    }

    /// The section a chunk is read in: the one where it sits farthest from a cut
    /// edge. The edges of the document itself are not cuts.
    fn section_for(&self, located: &Located) -> usize {
        let start = located.start as i64;
        let end = (located.start + located.len) as i64;
        let mut best = (i64::MIN, 0usize);
        for (index, section) in self.sections.iter().enumerate() {
            let left = if section.start == 0 {
                i64::MAX
            } else {
                start - section.start as i64
            };
            let right = if section.end == self.total_chars {
                i64::MAX
            } else {
                section.end as i64 - end
            };
            let score = left.min(right);
            // Strictly greater: a tie goes to the earlier section, so the choice
            // never depends on anything but the position.
            if score > best.0 {
                best = (score, index);
            }
        }
        best.1
    }

    fn section_text(&self, section: &Section) -> String {
        let header = self.cfg.header_chars();
        let body = self.slice(section.start, section.end);
        let mut text = String::with_capacity(body.len() + header * 2 + 32);
        // A section that already begins inside the header does not repeat it.
        if section.start > header {
            text.push_str(self.slice(0, header));
            text.push_str("\n\n");
            text.push_str(ELIDED);
            text.push_str("\n\n");
        }
        text.push_str(body);
        if section.end < self.total_chars {
            text.push_str("\n\n");
            text.push_str(ELIDED);
        }
        text
    }

    fn locate(&self, chunk_text: &str, chunk_index: i32, total_chunks: Option<usize>) -> Located {
        let estimate = self.estimate(chunk_index, total_chunks);
        for needle in [chunk_text, chunk_text.trim()] {
            if needle.is_empty() {
                continue;
            }
            if let Some(byte) = self.nearest_occurrence(needle, estimate) {
                return Located {
                    start: self.char_of_byte(byte),
                    len: needle.chars().count(),
                };
            }
        }
        // Not found: the proportional position, deterministic.
        let len = chunk_text.chars().count().min(self.total_chars);
        let position = estimate.unwrap_or_else(|| {
            (usize::try_from(chunk_index.max(0)).unwrap_or(0)).saturating_mul(ASSUMED_CHUNK_CHARS)
        });
        Located {
            start: position.min(self.total_chars.saturating_sub(len)),
            len,
        }
    }

    /// Where the chunk is expected to start, when the document's chunk count is
    /// known.
    fn estimate(&self, chunk_index: i32, total_chunks: Option<usize>) -> Option<usize> {
        let total_chunks = total_chunks.filter(|count| *count > 0)?;
        let index = u128::try_from(chunk_index.max(0)).unwrap_or(0);
        let position = index * self.total_chars as u128 / total_chunks as u128;
        Some(usize::try_from(position).unwrap_or(self.total_chars))
    }

    /// The byte offset of the occurrence of `needle` nearest `estimate`, or the
    /// first one when there is no estimate.
    fn nearest_occurrence(&self, needle: &str, estimate: Option<usize>) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (byte, _) in self.text.match_indices(needle).take(MAX_OCCURRENCES) {
            let Some(estimate) = estimate else {
                return Some(byte);
            };
            let distance = self.char_of_byte(byte).abs_diff(estimate);
            if best.is_none_or(|(nearest, _)| distance < nearest) {
                best = Some((distance, byte));
            }
        }
        best.map(|(_, byte)| byte)
    }

    fn slice(&self, start_char: usize, end_char: usize) -> &str {
        &self.text[self.byte_of_char(start_char)..self.byte_of_char(end_char)]
    }

    /// The byte offset of character `index` (the end of the text when past it).
    fn byte_of_char(&self, index: usize) -> usize {
        if index >= self.total_chars {
            return self.text.len();
        }
        let base = self.grid[index / GRID];
        let skip = index % GRID;
        if skip == 0 {
            return base;
        }
        let rest = &self.text[base..];
        base + rest
            .char_indices()
            .nth(skip)
            .map_or(rest.len(), |(byte, _)| byte)
    }

    /// The character index of a byte offset that lies on a character boundary.
    fn char_of_byte(&self, byte: usize) -> usize {
        // `grid[0]` is 0, so at least one entry satisfies the predicate.
        let entry = self.grid.partition_point(|offset| *offset <= byte) - 1;
        entry * GRID + self.text[self.grid[entry]..byte].chars().count()
    }
}

#[cfg(test)]
mod tests;
