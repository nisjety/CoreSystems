# Shipping Core

Last verified: 2026-07-10

Shipping Core is the Ingestion Plane's carrier-neutral freight service. It owns carrier adapter integration, quote normalization, booking/tracking orchestration, shipping documents, and factual delivery evidence. It must be consumed through plane contracts; it must not become a second identity, tenant, billing, or knowledge authority.

## Current status

The service is functional for authenticated, tenant-scoped rate shopping, including a real Bring production connection. The source is a production-MVP candidate, but it is **not deployed or live-accepted** while Docker rebuilds are blocked.

The 2026-07-10 local Docker verification established:

- `shipping-core` was healthy on host port `3156` and its database readiness check passed.
- Direct and Verevon-gateway carrier/quote requests returned 200.
- Bring production, DHL test, and UPS CIE returned rates.
- FedEx sandbox was configured but returned an authorization error.
- PostNord, DSV, Helthjem and Porterbuddy were explicit local mocks.
- Direct and gateway shipping endpoints were reachable without authentication.
- Current source contains reliability and AI recommendation routes, but the running shipping-core and gateway images returned 404 for both.
- Bring promised-delivery parsing is fixed in source and covered by production-shaped tests; the running image still predates the fix.

Running images are not revision-labelled. At verification, shipping-core's image was created on 2026-07-04 and the gateway image on 2026-07-08. Rebuild and probe the deployment before claiming that a current-source feature is live.

## Ownership and boundaries

Shipping Core owns:

- the normalized carrier adapter interface;
- rate quote fan-out and comparison;
- carrier booking, cancellation, labels, pickup, tracking and manifests;
- shipping reliability calculation from observed delivery history;
- shipping lifecycle events; and
- factual delivery-evidence handoff to Data Plane v2.

Shipping Core does not own:

- users, organizations, roles or entitlements — Control Plane owns them;
- model planning, tool selection or approval policy — Model Plane owns them;
- collaborative UI state — Application/Frontend Plane owns it; or
- durable knowledge/retrieval — Data Plane v2 owns it.

Every content-persisting handoff must propagate tenant ownership and Zero Data Retention posture. Current code does not yet meet that requirement consistently.

## Capability shape

The working-tree source supports:

- concurrent carrier quote fan-out with a per-carrier timeout;
- explicit carrier error reporting rather than silently dropping failures;
- quote validation and cheapest-first ordering;
- a two-step create/confirm booking flow;
- label, customs-document and manifest generation;
- pickup, cancellation and tracking operations;
- append-only booking audit records;
- reliability scoring from real promised-versus-delivered observations;
- optional Model Plane recommendation over returned quotes;
- optional NATS lifecycle events; and
- optional Data Plane delivery-evidence persistence.

Implemented does not mean safely exposed. The router currently has only request logging; no user/session authentication, tenant derivation, role check, rate limiting, or ZDR middleware is mounted.

## Verevon and Model Plane access

There are currently three different access paths:

1. Verevon's Rust gateway proxies `/api/v1/shipping/*` to this service.
2. Model Plane execution-core has carrier, quote, tracking and booking tools.
3. The frontend chat only advertises tools when Browse/actions/tools or Plan mode activates the relevant path.

Normal chat does not automatically receive shipping tools. A healthy Shipping Core therefore does not mean a default Verevon chat turn can use it. The target AI-first contract is one generated shipping action definition shared by the human UI, gateway, Model tool catalog, approval policy, audit and tests.

Booking must remain a high-risk confirmed action. Quote and carrier listing are read-only; create/confirm/cancel/pickup/manifest operations are writes with external consequences.

## Carrier provenance

| Carrier | Activation rule | Default/source behavior | Verified local runtime on 2026-07-10 |
|---|---|---|---|
| Bring | UID, API key and customer number all configured | Production Shipping Guide unless its base URL is explicitly overridden; live booking separately gated | Production rates succeeded. Booking not tested. |
| DHL Express | API key and API secret configured | Production base by default; vendor test base can be selected; live booking separately gated | Vendor test endpoint returned rates. |
| UPS | OAuth client ID and secret configured | Production base by default; CIE can be selected; live booking separately gated | CIE test endpoint returned rates. |
| FedEx | OAuth client ID/secret and account number configured | Production or sandbox selected by base URL; live booking separately gated | Sandbox configured; rate authorization failed. |
| PostNord | Built-in mock | Mock | Mock rate returned. |
| DSV | Built-in mock | Mock | Mock rate returned. |
| Helthjem | Built-in mock | Mock/B2C | Registered; not applicable to the B2B sample. |
| Porterbuddy | Built-in mock | Mock/B2C | Registered; not applicable to the B2B sample. |

