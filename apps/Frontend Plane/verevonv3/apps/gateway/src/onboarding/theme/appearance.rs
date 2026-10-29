use serde_json::{json, Value};

pub(super) fn appearance_update_body(mode: &str, color: &str, current_body: &Value) -> Value {
    let theme = current_body
        .get("theme")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("light");
    let font_size = current_body
        .get("fontSize")
        .cloned()
        .unwrap_or_else(|| Value::String("medium".into()));
    let compact_mode = current_body
        .get("compactMode")
        .cloned()
        .unwrap_or(Value::Bool(false));

    json!({
        "theme": if theme == "system" { "auto" } else { theme },
        "colorScheme": if mode == "brand" { color } else { "blue" },
        "fontSize": font_size,
        "compactMode": compact_mode,
    })
}
