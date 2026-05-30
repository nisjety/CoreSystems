import { afterEach, describe, expect, it, vi } from "vitest";
import {
  buildControlPlaneAuthUrl,
  extractControlPlaneAuthUser,
  isControlPlaneAuthConfigured,
  proxyControlPlaneAuthRequest,
  rewriteSetCookieForProxy,
} from "@/lib/auth/control-plane";

const ORIGINAL_ENV = { ...process.env };

afterEach(() => {
  vi.unstubAllEnvs();
  process.env = { ...ORIGINAL_ENV };
});

describe("control plane auth integration", () => {
  it("stays disabled until an auth-core URL is configured", () => {
    vi.stubEnv("CONTROL_PLANE_AUTH_URL", "");
    vi.stubEnv("AUTH_CORE_URL", "");
    vi.stubEnv("AUTH_SERVICE_URL", "");

    expect(isControlPlaneAuthConfigured()).toBe(false);
  });

  it("builds auth-core URLs without duplicating the Better Auth prefix", () => {
    vi.stubEnv("AUTH_CORE_URL", "http://localhost:3011/api/auth");

    expect(
      buildControlPlaneAuthUrl("/api/auth/sign-in/email", "?redirect=false").toString(),
    ).toBe("http://localhost:3011/api/auth/sign-in/email?redirect=false");
  });

  it("supports root auth-core service URLs", () => {
    vi.stubEnv("AUTH_CORE_URL", "http://auth-core:3011");

    expect(buildControlPlaneAuthUrl("/api/auth/get-session").toString()).toBe(
      "http://auth-core:3011/api/auth/get-session",
    );
  });

  it("extracts a Better Auth session user from direct or data-wrapped payloads", () => {
    expect(extractControlPlaneAuthUser({ user: { id: "user_1", email: "a@b.test" } })).toEqual({
      id: "user_1",
      email: "a@b.test",
      image: undefined,
      name: undefined,
    });
    expect(extractControlPlaneAuthUser({ data: { user: { id: "user_2", name: "Ima" } } })).toEqual({
      id: "user_2",
      email: undefined,
      image: undefined,
      name: "Ima",
    });
  });

  it("strips upstream cookie domains by default for the same-origin proxy", () => {
    expect(rewriteSetCookieForProxy("sid=abc; Path=/; Domain=auth.local; HttpOnly")).toBe(
      "sid=abc; Path=/; HttpOnly",
    );
  });

  it("proxies auth requests to auth-core and preserves safe cookie attributes", async () => {
    vi.stubEnv("AUTH_CORE_URL", "http://auth-core:3011");
    const fetchMock = vi.fn(async () => {
      const response = new Response(JSON.stringify({ ok: true }), {
        headers: {
          "content-type": "application/json",
          "set-cookie": "sid=abc; Path=/; Domain=auth.local; HttpOnly",
        },
        status: 200,
      });
      return response;
    });
    vi.stubGlobal("fetch", fetchMock);

    const response = await proxyControlPlaneAuthRequest(
      new Request("http://localhost:3107/api/auth/sign-in/email", {
        body: JSON.stringify({ email: "ima@example.com", password: "secret" }),
        headers: {
          "content-type": "application/json",
          cookie: "existing=value",
          host: "localhost:3107",
          origin: "http://localhost:3107",
        },
        method: "POST",
      }),
    );

    expect(fetchMock).toHaveBeenCalledWith(
      new URL("http://auth-core:3011/api/auth/sign-in/email"),
      expect.objectContaining({
        cache: "no-store",
        method: "POST",
        redirect: "manual",
      }),
    );
    expect(response.headers.get("set-cookie")).toBe("sid=abc; Path=/; HttpOnly");
  });
});
