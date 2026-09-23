//! Local evidence derived from supported attachment layouts. These helpers do
//! not infer business approval, causality or facts missing from the sources.
use crate::source_validation::SourceContext;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Shared by authoring, private repairs and review so a correction does not
/// lose the scope constraints that applied to the original source evidence.
pub const SCOPE_RULES: &str = "Scope checks: an observed_sample supports proportions only within the described sample, not prevalence in the wider population. A candidate must retain that sample restriction; a source citation alone does not restrict an otherwise general claim. More than half is a majority of that sample, including verbal counts. role_unavailability supports a possible delay within that role's documented work, not that an obstacle exists, the work is unfinished or completion is impossible. The stated role itself establishes its ordinary work domain: a technical owner can support a qualified risk involving technical work without a separately named task. Do not expand that risk to unrelated responsibilities. In a qualified risk, 'obstacles may delay' describes a contingency, not a present obstacle. A grounded operational risk is different from a product benefit: even 'may/could' improve a business or health outcome asserts an undocumented product effect unless the source establishes that effect. Product controls, dimensions and materials alone do not establish such effects. undocumented_evidence means the claimed effect/experience lacks support; it neither establishes the effect as possible nor proves it impossible. A suggestion to investigate an effect is allowed; asserting that a feature may produce it is still a benefit claim. explicit_unknown_status supports neither completed nor incomplete. Preserve business transaction types: a quotation/offer is not evidence of a tender, order, sale or invoice; a paraphrase must not add a procurement process or transaction state. These are claim-scope checks, not a ban on ordinary equivalent wording.";

pub(crate) fn cents(value: &str) -> Option<i128> {
    let value = value.trim();
    let negative = value.starts_with('-');
    let value = value.strip_prefix('-').unwrap_or(value);
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty() || !whole.chars().all(|c| c.is_ascii_digit())
        || fraction.len() > 2 || !fraction.chars().all(|c| c.is_ascii_digit()) { return None; }
    let whole: i128 = whole.parse().ok()?;
    if whole > 1_000_000_000_000 { return None; }
    let fraction = if fraction.is_empty() { 0 } else {
        fraction.parse::<i128>().ok()? * if fraction.len() == 1 { 10 } else { 1 }
    };
    Some((whole * 100 + fraction) * if negative { -1 } else { 1 })
}

pub(crate) fn money(value: i128) -> String {
    format!("{}{}.{:02}", if value < 0 { "-" } else { "" }, value.abs() / 100, value.abs() % 100)
}

