/**
 * voice-realtime-client.ts — browser realtime voice controller (chat-parity
 * Phase 3 voice). Wires the *tested* layers — session-mint (`createVoiceSession`),
 * audio DSP (`voice-audio`), and the WS protocol (`voice-protocol`) — to
 * `getUserMedia` + `AudioContext` + a `WebSocket`.
 *
 * The protocol + DSP are unit-tested; this controller is the browser glue and
 * degrades gracefully (returns `null`) when the environment lacks the APIs.
 * The ONE line that needs live confirmation is the WS auth handshake (see
 * `openSocket`) — the minting service determines whether the ephemeral
 * credential rides in `websocketUrl` or a subprotocol; verified on-stack.
 */

import { floatTo16BitPCM, pcm16ToFloat32 } from "./voice-audio";
import { buildAudioAppend, buildSessionUpdate, parseServerEvent } from "./voice-protocol";
import { createVoiceSession, type RealtimeVoiceSession, type VoiceSessionOptions } from "./voice-session";

const SAMPLE_RATE = 24_000; // realtime PCM16 rate

export type RealtimeVoiceCallbacks = {
  onTranscript?: (text: string) => void;
  onText?: (text: string) => void;
  onError?: (message: string) => void;
  onClose?: () => void;
};

export type RealtimeVoiceHandle = { stop: () => void };

function isSupported(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof WebSocket !== "undefined" &&
    typeof AudioContext !== "undefined" &&
    typeof navigator !== "undefined" &&
    Boolean(navigator.mediaDevices?.getUserMedia)
  );
}

/**
 * Open the realtime WS. The ephemeral `clientSecret` is passed as a subprotocol
 * (a common browser pattern, since browsers can't set WS headers); if the
 * minting service instead embeds the credential in `websocketUrl`, the extra
 * subprotocol is harmless. This is the single handshake detail to confirm live.
 */
function openSocket(session: RealtimeVoiceSession): WebSocket {
  return session.clientSecret
    ? new WebSocket(session.websocketUrl, ["realtime", `bearer.${session.clientSecret}`])
    : new WebSocket(session.websocketUrl);
}

/**
 * Start a realtime voice session: mic → PCM16 → WS, and server audio → playback.
 * Returns a handle to stop, or `null` when unsupported / mint failed.
 */
export async function startRealtimeVoice(
  opts: VoiceSessionOptions & RealtimeVoiceCallbacks = {},
): Promise<RealtimeVoiceHandle | null> {
  if (!isSupported()) {
    return null;
  }
  const session = await createVoiceSession({
    voice: opts.voice,
    model: opts.model,
    instructions: opts.instructions,
  });
  if (!session) {
    return null;
  }

  let closed = false;
  const ws = openSocket(session);
  const captureCtx = new AudioContext({ sampleRate: SAMPLE_RATE });
  const playbackCtx = new AudioContext({ sampleRate: SAMPLE_RATE });
  let micStream: MediaStream | null = null;
  let processor: ScriptProcessorNode | null = null;
  let playHead = 0;

  const cleanup = () => {
    if (closed) {
      return;
    }
    closed = true;
    try {
      processor?.disconnect();
    } catch {
      /* already torn down */
    }
    micStream?.getTracks().forEach((t) => t.stop());
    try {
      ws.close();
    } catch {
      /* already closing */
    }
    void captureCtx.close().catch(() => undefined);
    void playbackCtx.close().catch(() => undefined);
    opts.onClose?.();
  };

  ws.onopen = () => {
    ws.send(JSON.stringify(buildSessionUpdate({ voice: opts.voice, instructions: opts.instructions })));
    navigator.mediaDevices
      .getUserMedia({ audio: true })
      .then((stream) => {
        if (closed) {
          stream.getTracks().forEach((t) => t.stop());
          return;
        }
        micStream = stream;
        const source = captureCtx.createMediaStreamSource(stream);
        const node = captureCtx.createScriptProcessor(4096, 1, 1);
        node.onaudioprocess = (e) => {
          if (closed || ws.readyState !== WebSocket.OPEN) {
            return;
          }
          const pcm = floatTo16BitPCM(e.inputBuffer.getChannelData(0));
          ws.send(JSON.stringify(buildAudioAppend(pcm)));
        };
        source.connect(node);
        node.connect(captureCtx.destination);
        processor = node;
      })
      .catch(() => {
        opts.onError?.("microphone access denied");
        cleanup();
      });
  };

  ws.onmessage = (e) => {
    const ev = parseServerEvent(typeof e.data === "string" ? e.data : "");
    switch (ev.kind) {
      case "audio": {
        const f32 = pcm16ToFloat32(ev.pcm);
        if (f32.length === 0) {
          return;
        }
        const buffer = playbackCtx.createBuffer(1, f32.length, SAMPLE_RATE);
        buffer.getChannelData(0).set(f32);
        const src = playbackCtx.createBufferSource();
        src.buffer = buffer;
        src.connect(playbackCtx.destination);
        // Schedule contiguously so deltas play gap-free.
        const startAt = Math.max(playbackCtx.currentTime, playHead);
        src.start(startAt);
        playHead = startAt + buffer.duration;
        break;
      }
      case "transcript":
        opts.onTranscript?.(ev.text);
        break;
      case "text":
        opts.onText?.(ev.text);
        break;
      case "error":
        opts.onError?.(ev.message);
        break;
      default:
        break;
    }
  };

  ws.onerror = () => opts.onError?.("voice connection error");
  ws.onclose = () => cleanup();

  return { stop: cleanup };
}
