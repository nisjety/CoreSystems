import { Controller, Logger } from '@nestjs/common';
import { GrpcMethod } from '@nestjs/microservices';
import { Metadata } from '@grpc/grpc-js';
import { auth } from '../auth/auth';
import * as crypto from 'crypto';
import type {
  SignUpRequest,
  SignUpResponse,
  SignInRequest,
  SignInResponse,
  SignOutRequest,
  SignOutResponse,
  GetCurrentUserRequest,
  GetCurrentUserResponse,
  HealthCheckRequest,
  HealthCheckResponse,
} from './auth/v1/auth';
import type { AuthUser } from './auth/v1/auth';

/** Service auth key — Data Plane services must send this in gRPC metadata */
const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET;
if (!INTERNAL_API_KEY) {
  throw new Error(
    'INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET env var is required. ' +
      'Refusing to start without service-to-service authentication.',
  );
}

@Controller()
export class AuthGrpcController {
  private readonly logger = new Logger(AuthGrpcController.name);

  constructor() {}

  // ── Cross-plane: Token Validation (called by Data Plane) ─────────────

  @GrpcMethod('TokenValidationService', 'ValidateToken')
  async validateToken(
    data: { token: string },
    metadata: Metadata,
  ): Promise<{
    valid: boolean;
    userId: string;
    orgId: string;
    email: string;
    name: string;
    role: string;
    sessionId: string;
    permissions: string[];
    expiresAt?: { seconds: number; nanos: number };
  }> {
    // Verify service-to-service auth key from gRPC metadata
    const serviceKey = metadata?.get('x-service-auth')?.[0];
    if (!serviceKey || serviceKey !== INTERNAL_API_KEY) {
      this.logger.warn('ValidateToken called without valid x-service-auth');
      return {
        valid: false,
        userId: '',
        orgId: '',
        email: '',
        name: '',
        role: '',
        sessionId: '',
        permissions: [],
      };
    }

    if (!data.token) {
      return {
        valid: false,
        userId: '',
        orgId: '',
        email: '',
        name: '',
        role: '',
        sessionId: '',
        permissions: [],
      };
    }

    try {
      // Use Better Auth's getSession API — works with Bearer tokens
      const session = await auth.api.getSession({
        headers: new Headers({ authorization: `Bearer ${data.token}` }),
      });

      if (!session || !session.user) {
        return {
          valid: false,
          userId: '',
          orgId: '',
          email: '',
          name: '',
          role: '',
          sessionId: '',
          permissions: [],
        };
      }

      // Extract active org from session if available
      const activeOrgId = (session.session as any)?.activeOrganizationId || '';

      return {
        valid: true,
        userId: session.user.id,
        orgId: activeOrgId,
        email: session.user.email || '',
        name: session.user.name || '',
        role: (session.user as any).role || 'user',
        sessionId: session.session?.id || '',
        permissions: [], // Populated by org-core CheckOrgAccess
        expiresAt: session.session?.expiresAt
          ? {
              seconds: Math.floor(
                new Date(session.session.expiresAt).getTime() / 1000,
              ),
              nanos: 0,
            }
          : undefined,
      };
    } catch (error) {
      this.logger.error('ValidateToken error:', error);
      return {
        valid: false,
        userId: '',
        orgId: '',
        email: '',
        name: '',
        role: '',
        sessionId: '',
        permissions: [],
      };
    }
  }

  @GrpcMethod('AuthService', 'SignUp')
  async signUp(data: SignUpRequest): Promise<SignUpResponse> {
    try {
      // Call Better Auth sign up
      const response = await auth.api.signUpEmail({
        body: {
          email: data.email,
          password: data.password,
          name: data.name,
          callbackURL: data.callbackUrl,
        },
      });

      if (!response || !response.user) {
        throw new Error('Sign up failed');
      }

      return {
        user: this.mapToAuthUser(response.user),
        token: response.token || '',
        session: undefined, // Session management handled separately
      };
    } catch (error) {
      console.error('SignUp gRPC error:', error);
      throw error;
    }
  }

