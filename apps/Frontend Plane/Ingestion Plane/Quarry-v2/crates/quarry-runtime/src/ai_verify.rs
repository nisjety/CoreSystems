//! Verbatim checks for model output (INNH-1 U2).
//!
//! A model's quote is only believed when Quarry finds it on the page. The
//! comparison is forgiving about things a model changes without meaning to
//! (runs of whitespace, no-break spaces, typographic quotes, zero-width
//! characters) and about nothing else: case, spelling and word order must
//! match. A found quote is returned as the page's own text with offsets into the
//! page, so the model's spelling of it never reaches the caller.
//!
//! Offsets are Unicode scalar (char) offsets, end-exclusive, into the text that
//! was searched: the same unit `Highlight::start` and `Highlight::end` use.

use quarry_core::ai_spec::{Highlight, HIGHLIGHT_MIN_QUOTE_CHARS};

/// Text folded for comparison, with the original char offset of every folded char.
#[derive(Debug, Clone)]
pub struct FoldedText {
    folded: String,
    /// `origin[i]` is the char offset in the original text of folded char `i`.
    origin: Vec<usize>,
    /// The original text, for cutting spans out of it.
    chars: Vec<char>,
}

/// One folded char: `None` when the char is dropped (zero-width), a space for
/// any whitespace, a plain quote for a typographic one.
fn fold_char(c: char) -> Option<char> {
    match c {
        '\u{200B}' | '\u{FEFF}' | '\u{00AD}' => None,
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' | '\u{2032}' => Some('\''),
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{2033}' => Some('"'),
        c if c.is_whitespace() => Some(' '),
        c => Some(c),
    }
}

/// Fold `text`: whitespace runs become one space, no-break and other Unicode
/// spaces become spaces, typographic quotes become plain ones, zero-width
/// characters go. Returns the folded text and the original char offset of each
/// folded char.
pub fn fold(text: &str) -> (String, Vec<usize>) {
    let mut folded = String::with_capacity(text.len());
    let mut origin = Vec::with_capacity(text.len());
    let mut last_space = false;
    for (index, c) in text.chars().enumerate() {
        let Some(c) = fold_char(c) else { continue };
        if c == ' ' {
            if last_space {
                continue;
            }
            last_space = true;
        } else {
            last_space = false;
        }
        folded.push(c);
        origin.push(index);
    }
    (folded, origin)
}

impl FoldedText {
    pub fn new(text: &str) -> Self {
        let (folded, origin) = fold(text);
        Self {
            folded,
            origin,
            chars: text.chars().collect(),
        }
    }

    /// Number of chars in the original text.
    pub fn char_len(&self) -> usize {
        self.chars.len()
    }

    /// The first place `quote` occurs, as `(start, end)` char offsets into the
    /// original text. `None` for a quote that is empty after folding or that is
    /// not there.
    pub fn find(&self, quote: &str) -> Option<(usize, usize)> {
        let (needle, _) = fold(quote);
        let needle = needle.trim();
        if needle.is_empty() {
            return None;
        }
        let byte = self.folded.find(needle)?;
        let start = self.folded[..byte].chars().count();
        let len = needle.chars().count();
        let first = *self.origin.get(start)?;
        let last = *self.origin.get(start + len - 1)?;
        Some((first, last + 1))
    }

    /// The original text between two char offsets.
    pub fn slice(&self, start: usize, end: usize) -> String {
        self.chars[start.min(self.chars.len())..end.min(self.chars.len())]
            .iter()
            .collect()
    }
}

/// [`FoldedText::find`] for a single search.
pub fn find_verbatim(source: &str, quote: &str) -> Option<(usize, usize)> {
    FoldedText::new(source).find(quote)
}

/// A quote counts as grounded when it is at least [`HIGHLIGHT_MIN_QUOTE_CHARS`]
/// long (folded) and found on the page. Shorter ones are too easy to find by
/// chance to prove anything.
pub fn is_grounded(page: &FoldedText, quote: &str) -> bool {
    let (folded, _) = fold(quote);
    folded.trim().chars().count() >= HIGHLIGHT_MIN_QUOTE_CHARS && page.find(quote).is_some()
}

/// What [`verify_highlights`] kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightVerdict {
    /// Ordered by position, non-overlapping.
    pub highlights: Vec<Highlight>,
    /// Quotes the model gave that are not on the page (or are too short to
    /// count). Duplicates and overlaps are removed silently and not counted.
    pub dropped: u32,
}

