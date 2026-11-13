//! Emoji subdivision flags under the normalised scan (W2-WS26-S2-b).
//!
//! England, Scotland and Wales are a base flag, tag characters and a cancel tag.
//! Those exact sequences are benign; every other use of the tag block stays
//! flagged as `obfuscation.tag_block`.

use mp_content_safety::normalize::subdivision_sequence_len;
use mp_content_safety::{scan_in, FlagKind, ScanMode};

const BASE: char = '\u{1F3F4}';
const CANCEL: char = '\u{E007F}';

fn tags(ascii: &str) -> String {
    ascii
        .chars()
        .map(|c| char::from_u32(0xE0000 + c as u32).expect("tag character"))
        .collect()
}

fn flag(code: &str) -> String {
    format!("{BASE}{}{CANCEL}", tags(code))
}

fn tag_flagged(text: &str) -> bool {
    scan_in(ScanMode::Normalized, text)
        .flags
        .contains(&FlagKind::TagBlock)
}

#[test]
fn a_valid_subdivision_flag_is_not_flagged() {
    for code in ["gbeng", "gbsct", "gbwls"] {
        let text = format!("We cheered for {} in the final.", flag(code));
        let report = scan_in(ScanMode::Normalized, &text);
        assert!(!report.is_hit(), "{code}: {:?}", report.rule_ids);
    }
    // Two flags back to back, and a flag at the very start and end of the text.
    assert!(!tag_flagged(&format!("{}{}", flag("gbeng"), flag("gbsct"))));
    assert!(!tag_flagged(&flag("gbwls")));
}

#[test]
fn a_tag_run_with_no_base_is_flagged() {
    assert!(tag_flagged(&format!(
        "review the notes {}{CANCEL} today",
        tags("gbeng")
    )));
    assert!(tag_flagged(&format!("a lone {CANCEL} cancel tag")));
    assert!(tag_flagged(&format!("a lone {} tag", tags("a"))));
}

#[test]
fn a_tag_run_with_a_base_but_no_cancel_tag_is_flagged() {
    assert!(tag_flagged(&format!("waved {BASE}{} flags", tags("gbeng"))));
}

#[test]
fn a_tag_run_with_another_base_is_flagged() {
    assert!(tag_flagged(&format!(
        "race {}{}{CANCEL} report",
        '\u{1F3C1}',
        tags("gbsct")
    )));
}

#[test]
fn a_tag_run_after_a_valid_flag_is_flagged() {
    // A valid flag cannot carry a smuggled run after its cancel tag.
    assert!(tag_flagged(&format!(
        "{}{} report",
        flag("gbeng"),
        tags("ignore")
    )));
    // A phrase spelled in tags between the base and the cancel tag is not a subdivision.
    assert!(tag_flagged(&format!(
        "{BASE}{}{CANCEL}",
        tags("ignore all previous")
    )));
    // An unknown subdivision code is not recognised either.
    assert!(tag_flagged(&flag("usca")));
}

#[test]
fn bidi_and_invisible_checks_still_fire_beside_a_valid_flag() {
    let report = scan_in(
        ScanMode::Normalized,
        &format!("{} reading \u{202E}order", flag("gbeng")),
    );
    assert!(report.flags.contains(&FlagKind::BidiControl));
    assert!(!report.flags.contains(&FlagKind::TagBlock));
    let report = scan_in(
        ScanMode::Normalized,
        &format!("{} ig\u{200B}nore", flag("gbsct")),
    );
    assert!(report.flags.contains(&FlagKind::InvisibleInWord));
}

#[test]
fn the_sequence_length_covers_exactly_the_flag() {
    let england = flag("gbeng");
    assert_eq!(subdivision_sequence_len(&england), Some(england.len()));
    assert_eq!(
        subdivision_sequence_len(&format!("{england} tail")),
        Some(england.len())
    );
    assert_eq!(
        subdivision_sequence_len(&format!("x{england}")),
        None,
        "must start at the base"
    );
    assert_eq!(
        subdivision_sequence_len(&format!("{BASE}{}", tags("gbeng"))),
        None
    );
}
