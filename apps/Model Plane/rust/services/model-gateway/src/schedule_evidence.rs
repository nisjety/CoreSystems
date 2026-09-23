//! Conditional scheduling for a small, explicitly stated dependency graph.
//! Unrecognized/ambiguous inputs produce no computed schedule. Source dates
//! remain possible approvals/targets; calculation never approves or books work.
use crate::source_validation::SourceContext;
use chrono::{Datelike, Duration, NaiveDate};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Span {
    source: usize,
    line: usize,
    start: usize,
    end: usize,
}
#[derive(Clone, Debug, Default)]
struct Task {
    name: String,
    owner: String,
    days: Option<u32>,
    dependencies: Vec<String>,
    anchor: Option<NaiveDate>,
    evidence: Vec<Span>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Dates {
    earliest_start: NaiveDate,
    earliest_finish: NaiveDate,
    work_dates: Vec<NaiveDate>,
}
#[derive(Debug)]
struct Graph {
    tasks: BTreeMap<String, Task>,
    absences: BTreeMap<String, BTreeSet<NaiveDate>>,
    policy: Vec<Span>,
    unestimated: Vec<String>,
}

fn normalized(value: &str) -> String {
    let value = value
        .trim()
        .trim_matches(['.', ':', '-'])
        .trim()
        .to_lowercase();
    if value.len() > 5 {
        value
            .strip_suffix("en")
            .or_else(|| value.strip_suffix("et"))
            .unwrap_or(&value)
            .to_owned()
    } else {
        value
    }
}
fn count(word: &str) -> Option<u32> {
    match word.to_lowercase().as_str() {
        "en" | "én" | "ett" | "one" => Some(1),
        "to" | "two" => Some(2),
        "tre" | "three" => Some(3),
        "fire" | "four" => Some(4),
        _ => word.parse().ok().filter(|n| (1..=60).contains(n)),
    }
}
fn month(word: &str) -> Option<u32> {
    match word.to_lowercase().as_str() {
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
fn workday(date: NaiveDate) -> bool {
    date.weekday().num_days_from_monday() < 5
}

fn graph(context: &SourceContext) -> Option<Graph> {
    static YEAR: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\b(?:19|20|21)\d{2}\b").unwrap());
    static ANCHOR: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)\b([\p{L}]+)\s+(?:kan godkjennes|may be approved on)\s+(\d{1,2})\.?\s+([\p{L}]+)",
        )
        .unwrap()
    });
    static DURATION: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)^([\p{L}][\p{L} -]{0,60}?)\s+(?:trenger|needs|requires)\s+(\d+|én|en|ett|to|tre|fire|one|two|three|four)\s+(?:(?:hel|hele|full)\s+)?(?:arbeidsdag(?:er)?|workdays?)(?:\s+(?:etter|after)\s+(?:(?:godkjent|ferdig|approved|completed)\s+)?([\p{L}][\p{L} -]{0,60}))?\.?$").unwrap()
    });
    static START: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)^(.+?)\s+(?:kan starte|can start)\s+([\p{L}]+)\s+(?:etter at|after)\s+(.+?)\s+(?:er klare|are complete)\.?$").unwrap()
    });
    static SEND: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)\b([\p{L}]+)\s+(?:først kan sendes når|may only be sent (?:when|after))\s+(.+?)\.?$").unwrap()
    });
    static ABSENT: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)^(.+?)\s+(?:er utilgjengelig|is unavailable)\s+(\d{1,2})\.?[–—-](\d{1,2})\.?\s+([\p{L}]+)").unwrap()
    });
    static SPLIT: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?i)\s+(?:og|and)\s+").unwrap());
    static READY: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)\s+(?:er godkjent|er klar|is approved|is ready)$").unwrap()
    });
    static UNKNOWN: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)(?:ikke (?:avsatt|satt av) tid til|no time (?:is )?allocated (?:to|for))\s+([\p{L}]+)").unwrap()
    });
    let mut years = BTreeSet::new();
    let mut lines = Vec::new();
    for source in &context.sources {
        let mut start = 0;
        for (line, text) in source.content.split_inclusive('\n').enumerate() {
            for year in YEAR.find_iter(text) {
                years.insert(year.as_str().parse::<i32>().ok()?);
            }
            // Source paragraphs can contain several statements. Preserve date
            // punctuation ("16. september") while splitting sentence endings.
            let mut clause_start = 0;
            for (index, ch) in text.char_indices() {
                if ch == '.'
                    && text[..index]
                        .chars()
                        .next_back()
                        .is_some_and(char::is_alphabetic)
                    && text[index + 1..]
                        .chars()
                        .next()
                        .is_none_or(char::is_whitespace)
                {
                    lines.push((
                        text[clause_start..index + 1]
                            .trim()
                            .trim_start_matches("- ")
                            .trim(),
                        Span {
                            source: source.id,
                            line,
                            start: start + clause_start,
                            end: start + index + 1,
                        },
                    ));
                    clause_start = index + 1;
                }
            }
            if !text[clause_start..].trim().is_empty() {
                lines.push((
                    text[clause_start..].trim().trim_start_matches("- ").trim(),
                    Span {
                        source: source.id,
                        line,
                        start: start + clause_start,
                        end: start + text.len(),
                    },
                ));
            }
            start += text.len();
        }
    }
    if years.len() != 1 {
        return None;
    }
    let year = *years.first()?;
    let mut result = Graph {
        tasks: BTreeMap::new(),
        absences: BTreeMap::new(),
        policy: vec![],
        unestimated: vec![],
    };
    let mut weekdays = false;
    let mut next_day = false;
    let mut aliases: Vec<(String, String)> = vec![];
    for (line, span) in lines {
        let lower = line.to_lowercase().replace(['–', '—'], "-");
        if lower.contains("arbeidsdager er mandag-fredag")
            || lower.contains("workdays are monday-friday")
        {
            weekdays = true;
            result.policy.push(span.clone());
        }
        if (lower.contains("dagens slutt") && lower.contains("neste arbeidsdag"))
            || (lower.contains("end of day") && lower.contains("next workday"))
        {
            next_day = true;
            result.policy.push(span.clone());
        }
        if let Some(c) = UNKNOWN.captures(line) {
            result.unestimated.push(normalized(&c[1]));
        }
        if let Some(c) = ABSENT.captures(line) {
            let start = NaiveDate::from_ymd_opt(year, month(&c[4])?, c[2].parse().ok()?)?;
            let end = NaiveDate::from_ymd_opt(year, month(&c[4])?, c[3].parse().ok()?)?;
            if end < start || (end - start).num_days() > 60 {
                return None;
            }
            let absent = result.absences.entry(normalized(&c[1])).or_default();
            for day in 0..=(end - start).num_days() {
                absent.insert(start + Duration::days(day));
            }
            result.policy.push(span.clone());
        }
        if let Some(c) = ANCHOR.captures(line) {
            let name = normalized(&c[1]);
            let date = NaiveDate::from_ymd_opt(year, month(&c[3])?, c[2].parse().ok()?)?;
            aliases.push((c[1].to_lowercase(), name.clone()));
            let task = result.tasks.entry(name.clone()).or_default();
            if task.anchor.is_some_and(|old| old != date) {
                return None;
            }
            task.name = name;
            task.anchor = Some(date);
            task.evidence.push(span.clone());
        }
        let (owner, body) = line
            .split_once(':')
            .map_or(("", line), |(owner, body)| (owner.trim(), body.trim()));
        if let Some(c) = DURATION.captures(body) {
            let name = normalized(&c[1]);
            let days = count(&c[2])?;
            aliases.push((c[1].to_lowercase(), name.clone()));
            let task = result.tasks.entry(name.clone()).or_default();
            if task.days.is_some_and(|old| old != days) {
                return None;
            }
            task.name = name;
            task.days = Some(days);
            if !owner.is_empty() {
                if !task.owner.is_empty() && task.owner != normalized(owner) {
                    return None;
                }
                task.owner = normalized(owner);
            }
            if let Some(dep) = c.get(3) {
                task.dependencies.push(normalized(dep.as_str()));
            }
            task.evidence.push(span.clone());
        }
        if let Some(c) = START.captures(line) {
            let name = normalized(&c[2]);
            aliases.push((c[2].to_lowercase(), name.clone()));
            let task = result.tasks.entry(name.clone()).or_default();
            task.name = name;
            if !task.owner.is_empty() && task.owner != normalized(&c[1]) {
                return None;
            }
            task.owner = normalized(&c[1]);
            task.dependencies.extend(SPLIT.split(&c[3]).map(normalized));
            task.evidence.push(span.clone());
        }
        if let Some(c) = SEND.captures(body) {
            let name = normalized(&c[1]);
            aliases.push((c[1].to_lowercase(), name.clone()));
            let task = result.tasks.entry(name.clone()).or_default();
            task.name = name;
            if task.days.is_some_and(|days| days != 0) {
                return None;
            }
            task.days = Some(0);
            task.dependencies.extend(
                SPLIT
                    .split(c[2].trim_end_matches('.'))
                    .map(|dep| normalized(&READY.replace(dep, ""))),
            );
            task.evidence.push(span.clone());
        }
    }
    if !weekdays || !next_day || result.tasks.len() < 2 || result.tasks.len() > 16 {
        return None;
    }
    let names: BTreeSet<_> = result.tasks.keys().cloned().collect();
    for task in result.tasks.values_mut() {
        for dep in &mut task.dependencies {
            if !names.contains(dep) {
                let matches: BTreeSet<_> = aliases
                    .iter()
                    .filter(|(raw, _)| {
                        raw.ends_with("en") && raw.strip_suffix('n') == Some(dep.as_str())
                    })
                    .map(|(_, name)| name.clone())
                    .collect();
                if matches.len() == 1 {
                    *dep = matches.first()?.clone();
                }
            }
        }
        task.dependencies.sort();
        task.dependencies.dedup();
    }
    Some(result)
}

