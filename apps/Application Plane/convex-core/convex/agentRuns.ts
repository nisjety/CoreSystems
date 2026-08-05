/**
 * U3-3 (ui-ux-verevon-gap.md §10) — Agent run lifecycle mirror.
 *
 * Functions:
 *   - `upsertAgentRun` (internalMutation) — called from `nats-subscriber.js`
 *     when a `mp.v1.run.{runId}.event` envelope arrives. Idempotent on
 *     `runId`; later events overwrite earlier ones (RUN_COMPLETED wins
 *     over RUN_STARTED on the same run).
 *   - `listForOrg` (query) — reactive subscription for verevon's
 *     `useQuery(api.agentRuns.listForOrg, { externalOrgId, limit })`.
 *     Returns runs newest-first, capped at the supplied limit.
 *   - `getByRunId` (query) — single-run lookup for drilldowns.
 *
 * The mutation accepts `payload` as `v.any()` so we don't have to evolve
 * the schema each time orchestrator-core adds a field. The mirror is a
 * projection, not the source of truth — orchestrator-core's events
 * remain authoritative.
 */

import { v } from "convex/values";
import { internalMutation, mutation, query } from "./_generated/server";
import { requireViewerMembership } from "./authz";

const statusValidator = v.union(
  v.literal("started"),
  v.literal("completed"),
  v.literal("failed"),
  v.literal("cancelled"),
);

export const upsertAgentRun = internalMutation({
  args: {
    runId: v.string(),
    externalOrgId: v.string(),
    externalUserId: v.optional(v.string()),
    agentId: v.optional(v.string()),
    status: statusValidator,
    error: v.optional(v.string()),
    payload: v.optional(v.any()),
    eventTimestampMs: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    const now = Date.now();
    const eventTs = args.eventTimestampMs ?? now;

    const existing = await ctx.db
      .query("agentRuns")
      .withIndex("by_run", (q) => q.eq("runId", args.runId))
      .unique();

    const completedAt =
      args.status === "completed" ||
      args.status === "failed" ||
      args.status === "cancelled"
        ? eventTs
        : undefined;

    if (existing) {
      // Latest-write-wins ordering: only allow the status to advance —
      // never overwrite a terminal state with `started` because NATS
      // doesn't guarantee delivery order across subjects.
      const terminal = ["completed", "failed", "cancelled"];
      if (
        terminal.includes(existing.status) &&
        !terminal.includes(args.status)
      ) {
        return existing._id;
      }

      await ctx.db.patch(existing._id, {
        status: args.status,
        error: args.error ?? existing.error,
        agentId: args.agentId ?? existing.agentId,
        externalUserId: args.externalUserId ?? existing.externalUserId,
        payload: args.payload ?? existing.payload,
        completedAt: completedAt ?? existing.completedAt,
        updatedAt: now,
      });
      return existing._id;
    }

    return await ctx.db.insert("agentRuns", {
      runId: args.runId,
      externalOrgId: args.externalOrgId,
      externalUserId: args.externalUserId,
      agentId: args.agentId,
      status: args.status,
      error: args.error,
      payload: args.payload,
      startedAt: eventTs,
      completedAt,
      updatedAt: now,
    });
  },
});

export const listForOrg = query({
  args: {
    externalOrgId: v.string(),
    limit: v.optional(v.number()),
    status: v.optional(statusValidator),
  },
  handler: async (ctx, args) => {
    await requireViewerMembership(ctx, args.externalOrgId);
    const limit = Math.max(1, Math.min(args.limit ?? 50, 200));

    if (args.status) {
      return await ctx.db
        .query("agentRuns")
        .withIndex("by_org_and_status", (q) =>
          q.eq("externalOrgId", args.externalOrgId).eq("status", args.status!),
        )
        .order("desc")
        .take(limit);
    }

    return await ctx.db
      .query("agentRuns")
      .withIndex("by_org_and_started", (q) =>
        q.eq("externalOrgId", args.externalOrgId),
      )
      .order("desc")
      .take(limit);
  },
});

export const getByRunId = query({
  args: { runId: v.string() },
  handler: async (ctx, args) => {
    const run = await ctx.db
      .query("agentRuns")
      .withIndex("by_run", (q) => q.eq("runId", args.runId))
      .unique();
    if (!run) return null;
    await requireViewerMembership(ctx, run.externalOrgId);
    return run;
  },
});

