import { createHmac, randomUUID, timingSafeEqual } from 'node:crypto';

import { AppConfig } from '../../common/config/app-config';
import { HttpError } from '../../common/http/http-error';
import { IntegrationEventPublisher } from '../../common/nats/event-publisher';
import { supportSubjects } from '../../common/nats/subjects';

// ---------------------------------------------------------------------------
// Zammad webhook payload types
// ---------------------------------------------------------------------------

interface ZammadUser {
  id: number;
  email?: string;
  login?: string;
  firstname?: string;
  lastname?: string;
}

interface ZammadState {
  name: string;
}

interface ZammadPriority {
  name: string;
}

interface ZammadGroup {
  name: string;
}

interface ZammadTicket {
  id: number;
  number?: string;
  title?: string;
  state?: ZammadState;
  priority?: ZammadPriority;
  group?: ZammadGroup;
  owner?: ZammadUser | null;
  customer?: ZammadUser;
  organization_id?: number | null;
  tags?: string[];
  created_at?: string;
  updated_at?: string;
  // Custom AI fields set by the triage worker after classification.
  ai_category?: string;
  ai_confidence?: number;
  ai_recommended_team?: string;
}

interface ZammadArticle {
  id: number;
  type?: string;
  internal?: boolean;
  body?: string;
  sender?: string;
  from?: string;
  created_at?: string;
}

interface ZammadWebhookPayload {
  ticket?: ZammadTicket;
  article?: ZammadArticle;
  [key: string]: unknown;
}

// ---------------------------------------------------------------------------
// Result
// ---------------------------------------------------------------------------

export interface ZammadWebhookResult {
  accepted: boolean;
  subject: string;
  eventId: string;
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

export class ZammadWebhookService {
  constructor(
    private readonly config: AppConfig,
    private readonly eventPublisher: IntegrationEventPublisher
  ) {}

  async handle(
    rawBody: string,
    headers: Record<string, string | string[] | undefined>
  ): Promise<ZammadWebhookResult> {
    this.authenticate(rawBody, headers);

    let payload: ZammadWebhookPayload;
    try {
      payload = JSON.parse(rawBody) as ZammadWebhookPayload;
    } catch {
      throw new HttpError(400, 'invalid_json', 'Webhook payload must be valid JSON');
    }

    if (!payload.ticket) {
      throw new HttpError(422, 'missing_ticket', 'Zammad webhook payload must include a ticket object');
    }

    const eventId = randomUUID();
    const subject = this.resolveSubject(payload, headers);
    const normalized = this.normalize(payload, eventId);

    await this.publishBestEffort(subject, normalized);

    return { accepted: true, subject, eventId };
  }

  // ---------------------------------------------------------------------------
  // Private helpers
  // ---------------------------------------------------------------------------

  /**
   * Verifies the Zammad webhook signature.
   * Zammad sends `X-Hub-Signature: sha1=<hex>` computed as HMAC-SHA1 of the
   * raw request body using the webhook token set in the Zammad admin UI.
   * Skips verification when ZAMMAD_WEBHOOK_TOKEN is not configured (dev mode).
   */
  private authenticate(
    rawBody: string,
    headers: Record<string, string | string[] | undefined>
  ): void {
    const secret = this.config.zammadWebhookToken;
    if (!secret) {
      return; // Token not configured — allow all (dev mode)
    }

    const sigHeader = headers['x-hub-signature'];
    const sigRaw = Array.isArray(sigHeader) ? sigHeader[0] : sigHeader;

    if (typeof sigRaw !== 'string' || !sigRaw.startsWith('sha1=')) {
      throw new HttpError(401, 'invalid_zammad_webhook_signature', 'Missing X-Hub-Signature header');
    }

    const received = Buffer.from(sigRaw.slice('sha1='.length), 'hex');
    const expected = Buffer.from(
      createHmac('sha1', secret).update(rawBody).digest('hex'),
      'hex',
    );

    if (received.length !== expected.length || !timingSafeEqual(received, expected)) {
      throw new HttpError(401, 'invalid_zammad_webhook_signature', 'X-Hub-Signature mismatch');
    }
  }

  /**
   * Derives the NATS subject from the `X-Zammad-Event` header Zammad sends
   * (configured in the trigger), falling back to payload inspection.
   */
  private resolveSubject(
    payload: ZammadWebhookPayload,
    headers: Record<string, string | string[] | undefined>
  ): string {
    const eventHeader = headers['x-zammad-event'];
    const event = Array.isArray(eventHeader) ? eventHeader[0] : eventHeader;

    if (typeof event === 'string') {
      switch (event.toLowerCase()) {
        case 'ticket.created':  return supportSubjects.ticketCreated;
        case 'ticket.updated':  return supportSubjects.ticketUpdated;
        case 'ticket.assigned': return supportSubjects.ticketAssigned;
        case 'article.created': return supportSubjects.articleAdded;
        case 'sla.breach':      return supportSubjects.slaBreach;
      }
    }

    // Fallback: inspect payload structure
    if (payload.article) return supportSubjects.articleAdded;

    // Distinguish created vs updated by comparing created_at and updated_at
    const ticket = payload.ticket!;
    if (ticket.created_at && ticket.updated_at && ticket.created_at === ticket.updated_at) {
      return supportSubjects.ticketCreated;
    }

    return supportSubjects.ticketUpdated;
  }

  private normalize(
    payload: ZammadWebhookPayload,
    eventId: string
  ): Record<string, unknown> {
    const ticket = payload.ticket!;

    return {
      eventId,
      ticketId: ticket.id,
      ticketNumber: ticket.number ?? null,
      title: ticket.title ?? null,
      state: ticket.state?.name ?? null,
      priority: ticket.priority?.name ?? null,
      group: ticket.group?.name ?? null,
      ownerId: ticket.owner?.id ?? null,
      ownerEmail: ticket.owner?.email ?? null,
      customerId: ticket.customer?.id ?? null,
      customerEmail: ticket.customer?.email ?? null,
      organizationId: ticket.organization_id ?? null,
      tags: ticket.tags ?? [],
      aiCategory: ticket.ai_category ?? null,
      aiConfidence: ticket.ai_confidence ?? null,
      aiRecommendedTeam: ticket.ai_recommended_team ?? null,
      article: payload.article
        ? {
            id: payload.article.id,
            type: payload.article.type ?? null,
            internal: payload.article.internal ?? false,
            sender: payload.article.sender ?? null,
          }
        : null,
      createdAt: ticket.created_at ?? null,
      updatedAt: ticket.updated_at ?? null,
    };
  }

  private async publishBestEffort(
    subject: string,
    payload: Record<string, unknown>
  ): Promise<void> {
    try {
      // VelionSubject is a string union — cast is safe because resolveSubject
      // always returns one of the defined supportSubjects values.
      await this.eventPublisher.publish(subject as Parameters<IntegrationEventPublisher['publish']>[0], payload);
    } catch (error) {
      console.error('[zammad-webhook] Failed to publish event', { subject, error });
    }
  }
}
