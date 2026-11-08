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
        | "back" | "forward" | "reload" | "get_content" | "download_ref" | "frame_download_ref" => 0.0001,
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

/// The rate a navigation refused by policy or by egress is charged, in USD, in place of the
/// flat navigate cost (W2-U21a). The refusal ends the action before the page is used, so it is
/// charged what the cheapest action in the table costs: the base rate, which `wait` and every
/// action the table does not name are charged. A navigation refused before its page loaded and
/// one whose landed page was refused after it loaded are both charged this rate; the byte term
/// of the flat navigate cost is not taken.
pub const REFUSAL_RATE_USD: f64 = 0.0001;

/// The cost of an action that policy or the egress boundary refused (W2-U21a). A navigation is
/// charged [`REFUSAL_RATE_USD`]. Every other action keeps its flat-table cost: back, forward and
/// reload already sit at the base rate, and a refused click or screenshot is charged what it was
/// always charged. A cost the driver metered is not priced here; callers keep it.
pub fn estimate_refused_action_cost(action: &str) -> ActionCost {
    if action == "navigate" {
        return ActionCost {
            model_usd: 0.0,
            browser_usd: REFUSAL_RATE_USD,
            egress_usd: 0.0,
            total_usd: REFUSAL_RATE_USD,
            source: CostSource::FlatTable,
        };
    }
    estimate_action_cost(action, 0)
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

    #[test]
    fn refusal_rate_is_the_base_rate_and_below_the_flat_navigate_cost() {
        assert_eq!(REFUSAL_RATE_USD, estimate_action_cost("wait", 0).total_usd);
        assert_eq!(
            REFUSAL_RATE_USD,
            estimate_action_cost("not_in_the_table", 0).total_usd
        );
        assert!(REFUSAL_RATE_USD < estimate_action_cost("navigate", 0).total_usd);
    }

    #[test]
    fn a_refused_navigation_is_charged_the_refusal_rate_whatever_the_bytes() {
        let refused = estimate_refused_action_cost("navigate");
        assert_eq!(refused.total_usd, REFUSAL_RATE_USD);
        assert_eq!(refused.browser_usd, REFUSAL_RATE_USD);
        assert_eq!(refused.model_usd, 0.0);
        assert_eq!(refused.egress_usd, 0.0);
        assert_eq!(refused.source, CostSource::FlatTable);
        // The flat navigate cost grows with the bytes; the refused one takes no byte term.
        assert!(estimate_action_cost("navigate", 1_000_000).total_usd > refused.total_usd);
        // A refusal fails closed: it is still charged something.
        assert!(refused.total_usd > 0.0);
    }

    #[test]
    fn other_refused_actions_keep_their_flat_table_cost() {
        for action in [
            "click",
            "type",
            "press",
            "scroll",
            "wait",
            "back",
            "forward",
            "reload",
            "screenshot",
            "evaluate",
        ] {
            assert_eq!(
                estimate_refused_action_cost(action),
                estimate_action_cost(action, 0),
                "{action}"
            );
        }
    }
}