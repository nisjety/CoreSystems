/**
 * Phase A · A1.1 — generic plane-token issuance.
 *
 * Issues short-lived RS256 JWTs for every non-Model-Plane audience
 * (`data-plane`, `quarry`, `ingestion`, `control-plane`,
 * `application-plane`). Mirrors the Model-Plane controller pattern but
 * keeps the per-audience route slug so verevon's call sites match the
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
 * — unknown audiences surface as 404 so a typo on the verevon side never
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
import { randomUUID } from 'node:crypto';
import type { Request } from 'express';

import { DirectNatsService } from '../nats/direct-nats.service';
import { issuedTokenAuditIdentity } from './audit-event-identity';
import { auth } from './auth';
import {
  ConvexTokenService,
  type ServiceRetentionPosture,
  type VerifiedPlaneSubjectToken,
  type PlaneAudience,
} from './convex-token.service';
import {
  assertServiceOrganizationBinding,
  resolveInteractiveRetentionPosture,
  resolveTenantServiceRetentionPosture,
} from './interactive-retention-policy';
import {
  authorizePlaneServicePrincipal,
  authorizePlaneServiceTokenExchange,
  loadPlaneServicePrincipalRegistry,
  ServicePrincipalConfigurationError,
  type AuthorizedPlaneServicePrincipal,
  type AuthorizedPlaneServiceTokenExchange,
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
}

/**
 * Delegated-identity token exchange (auth-core §1). RFC 8693 shapes the
 * body deliberately follows: `subjectToken` is the caller's own
 * already-verified `aud=session-core` bearer (execution-core's delegated
 * session bearer), never a caller-selected identity.
 */
interface PlaneTokenExchangeBody {
  orgId?: string;
  scopes?: readonly string[];
  reason?: string;
  subjectToken?: string;
  subjectTokenType?: string;
}

const TOKEN_EXCHANGE_SUBJECT_TOKEN_TYPE =
  'urn:ietf:params:oauth:token-type:jwt';

/**
 * Hard-coded, not registry-derived (T-3): the only subject-token audience
 * `verifyPlaneSubjectToken` will ever cryptographically accept for a token
 * exchange, regardless of what any principal's `tokenExchange` registry
 * entry claims. A registry entry can narrow which SERVICE PRINCIPALS may
 * use this capability at all, but can never widen which subject audiences
 * are acceptable in the first place.
 */
const TOKEN_EXCHANGE_ALLOWED_SUBJECT_AUDIENCES: readonly PlaneAudience[] = [
  'session-core',
];

/** Shorter than every audience's normal `/internal-token` default (300s) —
 * minted fresh per tool call, never cached, for a read that completes in
 * one HTTP round trip. See design-2 §1.3. Each token also carries its own
 * `jti` so the resource server can consume it exactly once. */
