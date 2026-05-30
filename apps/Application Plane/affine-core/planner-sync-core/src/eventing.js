import { connect, JSONCodec } from 'nats';

const VELION_NATS_URL =
  process.env.VELION_NATS_URL || process.env.NATS_SHARED_URL || 'nats://velion-nats:4222';
const VELION_NATS_TOKEN =
  process.env.VELION_NATS_TOKEN || process.env.NATS_SHARED_TOKEN || process.env.NATS_TOKEN || '';
const codec = JSONCodec();

let natsConnectionPromise = null;

async function getNatsConnection() {
  if (!VELION_NATS_URL) {
    return null;
  }

  if (!natsConnectionPromise) {
    natsConnectionPromise = connect({
      servers: VELION_NATS_URL,
      ...(VELION_NATS_TOKEN ? { token: VELION_NATS_TOKEN } : {}),
    }).catch((error) => {
      natsConnectionPromise = null;
      throw error;
    });
  }

  return natsConnectionPromise;
}

export async function publishPlannerEvent(subject, payload) {
  try {
    const connection = await getNatsConnection();
    if (!connection) {
      return false;
    }

    connection.publish(subject, codec.encode(payload));
    return true;
  } catch (error) {
    console.warn(`[planner-sync-core] failed to publish ${subject}`, error);
    return false;
  }
}

export async function closeEventing() {
  if (!natsConnectionPromise) {
    return;
  }

  try {
    const connection = await natsConnectionPromise;
    await connection.drain();
  } catch {
    // Ignore shutdown errors.
  } finally {
    natsConnectionPromise = null;
  }
}
