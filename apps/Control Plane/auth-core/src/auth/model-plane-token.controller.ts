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
 *   - **POST /api/model-plane/internal-token**: from an `X-Internal-Api-Key`
 *     header plus `orgId` + `userId` in the body. For service-to-service
 *     calls (e.g. cron jobs, background workers) that don't have a user
 *     session but need to talk to the gateway. Mirrors the existing
 *     internal-key pattern used by `convex-auth.controller.ts`.
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
  UnauthorizedException,
} from '@nestjs/common';
import { ApiOperation, ApiResponse, ApiTags } from '@nestjs/swagger';
import type { Request } from 'express';

import { auth } from './auth';
import { ConvexTokenService } from './convex-token.service';

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
  userId?: string;
  orgId?: string;
  email?: string;
  scopes?: readonly string[];
}

interface SessionContextResponse {
  orgId?: string;
  role?: string;
}

@ApiTags('Model Plane Auth')
@Controller('api/model-plane')
export class ModelPlaneTokenController {
  private readonly logger = new Logger(ModelPlaneTokenController.name);

  private readonly userServiceUrl = (
    process.env.USER_SERVICE_URL || 'http://user-service:3012'
  ).replace(/\/+$/, '');

  private readonly internalApiKey =
    process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET || '';

  constructor(private readonly convexTokenService: ConvexTokenService) {}

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
    const session = await auth.api.getSession({
      headers: toWebHeaders(request.headers),
    });

    if (!session?.user?.id) {
      throw new UnauthorizedException('Authentication required');
    }

    const sessionContext = await this.fetchSessionContext({
      userId: session.user.id,
      email: session.user.email ?? '',
      name: session.user.name ?? null,
    });

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
    const role = (sessionContext.role ?? '').toLowerCase();
    const scopes = role === 'owner' || role === 'admin' ? ['admin'] : undefined;

    const bundle = this.convexTokenService.issueModelPlaneToken({
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
   * Service-to-service token issuance. Caller must present a valid
   * `X-Internal-Api-Key` header (matching `INTERNAL_API_KEY` or
   * `INTERNAL_SERVICE_SECRET`). Body must supply `userId` + `orgId`.
   *
   * Used by background workers / cron tasks that need to call the
   * gateway without a user session. Production should rotate this
   * shared secret and limit the surface to internal network only.
   */
  @Post('internal-token')
  @ApiOperation({
    summary:
      'Mint a Model Plane gateway JWT for service-to-service traffic (internal key required)',
  })
  async issueInternalToken(
    @NestHeaders('x-internal-api-key') apiKey: string | undefined,
    @Body() body: ModelPlaneInternalTokenBody,
  ) {
    if (!this.internalApiKey) {
      this.logger.error(
        'INTERNAL_API_KEY / INTERNAL_SERVICE_SECRET not configured; refusing internal token issuance',
      );
      throw new ForbiddenException('Internal token issuance not configured');
    }
    if (!apiKey || apiKey !== this.internalApiKey) {
      throw new ForbiddenException('Invalid internal API key');
    }
    const userId = (body.userId ?? '').trim();
    const orgId = (body.orgId ?? '').trim();
    if (!userId || !orgId) {
      throw new BadRequestException(
        'userId and orgId are required for internal token issuance',
      );
    }
    return this.convexTokenService.issueModelPlaneToken({
      userId,
      orgId,
      email: body.email,
      scopes: body.scopes,
    });
  }

  private async fetchSessionContext(actor: {
    userId: string;
    email: string;
    name: string | null;
  }): Promise<SessionContextResponse | null> {
    try {
      const response = await fetch(
        `${this.userServiceUrl}/api/v1/me/session-context`,
        {
          method: 'GET',
          headers: {
            'Content-Type': 'application/json',
            'X-Internal-Api-Key': this.internalApiKey,
            'X-User-Id': actor.userId,
            'X-User-Email': actor.email,
            ...(actor.name ? { 'X-User-Name': actor.name } : {}),
          },
          cache: 'no-store',
        },
      );

      if (!response.ok) {
        this.logger.warn(
          `Failed to resolve user-core session context for Model Plane token: ${response.status}`,
        );
        return null;
      }

      return (await response.json()) as SessionContextResponse;
    } catch (error) {
      this.logger.warn(
        `Failed to resolve user-core session context for Model Plane token: ${error instanceof Error ? error.message : String(error)}`,
      );
      return null;
    }
  }
}
