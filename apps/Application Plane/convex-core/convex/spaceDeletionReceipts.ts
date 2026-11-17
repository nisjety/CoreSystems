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
