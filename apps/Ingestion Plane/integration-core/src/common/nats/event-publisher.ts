import { JSONCodec, NatsConnection, connect, JetStreamClient, JetStreamManager } from 'nats';

import { AppConfig } from '../config/app-config';
import { VelionSubject } from './subjects';

export interface IntegrationEventPublisher {
  publish(subject: VelionSubject, payload: Record<string, unknown>): Promise<void>;
  close(): Promise<void>;
}

export class NatsIntegrationEventPublisher implements IntegrationEventPublisher {
  private readonly codec = JSONCodec<Record<string, unknown>>();
  private js: JetStreamClient;

  constructor(private readonly connection: NatsConnection, js: JetStreamClient) {
    this.js = js;
  }

  async publish(subject: VelionSubject, payload: Record<string, unknown>): Promise<void> {
    try {
      await this.js.publish(subject, this.codec.encode(payload));
    } catch {
      // Fire-and-forget: log but don't block caller
      console.warn(`[integration-core] Failed to publish ${subject}`);
    }
  }

  async close(): Promise<void> {
    await this.connection.drain();
  }
}

async function ensureStream(
  jsm: JetStreamManager,
  name: string,
  subjects: string[]
): Promise<void> {
  try {
    await jsm.streams.add({
      name,
      subjects,
      max_age: 14 * 24 * 60 * 60 * 1_000_000_000, // 14 days in nanoseconds
      max_msgs: 100_000
    });
  } catch {
    // Stream already exists — safe to ignore
  }
}

export async function createNatsEventPublisher(config: AppConfig): Promise<IntegrationEventPublisher> {
  const connection = await connect({
    name: config.serviceName,
    servers: [config.velionNatsUrl],
    token: config.velionNatsToken
  });

  const jsm: JetStreamManager = await connection.jetstreamManager();
  const js: JetStreamClient = connection.jetstream();

  await ensureStream(jsm, 'VELION_INGESTION', ['velion.ingestion.>']);
  await ensureStream(jsm, 'VELION_SUPPORT',   ['velion.support.>']);

  return new NatsIntegrationEventPublisher(connection, js);
}