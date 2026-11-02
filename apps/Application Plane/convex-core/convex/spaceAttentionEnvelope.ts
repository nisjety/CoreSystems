/**
 * The Space event envelope the attention ingest accepts (WS11-S6).
 *
 * Pure: no Convex imports, so it is tested through
 * `require('../convex/spaceAttentionEnvelope.ts')` like the other rule files.
 *
 * The envelope is produced by Session Core's `space_attention_to_outbox`
 * trigger (migration 0080) and carried on `mp.v1.space.<space_ref>.event`. It
 * holds ids and closed words only. Validation here is strict and fail-closed:
 * anything that is not exactly the shape Session Core writes is rejected, and a
 * rejected event is acknowledged (never retried forever) and never reaches a
 * table. Nothing in an event is an authorization input: the audience, the
 * membership and the level are all re-read from Convex rows when it is applied.
 */

export const SPACE_EVENT_TYPES = {
  SPACE_MESSAGE_APPENDED: "message.appended",
  SPACE_RUN_STATE_CHANGED: "run.state_changed",
  SPACE_THREAD_DELETED: "thread.deleted",
} as const;
export type SpaceEventType = keyof typeof SPACE_EVENT_TYPES;
export type SpaceEventKind = (typeof SPACE_EVENT_TYPES)[SpaceEventType];

export const MAX_EVENT_ID_CHARS = 128;
export const MAX_MENTIONS = 10;
export const MAX_STATE_CHARS = 40;

export interface ParsedSpaceEvent {
  readonly eventId: string;
  readonly orgId: string;
  readonly spaceRef: string;
  readonly kind: SpaceEventKind;
  readonly threadId: string;
  readonly messageId?: string;
  /** `user` or `assistant` for a message event. */
  readonly role?: "user" | "assistant";
  readonly authorSubjectId: string;
  readonly threadOwnerSubjectId: string;
  readonly mentions: readonly string[];
  readonly runId?: string;
  readonly state?: string;
  readonly starterSubjectId: string;
  /** Not carried by Session Core today; read when a later version adds it. */
  readonly agentId?: string;
}

export type ParseResult =
  | { readonly ok: true; readonly event: ParsedSpaceEvent }
  | { readonly ok: false; readonly reason: string };

const REJECT = (reason: string): ParseResult => ({ ok: false, reason });

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** A bounded id: non-empty, trimmed, no control characters. */
function idOrNull(value: unknown): string | null {
  if (typeof value !== "string") return null;
  if (value.length === 0 || value.length > MAX_EVENT_ID_CHARS) return null;
  if (value !== value.trim()) return null;
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f]/.test(value)) return null;
  return value;
}

/** An optional id: absent or empty is fine, anything else must be a valid id. */
function optionalId(value: unknown): { ok: true; value: string } | { ok: false } {
  if (value === undefined || value === null || value === "") return { ok: true, value: "" };
  const id = idOrNull(value);
  return id === null ? { ok: false } : { ok: true, value: id };
}

/** The ref becomes one subject token; the same shape Session Core's trigger accepts. */
export function isSubjectSafeSpaceRef(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 128 && /^[^.*>\s]+$/.test(value);
}

export function parseSpaceEvent(raw: unknown): ParseResult {
  if (!isRecord(raw)) return REJECT("not_an_object");
  if (raw.schema_version !== 1) return REJECT("schema_version");
  // Strictly false: a missing, null or truthy value is refused (ZDR fails closed).
  if (raw.zdr !== false) return REJECT("zdr");
  if (raw.producer !== "session-core") return REJECT("producer");

  const eventType = raw.event_type;
  if (typeof eventType !== "string" || !Object.prototype.hasOwnProperty.call(SPACE_EVENT_TYPES, eventType)) {
    return REJECT("event_type");
  }
  const kind = SPACE_EVENT_TYPES[eventType as SpaceEventType];

  const eventId = idOrNull(raw.event_id);
  if (!eventId) return REJECT("event_id");
  const orgId = idOrNull(raw.org_id);
  if (!orgId) return REJECT("org_id");

  const payload = raw.payload;
  if (!isRecord(payload)) return REJECT("payload");
  if (payload.kind !== kind) return REJECT("kind_mismatch");
  const spaceRef = payload.space_ref;
  if (!isSubjectSafeSpaceRef(spaceRef)) return REJECT("space_ref");
  if (raw.resource_ref !== `space:${spaceRef}`) return REJECT("resource_ref");
  const threadId = idOrNull(payload.thread_id);
  if (!threadId) return REJECT("thread_id");

  const base = { eventId, orgId, spaceRef, kind, threadId };

  if (kind === "message.appended") {
    const messageId = idOrNull(payload.message_id);
    if (!messageId) return REJECT("message_id");
    const role = payload.role;
    if (role !== "user" && role !== "assistant") return REJECT("role");
    const author = optionalId(payload.author_subject_id);
    const owner = optionalId(payload.thread_owner_subject_id);
    if (!author.ok || !owner.ok) return REJECT("subject_id");
    const rawMentions = payload.mentions === undefined ? [] : payload.mentions;
    if (!Array.isArray(rawMentions) || rawMentions.length > MAX_MENTIONS) return REJECT("mentions");
    const mentions: string[] = [];
    for (const entry of rawMentions) {
      const id = idOrNull(entry);
      if (!id) return REJECT("mentions");
      if (!mentions.includes(id)) mentions.push(id);
    }
    return {
      ok: true,
      event: {
        ...base,
        messageId,
        role,
        authorSubjectId: author.value,
        threadOwnerSubjectId: owner.value,
        mentions,
        starterSubjectId: "",
      },
    };
  }

  if (kind === "run.state_changed") {
    const runId = idOrNull(payload.run_id);
    if (!runId) return REJECT("run_id");
    const state = payload.state;
    if (typeof state !== "string" || state.length === 0 || state.length > MAX_STATE_CHARS) {
      return REJECT("state");
    }
    const starter = optionalId(payload.starter_subject_id);
    const agent = optionalId(payload.agent_id);
    if (!starter.ok || !agent.ok) return REJECT("subject_id");
    return {
      ok: true,
      event: {
        ...base,
        runId,
        state,
        authorSubjectId: "",
        threadOwnerSubjectId: "",
        mentions: [],
        starterSubjectId: starter.value,
        ...(agent.value ? { agentId: agent.value } : {}),
      },
    };
  }

  // thread.deleted: ids only.
  return {
    ok: true,
    event: {
      ...base,
      authorSubjectId: "",
      threadOwnerSubjectId: "",
      mentions: [],
      starterSubjectId: "",
    },
  };
}

export type RecipientKind = "mention" | "reply";

/**
 * Who a user message is for, before the delivery rules: mentioned people get a
 * `mention` (which wins over `reply`), thread followers and the thread owner
 * get a `reply`; the actor is never a recipient and nobody appears twice.
 * Order is stable: mentions first, then followers, then the owner.
 */
export function planMessageRecipients(input: {
  readonly actor: string;
  readonly mentions: readonly string[];
  readonly followers: readonly string[];
  readonly owner: string;
}): { readonly recipient: string; readonly kind: RecipientKind }[] {
  const out: { recipient: string; kind: RecipientKind }[] = [];
  const seen = new Set<string>();
  const add = (recipient: string, kind: RecipientKind) => {
    if (!recipient || recipient === input.actor || seen.has(recipient)) return;
    seen.add(recipient);
    out.push({ recipient, kind });
  };
  for (const id of input.mentions) add(id, "mention");
  for (const id of input.followers) add(id, "reply");
  add(input.owner, "reply");
  return out;
}
