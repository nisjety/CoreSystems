# Architect A: decisions, interfaces, reconciliation (Quarry v2 plus shared browser plan)

Author: Sonnet 5.5, architect A (decisions and interfaces). Date: 2026-10-15. Read-only pass. Inputs: audit `C:/dev/verevon-audit-2026-10-14/02-quarry-and-browser-audit.md` (sections 8, 9.3, 10.2 via `.plan-work/audit-sections`), designs D1 to D16 (interface summaries and sections 3, 5, 7, 8, 9), sibling audits 01 (Space) and 03 (Model Plane). Live tree `C:/dev/CoresSystem` (afb83928 plus ~324 uncommitted entries). Path tags from `tools/provenance.sh`: `[C]` committed, `[M]` modified in live tree, `[U]` untracked, `[NEW]` created by the plan.

Workstream ids (fixed): WS1 grants (D1) | WS2 run control (D2) | WS3 driver (D3) | WS4 ZDR/tenant (D4) | WS5 scrape contract (D5) | WS6 budgets (D6) | WS7 web policy (D7) | WS8 shared browser backend (D8) | WS9 shared browser frontend (D9) | WS10 run trace (D10) | WS11 planner (D11) | WS12 redaction (D12) | WS13 scrape actions/capture (D13) | WS14 crawl scope/robots (D14a) | WS15 job lifecycle/schedules (D14b) | WS16 extraction (D15) | WS17 profiles (D16). Two designs label themselves wrongly and are corrected here: D13 says "WS17" (it is WS13); D3 says its actionability slice is "WS12" (it is WS3, a W2 slice inside WS3). Slice ids are written `WS<n>.<slice>` using each design's own slice names (D2 `S3`, D4 `S-8`, D8 `S8.5`, D9 `Slice 6b`, D14b `L1`, D16 `E2`, D12 `S5`).

How to read the records: "Blocking?" says whether the decision blocks BUILDING or MERGING work in W1 or W2, or only later. "Proceeds without" names the slices that ship a conservative default and need no ruling. Evidence is `[READ]` when I re-opened the live code in this pass, otherwise the design's own verified claim is cited as D<n> §<m>.

Live-code spot checks made in this pass (all confirmed): production quarry-edge refuses to start without a BrowserBroker validator (`quarry-edge/src/main.rs` [M] 178-204); `Claims::zdr_floor` ignores service principals by design (`auth.rs` [C] 104-123); the gateway search cache key has no org segment (`gateway/src/domains/search.rs` [C] 211-229); `refuse_durable_job_under_zdr` only covers crawl and batch (`routes.rs` [M] 560-588); `stealth.rs` [C] has no caller (grep for `stealth::` outside the file is empty) and `block_rotation` is live (`quarry-runtime/src/egress_broker.rs` [C] 149); the viewport relaunch kills every page (`chromiumoxide.rs` [C] 194-223); compose defaults `QUARRY_BROWSER_NO_SANDBOX` to 1 (`deploy/compose/docker-compose.yml` [C] ~97-99); `quarry-edge` in the principal registry holds only the `data-plane` audience today (`plane-service-principals.json` [C]); `cross-plane-token-requests.json` [M] keeps `model-invoke` at `granted:false`; `events.proto` [C] has `reserved 14 to 20` (D10 edits it); migrations: next free is quarry-runtime `0010`, quarry-control `015`, org-core `021`, session-core `0046` (0044 and 0045 are `[U]`), broker none yet.

---

## 1. DECISION RECORDS

Ids: `ADR-1..10` are the audit 9.3 topics; `OQ-1..15` are the audit 10.2 questions (those that restate an ADR are short and point to it); `X-n` are decisions the designs tag ADR-GATED or owner-gated that the audit does not list. Within each group the order is blocking-W1, blocking-W2, later.

### 1A. BLOCKING W1 (blocks building, merging or production enablement of a W1 slice)

