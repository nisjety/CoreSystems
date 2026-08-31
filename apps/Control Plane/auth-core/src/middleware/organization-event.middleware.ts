/**
 * Organization Event Middleware
 *
 * Intercepts Better Auth organization endpoints and publishes events to NATS
 * when organizations are created or members are added/removed.
 */

import { Injectable, NestMiddleware, Logger } from '@nestjs/common';
import { Request, Response, NextFunction } from 'express';
import { AuthEventPublisher } from '../internal/auth-event.publisher';

/**
 * Shape of the fields this middleware reads off Better Auth organization
 * endpoint responses/requests. Better Auth does not export a typed response
 * contract for these routes, so this interface only covers the properties
 * actually accessed below; everything is optional because the exact payload
 * varies per endpoint.
 */
interface BetterAuthOrganizationBody {
  id?: string;
  name?: string;
  slug?: string;
  metadata?: Record<string, unknown>;
  members?: Array<{ userId?: string }>;
  success?: boolean;
  organizationId?: string;
  email?: string;
  userId?: string;
  role?: string;
}

@Injectable()
export class OrganizationEventMiddleware implements NestMiddleware {
  private readonly logger = new Logger(OrganizationEventMiddleware.name);

  constructor(private readonly eventPublisher: AuthEventPublisher) {}

  use(req: Request, res: Response, next: NextFunction) {
    this.logger.debug(`🔍 Middleware intercepting: ${req.method} ${req.path}`);

    if (!req.path.startsWith('/api/auth/organization')) {
      return next();
    }

    this.logger.log(`📥 Intercepting organization endpoint: ${req.path}`);

    // Store original methods
    const originalJson = res.json.bind(res) as typeof res.json;
    const originalSend = res.send.bind(res) as typeof res.send;

    // Intercept json responses
    res.json = (body: unknown) => {
      void this.handleResponse(req, body);
      return originalJson(body);
    };

    // Intercept send responses
    res.send = (body: unknown) => {
      if (typeof body === 'string') {
        try {
          const parsed: unknown = JSON.parse(body);
          void this.handleResponse(req, parsed);
        } catch {
          // Not JSON, ignore
        }
      } else if (typeof body === 'object') {
        void this.handleResponse(req, body);
      }
      return originalSend(body);
    };

    next();
  }

  private async handleResponse(req: Request, body: unknown): Promise<void> {
    try {
      const path = req.path;
      const parsedBody = body as BetterAuthOrganizationBody | undefined;

      // Handle organization creation
      if (path === '/api/auth/organization/create' && parsedBody?.id) {
        this.logger.log(
          `🎊 Organization created via Better Auth: ${parsedBody.name}`,
        );

        // Extract creator from members
        const creatorMember = parsedBody.members?.[0];

        await this.eventPublisher.publishOrganizationCreated({
          organizationId: parsedBody.id,
          name: parsedBody.name || '',
          slug: parsedBody.slug || '',
          creatorId: creatorMember?.userId || 'unknown',
          creatorEmail: '', // Not available in response
          metadata: parsedBody.metadata || {},
        });

        this.logger.log(
          `📢 Published organization.created event: ${parsedBody.id}`,
        );
      }

      // Handle member invitation/addition
      if (path === '/api/auth/organization/invite-member' && parsedBody?.id) {
        this.logger.log(`👤 Member invited to organization via Better Auth`);

        // The invitation response may not have all details, but we can publish what we have
        if (parsedBody.organizationId && parsedBody.email) {
          await this.eventPublisher.publishOrganizationMemberAdded({
            organizationId: parsedBody.organizationId,
            organizationName: '', // Not available
            userId: parsedBody.userId || 'pending', // May be pending until accepted
            userEmail: parsedBody.email,
            role: parsedBody.role || 'member',
            invitedBy: undefined,
          });

          this.logger.log(
            `📢 Published member_added event for: ${parsedBody.email}`,
          );
        }
      }

      // Handle member removal
      if (
        path === '/api/auth/organization/remove-member' &&
        parsedBody?.success
      ) {
        this.logger.log(`👋 Member removed from organization via Better Auth`);

        // Similar to invitation, publish what we have from the request
        const reqBody = req.body as BetterAuthOrganizationBody | undefined;
        if (reqBody?.organizationId && reqBody.userId) {
          await this.eventPublisher.publishOrganizationMemberRemoved({
            organizationId: reqBody.organizationId,
            organizationName: '', // Not available
            userId: reqBody.userId,
            userEmail: '', // Not available
            removedBy: undefined,
          });

          this.logger.log(`📢 Published member_removed event`);
        }
      }
    } catch (error) {
      this.logger.error('Failed to publish organization event:', error);
      // Don't throw - we don't want to break the response
    }
  }
}
