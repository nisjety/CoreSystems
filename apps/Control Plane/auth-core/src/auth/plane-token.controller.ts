/**
 * Phase A · A1.1 — generic plane-token issuance.
 *
 * Issues short-lived RS256 JWTs for every non-Model-Plane audience
 * (`data-plane`, `quarry`, `ingestion`, `control-plane`,
 * `application-plane`). Mirrors the Model-Plane controller pattern but
 * keeps the per-audience route slug so velion's call sites match the
 * existing `mintPlaneToken({ audience, ... })` helper without rewrites.
 *
 * Routes:
 *   GET  /api/data-plane/token          → session-cookie path
 *   POST /api/data-plane/internal-token → service-to-service path
 *   GET  /api/quarry/token              → ...
 *   POST /api/quarry/internal-token     → ...
 *   GET  /api/ingestion/token           → ...
 *   POST /api/ingestion/internal-token  → ...
 *   GET  /api/control-plane/token       → ...
 *   POST /api/control-plane/internal-token → ...
 *   GET  /api/application-plane/token   → ...
 *   POST /api/application-plane/internal-token → ...
 *   POST /api/control-policy/internal-token → Data policy caller (service only)
 *
 * The path slug is parsed back into a `PlaneAudience` and validated
 * against the configured set in `ConvexTokenService.isKnownPlaneAudience`
 * — unknown audiences surface as 404 so a typo on the velion side never
 * gets a silent fallback token.
 *
 * All tokens share the keypair / JWKS that `convex-auth.controller.ts`
 * publishes at `/api/convex-auth/jwks`. Audience separation prevents
 * cross-surface reuse: each plane's middleware verifies `aud` against
 * its own expected value.
 */

