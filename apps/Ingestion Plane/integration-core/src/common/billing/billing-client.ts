/**
 * Publishes usage/metering events to NATS for billing reconciliation.
 *
 * Uses the shared NATS connection already configured for integration events.
 * Fire-and-forget — failures are logged but never block the caller.
 */
import { randomUUID } from 'node:crypto';

import { IntegrationEventPublisher } from '../nats/event-publisher';

// ─── Types ────────────────────────────────────────────────────────────────────

export interface UsageEvent {
  eventId: string;
  orgId: string;
  metric: string;
  quantity: number;
  source: string;
  occurredAt: string;
  metadata?: Record<string, unknown>;
}

export interface BillingClient {
  recordUsage(
    orgId: string,
    metric: string,
    quantity: number,
    metadata?: Record<string, unknown>
  ): void;
}

// NATS subject for billing usage events (consumed by billing-core).
const BILLING_USAGE_SUBJECT = 'velion.billing.usage.recorded' as const;

// ─── Implementation ───────────────────────────────────────────────────────────

export class NatsBillingClient implements BillingClient {
  constructor(
    private readonly publisher: IntegrationEventPublisher,
    private readonly source: string = 'integration-core'
  ) {}

  /**
   * Publishes a usage event to NATS for billing-core consumption.
   * Non-blocking — catches and logs any errors internally.
   */
  recordUsage(
    orgId: string,
    metric: string,
    quantity: number,
    metadata?: Record<string, unknown>
  ): void {
    if (!orgId || !metric || quantity <= 0) {
      return;
    }

    const event: UsageEvent = {
      eventId: randomUUID(),
      orgId,
      metric,
      quantity,
      source: this.source,
      occurredAt: new Date().toISOString(),
      metadata
    };

    // Fire-and-forget; catch to prevent unhandled rejections.
    this.publisher
      .publish(BILLING_USAGE_SUBJECT as never, event as unknown as Record<string, unknown>)
      .catch((error: unknown) => {
        console.error(
          `[billing-client] Failed to publish usage event: metric=${metric} org=${orgId}`,
          error instanceof Error ? error.message : error
        );
      });
  }
}
