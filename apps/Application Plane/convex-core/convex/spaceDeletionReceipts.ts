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
  unknown: ["succeeded", "partial", "failed"],
};

export function assertReceiptTransition(from: SpaceDeletionOwnerStatus, to: SpaceDeletionOwnerStatus): void {
  if (from === to) return;
  if (!TRANSITIONS[from].includes(to)) {
    throw new Error(`invalid Space deletion owner receipt transition: ${from} -> ${to}`);
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
