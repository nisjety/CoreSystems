/**
 * The Space event ingest behind attention (WS11-S6).
 *
 * # Flow
 *
 * Session Core writes one content-free event per persistent Space log row
 * (`mp.v1.space.<space_ref>.event`, WS11-S5). `nats-subscriber.js` binds the
 * durable consumer and posts each event to `/api/webhook/nats/onSpaceEvent`;
 * `ingestSpaceEvent` below applies it and the subscriber acks only after this
 * mutation succeeded. A thrown error means "redeliver"; a bad event is
 * answered with `{status: "rejected"}` so it is acknowledged and dropped, not
 * retried forever.
 *
 * # Authority
 *
 * Nothing in an event is an authorization input. The recipient must be in the
 * ACTIVE, acknowledged recipient audience of the Space (Convex's projection of
 * Control's roster) and in the event's organization, or no row is written for
 * them (a mention of a non-member does nothing and reveals nothing). Level,
 * mute and follow rows are the recipient's own. Rows hold ids, closed words and
 * timestamps; there is no title, text or name. Events with `zdr` anything but
 * the literal `false` are refused by the envelope parser, so no ZDR content
 * path reaches these tables. `SPACE_ATTENTION_INGEST` (Convex env, read at
 * handler time) is a rollout switch only: unset or off means events are
 * acknowledged and dropped. Erasure is the exception: a `thread.deleted` event
 * purges its rows with the switch off too (no dedupe row is written then, so a
 * later replay purges again; the purge is idempotent).
 *
 * # Delivery rules
 *
 * The fixed precedence lives in `deliveryDecision` (spaceAttentionRules.ts);
 * this file loads the rows that function needs and writes its answer.
 */
import { v } from "convex/values";
import { internal } from "./_generated/api";
import { internalAction, internalMutation, internalQuery } from "./_generated/server";
import {
  DEFAULT_ROOM_LEVEL,
  MAX_ITEMS_PER_RECIPIENT,
  MAX_THREAD_FOLLOWERS,
  OBSERVING_WINDOW_MS,
  coalesceKey,
  consumeBudget,
  courierKindFor,
  defaultRoomLevel,
  deliveryDecision,
  envFlagOn,
  isDmSpaceKind,
  mergeAutoFollow,
  runStateToSignal,
  type AttentionKind,
  type DeliveryDecision,
  type MemberState,
  type RoomLevel,
  type SignalKind,
} from "./spaceAttentionRules";
import { parseSpaceEvent, planMessageRecipients, type ParsedSpaceEvent } from "./spaceAttentionEnvelope";
import { writeAttentionItem } from "./spaceAttention";
import {
  COURIER_MAX_ATTEMPTS,
  COURIER_PATH,
  COURIER_REQUEST_TIMEOUT_MS,
  COURIER_ZDR_REFUSAL,
  buildCourierRequest,
  courierGateDecision,
  courierRetryDelayMs,
  isRetryableCourierStatus,
  signCourierRequest,
} from "./spaceAttentionCourier";

/** Events are kept for dedupe this long (matches the stream's 48 h retention). */
const EVENT_DEDUPE_MS = 48 * 60 * 60 * 1000;
/** Rows touched per purge/sweep transaction; a full batch reschedules itself. */
const PURGE_BATCH = 200;
const SWEEP_BATCH = 500;

export type IngestResult = {
  status: "disabled" | "rejected" | "duplicate" | "dropped" | "applied";
  reason?: string;
  items?: number;
  outOfApp?: number;
};

interface Signal {
  readonly recipient: string;
  readonly kind: SignalKind;
  readonly actorAuthId: string;
  readonly actorKind: "user" | "agent" | "system";
  readonly isRunStarter?: boolean;
  readonly messageId?: string;
  readonly runId?: string;
  /** D22r: the message was posted in a DM or group DM. */
  readonly dmRoom?: boolean;
}

async function loadAudience(ctx: any, orgId: string, spaceRef: string): Promise<Set<string>> {
  const audience = await ctx.db
    .query("spaceRecipientAudiences")
    .withIndex("by_space_and_state", (q: any) => q.eq("spaceRef", spaceRef).eq("state", "active"))
    .first();
  if (!audience || audience.controlState !== "acknowledged" || audience.externalOrgId !== orgId) {
    return new Set();
  }
  return new Set<string>(audience.recipientExternalAuthIds);
}

