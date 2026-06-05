/**
 * voice-protocol.ts — realtime voice WS event builders/parsers (chat-parity
 * Phase 3 voice). Implements the documented OpenAI Realtime WS event shapes
 * (the dominant realtime API): client sends `session.update` +
 * `input_audio_buffer.append` (base64 PCM16); server streams
 * `response.audio.delta` (base64 PCM16) + transcript/text deltas.
 *
 * Pure functions — unit-tested without a socket. The controller that owns the
 * WebSocket + AudioContext uses these; the live handshake verifies on-stack.
 */

import { int16ToBase64, base64ToInt16 } from "./voice-audio";

export type VoiceSessionConfig = {
  voice?: string;
  instructions?: string;
};

/** `session.update` — configure modalities, audio formats, voice, VAD. */
export function buildSessionUpdate(config: VoiceSessionConfig = {}): Record<string, unknown> {
  return {
    type: "session.update",
    session: {
      modalities: ["audio", "text"],
      instructions: config.instructions ?? "",
      voice: config.voice ?? "alloy",
      input_audio_format: "pcm16",
      output_audio_format: "pcm16",
      turn_detection: { type: "server_vad" },
    },
  };
}

/** `input_audio_buffer.append` — stream a captured mic frame (base64 PCM16). */
export function buildAudioAppend(frame: Int16Array): Record<string, unknown> {
  return { type: "input_audio_buffer.append", audio: int16ToBase64(frame) };
}

/** `input_audio_buffer.commit` — end the current input turn. */
export function buildAudioCommit(): Record<string, unknown> {
  return { type: "input_audio_buffer.commit" };
}

/** Parsed server event, normalized to what the controller acts on. */
export type ServerVoiceEvent =
  | { kind: "audio"; pcm: Int16Array }
  | { kind: "transcript"; text: string }
  | { kind: "text"; text: string }
  | { kind: "done" }
  | { kind: "error"; message: string }
  | { kind: "other"; type: string };

/** Parse a raw server WS message into a normalized [`ServerVoiceEvent`]. */
export function parseServerEvent(raw: string): ServerVoiceEvent {
  let msg: Record<string, unknown>;
  try {
    msg = JSON.parse(raw) as Record<string, unknown>;
  } catch {
    return { kind: "error", message: "malformed server event" };
  }
  const type = typeof msg["type"] === "string" ? (msg["type"] as string) : "";
  switch (type) {
    case "response.audio.delta": {
      const delta = typeof msg["delta"] === "string" ? (msg["delta"] as string) : "";
      return { kind: "audio", pcm: base64ToInt16(delta) };
    }
    case "response.audio_transcript.delta":
      return { kind: "transcript", text: asDelta(msg) };
    case "response.text.delta":
      return { kind: "text", text: asDelta(msg) };
    case "response.done":
      return { kind: "done" };
    case "error": {
      const err = msg["error"];
      const message =
        err && typeof err === "object" && typeof (err as { message?: unknown }).message === "string"
          ? ((err as { message: string }).message)
          : "realtime error";
      return { kind: "error", message };
    }
    default:
      return { kind: "other", type };
  }
}

function asDelta(msg: Record<string, unknown>): string {
  return typeof msg["delta"] === "string" ? (msg["delta"] as string) : "";
}
