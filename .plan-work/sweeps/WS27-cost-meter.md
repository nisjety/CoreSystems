# WS27-cost-meter: code-map sweep (F-t3-028)

Read-only sweep, 2026-10-15. No product file was edited or staged.

- Audit card: `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md`. Card lines 752-762 ("Verevon today and approach" at 760). Wave W2 table row at 1568 (section 9.1 "Wave W2" at 1551). Appendix B row at 1845 (Appendix B starts 1813). Appendix C (1918+) covers Priority items F-t3-001..023 only, so F-t3-028 has no C entry. Appendix D (2111-2160) has no F-t3-028 line.
- W = worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean except `.plan-work/`). L = live `C:/dev/CoresSystem` (HEAD afb83928, branch main, 324 entries in `.plan-work/live-tree-status.txt`).
- Paths are repo-relative and identical in W and L. Line cells give `W:` or `L:` only where the trees differ. Live status: M = modified in L, ?? = only in L, clean = not listed in live-tree-status.txt.
- Not run: no test or build was executed. Files marked "grep-only" were located by grep and not read in full.

## 0. Findings

1. Card citations: `sse_events.rs:268-286` matches L exactly (the Usage variant) and has drifted in W to 228-244. `chat-client.ts:335-350` matches the L type declaration and has drifted in W to 323-339. The cache read itself is in parse code (L:1394-1395 and L:1640-1641; W:1363-1364 and W:1608-1609). `routing_policy.rs:126-134` matches W for the 80 percent constant (line 131), but the one-tier downgrade is in `provider/intent.rs:211-216`, not in that file.
2. Gap confirmed: the `ChatEvent::Usage` frame has no cache fields in W or L (L:268-286). Cache tokens already reach cost-core: the NATS usage envelope carries `cache_read_input_tokens` and `cache_creation_input_tokens` (W:grpc.rs:555), and the cost-core ledger has `CacheReadInputTokens` (ledger.go:93-100, roll-up 202-206, 361; migration `0004_cache_tokens`). The SPA parses cache keys (chat-client.ts) but `onUsage` (use-chat-controller.ts:3187-3202) copies only tokens, latency, cost, basis, servedBy and confidence. No producer sends cache keys on the usage frame.
3. ABSENT in code: no UsageProgress, budget-warning, quota-warning or allowance-warning frame in model-gateway/src, proto or SPA. No pre-run USD estimate in the chat UI (ChatPanels.tsx:620 and :702 show token estimates only; ChatMessages.tsx:1726 says cost is "never estimated"). The only server-side USD estimator found is fine-tuning: `finetune_routes.rs:192` (`server_estimate_cost_usd`), usable as a precedent.
4. Silent downgrade confirmed: `BudgetPosture` is not referenced anywhere in model-gateway/src. The only use of `Decision.posture` outside tests is a tracing field (`provider/fallback.rs:1034`). The one-tier shift (`intent.rs:211-216`) never reaches an SSE frame.
5. Quotas and allowances exist as the card says: org-core `org_quotas` with `reset_period` (migration 002), billing-core allowance engine and routes (org-scoped only), gateway BFF routes, Insights `BillingAllowancesSection` (org scope) and `CostDashboardPage`.
6. Two budget gates with different failure semantics: model-gateway `budget.rs` (fail closed, 503) and inference-core `BudgetClient` (400 ms timeout, falls back to posture Unknown). A warning design must name one authority.
7. Live-tree hazard: the Usage frame and its emitters are uncommitted in L (sse_events.rs adds `retention_class` at L:283-285; sse.rs +781 lines, Usage emitted at L:4242). Any cache-field change to the frame conflicts with that ZDR work.
8. Card plan "I16" (card 760): not found by grep in `C:/dev/verevon-audit-2026-10-14/final/` or `cons/`.

## 1. Card facts (F-t3-028, audit 752-762)

| Field | Value | Source |
|---|---|---|
| Status / effort / wave | Partial / L / W2, confidence High | card 754 |
| Plane | Model Plane (cost-core, model-gateway events) with Control Plane (quotas) and Frontend Plane | card 754 |
| Ask | cumulative usage progress after each provider call; live meter in Work panel and Run Console; warn at 80 percent of run or org ceiling; labelled estimate later; honesty contract (cost null when unknown; basis marks subscription) | card 756 |
| Approach | plan I16 (UsageProgress frames plus warning); add cache fields to Usage frame; surface silent downgrade in same warning; drop disjoint reasoning tokens; defer attribution, thread totals, estimate; ZDR turns compute meter in-stream | card 760 |
| Dependencies | F-t3-022 (card 665), F-t3-042 (card 921); per-user allowance needs Control Plane billing support | card 762 |
| Downstream | F-t3-036 spend gate (card 849-858), F-t3-083 run admission (1644), F-t3-092 council (1653) | audit |
| Mentions | executive summary 63; risk 10.1 at 1715 ("F-t3-022 and F-t3-028 come first") | audit |

## 2. Citation verification

