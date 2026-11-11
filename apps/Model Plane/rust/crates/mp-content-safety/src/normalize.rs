//! Folding applied before any marker is looked for. Each step closes one way
//! a payload hides a phrase from a plain substring match.

use unicode_normalization::{is_nfkc_quick, IsNormalized, UnicodeNormalization};

/// Zero-width and other invisible characters. They split a word without
/// showing anything, so they are removed before matching. Soft hyphen and the
/// directional marks LRM and RLM are removed silently; only the override
/// controls (see [`is_bidi_override`]) are flagged.
#[must_use]
pub fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2060}'..='\u{2064}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{E0100}'..='\u{E01EF}'
    )
}

/// The Unicode tag block. It has no visible glyph, and no ordinary text uses
/// it, so every occurrence is a flag, except the tag characters of an emoji
/// subdivision flag (see [`subdivision_sequence_len`]).
#[must_use]
pub fn is_tag(c: char) -> bool {
    ('\u{E0000}'..='\u{E007F}').contains(&c)
}

/// The base of an emoji tag sequence: the waving black flag.
const TAG_SEQUENCE_BASE: char = '\u{1F3F4}';

/// The cancel tag that ends an emoji tag sequence.
const CANCEL_TAG: char = '\u{E007F}';

/// The tag characters that spell England (`gbeng`), Scotland (`gbsct`) and
/// Wales (`gbwls`): the lowercase ISO 3166-2 codes without their hyphen. They
/// are the only subdivision flags Unicode defines as emoji.
const ENGLAND_TAGS: [char; 5] = ['\u{E0067}', '\u{E0062}', '\u{E0065}', '\u{E006E}', '\u{E0067}'];
const SCOTLAND_TAGS: [char; 5] = ['\u{E0067}', '\u{E0062}', '\u{E0073}', '\u{E0063}', '\u{E0074}'];
const WALES_TAGS: [char; 5] = ['\u{E0067}', '\u{E0062}', '\u{E0077}', '\u{E006C}', '\u{E0073}'];

/// The byte length of the emoji subdivision flag at the start of `text`, or
/// `None` when `text` does not start with one. The flag is the waving black
/// flag, then the tag characters of England, Scotland or Wales, then the cancel
/// tag.
///
/// Only those three codes are recognised, not every run of tag characters after
/// the base. A broader rule would let a phrase ride inside one run, or be split
/// across several. The folded text has no tag characters, so the tag-block flag
/// is the only thing that would catch such a phrase. Any other run, a different
/// base, a lone tag character or a missing cancel tag returns `None`.
#[must_use]
pub fn subdivision_sequence_len(text: &str) -> Option<usize> {
    let body = text.strip_prefix(TAG_SEQUENCE_BASE)?;
    [&ENGLAND_TAGS[..], &SCOTLAND_TAGS[..], &WALES_TAGS[..]]
        .into_iter()
        .find_map(|code| {
            let mut rest = body.chars();
            let complete = code.iter().all(|&tag| rest.next() == Some(tag)) && rest.next() == Some(CANCEL_TAG);
            complete.then(|| text.len() - rest.as_str().len())
        })
}

/// Bidirectional override and isolate controls: the characters that make
/// displayed text read differently from its stored order.
#[must_use]
pub fn is_bidi_override(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Lowercase Latin look-alikes from Cyrillic and Greek. Applied only after the
/// plain fold, and only used when it completes a marker the plain fold did
/// not, so genuine Russian or Greek text is never rewritten for its own sake.
#[must_use]
pub fn confusable_fold(c: char) -> char {
    match c {
        'а' | 'α' => 'a',
        'е' => 'e',
        'о' | 'ο' => 'o',
        'р' | 'ρ' => 'p',
        'с' => 'c',
        'у' | 'υ' => 'y',
        'х' | 'χ' => 'x',
        'і' | 'ι' => 'i',
        'ј' => 'j',
        'ѕ' => 's',
        'һ' => 'h',
        'ԁ' => 'd',
        'ԛ' => 'q',
        'ԝ' => 'w',
        'ѵ' => 'v',
        'ν' => 'v',
        'κ' => 'k',
        'ɡ' => 'g',
        other => other,
    }
}

/// Plain fold: strip invisible, tag and bidi characters, compatibility-normalise
/// (NFKC, so fullwidth and ligature forms become ASCII), lowercase, and collapse
/// every whitespace run to one space. Line breaks inside a phrase therefore
/// match the same phrase written on one line.
#[must_use]
pub fn fold(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    stripped.extend(
        text.chars()
            .filter(|&c| !is_invisible(c) && !is_tag(c) && !is_bidi_override(c)),
    );
    // The quick check answers Yes for nearly all real text, and then NFKC is
    // the identity; the full pass is only paid when the text needs it.
    let normalised: String = if is_nfkc_quick(stripped.chars()) == IsNormalized::Yes {
        stripped
    } else {
        stripped.nfkc().collect()
    };
    collapse_whitespace(&normalised.to_lowercase())
}

/// Applies [`confusable_fold`] to an already plain-folded string.
#[must_use]
pub fn fold_confusables(folded: &str) -> String {
    folded.chars().map(confusable_fold).collect()
}

/// The text a model should read in place of a payload that may carry hidden
/// characters: the same text with every invisible, tag and bidi-override
/// character removed. Visible characters are untouched; no case folding or
/// compatibility normalisation is applied.
#[must_use]
pub fn sanitize_for_model(text: &str) -> String {
    text.chars()
        .filter(|&c| !is_invisible(c) && !is_tag(c) && !is_bidi_override(c))
        .collect()
}

fn collapse_whitespace(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !collapsed.is_empty() {
            collapsed.push(' ');
        }
        collapsed.push_str(word);
    }
    collapsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_width_characters_are_removed_before_matching() {
        assert_eq!(fold("ign\u{200B}ore"), "ignore");
        assert_eq!(fold("ig\u{00AD}nore\u{2060}"), "ignore");
    }

    #[test]
    fn fullwidth_and_ligature_forms_fold_to_ascii() {
        assert_eq!(fold("ｉｇｎｏｒｅ ｐｒｅｖｉｏｕｓ"), "ignore previous");
        assert_eq!(fold("ﬁnal"), "final");
    }

    #[test]
    fn whitespace_runs_collapse_so_line_breaks_do_not_hide_a_phrase() {
        assert_eq!(fold("ignore\tprevious\n\ninstructions"), "ignore previous instructions");
    }

    #[test]
    fn confusables_fold_only_in_the_second_pass() {
        let plain = fold("оrder");
        assert_eq!(plain, "оrder", "the plain fold leaves Cyrillic alone");
        assert_eq!(fold_confusables(&plain), "order");
    }

    #[test]
    fn sanitize_removes_hidden_characters_and_keeps_visible_text() {
        let hidden = "Hello\u{E0041}\u{202E} world\u{200B}!";
        assert_eq!(sanitize_for_model(hidden), "Hello world!");
    }
}
