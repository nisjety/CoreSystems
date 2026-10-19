//! The gateway's handling of a turn that names the user's own model connection, under zero data
//! retention (ZDR).
//!
//! ## The defect this closes
//!
//! A user can pin a turn to their own `ChatGPT` subscription
//! (`provider: openai-codex-subscription` plus the opaque
//! `subscription_connection_id`). A subscription cannot give zero retention, so
//! the persistence-free ZDR branch cannot serve it. Normalisation only looked at
//! the REQUEST's `zdr` flag, so a turn whose ZDR came from the signed TOKEN (an
//! organisation with ZDR on, a body that said nothing) passed it, took the ZDR
//! branch and called inference-core with the provider hint and the connection id
//! replaced by empty strings. The user's choice vanished: the turn was served,
//! or refused, on the default ZDR chain and nothing said their connection had
//! not been used.
//!
//! ## What decides it now
//!
//! The posture of a turn is decided once, before normalisation, a run, a budget read or any model
//! call, by `zdr_posture::decide`, which goes through the mp-contracts resolver (Z-15). A ZDR token
//! on the user's own connection is refused there as `zdr_connection_not_acknowledged`, a waiver is
//! honoured only on the connection it names, and every other combination follows the posture
//! matrix. A body that says `zdr: false` cannot lower a token that says `true`.
//!
//! This module keeps what the decision reads and what the persistence-free path forwards:
//!
//! * [`names_user_connection`] recognises a request that names one of the caller's own model
//!   connections, under every spelling inference-core accepts.
//! * [`ConnectionRoute`] is the backstop. Were the decision ever bypassed, the persistence-free
//!   branch forwards the user's connection route to inference-core instead of dropping it, and
//!   inference-core refuses a connection without ZDR support under a ZDR request.
//! * [`http`] is the unary answer to a typed refusal, in the shape `zdr_posture` defines.

use axum::{http::StatusCode, Json};
use mp_contracts::zdr_refusal::Refusal;
use serde_json::Value;

/// The unary answer to a typed ZDR refusal. The status is `zdr_posture::http_status`'s:
/// 403 for a refusal about credentials or routes, 412 for every other ZDR precondition.
pub fn http(refusal: &Refusal) -> (StatusCode, Json<Value>) {
    crate::zdr_posture::http(refusal)
}

/// Whether `provider` names the `ChatGPT` subscription route the way inference-core
/// reads a provider hint: trimmed, case-insensitive, `_` read as `-`, the
/// canonical id or either alias. `normalize` matches a little less (no `_`),
/// which only matters here: a hint it does not recognise would otherwise sail
/// past it with no connection id and be dropped by the ZDR branch.
fn names_subscription_provider(provider: &str) -> bool {
    let reading = provider.trim().to_ascii_lowercase().replace('_', "-");
    crate::normalize::is_codex_subscription_hint(&reading)
}

/// Whether a request names one of the caller's own model connections: the
/// subscription provider under any spelling inference-core accepts, or a
/// connection id (even without a provider, which `normalize` would call
/// malformed, because the caller did name a connection).
#[must_use]
pub fn names_user_connection(
    provider: Option<&str>,
    subscription_connection_id: Option<&str>,
) -> bool {
    provider.is_some_and(names_subscription_provider)
        || subscription_connection_id.is_some_and(|id| !id.trim().is_empty())
}

/// The user-connection route the persistence-free path forwards to
/// inference-core: the provider hint and connection id when the turn names the
/// user's own connection, both empty otherwise (a platform provider hint keeps
/// the default ZDR chain, as it always did on this path).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ConnectionRoute {
    pub provider_hint: String,
    pub subscription_connection_id: String,
}

impl ConnectionRoute {
    /// The route named by a normalised request's provider hint and connection
    /// id (both already trimmed).
    #[must_use]
    pub fn of(provider_hint: &str, subscription_connection_id: &str) -> Self {
        if names_user_connection(Some(provider_hint), Some(subscription_connection_id)) {
            Self {
                provider_hint: provider_hint.trim().to_owned(),
                subscription_connection_id: subscription_connection_id.trim().to_owned(),
            }
        } else {
            Self::default()
        }
    }

