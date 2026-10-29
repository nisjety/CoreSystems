import { Injectable, Logger, OnModuleInit } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import * as grpc from '@grpc/grpc-js';
import * as protoLoader from '@grpc/proto-loader';
import { join } from 'path';

// gRPC client for UserService
type UserServiceClient = any;

@Injectable()
export class UserServiceGrpcClient implements OnModuleInit {
  private readonly logger = new Logger(UserServiceGrpcClient.name);
  private client: UserServiceClient | null = null;
  private readonly grpcUrl: string;
  private readonly internalApiKey: string;
  private isConnected = false;

  constructor(private readonly configService: ConfigService) {
    this.grpcUrl =
      this.configService.get<string>('USER_SERVICE_GRPC_URL') ||
      'user-service:50012';
    this.internalApiKey =
      this.configService.get<string>('INTERNAL_API_KEY') ||
      this.configService.get<string>('INTERNAL_SERVICE_SECRET') ||
      '';
    this.logger.log(`User service gRPC URL: ${this.grpcUrl}`);
  }

  private callMetadata(): grpc.Metadata {
    const metadata = new grpc.Metadata();
    if (this.internalApiKey) {
      metadata.set('x-internal-api-key', this.internalApiKey);
    }
    return metadata;
  }

  async onModuleInit() {
    await this.connect();
  }

  private async connect(): Promise<void> {
    try {
      const protoPath =
        process.env.NODE_ENV === 'production'
          ? join(__dirname, '../proto/user/v1/user.proto')
          : join(process.cwd(), 'proto/user/v1/user.proto');

      const packageDefinition = protoLoader.loadSync(protoPath, {
        keepCase: true,
        longs: String,
        enums: String,
        defaults: true,
        oneofs: true,
      });

      const protoDescriptor: any =
        grpc.loadPackageDefinition(packageDefinition);
      const UserService = protoDescriptor.user.v1.UserService;

      this.client = new UserService(
        this.grpcUrl,
        grpc.credentials.createInsecure(),
      );

      this.isConnected = true;
      this.logger.log(`Connected to user service gRPC at ${this.grpcUrl}`);
    } catch (error) {
      this.logger.error('Failed to connect to user service gRPC:', error);
      this.isConnected = false;
    }
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
    if (!this.isConnected || !this.client) {
      this.logger.warn('User service gRPC not connected, skipping user sync');
      return false;
    }

    try {
      this.logger.log(
        `Syncing user ${data.authUserId} (${data.email}) to user service via gRPC`,
      );

      return new Promise((resolve, reject) => {
        this.client.CreateUser(
          {
            id: data.authUserId,
            email: data.email,
            name: data.name || '',
            avatar: data.image || '',
          },
          this.callMetadata(),
          (error: any, response: any) => {
            if (error) {
              // Check if user already exists (common case)
              if (error.code === grpc.status.ALREADY_EXISTS) {
                this.logger.log(
                  `User ${data.email} already exists, updating instead`,
                );
                // Try to update the user instead
                this.client.UpdateUser(
                  {
                    id: data.authUserId,
                    email: data.email,
                    name: data.name || '',
                    avatar: data.image || '',
                  },
                  this.callMetadata(),
                  (updateError: any, updateResponse: any) => {
                    if (updateError) {
                      this.logger.error(
                        'Failed to update existing user:',
                        updateError,
                      );
                      resolve(false);
                    } else {
                      this.logger.log(
                        `User ${data.authUserId} updated successfully`,
                      );
                      resolve(true);
                    }
                  },
                );
              } else {
                this.logger.error('Failed to sync user via gRPC:', error);
                resolve(false);
              }
            } else {
              this.logger.log(`User sync successful: ${response.user?.id}`);
              resolve(true);
            }
          },
        );
      });
    } catch (error) {
      this.logger.error('Failed to sync user to user service:', error);
      return false;
    }
  }

