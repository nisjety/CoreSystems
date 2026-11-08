/**
 * Message reactions (WS12-S8): one person, one emoji, one message.
 *
 * # Application owns this, deliberately
 *
 * "Who reacted to what" is a workspace projection, so it lives here. Session Core stores the
 * message and knows nothing of the reaction. A row holds identifiers, one emoji cluster and a
 * timestamp: never message text and never a name, so a Zero Data Retention organization has
 * nothing to withhold (the gateway still refuses to write one for such an organization).
 *
 * # Who checks what
 *
 * The calling gateway checks the Space lifecycle, the caller's live Control role (editor or
 * above), the Control shared-read decision and, through model-gateway, that the message exists in
 * that Space thread and is not deleted. This plane verifies the service key and that the caller
 * belongs to the organization, validates the emoji, bounds the input and the number of distinct
 * emoji per message, and scopes every row by organization. A reaction grants nothing: it never
 * starts a run, never counts as an approval and nothing here is read by approval or wake code.
 *
 * Flag `SPACE_REACTIONS` (Convex env, set by startup.sh, off by default) is a rollout control only,
 * never an authorization input. Off, writes throw `reactions_disabled` and reads return nothing.
 *
 * Errors are stable codes the gateway maps: `reactions_disabled`, `invalid_emoji`,
 * `reaction_limit`, `reaction_invalid:<field>`.
 */
import { v } from "convex/values";
import { mutation, query } from "./_generated/server";
import { assertServiceKey, requireGatewayMember } from "./authz";

/** Most distinct emoji one message carries. */
export const MAX_DISTINCT_EMOJI_PER_MESSAGE = 20;
/** The longest emoji cluster accepted, in UTF-8 bytes (a family ZWJ sequence is 25). */
export const MAX_EMOJI_BYTES = 32;
/** Rows read for one message / one thread. Well under Convex's per-query document limit. */
const MAX_ROWS_PER_MESSAGE = 2000;
const MAX_ROWS_PER_THREAD = 4000;
const CLEAR_BATCH = 500;
const MAX_ID_CHARS = 200;

export function reactionsEnabled(): boolean {
  const raw = (process.env.SPACE_REACTIONS || "").trim().toLowerCase();
  return raw === "1" || raw === "true";
}

// One pictographic element with an optional variation selector or skin tone; ZWJ joins several.
const PICTOGRAPH = "\\p{Extended_Pictographic}(?:\\uFE0F|\\p{Emoji_Modifier})?";
const ZWJ_SEQUENCE = `${PICTOGRAPH}(?:\\u200D${PICTOGRAPH})*`;
const KEYCAP = "[0-9#*]\\uFE0F?\\u20E3";
const FLAG = "\\p{Regional_Indicator}{2}";
const TAG_FLAG = "\\u{1F3F4}[\\u{E0020}-\\u{E007E}]+\\u{E007F}";
const EMOJI_CLUSTER = new RegExp(`^(?:${TAG_FLAG}|${KEYCAP}|${FLAG}|${ZWJ_SEQUENCE})$`, "u");

/**
 * The emoji in its stored form (NFC), or `null` when it is not exactly one pictographic cluster:
 * at most 32 bytes, optional variation selector, skin tone or ZWJ sequence, a keycap or a flag; no
 * whitespace, no text, no second emoji.
 */
export function validateEmoji(raw: unknown): string | null {
  if (typeof raw !== "string" || raw.length === 0) return null;
  const emoji = raw.normalize("NFC");
  if (new TextEncoder().encode(emoji).length > MAX_EMOJI_BYTES) return null;
  return EMOJI_CLUSTER.test(emoji) ? emoji : null;
}

export interface ReactionRow {
  readonly messageId: string;
  readonly emoji: string;
  readonly externalAuthId: string;
  readonly createdAt: number;
}

export interface ReactionSummary {
  readonly emoji: string;
  readonly count: number;
  readonly reactedByMe: boolean;
}

/** Distinct emoji of one message with counts, in the order each emoji was first used. */
export function summarizeMessage(rows: readonly ReactionRow[], viewer: string): ReactionSummary[] {
  const byEmoji = new Map<string, { count: number; mine: boolean; first: number }>();
  for (const row of rows) {
    const entry = byEmoji.get(row.emoji) ?? { count: 0, mine: false, first: row.createdAt };
    entry.count += 1;
    entry.mine = entry.mine || row.externalAuthId === viewer;
    entry.first = Math.min(entry.first, row.createdAt);
    byEmoji.set(row.emoji, entry);
  }
  return [...byEmoji.entries()]
    .sort((a, b) => a[1].first - b[1].first || a[0].localeCompare(b[0]))
    .map(([emoji, entry]) => ({ emoji, count: entry.count, reactedByMe: entry.mine }));
}

/** Every message's summaries for a thread, flattened to `[{messageId, emoji, count, reactedByMe}]`. */
export function groupForThread(
  rows: readonly ReactionRow[],
  viewer: string,
): Array<ReactionSummary & { messageId: string }> {
  const byMessage = new Map<string, ReactionRow[]>();
  for (const row of rows) {
    const list = byMessage.get(row.messageId);
    if (list) list.push(row);
    else byMessage.set(row.messageId, [row]);
  }
  const out: Array<ReactionSummary & { messageId: string }> = [];
  for (const [messageId, list] of [...byMessage.entries()].sort((a, b) => a[0].localeCompare(b[0]))) {
    for (const summary of summarizeMessage(list, viewer)) out.push({ messageId, ...summary });
  }
  return out;
}

