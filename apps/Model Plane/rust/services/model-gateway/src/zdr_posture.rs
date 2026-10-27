//! The turn posture of model-gateway: what a request is under zero data retention, decided once,
//! from the verified claims of every bearer it presents and the route it names.
//!
//! Every decision goes through the mp-contracts resolver (`mp_contracts::retention::resolve_turn`,
//! whose golden is `zdr-posture-matrix.json`). This module adapts the gateway's claims and request
//! to the resolver's inputs and turns its answer into what the gateway needs: the `zdr` flag each
//! downstream call carries, the retention class the usage frame states, the Data Plane `zdr_mode`,
//! and the typed refusal in its HTTP, gRPC and SSE shapes.
//!
//! Nothing else in the gateway reads a token's `zdr`, `org_zdr` or `zdr_waiver` to decide anything.
//! The source scan in this module's tests names every place that still reads one and fails on any
//! other, so a new site cannot quietly keep the old `effective_zdr` rule.
//!
//! A waived turn (`TurnPosture::Waived`) is persistent for its one connection. Its `zdr` flag is
//! false, its usage frame states `zdr_waived` with the reason, and its Data Plane retrieval runs in
//! `reject` mode, so restricted documents never reach the personal account. A waiver is honoured on
//! the one route it names and nowhere else: any other route is `zdr_waiver_route_mismatch`.
//!
//! Space ZDR is not applied in v1. A turn carries no space id to decide from (honest-limits L14).
//!
//! The waiver is minted by Auth Core only behind its kill switch, which is off by default, so until
//! that switch is on no token carries `zdr_waiver` and every turn decides exactly as it did before.

use axum::{http::StatusCode, Json};
use mp_contracts::retention::{
    resolve_turn, ClaimsView, Reason, Resolution, RouteView, WaiverView,
};
use mp_contracts::zdr_refusal::{code, Refusal};
use serde_json::Value;
use tonic::Code;

use crate::auth::{Claims, ZdrWaiverClaim};
use crate::sse_events::RetentionClass;

/// The route a request names, as the resolver reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Route<'a> {
    /// The provider as the request spells it. Empty is the default chain.
    pub provider: &'a str,
    /// The user connection the request names, when it names one.
    pub connection: Option<&'a str>,
    /// Whether the request asks for ZDR itself (Temporary chat, `x-zdr`, body `zdr: true`).
    pub explicit_zdr: bool,
}

impl<'a> Route<'a> {
    /// A request that names no provider and no connection: the default chain, or a request that is
    /// not a model call at all.
    #[must_use]
    pub const fn plain(explicit_zdr: bool) -> Self {
        Self {
            provider: "",
            connection: None,
            explicit_zdr,
        }
    }

    /// The route an invoke-shaped request names. Both names are trimmed, and an empty connection is
    /// no connection.
    #[must_use]
    pub fn named(
        provider: Option<&'a str>,
        connection: Option<&'a str>,
        explicit_zdr: bool,
    ) -> Self {
        Self {
            provider: provider.map_or("", str::trim),
            connection: connection.map(str::trim).filter(|name| !name.is_empty()),
            explicit_zdr,
        }
    }

    fn view(self) -> RouteView<'a> {
        RouteView {
            provider: self.provider,
            connection: self.connection,
            explicit_zdr: self.explicit_zdr,
        }
    }
}

/// What one turn is. Owned, so it outlives the claims and the body it was decided from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnPosture {
    /// Zero retention applies. The usage frame states `zdr`.
    Zdr { reason: Reason },
    /// Persistent, as a turn of an org that is not ZDR is. Nothing is stated about retention.
    Persistent,
    /// The org is ZDR, and this one connection runs with ZDR off for this turn only. The usage
    /// frame states `zdr_waived` with the reason.
    Waived { connection: String, reason: Reason },
}

impl TurnPosture {
    /// The `zdr` flag every downstream call carries. True only for a ZDR turn: a waived turn is
    /// persistent for its connection, so nothing downstream may treat it as ZDR.
    #[must_use]
    pub const fn zdr(&self) -> bool {
        matches!(self, Self::Zdr { .. })
    }

    /// Whether the turn runs under a waiver for one connection.
    #[must_use]
    pub const fn is_waived(&self) -> bool {
        matches!(self, Self::Waived { .. })
    }

    /// The connection a waived turn is bound to.
    #[must_use]
    pub fn waived_connection(&self) -> Option<&str> {
        match self {
            Self::Waived { connection, .. } => Some(connection),
            Self::Zdr { .. } | Self::Persistent => None,
        }
    }

    /// The retention class the usage frame states. A persistent turn states none, and its frames
    /// omit the key as they always have.
    #[must_use]
    pub fn retention_class(&self) -> Option<RetentionClass> {
        match self {
            Self::Zdr { .. } => Some(RetentionClass::Zdr),
            Self::Waived { reason, .. } => Some(RetentionClass::ZdrWaived {
                reason: reason.as_str(),
            }),
            Self::Persistent => None,
        }
    }

    /// The `zdr_mode` a Data Plane retrieval for this turn carries: `ephemeral` for a ZDR turn,
    /// `reject` for a waived turn (filtering restricted documents before they reach the personal
    /// account), and absent for a persistent turn, which is what the Data Plane takes absence to
    /// mean and what every persistent request has always sent.
    #[must_use]
    pub fn data_plane_mode(&self) -> Option<String> {
        crate::retrieval::data_plane_zdr_mode(self.zdr(), self.is_waived())
    }
}

/// The posture of one request, from every bearer it presents (the ingress token and each delegated
/// user bearer) and the route it names. A refusal is returned as its typed catalogue entry.
///
/// # Errors
/// The resolver's refusal: a posture mismatch between the bearers, a waiver on a route it does not
/// name, a ZDR turn on a connection that cannot give ZDR, and the other catalogue refusals.
pub fn decide(bearers: &[&Claims], route: &Route<'_>) -> Result<TurnPosture, Refusal> {
    let views: Vec<ClaimsView<'_>> = bearers.iter().map(|claims| claims_view(claims)).collect();
    decide_views(&views, route)
}

