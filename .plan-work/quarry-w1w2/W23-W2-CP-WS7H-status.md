# W2-CP-WS7H status: org-core web policy storage, admin API and internal read (lane d, branch w23/lane-d)

## Built
All of the scope. Checked first: nothing of WS7.H existed (no `web_policy*` file, no migration 025-027, no route; only the `org:web-policy:read:any` allowlist entry and the compose holder from W2-CP-R13-2). The lane was fast-forwarded to `w23/integ` first (clean).

Migrations (`apps/Control Plane/org-core/migrations`, LF by attribute):
- `025_organization_web_policy` (`org_id` PK with `ON DELETE CASCADE` to organizations, `policy` JSONB, `revision > 0`, `rev_digest` 12 hex, `updated_by`, `updated_at`), RLS as 019/023, `REVOKE ALL` then `GRANT SELECT, INSERT, UPDATE` (migration 009's default privileges would otherwise give the runtime role DELETE).
- `026_organization_web_policy_history` (PK `(org_id, revision)`, `policy`, `changed_by`, `changed_at`, `tightened`), RLS, `GRANT SELECT, INSERT` only.
- `027_organization_web_policy_audit_outbox` (shape of 022's SIEM outbox, subject pinned to `verevon.audit.v2.control.org-core.org_web_policy_changed`), RLS, `GRANT SELECT, INSERT`.
- Verified on a throwaway Postgres 16: applied by the real runner, re-run (`IF NOT EXISTS`, NOTICEs only), `.down` x3, applied again.

org-core `internal/org` (CRLF files):
- `web_policy.go` [NEW]: `WebPolicyDoc` (`caps`, `robots_max`, `proxy_max`, `domains{allow,deny}`, the claim body without `v`/`rev`), `Validate(catalogue)`, `Normalize`, `Canonical`, `Digest()` (first 12 hex of sha256 of canonical JSON), `Effective(entitlements)`, `CheckEntitled`, `Tightened(old,new)`, `ChangedKeys`, errors `ErrRevisionConflict`, `ErrIllegalMode`, `ErrUnknownCapability`, `ErrInvalidDomain`, `ErrWebPolicyTooLarge`, `ErrNotEntitled` (typed `*WebPolicyError`, `*RevisionConflictError`). The catalogue is embedded with `go:embed` from `testdata/web-policy-catalogue.v1.json` (byte-identical to `quarry-core/contracts/web-policy-catalogue.v1.json`, pin `.sha256` beside it, sha 998c8034...).
- `web_policy_repository.go` [NEW]: `GetWebPolicy`, `PutWebPolicy(ctx, orgID, doc, ifMatch, changedBy, actorRole)` in one `WithOrgScope` transaction (organization row `FOR UPDATE`, revision compare, plan entitlement check, history insert, upsert, audit outbox insert), `ListWebPolicyHistory` (newest first, revision cursor), outbox claim/mark and `FlushWebPolicyAuditOutbox` (the SIEM loop).
- `types.go` [C]: the three computed entitlement plan maps and `PlanAllowsWebCapability`, `WebEntitlementsForPlan`, beside `zeroDataRetentionPlans`.
- `service_enhanced.go` [C]: `GetWebPolicyPosture`, `SetWebPolicy` (normalise, validate, write, best-effort inline audit flush), `ListWebPolicyHistory`.

org-core `internal/http` (CRLF files):
- `web_policy_handler.go` [NEW]: `getWebPolicy`, `putWebPolicy`, `listWebPolicyHistory`, `getWebPolicyInternal`. Typed envelopes `{data}`, `{data, meta:{cursor}}`, `{error:{code,message,details}}`. PUT: `If-Match: <revision>` (quoted and `W/` forms accepted; 428 `precondition_required` when absent, 400 `invalid_if_match`), body is the document (strict, 16 KiB cap -> 413 `too_large`), 200 `{data:{revision, rev}}`, 409 `revision_conflict` (`details.current`), 422 `illegal_mode` / `unknown_capability` / `invalid_domain` / `too_large`, 402 `not_entitled` (`details.capability`), 403 `admin_required`. The caller's role is read from org-core's membership table, never from the forwarded header.
- `server.go` [C]: routes `GET/PUT /orgs/:id/web-policy`, `GET /orgs/:id/web-policy/history` and the `/api/v1/organizations/:id/...` twins (PUT behind `requireActiveMembership`), internal `GET /internal/orgs/:orgId/web-policy`; `Server.webPolicy` is the `*org.Service` when present.
- `service_auth.go` [C]: scope mapping (user routes `org:read:self` / `org:settings:write:self`, internal route `org:web-policy:read:any`), and a validator pin: `org:web-policy:read:any` is valid only for the `auth-core` principal.
- `service_auth_test.go` [C, one hunk]: `TestOrgKeysAndWebPolicyScopesAreMachineOnly` now gives the web-policy scope to `auth-core` (the key scopes stay quarry-edge's).
- `testdata/auth-core-org-core-web-policy.contract.json` + `.sha256` [NEW]: the vendored golden (see Not verified).

## Commits (lane d)
- 0fa274ff5 feat(org-core): web policy storage, history and audit outbox migrations (WS7.H)
- 0bfa4e00e feat(org-core): web policy document, catalogue pin and computed web entitlements (WS7.H)
- dc45a8c49 feat(org-core): web policy repository with If-Match compare-and-set, history and audit intent (WS7.H)
- 7190654ce feat(org-core): web policy admin API and Auth Core internal read (WS7.H)
- 05bb3afe7 fix(org-core): size the web policy cap for the claim after the entitlement fold (WS7.H)
- d0301ad3f docs(plan-work): W2-CP-WS7H status and depth rows (this file's first version)

## Tests run (host, Go 1.26, all passed)
- Pure (`internal/org`): `TestValidateRejectsForcedOnAbility`, `TestValidateRejectsUnknownKey`, `TestValidateRejectsIPLiteralAndWildcard`, `TestEffectiveFoldsEntitlements`, `TestDigestIsStableUnderKeyOrder`, `TestTightenedDetectsOnlyTightening` (the six the plan names) plus protection modes, ceilings/rules not in caps, normalisation, size caps, claim-envelope room, digest pin, entitlement check and plan lines, `ChangedKeys`, content-free audit payload, migration text pins, catalogue loader refusals, catalogue pin and byte-compare with the Quarry source.
- HTTP (`internal/http`): `TestAdminMatrix` (owner/admin/member/viewer/stranger x GET/PUT/history x both route forms, forwarded role ignored), `TestPutRejectsOverlargeDocument`, `TestInternalRouteRequiresItsOwnScope`, `TestRetentionPostureStillOnlyTwoBooleans`, `TestPutCreatesWithIfMatchZeroAndGetReturnsTheSameRev`, `TestStalePutIs409`, If-Match, typed-error mapping (14 cases), history paging, scope mapping, validator pin, unknown org and 5xx without detail.
- Contract: `TestWebPolicyContractCopyMatchesPin`, golden replay through the real middleware and the real compose credential block (`TestAuthCoreWebPolicyRequestIsAdmittedByRealOrgCoreMiddleware`), `TestWebPolicyNotFoundAnswerMatchesGolden`, unauthenticated refusal, only-auth-core through the compose registry.
- Postgres (DSN-gated, 8 tests): `TestControlLifecyclePutCreatesWithIfMatchZero`, `StalePutIs409`, `CrossOrgReadIsNotFound`, `HistoryIsAppendOnly`, `HardDeleteRemovesPolicyAndHistory`, `ConcurrentPutsExactlyOneWins` (8 writers, 1 winner), `OrgCoreAppHoldsOnlyTheGrantsItNeedsOnWebPolicyTables`, `WebPolicyAuditOutboxRetriesAFailedPublish`.
- Counts: the 57 web-policy tests (incl. the 3 existing ones that cover the same scope) all pass with the database; without it 48 pass and the 9 DSN-gated ones skip. Whole module `REQUIRE_CONTRACT=1 go test -count=1 -p 1 ./...`: every package ok both ways (without `*_DATABASE_URL`, and with `CONTROL_LIFECYCLE_TEST_DATABASE_URL` + `TEST_DATABASE_URL` on a throwaway `postgres:16-alpine` container started and removed by this run, role `org_core_app` and the lifecycle marker created as in `org-service.yml`). `go vet` clean, `gofmt` clean (checked on the LF form before the CRLF conversion).

## Flag and default
None, as planned: the routes are additive and unread until WS7.I (`WEB_POLICY_CLAIM_ENABLED`, default 0, in auth-core). Nothing in org-core reads the new tables except these routes. Migrations apply through the existing runner on the next org-core start; rollback is the three `.down.sql` files.

## Assumed decisions
- Document shape = the claim body: `{"caps":{key:mode},"robots_max":"strict|respect|ignore","proxy_max":"none|pool","domains":{"allow":[...],"deny":[...]}}`, exactly what the edge's `parse_web_policy_claim` reads. Ceilings and domain rules are refused inside `caps` (422 `illegal_mode`). Hosts are lower-cased, trimmed, de-duplicated and sorted on write, then validated by the same rules as `quarry-core valid_host`.
- Canonical document cap 1950 bytes (not 2048): the claim adds `{"v":1,"rev":"<12 hex>",` (28 bytes) and `Effective` can add 42 bytes of disabled paid search after the check, and the issuer fails closed above 2 KiB; a test builds a near-maximal document and measures its effective claim. 64 rules and 2048 rule bytes as in the edge.
- Entitlement lines (product input, three maps in `types.go`): `web.robots_override` enterprise only; `web.proxy_pool` pro and enterprise; `web.paid_search` trial, hobby, standard, pro, enterprise (free has none). A PUT that unlocks what the plan lacks is 402 (`robots_max=ignore`, `proxy_max=pool`, `search.paid_provider=allowed`); `Effective` folds such an unlock away and disables paid search when the plan lacks it, tightening only. Who may actually be granted the robots override (OQ-11, platform operators only) stays Quarry's check; the plan check here is only the org-level entitlement.
- `GET /orgs/:id/entitlements` is unchanged (no `web.*` keys); the web-policy GET carries the three entitlements itself.
- GET readers: owner/admin see the stored document, `updated_by`, `updated_at`; any other active member sees `effective`, `entitlements`, `revision`, `rev` with `policy`, `updated_by`, `updated_at` null (the crawl UI needs the effective view). Write and history are owner/admin only.
- `rev` on the internal and admin reads is the STORED policy's digest ("0" with no row); `policy` on the internal read is the EFFECTIVE document.
- Audit event `org_web_policy_changed`: `revision`, `rev`, `tightened`, changed capability KEYS only, never a host name or a value (so no domain names for a ZDR org or any other).
- No audit flush loop in `cmd/server/main.go` (outside the unit's files): the write flushes once inline after commit, a failed publish stays pending (depth row).
- Pinned `org:web-policy:read:any` to `auth-core` in the validator (R-13-2 depth row asked for it when the route lands). This reverses that unit's "no pin" assumption on purpose.
- Missing `If-Match` is 428, not a default of 0.

## Model Plane asks
None. Nothing appended to MP-ASKS.md.

## Not verified
- The org-core Docker image was not built, so `go:embed` of `internal/org/testdata` inside the image build is reasoned (the `.dockerignore` excludes `*_test.go`, not `testdata`) and not proven.
- Nothing deployed, no live org-core restart, no live PUT or GET, no gateway call. The deployment evidence steps (1)-(6) of the plan were not run.
- Auth Core's consumption: the golden `auth-core-org-core-web-policy.contract.json` was written by THIS unit from the route's answer (the plan has WS7.I's TypeScript spec write it). WS7.I's `web-policy.org-core-contract.spec.ts` must reproduce these bytes or change both copies together; until then the golden pins org-core's side only.
- The repo-level sync script rows (`scripts/tests/zdr-contract-sync.test.sh` GOLDENS/VENDORED for the golden and the catalogue copy) and the `contract-goldens.yml` path globs were not added (outside the unit's files).
- Postgres behaviour was proven on Postgres 16 alpine only (CI uses the `org-service.yml` image).
- `go test -race` (needs cgo, not available on this host).
