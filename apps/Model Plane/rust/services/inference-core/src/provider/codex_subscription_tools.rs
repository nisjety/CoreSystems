//! Constrained proposals only. Integration Core never executes these tools;
//! Model Gateway still validates and authorizes every action before dispatch.
use super::{ChatMessage, InferRequest, ProviderError, ToolCall};
use serde::Deserialize;
use serde_json::{json, Value};

fn invalid(message: &str) -> ProviderError {
    ProviderError::InvalidResponse(message.to_owned())
}

pub(super) fn prepare(
    req: &InferRequest,
) -> Result<(Vec<ChatMessage>, Option<Value>), ProviderError> {
    let mut messages = req.messages.clone();
    if req.tools.is_empty() {
        let schema = req
            .structured_output_schema
            .as_ref()
            .map(|raw| {
                let value: Value = serde_json::from_str(raw)
                    .map_err(|_| invalid("invalid subscription output schema"))?;
                if value["type"] != "object" {
                    return Err(invalid(
                        "subscription output schema must describe an object",
                    ));
                }
                Ok(value)
            })
            .transpose()?;
        return Ok((messages, schema));
    }
    if req.structured_output_schema.is_some() {
        return Err(invalid(
            "subscription tools and a separate output schema cannot be combined",
        ));
    }
    let choice = req.tool_choice.as_str();
    if !matches!(choice, "" | "auto" | "none" | "required")
        && !req.tools.iter().any(|tool| tool.name == choice)
    {
        return Err(invalid("subscription tool choice is not offered"));
    }
    let definitions = req
        .tools
        .iter()
        .map(|tool| {
            let parameters: Value = serde_json::from_str(&tool.parameters_json)
                .map_err(|_| invalid("invalid subscription tool parameters"))?;
            Ok(json!({"name":tool.name,"description":tool.description,"parameters":parameters}))
        })
        .collect::<Result<Vec<_>, ProviderError>>()?;
    // Arguments are encoded as JSON text because the supplied tool schemas may
    // contain optional fields or arbitrary objects unsupported by strict output
    // schemas. The normal gateway dispatch validates their actual contracts.
    messages.push(ChatMessage { role: "system".into(), content: format!(
        "Verevon tool-proposal protocol: return only the constrained JSON object. Do not execute native Codex tools. toolCalls are proposals for Verevon's existing action dispatcher, not claims that work has run. Each arguments field must be a JSON-encoded object matching that tool's parameters. Use content for ordinary response text, usually empty when proposing tools. tool_choice={choice:?}: required means at least one proposal; none means none; a tool name means only that tool; auto permits either. Respect dependencies: do not propose operations that depend on results you have not received. Allowed definitions: {}", Value::Array(definitions)
    ), name: String::new(), compaction_summary: String::new() });
    let names: Vec<_> = if matches!(choice, "" | "auto" | "none" | "required") {
        req.tools.iter().map(|tool| tool.name.as_str()).collect()
    } else {
        vec![choice]
    };
    let minimum_calls = if matches!(choice, "" | "auto" | "none") { 0 } else { 1 };
    let maximum_calls = if choice == "none" { 0 } else { 16 };
    Ok((
        messages,
        Some(json!({
            "type":"object", "additionalProperties":false, "required":["content","toolCalls"],
            "properties": {
                "content":{"type":"string"},
                "toolCalls":{"type":"array","minItems":minimum_calls,"maxItems":maximum_calls,"items":{
                    "type":"object","additionalProperties":false,"required":["name","arguments"],
                    "properties":{"name":{"type":"string","enum":names},"arguments":{"type":"string"}}
                }}
            }
        })),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Proposal {
    content: String,
    tool_calls: Vec<Call>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
    name: String,
    arguments: String,
}

pub(super) fn parse(
    req: &InferRequest,
    content: &str,
) -> Result<(String, Vec<ToolCall>), ProviderError> {
    if req.tools.is_empty() {
        return Ok((content.to_owned(), Vec::new()));
    }
    let proposal: Proposal = serde_json::from_str(content)
        .map_err(|_| invalid("invalid subscription tool proposal envelope"))?;
    let choice = req.tool_choice.as_str();
    if proposal.tool_calls.len() > 16
        || (choice == "none" && !proposal.tool_calls.is_empty())
        || (!matches!(choice, "" | "auto" | "none") && proposal.tool_calls.is_empty())
    {
        return Err(invalid(
            "subscription proposal violates tool choice or call limit",
        ));
    }
    let mut calls = Vec::new();
    for call in proposal.tool_calls {
        if !req.tools.iter().any(|tool| tool.name == call.name)
            || (!matches!(choice, "" | "auto" | "none" | "required") && call.name != choice)
        {
            return Err(invalid(
                "subscription proposed a tool outside the allowed selection",
            ));
        }
        let args: Value = serde_json::from_str(&call.arguments)
            .map_err(|_| invalid("invalid subscription tool arguments"))?;
        if !args.is_object() {
            return Err(invalid("subscription tool arguments must be an object"));
        }
        calls.push(ToolCall {
            id: format!("codex-{}", mp_ids::new_ulid()),
            name: call.name,
            arguments_json: args.to_string(),
        });
    }
    Ok((proposal.content, calls))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ToolDefinition;
    fn request() -> InferRequest {
        InferRequest {
            tools: vec![ToolDefinition {
                name: "create_artifact".into(),
                description: "Draft a document".into(),
                parameters_json: r#"{"type":"object"}"#.into(),
            }],
            tool_choice: "required".into(),
            ..Default::default()
        }
    }
    #[test]
    fn proposals_are_data_and_unknown_or_malformed_actions_fail_closed() {
        let req = request();
        let (messages, schema) = prepare(&req).unwrap();
        assert!(messages
            .last()
            .unwrap()
            .content
            .contains("Do not execute native Codex tools"));
        assert_eq!(
            schema.unwrap()["properties"]["toolCalls"]["items"]["properties"]["name"]["enum"],
            json!(["create_artifact"])
        );
        let (_, schema) = prepare(&req).unwrap();
        assert_eq!(schema.as_ref().unwrap()["properties"]["toolCalls"]["minItems"], 1);
        assert_eq!(schema.as_ref().unwrap()["properties"]["toolCalls"]["maxItems"], 16);
        let (_, schema) = prepare(&InferRequest { tool_choice: "none".into(), ..req.clone() }).unwrap();
        assert_eq!(schema.as_ref().unwrap()["properties"]["toolCalls"]["maxItems"], 0);
        let mut named = req.clone();
        named.tools.push(ToolDefinition { name: "finish_tool_phase".into(), description: "Finish".into(), parameters_json: r#"{"type":"object"}"#.into() });
        named.tool_choice = "create_artifact".into();
        let (_, schema) = prepare(&named).unwrap();
        assert_eq!(schema.as_ref().unwrap()["properties"]["toolCalls"]["items"]["properties"]["name"]["enum"], json!(["create_artifact"]));
        for value in [
            json!({"content":"","toolCalls":[]}),
            json!({"content":"","toolCalls":[{"name":"shell","arguments":"{}"}]}),
            json!({"content":"","toolCalls":[{"name":"create_artifact","arguments":"[]"}]}),
            json!({"content":"","toolCalls":[{"name":"create_artifact","arguments":"incomplete"}]}),
        ] {
            assert!(parse(&req, &value.to_string()).is_err());
        }
        let raw=json!({"content":"","toolCalls":[{"name":"create_artifact","arguments":"{\"content\":\"Hei\"}"}]}).to_string();
        let (_, calls) = parse(&req, &raw).unwrap();
        assert_eq!(calls[0].name, "create_artifact");
        assert_eq!(
            serde_json::from_str::<Value>(&calls[0].arguments_json).unwrap()["content"],
            "Hei"
        );
        assert_ne!(calls[0].id, parse(&req, &raw).unwrap().1[0].id);
        assert!(parse(
            &InferRequest {
                tool_choice: "none".into(),
                ..req
            },
            &raw
        )
        .is_err());
    }
    #[test]
    fn structured_review_and_plain_text_have_distinct_protocols() {
        assert!(prepare(&InferRequest::default()).unwrap().1.is_none());
        let req = InferRequest {
            structured_output_schema: Some(
                r#"{"type":"object","properties":{"accepted":{"type":"boolean"}}}"#.into(),
            ),
            ..Default::default()
        };
        assert_eq!(
            prepare(&req).unwrap().1.unwrap()["properties"]["accepted"]["type"],
            "boolean"
        );
        assert!(prepare(&InferRequest {
            structured_output_schema: req.structured_output_schema,
            ..request()
        })
        .is_err());
    }
}
