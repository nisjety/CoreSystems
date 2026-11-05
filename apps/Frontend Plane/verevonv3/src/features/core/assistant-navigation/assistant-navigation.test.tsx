import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createAssistantNavigationGuide, resolveAssistantNavigation } from './assistant-navigation'

describe('assistant navigation', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    window.history.replaceState({}, '', '/')
    document.body.innerHTML = '<nav class="core-sidebar__mini-nav"><a href="/knowledge">Kunnskap</a></nav>'
    const target = document.querySelector('a')!
    target.getBoundingClientRect = () => ({ x: 10, y: 100, left: 10, top: 100, right: 50, bottom: 140, width: 40, height: 40, toJSON() {} })
  })
  afterEach(() => { vi.useRealTimers(); document.body.innerHTML = '' })

  it('accepts direct location questions and explicit navigation only', () => {
    expect(resolveAssistantNavigation('Hvor finner jeg kunnskapsbasen?')).toEqual({ label: 'Kunnskap', href: '/knowledge', action: 'point' })
    expect(resolveAssistantNavigation('Open the knowledge base')).toEqual({ label: 'Kunnskap', href: '/knowledge', action: 'navigate' })
    expect(resolveAssistantNavigation('Find something about knowledge for me')).toBeNull()
    expect(resolveAssistantNavigation('open https://example.com')).toBeNull()
    expect(resolveAssistantNavigation('open knowledge and send an email')).toBeNull()
  })

  it('recognizes the reported misspelling and bounded, unambiguous variants', () => {
    const knowledge = { label: 'Kunnskap', href: '/knowledge', action: 'point' }
    expect(resolveAssistantNavigation('hvor er kunskaps')).toEqual(knowledge)
    expect(resolveAssistantNavigation('hvor er kunskaps basen')).toEqual(knowledge)
    expect(resolveAssistantNavigation('Hvor er kunnskap?')).toEqual(knowledge)
    expect(resolveAssistantNavigation('Kan du vise meg kunskap?')).toEqual(knowledge)
    expect(resolveAssistantNavigation('where is knowlege?')).toEqual(knowledge)
    expect(resolveAssistantNavigation('where is knoweldge?')).toEqual(knowledge)
    expect(resolveAssistantNavigation('open knowlege')).toEqual({ ...knowledge, action: 'navigate' })
    // Short partial words and multi-part requests must never trigger navigation.
    expect(resolveAssistantNavigation('hvor er kun')).toBeNull()
    expect(resolveAssistantNavigation('open support and delete files')).toBeNull()
    expect(resolveAssistantNavigation('hvor er kunskaps dokumentene')).toBeNull()
    expect(resolveAssistantNavigation('where is help')).toBeNull()
    expect(resolveAssistantNavigation('open constructor')).toBeNull()
  })

  it('finds the requested knowledge page without navigating or interpreting extra tasks', () => {
    const point = { label: 'Kunnskap', href: '/knowledge', action: 'point' }
    expect(resolveAssistantNavigation('kan du finne kunskap siden for meg')).toEqual(point)
    expect(resolveAssistantNavigation('kan du finne kunnskap siden for meg')).toEqual(point)
    expect(resolveAssistantNavigation('finn kunnskapssiden')).toEqual(point)
    expect(resolveAssistantNavigation('Can you find the knowledge page for me?')).toEqual(point)
    expect(resolveAssistantNavigation('åpne kunnskap siden for meg')).toEqual({ ...point, action: 'navigate' })
    expect(resolveAssistantNavigation('finn kunnskap siden og send en epost')).toBeNull()
    expect(resolveAssistantNavigation('finn kunnskap dokumentene for meg')).toBeNull()
    expect(resolveAssistantNavigation('finn support page and delete files')).toBeNull()
  })

  it('navigates for a request to go to a page and open it, but not unrelated extra tasks', () => {
    const knowledge = { label: 'Kunnskap', href: '/knowledge', action: 'navigate' }
    expect(resolveAssistantNavigation('kan du gå til kunskapsbasen og åpne den for meg')).toEqual(knowledge)
    expect(resolveAssistantNavigation('Gå til kunnskapsbasen og åpne den')).toEqual(knowledge)
    expect(resolveAssistantNavigation('Kan du åpne kunskapsbasen for meg?')).toEqual(knowledge)
    expect(resolveAssistantNavigation('Can you go to the knowledge base and open it for me?')).toEqual(knowledge)
    expect(resolveAssistantNavigation('hvor er kunnskapsbasen og åpne den')).toEqual(knowledge)
    expect(resolveAssistantNavigation('gå til kunnskapsbasen og slett filer')).toBeNull()
    expect(resolveAssistantNavigation('gå til kunnskapsbasen og åpne den og slett filer')).toBeNull()
    expect(resolveAssistantNavigation('gå til kunnskapsbasen og åpne chat')).toBeNull()
  })

  it('starts the guide for the exact reported prompt', () => {
    const guide = createAssistantNavigationGuide({ navigate: vi.fn(), onComplete: vi.fn(), onOpenChat: vi.fn() })
    expect(guide.canStart('hvor er kunskaps')).toBe(true)
    expect(guide.canStart('where is help')).toBe(false)
    expect(guide.start('hvor er kunskaps')).toBe(true)
    expect(guide.destination()?.href).toBe('/knowledge')
    guide.stop()
  })

  it('accepts only a validated model-proposed app action', () => {
    const navigate = vi.fn()
    const guide = createAssistantNavigationGuide({ navigate, onComplete: vi.fn(), onOpenChat: vi.fn() })
    expect(guide.startAction({ type: 'assistant.navigation.guide', href: 'https://example.com', mode: 'navigate' })).toBe(false)
    expect(guide.startAction({ type: 'assistant.navigation.guide', href: '/knowledge', mode: 'navigate', extra: true })).toBe(false)
    expect(guide.startAction({ type: 'assistant.navigation.guide', href: '/knowledge', mode: 'navigate' })).toBe(true)
    vi.advanceTimersByTime(2000)
    expect(navigate).toHaveBeenCalledExactlyOnceWith('/knowledge')
    guide.stop()
  })

  it('points at a real rail link and offers a follow-up without navigating', () => {
    const navigate = vi.fn(), onComplete = vi.fn(), onOpenChat = vi.fn()
    const guide = createAssistantNavigationGuide({ navigate, onComplete, onOpenChat })
    expect(guide.start('Hvor er kunnskap?')).toBe(true)
    vi.advanceTimersByTime(2000)
    expect(navigate).not.toHaveBeenCalled()
    expect(onComplete).toHaveBeenCalledWith('Hva kan jeg gjøre i Kunnskap?')
    guide.stop()
  })

  it('does not navigate after stop, a route change, or when the destination is absent', () => {
    const navigate = vi.fn(), onComplete = vi.fn(), onOpenChat = vi.fn()
    const guide = createAssistantNavigationGuide({ navigate, onComplete, onOpenChat })
    expect(guide.start('open knowledge')).toBe(true)
    guide.stop()
    vi.advanceTimersByTime(2000)
    expect(navigate).not.toHaveBeenCalled()
    expect(onComplete).not.toHaveBeenCalled()
    expect(guide.canStart('open agents')).toBe(false)
    expect(guide.start('open agents')).toBe(false)
    expect(guide.start('open knowledge')).toBe(true)
    document.querySelector('a')!.remove()
    vi.advanceTimersByTime(2000)
    expect(navigate).not.toHaveBeenCalled()
  })

  it('navigates only after an uncancelled explicit request', () => {
    const navigate = vi.fn((href: string) => window.history.pushState({}, '', href)), onComplete = vi.fn(), onOpenChat = vi.fn()
    const guide = createAssistantNavigationGuide({ navigate, onComplete, onOpenChat })
    expect(guide.start('åpne kunnskap')).toBe(true)
    vi.advanceTimersByTime(2000)
    expect(navigate).toHaveBeenCalledExactlyOnceWith('/knowledge')
    expect(onComplete).toHaveBeenCalledOnce()
  })

  it('opens the knowledge page for the reported request after showing the guide', () => {
    const navigate = vi.fn((href: string) => window.history.pushState({}, '', href)), onComplete = vi.fn()
    const guide = createAssistantNavigationGuide({ navigate, onComplete, onOpenChat: vi.fn() })
    expect(guide.start('kan du gå til kunskapsbasen og åpne den for meg')).toBe(true)
    expect(navigate).not.toHaveBeenCalled()
    vi.advanceTimersByTime(2000)
    expect(navigate).toHaveBeenCalledExactlyOnceWith('/knowledge')
    expect(onComplete).toHaveBeenCalledOnce()
  })

  it('never executes or reports an action for a changed user or organization', () => {
    let scope = 'user:org-one'
    const navigate = vi.fn(), onComplete = vi.fn(), onFailure = vi.fn()
    const guide = createAssistantNavigationGuide({ scope: () => scope, navigate, onComplete, onFailure, onOpenChat: vi.fn() })
    expect(guide.start('open knowledge')).toBe(true)
    scope = 'user:org-two'
    vi.advanceTimersByTime(2000)
    expect(navigate).not.toHaveBeenCalled()
    expect(onComplete).not.toHaveBeenCalled()
    expect(onFailure).not.toHaveBeenCalled()
  })

  it('reports a redirect or failed navigation instead of claiming completion', () => {
    const navigate = vi.fn(), onComplete = vi.fn(), onFailure = vi.fn()
    const guide = createAssistantNavigationGuide({ navigate, onComplete, onFailure, onOpenChat: vi.fn() })
    expect(guide.start('open knowledge')).toBe(true)
    vi.advanceTimersByTime(2000)
    expect(navigate).toHaveBeenCalledExactlyOnceWith('/knowledge')
    expect(onComplete).not.toHaveBeenCalled()
    expect(onFailure).toHaveBeenCalledExactlyOnceWith('Jeg kunne ikke åpne Kunnskap.')
  })

  it('pauses while its cursor menu is open and stops when the user opens chat', () => {
    const navigate = vi.fn(), onComplete = vi.fn(), onOpenChat = vi.fn()
    const guide = createAssistantNavigationGuide({ navigate, onComplete, onOpenChat })
    expect(guide.start('open knowledge')).toBe(true)
    guide.toggleMenu()
    vi.advanceTimersByTime(2500)
    expect(navigate).not.toHaveBeenCalled()
    expect(onComplete).not.toHaveBeenCalled()
    guide.openChat()
    expect(onOpenChat).toHaveBeenCalledOnce()
    vi.advanceTimersByTime(2500)
    expect(navigate).not.toHaveBeenCalled()
  })

  it('opens only the matched app page in a new tab on an explicit menu action', () => {
    const open = vi.spyOn(window, 'open').mockImplementation(() => null)
    const navigate = vi.fn(), onComplete = vi.fn(), onOpenChat = vi.fn()
    const guide = createAssistantNavigationGuide({ navigate, onComplete, onOpenChat })
    expect(guide.start('open knowledge')).toBe(true)
    guide.toggleMenu()
    guide.openInNewTab()
    expect(open).toHaveBeenCalledExactlyOnceWith('/knowledge', '_blank', 'noopener,noreferrer')
    expect(navigate).not.toHaveBeenCalled()
    open.mockRestore()
  })
})
