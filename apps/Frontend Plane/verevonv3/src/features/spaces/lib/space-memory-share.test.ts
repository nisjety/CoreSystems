import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { SpaceSummary } from '@/shared/api/spaces-client'

// W3-FIX-13: the share lib as the dialog runs it. The feature read, the room filter and the refusal sentence are answered
// here. The writes themselves are asserted through the dialog (components/MemoryDeclassifyDialog.test.tsx).

const spacesClient = vi.hoisted(() => ({ getSpaceFeatures: vi.fn() }))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))

const { ApiError } = await import('@/shared/api/api-error')
const { translateApiError } = await import('@/shared/i18n')
const { memoryDeclassifyOffered, memoryRoomErrorMessage, shareableRooms } = await import('./space-memory-share')

// The lib takes its translator from the caller. The copy under test is the Norwegian default, as in the page tests.
const tr = (no: string) => no
const FALLBACK = {
  no: 'Minnet ble ikke delt. Teksten din er beholdt; prøv igjen.',
  en: 'The memory was not shared. Your text is kept; try again.',
}

const room = (over: Partial<SpaceSummary> = {}): SpaceSummary => ({
  space_ref: 'room-x',
  name: 'Room X',
  kind: 'room',
  lifecycle: 'active',
  ...over,
})

beforeEach(() => {
  spacesClient.getSpaceFeatures.mockReset()
})

describe('W3-FIX-13: memoryDeclassifyOffered', () => {
  it('is false when the features read fails, so the control stays hidden', async () => {
    spacesClient.getSpaceFeatures.mockRejectedValueOnce(new ApiError('unavailable', 503, null))
    await expect(memoryDeclassifyOffered()).resolves.toBe(false)
  })

  it('is false when the key is absent from the report', async () => {
    spacesClient.getSpaceFeatures.mockResolvedValueOnce({ message_search: true })
    await expect(memoryDeclassifyOffered()).resolves.toBe(false)
  })

  it('is true only when the key is on', async () => {
    spacesClient.getSpaceFeatures.mockResolvedValueOnce({ memory_declassify: true })
    await expect(memoryDeclassifyOffered()).resolves.toBe(true)
  })
})

describe('W3-FIX-13: shareableRooms', () => {
  it('drops personal Spaces, direct messages, group DMs, archived rooms and rooms that are not active', () => {
    const listed = shareableRooms([
      room({ space_ref: 'personal-kari', name: 'Kari', kind: 'personal' }),
      room({ space_ref: 'dm-kari-ola', name: 'Ola', kind: 'dm' }),
      room({ space_ref: 'group-dm-1', name: 'Kari, Ola and Assistant', kind: 'group_dm' }),
      room({ space_ref: 'room-old', name: 'Room Old', archived: true }),
      room({ space_ref: 'room-pending', name: 'Room Pending', lifecycle: 'pending_registration' }),
      room({ space_ref: 'room-suspended', name: 'Room Suspended', lifecycle: 'suspended' }),
      room({ space_ref: 'room-a', name: 'Room A' }),
    ])
    expect(listed.map((entry) => entry.space_ref)).toEqual(['room-a'])
  })

  it('sorts the rooms it keeps by name', () => {
    const listed = shareableRooms([
      room({ space_ref: 'room-b', name: 'Room B' }),
      room({ space_ref: 'room-a', name: 'Room A' }),
    ])
    expect(listed.map((entry) => entry.name)).toEqual(['Room A', 'Room B'])
  })
})

describe('W3-FIX-13: memoryRoomErrorMessage', () => {
  it('gives a 503 space_decision_unavailable the sentence of the operation it came from', () => {
    const unavailable = new ApiError('unavailable', 503, 'space_decision_unavailable')
    expect(memoryRoomErrorMessage(unavailable, tr, 'share', FALLBACK)).toBe(
      'Rommet kunne ikke bekrefte tilgangen din akkurat nå. Ingenting er delt; prøv igjen om litt.',
    )
    expect(memoryRoomErrorMessage(unavailable, tr, 'take_back', FALLBACK)).toBe(
      'Rommet kunne ikke bekrefte tilgangen din akkurat nå. Ingenting er endret; prøv igjen om litt.',
    )
  })

  it('passes any other code to translateApiError, and to the caller fallback for a code it does not know', () => {
    const refused = new ApiError('no', 403, 'memory_retract_not_authorized')
    expect(memoryRoomErrorMessage(refused, tr, 'take_back', FALLBACK)).toBe(translateApiError(refused, tr, FALLBACK))
    expect(memoryRoomErrorMessage(refused, tr, 'take_back', FALLBACK)).toBe(
      'Du har ikke tilgang til å ta dette minnet tilbake fra rommet. Ingenting er endret.',
    )

    const unknown = new ApiError('boom', 500, 'boom_unknown')
    expect(memoryRoomErrorMessage(unknown, tr, 'share', FALLBACK)).toBe(FALLBACK.no)
  })
})
