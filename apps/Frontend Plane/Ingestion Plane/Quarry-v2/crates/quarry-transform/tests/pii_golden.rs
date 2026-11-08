//! Golden corpus for the PII validators and scanner (E3.6-1).
//!
//! `tests/fixtures/pii/golden.json` is the specification. Its validator cases
//! fix each validator's verdict, and its text cases fix the exact findings
//! (kind, then matched text, in text order) that `scan` must return. Nothing
//! here is wired into a runtime path.
//!
//! The tests are named `pii_golden_*` so `cargo test -p quarry-transform pii_golden`
//! selects them.

use quarry_transform::pii::{self, PiiKind};
use serde::Deserialize;

const GOLDEN: &str = include_str!("fixtures/pii/golden.json");
const SCHEMA: &str = "quarry-pii-golden/1";

const ALL_KINDS: [PiiKind; 5] = [
    PiiKind::Email,
    PiiKind::Phone,
    PiiKind::Iban,
    PiiKind::PaymentCard,
    PiiKind::Fodselsnummer,
];

#[derive(Deserialize)]
struct Golden {
    schema: String,
    validators: Vec<ValidatorCase>,
    cases: Vec<TextCase>,
}

#[derive(Deserialize)]
struct ValidatorCase {
    kind: String,
    input: String,
    valid: bool,
    #[serde(default)]
    note: String,
}

#[derive(Deserialize)]
struct TextCase {
    id: String,
    group: String,
    text: String,
    expect: Vec<Expected>,
}

#[derive(Deserialize)]
struct Expected {
    kind: String,
    text: String,
}

fn golden() -> Golden {
    serde_json::from_str(GOLDEN).expect("golden corpus parses")
}

fn kind_from_label(label: &str) -> PiiKind {
    match label {
        "email" => PiiKind::Email,
        "phone" => PiiKind::Phone,
        "iban" => PiiKind::Iban,
        "card" => PiiKind::PaymentCard,
        "fodselsnummer" => PiiKind::Fodselsnummer,
        other => panic!("unknown kind label in the golden corpus: {other}"),
    }
}

fn validate(kind: PiiKind, input: &str) -> bool {
    match kind {
        PiiKind::Email => pii::is_email(input),
        PiiKind::Phone => pii::is_phone(input),
        PiiKind::Iban => pii::is_iban(input),
        PiiKind::PaymentCard => pii::is_payment_card(input),
        PiiKind::Fodselsnummer => pii::is_fodselsnummer(input),
    }
}

#[test]
fn pii_golden_corpus_header_is_the_expected_schema() {
    let g = golden();
    assert_eq!(g.schema, SCHEMA);
    assert!(!g.validators.is_empty());
    assert!(!g.cases.is_empty());
}

#[test]
fn pii_golden_validators_match_their_verdicts() {
    let g = golden();
    let mut failures = Vec::new();
    for case in &g.validators {
        let kind = kind_from_label(&case.kind);
        if validate(kind, &case.input) != case.valid {
            failures.push(format!(
                "{} {:?} expected valid={} ({})",
                case.kind, case.input, case.valid, case.note
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "validator mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn pii_golden_scan_finds_exactly_the_expected_spans() {
    let g = golden();
    let mut failures = Vec::new();
    for case in &g.cases {
        let got: Vec<(String, String)> = pii::scan(&case.text)
            .iter()
            .map(|f| {
                (
                    f.kind.label().to_string(),
                    case.text[f.start..f.end].to_string(),
                )
            })
            .collect();
        let want: Vec<(String, String)> = case
            .expect
            .iter()
            .map(|e| (e.kind.clone(), e.text.clone()))
            .collect();
        if got != want {
            failures.push(format!("{}: want {:?}, got {:?}", case.id, want, got));
        }
    }
    assert!(
        failures.is_empty(),
        "scan mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn pii_golden_redact_removes_every_expected_match() {
    let g = golden();
    let mut failures = Vec::new();
    for case in &g.cases {
        let out = pii::redact(&case.text);
        if case.expect.is_empty() {
            if out != case.text {
                failures.push(format!("{}: text with no findings was changed", case.id));
            }
            continue;
        }
        for e in &case.expect {
            if out.contains(&e.text) {
                failures.push(format!("{}: {:?} survived redaction", case.id, e.text));
            }
            let kind = kind_from_label(&e.kind);
            if !out.contains(kind.marker()) {
                failures.push(format!(
                    "{}: no {} marker in output",
                    case.id,
                    kind.marker()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "redaction failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn pii_golden_redact_copies_text_outside_findings_byte_for_byte() {
    let text = "Søknad, 12058512336 og ingen ting mer. Ring +47 22 33 44 55!";
    let out = pii::redact(text);
    assert_eq!(
        out,
        "Søknad, [redacted:pii.fodselsnummer] og ingen ting mer. Ring [redacted:pii.phone]!"
    );
}

#[test]
fn pii_golden_corpus_covers_every_kind_with_both_verdicts() {
    let g = golden();
    for kind in ALL_KINDS {
        let verdicts: Vec<bool> = g
            .validators
            .iter()
            .filter(|c| kind_from_label(&c.kind) == kind)
            .map(|c| c.valid)
            .collect();
        let valid = verdicts.iter().filter(|v| **v).count();
        let invalid = verdicts.len() - valid;
        assert!(
            valid >= 2,
            "{}: fewer than two valid examples",
            kind.label()
        );
        assert!(
            invalid >= 2,
            "{}: fewer than two invalid examples",
            kind.label()
        );
    }
}

#[test]
fn pii_golden_corpus_has_each_required_group() {
    let g = golden();
    for group in ["norwegian", "english", "near_miss"] {
        assert!(
            g.cases.iter().any(|c| c.group == group),
            "no text case in group {group}"
        );
    }
}

#[test]
fn pii_marker_is_stable_per_kind() {
    assert_eq!(PiiKind::Email.marker(), "[redacted:pii.email]");
    assert_eq!(PiiKind::Phone.marker(), "[redacted:pii.phone]");
    assert_eq!(PiiKind::Iban.marker(), "[redacted:pii.iban]");
    assert_eq!(PiiKind::PaymentCard.marker(), "[redacted:pii.card]");
    assert_eq!(
        PiiKind::Fodselsnummer.marker(),
        "[redacted:pii.fodselsnummer]"
    );
}

#[test]
fn pii_fodselsnummer_rejects_a_remainder_of_ten() {
    // 010190456 has no valid first control digit (remainder 10), so no
    // eleven-digit number built on it can pass.
    for k1 in 0..=9u8 {
        for k2 in 0..=9u8 {
            let candidate = format!("010190456{k1}{k2}");
            if pii::is_fodselsnummer(&candidate) {
                // A valid number here would need k1 to equal the computed control
                // digit, which does not exist for this prefix.
                panic!("prefix 010190456 must never validate: {candidate}");
            }
        }
    }
}