/**
 * Wave 11 §5 — Fin G/A/P feedback. Sets the rating on an existing run
 * row from the playground reply chips. Idempotent: re-rating overwrites
 * the previous value with the new `ratedAt` timestamp.
 *
 * Permission model: caller must belong to the same org as the run.
 * The HTTP route in verevon (`/api/agents/runs/[runId]/rate`) validates
 * `actor.orgId === run.externalOrgId` before invoking this mutation.
 */
const ratingValidator = v.union(
  v.literal("good"),
  v.literal("acceptable"),
  v.literal("poor"),
);

export const rate = mutation({
  args: {
    runId: v.string(),
    externalOrgId: v.string(),
    rating: ratingValidator,
    note: v.optional(v.string()),
  },
  handler: async (ctx, args) => {
    const existing = await ctx.db
      .query("agentRuns")
      .withIndex("by_run", (q) => q.eq("runId", args.runId))
      .unique();
    if (!existing) {
      throw new Error("Run not found");
    }
    await requireViewerMembership(ctx, existing.externalOrgId);
    if (existing.externalOrgId !== args.externalOrgId) {
      throw new Error("Run belongs to a different organization");
    }
    const now = Date.now();
    await ctx.db.patch(existing._id, {
      rating: args.rating,
      ratingNote: args.note,
      ratedAt: now,
      updatedAt: now,
    });
    return await ctx.db.get(existing._id);
  },
});

/**
 * U3-9 (ui-ux-verevon-gap.md §14): aggregate run stats for a single agent.
 *
 * Returns honest, computed metrics from the agentRuns mirror — replaces the
 * hardcoded "1,248 conversations / 76.2% deflection / 1m 12s avg / 23.8%
 * fallback" cards that used to render for every agent in
 * `AgentWorkspaceView` analytics tab.
 *
 * Counts:
 *   - `total` — every row for the agent
 *   - `started` / `completed` / `failed` / `cancelled` — by status
 *   - `successRate` — completed / (completed + failed), 0 when no terminal runs
 *   - `avgDurationMs` — mean over completed runs that have completedAt
 *   - `recent` — 10 most recent rows (for a live activity feed in the tab)
 *
 * The window is the last `lookbackDays` days (default 30). Bounded scan via
 * the `by_org_and_started` index; falls back to a full scan filtered on
 * agentId when the index isn't selective enough.
 */
export const statsByAgent = query({
  args: {
    externalOrgId: v.string(),
    agentId: v.string(),
    lookbackDays: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    await requireViewerMembership(ctx, args.externalOrgId);
    const lookbackDays = Math.max(1, Math.min(args.lookbackDays ?? 30, 365));
    const sinceMs = Date.now() - lookbackDays * 24 * 60 * 60 * 1000;

    // Pull recent runs for the org, then filter by agentId in memory. We
    // don't have a composite index on (org, agent) and adding one would be
    // overkill for this read path — orgs typically have <1k agents and
    // <100 runs/day per agent, so the scan is bounded.
    const all = await ctx.db
      .query("agentRuns")
      .withIndex("by_org_and_started", (q) =>
        q.eq("externalOrgId", args.externalOrgId).gte("startedAt", sinceMs),
      )
      .order("desc")
      .take(1000);

    const rows = all.filter((r) => r.agentId === args.agentId);

    let started = 0;
    let completed = 0;
    let failed = 0;
    let cancelled = 0;
    let totalDuration = 0;
    let durationCount = 0;
    for (const r of rows) {
      if (r.status === "started") started += 1;
      else if (r.status === "completed") completed += 1;
      else if (r.status === "failed") failed += 1;
      else if (r.status === "cancelled") cancelled += 1;

      if (r.status === "completed" && r.completedAt) {
        totalDuration += r.completedAt - r.startedAt;
        durationCount += 1;
      }
    }

    const terminal = completed + failed;
    const successRate = terminal > 0 ? completed / terminal : 0;
    const avgDurationMs =
      durationCount > 0 ? Math.round(totalDuration / durationCount) : 0;

    return {
      total: rows.length,
      started,
      completed,
      failed,
      cancelled,
      successRate,
      avgDurationMs,
      lookbackDays,
      recent: rows.slice(0, 10).map((r) => ({
        runId: r.runId,
        status: r.status,
        startedAt: r.startedAt,
        completedAt: r.completedAt ?? null,
        error: r.error ?? null,
      })),
    };
  },
});
