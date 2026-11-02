/**
 * The service-key API behind Space attention (WS11-S3): per-room counts for the
 * sidebar badge, the inbox rows, notification levels, thread follow and the
 * invite signal.
 *
 * # Authority
 *
 * Every function starts with `assertServiceKey` (the caller is the gateway) and
 * then `requireGatewayMember` (the subject really belongs to the organization).
 * Every function touches only the CALLER's own rows: the recipient of an item,
 * the owner of a preference, the author of a follow. Nothing here widens access
 * to a Space: the gateway has already proved membership of each ref it passes
 * (Control's index and read decisions) and nothing in these rows is read by any
 * authorization decision. A row for another member answers exactly like a row
 * that does not exist (`attention_item_not_found`).
 *
 * # Content
 *
 * Rows hold ids, closed words and timestamps. There is no title, text or name
 * to leak; the inbox page re-reads titles from Model Plane under the viewer's
 * current decision.
 *
 * # Errors
 *
 * Plain `Error` messages that are stable codes the gateway maps to its typed
 * envelope: `invalid_space_ref`, `too_many_refs`, `invalid_level`,
 * `invalid_follow_state`, `invalid_snooze`, `invalid_item_state`,
 * `invalid_kind`, `attention_item_not_found`, `space_not_found`.
 */
import { v } from "convex/values";
import { mutation, query } from "./_generated/server";
import { assertServiceKey, requireGatewayMember } from "./authz";
import {
  DEFAULT_ROOM_LEVEL,
  MAX_ATTENTION_REFS,
  MAX_ITEMS_PER_RECIPIENT,
  coalesceKey,
  checkSnooze,
  deliveryDecision,
  effectiveItemState,
  envFlagOn,
  isAttentionKind,
  isRoomLevel,
  itemExpiry,
  type AttentionKind,
  type RoomLevel,
} from "./spaceAttentionRules";
import { effectiveMarker } from "./spaceReadMarkers";

const MAX_REF_CHARS = 200;
const MAX_LIST_LIMIT = 100;
const DEFAULT_LIST_LIMIT = 50;
const READ_DRIVEN_KINDS: readonly AttentionKind[] = ["mention", "reply"];

function cleanRef(value: string): string {
  const ref = (value ?? "").trim();
  if (!ref || ref.length > MAX_REF_CHARS) throw new Error("invalid_space_ref");
  return ref;
}

function cleanRefs(values: readonly string[]): string[] {
  const out = Array.from(new Set(values.map(cleanRef)));
  if (out.length > MAX_ATTENTION_REFS) throw new Error("too_many_refs");
  return out;
}

function cleanId(value: string, code: string): string {
  const id = (value ?? "").trim();
  if (!id || id.length > MAX_REF_CHARS) throw new Error(code);
  return id;
}

function openLikeStates(): readonly ("open" | "snoozed")[] {
  return ["open", "snoozed"];
}

async function ownItems(
  ctx: any,
  externalAuthId: string,
  externalOrgId: string,
  state: "open" | "snoozed" | "done",
): Promise<any[]> {
  const rows = await ctx.db
    .query("spaceAttentionItems")
    .withIndex("by_recipient_state_updated", (q: any) =>
      q.eq("recipientAuthId", externalAuthId).eq("state", state),
    )
    .order("desc")
    .take(MAX_ITEMS_PER_RECIPIENT);
  return rows.filter((row: any) => row.externalOrgId === externalOrgId);
}

function itemView(row: any, now: number) {
  return {
    itemId: String(row._id),
    kind: row.kind,
    spaceRef: row.spaceRef,
    threadId: row.threadId ?? null,
    messageId: row.messageId ?? null,
    runId: row.runId ?? null,
    actorKind: row.actorKind,
    actorRef: row.actorRef ?? null,
    state: effectiveItemState(row, now),
    snoozedUntil: row.snoozedUntil ?? null,
    count: row.count,
    createdAt: row.createdAt,
    updatedAt: row.updatedAt,
  };
}

