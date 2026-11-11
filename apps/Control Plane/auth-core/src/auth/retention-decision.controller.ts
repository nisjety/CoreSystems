import {
  Body,
  Controller,
  Headers as NestHeaders,
  HttpException,
  Logger,
  Post,
  Req,
  Res,
} from '@nestjs/common';
import type { Request, Response } from 'express';
import { randomUUID } from 'node:crypto';

import { DirectNatsService } from '../nats/direct-nats.service';
import { auth } from './auth';
import {
  ConvexTokenService,
  PlaneTokenVerificationError,
  type VerifiedPlaneServicePrincipal,
} from './convex-token.service';
import {
  type CanonicalTokenContext,
  resolveCanonicalTokenContext,
} from './plane-token-membership';
import {
  RetentionDecisionService,
  retentionDecisionRefusal,
  retentionDecisionRefusalStatus,
  type RetentionDecisionAuditFacts,
  type RetentionDecisionOutcome,
  type RetentionDecisionRefusalCode,
} from './retention-decision.service';

/**
 * POST /api/retention/zdr-decision (ZDR.md section 5.3, ZDR task Z-14).
 *
 * The decision endpoint for a per-connection ZDR waiver. It answers only a
 * service-attested caller that also carries the user's own session, and it
 * evaluates the matrix rule in RetentionDecisionService. It never writes the
 * organisation's setting, never signs for a service principal, and stores
 * nothing. Dark: with ZDR_WAIVER_ENABLED off the endpoint still answers (the
 * BFF gets the typed `zdr_connection_not_supported`) and never signs.
 *
 * Caller authentication has two factors:
 *  1. `Authorization: Bearer` is a control-policy plane token for the
 *     registered `verevon-gateway-retention-decision` principal with the
 *     `retention:zdr-decide` scope, minted for ONE org.
 *  2. The user's Better Auth cookie. The org is the session's active org from
 *     the canonical membership read, and it must equal the policy token's org
 *     or the request is a 403 before anything is read.
 */

/** Subject of the one principal allowed to ask (registry key, `service:` form). */
const EXPECTED_CALLER_SUBJECT = 'service:verevon-gateway-retention-decision';
const REQUIRED_SCOPE = 'retention:zdr-decide';
const MAX_ID_LENGTH = 128;
const REQUEST_KEYS: readonly string[] = [
  'connection_id',
  'provider',
  'explicit_zdr',
];

const AUDIT_EVENT = 'retention_zdr_decided';
const AUDIT_SUBJECT = `verevon.audit.v2.control.auth-core.${AUDIT_EVENT}`;

interface AuthSessionSnapshot {
  user?: { id?: string };
  session?: { id?: string; activeOrganizationId?: string };
}

const typedSessionAuth = auth as unknown as {
  api: {
    getSession(input: {
      headers: Headers;
    }): Promise<AuthSessionSnapshot | null>;
  };
};

/** Same conversion as the other session-reading controllers. */
function toWebHeaders(
  source: Record<string, string | string[] | undefined>,
): Headers {
  const headers = new Headers();
  for (const [key, value] of Object.entries(source)) {
    if (typeof value === 'string') {
      headers.set(key, value);
    } else if (Array.isArray(value)) {
      headers.set(key, value.join(', '));
    }
  }
  return headers;
}

type ParsedRequest = Readonly<{
  connectionId: string | null;
  provider: string | null;
  explicitZdr: boolean;
}>;

// eslint-disable-next-line no-control-regex
const CONTROL_CHARACTERS = /[\u0000-\u001f\u007f-\u009f]/;

function isCleanId(value: unknown): value is string {
  return (
    typeof value === 'string' &&
    value.length > 0 &&
    value.length <= MAX_ID_LENGTH &&
    value.trim() === value &&
    !CONTROL_CHARACTERS.test(value)
  );
}

/**
 * The body must be exactly `{ connection_id: string | null, provider:
 * string | null, explicit_zdr: boolean }`; extra or missing fields, wrong
 * types and ids over 128 characters are refused.
 */
function parseRequest(body: unknown): ParsedRequest | null {
  if (typeof body !== 'object' || body === null || Array.isArray(body)) {
    return null;
  }
  const record = body as Record<string, unknown>;
  const keys = Object.keys(record);
  if (
    keys.length !== REQUEST_KEYS.length ||
    !REQUEST_KEYS.every((key) => keys.includes(key))
  ) {
    return null;
  }
  const { connection_id, provider, explicit_zdr } = record;
  if (
    (connection_id !== null && !isCleanId(connection_id)) ||
    (provider !== null && !isCleanId(provider)) ||
    typeof explicit_zdr !== 'boolean'
  ) {
    return null;
  }
  return {
    connectionId: connection_id as string | null,
    provider: provider as string | null,
    explicitZdr: explicit_zdr,
  };
}

function errorBody(code: string, message: string, status: number) {
  return new HttpException({ error: { code, message } }, status);
}

@Controller('api/retention')
export class RetentionDecisionController {
  private readonly logger = new Logger(RetentionDecisionController.name);

  constructor(
    private readonly convexTokenService: ConvexTokenService,
    private readonly decisions: RetentionDecisionService,
    private readonly directNats: DirectNatsService,
  ) {}