/// [`decide`] for one bearer.
///
/// # Errors
/// As [`decide`].
pub fn decide_for(claims: &Claims, route: &Route<'_>) -> Result<TurnPosture, Refusal> {
    decide(&[claims], route)
}

/// [`decide`] over resolver views built by a caller that holds verified claims in another shape
/// (gRPC identities keep their own fields).
///
/// # Errors
/// As [`decide`].
pub fn decide_views(bearers: &[ClaimsView<'_>], route: &Route<'_>) -> Result<TurnPosture, Refusal> {
    match resolve_turn(bearers, &route.view()) {
        Resolution::Zdr { reason } => Ok(TurnPosture::Zdr { reason }),
        Resolution::Persistent { .. } => Ok(TurnPosture::Persistent),
        Resolution::Waived {
            connection, reason, ..
        } => Ok(TurnPosture::Waived {
            connection: connection.to_owned(),
            reason,
        }),
        Resolution::Refuse(refusal) => Err(refusal),
    }
}

/// The resolver's view of one bearer's claims. The one place a verified token's retention claims
/// become resolver inputs.
#[must_use]
pub fn claims_view(claims: &Claims) -> ClaimsView<'_> {
    view_of(claims.zdr, claims.org_zdr, claims.zdr_waiver.as_ref())
}

/// The resolver's view of retention claims a caller holds in its own shape.
#[must_use]
pub fn view_of(
    zdr: bool,
    org_zdr: Option<bool>,
    waiver: Option<&ZdrWaiverClaim>,
) -> ClaimsView<'_> {
    ClaimsView {
        zdr,
        org_zdr,
        waiver: waiver.map(|waiver| WaiverView {
            version: waiver.version,
            provider: &waiver.provider,
            connection: &waiver.connection,
        }),
    }
}

/// The HTTP status a typed ZDR refusal answers with. The two refusals about credentials and routes
/// are 403, as the middleware's own posture refusal is. Every other ZDR precondition on this
/// gateway is 412.
#[must_use]
pub fn http_status(refusal: &Refusal) -> StatusCode {
    if is_credential_refusal(refusal) {
        StatusCode::FORBIDDEN
    } else {
        StatusCode::PRECONDITION_FAILED
    }
}

/// The unary answer to a typed ZDR refusal: its status and the catalogue body.
#[must_use]
pub fn http(refusal: &Refusal) -> (StatusCode, Json<Value>) {
    (http_status(refusal), Json(refusal.json_body()))
}

/// The gRPC code a typed ZDR refusal carries. The credential refusals are `PermissionDenied`, the
/// others `FailedPrecondition`, as the gateway's gRPC refusals have always been.
#[must_use]
pub fn grpc_code(refusal: &Refusal) -> Code {
    if is_credential_refusal(refusal) {
        Code::PermissionDenied
    } else {
        Code::FailedPrecondition
    }
}

/// Whether a request that names `route` runs ZDR, for a handler that answers a refusal in HTTP
/// (`Err` is the refusal's status and body). A waived token on a route it does not name is refused
/// here, as a route mismatch, before anything else is decided.
///
/// # Errors
/// The typed refusal as an HTTP answer.
pub fn zdr_or_refuse(
    claims: &Claims,
    route: &Route<'_>,
) -> Result<bool, (StatusCode, Json<Value>)> {
    decide_for(claims, route)
        .map(|posture| posture.zdr())
        .map_err(|refusal| http(&refusal))
}

/// The gate in front of a durable continuation: a stored run, plan, todo or approval is started or
/// advanced (an approval or rejection, a todo update, a plan approval, a resume, a cancel, a plan-mode
/// change). A ZDR credential is refused as a durable mutation before any lookup, because a ZDR run keeps
/// no plan, todo or approval to advance. A valid waiver is not refused here: on the plain route it is a
/// route mismatch, and that is the only case this gate lets through. Session Core checks the waiver
/// against the receipt of the object it would advance (owner decision 17). A persistent credential is
/// served here, and its stored posture is checked there too.
///
/// # Errors
/// The typed refusal as an HTTP answer.
pub fn continuation_gate(claims: &Claims) -> Result<(), (StatusCode, Json<Value>)> {
    match decide_for(claims, &Route::plain(false)) {
        Ok(TurnPosture::Zdr { .. }) => Err(http(&Refusal::DURABLE_MUTATION_FORBIDDEN)),
        Ok(TurnPosture::Persistent | TurnPosture::Waived { .. }) => Ok(()),
        Err(refusal) if refusal.code() == code::WAIVER_ROUTE_MISMATCH => Ok(()),
        Err(refusal) => Err(http(&refusal)),
    }
}

/// The posture decision of a content route: one whose request sends user content to inference on a
/// request that carries no retention field (translation, document analysis, language analysis, a
/// realtime session and video generation). Inference-core receives no retention flag for them, so
/// no provider can serve them under zero retention yet, and a ZDR turn is refused with the
/// operation's feature before any model call. A waived
/// turn is a route mismatch: a waiver binds one chat route on one connection, and these routes name
/// no connection. A persistent turn is served as it always was. `feature` is a key of the catalogue
/// vocabulary (`mp_contracts::zdr_refusal::FEATURES`), and the refusal is logged with it.
///
/// # Errors
/// The typed refusal as an HTTP answer.
pub fn decide_content_route(
    claims: &Claims,
    provider: Option<&str>,
    feature: &'static str,
) -> Result<(), (StatusCode, Json<Value>)> {
    let refusal = match decide_for(claims, &Route::named(provider, None, false)) {
        Ok(TurnPosture::Persistent) => return Ok(()),
        Ok(TurnPosture::Zdr { .. }) => Refusal::feature_not_supported(feature),
        Ok(TurnPosture::Waived { .. }) => Refusal::WAIVER_ROUTE_MISMATCH,
        Err(refusal) => refusal,
    };
    tracing::info!(
        feature,
        code = refusal.code(),
        "content route refused under zero data retention before any model call"
    );
    Err(http(&refusal))
}

