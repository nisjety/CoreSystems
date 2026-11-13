//! Emoji subdivision flags inside `flag_raw_characters` (W2-WS26-S2-b step 4).
//!
//! A valid England, Scotland or Wales flag is not a tag-block hit; every other
//! tag character is, including one placed right after a valid flag. The bidi and
//! invisible checks are not weakened by a flag nearby.

use super::*;

const BASE: char = '\u{1F3F4}';
const CANCEL: char = '\u{E007F}';

fn tags(ascii: &str) -> String {
    ascii
        .chars()
        .map(|c| char::from_u32(0xE0000 + c as u32).expect("tag character"))
        .collect()
}

fn raw_flags(text: &str) -> Vec<FlagKind> {
    let mut report = ScanReport::default();
    flag_raw_characters(text, &mut report);
    report.flags
}

#[test]
fn a_valid_subdivision_is_not_flagged() {
    for code in ["gbeng", "gbsct", "gbwls"] {
        let text = format!("go {BASE}{}{CANCEL} go", tags(code));
        assert!(!raw_flags(&text).contains(&FlagKind::TagBlock), "{code}");
        assert!(!scan_in(ScanMode::Normalized, &text).is_hit(), "{code}");
    }
}

#[test]
fn a_tag_run_with_no_base_is_flagged() {
    let text = format!("go {}{CANCEL} go", tags("gbeng"));
    assert!(raw_flags(&text).contains(&FlagKind::TagBlock));
}

#[test]
fn a_tag_run_with_a_base_but_no_cancel_tag_is_flagged() {
    let text = format!("go {BASE}{} go", tags("gbeng"));
    assert!(raw_flags(&text).contains(&FlagKind::TagBlock));
}

#[test]
fn a_tag_smuggled_right_after_a_valid_flag_is_flagged() {
    let text = format!("{BASE}{}{CANCEL}{}", tags("gbeng"), tags("hi"));
    assert!(raw_flags(&text).contains(&FlagKind::TagBlock));
    // An unknown subdivision code after the base stays flagged.
    let other = format!("{BASE}{}{CANCEL}", tags("usca"));
    assert!(raw_flags(&other).contains(&FlagKind::TagBlock));
}

#[test]
fn a_flag_does_not_hide_a_bidi_control_or_an_invisible_in_a_word() {
    let flag = format!("{BASE}{}{CANCEL}", tags("gbsct"));
    let bidi = raw_flags(&format!("{flag}\u{202E}order"));
    assert!(bidi.contains(&FlagKind::BidiControl));
    assert!(!bidi.contains(&FlagKind::TagBlock));
    let invisible = raw_flags(&format!("{flag} ig\u{200B}nore"));
    assert!(invisible.contains(&FlagKind::InvisibleInWord));
}
