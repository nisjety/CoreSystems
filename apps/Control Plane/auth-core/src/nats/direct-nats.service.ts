import { Injectable, OnModuleInit, OnModuleDestroy } from '@nestjs/common';
import {
  connect,
  NatsConnection,
  StringCodec,
  Msg,
  type ConnectionOptions,
  type JetStreamClient,
} from 'nats';

type ServiceAuthenticationRequest = {
  serviceId: string;
  serviceSecret: string;
};

function isServiceAuthenticationRequest(
  value: unknown,
): value is ServiceAuthenticationRequest {
  return (
    typeof value === 'object' &&
    value !== null &&
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

  async onModuleInit() {
    try {
      console.log('🔌 Connecting to NATS for direct request-reply...');

      const natsToken = process.env.NATS_TOKEN || process.env.NATS_AUTH_TOKEN;
      const natsUser = process.env.NATS_USER;
      const natsPass = process.env.NATS_PASS;

      const connectionOptions: ConnectionOptions = {
        servers: [process.env.NATS_URL || 'nats://nats:4222'],
        maxReconnectAttempts: -1,
        reconnectTimeWait: 2000,
        name: 'auth-service-direct',
      };

      // Token auth takes precedence (for production with aquatiq root container)
      if (natsToken) {
        connectionOptions.token = natsToken;
        console.log('🔐 Using NATS token authentication');
      } else if (natsUser && natsPass) {
        connectionOptions.user = natsUser;
        connectionOptions.pass = natsPass;
        console.log('🔐 Using NATS user/password authentication');
      }

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

      // Validate service credentials
      const validServiceIds = (
        process.env.INTERNAL_SERVICE_IDS || 'admin-service,user-service'
      ).split(',');
      const expectedSecret =
        process.env.INTERNAL_SERVICE_SECRET || process.env.INTERNAL_API_KEY;

      if (!expectedSecret) {
        console.error('❌ INTERNAL_SERVICE_SECRET not configured');
        const errorResponse = {
          authenticated: false,
          error: 'Service authentication not configured',
        };
        msg.respond(this.sc.encode(JSON.stringify(errorResponse)));
        return;
      }

      if (!validServiceIds.includes(data.serviceId)) {
        console.log('❌ Invalid service ID:', data.serviceId);
        const errorResponse = {
          authenticated: false,
          error: 'Invalid service ID',
        };
        msg.respond(this.sc.encode(JSON.stringify(errorResponse)));
        return;
      }

      if (data.serviceSecret !== expectedSecret) {
        console.log('❌ Invalid service secret');
        const errorResponse = {
          authenticated: false,
          error: 'Invalid service secret',
        };
        msg.respond(this.sc.encode(JSON.stringify(errorResponse)));
        return;
      }

      // Success - send service secret for HTTP headers
      const response = {
        authenticated: true,
        serviceSecret: expectedSecret,
        serviceId: data.serviceId,
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
   * Fire-and-forget core publish for audit events (velion.audit.v1.control.*).
   *
   * This is the LOCAL control-plane bus connection (NATS_URL → controlplane-nats),
   * where audit-core's primary core QueueSubscribe listens — NOT the shared
   * velion-nats bus. Never throws: a missing/closed connection silently no-ops,
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
    if (!subject.startsWith('velion.audit.v1.')) {
      throw new Error('Invalid durable audit subject');
    }
    if (!this.nc || this.nc.isClosed() || !this.jetStream) {
      throw new Error('Durable audit transport unavailable');
    }

    const ack = await this.jetStream.publish(
      subject,
      this.sc.encode(JSON.stringify(payload)),
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