  @GrpcMethod('AuthService', 'SignIn')
  async signIn(data: SignInRequest): Promise<SignInResponse> {
    try {
      const response = await auth.api.signInEmail({
        body: {
          email: data.email,
          password: data.password,
        },
      });

      if (!response || !response.user) {
        throw new Error('Sign in failed');
      }

      return {
        user: this.mapToAuthUser(response.user),
        token: response.token || '',
        session: undefined, // Session management handled separately
        requiresTwoFactor: false,
      };
    } catch (error) {
      console.error('SignIn gRPC error:', error);
      throw error;
    }
  }

  @GrpcMethod('AuthService', 'SignOut')
  async signOut(data: SignOutRequest): Promise<SignOutResponse> {
    try {
      await auth.api.signOut({
        headers: {
          authorization: `Bearer ${data.token}`,
        },
      });

      return { success: true };
    } catch (error) {
      console.error('SignOut gRPC error:', error);
      return { success: false };
    }
  }

  @GrpcMethod('AuthService', 'GetCurrentUser')
  async getCurrentUser(
    data: GetCurrentUserRequest,
  ): Promise<GetCurrentUserResponse> {
    try {
      // For gRPC calls, validate JWT directly instead of using Better Auth's session API
      const payload = this.validateJWT(data.token);

      if (!payload || !payload.userId) {
        throw new Error('Invalid token or user not found');
      }

      // Return user from JWT claims
      return {
        user: {
          id: payload.userId,
          email: payload.email || '',
          name: payload.name || '',
          image: '',
          emailVerified: false,
          twoFactorEnabled: false,
          phoneNumber: '',
          phoneNumberVerified: false,
          role: payload.isAdmin ? 'admin' : 'user',
          createdAt: undefined,
          updatedAt: undefined,
        },
      };
    } catch (error) {
      console.error('GetCurrentUser gRPC error:', error);
      throw error;
    }
  }

  // Helper method to validate JWT tokens
  private validateJWT(token: string): any {
    try {
      const secret =
        process.env.JWT_SECRET ||
        'sCmpCA9xm6bR40cRQKmw18MeQQtu0cS3hwBoIdszpeRJVJXsqf6ff8NEX5bndxpt';
      const [headerB64, payloadB64, signatureB64] = token.split('.');

      if (!headerB64 || !payloadB64 || !signatureB64) {
        throw new Error('Invalid token format');
      }

      // Verify signature using base64url encoding
      const message = `${headerB64}.${payloadB64}`;

      const hmac = crypto.createHmac('sha256', secret);
      hmac.update(message);
      const signature = hmac.digest();

      // Convert signature to base64url (manual conversion since Node.js < 16 might not support it)
      const expectedSignature = signature
        .toString('base64')
        .replace(/\+/g, '-')
        .replace(/\//g, '_')
        .replace(/=/g, '');

      if (expectedSignature !== signatureB64) {
        throw new Error('Invalid signature');
      }

      // Decode payload
      const payload = JSON.parse(
        Buffer.from(payloadB64, 'base64url').toString('utf-8'),
      );

      // Check expiration
      if (payload.exp && payload.exp < Math.floor(Date.now() / 1000)) {
        throw new Error('Token expired');
      }

      return payload;
    } catch (error) {
      console.error('JWT validation error:', error);
      throw error;
    }
  }

  @GrpcMethod('AuthService', 'HealthCheck')
  async healthCheck(data: HealthCheckRequest): Promise<HealthCheckResponse> {
    return {
      status: 1, // SERVING
      message: 'Auth service is healthy',
      details: {
        service: 'auth-service',
        version: '1.0.0',
        timestamp: new Date().toISOString(),
      },
    };
  }

  // Helper methods to map Better Auth types to proto types
  private mapToAuthUser(user: any): AuthUser {
    return {
      id: user.id,
      email: user.email,
      name: user.name || '',
      image: user.image || '',
      emailVerified: user.emailVerified || false,
      twoFactorEnabled: user.twoFactorEnabled || false,
      phoneNumber: user.phoneNumber || '',
      phoneNumberVerified: user.phoneNumberVerified || false,
      role: user.role || '',
      createdAt: user.createdAt
        ? {
            seconds: Math.floor(new Date(user.createdAt).getTime() / 1000),
            nanos: 0,
          }
        : undefined,
      updatedAt: user.updatedAt
        ? {
            seconds: Math.floor(new Date(user.updatedAt).getTime() / 1000),
            nanos: 0,
          }
        : undefined,
    };
  }
}
