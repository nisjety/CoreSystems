# documents-api-go Research Dive

Generated: 2026-07-10 (supersedes 2026-06-07 pass; re-verified live against the running `dpv2-documents-api` container, its Postgres, and a real Control Plane auth-core session)

Scope: `apps/Data Plane v2/services/documents-api-go`

## Secure-MVP current state — 2026-07-10

- **Implemented:** JWT signature, issuer, audience, expiry, and JWKS verification
  are enforced by default; production posture fails startup without verification
  material. Tenant/owner identity is claim-pinned. The shared inbound internal-key
  bypass is removed, service principals require explicit document scopes, and
  viewer-less service reads no longer silently broaden private visibility.
- **Implemented ZDR/privacy:** single and bulk ingest share restrictive durable-
  write rejection. Cross-user idempotency-key reuse returns a generic conflict
  rather than another user's document result.
- **Tested:** `go test -race ./...` and `go vet ./...` passed; `govulncheck`
  reported no reachable vulnerability. The changed `pkg/authctx` package measured
  95.4% statement coverage in the final profile, including JWKS rotation/outage,
  startup posture, verified-user-proof retention, and serialization safety.
- **Built/deployed/reachable/effective:** an earlier checkpoint image built with
  verified revision/build labels. The later user-proof/outbox source has not
  been rebuilt because Docker's content store is failing reads; it has not been
  deployed or exercised by the isolated bearer/ZDR matrix. Earlier observe-mode/
  internal-key findings below are historical deployment evidence, not proof
  about current source behavior.
- **Containment/blocker:** the unsigned GDPR NATS consumer is disabled by default
  behind two insecure-development gates. GDPR async handling remains ineffective
  until signed, scoped events and NATS authorization exist.

The remainder is a superseded, sanitized pre-fix audit retained for root-cause
history. Do not copy its old deployment defaults into new configuration.

## Historical snapshot (superseded for current state)

`documents-api-go` is the ingest and document-metadata authority for Data Plane v2. It owns document CRUD, bulk ingest, source-object lifecycle, duplicate inspection, and the canonical document event outbox.

Headline change since the 2026-06-07 dive: **the JWT/JWKS verification that doc used to call "explicitly unimplemented" is no longer a stub.** `pkg/authctx/verify.go` (added 2026-07-07, commit `9a9ac7fc`) is a complete RS256 verifier — static-key + JWKS-by-`kid` lookup, audience/issuer/expiry checks, a 6-test unit suite plus a 2-scenario middleware suite, all of which pass live (`go test ./pkg/authctx/... -v`, all green, re-run in this pass). The package's own top-of-file doc comment (lines 1-46 of `authctx.go`) still says "the Verify path is intentionally stubbed... returns ErrNotImplemented" — that comment is now flatly wrong and should be deleted or rewritten; do not trust it.

What has **not** changed: the verifier is wired but not turned on. `AUTHCTX_ENFORCE` defaults to `0` (observe mode) in both `docker-compose.yml` and the live container's env. In observe mode the service decodes whatever JWT is handed to it without checking the signature, and authorization is still 100% driven by the caller-supplied `X-Org-ID` header plus a single fleet-shared `INTERNAL_API_KEY`. This was live-verified in this pass (see "Live Verification" below): a cryptographically genuine, freshly-minted, correctly-signed data-plane JWT and an outright garbage string in the `Authorization` header produce **byte-identical** responses. The code *can* tell them apart (enforce mode does, per the test suite); the deployed default configuration does not ask it to.