| Card id | Cited path:lines | Claim (short) | W verdict | Live status | Corrected path:lines |
|---|---|---|---|---|---|
| F-t3-028 | apps/Model Plane/rust/services/model-gateway/src/sse_events.rs:268-286 | Usage frame, no cache fields | DRIFT: Usage variant at W:228-244 (doc 228, `Usage {` 229, `},` 244); no cache fields | M (adds retention_class) | L:267-286 MATCH (doc 267, variant 268-286); W:228-244 |
| F-t3-028 | apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts:335-350 | client reads cache_read_tokens | DRIFT: type `ChatUsageEvent` W:323-339 (cacheReadTokens 330); parse W:1355-1364 and W:1600-1609 | M (+52) | L:335-351 MATCH for type (cacheReadTokens 342, cacheWriteTokens 343); parse L:1386-1395 and L:1632-1641 |
| F-t3-028 | apps/Model Plane/rust/services/inference-core/src/provider/routing_policy.rs:126-134 | 80 percent Constrained shifts one tier cheaper | PARTIAL MATCH: `constrained_fraction: 0.8` at :131 inside `impl Default` (125-134); tier shift is not in this file | clean | routing_policy.rs:131 (constant), :137-139 (doc); behaviour at provider/intent.rs:211-216 (choose) and :413-426 (classify_posture, `>= cap * fraction` at 422-423) |
| F-t3-028 | (no path) usage emitted once at turn end | claim on emission | MATCH on streaming path: sse.rs:4112 (`ChatEvent::Usage` after citation anchoring) | M | L:sse.rs:4242 (same site); other constructions W:6849, W:9716 / L:6983, 8525, 10077 not opened |
| F-t3-028 | (no path) cost-core ledger with per-run aggregate | exists | MATCH | clean | cost-core/internal/server/server.go:142 (GET /api/v1/cost/run); ledger/ledger_test.go (TestStore_GetRunUsage, :184) |
| F-t3-028 | (no path) org allowance page | exists | MATCH | clean | SPA features/insights/components/BillingAllowancesSection.tsx, mounted at InsightsPage.tsx:259 (org scope only) |
| F-t3-028 | (no path) quotas with reset period | exists | MATCH | clean | org-core/migrations/002_add_enterprise_fields.up.sql:2 (CREATE org_quotas), :7 (reset_period), :79 (seed rows); org-core/internal/http/handlers.go:400 (getQuotas), :420 (putQuota) |
| F-t3-028 | (no path) silent 80 percent posture | exists | MATCH | clean | provider/intent.rs:413-426; sole consumer log at provider/fallback.rs:1034 |
| F-t3-028 | (no path) plan I16 | UsageProgress plan | NOT DETERMINED (not in audit final/ or cons/; not in repo) | n/a | n/a |

## 3. Affected-file map (depth 2)

Columns: path (role) | lines (W unless marked L) | live | tests (path, test names where cheap).

### 3.1 Model Plane: model-gateway (`apps/Model Plane/rust/services/model-gateway/src/`)

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| sse_events.rs | ChatEvent enum; Usage variant (input/output tokens, cost_usd, cost_basis, latency_ms, confidence, served_by); SSE name map; JSON writer | W:228-244, W:410; L:267-286, L:472, L:580-610 (writer; cost_usd always written at L:593, cost_basis at L:598-600, served_by at L:602-603) | M | sse_events.rs: usage_cost_basis_explains_a_subscription_cost_and_is_otherwise_optional (W:1631), usage_carries_server_stamped_provenance_and_omits_it_when_empty (W:1692), turn_timing_is_gated_exactly_like_usage (W:1796); L adds usage_names_its_retention_class_only_when_one_is_stamped and a_waived_usage_frame_states_its_class_and_reason |
| sse.rs | Turn paths (stream and non-stream): builds Usage frame, reads upstream cache tokens, writes usage envelope | W Usage at 4112, 6849, 9716; W cache reads 3183, 3998, 4038, 6442, 6822, 6844. L Usage at 4242, 6983, 8525, 10077; L cache 3312-3315, 4128-4129, 4168-4169, 6576-6579 | M (+781; ZDR posture wiring) | sse.rs mod tests: L:10165-10202 helper writes cache_read_input_tokens/cache_creation_input_tokens; L:11642-11713 assert them |
| grpc.rs | publish_usage_envelope: NATS usage envelope (input_tokens, cache keys, cost_basis) to mp.v1.usage.{org} | W:510-566 (envelope 524, cache keys 555, publish 565); call site W:857 | M (+47; no usage/cache hunks) | grpc.rs:4472, :4544 (subject starts_with "mp.v1.usage.") |
| http_routes.rs | Second usage-envelope publisher on HTTP route; ZDR gates | W:9235-9273 | M (+188; not read in full) | not determined |
| budget.rs | Pre-flight budget gate: POST {COST_CORE_URL}/api/v1/budget/check; 402 over budget; 503 on auth/transport/parse failure; org ceiling applies when request names no cap | check_budget :48, check_budget_at :82, URL :101, env :13-14 | clean | budget.rs:194 authenticated_budget_check_forwards_verified_user_scope; :228 budget_dependency_auth_or_transport_failure_fails_closed; :266 an_org_ceiling_applies_to_a_request_that_names_no_cap; :299 a_request_cap_wins_over_the_org_ceiling_only_where_it_is_set |
| org_quota.rs | Reads org ceilings from Control Plane quota rows: max_cost_per_run_usd_micros (:24), max_tokens_per_run (:26); usd/micro conversion (:34, :40); fetch_org_limits (:112) | 24-112 | clean | :164 micro_dollars_round_trip_without_truncation; :174 both_run_ceilings_are_read_from_the_payload; :186 zero_or_negative_ceiling_reads_as_unset; :197 missing/malformed payload yields no ceilings |
| pricing.rs | CostBasis enum (:47), of_route (:60), served_by (:74), as_str (:86); PricingCache (:179), new(Option base_url) (:191), cost_usd (:213), cost_usd_for (:240), cost_from_rate (:314) | 47-314 | clean | :541 cost_basis_follows_the_serving_provider_and_falls_back_to_the_route; :533, :564 construct PricingCache::new(None, ...) |
| state.rs | AppState; PricingCache built with None when disabled (:427); comment at :426 "A disabled cache emits a null cost, never a fake" | W:426-427 | M (+5) | not determined |
| turn_timing.rs | Per-turn timing report; TurnTiming frame gated like usage | whole file | clean | via sse_events.rs W:1796 |
| finetune_routes.rs | server_estimate_cost_usd (W:192) and its use (W:213); precedent for a labelled USD estimate | W:192-213 | M (no estimate hunk) | finetune_routes.rs:1624 (100 examples on gpt-4o-mini = 0.50) |
| tool_loop.rs, gateway_metrics.rs, implicit_feedback.rs | Tool loop / metrics; no cost-frame relevance found | - | M | not determined |

