## 9. Roadmap and decisions

Tiers and efforts below are the verified values. Six items moved from Recommended to Nice-to-have during verification (F-t2-054, 057, 061, 063, 064, 065) but keep their wave; 33 efforts were revised, mostly upward. ZDR means Zero-Data-Retention. ADR means Architecture Decision Record.

### 9.1 Implementation waves

**W1: unblock and make safe.** Goal: make every browser path work in production mode, close the ZDR and tenant leaks, gate raw script, stop resource leaks and relaunches, and ship the inline scrape contract and file routing. Phase 1 of [F-t2-013](#F-t2-013) (takeover enforced inside Quarry, S) also lands here.

| ID | Title | Tier | Effort | Plane | Dependencies |
|---|---|---|---|---|---|
| [F-t2-001](#F-t2-001) | Production BrowserBroker grants and delegated bearer forwarding | Priority | L | Model Plane, Frontend Plane | Broker persistence, gateway browser-bearer plumbing; foundation for every browser item |
| [F-t2-002](#F-t2-002) | Gate Evaluate and raw script actions | Priority | S | Ingestion Plane | Broker sensitive-action allowlist change |
| [F-t2-003](#F-t2-003) | Close the search-cache ZDR and tenant hole; ZDR capability matrix | Priority | M | Frontend Plane, Ingestion Plane | Existing gateway zdr helpers |
| [F-t2-004](#F-t2-004) | ZDR-safe telemetry | Priority | M | Ingestion Plane, Model Plane, Frontend Plane | Phase 2 needs spans from [F-t2-029](#F-t2-029) |
| [F-t2-005](#F-t2-005) | One strict ScrapeOptions contract, inline return | Priority | M | Ingestion Plane | OpenAPI-from-types slice of [F-t2-064](#F-t2-064); crawl and batch embedding is a follow-on |
| [F-t2-006](#F-t2-006) | Reap abandoned runs, cap concurrent runs | Priority | M | Ingestion Plane | Shares admission code with [F-t2-025](#F-t2-025) |
| [F-t2-007](#F-t2-007) | Stop viewport-triggered relaunches, process pool | Priority | M | Ingestion Plane | None |
| [F-t2-009](#F-t2-009) | Route PDFs and Office files to the right parser | Priority | S | Ingestion Plane | None |
| [F-t2-026](#F-t2-026) | Per-operation browser deadlines | Recommended | S | Ingestion Plane | Optional [F-t2-047](#F-t2-047) sub-codes |
| [F-t2-027](#F-t2-027) | Honest bot-block handling, no stealth code | Recommended | M | Ingestion Plane, Model Plane, Frontend Plane | Anti-bot ADR for the live escalation paths |

**W2: table-stakes parity.** Goal: a browser that users and the AI share (free browsing, chat beside the page, co-browsing, planner on, trusted input, run trace) and an acquisition API with scoped crawl, job lifecycle, executable schedules, extraction and actions, all under budgets, encryption, redaction, org policy and capacity limits.

| ID | Title | Tier | Effort | Plane | Dependencies |
|---|---|---|---|---|---|
| [F-t2-008](#F-t2-008) | Enforce cost and runtime budgets in the Quarry executor | Priority | L | Ingestion Plane, Model Plane, Control Plane | [F-t2-022](#F-t2-022) claim channel, [F-t2-006](#F-t2-006), [F-t2-046](#F-t2-046) |
| [F-t2-010](#F-t2-010) | LLM browser planner on by default, connector-first routing | Priority | M | Model Plane | [F-t2-001](#F-t2-001), [F-t2-015](#F-t2-015), budgets ([F-t2-008](#F-t2-008)), [F-t2-021](#F-t2-021) |
| [F-t2-011](#F-t2-011) | Free human browsing: human-session policy plus omnibox | Priority | L | Ingestion Plane, Model Plane, Frontend Plane | [F-t2-001](#F-t2-001), [F-t2-035](#F-t2-035), [F-t2-003](#F-t2-003) |
| [F-t2-012](#F-t2-012) | Chat beside the browser, dockable panel | Priority | L | Frontend Plane | [F-t2-001](#F-t2-001), [F-t2-013](#F-t2-013) |
| [F-t2-013](#F-t2-013) | Co-browsing in agent-flagged tabs (phase 2) | Priority | L | Ingestion Plane, Model Plane, Frontend Plane | [F-t2-001](#F-t2-001), [F-t2-011](#F-t2-011), delegation design |
| [F-t2-014](#F-t2-014) | One canonical run trace in the UI | Priority | L | Model Plane, Ingestion Plane, Frontend Plane | [F-t2-001](#F-t2-001), [F-t2-004](#F-t2-004), Application Plane projection |
| [F-t2-015](#F-t2-015) | Actionability checks and trusted input | Priority | M | Ingestion Plane | None |
| [F-t2-016](#F-t2-016) | Scoped, robots-compliant crawl | Priority | L | Ingestion Plane, Control Plane | [F-t2-005](#F-t2-005), [F-t2-017](#F-t2-017), ZDR ADR |
| [F-t2-017](#F-t2-017) | Public async job lifecycle | Priority | L | Ingestion Plane | Temporal client reachable from control or edge |
| [F-t2-018](#F-t2-018) | Schedules that execute (Temporal) | Priority | L | Ingestion Plane | Schedule persistence, [F-t2-016](#F-t2-016), [F-t2-008](#F-t2-008) |
| [F-t2-019](#F-t2-019) | Structured extraction over a real structured-output grant | Priority | M | Ingestion Plane, Model Plane, Control Plane | [F-t2-005](#F-t2-005), model-gateway grant to quarry-edge |
| [F-t2-020](#F-t2-020) | Harden persistent browser profiles | Priority | L | Ingestion Plane, Control Plane | Control Plane key API, [F-t2-007](#F-t2-007) |
| [F-t2-021](#F-t2-021) | Secret redaction at capture time | Priority | L | Ingestion Plane, Model Plane | [F-t2-036](#F-t2-036) for fill audit |
| [F-t2-022](#F-t2-022) | Org-pinned, tighten-only web policy | Priority | L | Control Plane, Model Plane, Ingestion Plane | Policy-owner ADR first; [F-t2-016](#F-t2-016), [F-t2-058](#F-t2-058) |
| [F-t2-023](#F-t2-023) | Scrape-time actions, screenshot and PDF formats | Priority | L | Ingestion Plane | [F-t2-005](#F-t2-005), [F-t2-002](#F-t2-002), [F-t2-003](#F-t2-003) cache admission |
| [F-t2-025](#F-t2-025) | Per-org capacity, fair share, plan quotas | Recommended | L | Ingestion Plane, Control Plane | Control quota keys, fail-open ADR |
| [F-t2-029](#F-t2-029) | Quarry metrics, drain, span vocabulary, alerts | Recommended | L | Ingestion Plane, Control Plane | [F-t2-004](#F-t2-004), [F-t2-014](#F-t2-014), [F-t2-006](#F-t2-006) |
| [F-t2-045](#F-t2-045) | Memory pollution guard for web-tainted runs | Recommended | M | Model Plane | None |

**W3: governed depth.** Goal: credential handoff, consent and approval UX, agent presence, observation quality, durable sessions, downloads, monitors, document parsing, webhooks, decision audit, batch, discovery, metering, caching and screening.

| ID | Title | Tier | Effort | Plane | Dependencies |
|---|---|---|---|---|---|
| [F-t2-024](#F-t2-024) | Credential handoff: fill_otp first, secrets later | Priority | L | Ingestion Plane, Model Plane | [F-t2-001](#F-t2-001), [F-t2-013](#F-t2-013), [F-t2-015](#F-t2-015), [F-t2-021](#F-t2-021), secret-custody ADR |
| [F-t2-028](#F-t2-028) | Page context to chat (@tab, selection, hit-test) | Recommended | L | Frontend Plane, Ingestion Plane, Model Plane | [F-t2-012](#F-t2-012), [F-t2-001](#F-t2-001) |
| [F-t2-030](#F-t2-030) | Monitor change quality | Recommended | L | Ingestion Plane, Model Plane | [F-t2-005](#F-t2-005), [F-t2-019](#F-t2-019) for the judge |
| [F-t2-031](#F-t2-031) | Tenant-scoped, SSRF-guarded webhooks | Recommended | M | Ingestion Plane | [F-t2-017](#F-t2-017); automation-owner ADR for phase 2 |
| [F-t2-032](#F-t2-032) | Per-origin consent, action-class grants, settings hub | Recommended | XL | Model Plane, Ingestion Plane, Control Plane, Frontend Plane | [F-t2-001](#F-t2-001), policy ADR |
| [F-t2-033](#F-t2-033) | Approval loop UX | Recommended | L | Model Plane, Frontend Plane, Application Plane | [F-t2-001](#F-t2-001), [F-t2-032](#F-t2-032) |
| [F-t2-034](#F-t2-034) | Observation quality and budget | Recommended | M | Ingestion Plane, Model Plane | [F-t2-015](#F-t2-015) |
| [F-t2-035](#F-t2-035) | Durable hosted browser sessions | Recommended | L | Ingestion Plane, Frontend Plane | [F-t2-020](#F-t2-020), [F-t2-023](#F-t2-023) |
| [F-t2-036](#F-t2-036) | Durable authority and decision audit | Recommended | XL | Model Plane, Control Plane, Ingestion Plane | Broker persistence, [F-t2-014](#F-t2-014) |
| [F-t2-037](#F-t2-037) | Document parsing depth, OCR routing | Recommended | L | Ingestion Plane, Model Plane | [F-t2-005](#F-t2-005), model-gateway grant (shared with [F-t2-019](#F-t2-019)) |
| [F-t2-038](#F-t2-038) | Agent presence and run control UX | Recommended | L | Frontend Plane, Model Plane, Application Plane | [F-t2-028](#F-t2-028), [F-t2-013](#F-t2-013), [F-t2-010](#F-t2-010) |
| [F-t2-039](#F-t2-039) | Governed downloads and uploads | Recommended | M | Ingestion Plane, Data Plane, Frontend Plane | [F-t2-001](#F-t2-001), tenant-scoped artifact store |
| [F-t2-040](#F-t2-040) | Parallel batch scrape | Recommended | L | Ingestion Plane | [F-t2-005](#F-t2-005), [F-t2-017](#F-t2-017) |
| [F-t2-041](#F-t2-041) | Sitemap and index-assisted discovery, URL dedupe | Recommended | M | Ingestion Plane | [F-t2-016](#F-t2-016) |
| [F-t2-042](#F-t2-042) | Agent web toolset, Quarry jobs as gated tools | Recommended | L | Model Plane, Ingestion Plane | [F-t2-005](#F-t2-005), [F-t2-008](#F-t2-008), [F-t2-022](#F-t2-022) |
| [F-t2-044](#F-t2-044) | Hermetic browser and scrape evals in CI | Recommended | L | Ingestion Plane, Model Plane | Browser loop wiring, [F-t2-008](#F-t2-008) |
| [F-t2-046](#F-t2-046) | Complete web-usage metering | Recommended | L | Control Plane, Ingestion Plane, Frontend Plane | [F-t2-022](#F-t2-022) claim channel, billing-core codes |
| [F-t2-047](#F-t2-047) | Fetch-path reliability, wired retries | Recommended | M | Ingestion Plane, Frontend Plane | [F-t2-016](#F-t2-016), [F-t2-027](#F-t2-027) |
| [F-t2-048](#F-t2-048) | Org-scoped URL cache, no-egress mode | Recommended | M | Ingestion Plane, Data Plane | [F-t2-003](#F-t2-003), [F-t2-022](#F-t2-022) |
| [F-t2-049](#F-t2-049) | URL reputation gate, layered screening | Recommended | M | Ingestion Plane, Model Plane | [F-t2-022](#F-t2-022), [F-t2-032](#F-t2-032) |
| [F-t2-050](#F-t2-050) | Fail-closed PII redaction stage | Recommended | L | Ingestion Plane, Model Plane, Control Plane | [F-t2-005](#F-t2-005), model-gateway grant |

**W4: platform and leverage.** Goal: procedures, result sets, packs, crawl planning, provider-API routing, engine planner v2, egress tiers, remote and isolated backends, routines, research jobs, code mode, the external contract and extraction profiles.

| ID | Title | Tier | Effort | Plane | Dependencies |
|---|---|---|---|---|---|
| [F-t2-043](#F-t2-043) | URL-aware planner guidance, provider hints | Recommended | L | Model Plane, Ingestion Plane | Browser loop wiring, OpenAPI completeness |
| [F-t2-051](#F-t2-051) | Procedure store and run-as-procedure | Recommended | XL | Ingestion Plane | [F-t2-001](#F-t2-001), [F-t2-008](#F-t2-008) |
| [F-t2-052](#F-t2-052) | Per-run result sets and export | Recommended | M | Ingestion Plane | Optional [F-t2-053](#F-t2-053) |
| [F-t2-053](#F-t2-053) | Saved job definitions and Quarry packs | Recommended | L | Ingestion Plane, Frontend Plane | [F-t2-018](#F-t2-018), [F-t2-052](#F-t2-052) |
| [F-t2-054](#F-t2-054) | Run- and turn-scoped tabs | Nice-to-have | L | Ingestion Plane, Model Plane | [F-t2-006](#F-t2-006), [F-t2-035](#F-t2-035), session-model decision |
| [F-t2-055](#F-t2-055) | Crawl planning and stronger /v1/map | Recommended | M | Ingestion Plane, Model Plane, Frontend Plane | [F-t2-016](#F-t2-016), [F-t2-005](#F-t2-005) |
| [F-t2-056](#F-t2-056) | Provider APIs and export rewrites before rendering | Recommended | M | Ingestion Plane | [F-t2-009](#F-t2-009), [F-t2-057](#F-t2-057) |
| [F-t2-057](#F-t2-057) | Engine planner v2 | Nice-to-have | M | Ingestion Plane | [F-t2-009](#F-t2-009), [F-t2-027](#F-t2-027), [F-t2-059](#F-t2-059) |
| [F-t2-058](#F-t2-058) | Request context and egress tiers | Recommended | XL | Ingestion Plane, Control Plane | [F-t2-007](#F-t2-007), [F-t2-020](#F-t2-020), [F-t2-022](#F-t2-022), [F-t2-027](#F-t2-027) |
| [F-t2-059](#F-t2-059) | Governed remote browser backends | Recommended | XL | Ingestion Plane | [F-t2-007](#F-t2-007), containment proof, substrate ADR |
| [F-t2-060](#F-t2-060) | Per-session or per-tenant isolation | Recommended | XL | Ingestion Plane | Egress broker, [F-t2-008](#F-t2-008), substrate ADR |
| [F-t2-061](#F-t2-061) | Agent routines with per-fire grants | Nice-to-have | XL | Model Plane | [F-t2-001](#F-t2-001), [F-t2-018](#F-t2-018), [F-t2-031](#F-t2-031), automation ADR |
| [F-t2-062](#F-t2-062) | Budgeted research job contract | Recommended | M | Model Plane, Ingestion Plane | [F-t2-001](#F-t2-001), [F-t2-019](#F-t2-019) |
| [F-t2-063](#F-t2-063) | Code-mode browser control | Nice-to-have | L | Model Plane | [F-t2-001](#F-t2-001), [F-t2-010](#F-t2-010), [F-t2-002](#F-t2-002), script ADR |
| [F-t2-064](#F-t2-064) | External contract: OpenAPI, then MCP server | Nice-to-have | XL | Ingestion Plane, Control Plane | [F-t2-005](#F-t2-005), [F-t2-017](#F-t2-017), [F-t2-031](#F-t2-031), MCP ADR |
| [F-t2-065](#F-t2-065) | Cached declarative extraction profiles | Nice-to-have | XL | Ingestion Plane, Model Plane | [F-t2-019](#F-t2-019), budget enforcement |

**W5: nice-to-have polish and long-tail bets.** Goal: schedule opportunistically or after the relevant ADR.

| ID | Title | Tier | Effort | Plane | Dependencies |
|---|---|---|---|---|---|
| [F-t2-066](#F-t2-066) | Consent-banner and ad-filter preset | Nice-to-have | M | Ingestion Plane | [F-t2-058](#F-t2-058), anti-bot ADR |
| [F-t2-067](#F-t2-067) | One honest product identity, signed agent traffic | Nice-to-have | M | Ingestion Plane, Control Plane | [F-t2-016](#F-t2-016), identity ADR |
| [F-t2-068](#F-t2-068) | Browsing data lifecycle | Nice-to-have | L | Ingestion Plane, Frontend Plane | [F-t2-011](#F-t2-011), [F-t2-032](#F-t2-032) |
| [F-t2-069](#F-t2-069) | Quarry-backed page utilities | Nice-to-have | M | Ingestion Plane, Frontend Plane | None |
| [F-t2-070](#F-t2-070) | Browser chrome polish | Nice-to-have | M | Frontend Plane | [F-t2-011](#F-t2-011) |
| [F-t2-071](#F-t2-071) | MCP App sandbox; retire WebPageViewer | Nice-to-have | L | Frontend Plane, Model Plane | Model Plane resources/read lane |
| [F-t2-072](#F-t2-072) | Share a run with Space members | Nice-to-have | L | Application Plane, Ingestion Plane, Control Plane | Resource taxonomy ADR |
| [F-t2-073](#F-t2-073) | SIEM export and governance extras | Nice-to-have | L | Control Plane | [F-t2-036](#F-t2-036) |
| [F-t2-074](#F-t2-074) | Failure-mode and provider hardening | Nice-to-have | S | Ingestion Plane | None |
| [F-t2-075](#F-t2-075) | Deprecation registry, signed images, usefulness signal | Nice-to-have | L | Ingestion Plane, Support Plane, Model Plane | None |
| [F-t2-076](#F-t2-076) | Child subagents and WebMCP page tools | Nice-to-have | L | Model Plane | [F-t2-001](#F-t2-001), [F-t2-010](#F-t2-010), [F-t2-032](#F-t2-032) |
| [F-t2-077](#F-t2-077) | Design spike: user's own Chrome via extension | Nice-to-have | XL | Ingestion Plane | Parked by owner ruling; [F-t2-059](#F-t2-059), [F-t2-024](#F-t2-024) |

### 9.2 Quick wins

These items are quick wins because they are wiring or deletion inside one plane with no unbuilt cross-plane dependency. Where the verified effort is M, the first slice is S.

- [F-t2-002](#F-t2-002) (S): a default-deny gate on `/step` for `AgentAction::Evaluate`; the isolated-world evaluator moves to [F-t2-023](#F-t2-023).
- [F-t2-003](#F-t2-003) (phase 1 is S): one gateway file, `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/search.rs:195-231`, plus a test, reusing the private `CacheAdmission` in `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/knowledge/quarry.rs:213-280`.
- [F-t2-004](#F-t2-004) (phase 1 is S): remove the URL field logged at `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/change_routes.rs:632-637`, sweep the tracing call sites, add a lint.
- [F-t2-005](#F-t2-005) (M for `/v1/scrape` only): formats, main-content stage and inline return reuse the existing pipeline.
- [F-t2-006](#F-t2-006) (M): a sweeper and caps at `start_run`; the fleet console proxy is a separate S.
- [F-t2-007](#F-t2-007) (part A is S): per-page viewport emulation removes the cross-session kill at `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs:194-223`.
- [F-t2-009](#F-t2-009) (S): the sniffer and both parsers exist; only PDF and DOCX routing needs wiring, since there is no image parser to route to.
- [F-t2-026](#F-t2-026) (S): one `cdp_call` wrapper inside quarry-browser.
- [F-t2-027](#F-t2-027) (deletion is S; full item M): `stealth.rs` has no caller, so removing it is safe.
- [F-t2-045](#F-t2-045) (skip-only slice is S): a `save_memory` preflight in execution-core plus a Dreaming window filter.
- Phase 1 of [F-t2-013](#F-t2-013) (S): reject agent-principal steps while `control_mode` is HumanTakeover in `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:2222-2310`.
- Two fixes surfaced by verification: the compose default at `apps/Ingestion Plane/Quarry-v2/deploy/compose/docker-compose.yml:97-99` disables the Chromium sandbox when `QUARRY_BROWSER_NO_SANDBOX` is unset ([F-t2-060](#F-t2-060) step 1), and `WebPageViewer` at `apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardSearchResults.tsx:147-192` is dead code that loads third-party pages outside Quarry's egress controls ([F-t2-071](#F-t2-071) slice a).

### 9.3 Decisions needed before building (ADR candidates)

1. **Web policy of record and tighten-only claims.** Question: does org web policy live in Control Plane org-core (delivered as signed claims) or in Model Plane capability-core (delivered as grant fields)? Items: [F-t2-022](#F-t2-022), [F-t2-032](#F-t2-032), [F-t2-008](#F-t2-008), [F-t2-025](#F-t2-025). Options: org-core claims, which already carry the ZDR floor but reach only user-token callers; or capability-core grants, which already have tenant-bound org-scope grants. Recommendation: service-token callers ignore the zdr claim by design (`apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/auth.rs:56-140`), so policy must travel by claim for user paths and by grant field for Model-driven runs whichever owner wins; ship the owner-neutral Quarry-only `ResolvedPolicy` slice first.
2. **Secret custody for browser credential use.** Question: who holds user secrets used inside pages, and does every email-OTP use need approval? Items: [F-t2-024](#F-t2-024), [F-t2-021](#F-t2-021). Options: extend integration-corev2 (today an at-rest token vault, not a user-secret store, with a shared internal API key) or create a Control Plane secret service. Recommendation: ship OTP first through the existing sensitive-approval path and defer password references until the ADR, as the verification suggests.
3. **Shared human+agent sessions.** Question: how does a grant bind a session that both a human and a Model run use, and what navigation policy governs free human browsing? Items: [F-t2-001](#F-t2-001), [F-t2-011](#F-t2-011), [F-t2-013](#F-t2-013), [F-t2-054](#F-t2-054). Options: a broker human-browse grant kind with renewal; a delegation claim so a service principal can attach to a user-owned run; or end and resume with a new grant. No reference implements takeover enforcement in the executor, so this is a Verevon design [INF]. Recommendation: design the three together; the data supports nothing more specific.
4. **ZDR semantics per feature.** Question: for monitors, URL caches, schedules, durable crawls and telemetry, does ZDR mean refused, inline-only or hash-only? Items: [F-t2-003](#F-t2-003), [F-t2-004](#F-t2-004), [F-t2-016](#F-t2-016), [F-t2-018](#F-t2-018), [F-t2-030](#F-t2-030), [F-t2-048](#F-t2-048). Today crawl, batch and schedules are refused under ZDR (`apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/routes.rs:563-588`). Recommendation: publish the matrix generated from route attributes rather than hand-written JSON; do not loosen existing refusals.
5. **Anti-bot posture.** Question: is honest identity official policy, and which policy-gated escalation (proxy rotation, TLS impersonation tiers) is allowed? Items: [F-t2-027](#F-t2-027), [F-t2-066](#F-t2-066), [F-t2-067](#F-t2-067), [F-t2-016](#F-t2-016), [F-t2-058](#F-t2-058). The live `block_rotation` already rotates proxies on bare 401/403/407/503, so the stance must cover shipped behaviour, not only the dead `stealth.rs`. Recommendation: confirm the honest posture and narrow rotation triggers with [F-t2-027](#F-t2-027).
6. **Script execution model.** Question: may the Model submit programs (evaluate, scrape action lists, code mode, generated extractors) if Quarry still validates every binding call? Items: [F-t2-002](#F-t2-002), [F-t2-023](#F-t2-023), [F-t2-063](#F-t2-063), [F-t2-065](#F-t2-065). Options: keep one governed action per call; or allow declarative capped action lists and a sandboxed script runtime whose every binding is a separate Quarry step. Recommendation: declarative lists and profiles fit the rule; code mode should be a slice of the generic code-mode already planned in Model Plane docs [DOC].
7. **External MCP exposure.** Question: will Verevon expose Quarry and read-only connectors to third-party agents, reversing the documented deferral in `apps/Ingestion Plane/Quarry-v2/docs/OSS_PARITY_BACKLOG.md:30`? Items: [F-t2-064](#F-t2-064), [F-t2-042](#F-t2-042). Options: stay REST-only; or add an audience-bound MCP front outside Quarry with customer API keys and audit routed through existing producers. Recommendation: do the OpenAPI-from-types slice now and gate the rest on an owner decision.
8. **Run trace envelope and sharing.** Question: which envelope is canonical, and how do Application Plane projections and Space grants avoid widening the owning plane's ACL? Items: [F-t2-014](#F-t2-014), [F-t2-038](#F-t2-038), [F-t2-072](#F-t2-072). Recommendation: extend the existing `model_plane.v1.Event` (reserved fields 14-20 in `apps/Model Plane/proto/model_plane/v1/events.proto:8-60`) rather than add a second envelope; run sharing needs a resource-taxonomy decision because `quarry_run` is org-team-shared today.
9. **Automation owner.** Question: do completion-triggered follow-ups and agent routines belong to Application Plane or Model Plane routines, and how are unattended browser runs authorized per fire? Items: [F-t2-031](#F-t2-031), [F-t2-061](#F-t2-061). Constraint: unattended runs are draft-only through Wave 2 by owner ruling, and the scheduled lane cannot borrow a human's delegated bearer. Recommendation: keep acquisition-only automation on Quarry schedules; agent routines wait for the pause/resume design.
10. **Browser execution substrate.** Question: shared Chromium pool, per-session containers or microVMs, remote providers, or a user-browser extension? Items: [F-t2-007](#F-t2-007), [F-t2-059](#F-t2-059), [F-t2-060](#F-t2-060), [F-t2-077](#F-t2-077). Constraint: the pinned egress proxy type is loopback-only (`apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/lib.rs:331-360`), so remote or containerised Chromium needs either a sidecar proxy or a reviewed relaxation. Recommendation: per-page emulation and the sandbox default now; CDP-connect worker pool next; the extension stays parked by the 2026-09-28 owner ruling.

### 9.4 Dependency notes

1. [F-t2-001](#F-t2-001) gates every browser item: production Quarry rejects runs without a grant (`apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs:182-200`) and nothing issues one, so [F-t2-010](#F-t2-010), [F-t2-011](#F-t2-011), [F-t2-012](#F-t2-012), [F-t2-013](#F-t2-013) phase 2, [F-t2-024](#F-t2-024), [F-t2-032](#F-t2-032) and [F-t2-063](#F-t2-063) cannot be tested end to end before it.
2. [F-t2-005](#F-t2-005) is the second foundation: [F-t2-016](#F-t2-016), [F-t2-019](#F-t2-019), [F-t2-023](#F-t2-023), [F-t2-030](#F-t2-030), [F-t2-037](#F-t2-037), [F-t2-040](#F-t2-040) and [F-t2-050](#F-t2-050) extend its options or formats.
3. The policy ADR (9.3 item 1) must precede [F-t2-022](#F-t2-022), which in turn supplies the claim channel for [F-t2-008](#F-t2-008), [F-t2-025](#F-t2-025), [F-t2-046](#F-t2-046), [F-t2-048](#F-t2-048) and [F-t2-058](#F-t2-058).
4. [F-t2-010](#F-t2-010) (planner on by default) must wait for budgets ([F-t2-008](#F-t2-008)) and capture-time redaction ([F-t2-021](#F-t2-021)), because the planner prompt receives raw observation text today.
5. [F-t2-016](#F-t2-016) scope must land before [F-t2-018](#F-t2-018) switches schedules on, or recurring crawls repeat the off-site walk; [F-t2-018](#F-t2-018) also needs an explicit re-enable so existing rows do not all start firing.
6. [F-t2-021](#F-t2-021) redaction precedes any `fill_secret` in [F-t2-024](#F-t2-024), since non-ZDR receipts store typed text verbatim (`apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/step_receipts.rs:185-200`).
7. [F-t2-019](#F-t2-019), [F-t2-030](#F-t2-030) phase 2, [F-t2-037](#F-t2-037) and [F-t2-065](#F-t2-065) all wait for the model-gateway grant to quarry-edge, which is `granted:false` in `apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json:40-46` and has no owner.
8. [F-t2-004](#F-t2-004) phase 2 and [F-t2-029](#F-t2-029) are mutually ordered: the sampler needs spans, and spans must not be exported for ZDR runs before the sampler exists; ship the log-field sweep first.
9. [F-t2-007](#F-t2-007) part A precedes [F-t2-020](#F-t2-020) restore hooks, [F-t2-058](#F-t2-058) emulation and both [F-t2-059](#F-t2-059) and [F-t2-060](#F-t2-060).
10. [F-t2-035](#F-t2-035) and [F-t2-054](#F-t2-054) must share one session state machine; decide whether a chat thread keeps one long-lived Quarry run across turns before building either.

