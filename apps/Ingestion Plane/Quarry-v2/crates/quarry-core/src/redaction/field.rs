//! Field semantics: which form fields hold a value that must never be shown.
//!
//! A field is classified from facts the driver collects (WS12.S1): the input
//! type, the autocomplete tokens, the labels, and (for a card number) the value
//! itself. The first hit wins, in this order:
//!
//! 1. `type=password` is a password and `type=tel` is a phone number.
//! 2. Types that carry no typed value (checkbox, button, ...) are unclassified.
//! 3. Autocomplete tokens, after dropping `section-*`, `shipping`, `billing`
//!    and the contact words: `current-password`, `new-password`,
//!    `one-time-code`, `cc-*`, `tel*`.
//! 4. A label regex over the accessible name, `aria-label`, placeholder, `name`
//!    and `id` (English and Norwegian). Bare `code` and `pin` prose is not a
//!    label: a ZIP code or a promo code is not a secret.
//! 5. A value that is a Luhn-valid card number.
//!
//! A username in a password form stays unclassified: it is an identifier, not a
//! secret, and masking it would blank the page for no gain.

use std::borrow::Cow;
use std::fmt;
use std::sync::OnceLock;

use regex::Regex;
use serde::Deserialize;

use super::{is_marker, luhn, MarkerKind, Mode, Tally};

/// What a field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldKind {
    Password,
    Otp,
    Card,
    Phone,
    Secret,
    /// Not known to hold a secret. The value is left alone.
    Unknown,
}

impl FieldKind {
    /// The marker kind that replaces the value, or `None` for `Unknown`.
    #[must_use]
    pub const fn marker_kind(self) -> Option<MarkerKind> {
        match self {
            Self::Password => Some(MarkerKind::Password),
            Self::Otp => Some(MarkerKind::Otp),
            Self::Card => Some(MarkerKind::Card),
            Self::Phone => Some(MarkerKind::Phone),
            Self::Secret => Some(MarkerKind::Secret),
            Self::Unknown => None,
        }
    }

    /// Does a value of this kind get masked?
    #[must_use]
    pub const fn is_sensitive(self) -> bool {
        !matches!(self, Self::Unknown)
    }

    const fn default_rule(self) -> &'static str {
        match self {
            Self::Password => "field.password",
            Self::Otp => "field.otp",
            Self::Card => "field.card",
            Self::Phone => "field.phone",
            Self::Secret | Self::Unknown => "field.label",
        }
    }
}

/// The facts a driver reads from one field. `value` is the field's current
/// value and is used only for the card-number test; `Debug` never prints it.
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct FieldFacts {
    pub input_type: Option<String>,
    pub autocomplete: Option<String>,
    pub name_attr: Option<String>,
    pub id_attr: Option<String>,
    pub aria_label: Option<String>,
    pub placeholder: Option<String>,
    pub accessible_name: Option<String>,
    pub value: Option<String>,
}

impl fmt::Debug for FieldFacts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FieldFacts")
            .field("input_type", &self.input_type)
            .field("autocomplete", &self.autocomplete)
            .field("name_attr", &self.name_attr)
            .field("id_attr", &self.id_attr)
            .field("aria_label", &self.aria_label)
            .field("placeholder", &self.placeholder)
            .field("accessible_name", &self.accessible_name)
            .field("value", &self.value.as_ref().map(|_| "<present>"))
            .finish()
    }
}

/// The verdict and the rule that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classification {
    pub kind: FieldKind,
    /// `field.password`, `field.otp`, `field.card`, `field.phone`,
    /// `field.label` or `field.luhn`; `None` for `Unknown`.
    pub rule: Option<&'static str>,
}

const UNKNOWN: Classification = Classification {
    kind: FieldKind::Unknown,
    rule: None,
};

/// Input types that never carry a typed secret.
const VALUELESS_TYPES: &[&str] = &[
    "checkbox", "radio", "button", "submit", "reset", "image", "range", "color", "file", "hidden",
];

/// Autocomplete tokens that qualify the field but do not name it.
const AUTOCOMPLETE_SKIP: &[&str] = &[
    "shipping", "billing", "home", "work", "mobile", "fax", "pager", "on", "off",
];