### 3.2 Model Plane: inference-core and execution-core

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| rust/services/inference-core/src/provider/intent.rs | BudgetPosture enum (177-189; Constrained 181-183; Exhausted 184-185); CHEAP_FALLBACK (196); choose (201-218; downgrade 211-216); resolve (236-263; posture call with constrained_fraction at 257); BudgetClient (276-345; endpoint 331 `{base}/api/v1/budget/check`; timeout 314 = 400 ms); posture (345-409); classify_posture (413-426) | as listed | clean | intent.rs:851 exhausted_budget_forces_cheap_fallback; :872 constrained_budget_downgrades_one_tier; :897 unknown_posture_routes_like_healthy; :942 classify_posture_thresholds; :986 resolve_verevon_balance_without_budget_is_unknown_posture; :1007 budget_client_rejects_empty_url; :1017 posture_forwards_the_caller_bearer_as_authorization; :1043 posture_with_empty_bearer_short_circuits_to_unknown |
| rust/services/inference-core/src/provider/routing_policy.rs | RoutingPolicy: budget_cap_usd 50.0 (:130), constrained_fraction 0.8 (:131), cheap_fallback (:132), mode table (140-160), doc (137-139) | 125-160 | clean | :297, :332-333, :370-371 (JSON fixtures) |
| rust/services/inference-core/src/provider/fallback.rs | Only non-test consumer of Decision.posture: tracing field at :1034 | :1028-1036 | clean | rust/services/inference-core/tests/fallback_test.rs |
| rust/services/inference-core/src/config.rs | COST_CORE_URL read (:236) -> BudgetClient; doc (:120) | 120, 236 | clean | not determined |
| rust/services/inference-core/src/grpc.rs | gRPC mapping of provider errors; no cost frame | - | M (not keyword-checked) | not determined |
| rust/services/execution-core/src/runtime_loop/agent.rs | Do loop; RunDeadline::NONE hunks in live diff | - | M (+193; 2 keyword hits) | not determined |
| rust/services/execution-core/src/llm_planner.rs | grep-only: contains a cost-core string (not opened). No cost-core client found in execution-core otherwise | - | clean | not determined |

### 3.3 Model Plane: cost-core (Go; `apps/Model Plane/go/services/cost-core/`)

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| internal/server/server.go | Routes (134-149): POST /api/v1/cost/record (140), GET /api/v1/usage (141), /api/v1/cost/run (142), /cost/aggregate (143), /cost/users (144), /cost/analytics (145), /cost/entries (146), /cost/azure/actual (147), GET /api/v1/pricing (148), POST /api/v1/budget/check (149); scope gate (221-228: budget check needs cost:read); handleBudgetCheck (540); org-wide scope when user_id empty (585-589); budgetDecision (664) | as listed | clean | internal/server/server_test.go: TestRecordThenUsage (:51), TestRecordThenUsageWithCacheTokenTelemetry (:115); auth_test.go; analytics_test.go; reporting_test.go; user_usage_test.go |
| internal/ledger/ledger.go | Sentinel errors (26-31); Entry with CacheReadInputTokens/CacheCreationInputTokens (93-100); ValidateBudget (165); roll-up totals (202-206, 361); CheckBudget (537-554) | as listed | clean | ledger/ledger_test.go: TestStore_RecordAndGetUsageRollsUpCacheTokens (:37), TestStore_CheckBudget (:253), TestNormalizeCostBasis (:283), TestStore_GetRunUsage (:184); ledger/user_usage_test.go: TestUserUsageIncludesEntireWindowAndSeparatesTenants (:10) |
| internal/ledger/user_usage.go, reporting.go, analytics.go, erasure.go | Per-user rollups, reporting, analytics, erasure | - | clean | erasure_test.go, user_usage_test.go |
| internal/postgres/store.go, user_usage.go, reporting.go, analytics.go, billing_delivery.go | Postgres store and billing-delivery outbox | - | clean | store_test.go; store_integration_test.go (skips unless COST_CORE_TEST_DATABASE_URL, :30-32); billing_delivery_test.go; billing_delivery_integration_test.go |
| internal/pricing/pricing.go | Pricing catalogue, cache-tier rates | - | clean | pricing_test.go: TestCostWithCacheAppliesTheTenXCacheReadDiscount (:51), TestCacheRatesMirrorModelGateway (:76) |
| internal/consumers/consumer.go | NATS usage consumer for mp.v1.usage.* | - | clean | consumer tests not determined |
| cmd/main.go | Usage subject constant (41-42 `mp.v1.usage.*`); usage consumer loop (307) | 41-42, 307 | clean | cmd/main_test.go: TestUsageEnvelopeMustMatchAuthenticatedSubjectScope (:23), TestUsageEnvelopeDecodesCacheTokenTelemetry (:80), TestUsageEnvelopeSubscriptionTurnLedgersRealTokensAtZeroCost (:156); cmd/usage_retry_test.go; cmd/usage_timestamp_test.go; cmd/nats_auth_test.go |
| migrations/0004_cache_tokens.up.sql, 0005_cost_basis.up.sql, 0009_usage_reporting_periods.up.sql, 0010_billing_usage_outbox.up.sql | Schema for cache tokens, cost basis, reporting periods, outbox | - | clean | - |

