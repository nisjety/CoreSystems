import { NextRequest } from 'next/server'

import { resolveChatActor } from '../chat/_lib/session-store'

// U2-14 (ui-ux-velion-gap.md §10 + Option A consolidation): Skills proxy
// now hits the Model Plane v1 Go `capability-core` service at
// `/api/v1/skills` instead of agent-core-v2 (v2 Python).
//
// capability-core stores the per-org agent_skills table in the v1 stack's
// `session_core` Postgres DB. Schema mirrors agent-core-v2's table (see
// migration 0006_agent_skills.up.sql).
//
// Wire shape kept identical to the v2 contract so the chat UI's
// SkillsList component stays as-is.

const CAPABILITY_CORE_URL =
  process.env.CAPABILITY_CORE_HTTP_URL ??
  process.env.CAPABILITY_CORE_URL ??
  'http://capability-core:8085'

interface CapabilityCoreSkill {
  id: string
  org_id: string
  name: string
  description: string
  content?: string
  trigger_keywords?: string[]
  trigger_file_patterns?: string[]
  tool_restrictions?: string[]
  enabled: boolean
  created_at: string
  updated_at: string
}

interface SkillsListResponse {
  skills: CapabilityCoreSkill[]
  status: 'ok' | 'service_unavailable'
  detail?: string
}

export async function GET(_request: NextRequest): Promise<Response> {
  try {
    const actor = await resolveChatActor()
    if (!actor.orgId) {
      return Response.json(
        { skills: [], status: 'ok' } satisfies SkillsListResponse,
        { status: 200 },
      )
    }

    const url = new URL(`${CAPABILITY_CORE_URL}/api/v1/skills`)
    url.searchParams.set('org_id', actor.orgId)

    const upstreamRes = await fetch(url.toString(), {
      method: 'GET',
      signal: AbortSignal.timeout(5_000),
      cache: 'no-store',
    })

    if (!upstreamRes.ok) {
      const detail = await upstreamRes.text().catch(() => '')
      return Response.json(
        {
          skills: [],
          status: 'service_unavailable',
          detail: `capability-core returned ${upstreamRes.status}: ${detail.slice(0, 200)}`,
        } satisfies SkillsListResponse,
        { status: 200 },
      )
    }

    // capability-core returns either an array or `{ skills: [...] }` depending
    // on the codepath. Normalize to the envelope shape.
    const raw = (await upstreamRes.json()) as
      | CapabilityCoreSkill[]
      | { skills?: CapabilityCoreSkill[] }
      | null
    const skills: CapabilityCoreSkill[] = Array.isArray(raw)
      ? raw
      : (raw?.skills ?? [])

    return Response.json(
      {
        skills: skills.filter((s) => s.enabled !== false),
        status: 'ok',
      } satisfies SkillsListResponse,
      { status: 200 },
    )
  } catch (error: unknown) {
    return Response.json(
      {
        skills: [],
        status: 'service_unavailable',
        detail: error instanceof Error ? error.message : 'unknown',
      } satisfies SkillsListResponse,
      { status: 200 },
    )
  }
}
