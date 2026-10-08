# Billing, Cost and Insight Core: status and future plan

**Updated:** 2026-10-05, Europe/Oslo.  
**Decision:** further implementation is on hold at the user's request. Preserve the current system and resume from this document when requested. This update changes documentation only; it does not stop running services.  
**Audience:** product, engineering and future implementers.

## 1. Current position

Verevon has tenant-scoped usage reporting, separate API/subscription measurements, source-checked support outcomes, and an implemented organization allowance engine. It does **not** yet have complete source coverage or a finished usage-based commercial billing lifecycle.

The latest implementation was rebuilt and deployed to the **local Docker stack**, and relevant tests passed. This is not evidence of a hosted production release, complete historical data, verified shared-Azure charge allocation, or a tested paid invoice lifecycle. The local frontend Compose target remains Vite development; the production frontend bundle was separately built successfully. The code is not restricted to a development-only alternative UI.

Customer charging remains unchanged. New raw AI counters do not create Lago invoice events. Actual allowance policies are not configured for Aquatiq's displayed plan. Nothing was pushed to GitHub during this work.

### Status terminology

| Status | Meaning |
|---|---|
| Implemented | Source and relevant tests exist; does not imply commercial activation or deployment everywhere. |
| Locally verified | A specified behavior was observed in the local stack or isolated integration environment. |
| Not configured | Implementation exists, but policy, permission or source configuration is missing. |
| Incomplete | Known engineering, evidence or coverage work remains. |
| Planned | Accepted direction; not a claim of an existing capability. |

## 2. Ownership and the quantities we must keep separate

| Owner | Authority | Must not become |
|---|---|---|
| **Control / Billing Core** | Plans, entitlements, organization allowance policy, grants, renewal, future prices and customer invoice settlement | A copy of provider cost telemetry or a browser-controlled credit wallet |
| **Model / Cost Core** | Recorded provider requests, input/output tokens, billing basis, authenticated org/user attribution, provider estimates and Azure cost evidence | The authority for customer subscription prices or money owed |
| **Application / Insight Core** | Workspace metrics, definitions, provenance, scope and coverage projections | A second financial ledger or a source of invented totals |
| **Owning source cores** | Original tickets, decisions, messages, runs and other domain records | Databases queried directly by another plane |
| **Frontend / gateway** | Authenticated access, normalized contracts and user-facing presentation | An authority for tenant identity, billing policy or provider secrets |

Data crosses planes through authenticated APIs and durable events, never direct database access. Control owns identity and membership. Shared Azure infrastructure is a provider resource, **not** an organization identifier.

Keep these quantities distinct in every API, UI, export and Verevon answer:

1. **Observed usage:** requests and tokens recorded for an authenticated organization/user.
2. **Provider estimate:** estimated API cost using a known rate/basis; not an invoice.
3. **Provider actual:** charges imported from Azure, with source period and currency.
4. **Allocated provider cost:** a documented share of shared provider charges; never relabel as directly billed tenant actuals.
5. **Product allowance:** included/granted units and remaining balance under a Verevon plan.
6. **Customer charges:** finalized invoice obligations, credits, refunds and payments owned by Billing Core/provider contracts.
7. **Codex allowance:** the connected ChatGPT account's provider quota window. It is not a tenant token wallet and cannot be reset by Verevon's reporting baseline.

## 3. What is implemented today

### Cost Core

