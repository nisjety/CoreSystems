# D7 web-policy-resolved-policy: one tighten-only ResolvedPolicy for Quarry, with the policy-of-record decision split off

Workstream WS7 (wave W2). Author: design agent D7. Date of code read: 2026-10-15 against the live tree `C:/dev/CoresSystem` (HEAD `afb83928`, 2026-10-10, plus about 324 uncommitted entries). Audit source: `C:/dev/verevon-audit-2026-10-14/02-quarry-and-browser-audit.md`. Nothing was built or run; every statement marked [READ] was verified by opening the file, [UNVERIFIED] means not checked, [INFER] means reasoned from code.

Path shorthand used below. `Q` = `apps/Ingestion Plane/Quarry-v2`; `QE` = `Q/crates/quarry-edge/src`; `QC` = `Q/crates/quarry-core/src`; `QR` = `Q/crates/quarry-runtime/src`; `CP` = `apps/Control Plane`; `MP` = `apps/Model Plane`; `FE` = `apps/Frontend Plane/verevonv3`. Provenance tags come from `tools/provenance.sh`: `[C]` committed and unchanged, `[M]` committed but modified in the live tree, `[U]` untracked, `[NEW]` a file this plan creates.

## 1. Items resolved and wave

| Item | Title | Wave | Treatment here |
|---|---|---|---|
| F-t2-022 | Org-pinned, tighten-only web policy: tri-state capability gates, signed claims, Quarry enforcement | W2 (Priority, L) | Fully designed. Split into six owner-neutral slices (S1-S6, about 21 engineer-days, no decision needed) and six owner-dependent slices (S7-S12, about 31 engineer-days plus half a day of decision records, blocked by audit ADR topic 1). The ADR is written as a decision record in section 3 with a recommendation. |
| F-t2-032 | Per-origin consent, action-class grants, typed denials, org origin policy, settings hub | W3 (Recommended, XL) | Epic outline only (section 4, "Epic E-032"). The typed-denial vocabulary, the action-class catalogue and the claim and grant carriers it needs are built here, so E-032 starts from them instead of inventing a second set. |

What this workstream delivers, in one paragraph: a single Rust type, `quarry_core::web_policy::ResolvedPolicy`, computed on every request and every agent step as the tighten-only meet of five layers (operator baseline, org policy, broker grant, caller request, ZDR), with typed `FORBIDDEN` denials that name the capability and the layer that decided; the wiring of that type into every place Quarry today either hard-wires `RunPolicy::default()` or takes an ad-hoc ZDR decision; the retirement of the dead policy shapes; and, after the policy-of-record decision, org-core storage, auth-core claim minting and broker grant fields that feed the same type.

## 2. Verified current state

### 2.1 What the code does today [READ unless tagged]

**There is no web policy of any kind in Quarry except ZDR and the broker's domain list.** The audit is right on the headline. Eight facts shape the design, and three of them correct the card.

1. **The ZDR floor is the only org-pinned decision and it covers user tokens only.** `Claims` is at `QE/auth.rs:61-91` [C] (`zdr: Option<bool>` at 89-90). `Claims::zdr_floor` is `!self.is_service() && self.zdr == Some(true)` (`QE/auth.rs:121-123`), `effective_zdr` is `requested || zdr_floor` (129-131), `effective_zdr_mode` wraps it for the `Option<bool>` body flag (136-138). The doc comment at 105-120 states the reason: model-gateway and execution-core hold `retentionByAudience.quarry = "zdr"` in the principal registry (`CP/config/plane-service-principals.json:148` and 298 [C]), so their token says `zdr:true` for every org on every call, and honouring it would force ZDR onto all chat web search. The golden `CP/auth-core/src/auth/fixtures/quarry-token-claims.golden.json` [C] captures exactly this (cases `service_model_gateway`, `service_execution_core`, both `zdr:true`, `ttl_seconds:300`).

2. **ZDR decisions are taken at about a dozen unrelated sites**, each calling `claims.effective_zdr*` itself: `QE/routes.rs:416` (scrape), `1072` (scrape_stream), `569` (`refuse_durable_job_under_zdr`, shared by crawl and batch handoff), `QE/extract_routes.rs:237` [M], `QE/answer_routes.rs:89` and `187` [M], `QE/audio_routes.rs:74-75` [M], `QE/search_routes.rs:727` [M] (plus `1401`, `1415`), and `QE/agent_routes.rs:1441-1445` (start, resume). `internal_run_page` (`QE/routes.rs:898-1001` [M]) reads `req.zdr` straight from the body (about line 975) because that route has no JWT.

3. **`RunPolicy::default()` is hard-wired at four production sites and a fifth holds a value nobody reads.** Production: `QE/routes.rs:461` (scrape), `QE/routes.rs:1001` (`internal_run_page`), `QE/routes.rs:1116` (`scrape_stream`), and `QE/main.rs:1493` [M] which fills `AppState.policy` (`QE/state.rs:102-106` [M]). `grep "state.policy"` finds no reader, so the `AppState` field is dead and the three `PageRunner` literals ignore it. The comment at `QE/main.rs:1489-1492` documents a `QUARRY_EDGE__RUN_POLICY=strict` knob that no code reads (S8 sweep, confirmed). Test-only sites: `QE/routes.rs:1332`, `QE/test_support.rs:129` [M], `Q/crates/quarry-edge/tests/support/mod.rs:74` [M], `QR/pipeline.rs:1191` and `1332`, `Q/crates/quarry-core/tests/contracts.rs:95`.

4. **[CLAIM-FALSE, larger than the audit says] Replacing `RunPolicy::default()` cannot by itself enforce anything, because the runtime `RunPolicy` is a determinism stamp.** `PageRunner` reads exactly one thing from it: `policy.determinism` and `record_determinism_inputs(url, &policy)` for the `DeterminismStamp` on the output (`QR/pipeline.rs:752-759` [C]). No code reads `policy.discovery.respect_robots`, `policy.retry`, `policy.block` or `policy.fetch.headers` during page execution. Robots is checked in one place only, `QE/map_routes.rs:256` (`rt.is_allowed(MAP_UA, ...)`). So a `ResolvedPolicy` has to bring its own enforcement sites; it cannot piggy-back on the type it replaces. This is why the first slice below is as large as it is.

5. **Two `RunPolicy` types exist, and a third and fourth policy shape sit beside them.**
   - `QC/policy.rs:6-16` [C]: the CONTRACTS §7 wire shape (`concurrency, delay, retry, proxy, robots, ordering, block, checkpoint, determinism`), mirrored by Go `Q/pkg/quarrycontracts/policy.go` (`Robots` at line 8; `RobotsMode` 52-56). Rust consumers: `QR/retry.rs:10` (`Retry`, `BackoffKind`) and `Q/crates/quarry-core/tests/contracts.rs:95`. Nothing else.
   - `QR/policy.rs:215-230` [C]: the live runtime bag (`determinism, discovery, fetch, retry, block, extraction, checkpoint`; presets `strict` 236, `best_effort` 261, `off` 276; `policy_fingerprint` 309). [DRIFT] The card's citation `QR/policy.rs:55-80` is `DiscoveryPolicy` (struct at 59, Default 71-79), not the second `RunPolicy`.
   - `QC/presets.rs:320` `builtin_presets()` [C]: six crawl bundles (docs-site, help-center, ...). No production caller (only its own tests).
   - Go `Q/services/quarry-control/internal/resources/presets.go:56-124` [C]: four `RunPolicy` presets (`fast` with `Robots: RobotsIgnore` at line 69, `polite`, `stealth`, `deterministic`).

6. **[CLAIM-FALSE] "control's presets are never consumed".** They are consumed, at job creation: `resolveJobPolicy` (`Q/services/quarry-control/internal/resources/resources.go:469-481` [C]) calls `ResolvePreset` (`presets.go:47-54`) and `createJob` (about 408) stores the result. What is true is the weaker claim that **the stored policy never reaches page execution**: the orchestrator's `RunPageInput` has only `RunID, URL, OrgID, UserID, Ingest` (`Q/services/quarry-orchestrator/internal/activities/activities.go:165-181` [C]) and `RunPage` posts `{url, org_id, user_id, ingest}` (line 214); `InternalRunPage` (`QE/routes.rs:743-769` [M]) has no policy field; the `workflows.Policy map[string]any` "opaque bundle" (`workflows.go:17-18`, 40, 71, 86 [C]) is carried but never forwarded. A caller that creates a job with `{"policy": {"robots": "ignore"}}` (explicit policy wins over preset, `resources.go:471-473`) therefore gets a stored `ignore` that nothing honours today and that would be honoured the moment F-t2-016 or F-t2-017 forwards it.

7. **`/v1/blocklists` is an org-less global list nobody reads.** `BlocklistEntry{ID, Pattern, IsRegex, CreatedAt}` (`Q/services/quarry-control/internal/store/store.go:397-402` [C]); the Postgres insert has no org column (`.../store/pg/webhooks_blocklists.go:190-196` [C]); `MountBlocklists` is a generic `mountSimple` (`blocklists.go:8-10`); no code in `QE` reads it. The edge's real host blocklist is the URLhaus/PhishTank snapshot (`QE/config.rs:34-37` read at `QE/main.rs:348`, S8 #16). `FE/docs/endpoint-map.md:340` [C] confirms the control routes are HMAC-internal and never proxied to the browser. No other consumer exists in the live tree.

8. **Agent path.** `start_run` (`QE/agent_routes.rs:1273-1663` [C]) makes capability decisions from four body fields (`persist_profile`, `profile_id`, `driver_requirements.{persistent_profile, downloads_to_artifacts, uploads_from_artifacts, devtools_trace}`, `execution_tier`; struct at 345-379 and 391-410) and nothing policy-shaped. `step` (2228-2409) runs: owner check, grant validation (2257-2267), `require_action_capabilities` (2269; driver proof, 1220-1270), the ephemeral-tier filter (2271-2274), `sensitive_approval_for_action` + one-shot insert (2276-2292), `enforce_step_budget` (2294), the Navigate domain check (2297-2305), `validate_navigation` (2307, `security.preflight` + DNS guard), the driver call, receipts, and a post-action URL check (2400-2406). There is **no `Evaluate` gate** (`QR/observation.rs:542` calls the driver directly): WS2/D2 slice S1 closes it. `validate_run_grant` (314-338) demands exact equality between `entry.constraints.allowed_domains` and the broker's canonical list at every step, but only when `require_browser_grants` is set. `BrowserEgressPolicy::from_allowed_domains(&[])` means **unconstrained** (`Q/crates/quarry-browser/src/lib.rs:41-95`): any intersection that comes out empty must become `deny_all()`, never `[]`.

**The service-token ZDR statement of the card is correct, but its consequence ("for Model-driven runs the policy must ride in the grant or the body") is not the only reading** (see section 3, ADR record): auth-core already mints org-resolved claims onto service tokens for `tenant`-retention principals (`CP/auth-core/src/auth/interactive-retention-policy.ts:514-545`, `convex-token.service.ts:150-170, 648-730` [C]), and master-ownership-matrix rule 16 records that as the pattern. execution-core caches its Quarry service token per (org, scope set) for the token lifetime with a 30 s refresh skew (`MP/rust/services/execution-core/src/quarry_auth.rs:17-19, 200-260` [C]).

### 2.2 Audit claims checked

| Claim (card / JSON) | Verdict | Code |
|---|---|---|
| ZDR claim reaches user tokens only; service callers ignore it by design | TRUE | `QE/auth.rs:105-123` |
| Edge hard-wires `RunPolicy::default()` (routes.rs:461, main.rs:1485-1493) | TRUE, understated: four production sites; `AppState.policy` is unread | section 2.1 item 3 |
| Two unrelated `RunPolicy` types | TRUE, and there are two more shapes (`presets.rs`, Go presets) | item 5 |
| Second `RunPolicy` at `QR/policy.rs:55-80` | [DRIFT] it is at 215-230 | item 5 |
| Control presets never consumed | [CLAIM-FALSE] consumed at job creation; never reach the edge | item 6 |
| `/v1/blocklists` has no org column, is not read by the edge | TRUE | item 7 |
| `agent_routes.rs:144-300` is a run-policy site | [DRIFT] it is the sensitive-approval gate (`sensitive_approval_for_action` 144-283, `validate_sensitive_approval` 284-308) | item 8 |
| Gateway `quarry_token` adds a `policy_floor` claim (JSON R-t2-04-05) | [CLAIM-FALSE] the gateway mints nothing; it fetches an auth-core token on every request with no cache (`FE/apps/gateway/src/audience_tokens.rs:87-117` [C]); minting is `ConvexTokenService.issuePlaneToken` (`convex-token.service.ts:648-730`) | item 8 |
| Model Plane gate is one coarse capability, `cap.browser.open` | TRUE | `MP/rust/services/execution-core/src/capability_policy.rs:793` [C]; seeded HIGH, so the engine answers Ask (`MP/go/services/capability-core/migrations/0008_execution_dispatch_capabilities.up.sql:12`; `internal/policy/engine.go:84-153`) |
| capability-core is a competing policy of record | PARTLY: its grants are tenant-bound and durable (`engine.go:158-216`, `registry/scope_store.go`), and it has an org-scope grant API with audit rows (`POST /api/v1/capabilities/scopes/grant` and `/revoke`, `GET .../scopes` and `/resolve`, `MP/go/services/capability-core/internal/api/capabilities.go:57-60, 309-376`, only `scope_kind=org` accepted), but a grant is one boolean per capability id and the engine answers allow/deny/ask from risk level: no parameter (tier, mode, domain list) can be stored or evaluated | `engine.go:84-153`, `capabilities.go:329, 365` |
| Broker grants are persisted in Postgres/Redis (JSON R-t2-06-02) | FALSE today: in-memory only (`grant.go:61-64`); WS1/D1 slice 1c adds Postgres | S8 #24 |
| robots is enforceable today so F-t2-022 can "switch it on" | FALSE: `PageRunner` never checks robots; only `map_routes.rs:256` does | item 4 |
| Evaluate is gated by a sensitive approval | FALSE: no gate at all (`QR/observation.rs:542`); D2 S1 adds a hard deny | D2 |

### 2.3 Uncommitted ([M]/[U]) files in the area and conflicts

| File | Tag | In-flight diff | Conflict with WS7 |
|---|---|---|---|
| `QE/routes.rs` | [M] | one line at 1329 (`model_plane: Default::default()` in the test literal) | none on 461, 1001, 1116; I touch three `PageRunner` literals and the test literal at 1332 |
| `QE/main.rs` | [M] | about 188 changed lines at 47, 244-253, 984-1463 | adjacent to my touch at 1493 (`AppState` literal); rebase, do not merge blind |
| `QE/state.rs` | [M] | 21 changed lines (doc comments, `model_plane` field at 90) | I change the type of the existing `policy` field at 106; one-line change, no new field |
| `QE/config.rs` | [M] | 187 lines | **avoided**: WS7 reads its settings from a new `policy_baseline.rs` (same reasoning as D2 D13) |
| `QE/test_support.rs`, `tests/support/mod.rs` | [M] | one line each | I change the `policy:` line in the same literals |
| `QE/search_routes.rs` | [M] | large: `SearchRequest` fields 54-143, `build_search_options` 541-552, ZDR_REQUIRED guard 696-727, cache gate 830+ | **real overlap** with the paid-provider edit (S4); sequence S4 after the diff commits |
| `QE/extract_routes.rs` | [M] | 205-208, 228-229 | 8 lines from the ZDR site at 237; mergeable |
| `QE/answer_routes.rs`, `QE/audio_routes.rs` | [M] | model-plane switches | low |
| `Q/crates/quarry-edge/tests/zdr_claim_floor.rs` | [M] | +10 lines at 919 and 979 | I add a sibling test file, not edits |
| `Q/docs/openapi.yaml` | [M] | +78 lines at 238-250, 4012-4150 | I add schemas in new components; agent paths untouched |
| `scripts/tests/zdr-contract-sync.test.sh` | [M] | +95 lines (new GOLDENS rows, e.g. QB-T09, QB-T10, usage ledger) | I append rows for the new catalogue and claim goldens (S1, S8) |
| `CP/auth-core/src/auth/fixtures/cross-plane-token-requests.json` (+.sha256) | [M] | 5 lines | not touched (no new Quarry-to-Auth-Core request rows) |
| `MP/rust/services/model-gateway/tests/fixtures/auth-core/` | [U] | whole directory (vendored `quarry-token-claims.golden.json` + .sha256) | **the third copy of the golden** that S8 must also regenerate; it does not exist in git |
| `MP/rust/services/model-gateway/src/auth.rs` | [M] | strict zdr-claim tests | S8 must re-run its vendored-golden tests |
| `QE/model_plane_gate.rs` | [U] | whole file | unrelated (search, answer, audio switches) |

None of the in-flight diffs touches the lines WS7 edits in `routes.rs`, `main.rs` or `state.rs`; the only genuine overlap is `search_routes.rs`.

## 3. Design decisions

Tags: **ADR-GATED** marks a decision that needs a user or ledger ruling before the named code merges, with the audit topic number from section 9.3 of the audit (1 = web policy of record; 3 = shared human and agent sessions; 4 = ZDR semantics per feature; 5 = anti-bot posture; 6 = script execution model). Everything not tagged ships without a ruling.

