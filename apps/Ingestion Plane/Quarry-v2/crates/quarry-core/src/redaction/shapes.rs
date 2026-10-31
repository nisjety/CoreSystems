//! The text rule table: token shapes (Live) and contextual rules (Durable).
//!
//! Ported from the Model Plane scrubber (`execution-core/src/scrub.rs`, read
//! only) and narrowed where that scrubber over-redacts: `bearer` and `basic`
//! need a header context or a token that looks like one, so the prose "bearer
//! of the message" and "Basic information" survive.
//!
//! All rules are linear-time `regex` patterns, compiled once. A `RegexSet` per
//! profile is the pre-pass: text with no secret costs one scan. The scan loops
//! to a fixed point so that `redact(redact(x)) == redact(x)`.

use std::borrow::Cow;
use std::sync::OnceLock;

use regex::{Captures, Regex, RegexSet};

use super::{luhn, mask, MarkerKind, Profile, Tally, MARKER_PREFIX};

/// Opaque bearer tokens outside a header must be at least this long and carry
/// a digit or a base64 symbol.
const BEARER_BARE_MIN: usize = 16;
/// Same for a bare `Basic <base64>`.
const BASIC_BARE_MIN: usize = 12;
/// How far before a match `Authorization` may sit to count as its header.
const HEADER_LOOKBEHIND: usize = 48;
/// Safety cap on the fixed-point loop. One pass is the normal case, two when a
/// replacement exposed nothing new; four is never reached on real input.
const MAX_PASSES: usize = 4;
/// A bare labelled value that is one of these is a type name or a state word,
/// not a secret: `password: required`, `"password": null`.
const LABELLED_STOPLIST: &[&str] = &[
    "null",
    "true",
    "false",
    "none",
    "undefined",
    "nil",
    "required",
    "optional",
    "empty",
    "string",
    "boolean",
    "hidden",
];