/// Label patterns per kind, tried in this order, over the normalized label
/// (lowercase, camelCase and punctuation turned into spaces).
const LABELS: &[(FieldKind, &str)] = &[
    (
        FieldKind::Password,
        r"password|passord|passwd|passphrase|passwort|kodeord|\bpwd\b",
    ),
    (
        FieldKind::Otp,
        r"\botp\b|\btotp\b|one ?time|verification code|verifiseringskode|bekreftelseskode|engangskode|engangspassord|sikkerhetskode|security code|confirmation code|login code|sms ?code|smskode|\b2fa\b|\bmfa\b|two ?factor|authenticator|passcode",
    ),
    (
        FieldKind::Card,
        r"card ?(?:number|no|num)\b|\bcc ?(?:num|number|no)\b|credit card|debit card|kredittkort|bankkort|kort ?nummer|\bcv[vcn]\b|\bcsc\b|card verification|\bpan\b",
    ),
    (
        FieldKind::Secret,
        r"secret|api ?key|access ?key|private ?key|\btokens?\b|credential|bank ?id|f[øo]dselsnummer|f[øo]dselsnr|personnummer|\bfnr\b|\bssn\b|social security|national id|kontonummer|\biban\b|account number|routing number|\bpin\b|pin ?kode|pin code",
    ),
    (
        FieldKind::Phone,
        r"phone|telephone|telefon|\btlf\b|mobilnummer|mobiltelefon|mobile (?:number|phone|no)\b|cell ?phone|\btel\b",
    ),
];

fn label_regexes() -> &'static [(FieldKind, Regex)] {
    static R: OnceLock<Vec<(FieldKind, Regex)>> = OnceLock::new();
    R.get_or_init(|| {
        LABELS
            .iter()
            .map(|(k, p)| {
                (
                    *k,
                    Regex::new(p)
                        .unwrap_or_else(|e| panic!("field label rule {k:?} does not compile: {e}")),
                )
            })
            .collect()
    })
}

/// Lowercase, split camelCase, and turn every non-alphanumeric into a space, so
/// `userPassword`, `user_password` and `User Password` read the same.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut prev_lower = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if c.is_uppercase() && prev_lower {
                out.push(' ');
            }
            out.extend(c.to_lowercase());
            prev_lower = c.is_lowercase() || c.is_numeric();
        } else {
            out.push(' ');
            prev_lower = false;
        }
    }
    out
}

fn from_autocomplete(ac: &str) -> Option<Classification> {
    for tok in ac.split_whitespace() {
        let tok = tok.to_ascii_lowercase();
        if tok.starts_with("section-") || AUTOCOMPLETE_SKIP.contains(&tok.as_str()) {
            continue;
        }
        let (kind, rule) = match tok.as_str() {
            "current-password" | "new-password" => (FieldKind::Password, "field.password"),
            "one-time-code" => (FieldKind::Otp, "field.otp"),
            t if t.starts_with("cc-") => (FieldKind::Card, "field.card"),
            t if t.starts_with("tel") => (FieldKind::Phone, "field.phone"),
            _ => continue,
        };
        return Some(Classification {
            kind,
            rule: Some(rule),
        });
    }
    None
}

fn from_labels(facts: &FieldFacts) -> Option<Classification> {
    let joined = [
        &facts.accessible_name,
        &facts.aria_label,
        &facts.placeholder,
        &facts.name_attr,
        &facts.id_attr,
    ]
    .into_iter()
    .flatten()
    .map(|s| normalize(s))
    .collect::<Vec<_>>()
    .join(" | ");
    if joined.is_empty() {
        return None;
    }
    label_regexes()
        .iter()
        .find(|(_, re)| re.is_match(&joined))
        .map(|(kind, _)| Classification {
            kind: *kind,
            rule: Some("field.label"),
        })
}

/// Classify a field and say which rule decided.
#[must_use]
pub fn classify(facts: &FieldFacts) -> Classification {
    let ty = facts
        .input_type
        .as_deref()
        .map(|t| t.trim().to_ascii_lowercase());
    match ty.as_deref() {
        Some("password") => {
            return Classification {
                kind: FieldKind::Password,
                rule: Some("field.password"),
            }
        }
        Some("tel") => {
            return Classification {
                kind: FieldKind::Phone,
                rule: Some("field.phone"),
            }
        }
        Some(t) if VALUELESS_TYPES.contains(&t) => return UNKNOWN,
        _ => {}
    }
    if let Some(c) = facts.autocomplete.as_deref().and_then(from_autocomplete) {
        return c;
    }
    if let Some(c) = from_labels(facts) {
        return c;
    }
    if facts.value.as_deref().is_some_and(luhn::is_pan) {
        return Classification {
            kind: FieldKind::Card,
            rule: Some("field.luhn"),
        };
    }
    UNKNOWN
}

/// The kind alone.
#[must_use]
pub fn classify_field(facts: &FieldFacts) -> FieldKind {
    classify(facts).kind
}

/// Mask a field value under the process-wide mode: the marker for a sensitive
/// kind, the value itself for `Unknown`, an empty value or a marker.
#[must_use]
pub fn mask<'a>(c: Classification, value: &'a str, tally: &mut Tally) -> Cow<'a, str> {
    mask_with(super::mode(), c, value, tally)
}

