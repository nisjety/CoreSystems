//! Computed calendar facts for dates in the user's brief, including dated fixtures.
//! These are calendar days, not promises about holidays or business availability.

use chrono::{Datelike, Duration, NaiveDate};
use mp_contracts::model_plane::v1::ChatMessage;
use std::{collections::BTreeSet, sync::OnceLock};

fn month(word: &str) -> Option<u32> {
    match word {
        "januar" | "january" => Some(1),
        "februar" | "february" => Some(2),
        "mars" | "march" => Some(3),
        "april" => Some(4),
        "mai" | "may" => Some(5),
        "juni" | "june" => Some(6),
        "juli" | "july" => Some(7),
        "august" => Some(8),
        "september" => Some(9),
        "oktober" | "october" => Some(10),
        "november" => Some(11),
        "desember" | "december" => Some(12),
        _ => None,
    }
}

/// Track fenced examples across paragraph boundaries. Only a matching fence
/// character of at least the opening length closes the block.
pub(crate) fn update_markdown_fence(line: &str, fence: &mut Option<(char, usize)>) -> bool {
    let line = line.trim_start();
    let Some(marker @ ('`' | '~')) = line.chars().next() else {
        return false;
    };
    let length = line.chars().take_while(|ch| *ch == marker).count();
    if length < 3 {
        return false;
    }
    if let Some((opened, minimum)) = *fence {
        if marker != opened || length < minimum || !line[length..].trim().is_empty() {
            return false;
        }
        *fence = None;
    } else {
        *fence = Some((marker, length));
    }
    true
}

/// Check an explicitly paired date and weekday, without inferring availability
/// or interpreting a date range as a single date. Yearless dates are checked
/// only when the user's material supplies exactly one possible year.
pub fn weekday_errors(messages: &[ChatMessage], candidate: &str) -> Vec<String> {
    static YEARS: OnceLock<regex::Regex> = OnceLock::new();
    static PAIRS: OnceLock<regex::Regex> = OnceLock::new();
    let year_pattern =
        YEARS.get_or_init(|| regex::Regex::new(r"\b(?:19\d{2}|20\d{2}|21\d{2}|2200)\b").unwrap());
    let years: BTreeSet<i32> = messages
        .iter()
        .filter(|message| message.role == "user")
        .flat_map(|message| year_pattern.find_iter(&message.content))
        .filter_map(|value| value.as_str().parse().ok())
        .collect();
    let year = (years.len() == 1).then(|| *years.first().unwrap());
    let pairs = PAIRS.get_or_init(|| regex::Regex::new(concat!(
        r"(?i)\b(\d{1,2})\.?\s+",
        r"(januar|january|februar|february|mars|march|april|mai|may|juni|june|juli|july|august|september|oktober|october|november|desember|december|jan|feb|mar|apr|jun|jul|aug|sep|sept|okt|oct|nov|des|dec)\.?",
        r"(?:\s+((?:19|20|21)\d{2}|2200))?\s*\(\s*",
        r"(mandag|monday|man|mon|tirsdag|tuesday|tir|tue|onsdag|wednesday|ons|wed|torsdag|thursday|tor|thu|fredag|friday|fre|fri|lørdag|saturday|lør|sat|søndag|sunday|søn|sun)\.?\s*\)"
    )).unwrap());
    // Quoted source passages and code examples are not asserted calendar labels.
    let mut fence = None;
    let text = candidate
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            if update_markdown_fence(line, &mut fence) {
                return false;
            }
            fence.is_none() && !line.starts_with('>') && !line.contains('`')
        })
        .collect::<Vec<_>>()
        .join("\n")
        .replace('*', "");
    pairs.captures_iter(&text).filter_map(|pair| {
        let year = pair.get(3).and_then(|value| value.as_str().parse().ok()).or(year)?;
        let name = pair[2].to_lowercase();
        let month = month(&name).or_else(|| match name.as_str() {
            "jan" => Some(1), "feb" => Some(2), "mar" => Some(3), "apr" => Some(4),
            "jun" => Some(6), "jul" => Some(7), "aug" => Some(8), "sep" | "sept" => Some(9),
            "okt" | "oct" => Some(10), "nov" => Some(11), "des" | "dec" => Some(12), _ => None,
        })?;
        let date = NaiveDate::from_ymd_opt(year, month, pair[1].parse().ok()?)?;
        let weekday = match pair[4].to_lowercase().as_str() {
            "mandag" | "monday" | "man" | "mon" => chrono::Weekday::Mon,
            "tirsdag" | "tuesday" | "tir" | "tue" => chrono::Weekday::Tue,
            "onsdag" | "wednesday" | "ons" | "wed" => chrono::Weekday::Wed,
            "torsdag" | "thursday" | "tor" | "thu" => chrono::Weekday::Thu,
            "fredag" | "friday" | "fre" | "fri" => chrono::Weekday::Fri,
            "lørdag" | "saturday" | "lør" | "sat" => chrono::Weekday::Sat,
            _ => chrono::Weekday::Sun,
        };
        (weekday != date.weekday()).then(|| format!("Gregorian date {date} is {}, not {weekday}. Correct the weekday label; this does not approve the proposed date, dependencies or availability.", date.weekday()))
    }).collect()
}

