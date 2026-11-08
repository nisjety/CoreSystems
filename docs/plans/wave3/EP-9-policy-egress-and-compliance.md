# EP-9 (Wave 3) - Policy, egress and compliance

Abbreviations: `MP` = `apps/Model Plane`; `MPR` = `MP/rust`; `MG` = `MPR/services/model-gateway/src`; `EC` = `MPR/services/execution-core/src`; `SC` = `MPR/services/session-core`; `MPC` = `MPR/crates/mp-contracts`; `CC` = `MP/go/services/capability-core`; `SM` = `MP/go/services/sandbox-manager`; `PROTO` = `MP/proto/model_plane/v1`; `OC` = `apps/Control Plane/org-core`; `UC` = `apps/Control Plane/user-core`; `AC` = `apps/Control Plane/audit-core`; `QC` = `apps/Ingestion Plane/Quarry-v2/crates`; `FE` = `apps/Frontend Plane/verevonv3`; `FES` = `FE/src`; `BFF` = `FE/apps/gateway/src`; `EV` = `docs/compliance/evidence`. Every `path:line` below was opened in worktree `epic-shaw-4d613b` at HEAD `70e67c8d7` (branch `model-plane`, Waves 1 and 2 built). `[INF]` = inference, not read in code. `[verify live]` = only a running stack can prove it.

Plan inputs: `docs/plans/model-plane-chat-build-plan-2026-10-14.md` sections 1.4, 2.1 (DEC-14, DEC-15, DEC-19, DEC-21, DEC-22, DEC-32, DEC-33, DEC-34, DEC-35, DEC-40), 3.3.0, 3.3.2 (EP-9), 3.3.3, 3.3.5, 4.3, 4.4. Audit cards: `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md:932-944` (F-t3-043), `:1076-1086` (F-t3-055), `:1160-1170` (F-t3-062), `:1232-1242` (F-t3-068). The audit is older than the tree; where they differ, code wins and the claim is marked stale in section 2. Sibling Wave 3 designs read for overlap: EP-2, EP-8, EP-10, EP-13, EP-14 and `docs/plans/wave3/00-integration-round-a.md`.

## 1. Scope, items resolved, owner

**Items (all W3, all Recommended, all Partial):** F-t3-055 web access governance and provider transparency (L); F-t3-068 policy and data-flow transparency (L); F-t3-043 sandbox egress tiers and credential-injecting proxy (XL); F-t3-062 enterprise compliance posture (XL). Audit dependencies: F-t3-032 (WS25), F-t3-009 (WS22), F-t3-004 (WS06), F-t3-005 (WS01), F-t3-006 (WS13).

The epic is four separable sub-tracks. Each has its own slices, and no sub-track waits on another except where stated in the slice.

| Track | Item | What it delivers in W3 | Decision-bound? |
|---|---|---|---|
| W (web) | 055 | "Searched via X" line; org web switches enforced in both loops; org domain rules applied to search and fetch, re-checked by Quarry | W2 needs DEC-14 (recorded, see 2.6); W3 needs DEC-19 (recorded) plus a Quarry-plan ask |
| T (transparency) | 068 | Declared egress catalogue with a generated disclosure and a CI check; effective-policy origin read API; the panel | No (T2/T3 consume W2 when it lands) |
| E (egress) | 043 | Per-call enforcement report in tool cards; closed tier enum and an egress entitlement in Control; a real network boundary with an allow-list proxy; shell-grammar rule proposals; credential custody ADR | E3 needs D-E2 (owner ack of a ledger change); E5 needs DEC-21 |
| C (compliance) | 062 | Retention windows and legal holds in Control; a session-core sweeper (dry-run, then delete); hold-versus-erasure integration; data-use label; SHA-pinned CI actions | C4 needs D-C1 (hold vs GDPR erasure, owner + counsel) |

**Owner:**
- Control Plane: org-core owns the retention policy and legal-hold records (C1) and stays the site-policy record (DEC-19, used by W3); user-core owns the Space egress tier and allow-list (E2) and the signed Space capability decision; audit-core receives audit events through the existing outbox and NATS path only (never a DB read).
- Model Plane: capability-core owns the org web switches as keys of the org agent policy registry (DEC-14, W2); model-gateway and execution-core enforce in both loops; sandbox-manager records the permitted tier on the lease; execution-core owns the network boundary and the proxy (E3); session-core owns the sweeper (C2, C3).
- Ingestion Plane: Quarry stays the final web enforcer. W3 asks the Quarry plan for one additive change (accept a request-layer deny list on `/v1/extract`). Model Plane proposes; Quarry executes or rejects.
- Frontend Plane: the "Searched via" line, the admin web section, the enforcement badge, the effective-policy panel, the retention and hold admin section; the BFF aggregates owner-plane reads.
- Support Plane: the audit's plane list puts signed releases there. W3 builds only the CI pinning gate (C6); the signed release itself stays WS01-S12 (blocked-external).
- Data Plane v2: no new code. The C3 sweeper must name every Data Plane store that holds thread-derived content and use the existing Data Plane deletion contract for it (see C3 risk).

| Slice | Track | Tag | Effort | In |
|---|---|---|---|---|
| W1 | 055 | [BUILD] | S | "Searched via X" provenance line from data the gateway already parses |
| W2 | 055 | [BUILD] | M | Org web switches (`web.mode`, `web.paid_providers`) as registry keys, enforced in the inline loop and the Do loop |
| W3 | 055 | [BUILD] | M | Org domain rules (the DEC-19 block list) applied to `fetch_url`/`web_fetch` and to search results, with a Quarry request-layer re-check |
| T1 | 068 | [BUILD] | M | Declared egress catalogue, generated disclosure, CI check |
| T2 | 068 | [BUILD] | M | Effective-policy origin read: Model Plane layers plus BFF aggregation |
| T3 | 068 | [BUILD] | S-M | Effective-policy panel (thread and Space) |
| E1 | 043 | [BUILD] | S | Per-call sandbox enforcement report in `code_interpreter` results and the tool card |
| E2 | 043 | [FINISH]+[BUILD] | S-M | Closed egress tier enum and a Space egress entitlement in Control; `space:egress` no longer granted by default |
| E3 | 043 | [BUILD] | L | Network boundary: unshared netns plus a socket-bridged allow-list proxy; `registries` and `allowlist` tiers |
| E4 | 043 | [BUILD] | M | Shell-grammar "allow always" rule proposals on the WS13-S7 rule path |
| E5 | 043 | [BUILD, decision-gated] | S | Credential custody ADR for the injecting proxy (no code) |
| C1 | 062 | [BUILD] | M | org-core retention policy and legal holds (org, thread), admin and internal APIs, audited |
| C2 | 062 | [BUILD] | M | session-core retention sweeper, dry-run only, audited counts |
| C3 | 062 | [BUILD] | M | Sweeper delete mode, propagation to derived memory, thread workspace and Data Plane |
| C4 | 062 | [BUILD, decision-gated] | S-M | Holds checked on user delete, privacy erase and org erasure, per D-C1 |
| C5 | 062 | [BUILD] | S | Provider data-use label in the model picker |
| C6 | 062 | [BUILD] | S | SHA-pinned GitHub Actions with a ratchet gate |

**First slice:** W1. It needs no decision, no migration and no proto, and the user sees where their search went.

**Explicitly OUT:**
- EP-9 has no W4/W5 members in the plan's epic table. Related later items that are not this epic: F-t3-084 per-Space environment specs (W5, waits on E3 and E5), F-t3-057 custom connectors (W4, EP-10, waits on E3 and E5), F-t3-046 share links (EP-12, DEC-40 share model).
- Parts of F-t3-062 that are not sliced for W3, with the reason: **share links** (EP-12 owns F-t3-046; the audit's "org-only default" is carried there); **signed release pipeline** (WS01-S12 is blocked-external on production keys; C6 is the only code piece that needs no keys); **regime tags** (needs D-C3, an owner vocabulary; deferred to W4, and C1's schema keeps no regime field so nothing has to be undone); **typed telemetry registry** (follows WS26-S1, whose live half is blocked-external; deferred to W4).
- Parts of F-t3-043: the injecting proxy itself (043(c)) is W4 after E5; versioned writes belong to F-t3-030.
- Parts of F-t3-055: **cached-only mode** needs a Quarry mode that does not exist (no `cached_only`/`cache_only` in `QC/quarry-edge/src/search_routes.rs`); the W2 key is defined as `reserved` until the Quarry plan ships it. **Per-domain consent prompts** for fetch (audit: "only on Do runs") stay with WS21's site consent card; W3 only allows or denies. **Provider pin** is reserved (no router hook in the request body today).
- Shared-Space sandbox and egress decisions: `IssueSpaceCapabilityDecision` requires a personal Space (`UC/internal/spaces/space_capability_decision.go:138`, `personal_thread_decision.go:115-117`). Fixing that is the Control "shared-Space decision family" epic (overlap-space D8), not this epic. E2/E3 apply to personal-Space runs only.
- Personal runs with no Space capability decision (the per-call ephemeral path) keep `Disabled` network in W3. Giving them egress needs an org-level entitlement design (open question Q-E4).
- Channel Plane.

## 2. Verified current state

### 2.1 Web governance (055)

