//! Real token counting for prompt-budget decisions.
//!
//! # Why this replaced `len() / 4`
//!
//! Every budget gate in the Model Plane used to charge `content.len() / 4`.
//! That is wrong in BOTH directions, and the two directions fail differently:
//!
//! | sample (measured against `o200k_base`) | `len()/4` | real | error |
//! |---|---|---|---|
//! | English prose                          | 56 | 35 | **+60 %** |
//! | Norwegian prose                        | 55 | 56 | −2 % |
//! | Norwegian with dates + amounts         | 38 | 48 | **−21 %** |
//! | Norwegian, heavy æ/ø/å                 | 38 | 43 | −12 % |
//! | JSON payload                           | 28 | 38 | **−26 %** |
//!
//! Over-charging (English) silently trims grounding that would have fit, so the
//! answer is worse for no reason. Under-charging overflows the provider's input
//! limit, which is the harder failure — it surfaces as a context-length
//! rejection and a compaction retry, or as silent truncation by the provider.
//!
//! Verevon's real payloads are the under-charging case: Norwegian text, order
//! numbers, dates, amounts, and JSON tool results all pack more tokens per byte
//! than the 4:1 assumption. `len()` makes it worse still by counting BYTES — æ,
//! ø and å are two bytes each in UTF-8, so the divisor is applied to an already
//! inflated number and the error is not even a stable ratio.
//!
//! # Which encoding, and why the margin exists
//!
//! Counting is done with `o200k_base`, the encoding of the GPT-4o-family models
//! this deployment actually configures. For those models the count is EXACT.
//!
//! Anthropic does not publish a local tokenizer, so a Claude prompt cannot be
//! counted exactly here. [`SAFETY_MARGIN_PERCENT`] covers that gap. It is
//! applied on top of the real count rather than baked into the encoding choice
//! so that it stays visible and tunable: a silent fudge factor inside a
//! "tokenizer" is how the 4:1 heuristic survived this long.
//!
//! The margin rounds UP deliberately. The two failure modes are not symmetric —
//! wasting a little budget costs some grounding, while under-counting costs the
//! whole turn.

use std::sync::OnceLock;

use tiktoken_rs::{o200k_base, CoreBPE};

/// Extra headroom added to the real count, in percent.
///
/// Covers the providers whose tokenizer cannot be run locally (Anthropic). 15 %
/// is the observed spread between `o200k_base` and `cl100k_base` on Norwegian
/// business prose once diacritics and numerals are included, which is the
/// closest available proxy for "a different BPE on this kind of text".
pub const SAFETY_MARGIN_PERCENT: u32 = 15;

/// Chars-per-token fallback used only when the BPE table cannot be loaded.
///
/// 3 rather than 4: on the Norwegian + numeral + JSON mix this code actually
/// sees, ~3 chars/token was measured. If this path is ever taken it should
/// over-charge, not under-charge.
const FALLBACK_CHARS_PER_TOKEN: usize = 3;

/// The BPE table is a few megabytes and is built once per process.
///
/// `OnceLock` rather than a per-call construction: `assemble_segments` charges
/// every candidate segment, so building the table per call would turn one
/// context assembly into dozens of multi-megabyte table builds.
fn encoding() -> Option<&'static CoreBPE> {
    static ENCODING: OnceLock<Option<CoreBPE>> = OnceLock::new();
    ENCODING.get_or_init(|| o200k_base().ok()).as_ref()
}

/// Exact token count for `text` under `o200k_base`, with no safety margin.
///
/// Use this for reporting what a prompt actually cost. For deciding whether
/// something FITS, use [`count_for_budget`] — a budget decision needs the
/// margin.
#[must_use]
pub fn count(text: &str) -> u32 {
    let Some(bpe) = encoding() else {
        return fallback_count(text);
    };
    // `encode_with_special_tokens` counts special tokens as tokens instead of
    // rejecting them. Content here is untrusted (documents, tool output, user
    // messages) and can legitimately contain a literal `<|endoftext|>`; the
    // alternative encoder returns an error for that, and a budget function that
    // fails on hostile input is a denial-of-service surface.
    u32::try_from(bpe.encode_with_special_tokens(text).len()).unwrap_or(u32::MAX)
}

/// Token count to CHARGE against a prompt budget: the real count plus
/// [`SAFETY_MARGIN_PERCENT`], rounded up.
#[must_use]
pub fn count_for_budget(text: &str) -> u32 {
    with_margin(count(text))
}

