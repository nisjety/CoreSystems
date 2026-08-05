//! Shipping quote tool for the agentic loop — Verevon's freight aggregator.
//!
//! Backed by `shipping-core` (Ingestion Plane), the carrier-adapter fleet
//! migrated from Suplayer-System: parallel quote fan-out across Bring,
//! PostNord, DHL, DSV, Helthjem, Porterbuddy (+ real Bring/UPS/FedEx once
//! their credentials are configured), each carrier bounded by its own
//! timeout, cheapest-first. This replaces the standalone `agent-service`
//! from the original repo — the governed loop here provides what that
//! service lacked (audit, HITL posture, cost accounting).
//!
//! `get_shipping_quotes` and `shipping_carriers` are read-only and run
//! under `ask` posture without an approval gate, like the info tools.
//! `book_shipment` places a REAL freight order: it is listed in
//! `permission::is_risky_tool`, so the run pauses for explicit human
//! approval first, and it then satisfies shipping-core's own two-step
//! confirmation gate (create → confirm with the returned token). Tracking
//! of an existing parcel is the separate `track_shipment` tool.
//!
//! Transport:
//!   POST {SHIPPING_CORE_URL}/api/quotes                → quotes cheapest-first
//!   GET  {SHIPPING_CORE_URL}/api/carriers              → registered fleet
//!   GET  {SHIPPING_CORE_URL}/api/tracking/{tracking_no} → tenant-scoped tracking
//!   POST {SHIPPING_CORE_URL}/api/bookings[+/confirm]   → gated booking
//!
//! Responses are parsed defensively as `serde_json::Value` like the other
//! cross-plane tools — the wire DTOs live in a Go service and may grow.

#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use std::fmt::Write as _;
use std::time::Duration;

use serde_json::Value;

/// Default shipping-core address. execution-core sits on model-plane-network
/// only, so cross-plane services are dialled via the host-published port
/// (same pattern as `INFORMATION_CORE_URL`/`QUARRY_EDGE_URL`).
const DEFAULT_SHIPPING_CORE_URL: &str = "http://host.docker.internal:3156";
const DEFAULT_AUTH_CORE_URL: &str = "http://host.docker.internal:3011";

/// Cap on quotes rendered to the model — the engine already sorts
/// cheapest-first, so the head is the interesting part.
const MAX_QUOTES: usize = 10;
const MAX_TRACKING_EVENTS: usize = 6;

