import { describe, expect, it } from 'vitest'
import { cronPresets, describeCron } from '@/features/ingestions/lib/cron'
import { changeCount, changeTimeline, shortFingerprint } from '@/features/ingestions/lib/monitoring'
import {
  agentInput,
  buildActions,
  codeSnippet,
  crawlRequestFromState,
  defaultPlaygroundState,
  missingInput,
  newAction,
  parseSchema,
  quarryRequestFor,
  scrapeRequestFromState,
  searchFiltersFromState,
  type PlaygroundState,
} from '@/features/ingestions/lib/playground'
import { countRunsByGroup, filterRuns, formatDuration, runDurationMs, runStatusGroup } from '@/features/ingestions/lib/runs'
import type { RunItem } from '@/shared/api/ingestions-client'

const tr = (no: string) => no
const state = (patch: Partial<PlaygroundState>): PlaygroundState => ({ ...defaultPlaygroundState, ...patch })

function run(patch: Partial<RunItem>): RunItem {
  return {
    id: 'job_1',
    kind: 'crawl',
    status: 'completed',
    createdAt: '2026-10-28T10:00:00Z',
    target: 'https://example.com',
    progress: {},
    stats: {},
    ...patch,
  }
}

describe('playground requests', () => {
  it('builds a scrape body with markdown always included and screenshot as a capture', () => {
    const body = scrapeRequestFromState(state({
      url: ' https://example.com ',
      formats: ['links', 'screenshot'],
      includeTags: 'main, article',
      waitMs: '1500',
    }))
    expect(body).toEqual({
      url: 'https://example.com',
      formats: ['markdown', 'links'],
      only_main_content: true,
      include_tags: ['main', 'article'],
      render: { waitForTimeoutMs: 1500 },
      capture: { screenshot: true },
    })
  })

  it('shows the same scrape in Quarry terms, screenshot as a format', () => {
    const request = quarryRequestFor(state({ url: 'https://example.com', formats: ['markdown', 'screenshot'] }))
    expect(request.path).toBe('/v1/scrape')
    expect(request.body.formats).toEqual(['markdown', 'screenshot'])
    expect(request.body).not.toHaveProperty('capture')
  })

  it('clamps crawl limits and drops empty scope fields', () => {
    expect(crawlRequestFromState(state({ endpoint: 'crawl', url: 'https://a.no', maxPages: '500', maxDepth: '' }))).toEqual({
      url: 'https://a.no',
      maxPages: 50,
      ingest: false,
    })
    expect(quarryRequestFor(state({ endpoint: 'crawl', url: 'https://a.no', maxDepth: '3', excludePaths: '/blog/*' })).body).toEqual({
      url: 'https://a.no',
      max_pages: 10,
      max_depth: 3,
      exclude_paths: ['/blog/*'],
    })
  })

  it('names what is missing per endpoint', () => {
    expect(missingInput(state({ endpoint: 'search' }))).toBe('query')
    expect(missingInput(state({ endpoint: 'batch', urls: ' , ' }))).toBe('urls')
    expect(missingInput(state({ endpoint: 'extract', url: 'https://a.no' }))).toBeNull()
    expect(missingInput(state({ endpoint: 'agent', url: 'https://a.no' }))).toBe('goal')
    expect(missingInput(state({ endpoint: 'map', url: 'https://a.no' }))).toBeNull()
  })

  it('renders snippets in all three languages from the same request', () => {
    const request = quarryRequestFor(state({ endpoint: 'map', url: 'https://a.no', allowSubdomains: true }))
    expect(codeSnippet('curl', request)).toContain('"$QUARRY_URL/v1/map"')
    expect(codeSnippet('javascript', request)).toContain('"include_subdomains": true')
    const python = codeSnippet('python', request)
    expect(python).toContain('"include_subdomains": True')
    expect(python).not.toContain('true')
  })
})

describe('run bookkeeping', () => {
  it('groups statuses into Apify-style families', () => {
    expect(runStatusGroup('Running')).toBe('active')
    expect(runStatusGroup('done')).toBe('succeeded')
    expect(runStatusGroup('timed_out')).toBe('failed')
    expect(runStatusGroup('paused')).toBe('other')
  })

  it('derives a duration only when the timestamps can tell', () => {
    expect(runDurationMs(run({ startedAt: '2026-10-28T10:00:00Z', completedAt: '2026-10-28T10:01:05Z' }))).toBe(65_000)
    expect(runDurationMs(run({ completedAt: null }))).toBeNull()
    const now = Date.parse('2026-10-28T10:00:30Z')
    expect(runDurationMs(run({ status: 'running' }), now)).toBe(30_000)
    expect(formatDuration(65_000, tr)).toBe('1 min 5 s')
    expect(formatDuration(3_720_000, tr)).toBe('1 t 2 min')
    expect(formatDuration(null, tr)).toBe('—')
  })

  it('filters by status family, kind and text', () => {
    const runs = [
      run({ id: 'a', status: 'failed', kind: 'batch', target: '3 URLs' }),
      run({ id: 'b', status: 'running', target: 'https://docs.example.com' }),
      run({ id: 'c' }),
    ]
    expect(filterRuns(runs, { status: 'failed', kind: 'all', text: '' }).map((r) => r.id)).toEqual(['a'])
    expect(filterRuns(runs, { status: 'all', kind: 'crawl', text: 'docs' }).map((r) => r.id)).toEqual(['b'])
    expect(countRunsByGroup(runs)).toEqual({ active: 1, succeeded: 1, failed: 1, other: 0 })
  })
})