### 3.4 Model Plane: proto, docs and contracts

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| proto/model_plane/v1/gateway.proto | InvokeRequest.max_cost_usd = 14 (223-224, "Gateway checks against cost-core before forwarding"); max_tokens_budget = 15 (226-227); InvokeResponse token comment (325); Trajectory.cost_usd = 11 (958-959); OrgPolicy.max_cost_per_run_usd = 2 (1346); GetAnalyticsResponse.total_cost_usd = 4 (1435). No UsageProgress message | as listed | clean | buf lint has pre-existing debt (Model Plane CLAUDE.md) |
| proto/model_plane/v1/sessions.proto | Session RPCs | - | M (+34; no cost hunks) | - |
| proto/model_plane/v1/routing_policy.proto | Routing policy proto (exists; not read) | - | not determined | - |
| docs/sse-event-taxonomy.md | Generated taxonomy; `usage` row at line 38 maps to ChatEvent::Usage | 38 | M (+4) | CI gate: .github/workflows/sse-taxonomy.yml:45 runs `scripts/gen-sse-event-taxonomy.py --check` |
| docs/capability-ownership-matrix.md | Owner rules: row 63 (provider routing, inference-core), row 74 (cost/usage ledger, cost-core) | 63, 74 | clean | - |
| docs/decisions/ledger.md | 45 "budget" hits; first 6 are unrelated to a cost meter (414-536). No cost-meter decision found | - | clean | - |
| docs/ZDR.md | ZDR rules (7 keyword hits, ZDR wording) | - | M (+52) | - |
| api/contracts/ | zdr-posture-matrix.json, connection-zdr-capabilities.json, infer-statuses.json (no usage contract found) | - | clean (zdr-refusals.json is ??) | - |
| deploy/docker-compose.yml | COST_CORE_URL default http://model-plane-cost-core-1:8089 (477, 857; comment 853 "budget gate is best-effort"); COST_CORE_AUTH_AUDIENCE default cost-core,inference-core (1779); MODEL_COST_CORE_NATS_PASSWORD (51, 1756) | as listed | clean | - |

### 3.5 Control Plane: billing-core and org-core

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| apps/Control Plane/billing-core/internal/billing/allowance.go | Allowance engine: validate (45, 110, 126), paused (69), allowanceBoundary (137), CalculateAllowance (157) | as listed | clean | internal/billing/allowance_test.go: TestAllowanceEndsAtExactContractBoundary, TestAllowanceRolloverExpiryAndAnchor, TestAllowanceGrantExpiryAndOverage, TestAllowanceResetNeverDropsHistory, and others; allowance_postgres_test.go: TestAllowancePostgresConcurrentUsageIsolationAndReplay (DB-gated, :16-18) |
| internal/billing/allowance_repository.go, allowance_revisions.go | Storage and scheduled revisions | - | clean | allowance_postgres_test.go |
| internal/billing/usage.go | Usage event validation (ValidateUsageEvent :39 and related) | 26-103 | clean | internal/billing/usage_test.go |
| internal/http/server.go | Group /api/v1/billing (:60); GET/PUT /orgs/:orgId/allowances (:62-63); POST grants (:64), POST revisions (:65); GET /orgs/:orgId/quotas/:metric (:69). All org-scoped; no per-user route | 60-69 | clean | - |
| internal/http/allowances.go | Handlers: getAllowances (:24), configureAllowance (:32), grantAllowance (:44), scheduleAllowanceRevision (:57) | 24-57 | clean | internal/http/allowances_auth_test.go: TestAllowanceRoutesRequireDelegatedTenantAndSeparateWriteScope (:11) |
| internal/http/handlers.go | getQuotaStatus (:178) | 178 | clean | - |
| migrations/0009_allowance_ledger.up.sql, 0010_allowance_revisions.up.sql | Schema (latest 0010) | - | clean | - |
| apps/Control Plane/org-core/internal/http/handlers.go | getQuotas (400), putQuota (420) | 400-420 | clean | not determined |
| apps/Control Plane/org-core/internal/http/server.go | /organizations/:id/quotas (96-97, guard on PUT); legacy /orgs/:id/quotas (114-115) | 96-115 | clean | - |
| apps/Control Plane/org-core/internal/org/types.go | Quota struct, ResetPeriod (39: daily, monthly, none) | 39 | clean | - |
| apps/Control Plane/org-core/migrations/002_add_enterprise_fields.up.sql | CREATE org_quotas (2), reset_period (7), seed rows (79) | 2-90 | clean | - |

