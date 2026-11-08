//! The stamp that records what redaction did to a record.
//!
//! It carries counts and static rule ids only: no content, no lengths, no
//! offsets. A reader learns that something was masked and by which rule, never
//! what it was or how long it was. Every field has a serde default so a stamp
//! written by an older build still reads.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Mode, Profile, Tally, RULE_VERSION};

/// How the field-facts probe ended (WS12.S1 sets it; `Skipped` until then).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeState {
    Ok,
    /// Some fields could not be probed.
    Partial,
    Failed,
    #[default]
    Skipped,
}

/// What happened to the screenshot (WS12.S5 sets it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotState {
    /// No screenshot was part of this record.
    #[default]
    #[serde(rename = "none")]
    NotApplicable,
    /// Sensitive element rectangles were painted over.
    Masked,
    /// The screenshot was withheld because it could not be masked.
    Withheld,
}

/// The redaction record attached to a receipt, event or observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RedactionStamp {
    pub rule_version: String,
    pub profile: Profile,
    pub mode: Mode,
    pub values_masked: u32,
    pub by_rule: BTreeMap<String, u32>,
    pub probe: ProbeState,
    pub probe_ms: u32,
    pub screenshot: ScreenshotState,
    /// WS12.S5: milliseconds the screenshot scan and mask took (0 when none ran).
    pub mask_ms: u32,
    /// WS12.S5: cross-origin iframes the scan could not look into. A count only.
    pub oopif_frames: u32,
    /// WS12.S5: how many rectangles were painted over (0 for `none` and `withheld`).
    pub rects: u32,
}

impl Default for RedactionStamp {
    fn default() -> Self {
        Self {
            rule_version: RULE_VERSION.to_owned(),
            profile: Profile::default(),
            mode: Mode::default(),
            values_masked: 0,
            by_rule: BTreeMap::new(),
            probe: ProbeState::default(),
            probe_ms: 0,
            screenshot: ScreenshotState::default(),
            mask_ms: 0,
            oopif_frames: 0,
            rects: 0,
        }
    }
}

impl RedactionStamp {
    /// The stamp for one pass: the current rule version, the profile and mode
    /// it ran under, and the counts. The probe and screenshot fields start at
    /// their "not applicable" values; their owners set them.
    #[must_use]
    pub fn from_tally(tally: &Tally, profile: Profile, mode: Mode) -> Self {
        Self {
            rule_version: RULE_VERSION.to_owned(),
            profile,
            mode,
            values_masked: tally.values_masked(),
            by_rule: tally
                .by_rule()
                .iter()
                .map(|(rule, n)| ((*rule).to_owned(), *n))
                .collect(),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_object_reads_as_the_defaults() {
        let s: RedactionStamp = serde_json::from_str("{}").unwrap();
        assert_eq!(s, RedactionStamp::default());
        assert_eq!(s.rule_version, RULE_VERSION);
        assert_eq!(s.probe, ProbeState::Skipped);
        assert_eq!(s.screenshot, ScreenshotState::NotApplicable);
        assert_eq!((s.mask_ms, s.oopif_frames, s.rects), (0, 0, 0));
    }

    #[test]
    fn the_screenshot_fields_round_trip_and_an_older_stamp_still_reads() {
        let mut s = RedactionStamp::default();
        s.screenshot = ScreenshotState::Masked;
        s.mask_ms = 12;
        s.oopif_frames = 2;
        s.rects = 3;
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["screenshot"], "masked");
        assert_eq!(v["mask_ms"], 12);
        assert_eq!(v["oopif_frames"], 2);
        assert_eq!(v["rects"], 3);
        let back: RedactionStamp = serde_json::from_value(v).unwrap();
        assert_eq!(back, s);
        // A stamp written before WS12.S5 has none of the three fields.
        let old: RedactionStamp =
            serde_json::from_str(r#"{"probe":"ok","screenshot":"none"}"#).unwrap();
        assert_eq!((old.mask_ms, old.oopif_frames, old.rects), (0, 0, 0));
    }

    #[test]
    fn the_wire_names_are_the_documented_ones() {
        let mut t = Tally::new();
        t.add("shape.jwt");
        let mut s = t.stamp(Profile::Live, Mode::Shadow);
        s.probe = ProbeState::Partial;
        s.screenshot = ScreenshotState::Withheld;
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["rule_version"], "qr-redact/4");
        assert_eq!(v["profile"], "live");
        assert_eq!(v["mode"], "shadow");
        assert_eq!(v["values_masked"], 1);
        assert_eq!(v["by_rule"]["shape.jwt"], 1);
        assert_eq!(v["probe"], "partial");
        assert_eq!(v["screenshot"], "withheld");
        assert_eq!(
            serde_json::to_value(ScreenshotState::NotApplicable).unwrap(),
            "none"
        );
        let back: RedactionStamp = serde_json::from_value(v).unwrap();
        assert_eq!(back, s);
    }
}
