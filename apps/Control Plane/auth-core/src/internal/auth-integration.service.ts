/**
 * Auth Integration Service
 *
 * Bridges Better Auth events with user service synchronization
 * Handles the separation of concerns between auth and user management
 */

import { Injectable, Logger } from '@nestjs/common';
import { UserServiceClient } from './user-service.client';
import { UserServiceGrpcClient } from './user-service-grpc.client';
import { AuthEventPublisher } from './auth-event.publisher';

@Injectable()
export class AuthIntegrationService {
  private readonly logger = new Logger(AuthIntegrationService.name);

  constructor(
    private userServiceClient: UserServiceClient,
    private userServiceGrpcClient: UserServiceGrpcClient,
    private authEventPublisher: AuthEventPublisher,
  ) {}

  /**
   * Handle user registration event from Better Auth
   */
  async handleUserRegistration(userData: {
    id: string;
    email: string;
    name?: string;
    emailVerified: boolean;
    provider?: string;
    metadata?: Record<string, any>;
    microsoftTenantId?: string;
    emailFromProvider?: string;
    scopesGranted?: string[];
    tokenRef?: string;
    profileHints?: {
      displayName?: string;
      avatar?: string;
      locale?: string;
      timezone?: string;
    };
  }): Promise<void> {
    this.logger.log(`Handling user registration: ${userData.email}`);

    try {
      // Sync user data to user service via gRPC
      await this.userServiceGrpcClient.syncUser({
        authUserId: userData.id,
        email: userData.email,
        name: userData.name,
        emailVerified: userData.emailVerified,
      });

      // Publish async event via NATS
      await this.authEventPublisher.publishUserRegistered({
        userId: userData.id,
        email: userData.email,
        name: userData.name,
        provider: userData.provider || 'email',
        emailVerified: userData.emailVerified,
        metadata: userData.metadata,
        microsoftTenantId: userData.microsoftTenantId,
        emailFromProvider: userData.emailFromProvider,
        scopesGranted: userData.scopesGranted,
        tokenRef: userData.tokenRef,
        profileHints: userData.profileHints,
      });

      this.logger.log(
        `User registration processed successfully: ${userData.email}`,
      );
    } catch (error) {
      this.logger.error(
        `Failed to process user registration: ${userData.email}`,
        error,
      );
      // Don't throw error to prevent auth flow interruption
    }
  }

  /**
   * Handle user login event from Better Auth
   */
  async handleUserLogin(loginData: {
    userId: string;
    email: string;
    sessionId: string;
    deviceInfo?: string;
    ipAddress?: string;
    userAgent?: string;
    provider?: string;
    name?: string;
  }): Promise<void> {
    this.logger.log(`Handling user login: ${loginData.email}`);

    try {
      // Ensure user exists in user-service (auto-provision on first login)
      const exists = await this.userServiceGrpcClient.validateUserExists(
        loginData.email,
      );
      if (!exists) {
        await this.userServiceGrpcClient.syncUser({
          authUserId: loginData.userId,
          email: loginData.email,
          name: loginData.name,
          emailVerified: true,
        });
      }

      // Update session in user service
      await this.userServiceGrpcClient.updateUserSession({
        sessionToken: loginData.sessionId,
        userId: loginData.userId,
        ipAddress: loginData.ipAddress,
        userAgent: loginData.userAgent,
        expiresAt: new Date(Date.now() + 24 * 60 * 60 * 1000), // 24 hours
      });

      // Publish login event
      await this.authEventPublisher.publishUserLogin({
        userId: loginData.userId,
        email: loginData.email,
        sessionId: loginData.sessionId,
        deviceInfo: loginData.deviceInfo,
        ipAddress: loginData.ipAddress,
        userAgent: loginData.userAgent,
        provider: loginData.provider,
      });

      // Publish session created event
      await this.authEventPublisher.publishSessionCreated({
        sessionId: loginData.sessionId,
        userId: loginData.userId,
        email: loginData.email,
        deviceInfo: loginData.deviceInfo,
        ipAddress: loginData.ipAddress,
        userAgent: loginData.userAgent,
        expiresAt: new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString(),
      });

      this.logger.log(`User login processed successfully: ${loginData.email}`);
    } catch (error) {
      this.logger.error(
        `Failed to process user login: ${loginData.email}`,
        error,
      );
    }
  }

