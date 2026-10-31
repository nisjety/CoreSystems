// @vitest-environment jsdom

import { describe, expect, it } from 'vitest'
import { ApiError } from '@/shared/api/http'
import { aiCodeMessage, jobControlMessage, playgroundErrorMessage, scrapeSwitchFor } from '@/features/ingestions/lib/errors'

const tr = (no: string) => no

function refusal(path: string, code = 'feature_disabled', message = 'off'): ApiError {
  return new ApiError('the request has invalid fields', 400, 'BAD_REQUEST', {
    details: { fields: [{ code, path, message }] },
  })
}

describe('refusals from Quarry name the switch that is off', () => {
  it('maps a disabled actions field to the scrape-actions switch', () => {
    expect(scrapeSwitchFor(refusal('actions'))).toBe('QUARRY_EDGE__SCRAPE_ACTIONS')
    expect(playgroundErrorMessage(refusal('actions'), tr, { no: 'x', en: 'x' })).toContain('QUARRY_EDGE__SCRAPE_ACTIONS')
  })

  it('maps a disabled formats field to the capture switch', () => {
    expect(scrapeSwitchFor(refusal('formats'))).toBe('QUARRY_EDGE__SCRAPE_CAPTURE')
  })

  it('says a reserved format is not offered yet', () => {
    expect(playgroundErrorMessage(refusal('formats[1]', 'reserved_format'), tr, { no: 'x', en: 'x' })).toBe(
      'Quarry tilbyr ikke dette formatet ennå.',
    )
  })

  it('names the job-control switch when the edge answers unsupported', () => {
    expect(jobControlMessage(new ApiError('job control is not enabled', 501, 'unsupported'), tr)).toContain('QUARRY_EDGE__JOB_CONTROL')
  })

  it('leaves an unrelated failure to the generic copy', () => {
    expect(scrapeSwitchFor(new ApiError('boom', 500, 'internal'))).toBeNull()
  })
})

describe('AI format refusals and status copy (INNH-1)', () => {
  const fallback = { no: 'x', en: 'x' }

  it('names the AI-format switch for a refused AI format, and the capture switch for a capture', () => {
    expect(scrapeSwitchFor(refusal('formats[1]'), ['markdown', 'json'])).toBe('QUARRY_EDGE__AI_FORMATS')
    expect(scrapeSwitchFor(refusal('formats'), ['screenshot'])).toBe('QUARRY_EDGE__SCRAPE_CAPTURE')
  })

  it('explains a model lane that is not connected and a missing AI scope in product copy', () => {
    const lane = new ApiError('x', 501, 'UNSUPPORTED', { details: { reason: 'model_plane_unavailable' } })
    expect(playgroundErrorMessage(lane, tr, fallback, { requested: ['json'] })).toContain('QUARRY_EDGE__MODEL_PLANE_URL')
    const scope = new ApiError('x', 403, 'FORBIDDEN', { details: { reason: 'extract_ai_scope_required' } })
    expect(playgroundErrorMessage(scope, tr, fallback)).toContain('extract:ai')
  })

  it('maps a schema keyword refusal to a sentence the person can act on', () => {
    expect(playgroundErrorMessage(refusal('formats[0].schema', 'schema_keyword_not_supported'), tr, fallback)).toContain('$ref')
  })

  it('gives every AI status code copy and says so when a code is new', () => {
    expect(aiCodeMessage('ZDR_UNAVAILABLE', tr)).toContain('ZDR')
    expect(aiCodeMessage('MODEL_RATE_LIMITED', tr, 30)).toContain('30')
    expect(aiCodeMessage('SOMETHING_NEW', tr)).toContain('SOMETHING_NEW')
  })
})
