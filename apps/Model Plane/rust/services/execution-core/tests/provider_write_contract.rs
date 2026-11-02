//! Guards the attested, approval-gated provider write (WS23-S8) against the ways it could send with
//! less authority than its approval gives. A write that reaches integration-corev2 without an
//! attestation bound to its own approval, payload, connection and actor sends nothing, and so does
//! one whose credential the registry does not grant. A forged attestation is refused. A retry of an
//! approved write presents the same authorization again, which integration-corev2 answers from its
//! receipt, so the retry is a replay and never a second send.
//!
//! integration-corev2 is the authority: its `internal/attestation` suite refuses forged attestations
//! and its `internal/api` suite makes a replay idempotent. The verifier below follows the same checks,
//! so the side that signs is pinned in the crate that signs. Every key here is generated for the run
//! and signs nothing outside it.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
use execution_core::attestation::{self, Authorization};
use execution_core::integration_tools::{ActionOutcome, IntegrationActionsClient};
use serde_json::{json, Value};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY_ID: &str = "model-execution-contract";
const ATTESTATION_TYPE: &str = "verevon.provider-write-attestation+jwt";
const APPROVAL: &str = "appr_contract_1";
const ORG: &str = "org-1";
const CONNECTION: &str = "conn-1";
const PROVIDER: &str = "google";
const OPERATION: &str = "gmail.send";
const ACTOR: &str = "user-1";

/// `IntegrationActionsClient::from_env` reads the process environment, so each test that builds a
/// client takes this lock while it sets the variables and reads them. The client keeps what it read.
static ENVIRONMENT: Mutex<()> = Mutex::new(());

/// A fresh Ed25519 seed for one test.
fn ephemeral_seed() -> [u8; 32] {
    use rand::RngCore as _;
    let mut seed = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut seed);
    seed
}

/// A client aimed at `server`. With `attested`, it carries a write attestor built from `seed`.
fn client_for(server: &MockServer, seed: [u8; 32], attested: bool) -> IntegrationActionsClient {
    let _guard = ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner);
    let key_var = "EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY";
    let key_id_var = "EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_KEY_ID";
    std::env::set_var("INTEGRATION_COREV2_URL", server.uri());
    std::env::set_var("AUTH_CORE_URL", server.uri());
    std::env::set_var("INGESTION_SERVICE_API_KEY", "test-only-model-execution-key");
    std::env::remove_var("INGESTION_SERVICE_ID");
    if attested {
        std::env::set_var(key_var, STANDARD.encode(seed));
        std::env::set_var(key_id_var, KEY_ID);
    } else {
        std::env::remove_var(key_var);
        std::env::remove_var(key_id_var);
    }
    IntegrationActionsClient::from_env().expect("the client builds from the test environment")
}

/// Auth Core. The registry grants `integration:read` to model-execution, and `integration:write`
/// only when `write_granted`, which is the row WS23-S8 adds.
async fn mount_auth(server: &MockServer, write_granted: bool) {
    Mock::given(method("POST"))
        .and(path("/api/ingestion/internal-token"))
        .and(body_partial_json(json!({ "scopes": ["integration:read"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "token": "read-token" })))
        .mount(server)
        .await;
    let write = if write_granted {
        ResponseTemplate::new(200).set_body_json(json!({ "token": "write-token" }))
    } else {
        ResponseTemplate::new(403).set_body_json(json!({ "error": "scope not granted" }))
    };
    Mock::given(method("POST"))
        .and(path("/api/ingestion/internal-token"))
        .and(body_partial_json(
            json!({ "scopes": ["integration:write"] }),
        ))
        .respond_with(write)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/connections/conn-1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "data": { "connection": { "providerKey": PROVIDER } } })),
        )
        .mount(server)
        .await;
}

/// integration-corev2's action gateway, expecting exactly `times` calls on it.
async fn mount_action(server: &MockServer, times: u64) {
    Mock::given(method("POST"))
        .and(path("/api/v1/connections/conn-1/actions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": { "action": { "providerKey": PROVIDER, "operation": OPERATION, "result": { "id": "msg-1" } } }
        })))
        .expect(times)
        .mount(server)
        .await;
}

