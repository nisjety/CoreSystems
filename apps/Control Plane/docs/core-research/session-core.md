# session-core Research Dive

Generated: 2026-06-07
Updated: 2026-07-15 (scoped runtime credentials and static re-verification)

Scope: `apps/Control Plane/session-core` (Go, HTTP `:3017`, gRPC `:50017`, container `session-core-service`)

> Do not confuse with **Model Plane**'s Rust `session-core` (`model-plane-session-core-1`, `:9091`/`:18081`), which owns plans/todos/lineage/approval state. This doc is the Control Plane legacy bridge / Control Session aggregator only.

## 2026-07-15 final secure-MVP addendum (current)

Session has no active inbound gRPC path that needs a legacy shared fleet key; the unused release startup/config requirement was removed. Its production service resets the developer `env_file`, keeps insecure defaults disabled, and uses distinct scoped Org/Billing/User and Control/shared-NATS credentials. Browser identity remains token-verified and gateway service delegation remains method/URI/body/subject-bound; `X-User-Id`, `X-Org-Id`, and `X-User-Role` have no independent authority.

Full `go test ./...` and `go vet ./...` pass. The prior live direct/gateway forged-token matrix remains the deployed evidence; no real session aggregate was read or mutated during this final isolated continuation. Production credential injection/rotation remains the operator-owned release gate.

## 2026-07-15 scoped-runtime detail (superseded by final addendum above)

Session's fail-closed browser and signed Gateway delegation contracts remain unchanged and green. Its Org/Billing/User callers consume distinct scoped credentials and never follow redirects; the release path uses separate Control/shared NATS principals, disables token fallback, and grants no runtime topology administration. Full `go test ./...`, `go vet ./...`, and the earlier race pass remain green. This final continuation did not recreate the existing Session container or exercise real session data; production credential rotation remains operator-owned.

## 2026-07-14 live re-verification (historical deployment evidence)

The Docker incident described in the older addendum is no longer active. Session Core and the rebuilt Velion gateway are healthy and their signed delegation pair is live. Missing browser auth and `Bearer garbage` combined with forged `X-User-Id`, `X-Org-Id`, and `X-User-Role: admin` return 401 both through the gateway `/api/v1/session/bootstrap` endpoint and directly from Session Core's `/api/v1/sessions/current` aggregate; no user aggregate is returned. The broader automated regressions below continue to cover malformed, expired, wrong-issuer, wrong-audience, wrong-signature, and header-impersonation cases.

## 2026-07-11 production-readiness addendum (historical)

The critical bearer/header impersonation described below is fixed, covered, deployed, and live-verified. Session Core has no header-fallback identity path. Browser bearer tokens are round-tripped to Auth Core and checked for signature, expiry, issuer, audience, and claims. Machine access uses a separate audience/scope-bound service credential path. Session's outbound User Core self-profile reads now carry the same subject/profile/body-bound HMAC delegation contract as the gateway; an unsigned service token is insufficient.

The final review also found Session's **inbound** gateway path still accepted a scoped static token plus caller-selected `X-User-Id`. A failing regression reproduced the 200 response. Session now verifies a 30-second HMAC envelope bound to configured principal/audience, timestamp, method, URI, body digest, and user/profile claims before setting identity. Unsigned delegation returns 403 in tests; Go and Rust share the fixed vector. The Session image was recreated, but the matching gateway build failed on Docker/BuildKit storage I/O and Session became unhealthy when Docker lost consistent Postgres metadata. Treat this pair as source-complete but not live-accepted.

Live results on `/api/v1/sessions/current`: missing bearer plus caller identity headers -> 401; `Bearer garbage` plus real `X-User-Id`, forged `X-Org-Id`, and asserted admin -> 401; forged JWT -> 401. No denial body contained the fixture identity. The Velion gateway bootstrap route returns the same 401 behavior. Automated regressions cover missing, malformed, expired, wrong issuer/audience/signature, and header impersonation cases.

`go test ./...` and `go vet ./...` pass. Measured whole-service coverage is 20.2%; `internal/internalkey` is 100% and the HTTP package 38.0%. The remaining legacy command ownership cases and broader HTTP coverage must be proven before MVP acceptance.

## 2026-07-10 Update Summary

