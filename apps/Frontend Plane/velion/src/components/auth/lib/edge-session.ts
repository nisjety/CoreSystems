import 'server-only'

import { headers } from 'next/headers'
import { redirect } from 'next/navigation'

import { getServerSession } from './auth-server'

const VELION_USER_ID_HEADER = 'x-velion-user-id'
const VELION_USER_EMAIL_HEADER = 'x-velion-user-email'
const VELION_USER_NAME_HEADER = 'x-velion-user-name'

export interface EdgeUser {
  userId: string
  email: string
  name: string
}

/**
 * G28 + G28-followup — first-line auth check for protected pages.
 *
 * The edge gate (`src/proxy.ts`) already validates the session for every
 * matched path and redirects unauthenticated users to `/login`. The gate
 * stamps the resolved user identity onto the forwarded request via three
 * headers (`x-velion-user-{id,email,name}`). Pages can therefore skip the
 * per-page `getServerSession()` round-trip whenever those headers are
 * present.
 *
 * When the headers are missing (auth-core was unreachable at gate time and
 * the gate fail-open'd, or the matcher didn't fire for this path), we fall
 * back to `getServerSession()` so we don't accidentally render protected
 * content to an unauthenticated request.
 *
 * Usage at the top of a `(dashboard)/.../page.tsx`:
 *
 *   const { userId, email, name } = await requireEdgeUser('/dashboard');
 *
 * Returns `{ userId, email, name }`. Throws via `redirect()` when there's
 * no session at all — the function does not return on the unauth path.
 */
export async function requireEdgeUser(
  pathForLoginRedirect: string,
): Promise<EdgeUser> {
  const hdrs = await headers()
  const userId = hdrs.get(VELION_USER_ID_HEADER)?.trim()
  if (userId) {
    return {
      userId,
      email: hdrs.get(VELION_USER_EMAIL_HEADER)?.trim() ?? '',
      name: hdrs.get(VELION_USER_NAME_HEADER)?.trim() ?? '',
    }
  }

  // Fail-back to the legacy auth-core round-trip.
  const session = await getServerSession()
  if (session?.user?.id) {
    return {
      userId: session.user.id,
      email: session.user.email ?? '',
      name: session.user.name ?? '',
    }
  }

  redirect(`/login?redirect=${encodeURIComponent(pathForLoginRedirect)}`)
}

export { VELION_USER_ID_HEADER, VELION_USER_EMAIL_HEADER, VELION_USER_NAME_HEADER }
