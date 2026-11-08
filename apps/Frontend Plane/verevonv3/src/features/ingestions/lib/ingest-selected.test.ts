// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/http'
import {
  INGEST_BATCH_LIMIT,
  ingestBatchRequest,
  ingestSelectedEnabled,
  ingestSelectedMessage,
  planIngest,
  togglePick,
  type PickableRow,
} from '@/features/ingestions/lib/ingest-selected'

const tr = (no: string) => no
const english = (_no: string, en: string) => en

afterEach(() => {
  vi.unstubAllEnvs()
})

const rows: PickableRow[] = [
  { seq: 1, url: 'https://vg.no/a' },
  { seq: 2, url: 'https://vg.no/b' },
  { seq: 3, url: 'https://vg.no/a' },
  { seq: 4, url: '' },
]

function zdrRefusal(code: string): ApiError {
  return new ApiError('ZDR active: durable batch jobs are not available', 403, 'FORBIDDEN', {
    details: { zdr_refusal: { code, action: 'batch' } },
  })
}

describe('the Ingest selected switch', () => {
  it('is off unless the build sets VITE_INGEST_SELECTED to exactly true', () => {
    expect(ingestSelectedEnabled()).toBe(false)
    vi.stubEnv('VITE_INGEST_SELECTED', 'true')
    expect(ingestSelectedEnabled()).toBe(true)
    vi.stubEnv('VITE_INGEST_SELECTED', '1')
    expect(ingestSelectedEnabled()).toBe(false)
  })
})

describe('togglePick', () => {
  it('adds a row, removes it on the second click, and never mutates the set it was given', () => {
    const before = new Set([1])
    const added = togglePick(before, 2)
    expect([...added]).toEqual([1, 2])
    expect([...before]).toEqual([1])
    expect([...togglePick(added, 2)]).toEqual([1])
  })

  it('stops adding at the batch limit, so a selection can never exceed it', () => {
    expect(INGEST_BATCH_LIMIT).toBe(200)
    expect([...togglePick(new Set([1, 2]), 3, 2)]).toEqual([1, 2])
    expect([...togglePick(new Set([1]), 2, 2)]).toEqual([1, 2])
  })
})

describe('planIngest (count first, nothing is sent)', () => {
  it('counts each picked URL once, in page order, and reports the repeats it skipped', () => {
    expect(planIngest(rows, new Set([3, 1, 2]))).toEqual({
      urls: ['https://vg.no/a', 'https://vg.no/b'],
      duplicates: 1,
    })
  })

  it('leaves out a row with no URL and never plans past the limit', () => {
    expect(planIngest(rows, new Set([1, 2, 4])).urls).toEqual(['https://vg.no/a', 'https://vg.no/b'])
    expect(planIngest(rows, new Set([1, 2]), 1).urls).toEqual(['https://vg.no/a'])
  })

  it('plans nothing when no row is picked', () => {
    expect(planIngest(rows, new Set())).toEqual({ urls: [], duplicates: 0 })
  })
})

describe('ingestBatchRequest', () => {
  it('is a batch of the URLs with ingest set, on a copy of the list', () => {
    const urls = ['https://vg.no/a']
    const body = ingestBatchRequest(urls)
    expect(body).toEqual({ kind: 'batch', urls: ['https://vg.no/a'], ingest: true })
    expect(body.urls).not.toBe(urls)
  })
})

describe('ingestSelectedMessage', () => {
  it('shows a ZDR refusal of the batch in the catalogue words, in both languages', () => {
    const refusal = zdrRefusal('zdr_durable_mutation_forbidden')
    expect(ingestSelectedMessage(refusal, tr)).toBe(
      'Legitimasjon med Zero Data Retention kan ikke opprette, endre eller slette varig data.',
    )
    expect(ingestSelectedMessage(refusal, english)).toBe(
      'Zero Data Retention credentials cannot create, change, or delete durable state.',
    )
  })

  it('gives an unknown ZDR code the generic ZDR sentence, never the server text', () => {
    expect(ingestSelectedMessage(zdrRefusal('zdr_something_newer'), tr)).toBe(
      'Forespørselen med Zero Data Retention kunne ikke fullføres. Kontakt support hvis problemet vedvarer.',
    )
  })

  it('does not read a refusal that carries no ZDR code as a ZDR refusal', () => {
    const plain = new ApiError('forbidden', 403, 'FORBIDDEN', { details: { zdr_refusal: { code: 'other' } } })
    expect(ingestSelectedMessage(plain, tr)).not.toContain('Zero Data Retention')
    expect(ingestSelectedMessage(new TypeError('Failed to fetch'), tr)).not.toContain('Zero Data Retention')
  })
})