/**
 * Per-room attention state for the caller, for the refs the gateway passes (at
 * most 60). Returns the effective read marker, the room level and the open
 * item counts; the gateway adds Session Core's unread and needs-you and
 * computes the one badge. A ref with no rows answers with defaults and nulls.
 */
export const attentionForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRefs: v.array(v.string()),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const refs = cleanRefs(args.spaceRefs);
    const wanted = new Set(refs);
    const now = Date.now();

    const markers = await ctx.db
      .query("spaceReadMarkers")
      .withIndex("by_org_and_subject", (q: any) =>
        q.eq("externalOrgId", args.externalOrgId).eq("externalAuthId", args.externalAuthId),
      )
      .take(2000);
    const markerByRef = new Map<string, any>();
    for (const row of markers) if (wanted.has(row.spaceRef)) markerByRef.set(row.spaceRef, row);

    const prefs = await ctx.db
      .query("spaceRoomNotifyPrefs")
      .withIndex("by_subject", (q: any) =>
        q.eq("externalOrgId", args.externalOrgId).eq("externalAuthId", args.externalAuthId),
      )
      .take(2000);
    const levelByRef = new Map<string, RoomLevel>();
    for (const row of prefs) if (wanted.has(row.spaceRef)) levelByRef.set(row.spaceRef, row.level);

    const settings = await ctx.db
      .query("spaceNotifyUserSettings")
      .withIndex("by_subject", (q: any) =>
        q.eq("externalOrgId", args.externalOrgId).eq("externalAuthId", args.externalAuthId),
      )
      .first();

    const counts = new Map<string, Record<AttentionKind, number>>();
    let otherOpen = 0;
    for (const state of openLikeStates()) {
      for (const row of await ownItems(ctx, args.externalAuthId, args.externalOrgId, state)) {
        if (effectiveItemState(row, now) !== "open") continue;
        if (!wanted.has(row.spaceRef)) {
          otherOpen += 1;
          continue;
        }
        const entry =
          counts.get(row.spaceRef) ??
          ({ mention: 0, reply: 0, invite: 0, agent_done: 0, agent_failed: 0, reminder: 0 } as Record<
            AttentionKind,
            number
          >);
        entry[row.kind as AttentionKind] += 1;
        counts.set(row.spaceRef, entry);
      }
    }

    return {
      generatedAt: now,
      hidePreview: settings?.hidePreview ?? false,
      otherOpen,
      rooms: refs.map((spaceRef) => {
        const marker = markerByRef.get(spaceRef);
        const open = counts.get(spaceRef);
        return {
          spaceRef,
          lastReadAt: effectiveMarker(marker),
          readThroughAt: marker ? marker.lastReadAt : null,
          manualUnread:
            marker !== undefined &&
            typeof marker.unreadFloor === "number" &&
            Number.isFinite(marker.unreadFloor),
          level: levelByRef.get(spaceRef) ?? DEFAULT_ROOM_LEVEL,
          levelIsDefault: !levelByRef.has(spaceRef),
          mentions: open?.mention ?? 0,
          replies: open?.reply ?? 0,
          invites: open?.invite ?? 0,
          agentDone: open?.agent_done ?? 0,
          agentFailed: open?.agent_failed ?? 0,
          reminders: open?.reminder ?? 0,
        };
      }),
    };
  },
});

/**
 * The caller's inbox rows, newest first, one page at a time. `open` also lists
 * snoozed rows whose time has passed (first page only). Cross-room filtering
 * against Control's index is the gateway's job (it drops rows of Spaces the
 * caller can no longer see).
 */