function requireId(value: string, field: string): string {
  const trimmed = (value ?? "").trim();
  if (!trimmed || trimmed.length > MAX_ID_CHARS || /[\u0000-\u001f\u007f]/.test(trimmed)) {
    throw new Error(`reaction_invalid:${field}`);
  }
  return trimmed;
}

/** Rows of one message that belong to this organization, Space and thread. */
async function messageRows(ctx: any, org: string, spaceRef: string, threadId: string, messageId: string) {
  const rows = await ctx.db
    .query("spaceReactions")
    .withIndex("by_message_subject_emoji", (q: any) => q.eq("messageId", messageId))
    .take(MAX_ROWS_PER_MESSAGE);
  return rows.filter(
    (row: any) => row.externalOrgId === org && row.spaceRef === spaceRef && row.threadId === threadId,
  );
}

/**
 * Add (`present: true`) or remove (`present: false`) the caller's reaction. Idempotent. Returns the
 * message's summary after the change. Adding a 21st distinct emoji throws `reaction_limit`.
 */
export const setForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    threadId: v.string(),
    messageId: v.string(),
    emoji: v.string(),
    present: v.boolean(),
  },
  handler: async (ctx, args): Promise<{ messageId: string; reactions: ReactionSummary[] }> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    if (!reactionsEnabled()) throw new Error("reactions_disabled");
    const spaceRef = requireId(args.spaceRef, "space_ref");
    const threadId = requireId(args.threadId, "thread_id");
    const messageId = requireId(args.messageId, "message_id");
    const emoji = validateEmoji(args.emoji);
    if (!emoji) throw new Error("invalid_emoji");
    const org = args.externalOrgId;
    const rows = await messageRows(ctx, org, spaceRef, threadId, messageId);
    const mine = rows.find((row: any) => row.externalAuthId === args.externalAuthId && row.emoji === emoji);
    if (args.present) {
      if (!mine) {
        const distinct = new Set(rows.map((row: any) => row.emoji));
        if (!distinct.has(emoji) && distinct.size >= MAX_DISTINCT_EMOJI_PER_MESSAGE) {
          throw new Error("reaction_limit");
        }
        const createdAt = Date.now();
        await ctx.db.insert("spaceReactions", {
          externalOrgId: org,
          spaceRef,
          threadId,
          messageId,
          emoji,
          externalAuthId: args.externalAuthId,
          createdAt,
        });
        rows.push({ messageId, emoji, externalAuthId: args.externalAuthId, createdAt } as any);
      }
    } else if (mine) {
      await ctx.db.delete(mine._id);
      rows.splice(rows.indexOf(mine), 1);
    }
    return { messageId, reactions: summarizeMessage(rows, args.externalAuthId) };
  },
});

/**
 * Every reaction of a thread, grouped per message and emoji, with `reactedByMe` for the caller.
 * Off, or nothing stored, answers an empty list; `truncated` is true when the thread holds more
 * rows than one read returns.
 */
export const reactionsForThreadForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    threadId: v.string(),
  },
  handler: async (
    ctx,
    args,
  ): Promise<{ enabled: boolean; truncated: boolean; reactions: Array<ReactionSummary & { messageId: string }> }> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    if (!reactionsEnabled()) return { enabled: false, truncated: false, reactions: [] };
    const spaceRef = requireId(args.spaceRef, "space_ref");
    const threadId = requireId(args.threadId, "thread_id");
    const rows = await ctx.db
      .query("spaceReactions")
      .withIndex("by_space_thread", (q: any) => q.eq("spaceRef", spaceRef).eq("threadId", threadId))
      .take(MAX_ROWS_PER_THREAD + 1);
    const truncated = rows.length > MAX_ROWS_PER_THREAD;
    const own = rows
      .slice(0, MAX_ROWS_PER_THREAD)
      .filter((row: any) => row.externalOrgId === args.externalOrgId);
    return { enabled: true, truncated, reactions: groupForThread(own, args.externalAuthId) };
  },
});

/**
 * Drop every reaction of one message (the message was deleted). Best effort from the gateway; not
 * gated by the flag so a switched-off store still clears. `more` is true when a further call is
 * needed.
 */
export const clearForMessageForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    threadId: v.string(),
    messageId: v.string(),
  },
  handler: async (ctx, args): Promise<{ cleared: number; more: boolean }> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = requireId(args.spaceRef, "space_ref");
    const threadId = requireId(args.threadId, "thread_id");
    const messageId = requireId(args.messageId, "message_id");
    const rows = await ctx.db
      .query("spaceReactions")
      .withIndex("by_message_subject_emoji", (q: any) => q.eq("messageId", messageId))
      .take(CLEAR_BATCH);
    let cleared = 0;
    for (const row of rows) {
      if (row.externalOrgId !== args.externalOrgId || row.spaceRef !== spaceRef || row.threadId !== threadId) continue;
      await ctx.db.delete(row._id);
      cleared += 1;
    }
    return { cleared, more: rows.length >= CLEAR_BATCH };
  },
});
