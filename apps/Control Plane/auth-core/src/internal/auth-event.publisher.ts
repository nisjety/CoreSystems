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
} from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import {
  connect,
  NatsConnection,
  StringCodec,
  JetStreamClient,
  type ConnectionOptions,
} from 'nats';
import { SharedNatsService } from '../nats/shared-nats.service';
import { selectNatsCredentials } from '../nats/nats-credentials';

const AUDIT_EVENT_ID = /^[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}$/;
const AUDIT_EVENT_NAME = /^[a-z0-9_]+$/;

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
  metadata?: Record<string, unknown>;
  /** Active org at sign-up time. */
  activeOrganizationId?: string;
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
  /** Active org at sign-in time. */
  activeOrganizationId?: string;
}

export interface UserLogoutEvent extends BaseEvent {
  type: 'auth.user.logout';
  userId: string;
  email: string;
  sessionId: string;
  reason?: 'manual' | 'timeout' | 'force';
  /** Active org at sign-out time. */
  activeOrganizationId?: string;
}

export interface UserCreatedEvent extends BaseEvent {
  type: 'user.created';
  userId: string;
  email: string;
  name?: string;
  role?: string;
  emailVerified: boolean;
  metadata?: Record<string, unknown>;
}

export interface UserUpdatedEvent extends BaseEvent {
  type: 'user.updated';
  userId: string;
  email: string;
  changes: Record<string, unknown>;
}

export interface UserProfileUpdatedEvent extends BaseEvent {
  type: 'auth.user.profile_updated';
  userId: string;
  email: string;
  changes: Record<string, unknown>;
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
  metadata?: Record<string, unknown>;
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
  changes: Record<string, unknown>;
  updatedBy?: string;
}

export interface OrganizationDeletedEvent extends BaseEvent {
  type: 'organization.deleted';
  organizationId: string;
  reason: string;
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
  | OrganizationDeletedEvent
  | OrganizationPlanChangedEvent
  | OrganizationMemberAddedEvent
  | OrganizationMemberRemovedEvent;

const ORGANIZATION_PROJECTION_SUBJECT = 'aqencia.controlplane.org.changed';
const ORGANIZATION_MEMBERSHIP_PROJECTION_SUBJECT =
  'aqencia.controlplane.org.member_changed';

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
    private sharedNats: SharedNatsService,
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
      this.setupJetStream();
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
    const credentials = selectNatsCredentials({
      NATS_TOKEN: this.configService.get<string>('NATS_TOKEN'),
      NATS_AUTH_TOKEN: this.configService.get<string>('NATS_AUTH_TOKEN'),
      NATS_USER: this.configService.get<string>('NATS_USER'),
      NATS_PASSWORD: this.configService.get<string>('NATS_PASSWORD'),
      NATS_PASS: this.configService.get<string>('NATS_PASS'),
      NATS_ALLOW_TOKEN_FALLBACK: this.configService.get<string>(
        'NATS_ALLOW_TOKEN_FALLBACK',
      ),
    });

    const connectionOptions: ConnectionOptions = {
      servers: [natsUrl],
      name: 'auth-service-publisher',
      maxReconnectAttempts: 10,
      reconnectTimeWait: 2000,
      inboxPrefix: '_INBOX.AUTH_CONTROL',
    };

    Object.assign(connectionOptions, credentials);