export const listItemsForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    state: v.union(v.literal("open"), v.literal("snoozed"), v.literal("done")),
    kind: v.optional(v.string()),
    limit: v.optional(v.number()),
    cursor: v.optional(v.union(v.string(), v.null())),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    if (args.kind !== undefined && !isAttentionKind(args.kind)) throw new Error("invalid_kind");
    const kind = args.kind as AttentionKind | undefined;
    const limit = Math.min(
      MAX_LIST_LIMIT,
      Math.max(1, Math.floor(Number.isFinite(args.limit) ? (args.limit as number) : DEFAULT_LIST_LIMIT)),
    );
    const now = Date.now();

    const page = await ctx.db
      .query("spaceAttentionItems")
      .withIndex("by_recipient_state_updated", (q: any) =>
        q.eq("recipientAuthId", args.externalAuthId).eq("state", args.state),
      )
      .order("desc")
      .paginate({ numItems: limit, cursor: args.cursor ?? null });

    let rows: any[] = page.page.filter(
      (row: any) => row.externalOrgId === args.externalOrgId && (!kind || row.kind === kind),
    );
    if (args.state === "open" && !args.cursor) {
      const snoozed = await ownItems(ctx, args.externalAuthId, args.externalOrgId, "snoozed");
      const due = snoozed.filter(
        (row: any) => effectiveItemState(row, now) === "open" && (!kind || row.kind === kind),
      );
      rows = [...rows, ...due].sort((a: any, b: any) => b.updatedAt - a.updatedAt);
    }
    if (args.state === "snoozed") {
      rows = rows.filter((row: any) => effectiveItemState(row, now) === "snoozed");
    }

    const settings = await ctx.db
      .query("spaceNotifyUserSettings")
      .withIndex("by_subject", (q: any) =>
        q.eq("externalOrgId", args.externalOrgId).eq("externalAuthId", args.externalAuthId),
      )
      .first();

    return {
      hidePreview: settings?.hidePreview ?? false,
      items: rows.map((row: any) => itemView(row, now)),
      nextCursor: page.isDone ? null : page.continueCursor,
    };
  },
});

/**
 * Done / reopen / snooze one of the caller's own rows. Another member's row
 * and a row that does not exist both answer `attention_item_not_found`.
 */
export const setItemStateForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    itemId: v.string(),
    state: v.string(),
    snoozeUntil: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    if (args.state !== "done" && args.state !== "open" && args.state !== "snoozed") {
      throw new Error("invalid_item_state");
    }
    const id = ctx.db.normalizeId("spaceAttentionItems", cleanId(args.itemId, "attention_item_not_found"));
    const row = id ? await ctx.db.get(id) : null;
    if (!row || row.recipientAuthId !== args.externalAuthId || row.externalOrgId !== args.externalOrgId) {
      throw new Error("attention_item_not_found");
    }
    const now = Date.now();
    if (args.state === "snoozed") {
      const check = checkSnooze(args.snoozeUntil, now);
      if (!check.ok) throw new Error("invalid_snooze");
      await ctx.db.patch(row._id, {
        state: "snoozed",
        snoozedUntil: check.snoozedUntil,
        updatedAt: now,
        expiresAt: itemExpiry("snoozed", now),
      });
      return { itemId: String(row._id), state: "snoozed", snoozedUntil: check.snoozedUntil };
    }
    await ctx.db.patch(row._id, {
      state: args.state,
      snoozedUntil: undefined,
      updatedAt: now,
      expiresAt: itemExpiry(args.state, now),
    });
    return { itemId: String(row._id), state: args.state, snoozedUntil: null };
  },
});

/**
 * Mark the caller's rooms read: advance existing markers to now and clear
 * manual floors (never creates a marker), and resolve the caller's open
 * mention/reply items in those rooms. `needs_you` is live-derived at the
 * gateway and is not touched. Idempotent.
 */
