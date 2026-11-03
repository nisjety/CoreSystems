/**
 * EP-12 S2 / INNH-10 CP-1: `POST /api/:audience/api-key-exchange`.
 *
 *   headers  x-service-id, x-service-api-key  (the CALLER: the Verevon gateway)
 *   body     { apiKey, orgId, scopes[] }
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
 * 404.
 *
 * One route, two audience families:
 *
 *   - Browser agent audiences (`quarry`, `browser-broker`; INNH-10 CP-1 / CP-4):
 *     `ApiKeyExchangeHandler` below. The token is the design's section 3 shape
 *     minted by `issueApiKeyAgentToken` alone (`web_policy`, and `key_hosts` on
 *     `browser-broker`), the answer is the bare token bundle, and refusals carry
 *     `code: api_key_*`.
 *   - Agent API audiences (`model-gateway`, `session-core`, `inference-core`;
 *     EP-12 S2): `ApiKeyExchangeController.exchange` with the pure policy in
 *     `api-key-exchange.ts`, minted through `issuePlaneToken` /
 *     `issueModelPlaneToken`, answered as `{ data: { token, expiresAt,
 *     audience } }`, refusals typed `{ error: { code, message, details: {
 *     reason } } }`.
 *
 * Order of checks on the browser agent path, each failing closed with no token:
 *   1. `API_KEY_ENABLED` and `API_KEY_AGENT_EXCHANGE_ENABLED` both `true`, else
 *      404 `api_key_agent_exchange_disabled` (the route is dark by default).
 *   2. Audience `quarry` or `browser-broker`, else 404.
 *   3. Body shape (no caller-selected `zdr`), else 400.
 *   4. The caller is a registry principal whose `apiKeyExchange` lists the
 *      audience (`authorizePlaneApiKeyExchange`), else 403; registry
 *      unreadable 503.
 *   5. Better Auth `verifyApiKey` with configId `user-keys` (OD-4: org keys are
 *      refused): enabled, not expired, rate limit consumed. 401 / 429.
 *   6. The key's agent metadata (`readApiKeyAgentMetadata`, strict) is valid and
 *      its `org_id` equals `orgId`. 403.
 *   7. The key holds `browser:agent` and every requested scope (OD-5; the token
 *      scope set is `browser:agent` only). 403.
 *   8. The key's owner is still a member of the org. 403; membership authority
 *      unreadable 503.
 *   9. The org's retention posture and web policy (503 when unreadable).
 *  10. Mint (`issueApiKeyAgentToken`: 120 s, jti, zdr = org OR key, web_policy,
 *      key_hosts on `browser-broker`).
 *  11. Durable audit `api_key_token_exchanged`, fail-closed: 503 and the token
 *      is discarded if there is no PubAck.
 *
 * On both paths every grant and every key-level refusal is a durable audit
 * event (`api_key_token_exchanged` / `api_key_token_denied`); a grant whose
 * event cannot be written answers 503 and returns no token, while a refusal is
 * answered whether or not its audit lands. Nothing here logs or audits the key,
 * a key prefix or a minted token.
 */
