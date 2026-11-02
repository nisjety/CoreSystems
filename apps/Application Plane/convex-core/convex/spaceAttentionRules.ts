/**
 * The pure rules behind Space attention (WS11-S3).
 *
 * Everything that decides "does this person get an inbox item, and may it also
 * go out of the app" is a pure function here, the way `spaceReadMarkers`'s
 * derivation is. The Convex handlers (`spaceAttention.ts`, and the event ingest
 * in WS11-S6) are thin shells that load rows, call these, and write the answer.
 * Tested directly through `require('../convex/spaceAttentionRules.ts')`.
 *
 * Nothing here reads a title, a name or message text. Inputs are ids, closed
 * words and timestamps; outputs are the same.
 */

export const ATTENTION_KINDS = [
  "mention",
  "reply",
  "invite",
  "agent_done",
  "agent_failed",
  "reminder",
] as const;
export type AttentionKind = (typeof ATTENTION_KINDS)[number];

/** Signals that only trigger an out-of-app request; in-app they are the live `needs_you`. */
export const OUT_OF_APP_ONLY_KINDS = ["approval_needed", "agent_awaiting"] as const;
export type OutOfAppOnlyKind = (typeof OUT_OF_APP_ONLY_KINDS)[number];

export type SignalKind = AttentionKind | OutOfAppOnlyKind;

export type RoomLevel = "all" | "mentions" | "off";
export const ROOM_LEVELS: readonly RoomLevel[] = ["all", "mentions", "off"];
/** PD-WS11: the default room level when a member never chose one. */
export const DEFAULT_ROOM_LEVEL: RoomLevel = "mentions";

export type MemberState = "available" | "away" | "dnd" | "offline";

/** Out-of-app budget: 30 requests per recipient per hour. */
export const OUT_OF_APP_BUDGET_LIMIT = 30;
export const OUT_OF_APP_BUDGET_WINDOW_MS = 60 * 60 * 1000;
/** A member counts as observing a room for this long after their last presence beat. */
export const OBSERVING_WINDOW_MS = 30_000;
/** Item life: 30 days after the last update; `done` rows 14 days more. */
export const ITEM_LIFE_MS = 30 * 24 * 60 * 60 * 1000;
export const DONE_ITEM_LIFE_MS = 14 * 24 * 60 * 60 * 1000;
/** Snooze bounds. */
export const MIN_SNOOZE_MS = 60 * 1000;
export const MAX_SNOOZE_MS = 30 * 24 * 60 * 60 * 1000;
/** Rooms per aggregate call and followers per thread. */
export const MAX_ATTENTION_REFS = 60;
export const MAX_THREAD_FOLLOWERS = 200;
/** Open rows kept per recipient (the hourly sweep enforces it; reads are bounded by it). */
export const MAX_ITEMS_PER_RECIPIENT = 500;

export function isRoomLevel(value: unknown): value is RoomLevel {
  return value === "all" || value === "mentions" || value === "off";
}

export function isAttentionKind(value: unknown): value is AttentionKind {
  return typeof value === "string" && (ATTENTION_KINDS as readonly string[]).includes(value);
}

/** Convex env flag read at handler time: set and not an off word. Empty = off. */
export function envFlagOn(value: string | undefined | null, defaultWhenUnset = false): boolean {
  if (value === undefined || value === null) return defaultWhenUnset;
  const word = value.trim().toLowerCase();
  if (word === "") return false;
  return !["0", "false", "off", "no"].includes(word);
}

/**
 * The scope an item coalesces on: the thread when there is one, else the Space.
 * The recipient is the index prefix (`by_recipient_coalesce`), so it is not part
 * of the key.
 */
export function coalesceKey(
  kind: SignalKind,
  scope: { readonly threadId?: string | null; readonly spaceRef: string },
): string {
  const thread = (scope.threadId ?? "").trim();
  return thread ? `${kind}:t:${thread}` : `${kind}:s:${scope.spaceRef.trim()}`;
}

export interface ExistingItemLike {
  readonly state: "open" | "done" | "snoozed";
  readonly snoozedUntil?: number | null;
}

export interface DeliveryContext {
  readonly recipientAuthId: string;
  /** The author / starter / inviter. Empty for system signals. */
  readonly actorAuthId?: string | null;
  /** Recipient is an active, acknowledged member of the Space audience, in the event's org. */
  readonly recipientInAudience: boolean;
  /** `null` when the member never chose: the default applies. */
  readonly level?: RoomLevel | null;
  readonly threadMuted?: boolean;
  readonly following?: boolean;
  /** For `agent_done` / `agent_failed`: the recipient started the run. */
  readonly isRunStarter?: boolean;
  /** Presence within {@link OBSERVING_WINDOW_MS} AND the suppress flag on. */
  readonly observingAndSuppressed?: boolean;
  readonly existing?: ExistingItemLike | null;
  readonly memberState?: MemberState | null;
  /** Budget has room for one more out-of-app request. */
  readonly budgetAvailable?: boolean;
}

