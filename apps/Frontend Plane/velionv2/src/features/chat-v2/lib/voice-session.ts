/**
 * voice-session.ts — realtime voice session minting (chat-parity Phase 3 voice).
 *
 * This is the *known-contract* half of voice: it mints an ephemeral realtime
 * session via the BFF (`POST /api/voice/session` → Model Plane `/v1/ai/realtime`).
 * The browser then opens the realtime media connection directly using
 * `websocketUrl` + `clientSecret` — that media client (mic capture, audio
 * framing, playback) depends on the provider's live WS/WebRTC handshake and is
 * wired against the running stack, not guessed here.
 */

export type RealtimeVoiceSession = {
  sessionId: string;
  clientSecret: string;
  websocketUrl: string;
  expiresAt: number;
  modelUsed: string;
  voice: string;
};

export type VoiceSessionOptions = {
  model?: string;
  voice?: string;
  instructions?: string;
};

function asString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function asNumber(value: unknown): number {
  return typeof value === "number" ? value : 0;
}

/**
 * Mint an ephemeral realtime voice session. Returns `null` on any failure so
 * the caller can disable the mic affordance gracefully rather than throw.
 */
export async function createVoiceSession(
  opts: VoiceSessionOptions = {},
): Promise<RealtimeVoiceSession | null> {
  try {
    const res = await fetch("/api/voice/session", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      credentials: "include",
      body: JSON.stringify({
        model: opts.model,
        voice: opts.voice,
        instructions: opts.instructions,
      }),
    });
    if (!res.ok) {
      return null;
    }
    const data = (await res.json()) as Record<string, unknown>;
    const clientSecret = asString(data["client_secret"]);
    const websocketUrl = asString(data["websocket_url"]);
    // A usable session needs both the ephemeral credential and an endpoint.
    if (!clientSecret || !websocketUrl) {
      return null;
    }
    return {
      sessionId: asString(data["session_id"]),
      clientSecret,
      websocketUrl,
      expiresAt: asNumber(data["expires_at"]),
      modelUsed: asString(data["model_used"]),
      voice: asString(data["voice"]),
    };
  } catch {
    return null;
  }
}