### 3.6 Frontend Plane: gateway (`apps/Frontend Plane/verevonv3/apps/gateway/src/`)

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| domains/cost.rs | BFF routes /api/v1/cost/summary (:37) and /api/v1/cost/pricing (:42); handlers `summary` and `pricing` not read. These paths are not in cost-core's route table | 37, 42 | clean | not determined |
| domains/billing.rs | BFF routes /api/v1/billing/allowances (:21) and /api/v1/billing/quotas/{metric} (:26), proxied to billing-core /api/v1/billing/orgs/{org}/... (55-63, 120-131) | as listed | clean | not determined |
| domains/orgs.rs | Routes GET /api/v1/orgs/{id}/quotas (:51), PUT /api/v1/orgs/{id}/quotas/{key} (:52) | 51-52 | clean | - |
| domains/orgs/quotas.rs | list_quotas (37), set_quota (72), proxies to org-core; key allowlist; test only_enforced_quota_keys_are_settable (:163) | 37-163 | clean | quotas.rs:163 |
| domains/actions/dispatchers.rs | dispatch_org_set_quota (4335-4363) | 4335-4363 | clean | - |
| domains/actions/handlers.rs | Routes action "org.set_quota" (:304) | 304 | clean | - |
| domains/integrations/model_subscriptions.rs | Codex allowance read (:66) | 66 | clean | - |