export type ItemAction = "none" | "create" | "increment" | "reopen";

export interface DeliveryDecision {
  readonly item: ItemAction;
  readonly outOfApp: boolean;
  /** Closed word for logs and tests; never recipient or space ids. */
  readonly reason: string;
}

const DROP = (reason: string): DeliveryDecision => ({ item: "none", outOfApp: false, reason });

/**
 * The fixed precedence (design D5), top to bottom:
 *
 *  1. recipient equals actor: drop (a reminder is the exception: it is the
 *     member's own signal to themselves);
 *  2. recipient must be an active, acknowledged audience member: drop;
 *  3. `mention`, `invite` (and `reminder`) pass every level and mute;
 *  4. level `off` drops everything else; a muted thread drops;
 *  5. `reply` needs a following recipient; `agent_done` / `agent_failed` go to
 *     the run starter only;
 *  6. observed room: no item for `reply` and `agent_done` (mentions are kept);
 *  7. coalesce on (recipient, kind, thread or space): one row; `done` reopens
 *     with count 1; `snoozed` only counts up;
 *  8. out-of-app only for a new or reopened row, level not `off` (mention,
 *     invite and approval excepted), status not `dnd`, budget available.
 *
 * `approval_needed` and `agent_awaiting` create no item (in-app they are the
 * live `needs_you`); they pass steps 1-2 and 4 (muted thread) and decide only
 * `outOfApp`.
 */
export function deliveryDecision(kind: SignalKind, ctx: DeliveryContext): DeliveryDecision {
  const actor = (ctx.actorAuthId ?? "").trim();
  const recipient = ctx.recipientAuthId.trim();
  if (!recipient) return DROP("no_recipient");
  const isReminder = kind === "reminder";
  const isOutOfAppOnly = isOutOfAppOnlyKind(kind);

  // 1
  if (!isReminder && actor && actor === recipient) return DROP("self");
  // 2
  if (!ctx.recipientInAudience) return DROP("not_in_audience");

  const passesEveryLevel = kind === "mention" || kind === "invite" || isReminder || kind === "approval_needed";
  const level = ctx.level ?? DEFAULT_ROOM_LEVEL;

  // 3 and 4
  if (!passesEveryLevel) {
    if (level === "off") return DROP("level_off");
    if (ctx.threadMuted) return DROP("thread_muted");
  }

  // 5
  if (kind === "reply" && !ctx.following) return DROP("not_following");
  if ((kind === "agent_done" || kind === "agent_failed") && !ctx.isRunStarter) {
    return DROP("not_run_starter");
  }

  // 6
  if ((kind === "reply" || kind === "agent_done") && ctx.observingAndSuppressed) {
    return DROP("observed_room");
  }

  // 7
  let item: ItemAction = "none";
  let isNewOrReopened = false;
  if (!isOutOfAppOnly) {
    const existing = ctx.existing ?? null;
    if (!existing) {
      item = "create";
      isNewOrReopened = true;
    } else if (existing.state === "done") {
      item = "reopen";
      isNewOrReopened = true;
    } else {
      // open, or snoozed: the count goes up and nothing else changes. A snoozed
      // row stays snoozed until its time; it does not go out of the app again.
      item = "increment";
    }
  } else {
    isNewOrReopened = true;
  }

  // 8
  const levelAllowsOut = level !== "off" || kind === "mention" || kind === "invite" || kind === "approval_needed";
  const outOfApp =
    isNewOrReopened &&
    levelAllowsOut &&
    ctx.memberState !== "dnd" &&
    ctx.budgetAvailable !== false;

  return { item, outOfApp, reason: item === "none" ? "out_of_app_only" : item };
}

export function isOutOfAppOnlyKind(kind: string): kind is OutOfAppOnlyKind {
  return kind === "approval_needed" || kind === "agent_awaiting";
}

export interface BudgetState {
  readonly windowStart: number;
  readonly count: number;
}

/**
 * Fixed-window budget. Returns whether one more request is allowed and the row
 * to store. A denied call does not increase the count. A window older than
 * {@link OUT_OF_APP_BUDGET_WINDOW_MS} restarts at `now`.
 */
export function consumeBudget(
  state: BudgetState | null | undefined,
  now: number,
  limit: number = OUT_OF_APP_BUDGET_LIMIT,
  windowMs: number = OUT_OF_APP_BUDGET_WINDOW_MS,
): { readonly allowed: boolean; readonly next: BudgetState } {
  if (!state || !Number.isFinite(state.windowStart) || now - state.windowStart >= windowMs || now < state.windowStart) {
    return limit > 0
      ? { allowed: true, next: { windowStart: now, count: 1 } }
      : { allowed: false, next: { windowStart: now, count: 0 } };
  }
  if (state.count >= limit) return { allowed: false, next: state };
  return { allowed: true, next: { windowStart: state.windowStart, count: state.count + 1 } };
}

