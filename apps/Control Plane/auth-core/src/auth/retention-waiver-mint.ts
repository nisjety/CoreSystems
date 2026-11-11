import { HttpException, type Logger } from '@nestjs/common';
import { randomUUID } from 'node:crypto';

import type { DirectNatsService } from '../nats/direct-nats.service';
import {
  isWaiverAudience,
  type AuthCoreAudience,
} from './convex-token.service';
import {
  retentionDecisionRefusal,
  retentionDecisionRefusalStatus,
  type RetentionDecisionRefusalCode,
} from './retention-decision.service';
import {
  RetentionDecisionUnavailableError,
  type RetentionDecisionSigner,
  type VerifiedRetentionDecision,
} from './retention-decision.signer';

/**
 * Mint side of the per-connection ZDR waiver (ZDR.md 5.3, ZDR-Z-14b).
 *
 * The two user token routes (`GET /api/model-plane/token` and
 * `GET /api/:audience/token`) read the header `x-retention-decision`, check the
 * signed decision against the cookie session, and pass the verified decision to
 * the issuer, which stamps the same `zdr_waiver` claim on every waiver audience
 * of the turn. The service routes (internal-token, token-exchange,
 * api-key-exchange) never call this module. Nothing here stores anything; the
 * audit event is content-free.
 */

/** Request header carrying the opaque signed decision the BFF relays. */
export const RETENTION_DECISION_HEADER = 'x-retention-decision';

/** Longest accepted header value, in characters. */
export const RETENTION_DECISION_HEADER_MAX_LENGTH = 4096;

/** Durable audit event for one waiver token mint. */
export const WAIVER_TOKEN_AUDIT_EVENT = 'retention_waiver_token_issued';

/**
 * A refusal in the catalogue envelope `{ error: { code, message, retryable,
 * action } }`, with the catalogue's status (503 for `zdr_decision_unavailable`,
 * else 412), so the BFF can relay it as it is.
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

/** What the cookie session says; the decision must agree with all three. */
export type WaiverSessionFacts = Readonly<{
  userId: string;
  /** Better Auth session id. */
  sessionId: string | undefined;
  /** The session's active organization, from the canonical membership read. */
  orgId: string;
}>;

/**
 * Reads and verifies the `x-retention-decision` header for one user mint.
 *
 * - Header absent: returns `undefined`; the mint is unchanged.
 * - Present on `browser-broker` or `sandbox-manager` (or any audience that is
 *   not a waiver audience): `zdr_waiver_route_mismatch`, nothing minted.
 * - Present, but the kill switch is off or the signer is unavailable:
 *   `zdr_decision_unavailable`.
 * - Present, but oversized, repeated, unverifiable, expired, or bound to a
 *   different session, user or organization: `zdr_posture_mismatch`.
 *
 * Throws {@link RetentionWaiverRefusal}; never returns a decision that was not
 * verified by the signer.
 */
export function resolveRetentionWaiver(input: {
  header: string | string[] | undefined;
  audience: AuthCoreAudience;
  signer: RetentionDecisionSigner | undefined;
  waiverEnabled: boolean;
  session: WaiverSessionFacts;
}): VerifiedRetentionDecision | undefined {
  const { header, audience, signer, waiverEnabled, session } = input;
  if (header === undefined) return undefined;

  if (!isWaiverAudience(audience)) {
    throw new RetentionWaiverRefusal('zdr_waiver_route_mismatch');
  }
  if (!waiverEnabled || !signer || !signer.available()) {
    throw new RetentionWaiverRefusal('zdr_decision_unavailable');
  }
  if (
    typeof header !== 'string' ||
    header.length === 0 ||
    header.length > RETENTION_DECISION_HEADER_MAX_LENGTH
  ) {
    throw new RetentionWaiverRefusal('zdr_posture_mismatch');
  }

  let decision: VerifiedRetentionDecision;
  try {
    decision = signer.verify(header);
  } catch (error) {
    if (error instanceof RetentionDecisionUnavailableError) {
      throw new RetentionWaiverRefusal('zdr_decision_unavailable');
    }
    throw new RetentionWaiverRefusal('zdr_posture_mismatch');
  }

  if (
    !session.sessionId ||
    decision.sessionId !== session.sessionId ||
    decision.userId !== session.userId ||
    decision.orgId !== session.orgId
  ) {
    throw new RetentionWaiverRefusal('zdr_posture_mismatch');
  }
  return decision;
}

/**
 * A waiver only lowers a ZDR organization. If the organization is no longer ZDR
 * the decision is stale for this mint: refuse, never mint a token that mixes the
 * two postures.
 */
export function assertWaiverOrgPosture(posture: { zdr: boolean }): void {
  if (posture.zdr !== true) {
    throw new RetentionWaiverRefusal('zdr_posture_mismatch');
  }
}

/**
 * Publishes the durable, content-free `retention_waiver_token_issued` event
 * (subject the user, resource the audience) before the token is returned.
 * Throws `zdr_decision_unavailable` when the publish is not acknowledged, so
 * the caller returns no token.
 */
export async function auditRetentionWaiverToken(
  directNats: DirectNatsService,
  logger: Pick<Logger, 'error'>,
  input: {
    orgId: string;
    userId: string;
    audience: AuthCoreAudience;
    decision: VerifiedRetentionDecision;
    ttlSeconds: number;
  },
): Promise<void> {
  try {
    await directNats.publishAuditDurable(
      `verevon.audit.v2.control.auth-core.${WAIVER_TOKEN_AUDIT_EVENT}`,
      {
        occurred_at: new Date().toISOString(),
        event_id: `retention-waiver-token-issued:${randomUUID()}`,
        org_id: input.orgId,
        actor_role: 'user',
        plane: 'control',
        producer: 'auth-core',
        event: WAIVER_TOKEN_AUDIT_EVENT,
        subject: input.userId,
        resource_id: input.audience,
        outcome: 'ok',
        details: {
          connection: input.decision.connection,
          ack_id: input.decision.ackId,
          decision_jti: input.decision.jti,
          ttl_seconds: input.ttlSeconds,
        },
      },
    );
  } catch {
    logger.error(
      'Durable audit unavailable; refusing a waiver token issuance',
    );
    throw new RetentionWaiverRefusal('zdr_decision_unavailable');
  }
}
