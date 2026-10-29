import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { POST } from "../../app/api/contact/route";
import {
  getContactConfig,
  getContactConfigStatus,
  isContactConfigured,
} from "./config";
import { handleContactRequest } from "./handler";

const payload = {
  name: " Test Person ",
  email: "test@example.com",
  company: "Test Company",
  projectType: "ki",
  message: "We would like help with our workflow.",
  submissionId: "95572209-f995-45a3-93b8-b6e8f115b156",
  source: "ki-og-automatisering",
  website: "",
};
const providerId = "40751a8f-530d-417f-bb8d-a29b0b2d58ca";
const request = (
  data: unknown = payload,
  headers: Record<string, string> = {},
) =>
  new Request("https://www.triodelab.no/api/contact", {
    method: "POST",
    headers: {
      "content-type": "application/json",
      origin: "https://www.triodelab.no",
      "x-vercel-forwarded-for": "192.0.2.1",
      ...headers,
    },
    body: JSON.stringify(data),
  });

function mockProviders() {
  const receipts = new Map<string, string>();
  const metricWrites: string[][] = [];
  const send = vi.fn(async () => Response.json({ id: providerId }));
  const fetchMock = vi.fn(
    async (url: string | URL | Request, init?: RequestInit) => {
      if (String(url) === "https://api.resend.com/emails") return send();
      const command = JSON.parse(String(init?.body)) as string[];
      if (command[0] === "GET") {
        return Response.json({ result: receipts.get(command[1]) ?? null });
      }
      if (command[1].includes("CONTACT_RATE_LIMIT")) {
        return Response.json({ result: [1, 0] });
      }
      const receiptKey = command[3];
      if (receipts.has(receiptKey)) return Response.json({ result: 0 });
      receipts.set(receiptKey, command[5]);
      metricWrites.push(command);
      return Response.json({ result: 1 });
    },
  );
  vi.stubGlobal("fetch", fetchMock);
  return { fetchMock, metricWrites, send };
}

beforeEach(() => {
  vi.stubEnv("NODE_ENV", "production");
  vi.stubEnv("VERCEL", "1");
  vi.stubEnv("RESEND_API_KEY", "test_resend_key");
  vi.stubEnv("CONTACT_FROM_EMAIL", "website@triodelab.no");
  vi.stubEnv("CONTACT_TO_EMAIL", "post@triodelab.no");
  vi.stubEnv("UPSTASH_REDIS_REST_URL", "https://test.upstash.io");
  vi.stubEnv("UPSTASH_REDIS_REST_TOKEN", "test_redis_token");
  vi.spyOn(console, "error").mockImplementation(() => undefined);
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
});

describe("contact configuration", () => {
  it("requires Resend settings but treats Upstash as optional", () => {
    expect(getContactConfig()?.redis).not.toBeNull();
    vi.stubEnv("CONTACT_FROM_EMAIL", "TriodeLab <website@triodelab.no>");
    expect(isContactConfigured()).toBe(true);
    vi.stubEnv("CONTACT_FROM_EMAIL", "not-an-email");
    expect(isContactConfigured()).toBe(false);
    expect(console.error).toHaveBeenCalledWith(
      "contact_config_invalid: from",
    );
    vi.stubEnv("CONTACT_FROM_EMAIL", "website@triodelab.no");
    vi.stubEnv("UPSTASH_REDIS_REST_TOKEN", "");
    expect(isContactConfigured()).toBe(true);
    expect(getContactConfig()?.redis).toBeNull();
  });

  it("falls back to a default sender when CONTACT_FROM_EMAIL is missing", () => {
    vi.stubEnv("CONTACT_FROM_EMAIL", "");
    expect(getContactConfig()?.from).toBe("TriodeLab <skjema@triodelab.no>");
    vi.stubEnv("RESEND_API_KEY", "");
    expect(getContactConfigStatus()).toEqual({
      configured: false,
      resendApiKey: false,
      fromEmail: false,
      toEmail: true,
      upstash: true,
    });
  });

  it("accepts the KV_* names from the Vercel Upstash integration", () => {
    vi.stubEnv("UPSTASH_REDIS_REST_URL", "");
    vi.stubEnv("UPSTASH_REDIS_REST_TOKEN", "");
    vi.stubEnv("KV_REST_API_URL", "https://kv.upstash.io");
    vi.stubEnv("KV_REST_API_TOKEN", "kv_token");
    expect(getContactConfig()?.redis).toEqual({
      url: "https://kv.upstash.io",
      token: "kv_token",
    });
  });
});