| Capability | Status and boundary |
|---|---|
| Org/user request attribution | Implemented with authenticated scoping. Personal reads pin the user; organization-wide cost reads require the appropriate owner/admin permission. Missing users remain unattributed rather than guessed. |
| API vs subscription usage | Separate request/input/output counters and billing basis. Unknown legacy basis and missing token counts are retained as uncertainty, not free requests. |
| Usage analytics | Complete-window grouped queries by day/model/provider/basis, filters, chart and CSV export. Queries have explicit limits and fail rather than silently truncate totals. |
| Reporting baseline | Implemented as a visible reporting start. Original ledger history, budgets, provider quotas and financial obligations are retained. |
| Model usage transport | JetStream storage acknowledgement and a durable Cost Core consumer cover ordinary consumer outages and redelivery. Finite retention/capacity and failure before producer acknowledgement remain limits. This is not proof of lossless publication from every producer. |
| Cost-to-Billing delivery | New explicitly classified entries create a transactional outbox; stable per-entry/per-meter IDs make partial delivery retries safe. Historical unknown records are not automatically backfilled. |
| Azure importer | Imports provider ActualCost evidence separately from request estimates. The displayed org has no available tenant cost report; shared/unmapped costs must not be assigned to Aquatiq just to populate the panel. |
| Erasure/replay protection | Existing Cost Core erasure and replay guards are covered by relevant tests. Full retention and deletion behavior across all present/future financial stores still needs a release review. |

The billing delivery counters are `ai.metered.requests`, `ai.metered.input_tokens`, `ai.metered.output_tokens`, and the corresponding `ai.subscription.*` counters. No prompt or completion content is sent in these accounting events.

### Billing Core

| Capability | Status and boundary |
|---|---|
| Integer allowance units | Implemented per organization/meter, with immutable initial policy and explicit start/end, unit and calendar anchor. |
| Renewal/reset | Implemented independently of invoice frequency; month-end anchors are preserved. History is retained. |
| Carry-over and expiry | Configurable cap and validity; recurring and additional grants have distinct rules; earliest-expiring units are consumed first. No commercial default has been activated. |
| Additional grants | Stable identities, recorded reasons and retry/conflict handling. This is not a monetary purchase/refund workflow. |
| Late usage | Recalculates the affected usage periods and subsequent carry-over. Does not silently rewrite an invoice. |
| Scheduled allowance changes | Append-only revisions at future renewal boundaries preserve prior policy versions. Mid-cycle proration and arbitrary anchor/unit changes are not implemented by this feature. |
| Pause/end handling | Paused periods skip recurring grants; paused/ended balances cannot authorize quota use. Reactivation does not back-grant missed unpaid periods. Product acceptance of this policy is still needed before commercial activation. |
| Quota reads | Return configured period balances. A read is not an atomic reservation that prevents concurrent requests exceeding a hard cap. |
| Access | Gateway can read authorized org allowances; it cannot mint credits. A separate trusted provisioning scope is needed for policy/grant/revision writes. |
| Invoicing | Existing provider adapters remain. The six new raw AI counters are internal measurements and deliberately do not enqueue Lago invoice events. Priced overage settlement is incomplete. |

The replay implementation currently bounds a meter at 100,000 entries and 1,200 periods. This needs incremental settlement/checkpointing and scale validation before high-volume commercial use. Browser contracts reject unsafe integer totals rather than rounding them.

### Insight Core and Verevon

| Capability | Status and boundary |
|---|---|
| Durable operational projections | PostgreSQL implementation exists, with in-memory fallback when not configured. Durability must be checked from configuration/runtime, not inferred from an HTTP health response. |
| Metric definitions and provenance | Scorecards carry source, unit, aggregation and supporting records/events. Counts are not automatically business outcomes or revenue. |
| Verified support outcomes | The BFF reads Conversation Core's saved ticket/review records and supplies a scoped, fresh outcome snapshot to Insight Core. Source, org, actor and reporting-window checks prevent accepting a mismatched snapshot. |
| Observed event metrics | Message and agent activity can be shown from recorded events, but delivery completeness is not established for all sources. |
| Global coverage | **Incomplete.** `Overview` currently returns `coverage: unknown`; the general delivery-checkpoint contract is missing. |
| Personal/team views | Personal usage, authorized organization totals and per-user reports exist. Organization allowance and team-cost sections are hidden in personal mode. |
| Refresh/UI | Minute refresh and manual refresh exist; raw implementation diagnostics are kept out of the primary customer view. Allowances, usage and provider quota are separate sections. |
| Verevon tool access | Model Gateway has `insights_overview` / `insights.overview` plus a personal Cost Core report when an authorized credential is available. Full parity with the dashboard's source-checked outcomes, organization billing permissions and new allowance view is not established. |
| External analytics | Connector registration is not proof of authorization, actual ingestion or data completeness. Provider-specific activation and reconciliation remain separate work. |

