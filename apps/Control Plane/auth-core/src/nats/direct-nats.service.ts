import { Injectable, OnModuleInit, OnModuleDestroy } from '@nestjs/common';
import {
  connect,
  NatsConnection,
  StringCodec,
  Msg,
  type ConnectionOptions,
  type JetStreamClient,
} from 'nats';
import { selectNatsCredentials } from './nats-credentials';
import {
  authorizeAuthInternalService,
  loadAuthInternalServiceCredentials,
} from '../internal/internal-service-auth';

type ServiceAuthenticationRequest = {
  credentialId: string;
  serviceId: string;
  serviceSecret: string;
};

const AUTH_AUDIT_SUBJECT =
  /^verevon\.audit\.v2\.control\.auth-core\.([a-z0-9_]+)$/;
const AUDIT_EVENT_ID = /^[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}$/;

function isServiceAuthenticationRequest(
  value: unknown,
): value is ServiceAuthenticationRequest {
  return (
    typeof value === 'object' &&
    value !== null &&
    'credentialId' in value &&
    typeof value.credentialId === 'string' &&
    'serviceId' in value &&
    typeof value.serviceId === 'string' &&
    'serviceSecret' in value &&
    typeof value.serviceSecret === 'string'
  );
}

/**
 * Direct NATS Service for Request-Reply Pattern
 *
 * This service provides direct NATS pub/sub and request-reply functionality
 * bypassing NestJS microservices layer which has issues with reply subjects.
 *
 * Use this for service-to-service authentication where Go clients use nc.Request()
 */
@Injectable()
export class DirectNatsService implements OnModuleInit, OnModuleDestroy {
  private nc: NatsConnection;
  private jetStream: JetStreamClient | null = null;
  private sc = StringCodec();
  private readonly serviceCredentials = loadAuthInternalServiceCredentials();

  async onModuleInit() {
    try {
      console.log('🔌 Connecting to NATS for direct request-reply...');

      const connectionOptions: ConnectionOptions = {
        servers: [process.env.NATS_URL || 'nats://nats:4222'],
        maxReconnectAttempts: -1,
        reconnectTimeWait: 2000,
        name: 'auth-service-direct',
        inboxPrefix: '_INBOX.AUTH_CONTROL',
      };

      Object.assign(connectionOptions, selectNatsCredentials(process.env));

      this.nc = await connect(connectionOptions);
      this.jetStream = this.nc.jetstream();

      console.log('✅ Direct NATS connection established');

      // Set up service authentication handler
      this.setupServiceAuthentication();
    } catch (error) {
      console.error('❌ Failed to connect to NATS:', error);
    }
  }

  async onModuleDestroy() {
    if (this.nc) {
      await this.nc.drain();
      console.log('🔌 Direct NATS connection closed');
    }
  }

  /**
   * Set up handler for service.authenticate requests
   * This allows Go services to authenticate via NATS request-reply
   */
  private setupServiceAuthentication(): void {
    const sub = this.nc.subscribe('service.authenticate');

    console.log('👂 Listening for service.authenticate requests...');

    // Process messages
    void (async () => {
      for await (const msg of sub) {
        this.handleServiceAuthentication(msg);
      }
    })();
  }

  /**
   * Handle service authentication request
   */
  private handleServiceAuthentication(msg: Msg): void {
    try {
      const data: unknown = JSON.parse(this.sc.decode(msg.data));

      console.log('🔔 NATS: Received service.authenticate request');
      console.log('📬 Reply subject:', msg.reply);

      if (!isServiceAuthenticationRequest(data)) {
        msg.respond(
          this.sc.encode(
            JSON.stringify({
              authenticated: false,
              error: 'Invalid service authentication request',
            }),
          ),
        );
        return;
      }
      console.log('🔑 Service ID:', data.serviceId);

      const authorized = authorizeAuthInternalService(
        {
          credentialId: data.credentialId,
          principal: data.serviceId,
          token: data.serviceSecret,
        },
        this.serviceCredentials,
        'nats:authenticate',
      );

      const response = {
        authenticated: true,
        credentialId: authorized.credentialId,
        serviceId: authorized.principal,
      };

      console.log('✅ Service authenticated successfully');
      console.log('📤 Sending response to:', msg.reply);

      msg.respond(this.sc.encode(JSON.stringify(response)));

      console.log('✅ Response sent successfully');
    } catch (error) {
      console.error('❌ Error handling service authentication:', error);

      const errorResponse = {
        authenticated: false,
        error: 'Internal server error',
      };

      try {
        msg.respond(this.sc.encode(JSON.stringify(errorResponse)));
      } catch (replyError) {
        console.error('❌ Failed to send error response:', replyError);
      }
    }
  }