describe('cron', () => {
  it('reads the preset shapes in plain language', () => {
    expect(cronPresets.map((preset) => describeCron(preset.cron, tr))).toEqual([
      'Hver time, på hel time',
      'Hver dag kl. 07:00 UTC',
      'Hverdager kl. 07:00 UTC',
      'Hver mandag kl. 07:00 UTC',
      'Den 1. hver måned kl. 07:00 UTC',
    ])
    expect(describeCron('*/15 * * * *', tr)).toBe('Hvert 15. minutt')
    expect(describeCron('5 4 * 2 *', tr)).toBeNull()
    expect(describeCron('not cron', tr)).toBeNull()
  })
})

describe('change timeline', () => {
  it('marks a change when the fingerprint differs from the next older baseline', () => {
    const base = { orgId: 'o', sourceUrl: 'https://a.no', capturedAt: '2026-10-28T10:00:00Z' }
    const timeline = changeTimeline([
      { ...base, baselineId: '3', fingerprint: 'blake3:bbb' },
      { ...base, baselineId: '2', fingerprint: 'blake3:aaa' },
      { ...base, baselineId: '1', fingerprint: 'blake3:aaa' },
    ])
    expect(timeline.map((entry) => entry.change)).toEqual(['changed', 'unchanged', 'first'])
    expect(changeCount(timeline)).toBe(1)
    expect(shortFingerprint('blake3:deadbeefcafe0123456789')).toBe('blake3:deadbeefcafe…')
  })
})

describe('browser steps, schemas and search filters', () => {
  it('builds the Quarry steps from the form and names the first incomplete one', () => {
    const built = buildActions([
      newAction('wait'),
      { ...newAction('click'), selector: '#go' },
      { ...newAction('scroll'), scroll: 'bottom' },
      { ...newAction('write'), selector: '#q', text: 'rust' },
      { ...newAction('press'), key: 'Enter' },
    ])
    expect(built).toEqual({
      ok: true,
      actions: [
        { type: 'wait', milliseconds: 1000 },
        { type: 'click', selector: '#go' },
        { type: 'scroll', to: 'bottom' },
        { type: 'write', selector: '#q', text: 'rust' },
        { type: 'press', key: 'Enter' },
      ],
    })
    expect(buildActions([newAction('click')])).toEqual({ ok: false, index: 0, field: 'selector' })
    expect(buildActions([{ ...newAction('write'), selector: '#q', text: '' }])).toEqual({ ok: false, index: 0, field: 'text' })
  })

  it('sends the steps and their error policy with a scrape, and nothing when there are none', () => {
    const body = scrapeRequestFromState(state({
      url: 'https://a.no',
      actions: [{ ...newAction('click'), selector: '#more' }],
      actionsOnError: 'continue',
    }))
    expect(body).toMatchObject({ actions: [{ type: 'click', selector: '#more' }], actions_on_error: 'continue' })
    expect(scrapeRequestFromState(state({ url: 'https://a.no' }))).not.toHaveProperty('actions')
  })

  it('reads the extraction schema as a JSON object or nothing', () => {
    expect(parseSchema('')).toEqual({ ok: true, schema: null })
    expect(parseSchema('{"type":"object"}')).toEqual({ ok: true, schema: { type: 'object' } })
    expect(parseSchema('[1]')).toEqual({ ok: false, reason: 'not_object' })
    expect(parseSchema('{bad')).toEqual({ ok: false, reason: 'invalid_json' })
  })

  it('forwards the schema on extract and leaves it out when there is none', () => {
    expect(quarryRequestFor(state({ endpoint: 'extract', url: 'https://a.no', schema: '{"type":"object"}' })).body).toEqual({
      urls: ['https://a.no'],
      schema: { type: 'object' },
    })
    expect(quarryRequestFor(state({ endpoint: 'extract', url: 'https://a.no' })).body).toEqual({ urls: ['https://a.no'] })
  })

  it('turns the search filters into the gateway filters, blanks dropped', () => {
    expect(searchFiltersFromState(state({
      searchCountry: 'no',
      searchLanguage: 'NO',
      searchTimeRange: 'week',
      searchIncludeDomains: 'a.no, b.no',
    }))).toEqual({
      timeRange: 'week',
      topic: null,
      exactMatch: false,
      includeDomains: ['a.no', 'b.no'],
      excludeDomains: [],
      country: 'NO',
      language: 'no',
    })
  })

  it('builds the agent input from the goal, limits and domains, and says it is a Verevon call', () => {
    const input = agentInput(state({ url: 'https://a.no', goal: ' Find it ', maxSteps: '12', allowedDomains: 'a.no' }))
    expect(input).toEqual({ url: 'https://a.no', goal: 'Find it', maxSteps: 12, allowedDomains: ['a.no'] })
    const request = quarryRequestFor(state({ endpoint: 'agent', url: 'https://a.no', goal: 'Find it' }))
    expect(request.path).toBe('/api/v1/actions/execute')
    expect(request.body).toMatchObject({ actionId: 'ingestions.start_agent_run' })
    expect(request.note).toContain('Verevon')
  })
})