async function loadThreadFollows(ctx: any, ev: ParsedSpaceEvent): Promise<any[]> {
  const rows = await ctx.db
    .query("spaceThreadFollows")
    .withIndex("by_thread", (q: any) => q.eq("threadId", ev.threadId))
    .take(MAX_THREAD_FOLLOWERS + 1);
  return rows.filter((row: any) => row.externalOrgId === ev.orgId && row.spaceRef === ev.spaceRef);
}

async function upsertAutoFollow(
  ctx: any,
  ev: ParsedSpaceEvent,
  follows: any[],
  subject: string,
  reason: "started" | "participated" | "mentioned",
  now: number,
): Promise<void> {
  const existing = follows.find((row) => row.externalAuthId === subject) ?? null;
  const merged = mergeAutoFollow(existing, reason);
  if (!merged) return;
  if (existing) {
    await ctx.db.patch(existing._id, { state: merged.state, reason: merged.reason, updatedAt: now });
    existing.state = merged.state;
    existing.reason = merged.reason;
    return;
  }
  if (follows.length >= MAX_THREAD_FOLLOWERS) {
    // Capped per thread: the overflow is a log line (a count, no ids), not a row.
    console.warn("[space-attention] follower cap reached; auto-follow skipped");
    return;
  }
  const id = await ctx.db.insert("spaceThreadFollows", {
    externalOrgId: ev.orgId,
    externalAuthId: subject,
    spaceRef: ev.spaceRef,
    threadId: ev.threadId,
    state: merged.state,
    reason: merged.reason,
    updatedAt: now,
  });
  follows.push({ _id: id, externalAuthId: subject, state: merged.state, reason: merged.reason });
}

async function isObserving(ctx: any, ev: ParsedSpaceEvent, recipient: string, now: number): Promise<boolean> {
  if (!envFlagOn(process.env.SPACE_ATTENTION_SUPPRESS_OBSERVED, true)) return false;
  const row = await ctx.db
    .query("spacePresence")
    .withIndex("by_space_and_subject", (q: any) => q.eq("spaceRef", ev.spaceRef).eq("externalAuthId", recipient))
    .first();
  return !!row && row.externalOrgId === ev.orgId && row.status !== "offline" && now - row.updatedAt <= OBSERVING_WINDOW_MS;
}

async function memberStateOf(ctx: any, orgId: string, recipient: string, now: number): Promise<MemberState | null> {
  const row = await ctx.db
    .query("spaceMemberStatus")
    .withIndex("by_org_and_subject", (q: any) => q.eq("externalOrgId", orgId).eq("externalAuthId", recipient))
    .first();
  if (!row) return null;
  if (typeof row.expiresAt === "number" && row.expiresAt <= now) return null;
  return row.state as MemberState;
}

/** Fixed window of 30 out-of-app requests per recipient per hour. */
export async function takeBudget(ctx: any, recipient: string, now: number): Promise<boolean> {
  const row = await ctx.db
    .query("spaceAttentionBudget")
    .withIndex("by_recipient", (q: any) => q.eq("recipientAuthId", recipient))
    .first();
  const result = consumeBudget(row ? { windowStart: row.windowStart, count: row.count } : null, now);
  if (row) {
    if (result.next.windowStart !== row.windowStart || result.next.count !== row.count) {
      await ctx.db.patch(row._id, { windowStart: result.next.windowStart, count: result.next.count });
    }
  } else if (result.allowed) {
    await ctx.db.insert("spaceAttentionBudget", {
      recipientAuthId: recipient,
      windowStart: result.next.windowStart,
      count: result.next.count,
    });
  }
  return result.allowed;
}

/** Keep at most {@link MAX_ITEMS_PER_RECIPIENT} rows per recipient; oldest `done` rows go first. */
async function enforceRecipientCap(ctx: any, recipient: string): Promise<void> {
  const rows = await ctx.db
    .query("spaceAttentionItems")
    .withIndex("by_recipient_state_updated", (q: any) => q.eq("recipientAuthId", recipient))
    .take(MAX_ITEMS_PER_RECIPIENT + 1);
  if (rows.length <= MAX_ITEMS_PER_RECIPIENT) return;
  const rank = (row: any) => (row.state === "done" ? 0 : 1);
  const victim = [...rows].sort((a, b) => rank(a) - rank(b) || a.updatedAt - b.updatedAt)[0];
  await ctx.db.delete(victim._id);
}

