# Velion Roadmap

**Last updated:** 2026-08-02. Synthesizes `velion-vision.md` (what Velion
should become), `VELION.md` and `Velion-ai-first.md` (what is verifiably
live today), and `velion-feature-map.md` (the feature-by-feature reality
map, including its own §5 recommended sequence, which this roadmap absorbs
and updates rather than duplicates). Also folds in the findings from a
code-health audit started today — see §7 for its actual status, which is
partial: a hard subagent-quota wall stopped it after 1 of 8 planned
velionv3 areas, and the "down the stack" pass across the other 5 planes
never started. That is a real, named gap in this roadmap's evidence base,
not swept under anything — §7 is the concrete plan to close it.

---

## 1. Where Velion actually stands, right now

The single biggest fact this week's work changes: **the "engine not proven
live" caveat that gated almost every recommendation in `velion-feature-map.md`
is gone.** Model Plane's hot path ran real streaming chat for hours against
live Azure providers, tool loop, and GraphRAG retrieval. That single proof
retires more open questions than any individual feature fix — it converts
"the source is built for this" (§2 of the vision doc) into "the running
system has demonstrated it" for the chat surface specifically.

Alongside that, this week closed several capabilities that were previously
either completely inert or outright faked, without anyone knowing it until
the diffs were actually read:

- Cost-aware model selection (Budget/Balance/Genius) was **silently inert**
  since it was built — a missing auth header 401'd every budget check.
- The per-answer confidence badge was **hardcoded at 72%** for every
  tool-grounded answer.
- Delegated subagents were **completely faked** — a canned string, zero
  execution.
- Durable Temporal workflows (deep research, wide research, memory/skill/
  feedback promotion) had **zero production callers**, despite being fully
  built and tested.

All four are now real and live-verified. This matters for how this roadmap
is written: **do not trust a "this exists" claim anywhere in this repo's
docs without a live-verification date next to it.** Several of the above
were documented as "live" for weeks while being completely inert in
production. The practice this roadmap commits to, and that any team
picking it up should keep: read the diff, not the commit message; verify
live, not just in source.

What is now real and load-bearing, condensed from the three status docs:
- **Chat**: streaming, resumable (a disconnect no longer kills the run),
  edit/regenerate version navigation, MCP third-party tool calls (proven
  live against a real Visma ERP), a code interpreter + canvas artifacts
  producing genuine files, a response cache reaching real traffic, memory
  provenance shown to the user, server-owned pinning.
- **Agents**: a real, HITL-enforced Run Console; durable workflows reachable
  end-to-end; cron-fired runs visible.
- **Knowledge**: hybrid + GraphRAG retrieval, real source traces, per-user
  ownership — the "crown jewel" claim in `velion-feature-map.md` §1.1 holds.
- **Inbox / Ticketing**: real backends, real AI-draft-with-HITL, real
  SLA/macros/automation-rule data model — with two fresh code-health gaps
  named below (§7.1).

What is still open, unchanged by this week's work:
- Post-approval continuation (an approved risky action has no restartable
  dispatcher/receipt).
- ZDR provider attestation (enforcement is complete; no provider is
  contractually attested — a business task, not an engineering one).
- The capability-health scope for code interpreter/canvas/sandbox is real
  in source but not yet granted in the deployed service-principal registry.
- WorkflowBuilder/ChatbotStudio/ChatbotPlayground remain presentation-only.
- Image provenance (no SHA tags/`/version` endpoints across the full
  estate — partially done for 5 slice services per feature-map §6.7).
- The session-core gRPC approval surface is unauthenticated — **and a
  second, more specific gap was found today**: even a fully authenticated
  caller can cross tenant boundaries on an approval, because model-gateway's
  approval RPCs carry no `org_id` at all (§7.2).

## 2. The vision, restated as a filter for everything below

Per `velion-vision.md`: Velion's moat is three properties combined —
grounded retrieval with real source traces, server-enforced *approvable*
execution, and EU/Norway residency as an architectural contract, not a PDF.
No roadmap item below should be read as "build a feature." Every item is
read as: **does this deepen one of those three properties, or does it not
matter yet.** The curated-wedge doctrine (vision §3, feature-map §6.6)
applies with equal force: one best-in-class move per competitor axis, never
a chase for parity.

## 3. Phase A — Close what this week's work surfaced (do first, small, real)

Concrete, bounded, already-scoped fixes surfaced by this week's digest and
today's audit — not speculative roadmap items, actual named bugs:

