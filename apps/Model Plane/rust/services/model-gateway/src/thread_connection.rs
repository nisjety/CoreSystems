//! The connection a side feature on one thread is served on (W4-BYO, DEC-V-R8).
//!
//! A thread whose turns were written under a connection is answered on that connection and never on the platform
//! chain. The connection is the stored `messages.model_connection_id` (one of the caller's customer-gateway
//! connections) or `messages.subscription_connection_id` (the caller's ChatGPT subscription), read from Session Core
//! over the thread's newest turns. A thread whose turns name no connection is served on the platform chain as before.
//! A thread whose turns name two or more connections, of either kind, has no one route that can serve its whole
//! transcript, so it is refused. With `MODEL_GATEWAY_BYO_GATEWAY` not the literal `true` no turn names a connection.
//!
//! Side chat (`side_chat_routes`), reasoning translation, the model council and the review run share this resolution
//! and its typed refusals. A call served on a connection names that connection and the model the request gives, and it
//! carries no platform model or role fallback, so no platform chain can serve it. A zero-retention token or request is
//! refused on a connection, as a chat turn is, and a waiver binds one subscription connection only. A thread that a
//! request names is first checked as side chat checks it: a Space thread, and a thread holding a zero-retention or
//! waived run, are refused before the transcript is read (`refuse_unavailable_thread`, DEC-06, DEC-32).

use axum::{http::StatusCode, Json};
use mp_contracts::agentic_subscription::PROVIDER_ID;
use mp_contracts::model_plane::v1::{
    ArchiveFilter, InferRequest, ListConversationRequest, ListThreadsRequest, SessionMessage,
};
use mp_contracts::zdr_refusal::Refusal;
use serde_json::json;

use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::{authenticated_session_request, session_thread_error, HttpJsonError};
use crate::pricing::CostBasis;
use crate::state::AppState;

/// The provider a customer-gateway call names, as `normalize` names it for a customer-gateway turn.
pub(crate) const CUSTOMER_GATEWAY_PROVIDER: &str = "customer-gateway";
/// The newest turns the connection read asks Session Core for. A side feature reads the connection of the newest turns.
pub(crate) const READ_TURNS: u32 = 40;

fn typed(status: StatusCode, code: &str, message: &str) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": {} } })),
    )
}

/// A connection a thread's turns were written under: one of the caller's customer-gateway connections (W4-BYO,
/// `messages.model_connection_id`), or the caller's own ChatGPT subscription connection (DEC-V-R8,
/// `messages.subscription_connection_id`). Both are opaque Integration Core ids, never keys.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ThreadConnection {
    CustomerGateway(String),
    Subscription(String),
}

impl ThreadConnection {
    /// The provider the call names for this connection: the customer-gateway registry id, or the subscription one.
    pub(crate) fn provider(&self) -> &'static str {
        match self {
            Self::CustomerGateway(_) => CUSTOMER_GATEWAY_PROVIDER,
            Self::Subscription(_) => PROVIDER_ID,
        }
    }

    /// The opaque connection id.
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::CustomerGateway(id) | Self::Subscription(id) => id,
        }
    }

    /// The cost basis of a call served on this connection, before the serving provider is known. A customer gateway
    /// bills its own calls, and a subscription is the user's own account, so neither carries a platform price.
    pub(crate) fn basis(&self) -> CostBasis {
        match self {
            Self::CustomerGateway(id) => CostBasis::of_gateway(id),
            Self::Subscription(id) => CostBasis::of_route(PROVIDER_ID, id),
        }
    }

    /// Whether a call on this connection can return a structured reply, which a feature's output schema asks for. A
    /// customer gateway returns none (inference-core refuses a schema on one), and a subscription returns one when the
    /// call offers no tools.
    pub(crate) fn returns_structured_output(&self) -> bool {
        matches!(self, Self::Subscription(_))
    }

    /// Names this connection and `model` on `request`: the connection of its kind, the provider the call names for it,
    /// and no platform model or role fallback, so no platform chain serves the call.
    pub(crate) fn apply_to(&self, model: &str, request: &mut InferRequest) {
        request.model = model.to_owned();
        request.role_fallback_model = String::new();
        request.provider_hint = self.provider().to_owned();
        request.model_connection_id = match self {
            Self::CustomerGateway(id) => id.clone(),
            Self::Subscription(_) => String::new(),
        };
        request.subscription_connection_id = match self {
            Self::Subscription(id) => id.clone(),
            Self::CustomerGateway(_) => String::new(),
        };
    }
}

