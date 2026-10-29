use serde::{Deserialize, Serialize};
use std::fmt;

/// Origin of a record / sync run. Single source of truth for routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    ErpRest,
    DatamartGraphql,
    ExpenseApi,
}

impl Source {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Source::ErpRest => "erp_rest",
            Source::DatamartGraphql => "datamart_graphql",
            Source::ExpenseApi => "expense_api",
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_matches_display() {
        for src in [Source::ErpRest, Source::DatamartGraphql, Source::ExpenseApi] {
            assert_eq!(src.as_str(), src.to_string());
        }
    }

    #[test]
    fn serializes_as_snake_case() {
        let v = serde_json::to_value(Source::DatamartGraphql).unwrap();
        assert_eq!(v, serde_json::json!("datamart_graphql"));
    }

    #[test]
    fn deserializes_snake_case() {
        let s: Source = serde_json::from_str("\"erp_rest\"").unwrap();
        assert_eq!(s, Source::ErpRest);
    }
}