## 4. Why “About data coverage” remains

The user's latest screenshot shows **Observed** agent-run/message metrics alongside **Verified** resolved-ticket and review metrics. This is an accurate distinction, not just a styling issue.

- **Observed** means records/events were received. It does not certify that every source event was delivered, that replay covered all history, or that the publisher never lost an event.
- **Verified** support metrics are checked against the owning service's retained source records for the requested scope/window. They do not certify every other metric or deleted historical records.
- A verified zero resolved-ticket count is possible. No recorded review decisions cannot produce a meaningful approval percentage.
- Resolved-ticket personal scope currently uses the **current assignee**, not a historical resolver. Reopened tickets are excluded. Preserve this definition unless explicitly changing the product metric.

`insight-core/internal/insights/service.go` still sets overall coverage to unknown. The BFF's source-record snapshot improves particular scorecards but does not add universal producer delivery checkpoints. A healthy container, a successful refresh, or an empty retry queue is insufficient evidence to label all data complete.

Completion means implementing and validating source-specific coverage, not removing the notice or replacing “Observed” with “Verified.” Improve the notice to identify affected sources/windows once the data contract supports that detail. Keep missing, unknown, unavailable and measured-zero states distinct.

## 5. Reset history and current local evidence

The latest authorized Aquatiq reporting baseline is **2026-10-05 03:28:59 Europe/Oslo** (`01:28:59.390138 UTC`). The earlier baseline at 03:15:20 is superseded for dashboard reporting. The recorded reset retained all 154 then-existing ledger rows and excluded two real verification requests from the current reporting window.

Those two requests had **11,015 tokens in total**. That number was valid: input includes instructions, history and tool context. Request counts and token counts measure different units. The reset did not repair an arithmetic bug or reset ChatGPT's own allowance.

At the last local browser verification in the preceding implementation turn, the current report showed zero new AI requests/tokens and no configured allowance policy. These are dated observations, not promises that live totals remain zero. The new screenshot still shows no user usage and no org Azure cost report; its operational counts may continue changing. Do not hard-code screenshot values as expected production totals.

### Verification already recorded

- Billing Core's Go suite passed against an isolated PostgreSQL fixture, serializing packages that share existing test fixtures. Later targeted allowance tests covered the final paused-quota fix.
- Cost Core's Go suite and PostgreSQL delivery integration tests passed, including partial acknowledgement/retry and stable event identities.
- Gateway billing tests, frontend allowance-contract tests, focused lint, typecheck and the production frontend build passed. Existing bundle-size warnings remain.
- Local Billing Core, Cost Core, gateway and frontend containers were healthy after rebuild/recreation.
- Browser checks verified the unconfigured allowance response, personal/org visibility, manual refresh and no captured browser errors in the final check. Earlier refresh checks are documented in the service guides.
- The latest allowance work did not create synthetic live usage, activate customer credits or initiate a paid invoice. The disposable database was removed afterward.

These checks do not certify financial settlement, every provider route, complete producer delivery, zero retention, or all tenant/role combinations. This documentation-only turn did not rerun or redeploy the system.

## 6. Future implementation plan

Resume in the following order. Each phase has a concrete exit condition; an earlier phase is not complete just because its UI looks correct.

