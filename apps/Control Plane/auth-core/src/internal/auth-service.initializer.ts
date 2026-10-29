/**
 * Auth Service Initializer
 *
 * Initializes the integration between Better Auth and User Service
 * Sets up service connections and event handlers
 */

import { Injectable, Logger, OnModuleInit } from '@nestjs/common';
import { AuthIntegrationService } from './auth-integration.service';
import { AuthEventPublisher } from './auth-event.publisher';
import { DirectNatsService } from '../nats/direct-nats.service';
import { setAuthIntegrationService } from '../auth/user-service-integration.plugin';
import { setOrganizationEventPublisher } from '../auth/organization-hooks';
import { setOrganizationEventPublisher as setPluginEventPublisher } from '../auth/organization-events.plugin';
import { validateOrganizationReconciliationCredentials } from '../auth/control-service-credentials';
import { setAuditNatsPublisher } from '../auth/audit-plugin';

@Injectable()
export class AuthServiceInitializer implements OnModuleInit {
  private readonly logger = new Logger(AuthServiceInitializer.name);

  constructor(
    private authIntegrationService: AuthIntegrationService,
    private authEventPublisher: AuthEventPublisher,
    private directNats: DirectNatsService,
  ) {}

  onModuleInit(): void {
    validateOrganizationReconciliationCredentials();
    try {
      this.logger.log('Initializing auth service integration...');

      // Set the integration service for the Better Auth plugin
      setAuthIntegrationService(this.authIntegrationService);

      // Set the event publisher for organization hooks
      setOrganizationEventPublisher(this.authEventPublisher);
      setPluginEventPublisher(this.authEventPublisher);
      this.logger.log('Organization event publisher initialized');

      // Wire the LOCAL control-plane bus (DirectNatsService → controlplane-nats)
      // into the audit plugin for Auth Core's verevon.audit.v2.control.* emission.
      // primary subscription listens there; the shared verevon-nats bus is reserved
      // for cross-plane domain/ACL events.
      setAuditNatsPublisher(this.directNats);
      this.logger.log(
        'Audit NATS publisher initialized (local control-plane bus)',
      );

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