    this.natsConnection = await connect(connectionOptions);
    this.logger.log(`Connected to NATS: ${natsUrl}`);
  }

  private setupJetStream() {
    if (!this.natsConnection) {
      throw new Error('NATS connection not established');
    }

    this.jetStream = this.natsConnection.jetstream();
    // Deployment-only audit-nats-provisioner owns stream topology. Runtime
    // auth-core receives publish/PubAck capability only, never stream admin.
    this.logger.log(
      'JetStream publisher ready (streams provisioned externally)',
    );
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
    idempotencyKey?: string,
  ): Promise<void> {
    const sharedNats = this.requireSharedNats();
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
    await sharedNats.publish(
      'aqencia.controlplane.user.registered',
      {
        user_id: data.userId,
        email: data.email,
        name: data.name,
        provider: data.provider,
        email_verified: data.emailVerified,
        tenant_id: data.tenantId,
        microsoft_tenant_id: data.microsoftTenantId,
        scopes_granted: data.scopesGranted,
        trace_id: traceId,
      },
      idempotencyKey ? { msgID: idempotencyKey } : undefined,
    );
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
    await this.sharedNats?.publish('aqencia.controlplane.user.signed_in', {
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
    idempotencyKey?: string,
  ): Promise<void> {
    const sharedNats = this.requireSharedNats();
    await this.publishEvent({
      ...data,
      type: 'auth.user.provider_linked',
      traceId,
    } as UserProviderLinkedEvent);

    // Cross-plane: critical event — Ingestion subscribes to provision M365 sync
    await sharedNats.publish(
      'aqencia.controlplane.user.provider_linked',
      {
        user_id: data.userId,
        email: data.email,
        provider: data.provider,
        provider_account_id: data.providerAccountId,
        tenant_id: data.tenantId,
        microsoft_tenant_id: data.microsoftTenantId,
        scopes_granted: data.scopesGranted,
        token_ref: data.tokenRef,
        trace_id: traceId,
      },
      idempotencyKey ? { msgID: idempotencyKey } : undefined,
    );
  }

  private requireSharedNats(): SharedNatsService {
    if (!this.sharedNats) {
      throw new Error('shared NATS publisher unavailable');
    }
    return this.sharedNats;
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
    idempotencyKey?: string,
  ): Promise<void> {
    await this.publishEvent({
      ...data,
      type: 'auth.organization.created',
      traceId,
    } as OrganizationCreatedEvent);

    // Cross-plane: ALL planes need to provision resources for a new org
    await this.sharedNats?.publish(
      'aqencia.controlplane.org.created',
      {
        org_id: data.organizationId,
        org_name: data.name,
        slug: data.slug,
        creator_id: data.creatorId,
        creator_email: data.creatorEmail,
        metadata: data.metadata,
        trace_id: traceId,
      },
      { msgID: idempotencyKey },
    );
  }

  async publishOrganizationProjection(
    data: {
      organizationId: string;
      name: string;
      slug: string;
      ownerUserId: string;
      metadata: Record<string, unknown>;
      revision: number;
    },
    idempotencyKey: string,
  ): Promise<void> {
    await this.requireSharedNats().publish(
      ORGANIZATION_PROJECTION_SUBJECT,
      {
        schema_version: 1,
        event_id: idempotencyKey,
        action: 'upsert',
        org_id: data.organizationId,
        name: data.name,
        slug: data.slug,
        owner_user_id: data.ownerUserId,
        metadata: data.metadata,
        revision: data.revision,
      },
      { msgID: idempotencyKey },
    );
  }

  async publishOrganizationMembershipProjection(
    data: {
      organizationId: string;
      userId: string;
      role: string;
      action: 'upsert' | 'remove';
      revision: number;
      organizationRevision: number;
      userEmail?: string;
    },
    idempotencyKey: string,
  ): Promise<void> {
    await this.requireSharedNats().publish(
      ORGANIZATION_MEMBERSHIP_PROJECTION_SUBJECT,
      {
        schema_version: 1,
        event_id: idempotencyKey,
        action: data.action,
        org_id: data.organizationId,
        user_id: data.userId,
        ...(data.action === 'upsert'
          ? { role: data.role, user_email: data.userEmail }
          : {}),
        revision: data.revision,
        organization_revision: data.organizationRevision,
      },
      { msgID: idempotencyKey },
    );
  }

  async publishOrganizationDeletionProjection(
    data: { organizationId: string; revision: number },
    idempotencyKey: string,
  ): Promise<void> {
    await this.requireSharedNats().publish(
      ORGANIZATION_PROJECTION_SUBJECT,
      {
        schema_version: 1,
        event_id: idempotencyKey,
        action: 'remove',
        org_id: data.organizationId,
        revision: data.revision,
      },
      { msgID: idempotencyKey },
    );
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

    await this.sharedNats?.publish('aqencia.controlplane.org.member_added', {
      org_id: data.organizationId,
      org_name: data.organizationName,
      user_id: data.userId,
      user_email: data.userEmail,
      role: data.role,
      invited_by: data.invitedBy,
      trace_id: traceId,
    });

    // Notify the invited user via notification-core (plain NATS, not JetStream)
    await this.sharedNats?.publishPlain('notifications.team.invite.sent', {
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

    await this.sharedNats?.publish('aqencia.controlplane.org.member_removed', {
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
   * Publish an audit event to velion.audit.v2.control.auth-core.<event> on Control's
   * plane-local broker. Audit Core pins this broker to the control authority.
   *
   * org_id is REQUIRED by audit-core. Durable identity and producer occurrence
   * time are mandatory even when org context is absent; callers must never
   * replace them with request-time randomness.
   */
  async publishVelionAudit(payload: {
    occurred_at: Date | string;
    org_id: string | undefined;
    user_id?: string;
    actor_role?: string;
    event: string;
    subject?: string;
    resource_id?: string;
    outcome: 'ok' | 'denied' | 'error';
    details?: Record<string, unknown>;
    event_id: string;
    request_id?: string;
    ip_address?: string;
    user_agent?: string;
  }): Promise<void> {
    if (
      !AUDIT_EVENT_ID.test(payload.event_id) ||
      !AUDIT_EVENT_NAME.test(payload.event)
    ) {
      throw new Error('Invalid durable audit identity');
    }
    const occurredAt = new Date(payload.occurred_at);
    if (Number.isNaN(occurredAt.getTime())) {
      throw new Error('Invalid durable audit identity');
    }
    if (!payload.org_id) return;
    if (!this.isEnabled || !this.jetStream) {
      throw new Error('Durable audit transport unavailable');
    }
    const natsSubject = `velion.audit.v2.control.auth-core.${payload.event}`;
    const encoded = this.stringCodec.encode(
      JSON.stringify({
        occurred_at: occurredAt.toISOString(),
        event_id: payload.event_id,
        org_id: payload.org_id,
        user_id: payload.user_id,
        actor_role: payload.actor_role,
        plane: 'control',
        producer: 'auth-core',
        event: payload.event,
        subject: payload.subject,
        resource_id: payload.resource_id,
        outcome: payload.outcome,
        details: payload.details,
        request_id: payload.request_id,
        ip_address: payload.ip_address,
        user_agent: payload.user_agent,
      }),
    );
    const ack = await this.jetStream.publish(natsSubject, encoded, {
      msgID: payload.event_id,
    });
    if (!ack?.stream || !Number.isSafeInteger(ack.seq) || ack.seq <= 0) {
      throw new Error('Invalid durable audit PubAck');
    }
    this.logger.debug(
      `velion.audit → ${natsSubject} (${ack.stream}:${ack.seq})`,
    );
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