  @Post('zdr-decision')
  async decide(
    @NestHeaders('authorization') authorization: string | undefined,
    @Req() request: Request,
    @Res({ passthrough: true }) response: Response,
    @Body() body: unknown,
  ) {
    response.setHeader('Cache-Control', 'no-store');

    // Factor one: the registered BFF principal, minted for one org.
    const principal = this.authenticateCaller(authorization);

    // Factor two: the user's own session.
    const session = await typedSessionAuth.api.getSession({
      headers: toWebHeaders(request.headers),
    });
    const userId = session?.user?.id;
    if (!userId) {
      throw errorBody('unauthorized', 'Authentication required', 401);
    }
    let context: CanonicalTokenContext | null;
    try {
      context = await resolveCanonicalTokenContext(
        userId,
        session?.session?.activeOrganizationId ?? '',
      );
    } catch {
      throw errorBody(
        'service_unavailable',
        'Canonical membership authority unavailable',
        503,
      );
    }
    const sessionId = session?.session?.id;
    if (!context?.orgId || !sessionId) {
      throw errorBody(
        'forbidden',
        'The session has no active organisation membership',
        403,
      );
    }
    // The policy token was minted for one org; it must be the session's org.
    // Nothing is read before this agrees.
    if (principal.orgId !== context.orgId) {
      throw errorBody(
        'forbidden',
        'Policy caller organisation does not match the session',
        403,
      );
    }

    const parsed = parseRequest(body);
    if (!parsed) {
      throw errorBody(
        'invalid_request',
        'Body must be { connection_id: string | null, provider: string | null, explicit_zdr: boolean }',
        400,
      );
    }

    const outcome = await this.decisions.decide({
      userId,
      orgId: context.orgId,
      sessionId,
      connectionId: parsed.connectionId,
      provider: parsed.provider,
      explicitZdr: parsed.explicitZdr,
    });

    const audited = await this.audit(context.orgId, userId, outcome);

    if (outcome.kind === 'refused') {
      return this.refuse(response, outcome.code);
    }
    if (outcome.decision === 'waived' && !audited) {
      // A waiver the audit trail cannot record is not given.
      this.logger.error(
        'Durable audit unavailable; refusing a waived ZDR decision',
      );
      return this.refuse(response, 'zdr_decision_unavailable');
    }

    return {
      data: {
        decision: outcome.decision,
        reason: outcome.reason,
        org_zdr: outcome.orgZdr,
        effective_zdr: outcome.effectiveZdr,
        retention_class: outcome.retentionClass,
        ...(outcome.signed
          ? {
              decision_token: outcome.signed.token,
              expires_at: outcome.signed.expiresAt,
            }
          : {}),
      },
    };
  }

  private refuse(response: Response, code: RetentionDecisionRefusalCode) {
    const entry = retentionDecisionRefusal(code);
    response.status(retentionDecisionRefusalStatus(code));
    return {
      error: {
        code: entry.code,
        message: entry.message,
        retryable: entry.retryable,
        action: entry.action,
      },
    };
  }

  /**
   * One durable, content-free event per decision: ids and closed codes only,
   * never a prompt, a token, the digest's input or any text. Returns whether
   * the publish was acknowledged.
   */
  private async audit(
    orgId: string,
    userId: string,
    outcome: RetentionDecisionOutcome,
  ): Promise<boolean> {
    const facts: RetentionDecisionAuditFacts = outcome.facts;
    const refused = outcome.kind === 'refused';
    try {
      await this.directNats.publishAuditDurable(AUDIT_SUBJECT, {
        occurred_at: new Date().toISOString(),
        event_id: `retention-zdr-decided:${randomUUID()}`,
        org_id: orgId,
        actor_role: 'user',
        plane: 'control',
        producer: 'auth-core',
        event: AUDIT_EVENT,
        subject: userId,
        resource_id: facts.connectionId,
        outcome: refused ? outcome.code : 'ok',
        details: {
          decision: refused ? 'refuse' : outcome.decision,
          reason: refused ? outcome.code : outcome.reason,
          provider: facts.provider,
          explicit_zdr: facts.explicitZdr,
          waiver_enabled: facts.waiverEnabled,
          ack_state: facts.ackState,
          ack_reason: facts.ackReason,
          ack_id: facts.ackId,
          decision_jti: refused ? '' : (outcome.signed?.jti ?? ''),
        },
      });
      return true;
    } catch {
      // For every outcome but a waiver the answer stands: it does not lower ZDR.
      this.logger.warn('Durable audit unavailable for a ZDR decision event');
      return false;
    }
  }

  private authenticateCaller(
    authorization: string | undefined,
  ): VerifiedPlaneServicePrincipal {
    const match = authorization?.match(
      /^Bearer ([A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+)$/,
    );
    if (!match) {
      throw errorBody(
        'unauthorized',
        'Valid policy caller token required',
        401,
      );
    }

    let principal: VerifiedPlaneServicePrincipal;
    try {
      principal = this.convexTokenService.verifyPlaneServiceToken(
        'control-policy',
        match[1],
      );
    } catch (error) {
      if (error instanceof PlaneTokenVerificationError) {
        throw errorBody(
          'unauthorized',
          'Valid policy caller token required',
          401,
        );
      }
      throw errorBody(
        'service_unavailable',
        'Policy caller verification is unavailable',
        503,
      );
    }

    if (principal.subject !== EXPECTED_CALLER_SUBJECT) {
      throw errorBody('forbidden', 'Policy caller is not authorized', 403);
    }
    if (!principal.scopes.includes(REQUIRED_SCOPE)) {
      throw errorBody(
        'forbidden',
        'Policy caller scope is not authorized',
        403,
      );
    }
    return principal;
  }
}
