import { NextRequest } from 'next/server'

import { resolveChatActor } from '../chat/_lib/session-store'

// U2-2 / U3-2 (ui-ux-verevon-gap.md §10): Live model registry proxy.
//
// Replaces the previous hardcoded `gpt-5.4-mini` defaults in the chat
// ModelSelector + agents page with a real catalog fetched from
// capability-core's `/api/v1/capabilities?kind=model` endpoint.
//
// capability-core is the Go service that owns the cross-org capability
// registry — including the `models` table seeded by migration
// `0002_seed_models.up.sql` and surfaced via `ListAsCapabilities()`.
// Filters: `kind=model` + `enabled=true` so only deployed / live models
// reach the UI.
//
// Response shape (envelope kept stable for the UI):
//   {
//     status: 'ok' | 'service_unavailable',
//     models: [
//       {
//         id:         <capability id>,
//         provider:   'openai' | 'azure' | 'anthropic' | 'google' | …,
//         name:       <model name, e.g. gpt-4o-mini>,
//         version:    <model version>,
//         label:      <pretty UI label>,
//         tier:       'low' | 'high',           // derived from risk_level
//         enabled:    true,
//         description?: string,
//       }, …
//     ],
//     detail?: <error string>,
//   }
//
// Verevon callers (ModelSelector, agents/data.ts) fall back to a static
// list if `status === 'service_unavailable'` so the UI stays functional
// when capability-core is down.

const CAPABILITY_CORE_URL =
  process.env.CAPABILITY_CORE_HTTP_URL ??
  process.env.CAPABILITY_CORE_URL ??
  'http://capability-core:8085'

interface CapabilityCoreModel {
  id: string
  org_id?: string
  scope?: string
  provider: string
  name: string
  version?: string
  config_json?: Record<string, unknown>
  enabled?: boolean
  risk_level?: string
  description?: string
  // ListAsCapabilities() shape — fields may also appear under nested keys
  kind?: string
  label?: string
}

interface ModelEntry {
  id: string
  provider: string
  name: string
  version: string
  label: string
  tier: 'low' | 'high'
  enabled: boolean
  description?: string
}

interface ModelsListResponse {
  status: 'ok' | 'service_unavailable'
  models: ModelEntry[]
  detail?: string
}

function deriveTier(risk: string | undefined): 'low' | 'high' {
  // capability-core's `risk_level` is the closest proxy for capability tier
  // available in the registry today. `low` → fast/cheap models; everything
  // else (medium/high) → expensive/slow.
  return (risk ?? 'low').toLowerCase() === 'low' ? 'low' : 'high'
}

function prettyLabel(name: string, version: string | undefined): string {
  // Capitalised + dash-separated form for the picker. The full version is
  // dropped from the label because the registry typically reuses the same
  // version for a long time — adding it would bloat the dropdown.
  const base = name
    .split(/[-_]/)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ')
  if (!version) return base
  return base
}

function normaliseModel(raw: CapabilityCoreModel): ModelEntry {
  return {
    id: raw.id,
    provider: (raw.provider ?? '').toLowerCase(),
    name: raw.name,
    version: raw.version ?? '',
    label: raw.label ?? prettyLabel(raw.name, raw.version),
    tier: deriveTier(raw.risk_level),
    enabled: raw.enabled !== false,
    description: raw.description,
  }
}

export async function GET(_request: NextRequest): Promise<Response> {
  try {
    const actor = await resolveChatActor()
    const url = new URL(`${CAPABILITY_CORE_URL}/api/v1/capabilities`)
    url.searchParams.set('kind', 'model')
    url.searchParams.set('enabled', 'true')
    if (actor.orgId) {
      url.searchParams.set('org_id', actor.orgId)
    }

    const upstreamRes = await fetch(url.toString(), {
      method: 'GET',
      signal: AbortSignal.timeout(5_000),
      cache: 'no-store',
    })

    if (!upstreamRes.ok) {
      const detail = await upstreamRes.text().catch(() => '')
      return Response.json(
        {
          status: 'service_unavailable',
          models: [],
          detail: `capability-core returned ${upstreamRes.status}: ${detail.slice(0, 200)}`,
        } satisfies ModelsListResponse,
        { status: 200 },
      )
    }

    // capability-core returns `{ capabilities: [...], count }` per
    // `capabilities.go::list`. The same handler accepts `?kind=model`
    // and returns those entries with provider + name + version columns.
    const raw = (await upstreamRes.json()) as {
      capabilities?: CapabilityCoreModel[]
      count?: number
    } | null

    const models = (raw?.capabilities ?? [])
      .filter((m) => m.kind === 'model' || m.kind === undefined)
      .map(normaliseModel)
      .filter((m) => m.enabled)

    return Response.json(
      { status: 'ok', models } satisfies ModelsListResponse,
      { status: 200 },
    )
  } catch (error: unknown) {
    return Response.json(
      {
        status: 'service_unavailable',
        models: [],
        detail: error instanceof Error ? error.message : 'unknown',
      } satisfies ModelsListResponse,
      { status: 200 },
    )
  }
}