type Verify = fn(&str, &Captures<'_>) -> bool;

#[derive(Clone, Copy, Debug)]
enum Replace {
    /// The whole match is the secret.
    Whole,
    /// Only the first of these capture groups that took part is the secret;
    /// the rest of the match (the label, the scheme) stays.
    Groups(&'static [usize]),
}

struct Def {
    id: &'static str,
    kind: MarkerKind,
    /// `Live` rules also run under `Durable`; `Durable` rules run only there.
    min_profile: Profile,
    pattern: &'static str,
    replace: Replace,
    verify: Option<Verify>,
}

/// The rule table, in application order.
static DEFS: &[Def] = &[
    Def {
        id: "shape.pem",
        kind: MarkerKind::Key,
        min_profile: Profile::Live,
        // An unterminated block (a truncated page) is redacted to the end of
        // the text: failing closed is the safe reading.
        pattern: r"-----BEGIN [A-Z ]*PRIVATE KEY(?: BLOCK)?-----[\s\S]*?(?:-----END [A-Z ]*PRIVATE KEY(?: BLOCK)?-----|\z)",
        replace: Replace::Whole,
        verify: None,
    },
    Def {
        id: "shape.jwt",
        kind: MarkerKind::Token,
        min_profile: Profile::Live,
        // The `eyJ` prefix is matched in any case: an element fingerprint keeps its text
        // lower-cased, and a token that went through that normaliser must still be masked.
        pattern: r"(?-u:\b)(?i:eyj)[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]*",
        replace: Replace::Whole,
        verify: None,
    },
    Def {
        id: "shape.userinfo",
        kind: MarkerKind::Credential,
        min_profile: Profile::Live,
        pattern: r"(?i)(?-u:\b)([a-z][a-z0-9+.\-]*://[^:/?#@\s]+:)([^@/?#\s]+)(@)",
        replace: Replace::Groups(&[2]),
        verify: None,
    },
    Def {
        id: "shape.aws",
        kind: MarkerKind::Key,
        min_profile: Profile::Live,
        pattern: r"(?-u:\b)(?:AKIA|ASIA)[0-9A-Z]{16}(?-u:\b)",
        replace: Replace::Whole,
        verify: None,
    },
    Def {
        id: "shape.github",
        kind: MarkerKind::Token,
        min_profile: Profile::Live,
        pattern: r"(?-u:\b)(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})",
        replace: Replace::Whole,
        verify: None,
    },
    Def {
        id: "shape.gitlab",
        kind: MarkerKind::Token,
        min_profile: Profile::Live,
        pattern: r"(?-u:\b)glpat-[A-Za-z0-9_-]{20,}",
        replace: Replace::Whole,
        verify: None,
    },
    Def {
        id: "shape.npm",
        kind: MarkerKind::Token,
        min_profile: Profile::Live,
        pattern: r"(?-u:\b)npm_[A-Za-z0-9]{30,}",
        replace: Replace::Whole,
        verify: None,
    },
    Def {
        id: "shape.slack",
        kind: MarkerKind::Token,
        min_profile: Profile::Live,
        pattern: r"(?-u:\b)(?:xox[baprso]-|xapp-)[A-Za-z0-9-]{10,}",
        replace: Replace::Whole,
        verify: None,
    },
    Def {
        id: "shape.google",
        kind: MarkerKind::Key,
        min_profile: Profile::Live,
        pattern: r"(?-u:\b)AIza[0-9A-Za-z_-]{35}",
        replace: Replace::Whole,
        verify: None,
    },
    Def {
        id: "shape.openai",
        kind: MarkerKind::Key,
        min_profile: Profile::Live,
        pattern: r"(?-u:\b)sk-[A-Za-z0-9_-]{16,}",
        replace: Replace::Whole,
        verify: None,
    },
    Def {
        id: "shape.stripe",
        kind: MarkerKind::Key,
        min_profile: Profile::Live,
        // Secret and restricted keys. A publishable key (pk_) is public.
        pattern: r"(?-u:\b)[sr]k_(?:live|test)_[A-Za-z0-9]{16,}",
        replace: Replace::Whole,
        verify: None,
    },
    Def {
        id: "shape.whsec",
        kind: MarkerKind::Secret,
        min_profile: Profile::Live,
        pattern: r"(?-u:\b)whsec_[A-Za-z0-9+/=]{16,}",
        replace: Replace::Whole,
        verify: None,
    },
    Def {
        id: "shape.bearer",
        kind: MarkerKind::Token,
        min_profile: Profile::Live,
        pattern: r"(?i)(?-u:\b)bearer\s+([A-Za-z0-9._~+/=-]{6,})",
        replace: Replace::Groups(&[1]),
        verify: Some(bearer_ok),
    },
    Def {
        id: "shape.basic",
        kind: MarkerKind::Credential,
        min_profile: Profile::Live,
        pattern: r"(?i)(?-u:\b)basic\s+([A-Za-z0-9+/]{8,}={0,2})",
        replace: Replace::Groups(&[1]),
        verify: Some(basic_ok),
    },
    // ---- Durable-only contextual rules -------------------------------------
    Def {
        id: "ctx.otp_numeric",
        kind: MarkerKind::Otp,
        min_profile: Profile::Durable,
        // "your verification code is 482913", "OTP: 123456", "Engangskode: 731904".
        // Bare `code` and `pin` are not labels (a ZIP code, "pin the tab").
        pattern: r"(?i)(?-u:\b)(?:otp|totp|one[- ]?time[- ](?:code|password|passcode|pin)|(?:verification|confirmation|security|login|sign-in)[- ]code|passcode|2fa[- ]code|engangskode|engangspassord|bekreftelseskode|sikkerhetskode|verifiseringskode)(?-u:\b)(?:[\s:=()\-\x{2013}]|(?-u:\b)(?:is|er|was|are)(?-u:\b))*([0-9]{4,8}|[0-9]{3}[ -][0-9]{3})(?-u:\b)",
        replace: Replace::Groups(&[1]),
        verify: None,
    },
    Def {
        id: "ctx.otp_numeric",
        kind: MarkerKind::Otp,
        min_profile: Profile::Durable,
        // "482913 is your verification code", "731904 er din engangskode".
        pattern: r"(?i)(?-u:\b)([0-9]{4,8})(?-u:\b)[\s,]*(?:is|er)\s+(?:your|the|my|din|ditt|min)\s+(?:otp|totp|one[- ]?time[- ](?:code|password|passcode)|(?:verification|confirmation|security|login|sign-in)[- ]code|passcode|2fa[- ]code|engangskode|bekreftelseskode|sikkerhetskode)(?-u:\b)",
        replace: Replace::Groups(&[1]),
        verify: None,
    },
    Def {
        id: "url.fragment",
        kind: MarkerKind::Token,
        min_profile: Profile::Durable,
        pattern: r#"(#(?:[^\s#=&]*=[^\s#&]*&)*?(?i:access_token|id_token|refresh_token|token|code)=)([^&\s"'<>]+)"#,
        replace: Replace::Groups(&[2]),
        verify: None,
    },
    Def {
        id: "url.query",
        kind: MarkerKind::Token,
        min_profile: Profile::Durable,
        // `;` is here so that an HTML-escaped separator (`&amp;token=`) is caught.
        pattern: r#"([?&;](?i:token|access_token|id_token|refresh_token|code|key|api_key|apikey|sig|signature|password|secret|session|sid|otp|jwt|x-amz-[a-z0-9-]+)=)([^&#\s"'<>]+)"#,
        replace: Replace::Groups(&[2]),
        verify: None,
    },
    Def {
        id: "ctx.labelled_secret",
        kind: MarkerKind::Secret,
        min_profile: Profile::Durable,
        // `DB_PASSWORD=x`, `api_key: "x"`, `"password": "x y"`, `x-api-key: x`,
        // `Passord: x`. Groups: 3 double-quoted, 4 single-quoted, 5 bare value.
        pattern: r#"(?i)(?-u:\b)([a-z0-9_]*(?:password|passwd|passord|secret|secret[_-]?key|api[_-]?key|access[_-]?key|auth[_-]?token|client[_-]?secret|private[_-]?key|token))(["']?\s*[:=]\s*)(?:"([^"\r\n]*)"|'([^'\r\n]*)'|([^\s"'&,;]{3,}))"#,
        replace: Replace::Groups(&[3, 4, 5]),
        verify: Some(labelled_ok),
    },
];

/// Does `Authorization` sit just before `pos`?
fn after_authorization(text: &str, pos: usize) -> bool {
    const NEEDLE: &[u8] = b"authorization";
    let lo = pos.saturating_sub(HEADER_LOOKBEHIND);
    text.as_bytes()[lo..pos]
        .windows(NEEDLE.len())
        .any(|w| w.eq_ignore_ascii_case(NEEDLE))
}

fn has_digit_or_symbol(s: &str, symbols: &str) -> bool {
    s.bytes()
        .any(|b| b.is_ascii_digit() || symbols.as_bytes().contains(&b))
}

fn bearer_ok(text: &str, caps: &Captures<'_>) -> bool {
    let (Some(whole), Some(token)) = (caps.get(0), caps.get(1)) else {
        return false;
    };
    after_authorization(text, whole.start())
        || (token.len() >= BEARER_BARE_MIN && has_digit_or_symbol(token.as_str(), "._~+/="))
}

fn basic_ok(text: &str, caps: &Captures<'_>) -> bool {
    let (Some(whole), Some(token)) = (caps.get(0), caps.get(1)) else {
        return false;
    };
    after_authorization(text, whole.start())
        || (token.len() >= BASIC_BARE_MIN && has_digit_or_symbol(token.as_str(), "+/="))
}

fn labelled_ok(_text: &str, caps: &Captures<'_>) -> bool {
    // Quoted values are always real values. A bare one is not when it is a
    // type name or a state word.
    if caps.get(3).is_some() || caps.get(4).is_some() {
        return true;
    }
    caps.get(5).is_some_and(|v| {
        !LABELLED_STOPLIST
            .iter()
            .any(|w| v.as_str().eq_ignore_ascii_case(w))
    })
}

struct Compiled {
    rules: Vec<Regex>,
    live_set: RegexSet,
    /// Set position -> index into `DEFS`/`rules`.
    live_map: Vec<usize>,
    durable_set: RegexSet,
    durable_map: Vec<usize>,
}

fn compiled() -> &'static Compiled {
    static C: OnceLock<Compiled> = OnceLock::new();
    C.get_or_init(|| {
        let rules: Vec<Regex> = DEFS
            .iter()
            .map(|d| {
                Regex::new(d.pattern)
                    .unwrap_or_else(|e| panic!("redaction rule {} does not compile: {e}", d.id))
            })
            .collect();
        let pick = |profile: Profile| -> (RegexSet, Vec<usize>) {
            let map: Vec<usize> = DEFS
                .iter()
                .enumerate()
                .filter(|(_, d)| d.min_profile <= profile)
                .map(|(i, _)| i)
                .collect();
            let set = RegexSet::new(map.iter().map(|&i| DEFS[i].pattern))
                .unwrap_or_else(|e| panic!("redaction rule set does not compile: {e}"));
            (set, map)
        };
        let (live_set, live_map) = pick(Profile::Live);
        let (durable_set, durable_map) = pick(Profile::Durable);
        Compiled {
            rules,
            live_set,
            live_map,
            durable_set,
            durable_map,
        }
    })
}

/// Replace every accepted match of one rule. `None` when nothing changed.
fn apply_rule(def: &Def, re: &Regex, text: &str, tally: &mut Tally) -> Option<String> {
    let marker = mask(def.kind);
    let mut out = String::new();
    let mut last = 0;
    let mut changed = false;
    let mut take = |start: usize, end: usize, out: &mut String| {
        out.push_str(&text[last..start]);
        out.push_str(marker);
        last = end;
        changed = true;
        tally.add(def.id);
    };
    match def.replace {
        Replace::Whole => {
            for m in re.find_iter(text) {
                if m.as_str().is_empty() || m.as_str().starts_with(MARKER_PREFIX) {
                    continue;
                }
                take(m.start(), m.end(), &mut out);
            }
        }
        Replace::Groups(groups) => {
            for caps in re.captures_iter(text) {
                let Some(secret) = groups.iter().find_map(|&g| caps.get(g)) else {
                    continue;
                };
                if secret.as_str().is_empty() || secret.as_str().starts_with(MARKER_PREFIX) {
                    continue;
                }
                if def.verify.is_some_and(|v| !v(text, &caps)) {
                    continue;
                }
                take(secret.start(), secret.end(), &mut out);
            }
        }
    }
    if changed {
        out.push_str(&text[last..]);
        Some(out)
    } else {
        None
    }
}

/// Redact `input` under `profile`, counting into `tally`. Text with nothing to
/// mask is returned borrowed.
pub(crate) fn redact<'a>(input: &'a str, profile: Profile, tally: &mut Tally) -> Cow<'a, str> {
    let c = compiled();
    let (set, map) = match profile {
        Profile::Live => (&c.live_set, &c.live_map),
        Profile::Durable => (&c.durable_set, &c.durable_map),
    };
    let mut text = Cow::Borrowed(input);
    for _ in 0..MAX_PASSES {
        let mut changed = false;
        for set_idx in set.matches(&text).iter() {
            let i = map[set_idx];
            if let Some(next) = apply_rule(&DEFS[i], &c.rules[i], &text, tally) {
                text = Cow::Owned(next);
                changed = true;
            }
        }
        if profile == Profile::Durable {
            let runs = luhn::find_pan_runs(&text);
            if !runs.is_empty() {
                let marker = mask(MarkerKind::Card);
                let mut out = String::with_capacity(text.len());
                let mut last = 0;
                for r in runs {
                    out.push_str(&text[last..r.start]);
                    out.push_str(marker);
                    last = r.end;
                    tally.add("ctx.pan");
                }
                out.push_str(&text[last..]);
                text = Cow::Owned(out);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    text
}

/// One line per rule for the drift fingerprint: everything that decides what
/// is masked, including the thresholds the verify functions use.
pub(crate) fn fingerprint_parts() -> Vec<String> {
    let mut parts: Vec<String> = DEFS
        .iter()
        .map(|d| {
            format!(
                "{}|{:?}|{:?}|{:?}|verify={}|{}",
                d.id,
                d.kind,
                d.min_profile,
                d.replace,
                d.verify.is_some(),
                d.pattern
            )
        })
        .collect();
    parts.push(format!(
        "thresholds|bearer={BEARER_BARE_MIN}|basic={BASIC_BARE_MIN}|lookbehind={HEADER_LOOKBEHIND}|passes={MAX_PASSES}"
    ));
    parts.push(format!("stoplist|{}", LABELLED_STOPLIST.join(",")));
    parts.push(format!("behaviour|{}", behaviour_probe()));
    parts
}

/// Inputs that sit on the edge of each verify function and of the label rules,
/// all letters, digits and symbols with no vendor shape in them. The drift
/// fingerprint hashes what both profiles do to each one, so a change inside a
/// verify closure (the digit-or-symbol clause, a length threshold, the header
/// look-behind) counts as a rule change, which the data tables alone cannot show.
const BEHAVIOUR_PROBES: &[&str] = &[
    "Authorization: Bearer abcdef",
    "Authorization: Basic abcdefgh",
    "send Bearer abcdefghijklmnop here",
    "send Bearer abcdefghijklmno1 here",
    "send Bearer abcdefghijklmn+p here",
    "send Bearer abcdefghijklm_p here",
    "send Bearer abcdefghijklmn.p here",
    "send Bearer abcde12 here",
    "send Bearer nightingaleinthedark here",
    "the bearer of the message arrived",
    "Basic infrastructure",
    "Basic abcdefghijkl",
    "Basic abcdefghijk1",
    "Basic abcdefghijk+",
    "Basic abcdefghijk/",
    "Basic abcdefghijk=",
    "Basic abcdefgh1",
    "Authorization:                                Bearer abcdefghij",
    "password: required token=null secret: ok1 secret='' key: \"two words\"",
    "passord=x1y2 api_key: abcd",
    "otp 482913 and 731904 er din engangskode",
];

/// What both profiles do to each of [`BEHAVIOUR_PROBES`], as text.
fn behaviour_probe() -> String {
    let mut out = String::new();
    for profile in [Profile::Live, Profile::Durable] {
        for p in BEHAVIOUR_PROBES {
            let mut t = Tally::new();
            let r = redact(p, profile, &mut t);
            out.push_str(&format!("{profile:?}:{r}/{};", t.values_masked()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(profile: Profile, s: &str) -> (String, Tally) {
        let mut t = Tally::new();
        let out = redact(s, profile, &mut t).into_owned();
        (out, t)
    }

    #[test]
    fn text_with_nothing_to_mask_is_returned_borrowed() {
        let mut t = Tally::new();
        let s = "status=completed path=/usr/bin count=42";
        assert!(matches!(
            redact(s, Profile::Durable, &mut t),
            Cow::Borrowed(_)
        ));
        assert!(t.is_empty());
    }

    #[test]
    fn every_def_id_is_a_known_rule_id() {
        for d in DEFS {
            assert!(super::super::RULE_IDS.contains(&d.id), "{}", d.id);
        }
    }

    #[test]
    fn a_labelled_secret_is_durable_only() {
        let (live, t) = run(Profile::Live, "password=hunter2");
        assert_eq!(live, "password=hunter2");
        assert!(t.is_empty());
        let (durable, t) = run(Profile::Durable, "password=hunter2");
        assert_eq!(durable, "password=[redacted:secret]");
        assert_eq!(t.by_rule().get("ctx.labelled_secret"), Some(&1));
    }

    #[test]
    fn a_second_pass_changes_nothing_and_counts_nothing() {
        let (first, _) = run(
            Profile::Durable,
            "x-api-key: abc123 and password='my pass' then otp 482913",
        );
        let (second, t) = run(Profile::Durable, &first);
        assert_eq!(first, second);
        assert!(t.is_empty(), "{:?}", t.by_rule());
    }

    #[test]
    fn quoted_values_are_masked_whole_and_empty_ones_are_left() {
        let (out, _) = run(
            Profile::Durable,
            r#"{"password": "two words", "token": ""}"#,
        );
        assert_eq!(out, r#"{"password": "[redacted:secret]", "token": ""}"#);
    }

    #[test]
    fn state_words_after_a_label_are_not_secrets() {
        let (out, t) = run(Profile::Durable, "password: required, token=null");
        assert_eq!(out, "password: required, token=null");
        assert!(t.is_empty());
    }

    #[test]
    fn bearer_in_prose_survives_and_a_header_does_not() {
        let (prose, _) = run(Profile::Live, "the bearer of the message arrived");
        assert_eq!(prose, "the bearer of the message arrived");
        let (hdr, _) = run(Profile::Live, "Authorization: Bearer abcdef");
        assert_eq!(hdr, "Authorization: Bearer [redacted:token]");
    }

    #[test]
    fn a_bare_bearer_word_of_letters_only_survives_however_long_it_is() {
        // 20 letters pass the pattern's minimum, so only the narrowing in
        // `bearer_ok` (a digit or a base64 symbol) keeps this prose alive.
        let s = "send Bearer nightingaleinthedark here";
        for profile in [Profile::Live, Profile::Durable] {
            let (out, t) = run(profile, s);
            assert_eq!(out, s, "{profile:?}");
            assert!(t.is_empty(), "{profile:?}");
        }
        // The same length with a digit, or a base64 symbol, is a token.
        for s in [
            "send Bearer nightingaleinthedar1 here",
            "send Bearer nightingaleinthedar+ here",
        ] {
            let (out, _) = run(Profile::Live, s);
            assert_eq!(out, "send Bearer [redacted:token] here", "{s}");
        }
        // Fifteen characters with a digit are below the bare minimum.
        let short = "send Bearer abcdefghijklmn1 here";
        let (out, _) = run(Profile::Live, short);
        assert_eq!(out, short);
    }

    #[test]
    fn every_behaviour_probe_is_stable_on_a_second_pass() {
        for profile in [Profile::Live, Profile::Durable] {
            for p in BEHAVIOUR_PROBES {
                let (first, _) = run(profile, p);
                let (second, t) = run(profile, &first);
                assert_eq!(first, second, "{p}");
                assert!(t.is_empty(), "{p}");
            }
        }
    }

    #[test]
    fn a_lower_cased_jwt_is_masked_too() {
        // What an element fingerprint's normalised text holds for a token in the text.
        let jwt = ["eyjhbgcioijiuzi1niisinr5cci6", ".eyjzdwiioiixmjm0", ".sflkxwrjsmekkf2qt4fwpm"].concat();
        let (out, t) = run(Profile::Live, &format!("go {jwt} now"));
        assert_eq!(out, "go [redacted:token] now");
        assert_eq!(t.by_rule().get("shape.jwt"), Some(&1));
        // Prose that merely starts the same way is not a token: it has no dotted segments.
        let (out, t) = run(Profile::Live, "eyjafjallajokull erupted");
        assert_eq!(out, "eyjafjallajokull erupted");
        assert!(t.is_empty());
    }

    #[test]
    fn a_pem_without_an_end_line_is_masked_to_the_end() {
        let s = "before\n-----BEGIN PRIVATE KEY-----\nAAAA\nBBBB";
        let (out, _) = run(Profile::Live, s);
        assert_eq!(out, "before\n[redacted:key]");
    }
}
