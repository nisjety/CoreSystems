//! E4.4-a: the locale, timezone and location a scrape asks the browser to emulate on the page it
//! opens (the `POST /v1/scrape` option keys `locale`, `timezone` and `location`).
//!
//! Off unless the edge enables `QUARRY_EDGE__PER_PAGE_LOCALE`. While it is off the keys are
//! unknown keys, as they were before this slice. The checks here are the contract: a value the
//! browser would refuse is a 400, never a page that quietly has the host's locale.
//!
//! Out of this slice on purpose: the user agent (a change of identity, ADR 5), a country-level
//! egress (E4.4-c, behind ADR 5 and ADR 10) and a device profile.
//!
//! E4.4-a2 (D-W5-2): with `QUARRY_EDGE__PAGE_ACCEPT_LANGUAGE` also on, a page that emulates a
//! locale sends an `Accept-Language` header derived from that already-checked locale
//! ([`accept_language_for`]). The user agent is never touched, and the header is set once when the
//! tab opens from the request, never in reaction to a response.

use serde::{Deserialize, Serialize};

/// The accuracy, in metres, of a location override that does not state one. Chromium reports the
/// position as unavailable without an accuracy, so one is always sent.
pub const DEFAULT_ACCURACY_M: f64 = 100.0;

/// The largest accuracy a location may state, in metres.
pub const MAX_ACCURACY_M: f64 = 100_000.0;

/// The option keys of this slice. Each is spelled the same in snake and camel case.
pub const PAGE_EMULATION_KEYS: [&str; 3] = ["locale", "timezone", "location"];

/// What the page emulates. A field the request did not set is absent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct PageEmulation {
    /// ICU style, `nb_NO`. The request's `nb-NO` is stored with `_`, which is what Chromium takes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// An IANA name, `Europe/Oslo`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// The position `navigator.geolocation` reports to the origin the scrape navigates to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// E4.4-a2: whether the page also sends `Accept-Language` derived from `locale`
    /// ([`accept_language_for`]). Set by the scrape check only, never by the caller: true only
    /// while `QUARRY_EDGE__PAGE_ACCEPT_LANGUAGE` is on and a locale is set.
    #[serde(default, skip_serializing_if = "is_false")]
    pub send_accept_language: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// The largest `Accept-Language` value [`accept_language_for`] returns.
pub const MAX_ACCEPT_LANGUAGE_LEN: usize = 64;

/// The `Accept-Language` header for a locale already checked by [`parse_locale`] (ICU style,
/// `nb_NO`): the full tag first, then its bare language at `q=0.9` (`nb-NO,nb;q=0.9`). A bare
/// language is sent alone (`de`). `None` when the locale is not a checked locale or the value
/// would be longer than [`MAX_ACCEPT_LANGUAGE_LEN`]. Never built from caller text directly.
#[must_use]
pub fn accept_language_for(locale_icu: &str) -> Option<String> {
    let checked = parse_locale(locale_icu)?;
    let tag = checked.replace('_', "-");
    let language = tag.split('-').next()?;
    let value = if language.len() == tag.len() {
        tag.clone()
    } else {
        format!("{tag},{language};q=0.9")
    };
    (value.len() <= MAX_ACCEPT_LANGUAGE_LEN).then_some(value)
}

impl PageEmulation {
    /// Whether nothing is emulated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.locale.is_none() && self.timezone.is_none() && self.location.is_none()
    }
}

/// A geolocation override, in decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    /// Metres. Absent in a request means [`DEFAULT_ACCURACY_M`]; a checked location always has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accuracy: Option<f64>,
}

/// The locale as Chromium takes it, or `None` when it is not a language tag: a language of two or
/// three letters, then subtags of one to eight letters or digits, joined by `-` or `_`.
#[must_use]
pub fn parse_locale(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() || value.len() > 35 {
        return None;
    }
    let mut parts = value.split(['-', '_']);
    let language = parts.next()?;
    let language_ok =
        (2..=3).contains(&language.len()) && language.chars().all(|c| c.is_ascii_alphabetic());
    let subtags_ok = parts.all(|part| {
        (1..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
    });
    (language_ok && subtags_ok).then(|| value.replace('-', "_"))
}

/// The timezone as Chromium takes it, or `None` when it cannot be an IANA name: letters, digits and
/// `_ + -` in segments joined by `/`, the first character a letter. Whether the zone exists is the
/// browser's to say, and a zone it refuses is refused the same way.
#[must_use]
pub fn parse_timezone(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() || value.len() > 64 {
        return None;
    }
    let starts_with_letter = value
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic());
    let segments_ok = value.split('/').all(|segment| {
        !segment.is_empty()
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))
    });
    (starts_with_letter && segments_ok).then(|| value.to_owned())
}