describe("contact submission boundary", () => {
  it.each([
    ["foreign origin", payload, { origin: "https://attacker.example" }, 403],
    ["null origin", payload, { origin: "null" }, 403],
    ["non-JSON content", payload, { "content-type": "text/plain" }, 415],
    ["invalid email", { ...payload, email: "invalid" }, {}, 400],
    ["oversize company", { ...payload, company: "x".repeat(201) }, {}, 400],
    [
      "unknown source",
      { ...payload, source: "https://example.com/?email=secret" },
      {},
      400,
    ],
    ["unknown service", { ...payload, projectType: "injected" }, {}, 400],
    ["invalid receipt ID", { ...payload, submissionId: "not-uuid" }, {}, 400],
    ["honeypot", { ...payload, website: "spam.example" }, {}, 400],
  ])(
    "rejects %s before contacting providers",
    async (_, data, headers, status) => {
      const { fetchMock } = mockProviders();
      expect((await POST(request(data, headers))).status).toBe(status);
      expect(fetchMock).not.toHaveBeenCalled();
    },
  );

  it("bounds actual streamed bytes even without Content-Length", async () => {
    const { fetchMock } = mockProviders();
    expect(
      (await POST(request({ ...payload, message: "x".repeat(17_000) }))).status,
    ).toBe(413);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("rejects missing origin, malformed JSON and unsupported methods", async () => {
    const { fetchMock } = mockProviders();
    const missingOrigin = request();
    missingOrigin.headers.delete("origin");
    expect((await POST(missingOrigin)).status).toBe(403);
    const brokenJson = new Request("https://www.triodelab.no/api/contact", {
      method: "POST",
      headers: request().headers,
      body: "{broken",
    });
    expect((await POST(brokenJson)).status).toBe(400);
    expect(
      (
        await handleContactRequest(
          new Request("https://www.triodelab.no/api/contact"),
        )
      ).status,
    ).toBe(405);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("allows only the deployment origin supplied by Vercel server configuration", async () => {
    const { send } = mockProviders();
    vi.stubEnv("VERCEL_URL", "triodelabs-preview.vercel.app");
    expect(
      (
        await POST(
          request(payload, { origin: "https://triodelabs-preview.vercel.app" }),
        )
      ).status,
    ).toBe(200);
    expect(
      (await POST(request(payload, { origin: "https://attacker.vercel.app" })))
        .status,
    ).toBe(403);
    expect(send).toHaveBeenCalledTimes(1);
  });

  it("returns an honest unavailable result when unconfigured", async () => {
    const { fetchMock } = mockProviders();
    vi.stubEnv("RESEND_API_KEY", "");
    const response = await POST(request());
    expect(response.status).toBe(503);
    expect(await response.json()).toMatchObject({ success: false });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("still sends when durable rate limiting is unavailable", async () => {
    const { fetchMock, send } = mockProviders();
    fetchMock.mockRejectedValueOnce(
      new Error("redis failure with sensitive detail"),
    );
    expect((await POST(request())).status).toBe(200);
    expect(send).toHaveBeenCalledTimes(1);
    expect(console.error).toHaveBeenCalledWith(
      "contact_rate_limit_unavailable",
    );
  });

  it("still sends via Resend when Upstash is not configured", async () => {
    const { fetchMock, send } = mockProviders();
    vi.stubEnv("UPSTASH_REDIS_REST_URL", "");
    vi.stubEnv("UPSTASH_REDIS_REST_TOKEN", "");
    const response = await POST(request());
    expect(response.status).toBe(200);
    expect(send).toHaveBeenCalledTimes(1);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(console.error).toHaveBeenCalledWith(
      "contact_rate_limit_not_configured",
    );
  });

  it("blocks exceeded limits with Retry-After", async () => {
    const { fetchMock, send } = mockProviders();
    fetchMock.mockResolvedValueOnce(Response.json({ result: [0, 400] }));
    const response = await POST(request());
    expect(response.status).toBe(429);
    expect(response.headers.get("retry-after")).toBe("400");
    expect(send).not.toHaveBeenCalled();
  });

  it("does not confirm or count a failed or unacknowledged provider request", async () => {
    const { send, metricWrites } = mockProviders();
    send.mockResolvedValueOnce(
      Response.json({ error: "private diagnostic" }, { status: 503 }),
    );
    expect((await POST(request())).status).toBe(502);
    send.mockResolvedValueOnce(Response.json({}));
    expect((await POST(request())).status).toBe(502);
    expect(metricWrites).toHaveLength(0);
  });

  it("handles provider timeout without revealing private diagnostics", async () => {
    const { send, metricWrites } = mockProviders();
    send.mockRejectedValueOnce(
      new DOMException("timeout details", "TimeoutError"),
    );
    const response = await POST(request());
    expect(response.status).toBe(502);
    expect(JSON.stringify(await response.json())).not.toContain(
      "timeout details",
    );
    expect(metricWrites).toHaveLength(0);
  });

  it("sends safely and records only allowlisted aggregate fields", async () => {
    const { fetchMock, metricWrites } = mockProviders();
    const response = await POST(request());
    expect(await response.json()).toEqual({ success: true });
    const emailCall = fetchMock.mock.calls.find(
      ([url]) => url === "https://api.resend.com/emails",
    );
    const options = emailCall?.[1];
    expect(options?.headers).toMatchObject({
      "Idempotency-Key": `contact/${payload.submissionId}`,
    });
    expect(JSON.parse(String(options?.body))).toMatchObject({
      from: "website@triodelab.no",
      to: ["post@triodelab.no"],
      reply_to: "test@example.com",
    });
    expect(metricWrites).toHaveLength(1);
    const redisCalls = fetchMock.mock.calls.filter(
      ([url]) => url !== "https://api.resend.com/emails",
    );
    const serialized = JSON.stringify(redisCalls);
    for (const privateValue of [
      payload.name.trim(),
      payload.email,
      payload.message,
      "192.0.2.1",
    ]) {
      expect(serialized).not.toContain(privateValue);
    }
    expect(serialized).toContain("projectType:ki");
    expect(serialized).toContain("source:ki-og-automatisering");
  });

  it("returns recorded success on retries without sending or counting twice", async () => {
    const { send, metricWrites } = mockProviders();
    expect((await POST(request())).status).toBe(200);
    expect((await POST(request())).status).toBe(200);
    expect(send).toHaveBeenCalledTimes(1);
    expect(metricWrites).toHaveLength(1);
    expect(
      (
        await POST(
          request({ ...payload, message: "Changed request with reused ID." }),
        )
      ).status,
    ).toBe(409);
    expect(send).toHaveBeenCalledTimes(1);
  });

  it("deduplicates metric writes for concurrent accepted retries", async () => {
    const { send, metricWrites, fetchMock } = mockProviders();
    const responses = await Promise.all([POST(request()), POST(request())]);
    expect(responses.every((response) => response.ok)).toBe(true);
    expect(send).toHaveBeenCalledTimes(2);
    expect(metricWrites).toHaveLength(1);
    const keys = fetchMock.mock.calls
      .filter(([url]) => url === "https://api.resend.com/emails")
      .map(
        ([, options]) =>
          (options?.headers as Record<string, string>)["Idempotency-Key"],
      );
    expect(new Set(keys).size).toBe(1);
  });

  it("keeps accepted success if aggregate metrics fail afterwards", async () => {
    const { fetchMock } = mockProviders();
    const original = fetchMock.getMockImplementation()!;
    fetchMock.mockImplementation(async (url, options) => {
      if (String(options?.body).includes("CONTACT_RECORD_LEAD"))
        throw new Error("metrics down");
      return original(url, options);
    });
    expect((await POST(request())).status).toBe(200);
    expect(console.error).toHaveBeenCalledWith("contact_metrics_unavailable");
  });
});