- Inline loop web availability: `web_search` is offered when the scope is not `KnowledgeOnly` and the toggle, the staleness heuristic, `WebOnly` or deep research asks for it (`MG/sse.rs:3043-3065`); the builtin loop skips only `web_search` when it is not available (`MG/sse.rs:3107-3110`). `fetch_url` is a builtin (`MG/tool_loop.rs:5439` inside `builtin_tool_defs`, `:5360`) and is removed only by the scope filter (`WEB_CLASS_TOOLS`, `MG/tool_loop.rs:5288`; `tool_allowed_for_retrieval_scope`, `:5316-5328`). So with the toggle off, `fetch_url` is still offered. There is no org input anywhere in this decision.
- Paid providers: `paid_providers_allowed` returns false under ZDR and true only for listed model aliases (`MG/tool_loop.rs:1724-1730`); stamped last so no argument can change it (`:1782-1821`).
- Do loop: `web_search` and `web_fetch` are always in the Do tool list (`EC/runtime_loop/agent.rs:6885-6894`), dispatched at `EC/runtime_loop/mod.rs:1337`. The Quarry body is `{query, limit, zdr}` only (`EC/web_tools.rs:254-256`), extract is `{urls, zdr}` (`:259-261`); paid providers are absent by decision (`:247-253`). Audit claim confirmed.
- Org policy plumbing that exists: capability-core is the org agent policy record (WS25-S7, done); model-gateway reads it with a 30 s TTL and bounded stale serving and refuses a Do run with `org_policy_unavailable` when it cannot read it (`MG/org_agent_policy.rs:1-15`, `:29-38`); the schema is a closed key list with `reserved`/`enforced` status (`MP/api/contracts/org-agent-policy-schema.json`, keys `mode_cap` .. `reference_uploads`). Do runs receive an `OrgPolicyContext` (`PROTO/execution.proto:277-284`, fields 1-3; next free **4**), carried as `RunAgentRequest.org_policy = 24` (`:271`).
- Site policy of record: org-core `browsersites` (WS21-S3, partial) stores mode `open|allowlist`, per-host `block|read|act` and member preferences (`OC/internal/browsersites/types.go:1-80`, `OC/migrations/023_org_browser_sites.up.sql`), with an internal resolve route held by the broker alone (`OC/internal/http/server.go:237-239`, scope `org:browser-site:read:any`).
- Quarry: it has a full web-policy lattice (`QC/quarry-core/src/web_policy/mod.rs:1-16`; layers invariant, ZDR, org, grant, baseline, request at `:490-511`; org input `Absent|Present{layer,rev}|Unreadable`, `:465-472`). The org layer is read from a `web_policy` token claim (`QC/quarry-edge/src/policy_baseline.rs:145-160`; claim parsing `QC/quarry-edge/src/auth.rs:92-96`, `:239-245`). `git grep web_policy` over Control Plane, Model Plane and the BFF finds no issuer of that claim. `/v1/search` accepts `exclude_domains` (`QC/quarry-edge/src/search_routes.rs:~113-118`) and returns `provider` and `policy_downgrades` (`:160-181`); `/v1/extract` has no domain or policy field (`QC/quarry-edge/src/extract_routes.rs:39-57`).
- Provenance data already parsed: each hit carries `source`, the provider that served it (`MG/quarry.rs:310-316`, filled from `provider` at `:1186-1194`); the reply struct drops the envelope provider (`MG/quarry.rs:410-415`). Provider ids in Quarry: `tantivy_local`, `brave`, `serper`, `searxng`, `stract`, plus composite labels `hybrid`, `reranked`, `fallback`, `smart_router` (`QC/quarry-runtime/src/local_index.rs:701`, `serp.rs:456,572,669,1091`, `hybrid.rs:104`, `rerank.rs:861`, `serp.rs:367`, `smart_router.rs:1081`).
- Disclosure frames today: `SourceScopeApplied` (`MG/sse_events.rs:263-275`) and `Usage.served_by` for the model (`:302-316`, `ServedBy` at `:45`). No search-provider frame exists. FE has `FES/features/chat/components/SourceScopeNotice.tsx` (with test) as the natural host.

**Stale audit claims (055):** (1) "Quarry's host list is a security feed, not org-configurable": stale in code, true in effect. Quarry has an org layer, but nothing mints its claim. (2) "No router decision or disclosure exists": partly stale. The serving provider is parsed per hit but not shown.

### 2.2 Transparency (068)

- Origin views that exist per owner: org-core explain routes for model entitlement and browser sites (`OC/internal/http/server.go:134`, `:141`); capability-core effective agent policy through `GET /v1/org/agent-policy/effective` (`MG/http_routes.rs:525-528`) and BFF `/api/v1/org/agent-policy/effective` (`BFF/domains/agent_policy.rs:45`). The posture ratchet is in `MG/profile.rs:1-19`; the ZDR one-way raise is `BFF/domains/chat/shared.rs:94-131` (`apply_org_zdr_posture` at `:110`). No route aggregates these with an origin per value.
- Egress inventory that exists: `EV/tools/scan-egress.mjs` (1866 lines) derives outbound hosts from code with a built-in vendor catalogue, `--strict`, `--summary`, `--verify-doc` (`:1-50`); `EV/subprocessors-from-code.md` is its register (engineering evidence, not counsel-reviewed, `:3`); `FE/apps/verevon-web/src/components/trust/trust-recipients.ts:1-30` lists excluded vendors and is checked by `trust-data.test.ts` against the scan. No workflow in `.github/workflows` invokes `scan-egress` (grep, zero hits). The catalogue records vendor, host, gating env and class; it does not record the contacting process per call path, the data class sent, or the ZDR behaviour.

**Stale audit claims (068):** (3) "no egress catalogue": partly stale (the scan and register exist; the declared per-destination catalogue and the CI job do not). (4) "no value-with-origin view": partly stale (three per-owner explain routes exist; no aggregate and no panel).

### 2.3 Sandbox egress (043)

- Network policy enum: `Disabled | AllowAll | AllowDomains(Vec<String>)` (`EC/policy.rs:23-32`). bwrap unshares the net namespace only for `Disabled` (`EC/sandbox.rs:209-224`). Since the 2026-08-22 ledger entry, the executor refuses `AllowDomains` when no proxy variable is set (`EC/executor.rs:116-123`, `:156-183`); when one is set, the process may still ignore it, so the allow-list is advisory. No caller uses `AllowDomains`: `code_interpreter` always runs `WorkspaceWrite` with `Disabled` (`EC/code_interpreter.rs:706-709`).
- `ExecOutcome` already has `sandboxed` (`EC/executor.rs:185-198`). `CodeResult` does not carry it, nor any enforcement field (`EC/code_interpreter.rs:189-205`, serialised `:728-736`).
- Capability claim: execution-core always claims `egress: "disabled_by_default"`, `credential_mode: "credential_free"` (`EC/sandbox.rs:350-368`) and asks Control for the signed decision at `POST /api/v1/internal/spaces/sandbox-capability-decision` (`EC/capability_client.rs:1-2`, `:21-22`, intent fields `:30-40`).
- Control: `IssueSpaceCapabilityDecision` (`UC/internal/spaces/space_capability_decision.go:131`) requires personal authority (`:138`), a Space `SandboxCapabilityEntitled` floor (`:144`), and then grants `space:egress` for any claim other than `disabled_by_default` (`:156-159`). Egress is a free string with no enum check (`:63-81` validates processes only). ZDR does not disqualify the decision (doc comment `:127-130`).
- sandbox-manager: refuses a non-default egress claim whose decision lacks `space:egress` (`SM/internal/authz/capability_verifier.go:179-181`). It runs no code, so it cannot enforce a network boundary; execution-core does. `processes_permitted` is the precedent for "grant the lease, record the withheld permission, enforce later" (`PROTO/sandboxes.proto:183-194`, `AcquireLeaseResponse` fields 1-6, next free **7**; `AcquireLeaseRequest` 1-8, next free **9**).
- Ledger: "The grant is deliberately NOT symmetric with egress. An egress-capable claim without the entitlement is refused outright" (`MP/docs/decisions/ledger.md:4440-4450`). E3 changes this (D-E2).

**Stale audit claims (043):** (5) "Control grants space:egress ... with no org entitlement": partly stale. A Space sandbox entitlement is required (`:144`); there is no egress-specific entitlement or tier. (6) "a proxy env var is advisory": still true when set; when unset the executor now refuses (`executor.rs:171-181`).

### 2.4 Compliance (062)

- Retention: `org_compliance.data_retention_days` exists (`OC/migrations/002_add_enterprise_fields.up.sql:30-44`) and is a struct field only (`OC/internal/org/types.go:69`); nothing reads it. session-core has no time-based thread deletion; its sweepers are for attention events and the tool ledger (`SC/src/attention_events.rs:247`, `SC/src/tool_ledger.rs:46`). The per-thread purge exists and is reusable: `delete_thread_rows` removes every content-bearing row of a thread and returns the deleted `agent_memory` ids for letta-bridge propagation (`SC/src/grpc.rs:655-700`). Org erasure is `SC/src/gdpr.rs:1-32` (only `subject_type == "organization"` purges).
- Holds: a Control-only Space legal hold exists (`UC/internal/spaces/deletion_authorization.go:25-46`; table `space_legal_holds`, `UC/migrations/022_space_deletion_authority.up.sql:14-18`; routes `UC/internal/http/server.go:357-358`; checked only by Space deletion, `UC/internal/spaces/repository.go:219-223`). No org or thread hold.
- Internal read pattern to copy: `GET /internal/orgs/:orgId/retention-posture` and `/agent-policy` (no-store, own narrow scope, 3 s timeout, 404 vs 500 semantics) (`OC/internal/http/retention_posture_handler.go:15-66`, `agent_policy_handler.go:15-50`, routes `server.go:229-232`).
- Releases: `MP/scripts/release-artifact.sh` signs a manifest with openssl (`:1745-1746`); the only workflow reference is a path filter (`.github/workflows/browser-broker-ci.yml:44,65`). 28 workflows, 177 `uses:` lines, 0 pinned to a 40-hex SHA (count at HEAD; the audit said 107 lines, so the count is stale, the claim holds).
- Provider data-use: `ModelInfo` fields 1-8 (`PROTO/inference.proto:620-629`); EP-13 reserves 9-11 (integration round A, 1.2).

