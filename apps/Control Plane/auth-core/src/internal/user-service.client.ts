import { Injectable, Logger } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';

// Removed unused request interfaces; using direct payloads per REST endpoints

@Injectable()
export class UserServiceClient {
  private readonly logger = new Logger(UserServiceClient.name);
  private readonly userServiceUrl: string;

  constructor(private readonly configService: ConfigService) {
    this.userServiceUrl =
      this.configService.get<string>('USER_SERVICE_URL') ||
      'http://localhost:3012';
    this.logger.log(`User service URL: ${this.userServiceUrl}`);
  }

  /**
   * Make HTTP request to user service
   */
  private async makeRequest<T>(
    endpoint: string,
    method: 'GET' | 'POST' | 'PUT' | 'DELETE' = 'GET',
    body?: any,
  ): Promise<T> {
    const url = `${this.userServiceUrl}${endpoint}`;
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      'User-Agent': 'auth-service/1.0.0',
    };

    const config: RequestInit = {
      method,
      headers,
    };

    if (body && method !== 'GET') {
      config.body = JSON.stringify(body);
    }

    this.logger.debug(`Making ${method} request to: ${url}`);

    const response = await fetch(url, config);

    if (!response.ok) {
      const errorText = await response.text();
      throw new Error(
        `HTTP ${response.status}: ${response.statusText}. ${errorText}`,
      );
    }

    return response.json() as T;
  }

  /**
   * Sync user data from auth service to user service
   * Called when a user registers or their profile is updated
   */
  async syncUser(data: {
    authUserId: string;
    email: string;
    name?: string;
    image?: string;
    emailVerified?: boolean;
  }): Promise<boolean> {
    try {
      this.logger.log(
        `Syncing user ${data.authUserId} (${data.email}) to user service`,
      );

      const result = await this.makeRequest<{ id: string }>(
        '/api/v1/users',
        'POST',
        {
          id: data.authUserId,
          email: data.email,
          name: data.name,
          image: data.image,
          emailVerified: data.emailVerified || false,
          isActive: true,
          isBlocked: false,
          isSuspended: false,
          theme: 'system',
          timezone: 'UTC',
          locale: 'en',
          profileVisibility: 'public',
          emailVisibility: 'private',
        },
      );

      this.logger.log(`User sync successful: ${result.id}`);
      return true;
    } catch (error) {
      this.logger.error('Failed to sync user to user service:', error);
      return false;
    }
  }

  /**
   * Check if user exists in user service by email
   */
  async validateUserExists(email: string): Promise<boolean> {
    try {
      this.logger.log(`Checking if user exists: ${email}`);

      await this.makeRequest(
        `/api/v1/users/by-email/${encodeURIComponent(email)}`,
      );

      this.logger.log(`User exists check result: true`);
      return true;
    } catch {
      this.logger.log(`User exists check result: false`);
      return false;
    }
  }

  /**
   * Update user session/login activity
   * Called when user logs in
   */
  async updateUserSession(data: {
    userId: string;
    sessionToken: string;
    ipAddress?: string;
    userAgent?: string;
    location?: string;
    device?: string;
    browser?: string;
    expiresAt: Date;
  }): Promise<boolean> {
    try {
      this.logger.log(`Updating user session for: ${data.userId}`);

      await this.makeRequest('/api/v1/users/sessions', 'POST', {
        userId: data.userId,
        sessionToken: data.sessionToken,
        ipAddress: data.ipAddress,
        userAgent: data.userAgent,
        location: data.location,
        device: data.device,
        browser: data.browser,
        expiresAt: data.expiresAt,
      });

      this.logger.log(`User session updated successfully`);
      return true;
    } catch (error) {
      this.logger.error('Failed to update user session:', error);
      return false;
    }
  }

  /**
   * Get user profile from user service
   */
  async getUserProfile(userId: string): Promise<any> {
    try {
      this.logger.log(`Getting user profile: ${userId}`);

      const user = await this.makeRequest(`/api/v1/users/${userId}/profile`);

      this.logger.log(`User profile retrieved successfully`);
      return user;
    } catch (error) {
      this.logger.error('Failed to get user profile:', error);
      return null;
    }
  }

  /**
   * Update user profile in user service
   */
  async updateUserProfile(userId: string, data: any): Promise<boolean> {
    try {
      this.logger.log(`Updating user profile: ${userId}`);

      await this.makeRequest(`/api/v1/users/${userId}/profile`, 'PUT', {
        id: userId,
        ...data,
      });

      this.logger.log(`User profile updated successfully`);
      return true;
    } catch (error) {
      this.logger.error('Failed to update user profile:', error);
      return false;
    }
  }

  /**
   * Delete user from user service
   */
  async deleteUser(userId: string): Promise<boolean> {
    try {
      this.logger.log(`Deleting user: ${userId}`);

      const result = await this.makeRequest<{ success: boolean }>(
        `/api/v1/users/${userId}`,
        'DELETE',
      );

      this.logger.log(`User deleted successfully: ${result.success}`);
      return result.success;
    } catch (error) {
      this.logger.error('Failed to delete user:', error);
      return false;
    }
  }

  /**
   * Health check endpoint
   */
  async healthCheck(): Promise<{
    status: 'healthy' | 'disabled' | 'unhealthy';
  }> {
    this.logger.log('Checking user service health');

    try {
      this.logger.debug(
        `Making GET request to: ${this.userServiceUrl}/api/v1/users/health/check`,
      );

      const response = await this.makeRequest<{ status: string }>(
        '/api/v1/users/health/check',
      );

      return {
        status: response.status === 'healthy' ? 'healthy' : 'unhealthy',
      };
    } catch (error) {
      this.logger.error('User service health check failed:');
      this.logger.error(error);
      return { status: 'disabled' };
    }
  }
}
