// @vitest-environment jsdom
import { fireEvent, render, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// The workspace lookup is the only thing mocked: the scrape preview goes through
// the real knowledge client and the real transport, so the test covers the whole
// path from a gateway refusal to the sentence on the screen.
vi.mock('@/shared/api/auth-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/auth-client')>()),
  getAuthSession: vi.fn(async () => ({ user: { id: 'user-1' } })),
  getSessionContext: vi.fn(async () => ({ orgs: [{ id: 'org-1' }] })),
}))

import { BLOCK_REASON_COPY, ERROR_COPY } from '@/shared/i18n/errors'
import { KnowledgeComposer } from './KnowledgeComposer'

const RAW_GATEWAY_MESSAGE = 'Quarry refused https://secret.example/private (org_render_cap) at postgres://db.internal'

function json(status: number, body: unknown, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json', ...headers }, status })
}

/** Answer the scrape preview with `scrape`, and everything else the composer asks
 * for on mount (profiles, jobs, a browser session) with a plain 404. */
function gatewayAnswers(scrape: Response) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url
      return url.includes('/api/v1/knowledge/scrape-preview')
        ? scrape.clone()
        : json(404, { error: { code: 'not_found', message: 'not found' } })
    }),
  )
}

async function failedScrape(scrape: Response): Promise<string> {
  gatewayAnswers(scrape)
  const view = render(() => <KnowledgeComposer />)
  fireEvent.click([...view.container.querySelectorAll<HTMLButtonElement>('.dashboard-knowledge-composer__modes button')].find(button => button.textContent?.includes('Extract'))!)
  const input = await waitFor(() => {
    const field = view.container.querySelector<HTMLInputElement>('.dashboard-knowledge-composer__input')
    expect(field).not.toBeNull()
    expect(field!.disabled).toBe(false)
    return field!
  })
  fireEvent.input(input, { target: { value: 'https://example.com/page' } })
  const form = view.container.querySelector('form')!
  await waitFor(() => expect(view.container.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(false))
  fireEvent.submit(form)
  const shown = await waitFor(() => {
    const notice = view.container.querySelector('.dashboard-knowledge-composer__error')
    expect(notice).not.toBeNull()
    return notice!.textContent ?? ''
  })
  // Whatever the case, no raw gateway or upstream text reaches the person.
  expect(shown).not.toMatch(/secret\.example|postgres|db\.internal|org_render_cap|Quarry|CHALLENGE_DETECTED/)
  expect(shown).not.toMatch(/\b[a-z]+(?:_[a-z]+)+\b/)
  return shown
}

describe('KnowledgeComposer scrape preview failures', () => {
  beforeEach(() => {
    try {
      window.localStorage.clear()
    } catch {
      /* storage is optional */
    }
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('shows the bot-wall sentence for site_blocked with a known reason, not the gateway message', async () => {
    const shown = await failedScrape(
      json(403, { error: { code: 'site_blocked', message: RAW_GATEWAY_MESSAGE, blockReason: 'captcha_widget' } }),
    )
    expect(shown).toBe(BLOCK_REASON_COPY.captcha_widget!.no)
  })

  it('falls back to the general site-blocked sentence for a reason it does not know', async () => {
    const shown = await failedScrape(
      json(403, { error: { code: 'site_blocked', message: RAW_GATEWAY_MESSAGE, blockReason: 'a_reason_nobody_mapped' } }),
    )
    expect(shown).toBe(ERROR_COPY.site_blocked!.no)
  })

  it('shows the wait sentence with the gateway retry hint for scrape_rate_limited', async () => {
    const shown = await failedScrape(
      json(429, { error: { code: 'scrape_rate_limited', message: RAW_GATEWAY_MESSAGE, retryAfterSeconds: 17 } }, { 'Retry-After': '17' }),
    )
    expect(shown).toBe(`${ERROR_COPY.scrape_rate_limited!.no} Prøv igjen om 17 sekunder.`)
  })

  it('shows its own sentence for a refusal and for an address the gateway rejected', async () => {
    expect(await failedScrape(json(403, { error: { code: 'scrape_refused', message: RAW_GATEWAY_MESSAGE } }))).toBe(
      ERROR_COPY.scrape_refused!.no,
    )
    expect(await failedScrape(json(400, { error: { code: 'invalid_url', message: RAW_GATEWAY_MESSAGE } }))).toBe(
      ERROR_COPY.invalid_url!.no,
    )
  })

  it('shows the caller fallback, not the response text, for an unknown code', async () => {
    const shown = await failedScrape(json(500, { error: { code: 'brand_new_failure', message: RAW_GATEWAY_MESSAGE } }))
    expect(shown).toBe('Skraping kunne ikke fullføres.')
  })
})
