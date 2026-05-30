/**
 * Auth Service Initializer
 *
 * Initializes the integration between Better Auth and User Service
 * Sets up service connections and event handlers
 */

import { Injectable, Logger, OnModuleInit } from '@nestjs/common';
import { AuthIntegrationService } from './auth-integration.service';
import { AuthEventPublisher } from './auth-event.publisher';
import { setAuthIntegrationService } from '../auth/user-service-integration.plugin';
import { setOrganizationEventPublisher } from '../auth/organization-hooks';
import { setOrganizationEventPublisher as setPluginEventPublisher } from '../auth/organization-events.plugin';

@Injectable()
export class AuthServiceInitializer implements OnModuleInit {
  private readonly logger = new Logger(AuthServiceInitializer.name);

  constructor(
    private authIntegrationService: AuthIntegrationService,
    private authEventPublisher: AuthEventPublisher,
  ) {}

  async onModuleInit() {
    try {
      this.logger.log('Initializing auth service integration...');

      // Set the integration service for the Better Auth plugin
      setAuthIntegrationService(this.authIntegrationService);

      // Set the event publisher for organization hooks
      setOrganizationEventPublisher(this.authEventPublisher);
      setPluginEventPublisher(this.authEventPublisher);
      this.logger.log('Organization event publisher initialized');

      // Perform health checks
      const health = await this.authIntegrationService.healthCheck();

      this.logger.log('Auth service integration initialized successfully', {
        userService: health.userService ? 'connected' : 'disconnected',
        natsEvents: health.natsEvents ? 'connected' : 'disconnected',
      });

      if (!health.userService) {
        this.logger.warn(
          'User service is not available - integration will retry automatically',
        );
      }

      if (!health.natsEvents) {
        this.logger.warn(
          'NATS events are not available - events will be skipped',
        );
      }
    } catch (error) {
      this.logger.error('Failed to initialize auth service integration', error);
      // Don't throw to prevent app startup failure
    }
  }
}
