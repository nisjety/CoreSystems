import { Injectable } from '@nestjs/common';

/**
 * Injection-token contract for the User Core gRPC client.
 *
 * The concrete implementation is always supplied via a NestJS DI override
 * (`internal-services.module.ts` binds this token to
 * `ScopedUserServiceGrpcClient`). This class intentionally has no method
 * bodies of its own: an earlier version duplicated the gRPC wiring here with
 * an untyped, dynamically-loaded proto client (`type UserServiceClient =
 * any`), but that code path never actually ran in production because the DI
 * override always wins. Keeping only the typed contract avoids maintaining
 * two divergent, mostly-`any` implementations of the same client.
 */
@Injectable()
export abstract class UserServiceGrpcClient {
  abstract syncUser(data: {
    authUserId: string;
    email: string;
    name?: string;
    image?: string;
    emailVerified?: boolean;
  }): Promise<boolean>;

  abstract validateUserExists(email: string): Promise<boolean>;

  abstract updateUserSession(data: {
    userId: string;
    sessionToken: string;
    ipAddress?: string;
    userAgent?: string;
    location?: string;
    device?: string;
    browser?: string;
    expiresAt: Date;
  }): Promise<boolean>;

  abstract getUserProfile(userId: string): Promise<unknown>;

  abstract updateUserProfile(
    userId: string,
    data: Readonly<{ email?: string; name?: string; avatar?: string }>,
  ): Promise<boolean>;

  abstract deleteUser(userId: string): Promise<boolean>;

  abstract healthCheck(): Promise<{
    status: 'healthy' | 'disabled' | 'unhealthy';
  }>;
}
