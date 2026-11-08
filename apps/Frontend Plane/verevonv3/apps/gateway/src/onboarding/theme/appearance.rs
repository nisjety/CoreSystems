use serde_json::{json, Value};

use crate::domains::navbar::carried_transcript_density;

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

    let mut body = json!({
        "theme": if theme == "system" { "auto" } else { theme },
        "colorScheme": if mode == "brand" { color } else { "blue" },
        "fontSize": font_size,
        "compactMode": compact_mode,
    });
    // The colour save names no transcript level, so the level the person chose is carried over (EP-14 S2).
    if let Some(density) = carried_transcript_density(current_body) {
        body["transcriptDensity"] = json!(density);
    }
    body
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::appearance_update_body;

    // A colour save keeps the transcript level the person chose. A save that finds no known level adds
    // no key, so a client that never chose one sends the same body as before (EP-14 S2).
    #[test]
    fn onboarding_appearance_keeps_transcript_density() {
        let kept = appearance_update_body(
            "brand",
            "#112233",
            &json!({ "theme": "dark", "compactMode": true, "transcriptDensity": "minimal" }),
        );
        assert_eq!(kept["transcriptDensity"], "minimal");
        assert_eq!(kept["colorScheme"], "#112233");
        assert_eq!(kept["compactMode"], true);

        let huge = json!({ "transcriptDensity": "huge" });
        let unknown = appearance_update_body("verevon", "#112233", &huge);
        assert!(unknown.get("transcriptDensity").is_none());

        let absent = appearance_update_body("verevon", "#112233", &json!({ "theme": "dark" }));
        assert!(absent.get("transcriptDensity").is_none());
        assert_eq!(absent["theme"], "dark");
        assert_eq!(absent["colorScheme"], "blue");
    }
}
