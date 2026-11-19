import { afterEach, describe, expect, it } from 'vitest'

import type { SpaceBrowserEvent, SpaceBrowserRunHeader } from '@/shared/api/spaces-client/browser-runs'

import {
  canOfferBrowserStop,
  isSpaceBrowserRunLive,
  latestSpaceBrowserEvidenceId,
  spaceBrowserEventLabel,
  spaceBrowserEvidenceIds,
  spaceBrowserStartOffered,
} from './space-browser'
import { DM_SPACE_FEATURE } from './space-kind'
import { clearSpaceRoomFeatures, publishSpaceRoomFeatures } from './space-room-features'

const en = (_noText: string, enText: string) => enText

function header(overrides: Partial<SpaceBrowserRunHeader> = {}): Pick<SpaceBrowserRunHeader, 'phase' | 'finishedAt'> {
  return { phase: 'running', finishedAt: null, ...overrides }
}

function event(overrides: Partial<SpaceBrowserEvent> & Pick<SpaceBrowserEvent, 'eventId' | 'kind'>): SpaceBrowserEvent {
  return { at: '2026-11-18T09:00:01.000Z', ...overrides }
}

describe('WS20-S5 room browser run presentation', () => {
  it('isSpaceBrowserRunLive is false once finishedAt is set or the phase is terminal', () => {
    expect(isSpaceBrowserRunLive(header({ phase: 'running' }))).toBe(true)
    expect(isSpaceBrowserRunLive(header({ phase: 'queued' }))).toBe(true)
    expect(isSpaceBrowserRunLive(header({ phase: 'running', finishedAt: '2026-11-18T09:05:00.000Z' }))).toBe(false)

    const terminalPhases = [
      'completed',
      'failed',
      'cancelled',
      'stopped',
      'timed_out',
      'blocked_by_site',
      'handoff_declined',
      'handoff_timeout',
    ]
    for (const phase of terminalPhases) {
      expect(isSpaceBrowserRunLive(header({ phase, finishedAt: null }))).toBe(false)
    }
  })

  it('latestSpaceBrowserEvidenceId and spaceBrowserEvidenceIds order newest first', () => {
    const events: SpaceBrowserEvent[] = [
      event({ eventId: 'e1', kind: 'observation', host: 'shop.example.com', evidenceId: 'ev-1' }),
      event({ eventId: 'e2', kind: 'action', actionType: 'click', host: 'shop.example.com' }),
      event({ eventId: 'e3', kind: 'observation', host: 'shop.example.com', evidenceId: 'ev-2' }),
      event({ eventId: 'e4', kind: 'result' }),
      event({ eventId: 'e5', kind: 'observation', host: 'shop.example.com', evidenceId: 'ev-2' }),
    ]
    expect(latestSpaceBrowserEvidenceId(events)).toBe('ev-2')
    expect(spaceBrowserEvidenceIds(events, 5)).toEqual(['ev-2', 'ev-1'])
    expect(spaceBrowserEvidenceIds(events, 1)).toEqual(['ev-2'])

    // A screenshot that appears again later moves to the newest position, once.
    const repeated: SpaceBrowserEvent[] = [
      event({ eventId: 'a', kind: 'observation', evidenceId: 'ev-1' }),
      event({ eventId: 'b', kind: 'observation', evidenceId: 'ev-2' }),
      event({ eventId: 'c', kind: 'observation', evidenceId: 'ev-1' }),
    ]
    expect(latestSpaceBrowserEvidenceId(repeated)).toBe('ev-1')
    expect(spaceBrowserEvidenceIds(repeated, 5)).toEqual(['ev-1', 'ev-2'])

    expect(latestSpaceBrowserEvidenceId([])).toBeUndefined()
    expect(spaceBrowserEvidenceIds([event({ eventId: 'x', kind: 'result' })], 5)).toEqual([])
  })

  it('labels carry a host and a closed word, never a URL', () => {
    const labelled = [
      event({ eventId: 'l1', kind: 'action', actionType: 'click', host: 'shop.example.com' }),
      event({ eventId: 'l2', kind: 'observation', host: 'shop.example.com' }),
      event({ eventId: 'l3', kind: 'handoff_requested', host: 'shop.example.com' }),
      event({ eventId: 'l4', kind: 'approval_required' }),
      event({ eventId: 'l5', kind: 'paused' }),
      event({ eventId: 'l6', kind: 'result' }),
      event({ eventId: 'l7', kind: 'something_new' }),
    ]
    const lines = labelled.map((item) => spaceBrowserEventLabel(item, en))

    expect(lines[0]).toBe('Clicked · shop.example.com')
    expect(lines[1]).toBe('Saw the page · shop.example.com')
    expect(lines[2]).toBe('A verification step blocks the page on shop.example.com')
    expect(lines[3]).toBe('Waiting for approval')
    expect(lines[4]).toBe('Paused')
    expect(lines[5]).toBe('Result')
    expect(lines[6]).toBe('Event')

    for (const line of lines) {
      expect(line).not.toMatch(/:\/\//)
      expect(line).not.toMatch(/https?/i)
      expect(line).not.toContain('/')
    }
    for (const line of lines.slice(0, 3)) {
      expect(line).toContain('shop.example.com')
    }

    // A raw URL in the action type never reaches the label: only the closed word for an unknown action shows.
    const urlAsAction = event({
      eventId: 'l8',
      kind: 'action',
      actionType: 'https://shop.example.com/cart?item=1',
      host: 'shop.example.com',
    })
    expect(spaceBrowserEventLabel(urlAsAction, en)).toBe('Action · shop.example.com')
  })

  it('an action type that names a property the table inherits is not a known verb', () => {
    // The table is a plain object, so a lookup by name reaches Object.prototype unless it is checked for an own key.
    for (const actionType of ['constructor', 'toString', 'hasOwnProperty', '__proto__']) {
      expect(spaceBrowserEventLabel(event({ eventId: 'p1', kind: 'action', actionType }), en)).toBe('Action')
    }
    const withHost = event({ eventId: 'p2', kind: 'action', actionType: 'constructor', host: 'shop.example.com' })
    expect(spaceBrowserEventLabel(withHost, en)).toBe('Action · shop.example.com')
    // A verb the table does name still reads as one.
    expect(spaceBrowserEventLabel(event({ eventId: 'p3', kind: 'action', actionType: 'click' }), en)).toBe('Clicked')
  })
})

afterEach(() => {
  clearSpaceRoomFeatures()
})

describe('WS20-S5 where a browser run is offered', () => {
  it('spaceBrowserStartOffered follows features.browser_run and never offers a start in a direct message', () => {
    expect(spaceBrowserStartOffered()).toBe(false)
    publishSpaceRoomFeatures({ browser_run: true })
    expect(spaceBrowserStartOffered()).toBe(true)
    // Control has no browser run outside personal, room, project and case Spaces.
    publishSpaceRoomFeatures({ browser_run: true, [DM_SPACE_FEATURE]: true })
    expect(spaceBrowserStartOffered()).toBe(false)
    publishSpaceRoomFeatures({ browser_run: true, [DM_SPACE_FEATURE]: false })
    expect(spaceBrowserStartOffered()).toBe(true)
    for (const browser_run of ['true', 1, {}, false]) {
      publishSpaceRoomFeatures({ browser_run })
      expect(spaceBrowserStartOffered()).toBe(false)
    }
  })

  it('canOfferBrowserStop needs manager Stop on in the listing and a manager or owner in the room', () => {
    expect(canOfferBrowserStop(true, 'manager')).toBe(true)
    expect(canOfferBrowserStop(true, 'owner')).toBe(true)
    expect(canOfferBrowserStop(true, 'editor')).toBe(false)
    expect(canOfferBrowserStop(true, 'viewer')).toBe(false)
    expect(canOfferBrowserStop(true, undefined)).toBe(false)
    expect(canOfferBrowserStop(false, 'owner')).toBe(false)
    expect(canOfferBrowserStop(undefined, 'manager')).toBe(false)
  })
})
