/**
 * NATS Event Publisher Service
 *
 * Publishes authentication events to NATS for async communication
 * with user service and other microservices
 */

import {
  Injectable,
  Logger,
  OnModuleInit,
  OnModuleDestroy,
  Optional,
} from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import {
  connect,
  NatsConnection,
  StringCodec,
  JetStreamClient,
  RetentionPolicy,
  StorageType,
} from 'nats';
import { SharedNatsService } from '../nats/shared-nats.service';

function isNonFatalStreamSetupError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  const normalized = message.toLowerCase();
  return (
    normalized.includes('stream name already in use') ||
    normalized.includes('subjects overlap with an existing stream') ||
    normalized.includes('err_code=10058') ||
    normalized.includes('err_code=10065')
  );
}

// Event schemas with tracing support
export interface BaseEvent {
  traceId?: string;
  correlationId?: string;
  timestamp: string;
}

export interface UserRegisteredEvent extends BaseEvent {
  type: 'auth.user.registered';
  userId: string;
  email: string;
  name?: string;
  provider: string;
  emailVerified: boolean;
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
  metadata?: Record<string, any>;
}

export interface UserLoginEvent extends BaseEvent {
  type: 'auth.user.login';
  userId: string;
  email: string;
  sessionId: string;
  deviceInfo?: string;
  ipAddress?: string;
  userAgent?: string;
  provider?: string;
}

export interface UserLogoutEvent extends BaseEvent {
  type: 'auth.user.logout';
  userId: string;
  email: string;
  sessionId: string;
  reason?: 'manual' | 'timeout' | 'force';
}

export interface UserCreatedEvent extends BaseEvent {
  type: 'user.created';
  userId: string;
  email: string;
  name?: string;
  role?: string;
  emailVerified: boolean;
  metadata?: Record<string, any>;
}

export interface UserUpdatedEvent extends BaseEvent {
  type: 'user.updated';
  userId: string;
  email: string;
  changes: Record<string, any>;
}

export interface UserProfileUpdatedEvent extends BaseEvent {
  type: 'auth.user.profile_updated';
  userId: string;
  email: string;
  changes: Record<string, any>;
}

export interface SessionCreatedEvent extends BaseEvent {
  type: 'auth.session.created';
  sessionId: string;
  userId: string;
  email: string;
  deviceInfo?: string;
  ipAddress?: string;
  userAgent?: string;
  expiresAt: string;
}

export interface SessionEndedEvent extends BaseEvent {
  type: 'auth.session.ended';
  sessionId: string;
  userId: string;
  email: string;
  reason: 'logout' | 'expired' | 'revoked';
}

export interface OrganizationCreatedEvent extends BaseEvent {
  type: 'auth.organization.created';
  organizationId: string;
  name: string;
  slug: string;
  creatorId: string;
  creatorEmail: string;
  metadata?: Record<string, any>;
}

export interface OrganizationMemberAddedEvent extends BaseEvent {
  type: 'auth.organization.member_added';
  organizationId: string;
  organizationName: string;
  userId: string;
  userEmail: string;
  role: string;
  invitedBy?: string;
}

export interface OrganizationMemberRemovedEvent extends BaseEvent {
  type: 'auth.organization.member_removed';
  organizationId: string;
  organizationName: string;
  userId: string;
  userEmail: string;
  removedBy?: string;
}

export interface OrganizationPlanChangedEvent extends BaseEvent {
  type: 'organization.plan.changed';
  organizationId: string;
  organizationName: string;
  previousPlan: string;
  newPlan: string;
  changedBy?: string;
  changeReason?: string;
}

export interface OrganizationUpdatedEvent extends BaseEvent {
  type: 'organization.updated';
  organizationId: string;
  changes: Record<string, any>;
  updatedBy?: string;
}

export interface UserProviderLinkedEvent extends BaseEvent {
  type: 'auth.user.provider_linked';
  userId: string;
  email: string;
  /** The newly linked OAuth provider, e.g. 'microsoft', 'google' */
  provider: string;
  /** The provider's own subject/account ID */
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
}

export type AuthEvent =
  | UserRegisteredEvent
  | UserCreatedEvent
  | UserUpdatedEvent
  | UserLoginEvent
  | UserLogoutEvent
  | UserProfileUpdatedEvent
  | UserProviderLinkedEvent
  | SessionCreatedEvent
  | SessionEndedEvent
  | OrganizationCreatedEvent
  | OrganizationUpdatedEvent
  | OrganizationPlanChangedEvent
  | OrganizationMemberAddedEvent
  | OrganizationMemberRemovedEvent;

@Injectable()
export class AuthEventPublisher implements OnModuleInit, OnModuleDestroy {
  private readonly logger = new Logger(AuthEventPublisher.name);
  private natsConnection: NatsConnection | null = null;
  private jetStream: JetStreamClient | null = null;
  private stringCodec = StringCodec();
  private isEnabled = true;
  private enableDualPublish = true; // Enable dual-publish for migration period

