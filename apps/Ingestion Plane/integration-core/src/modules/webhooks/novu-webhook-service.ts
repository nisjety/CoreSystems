import { createHmac, randomUUID, timingSafeEqual } from 'node:crypto';

import { AppConfig } from '../../common/config/app-config';
import { HttpError } from '../../common/http/http-error';
import { IntegrationEventPublisher } from '../../common/nats/event-publisher';
import { supportSubjects } from '../../common/nats/subjects';

// ─── Novu webhook payload types ───────────────────────────────────────────

interface NovuSubscriber {
  id: string;
  email?: string;
  firstName?: string;
  lastName?: string;
  phone?: string;
  avatar?: string;
  locale?: string;
  channels?: Array<{
    type: string;
    credentials: Record<string, unknown>;
  }>;
  subscriberCustomData?: Record<string, unknown>;
}

interface NovuNotificationEvent {
  _id: string;
  type: string;
  status: 'sent' | 'failed' | 'read' | 'unseen';
  email?: string;
  subscriberId?: string;
  templateIdentifier?: string;
  notificationIdentifier?: string;
  createdAt?: string;
  updatedAt?: string;
  payload?: Record<string, unknown>;
  channels?: Array<{
    type: string;
    status: string;
  }>;
}

interface NovuWebhookPayload {
  type: string;
  createdAt: string;
  data:
    | {
        subscriber?: NovuSubscriber;
        event?: NovuNotificationEvent;
        [key: string]: unknown;
      }
    | Record<string, unknown>;
}

// ─── Result type ─────────────────────────────────────────────────────────

export interface NovuWebhookResult {
  accepted: boolean;
  eventId: string;
  eventType: string;
}

// ─── Service ──────────────────────────────────────────────────────────────

export class NovuWebhookService {
  constructor(
    private readonly config: AppConfig,
    private readonly eventPublisher: IntegrationEventPublisher,
  ) {}

  async handle(
    rawBody: string,
    headers: Record<string, string | string[] | undefined>,
  ): Promise<NovuWebhookResult> {
    this.authenticate(rawBody, headers);

    let payload: NovuWebhookPayload;
    try {
      payload = JSON.parse(rawBody) as NovuWebhookPayload;
    } catch {
      throw new HttpError(400, 'invalid_json', 'Webhook payload must be valid JSON');
    }

    if (!payload.type) {
      throw new HttpError(422, 'missing_type', 'Novu webhook payload must include a type field');
    }

    const eventId = randomUUID();
    const eventType = payload.type;

    await this.publishBestEffort(eventType, payload, eventId);

    return {
      accepted: true,
      eventId,
      eventType,
    };
  }

  // ─── Private helpers ──────────────────────────────────────────────────

  /**
   * Validates the Novu webhook signature using HMAC-SHA256.
   * Novu sends `x-novu-signature-v1: t=<timestamp>,v1=<hex>` computed as
   * HMAC-SHA256 of `<timestamp>.<raw_body>` using the webhook secret.
   * Skips validation when NOVU_WEBHOOK_SECRET is not configured (dev mode).
   */
  private authenticate(
    rawBody: string,
    headers: Record<string, string | string[] | undefined>,
  ): void {
    const secret = this.config.novuWebhookSecret;
    if (!secret) {
      return; // Token not configured — allow all (dev mode)
    }

    const sigHeader = headers['x-novu-signature-v1'];
    const sigRaw = Array.isArray(sigHeader) ? sigHeader[0] : sigHeader;

    if (typeof sigRaw !== 'string') {
      throw new HttpError(401, 'invalid_novu_webhook_signature', 'Missing x-novu-signature-v1 header');
    }

    // Parse signature: "t=<timestamp>,v1=<hex>"
    const parts = sigRaw.split(',').reduce(
      (acc, part) => {
        const [key, value] = part.split('=');
        acc[key.trim()] = value?.trim();
        return acc;
      },
      {} as Record<string, string | undefined>,
    );

    const timestamp = parts.t;
    const received = parts.v1;

    if (!timestamp || !received) {
      throw new HttpError(401, 'invalid_novu_webhook_signature', 'Malformed x-novu-signature-v1 header');
    }

    const toSign = `${timestamp}.${rawBody}`;
    const expected = createHmac('sha256', secret).update(toSign).digest('hex');
    const receivedBuf = Buffer.from(received, 'hex');
    const expectedBuf = Buffer.from(expected, 'hex');

    if (receivedBuf.length !== expectedBuf.length || !timingSafeEqual(receivedBuf, expectedBuf)) {
      throw new HttpError(401, 'invalid_novu_webhook_signature', 'x-novu-signature-v1 mismatch');
    }
  }

  private async publishBestEffort(
    eventType: string,
    payload: NovuWebhookPayload,
    eventId: string,
  ): Promise<void> {
    try {
      const eventData = {
        novuEventId: eventId,
        eventType,
        timestamp: payload.createdAt,
        payload,
      };

      // Publish to generic notification subject for cross-service consumption.
      await this.eventPublisher.publish(
        'velion.notifications.novu.event',
        eventData,
      );

      // Optionally publish to specific event type subjects for routing.
      switch (eventType.toLowerCase()) {
        case 'notification.sent':
          await this.eventPublisher.publish('velion.notifications.novu.sent', eventData);
          break;
        case 'notification.failed':
          await this.eventPublisher.publish('velion.notifications.novu.failed', eventData);
          break;
        case 'notification.read':
          await this.eventPublisher.publish('velion.notifications.novu.read', eventData);
          break;
        case 'subscriber.created':
          await this.eventPublisher.publish('velion.notifications.novu.subscriber_created', eventData);
          break;
        case 'subscriber.updated':
          await this.eventPublisher.publish('velion.notifications.novu.subscriber_updated', eventData);
          break;
      }
    } catch (err) {
      console.error('[novu-webhook] Failed to publish event:', err);
      // Best-effort: don't throw — webhook must succeed even if publish fails.
    }
  }
}
