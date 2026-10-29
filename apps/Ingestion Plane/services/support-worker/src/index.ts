import path from 'path';
import { Worker, NativeConnection } from '@temporalio/worker';
import { Client as TemporalClient, Connection } from '@temporalio/client';
import { config } from './config';
import { startNatsBridge } from './nats-bridge';
import * as activities from './activities';

const TASK_QUEUE = 'support-task-queue';

async function main(): Promise<void> {
  console.log('[support-worker] Starting…');

  // ── 1. Temporal connections ────────────────────────────────────────────────
  // Worker uses NativeConnection (Rust bindings); Client uses the gRPC Connection.
  const nativeConnection = await NativeConnection.connect({
    address: config.TEMPORAL_ADDRESS,
  });

  const clientConnection = await Connection.connect({
    address: config.TEMPORAL_ADDRESS,
  });

  const temporalClient = new TemporalClient({
    connection: clientConnection,
    namespace: config.TEMPORAL_NAMESPACE,
  });

  // ── 2. Temporal Worker ─────────────────────────────────────────────────────
  const worker = await Worker.create({
    connection: nativeConnection,
    namespace: config.TEMPORAL_NAMESPACE,
    taskQueue: TASK_QUEUE,
    workflowsPath: path.join(__dirname, 'workflows'),
    activities,
  });

  // Run the worker in the background (returns a promise that resolves on shutdown).
  const workerRunPromise = worker.run();
  console.log(`[support-worker] Temporal worker listening on "${TASK_QUEUE}"`);

  // ── 3. NATS bridge ─────────────────────────────────────────────────────────
  const stopNatsBridge = await startNatsBridge(temporalClient);
  console.log('[support-worker] NATS bridge active.');

  // ── 4. Graceful shutdown ───────────────────────────────────────────────────
  async function shutdown(signal: string): Promise<void> {
    console.log(`[support-worker] Received ${signal}, shutting down…`);

    try {
      await stopNatsBridge();
    } catch (err) {
      console.error('[support-worker] Error stopping NATS bridge:', err);
    }

    worker.shutdown();

    try {
      await workerRunPromise;
    } catch (err) {
      console.error('[support-worker] Worker exited with error:', err);
    }

    await clientConnection.close();
    await nativeConnection.close();
    console.log('[support-worker] Shutdown complete.');
    process.exit(0);
  }

  process.on('SIGTERM', () => void shutdown('SIGTERM'));
  process.on('SIGINT', () => void shutdown('SIGINT'));

  // Block until the worker stops.
  await workerRunPromise;
}

main().catch((err: unknown) => {
  console.error('[support-worker] Fatal error:', err);
  process.exit(1);
});
