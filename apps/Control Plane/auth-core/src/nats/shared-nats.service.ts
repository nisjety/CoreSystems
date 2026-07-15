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
  ConnectionOptions,
} from 'nats';
import { selectNatsCredentials } from './nats-credentials';

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
      const credentials = selectNatsCredentials({
        NATS_USER: this.config.get<string>('NATS_SHARED_USER'),
        NATS_PASSWORD: this.config.get<string>('NATS_SHARED_PASSWORD'),
        NATS_TOKEN: this.config.get<string>('NATS_SHARED_TOKEN'),
        NATS_ALLOW_TOKEN_FALLBACK: this.config.get<string>(
          'NATS_SHARED_ALLOW_TOKEN_FALLBACK',
        ),
      });
      if (Object.keys(credentials).length === 0) {
        throw new Error('shared NATS requires scoped credentials');
      }
      const options: ConnectionOptions = {
        servers: [url],
        name: 'auth-core-shared',
        maxReconnectAttempts: -1,
        reconnectTimeWait: 3000,
        inboxPrefix: '_INBOX.AUTH_SHARED',
      };
      Object.assign(options, credentials);
      this.conn = await connect({
        ...options,
      });

      this.js = this.conn.jetstream();
      this.enabled = true;
      this.logger.log(`Connected to shared NATS at ${url}`);
    } catch (err) {
      this.logger.error(
        'Failed to connect to shared NATS — cross-plane publishing disabled',
        err,
      );
      throw err;
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
    options?: { msgID?: string },
  ): Promise<void> {
    if (!this.enabled || !this.js) {
      throw new Error('shared NATS publisher unavailable');
    }

    const enriched = {
      ...payload,
      _source: 'auth-core',
      _published_at: new Date().toISOString(),
    };
    const ack = await this.js.publish(
      subject,
      this.sc.encode(JSON.stringify(enriched)),
      options?.msgID ? { msgID: options.msgID } : undefined,
    );
    if (!ack.stream || !Number.isSafeInteger(ack.seq) || ack.seq <= 0) {
      throw new Error('invalid shared NATS PubAck');
    }
    this.logger.debug(`SharedNATS → ${subject} (${ack.stream}:${ack.seq})`);
  }

  /**
   * Publish an event via plain NATS core (not JetStream).
   * Used for subjects that have plain-NATS subscribers (e.g. notification-core).
   */
  async publishPlain(
    subject: string,
    payload: Record<string, unknown>,
  ): Promise<void> {
    if (!this.conn || this.conn.isClosed()) {
      throw new Error('shared NATS connection unavailable');
    }
    this.conn.publish(subject, this.sc.encode(JSON.stringify(payload)));
    await this.conn.flush();
    this.logger.debug(`SharedNATS plain → ${subject}`);
  }

  isConnected(): boolean {
    return this.enabled && this.conn !== null && !this.conn.isClosed();
  }
}