fn is_credential_refusal(refusal: &Refusal) -> bool {
    matches!(
        refusal.code(),
        code::POSTURE_MISMATCH | code::WAIVER_ROUTE_MISMATCH
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_contracts::agentic_subscription::PROVIDER_ID;
    use serde_json::json;

    /// A user token as Auth Core mints it, decoded by the same serde path the middleware uses.
    fn token(zdr: bool, org_zdr: Option<bool>, waiver: Option<Value>) -> Claims {
        let mut payload = json!({
            "sub": "user-1",
            "iss": "https://auth.test/model",
            "aud": "model-gateway",
            "exp": 4_102_444_800_i64,
            "nbf": 1_700_000_000_i64,
            "org_id": "org-1",
            "user_id": "user-1",
            "principal_type": "user",
            "zdr": zdr,
        });
        if let Some(org_zdr) = org_zdr {
            payload["org_zdr"] = json!(org_zdr);
        }
        if let Some(waiver) = waiver {
            payload["zdr_waiver"] = waiver;
        }
        serde_json::from_value(payload).expect("a minted token's payload is valid Claims")
    }

    fn waiver(version: u32, provider: &str, connection: &str) -> Value {
        json!({"version": version, "provider": provider, "connection": connection})
    }

    fn bound() -> Route<'static> {
        Route::named(Some(PROVIDER_ID), Some("conn-1"), false)
    }

    #[test]
    fn the_retention_claims_decode_from_the_minted_shape_and_are_optional() {
        let plain = token(false, None, None);
        assert_eq!(plain.org_zdr, None);
        assert_eq!(plain.zdr_waiver, None);
        let waived = token(false, Some(true), Some(waiver(1, PROVIDER_ID, "conn-1")));
        assert_eq!(waived.org_zdr, Some(true));
        assert_eq!(
            waived.zdr_waiver,
            Some(ZdrWaiverClaim {
                version: 1,
                provider: PROVIDER_ID.to_owned(),
                connection: "conn-1".to_owned(),
            })
        );
    }

    /// Owner decision 17: a durable continuation (approve, reject, a todo update, a plan approval, a
    /// resume or a cancel) is refused for a ZDR credential before anything runs. A valid waiver is not
    /// refused here: session core checks it against the receipt of the run it would advance. A
    /// persistent credential of a ZDR org with no waiver, or a waiver of an unknown version, is refused.
    #[test]
    fn a_durable_continuation_refuses_zdr_and_leaves_a_waiver_to_the_stored_receipt() {
        let (status, body) = continuation_gate(&token(true, Some(true), None))
            .expect_err("a ZDR credential never advances durable work");
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(body.0["error"]["code"], code::DURABLE_MUTATION_FORBIDDEN);

        assert!(continuation_gate(&token(false, Some(false), None)).is_ok());
        assert!(continuation_gate(&token(
            false,
            Some(true),
            Some(waiver(1, PROVIDER_ID, "conn-1"))
        ))
        .is_ok());

        let (status, body) = continuation_gate(&token(false, Some(true), None))
            .expect_err("a persistent credential of a ZDR org is never served");
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body.0["error"]["code"], code::POSTURE_MISMATCH);

        let (_, body) = continuation_gate(&token(
            false,
            Some(true),
            Some(waiver(9, PROVIDER_ID, "conn-1")),
        ))
        .expect_err("a waiver of an unknown version is not read as the current one");
        assert_eq!(body.0["error"]["code"], code::POSTURE_MISMATCH);
    }

    #[test]
    fn a_persistent_token_on_the_default_route_is_persistent_and_states_nothing() {
        let got =
            decide_for(&token(false, Some(false), None), &Route::plain(false)).expect("decided");
        assert_eq!(got, TurnPosture::Persistent);
        assert!(!got.zdr());
        assert_eq!(got.retention_class(), None);
        assert_eq!(got.data_plane_mode(), None);
    }

    #[test]
    fn a_zdr_token_on_the_default_route_is_zdr_and_states_its_class() {
        let got =
            decide_for(&token(true, Some(true), None), &Route::plain(false)).expect("decided");
        assert_eq!(
            got,
            TurnPosture::Zdr {
                reason: Reason::OrgZdr
            }
        );
        assert!(got.zdr());
        assert_eq!(got.retention_class(), Some(RetentionClass::Zdr));
        assert_eq!(got.data_plane_mode().as_deref(), Some("ephemeral"));
    }

    #[test]
    fn a_zdr_token_never_reaches_the_subscription_route() {
        let refused = decide_for(&token(true, Some(true), None), &bound()).expect_err("refused");
        assert_eq!(refused, Refusal::CONNECTION_NOT_ACKNOWLEDGED);
    }

    /// The subscription aliases the resolver knows are exactly the ones the gateway reads as the
    /// subscription route, so no spelling is a subscription route to one and a platform route to the
    /// other. A ZDR token is refused on each of them.
    #[test]
    fn every_subscription_alias_is_read_by_the_gateway_and_refused_for_a_zdr_token() {
        assert_eq!(
            mp_contracts::retention::SUBSCRIPTION_ALIASES,
            &["codex-subscription", "chatgpt-codex"],
            "a new alias must be read by normalize::is_codex_subscription_hint too"
        );
        for alias in mp_contracts::retention::SUBSCRIPTION_ALIASES {
            assert!(
                crate::normalize::is_codex_subscription_hint(alias),
                "{alias}"
            );
            let refused = decide_for(
                &token(true, Some(true), None),
                &Route::named(Some(*alias), None, false),
            );
            assert_eq!(
                refused.expect_err("a ZDR token is refused on the alias"),
                Refusal::CONNECTION_NOT_ACKNOWLEDGED,
                "{alias}"
            );
        }
    }

    #[test]
    fn an_explicit_request_on_a_connection_is_refused_as_such_even_off_org() {
        let route = Route::named(Some(PROVIDER_ID), Some("conn-1"), true);
        let refused = decide_for(&token(false, Some(false), None), &route).expect_err("refused");
        assert_eq!(
            refused,
            Refusal::EXPLICIT_REQUEST_NOT_SUPPORTED_BY_CONNECTION
        );
    }

    #[test]
    fn a_waiver_is_honoured_on_its_bound_route_and_states_zdr_waived() {
        let got = decide_for(
            &token(false, Some(true), Some(waiver(1, PROVIDER_ID, "conn-1"))),
            &bound(),
        )
        .expect("decided");
        assert_eq!(
            got,
            TurnPosture::Waived {
                connection: "conn-1".to_owned(),
                reason: Reason::ConnectionNotZdrCapable,
            }
        );
        assert!(!got.zdr(), "a waived turn is not ZDR downstream");
        assert!(got.is_waived());
        assert_eq!(got.waived_connection(), Some("conn-1"));
        assert_eq!(
            got.retention_class(),
            Some(RetentionClass::ZdrWaived {
                reason: "connection_not_zdr_capable",
            })
        );
        assert_eq!(got.data_plane_mode().as_deref(), Some("reject"));
    }

    #[test]
    fn a_waiver_on_any_other_route_is_a_route_mismatch() {
        let waived = token(false, Some(true), Some(waiver(1, PROVIDER_ID, "conn-1")));
        let other_connection = Route::named(Some(PROVIDER_ID), Some("conn-2"), false);
        let default_chain = Route::plain(false);
        let platform = Route::named(Some("azure-openai"), None, false);
        let unnamed_connection = Route::named(Some(PROVIDER_ID), None, false);
        for route in [
            other_connection,
            default_chain,
            platform,
            unnamed_connection,
        ] {
            assert_eq!(
                decide_for(&waived, &route).expect_err("refused"),
                Refusal::WAIVER_ROUTE_MISMATCH,
                "{route:?}"
            );
        }
        let explicit = Route::named(Some(PROVIDER_ID), Some("conn-1"), true);
        assert_eq!(
            decide_for(&waived, &explicit).expect_err("refused"),
            Refusal::WAIVER_ROUTE_MISMATCH,
            "an explicit request is never waived"
        );
    }

    #[test]
    fn a_waiver_that_does_not_fit_the_token_is_a_posture_mismatch() {
        let route = bound();
        let cases = [
            (
                "zdr true",
                token(true, Some(true), Some(waiver(1, PROVIDER_ID, "conn-1"))),
            ),
            (
                "no org claim",
                token(false, None, Some(waiver(1, PROVIDER_ID, "conn-1"))),
            ),
            (
                "org off",
                token(false, Some(false), Some(waiver(1, PROVIDER_ID, "conn-1"))),
            ),
            (
                "unknown version",
                token(false, Some(true), Some(waiver(2, PROVIDER_ID, "conn-1"))),
            ),
        ];
        for (name, claims) in cases {
            assert_eq!(
                decide_for(&claims, &route).expect_err("refused"),
                Refusal::POSTURE_MISMATCH,
                "{name}"
            );
        }
    }

    #[test]
    fn an_org_zdr_token_without_a_waiver_is_never_served_persistent() {
        let persistent_for_zdr_org = token(false, Some(true), None);
        assert_eq!(
            decide_for(&persistent_for_zdr_org, &Route::plain(false)).expect_err("refused"),
            Refusal::POSTURE_MISMATCH
        );
    }

    #[test]
    fn bearers_that_disagree_about_posture_or_waiver_are_a_posture_mismatch() {
        let zdr = token(true, Some(true), None);
        let persistent = token(false, Some(false), None);
        let waived_a = token(false, Some(true), Some(waiver(1, PROVIDER_ID, "conn-1")));
        let waived_b = token(false, Some(true), Some(waiver(1, PROVIDER_ID, "conn-2")));
        let mixed = [(&zdr, &persistent), (&persistent, &zdr), (&waived_a, &zdr)];
        for (first, second) in mixed {
            let refused = decide(&[first, second], &bound()).expect_err("refused");
            assert_eq!(refused, Refusal::POSTURE_MISMATCH);
        }
        assert_eq!(
            decide(&[&waived_a, &waived_b], &bound()).expect_err("refused"),
            Refusal::POSTURE_MISMATCH,
            "waivers for different connections are not one posture"
        );
        assert_eq!(
            decide(&[&waived_a, &waived_a.clone()], &bound())
                .expect("equal waived bearers are one posture"),
            TurnPosture::Waived {
                connection: "conn-1".to_owned(),
                reason: Reason::ConnectionNotZdrCapable,
            }
        );
    }

    #[test]
    fn the_statuses_and_grpc_codes_follow_the_credential_and_route_split() {
        assert_eq!(
            http_status(&Refusal::WAIVER_ROUTE_MISMATCH),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            http_status(&Refusal::POSTURE_MISMATCH),
            StatusCode::FORBIDDEN
        );
        for refusal in [
            Refusal::CONNECTION_NOT_ACKNOWLEDGED,
            Refusal::CONNECTION_NOT_SUPPORTED,
            Refusal::UNAVAILABLE,
            Refusal::DECISION_UNAVAILABLE,
        ] {
            assert_eq!(
                http_status(&refusal),
                StatusCode::PRECONDITION_FAILED,
                "{}",
                refusal.code()
            );
        }
        assert_eq!(
            grpc_code(&Refusal::WAIVER_ROUTE_MISMATCH),
            Code::PermissionDenied
        );
        assert_eq!(
            grpc_code(&Refusal::POSTURE_MISMATCH),
            Code::PermissionDenied
        );
        assert_eq!(grpc_code(&Refusal::UNAVAILABLE), Code::FailedPrecondition);
        let (status, Json(body)) = http(&Refusal::WAIVER_ROUTE_MISMATCH);
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "zdr_waiver_route_mismatch");
    }

    #[test]
    fn a_turn_that_is_both_zdr_and_waived_cannot_be_built_from_the_resolver() {
        // The resolver never answers zdr for a waiver (a waiver needs a persistent token), so the
        // posture type has no shape for it. The mode a waived turn takes is the filtering one.
        let zdr = TurnPosture::Zdr {
            reason: Reason::OrgZdr,
        };
        let waived = TurnPosture::Waived {
            connection: "conn-1".to_owned(),
            reason: Reason::ConnectionNotZdrCapable,
        };
        assert_eq!(zdr.data_plane_mode().as_deref(), Some("ephemeral"));
        assert_eq!(waived.data_plane_mode().as_deref(), Some("reject"));
    }

    #[test]
    fn every_content_route_feature_is_in_the_catalogue() {
        for feature in [
            "translation",
            "document_analysis",
            "language_analysis",
            "realtime_voice",
            "video_generation",
        ] {
            assert!(
                mp_contracts::zdr_refusal::feature(feature).is_some(),
                "{feature} is not a catalogue feature"
            );
        }
    }

    #[test]
    fn a_zdr_token_on_a_content_route_is_refused_with_the_operations_feature() {
        let (status, Json(body)) =
            decide_content_route(&token(true, Some(true), None), None, "translation")
                .expect_err("refused");
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(body["error"]["code"], "zdr_feature_not_supported");
        assert_eq!(body["error"]["feature"], "translation");
        assert_eq!(body["error"]["action"], "turn_off_feature");
    }

    #[test]
    fn a_persistent_token_on_a_content_route_is_served_as_before() {
        assert!(decide_content_route(
            &token(false, Some(false), None),
            Some("azure-openai"),
            "document_analysis"
        )
        .is_ok());
        assert!(decide_content_route(&token(false, None, None), None, "language_analysis").is_ok());
    }

    #[test]
    fn a_waiver_on_a_content_route_is_a_route_mismatch_for_every_spelling() {
        let waived = token(false, Some(true), Some(waiver(1, PROVIDER_ID, "conn-1")));
        for provider in [None, Some(""), Some("azure-openai"), Some(PROVIDER_ID)] {
            let (status, Json(body)) =
                decide_content_route(&waived, provider, "realtime_voice").expect_err("refused");
            assert_eq!(status, StatusCode::FORBIDDEN, "{provider:?}");
            assert_eq!(
                body["error"]["code"], "zdr_waiver_route_mismatch",
                "{provider:?}"
            );
        }
    }

    #[test]
    fn a_zdr_token_naming_the_subscription_route_on_a_content_route_is_not_acknowledged() {
        let (status, Json(body)) = decide_content_route(
            &token(true, Some(true), None),
            Some(PROVIDER_ID),
            "translation",
        )
        .expect_err("refused");
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(body["error"]["code"], "zdr_connection_not_acknowledged");
    }

    #[test]
    fn an_org_zdr_token_without_a_waiver_is_a_posture_mismatch_on_a_content_route() {
        let (status, Json(body)) =
            decide_content_route(&token(false, Some(true), None), None, "video_generation")
                .expect_err("refused");
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "zdr_posture_mismatch");
    }

    /// The source scan. A raw read of a token's retention claims is allowed only where it is named
    /// here, and each allowed site says why. Every decision reads the posture through `decide`.
    #[test]
    fn no_raw_retention_claim_is_read_outside_the_named_inventory() {
        // (file, function, why the raw read is not a decision)
        const RAW_READ_INVENTORY: &[(&str, &str, &str)] = &[
            (
                "zdr_posture.rs",
                "claims_view",
                "the one adapter from a verified token to the resolver's input",
            ),
            (
                "auth.rs",
                "verify_delegated_user_bearer",
                "posture equality across the user bearers of one request, not a decision",
            ),
            (
                "grpc_auth.rs",
                "verify_delegated_bearer",
                "posture equality across the user bearers of one gRPC request, not a decision",
            ),
            (
                "grpc_auth.rs",
                "validate_identity",
                "copies the verified token's retention claims into the identity the resolver reads",
            ),
            (
                "grpc_auth.rs",
                "fmt",
                "the identity's Debug text (and the field's declaration), not a decision",
            ),
            (
                "grpc_auth.rs",
                "posture_view",
                "the adapter from a gRPC identity to the resolver's input",
            ),
        ];
        const FORBIDDEN: &[&str] = &[
            "effective_zdr(",
            "claims.zdr",
            "claims.org_zdr",
            "claims.zdr_waiver",
            "model_claims.zdr",
            "issuer_zdr",
        ];
        let mut found = Vec::new();
        for (file, function, line) in production_lines_mentioning(FORBIDDEN) {
            let allowed = RAW_READ_INVENTORY
                .iter()
                .any(|(allowed_file, allowed_function, _)| {
                    *allowed_file == file && *allowed_function == function
                });
            if !allowed {
                found.push(format!("{file}::{function}: {line}"));
            }
        }
        assert!(
            found.is_empty(),
            "raw retention claims are read outside the named inventory; route the decision through zdr_posture::decide:\n{}",
            found.join("\n")
        );
    }

    /// Every entry point that decides a turn's or a request's posture must do it through
    /// `zdr_posture`. Named here so a new entry point is added on purpose.
    #[test]
    fn every_named_entry_point_decides_through_zdr_posture() {
        const ENTRY_POINTS: &[(&str, &str)] = &[
            ("sse.rs", "invoke_stream_sse"),
            ("http_routes.rs", "invoke"),
            ("http_routes.rs", "ai_chat"),
            ("http_routes.rs", "recommend_plan"),
            ("http_routes.rs", "ai_embeddings"),
            ("http_routes.rs", "ai_dictate"),
            ("http_routes.rs", "ai_speech"),
            ("http_routes.rs", "ai_documents"),
            ("http_routes.rs", "create_document"),
            ("http_routes.rs", "require_non_zdr_durable_mutation"),
            ("http_routes.rs", "browser_visual_summary"),
            ("http_routes.rs", "extract_image_text"),
            ("http_routes.rs", "analyze_image"),
            ("http_routes.rs", "browser_suggest_action"),
            ("http_routes.rs", "generate_image"),
            ("http_routes.rs", "ai_translate"),
            ("http_routes.rs", "ai_translate_detect"),
            ("http_routes.rs", "analyze_document_with_model"),
            ("http_routes.rs", "analyze_language_with_operation"),
            ("http_routes.rs", "ai_realtime"),
            ("http_routes.rs", "ai_video_generate"),
            ("dataplane.rs", "reject_zdr_durable_mutation"),
            ("dataplane.rs", "retrieve"),
            ("finetune_routes.rs", "require_non_zdr_finetune_mutation"),
            ("finetune_routes.rs", "get_job"),
            ("browser_run.rs", "browser_run_zdr_blocked"),
            ("grpc_auth.rs", "posture"),
            ("grpc_auth.rs", "authorize_request"),
            ("grpc.rs", "require_explicit_zdr_contract"),
            ("grpc.rs", "invoke"),
            ("grpc.rs", "invoke_stream"),
            ("grpc.rs", "web_search"),
            ("grpc.rs", "fetch"),
            ("grpc.rs", "extract_structured"),
            ("grpc.rs", "text_to_speech"),
            ("grpc.rs", "speech_to_text"),
        ];
        let mut missing = Vec::new();
        for (file, function) in ENTRY_POINTS {
            let body = function_body(file, function);
            match body {
                None => missing.push(format!("{file}::{function} is not found")),
                Some(body) if !body.contains("zdr_posture::") => {
                    missing.push(format!("{file}::{function} does not call zdr_posture"));
                }
                Some(_) => {}
            }
        }
        assert!(missing.is_empty(), "{}", missing.join("\n"));
    }

    /// Every durable mutation is guarded before it runs. A mutation that starts or advances stored work
    /// (a continuation) refuses a ZDR token through `continuation_gate` and checks a waiver against the
    /// stored receipt in Session Core (owner decision 17). Every other durable mutation refuses a ZDR
    /// token and a waived token through `require_non_zdr_durable_mutation`. Deletes and archives are
    /// not in this list: they are allowed under every posture. Reads of durable state are not guarded
    /// here: they send nothing to a processor.
    #[test]
    fn every_durable_mutation_refuses_a_zdr_or_waived_token_before_it_runs() {
        const DURABLE_MUTATIONS: &[(&str, &str)] = &[
            ("http_routes.rs", "create_memory"),
            ("http_routes.rs", "invoke_queue_input"),
            ("http_routes.rs", "plan_approval"),
            ("http_routes.rs", "create_document"),
            ("http_routes.rs", "proxy_to_capability_core"),
            ("http_routes.rs", "approve_plan"),
            ("http_routes.rs", "reject_plan"),
            ("http_routes.rs", "update_todo_status"),
            ("http_routes.rs", "decide_approval"),
            ("http_routes.rs", "resume_run"),
            // `/continue` consumes a granted approval and starts a segment of a stored
            // run (build plan WS03 S3): the same continuation gate, then a stricter
            // persistent-only check of its own in `continue_preflight`.
            ("sse.rs", "continue_run"),
            ("http_routes.rs", "cancel_run"),
            ("http_routes.rs", "ingest_feedback"),
            // Question answers are stored user content (WS14-S3); the list route is
            // gated too, since a waived run never owns a question.
            ("http_routes.rs", "list_run_questions"),
            ("http_routes.rs", "answer_run_question"),
            // Human posts in a Space room (WS01-S4) store free text under a member's name.
            ("space_routes/posts.rs", "create_space_post"),
            ("space_routes/posts.rs", "append_space_post"),
        ];
        let mut missing = Vec::new();
        for (file, function) in DURABLE_MUTATIONS {
            match function_body(file, function) {
                None => missing.push(format!("{file}::{function} is not found")),
                Some(body)
                    if !(body.contains("require_non_zdr_durable_mutation(")
                        || body.contains("continuation_gate(")
                        || body.contains("zdr_or_refuse(")) =>
                {
                    missing.push(format!(
                        "{file}::{function} does not refuse a ZDR or waived token before it runs"
                    ));
                }
                Some(_) => {}
            }
        }
        // `continuation_gate` lets a waiver through (a cold approval's decision must
        // still work), but `/continue` resumes stored state and a waived run keeps
        // none. Its handler therefore also calls the persistent-only helper, whose
        // behaviour the handler-boundary tests in `sse.rs` pin with a waived token:
        // a handler that stopped calling it would pass every one of those tests.
        match function_body("sse.rs", "continue_run") {
            None => missing.push("sse.rs::continue_run is not found".to_owned()),
            Some(body) if !body.contains("continue_persistent_gate(") => missing.push(
                "sse.rs::continue_run does not call continue_persistent_gate, so a waived \
                 token is not refused before any work"
                    .to_owned(),
            ),
            Some(_) => {}
        }
        assert!(missing.is_empty(), "{}", missing.join("\n"));
    }

    /// Every production call into inference-core, named with the route it carries. A call site that
    /// is not named here fails until someone classifies it: a call on a turn carries that turn's
    /// decided connection (or inherits it), a call a waived token may not make is refused before it
    /// is made, and no call sends a token-bearing turn's content to the platform chain unannounced.
    #[test]
    fn every_inference_call_site_is_named_with_the_route_it_carries() {
        // (file, function, the route the call carries)
        const CALL_SITES: &[(&str, &str, &str)] = &[
            // The turn's own answer, and the calls it makes on the turn's route.
            ("sse.rs", "invoke_stream_sse", "the turn's decided route"),
            (
                "sse.rs",
                "run_infer_fallback",
                "the turn's decided route (the request it was sent)",
            ),
            (
                "sse.rs",
                "summarize_head",
                "the turn's route, through direct_infer",
            ),
            (
                "sse.rs",
                "direct_infer",
                "the caller's route (the turn's, or the ZDR stream's)",
            ),
            ("sse.rs", "direct_infer_turn", "the caller's route"),
            (
                "sse.rs",
                "zdr_direct_stream",
                "the ZDR turn's route (a ZDR posture only)",
            ),
            (
                "sse.rs",
                "judge_sources",
                "the turn's route, inherited (subscription_scoped_request)",
            ),
            (
                "sse.rs",
                "resample_answer",
                "the turn's route, inherited (subscription_scoped_request)",
            ),
            (
                "sse.rs",
                "generate_thread_title",
                "the turn's route, inherited (subscription_scoped_request)",
            ),
            (
                "sse.rs",
                "generate_follow_ups",
                "the turn's route, inherited (subscription_scoped_request)",
            ),
            (
                "sse.rs",
                "image_gen_stream",
                "the default chain; refused on a waived turn at the stream entry",
            ),
            (
                "sse.rs",
                "vision_stream",
                "the default chain; refused on a waived turn at the stream entry (a persistent branch only)",
            ),
            (
                "tool_loop.rs",
                "run_tool_rounds_for_model_with_attachments",
                "the turn's route (provider hint and connection)",
            ),
            (
                "result_validation.rs",
                "infer_candidate",
                "the route of the request its caller built",
            ),
            (
                "deep_research.rs",
                "plan_once",
                "refused on a waived turn at the stream entry",
            ),
            (
                "deep_research.rs",
                "synthesize_report",
                "refused on a waived turn at the stream entry",
            ),
            // The gRPC surface, each decided by zdr_for before its call.
            ("grpc.rs", "invoke", "the request's route (zdr_for)"),
            ("grpc.rs", "invoke_stream", "the request's route (zdr_for)"),
            (
                "grpc.rs",
                "extract_structured",
                "the request's route (zdr_for)",
            ),
            (
                "grpc.rs",
                "text_to_speech",
                "speech, no connection (zdr_for)",
            ),
            (
                "grpc.rs",
                "speech_to_text",
                "speech, no connection (zdr_for)",
            ),
            // The HTTP surface, each decided by zdr_or_refuse or decide_for before its call.
            (
                "http_routes.rs",
                "invoke",
                "the request's route (decide_for)",
            ),
            (
                "http_routes.rs",
                "ai_chat",
                "the request's route (zdr_or_refuse)",
            ),
            (
                "http_routes.rs",
                "recommend_plan",
                "no route (zdr_or_refuse)",
            ),
            (
                "http_routes.rs",
                "ai_embeddings",
                "the request's provider (zdr_or_refuse)",
            ),
            ("http_routes.rs", "ai_dictate", "no route (zdr_or_refuse)"),
            (
                "http_routes.rs",
                "ai_speech",
                "the request's provider (zdr_or_refuse)",
            ),
            (
                "http_routes.rs",
                "browser_visual_summary",
                "the request's provider (zdr_or_refuse)",
            ),
            (
                "http_routes.rs",
                "browser_suggest_action",
                "the request's provider (zdr_or_refuse)",
            ),
            (
                "http_routes.rs",
                "extract_image_text",
                "the request's provider (zdr_or_refuse)",
            ),
            (
                "http_routes.rs",
                "analyze_image",
                "refused under ZDR and on a waived token (zdr_or_refuse)",
            ),
            (
                "http_routes.rs",
                "generate_image",
                "refused under ZDR and on a waived token (zdr_or_refuse)",
            ),
            // The content routes: each decides its request's posture before any call
            // (`decide_content_route`), and names the feature a ZDR turn is refused for.
            (
                "http_routes.rs",
                "ai_translate",
                "the request's provider (decide_content_route, translation)",
            ),
            (
                "http_routes.rs",
                "batch_translate",
                "reached from ai_translate, which decides first (translation)",
            ),
            (
                "http_routes.rs",
                "detect_text_language",
                "reached from ai_translate and ai_translate_detect, both decide first (translation)",
            ),
            (
                "http_routes.rs",
                "analyze_document_with_model",
                "the request's provider (decide_content_route, document_analysis)",
            ),
            (
                "http_routes.rs",
                "analyze_language_with_operation",
                "the request's provider (decide_content_route, language_analysis)",
            ),
            (
                "http_routes.rs",
                "ai_realtime",
                "the request's provider (decide_content_route, realtime_voice)",
            ),
            (
                "http_routes.rs",
                "ai_video_generate",
                "the request's provider (decide_content_route, video_generation)",
            ),
            // Catalogue and status reads: no user content goes to a processor.
            (
                "http_routes.rs",
                "list_translation_languages",
                "catalogue read: no user content",
            ),
            ("http_routes.rs", "ai_models", "catalogue read: no user content"),
            ("http_routes.rs", "list_models", "catalogue read: no user content"),
            (
                "http_routes.rs",
                "ai_realtime_models",
                "catalogue read: no user content",
            ),
            (
                "http_routes.rs",
                "ai_video_models",
                "catalogue read: no user content",
            ),
            (
                "http_routes.rs",
                "ai_video_job",
                "job status read: no user content",
            ),
            (
                "http_routes.rs",
                "ai_video_content",
                "generated media read by generation id: no user content",
            ),
            (
                "http_routes.rs",
                "ai_speech_voices",
                "voice catalogue read: no user content",
            ),
            // A derived content-safety pass over tool payloads. It runs only on a persistent turn:
            // `dispatch_tool_with_cost` passes `zdr || data_plane_mode.is_some()` to
            // `assess_external_payload`, so a ZDR or waived turn sends nothing to a processor here.
            (
                "semantic_screening.rs",
                "classify",
                "derived content-safety pass, skipped on a non-persistent turn (assess_external_payload)",
            ),
        ];
        const INFERENCE_CALLS: &[&str] = &[
            ".infer(",
            ".infer_stream(",
            ".create_embedding(",
            ".generate_image(",
            ".synthesize_speech(",
            ".transcribe_speech(",
            ".analyze_image(",
            ".extract_image_text(",
            "direct_infer_turn(",
            "direct_infer(",
            ".translate_text(",
            ".batch_translate_text(",
            ".detect_text_language(",
            ".list_translation_languages(",
            ".analyze_document(",
            ".analyze_language(",
            ".create_realtime_session(",
            ".create_video_generation_job(",
            ".get_video_generation_job(",
            ".list_models(",
            ".list_speech_voices(",
            ".stream_video_generation_content(",
        ];
        let mut unnamed = Vec::new();
        for (file, text) in read_sources() {
            let stripped = strip_test_items(&text);
            let mut function = String::new();
            for line in stripped.lines() {
                if let Some(name) = declared_function(line) {
                    function = name;
                }
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") || trimmed.contains("fn ") {
                    continue;
                }
                if INFERENCE_CALLS.iter().any(|call| line.contains(call))
                    && !CALL_SITES.iter().any(|(named_file, named_fn, _)| {
                        *named_file == file && *named_fn == function
                    })
                {
                    unnamed.push(format!("{file}::{function}: {trimmed}"));
                }
            }
        }
        assert!(
            unnamed.is_empty(),
            "an inference call is not named with the route it carries:\n{}",
            unnamed.join("\n")
        );
    }

    fn source_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    fn read_sources() -> Vec<(String, String)> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(source_dir()).expect("the source directory is readable") {
            let path = entry.expect("a directory entry").path();
            if path.extension().is_some_and(|ext| ext == "rs") {
                let name = path
                    .file_name()
                    .expect("a file name")
                    .to_string_lossy()
                    .into_owned();
                let text = std::fs::read_to_string(&path).expect("a source file is readable");
                files.push((name, text));
            }
        }
        // Space room routes live one level down and are named by subpath, so a function in
        // `space_routes/` is found by the same scans as a top-level one (WS01-S4, K2-04).
        let space_routes = source_dir().join("space_routes");
        if let Ok(entries) = std::fs::read_dir(&space_routes) {
            for entry in entries {
                let path = entry.expect("a directory entry").path();
                if path.extension().is_some_and(|ext| ext == "rs") {
                    let name = format!(
                        "space_routes/{}",
                        path.file_name().expect("a file name").to_string_lossy()
                    );
                    let text = std::fs::read_to_string(&path).expect("a source file is readable");
                    files.push((name, text));
                }
            }
        }
        files.sort();
        files
    }

    /// Each production line that mentions a forbidden token, with the function it sits in.
    fn production_lines_mentioning(forbidden: &[&str]) -> Vec<(String, String, String)> {
        let mut hits = Vec::new();
        for (file, text) in read_sources() {
            let stripped = strip_test_items(&text);
            let mut function = String::new();
            for line in stripped.lines() {
                if let Some(name) = declared_function(line) {
                    function = name;
                }
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                if forbidden.iter().any(|token| line.contains(token)) {
                    hits.push((file.clone(), function.clone(), trimmed.to_owned()));
                }
            }
        }
        hits
    }

    /// The production body of a function, from its declaration to the line that closes it at the
    /// same indentation.
    fn function_body(file: &str, function: &str) -> Option<String> {
        let (_, text) = read_sources().into_iter().find(|(name, _)| name == file)?;
        let stripped = strip_test_items(&text);
        let lines: Vec<&str> = stripped.lines().collect();
        let start = lines
            .iter()
            .position(|line| declared_function(line).as_deref() == Some(function))?;
        let indent: String = lines[start].chars().take_while(|c| *c == ' ').collect();
        let closing = format!("{indent}}}");
        let end = lines[start + 1..]
            .iter()
            .position(|line| *line == closing)
            .map_or(lines.len(), |offset| start + 1 + offset);
        Some(lines[start..end].join("\n"))
    }

    /// The name a line declares as a function, if it declares one.
    fn declared_function(line: &str) -> Option<String> {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            return None;
        }
        let index = trimmed.find("fn ")?;
        let before_ok =
            index == 0 || trimmed[..index].ends_with(' ') || trimmed[..index].ends_with('(');
        if !before_ok {
            return None;
        }
        let name: String = trimmed[index + 3..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        (!name.is_empty()).then_some(name)
    }

    /// The source with every `#[cfg(test)]` item blanked out, newlines kept.
    fn strip_test_items(source: &str) -> String {
        const MARK: &str = "#[cfg(test)]";
        let mut out = String::with_capacity(source.len());
        let mut rest = source;
        while let Some(index) = rest.find(MARK) {
            out.push_str(&rest[..index]);
            let after = &rest[index + MARK.len()..];
            let end = match after.find('{') {
                Some(open) => {
                    let mut depth = 0_usize;
                    let mut close = after.len();
                    for (position, character) in after[open..].char_indices() {
                        match character {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    close = open + position + 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    close
                }
                None => after.len(),
            };
            let blanked = &rest[index..index + MARK.len() + end];
            out.extend(blanked.chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
            rest = &rest[index + MARK.len() + end..];
        }
        out.push_str(rest);
        out
    }
}
