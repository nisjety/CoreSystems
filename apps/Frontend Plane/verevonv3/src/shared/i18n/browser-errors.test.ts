import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import { SCRAPE_PREVIEW_FAILURE_CODES } from '@/shared/api/knowledge-client'
import { BLOCK_REASON_COPY, ERROR_COPY, describeApiError, translateApiError } from './errors'

const no = (norwegian: string) => norwegian
const en = (_norwegian: string, english: string) => english

// Every code a browser session, an agent run or a scrape preview can fail with
// that the person at the screen can act on: the gateway's own codes plus the raw
// Quarry codes a passthrough body may still carry.
const BROWSER_ERROR_CODES = [
  'browser_session_cap',
  'browser_capacity_reached',
  'browser_page_cap',
  'browser_tab_limit_reached',
  'browser_session_grant_inactive',
  'browser_session_grant_missing',
  'browser_session_grant_expired',
  'browser_grant_not_derivable',
  'browser_host_not_grantable',
  'browser_grant_denied',
  'browser_grant_limit_reached',
  'browser_grant_unavailable',
  'browser_process_lost',
  'browser_session_reaped',
  'session_reaped',
  'browser_unhealthy',
  'browser_action_denied',
  'browser_artifact_withheld',
  'browser_artifact_missing',
  'browser_human_takeover_active',
  'USER_TOOK_OVER',
  'browser_site_blocked',
  'challenge_detected',
  'CHALLENGE_DETECTED',
  'site_blocked',
  'scrape_refused',
  'scrape_rate_limited',
  'scrape_timeout',
  'rate_limited',
  'unauthorized',
  // The refusals of a keyed AI-run start (the SPA sends an Idempotency-Key now).
  'invalid_idempotency_key',
  'browser_idempotency_key_reused',
  'browser_idempotency_key_run_ended',
  'browser_idempotency_unverified',
  // Refusals the gateway answers before any upstream call, and its agent proxy's outage.
  'invalid_url',
  'invalid_crawl_request',
  'invalid_browser_action',
  'zdr_persistent_profile_forbidden',
  'model_gateway_unreachable',
] as const

const BLOCK_REASONS = ['captcha_widget', 'waf_interstitial', 'unexpected_bot_error', 'challenge_loop'] as const

// An internal id in a sentence: snake_case or UPPER_SNAKE words, plus the names
// of services and mechanisms a person has no use for.
const RAW_ID = /\b[a-z][a-z0-9]*(?:_[a-z0-9]+)+\b|\b[A-Z]{2,}(?:_[A-Z0-9]+)+\b/
const INTERNAL_WORD = /quarry|hbbs|hbbr|grant|gateway|model plane|control plane|json|https?:|\b[45]\d\d\b/i

function copyOf(table: Record<string, { no: string; en: string }>, key: string): { no: string; en: string } {
  const copy = table[key]
  if (!copy) throw new Error(`no copy for ${key}`)
  return copy
}

function expectEndUserCopy(label: string, copy: { no: string; en: string } | undefined) {
  expect(copy, `${label} has copy`).toBeDefined()
  for (const text of [copy!.no, copy!.en]) {
    expect(text.trim().length, `${label}: non-empty`).toBeGreaterThan(15)
    expect(text, `${label}: no raw id`).not.toMatch(RAW_ID)
    expect(text, `${label}: no internal word`).not.toMatch(INTERNAL_WORD)
    expect(text.endsWith('.'), `${label}: ends as a sentence`).toBe(true)
  }
  expect(copy!.no, `${label}: the two languages differ`).not.toBe(copy!.en)
}

