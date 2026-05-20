import { Module, NestModule, MiddlewareConsumer } from '@nestjs/common';
import { ScheduleModule } from '@nestjs/schedule';
import { HttpModule } from '@nestjs/axios';
import { AuthModule } from '@thallesp/nestjs-better-auth';
import { auth } from './auth/auth';
import { UsersController } from './users/users.controller';
import { NatsAuthController } from './auth/nats-auth.controller';
import { ConvexAuthController } from './auth/convex-auth.controller';
import { ModelPlaneTokenController } from './auth/model-plane-token.controller';
import { PlaneTokenController } from './auth/plane-token.controller';
import { AuthGrpcController } from './grpc/auth-grpc.controller';
import { EmailModule } from './email/email.module';
import { ORPCModule } from './orpc/orpc.module';
import { SessionCleanupService } from './services/session-cleanup.service';
import { MicrosoftGraphService } from './services/microsoft-graph.service';
import { ConvexTokenService } from './auth/convex-token.service';
import { DocsModule } from './docs/docs.module';
import { InternalServicesModule } from './internal/internal-services.module';
import { NatsModule } from './nats/nats.module';
import { OrganizationEventMiddleware } from './middleware/organization-event.middleware';

@Module({
  imports: [
    ScheduleModule.forRoot(),
    HttpModule.register({
      timeout: 10000,
      maxRedirects: 5,
    }),
    EmailModule,
    DocsModule,
    NatsModule, // Direct NATS for request-reply pattern
    InternalServicesModule, // Internal services for user service communication
    ORPCModule, // Register enhanced endpoints first
    AuthModule.forRoot(auth), // Better Auth catch-all comes after all custom routes
  ],
  controllers: [
    UsersController,
    NatsAuthController,
    ConvexAuthController,
    ModelPlaneTokenController,
    PlaneTokenController,
    AuthGrpcController,
  ],
  providers: [
    SessionCleanupService,
    MicrosoftGraphService,
    ConvexTokenService,
    OrganizationEventMiddleware,
  ],
})
export class AppModule implements NestModule {
  configure(consumer: MiddlewareConsumer) {
    consumer
      .apply(OrganizationEventMiddleware)
      .forRoutes(
        '/api/auth/organization/create',
        '/api/auth/organization/invite-member',
        '/api/auth/organization/remove-member',
      );
  }
}
