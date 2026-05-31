/**
 * G35 — Convex projection of the Control Session aggregate (per ADR 0002).
 *
 * Writers: CP session-core's `ControlSessionService.Refresh` calls the
 * `upsertControlSession` HTTP action after re-aggregating user-core +
 * org-core + billing-core. The snapshot is stored opaquely so we don't have
 * to track every upstream shape change.
 *
 * Readers: velion's `(dashboard)/*` pages subscribe via
 * `useQuery(api.controlSessions.byUser, { externalUserId })` for reactive
 * plan / entitlement / billing UI — no more poll loops against the
 * `/api/v1/sessions/current` REST endpoint.
 *
 * Auth: writes go through the HTTP action gated by `X-Service-Key` (same
 * pattern as `convex/ingest.ts`); reads are scoped by `externalUserId`
 * which Better Auth's JWT already binds, so a malicious client can only
 * subscribe to their own row.
 */

import { v } from "convex/values";

import { httpAction, mutation, query } from "./_generated/server";
import { api } from "./_generated/api";

function getServiceKey(): string {
  return (
    process.env.CONVEX_INTERNAL_SERVICE_KEY ||
    process.env.INTERNAL_API_KEY ||
    "change-me-internal-service-secret"
  );
}

function assertIngestKey(request: Request): void {
  const provided = request.headers.get("X-Service-Key");
  if (!provided || provided !== getServiceKey()) {
    throw new Error("Unauthorized");
  }
}

function jsonOk(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

function jsonErr(message: string, status: number): Response {
  return new Response(JSON.stringify({ error: message }), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

/**
 * upsertControlSessionInternal — internal mutation called by the HTTP
 * action. Splits the write from the auth/parsing layer so the mutation
 * stays trivially unit-testable.
 */
export const upsertControlSessionInternal = mutation({
  args: {
    externalUserId: v.string(),
    externalOrgId: v.optional(v.string()),
    snapshot: v.any(),
    fetchedAt: v.number(),
  },
  handler: async (ctx, args) => {
    const now = Date.now();
    const existing = await ctx.db
      .query("controlSessions")
      .withIndex("by_external_user_and_org", (q) =>
        q
          .eq("externalUserId", args.externalUserId)
          .eq("externalOrgId", args.externalOrgId),
      )
      .first();

    if (existing) {
      await ctx.db.patch(existing._id, {
        snapshot: args.snapshot,
        fetchedAt: args.fetchedAt,
        updatedAt: now,
      });
      return existing._id;
    }

    return await ctx.db.insert("controlSessions", {
      externalUserId: args.externalUserId,
      externalOrgId: args.externalOrgId,
      snapshot: args.snapshot,
      fetchedAt: args.fetchedAt,
      createdAt: now,
      updatedAt: now,
    });
  },
});

/**
 * POST /ingest/control-session
 *
 * session-core calls this after re-aggregating. Body:
 *   external_user_id:  string   — auth-core user id
 *   external_org_id?:  string   — active org id (omit for no-org sessions)
 *   snapshot:          object   — full ControlSession JSON
 *   fetched_at?:       number   — ms-epoch, defaults to Date.now()
 */
export async function upsertControlSessionHandler(ctx: any, request: Request) {
  try {
    assertIngestKey(request);
  } catch {
    return jsonErr("Unauthorized", 401);
  }

  let payload: {
    external_user_id: string;
    external_org_id?: string;
    snapshot: unknown;
    fetched_at?: number;
  };
  try {
    payload = await request.json();
  } catch {
    return jsonErr("Invalid JSON", 400);
  }

  if (!payload.external_user_id || typeof payload.external_user_id !== "string") {
    return jsonErr("external_user_id required", 400);
  }
  if (payload.snapshot === undefined || payload.snapshot === null) {
    return jsonErr("snapshot required", 400);
  }

  const id = await ctx.runMutation(api.controlSessions.upsertControlSessionInternal, {
    externalUserId: payload.external_user_id,
    externalOrgId: payload.external_org_id,
    snapshot: payload.snapshot,
    fetchedAt: payload.fetched_at ?? Date.now(),
  });

  return jsonOk({ id });
}

export const upsertControlSession = httpAction(upsertControlSessionHandler);

/**
 * byUser — velion's reactive subscription target. Returns the most recent
 * Control Session snapshot for the given external user id (across orgs).
 *
 * Useful when velion wants to show "you're on Pro" without caring which
 * org the user is currently scoped to. For org-aware views, prefer
 * `byUserAndOrg`.
 */
export const byUser = query({
  args: { externalUserId: v.string() },
  handler: async (ctx, { externalUserId }) => {
    return await ctx.db
      .query("controlSessions")
      .withIndex("by_external_user", (q) => q.eq("externalUserId", externalUserId))
      .order("desc")
      .first();
  },
});

/**
 * byUserAndOrg — scoped subscription for a specific (user, org) pair.
 */
export const byUserAndOrg = query({
  args: {
    externalUserId: v.string(),
    externalOrgId: v.optional(v.string()),
  },
  handler: async (ctx, { externalUserId, externalOrgId }) => {
    return await ctx.db
      .query("controlSessions")
      .withIndex("by_external_user_and_org", (q) =>
        q.eq("externalUserId", externalUserId).eq("externalOrgId", externalOrgId),
      )
      .first();
  },
});
