/**
 * EP-12 S2 / INNH-10 CP-1: `POST /api/:audience/api-key-exchange`.
 *
 * The one route that can mint an `api_key` principal token. A registered
 * service (the Verevon gateway, opted in per audience through the registry's
 * `apiKeyExchange` list) presents a caller's Better Auth `user-keys` API key;
 * auth-core verifies the key, the key's org binding, its owner's membership
 * and its permission for the audience's closed scope, then mints one token of
 * at most 120 seconds with a fresh `jti`, `sub = apikey:<key_id>` and the
 * owner as `acting_user_id` only.
 *
 * Dark by default: unless both `API_KEY_ENABLED` and
 * `API_KEY_AGENT_EXCHANGE_ENABLED` are the literal `true`, the route answers
 * exactly as it does for an unknown audience (404).
 *
 * Every grant and every key-level refusal is a durable audit event
 * (`api_key_token_exchanged` / `api_key_token_denied`); a grant whose event
 * cannot be written answers 503 and returns no token. Nothing here logs or
 * audits the key, a key prefix or a minted token.
 */

import {
  Body,
  Controller,
  Headers as NestHeaders,
  HttpException,
  HttpStatus,
  Logger,
  Param,
  Post,
} from '@nestjs/common';
import { ApiOperation, ApiResponse, ApiTags } from '@nestjs/swagger';
import { randomUUID } from 'node:crypto';

import { DirectNatsService } from '../nats/direct-nats.service';
import {
  API_KEY_EXCHANGE_CONFIG_ID,
  apiKeyTokenZdr,
  apiKeyVerificationDenial,
  decideApiKeyExchange,
  isApiKeyExchangeAudience,
  type ApiKeyExchangeAudience,
  type ApiKeyExchangeDenialReason,
  type VerifiedApiKey,
} from './api-key-exchange';
import { issuedTokenAuditIdentity } from './audit-event-identity';
import { auth } from './auth';
import { ConvexTokenService } from './convex-token.service';
import {
  resolveInteractiveRetentionPosture,
  RetentionPostureUnavailableError,
} from './interactive-retention-policy';
import {
  authorizePlaneServiceApiKeyExchange,
  loadPlaneServicePrincipalRegistry,
  ServicePrincipalConfigurationError,
  type AuthorizedPlaneServiceApiKeyExchange,
} from './plane-service-principal';
import {
  resolveCanonicalTokenContext,
  type CanonicalTokenContext,
} from './plane-token-membership';

interface ApiKeyExchangeBody {
  apiKey?: unknown;
  orgId?: unknown;
  scopes?: unknown;
}

/**
 * The fields of Better Auth's `verifyApiKey` answer this route reads. The
 * plugin strips the hashed key before answering; `error.code` names why a key
 * it could not accept was refused.
 */
interface VerifyApiKeyResult {
  valid: boolean;
  error: { code?: string } | null;
  key: VerifiedApiKey | null;
}

const apiKeyAuth = auth as unknown as {
  api: {
    verifyApiKey?: (input: {
      body: { key: string };
    }) => Promise<VerifyApiKeyResult>;
  };
};

const MAX_REQUESTED_SCOPES = 8;

/** The strict switch idiom: only the literal `true` turns the route on. */
function apiKeyExchangeEnabled(): boolean {
  return (
    process.env.API_KEY_ENABLED === 'true' &&
    process.env.API_KEY_AGENT_EXCHANGE_ENABLED === 'true'
  );
}

/** A typed `{ error: { code, message, details: { reason } } }` refusal. */
function refusal(
  status: HttpStatus,
  code: string,
  message: string,
  reason: string,
): HttpException {
  return new HttpException(
    { error: { code, message, details: { reason } } },
    status,
  );
}

/** Same body whether the switch is off or the audience is unknown. */
function notFound(): HttpException {
  return refusal(
    HttpStatus.NOT_FOUND,
    'not_found',
    'Unknown API-key exchange audience',
    'audience_not_found',
  );
}

function denialStatus(reason: ApiKeyExchangeDenialReason): HttpStatus {
  switch (reason) {
    case 'key_invalid':
    case 'key_disabled':
    case 'key_expired':
      return HttpStatus.UNAUTHORIZED;
    case 'key_rate_limited':
      return HttpStatus.TOO_MANY_REQUESTS;
    default:
      return HttpStatus.FORBIDDEN;
  }
}

@ApiTags('Plane Auth')
@Controller('api')
export class ApiKeyExchangeController {
  private readonly logger = new Logger(ApiKeyExchangeController.name);

