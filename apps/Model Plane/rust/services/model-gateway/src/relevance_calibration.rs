//! Calibration of the cross-encoder score the Quarry edge relays (`score_kind =
//! "cross_encoder.v1"`).
//!
//! A cross-encoder score is an uncalibrated number: 0.9 from one reranker is not the same
//! relevance as 0.9 from another. The relevance gate blends a provider score with lexical overlap
//! against a fixed keep threshold, so a raw score on the wrong scale either admits irrelevant hits
//! or drops relevant ones. The fix is a monotone piecewise-linear map from the raw score to a
//! relevance estimate. It is fitted on the judged web-result set (QB-T14) and shipped as a reviewed
//! artefact next to this crate.
//!
//! Until that artefact is fitted it says so (`"state": "unfitted"`). The switch that asks Quarry for
//! cross-encoder scores is then refused at boot, so the gate never reads a raw score as if it were
//! calibrated. The switch is off by default, and while it is off the shipped artefact is not even
//! read.

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The switch that asks the Quarry edge for cross-encoder scores. Unset means off.
pub const SWITCH_ENV: &str = "VEREVON_RELEVANCE_CROSS_ENCODER_SCORES";

/// The most knots a fitted table may carry. Every knot is a number someone can check against the
/// judged labels, so a longer curve is a curve nobody has checked.
pub const MAX_KNOTS: usize = 8;

/// The fewest knots a table may carry: the two anchors.
const MIN_KNOTS: usize = 2;

const SCHEMA: &str = "relevance-calibration-v1";
const SCORE_KIND: &str = "cross_encoder.v1";
const SHIPPED: &str = include_str!("../calibration/quarry-cross-encoder-v1.json");
const SHIPPED_PIN: &str = include_str!("../calibration/quarry-cross-encoder-v1.json.sha256");

/// A monotone piecewise-linear map from a raw cross-encoder score to a relevance estimate in
/// `[0, 1]`. It runs from `(0, 0)` to `(1, 1)`, so a raw score of 0 is never judged relevant and
/// a raw score of 1 is always judged fully relevant.
#[derive(Debug, Clone, PartialEq)]
pub struct Calibration {
    knots: Vec<(f32, f32)>,
}

/// Why a calibration artefact or a switch value was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CalibrationError {
    /// The artefact is not the schema this gateway reads, or its fields disagree with each other.
    #[error("calibration artefact is malformed: {0}")]
    Malformed(String),
    /// The shipped artefact's bytes are not the bytes its sha256 pin names.
    #[error(
        "calibration artefact does not match its sha256 pin (pinned {expected}, found {found})"
    )]
    PinMismatch { expected: String, found: String },
    /// The table has fewer than two or more than [`MAX_KNOTS`] knots.
    #[error("calibration table has {0} knots; it needs between 2 and the documented maximum")]
    KnotCount(usize),
    /// A knot is not finite, or lies outside `[0, 1]`. The index is the knot's position.
    #[error("calibration knot {0} is not finite or lies outside [0, 1]")]
    KnotRange(usize),
    /// Raw scores must strictly increase. The index is the first knot that breaks the order.
    #[error("calibration knot {0} does not increase the raw score")]
    NotIncreasingX(usize),
    /// Calibrated values must never decrease. The index is the first knot that breaks the order.
    #[error("calibration knot {0} decreases the calibrated value")]
    NotMonotone(usize),
    /// The curve must start at (0, 0) and end at (1, 1).
    #[error("calibration table must run from (0, 0) to (1, 1)")]
    Anchors,
    /// The switch asks for cross-encoder scores, and the shipped table is not fitted yet.
    #[error(
        "VEREVON_RELEVANCE_CROSS_ENCODER_SCORES is on, but the shipped cross-encoder calibration is \
         unfitted; the switch is refused until QB-T14 knots are committed"
    )]
    SwitchWithoutCalibration,
    /// The switch is set to a value that is neither on nor off.
    #[error("VEREVON_RELEVANCE_CROSS_ENCODER_SCORES must be on or off, not {0:?}")]
    UnreadableSwitch(String),
}