### 3.1 ADR record: where does org web policy live?  (ADR-GATED, audit topic 1)

**Context.** F-t2-022, F-t2-032, F-t2-008, F-t2-025 and F-t2-046 all need an org-level answer to "may this org's browser and web calls do X". Quarry must enforce it ("Quarry executes or rejects"), and three delivery channels exist: the signed token claim (today carries ZDR), the BrowserBroker grant (today carries domains and one-shot approvals), and the request body. Two candidate owners exist: Control Plane org-core (org settings, entitlements, admin RBAC) with auth-core as issuer, and Model Plane capability-core (durable org-scope capability grants).

**Options.**

| | A. Claims on user tokens only; grant fields for Model-driven runs (the audit's literal text) | B. capability-core owns; grant fields only | C. org-core owns; claim on user tokens and on org-resolved service tokens; grant carries a snapshot (recommended) |
|---|---|---|---|
| Store and admin API | org-core | capability-core (new parameter store) | org-core |
| Reaches `/v1/scrape`, `/v1/search`, `/v1/extract`, `/v1/map`, `/v1/crawl`, `/v1/batch` called by a user or SDK | yes (user claim) | no: these routes carry no grant (`QE/routes.rs:130-190`) | yes (user claim) |
| Reaches Model-driven runs (execution-core and model-gateway call Quarry with service tokens) | only through the grant, so a second place resolves policy | grant only | claim on the service token (Quarry sees the org floor itself) plus grant snapshot |
| Sources Quarry resolves per call | claim and grant: two paths | grant only | claim, grant, baseline, request: one function, four inputs |
| Revocation mid-run | claim staleness for users (45 s resolver cache), grant TTL 15 min for Model runs | grant TTL 15 min plus a new broker revoke-by-org call | claim staleness for both (about 345 s default, 105 s with a 60 s cap) |
| New Control work | org-core storage, auth-core user claim | none in Control; capability-core parameter schema (new) | org-core storage, auth-core user claim, registry field `webPolicyByAudience`, parity script |
| Contract goldens that change | `quarry-token-claims.golden.json` (3 copies) | none | same, plus service cases |

**Evidence from the code.**

1. `Claims::zdr_floor` ignores a service principal's `zdr` because that value is a property of the registry row, not of the org (`QE/auth.rs:105-123`). This is why the ZDR claim cannot be reused as the carrier. It says nothing against a different claim.
2. Auth Core already resolves an org-owned value at mint time for service tokens when the registry row says `tenant`: `resolveTenantServiceRetentionPosture` (`CP/auth-core/src/auth/interactive-retention-policy.ts:514-545`) reads org-core through the same 45 s cache, fails closed with 503 `retention_posture_unavailable` and no token, and the signer caps such a token at `TENANT_PRINCIPAL_TOKEN_TTL_SECONDS = 60` (`convex-token.service.ts:150-170`, applied in `issuePlaneToken` at 648-730). master-ownership-matrix rule 16 records this as the sanctioned pattern ("Auth Core as the only authority", enforcers elsewhere). A `web_policy` claim on the `quarry`-audience tokens of execution-core and model-gateway reuses that machinery.
3. org-core already owns what a policy of record needs: an org admin settings route behind the membership guard (`CP/org-core/internal/http/server.go:104`, handler `handlers.go:341-372`), RLS-scoped storage (`repository.go:361-410`, `db.WithOrgScope`), a computed intent-and-entitlement posture (`org/types.go:255-316`, `RetentionPosture.Effective()`), an outbox plus NATS publisher (`migrations/019_interactive_retention_outbox.up.sql`, `internal/nats/shared_publisher.go:179-191`) and plan entitlements. CLAUDE.md assigns identity, orgs, entitlements and audit to Control.
4. capability-core does have an org-scope grant API with audit rows (`MP/go/services/capability-core/internal/api/capabilities.go:57-60, 309-376`), but a grant is one boolean per capability id and `EvaluateCapability` answers allow, deny or ask from `RiskLevel` (`internal/policy/engine.go:84-153`). `cap.browser.open` is seeded HIGH, so it always answers Ask. There is no place to store a proxy ceiling, a robots mode or a domain list, and no evaluation of one.
5. The acquisition routes (`/v1/scrape`, `/v1/scrape/stream`, `/v1/crawl`, `/v1/batch`, the `/v1/search` family, `/v1/map`, `/v1/extract`, `/v1/answer`) carry no grant, and the agent routes use a grant only in production mode (`QE/agent_routes.rs:1377-1383`, `main.rs:183-184`). Option B therefore leaves every user and SDK call on the acquisition API unpinned (and F-t2-064's external contract would inherit that).
6. The broker store is in-memory (`MP/go/services/browser-broker/internal/grant/grant.go:61-64`); a policy that lived there would vanish on restart until WS1 slice 1c lands, and ZDR grants are never persisted by design (D1 D6).
7. The grant is validated on every step already (`QE/agent_routes.rs:2257-2267`), so anything the grant carries is re-read per step without a new RPC. That is the reason to also stamp a snapshot into the grant even though the claim is the authority.

**Consequences of C.**
- Control: org-core gains tables, an admin API and one internal read route with a new scope; auth-core gains a resolver, a claim and one registry field. The registry parity script and the principal-policy contract spec change.
- Quarry reads a claim for **every** principal type (the opposite of `zdr_floor`), so the golden fixtures need service-token cases and three copies of one golden change together (`CP/auth-core/src/auth/fixtures/` [C], `Q/crates/quarry-edge/tests/fixtures/auth-core/` [C], `MP/rust/services/model-gateway/tests/fixtures/auth-core/` [U]).
- Token lifetime: if the 60 s tenant cap were applied to `web_policy` tokens, execution-core's per-(org, scope) token cache (`quarry_auth.rs:17-19`) would refresh about five times as often. Recommendation: **do not apply the 60 s cap**; keep the audience TTL (300 s) and accept a worst-case tightening delay of 300 s plus 45 s. A stricter org can set `WEB_POLICY_TOKEN_TTL_CAP_SECONDS=60`. The ZDR cap exists because a late ZDR change leaks content; a late tightening of upload or robots has a bounded, non-persistent effect.
- A claim missing from a token means "this org has no stored policy" (baseline applies). A claim that is present but unparseable or `v` greater than the Quarry version means "unknown": Quarry fails closed for gated capabilities (typed denial `policy_unreadable`), never open.
- ZDR stays a separate, untouched mechanism (decision 3.7). C does not move ZDR into `web_policy`.

**Recommendation: C.** It is the audit's own recommendation (claim for user paths, grant field for Model-driven runs) with one correction: the Model-driven path does not have to rely on the grant alone, because a service token can carry an org-resolved claim by an existing, reviewed mechanism. Fallback if Control declines the registry field: **A** (everything else in this design is unchanged; S8's registry part drops, S10 becomes mandatory for Model runs and F-t2-001 becomes a hard dependency of org enforcement on those runs). **B is not recommended**: it cannot carry the parameters, cannot reach the acquisition API and would make the Model Plane the owner of org entitlements.

**Reversibility.** High until S7 merges: the owner-neutral slices (S1-S6) work with any outcome, because `ResolvedPolicy` takes an `Option<OrgWebPolicy>` layer whose source is irrelevant to it. After S7-S9 the cost of switching owner is the migration plus three golden regenerations.

**Decision home.** A ledger entry in `MP/docs/decisions/ledger.md` [C] (D1 uses the same file), a row in `docs/CROSS_PLANE_CONTRACT_MATRIX.md` [C] for the new Control-to-Ingestion claim edge, and a "Web policy posture" paragraph next to retention posture in `CP/CONTROL_PLANE_OWNERSHIP.md:355` [C]. The ruling needs the Control Plane owner (registry field) and the Model Plane owner (grant fields).

### 3.2 Value model: tri-state with a declared direction, plus ceilings and rule lists

The card asks for `disabled | allowed | forced` per capability. The three words only have one meaning if each capability declares which direction is the risky one, otherwise "forced" is nonsense for "may upload". Decision: every key in the catalogue declares a **kind**, and the kind fixes the legal values and the tighten direction.

- **ability** (the behaviour is risky when on): legal values `disabled | allowed`; `forced` is rejected at every layer constructor (a build-time error in Rust, a 422 in org-core). Meaning: `disabled` = the behaviour never happens, a request for it is denied; `allowed` = it happens only if the request asks and every other layer agrees. Tightening = `allowed` to `disabled`.
- **protection** (the behaviour is safe when on): legal values `allowed | forced`; `disabled` is rejected. `forced` = always on regardless of the request; tightening = `allowed` to `forced`. In v1 there is exactly one protection candidate, ZDR, which is deliberately **not** a policy key (3.7). The slot exists so credential stripping can be modelled when F-t2-005 gives it a request option; until then it is an ability named `request.credentials`.
- **ceiling** (ordered enum, the policy value is the most permissive allowed): `robots.max_mode` (`strict < respect < ignore`), `proxy.max_tier` (`none < pool` in v1; F-t2-058 appends tiers). Meet = minimum.
- **rules**: `domains.allow` (optional list, autonomous principals only: the agent fence) and `domains.deny` (list, all principals). Meet of two allow lists is the intersection, of two deny lists the union. Matching is exact host or dot-suffix (the rule `domain_allowed` already uses, `QE/agent_routes.rs:442-444`); **no wildcards** until `isCanonicalDomain` and `BrowserEgressPolicy` change together (F-t2-032 risk).

The catalogue (v1). "Applies to" is `all` or `autonomous` (service principals and agent-initiated steps; the human user's own manual browsing is never gated by an autonomous-only key, which honours the card's rule). "Site" is where WS7 enforces it; "reserved" means the option the key would gate does not exist yet, so the key is accepted by the contract, stored, and enforced the day its option lands, and a test fails if an option appears without a `require()` call.

| Key | Kind | Applies to | Baseline | Today's behaviour | Enforcement site |
|---|---|---|---|---|---|
| `browser.act.script` (evaluate, raw script) | ability | all | `disabled`, invariant, not unlockable | no gate at all (`QR/observation.rs:542`); D2 S1 hard-denies | step REST and WS, `ObservationRunner`, `ActionRuntime` (D2 S1 owns the code; WS7 maps it into the catalogue) |
| `browser.act.input` (Type, TypeRef, FrameTypeRef, TypeSemantic, Select\*, Press: login and form-write) | ability | autonomous | `allowed` | only execution-core's risk gates (`MP/.../browser_risk.rs`) | `step` and WS (S3) |
| `browser.act.interact` (Click\*, ClickPoint, RespondDialog) | ability | autonomous | `allowed` | none in Quarry | `step` and WS (S3) |
| `browser.act.upload` (UploadRef, FrameUploadRef) | ability | all | `allowed` (still one-shot approval-gated) | sensitive approval (`agent_routes.rs:184-232`) | `step`, `start_run` (`driver_requirements.uploads_from_artifacts`) (S3) |
| `browser.act.download` (DownloadRef, FrameDownloadRef) | ability | all | `allowed` (approval-gated) | sensitive approval (233-283) | same |
| `browser.profile.persistent` | ability | all | `allowed`; ZDR implies `disabled` | `zdr_forbids_persistent_profile` (`agent_routes.rs:527-533, 1470-1476`); `/v1/profiles` checks org only (`QE/profile_routes.rs:102-432`) | `start_run`, resume, profile routes (S3, S4) |
| `browser.recording` | ability | all | `disabled` | no request option; driver config `recording:false` (`QE/main.rs:320`) | reserved |
| `search.paid_provider` | ability | all | `allowed`; ZDR implies `disabled` | `allow_paid_providers && !zdr` (`QE/search_routes.rs:529`) | `build_search_options` (S4) |
| `request.credentials` (Authorization, Cookie, Proxy-Authorization, URL userinfo, request cookies) | ability | all | `disabled` | no request option exists | reserved for F-t2-005 |
| `robots.max_mode` | ceiling | all | `respect` | not enforced in `PageRunner` | reserved for F-t2-016 |
| `proxy.max_tier` | ceiling | all | `none` (no pool) | `ProxyStrategy::None` default, `block_rotation` rotates on bare 403 | reserved for F-t2-058 |
| `domains.allow` | rule list | autonomous | unset (no extra fence) | broker grant domains only | `start_run` (egress narrowing), Navigate, scrape-family URL pre-check (S2, S3) |
| `domains.deny` | rule list | all | unset | URLhaus and PhishTank snapshot only | scrape family, extract, map, crawl and batch seeds, Navigate, post-action URL (S2-S4); sub-resource denial needs a `BrowserEgressPolicy` extension (S6) |
| ZDR | read-only here | all | from claim and body | `effective_zdr` | see 3.7 |

Not in v1, listed so nobody re-derives them: `ingest.persist` (a scrape's `ingest:true`), `crawl.external_links` (F-t2-016), `browser.multi_tab`, `cookies.import`. Adding a key is additive: edit the catalogue JSON, regenerate, bump nothing unless semantics change (3.9).

**One catalogue, four consumers.** The catalogue is a JSON document generated by the Rust code that enforces it (`Q/crates/quarry-core/contracts/web-policy-catalogue.v1.json` [NEW], regenerated with `UPDATE_CONTRACT_GOLDEN=1`, pinned by a `.sha256`), because Quarry is the only party that knows which keys it can actually enforce. org-core (Go), auth-core (TypeScript) and browser-broker (Go) vendor byte-identical copies, registered in `scripts/tests/zdr-contract-sync.test.sh` GOLDENS and VENDORED the way the existing goldens are. This is the guard against the audit's named risk "policy resolved in two places can drift": the vocabulary has one source and each consumer's test fails when its copy differs.

### 3.3 Layers and the merge

`ResolvedPolicy = meet(baseline, org, grant, ask)` over the catalogue, plus `zdr` taken from the existing floor and the invariants. Five inputs, one function, in `quarry-core` (no I/O, like the rest of that crate; `QC/lib.rs:1-5`).

| Layer | Source | Present when | Can it widen? |
|---|---|---|---|
| Invariants | code constants | always | n/a (`browser.act.script` is `disabled` and not unlockable) |
| Baseline | `QUARRY_EDGE__WEB_POLICY_BASELINE_JSON` via `policy_baseline.rs` [NEW] | always (default document = today's behaviour) | no: it is the starting upper bound |
| Org | `web_policy` token claim (S9) | org has a stored non-default policy | no: the meet can only lower the baseline's upper bound |
| Grant | `GrantValidation.policy` from the broker (S10) | production runs | no |
| Ask | `RequestAsk` in the body (S2) | caller supplies it | no: the type cannot express a loosening (see 3.4) |

Merge rule per kind: ability and protection take the more restrictive value; ceilings take the minimum; allow lists intersect; deny lists union. An org-only fact (entitlement) never appears in Quarry: org-core folds it into the claim before minting (3.12).

**`decision_source`** of a denial is the layer that produced the final restrictive value, ties broken in this order: `invariant`, `zdr`, `org_policy`, `grant`, `baseline`, `request`. The stable wire strings are those six words.

### 3.4 Request layer: tighten-only by construction

`RequestAsk` is a separate type with `#[serde(deny_unknown_fields)]` whose fields can only restrict: `disable: Vec<CapKey>`, `robots_max`, `proxy_max`, `deny_domains`, `allow_domains` (interpreted as an intersection). An attempt to loosen is unrepresentable and therefore a 400 `unknown_field`, not a silent ignore. Spaces, the planner and the gateway can only enter policy through this type (rule "Space access never widens an owner-plane ACL"); a Space-level restriction is just an ask that model-gateway builds from the Space's own settings. Unknown capability keys are 400 `unknown_capability` (a typo must not become a no-op restriction). Presets are not a layer: a job-level `JobPolicy` (section 3.10) is translated into an ask by the caller and is subject to the same meet.

### 3.5 Typed denials

- No new `ErrorCode`. The denial is `ErrorCode::Forbidden` (HTTP 403, non-retryable, `QC/error.rs:41-70`) with `details`:
  `{"reason":"policy_denied","capability":"browser.act.upload","decision_source":"org_policy","denied_action":"upload_ref","policy_rev":"7f3a9c1e2b4d","retryable":false}`.
  `policy_rev` appears only when the org layer decided. The edge renders it through the existing `driver_err` (`agent_routes.rs:1116-1122`), which preserves code and details; it must **not** go through `status_err`, which stamps every status as `BAD_REQUEST` (`1106-1114`; D2 slice S6 fixes that mislabel and is a soft dependency).
- Closed set of `reason` strings: `policy_denied`, `raw_script_denied` (D2 S1's string for evaluate, kept byte-for-byte; it additionally carries `capability:"browser.act.script"` and `decision_source:"invariant"`), `policy_unreadable` (a present claim Quarry cannot parse), `unknown_capability`, `ceiling_exceeded` (carries `max` and `requested`), `domain_denied` (carries `host`), `domain_not_allowed`.
- Modelled on `CrawlDenialReason` (`QC/crawl_denial.rs:12-37`): a `PolicyDenial` enum with `code()` and `message()`, serde round-trip test, unique-code test.
- The denial is raised **before** anything is consumed: before `sensitive_approval_for_action` inserts the one-shot grant, before `enforce_step_budget`, before a receipt. Ordering in `step`: owner check, grant validation, capability proof, `deny_raw_script` (D2 S1), **policy gate** (WS7), takeover and initiator (D2 S3), sensitive approval, budget, domain. A denied upload with a valid approval grant must leave the grant unused.
- Telemetry: one event type `policy.denied` (S2) with `{capability, decision_source, denied_action, policy_rev}` only. Under ZDR the event goes through `emit_for_zdr` and carries no URL, host or script (the rule from F-t2-004: never log a URL inside a ZDR branch).
- Execution-core side (sibling b): a `details.reason == "policy_denied"` response maps to `ObservationStatus::Blocked` with a fixed, non-negotiable line ("denied by organisation policy: <capability>; do not retry or try another route") so the planner stops circumventing. A `policy_denied` is never offered to the approval UI: org policy is not approvable (only a domain miss against the user's own grant is, and that belongs to E-032).

### 3.6 Humans versus autonomous principals

`autonomous` = the step's verified principal is a service principal (`Claims::is_service`, `QE/auth.rs:101-103`). D2 slice S3 derives an explicit `StepInitiator` from the same fact; WS7 consumes `entry`'s initiator when D2 S3 has merged and falls back to `claims.is_service()` before that. Autonomous-only keys (`browser.act.input`, `browser.act.interact`, `domains.allow`) are skipped for a user principal driving their own session. `browser.act.upload`, `.download`, `browser.profile.persistent`, `domains.deny` apply to everyone because they are data-movement and security decisions an admin makes for the org; `domains.deny` for humans is the one place where this design could surprise a user and is flagged in section 9 as a non-blocking confirmation.

### 3.7 ZDR is carried, never re-modelled

Decision: **ZDR is not a key in `web_policy` and not part of the claim.** `ResolvedPolicy.zdr` is exactly `claims.effective_zdr_mode(request_flag)` (`QE/auth.rs:129-138`) with the resume rule of `agent_routes.rs:1440-1445`. Reason: two sources for one org fact is how a ZDR org ends up non-ZDR; the existing golden, `zdr_claim_floor.rs` and the contract-sync gate already guard this path, and the matrix (rules 4, 16, 17) forbids any admin or policy table from lowering ZDR organisation-wide.

What WS7 adds is the **ZDR implication table** in one place: `ResolvedPolicy::zdr_forces_disabled(cap) -> bool` returns true for `browser.profile.persistent`, `search.paid_provider` and `browser.recording`, and (in the durable-job gate) for crawl and batch. That is the matrix F-t2-003 wants generated from code. **Existing refusals are not loosened and not removed in S2-S4**: `refuse_durable_job_under_zdr` (`QE/routes.rs:563-588`), `zdr_forbids_persistent_profile` (`agent_routes.rs:527-533`) and `allow_paid_providers && !zdr` (`search_routes.rs:529`) stay as written and run alongside the new `require()`. S4 ends with an equivalence test (`zdr_implications_match_the_legacy_refusals`) and only then a follow-up PR may delete the legacy checks.

### 3.8 Enforcement placement: edge pre-check plus three backstops

1. **Edge pre-check (typed).** Every handler resolves a `ResolvedPolicy` once and calls `require()` or `check_url()` before doing work. This is the only place that produces the typed 403.
2. **`SecurityEngine` decorator backstop (S6).** A `PolicySecurityEngine { inner, resolved }` implements `quarry_security::SecurityEngine` (`Q/crates/quarry-security/src/lib.rs:48-55`: `preflight`, `check_discovered`, `allow_private_hosts`). Handlers wrap `state.security.clone()` per request. `PageRunner::run_inner` already calls `self.security.preflight` first (`QR/pipeline.rs:305`) and the agent path calls it from `validate_navigation` (`agent_routes.rs:109-133`), so **no `PageRunner` field is added** and none of its literals (3 production, 3 test) change for this. A blocked URL becomes `SecurityBlocked` with a reason prefix `policy_denied:` (stringly, by the trait's shape); the typed form is the edge's job.
3. **Redirect and final-URL backstop (S6).** After the fetch, before persist or ingest, `run_inner` re-runs `preflight` on `final_url` when it differs from the requested URL. Today only the requested URL is preflighted (fetch re-validates redirect hops for SSRF only, `QR/fetch.rs:193-217`). Side benefit: URLhaus and PhishTank now also apply to a redirect target. Risk noted in S6.
4. **Per-step re-resolution (S3).** `step` recomputes `ResolvedPolicy` from the caller's current claims, the grant just validated, and facts the run stored at start (`persist_profile`, tier). Because the grant is validated every step, no new RPC exists. Mid-run the policy can only tighten (decision 3.14).

**Org domain narrowing must not touch `constraints.allowed_domains`.** `validate_run_grant` requires that list to equal the broker's canonical list at every step in governed mode (`agent_routes.rs:326-335`); narrowing it would produce 403 "run domain policy no longer matches its broker grant" on the second step. So narrowing is applied in three other places only: the egress policy handed to the driver (`BrowserEgressPolicy::from_allowed_domains(&policy.narrow(&constraints.allowed_domains))` at `agent_routes.rs:1521-1531`; an empty intersection must produce `BrowserEgressPolicy::deny_all()`, because `from_allowed_domains(&[])` is **unconstrained**), the Navigate pre-check (`2297-2305`) and the post-action URL check (`2400-2406`).

### 3.9 Contract versioning

`web_policy.v` and the catalogue `version` are both `1`. Adding a key, a legal value or a tier is additive and does not bump. Changing the meaning of a key, a kind or a merge rule bumps to `2`; a Quarry that sees a higher major fails closed (3.1). The catalogue JSON carries `version` and a per-key `since`.

### 3.10 Reconcile the policy shapes (no third shape)

- **`QC/policy.rs` `RunPolicy` is renamed `JobPolicy`** (serde and wire unchanged). It is the CONTRACTS §7 job-intent shape mirrored by Go `quarrycontracts.RunPolicy` and stored by quarry-control; deleting it would diverge from Go and from `GET /v1/presets`. After the rename the three layers read cleanly: `JobPolicy` (what a job asked for, wire), `ResolvedPolicy` (what is allowed and will happen, effective), `RunPolicy` in `quarry-runtime` (the determinism stamp, now constructed only through `RunPolicy::from(&ResolvedPolicy)`). Rejected: deleting the core type (Go parity), renaming the runtime type (about 12 sites and the artifact-stamp vocabulary), keeping both names (the audit's finding).
- **Presets.** Go presets stay (they are served by `GET /v1/presets` and used at job creation) but gain `Requires []string json:"requires,omitempty"`; `fast` declares `["robots.max_mode=ignore"]`, `stealth` declares `["proxy.max_tier=pool"]`. A Go test fails if any preset whose policy exceeds the catalogue baseline lacks `requires`. When F-t2-016 and F-t2-017 later forward `JobPolicy` to `run_page`, the meet refuses `fast` for any org that has not been granted the ability. The `stealth` preset itself (and `stealth.rs`, which has no caller) belongs to audit topic 5 and F-t2-027; WS7 does not delete it. `QC/presets.rs` (six crawl bundles, no production caller) is left alone and noted for F-t2-005/017.
- **`/v1/blocklists` is retired, not wired**: it is org-less (a cross-tenant hazard if the edge ever read it), unread, and superseded by `domains.deny` (org-scoped, in policy) for tenant rules and by the URLhaus/PhishTank snapshot for global threat feeds. S5 stops mounting it (`410 Gone` with a pointer to the policy API) and leaves the table untouched (no data migration, rollback is a one-line remount). Verified no consumer exists in the live tree.
- **`QUARRY_EDGE__RUN_POLICY`**: the unread knob in the `main.rs:1489-1492` comment is deleted with the `AppState.policy` type swap.

### 3.11 Baseline and rollout switch

`PolicyBaseline` is parsed by `PolicyBaseline::from_env()` in `QE/policy_baseline.rs` [NEW] from `QUARRY_EDGE__WEB_POLICY_BASELINE_JSON` (same schema as the claim; default document reproduces today's behaviour exactly) and **fails boot on an unparsable value**, matching the production refusals in `main.rs:128-215`; it is deliberately not a field of `EdgeConfig`, because `EdgeConfig` falls back silently to defaults on a deserialize error (`QE/config.rs:334-346`, S9 section 2.3), which is wrong for a safety limit, and because `config.rs` carries a 187-line uncommitted diff. `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT=off|observe|enforce` (default `enforce`): `observe` computes everything, logs `web_policy_would_deny_total{capability,decision_source}` and enforces nothing; `off` skips computation except invariants. Invariants (evaluate) are enforced in every mode. In S2-S4 the default document equals today's behaviour, so `enforce` introduces no new denial except those the caller explicitly asks for through `RequestAsk`.

### 3.12 Entitlements fold into the claim in org-core (outcome A and C)

org-core computes `effective = admin_settings AND plan_entitlements`, the way `RetentionPosture.Effective()` is `intent && entitled` (`org/types.go:314-316`). Entitlement keys: `web.robots_override` (needed for `robots.max_mode=ignore`), `web.proxy_pool` (for `proxy.max_tier=pool`, F-t2-058), `web.paid_search` (for `search.paid_provider=allowed`). An org whose plan lapses keeps its stored intent but its effective policy tightens, with a warning, exactly as ZDR does. Quarry never sees entitlements. Which capabilities are plan-gated is a product input (section 9, Q3); the default proposed is "only the three above".

### 3.13 Revocation mid-run, given 15-minute grants (answers audit open question 1)

Under outcome C the enforcement authority is the claim on each request, not the grant, so revocation latency is the claim's staleness and not the grant TTL:
- user path: the gateway mints a plane token on every request (`audience_tokens.rs:93-98`), so a stale claim is at most the resolver cache (45 s) [the browser WebSocket relay may hold one token longer: UNVERIFIED];
- service path: execution-core caches the Quarry token per (org, scope set) for its lifetime with a 30 s refresh skew (`quarry_auth.rs:17, 200-260`), so the bound is token TTL 300 s + 45 s = 345 s (105 s if the cap is 60 s);
- grant path: the 15-minute broker TTL (`MP/go/services/browser-broker/internal/server/server.go:22`) bounds only what the grant snapshot says, and the snapshot can only be as tight or tighter than the claim at issue time.
Per-step effective policy is `meet(claim_now, grant_snapshot, run_facts)`. A loosening takes effect for new grants only (the snapshot keeps the run tighter until the grant expires), a tightening takes effect on the next step carrying a fresh claim. Optional hardening (S12): org-core emits a tightening event, the broker revokes or re-derives the org's live grants. It is off by default because revoking a session for a minor tightening is a worse user outcome than a 6-minute delay. Under outcome B or A-without-service-claim the grant TTL (15 min) is the honest bound and S12 becomes mandatory.

### 3.14 Mid-run monotonicity and resume

A run records nothing about policy except what it already stores. On resume (`start_run` with `resume_run_id`), the persisted constraints stay authoritative (`agent_routes.rs:1362-1368`) and the policy is re-resolved from the resuming caller's claim; the resume is refused with a typed denial if the persisted run facts (a persistent profile, an upload-requiring driver contract) now violate the current meet, exactly as ZDR already refuses a resume that needs a persistent profile (`1433-1445`, `1470-1476`).

### 3.15 What F-t2-032 must reuse

Typed denial vocabulary (3.5), the action-class mapping (the table in 3.2), the `RequestAsk` type, the claim and grant carriers, and the rule that consent is a **request-layer** input: a user's per-origin approval can only add to the run's grant-borne domain list inside the org's `domains.allow` and outside `domains.deny`; it can never relax an org-pinned capability. "Any deny beats any approval" is the meet.

## 4. Slices (PR-sized, in build order)

Build order: **S1 to S6 are owner-neutral and need no ruling**; **S7 to S12 follow the policy-of-record ruling** (audit ADR topic 1). S1 first (pure library), S2 next (the first deployed behaviour), S3 and S4 can run in parallel after S2, S5 is independent after S1, S6 last of the neutral set (it carries the only soft dependencies).

Commands used throughout (CLAUDE.md and the S9 sweep). From `Q`: `cargo test -p quarry-core`, `cargo test -p quarry-runtime`, `cargo test -p quarry-edge`, CI-equivalent `cargo check --workspace && cargo test --workspace` (`.github/workflows/quarry-v2-ci.yml:201-205` [C]). The Windows host cannot build `quarry-runtime` or `quarry-edge` natively (boring-sys2 needs cmake, clang, perl, go, protoc): run them in a `rust:1` container with the `quarry-cargo` and `quarry-target` cache volumes and `bash -c` (not `bash -lc`); a combined `cargo test -p quarry-core -p quarry-runtime -p quarry-edge` runs more than 10 minutes, so run it in the background (memory note `quarry-rust-tests-in-docker`). Whether `quarry-core` alone builds on the host is [UNVERIFIED]; use the container if it does not. Go: `cd "Q/services/quarry-control" && go test ./...`. Control: `cd "CP/org-core" && go test -p 1 ./...` with the Postgres bootstrap of `org-service.yml:74-103`; `cd "CP/auth-core" && npx jest <file>` (baseline has 15 pre-existing failures in three suites: compare failing sets with HEAD, do not trust the exit code). Drift gate: `bash scripts/tests/zdr-contract-sync.test.sh` and `--self-test`. Model Plane: `cd "MP/go" && go test ./services/browser-broker/...`; `cd "MP/rust" && cargo test -p model-gateway --lib browser_run` (two cargo targets fail on Windows: use `--lib`); proto regeneration through the `bufbuild/buf` Docker image (buf is not installed on the host). Gateway (only if touched): `cargo test --manifest-path "FE/apps/gateway/Cargo.toml" --all-targets`. Stack: `cd "apps/Ingestion Plane" && make up` with the env-file chain of memory note `ingestion-plane-deploy-recipe` (the Makefile passes only two env files, so some interpolations may be unset [UNVERIFIED]). Line endings: Quarry and Control files in the live tree show `LF will be replaced by CRLF` warnings, so scripted edits must preserve each file's endings (memory note `auth-core-local-testing`).

---

### S1. `quarry_core::web_policy`: catalogue, lattice, `ResolvedPolicy`, typed denials (effort M, 3 engineer-days) [owner-neutral]

**Goal.** One pure, I/O-free module that answers every policy question Quarry will ever ask, with the vocabulary of 3.2, the merge of 3.3 and the denials of 3.5, plus the catalogue JSON the other planes vendor. Nothing is wired yet.

**File-level change list**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QC/web_policy/mod.rs` | `CapKey` (serde as `"browser.act.upload"`), `Kind {Ability, Protection, Ceiling, Rules}`, `AppliesTo {All, Autonomous}`, `Mode {Disabled, Allowed, Forced}`, `RobotsMax`, `ProxyMax`, `DomainRules`, `PolicyLayer` (claim/baseline/grant shape, `v`, optional `rev`), `RequestAsk` (`deny_unknown_fields`, tighten-only fields), `Principal {Human, Autonomous}`, `PolicyInputs`, `ResolvedPolicy` | new; `ResolvedPolicy::resolve(&PolicyInputs) -> Result<Self, PolicyDenial>`, `require(CapKey, Principal) -> Result<(), PolicyDenial>`, `allows(CapKey, Principal) -> bool` (for downgrade-style flags), `check_url(&Url, Principal)`, `narrow_egress(&[String]) -> EgressNarrowing {Unchanged, Narrowed(Vec<String>), DenyAll}`, `zdr()`, `zdr_forces_disabled(CapKey) -> bool`, `digest() -> String` (blake3, for logs only) | [NEW] |
| `QC/web_policy/catalogue.rs` | `CATALOGUE: &[CapSpec]` with `key, kind, applies_to, baseline, unlockable, site: EnforcementSite {Edge, Step, Egress, Reserved(&'static str)}`; `fn catalogue_json() -> serde_json::Value` | the table of 3.2 | [NEW] |
| `QC/web_policy/action_class.rs` | `enum ActionClass {Read, Navigate, Interact, Input, Dialog, Upload, Download, Script}`, `fn action_class(&AgentAction) -> ActionClass`, `ActionClass::cap_key() -> Option<CapKey>` | **exhaustive match over the 33 `AgentAction` variants with no wildcard arm** (`QC/contracts.rs:640-826`), so adding an action is a compile error until it declares its class (the property F-t2-032 takes from Aside) | [NEW] |
| `QC/web_policy/denial.rs` | `PolicyDenial` enum (variants of 3.5), `DecisionSource` (six values), `code()`, `message()`, `impl From<PolicyDenial> for QuarryError` (`ErrorCode::Forbidden`, `details` per 3.5, `retryable=false`) | modelled on `QC/crawl_denial.rs` | [NEW] |
| `QC/lib.rs` | `pub mod web_policy;` | one line, alphabetical position after `tracked_snapshot` | [C] |
| `Q/crates/quarry-core/contracts/web-policy-catalogue.v1.json` + `web-policy-catalogue.v1.sha256` | generated catalogue (`version:1`, per key: `key, kind, applies_to, baseline, unlockable, since, site`) | generated by the test below with `UPDATE_CONTRACT_GOLDEN=1`, never hand-edited; LF endings | [NEW] |
| `Q/crates/quarry-core/tests/web_policy.rs` | integration tests | see Tests | [NEW] |
| `scripts/tests/zdr-contract-sync.test.sh` | GOLDENS row `Q/crates/quarry-core/contracts/web-policy-catalogue.v1.json\|<sha256>` | append after the in-flight rows (the file carries a 95-line uncommitted diff; rebase) | [M] |

**API / data-model / migration.** None. The only artefact other crates see is the JSON catalogue. No `ErrorCode` is added, no proto, no SQL.

**Feature flag and rollout.** None (library only). Rollback is a revert.

**Tests (unit and property, `cargo test -p quarry-core web_policy`)**
- Lattice laws (proptest, the crate already has `proptest = "1"` as a dev-dependency): `meet_is_commutative_associative_idempotent`, `resolved_is_never_more_permissive_than_any_input_layer`, `ask_cannot_express_a_loosening` (a JSON body containing `"allow":["browser.act.upload"]` fails to deserialize), `forced_on_an_ability_is_rejected_at_construction`, `disabled_on_a_protection_is_rejected`.
- Decision-source ordering: `decision_source_prefers_invariant_then_zdr_then_org_then_grant_then_baseline_then_request`.
- `action_class_is_total` (compile-time via the exhaustive match; plus a runtime table check that every variant of a `strum`-free list of the 33 names maps) and `evaluate_maps_to_script_and_script_is_never_unlockable`.
- Domain rules: `deny_wins_over_allow`, `suffix_match_requires_a_dot_boundary` (`evilexample.com` does not match `example.com`), `empty_intersection_is_deny_all_not_unconstrained`, `ip_literals_and_wildcards_are_rejected_in_a_layer`, `rules_cap_at_64_entries_and_2_KiB`.
- ZDR: `zdr_is_taken_from_the_floor_and_cannot_be_lowered_by_any_layer`; `zdr_forces_disabled_covers_profile_paid_search_and_recording`.
- Humans versus autonomous: `autonomous_only_keys_are_skipped_for_a_human`; `upload_applies_to_humans_too`.
- Denial wire shape: `denial_serializes_to_the_documented_details`, `codes_are_unique_and_stable`, `raw_script_denied_is_byte_compatible_with_d2` (reason `raw_script_denied`, plus the two new fields).
- Catalogue contract: `catalogue_json_matches_the_checked_in_file_and_its_pin` (the pin test pattern of `zdr_claim_floor.rs:401`), `every_key_has_a_site_or_is_reserved`, `reserved_keys_are_listed_so_a_new_option_must_call_require` (a table of option-field names that must not exist yet; when F-t2-005 adds `headers` the test fails until it calls `require(RequestCredentials)`).
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-core web_policy`; regenerate: `UPDATE_CONTRACT_GOLDEN=1 cargo test -p quarry-core --test web_policy`; gate: `bash scripts/tests/zdr-contract-sync.test.sh`.

**Definition of done.** Property tests green under 4096 cases; the exhaustive action match compiles; catalogue JSON, its pin and the sync-script row agree; `cargo doc -p quarry-core` shows no public item named `RunPolicy` change (rename is S5).

**Deployment evidence required.** None claimed: S1 ships no behaviour. Evidence is the test run (container, log kept in the PR), nothing deployed.

**Risks and mitigations.** Over-design: the catalogue ships with five keys that have an enforcement site (`browser.act.*`, `domains.*`, `browser.profile.persistent`, `search.paid_provider`) and five reserved; reserved keys are inert and test-guarded. Naming churn: key strings are public once the claim exists, so S1 is the last cheap moment to rename them; review them with Control before S7.

---

### S2. Edge wiring for the scrape family: replace `RunPolicy::default()`, typed denial, `RequestAsk`, `policy_baseline.rs` (effort M, 4 engineer-days) [owner-neutral]

**Goal.** Every production `RunPolicy::default()` site is replaced by a per-request `ResolvedPolicy`; `/v1/scrape`, `/v1/scrape/stream` and `/v1/internal/run_page` refuse a denied URL with the typed 403 before touching cache, driver or ingest; callers can tighten through `RequestAsk`; the dead `AppState.policy` field and the dead `QUARRY_EDGE__RUN_POLICY` comment go away.

**The sites replaced (all path:line verified against the live tree)**

| Site | Today | After |
|---|---|---|
| `QE/routes.rs:461` (`scrape`) | `policy: RunPolicy::default()` | `policy: RunPolicy::from(&*resolved)`; `resolved` computed after the ZDR floor at 416 |
| `QE/routes.rs:1116` (`scrape_stream`) | same | same, after 1072 |
| `QE/routes.rs:1001` (`internal_run_page`) | same, no claims available | `resolved` from baseline plus invariants (no org layer on this JWT-less route until S6) |
| `QE/main.rs:1493` and `QE/state.rs:106` | `AppState.policy: RunPolicy` (unread) | `AppState.policy: PolicyBaseline`, `PolicyBaseline::from_env()` (boot fails on a bad value) |
| `QE/routes.rs:1332`, `QE/test_support.rs:129`, `Q/crates/quarry-edge/tests/support/mod.rs:74` | `RunPolicy::default()` in test literals | `PolicyBaseline::default()` |

`QR/pipeline.rs:1191`, `1332` and `Q/crates/quarry-core/tests/contracts.rs:95` are runtime-internal test fixtures of the stamp type and stay.

**File-level change list**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QE/policy_baseline.rs` | `PolicyBaseline` (a `PolicyLayer` plus `Enforcement {Off, Observe, Enforce}`), `from_env()`, `Default` = today's behaviour, `resolve(&Claims, Option<RequestAsk>, ZdrMode) -> Result<Arc<ResolvedPolicy>, PolicyDenial>` | new; reads `QUARRY_EDGE__WEB_POLICY_BASELINE_JSON` and `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT`; parse failure returns `Err`, `main` aborts | [NEW] |
| `QE/web_policy_edge.rs` | `fn denial_response(request_id, PolicyDenial) -> (StatusCode, Json<Envelope<()>>)` (403, code `FORBIDDEN`, details per 3.5); `fn record_denial(state, claims, &PolicyDenial, zdr)` (metric `web_policy_denied_total{capability,decision_source}`, event `policy.denied` through `emit_for_zdr`, no URL); `fn principal(&Claims) -> Principal` (service means autonomous) | new | [NEW] |
| `QE/state.rs` | field `policy` | type change only | [M] |
| `QE/main.rs` | line 1493 and the comment 1489-1492; `mod policy_baseline; mod web_policy_edge;` placed in the module block away from line 47 | edit | [M] |
| `QE/lib.rs` | same two `mod` lines (the crate has both a bin and a lib root, as D2 found) | edit | [M] |
| `QE/routes.rs` | `ScrapeRequest` (add `#[serde(default)] pub policy: Option<RequestAsk>`, struct at 346-379); `scrape` (after URL parse 424-430, before the cache read 432: `resolved.check_url(&url, principal)?`); `scrape_stream` (same position); `internal_run_page` (check after URL parse about 984) | edit; the check runs **before** the cache so a cached page for a denied host is refused too | [M] |
| `QR/policy.rs` | `impl From<&ResolvedPolicy> for RunPolicy` | default resolved policy returns `RunPolicy::best_effort()` unchanged; `robots_max == Strict` sets `discovery.respect_robots = true` (already default) | [C] |
| `Q/docs/openapi.yaml` | components `RequestAsk`, `PolicyDenial`; property `policy` on `ScrapeRequest`; a 403 example | additive, away from the in-flight hunks at 238-250 and 4012-4150 | [M] |
| `Q/sdks/typescript`, `Q/sdks/python` | generated | regenerate with `Q/sdks/generate.sh`; commit only if the diff is limited to the new schemas | [C] |

**API / contract changes.** New optional request field `policy` (a `RequestAsk`) on `POST /v1/scrape` and `/v1/scrape/stream`; unknown keys inside it are 400. New 403 body `error.details` per 3.5. Env: `QUARRY_EDGE__WEB_POLICY_BASELINE_JSON`, `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT`. No proto, no migration.

**Flags and rollout.** `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` default `enforce`; the default baseline document reproduces today's behaviour, so the only new denials are those a caller or operator asks for. Roll out as a normal edge image deploy; to stage an operator baseline, start in `observe` and watch `web_policy_would_deny_total`. Rollback: revert, or set `off` (invariants only).

**Tests**
- Unit (`QE/policy_baseline.rs`): `default_baseline_equals_todays_behaviour`, `bad_json_fails_from_env`, `observe_never_denies_but_counts`, `off_still_enforces_invariants`.
- Runtime (`QR/policy.rs` tests): `default_resolved_policy_has_the_same_policy_fingerprint_as_best_effort` (protects the `DeterminismStamp` and cache identity), `strict_ask_changes_the_fingerprint`.
- Integration `Q/crates/quarry-edge/tests/web_policy_scrape.rs` [NEW], built on the real-token harness of `zdr_claim_floor.rs` (RS256 against a JWKS mock, real `require_auth`, real router, tokens from the checked-in golden): `scrape_denied_domain_is_403_policy_denied_before_the_cache`, `scrape_stream_denied_domain_emits_the_error_event`, `run_page_denied_domain_is_403`, `ask_cannot_loosen_a_baseline_deny` (a request carrying `deny_domains: []` leaves the baseline deny in force), `ask_with_unknown_capability_is_400`, `ask_with_unknown_field_is_400`, `zdr_floor_still_wins_when_the_ask_is_empty` (the existing behaviour, so the new path does not regress `zdr_claim_floor.rs`), `denial_event_carries_no_url_under_zdr`.
- Regression: `cargo test -p quarry-edge --test zdr_claim_floor` unchanged and green (all 23 tests, including `the_golden_is_the_one_its_pin_names`).
- Commands: `cargo test -p quarry-runtime policy`; `cargo test -p quarry-edge --test web_policy_scrape --test zdr_claim_floor`; CI `cargo test --workspace`; `node --test "apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs"` (the OpenAPI contract test is untracked [U]; run it from the live tree).

**Definition of done.** Zero `RunPolicy::default()` outside tests and the runtime's own fixtures (a grep gate in the PR); `AppState.policy` is a `PolicyBaseline`; the three handlers deny before any I/O; OpenAPI and SDK text updated; a default scrape's `determinism.policy_fp` is byte-identical before and after.

**Deployment evidence required.** (1) Rebuild the quarry-edge image through the Ingestion compose (`docker compose ... up -d --build quarry-edge` with the recipe env chain) and show the new image id and `GET /version` revision. (2) Against the running edge, with a user-principal token obtained the way the gateway does (`get_audience_token(..., "quarry")`; record the exact mint command in the PR [UNVERIFIED]): `POST /v1/scrape {"url":"https://example.org/"}` returns 200 and note `data.determinism.policy_fp`; the same call with `"policy":{"deny_domains":["example.org"]}` returns **403, `error.code == "FORBIDDEN"`, `error.details.reason == "policy_denied"`, `capability == "domains.deny"`, `decision_source == "request"`**; restart the edge with `QUARRY_EDGE__WEB_POLICY_BASELINE_JSON={"domains":{"deny":["example.net"]}}` and the plain call for `example.net` returns 403 with `decision_source == "baseline"` while `example.org` still returns 200; `docker logs quarry-edge` shows the `policy.denied` line with no URL; `data.determinism.policy_fp` of the 200 equals the value recorded before the deploy. (3) A user token of a ZDR org: the scrape still runs ZDR (artifacts absent) exactly as `zdr_claim_floor.rs` asserts.

**Risks and mitigations.** Behavioural regressions through the type swap: the default document is today's behaviour and the fingerprint test pins it. In-flight diffs in `main.rs`, `state.rs`, `routes.rs`: the edits are single lines, rebase rather than merge blind. `internal_run_page` has no org layer yet (a durable crawl cannot be pinned until S6): documented in the PR and in section 8.

---

### S3. Agent path: start-run and per-step gates (effort M, 4 to 5 engineer-days) [owner-neutral, ordering dependency on WS2/D2]

**Goal.** `start_run`, `step`, the WebSocket action path and the tab routes refuse what the resolved policy forbids, with the typed denial, before any approval is consumed; the org `domains.allow` narrows the browser egress policy without breaking the grant-equality invariant; the policy is re-resolved on every step.

**Ordering with WS2/D2.** Land after D2 S1 (evaluate hard deny) and preferably after D2 S2 (module split, which freezes `agent_routes.rs` for two working days) and S3 (the `admit_action` pipeline). With the split, the new gate is one line in `step.rs` and `execute_ws_action`'s list and the new code lives in a new module; without it, the same edits land in the 2228-2409 and 3086-3193 bodies and the rebase cost is higher. The design works either way.

**File-level change list**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QE/agent_policy_gate.rs` | `fn gate_policy(state, claims, entry, grant: Option<&GrantValidation>, action: &AgentAction) -> QuarryResult<()>` (resolves per step, `action_class`, `require`, `check_url` for Navigate); `fn resolve_for_start(...)` | new, keeps WS7 code out of the contested file | [NEW] |
| `QE/agent_routes.rs` | `StartRunBody` (345-379): `#[serde(default)] pub policy: Option<RequestAsk>` | edit | [C] |
| `QE/agent_routes.rs` | `start_run` after the ZDR block (1445): resolve; next to `zdr_forbids_persistent_profile` (1470-1476): `require(BrowserProfilePersistent)` when `persist_profile`, `profile_id` or a persistent resume; refuse early a `driver_requirements.uploads_from_artifacts` or `downloads_to_artifacts` that the policy disables; `configure_egress_policy` call (1521-1531): pass `policy.narrow_egress(&constraints.allowed_domains)` mapped to `BrowserEgressPolicy` (`DenyAll` becomes `BrowserEgressPolicy::deny_all()`); recovery navigation (1536): `check_url` as well as `domain_allowed` | edit | [C] |
| `QE/agent_routes.rs` | `step`: call `gate_policy` at the position fixed in 3.5 (after `require_action_capabilities`, before `sensitive_approval_for_action`, 2269-2276); Navigate: `check_url` beside `domain_allowed` (2297-2305); post-action URL: `check_url` beside `domain_allowed` (2400-2406). `execute_ws_action` (3094-3127): same call, same position | edit | [C] |
| `QE/agent_routes.rs` | tab routes that navigate (3202-3451): `check_url` on the target URL | edit | [C] |
| `QE/agent_routes.rs` | `validate_run_grant` (314-338), `RunEntry` (77-107) | **not changed**: equality of `constraints.allowed_domains` with the broker list is preserved; no new `RunEntry` field is needed because every start-time fact the gate reads (`lease.persist_profile`, `profile_scope`, `execution_tier`) is already stored | [C] |
| `QE/web_policy_edge.rs` | `fn denial_response` already exists from S2; `ApiErr` adapter for `agent_routes` (`driver_err` form) | small addition | [NEW] |
| `Q/docs/openapi.yaml` | `policy` on `StartRunBody`; 403 text on `/step`, `/interact`, `/runs` | additive | [M] |

**API / contract changes.** `policy` on `POST /v1/agent/runs`; 403 `FORBIDDEN` with the 3.5 details on `/step`, `/interact`, the WebSocket error frame, `/runs`. The WebSocket frame text is `{"type":"error","message":"Forbidden: policy_denied browser.act.upload"}` (message text only; the typed fields are REST-only until the WS protocol gets a structured error, listed as a follow-up).

**Flags and rollout.** Same two flags as S2. Because the baseline equals today's behaviour, a non-autonomous caller sees no change unless it asks. Roll out with `enforce`; `observe` is available for the first week of any non-default baseline.

**Tests**
- Unit (`agent_policy_gate.rs`, inline): `gate_order_is_policy_before_approval_before_budget`, `read_and_navigate_classes_are_never_denied_by_capabilities`, `human_principal_skips_autonomous_only_keys`, `service_principal_input_denied_when_org_disables_input`.
- Integration `Q/crates/quarry-edge/tests/web_policy_agent.rs` [NEW] (router with `Extension(claims)` and a `RunEntry` literal inserted into `state.agent_runs`, the D2 test style; no JWKS needed): `denied_upload_returns_403_and_leaves_the_approval_grant_unused` (the second attempt with an allowed action succeeds and the same grant id is still usable), `denied_download_same`, `org_allow_narrowing_does_not_break_grant_equality` (two consecutive steps with `require_browser_grants=true` and a stub validator return 200 both times), `empty_intersection_denies_all_instead_of_allowing_all` (asserts the driver received `deny_all`), `persistent_profile_denied_when_policy_disables_it_even_without_zdr`, `resume_refused_when_a_persisted_profile_is_now_disabled`, `ws_and_rest_share_one_gate` (both paths return the same denial for the same action), `evaluate_is_still_raw_script_denied_with_the_new_fields` (D2 contract preserved), `denial_consumes_no_step_and_writes_no_receipt`.
- Commands: `cargo test -p quarry-edge --test web_policy_agent agent_routes`; CI `cargo test --workspace`.

**Definition of done.** All four entry points (REST step, interact, WebSocket, tabs) call the same gate; no new `RunEntry` field; a denied action consumes nothing; D2's `raw_script_denied` string unchanged; OpenAPI updated.

**Deployment evidence required.** Rebuild the edge image; with the dev stack (`require_browser_grants=false`, the Noop validator, as D2 uses) and a user token: `POST /v1/agent/runs {"policy":{"disable":["browser.act.upload"]}}` returns 200; `POST .../step {"action":{"type":"navigate","url":"https://example.com"}}` returns 200 and the page title `Example Domain`; `POST .../step` with an `upload_ref` action returns **403 with `details.capability == "browser.act.upload"` and `decision_source == "request"`** (a stale-target 409 would mean the gate ran in the wrong place: the denial must come before target resolution); `GET .../receipts` lists exactly the navigate receipt; a following `get_content` step returns 200. With a service-principal token (the execution-core path) and `{"policy":{"disable":["browser.act.input"]}}`, a `type_ref` step is 403 while the same action with a user token for the same run owner is 200 (human skip). Prod-mode evidence (`require_browser_grants=true`) waits for F-t2-001; until then state "dev-mode only" in the PR.

**Risks and mitigations.** Conflict with the nine items that edit `agent_routes.rs` (D2 section 5.3): the new module and the D2 pipeline keep the in-file diff to about 12 lines. A resume that was valid before a policy change becomes refused: typed, documented, and only for persistent profiles. The WS error frame stays untyped: noted.

---

### S4. Remaining edge routes and ZDR consolidation (effort M, 3 to 4 engineer-days) [owner-neutral, overlaps in-flight diffs]

**Goal.** The rest of Quarry's capability decisions flow through `ResolvedPolicy`: paid search providers, URL-taking routes, durable-job handoff, persistent profile routes, and, last, the dozen ad-hoc ZDR decisions.

| Path | Change | Tag |
|---|---|---|
| `QE/search_routes.rs` `build_search_options` (live 541-552) | `allow_paid_providers: req.allow_paid_providers && !zdr && resolved.allows(SearchPaidProvider, principal)`. **Downgrade, not reject**: a permission flag the Model Plane sets from the user's plan must not 403 an ordinary search; the response gains `policy_downgrades: ["search.paid_provider"]` (additive field on `SearchResponse`, near the in-flight 178-185 hunk) and the counter `web_policy_downgraded_total` | [M] |
| `QE/extract_routes.rs` (URL list near 205-237) | `check_url` for every URL; first denial rejects the request with the typed 403 and a count, not the URLs | [M] |
| `QE/answer_routes.rs`, `QE/map_routes.rs` (robots check at 256 stays) | `check_url` on the seed; the answer pipeline's candidate fetches are filtered by `PolicySecurityEngine` (S6) | [M]/[C] |
| `QE/routes.rs` `crawl_handoff` (590-680), `batch_handoff` (681-846) | `check_url` on seed and on every batch URL; the existing `refuse_durable_job_under_zdr` (563-588) stays | [M] |
| `QE/profile_routes.rs` (`save_profile` 102, `create_profile` 128) | `require(BrowserProfilePersistent)`; these routes check org id only today (audit section 8), so this is the first policy check they get. F-t2-020 edits the same file: coordinate | [C] |
| ZDR consolidation (last commit, optional, behind its own PR) | replace the `claims.effective_zdr*` calls at `routes.rs:416, 1072`, `extract_routes.rs:237`, `answer_routes.rs:89, 187`, `search_routes.rs:727` by `resolved.zdr()` | [M] |
| `Q/crates/quarry-edge/tests/web_policy_routes.rs` | integration tests, names below | [NEW] |

**Tests.** `paid_provider_disabled_by_org_is_downgraded_and_disclosed`, `paid_provider_still_off_under_zdr_whatever_the_policy_says`, `extract_denied_url_is_403_without_echoing_urls`, `crawl_and_batch_deny_a_denied_seed_before_the_handoff` (the orchestrator double records zero calls), `profile_create_denied_when_policy_disables_persistence`, `zdr_implications_match_the_legacy_refusals` (table: for each of the three legacy refusals, `zdr_forces_disabled` agrees), and the full existing `zdr_claim_floor.rs` suite unchanged. Commands as S2.

**Definition of done.** Every route that takes a URL or a capability flag calls `ResolvedPolicy`; the ZDR consolidation PR changes no test; legacy refusals still present.

**Deployment evidence required.** Rebuild the edge; a user token: `POST /v1/search` with `allow_paid_providers:true` and the baseline JSON `{"caps":{"search.paid_provider":"disabled"}}` returns 200 with `policy_downgrades` containing `search.paid_provider` and the provider telemetry shows no Brave or Serper call; `POST /v1/crawl` with a denied seed returns 403 and the orchestrator log shows no `crawl` handoff. A ZDR org user: unchanged (crawl still refused with the existing text).

**Risks and mitigations.** `search_routes.rs` has a large uncommitted diff (541-552, 696-727, 830+): do S4's search edit after that diff commits, or rebase it by hand; the other edits are independent. Downgrade versus reject is a deliberate asymmetry (permission flag versus requested action) and is written into the OpenAPI text.

---

### S5. Reconcile and retire: `JobPolicy` rename, preset `requires`, `/v1/blocklists` 410 (effort S, 2 engineer-days) [owner-neutral]

**Goal.** No second `RunPolicy`, no preset that silently contradicts governance, no org-less global blocklist.

| Path | Change | Tag |
|---|---|---|
| `QC/policy.rs` | `RunPolicy` becomes `JobPolicy` (struct 6-16, `Default` 118-152); doc comment: "wire shape of CONTRACTS section 7; job intent; not an effective policy"; serde unchanged | [C] |
| `Q/crates/quarry-core/tests/contracts.rs:14, 95` | follow the rename | [C] |
| `Q/services/quarry-control/internal/resources/presets.go` | `Preset.Requires []string \`json:"requires,omitempty"\``; `fast` gets `["robots.max_mode=ignore"]`, `stealth` gets `["proxy.max_tier=pool"]` | [C] |
| `Q/services/quarry-control/internal/resources/presets_test.go` | new `TestEveryPresetThatExceedsTheBaselineDeclaresRequires` (reads the vendored catalogue only if S7 vendored it; before that, a hard-coded list of the two keys) | [C] |
| `Q/services/quarry-control/internal/resources/blocklists.go` and `cmd/control/main.go:172` | `MountBlocklists` replaced by a handler that answers `410 Gone` with the typed envelope and a pointer to the org policy API; the table and `store.BlocklistEntry` stay untouched (no data migration) | [C] |
| `FE/docs/endpoint-map.md:340`, `Q/docs/POLICY.md`, `Q/docs/PRESETS.md`, `Q/docs/openapi.yaml` (blocklist mentions) | update the three-layer story (`JobPolicy`, `ResolvedPolicy`, `RunPolicy` stamp); remove `/v1/blocklists` | [C]/[M] |
| `QE/main.rs` comment 1489-1492 | already removed in S2 | [M] |

**Tests.** `cargo test -p quarry-core contracts` (rename), `go test ./internal/resources/ -run 'Preset|Blocklist'`; a repo grep gate that `quarry_core::policy::RunPolicy` no longer exists. Commands: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./...`.

**Definition of done.** `rg "policy::RunPolicy"` finds only the runtime type; `GET /v1/blocklists` answers 410; `GET /v1/presets` shows `requires`.

**Deployment evidence required.** Rebuild quarry-control; with the HMAC-signed internal call the Verevon gateway already makes (the endpoint-map says control routes are never exposed): `GET /v1/blocklists` returns 410 with `error.code`; `GET /v1/presets` lists `fast` with `requires:["robots.max_mode=ignore"]`; job creation with `preset:"polite"` still returns 201.

**Risks and mitigations.** An unknown external caller of `/v1/blocklists` (none exists in the tree, control is HMAC-internal): 410 with a pointer is reversible by remounting. The rename touches only Rust test code outside `quarry-core`.

---

### S6. Backstops: `PolicySecurityEngine`, final-URL recheck, egress deny list, durable-job snapshot (effort M, 4 to 5 engineer-days) [owner-neutral; soft dependencies F-t2-016 and F-t2-017]

**Goal.** Make the per-URL rule hold below the edge pre-check: inside `PageRunner`, on redirects, in the browser's egress guard, and across the durable crawl boundary, without adding a `PageRunner` field.

| Path | Change | Tag |
|---|---|---|
| `QR/policy_gate.rs` | `PolicySecurityEngine { inner: Arc<dyn SecurityEngine>, resolved: Arc<ResolvedPolicy>, principal }` implementing `preflight`, `check_discovered` (also applies `domains.*` to crawl children when F-t2-016 wires discovery), `allow_private_hosts` (delegates). A block carries the reason prefix `policy_denied:` | [NEW] |
| `QE/routes.rs` (three `PageRunner` literals) and `agent_routes.rs` `validate_navigation` callers | `security: Arc::new(PolicySecurityEngine::new(state.security.clone(), resolved.clone()))` instead of `state.security.clone()` (same field type, so no literal shape change) | [M]/[C] |
| `QR/pipeline.rs` | after the body-size cap and before the `PageFetched` event (about 436-448): if `resp.final_url != *requested_url`, run `self.security.preflight(&resp.final_url)`; on `Block` emit `PageBlocked` (no URL under ZDR via `emit_page_event`) and return the typed error; nothing is persisted or ingested | [C] |
| `Q/crates/quarry-browser/src/lib.rs` | `BrowserEgressPolicy` gains `denied_domains: Vec<String>` (default empty, same suffix rule) and `with_denied_domains()`; `allows_url` checks it first. `BrowserEgressProxyProvider::configure_policy` already takes the whole struct (`lib.rs:388-392`) and the driver hook is `configure_egress_policy` (`lib.rs:445`) | [C] |
| `QR/browser_egress_proxy.rs` | honour `denied_domains` in the proxy and in the Fetch guard | [C] |
| Durable crawl and batch (**depends on F-t2-016 and F-t2-017**) | `crawl_handoff` and `batch_handoff` serialise `ResolvedPolicy` into a compact `policy_snapshot` (`{v, rev, caps, robots_max, proxy_max, domains}`) in the handoff payload (`QE/routes.rs:626-641`, `688-702`); orchestrator `RunPageInput` (`activities.go:165-181`) gains `PolicySnapshot map[string]any`, carried through the workflow memo and sent in the `run_page` body (`activities.go:214`); `InternalRunPage` gains `policy_snapshot: Option<PolicySnapshot>`; the edge meets it with its own baseline so **a snapshot can only tighten**; the HMAC binding (`verify_run_binding`, over org and URL) is extended to org, URL and the snapshot's sha256 so a leaked runtime token cannot swap it | [M]/[C] |
| `Q/crates/quarry-edge/tests/web_policy_backstop.rs` | tests | [NEW] |

**Tests.** `redirect_to_a_denied_host_returns_security_blocked_and_persists_nothing` (a driver double that redirects; zero artifacts, zero ingest, zero `PageFetched`), `final_url_equal_to_requested_is_not_preflighted_twice`, `policy_security_engine_delegates_private_host_rules`, `egress_deny_blocks_a_subresource_on_an_allowed_page` (chromiumoxide feature; `#[ignore]` unless a browser binary exists, like `quarry-browser/tests/ssrf.rs`), `snapshot_cannot_loosen_the_edge_baseline`, `missing_snapshot_on_a_run_page_uses_the_baseline_and_counts_it` (`web_policy_snapshot_missing_total`), `hmac_binding_rejects_a_swapped_snapshot`. Go: `go test ./services/quarry-orchestrator/...` (the activity forwards the field). Commands: `cargo test -p quarry-runtime policy_gate pipeline`; `cargo test -p quarry-edge --test web_policy_backstop`; `cargo test -p quarry-browser --features chromiumoxide` (the CI line 207-211 runs it, ignored tests skipped).

**Definition of done.** No new `PageRunner` field; a redirect to a denied host persists nothing; egress deny lists work inside the browser; (when F-t2-017 lands) a crawl child for a denied host returns the typed error and the job reports it.

**Deployment evidence required.** Rebuild edge and (for the snapshot slice) orchestrator and quarry-control images; a scrape of a URL that redirects (a local `httpbin`-style redirect host) to a baseline-denied host returns 403 `SecurityBlocked` text starting `policy_denied:` and `GET /v1/artifacts` for the org shows no new artifact; with a browser run, a page on an allowed host that embeds an `<img>` from a denied host shows the request blocked in `GET .../devtools-egress` receipts (`agent_routes.rs:3453-3559`).

**Risks and mitigations.** Final-URL preflight also applies URLhaus and PhishTank to redirect targets, which is a tightening beyond policy: verify with a corpus of known-good redirects (http to https, www redirects) before enabling it by default; ship with `QUARRY_EDGE__WEB_POLICY_FINAL_URL_CHECK=1` default on in dev and staged in prod. Sub-resource denial changes a proxy type used by other code: unit-test the default (empty list) is byte-identical. The snapshot slice is the only one blocked on other items; S6a (decorator, final URL, egress deny) can ship alone.

---

### Owner-dependent slices (S7 to S12): blocked by audit ADR topic 1

These slices are written for the recommended outcome **C** (org-core is the policy of record; claim on user tokens and org-resolved service tokens; grant carries a snapshot). The table at the end of this part says exactly what changes under outcome **A** (user claims only) and outcome **B** (capability-core owns). Everything below consumes S1 only through `PolicyLayer` JSON, so the owner-neutral code does not change under any outcome.

**S0 (0.5 engineer-day, documents only).** When the ruling is made: ledger entry in `MP/docs/decisions/ledger.md` [C] (decision, options, evidence of section 3.1, the 345 s revocation bound, the claim name `web_policy`), a row in `docs/CROSS_PLANE_CONTRACT_MATRIX.md` [C] for the Control-to-Ingestion claim edge, a "Web policy posture" paragraph in `CP/CONTROL_PLANE_OWNERSHIP.md` beside line 355 [C], and a rule-16-style sentence in `apps/master-ownership-matrix.md` [C] stating that Auth Core is the only issuer of org web policy and that Quarry, broker and gateway only enforce.

---

### S7. org-core: storage, admin API, internal read (Go) (effort L, 6 to 7 engineer-days) [outcome A or C]

**Goal.** org-core stores an org's web policy as a versioned, audited document, lets an org admin edit it within plan entitlements, and serves the effective policy to Auth Core on a narrow internal route.

**File-level change list** (`CP/org-core`, Go module `github.com/I-Dacosta/AquatiqCMS/apps/org-core`)

| Path | Change | Tag |
|---|---|---|
| `migrations/021_organization_web_policy.up.sql` and `.down.sql` | `organization_web_policy(org_id TEXT PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE, policy JSONB NOT NULL, revision BIGINT NOT NULL CHECK (revision > 0), rev_digest TEXT NOT NULL, updated_by TEXT NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())`; RLS `USING/WITH CHECK (org_id = current_setting('app.current_org', true))` exactly as `019_interactive_retention_outbox.up.sql`; `GRANT SELECT, INSERT, UPDATE ... TO org_core_app`. `ON DELETE CASCADE` is the erasure path (hard delete in `003_gdpr_hard_delete.up.sql` removes the org row; migration 018 uses the same FK pattern). Statements re-runnable (`IF NOT EXISTS`) | [NEW] |
| `migrations/022_organization_web_policy_history.up.sql` and `.down.sql` | append-only audit: `(org_id, revision, policy JSONB, changed_by, changed_at, tightened BOOLEAN, PRIMARY KEY (org_id, revision))`; RLS as above; `GRANT SELECT, INSERT` only (no UPDATE or DELETE grant, so the trail cannot be edited by the app role) | [NEW] |
| `internal/org/web_policy.go` | `WebPolicyDoc` (the claim schema without `rev`), `Validate(catalogue)` (key known, value legal for the key's kind, no `forced` on an ability, caps of 64 domain entries and 2 KiB, no IP literal or wildcard, host canonical), `Effective(plan, entitlements) WebPolicyDoc` (admin settings AND entitlements: 3.12), `Digest()` (sha256 of canonical JSON, first 12 hex), `Tightened(old, new) bool` (new is the meet of old and new and differs), typed errors `ErrRevisionConflict`, `ErrIllegalMode`, `ErrUnknownCapability`, `ErrNotEntitled` | [NEW] |
| `internal/org/web_policy_repository.go` | `GetWebPolicy(ctx, orgID)`, `PutWebPolicy(ctx, orgID, doc, ifMatch int64, changedBy)` in one `WithOrgScope` transaction: `SELECT ... FOR UPDATE`, compare revision (`If-Match` of 0 creates), insert history, upsert; mirrors `SetInteractiveRetention` (`repository.go:368-410`) | [NEW] |
| `internal/org/types.go` | computed (never stored) entitlement keys `web.robots_override`, `web.proxy_pool`, `web.paid_search` and `PlanAllowsWebCapability(plan, key)` beside `zeroDataRetentionPlans` (281-284) | [C] |
| `internal/org/service_enhanced.go` | `GetWebPolicyPosture`, `SetWebPolicy` (service layer, pattern of `GetRetentionPosture` at 587 and `SetInteractiveRetention` at 606) | [C] |
| `internal/org/testdata/web-policy-catalogue.v1.json` + `.sha256` | vendored byte-identical copy of the S1 catalogue, embedded with `go:embed`; a test fails if it differs from its pin | [NEW] |
| `internal/http/web_policy_handler.go` | `getWebPolicy`, `putWebPolicy`, `listWebPolicyHistory`, `getWebPolicyInternal` (shapes below). Admin routes sit behind `requireActiveMembership` and **additionally check owner or admin role in the handler** (the pattern `authorizeOrgErasure` uses; settings today rely on the gateway's `require_org_admin` only, `handlers.go:341-343`) | [NEW] |
| `internal/http/server.go` | mount `GET/PUT /orgs/:id/web-policy`, `GET /orgs/:id/web-policy/history` and the `/api/v1/organizations/:id/...` twins beside `PATCH /orgs/:id/capabilities` (line 130); internal `GET /internal/orgs/:orgId/web-policy` beside the retention route (line 168) | [C] |
| `internal/http/service_auth.go` | scope `org:web-policy:read:any` added to `orgServiceScopeAllowlist` (line 50 block), a case in `internalOrganizationScopes` beside line 360, and the admin routes mapped to `org:settings:write:self` / `org:read:self` in `organizationProxyScopes` and `organizationAPIScopes`. The `authRequired` boot check (line 202) is extended **in S8**, after the compose credentials carry the scope | [C] |
| `internal/http/testdata/auth-core-org-core-web-policy.contract.json` + `.sha256` | vendored copy of the golden request and response Auth Core sends and expects | [NEW] |
| `CP/docker-compose.yml` (line 540, the `auth-core` credential's `scopes`), `CP/scripts/run-isolated-control-container-e2e.sh:188`, `CP/config/README.md`, `CP/config/contracts/README.md` | add `org:web-policy:read:any` to the auth-core principal; document the contract (the README states the contract lives with the producer, Auth Core) | [C] |
| `internal/http/web_policy_contract_test.go`, `web_policy_handler_test.go`, `internal/org/web_policy_test.go`, `web_policy_postgres_test.go` | tests (below) | [NEW] |

**API (all under typed envelopes; org-core's older routes use `{"error": "..."}` strings, the new routes use the CLAUDE.md shape)**
- `GET /orgs/:id/web-policy` returns `{"data":{"policy":{"v":1,"caps":{"browser.act.upload":"disabled"},"robots_max":"respect","proxy_max":"none","domains":{"allow":[],"deny":["bad.example"]}},"effective":{...},"revision":3,"rev":"7f3a9c1e2b4d","entitlements":{"web.robots_override":false,"web.proxy_pool":false,"web.paid_search":true},"updated_by":"...","updated_at":"..."}}`; absent policy returns revision 0 and the baseline-equivalent empty document.
- `PUT /orgs/:id/web-policy` with `If-Match: <revision>`; 200 `{data:{revision, rev}}`; 409 `{"error":{"code":"revision_conflict","message":"...","details":{"current":4}}}`; 422 `illegal_mode`, `unknown_capability`, `invalid_domain`, `too_large`; 402 `not_entitled` with `details.capability`; 403 for non-admin members.
- `GET /orgs/:id/web-policy/history?limit=50` returns `{data:[{revision, changed_by, changed_at, tightened, policy}], meta:{cursor}}`.
- Internal `GET /internal/orgs/:orgId/web-policy` (scope `org:web-policy:read:any`, `Cache-Control: no-store`, 3 s timeout like `retentionPostureTimeout`) returns `{"data":{"v":1,"rev":"7f3a9c1e2b4d","caps":{...},"robots_max":"...","proxy_max":"...","domains":{...}}}` already folded with entitlements, so Auth Core never sees them; 404 for an unknown org; an org with no row returns the empty document with `rev:"0"`.
- Audit event on every PUT through the publisher org-core already uses for lifecycle audit [UNVERIFIED which one: reuse the `gdpr_audit_outbox.go` pattern]; the details carry capability keys and `tightened`, never domain names for a ZDR org.

**Flags and rollout.** No flag; the routes are additive and nothing reads them until S8. Migration 021 and 022 apply on org-core start through the existing migration runner. Rollback: the `.down.sql` files; the routes can be left mounted.

**Tests** (`cd "CP/org-core" && REQUIRE_CONTRACT=1 TEST_DATABASE_URL=... CONTROL_LIFECYCLE_TEST_DATABASE_URL=... go test -p 1 ./...`, the CI shape of `org-service.yml:95-103`)
- Pure: `TestValidateRejectsForcedOnAbility`, `TestValidateRejectsUnknownKey`, `TestValidateRejectsIPLiteralAndWildcard`, `TestEffectiveFoldsEntitlements` (plan lapse tightens), `TestDigestIsStableUnderKeyOrder`, `TestTightenedDetectsOnlyTightening`.
- Postgres: `TestPutCreatesWithIfMatchZero`, `TestStalePutIs409`, `TestCrossOrgReadIsNotFound` (RLS, a read under another org scope sees nothing), `TestHistoryIsAppendOnly` (an UPDATE and a DELETE as `org_core_app` fail), `TestHardDeleteRemovesPolicyAndHistory`, `TestConcurrentPutsExactlyOneWins`.
- HTTP: `TestAdminMatrix` (owner and admin 200; member and viewer 403; unauthenticated 401), `TestPutRejectsOverlargeDocument`, `TestInternalRouteRequiresItsOwnScope` (an `org:read:any` credential gets 403), `TestRetentionPostureStillOnlyTwoBooleans` (the existing test must stay green: the new route must not widen the retention response).
- Contract: `TestWebPolicyContractCopyMatchesPin` and a replay of the golden request through the real middleware, as `retention_posture_contract_test.go` does.

**Definition of done.** Both migrations apply on a fresh and on a populated database; RLS proven; history append-only; internal route reachable by Auth Core's credential only; catalogue copy pinned; the sync script has the new GOLDENS and VENDORED rows.

**Deployment evidence required.** `cd "apps/Control Plane" && bash scripts/run-control-plane.sh build org-core` then `bash scripts/run-control-plane.sh up -d --no-deps --build org-core` (a bare `docker compose build` exits 0 and builds nothing: memory note `control-plane-build-and-deploy`). Against the running stack: (1) as an org admin through the gateway principal, `PUT /orgs/<org>/web-policy` with `If-Match: 0` and `caps.browser.act.upload=disabled` returns 200 and `revision 1`; (2) `GET` returns the same `rev`; (3) a second `PUT` with `If-Match: 0` returns 409 `revision_conflict`; (4) a member token gets 403; (5) `psql` on `org_core` shows one history row and `UPDATE organization_web_policy_history ...` as `org_core_app` is refused; (6) the auth-core credential calls the internal route and gets the document with the same `rev`, while the billing-core credential gets 403.

**Risks and mitigations.** Admin authority: a permissive default would let a member change the org's policy: the handler's own role check plus the matrix test. Entitlement product decision (Q3) can change the three keys without schema change (they are computed). Migration numbering 021 to 022 collides with any other Control change landing first: re-check `ls migrations` the day of the PR.

---

### S8. auth-core: claim minting, registry field, goldens in three trees (TypeScript) (effort L, 7 to 8 engineer-days) [outcome A drops the registry part; outcome C full]

**Goal.** Auth Core resolves the org's effective web policy from org-core and stamps a `web_policy` claim onto `quarry` and `browser-broker` plane tokens, for interactive users always and for service principals whose registry row says `webPolicyByAudience[audience] = "tenant"`.

**Claim shape (final).** On `aud` in {`quarry`, `browser-broker`}: `"web_policy": {"v":1,"rev":"<12 hex or \"0\">","caps":{"<key>":"disabled"},"robots_max":"respect","proxy_max":"none","domains":{"allow":[...],"deny":[...]}}`. Only non-baseline entries are included; the claim is **always present** on those audiences (`{"v":1,"rev":"0"}` when the org has no policy), so an absent claim identifies an old issuer and Quarry's `QUARRY_EDGE__WEB_POLICY_REQUIRE_CLAIM=1` can refuse it. Size cap 2 KiB serialised; over the cap the mint fails closed (`web_policy_too_large`), which the org-core caps make unreachable. `zdr` is untouched and stays the only ZDR carrier (3.7).

**File-level change list** (`CP/auth-core/src/auth`, `CP/config`)

| Path | Change | Tag |
|---|---|---|
| `web-policy.ts` | `resolveWebPolicy(orgId)`: calls org-core `GET /internal/orgs/:orgId/web-policy` with `x-service-id: auth-core` and the scoped service token (same `requiredScopedServiceToken` helper as `interactive-retention-policy.ts`), 45 s fresh cache, stale-if-error 15 min (a stale tighter policy is safe and a stale looser one is bounded by the grant TTL), no failure caching, `WebPolicyUnavailableError` (503 `web_policy_unavailable`) with **no token issued** when nothing usable is remembered; validates the shape against the vendored catalogue | [NEW] |
| `convex-token.service.ts` | `PlaneJwtClaims.webPolicy?`; in `issuePlaneToken` (648-730) add `...(claims.webPolicy ? { web_policy: claims.webPolicy } : {})` beside `zdr` (line 714); optional TTL cap `WEB_POLICY_TOKEN_TTL_CAP_SECONDS` (default none, so the audience TTL of 300 s stands; 3.1) | [C] |
| `plane-token.controller.ts` | `getToken` (interactive, after the posture at about 256): resolve for the two audiences; `issueInternalToken` (after the posture at about 343-349): resolve when `principal.webPolicy === 'tenant'`; the audit event `plane_service_token_issued` details gain `web_policy_rev` | [C] |
| `plane-service-principal.ts` | registry field `webPolicyByAudience?: Record<string, 'tenant'>` validated like `retentionByAudience` (311-332) and surfaced as `AuthorizedPlaneServicePrincipal.webPolicy` | [C] |
| `CP/config/plane-service-principals.json` | `execution-core` and `model-gateway`: `"webPolicyByAudience": {"quarry": "tenant"}`. **No other principal and no other audience**; D1 also edits this file (a `browser-broker` audience for `quarry-edge`): merge by hand | [C] |
| `plane-service-principals-policy.contract.spec.ts` | assertions: only those two principals hold `tenant`; only for `quarry`; interactive tokens for `browser-broker` and `quarry` carry the claim; `zdr` for service tokens still equals the registry row | [C] |
| `fixtures/web-policy-catalogue.v1.json` + `.sha256` | vendored catalogue | [NEW] |
| `fixtures/quarry-token-claims.golden.json` + `quarry-token-claims.golden.sha256` | regenerated with new cases: `user_in_org_with_web_policy`, `user_in_org_without_policy` (`rev:"0"`), `service_execution_core_with_web_policy`, `service_model_gateway_with_web_policy`; existing cases gain the `{"v":1,"rev":"0"}` claim | [C] |
| `quarry-token-zdr-claims.golden.spec.ts` | extended cases; regenerate with `UPDATE_GOLDENS=1 npx jest src/auth/quarry-token-zdr-claims.golden.spec.ts` (writes the canonical and the Quarry-vendored copy) | [C] |
| `fixtures/auth-core-org-core-web-policy.contract.json` + `.sha256` and `web-policy.org-core-contract.spec.ts` | canonical contract and its replay test (pattern of `interactive-retention-policy.org-core-contract.spec.ts`); regenerate with `UPDATE_CONTRACT_GOLDEN=1 npx jest web-policy.org-core-contract` | [NEW] |
| `web-policy-unavailable.controller.spec.ts`, `web-policy.spec.ts` | resolver cache and failure semantics, 503 behaviour (pattern of `interactive-retention-unavailable.controller.spec.ts`) | [NEW] |
| **Vendored copies of the changed golden** | `Q/crates/quarry-edge/tests/fixtures/auth-core/quarry-token-claims.golden.{json,sha256}` [C]; `MP/rust/services/model-gateway/tests/fixtures/auth-core/quarry-token-claims.golden.{json,sha256}` **[U]** (not in git: it must be committed with the rest of the in-flight fixture set, S9 H1) | [C]/[U] |
| `scripts/tests/zdr-contract-sync.test.sh` | update the pinned digest of `quarry-token-claims.golden.json`, add rows for the two new contract files and the catalogue copies | [M] |
| `scripts/tests/principal-registry-parity.test.sh` | update if it enumerates registry fields [UNVERIFIED: read it before the PR] | [C] |
| `CP/org-core/internal/http/service_auth.go` | now extend `authRequired` (line 202) so org-core refuses to boot without the scope on the auth-core principal | [C] |

**Flags and rollout.** `WEB_POLICY_CLAIM_ENABLED` (auth-core env, default `1` once S7 is deployed) lets Control disable stamping without a registry rollback: when `0` the claim is `{"v":1,"rev":"0"}` for everyone. Deploy order: org-core S7, auth-core with the claim, then Quarry S9 in `observe`, then `enforce`. Rollback: flag off, or revert.

**Tests** (`cd "CP/auth-core" && npx jest quarry-token-zdr-claims web-policy plane-service-principals-policy interactive-retention`; full CI shape `REQUIRE_CONTRACT=1 pnpm test`; compare failing sets with HEAD because 15 tests in three suites already fail (`deployment-contract`, `plane-service-principal`, `model-plane-token.controller`); `pnpm lint` dies with ENOBUFS on the host, pre-existing)
`user_token_carries_the_claim_for_quarry_and_browser_broker_only`, `service_token_carries_the_claim_only_for_tenant_rows`, `claim_is_always_present_on_those_audiences`, `zdr_claim_is_byte_identical_before_and_after` (golden diff limited to the added field), `org_core_down_with_nothing_remembered_is_503_and_no_token`, `stale_policy_served_for_15_minutes_then_503`, `failure_is_never_cached`, `claim_over_2KiB_fails_closed`, `registry_refuses_tenant_for_any_other_audience`, `service_token_ttl_unchanged_without_the_cap`. Drift gates: `bash scripts/tests/zdr-contract-sync.test.sh` and `--self-test`; `cargo test -p quarry-edge --test zdr_claim_floor` (re-pins the vendored golden); `cd "MP/rust" && cargo test -p model-gateway --lib auth` (its vendored-golden tests).

**Definition of done.** All three golden copies and the sync script agree; the only diff in the `zdr` claim is none; both service principals mint the claim; the 503 path proven.

**Deployment evidence required.** Rebuild auth-core and org-core (`bash scripts/run-control-plane.sh ...`). With the S7 policy stored: (1) fetch a user token through the gateway path (`GET /api/quarry/token` with a session cookie) and a service token (`POST /api/quarry/internal-token` with the execution-core credential, `orgId`, scopes `["browser:execute"]`); decode both payloads (no verification needed) and assert `web_policy.rev` equals the S7 `rev`, `zdr` equals what it was before, and the service token's `exp - iat` is unchanged; (2) `docker stop` org-core and mint again for an org not remembered within 15 minutes: HTTP 503 `web_policy_unavailable`, no token; for a remembered org the stale policy is served and a warning is logged.

**Risks and mitigations.** The 503 couples Quarry token availability to org-core for orgs outside the stale window (the same trade-off the module comment of `interactive-retention-policy.ts` documents for ZDR); the 15-minute stale window mirrors the non-ZDR rule. Registry schema change: parity script and contract spec; ask for Control sign-off (Q2). Three-copy golden drift: the sync script is the guard. The claim adds up to 2 KiB to every quarry and broker bearer (header budget 8 KiB): measured in the PR.

---

### S9. Quarry: claim intake and the `observe` to `enforce` switch (effort M, 3 engineer-days) [outcome A or C]

**Goal.** `Claims` carries `web_policy`; the resolver reads it for every principal type; a missing claim can be made an error in production; an unreadable claim fails closed.

| Path | Change | Tag |
|---|---|---|
| `QE/auth.rs` | `Claims.web_policy: Option<serde_json::Value>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`, deliberately untyped so a malformed claim never fails authentication); `Claims::web_policy_layer() -> Result<Option<PolicyLayer>, PolicyDenial>` (absent `Ok(None)`, unparseable or `v > 1` `Err(policy_unreadable)`). **Not routed through `zdr_floor`**: `is_service` is irrelevant for this claim. Literals to extend with `web_policy: None`: `auth.rs:397` (dev-bypass stub) and the test literals at `auth.rs` lines 576, 601, 622, 662, 693, 749, 791, 983, 1034, 1073; `agent_routes.rs:2030`; `graphql/schema.rs:159`; `routes.rs:1355`; `test_support.rs:153` | [C]/[M] |
| `QE/policy_baseline.rs` | `resolve` feeds the claim layer; `QUARRY_EDGE__WEB_POLICY_REQUIRE_CLAIM` (default off; on in the production overlay once S8 is live) refuses a user or service token without the field with 403 `policy_unreadable` | [NEW] (from S2) |
| `Q/crates/quarry-edge/tests/fixtures/auth-core/` | the regenerated golden from S8 | [C] |
| `Q/crates/quarry-edge/tests/web_policy_claim.rs` | integration tests on the real-token harness | [NEW] |
| `Q/docs/SELF_HOST.md` [M], `Q/docs/RUNBOOKS.md` [M] | document the three env vars (both files have uncommitted diffs: append, do not reflow) | [M] |

**Tests** (`cargo test -p quarry-edge --test web_policy_claim --test zdr_claim_floor`): `user_claim_disables_upload_even_when_the_body_asks`, `service_claim_applies_to_execution_core_tokens` (the golden's service cases), `service_zdr_claim_is_still_ignored` (the old rule stays: `service_execution_core` token with `zdr:true` does not force ZDR), `absent_claim_is_baseline_unless_required`, `required_claim_missing_is_403`, `unparseable_claim_fails_closed`, `unknown_version_fails_closed`, `unknown_key_is_ignored_and_counted`, `ask_cannot_loosen_the_claim`, `claim_rev_is_in_the_denial_details`.

**Definition of done.** Both principal types enforce the claim; `zdr_claim_floor.rs` green and unchanged; the two required-claim modes tested.

**Deployment evidence required.** Rebuild the edge; with S7 and S8 deployed and `caps.browser.act.upload=disabled` stored for the org: a **user** token `POST /v1/agent/runs` then an `upload_ref` step returns 403 with `decision_source == "org_policy"` and `policy_rev` equal to the S7 `rev`; an **execution-core service** token (minted by `POST /api/quarry/internal-token`, `reason` set) for the same org does the same; a token of another org is unaffected; then `PUT` the policy back to `allowed`: a **new** run permits the step within the 345 s bound (record the measured delay), and an already-running run keeps the tighter snapshot only if its grant did (3.13).

**Risks and mitigations.** Twelve `Claims` literals to touch (mechanical; listed above so the PR is reviewable). In production the require-claim flag must follow, not precede, the auth-core rollout or every request 403s: the runbook orders them.

---

### S10. Broker: web-policy snapshot in grants, `PolicyResolver` implementation, Quarry validator field (effort L, 6 to 7 engineer-days) [outcome A (mandatory for Model runs) or C (snapshot)]

**Goal.** A grant carries the org's effective policy as of issue time, computed by the broker from the verified bearer's claim and never accepted from a caller; Quarry's `GrantValidation` exposes it so it is re-read on every step; the planner can see what is allowed.

**Dependency.** WS1/D1 (F-t2-001): this slice extends D1's `grant/policy.go` `PolicyResolver` hook, its Postgres schema and its proto allocation. It cannot be tested in production mode before D1 slice 1a and 1c; against the in-memory store it can ship earlier.

| Path | Change | Tag |
|---|---|---|
| `MP/proto/model_plane/v1/browser.proto` | `message WebPolicy {uint32 v = 1; string rev = 2; map<string,string> caps = 3; string robots_max = 4; string proxy_max = 5; repeated string domains_allow = 6; repeated string domains_deny = 7;}`; `WebPolicy web_policy = 20;` on `AcquireGrantResponse`, `ValidateGrantResponse` and D1's `DeriveRunGrantResponse`. **No field on any request**: the broker derives the snapshot from the verified principal. Field number 20 is the first of the 20-39 range D1 reserved for F-t2-032 (D1 D11); WS7 takes only 20 | [C] |
| `MP/go/gen/...` | regenerate with `buf generate` through the `bufbuild/buf` image | [C] |
| `MP/go/pkg/authctx/authctx.go` | `Principal.WebPolicy json.RawMessage` and the claim field in the JWT `claims` struct (next to `ZDR` at line 63, parsed at 155-157); same package serves capability-core and cost-core, so it must stay optional | [C] |
| `MP/go/services/browser-broker/internal/grant/policy.go` | D1 creates the file with `PolicyResolver.Resolve(org, owner, requested) -> allowed` and a static deny-list default; WS7 adds `ClaimPolicyResolver` (narrows requested domains by `domains.allow`, removes `domains.deny`, returns `ErrDomainDenied` as `browser_host_not_grantable`) and `Snapshot(principal) *WebPolicy` | [NEW] (D1) |
| `MP/go/services/browser-broker/internal/grant/grant.go` | `Grant.WebPolicy *WebPolicy`; derived `run` grants copy the parent's snapshot and may only tighten it (meet with the principal's current claim) | [C] |
| `MP/go/services/browser-broker/internal/server/server.go` | `AcquireGrant` (lines 42-108) and D1's derive RPC fill the snapshot; `ValidateGrant` returns it | [C] |
| `MP/go/services/browser-broker/migrations/0002_browser_grants_web_policy.up.sql` and `.down.sql` | `ALTER TABLE browser_grants ADD COLUMN IF NOT EXISTS web_policy JSONB;` (ZDR grants are never persisted by D1 D6, so nothing about ZDR changes). If D1 slice 1c has not merged, fold the column into D1's `0001` instead | [NEW] |
| `Q/crates/quarry-runtime/proto/browser.proto` | vendored copy: add `WebPolicy` and field 20 | [C] |
| `QR/grant_validator.rs` | `GrantValidation.web_policy: Option<serde_json::Value>` (`#[serde(default)]`; `NoopGrantValidator` returns `None`) | [C] |
| `QR/grpc/browser_broker.rs` | map proto field 20 into `GrantValidation.web_policy` | [C] |
| `QE/agent_policy_gate.rs` | the S3 gate already takes `grant: Option<&GrantValidation>`; now reads its snapshot as the grant layer (3.3) | [NEW] (S3) |
| `MP/rust/services/model-gateway/src/browser_run.rs` | no logic change: `browser_run_start` validates the grant before any thread or run write (D1); the snapshot rides in the grant. The `POST /v1/browser/grants` response (D1) adds `policy_rev` for the UI | [M] |

**Tests.** Go (`cd "MP/go" && go test ./services/browser-broker/...`): `TestSnapshotComesFromVerifiedPrincipalNotRequest` (a request cannot carry one: no field exists; a forged `web_policy` in gRPC metadata is ignored), `TestRunGrantSnapshotOnlyTightens`, `TestDeniedDomainIsNotGrantable`, `TestAllowListIntersectsRequestedDomains`, `TestPolicyAbsentYieldsNoSnapshot`, `TestPostgresRoundTripKeepsSnapshot` (integration tag). Proto: the L1 contract assertion of D1 (field numbers 1-12 and now 20 on three messages, no reuse). Rust: `cargo test -p quarry-runtime grpc::browser_broker grant_validator`, `cargo test -p quarry-edge --test web_policy_agent` extended with `grant_snapshot_is_met_with_the_claim_per_step`, `looser_claim_does_not_loosen_a_tighter_grant_snapshot`, `tighter_claim_tightens_a_running_run_on_the_next_step`.

**Flags and rollout.** No new flag: absent snapshot means no grant layer. Deploy broker first (it only adds a field), then Quarry reads it.

**Definition of done.** Snapshot present in `ValidateGrant` for a run created after S8; Quarry meets it per step; the proto allocation is recorded in the D1 ledger entry.

**Deployment evidence required.** Rebuild browser-broker (`bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build browser-broker`), model-gateway and the edge. With D1 deployed: open a gateway session for an org whose policy denies upload (S7), read the grant through `grpcurl ... ValidateGrant` (or the Quarry log line of the validated grant, without domains) and assert `web_policy.rev` equals the org's `rev`; restart the broker and, for a non-ZDR grant, assert the snapshot survives (`psql` shows the column); a ZDR grant keeps working in memory only.

**Risks and mitigations.** D1 and WS7 both edit `grant.go`, `server.go`, `browser.proto` and the registry JSON: merge D1 first and rebase. Under outcome A this slice carries all Model-run enforcement, so F-t2-001's production issuance becomes a hard dependency. The broker never trusts a caller-supplied policy: asserted by the test above.

---

### S11. Model-driven runs: request layer and denial rendering (sibling b owns the code) (effort S to M, 3 engineer-days) [any outcome]

**Goal.** Planner-visible restrictions and a non-retryable rendering of org denials, so a Model run neither proposes forbidden actions nor retries around a denial. Contract owner: this workstream; code owner: sibling (b).

| Path | Change | Tag |
|---|---|---|
| `MP/rust/services/execution-core/src/quarry_agent.rs` | `StartRunRequest` (struct near 182-192) gains `#[serde(skip_serializing_if = "Option::is_none")] pub policy: Option<WireRequestAsk>` carrying only `disable` keys and domain subsets; filled from run options (plan mode disables input, interact, upload, download; a Space restriction enters here and only here) | [C] |
| same | where a step response is a 403 with `error.details.reason == "policy_denied"`: produce `ObservationStatus::Blocked` with the fixed text of 3.5 and `retryable=false`; `observation_from_wire` (about 408-452) already maps `policy_denials` to Blocked | [C] |
| `MP/rust/services/execution-core/src/browser_agent/browser_risk.rs` and the planner prompt (`llm_planner`) | add the allowed-capabilities list from the grant snapshot so the planner stops proposing denied classes (documentation of the snapshot, not enforcement) | [C] |
| approvals (F-t2-033) | a `policy_denied` is never turned into an approval request; only a domain miss against the user's own grant is approvable (E-032) | design rule |

**Tests** (`cd "MP/rust" && cargo test -p execution-core --lib quarry_agent`): `policy_denied_maps_to_blocked_with_the_fixed_text`, `blocked_denial_is_not_offered_to_approvals`, `plan_mode_sends_disable_for_input_interact_upload_download`, `space_restriction_enters_only_through_the_ask`, `ask_serialisation_matches_quarry_request_ask_schema` (a golden shared with the Rust `RequestAsk` test).

**Deployment evidence required.** Rebuild execution-core and model-gateway; run a browser task that tries an upload under an org policy that disables it: the run's observation shows Blocked with the fixed text and `GET .../receipts` on Quarry shows no upload receipt; the same task in plan mode never reaches Quarry with an input action (the edge counter `web_policy_denied_total{decision_source="request"}` stays 0 because the planner did not propose it).

**Risks and mitigations.** Sibling (b) is changing the same files (agent browser path, delegation): the contract is the JSON of `RequestAsk` and the denial `details`; neither side needs the other's branch.

---

### S12. Revocation push (optional under C, mandatory under A without service claims or under B) (effort M, 4 engineer-days)

**Goal.** Shorten the tightening delay from the claim staleness (about 345 s) or the grant TTL (15 min) to seconds, for orgs that need it, without ending sessions for loosenings.

| Path | Change | Tag |
|---|---|---|
| `CP/org-core/migrations/023_organization_web_policy_outbox.up.sql` and `.down.sql` | outbox `(event_id BIGINT GENERATED ALWAYS AS IDENTITY, org_id, revision, attempts, processing_at, published_at, last_error, created_at, updated_at)` with RLS and grants exactly as migration 019 | [NEW] |
| `CP/org-core/internal/org/web_policy_repository.go` | `PutWebPolicy` inserts an outbox row only when `Tightened(old, new)` | [NEW] (S7) |
| `CP/org-core/internal/nats/shared_publisher.go` | `PublishWebPolicyTightened(ctx, orgID, revision, eventID)` beside `PublishInteractiveRetentionEnabled` (179-191); subject name to be fixed with the other planes [UNVERIFIED naming scheme]; payload `{org_id, revision, event_id}` only (no policy content on the bus) | [C] |
| `CP/org-core/internal/org/service_enhanced.go` | `FlushWebPolicyOutbox` mirroring `FlushInteractiveRetentionOutbox` (638-660) | [C] |
| browser-broker | a durable consumer (new dependency: the broker has no NATS client today) that, on an event, calls an internal repository method `RevokeOrgGrants(org, reason="policy_tightened", exceptKindsNewerThan=revision)`; sessions re-issue through the gateway keep-alive of D1/WS8. Whether the broker or model-gateway hosts the consumer is a sibling (b) decision | [C]/[NEW] |

**Tests.** org-core: `TestOutboxOnlyOnTightening`, `TestLooseningWritesNoOutbox`, `TestFlushPublishesOnceAndMarksPublished` (Postgres). Broker: `TestRevokeOrgGrantsRevokesRunAndSessionKinds`, `TestRevokeIsIdempotent`. **Evidence:** tighten the policy while a run is mid-flight and measure the next step's denial latency (target under 10 s) against the baseline (up to 345 s).

**Risks.** Revoking a human's session for a minor tightening is a worse outcome than a delay: the default is off (`WEB_POLICY_PUSH_REVOKE=0`). The broker gaining a NATS dependency is new surface: hardening review first.

---

### How the outcomes change the slices

| Slice | Outcome C (recommended) | Outcome A (user claims only) | Outcome B (capability-core owns) |
|---|---|---|---|
| S1-S6 | unchanged | unchanged | unchanged |
| S7 org-core storage and admin API | as written | as written | **replaced** by capability-core: migration `MP/go/services/capability-core/migrations/0016_org_web_policy.up.sql` (next free after `0015`), a parameter store beside `registry/scope_store.go`, admin routes under `internal/api/` with the existing audit log; no entitlement fold-in (Model Plane does not own plans) |
| S8 auth-core | claim on user and tenant service tokens, registry field | claim on user tokens only; **no registry change**; execution-core and model-gateway tokens do not carry it | none (no claim) |
| S9 Quarry intake | reads claim for any principal | reads claim for user principals; service principals get policy from the grant | **user and SDK callers are unpinned** (no claim, no grant on acquisition routes): the baseline is the only org-independent floor; a gateway-set `RequestAsk` can restrict but cannot be proven to come from the org |
| S10 broker | snapshot (hint plus per-step meet) | **mandatory carrier** for Model runs; F-t2-001 hard dependency | sole carrier; broker calls capability-core at issue time (new Model-internal client) |
| S11 | same | same | same |
| S12 push revoke | optional | mandatory for service paths | mandatory (grant TTL is the only bound) |
| Revocation bound | 345 s (105 s with 60 s cap) | users 45 s; Model runs 15 min | 15 min |
| Plane-ownership statement | Control owns, Ingestion enforces, Model Plane carries a snapshot | same, but Model Plane carries the only copy for its runs | Model Plane owns org entitlements (conflicts with CLAUDE.md "Control owns entitlements") |

---

### Epic E-032 (F-t2-032): per-origin consent, action-class grants, settings hub. Outline only, size XL, wave W3

Not designed at file level here. It is three deliverables on top of what WS7 builds, each blocked by F-t2-001 and by the policy ruling, and by the owner decision OD13 (start host plus at most 8 user-listed domains, broker as authority) before the pattern language is fixed.

1. **E1 Typed denials with `decision_source` (S to M).** Largely **done by S1 to S3**: the vocabulary, `PolicyDenial`, the action-class mapping and the 3.5 details exist. What remains: the optional `retryable` and `decision_source` fields on `QuarryError` itself (so non-policy denials, such as `SecurityBlocked`, speak the same dialect; touches `QC/error.rs`, which D2 also edits) and the anti-circumvention line in execution-core tool text (S11). Fix `status_err`'s `BAD_REQUEST` mislabel (D2 S6).
2. **E2 Per-origin consent (L).** A new parent-bound **origin-extension** grant kind in the broker (the existing child grants authorise exactly one of three actions, and `validate_run_grant` demands run domains equal the parent at every step, `QE/agent_routes.rs:314-338`, so an approved origin cannot be added to a live run today); Quarry validates it and widens the egress policy through `configure_egress_policy`; scopes turn, conversation and always, any deny beats any approval (= the meet in 3.3, with consent as a request-layer input that can only add inside `domains.allow` and outside `domains.deny`); decisions persisted in Control Plane user settings (user-core `UserSettings` rows), not in the broker; ZDR orgs limited to turn scope (persisting host names would break ZDR); no wildcard patterns until `isCanonicalDomain` (`MP/go/services/browser-broker/internal/grant/grant.go:332-354`) and `BrowserEgressPolicy::canonical_broker_domains` change together.
3. **E3 Org origin policy and Settings > Browser hub (L).** The org half is S7 to S10 plus a per-origin `access/uploads/downloads/rawCdp/persist` row schema in the policy document (v2 of the claim, additive keys); the hub is Frontend: a settings section backed by gateway routes that proxy org-core (admin) and user-core (user rules), managed rows labelled and locked, following the `McpServersSection.tsx` and `settings-ui.tsx` patterns named in the verification [UNVERIFIED paths], with the Verevon v3 action contracts added under `src/shared/actions` before any control is wired.
Dependencies: F-t2-001 (production grants), the ADR, OD13, F-t2-033 (approval loop UX). Effort as the verification states: split S-M plus L plus L; with WS7 absorbing E1, the remainder is about L plus L.

## 5. Dependencies and ordering

### 5.1 Ordering and hard versus soft dependencies

| Slice | Hard dependency | Soft dependency | Notes |
|---|---|---|---|
| S1 | none | catalogue key names reviewed with Control before S7 | pure library |
| S2 | S1 | WS2/D2 S1 (evaluate deny) is independent | first deployed behaviour |
| S3 | S1, S2 | **D2 S1 first** (otherwise evaluate stays ungated and the catalogue row `browser.act.script` is aspirational); D2 S2 (module split) and S3 (`admit_action` pipeline) make the edit one line | `agent_routes.rs` freeze: land in the quiet window D2 announces, or after |
| S4 | S2 | in-flight `search_routes.rs` diff committed; F-t2-020 (profile routes) | search edit last |
| S5 | S1 (for the catalogue-aware preset test) | none | independent of S2 to S4 |
| S6a (decorator, final URL, egress deny) | S2 | none | ships alone |
| S6b (durable snapshot) | S2 | **F-t2-016** (robots enforcement gives the snapshot something to carry) and **F-t2-017** (public job lifecycle) | until then a crawl child runs under baseline plus invariants only; documented gap |
| S0 | the ADR ruling | none | docs |
| S7 | ADR topic 1 (C or A); Control owner | product input on entitlements (Q3) | |
| S8 | S7; Control sign-off on `webPolicyByAudience` | none | outcome A drops the registry part |
| S9 | S2; S8 for the golden | none | can ship earlier in `observe` with an absent claim |
| S10 | **F-t2-001 / WS1** slice 1a for prod-mode testing, 1c for persistence; proto allocation agreed | S8 | merge D1 first |
| S11 | S3 (denial contract), sibling (b) | S10 (planner list) | works without a snapshot |
| S12 | S7, S10 | none | optional under C |

Downstream consumers (they depend on this workstream, not the reverse): F-t2-016 uses `robots.max_mode` and `JobPolicy`; F-t2-058 uses `proxy.max_tier`; F-t2-005 must call `require(RequestCredentials)` when it adds header options (S1's reserved-key test fails otherwise) and should reuse `RequestAsk` as the name and shape of its `policy` field; F-t2-023 adds a read-only evaluate as a **new** action class and key, never by relaxing `browser.act.script`; F-t2-039 consumes `browser.act.upload/download`; F-t2-049 reuses the `SecurityEngine` decorator pattern; F-t2-048 must check `domains.deny` and ZDR before any URL cache read; F-t2-008, F-t2-025 and F-t2-046 get their "claim channel" from S8 (a budget or quota claim is a sibling of `web_policy`, not another field inside it, to keep the 2 KiB cap).

### 5.2 Merge-conflict hotspots

| File | Lines (live tree) | Shared with | Handling |
|---|---|---|---|
| `QE/agent_routes.rs` [C] | `step` 2228-2409; `start_run` 1273-1663 (ZDR 1433-1445, profile 1470-1476, egress 1521-1531); `validate_run_grant` 314-338; `StartRunBody` 345-379; `execute_ws_action` 3086-3193; tab routes 3202-3451 | D2 S1-S4 (and its split), D1 (its domain-override condition at 1397-1412, adjacent to my egress call), F-t2-008, 011, 013, 014, 021, 032, 036 | WS7 code lives in `agent_policy_gate.rs`; in-file diff about 12 lines; land after D2 S2 |
| `QE/main.rs` [M] | 26-47 module block, 1485-1493 | D2 (mod lines, serve block), the 188-line uncommitted diff | single-line edits away from line 47; rebase by hand |
| `QE/state.rs` [M] | 102-106 | uncommitted `model_plane` field (line 90) | type change of an existing field |
| `QE/routes.rs` [M] | `ScrapeRequest` 346-379, 416, 424-432, 461, 984-1001, 1072, 1116, 1332 | F-t2-005 (rewrites `ScrapeRequest`), F-t2-016, F-t2-017 | add one optional field; F-t2-005 reuses the name `policy` |
| `QE/search_routes.rs` [M] | `build_search_options` 541-552, ZDR guard 696-727, response 178-185 | the uncommitted rerank and ZDR_REQUIRED work, F-t2-003, F-t2-042 | S4 after that diff commits |
| `QE/extract_routes.rs` [M], `answer_routes.rs` [M] | 205-237; 72-89, 172-187 | model-plane switch work | small |
| `QE/profile_routes.rs` [C] | 102-432 | F-t2-020 | coordinate |
| `QR/pipeline.rs` [C] | 436-448 (final-URL insert) | F-t2-005, 016, 023, 037 | one block |
| `scripts/tests/zdr-contract-sync.test.sh` [M] | GOLDENS and VENDORED arrays | every golden change (the uncommitted +95 lines) | append rows only |
| `quarry-token-claims.golden.json` (3 copies: `CP/auth-core/src/auth/fixtures/` [C], `Q/crates/quarry-edge/tests/fixtures/auth-core/` [C], `MP/rust/services/model-gateway/tests/fixtures/auth-core/` [U]) | whole file | F-t2-019, F-t2-037 (new principals), D1 | regenerate once, commit the three together |
| `CP/config/plane-service-principals.json` [C] | `execution-core` 134-178, `model-gateway` 289-317 | D1 (`browser-broker` audience for `quarry-edge`), F-t2-019, F-t2-037 | hand-merge |
| `MP/proto/model_plane/v1/browser.proto` [C] | message fields | D1 (10-19), WS7 (20), F-t2-032 (21-39), F-t2-008 (40-49) | numbers allocated in section 7 |
| `MP/go/services/browser-broker/internal/grant/grant.go`, `server/server.go` [C] | Grant struct 39-59; `AcquireGrant` 42-108 | D1 (large rewrite) | merge D1 first |
| `Q/services/quarry-orchestrator/internal/activities/activities.go` [C] | `RunPageInput` 165-181, body 214 | F-t2-016, 017, 018 | S6b only |
| `Q/docs/openapi.yaml` [M] | agent paths 263-345, scrape schemas; uncommitted 238-250, 4012-4150 | F-t2-002, 005, 013 | additive components |
| `CP/org-core/internal/http/service_auth.go` [C] | 50 (allowlist), 190-230 (boot checks), 340-370 (scope map) | any Control item adding a scope | small |
| `CP/org-core/migrations/` | next free 021, 022, 023 | any Control migration | re-check the day of the PR |

## 6. Plane ownership, ZDR propagation and Model-proposes/Quarry-executes check

- **Control Plane owns identity, users, orgs, billing, sessions, audit, quotas, entitlements.** Outcome C stores org web policy, plan entitlements and the audit history in org-core and issues the claim from Auth Core. Satisfied. **At risk under outcome B**, which would make the Model Plane the owner of org entitlements; that is the main reason B is not recommended.
- **Data Plane v2 owns documents, chunks, embeddings, retrieval, graph, wiki.** Not touched. No new Data Plane call; `ingest.persist` is listed as a future key and nothing here changes the ingest path.
- **Ingestion Plane captures evidence and persists through Data Plane contracts only.** WS7 only restricts. Quarry adds no store of its own for policy; the baseline is env, the org layer is a claim, the grant layer is a field.
- **Provider actions go through integration-corev2's actions surface.** Unaffected: no provider call is opened or closed by policy.
- **Model Plane owns reasoning, runs, capabilities, sandboxes, browser grants and cost.** The broker carries a **snapshot** it derives from the verified bearer and never accepts policy from a caller (no request field exists). capability-core stays the allow/deny/ask engine for `cap.browser.open` and is not made a policy store. Satisfied.
- **Application Plane and Spaces.** Not touched. A Space can restrict a run only through `RequestAsk`, which cannot loosen anything (3.4, test `ask_cannot_loosen_*`), so no Space can widen an org or owner-plane ACL.
- **Frontend Plane.** No UI here; the settings hub is the epic. The gateway is unchanged except an optional translation of the typed 403 for display. The gateway mints a fresh token per request and holds no policy.
- **No direct database crossing between planes.** org-core's tables are read only by org-core and served over an internal HTTP route; Quarry reads a signed claim and a gRPC field; the broker reads a claim; quarry-control's `blocklist_entries` is untouched. Verified: nothing in this design opens a connection to another plane's database.
- **Model proposes browser actions; Quarry executes or rejects.** Every denial is produced by Quarry (typed 403) before an approval, budget or receipt is consumed; the Model's only influence is a tighten-only ask and a planner hint. The broker snapshot is a hint plus a per-step meet input, never a Model-supplied authority. Satisfied; the one place to watch is S11, where execution-core builds the ask: it may only call `disable`.
- **ZDR must propagate.** ZDR is not re-modelled (3.7): `ResolvedPolicy.zdr` is the existing floor, the existing refusals stay, and `zdr_forces_disabled` documents the implications; the equivalence test blocks any loosening. The `web_policy` claim carries no content. Denial events and metrics carry capability and decision source only, never a URL, host or script, and go through `emit_for_zdr`; the typed 403 body may name a host in `details.host` because it goes only to the caller. org-core audit rows carry keys and `tightened`, not domain names, for a ZDR org. ZDR grants stay in broker memory (D1 D6); the snapshot column exists only on persisted rows. S6b's snapshot travels only on non-ZDR durable jobs because crawl and batch are refused under ZDR today (`QE/routes.rs:563-588`). The final-URL recheck uses `emit_page_event`, which already routes through the ZDR sink.
- **Rules at risk, stated plainly.** (1) Outcome B conflicts with "Control owns entitlements". (2) `domains.deny` applying to human manual browsing is a deliberate exception to "never block the human user's own manual browsing" (3.6) and needs a confirmation (Q4). (3) The final-URL preflight (S6) tightens redirect handling beyond policy (it also applies URLhaus and PhishTank to redirect targets): staged behind a flag. (4) Sub-resource denial (S6) changes a proxy type used by other code.

## 7. Cross-session interfaces

### (a) Verevon Space (shared workspaces, agent PC and browser inside Spaces, ZDR-safe telemetry)

| Shared piece | Proposed owner | Notes |
|---|---|---|
| `RequestAsk` as the only way a Space restricts an agent browser | contract: WS7; Space settings storage: Application Plane (Space owner) | Space setting becomes an ask built by model-gateway; the meet makes widening impossible |
| `policy.denied` event and `web_policy_*` counters | WS7 defines shape; Space UI consumes | fields: `capability`, `decision_source`, `denied_action`, `policy_rev`; never a URL or host; ZDR runs go through `emit_for_zdr` |
| Which org policy a Space run is under | the run's owner and org (the claim and the grant), never the Space | a Space member cannot borrow another member's looser grant |

The user must reconcile: whether Space-level browser restrictions exist at all (and who stores them), and that the Space session's telemetry design (F-t2-004) accepts this event shape.

### (b) Model Plane and chat or agents (grants, delegated bearer, approvals, agent browser path)

| Shared piece | Proposed owner | Notes |
|---|---|---|
| `browser.proto` field 20 `WebPolicy` on `AcquireGrantResponse`, `ValidateGrantResponse`, `DeriveRunGrantResponse` | contract WS7, code sibling (b) with WS1/D1 | allocation: D1 uses 1-12 (10-19 new), WS7 20, F-t2-032 21-39, F-t2-008 40-49 |
| `PolicyResolver` hook in `grant/policy.go` | D1 defines, WS7 implements `ClaimPolicyResolver` | D1 D4 left the hook owner-neutral for exactly this |
| `browser-broker` user bearer carries the claim | WS7 (auth-core S8) | D1's delegated browser bearer is the transport for the issue-time claim |
| `StartRunRequest.policy` and the `policy_denied` mapping in `execution-core/src/quarry_agent.rs` | contract WS7, code sibling (b) | JSON golden for `RequestAsk` vendored by execution-core (separate Cargo workspace) with a sha256 pin, like the other goldens |
| approvals (F-t2-033) | sibling (b) | `policy_denied` is never approvable; only a domain miss against the user's own grant is (E-032) |
| `cap.browser.open` and capability-core allow/deny/ask | stays Model Plane | not a policy store |

The user must reconcile: the proto field allocation; who owns the vendored `RequestAsk` golden; whether the planner prompt (F-t2-010) lists allowed capabilities from the snapshot; and the revocation expectation (345 s for Model runs under outcome C) with the approval-timeout design.

## 8. What can proceed WITHOUT an ADR/decision and what is blocked

**Proceeds now, no ruling needed:** S1 (library and catalogue), S2 (scrape-family wiring and the four `RunPolicy::default()` sites), S3 (agent gates; order after D2 S1), S4 (remaining routes; ZDR consolidation optional), S5 (`JobPolicy` rename, preset `requires`, `/v1/blocklists` 410), S6a (decorator, final-URL recheck, egress deny list), S11 contract and rendering (sibling b), and the E1 part of E-032. Total about 24 engineer-days, none of it blocked, and every slice works under any outcome of topic 1 because the org layer is an `Option<PolicyLayer>` whose source it does not know.

**Blocked by audit ADR topic 1 (policy of record):** S0, S7, S8, S9 (claim intake), S10, S12. Which of them and in what shape depends on the outcome (table at the end of section 4).

**Blocked by other items, not by an ADR:** S6b (F-t2-016, F-t2-017); production-mode evidence for S3 and S10 (F-t2-001); the Control sign-off on the registry field (part of the topic 1 ruling).

**Not blocked, but adjacent decisions to respect:** topic 3 (shared human and agent sessions): WS7 uses the principal-derived autonomy of D2 and does not decide session sharing. Topic 4 (ZDR per feature): untouched, existing refusals kept. Topic 5 (anti-bot posture): `stealth` and `fast` presets are marked, not deleted; `proxy.max_tier` is reserved. Topic 6 (script model): `browser.act.script` is a non-unlockable invariant, compatible with any later answer.

## 9. Open questions for the user (only real blockers)

1. **Policy of record (audit ADR topic 1).** Confirm outcome C (org-core owns; claim on user tokens and org-resolved service tokens; grant carries a snapshot), or choose A (user claims only, grant mandatory for Model runs) or B. Blocks S0 and S7 to S10 and S12. Recommendation: C.
2. **If C: sign-off from the Control Plane owner** on the new registry field `webPolicyByAudience` (only `execution-core` and `model-gateway`, only for `quarry`), and on token lifetime: keep the 300 s audience TTL (tightening delay about 345 s) or apply a 60 s cap (about 105 s, about five times the mints from execution-core). Blocks S8. Recommendation: no cap by default, cap available by env.
3. **Entitlements.** Which capabilities are plan-gated, and who may grant `robots.max_mode=ignore` (the audit's open question 11: a platform operator or an org admin)? Blocks only the entitlement fold-in of S7. Proposed default: `web.robots_override`, `web.proxy_pool`, `web.paid_search` plan-gated, robots override grantable by platform operators only.

Non-blocking confirmations (defaults proposed, reversible by one catalogue edit): (a) `domains.deny` applies to human manual browsing as well as to agents (security semantics); (b) `/v1/blocklists` answers 410 and its table is left in place, rather than being dropped.

## 10. INTERFACE SUMMARY

**Files touched.** *Ingestion (Quarry-v2):* quarry-core (`web_policy/*` NEW, `policy.rs` rename to `JobPolicy`, `lib.rs`, contracts catalogue JSON NEW); quarry-runtime (`policy.rs`, `policy_gate.rs` NEW, `pipeline.rs` final-URL, `grant_validator.rs`, `grpc/browser_broker.rs`, vendored `browser.proto`, egress proxy); quarry-browser (`BrowserEgressPolicy.denied_domains`); quarry-edge (`policy_baseline.rs`, `web_policy_edge.rs`, `agent_policy_gate.rs` NEW; `routes.rs`, `main.rs`, `state.rs`, `auth.rs`, `agent_routes.rs`, `search_routes.rs`, `extract_routes.rs`, `profile_routes.rs`); quarry-control Go (`presets.go`, `blocklists.go`, `cmd/control/main.go`); orchestrator (S6b); `openapi.yaml`. *Control:* org-core (migrations 021-023, `web_policy*.go`, `service_auth.go`, `server.go`); auth-core (`web-policy.ts`, `convex-token.service.ts`, `plane-token.controller.ts`, registry); `plane-service-principals.json`; compose. *Model Plane:* `browser.proto`, browser-broker (`grant/*`, `server.go`, migration 0002), `authctx`, execution-core `quarry_agent.rs`. *Repo:* `zdr-contract-sync.test.sh`, three golden copies.

**New contracts.** Claim `web_policy` (v1) on `quarry` and `browser-broker` tokens; catalogue `web-policy-catalogue.v1.json`; `RequestAsk` on scrape, scrape/stream and `POST /v1/agent/runs`; 403 `FORBIDDEN` details `{reason, capability, decision_source, policy_rev}`; event `policy.denied`; proto `WebPolicy` field 20; org-core `GET/PUT /orgs/:id/web-policy`, internal `GET /internal/orgs/:orgId/web-policy` (scope `org:web-policy:read:any`); registry `webPolicyByAudience`; env `QUARRY_EDGE__WEB_POLICY_{BASELINE_JSON,ENFORCEMENT,REQUIRE_CLAIM,FINAL_URL_CHECK}`.

**Dependencies.** S1-S6a none external; S3 after D2 S1; S6b needs F-t2-016/017; S10 needs F-t2-001/D1.

**Effort.** S1 3 d, S2 4, S3 4.5, S4 3.5, S5 2, S6 4.5 (21.5 d, no decision); S0 0.5, S7 6.5, S8 7.5, S9 3, S10 6.5, S11 3, S12 4 (31 d). Total about 52 engineer-days; F-t2-022 stays L, owner-neutral part M.

**ADR gates.** Topic 1 gates S0, S7-S10, S12. No other topic blocks.

**Parallelisable with.** F-t2-005, 007, 009, 015, 026, 027 (disjoint files); S5 with anything after S1; S3 and S4 together after S2.

**Conflicts with.** D2 (`agent_routes.rs`), D1 (`grant.go`, `browser.proto`, principal registry, `main.rs` flags), F-t2-005 (`ScrapeRequest`), F-t2-020 (profile routes), in-flight uncommitted `search_routes.rs`, `main.rs`, `state.rs`, sync script.

**Key findings.** `RunPolicy` is only a determinism stamp, so replacing it enforces nothing by itself; there are four production `default()` sites, not two; presets are consumed but never reach the edge; service tokens can carry an org claim by the existing `tenant` mechanism, so the grant need not be the only carrier.

<!-- COMPLETE -->
