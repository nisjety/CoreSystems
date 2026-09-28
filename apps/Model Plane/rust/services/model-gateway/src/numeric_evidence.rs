//! Bounded arithmetic checks over exact CSV evidence and explicit prose
//! equations. Unrecognized prose still requires semantic source review.
use crate::source_facts::{cents, money};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::LazyLock};

type Groups = BTreeMap<Vec<(String, String)>, (i128, i128, Vec<usize>)>;

fn ratio(numerator: i128, denominator: i128) -> Option<String> {
    if denominator == 0 {
        return None;
    }
    let negative = (numerator < 0) != (denominator < 0);
    let numerator = numerator.checked_abs()?;
    let denominator = denominator.checked_abs()?;
    let mut scaled = (numerator / denominator).checked_mul(10_000)?;
    let mut remainder = numerator % denominator;
    for place in [1000, 100, 10, 1] {
        remainder = remainder.checked_mul(10)?;
        scaled += remainder / denominator * place;
        remainder %= denominator;
    }
    if remainder.checked_mul(2)? >= denominator {
        scaled += 1;
    }
    Some(format!(
        "{}{}.{:04}",
        if negative && scaled != 0 { "-" } else { "" },
        scaled / 10_000,
        scaled % 10_000
    ))
}

/// Compare two explicitly labelled weeks, never combine different channels or
/// product groups. More/ambiguous periods supply no implicit period selection.
pub(crate) fn comparisons(groups: &Groups) -> Vec<Value> {
    let mut periods = std::collections::BTreeSet::new();
    for dimensions in groups.keys() {
        for (key, value) in dimensions {
            if matches!(key.as_str(), "week" | "uke") {
                let Ok(week) = value.parse::<u32>() else {
                    return vec![];
                };
                if !(1..=53).contains(&week) {
                    return vec![];
                }
                periods.insert((week, key.clone(), value.clone()));
            }
        }
    }
    if periods.len() != 2 {
        return vec![];
    }
    let periods: Vec<_> = periods.into_iter().collect();
    if periods[0].1 != periods[1].1 {
        return vec![];
    }
    let mut result = Vec::new();
    for (dimensions, before) in groups {
        if !dimensions.contains(&(periods[0].1.clone(), periods[0].2.clone())) {
            continue;
        }
        let mut next = dimensions.clone();
        for (key, value) in &mut next {
            if key == &periods[0].1 {
                *value = periods[1].2.clone();
            }
        }
        let Some(after) = groups.get(&next) else {
            continue;
        };
        let mut metrics = serde_json::Map::new();
        for (metric, a, b) in [
            ("revenue", before.0, after.0),
            ("cost", before.1, after.1),
            ("grossProfit", before.0 - before.1, after.0 - after.1),
        ] {
            metrics.insert(metric.into(), json!({"before":money(a),"after":money(b),"change":money(b-a),"relativeChangePercent":ratio((b-a)*100,a)}));
        }
        metrics.insert(
            "marginChangePercentagePoints".into(),
            json!(if before.0 == 0 || after.0 == 0 {
                None
            } else {
                ratio(
                    100 * ((after.0 - after.1) * before.0 - (before.0 - before.1) * after.0),
                    after.0 * before.0,
                )
            }),
        );
        result.push(json!({"periodDimension":periods[0].1,"fromPeriod":periods[0].2,"toPeriod":periods[1].2,
            "dimensions":dimensions.iter().filter(|(key,_)|key != &periods[0].1).cloned().collect::<BTreeMap<_,_>>(),
            "beforeSourceLines":before.2,"afterSourceLines":after.2,"metrics":metrics}));
    }
    result
}