impl Calibration {
    /// Build a table from its knots, `(raw, calibrated)` pairs in ascending raw order.
    ///
    /// # Errors
    ///
    /// Refuses any table the gate cannot rely on: fewer than two or more than [`MAX_KNOTS`] knots,
    /// a knot that is not finite or lies outside `[0, 1]`, raw scores that do not strictly increase,
    /// calibrated values that decrease, or a curve that does not run from `(0, 0)` to `(1, 1)`.
    pub fn from_knots(knots: &[(f32, f32)]) -> Result<Self, CalibrationError> {
        if !(MIN_KNOTS..=MAX_KNOTS).contains(&knots.len()) {
            return Err(CalibrationError::KnotCount(knots.len()));
        }
        let inside = |value: f32| value.is_finite() && (0.0..=1.0).contains(&value);
        for (index, &(x, y)) in knots.iter().enumerate() {
            if !(inside(x) && inside(y)) {
                return Err(CalibrationError::KnotRange(index));
            }
        }
        for (index, pair) in knots.windows(2).enumerate() {
            let (left, right) = (pair[0], pair[1]);
            if right.0 <= left.0 {
                return Err(CalibrationError::NotIncreasingX(index + 1));
            }
            if right.1 < left.1 {
                return Err(CalibrationError::NotMonotone(index + 1));
            }
        }
        if knots.first() != Some(&(0.0, 0.0)) || knots.last() != Some(&(1.0, 1.0)) {
            return Err(CalibrationError::Anchors);
        }
        Ok(Self {
            knots: knots.to_vec(),
        })
    }

    /// The knots, in ascending raw order.
    #[must_use]
    pub fn knots(&self) -> &[(f32, f32)] {
        &self.knots
    }

    /// Map a raw score to a calibrated relevance in `[0, 1]`.
    ///
    /// A score that is not a number (NaN or infinite) is **absent**, so `None` is returned: it is
    /// not zero, and a gate that read it as zero would judge the hit irrelevant on no evidence.
    /// Scores outside `[0, 1]` are clamped first.
    #[must_use]
    pub fn apply(&self, raw: f32) -> Option<f32> {
        if !raw.is_finite() {
            return None;
        }
        let x = raw.clamp(0.0, 1.0);
        // Validation guarantees the anchors, so the first knot is (0, 0) and the last is (1, 1).
        // Returning them directly keeps the endpoints exact instead of leaving them to rounding.
        if x <= 0.0 {
            return Some(self.knots[0].1);
        }
        if x >= 1.0 {
            return self.knots.last().map(|&(_, y)| y);
        }
        // The segment whose left knot is the last one at or before x. Raw scores strictly increase,
        // so the segment is unique, and x in (0, 1) keeps both of its knots in range.
        let index = self.knots.partition_point(|&(knot_x, _)| knot_x <= x) - 1;
        let (x0, y0) = self.knots[index];
        let (x1, y1) = self.knots[index + 1];
        let t = (x - x0) / (x1 - x0);
        // Clamped to the segment's own calibrated values, so rounding cannot step outside them and
        // the curve stays monotone across the knots as well as within each segment.
        Some((y0 + t * (y1 - y0)).clamp(y0, y1))
    }
}

/// What the shipped artefact says.
#[derive(Debug, Clone, PartialEq)]
pub enum Artefact {
    /// No table has been fitted. The switch cannot be enabled against it.
    Unfitted,
    /// A reviewed table, ready for the gate.
    Fitted(Calibration),
}

#[derive(Deserialize)]
struct WireArtefact {
    schema: String,
    score_kind: String,
    state: String,
    #[serde(default)]
    knots: Option<Vec<[f32; 2]>>,
}

