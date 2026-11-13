import type {
  ProofApproval,
  ProofBundle,
  ProofExecution,
  ProofOutcome,
  ProofRunSummary,
  ProofUnavailableSection,
  ProofVerification,
} from '@/shared/api/run-console-client'

// ── Run support bundle (W5-096-b) ───────────────────────────────────────────
// The JSON file a person downloads from the run console and sends to support. It is built from the proof
// bundle already on screen, so it makes no request of its own, and it is written to be shared:
//
//   * Free text never goes into the file. The run's goal, an approval's decision reason and a verification's
//     reason are a person's words or a sentence about one, so they are left out whatever the run kept.
//   * A run that keeps no content (zero retention or temporary) gets ids, statuses and timestamps only. Its
//     closed classifications (approval kind, verification method, failure code, descriptor version) are left
//     out as well, so a key that is absent from such a file was withheld, not unrecorded.
//   * A null link stays null. "No execution recorded", "still in flight" and "not independently verified" are
//     three different answers, and the file keeps them apart just as the proof bundle does.
//
// The builder is a pure function of the bundle and the caller's extras, so it can be checked without the DOM.

/** The format tag every support bundle carries. A reader that does not know it must refuse to interpret the file. */
export const SUPPORT_BUNDLE_FORMAT = 'verevon.run-support.v1'

/** What the builder cannot read from the proof bundle itself. */
export type SupportBundleExtras = {
  /** The run the console is showing. Used only when the bundle does not name its run. */
  runId: string
  /** When the file is assembled, ISO-8601. Passed in so the builder stays a pure function. */
  now: string
  /** True when the run keeps no content. The file then carries ids, statuses and timestamps only. */
  zeroRetention: boolean
  /**
   * Recent failed request ids from the support-correlation ring. Omitted from the file when not supplied, so an
   * absent key means "not collected", never "none failed".
   */
  requestIds?: readonly string[]
}

export type SupportBundleRun = {
  agentId: string | null
  status: string | null
  createdAt: string | null
}

export type SupportBundleVerification = {
  status: string | null
  verifiedAt: string | null
  /** Withheld on a zero-retention run. */
  method?: string | null
}

export type SupportBundleOutcome = {
  outcome: string | null
  providerReceiptId: string | null
  finalizedAt: string | null
  /** Withheld on a zero-retention run. */
  failureCode?: string | null
  /** `null` when no independent verification was recorded. That is neither a failed check nor a passed one. */
  verification: SupportBundleVerification | null
}

export type SupportBundleExecution = {
  receiptId: string | null
  deliveryId: string | null
  actionFingerprint: string | null
  executionServiceId: string | null
  /** Withheld on a zero-retention run. */
  descriptorVersion?: number | null
  startedAt: string | null
  /** `null` while the work is in flight, and never a stand-in for a failure. */
  outcome: SupportBundleOutcome | null
}

export type SupportBundleApproval = {
  approvalId: string | null
  /** Withheld on a zero-retention run. */
  kind?: string | null
  status: string | null
  requestedBy: string | null
  decidedBy: string | null
  requestedAt: string | null
  decidedAt: string | null
  /** `null` when no work was proven to start for this approval. That is not a failed execution. */
  execution: SupportBundleExecution | null
}

export type SupportBundleUnavailable = {
  section: string | null
  reason: string | null
}

export type SupportBundle = {
  format: typeof SUPPORT_BUNDLE_FORMAT
  /** When this file was assembled. */
  generatedAt: string
  runId: string
  /** The proof bundle's own version. */
  bundleVersion: number | null
  /** When the proof bundle was assembled on the server. That is not when the work happened. */
  proofGeneratedAt: string | null
  /** The server's read-only, proposed or effectful classification of the run. It is a status, so every run keeps it. */
  effectClass: string | null
  /** The `x-request-id` of the read that returned the proof bundle. */
  requestId: string | null
  /** Recent failed request ids, present only when the caller supplied them. */
  requestIds?: string[]
  run: SupportBundleRun | null
  approvals: SupportBundleApproval[]
  /** The dimensions the proof bundle declines to claim, each with the server's own reason. */
  unavailable: SupportBundleUnavailable[]
}

