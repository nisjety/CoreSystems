import { Injectable, Logger } from '@nestjs/common';
import { SharedNatsService } from './shared-nats.service';

/**
 * SharedPublisher is a typed facade on top of SharedNatsService
 * that provides strongly-typed publish methods for cross-plane domain events.
 *
 * Subject convention: controlplane.<entity>.<verb>
 *   controlplane.user.registered
 *   controlplane.user.created
 *   controlplane.user.oauth_callback
 */
@Injectable()
export class SharedPublisher {
  private readonly logger = new Logger(SharedPublisher.name);

  constructor(private readonly sharedNats: SharedNatsService) {}

  /**
   * Connect is a no-op since SharedNatsService initializes itself via OnModuleInit
   */
  connect(sharedURL: string, token: string): void {
    void sharedURL;
    void token;
    if (this.sharedNats.isConnected()) {
      this.logger.log('✅ Shared NATS (connected via SharedNatsService)');
    } else {
      this.logger.warn(
        '⚠️  Shared NATS not yet connected via SharedNatsService',
      );
    }
  }

  /**
   * PublishUserRegistered publishes controlplane.user.registered.
   */
  async publishUserRegistered(
    userId: string,
    email: string,
    name: string,
    provider: string,
  ): Promise<void> {
    await this.sharedNats.publish('controlplane.user.registered', {
      user_id: userId,
      email,
      name,
      provider,
    });
  }

  /**
   * PublishUserCreated publishes controlplane.user.created.
   */
  async publishUserCreated(
    userId: string,
    email: string,
    name: string,
  ): Promise<void> {
    await this.sharedNats.publish('controlplane.user.created', {
      user_id: userId,
      email,
      name,
    });
  }

  /**
   * PublishOAuthCallback publishes controlplane.user.oauth_callback.
   * Critical event for Ingestion Plane to handle M365 connectors.
   */
  async publishOAuthCallback(
    userId: string,
    email: string,
    provider: string,
    metadata?: Record<string, unknown>,
  ): Promise<void> {
    await this.sharedNats.publish('controlplane.user.oauth_callback', {
      user_id: userId,
      email,
      provider,
      metadata: metadata ?? {},
    });
  }

  /**
   * Close is a no-op since SharedNatsService manages its own lifecycle
   */
  close(): void {
    this.logger.log(
      '🔌 SharedPublisher close (no-op, SharedNatsService handles cleanup)',
    );
  }
}
