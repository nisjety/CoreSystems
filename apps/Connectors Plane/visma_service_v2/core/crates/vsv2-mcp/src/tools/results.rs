use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::results::{read, ResultStore};
use crate::tool::{parse_args, Tool, ToolDef, ToolError, ToolKind, RESULT_TOOL};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 500;

/// Pages through a result that was too large to return in one piece.
pub struct ReadResult(pub Arc<ResultStore>);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    result_id: String,
    /// JSON pointer into the result (RFC 6901), e.g. `/rows`. Empty or omitted = the whole value.
    #[serde(default)]
    pointer: Option<String>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[async_trait]
impl Tool for ReadResult {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: RESULT_TOOL,
            description: "Read part of a large result that another tool parked instead of returning \
                (it answered with `result_truncated: true` and a `result_id`). Give a JSON pointer \
                (e.g. /rows), and for arrays an offset/limit; follow `next_offset` for the next page. \
                Handles live in memory for a few minutes only. Read-only."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "result_id": {"type": "string"},
                    "pointer": {"type": "string", "description": "JSON pointer, e.g. /rows or /rows/0"},
                    "offset": {"type": "integer", "minimum": 0},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_LIMIT}
                },
                "required": ["result_id"],
                "additionalProperties": false
            }),
            kind: ToolKind::Read,
        }
    }

    async fn call(&self, args: Value) -> Result<Value, ToolError> {
        let a: Args = parse_args(args)?;
        let value = self.0.get(&a.result_id).ok_or_else(|| {
            ToolError::Failed("unknown or expired result_id; repeat the original request (handles last only a few minutes)".into())
        })?;
        let pointer = a.pointer.unwrap_or_default();
        if !pointer.is_empty() && !pointer.starts_with('/') {
            return Err(ToolError::InvalidArgs(
                "pointer must be empty or start with `/` (RFC 6901)".into(),
            ));
        }
        let limit = a.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let mut page =
            read(&value, &pointer, a.offset.unwrap_or(0), limit).map_err(ToolError::Failed)?;
        page["result_id"] = json!(a.result_id);
        Ok(page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_with(v: Value) -> (ReadResult, String) {
        let s = Arc::new(ResultStore::default());
        let id = s.put(v);
        (ReadResult(s), id)
    }

    #[tokio::test]
    async fn pages_through_a_parked_result_by_pointer() {
        let (t, id) =
            tool_with(json!({"rows": (0..120).collect::<Vec<_>>(), "meta": {"source": "x"}}));
        let p1 = t
            .call(json!({"result_id": id, "pointer": "/rows", "limit": 100}))
            .await
            .unwrap();
        assert_eq!(
            (
                p1["total"].as_u64(),
                p1["returned"].as_u64(),
                p1["next_offset"].as_u64()
            ),
            (Some(120), Some(100), Some(100))
        );
        let p2 = t
            .call(json!({"result_id": id, "pointer": "/rows", "offset": 100, "limit": 100}))
            .await
            .unwrap();
        assert_eq!(p2["items"].as_array().unwrap().len(), 20);
        assert!(p2["next_offset"].is_null());
        let m = t
            .call(json!({"result_id": id, "pointer": "/meta"}))
            .await
            .unwrap();
        assert_eq!(m["value"], json!({"source": "x"}));
        assert_eq!(p1["result_id"], json!(id));
    }

    #[tokio::test]
    async fn unknown_handles_bad_pointers_and_extra_arguments_are_rejected() {
        let (t, id) = tool_with(json!({"a": 1}));
        assert!(t
            .call(json!({"result_id": "nope"}))
            .await
            .unwrap_err()
            .to_string()
            .contains("expired"));
        assert!(matches!(
            t.call(json!({"result_id": id, "pointer": "rows"})).await,
            Err(ToolError::InvalidArgs(_))
        ));
        assert!(t
            .call(json!({"result_id": id, "pointer": "/missing"}))
            .await
            .unwrap_err()
            .to_string()
            .contains("does not exist"));
        assert!(matches!(
            t.call(json!({"result_id": id, "x": 1})).await,
            Err(ToolError::InvalidArgs(_))
        ));
    }

    #[tokio::test]
    async fn end_to_end_through_the_registry_an_oversized_tool_result_can_be_paged_back_completely()
    {
        struct Rows;
        #[async_trait]
        impl Tool for Rows {
            fn def(&self) -> ToolDef {
                ToolDef {
                    name: "rows",
                    description: "".into(),
                    input_schema: json!({"type":"object"}),
                    kind: ToolKind::Read,
                }
            }
            async fn call(&self, _: Value) -> Result<Value, ToolError> {
                Ok(
                    json!({"rows": (0..2000).map(|i| json!({"i": i, "pad": "x".repeat(200)})).collect::<Vec<_>>()}),
                )
            }
        }
        let reg = crate::tool::ToolRegistry::new().with(Rows);
        let reg = {
            let store = reg.results();
            reg.with(ReadResult(store))
        };
        let parked = reg.call("rows", json!({})).await.unwrap();
        assert_eq!(parked["result_truncated"], true);
        assert_eq!(parked["shape"]["fields"]["rows"]["len"], 2000);
        let id = parked["result_id"].as_str().unwrap().to_string();

        let mut seen = 0u64;
        let mut offset = 0u64;
        loop {
            let page = reg
                .call(
                    RESULT_TOOL,
                    json!({"result_id": id, "pointer": "/rows", "offset": offset, "limit": 500}),
                )
                .await
                .unwrap();
            let items = page["items"].as_array().unwrap();
            assert_eq!(
                items[0]["i"].as_u64(),
                Some(offset),
                "pages are contiguous and in order"
            );
            seen += items.len() as u64;
            match page["next_offset"].as_u64() {
                Some(n) => offset = n,
                None => break,
            }
        }
        assert_eq!(seen, 2000, "every row is reachable");
    }
}
