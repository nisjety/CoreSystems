/**
 * Org-scoped member status and agent availability chips (WS11-S10).
 *
 * # Authority
 *
 * Every function starts with `assertServiceKey` (the caller is the gateway) and
 * then `requireGatewayMember` (the caller belongs to the organization). Writes
 * touch only the caller's own row. A status is organization-scoped: every read
 * and write is keyed by (organization, subject), so text written in one
 * organization is never visible in another. Nothing here is an authorization
 * input and nothing widens access to a Space: the gateway has already proved
 * membership of the room it asks about, and the agent activity read returns
 * closed words for ids the gateway passed in.
 *
 * # Content and ZDR
 *
 * The status text is the one free-text field in the attention tables. The
 * gateway refuses a text write for a ZDR organization (typed 412) and passes
 * `includeText: false` on every read for such an organization; this module
 * then returns state and expiry only. The text is never copied into events,
 * notifications or prompts.
 *
 * # Expiry
 *
 * Evaluated at read time (an expired row reads as cleared) and swept hourly by
 * `sweepExpired`.
 *
 * # Errors
 *
 * Plain `Error` messages that are stable codes the gateway maps to its typed
 * envelope: `invalid_status` (all rejected status input), `invalid_space_ref`,
 * `too_many_subjects`, `too_many_agents`.
 */
import { v } from "convex/values";
import { internal } from "./_generated/api";
import { internalMutation, mutation, query } from "./_generated/server";
import { assertServiceKey, requireGatewayMember } from "./authz";
import {
  MAX_ACTIVITY_LOOKUP_AGENTS,
  MAX_STATUS_LOOKUP_SUBJECTS,
  agentChipState,
  checkStatusInput,
  statusView,
} from "./spaceMemberStatusRules";

const MAX_REF_CHARS = 200;
const SWEEP_BATCH = 200;

function cleanId(value: string, code: string): string {
  const id = (value ?? "").trim();
  if (!id || id.length > MAX_REF_CHARS) throw new Error(code);
  return id;
}

async function ownRow(ctx: any, externalOrgId: string, externalAuthId: string) {
  return await ctx.db
    .query("spaceMemberStatus")
    .withIndex("by_org_and_subject", (q: any) =>
      q.eq("externalOrgId", externalOrgId).eq("externalAuthId", externalAuthId),
    )
    .first();
}

/** The caller's own status in this organization, or null when none or expired. */
export const getOwnStatusForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    includeText: v.boolean(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const row = await ownRow(ctx, args.externalOrgId, args.externalAuthId);
    return statusView(row, Date.now(), args.includeText);
  },
});

/**
 * Set the caller's own status. `text` and `expiresAt` are optional; absent
 * means no text and no expiry. Replaces the previous value entirely.
 */
export const setStatusForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    state: v.string(),
    text: v.optional(v.union(v.string(), v.null())),
    expiresAt: v.optional(v.union(v.number(), v.null())),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const now = Date.now();
    const checked = checkStatusInput(
      { state: args.state, text: args.text, expiresAt: args.expiresAt },
      now,
    );
    if (!checked.ok) throw new Error("invalid_status");

    const patch = {
      state: checked.state,
      text: checked.text ?? undefined,
      expiresAt: checked.expiresAt ?? undefined,
      updatedAt: now,
    };
    const row = await ownRow(ctx, args.externalOrgId, args.externalAuthId);
    if (row) {
      // `replace` drops the optional fields the new value leaves out.
      await ctx.db.replace(row._id, {
        externalOrgId: args.externalOrgId,
        externalAuthId: args.externalAuthId,
        ...patch,
      });
    } else {
      await ctx.db.insert("spaceMemberStatus", {
        externalOrgId: args.externalOrgId,
        externalAuthId: args.externalAuthId,
        ...patch,
      });
    }
    return statusView(
      { state: checked.state, text: checked.text, expiresAt: checked.expiresAt, updatedAt: now },
      now,
      true,
    );
  },
});

/** Clear the caller's own status. Idempotent. */
export const clearStatusForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const row = await ownRow(ctx, args.externalOrgId, args.externalAuthId);
    if (row) await ctx.db.delete(row._id);
    return { cleared: !!row };
  },
});

/**
 * Statuses of other members of the same organization, for the roster. Returns
 * one entry per requested subject that has an unexpired row in THIS
 * organization; a subject with no row, an expired row or a row in another
 * organization is simply absent.
 */
export const statusesForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    subjectIds: v.array(v.string()),
    includeText: v.boolean(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const subjects: string[] = Array.from(
      new Set<string>(args.subjectIds.map((id: string) => cleanId(id, "invalid_status"))),
    );
    if (subjects.length > MAX_STATUS_LOOKUP_SUBJECTS) throw new Error("too_many_subjects");
    const now = Date.now();
    const out: Array<{
      externalAuthId: string;
      state: string;
      text: string | null;
      expiresAt: number | null;
    }> = [];
    for (const subject of subjects) {
      const row = await ownRow(ctx, args.externalOrgId, subject);
      const view = statusView(row, now, args.includeText);
      if (view) {
        out.push({
          externalAuthId: subject,
          state: view.state,
          text: view.text,
          expiresAt: view.expiresAt,
        });
      }
    }
    return out;
  },
});

/**
 * Agent availability for the chips in a room's agent list. Returns one entry
 * per requested agent id: `working`, `waiting` or `idle` (no row, a stale row
 * older than 20 minutes, or an unknown word all read as idle). Ids only; no
 * run id, title or text.
 */
export const agentActivityForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    agentIds: v.array(v.string()),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = cleanId(args.spaceRef, "invalid_space_ref");
    const agentIds: string[] = Array.from(
      new Set<string>(args.agentIds.map((id: string) => cleanId(id, "invalid_status"))),
    );
    if (agentIds.length > MAX_ACTIVITY_LOOKUP_AGENTS) throw new Error("too_many_agents");
    const now = Date.now();
    const out: Array<{ agentId: string; state: "working" | "waiting" | "idle"; updatedAt: number | null }> = [];
    for (const agentId of agentIds) {
      const row = await ctx.db
        .query("spaceAgentActivity")
        .withIndex("by_space_and_agent", (q: any) => q.eq("spaceRef", spaceRef).eq("agentId", agentId))
        .first();
      const own = row && row.externalOrgId === args.externalOrgId ? row : null;
      out.push({ agentId, state: agentChipState(own, now), updatedAt: own ? own.updatedAt : null });
    }
    return out;
  },
});

/**
 * Hourly housekeeping: delete status rows past their expiry. Deletes only;
 * safe to run repeatedly. A full batch schedules the next one.
 */
export const sweepExpired = internalMutation({
  args: {},
  handler: async (ctx): Promise<{ expired: number }> => {
    const now = Date.now();
    // `gt(0)` skips rows with no expiry (undefined sorts before numbers).
    const expired = await ctx.db
      .query("spaceMemberStatus")
      .withIndex("by_expiry", (q: any) => q.gt("expiresAt", 0).lt("expiresAt", now))
      .take(SWEEP_BATCH);
    for (const row of expired) await ctx.db.delete(row._id);
    if (expired.length >= SWEEP_BATCH) {
      await ctx.scheduler.runAfter(0, internal.spaceMemberStatus.sweepExpired, {});
    }
    return { expired: expired.length };
  },
});
