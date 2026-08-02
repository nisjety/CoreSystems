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

## 7. The interrupted code-health audit — real status, and how to resume it

This roadmap was supposed to be grounded in a full code audit "starting
with velionv3, working down the stack." That audit is **partial**, and this
section says so precisely rather than presenting Phase A-D above as if they
rested on complete evidence.

### 7.1 What actually ran
A parallel 8-agent pass across velionv3 covering tickets, agents-console,
settings-integrations, social-studio, onboarding-auth, shared-infra,
gateway-bff, and a cross-cutting dead-code/test-coverage sweep. **7 of 8
agents hit a hard subagent-usage quota mid-investigation** (each had already
made 16-31 tool calls) with the error `You've hit your session limit ·
resets 2am (Europe/Oslo)`. Only the `tickets` agent completed — its findings
are folded into §3.1 and `velion-feature-map.md` §1.4.

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

### 7.3 What did NOT run at all
- velionv3: agents-console, settings-integrations, social-studio,
  onboarding-auth, shared-infra (beyond the one gateway spot-check),
  cross-cutting dead-code/TODO/test-coverage sweep.
- The entire "work your way down" pass: Model Plane (Rust: model-gateway/
  inference-core/execution-core; Go: session-core/orchestrator-core/
  capability-core/letta-bridge), Data Plane v2, Control Plane, Ingestion
  Plane, Application Plane.

### 7.4 How to resume
The quota resets at 2am Europe/Oslo. To pick this back up:
1. Re-run the velionv3 Stage 1 workflow for the 7 areas that failed (the
   script is preserved and can be re-invoked with the cached `tickets`
   result reused rather than re-run).
2. Run the "down the stack" Stage 2 pass across the 5 remaining planes,
   same code-health lens (architecture-rule violations against CLAUDE.md,
   security, test coverage, dead code, maintainability), one agent per
   plane at minimum, splitting Model Plane into Rust/Go given its size.
3. Flush findings into `velion-feature-map.md`'s per-feature "Honest gaps"
   sections and this roadmap's §3 as each stage completes — do not batch
   until the very end, in case of another interruption.
4. Re-check whether §3-§6 above need updating once broader findings land —
   this version of the roadmap is grounded in vision + status docs + one
   completed area + one manual spot-check, which is a real but partial
   evidence base, honestly represented.

## 8. Explicitly not now

Unchanged from `velion-feature-map.md` §5 and `velion-vision.md` §5:
n8n-parity builder, full Google-class search, classic SEO, Studio Plane
services, Channel Plane, meeting notes/voice, enterprise-only items
(mTLS/SPIFFE, HA/DR, multi-region, SOC2) — sequenced after revenue, not
before. Upgrading the sovereignty claim beyond "EU/EØS" — held on the
Telenor AI Factory outcome, not a timeline.