  constructor(
    private readonly convexTokenService: ConvexTokenService,
    private readonly directNats: DirectNatsService,
  ) {}

  @Post(':audience/api-key-exchange')
  @ApiOperation({
    summary:
      'Exchange a user API key for a short-lived api_key principal token (registered gateway only)',
  })
  @ApiResponse({
    status: 201,
    description: '`{ data: { token, expiresAt, audience } }`',
  })
  async exchange(
    @Param('audience') audience: string,
    @NestHeaders('x-service-id') serviceId: string | undefined,
    @NestHeaders('x-service-api-key') credential: string | undefined,
    @Body() body: ApiKeyExchangeBody,
  ) {
    if (
      !apiKeyExchangeEnabled() ||
      !isApiKeyExchangeAudience(audience) ||
      !this.convexTokenService.isKnownAuthAudience(audience)
    ) {
      throw notFound();
    }

    const request: ApiKeyExchangeBody = body ?? {};
    if (Object.prototype.hasOwnProperty.call(request, 'zdr')) {
      throw refusal(
        HttpStatus.BAD_REQUEST,
        'invalid_request',
        'Retention posture is selected by organization policy',
        'retention_not_selectable',
      );
    }
    const apiKey = typeof request.apiKey === 'string' ? request.apiKey : '';
    const orgId = typeof request.orgId === 'string' ? request.orgId.trim() : '';
    const scopes = request.scopes;
    if (
      !apiKey ||
      !orgId ||
      !Array.isArray(scopes) ||
      scopes.length === 0 ||
      scopes.length > MAX_REQUESTED_SCOPES ||
      scopes.some((scope) => typeof scope !== 'string')
    ) {
      throw refusal(
        HttpStatus.BAD_REQUEST,
        'invalid_request',
        'apiKey, orgId and scopes are required',
        'malformed_request',
      );
    }
    const requestedScopes = scopes as string[];

    let caller: AuthorizedPlaneServiceApiKeyExchange;
    try {
      caller = authorizePlaneServiceApiKeyExchange(
        loadPlaneServicePrincipalRegistry(),
        {
          serviceId: serviceId ?? '',
          credential: credential ?? '',
          audience,
          orgId,
        },
      );
    } catch (error) {
      if (error instanceof ServicePrincipalConfigurationError) {
        this.logger.error(
          'Service-principal registry unavailable; refusing API-key exchange',
        );
        throw refusal(
          HttpStatus.SERVICE_UNAVAILABLE,
          'unavailable',
          'API-key exchange is unavailable',
          'registry_unavailable',
        );
      }
      // Not audited: an unauthenticated caller must not be able to fill the
      // durable audit stream.
      throw refusal(
        HttpStatus.FORBIDDEN,
        'forbidden',
        'Service principal is not authorized for API-key exchange',
        'caller_not_authorized',
      );
    }

    let verified: VerifyApiKeyResult;
    try {
      const verifyApiKey = apiKeyAuth.api.verifyApiKey;
      if (!verifyApiKey) {
        throw new Error('api-key plugin not loaded');
      }
      // No configId: the policy names an org key's refusal itself instead of
      // the plugin reporting it as an unknown key.
      verified = await verifyApiKey({ body: { key: apiKey } });
    } catch {
      this.logger.error('API-key verification unavailable; refusing exchange');
      throw refusal(
        HttpStatus.SERVICE_UNAVAILABLE,
        'unavailable',
        'API-key exchange is unavailable',
        'key_verification_unavailable',
      );
    }
    if (!verified.valid || !verified.key) {
      throw await this.deny(
        caller,
        audience,
        requestedScopes,
        apiKeyVerificationDenial(verified.error?.code),
        undefined,
      );
    }
    const key = verified.key;

    let membership: CanonicalTokenContext | null;
    try {
      membership = await resolveCanonicalTokenContext(key.referenceId, orgId);
    } catch {
      throw refusal(
        HttpStatus.SERVICE_UNAVAILABLE,
        'unavailable',
        'Canonical membership authority unavailable',
        'membership_unavailable',
      );
    }

    const decision = decideApiKeyExchange(key, membership, {
      audience,
      orgId,
      scopes: requestedScopes,
    });
    if (!decision.allowed) {
      throw await this.deny(
        caller,
        audience,
        requestedScopes,
        decision.reason,
        key,
      );
    }
    const grant = decision.grant;

    // The org's standing posture, raised by the key's own flag. When org-core
    // cannot be read and nothing usable is remembered no token is issued; it
    // never falls back to a persistent token.
    let orgZdr: boolean;
    try {
      orgZdr = (await resolveInteractiveRetentionPosture(grant.orgId)).zdr;
    } catch (error) {
      if (error instanceof RetentionPostureUnavailableError) {
        throw refusal(
          HttpStatus.SERVICE_UNAVAILABLE,
          'retention_posture_unavailable',
          'The organization retention posture could not be verified. Retry shortly.',
          'retention_posture_unavailable',
        );
      }
      throw error;
    }
    const zdr = apiKeyTokenZdr(orgZdr, grant.keyZdr);

    const response = this.mint(grant.audience, {
      userId: `apikey:${grant.keyId}`,
      orgId: grant.orgId,
      scopes: grant.scopes,
      principalType: 'api_key',
      keyId: grant.keyId,
      actingUserId: grant.actingUserId,
      retentionPosture: { zdr, authority: 'interactive-org-retention-policy' },
      // The signer caps every api_key token at
      // API_KEY_TOKEN_MAX_TTL_SECONDS (120 s); no override is needed here.
      tokenId: randomUUID(),
    });

    const event = 'api_key_token_exchanged';
    let audited = false;
    try {
      const auditIdentity = issuedTokenAuditIdentity(
        response.token,
        grant.audience === 'model-gateway' ? 'model-token' : 'plane-token',
      );
      await this.directNats.publishAuditDurable(
        `verevon.audit.v2.control.auth-core.${event}`,
        {
          occurred_at: auditIdentity.occurredAt,
          event_id: auditIdentity.eventId,
          org_id: grant.orgId,
          actor_role: 'api_key',
          plane: 'control',
          producer: 'auth-core',
          event,
          subject: `apikey:${grant.keyId}`,
          resource_id: grant.audience,
          outcome: 'ok',
          details: {
            audience: grant.audience,
            key_id: grant.keyId,
            acting_user_id: grant.actingUserId,
            scopes: grant.scopes,
            zdr,
            key_zdr: grant.keyZdr,
            calling_service: caller.subject,
            org_binding: caller.orgBinding,
          },
        },
      );
      audited = true;
    } catch {
      // Missing/failed PubAck preserves the endpoint's fail-closed contract.
    }
    if (!audited) {
      this.logger.error(
        'Durable audit unavailable; refusing API-key token issuance',
      );
      throw refusal(
        HttpStatus.SERVICE_UNAVAILABLE,
        'unavailable',
        'API-key exchange audit is unavailable',
        'audit_unavailable',
      );
    }

    return {
      data: {
        token: response.token,
        expiresAt: response.expiresAt,
        audience: response.audience,
      },
    };
  }

