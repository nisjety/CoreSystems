# W2-CP-INNH10A status: INNH-10 CP-1 api-key exchange route and CP-4 api_key principal type (lane b, branch w23/lane-b)

## Built
All of the scope, CODE ONLY. Checked first: the registry side already existed (`apiKeyExchange` field, its validation and `authorizePlaneApiKeyExchange` in `plane-service-principal.ts`, from R-13 PR 1), as did the key metadata helpers (`readApiKeyAgentMetadata`, `apiKeyHoldsBrowserAgent`, `apiKeyAgentExchangeEnabled`, INNH10B) and the web_policy minting (WS7I). Nothing of the route or the `api_key` principal type existed. Only the gap was built.

- `apps/Control Plane/auth-core/src/auth/convex-token.service.ts` [C] (CP-4): `PlanePrincipalType = 'user' | 'service' | 'api_key'`, `API_KEY_AGENT_AUDIENCES` (`quarry`, `browser-broker`), `isApiKeyAgentAudience`, `API_KEY_AGENT_TOKEN_TTL_SECONDS = 120`, `ApiKeyAgentJwtClaims`. `issuePlaneToken` now refuses any principal type other than user/service (runtime guard against a cast) and any `apikey:` subject. New `issueApiKeyAgentToken(audience, claims)`, the ONLY minter of an api_key token: `sub apikey:<key_id>`, `principal_type api_key`, `key_id` (printable, at most 128), `acting_user_id`, required `jti`, no `user_id`/`service_id`, non-empty scopes, required `interactive-org-retention-policy` posture, `zdr = org OR key` (raise only), `web_policy` as for a user token (baseline when none), `key_hosts` required on browser-broker and refused on quarry (1..32), TTL min(audience TTL, 120, shorter override, web-policy cap). Private `signPlanePayload` signs it (issuePlaneToken's own signing is unchanged).
- `apps/Control Plane/auth-core/src/auth/api-key-exchange.controller.ts` [NEW] (CP-1): `ApiKeyExchangeHandler.exchange`. Order: flag (404 `api_key_agent_exchange_disabled`) -> audience (404) -> body (400; a caller `zdr` or `ttlSeconds` is refused) -> caller `authorizePlaneApiKeyExchange` (403; registry unreadable 503) -> Better Auth `verifyApiKey({ key, configId: 'user-keys' })` (401 invalid/expired/disabled, 429 rate limited/usage exceeded, 503 if it throws) -> re-checks configId `user-keys` (403 `api_key_kind_refused`), owner present, `enabled`, `expiresAt` -> strict metadata (403 `api_key_metadata_invalid`), `org_id == orgId` (403 `api_key_org_mismatch`) -> holds `browser:agent` and every requested scope, token scopes limited to `browser:agent` (403 `api_key_scope_missing`) -> owner still a member (`resolveCanonicalTokenContext`, 403 `api_key_owner_not_member`, 503 when unreadable) -> org posture and web policy (503 propagated) -> mint with a fresh `randomUUID()` jti -> durable `api_key_token_exchanged` audit, fail-closed 503 (token discarded). Denials after caller auth publish `api_key_token_denied` (best effort). The key is never logged or audited; the verify error object is never logged.
- `apps/Control Plane/auth-core/src/auth/plane-token.controller.ts` [C]: route `POST :audience/api-key-exchange` (`exchangeApiKey`) delegating to the handler, built in the constructor. No module registration change.
- `apps/Control Plane/auth-core/src/auth/api-key-exchange.controller.spec.ts` [NEW]: real ConvexTokenService + real controller route; doubles for Better Auth verify, membership, org posture, NATS. Cases: quarry token shape + audit, broker `key_hosts`, fresh jti, disabled flag (3 forms, no verify, no audit), non-agent audiences 404, body refusals incl. caller zdr/ttl, caller without `apiKeyExchange` for the audience / wrong credential / unknown (403, key never verified), broker-only caller, org mismatch (+ denial audit), metadata invalid, missing permission (2), org-key refused (2), expired/disabled/rate-limited/usage-exceeded/not-found mapping, returned-key expiry/enabled re-check, owner left org, membership 503, posture 503, TTL cap (3600 s audience TTL -> 120), override narrows only, audit failure 503, denial stays denial when audit fails, zdr raise-only matrix (4), key never in logs/audits/errors; CP-4 signer guards (issuePlaneToken refuses api_key type and apikey: subject, audience fence, key_hosts rules, 10 fail-closed claim cases).

## Commits (lane b)
- 1cda680be feat(auth-core): api_key principal type, minted only by issueApiKeyAgentToken (INNH-10 CP-4)
- 955df26be feat(auth-core): POST /api/:audience/api-key-exchange behind API_KEY_AGENT_EXCHANGE_ENABLED (INNH-10 CP-1)
- 0b27467a3 test(auth-core): api-key exchange and api_key principal type specs (INNH-10 CP-1, CP-4)
- (this file, MP ask and depth rows: the docs commit that adds this file)

## Code only, not compiled
No jest, tsc, prettier or lint was run (owner order 2026-11-03). Integration must run `tsc6 -p tsconfig.app.json` (or the package tsc) and `npx jest api-key-exchange plane-token convex-token plane-token-exchange quarry-token-zdr-claims`, then diff the full failing set against base (15 pre-existing failures in 3 suites). Run `prettier --write --end-of-line auto` on the 4 files before the lint ratchet (some lines exceed 80 columns).

## Flag and default
`API_KEY_AGENT_EXCHANGE_ENABLED` (auth-core), default off, on top of `API_KEY_ENABLED`: the route answers 404 `api_key_agent_exchange_disabled` and touches nothing (no registry read, no key verification, no audit). No other flag. The registry row of the Verevon gateway must also list the audience in `apiKeyExchange` (absent by default).

## Assumed decisions
- OD-1: new `api_key` principal type; OD-2: only a registry principal with `apiKeyExchange` (the Verevon gateway) may exchange; OD-4: user-keys only (verify scoped to `user-keys` plus a re-check); OD-5: token scopes are `browser:agent` only and must be held by the key; OD-9: 120 s cap, fresh jti; OD-12: zdr = org posture OR key flag, raise only; OD-6: `key_hosts` from the key metadata on the broker audience only.
- Disabled flag answers 404 (the route is dark), not 403.
- Event names `api_key_token_exchanged` / `api_key_token_denied`; subject `apikey:<key_id>`, `resource_id` key_id; details `key_id, org_id, acting_user_id, audience, scopes, zdr, zdr_source (org|key), calling_service, web_policy_rev`; denial details carry `reason`. event_id: the token digest (`plane-token:` prefix, the existing helper) for exchanged; `api-key-exchange:denied:<uuid>` for denials.
- Denials are audited only after the caller is authorized, and best effort (a denial is returned even if its audit fails); only issuance is fail-closed.
- Membership: `resolveCanonicalTokenContext(owner, orgId)` (any role). Owner = Better Auth `referenceId` (fallback `userId`).
- Request bounds: apiKey at most 512 chars, no surrounding whitespace; at most 8 scopes; orgId the same pattern as the retention resolver.
- Response is the plain token bundle (`token, expiresAt, expiresInSeconds, issuer, audience`).
- OD-11 rate (600/h per key) not built here: Better Auth's per-key limit applies (depth row).

## MP asks
Appended to MP-ASKS.md under "W2-CP-INNH10A": the exact api_key token claim set for the broker (MP-1: accept `principal_type=api_key`, owner = `acting_user_id`, hosts subset of `key_hosts`) and model-gateway's key-grants door (MP-3).

## Not verified
- Nothing compiled or run. Narrowing after the `never`-returning `deny()` calls relies on TS assertion-style control flow (explicit `: never` function declaration).
- Better Auth 1.6.23 `verifyApiKey` semantics read from the plugin source (configId-scoped lookup, error codes KEY_EXPIRED / KEY_DISABLED / RATE_LIMITED / USAGE_EXCEEDED / INVALID_API_KEY, returned `referenceId`, parsed `permissions` and `metadata`), not live.
- No live exchange, no deploy, no gateway (GW-1) or broker (MP-1..3) consumer.
