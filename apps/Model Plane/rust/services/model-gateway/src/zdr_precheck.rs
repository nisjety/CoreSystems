//! The gateway's answer to a turn that names the user's own model connection
//! while the signed token says zero data retention (ZDR).
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
//! not been used. The BFF hid it in steady state by forcing `zdr: true` into
//! every body, which sends the turn to the generic refusal in `normalize`
//! instead; a client that is not the BFF, or a window in which the BFF and Auth
//! Core disagree about the organisation, did not get that cover.
//!
//! ## What this does
//!
//! [`refuse_subscription_under_token_zdr`] is a pure function of the verified
//! claims and the request. Both transports (`/v1/invoke/stream` and
//! `/v1/invoke`) call it first, before normalisation, budget, a run or any
//! model call, and a refusal it returns is final: one typed answer, no run, no
//! provider touched. It deliberately lives outside `normalize`: normalisation
//! reads the request only (it has no claims, and 38 tests call it that way), and
//! the retention posture of a turn is the token's, never the body's.
//!
//! It reads the token's own `zdr` and nothing else. A body that says
//! `zdr: false` cannot lower a token that says `true` (the flag can only raise
//! the posture), and a body that says `zdr: true` on a token that does not is
//! `normalize`'s to refuse, byte for byte as before. A caller whose token is not
//! ZDR is never touched by anything in this file.
//!
//! The code is [`ZDR_CONNECTION_NOT_SUPPORTED`], the neutral catalogue code:
//! nothing can be acknowledged yet, so the answer must not promise that
//! something can. The resolver (Z-15) later splits this case into the codes of
//! the posture matrix (`zdr_connection_not_acknowledged` once an
//! acknowledgement path exists, `zdr_explicit_request_not_supported_by_connection`
//! for Temporary chat); until then this is the one honest answer.
//!
//! [`ConnectionRoute`] is the backstop. Were the pre-check ever bypassed, the
//! persistence-free branch forwards the user's connection route to
//! inference-core instead of dropping it, and inference-core, which will not
//! serve a connection without ZDR support under a ZDR request, refuses it. A
//! platform provider hint keeps what it always did on this path (the default ZDR
//! chain, filtered by attestation); what a ZDR turn that names a platform
//! provider should do is the resolver's decision, not this stop-gap's.

use axum::{http::StatusCode, Json};
use serde_json::{json, Value};

use crate::auth::Claims;
use crate::http_routes::InvokeRequest;

/// The stable code of this refusal (the Z-05a catalogue; also in the posture
/// matrix vocabulary `refusal_codes`).
pub const ZDR_CONNECTION_NOT_SUPPORTED: &str = "zdr_connection_not_supported";

/// What the user is told. It states only what is true: nothing was sent to any
/// model and no other model stood in. It names no way out, because none exists
/// yet.
const CONNECTION_NOT_SUPPORTED_MESSAGE: &str = "Zero data retention is on for this organisation, and the ChatGPT connection selected for this chat cannot provide it. The request was not sent to any model. No other model was used.";

/// A refusal ready to be rendered by either transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZdrRefusal {
    /// Stable, matchable code.
    pub code: &'static str,
    /// Human-readable reason (English; clients word it by `code`).
    pub message: &'static str,
    /// Whether sending the same turn again could ever succeed.
    pub retryable: bool,
}

impl ZdrRefusal {
    /// A turn that names the user's own connection while ZDR applies.
    pub const CONNECTION_NOT_SUPPORTED: Self = Self {
        code: ZDR_CONNECTION_NOT_SUPPORTED,
        message: CONNECTION_NOT_SUPPORTED_MESSAGE,
        retryable: false,
    };

    /// The unary body, in the flat shape `/v1/invoke` already uses for its other
    /// typed codes (`error` is the sentence, `code` the match key).
    #[must_use]
    pub fn body(&self) -> Value {
        json!({
            "error": self.message,
            "code": self.code,
            "retryable": self.retryable,
        })
    }