/// The location checked, with its accuracy filled in. On a failure, the name of the field that is
/// out of range.
pub fn check_location(location: &Location) -> Result<Location, &'static str> {
    let within = |value: f64, bound: f64| value.is_finite() && value.abs() <= bound;
    if !within(location.latitude, 90.0) {
        return Err("latitude");
    }
    if !within(location.longitude, 180.0) {
        return Err("longitude");
    }
    let accuracy = location.accuracy.unwrap_or(DEFAULT_ACCURACY_M);
    if !accuracy.is_finite() || accuracy <= 0.0 || accuracy > MAX_ACCURACY_M {
        return Err("accuracy");
    }
    Ok(Location {
        accuracy: Some(accuracy),
        ..*location
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_are_language_tags_stored_the_way_chromium_takes_them() {
        assert_eq!(parse_locale("nb-NO").as_deref(), Some("nb_NO"));
        assert_eq!(parse_locale(" en_US ").as_deref(), Some("en_US"));
        assert_eq!(parse_locale("zh-Hant-TW").as_deref(), Some("zh_Hant_TW"));
        assert_eq!(parse_locale("de").as_deref(), Some("de"));
        let long = "x".repeat(36);
        for bad in [
            "",
            "e",
            "en US",
            "en_US; drop",
            "en--US",
            "en-",
            "1n-US",
            long.as_str(),
        ] {
            assert_eq!(parse_locale(bad), None, "{bad}");
        }
    }

    #[test]
    fn timezones_are_iana_shaped() {
        assert_eq!(
            parse_timezone("Europe/Oslo").as_deref(),
            Some("Europe/Oslo")
        );
        assert_eq!(parse_timezone("UTC").as_deref(), Some("UTC"));
        assert_eq!(
            parse_timezone("America/Port-au-Prince").as_deref(),
            Some("America/Port-au-Prince")
        );
        assert_eq!(parse_timezone("Etc/GMT+1").as_deref(), Some("Etc/GMT+1"));
        for bad in [
            "",
            "../etc/passwd",
            "Europe//Oslo",
            "/Oslo",
            "Europe/",
            "1Zone",
            "Europe/Oslo;x",
        ] {
            assert_eq!(parse_timezone(bad), None, "{bad}");
        }
    }

    #[test]
    fn locations_are_in_range_and_take_a_default_accuracy() {
        let oslo = Location {
            latitude: 59.9139,
            longitude: 10.7522,
            accuracy: None,
        };
        let checked = check_location(&oslo).expect("in range");
        assert_eq!(checked.accuracy, Some(DEFAULT_ACCURACY_M));
        assert_eq!((checked.latitude, checked.longitude), (59.9139, 10.7522));
        let at = |latitude: f64, longitude: f64, accuracy: Option<f64>| {
            check_location(&Location {
                latitude,
                longitude,
                accuracy,
            })
        };
        assert_eq!(at(91.0, 0.0, None), Err("latitude"));
        assert_eq!(at(0.0, -180.5, None), Err("longitude"));
        assert_eq!(at(f64::NAN, 0.0, None), Err("latitude"));
        assert_eq!(at(0.0, 0.0, Some(0.0)), Err("accuracy"));
        assert_eq!(at(0.0, 0.0, Some(100_001.0)), Err("accuracy"));
        assert!(at(-90.0, 180.0, Some(5.0)).is_ok());
    }

    #[test]
    fn accept_language_is_derived_from_the_checked_locale_only() {
        assert_eq!(
            accept_language_for("nb_NO").as_deref(),
            Some("nb-NO,nb;q=0.9")
        );
        assert_eq!(accept_language_for("de").as_deref(), Some("de"));
        assert_eq!(
            accept_language_for("zh_Hant_TW").as_deref(),
            Some("zh-Hant-TW,zh;q=0.9")
        );
        assert_eq!(accept_language_for("en_US; drop"), None);
        assert_eq!(accept_language_for(""), None);
        let off = serde_json::to_value(PageEmulation {
            locale: Some("de".to_owned()),
            ..PageEmulation::default()
        })
        .expect("serializes");
        assert!(off.get("send_accept_language").is_none());
    }

    #[test]
    fn an_emulation_with_nothing_set_is_empty() {
        assert!(PageEmulation::default().is_empty());
        let one = PageEmulation {
            timezone: Some("UTC".to_owned()),
            ..PageEmulation::default()
        };
        assert!(!one.is_empty());
    }
}
