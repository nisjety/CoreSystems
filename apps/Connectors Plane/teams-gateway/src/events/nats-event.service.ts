/**
 * NATS Event Listener Service for Teams Gateway
 * 
 * Subscribes to authentication and user events from the auth service
 * and user service to maintain consistent user state
 */

import { Injectable, Logger, OnModuleInit, OnModuleDestroy } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { connect, NatsConnection, StringCodec, JetStreamClient, ConsumerConfig, AckPolicy, DeliverPolicy } from 'nats';

// Import auth event types from auth service
export interface UserRegisteredEvent {
  type: 'auth.user.registered';
  userId: string;
  email: string;
  name?: string;
  provider: string;
  emailVerified: boolean;
  tenantId?: string;
  metadata?: Record<string, any>;
  timestamp: string;
}

export interface UserLoginEvent {
  type: 'auth.user.login';
  userId: string;
  email: string;
  sessionId: string;
  deviceInfo?: string;
  ipAddress?: string;
  userAgent?: string;
  provider?: string;
  timestamp: string;
}

export interface UserLogoutEvent {
  type: 'auth.user.logout';
  userId: string;
  email: string;
  sessionId: string;
  reason?: 'manual' | 'timeout' | 'force';
  timestamp: string;
}

export interface SessionCreatedEvent {
  type: 'auth.session.created';
  sessionId: string;
  userId: string;
  email: string;
  deviceInfo?: string;
  ipAddress?: string;
  userAgent?: string;
  expiresAt: string;
  timestamp: string;
}

export interface SessionEndedEvent {
  type: 'auth.session.ended';
  sessionId: string;
  userId: string;
  email: string;
  reason: 'logout' | 'expired' | 'revoked';
  timestamp: string;
}

export type AuthEvent = 
  | UserRegisteredEvent 
  | UserLoginEvent 
  | UserLogoutEvent 
  | SessionCreatedEvent 
  | SessionEndedEvent;

@Injectable()
export class NatsEventService implements OnModuleInit, OnModuleDestroy {
  private readonly logger = new Logger(NatsEventService.name);
  private natsConnection: NatsConnection | null = null;
  private jetStream: JetStreamClient | null = null;
  private stringCodec = StringCodec();
  private isEnabled = true;

  constructor(
    private configService: ConfigService,
  ) {
    this.isEnabled = this.configService.get<boolean>('ENABLE_NATS_EVENTS', true) && 
                     this.configService.get<string>('NODE_ENV') !== 'test';
  }