    /// Whether a connection is named at all.
    #[must_use]
    pub fn is_named(&self) -> bool {
        !self.provider_hint.is_empty() || !self.subscription_connection_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A body as a client sends it, parsed the way the route parses it.
    fn body(value: Value) -> crate::http_routes::InvokeRequest {
        serde_json::from_value(value).expect("a wire body parses")
    }

    #[test]
    fn a_request_naming_the_subscription_or_a_connection_names_a_user_connection() {
        // inference-core reads a hint trimmed, case-insensitively, `_` as `-`,
        // with two aliases; the camelCase body alias carries a connection id.
        let namings = [
            json!({"content": "x", "provider": "openai-codex-subscription"}),
            json!({"content": "x", "provider": "OpenAI-Codex-Subscription"}),
            json!({"content": "x", "provider": "openai_codex_subscription"}),
            json!({"content": "x", "provider": "  openai-codex-subscription "}),
            json!({"content": "x", "provider": "chatgpt-codex"}),
            json!({"content": "x", "provider": "codex_subscription"}),
            json!({"content": "x", "provider": "openai-codex-subscription", "subscription_connection_id": "c"}),
            json!({"content": "x", "subscriptionConnectionId": "c"}),
            json!({"content": "x", "provider": "azure-openai", "subscription_connection_id": "c"}),
        ];
        for naming in namings {
            let req = body(naming.clone());
            assert!(
                names_user_connection(
                    req.provider.as_deref(),
                    req.subscription_connection_id.as_deref()
                ),
                "{naming}"
            );
        }
    }

    #[test]
    fn a_request_naming_no_connection_or_a_platform_provider_names_no_user_connection() {
        let unnamed = [
            json!({"content": "x"}),
            json!({"content": "x", "model": "gpt-5-mini"}),
            json!({"content": "x", "provider": ""}),
            json!({"content": "x", "provider": "   "}),
            json!({"content": "x", "subscription_connection_id": ""}),
            json!({"content": "x", "subscription_connection_id": "   "}),
            // A platform provider is not the user's connection: it keeps the
            // default ZDR chain, as before.
            json!({"content": "x", "provider": "azure-openai"}),
            json!({"content": "x", "provider": "anthropic"}),
            json!({"content": "x", "provider": "openai-codex"}),
        ];
        for request in unnamed {
            let req = body(request.clone());
            assert!(
                !names_user_connection(
                    req.provider.as_deref(),
                    req.subscription_connection_id.as_deref()
                ),
                "{request}"
            );
        }
    }

    #[test]
    fn the_refusal_is_typed_final_and_promises_nothing_that_cannot_happen() {
        let refusal = Refusal::CONNECTION_NOT_SUPPORTED;
        let (status, Json(wire)) = http(&refusal);
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(wire["error"]["code"], "zdr_connection_not_supported");
        assert_eq!(wire["error"]["retryable"], false);
        assert_eq!(wire["error"]["action"], "choose_another_model");
        assert_eq!(wire["error"]["message"], refusal.message());

        let message = refusal.message();
        assert!(message.contains("not sent to any model"), "{message}");
        assert!(message.contains("No other model was used"), "{message}");
        // Nothing can be acknowledged, consented to or waived yet, so the
        // sentence may not point at any of it.
        for promise in [
            "acknowledge",
            "consent",
            "waive",
            "continue",
            "retry",
            "try again",
        ] {
            assert!(
                !message.to_lowercase().contains(promise),
                "{promise}: {message}"
            );
        }
    }

    #[test]
    fn the_backstop_forwards_only_a_named_connection() {
        let named = ConnectionRoute::of("openai-codex-subscription", "conn-1");
        assert_eq!(named.provider_hint, "openai-codex-subscription");
        assert_eq!(named.subscription_connection_id, "conn-1");
        assert!(named.is_named());

        // The alias is forwarded as the caller wrote it (trimmed), so
        // inference-core reads it exactly as it would have on any other path.
        let alias = ConnectionRoute::of("  chatgpt-codex ", "");
        assert_eq!(alias.provider_hint, "chatgpt-codex");
        assert!(alias.is_named());

        // A platform provider hint is not forwarded: today's behaviour.
        for (hint, connection) in [("", ""), ("azure-openai", ""), ("anthropic", "")] {
            let route = ConnectionRoute::of(hint, connection);
            assert_eq!(route, ConnectionRoute::default(), "{hint:?}");
            assert!(!route.is_named());
        }
    }

    /// The ZDR branch must hand the route it was given to the inference call.
    /// The call itself cannot be reached through the HTTP surface while the
    /// decision stands, so this reads the one place that matters: the
    /// persistence-free stream may not call `direct_infer` with a literal empty
    /// provider and connection again, which is the line the defect lived on.
    #[test]
    fn the_zdr_stream_forwards_its_route_instead_of_dropping_it() {
        // A Windows checkout (core.autocrlf) has CRLF line ends; the scan below looks for an LF terminator.
        let source = include_str!("sse.rs").replace("\r\n", "\n");
        let start = source
            .find("async fn zdr_direct_stream(")
            .expect("`zdr_direct_stream` exists");
        let tail = &source[start..];
        let end = tail.find("\n}\n").expect("the function ends");
        let function = &tail[..end];
        let call = function
            .split("direct_infer_turn(")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .expect("zdr_direct_stream calls direct_infer_turn");
        assert!(
            call.contains("&route.provider_hint")
                && call.contains("&route.subscription_connection_id"),
            "zdr_direct_stream must pass its route to direct_infer: {call}"
        );
    }
}