/// One action request integration-corev2 received, with the credential and key it carried.
struct Sent {
    body: Value,
    authorization: String,
    idempotency_key: Option<String>,
}

async fn sent_actions(server: &MockServer) -> Vec<Sent> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|request| {
            request.method.as_str() == "POST" && request.url.path().ends_with("/actions")
        })
        .map(|request| Sent {
            body: request.body_json().expect("the action body is JSON"),
            authorization: request
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_owned(),
            idempotency_key: request
                .headers
                .get("idempotency-key")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        })
        .collect()
}

/// The mail the model proposed, before the client adds the approval id to its body.
fn mail_body() -> Value {
    json!({ "to": ["alice@example.com"], "subject": "Status", "bodyText": "Hei" })
}

/// The body as integration-corev2 receives it, with the approval id the client adds.
fn sent_body() -> Value {
    let mut body = mail_body();
    body["approvalId"] = json!(APPROVAL);
    body
}

/// The binding integration-corev2 recomputes for a write from the params and body it received.
fn binding_for(params: &Value, body: &Value) -> Value {
    json!({
        "org_id": ORG,
        "connection_id": CONNECTION,
        "provider_key": PROVIDER,
        "operation": OPERATION,
        "payload_sha256": attestation::payload_sha256(ORG, CONNECTION, PROVIDER, OPERATION, params, body)
            .expect("the payload digests"),
        "idempotency_key": APPROVAL,
    })
}

fn now_unix() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_secs(),
    )
    .expect("the clock fits an i64")
}

/// A compact JWS for the approved send, from the real signer under `seed`.
fn signed_send(seed: [u8; 32], body: &Value) -> String {
    let payload_sha256 =
        attestation::payload_sha256(ORG, CONNECTION, PROVIDER, OPERATION, &json!({}), body)
            .expect("the payload digests");
    attestation::Signer::new(seed, KEY_ID)
        .expect("the signer takes its key id")
        .sign(Authorization {
            authorization_kind: attestation::AUTHORIZATION_HUMAN_APPROVED_AI_ACTION.to_owned(),
            authorization_id: APPROVAL.to_owned(),
            approval_id: APPROVAL.to_owned(),
            action_id: APPROVAL.to_owned(),
            org_id: ORG.to_owned(),
            connection_id: CONNECTION.to_owned(),
            provider_key: PROVIDER.to_owned(),
            operation: OPERATION.to_owned(),
            actor_id: ACTOR.to_owned(),
            payload_sha256,
            idempotency_key: APPROVAL.to_owned(),
        })
        .expect("a valid approved authorization signs")
}

fn trusted_header() -> Value {
    json!({ "alg": "EdDSA", "typ": ATTESTATION_TYPE, "kid": KEY_ID })
}

/// The claims a correct signer makes for the write `binding` describes, live from `now`.
fn live_claims(now: i64, binding: &Value) -> Value {
    json!({
        "v": 1,
        "iss": "model-execution",
        "aud": "integration-corev2",
        "presenter_service": "model-execution",
        "authorization_kind": "human_approved_ai_action",
        "authorization_id": APPROVAL,
        "approval_id": APPROVAL,
        "action_id": APPROVAL,
        "org_id": binding["org_id"],
        "connection_id": binding["connection_id"],
        "provider_key": binding["provider_key"],
        "operation": binding["operation"],
        "actor_id": ACTOR,
        "payload_sha256": binding["payload_sha256"],
        "idempotency_key": APPROVAL,
        "jti": "forged-jti-1",
        "iat": now,
        "nbf": now,
        "exp": now + 30,
    })
}

fn with_claim(claims: &Value, name: &str, value: Value) -> Value {
    let mut edited = claims.clone();
    edited[name] = value;
    edited
}

/// A compact JWS over any header and claims, signed by `key`: how a test makes a token that a
/// correct signer would never produce, under a key the test chooses.
fn sign_compact(key: &SigningKey, header: &Value, claims: &Value) -> String {
    let protected = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(header).expect("header is JSON")),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).expect("claims are JSON")),
    );
    let signature = key.sign(protected.as_bytes());
    format!(
        "{protected}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    )
}

