/**
 * Reminders (WS11-S11, design D8).
 *
 * # Flow
 *
 * The gateway creates a reminder for the CALLER on a thread (and optionally one
 * message). Creation schedules `fireReminder` with `ctx.scheduler.runAfter`; the
 * five minute cron `healReminders` re-schedules any pending reminder that is
 * overdue (a lost schedule) and deletes settled rows. At fire time the member
 * is re-checked against the ACTIVE, acknowledged recipient audience of the Space
 * and the organization; a member who was removed gets `skipped`, not an item.
 * A fired reminder is an inbox item of kind `reminder` carrying the Space ref
 * and the thread and message ids only.
 *
 * # Authority
 *
 * Every public function starts with `assertServiceKey` (the caller is the
 * gateway) and then `requireGatewayMember`, and touches only the caller's own
 * rows; another member's reminder answers exactly like one that does not exist.
 * Nothing here is an authorization input and nothing widens access to a Space:
 * the gateway has already proved membership of the room it passes, the item is
 * ids only, and the inbox re-reads titles under the viewer's current decision.
 *
 * # Content and ZDR
 *
 * There is no note field: a note would be user content. The row holds ids, a
 * closed state word and timestamps. The gateway refuses reminder routes for a
 * ZDR organization with the typed 412 before calling these functions.
 *
 * # Errors
 *
 * Plain `Error` messages that are stable codes the gateway maps to its typed
 * envelope: `invalid_space_ref`, `invalid_thread_id`, `invalid_message_id`,
 * `invalid_reminder_time`, `too_many_reminders`, `space_not_found`,
 * `reminder_not_found`.
 */
import { v } from "convex/values";
import { internal } from "./_generated/api";
import { internalMutation, mutation, query } from "./_generated/server";
import { assertServiceKey, requireGatewayMember } from "./authz";
import { writeAttentionItem } from "./spaceAttention";
import { takeBudget } from "./spaceAttentionIngest";
import { DEFAULT_ROOM_LEVEL, coalesceKey, deliveryDecision, envFlagOn, type RoomLevel } from "./spaceAttentionRules";
import {
  HEAL_GRACE_MS,
  MAX_PENDING_REMINDERS,
  SETTLED_RETENTION_MS,
  checkReminderTime,
  reminderEventId,
} from "./spaceRemindersRules";

const MAX_ID_CHARS = 200;
const HEAL_BATCH = 100;
const SETTLED_BATCH = 200;

function cleanId(value: string | undefined, code: string): string {
  const id = (value ?? "").trim();
  if (!id || id.length > MAX_ID_CHARS) throw new Error(code);
  return id;
}

function view(row: any) {
  return {
    reminderId: row._id as string,
    spaceRef: row.spaceRef as string,
    threadId: row.threadId as string,
    messageId: (row.messageId as string | undefined) ?? null,
    remindAt: row.remindAt as number,
    state: row.state as string,
    createdAt: row.createdAt as number,
  };
}

async function pendingOf(ctx: any, externalAuthId: string, externalOrgId: string, limit: number): Promise<any[]> {
  const rows = await ctx.db
    .query("spaceReminders")
    .withIndex("by_subject_and_state", (q: any) => q.eq("externalAuthId", externalAuthId).eq("state", "pending"))
    .take(limit);
  return rows.filter((row: any) => row.externalOrgId === externalOrgId);
}

/**
 * Create a reminder for the caller. Idempotent: a pending reminder with the
 * same Space, thread, message and time is returned instead of a second row.
 */
