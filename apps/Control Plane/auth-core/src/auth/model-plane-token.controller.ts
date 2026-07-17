/**
 * U2-5 — Model Plane gateway token issuance.
 *
 * Mints a short-lived RS256 JWT that the Model Plane gateway's
 * `auth::require_auth` middleware accepts. Reuses the same RS256
 * keypair as the Convex auth flow (a single JWKS endpoint —
 * `/api/convex-auth/jwks` — serves both audiences); the only
 * differences are `aud=model-gateway` and the claim shape required by
 * `apps/Model Plane/rust/services/model-gateway/src/auth.rs`
 * (`org_id` + `user_id` snake_case top-level claims).
 *
 * Two issuance paths:
 *   - **GET /api/model-plane/token**: from the active Better Auth
 *     session cookie. Used by velion's server-side API routes when
 *     they proxy to the gateway on behalf of a logged-in user.
 *   - **POST /api/model-plane/internal-token**: from a registered,
 *     audience/scope/tenant-bound service credential. Request fields may
 *     narrow the deployment allowlist but can never widen it.
 *
 * **Production rollout note** (Phase 2 of U2-5): once velion is fully
 * forwarding these tokens, set on the gateway:
 *   AUTH_CORE_JWKS_URL=http://auth-core:3011/api/convex-auth/jwks
 *   AUTH_CORE_AUDIENCE=model-gateway
 *   AUTH_CORE_ISSUER=<whatever MODEL_PLANE_AUTH_ISSUER resolves to>
 * and **remove** `MODEL_GATEWAY_AUTH_DEV_BYPASS=1` from production
 * compose. Dev compose can keep the bypass for offline iteration.
 */

