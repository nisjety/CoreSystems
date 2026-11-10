// Parser and validator for the ```crawl-plan fence that execution-core's knowledge.crawl_plan emits
// (E4.3-055c). The fence body is model-authored JSON, so it is untrusted: every field is validated,
// off-site pages are dropped, and nothing here confers authority. The card that renders a plan asks the
// person to approve, and the crawl itself still starts through the crawl_site action under their token.
// Pure module: no DOM, no network, no storage. Any failure returns ok:false so the caller falls back to
// the plain code block.

import { z } from 'zod'

/** The most pages one plan may list. */
export const CRAWL_PLAN_MAX_PAGES = 50

const MAX_FENCE_CHARS = 64 * 1024
const MAX_URL_CHARS = 2048
const MAX_TITLE_CHARS = 200
const MAX_GOAL_CHARS = 300
// Identical to `crawlSiteInput` in shared/actions/action-registry.ts.
const MAX_SCOPE_PATTERNS = 100
const MAX_SCOPE_PATTERN_CHARS = 512
const MAX_SCOPE_DEPTH = 10

export type CrawlPlan = {
  url: string
  goal?: string
  pages: { url: string; title?: string }[]
  scope: {
    maxPages?: number
    maxDepth?: number
    includePaths?: string[]
    excludePaths?: string[]
    allowSubdomains?: boolean
  }
}

export type CrawlPlanParseResult = { ok: true; plan: CrawlPlan } | { ok: false; reason: string }

/** Whether the crawl-plan card is on for this build. `VITE_CRAWL_PLAN_CARD` counts only as the literal "true". */
export function crawlPlanCardEnabled(): boolean {
  return import.meta.env.VITE_CRAWL_PLAN_CARD === 'true'
}

/** An absolute http(s) URL without credentials. */
function isHttpUrl(value: string): boolean {
  const parsed = parseHttpUrl(value)
  return parsed !== undefined
}

