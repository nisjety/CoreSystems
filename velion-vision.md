# Velion Vision

**Last updated:** 2026-08-02. Builds on `apps/Frontend Plane/velionv3/apps/velion-web/COMPETITOR-ANALYSIS.md`
(2026-07-20, velion-web homepage vs. 10 Nordic + world competitors) and
`velion-feature-map.md` §6.4–6.6a (the "north star," the curated-wedge
doctrine, and the two-trust-levers pattern). This document exists to answer
one question plainly: **what should Velion become**, given what the
competitive landscape actually rewards and what Velion's architecture
actually makes possible. `velion-roadmap.md` turns this into sequenced work.

---

## 1. The thesis

Velion is an **AI-first organization workbench**, not a support-ticket tool
wearing an AI badge. The operative claim, restated precisely: **the chat
surface is the platform** — every capability a human can reach through a
work surface (dashboard, inbox, tickets, agents, knowledge, social, studio)
is also reachable as an API the chat/agent loop can call, under the same
governance (approval, audit, cost, retention) a human-driven action would
go through. "Every UI is an API" is not a slogan here — as of 2026-08-01 it
is demonstrably true for a growing slice of the surface: chat can look up a
shipping quote, query a connected ERP via MCP, run a sandboxed code
interpreter, generate a real spreadsheet, and search the org's own knowledge
base — all through the same conversational entry point a human docs into
manually elsewhere in the product.

The core loop, unchanged since the 2026-07-22 product-model correction:
**connect → understand → search → decide → act → approve → audit.** Five
capability layers underneath it — sources, knowledge/context, reasoning/
action, governance, work surfaces — are the actual architecture; the eleven
named feature areas in `velion-feature-map.md` are *expressions* of those
five layers, not the product itself. This distinction matters for the
vision: Velion should be judged, and built, by how well it deepens the five
layers, not by how many named surfaces it ships.

## 2. What the moat actually is (not aspirational — architectural)

Per `velion-feature-map.md` §6.4, strip every named feature away and three
properties remain that no single competitor currently combines:

1. **Grounded retrieval with real source traces and a knowledge graph.**
   Hybrid retrieval + GraphRAG + wiki + citations, not a vector-search demo.
2. **Server-enforced, visible, approvable agent execution.** A paused tool
   call with cost and source attached, waiting on a human — not a vendor's
   opaque resolution-rate claim.
3. **EU/Norway residency and audit as a plane *contract***, not a compliance
   PDF bolted on after the fact — ZDR posture, cross-plane audit-core, GDPR
   metadata propagation enforced by architectural rule, independently
   verified boundary-by-boundary (see `apps/Model Plane/docs/ZDR.md`).

Ayfie has #1 without #2. Intercom/Zendesk/Gorgias have automation without
#2's transparency or #3's EU-native architecture. Chatbase/agenci-shaped
RAG-widget products have #1 cheaply but own none of the pipeline underneath,
so they can never show a customer the actual evidence chain. **This is a
capability the source is built for; §6.9's security-gate work and this
week's engine-liveness proof are what convert it from "the source is built
for this" into "the running system has demonstrated it."** The gap between
those two states is exactly what `velion-roadmap.md` sequences.

## 3. The competitive landscape — condensed, current as of 2026-07-20