/// Input for a quote request, mirroring shipping-core's wire shape
/// (`internal/quoteengine/http.go` quoteRequestDTO — snake_case JSON).
#[derive(serde::Deserialize)]
pub struct QuoteInput {
    pub from: AddressInput,
    pub to: AddressInput,
    pub weight_kg: f64,
    pub length_cm: f64,
    pub width_cm: f64,
    pub height_cm: f64,
    #[serde(default)]
    pub dangerous_good: bool,
    /// "b2b" when the RECIPIENT is a business, else "b2c".
    pub segment: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct AddressInput {
    pub name: String,
    #[serde(default)]
    pub street: String,
    pub postal_code: String,
    pub city: String,
    /// ISO 3166-1 alpha-2, e.g. "NO".
    pub country: String,
    #[serde(default)]
    pub is_business: bool,
    /// Delivery-notification contact. Optional here — the model has no
    /// reason to know either for a recipient it has never met — but some
    /// carriers (Bring included) reject a booking outright without at
    /// least one on the recipient. `book_shipment`'s callers fill this in
    /// from user-core when the model omits it; see `user_core_client`.
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

/// Input for book_shipment: the chosen quote's identity + the shipment.
/// `customs` passes through verbatim to shipping-core, which enforces the
/// cross-border requirement server-side.
#[derive(Debug, serde::Deserialize)]
pub struct BookInput {
    #[serde(default)]
    pub quote_ref: String,
    pub carrier_code: String,
    pub service_name: String,
    pub price_amount_cents: i64,
    pub price_currency: String,
    pub from: AddressInput,
    pub to: AddressInput,
    pub weight_kg: f64,
    pub length_cm: f64,
    pub width_cm: f64,
    pub height_cm: f64,
    #[serde(default)]
    pub dangerous_good: bool,
    #[serde(default)]
    pub customs: Option<Value>,
    pub booked_by: String,
}

/// Result of a successful `book_shipment` call: text for the model, plus
/// shipping-core's own durable `booking_id` — the authoritative receipt a
/// durable approval-continuation outcome may cite (see
/// `approval_delivery_worker`). Always present on `Ok` (unlike
/// `IntegrationActionsClient::execute_action`'s heuristic, optional
/// `provider_receipt_id`): the create step's response is required to carry
/// it or `book_shipment` already returns `Err` before reaching confirm.
#[derive(Debug)]
pub struct BookingOutcome {
    pub rendered: String,
    pub booking_id: String,
}

fn address_json(a: &AddressInput) -> Value {
    serde_json::json!({
        "name": a.name, "street": a.street, "postal_code": a.postal_code,
        "city": a.city, "country": a.country, "is_business": a.is_business,
        "phone": a.phone, "email": a.email,
    })
}

/// HTTP client for shipping-core. Cheap to clone.
#[derive(Clone)]
pub struct ShippingToolsClient {
    base_url: String,
    auth_core_url: String,
    service_id: String,
    service_credential: String,
    http: reqwest::Client,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaneTokenResponse {
    token: String,
}

impl ShippingToolsClient {
    /// Build from the environment: `SHIPPING_CORE_URL` overrides the
    /// host-published compose default. Returns `None` only when the reqwest
    /// client cannot be built.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("SHIPPING_CORE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_SHIPPING_CORE_URL.to_owned());
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .ok()?;
        let auth_core_url = std::env::var("AUTH_CORE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_AUTH_CORE_URL.to_owned());
        let service_id = std::env::var("INGESTION_SERVICE_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "model-execution".to_owned());
        let service_credential = std::env::var("INGESTION_SERVICE_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        Some(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id,
            service_credential,
            http,
        })
    }

    #[cfg(test)]
    fn new_for_test(
        base_url: String,
        auth_core_url: String,
        service_id: &str,
        service_credential: &str,
    ) -> Self {
        Self {
            base_url,
            auth_core_url,
            service_id: service_id.to_owned(),
            service_credential: service_credential.to_owned(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("test HTTP client"),
        }
    }

    /// POST /api/quotes and render the comparison compactly for the model:
    /// carrier, service, price, transit, plus any per-carrier errors (the
    /// engine reports failed carriers instead of silently dropping them).
    pub async fn get_quotes(&self, input: &QuoteInput, org_id: &str) -> Result<String, String> {
        let body = serde_json::json!({
            "from": {
                "name": input.from.name,
                "street": input.from.street,
                "postal_code": input.from.postal_code,
                "city": input.from.city,
                "country": input.from.country,
                "is_business": input.from.is_business,
            },
            "to": {
                "name": input.to.name,
                "street": input.to.street,
                "postal_code": input.to.postal_code,
                "city": input.to.city,
                "country": input.to.country,
                "is_business": input.to.is_business,
            },
            "package": {
                "weight_kg": input.weight_kg,
                "length_cm": input.length_cm,
                "width_cm": input.width_cm,
                "height_cm": input.height_cm,
                "dangerous_good": input.dangerous_good,
            },
            "segment": input.segment,
        });

        let token = self.mint_ingestion_token(org_id, "shipping:read").await?;
        let resp = self
            .http
            .post(format!("{}/api/quotes", self.base_url))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("shipping-core /api/quotes request failed: {e}"))?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|e| format!("shipping-core /api/quotes decode failed: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "shipping-core /api/quotes returned {status}: {value}"
            ));
        }
        Ok(render_quotes(&value))
    }

    /// book_shipment: create + confirm a booking against shipping-core's
    /// two-step gate. The HUMAN approval already happened at the agent-loop
    /// level — `book_shipment` is in `permission::is_risky_tool`, so under
    /// `ask` posture the run pauses for explicit approval before this
    /// executes. shipping-core's own token gate is then satisfied by chaining
    /// the token from the create response into the confirm call; the audit
    /// log on shipping-core records both steps with the acting run id.
    pub async fn book_shipment(
        &self,
        input: &BookInput,
        org_id: &str,
        approval_id: &str,
        idempotency_key: &str,
    ) -> Result<BookingOutcome, String> {
        if approval_id.trim().is_empty() || idempotency_key.trim().is_empty() {
            return Err(
                "booking blocked: durable approval and idempotency key are required".to_owned(),
            );
        }
        let create_body = serde_json::json!({
            "quote_ref": input.quote_ref,
            "carrier_code": input.carrier_code,
            "service_name": input.service_name,
            "price": {"amount_cents": input.price_amount_cents, "currency": input.price_currency},
            "from": address_json(&input.from),
            "to": address_json(&input.to),
            "package": {
                "weight_kg": input.weight_kg, "length_cm": input.length_cm,
                "width_cm": input.width_cm, "height_cm": input.height_cm,
                "dangerous_good": input.dangerous_good,
            },
            "customs": input.customs,
            "booked_by": input.booked_by,
            "approval_id": approval_id,
        });
        let bearer = self.mint_ingestion_token(org_id, "shipping:write").await?;
        let created = self
            .post_json("/api/bookings", &create_body, &bearer, idempotency_key)
            .await
            .map_err(|e| format!("booking create failed: {e}"))?;
        let booking_id = created
            .get("booking_id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("booking create response missing booking_id: {created}"))?
            .to_owned();
        let confirmation_token = created
            .get("confirmation_token")
            .and_then(Value::as_str)
            .ok_or("booking create response missing confirmation token")?
            .to_owned();

        let confirm_body = serde_json::json!({
            "confirmation_token": confirmation_token,
            "actor": input.booked_by,
        });
        let booked = self
            .post_json(
                &format!("/api/bookings/{booking_id}/confirm"),
                &confirm_body,
                &bearer,
                idempotency_key,
            )
            .await
            .map_err(|e| {
                format!("booking confirm failed (booking {booking_id} stays pending): {e}")
            })?;

        let tracking = booked
            .get("tracking_no")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let carrier = booked
            .get("carrier_name")
            .and_then(Value::as_str)
            .unwrap_or(&input.carrier_code);
        let booking_ref = booked
            .get("booking_ref")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let rendered = format!(
            "Shipment BOOKED with {carrier}.\n- Booking id: {booking_id}\n- Carrier ref: {booking_ref}\n- Tracking number: {tracking}\n- Label: {}\nThe label PDF and audit trail are available on the booking.",
            if booked.get("has_label").and_then(Value::as_bool).unwrap_or(false) { "stored" } else { "not available" },
        );
        Ok(BookingOutcome {
            rendered,
            booking_id,
        })
    }

    async fn post_json(
        &self,
        path: &str,
        body: &Value,
        token: &str,
        idempotency_key: &str,
    ) -> Result<Value, String> {
        let mut request = self
            .http
            .post(format!("{}{path}", self.base_url))
            .bearer_auth(token)
            .json(body);
        if !idempotency_key.trim().is_empty() {
            request = request.header("idempotency-key", idempotency_key.trim());
        }
        let resp = request
            .send()
            .await
            .map_err(|e| format!("shipping-core {path} request failed: {e}"))?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|e| format!("shipping-core {path} decode failed: {e}"))?;
        if !status.is_success() {
            return Err(format!("shipping-core {path} returned {status}: {value}"));
        }
        Ok(value)
    }

    /// GET /api/carriers — the registered fleet (mock vs credentialed real
    /// adapters), so the model can answer "which carriers can Verevon compare".
    pub async fn list_carriers(&self, org_id: &str) -> Result<String, String> {
        let token = self.mint_ingestion_token(org_id, "shipping:read").await?;
        let resp = self
            .http
            .get(format!("{}/api/carriers", self.base_url))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("shipping-core /api/carriers request failed: {e}"))?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|e| format!("shipping-core /api/carriers decode failed: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "shipping-core /api/carriers returned {status}: {value}"
            ));
        }
        Ok(render_carriers(&value))
    }

    /// GET /api/tracking/{tracking_no} — resolve an existing shipment only in
    /// the verified organization and render its current persisted/event state.
    pub async fn track_shipment(&self, tracking_no: &str, org_id: &str) -> Result<String, String> {
        let tracking_no = tracking_no.trim();
        if tracking_no.is_empty()
            || tracking_no.len() > 120
            || !tracking_no
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err("track_shipment requires a bounded tracking number".to_owned());
        }
        let token = self.mint_ingestion_token(org_id, "shipping:read").await?;
        let resp = self
            .http
            .get(format!("{}/api/tracking/{}", self.base_url, tracking_no))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("shipping-core /api/tracking request failed: {e}"))?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .map_err(|e| format!("shipping-core /api/tracking decode failed: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "shipping-core /api/tracking returned {status}: {value}"
            ));
        }
        Ok(render_tracking(&value))
    }

    async fn mint_ingestion_token(&self, org_id: &str, scope: &str) -> Result<String, String> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            return Err("shipping request blocked: run organization is required".to_owned());
        }
        let reason = if scope == "shipping:write" {
            "execution-core shipping write"
        } else {
            "execution-core shipping read"
        };
        let response = self
            .http
            .post(format!(
                "{}/api/ingestion/internal-token",
                self.auth_core_url
            ))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", &self.service_credential)
            .json(&serde_json::json!({
                "orgId": org_id,
                "scopes": [scope],
                "reason": reason,
            }))
            .send()
            .await
            .map_err(|error| format!("Auth Core token request failed: {error}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "Auth Core refused the scoped shipping credential ({status})"
            ));
        }
        let token = response
            .json::<PlaneTokenResponse>()
            .await
            .map_err(|error| format!("Auth Core token response was invalid: {error}"))?
            .token;
        if token.trim().is_empty() {
            return Err("Auth Core returned an empty shipping credential".to_owned());
        }
        Ok(token)
    }
}

