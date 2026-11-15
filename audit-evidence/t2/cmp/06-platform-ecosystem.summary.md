# T2 / Category 6: Platform & ecosystem (comparison summary)

Inputs: 77 reference records (Apify 37, Firecrawl 21, Codex in-app browser 10, Aside 9) and 19 Verevon baseline records. Output: 35 clusters, 74 records assigned and 3 excluded (Firecrawl's support proxy, its open-core/cloud split, and Aside's daemon router map). Clusters file: `06-platform-ecosystem.clusters.json`.

## Shared baseline (table-stakes)
- **Usage pricing and metering (C-010).** One pure pricing function per request: a base unit per page, surcharges for LLM and security features, and failures free (Firecrawl, SRC). Apify bills compute units (CU). Both web platforms have this. The desktop apps have no per-call pricing, so the class is adjusted to table-stakes for web benchmarks. Verevon is **partial**: Quarry emits NATS usage events but has no pricing function and no user-visible web usage.
- **Plans, limits and plan-gated features (C-023)**, present in 3 of 4 sources. Verevon is **partial**: org-core has entitlements, and ZDR is a plan-gated entitlement. Browser and web features are not plan-gated, and browser, search and ingestion gateway routes have no rate limiting.

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Budget caps enforced by the executor (C-011) | Apify (+ Firecrawl) | `charge()` returns how much can still be charged within the cap, so the run can stop gracefully. Firecrawl reserves the browser TTL before an LLM prompt and settles on stop. |
| Tri-state org entitlements / feature gates (C-024) | Firecrawl (+ Codex) | Each flag is disabled, allowed or forced (for example ZDR forced), so a request cannot weaken org policy. Codex has fine-grained browser gates that default to off. |
| Capability registry + backend matrix (C-025) | Codex | Each API member lists the backends it is unsupported on (`unsupportedByDefaultIn`); the agent calls `list()` and then `documentation()` only on demand; the runtime does not depend on Electron (REA). |
| Skills with URL auto-inject (C-018) | Aside | `SKILL.md` frontmatter can set keywords or URL patterns. 16 site playbooks scope page snapshots with selectors. |
| Run storage + exports (C-017) | Apify | Each run gets a dataset, a key-value store and a request queue. Exports are server-side (CSV/JSONL/XLSX) with field projection, and retention depends on whether storage is named. |
| Packaged unit + manifest (C-001) | Apify | `actor.json` holds the schemas, storages and version; the same unit runs from UI, API, CLI or a schedule. |
| Run turned into a reusable skill (C-020) | Firecrawl | `GET /v2/agent/:id/skill` returns a SKILL.md, a workflow and a schema. |
| Deprecation protocol (C-032) | Firecrawl | A typed registry drives Deprecation, Sunset, Link and Warning headers plus body notices, with per-version adapters. |
| Per-credential restrictions (C-033) | Firecrawl | Keys can be limited by endpoint group, format/action and IP allow-list. |

## Differentiators (2+ sources, not most)
Packaged units (C-001: Apify, Codex), marketplaces (C-008: Apify, Firecrawl; skip), budget enforcement (C-011), sharing runs and results (C-015: Apify, Aside), and feature gates / tri-state entitlements (C-024: Codex, Firecrawl).

## Verevon's biggest gaps (selected tiers)
1. **Priority, budget enforcement (C-011, sig 4, M).** The contract has `max_cost_usd` and `max_runtime_s`, but only `max_steps` is enforced. The defaults are unbounded. Quarry should enforce the caps as executor, and Model Plane cost-core should pass the remaining budget with each grant.
2. **Priority, org-policy capability gates (C-024, sig 4, M).** Raw JS `evaluate` runs on normal Quarry runs without approval. Gates are env- or manifest-level only, not org policy. Control Plane should own a disabled/allowed/forced map that reaches Quarry as signed grant claims (no cross-plane DB reads). This turns "ZDR must propagate" into a forced posture.
3. **Recommended, highest value:**
   - **Procedure store + run-as-procedure (C-020, sig 4).** Quarry already compiles and checks procedures but has no store and no execute endpoint.
   - **Per-run result sets with export (C-017).**
   - **Exposing the existing presets and making schedules actually run (C-007).**
   - **URL-triggered site skills (C-018).** Extends the existing session-core skills.
   - **Driver capability matrix fed to the planner (C-025).**
   - **Action- and format-scoped BrowserBroker grants (C-033).**
   - **Pricing attributes on usage events (C-010).**
   - **Rate limits and plan keys (C-023).**
   - **Encrypting browser profile snapshots (C-003).**
   - **Quarry packs (C-001).**
   - **Evals on the production contract (C-027).**
   - **Agent routing guidance (C-019).**
   - **Wiring Quarry jobs as approval-gated agent tools (C-016).**
4. **Skip:**
   - CI/source hosting (C-006)
   - Marketplace (C-008); at most, consume Apify through integration-corev2
   - Agentic payments (C-013)
   - Extension channels (C-029)
   - Keyless tier and device-flow onboarding (C-031)

## Verevon strengths (tier n/a)
- **Tool scoring (C-009).** capability-core computes a deterministic composite capability score from success, schema failures, latency, cost, approvals, incidents and operator rating. It is aimed at agent tool selection, which is ahead of Apify's marketplace score. Keep it, and check that Quarry tools feed it.
- **Org tenancy (C-014).** The org comes from the verified session on every web route, and Quarry uses JWT org scoping with tenant-stamped artifacts.
- **Server-side agent configuration (C-022).** It removes the need for Aside-style device sync.
- **Search corpora (C-035).** Verevon has a durable sources registry, an org-scoped local index that is skipped under ZDR, hybrid search over Data Plane vectors with rerank, and four SERP providers with paid-provider and ZDR switches.
- **Partial areas with strong foundations:**
  - Risk-classified HITL approvals (C-016).
  - Domain-allowlisted, single-use sensitive grants (C-033).
  - Fail-closed driver gating (C-025).
  - Session-core skills with a size budget and a learning skill sink (C-018).

## Caveats
- Apify records are all DOC (public docs). Codex training parity (C-027) is DOC-only. Firecrawl's agent-skill export (C-020) and Labs (C-035) proxy closed upstream services, so only the route shapes are SRC.
- Several Verevon mappings were found in code during this comparison and have no baseline record:
  - skills (C-018)
  - capability scoring (C-009)
  - org entitlements (C-023, C-024)
  - cost-core budget errors (C-011)

  They were read lightly; treat their confidence as medium.
- Status **unknown**, still to verify:
  - **C-002:** schema-driven forms. Check verevonv3 for a zod-to-form renderer.
  - **C-028:** image or bundle signing. Check CI for cosign or admission verification.
- Several partial statuses depend on the BrowserBroker grant path, which has no production issuer. Until it does, browser-agent features that look shipped in Quarry are not reachable in production.
- Many in-repo docs are stale. Code was treated as ground truth.