async function applySignal(
  ctx: any,
  ev: ParsedSpaceEvent,
  audience: ReadonlySet<string>,
  follows: readonly any[],
  signal: Signal,
  now: number,
): Promise<{ decision: DeliveryDecision; outOfApp: boolean }> {
  const { recipient, kind } = signal;
  const inAudience = audience.has(recipient);
  // Step 1-2 of the precedence are cheap and need no reads: skip the row loads.
  if (!inAudience || recipient === signal.actorAuthId) {
    return { decision: deliveryDecision(kind, { recipientAuthId: recipient, actorAuthId: signal.actorAuthId, recipientInAudience: inAudience }), outOfApp: false };
  }

  const prefs = await ctx.db
    .query("spaceRoomNotifyPrefs")
    .withIndex("by_subject_and_space", (q: any) => q.eq("externalAuthId", recipient).eq("spaceRef", ev.spaceRef))
    .take(5);
  const pref = prefs.find((row: any) => row.externalOrgId === ev.orgId);
  const level: RoomLevel | null = pref ? (pref.level as RoomLevel) : null;
  const follow = follows.find((row) => row.externalAuthId === recipient) ?? null;
  const existingRow = signal.kind === "approval_needed" || signal.kind === "agent_awaiting"
    ? null
    : await ctx.db
        .query("spaceAttentionItems")
        .withIndex("by_recipient_coalesce", (q: any) =>
          q.eq("recipientAuthId", recipient).eq("coalesceKey", coalesceKey(kind, { threadId: ev.threadId, spaceRef: ev.spaceRef })),
        )
        .first();
  const existing = existingRow && existingRow.externalOrgId === ev.orgId ? existingRow : null;

  const decision = deliveryDecision(kind, {
    recipientAuthId: recipient,
    actorAuthId: signal.actorAuthId,
    recipientInAudience: true,
    level: level ?? (signal.dmRoom ? defaultRoomLevel("dm") : DEFAULT_ROOM_LEVEL),
    threadMuted: follow?.state === "muted",
    dmRoom: signal.dmRoom === true,
    // A reply recipient is a follower by construction (a follow row or the thread
    // owner); a muted row is handled above. In a DM every member is planned as a
    // `reply` (D22r), so there only a row or ownership counts as following.
    following: follow
      ? follow.state === "following"
      : !signal.dmRoom || recipient === ev.threadOwnerSubjectId,
    isRunStarter: signal.isRunStarter === true,
    observingAndSuppressed: await isObserving(ctx, ev, recipient, now),
    existing,
    memberState: await memberStateOf(ctx, ev.orgId, recipient, now),
  });

  if (decision.item !== "none") {
    if (decision.item === "create") await enforceRecipientCap(ctx, recipient);
    await writeAttentionItem(ctx, {
      action: decision.item,
      existing,
      externalOrgId: ev.orgId,
      recipientAuthId: recipient,
      spaceRef: ev.spaceRef,
      kind: kind as AttentionKind,
      threadId: ev.threadId,
      messageId: signal.messageId,
      runId: signal.runId,
      actorKind: signal.actorKind,
      actorRef: signal.actorAuthId || undefined,
      eventId: ev.eventId,
      now,
    });
  }

  let outOfApp = false;
  if (decision.outOfApp && envFlagOn(process.env.SPACE_ATTENTION_OUT_OF_APP)) {
    outOfApp = await takeBudget(ctx, recipient, now);
  }
  if (outOfApp) {
    await ctx.scheduler.runAfter(0, internal.spaceAttentionIngest.deliverOutOfApp, {
      eventId: ev.eventId,
      externalOrgId: ev.orgId,
      recipientAuthId: recipient,
      spaceRef: ev.spaceRef,
      kind: courierKindFor(kind, signal.dmRoom === true),
      threadId: ev.threadId,
      messageId: signal.messageId,
      runId: signal.runId,
      actorKind: signal.actorKind,
      zdr: ev.zdr,
    });
  }
  return { decision, outOfApp };
}