pub fn reference_calendar(messages: &[ChatMessage]) -> Option<String> {
    let texts: Vec<Vec<String>> = messages
        .iter()
        .filter(|m| m.role == "user")
        .map(|m| {
            m.content
                .split_whitespace()
                .take(25_000)
                .map(|w| {
                    w.trim_matches(|c: char| !c.is_alphanumeric())
                        .to_lowercase()
                })
                .collect()
        })
        .collect();
    let years: BTreeSet<i32> = texts
        .iter()
        .flatten()
        .filter_map(|w| {
            (w.len() == 4)
                .then(|| w.parse::<i32>().ok())
                .flatten()
                .filter(|y| (1900..=2200).contains(y))
        })
        .collect();
    let unambiguous_year = (years.len() == 1).then(|| *years.first().unwrap());
    let mut dates = BTreeSet::new();
    let mut scenario_date = None;
    for words in &texts {
        let fictional = words
            .iter()
            .any(|w| w.starts_with("fiktiv") || w == "fictional" || w == "demogrunnlag");
        for (index, pair) in words.windows(2).enumerate() {
            let (day, month) = match (pair[0].parse::<u32>(), month(&pair[1])) {
                (Ok(day), Some(month)) => (day, month),
                _ => continue,
            };
            let explicit_year = words
                .get(index + 2)
                .and_then(|w| w.parse::<i32>().ok())
                .filter(|y| (1900..=2200).contains(y));
            let Some(year) = explicit_year.or(unambiguous_year) else {
                continue;
            };
            let Some(date) = NaiveDate::from_ymd_opt(year, month, day) else {
                continue;
            };
            if fictional
                && index >= 2
                && matches!(words[index - 2].as_str(), "situasjon" | "status" | "as")
                && matches!(words[index - 1].as_str(), "per" | "of")
            {
                scenario_date = Some(date);
            }
            if dates.len() >= 32 {
                break;
            }
            // Include the next three dates so next-weekday reasoning around a
            // weekend has computed calendar evidence too. Holidays are not inferred.
            for offset in 0..=3 {
                if dates.len() < 32 {
                    dates.insert(date + Duration::days(offset));
                }
            }
        }
    }
    if dates.is_empty() {
        return None;
    }
    let rows = dates
        .iter()
        .map(|d| format!("{d}: {}", d.weekday()))
        .collect::<Vec<_>>()
        .join("; ");
    let anchor = scenario_date.map(|date| {
        let mut next = date + Duration::days(1);
        while matches!(next.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun) { next += Duration::days(1); }
        format!(" This explicitly fictional brief sets its scenario clock to {date} ({}). Relative deadlines in the draft belong to that scenario, not today's real date, unless the user explicitly changes the scenario. The next Monday-Friday date after that scenario date is {next} ({}); this does not account for holidays.", date.weekday(), next.weekday())
    }).unwrap_or_default();
    Some(format!("Computed calendar reference for dates in the user's material (Gregorian): {rows}. Use these weekdays rather than guessing. This table does not establish working days, holidays, delivery availability, or permission to take an action. Keep a source's relative deadline when no exact date is needed.{anchor}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference(text: &str) -> Option<String> {
        reference_calendar(&[ChatMessage {
            role: "user".into(),
            content: text.into(),
            ..Default::default()
        }])
    }
    #[test]
    fn checks_explicit_weekday_pairs_without_inventing_a_year_or_business_availability() {
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: "Planning: 14. september 2026.".into(),
            ..Default::default()
        }];
        let errors = weekday_errors(
            &messages,
            "**22. september (mandag)**; 23. sep (tir); 24. September (Wednesday)",
        );
        assert_eq!(errors.len(), 3);
        assert!(errors[0].contains("2026-09-22 is Tue, not Mon"));
        assert!(weekday_errors(
            &messages,
            "22. september (tirsdag); 23. sep (ons); 24. September (Thursday)"
        )
        .is_empty());
        assert!(weekday_errors(&messages, "17.–18. sep (tor–fre)").is_empty());
        assert!(weekday_errors(
            &messages,
            "> Source quote: 22. september (mandag)\n\n```\n22. september (mandag)\n```"
        )
        .is_empty());
        assert!(weekday_errors(&[], "22. september (mandag)").is_empty());
        assert_eq!(weekday_errors(&[], "22. september 2026 (mandag)").len(), 1);
        let ambiguous = vec![ChatMessage {
            role: "user".into(),
            content: "Plans for 2025 and 2026".into(),
            ..Default::default()
        }];
        assert!(weekday_errors(&ambiguous, "22. september (mandag)").is_empty());
        assert!(weekday_errors(
            &messages,
            "A quote from last year: 22. september 2025 (mandag)"
        )
        .is_empty());
    }
    #[test]
    fn computes_the_fixture_weekday_and_next_date_without_relying_on_today() {
        let result = reference("Situasjon per 14. september 2026. Ankomst 17. september.").unwrap();
        assert!(result.contains("2026-09-14: Mon"));
        assert!(result.contains("2026-09-15: Tue"));
        assert!(result.contains("2026-09-17: Thu"));
        assert!(!result.contains("2026-09-15: Mon"));
    }
    #[test]
    fn does_not_guess_a_year_when_the_brief_is_ambiguous() {
        let result =
            reference("2025 and 2026. Meeting 15. september. Baseline 14. september 2026.")
                .unwrap();
        assert!(!result.contains("2025-09"));
        assert!(reference("Meeting 15. september.").is_none());
    }
    #[test]
    fn invalid_dates_are_not_calendar_evidence() {
        assert!(reference("31. februar 2026").is_none());
        assert!(reference("29. februar 2025").is_none());
        assert!(reference("29. februar 2024")
            .unwrap()
            .contains("2024-02-29: Thu"));
    }
    #[test]
    fn calendar_does_not_claim_that_weekdays_are_business_days() {
        let result = reference("18. september 2026").unwrap();
        assert!(result.contains("2026-09-21: Mon"));
        assert!(result.contains("does not establish working days"));
    }
    #[test]
    fn a_fictional_snapshot_has_its_own_clock_but_real_documents_do_not() {
        let result = reference("DEMOGRUNNLAG: Alle opplysninger er fiktive. Situasjon per 14. september 2026 kl. 09.00.").unwrap();
        assert!(result.contains("scenario clock to 2026-09-14"));
        assert!(result.contains("is 2026-09-15 (Tue)"));
        assert!(!reference("Ordrestatus per 14. september 2026")
            .unwrap()
            .contains("scenario clock"));
    }
}