  /** model-gateway has its own issuer and audience; every other audience is a plane. */
  private mint(
    audience: ApiKeyExchangeAudience,
    claims: Parameters<ConvexTokenService['issuePlaneToken']>[1],
  ) {
    return audience === 'model-gateway'
      ? this.convexTokenService.issueModelPlaneToken(claims)
      : this.convexTokenService.issuePlaneToken(audience, claims);
  }

  /**
   * Audit a key-level refusal and build its response. The refusal issues no
   * token, so an audit that cannot be written is logged and the refusal still
   * answers; it never turns into a grant.
   */
  private async deny(
    caller: AuthorizedPlaneServiceApiKeyExchange,
    audience: ApiKeyExchangeAudience,
    requestedScopes: readonly string[],
    reason: ApiKeyExchangeDenialReason,
    key: VerifiedApiKey | undefined,
  ): Promise<HttpException> {
    const event = 'api_key_token_denied';
    try {
      await this.directNats.publishAuditDurable(
        `verevon.audit.v2.control.auth-core.${event}`,
        {
          occurred_at: new Date().toISOString(),
          event_id: `api-key-exchange:denied:${randomUUID()}`,
          org_id: caller.orgId,
          actor_role: 'service',
          plane: 'control',
          producer: 'auth-core',
          event,
          subject: caller.subject,
          resource_id: audience,
          outcome: 'denied',
          details: {
            audience,
            reason,
            scopes: [...requestedScopes],
            calling_service: caller.subject,
            ...(key ? { key_id: key.id } : {}),
            // An org key's reference is an organization, not a person.
            ...(key?.configId === API_KEY_EXCHANGE_CONFIG_ID
              ? { acting_user_id: key.referenceId }
              : {}),
          },
        },
      );
    } catch {
      this.logger.error(
        `Durable audit unavailable for an API-key exchange refusal (${reason})`,
      );
    }
    return refusal(
      denialStatus(reason),
      'api_key_exchange_denied',
      'The API key cannot be exchanged for this audience',
      reason,
    );
  }
}
