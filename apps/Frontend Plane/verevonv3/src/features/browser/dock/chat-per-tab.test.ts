// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import {
  CHAT_PER_TAB_DEFAULT,
  createChatThreadByTab,
  planChatForTabSwitch,
  shouldStartNewThread,
} from './chat-per-tab'

describe('chat per tab rule (WS9.H)', () => {
  it('defaults to keep, so the chat never moves on its own', () => {
    expect(CHAT_PER_TAB_DEFAULT).toBe('keep')
  })

  it('moves the chat only under new, and only between two known, different tabs', () => {
    expect(shouldStartNewThread('tab-a', 'tab-b', 'keep')).toBe(false)
    expect(shouldStartNewThread('tab-a', 'tab-b', 'new')).toBe(true)
    expect(shouldStartNewThread('tab-a', 'tab-a', 'new')).toBe(false)
    expect(shouldStartNewThread(null, 'tab-b', 'new')).toBe(false)
    expect(shouldStartNewThread('tab-a', null, 'new')).toBe(false)
    expect(shouldStartNewThread(undefined, 'tab-b', 'new')).toBe(false)
  })
})

describe('planChatForTabSwitch', () => {
  it('keeps the chat where it is under keep, even when the tab has a remembered chat', () => {
    const threads = createChatThreadByTab()
    threads.remember('tab-b', 'thread-b')

    const plan = planChatForTabSwitch({
      previousTab: 'tab-a',
      nextTab: 'tab-b',
      rule: 'keep',
      threadFor: threads.threadFor,
    })

    expect(plan).toEqual({ kind: 'stay' })
  })

  it('returns to the chat a tab remembered under new', () => {
    const threads = createChatThreadByTab()
    threads.remember('tab-b', 'thread-b')

    const plan = planChatForTabSwitch({
      previousTab: 'tab-a',
      nextTab: 'tab-b',
      rule: 'new',
      threadFor: threads.threadFor,
    })

    expect(plan).toEqual({ kind: 'select-thread', threadId: 'thread-b' })
  })

  it('starts a new chat under new when the tab has none yet', () => {
    const threads = createChatThreadByTab()

    const plan = planChatForTabSwitch({
      previousTab: 'tab-a',
      nextTab: 'tab-c',
      rule: 'new',
      threadFor: threads.threadFor,
    })

    expect(plan).toEqual({ kind: 'new-thread' })
  })

  it('never moves the chat on the first tab of a session, or without a next tab', () => {
    const threads = createChatThreadByTab()

    expect(planChatForTabSwitch({ previousTab: null, nextTab: 'tab-a', rule: 'new', threadFor: threads.threadFor }))
      .toEqual({ kind: 'stay' })
    expect(planChatForTabSwitch({ previousTab: 'tab-a', nextTab: null, rule: 'new', threadFor: threads.threadFor }))
      .toEqual({ kind: 'stay' })
  })
})

describe('createChatThreadByTab', () => {
  it('remembers one thread per browser tab and replaces it on a later remember', () => {
    const threads = createChatThreadByTab()

    threads.remember('tab-a', 'thread-1')
    threads.remember('tab-b', 'thread-2')
    threads.remember('tab-a', 'thread-3')

    expect(threads.threadFor('tab-a')).toBe('thread-3')
    expect(threads.threadFor('tab-b')).toBe('thread-2')
    expect(threads.threadFor('tab-unknown')).toBeNull()
    expect(threads.size()).toBe(2)
  })

  it('ignores an empty browser tab id or an empty thread id', () => {
    const threads = createChatThreadByTab()

    threads.remember('', 'thread-1')
    threads.remember('tab-a', null)
    threads.remember('tab-a', undefined)
    threads.remember('tab-a', '')

    expect(threads.size()).toBe(0)
    expect(threads.threadFor('tab-a')).toBeNull()
  })

  it('forgets one tab, and clears every tab', () => {
    const threads = createChatThreadByTab()
    threads.remember('tab-a', 'thread-1')
    threads.remember('tab-b', 'thread-2')

    threads.forget('tab-a')
    expect(threads.threadFor('tab-a')).toBeNull()
    expect(threads.size()).toBe(1)

    threads.clear()
    expect(threads.size()).toBe(0)
  })

  it('writes nothing to storage, so a reload starts with no remembered chat', () => {
    const before = localStorage.length
    const threads = createChatThreadByTab()

    threads.remember('tab-a', 'thread-1')

    expect(localStorage.length).toBe(before)
    expect(createChatThreadByTab().threadFor('tab-a')).toBeNull()
  })
})