/// Keep the quotes that are verbatim on the page.
///
/// Quotes are taken in the model's order until `max_highlights` are kept. A
/// quote that is found but longer than `max_chars_each` is cut to it (the cut is
/// still verbatim); one that overlaps a kept span is skipped. The result is
/// ordered by position.
pub fn verify_highlights(
    page: &FoldedText,
    quotes: &[String],
    max_highlights: usize,
    max_chars_each: usize,
) -> HighlightVerdict {
    let mut kept: Vec<(usize, usize)> = Vec::new();
    let mut dropped = 0u32;
    for quote in quotes {
        if kept.len() >= max_highlights {
            break;
        }
        let (folded, _) = fold(quote);
        let found = if folded.trim().chars().count() >= HIGHLIGHT_MIN_QUOTE_CHARS {
            page.find(quote)
        } else {
            None
        };
        let Some((start, mut end)) = found else {
            dropped += 1;
            continue;
        };
        if end - start > max_chars_each {
            end = start + max_chars_each;
        }
        if kept.iter().any(|&(s, e)| start < e && s < end) {
            continue;
        }
        kept.push((start, end));
    }
    kept.sort_unstable();
    HighlightVerdict {
        highlights: kept
            .into_iter()
            .map(|(start, end)| Highlight {
                text: page.slice(start, end),
                start: start as u64,
                end: end as u64,
            })
            .collect(),
        dropped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_collapses_whitespace_and_quotes() {
        let (folded, origin) = fold("a \u{00A0}\n b\u{200B}c \u{201C}x\u{201D} \u{2019}");
        assert_eq!(folded, "a bc \"x\" '");
        assert_eq!(origin.len(), folded.chars().count());
        // The 'b' sat at original char 5, the 'c' at 7 (the zero-width char was between).
        assert_eq!(origin[2], 5);
        assert_eq!(origin[3], 7);
    }

    #[test]
    fn find_gives_char_offsets_of_the_original() {
        let page = "Prisen for varen er 199 kr.  Levering\u{00A0}tar to dager.";
        let (start, end) = find_verbatim(page, "Levering tar to dager.").expect("found");
        let page_text = FoldedText::new(page);
        // The span is the page's own spelling, including the no-break space.
        assert_eq!(page_text.slice(start, end), "Levering\u{00A0}tar to dager.");
        assert_eq!(end, page.chars().count());
    }

    #[test]
    fn offsets_count_chars_not_bytes() {
        let page = "Blåbærsyltetøy på bordet. Æøå er norske bokstaver, og de er tre.";
        let (start, end) = find_verbatim(page, "Æøå er norske bokstaver").expect("found");
        assert_eq!(start, 26);
        assert_eq!(end, 26 + "Æøå er norske bokstaver".chars().count());
        assert!(page.len() > page.chars().count(), "the fixture has multi-byte chars");
        assert_eq!(FoldedText::new(page).slice(start, end), "Æøå er norske bokstaver");
    }

    #[test]
    fn case_spelling_and_order_must_match() {
        let page = "The quick brown fox jumps over the lazy dog.";
        assert!(find_verbatim(page, "the quick brown fox").is_none());
        assert!(find_verbatim(page, "quick brown dog").is_none());
        assert!(find_verbatim(page, "").is_none());
        assert!(find_verbatim(page, "  \u{00A0} ").is_none());
        assert!(find_verbatim(page, "quick   brown\nfox").is_some());
    }

    #[test]
    fn typographic_quotes_in_the_page_match_plain_ones_in_the_quote() {
        let page = "Han sa \u{201C}det er ferdig\u{201D} og gikk.";
        let (start, end) = find_verbatim(page, "Han sa \"det er ferdig\" og gikk.").expect("found");
        assert_eq!((start, end), (0, page.chars().count()));
    }

    #[test]
    fn highlights_keep_only_verbatim_spans_and_count_the_rest() {
        let page = FoldedText::new(
            "Alpha beta gamma delta. Epsilon zeta eta theta. Iota kappa lambda mu nu.",
        );
        let quotes = vec![
            "Epsilon zeta eta theta.".to_owned(),     // kept
            "Fabricated sentence here".to_owned(),    // not on the page
            "Alpha beta gamma delta.".to_owned(),     // kept, earlier on the page
            "Epsilon zeta eta theta.".to_owned(),     // duplicate: overlaps, not counted
            "short".to_owned(),                       // under the minimum: dropped
            "zeta eta theta. Iota kappa".to_owned(),  // overlaps the first kept span
        ];
        let verdict = verify_highlights(&page, &quotes, 5, 300);
        let texts: Vec<&str> = verdict.highlights.iter().map(|h| h.text.as_str()).collect();
        assert_eq!(texts, ["Alpha beta gamma delta.", "Epsilon zeta eta theta."]);
        assert_eq!(verdict.dropped, 2);
        assert!(verdict.highlights[0].end <= verdict.highlights[1].start);
    }

    #[test]
    fn highlights_respect_the_count_and_cut_long_spans() {
        let page = FoldedText::new("One two three four five six seven eight nine ten eleven twelve.");
        let quotes = vec![
            "One two three four five six seven eight nine ten eleven twelve.".to_owned(),
            "never reached".to_owned(),
        ];
        let verdict = verify_highlights(&page, &quotes, 1, 20);
        assert_eq!(verdict.highlights.len(), 1);
        assert_eq!(verdict.highlights[0].text, "One two three four f");
        assert_eq!(verdict.highlights[0].end - verdict.highlights[0].start, 20);
        assert_eq!(verdict.dropped, 0, "the loop stopped at the count");
    }

    #[test]
    fn grounded_needs_length_and_presence() {
        let page = FoldedText::new("Oslo er hovedstaden i Norge.");
        assert!(is_grounded(&page, "Oslo er hovedstaden"));
        assert!(!is_grounded(&page, "Oslo"), "too short to prove anything");
        assert!(!is_grounded(&page, "Bergen er hovedstaden"));
    }
}
