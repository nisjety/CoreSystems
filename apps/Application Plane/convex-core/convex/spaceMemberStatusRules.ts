/**
 * Pure rules for the org-scoped Space status and the agent availability chips
 * (WS11-S10). No database, no Convex imports, so a plain Node test can load the
 * file like `spaceAttentionRules.ts`.
 *
 * The status is a closed state plus optional short text and an optional expiry.
 * The text is the one free-text field in the attention tables. It is never
 * copied into a prompt, an event or a notification; this file only shapes and
 * checks it.
 */

import type { MemberState } from "./spaceAttentionRules";

export const STATUS_STATES: readonly MemberState[] = ["available", "away", "dnd", "offline"];
export const STATUS_TEXT_MAX_CHARS = 80;
export const STATUS_MAX_LIFE_MS = 7 * 24 * 60 * 60 * 1000;
export const MAX_STATUS_LOOKUP_SUBJECTS = 200;
export const MAX_ACTIVITY_LOOKUP_AGENTS = 100;
/** An agent activity row older than this reads as idle (the run events stopped). */
export const AGENT_ACTIVITY_STALE_MS = 20 * 60 * 1000;

export type AgentChipState = "working" | "waiting" | "idle";

export type StatusRejection =
  | "invalid_state"
  | "text_not_a_string"
  | "text_too_long"
  | "text_bidi_override"
  | "text_control_character"
  | "invalid_expiry"
  | "expiry_in_past"
  | "expiry_too_far";

export type StatusCheck =
  | { ok: true; state: MemberState; text: string | null; expiresAt: number | null }
  | { ok: false; reason: StatusRejection };

// Bidirectional overrides and isolates. They let a label render as something
// other than what it stores.
const BIDI_CONTROL = /[‪-‮⁦-⁩]/u;
// C0/C1 controls except tab, line feed and carriage return (collapsed below).
const CONTROL = /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F-\u009F]/u;

export function isMemberState(value: unknown): value is MemberState {
  return typeof value === "string" && (STATUS_STATES as readonly string[]).includes(value);
}

function codePointLength(value: string): number {
  let count = 0;
  for (const _ of value) count += 1;
  return count;
}

/**
 * Validate a status write. Text is NFC, whitespace collapsed, at most 80
 * characters; empty text means no text. Expiry, when given, must be in the
 * future and at most 7 days away. Absent expiry means the status stays until
 * changed.
 */
export function checkStatusInput(
  input: { state: unknown; text?: unknown; expiresAt?: unknown },
  now: number,
): StatusCheck {
  if (!isMemberState(input.state)) return { ok: false, reason: "invalid_state" };

  let text: string | null = null;
  if (input.text !== undefined && input.text !== null) {
    if (typeof input.text !== "string") return { ok: false, reason: "text_not_a_string" };
    const nfc = input.text.normalize("NFC");
    if (BIDI_CONTROL.test(nfc)) return { ok: false, reason: "text_bidi_override" };
    if (CONTROL.test(nfc)) return { ok: false, reason: "text_control_character" };
    const clean = nfc.replace(/\s+/gu, " ").trim();
    if (codePointLength(clean) > STATUS_TEXT_MAX_CHARS) return { ok: false, reason: "text_too_long" };
    text = clean.length === 0 ? null : clean;
  }

  let expiresAt: number | null = null;
  if (input.expiresAt !== undefined && input.expiresAt !== null) {
    if (typeof input.expiresAt !== "number" || !Number.isFinite(input.expiresAt)) {
      return { ok: false, reason: "invalid_expiry" };
    }
    if (input.expiresAt <= now) return { ok: false, reason: "expiry_in_past" };
    if (input.expiresAt > now + STATUS_MAX_LIFE_MS) return { ok: false, reason: "expiry_too_far" };
    expiresAt = Math.floor(input.expiresAt);
  }

  return { ok: true, state: input.state, text, expiresAt };
}

export interface StatusRowLike {
  state: string;
  text?: string | null;
  expiresAt?: number | null;
  updatedAt: number;
}

export interface StatusView {
  state: MemberState;
  text: string | null;
  expiresAt: number | null;
  updatedAt: number;
}

/** An expired row reads as cleared (null). Text is dropped when the caller says so (ZDR org). */
export function statusView(
  row: StatusRowLike | null | undefined,
  now: number,
  includeText: boolean,
): StatusView | null {
  if (!row || !isMemberState(row.state)) return null;
  if (typeof row.expiresAt === "number" && row.expiresAt <= now) return null;
  return {
    state: row.state,
    text: includeText && typeof row.text === "string" && row.text.length > 0 ? row.text : null,
    expiresAt: typeof row.expiresAt === "number" ? row.expiresAt : null,
    updatedAt: row.updatedAt,
  };
}

export interface ActivityRowLike {
  state: string;
  updatedAt: number;
}

/** Agent chip state: a missing, unknown or stale row reads as idle. */
export function agentChipState(row: ActivityRowLike | null | undefined, now: number): AgentChipState {
  if (!row) return "idle";
  if (now - row.updatedAt > AGENT_ACTIVITY_STALE_MS) return "idle";
  if (row.state === "working" || row.state === "waiting") return row.state;
  return "idle";
}