/// The connection a thread's turns were written under, read over the turns the side feature reads. `None` when the
/// switch is off, or when no turn names one: a platform thread, served as before. A thread whose turns name two or
/// more connections, of either kind, has no one connection that can serve its whole transcript, so it is refused
/// rather than served on one of them or on the platform chain.
pub(crate) fn thread_connection(
    byo_enabled: bool,
    messages: &[SessionMessage],
) -> Result<Option<ThreadConnection>, HttpJsonError> {
    if !byo_enabled {
        return Ok(None);
    }
    let mut named: Vec<ThreadConnection> = Vec::new();
    for message in messages {
        let gateway = message.model_connection_id.trim();
        if !gateway.is_empty() {
            named.push(ThreadConnection::CustomerGateway(gateway.to_owned()));
        }
        let subscription = message.subscription_connection_id.trim();
        if !subscription.is_empty() {
            named.push(ThreadConnection::Subscription(subscription.to_owned()));
        }
    }
    named.sort_unstable();
    named.dedup();
    match named.len() {
        0 => Ok(None),
        1 => Ok(named.pop()),
        _ => Err(typed(
            StatusCode::CONFLICT,
            "connection_ambiguous",
            "side chat is not available for a thread whose turns were written under more than one connection",
        )),
    }
}

/// The posture of a call served on a connection. The connection route decides it, as it decides a chat turn that names
/// the connection: a zero-retention token or an explicit zero-retention request is refused typed before any call,
/// because neither a customer gateway nor a subscription is zero retention. A waiver binds one subscription
/// connection, so on any other connection it is refused as a route mismatch.
pub(crate) fn connection_posture(
    claims: &Claims,
    connection: &ThreadConnection,
    explicit_zdr: bool,
) -> Result<(), HttpJsonError> {
    let zdr = crate::zdr_posture::zdr_or_refuse(
        claims,
        &crate::zdr_posture::Route::named(
            Some(connection.provider()),
            Some(connection.id()),
            explicit_zdr,
        ),
    )?;
    if zdr {
        // The resolver refuses every zero-retention request on a connection route, so this is not reached. It is
        // kept so that a change there fails closed here.
        return Err(crate::zdr_posture::http(
            &Refusal::CONNECTION_NOT_ACKNOWLEDGED,
        ));
    }
    Ok(())
}

/// Where the one call of a side feature is served (W4-BYO, DEC-V-R8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SideRoute {
    /// The platform chain, as a side feature always was: the call names neither a model nor a connection.
    Platform,
    /// The thread's own connection, with the model the request names for it. A customer gateway or subscription call
    /// names its own model, and inference-core serves it on that connection alone.
    Connection {
        connection: ThreadConnection,
        model: String,
    },
}

impl SideRoute {
    /// Puts the route on `request`. A connection route names its connection and model (see
    /// [`ThreadConnection::apply_to`]); the platform route leaves the call as it was built.
    pub(crate) fn apply_to(&self, request: &mut InferRequest) {
        if let Self::Connection { connection, model } = self {
            connection.apply_to(model, request);
        }
    }
}

/// The route a side feature's one call is served on. A platform thread is served on the platform chain. A thread on a
/// connection is served on that connection with the model the request names: a customer gateway or subscription call
/// names its own model, so a request that names none is refused, and so is a model the deployment allowlist leaves
/// out, as on every turn.
pub(crate) fn side_route(
    connection: Option<ThreadConnection>,
    model: &str,
    allowed: Option<&[String]>,
) -> Result<SideRoute, HttpJsonError> {
    let Some(connection) = connection else {
        return Ok(SideRoute::Platform);
    };
    let model = model.trim();
    if model.is_empty() {
        return Err(typed(
            StatusCode::BAD_REQUEST,
            "side_chat_model_required",
            "a side chat on a customer gateway or subscription thread names the model the connection serves",
        ));
    }
    if let Some(refusal) = crate::normalize::deployment_allowlist_refusal(allowed, model) {
        return Err(refusal);
    }
    Ok(SideRoute::Connection {
        connection,
        model: model.to_owned(),
    })
}

/// The cost basis of an answer. A platform answer is priced as it always was. A customer-gateway answer is billed by the
/// gateway, and a subscription answer is the user's own subscription, so neither carries a platform price. A call the
/// platform served is never labelled as either.
pub(crate) fn answer_basis(route: &SideRoute, provider_used: &str) -> CostBasis {
    let basis = match route {
        SideRoute::Platform => CostBasis::of_route("", ""),
        SideRoute::Connection { connection, .. } => connection.basis(),
    };
    basis.served_by(provider_used)
}