  /**
   * Handle account linking — an existing user connected a new OAuth provider
   */
  async handleAccountLinked(linkData: {
    id: string;
    email: string;
    name?: string;
    provider: string;
    providerAccountId?: string;
    tenantId?: string;
    microsoftTenantId?: string;
    emailFromProvider?: string;
    scopesGranted?: string[];
    tokenRef?: string;
    profileHints?: {
      displayName?: string;
      avatar?: string;
      locale?: string;
      timezone?: string;
    };
  }): Promise<void> {
    this.logger.log(
      `Handling account link: ${linkData.email} → ${linkData.provider}`,
    );

    try {
      // Ensure user-service reflects any updated profile data from the new provider
      await this.userServiceGrpcClient.syncUser({
        authUserId: linkData.id,
        email: linkData.email,
        name: linkData.name,
        emailVerified: true, // OAuth providers deliver verified emails
      });

      // Publish provider linked event so user-core can persist the provider account
      await this.authEventPublisher.publishUserProviderLinked({
        userId: linkData.id,
        email: linkData.email,
        provider: linkData.provider,
        providerAccountId: linkData.providerAccountId,
        tenantId: linkData.tenantId,
        microsoftTenantId: linkData.microsoftTenantId,
        emailFromProvider: linkData.emailFromProvider,
        scopesGranted: linkData.scopesGranted,
        tokenRef: linkData.tokenRef,
        profileHints: linkData.profileHints,
      });

      this.logger.log(
        `Account link processed: ${linkData.email} → ${linkData.provider}`,
      );
    } catch (error) {
      this.logger.error(
        `Failed to process account link: ${linkData.email}`,
        error,
      );
      // Don't rethrow — account linking must not interrupt the auth flow
    }
  }

  /**
   * Handle user logout event from Better Auth
   */
  async handleUserLogout(logoutData: {
    userId: string;
    email: string;
    sessionId: string;
    reason?: 'manual' | 'timeout' | 'force';
  }): Promise<void> {
    this.logger.log(`Handling user logout: ${logoutData.email}`);

    try {
      // Publish logout event
      await this.authEventPublisher.publishUserLogout({
        userId: logoutData.userId,
        email: logoutData.email,
        sessionId: logoutData.sessionId,
        reason: logoutData.reason || 'manual',
      });

      // Publish session ended event
      await this.authEventPublisher.publishSessionEnded({
        sessionId: logoutData.sessionId,
        userId: logoutData.userId,
        email: logoutData.email,
        reason: 'logout',
      });

      this.logger.log(
        `User logout processed successfully: ${logoutData.email}`,
      );
    } catch (error) {
      this.logger.error(
        `Failed to process user logout: ${logoutData.email}`,
        error,
      );
    }
  }

  /**
   * Handle user profile update event
   */
  async handleUserProfileUpdate(updateData: {
    userId: string;
    email: string;
    changes: Record<string, any>;
  }): Promise<void> {
    this.logger.log(`Handling profile update: ${updateData.email}`);

    try {
      // Update profile in user service if relevant changes
      const profileData = this.extractProfileData(updateData.changes);
      if (Object.keys(profileData).length > 0) {
        await this.userServiceClient.updateUserProfile(
          updateData.userId,
          profileData,
        );
      }

      // Publish profile updated event
      await this.authEventPublisher.publishUserProfileUpdated({
        userId: updateData.userId,
        email: updateData.email,
        changes: updateData.changes,
      });

      this.logger.log(
        `Profile update processed successfully: ${updateData.email}`,
      );
    } catch (error) {
      this.logger.error(
        `Failed to process profile update: ${updateData.email}`,
        error,
      );
    }
  }

  /**
   * Handle user deletion event
   */
  async handleUserDeletion(deletionData: {
    userId: string;
    email: string;
  }): Promise<void> {
    this.logger.log(`Handling user deletion: ${deletionData.email}`);

    try {
      // Delete user from user service
      await this.userServiceClient.deleteUser(deletionData.userId);

      this.logger.log(
        `User deletion processed successfully: ${deletionData.email}`,
      );
    } catch (error) {
      this.logger.error(
        `Failed to process user deletion: ${deletionData.email}`,
        error,
      );
    }
  }

  /**
   * Health check for integration services
   */
  async healthCheck(): Promise<{
    userService: string;
    natsEvents: string;
  }> {
    const userServiceHealth = await this.userServiceGrpcClient.healthCheck();
    return {
      userService:
        userServiceHealth.status === 'healthy' ? 'connected' : 'disconnected',
      natsEvents: this.authEventPublisher.isHealthy()
        ? 'connected'
        : 'disconnected',
    };
  }

  /**
   * Extract profile-relevant data from changes
   */
  private extractProfileData(
    changes: Record<string, any>,
  ): Record<string, any> {
    const profileFields = [
      'firstName',
      'lastName',
      'displayName',
      'avatar',
      'timezone',
      'locale',
    ];
    const profileData: Record<string, any> = {};

    for (const field of profileFields) {
      if ((changes as Record<string, unknown>)[field] !== undefined) {
        profileData[field] = (changes as Record<string, unknown>)[field];
      }
    }

    return profileData;
  }
}
