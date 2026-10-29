# Verevon — Phase 3 "Complete the FULL versions" Execution Plan

> **Status:** Approved 2026-06-21 (recon-grounded + hardened by a 6-persona triage council). Source roadmap: `Verevon-ai-first.md` (§7 CONTINUE + council #7 + the FULL completions of the Phase-2 product MVPs). **The greenfield wishlist (audit council #9) stays CUT.**
>
> ## What Phase 3 is
> **Make the wedge spine PROVABLE end-to-end and residency-honest:** monitor (Quarry change→notification) → brief (insight producers + scheduled delivery) → approve (durable HITL) → **act in-region** (embeddings on the model_plane gRPC hop, not direct-Azure). Plus the non-PII Brreg leads completion and the honest WorkflowBuilder label + the solid/* bug fixes.
>
> ## SCOPE LEDGER (the discipline that keeps this shippable)
> Phase 3 carries **exactly ONE large-but-bounded headline (B, the embedding/residency hop) and ZERO pure-debt XLs.** The independent cheap completions (D-1, E, F-leads, G-solid-fixes) parallelize. **Adding any deferred item back requires displacing an equal IN item.**
>
> ## THE ABSOLUTE RULES (carried from prior phases)
> - **Honesty:** nothing ships labeled real/durable/at-scale/private/enriched/configured/approved/sent over volatile or absent backend state. Complete to REAL, or label honestly.
> - **IDOR-clean:** every new gateway domain (monitoring, leads.build_list, brief delivery) resolves org server-side via `authorized_org_id` and passes the cross-tenant regression test + the no-fabricated-state lint before it can render in v3.
> - **No net-new public unauthenticated route** ships in Phase 3 (the `/embed` surface is deferred).
>
> ## ENTRY-GATE
> Several items depend on **Phases 1/2/Ownership having LANDED** (E's Preview-gate carry-through; any Ownership/GDPR-coupled item; the D-1 surfaces). Treat that as an explicit gate, not an assumption — do not schedule a dependent sub-item before its predecessor is merged-and-green.

Paths: gateway = `apps/Frontend Plane/verevonv3/apps/gateway`; SPA = `apps/Frontend Plane/verevonv3/src` (quote the space).

---

## PR sequence

### `PR-0a` — Secret rotation + history scrub *(DAY-0 HARD GATE, independent)*
The recon **verified a live-shaped Azure OpenAI key committed at `apps/Data Plane v2/.env`** (ties to the known secret-leak incident).
- Rotate Azure KEY1 (core-ai-rg) + `DB_PASSWORD` + `BETTER_AUTH_SECRET` in the providers/portal.
- Scrub git history (git-filter-repo/BFG), force-push, coordinate team re-clone; confirm no live secret remains in tree **or** history.
- Add a `gitleaks`/`trufflehog` CI gate that fails on a reintroduced secret.
- **DoD:** no live secret in tree or history; key rotated; CI secret-scanner green on a planted-secret test.

### `PR-0b` — Commit/stabilize the in-flight W2 Quarry engine *(DAY-0, protects at-risk work)*
The recon **verified the W2 change-monitor engine is uncommitted** (untracked `notify/`, `schedule_wire.go`, migrations `008/009` + modified `change_routes.rs`, `workflows.go`, `activities.go`, quarry-control resources).
- Commit it in honest stacked commits; `go test ./...` (control/orchestrator) + `cargo test` (edge) green; a fresh clone builds quarry-{edge,control,orchestrator} without the worktree.
- **DoD:** W2 committed; clean-checkout build + tests green; no Quarry work lives only in the worktree.

### `PR-1` — A: WorkflowBuilder "design preview" label + neutralize dead controls *(honesty win)*
The builder is a 100% mockup with no backend, no execution substrate (model-plane is an LLM agent loop, not an n8n DAG), and no MVP predecessor — building it real is on the audit CUT list.
- Render a visible **"Design preview"** label; **disable/remove the dead Test Run/Publish** (and any action-implying) controls **in the same change**; sweep `agents/*` for other non-wired affordances.
- **DoD:** no control implies a backend that doesn't exist (screenshot confirms label + neutralized controls); no backend was built. *(Honesty rule: label + neutralize ship together or not at all.)*

### `PR-2` — B-spike: diagnose the DP→MP gRPC embedding hop *(timeboxed ~1 day, headline risk first)* — deps: PR-0a
The hop is fully coded both ends (`inference-core` `create_embedding` real, binds `:9092`) but fails at runtime with a Status error and **no server-side log**.
- Stand up a live DP+MP stack; fire one `embed_query` with `EMBEDDING_PROVIDER=model_plane`; capture the gRPC `Status` text + server log. Check stale image vs missing deployment/creds vs proto/port/h2c mismatch.
- **Explicit exit criterion + fallback:** if not fixable in the box, ship `org_id`+`zdr` + the ZDR e2e on direct-Azure and re-spike in Phase 4.
- **DoD:** written root cause reproduced once locally; retrieval returns a non-empty embedding vector from inference-core over gRPC, **OR** the fallback decision is recorded.

### `PR-3` — B-fix: real model_plane hop + org_id/zdr + residency/ZDR/freshness tests — deps: PR-2
- Land the spike fix; verify the embedding default is **live `model_plane`** (not just config); demote direct-Azure to fallback only.
- Add a `zdr` field to `CreateEmbeddingRequest` (proto + regen retrieval-engine-rs + Model Plane + DPv2 enforcement); thread `org_id`+`zdr` through the hop.
- Convert the ZDR-reject SQL into a **full-pipeline e2e via the orchestrator** (`pipeline_e2e.rs` TODOs no longer stubbed), **covering the direct-Azure fallback branch** (ZDR content must never egress on fallback); add a **freshness integration test**.
- **DoD:** live model_plane round-trip; `org_id`+`zdr` propagate and asserted; freshness + ZDR-reject (incl. fallback) e2e green; `cargo test --workspace` + `make test-integration` pass.
- **⚠ Residency honesty:** the hop alone does NOT satisfy EU residency (both provider paths still default to Azure) — **do not market residency on B's completion**; an EU model-plane embedding provider is a Phase-4 prerequisite.

### `PR-4` — D-1: HITL approval durability *(one Rust service, no new migrations)* — deps: Entry-gate; parallel with B
`model-gateway` writes approvals **in-memory first** and persists best-effort-after (a restart can silently drop a pending approval); `session-core` already owns the durable Postgres approvals store.
- Reorder to **durable-FIRST write** to session-core *before* the handler returns (`request_approval` + `decide`); add **read-through** in `list_pending`; **rehydrate** the in-memory store on boot; idempotency key.
- **DoD:** approval written durably before the API returns; survives a process restart (proven by a kill-mid-pending test); no in-memory-only path of record; `cargo test` + `clippy -D warnings` green. *(Defers D-2 draft.reply-SEND + D-3 actions-producer to Phase 4.)*

### `PR-5` — E: insight-core producer legs + scheduled brief delivery — deps: Entry-gate; parallel with B
W3 is live with 1/4 producers (conversation-core).
- Add the **social-core** producer leg (map `verevon.application.social.*` lifecycle subjects → `surface=social`) [S] + the **model-plane-agents** leg (run/tool/approval counts via the model-plane-nats bridge) [M].
- **Scheduled brief DELIVERY** via the existing **notification-core Novu adapter** (a Go scheduler POSTs a `daily_brief` notification carrying the **Preview gate** — do NOT build Novu) [M]; register the `daily_brief` workflow; fix the 2 stale code comments.
- **DoD:** ≥3/4 producers live with real counts; brief delivered in_app+email with the Preview gate visible; idempotent; **unmapped-event-skipped test** locks the allow-list honesty. *(Defers GA4/GSC, the Quarry-change producer, and citation enrichment.)*

### `PR-6` — F-leads NON-PII enrichment + `leads.build_list` tool — deps: live leads-core (Phase-2 PR-5 landed)
- Extend the **company-only** Brreg client with `/underenheter` (branches) + `/regnskap` (financials) — **never `/roller`**; cross-list dedupe (orgnr canonical) with tests.
- A governed, **entitlement-gated, metered, audited** `leads.build_list` agent tool (mirror `info_tools` `COMPANY_LOOKUP_TOOL`), org resolved server-side (IDOR-clean).
- **DoD:** branches/financials/dedupe return real Brreg data through metered+audited leads-core; `build_list` creates an org-scoped saved list; a **company-only invariant test** asserts no person/role/fødselsnummer field is ever populated. *(Defers roller-PII + bulk-CSV>10k to Phase 4.)*

### `PR-7` — C: Quarry monitoring MVP *(needs a live Temporal + 4-process Quarry stack)* — deps: PR-0b
- `quarry_sources` CRUD on migration 005 (replace the empty `/v1/sources` stub); wire Temporal **trigger/backfill**; **cross-org reaper isolation test** (mandatory security gate — two orgs through a reconcile, org-scoped reaping); change→webhook→in-app-notification **e2e** on the live stack; a **minimal v3 Monitoring tab** (CREATE/LIST/DELETE) with **honest empty/error states** when Quarry is down (no synthetic competitor-watch).
- **DoD:** sources CRUD live; change→notify e2e green; cross-org reaper test green; Monitoring tab renders real events or honest empty/error. *(Defers request-queue→fanout + per-org quotas/team aggregates to Phase 4 — the "at-scale" half.)*

### `PR-8` — G: solid/* bug fixes + ChatPage split *(continuous hygiene track)*
- Remove the **3 blanket `eslint-disable solid/*`** and fix the **99 solid/* problems** (52 `prefer-for` `Array#map`→`<For>`, 29 reactivity, 16 return-once) — real bugs on live surfaces; split `ChatPage.tsx` (3685) below the 800-line guideline; DashboardComposer if capacity.
- **DoD:** lint clean on solid/* with no blanket disables, no reactivity regressions; ChatPage split; `pnpm verify` green. *(Defers the agents/studio Tailwind→semantic-CSS XL rewrite, the TIER-2 sweep, and the remaining file splits to a dedicated Phase-4 UI pass.)*

### Sequencing
0a (key rotation) + 0b (commit W2) day-0 → PR-1 (label) → **PR-2 B-spike sizes the phase** → in parallel: PR-4 (D-1), PR-5 (E), PR-6 (F-leads) → PR-3 (B-fix headline) → PR-7 (C, alongside B since both need live stacks) → PR-8 (G hygiene, continuous).

---

## Explicitly deferred to Phase 4 (documented, not dropped — each with its gate)
- **A build-real** WorkflowBuilder (n8n-class; audit CUT — only if a paying customer re-adds it as net-new).
- **The entire `/embed` public surface** (audit DEFER; needs its own threat model + scoped-public-key store + origin allowlist + per-visitor rate-limit + anti-enumeration + SSRF review + embed.js — a dedicated Phase-4 funnel/security sprint).
- **Leads roller/contacts PII** (must co-release with Phase-1 GDPR + the Ownership taxonomy in one entitlement-gated, default-OFF release with a written Legitimate Interest Assessment) + **bulk-CSV >10k**.
- **D-2** draft.reply SEND executor (needs a net-new cc-go customer-outbound path; irreversible) + **D-3** Model→Application actions producer (XL, no cross-plane channel).
- **GA4/GSC** connector activation (external-OAuth-blocked) + the **Quarry-change insight producer** (C-gated) + brief **citation enrichment**.
- **C at-scale:** request-queue→BatchJobWF fanout + per-org quotas/team aggregates.
- **An EU model-plane embedding provider** (the actual residency prerequisite).
- **G:** the agents/studio Tailwind→semantic-CSS XL rewrite + TIER-2 stray-Tailwind sweep + remaining file splits.

## Risk register
- **WorkflowBuilder false-functional** → label + neutralize dead controls in the SAME change (PR-1); reject if any action-implying control survives.
- **Public `/embed` attack surface** → fully deferred to a Phase-4 security sprint; no public unauthenticated route in Phase 3.
- **roller/contacts PII lawful basis** → deferred; ships only co-released with GDPR + Ownership, entitlement-gated default-OFF, LIA first.
- **Embedding-hop live-debug unknown** → run B as a timeboxed spike-first with an explicit exit criterion + written fallback.
- **Over-scoping** → one bounded headline (B), zero pure-debt XL; scope ledger in the header; swap-one-out to add one back.
- **Fabricated/unverifiable-at-scale** → Monitoring tab ships only with the reaper-isolation + change→notify e2e green on a live stack, honest empty/error otherwise.
- **Residency over-claim** → don't market residency on B's completion; EU embedding provider is a Phase-4 prerequisite; ZDR e2e must cover the Azure fallback.
- **In-flight dependency** → Phases 1/2/Ownership landing is an explicit ENTRY-GATE for the dependent sub-items.

## Tooling & skills kit for the executor
- **Search:** codegraph (`codegraph_context`→`codegraph_explore`) + context-mode (`ctx_execute_file`); re-confirm with `rg`. **Docs:** context7/`ctx7` for tonic/gRPC, sqlx, Temporal Go SDK, Novu, axum, SolidJS, the Brreg Enhetsregisteret + Regnskapsregisteret APIs.
- **Rust (B hop + ZDR/freshness, C edge, D-1 model-gateway/session-core):** `rust-patterns`, `rust-testing`; agents `rust-reviewer`, `rust-build-resolver`.
- **Go (PR-0b W2 commit, E producers + scheduler, F-leads + build_list tool, C quarry-control):** `golang-pro`, `golang-patterns`, `golang-testing`, `go-concurrency-patterns`; agents `go-reviewer`, `go-build-resolver`.
- **SolidJS (A label, C Monitoring tab, G solid fixes + ChatPage split):** `solidjs-vite-typescript`, `solid-*`, `vite` (NO Tailwind; `<For>/<Show>`; `props.x`); agents `typescript-reviewer`, `build-error-resolver`.
- **DB/migrations (C migration 005, the zdr proto field):** `database-migrations`, `postgres-patterns`; agent `database-reviewer`.
- **Security (PR-0a rotation + scrub, ZDR, the reaper-isolation test, IDOR-clean new domains):** `security-review`, `security-scan`; agent `security-reviewer` (MUST review PR-0a, PR-3 ZDR, PR-7 reaper).
- **Method:** `tdd`/`tdd-workflow` (RED-first for the restart-survival, ZDR-reject, reaper-isolation, company-only-invariant, unmapped-event tests), `code-review`, `verification-loop`; agents `code-reviewer`, `tdd-guide`, `e2e-runner`. **Browser smoke:** playwright/chrome-devtools. **PRs:** github MCP/`gh`.

## Global Definition of Done
WEDGE SPINE PROVABLE end-to-end (monitor→brief→approve→act-in-region demo); SECURITY GATE green (key rotated + history-scrubbed + CI scanner; cross-org reaper + ZDR-fallback tests; no net-new public route); HONESTY GATE green (WorkflowBuilder labeled + dead controls neutralized; Monitoring tab honest empty/error; no over-claim); W2 engine committed + quarry_sources CRUD live; LEADS advanced NON-PII only with the company-only invariant test; code-quality (99 solid/* fixed, ChatPage split, `pnpm verify` green); everything OUT is documented as Phase-4 with its gate; SCOPE DISCIPLINE held (one headline, zero pure-debt XL); every new gateway domain passes the IDOR regression + no-fabricated-state lint.