- Container confirmed healthy: `session-core-service` up 13h, ports `0.0.0.0:3017->3017`, `0.0.0.0:50017->50017`.
- No uncommitted working-tree changes exist under `apps/Control Plane/session-core` (`git status --porcelain` and `git diff --stat` both empty). The large in-flight WIP diff currently in the tree (auth-core `organization-events.plugin.ts`, org-core/user-core/billing-core changes) does **not** touch this service.
- TODO/FIXME/mock/stub/fake/placeholder grep: no live mocks, stubs, or fakes in non-test source. All hits are either (a) historical "G36-cutover" decommission comments about the removed plans/todos/lineage surface, (b) the `Todo*` domain enum/struct names left over from that removed surface, or (c) the *legitimate* placeholder-secret detector in `internal/internalkey/assert.go` (its job is to detect placeholder keys, so `placeholder` appearing there is expected, not a code smell). `app/{api,clients,models,services}` is a set of four empty leftover directories (0 files) — worth deleting for hygiene but not a functional issue.
- **The prior audit's flagged item — "invalid bearer/header impersonation behavior in session-core" — is CONFIRMED LIVE as a CRITICAL authentication-bypass vulnerability.** See "Live Security Verification" below for exact requests/responses. It is not a hypothetical: a forged, unverified `Authorization: Bearer` value combined with a self-asserted `X-User-Id` header is sufficient to authenticate as **any** user, including on the Control Session snapshot endpoint that Velion v3's frontend depends on for session bootstrap (user profile, entitlements, onboarding status).
- No automated test exists anywhere in the service (`tests/` directory is empty, `scripts/smoke_test_api.sh` doesn't cover auth) to catch or prevent this. This is a real, unguarded gap, not a regression risk that CI would catch today.

## Live Security Verification (2026-07-10)

Target: `http://localhost:3017` (host-exposed port of `session-core-service`, confirmed healthy before testing).

Relevant code — `internal/http/server.go`, `authContextMiddleware()`:

```go
func authContextMiddleware() gin.HandlerFunc {
	configuredKeys := []string{
		strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")),
		strings.TrimSpace(os.Getenv("INTERNAL_SERVICE_SECRET")),
	}
	return func(c *gin.Context) {
		if c.Request.URL.Path == "/health" || c.Request.Method == http.MethodOptions {
			c.Next()
			return
		}
		// Internal service-to-service auth
		if reqKey := c.GetHeader("X-Internal-Api-Key"); reqKey != "" {
			for _, key := range configuredKeys {
				if key != "" && reqKey == key {
					c.Set("user_id", c.GetHeader("X-User-Id"))
					...
					c.Next()
					return
				}
			}
		}
		// Bearer token auth
		if authHeader := c.GetHeader("Authorization"); strings.HasPrefix(authHeader, "Bearer ") {
			// Forward to auth-core via session.validate NATS or direct HTTP
			// For now, extract user-id from forwarded headers (frontend proxy sets these)
			if userID := c.GetHeader("X-User-Id"); userID != "" {
				c.Set("user_id", userID)
				c.Set("auth_method", "bearer")
				c.Next()
				return
			}
		}
		c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
	}
}
```

The comment on the bearer branch is accurate and damning: the token is never sent to auth-core, never signature-checked, never expiry-checked. Any non-empty string after `Bearer ` combined with any `X-User-Id` header value is accepted as a fully authenticated identity.

### Test 1 — baseline, no auth at all

```
$ curl -i -X POST http://localhost:3017/v1/sessions -H "Content-Type: application/json" -d '{}'
HTTP/1.1 401 Unauthorized
{"error":"authentication required"}
```
Correctly rejected.

### Test 2 — forged `X-Internal-Api-Key` (wrong value) + spoofed user headers

```
$ curl -i -X POST http://localhost:3017/v1/sessions \
    -H "X-Internal-Api-Key: totally-forged-not-the-real-key-12345" \
    -H "X-User-Id: victim-user-999" -H "X-User-Email: victim@example.com" \
    -d '{}'
HTTP/1.1 401 Unauthorized
{"error":"authentication required"}
```
Correctly rejected — the internal-key branch does require an exact match against `INTERNAL_API_KEY`/`INTERNAL_SERVICE_SECRET`, so a wrong/guessed internal key is not exploitable on its own.

### Test 3 — forged/invalid Bearer token + spoofed `X-User-Id` on the legacy session-creation route

```
$ curl -i -X POST http://localhost:3017/v1/sessions \
    -H "Authorization: Bearer this-is-a-completely-invalid-forged-jwt-token-xyz" \
    -H "X-User-Id: victim-user-999" -H "X-User-Email: victim@example.com" \
    -H "X-User-Name: Victim Impersonated" \
    -d '{"org_id":"some-org-id","initial_message":"impersonation test"}'
HTTP/1.1 400 Bad Request
{"error":"invalid request body","details":"Key: 'CreateSessionRequest.TenantID' Error:Field validation for 'TenantID' failed on the 'required' tag\nKey: 'CreateSessionRequest.WorkspaceID' Error:Field validation for 'WorkspaceID' failed on the 'required' tag"}
```
This is **not** a 401. The request cleared `authContextMiddleware` entirely — it failed downstream on Gin's struct-binding validation (missing `tenant_id`/`workspace_id`), proving the forged bearer token + spoofed identity was accepted as authenticated.

### Test 4 — same forgery, fully-shaped body

```
$ curl -i -X POST http://localhost:3017/v1/sessions \
    -H "Authorization: Bearer forged.invalid.jwt.token.not-checked-anywhere" \
    -H "X-User-Id: victim-user-999" -H "X-User-Email: victim@example.com" \
    -d '{"tenant_id":"forged-tenant","workspace_id":"forged-workspace","org_id":"forged-org-id","plan_mode":false}'
HTTP/1.1 403 Forbidden
{"error":"org membership required"}
```
Confirms the auth gate was passed (no 401); the only thing that stopped session creation here was a *downstream, resource-specific* org-membership check (`orgClient.ValidateMembership` in `SessionService.CreateSession`) rejecting the fabricated `org_id`/`victim-user-999` pairing against org-core. That membership check exists solely for `CreateSession` — see gap below.

### Test 5/6 — the actively-used Control Session aggregator endpoint (`GET /api/v1/sessions/current`)

```
$ curl -i http://localhost:3017/api/v1/sessions/current
HTTP/1.1 401 Unauthorized
{"error":"authentication required"}

$ curl -i http://localhost:3017/api/v1/sessions/current \
    -H "Authorization: Bearer totally-forged-not-a-real-jwt" \
    -H "X-User-Id: velion-v3-local-user" \
    -H "X-User-Email: local@velion.dev"
HTTP/1.1 200 OK
{"user":{"id":"velion-v3-local-user","email":"user-velion-v3-local-user@placeholder.local","name":"User","onboardingComplete":false},"entitlements":[],"onboardingStatus":"CREATED","fetchedAt":"2026-07-10T09:58:58.645709962Z"}
```
**This is the clean, fully-successful exploit.** `getControlSessionCurrent` (`internal/http/control_session_handlers.go`) has no additional resource-ownership check beyond `userIDFromContext(c)` — it trusts whatever `user_id` the middleware set, then calls `s.controlSessionService.Get(ctx, userID)` and returns a full `200 OK` snapshot. A forged bearer token plus a guessed/known `X-User-Id` value is sufficient, on its own, to pull another user's aggregated Control Session profile (user record, entitlements, onboarding status) with zero token verification anywhere in the path. `velion-v3-local-user` is the known dev/superadmin identity (see `MEMORY.md` "Velion super-admin"), used here only as a non-destructive read-only proof value already documented as existing in this environment.

### Verdict

**CONFIRMED, not hypothetical.** `authContextMiddleware`'s bearer-token branch performs zero cryptographic or session-store validation of the `Authorization` header — it only checks for the literal prefix `Bearer ` and then trusts a client-supplied `X-User-Id` header verbatim. This is exploitable directly against the host-exposed port (`:3017`), independent of whether the intended frontend proxy is in the path. The internal-key branch is the only well-formed control (exact-match against a real secret); the bearer branch is effectively decorative.

**Blast radius by route:**
- `GET /api/v1/sessions/current`, `POST /api/v1/sessions/refresh` — full impersonation, live-verified 200 OK. No secondary check at all.
- `POST /v1/sessions` (create) — impersonation succeeds at the auth layer; blocked in practice only when the attacker doesn't also know a real `(org_id, user_id)` membership pair that org-core will validate. Live-verified: auth gate passed, org-membership check caught the fabricated pairing.
- `GET /v1/sessions/:id/state`, `GET /v1/sessions/:id/events` (SSE), `POST /v1/sessions/:id/messages`, `POST /v1/sessions/:id/approvals/:approval_id`, `POST /v1/sessions/:id/resume` — code review (`internal/service/session_service.go`) shows **none** of `GetSessionState`, `GetEventsSince`, `SendMessage`, `ResolveApproval`, or `ResumeSession` take a `userID` parameter at all; they operate purely on `sessionID` with no ownership/tenant binding. Once past the same forged-bearer auth gate, any caller who knows or guesses a session ID can read its full state/event stream or write messages/approvals to it, regardless of who created it. (Not independently live-verified against real data — the `session_core.sessions` table is currently empty in this environment, so there was no real cross-tenant session to read back; the gap is a direct, unambiguous reading of the service code, not a live-tested end-to-end case.)

No automated regression test exists anywhere in the repo (`tests/` is empty) that would catch this if patched-and-reverted, or that documents the intended trust boundary.

## Recommended Fix Direction (not yet implemented)

1. Bearer branch of `authContextMiddleware` must actually validate the token — either call auth-core's session/JWT verification synchronously, or drop the standalone bearer branch entirely and require the internal-key path (with the gateway/BFF performing real auth upstream and asserting identity only over the internal-key channel, matching how `X-Internal-Api-Key` is already meant to work).
2. Add session-ownership/tenant checks to `GetSessionState`, `GetEventsSince`, `SendMessage`, `ResolveApproval`, and `ResumeSession` so a valid `userID` can only act on sessions it is entitled to see (matching the model already used in `CreateSession`'s `ValidateMembership` call).
3. Add a security regression test (`tests/` currently empty) asserting: (a) no auth → 401, (b) wrong internal key → 401, (c) forged bearer + arbitrary `X-User-Id` with no internal key → must be 401, not accepted. Wire it into CI so it can't silently regress again.

## Snapshot

`session-core` is no longer the broad orchestration surface described in its older docs. The current Go core is primarily:

- the Control Session aggregator for frontend/BFF consumption
- the legacy session command/event bridge toward Model Plane
- the cache invalidation and republish boundary for entitlements/session snapshots

Current evidence highlights:

- current business HTTP split between legacy `/v1/sessions/*` and Control Session `/api/v1/sessions/*`
- gRPC listener exists but registers no services
- old docs still describe plans/todos/lineage APIs that were removed from this core
- upstream invalidation has a known org-only cache-bust gap and falls back to TTL
- **(new, 2026-07-10) the shared `authContextMiddleware` guarding both surfaces has a live-confirmed authentication bypass on its bearer-token branch** (see above)

Non-generated/non-vendored file count from the current tree: about `46` (29 Go files, 7 SQL migrations, 3 shell scripts). The `app/{api,clients,models,services}` subtree is four empty directories with no files — leftover scaffolding, not functional code.

## Runtime Shape

Key runtime entrypoints:

- `cmd/server/main.go`
  - internal-key startup gate (`internalkey.AssertFromEnv`, fatal in production if the key looks like a placeholder — this is a real, working guard, unrelated to the bearer-token bug above)
  - DB migrations
  - local and shared NATS setup
  - optional Redis
  - repository creation
  - legacy session service creation
  - Control Session aggregator service creation
  - Convex/user/org/billing client hookup
  - upstream invalidator subscriber startup
  - HTTP startup
  - gRPC listener startup without service registration
- `internal/http/server.go`
  - route registration
  - `authContextMiddleware()` — the shared auth gate for every non-`/health` route; see Live Security Verification for its confirmed bypass
- `internal/http/handlers.go`
  - legacy `/v1/sessions/*` handlers
- `internal/http/control_session_handlers.go`
  - `/api/v1/sessions/current` and `/api/v1/sessions/refresh`
- `internal/service/control_session_service.go`
  - Control Session snapshot assembly and refresh
- `internal/service/session_service.go`
  - legacy session command/event bridge; only `CreateSession` takes and enforces a `userID`/org-membership check — the other five methods (`GetSessionState`, `GetEventsSince`, `SendMessage`, `ResolveApproval`, `ResumeSession`) do not
- `internal/subscribers/upstream_invalidator.go`
  - shared-bus cache invalidation and republish logic
- `internal/internalkey/assert.go`
  - placeholder-secret detector shared in spirit with the other three Go Control Plane services (duplicated file, not shared as a library)

## API And Relationship Map

Legacy session HTTP surface:

- `POST /v1/sessions`
- `GET /v1/sessions/:id/state`
- `GET /v1/sessions/:id/events`
- `POST /v1/sessions/:id/messages`
- `POST /v1/sessions/:id/approvals/:approval_id`
- `POST /v1/sessions/:id/resume`

Current Control Session surface:

- `GET /api/v1/sessions/current`
- `POST /api/v1/sessions/refresh`

gRPC surface:

- listener only, no registered business services

Current relationships:

- `session-core` -> `user-core`
  - Control Session aggregation
- `session-core` -> `org-core`
  - Control Session aggregation and org validation (membership check only enforced on session creation, see gap above)
- `session-core` -> `billing-core`
  - Control Session aggregation
- `session-core` -> Convex
  - snapshot mirroring
- `session-core` -> shared NATS
  - session commands/events
  - `app.session.entitlements_changed`
- `session-core` -> Model Plane
  - versioned legacy/new session command routing

## Duplicates, Redundancies, And Non-Relationships

Clear duplication:

- `internal/internalkey/assert.go` is duplicated across the four Go Control Plane services.

Structural redundancy:

- the core carries both legacy session bridge routes and the newer Control Session aggregator routes
- old documentation still describes removed plans/todos/lineage behavior

Non-relationship / partial relationship:

- `cmd/server/main.go` explicitly says Rust Model Plane session-core now owns plan/todo/lineage/approval state
- gRPC listener is started, but no services are registered

## Stubs, Missing Connections, And Drift

Explicit current drift:

- `scripts/smoke_test_api.sh` still exercises todo endpoints that are no longer part of this core
- old docs claim plans/todos/lineage APIs remain here

Known missing connection:

- `internal/subscribers/upstream_invalidator.go` logs `org-only event, no user index - relying on TTL`
- result: org-only updates cannot immediately invalidate all affected cached user snapshots

Known missing control (new, 2026-07-10, live-verified — see above):

- `authContextMiddleware`'s bearer-token branch performs no token validation at all; it is a full authentication bypass reachable directly on the host-exposed port
- five of six legacy session handlers perform no session-ownership check, only a "some authenticated user_id is present" check

## API Design And Performance Notes

API design:

- there are effectively two session APIs in one service: legacy bridge routes and the Control Session aggregator
- this is acceptable as a transition state, but it is documentation-heavy and easy to misread
- health-only gRPC should either be documented clearly or removed later
- the single shared `authContextMiddleware` for both API surfaces means the bearer-bypass affects both the legacy bridge and the actively-used Control Session aggregator simultaneously

Performance and operational notes:

- read-through Redis cache is a good fit for the Control Session snapshot
- missing org-to-user reverse index means some cache invalidation falls back to TTL
- explicit `/refresh` is the manual correctness escape hatch

## Current Doc Cleanup Read

Delete-ready:

- `90_PERCENT_COMPLETE.md`
- `API_REFERENCE.md`
- `GAP_ANALYSIS.md`
- `IMPLEMENTATION_SUMMARY.md`

These files all describe the older plans/todos/lineage-heavy session-core role and are now misleading.

Keep for now:

- `scripts/smoke_test_api.sh`
  - stale for some routes, but this is a script cleanup task rather than doc cleanup

Housekeeping (new, 2026-07-10):

- `app/{api,clients,models,services}` — four empty leftover directories, safe to delete

## Bottom Line

`session-core` is one of the clearest examples of architecture drift being fixed in code but not in docs. The current service is much narrower than its historical documentation suggests. As of 2026-07-10 the most important issue is no longer just documentation drift: the shared auth middleware guarding both the legacy bridge and the Control Session aggregator has a **live-confirmed critical authentication bypass** — a forged bearer token plus a self-asserted `X-User-Id` header authenticates as any user, with a full 200 OK impersonation demonstrated against the actively-used `/api/v1/sessions/current` endpoint. Remaining issues:

- **critical**: bearer-token auth bypass in `authContextMiddleware` (live-verified 2026-07-10, no automated test guards it)
- **high**: five of six legacy session handlers have no session-ownership/tenant check once past the (broken) auth gate
- stale status/reference docs
- gRPC infrastructure without registered services
- partial invalidation for org-only events
- residual legacy route surface during transition
