/**
 * Internal Services Module
 *
 * Module for internal service communication including:
 * - User Service oRPC client
 * - NATS event publishing
 * - Service-to-service security
 */

import { Module } from '@nestjs/common';
import { ConfigModule } from '@nestjs/config';
import { UserServiceClient } from './user-service.client';
import { UserServiceGrpcClient } from './user-service-grpc.client';
import { AuthEventPublisher } from './auth-event.publisher';
import { AuthIntegrationService } from './auth-integration.service';
import { AuthServiceInitializer } from './auth-service.initializer';
import { InternalOAuthController } from './internal-oauth.controller';
import { InternalOAuthService } from './internal-oauth.service';
import { InternalAgentSignupController } from './internal-agent-signup.controller';
import { ScopedUserServiceGrpcClient } from './scoped-user-service-grpc.client';

@Module({
  imports: [ConfigModule],
  controllers: [InternalOAuthController, InternalAgentSignupController],
  providers: [
    UserServiceClient,
    {
      provide: UserServiceGrpcClient,
      useClass: ScopedUserServiceGrpcClient,
    },
    AuthEventPublisher,
    AuthIntegrationService,
    AuthServiceInitializer,
    InternalOAuthService,
  ],
  exports: [
    UserServiceClient,
    UserServiceGrpcClient,
    AuthEventPublisher,
    AuthIntegrationService,
    AuthServiceInitializer,
    InternalOAuthService,
  ],
})
export class InternalServicesModule {}
