import { NextResponse } from "next/server"
import type { NextRequest } from "next/server"

/**
 * Next.js 16 "proxy" (formerly middleware.ts; renamed + Node-only runtime).
 *
 * COARSE auth gate only: redirects clearly-unauthenticated requests for
 * workspace routes to /login fast, before the RSC render. The authoritative
 * checks remain server-side (every workspace page calls
 * requireCompletedOnboarding / requireOnboardingAccess, and the workspace
 * layout calls getControlPlaneContext). This is fail-OPEN here / fail-CLOSED
 * in RSC: we only redirect when NO session-like cookie is present, so we never
 * bounce a genuinely authenticated user on a cookie-name mismatch.
 */

const SESSION_COOKIE_HINT = /sess|sid|auth|idknuten|velion/i

function extraCookieNames(): string[] {
  const rawCookieNames = process.env.AUTH_SESSION_COOKIE_NAMES || ""

  return rawCookieNames
    .split(",")
    .map((value) => value.trim())
    .filter((value) => value.length > 0)
}

function isPlaywrightAuthenticated(request: NextRequest) {
  if (process.env.NODE_ENV === "production" || process.env.PLAYWRIGHT_TEST_AUTH !== "1") {
    return false
  }

  const userId = request.headers.get("x-playwright-auth-user-id")
  return Boolean(userId && userId.trim())
}

export function proxy(request: NextRequest) {
  if (isPlaywrightAuthenticated(request)) {
    return NextResponse.next()
  }

  const extra = extraCookieNames()
  const looksAuthenticated = request.cookies
    .getAll()
    .some((cookie) => SESSION_COOKIE_HINT.test(cookie.name) || extra.includes(cookie.name))

  if (looksAuthenticated) {
    return NextResponse.next()
  }

  // Open-redirect guard: only echo a same-origin relative path as callbackUrl.
  const pathname = request.nextUrl.pathname
  const safeCallback =
    pathname.startsWith("/") && !pathname.startsWith("//")
      ? pathname + request.nextUrl.search
      : "/dashboard"

  const loginUrl = new URL("/login", request.url)
  loginUrl.searchParams.set("callbackUrl", safeCallback)
  return NextResponse.redirect(loginUrl)
}

export const config = {
  matcher: [
    "/dashboard/:path*",
    "/agents/:path*",
    "/chat/:path*",
    "/inbox/:path*",
    "/knowledge/:path*",
    "/account/:path*",
    "/settings/:path*",
    "/onboarding/:path*",
  ],
}
