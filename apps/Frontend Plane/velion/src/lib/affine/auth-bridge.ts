/**
 * AFFiNE auth bridge.
 *
 * AFFiNE self-host uses cookie-backed sessions.
 *
 * This module mediates between the two:
 *  1. Calls your own Next.js bridge route (POST /api/affine/auth/exchange)
 *  2. That route signs into AFFiNE server-side and forwards AFFiNE cookies
 *     back to the browser for localhost-based WebSocket sync
 *  3. Caches the resulting AFFiNE session metadata in sessionStorage
 */

const AFFINE_TOKEN_KEY = 'affine_session_token';

export interface AffineSession {
  token: string;
  userId: string;
  workspaceId: string;
  expiresAt: number; // Unix ms
}

/** Retrieve a cached AFFiNE session if still valid (5-min buffer). */
function getCachedSession(): AffineSession | null {
  if (typeof window === 'undefined') return null;
  try {
    const raw = sessionStorage.getItem(AFFINE_TOKEN_KEY);
    if (!raw) return null;
    const session: AffineSession = JSON.parse(raw);
    const isExpired = Date.now() > session.expiresAt - 5 * 60 * 1000;
    return isExpired ? null : session;
  } catch {
    return null;
  }
}

function cacheSession(session: AffineSession) {
  if (typeof window === 'undefined') return;
  sessionStorage.setItem(AFFINE_TOKEN_KEY, JSON.stringify(session));
}

function clearSession() {
  if (typeof window === 'undefined') return;
  sessionStorage.removeItem(AFFINE_TOKEN_KEY);
}

/**
 * Get (or refresh) an AFFiNE session for the currently-logged-in user.
 *
 * Falls back to a local-only (offline) mode if the exchange request fails —
 * the editor still works via IndexedDB, just without server sync.
 */
export async function getAffineSession(): Promise<AffineSession | null> {
  const cached = getCachedSession();
  if (cached) return cached;

  try {
    const res = await fetch('/api/affine/auth/exchange', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      credentials: 'include',
    });

    if (!res.ok) {
      console.warn('[affine-bridge] token exchange failed:', res.status);
      return null;
    }

    const session: AffineSession = await res.json();
    cacheSession(session);
    return session;
  } catch (err) {
    console.warn('[affine-bridge] offline or exchange error:', err);
    return null;
  }
}

/** Build the WebSocket URL with the AFFiNE token as a query param. */
export function buildSyncWsUrl(baseWsUrl: string, token: string | null, workspaceId: string): string {
  const url = new URL(`${baseWsUrl}/api/sync`);
  url.searchParams.set('workspaceId', workspaceId);
  if (token) url.searchParams.set('token', token);
  return url.toString();
}

export { clearSession };