function parseHttpUrl(value: string): URL | undefined {
  let parsed: URL
  try {
    parsed = new URL(value)
  } catch {
    return undefined
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return undefined
  if (parsed.hostname === '') return undefined
  if (parsed.username !== '' || parsed.password !== '') return undefined
  return parsed
}

const httpUrl = z
  .string()
  .trim()
  .min(1)
  .max(MAX_URL_CHARS)
  .refine(isHttpUrl, { message: 'must be an absolute http(s) URL' })

const pathPatterns = z.array(z.string().min(1).max(MAX_SCOPE_PATTERN_CHARS)).max(MAX_SCOPE_PATTERNS)

const crawlPlanSchema = z.object({
  v: z.literal(1),
  url: httpUrl,
  goal: z.string().trim().max(MAX_GOAL_CHARS).optional(),
  pages: z
    .array(
      z.object({
        url: httpUrl,
        title: z.string().trim().max(MAX_TITLE_CHARS).optional(),
      }),
    )
    .min(1)
    .max(CRAWL_PLAN_MAX_PAGES),
  scope: z
    .object({
      maxPages: z.number().int().positive().optional(),
      maxDepth: z.number().int().positive().max(MAX_SCOPE_DEPTH).optional(),
      includePaths: pathPatterns.optional(),
      excludePaths: pathPatterns.optional(),
      allowSubdomains: z.boolean().optional(),
    })
    .optional(),
})

// Second-level labels under a two-letter country code (co.uk, com.au, ...). There is no public suffix
// list in the bundle, so this small heuristic is the registrable-domain rule; a wrong guess can only
// make host pinning stricter or looser by one label, and the crawl itself is pinned again server side.
const SECOND_LEVEL_LABELS = new Set(['co', 'com', 'org', 'net', 'gov', 'edu', 'ac', 'or', 'ne', 'go'])

function normalizedHost(hostname: string): string {
  return hostname.toLowerCase().replace(/\.$/, '')
}

function registrableHost(host: string): string {
  // An IP literal or a single-label host has no registrable parent.
  if (host.includes(':') || /^\d+(\.\d+){3}$/.test(host)) return host
  const labels = host.split('.')
  if (labels.length <= 2) return host
  const tld = labels[labels.length - 1] ?? ''
  const second = labels[labels.length - 2] ?? ''
  const keep = tld.length === 2 && SECOND_LEVEL_LABELS.has(second) ? 3 : 2
  return labels.slice(-keep).join('.')
}

function isSubdomainOf(host: string, parent: string): boolean {
  return host.endsWith(`.${parent}`)
}

/** Whether a page may stay in a plan whose start URL has `planHost`. */
function pageHostAllowed(pageHost: string, planHost: string, allowSubdomains: boolean): boolean {
  if (pageHost === planHost) return true
  if (registrableHost(pageHost) !== registrableHost(planHost)) return false
  if (!allowSubdomains) return false
  // Same registrable domain: a subdomain of the start host, or of the registrable domain itself.
  return isSubdomainOf(pageHost, planHost) || isSubdomainOf(pageHost, registrableHost(planHost))
}

function pageKey(parsed: URL): string {
  const copy = new URL(parsed.href)
  copy.hash = ''
  return copy.href
}

function firstIssue(error: z.ZodError): string {
  const issue = error.issues[0]
  if (!issue) return 'invalid crawl plan'
  const path = issue.path.length > 0 ? issue.path.join('.') : 'plan'
  return `${path}: ${issue.message}`
}

/**
 * Parses the body of a closed ```crawl-plan fence: JSON `{ v: 1, url, goal, pages, scope }`.
 * Unknown keys are stripped; pages on another site are dropped and repeats removed. Returns
 * `ok: false` for anything that is not a well-formed plan with at least one page left.
 */
export function parseCrawlPlanFence(text: string): CrawlPlanParseResult {
  if (typeof text !== 'string') return { ok: false, reason: 'crawl plan is not text' }
  if (text.length > MAX_FENCE_CHARS) return { ok: false, reason: 'crawl plan is too large' }
  const body = text.trim()
  if (body === '') return { ok: false, reason: 'crawl plan is empty' }

  let raw: unknown
  try {
    raw = JSON.parse(body)
  } catch {
    return { ok: false, reason: 'crawl plan is not valid JSON' }
  }

  const checked = crawlPlanSchema.safeParse(raw)
  if (!checked.success) return { ok: false, reason: firstIssue(checked.error) }
  const data = checked.data

  const start = parseHttpUrl(data.url)
  if (!start) return { ok: false, reason: 'url: must be an absolute http(s) URL' }
  const planHost = normalizedHost(start.hostname)
  const allowSubdomains = data.scope?.allowSubdomains === true

  const seen = new Set<string>()
  const pages: CrawlPlan['pages'] = []
  for (const page of data.pages) {
    const parsed = parseHttpUrl(page.url)
    if (!parsed) continue
    if (!pageHostAllowed(normalizedHost(parsed.hostname), planHost, allowSubdomains)) continue
    const key = pageKey(parsed)
    if (seen.has(key)) continue
    seen.add(key)
    pages.push(page.title ? { url: page.url, title: page.title } : { url: page.url })
  }
  if (pages.length === 0) return { ok: false, reason: 'pages: none left on the start site' }

  const scope: CrawlPlan['scope'] = {}
  const source = data.scope
  if (source) {
    if (source.maxPages !== undefined) scope.maxPages = source.maxPages
    if (source.maxDepth !== undefined) scope.maxDepth = source.maxDepth
    if (source.includePaths !== undefined) scope.includePaths = source.includePaths
    if (source.excludePaths !== undefined) scope.excludePaths = source.excludePaths
    if (source.allowSubdomains !== undefined) scope.allowSubdomains = source.allowSubdomains
  }

  const plan: CrawlPlan = { url: data.url, pages, scope }
  if (data.goal) plan.goal = data.goal
  return { ok: true, plan }
}
