import { Module } from '@nestjs/common';
import { OpenApiController } from './openapi.controller';
import { ConsolidatedAuthController } from './auth.controller';
import { OrganizationsController } from './organizations.controller';
import { InternalServicesModule } from '../internal/internal-services.module';

@Module({
  imports: [InternalServicesModule], // Import to make AuthIntegrationService available
  controllers: [
    OpenApiController, // Keep for oRPC OpenAPI documentation
    ConsolidatedAuthController, // Enhanced auth controller with individual endpoints
    OrganizationsController, // Organizations controller for frontend compatibility
  ],
  providers: [],
  exports: [],
})
export class ORPCModule {}