/// Deliberately limited to unquoted comma-separated records and recognized
/// revenue/cost headers. Ambiguous or malformed input supplies no ledger; it
/// never silently drops a bad row or averages per-row margins.
pub fn computed_csv(context: &SourceContext) -> Vec<Value> {
    context.sources.iter().filter_map(|source| {
        if !source.name.to_lowercase().ends_with(".csv") || source.content.contains('"') { return None; }
        let mut lines = source.content.lines();
        let header: Vec<_> = lines.next()?.trim_start_matches('\u{feff}').split(',').map(str::trim).collect();
        let revenue = header.iter().position(|h| matches!(*h, "omsetning_nok" | "revenue" | "revenue_nok"))?;
        let cost = header.iter().position(|h| matches!(*h, "varekost_nok" | "cost" | "cost_nok"))?;
        if header.iter().filter(|h| matches!(**h, "omsetning_nok" | "revenue" | "revenue_nok")).count() != 1
            || header.iter().filter(|h| matches!(**h, "varekost_nok" | "cost" | "cost_nok")).count() != 1 { return None; }
        let dimensions: Vec<_> = header.iter().enumerate().filter(|(_, h)|
            matches!(**h, "uke" | "week" | "produktgruppe" | "product_group" | "kanal" | "channel")).map(|(i, _)| i).collect();
        if dimensions.len() > 3 { return None; }
        let mut groups: BTreeMap<Vec<(String, String)>, (i128, i128, Vec<usize>)> = BTreeMap::new();
        let mut rows = 0;
        for (index, line) in lines.enumerate() {
            if line.trim().is_empty() { return None; }
            rows += 1;
            if rows > 2000 { return None; }
            let cells: Vec<_> = line.split(',').map(str::trim).collect();
            if cells.len() != header.len() { return None; }
            let (revenue, cost) = (cents(cells[revenue])?, cents(cells[cost])?);
            for mask in 0..(1 << dimensions.len()) {
                let key: Vec<_> = dimensions.iter().enumerate().filter(|(bit, _)| mask & (1 << bit) != 0)
                    .map(|(_, col)| (header[*col].to_owned(), cells[*col].to_owned())).collect();
                let total = groups.entry(key).or_default();
                total.0 += revenue; total.1 += cost; total.2.push(index + 1);
            }
            if groups.len() > 128 { return None; }
        }
        if rows == 0 { return None; }
        let comparisons = crate::numeric_evidence::comparisons(&groups);
        Some(json!({"source":source.id,"sourceHash":crate::result_validation::content_hash(&source.content),
            "checker":"csv-revenue-cost-v2","formula":"grossProfit = sum(revenue) - sum(cost); weightedMarginPercent = 100 * grossProfit / sum(revenue). Comparisons bind the same dimensions in two labelled weeks: change = after - before; relativeChangePercent = 100 * change / before. Margin differences are percentage points. Amounts use exact decimal cents; percentages round to four decimals and are undefined for zero denominators. No causal conclusions.",
            "comparisons":comparisons,"groups":groups.into_iter().map(|(dimensions,(revenue,cost,lines))| json!({
                "dimensions":dimensions.into_iter().collect::<BTreeMap<_,_>>(),"sourceLines":lines,
                "revenue":money(revenue),"cost":money(cost),"grossProfit":money(revenue-cost),
                "weightedMarginPercent":(revenue != 0).then(|| format!("{:.4}",100.0*(revenue-cost) as f64/revenue as f64))
            })).collect::<Vec<_>>() }))
    }).collect()
}

/// Attach logical roles to explicit source statements. These are not extracted
/// world facts: a prerequisite line alone says nothing about current completion,
/// and a lack of confirmation says nothing about whether an event occurred.
/// Exact source spans remain available to check other statements and precedence.
pub fn source_constraints(context: &SourceContext) -> Vec<Value> {
    static PREREQUISITE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(||
        regex::Regex::new(r"(?i)\b(?:må være|must be)\s+(?:ferdig|fullført|godkjent|complete|completed|approved|signed)\s+(?:før|before)\b|\b(?:is required|kreves)\s+(?:before|før)\b").unwrap());
    static UNCONFIRMED: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(||
        regex::Regex::new(r"(?i)\b(?:ikke|not)\s+(?:bekreftet|confirmed)\b|\bingen\b[^.!?\n]{0,60}\bbekreftet\b").unwrap());
    static UNAVAILABLE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(||
        regex::Regex::new(r"(?i)\b(?:ingen|no)\b[^.!?\n]{0,100}\b(?:kundeuttalelser|customer (?:quotes|testimonials)|attribusjonsdata|attribution data)\b").unwrap());
    static ABSENCE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(||
        regex::Regex::new(r"(?i)\b(?:is|are|er)\s+(?:unavailable|utilgjengelig)\b").unwrap());
    static SAMPLE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(||
        regex::Regex::new(r"(?i)\b(?:we|vi)\s+(?:interviewed|surveyed|intervjuet|undersøkte)\s+(?:\d+|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|en|ett|to|tre|fire|fem|seks|sju|syv|åtte|ni|ti|elleve|tolv)\s+\p{L}").unwrap());
    static UNKNOWN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(||
        regex::Regex::new(r"(?i)\b(?:does not (?:state|establish)|sier ikke|dokumenterer ikke)\s+(?:whether|om)\b").unwrap());
    static UNDOCUMENTED: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(||
        regex::Regex::new(r"(?i)\b(?:no|ingen)\s+[^.!?;\n]{1,100}\s+(?:are documented|er dokumentert)\b").unwrap());
    let mut constraints = Vec::new();
    for source in &context.sources {
        let mut line_start = 0;
        for (line, content) in source.content.split_inclusive('\n').enumerate() {
            let mut clause_start = line_start;
            for clause in content.split_inclusive(['.', '!', '?', ';', '\n']) {
                // A clause can contain more than one limitation. Keeping only
                // the first hides uncertainty behind a prerequisite statement.
                for (pattern, relation, not_established) in [
                    (&*PREREQUISITE, "prerequisite", "current_completion"),
                    (&*UNCONFIRMED, "unconfirmed", "non_occurrence"),
                    (&*UNAVAILABLE, "unavailable_evidence", "experience_or_causality"),
                    (&*ABSENCE, "role_unavailability", "unfinished_work_or_impossibility_or_unrelated_responsibility"),
                    (&*SAMPLE, "observed_sample", "wider_population_prevalence"),
                    (&*UNKNOWN, "explicit_unknown_status", "either_current_status"),
                    (&*UNDOCUMENTED, "undocumented_evidence", "asserted_effect_or_experience"),
                ] {
                    if !pattern.is_match(clause) { continue; }
                    constraints.push(json!({"source":source.id,"line":line,"start":clause_start,"end":clause_start+clause.len(),
                        "text":clause,"relation":relation,"statementAloneDoesNotEstablish":not_established}));
                    // Advisory hints only; the reviewer still receives all
                    // allowed source text, including unrecognized statements.
                    if constraints.len() == 128 { return constraints; }
                }
                clause_start += clause.len();
            }
            line_start += content.len();
        }
    }
    constraints
}

