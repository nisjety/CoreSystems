/**
 * voice-audio.ts — pure audio DSP helpers for the realtime voice client
 * (chat-parity Phase 3 voice).
 *
 * These are the deterministic, provider-agnostic core that any realtime audio
 * transport needs: convert Web Audio Float32 frames to/from 16-bit little-endian
 * PCM and base64 (the wire encoding realtime APIs use for `input_audio_buffer`
 * / audio deltas). Pure functions — fully unit-tested without audio hardware.
 * The transport/media wiring that uses them connects against the live stack.
 */

/** Clamp + quantize Web Audio Float32 samples (−1..1) to 16-bit PCM. */
export function floatTo16BitPCM(input: Float32Array): Int16Array {
  const out = new Int16Array(input.length);
  for (let i = 0; i < input.length; i += 1) {
    const s = Math.max(-1, Math.min(1, input[i]));
    out[i] = s < 0 ? Math.round(s * 0x8000) : Math.round(s * 0x7fff);
  }
  return out;
}

/** Inverse of {@link floatTo16BitPCM}: 16-bit PCM back to Float32 (−1..1). */
export function pcm16ToFloat32(input: Int16Array): Float32Array {
  const out = new Float32Array(input.length);
  for (let i = 0; i < input.length; i += 1) {
    out[i] = input[i] / (input[i] < 0 ? 0x8000 : 0x7fff);
  }
  return out;
}

/** Encode PCM16 samples as base64 of their little-endian bytes (wire frame). */
export function int16ToBase64(input: Int16Array): string {
  const bytes = new Uint8Array(input.length * 2);
  for (let i = 0; i < input.length; i += 1) {
    bytes[i * 2] = input[i] & 0xff;
    bytes[i * 2 + 1] = (input[i] >> 8) & 0xff;
  }
  let binary = "";
  for (let i = 0; i < bytes.length; i += 1) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

/** Decode a base64 PCM16 wire frame back to samples (little-endian, signed). */
export function base64ToInt16(b64: string): Int16Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  const out = new Int16Array(Math.floor(bytes.length / 2));
  for (let i = 0; i < out.length; i += 1) {
    // Combine LE bytes then sign-extend the 16-bit value.
    out[i] = ((bytes[i * 2] | (bytes[i * 2 + 1] << 8)) << 16) >> 16;
  }
  return out;
}
