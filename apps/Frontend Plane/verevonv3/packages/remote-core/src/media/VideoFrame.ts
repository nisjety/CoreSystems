import type { RemoteVideoFrame } from '../types/public.js';

/**
 * `VideoFrame` og `ImageBitmap` holder på GPU-/dekoder-ressurser som må
 * frigjøres eksplisitt — ellers lekker minne selv om referansen til objektet
 * forsvinner. Kalles når en frame er ferdig konsumert (tegnet, sendt til AI,
 * eller forkastet fordi en nyere frame kom før den ble vist).
 */
export function releaseVideoFrame(frame: RemoteVideoFrame): void {
  frame.videoFrame?.close();
  frame.bitmap?.close();
}
