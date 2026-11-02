/**
 * Pure rules for Space reminders (WS11-S11). No Convex imports, so the test
 * phase can load this file directly.
 */

/** Pending reminders per member (design D8: the 51st is refused). */
export const MAX_PENDING_REMINDERS = 50;
/** A reminder must be at least this far in the future when it is created. */
export const MIN_REMINDER_LEAD_MS = 5_000;
/** ...and at most this far (one year). */
export const MAX_REMINDER_LEAD_MS = 366 * 24 * 60 * 60 * 1000;
/** The heal cron only picks up reminders this overdue, so it never races the normal schedule. */
export const HEAL_GRACE_MS = 60_000;
/** Fired, cancelled and skipped rows are deleted after this long. */
export const SETTLED_RETENTION_MS = 7 * 24 * 60 * 60 * 1000;

export type ReminderTimeCheck = { readonly ok: true; readonly remindAt: number } | { readonly ok: false };

/** `remind_at` is epoch milliseconds, finite, inside [now + 5 s, now + 366 d]. */
export function checkReminderTime(value: unknown, now: number): ReminderTimeCheck {
  if (typeof value !== "number" || !Number.isFinite(value)) return { ok: false };
  const at = Math.trunc(value);
  if (at < now + MIN_REMINDER_LEAD_MS || at > now + MAX_REMINDER_LEAD_MS) return { ok: false };
  return { ok: true, remindAt: at };
}

/** The event id a fired reminder uses for its item and its out-of-app request. */
export function reminderEventId(reminderId: string): string {
  return `reminder:${reminderId}`;
}