import {
  BadRequestException,
  Body,
  Controller,
  ForbiddenException,
  Get,
  Headers as NestHeaders,
  Logger,
  Post,
  Req,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';
import { ApiOperation, ApiResponse, ApiTags } from '@nestjs/swagger';
import type { Request } from 'express';

import { DirectNatsService } from '../nats/direct-nats.service';
import { issuedTokenAuditIdentity } from './audit-event-identity';
import { auth } from './auth';
import { ConvexTokenService } from './convex-token.service';
import { InteractiveRetentionPolicyConfigurationError } from './interactive-retention-policy';
import {
  authorizePlaneServicePrincipal,
  type AuthorizedPlaneServicePrincipal,
  loadPlaneServicePrincipalRegistry,
  ServicePrincipalConfigurationError,
} from './plane-service-principal';
import {
  type CanonicalTokenContext,
  resolveCanonicalTokenContext,
} from './plane-token-membership';
import { modelGatewayScopesForRole } from './plane-token-scopes';

/**
 * Convert Express's `IncomingHttpHeaders` (plain object) into a Web API
 * `Headers` instance — Better Auth's `auth.api.getSession({ headers })`
 * expects a Web `Headers` and calls `.get('cookie')` on it. Same helper
 * as `convex-auth.controller.ts`; duplicated here to avoid coupling.
 */
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

interface ModelPlaneInternalTokenBody {
  orgId?: string;
  scopes?: readonly string[];
  reason?: string;
}

interface AuthSessionSnapshot {
  user?: { id?: string; email?: string | null };
  session?: { activeOrganizationId?: string };
}

const typedSessionAuth = auth as unknown as {
  api: {
    getSession(input: {
      headers: Headers;
    }): Promise<AuthSessionSnapshot | null>;
  };
};

@ApiTags('Model Plane Auth')
@Controller('api/model-plane')
export class ModelPlaneTokenController {
  private readonly logger = new Logger(ModelPlaneTokenController.name);

  constructor(
    private readonly convexTokenService: ConvexTokenService,
    private readonly directNats: DirectNatsService,
  ) {}

  /**
   * Mint a model-plane JWT from the active Better Auth session.
   *
   * The session cookie is required — without it we can't resolve a user
   * identity (returns 401). User-Service is queried for the active org
   * context. If the user has no active org, the token is still issued
   * with `org_id=""` and the gateway will reject it (its `Claims`
   * struct refuses empty org_id) — failing closed is intentional.
   */
  @Get('token')
  @ApiOperation({
    summary:
      'Mint a short-lived Model Plane gateway JWT from the active Better Auth session',
  })
  @ApiResponse({
    status: 200,
    description: 'RS256-signed JWT for `Authorization: Bearer <token>`',
  })
  async getToken(@Req() request: Request) {
    const session = await typedSessionAuth.api.getSession({
      headers: toWebHeaders(request.headers),
    });

    if (!session?.user?.id) {
      throw new UnauthorizedException('Authentication required');
    }

    let sessionContext: CanonicalTokenContext | null;
    try {
      sessionContext = await resolveCanonicalTokenContext(
        session.user.id,
        session.session?.activeOrganizationId ?? '',
      );
    } catch {
      throw new ServiceUnavailableException(
        'Canonical membership authority unavailable',
      );
    }

    if (!sessionContext?.orgId) {
      // The gateway rejects tokens with empty org_id (Claims.org_id check
      // in auth.rs). Surface that here as a clear 400 instead of letting
      // the user discover via a confusing 401 at the gateway boundary.
      throw new BadRequestException(
        'No active organisation on session; user-core returned no orgId',
      );
    }

    // Wave 7 (velion ui-ux-velion-gap.md §17): owners + admins get the
    // `admin` scope embedded in the JWT. The model-gateway's fine-tune
    // routes (and any other admin-gated surface) check this via
    // `claims.has_scope("admin")`. Anyone else gets an empty scopes
    // array — the gateway rejects admin actions with 403.
    const scopes = modelGatewayScopesForRole(sessionContext.role);

    let bundle;
    try {
      bundle = this.convexTokenService.issueModelPlaneToken({
        userId: session.user.id,
        orgId: sessionContext.orgId,
        email: session.user.email ?? undefined,
        scopes: scopes.length > 0 ? scopes : undefined,
      });
    } catch (error) {
      if (error instanceof InteractiveRetentionPolicyConfigurationError) {
        this.logger.error(
          'Interactive retention policy unavailable; refusing Model token issuance',
        );
        throw new ServiceUnavailableException(
          'Interactive retention policy is unavailable',
        );
      }
      throw error;
    }

    return {
      ...bundle,
      userId: session.user.id,
      orgId: sessionContext.orgId,
      role: sessionContext.role ?? null,
    };
  }

  /**
   * Service-to-service token issuance. The deployment registry binds the
   * credential to a service identity, Model audience, tenants, and maximum
   * scopes. The request can only select a configured subset.
   */
  @Post('internal-token')
  @ApiOperation({
    summary: 'Mint a bounded Model Plane gateway JWT for a service principal',
  })
  async issueInternalToken(
    @NestHeaders('x-service-id') serviceId: string | undefined,
    @NestHeaders('x-service-api-key') credential: string | undefined,
    @Body() body: ModelPlaneInternalTokenBody,
    @NestHeaders('x-zdr') requestedZdr?: string,
  ) {
    if (
      requestedZdr !== undefined ||
      Object.prototype.hasOwnProperty.call(body, 'zdr')
    ) {
      throw new BadRequestException(
        'Retention posture is selected by deployment policy',
      );
    }
    const orgId = (body.orgId ?? '').trim();
    const scopes = Array.isArray(body.scopes) ? body.scopes : [];
    const reason = (body.reason ?? '').trim();
    let principal: AuthorizedPlaneServicePrincipal;
    try {
      principal = authorizePlaneServicePrincipal(
        loadPlaneServicePrincipalRegistry(),
        {
          serviceId: serviceId ?? '',
          credential: credential ?? '',
          audience: 'model-gateway',
          orgId,
          requestedScopes: scopes,
          reason,
        },
      );
    } catch (error) {
      if (error instanceof ServicePrincipalConfigurationError) {
        this.logger.error(
          'Service-principal registry unavailable; refusing Model token issuance',
        );
        throw new ServiceUnavailableException(
          'Service-principal issuance is unavailable',
        );
      }
      throw new ForbiddenException('Service principal is not authorized');
    }
    const response = this.convexTokenService.issueModelPlaneToken({
      userId: principal.subject,
      orgId: principal.orgId,
      scopes: principal.scopes,
      principalType: 'service',
      serviceId: principal.serviceId,
      reason: principal.reason,
      retentionPosture: {
        zdr: principal.zdr,
        authority: 'service-principal-policy',
      },
    });
    const event = 'model_service_token_issued';
    let audited = false;
    try {
      const auditIdentity = issuedTokenAuditIdentity(
        response.token,
        'model-token',
      );
      await this.directNats.publishAuditDurable(
        `velion.audit.v2.control.auth-core.${event}`,
        {
          occurred_at: auditIdentity.occurredAt,
          event_id: auditIdentity.eventId,
          org_id: principal.orgId,
          actor_role: 'service',
          plane: 'control',
          producer: 'auth-core',
          event,
          subject: principal.subject,
          resource_id: 'model-gateway',
          outcome: 'ok',
          details: {
            audience: 'model-gateway',
            scopes: principal.scopes,
            reason: principal.reason,
            zdr: principal.zdr,
          },
        },
      );
      audited = true;
    } catch {
      // Missing/failed PubAck preserves the endpoint's fail-closed contract.
    }
    if (!audited) {
      this.logger.error(
        'Durable audit unavailable; refusing Model service-token issuance',
      );
      throw new ServiceUnavailableException(
        'Service-principal issuance audit is unavailable',
      );
    }
    return response;
  }
}