### 3.7 Frontend Plane: SPA (`apps/Frontend Plane/verevonv3/src/`)

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| shared/api/chat-client.ts | ChatUsageEvent type with cacheReadTokens/cacheWriteTokens; `usage` frame parse (cache keys read at parse sites) | W:323-339, W:1355-1364, W:1600-1609; L:335-351, L:1386-1395, L:1632-1641 | M (+52, no cost hunks) | chat-client.test.ts: usage frames at W:1685 (subscription), W:1686 (other basis), W:1708 (cost only) |
| shared/api/chat-client.test.ts | Usage/turn tests | - | M (+20) | see above |
| shared/api/ag-ui-client.ts | Second usage parser (652-659, cache keys) and usage.recorded projection (184, 469-476) | as listed | clean | not determined |
| shared/api/sse.ts | SSE transport; grep-only, no usage/cost/cache hunk | - | M (0 hits) | - |
| shared/chat/verevon-ui-events.ts | UI event `usage.recorded` type carrying cacheReadTokens (:32) | 32 | clean | - |
| features/chat/components/use-chat-controller.ts | onUsage handler: copies inputTokens, outputTokens, latencyMs, costUsd (undefined on subscription), costBasis, servedBy, confidence (W:3187-3202; L same lines for 3187 and 3197) | W:3187-3205 | M (+4) | use-chat-controller.recovery.test.ts; zdr-refusal test |
| features/chat/components/ChatMessages.tsx | Cost rows: basis-aware "Kostnad" (W:1634-1635, 1739-1740, 1770, 1783, 1791; L:1742-1744 for the MetricRow); comment "never estimated" (1726) | as listed | M (+181, no cost hunks) | ChatMessages.metrics.test.tsx :57-83 (subscription wording; metered $ value at :65) |
| features/chat/components/ChatPanels.tsx | latestUsage summary (915-927) and "Prompt cache" metric (928-936, shown when cacheTotal > 0); token estimates only (559, 620, 702) | 915-936 | clean | ChatPanels.test.tsx (not read) |
| features/chat/components/chat-types.ts | Message type: costUsd/costBasis (L:283-286) | L:283-286 | M (+5) | - |
| features/chat/components/chat-normalizers.ts | Normalizers (no cost hunks) | - | M (+37) | chat-normalizers.wave2.test.ts (not read) |
| features/agents/components/AgentRunConsole.tsx | Run Console cost: RunUsage.costUsd (L:142-145), usage: RunUsage or null (L:162, 189) | L:142-189 | M (+9) | AgentRunConsole.test.tsx (M) |
| features/cost/components/CostDashboardPage.tsx | Cost page on /api/v1/cost/* via cost-client | - | clean | CostDashboardPage.test.tsx:45 (subscription shown as included, not $0.00) |
| features/insights/components/InsightsPage.tsx | Mounts BillingAllowancesSection for organization scope with cost access (:23 import, :259) | 23, 259 | clean | - |
| features/insights/components/BillingAllowancesSection.tsx | Allowance UI (org scope) | - | clean | not determined |
| shared/api/cost-client.ts | Calls /api/v1/cost/analytics (:24), /cost/users (:70), /cost/summary (:137), /cost/azure/actual (:147), /cost/entries (:161), /cost/pricing (:168) | 24-168 | clean | not determined |
| shared/api/billing-allowances.ts | GET /api/v1/billing/allowances; zod allowanceSchema (current, history, next_change) | whole file | clean | billing-allowances.test.ts |
| shared/api/org-quota-client.ts | OrgQuota type; keys max_cost_per_run_usd_micros and max_tokens_per_run; QuotaResetPeriod; micro-dollar conversion; no write of `value` | whole file | clean | not determined |
| shared/actions/action-registry.ts | org.set_quota (2739-2746; input 978); maxCostUsd (1105); routing-policy budget_cap_usd (1200), mode table (1220); billing/insights actions registered | as listed | clean | action-surface-contract.test.mjs (package.json test script) |
| shared/actions/billing-allowances.ts | Action billing.allowances.read (:4) | 4 | clean | - |
| shared/actions/insights-usage.ts | Actions insights.usage.read and insights.usage.export (:5) | 5 | clean | - |
| shared/actions/action-client.ts | "estimatedCost" is a risk label ("low token budget" etc., 14-17, 38), not USD | 14-38 | clean | - |
| features/chat/components/ChatWorkPeriodPicker.tsx | Live-only period picker (??), no cost hits | - | ?? (missing in W) | - |

Note: no SPA component was found that calls the org quota PUT route (gateway orgs.rs:52). Whether any UI consumes it is not determined.

## 4. Related contracts

- SSE `usage` frame (event name "usage", sse_events.rs W:410 / L:472): keys input_tokens, output_tokens, cost_usd (always written; null when unknown, L:593), latency_ms, confidence, cost_basis (omitted when None, L:598-600), served_by (three keys, omitted when None, L:602-603). Live-only: retention_class (L:283-285, written only when Some). Cache keys are not part of this frame.
- SSE `turn_timing` frame: gated exactly like usage (test W:1796).
- SPA UI event `usage.recorded` (verevon-ui-events.ts:32) includes cacheReadTokens and cacheWriteTokens.
- NATS: subject `mp.v1.usage.{org_id}` (model-gateway grpc.rs:565 publish; cost-core cmd/main.go:41-42 subscribe `mp.v1.usage.*`). Envelope payload includes input_tokens, cache_read_input_tokens, cache_creation_input_tokens, cost_basis (grpc.rs:524-557).
- cost-core HTTP (server.go:140-149): /api/v1/cost/record, /api/v1/usage, /api/v1/cost/run, /api/v1/cost/aggregate, /api/v1/cost/users, /api/v1/cost/analytics, /api/v1/cost/entries, /api/v1/cost/azure/actual, /api/v1/pricing, /api/v1/budget/check. Scope cost:read for budget check (server.go:221-228).
- billing-core HTTP, group /api/v1/billing (server.go:60-69): org-scoped allowances and quota status only.
- org-core HTTP: /api/v1/organizations/:id/quotas and PUT /:key (server.go:96-97); legacy /orgs/:id/quotas (114-115).
- Gateway BFF: /api/v1/billing/allowances, /api/v1/billing/quotas/{metric} (billing.rs:21, :26); /api/v1/orgs/{id}/quotas (orgs.rs:51-52); /api/v1/cost/summary and /api/v1/cost/pricing (cost.rs:37, :42).
- Action registry ids (SPA): org.set_quota (action-registry.ts:2739), billing.allowances.read (shared/actions/billing-allowances.ts:4), insights.usage.read and insights.usage.export (insights-usage.ts:5). Per repo CLAUDE.md, add or update the action contract before wiring a meaningful UI operation.
- Proto: gateway.proto fields listed in 3.4. No UsageProgress message exists. A proto addition would be an additive change in the gateway proto.
- Env: COST_CORE_URL (budget.rs:13-14, default http://localhost:8089; inference-core config.rs:236; compose 477, 857); COST_CORE_AUTH_AUDIENCE (compose 1779); MODEL_COST_CORE_NATS_PASSWORD (compose 51, 1756); COST_CORE_TEST_DATABASE_URL (store_integration_test.go:30); CONTROL_LIFECYCLE_TEST_DATABASE_URL (allowance_postgres_test.go:16).
- Config JSON: RoutingPolicy fields budget_cap_usd, constrained_fraction, cheap_fallback (routing_policy.rs:130-132; test JSON fixtures :332-333, :370-371); org routing policy write is action-registry.ts:1200-1220.
- SQL migrations, latest names (lexical last of each dir, worktree):
  - cost-core: `0010_billing_usage_outbox.up.sql` / `.down.sql` (0009_usage_reporting_periods, 0008_erasure_replay_guard before it).
  - billing-core: `0010_allowance_revisions.up.sql` / `.down.sql` (0009_allowance_ledger before it).
  - org-core: `020_interactive_retention_outbox_org_id_check.up.sql` / `.down.sql` (org_quotas created in `002_add_enterprise_fields.up.sql`).
  - session-core (W): `0043_audit_outbox_requeue.sql` is the last. L-only (??): `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `migrations/revert/`.

## 5. Test and build commands

- Rust, Model Plane (`apps/Model Plane/rust/`, workspace members at `Cargo.toml:14` services/model-gateway and `:16` services/inference-core):
  - `cargo test -q -p model-gateway --all-targets` and `cargo test -q -p inference-core --all-targets` (Model Plane CLAUDE.md, Testing).
  - `cargo test --workspace --no-fail-fast` (CI: `.github/workflows/model-plane-rust-ci.yml:119`).
  - Crate names from Cargo.toml [package]: model-gateway, inference-core, execution-core, session-core, mp-contracts.
  - User memory (MEMORY.md, Model Plane build entry) says two cargo targets fail on Windows and to use `--lib`. Not re-verified here.
- Go, cost-core (`apps/Model Plane/go/services/cost-core/`, module `github.com/triodelab/model-plane/services/cost-core`):
  - `go test ./...` and `go vet ./...` (Model Plane CLAUDE.md, Testing).
  - Postgres integration test skips unless COST_CORE_TEST_DATABASE_URL is set (store_integration_test.go:30-32).
  - No .github/workflows file references cost-core (grep). Whether the Go suite runs in CI is not determined.
- Go, billing-core (`apps/Control Plane/billing-core/`, module `github.com/I-Dacosta/AquatiqCMS/apps/billing-core`):
  - `go test -v -p 1 ./...` (`.github/workflows/billing-core-service.yml:84`, working directory line 32).
  - DB-gated tests need CONTROL_LIFECYCLE_TEST_DATABASE_URL (allowance_postgres_test.go:16-18).
- Go, org-core (`apps/Control Plane/org-core/go.mod` exists): workflow `.github/workflows/org-service.yml` exists (not read). Exact command not determined.
- Frontend gateway (`apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml`, crate verevon-gateway-rs):
  - `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` (repo CLAUDE.md, Build And Run).
  - CI: `.github/workflows/verevonv3-ci.yml:141` (`cargo test`; working directory not read).
- Frontend SPA (`apps/Frontend Plane/verevonv3/`, package.json scripts):
  - `pnpm test` (node --test on gateway scripts, then `vitest run`), `pnpm typecheck` (`tsc -b`), `pnpm lint` (`eslint .`), `pnpm verify` (lint, typecheck, test, build). CI runs `pnpm verify` at `.github/workflows/verevonv3-ci.yml:193`.
  - Single files: `pnpm vitest run <file>` for chat-client.test.ts, ChatMessages.metrics.test.tsx, CostDashboardPage.test.tsx, billing-allowances.test.ts, use-chat-controller.recovery.test.ts.
- Contract gates:
  - `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` (`.github/workflows/sse-taxonomy.yml:45`). Run it after any SSE frame change.
  - `api/contracts/*.sha256` checks: tool not determined.
- Scope note: `apps/Model Plane/CLAUDE.md` says one canonical owner per capability; check `docs/capability-ownership-matrix.md` and `docs/decisions/ledger.md` before adding a second budget or meter path.

## 6. Owners

- Repo CLAUDE.md, Architecture Rules: "Control Plane owns identity, users, orgs, billing, sessions, audit, quotas, and entitlements" (line 23). So quota rows (org-core) and allowances (billing-core) are Control Plane.
- Repo CLAUDE.md line 27: "Model Plane owns reasoning, sessions/runs, inference, execution loops, capabilities, sandboxes, browser grants, and cost." So the meter, usage frames and budget posture are Model Plane.
- Repo CLAUDE.md (Architecture Rules): Frontend Plane owns the Verevon v3 UI and same-origin gateway/BFF normalization.
- `apps/master-ownership-matrix.md`, table with header "Tool / feature | Quarry v2 | Data Plane | Model Plane | App Shell | Rust | Go | Python" (table containing line 88):
  - line 88, "Cost ledger / budgets": Model Plane column "**Owner** inference/agent cost"; Go column "**cost-core-go**: ledger of record; the chat budget counts chat rows only".
  - line 89, "Background provider metering": Data Plane owns the usage event; cost-core is ledger of record.
  - line 133, service table: `cost-core` = Go, "Token/cost ledger, budgets, usage events".
  - line 175, rule 16: every paid provider call is metered under an org-attributed feature; background rows are excluded from the chat budget and cannot downgrade a person's chat model.
- `apps/Model Plane/docs/capability-ownership-matrix.md`:
  - row 74, "Cost / usage ledger": owner cost-core (Go), Postgres, gateway emits usage.* over NATS.
  - row 63, "Provider routing / inference": owner inference-core (provider/*), usage over NATS.
- Ownership by code path (derived): posture and downgrade in inference-core (provider/intent.rs); hard pre-flight gate in model-gateway (budget.rs); usage frame in model-gateway (sse_events.rs, sse.rs); ledger in cost-core; quota rows in org-core; allowances in billing-core; BFF in Frontend gateway; display in SPA. The card's plan is a Model Plane change with Control Plane quota and Frontend display.

## 7. Uncommitted-only (live tree)

Live-tree-status entries touching this workstream, with a one-line summary from `git --no-optional-locks diff`:

| Path (L) | Status | Summary |
|---|---|---|
| model-gateway/src/sse_events.rs | M (+192) | Usage frame gains `retention_class` (L:283-285, written at about L:606) and two ZDR tests. No cache fields added. |
| model-gateway/src/sse.rs | M (+781) | ZDR posture wired into turn paths; Usage emitted with retention_class (L:4242). 82 hunk lines match the cost/usage/cache/budget/posture keyword set; a sampled subset is ZDR posture wording (rest not read). |
| model-gateway/src/http_routes.rs | M (+188) | ZDR gates (zdr_posture::zdr_or_refuse, continuation_gate) and usage-keyword hits (64); not read in full. |
| model-gateway/src/tool_loop.rs | M (+352) | ZDR posture test doubles; 1 keyword line. |
| model-gateway/src/grpc.rs | M (+47) | No usage or cache hunk (0 keyword hits). |
| model-gateway/src/finetune_routes.rs | M | No server_estimate_cost_usd hunk (0). |
| model-gateway/src/state.rs | M (+5) | 0 keyword hits. |
| model-gateway/src/gateway_metrics.rs | M (+9) | 0 keyword hits. |
| execution-core/src/runtime_loop/agent.rs | M (+193) | RunDeadline::NONE (run_budget) hunks; 2 keyword hits; not a cost-core change. |
| inference-core/src/grpc.rs | M | Not keyword-checked. |
| proto/model_plane/v1/sessions.proto | M (+34) | 0 keyword hits. |
| docs/sse-event-taxonomy.md | M (+4) | 0 hits. Regenerate if frames change. |
| docs/ZDR.md | M (+52) | 7 ZDR keyword hits. |
| api/contracts/README.md | M | Not checked. |
| shared/api/chat-client.ts | M (+52) | 0 cost, usage or cache hunks; shifts usage-parse lines (+12 type, +31 parse). |
| shared/api/chat-client.test.ts | M (+20) | 0 hits. |
| shared/api/sse.ts | M | 0 usage, cost or cache hits. |
| features/chat/components/use-chat-controller.ts | M (+4) | 0 hits; onUsage lines unchanged at L:3187-3197. |
| features/chat/components/ChatMessages.tsx | M (+181) | 0 cost hits; cost rows at L:1742-1744. |
| features/chat/components/chat-types.ts | M (+5) | 0 hits. |
| features/chat/components/chat-normalizers.ts | M (+37) | 0 hits. |
| features/agents/components/AgentRunConsole.tsx | M (+9) | 0 hits. |
| features/agents/components/AgentRunConsole.test.tsx | M | Not checked. |
| L-only (??): model-gateway/src/zdr_posture.rs, zdr_features.rs, relevance_calibration.rs, calibration/ | ?? | ZDR and relevance work. Needed for retention_class in the usage frame. |
| L-only (??): mp-contracts/src/retention.rs, zdr_refusal.rs | ?? | ZDR retention types. |
| L-only (??): session-core/migrations/0044_run_retention_receipt.sql, 0045_zdr_waived_runs_org_index.sql, migrations/revert/, src/retention_receipt.rs | ?? | ZDR receipts and indexes. |
| L-only (??): api/contracts/zdr-refusals.json (+ .sha256); SPA src/shared/api/contracts/zdr-refusals.json (+ .sha256) | ?? | ZDR refusal contract. |
| L-only (??): SPA features/chat/components/ChatWorkPeriodPicker.tsx | ?? | Period picker. No cost hits. Missing in W. |

Clean in L (no uncommitted change): all of cost-core, billing-core, org-core, budget.rs, org_quota.rs, pricing.rs, turn_timing.rs, inference-core provider/intent.rs, routing_policy.rs, config.rs, gateway.proto, both ownership matrices, gateway billing.rs, orgs/quotas.rs, cost.rs, dispatchers.rs, SPA cost-client.ts, billing-allowances.ts, org-quota-client.ts, ag-ui-client.ts, verevon-ui-events.ts, action-registry.ts, action-client.ts, insights-usage.ts, ChatPanels.tsx, ChatMessages.metrics.test.tsx, CostDashboardPage.tsx and its test, InsightsPage.tsx, BillingAllowancesSection.tsx.

Implication: the cost-core, billing-core, org-core and budget paths are unchanged in L. The shared frame, its emitters and the chat turn controller are not, so WS27 and the ZDR workstream touch the same lines.

## 8. NOT DETERMINED

1. Card plan "I16" (card 760): not found in `C:/dev/verevon-audit-2026-10-14/final/` or `cons/`. Location unknown.
2. Source of `latestUsage()` in ChatPanels.tsx:915-927 (possibly the usage.recorded projection in ag-ui-client.ts:469-476). Not traced.
3. Data source of gateway handlers `summary` and `pricing` (domains/cost.rs:37, :42). cost-core's route table (server.go:140-149) has no /summary or /cost/pricing.
4. Whether ZDR suppresses ledger envelopes (card 760, "zdr ledger envelopes are suppressed"). Not located. The grep for suppression in sse.rs found only unrelated hits.
5. Whether any Go suite for cost-core runs in CI (no workflow mentions cost-core). Whether org-core tests run in org-service.yml (not read).
6. Whether the Do loop writes usage to cost-core. execution-core has no cost client found beyond a grep hit in llm_planner.rs (not opened). The card's Do-run scope is not confirmed.
7. Whether every ChatEvent::Usage construction emits once per turn: only the streaming site (L:4242) was read. Sites L:6983, L:8525, L:10077 were not opened.
8. Mapping of the card's "Work panel" to a component. ChatPanels.tsx holds the usage summary and "Prompt cache"; ChatLiveRunPanel.tsx was not checked for cost hits. AgentRunConsole.tsx is the confirmed Run Console cost field.
9. Whether any SPA UI calls the org quota PUT route (gateway orgs.rs:52). Not found in this sweep.
10. Per-user allowance: billing-core routes are org-scoped only (server.go:62-69). No per-user allowance route or table was found, so the card's per-user need stands.
11. Whether `constrained_fraction` is configurable per org (the action registry takes budget_cap_usd, action-registry.ts:1200). Not traced end to end.
12. Not run: all test commands above. Test pass or fail state for WS27 is not established.