#[derive(Debug)]
struct Decimal {
    units: i128,
    scale: i128,
}
fn decimal(text: &str, english: bool) -> Option<Decimal> {
    let compact = text
        .replace([' ', '\u{a0}', '\u{202f}'], "")
        .replace('−', "-");
    let negative = compact.starts_with('-');
    let unsigned = compact.trim_start_matches(['+', '-']);
    let (group, separator) = if english { (',', '.') } else { ('.', ',') };
    let (integer, fraction) = if let Some((whole, fraction)) = unsigned.split_once(separator) {
        (whole.replace(group, ""), fraction.to_owned())
    } else if unsigned.contains(group) {
        let parts: Vec<_> = unsigned.split(group).collect();
        if parts.len() > 1 && parts.iter().skip(1).all(|part| part.len() == 3) {
            (parts.join(""), String::new())
        } else if !english && parts.len() == 2 {
            (parts[0].into(), parts[1].into())
        } else {
            return None;
        }
    } else {
        (unsigned.into(), String::new())
    };
    if integer.is_empty()
        || integer.len() > 16
        || fraction.len() > 6
        || !integer
            .chars()
            .chain(fraction.chars())
            .all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let scale = 10_i128.pow(fraction.len() as u32);
    let units = integer.parse::<i128>().ok()? * scale
        + if fraction.is_empty() {
            0
        } else {
            fraction.parse::<i128>().ok()?
        };
    Some(Decimal {
        units: if negative { -units } else { units },
        scale,
    })
}
fn rounded_ratio_matches(claim: &Decimal, numerator: i128, denominator: i128) -> bool {
    if denominator == 0 {
        return false;
    }
    // Compare in units of the claimed precision. Multiplying a large claimed
    // percentage by its baseline can overflow even for supported decimals.
    let Some(scaled) = numerator.checked_mul(claim.scale) else {
        return false;
    };
    let quotient = scaled / denominator;
    let remainder = scaled % denominator;
    let distance = quotient - claim.units;
    if distance == 0 {
        2 * remainder.abs() <= denominator.abs()
    } else if distance.abs() == 1 {
        let toward_claim = if (scaled < 0) != (denominator < 0) {
            -1
        } else {
            1
        };
        distance == -toward_claim && 2 * remainder.abs() >= denominator.abs()
    } else {
        false
    }
}