fn render_quotes(value: &Value) -> String {
    let quotes = value
        .pointer("/quotes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let errors = value
        .pointer("/errors")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if quotes.is_empty() && errors.is_empty() {
        return "No carrier returned a quote for this shipment.".to_owned();
    }

    let mut s = format!(
        "Shipping quotes (cheapest first, {} option(s)):\n",
        quotes.len()
    );
    for q in quotes.iter().take(MAX_QUOTES) {
        let carrier = q
            .get("carrier_name")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let service = q
            .get("service_name")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let cents = q
            .pointer("/price/amount_cents")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let currency = q
            .pointer("/price/currency")
            .and_then(Value::as_str)
            .unwrap_or("NOK");
        let transit = q.get("transit_days").and_then(Value::as_i64).unwrap_or(-1);
        let eta = q
            .get("estimated_delivery")
            .and_then(Value::as_str)
            .unwrap_or("");
        let features = q
            .get("features")
            .and_then(Value::as_array)
            .map(|f| {
                f.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        // Integer money formatting — no float precision loss on øre amounts.
        let _ = write!(
            s,
            "- {carrier} — {service}: {}.{:02} {currency}",
            cents / 100,
            (cents % 100).unsigned_abs()
        );
        if transit >= 0 {
            let _ = write!(s, ", {transit} day(s) transit");
        }
        if !eta.is_empty() {
            let _ = write!(s, ", ETA {eta}");
        }
        if !features.is_empty() {
            let _ = write!(s, " [{features}]");
        }
        s.push('\n');
    }
    if quotes.len() > MAX_QUOTES {
        let _ = writeln!(s, "… and {} more option(s).", quotes.len() - MAX_QUOTES);
    }
    for e in &errors {
        let code = e
            .get("carrier_code")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let msg = e.get("message").and_then(Value::as_str).unwrap_or("failed");
        let _ = writeln!(s, "! {code}: no quote ({msg})");
    }
    s
}

fn render_carriers(value: &Value) -> String {
    let carriers = value
        .pointer("/carriers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if carriers.is_empty() {
        return "No carriers are registered in the shipping aggregator.".to_owned();
    }
    let mut s = format!("Registered carriers ({}):\n", carriers.len());
    for c in &carriers {
        let name = c.get("name").and_then(Value::as_str).unwrap_or("unknown");
        let segment = c.get("segment").and_then(Value::as_str).unwrap_or("");
        let is_mock = c.get("is_mock").and_then(Value::as_bool).unwrap_or(false);
        let provider_mode = c.get("mode").and_then(Value::as_str).unwrap_or(if is_mock {
            "mock"
        } else {
            "unknown"
        });
        let mode = match provider_mode {
            "mock" => "demo prices",
            "production" => "production",
            "sandbox" => "sandbox",
            _ => "unknown mode",
        };
        let verified_at = c.get("verified_at").and_then(Value::as_str);
        let degraded_reason = c
            .get("degraded_reason")
            .and_then(Value::as_str)
            .unwrap_or("");
        if provider_mode != "mock" && verified_at.is_none() && !degraded_reason.is_empty() {
            let _ = writeln!(
                s,
                "- {name} ({segment}, {mode}, unverified: {degraded_reason})"
            );
        } else if let Some(verified_at) = verified_at {
            let _ = writeln!(s, "- {name} ({segment}, {mode}, verified {verified_at})");
        } else {
            let _ = writeln!(s, "- {name} ({segment}, {mode})");
        }
    }
    s
}

fn render_tracking(value: &Value) -> String {
    let tracking_no = value
        .get("tracking_no")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let status = value
        .get("current_status")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("status unknown");
    let mut out = format!("Shipment {tracking_no}: {status}");
    if let Some(events) = value.get("events").and_then(Value::as_array) {
        if !events.is_empty() {
            out.push_str("\nRecent events:\n");
            for event in events.iter().take(MAX_TRACKING_EVENTS) {
                let event_status = event
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("status unknown");
                let description = event
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let occurred_at = event
                    .get("occurred_at")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let _ = write!(out, "  - {occurred_at} {event_status}");
                if !description.is_empty() {
                    let _ = write!(out, ": {description}");
                }
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{body_json, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[test]
    fn renders_quotes_compactly() {
        let value: Value = serde_json::from_str(
            r#"{"quotes":[{"carrier_code":"mock-helthjem","carrier_name":"Helthjem","service_name":"Helthjem Standard","price":{"amount_cents":13300,"currency":"NOK"},"estimated_delivery":"2026-07-06","transit_days":2,"features":["home_delivery"],"reliability_score":null}],"errors":[{"carrier_code":"mock-dhl","message":"timeout"}]}"#,
        )
        .unwrap();
        let s = render_quotes(&value);
        assert!(s.contains("Helthjem — Helthjem Standard: 133.00 NOK"));
        assert!(s.contains("2 day(s) transit"));
        assert!(s.contains("! mock-dhl: no quote (timeout)"));
    }

    #[test]
    fn renders_empty_quotes_honestly() {
        let value: Value = serde_json::from_str(r#"{"quotes":[]}"#).unwrap();
        assert_eq!(
            render_quotes(&value),
            "No carrier returned a quote for this shipment."
        );
    }

    #[test]
    fn renders_carrier_fleet_with_mode() {
        let value: Value = serde_json::from_str(
            r#"{"carriers":[{"code":"mock-bring","name":"Bring","segment":"both","mode":"mock","is_mock":true},{"code":"ups","name":"UPS","segment":"b2b","mode":"sandbox","is_mock":false,"verified_at":null,"degraded_reason":"not verified"}]}"#,
        )
        .unwrap();
        let s = render_carriers(&value);
        assert!(s.contains("Bring (both, demo prices)"));
        assert!(s.contains("UPS (b2b, sandbox, unverified: not verified)"));
    }

    #[test]
    fn renders_tenant_scoped_tracking_events() {
        let value = serde_json::json!({
            "tracking_no": "370000000000000000",
            "current_status": "In transit",
            "events": [{
                "status": "Loaded on vehicle",
                "description": "Parcel accepted",
                "occurred_at": "2026-06-13T10:00:00Z"
            }]
        });
        let output = render_tracking(&value);
        assert!(output.contains("Shipment 370000000000000000: In transit"));
        assert!(output.contains("Loaded on vehicle: Parcel accepted"));
    }

    #[tokio::test]
    async fn mints_scoped_ingestion_token_before_shipping_request() {
        let auth = MockServer::start().await;
        let shipping = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/internal-token"))
            .and(header("x-service-id", "model-execution"))
            .and(header("x-service-api-key", "synthetic-service-credential"))
            .and(body_json(serde_json::json!({
                "orgId": "org-test",
                "scopes": ["shipping:read"],
                "reason": "execution-core shipping read"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "signed-ingestion-token",
                "expiresInSeconds": 300
            })))
            .expect(1)
            .mount(&auth)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/quotes"))
            .and(header("authorization", "Bearer signed-ingestion-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "quotes": []
            })))
            .expect(1)
            .mount(&shipping)
            .await;

        let client = ShippingToolsClient::new_for_test(
            shipping.uri(),
            auth.uri(),
            "model-execution",
            "synthetic-service-credential",
        );
        let input = QuoteInput {
            from: AddressInput {
                name: "Synthetic Sender".to_owned(),
                street: "Testveien 1".to_owned(),
                postal_code: "0001".to_owned(),
                city: "Oslo".to_owned(),
                country: "NO".to_owned(),
                is_business: true,
                phone: None,
                email: None,
            },
            to: AddressInput {
                name: "Synthetic Recipient".to_owned(),
                street: "Testgata 2".to_owned(),
                postal_code: "7010".to_owned(),
                city: "Trondheim".to_owned(),
                country: "NO".to_owned(),
                is_business: true,
                phone: None,
                email: None,
            },
            weight_kg: 1.0,
            length_cm: 10.0,
            width_cm: 10.0,
            height_cm: 10.0,
            dangerous_good: false,
            segment: "b2b".to_owned(),
        };

        let output = client.get_quotes(&input, "org-test").await.unwrap();
        assert_eq!(output, "No carrier returned a quote for this shipment.");
    }

    #[tokio::test]
    async fn booking_forwards_durable_approval_and_idempotency() {
        let auth = MockServer::start().await;
        let shipping = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/ingestion/internal-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "signed-write-token"
            })))
            .expect(1)
            .mount(&auth)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/bookings"))
            .and(header("authorization", "Bearer signed-write-token"))
            .and(header("idempotency-key", "run-test:step-test"))
            .and(body_json(serde_json::json!({
                "quote_ref": "quote-test",
                "carrier_code": "mock-bring",
                "service_name": "Synthetic Service",
                "price": {"amount_cents": 1000, "currency": "NOK"},
                "from": {"name":"Synthetic Sender","street":"Testveien 1","postal_code":"0001","city":"Oslo","country":"NO","is_business":true,"phone":null,"email":null},
                "to": {"name":"Synthetic Recipient","street":"Testgata 2","postal_code":"7010","city":"Trondheim","country":"NO","is_business":true,"phone":null,"email":null},
                "package": {"weight_kg":1.0,"length_cm":10.0,"width_cm":10.0,"height_cm":10.0,"dangerous_good":false},
                "customs": null,
                "booked_by": "user-test",
                "approval_id": "approval-test"
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "booking_id": "booking-test",
                "confirmation_token": "confirmation-test"
            })))
            .expect(1)
            .mount(&shipping)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/bookings/booking-test/confirm"))
            .and(header("authorization", "Bearer signed-write-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "carrier_name": "Bring",
                "booking_ref": "carrier-booking-test",
                "tracking_no": "synthetic-tracking",
                "has_label": false
            })))
            .expect(1)
            .mount(&shipping)
            .await;

        let client = ShippingToolsClient::new_for_test(
            shipping.uri(),
            auth.uri(),
            "model-execution",
            "synthetic-service-credential",
        );
        let input = BookInput {
            quote_ref: "quote-test".to_owned(),
            carrier_code: "mock-bring".to_owned(),
            service_name: "Synthetic Service".to_owned(),
            price_amount_cents: 1000,
            price_currency: "NOK".to_owned(),
            from: AddressInput {
                name: "Synthetic Sender".to_owned(),
                street: "Testveien 1".to_owned(),
                postal_code: "0001".to_owned(),
                city: "Oslo".to_owned(),
                country: "NO".to_owned(),
                is_business: true,
                phone: None,
                email: None,
            },
            to: AddressInput {
                name: "Synthetic Recipient".to_owned(),
                street: "Testgata 2".to_owned(),
                postal_code: "7010".to_owned(),
                city: "Trondheim".to_owned(),
                country: "NO".to_owned(),
                is_business: true,
                phone: None,
                email: None,
            },
            weight_kg: 1.0,
            length_cm: 10.0,
            width_cm: 10.0,
            height_cm: 10.0,
            dangerous_good: false,
            customs: None,
            booked_by: "user-test".to_owned(),
        };

        let result = client
            .book_shipment(&input, "org-test", "approval-test", "run-test:step-test")
            .await;
        assert!(result.is_ok(), "booking failed: {result:?}");
    }
}
