import { NextRequest } from 'next/server'

import { resolveChatActor } from '../chat/_lib/session-store'
import { convexMutation, convexQuery } from '../_lib/convex-client'

// U2-14 follow-up (verevon ui-ux-verevon-gap.md §10): real Projects backend.
//
// Backs the "Add to project" picker in `ChatSettingsModal`. Previously
// the picker rendered `SAMPLE_PROJECTS = [{title:'How to use Aquatiq'},
// {title:'Product Development'}]` — pure stub. Now it lists real
// per-org projects from Convex (`projects` table, see
// apps/Application Plane/convex-core/convex/projects.ts).
//
// Endpoints:
//   GET  /api/projects                 → list active projects for the org
//   POST /api/projects { title, description? } → create
//
// Both gate on the session actor; the active org id is `actor.orgId`.

export interface ProjectRow {
  id: string
  title: string
  description?: string
  createdBy: string
  color?: string
  archived: boolean
  createdAt: number
  updatedAt: number
}

interface ProjectsListResponse {
  projects: ProjectRow[]
}

interface CreateProjectBody {
  title?: string
  description?: string
  color?: string
}

interface ConvexProjectDoc {
  _id: string
  title: string
  description?: string
  createdBy: string
  color?: string
  archived: boolean
  createdAt: number
  updatedAt: number
}

function normalise(doc: ConvexProjectDoc): ProjectRow {
  return {
    id: doc._id,
    title: doc.title,
    description: doc.description,
    createdBy: doc.createdBy,
    color: doc.color,
    archived: doc.archived,
    createdAt: doc.createdAt,
    updatedAt: doc.updatedAt,
  }
}

export async function GET(_request: NextRequest): Promise<Response> {
  try {
    const actor = await resolveChatActor()
    if (!actor.orgId) {
      return Response.json({ projects: [] } satisfies ProjectsListResponse, {
        status: 200,
      })
    }

    const docs = await convexQuery<ConvexProjectDoc[]>('projects:listByOrg', {
      externalOrgId: actor.orgId,
    })

    return Response.json(
      { projects: (docs ?? []).map(normalise) } satisfies ProjectsListResponse,
      { status: 200 },
    )
  } catch (error: unknown) {
    return Response.json(
      {
        projects: [],
        error: error instanceof Error ? error.message : 'unknown',
      },
      { status: 200 },
    )
  }
}

export async function POST(request: NextRequest): Promise<Response> {
  try {
    const actor = await resolveChatActor()
    if (!actor.orgId || !actor.userId) {
      return Response.json(
        { error: 'No active org/user in session' },
        { status: 400 },
      )
    }

    const body = (await request.json()) as CreateProjectBody
    const title = (body.title ?? '').trim()
    if (!title) {
      return Response.json(
        { error: 'title required' },
        { status: 400 },
      )
    }
    if (title.length > 128) {
      return Response.json(
        { error: 'title too long (max 128 characters)' },
        { status: 413 },
      )
    }

    const result = await convexMutation<{ id: string }>('projects:create', {
      externalOrgId: actor.orgId,
      title,
      description: body.description?.trim() || undefined,
      createdBy: actor.userId,
      color: body.color,
    })

    return Response.json({ id: result.id, title }, { status: 201 })
  } catch (error: unknown) {
    return Response.json(
      {
        error: 'create_failed',
        detail: error instanceof Error ? error.message : 'unknown',
      },
      { status: 500 },
    )
  }
}
