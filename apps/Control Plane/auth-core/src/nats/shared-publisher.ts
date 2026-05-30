import { Injectable, Logger } from '@nestjs/common';
import { SharedNatsService } from './shared-nats.service';

/**
 * SharedPublisher is a typed facade on top of SharedNatsService
 * that provides strongly-typed publish methods for cross-plane domain events.
 *
 * Subject convention: aqencia.controlplane.<entity>.<verb>
 *   aqencia.controlplane.user.registered
 *   aqencia.controlplane.user.created
 *   aqencia.controlplane.user.oauth_callback
 */
@Injectable()
export class SharedPublisher {
  private readonly logger = new Logger(SharedPublisher.name);

  constructor(private readonly sharedNats: SharedNatsService) {}

  /**
   * Connect is a no-op since SharedNatsService initializes itself via OnModuleInit
   */
  async connect(sharedURL: string, token: string): Promise<void> {
    if (this.sharedNats.isConnected()) {
      this.logger.log('✅ Shared NATS (connected via SharedNatsService)');
    } else {
      this.logger.warn(
        '⚠️  Shared NATS not yet connected via SharedNatsService',
      );
    }
  }

  /**
   * PublishUserRegistered publishes aqencia.controlplane.user.registered.
   */
  async publishUserRegistered(
    userId: string,
    email: string,
    name: string,
    provider: string,
  ): Promise<void> {
    await this.sharedNats.publish('aqencia.controlplane.user.registered', {
      user_id: userId,
      email,
      name,
      provider,
    });
  }

  /**
   * PublishUserCreated publishes aqencia.controlplane.user.created.
   */
  async publishUserCreated(
    userId: string,
    email: string,
    name: string,
  ): Promise<void> {
    await this.sharedNats.publish('aqencia.controlplane.user.created', {
      user_id: userId,
      email,
      name,
    });
  }

  /**
   * PublishOAuthCallback publishes aqencia.controlplane.user.oauth_callback.
   * Critical event for Ingestion Plane to handle M365 connectors.
   */
  async publishOAuthCallback(
    userId: string,
    email: string,
    provider: string,
    metadata?: Record<string, any>,
  ): Promise<void> {
    await this.sharedNats.publish('aqencia.controlplane.user.oauth_callback', {
      user_id: userId,
      email,
      provider,
      metadata: metadata || {},
    });
  }

  /**
   * Close is a no-op since SharedNatsService manages its own lifecycle
   */
  async close(): Promise<void> {
    this.logger.log(
      '🔌 SharedPublisher close (no-op, SharedNatsService handles cleanup)',
    );
  }
}