`GET /api/carriers` currently derives `is_mock` only from a `mock-` code prefix. Consequently DHL/UPS/FedEx sandbox adapters appear as `is_mock=false`. Consumers must not translate that field into “production.” Replace it with explicit provenance, for example:

```json
{
  "mode": "credential_backed",
  "environment": "sandbox",
  "availability": "verified",
  "verified_at": "2026-07-10T09:12:00Z"
}
```

Do not include credentials, account numbers or sensitive endpoint parameters in this metadata.

## HTTP API

### Live-verified read-only routes

| Method | Path | Purpose | 2026-07-10 deployed result |
|---|---|---|---|
| GET | `/healthz` | Process health | 200 |
| GET | `/readyz` | Database readiness | 200, DB `ok` |
| GET | `/api/carriers` | Carrier inventory | 200 without auth |
| POST | `/api/quotes` | Fan-out rate quote | 200 without auth |

### Present in current source

| Method | Path | Purpose | Deployment note |
|---|---|---|---|
| GET | `/api/carriers/reliability` | Real observed on-time scores | Running image returned 404. |
| POST | `/api/quotes/recommend` | Quote list plus Model recommendation | Running image returned 404. |
| POST | `/api/bookings` | Create an unconfirmed booking | Not live-probed; write operation. |
| GET | `/api/bookings` | List bookings | Not live-probed; contains tenant/PII data. |
| GET | `/api/bookings/{id}` | Booking detail | Not live-probed. |
| POST | `/api/bookings/{id}/confirm` | Confirm with one-time token | Never use as a smoke probe. |
| POST | `/api/bookings/{id}/cancel` | Cancel shipment | Never use as a smoke probe. |
| GET | `/api/bookings/{id}/label` | Label PDF/ZPL/JSON envelope | Not live-probed. |
| GET | `/api/bookings/{id}/customs-document` | Customs PDF/JSON envelope | Not live-probed. |
| POST | `/api/bookings/{id}/pickup` | Schedule pickup | Never use as a smoke probe. |
| GET | `/api/bookings/{id}/tracking` | Refresh/read tracking | Not live-probed; may call carrier API. |
| GET | `/api/tracking/{trackingNo}` | Tenant-scoped tracking lookup by tracking number | Added for Model Plane `track_shipment`; deployment verification pending. |
| GET | `/api/bookings/{id}/audit` | Booking audit trail | Not live-probed. |
| POST | `/api/manifests` | Build end-of-day manifest | Never use as a smoke probe. |
| GET | `/api/manifests/{id}/document` | Manifest PDF | Not live-probed. |

### Quote request

The route validates addresses, ISO alpha-2 countries, package dimensions/weight and `b2b`/`b2c` segment before calling adapters.

```bash
curl -sS http://127.0.0.1:3156/api/quotes \
  -H 'Authorization: Bearer <ingestion-audience-token>' \
  -H 'Content-Type: application/json' \
  --data-binary '{
    "from": {
      "name": "Verevon Test",
      "postal_code": "0150",
      "city": "Oslo",
      "country": "NO",
      "is_business": true
    },
    "to": {
      "name": "Verevon Test",
      "postal_code": "7010",
      "city": "Trondheim",
      "country": "NO",
      "is_business": true
    },
    "package": {
      "weight_kg": 5,
      "length_cm": 30,
      "width_cm": 20,
      "height_cm": 15,
      "dangerous_good": false
    },
    "segment": "b2b"
  }'
```

This is a read-only rate request. It is suitable for a controlled local smoke test. Do not reuse production customer names or addresses in test payloads.

## Bring delivery-time defect (fixed in source, deployment pending)

For the sample above, a sanitized direct Bring production response returned:

| Product | Working days | Expected delivery |
|---|---:|---|
| `3570` | 4 | 2026-07-15 |
| `3584` | 4 | 2026-07-15 |
| `9300` | 2 | 2026-07-14 |

Shipping-core exposed each with `transit_days: 0` and a zero-value date.

Cause:

- Bring returned `expectedDelivery.workingDays` and `expectedDelivery.formattedExpectedDeliveryDate` at the top level.
- `expectedDelivery.alternativeDeliveryDates` was empty.
- `internal/carrier/bring/wire.go` models only `alternativeDeliveryDates`.
- `internal/carrier/bring/bring.go` populates domain delivery fields only from element zero of that array.

Implemented source fix:

