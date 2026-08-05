# Frontend Plane (verevonv3) — Roadmap

> **2026-07-13 correction:** do not implement historical Phase B item 1 as an
> unconditional default-tool policy. Keep plain chat cheap/tool-free and give
> users an intentional Actions/Plan choice backed by one authoritative Model
> Plane capability-state contract. Approval-required actions must select the
> governed agentic path; unavailable/unhealthy/not-configured actions must be
> visible but non-runnable with a machine-readable reason. Exact audience
> issuance/forwarding is source-fixed; the remaining UX/contract matrix is in
> [MODEL_PLANE_CAPABILITY_HANDOFF_2026-07-13.md](docs/MODEL_PLANE_CAPABILITY_HANDOFF_2026-07-13.md).

Derived from `docs/core-research/plane-audit-2026-07-11.md`. Read `FRONTEND_PLANE_STATUS.md` first. verevonv3 SPA changes hot-reload (Vite, bind-mounted source) so SPA fixes go live without a Docker rebuild; gateway (Rust) fixes need a rebuild (currently blocked by the Docker maintenance gate).

## Phase A — Security (the new IDORs)

1. **Fix the `onboarding/graph-preview` cross-tenant IDOR (HIGH)** — `apps/gateway/src/onboarding/lookup/graph.rs` must derive org from the verified session (`authorized_org_id(&user)`), exactly like its sibling `translate_recommendation`, and ignore the client `org_id` query param. Today any authenticated user reads any org's knowledge graph via `?orgId=<victim>`. Add a regression test mirroring the `x-verevon-org-id` strip tests.
2. **Gate onboarding connector actions on membership (MEDIUM)** — `connectors.rs` `start_connect_session`/`discover_source`/cleanup should verify the body `org_id` equals the caller's authorized org (as the billing path does via `checkout_lifecycle_ready`), not trust it blindly. Confirm whether integration-corev2 independently enforces actor-vs-org; if not, this is a live cross-tenant connect/enumerate.
3. Remove `x-verevon-org-id` from the CORS allow-headers list (defense-in-depth; it's stripped at ingress anyway, but advertising it invites a future handler to read it).

## Historical Phase B — superseded by the 2026-07-13 handoff

1. **Default-advertise low-risk tools on every chat turn** — in `src/shared/api/chat-client.ts` `buildChatWireBody`/`buildToolSpecs`, merge the registry's `requiresApproval:false` tools (brreg lookup, knowledge scrape/crawl/import, operating-map generate/refresh, etc.) into the emitted tools so plain reads/lookups work on the cheap direct loop. Small fixed prompt-token cost, no latency/HITL cost.
2. **Close the HITL-bypass in the SAME edit** — force `features.add('agentic')` whenever any emitted tool is `requiresApproval` (`if (request.planMode || emitsRiskyTool) features.add('agentic')`), so a composer-selected write can never run un-gated on the direct loop.
3. **Optional UX**: surface an explicit "let the assistant use tools" / Actions toggle on the composer for discoverability (Plan-mode chip already exists for the agentic path).
4. **Prerequisite for chat-shipping** (your literal example): add shipping actions to `src/shared/actions/action-registry.ts` (proper zod I/O, risk, `requiresApproval`) AND ensure model-gateway's tool executor routes them to shipping-core. Only then does a chat question like "shipping time Oslo→Trondheim" invoke `get_shipping_quotes`. (Cross-ref Model Plane roadmap.)

## Phase C — Residual de-fake + honesty

1. Empty/replace the two hardcoded arrays in `src/features/settings/components/WorkspaceSettingsPage.tsx` (`businessHourRows`, `roleRows` fake member counts) — same treatment the Phase-4 de-fake sweep applied to the adjacent status grids, or wire them to the real member/schedule data (MembersSection already loads real members right below).
2. (Optional) The agents blueprint surfaces are honestly labeled `DesignPreviewBadge` — no change needed unless/until they're wired to live per-org agent config.

## Phase D — MCP connectors "like Claude Code" (ties to Model Plane)

1. ✅ Done: the MCP-add form now validates transport/URL scheme (no more silent stdio+HTTPS dead records).
2. Surface **discovery/health feedback** on the MCP settings row — today registration only stores; a server that fails discovery shows as "aktiv" with zero tools. Needs a model-gateway "test/discover on register" signal (Model Plane work) surfaced in `McpServersSection.tsx`.
3. For connector-style remote servers like Visma Net (OAuth Streamable-HTTP), the real blocker is Model Plane lacking an OAuth remote-MCP client — see Model Plane roadmap. The verevonv3 form is ready once that backend support exists.

## Phase E — Docs

1. Delete `docs/core-research/mock-backed-surfaces.md` (obsolete; on the register). Repoint any inbound links.
2. The other core-research docs were corrected in place this pass; keep them as the current source of truth over the older top-level docs.

## Sequencing note

Phase A#1 (graph-preview IDOR) is the highest priority — a live cross-tenant data read — but needs a gateway rebuild to deploy (Docker gate). Phase B (chat tools) and Phase C (de-fake) are SPA-only and hot-reload immediately once applied. Phase D#2/#3 depend on Model Plane work.
