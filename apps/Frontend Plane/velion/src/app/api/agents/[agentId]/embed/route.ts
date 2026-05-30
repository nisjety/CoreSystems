import { NextRequest, NextResponse } from 'next/server'

import { resolveChatActor } from '@/app/api/chat/_lib/session-store'
import { convexMutation } from '@/app/api/_lib/convex-client'

/**
 * Wave 9 (ui-ux-velion-gap.md §19): authenticated embed-config proxy.
 *
 * POST   /api/agents/{agentId}/embed  → enables (or rotates secret).
 *                                       Body: { theme?: {...} }
 * DELETE /api/agents/{agentId}/embed  → disables (drops secret too).
 *
 * Both call Convex mutations directly using the internal service key —
 * this is a normal authenticated agent-config endpoint, not the public
 * embed surface (which lives at /api/embed/...).
 */

interface ThemeInput {
  accentColor?: string
  buttonLabel?: string
  welcomeMessage?: string
}

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ agentId: string }> },
): Promise<Response> {
  try {
    const { agentId } = await params
    const actor = await resolveChatActor()
    const body = (await request.json().catch(() => ({}))) as { theme?: ThemeInput }
    const result = await convexMutation<{ publicSecret: string }>(
      'agents:enablePublicEmbed',
      {
        agentId,
        orgId: actor.convexOrgId,
        theme: body.theme,
      },
    )
    return NextResponse.json(result)
  } catch (e: unknown) {
    return NextResponse.json(
      { error: e instanceof Error ? e.message : 'enable failed' },
      { status: 502 },
    )
  }
}

export async function DELETE(
  _request: NextRequest,
  { params }: { params: Promise<{ agentId: string }> },
): Promise<Response> {
  try {
    const { agentId } = await params
    const actor = await resolveChatActor()
    await convexMutation('agents:disablePublicEmbed', {
      agentId,
      orgId: actor.convexOrgId,
    })
    return NextResponse.json({ ok: true })
  } catch (e: unknown) {
    return NextResponse.json(
      { error: e instanceof Error ? e.message : 'disable failed' },
      { status: 502 },
    )
  }
}