export const markAllReadForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRefs: v.array(v.string()),
    kinds: v.optional(v.array(v.string())),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const refs = cleanRefs(args.spaceRefs);
    const wanted = new Set(refs);
    const kinds = new Set<string>(
      args.kinds === undefined
        ? READ_DRIVEN_KINDS
        : args.kinds.map((kind) => {
            if (!isAttentionKind(kind)) throw new Error("invalid_kind");
            return kind;
          }),
    );
    const now = Date.now();

    let roomsMarked = 0;
    const markers = await ctx.db
      .query("spaceReadMarkers")
      .withIndex("by_org_and_subject", (q: any) =>
        q.eq("externalOrgId", args.externalOrgId).eq("externalAuthId", args.externalAuthId),
      )
      .take(2000);
    for (const marker of markers) {
      if (!wanted.has(marker.spaceRef)) continue;
      const lastReadAt = Math.max(marker.lastReadAt, now);
      await ctx.db.patch(marker._id, { lastReadAt, unreadFloor: undefined });
      roomsMarked += 1;
    }

    let itemsResolved = 0;
    for (const row of await ownItems(ctx, args.externalAuthId, args.externalOrgId, "open")) {
      if (!wanted.has(row.spaceRef) || !kinds.has(row.kind)) continue;
      await ctx.db.patch(row._id, {
        state: "done",
        snoozedUntil: undefined,
        updatedAt: now,
        expiresAt: itemExpiry("done", now),
      });
      itemsResolved += 1;
    }
    return { roomsMarked, itemsResolved, markedAt: now };
  },
});

/** The caller's level for one room (the default when never chosen) and the hide-preview flag. */
export const getRoomLevelForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = cleanRef(args.spaceRef);
    const pref = await loadPref(ctx, args.externalAuthId, args.externalOrgId, spaceRef);
    const settings = await loadSettings(ctx, args.externalAuthId, args.externalOrgId);
    return {
      level: (pref?.level as RoomLevel | undefined) ?? DEFAULT_ROOM_LEVEL,
      levelIsDefault: !pref,
      hidePreview: settings?.hidePreview ?? false,
    };
  },
});

async function loadPref(ctx: any, externalAuthId: string, externalOrgId: string, spaceRef: string) {
  const rows = await ctx.db
    .query("spaceRoomNotifyPrefs")
    .withIndex("by_subject_and_space", (q: any) =>
      q.eq("externalAuthId", externalAuthId).eq("spaceRef", spaceRef),
    )
    .take(5);
  return rows.find((row: any) => row.externalOrgId === externalOrgId) ?? null;
}

async function loadSettings(ctx: any, externalAuthId: string, externalOrgId: string) {
  return await ctx.db
    .query("spaceNotifyUserSettings")
    .withIndex("by_subject", (q: any) =>
      q.eq("externalOrgId", externalOrgId).eq("externalAuthId", externalAuthId),
    )
    .first();
}

/**
 * Set the caller's level for a room and/or their hide-preview flag. The levels
 * are a closed vocabulary (`all`, `mentions`, `off`), so this is allowed for
 * ZDR organizations: it stores no content. Membership of the room is the
 * gateway's check.
 */
export const setRoomLevelForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    level: v.optional(v.string()),
    hidePreview: v.optional(v.boolean()),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = cleanRef(args.spaceRef);
    if (args.level !== undefined && !isRoomLevel(args.level)) throw new Error("invalid_level");
    const now = Date.now();

    let pref = await loadPref(ctx, args.externalAuthId, args.externalOrgId, spaceRef);
    if (args.level !== undefined) {
      if (pref) {
        await ctx.db.patch(pref._id, { level: args.level, updatedAt: now });
      } else {
        const id = await ctx.db.insert("spaceRoomNotifyPrefs", {
          externalOrgId: args.externalOrgId,
          externalAuthId: args.externalAuthId,
          spaceRef,
          level: args.level,
          updatedAt: now,
        });
        pref = { _id: id, level: args.level };
      }
    }

    let settings = await loadSettings(ctx, args.externalAuthId, args.externalOrgId);
    if (args.hidePreview !== undefined) {
      if (settings) {
        await ctx.db.patch(settings._id, { hidePreview: args.hidePreview, updatedAt: now });
      } else {
        await ctx.db.insert("spaceNotifyUserSettings", {
          externalOrgId: args.externalOrgId,
          externalAuthId: args.externalAuthId,
          hidePreview: args.hidePreview,
          updatedAt: now,
        });
      }
      settings = { hidePreview: args.hidePreview };
    }

    return {
      level: (pref?.level as RoomLevel | undefined) ?? DEFAULT_ROOM_LEVEL,
      levelIsDefault: !pref,
      hidePreview: settings?.hidePreview ?? false,
    };
  },
});

