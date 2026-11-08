/**
 * Room pins (WS13-S8): the threads a room's editors have pinned for everyone.
 *
 * # Application owns this, deliberately
 *
 * "What this room has pinned" is a workspace projection, so it lives here. Session Core's own
 * `pinned_at` is a different fact (the post author's personal pin, owner-only) and is untouched.
 * A pin stores identifiers and a timestamp only: never a title and never content, so it says
 * nothing about what a post contains and a Zero Data Retention organization has nothing to
 * withhold.
 *
 * # Who checks what
 *
 * The calling gateway checks the Space lifecycle, the caller's live Control role (editor or
 * above) and that the thread belongs to the room (through a Control read decision) before it
 * calls a mutation here. This plane verifies organization membership and the service key, bounds
 * the input and the number of pins per room, and records who pinned. Reading a pinned post still
 * goes through Session Core under the viewer's own decision: a pin grants nothing.
 */
import { v } from "convex/values";
import { mutation, query } from "./_generated/server";
import { assertServiceKey, requireGatewayMember } from "./authz";

/** Most pins one room carries. A pin is a highlight; past this it stops being one. */
export const MAX_PINS_PER_ROOM = 50;

const MAX_ID_CHARS = 200;

export interface SpacePinRow {
  readonly threadId: string;
  readonly pinnedByExternalAuthId: string;
  readonly pinnedAt: number;
}

/** Newest pin first, ties broken by thread id so the order is stable across reads. */
export function orderPins<T extends { pinnedAt: number; threadId: string }>(pins: readonly T[]): T[] {
  return [...pins].sort((a, b) =>
    b.pinnedAt !== a.pinnedAt ? b.pinnedAt - a.pinnedAt : a.threadId.localeCompare(b.threadId),
  );
}

/** A trimmed identifier, or throws a stable `pin_invalid:<field>` message the gateway can map. */
export function requireId(value: string, field: string): string {
  const trimmed = (value ?? "").trim();
  if (!trimmed || trimmed.length > MAX_ID_CHARS || /[\u0000-\u001f\u007f]/.test(trimmed)) {
    throw new Error(`pin_invalid:${field}`);
  }
  return trimmed;
}

async function loadPin(ctx: any, spaceRef: string, threadId: string) {
  return ctx.db
    .query("spacePins")
    .withIndex("by_space_and_thread", (q: any) => q.eq("spaceRef", spaceRef).eq("threadId", threadId))
    .unique();
}

/** The room's pins, newest first. Rows from another organization are never returned. */
export const listForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
  },
  handler: async (ctx, args): Promise<{ pins: SpacePinRow[] }> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = requireId(args.spaceRef, "space_ref");
    const rows = await ctx.db
      .query("spacePins")
      .withIndex("by_space", (q: any) => q.eq("spaceRef", spaceRef))
      .order("desc")
      .take(MAX_PINS_PER_ROOM + 1);
    const pins = rows
      .filter((row: any) => row.externalOrgId === args.externalOrgId)
      .slice(0, MAX_PINS_PER_ROOM)
      .map((row: any) => ({
        threadId: row.threadId as string,
        pinnedByExternalAuthId: row.pinnedByExternalAuthId as string,
        pinnedAt: row.pinnedAt as number,
      }));
    return { pins: orderPins(pins) };
  },
});

/**
 * Pin a thread for the room. Idempotent: pinning an already pinned thread changes nothing and
 * keeps the original pinner and time. Throws `room_pins_limit` at {@link MAX_PINS_PER_ROOM}.
 */
export const setForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    threadId: v.string(),
  },
  handler: async (ctx, args): Promise<{ threadId: string; pinnedAt: number; changed: boolean }> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = requireId(args.spaceRef, "space_ref");
    const threadId = requireId(args.threadId, "thread_id");
    const existing = await loadPin(ctx, spaceRef, threadId);
    if (existing) {
      if (existing.externalOrgId !== args.externalOrgId) throw new Error("Unauthorized");
      return { threadId, pinnedAt: existing.pinnedAt, changed: false };
    }
    const current = await ctx.db
      .query("spacePins")
      .withIndex("by_space", (q: any) => q.eq("spaceRef", spaceRef))
      .take(MAX_PINS_PER_ROOM);
    if (current.length >= MAX_PINS_PER_ROOM) throw new Error("room_pins_limit");
    const pinnedAt = Date.now();
    await ctx.db.insert("spacePins", {
      spaceRef,
      externalOrgId: args.externalOrgId,
      threadId,
      pinnedByExternalAuthId: args.externalAuthId,
      pinnedAt,
    });
    return { threadId, pinnedAt, changed: true };
  },
});

/** Unpin a thread. Idempotent: an absent pin is not an error. */
export const clearForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    threadId: v.string(),
  },
  handler: async (ctx, args): Promise<{ threadId: string; changed: boolean }> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = requireId(args.spaceRef, "space_ref");
    const threadId = requireId(args.threadId, "thread_id");
    const existing = await loadPin(ctx, spaceRef, threadId);
    if (!existing || existing.externalOrgId !== args.externalOrgId) return { threadId, changed: false };
    await ctx.db.delete(existing._id);
    return { threadId, changed: true };
  },
});
