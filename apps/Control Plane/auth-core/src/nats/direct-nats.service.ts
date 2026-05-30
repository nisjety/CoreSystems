import { Injectable, OnModuleInit, OnModuleDestroy } from '@nestjs/common';
import { connect, NatsConnection, StringCodec, Msg } from 'nats';

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
  private sc = StringCodec();

  async onModuleInit() {
    try {
      console.log('🔌 Connecting to NATS for direct request-reply...');

      const natsToken = process.env.NATS_TOKEN || process.env.NATS_AUTH_TOKEN;
      const natsUser = process.env.NATS_USER;
      const natsPass = process.env.NATS_PASS;

      const connectionOptions: any = {
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

      console.log('✅ Direct NATS connection established');

      // Set up service authentication handler
      await this.setupServiceAuthentication();
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
  private async setupServiceAuthentication() {
    const sub = this.nc.subscribe('service.authenticate');

    console.log('👂 Listening for service.authenticate requests...');

    // Process messages
    void (async () => {
      for await (const msg of sub) {
        await this.handleServiceAuthentication(msg);
      }
    })();
  }

  /**
   * Handle service authentication request
   */
  private async handleServiceAuthentication(msg: Msg) {
    try {
      const data = JSON.parse(this.sc.decode(msg.data));

      console.log('🔔 NATS: Received service.authenticate request');
      console.log('🔑 Service ID:', data.serviceId);
      console.log('📬 Reply subject:', msg.reply);

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
  async publish(subject: string, data: any): Promise<void> {
    if (!this.nc) {
      throw new Error('NATS not connected');
    }

    const payload = typeof data === 'string' ? data : JSON.stringify(data);
    this.nc.publish(subject, this.sc.encode(payload));
  }

  /**
   * Request-reply pattern (for future use)
   */
  async request(subject: string, data: any, timeout = 5000): Promise<any> {
    if (!this.nc) {
      throw new Error('NATS not connected');
    }

    const payload = typeof data === 'string' ? data : JSON.stringify(data);
    const response = await this.nc.request(subject, this.sc.encode(payload), {
      timeout,
    });

    return JSON.parse(this.sc.decode(response.data));
  }
}