/**
 * Follow, mute or clear the caller's follow of one thread. An explicit choice
 * is stored with reason `manual`; `none` deletes the row. Membership of the
 * room and the ZDR refusal (a follow is durable) are the gateway's checks.
 */
export const setThreadFollowForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    threadId: v.string(),
    state: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = cleanRef(args.spaceRef);
    const threadId = cleanId(args.threadId, "invalid_thread_id");
    if (args.state !== "following" && args.state !== "muted" && args.state !== "none") {
      throw new Error("invalid_follow_state");
    }
    const rows = await ctx.db
      .query("spaceThreadFollows")
      .withIndex("by_subject_and_thread", (q: any) =>
        q.eq("externalAuthId", args.externalAuthId).eq("threadId", threadId),
      )
      .take(5);
    const existing = rows.find((row: any) => row.externalOrgId === args.externalOrgId) ?? null;
    if (args.state === "none") {
      if (existing) await ctx.db.delete(existing._id);
      return { state: "none", reason: null };
    }
    const now = Date.now();
    if (existing) {
      await ctx.db.patch(existing._id, {
        spaceRef,
        state: args.state,
        reason: "manual",
        updatedAt: now,
      });
    } else {
      await ctx.db.insert("spaceThreadFollows", {
        externalOrgId: args.externalOrgId,
        externalAuthId: args.externalAuthId,
        spaceRef,
        threadId,
        state: args.state,
        reason: "manual",
        updatedAt: now,
      });
    }
    return { state: args.state, reason: "manual" };
  },
});

/**
 * The caller's own follow state for one thread (WS11-S7 follow control): `following`, `muted` or
 * `none`, and why. Reads only the caller's row; a thread nobody follows answers `none`.
 */
export const getThreadFollowForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    threadId: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const threadId = cleanId(args.threadId, "invalid_thread_id");
    const rows = await ctx.db
      .query("spaceThreadFollows")
      .withIndex("by_subject_and_thread", (q: any) =>
        q.eq("externalAuthId", args.externalAuthId).eq("threadId", threadId),
      )
      .take(5);
    const row = rows.find((candidate: any) => candidate.externalOrgId === args.externalOrgId);
    return row ? { state: row.state, reason: row.reason } : { state: "none", reason: null };
  },
});

/**
 * Create or bump an inbox row from a decision. Shared with the event ingest
 * (WS11-S6). `existing` is the row found through `by_recipient_coalesce`.
 */
export async function writeAttentionItem(
  ctx: any,
  input: {
    readonly action: "create" | "increment" | "reopen";
    readonly existing: any | null;
    readonly externalOrgId: string;
    readonly recipientAuthId: string;
    readonly spaceRef: string;
    readonly kind: AttentionKind;
    readonly threadId?: string;
    readonly messageId?: string;
    readonly runId?: string;
    readonly actorKind: "user" | "agent" | "system";
    readonly actorRef?: string;
    readonly eventId: string;
    readonly now: number;
  },
): Promise<void> {
  const { existing, now } = input;
  if (input.action === "create" || !existing) {
    await ctx.db.insert("spaceAttentionItems", {
      externalOrgId: input.externalOrgId,
      recipientAuthId: input.recipientAuthId,
      spaceRef: input.spaceRef,
      kind: input.kind,
      ...(input.threadId ? { threadId: input.threadId } : {}),
      ...(input.messageId ? { messageId: input.messageId } : {}),
      ...(input.runId ? { runId: input.runId } : {}),
      actorKind: input.actorKind,
      ...(input.actorRef ? { actorRef: input.actorRef } : {}),
      state: "open" as const,
      count: 1,
      firstEventId: input.eventId,
      lastEventId: input.eventId,
      coalesceKey: coalesceKey(input.kind, { threadId: input.threadId, spaceRef: input.spaceRef }),
      createdAt: now,
      updatedAt: now,
      expiresAt: itemExpiry("open", now),
    });
    return;
  }
  if (input.action === "reopen") {
    await ctx.db.patch(existing._id, {
      state: "open",
      snoozedUntil: undefined,
      count: 1,
      messageId: input.messageId ?? existing.messageId,
      runId: input.runId ?? existing.runId,
      actorKind: input.actorKind,
      actorRef: input.actorRef ?? existing.actorRef,
      lastEventId: input.eventId,
      updatedAt: now,
      expiresAt: itemExpiry("open", now),
    });
    return;
  }
  await ctx.db.patch(existing._id, {
    count: existing.count + 1,
    messageId: input.messageId ?? existing.messageId,
    runId: input.runId ?? existing.runId,
    lastEventId: input.eventId,
    updatedAt: now,
    expiresAt: itemExpiry(existing.state === "snoozed" ? "snoozed" : "open", now),
  });
}