Full detail lives in `COMPETITOR-ANALYSIS.md` (velion-web homepage) and
`velion-feature-map.md` §6.6/§6.6a (product-level, 6-competitor structural
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

**Velion clears both levers architecturally (§2 above) but, as of the last
homepage audit, was the only site in the set where the trust lever was real
and well-documented yet invisible** — the Trust Center at `/trust` is, on
inspection, the most rigorous of the entire set (it names live-vs-planned
controls individually and actively undercuts its own sovereignty claim: *"EU
residency is not data sovereignty... reachable under the US CLOUD Act"* —
no competitor admits a gap like that) — but it sat behind a plain-text
"TILLIT" nav label, three clicks deep. A first, minimal fix shipped
2026-07-20 (an eyebrow line + a second CTA + a named-partner line on the
homepage hero, deliberately NOT a UI-mockup/logo-wall — that would have made
Velion look like the rest of the set instead of itself). The underlying
gap — the Trust Center being real but under-marketed — is structural, not a
one-page fix, and is the highest-leverage GTM item this vision identifies.

**One sovereignty gap Velion has not closed**: Ayfie + Telenor AI Factory
(announced 2026-02-23) can claim data processed on **Norwegian-owned
infrastructure**; Velion runs on Azure OpenAI Sweden Central — an EU/Nordic
region, but Azure is a US hyperscaler regardless of region. Velion's own
Telenor AI Factory conversation is in contact, not closed (per project
memory, still open as of this vision's writing) — **do not upgrade any
copy claim until it closes.** What still holds regardless of who wins on
sovereignty: Ayfie has no acting, approvable agent product, no inbox, no
ticketing — that gap is Velion's regardless.

**Curated-wedge doctrine (§6.6, unchanged and re-confirmed by §6.6a's
cross-check of 6 Nordic peers)**: Velion cannot and should not chase full
feature parity with any competitor. Pick the one best-in-class feature per
category, make it better on the axis that matters (grounding /
approvability / trust), consciously skip the rest. The per-competitor
counter-table in §6.6 is the operative reference — it does not need
restating here, it needs to be **applied** in the roadmap.

## 4. What "Velion Support" should be (§6.5, restated as vision, not a feature)

Package the existing Inbox + Ticketing + AI-draft-HITL + embeddable-widget
work as a **named, curated Intercom-equivalent** — precision matters for the
pitch. The wedge is **not** "GDPR forbids Intercom" (false, and overclaiming
it is exactly the trust-misrepresentation failure mode this doc's own audits
have flagged elsewhere). The real, defensible wedge is US CLOUD Act reach +
Schrems-II-era sovereignty anxiety + Intercom's per-seat cost ($55–115+/
agent on AI-capable tiers) — a combination Ayfie already proves converts at
Norwegian enterprise scale even without an inbox product at all. Velion's
position: **Ayfie's EU trust posture, with an actual support product Ayfie
doesn't have.**

## 5. What Velion should NOT become

Restated because it is as load-bearing as what to build:

- **Not a feature-parity suite.** §6.6a's pattern check found *zero
  functional overlap* between any of the 6 Nordic AI companies studied —
  one narrow wedge, never a combined suite, is how this exact market goes
  to market. Velion's own architecture (owning inbox+KB+agents+search at
  once) is structurally the *opposite* of the regional norm — which is
  precisely why shipping one wedge deep matters beyond team-capacity
  reasons; it is also the credible go-to-market shape.
- **Not a workflow-builder company.** The n8n-style canvas
  (`WorkflowBuilder`/`ChatbotStudio`/`ChatbotPlayground`) is commodity UI on
  top of the real differentiator (HITL + audit + cost primitives). Build it
  only on demonstrated pilot pull to modify presets, never speculatively.
- **Not a classic-SEO company.** That market is lost to Ahrefs/Semrush.
  If a Norwegian AI-answer-visibility niche (Peec's category) is ever
  pursued, it ships as a Velion-native module reusing Quarry + the
  knowledge base, not a separate branded product — see the ecosystem
  addendum in `velion-feature-map.md` §6.1–6.3 for the full reasoning
  (avelis, agenci, and the Aquatiq integration fleet all resolve the same
  way: harvest capabilities, do not port runtimes, do not merge products
  prematurely).
- **Not a sovereignty-claim overclaim.** EU residency (Sweden Central) is
  real; "fully Norwegian infrastructure" is not, until a Telenor-class deal
  actually closes. The Trust Center's own honesty about this gap is an
  asset — preserve it, don't paper over it for a punchier headline.

## 6. The forward vision, in one paragraph

Velion in its intended end-state is the org's single AI-first control
surface: every employee action a human currently performs by hand across
inbox, tickets, knowledge, and internal systems has a chat-reachable,
governed, auditable equivalent — proposed by an agent, grounded in the org's
own retrieved knowledge with real citations, paused for approval when it
touches something consequential, priced and logged either way. The product
does not compete on having the most features; it competes on being the only
option in the Norwegian/EU market that is simultaneously **acting** (not
just answering), **approvable** (not a black-box resolution rate), and
**architecturally sovereign** (not a compliance page bolted onto a US-hosted
stack). Everything in `velion-roadmap.md` is sequenced against closing the
gap between that end-state and what is running today.
