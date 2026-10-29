use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;
use serde_json::Value;

use crate::{config::AppState, envelope::ok, middleware::require_session};

#[derive(Clone, Copy)]
enum RuntimeResource {
    Agents,
    Groups,
    Macros,
}

#[derive(Debug, Serialize)]
struct RuntimeStatus {
    support: SupportRuntimeStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SupportRuntimeStatus {
    configured: bool,
    connected: bool,
    agents: usize,
    groups: usize,
    macros: usize,
    message: &'static str,
}

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/agents/chatbot/runtime", get(chatbot_runtime))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

async fn chatbot_runtime(State(state): State<AppState>) -> impl IntoResponse {
    if state.zammad_api_token.is_empty() {
        return Json(ok(RuntimeStatus {
            support: SupportRuntimeStatus {
                configured: false,
                connected: false,
                agents: 0,
                groups: 0,
                macros: 0,
                message: "Set ZAMMAD_API_URL and ZAMMAD_API_TOKEN to enable live support actions.",
            },
        }));
    }

    let agents = fetch_runtime_count(&state, RuntimeResource::Agents);
    let groups = fetch_runtime_count(&state, RuntimeResource::Groups);
    let macros = fetch_runtime_count(&state, RuntimeResource::Macros);
    let (agents, groups, macros) = tokio::join!(agents, groups, macros);

    let agents = agents.unwrap_or(0);
    let groups = groups.unwrap_or(0);
    let macros = macros.unwrap_or(0);
    let connected = agents > 0 || groups > 0 || macros > 0;

    Json(ok(RuntimeStatus {
        support: SupportRuntimeStatus {
            configured: true,
            connected,
            agents,
            groups,
            macros,
            message: if connected {
                "Support integration connected. Agent actions can use live support teams, groups, and macros."
            } else {
                "Support integration is configured, but Verevon could not reach Zammad."
            },
        },
    }))
}

async fn fetch_runtime_count(state: &AppState, resource: RuntimeResource) -> Option<usize> {
    let url = match resource {
        RuntimeResource::Agents => format!("{}/api/v1/users?role=Agent", state.zammad_api_url),
        RuntimeResource::Groups => format!("{}/api/v1/groups", state.zammad_api_url),
        RuntimeResource::Macros => format!("{}/api/v1/macros", state.zammad_api_url),
    };

    let response = state
        .client
        .get(url)
        .header(
            "authorization",
            format!("Token token={}", state.zammad_api_token),
        )
        .header("content-type", "application/json")
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let payload = response.json::<Value>().await.ok()?;
    Some(count_runtime_payload(&payload))
}

fn count_runtime_payload(payload: &Value) -> usize {
    if let Some(items) = payload.as_array() {
        return items.len();
    }

    if let Some(items) = payload.as_object() {
        return items.len();
    }

    0
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::count_runtime_payload;

    #[test]
    fn runtime_payload_count_accepts_arrays_and_objects() {
        assert_eq!(count_runtime_payload(&json!([{}, {}, {}])), 3);
        assert_eq!(count_runtime_payload(&json!({ "1": {}, "2": {} })), 2);
        assert_eq!(count_runtime_payload(&json!(null)), 0);
    }
}
