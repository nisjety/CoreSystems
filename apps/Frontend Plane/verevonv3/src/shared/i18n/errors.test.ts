import { describe, expect, it } from 'vitest'
import { translateApiError } from './errors'

describe('translateApiError', () => {
  it('keeps an unknown provider-send outcome distinct from a normal retryable failure', () => {
    const tr = (no: string) => no

    expect(translateApiError({ code: 'delivery_unknown' }, tr)).toMatch(/ikke send på nytt automatisk/i)
    expect(translateApiError({ code: 'send_failed' }, tr)).toMatch(/ble ikke sendt/i)
    expect(translateApiError({ code: 'delivery_unavailable' }, tr)).toMatch(/ikke sendt eller lagret/i)
  })
})