/**
 * Apply one Space event. Internal only: reached through the NATS webhook, which
 * authenticates with the Control projection key. Idempotent per event id.
 */
export const ingestSpaceEvent = internalMutation({
  args: { event: v.any() },
  handler: async (ctx, args): Promise<IngestResult> => {
    if (!envFlagOn(process.env.SPACE_ATTENTION_INGEST)) {
      // DEPTH-3D-PURGE: erasure is not gated by the rollout switch. The event is
      // acked either way and never redelivered, so a gated erasure would be lost.
      const off = parseSpaceEvent(args.event);
      if (off.ok) {
        const ev = (off as { event: ParsedSpaceEvent }).event;
        if (ev.kind === "thread.deleted") {
          await eraseThread(ctx, ev.orgId, ev.spaceRef, ev.threadId);
          return { status: "applied", reason: "thread_purged" };
        }
      }
      return { status: "disabled" };
    }

    const parsed = parseSpaceEvent(args.event);
    if (!parsed.ok) {
      const reason = (parsed as { reason: string }).reason;
      console.warn(`[space-attention] rejected event: ${reason}`);
      return { status: "rejected", reason };
    }
    const ev = (parsed as { event: ParsedSpaceEvent }).event;
    const now = Date.now();

    const seen = await ctx.db
      .query("spaceAttentionEvents")
      .withIndex("by_event_id", (q: any) => q.eq("eventId", ev.eventId))
      .first();
    if (seen) return { status: "duplicate" };

    const finish = async (result: IngestResult): Promise<IngestResult> => {
      await ctx.db.insert("spaceAttentionEvents", { eventId: ev.eventId, createdAt: now });
      return result;
    };

    // Erasure does not depend on the audience or the Space still being active.
    if (ev.kind === "thread.deleted") {
      await eraseThread(ctx, ev.orgId, ev.spaceRef, ev.threadId);
      return await finish({ status: "applied", reason: "thread_purged" });
    }

    const space = await ctx.db
      .query("spaces")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", ev.spaceRef))
      .first();
    if (!space || space.externalOrgId !== ev.orgId || space.lifecycle !== "active") {
      return await finish({ status: "dropped", reason: "space_unavailable" });
    }

    const audience = await loadAudience(ctx, ev.orgId, ev.spaceRef);
    let items = 0;
    let outOfApp = 0;
    const tally = (r: { decision: DeliveryDecision; outOfApp: boolean }) => {
      if (r.decision.item !== "none") items += 1;
      if (r.outOfApp) outOfApp += 1;
    };

    if (ev.kind === "message.appended") {
      // Assistant messages create no item (the run's own signal does).
      if (ev.role !== "user") return await finish({ status: "applied", reason: "assistant_message" });
      const follows = await loadThreadFollows(ctx, ev);
      // D22r: in a DM every message is for every other member, follow or not.
      const dmRoom = isDmSpaceKind(space.kind);
      const plan = planMessageRecipients({
        actor: ev.authorSubjectId,
        mentions: ev.mentions,
        followers: follows.filter((row) => row.state === "following").map((row) => row.externalAuthId),
        owner: ev.threadOwnerSubjectId,
        roomMembers: dmRoom ? [...audience].sort() : undefined,
      });

      for (const { recipient, kind } of plan) {
        tally(
          await applySignal(
            ctx,
            ev,
            audience,
            follows,
            {
              recipient,
              kind,
              actorAuthId: ev.authorSubjectId,
              actorKind: "user",
              messageId: ev.messageId,
              dmRoom,
            },
            now,
          ),
        );
      }

      // Follows follow participation. Only audience members get a row.
      if (ev.threadOwnerSubjectId && audience.has(ev.threadOwnerSubjectId)) {
        await upsertAutoFollow(ctx, ev, follows, ev.threadOwnerSubjectId, "started", now);
      }
      if (ev.authorSubjectId && ev.authorSubjectId !== ev.threadOwnerSubjectId && audience.has(ev.authorSubjectId)) {
        await upsertAutoFollow(ctx, ev, follows, ev.authorSubjectId, "participated", now);
      }
      for (const mentioned of ev.mentions) {
        if (mentioned !== ev.authorSubjectId && audience.has(mentioned)) {
          await upsertAutoFollow(ctx, ev, follows, mentioned, "mentioned", now);
        }
      }
      return await finish({ status: "applied", items, outOfApp });
    }

    // run.state_changed
    const signal = runStateToSignal(ev.state);
    const starter = ev.starterSubjectId;
    if (starter && (signal.item || signal.outOfApp)) {
      const follows = await loadThreadFollows(ctx, ev);
      const kind: SignalKind = (signal.item ?? signal.outOfApp) as SignalKind;
      tally(
        await applySignal(
          ctx,
          ev,
          audience,
          follows,
          {
            recipient: starter,
            kind,
            // The agent's Control subject (agent_id = runs.agent_subject_id, set only by a
            // verified wake; DEPTH-WS11-S5-2) names the actor; empty for any other run.
            actorAuthId: ev.agentId ?? "",
            actorKind: "agent",
            isRunStarter: true,
            runId: ev.runId,
          },
          now,
        ),
      );
    }
    if (ev.agentId && signal.activity) {
      const row = await ctx.db
        .query("spaceAgentActivity")
        .withIndex("by_space_and_agent", (q: any) => q.eq("spaceRef", ev.spaceRef).eq("agentId", ev.agentId!))
        .first();
      const patch = { state: signal.activity, runId: ev.runId, updatedAt: now };
      if (row && row.externalOrgId === ev.orgId) {
        await ctx.db.patch(row._id, patch);
      } else if (!row) {
        await ctx.db.insert("spaceAgentActivity", {
          externalOrgId: ev.orgId,
          spaceRef: ev.spaceRef,
          agentId: ev.agentId,
          ...patch,
        });
      }
    }
    return await finish({ status: "applied", items, outOfApp });
  },
});

