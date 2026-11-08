/**
 * Direct-message identity (WS12-S2).
 *
 * A DM is identified by the set of people in it, not by a name or a creator:
 * the same set always resolves to the one live DM of an organization, whoever
 * opens it. This module is the Application twin of Control's
 * `user-core/internal/spaces/dm_participants.go`; the shared vector file
 * `user-core/internal/spaces/testdata/participant-set-vectors.v1.json` pins the
 * encoding for both planes. It commits to Better Auth ids only and never
 * accepts names or emails. Pure: no database, no environment except the one
 * flag reader at the bottom.
 */
import { compareCodePoints } from './spaceAudience';

export const MIN_DM_PARTICIPANTS = 2;
export const MAX_DM_PARTICIPANTS = 9;

// Its own domain (never "recipient-audience/v1") so a participant hash can
// never equal an audience hash over the same ids.
const PARTICIPANT_SET_DOMAIN = 'participant-set/v1\0';
export const PARTICIPANT_HASH_PREFIX = 'sha256:';

export type DmKind = 'dm' | 'group_dm';

/** Closed reasons a participant set is refused (same words as Control). */
export type DmParticipantReason = 'count' | 'duplicate' | 'empty';

export class DmParticipantError extends Error {
  readonly reason: DmParticipantReason;
  constructor(reason: DmParticipantReason) {
    super(`direct message participants are invalid:${reason}`);
    this.name = 'DmParticipantError';
    this.reason = reason;
  }
}

export function isDmKind(kind: unknown): kind is DmKind {
  return kind === 'dm' || kind === 'group_dm';
}

/** Two people is a `dm`, three to nine a `group_dm`. */
export function dmKindForCount(count: number): DmKind {
  if (count === MIN_DM_PARTICIPANTS) return 'dm';
  if (count > MIN_DM_PARTICIPANTS && count <= MAX_DM_PARTICIPANTS) return 'group_dm';
  throw new DmParticipantError('count');
}

/**
 * Trim each id and return them in byte (code point) order. Refuses an empty id,
 * a duplicate, or a count outside 2..9.
 */
export function canonicalParticipants(ids: readonly string[]): string[] {
  if (ids.length < MIN_DM_PARTICIPANTS || ids.length > MAX_DM_PARTICIPANTS) {
    throw new DmParticipantError('count');
  }
  const canonical = ids.map((id) => id.trim());
  if (canonical.some((id) => !id)) throw new DmParticipantError('empty');
  canonical.sort(compareCodePoints);
  if (canonical.some((id, index) => id === canonical[index - 1])) {
    throw new DmParticipantError('duplicate');
  }
  return canonical;
}

function encodeUint64(value: number): Uint8Array {
  const output = new Uint8Array(8);
  const view = new DataView(output.buffer);
  view.setUint32(0, Math.floor(value / 2 ** 32));
  view.setUint32(4, value >>> 0);
  return output;
}

/**
 * `"sha256:" + hex(SHA-256("participant-set/v1\0" || for each id in sorted
 * unique order: uint64be(len(id)) || id))`, equal to Control's
 * `ParticipantSetHash`. The kind is not an input: it follows from the count.
 */
export async function participantSetHash(ids: readonly string[]): Promise<string> {
  const encoder = new TextEncoder();
  const canonical = canonicalParticipants(ids);
  const parts: Uint8Array[] = [encoder.encode(PARTICIPANT_SET_DOMAIN)];
  for (const id of canonical) {
    const bytes = encoder.encode(id);
    parts.push(encodeUint64(bytes.length), bytes);
  }
  const length = parts.reduce((total, part) => total + part.length, 0);
  const payload = new ArrayBuffer(length);
  const view = new Uint8Array(payload);
  let offset = 0;
  for (const part of parts) {
    view.set(part, offset);
    offset += part.length;
  }
  const digest = await crypto.subtle.digest('SHA-256', payload);
  return `${PARTICIPANT_HASH_PREFIX}${Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('')}`;
}

/**
 * Flag `SPACE_DM_OPEN` (Convex env, set by startup.sh, off by default).
 * Read at call time so `docker restart convex-gateway` is the whole switch. A
 * rollout control only: never an authorization input; Control's
 * `dm_create_entitled` and eligibility decide whether a DM may exist.
 */
export function dmOpenEnabled(): boolean {
  const raw = (process.env.SPACE_DM_OPEN || '').trim().toLowerCase();
  return raw === '1' || raw === 'true';
}
