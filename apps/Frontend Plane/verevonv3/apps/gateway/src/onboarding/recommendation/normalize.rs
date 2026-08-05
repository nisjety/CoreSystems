use serde_json::Value;

use crate::contracts::RecommendContext;

pub(crate) fn plan_id(value: &str) -> &'static str {
    match value {
        "hobby" => "hobby",
        "standard" => "standard",
        "pro" => "pro",
        "enterprise" => "enterprise",
        _ => "trial",
    }
}

pub(super) fn human_plan_name(value: &str, locale: &str) -> &'static str {
    match (locale, value) {
        ("nb", "hobby") => "Essential",
        ("nb", "standard") => "Advanced",
        ("nb", "pro") => "Expert",
        ("nb", "enterprise") => "Enterprise",
        ("nb", _) => "Gratis prøve",
        ("en", "hobby") => "Essential",
        ("en", "standard") => "Advanced",
        ("en", "pro") => "Expert",
        ("en", "enterprise") => "Enterprise",
        _ => "Trial",
    }
}

pub(super) fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn connector_source_count(context: &RecommendContext) -> u32 {
    context
        .connectors
        .as_ref()
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.source_count
                        .filter(|count| *count > 0)
                        .unwrap_or_else(|| {
                            let source_count = item
                                .sources
                                .iter()
                                .map(|source| source.trim())
                                .filter(|source| !source.is_empty())
                                .count() as u32;
                            source_count.max(1)
                        })
                })
                .sum()
        })
        .unwrap_or(0)
}

pub(super) fn total_source_count(context: &RecommendContext) -> u32 {
    let derived_count = connector_source_count(context)
        + context
            .websites
            .as_ref()
            .map(|items| items.len())
            .unwrap_or(0) as u32;
    context
        .source_count
        .filter(|source_count| *source_count >= derived_count)
        .unwrap_or(derived_count)
}

pub(super) fn align_source_proof_points(
    proof_points: Vec<String>,
    context: &RecommendContext,
    locale: &str,
) -> Vec<String> {
    let connected_count = connector_source_count(context);
    let source_point = (connected_count > 0).then(|| {
        if locale == "nb" {
            format!("{connected_count} tilkoblede kilder valgt.")
        } else {
            format!("{connected_count} connected sources selected.")
        }
    });

    source_point
        .into_iter()
        .chain(
            proof_points
                .into_iter()
                .filter(|point| !looks_like_numeric_source_point(point)),
        )
        .take(3)
        .collect()
}

fn looks_like_numeric_source_point(value: &str) -> bool {
    let lower = value.to_lowercase();
    lower.chars().any(|char| char.is_ascii_digit())
        && (lower.contains("kilde") || lower.contains("source"))
}
