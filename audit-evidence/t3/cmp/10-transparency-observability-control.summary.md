# T3 / 10: Transparency, observability and control (cross-source comparison)

Input: 169 reference records from 12 sources. 166 are assigned to 47 clusters and 3 are excluded as IDE-only or desktop/terminal trivia. Verevon baseline: 33 records for this category, plus cross-category records. DeepSeek supplies 55 of the 169 records, so it dominates several clusters. Clusters file: `10-transparency-observability-control.clusters.json`.

## Shared baseline (table-stakes / common)
- **Tool-call cards and work log** (C-003, 5 sources). Verevon has **shipped** this at parity: intent, hit count, truncation, args, output and diff. Presentation is recomputed and never persisted.
- **Usage allowance and history page** (C-009, 7 sources). Verevon is **partial**: it has a cost dashboard and an org quota, but no "allowance left / resets at" meter for each user.
- **Context window inspector** (C-005, 4). Verevon has **shipped** it and is level with Cursor and Hermes.
- **Exact model-visible request capture** (C-006, 4). Verevon is **partial**.
- **Workspace analytics** (C-012, 5). Verevon is **partial**.
- **Customer telemetry export (OTLP/SIEM)** (C-013, 5). Verevon is **absent**.
- **Data-flow and egress disclosure** (C-017, 4). Verevon is **partial**.
- **Support and diagnostic bundles** (C-018, 6, desktop-centric). Verevon status is **unknown**.

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Trajectory ledger + inspector (C-001) | DeepSeek ds-005/008/009 | A second complete view: records grouped by turn, step and request; 13 inspector tabs; Inspect deep link from chat |
| Model-visible = logged (C-006) | DeepSeek ds-001/002/004 | Request-header events (config, adapter defaults, tool catalogue) are written only when something changes; the system prompt is a diffable event |
| Timing waterfall (C-002) | DeepSeek ds-006/007, Antigravity ag-095 | Separates time to first token from decoding; shows "unavailable" instead of estimating; includes queue and approval wait |
| Approval "why am I asked" (C-032) | Claude cle-122/153, Antigravity ag-097 | About 33 cause codes, each with a plain sentence; a named reason for every auto-deny; records who decided (user, system, model or default) |
| Scrubbable replay (C-042) | Manus mau-063, mac-021 | Timeline over tool events with idle-gap compression and "Jump to live"; pointer overlay with a short brief per action |
| Learning ledger + rollback (C-043) | Hermes hel-066/067 | Content-addressed before/after copies, rollback that fails closed, and a timeline of what the agent learned |
| Egress catalogue (C-017) | Claude cle-107 | One declaration per destination generates the CSP, firewall lists, toggles and fallbacks |
| Customer OTLP export (C-013) | Claude cle-042, web-claudeai wca-089 | Content-capture categories default to metadata only (fail closed) |
| Closed-vocabulary telemetry (C-016) | Hermes hel-081 | Closed enums and count buckets; opt-in; the recorder never raises an error into callers |
| Plain-language narration (C-004) | Codex cxa-088/097 | About 900 per-connector templates for each state; shell commands mapped to plain actions |
| Status pills (C-040) | t3code t3u-021 | Fixed precedence rules, a rollup to the parent group, tracking of unseen completions |
| Replay regression tests (C-023) | DeepSeek ds-035 | 600 recorded sessions replayed with no live model |

## Differentiators worth noting
- DeepSeek's "see everything" stack: C-001, C-006, C-007, C-020, C-021 and C-023.
- Claude's governance and diagnostics: C-013, C-017, C-018, C-032 and C-034 (hash-chained audit).
- Manus's visible computer use and replay (C-042).
- Hermes's learning transparency (C-043) and evals as release gates (C-024).

## Verevon's biggest gaps (priority tier)
1. **Audit trail of AI actions (C-034, partial).** The outbox fix is built but not deployed, and 4,992 events were never delivered. There is no hash chain and no export. This blocks enterprise trust and also C-012 and C-013.
2. **Approval transparency (C-032, partial).** `approval_reason_for` returns a generic "tool X requires approval". Needed: a cause-code enum, a record of who decided each automatic allow or deny, and localised sentences (reuse the zdr-refusals.json pattern).
3. **Model-visible request capture (C-006, partial), then the trajectory ledger (C-001, partial).** Verevon should store request snapshots with content-addressed prompt and tool-catalogue blobs, keeping only hashes under ZDR. The flat Trace tab should become a grouped ledger with an inspector.
4. **Live running cost and pre-run estimate (C-010, planned).** No running-spend event exists in ChatEvent. Planned behaviour is a warning at 80% (OD19).
5. **Scrubbable computer-use replay (C-042, partial).** Quarry should store a frame and a receipt for each executed action, and the Model Plane should link proposal, approval and receipt.
6. **Learning ledger and rollback (C-043, partial).** Rollback is planned (v-planned-decisions-110). There is no mutation ledger, no learning timeline and no "learned X" notification.

Recommended cheap wins:
- **Visible retries and typed turn end (C-037):** there is no retry event today.
- **Compaction marker (C-038):** there is no compaction event or marker today.
- **Narration templates (C-004).**
- **Collapsed reasoning summaries (C-028).**
- **Research activity history (C-029).**
- **Per-turn "Changes" card from receipts (C-039).**
- **Effective-policy origin view (C-020).**
- **Egress catalogue (C-017).**

## Verevon strengths (tier n/a; keep)
- **Per-turn usage (C-008):** cost is null when unknown, with a cost basis and confidence.
- **Context inspector (C-005):** itemised by segment, using real BPE token counts.
- **Served-by provenance (C-030):** shows provider, residency and the strongest substantiated privacy tier, which goes beyond a model label.
- **Tool cards (C-003).**
- **Run history, subagent lineage and Agent Run Console with proof bundles (C-041).**
- **Feedback (C-044):** thumbs and reason chips feed skill credit and dissatisfaction detection.
- **Metrics and tracing (C-014):** a metered decorator on every inference RPC, which still needs deploying.
- **Decision ledger and postmortems (C-022).**
- **Graduated confidence and verification pass.** This is unique to Verevon and covered by the baseline records, not by a reference cluster.

## Skipped or not applicable
- Published model and system cards (C-031): Verevon is a router, not a model lab, so a link to each provider's card is enough.
- Developer DevTools panels (C-047): covered by browser DevTools and the planned ledger.
- t3code's local process kill switch.
- Cursor's NDJSON log-ingest server (IDE-only).

## Caveats
- **Unknown statuses.** Seven clusters have status unknown and need code verification: C-015, C-018, C-025, C-031, C-033, C-035 and C-046.
- **Evidence from greps.** Several gaps are inferred from greps of `sse_events.rs` and the SPA (no retry or compaction events, no OTLP, no analytics SDK). They are tagged INF where not directly read.
- **Web-benchmark sources are DOC-only** (ChatGPT, Claude.ai, Manus and Perplexity web). Clusters supported only by them are marked low or medium confidence: C-011, C-028, C-031, C-035, C-036, C-045 and C-046. Their low prevalence partly reflects that web products cannot be inspected internally.
- **Source skew.** DeepSeek's 55 records over-represent its view; prevalence counts each source once per cluster.
- **Deployment state.** The Verevon baseline marks some shipped items as not yet deployed (the metering decorator and the audit outbox fix). "Shipped" means in code, not necessarily live.