fn solve(
    name: &str,
    graph: &Graph,
    seen: &mut BTreeSet<String>,
    solved: &mut BTreeMap<String, Dates>,
) -> Option<Dates> {
    if let Some(result) = solved.get(name) {
        return Some(result.clone());
    }
    if !seen.insert(name.into()) {
        return None;
    }
    let task = graph.tasks.get(name)?;
    let result = if let Some(date) = task.anchor {
        if !task.dependencies.is_empty() || task.days.is_some() || !workday(date) {
            return None;
        }
        Dates {
            earliest_start: date,
            earliest_finish: date,
            work_dates: vec![],
        }
    } else {
        let days = task.days?;
        let mut finish = None;
        for dep in &task.dependencies {
            let dependency = solve(dep, graph, seen, solved)?;
            finish = Some(
                finish.map_or(dependency.earliest_finish, |date: NaiveDate| {
                    date.max(dependency.earliest_finish)
                }),
            );
        }
        let mut date = finish?;
        let mut work = Vec::new();
        for _ in 0..366 {
            date = date.checked_add_signed(Duration::days(1))?;
            if workday(date)
                && !graph
                    .absences
                    .get(&task.owner)
                    .is_some_and(|absent| absent.contains(&date))
            {
                work.push(date);
                if work.len() >= days.max(1) as usize {
                    break;
                }
            }
        }
        if work.len() != days.max(1) as usize {
            return None;
        }
        Dates {
            earliest_start: work[0],
            earliest_finish: *work.last()?,
            work_dates: work,
        }
    };
    seen.remove(name);
    solved.insert(name.into(), result.clone());
    Some(result)
}

