import { describe, expect, it } from 'vitest'

import { CRASH_PARK_NOTICE_COPY } from './errors'

describe('CRASH_PARK_NOTICE_COPY (O-02)', () => {
  it('words a crash-parked run in English and Norwegian, with the continue-or-leave choice', () => {
    expect(CRASH_PARK_NOTICE_COPY.en).toBe('The run stopped unexpectedly and is waiting; continue it or leave it')
    expect(CRASH_PARK_NOTICE_COPY.no).toBe('Kjøringen stoppet uventet og venter; fortsett den eller la den ligge')
  })
})
