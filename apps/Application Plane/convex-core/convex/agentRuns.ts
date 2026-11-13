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
 *   - `getByRunId` (query) — single-run lookup for drilldowns; returns
 *     `browserActivity` to the run owner only (WS10.S7a). An unknown run and
 *     a run the viewer cannot see both return `null`.
 *   - `upsertBrowserActivity` (internalMutation) — WS10.S7a browser trace
 *     projection; patches an existing row only (see its comment).
 *
 * The mutation accepts `payload` as `v.any()` so we don't have to evolve
 * the schema each time orchestrator-core adds a field. The mirror is a
 * projection, not the source of truth — orchestrator-core's events
 * remain authoritative.
 */

import { v } from "convex/values";
import { internalMutation, mutation, query } from "./_generated/server";
import { requireIdentity, requireViewerMembership } from "./authz";

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

export async function listForOrgHandler(ctx: any, args: { externalOrgId: string; limit?: number; status?: "started" | "completed" | "failed" | "cancelled" }) {
    const { externalAuthId } = await requireViewerMembership(ctx, args.externalOrgId);
    const limit = Math.max(1, Math.min(args.limit ?? 50, 200));

    if (args.status) {
      const runs = await ctx.db
        .query("agentRuns")
        .withIndex("by_org_and_status", (q: any) =>
          q.eq("externalOrgId", args.externalOrgId).eq("status", args.status!),
        )
        .order("desc")
        .take(limit);
      return runs.map((run: any) => projectRunForViewer(run, externalAuthId));
    }

    const runs = await ctx.db
      .query("agentRuns")
      .withIndex("by_org_and_started", (q: any) =>
        q.eq("externalOrgId", args.externalOrgId),
      )
      .order("desc")
      .take(limit);
    return runs.map((run: any) => projectRunForViewer(run, externalAuthId));
}

export const listForOrg = query({
  args: {
    externalOrgId: v.string(),
    limit: v.optional(v.number()),
    status: v.optional(statusValidator),
  },
  handler: listForOrgHandler,
});

/**
 * `requireViewerMembership` for a lookup that must not reveal whether an org
 * exists: a viewer who is not a member of the org, or an org that is missing
 * or deleted, gets `null`. Any other failure still throws.
 */
async function viewerMembershipOrNull(ctx: any, externalOrgId: string) {
  try {
    return await requireViewerMembership(ctx, externalOrgId);
  } catch (error) {
    const message = error instanceof Error ? error.message : "";
    if (message === "Unauthorized" || message === "Organization not found") {
      return null;
    }
    throw error;
  }
}

/**
 * Single-run lookup. Any member of the run's org gets the row; the
 * `browserActivity` projection is owner-only in v1 (WS10.S7a, ADR-8 part 2,
 * "projections only narrow"): it is returned only when the viewer is the
 * run's `externalUserId`.
 *
 * The caller is authenticated before the row is read. An unknown run and a
 * run in an org the viewer is not in both answer `null`, so the reply does
 * not reveal which run ids exist.
 */
export async function getByRunIdHandler(ctx: any, args: { runId: string }) {
  const { externalAuthId } = await requireIdentity(ctx);
  const run = await ctx.db
    .query("agentRuns")
    .withIndex("by_run", (q: any) => q.eq("runId", args.runId))
    .unique();
  if (!run) return null;
  if (!(await viewerMembershipOrNull(ctx, run.externalOrgId))) return null;
  return projectRunForViewer(run, externalAuthId);
}

export const getByRunId = query({
  args: { runId: v.string() },
  handler: getByRunIdHandler,
});

/**
 * WS10.S7a (ADR-8 part 2): browser activity projection on `agentRuns`.
 *
 * `upsertBrowserActivity` is called by the trace consumer (WS10.S7b, not
 * built here) with a debounced, reduced view of `mp.v1.trace.run.{runId}`
 * events. Rules:
 *   - patch an existing run row only: a trace event for an unknown run
 *     creates nothing (the run lifecycle mirror owns row creation);
 *   - the caller's `externalOrgId` (taken by the consumer from the verified
 *     subject mapping, never from the payload) must equal the row's;
 *   - `lastProducerSeq` is monotonic: a stale or duplicate seq is ignored;
 *   - labels are short identifiers and counters are non-negative integers,
 *     so no payload text (URLs, selectors, page content) can land here.
 * There is no flag: nothing calls this until the consumer and a producer
 * exist.
 */
export type BrowserActivity = {
  lastProducerSeq: number;
  lastEventType: string;
  lastEventAt: number;
  sessionState?: string;
  control?: string;
  blockedEgress?: number;
  steps?: number;
  artifacts?: number;
};

export type BrowserActivityUpdate = {
  runId: string;
  externalOrgId: string;
  producerSeq: number;
  eventType: string;
  eventAt: number;
  sessionState?: string;
  control?: string;
  blockedEgress?: number;
  steps?: number;
  artifacts?: number;
};

export type BrowserActivityDecision =
  | { kind: "apply"; browserActivity: BrowserActivity }
  | {
      kind: "skip";
      reason: "unknown_run" | "org_mismatch" | "stale_seq" | "invalid";
    };

/** A label (event type, session state, control holder), never free text. */
const BROWSER_ACTIVITY_LABEL = /^[A-Za-z0-9_.:-]{1,64}$/;

function isLabel(value: unknown): value is string {
  return typeof value === "string" && BROWSER_ACTIVITY_LABEL.test(value);
}

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

/**
 * The pure decision behind `upsertBrowserActivity`. Optional fields the
 * update omits keep their previous value (counters are approximate totals
 * from the consumer's reducer, not deltas).
 */