import {
  BadRequestException,
  Body,
  Controller,
  ForbiddenException,
  Headers as NestHeaders,
  HttpException,
  HttpStatus,
  Logger,
  NotFoundException,
  Param,
  Post,
  ServiceUnavailableException,
  UnauthorizedException,
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
import {
  API_KEY_USER_CONFIG_ID,
  BROWSER_AGENT_PERMISSION,
  apiKeyAgentExchangeEnabled,
  apiKeyHoldsBrowserAgent,
  apiKeyPermissionList,
  readApiKeyAgentMetadata,
  type ApiKeyAgentMetadata,
} from './api-key-metadata';
import { issuedTokenAuditIdentity } from './audit-event-identity';
import { auth } from './auth';
import {
  ConvexTokenService,
  isApiKeyAgentAudience,
  type ApiKeyAgentAudience,
} from './convex-token.service';
import {
  resolveInteractiveRetentionPosture,
  RetentionPostureUnavailableError,
  type InteractiveRetentionPosture,
} from './interactive-retention-policy';
import {
  authorizePlaneApiKeyExchange,
  authorizePlaneServiceApiKeyExchange,
  loadPlaneServicePrincipalRegistry,
  ServicePrincipalConfigurationError,
  type AuthorizedPlaneApiKeyExchange,
  type AuthorizedPlaneServiceApiKeyExchange,
} from './plane-service-principal';
import {
  resolveCanonicalTokenContext,
  type CanonicalTokenContext,
} from './plane-token-membership';
import { webPolicyClaimFor } from './web-policy';

export const API_KEY_TOKEN_EXCHANGED_EVENT = 'api_key_token_exchanged';
export const API_KEY_TOKEN_DENIED_EVENT = 'api_key_token_denied';

/** Scopes an exchanged key token may carry at all (design section 4). */
export const API_KEY_EXCHANGE_TOKEN_SCOPES: readonly string[] = Object.freeze([
  BROWSER_AGENT_PERMISSION,
]);

const MAX_API_KEY_LENGTH = 512;
const MAX_REQUESTED_SCOPES = 8;
const ORGANIZATION_ID = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/;

export interface ApiKeyExchangeBody {
  apiKey?: unknown;
  orgId?: unknown;
  scopes?: unknown;
}

export type ApiKeyExchangeDenyReason =
  | 'api_key_invalid'
  | 'api_key_expired'
  | 'api_key_disabled'
  | 'api_key_rate_limited'
  | 'api_key_kind_refused'
  | 'api_key_metadata_invalid'
  | 'api_key_org_mismatch'
  | 'api_key_scope_missing'
  | 'api_key_owner_not_member';

type ExchangeTokenBundle = ReturnType<
  ConvexTokenService['issueApiKeyAgentToken']
>;

/** The subset of Better Auth's `verifyApiKey` answer this exchange reads. */
interface VerifiedApiKeyResult {
  valid?: boolean;
  error?: { message?: string; code?: string } | null;
  key?: {
    id?: unknown;
    configId?: unknown;
    referenceId?: unknown;
    userId?: unknown;
    enabled?: unknown;
    expiresAt?: unknown;
    permissions?: unknown;
    metadata?: unknown;
  } | null;
}

type VerifyApiKey = (options: {
  body: { key: string; configId: string };
}) => Promise<VerifiedApiKeyResult>;

/** Better Auth's verifier, looked up per call (the plugin is optional). */
function betterAuthVerifyApiKey(): VerifyApiKey | null {
  const method = (auth.api as Record<string, unknown>).verifyApiKey;
  if (typeof method !== 'function') return null;
  return async (options) =>
    (await Reflect.apply(method, auth.api, [options])) as VerifiedApiKeyResult;
}

class ApiKeyExchangeDenied extends Error {
  constructor(
    readonly reason: ApiKeyExchangeDenyReason,
    readonly status: HttpStatus,
    readonly keyId?: string,
    readonly actingUserId?: string,
  ) {
    super(reason);
  }
}

function deny(
  reason: ApiKeyExchangeDenyReason,
  status: HttpStatus,
  keyId?: string,
  actingUserId?: string,
): never {
  throw new ApiKeyExchangeDenied(reason, status, keyId, actingUserId);
}

function denialException(denied: ApiKeyExchangeDenied): HttpException {
  const body = {
    code: denied.reason,
    message: 'API key exchange refused',
  };
  switch (denied.status) {
    case HttpStatus.UNAUTHORIZED:
      return new UnauthorizedException(body);
    case HttpStatus.FORBIDDEN:
      return new ForbiddenException(body);
    default:
      return new HttpException(body, denied.status);
  }
}

function reasonFromVerifyError(code: string | undefined): {
  reason: ApiKeyExchangeDenyReason;
  status: HttpStatus;
} {
  switch (code) {
    case 'KEY_EXPIRED':
      return { reason: 'api_key_expired', status: HttpStatus.UNAUTHORIZED };
    case 'KEY_DISABLED':
      return { reason: 'api_key_disabled', status: HttpStatus.UNAUTHORIZED };
    case 'USAGE_EXCEEDED':
    case 'RATE_LIMITED':
      return {
        reason: 'api_key_rate_limited',
        status: HttpStatus.TOO_MANY_REQUESTS,
      };
    default:
      return { reason: 'api_key_invalid', status: HttpStatus.UNAUTHORIZED };
  }
}

function expiredAt(value: unknown, now: number): boolean {
  if (value === null || value === undefined || value === '') return false;
  const at =
    value instanceof Date
      ? value.getTime()
      : new Date(value as string | number).getTime();
  // An unreadable expiry fails closed as expired.
  return !Number.isFinite(at) || at <= now;
}

function parseBody(body: ApiKeyExchangeBody | undefined): {
  apiKey: string;
  orgId: string;
  scopes: string[];
} {
  const record = (body ?? {}) as Record<string, unknown>;
  if (
    Object.prototype.hasOwnProperty.call(record, 'zdr') ||
    Object.prototype.hasOwnProperty.call(record, 'ttlSeconds')
  ) {
    throw new BadRequestException({
      code: 'api_key_exchange_body_invalid',
      message: 'Retention posture and lifetime are selected by policy',
    });
  }
  const apiKey = typeof record.apiKey === 'string' ? record.apiKey : '';
  const orgId = typeof record.orgId === 'string' ? record.orgId.trim() : '';
  const rawScopes = record.scopes;
  const scopes =
    Array.isArray(rawScopes) &&
    rawScopes.length > 0 &&
    rawScopes.length <= MAX_REQUESTED_SCOPES &&
    rawScopes.every((scope) => typeof scope === 'string' && scope.trim())
      ? [...new Set((rawScopes as string[]).map((scope) => scope.trim()))]
      : null;
  if (
    !apiKey ||
    apiKey.length > MAX_API_KEY_LENGTH ||
    apiKey.trim() !== apiKey ||
    !ORGANIZATION_ID.test(orgId) ||
    !scopes
  ) {
    throw new BadRequestException({
      code: 'api_key_exchange_body_invalid',
      message: 'apiKey, orgId and a non-empty scopes list are required',
    });
  }
  return { apiKey, orgId, scopes };
}

/**
 * The exchange itself, kept out of the Nest controller so the route stays on
 * `PlaneTokenController` and the logic is testable with plain doubles.
 */
export class ApiKeyExchangeHandler {
  private readonly logger = new Logger('ApiKeyExchange');

  constructor(
    private readonly convexTokenService: Pick<
      ConvexTokenService,
      'issueApiKeyAgentToken'
    >,
    private readonly directNats: Pick<DirectNatsService, 'publishAuditDurable'>,
    private readonly verifyApiKey: () => VerifyApiKey | null = betterAuthVerifyApiKey,
  ) {}

  async exchange(
    audience: string,
    serviceId: string | undefined,
    credential: string | undefined,
    body: ApiKeyExchangeBody | undefined,
  ): Promise<ExchangeTokenBundle> {
    if (!apiKeyAgentExchangeEnabled()) {
      throw new NotFoundException({
        code: 'api_key_agent_exchange_disabled',
        message: 'API key exchange is not enabled',
      });
    }
    if (!isApiKeyAgentAudience(audience)) {
      throw new NotFoundException(`Unknown API key exchange audience: ${audience}`);
    }
    const request = parseBody(body);

    let caller: AuthorizedPlaneApiKeyExchange;
    try {
      caller = authorizePlaneApiKeyExchange(loadPlaneServicePrincipalRegistry(), {
        serviceId: serviceId ?? '',
        credential: credential ?? '',
        audience,
      });
    } catch (error) {
      if (error instanceof ServicePrincipalConfigurationError) {
        this.logger.error(
          'Service-principal registry unavailable; refusing API key exchange',
        );
        throw new ServiceUnavailableException(
          'Service-principal issuance is unavailable',
        );
      }
      throw new ForbiddenException(
        'Service principal is not authorized for API key exchange',
      );
    }

    try {
      return await this.exchangeForCaller(audience, caller, request);
    } catch (error) {
      if (error instanceof ApiKeyExchangeDenied) {
        await this.auditDenied(audience, caller, request, error);
        throw denialException(error);
      }
      throw error;
    }
  }

  private async exchangeForCaller(
    audience: ApiKeyAgentAudience,
    caller: AuthorizedPlaneApiKeyExchange,
    request: { apiKey: string; orgId: string; scopes: string[] },
  ): Promise<ExchangeTokenBundle> {
    const verify = this.verifyApiKey();
    if (!verify) {
      this.logger.error('Better Auth api-key plugin unavailable');
      throw new ServiceUnavailableException('API key verification is unavailable');
    }
    let verified: VerifiedApiKeyResult;
    try {
      verified = await verify({
        body: { key: request.apiKey, configId: API_KEY_USER_CONFIG_ID },
      });
    } catch {
      // Never log the error object: it may echo the request body.
      this.logger.error('API key verification failed; refusing exchange');
      throw new ServiceUnavailableException('API key verification is unavailable');
    }
    if (!verified?.valid || !verified.key) {
      const { reason, status } = reasonFromVerifyError(
        verified?.error?.code ?? undefined,
      );
      deny(reason, status);
    }
    const key = verified.key;
    const keyId = typeof key.id === 'string' ? key.id : '';
    if (!/^[\x21-\x7e]{1,128}$/.test(keyId)) {
      deny('api_key_invalid', HttpStatus.UNAUTHORIZED);
    }
    // OD-4: user keys only. verifyApiKey already scoped the lookup to
    // `user-keys`; this re-check holds even if that scoping changes upstream.
    if (key.configId !== API_KEY_USER_CONFIG_ID) {
      deny('api_key_kind_refused', HttpStatus.FORBIDDEN, keyId);
    }
    const ownerId =
      typeof key.referenceId === 'string' && key.referenceId
        ? key.referenceId
        : typeof key.userId === 'string'
          ? key.userId
          : '';
    if (!ownerId) {
      deny('api_key_kind_refused', HttpStatus.FORBIDDEN, keyId);
    }
    if (key.enabled === false) {
      deny('api_key_disabled', HttpStatus.UNAUTHORIZED, keyId, ownerId);
    }
    if (expiredAt(key.expiresAt, Date.now())) {
      deny('api_key_expired', HttpStatus.UNAUTHORIZED, keyId, ownerId);
    }

    const metadata: ApiKeyAgentMetadata | null = readApiKeyAgentMetadata(
      key.metadata,
    );
    if (!metadata) {
      deny('api_key_metadata_invalid', HttpStatus.FORBIDDEN, keyId, ownerId);
    }
    if (metadata.org_id !== request.orgId) {
      deny('api_key_org_mismatch', HttpStatus.FORBIDDEN, keyId, ownerId);
    }

    const held = new Set(apiKeyPermissionList(key.permissions));
    if (
      !apiKeyHoldsBrowserAgent(key.permissions) ||
      request.scopes.some(
        (scope) =>
          !held.has(scope) || !API_KEY_EXCHANGE_TOKEN_SCOPES.includes(scope),
      )
    ) {
      deny('api_key_scope_missing', HttpStatus.FORBIDDEN, keyId, ownerId);
    }

    let membership: Awaited<ReturnType<typeof resolveCanonicalTokenContext>>;
    try {
      membership = await resolveCanonicalTokenContext(ownerId, request.orgId);
    } catch {
      throw new ServiceUnavailableException(
        'Canonical membership authority unavailable',
      );
    }
    if (!membership || membership.orgId !== request.orgId) {
      deny('api_key_owner_not_member', HttpStatus.FORBIDDEN, keyId, ownerId);
    }

    // 503 (retention_posture_unavailable / web_policy_unavailable) and NO
    // token when org-core cannot answer: let both propagate.
    const retentionPosture: InteractiveRetentionPosture =
      await resolveInteractiveRetentionPosture(request.orgId);
    const webPolicy = await webPolicyClaimFor(audience, request.orgId);

    const response = this.convexTokenService.issueApiKeyAgentToken(audience, {
      keyId,
      actingUserId: ownerId,
      orgId: request.orgId,
      scopes: request.scopes,
      retentionPosture,
      keyZdr: metadata.zdr,
      webPolicy,
      ...(audience === 'browser-broker' ? { keyHosts: metadata.key_hosts } : {}),
      tokenId: randomUUID(),
    });
    const zdr = retentionPosture.zdr || metadata.zdr;

    let audited = false;
    try {
      const auditIdentity = issuedTokenAuditIdentity(
        response.token,
        'plane-token',
      );
      await this.directNats.publishAuditDurable(
        `verevon.audit.v2.control.auth-core.${API_KEY_TOKEN_EXCHANGED_EVENT}`,
        {
          occurred_at: auditIdentity.occurredAt,
          event_id: auditIdentity.eventId,
          org_id: request.orgId,
          actor_role: 'service',
          plane: 'control',
          producer: 'auth-core',
          event: API_KEY_TOKEN_EXCHANGED_EVENT,
          subject: `apikey:${keyId}`,
          resource_id: keyId,
          outcome: 'ok',
          details: {
            key_id: keyId,
            org_id: request.orgId,
            acting_user_id: ownerId,
            audience,
            scopes: request.scopes,
            zdr,
            zdr_source: metadata.zdr && !retentionPosture.zdr ? 'key' : 'org',
            calling_service: caller.serviceId,
            ...(webPolicy !== undefined
              ? { web_policy_rev: webPolicy.rev }
              : {}),
          },
        },
      );
      audited = true;
    } catch {
      // Missing/failed PubAck preserves the endpoint's fail-closed contract.
    }
    if (!audited) {
      this.logger.error('Durable audit unavailable; refusing API key exchange');
      throw new ServiceUnavailableException(
        'API key exchange audit is unavailable',
      );
    }
    return response;
  }

  private async auditDenied(
    audience: ApiKeyAgentAudience,
    caller: AuthorizedPlaneApiKeyExchange,
    request: { orgId: string; scopes: string[] },
    denied: ApiKeyExchangeDenied,
  ): Promise<void> {
    try {
      await this.directNats.publishAuditDurable(
        `verevon.audit.v2.control.auth-core.${API_KEY_TOKEN_DENIED_EVENT}`,
        {
          occurred_at: new Date().toISOString(),
          event_id: `api-key-exchange:denied:${randomUUID()}`,
          org_id: request.orgId,
          actor_role: 'service',
          plane: 'control',
          producer: 'auth-core',
          event: API_KEY_TOKEN_DENIED_EVENT,
          subject: denied.keyId ? `apikey:${denied.keyId}` : caller.subject,
          resource_id: denied.keyId ?? audience,
          outcome: 'denied',
          details: {
            ...(denied.keyId ? { key_id: denied.keyId } : {}),
            org_id: request.orgId,
            ...(denied.actingUserId
              ? { acting_user_id: denied.actingUserId }
              : {}),
            audience,
            scopes: request.scopes,
            reason: denied.reason,
            calling_service: caller.serviceId,
          },
        },
      );
    } catch {
      // The request is refused either way; only the trail is missing.
      this.logger.error(
        `Durable audit unavailable for an API key exchange denial (${denied.reason})`,
      );
    }
  }
}

// ---------------------------------------------------------------------------
// The route (EP-12 S2): Agent API audiences here, browser agent audiences
// delegated to ApiKeyExchangeHandler above.
// ---------------------------------------------------------------------------

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
  /** INNH-10 CP-1: the browser agent audiences (`quarry`, `browser-broker`). */
  private readonly browserAgentExchange: ApiKeyExchangeHandler;

  constructor(
    private readonly convexTokenService: ConvexTokenService,
    private readonly directNats: DirectNatsService,
  ) {
    this.browserAgentExchange = new ApiKeyExchangeHandler(
      convexTokenService,
      directNats,
    );
  }

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
    // INNH-10 CP-1: a browser agent token carries web_policy and key_hosts and
    // is minted by issueApiKeyAgentToken alone; that path keeps its own checks,
    // answer shape and refusal codes (the Verevon gateway's api_key_agent reads
    // them).
    if (isApiKeyAgentAudience(audience)) {
      return this.browserAgentExchange.exchange(
        audience,
        serviceId,
        credential,
        body,
      );
    }
    if (
      !apiKeyAgentExchangeEnabled() ||
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
