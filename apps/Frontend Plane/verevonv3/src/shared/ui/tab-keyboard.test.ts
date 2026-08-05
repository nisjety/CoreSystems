// @vitest-environment jsdom

import { describe, expect, it, vi } from 'vitest'
import { handleTabKeyDown } from './tab-keyboard'

describe('handleTabKeyDown', () => {
  it('wraps through a tablist and activates the focused tab', () => {
    const tablist = document.createElement('div')
    tablist.setAttribute('role', 'tablist')
    const first = document.createElement('button')
    const last = document.createElement('button')
    first.setAttribute('role', 'tab')
    last.setAttribute('role', 'tab')
    tablist.append(first, last)
    document.body.append(tablist)
    const click = vi.spyOn(first, 'click')
    const preventDefault = vi.fn()

    handleTabKeyDown({ key: 'ArrowRight', currentTarget: last, preventDefault } as unknown as KeyboardEvent & { currentTarget: HTMLElement })

    expect(document.activeElement).toBe(first)
    expect(click).toHaveBeenCalledOnce()
    expect(preventDefault).toHaveBeenCalledOnce()
    tablist.remove()
  })
})