1. **Ticketing error-handling and action-contract bypass** (found
   2026-08-02, `velion-feature-map.md` §1.4): every ticket data fetch
   swallows its own error, making the "Ticketing unavailable" UI dead code;
   ticket mutations bypass the already-built, validated `tickets.*` action
   contract entirely. Both are surgical, well-scoped fixes — do them before
   anything else in this list, they are the cheapest wins here.
2. **Cross-tenant approval IDOR** (found 2026-08-02, §1 above and
   `VELION.md`): stamp `org_id` on the approval record in session-core/
   model-gateway and check it in `GetApprovalRequest`/`DecideApprovalRequest`.
   This is a genuine security gap, not a nice-to-have — prioritize ahead of
   any new-feature work, alongside the pre-existing unauthenticated-approval-
   surface item it sits next to.
3. **Post-approval continuation** (long-standing P0, `velion-feature-map.md`
   §1.2/§1.8): an approved risky action has no restartable dispatcher. Given
   HITL + audit is the actual moat (vision §2), a pause that cannot resume
   durably undercuts the differentiator itself, not just UX polish.
4. **Grant the capability-health scope in the deployed registry** for
   `cap.command.shell`/`cap.command.sandbox` (code interpreter/canvas) — an
   operator/admin action, not code; the self-attestation heartbeat already
   works and is just waiting on this.
5. **Social calendar's clock is frozen in June 2026, live right now** (found
   2026-08-02, `velion-feature-map.md` §1.6): every draft a user creates
   today gets silently scheduled about two months in the past, and the
   calendar never shows the real current day. One-line fix (default to
   `new Date()`), but it is actively corrupting data for anyone using the
   Social calendar until fixed — treat as urgent, not just "found this week."
6. **2FA step-up password renders in plaintext** (found 2026-08-02,
   `velion-feature-map.md` §1.7): a security-relevant regression, fix before
   any demo that touches 2FA.
7. **The registry's one irreversible, approval-gated action has no approval
   gate in practice** (found 2026-08-02, `velion-feature-map.md` §1.6):
   `social.publish_post` (`risk: high, requiresApproval: true, reversible:
   false`) is bypassed entirely by the human UI, which calls the raw REST
   route directly with no `approvalId`. The same "UI operation wired outside
   its action contract" pattern recurs at least 3 times now (ticketing,
   social, and — see below — the chat Brreg tool with a different failure
   mode): **this is a systemic pattern, not three unrelated bugs.** Worth a
   single fix at the pattern level — e.g. a lint rule or a runtime assertion
   that a `LIVE_ACTIONS` entry marked `requiresApproval` cannot be reached by
   a code path that skips `executeAction` — rather than three point patches.
8. **Chat's auto-attached Brreg tool silently loses its parameter schema**
   (found 2026-08-02, `velion-feature-map.md` §1.2): a one-character id
   mismatch (`_` vs `.`) between the tool declaration and the action
   registry. One-line fix, but currently active in production every time the
   auto-attach keyword fires.
9. **Onboarding has zero runtime validation at any API boundary** (found
   2026-08-02) — every gateway response (Brreg search, crawl SSE packets,
   checkout, plan recommendation) is trusted via a bare type cast, directly
   contradicting this project's own "validate all external data at system
   boundaries with Zod" rule. Compounding this, `OnboardingPage.tsx` (1,096
   lines: org creation, crawl, connector OAuth, checkout, session
   finalization all in one orchestrator) has almost no test coverage for its
   actual logic — one test covers one guard clause. Two more, smaller
   onboarding findings worth a fast fix: a full sign-out + state-wipe is
   triggered by an undifferentiated back-arrow with no confirmation, and a
   code comment asserting "ZDR defaults ON" contradicts the actual shipped
   default (OFF) — fix the comment before it misleads someone auditing this
   path.
