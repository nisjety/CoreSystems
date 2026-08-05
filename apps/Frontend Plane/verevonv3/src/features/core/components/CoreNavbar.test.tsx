// @vitest-environment jsdom

import { afterEach, describe, expect, it } from 'vitest'
import { shouldDeferGlobalSlashSearch } from './CoreNavbar'

afterEach(() => {
  document.body.replaceChildren()
})

describe('shouldDeferGlobalSlashSearch', () => {
  it('lets the Inbox own slash when its local queue search is mounted', () => {
    const inboxSearch = document.createElement('input')
    inboxSearch.id = 'verevon-inbox-search'
    document.body.append(inboxSearch)

    expect(shouldDeferGlobalSlashSearch()).toBe(true)
  })

  it('keeps the global shortcut available outside Inbox', () => {
    expect(shouldDeferGlobalSlashSearch()).toBe(false)
  })
})