### 2.5 Prerequisites: queue status and open register rows

| WS slice | Queue status | Matters for | Open register rows that matter | Can our slice start? |
|---|---|---|---|---|
| WS25-S7 org agent policy registry | done | W2 | none found | yes |
| WS25-S9 enforcement, policy client | done | W2 | none found | yes |
| WS25-S10 BFF routes | partial | W2 admin route | (W2 adds keys only; the proxy is generic) | yes |
| WS25-S11 Customize hub | partial | W2 UI | none blocking | yes (UI part last) |
| WS25-S12 flag flips | blocked-external | W2 live flip | WS25 switches carry open P1s per the queue note | build yes, flip waits on WS25-S12 proof stack |
| WS21-S3 org-core site policy | partial | W3 | D2-299 (`space_id` unused in grant), D1-751 resolved | yes; W3 adds a model-gateway reader scope |
| WS21-S4 broker reads site policy | partial | W3 (parallel reader) | D2-688 (broker mode default off) | yes (W3 does not use the broker) |
| WS22-S1/S2/S3/S6 thread lease, sync, lifecycle | partial | E3 (thread leases only) | D2-145, D2-147, D2-149 (thread authz, ZDR only at acquire) | E3 starts on the Space lease path; thread leases join after these close |
| WS22-S5 thread workspace erasure | done | C3 | none | yes |
| WS22-S7/S8 SandboxBackend, microVM | optional-deferred | not needed | n/a | E3 does not need them (D-E1) |
| WS13-S7 "Always" rules | partial | E4 | D2-1122 (digest producers), D2-1127 (proposal list-confirm-dismiss chain) | no: E4 waits for these two rows |
| WS23-S3.code tainted egress | partial [INF from register] | E3 high-risk rule | D2-353 (cause still `RiskyEffectUnderAsk`), D2-354 (taint lost on resume), D2-604 (Ask loop) | E3 builds; its "high-risk run" check is only as good as the taint, stated in E3 risks |
| WS20-S6 scheduled egress | partial [INF] | W2 | D2-321 (search query egress, owner decision) | yes; W2 gives orgs an off switch, it does not settle D2-321 |
| WS06-S3/S5/S8 retention-safe chain, compliance API, SIEM | partial | C1 audit rows, T1 rows | D1-186 (SIEM outbox survives erasure), D1-429/D1-431 (siem_export entitlement, counsel register class), D1-399 (details guard) | yes; C1 writes audit rows through the existing outbox only |
| WS06-S7 SIEM destinations | done | T1 catalogue row | D1-431 | yes |
| WS01-S11 ops gates | done | C6 | none | yes |
| WS01-S12 signed baseline | blocked-external (code half partial) | C6 note | external keys | C6 yes; signing claims no |
| WS01-S13/S14 ZDR attestation, trust wording | blocked-external | T1 publishing, C5 wording | counsel wording | T1/C5 build; nothing is published as a public claim before counsel |
| WS10-S2/S4 model entitlement | partial | T2 (explain read) | D1-523 (live DoD) | yes |
| WS12-S9 plane-boundary gate | partial | T1 lane | none blocking | yes |

### 2.6 Recorded rulings