| Phase | Owner and deliverables | Exit evidence |
|---|---|---|
| **1 — Source coverage and reconciliation** | Insight Core plus each producer: define metric/source/scope/window contracts; producer publication checkpoints, consumer progress and owner-record reconciliation; expose missing intervals and replay/backfill status. Prioritize messages and agent runs shown as Observed. | For two tenants and personal scopes, intentionally interrupt publication and consumption. Detect the gap, replay or reconcile, prove counts against the source, and mark only the covered metric/window verified. Old uncovered windows stay qualified. |
| **2 — Complete accounting delivery** | Model Gateway/producers and Cost Core: close pre-acknowledgement failure windows with durable producer handoff/reconciliation; operator-visible retries/dead letters/lag; explicit missing-token and unknown-basis handling; resolve accounting behavior for strict ZDR paths without persisting disallowed content. | Trace authorized metered and subscription requests from provider invocation to Cost Core and Billing Core with stable IDs, org/user and occurrence time; test crashes, retries, late arrivals, deletion replay and retention exhaustion. No double count, silent drop or false free request. |
| **3 — Shared Azure attribution** | Cost Core with product/finance policy: keep direct tenant mappings only for dedicated resources; add versioned showback allocation for shared resources using attributable Azure usage and matching provider/date/meter/currency buckets. Keep residual/unattributed cost explicit. | Each bucket's allocated amounts plus residual reconcile exactly to imported provider evidence, including provider corrections. Codex subscription usage is excluded from Azure allocations. Org/user permissions and exports never expose another tenant's rows. |
| **4 — Commercial allowance activation and scale** | Billing Core: approve plan units/caps/carry policy; configure least-privileged provisioning; connect policy versions to actual subscription events; add grant correction/reversal lineage, controlled policy management, scalable checkpoints and transactional reservations if hard quotas are required. | Test upgrades/downgrades, zero-use renewal, expired credit, pause/resume, late corrections, duplicate provider events and concurrent reservations. Load/recovery tests exceed current replay bounds. Previewed balance transitions equal actual transitions. |
| **5 — Prices, settlement and customer billing** | Billing Core and configured payment/invoice providers: version meter-to-price mapping and exact currency/rounding; price overage only under approved rules; immutable settlement references; credits/refunds, proration, seats, trials, discounts, cancellation, failed-payment handling and invoice/payment reconciliation. | Provider sandbox lifecycle proves duplicate/out-of-order webhooks, failed payments, refund retries, invoice corrections and totals. Raw subscription tokens never become an unapproved token charge. No real charging until rate cards and provider behavior are accepted. |
| **6 — Customer experience and Verevon parity** | Frontend, Insight Core and Model Gateway: concise actionable availability states; source freshness and coverage detail; allowance/history/expiry, invoice and payment-method flows; scoped chat tools for the same reports; supporting provenance in answers; permissioned operational troubleshooting. | UI, export and Verevon answer agree for the same scope/window/baseline and identify estimate/actual/allocation/allowance separately. Test ordinary member, owner/admin, changed membership, org switch, personal mode, stale responses and mobile use. |
| **7 — Release acceptance** | All owning planes: migration/rollback plan, load and failure recovery, secrets/permissions review, privacy/retention checks and release evidence on the intended production configuration. | A written acceptance record identifies exact deployed versions, tests, configured policies, limits and remaining exceptions. Local Docker health alone is not the release gate. |

Phases 1–3 improve truth and data delivery before financial activation. UI/tool work may proceed alongside them, but must not claim missing evidence exists. No schedule, paid provider switch, default price or launch date is committed by this roadmap.

### Required coverage contract

Agree on these fields and semantics before adding a “complete” badge:

- Owning source, metric definition/version, canonical org and optional verified actor.
- Inclusive start/exclusive end, occurrence time, ingestion time and source snapshot/checkpoint time.
- Publication and consumption positions or another auditable completeness proof; event count alone is insufficient.
- Source/consumer retention horizon, missing intervals, backfill/replay state and reconciliation result.
- Coverage at **metric + scope + time-window** level, with freshness and concise reason. Do not give a global completeness claim from one verified source.
- Clear correction semantics for deleted/reopened/late records and non-additive metrics such as averages or distinct counts.