    /// The unary answer. 412 is what every other ZDR precondition on this
    /// gateway answers with (a failed-precondition from inference-core, the
    /// durable-mutation guard, speech).
    pub fn http(&self) -> (StatusCode, Json<Value>) {
        (StatusCode::PRECONDITION_FAILED, Json(self.body()))
    }
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

/// The refusal a turn gets when the token says ZDR and the request names the
/// user's own connection; `None` for every other turn, which is then untouched.
///
/// Takes the verified [`Claims`] rather than a flag so no caller can hand it
/// the wrong posture (the token's `zdr`, never `effective_zdr(req.zdr)`).
#[must_use]
pub fn refuse_subscription_under_token_zdr(
    claims: &Claims,
    req: &InvokeRequest,
) -> Option<ZdrRefusal> {
    if claims.zdr
        && names_user_connection(
            req.provider.as_deref(),
            req.subscription_connection_id.as_deref(),
        )
    {
        return Some(ZdrRefusal::CONNECTION_NOT_SUPPORTED);
    }
    None
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

    /// The payload of a model-gateway token as Auth Core mints it, with `zdr`
    /// present only when given, decoded by the same serde path the middleware
    /// uses. No struct literal: whatever a token without the claim means is
    /// decided by `Claims` itself, not by this test.
    fn token_claims(zdr: Option<bool>) -> Claims {
        let mut payload = json!({
            "sub": "user-1",
            "iss": "https://auth.test/model",
            "aud": "model-gateway",
            "exp": 4_102_444_800_i64,
            "nbf": 1_700_000_000_i64,
            "org_id": "org-1",
            "user_id": "user-1",
            "principal_type": "user",
        });
        if let Some(zdr) = zdr {
            payload["zdr"] = json!(zdr);
        }
        serde_json::from_value(payload).expect("a minted token's payload is valid Claims")
    }

    /// A body as a client sends it, parsed the way the route parses it.
    fn body(value: Value) -> InvokeRequest {
        serde_json::from_value(value).expect("a wire body parses")
    }

    fn terra() -> InvokeRequest {
        body(json!({
            "content": "hello",
            "model": "gpt-5.6-terra",
            "provider": "openai-codex-subscription",
            "subscription_connection_id": "conn-1",
        }))
    }

    #[test]
    fn a_zdr_token_naming_the_subscription_is_refused() {
        let refusal = refuse_subscription_under_token_zdr(&token_claims(Some(true)), &terra())
            .expect("a ZDR token on the user's own connection is refused");
        assert_eq!(refusal, ZdrRefusal::CONNECTION_NOT_SUPPORTED);
        assert_eq!(refusal.code, "zdr_connection_not_supported");
        assert!(!refusal.retryable);
    }

    #[test]
    fn the_route_is_recognised_however_it_is_named() {
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
            assert!(
                refuse_subscription_under_token_zdr(&token_claims(Some(true)), &body(naming.clone()))
                    .is_some(),
                "{naming}"
            );
        }
    }