/// `compact` with one claim replaced and the signature left as it was: a token edited after signing.
fn edit_claim(compact: &str, name: &str, value: Value) -> String {
    let [header, claims, signature]: [&str; 3] = compact
        .split('.')
        .collect::<Vec<_>>()
        .try_into()
        .expect("a compact JWS has three segments");
    let mut edited = decode(claims).expect("the claims decode");
    edited[name] = value;
    format!(
        "{header}.{}.{signature}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&edited).expect("claims are JSON"))
    )
}

fn decode_bytes(segment: &str) -> Result<Vec<u8>, &'static str> {
    URL_SAFE_NO_PAD
        .decode(segment)
        .map_err(|_| "a segment is not base64url")
}

fn decode(segment: &str) -> Result<Value, &'static str> {
    serde_json::from_slice(&decode_bytes(segment)?).map_err(|_| "a segment is not JSON")
}

fn jti(compact: &str) -> String {
    decode(compact.split('.').nth(1).expect("the claims segment")).expect("the claims decode")
        ["jti"]
        .as_str()
        .expect("the attestation has a jti")
        .to_owned()
}

/// The claims of `compact`, when it passes the checks integration-corev2 makes on a write: its
/// signature under the trusted key, the issuer, audience and presenter, the binding to this write,
/// an approved action named by its own approval, and a lifetime of at most a minute that is live now.
fn check(
    compact: &str,
    trusted: &VerifyingKey,
    binding: &Value,
    now: i64,
) -> Result<Value, &'static str> {
    let [header_segment, claims_segment, signature_segment]: [&str; 3] = compact
        .split('.')
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| "the attestation is not three segments")?;
    let header = decode(header_segment)?;
    if header["alg"] != "EdDSA" || header["typ"] != ATTESTATION_TYPE || header["kid"] != KEY_ID {
        return Err("the header names another algorithm, type or key");
    }
    let signature = Signature::from_slice(&decode_bytes(signature_segment)?)
        .map_err(|_| "the signature is malformed")?;
    trusted
        .verify(
            format!("{header_segment}.{claims_segment}").as_bytes(),
            &signature,
        )
        .map_err(|_| "the signature does not verify under the trusted key")?;
    let claims = decode(claims_segment)?;
    if claims["iss"] != "model-execution"
        || claims["aud"] != "integration-corev2"
        || claims["presenter_service"] != "model-execution"
        || claims["v"] != 1
    {
        return Err("the issuer, audience, presenter or version is not the one accepted");
    }
    for field in [
        "org_id",
        "connection_id",
        "provider_key",
        "operation",
        "payload_sha256",
        "idempotency_key",
    ] {
        if claims[field] != binding[field] {
            return Err("the attestation is bound to another write");
        }
    }
    let approval = &claims["approval_id"];
    if claims["authorization_kind"] != "human_approved_ai_action"
        || !approval.is_string()
        || approval != &claims["action_id"]
    {
        return Err("an approved write must name its approval as its action");
    }
    let (Some(iat), Some(nbf), Some(exp)) = (
        claims["iat"].as_i64(),
        claims["nbf"].as_i64(),
        claims["exp"].as_i64(),
    ) else {
        return Err("the attestation has no validity window");
    };
    if iat != nbf || iat > now || nbf > now || exp <= now || !(1..=60).contains(&(exp - iat)) {
        return Err("the attestation is not live for at most sixty seconds");
    }
    if claims["jti"].as_str().unwrap_or_default().is_empty() {
        return Err("the attestation has no jti");
    }
    Ok(claims)
}

/// What integration-corev2 does with one verified attestation (`ClaimActionReceipt` and
/// `actionReceiptMatchesAuthorization`): a first attestation under an idempotency key sends, the same
/// authorization again is answered from the stored receipt, and a different authorization under a used
/// key is a conflict. The jti is not compared, so a retry with a fresh one still matches.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Sent,
    Replayed,
    Conflict,
}

const SAME_AUTHORIZATION: [&str; 11] = [
    "org_id",
    "connection_id",
    "provider_key",
    "operation",
    "payload_sha256",
    "idempotency_key",
    "authorization_kind",
    "authorization_id",
    "approval_id",
    "action_id",
    "actor_id",
];