/// The caller's delegated session credential, which Session Core needs to read the thread. A side feature that names a
/// thread on a connection cannot learn that connection without it, so the request is refused before any call (401,
/// as the teach draft refuses a missing quarry credential).
pub(crate) fn require_session_bearer(
    bearer: Option<&VerifiedSessionBearer>,
) -> Result<&VerifiedSessionBearer, HttpJsonError> {
    bearer.ok_or_else(|| {
        typed(
            StatusCode::UNAUTHORIZED,
            "delegated_credential_required",
            "a side feature on a thread needs your delegated session credential",
        )
    })
}

/// The refusal a feature gets when its call asks for a structured reply and the thread's connection cannot return one
/// (W4-BYO). It is typed and made before any model call, as a chat turn refuses a feature its connection does not
/// support (`feature_not_supported`), so a side feature never runs part of itself on the connection and fails in the
/// middle. `feature` names the feature in the message.
pub(crate) fn refuse_without_structured_output(
    connection: &ThreadConnection,
    feature: &str,
) -> Result<(), HttpJsonError> {
    if connection.returns_structured_output() {
        return Ok(());
    }
    Err(typed(
        StatusCode::BAD_REQUEST,
        "feature_not_supported",
        &format!(
            "{feature} needs a structured reply, and a customer gateway connection does not return one"
        ),
    ))
}

/// The newest turns of the thread's active path, and whether older ones exist. Human posts stay out, so the model reads
/// the conversation's turns and never a post a member wrote in a room. Session Core checks that the caller owns the
/// thread.
pub(crate) async fn read_active_path(
    state: &AppState,
    claims: &Claims,
    session_bearer: &VerifiedSessionBearer,
    thread_id: &str,
    limit: u32,
) -> Result<(Vec<SessionMessage>, bool), HttpJsonError> {
    let response = state
        .session_client
        .clone()
        .list_conversation(authenticated_session_request(
            ListConversationRequest {
                org_id: claims.org_id.clone(),
                thread_id: thread_id.to_owned(),
                include_inactive: false,
                limit,
                include_human_posts: false,
                ..Default::default()
            },
            session_bearer,
        )?)
        .await
        .map_err(|e| session_thread_error("session-core list_conversation failed", &e))?
        .into_inner();
    Ok((response.messages, response.has_more))
}

/// Refuses a thread a side feature may not read, before its transcript is read (DEC-06, DEC-32). Session Core's
/// conversation read carries no Space id, so the thread's own summary is read by id, through the owner-bound thread
/// listing: a thread the caller does not own is not listed, and Session Core refuses its read. A Space thread is refused
/// with 409 `scope_not_available_in_space`, and a thread holding a zero-retention or waived run with 409
/// `side_chat_thread_retention_refused`; the Space check comes first. Side chat and every feature on a thread share this
/// refusal, and `feature` names the feature in its message.
pub(crate) async fn refuse_unavailable_thread(
    state: &AppState,
    claims: &Claims,
    session_bearer: &VerifiedSessionBearer,
    thread_id: &str,
    feature: &str,
) -> Result<(), HttpJsonError> {
    let response = state
        .session_client
        .clone()
        .list_threads(authenticated_session_request(
            ListThreadsRequest {
                org_id: claims.org_id.clone(),
                user_id: claims.user_id.clone(),
                limit: 1,
                thread_id: thread_id.to_owned(),
                archived: ArchiveFilter::All as i32,
                ..Default::default()
            },
            session_bearer,
        )?)
        .await
        .map_err(|e| session_thread_error("session-core list_threads failed", &e))?
        .into_inner();
    if response
        .threads
        .iter()
        .any(|thread| !thread.space_id.is_empty())
    {
        return Err(typed(
            StatusCode::CONFLICT,
            "scope_not_available_in_space",
            &format!("{feature} is not available in a Space thread"),
        ));
    }
    if response
        .threads
        .iter()
        .any(|thread| thread.holds_retention_run)
    {
        return Err(typed(
            StatusCode::CONFLICT,
            "side_chat_thread_retention_refused",
            &format!(
                "{feature} is not available for a thread that holds a zero-retention or waived run"
            ),
        ));
    }
    Ok(())
}

