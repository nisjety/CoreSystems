import { cleanup, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  AnnouncerRegions,
  announce,
  resetAnnouncerForTests,
  writeAnnouncePostsPreference,
} from './announcer'

// WS16-S4: the live regions are the only output, so the tests read what each region holds.
const region = (name: 'polite-a' | 'polite-b' | 'assertive-a' | 'assertive-b') =>
  document.querySelector(`[data-announcer-region="${name}"]`)?.textContent ?? ''

/** What a screen reader would speak now: the polite regions that hold text. */
const politeNow = () => ['polite-a', 'polite-b'].map((name) => region(name as 'polite-a')).filter((text) => text !== '')

/** Moves the fake clock on and lets Solid apply the signal writes. */
const step = (ms: number) => {
  vi.advanceTimersByTime(ms)
  flush()
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date('2026-11-18T09:00:00.000Z'))
  resetAnnouncerForTests()
  render(() => <AnnouncerRegions />)
})

afterEach(() => {
  resetAnnouncerForTests()
  cleanup()
  vi.useRealTimers()
})

describe('announce', () => {
  it('says an identical message again straight after the same text, in the other region', () => {
    announce('Rommet er oppdatert')
    step(0)
    expect(politeNow()).toEqual(['Rommet er oppdatert'])

    announce('Rommet er oppdatert')
    step(1200)
    expect(region('polite-b')).toBe('Rommet er oppdatert')
  })

  it('collapses the same key inside its window to the first message', () => {
    announce('Ny melding', { key: 'thread-1' })
    announce('Ny melding til', { key: 'thread-1' })
    step(0)
    step(3600)
    expect(politeNow()).toEqual(['Ny melding'])
    expect(region('polite-b')).toBe('')
  })

  it('keeps at most five queued polite messages and drops the oldest', () => {
    for (let index = 1; index <= 7; index += 1) announce(`Melding ${index}`)
    const heard: string[] = []
    for (let index = 0; index < 5; index += 1) {
      step(index === 0 ? 0 : 1200)
      step(150)
      heard.push(...politeNow())
    }
    expect(heard).toEqual(['Melding 3', 'Melding 4', 'Melding 5', 'Melding 6', 'Melding 7'])
  })

  it('speaks one polite message per 1.2 seconds', () => {
    announce('Først')
    announce('Deretter')
    step(0)
    expect(politeNow()).toEqual(['Først'])
    step(1199)
    expect(region('polite-b')).toBe('')
    step(1)
    expect(region('polite-b')).toBe('Deretter')
  })

  it('says three or more queued messages of one group as a single summary', () => {
    const summarise = (count: number) => `${count} nye meldinger i rommet`
    announce('Svar 1', { group: 'room-1', summarise })
    announce('Svar 2', { group: 'room-1', summarise })
    announce('Svar 3', { group: 'room-1', summarise })
    step(0)
    expect(politeNow()).toEqual(['3 nye meldinger i rommet'])
    step(2400)
    expect(politeNow()).toEqual(['3 nye meldinger i rommet'])
  })

  it('speaks each of two grouped messages on its own', () => {
    const summarise = (count: number) => `${count} nye meldinger i rommet`
    announce('Svar 1', { group: 'room-1', summarise })
    announce('Svar 2', { group: 'room-1', summarise })
    step(0)
    expect(politeNow()).toEqual(['Svar 1'])
    step(1200)
    expect(region('polite-b')).toBe('Svar 2')
  })

  it('speaks an assertive message at once, ahead of the polite queue', () => {
    announce('Vanlig melding')
    announce('Krever godkjenning nå', { politeness: 'assertive' })
    flush()
    expect(region('assertive-a')).toBe('Krever godkjenning nå')
    expect(region('polite-a')).toBe('')
    step(0)
    expect(politeNow()).toEqual(['Vanlig melding'])
  })

  it('speaks plain text: markdown is stripped, and a long message is cut at 140 characters', () => {
    announce('**Viktig:** sjekk `kode` nå')
    step(0)
    expect(politeNow()).toEqual(['Viktig: sjekk kode nå'])

    resetAnnouncerForTests()
    announce('a'.repeat(200))
    step(0)
    const [spoken] = politeNow()
    expect(spoken).toHaveLength(140)
    expect(spoken?.endsWith('…')).toBe(true)
  })

  it('stays silent for posts and mentions when post announcements are off, and still speaks status', () => {
    writeAnnouncePostsPreference('off')
    announce('Ny innlegg', { kind: 'post' })
    announce('Du ble nevnt', { kind: 'mention' })
    announce('Rommet er lagret', { kind: 'status' })
    step(0)
    expect(politeNow()).toEqual(['Rommet er lagret'])
  })

  it('speaks only mentions when post announcements are set to mentions', () => {
    writeAnnouncePostsPreference('mentions')
    announce('Ny innlegg', { kind: 'post' })
    announce('Du ble nevnt', { kind: 'mention' })
    step(0)
    expect(politeNow()).toEqual(['Du ble nevnt'])
  })
})