/// An explicit approved-date label needs an actual approval statement, not
/// merely a proposed/possible date somewhere in the source. This narrow guard
/// supplements semantic review; it does not parse general project scheduling.
pub fn unsupported_approved_dates(context: &SourceContext, text: &str) -> Vec<String> {
    static DATE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(||
        regex::Regex::new(r"(?i)\b([0-3]?\d)\.\s*(jan(?:uar)?|feb(?:ruar)?|mar(?:s)?|apr(?:il)?|mai|jun(?:i)?|jul(?:i)?|aug(?:ust)?|sep(?:tember)?|okt(?:ober)?|nov(?:ember)?|des(?:ember)?)\b").unwrap());
    static APPROVAL_TAG: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(||
        regex::Regex::new(r"(?i)(?:←|→|—|–|\s-\s|:)\s*\*{0,2}(?:vedtatt|godkjent|approved)(?:\s+(?:dato|mål|date|target))?\*{0,2}[.!]?\s*$").unwrap());
    static APPROVAL_PREFIX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(||
        regex::Regex::new(r"(?i)\b(?:vedtatt|godkjent|approved)\s+(?:(?:dato|mål|date|target)\s+)?[0-3]?\d\.\s*(?:jan|feb|mar|apr|mai|jun|jul|aug|sep|okt|nov|des)").unwrap());
    static AFFIRMATIVE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(||
        regex::Regex::new(r"(?i)\b(?:er|ble|har)\s+(?:godkjent|vedtatt)\b|\b(?:is|was)\s+approved\b").unwrap());
    let affirmative = |s: &str| AFFIRMATIVE.is_match(s);
    let qualified = |s: &str| ["ikke", "not ", "hvis ", "dersom ", "når ", "if ", "when ", "kan ", "may ", "mulig", "foresl", "forslag", "foruts"]
        .iter().any(|term| s.contains(term));
    let dates = |s: &str| DATE.captures_iter(s).map(|c| (c[1].parse::<u8>().unwrap_or(0), c[2].to_lowercase().chars().take(3).collect::<String>())).collect::<Vec<_>>();
    let mut errors = Vec::new();
    for line in text.lines() {
        let lower = line.to_lowercase();
        let approved_cell = lower.split('|').any(|cell| matches!(cell.trim().trim_matches('*').trim(), "vedtatt" | "godkjent" | "approved"));
        let approved_label = lower.contains("[vedtatt]") && !lower.contains("ikke [vedtatt]");
        let approved_tag = APPROVAL_TAG.is_match(&lower);
        let approved_prefix = lower.split('|').any(|cell| APPROVAL_PREFIX.is_match(cell) && !qualified(cell));
        if !approved_cell && !approved_label && !approved_tag && !approved_prefix && !(affirmative(&lower) && !qualified(&lower)) { continue; }
        for (day, month) in dates(&lower) {
            let explicit = context.sources.iter().any(|source| source.content.lines().any(|line| {
                let line = line.to_lowercase();
                ((affirmative(&line) || APPROVAL_PREFIX.is_match(&line)) && !qualified(&line))
                    && dates(&line).contains(&(day, month.clone()))
            }));
            if !explicit {
                errors.push(format!("Approval on {day}. {month} is asserted without an explicit source approval. Preserve the source's possible, proposed or target status; a stated date is not approval."));
            }
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_validation::Source;
    fn context(name: &str, content: &str) -> SourceContext {
        SourceContext { sources: vec![Source { id:0,name:name.into(),content:content.into() }] }
    }
    #[test]
    fn source_roles_bind_to_exact_spans_without_inventing_current_status() {
        let c=context("evidence.md", "Åpning. Teknisk avklaring må være ferdig før oppstart. Teknisk avklaring er ferdig.\nIngen kolli er bekreftet hentet. Ingen kundeuttalelser eller tester er tilgjengelige.\nSafety review must be complete before release.");
        let constraints=source_constraints(&c);
        assert_eq!(constraints.len(),4);
        assert_eq!(constraints[0]["relation"],"prerequisite");
        assert_eq!(constraints[1]["relation"],"unconfirmed");
        assert_eq!(constraints[2]["relation"],"unavailable_evidence");
        assert_eq!(constraints[3]["statementAloneDoesNotEstablish"],"current_completion");
        for row in constraints {
            let start=row["start"].as_u64().unwrap() as usize;
            let end=row["end"].as_u64().unwrap() as usize;
            assert_eq!(&c.sources[0].content[start..end],row["text"].as_str().unwrap());
            assert!(row.get("currentStatus").is_none(), "the following source sentence can establish completion");
        }
        let many=context("many.md",&"Pickup is not confirmed.\n".repeat(500));
        assert_eq!(source_constraints(&many).len(),128);
    }
    #[test]
    fn scope_roles_preserve_multiple_limits_and_exact_unicode_spans() {
        let c = context("scope.md", "Årsak ukjent. Review must be complete before launch, but launch is not confirmed.\nThe owner is unavailable on Monday. The source does not state whether review is complete.\nWe interviewed eleven warehouse managers. Eight requested handles.\nVi intervjuet åtte ledere. Seks ønsket lys.");
        let rows = source_constraints(&c);
        assert_eq!(rows.len(), 6);
        let relations: Vec<_> = rows.iter().map(|r| r["relation"].as_str().unwrap()).collect();
        assert_eq!(relations, ["prerequisite", "unconfirmed", "role_unavailability", "explicit_unknown_status", "observed_sample", "observed_sample"]);
        for row in &rows {
            let start = row["start"].as_u64().unwrap() as usize;
            let end = row["end"].as_u64().unwrap() as usize;
            assert_eq!(&c.sources[0].content[start..end], row["text"].as_str().unwrap());
            assert!(row.get("currentStatus").is_none());
        }
        for text in ["We did not interview six managers.", "No interviews were recorded.", "The owner is not unavailable.", "We may survey twelve people."] {
            assert!(source_constraints(&context("negative.md", text)).is_empty());
        }
    }
    #[test]
    fn undocumented_effects_are_source_bound_limits_not_effect_verdicts() {
        let c = context("controls.txt", "Åpning. No productivity or health effects are documented.\nIngen ergonomiske effekter er dokumentert.");
        let rows = source_constraints(&c);
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_eq!(row["relation"], "undocumented_evidence");
            let start = row["start"].as_u64().unwrap() as usize;
            let end = row["end"].as_u64().unwrap() as usize;
            assert_eq!(&c.sources[0].content[start..end], row["text"].as_str().unwrap());
        }
        for text in ["Productivity effects are documented.", "No effects are claimed here.", "No new tests were requested."] {
            assert!(source_constraints(&context("positive.txt", text)).is_empty());
        }
    }
    #[test]
    fn exact_aggregation_uses_weighted_margin_and_preserves_provenance() {
        let c=context("sales.csv","week,product_group,revenue,cost\n4,A,0.10,0.01\n4,A,0.20,0.08\n4,B,9.70,8.91\n");
        let ledger=computed_csv(&c);let total=&ledger[0]["groups"][0];
        assert_eq!(total["revenue"],"10.00");assert_eq!(total["cost"],"9.00");
        assert_eq!(total["grossProfit"],"1.00");assert_eq!(total["weightedMarginPercent"],"10.0000");
        assert_eq!(total["sourceLines"],json!([1,2,3]));
        assert_eq!(ledger[0]["sourceHash"],crate::result_validation::content_hash(&c.sources[0].content));
        assert_eq!(cents("-1.2"),Some(-120));assert_eq!(cents("1e6"),None);
    }
    #[test]
    fn malformed_or_ambiguous_csv_never_yields_partial_totals() {
        for content in ["revenue,cost\n1,0\nbad,1", "revenue,cost\n1,0,extra", "revenue,cost\n\"1\",0", "revenue,revenue,cost\n1,1,0"] {
            assert!(computed_csv(&context("x.csv",content)).is_empty());
        }
        assert!(computed_csv(&context("x.txt","revenue,cost\n1,0")).is_empty());
        assert_eq!(computed_csv(&context("x.csv","revenue,cost\n0,1"))[0]["groups"][0]["weightedMarginPercent"],Value::Null);
    }
    #[test]
    fn possible_date_and_acceptance_criteria_do_not_establish_approval() {
        let c=context("plan.md","Skjemaet kan godkjennes 12. oktober.\nNår skjemaet er godkjent 12. oktober, kan arbeidet starte.");
        assert!(!unsupported_approved_dates(&c,"| Skjema | 12. okt | [vedtatt] |").is_empty());
        assert!(!unsupported_approved_dates(&c,"| Skjema | 12. okt | Vedtatt |").is_empty());
        assert!(!unsupported_approved_dates(&c,"Skjemaet er godkjent 12. oktober.").is_empty());
        assert!(!unsupported_approved_dates(&c,"Man 12. okt [T1] Skjema godkjent ← vedtatt dato").is_empty());
        assert!(!unsupported_approved_dates(&c,"12. okt (mandag) — **vedtatt mål**").is_empty());
        assert!(unsupported_approved_dates(&c,"12. okt ← ikke vedtatt dato").is_empty());
        assert!(unsupported_approved_dates(&c,"12. okt ← foreslått dato").is_empty());
        assert!(unsupported_approved_dates(&c,"Skjemaet kan godkjennes 12. oktober; datoen er ikke vedtatt.").is_empty());
        assert!(unsupported_approved_dates(&c,"12. oktober er ikke [vedtatt].").is_empty());
        // "krever godkjent" ends with the letters "er godkjent" but is a
        // prerequisite, not an assertion that approval happened on either date.
        assert!(unsupported_approved_dates(&c,"- **Konflikt – invitasjon 22. september:** Datoen er et forslag, men prosjektleder krever godkjent brukertest før utsendelse. Ved tidligste kjede avsluttes testen 23. september.").is_empty());
        assert!(!unsupported_approved_dates(&c,"Skjemaet er godkjent 22. september.").is_empty());
        let review=context("plan.md","Driftsansvarlig tar intern gjennomgang 28. september.");
        assert!(!unsupported_approved_dates(&review,"Intern gjennomgang – vedtatt dato 28. september").is_empty());
        assert!(!unsupported_approved_dates(&review,"| Intern gjennomgang – vedtatt dato 28. september | Driftsansvarlig | ikke oppgitt |").is_empty());
        assert!(unsupported_approved_dates(&review,"Intern gjennomgang – oppgitt dato 28. september").is_empty());
        let approved=context("plan.md","Skjemaet ble godkjent 12. oktober.");
        assert!(unsupported_approved_dates(&approved,"| Skjema | 12. okt | [vedtatt] |").is_empty());
        let labeled=context("plan.md","Intern gjennomgang: vedtatt dato 28. september.");
        assert!(unsupported_approved_dates(&labeled,"Intern gjennomgang – vedtatt dato 28. september").is_empty());
    }
}
