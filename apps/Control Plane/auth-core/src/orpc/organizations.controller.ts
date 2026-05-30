import {
  Controller,
  Get,
  Req,
  Res,
  HttpCode,
  HttpStatus,
} from '@nestjs/common';
import {
  ApiTags,
  ApiOperation,
  ApiResponse,
  ApiBearerAuth,
} from '@nestjs/swagger';
import type { Request, Response } from 'express';
import { auth } from '../auth/auth';

interface ErrorWithStatus extends Error {
  status?: number;
}

/**
 * Organizations Controller
 *
 * Provides direct REST API endpoints for organization management that match
 * the frontend client expectations. This controller acts as a bridge between
 * the frontend and the oRPC organization procedures.
 */
@ApiTags('Organizations API (v2)')
@Controller('api/v2/organizations')
@ApiBearerAuth('bearer')
export class OrganizationsController {
  private createContext(request: Request, response: Response) {
    const headers = new Headers();

    // Copy relevant headers
    if (request.get('cookie')) {
      headers.set('cookie', request.get('cookie')!);
    }
    if (request.get('authorization')) {
      headers.set('authorization', request.get('authorization')!);
    }
    if (request.get('user-agent')) {
      headers.set('user-agent', request.get('user-agent')!);
    }
    if (request.get('x-forwarded-for')) {
      headers.set('x-forwarded-for', request.get('x-forwarded-for')!);
    }

    return {
      headers,
      setHeader: (name: string, value: string | string[]) => {
        if (Array.isArray(value)) {
          value.forEach((v) => response.append(name, v));
        } else {
          response.set(name, value);
        }
      },
    };
  }

  /**
   * List Organizations
   *
   * @description Get all organizations for the authenticated user
   */
  @Get()
  @ApiOperation({
    summary: 'List organizations',
    description: 'Retrieve all organizations that the user has access to',
  })
  @ApiResponse({
    status: 200,
    description: 'Organizations retrieved successfully',
    schema: {
      type: 'object',
      properties: {
        organizations: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              id: { type: 'string' },
              name: { type: 'string' },
              slug: { type: 'string' },
              logo: { type: 'string', nullable: true },
              role: { type: 'string', enum: ['owner', 'admin', 'member'] },
              memberCount: { type: 'number' },
              createdAt: { type: 'string', format: 'date-time' },
            },
          },
        },
        total: { type: 'number' },
      },
    },
  })
  @ApiResponse({
    status: 401,
    description: 'Authentication required',
  })
  @HttpCode(HttpStatus.OK)
  async getOrganizations(
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<void> {
    const requestId = Math.random().toString(36).substring(7);
    const startTime = Date.now();

    console.log(`🔄 Organizations API GET /organizations`, {
      requestId,
      timestamp: new Date().toISOString(),
    });

    try {
      // Resolve the authenticated user's session so we can pass their ID to org-core.
      const context = this.createContext(request, response);
      const session = await auth.api.getSession({ headers: context.headers });

      if (!session?.user?.id) {
        const duration = Date.now() - startTime;
        console.warn(`⚠️ [${requestId}] No session (${duration}ms)`);
        response.status(401).json({
          error: 'Authentication required',
          code: 'UNAUTHORIZED',
          timestamp: new Date().toISOString(),
          requestId,
        });
        return;
      }

      // Proxy to org-core which holds the authoritative organisation records.
      // Better Auth's own organisation plugin table is not used by this system.
      const orgServiceUrl = (
        process.env.ORG_SERVICE_URL ?? 'http://org-core:8080'
      ).replace(/\/$/, '');

      console.log(
        `🔧 [${requestId}] Proxying to org-core: ${orgServiceUrl}/api/v1/organizations`,
      );

      const orgResp = await fetch(`${orgServiceUrl}/api/v1/organizations`, {
        headers: { 'x-user-id': session.user.id },
      });

      const duration = Date.now() - startTime;

      if (!orgResp.ok) {
        const errText = await orgResp.text();
        console.error(
          `❌ [${requestId}] org-core error ${orgResp.status} (${duration}ms): ${errText}`,
        );
        response.status(orgResp.status).json({
          error: 'Failed to fetch organizations',
          code: 'ORG_SERVICE_ERROR',
          timestamp: new Date().toISOString(),
          requestId,
        });
        return;
      }

      // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
      const body = await orgResp.json();
      console.log(`✅ [${requestId}] org-core responded in ${duration}ms`);

      // org-core returns { organizations: [...] } or a bare array — normalise both.

      const organizations: unknown[] = Array.isArray(body)
        ? (body as unknown[])
        : Array.isArray((body as Record<string, unknown>).organizations)
          ? ((body as Record<string, unknown>).organizations as unknown[])
          : [];

      response.json({
        organizations,
        total: organizations.length,
        success: true,
      });
    } catch (error: unknown) {
      const duration = Date.now() - startTime;
      console.error(`❌ [${requestId}] Failed after ${duration}ms:`, error);

      const errorMessage =
        error instanceof Error ? error.message : 'Internal server error';
      const statusCode =
        (error as ErrorWithStatus).status || HttpStatus.INTERNAL_SERVER_ERROR;

      response.status(statusCode).json({
        error: errorMessage,
        code: 'INTERNAL_ERROR',
        timestamp: new Date().toISOString(),
        requestId,
      });
    }
  }
}