export function decideBrowserActivityUpdate(
  run: { externalOrgId: string; browserActivity?: BrowserActivity } | null,
  update: BrowserActivityUpdate,
): BrowserActivityDecision {
  if (!run) return { kind: "skip", reason: "unknown_run" };
  if (run.externalOrgId !== update.externalOrgId) {
    return { kind: "skip", reason: "org_mismatch" };
  }
  if (
    !Number.isSafeInteger(update.producerSeq) ||
    update.producerSeq < 1 ||
    !isLabel(update.eventType) ||
    typeof update.eventAt !== "number" ||
    !Number.isFinite(update.eventAt) ||
    (update.sessionState !== undefined && !isLabel(update.sessionState)) ||
    (update.control !== undefined && !isLabel(update.control)) ||
    (update.blockedEgress !== undefined && !isCount(update.blockedEgress)) ||
    (update.steps !== undefined && !isCount(update.steps)) ||
    (update.artifacts !== undefined && !isCount(update.artifacts))
  ) {
    return { kind: "skip", reason: "invalid" };
  }

  const previous = run.browserActivity;
  if (previous && update.producerSeq <= previous.lastProducerSeq) {
    return { kind: "skip", reason: "stale_seq" };
  }

  const next: BrowserActivity = {
    lastProducerSeq: update.producerSeq,
    lastEventType: update.eventType,
    lastEventAt: update.eventAt,
  };
  const sessionState = update.sessionState ?? previous?.sessionState;
  if (sessionState !== undefined) next.sessionState = sessionState;
  const control = update.control ?? previous?.control;
  if (control !== undefined) next.control = control;
  const blockedEgress = update.blockedEgress ?? previous?.blockedEgress;
  if (blockedEgress !== undefined) next.blockedEgress = blockedEgress;
  const steps = update.steps ?? previous?.steps;
  if (steps !== undefined) next.steps = steps;
  const artifacts = update.artifacts ?? previous?.artifacts;
  if (artifacts !== undefined) next.artifacts = artifacts;

  return { kind: "apply", browserActivity: next };
}

/**
 * Owner-only read (v1): the row without `browserActivity` unless the viewer
 * is the run's `externalUserId`. A run with no `externalUserId` shows the
 * projection to nobody.
 */
export function projectRunForViewer<T extends { externalUserId?: string; browserActivity?: unknown }>(
  run: T,
  viewerExternalAuthId: string,
): T {
  if (run.browserActivity === undefined) return run;
  const isOwner =
    typeof run.externalUserId === "string" &&
    run.externalUserId.length > 0 &&
    run.externalUserId === viewerExternalAuthId;
  if (isOwner) return run;
  const narrowed = { ...run };
  delete narrowed.browserActivity;
  return narrowed;
}

export async function upsertBrowserActivityHandler(
  ctx: any,
  args: BrowserActivityUpdate,
): Promise<{ applied: boolean; reason?: string }> {
  const run = await ctx.db
    .query("agentRuns")
    .withIndex("by_run", (q: any) => q.eq("runId", args.runId))
    .unique();
  const decision = decideBrowserActivityUpdate(run, args);
  if (decision.kind === "skip") {
    return { applied: false, reason: decision.reason };
  }
  await ctx.db.patch(run._id, {
    browserActivity: decision.browserActivity,
    updatedAt: Date.now(),
  });
  return { applied: true };
}

export const upsertBrowserActivity = internalMutation({
  args: {
    runId: v.string(),
    externalOrgId: v.string(),
    producerSeq: v.number(),
    eventType: v.string(),
    eventAt: v.number(),
    sessionState: v.optional(v.string()),
    control: v.optional(v.string()),
    blockedEgress: v.optional(v.number()),
    steps: v.optional(v.number()),
    artifacts: v.optional(v.number()),
  },
  handler: upsertBrowserActivityHandler,
});

/**
 * Wave 11 §5 — Fin G/A/P feedback. Sets the rating on an existing run
 * row from the playground reply chips. Idempotent: re-rating overwrites
 * the previous value with the new `ratedAt` timestamp.
 *
 * Permission model: caller must belong to the same org as the run.
 * The HTTP route in verevon (`/api/agents/runs/[runId]/rate`) validates
 * `actor.orgId === run.externalOrgId` before invoking this mutation.
 * The caller is authenticated before the lookup. An unknown run and a run
 * in an org the caller is not in both throw `Run not found`.
 */
const ratingValidator = v.union(
  v.literal("good"),
  v.literal("acceptable"),
  v.literal("poor"),
);

export async function rateHandler(
  ctx: any,
  args: {
    runId: string;
    externalOrgId: string;
    rating: "good" | "acceptable" | "poor";
    note?: string;
  },
) {
  const { externalAuthId } = await requireIdentity(ctx);
  const existing = await ctx.db
    .query("agentRuns")
    .withIndex("by_run", (q: any) => q.eq("runId", args.runId))
    .unique();
  const viewer = existing
    ? await viewerMembershipOrNull(ctx, existing.externalOrgId)
    : null;
  // An unknown run and a run in an org the viewer is not in get the same refusal.
  if (!existing || !viewer) {
    throw new Error("Run not found");
  }
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
  // The reply is the run row, so it takes the owner-only projection that getByRunId and
  // listForOrg apply: browserActivity reaches the run's owner and nobody else.
  const row = await ctx.db.get(existing._id);
  return row ? projectRunForViewer(row, externalAuthId) : null;
}

export const rate = mutation({
  args: {
    runId: v.string(),
    externalOrgId: v.string(),
    rating: ratingValidator,
    note: v.optional(v.string()),
  },
  handler: rateHandler,
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
