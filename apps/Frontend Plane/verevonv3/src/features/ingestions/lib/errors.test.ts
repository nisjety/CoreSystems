// @vitest-environment jsdom

import { describe, expect, it } from 'vitest'
import { ApiError } from '@/shared/api/http'
import { jobControlMessage, playgroundErrorMessage, scrapeSwitchFor } from '@/features/ingestions/lib/errors'

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
