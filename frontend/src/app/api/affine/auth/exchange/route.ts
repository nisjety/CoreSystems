/**
 * POST /api/affine/auth/exchange
 *
 * Signs into the self-hosted AFFiNE backend server-side using the configured
 * admin credentials and forwards AFFiNE cookies back to the browser.
 *
 * This is intentionally cookie-based because the current AFFiNE self-host API
 * uses session cookies rather than bearer tokens for browser auth.
 */

import { NextResponse } from 'next/server';
import { AFFINE_SERVER_URL } from '@/lib/affine/config';
import { getServerSession } from '@/components/auth/lib/auth-server';

const AFFINE_SERVER_INTERNAL_URL =
  process.env.AFFINE_SERVER_INTERNAL_URL ?? AFFINE_SERVER_URL;
const AFFINE_ADMIN_EMAIL = process.env.AFFINE_ADMIN_EMAIL ?? 'admin@localhost';
const AFFINE_ADMIN_PASSWORD = process.env.AFFINE_ADMIN_PASSWORD ?? '';

function getSetCookieHeaders(response: Response) {
  const responseHeaders = response.headers as Headers & {
    getSetCookie?: () => string[];
  };

  if (typeof responseHeaders.getSetCookie === 'function') {
    return responseHeaders.getSetCookie();
  }

  const fallback = response.headers.get('set-cookie');
  return fallback ? [fallback] : [];
}

// ── Route handler ─────────────────────────────────────────────────────────────

export async function POST() {
  if (!AFFINE_ADMIN_PASSWORD) {
    return NextResponse.json(
      { error: 'AFFiNE server not configured (set AFFINE_ADMIN_PASSWORD)' },
      { status: 503 }
    );
  }

  const allowDevFallback = process.env.NODE_ENV !== 'production';
  const appSession = allowDevFallback ? null : await getServerSession();

  if (!appSession && !allowDevFallback) {
    return NextResponse.json({ error: 'Unauthorised' }, { status: 401 });
  }

  try {
    const upstream = await fetch(`${AFFINE_SERVER_INTERNAL_URL}/api/auth/sign-in`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email: AFFINE_ADMIN_EMAIL,
        password: AFFINE_ADMIN_PASSWORD,
      }),
      redirect: 'manual',
      cache: 'no-store',
    });

    if (!upstream.ok) {
      throw new Error(`AFFiNE admin sign-in failed: ${upstream.status}`);
    }

    const affineUser = (await upstream.json()) as {
      id?: string;
      email?: string;
      name?: string;
    };

    const response = NextResponse.json({
      token: null,
      userId: affineUser.id ?? affineUser.email ?? AFFINE_ADMIN_EMAIL,
      workspaceId: 'planner',
      expiresAt: Date.now() + 7 * 24 * 60 * 60 * 1000,
      proxiedAsAdmin: true,
      appUserId: appSession?.user.id ?? null,
    });

    for (const setCookie of getSetCookieHeaders(upstream)) {
      response.headers.append('set-cookie', setCookie);
    }

    return response;
  } catch (err) {
    const message = err instanceof Error ? err.message : 'Internal error';
    console.error('[affine/auth/exchange]', message);
    return NextResponse.json({ error: message }, { status: 502 });
  }
}