/**
 * The invite signal. The gateway calls this after Control accepted a member
 * add. The CALLER is the inviter; `inviteeAuthId` is the person who was added.
 * Ingest flag `SPACE_ATTENTION_INGEST` (Convex env, read at handler time) off
 * means nothing is written. The invitee must belong to the organization; the
 * Space audience is NOT consulted because it is reconciled after the Control
 * add and lags it (the row carries only a ref and the gateway re-filters every
 * read through Control's index). Idempotent when `idempotencyKey` is given.
 */
export const recordInviteForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    inviteeAuthId: v.string(),
    idempotencyKey: v.optional(v.string()),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    if (!envFlagOn(process.env.SPACE_ATTENTION_INGEST)) {
      return { recorded: false, reason: "disabled" };
    }
    const spaceRef = cleanRef(args.spaceRef);
    const invitee = cleanId(args.inviteeAuthId, "invalid_invitee");
    const space = await ctx.db
      .query("spaces")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", spaceRef))
      .first();
    if (!space || space.externalOrgId !== args.externalOrgId) throw new Error("space_not_found");
    try {
      await requireGatewayMember(ctx, invitee, args.externalOrgId);
    } catch {
      return { recorded: false, reason: "invitee_not_in_org" };
    }

    const key = (args.idempotencyKey ?? "").trim();
    const eventId = key ? `invite:${key}:${invitee}` : `invite:${spaceRef}:${invitee}:${Date.now()}`;
    if (key) {
      const seen = await ctx.db
        .query("spaceAttentionEvents")
        .withIndex("by_event_id", (q: any) => q.eq("eventId", eventId))
        .first();
      if (seen) return { recorded: false, reason: "duplicate" };
    }

    const existing = await ctx.db
      .query("spaceAttentionItems")
      .withIndex("by_recipient_coalesce", (q: any) =>
        q.eq("recipientAuthId", invitee).eq("coalesceKey", coalesceKey("invite", { spaceRef })),
      )
      .first();
    const decision = deliveryDecision("invite", {
      recipientAuthId: invitee,
      actorAuthId: args.externalAuthId,
      recipientInAudience: true,
      existing: existing && existing.externalOrgId === args.externalOrgId ? existing : null,
    });
    if (decision.item === "none") return { recorded: false, reason: decision.reason };

    const now = Date.now();
    await writeAttentionItem(ctx, {
      action: decision.item,
      existing: existing && existing.externalOrgId === args.externalOrgId ? existing : null,
      externalOrgId: args.externalOrgId,
      recipientAuthId: invitee,
      spaceRef,
      kind: "invite",
      actorKind: "user",
      actorRef: args.externalAuthId,
      eventId,
      now,
    });
    if (key) await ctx.db.insert("spaceAttentionEvents", { eventId, createdAt: now });
    return { recorded: true, reason: decision.item };
  },
});
