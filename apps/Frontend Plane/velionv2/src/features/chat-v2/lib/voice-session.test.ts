import { afterEach, describe, expect, it, vi } from "vitest";
import { createVoiceSession } from "./voice-session";

describe("createVoiceSession", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("POSTs to the BFF and maps the minted session", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          session_id: "sess-1",
          client_secret: "ek_abc",
          websocket_url: "wss://rt/x",
          expires_at: 123,
          model_used: "gpt-realtime",
          voice: "alloy",
        }),
        { status: 200 },
      ),
    );

    const session = await createVoiceSession({ voice: "alloy" });

    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toBe("/api/voice/session");
    expect(init).toMatchObject({ method: "POST", credentials: "include" });
    expect(session).toEqual({
      sessionId: "sess-1",
      clientSecret: "ek_abc",
      websocketUrl: "wss://rt/x",
      expiresAt: 123,
      modelUsed: "gpt-realtime",
      voice: "alloy",
    });
  });

  it("returns null when the session lacks a credential or endpoint", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ session_id: "s", client_secret: "", websocket_url: "" }), {
        status: 200,
      }),
    );
    expect(await createVoiceSession()).toBeNull();
  });

  it("returns null on a non-ok response", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response("nope", { status: 502 }));
    expect(await createVoiceSession()).toBeNull();
  });
});
