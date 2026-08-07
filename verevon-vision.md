# Verevon Vision

**Last updated:** 2026-08-03. Builds on `apps/Frontend Plane/verevonv3/apps/verevon-web/COMPETITOR-ANALYSIS.md`
(2026-07-20, verevon-web homepage vs. 10 Nordic + world competitors) and
`verevon-feature-map.md` §6.4–6.6a (the "north star," the curated-wedge
doctrine, and the two-trust-levers pattern). This document exists to answer
one question plainly: **what should Verevon become**, given what the
competitive landscape actually rewards and what Verevon's architecture
actually makes possible. `verevon-roadmap.md` turns this into sequenced work.

**Implementation boundary:** the current roadmap is executed locally in
Docker. The team should rebuild and run the updated stack, apply migrations,
verify implemented flows, fix runtime bugs, and continue iterating. The vision
does not call for an external production launch before the local stack and its
core workflows are complete and stable.

**Current execution ledger — 2026-08-05.** The Vision remains the product
direction. Its current implementation status and ordered delivery work are
maintained in
[verevon-roadmap.md](verevon-roadmap.md#current-execution-ledger--2026-08-05),
which resolves conflicting dated status claims across the companion documents.

**Architecture source set — 2026-08-03:**

- `/Volumes/Lagring/Triodelab/CoreSystem/apps/Ingestion Plane/QUARRY_V2_BROWSER_AUTOMATION_IMPROVEMENTS_2026.md`
- `/Volumes/Lagring/Triodelab/CoreSystem/apps/Model Plane/docs/MODEL_PLANE_IMPROVEMENTS_2026.md`

These sources sharpen rather than replace the vision: Quarry owns secure web
evidence and browser execution, Data Plane owns durable knowledge and
organizational memory, Model Plane owns reasoning/planning/proposals, and
Control/Application own identity, durable intent, policy, approval, and human
review. Mem0 is a Data Plane design donor and benchmark target, not Verevon's
canonical memory authority.

---

## 1. The thesis

Verevon is an **AI-first organization workbench**, not a support-ticket tool
wearing an AI badge. The operative claim, restated precisely: **the chat
surface is the platform** — every capability a human can reach through a
work surface (dashboard, inbox, tickets, agents, knowledge, social, studio)
is also reachable as an API the chat/agent loop can call, under the same
governance (approval, audit, cost, retention) a human-driven action would
go through. "Every UI is an API" is not a slogan here — as of 2026-08-01 it
is demonstrably true for a growing slice of the surface: chat can look up a
shipping quote, query a connected ERP via MCP, run a sandboxed code
interpreter, generate a real spreadsheet, and search the org's own knowledge
base — all through the same conversational entry point a human uses elsewhere
in the product.

The architecture synthesis adds one precision: chat is the **universal
governed orchestration surface**, not a second authority and not a replacement
for understandable manual controls. Manual UI, chat/agent, and direct typed API
paths should converge on the same backend action, policy decision, verifier,
and audit record.

The core loop, expanded from the 2026-07-22 product-model correction:
**connect → understand → search → decide → act → approve → verify → audit → improve.**
Five capability layers underneath it — sources, knowledge/context, reasoning/
action, governance, work surfaces — are the actual architecture; the eleven
named feature areas in `verevon-feature-map.md` are _expressions_ of those five
layers, not the product itself. This distinction matters for the vision:
Verevon should be judged, and built, by how well it deepens the five layers,
not by how many named surfaces it ships.

## 2. What the moat actually is (not aspirational — architectural)

Per `verevon-feature-map.md` §6.4, strip every named feature away and four
properties remain that no single competitor currently combines:

1. **Grounded retrieval with real source traces and governed organizational
   memory.** Hybrid retrieval + GraphRAG + wiki + visual evidence + citations,
   extended with provenance-bearing memory claims, temporal validity,
   contradiction, supersession, retention, and deletion proof — not a
   vector-search demo or an ADD-only pile of extracted facts.
2. **Server-enforced, visible, approvable agent execution.** A paused tool
   call with exact payload, authority, cost, source, and expected effect
   attached, waiting on a human — not a vendor's opaque resolution-rate claim.
3. **Verified outcomes rather than model activity.** Quarry/browser,
   capability, integration, and provider effects produce receipts and
   deterministic postconditions. A Verevon Proof Bundle can show what was
   known, decided, authorized, executed, observed, verified, retained, and
   charged.
4. **EU/Norway residency and audit as a plane contract**, not a compliance PDF
   bolted on after the fact — ZDR posture, cross-plane audit-core, GDPR metadata
   propagation, and deletion scope enforced boundary-by-boundary. Today this
   is architecturally governed and sovereignty-aware; it is not yet equivalent
   to Norwegian-owned infrastructure.

Ayfie has #1 without #2 or #3. Intercom/Zendesk/Gorgias have automation
without #2's transparency, #3's outcome proof, or #4's EU-native architecture.
Chatbase/agenci-shaped RAG-widget products have #1 cheaply but own none of the
pipeline underneath, so they cannot show a customer the actual evidence and
effect chain. **This is a capability the source is built for; §6.9's
security-gate work, the engine-liveness proof, and the new Quarry/Model Plane
improvement programs are what convert it from "the source is built for this"
into "the running system has demonstrated this repeatedly."** The gap between
those states is exactly what `verevon-roadmap.md` sequences.

### 2.1 The proof-and-quality layer the new findings add

The architecture reviews add a common substrate beneath every wedge rather
than another customer-facing module:

```text
Quarry evidence and browser receipts
+ Data Plane source, retrieval, graph, and memory provenance
+ Model Plane plan, skill/procedure, model, and harness decisions
+ Control/Application identity, policy, approval, and correction
+ Integration/provider effect receipts
        ↓
Runtime Evidence Manifest
        ↓
Verevon Proof Bundle
        ↓
Agent Quality OS: replay, simulation, shadow, canary, promotion, rollback
```

This layer is how Verevon improves quality without simply buying a larger model.
It should measure verified completion, false success, evidence support, human
effort, intervention, unnecessary approval, cost and time per verified outcome,
and rollback rate.

## 3. The competitive landscape — condensed, current as of 2026-07-20

Full detail lives in `COMPETITOR-ANALYSIS.md` (verevon-web homepage) and
`verevon-feature-map.md` §6.6/§6.6a (product-level, 6-competitor structural
check). The findings converge on one non-negotiable pattern:

**Every winning Nordic/EU AI company markets at least one of two trust
levers — an explicit sovereignty/compliance claim, or an explicit
approval/control mechanic.** Checked across Ayfie, boost.ai, Semine,
Simplifai, Sana Labs, Mimir, and (via the homepage pass) Cobrief, Taito,
Wonderful — zero exceptions. The 6 "world" competitors (Attio, Peec,
Intercom, Chatbase, Gorgias, Zendesk) instead pair self-serve entry with
published pricing and lean on scale/social proof — confirming the
sovereignty lever specifically is a **Nordic/EU-buyer concern**, not a
universal SaaS default, and should not be over-generalized outside that
market.

**Verevon clears both levers architecturally (§2 above) but, as of the last
homepage audit, was the only site in the set where the trust lever was real
and well-documented yet invisible** — the Trust Center at `/trust` is, on
inspection, the most rigorous of the entire set (it names live-vs-planned
controls individually and actively undercuts its own sovereignty claim:
_"EU residency is not data sovereignty... reachable under the US CLOUD Act"_
— no competitor admits a gap like that) — but it sat behind a plain-text
"TILLIT" nav label, three clicks deep. A first, minimal fix shipped
2026-07-20 (an eyebrow line + a second CTA + a named-partner line on the
homepage hero, deliberately NOT a UI-mockup/logo-wall — that would have made
Verevon look like the rest of the set instead of itself). The underlying gap —
the Trust Center being real but under-marketed — is structural, not a one-page
fix, and is the highest-leverage GTM item this vision identifies.

**One sovereignty gap Verevon has not closed**: Ayfie + Telenor AI Factory
(announced 2026-02-23) can claim data processed on **Norwegian-owned
infrastructure**; Verevon runs on Azure OpenAI Sweden Central — an EU/Nordic
region, but Azure is a US hyperscaler regardless of region. Verevon's own
Telenor AI Factory conversation is in contact, not closed (per project
memory, still open as of this vision's writing) — **do not upgrade any copy
claim until it closes.** What still holds regardless of who wins on
sovereignty: Ayfie has no acting, approvable agent product, no inbox, no
ticketing — that gap is Verevon's regardless.

**Curated-wedge doctrine (§6.6, unchanged and re-confirmed by §6.6a's
cross-check of 6 Nordic peers)**: Verevon cannot and should not chase full
feature parity with any competitor. Pick the one best-in-class feature per
category, make it better on the axis that matters (grounding / approvability /
verification / trust), consciously skip the rest. The per-competitor
counter-table in §6.6 is the operative reference — it does not need restating
here, it needs to be **applied** in the roadmap.

## 4. What "Verevon Support" should be (§6.5, restated as vision, not a feature)

Package the existing Inbox + Ticketing + AI-draft-HITL + embeddable-widget
work as a **named, curated Intercom-equivalent** — precision matters for the
pitch. The wedge is **not** "GDPR forbids Intercom" (false, and overclaiming
it is exactly the trust-misrepresentation failure mode this doc's own audits
have flagged elsewhere). The real, defensible wedge is US CLOUD Act reach +
Schrems-II-era sovereignty anxiety + Intercom's per-seat cost ($55–115+/
agent on AI-capable tiers) — a combination Ayfie already proves converts at
Norwegian enterprise scale even without an inbox product at all. Verevon's
position: **Ayfie's EU trust posture, with an actual support product Ayfie
doesn't have.**

Support should also become the first complete quality laboratory: human-
readable procedures, stateful provider/browser simulations, shadow execution,
verified send/delivery outcomes, memory updates tied to receipts, and cost per
verified resolution.

## 5. What Verevon should NOT become

Restated because it is as load-bearing as what to build:

- **Not a feature-parity suite.** §6.6a's pattern check found _zero functional
  overlap_ between any of the 6 Nordic AI companies studied — one narrow wedge,
  never a combined suite, is how this exact market goes to market. Verevon's own
  architecture (owning inbox+KB+agents+search at once) is structurally the
  _opposite_ of the regional norm — which is precisely why shipping one wedge
  deep matters beyond team-capacity reasons; it is also the credible
  go-to-market shape.
- **Not a workflow-builder company.** The n8n-style canvas
  (`WorkflowBuilder`/`ChatbotStudio`/`ChatbotPlayground`) is commodity UI on top
  of the real differentiator (HITL + audit + cost + proof primitives). Build it
  only on demonstrated pilot pull to modify presets, never speculatively.
  Human-readable Procedures/Skills should compile into capability, retrieval,
  approval, verification, rollback, and simulation contracts first.
- **Not a classic-SEO company.** That market is lost to Ahrefs/Semrush. If a
  Norwegian AI-answer-visibility niche (Peec's category) is ever pursued, it
  ships as a Verevon-native module reusing Quarry + the knowledge base, not a
  separate branded product — see the ecosystem addendum in
  `verevon-feature-map.md` §6.1–6.3 for the full reasoning (avelis, agenci, and
  the Aquatiq integration fleet all resolve the same way: harvest capabilities,
  do not port runtimes, do not merge products prematurely).
- **Not a sovereignty-claim overclaim.** EU residency (Sweden Central) is real;
  "fully Norwegian infrastructure" is not, until a Telenor-class deal actually
  closes. The Trust Center's own honesty about this gap is an asset — preserve
  it, don't paper over it for a punchier headline.
- **Not an agent-framework or memory-service wrapper.** Verevon owns authority,
  evidence, durable knowledge, procedures, execution contracts, verification,
  evaluation, and customer UX. External systems are adapters, labs, design
  donors, or benchmarks.
- **Not uncontrolled self-improvement.** Production corrections may create
  candidate prompts, procedures, skills, routes, and evals, but deterministic
  gates and human policy control promotion.

## 6. The forward vision, in one paragraph

Verevon in its intended end-state is the org's single AI-first control surface:
every employee action a human currently performs by hand across inbox,
tickets, knowledge, internal systems, and the web has a chat-reachable,
typed, governed, auditable equivalent — proposed by an agent, grounded in the
org's own retrieved knowledge and memory with real citations, paused for
approval when it touches something consequential, executed through the plane
that owns the effect, independently verified against the resulting state,
priced and logged either way, and packaged into a portable proof bundle.

The product does not compete on having the most features or the most autonomous
model; it competes on being the only option in the Norwegian/EU market that is
simultaneously **grounded** (not guessing), **acting** (not just answering),
**approvable** (not a black-box resolution rate), **verifiable** (not a claimed
success), and **architecturally governed and sovereignty-aware** (not a
compliance page bolted onto an uncontrolled stack). Everything in
`verevon-roadmap.md` is sequenced against closing the gap between that end-state
and what is running today.

## 7. The improvement flywheel

Verevon should become easier and better to build with every verified task:

```text
production trace / human correction / incident / knowledge change
→ classify the real gap
→ create or update a deterministic case and stateful simulation
→ test retrieval, memory, model, skill/procedure, tool, browser, and policy candidates
→ shadow or canary
→ promote or roll back
→ preserve the proof and runtime manifest
```

The platform should distinguish knowledge, retrieval, contradiction,
data-access, capability, procedure, policy, approval, verification, model,
infrastructure, and security gaps. This **Action Readiness Map** turns failures
into ranked improvements rather than generic thumbs-down feedback.

## 8. Plane responsibilities in the end state

- **Quarry captures evidence and executes browser procedures.** It owns secure
  acquisition, runtime selection, observations, adaptive targets, challenge
  intelligence, typed browser actions, change impact, deterministic browser
  verification, and browser proof receipts.
- **Data Plane knows.** It owns documents, chunks, embeddings, retrieval,
  GraphRAG, wiki, source logs, canonical organizational memory,
  validity/supersession/contradiction, retention, and deletion of canonical
  plus derived data. Mem0, Zep/Graphiti, Letta, Cognee, and similar systems are
  references or adapters, not authorities.
- **Model Plane reasons.** It owns context assembly, model and capability
  routing, plans, procedures/skills, tool use, recovery, memory-write
  proposals, adaptive compute, and quality/evaluation workflows.
- **Control/Application own identity, intent, and review.** They resolve scope,
  policy, durable human approval, correction, and the customer-facing
  proof/quality experience.
- **Integration owns non-browser provider effects and authoritative provider
  receipts.**
