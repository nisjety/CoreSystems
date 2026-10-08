# Polar-inspired usage in Verevon

Implemented 2026-10-05. This is an adaptation of usage-reporting concepts, not a Polar deployment or a new payment processor. No Polar code was copied, no customer data was sent to Polar, and no checkout, prices or charges were changed.

**Current handoff:** [Billing, Cost and Insight Core status and future plan](../../../../../../docs/BILLING_COST_INSIGHTS_STATUS_AND_ROADMAP.md). Further implementation is on hold. Later allowance work is documented there and in Billing Core's guide; the feature table below describes the scope of this earlier analytics change.

## What users get

- Separate model-request, input-token and output-token meters. A request is a recorded provider invocation, not necessarily one user message. Input can include instructions, conversation history and tool context; generated tokens are output. A brief visible response can therefore accompany thousands of input tokens.
- API estimates separate from subscription consumption. Unknown billing types remain unclassified; missing token counts do not become free usage. Positive sub-cent estimates remain visible as `< $0.01`.
- Breakdowns by model, day and billing type, a daily activity chart, API/subscription filters, and CSV export with exact recorded decimal estimates.
- Organization scope for authorized owners/admins; personal scope for the authenticated user. Existing per-user reporting and minute refresh remain available.
- Links to the existing plan/billing and usage-limit screens for administrators. The reporting baseline does not forgive an invoice, credits consumption or budget enforcement.

## Where and how

Cost Core owns the raw cost ledger and the grouped query. `internal/postgres/analytics.go` groups the whole authorized window by UTC day, model, provider and billing basis. It does not sample the most recent requests. More than 10,000 groups returns a bounded error asking for a shorter period, never truncated totals presented as complete. The in-memory ledger has matching semantics for tests.

`GET /api/v1/cost/analytics?scope=me|organization&since=...&until=...` authenticates the caller, pins the canonical organization and personal actor, requires `cost:org:read` for every organization report, then applies the organization's latest reporting baseline. A browser cannot select another actor or disable the baseline. Responses include effective start/end timestamps, the reporting start and UTC timezone. Each query is one database snapshot. Other summary requests may naturally capture a slightly different instant while usage arrives.

The Frontend gateway exposes `/api/v1/cost/analytics` using its session-resolved organization, signed Cost Core audience token, role checks and allowlisted scope/time parameters. No browser tenant or actor selector is forwarded. The Solid frontend validates the response, filters and groups the returned aggregates, and exports only the report currently authorized for that screen. Stale results from a different identity/window are hidden. Report requests refresh with the normal Insights timer, including All time.

UI: `src/features/insights/components/UsageAnalyticsSection.tsx` and `usage-analytics.css`. Client contract: `src/shared/api/cost-client.ts`. Read/export action contract: `src/shared/actions/insights-usage.ts`. Grouping/export helpers and tests: `src/features/insights/lib/usage-analytics*`.

Exports include scope, effective window, UTC day, model, provider, billing type, request/input/output counts, API estimate and missing-token count. No prompts, responses, emails, raw user identifiers or credentials are exported. Spreadsheet formula prefixes are escaped. Subscription/unknown estimate cells are blank rather than misleading zero-dollar charges.

## Aquatiq reset

At the user's request, a second audited reporting period began at **2026-10-05 01:28:59.390138 UTC / 03:28:59 Oslo**. It excludes the two real verification requests from the previous reporting period, resetting requests and tokens together. All **154** existing ledger rows remain intact. The operation is idempotent by its documented reason. No further live inference is needed for this feature's checks; test fixtures use an isolated disposable database.

The excluded verification requests were 6,241 input + 5 output tokens for Codex and 4,768 input + 1 output for Azure: 11,015 total tokens across two provider invocations. Those original figures were valid. Resetting the reporting period does not change ChatGPT's own allowance/reset schedule.

## Polar feature mapping and remaining product work

Scope clarification, 2026-10-05: the complete billing lifecycle is in scope, including automatic allowance renewals, credit grants, carry-over, expiry and overages. Today's reporting reset is a one-time clean start, not a restriction on future billing features. The table distinguishes existing implementation from that accepted target. See the [pinned Polar source review and implementation acceptance criteria](polar-source-review-2026-10-05.md).

| Polar concept | Verevon implementation / boundary |
|---|---|
| Usage events and meters | Existing durable, idempotent provider ledger; new complete-window request/token/basis aggregates |
| Customer-level usage | Canonical organization scope and actor-pinned personal usage, plus administrator user reports |
| Cost insights | API estimates and model/day breakdowns; Azure invoice actuals remain a separate source |
| Customer portal | Existing billing and quota settings linked from usage; this change does not add invoice download or payment-method/cancellation workflows |
| Credits and included units | Billing Core already owns account credits and plan quotas; a meter-specific prepaid wallet, grant expiry and overage policy are not implemented by this analytics change |
| Subscriptions, seats, trials, discounts | Preserve existing Billing Core authority. Full Polar feature parity requires explicit product, proration, seat and discount rules |
| Revenue, profit and lifetime value | Do not calculate from cost estimates alone. Requires authoritative revenue/refund data, currency policy and period-matched cost attribution |
| Checkout, tax, payouts and merchant of record | Remain with configured billing/payment providers. Polar as a provider would require server-side credentials, customer mapping, signed replay-safe webhooks and sandbox verification before enabling real checkout |

A reporting reset is deliberately not a billing-cycle reset or a prepaid-credit grant. Pricing and entitlements should be versioned in Billing Core; cost telemetry should continue through Model Plane contracts. Provider invoices for shared Azure infrastructure require verified tenant attribution before appearing as an organization's actual charges.

## Sources reviewed

- [Polar source and feature overview](https://github.com/polarsource/polar)
- [Usage-based billing](https://polar.sh/docs/features/usage-based-billing/introduction)
- [Meters](https://polar.sh/docs/features/usage-based-billing/meters)
- [Credits](https://polar.sh/docs/features/usage-based-billing/credits)
- [Cost Insights](https://polar.sh/docs/features/cost-insights/introduction)
- [Customer portal](https://polar.sh/docs/features/customer-portal/introduction)

## Validation

Go tests cover personal/organization isolation, forged selectors, role enforcement, billing-basis separation, period clipping and baseline bypass attempts. Real PostgreSQL integration tests validate the grouped query against all three billing bases and check tenant/user/time exclusion. Frontend tests reconcile the exact two-request example and cover grouping, filters, empty totals, sub-cent formatting and safe CSV output. Typecheck, focused lint and the production frontend build pass. Live Docker/browser checks are recorded after deployment.

Live verification: Cost Core and the Rust BFF were rebuilt and deployed with the canonical launchers. The browser confirmed zero requests/input/output in the new period, in Organization and My activity, including All time and Subscription filtering. The original Organization/30d/All usage view was restored. Reporting remained at zero after verification; no additional provider request was sent. Populated breakdown/export arithmetic was checked with isolated fixtures, not fabricated production usage. A stale headline during scope changes was corrected by keying displayed workspace data to the selected scope/range/section as well as identity.