### Decisions needed before commercial activation

| Decision | Current position |
|---|---|
| Included quantities and billable unit per plan | Not selected for the new AI allowances. Requests, input tokens and output tokens are distinct. |
| Carry-over cap and expiry | Engine supports configurable policies. One extra cycle capped at one allowance was proposed; **not approved or activated**. |
| Overage price, currency, rounding and invoice authority | Not selected for the new counters. No automatic overage charge. |
| Hard stop vs soft limit | Current allowance read is informational, not an atomic reservation. Decide before promising enforcement. |
| Shared Azure allocation | Not approved. Define eligible Azure usage, residual treatment, corrections and whether it is showback or customer chargeback. |
| Subscription/seat/trial/discount/proration rules | Existing adapters/features are not acceptance evidence for the full new lifecycle. Select and test explicit rules. |
| Credit expiry, reversals, refunds and unpaid periods | Current engine has specified unit rules; financial correction and consumer-facing policy still need agreement. |
| Privacy/retention and ZDR accounting | Confirm route capabilities, permitted accounting metadata and retention/deletion across all boundaries. Proxying alone is not a zero-retention or end-to-end-encryption guarantee. |
| Polar integration | Concepts/source patterns were reviewed; Polar is not installed as the payment provider and no provider switch was approved. |

## 7. Resume checklist

1. Read this status, the ownership matrix and the linked service guides. Inspect the working tree; it contains substantial pre-existing changes. Do not reset, clean or overwrite them.
2. Confirm the user has resumed implementation and select the next phase. Until then, make documentation changes only. No additional reporting reset is needed.
3. Refresh evidence through scoped service APIs and logs; do not reinterpret this dated snapshot as current production state or expose credentials in output.
4. Begin with the source coverage contract and a single source/window reconciliation path, then extend to remaining producers. Preserve the current source-verified support outcomes.
5. Use isolated fixtures for replay, balances, invoices and failure tests. Avoid polluting the clean Aquatiq period with synthetic ledger rows. Use any necessary real inference only as an explicitly identified verification request.
6. Record implemented vs configured vs verified status and update this document after each accepted milestone. Preserve the instruction **not to push to GitHub** unless the user changes it.

## 8. Evidence and detailed guides

- [Cross-plane orientation](../apps/CODEBASE_INFORMATION_SYSTEM.md) and [ownership matrix](../apps/master-ownership-matrix.md).
- [Allowance implementation and operations](../apps/Control%20Plane/billing-core/docs/usage-allowances.md): API, authorization, renewal, scheduled changes, limits and remaining financial workflows.
- [Azure, Insights and deployment history](../apps/Model%20Plane/go/services/cost-core/docs/azure-insights-integration.md): source-checked outcomes, per-user reporting, durable usage transport and historical verification.
- [Usage analytics and latest reporting reset](../apps/Model%20Plane/go/services/cost-core/docs/polar-inspired-usage.md).
- [Tenant metering and privacy](../apps/Model%20Plane/go/services/cost-core/docs/tenant-metering-and-privacy.md) and [Azure importer setup](../apps/Model%20Plane/go/services/cost-core/docs/azure-cost-management.md).
- [Pinned Polar source review](../apps/Model%20Plane/go/services/cost-core/docs/polar-source-review-2026-10-05.md): focused review of commit `c063d7a9ad32ddae22a41cd33e47396cb78df969`, not a complete security audit or parity certification.
- [Insight Core service](../apps/Application%20Plane/insight-core/README.md), [coverage implementation](../apps/Application%20Plane/insight-core/internal/insights/service.go), [BFF outcome composition](../apps/Frontend%20Plane/verevonv3/apps/gateway/src/domains/insights.rs), and [Verevon Insights read tool](../apps/Model%20Plane/rust/services/model-gateway/src/verevon_actions.rs).

This status document governs the current handoff. Older service notes remain useful historical evidence; a dated successful check in them does not remove a gap explicitly listed here.
