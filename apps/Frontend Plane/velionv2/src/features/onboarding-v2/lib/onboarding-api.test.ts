import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  cleanupOnboardingSource,
  createConnectSession,
  disconnectConnections,
} from "./onboarding-api";

function makeFetchResponse(body: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  } as Response;
}

describe("createConnectSession", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("unwraps the BFF data envelope", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({
        data: {
          connectUrl: "https://login.microsoftonline.com/oauth",
          authMode: "direct-oauth",
          sessionToken: "session_123",
          expiresAt: "2026-06-02T18:00:00.000Z",
          providerConfigKey: "microsoft",
        },
      }),
    );

    const result = await createConnectSession({
      provider: "microsoft",
      sources: ["sharepoint"],
    });

    const [url, init] = mockFetch.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("/api/v1/integrations/providers/microsoft/connect-session");
    expect(init.credentials).toBe("include");
    expect(JSON.parse(init.body as string)).toEqual({
      selectedSources: ["sharepoint"],
      bundles: ["onboarding"],
    });
    expect(result).toEqual({
      connectUrl: "https://login.microsoftonline.com/oauth",
      authMode: "direct-oauth",
      sessionToken: "session_123",
      expiresAt: "2026-06-02T18:00:00.000Z",
      providerConfigKey: "microsoft",
    });
  });

  it("keeps compatibility with a flat connect-session response", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({
        connectUrl: "https://slack.com/oauth",
        authMode: "direct-oauth",
        sessionToken: "session_456",
      }),
    );

    await expect(
      createConnectSession({ provider: "slack", sources: ["messages"] }),
    ).resolves.toEqual({
      connectUrl: "https://slack.com/oauth",
      authMode: "direct-oauth",
      sessionToken: "session_456",
      expiresAt: undefined,
      providerConfigKey: undefined,
    });
  });

  it("throws a readable BFF error when connect-session fails", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse(
        { error: { code: "connect_session_failed", message: "Provider credentials are missing." } },
        400,
      ),
    );

    await expect(
      createConnectSession({ provider: "github", sources: ["issues"] }),
    ).rejects.toThrow("Provider credentials are missing.");
  });
});

describe("disconnectConnections", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("does not call the BFF when there are no connectors to remove", async () => {
    const result = await disconnectConnections([]);

    expect(fetch).not.toHaveBeenCalled();
    expect(result).toEqual({ disconnected: [], failed: [], skippedProviderKeys: [] });
  });

  it("POSTs connector ids to the disconnect BFF as a keepalive request", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({
        disconnected: [{ id: "conn-1", providerKey: "notion" }],
        failed: [],
        skippedProviderKeys: [],
      }),
    );

    await disconnectConnections(["notion"]);

    const [url, init] = mockFetch.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("/api/connections/disconnect");
    expect(init.method).toBe("POST");
    expect(init.credentials).toBe("include");
    expect(init.keepalive).toBe(true);
    expect(JSON.parse(init.body as string)).toEqual({ connectors: ["notion"] });
  });

  it("throws a readable BFF error", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeFetchResponse({ error: { message: "No active organization found." } }, 409),
    );

    await expect(disconnectConnections(["github"])).rejects.toThrow(
      "No active organization found.",
    );
  });
});

describe("cleanupOnboardingSource", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("requests best-effort source cleanup as keepalive", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(makeFetchResponse({ cleanupStatus: "completed" }));

    const result = await cleanupOnboardingSource({
      connectorId: "github",
      documentId: "doc_123",
    });

    const [url, init] = mockFetch.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("/api/onboarding/source-cleanup");
    expect(init.method).toBe("POST");
    expect(init.keepalive).toBe(true);
    expect(JSON.parse(init.body as string)).toEqual({
      connectorId: "github",
      documentId: "doc_123",
    });
    expect(result).toEqual({ cleanupStatus: "completed" });
  });

  it("degrades to failed cleanup status on BFF failure", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(makeFetchResponse({ error: "down" }, 502));

    await expect(cleanupOnboardingSource({ connectorId: "github" })).resolves.toEqual({
      cleanupStatus: "failed",
    });
  });
});