/// [`mask`] under an explicit mode. `Shadow` counts and returns the value.
#[must_use]
pub fn mask_with<'a>(
    mode: Mode,
    c: Classification,
    value: &'a str,
    tally: &mut Tally,
) -> Cow<'a, str> {
    let Some(kind) = c.kind.marker_kind() else {
        return Cow::Borrowed(value);
    };
    if mode == Mode::Off || value.is_empty() || is_marker(value) {
        return Cow::Borrowed(value);
    }
    tally.add(c.rule.unwrap_or_else(|| c.kind.default_rule()));
    if mode == Mode::Shadow {
        return Cow::Borrowed(value);
    }
    Cow::Borrowed(super::mask(kind))
}

pub(crate) fn fingerprint_parts() -> Vec<String> {
    let mut parts: Vec<String> = LABELS
        .iter()
        .map(|(k, p)| format!("field.label|{k:?}|{p}"))
        .collect();
    parts.push(format!("field.valueless|{}", VALUELESS_TYPES.join(",")));
    parts.push(format!(
        "field.autocomplete_skip|{}",
        AUTOCOMPLETE_SKIP.join(",")
    ));
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(f: impl FnOnce(&mut FieldFacts)) -> FieldFacts {
        let mut x = FieldFacts::default();
        f(&mut x);
        x
    }

    #[test]
    fn type_decides_first() {
        let pw = facts(|f| f.input_type = Some("PASSWORD".into()));
        assert_eq!(classify(&pw).kind, FieldKind::Password);
        let tel = facts(|f| f.input_type = Some("tel".into()));
        assert_eq!(classify(&tel).kind, FieldKind::Phone);
        let cb = facts(|f| {
            f.input_type = Some("checkbox".into());
            f.accessible_name = Some("Show password".into());
        });
        assert_eq!(classify(&cb).kind, FieldKind::Unknown);
    }

    #[test]
    fn autocomplete_ignores_section_and_contact_tokens() {
        let f = facts(|f| f.autocomplete = Some("section-blue shipping cc-number".into()));
        assert_eq!(
            classify(&f),
            Classification {
                kind: FieldKind::Card,
                rule: Some("field.card")
            }
        );
        let f = facts(|f| f.autocomplete = Some("home tel-national".into()));
        assert_eq!(classify(&f).kind, FieldKind::Phone);
    }

    #[test]
    fn labels_read_camel_case_and_norwegian() {
        let f = facts(|f| f.name_attr = Some("userPassword".into()));
        assert_eq!(classify(&f).kind, FieldKind::Password);
        let f = facts(|f| f.placeholder = Some("Skriv inn engangskode".into()));
        assert_eq!(classify(&f).kind, FieldKind::Otp);
        let f = facts(|f| f.accessible_name = Some("Fødselsnummer (11 siffer)".into()));
        assert_eq!(classify(&f).kind, FieldKind::Secret);
    }

    #[test]
    fn ordinary_fields_stay_unclassified() {
        for name in ["Username", "Search", "Postal code", "Promo code", "Email"] {
            let f = facts(|f| f.accessible_name = Some(name.into()));
            assert_eq!(classify(&f).kind, FieldKind::Unknown, "{name}");
        }
    }

    #[test]
    fn a_card_number_value_is_caught_in_an_unlabelled_field() {
        let f = facts(|f| {
            f.name_attr = Some("ref".into());
            f.value = Some("4111 1111 1111 1111".into());
        });
        assert_eq!(
            classify(&f),
            Classification {
                kind: FieldKind::Card,
                rule: Some("field.luhn")
            }
        );
    }

    #[test]
    fn debug_never_prints_the_value() {
        let f = facts(|f| f.value = Some("4111111111111111".into()));
        assert!(!format!("{f:?}").contains("4111"));
    }

    #[test]
    fn mask_replaces_sensitive_values_only() {
        let mut t = Tally::new();
        let c = classify(&facts(|f| f.input_type = Some("password".into())));
        assert_eq!(
            mask_with(Mode::Enforce, c, "pw", &mut t),
            "[redacted:password]"
        );
        assert_eq!(mask_with(Mode::Enforce, c, "", &mut t), "");
        assert_eq!(
            mask_with(Mode::Enforce, c, "[redacted:password]", &mut t),
            "[redacted:password]"
        );
        assert_eq!(mask_with(Mode::Shadow, c, "pw", &mut t), "pw");
        assert_eq!(mask_with(Mode::Off, c, "pw", &mut t), "pw");
        assert_eq!(t.by_rule().get("field.password"), Some(&2));
        assert_eq!(
            mask_with(Mode::Enforce, UNKNOWN, "visible", &mut t),
            "visible"
        );
    }
}
