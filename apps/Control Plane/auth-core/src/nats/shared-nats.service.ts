/**
 * Shared NATS Service
 *
 * Connects to the cross-plane NATS broker (velion-nats) and publishes
 * domain events on the `aqencia.controlplane.*` subject namespace so that
 * all other planes (Ingestion, Data, Reasoning) can subscribe to them.
 *
 * Subject convention: aqencia.controlplane.<entity>.<verb>
 *   aqencia.controlplane.org.created
 *   aqencia.controlplane.org.member_added
 *   aqencia.controlplane.org.member_removed
 *   aqencia.controlplane.user.registered
 *   aqencia.controlplane.user.signed_in
 *   aqencia.controlplane.user.provider_linked
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
  RetentionPolicy,
  StorageType,
} from 'nats';

function isNonFatalStreamError(err: unknown): boolean {
  const msg = (err instanceof Error ? err.message : String(err)).toLowerCase();
  return (
    msg.includes('stream name already in use') ||
    msg.includes('subjects overlap') ||
    msg.includes('err_code=10058') ||
    msg.includes('err_code=10065')
  );
}

@Injectable()
export class SharedNatsService implements OnModuleInit, OnModuleDestroy {
  private readonly logger = new Logger(SharedNatsService.name);
  private conn: NatsConnection | null = null;
  private js: JetStreamClient | null = null;
  private readonly sc = StringCodec();
  private enabled = false;

  constructor(private readonly config: ConfigService) {}

  async onModuleInit() {
    const url =
      this.config.get<string>('VELION_NATS_URL') ||
      this.config.get<string>('NATS_SHARED_URL');
    if (!url) {
      this.logger.warn(
        'VELION_NATS_URL not set — cross-plane event publishing disabled',
      );
      return;
    }

    try {
      const token =
        this.config.get<string>('VELION_NATS_TOKEN') ||
        this.config.get<string>('NATS_SHARED_TOKEN');

      this.conn = await connect({
        servers: [url],
        name: 'auth-core-shared',
        maxReconnectAttempts: -1,
        reconnectTimeWait: 3000,
        ...(token ? { token } : {}),
      });

      this.js = this.conn.jetstream();

      // Ensure the AQENCIA_CONTROLPLANE JetStream exists
      const jsm = await this.conn.jetstreamManager();
      try {
        await jsm.streams.add({
          name: 'AQENCIA_CONTROLPLANE',
          subjects: ['aqencia.controlplane.>'],
          retention: RetentionPolicy.Limits,
          max_msgs: 100_000,
          // 14 days in nanoseconds
          max_age: 14 * 24 * 60 * 60 * 1_000_000_000,
          storage: StorageType.File,
          duplicate_window: 60_000_000_000, // 60s dedup window (ns)
        });
        this.logger.log('AQENCIA_CONTROLPLANE JetStream stream ready');
      } catch (err) {
        if (!isNonFatalStreamError(err)) {
          this.logger.warn(
            `AQENCIA_CONTROLPLANE stream setup: ${err instanceof Error ? err.message : String(err)}`,
          );
        } else {
          this.logger.debug('AQENCIA_CONTROLPLANE stream already exists');
        }
      }

      this.enabled = true;
      this.logger.log(`Connected to shared NATS at ${url}`);
    } catch (err) {
      this.logger.error(
        'Failed to connect to shared NATS — cross-plane publishing disabled',
        err,
      );
      // Non-fatal: shared NATS is opt-in; the service continues without it
    }
  }

  async onModuleDestroy() {
    if (this.conn) {
      await this.conn.drain();
      this.logger.log('Shared NATS connection drained');
    }
  }

  /**
   * Publish a cross-plane event to the shared NATS JetStream.
   *
   * @param subject  Full subject, e.g. "aqencia.controlplane.org.created"
   * @param payload  Event payload (JSON-serialisable object)
   */
  async publish(
    subject: string,
    payload: Record<string, unknown>,
  ): Promise<void> {
    if (!this.enabled || !this.js) {
      return;
    }

    try {
      const enriched = {
        ...payload,
        _source: 'auth-core',
        _published_at: new Date().toISOString(),
      };

      await this.js.publish(subject, this.sc.encode(JSON.stringify(enriched)));

      this.logger.debug(`SharedNATS → ${subject}`);
    } catch (err) {
      // Fire-and-forget: never fail the caller due to shared NATS issues
      this.logger.error(`Failed to publish "${subject}" to shared NATS`, err);
    }
  }

  /**
   * Publish an event via plain NATS core (not JetStream).
   * Used for subjects that have plain-NATS subscribers (e.g. notification-core).
   */
  publishPlain(subject: string, payload: Record<string, unknown>): void {
    if (!this.conn || this.conn.isClosed()) {
      return;
    }
    try {
      this.conn.publish(subject, this.sc.encode(JSON.stringify(payload)));
      this.logger.debug(`SharedNATS plain → ${subject}`);
    } catch (err) {
      this.logger.error(`Failed to plain-publish "${subject}"`, err);
    }
  }

  isConnected(): boolean {
    return this.enabled && this.conn !== null && !this.conn.isClosed();
  }
}