10. **Two CLAUDE.md-mandated architectural contracts are fully unwired**
    (found 2026-08-02): `buildModelContextPack` (the context-packs contract
    this project's own CLAUDE.md names as load-bearing) is called only from
    its own unit test — no feature or action handler uses it. `selectModelTier`
    (the client-side cost-policy module) has zero callers anywhere, including
    tests. Both currently do nothing for a real user despite being presented
    as part of the architecture. Either wire them into the flows they were
    designed for, or delete them — a false signal of "this control exists" is
    worse than an honestly-absent one, per this project's own stated values.
11. **The biggest files in the repo are also the least tested** (found
    2026-08-02): `DashboardComposer.tsx` (2,586 lines), `BrowserChrome.tsx`
    (2,104), `WorkspaceSettingsPage.tsx` (2,053), `KnowledgeComposer.tsx`
    (1,949), `AgentRunConsole.tsx` (1,782), `use-chat-controller.ts` (1,416),
    `OnboardingPage.tsx` (1,096) — 21 files over the project's 800-line rule,
    6 of them over 1,500 — and every one of the largest six has **zero**
    component-level tests. This is not a coincidence: a file this size is
    both the hardest to safely refactor and the least likely to have been
    tested along the way. Treat file-size and test-coverage debt as one
    problem, not two — splitting a mega-file is also how it becomes testable.

### 3a. Down-the-stack findings (2026-08-02) — three CRITICAL, rank above the rest of Phase A

A full code-health pass across the remaining 5 planes (Model Plane Rust +
Go, Data Plane v2, Control Plane, Ingestion Plane, Application Plane)
surfaced 29 findings. Three are **critical severity** and should be treated
as ranking above everything else in this Phase — not because the process
says so, but because they are live, wired, currently-exploitable-in-shape
gaps in exactly the properties `velion-vision.md` §2 names as the moat
(approvable execution, trust boundaries):

1. **Quarry-v2's headless-browser driver (`chromiumoxide.rs`) performs zero
   SSRF enforcement of its own.** `goto()` and `open_tab_page()` call the CDP
   browser directly with no call into `quarry_security`/`dns_guard` anywhere
   in the file. The crate's own `tests/ssrf.rs` asserts `goto()` must reject
   metadata/loopback/private URLs — an assertion the implementation cannot
   satisfy, and every test in that file is permanently `#[ignore]`d, so this
   has never been caught by CI. This is the single most consequential
   finding of the whole audit: CLAUDE.md names Quarry-v2 as the trusted
   arbiter of browser actions ("Model may propose browser actions; Quarry-v2
   executes or rejects them") — a browser driver with no SSRF check of its
   own means that arbitration is not actually happening for anything routed
   through the headless path, including sub-resource requests the rendered
   page itself issues (JS fetch/XHR/iframe/img — the classic browser-SSRF
   vector), which no check anywhere in the pipeline covers.
2. **The static fetch driver auto-follows redirects with no SSRF re-check.**
   `reqwest`'s `Policy::limited(5)` follows up to 5 redirects with no
   callback; the one preflight+DNS-guard check runs once, on the original
   URL, before the fetch. A 301/302 to a private/metadata address is
   followed transparently. imports-core's own connector code already has the
   right discipline (`follow_redirects=False`, raises on any redirect) —
   apply the same discipline to Quarry's primary crawl driver.
3. **social-core's live publish path bypasses integration-corev2's actions
   surface entirely, for every provider write.** social-core already has a
   correct `ExecuteAction()` implementation and uses it for reads. The
   highest-risk operations — actually publishing to LinkedIn/Meta/TikTok/
   Snapchat — instead lease a raw third-party OAuth token and call each
   provider's API directly. This is wired in production (`main.go`
   constructs one `integrationClient` and passes it as both the raw-token
   broker AND the correct action executor to the same service) — a live
   split, not disused code. This is the single architecture rule this
   monorepo states most explicitly, violated on the highest-risk path in the
   one service most exposed to it.

**One more HIGH finding worth flagging above the rest, because it is
systemic, not local**: the DNS-rebinding TOCTOU gap (resolve, check, then
hand a re-resolvable *hostname* — not the checked IP — to the actual
connection) exists **independently in both** Quarry-v2's Rust `dns_guard.rs`
and imports-core's Python `network_policy.py`. The same architectural
mistake was made twice, in two languages, in the same plane — worth fixing
once, in a way that can't recur (pin the checked IP into the connection,
don't re-resolve).

**Everything else from this pass** (29 findings total; the remaining 25 are
HIGH/MEDIUM/LOW) is real and worth doing, but does not carry the same
urgency as the three above. Highlights, grouped by theme rather than listed
individually — read the audit output for full file:line detail if picking
one up:
- **Trust-boundary drift across Application Plane siblings**: conversation-
  core-go has a mature HMAC-delegation-with-replay-protection pattern;
  insight-core and social-core instead trust a client-supplied `org_id`
  behind one fleet-wide shared static key (today's live exploit path is
  closed because the gateway resolves org_id server-side before calling
  them, but the services themselves have no defense if that changes); convex-
  core exposes tenant-scoped mutations as public Convex functions gated by a
  non-constant-time secret compare instead of `internalMutation`.
  session-core is the one Control Plane Go service whose HMAC delegation
  never got the anti-replay nonce every sibling service already has.
- **A documented HITL safety invariant is violated, not just untested**: the
  browser-agent risk classifier's "self-report OR deterministic backstop" is
  actually "self-report short-circuits the backstop" — a model that
  under-reports risk (not just omits it) can cause a checkout/destructive
  action to reach the human approver mislabeled as low-risk. The gate still
  fires; what the human reads can be wrong.
- **Architecture-rule bypass, smaller scale**: user-core calls Microsoft
  Graph directly with a raw user token instead of through integration-
  corev2's existing `microsoft.profile` operation — the same class of
  violation as social-core above, one plane over.
- **A DSAR/erasure-relevant correctness gap**: letta-bridge's Postgres memory
  tier makes `DeleteMemory` a permanent silent no-op (the table has no
  per-user ownership column), indistinguishable on the wire from "already
  deleted." Any erasure workflow that trusts this response leaves rows
  behind indefinitely.
- **Error-swallowing that produces false success**: 13 call sites across
  capability-core's PATCH/DELETE handlers discard both the SQL error and the
  rows-affected count, so a mutation against a wrong/foreign/nonexistent id
  still returns 200. For `safety_policies`/`routing_policies` specifically,
  an operator "disabling" a policy can get a success response while the
  policy stays active.
- **Duplicated trust-critical code**: 4 Go services in Data Plane v2 each
  independently reimplement ~350-460 lines of JWT-verification middleware
  (already drifted — two of the four carry claims the other two don't); the
  same weak org_id-trust boilerplate is duplicated verbatim between insight-
  core and social-core.
- **One inconsistency in an otherwise well-hardened plane**: Data Plane v2's
  embedding and graph-extraction calls were correctly hardened to route
  through Model Plane token-minting; reranking (Cohere/Azure) was not, and
  still holds a locally-held API key with no audit trail for non-ZDR content.
- **Maintainability, same pattern as velionv3**: several more god-files past
  the 800-line rule (user-core's handlers.go at 1,944 lines; conversation-
  core-go's repository.go at 2,769; capability-core's registry_apis.go at
  1,574) — the same file-size-correlates-with-test-coverage-gap pattern
  found in Phase A item 11 recurs down the stack too.

## 4. Phase B — Make the moat visible (GTM, from the vision doc)

Per `velion-vision.md` §3: the Trust Center is the most rigorous of the
entire competitor set and was, until 2026-07-20, invisible. A first pass
shipped (an eyebrow line, a second CTA, a named-partner line). What remains,
in priority order:

1. **Decide the `BrandLogosSection` connector-carousel question** (open per
   `COMPETITOR-ANALYSIS.md`'s "Open, not yet done" note): confirm which
   connectors are genuinely live/credentialed in production (not mock
   fallback) before adding Vipps/Bring/UPS/DHL/FedEx/Meta/X/LinkedIn, and
   drop Altinn (zero code found). Bring specifically was flagged
   "unverified" in an earlier audit — re-check before including it.
2. **Do not upgrade the sovereignty claim** from "EU/EØS" to a
   Norwegian-infrastructure claim until the Telenor AI Factory conversation
   closes (still open, per project memory — this is a hold, not a task).
3. **Apply the curated-wedge per-competitor counter-table**
   (`velion-feature-map.md` §6.6) concretely: Ayfie (acting agent they lack),
   Mimir (visible approval step they don't show), Chatbase (match the <15min
   time-to-value number, it's testable), Intercom (same shape, SMB-priced,
   paused-approved action instead of a trust-us number). These are
   marketing/positioning tasks, not engineering ones, and belong on a GTM
   team's plan, referencing this roadmap as the source.
4. **"Velion Support" packaging** (vision §4): name and package the
   existing Inbox + Ticketing + AI-draft-HITL + embeddable-widget work as a
   curated Intercom-equivalent, once Phase A's ticketing fixes land and the
   post-approval-continuation P0 closes — packaging a differentiator before
   the differentiator itself is fully solid would be premature.

## 5. Phase C — The pilot security gate (unchanged in substance from feature-map §5 Phase 3, updated)

Everything `velion-feature-map.md` §5 Phase 3 already named, now with two
additions from this week/today:
- Full IDOR closure — **now explicitly including the cross-tenant approval
  gap found today (§3.2 above)**, not only the header-based org-id class
  already closed.
- RLS active + enforce-mode auth; erasure/DSAR purge subscriber (a
  product/legal decision on scope is still the blocker, not code); Data
  Plane credential rotation (operator-owned, out of agent scope by design);
  a real CI gate on the cross-tenant regression tests that exist but are not
  wired into any workflow GitHub Actions actually reads.
- Image provenance: extend the git-SHA-tag + `/version` pattern already
  done for 5 slice services (feature-map §6.7) to the rest of the estate.

## 6. Phase D — Open pilot, then pull-driven expansion (unchanged from feature-map §5 Phase 4-5)

No new information this week changes this sequencing. Restated once, briefly,
so this doc is self-contained: self-serve onboarding, NOK pricing, Norwegian
UI pass, 10 external users active weekly with honest Insights numbers — then
expansion strictly pull-driven (Agents canvas only on demonstrated
preset-modification demand; Insights depth; more inbox channels; Social
live-proof; Norwegian AI-visibility as a new bet; Zendesk-parity extras).
`velion-feature-map.md` §5 has the full gate language; it is not repeated
here to avoid the two docs drifting out of sync — treat that section as the
canonical detail, this roadmap as the "what changed, what's next" layer on
top of it.

## 7. The code-health audit — status, and how the rest resumes

This roadmap was supposed to be grounded in a full code audit "starting
with velionv3, working down the stack." **Update: velionv3 (Stage 1) is now
complete.** The section below is kept as the honest record of how it
actually went, including the interruption, rather than rewritten as if it
had gone smoothly — the interruption and recovery are themselves useful
information for whoever runs the next one of these.

### 7.1 What actually ran
A parallel 8-agent pass across velionv3 covering tickets, agents-console,
settings-integrations, social-studio, onboarding-auth, shared-infra,
gateway-bff, and a cross-cutting dead-code/test-coverage sweep. **7 of 8
agents hit a hard subagent-usage quota mid-investigation** (each had already
made 16-31 tool calls) with the error `You've hit your session limit ·
resets 2am (Europe/Oslo)`. Only the `tickets` agent completed on the first
pass — its findings are folded into §3.1 and `velion-feature-map.md` §1.4.

**The quota reset and the remaining 6 areas were re-run successfully**,
producing 42 more findings — folded into §3 items 5-11 above and
`velion-feature-map.md` §1.2/§1.6/§1.7/§1.8/§1.10. Stage 2 (the "down the
stack" pass across Model Plane, Data Plane v2, Control Plane, Ingestion
Plane, and Application Plane) was launched immediately after and its status
is recorded in §7.3.

### 7.2 What was done manually instead
Rather than wait idle or grind through the remaining 7 velionv3 areas plus
5-6 more planes entirely by hand (an enormous, context-expensive undertaking
better suited to the parallel approach once it's available again), one
targeted manual pass was done on the single highest-security-value item: the
Rust gateway BFF (`apps/gateway`), since CLAUDE.md explicitly calls out its
secret/token/header-forwarding hygiene as a hard requirement. That surfaced
the cross-tenant approval IDOR in §3.2. The adjacent org-id/token-minting
paths (`mcp.rs`, `model_token`/`required_capability_token` in
`chat/shared.rs`) were checked and confirmed clean — correctly keyed off
`user_id` + verified session, not a client-controllable org field.

### 7.3 Final status: complete
Stage 2 ran successfully on the first attempt after the quota reset — 6/6
service groups (Model Plane Rust, Model Plane Go, Data Plane v2, Control
Plane, Ingestion Plane, Application Plane), 29 findings, folded into §3a
above and `velion-feature-map.md` §2 item 8. **Both stages of the
"velionv3 first, then work your way down" audit are now done**: 8 velionv3
areas (7 findings from the first pass + 42 from the continuation after the
quota reset, 49 total) + 1 manual gateway spot-check (the cross-tenant
approval IDOR) + 6 down-the-stack service groups (29 findings) — **79
findings in total** across the two source workflow outputs. Not every one
made it into these docs verbatim; the highest-severity and most
consequential ones did, per this section's own §7.1/§7.2 process — the full
per-finding detail (file:line, exact recommendation) lives in the workflow
transcripts if a smaller item needs picking up later.

This roadmap's §1-§6 now rest on a genuinely broad evidence base spanning
the full monorepo, not just this session's own chat-feature work — the
"partial evidence" caveat earlier versions of this section carried no longer
applies. What was NOT done, and would be the natural next pass: acting on
the findings themselves (this audit found and documented, it did not fix,
with the exception of the chat-resume/edit-version work and doc corrections
already shipped this session) — that is what §3/§3a/§5 are for.

## 8. Explicitly not now

Unchanged from `velion-feature-map.md` §5 and `velion-vision.md` §5:
n8n-parity builder, full Google-class search, classic SEO, Studio Plane
services, Channel Plane, meeting notes/voice, enterprise-only items
(mTLS/SPIFFE, HA/DR, multi-region, SOC2) — sequenced after revenue, not
before. Upgrading the sovereignty claim beyond "EU/EØS" — held on the
Telenor AI Factory outcome, not a timeline.
