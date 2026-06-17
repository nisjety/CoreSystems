import { NextResponse } from 'next/server'

import { resolveChatActor } from '@/app/api/chat/_lib/session-store'
import { convexMutation, convexQuery } from '@/app/api/_lib/convex-client'
import type { PersistedAgent } from '@/components/agents/types'
import { MOCK_AGENTS } from '@/components/agents/data'

/**
 * One-time seed endpoint: creates the mock demo agents in Convex
 * for the authenticated user's org if they don't already exist.
 *
 * Gate: only runs if zero agents exist for the org (idempotent).
 *
 * POST /api/agents/seed
 */
export async function POST() {
  try {
    const actor = await resolveChatActor()

    // Only seed if org has no agents yet
    const existing = await convexQuery<PersistedAgent[]>('agents:listByOrg', {
      orgId: actor.convexOrgId,
    })

    if (existing.length > 0) {
      return NextResponse.json({
        success: true,
        message: `Skipped: org already has ${existing.length} agent(s)`,
        seeded: 0,
      })
    }

    const created: string[] = []
    for (const mock of MOCK_AGENTS) {
      const id = await convexMutation<string>('agents:create', {
        orgId: actor.convexOrgId,
        createdBy: actor.convexUserId,
        name: mock.name,
        description: mock.description,
        useCase: mock.useCase,
        status: mock.status,
        model: mock.model,
        temperature: 0.4,
        systemPrompt: mock.instructionSummary,
        greeting: mock.greeting,
        tools: mock.tools,
      })
      created.push(id)
    }

    return NextResponse.json({
      success: true,
      message: `Seeded ${created.length} demo agent(s)`,
      seeded: created.length,
    })
  } catch (error) {
    return NextResponse.json(
      {
        success: false,
        error: error instanceof Error ? error.message : 'Seed failed',
      },
      { status: 500 },
    )
  }
}