fn receive(
    receipts: &mut HashMap<String, Value>,
    trusted: &VerifyingKey,
    binding: &Value,
    compact: &str,
    now: i64,
) -> Result<Outcome, &'static str> {
    let claims = check(compact, trusted, binding, now)?;
    let key = binding["idempotency_key"]
        .as_str()
        .expect("a binding names its idempotency key")
        .to_owned();
    if let Some(stored) = receipts.get(&key) {
        let same = SAME_AUTHORIZATION
            .iter()
            .all(|&field| stored[field] == claims[field]);
        return Ok(if same {
            Outcome::Replayed
        } else {
            Outcome::Conflict
        });
    }
    receipts.insert(key, claims);
    Ok(Outcome::Sent)
}

/// The refusal an unsent write gives. A write that was sent is a failure of the test.
fn refusal(result: Result<ActionOutcome, String>) -> String {
    match result {
        Ok(_) => panic!("the write was sent"),
        Err(error) => error,
    }
}

#[tokio::test]
async fn an_approved_write_is_attested_to_its_own_approval_payload_and_actor() {
    let server = MockServer::start().await;
    mount_auth(&server, true).await;
    mount_action(&server, 1).await;
    let seed = ephemeral_seed();
    let client = client_for(&server, seed, true);

    client
        .execute_action(
            ORG,
            CONNECTION,
            OPERATION,
            json!({}),
            mail_body(),
            ACTOR,
            Some(APPROVAL),
            None,
        )
        .await
        .expect("an attested write is sent");

    let sent = sent_actions(&server).await;
    assert_eq!(sent.len(), 1, "one send");
    let action = &sent[0];
    assert_eq!(
        action.authorization, "Bearer write-token",
        "the write carries the write credential"
    );
    assert_eq!(action.idempotency_key.as_deref(), Some(APPROVAL));
    assert_eq!(action.body["idempotencyKey"], APPROVAL);
    assert_eq!(action.body["body"]["approvalId"], APPROVAL);
    let token = action.body["writeAttestation"]
        .as_str()
        .expect("the write carries its attestation");
    let trusted = SigningKey::from_bytes(&seed).verifying_key();
    let binding = binding_for(&action.body["params"], &action.body["body"]);
    let claims = check(token, &trusted, &binding, now_unix())
        .expect("the attestation verifies for the write it authorizes");
    assert_eq!(
        claims["actor_id"], ACTOR,
        "the attestation names the approving user"
    );
    assert_eq!(
        claims["approval_id"], APPROVAL,
        "the attestation names its approval"
    );
    server.verify().await;
}

#[tokio::test]
async fn a_write_the_registry_does_not_grant_sends_nothing() {
    let server = MockServer::start().await;
    mount_auth(&server, false).await;
    mount_action(&server, 0).await;
    let client = client_for(&server, ephemeral_seed(), true);

    let error = refusal(
        client
            .execute_action(
                ORG,
                CONNECTION,
                OPERATION,
                json!({}),
                mail_body(),
                ACTOR,
                Some(APPROVAL),
                None,
            )
            .await,
    );
    assert!(error.contains("refused"), "{error}");
    server.verify().await;
}

#[tokio::test]
async fn a_write_without_the_attestation_keys_sends_nothing() {
    let server = MockServer::start().await;
    mount_auth(&server, true).await;
    mount_action(&server, 0).await;
    let client = client_for(&server, ephemeral_seed(), false);

    let error = refusal(
        client
            .execute_action(
                ORG,
                CONNECTION,
                OPERATION,
                json!({}),
                mail_body(),
                ACTOR,
                Some(APPROVAL),
                None,
            )
            .await,
    );
    assert!(
        error.contains("attestation signer is not configured"),
        "{error}"
    );
    server.verify().await;
}

#[tokio::test]
async fn a_write_with_no_acting_user_sends_nothing() {
    let server = MockServer::start().await;
    mount_auth(&server, true).await;
    mount_action(&server, 0).await;
    let client = client_for(&server, ephemeral_seed(), true);

    let error = refusal(
        client
            .execute_action(
                ORG,
                CONNECTION,
                OPERATION,
                json!({}),
                mail_body(),
                "  ",
                Some(APPROVAL),
                None,
            )
            .await,
    );
    assert!(error.contains("acting user"), "{error}");
    server.verify().await;
}

