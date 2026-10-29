import { Injectable, Logger, OnModuleInit } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import * as grpc from '@grpc/grpc-js';
import * as protoLoader from '@grpc/proto-loader';
import { join } from 'node:path';
import {
  loadUserCoreGrpcClientCredential,
  type UserCoreGrpcClientCredential,
} from './user-core-grpc-credential';
import { loadUserCoreGrpcChannelCredentials } from './user-core-grpc-transport';

type UserMethod =
  | 'CreateUser'
  | 'UpdateUser'
  | 'GetUserByEmail'
  | 'CreateSession'
  | 'GetUser'
  | 'DeleteUser'
  | 'HealthCheck';

type UnaryMethod = (
  request: unknown,
  metadata: grpc.Metadata,
  callback: (error: grpc.ServiceError | null, response: unknown) => void,
) => grpc.ClientUnaryCall;

type UserServiceClient = grpc.Client & Record<UserMethod, UnaryMethod>;
type UserServiceClientConstructor = new (
  address: string,
  credentials: grpc.ChannelCredentials,
) => UserServiceClient;

type UserProfileUpdate = Readonly<{
  email?: string;
  name?: string;
  avatar?: string;
}>;

type Invocation = Readonly<{
  error: grpc.ServiceError | null;
  response: unknown;
}>;

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function userServiceConstructor(
  descriptor: grpc.GrpcObject,
): UserServiceClientConstructor {
  const user = record(descriptor.user);
  const v1 = record(user?.v1);
  const constructor = v1?.UserService;
  if (typeof constructor !== 'function') {
    throw new Error('user.v1.UserService constructor is unavailable');
  }
  return constructor as unknown as UserServiceClientConstructor;
}

/**
 * Clean replacement provider for the legacy gRPC adapter. It keeps the public
 * method surface while binding every call to one file-backed User Core tuple.
 */
@Injectable()
export class ScopedUserServiceGrpcClient implements OnModuleInit {
  private readonly logger = new Logger(ScopedUserServiceGrpcClient.name);
  private readonly grpcUrl: string;
  private readonly serviceCredential: UserCoreGrpcClientCredential;
  private client: UserServiceClient | null = null;

  constructor(configService: ConfigService) {
    this.grpcUrl =
      configService.get<string>('USER_SERVICE_GRPC_URL') ??
      'user-service:50012';
    this.serviceCredential = loadUserCoreGrpcClientCredential();
  }

  onModuleInit(): void {
    const protoPath = join(process.cwd(), 'proto/user/v1/user.proto');
    const definition = protoLoader.loadSync(protoPath, {
      keepCase: true,
      longs: String,
      enums: String,
      defaults: true,
      oneofs: true,
    });
    const constructor = userServiceConstructor(
      grpc.loadPackageDefinition(definition),
    );
    this.client = new constructor(
      this.grpcUrl,
      loadUserCoreGrpcChannelCredentials(),
    );
    this.logger.log(`Initialized User Core gRPC client at ${this.grpcUrl}`);
  }

  private metadata(): grpc.Metadata {
    const metadata = new grpc.Metadata();
    metadata.set(
      'x-service-credential-id',
      this.serviceCredential.credentialId,
    );
    metadata.set('x-service-principal', this.serviceCredential.principal);
    metadata.set('x-service-auth', this.serviceCredential.token);
    return metadata;
  }

  private invoke(method: UserMethod, request: unknown): Promise<Invocation> {
    const client = this.client;
    if (!client) {
      throw new Error('User Core gRPC client is unavailable');
    }
    return new Promise((resolve) => {
      client[method](request, this.metadata(), (error, response) => {
        resolve({ error, response });
      });
    });
  }

  async syncUser(data: {
    authUserId: string;
    email: string;
    name?: string;
    image?: string;
    emailVerified?: boolean;
  }): Promise<boolean> {
    const request = {
      id: data.authUserId,
      email: data.email,
      name: data.name ?? '',
      avatar: data.image ?? '',
    };
    const created = await this.invoke('CreateUser', request);
    if (!created.error) return created.response !== null;
    if (created.error.code !== grpc.status.ALREADY_EXISTS) {
      this.logger.error('Failed to sync user via gRPC', created.error);
      return false;
    }
    const updated = await this.invoke('UpdateUser', request);
    if (updated.error) {
      this.logger.error('Failed to update existing user', updated.error);
      return false;
    }
    return updated.response !== null;
  }

  async validateUserExists(email: string): Promise<boolean> {
    const result = await this.invoke('GetUserByEmail', { email });
    return result.error === null && result.response !== null;
  }

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
    const expiresInSeconds = Math.floor(
      (data.expiresAt.getTime() - Date.now()) / 1000,
    );
    const result = await this.invoke('CreateSession', {
      user_id: data.userId,
      device_info: data.device ?? data.userAgent ?? '',
      ip_address: data.ipAddress ?? '',
      user_agent: data.userAgent ?? '',
      expires_in_seconds: expiresInSeconds,
    });
    return result.error === null && result.response !== null;
  }

  async getUserProfile(userId: string): Promise<unknown> {
    const result = await this.invoke('GetUser', { id: userId });
    if (result.error) {
      this.logger.error('Failed to get user profile', result.error);
      return null;
    }
    return record(result.response)?.user ?? null;
  }

  async updateUserProfile(
    userId: string,
    data: UserProfileUpdate,
  ): Promise<boolean> {
    const result = await this.invoke('UpdateUser', { id: userId, ...data });
    return result.error === null && result.response !== null;
  }

  async deleteUser(userId: string): Promise<boolean> {
    const result = await this.invoke('DeleteUser', { id: userId });
    if (result.error) return false;
    return record(result.response)?.success === true;
  }

  async healthCheck(): Promise<{
    status: 'healthy' | 'disabled' | 'unhealthy';
  }> {
    if (!this.client) return { status: 'unhealthy' };
    const result = await this.invoke('HealthCheck', { service: 'user' });
    if (result.error) return { status: 'unhealthy' };
    return {
      status: record(result.response)?.status === 1 ? 'healthy' : 'unhealthy',
    };
  }
}
