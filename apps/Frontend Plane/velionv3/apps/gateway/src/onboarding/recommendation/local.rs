use chrono::Utc;

use crate::contracts::{PlanRecommendation, RecommendContext};

use super::{normalize::human_plan_name, plan_id};

pub(crate) fn build_local_recommendation(context: &RecommendContext) -> PlanRecommendation {
    let connector_count = context
        .connectors
        .as_ref()
        .map(|items| items.len())
        .unwrap_or(0);
    let source_count = context.source_count.unwrap_or(
        connector_count as u32
            + context
                .websites
                .as_ref()
                .map(|items| items.len())
                .unwrap_or(0) as u32,
    );
    let employees = context
        .organization
        .as_ref()
        .and_then(|org| org.employee_count)
        .unwrap_or(0);
    let brief = context
        .website
        .as_ref()
        .and_then(|website| website.agent_brief.clone())
        .unwrap_or_default()
        .to_lowercase();
    let locale = if context.locale.as_deref() == Some("nb") {
        "nb"
    } else {
        "en"
    };

    let plan = if employees >= 100
        || connector_count >= 4
        || brief.contains("compliance")
        || brief.contains("governance")
    {
        "enterprise"
    } else if employees >= 40
        || connector_count >= 3
        || source_count >= 4
        || brief.contains("multi")
        || brief.contains("sla")
    {
        "pro"
    } else if employees >= 11
        || connector_count >= 2
        || source_count >= 3
        || brief.contains("automation")
        || brief.contains("routing")
    {
        "standard"
    } else if source_count > 0 || !brief.is_empty() {
        "hobby"
    } else {
        "trial"
    };

    let reason = if locale == "nb" {
        match plan {
            "enterprise" => "Kompleksitet, volum eller governance-signaler peker mot Enterprise.",
            "pro" => "Flere kilder og høyere operasjonell kompleksitet peker mot Expert.",
            "standard" => "Behov for flere kilder og automasjon peker mot Advanced.",
            "hobby" => "Et mindre oppsett kan valideres billigst med Essential.",
            _ => "Lite signal ennå, så prøven er tryggest å starte med.",
        }
    } else {
        match plan {
            "enterprise" => "Complexity, volume, or governance signals point to Enterprise.",
            "pro" => "More sources and higher operating complexity point to Expert.",
            "standard" => "Multiple sources and automation needs point to Advanced.",
            "hobby" => "A smaller setup is cheapest to validate on Essential.",
            _ => "There is still limited signal, so the trial is the safest starting point.",
        }
    };

    let summary = if locale == "nb" {
        format!(
            "{} kilder og {} ansatte gir best start med {}.",
            source_count,
            employees,
            human_plan_name(plan, locale)
        )
    } else {
        format!(
            "{} sources and {} employees make {} the best starting point.",
            source_count,
            employees,
            human_plan_name(plan, locale)
        )
    };

    PlanRecommendation {
        plan_id: plan_id(plan),
        reason: reason.into(),
        summary,
        proof_points: vec![
            if locale == "nb" {
                format!("{} kilder er valgt i onboarding.", source_count)
            } else {
                format!("{} sources are selected in onboarding.", source_count)
            },
            if employees > 0 {
                if locale == "nb" {
                    format!("Brreg eller brukeren oppga {} ansatte.", employees)
                } else {
                    format!("Brreg or the operator supplied {} employees.", employees)
                }
            } else if locale == "nb" {
                "Teamstørrelse er fortsatt ukjent.".into()
            } else {
                "Team size is still unknown.".into()
            },
        ],
        scope_signals: vec![
            context
                .website
                .as_ref()
                .and_then(|website| website.url.clone())
                .unwrap_or_default(),
            context
                .connectors
                .as_ref()
                .map(|items| {
                    items
                        .iter()
                        .map(|item| item.label.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default(),
        ]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect(),
        opportunities: if locale == "nb" {
            vec![
                "Start med de vanligste spørsmålene fra nettsted og dokumentkilder.".into(),
                "Bruk agenten først på godkjenningsvennlige arbeidsflyter.".into(),
            ]
        } else {
            vec![
                "Start with the highest-volume questions from the website and document sources."
                    .into(),
                "Use the agent first on approval-friendly workflows.".into(),
            ]
        },
        generated_at: Utc::now().to_rfc3339(),
        source: "local",
    }
}
