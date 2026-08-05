import "server-only";
import { headers } from "next/headers";
import { getControlPlaneCurrentUser, isControlPlaneAuthConfigured } from "@/lib/auth/control-plane";
import { hasAuthDatabaseConfig } from "@/lib/auth/database";

type BetterAuthUser = {
  id: string;
  email?: string;
  image?: string | null;
  name?: string;
};

export type AuthenticatedUser = BetterAuthUser & {
  cookieHeader?: string;
  testAuth?: boolean;
  testOrgId?: string;
};

function playwrightAuthUser(headerList: Awaited<ReturnType<typeof headers>>): AuthenticatedUser | null {
  if (process.env.NODE_ENV === "production" || process.env.PLAYWRIGHT_TEST_AUTH !== "1") {
    return null;
  }

  const id = headerList.get("x-playwright-auth-user-id")?.trim();
  if (!id) {
    return null;
  }

  return {
    id,
    email: headerList.get("x-playwright-auth-email")?.trim() || "playwright@verevon.local",
    image: null,
    name: headerList.get("x-playwright-auth-name")?.trim() || "Playwright User",
    cookieHeader: headerList.get("cookie") ?? undefined,
    testAuth: true,
    testOrgId: headerList.get("x-playwright-org-id")?.trim() || "org_playwright",
  };
}

type CachedAuthUser = { user: AuthenticatedUser; expiresAtMs: number };
const AUTH_USER_CACHE = new Map<string, CachedAuthUser>();
const AUTH_USER_CACHE_TTL_MS = 15_000;
const AUTH_USER_CACHE_MAX = 512;

function authCacheKey(cookie: string): string {
  let hash = 2166136261;
  for (let i = 0; i < cookie.length; i += 1) {
    hash ^= cookie.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(36);
}

export async function getCurrentAuthUser(): Promise<AuthenticatedUser | null> {
  const headerList = await headers();
  const testUser = playwrightAuthUser(headerList);

  if (testUser) {
    return testUser;
  }

  if (isControlPlaneAuthConfigured()) {
    const cookie = headerList.get("cookie") ?? "";
    const cacheKey = cookie ? authCacheKey(cookie) : null;
    const now = Date.now();

    if (cacheKey) {
      const cached = AUTH_USER_CACHE.get(cacheKey);
      if (cached && cached.expiresAtMs > now) {
        return cached.user;
      }
      if (cached) {
        AUTH_USER_CACHE.delete(cacheKey);
      }
    }

    try {
      const user = await getControlPlaneCurrentUser(headerList);
      if (!user) {
        return null;
      }

      const resolved: AuthenticatedUser = {
        id: user.id,
        email: user.email,
        image: user.image,
        name: user.name,
        cookieHeader: cookie || undefined,
      };

      if (cacheKey) {
        if (AUTH_USER_CACHE.size >= AUTH_USER_CACHE_MAX) {
          const oldestKey = AUTH_USER_CACHE.keys().next().value;
          if (oldestKey !== undefined) {
            AUTH_USER_CACHE.delete(oldestKey);
          }
        }
        AUTH_USER_CACHE.set(cacheKey, { user: resolved, expiresAtMs: now + AUTH_USER_CACHE_TTL_MS });
      }

      return resolved;
    } catch {
      return null;
    }
  }

  if (!hasAuthDatabaseConfig()) {
    return null;
  }

  try {
    const { auth } = await import("@/lib/auth/auth");
    const session = await auth.api.getSession({ headers: headerList });
    const user = session?.user as BetterAuthUser | undefined;

    if (!user?.id) {
      return null;
    }

    return {
      id: user.id,
      email: user.email,
      image: user.image,
      name: user.name,
      cookieHeader: headerList.get("cookie") ?? undefined,
    };
  } catch {
    return null;
  }
}
