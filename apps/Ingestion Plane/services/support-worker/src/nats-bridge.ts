import {
  connect,
  AckPolicy,
  DeliverPolicy,
  type NatsConnection,
  type JetStreamClient,
  type JetStreamManager,
  type ConsumerMessages,
  StringCodec,
} from 'nats';
import { Client as TemporalClient } from '@temporalio/client';
import { config } from './config';
import { triageTicket, type TicketPayload } from './workflows/triage';
import { slaCountdown } from './workflows/sla';
import { notifyAgentActivity } from './activities/notify-agent';

const TASK_QUEUE = 'support-task-queue';
const STREAM = 'VELION_SUPPORT';
const CONSUMER = 'support-worker';
const sc = StringCodec();

// ─── Zammad webhook event shapes ────────────────────────────────────────────

interface ZammadTicketEvent {
  ticket: {
    id: number;
    title: string;
    owner_id?: number;
    group_id?: number;
    customer?: { email?: string };
    sla_deadline?: string; // ISO-8601
  };
  article?: {
    body: string;
    internal?: boolean;
  };
}

// ─── SLA deadline helper ─────────────────────────────────────────────────────

function slaDeadlineMs(event: ZammadTicketEvent): number | null {
  const raw = event.ticket.sla_deadline;
  if (!raw) return null;
  const ms = new Date(raw).getTime();
  return Number.isFinite(ms) ? ms : null;
}

// ─── Consumer bootstrap ───────────────────────────────────────────────────────

/**
 * Creates the durable pull consumer on VELION_SUPPORT if it doesn't exist.
 * Idempotent — safe to call on every startup.
 */
async function ensureConsumer(jsm: JetStreamManager): Promise<void> {
  try {
    await jsm.consumers.info(STREAM, CONSUMER);
    // Consumer already exists — nothing to do.
  } catch {
    await jsm.consumers.add(STREAM, {
      name: CONSUMER,
      durable_name: CONSUMER,
      filter_subject: 'velion.support.>',
      ack_policy: AckPolicy.Explicit,
      deliver_policy: DeliverPolicy.New,
    });
    console.log(`[nats-bridge] Created durable consumer "${CONSUMER}" on stream "${STREAM}".`);
  }
}

// ─── Main bridge ─────────────────────────────────────────────────────────────

/**
 * Connect to NATS with bounded exponential backoff. The `velion-nats`
 * container lives in the Frontend Plane Velion compose stack, which boots
 * after the Ingestion Plane stack — so `getaddrinfo ENOTFOUND velion-nats`
 * is expected during cold-boot of the full system. This retry loop keeps
 * the worker alive (Temporal connection stays healthy) until NATS comes up,
 * instead of crash-looping the whole process.
 */
async function connectWithRetry(): Promise<NatsConnection> {
  const baseDelayMs = 2_000;
  const maxDelayMs = 30_000;
  let attempt = 0;
  // eslint-disable-next-line no-constant-condition
  while (true) {
    try {
      const connectOpts = config.VELION_NATS_TOKEN
        ? {
            servers: config.VELION_NATS_URL,
            token: config.VELION_NATS_TOKEN,
            reconnect: true,
            maxReconnectAttempts: -1,
            reconnectTimeWait: 5_000,
          }
        : {
            servers: config.VELION_NATS_URL,
            reconnect: true,
            maxReconnectAttempts: -1,
            reconnectTimeWait: 5_000,
          };
      return await connect(connectOpts);
    } catch (err: unknown) {
      attempt += 1;
      const delay = Math.min(baseDelayMs * 2 ** Math.min(attempt - 1, 4), maxDelayMs);
      const reason = err instanceof Error ? err.message : 'unknown error';
      console.warn(
        `[nats-bridge] NATS connect attempt ${attempt} failed (${reason}); retrying in ${delay}ms…`,
      );
      await new Promise((resolve) => setTimeout(resolve, delay));
    }
  }
}

