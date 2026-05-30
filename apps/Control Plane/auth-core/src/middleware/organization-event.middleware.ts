/**
 * Organization Event Middleware
 *
 * Intercepts Better Auth organization endpoints and publishes events to NATS
 * when organizations are created or members are added/removed.
 */

import { Injectable, NestMiddleware, Logger } from '@nestjs/common';
import { Request, Response, NextFunction } from 'express';
import { AuthEventPublisher } from '../internal/auth-event.publisher';

@Injectable()
export class OrganizationEventMiddleware implements NestMiddleware {
  private readonly logger = new Logger(OrganizationEventMiddleware.name);

  constructor(private readonly eventPublisher: AuthEventPublisher) {}

  use(req: Request, res: Response, next: NextFunction) {
    this.logger.debug(`🔍 Middleware intercepting: ${req.method} ${req.path}`);

    // Only intercept organization-related endpoints
    const orgCreatePath = '/api/auth/organization/create';
    const orgInvitePath = '/api/auth/organization/invite-member';
    const orgRemovePath = '/api/auth/organization/remove-member';

    if (!req.path.startsWith('/api/auth/organization')) {
      return next();
    }

    this.logger.log(`📥 Intercepting organization endpoint: ${req.path}`);

    // Store original methods
    const originalJson = res.json.bind(res);
    const originalSend = res.send.bind(res);

    // Intercept json responses
    res.json = (body: any) => {
      this.handleResponse(req, body);
      return originalJson(body);
    };

    // Intercept send responses
    res.send = (body: any) => {
      if (typeof body === 'string') {
        try {
          const parsed = JSON.parse(body);
          this.handleResponse(req, parsed);
        } catch {
          // Not JSON, ignore
        }
      } else if (typeof body === 'object') {
        this.handleResponse(req, body);
      }
      return originalSend(body);
    };

    next();
  }

  private async handleResponse(req: Request, body: any) {
    try {
      const path = req.path;

      // Handle organization creation
      if (path === '/api/auth/organization/create' && body?.id) {
        this.logger.log(
          `🎊 Organization created via Better Auth: ${body.name}`,
        );

        // Extract creator from members
        const creatorMember = body.members?.[0];

        await this.eventPublisher.publishOrganizationCreated({
          organizationId: body.id,
          name: body.name,
          slug: body.slug,
          creatorId: creatorMember?.userId || 'unknown',
          creatorEmail: '', // Not available in response
          metadata: body.metadata || {},
        });

        this.logger.log(`📢 Published organization.created event: ${body.id}`);
      }

      // Handle member invitation/addition
      if (path === '/api/auth/organization/invite-member' && body?.id) {
        this.logger.log(`👤 Member invited to organization via Better Auth`);

        // The invitation response may not have all details, but we can publish what we have
        if (body.organizationId && body.email) {
          await this.eventPublisher.publishOrganizationMemberAdded({
            organizationId: body.organizationId,
            organizationName: '', // Not available
            userId: body.userId || 'pending', // May be pending until accepted
            userEmail: body.email,
            role: body.role || 'member',
            invitedBy: undefined,
          });

          this.logger.log(`📢 Published member_added event for: ${body.email}`);
        }
      }

      // Handle member removal
      if (path === '/api/auth/organization/remove-member' && body?.success) {
        this.logger.log(`👋 Member removed from organization via Better Auth`);

        // Similar to invitation, publish what we have from the request
        const reqBody = (req as any).body;
        if (reqBody?.organizationId && reqBody?.userId) {
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
