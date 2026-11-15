# T2 / Category 7: Observability and operations, cross-source comparison

Inputs: 91 reference records (apify 13, aside 22, codexbrowser 30, firecrawl 26) and 19 Verevon baseline records. Output: 34 clusters, 89 records assigned, 2 excluded (Aside launcher stub and upstream Chromium components). Most records are SRC. Every Apify record is DOC. File: `07-observability-operations.clusters.json`.

## Shared baseline (table-stakes / common)
- **Run history with retention** (C-010) and **request correlation / run origin** (C-005). Firecrawl, Apify and Codex all have both.
- **Least-privilege run permissions** (C-017): limited by default, scoped expiring tokens, Allow/Prompt/Forbidden.
- **Per-run cost visibility and caps** (C-007), **agent/user-readable logs** (C-001) and a **self-host stack** (C-028).

## Best of breed
| Capability | Best source (records) | Why |
|---|---|---|
| ZDR-aware telemetry (C-008) | Firecrawl (eng-061, eng-062) | Sampler returns NOT_RECORD; late-ZDR trace registry drops in-flight spans; ZDR log filter; X-Zero-Data-Retention header; retention only gets stricter. |
| Agent run trace (C-011) | Firecrawl (api-091) + Codex (as-067/068) | Versioned envelope with producerSequence and parent agent; typed events; artifact snapshot versions; UI tab/surface metadata on each result. |
| Permission-decision audit (C-012) | Codex (as-027, iab-025) | One record per decision: decided_by enum, decision_context, origin only, duration_ms. Only denials are logged for URL checks. |
| Browser-time metering and caps (C-007) | Firecrawl (eng-078, eng-066) | Reserve on create, settle once under a row lock, reconciliation sweep; request-scoped cost tracker that throws at the cap. |
| Privacy-classified logging (C-002) | Codex (iab-024) | `{safe, sensitive}` buckets are part of the logger API itself. |
| Credential vault (C-015) | Aside (ac-054/055) | Opaque refs; fill in an isolated world that re-checks origin and that the target is a password field; per-fill audit. |
| Secret redaction (C-016) | Aside (ac-056) | Two tiers: snapshots always redact passwords; durable captures redact by field meaning and value shape. |
| Quality evals and hermetic site (C-031) | Firecrawl (eng-069/070/071) | Local test site including prompt-injection pages; mock record/replay; evals on a PR hashtag and after deploy. |
| SIEM export (C-013) | Firecrawl (eng-067) | Per-org batches, bounded queue that drops the oldest entries, retry ladder, then DLQ; destination host allowlist. |
| Alerts incl. data quality (C-006) | Apify (089, DOC) | Alerts on metric thresholds, run status and dataset field-stat outliers. |

## Differentiators (2+ sources, not most)
ZDR-aware telemetry (C-008), canonical run trace (C-011), metrics plus per-run statistics (C-003), spans that exclude approval wait (C-004), privacy-classified logs (C-002) and secret redaction (C-016). Local OS sandboxes (C-020) and ambient capture (C-025) are also differentiators, but only for desktop products.

## Verevon's biggest gaps (tier priority, then recommended)
1. **C-011 Run trace not surfaced** (priority, sig 5). Quarry has typed events, NATS, `?from_seq` SSE and receipts. Verevon shows none of the receipts, procedures, live view or typed stream. AI-run start likely fails on grant/bearer wiring. No cross-plane envelope exists.
2. **C-008 ZDR leaks through telemetry** (priority, sig 5). `quarry-edge/src/telemetry.rs` always installs JSON fmt and OTLP layers. It has no ZDR sampler or filter. ZDR events and artifacts are already handled.
3. **C-007 Budgets not enforced** (priority, sig 4). `max_cost_usd`/`max_runtime_s` are accepted but not enforced on Quarry's `/step` path. Browser time is not metered. The Model Plane cost-core ledger already exists.
4. **C-003 No /metrics on Quarry edge** (priority, sig 4). The metrics crates are inert, so alerting (C-006, absent) is blocked. The Control Plane Prometheus config already exists.
5. Recommended:
   - Decision-audit schema across planes into audit-core (C-012).
   - Password and OCR redaction, and wiring up `redact_sensitive` (C-016).
   - Memory-pollution guard on memory/graph writes after web tools (C-026, unverified).
   - Durable BrowserBroker grants and putting `evaluate` behind a sensitive grant (C-017).
   - Org-scoped webhooks (C-033).
   - ZDR matrix, plus fixing the gateway search cache, which has no ZDR gating (C-009).
   - Per-session browser isolation (C-019).
   - Hermetic evals in CI (C-031).
   - Credential vault (C-015, large).
   - Correlation ids (C-005), spans (C-004), structured logs (C-002), retention janitor default and a ZDR tombstone row (C-010).
6. Skipped as a poor fit for a web SPA with a remote, leased Chromium: local OS sandboxes (C-020), local daemon security (C-022), ambient capture (C-025) and webview hardening (C-027).

## Verevon strengths (tier n/a)
- **Egress policy (C-021):** allowed domains come only from the broker grant, domains are checked before and after each action, agent runs need isolated-egress proof, and egress receipts are redacted.
- **Ops readiness (C-023):** `/ready` reports durability, and production refuses ephemeral stores or default control keys. Internal control uses HMAC, not Firecrawl's path-secret admin.
- **Self-host (C-028):** production defaults are safer than Firecrawl's unauthenticated default.
- **Ingestions console (C-034).**
- **ZDR strictness (C-009):** ZDR runs are live-only, not resumable and get no persistent profiles. This is stricter than any reference.
- **Webhooks (C-033):** HMAC signing and DLQ go beyond Apify's documented model.

## Caveats
- All Apify records are DOC. C-006 (alerting), C-019 (per-run isolation) and C-033/C-034 rest only on DOC references; the Verevon side of those is SRC-verified.
- Status "unknown" needs a code check for C-002, C-014, C-018, C-024 and C-026. Statuses for C-005, C-007, C-012, C-016, C-029 and C-030 rest on targeted greps done here (SRC), not on baseline records, so confidence is medium.
- The category mixes observability with security and sandboxing records from Aside and Codex. Desktop-only sandboxes were kept as skip clusters rather than excluded.
- Baseline record v-quarry-113 (vision sidecar) has no counterpart in this category and was not mapped.
- Out of 34 clusters, Verevon has not shipped 30. Four of those are skip clusters, so 26 are real gaps.
