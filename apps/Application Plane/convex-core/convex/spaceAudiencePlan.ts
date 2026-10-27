/**
 * Pure planning for the recipient-audience reconcile (WS04-S5).
 *
 * Control owns who is a member; Application owns the versioned recipient
 * snapshot that Control registers and later checks every shared decision
 * against. This module only decides what to do given Control's current
 * candidates and Application's active snapshot. It performs no I/O so it can
 * be tested without Convex.
 */
import { compareCodePoints } from "./spaceAudience";

/** Control's recipient-audience contract holds at most this many people. */
export const MAX_AUDIENCE_RECIPIENTS = 512;

export type ActiveAudienceForPlan = {
  revision: number;
  controlState: "pending" | "acknowledged" | "rejected";
  recipientExternalAuthIds: readonly string[];
} | null;

export type AudiencePlan =
  | { action: "noop" }
  | { action: "replace"; recipients: string[]; expectedRevision: number | undefined }
  | { action: "wait" }
  | { action: "refuse"; reason: "empty_candidates" | "audience_too_large" };

/** Trimmed, de-duplicated, in code point order (the canonical order Control hashes). */
export function normalizeAudienceCandidates(candidates: readonly unknown[]): string[] {
  const seen = new Set<string>();
  for (const candidate of candidates) {
    if (typeof candidate !== "string") continue;
    const id = candidate.trim();
    if (id) seen.add(id);
  }
  return Array.from(seen).sort(compareCodePoints);
}

function sameRecipients(left: readonly string[], right: readonly string[]): boolean {
  if (left.length !== right.length) return false;
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) return false;
  }
  return true;
}

/**
 * - `refuse`: no candidates (an empty audience is not representable, and an
 *   unreadable roster must never empty one) or more than 512; the last
 *   registered audience stays in force.
 * - `noop`: the active audience already equals the candidates and Control has
 *   not rejected it.
 * - `wait`: the candidates differ but the active audience has not been
 *   acknowledged by Control yet; replacing it would supersede a snapshot Control
 *   may still be receiving. The next trigger or the cron retries.
 * - `replace`: register a new revision, compare-and-set on the active revision
 *   (`undefined` when there is no audience yet). A rejected active audience is
 *   replaced even when the recipients are equal, because it is not in force.
 */
export function planAudienceReconcile(input: {
  candidates: readonly unknown[];
  active: ActiveAudienceForPlan;
}): AudiencePlan {
  const recipients = normalizeAudienceCandidates(input.candidates);
  if (recipients.length === 0) return { action: "refuse", reason: "empty_candidates" };
  if (recipients.length > MAX_AUDIENCE_RECIPIENTS) {
    return { action: "refuse", reason: "audience_too_large" };
  }
  const active = input.active;
  if (!active) return { action: "replace", recipients, expectedRevision: undefined };
  const activeRecipients = normalizeAudienceCandidates(active.recipientExternalAuthIds);
  if (sameRecipients(activeRecipients, recipients) && active.controlState !== "rejected") {
    return { action: "noop" };
  }
  if (active.controlState === "pending") return { action: "wait" };
  return { action: "replace", recipients, expectedRevision: active.revision };
}