/// Read a calibration artefact.
///
/// # Errors
///
/// Refuses an artefact of another schema or score kind, an unknown state, a `fitted` artefact
/// without knots, an `unfitted` artefact that carries knots, and any table [`Calibration::from_knots`]
/// refuses.
pub fn parse_artefact(text: &str) -> Result<Artefact, CalibrationError> {
    let wire: WireArtefact = serde_json::from_str(text)
        .map_err(|error| CalibrationError::Malformed(error.to_string()))?;
    if wire.schema != SCHEMA {
        return Err(CalibrationError::Malformed(format!(
            "schema is {:?}, expected {SCHEMA:?}",
            wire.schema
        )));
    }
    if wire.score_kind != SCORE_KIND {
        return Err(CalibrationError::Malformed(format!(
            "score_kind is {:?}, expected {SCORE_KIND:?}",
            wire.score_kind
        )));
    }
    match (wire.state.as_str(), wire.knots) {
        ("unfitted", None) => Ok(Artefact::Unfitted),
        ("unfitted", Some(_)) => Err(CalibrationError::Malformed(
            "an unfitted artefact must not carry knots".to_owned(),
        )),
        ("fitted", Some(knots)) => {
            let pairs: Vec<(f32, f32)> = knots.iter().map(|&[x, y]| (x, y)).collect();
            Calibration::from_knots(&pairs).map(Artefact::Fitted)
        }
        ("fitted", None) => Err(CalibrationError::Malformed(
            "a fitted artefact must carry knots".to_owned(),
        )),
        (other, _) => Err(CalibrationError::Malformed(format!(
            "state is {other:?}, expected \"fitted\" or \"unfitted\""
        ))),
    }
}

/// The artefact compiled into this binary, after its sha256 pin has been checked.
///
/// # Errors
///
/// [`CalibrationError::PinMismatch`] when the bytes differ from the pin, and anything
/// [`parse_artefact`] refuses.
pub fn shipped_artefact() -> Result<Artefact, CalibrationError> {
    verify_pin(SHIPPED, SHIPPED_PIN)?;
    parse_artefact(SHIPPED)
}

/// Check that `bytes` are the bytes `pin` names. The pin is the line `sha256sum` writes: the digest, then
/// the file name. Whitespace after the digest, a CR from a Windows checkout included, is not part of it.
///
/// # Errors
///
/// [`CalibrationError::PinMismatch`] when the pin names no digest or names another one.
fn verify_pin(bytes: &str, pin: &str) -> Result<(), CalibrationError> {
    let expected = pin.split_whitespace().next().unwrap_or_default().to_owned();
    let found = sha256_hex(bytes.as_bytes());
    if expected != found {
        return Err(CalibrationError::PinMismatch { expected, found });
    }
    Ok(())
}

/// Read the switch value. Unset, empty or an explicit off value is off; an explicit on value is on.
/// Anything else is refused, so a typo cannot silently leave the feature in the wrong state.
///
/// # Errors
///
/// [`CalibrationError::UnreadableSwitch`] for a value that is neither on nor off.
pub fn parse_switch(value: Option<&str>) -> Result<bool, CalibrationError> {
    let Some(raw) = value else {
        return Ok(false);
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "0" | "false" | "off" | "no" => Ok(false),
        "1" | "true" | "on" | "yes" => Ok(true),
        _ => Err(CalibrationError::UnreadableSwitch(raw.to_owned())),
    }
}

/// The boot-time decision. Off means no table and no request for cross-encoder scores. On needs a
/// fitted table, and an unfitted one is refused.
///
/// # Errors
///
/// [`CalibrationError::SwitchWithoutCalibration`] when the switch is on and the table is unfitted.
pub fn calibration_for_switch(
    requested: bool,
    artefact: &Artefact,
) -> Result<Option<Calibration>, CalibrationError> {
    match (requested, artefact) {
        (false, _) => Ok(None),
        (true, Artefact::Unfitted) => Err(CalibrationError::SwitchWithoutCalibration),
        (true, Artefact::Fitted(calibration)) => Ok(Some(calibration.clone())),
    }
}