#[tokio::test]
async fn a_read_carries_no_attestation_and_no_write_credential() {
    let server = MockServer::start().await;
    mount_auth(&server, true).await;
    mount_action(&server, 1).await;
    let client = client_for(&server, ephemeral_seed(), true);

    client
        .read_action_json(ORG, CONNECTION, "mail.messages", json!({}), None)
        .await
        .expect("a read is sent");

    let sent = sent_actions(&server).await;
    assert_eq!(sent[0].authorization, "Bearer read-token");
    assert!(
        sent[0].body.get("writeAttestation").is_none(),
        "a read is not attested"
    );
    assert!(
        sent[0].idempotency_key.is_none(),
        "a read has no idempotency key"
    );
    server.verify().await;
}

#[tokio::test]
async fn a_retried_approved_write_presents_the_same_authorization_and_is_a_replay() {
    let server = MockServer::start().await;
    mount_auth(&server, true).await;
    mount_action(&server, 2).await;
    let seed = ephemeral_seed();
    let client = client_for(&server, seed, true);

    for _ in 0..2 {
        client
            .execute_action(
                ORG,
                CONNECTION,
                OPERATION,
                json!({}),
                mail_body(),
                ACTOR,
                Some(APPROVAL),
                None,
            )
            .await
            .expect("the write is sent");
    }

    let trusted = SigningKey::from_bytes(&seed).verifying_key();
    let now = now_unix();
    let mut receipts = HashMap::new();
    let mut outcomes = Vec::new();
    for action in sent_actions(&server).await {
        let binding = binding_for(&action.body["params"], &action.body["body"]);
        let token = action.body["writeAttestation"]
            .as_str()
            .expect("each write is attested");
        outcomes.push(
            receive(&mut receipts, &trusted, &binding, token, now)
                .expect("each attestation verifies"),
        );
    }
    assert_eq!(outcomes, vec![Outcome::Sent, Outcome::Replayed]);
    server.verify().await;
}

#[test]
fn a_forged_write_attestation_is_refused() {
    let seed = ephemeral_seed();
    let key = SigningKey::from_bytes(&seed);
    let trusted = key.verifying_key();
    let binding = binding_for(&json!({}), &sent_body());
    let now = now_unix();
    let genuine = signed_send(seed, &sent_body());
    assert!(
        check(&genuine, &trusted, &binding, now).is_ok(),
        "the genuine attestation verifies"
    );

    let claims = live_claims(now, &binding);
    let header = trusted_header();
    let forged: Vec<(&str, String)> = vec![
        (
            "signed under a key nobody trusts",
            sign_compact(&SigningKey::from_bytes(&ephemeral_seed()), &header, &claims),
        ),
        (
            "an operation edited after signing",
            edit_claim(&genuine, "operation", json!("gmail.messages")),
        ),
        (
            "a digest edited after signing",
            edit_claim(&genuine, "payload_sha256", json!("0".repeat(64))),
        ),
        (
            "an approval other than the action it authorizes",
            sign_compact(
                &key,
                &header,
                &with_claim(&claims, "approval_id", json!("appr_other")),
            ),
        ),
        (
            "an issuer other than model-execution",
            sign_compact(
                &key,
                &header,
                &with_claim(&claims, "iss", json!("conversation-core")),
            ),
        ),
        (
            "an authorization of human intent",
            sign_compact(
                &key,
                &header,
                &with_claim(&claims, "authorization_kind", json!("human_intent")),
            ),
        ),
        (
            "a lifetime over a minute",
            sign_compact(&key, &header, &with_claim(&claims, "exp", json!(now + 61))),
        ),
        (
            "a token not yet valid",
            sign_compact(
                &key,
                &header,
                &with_claim(
                    &with_claim(&claims, "iat", json!(now + 5)),
                    "nbf",
                    json!(now + 5),
                ),
            ),
        ),
        (
            "an expired token",
            sign_compact(&key, &header, &with_claim(&claims, "exp", json!(now))),
        ),
        (
            "a header naming another key",
            sign_compact(
                &key,
                &json!({ "alg": "EdDSA", "typ": ATTESTATION_TYPE, "kid": "unregistered" }),
                &claims,
            ),
        ),
        (
            "a header naming another algorithm",
            sign_compact(
                &key,
                &json!({ "alg": "HS256", "typ": ATTESTATION_TYPE, "kid": KEY_ID }),
                &claims,
            ),
        ),
    ];
    for (name, token) in forged {
        assert!(
            check(&token, &trusted, &binding, now).is_err(),
            "a forged attestation was accepted: {name}"
        );
    }
}

