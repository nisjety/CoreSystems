//! Shipping quote tool for the agentic loop — Velion's freight aggregator.
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

/// Cap on quotes rendered to the model — the engine already sorts
/// cheapest-first, so the head is the interesting part.
const MAX_QUOTES: usize = 10;

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

#[derive(serde::Deserialize)]
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
}

/// Input for book_shipment: the chosen quote's identity + the shipment.
/// `customs` passes through verbatim to shipping-core, which enforces the
/// cross-border requirement server-side.
#[derive(serde::Deserialize)]
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

fn address_json(a: &AddressInput) -> Value {
    serde_json::json!({
        "name": a.name, "street": a.street, "postal_code": a.postal_code,
        "city": a.city, "country": a.country, "is_business": a.is_business,
    })
}

/// HTTP client for shipping-core. Cheap to clone.
#[derive(Clone)]
pub struct ShippingToolsClient {
    base_url: String,
    http: reqwest::Client,
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
        Some(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            http,
        })
    }

    /// POST /api/quotes and render the comparison compactly for the model:
    /// carrier, service, price, transit, plus any per-carrier errors (the
    /// engine reports failed carriers instead of silently dropping them).
    pub async fn get_quotes(&self, input: &QuoteInput) -> Result<String, String> {
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

        let resp = self
            .http
            .post(format!("{}/api/quotes", self.base_url))
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
    pub async fn book_shipment(&self, input: &BookInput) -> Result<String, String> {
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
        });
        let created = self
            .post_json("/api/bookings", &create_body)
            .await
            .map_err(|e| format!("booking create failed: {e}"))?;
        let booking_id = created
            .get("booking_id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("booking create response missing booking_id: {created}"))?
            .to_owned();
        let token = created
            .get("confirmation_token")
            .and_then(Value::as_str)
            .ok_or("booking create response missing confirmation token")?
            .to_owned();

        let confirm_body = serde_json::json!({
            "confirmation_token": token,
            "actor": input.booked_by,
        });
        let booked = self
            .post_json(
                &format!("/api/bookings/{booking_id}/confirm"),
                &confirm_body,
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
        Ok(format!(
            "Shipment BOOKED with {carrier}.\n- Booking id: {booking_id}\n- Carrier ref: {booking_ref}\n- Tracking number: {tracking}\n- Label: {}\nThe label PDF and audit trail are available on the booking.",
            if booked.get("has_label").and_then(Value::as_bool).unwrap_or(false) { "stored" } else { "not available" },
        ))
    }

    async fn post_json(&self, path: &str, body: &Value) -> Result<Value, String> {
        let resp = self
            .http
            .post(format!("{}{path}", self.base_url))
            .json(body)
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
    /// adapters), so the model can answer "which carriers can Velion compare".
    pub async fn list_carriers(&self) -> Result<String, String> {
        let resp = self
            .http
            .get(format!("{}/api/carriers", self.base_url))
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
        let mode = if is_mock {
            "demo prices"
        } else {
            "live agreement prices"
        };
        let _ = writeln!(s, "- {name} ({segment}, {mode})");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

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
            r#"{"carriers":[{"code":"mock-bring","name":"Bring","segment":"both","is_mock":true},{"code":"ups","name":"UPS","segment":"b2b","is_mock":false}]}"#,
        )
        .unwrap();
        let s = render_carriers(&value);
        assert!(s.contains("Bring (both, demo prices)"));
        assert!(s.contains("UPS (b2b, live agreement prices)"));
    }
}