const TOKEN_EXCHANGE_TTL_SECONDS = 90;

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
   * `PlaneAudience`. The service-only audiences — `control-policy` and
   * `orchestrator-core`, listed once in `SERVICE_ONLY_PLANE_AUDIENCES` — are
   * deliberately excluded and available only through the scoped
   * service-principal path. Unknown or service-only audiences → 404, no
   * session → 401, no active org → 400
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

    // Resolved asynchronously against org-core (stored intent + plan
    // entitlement). If org-core cannot be read and no last-known posture is
    // usable this throws a 503 (retention_posture_unavailable) and NO token is
    // issued; it never degrades to zdr:false. Let it propagate — see
    // interactive-retention-policy.ts.
    const retentionPosture = await resolveInteractiveRetentionPosture(
      sessionContext.orgId,
    );

    const bundle = this.convexTokenService.issuePlaneToken(audience, {
      userId: session.user.id,
      orgId: sessionContext.orgId,
      email: session.user.email ?? undefined,
      scopes,
      retentionPosture,
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
    @NestHeaders('x-zdr') requestedZdr?: string,
  ) {
    if (!this.convexTokenService.isKnownPlaneAudience(audience)) {
      throw new NotFoundException(`Unknown plane audience: ${audience}`);
    }
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
          audience,
          orgId,
          requestedScopes: scopes,
          reason,
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
    // A `directory`-bound principal (the production form for a worker that acts
    // for whichever organization the work belongs to) may only mint for an
    // organization org-core's directory holds. Checked here, explicitly, after
    // the caller is authorized and before anything else is resolved or signed:
    // 403 for an organization the directory does not hold, 503
    // (organization_directory_unavailable) when org-core cannot be read, and
    // NO token in either case.
    await assertServiceOrganizationBinding(principal);
    // A `tenant` principal's posture is the ORG's, so it is resolved here,
    // explicitly, after the caller is authorized and before anything is signed.
    // If org-core cannot be read and no last-known posture is usable this throws
    // a 503 (retention_posture_unavailable) and NO token is issued; it never
    // falls back to the registry's zdr:true default, which would stall every
    // non-ZDR org's background work, nor to false, which would persist a ZDR
    // org's. Every other principal keeps exactly what its registry row says.
    const retentionPosture: ServiceRetentionPosture =
      principal.retention === 'tenant'
        ? await resolveTenantServiceRetentionPosture(principal)
        : { zdr: principal.zdr, authority: 'service-principal-policy' };
    const response = this.convexTokenService.issuePlaneToken(audience, {
      userId: principal.subject,
      orgId: principal.orgId,
      scopes: principal.scopes,
      principalType: 'service',
      serviceId: principal.serviceId,
      reason: principal.reason,
      retentionPosture,
    });
    const event = 'plane_service_token_issued';
    let audited = false;
    try {
      const auditIdentity = issuedTokenAuditIdentity(
        response.token,
        'plane-token',
      );
      await this.directNats.publishAuditDurable(
        `verevon.audit.v2.control.auth-core.${event}`,
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
            zdr: retentionPosture.zdr,
            zdr_source: retentionPosture.source ?? 'registry',
            org_binding: principal.orgBinding,
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

  /**
   * RFC 8693-style token exchange (delegated-identity design 2, §1). A
   * registered service principal presents its own, already-independently-
   * verified `aud=session-core` bearer as `subjectToken` and receives back a
   * new, narrower, short-lived token for `audience` that additionally
   * carries `acting_user_id` — the subject token's own user, never a
   * caller-selected value.
   *
   * Distinct code path from `issueInternalToken` above (T-5): the minted
   * token's `zdr` is the SUBJECT token's own verified claim, not the
   * registry's `retentionByAudience` default, and reaching this endpoint at
   * all additionally requires the principal's `tokenExchange` registry
   * field to name the subject token's audience for this target — absent by
   * default, so no currently-registered principal gains this capability
   * until a deployment explicitly opts it in.
   */
  @Post(':audience/token-exchange')
  @ApiOperation({
    summary:
      'RFC 8693 token exchange: mint a delegated plane token bound to a verified subject token\'s acting user',
  })
  async issueTokenExchange(
    @Param('audience') audience: string,
    @NestHeaders('x-service-id') serviceId: string | undefined,
    @NestHeaders('x-service-api-key') credential: string | undefined,
    @Body() body: PlaneTokenExchangeBody,
  ) {
    if (!this.convexTokenService.isKnownPlaneAudience(audience)) {
      throw new NotFoundException(`Unknown plane audience: ${audience}`);
    }

    const subjectToken = (body.subjectToken ?? '').trim();
    const subjectTokenType = (body.subjectTokenType ?? '').trim();
    if (!subjectToken || subjectTokenType !== TOKEN_EXCHANGE_SUBJECT_TOKEN_TYPE) {
      throw new BadRequestException(
        'subjectToken and subjectTokenType are required for token exchange',
      );
    }

    let subject: VerifiedPlaneSubjectToken;
    try {
      subject = this.convexTokenService.verifyPlaneSubjectToken(
        subjectToken,
        TOKEN_EXCHANGE_ALLOWED_SUBJECT_AUDIENCES,
      );
    } catch {
      throw new UnauthorizedException('Subject token is not verifiable');
    }

    const orgId = (body.orgId ?? '').trim();
    if (!orgId || orgId !== subject.orgId) {
      throw new BadRequestException(
        'Request orgId does not match the subject token',
      );
    }

    const scopes = Array.isArray(body.scopes) ? body.scopes : [];
    const reason = (body.reason ?? '').trim();
    let principal: AuthorizedPlaneServiceTokenExchange;
    try {
      principal = authorizePlaneServiceTokenExchange(
        loadPlaneServicePrincipalRegistry(),
        {
          serviceId: serviceId ?? '',
          credential: credential ?? '',
          audience,
          orgId,
          requestedScopes: scopes,
          reason,
          subjectAudience: subject.audience,
        },
      );
    } catch (error) {
      if (error instanceof ServicePrincipalConfigurationError) {
        this.logger.error(
          'Service-principal registry unavailable; refusing token exchange',
        );
        throw new ServiceUnavailableException(
          'Service-principal issuance is unavailable',
        );
      }
      throw new ForbiddenException(
        'Service principal is not authorized for this token exchange',
      );
    }

    const response = this.convexTokenService.issuePlaneToken(audience, {
      userId: principal.subject,
      orgId: principal.orgId,
      scopes: principal.scopes,
      principalType: 'service',
      serviceId: principal.serviceId,
      reason: principal.reason,
      actingUserId: subject.userId,
      // Deliberately the SUBJECT token's own zdr, not the registry default
      // — bypasses issuedTokenZdr's normal service-principal-registry
      // sourcing on purpose (T-5). A ZDR-scoped user's run can never mint a
      // persistent-posture delegated token through this path.
      retentionPosture: { zdr: subject.zdr, authority: 'service-principal-policy' },
      ttlSecondsOverride: TOKEN_EXCHANGE_TTL_SECONDS,
      // T-2/A-1: the exchanged token is a complete standalone bearer, so it
      // carries a fresh unguessable id that integration-corev2 consumes on
      // first use. The single-use guard belongs on THIS token (one per tool
      // call), not on the run's session-core bearer (one per run).
      tokenId: randomUUID(),
    });

    const event = 'plane_service_token_exchanged';
    let audited = false;
    try {
      const auditIdentity = issuedTokenAuditIdentity(
        response.token,
        'plane-token',
      );
      await this.directNats.publishAuditDurable(
        `verevon.audit.v2.control.auth-core.${event}`,
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
            acting_user_id: subject.userId,
            subject_audience: subject.audience,
          },
        },
      );
      audited = true;
    } catch {
      // Missing/failed PubAck preserves the endpoint's fail-closed contract.
    }
    if (!audited) {
      this.logger.error(
        'Durable audit unavailable; refusing token-exchange issuance',
      );
      throw new ServiceUnavailableException(
        'Token-exchange issuance audit is unavailable',
      );
    }
    return response;
  }
}