/**
 * The out-of-app courier (WS11-S9; retries and the delivery gate,
 * DEPTH-WS11-COURIER). S6 decides and schedules (flag, rules, budget); this
 * posts one refs-only request to notification-core as service id
 * `space-attention`. Off unless `SPACE_ATTENTION_OUT_OF_APP` is on and both
 * `NOTIFICATION_CORE_URL` and `NOTIFICATION_SPACE_ATTENTION_SERVICE_TOKEN` are
 * set; a missing one is a quiet "not delivered", never a thrown error.
 *
 * Every attempt runs the same checks: the flag; the ZDR posture stamped on the
 * job, which must be the literal `false` (anything else is refused, fail
 * closed); then `courierGate`, which reads the Space, the audience and the
 * recipient's status as they are now. The ZDR check reads only the stamped
 * flag, not the organization's live posture (Convex holds none): today every
 * caller stamps `false` (ZDR events are refused at ingest, reminder routes are
 * refused by the gateway under ZDR), so the refusal fires only for jobs queued
 * without the field and for future callers. An organization that turns ZDR on
 * after the job was queued is not stopped here.
 *
 * Each request is bounded by `COURIER_REQUEST_TIMEOUT_MS`; a send that throws,
 * gets no answer in time, or gets a 408, 429 or 5xx is retried on
 * `COURIER_RETRY_DELAYS_MS`; any other refusal is final. Each attempt sends the
 * same body, so the idempotency key `<eventId>:<recipient>` repeats. That key
 * alone does not prevent a duplicate: notification-core re-dispatches a stored
 * request it did not deliver, and what stops a second message is the
 * provider's dedupe on the request id. Whether anything is actually sent also
 * depends on notification-core's own delivery mode and the recipient's
 * per-type email preference.
 */