pub fn computed_schedule(context: &SourceContext) -> Option<Value> {
    let graph = graph(context)?;
    let mut dates = BTreeMap::new();
    for name in graph.tasks.keys() {
        solve(name, &graph, &mut BTreeSet::new(), &mut dates)?;
    }
    // Do not call independently computed dates feasible if an identified owner
    // is assigned overlapping work. Choosing a resource order needs user facts.
    let mut occupied = BTreeSet::new();
    for (name, task) in &graph.tasks {
        if !task.owner.is_empty() {
            for date in &dates[name].work_dates {
                if !occupied.insert((task.owner.clone(), *date)) {
                    return None;
                }
            }
        }
    }
    Some(
        json!({"checker":"explicit-workday-chain-v1","scope":"conditional_lower_bounds",
        "conditions":["Each possible approval occurs on its stated date; it is not already approved.","Every predecessor satisfies its required completion and approval criteria by its calculated finish. No extra approval wait is assumed; any wait moves dependent work later.","Minimum stated durations are achieved.","No blocking unestimated work is needed; unknown repair duration cannot be invented.","Only explicitly recognized dependencies and owner absences are computed; additional constraints require a new calculation."],
        "policyEvidence":graph.policy,"unestimatedTasks":graph.unestimated,
        "tasks":graph.tasks.iter().map(|(name,task)|json!({"id":name,"owner":task.owner,"workdays":task.days,"dependencies":task.dependencies,
            "possibleApproval":task.anchor,"dates":dates[name],"evidence":task.evidence})).collect::<Vec<_>>() }),
    )
}

