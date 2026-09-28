//! Opt-in replay of private fictional source/candidate regressions. No provider,
//! authentication or network is used; ordinary unit tests remain self-contained.
use model_gateway::{
    numeric_evidence, schedule_evidence, source_facts,
    source_validation::{Source, SourceContext},
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    id: String,
    sources: Vec<InputSource>,
    candidate: String,
    expected_rejected: bool,
    #[serde(default)]
    expected_schedule: std::collections::BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputSource {
    name: String,
    content: String,
}

#[test]
#[ignore = "set VEREVON_COMPUTED_CASES and VEREVON_COMPUTED_REPORT to private fictional replay files"]
fn saved_evidence_regressions() -> anyhow::Result<()> {
    let cases: Vec<Case> =
        serde_json::from_slice(&std::fs::read(std::env::var("VEREVON_COMPUTED_CASES")?)?)?;
    let mut reports = Vec::new();
    for case in cases {
        let context = SourceContext {
            sources: case
                .sources
                .into_iter()
                .enumerate()
                .map(|(id, source)| Source {
                    id,
                    name: source.name,
                    content: source.content,
                })
                .collect(),
        };
        let csv = source_facts::computed_csv(&context);
        let schedule = schedule_evidence::computed_schedule(&context);
        let mut errors = numeric_evidence::errors(&csv, &case.candidate);
        errors.extend(schedule_evidence::errors(
            schedule.as_ref(),
            &case.candidate,
        ));
        let dates_match = case.expected_schedule.iter().all(|(name, expected)| {
            schedule
                .as_ref()
                .and_then(|s| s["tasks"].as_array())
                .and_then(|tasks| tasks.iter().find(|task| task["id"] == name.as_str()))
                .is_some_and(|task| task["dates"]["earliestFinish"] == expected.as_str())
        });
        let correct = (!errors.is_empty()) == case.expected_rejected && dates_match;
        println!(
            "{}: correct={correct}, rejected={}, schedule={}",
            case.id,
            !errors.is_empty(),
            schedule.is_some()
        );
        reports.push(json!({"id":case.id,"correct":correct,"rejected":!errors.is_empty(),"errors":errors,"computedCsv":csv,"computedSchedule":schedule}));
    }
    std::fs::write(
        std::env::var("VEREVON_COMPUTED_REPORT")?,
        serde_json::to_vec_pretty(&reports)?,
    )?;
    anyhow::ensure!(
        reports.iter().all(|r| r["correct"] == Value::Bool(true)),
        "computed evidence regression failed; inspect private report"
    );
    Ok(())
}