export const deliverOutOfApp = internalAction({
  args: {
    eventId: v.string(),
    externalOrgId: v.string(),
    recipientAuthId: v.string(),
    spaceRef: v.string(),
    kind: v.string(),
    threadId: v.optional(v.string()),
    messageId: v.optional(v.string()),
    runId: v.optional(v.string()),
    actorKind: v.string(),
    // Absent on a job queued before this check existed: refused, never sent.
    zdr: v.optional(v.boolean()),
    // 1-based. Absent on the first attempt.
    attempt: v.optional(v.number()),
  },
  handler: async (ctx, args): Promise<{ delivered: boolean; reason: string }> => {
    if (!envFlagOn(process.env.SPACE_ATTENTION_OUT_OF_APP)) return { delivered: false, reason: "disabled" };
    if (args.zdr !== false) return { delivered: false, reason: COURIER_ZDR_REFUSAL };
    const baseUrl = (process.env.NOTIFICATION_CORE_URL || "").trim().replace(/\/+$/, "");
    const secret = (process.env.NOTIFICATION_SPACE_ATTENTION_SERVICE_TOKEN || "").trim();
    if (!baseUrl || !secret) return { delivered: false, reason: "not_configured" };
    const request = buildCourierRequest(args);
    if (!request) return { delivered: false, reason: "unsupported_kind" };

    const attempt = args.attempt ?? 1;
    const retryOrGiveUp = async (reason: string): Promise<{ delivered: boolean; reason: string }> => {
      const delay = courierRetryDelayMs(attempt);
      if (delay === null) {
        console.warn(`[space-attention] courier gave up after ${attempt} attempts`);
        return { delivered: false, reason };
      }
      await ctx.scheduler.runAfter(delay, internal.spaceAttentionIngest.deliverOutOfApp, {
        ...args,
        attempt: attempt + 1,
      });
      console.warn(`[space-attention] courier attempt ${attempt + 1} of ${COURIER_MAX_ATTEMPTS} scheduled`);
      return { delivered: false, reason };
    };

    let gate: { send: boolean; reason: string };
    try {
      gate = await ctx.runQuery(internal.spaceAttentionIngest.courierGate, {
        externalOrgId: args.externalOrgId,
        spaceRef: args.spaceRef,
        recipientAuthId: args.recipientAuthId,
      });
    } catch {
      console.warn("[space-attention] courier gate unavailable");
      return await retryOrGiveUp("gate_unavailable");
    }
    if (!gate.send) return { delivered: false, reason: gate.reason };

    const body = JSON.stringify(request);
    let failure: string;
    const abort = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const timedOut = new Promise<"timeout">((resolve) => {
      timer = setTimeout(() => {
        abort.abort();
        resolve("timeout");
      }, COURIER_REQUEST_TIMEOUT_MS);
    });
    try {
      const headers = await signCourierRequest(secret, request.organization_id, body);
      // The race bounds the attempt even if the fetch ignores the abort signal.
      const response = await Promise.race([
        fetch(`${baseUrl}${COURIER_PATH}`, { method: "POST", headers, body, signal: abort.signal }),
        timedOut,
      ]);
      if (response === "timeout") {
        console.warn("[space-attention] courier timed out");
        failure = "timeout";
      } else {
        if (response.ok) return { delivered: true, reason: "accepted" };
        // Status only: the response body is never logged.
        console.warn(`[space-attention] courier refused: status ${response.status}`);
        failure = `http_${response.status}`;
        if (!isRetryableCourierStatus(response.status)) return { delivered: false, reason: failure };
      }
    } catch {
      console.warn("[space-attention] courier unreachable");
      failure = abort.signal.aborted ? "timeout" : "unreachable";
    } finally {
      clearTimeout(timer);
    }
    return await retryOrGiveUp(failure);
  },
});

/**
 * The delivery gate for `deliverOutOfApp` (DEPTH-WS11-COURIER). Re-reads, for
 * the courier's own moment, the three things the decision read: the Space is
 * still active in the event's organization, the recipient is still in its
 * active, acknowledged audience, and the recipient is not in dnd. Internal
 * only; the action reaches it through `ctx.runQuery`.
 */
export const courierGate = internalQuery({
  args: { externalOrgId: v.string(), spaceRef: v.string(), recipientAuthId: v.string() },
  handler: async (ctx, args): Promise<{ send: boolean; reason: string }> => {
    const now = Date.now();
    const space = await ctx.db
      .query("spaces")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", args.spaceRef))
      .first();
    const spaceActive = !!space && space.externalOrgId === args.externalOrgId && space.lifecycle === "active";
    const recipientInAudience =
      spaceActive && (await loadAudience(ctx, args.externalOrgId, args.spaceRef)).has(args.recipientAuthId);
    const memberState = await memberStateOf(ctx, args.externalOrgId, args.recipientAuthId, now);
    return courierGateDecision({ spaceActive, recipientInAudience, memberState });
  },
});

// ---------------------------------------------------------------------------
// Erasure
// ---------------------------------------------------------------------------

