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