export type AgentActivityState = "working" | "waiting" | "idle";

export interface RunSignal {
  /** In-app item kind for the run starter, if the state ends a run. */
  readonly item: "agent_done" | "agent_failed" | null;
  /** Out-of-app only trigger. */
  readonly outOfApp: OutOfAppOnlyKind | null;
  /** What the agent availability chip shows afterwards; `null` leaves it alone. */
  readonly activity: AgentActivityState | null;
}

/**
 * The single mapper from a run-state word to attention signals (conflict C17).
 * The sibling session's run-state enum (`needs_input > failed > running >
 * parked > queued > unseen_done > idle`) and WS02's `run_state` words both land
 * here; a rename on their side changes this one function. Unknown words map to
 * "nothing", never to a guess.
 */
export function runStateToSignal(state: string | null | undefined): RunSignal {
  switch ((state ?? "").trim().toLowerCase()) {
    case "completed":
    case "succeeded":
    case "done":
    case "unseen_done":
      return { item: "agent_done", outOfApp: null, activity: "idle" };
    case "failed":
    case "errored":
    case "error":
      return { item: "agent_failed", outOfApp: null, activity: "idle" };
    case "awaiting_approval":
      return { item: null, outOfApp: "approval_needed", activity: "waiting" };
    case "needs_input":
      return { item: null, outOfApp: "agent_awaiting", activity: "waiting" };
    case "running":
    case "queued":
      return { item: null, outOfApp: null, activity: "working" };
    case "cancelled":
    case "canceled":
    case "parked":
    case "idle":
      return { item: null, outOfApp: null, activity: "idle" };
    default:
      return { item: null, outOfApp: null, activity: null };
  }
}

export interface FollowRowLike {
  readonly state: "following" | "muted";
  readonly reason: "started" | "participated" | "mentioned" | "manual";
}

/**
 * Merge an automatic follow into the existing row. An explicit `muted` is never
 * overwritten by an auto-follow; a manual follow is never downgraded to an
 * automatic reason. Returns `null` when nothing should change.
 */
export function mergeAutoFollow(
  existing: FollowRowLike | null | undefined,
  reason: "started" | "participated" | "mentioned",
): FollowRowLike | null {
  if (!existing) return { state: "following", reason };
  if (existing.state === "muted") return null;
  if (existing.reason === "manual") return null;
  if (existing.reason === reason) return null;
  return { state: "following", reason };
}

export type BadgeKind = "count" | "dot" | "none";

/**
 * The badge rule (mirrored in Rust at the gateway, which is the one that is
 * shipped): `n = needs_you + mentions`; `n > 0` gives a count at every level;
 * else level `all` with unread a muted count, `mentions` a dot, `off` nothing.
 * Kept here so the Convex tests pin the same table.
 */
export function roomBadge(input: {
  readonly needsYou: number;
  readonly mentions: number;
  readonly level: RoomLevel;
  readonly unread: boolean;
}): { readonly kind: BadgeKind; readonly n: number } {
  const n = Math.max(0, input.needsYou) + Math.max(0, input.mentions);
  if (n > 0) return { kind: "count", n };
  if (!input.unread) return { kind: "none", n: 0 };
  if (input.level === "all") return { kind: "count", n: 0 };
  if (input.level === "mentions") return { kind: "dot", n: 0 };
  return { kind: "none", n: 0 };
}

export type SnoozeCheck =
  | { readonly ok: true; readonly snoozedUntil: number }
  | { readonly ok: false };

/** now + 1 minute .. now + 30 days. */
export function checkSnooze(snoozeUntil: unknown, now: number): SnoozeCheck {
  if (typeof snoozeUntil !== "number" || !Number.isFinite(snoozeUntil)) return { ok: false };
  if (snoozeUntil < now + MIN_SNOOZE_MS || snoozeUntil > now + MAX_SNOOZE_MS) return { ok: false };
  return { ok: true, snoozedUntil: Math.floor(snoozeUntil) };
}

/** A snoozed row whose time has passed counts as open. */
export function effectiveItemState(
  row: { readonly state: "open" | "done" | "snoozed"; readonly snoozedUntil?: number | null },
  now: number,
): "open" | "done" | "snoozed" {
  if (row.state === "snoozed" && typeof row.snoozedUntil === "number" && row.snoozedUntil <= now) {
    return "open";
  }
  return row.state;
}

/** `expiresAt` for a row last updated at `updatedAt`; a done row lives 14 more days. */
export function itemExpiry(state: "open" | "done" | "snoozed", updatedAt: number): number {
  return updatedAt + (state === "done" ? DONE_ITEM_LIFE_MS : ITEM_LIFE_MS);
}
