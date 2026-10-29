import type { IncomingMessage, ServerResponse } from 'node:http'
import { assertComparisonContext, comparisonQuery, COMPARISON_PATH, COMPARISON_ROW_LIMIT, sanitizeSearchComparison, type ComparisonInput } from './search-comparison-client.ts'

type Boundary = {
  serviceKey: string; upstreamUrl: string
  readLimitedBody: (response: Response, maxBytes: number) => Promise<Buffer>
  registryError: (response: ServerResponse, upstream: Response) => Promise<boolean>
  send: (status: number, code: string, message: string) => void
  limited: () => boolean
}

/** Shares the loopback/same-origin guard and read budget of the Studio proxy. */
export function handleComparisonProxy(request: IncomingMessage, response: ServerResponse, url: URL, boundary: Boundary): boolean {
  if (url.pathname !== COMPARISON_PATH) return false
  if (request.method !== 'GET') { response.setHeader('Allow', 'GET'); boundary.send(405, 'METHOD_NOT_ALLOWED', 'GET is required'); return true }
  let input: ComparisonInput
  try {
    const allowed = ['site', 'websiteId', 'dimension', 'currentStart', 'currentEnd', 'previousStart', 'previousEnd', 'rowLimit']
    if ([...url.searchParams.keys()].some((key) => !allowed.includes(key)) || allowed.some((key) => url.searchParams.getAll(key).length !== 1) || url.searchParams.get('rowLimit') !== String(COMPARISON_ROW_LIMIT)) throw new Error('Invalid query')
    input = { site: url.searchParams.get('site')!, websiteId: url.searchParams.get('websiteId')!, dimension: url.searchParams.get('dimension')! as ComparisonInput['dimension'],
      currentStart: url.searchParams.get('currentStart')!, currentEnd: url.searchParams.get('currentEnd')!, previousStart: url.searchParams.get('previousStart')!, previousEnd: url.searchParams.get('previousEnd')! }
    comparisonQuery(input)
    const now = new Date()
    if (Date.parse(`${input.currentEnd}T00:00:00Z`) >= Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate())) throw new Error('Incomplete period')
  } catch { boundary.send(400, 'INVALID_QUERY', 'Choose a registered website and adjacent completed comparison periods'); return true }
  if (boundary.limited()) { boundary.send(429, 'RATE_LIMITED', 'Too many data requests'); return true }
  const target = new URL(COMPARISON_PATH, boundary.upstreamUrl)
  target.search = comparisonQuery(input)
  void fetch(target, { method: 'GET', redirect: 'manual', signal: AbortSignal.timeout(15_000), headers: { Accept: 'application/json', 'X-Internal-API-Key': boundary.serviceKey } })
    .then(async (upstream) => {
      if (await boundary.registryError(response, upstream)) return
      if (upstream.status !== 200 || !upstream.headers.get('content-type')?.startsWith('application/json')) throw new Error('Source unavailable')
      // At most 100 rows per period; allow Go's six-byte JSON HTML escapes for page keys.
      const bytes = await boundary.readLimitedBody(upstream, 8 << 20)
      const payload: unknown = JSON.parse(bytes.toString('utf8'))
      const data = sanitizeSearchComparison(typeof payload === 'object' && payload !== null && 'data' in payload ? payload.data : undefined)
      assertComparisonContext(data, input)
      response.setHeader('Content-Type', 'application/json; charset=utf-8')
      response.end(JSON.stringify({ data }))
    }).catch(() => boundary.send(502, 'SOURCE_UNAVAILABLE', 'Search comparison is unavailable'))
  return true
}