export async function startNatsBridge(
  temporalClient: TemporalClient,
): Promise<() => Promise<void>> {
  const nc: NatsConnection = await connectWithRetry();
  console.log('[nats-bridge] Connected to', config.VELION_NATS_URL);

  const jsm: JetStreamManager = await nc.jetstreamManager();
  const js: JetStreamClient = nc.jetstream();

  await ensureConsumer(jsm);

  const consumer = await js.consumers.get(STREAM, CONSUMER);
  const messages: ConsumerMessages = await consumer.consume();

  // Process messages in the background.
  void (async () => {
    for await (const msg of messages) {
      try {
        const subject = msg.subject;
        const raw = sc.decode(msg.data);
        const event = JSON.parse(raw) as ZammadTicketEvent;

        await handleMessage(subject, event, temporalClient);
        msg.ack();
      } catch (err) {
        console.error('[nats-bridge] Failed to handle message:', err);
        msg.nak();
      }
    }
  })();

  // Return a teardown function for graceful shutdown.
  return async () => {
    messages.stop();
    await nc.drain();
    console.log('[nats-bridge] Drained and disconnected.');
  };
}

// ─── Per-subject routing ─────────────────────────────────────────────────────

async function handleMessage(
  subject: string,
  event: ZammadTicketEvent,
  client: TemporalClient,
): Promise<void> {
  const ticketId = event.ticket.id;

  if (subject === 'velion.support.ticket.created') {
    await handleTicketCreated(ticketId, event, client);
    return;
  }

  if (subject === 'velion.support.article.added') {
    await handleArticleAdded(ticketId, event, client);
    return;
  }

  if (subject === 'velion.support.ticket.assigned') {
    await handleTicketAssigned(ticketId, event);
    return;
  }

  // velion.support.ticket.updated and velion.support.sla.breach handled
  // downstream — no extra routing needed here.
}

async function handleTicketCreated(
  ticketId: number,
  event: ZammadTicketEvent,
  client: TemporalClient,
): Promise<void> {
  const ticketPayload: TicketPayload = {
    ticketId,
    title: event.ticket.title,
    body: event.article?.body ?? '',
    ownerId: event.ticket.owner_id,
    groupId: event.ticket.group_id,
  };

  // Start triage workflow (idempotent — same workflowId is safe to re-send).
  await client.workflow.start(triageTicket, {
    taskQueue: TASK_QUEUE,
    workflowId: `triage-${ticketId}`,
    args: [ticketPayload],
  });

  // Start SLA countdown if a deadline is set.
  const deadline = slaDeadlineMs(event);
  if (deadline !== null) {
    await client.workflow.start(slaCountdown, {
      taskQueue: TASK_QUEUE,
      workflowId: `sla-${ticketId}`,
      args: [ticketId, deadline],
    });
  }
}

async function handleArticleAdded(
  ticketId: number,
  event: ZammadTicketEvent,
  client: TemporalClient,
): Promise<void> {
  // Only external (customer) replies should reset the SLA timer.
  if (event.article?.internal === true) return;

  const deadline = slaDeadlineMs(event);
  if (deadline === null) return;

  // Terminate the previous countdown (if still running) and start a fresh one.
  try {
    const handle = client.workflow.getHandle(`sla-${ticketId}`);
    await handle.terminate('Customer replied — SLA timer reset.');
  } catch {
    // Workflow may have already finished — not an error.
  }

  await client.workflow.start(slaCountdown, {
    taskQueue: TASK_QUEUE,
    workflowId: `sla-${ticketId}-${Date.now()}`,
    args: [ticketId, deadline],
  });
}

async function handleTicketAssigned(
  ticketId: number,
  event: ZammadTicketEvent,
): Promise<void> {
  // Fire a direct notification — no workflow overhead needed.
  await notifyAgentActivity({
    type: 'ticket.assigned',
    ticketId,
    recipientId: event.ticket.owner_id ?? 'broadcast',
    payload: {
      ticketId,
      message: `Ticket #${ticketId} has been assigned to you.`,
    },
  });
}