/** Delete up to one batch of `rows` that belong to `orgId`. Returns whether more may remain. */
async function deleteBatch(ctx: any, rows: any[], orgId: string): Promise<boolean> {
  let deleted = 0;
  for (const row of rows) {
    if (row.externalOrgId !== orgId) continue;
    await ctx.db.delete(row._id);
    deleted += 1;
  }
  return deleted > 0 && rows.length >= PURGE_BATCH;
}

/** Purge one batch of a deleted thread's rows and schedule `purgeThread` for the rest. */
async function eraseThread(ctx: any, orgId: string, spaceRef: string, threadId: string): Promise<void> {
  const more = await purgeThreadRows(ctx, orgId, spaceRef, threadId);
  if (more) {
    await ctx.scheduler.runAfter(0, internal.spaceAttentionIngest.purgeThread, {
      externalOrgId: orgId,
      spaceRef,
      threadId,
    });
  }
}

async function purgeThreadRows(ctx: any, orgId: string, spaceRef: string | undefined, threadId: string): Promise<boolean> {
  let more = false;
  more = (await deleteBatch(
    ctx,
    await ctx.db.query("spaceAttentionItems").withIndex("by_thread", (q: any) => q.eq("threadId", threadId)).take(PURGE_BATCH),
    orgId,
  )) || more;
  more = (await deleteBatch(
    ctx,
    await ctx.db.query("spaceThreadFollows").withIndex("by_thread", (q: any) => q.eq("threadId", threadId)).take(PURGE_BATCH),
    orgId,
  )) || more;
  // WS12-S8: the thread's message reactions. by_space_thread leads with spaceRef, so the Space is named too.
  // A job queued before spaceRef was an argument has none; it still purges items and follows.
  if (spaceRef !== undefined) {
    more = (await deleteBatch(
      ctx,
      await ctx.db.query("spaceReactions").withIndex("by_space_thread", (q: any) => q.eq("spaceRef", spaceRef).eq("threadId", threadId)).take(PURGE_BATCH),
      orgId,
    )) || more;
  }
  return more;
}

/** A thread was deleted: its items, follows and message reactions go. Pages itself until empty. */
export const purgeThread = internalMutation({
  // spaceRef is optional only so a job queued with the older arguments still validates.
  args: { externalOrgId: v.string(), spaceRef: v.optional(v.string()), threadId: v.string() },
  handler: async (ctx, args): Promise<{ more: boolean }> => {
    const more = await purgeThreadRows(ctx, args.externalOrgId, args.spaceRef, args.threadId);
    if (more) await ctx.scheduler.runAfter(0, internal.spaceAttentionIngest.purgeThread, args);
    return { more };
  },
});

/**
 * A Space is being deleted: every attention, follow, preference, reminder,
 * agent-activity, read-marker, presence, reaction, pin, room-notice and DM-hide
 * row for it goes. Scheduled beside the audience purge in `spaces.ts`.
 */
export const purgeSpace = internalMutation({
  args: { externalOrgId: v.string(), spaceRef: v.string() },
  handler: async (ctx, args): Promise<{ more: boolean }> => {
    const { externalOrgId: org, spaceRef } = args;
    const take = (table: string, index: string, build: (q: any) => any) =>
      (ctx.db as any).query(table).withIndex(index, build).take(PURGE_BATCH);
    let more = false;
    more = (await deleteBatch(ctx, await take("spaceAttentionItems", "by_space", (q) => q.eq("spaceRef", spaceRef)), org)) || more;
    more = (await deleteBatch(ctx, await take("spaceThreadFollows", "by_space", (q) => q.eq("spaceRef", spaceRef)), org)) || more;
    more = (await deleteBatch(ctx, await take("spaceAgentActivity", "by_space_and_agent", (q) => q.eq("spaceRef", spaceRef)), org)) || more;
    more = (await deleteBatch(ctx, await take("spaceReadMarkers", "by_space_and_subject", (q) => q.eq("spaceRef", spaceRef)), org)) || more;
    more = (await deleteBatch(ctx, await take("spacePresence", "by_space", (q) => q.eq("spaceRef", spaceRef)), org)) || more;
    more = (await deleteBatch(ctx, await take("spaceRoomNotifyPrefs", "by_space", (q) => q.eq("spaceRef", spaceRef)), org)) || more;
    more = (await deleteBatch(ctx, await take("spaceReminders", "by_space", (q) => q.eq("spaceRef", spaceRef)), org)) || more;
    // WS12-S8: message reactions of the Space.
    more = (await deleteBatch(ctx, await take("spaceReactions", "by_space_thread", (q) => q.eq("spaceRef", spaceRef)), org)) || more;
    more = (await deleteBatch(ctx, await take("spacePins", "by_space", (q) => q.eq("spaceRef", spaceRef)), org)) || more;
    // WS15-S12: room notices of the Space (by_space_and_created leads with spaceRef).
    more = (await deleteBatch(ctx, await take("spaceRoomNotices", "by_space_and_created", (q) => q.eq("spaceRef", spaceRef)), org)) || more;
    // WS12-S2: per-viewer DM hides of the Space.
    more = (await deleteBatch(ctx, await take("spaceDmHides", "by_space", (q) => q.eq("spaceRef", spaceRef)), org)) || more;
    if (more) await ctx.scheduler.runAfter(0, internal.spaceAttentionIngest.purgeSpace, args);
    return { more };
  },
});

