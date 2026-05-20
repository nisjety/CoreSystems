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
  UnauthorizedException,
} from '@nestjs/common';
import { ApiOperation, ApiResponse, ApiTags } from '@nestjs/swagger';
import type { Request } from 'express';

import { auth } from './auth';
import { ConvexTokenService, type PlaneAudience } from './convex-token.service';

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

interface PlaneInternalTokenBody {
  userId?: string;
  orgId?: string;
  email?: string;
  scopes?: readonly string[];
}

interface SessionContextResponse {
  orgId?: string;
  role?: string;
}

@ApiTags('Plane Auth')
@Controller('api')
export class PlaneTokenController {
  private readonly logger = new Logger(PlaneTokenController.name);

  private readonly userServiceUrl = (
    process.env.USER_SERVICE_URL || 'http://user-service:3012'
  ).replace(/\/+$/, '');

  private readonly internalApiKey =
    process.env.INTERNAL_API_KEY ||
    process.env.INTERNAL_SERVICE_SECRET ||
    '';

  constructor(private readonly convexTokenService: ConvexTokenService) {}

  /**
   * Mint a plane-scoped JWT from the active Better Auth session. The
   * audience comes from the URL path so the controller can serve every
   * non-Model-Plane plane without duplicate route handlers.
   *
   * Routes registered: `/api/:audience/token` for any `audience` in
   * `PlaneAudience`. Unknown audiences → 404, no session → 401, no active
   * org → 400 (fail-closed; a token with empty `org_id` would be rejected
   * downstream anyway, so surface it here as a clear error).
   */
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
    if (!this.convexTokenService.isKnownPlaneAudience(audience)) {
      throw new NotFoundException(`Unknown plane audience: ${audience}`);
    }

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
      throw new BadRequestException(
        'No active organisation on session; user-core returned no orgId',
      );
    }

    const role = (sessionContext.role ?? '').toLowerCase();
    const scopes =
      role === 'owner' || role === 'admin' ? ['admin'] : undefined;

    const bundle = this.convexTokenService.issuePlaneToken(
      audience as PlaneAudience,
      {
        userId: session.user.id,
        orgId: sessionContext.orgId,
        email: session.user.email ?? undefined,
        scopes,
      },
    );

    return {
      ...bundle,
      userId: session.user.id,
      orgId: sessionContext.orgId,
      role: sessionContext.role ?? null,
    };
  }

  /**
   * Mint a plane-scoped JWT for a service-to-service caller. Caller must
   * present a valid `X-Internal-Api-Key` header. Body must supply
   * `userId` + `orgId` so the resulting token carries a real tenant
   * identity (auth-core does not synthesise one — that would defeat the
   * Wave 3 multi-tenant trust contract).
   */
  @Post(':audience/internal-token')
  @ApiOperation({
    summary:
      'Mint a plane-scoped JWT for service-to-service traffic (internal key required)',
  })
  async issueInternalToken(
    @Param('audience') audience: string,
    @NestHeaders('x-internal-api-key') apiKey: string | undefined,
    @Body() body: PlaneInternalTokenBody,
  ) {
    if (!this.convexTokenService.isKnownPlaneAudience(audience)) {
      throw new NotFoundException(`Unknown plane audience: ${audience}`);
    }
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
    return this.convexTokenService.issuePlaneToken(
      audience as PlaneAudience,
      {
        userId,
        orgId,
        email: body.email,
        scopes: body.scopes,
      },
    );
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
          `Failed to resolve user-core session context for plane token: ${response.status}`,
        );
        return null;
      }

      return (await response.json()) as SessionContextResponse;
    } catch (error) {
      this.logger.warn(
        `Failed to resolve user-core session context for plane token: ${error instanceof Error ? error.message : String(error)}`,
      );
      return null;
    }
  }
}