export const createReminderForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    threadId: v.string(),
    messageId: v.optional(v.string()),
    remindAt: v.number(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = cleanId(args.spaceRef, "invalid_space_ref");
    const threadId = cleanId(args.threadId, "invalid_thread_id");
    const messageId = args.messageId === undefined ? undefined : cleanId(args.messageId, "invalid_message_id");
    const now = Date.now();
    const time = checkReminderTime(args.remindAt, now);
    if (!time.ok) throw new Error("invalid_reminder_time");

    const space = await ctx.db
      .query("spaces")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", spaceRef))
      .first();
    if (!space || space.externalOrgId !== args.externalOrgId) throw new Error("space_not_found");

    const pending = await pendingOf(ctx, args.externalAuthId, args.externalOrgId, MAX_PENDING_REMINDERS + 1);
    const same = pending.find(
      (row) =>
        row.spaceRef === spaceRef &&
        row.threadId === threadId &&
        (row.messageId ?? undefined) === messageId &&
        row.remindAt === time.remindAt,
    );
    if (same) return { ...view(same), deduplicated: true };
    if (pending.length >= MAX_PENDING_REMINDERS) throw new Error("too_many_reminders");

    const id = await ctx.db.insert("spaceReminders", {
      externalOrgId: args.externalOrgId,
      externalAuthId: args.externalAuthId,
      spaceRef,
      threadId,
      ...(messageId ? { messageId } : {}),
      remindAt: time.remindAt,
      state: "pending" as const,
      createdAt: now,
    });
    await ctx.scheduler.runAfter(Math.max(0, time.remindAt - now), internal.spaceReminders.fireReminder, {
      reminderId: id,
    });
    return {
      reminderId: id as string,
      spaceRef,
      threadId,
      messageId: messageId ?? null,
      remindAt: time.remindAt,
      state: "pending",
      createdAt: now,
      deduplicated: false,
    };
  },
});

/** The caller's own pending reminders, soonest first (at most 50). */
export const listPendingRemindersForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const rows = await pendingOf(ctx, args.externalAuthId, args.externalOrgId, MAX_PENDING_REMINDERS);
    return rows.sort((a, b) => a.remindAt - b.remindAt).map(view);
  },
});

/**
 * Cancel one of the caller's own pending reminders. Already cancelled returns
 * `cancelled: true` again; a fired, skipped, foreign or unknown id is
 * `reminder_not_found`. The scheduled `fireReminder` finds a non-pending row
 * and does nothing.
 */
export const cancelReminderForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    reminderId: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const normalized = ctx.db.normalizeId("spaceReminders", args.reminderId.trim());
    const row = normalized ? await ctx.db.get(normalized) : null;
    if (!row || row.externalAuthId !== args.externalAuthId || row.externalOrgId !== args.externalOrgId) {
      throw new Error("reminder_not_found");
    }
    if (row.state === "cancelled") return { reminderId: args.reminderId, cancelled: true };
    if (row.state !== "pending") throw new Error("reminder_not_found");
    await ctx.db.patch(row._id, { state: "cancelled" as const });
    return { reminderId: args.reminderId, cancelled: true };
  },
});

/**
 * Fire one reminder: re-check the member, write (or count up) the `reminder`
 * inbox item and, when out-of-app delivery is on, hand a refs-only request to
 * the courier. Idempotent: only a `pending` row acts.
 */
