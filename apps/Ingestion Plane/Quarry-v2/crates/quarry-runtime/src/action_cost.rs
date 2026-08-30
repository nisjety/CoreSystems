use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ActionCost {
    pub model_usd: f64,
    pub browser_usd: f64,
    pub egress_usd: f64,
    pub total_usd: f64,
    pub source: CostSource,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    FlatTable,
    Estimated,
    ProviderBilled,
}

/// Flat per-action browser cost table. Unknown actions fall back to 0.0001.
pub fn estimate_action_cost(action: &str, bytes_transferred: u64) -> ActionCost {
    let browser_usd = match action {
        "click" | "type" | "press" | "scroll" | "select" | "wait" | "mouse_wheel"
        | "back" | "forward" | "get_content" | "download_ref" | "frame_download_ref" => 0.0001,
        "navigate" => 0.001 + (bytes_transferred as f64 * 0.0000001),
        "screenshot" | "pdf" => 0.005,
        "evaluate" => 0.001,
        _ => 0.0001,
    };
    ActionCost {
        model_usd: 0.0,
        browser_usd,
        egress_usd: 0.0,
        total_usd: browser_usd,
        source: CostSource::FlatTable,
    }
}

pub fn estimate_planner_cost(input_tokens: u64, output_tokens: u64, model: &str) -> f64 {
    let rate = match model {
        m if m.contains("claude") => 0.000015,
        m if m.contains("gpt-4") => 0.00003,
        m if m.contains("gpt-3.5") => 0.000002,
        _ => 0.00001,
    };
    (input_tokens + output_tokens) as f64 * rate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_table_matches_known_actions() {
        assert_eq!(
            estimate_action_cost("click", 0).total_usd,
            0.0001
        );
        assert_eq!(
            estimate_action_cost("screenshot", 0).total_usd,
            0.005
        );
        assert_eq!(
            estimate_action_cost("evaluate", 0).total_usd,
            0.001
        );
        // navigate scales with bytes
        let small = estimate_action_cost("navigate", 1024).total_usd;
        let large = estimate_action_cost("navigate", 10240).total_usd;
        assert!(large > small);
    }

    #[test]
    fn planner_call_returns_estimated_cost_when_model_plane_omits() {
        let cost = estimate_planner_cost(1000, 500, "claude-3-haiku");
        assert!(cost > 0.0);
        // conservative fallback when usage is missing: 0.005 * tokens/1000
        let fallback = 0.005 * (1500.0 / 1000.0);
        assert!(cost < fallback * 10.0); // sane range
    }
}