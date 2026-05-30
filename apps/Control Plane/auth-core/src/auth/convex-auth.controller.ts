import {
  Controller,
  Get,
  Logger,
  Req,
  UnauthorizedException,
} from '@nestjs/common';
import { ApiOperation, ApiResponse, ApiTags } from '@nestjs/swagger';
import type { Request } from 'express';

import { auth } from './auth';
import { ConvexTokenService } from './convex-token.service';

/**
 * Convert Express's `IncomingHttpHeaders` (plain object) into a Web API
 * `Headers` instance. Better Auth's `auth.api.getSession({ headers })`
 * expects a Web Headers object and calls `.get('cookie')` on it; passing
 * the raw Express headers (even via `as unknown as Headers`) silently
 * fails because the plain object has no `.get()` method and thus the
 * cookie is never seen — causing every session lookup to 401. See
 * `velion/velion-gap.md` G31.
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

type SessionContextResponse = {
  orgId?: string;
  role?: string;
};

@ApiTags('Convex Auth')
@Controller('api/convex-auth')
export class ConvexAuthController {
  private readonly logger = new Logger(ConvexAuthController.name);
  private readonly userServiceUrl = (
    process.env.USER_SERVICE_URL || 'http://user-service:3012'
  ).replace(/\/+$/, '');
  private readonly internalApiKey =
    process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET || '';

  constructor(private readonly convexTokenService: ConvexTokenService) {}

  @Get('jwks')
  @ApiOperation({
    summary: 'JWKS endpoint for Convex custom JWT verification',
  })
  getJwks() {
    return this.convexTokenService.getJwks();
  }

  @Get('token')
  @ApiOperation({
    summary:
      'Mint a short-lived Convex auth token from the active Better Auth session',
  })
  @ApiResponse({
    status: 200,
    description: 'Short-lived Convex JWT for browser subscriptions',
  })
  async getToken(@Req() request: Request) {
    const session = await auth.api.getSession({
      headers: toWebHeaders(request.headers),
    });

    if (!session?.user?.id || !session.user.email) {
      throw new UnauthorizedException('Authentication required');
    }

    const sessionContext = await this.fetchSessionContext({
      userId: session.user.id,
      email: session.user.email,
      name: session.user.name,
    });

    const token = this.convexTokenService.issueToken({
      externalAuthId: session.user.id,
      email: session.user.email,
      name: session.user.name,
      activeOrgId: sessionContext?.orgId,
      activeOrgRole: sessionContext?.role,
    });

    return {
      ...token,
      userId: session.user.id,
      email: session.user.email,
      activeOrgId: sessionContext?.orgId ?? null,
      activeOrgRole: sessionContext?.role ?? null,
    };
  }

  private async fetchSessionContext(actor: {
    userId: string;
    email: string;
    name?: string | null;
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
          `Failed to resolve user-core session context for Convex token: ${response.status}`,
        );
        return null;
      }

      return (await response.json()) as SessionContextResponse;
    } catch (error) {
      this.logger.warn(
        `Failed to resolve user-core session context for Convex token: ${error instanceof Error ? error.message : String(error)}`,
      );
      return null;
    }
  }
}