1. Add top-level expected-delivery fields to the wire type.
2. Add a production-shaped JSON fixture with an empty alternatives array.
3. Map top-level working days/date first, retaining alternatives only as a documented fallback.
4. Reject or mark an option incomplete when price exists but promised-delivery data required by the user is unavailable.
5. Add an E2E assertion that the Oslo-to-Trondheim response never emits year 1 or `transit_days=0` when Bring provided a promise.

Until the rebuilt image passes the read-only live quote gate, Verevon must still treat the running deployment's zero promise as stale/degraded rather than infer a delivery time.

## Configuration

Copy `.env.example` for local host execution. Go does not load `.env` files automatically.

Required:

- `DATABASE_URL`

Optional groups:

- Runtime: `PORT`
- Bring: `BRING_API_UID`, `BRING_API_KEY`, `BRING_CUSTOMER_NUMBER`, `BRING_CLIENT_URL`, optional API overrides, `BRING_LIVE_BOOKING`
- DHL: `DHL_API_KEY`, `DHL_API_SECRET`, `DHL_ACCOUNT_NUMBER`, `DHL_API_BASE_URL`, `DHL_LIVE_BOOKING`
- UPS: `UPS_CLIENT_ID`, `UPS_CLIENT_SECRET`, `UPS_ACCOUNT_NUMBER`, `UPS_API_BASE_URL`, `UPS_LIVE_BOOKING`
- FedEx: `FEDEX_CLIENT_ID`, `FEDEX_CLIENT_SECRET`, `FEDEX_ACCOUNT_NUMBER`, `FEDEX_API_BASE_URL`, `FEDEX_LIVE_BOOKING`
- Model recommendation: `AUTH_CORE_URL`, `MODEL_GATEWAY_URL`, `INTERNAL_API_KEY`
- Events: `NATS_URL`, `NATS_TOKEN`, `NATS_SUBJECT_PREFIX`
- Data evidence: `DATA_PLANE_DOCUMENTS_URL`, `AUTH_CORE_URL`, `INGESTION_SERVICE_ID`, `SHIPPING_SERVICE_API_KEY`

Never commit populated environment files. A carrier being credential-configured is not enough to call it production-ready; verify the selected endpoint, account permissions and a read-only request.

## Build and test

From this directory:

```bash
go test ./...
go vet ./...
gofmt -l .
go build ./cmd/shipping-core
```

Run directly after exporting a safe local environment:

```bash
go run ./cmd/shipping-core
```

Run in the Ingestion stack from the parent directory:

```bash
docker compose up -d --build shipping-core
docker compose ps shipping-core
curl -sS http://127.0.0.1:3156/healthz
curl -sS http://127.0.0.1:3156/readyz
```

After every rebuild, record and expose the source revision. The current Dockerfile does not apply an OCI revision label.

## Security and release blockers

Remaining release checks (the tenant/auth/storage items below are source-fixed but still require deployed proof):

1. Require a validated session/service identity at the gateway and service boundary.
2. Derive organization and actor identity from that principal; never accept them as authority from request fields.
3. Add organization ownership to bookings, manifests, labels, tracking and audit queries.
4. Enforce role/scope and authoritative approval for every external write.
5. Add idempotency keys for create, confirm, cancel, pickup and manifest operations.
6. Rate-limit quotes and carrier calls by organization/user.
7. Propagate ZDR/retention posture into events and Data Plane evidence.
8. Redact PII, labels, customs content, carrier responses and credentials from logs/errors.
9. Keep live-booking gates fail-closed and test that sandbox credentials cannot reach production hosts.
10. Add cross-tenant negative tests and a deployment smoke that proves auth is enforced.

The two-step confirmation token is a useful workflow guard, but it is not authentication or tenant authorization.

## Safe verification checklist

Allowed for routine smoke:

- health and readiness;
- carrier inventory;
- quote using synthetic names/addresses;
- verification that protected endpoints reject missing credentials once auth is implemented.

Never use as routine smoke:

- booking create/confirm;
- cancellation;
- pickup scheduling;
- manifest creation;
- live label generation; or
- any endpoint that can incur an external carrier charge.

## Priorities

1. Add canonical auth/tenant/approval enforcement.
2. Deploy and live-verify the Bring delivery parsing regression fix.
3. Return explicit live/sandbox/mock provenance.
4. Rebuild revision-labelled shipping and gateway images and verify source/deployment parity.
5. Expose quote/carrier actions through the single Verevon action contract and effective-capability inventory.
6. Add read-only E2E coverage through frontend -> gateway -> Model execution -> shipping-core, then separately test write operations against vendor sandboxes only.