#### X-1 Register `quarry-edge` as a read-only `browser-broker` principal (D1 D5)
- **Question.** How does production Quarry authenticate to the broker to validate grants? Without an answer no production run validates (G1, `main.rs` [M] 178-204 `[READ]`).
- **Options.** (a) registry row `quarry-edge` audience `browser-broker`, scope `browser:read` only, per-org token via the existing manifest machinery; (b) forward the user's browser bearer to Quarry (fails for the AI path: execution-core calls Quarry with its own org-scoped service token and the user bearer lives 300 s against a 15 min grant); (c) self-contained signed grants verified offline (loses revocation, different security model).
- **Recommendation.** (a). `browser:read` only validates; no service principal gets `browser:write` (a contract-spec assertion). Retention for this audience `zdr` (token carries no retained content).
- **Gates.** WS1.1a merge (Quarry credential, auth-core rebuild); every browser item end-to-end (F-t2-010/011/012/013 ph2/024/032/033/039/063).
- **Proceeds without.** WS1 broker Repository refactor, kinds, model-gateway grant routes, L1 contract test, Quarry G6 fix, boot gates; leg A of L2 against a dev-bypass Quarry.
- **Decider.** Control Plane owner (principal registry) plus the user for the ledger entry.
- **Blocking?** Yes, W1 (merge of 1a). Bundle with the other `quarry-edge` registry and credential edits (section 2, R-13) so Control reviews once.

#### X-2 ZDR browser sessions use memory-only grants on a single broker replica (D1 D6; ADR-4 extension)
- **Question.** ZDR grants must not be persisted, so they die on broker restart and cannot span replicas. Acceptable?
- **Options.** (a) ZDR grants in broker memory only, id prefix `z_` routes to the memory repository, broker stays one replica (today it is one container); (b) persist ZDR grants encrypted (violates "refused or ephemeral"); (c) refuse ZDR browser sessions whenever the broker has more than one replica.
- **Recommendation.** (a), written into the ledger; add an assertion that Postgres has no `zdr` column. model-gateway already refuses ZDR AI runs (`browser_run.rs` [M] 307-315), so only ZDR human sessions rely on this.
- **Gates.** WS1.1c production rollout; WS8 `human_browse` for ZDR users (a 4 h session on an in-memory broker dies on restart).
- **Proceeds without.** 1a, 1b, the Postgres store for non-ZDR grants.
- **Decider.** The user (data governance), Model Plane owner for the replica constraint.
- **Blocking?** W1 production rollout of 1c, not building.

