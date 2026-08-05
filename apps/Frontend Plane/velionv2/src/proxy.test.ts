import { afterEach, describe, expect, it, vi } from "vitest"
import { NextRequest } from "next/server"

import { proxy } from "./proxy"

function createRequest(path: string, init?: ConstructorParameters<typeof NextRequest>[1]) {
  return new NextRequest(new URL(path, "https://app.verevon.test"), init)
}

function expectNextResponse(response: Response) {
  expect(response.status).toBe(200)
  expect(response.headers.get("x-middleware-next")).toBe("1")
}

describe("proxy", () => {
  afterEach(() => {
    vi.unstubAllEnvs()
  })

  it("redirects unauthenticated workspace requests to login with a safe callback", () => {
    const response = proxy(createRequest("/dashboard?tab=inbox"))
    const location = response.headers.get("location")

    expect(response.status).toBe(307)
    expect(location).not.toBeNull()

    const redirectUrl = new URL(location ?? "", "https://app.verevon.test")
    expect(redirectUrl.pathname).toBe("/login")
    expect(redirectUrl.searchParams.get("callbackUrl")).toBe("/dashboard?tab=inbox")
  })

  it("allows requests with session-like cookies through", () => {
    const response = proxy(
      createRequest("/knowledge", {
        headers: { cookie: "idknuten.sid=session_123" },
      }),
    )

    expectNextResponse(response)
  })

  it("allows requests with configured session cookie names through", () => {
    vi.stubEnv("AUTH_SESSION_COOKIE_NAMES", "custom_session")

    const response = proxy(
      createRequest("/settings", {
        headers: { cookie: "custom_session=session_123" },
      }),
    )

    expectNextResponse(response)
  })

  it("allows Playwright-authenticated development requests through", () => {
    vi.stubEnv("NODE_ENV", "development")
    vi.stubEnv("PLAYWRIGHT_TEST_AUTH", "1")

    const response = proxy(
      createRequest("/dashboard", {
        headers: { "x-playwright-auth-user-id": "user_test" },
      }),
    )

    expectNextResponse(response)
  })
})