  /**
   * Check if user exists in user service by email
   */
  async validateUserExists(email: string): Promise<boolean> {
    if (!this.isConnected || !this.client) {
      return false;
    }

    try {
      this.logger.log(`Checking if user exists: ${email}`);

      return new Promise((resolve) => {
        this.client.GetUserByEmail(
          { email },
          this.callMetadata(),
          (error: any, response: any) => {
            if (error) {
              this.logger.log(`User exists check result: false`);
              resolve(false);
            } else {
              this.logger.log(`User exists check result: true`);
              resolve(true);
            }
          },
        );
      });
    } catch (error) {
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
    if (!this.isConnected || !this.client) {
      return false;
    }

    try {
      this.logger.log(`Creating user session for: ${data.userId}`);

      const expiresInSeconds = Math.floor(
        (data.expiresAt.getTime() - Date.now()) / 1000,
      );

      return new Promise((resolve) => {
        this.client.CreateSession(
          {
            user_id: data.userId,
            device_info: data.device || data.userAgent || '',
            ip_address: data.ipAddress || '',
            user_agent: data.userAgent || '',
            expires_in_seconds: expiresInSeconds,
          },
          this.callMetadata(),
          (error: any, response: any) => {
            if (error) {
              this.logger.error('Failed to create user session:', error);
              resolve(false);
            } else {
              this.logger.log(`User session created successfully`);
              resolve(true);
            }
          },
        );
      });
    } catch (error) {
      this.logger.error('Failed to update user session:', error);
      return false;
    }
  }

  /**
   * Get user profile from user service
   */
  async getUserProfile(userId: string): Promise<any> {
    if (!this.isConnected || !this.client) {
      return null;
    }

    try {
      this.logger.log(`Getting user profile: ${userId}`);

      return new Promise((resolve) => {
        this.client.GetUser(
          { id: userId },
          this.callMetadata(),
          (error: any, response: any) => {
            if (error) {
              this.logger.error('Failed to get user profile:', error);
              resolve(null);
            } else {
              this.logger.log(`User profile retrieved successfully`);
              resolve(response.user);
            }
          },
        );
      });
    } catch (error) {
      this.logger.error('Failed to get user profile:', error);
      return null;
    }
  }

  /**
   * Update user profile in user service
   */
  async updateUserProfile(userId: string, data: any): Promise<boolean> {
    if (!this.isConnected || !this.client) {
      return false;
    }

    try {
      this.logger.log(`Updating user profile: ${userId}`);

      return new Promise((resolve) => {
        this.client.UpdateUser(
          {
            id: userId,
            ...data,
          },
          this.callMetadata(),
          (error: any, response: any) => {
            if (error) {
              this.logger.error('Failed to update user profile:', error);
              resolve(false);
            } else {
              this.logger.log(`User profile updated successfully`);
              resolve(true);
            }
          },
        );
      });
    } catch (error) {
      this.logger.error('Failed to update user profile:', error);
      return false;
    }
  }

  /**
   * Delete user from user service
   */
  async deleteUser(userId: string): Promise<boolean> {
    if (!this.isConnected || !this.client) {
      return false;
    }

    try {
      this.logger.log(`Deleting user: ${userId}`);

      return new Promise((resolve) => {
        this.client.DeleteUser(
          { id: userId },
          this.callMetadata(),
          (error: any, response: any) => {
            if (error) {
              this.logger.error('Failed to delete user:', error);
              resolve(false);
            } else {
              this.logger.log(`User deleted successfully: ${response.success}`);
              resolve(response.success);
            }
          },
        );
      });
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
    if (!this.isConnected || !this.client) {
      this.logger.warn('User service gRPC not connected');
      return { status: 'disabled' };
    }

    this.logger.log('Checking user service health via gRPC');

    try {
      return new Promise((resolve) => {
        this.client.HealthCheck(
          { service: 'user' },
          this.callMetadata(),
          (error: any, response: any) => {
            if (error) {
              this.logger.error('User service health check failed:', error);
              resolve({ status: 'unhealthy' });
            } else {
              const isHealthy = response.status === 1; // SERVING = 1
              resolve({
                status: isHealthy ? 'healthy' : 'unhealthy',
              });
            }
          },
        );
      });
    } catch (error) {
      this.logger.error('User service health check failed:', error);
      return { status: 'disabled' };
    }
  }
}