#### ADR-4 / OQ-4 ZDR semantics per feature (monitors, schedules, durable crawls, URL caches, telemetry, profiles, AI formats)
- **Question.** For each feature does ZDR mean refused, inline-only, hash-only or metadata-only?
- **Options and evidence (D4 §3.3 matrix, 24 rows).** Refused (today for durable crawl/batch `routes.rs` [M] 563-588 `[READ]`; PageCache and SearchCache reads and writes skipped under ZDR); inline-only (scrape results returned in the response, nothing persisted: D5 D12); hash-only (rejected by D4: a hash of a known URL is a membership oracle, and a hash-only URL cache serves nothing); metadata-only (a tombstone with ids, outcome, counts, no URL: D4 §3.9). Correction to the audit text: schedules and monitors are NOT refused today (D4 §2.3); a ZDR scrape logs URL and title at INFO.
- **Recommendation.** Adopt D4's table as the published default: never loosen an existing refusal; refuse monitors, schedules, profile CRUD (except list/delete) and AI formats for ZDR orgs behind `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` (observe for one week, then enforce, customer notice first); scrape inline-only; captures refused until the inline path ships; log fields content-free; spans dropped at the sampler. Tombstone only after counsel review. Publish the matrix generated from route attributes, not hand-written JSON.
- **Gates.** WS4 S-8 (enforce), S-10 (tombstone), WS14b L-S4 posture enforce, WS17 P0 enforce, WS16 `AI_FORMATS_ZDR=allow`, WS10 ring wording, WS9 D-3.
- **Proceeds without.** WS4 S-1 to S-7 (S-7 documents and pins today's behaviour), all observe modes, log-field discipline, inline-only scrape in WS5.
- **Decider.** The user (product and legal); Control Plane for the org-posture signal (X-9).
- **Blocking?** W1 for WS4 S-8/S-10 only; the rest of WS4 proceeds. A plain "yes, as recommended" is enough.

#### OQ-3 Was the gateway search cache ever live outside a developer machine, and did a cross-org hit happen? (D4 Q1)
- **Question.** The cache key has no org segment and feeds org-scoped private-corpus providers (`search.rs` [C] 211-229 `[READ]`). Stock compose does not set `GATEWAY_CACHE_REDIS_URL`; the e2e compose and a local `.env` do.
- **Options.** (a) fix only (org-scoped key, ZDR gate, request-shape deny list, purge); (b) fix plus forensic count of cross-org hits from Redis plus incident record.
- **Recommendation.** (b): WS4 S-1 ships the fix first (3 days), S-2 runs the forensics and purge on every environment that ever set the variable. State the incident record as "reachable in production since <revision>" or "dev only" from deployment history, not from code.
- **Gates.** Wording of the incident record only; D9's omnibox search flag stays off until S-1 merges.
- **Proceeds without.** Everything.
- **Decider.** The user (incident classification), ops for the deployment history.
- **Blocking?** No for building; yes for closing the incident.

#### ADR-5 / OQ-5 Anti-bot posture (honest identity, rotation triggers, stealth code)
- **Question.** Is honest identity official, and which policy-gated escalation is allowed?
- **Options.** (a) honest: a status is never an identity-change trigger; delete `stealth.rs`; proxy rotation on transport failure only; TLS-fingerprint fallback not a block reaction; rename the `stealth` preset with a one-release alias; (b) keep shipped behaviour (rotation on bare 401/403/407/503, `prior_block_signals` forcing, Chrome-TLS fallback); (c) a policy-gated middle (tiers allowed per org, F-t2-058).
- **Evidence.** `stealth.rs` [C] has no caller `[READ]`, deletion is safe; live rotation exists (`egress_broker.rs` [C] 149 `[READ]`, `fallback_driver.rs` 95-108, `driver_plan.rs` 40-46 per D3 §3 D3-13). D3 centralises triggers in `escalation_allowed()` behind `QUARRY_BLOCK_ESCALATION=legacy|honest`.
- **Recommendation.** (a). The Chrome-TLS driver may stay registered for non-block reasons. The profile restore does not replay the user agent (D16 Q5). Identity token for robots stays `quarrybot`, `quarry` until F-t2-067.
- **Gates.** WS3 S8 (default flip and preset rename), WS14 robots-unreachable posture and identity tokens, WS17 UA replay.
- **Proceeds without.** WS3 S4 (delete stealth, stealth_level, FingerprintRotator, false doc claims), S7 (classification, one-recovery rule), S8 mechanism with `legacy` default.
- **Decider.** The user (policy).
- **Blocking?** W1, but only for the S8 default flip.

#### OQ-10 Which claims need runtime confirmation first? (spikes)
- **Question.** AI-path 401/403 in production, viewport relaunch frequency, whether Chromium masks password values in the AX tree, whether same-host scrape sessions from different orgs share a session key.
- **Options.** (a) plan on the code reading; (b) reproduce first; the audit states nothing in Verevon was run.
- **Recommendation.** (b), as the first days of W1, each with a named owner and a pass/fail recipe already in the designs: D1 L2 legs A to C on the prod overlay (AI-path 401/403 and Quarry prod-boot spike); D3 S0/S1 plus the relaunch counter from S2 (relaunch frequency); D12 recipe D12-6 (AX password masking, decides the field-probe primary); D13 slice 1 (same-host scrape session sharing, fixed there regardless). Record results in each PR.
- **Gates.** WS1 scope (XL only if the boot spike finds more), WS12 S1 primary probe, WS3 S2 priority.
- **Proceeds without.** Everything: each fix is correct under either answer.
- **Decider.** Engineering; no user ruling.
- **Blocking?** W1 first week, evidence not decision.

#### X-3 Merge governance: arbiter, single editors, allocation ledger (not tagged ADR-GATED; every design asks for it)
- **Question.** Who arbitrates merge order and who is the single editor of the hot files?
- **Evidence.** `agent_routes.rs` is edited by 9 workstreams (D2 §5.3); broker files by 6; execution-core browser files by D3/D6/D7/D8/D11 and sibling (b); `chromiumoxide.rs` by D3/D8/D12/D13/D16; ~324 uncommitted entries sit under `main.rs`, `state.rs`, `config.rs`, `routes.rs`, `search_routes.rs`, `extract_routes.rs`, `zdr-contract-sync.test.sh`, `openapi.yaml`, session-core `0044/0045`.
- **Recommendation.** (1) Commit or branch the in-flight uncommitted work as a baseline before W1 starts. (2) One arbiter for merge order. (3) Merge order rules: broker files WS1 first; `agent_routes.rs` WS2 S1 then S2 split (two-working-day freeze) then S3; `chromiumoxide.rs` serial D3 S2, S3, S5, S10, S6 then D8 S8.4; execution-core browser files one editor (D11 proposal). (4) Migration, proto, event-number and registry allocations from the tables in section 2 recorded in one ledger entry. (5) Contract goldens committed as a triple (three copies of `quarry-token-claims.golden.json`).
- **Gates.** WS2 S2, WS1 broker work, every W2 slice in the hot files.
- **Proceeds without.** Slices in disjoint files (WS4 S-1, WS5 slices 1-2, WS3 S0/S1/S4, WS2 S1/S5).
- **Decider.** The user or a delegate.
- **Blocking?** Yes W1 (process).

### 1B. BLOCKING W2

#### ADR-1 / OQ-1 Policy of record for web and browser capability (plus fail mode, minter, revocation)
- **Question.** Does org web policy live in Control Plane org-core (signed claims) or Model Plane capability-core (grant fields)? How is a policy revoked mid-run when grants live 15 min?
- **Options (D7 §3.1).** A: claims on user tokens only, grant fields for Model-driven runs (two resolution paths). B: capability-core owns, grant fields only: cannot reach `/v1/scrape`, `/v1/search`, `/v1/crawl` etc. (no grant on those routes, `routes.rs` 130-190) and cannot store a proxy ceiling or robots mode (`EvaluateCapability` is allow/deny/ask by risk). C: org-core owns; claim on user tokens AND on org-resolved service tokens (auth-core already resolves tenant-owned values at mint time via `resolveTenantServiceRetentionPosture`, `interactive-retention-policy.ts` 514-545); grant carries a snapshot.
- **Recommendation.** C. Revocation: claim staleness about 345 s default (300 s audience TTL plus 45 s resolver cache), 105 s with `WEB_POLICY_TOKEN_TTL_CAP_SECONDS=60`; do not apply the 60 s cap by default. Sub-decisions that ride with it: fail mode for the entitlement read is fail-to-floor (last good for 15 min, then static floor; D6 DEC-6); the budget claim minter is model-gateway (D6 DEC-8; if capability-core wins later only the minter moves); fallback to A if Control declines the registry field `webPolicyByAudience`.
- **Gates.** WS7 S0, S7 to S10, S12; WS6 S3 plan-aware clamps and DEC-6; WS14 override entitlement (slice 7); F-t2-032, F-t2-025/046/048 claim channel.
- **Proceeds without.** WS7 S1 to S6a (library, scrape-family wiring, agent gates, preset `requires`, decorator; owner-neutral `Option<PolicyLayer>`), WS6 static mode, WS11 interim env allowlist.
- **Decider.** The user, with Control Plane owner sign-off (registry field and token lifetime) and Model Plane owner (grant fields).
- **Blocking?** W2 for WS7 org-layer slices; not blocking W1.

#### ADR-3 / OQ-2 / OQ-12 Shared human plus agent sessions (grant binding, navigation policy, idle numbers, run lifetime per chat thread)
- **Question.** How does a grant bind a session that a human and a Model run both use, what navigation policy governs free human browsing, what replaces the 120 s run TTL, and does a chat thread keep one long-lived Quarry run?
- **Options (D8 §3 D8-1).** 1: broker `human_browse` kind (open public web minus a broker deny-list, renewable by user presence) plus run-bound `attach` child grant; service principal attaches; Quarry attach registry; per-tab control. 2: delegation claim (RFC 8693 `acting_user_id`): authorises the service on EVERY live session of that user, says nothing about hosts or TTL; kept only as optional hardening S8.10. 2b: forward the user's `aud=quarry` bearer (OD13 clause): agent origin becomes client-asserted, 300 s bearer cannot cover a run. 3: end and resume under a new grant: fails because `resume_run_id` requires owner equality (`agent_routes.rs` 1334-1342) and restores URL only. D9 offers a fourth: observe-only relay (Slice 6a) then attach (6b).
- **Recommendation.** Option 1, ratified as one package: D8-1 (kinds and invariants I1 to I8), D8-2 (open web minus deny-list), D8-3 (900 s idle interactive, 1800 s takeover idle, 4 h absolute, attach 900 s with 180 s idle release, keep-alive 300 s), D8-7 implicit take-over on non-passive human action. D2 numbers: agent idle 300 s, interactive idle 900 s, takeover idle 1800 s, backstop 4 h. OQ-12: one long-lived user-owned run across chat turns, a Model Plane run attaches per run, agent-private browsing stays one run per Model Plane run. Skip D9's 6a relay if this is ratified before W2 starts (saves 8 to 10 days); do 6a only as the fallback if ADR-3 is refused. Human seconds are metered never capped (D6 DEC-1).
- **Gates.** WS8 S8.2/S8.5/S8.6 merge, `human_browse` and `ATTACH` production enablement; WS9 Slices 4c GA, 6b, 7, D-13; F-t2-012/013 ph2/035/054/024/038; WS2 phase 2.
- **Proceeds without.** WS8 S8.0, S8.1 (strict tightenings, flagged), S8.4, S8.7; WS9 Slices 1, 2, 3a to 3c, 4a, 4b, 4c behind flag, 5; WS2 phase 1.
- **Decider.** The user; Model Plane owner for grant kinds; Control Plane not needed in phase A.
- **Blocking?** W2 (blocks merge of S8.2/S8.5/S8.6 and GA); the ratification should happen in the first weeks of W1 so W2 can start.

#### ADR-8 / part 1 and part 2 Run trace envelope and sharing (D10 D1, D13)
- **Question.** Which envelope is canonical, and how do Application Plane projections and Space reads avoid widening the owner's ACL?
- **Options.** Part 1: extend `model_plane.v1.Event` (fields 14-18 inside the reserved range; event types 150-159) versus a second envelope. Part 2 (D10 D13): A coarse debounced Convex projection, owner-checked read-through; B full trace in Convex (rejected: write amplification and a second ACL); C copy events into a Space (rejected: widens ACL, violates matrix rule 10); D no projection.
- **Recommendation.** Part 1: extend (additive, the reserved range was created for it `events.proto` [C]); record allocation in the ledger. Part 2: A, owner-only reads in v1 until the resource-taxonomy ADR (F-t2-072).
- **Gates.** WS10 S2 ratification before production; S7 (App Plane projection) blocked until part 2.
- **Proceeds without.** WS10 S1, S3, S4, S8a/b/c.
- **Decider.** The user; Application Plane owner for S7.
- **Blocking?** W2: part 1 blocks production use of S2, part 2 blocks S7 only.

#### X-4 Cross-plane NATS credential for run trace (D10 D5)
- **Options.** (a) publish-only principal `quarry-trace-producer` on Model-nats, subject `mp.v1.trace.run.*`, no JetStream API rights; (b) a bridge service (adds an unowned service, keeps raw payloads on the Ingestion bus); (c) fold into the existing Ingestion bus with a filter.
- **Recommendation.** (a) after security review. **Gates.** WS10 S5 enablement outside dev. **Proceeds without.** Build of S5. **Decider.** Security reviewer plus Model Plane owner. **Blocking?** W2 (enablement only).

#### X-5 ZDR and the replay ring (D10 D9, ADR-4 confirmation)
- **Options.** In-memory, live-run-only replay ring for ZDR runs (holds what the 256-slot broadcast channel already holds, freed at `close_run`) versus no replay for ZDR. **Recommendation.** Ring allowed; one-line flip either way. **Decider.** The user. **Blocking?** No; confirm during W2.

#### X-6 Owner of the Model Plane trace persistence, session-core consumer and `EventLog` server (D10 S6, about 10 engineer-days)
- **Options.** Sibling (b) staffs S6; this plan staffs it in Model Plane files that carry uncommitted diffs; defer S6 and ship S1 to S4 (16.5 days of value without persistence). **Recommendation.** Sibling (b) owns, after `0044/0045` are committed; defer if unstaffed. **Decider.** The user. **Blocking?** W2 scheduling of S6.

#### X-7 DR-15.1 the Quarry-to-Model-Plane structured-extraction lane, and OQ-14 owner of DEF-QB-T17-PART2 (D15 DEC-2)
- **Question.** Who owns the org-bound metered Model Plane path for services, and what is the grant shape?
- **Options.** G1 two registry audiences (`model-gateway: models:invoke`, `inference-core: inference:invoke, models:invoke`), retention `tenant`, two new manifest rows, `model-invoke/stream/audio` stay dark; G2 one audience plus a gateway-held inference principal (ambient authority, rejected); G3 no grant (interim Model-proposed route only); G4 status quo (refused in prod, `main.rs:176`). Owners: O1 Model Plane gateway owner; O2 Control registry owner; O3 Ingestion (violates ownership); O4 split with O1 accountable.
- **Recommendation.** G1 and O4 with O1 accountable (matrix rows 62 and 88), written as the draft ledger entry in D15; ship slices 3 to 5 and 8 dark meanwhile. OQ-14 answer: the Model Plane gateway owner.
- **Gates.** WS16 slice 2 and any production use of AI formats; F-t2-030 ph2, 037, 050, 065 (all wait on the same grant).
- **Proceeds without.** WS16 slices 0, 1, 3, 4, 5, 7, 8 (interim route).
- **Decider.** The user names the owner; Control Plane owner co-signs.
- **Blocking?** W2.

#### X-8 Key management ownership (D16-3, D16-4, D16-7; proposed audit item 11)
- **Options.** (a) org-core behind an interface (`internal/orgkeys`, hierarchy master -> per-org KEK -> per-profile DEK; cascade delete gives crypto-shred); (b) auth-core (wrong blast radius); (c) new `key-core` service (XL). Master key: env first (loss destroys every profile; escrow runbook, boot canary, restore drill), KMS later. Owner model: `owner_user_id` set on insert, `user_private` owner-only, `org_shared` members read, non-owner gets 404.
- **Recommendation.** (a), env master as an explicit logged interim, observe-first owner enforcement, legacy `user_private` rows hidden not deleted.
- **Gates.** WS17 E2 and E3, `REQUIRED=1`, P2 enforce. **Proceeds without.** WS17 P0, P1, E1 with the `env` provider, S0, C1, C2. **Decider.** The user (ownership docs change) and Control Plane owner. **Blocking?** W2 (late).

#### X-9 Job-lifecycle decisions that are not audit topics (D14b D9, D14, D15, D17)
- **D9 results source.** Option A: events as a page index (`page_extracted` rows, excerpt only, capped); alternatives: wait for F-t2-052 result sets. Recommend A as phase 1. Blocks L4 only.
- **D14 trigger and backfill transport.** 1 Temporal SDK client in quarry-control (second client, silent-success `NoopClient` hazard), 2 pull-model `schedule_requests` rows claimed by the orchestrator every 2 s (`FOR UPDATE SKIP LOCKED`, recommended), 3 orchestrator HTTP endpoint. Backfill refused for crawl and batch, 24-window cap.
- **D15 ZDR for schedules.** Refuse at creation with D4's code; re-check posture at fire time via an edge preflight that reads org-core `retention-posture` (needs a `quarry-edge` principal with `org:retention:read:any`, currently held only by auth-core, a documented one-holder invariant and two contract tests). Fail closed `skipped_posture_unknown`.
- **D17 failure notices.** Application Plane registers `quarry-control` as a notification-core caller (types `quarry.schedule_run_failed|skipped`, HMAC v2 delegation, content-free payload). Needs the App Plane owner.
- **Gates.** WS15 S3, S4 enforce, failure notices, S5 go-live (also needs WS14 scope deployed, D6 S0 deployed, one week of observe). **Proceeds without.** S0, L1, L2, L3, L5 without results, S1 (org scoping closes a cross-tenant exposure), S2 flag off. **Decider.** The user, Control and Application Plane owners. **Blocking?** W2 for S3/S4/S5; S0/L1/S1 start immediately.

#### X-10 Crawl and robots decisions (D14a D4, D9, D10, D1)
- **Robots when unreachable (5xx or timeout).** disallow (RFC 9309, recommended, flaky robots endpoints will stop crawls; observe soak first) versus allow (Firecrawl). Tokens `["quarrybot","quarry"]` until F-t2-067.
- **Ignore-robots override.** Audited Control Plane entitlement through D7's `robots.max_mode` ceiling; audit event written by quarry-control BEFORE the job is acknowledged (fail closed); grantor: platform operators only (D7 Q3 default) versus org admins (OQ-11).
- **Crawl under ZDR.** keep the refusal (the alternative is an XL redesign: five retention surfaces).
- **Default scope change.** same host with `www.` equivalence, whole host, page cap 1000, `max_pages:0` rejected. Existing crawls lose off-site pages; soak in `observe` first.
- **Gates.** WS14 slice 8 (observe to enforce), slice 7. **Proceeds without.** Slices 0 to 6. **Decider.** The user (ADR-5, ADR-1). **Blocking?** W2 for the flip and slice 7.

#### X-11 Production-enablement owner decisions for the planner (D11 OD13, quota keys)
- **Question.** Planner route (stay on Balance or pin a subscription route with a guard), quota-key convention for the interim org flag, who edits execution-core browser files. Note: "OD13" is one open-decision bundle of `CHAT_GAP_CLOSURE_PLAN_2026-10-01`; D1 cites its "start host plus at most 8" clause, D8 its "user's aud=quarry bearer" clause, D11 its "pin Terra" clause. They must be answered together by sibling (b).
- **Recommendation.** Keep Balance until Tier C numbers exist; org-core quota rows `browser_agent_llm_planner` (1 on, 2 off) and `browser_agent_max_tokens_per_run` as an interim labelled carrier removed when ADR-1's carrier lands; one editor of the execution-core browser files. Run length about 4 minutes until bearer refresh exists.
- **Gates.** WS11 stage 3 (`default_on`), production use of S2. **Proceeds without.** S0, S1, S3, S4, S6, S7, S2 for dev. **Decider.** The user and sibling (b); Control owner for quota keys. **Blocking?** W2 stage 3 only.

#### X-12 Budget numbers and gate order (D6 Q1, Q6)
- **Question.** Shipped defaults and ceilings (agent 60 steps, 900 s, 1.00 USD; ceilings 300 steps, 3600 s, 20 USD; crawl 1000 pages depth 5, ceilings 5000 and 10; 3 concurrent jobs, backlog 200; estimator 0.00003 USD per browser second to be replaced by the billing-core price table), and the gate order in `step`.
- **Recommendation.** Accept as placeholders ratified before `on`; adopt the canonical gate order in section 2 R-7 (budget before sensitive approval, so a refusal never burns a single-use approval). **Decider.** The user. **Blocking?** W2: before WS6 S1/S3 leave shadow/static.

#### X-13 Browser human-browsing evidence and jar decisions (D8-5, D8-6, D8-7)
- **D8-5.** Human steps on non-ZDR runs persist metadata-only receipts (typed text `[redacted]`, query and fragment dropped, no observation, checkpoint URL stripped); history is not stored in Verevon. Hard prerequisite to enabling `human_browse`; a strict tightening that can ship now. **D8-6.** Attach shares the user's logged-in jar (default) or opens an isolated context. Recommend shared jar plus the per-tab egress policy equal to the attach list, with F-t2-032 later replacing implicit consent. **D8-7.** Implicit take-over (Codex lets the user interact at any time): recommend yes, one constant switches to explicit button. **Decider.** The user. **Blocking?** W2: D8-5 blocks `human_browse` enablement, D8-6 blocks `ATTACH` GA, D8-7 blocks the S8.5 default.

#### X-14 Document parser isolation, skipped-media reporting, strict-validation date, scrape retention (D5 Q1 to Q4)
- **Q1 parser isolation.** In-process parser with input and output caps, 15 s deadline, 2 concurrent parses and a container memory limit (slice 2) versus child process with RLIMIT (slice 3, 4 days). Evidence: `lopdf` 0.42.0 `decompress_zlib` has no output cap. Recommend ship slice 2, keep `DOC_ROUTING=shadow/skip` as default until slice 3 lands, then `on`. **Q2.** HTTP 415 `UNSUPPORTED_MEDIA` (non-retryable, counted in `pages_failed`) versus 200 with empty body: recommend 415. **Q3.** lenient plus warnings for two releases, then strict. **Q4.** scrape-record retention follows artifacts, armed by `QUARRY_RETENTION_DAYS`; accept. **Decider.** The user. **Blocking?** W1: Q1 blocks the production default of `DOC_ROUTING=on`; Q2 should be answered before slice 7; Q3/Q4 non-blocking.

### 1C. LATER (does not block W1 or W2 building)

#### ADR-2 / OQ-7 Secret custody for browser credential use
- **Options.** Extend integration-corev2 (an at-rest token vault with a shared internal API key, not a user-secret store) versus a Control Plane secret service. **Recommendation.** OTP first through the existing sensitive-approval path; defer password references until the ADR (also D13 D17 keeps a heuristic refusal of credential entry in `write`). Every OTP read should be approved and shown with origin and connection. **Gates.** F-t2-024 E3/E5 (W3). **Proceeds without.** WS12 in full (it is the prerequisite: redaction precedes any `fill_secret`). **Decider.** The user plus Control Plane owner. **Blocking?** No, W3.

#### ADR-6 / OQ-9 Script execution model (evaluate, scrape action lists, code mode, generated extractors)
- **Options.** Keep one governed action per call; or allow declarative capped action lists and a sandboxed runtime whose every binding is a separate Quarry step. D2 D1 chose reject outright (broker has no script-hash scope; `/step` discards the return value `observation.rs:542-544`) over allowlist plus approval; D13 ships a closed, declarative, capped action list for user principals only with no `evaluate` in its wire enum.
- **Recommendation.** Declarative lists and profiles fit "Model proposes, Quarry executes"; code mode (F-t2-063) only as a slice of the generic code-mode already planned in Model Plane; Python-over-RPC is not the first engine (OQ-9); a future read-only evaluator is a NEW action variant, never a relaxed `Evaluate`. Record D2 D1 as the ledger default now. **Gates.** WS13 slice 5 opening to the Model (`scrape:interact` granted to no principal), F-t2-063, F-t2-065. **Proceeds without.** WS2 S1, WS13 slices 1 to 4, 6 to 8. **Decider.** The user. **Blocking?** No.

#### ADR-7 / OQ-6 External MCP exposure
- **Options.** REST only, or an audience-bound MCP front outside Quarry with customer API keys. **Recommendation.** OpenAPI-from-types slice now (WS5 slice 8); everything else waits for an owner decision (reverses `OSS_PARITY_BACKLOG.md:30`). **Gates.** F-t2-064, F-t2-042. **Decider.** The user. **Blocking?** No.

#### ADR-9 Automation owner (completion triggers, agent routines)
- **Recommendation.** Quarry schedules stay acquisition-only; unattended browser runs are draft-only through W2 by owner ruling, and the scheduled lane cannot borrow a human's bearer; agent routines wait for pause/resume (sibling F-t3-018). **Gates.** F-t2-031 ph2, F-t2-061. **Decider.** The user, Application Plane and Model Plane owners. **Blocking?** No.

#### ADR-10 / OQ-8 Browser execution substrate
- **Options.** One Chromium with caps (K=1), a K-process pool (crash containment, no tenant isolation claim), per-session container or microVM (F-t2-060), remote providers (F-t2-059), user's own Chrome (F-t2-077, parked 2026-09-28). Constraint: the pinned egress proxy type is loopback-only (`quarry-browser/src/lib.rs:331-360`); remote or containerised Chromium needs a sidecar proxy or a reviewed relaxation (OQ-8). **Recommendation.** Per-page emulation and sandbox default now (WS3 S0/S2), K=1 with slot bookkeeping (S5), pool (S6) only after a measured memory budget; make no isolation claim from a pool; sidecar proxy decision deferred. **Gates.** WS3 S6 beyond K=1, F-t2-058/059/060, WS6 L1. **Decider.** The user, infrastructure owner. **Blocking?** No.