/**
 * A member left the organization: their attention rows in that organization go
 * (items, follows, level and preview settings, read markers, status, reminders).
 * Scheduled from the membership-removal projection.
 */
export const purgeSubject = internalMutation({
  args: { externalOrgId: v.string(), externalAuthId: v.string() },
  handler: async (ctx, args): Promise<{ more: boolean }> => {
    const { externalOrgId: org, externalAuthId: subject } = args;
    const take = (table: string, index: string, build: (q: any) => any) =>
      (ctx.db as any).query(table).withIndex(index, build).take(PURGE_BATCH);
    const bySubjectOrg = (q: any) => q.eq("externalOrgId", org).eq("externalAuthId", subject);
    let more = false;
    more = (await deleteBatch(ctx, await take("spaceAttentionItems", "by_recipient_state_updated", (q) => q.eq("recipientAuthId", subject)), org)) || more;
    more = (await deleteBatch(ctx, await take("spaceThreadFollows", "by_subject_and_thread", (q) => q.eq("externalAuthId", subject)), org)) || more;
    more = (await deleteBatch(ctx, await take("spaceRoomNotifyPrefs", "by_subject", bySubjectOrg), org)) || more;
    more = (await deleteBatch(ctx, await take("spaceNotifyUserSettings", "by_subject", bySubjectOrg), org)) || more;
    more = (await deleteBatch(ctx, await take("spaceReadMarkers", "by_org_and_subject", bySubjectOrg), org)) || more;
    more = (await deleteBatch(ctx, await take("spaceMemberStatus", "by_org_and_subject", bySubjectOrg), org)) || more;
    more = (await deleteBatch(ctx, await take("spaceReminders", "by_subject_and_state", (q) => q.eq("externalAuthId", subject)), org)) || more;
    if (more) await ctx.scheduler.runAfter(0, internal.spaceAttentionIngest.purgeSubject, args);
    return { more };
  },
});

/**
 * Hourly housekeeping: items past `expiresAt` (30 days after the last update,
 * `done` rows 14 days), and dedupe rows older than 48 hours. Deletes only;
 * safe to run repeatedly and with the ingest flag off. A full batch schedules
 * the next one.
 */
export const sweep = internalMutation({
  args: {},
  handler: async (ctx): Promise<{ items: number; events: number }> => {
    const now = Date.now();
    const expired = await ctx.db
      .query("spaceAttentionItems")
      .withIndex("by_expiry", (q: any) => q.lt("expiresAt", now))
      .take(SWEEP_BATCH);
    for (const row of expired) await ctx.db.delete(row._id);
    const oldEvents = await ctx.db
      .query("spaceAttentionEvents")
      .withIndex("by_created", (q: any) => q.lt("createdAt", now - EVENT_DEDUPE_MS))
      .take(SWEEP_BATCH);
    for (const row of oldEvents) await ctx.db.delete(row._id);
    if (expired.length >= SWEEP_BATCH || oldEvents.length >= SWEEP_BATCH) {
      await ctx.scheduler.runAfter(0, internal.spaceAttentionIngest.sweep, {});
    }
    return { items: expired.length, events: oldEvents.length };
  },
});