/// Only explicit earliest-possible, no-repair claims and invented repair
/// minima are rejected here. Chosen later proposals remain legal.
pub fn errors(schedule: Option<&Value>, candidate: &str) -> Vec<String> {
    let Some(schedule) = schedule else {
        return vec![];
    };
    static EARLIEST: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)(?:tidligste mulige|earliest possible)[^\n]{0,100}?\b(\d{1,2})\.?\s+(januar|january|februar|february|mars|march|april|mai|may|juni|june|juli|july|august|september|oktober|october|november|desember|december)").unwrap()
    });
    let mut errors = Vec::new();
    let text = candidate.replace('*', "");
    for line in text.lines() {
        if line.trim_start().starts_with('>') {
            continue;
        }
        let lower = line.to_lowercase();
        for task in schedule["unestimatedTasks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let minimum = regex::Regex::new(&format!(
                r"(?i)\b{}(?:en|et)?\b[^.;!?\n]{{0,60}}?\b(?:tar|trenger|krever|takes|requires|needs)\s+(?:minst|at least)\s+(?:\d|en\b|én\b|ett\b|to\b|tre\b|one\b|two\b|three\b)",
                regex::escape(task)
            )).expect("escaped task name");
            if minimum.is_match(line)
                && !["hvis ", "dersom ", "if ", "forslag", "proposal", "hypotes"]
                    .iter()
                    .any(|word| lower.contains(word))
            {
                errors.push(format!("No duration is supplied for {task}. Do not assert a minimum repair duration/date range. Keep it unestimated; a suggested buffer is not an estimate."));
            }
        }
        for claim in EARLIEST.captures_iter(line) {
            if !lower.contains("ingen blokkerende feil")
                && !lower.contains("no blocking defects")
                && !lower.contains("no errors")
            {
                continue;
            }
            // A claim can use a later hypothetical approval. Only compare the
            // exact source calculation when this same statement explicitly
            // binds every possible approval to its source date. Otherwise the
            // supplied lower bounds inform semantic review, not a hard veto.
            let anchors: Vec<_> = schedule["tasks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|task| task["possibleApproval"].is_string())
                .collect();
            if anchors.is_empty()
                || !anchors.iter().all(|task| {
                    let Some(name) = task["id"].as_str() else {
                        return false;
                    };
                    let Some(date) = task["possibleApproval"]
                        .as_str()
                        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                    else {
                        return false;
                    };
                    let pattern = regex::Regex::new(&format!(
                        r"(?i)\b{}(?:et|en|n)?\b[^.!?\n]{{0,55}}?\b(\d{{1,2}})\.?\s+([\p{{L}}]+)",
                        regex::escape(name)
                    ))
                    .expect("escaped task name");
                    let matches = pattern.captures_iter(line).any(|capture| {
                        capture[1].parse::<u32>().ok() == Some(date.day())
                            && month(&capture[2]) == Some(date.month())
                    });
                    matches
                })
            {
                continue;
            }
            let claimed_month = month(&claim[2]);
            let claimed_day = claim[1].parse::<u32>().ok();
            for task in schedule["tasks"].as_array().into_iter().flatten() {
                let Some(name) = task["id"].as_str() else {
                    continue;
                };
                if task["workdays"] != 0 || !lower.contains(name.trim_end_matches('r')) {
                    continue;
                }
                let Some(date) = task["dates"]["earliestStart"]
                    .as_str()
                    .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                else {
                    continue;
                };
                if claimed_month.is_some()
                    && claimed_day.is_some()
                    && (claimed_month != Some(date.month()) || claimed_day != Some(date.day()))
                {
                    errors.push(format!("The explicit source chain permits {name} on {date} if its possible approval occurs and there are no blocking defects. Distinguish this conditional lower bound from a deliberately later proposal; do not invent extra dependencies or repair days."));
                }
            }
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_validation::Source;
    fn context(text: &str) -> SourceContext {
        SourceContext {
            sources: vec![Source {
                id: 0,
                name: "plan.md".into(),
                content: text.into(),
            }],
        }
    }
    const PLAN:&str="Plan 2026.\nArbeidsdager er mandag–fredag.\nGodkjenning ved dagens slutt gir start neste arbeidsdag.\nSkjemaet kan godkjennes 16. september.\nTeknisk ansvarlig: Eksporten trenger to hele arbeidsdager etter godkjent skjema.\nKundeserviceansvarlig: Veiledningen trenger én hel arbeidsdag etter ferdig eksport.\nKundeserviceansvarlig kan starte brukertest etter at eksporten og veiledningen er klare.\nBrukertest trenger to hele arbeidsdager.\nInvitasjoner først kan sendes når brukertest er godkjent og veiledningen er klar.\nTeknisk ansvarlig er utilgjengelig 21.–23. september.\nDet er ikke avsatt tid til feilretting.";
    #[test]
    fn source_chain_computes_conditional_dates_and_exact_spans() {
        let c = context(PLAN);
        let schedule = computed_schedule(&c).expect("complete explicit chain");
        let tasks = schedule["tasks"].as_array().unwrap();
        for (id, expected) in [
            ("eksport", "2026-09-18"),
            ("veiledning", "2026-09-21"),
            ("brukertest", "2026-09-23"),
            ("invitasjoner", "2026-09-24"),
        ] {
            assert_eq!(
                tasks.iter().find(|t| t["id"] == id).unwrap()["dates"]["earliestFinish"],
                expected
            );
        }
        for task in tasks {
            for span in task["evidence"].as_array().unwrap() {
                let start = span["start"].as_u64().unwrap() as usize;
                let end = span["end"].as_u64().unwrap() as usize;
                assert!(!c.sources[0].content[start..end].trim().is_empty());
            }
        }
        assert_eq!(schedule["scope"], "conditional_lower_bounds");
        assert!(!errors(Some(&schedule),"Hvis skjemaet godkjennes 16. september: Tidligste mulige avsendingsdato for invitasjoner er fredag 25. september (forutsatt ingen blokkerende feil i brukertest).").is_empty());
        assert!(!errors(Some(&schedule),"Hvis skjemaet godkjennes 16. september: Tidligste mulige avsendingsdato for invitasjoner er 24. oktober (forutsatt ingen blokkerende feil).").is_empty());
        assert!(errors(Some(&schedule),"Hvis skjemaet godkjennes 18. september: Tidligste mulige avsendingsdato for invitasjoner er 1. oktober (forutsatt ingen blokkerende feil).").is_empty(),"different hypothetical parameters remain semantic review");
        assert!(errors(
            Some(&schedule),
            "Forslag: send invitasjoner 25. september hvis alt er godkjent."
        )
        .is_empty());
        assert!(!errors(
            Some(&schedule),
            "Feilretting med påfølgende kontroll tar minst 24.–25. september."
        )
        .is_empty());
        assert!(errors(
            Some(&schedule),
            "Feilretting er ikke estimert. Forslag: sett av 24.–25. september som buffer."
        )
        .is_empty());
        assert!(errors(
            Some(&schedule),
            "Feilretting er ikke estimert; eksport tar minst to arbeidsdager."
        )
        .is_empty());
        assert!(errors(
            Some(&schedule),
            "Hvis feilretting tar minst to dager, må planen revurderes."
        )
        .is_empty());
    }
    #[test]
    fn changed_names_durations_absence_and_cycles_are_not_fixture_answers() {
        let moved = PLAN
            .replace("Skjemaet", "Avtalen")
            .replace("skjema", "avtale")
            .replace("16. september", "18. september")
            .replace("to hele arbeidsdager etter", "tre hele arbeidsdager etter");
        let schedule = computed_schedule(&context(&moved)).unwrap();
        assert_eq!(
            schedule["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["id"] == "eksport")
                .unwrap()["dates"]["workDates"],
            json!(["2026-09-24", "2026-09-25", "2026-09-28"])
        );
        assert!(computed_schedule(&context(
            &PLAN.replace("etter godkjent skjema", "etter ferdig brukertest")
        ))
        .is_none());
        assert!(computed_schedule(&context(
            &PLAN.replace("Arbeidsdager er mandag–fredag.", "")
        ))
        .is_none());
        assert!(computed_schedule(&context(&format!("{PLAN}\nOther plan 2027."))).is_none());
    }

    #[test]
    fn english_chain_handles_a_different_month_and_role_absence() {
        let input="Planning 2027. Workdays are Monday-Friday.\nApproval at end of day permits dependent work the next workday.\nDesign may be approved on 12 November.\nEngineering: Export needs two full workdays after approved design.\nSupport: Guide needs one full workday after completed export.\nSupport can start testing after export and guide are complete. Testing needs two full workdays.\nInvitations may only be sent after testing is approved and guide is ready.\nEngineering is unavailable 15-16 November.\nNo time allocated for repairs.";
        let schedule = computed_schedule(&context(input)).unwrap();
        let invitation = schedule["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "invitations")
            .unwrap();
        assert_eq!(invitation["dates"]["earliestFinish"], "2027-11-24");
        assert!(computed_schedule(&context(&format!(
            "{input}\nOther: Guide needs one full workday after completed export."
        )))
        .is_none());
    }
}
