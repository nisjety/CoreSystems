/**
 * Phase A · A3 — /api/admin/health proxy.
 *
 * Fans out to every plane's `/healthz` (or `/health`) endpoint over the
 * `inter-plane-bus` network and returns the rolled-up status to the
 * `/admin/system-health` page. Each probe times out at 3s so a single
 * unreachable plane never wedges the dashboard request.
 *
 * Auth: requires a Better Auth session AND the caller's role is
 * resolved via the existing control-plane-auth helper. The
 * AdminLayout already gates the route to authenticated users — once
 * the proper `role === 'admin'` check from the layout TODO lands,
 * non-admins will hit a 403 before reaching here.
 *
 * Once Wave 3 enforce mode is live, this proxy will also forward a
 * `control-plane`-audience JWT minted via `mintPlaneToken` so each
 * plane can include the org in its own audit trail.
 */

import { NextRequest, NextResponse } from 'next/server'
import {
  authErrorResponse,
  requireSession,
} from '../../_lib/control-plane-auth'

type ProbeStatus = 'healthy' | 'degraded' | 'down' | 'unknown'

interface PlaneProbe {
  plane: string
  service: string
  url: string
  status: ProbeStatus
  http_status?: number
  latency_ms?: number
  error?: string
}

interface PlaneTarget {
  plane: string
  service: string
  url: string
}

const PROBE_TIMEOUT_MS = 3_000

/**
 * One row per cross-plane HTTP healthcheck verevon cares about. Targets
 * use the `inter-plane-bus` DNS names (the network verevon's compose
 * already joins). For services that live on plane-private networks
 * (e.g. Model Plane's `model-gateway`), the probe goes via the
 * host-port mapping documented in `apps/Model Plane/docs/gap-model.md`.
 */
const TARGETS: ReadonlyArray<PlaneTarget> = [
  // Application Plane
  { plane: 'application', service: 'convex-backend',  url: 'http://convex-backend:3210/version' },
  { plane: 'application', service: 'affine-runtime',  url: 'http://affine-runtime:3010/info' },
  { plane: 'application', service: 'notification-core', url: 'http://notification-core:3140/healthz' },
  // Data Plane v2
  { plane: 'data',        service: 'documents-api',   url: 'http://dpv2-documents-api:8010/readyz' },
  { plane: 'data',        service: 'retrieval-engine',url: 'http://dpv2-retrieval-engine:8004/readyz' },
  { plane: 'data',        service: 'index-engine',    url: 'http://dpv2-index-engine:9201/readyz' },
  { plane: 'data',        service: 'embedding-engine',url: 'http://dpv2-embedding-engine:9202/readyz' },
  { plane: 'data',        service: 'graph-index',     url: 'http://dpv2-graph-index:9203/readyz' },
  { plane: 'data',        service: 'wiki-store',      url: 'http://dpv2-wiki-store:8011/readyz' },
  { plane: 'data',        service: 'data-orchestrator', url: 'http://dpv2-data-orchestrator:8012/readyz' },
  // Ingestion Plane
  { plane: 'ingestion',   service: 'quarry-edge',     url: 'http://quarry-edge:8082/health' },
  { plane: 'ingestion',   service: 'quarry-control',  url: 'http://quarry-control:8081/healthz' },
  { plane: 'ingestion',   service: 'integration-api', url: 'http://integration-api:3026/health' },
  { plane: 'ingestion',   service: 'finspo-api',      url: 'http://finspo-api:3130/health' },
  // Control Plane
  { plane: 'control',     service: 'auth-service',    url: 'http://auth-service:3011/api/health' },
  { plane: 'control',     service: 'user-service',    url: 'http://user-service:3012/healthz' },
  { plane: 'control',     service: 'org-core',        url: 'http://org-core-service:8080/healthz' },
  { plane: 'control',     service: 'billing-core',    url: 'http://billing-core-service:3014/healthz' },
  { plane: 'control',     service: 'audit-core',      url: 'http://audit-core-service:8187/healthz' },
]

export async function GET(request: NextRequest): Promise<NextResponse> {
  try {
    await requireSession(request)
  } catch (error) {
    return authErrorResponse(error)
  }

  const probes = await Promise.all(TARGETS.map(probeOne))
  const summary = probes.reduce(
    (acc, p) => {
      acc[p.status] = (acc[p.status] ?? 0) + 1
      return acc
    },
    {} as Record<ProbeStatus, number>,
  )

  return NextResponse.json({
    data: probes,
    meta: {
      total: probes.length,
      healthy: summary.healthy ?? 0,
      degraded: summary.degraded ?? 0,
      down: summary.down ?? 0,
      unknown: summary.unknown ?? 0,
      checked_at: new Date().toISOString(),
    },
    error: null,
  })
}

async function probeOne(target: PlaneTarget): Promise<PlaneProbe> {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS)
  const started = Date.now()
  try {
    const res = await fetch(target.url, {
      method: 'GET',
      cache: 'no-store',
      signal: controller.signal,
    })
    return {
      ...target,
      status: classify(res.status),
      http_status: res.status,
      latency_ms: Date.now() - started,
    }
  } catch (err: unknown) {
    return {
      ...target,
      status: 'down',
      latency_ms: Date.now() - started,
      error: err instanceof Error ? err.message : String(err),
    }
  } finally {
    clearTimeout(timer)
  }
}

function classify(httpStatus: number): ProbeStatus {
  if (httpStatus >= 200 && httpStatus < 300) return 'healthy'
  if (httpStatus >= 500) return 'down'
  return 'degraded'
}
