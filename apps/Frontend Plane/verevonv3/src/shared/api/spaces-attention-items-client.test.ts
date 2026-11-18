import { beforeEach, describe, expect, it, vi } from 'vitest'

const http = vi.hoisted(() => ({ requestJson: vi.fn() }))

vi.mock('./http', () => ({ requestJson: http.requestJson }))

const {
  attentionItemsFromWire,
  getThreadFollow,
  setAttentionItemState,
  setThreadFollow,
} = await import('./spaces-attention-items-client')

/** The JSON body a request was sent with. */
function sentBody(callIndex = 0): Record<string, unknown> {
  const init = http.requestJson.mock.calls[callIndex]?.[1] as RequestInit
  return JSON.parse(String(init.body)) as Record<string, unknown>
}

beforeEach(() => {
  http.requestJson.mockReset()
})

// WS11-S7: a row is ids, a closed kind word and times. A title is read at render under the viewer's
// access, so a malformed row or an unknown kind is dropped, and an unknown headline state is restricted.
const row = {
  item_id: 'item_1',
  kind: 'mention',
  space_ref: 'room-a',
  space_name: 'Driftsrom',
  thread_id: 'thr_1',
  message_id: 'msg_1',
  run_id: null,
  actor: { kind: 'user', subject_id: 'user_2', agent_ref: null },
  count: 3,
  state: 'open',
  snoozed_until: null,
  created_at: 1000,
  updated_at: 2000,
  headline: 'Hei fra teamet',
  headline_state: 'ok',
}

describe('attentionItemsFromWire (WS11-S7)', () => {
  it('reads a row as the gateway sent it, in camel case', () => {
    const page = attentionItemsFromWire({
      items: [row],
      needs_you: [],
      mode: 'live',
      gaps: ['session_core'],
      next_cursor: 'cursor-2',
    })
    expect(page.items).toEqual([{
      itemId: 'item_1',
      kind: 'mention',
      spaceRef: 'room-a',
      spaceName: 'Driftsrom',
      threadId: 'thr_1',
      messageId: 'msg_1',
      runId: null,
      actor: { kind: 'user', subjectId: 'user_2', agentRef: null },
      count: 3,
      state: 'open',
      snoozedUntil: null,
      createdAt: 1000,
      updatedAt: 2000,
      headline: 'Hei fra teamet',
      headlineState: 'ok',
    }])
    expect(page).toMatchObject({ mode: 'live', gaps: ['session_core'], nextCursor: 'cursor-2' })
  })

  it('drops malformed rows and rows of an unknown kind', () => {
    const page = attentionItemsFromWire({
      items: [
        row,
        { ...row, item_id: 'item_2', kind: 'poke' },
        { ...row, item_id: 'item_3', space_ref: '   ' },
        { ...row, item_id: '' },
        'not a row',
        null,
        ['item_4'],
      ],
      needs_you: [
        { approval_id: 'appr_1', space_ref: 'room-a', headline: null, headline_state: 'ok' },
        { space_ref: 'room-b' },
        { approval_id: 'appr_3', space_ref: '   ' },
        'junk',
      ],
    })
    expect(page.items.map((item) => item.itemId)).toEqual(['item_1'])
    expect(page.needsYou.map((item) => item.approvalId)).toEqual(['appr_1'])
  })

  it('reads an unknown headline_state as restricted, and keeps the states it knows', () => {
    const page = attentionItemsFromWire({
      items: [
        { ...row, item_id: 'item_5', headline: null, headline_state: 'loud' },
        { ...row, item_id: 'item_6', headline: null, headline_state: 'hidden' },
        { ...row, item_id: 'item_7', headline: 'Hei', headline_state: 'ok' },
        { ...row, item_id: 'item_8', headline: null, headline_state: undefined },
      ],
      needs_you: [{ approval_id: 'appr_2', space_ref: 'room-a', headline: null, headline_state: 'loud' }],
    })
    expect(page.items.map((item) => [item.itemId, item.headlineState])).toEqual([
      ['item_5', 'restricted'],
      ['item_6', 'hidden'],
      ['item_7', 'ok'],
      ['item_8', 'restricted'],
    ])
    expect(page.needsYou[0]?.headlineState).toBe('restricted')
  })
})

describe('attention item state and thread follow requests (WS11-S7)', () => {
  it('marks an item done or open with its state alone', async () => {
    http.requestJson.mockResolvedValue({ item_id: 'item/1', state: 'done', snoozed_until: null })
    await expect(setAttentionItemState('item/1', { state: 'done' })).resolves.toEqual({
      itemId: 'item/1',
      state: 'done',
      snoozedUntil: null,
    })
    expect(http.requestJson).toHaveBeenCalledWith('/api/v1/spaces/attention/items/item%2F1/state', {
      method: 'POST',
      body: JSON.stringify({ state: 'done' }),
    })
  })

  it('snoozes an item with its snooze_until and nothing else', async () => {
    http.requestJson.mockResolvedValue({ item_id: 'item_1', state: 'snoozed', snoozed_until: 5000 })
    await expect(setAttentionItemState('item_1', { state: 'snoozed', snoozeUntil: 5000 })).resolves.toEqual({
      itemId: 'item_1',
      state: 'snoozed',
      snoozedUntil: 5000,
    })
    expect(sentBody()).toEqual({ state: 'snoozed', snooze_until: 5000 })
  })

  it('reads a thread follow, and sends a mute as the state alone', async () => {
    http.requestJson.mockResolvedValueOnce({ state: 'following', reason: 'self' })
    await expect(getThreadFollow('room a', 'thr/1')).resolves.toBe('following')
    expect(http.requestJson).toHaveBeenLastCalledWith('/api/v1/spaces/room%20a/threads/thr%2F1/follow')

    http.requestJson.mockResolvedValueOnce({ state: 'muted' })
    await expect(setThreadFollow('room-a', 'thr_1', 'muted')).resolves.toBe('muted')
    expect(http.requestJson).toHaveBeenLastCalledWith('/api/v1/spaces/room-a/threads/thr_1/follow', {
      method: 'PUT',
      body: JSON.stringify({ state: 'muted' }),
    })
  })

  it('reads a follow state it does not know as none', async () => {
    http.requestJson.mockResolvedValueOnce({ state: 'watching_closely' })
    await expect(setThreadFollow('room-a', 'thr_1', 'none')).resolves.toBe('none')
  })
})
