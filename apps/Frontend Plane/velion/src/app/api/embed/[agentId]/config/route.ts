import { NextRequest, NextResponse } from 'next/server'

import { convexQuery } from '@/app/api/_lib/convex-client'

/**
 * Wave 9 (ui-ux-verevon-gap.md §19): public embed widget config endpoint.
 *
 * Called by the public `embed.js` bundle when the script tag first
 * mounts on a customer site. Returns the minimal info the bubble needs:
 *   { id, name, greeting, theme: { accentColor, buttonLabel, welcomeMessage } }
 *
 * Requires `?secret=...` in the query string — matches what's in the
 * agent's `publicSecret`. Without the right secret OR if the agent has
 * `publicEnabled=false`, returns 404 (we deliberately don't leak the
 * distinction so a scraper can't enumerate agent ids).
 *
 * Public surface: NO auth, NO CORS check — by design. The bubble is
 * meant to load from arbitrary third-party sites. Origin enforcement
 * happens on the message-send endpoint, not here.
 */
interface EmbedConfig {
  id: string
  name: string
  greeting: string
  theme: {
    accentColor?: string
    buttonLabel?: string
    welcomeMessage?: string
  }
  orgId: string
}

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ agentId: string }> },
): Promise<Response> {
  const { agentId } = await params
  const secret = request.nextUrl.searchParams.get('secret') ?? ''
  if (!secret) {
    return NextResponse.json({ error: 'not_found' }, { status: 404 })
  }
  try {
    const config = await convexQuery<EmbedConfig | null>('agents:getEmbedConfig', {
      agentId,
      publicSecret: secret,
    })
    if (!config) {
      return NextResponse.json({ error: 'not_found' }, { status: 404 })
    }
    // Strip orgId from the response — it's used server-side for the
    // message-send path; not needed by the browser.
    const { orgId: _orgId, ...publicConfig } = config
    return NextResponse.json(publicConfig, {
      headers: {
        // Cache aggressively at the edge — config rarely changes and
        // the widget loads on every page view. Operators rotate the
        // secret to invalidate.
        'Cache-Control': 'public, max-age=300, s-maxage=300',
        // Permissive CORS so the widget can fetch from any host.
        'Access-Control-Allow-Origin': '*',
      },
    })
  } catch {
    return NextResponse.json({ error: 'not_found' }, { status: 404 })
  }
}

export async function OPTIONS(): Promise<Response> {
  return new NextResponse(null, {
    status: 204,
    headers: {
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Methods': 'GET, OPTIONS',
      'Access-Control-Allow-Headers': 'Content-Type',
      'Access-Control-Max-Age': '86400',
    },
  })
}