import {
  BadRequestException,
  Body,
  Controller,
  ForbiddenException,
  Get,
  Headers as NestHeaders,
  Logger,
  NotFoundException,
  Param,
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
import {
  authorizePlaneServicePrincipal,
  ServicePrincipalConfigurationError,
  type AuthorizedPlaneServicePrincipal,
} from './plane-service-principal';
import {
  resolveCanonicalTokenContext,
  type CanonicalTokenContext,
} from './plane-token-membership';
import { planeScopesForRole } from './plane-token-scopes';

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

/**
 * Reserved, non-tenant `org_id` for pre-org onboarding preview tokens. It is
 * never a real organization: preview crawls minted with it are working-set
 * only (never persisted to Data Plane), so no data is ever written under it.
 */
const ONBOARDING_ORG_SENTINEL = 'onboarding';

interface PlaneInternalTokenBody {
  orgId?: string;
  scopes?: readonly string[];
  reason?: string;
  zdr?: boolean;
}

@ApiTags('Plane Auth')
@Controller('api')
export class PlaneTokenController {
  private readonly logger = new Logger(PlaneTokenController.name);

  constructor(
    private readonly convexTokenService: ConvexTokenService,
    private readonly directNats: DirectNatsService,
  ) {}

  /**
   * Mint a plane-scoped JWT from the active Better Auth session. The
   * audience comes from the URL path so the controller can serve every
   * non-Model-Plane plane without duplicate route handlers.
   *
   * Routes registered: `/api/:audience/token` for interactive audiences in
   * `PlaneAudience`. The `control-policy` audience is deliberately excluded
   * and available only through the scoped service-principal path. Unknown or
   * service-only audiences → 404, no session → 401, no active org → 400
   * (fail-closed; a token with empty `org_id` would be rejected downstream
   * anyway, so surface it here as a clear error).
   */
  /**
   * Pre-org onboarding preview token. The onboarding website step runs BEFORE
   * the user creates an organization (the crawl is used to infer the company),
   * so `:audience/token` — which fails closed with no active org — cannot serve
   * it. This mints a `quarry`-audience JWT carrying a reserved sentinel
   * `org_id` (`onboarding`) and a single restricted scope (`onboarding:preview`)
   * so quarry-edge, whose auth requires a non-empty `org_id`, accepts the
   * transient preview crawl (working-set only, never persisted to a real
   * tenant). A real org-scoped token is still required for any durable ingest.
   * Requires only a valid session — no org.
   */
  @Get('quarry/onboarding-token')
  @ApiOperation({
    summary: 'Mint a pre-org quarry preview token for onboarding',
  })
  async getOnboardingToken(@Req() request: Request) {
    const session = await auth.api.getSession({
      headers: toWebHeaders(request.headers),
    });
    if (!session?.user?.id) {
      throw new UnauthorizedException('Authentication required');
    }
    const bundle = this.convexTokenService.issuePlaneToken('quarry', {
      userId: session.user.id,
      orgId: ONBOARDING_ORG_SENTINEL,
      email: session.user.email ?? undefined,
      scopes: ['onboarding:preview'],
    });
    return {
      ...bundle,
      userId: session.user.id,
      orgId: ONBOARDING_ORG_SENTINEL,
      role: null,
    };
  }

  @Get(':audience/token')
  @ApiOperation({
    summary:
      'Mint a short-lived plane-scoped JWT from the active Better Auth session',
  })
  @ApiResponse({
    status: 200,
    description: 'RS256-signed JWT for `Authorization: Bearer <token>`',
  })
  async getToken(@Param('audience') audience: string, @Req() request: Request) {
    if (!this.convexTokenService.isInteractivePlaneAudience(audience)) {
      throw new NotFoundException(`Unknown plane audience: ${audience}`);
    }

    const session = await auth.api.getSession({
      headers: toWebHeaders(request.headers),
    });

    if (!session?.user?.id) {
      throw new UnauthorizedException('Authentication required');
    }

    const activeOrganizationId = (
      session.session as { activeOrganizationId?: string }
    ).activeOrganizationId;
    let sessionContext: CanonicalTokenContext | null;
    try {
      sessionContext = await resolveCanonicalTokenContext(
        session.user.id,
        activeOrganizationId ?? '',
      );
    } catch {
      throw new ServiceUnavailableException(
        'Canonical membership authority unavailable',
      );
    }

    if (!sessionContext?.orgId) {
      throw new BadRequestException(
        'No active organisation on session; user-core returned no orgId',
      );
    }

    // User-level scopes preserve ordinary document/wiki workflows, while
    // tenant-wide/admin capabilities remain role-gated. Destructive search
    // rebuild is intentionally absent from every interactive-user token.
    const scopes = planeScopesForRole(sessionContext.role, audience);

    const bundle = this.convexTokenService.issuePlaneToken(audience, {
      userId: session.user.id,
      orgId: sessionContext.orgId,
      email: session.user.email ?? undefined,
      scopes,
    });

    return {
      ...bundle,
      userId: session.user.id,
      orgId: sessionContext.orgId,
      role: sessionContext.role ?? null,
    };
  }

  /**
   * Mint a plane-scoped JWT for a registered service principal. The caller's
   * identity, audiences, tenants, and maximum scopes come from the deployment
   * allowlist. Request fields can only narrow those bounds.
   */
  @Post(':audience/internal-token')
  @ApiOperation({
    summary:
      'Mint a bounded plane-scoped JWT for a registered service principal',
  })
  async issueInternalToken(
    @Param('audience') audience: string,
    @NestHeaders('x-service-id') serviceId: string | undefined,
    @NestHeaders('x-service-api-key') credential: string | undefined,
    @Body() body: PlaneInternalTokenBody,
  ) {
    if (!this.convexTokenService.isKnownPlaneAudience(audience)) {
      throw new NotFoundException(`Unknown plane audience: ${audience}`);
    }
    const orgId = (body.orgId ?? '').trim();
    const scopes = Array.isArray(body.scopes) ? body.scopes : [];
    const reason = (body.reason ?? '').trim();
    let principal: AuthorizedPlaneServicePrincipal;
    try {
      principal = authorizePlaneServicePrincipal(
        process.env.PLANE_SERVICE_PRINCIPALS_JSON ?? '',
        {
          serviceId: serviceId ?? '',
          credential: credential ?? '',
          audience,
          orgId,
          requestedScopes: scopes,
          reason,
          zdr: body.zdr,
        },
      );
    } catch (error) {
      if (error instanceof ServicePrincipalConfigurationError) {
        this.logger.error(
          'Service-principal registry unavailable; refusing token issuance',
        );
        throw new ServiceUnavailableException(
          'Service-principal issuance is unavailable',
        );
      }
      throw new ForbiddenException('Service principal is not authorized');
    }
    const response = this.convexTokenService.issuePlaneToken(audience, {
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
    const event = 'plane_service_token_issued';
    let audited = false;
    try {
      const auditIdentity = issuedTokenAuditIdentity(
        response.token,
        'plane-token',
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
          resource_id: audience,
          outcome: 'ok',
          details: {
            audience,
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
        'Durable audit unavailable; refusing service-token issuance',
      );
      throw new ServiceUnavailableException(
        'Service-principal issuance audit is unavailable',
      );
    }
    return response;
  }
}
