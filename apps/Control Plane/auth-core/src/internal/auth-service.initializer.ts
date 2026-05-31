/**
 * Auth Service Initializer
 *
 * Initializes the integration between Better Auth and User Service
 * Sets up service connections and event handlers
 */

import { Injectable, Logger, OnModuleInit } from '@nestjs/common';
import { AuthIntegrationService } from './auth-integration.service';
import { AuthEventPublisher } from './auth-event.publisher';
import { SharedNatsService } from '../nats/shared-nats.service';
import { setAuthIntegrationService } from '../auth/user-service-integration.plugin';
import { setOrganizationEventPublisher } from '../auth/organization-hooks';
import { setOrganizationEventPublisher as setPluginEventPublisher } from '../auth/organization-events.plugin';
import { setAuditNatsPublisher } from '../auth/audit-plugin';

@Injectable()
export class AuthServiceInitializer implements OnModuleInit {
  private readonly logger = new Logger(AuthServiceInitializer.name);

  constructor(
    private authIntegrationService: AuthIntegrationService,
    private authEventPublisher: AuthEventPublisher,
    private sharedNats: SharedNatsService,
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

      // Wire SharedNatsService into the audit plugin for velion.audit.v1.* emission
      setAuditNatsPublisher(this.sharedNats);
      this.logger.log('Audit NATS publisher initialized');

      this.logger.log('Auth service integration initialized successfully', {
        // user-core authenticates against auth-core during its own startup, so
        // probing user-core here creates a startup-order loop. Runtime calls and
        // health endpoints still perform the actual user-core checks.
        userService: 'lazy',
        natsEvents: this.authEventPublisher.isHealthy()
          ? 'connected'
          : 'deferred',
      });
    } catch (error) {
      this.logger.error('Failed to initialize auth service integration', error);
      // Don't throw to prevent app startup failure
    }
  }
}