  constructor(
    private configService: ConfigService,
    @Optional() private sharedNats: SharedNatsService,
  ) {
    this.isEnabled = this.configService.get<string>('NODE_ENV') !== 'test';
    // Allow disabling dual-publish via env var for testing target state
    this.enableDualPublish =
      this.configService.get<string>('ENABLE_DUAL_PUBLISH') !== 'false';
  }

  async onModuleInit() {
    if (!this.isEnabled) {
      this.logger.log('NATS event publishing disabled for testing');
      return;
    }

    try {
      await this.connectToNATS();
      await this.setupJetStream();
      this.logger.log('NATS event publisher initialized successfully');
    } catch (error) {
      this.logger.error('Failed to initialize NATS event publisher', error);
      // Don't throw error to prevent app startup failure
      this.isEnabled = false;
    }
  }

  async onModuleDestroy() {
    if (this.natsConnection) {
      await this.natsConnection.close();
      this.logger.log('NATS connection closed');
    }
  }

  private async connectToNATS() {
    const natsUrl =
      this.configService.get<string>('NATS_URL') || 'nats://localhost:4222';
    const natsToken =
      this.configService.get<string>('NATS_TOKEN') ||
      this.configService.get<string>('NATS_AUTH_TOKEN');
    const natsUser = this.configService.get<string>('NATS_USER');
    const natsPass = this.configService.get<string>('NATS_PASS');

    const connectionOptions: any = {
      servers: [natsUrl],
      name: 'auth-service-publisher',
      maxReconnectAttempts: 10,
      reconnectTimeWait: 2000,
    };

    // Token auth takes precedence (for production with aquatiq root container)
    if (natsToken) {
      connectionOptions.token = natsToken;
      this.logger.log('Using NATS token authentication');
    } else if (natsUser && natsPass) {
      connectionOptions.user = natsUser;
      connectionOptions.pass = natsPass;
      this.logger.log('Using NATS user/password authentication');
    } else {
      this.logger.log('Using NATS without authentication (development)');
    }

    this.natsConnection = await connect(connectionOptions);
    this.logger.log(`Connected to NATS: ${natsUrl}`);
  }

