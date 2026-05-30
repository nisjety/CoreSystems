# Tenancy — Multi-Tenant Isolation in Quarry v2

P0 / cluster #auth+tenancy (shipped after Cycle 19).

This doc explains how Quarry ensures one organization's scrapes,
searches, profiles, and session state are invisible to every other
organization.

## TL;DR

Every `/v1/*` route requires a Bearer JWT issued by Control-Plane's
`auth-core`. The middleware verifies the token against `auth-core`'s
JWKS, extracts `org_id` + `user_id` claims, and injects them into the
request. Handlers ignore any client-supplied `org_id` and use the
verified claim for every data-access call. Backends (Tantivy local
index, profile store, artifact store, event bus) all accept `org_id`
as a required parameter — there is no org-agnostic path on the request
path.

## Auth flow

```
┌────────┐ Bearer JWT ┌──────────────────────────────┐
│ Caller ├───────────►│ quarry-edge `require_auth`   │
└────────┘            │  - decode header             │
                      │  - pin RS256                 │
                      │  - JWKS lookup by `kid`      │
                      │  - validate exp / nbf / aud  │
                      │  - inject `Claims` into req  │
                      └────────────┬─────────────────┘
                                   │  Extension<Claims>
                                   ▼
                      ┌──────────────────────────────┐
                      │ /v1/* handler                │
                      │  - **discard** client org_id │
                      │  - org_id = claims.org_id    │
                      │  - thread through to runtime │
                      └──────────────────────────────┘
```

### JWT shape

Issued by `auth-core` (NestJS, port 3011). Algorithm pinned to RS256.

```jsonc
{
  "sub":     "user_01H...",
  "iss":     "auth-core",
  "exp":     1735689600,
  "org_id":  "org_01HQ...",          // required
  "user_id": "user_01H...",          // required
  "aud":     "quarry-edge",          // checked when AUTH_CORE_AUDIENCE set
  "scopes":  ["search:read", "..."]  // optional, scope-gating ready
}
```

### Verification

`crates/quarry-edge/src/auth.rs` ports the canonical impl from Model
Plane gateway:
- TTL-cached JWKS (default 300 s).
- `kid` miss forces JWKS refresh once (key rotation safe).
- Algorithm hard-pinned to RS256 (blocks `alg=none` and HS256
  alg-confusion against the public key).
- `exp` + `nbf` validated with default 30 s leeway.
- `aud` required by default; opt out with `AUTH_CORE_AUDIENCE_OPTIONAL=1`.

## Data partitioning

| Surface             | Mechanism                                                 | Cross-tenant leakage prevented |
| ------------------- | --------------------------------------------------------- | ------------------------------ |
| Tantivy local index | `org_id` field on every doc; `SearchOptions.org_id` filter | Doc tagged `org_a` invisible to `org_b` search |
| Smart router cache  | `org_id` participates in blake3 cache key                 | Same query from two orgs cached separately |
| Profile store       | Trait takes `&org_id`; key = `{org_id}\0{profile_id}` (memory) / `profiles/{org_id}/{id}.json` (S3) | Cross-tenant load returns `None`; list filters by prefix |
| Browser lease       | `BrowserLease.org_id` carries identity from edge → orchestrator → driver | Profile restore + capture scoped per org |
| Crawl / scrape events | NATS payload includes `org_id` + `user_id`              | Downstream consumers can partition |
| `HostDiscovered` event | Same                                                   | autocomplete-core can scope by org |
| `SearchIssued` event | Same                                                     | Per-org typeahead corpora |

## Configuration

| Env var                          | Default                                          | Meaning |
| -------------------------------- | ------------------------------------------------ | --- |
| `AUTH_CORE_JWKS_URL`             | `http://auth-core:3011/api/convex-auth/jwks`     | Required. JWKS endpoint. |
| `AUTH_CORE_AUDIENCE`             | `quarry-edge`                                    | Required unless `AUTH_CORE_AUDIENCE_OPTIONAL=1`. |
| `AUTH_CORE_ISSUER`               | _(unset)_                                        | Optional. Validates `iss` when set. |
| `AUTH_CORE_JWKS_TTL_SECS`        | `300`                                            | JWKS refresh cadence. |
| `AUTH_CORE_JWT_LEEWAY_SECS`      | `30`                                             | Clock-skew tolerance. |
| `AUTH_CORE_AUDIENCE_OPTIONAL`    | _(unset)_                                        | `1` to skip `aud` check (warn-only). |
| `QUARRY_EDGE_AUTH_DEV_BYPASS`    | _(unset)_                                        | `1` accepts any bearer + injects stub claims. **NEVER in production.** |