- DEC-14 (capability-core is the org agent policy record) is recorded in the ledger hand-off for WS25-S0 (`MP/docs/decisions/ledger.md:264`). DEC-19 (layered browser policy, org-core record) is recorded in the WS21-S0 hand-off (`:250`). Both are lead hand-off texts; whether the entry bodies were inserted above the 2026-10-21 line was not checked [verify].
- DEC-21 (custody) has no ledger entry. DEC-22 is recorded as decided at defaults with switches off (`:266`).
- 2026-08-22: AllowDomains fail-closed fix (`ledger.md:2015-2019`). S4.2 process registry: egress refusal asymmetry (`:4217-4225`, `:4440-4450`).

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates | Proceed without? |
|---|---|---|---|---|---|
| D-W1 | Where the org web switches live | (a) org-core new record (audit card); (b) capability-core registry keys (DEC-14); (c) switches in capability-core, host lists in org-core | **(c).** On/off agent capability switches go to the DEC-14 store: the gateway already reads it fail-closed with bounded staleness and passes it to Do runs, so W2 needs no new store, client or migration. Host lists stay in the DEC-19 store, so there is one domain list of record, not two. This deviates from the card's "stored in org-core" and from the plan's [RECONCILE] note ("org-core now, registry later"); it lands the plan's end state directly. New ledger entry | W2, W3 | W1, T*, E*, C* yes |
| D-W2 | How Quarry learns the org's domain rules | (a) mint the `web_policy` token claim (Quarry WS7.J); (b) send the rules per call as a request layer; (c) Quarry pulls org-core | **(b) for W3.** A request layer can only tighten (lattice `meet`, `QC/quarry-core/src/web_policy/mod.rs:3-10`), so Quarry can apply it without trusting the caller to loosen anything. (a) contradicts DEC-19 "no bearer claim" and puts policy in a 300 s token. (c) is the long-term DEC-19 shape and belongs to the Quarry plan. Ask the Quarry plan for `exclude_domains`-style input on `/v1/extract` and to echo `policy_rev` | W3 Quarry half only | W3 Model half yes |
| D-W3 | Unknown or unreadable web policy | fail-open / last-known / fail-closed | Follow `org_agent_policy.rs` exactly: inline Ask turns keep the floor they had (web follows the toggle), Do runs refuse with `org_policy_unavailable` past the stale bound. Web off is never inferred from an outage on Ask (that would silently break answers); Do keeps its existing refusal | W2 | yes (default) |
| D-E1 | Network boundary mechanism | (a) netns plus nftables per call; (b) unshared netns plus a socket-bridged proxy; (c) wait for the WS22-S8 microVM | **(b).** bwrap already unshares the netns; the only path out is a Unix socket bind-mounted into the sandbox and bridged to `127.0.0.1:3128` inside it, with the allow-list enforced by a proxy outside. No root, no firewall rules, works on the current bwrap host, and needs no SandboxBackend trait. (a) needs CAP_NET_ADMIN in the container [INF]. (c) is not scheduled. Reference design: Anthropic's open sandbox runtime uses the same bwrap-plus-socket pattern [INF from public docs, not read here] | E3 | E1, E2 yes |
| D-E2 | What happens when a backend claims egress for a Space with no egress entitlement | keep "refuse the lease" (ledger :4440) / grant the lease at tier `off` and record the permitted tier (processes pattern) | **Grant at `off` and record.** Once E3 lands, every execution-core will claim the tier it can enforce. Keeping the refusal would break ordinary `code_interpreter` in every Space without the entitlement, the same trap the ledger names for processes. Enforcement moves to the per-call boundary, which is now real. Changes a recorded ledger rule, so needs owner acknowledgement | E3 | E1, E2 yes |
| D-E3 | Which tiers exist in W3 | off, registries, allowlist, all | Closed enum `off | registries | allowlist | all`; unknown means `off`. `all` is in the vocabulary but not storable in W3 (Control refuses it). Any tier above `off` is refused for ZDR runs, waived runs (DEC-32), plan-mode runs and runs with a content-safety taint | E2, E3 | yes (default) |
| D-E4 | Credential custody for the injecting proxy | Control secret store (card) / integration-corev2 custody (DEC-21) / Quarry vault | **integration-corev2 custody**, proxy reads a sealed reference and injects per host; the sandbox sees a sentinel. One ADR also covering F-t2-024 and F-t3-057. Decided by DEC-21 owners | E5 (and W4 043(c)) | everything else yes |
| D-C1 | Legal hold versus GDPR erasure | erasure always wins / hold always wins / hold blocks erasure of held scope with a typed refusal and an audited override | Recommendation for the owner and counsel: a hold blocks automatic retention deletion always; it blocks user and org erasure of the held scope with the typed refusal `erasure_blocked_legal_hold`, an audit row, and a Control-side override that releases the hold first. Basis [INF, not legal advice]: GDPR Art. 17(3)(e) exempts data needed for legal claims. Until decided, today's behaviour stays (erasure wins) and C4 does not merge | C4 | C1-C3 yes |
| D-C2 | Retention clock | from creation / from last activity | **Last activity** of the thread (latest message or run end). A long-running thread is not deleted while in use. Window bounds 30..3650 days; unset means keep (today's behaviour) | C1-C3 | yes (default) |
| D-C3 | Regime tags vocabulary | none / closed list (gdpr, hipaa, ...) | Defer to W4. Do not reuse `org_compliance.hipaa_compliant` style booleans as enforcement inputs | not sliced | yes |
| D-C4 | `org_compliance.data_retention_days` | reuse as the window / ignore | **Ignore and document.** It has no revision, no audit and an unknown writer; C1's record replaces it. A later cleanup drops it | C1 | yes |
| Q-W1 | Should W2 `web.mode=off` also remove `fetch_url` when the person pasted a URL? | yes / no | Yes. Off means no public web egress. The refusal text says the org turned web access off | W2 | yes (default yes) |
| Q-E4 | Egress for personal runs with no Space decision | org entitlement in org-core / never | Open; not W3 | none | yes |
| Q-T1 | Is the generated disclosure published on the trust page? | yes / after counsel | After counsel (WS01-S14 wording); W3 generates it into `docs/compliance/generated/` only | T1 publish | yes |

Ledger entries this epic adds (newest first, `MP/docs/decisions/ledger.md`, CRLF): "Org web switches are agent-policy keys; host lists stay in the site policy" (D-W1, D-W2); "Sandbox egress: tier enum, lease granted at off, boundary is netns plus socket proxy" (D-E1, D-E2, D-E3; amends `:4440-4450`); "Retention is Control policy, session-core sweeps, holds never deleted automatically" (D-C2, D-C4); D-C1 when ruled.

## 4. Slices

Test commands used below. Rust: `cd "apps/Model Plane/rust" && cargo test -q -p <crate> --lib` (two targets fail pre-existing on Windows; use `--lib` and diff failing sets), `cargo fmt --all -- --check`. Go: `go test ./...` per module (`GOWORK=off` for new deps). Proto: `buf lint`, `buf breaking ... --against proto/baseline.binpb` in `MP/proto`, Go bindings regenerated under `MP/go/gen/model_plane/v1/`. SSE: `python3 "MP/scripts/gen-sse-event-taxonomy.py" --check`. ZDR: `bash scripts/tests/zdr-contract-sync.test.sh`, `cargo test -p mp-contracts zdr_refusal` (regenerate with `MP_UPDATE_GOLDEN=1`). FE from `FE`: `pnpm exec vitest run <files>`, `pnpm lint`, `pnpm exec tsc -p tsconfig.app.json --noEmit` (memory: `tsc -p tsconfig.json` checks nothing). BFF: `cargo test --manifest-path "FE/apps/gateway/Cargo.toml" --all-targets`. Every Go service migration must be idempotent (re-run at boot). New flags default off; vocabularies named per `.plan-work/common/flags-and-config.md`.

### Track W - web governance (F-t3-055)

#### W1 - "Searched via X" provenance line [BUILD]
**Goal.** After a turn that searched the web, the answer shows one line: which provider(s) served the results and why (free chain, paid allowed, ZDR free-only). No Quarry change.
**Files.**
- `MG/quarry.rs` - `SearchReply` gains `provider: String` (envelope `provider`, `:410-415`); keep per-hit `source`.
- `MG/search_provenance.rs` NEW - pure `SearchProvenance { providers: Vec<ProviderUse{id, label, hits}>, route: Route }` where `Route = FreeChain | PaidAllowed | ZdrFreeOnly`; closed id-to-label table for the ids verified in 2.1 (`tantivy_local` "Verevon index", `brave` "Brave Search", `serper` "Serper (Google results)", `searxng` "SearXNG metasearch", `stract` "Stract"; composite labels map to their parts or to "Verevon search"); an unknown id renders as "other provider", never the raw id. Accumulates across all `web_search` calls of a turn.
- `MG/tool_loop.rs` - the `web_search` arm (`:3633-3721`) records into the turn accumulator; route from `allow_paid_providers` and the turn's ZDR.
- `MG/sse_events.rs` - new event `SearchProvenance { providers, route }`, emitted once per turn after the loop when at least one search ran; taxonomy row regenerated.
- `FES/shared/api/chat-client.ts`, `chat-normalizers.ts`, `chat-types.ts` - parse `search_provenance`.
- `FES/features/chat/components/SourceScopeNotice.tsx` - render "Søkt via Brave Search, Verevon-indeks" / "Searched via ..." with a short reason line.
- `FES/shared/i18n` - nb and en strings.
**Contract/proto/migration.** SSE event only (taxonomy gate); no proto, no migration.
**Flag.** `MODEL_GATEWAY_SEARCH_PROVENANCE` (model-gateway; vocabulary C, literal `true`; default off). FE renders only when the frame arrives (no VITE flag).
**ZDR.** No store. The frame names providers only, no query or URL. Under ZDR the route is `ZdrFreeOnly`. Waived: same as non-ZDR (nothing is stored).
**Tests.** `cargo test -q -p model-gateway --lib search_provenance` (label table covers every verified id; unknown id never echoed; accumulation over two calls; route under ZDR); event serialisation golden in `sse_events.rs` tests; taxonomy `--check`; FE `SourceScopeNotice.test.tsx` (one provider, two providers, unknown, ZDR reason), `chat-normalizers.test.ts`.
**Live proof.** Rebuilt model-gateway and SPA on a second dev server; one Norwegian time-sensitive question; capture the SSE transcript with `search_provenance` and a screenshot of the line. Then the flag flips on in compose.
**Effort.** S, 2-3 d. **Deps.** none. **Risks.** Hot files `tool_loop.rs`, `sse_events.rs` (EP-2 S2/S3 also edit the web arm; land W1 first or rebase); the label table can go stale when Quarry adds a provider (unknown renders generically, and T1's catalogue check lists Quarry providers).

#### W2 - Org web switches enforced in both loops [BUILD]
**Goal.** An org admin can turn public web access off for agents and chat, and can forbid paid search providers. Both loops honour it. Quarry is not trusted with this decision; the tool is simply not offered.
**Files.**
- `MP/api/contracts/org-agent-policy-schema.json` (+ `.sha256`) - add `web.mode` (enum, strictness `off`, `cached_only`, `live`; default `live`; fail-closed `off`; status `enforced` for `off|live`, `cached_only` refused as `not_enforced` until Quarry ships it) and `web.paid_providers` (enum `off`, `on`; default `on`; fail-closed `off`; `enforced`).
- `MPC/src/org_agent_policy.rs` and `CC/internal/api/org_agent_policy.go` - each consumer's table gains the two keys (each tests its table against the JSON, per the schema description).
- `MG/org_agent_policy.rs` - `web_caps(org) -> WebCaps{mode, paid}` from the cached document, same TTL/stale rule; `shadow` logs only.
- `MG/sse.rs` - `web_search_available` (`:3059-3065`) also requires `WebCaps.mode == live`; when off, also drop `fetch_url`; emit a `SourceScopeApplied`-style notice with `excluded_tools` and reason `org_web_off` so the person sees why. `paid_providers_allowed` call site (`MG/tool_loop.rs:7746`) ANDs `WebCaps.paid`.
- `PROTO/execution.proto` - `OrgPolicyContext.web = 4` (message `WebPolicyContext { string mode = 1; bool paid_providers = 2; int64 revision = 3; }`, commented, additive). Gateway fills it for Do runs.
- `EC/runtime_loop/agent.rs` - when `org_policy.web.mode == "off"`, `web_search`, `web.search`, `web_fetch` and the `web.read` alias are not offered (`:6885-6894`, alias list `:6279`); `EC/runtime_loop/mod.rs:1337` refuses a call that arrives anyway with a typed tool error `org_web_off`.
- `FES/features/customize/` - a "Web access" block in the org policy section (two selects), behind the existing Customize probe; action contract entries `org.set_web_policy` in `FES/shared/actions/action-registry.ts` (+ test) before wiring.
**Flag.** No new env switch. Enforcement follows `ORG_AGENT_POLICY_MODE` (`off|shadow|enforce`, unknown = enforce, K3-26). The keys are writable only after the schema flip in the same PR; with `ORG_AGENT_POLICY_MODE=off` nothing changes.
**ZDR.** No new store (keys live in the existing JSONB document). Web off removes egress, so it only tightens. ZDR turns already refuse paid providers.
**Tests.** mp-contracts schema-table test; `go test ./internal/api/...` in CC (write `off`, write `cached_only` refused `not_enforced`, write junk refused); `cargo test -q -p model-gateway --lib org_agent_policy` (caps under fresh, stale-restrictive, stale-permissive, unreadable); gateway test in `MG/tests/org_policy_enforcement_test.rs`: org off removes `web_search` and `fetch_url` and emits the notice; paid off forces `allow_paid_providers=false`; `cargo test -q -p execution-core --lib` new `org_web_off_offers_no_web_tools`, `org_web_off_refuses_a_forged_call`; buf lint/breaking; FE section and action tests.
**Live proof.** Proof stack with `ORG_AGENT_POLICY_MODE=enforce`: set `web.mode=off` through the BFF; an Ask turn with the toggle on gets no web call and shows the notice; a Do run offers no web tool (EC log of the offered set); flip back. Capture BFF PUT/GET JSON, both SSE transcripts, CC row. [verify live]
**Effort.** M, 4-6 d. **Deps.** D-W1; WS25-S7/S9 done. **Risks.** `sse.rs` and `agent.rs` hot files; the schema file and its sha are shared with EP-2 S8 (one new key there): append-only, rebase.

#### W3 - Org domain rules on fetch and search, Quarry re-check [BUILD]
**Goal.** The org's blocked hosts (and allowlist mode) from the DEC-19 site policy apply to `fetch_url`/`web_fetch` and to search results, in both loops; Quarry also receives the rule as a request layer it can only tighten.
**Files.**
- `apps/Control Plane/config/plane-service-principals.json` - grant `org:browser-site:read:any` to model-gateway and execution-core service principals (today held by browser-broker alone, `OC/internal/http/server.go:237-239`). Control change, CP acknowledges.
- `MPC/src/site_policy.rs` NEW - pure host canonicaliser (same rules as `MP/api/contracts/browser-domain-canonical.json`) and `fetch_allowed(effective) -> bool` (`deny` blocks; `ask|read|act` allow, because a fetch is a read and W3 adds no prompt).
- `MG/site_policy_client.rs` NEW and `EC/site_policy_client.rs` NEW - call `GET /internal/orgs/:orgId/browser-sites/resolve?host=..&user_id=` (≤32 hosts per call, `MaxResolveHosts`), 30 s cache keyed by (org, host, revision), unreadable = deny for the hosts in question (fail closed on fetch, which is the effectful read).
- `MG/tool_loop.rs` - `fetch_url` arm (`:3772`) checks the host first, typed tool error `org_site_blocked` naming the host; `web_search` arm passes the org's block list as `exclude_domains` (bounded 64 entries, the Quarry layer cap `MAX_RULE_ENTRIES`) and drops returned hits whose host resolves to deny.
- `EC/web_tools.rs` - `search_body` and `extract_body` gain the same inputs; the fetch path checks the host before calling.
- `PROTO/execution.proto` - `WebPolicyContext.site_policy_revision = 4` (so a Do run's checks are logged against a revision).
- Quarry-plan ask (not built here): accept a request-layer deny list on `/v1/extract` and echo `policy_rev`. Until it ships, Model-side checks are the enforcement and Quarry's re-check covers search only (via `exclude_domains`).
**Flag.** `WEB_DOMAIN_POLICY_MODE` (model-gateway and execution-core, own env each; `off|shadow|enforce`; unknown = enforce, K3-26; default off).
**ZDR.** No new store. Hosts are sent to org-core (Control) as today's broker does; no URL path or query leaves (canonical host only). Under ZDR the same checks apply.
**Tests.** mp-contracts canonicaliser golden against `browser-domain-canonical.json`; client tests (cache, unreadable denies, 32-host batching); MG: blocked host fetch refused, search hits filtered, `exclude_domains` sent; EC: same; shadow mode logs and allows.
**Live proof.** Add `example.org` as `block` through the browser-sites admin route; an Ask turn asked to read it gets `org_site_blocked`; a search whose top hit is on that host drops it; Quarry request log shows `exclude_domains`. [verify live]
**Effort.** M, 5-7 d. **Deps.** D-W1, D-W2, WS21-S3 (partial; its resolve route is built). **Risks.** One more caller of an org-core internal route (latency; cache mitigates); open-mode orgs with no rules pay one resolve per new host (batch per turn); Quarry half depends on another session.

### Track T - policy and data-flow transparency (F-t3-068)

#### T1 - Declared egress catalogue, generated disclosure, CI check [BUILD]
**Goal.** Every outbound destination is declared once, with the process that contacts it, what data class leaves, the switch that turns it off and what ZDR does; a generated table is diffed in CI so a new egress cannot arrive unnoticed.
**Files.**
- `docs/compliance/egress-catalogue.json` NEW - entries `{id, vendor_id (scan-egress vendor), plane, contacting_service, hosts[], data_classes[] (prompt, query_terms, document, metadata, none), toggle_env[], zdr ("refused" | "no_retention_attested" | "unchanged_no_content"), region, notes}`. Seeded from the scan's vendor list (58 vendors in `EV/egress-scan-summary-2026-10-01.txt`) plus the Quarry providers from 2.1.
- `EV/tools/scan-egress.mjs` - `--catalogue` already accepts a JSON array (header `:41-44`); add `--declared FILE` that fails `--strict` when a scanned vendor has no declared entry or a declared entry has no evidence.
- `scripts/gates/gen-egress-disclosure.mjs` NEW - writes `docs/compliance/generated/egress-disclosure.md` from the catalogue; `--check` fails when the file differs.
- `.github/workflows/repo-gates.yml` - job `egress-catalogue` (node only, no stack): scan `--strict --declared`, generator `--check`.
**Flag.** None. The job lands non-required for one week, then becomes required (owner flips the branch rule).
**ZDR.** No runtime change. The catalogue states ZDR behaviour; each row's `zdr` value must cite a test or a matrix row id in `notes` (reviewed, not machine-checked).
**Tests.** `node --test EV/tools/scan-egress.test.mjs` (new cases: undeclared vendor fails, stale declared entry fails), generator unit test, a deliberate new URL literal in a fixture turns the job red.
**Live proof.** Not a stack proof: two CI runs, one green, one red on a fixture branch; evidence = links.
**Effort.** M, 4-5 d. **Deps.** none (WS12 lane). **Risks.** The disclosure is a public claim: it lives in `docs/compliance/generated/` and is not copied to the trust page before counsel (Q-T1, D1-431). Must not print env values (the scan already prints set/empty only).

#### T2 - Effective-policy origin read [BUILD]
**Goal.** One read returns, for a thread or a Space, the effective value of approval posture, ZDR, privacy-tier floor, model access, routing, budgets, web access and browser site mode, each with the layer that set it and that layer's revision. Each owner plane answers for its own layers; the BFF aggregates. No cross-plane DB read.
**Files.**
- `MG/policy_effective.rs` NEW, route `GET /v1/policy/effective?thread_id=` in `MG/http_routes.rs` (append at the end of the router) - Model Plane layers only: posture floor and its source (`MG/profile.rs`, agent mode resolve), privacy-tier floor, routing (route note source), run budget/ceiling, web caps (W2) with the capability-core revision. Shape `{data:{items:[{key, value, origin: "platform"|"org"|"space"|"user"|"request", revision?, locked: bool}]}}`.
- `BFF/domains/policy_effective.rs` NEW - `GET /api/v1/policy/effective?thread_id=|space_id=`: fans out to MG (above), capability-core effective agent policy (existing proxy), org-core model-entitlement explain and browser-sites mode, the org ZDR posture as the BFF already resolves it (`chat/shared.rs:110`). Member view: values and origin labels only; admin-only details (hook rule bodies, block-list contents, SIEM destinations) become counts. Other-tenant ids are never echoed (org comes from the verified token only).
- `FES/shared/api/policy-effective-client.ts` NEW; action contract `policy.read_effective` in `action-registry.ts` (+ test).
**Flag.** `MODEL_GATEWAY_POLICY_EFFECTIVE` (vocabulary C, default off; route 404 while off); `POLICY_EFFECTIVE_VIEW_ENABLED` (BFF, strict true/false, default false).
**ZDR.** Read-only, no store. A ZDR caller may read (it is configuration, not content); the read itself shows `zdr: on, origin: org|request`.
**Tests.** MG unit tests per layer source; BFF tests: member vs admin redaction, one owner timing out gives that item `unavailable` (never a guessed value), other-org id in the query refused; `cargo test --manifest-path ... --all-targets`.
**Live proof.** Admin and member bearers on the same Space; capture both JSON bodies; set the org mode cap to `read_only` and see origin `org` and the new revision. [verify live]
**Effort.** M, 4-6 d. **Deps.** none to build; W2 for the web item (absent until W2 lands). **Risks.** Leaking admin-only policy to members (redaction tests are the guard); one slow owner must not stall the panel (per-owner 2 s timeout).

#### T3 - Effective-policy panel [BUILD]
**Goal.** Thread info and Space settings show the T2 list: value, where it came from, a lock icon when a higher layer fixed it.
**Files.** `FES/features/chat/components/EffectivePolicyPanel.tsx` NEW (+ test), mounted from the thread info area and from `FES/features/settings/components/` Space settings; strings nb/en.
**Flag.** Follows T2's BFF switch (the panel hides on 404).
**ZDR.** None.
**Tests.** vitest: renders each origin, `unavailable` item, member redaction, keyboard and 375 px layout.
**Live proof.** Screenshots at 1280 and 375 px, member and admin, on a second dev server (memory: `:5173` serves the main checkout).
**Effort.** S-M, 3-4 d. **Deps.** T2. **Risks.** Copy that over-claims; each label says "set by" only for origins T2 returns.

### Track E - sandbox egress (F-t3-043)

#### E1 - Per-call enforcement report [BUILD]
**Goal.** Every `code_interpreter` result says whether it ran sandboxed, with what network, and whether enforcement was full; the tool card shows a badge.
**Files.**
- `EC/code_interpreter.rs` - `CodeResult` (`:189-205`) gains `sandbox: SandboxReport { sandboxed: bool, network: "off" | "<tier>", enforcement: "full" | "partial" }`, filled from `ExecOutcome.sandboxed` and the policy actually used (`:706-709`). `partial` is reserved for E3's proxy path if a check cannot be confirmed; today every run is `full`/`off`.
- `MPC/src/sandbox_report.rs` NEW - the closed vocabulary, shared by EC and the FE golden.
- `FES/shared/chat-nodes/tool-presentation.ts` - badge "Isolert, uten nett" / "Isolated, no network"; unknown values render as "unknown", never as isolated.
**Flag.** `CODE_INTERPRETER_ENFORCEMENT_REPORT` (execution-core, vocabulary C, default off; off = result bytes unchanged, as the Office verifier fields do).
**ZDR.** No store change: the report is part of the tool result, which follows the existing step-record rules (non-ZDR truncated output, ZDR nothing).
**Tests.** EC unit: report fields for the Disabled path, serialisation omits the key when off; FE `tool-presentation.test.ts`.
**Live proof.** One Do run with `code_interpreter`; capture the tool result JSON and the card screenshot.
**Effort.** S, 2 d. **Deps.** none. **Risks.** none beyond hot file `code_interpreter.rs`.

#### E2 - Egress tier enum and Space egress entitlement in Control [FINISH]+[BUILD]
**Goal.** `space:egress` is no longer granted to any non-default claim. Control knows a closed tier vocabulary, a Space ceiling (default `off`) and a bounded allow-list, and signs only what the Space allows. No behaviour change today, because every backend still claims `disabled_by_default`.
**Files.**
- `UC/migrations/044_space_sandbox_egress.up.sql` (+ `.down.sql`; head is `032_space_browser_entitlement`): `ALTER TABLE space_effect_policies ADD COLUMN IF NOT EXISTS sandbox_egress_tier TEXT NOT NULL DEFAULT 'off' CHECK (sandbox_egress_tier IN ('off','registries','allowlist'))`, `ADD COLUMN IF NOT EXISTS sandbox_egress_allowlist JSONB NOT NULL DEFAULT '[]'::jsonb` (≤32 canonical hosts, checked by the service).
- `UC/internal/spaces/space_capability_decision.go` - `Egress` validated against `off | registries | allowlist | all` plus the legacy `disabled_by_default` (= `off`); permission becomes `space:egress:<tier>` for `min(claim, Space ceiling)` and only when not ZDR (`evidence.Privacy.ZeroDataRetention`); `space:egress` (bare) is still emitted for `allowlist` and above during one release so the old verifier keeps working [INF: confirm no other reader]. Payload digest gains `egress_allowlist_digest` (length-prefixed like the other fields, `:93-125`); action schema hash bumps to v2 so an old verifier refuses a v2 decision instead of misreading it.
- `UC/internal/spaces/authority.go`, `repository.go` (`:1120`, `:1247-1290`, `:1336`) - load and write the two columns; Space policy writer validates hosts with the canonical rules.
- `UC/internal/http/spaces.go` (`:1126`, `:1153`) - decision response returns the allow-list beside the token so execution-core can configure its proxy (E3).
- `SM/internal/authz/capability_verifier.go:179-181` - accept the tier permissions; unknown tier string = refuse.
**Flag.** None needed for the tightening (no backend claims egress yet). The schema hash bump is the compatibility gate; deploy order user-core, then sandbox-manager, then execution-core.
**ZDR.** ZDR orgs never get an egress permission. The columns hold hostnames only.
**Tests.** `go test ./internal/spaces/...` in UC: legacy claim gets no egress permission; `registries` claim with Space `off` gets none; with Space `allowlist` gets `space:egress:allowlist` plus digest; ZDR evidence gets none; `all` refused at write; SM verifier tests for each tier and an unknown tier.
**Live proof.** Integration test against Postgres (user-core and sandbox-manager), then one decision requested on the dev stack with a forged `registries` claim and the Space at `off`: response has no egress permission. [verify live]
**Effort.** S-M, 3-4 d. **Deps.** none (pull-forward). **Risks.** The decision digest is shared by user-core and sandbox-manager: both must change in one train; the v2 schema hash makes a half-deploy fail closed.

#### E3 - Network boundary and the allow-list proxy [BUILD]
**Goal.** A Space whose ceiling is `registries` or `allowlist` gets real, enforced egress for `code_interpreter`: the sandbox has no network except a socket to a proxy that admits only the tier's hosts. Package installs work; nothing else leaves.
**Why L.** Three services change (execution-core boundary and proxy, sandbox-manager lease record, Control already done in E2), plus a small helper binary in the sandbox image and a ledger change (D-E2).
**Files.**
- `EC/egress_proxy.rs` NEW - in-process HTTP CONNECT proxy per call, listening on a per-call Unix socket in the call's temp dir. Admits `CONNECT host:443` only when host matches the tier list (`registries`: a fixed, reviewed list in `MPC/src/egress_tiers.rs` such as `pypi.org`, `files.pythonhosted.org`, `registry.npmjs.org` [list to be reviewed]; `allowlist`: the Space list from E2). Resolves DNS itself and refuses private, loopback, link-local and metadata addresses (port of Quarry's DNS guard idea, written fresh in EC with its own tests; no cross-plane crate dependency). Counts allowed and denied hosts per call.
- `EC/sandbox.rs` - the argv always adds `--unshare-net` (`:222-224` becomes unconditional for local runs); for a tiered call it bind-mounts the socket read-write at `/run/egress.sock` and starts the bridge.
- `MPR/tools/mp-netbridge/` NEW tiny Rust binary (or `socat` if already in the image; check `EC/../Dockerfile:41-97` [verify]) that listens on `127.0.0.1:3128` inside the netns and forwards to `/run/egress.sock`; `HTTPS_PROXY`/`HTTP_PROXY`/`PIP_*`/`npm_config_proxy` set in the child env.
- `EC/executor.rs` - `require_requested_isolation` (`:156-183`) checks "socket proxy running for this call" instead of reading `HTTPS_PROXY`.
- `EC/code_interpreter.rs` - picks the tier from the lease (`min(lease tier, run tier)`); E1's report gets `network: "<tier>"`, `enforcement: "full"`, `egress: {allowed_hosts, denied_hosts}` counts.
- `EC/sandbox.rs:350-368` - claim `egress` = the highest tier this host can enforce (`EXECUTION_SANDBOX_EGRESS_MAX_TIER`).
- `PROTO/sandboxes.proto` - `AcquireLeaseResponse.egress_tier = 7` (new enum `SandboxEgressTier { EGRESS_TIER_UNSPECIFIED = 0; EGRESS_TIER_OFF = 1; EGRESS_TIER_REGISTRIES = 2; EGRESS_TIER_ALLOWLIST = 3; EGRESS_TIER_ALL = 4; }`, unspecified read as off), commented, additive.
- `SM/migrations/0008_lease_egress_tier.up.sql` (+ down; head `0007_lease_lifecycle`): `leases.egress_tier_permitted TEXT NOT NULL DEFAULT 'off'`. `SM/internal/server/server.go` (near `:181`) - D-E2: grant the lease at the permitted tier, never refuse for a capable claim.
- Run-level refusals in `EC/runtime_loop/agent.rs`: tier forced to `off` for ZDR, waived (DEC-32), plan-mode and tainted runs (WS23 taint flag) and when the org `mode_cap` is `lockdown` or `read_only`.
**Flag.** `EXECUTION_SANDBOX_EGRESS` (execution-core; `off|registries|allowlist`; default off; unknown = off, because this switch grants egress); `SANDBOX_MANAGER_EGRESS_TIERS` (sandbox-manager; strict true; default false; off = today's refuse rule).
**ZDR.** New egress, refused for ZDR and waived runs (golden rows below). No new content store: proxy counts are in the tool result only; the proxy logs host names only, never paths.
**Tests.** EC: proxy admits listed host, refuses unlisted, refuses an IP literal, refuses a name resolving to 10.0.0.1 and 169.254.169.254; argv always unshares net; a Python child with `urllib` to a non-listed host fails; `pip download` against a local fake index through the proxy (test server on the host side) succeeds [INF on test harness]; ZDR/waived/plan/tainted force off. Linux-only tests are `#[cfg(target_os = "linux")]` and run in the CI container (memory: Rust tests in a `rust:1` container). SM: lease granted at off for a capable claim with no permission, tier persisted, response field set. buf lint/breaking.
**Live proof.** Dev stack, personal Space set to `registries`: a Do run installs a small package with pip and imports it; the same run's `curl https://example.org` fails; the tool result shows the counts; a ZDR-org run gets `off`. Capture tool results, EC proxy log (hosts only), lease row. Then the flags flip for one internal org only. [verify live]
**Effort.** L, 8-12 d. **Deps.** E1, E2, D-E1, D-E2; WS22 thread-lease rows (D2-145/147/149) only before thread leases are tiered. **Risks.** (1) Exfiltration through allowed hosts: a CONNECT proxy sees the host, not the path, so data can leave inside a request to an allowed registry; mitigated by refusing tiers for ZDR, waived and tainted runs, by keeping `registries` small and by `all` not being storable; stated as a residual risk in the catalogue. (2) The taint signal is weak until D2-353/354/604 close. (3) bwrap user-namespace availability in the deploy container [verify live].

#### E4 - Shell-grammar "allow always" proposals [BUILD]
**Goal.** When a person approves a shell command, the proposal offered is a parsed, narrow rule (program plus subcommand, for example `git status`), never the raw string with arguments.
**Files.** `MPC/src/shell_rule.rs` NEW (a conservative tokenizer: refuses pipes, `;`, `&&`, redirects, command substitution and env assignments; returns `{program, subcommand}` or "no rule"); the WS13-S7 proposal producer in `EC/runtime_loop/agent.rs` uses it for the shell tool; FE proposal card shows the rule text.
**Flag.** `APPROVAL_SHELL_RULE_PROPOSALS_ENABLED` (default false; refused at startup unless `APPROVAL_RULE_PROPOSALS_ENABLED` is on).
**ZDR.** Rule rows are WS13-S7's store; ZDR rule use is already a WS13-S7 depth item, unchanged here.
**Tests.** tokenizer table (30+ cases including quoting and Norwegian characters); proposal never contains an argument token; no rule for compound commands.
**Live proof.** One Do run, approve `git status`, accept "always", second run runs it without a card.
**Effort.** M, 4-6 d. **Deps.** WS13-S7 rows D2-1122 and D2-1127 closed. **Risks.** Over-broad rules; the tokenizer refuses anything it does not fully understand.

#### E5 - Credential custody ADR [BUILD, decision-gated]
**Goal.** One ADR (number assigned at filing) that settles DEC-21 for the injecting proxy: where the secret lives (recommended integration-corev2), how the proxy obtains a sealed reference per call, the sentinel format, per-host injection, SigV4 re-signing scope, and the audit row. No code.
**Files.** `MP/docs/adr/ADR-<next>-sandbox-credential-injection.md`; ledger entry; capability matrix row.
**ZDR.** States: credential injection is refused for ZDR and waived runs (no secret ever enters a ZDR sandbox path).
**Effort.** S, 1-2 d. **Deps.** D-E4 owners (OWN + QRY + CP). **Risks.** none; it unblocks W4 043(c) and F-t3-057.

### Track C - compliance posture (F-t3-062)

#### C1 - Retention policy and legal holds in Control [BUILD]
**Goal.** An org admin sets a retention window for personal threads and runs; an org owner or compliance role places a hold on the org or on a thread. Model Plane reads both through a narrow, fail-closed internal route.
**Files.**
- `OC/migrations/030_org_retention_policies.up.sql` (+ down; head `024_org_core_app_least_privilege`): `org_retention_policies(org_id PK FK organizations ON DELETE CASCADE, thread_days INT NULL CHECK (thread_days BETWEEN 30 AND 3650), revision BIGINT NOT NULL, updated_by TEXT NOT NULL, updated_at)`; `org_legal_holds(id TEXT PK, org_id FK CASCADE, scope TEXT CHECK (scope IN ('org','thread')), target_ref TEXT NOT NULL, hold_ref TEXT NOT NULL CHECK (btrim(hold_ref) <> ''), created_by, created_at, released_by NULL, released_at NULL)`, partial unique index on active `(org_id, scope, target_ref)`; append-only `org_retention_history`. Explicit grants for `org_core_app` (SELECT, INSERT on history only), with a test, because default privileges once gave every table full rights (D1-751, resolved by 024).
- `OC/internal/retention/` NEW package (`types.go`, `validate.go`, `service.go`, `repository.go`) following `browsersites`.
- `OC/internal/http/retention_handlers.go` NEW: admin `GET/PUT /api/v1/organizations/:id/retention` (owner/admin), `GET/POST /api/v1/organizations/:id/legal-holds`, `POST .../legal-holds/:holdId/release` (owner or a compliance role; [INF] role name to confirm with Control); internal `POST /internal/orgs/:orgId/retention/check` body `{thread_ids: [≤500]}` answering `{window_days, revision, org_held, held_thread_ids}`; no-store; scope `org:retention-policy:read:any` granted to session-core only (`apps/Control Plane/config/plane-service-principals.json`).
- Every write emits an audit event through org-core's existing audit outbox (as the SIEM destination writes do) [INF: confirm event type registry in audit-core accepts it without a migration].
- `BFF/domains/orgs/` retention proxy routes; `FES/features/settings/components/OrgRetentionSection.tsx` NEW; action contracts `org.set_retention`, `org.place_legal_hold`, `org.release_legal_hold` (risk high).
**Flag.** `ORG_RETENTION_POLICY_ENABLED` (org-core, default 0; routes 404 while off).
**ZDR.** The records hold ids, numbers and a hold reference, no content. A thread hold on a thread with a ZDR or waived retention receipt is answered with the typed refusal `legal_hold_nothing_retained` (nothing exists to hold; the person is told so). An org hold on a ZDR org is allowed and holds whatever non-ZDR content exists.
**Tests.** Go unit and `TestControlLifecycleRetention*` Postgres tests (RLS, grants, append-only history, active-hold uniqueness, release); handler tests (member forbidden, 404 when off, check route scope).
**Live proof.** Throwaway Postgres run of the lifecycle tests (memory: the lead's org-core recipe), then the admin flow through the BFF with captured JSON and the audit event in audit-core.
**Effort.** M, 5-7 d. **Deps.** D-C2, D-C4. **Risks.** Two Control services hold holds (Space holds in user-core). C2 sweeps personal threads only, so it needs only org-core in W3.

#### C2 - Retention sweeper, dry-run [BUILD]
**Goal.** session-core finds personal threads past their org's window and reports what it would delete, per org, without deleting.
**Files.**
- `SC/src/retention_sweeper.rs` NEW: enumerate orgs with a policy (org-core `/internal/orgs` plus the check route), select candidate personal threads by last activity older than the window (Space threads and threads with Space ids are excluded in W3), drop held ones from the check answer, write one `retention_sweep_runs` row per org and pass (counts only: candidates, held, skipped_zdr, would_delete), and publish an audit event with the same counts.
- `SC/migrations/0107_retention_sweep_runs.sql` (renumbered from 0095; + `revert/0107_retention_sweep_runs.down.sql`; head `0071_run_question_consume`): `retention_sweep_runs(id, org_id, mode, policy_revision, started_at, finished_at, candidates, held, would_delete, deleted, error_code)`.
- `SC/src/main.rs` - spawn behind the switch; interval default 6 h.
**Flag.** `SESSION_RETENTION_SWEEP` (`off|dry_run|enforce`; default off; an unknown value is treated as `dry_run`, never as enforce, because this switch deletes data. This deviates from K3-26 on purpose and is stated in the ledger entry).
**ZDR.** New store with counts only. ZDR threads have no content rows to find.
**Tests.** real-PG `#[ignore]` tests (`cargo test -p session-core --bin session-core retention_sweep -- --ignored --test-threads=1`): window boundary, held thread kept, Space thread kept, org-core unreachable means no candidates (fail closed), counts row written; unit tests for the switch parser.
**Live proof.** Seed an org with old threads on the dev stack, set a 30-day window, run one dry pass; capture the counts row and the audit event; prove no row was deleted (row counts before/after). [verify live]
**Effort.** M, 4-6 d. **Deps.** C1. **Risks.** Last-activity query cost on large orgs (index on threads `(org_id, updated_at)` checked first [verify]).

#### C3 - Sweeper delete mode and propagation [BUILD]
**Goal.** With `enforce`, the sweeper deletes what the dry run listed, through the same path a user delete uses, and propagates to every derived store.
**Files.**
- `SC/src/retention_sweeper.rs` - per thread: one transaction calling `delete_thread_rows` (`SC/src/grpc.rs:668`), then after commit the letta-bridge propagation (`SC/src/memory_erasure.rs`) and sandbox-manager `DeleteThreadWorkspace` (WS22-S5). Batches of 100, a per-org daily cap (default 10 000 threads) so a misconfigured window cannot wipe an org in one pass; a second check of holds right before each batch.
- Data Plane: inventory first (this slice's first task): which chat-derived content reaches Data Plane v2 (chat attachments promoted to documents [INF]); call the existing Data Plane deletion contract for those document ids, never its database. If no such content exists, record that in the slice notes and the ledger.
**Flag.** Same `SESSION_RETENTION_SWEEP=enforce`; turned on per environment only after one dry-run week whose counts the owner reviewed.
**ZDR.** No new store. Deletion of non-ZDR content only.
**Tests.** real-PG: delete removes every row family `delete_thread_rows` covers (reuse `delete_thread_and_bulk_delete_are_owner_bound_against_real_pg` fixtures, `grpc.rs:15139`), held thread untouched, a hold placed between dry run and delete is honoured, cap respected, letta and sandbox-manager calls made after commit (mocks).
**Live proof.** Dev stack: 3 seeded old threads, one held; enforce pass deletes 2; psql counts before/after; audit event; letta-bridge memory gone. [verify live]
**Effort.** M, 5-7 d. **Deps.** C2 and one reviewed dry-run week. **Risks.** Irreversible; mitigated by dry-run first, the cap, the second hold check and the audit trail. Data Plane inventory may find a store without a delete contract (then that store is listed as a gap and the sweeper does not claim it).

#### C4 - Holds against user delete and erasure [BUILD, decision-gated]
**Goal.** Per D-C1: a held thread or org cannot be erased by a user delete, a privacy erase or an org GDPR erasure until the hold is released; the caller gets a typed refusal; the attempt is audited.
**Files.** `SC/src/grpc.rs` `delete_thread`/`delete_threads` (`:8492`, `:8563`) ask the C1 check route first; `SC/src/gdpr.rs` org purge checks `org_held`; `OC` org hard-delete path (`gdpr_handlers.go:40`, `:153`) refuses while an org hold is active; BFF `privacy.rs` erase (`BFF/domains/privacy.rs:40`) maps the refusal; refusal code `erasure_blocked_legal_hold` added to the FE error catalogue.
**Flag.** `ERASURE_HOLD_CHECK` (session-core and org-core, strict true, default false).
**ZDR.** None new.
**Tests.** each path refuses while held and succeeds after release; check route unreachable means delete refused with a retryable error (fail closed for a hold).
**Live proof.** Place a thread hold, attempt delete from the SPA, see the refusal; release, delete succeeds.
**Effort.** S-M, 3-4 d. **Deps.** D-C1 recorded; C1. **Risks.** A fail-closed check can block legitimate erasure during an org-core outage; the refusal is retryable and alerts (WS06-S0 rules).

#### C5 - Provider data-use label [BUILD]
**Goal.** The model picker shows, per model, the provider's data-use posture: trains on data or not, retention days, ZDR attested or not.
**Files.** `PROTO/inference.proto` `ModelInfo.data_use = 12` (message `ProviderDataUse { bool trains_on_inputs = 1; int32 retention_days = 2; bool zdr_attested = 3; string statement_ref = 4; }`; 9-11 are EP-13's, so this lands after EP-13 S2); inference-core fills it from its provider capability table (attested values only; unknown is "not stated", never "no"); `FES/shared/components/AiModelPicker.tsx` renders a short label.
**Flag.** None (additive field; FE renders when present).
**ZDR.** No store, no egress.
**Tests.** inference-core: every provider either fills the field from a cited source or leaves it unset; FE picker test.
**Live proof.** Screenshot of the picker with two providers.
**Effort.** S, 2-3 d. **Deps.** EP-13 S2 (field numbers). **Risks.** Public-claim wording: the label text goes through the WS01-S14 counsel wording before release.

#### C6 - SHA-pinned GitHub Actions [BUILD]
**Goal.** Every `uses:` line in `.github/workflows` is pinned to a 40-hex commit SHA with the tag in a comment, and a gate keeps it that way.
**Files.** `scripts/gates/pin-actions.py` NEW (`--check` fails on any unpinned third-party action; `--baseline` file for a ratchet); all 28 workflows rewritten (177 lines at HEAD); `.github/workflows/repo-gates.yml` job `pinned-actions`.
**Flag.** None.
**ZDR.** None.
**Tests.** `python -m pytest scripts/tests/test_pin_actions.py` (fixture with a tag ref fails, SHA passes, local `./` actions exempt).
**Live proof.** CI run green on the pinned tree, red on a fixture.
**Effort.** S, 2-3 d. **Deps.** none. **Risks.** Pinned SHAs need updates; a Dependabot config for actions is part of the slice [INF: confirm Dependabot is allowed on this repo].

## 5. ZDR clause

Rule for this epic: a waived run (DEC-32) counts as ZDR for every new store and every new egress. Nothing in this epic stores content.

| New store or egress | Slice | Holds content? | ZDR / waived behaviour | Refusal |
|---|---|---|---|---|
| `search_provenance` SSE frame | W1 | No (provider ids) | Emitted; route says `ZdrFreeOnly` | none |
| `web.*` keys in the org agent policy document | W2 | No | Tighten only; ZDR already refuses paid providers | none |
| Host lookups to org-core resolve | W3 | No (canonical host only) | Same checks under ZDR | `org_site_blocked` (tool error) |
| `docs/compliance/egress-catalogue.json` | T1 | No | Declares each destination's ZDR behaviour | n/a |
| Effective-policy read | T2 | No | Readable by ZDR callers | n/a |
| `CodeResult.sandbox` report | E1 | No | Same as today's tool results | none |
| `space_effect_policies.sandbox_egress_*` | E2 | No (hosts) | ZDR evidence never gets an egress permission | decision issued with no egress |
| Sandbox egress through the proxy | E3 | Data may leave to allowed hosts | **Refused**: tier forced to `off` for ZDR and waived runs | feature `sandbox_egress`, code `feature_not_supported` when a ZDR run requests it explicitly |
| `leases.egress_tier_permitted` | E3 | No | Thread leases are already refused under ZDR (`sandboxes.proto` field 8) | none |
| `org_retention_policies`, `org_legal_holds`, history | C1 | No (ids, numbers, reference) | Thread hold on a ZDR or waived thread refused | `legal_hold_nothing_retained` |
| `retention_sweep_runs` | C2 | No (counts) | Nothing to sweep for ZDR | none |
| Deletions | C3 | removes content | n/a | none |
| `ModelInfo.data_use` | C5 | No | Same list for ZDR orgs | none |

Golden matrix additions (proposed names; the integrator aligns them with the `e_` prefix the loader requires):
- `apps/Model Plane/api/contracts/zdr-posture-matrix.json` enforcement rows: `e_sandbox_egress_refused_under_zdr` (ZDR bearer, tiered Space: decision `zdr`, egress off), `e_sandbox_egress_refused_for_waived_run` (waived bearer: egress off), `e_legal_hold_on_zdr_thread_refused`.
- `apps/Model Plane/api/contracts/zdr-refusals.json` features: `sandbox_egress` (E3) and `legal_hold` (C1); regenerate the `.sha256`, the FE copy `FES/shared/api/contracts/zdr-refusals.json` and its sha, and run `zdr-contract-sync.test.sh` (D2-875 shows the trap: a new `FEATURES` entry without the regenerated JSON fails `mp-contracts`).
- Inventory tests to extend: `MPR/services/execution-core/tests/zdr_refusal_inventory.rs` (egress refusal site), `MPR/services/model-gateway/tests/zdr_posture_matrix.rs` (row count changes).

## 6. Proof plan

| Slice | Local proof | Live proof (lead runs docker) | Evidence file |
|---|---|---|---|
| W1 | unit, taxonomy, vitest | Norwegian time-sensitive turn, SSE + screenshot | `EV/live-proof-ep9-w1-searched-via-<date>.txt` |
| W2 | unit, CC Go, EC lib | org off: Ask and Do transcripts, BFF JSON | `EV/live-proof-ep9-w2-<date>.txt` |
| W3 | unit, client tests | blocked host fetch refused, hit filtered, Quarry log | `EV/live-proof-ep9-w3-<date>.txt` |
| T1 | node tests | two CI runs (green, red fixture) | CI links in the ledger entry |
| T2/T3 | BFF and vitest | admin vs member JSON, screenshots | `EV/live-proof-ep9-t2-<date>.txt` |
| E1 | EC lib, vitest | one Do run tool result + card | in E3's file |
| E2 | Go tests + Postgres integration | forged claim decision on dev stack | `EV/live-proof-ep9-e2-<date>.txt` |
| E3 | EC Linux tests in container, SM tests | pip install works, curl blocked, ZDR forced off | `EV/live-proof-ep9-e3-<date>.txt` |
| E4 | tokenizer table | approve once, second run uses the rule | `EV/live-proof-ep9-e4-<date>.txt` |
| C1 | Postgres lifecycle tests | admin flow + audit event | `EV/live-proof-ep9-c1-<date>.txt` |
| C2 | real-PG ignored tests | dry pass counts, no deletions | `EV/live-proof-ep9-c2-<date>.txt` |
| C3 | real-PG ignored tests | 2 of 3 deleted, held kept, letta gone | `EV/live-proof-ep9-c3-<date>.txt` |
| C4 | path tests | delete refused while held | in C3's file |
| C5, C6 | unit, CI | screenshot, CI links | ledger |

Flag flips, in order, each after its live proof: `MODEL_GATEWAY_SEARCH_PROVENANCE` (W1); W2 needs no switch of its own but rides `ORG_AGENT_POLICY_MODE=enforce`, which WS25-S12 owns; `WEB_DOMAIN_POLICY_MODE` shadow, then enforce (W3); `CODE_INTERPRETER_ENFORCEMENT_REPORT` (E1); `SANDBOX_MANAGER_EGRESS_TIERS` then `EXECUTION_SANDBOX_EGRESS=registries` for one internal org (E3); `ORG_RETENTION_POLICY_ENABLED` (C1); `SESSION_RETENTION_SWEEP=dry_run` for a week, owner reviews, then `enforce` (C2, C3); `ERASURE_HOLD_CHECK` after D-C1 (C4); the effective-policy pair (T2). The owner's live-test rule (stop each batch at the usage line, the live key from the env file outside the repos) applies to every live proof.

Regression guards (audit section 8 strengths): policy only tightens versus the server floor (W2, W3, E2, E3 all narrow); the two loops stay separate (shared code is only in `mp-contracts`); credential isolation holds (no secret enters a sandbox in W3; E5 is an ADR); typed refusals stay typed (`org_web_off`, `org_site_blocked`, `legal_hold_nothing_retained`, `erasure_blocked_legal_hold` go into the error catalogue); served-by provenance is extended, not replaced (W1 is a new frame).

Session-core regression gate for C2/C3: full DB harness run compared with the 1031-passed baseline of `c18196af2`; diff failing sets, not exit codes.

## 7. Risks and what the owner must decide

**Owner decisions, with what each blocks:**
1. **D-W1** (web switches in capability-core, host lists in org-core). Blocks W2 and W3. Recommended because it reuses two recorded stores (DEC-14, DEC-19) and adds no new one. Everything else can start without it.
2. **D-E2** (grant the lease at `off` instead of refusing an egress-capable claim; amends ledger `:4440-4450`). Blocks E3 only.
3. **D-C1** (legal hold versus GDPR erasure; owner plus counsel). Blocks C4 only. C1-C3 build and run with "erasure wins" unchanged.
4. **DEC-21 / D-E4** (credential custody). Blocks E5 and W4 043(c).
5. **Q-T1** (publish the generated disclosure). Blocks only publishing, not T1.
6. Confirm the deferrals: regime tags (D-C3), telemetry registry, share links (EP-12), signed release (WS01-S12) are not W3 slices.

**Risks:**
1. **The proxy as an exfiltration path (E3).** A CONNECT proxy cannot see paths, so a prompt-injected run can leak through an allowed host. Mitigations: ZDR, waived, plan-mode, tainted and locked-down runs get `off`; `all` is not storable; `registries` is a short reviewed list; the catalogue states the residual risk. The taint input is weak until D2-353, D2-354 and D2-604 close.
2. **Irreversible deletion (C3).** Mitigated by a dry-run week, a per-org daily cap, a second hold check, audit events and the unknown-value-means-dry-run switch rule.
3. **Leaking policy across roles or tenants (T2).** Redaction to counts for members, org from the token only, per-owner timeouts that return `unavailable` instead of guesses.
4. **Public claims (T1, C5, W1 labels).** Generated, never hand-edited; nothing goes to the trust page before counsel.
5. **Cross-session dependency (W3).** The Quarry half (`/v1/extract` request layer, `policy_rev` echo) belongs to the Quarry plan; until it ships, the Model-side check is the enforcement for fetch. The unminted `web_policy` claim in Quarry should be named in the Quarry plan as superseded by D-W2 (b), or the two designs will drift.
6. **Hot files.** `sse.rs`, `tool_loop.rs`, `agent.rs`, `http_routes.rs`, `action-registry.ts`, the org agent policy schema (shared with EP-2 S8), `execution.proto` (`OrgPolicyContext` 4 is free and claimed here), `sandboxes.proto` (`AcquireLeaseResponse` 7 claimed here), `inference.proto` (`ModelInfo` 12 after EP-13's 9-11). Land additive blocks, one slice at a time per file, CRLF preserved.
7. **Personal-Space-only egress.** Shared Spaces cannot get sandbox decisions at all until the Control shared-Space decision family lands; personal non-Space runs have no decision path (Q-E4). The audit's "package installs" value reaches personal-Space runs first.

**Migration and proto reservations proposed (integrator assigns):**
- org-core `030` (round B; `025`-`029` exist on branches `w23/integ` and `space-w2`): `org_retention_policies` (C1).
- user-core `044` (round B; `033`-`043` exist on other branches): `space_sandbox_egress` (E2).
- sandbox-manager `0008`: `lease_egress_tier` (E3).
- session-core `0095` (round B): `retention_sweep_runs` (C2), with `revert/`.
- capability-core: none (head stays `0030`; W2 keys live in the existing JSONB).
- `execution.proto` `OrgPolicyContext.web = 4` (`WebPolicyContext` fields 1-4) (W2, W3); `sandboxes.proto` `AcquireLeaseResponse.egress_tier = 7` plus enum `SandboxEgressTier` (E3); `inference.proto` `ModelInfo.data_use = 12` plus `ProviderDataUse` (C5).