  private async setupJetStream() {
    if (!this.natsConnection) {
      throw new Error('NATS connection not established');
    }

    this.jetStream = this.natsConnection.jetstream();
    const jsm = await this.natsConnection.jetstreamManager();

    // Ensure auth events stream exists (OLD - compatibility)
    try {
      await jsm.streams.add({
        name: 'AUTH_EVENTS',
        subjects: ['auth.>'],
        retention: RetentionPolicy.Limits,
        max_msgs: 10000,
        max_age: 7 * 24 * 60 * 60 * 1000 * 1000000, // 7 days in nanoseconds
        storage: StorageType.File,
      });
      this.logger.log('AUTH_EVENTS stream configured (compatibility)');
    } catch (error) {
      if (!isNonFatalStreamSetupError(error)) {
        this.logger.warn(
          `AUTH_EVENTS stream setup issue: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
      this.logger.debug(
        'AUTH_EVENTS stream configuration skipped (might exist)',
      );
    }

    // Ensure new simplified event streams exist (TARGET)
    if (this.enableDualPublish) {
      try {
        await jsm.streams.add({
          name: 'USER_EVENTS',
          subjects: ['user.>', 'session.>'],
          retention: RetentionPolicy.Limits,
          max_msgs: 10000,
          max_age: 7 * 24 * 60 * 60 * 1000 * 1000000,
          storage: StorageType.File,
        });
        this.logger.log('USER_EVENTS stream configured (target)');
      } catch (error) {
        if (!isNonFatalStreamSetupError(error)) {
          this.logger.warn(
            `USER_EVENTS stream setup issue: ${error instanceof Error ? error.message : String(error)}`,
          );
        }
        this.logger.debug(
          'USER_EVENTS stream configuration skipped (might exist)',
        );
      }

      try {
        await jsm.streams.add({
          name: 'ORGANIZATION_EVENTS',
          subjects: ['organization.>'],
          retention: RetentionPolicy.Limits,
          max_msgs: 10000,
          max_age: 7 * 24 * 60 * 60 * 1000 * 1000000,
          storage: StorageType.File,
        });
        this.logger.log('ORGANIZATION_EVENTS stream configured (target)');
      } catch (error) {
        if (!isNonFatalStreamSetupError(error)) {
          this.logger.warn(
            `ORGANIZATION_EVENTS stream setup issue: ${error instanceof Error ? error.message : String(error)}`,
          );
        }
        this.logger.debug(
          'ORGANIZATION_EVENTS stream configuration skipped (might exist)',
        );
      }
    }
  }

  /**
   * Publish authentication event to NATS with dual-publish support
   * During migration period, publishes to both old and new event names
   */
  async publishEvent(event: AuthEvent): Promise<void> {
    if (!this.isEnabled || !this.jetStream) {
      this.logger.debug(`Event publishing skipped: ${event.type}`);
      return;
    }

    try {
      // Generate trace ID if not provided
      const enrichedEvent = {
        ...event,
        traceId: event.traceId || this.generateTraceId(),
        correlationId: event.correlationId || this.generateTraceId(),
        timestamp: new Date().toISOString(),
      };

      // Publish to OLD event name (compatibility)
      const oldSubject = this.getSubjectForEvent(enrichedEvent.type);
      const payload = this.stringCodec.encode(JSON.stringify(enrichedEvent));
      await this.jetStream.publish(oldSubject, payload);
      this.logger.debug(
        `Published event: ${enrichedEvent.type} to ${oldSubject} (old)`,
      );

      // Publish to NEW simplified event name (target) if dual-publish enabled
      if (this.enableDualPublish) {
        const newSubject = this.getTargetSubjectForEvent(enrichedEvent.type);
        const targetEvent = {
          ...enrichedEvent,
          type: newSubject.replace(/\./g, '.'), // Ensure proper subject formatting
        };
        const newPayload = this.stringCodec.encode(JSON.stringify(targetEvent));
        await this.jetStream.publish(newSubject, newPayload);
        this.logger.debug(`Published event: ${newSubject} (new)`);
      }
    } catch (error) {
      this.logger.error(`Failed to publish event: ${event.type}`, error);
      // Don't throw error to prevent business logic failure
    }
  }

  /**
   * Generate a unique trace ID for correlation
   */
  private generateTraceId(): string {
    return `${Date.now()}-${Math.random().toString(36).substring(2, 15)}`;
  }

  /**
   * Convert old event type to new simplified subject
   * auth.user.registered -> user.registered
   * auth.organization.created -> organization.created
   */
  private getTargetSubjectForEvent(oldEventType: string): string {
    // Remove 'auth.' prefix for new simplified naming
    return oldEventType.replace(/^auth\./, '');
  }

  /**
   * Publish user created event (simplified)
   */
  async publishUserCreated(
    data: Omit<UserCreatedEvent, 'type' | 'timestamp'>,
    traceId?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'user.created',
      traceId,
    } as UserCreatedEvent);
  }

  /**
   * Publish user updated event (simplified)
   */
  async publishUserUpdated(
    data: Omit<UserUpdatedEvent, 'type' | 'timestamp'>,
    traceId?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'user.updated',
      traceId,
    } as UserUpdatedEvent);
  }

  /**
   * Publish user registered event
   */
  async publishUserRegistered(
    data: Omit<UserRegisteredEvent, 'type' | 'timestamp'>,
    traceId?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'auth.user.registered',
      traceId,
    } as UserRegisteredEvent);

    // Also publish simplified user.created event
    await this.publishUserCreated(
      {
        userId: data.userId,
        email: data.email,
        name: data.name,
        role: undefined,
        emailVerified: data.emailVerified,
        metadata: data.metadata,
      },
      traceId,
    );

    // Cross-plane: publish to shared NATS for Ingestion/Data/Reasoning planes
    void this.sharedNats?.publish('aqencia.controlplane.user.registered', {
      user_id: data.userId,
      email: data.email,
      name: data.name,
      provider: data.provider,
      email_verified: data.emailVerified,
      tenant_id: data.tenantId,
      microsoft_tenant_id: data.microsoftTenantId,
      scopes_granted: data.scopesGranted,
      trace_id: traceId,
    });
  }

  /**
   * Publish user login event
   */
  async publishUserLogin(
    data: Omit<UserLoginEvent, 'type' | 'timestamp'>,
    traceId?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'auth.user.login',
      traceId,
    } as UserLoginEvent);

    // Cross-plane: notify other planes of sign-in (useful for session-aware services)
    void this.sharedNats?.publish('aqencia.controlplane.user.signed_in', {
      user_id: data.userId,
      email: data.email,
      session_id: data.sessionId,
      provider: data.provider,
      trace_id: traceId,
    });
  }

  /**
   * Publish user logout event
   */
  async publishUserLogout(
    data: Omit<UserLogoutEvent, 'type' | 'timestamp'>,
    traceId?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'auth.user.logout',
      traceId,
    } as UserLogoutEvent);
  }

  /**
   * Publish user provider linked event
   * Fired when an existing user connects a new OAuth provider to their account
   */
  async publishUserProviderLinked(
    data: Omit<UserProviderLinkedEvent, 'type' | 'timestamp'>,
    traceId?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'auth.user.provider_linked',
      traceId,
    } as UserProviderLinkedEvent);

    // Cross-plane: critical event — Ingestion subscribes to provision M365 sync
    void this.sharedNats?.publish('aqencia.controlplane.user.provider_linked', {
      user_id: data.userId,
      email: data.email,
      provider: data.provider,
      provider_account_id: data.providerAccountId,
      tenant_id: data.tenantId,
      microsoft_tenant_id: data.microsoftTenantId,
      scopes_granted: data.scopesGranted,
      token_ref: data.tokenRef,
      trace_id: traceId,
    });
  }

  /**
   * Publish user profile updated event
   */
  async publishUserProfileUpdated(
    data: Omit<UserProfileUpdatedEvent, 'type' | 'timestamp'>,
    traceId?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'auth.user.profile_updated',
      traceId,
    } as UserProfileUpdatedEvent);

    // Also publish simplified user.updated event
    await this.publishUserUpdated(
      {
        userId: data.userId,
        email: data.email,
        changes: data.changes,
      },
      traceId,
    );
  }

  /**
   * Publish session created event
   */
  async publishSessionCreated(
    data: Omit<SessionCreatedEvent, 'type' | 'timestamp'>,
    traceId?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'auth.session.created',
      traceId,
    } as SessionCreatedEvent);
  }

  /**
   * Publish session ended event
   */
  async publishSessionEnded(
    data: Omit<SessionEndedEvent, 'type' | 'timestamp'>,
    traceId?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'auth.session.ended',
      traceId,
    } as SessionEndedEvent);
  }

  /**
   * Publish organization created event
   */
  async publishOrganizationCreated(
    data: Omit<OrganizationCreatedEvent, 'type' | 'timestamp'>,
    traceId?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'auth.organization.created',
      traceId,
    } as OrganizationCreatedEvent);

    // Cross-plane: ALL planes need to provision resources for a new org
    void this.sharedNats?.publish('aqencia.controlplane.org.created', {
      org_id: data.organizationId,
      org_name: data.name,
      slug: data.slug,
      creator_id: data.creatorId,
      creator_email: data.creatorEmail,
      metadata: data.metadata,
      trace_id: traceId,
    });
  }

  /**
   * Publish organization member added event
   */
  async publishOrganizationMemberAdded(
    data: Omit<OrganizationMemberAddedEvent, 'type' | 'timestamp'>,
    traceId?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'auth.organization.member_added',
      traceId,
    } as OrganizationMemberAddedEvent);

    void this.sharedNats?.publish('aqencia.controlplane.org.member_added', {
      org_id: data.organizationId,
      org_name: data.organizationName,
      user_id: data.userId,
      user_email: data.userEmail,
      role: data.role,
      invited_by: data.invitedBy,
      trace_id: traceId,
    });

    // Notify the invited user via notification-core (plain NATS, not JetStream)
    this.sharedNats?.publishPlain('notifications.team.invite.sent', {
      subscriberId: data.userId,
      inviteeEmail: data.userEmail,
      orgId: data.organizationId,
      orgName: data.organizationName,
      orgSlug: '',
      inviterName: data.invitedBy ?? '',
      inviterEmail: '',
      inviteCode: '',
      role: data.role,
      actorId: data.invitedBy ?? '',
      actorName: data.invitedBy ?? '',
    });
  }

  /**
   * Publish organization member removed event
   */
  async publishOrganizationMemberRemoved(
    data: Omit<OrganizationMemberRemovedEvent, 'type' | 'timestamp'>,
    traceId?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'auth.organization.member_removed',
      traceId,
    } as OrganizationMemberRemovedEvent);

    void this.sharedNats?.publish('aqencia.controlplane.org.member_removed', {
      org_id: data.organizationId,
      org_name: data.organizationName,
      user_id: data.userId,
      user_email: data.userEmail,
      removed_by: data.removedBy,
      trace_id: traceId,
    });
  }

  /**
   * Publish organization updated event
   */
  async publishOrganizationUpdated(
    data: Omit<OrganizationUpdatedEvent, 'type' | 'timestamp'>,
    traceId?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'organization.updated',
      traceId,
    } as OrganizationUpdatedEvent);
  }

  /**
   * Publish organization plan changed event
   */
  async publishOrganizationPlanChanged(
    data: Omit<OrganizationPlanChangedEvent, 'type' | 'timestamp'>,
    traceId?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'organization.plan.changed',
      traceId,
    } as OrganizationPlanChangedEvent);
  }

  private getSubjectForEvent(eventType: string): string {
    // Convert auth.user.registered to auth.user.registered
    return eventType;
  }

  /**
   * Health check for NATS connection
   */
  isHealthy(): boolean {
    return (
      this.isEnabled &&
      this.natsConnection !== null &&
      !this.natsConnection.isClosed()
    );
  }
}