/// Local checks reject contradictions even if the reviewer would accept them.
/// Equation checks prove arithmetic consistency, not the source identity of
/// the stated endpoints. Ratio-word checks additionally bind a CSV series.
pub fn errors(computed: &[Value], candidate: &str) -> Vec<String> {
    if computed.is_empty() {
        return vec![];
    }
    static EQUATION: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(concat!(
        r"(?i)\b(?:bruttofortjenesten?|gross profit|omsetningen?|revenue|varekost(?:en)?|cost)[^\n!?]{0,140}?\b(?P<language>fra|from)\s+",
        r"(?P<before>[+−-]?\d[\d .,\x{00a0}\x{202f}]*?)\s*(?P<before_unit>NOK|kr|USD|EUR)?\s+(?:til|to)\s+",
        r"(?P<after>[+−-]?\d[\d .,\x{00a0}\x{202f}]*?)\s*(?P<after_unit>NOK|kr|USD|EUR)\s*\(\s*",
        r"(?P<change>[+−-]?\d[\d .,\x{00a0}\x{202f}]*?)\s*(?P<change_unit>NOK|kr|USD|EUR)(?:\s*,\s*(?P<percent>[+−-]?\d[\d.,]*)\s*%)?"
    )).unwrap()
    });
    let text = candidate.replace(['*', '_'], "");
    let mut errors = Vec::new();
    for line in text
        .lines()
        .filter(|line| !line.trim_start().starts_with('>'))
    {
        for claim in EQUATION.captures_iter(line) {
            let english = claim["language"].eq_ignore_ascii_case("from");
            let currency = |unit: &str| {
                if unit.eq_ignore_ascii_case("kr") {
                    "nok".to_owned()
                } else {
                    unit.to_lowercase()
                }
            };
            let after_currency = currency(&claim["after_unit"]);
            if currency(&claim["change_unit"]) != after_currency
                || claim
                    .name("before_unit")
                    .is_some_and(|unit| currency(unit.as_str()) != after_currency)
            {
                continue;
            }
            let (Some(before), Some(after), Some(change)) = (
                decimal(claim["before"].trim(), english),
                decimal(claim["after"].trim(), english),
                decimal(claim["change"].trim(), english),
            ) else {
                continue;
            };
            if before.scale > 100 || after.scale > 100 {
                continue;
            }
            let a = before.units * 100 / before.scale;
            let b = after.units * 100 / after.scale;
            let delta = b - a;
            let signed = claim["change"].trim().starts_with(['+', '-', '−']);
            let expected = if delta < 0 && !signed {
                delta.abs()
            } else {
                delta
            };
            if change.units * 100 != expected * change.scale {
                errors.push(format!("The stated change from {} to {} must be {} currency units, not {}. Correct the prose as well as any table.",money(a),money(b),money(delta),&claim["change"]));
            }
            if let Some(percent) = claim
                .name("percent")
                .and_then(|p| decimal(p.as_str(), english))
            {
                let numerator = if delta < 0 && !claim["percent"].starts_with(['+', '-', '−']) {
                    delta.abs() * 100
                } else {
                    delta * 100
                };
                if !rounded_ratio_matches(&percent, numerator, a) {
                    errors.push(format!("Relative change for {} to {} is {} percent using (after-before)/before; preserve rounding precision and do not confuse currency change with percent.",money(a),money(b),ratio(delta*100,a).unwrap_or_else(||"undefined (zero baseline)".into())));
                }
            }
        }
        let lower = line.to_lowercase();
        // A change/percentage-point claim stated without an explicit
        // "fra ... til" equation (a bare "falt/økte ... X" instead) is not
        // matched above: a false Q16 acceptance stated exactly this shape,
        // correctly, and no deterministic check bound it to the CSV either
        // way. Bind it directly to the exact CSV total or named group,
        // comparing the computed ground truth, never a re-derived
        // before/after. Ambiguous or unresolved scope is left to semantic
        // review rather than guessed, matching the ratio-word branch below.
        static DIRECT_CHANGE: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(concat!(
                r"(?i)\b(?P<metric>bruttofortjenesten?|gross profit|omsetningen?|revenue|varekost(?:en)?|cost|(?:brutto)?marginen?|margin)",
                r"[^\n!?]{0,80}?\b(?P<verb>økte|steg|vokste|rose|increased|grew|gikk opp|falt|sank|fell|decreased|dropped|gikk ned)\b",
                r"\s*(?:med\s+|by\s+)?(?P<amount>[+−-]?\d[\d .,\x{00a0}\x{202f}]*?)\s*(?P<unit>NOK|kr|USD|EUR|prosentpoeng|percentage\s+points|%)"
            )).unwrap()
        });
        // The metric->verb gap above is only bounded by newline/!/?, so a lazy
        // match can cross an entire other sentence to reach a verb whose
        // amount happens to parse, including a verb that belongs to a
        // DIFFERENT metric's own clause (a false Q16 acceptance's mirror
        // image: a false rejection that bound one metric's verb+amount to
        // another metric word entirely). classify_metric lets the loop below
        // refuse any match whose gap itself contains another metric's word.
        fn classify_metric(word: &str) -> &'static str {
            let word = word.to_lowercase();
            if word.contains("margin") {
                "margin"
            } else if word.contains("bruttofortjeneste") || word.contains("gross profit") {
                "grossProfit"
            } else if word.contains("omsetning") || word.contains("revenue") {
                "revenue"
            } else {
                "cost"
            }
        }
        static METRIC_WORD: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(
                r"(?i)\b(?:bruttofortjenesten?|gross profit|omsetningen?|revenue|varekost(?:en)?|cost|(?:brutto)?marginen?|margin)\b",
            )
            .unwrap()
        });
        // A per-unit/average/otherwise differently-scoped figure that merely
        // shares a metric word with the CSV total is not the CSV's own
        // tracked total; leave it to semantic review rather than binding it.
        let disqualified_scope = [
            "per enhet", "per unit", "per stk", "per stykk", "gjennomsnitt", "average",
            "per order", "per ordre",
        ]
        .iter()
        .any(|term| lower.contains(term));
        if computed.len() == 1 {
            if let Some(comparisons) = computed[0]["comparisons"].as_array() {
                for claim in DIRECT_CHANGE.captures_iter(line) {
                    if disqualified_scope {
                        continue;
                    }
                    let metric = classify_metric(&claim["metric"]);
                    let gap = &line[claim.name("metric").unwrap().end()..claim.name("verb").unwrap().start()];
                    if METRIC_WORD.find_iter(gap).any(|other| classify_metric(other.as_str()) != metric) {
                        continue;
                    }
                    let unit = claim["unit"].to_lowercase();
                    let is_points = unit.contains("prosentpoeng") || unit.contains("percentage");
                    let is_percent = unit == "%";
                    // Margin moves in percentage points; the other metrics
                    // never do (a bare "%" for them is a relative change).
                    if metric == "margin" {
                        if !is_points {
                            continue;
                        }
                    } else if is_points {
                        continue;
                    }
                    // The CSV's own figures are always NOK; a currency literal
                    // that is not kr/NOK is not directly comparable and needs
                    // scoped source review instead, matching the EQUATION
                    // branch's currency-consistency guard above.
                    if !is_points && !is_percent && unit != "nok" && unit != "kr" {
                        continue;
                    }
                    // The regex matches both Norwegian and English verbs; parse
                    // the claimed amount with the format that verb implies
                    // instead of hardcoding Norwegian for every match.
                    let english = matches!(
                        claim["verb"].to_lowercase().as_str(),
                        "increased" | "grew" | "fell" | "decreased" | "dropped" | "rose"
                    );
                    let Some(amount) = decimal(claim["amount"].trim(), english) else {
                        continue;
                    };
                    let down = ["falt", "sank", "fell", "decreased", "dropped", "gikk ned"]
                        .iter()
                        .any(|word| claim["verb"].eq_ignore_ascii_case(word));
                    // An explicit sign in the prose is preserved as typed; a
                    // bare magnitude takes the verb's own direction.
                    let signed_units = if claim["amount"].trim().starts_with(['+', '-', '−']) {
                        amount.units
                    } else if down {
                        -amount.units.abs()
                    } else {
                        amount.units.abs()
                    };
                    let mut product_values = std::collections::BTreeSet::new();
                    let mut channel_values = std::collections::BTreeSet::new();
                    for entry in comparisons {
                        if let Some(dimensions) = entry["dimensions"].as_object() {
                            for (key, value) in dimensions {
                                if let Some(value) = value
                                    .as_str()
                                    .filter(|value| word_present(&lower, &value.to_lowercase()))
                                {
                                    match key.as_str() {
                                        "produktgruppe" | "product_group" => {
                                            product_values.insert(value.to_owned());
                                        }
                                        "kanal" | "channel" => {
                                            channel_values.insert(value.to_owned());
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    // More than one named group in scope, or no CSV row that
                    // matches, has no single ground truth to bind to.
                    if product_values.len() > 1 || channel_values.len() > 1 {
                        continue;
                    }
                    let matches: Vec<_> = comparisons
                        .iter()
                        .filter(|entry| {
                            let dimensions = &entry["dimensions"];
                            let product = dimensions
                                .get("produktgruppe")
                                .or_else(|| dimensions.get("product_group"))
                                .and_then(Value::as_str);
                            let channel = dimensions
                                .get("kanal")
                                .or_else(|| dimensions.get("channel"))
                                .and_then(Value::as_str);
                            let product_ok = match product {
                                Some(v) => product_values.contains(v),
                                None => product_values.is_empty(),
                            };
                            let channel_ok = match channel {
                                Some(v) => channel_values.contains(v),
                                None => channel_values.is_empty(),
                            };
                            product_ok && channel_ok
                        })
                        .collect();
                    if matches.len() != 1 {
                        continue;
                    }
                    let entry = matches[0];
                    if is_percent || is_points {
                        // Reconstruct the exact numerator/denominator from the
                        // cents already available, and compare with tolerance
                        // for the claim's own rounding precision instead of
                        // cross-multiplying against relativeChangePercent's
                        // pre-rounded 4-decimal display string: correctly
                        // rounded prose stated at a coarser precision must
                        // still pass.
                        let Some((numerator, denominator)) = (|| {
                            if metric == "margin" {
                                let revenue_before = entry["metrics"]["revenue"]["before"].as_str().and_then(cents)?;
                                let revenue_after = entry["metrics"]["revenue"]["after"].as_str().and_then(cents)?;
                                let cost_before = entry["metrics"]["cost"]["before"].as_str().and_then(cents)?;
                                let cost_after = entry["metrics"]["cost"]["after"].as_str().and_then(cents)?;
                                if revenue_before == 0 || revenue_after == 0 {
                                    return None; // zero baseline/undefined: no ground truth to bind to
                                }
                                Some((
                                    100 * ((revenue_after - cost_after) * revenue_before
                                        - (revenue_before - cost_before) * revenue_after),
                                    revenue_after * revenue_before,
                                ))
                            } else {
                                let metric_before = entry["metrics"][metric]["before"].as_str().and_then(cents)?;
                                let metric_after = entry["metrics"][metric]["after"].as_str().and_then(cents)?;
                                if metric_before == 0 {
                                    return None; // zero baseline/undefined: no ground truth to bind to
                                }
                                Some(((metric_after - metric_before) * 100, metric_before))
                            }
                        })() else {
                            continue;
                        };
                        let claimed = Decimal { units: signed_units, scale: amount.scale };
                        if !rounded_ratio_matches(&claimed, numerator, denominator) {
                            let text = ratio(numerator, denominator)
                                .unwrap_or_else(|| "undefined (zero baseline)".into());
                            errors.push(format!(
                                "The stated {metric} change of {}{} does not match the computed {} for {} between {} and {}. Bind changes to computedCsv, never a restated before/after.",
                                claim["amount"].trim(), if metric == "margin" { " percentage points" } else { "%" }, text, entry["dimensions"], entry["fromPeriod"], entry["toPeriod"]
                            ));
                        }
                    } else {
                        let Some(text) = entry["metrics"][metric]["change"].as_str() else {
                            continue;
                        };
                        let Some(expected) = cents(text) else {
                            continue;
                        };
                        let (Some(lhs), Some(rhs)) = (
                            signed_units.checked_mul(100),
                            expected.checked_mul(amount.scale),
                        ) else {
                            continue;
                        };
                        if lhs != rhs {
                            errors.push(format!(
                                "The stated {metric} change of {} {} does not match the computed change of {} for {} between {} and {}. Bind changes to computedCsv, never a restated before/after.",
                                claim["amount"].trim(), claim["unit"].trim(), money(expected), entry["dimensions"], entry["fromPeriod"], entry["toPeriod"]
                            ));
                        }
                    }
                }
            }
        }
        let factor = if ["halverte", "halvert", "halved"]
            .iter()
            .any(|word| word_present(&lower, word))
        {
            Some((1, 2))
        } else if ["doblet", "doubled"]
            .iter()
            .any(|word| word_present(&lower, word))
        {
            Some((2, 1))
        } else {
            None
        };
        let Some((numerator, denominator)) = factor else {
            continue;
        };
        if [
            "ikke halvert",
            "ikke doblet",
            "not halved",
            "not doubled",
            "hvis ",
            "dersom ",
            "if ",
            "hypotes",
            "scenario",
            "forslag",
            "proposal",
        ]
        .iter()
        .any(|term| lower.contains(term))
        {
            continue;
        }
        let metrics: Vec<_> = [
            (
                "revenue",
                lower.contains("omsetning") || lower.contains("revenue"),
            ),
            (
                "grossProfit",
                lower.contains("bruttofortjeneste") || lower.contains("gross profit"),
            ),
            (
                "cost",
                lower.contains("varekost") || word_present(&lower, "cost"),
            ),
        ]
        .into_iter()
        .filter_map(|(metric, present)| present.then_some(metric))
        .collect();
        // Multiple financial metrics can belong to different clauses. Do not
        // attach a ratio word to whichever metric happens to match first.
        if metrics.len() != 1 {
            continue;
        }
        let metric = metrics[0];
        // Multiple CSVs or period pairs can express different scopes. Do not
        // select one by guessing. A later typed claim contract can cover them.
        if computed.len() != 1 {
            continue;
        }
        let Some(comparisons) = computed[0]["comparisons"].as_array() else {
            continue;
        };
        let mut product_values = std::collections::BTreeSet::new();
        let mut channel_values = std::collections::BTreeSet::new();
        for entry in comparisons {
            if let Some(dimensions) = entry["dimensions"].as_object() {
                for (key, value) in dimensions {
                    if let Some(value) = value
                        .as_str()
                        .filter(|value| word_present(&lower, &value.to_lowercase()))
                    {
                        match key.as_str() {
                            "produktgruppe" | "product_group" => {
                                product_values.insert(value.to_owned());
                            }
                            "kanal" | "channel" => {
                                channel_values.insert(value.to_owned());
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        if product_values.len() > 1 || (product_values.is_empty() && !lower.contains("total")) {
            continue;
        }
        // A combined total across named channels does not establish that
        // each channel changed by that ratio.
        if channel_values.len() > 1
            && !["både", "both", "hver", "each"]
                .iter()
                .any(|word| word_present(&lower, word))
        {
            continue;
        }
        for entry in comparisons {
            let dimensions = &entry["dimensions"];
            let product = dimensions
                .get("produktgruppe")
                .or_else(|| dimensions.get("product_group"))
                .and_then(Value::as_str);
            let channel = dimensions
                .get("kanal")
                .or_else(|| dimensions.get("channel"))
                .and_then(Value::as_str);
            if !match product {
                Some(value) => product_values.contains(value),
                None => product_values.is_empty(),
            } {
                continue;
            }
            if !match channel {
                Some(value) => channel_values.contains(value),
                None => channel_values.is_empty(),
            } {
                continue;
            }
            let (Some(a), Some(b)) = (
                entry["metrics"][metric]["before"].as_str().and_then(cents),
                entry["metrics"][metric]["after"].as_str().and_then(cents),
            ) else {
                continue;
            };
            if b * denominator != a * numerator {
                errors.push(format!("The asserted {} ratio for {metric} is contradicted by CSV dimensions {} in periods {} → {}: {} → {}. Do not describe this as halving/doubling.",if numerator==1{"one-half"}else{"twofold"},dimensions,entry["fromPeriod"],entry["toPeriod"],money(a),money(b)));
            }
        }
    }
    errors
}

fn word_present(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(start, _)| {
        text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric())
            && text[start + word.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_validation::{Source, SourceContext};
    fn ledger(csv: &str) -> Vec<Value> {
        crate::source_facts::computed_csv(&SourceContext {
            sources: vec![Source {
                id: 0,
                name: "data.csv".into(),
                content: csv.into(),
            }],
        })
    }
    #[test]
    fn rejects_saved_prose_even_when_the_table_is_correct() {
        let data = ledger(
            "week,product_group,revenue,cost\n36,Tables,360000,237000\n37,Tables,360000,235320",
        );
        let wrong="Bruttofortjenesten steg marginalt fra 123 000 til 124 680 NOK (+1 368 NOK, +1,1 %).\n| Profit | 123 000 | 124 680 | +1 680 | +1,37 % |";
        assert_eq!(errors(&data, wrong).len(), 2);
        assert!(errors(
            &data,
            "Bruttofortjenesten steg fra **123 000** til **124 680 NOK** (+1 680 NOK, +1,37 %)."
        )
        .is_empty());
        assert!(errors(
            &data,
            "Gross profit rose from 123,000 to 124,680 USD (+1,680 USD, +1.4%)."
        )
        .is_empty());
        assert!(errors(&data, "Revenue fell from 100 to 75 EUR (25 EUR, 25%).").is_empty());
        assert!(!errors(&data, "Revenue fell from 100 to 75 EUR (+25 EUR, +25%).").is_empty());
        assert!(!errors(&data, "Revenue rose from 0 to 10 EUR (+10 EUR, +100%).").is_empty());
        assert!(!errors(&data, "Revenue rose from 9999999999999998 to 9999999999999999 EUR (+1 EUR, +9999999999999999.999999%).").is_empty());
        assert!(errors(
            &data,
            "> Bruttofortjenesten steg fra 123 000 til 124 680 NOK (+1 368 NOK)."
        )
        .is_empty());
        assert!(
            errors(
                &data,
                "Revenue moved from 100 USD to 75 EUR (+20 EUR, +20%)."
            )
            .is_empty(),
            "mixed currencies require scoped source review, not subtraction across units"
        );
    }
    #[test]
    fn ratio_words_bind_to_the_named_product_and_channels() {
        let data=ledger("uke,produktgruppe,kanal,omsetning_nok,varekost_nok\n8,Bord,Nettbutikk,100,50\n8,Bord,Bedriftssalg,200,100\n9,Bord,Nettbutikk,70,35\n9,Bord,Bedriftssalg,140,70\n8,Lamper,Nettbutikk,40,20\n9,Lamper,Nettbutikk,20,10");
        assert_eq!(
            errors(
                &data,
                "Bord: Både Nettbutikk og Bedriftssalg halverte volum og omsetning."
            )
            .len(),
            2
        );
        assert!(errors(&data, "Lamper halverte omsetningen i Nettbutikk.").is_empty());
        assert!(errors(&data, "Bord har ikke halvert omsetningen.").is_empty());
        assert!(errors(
            &data,
            "Bord: Omsetningen falt 30 %, og varekost ble halvert."
        )
        .is_empty());
        assert!(errors(&data, "Total omsetning for Bord og Lamper ble halvert.").is_empty());
        assert!(errors(
            &data,
            "Bord: Total omsetning i Nettbutikk og Bedriftssalg ble halvert."
        )
        .is_empty());
        assert!(errors(
            &data,
            "Forslag: hvis Bord halverte omsetningen, må tiltak vurderes."
        )
        .is_empty());
        assert!(
            errors(&data, "Omsetningen ble halvert.").is_empty(),
            "ambiguous product scope remains semantic review"
        );
    }
    /// The real failing shape from the Q16 false-acceptance investigation, in
    /// generic form: a bare "falt/økte ... X" claim with no "fra ... til"
    /// equation. A wrong total must fail, correct group and total figures
    /// (currency and percentage-point) must pass, and a number with no
    /// recognized CSV metric word must never be flagged.
    #[test]
    fn direct_change_claims_bind_to_the_csv_total_or_named_group() {
        let data = ledger(concat!(
            "week,produktgruppe,revenue,cost\n",
            "36,Belysning,120000,72000\n37,Belysning,150000,90000\n",
            "36,Skrivebord,180000,126000\n37,Skrivebord,126000,90720\n",
            "36,Oppbevaring,60000,39000\n37,Oppbevaring,84000,54600\n",
        ));
        // A wrong total change must fail. Totalt actually rose 1 680 kr; this
        // sentence names no group, so it binds to the grand total and is
        // contradicted by it (this is the exact wrong number a real
        // per-group claim carried, mis-scoped here to the total on purpose).
        assert!(!errors(&data, "Bruttofortjenesten falt 18 720 kr i perioden.").is_empty());
        // Correct figures must pass: the real total claim,
        assert!(errors(
            &data,
            "Bruttofortjenesten økte med 1 680 kr til 124 680 kr."
        )
        .is_empty());
        // and the real per-group claim, scoped by a group name stated
        // earlier in the same bullet/paragraph (not a re-split "sentence").
        assert!(errors(
            &data,
            "Skrivebord sto for hele nettofallet. Bruttofortjenesten falt 18 720 kr, og marginen gikk ned 2,00 prosentpoeng."
        )
        .is_empty());
        // A wrong group figure still fails once scoped.
        assert!(!errors(&data, "Skrivebord: Bruttofortjenesten falt 20 000 kr.").is_empty());
        // Correct and wrong relative-percent claims for a named group.
        assert!(errors(&data, "Skrivebord: Omsetningen falt 30 %.").is_empty());
        assert!(!errors(&data, "Skrivebord: Omsetningen falt 25 %.").is_empty());
        // Unrelated numbers, with no recognized CSV metric word, are never
        // flagged even though they share the same change-verb shape.
        assert!(errors(&data, "Antall ansatte økte med 1 680 til 5 000.").is_empty());
        assert!(errors(&data, "Belysning: Antall enheter falt 12 000 stk.").is_empty());
        // Ambiguous (multi-group) scope is left to semantic review, not guessed.
        assert!(errors(
            &data,
            "Belysning og Skrivebord: Bruttofortjenesten falt 18 720 kr."
        )
        .is_empty());
    }
    /// The exact take-04 false-rejection shape: the metric->verb gap is only
    /// bounded by newline/!/?, so a lazy match can leap over an intervening,
    /// differently-named metric's own clause and bind that metric's verb and
    /// amount to the first metric word instead, producing a phantom claim
    /// that never matches the CSV. Both reproduced shapes (an immediately
    /// adjacent but downstream-failing verb, and a verb reached only because
    /// an even later one's amount happens to parse) must be left unbound
    /// rather than falsely rejected.
    #[test]
    fn direct_change_never_binds_a_verb_across_another_metrics_own_clause() {
        let data = ledger("week,revenue,cost\n36,100,70\n37,100,65");
        // Revenue is unchanged; grossProfit genuinely rose by 5 (70->65 cost).
        // The bug bound grossProfit's own "økte med 5 kr" to "Omsetningen"
        // (revenue), wrongly flagging a revenue change of 5 against 0.00.
        assert!(errors(
            &data,
            "Omsetningen var uendret på 100 kr i uke 37. Bruttofortjenesten økte med 5 kr til 35 kr."
        )
        .is_empty());
        let data = ledger("week,revenue,cost\n36,100,50\n37,200,50");
        // grossProfit rose by 100 kr; cost is unchanged. The bug skipped the
        // immediately-adjacent, correct verb ("økte", whose amount can't
        // parse because "fordi" intervenes) and backtracked all the way to
        // cost's own "falt med 5 kr", inverting the sign and metric both.
        assert!(errors(
            &data,
            "Total bruttofortjeneste økte fordi varekost falt med 5 kr."
        )
        .is_empty());
    }
    /// The percent/margin-points comparison must tolerate the claim's own
    /// rounding precision (rounded_ratio_matches), not demand exact equality
    /// to relativeChangePercent's pre-rounded 4-decimal display string.
    #[test]
    fn direct_change_percent_tolerates_the_claims_own_rounding_precision() {
        // True change is 30/70 = 42.857142...%.
        let data = ledger("week,revenue,cost\n1,70,10\n2,100,10");
        assert!(
            errors(&data, "Revenue increased by 42.86%.").is_empty(),
            "a textbook-correct 2-decimal rounding of the true percentage must pass"
        );
        assert!(
            errors(&data, "Revenue increased by 42.8571%.").is_empty(),
            "the ground truth's own 4-decimal precision must still pass"
        );
        assert!(
            !errors(&data, "Revenue increased by 40%.").is_empty(),
            "a genuinely wrong percentage must still fail"
        );
    }
    /// DIRECT_CHANGE matches both Norwegian and English verbs but must parse
    /// the claimed amount using the number format that verb implies, not
    /// hardcode Norwegian formatting for every match.
    #[test]
    fn direct_change_parses_the_amount_in_the_verbs_own_language() {
        let data = ledger("week,revenue,cost\n1,1000,0\n2,2234,0");
        assert!(
            errors(&data, "Revenue increased by 1,234 NOK.").is_empty(),
            "an English comma-thousands amount must not be misread as a decimal fraction"
        );
        assert!(
            errors(&data, "Revenue increased by 1,234.00 NOK.").is_empty(),
            "full English formatting (comma-thousands, period-decimal) must parse"
        );
        assert!(
            !errors(&data, "Revenue increased by 1,999.00 NOK.").is_empty(),
            "a genuinely wrong English-formatted amount must still be caught"
        );
    }
    /// DIRECT_CHANGE must not compare a claim's magnitude directly against
    /// the NOK CSV when the claim's own currency literal is something else;
    /// that requires scoped source review, not cross-currency subtraction,
    /// matching the EQUATION branch's own currency-consistency guard.
    #[test]
    fn direct_change_stands_aside_on_a_mismatched_currency_literal() {
        let data = ledger("week,revenue,cost\n1,1000,0\n2,2234,0");
        assert!(
            errors(&data, "Revenue increased by 5000 USD.").is_empty(),
            "mixed currencies require scoped source review, not direct subtraction"
        );
    }
    /// A figure that merely shares a metric word with the CSV total, but is
    /// qualified as per-unit or average, is not the CSV's own tracked total
    /// and must be left to semantic review rather than bound to it.
    #[test]
    fn direct_change_excludes_per_unit_and_average_qualified_figures() {
        let data = ledger(concat!(
            "week,produktgruppe,revenue,cost\n",
            "36,Belysning,120000,72000\n37,Belysning,150000,90000\n",
            "36,Skrivebord,180000,126000\n37,Skrivebord,126000,90720\n",
            "36,Oppbevaring,60000,39000\n37,Oppbevaring,84000,54600\n",
        ));
        // Actual total cost change is -1 680 kr; these wildly different
        // per-unit/average figures must never be bound to it.
        assert!(errors(&data, "Cost per unit fell 999 999 kr this week.").is_empty());
        assert!(errors(&data, "Varekost per enhet falt 999 999 kr denne uken.").is_empty());
    }
    /// DIRECT_CHANGE (and the pre-existing ratio-word branch) only bind a
    /// claim when exactly one CSV source is attached (`computed.len() == 1`).
    /// With two attached CSVs the check is a documented no-op: a wrong claim
    /// is silently left unchecked rather than guessed against either source.
    #[test]
    fn direct_change_and_ratio_words_are_a_documented_no_op_with_two_csv_sources() {
        let computed = crate::source_facts::computed_csv(&SourceContext {
            sources: vec![
                Source { id: 0, name: "a.csv".into(), content: "week,revenue,cost\n1,100,50\n2,150,50".into() },
                Source { id: 1, name: "b.csv".into(), content: "week,revenue,cost\n1,200,80\n2,100,80".into() },
            ],
        });
        assert_eq!(computed.len(), 2);
        assert!(
            errors(&computed, "Revenue increased by 9999 kr.").is_empty(),
            "DIRECT_CHANGE does not guess which of two attached CSVs a claim belongs to"
        );
        assert!(
            errors(&computed, "Omsetningen ble doblet.").is_empty(),
            "the pre-existing ratio-word branch has the same documented limitation"
        );
    }
    #[test]
    fn computed_deltas_keep_scope_lines_and_zero_denominators() {
        let data = ledger("week,product_group,revenue,cost\n6,A,0,0\n7,A,1,0.33");
        let comparison = &data[0]["comparisons"][0];
        assert_eq!(comparison["metrics"]["grossProfit"]["change"], "0.67");
        assert!(comparison["metrics"]["revenue"]["relativeChangePercent"].is_null());
        assert_eq!(comparison["beforeSourceLines"], json!([1]));
        assert_eq!(comparison["afterSourceLines"], json!([2]));
        assert!(
            ledger("week,revenue,cost\n1,1,0\n2,2,0\n3,3,0")[0]["comparisons"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(ratio(-1, 3).unwrap(), "-0.3333");
        assert_eq!(
            ratio(4 * 10_i128.pow(36), 4 * 10_i128.pow(34)).unwrap(),
            "100.0000"
        );
    }
}
