use serde_json::Value;

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
