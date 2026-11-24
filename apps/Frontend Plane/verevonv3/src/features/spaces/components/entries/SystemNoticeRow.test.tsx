// @vitest-environment jsdom

import { cleanup, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it } from 'vitest'

import type { SpaceSystemEntry } from '@/shared/api/spaces-client'
import { clearPreferences, receivePreferences } from '@/shared/preferences/preferences-store'
import { SystemNoticeRow } from './SystemNoticeRow'

// B-REC-06 (DEPTH-WS16-FORMAT): a room system row's time follows the viewer's Settings time format and zone, like
// every other Space time, instead of the browser's own default.

const NOT_EXPLICIT = { language: false, timezone: false, dateFormat: false, timeFormat: false, compactMode: false }
const entry = { kind: 'system', id: 'sys-1', at: '2026-11-01T13:05:00Z', code: 'member_joined', subjectId: 'u-ola' } as SpaceSystemEntry

afterEach(() => {
  cleanup()
  clearPreferences()
})

const timeText = () => document.querySelector('.verevon-room-system-row__time')?.textContent ?? ''

describe('SystemNoticeRow time', () => {
  it('uses the explicit time format and zone from Settings', () => {
    receivePreferences({
      timezone: 'Asia/Tokyo',
      timeFormat: '12h',
      explicit: { ...NOT_EXPLICIT, timezone: true, timeFormat: true },
    })
    flush()
    render(() => <SystemNoticeRow entry={entry} subjectName="Ola" />)
    // 13:05 UTC is 22:05 in Tokyo, in the 12-hour clock.
    expect(timeText()).toMatch(/10:05/)
    expect(timeText()).toMatch(/p\.?\s?m/i)
    expect(document.querySelector('time')?.getAttribute('datetime')).toBe('2026-11-01T13:05:00.000Z')
  })

  it('switches to the 24-hour clock when that is the choice', () => {
    receivePreferences({
      timezone: 'Asia/Tokyo',
      timeFormat: '24h',
      explicit: { ...NOT_EXPLICIT, timezone: true, timeFormat: true },
    })
    flush()
    render(() => <SystemNoticeRow entry={entry} subjectName="Ola" />)
    expect(timeText()).toMatch(/22[:.]05/)
  })
})
