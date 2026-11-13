import { HttpException } from '@nestjs/common';

import zdrRefusalsFixture from './fixtures/zdr-refusals.json';

/**
 * The refusal catalogue of the per-connection ZDR waiver (ZDR.md 5.3,
 * ZDR-Z-14b), and the typed refusal the waiver mint and the token issuer throw.
 *
 * This module imports nothing from the signer, the decision service or the
 * issuer on purpose. The issuer (convex-token.service.ts) throws
 * {@link RetentionWaiverRefusal} for a waiver it will not sign, and the signer
 * imports the issuer for its Nest provider, so a dependency from here back to
 * either would be a load-order cycle. The catalogue is the vendored
 * `fixtures/zdr-refusals.json`, a byte copy of
 * apps/Model Plane/api/contracts/zdr-refusals.json.
 */

/** Refusal codes this endpoint can answer (a subset of the Z-05a catalogue). */
export type RetentionDecisionRefusalCode =
  | 'zdr_connection_not_acknowledged'
  | 'zdr_connection_not_supported'
  | 'zdr_explicit_request_not_supported_by_connection'
  | 'zdr_decision_unavailable'
  | 'zdr_waiver_route_mismatch'
  | 'zdr_posture_mismatch';

const REFUSAL_CODES: readonly RetentionDecisionRefusalCode[] = [
  'zdr_connection_not_acknowledged',
  'zdr_connection_not_supported',
  'zdr_explicit_request_not_supported_by_connection',
  'zdr_decision_unavailable',
  'zdr_waiver_route_mismatch',
  'zdr_posture_mismatch',
];

/** What the client is told about one refusal, read from the vendored catalogue. */
export type RetentionDecisionRefusal = Readonly<{
  code: RetentionDecisionRefusalCode;
  message: string;
  retryable: boolean;
  action: string;
}>;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * Builds the refusal table from the vendored catalogue
 * (`fixtures/zdr-refusals.json`, a byte copy of
 * apps/Model Plane/api/contracts/zdr-refusals.json). A code missing from the
 * catalogue, or an entry of the wrong shape, throws: the service must not boot
 * with a refusal it cannot describe.
 */
export function loadRetentionDecisionRefusals(
  catalogue: unknown,
): ReadonlyMap<RetentionDecisionRefusalCode, RetentionDecisionRefusal> {
  const codes =
    isRecord(catalogue) && Array.isArray(catalogue.codes)
      ? (catalogue.codes as unknown[])
      : [];
  const table = new Map<
    RetentionDecisionRefusalCode,
    RetentionDecisionRefusal
  >();
  for (const code of REFUSAL_CODES) {
    const entry = codes.find((item) => isRecord(item) && item.code === code);
    if (
      !isRecord(entry) ||
      typeof entry.message_en !== 'string' ||
      entry.message_en.length === 0 ||
      typeof entry.retryable !== 'boolean' ||
      typeof entry.action !== 'string' ||
      entry.action.length === 0
    ) {
      throw new Error(`zdr-refusals catalogue has no usable entry for ${code}`);
    }
    table.set(code, {
      code,
      message: entry.message_en,
      retryable: entry.retryable,
      action: entry.action,
    });
  }
  return table;
}

const REFUSALS = loadRetentionDecisionRefusals(zdrRefusalsFixture);

/** The catalogue entry for a refusal code. */
export function retentionDecisionRefusal(
  code: RetentionDecisionRefusalCode,
): RetentionDecisionRefusal {
  return REFUSALS.get(code) as RetentionDecisionRefusal;
}

/**
 * HTTP status of a refusal: 503 for `zdr_decision_unavailable`, 412 for every
 * other code (the model-gateway http_status rule).
 */
export function retentionDecisionRefusalStatus(
  code: RetentionDecisionRefusalCode,
): number {
  return code === 'zdr_decision_unavailable' ? 503 : 412;
}

/**
 * A refusal in the catalogue envelope `{ error: { code, message, retryable,
 * action } }`, with the catalogue's status (503 for `zdr_decision_unavailable`,
 * else 412), so the BFF can relay it as it is. Thrown by the waiver mint and by
 * the token issuer, so a refused waiver is never an untyped 500.
 */
export class RetentionWaiverRefusal extends HttpException {
  constructor(readonly code: RetentionDecisionRefusalCode) {
    const entry = retentionDecisionRefusal(code);
    super(
      {
        error: {
          code: entry.code,
          message: entry.message,
          retryable: entry.retryable,
          action: entry.action,
        },
      },
      retentionDecisionRefusalStatus(code),
    );
  }
}