/// The connection a side feature on `thread_id` is served on, or `None` when it is served on the platform chain as
/// before: the switch is off, the request names no thread, or the thread's turns name no connection. A named thread is
/// refused as side chat refuses it (`refuse_unavailable_thread`) before its transcript is read. A thread that names a
/// connection is read through the caller's session credential, which a request without one cannot supply, so it is
/// refused typed. The posture is decided on that connection, as a chat turn's is (`connection_posture`). Nothing is
/// written. `feature` names the feature in a refusal's message.
pub(crate) async fn feature_connection(
    state: &AppState,
    claims: &Claims,
    session_bearer: Option<&VerifiedSessionBearer>,
    thread_id: &str,
    explicit_zdr: bool,
    feature: &str,
) -> Result<Option<ThreadConnection>, HttpJsonError> {
    feature_connection_with(
        crate::normalize::byo_gateway_enabled(),
        state,
        claims,
        session_bearer,
        thread_id,
        explicit_zdr,
        feature,
    )
    .await
}

/// [`feature_connection`] with the switch passed in, so a test decides it without the environment variable. With the
/// switch off, or a blank `thread_id`, the answer is `None` before the session credential is checked and before Session
/// Core is read: such a request is served exactly as it was before the feature.
pub(crate) async fn feature_connection_with(
    byo_enabled: bool,
    state: &AppState,
    claims: &Claims,
    session_bearer: Option<&VerifiedSessionBearer>,
    thread_id: &str,
    explicit_zdr: bool,
    feature: &str,
) -> Result<Option<ThreadConnection>, HttpJsonError> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() || !byo_enabled {
        return Ok(None);
    }
    let session_bearer = require_session_bearer(session_bearer)?;
    refuse_unavailable_thread(state, claims, session_bearer, thread_id, feature).await?;
    let (messages, _) =
        read_active_path(state, claims, session_bearer, thread_id, READ_TURNS).await?;
    let connection = thread_connection(true, &messages)?;
    if let Some(connection) = &connection {
        connection_posture(claims, connection, explicit_zdr)?;
    }
    Ok(connection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::ZdrWaiverClaim;
    use crate::http_routes::speech_route_tests::{state_with, SpeechInference};
    use mp_contracts::model_plane::v1::{
        session_core_client::SessionCoreClient, ListConversationResponse, ListThreadsResponse,
        ThreadSummary,
    };
    use mp_contracts::zdr_refusal::code;
    use std::future::Future;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll};
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::codegen::{http, Body, BoxFuture, Service, StdError};

    fn claims(zdr: bool, org_zdr: Option<bool>) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: "user-a".to_owned(),
            iss: "test-issuer".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr,
            sovereign: None,
            org_zdr,
            zdr_waiver: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    fn refusal_code(error: &HttpJsonError) -> &str {
        let Json(body) = &error.1;
        body["error"]["code"].as_str().unwrap_or_default()
    }

    fn refusal_message(error: &HttpJsonError) -> &str {
        let Json(body) = &error.1;
        body["error"]["message"].as_str().unwrap_or_default()
    }

    fn gateway_connection() -> ThreadConnection {
        ThreadConnection::CustomerGateway("conn-a".to_owned())
    }

    fn gateway_turn(connection: &str) -> SessionMessage {
        SessionMessage {
            role: "user".to_owned(),
            content: "Hei".to_owned(),
            model_connection_id: connection.to_owned(),
            ..Default::default()
        }
    }

    fn subscription_turn(connection: &str) -> SessionMessage {
        SessionMessage {
            role: "user".to_owned(),
            content: "Hei".to_owned(),
            subscription_connection_id: connection.to_owned(),
            ..Default::default()
        }
    }

    /// A request as a feature builds it before the route is put on it: the platform fields, with the connection fields
    /// empty.
    fn platform_call() -> InferRequest {
        InferRequest {
            request_id: "nonce-1-feature".to_owned(),
            model: "platform-model".to_owned(),
            role_fallback_model: "platform-fallback".to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn a_platform_thread_names_no_connection_and_the_call_is_left_as_built() {
        let messages = vec![SessionMessage {
            role: "user".to_owned(),
            content: "Hei".to_owned(),
            ..Default::default()
        }];
        let connection = thread_connection(true, &messages).expect("not refused");
        assert_eq!(connection, None);
        let route = side_route(connection, "", None).expect("a platform route needs no model");
        let mut request = platform_call();
        let before = request.clone();
        route.apply_to(&mut request);
        assert_eq!(request, before);
    }

    #[test]
    fn with_the_switch_off_no_thread_names_a_connection() {
        let messages = vec![gateway_turn("conn-a")];
        assert_eq!(
            thread_connection(false, &messages).expect("not refused"),
            None
        );
        let messages = vec![subscription_turn("conn_sub")];
        assert_eq!(
            thread_connection(false, &messages).expect("not refused"),
            None
        );
    }

    #[test]
    fn a_customer_gateway_call_names_only_its_connection_and_no_platform_route() {
        let route = side_route(
            Some(ThreadConnection::CustomerGateway("conn-a".to_owned())),
            " gpt-4o-mini ",
            None,
        )
        .expect("a named model is served");
        let mut request = platform_call();
        route.apply_to(&mut request);
        assert_eq!(request.model, "gpt-4o-mini");
        assert_eq!(request.model_connection_id, "conn-a");
        assert_eq!(request.provider_hint, CUSTOMER_GATEWAY_PROVIDER);
        assert!(request.subscription_connection_id.is_empty());
        assert!(request.role_fallback_model.is_empty());
    }

    #[test]
    fn a_subscription_call_names_only_its_subscription_and_no_platform_route() {
        let route = side_route(
            Some(ThreadConnection::Subscription("conn_sub".to_owned())),
            "gpt-4o-mini",
            None,
        )
        .expect("a named model is served");
        let mut request = platform_call();
        route.apply_to(&mut request);
        assert_eq!(request.model, "gpt-4o-mini");
        assert_eq!(request.subscription_connection_id, "conn_sub");
        assert_eq!(request.provider_hint, PROVIDER_ID);
        assert!(request.model_connection_id.is_empty());
        assert!(request.role_fallback_model.is_empty());
    }

    #[test]
    fn a_connection_call_is_billed_on_its_own_basis_and_a_platform_answer_is_not_relabelled() {
        let gateway = SideRoute::Connection {
            connection: ThreadConnection::CustomerGateway("conn-a".to_owned()),
            model: "m".to_owned(),
        };
        assert_eq!(
            answer_basis(&gateway, "customer-gateway.conn-a"),
            CostBasis::CustomerBilled
        );
        assert_eq!(answer_basis(&gateway, "azure-openai"), CostBasis::Metered);
        let subscription = SideRoute::Connection {
            connection: ThreadConnection::Subscription("conn_sub".to_owned()),
            model: "m".to_owned(),
        };
        assert_eq!(
            answer_basis(&subscription, PROVIDER_ID),
            CostBasis::Subscription
        );
        assert_eq!(
            answer_basis(&SideRoute::Platform, "azure-openai"),
            CostBasis::Metered
        );
    }

    #[test]
    fn a_feature_on_a_thread_without_a_session_credential_is_refused_typed_before_any_read() {
        let error = require_session_bearer(None).expect_err("no session credential, no read");
        assert_eq!(error.0, StatusCode::UNAUTHORIZED);
        assert_eq!(refusal_code(&error), "delegated_credential_required");
    }

    #[test]
    fn a_zero_retention_request_is_refused_typed_on_a_connection() {
        let refused = connection_posture(
            &claims(true, Some(true)),
            &ThreadConnection::CustomerGateway("conn-a".to_owned()),
            false,
        )
        .expect_err("a ZDR org cannot use a customer gateway");
        assert_eq!(refusal_code(&refused), code::CONNECTION_NOT_ACKNOWLEDGED);
        let refused = connection_posture(
            &claims(false, None),
            &ThreadConnection::Subscription("conn_sub".to_owned()),
            true,
        )
        .expect_err("an explicit ZDR request cannot use a subscription");
        assert_eq!(
            refusal_code(&refused),
            code::EXPLICIT_REQUEST_NOT_SUPPORTED_BY_CONNECTION
        );
    }

    #[test]
    fn a_waiver_for_another_route_is_a_route_mismatch_on_a_connection() {
        let waived = Claims {
            org_zdr: Some(true),
            zdr_waiver: Some(ZdrWaiverClaim {
                version: 1,
                provider: PROVIDER_ID.to_owned(),
                connection: "conn_sub".to_owned(),
            }),
            ..claims(false, None)
        };
        let refused = connection_posture(
            &waived,
            &ThreadConnection::CustomerGateway("conn-a".to_owned()),
            false,
        )
        .expect_err("a waiver names no customer gateway");
        assert_eq!(refusal_code(&refused), code::WAIVER_ROUTE_MISMATCH);
        assert!(connection_posture(
            &waived,
            &ThreadConnection::Subscription("conn_sub".to_owned()),
            false
        )
        .is_ok());
    }

    #[test]
    fn a_customer_gateway_is_refused_typed_for_a_feature_that_needs_a_structured_reply() {
        let error = refuse_without_structured_output(&gateway_connection(), "a review")
            .expect_err("a customer gateway returns no structured reply");
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(refusal_code(&error), "feature_not_supported");
        assert!(refusal_message(&error).starts_with("a review needs a structured reply"));
    }

    #[test]
    fn a_subscription_keeps_a_feature_that_needs_a_structured_reply() {
        let subscription = ThreadConnection::Subscription("conn_sub".to_owned());
        assert!(subscription.returns_structured_output());
        assert!(refuse_without_structured_output(&subscription, "a council").is_ok());
        assert!(!gateway_connection().returns_structured_output());
    }

    #[test]
    fn a_thread_on_a_customer_gateway_and_a_subscription_is_ambiguous() {
        let messages = vec![gateway_turn("conn-a"), subscription_turn("conn_sub")];
        let error = thread_connection(true, &messages).expect_err("two connections, no one route");
        assert_eq!(error.0, StatusCode::CONFLICT);
        assert_eq!(refusal_code(&error), "connection_ambiguous");
    }

    // The gate itself, with the switch passed in. Each request below carries no session credential, so an answer of
    // `Ok(None)` or the typed 401 proves the gate returned before any Session Core read.

    #[tokio::test]
    async fn with_the_switch_off_a_thread_is_served_on_the_platform_with_no_session_credential() {
        let state = state_with(SpeechInference::default()).await;
        let connection = feature_connection_with(
            false,
            &state,
            &claims(false, None),
            None,
            "thread-1",
            false,
            "a review",
        )
        .await
        .expect("a switch-off request is not refused");
        assert_eq!(connection, None);
    }

    #[tokio::test]
    async fn with_the_switch_off_a_zero_retention_request_decides_no_connection_posture() {
        let state = state_with(SpeechInference::default()).await;
        let connection = feature_connection_with(
            false,
            &state,
            &claims(true, None),
            None,
            "thread-1",
            true,
            "a review",
        )
        .await
        .expect("the switch-off path decides no connection posture");
        assert_eq!(connection, None);
    }

    #[tokio::test]
    async fn with_the_switch_on_a_blank_thread_is_served_on_the_platform_with_no_read() {
        let state = state_with(SpeechInference::default()).await;
        let connection = feature_connection_with(
            true,
            &state,
            &claims(false, None),
            None,
            "  ",
            false,
            "a review",
        )
        .await
        .expect("a blank thread names no connection");
        assert_eq!(connection, None);
    }

    #[tokio::test]
    async fn with_the_switch_on_a_thread_without_a_session_credential_is_refused_before_any_read() {
        let state = state_with(SpeechInference::default()).await;
        let error = feature_connection_with(
            true,
            &state,
            &claims(false, None),
            None,
            "thread-1",
            false,
            "a review",
        )
        .await
        .expect_err("the thread's connection cannot be read without the session credential");
        assert_eq!(error.0, StatusCode::UNAUTHORIZED);
        assert_eq!(refusal_code(&error), "delegated_credential_required");
    }

    // The gate with a Session Core stand-in (V3-mg-2-rel2). A feature on a thread reads the thread's summary before it
    // reads the transcript, as side chat does: a Space thread, and a thread holding a zero-retention or waived run, are
    // refused there, so a later non-waived token cannot reach the subscription a waived run used.

    /// The Session Core reads the gate makes, answered by a loopback stand-in: `ListThreads` with `threads`, and
    /// `ListConversation` with `messages`. Each read is counted, and every other RPC is unimplemented, so a read that a
    /// test does not expect fails it.
    #[derive(Clone, Default)]
    struct SessionReads {
        threads: Vec<ThreadSummary>,
        messages: Vec<SessionMessage>,
        listed: Arc<AtomicUsize>,
        conversations: Arc<AtomicUsize>,
    }

    /// One unary call answered by `handler`, framed as tonic's generated servers frame it.
    async fn serve_unary<Req, Resp, F, Fut, B>(
        request: http::Request<B>,
        handler: F,
    ) -> http::Response<tonic::body::BoxBody>
    where
        Req: prost::Message + Default + Send + 'static,
        Resp: prost::Message + Send + 'static,
        F: FnOnce(tonic::Request<Req>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<tonic::Response<Resp>, tonic::Status>> + Send + 'static,
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
    {
        struct Once<F>(Option<F>);
        impl<Req, Resp, F, Fut> tonic::server::UnaryService<Req> for Once<F>
        where
            Req: prost::Message + Default + Send + 'static,
            Resp: prost::Message + Send + 'static,
            F: FnOnce(tonic::Request<Req>) -> Fut + Send + 'static,
            Fut: Future<Output = Result<tonic::Response<Resp>, tonic::Status>> + Send + 'static,
        {
            type Response = Resp;
            type Future = BoxFuture<tonic::Response<Resp>, tonic::Status>;

            fn call(&mut self, request: tonic::Request<Req>) -> Self::Future {
                let handler = self.0.take().expect("one handler call per request");
                Box::pin(handler(request))
            }
        }
        let mut grpc = tonic::server::Grpc::new(tonic::codec::ProstCodec::default());
        grpc.unary(Once(Some(handler)), request).await
    }

    impl tonic::server::NamedService for SessionReads {
        const NAME: &'static str = "model_plane.v1.SessionCore";
    }

    impl<B> Service<http::Request<B>> for SessionReads
    where
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
    {
        type Response = http::Response<tonic::body::BoxBody>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: http::Request<B>) -> Self::Future {
            let this = self.clone();
            match request.uri().path() {
                "/model_plane.v1.SessionCore/ListThreads" => Box::pin(async move {
                    Ok(serve_unary(
                        request,
                        move |_: tonic::Request<ListThreadsRequest>| async move {
                            this.listed.fetch_add(1, Ordering::SeqCst);
                            Ok(tonic::Response::new(ListThreadsResponse {
                                threads: this.threads,
                                next_cursor: String::new(),
                            }))
                        },
                    )
                    .await)
                }),
                "/model_plane.v1.SessionCore/ListConversation" => Box::pin(async move {
                    Ok(serve_unary(
                        request,
                        move |_: tonic::Request<ListConversationRequest>| async move {
                            this.conversations.fetch_add(1, Ordering::SeqCst);
                            Ok(tonic::Response::new(ListConversationResponse {
                                messages: this.messages,
                                has_more: false,
                                ..Default::default()
                            }))
                        },
                    )
                    .await)
                }),
                _ => Box::pin(async move {
                    Ok(
                        tonic::Status::unimplemented("not answered by the gate's stand-in")
                            .into_http(),
                    )
                }),
            }
        }
    }

    /// A Session Core client on a loopback stand-in that answers `reads`, and the stand-in's counters.
    async fn session_reads(
        reads: SessionReads,
    ) -> (SessionCoreClient<tonic::transport::Channel>, SessionReads) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind the session stand-in");
        let address = listener.local_addr().expect("session stand-in address");
        let counted = reads.clone();
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(reads)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
            .expect("session stand-in endpoint")
            .connect_lazy();
        (SessionCoreClient::new(channel), counted)
    }

    /// The gate's state: the speech stand-in for inference, and `reads` behind Session Core.
    async fn gate_state(reads: SessionReads) -> (AppState, SessionReads) {
        let (client, counted) = session_reads(reads).await;
        let mut state = state_with(SpeechInference::default()).await;
        state.session_client = client;
        (state, counted)
    }

    fn session_bearer() -> VerifiedSessionBearer {
        VerifiedSessionBearer::for_test("session-token")
    }

    fn summary(thread_id: &str, space_id: &str, holds_retention_run: bool) -> ThreadSummary {
        ThreadSummary {
            thread_id: thread_id.to_owned(),
            space_id: space_id.to_owned(),
            holds_retention_run,
            ..Default::default()
        }
    }

    /// The reads made so far: the thread summary, then the transcript.
    fn reads_made(counted: &SessionReads) -> (usize, usize) {
        (
            counted.listed.load(Ordering::SeqCst),
            counted.conversations.load(Ordering::SeqCst),
        )
    }

    #[tokio::test]
    async fn a_feature_on_a_retention_thread_is_refused_before_its_transcript_is_read() {
        // A thread that held a waived subscription run names that subscription. A later, non-waived token must not
        // translate on it persistently, so the thread is refused before its turns are read.
        let (state, counted) = gate_state(SessionReads {
            threads: vec![summary("thread-1", "", true)],
            messages: vec![subscription_turn("conn_sub")],
            ..Default::default()
        })
        .await;
        let error = feature_connection_with(
            true,
            &state,
            &claims(false, None),
            Some(&session_bearer()),
            "thread-1",
            false,
            "reasoning translation",
        )
        .await
        .expect_err("a thread holding a waived run is refused for a feature");
        assert_eq!(error.0, StatusCode::CONFLICT);
        assert_eq!(refusal_code(&error), "side_chat_thread_retention_refused");
        assert_eq!(
            refusal_message(&error),
            "reasoning translation is not available for a thread that holds a zero-retention or waived run"
        );
        assert_eq!(
            reads_made(&counted),
            (1, 0),
            "the summary is read, the transcript is not"
        );
    }

    #[tokio::test]
    async fn a_feature_on_a_space_thread_is_refused_before_its_transcript_is_read() {
        let (state, counted) = gate_state(SessionReads {
            threads: vec![summary("thread-1", "space-1", false)],
            messages: vec![subscription_turn("conn_sub")],
            ..Default::default()
        })
        .await;
        let error = feature_connection_with(
            true,
            &state,
            &claims(false, None),
            Some(&session_bearer()),
            "thread-1",
            false,
            "a review",
        )
        .await
        .expect_err("a Space thread is refused for a feature");
        assert_eq!(error.0, StatusCode::CONFLICT);
        assert_eq!(refusal_code(&error), "scope_not_available_in_space");
        assert_eq!(
            refusal_message(&error),
            "a review is not available in a Space thread"
        );
        assert_eq!(
            reads_made(&counted),
            (1, 0),
            "the summary is read, the transcript is not"
        );
    }

    #[tokio::test]
    async fn a_space_thread_holding_a_retention_run_is_refused_as_a_space_thread() {
        // The Space check comes first: a thread in a Space is refused as one, even when it also holds a retention run.
        let (state, counted) = gate_state(SessionReads {
            threads: vec![summary("thread-1", "space-1", true)],
            messages: vec![subscription_turn("conn_sub")],
            ..Default::default()
        })
        .await;
        let error = feature_connection_with(
            true,
            &state,
            &claims(false, None),
            Some(&session_bearer()),
            "thread-1",
            false,
            "a review",
        )
        .await
        .expect_err("a Space thread is refused as a Space thread first");
        assert_eq!(error.0, StatusCode::CONFLICT);
        assert_eq!(refusal_code(&error), "scope_not_available_in_space");
        assert_eq!(
            reads_made(&counted),
            (1, 0),
            "the summary is read, the transcript is not"
        );
    }

    #[tokio::test]
    async fn a_plain_thread_and_a_connection_thread_pass_the_gate() {
        // Nothing to refuse: a platform thread stays on the platform, and a thread on a connection is served on it.
        let (state, counted) = gate_state(SessionReads {
            threads: vec![summary("thread-1", "", false)],
            messages: vec![SessionMessage {
                role: "user".to_owned(),
                content: "Hei".to_owned(),
                ..Default::default()
            }],
            ..Default::default()
        })
        .await;
        let connection = feature_connection_with(
            true,
            &state,
            &claims(false, None),
            Some(&session_bearer()),
            "thread-1",
            false,
            "a council",
        )
        .await
        .expect("a plain thread is not refused");
        assert_eq!(connection, None);
        assert_eq!(reads_made(&counted), (1, 1));

        let (state, counted) = gate_state(SessionReads {
            threads: vec![summary("thread-1", "", false)],
            messages: vec![subscription_turn("conn_sub")],
            ..Default::default()
        })
        .await;
        let connection = feature_connection_with(
            true,
            &state,
            &claims(false, None),
            Some(&session_bearer()),
            "thread-1",
            false,
            "a council",
        )
        .await
        .expect("a thread on a connection is not refused");
        assert_eq!(
            connection,
            Some(ThreadConnection::Subscription("conn_sub".to_owned()))
        );
        assert_eq!(reads_made(&counted), (1, 1));
    }

    #[tokio::test]
    async fn with_the_switch_off_a_retention_thread_is_not_read_for_a_feature() {
        let (state, counted) = gate_state(SessionReads {
            threads: vec![summary("thread-1", "", true)],
            messages: vec![subscription_turn("conn_sub")],
            ..Default::default()
        })
        .await;
        let connection = feature_connection_with(
            false,
            &state,
            &claims(false, None),
            Some(&session_bearer()),
            "thread-1",
            false,
            "a review",
        )
        .await
        .expect("the switch-off path makes no read and refuses nothing");
        assert_eq!(connection, None);
        assert_eq!(reads_made(&counted), (0, 0));
    }
}
