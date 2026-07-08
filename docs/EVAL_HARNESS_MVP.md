# Eval Harness MVP — Scoping (2026-07-07)

> **STATUS: SHIPPED 2026-07-08** — implemented in `apps/Model Plane/python/eval-lab-py`
> (`make selftest` = CI path, `make eval` = live path; report in `docs/eval-reports/`).
> First calibrated baseline: **6/7 live cases pass, 1 known-failing, 5 skip pending
> fixtures** (knowledge/image/ZDR seeding — enable via `EVAL_CAPABILITIES`).
> **First real finding (case 07, known-failing by design):** the `deployed_agent`
> invoke path never surfaces a social-publish tool, so the model drafts text instead
> of attempting the action and the HITL approval gate is never exercised for social
> posting. The case turns green when social publishing joins the agent tool catalog
> with its approval gate intact.

## Why (and why now)

The 2026-07-05 isolation/AI-first audit confirmed: **no eval loop exists**.
`eval-lab-py` is an unused exact-string-match scaffold with zero non-test
references (docs claiming it was "✅ IMPLEMENTED" were corrected by the
remediation program). The Phase 7 confidence scorer (B6) is an honestly-scoped
heuristic whose own comments say it is waiting for a real eval substrate.
Meanwhile the product claim is "observable, approvable, cost-aware agent
runner" — a claim we currently cannot measure regression against.

## MVP scope — what it IS

A **checked-in, repeatable, live-stack eval suite for agent runs**, runnable
with one command against the local compose fleet, producing a scored report
per case and in aggregate.

### The four metrics

| Metric | Source | How measured |
|---|---|---|
| **Accuracy** | run output | Per-case assertions: deterministic checks where possible (structured fields, tool calls made, entities present), LLM-judge rubric otherwise (judged via inference-core, `velion-balance`) |
| **Groundedness** | run output + retrieval traces | Claims in the answer must be attributable to retrieved sources; retrieval-engine already returns `trace_id` per query — the judge receives answer + traced sources and scores support/contradiction/unsupported |
| **Cost per task** | already emitted | Phase 7 B5 priced ledger: `cost_usd` on the run stream, aggregated per case and compared to a per-case budget ceiling |
| **Retry / loop health** | run event stream | Tool-call retries, failed steps, pause/approval counts, loop iterations per run — from the same SSE/run-events stream the Agent Run Console uses |

### Harness shape

- **Language/home**: Python, extending `eval-lab-py`'s home (labs own evals
  per the stack rules) but replacing exact-string-match with the metric
  runners above. Cases are declarative YAML: prompt, org/fixture refs,
  expected properties, budget ceiling, allowed tools.
- **Execution**: drives the REAL stack via model-gateway `/v1/invoke` (SSE,
  same path as production chat/agents) — no mocks. Fixtures: a dedicated eval
  org with seeded knowledge documents (ingest via documents-api, the path
  live-verified 2026-07-07).
- **Runner**: pytest-parametrized (one case = one test) → JUnit/HTML report
  for free, `make eval` entry point.

### Baseline suite (~12 cases, checked in)

1. Brreg lookup correctness (live server-side op).
2–4. knowledge_search groundedness: crawled-site fact, uploaded-doc fact,
   honest-refusal when the answer is not in the sources.
5. Cross-modal retrieval (text→page-image, Embed v4 arm).
6. WhatsApp draft-reply generation (draft quality rubric, no send).
7. Social post draft with approval gate — run must PAUSE (HITL posture
   respected), not publish.
8. Risky-tool read vs write gating (op-aware permission behavior from
   Stream 1 P3).
9. Budget ceiling: `velion-budget` alias must downgrade model, stay under
   cost ceiling.
10. Multi-tool loop task (research → summarize) — retry/loop health bounds.
11. Cross-org isolation probe: eval org B's agent must see zero of org A's
    documents (regression net for the ownership work).
12. ZDR case: restricted content must not surface in grounding.

### CI wiring — honest about the constraint

Full-stack eval needs live services + Azure credentials; that does not belong
in every PR. MVP ships:
- **CI (every PR)**: harness self-test only — case-schema validation, metric
  runners against recorded fixtures, no network. Keeps the harness itself
  from rotting.
- **Nightly/on-demand (operator or scheduled runner on a machine with the
  fleet)**: `make eval` against the compose stack; scored report written to
  `docs/eval-reports/<date>.md`; regression = any case dropping below its
  floor.
- PR-gating on a fast subset is a later phase, once flake rates are known.

## What it is NOT (explicitly out of scope for MVP)

- No production-traffic sampling or online eval.
- No statistical rigor beyond n=1..3 per case (flag variance, don't model it).
- No auto-finetune/optimizer loop (the router-policy tuning system is a
  separate, existing track).
- No replacement of the B6 confidence scorer — feeding it real eval priors is
  a natural phase 2.

## Effort estimate

Harness core + metric runners: ~3–5 agent-days. Fixture org + seeding: ~1.
Baseline suite authoring: ~2. CI self-test wiring: ~0.5. Comparable to one
remediation stream — suitable for a single autonomous background stream once
the 3b/3c merges land.
