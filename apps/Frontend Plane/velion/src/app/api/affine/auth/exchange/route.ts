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

const AFFINE_CORE_URL = process.env.AFFINE_CORE_URL ?? 'http://affine-core:3180';

// ── Route handler ─────────────────────────────────────────────────────────────

export async function POST(request: Request) {
  try {
    const upstream = await fetch(`${AFFINE_CORE_URL}/api/v1/session/exchange`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'x-internal-api-key': process.env.INTERNAL_API_KEY ?? '',
        cookie: request.headers.get('cookie') ?? '',
      },
      body: JSON.stringify({}),
      cache: 'no-store',
    });

    if (upstream.status === 401) {
      return NextResponse.json({ error: 'Unauthorised' }, { status: 401 });
    }

    if (!upstream.ok) {
      throw new Error(`affine-core session exchange failed: ${upstream.status}`);
    }

    const affineSession = (await upstream.json()) as {
      user_id?: string;
      workspace_id?: string;
      expires_at?: number;
      runtime_cookies?: string[];
    };

    const response = NextResponse.json({
      token: null,
      userId: affineSession.user_id ?? 'planner-user',
      workspaceId: affineSession.workspace_id ?? 'planner-anonymous',
      expiresAt: affineSession.expires_at ?? Date.now() + 7 * 24 * 60 * 60 * 1000,
      proxiedAsAdmin: true,
      appUserId: affineSession.user_id ?? null,
    });

    for (const setCookie of affineSession.runtime_cookies ?? []) {
      response.headers.append('set-cookie', setCookie);
    }

    return response;
  } catch (err) {
    const message = err instanceof Error ? err.message : 'Internal error';
    console.error('[affine/auth/exchange]', message);
    return NextResponse.json({ error: message }, { status: 502 });
  }
}
