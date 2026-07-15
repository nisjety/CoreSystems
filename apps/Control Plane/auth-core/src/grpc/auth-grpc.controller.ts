import { Controller, Logger } from '@nestjs/common';
import { GrpcMethod } from '@nestjs/microservices';
import { Metadata } from '@grpc/grpc-js';
import { auth } from '../auth/auth';
import {
  authorizeAuthGrpcService,
  type AuthGrpcScope,
  type AuthGrpcServiceCredential,
  loadAuthGrpcServiceCredentials,
} from './auth-grpc-service-auth';
import type {
  SignUpRequest,
  SignUpResponse,
  SignInRequest,
  SignInResponse,
  SignOutRequest,
  SignOutResponse,
  GetCurrentUserRequest,
  GetCurrentUserResponse,
  HealthCheckResponse,
} from './auth/v1/auth';
import type { AuthUser } from './auth/v1/auth';

const emptyRecord: Readonly<Record<string, unknown>> = Object.freeze({});

function asRecord(value: unknown): Readonly<Record<string, unknown>> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Readonly<Record<string, unknown>>)
    : emptyRecord;
}

function optionalString(
  record: Readonly<Record<string, unknown>>,
  field: string,
): string | undefined {
  const value = record[field];
  return typeof value === 'string' ? value : undefined;
}

function optionalBoolean(
  record: Readonly<Record<string, unknown>>,
  field: string,
): boolean | undefined {
  const value = record[field];
  return typeof value === 'boolean' ? value : undefined;
}

function optionalTimestamp(value: unknown):
  | Readonly<{
      seconds: number;
      nanos: number;
    }>
  | undefined {
  if (
    typeof value !== 'string' &&
    typeof value !== 'number' &&
    !(value instanceof Date)
  ) {
    return undefined;
  }
  const milliseconds = new Date(value).getTime();
  return Number.isFinite(milliseconds)
    ? Object.freeze({ seconds: Math.floor(milliseconds / 1000), nanos: 0 })
    : undefined;
}

@Controller()
export class AuthGrpcController {
  private readonly logger = new Logger(AuthGrpcController.name);
  private readonly serviceCredentials: readonly AuthGrpcServiceCredential[];

  constructor() {
    this.serviceCredentials = loadAuthGrpcServiceCredentials();
  }

  private authorize(metadata: Metadata, requiredScope: AuthGrpcScope): void {
    authorizeAuthGrpcService(metadata, this.serviceCredentials, requiredScope);
  }

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
    this.authorize(metadata, 'auth:token:validate');

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
      const sessionRecord = asRecord(session.session);
      const userRecord = asRecord(session.user);
      const activeOrgId =
        optionalString(sessionRecord, 'activeOrganizationId') ?? '';

      return {
        valid: true,
        userId: session.user.id,
        orgId: activeOrgId,
        email: session.user.email || '',
        name: session.user.name || '',
        role: optionalString(userRecord, 'role') || 'user',
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
    } catch {
      this.logger.error('Token validation failed');
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
  async signUp(
    data: SignUpRequest,
    metadata: Metadata,
  ): Promise<SignUpResponse> {
    this.authorize(metadata, 'auth:signup');
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
  async signIn(
    data: SignInRequest,
    metadata: Metadata,
  ): Promise<SignInResponse> {
    this.authorize(metadata, 'auth:signin');
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
  async signOut(
    data: SignOutRequest,
    metadata: Metadata,
  ): Promise<SignOutResponse> {
    this.authorize(metadata, 'auth:signout');
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
    metadata: Metadata,
  ): Promise<GetCurrentUserResponse> {
    this.authorize(metadata, 'auth:user:read');
    try {
      const session = await auth.api.getSession({
        headers: new Headers({ authorization: `Bearer ${data.token}` }),
      });
      if (!session?.user) {
        throw new Error('Invalid token or user not found');
      }

      return {
        user: this.mapToAuthUser(session.user),
      };
    } catch (error) {
      console.error('GetCurrentUser gRPC error:', error);
      throw error;
    }
  }

  @GrpcMethod('AuthService', 'HealthCheck')
  healthCheck(): HealthCheckResponse {
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
  private mapToAuthUser(user: unknown): AuthUser {
    const source = asRecord(user);
    const id = optionalString(source, 'id');
    const email = optionalString(source, 'email');
    if (!id || !email) {
      throw new Error('Auth provider returned an invalid user');
    }

    return {
      id,
      email,
      name: optionalString(source, 'name') || '',
      image: optionalString(source, 'image') || '',
      emailVerified: optionalBoolean(source, 'emailVerified') || false,
      twoFactorEnabled: optionalBoolean(source, 'twoFactorEnabled') || false,
      phoneNumber: optionalString(source, 'phoneNumber') || '',
      phoneNumberVerified:
        optionalBoolean(source, 'phoneNumberVerified') || false,
      role: optionalString(source, 'role') || '',
      createdAt: optionalTimestamp(source.createdAt),
      updatedAt: optionalTimestamp(source.updatedAt),
    };
  }
}