Non-generated, non-vendored file count: 22 `.go` files (excluding `_test.go`), ~3,300 lines; ~4,350 lines including tests.

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go`
  - loads config, opens Postgres + NATS, fails closed at boot if `INTERNAL_API_KEY` is empty (unless `ALLOW_INSECURE_DEV_DEFAULTS=1` — this fail-closed boot check was added 2026-07-07, commit `49dc5720`, and is confirmed present in the live binary via `strings` extraction, see below)
  - wires `internalAuthMiddleware` → `authctx.Middleware` → `handler.OrgIDMiddleware`, in that order, on every `/v1/documents`, `/v1/sources`, `/v1/source-objects` route
  - starts the outbox publisher loop (`FOR UPDATE SKIP LOCKED`, replica-safe)
- `internal/handler/*` — document and source-object HTTP handlers
- `internal/repo/*` — canonical document and source-object persistence
- `internal/events/*` — document lifecycle publication (outbox pattern)
- `internal/userauthz/*` — per-user resource-grant facade client (user-core)
- `internal/gdpr/*` — cross-plane erasure/ownership-transfer subscriber (shared NATS bus)
- `pkg/authctx/{authctx,verify}.go` — the JWT/JWKS auth-context middleware (see deep dive below)
- `pkg/usagepub/usagepub.go` — usage/audit publisher, constructed in `main.go` but **still not forwarded to any handler call site** (`_ = usagePublisher` at `cmd/main.go:108`, unchanged since 2026-06-07)

Primary HTTP surface:

- `/v1/documents` (GET list, POST create, POST `/bulk`, GET/DELETE `/{documentID}`)
- `/v1/sources` (GET)
- `/v1/source-objects` (GET `/duplicates`, POST create, POST `/delete`)
- `/health`, `/readyz`, `/metrics` — historically unguarded ops endpoints; health
  returned 200, with body omitted.

## API And Relationship Map

Current relationships:

- Ingestion Plane -> `documents-api-go` — canonical synchronous document write path (`Quarry-v2`'s `ingest_client.rs` posts to `/v1/documents` and `/v1/documents/bulk`, forwarding `initiator_user_id`/`visibility` as `X-User-Id` + body fields — see the cross-plane contract note below)
- `documents-api-go` -> Postgres — document authority and source-object state
- `documents-api-go` -> NATS — lifecycle events via outbox
- `documents-api-go` -> downstream Data Plane services — `index-engine-rs` consumes its document progression events
- `documents-api-go` -> `user-core` (`USER_CORE_URL`, port 3012) — per-viewer resource-grant resolution (`internal/userauthz`), fails open to owner+org/shared visibility on error

## Auth Boundary Deep Dive — `pkg/authctx`

Three layers stack on every guarded route, in this exact order (`cmd/main.go:152-181`):

1. **`internalAuthMiddleware(cfg.InternalAPIKey)`** — constant-time compare against a single `INTERNAL_API_KEY` shared across every internal caller in the fleet (confirmed identical value on `dpv2-documents-api` and `auth-service` in this pass: `<INTERNAL_API_KEY-redacted>...redacted`). No caller-specific identity here at all — it is a fleet membership card, not a credential tied to a tenant, user, or even a specific calling service.
2. **`authctx.Middleware`** — observe mode by default (`AUTHCTX_ENFORCE=0`). Decodes `Authorization: Bearer <jwt>` **without verifying the signature**, logs a warning if the decoded (unverified) `org_id` disagrees with `X-Org-ID`, and always calls `next.ServeHTTP` regardless of what it found (or didn't find).
3. **`handler.OrgIDMiddleware`** — pulls `X-Org-ID` straight off the header (400 if absent) and stamps it into context. This is what every downstream repo query actually scopes on.

`pkg/authctx/verify.go` implements the enforce path in full:

- RS256 signature check via `github.com/golang-jwt/jwt/v5`, key resolved by `kid` from a JWKS cache (`AUTH_CORE_JWKS_URL`, refreshed on cache-miss with a 30s floor) or a static `JWT_PUBLIC_KEY_FILE` fallback.
- Audience pinned to `data-plane`, issuer pinned to `http://.../api/convex-auth`, 30s leeway on expiry (matches the mint-side `REFRESH_SAFETY_MS`).
- Rejects: wrong audience, wrong issuer, expired token, token signed by the wrong key, and a token with an empty `org_id` claim.
- Enforce-mode middleware behavior (test-proven, re-run in this pass, all green): no token → 401; valid token → 200; valid token whose verified `org_id` disagrees with a caller-supplied `X-Org-ID` → 403 (re-stamps `X-Org-ID` from the *verified* claim so downstream code can't be tricked by a mismatched header); enforce flipped on with no key material available → 503 fail-closed, never fail-open.

Both `JWT_PUBLIC_KEY_FILE` (mounted read-only from `services/retrieval-engine-rs/keys`) and `AUTH_CORE_JWKS_URL` are already configured with real values on the live container — enforcing this is a one-line env flip (`AUTHCTX_ENFORCE=1`), not a development task. `docker-compose.yml`'s own comment (line 572-574) says as much: *"flip to 1 only once the gateway mints data-plane Bearer tokens on every leg (it now does) and the cross-tenant isolation tests pass."* Whether that isolation-test gate has actually been run is outside this service's own tree; from documents-api-go's side, the prerequisite work is done and idle.

**Net effect**: the "JWT/JWKS not implemented" framing from the 2026-06-07 doc and the 2026-07-10 baseline handed into this pass is now inaccurate for documents-api-go specifically. The implementation is done, tested, and deployed. The gap that remains is a **configuration/rollout gap**, not an engineering gap — but its live blast radius is identical to what an implementation gap would produce, because the safer path is switched off by default. See Live Verification for the concrete exploit.

## Stubs, Placeholders, TODO/FIXME Grep

Full-tree grep for `TODO|FIXME|mock|stub|fake|placeholder|not implemented|unimplemented` across non-test `.go` files:

- `pkg/authctx/authctx.go:1,22-23` — package doc comment claims the Verify path is "intentionally stubbed" and "returns ErrNotImplemented." **Stale**: `verify.go` fully implements it. This comment should be corrected; it actively misleads anyone reading the package doc without also reading `verify.go`.
- Everything else that matched (`fakeTransferrer`, `fakePublisher` in `internal/gdpr/subscriber_test.go`) is legitimate test-double naming, not production stub code — no other stub/mock/placeholder markers exist in non-test source.
- `pkg/usagepub` is instantiated in `main.go` but no handler forwards events to it (`_ = usagePublisher`, unchanged from the prior pass — this is a real, still-open gap, just not a security one).

## Live Verification (this pass, 2026-07-10)

Container: `dpv2-documents-api`, up 14h, `healthy`, port 8010. Confirmed the running binary is current (not stale) by extracting embedded strings from the container's `/usr/local/bin/documents-api` — it contains the exact fail-closed boot messages from the 2026-07-07 `49dc5720` security-hardening commit ("INTERNAL_API_KEY is required (set ALLOW_INSECURE_DEV_DEFAULTS=1...)"), so the deployed binary is not behind the current source tree in any way that matters here.

**1. Health/readiness** — `GET /health` and `GET /readyz` returned 200; response
bodies are omitted.

**2. Unauthenticated access** — `GET /v1/documents` with zero headers, a tenant
header alone, or a wrong internal credential returned 401. Bodies are redacted.

**3. Control authentication fixture** — the pre-fix pass created and verified a
disposable user through supported Control APIs, then minted a short-lived,
correctly signed data-plane token. User identifiers, OTP material, session values,
tenant identifiers, and response bodies are intentionally omitted.

**4. Real signed token vs. forged token vs. no token — identical outcome (this is the core finding).** All three requests used the same valid `X-Internal-Api-Key` and the same `X-Org-ID: [redacted-org]` (a different, real org than the one in the JWT's claims — i.e. the header and the JWT actively disagree):

| Authorization header | HTTP status | Response |
|---|---|---|
| Genuine RS256 JWT with a different tenant claim than the header | 200 | Returned the header-selected tenant's documents; response body redacted |
| Malformed bearer | 200 | Same response shape; body redacted |
| No `Authorization` header | 200 | Same response shape; body redacted |

In the deployed default configuration, documents-api-go **cannot and does not** distinguish a cryptographically valid, freshly-minted Control-Plane-issued credential from an arbitrary garbage string or no credential at all. Authorization is determined entirely by whichever `X-Org-ID` value the caller chooses to send, gated only by possession of the one fleet-wide `INTERNAL_API_KEY`. Since that key is shared identically across `auth-core`, `documents-api-go`, and (per prior audits) most of the rest of the fleet, **any service or leaked value of that single key is sufficient to read any org's documents**, with zero user-level authentication.

**5. Confirmed against populated data, not just an empty fixture**: the historical
request returned stored tenant documents. Customer name, tenant ID, URL, document
text, counts, and body are redacted.

**6. A second, compounding finding surfaced by the same test**: without a verified
viewer, the historical implementation skipped ownership filtering and returned
private rows belonging to other users. Owner identifiers and row content are
redacted. The current source removes this viewer-less privilege expansion.

**7. ZDR bulk-ingest bypass — reproduced live with a stored row as proof.** `internal/handler/documents.go:189` rejects single-document `POST /v1/documents` with `403` when `ingest_policy.zdr_mode="on"` and `content` is non-empty. `BulkIngest` (`POST /v1/documents/bulk`, same file, lines 269-358) has no equivalent check anywhere in its per-document loop.
- Single-create with `{"ingest_policy":{"zdr_mode":"on"}, "content": "..."}` → `403`, correctly rejected.
- The equivalent bulk payload returned 200 and a scoped database check confirmed
  durable persistence. Fixture ID, organization, content, and response body are
  redacted. The test-created row was removed through the verified cleanup path.
  Current source applies the shared restrictive-ZDR guard to both paths.

**8. Cross-plane contract drift claim re-checked and found stale.** The 2026-07-10 baseline handed into this pass claimed "Quarry-v2 cargo test fails because `DataPlaneIngestRequest` constructors lack `initiator_user_id` and `visibility` fields." Both fields exist in `Quarry-v2/crates/quarry-core/src/contracts.rs` (`initiator_user_id: Option<String>`, `visibility: Option<String>`, both documented as forwarding to documents-api-go's `owner_id`/`visibility` stamping) and are wired through `quarry-runtime/src/ingest_client.rs` and `pipeline.rs`. Ran live: `cargo test --workspace --no-run` across all of Quarry-v2 compiles clean, and `cargo test -p quarry-core --test contracts` passes all 21 tests including `data_plane_ingest_request_serde_roundtrip` and `data_plane_ingest_request_omits_absent_ownership_fields`. This gap is closed; the baseline note is out of date and should not be carried forward.

## Build/Test/Fmt Status (this pass)

- `go build ./...` — clean.
- `go vet ./...` — clean.
- `go test ./... -count=1` — all packages pass, including the full `pkg/authctx` suite (11 tests: 6 `Verify()` unit tests + 2 middleware scenarios + boundary cases, all green) and `internal/repo`'s integration/ownership tests.
- `gofmt -l .` — **not clean**: `internal/config/config.go` and `internal/repo/document_repo.go` have formatting drift. Consistent with the "Go fmt drift in the 4 Go services" baseline note, now pinned to these two specific files for this service.
- All 15 `dpv2-*` containers confirmed `healthy` in this pass (documents-api, retrieval-engine, graph-index, embedding-engine, index-engine, quickwit-adapter, data-quality, data-orchestrator, wiki-store, quickwit, postgres, qdrant, nats, minio, dragonfly) — matches the baseline list.

## Duplicates, Redundancies, And Inactive Surfaces

- The auth boundary is still split three ways (internal API key / observe-mode JWT decode / legacy org header), same as the 2026-06-07 read — except it is no longer "transitional pending an unbuilt verifier," it is "transitional pending a flag flip on an already-built verifier." The duplication itself is unchanged; only its remaining cost changed.
- `pkg/usagepub` remains constructed-but-unconsumed, unchanged since 2026-06-07.
- Old shared-NATS `quarry.documents.crawled` subscriber path remains intentionally removed in favor of synchronous HTTP ingest (unchanged).
- `internal/config/config.go`'s `UserCoreURL` fallback default is `http://user-core:8080` (stale port — the real facade port is 3012, per its own doc comment two lines above and per the memory note "HTTP/facade port is 3012, NOT 8080"). Harmless today because `docker-compose.yml` always sets `USER_CORE_URL` explicitly, but the hardcoded fallback would silently misroute if that env var were ever dropped.

## API Design And Performance Notes

- Consolidating document writes into HTTP here remains the right ownership boundary; duplicate detection and source lifecycle sharing persistence context with document routes is still coherent.
- Outbox publish loop (`FOR UPDATE SKIP LOCKED`) is still replica-safe and appropriate.
- The single biggest operational risk is no longer "auth transition logic" in the abstract — it is concretely: **the fleet-shared internal key plus default observe-mode authctx together make org and per-user visibility boundaries advisory, not enforced, for this service's entire HTTP surface.** Flipping `AUTHCTX_ENFORCE=1` closes the JWT-vs-forgery gap (point 4 above) but does **not** by itself close the internal-key breadth-of-access problem (a caller can still omit the Bearer token or omit `X-User-Id`; enforce mode 401s a missing Bearer token, which is actually stronger than today, but the private-doc leak in point 6 is a `viewerID()` design choice independent of authctx entirely) or the ZDR bulk bypass (point 7, unrelated to authctx).

## Current Doc Cleanup Read

Keep:
- `DATA_PLANE_DEEP_DIVE.md`
- `docs/migration-v1-to-v2.md`

Update or archive, not delete:
- `docs/gap-data.md` — still useful as a historical completion ledger; overstates some closed areas versus current runtime, same caveat as before.
- Any doc (including this one's own prior 2026-06-07 revision, and the 2026-07-10 baseline handed into this pass) that repeats "authctx JWT/JWKS verification is not implemented" — that claim is now false for this service and should not be propagated further without a "as of when" qualifier.

## Historical bottom line (superseded)

`documents-api-go` is real and central, and its authctx story is in better shape than the last two passes credited it: the verifier is built, tested, and deployable with a single env flip, not a future engineering task. What remains open, in order of severity:

1. **Live, exploitable**: any caller holding the one fleet-shared `INTERNAL_API_KEY` reads any org's documents (and, absent an `X-User-Id`, any user's *private* documents within an org) by simply choosing an `X-Org-ID` value — a real JWT changes nothing in the current default config. Fix is operational (`AUTHCTX_ENFORCE=1`, keys already staged) for the org-crossing half; the private-doc-visibility half needs its own decision (should a viewer-less caller ever see `private` docs, or should that fallback be removed/scoped to system-only source types).
2. **Live, exploitable, compliance-relevant**: `POST /v1/documents/bulk` silently persists `zdr_mode=on` content that the single-document path correctly rejects — reproduced with a stored Postgres row in this pass.
3. **Documentation debt, not a functional bug**: `authctx.go`'s package doc comment still describes the verifier as an unimplemented stub; fix the comment so the next audit doesn't have to re-discover this from `verify.go` and `verify_test.go` directly.
4. **Unchanged from 2026-06-07**: `pkg/usagepub` built but not wired into any handler.
5. **Resolved, drop from future baselines**: the Quarry-v2 `DataPlaneIngestRequest` cross-plane contract drift (`initiator_user_id`/`visibility` fields) — both fields exist, both are wired, all 21 `quarry-core` contract tests pass live.
6. **Cosmetic**: `gofmt` drift in `internal/config/config.go` and `internal/repo/document_repo.go`.

## 2026-07-11 secure-MVP delta (current)

The historical bottom line above is superseded. Strict RS256/JWKS verification,
tenant/owner pinning, single+bulk restrictive-ZDR rejection, and atomic signed
JetStream outbox publication are implemented and tested. Explicit-grant lookup
now forwards the original already-verified user bearer in memory; User Core must
independently verify that proof. Outbox retries use a stable `Nats-Msg-Id`.
`go test -race ./...`, vet, build, and `govulncheck ./...` pass; `pkg/authctx`
measures 95.4%. Current image/runtime effectiveness remains Docker-blocked.