export const fireReminder = internalMutation({
  args: { reminderId: v.id("spaceReminders") },
  handler: async (ctx, args): Promise<{ status: "fired" | "skipped" | "ignored"; reason?: string }> => {
    const row = await ctx.db.get(args.reminderId);
    if (!row || row.state !== "pending") return { status: "ignored", reason: "not_pending" };
    const now = Date.now();
    // An early call (a heal that raced a reschedule) waits for its own schedule.
    if (row.remindAt > now + 1000) return { status: "ignored", reason: "not_due" };

    const skip = async (reason: string) => {
      await ctx.db.patch(row._id, { state: "skipped" as const });
      return { status: "skipped" as const, reason };
    };

    const space = await ctx.db
      .query("spaces")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", row.spaceRef))
      .first();
    if (!space || space.externalOrgId !== row.externalOrgId || space.lifecycle !== "active") {
      return await skip("space_unavailable");
    }
    try {
      await requireGatewayMember(ctx, row.externalAuthId, row.externalOrgId);
    } catch {
      return await skip("not_in_org");
    }
    const audience = await ctx.db
      .query("spaceRecipientAudiences")
      .withIndex("by_space_and_state", (q: any) => q.eq("spaceRef", row.spaceRef).eq("state", "active"))
      .first();
    const inAudience =
      !!audience &&
      audience.controlState === "acknowledged" &&
      audience.externalOrgId === row.externalOrgId &&
      (audience.recipientExternalAuthIds as string[]).includes(row.externalAuthId);
    if (!inAudience) return await skip("not_in_audience");

    const eventId = reminderEventId(row._id as string);
    const prefs = await ctx.db
      .query("spaceRoomNotifyPrefs")
      .withIndex("by_subject_and_space", (q: any) =>
        q.eq("externalAuthId", row.externalAuthId).eq("spaceRef", row.spaceRef),
      )
      .take(5);
    const pref = prefs.find((p: any) => p.externalOrgId === row.externalOrgId);
    const level: RoomLevel = pref ? (pref.level as RoomLevel) : DEFAULT_ROOM_LEVEL;
    const status = await ctx.db
      .query("spaceMemberStatus")
      .withIndex("by_org_and_subject", (q: any) =>
        q.eq("externalOrgId", row.externalOrgId).eq("externalAuthId", row.externalAuthId),
      )
      .first();
    const memberState =
      status && !(typeof status.expiresAt === "number" && status.expiresAt <= now) ? status.state : null;
    const existingRow = await ctx.db
      .query("spaceAttentionItems")
      .withIndex("by_recipient_coalesce", (q: any) =>
        q
          .eq("recipientAuthId", row.externalAuthId)
          .eq("coalesceKey", coalesceKey("reminder", { threadId: row.threadId, spaceRef: row.spaceRef })),
      )
      .first();
    const existing = existingRow && existingRow.externalOrgId === row.externalOrgId ? existingRow : null;

    const decision = deliveryDecision("reminder", {
      recipientAuthId: row.externalAuthId,
      actorAuthId: "",
      recipientInAudience: true,
      level,
      existing,
      memberState,
    });
    if (decision.item !== "none") {
      await writeAttentionItem(ctx, {
        action: decision.item,
        existing,
        externalOrgId: row.externalOrgId,
        recipientAuthId: row.externalAuthId,
        spaceRef: row.spaceRef,
        kind: "reminder",
        threadId: row.threadId,
        messageId: row.messageId,
        actorKind: "system",
        eventId,
        now,
      });
    }
    await ctx.db.patch(row._id, { state: "fired" as const });

    if (
      decision.outOfApp &&
      envFlagOn(process.env.SPACE_ATTENTION_OUT_OF_APP) &&
      (await takeBudget(ctx, row.externalAuthId, now))
    ) {
      await ctx.scheduler.runAfter(0, internal.spaceAttentionIngest.deliverOutOfApp, {
        eventId,
        externalOrgId: row.externalOrgId,
        recipientAuthId: row.externalAuthId,
        spaceRef: row.spaceRef,
        kind: "reminder",
        threadId: row.threadId,
        messageId: row.messageId,
        actorKind: "system",
      });
    }
    return { status: "fired", reason: decision.reason };
  },
});

/**
 * The five minute net (cron): re-schedule pending reminders that are overdue
 * (their own schedule was lost) and delete settled rows past retention.
 */
export const healReminders = internalMutation({
  args: {},
  handler: async (ctx): Promise<{ rescheduled: number; deleted: number }> => {
    const now = Date.now();
    const overdue = await ctx.db
      .query("spaceReminders")
      .withIndex("by_due", (q: any) => q.eq("state", "pending").lte("remindAt", now - HEAL_GRACE_MS))
      .take(HEAL_BATCH);
    for (const row of overdue) {
      await ctx.scheduler.runAfter(0, internal.spaceReminders.fireReminder, { reminderId: row._id });
    }

    let deleted = 0;
    for (const state of ["fired", "cancelled", "skipped"] as const) {
      const old = await ctx.db
        .query("spaceReminders")
        .withIndex("by_due", (q: any) => q.eq("state", state).lte("remindAt", now - SETTLED_RETENTION_MS))
        .take(SETTLED_BATCH);
      for (const row of old) {
        await ctx.db.delete(row._id);
        deleted += 1;
      }
    }
    return { rescheduled: overdue.length, deleted };
  },
});
