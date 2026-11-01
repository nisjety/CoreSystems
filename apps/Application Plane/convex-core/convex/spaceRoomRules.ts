/**
 * Application-owned room display-detail rules (WS04-S2).
 *
 * A room's name and topic are LABELS: authored text shown in headers and lists.
 * They are never an authorization input, never composed into a model prompt and
 * never copied into an event or a notification (D-WS04-6). This module is pure
 * (no Convex imports) so the gateway's Rust copy of the rules and the Convex
 * mutation can be tested against the same table.
 */

export const ROOM_NAME_MAX_LENGTH = 120;
export const ROOM_TOPIC_MAX_LENGTH = 250;

export type RoomDetailsField = 'name' | 'topic';

export type RoomDetailsRejectionReason =
  | 'name_required'
  | 'name_too_long'
  | 'topic_too_long'
  | 'control_character'
  | 'bidi_override'
  | 'not_a_string';

export type RoomDetailsInput = Readonly<{
  name?: unknown;
  topic?: unknown;
}>;

export type NormalizedRoomDetails = Readonly<{
  /** Present only when the caller supplied a name. */
  name?: string;
  /** Present only when the caller supplied a topic; `null` clears it. */
  topic?: string | null;
}>;

// One flat shape (not a discriminated union): the Convex package compiles with
// `strict: false`, where narrowing on `ok` does not work. `ok: true` carries
// `value`; `ok: false` carries `field` and `reason`.
export type RoomDetailsResult = Readonly<{
  ok: boolean;
  value?: NormalizedRoomDetails;
  field?: RoomDetailsField;
  reason?: RoomDetailsRejectionReason;
}>;

type Rejection = Readonly<{ ok: false; field: RoomDetailsField; reason: RoomDetailsRejectionReason }>;

// Bidirectional overrides and isolates: U+202A-202E and U+2066-2069. They let a
// label render as something other than what it stores.
const BIDI_CONTROL = /[\u202A-\u202E\u2066-\u2069]/u;
// C0/C1 control characters. Tab, line feed and carriage return are ordinary
// whitespace and are collapsed to one space instead of rejected.
const CONTROL = /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F-\u009F]/u;

function codePointLength(value: string): number {
  let count = 0;
  for (const _ of value) count += 1;
  return count;
}

/** The normalised text, or a rejection. */
function normalizeText(field: RoomDetailsField, raw: unknown): string | Rejection {
  if (typeof raw !== 'string') return { ok: false, field, reason: 'not_a_string' };
  const nfc = raw.normalize('NFC');
  if (BIDI_CONTROL.test(nfc)) return { ok: false, field, reason: 'bidi_override' };
  if (CONTROL.test(nfc)) return { ok: false, field, reason: 'control_character' };
  return nfc.replace(/\s+/gu, ' ').trim();
}

/**
 * Validates and normalises a room name and/or topic.
 *
 * Unicode NFC, trimmed, inner whitespace collapsed to single spaces. Name is
 * 1 to 120 characters; topic is 0 to 250 and an empty topic clears it (`null`).
 * Control characters and bidi overrides are rejected. A field the caller did
 * not supply is left out of the result so a name-only update never touches the
 * topic.
 */
export function normalizeRoomDetails(input: RoomDetailsInput): RoomDetailsResult {
  const out: { name?: string; topic?: string | null } = {};

  if (input.name !== undefined) {
    const name = normalizeText('name', input.name);
    if (typeof name !== 'string') return name;
    if (name.length === 0) return { ok: false, field: 'name', reason: 'name_required' };
    if (codePointLength(name) > ROOM_NAME_MAX_LENGTH) {
      return { ok: false, field: 'name', reason: 'name_too_long' };
    }
    out.name = name;
  }

  if (input.topic !== undefined) {
    if (input.topic === null) {
      out.topic = null;
    } else {
      const topic = normalizeText('topic', input.topic);
      if (typeof topic !== 'string') return topic;
      if (codePointLength(topic) > ROOM_TOPIC_MAX_LENGTH) {
        return { ok: false, field: 'topic', reason: 'topic_too_long' };
      }
      out.topic = topic.length === 0 ? null : topic;
    }
  }

  return { ok: true, value: out };
}