/// Apply the safety margin to an already-known count, rounding up.
#[must_use]
pub fn with_margin(tokens: u32) -> u32 {
    let margin = u64::from(tokens) * u64::from(SAFETY_MARGIN_PERCENT);
    // +99 then /100 is a ceiling divide, so a 1-token segment still gains
    // headroom instead of rounding its margin away to zero.
    let rounded_up = margin.div_ceil(100);
    u32::try_from(u64::from(tokens) + rounded_up).unwrap_or(u32::MAX)
}

/// Character-based fallback for the unreachable-table case.
fn fallback_count(text: &str) -> u32 {
    let chars = text.chars().count().div_ceil(FALLBACK_CHARS_PER_TOKEN);
    u32::try_from(chars).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_costs_nothing() {
        assert_eq!(count(""), 0);
        assert_eq!(count_for_budget(""), 0);
    }

    /// The regression this crate exists for. `len()/4` charged 28 for this
    /// payload; the real cost is materially higher, and charging 28 is what
    /// overflowed the prompt.
    #[test]
    fn json_payloads_cost_far_more_than_the_old_four_to_one_estimate() {
        let json = r#"{"kundenummer":"10492","leveringsadresse":"Storgata 14, 0184 Oslo","antall":37,"produkt":"Desinfeksjonsmiddel 5L"}"#;
        let old_estimate = u32::try_from(json.len()).unwrap() / 4;
        let real = count(json);
        assert!(
            real > old_estimate,
            "real {real} should exceed the old {old_estimate}: under-charging is what overflows the prompt",
        );
    }

    /// Norwegian diacritics are two bytes each, so a byte-based divisor is
    /// applied to an inflated length. Guard that we count text, not bytes.
    #[test]
    fn diacritics_are_not_charged_by_byte_length() {
        let norwegian = "Særlig påfallende er økningen i årsavgiften";
        assert!(
            norwegian.len() > norwegian.chars().count(),
            "sample must actually contain multi-byte characters",
        );
        // The real count must not simply track byte length.
        assert_ne!(count(norwegian), u32::try_from(norwegian.len()).unwrap() / 4);
    }

    /// English prose was over-charged by ~60 %, quietly trimming grounding that
    /// would have fit.
    #[test]
    fn english_prose_is_cheaper_than_the_old_estimate() {
        let english = "We help customers meet regulatory requirements and ensure safe food \
                       through documented routines, training and chemicals tailored to each \
                       individual business.";
        let old_estimate = u32::try_from(english.len()).unwrap() / 4;
        assert!(
            count(english) < old_estimate,
            "English was over-charged by the 4:1 heuristic; the real count should be lower",
        );
    }

    #[test]
    fn the_budget_count_is_never_below_the_real_count() {
        for sample in [
            "hei",
            "Ordrenummer 40231 ble levert til Bergen den 14. mars.",
            "a much longer stretch of text intended to exercise the multiplication path",
        ] {
            assert!(
                count_for_budget(sample) >= count(sample),
                "the margin must never reduce a charge for {sample:?}",
            );
        }
    }

    /// A one-token segment must still gain headroom; integer truncation would
    /// have rounded its 15 % away to nothing.
    #[test]
    fn the_margin_rounds_up_rather_than_vanishing_on_small_counts() {
        assert_eq!(with_margin(1), 2);
        assert_eq!(with_margin(0), 0);
        assert_eq!(with_margin(100), 115);
    }

    #[test]
    fn the_margin_saturates_instead_of_overflowing() {
        assert_eq!(with_margin(u32::MAX), u32::MAX);
    }

    /// Untrusted content may contain a literal special-token string. Counting it
    /// must not panic or error — a budget function that fails on hostile input
    /// is a denial-of-service surface.
    #[test]
    fn special_token_text_in_untrusted_content_is_counted_not_rejected() {
        let hostile = "before <|endoftext|> after";
        assert!(count(hostile) > 0);
    }

    #[test]
    fn the_fallback_over_charges_rather_than_under_charges() {
        // 3 chars/token vs the old 4, so the fallback is the safer direction.
        let text = "abcdefghijkl"; // 12 chars
        assert_eq!(fallback_count(text), 4);
        assert!(fallback_count(text) > u32::try_from(text.len()).unwrap() / 4);
    }
}