/** A value the bundle did not record is `null` in the file, never an invented default. */
function orNull<T>(value: T | undefined): T | null {
  return value === undefined ? null : value
}

function runSummary(run: ProofRunSummary | null): SupportBundleRun | null {
  if (!run) return null
  // `goal` is the run's title as the person wrote it, so it is never carried.
  return {
    agentId: orNull(run.agentId),
    status: orNull(run.status),
    createdAt: orNull(run.createdAt),
  }
}

function verificationSummary(
  verification: ProofVerification | null,
  withClassifications: boolean,
): SupportBundleVerification | null {
  if (!verification) return null
  // `reason` is a sentence written by the verifier, often about the outcome itself, so it is never carried.
  return {
    status: orNull(verification.status),
    verifiedAt: orNull(verification.verifiedAt),
    ...(withClassifications ? { method: orNull(verification.method) } : {}),
  }
}

function outcomeSummary(outcome: ProofOutcome | null, withClassifications: boolean): SupportBundleOutcome | null {
  if (!outcome) return null
  return {
    outcome: orNull(outcome.outcome),
    providerReceiptId: orNull(outcome.providerReceiptId),
    finalizedAt: orNull(outcome.finalizedAt),
    ...(withClassifications ? { failureCode: orNull(outcome.failureCode) } : {}),
    verification: verificationSummary(outcome.verification, withClassifications),
  }
}

function executionSummary(
  execution: ProofExecution | null,
  withClassifications: boolean,
): SupportBundleExecution | null {
  if (!execution) return null
  return {
    receiptId: orNull(execution.receiptId),
    deliveryId: orNull(execution.deliveryId),
    actionFingerprint: orNull(execution.actionFingerprint),
    executionServiceId: orNull(execution.executionServiceId),
    ...(withClassifications ? { descriptorVersion: orNull(execution.descriptorVersion) } : {}),
    startedAt: orNull(execution.startedAt),
    outcome: outcomeSummary(execution.outcome, withClassifications),
  }
}

function approvalSummary(approval: ProofApproval, withClassifications: boolean): SupportBundleApproval {
  return {
    approvalId: orNull(approval.approvalId),
    ...(withClassifications ? { kind: orNull(approval.kind) } : {}),
    status: orNull(approval.status),
    requestedBy: orNull(approval.requestedBy),
    decidedBy: orNull(approval.decidedBy),
    requestedAt: orNull(approval.requestedAt),
    decidedAt: orNull(approval.decidedAt),
    // `decisionReason` is a note a person wrote about the decision, so it is never carried.
    execution: executionSummary(approval.execution, withClassifications),
  }
}

function unavailableSummary(item: ProofUnavailableSection): SupportBundleUnavailable {
  // The reason is the server's own plain-language note on what the bundle does not claim. It names no content,
  // so it is kept on every run.
  return {
    section: orNull(item.section),
    reason: orNull(item.reason),
  }
}

/**
 * The support bundle for one run's proof bundle. Pure: the same bundle and extras always give the same file.
 */
export function buildSupportBundle(bundle: ProofBundle, extras: SupportBundleExtras): SupportBundle {
  const withClassifications = !extras.zeroRetention
  return {
    format: SUPPORT_BUNDLE_FORMAT,
    generatedAt: extras.now,
    runId: bundle.runId ?? extras.runId,
    bundleVersion: orNull(bundle.bundleVersion),
    proofGeneratedAt: orNull(bundle.generatedAt),
    effectClass: orNull(bundle.effectClass),
    requestId: orNull(bundle.requestId),
    ...(extras.requestIds ? { requestIds: [...extras.requestIds] } : {}),
    run: runSummary(bundle.run),
    approvals: bundle.approvals.map((approval) => approvalSummary(approval, withClassifications)),
    unavailable: bundle.unavailable.map(unavailableSummary),
  }
}

/** The download's file name, `run-<id>-support.json`, with any character that is not a plain id character replaced. */
export function supportBundleFileName(runId: string): string {
  return `run-${runId.replace(/[^A-Za-z0-9_.-]/g, '_')}-support.json`
}