    #[test]
    fn a_zdr_token_that_names_no_connection_is_not_this_refusals_business() {
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
            assert!(
                refuse_subscription_under_token_zdr(&token_claims(Some(true)), &body(request.clone()))
                    .is_none(),
                "{request}"
            );
        }
    }

    #[test]
    fn a_token_without_zdr_is_never_touched() {
        // Present-and-false and absent are the same here: this reads the token's
        // own claim and invents no posture.
        for zdr in [Some(false), None] {
            for request in [
                terra(),
                body(json!({"content": "x", "zdr": true, "provider": "openai-codex-subscription", "subscription_connection_id": "c"})),
                body(json!({"content": "x"})),
            ] {
                assert_eq!(
                    refuse_subscription_under_token_zdr(&token_claims(zdr), &request),
                    None,
                    "token zdr {zdr:?}"
                );
            }
        }
    }

    #[test]
    fn the_body_can_raise_the_posture_but_never_lower_the_token() {
        // `zdr: false` in the body on a ZDR token changes nothing.
        let lowered = body(json!({
            "content": "x", "zdr": false,
            "provider": "openai-codex-subscription", "subscription_connection_id": "c",
        }));
        assert!(refuse_subscription_under_token_zdr(&token_claims(Some(true)), &lowered).is_some());
        // `zdr: true` in the body on a token that is not ZDR is `normalize`'s to
        // refuse, so this does not.
        let raised = body(json!({
            "content": "x", "zdr": true,
            "provider": "openai-codex-subscription", "subscription_connection_id": "c",
        }));
        assert!(refuse_subscription_under_token_zdr(&token_claims(Some(false)), &raised).is_none());
    }

    /// The enforcement rows of the posture matrix that a single token and a
    /// single route decide, and that this refusal can answer (the matrix is
    /// evaluated whole by the resolver, Z-15). Each is (row id, token zdr,
    /// provider, connection, refused here). Rows whose token is not ZDR are
    /// the resolver's, never this refusal's.
    #[test]
    fn it_answers_the_matrix_rows_a_single_token_and_route_decide() {
        let rows: [(&str, bool, &str, Option<&str>, bool); 8] = [
            ("e_persistent_default_route", false, "", None, false),
            ("e_persistent_subscription_route_unchanged", false, "openai-codex-subscription", Some("conn-1"), false),
            ("e_zdr_default_route", true, "", None, false),
            ("e_zdr_token_never_reaches_subscription", true, "openai-codex-subscription", Some("conn-1"), true),
            ("e_zdr_token_subscription_explicit", true, "openai-codex-subscription", Some("conn-1"), true),
            ("e_temporary_chat_refuses_subscription_org_off", false, "openai-codex-subscription", Some("conn-1"), false),
            ("e_waiver_with_zdr_true", true, "openai-codex-subscription", Some("conn-1"), true),
            ("e_waiver_platform_provider", false, "azure-openai", None, false),
        ];
        for (id, zdr, provider, connection, refused) in rows {
            let mut wire = json!({"content": "x", "provider": provider});
            if let Some(connection) = connection {
                wire["subscription_connection_id"] = json!(connection);
            }
            assert_eq!(
                refuse_subscription_under_token_zdr(&token_claims(Some(zdr)), &body(wire)).is_some(),
                refused,
                "{id}"
            );
        }
    }

    #[test]
    fn the_refusal_is_typed_final_and_promises_nothing_that_cannot_happen() {
        let refusal = ZdrRefusal::CONNECTION_NOT_SUPPORTED;
        let (status, Json(wire)) = refusal.http();
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(wire["code"], "zdr_connection_not_supported");
        assert_eq!(wire["retryable"], false);
        assert_eq!(wire["error"], refusal.message);

        let message = refusal.message;
        assert!(message.contains("not sent to any model"), "{message}");
        assert!(message.contains("No other model was used"), "{message}");
        // Nothing can be acknowledged, consented to or waived yet, so the
        // sentence may not point at any of it.
        for promise in ["acknowledge", "consent", "waive", "continue", "retry", "try again"] {
            assert!(!message.to_lowercase().contains(promise), "{promise}: {message}");
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
    /// pre-check stands, so this reads the one place that matters: the
    /// persistence-free stream may not call `direct_infer` with a literal empty
    /// provider and connection again, which is the line the defect lived on.
    #[test]
    fn the_zdr_stream_forwards_its_route_instead_of_dropping_it() {
        let source = include_str!("sse.rs");
        let start = source
            .find("async fn zdr_direct_stream(")
            .expect("`zdr_direct_stream` exists");
        let tail = &source[start..];
        let end = tail.find("\n}\n").expect("the function ends");
        let function = &tail[..end];
        let call = function
            .split("direct_infer(")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .expect("zdr_direct_stream calls direct_infer");
        assert!(
            call.contains("&route.provider_hint")
                && call.contains("&route.subscription_connection_id"),
            "zdr_direct_stream must pass its route to direct_infer: {call}"
        );
    }
}