#[test]
fn an_attestation_for_one_write_is_refused_for_another() {
    let seed = ephemeral_seed();
    let trusted = SigningKey::from_bytes(&seed).verifying_key();
    let now = now_unix();
    let genuine = signed_send(seed, &sent_body());

    let mut other_body = sent_body();
    other_body["bodyText"] = json!("Another message");
    assert_eq!(
        check(
            &genuine,
            &trusted,
            &binding_for(&json!({}), &other_body),
            now
        ),
        Err("the attestation is bound to another write"),
        "a changed payload"
    );
    let mut elsewhere = binding_for(&json!({}), &sent_body());
    elsewhere["connection_id"] = json!("conn-2");
    assert_eq!(
        check(&genuine, &trusted, &elsewhere, now),
        Err("the attestation is bound to another write"),
        "another connection"
    );
    let mut rekeyed = binding_for(&json!({}), &sent_body());
    rekeyed["idempotency_key"] = json!("appr_other");
    assert_eq!(
        check(&genuine, &trusted, &rekeyed, now),
        Err("the attestation is bound to another write"),
        "another idempotency key"
    );
}

#[test]
fn a_replayed_attestation_is_a_replay_or_a_conflict_and_never_a_second_send() {
    let seed = ephemeral_seed();
    let trusted = SigningKey::from_bytes(&seed).verifying_key();
    let binding = binding_for(&json!({}), &sent_body());
    let now = now_unix();
    let first = signed_send(seed, &sent_body());
    let retry = signed_send(seed, &sent_body());
    assert_ne!(
        jti(&first),
        jti(&retry),
        "every attestation carries its own jti"
    );

    let mut receipts = HashMap::new();
    assert_eq!(
        receive(&mut receipts, &trusted, &binding, &first, now),
        Ok(Outcome::Sent)
    );
    assert_eq!(
        receive(&mut receipts, &trusted, &binding, &retry, now),
        Ok(Outcome::Replayed),
        "a retry of the same authorization is answered from the receipt"
    );
    assert_eq!(
        receive(&mut receipts, &trusted, &binding, &first, now),
        Ok(Outcome::Replayed),
        "the first attestation presented again is the same replay"
    );

    let mut altered = sent_body();
    altered["bodyText"] = json!("Changed after approval");
    let altered_binding = binding_for(&json!({}), &altered);
    assert_eq!(
        receive(&mut receipts, &trusted, &altered_binding, &first, now),
        Err("the attestation is bound to another write"),
        "the approved attestation does not authorize the changed payload"
    );
    assert_eq!(
        receive(
            &mut receipts,
            &trusted,
            &altered_binding,
            &signed_send(seed, &altered),
            now
        ),
        Ok(Outcome::Conflict),
        "a new attestation for the changed payload under the used key is a conflict, not a send"
    );
}

/// The text of the method whose signature starts with `signature`, to its closing brace at the
/// method's own indent, so the next method is never read as part of it.
fn method_body<'a>(text: &'a str, signature: &str) -> &'a str {
    let start = text
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` not found"));
    let rest = &text[start..];
    let close = "\n    }\n";
    let end = rest
        .find(close)
        .map_or(rest.len(), |offset| offset + close.len());
    &rest[..end]
}

#[test]
fn the_read_path_never_mints_the_write_scope_and_a_write_mints_it_only_when_approved() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/integration_tools.rs"
    ))
    .expect("the integration tools source is readable")
    .replace("\r\n", "\n");
    let read = method_body(&source, "async fn send_read(");
    assert!(
        read.contains("\"integration:read\"") && !read.contains("integration:write"),
        "the shared read path names only the read scope"
    );
    let execute = method_body(&source, "pub async fn execute_action(");
    assert!(
        execute.contains("let scope = if approval.is_some() {"),
        "the write scope is chosen only where an approval is present"
    );
}
