import { Controller, Get, Req, UnauthorizedException } from '@nestjs/common';
import { MessagePattern, Payload } from '@nestjs/microservices';
import type { Request } from 'express';
import { auth } from './auth';
import { MicrosoftGraphService } from '../services/microsoft-graph.service';

/**
 * Convert Express's `IncomingHttpHeaders` (plain object) into a Web API
 * `Headers` instance. Better Auth's `auth.api.getSession({ headers })`
 * calls `.get('cookie')` on the headers; passing the raw Express headers
 * (even via `as unknown as Headers`) silently fails because the plain
 * object has no `.get()` method. See velion/velion-gap.md G31.
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

/**
 * NATS Auth Controller
 *
 * Handles NATS message patterns for authentication operations.
 * This allows other microservices (like user-service) to validate sessions
 * without making HTTP requests.
 *
 * HTTP endpoints use 'api/v2/auth' to avoid conflicts with Better Auth's /api/auth/* routes
 */
@Controller('api/v2/auth')
export class NatsAuthController {
  constructor(private readonly microsoftGraphService: MicrosoftGraphService) {}
  /**
   * Validate session from cookies sent via NATS
   *
   * Pattern: 'session.validate' (avoid 'auth.' prefix to prevent JetStream capture)
   * Payload: { cookies: string }
   * Returns: Session data with user information
   */
  @MessagePattern('session.validate')
  async validateSession(@Payload() data: { cookies: string }) {
    console.log('🔔 NATS: Received session.validate message');
    console.log('🍪 Cookies:', data.cookies?.substring(0, 50) + '...');

    try {
      // Parse cookies into Headers object
      const headers = new Headers();
      headers.set('cookie', data.cookies);

      // Use Better Auth to validate the session
      const session = await auth.api.getSession({ headers });

      console.log('✅ Session validated:', session ? 'YES' : 'NO');
      console.log('👤 User:', session?.user?.email);

      if (!session || !session.user) {
        console.log('❌ Invalid session - no user found');
        return {
          error: 'Invalid session',
          authenticated: false,
        };
      }

      console.log('✅ Returning valid session data');
      return {
        authenticated: true,
        user: {
          id: session.user.id,
          email: session.user.email,
          name: session.user.name,
          emailVerified: session.user.emailVerified,
          image: session.user.image,
        },
        session: {
          id: session.session.id,
          userId: session.session.userId,
          expiresAt: session.session.expiresAt,
        },
      };
    } catch (error) {
      console.error('❌ NATS session validation error:', error);
      return {
        error: 'Session validation failed',
        authenticated: false,
      };
    }
  }

  /**
   * Service account authentication for internal microservices
   *
   * Pattern: 'service.authenticate'
   * Payload: { serviceId: string, serviceSecret: string }
   * Returns: Internal service secret to use as X-Internal-Service-Secret header
   *
   * This allows trusted internal services to bypass admin API authentication
   * by including the service secret in their HTTP requests.
   */
  @MessagePattern('service.authenticate')
  async authenticateService(
    @Payload() data: { serviceId: string; serviceSecret: string },
  ): Promise<{
    authenticated: boolean;
    serviceSecret?: string;
    serviceId?: string;
    error?: string;
  }> {
    console.log('🔔 NATS: Received service.authenticate message');
    console.log('🔑 Service ID:', data.serviceId);

    try {
      // Validate service credentials against environment variables
      const validServiceIds = (
        process.env.INTERNAL_SERVICE_IDS || 'admin-service,user-service'
      ).split(',');
      const expectedSecret =
        process.env.INTERNAL_SERVICE_SECRET || process.env.INTERNAL_API_KEY;

      if (!expectedSecret) {
        console.error('❌ INTERNAL_SERVICE_SECRET not configured');
        return {
          error: 'Service authentication not configured',
          authenticated: false,
        };
      }

      if (!validServiceIds.includes(data.serviceId)) {
        console.log('❌ Invalid service ID:', data.serviceId);
        return {
          error: 'Invalid service ID',
          authenticated: false,
        };
      }

      if (data.serviceSecret !== expectedSecret) {
        console.log('❌ Invalid service secret');
        return {
          error: 'Invalid service secret',
          authenticated: false,
        };
      }

      const response = {
        authenticated: true,
        serviceSecret: expectedSecret,
        serviceId: data.serviceId,
      };

      console.log('✅ Service authenticated successfully');
      console.log('📤 Sending response:', JSON.stringify(response));

      return response;
    } catch (error) {
      console.error('❌ NATS service authentication error:', error);
      const errorResponse = {
        error: 'Service authentication failed',
        authenticated: false,
      };
      console.log('📤 Sending error response:', JSON.stringify(errorResponse));
      return errorResponse;
    }
  }

  /**
   * Health check endpoint for NATS
   *
   * Pattern: 'health.check'
   */
  @MessagePattern('health.check')
  healthCheck() {
    return {
      status: 'healthy',
      service: 'auth-service',
      timestamp: new Date().toISOString(),
    };
  }

  /**
   * Get user profile with Microsoft Graph data
   *
   * HTTP GET /api/auth/user/profile
   * Returns user profile enriched with Microsoft 365 data if available
   */
  @Get('user/profile')
  async getUserProfile(@Req() request: Request) {
    try {
      // Get session from Better Auth
      const session = await auth.api.getSession({
        headers: toWebHeaders(request.headers),
      });

      if (!session || !session.user) {
        throw new UnauthorizedException('Not authenticated');
      }

      const user = session.user;
      console.log('📋 Fetching profile for user:', user.email);

      // Check if user has Microsoft OAuth account
      const { valid, accessToken } =
        this.microsoftGraphService.hasValidMicrosoftAccount(user);

      if (valid && accessToken) {
        console.log(
          '🔑 User has valid Microsoft account, fetching Graph data...',
        );
        try {
          const graphProfile =
            await this.microsoftGraphService.getUserProfile(accessToken);

          console.log(
            '✅ Microsoft Graph profile fetched:',
            graphProfile.displayName,
          );

          return {
            id: user.id,
            email: user.email,
            name: user.name,
            displayName: graphProfile.displayName || user.name,
            givenName: graphProfile.givenName,
            surname: graphProfile.surname,
            jobTitle: graphProfile.jobTitle,
            officeLocation: graphProfile.officeLocation,
            mobilePhone: graphProfile.mobilePhone,
            image: graphProfile.photo || user.image,
            emailVerified: user.emailVerified,
            source: 'microsoft-graph',
          };
        } catch (error) {
          console.log(
            '⚠️ Failed to fetch Microsoft Graph data, using local data:',
            error instanceof Error ? error.message : String(error),
          );
          // Fall through to return local user data
        }
      }

      // Return local user data if no Microsoft account or Graph API fails
      console.log('📌 Returning local user data');
      return {
        id: user.id,
        email: user.email,
        name: user.name,
        displayName: user.name,
        image: user.image,
        emailVerified: user.emailVerified,
        source: 'local',
      };
    } catch (error) {
      console.error('❌ Error in getUserProfile:', error);
      throw error;
    }
  }
}