### Cross-plane event fan-out (P1 / cluster #nats)

`EventSink` publishes to NATS JetStream when configured. Subject layout:

```
quarry.run.<run_id>.<event_type>     # per-run topic
quarry.events.<event_type>           # aggregate topic
```

Stream `QUARRY_EVENTS` is auto-created with 7-day retention. Cross-plane
consumers (autocomplete-core / org-core / ai-core) subscribe with their
own pull consumers. Events carry the verified `org_id` and `user_id` in
the payload so subscribers can partition by tenant.

| Env var                              | Default                          | Meaning |
| ------------------------------------ | -------------------------------- | --- |
| `QUARRY_EDGE__NATS_URL`              | `nats://quarry-nats:4222`        | Empty → disable fan-out (mpsc-only). |
| `QUARRY_EDGE__NATS_TOKEN`            | _(unset)_                        | Token auth (mutually exclusive with creds file). |
| `QUARRY_EDGE__NATS_CREDS_FILE`       | _(unset)_                        | Path to `.creds` (NATS user JWT + nkey). |
| `QUARRY_EDGE__NATS_SUBJECT_PREFIX`   | `quarry`                         | Stream subject root. |

NATS failure is non-fatal: connect errors fall back to mpsc-only;
runtime publish errors are warn-logged and the local handler returns
normally. The control-plane HTTP publisher remains the authoritative
durable channel.

## Public surface

| Path           | Authenticated? |
| -------------- | -------------- |
| `GET /health`  | No (LB / k8s probes) |
| `GET /ready`   | No (LB / k8s probes) |
| `POST /v1/*`   | **Yes**, every route |

## Security properties

1. **Client cannot impersonate.** `org_id` and `user_id` fields on JSON
   request bodies are ignored — even if a malicious client sends
   `{"org_id":"victim"}`, the handler overwrites it with the verified
   claim.
2. **Cross-tenant search impossible.** The Tantivy `SearchProvider`
   impl routes through `search_with_org` whenever `SearchOptions.org_id`
   is set, applying a `Term::from_field_text(org_id, …)` filter to the
   tantivy query. The smart router caches results keyed on the org so
   tenants share neither result lists nor cache entries.
3. **Public-web SERPs ignore org_id.** Brave/Serper/SearXNG/Stract
   search the public web — there is no per-tenant concept to leak.
4. **Profile data partitioned at storage.** S3 paths are
   `profiles/{org_id}/{id}.json`; in-memory keys use a NUL separator.
   Cross-tenant `list()` returns an empty vec. Cross-tenant `delete()`
   does not remove another org's profile.
5. **Future-proofed for billing.** `claims.user_id` is captured in
   crawl handoff and search-issued event payloads so billing-core can
   meter per user *and* per org.

## What's still pending (not P0 scope)

- **Scope enforcement.** `Claims.has_scope()` is implemented but no route
  currently calls `require_scope()`. Add per-route scope gating when we
  have scope semantics (`crawl:write`, `profile:write`, etc.) defined
  by org-core.
- **Service-to-service auth.** Orchestrator → edge currently uses
  `QUARRY_EDGE__RUNTIME_AUTH_TOKEN` (static shared secret). Migrate to
  `INTERNAL_API_KEY` header + Internal-issued JWT.
- **Billing hooks.** Capture `{org_id, user_id, kind, cost_units}` per
  request and emit to `billing-core`.
- **Audit log.** Persist `{ts, claims, route, status}` for SOC2 trail.

## Tests

Auth + tenancy behavior is verified by:

- `crates/quarry-edge/src/auth.rs` — 5 tests (missing bearer 401,
  dev-bypass accept, malformed token 401, claims serde, has_scope).
- `crates/quarry-browser/src/session.rs` —
  `in_memory_store_isolates_orgs` (cross-tenant load + list + delete
  isolation).
- `crates/quarry-runtime/src/local_index.rs` — existing
  `search_with_org` tests prove org-A docs invisible to org-B query.
- `crates/quarry-runtime/src/s3_profile_store.rs` —
  `index_prefix_isolates_orgs` and key-shape tests.
