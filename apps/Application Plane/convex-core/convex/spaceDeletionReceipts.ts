/**
 * Deletion receipt aggregation is intentionally conservative. A timeout or
 * missing owner response never becomes success; it remains pending/unknown
 * until a named owner provides an idempotent receipt or an operator resolves
 * the exception.
 */
export const SPACE_DELETION_OWNERS = [
  "application",
  "control",
  "data",
  "ingestion",
  "model",
  "infra",
] as const;

export type SpaceDeletionOwner = (typeof SPACE_DELETION_OWNERS)[number];
export type SpaceDeletionOwnerStatus = "pending" | "blocked_legal_hold" | "succeeded" | "partial" | "failed" | "unknown";
export type SpaceDeletionAggregateStatus = SpaceDeletionOwnerStatus;

export type SpaceDeletionReceipt = Readonly<{
  ownerPlane: SpaceDeletionOwner;
  status: SpaceDeletionOwnerStatus;
}>;

const TRANSITIONS: Readonly<Record<SpaceDeletionOwnerStatus, readonly SpaceDeletionOwnerStatus[]>> = {
  pending: ["blocked_legal_hold", "succeeded", "partial", "failed", "unknown"],
  blocked_legal_hold: [],
  succeeded: [],
  partial: ["succeeded", "failed", "unknown"],
  failed: ["succeeded", "partial", "unknown"],
  unknown: ["succeeded", "partial", "failed", "blocked_legal_hold"],
};

/**
 * Flag `SPACE_DELETION_HOLD_RELEASE` (Convex env, read at call time, off by
 * default). Only the literal "true" turns it on. With it off a legal-hold block
 * is terminal, exactly as before the flag existed.
 */
export function legalHoldReleaseEnabled(): boolean {
  return process.env.SPACE_DELETION_HOLD_RELEASE === "true";
}

/**
 * Flag `SPACE_DELETION_PERMANENT_REFUSAL_PARK` (Convex env, read at call time, off
 * by default). Only the literal "true" turns it on. With it off a permanent owner
 * refusal (400, 401, 403, 412) releases the delivery for a retry like any other
 * non-200, so a bearer that is fixed later still lets the deletion complete. With
 * it on such a refusal parks the delivery as `failed_permanent`; the trusted ingress
 * re-arms it with `rearmParkedDeletionOwnerDelivery` once the bearer is fixed.
 */
export function permanentRefusalParkEnabled(): boolean {
  return process.env.SPACE_DELETION_PERMANENT_REFUSAL_PARK === "true";
}

// Owner answers a blocked receipt may take once its hold is lifted. `pending` is
// deliberately absent: a released receipt goes back to pending only through the
// re-drive (`assertReceiptRearmAfterLegalHold`), which also re-arms the delivery.
const LEGAL_HOLD_RELEASE_ANSWERS: readonly SpaceDeletionOwnerStatus[] = ["succeeded", "partial", "failed", "unknown"];

export function assertReceiptTransition(
  from: SpaceDeletionOwnerStatus,
  to: SpaceDeletionOwnerStatus,
  options: Readonly<{ legalHoldRelease?: boolean }> = {},
): void {
  if (from === to) return;
  if (TRANSITIONS[from].includes(to)) return;
  if (options.legalHoldRelease === true && from === "blocked_legal_hold" && LEGAL_HOLD_RELEASE_ANSWERS.includes(to)) return;
  throw new Error(`invalid Space deletion owner receipt transition: ${from} -> ${to}`);
}

/**
 * The one edge out of a legal-hold block that is not an owner's answer: a
 * re-drive after the hold is lifted returns the receipt to `pending`, in the same
 * mutation that re-arms the owner's delivery.
 */
export function assertReceiptRearmAfterLegalHold(from: SpaceDeletionOwnerStatus): void {
  if (from !== "blocked_legal_hold") {
    throw new Error(`invalid Space deletion owner receipt transition: ${from} -> pending`);
  }
}

/**
 * A legal hold parks an owner's delivery only once that owner's own adapter has
 * received the typed refusal (the Model, Data and Ingestion adapters alike). The
 * trusted coordinator ingress may record `blocked_legal_hold` for the Model owner
 * alone, and only on a Model delivery that is already parked (see
 * `requireParkedModelDelivery` in spaces.ts). A receipt the ingress blocks for any
 * other owner has no parked delivery behind it, so that owner's live delivery
 * would later answer over a blocked receipt, which no transition allows.
 */
export function assertCoordinatorLegalHold(ownerPlane: SpaceDeletionOwner, to: SpaceDeletionOwnerStatus): void {
  if (to === "blocked_legal_hold" && ownerPlane !== "model") {
    throw new Error(`invalid Space deletion owner receipt transition: ${ownerPlane} owner may not record blocked_legal_hold`);
  }
}

export function aggregateDeletionReceipts(receipts: readonly SpaceDeletionReceipt[]): SpaceDeletionAggregateStatus {
	if (receipts.some((receipt) => receipt.status === "blocked_legal_hold")) return "blocked_legal_hold";
	if (receipts.length === 0 || receipts.some((receipt) => receipt.status === "pending")) return "pending";
  if (receipts.some((receipt) => receipt.status === "unknown")) return "unknown";
  const succeeded = receipts.filter((receipt) => receipt.status === "succeeded").length;
  const failed = receipts.filter((receipt) => receipt.status === "failed").length;
  const partial = receipts.filter((receipt) => receipt.status === "partial").length;
  if (succeeded === receipts.length) return "succeeded";
  if (succeeded > 0 && (failed > 0 || partial > 0)) return "partial";
  if (partial > 0) return "partial";
  return "failed";
}

export function receiptDeadlineHasElapsed(status: SpaceDeletionOwnerStatus, deadlineAt: number, now: number): boolean {
  return status === "pending" && Number.isSafeInteger(deadlineAt) && deadlineAt > 0 && now >= deadlineAt;
}

/**
 * The typed legal-hold refusal an owner returns for an erase that a legal hold
 * covers: the exact code with `details.retryable: false`. Only those fields are
 * read. The upstream message is never read or stored, and any other body is not a hold.
 */
export function isOwnerLegalHoldRefusal(body: unknown): boolean {
  if (!body || typeof body !== "object") return false;
  const error = (body as { error?: unknown }).error;
  if (!error || typeof error !== "object") return false;
  const { code, details } = error as { code?: unknown; details?: unknown };
  if (code !== "erasure_blocked_legal_hold") return false;
  return Boolean(details && typeof details === "object" && (details as { retryable?: unknown }).retryable === false);
}

/**
 * Statuses an owner returns when a retry cannot succeed: a bad request (400), a
 * bad bearer (401, 403) or a precondition the owner will not meet (412). With
 * SPACE_DELETION_PERMANENT_REFUSAL_PARK on, such a delivery parks as
 * `failed_permanent` instead of retrying without limit; with it off each one
 * releases for a retry as before. Every other non-200 (a 409 that is not a hold,
 * 429, 5xx, a timeout) stays retryable.
 */
const PERMANENT_OWNER_REFUSAL_STATUSES: readonly number[] = [400, 401, 403, 412];

export function isPermanentOwnerRefusal(status: number): boolean {
  return PERMANENT_OWNER_REFUSAL_STATUSES.includes(status);
}