describe('browser error copy', () => {
  it.each(BROWSER_ERROR_CODES)('has non-empty Norwegian and English copy for %s, with no raw id', (code) => {
    expectEndUserCopy(code, ERROR_COPY[code])
  })

  it.each(BLOCK_REASONS)('has copy for the bot-wall reason %s that says what to do', (reason) => {
    expectEndUserCopy(reason, BLOCK_REASON_COPY[reason])
    expect(copyOf(BLOCK_REASON_COPY, reason).no).toMatch(/Ta over|prøv en annen kilde/i)
    expect(copyOf(BLOCK_REASON_COPY, reason).en).toMatch(/take over|another source/i)
  })

  it('covers exactly the closed set of bot-wall reasons', () => {
    expect(Object.keys(BLOCK_REASON_COPY).sort()).toEqual([...BLOCK_REASONS].sort())
  })

  it('has copy for every failure code the scrape preview can carry', () => {
    for (const code of SCRAPE_PREVIEW_FAILURE_CODES) {
      expect(ERROR_COPY[code], code).toBeDefined()
    }
  })

  it('never offers a proxy', () => {
    for (const [code, copy] of Object.entries(ERROR_COPY)) {
      expect(`${copy.no} ${copy.en}`, code).not.toMatch(/proxy/i)
    }
  })

  // The table cannot drift from the gateway: every code the gateway's browser and
  // scrape handlers answer with, whatever its prefix, has an entry (a code added
  // there without copy here would reach the person as the generic message).
  // Maintenance: this reads the gateway Rust sources (the part above `mod tests {`)
  // with the regexes below, so a NEW multi-word snake_case string literal alone on a
  // line in browser.rs or knowledge/quarry.rs fails this test until it is either given
  // copy in errors.ts (it is a code) or added to NOT_AN_ERROR_CODE (it is not). That is
  // the intended tripwire, not a false positive.
  const gatewayDomains = resolve(process.cwd(), 'apps/gateway/src/domains')
  // Line endings are normalised first: a Windows checkout of the .rs files may be CRLF.
  const productionHalf = (file: string) =>
    (readFileSync(resolve(gatewayDomains, file), 'utf8').replace(/\r\n/g, '\n').split(/^mod tests {/m)[0] ?? '')
  // The three shapes a handler names its code in: error("code", ..), a
  // (StatusCode, "code", "message") tuple, and a snake_case literal on its own
  // line (a code in a multi-line call or a returned pair).
  function emittedCodes(source: string): Set<string> {
    const codes = new Set<string>()
    for (const match of source.matchAll(/\berror\(\s*"([a-z][a-z_]+)"/g)) codes.add(match[1] ?? '')
    for (const match of source.matchAll(/StatusCode::[A-Z_]+,\s*"([a-z][a-z_]+)",\s*"/g)) codes.add(match[1] ?? '')
    for (const match of source.matchAll(/^\s*"([a-z]+(?:_[a-z]+)+)",?\r?$/gm)) codes.add(match[1] ?? '')
    return codes
  }
  // Snake-case literals the scan above meets that are not error codes. (The bot-wall
  // reasons the scrape handler lists are not codes either; they have BLOCK_REASON_COPY.)
  const NOT_AN_ERROR_CODE = new Set<string>([])

  it('has copy for every code the gateway browser and scrape handlers emit', () => {
    const emitted = new Set<string>()
    for (const file of ['browser.rs', 'knowledge/quarry.rs']) for (const code of emittedCodes(productionHalf(file))) emitted.add(code)
    for (const code of [
      'browser_capacity_reached',
      'browser_tab_limit_reached',
      'browser_session_reaped',
      'browser_site_blocked',
      'scrape_refused',
      'site_blocked',
      'invalid_url',
      'invalid_crawl_request',
      'invalid_browser_action',
      'zdr_persistent_profile_forbidden',
      'model_gateway_unreachable',
      'invalid_idempotency_key',
    ]) {
      expect(emitted.has(code), `the scan found ${code}`).toBe(true)
    }
    const withoutCopy = [...emitted].filter(
      (code) =>
        !ERROR_COPY[code] && !BLOCK_REASON_COPY[code] && !NOT_AN_ERROR_CODE.has(code),
    )
    expect(withoutCopy).toEqual([])
  })

  // SCRAPE_PREVIEW_FAILURE_CODES is the closed list the preview can fail with:
  // the codes classify_scrape_failure returns plus the ones scrape_preview itself
  // answers before any upstream call. `unsupported_media` is the 415 slice's code (WS5.G):
  // that slice brings its own copy entry in errors.ts and may put the code on the list in
  // the same change or a later one, so it is exempt from the first comparison only.
  it('lists exactly the failure codes the scrape preview can answer with', () => {
    const source = productionHalf('knowledge/quarry.rs')
    const body = (signature: string): string => {
      const start = source.indexOf(signature)
      if (start < 0) throw new Error(`no ${signature} in knowledge/quarry.rs`)
      return source.slice(start, source.indexOf('\n}\n', start))
    }
    const answered = new Set([...emittedCodes(body('fn classify_scrape_failure(')), ...emittedCodes(body('pub(super) async fn scrape_preview('))])
    expect(answered.has('scrape_failed')).toBe(true)
    expect(answered.has('invalid_url')).toBe(true)
    const listed = new Set<string>(SCRAPE_PREVIEW_FAILURE_CODES)
    expect([...answered].filter((code) => !listed.has(code) && code !== 'unsupported_media')).toEqual([])
    expect([...listed].filter((code) => !answered.has(code))).toEqual([])
  })

  it('keeps both languages non-empty for every entry of the whole table', () => {
    for (const [code, copy] of Object.entries(ERROR_COPY)) {
      expect(copy.no.trim().length, `${code} no`).toBeGreaterThan(0)
      expect(copy.en.trim().length, `${code} en`).toBeGreaterThan(0)
    }
  })
})

describe('describeApiError', () => {
  const failure = (fields: Record<string, unknown>) => ({ code: null, status: 400, ...fields })

  it('falls back safely for an unknown code instead of showing the raw message', () => {
    const err = Object.assign(new Error('upstream exploded at https://secret.example/x: postgres://leak'), {
      code: 'some_new_code_nobody_mapped',
      status: 400,
    })
    for (const tr of [no, en]) {
      const generic = describeApiError(err, tr)
      expect(generic.message).not.toMatch(/secret\.example|postgres|exploded|some_new_code/)
      expect(generic.message.length).toBeGreaterThan(10)
      expect(describeApiError(err, tr, { no: 'Nettleserhandlingen kunne ikke fullføres.', en: 'The browser action could not be completed.' }).message)
        .toMatch(/kunne ikke fullføres|could not be completed/)
    }
    expect(describeApiError(err, no).kind).toBe('client')
  })

  it('tells the status classes apart by kind and by copy', () => {
    expect(describeApiError(failure({ status: 401, code: 'unauthorized' }), no)).toMatchObject({
      kind: 'unauthenticated',
      message: 'Du må logge inn på nytt.',
    })
    expect(describeApiError(failure({ status: 401 }), no)).toMatchObject({
      kind: 'unauthenticated',
      message: 'Du må logge inn på nytt.',
    })
    expect(describeApiError(failure({ status: 401 }), en).message).toBe('You need to sign in again.')
    expect(describeApiError(failure({ status: 403, code: 'forbidden' }), en).kind).toBe('forbidden')
    // An unmapped 403 is a refusal, not the caller's "could not do X".
    expect(describeApiError(failure({ status: 403 }), no, { no: 'Kunne ikke bytte nettleserfane.', en: 'x' }).message).toMatch(/ikke tilgang/)
    expect(describeApiError(failure({ status: 502, code: 'upstream_error' }), no).kind).toBe('server')
    expect(describeApiError(failure({ status: 500 }), no, { no: 'Kunne ikke bytte nettleserfane.', en: 'x' }).message).toBe('Kunne ikke bytte nettleserfane.')
    expect(describeApiError(failure({ status: 410, code: 'browser_session_reaped' }), no).kind).toBe('session_lost')
    expect(describeApiError(failure({ status: 502, code: 'browser_process_lost' }), no).kind).toBe('session_lost')
    expect(describeApiError(new TypeError('Failed to fetch'), no)).toMatchObject({ kind: 'network' })
    expect(describeApiError(new TypeError('Failed to fetch'), no).message).not.toMatch(/Failed to fetch/)
  })

  it('treats a take-over 409 as its own case, by either code', () => {
    for (const code of ['USER_TOOK_OVER', 'browser_human_takeover_active']) {
      const description = describeApiError(failure({ status: 409, code }), no)
      expect(description.kind).toBe('takeover')
      expect(description.message).toMatch(/overtatt/)
    }
    // Another 409 is not a take-over.
    expect(describeApiError(failure({ status: 409, code: 'browser_run_conflict' }), no).kind).toBe('client')
  })

  it('appends the retry hint to a 429 and to every retryable entry, and only then', () => {
    const capped = describeApiError(failure({ status: 429, code: 'browser_capacity_reached', retryAfterSeconds: 15 }), no)
    expect(capped).toMatchObject({ kind: 'rate_limited', retryAfterSeconds: 15 })
    expect(capped.message).toBe('Det er for mange nettleserøkter i gang akkurat nå. Prøv igjen om 15 sekunder.')
    expect(describeApiError(failure({ status: 429, code: 'browser_capacity_reached', retryAfterSeconds: 15 }), en).message).toMatch(/Try again in 15 seconds\.$/)
    expect(describeApiError(failure({ status: 429, code: 'browser_capacity_reached', retryAfterSeconds: 1 }), en).message).toMatch(/Try again in 1 second\.$/)
    // No hint: a plain "shortly".
    expect(describeApiError(failure({ status: 429, code: 'scrape_rate_limited' }), no).message).toMatch(/Prøv igjen om litt\.$/)
    // An unmapped 429 still says wait.
    const unmapped = describeApiError(failure({ status: 429, code: 'some_new_limit', retryAfterSeconds: 30 }), no)
    expect(unmapped.message).toMatch(/for mange forespørsler.*Prøv igjen om 30 sekunder\.$/)
    // A non-retryable entry gets no sentence.
    expect(describeApiError(failure({ status: 409, code: 'browser_session_grant_inactive' }), no).message).not.toMatch(/Prøv igjen om \d|om litt/)
    // A hint that is not a sane number is ignored.
    expect(describeApiError(failure({ status: 429, code: 'scrape_rate_limited', retryAfterSeconds: 0 }), no).retryAfterSeconds).toBeNull()
  })

  it('uses the sentence for the bot-wall reason when the refusal names a known one', () => {
    const wall = (blockReason: string | null) => failure({ status: 403, code: 'site_blocked', blockReason })
    expect(describeApiError(wall('captcha_widget'), no)).toMatchObject({ kind: 'blocked' })
    expect(describeApiError(wall('captcha_widget'), no).message).toBe(copyOf(BLOCK_REASON_COPY, 'captcha_widget').no)
    expect(describeApiError(wall('waf_interstitial'), en).message).toBe(copyOf(BLOCK_REASON_COPY, 'waf_interstitial').en)
    // Unknown or missing reasons use the general site-blocked sentence, never the string.
    expect(describeApiError(wall('https://secret.example'), no).message).toBe(copyOf(ERROR_COPY, 'site_blocked').no)
    expect(describeApiError(wall(null), no).message).toBe(copyOf(ERROR_COPY, 'site_blocked').no)
    // A reason beside a code that is not a bot wall is not used.
    expect(describeApiError(failure({ status: 403, code: 'scrape_refused', blockReason: 'captcha_widget' }), no).message).toBe(copyOf(ERROR_COPY, 'scrape_refused').no)
  })

  it('passes resumable through and keeps translateApiError on the code-then-fallback rule', () => {
    expect(describeApiError(failure({ status: 404, code: 'NOT_FOUND', resumable: true }), no).resumable).toBe(true)
    expect(describeApiError(failure({ status: 404, code: 'NOT_FOUND' }), no).resumable).toBe(false)
    // translateApiError does not answer by status class: the caller's fallback wins.
    expect(translateApiError(failure({ status: 403 }), no, { no: 'Kunne ikke slette planen.', en: 'x' })).toBe('Kunne ikke slette planen.')
    expect(translateApiError(failure({ status: 429, code: 'scrape_rate_limited', retryAfterSeconds: 8 }), no)).toMatch(/Prøv igjen om 8 sekunder\.$/)
  })
})