/// The boot-time check the gateway runs once, reading the switch and, only when it is on, the
/// shipped artefact.
///
/// # Errors
///
/// Anything [`parse_switch`], [`shipped_artefact`] or [`calibration_for_switch`] refuses.
pub fn from_environment() -> Result<Option<Calibration>, CalibrationError> {
    from_switch_value(std::env::var(SWITCH_ENV).ok().as_deref())
}

/// The boot-time decision for one value of the switch. [`from_environment`] reads the value from the
/// process environment; the tests pass a value in, so the decision is checked without the environment.
fn from_switch_value(value: Option<&str>) -> Result<Option<Calibration>, CalibrationError> {
    let requested = parse_switch(value)?;
    if !requested {
        return Ok(None);
    }
    let calibration = calibration_for_switch(true, &shipped_artefact()?)?;
    tracing::info!(
        score_kind = SCORE_KIND,
        "quarry cross-encoder scores are on: the relevance gate reads calibrated scores"
    );
    Ok(calibration)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Calibration {
        Calibration::from_knots(&[(0.0, 0.0), (0.2, 0.05), (0.5, 0.4), (0.8, 0.9), (1.0, 1.0)])
            .expect("a valid table")
    }

    const FITTED: &str = r#"{
        "schema": "relevance-calibration-v1",
        "score_kind": "cross_encoder.v1",
        "state": "fitted",
        "knots": [[0.0, 0.0], [0.5, 0.4], [1.0, 1.0]]
    }"#;

    // --- the curve -----------------------------------------------------------

    /// The gate blends this value with lexical overlap, so a dip in it would let a hit with a higher
    /// raw score be judged less relevant. Dense sampling, no tolerance.
    #[test]
    fn a_table_is_monotone_on_a_dense_grid() {
        let curve = table();
        let mut previous = curve.apply(0.0).expect("0 is a number");
        for step in 1..=20_000_u32 {
            let raw = step as f32 / 20_000.0;
            let value = curve.apply(raw).expect("a grid point is a number");
            assert!(
                value >= previous,
                "not monotone at {raw}: {value} < {previous}"
            );
            previous = value;
        }
    }

    #[test]
    fn a_flat_stretch_is_monotone_too() {
        let flat = Calibration::from_knots(&[(0.0, 0.0), (0.4, 0.3), (0.6, 0.3), (1.0, 1.0)])
            .expect("a valid table");
        let mut previous = 0.0_f32;
        for step in 0..=2_000_u32 {
            let value = flat.apply(step as f32 / 2_000.0).expect("a number");
            assert!(value >= previous, "{value} < {previous}");
            previous = value;
        }
    }

    #[test]
    fn the_anchors_map_exactly_to_zero_and_one() {
        let curve = table();
        assert_eq!(curve.apply(0.0), Some(0.0), "f(0) = 0");
        assert_eq!(curve.apply(1.0), Some(1.0), "f(1) = 1");
        assert_eq!(curve.apply(-3.0), Some(0.0), "below the range clamps to 0");
        assert_eq!(curve.apply(7.0), Some(1.0), "above the range clamps to 1");
    }

    #[test]
    fn every_knot_is_reproduced_exactly() {
        let curve = table();
        assert_eq!(curve.apply(0.2), Some(0.05));
        assert_eq!(curve.apply(0.5), Some(0.4));
        assert_eq!(curve.apply(0.8), Some(0.9));
    }

    #[test]
    fn a_score_that_is_not_a_number_stays_absent_and_is_not_zero() {
        let curve = table();
        assert_eq!(curve.apply(f32::NAN), None, "NaN is absent");
        assert_eq!(curve.apply(f32::INFINITY), None, "infinity is absent");
        assert_eq!(
            curve.apply(f32::NEG_INFINITY),
            None,
            "negative infinity is absent"
        );
    }

    #[test]
    fn outputs_stay_inside_the_unit_interval() {
        let curve = table();
        for step in -200..=300_i32 {
            let value = curve.apply(step as f32 / 100.0).expect("a number");
            assert!((0.0..=1.0).contains(&value), "{value} escapes [0, 1]");
        }
    }

    // --- refusing a table that is not a curve --------------------------------

    #[test]
    fn a_table_that_is_not_a_curve_is_refused() {
        assert_eq!(
            Calibration::from_knots(&[(0.0, 0.0)]),
            Err(CalibrationError::KnotCount(1)),
            "one knot is no curve"
        );
        let nine: Vec<(f32, f32)> = (0..9).map(|i| (i as f32 / 8.0, i as f32 / 8.0)).collect();
        assert_eq!(
            Calibration::from_knots(&nine),
            Err(CalibrationError::KnotCount(9)),
            "more than the documented maximum"
        );
        assert_eq!(
            Calibration::from_knots(&[(0.0, 0.0), (0.5, 0.6), (0.7, 0.5), (1.0, 1.0)]),
            Err(CalibrationError::NotMonotone(2)),
            "a decreasing calibrated value"
        );
        assert_eq!(
            Calibration::from_knots(&[(0.0, 0.0), (0.5, 0.2), (0.5, 0.4), (1.0, 1.0)]),
            Err(CalibrationError::NotIncreasingX(2)),
            "a raw score that does not increase"
        );
        assert_eq!(
            Calibration::from_knots(&[(0.1, 0.0), (1.0, 1.0)]),
            Err(CalibrationError::Anchors),
            "a curve that does not start at zero"
        );
        assert_eq!(
            Calibration::from_knots(&[(0.0, 0.0), (0.9, 1.0)]),
            Err(CalibrationError::Anchors),
            "a curve that does not end at one"
        );
        assert_eq!(
            Calibration::from_knots(&[(0.0, 0.0), (1.2, 1.0)]),
            Err(CalibrationError::KnotRange(1)),
            "a knot outside the unit square"
        );
        assert_eq!(
            Calibration::from_knots(&[(0.0, 0.0), (f32::NAN, 0.5), (1.0, 1.0)]),
            Err(CalibrationError::KnotRange(1)),
            "a knot that is not a number"
        );
    }

    // --- the artefact --------------------------------------------------------

    /// The shipped artefact is unfitted, and its bytes are the bytes its sha256 pin names.
    #[test]
    fn the_shipped_artefact_is_unfitted_and_its_pin_holds() {
        assert_eq!(shipped_artefact(), Ok(Artefact::Unfitted));
    }

    #[test]
    fn a_fitted_artefact_yields_its_table() {
        let artefact = parse_artefact(FITTED).expect("a fitted artefact parses");
        let Artefact::Fitted(curve) = artefact.clone() else {
            panic!("expected a fitted table, got {artefact:?}");
        };
        assert_eq!(curve.knots(), &[(0.0, 0.0), (0.5, 0.4), (1.0, 1.0)]);
        assert_eq!(
            calibration_for_switch(true, &artefact),
            Ok(Some(curve)),
            "a fitted table enables the switch"
        );
    }

    #[test]
    fn a_malformed_artefact_is_refused() {
        let wrong_schema = FITTED.replace("relevance-calibration-v1", "relevance-calibration-v0");
        assert!(matches!(
            parse_artefact(&wrong_schema),
            Err(CalibrationError::Malformed(_))
        ));
        let wrong_kind = FITTED.replace("cross_encoder.v1", "cross_encoder.v2");
        assert!(matches!(
            parse_artefact(&wrong_kind),
            Err(CalibrationError::Malformed(_))
        ));
        let fitted_without_knots = FITTED.replace(
            r#""knots": [[0.0, 0.0], [0.5, 0.4], [1.0, 1.0]]"#,
            r#""knots": null"#,
        );
        assert!(matches!(
            parse_artefact(&fitted_without_knots),
            Err(CalibrationError::Malformed(_))
        ));
        let unknown_state = FITTED.replace("\"fitted\"", "\"maybe\"");
        assert!(matches!(
            parse_artefact(&unknown_state),
            Err(CalibrationError::Malformed(_))
        ));
        let broken_curve = FITTED.replace("[0.5, 0.4]", "[0.5, 1.4]");
        assert_eq!(
            parse_artefact(&broken_curve),
            Err(CalibrationError::KnotRange(1)),
            "a fitted artefact is validated like any table"
        );
    }

    // --- the switch ----------------------------------------------------------

    #[test]
    fn the_switch_is_off_by_default_and_an_unfitted_table_is_then_no_error() {
        assert_eq!(parse_switch(None), Ok(false));
        assert_eq!(parse_switch(Some("")), Ok(false));
        assert_eq!(parse_switch(Some("0")), Ok(false));
        assert_eq!(calibration_for_switch(false, &Artefact::Unfitted), Ok(None));
    }

    #[test]
    fn the_switch_is_refused_at_boot_while_the_table_is_unfitted() {
        assert_eq!(parse_switch(Some("1")), Ok(true));
        assert_eq!(parse_switch(Some(" TRUE ")), Ok(true));
        assert_eq!(
            calibration_for_switch(true, &Artefact::Unfitted),
            Err(CalibrationError::SwitchWithoutCalibration)
        );
    }

    #[test]
    fn a_typo_in_the_switch_is_refused_rather_than_ignored() {
        assert_eq!(
            parse_switch(Some("ture")),
            Err(CalibrationError::UnreadableSwitch("ture".to_owned()))
        );
    }

    #[test]
    fn an_unfitted_artefact_must_not_carry_knots() {
        let unfitted_with_knots = r#"{
            "schema": "relevance-calibration-v1",
            "score_kind": "cross_encoder.v1",
            "state": "unfitted",
            "knots": [[0.0, 0.0], [1.0, 1.0]]
        }"#;
        assert!(matches!(
            parse_artefact(unfitted_with_knots),
            Err(CalibrationError::Malformed(_))
        ));
    }

    // --- the pin and the boot decision ---------------------------------------

    /// The pin names the digest the bytes must have. An edited artefact does not pass its old pin, a
    /// missing pin is refused, and the CR a Windows checkout leaves after the name is not part of the digest.
    #[test]
    fn the_pin_names_exactly_the_bytes_it_was_written_for() {
        let digest = sha256_hex(SHIPPED.as_bytes());
        assert_eq!(
            verify_pin(
                SHIPPED,
                &format!("{digest}  quarry-cross-encoder-v1.json\n")
            ),
            Ok(())
        );
        assert_eq!(
            verify_pin(
                SHIPPED,
                &format!("{digest}  quarry-cross-encoder-v1.json\r\n")
            ),
            Ok(())
        );
        let edited = SHIPPED.replace("\"unfitted\"", "\"fitted\"");
        assert!(
            matches!(
                verify_pin(
                    &edited,
                    &format!("{digest}  quarry-cross-encoder-v1.json\n")
                ),
                Err(CalibrationError::PinMismatch { .. })
            ),
            "an edited artefact must not pass its old pin"
        );
        assert!(
            matches!(
                verify_pin(SHIPPED, ""),
                Err(CalibrationError::PinMismatch { .. })
            ),
            "a missing pin is refused"
        );
    }

    /// The boot decision for each value the switch can hold, against the shipped (unfitted) table.
    #[test]
    fn the_boot_decision_refuses_the_switch_against_the_unfitted_table() {
        assert_eq!(from_switch_value(None), Ok(None), "unset is off");
        assert_eq!(
            from_switch_value(Some("off")),
            Ok(None),
            "an explicit off is off"
        );
        assert_eq!(
            from_switch_value(Some("on")),
            Err(CalibrationError::SwitchWithoutCalibration),
            "on against the unfitted table stops the boot"
        );
        assert_eq!(
            from_switch_value(Some("onn")),
            Err(CalibrationError::UnreadableSwitch("onn".to_owned())),
            "a typo stops the boot"
        );
    }
}