  async onModuleInit() {
    if (!this.isEnabled) {
      this.logger.log('NATS event listening disabled');
      return;
    }

    try {
      await this.connectToNATS();
      await this.setupJetStream();
      await this.subscribeToEvents();
      this.logger.log('NATS event listener initialized successfully');
    } catch (error) {
      this.logger.error('Failed to initialize NATS event listener', error);
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
    const natsUrl = this.configService.get<string>('NATS_URL') || 'nats://localhost:4222';
    const natsUser = this.configService.get<string>('NATS_USER');
    const natsPass = this.configService.get<string>('NATS_PASS');

    const connectionOptions: any = {
      servers: [natsUrl],
      name: 'teams-gateway-listener',
      maxReconnectAttempts: 10,
      reconnectTimeWait: 2000,
    };

    if (natsUser && natsPass) {
      connectionOptions.user = natsUser;
      connectionOptions.pass = natsPass;
    }

    this.natsConnection = await connect(connectionOptions);
    this.logger.log(`Connected to NATS: ${natsUrl}`);
  }

  private async setupJetStream() {
    if (!this.natsConnection) {
      throw new Error('NATS connection not established');
    }

    this.jetStream = this.natsConnection.jetstream();
    this.logger.log('JetStream client initialized');
  }

  private async subscribeToEvents() {
    if (!this.jetStream) {
      throw new Error('JetStream not initialized');
    }

    // Subscribe to authentication events
    const authConsumerConfig: Partial<ConsumerConfig> = {
      durable_name: 'teams-gateway-auth-consumer',
      deliver_policy: DeliverPolicy.New,
      ack_policy: AckPolicy.Explicit,
      max_deliver: 3,
      filter_subject: 'auth.>'
    };

    const authSubscription = await this.jetStream.subscribe('auth.>', authConsumerConfig);
    
    this.logger.log('Subscribed to auth events');

    // Handle auth events
    (async () => {
      for await (const message of authSubscription) {
        try {
          const data = this.stringCodec.decode(message.data);
          const event = JSON.parse(data) as AuthEvent;
          
          await this.handleAuthEvent(event);
          message.ack();
          
        } catch (error) {
          this.logger.error('Error processing auth event', error);
          message.nak();
        }
      }
    })();

    // Subscribe to user events
    const userConsumerConfig: Partial<ConsumerConfig> = {
      durable_name: 'teams-gateway-user-consumer',
      deliver_policy: DeliverPolicy.New,
      ack_policy: AckPolicy.Explicit,
      max_deliver: 3,
      filter_subject: 'user.>'
    };

    const userSubscription = await this.jetStream.subscribe('user.>', userConsumerConfig);
    
    this.logger.log('Subscribed to user events');

    // Handle user events
    (async () => {
      for await (const message of userSubscription) {
        try {
          const data = this.stringCodec.decode(message.data);
          const event = JSON.parse(data);
          
          await this.handleUserEvent(event);
          message.ack();
          
        } catch (error) {
          this.logger.error('Error processing user event', error);
          message.nak();
        }
      }
    })();
  }

  /**
   * Handle authentication events
   */
  private async handleAuthEvent(event: AuthEvent): Promise<void> {
    this.logger.debug(`Received auth event: ${event.type} for user ${event.userId}`);

    switch (event.type) {
      case 'auth.user.registered':
        await this.handleUserRegistered(event);
        break;
      
      case 'auth.user.login':
        await this.handleUserLogin(event);
        break;
      
      case 'auth.user.logout':
        await this.handleUserLogout(event);
        break;
      
      case 'auth.session.created':
        await this.handleSessionCreated(event);
        break;
      
      case 'auth.session.ended':
        await this.handleSessionEnded(event);
        break;
      
      default:
        this.logger.warn(`Unknown auth event type: ${(event as any).type}`);
    }
  }

  /**
   * Handle user service events
   */
  private async handleUserEvent(event: any): Promise<void> {
    this.logger.debug(`Received user event: ${event.type} for user ${event.userId}`);

    // Handle user profile updates, preferences changes, etc.
    switch (event.type) {
      case 'user.profile.updated':
        await this.handleUserProfileUpdated(event);
        break;
      
      case 'user.preferences.updated':
        await this.handleUserPreferencesUpdated(event);
        break;
      
      default:
        this.logger.debug(`Unhandled user event type: ${event.type}`);
    }
  }

  /**
   * Handle user registration event
   */
  private async handleUserRegistered(event: UserRegisteredEvent): Promise<void> {
    try {
      // Initialize user state in Teams Gateway
      // This could create default user preferences, conversations, etc.
      this.logger.log(`New user registered: ${event.email} (${event.userId})`);
      
      // TODO: Initialize user state in teams-gateway database
      // await this.userStateService.initializeUser(event.userId, {
      //   email: event.email,
      //   name: event.name,
      //   tenantId: event.tenantId
      // });
      
    } catch (error) {
      this.logger.error(`Error handling user registration for ${event.userId}`, error);
    }
  }

  /**
   * Handle user login event
   */
  private async handleUserLogin(event: UserLoginEvent): Promise<void> {
    try {
      this.logger.log(`User logged in: ${event.email} (session: ${event.sessionId})`);
      
      // TODO: Update user session state
      // await this.userStateService.updateActiveSession(event.userId, event.sessionId);
      
    } catch (error) {
      this.logger.error(`Error handling user login for ${event.userId}`, error);
    }
  }

  /**
   * Handle user logout event
   */
  private async handleUserLogout(event: UserLogoutEvent): Promise<void> {
    try {
      this.logger.log(`User logged out: ${event.email} (reason: ${event.reason})`);
      
      // TODO: Clear user session state
      // await this.userStateService.clearActiveSession(event.userId);
      
    } catch (error) {
      this.logger.error(`Error handling user logout for ${event.userId}`, error);
    }
  }

  /**
   * Handle session created event
   */
  private async handleSessionCreated(event: SessionCreatedEvent): Promise<void> {
    try {
      this.logger.log(`Session created: ${event.sessionId} for user ${event.userId}`);
      
      // TODO: Track active sessions
      // await this.userStateService.trackSession(event.sessionId, event.userId);
      
    } catch (error) {
      this.logger.error(`Error handling session creation for ${event.userId}`, error);
    }
  }

  /**
   * Handle session ended event
   */
  private async handleSessionEnded(event: SessionEndedEvent): Promise<void> {
    try {
      this.logger.log(`Session ended: ${event.sessionId} (reason: ${event.reason})`);
      
      // TODO: Clean up session data
      // await this.userStateService.endSession(event.sessionId);
      
    } catch (error) {
      this.logger.error(`Error handling session end for ${event.userId}`, error);
    }
  }

  /**
   * Handle user profile updated event
   */
  private async handleUserProfileUpdated(event: any): Promise<void> {
    try {
      this.logger.log(`User profile updated: ${event.userId}`);
      
      // TODO: Update local user data cache
      // await this.userStateService.updateUserProfile(event.userId, event.changes);
      
    } catch (error) {
      this.logger.error(`Error handling user profile update for ${event.userId}`, error);
    }
  }

  /**
   * Handle user preferences updated event
   */
  private async handleUserPreferencesUpdated(event: any): Promise<void> {
    try {
      this.logger.log(`User preferences updated: ${event.userId}`);
      
      // TODO: Update user preferences in local state
      // await this.userStateService.updatePreferences(event.userId, event.preferences);
      
    } catch (error) {
      this.logger.error(`Error handling user preferences update for ${event.userId}`, error);
    }
  }

  /**
   * Health check for NATS connection
   */
  isHealthy(): boolean {
    return this.isEnabled && this.natsConnection !== null && !this.natsConnection.isClosed();
  }
}