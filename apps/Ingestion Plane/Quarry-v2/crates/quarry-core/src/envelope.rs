//! REST resource envelope. Mirrors CONTRACTS §2.

use serde::{Deserialize, Serialize};

use crate::contracts::BudgetMeta;
use crate::error::QuarryError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub data: Option<T>,
    pub meta: EnvelopeMeta,
    pub error: Option<QuarryError>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvelopeMeta {
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<PageMeta>,
    /// The agent run's budget, on a browser step response (WS6.B). Absent elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<BudgetMeta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    pub limit: u32,
}

impl<T> Envelope<T> {
    /// Attach `meta.budget` (WS6.B). `None` leaves the envelope as it was.
    pub fn with_budget(mut self, budget: Option<BudgetMeta>) -> Self {
        self.meta.budget = budget;
        self
    }

    pub fn ok(request_id: impl Into<String>, data: T) -> Self {
        Self {
            data: Some(data),
            meta: EnvelopeMeta {
                request_id: request_id.into(),
                page: None,
                budget: None,
            },
            error: None,
        }
    }

    pub fn err(request_id: impl Into<String>, err: QuarryError) -> Self {
        Self {
            data: None,
            meta: EnvelopeMeta {
                request_id: request_id.into(),
                page: None,
                budget: None,
            },
            error: Some(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_is_absent_unless_attached() {
        let plain = serde_json::to_value(Envelope::ok("req_1", 1u32)).unwrap();
        assert!(plain["meta"].get("budget").is_none(), "no budget, no key");

        let budget = BudgetMeta {
            class: "agent".into(),
            steps: 2,
            attempts: 3,
            max_steps: Some(60),
            max_attempts: Some(120),
            elapsed_s: 1.5,
            max_runtime_s: Some(900),
            spent_usd: 0.0021,
            max_cost_usd: Some(1.0),
            estimated: true,
            cost_source: "flat_table".into(),
            exhausted: None,
        };
        let attached = serde_json::to_value(
            Envelope::ok("req_2", 1u32).with_budget(Some(budget)),
        )
        .unwrap();
        assert_eq!(attached["meta"]["budget"]["class"], "agent");
        assert_eq!(attached["meta"]["budget"]["max_steps"], 60);
        assert_eq!(attached["meta"]["budget"]["estimated"], true);
        assert!(attached["meta"]["budget"].get("exhausted").is_none());
    }

    #[test]
    fn interactive_budget_omits_the_bounds() {
        let budget = BudgetMeta {
            class: "interactive".into(),
            steps: 13,
            attempts: 13,
            max_steps: None,
            max_attempts: None,
            elapsed_s: 0.0,
            max_runtime_s: None,
            spent_usd: 0.0,
            max_cost_usd: None,
            estimated: true,
            cost_source: "flat_table".into(),
            exhausted: None,
        };
        let value = serde_json::to_value(Envelope::ok("req_3", 1u32).with_budget(Some(budget))).unwrap();
        assert!(value["meta"]["budget"].get("max_steps").is_none(), "a person has no step bound");
        assert_eq!(value["meta"]["budget"]["steps"], 13);
    }
}