  /**
   * Publish a message to a subject (for future use)
   */
  publish(subject: string, data: unknown): void {
    if (!this.nc) {
      throw new Error('NATS not connected');
    }

    const payload = typeof data === 'string' ? data : JSON.stringify(data);
    this.nc.publish(subject, this.sc.encode(payload));
  }

  /**
   * Fire-and-forget core publish for non-durable local events.
   *
   * This is the LOCAL control-plane bus connection (NATS_URL → controlplane-nats),
   * where audit-core's primary core QueueSubscribe listens — NOT the shared
   * verevon-nats bus. Never throws: a missing/closed connection silently no-ops,
   * matching SharedNatsService.publishPlain so callers stay fire-and-forget.
   */
  publishPlain(subject: string, payload: Record<string, unknown>): boolean {
    if (!this.nc || this.nc.isClosed()) {
      return false;
    }
    try {
      this.nc.publish(subject, this.sc.encode(JSON.stringify(payload)));
      return true;
    } catch (error) {
      console.error(`❌ Failed to publish "${subject}" to local NATS:`, error);
      return false;
    }
  }

  /**
   * Persist an audit event to the local control-plane JetStream and wait for
   * the server PubAck. Resolving this promise means the event is stored in the
   * file-backed audit stream and can be retried by audit-core's durable
   * consumer even if either service restarts immediately afterwards.
   *
   * Unlike publishPlain, this method intentionally throws on every unavailable
   * or unacknowledged path so security-sensitive callers can fail closed.
   */
  async publishAuditDurable(
    subject: string,
    payload: Record<string, unknown>,
  ): Promise<{ stream: string; seq: number }> {
    const subjectMatch = AUTH_AUDIT_SUBJECT.exec(subject);
    if (!subjectMatch) {
      throw new Error('Invalid durable audit subject');
    }
    const eventId = payload.event_id;
    const occurredAt = payload.occurred_at;
    const parsedOccurredAt =
      typeof occurredAt === 'string' ? new Date(occurredAt) : null;
    if (
      typeof eventId !== 'string' ||
      !AUDIT_EVENT_ID.test(eventId) ||
      typeof occurredAt !== 'string' ||
      occurredAt.length === 0 ||
      parsedOccurredAt === null ||
      Number.isNaN(parsedOccurredAt.getTime()) ||
      parsedOccurredAt.toISOString() !== occurredAt ||
      typeof payload.org_id !== 'string' ||
      payload.org_id.trim().length === 0 ||
      payload.plane !== 'control' ||
      payload.producer !== 'auth-core' ||
      payload.event !== subjectMatch[1]
    ) {
      throw new Error('Invalid durable audit identity');
    }
    if (!this.nc || this.nc.isClosed() || !this.jetStream) {
      throw new Error('Durable audit transport unavailable');
    }

    const ack = await this.jetStream.publish(
      subject,
      this.sc.encode(JSON.stringify(payload)),
      { msgID: eventId },
    );
    if (!ack || !ack.stream || !Number.isSafeInteger(ack.seq) || ack.seq <= 0) {
      throw new Error('Invalid durable audit PubAck');
    }
    return { stream: ack.stream, seq: ack.seq };
  }

  /**
   * Request-reply pattern (for future use)
   */
  async request(
    subject: string,
    data: unknown,
    timeout = 5000,
  ): Promise<unknown> {
    if (!this.nc) {
      throw new Error('NATS not connected');
    }

    const payload = typeof data === 'string' ? data : JSON.stringify(data);
    const response = await this.nc.request(subject, this.sc.encode(payload), {
      timeout,
    });

    const decoded: unknown = JSON.parse(this.sc.decode(response.data));
    return decoded;
  }
}
