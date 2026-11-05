import { onCleanup, Show } from 'solid-js'
import { Portal } from '@solidjs/web'
import { createSignal } from '@/shared/lib/create-immediate-signal'
import { assistantNavigationActionSchema } from '@/shared/actions/assistant-navigation'
import './assistant-navigation.css'

export type AssistantDestination = {
  label: string
  href: string
  action: 'point' | 'navigate'
}

const destinations: Readonly<Record<string, { label: string; href: string }>> = {
  hjem: { label: 'Hjem', href: '/dashboard' }, home: { label: 'Hjem', href: '/dashboard' },
  dashboard: { label: 'Hjem', href: '/dashboard' }, chat: { label: 'Chat', href: '/chat' },
  rom: { label: 'Rom', href: '/spaces' }, spaces: { label: 'Rom', href: '/spaces' },
  support: { label: 'Support', href: '/support' },
  engasjement: { label: 'Engasjement', href: '/engagement' }, engagement: { label: 'Engasjement', href: '/engagement' },
  agenter: { label: 'Agenter', href: '/agents' }, agents: { label: 'Agenter', href: '/agents' },
  kunnskap: { label: 'Kunnskap', href: '/knowledge' }, kunnskapsbasen: { label: 'Kunnskap', href: '/knowledge' },
  // Common truncated spelling of "kunnskapsbasen", including a missing n.
  kunskaps: { label: 'Kunnskap', href: '/knowledge' }, kunnskaps: { label: 'Kunnskap', href: '/knowledge' },
  'kunskaps basen': { label: 'Kunnskap', href: '/knowledge' }, 'kunnskaps basen': { label: 'Kunnskap', href: '/knowledge' },
  knowledge: { label: 'Kunnskap', href: '/knowledge' }, 'knowledge base': { label: 'Kunnskap', href: '/knowledge' },
  innsikt: { label: 'Innsikt', href: '/insights' }, insights: { label: 'Innsikt', href: '/insights' },
}

/** Allow one edit only on sufficiently long names, and never guess between routes. */
function oneEditApart(a: string, b: string): boolean {
  if (Math.abs(a.length - b.length) > 1) return false
  let i = 0, j = 0, edits = 0
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) { i += 1; j += 1; continue }
    edits += 1
    if (edits > 1) return false
    if (a.length === b.length && a[i] === b[j + 1] && a[i + 1] === b[j]) { i += 2; j += 2 }
    else if (a.length > b.length) i += 1
    else if (b.length > a.length) j += 1
    else { i += 1; j += 1 }
  }
  return edits + (i < a.length || j < b.length ? 1 : 0) <= 1
}

function findDestination(text: string): { label: string; href: string } | null {
  const exact = Object.prototype.hasOwnProperty.call(destinations, text) ? destinations[text] : undefined
  if (exact) return exact
  // A generic page suffix describes an existing destination; it must never
  // turn other trailing words into an instruction or a different destination.
  const page = /^(.*?) (?:side|siden|sida|page)$/.exec(text)
  if (page) return findDestination(page[1]!)
  // Norwegian compound forms such as «kunnskapssiden» retain their genitive s.
  if (text.length >= 11 && text.endsWith('siden')) {
    const stem = text.slice(0, -'siden'.length)
    if (stem.length >= 6) return findDestination(stem)
  }
  // Small destination names are too easy to confuse with unrelated words.
  if (text.length < 6) return null
  const matches = Object.entries(destinations)
    .filter(([name]) => name.length >= 6 && oneEditApart(text, name))
    .map(([, destination]) => destination)
  const routes = new Set(matches.map((match) => match.href))
  return routes.size === 1 ? matches[0]! : null
}

/** A small, explicitly requested navigation action, never a model or page instruction. */
export function resolveAssistantNavigation(prompt: string): AssistantDestination | null {
  const text = prompt.toLocaleLowerCase().trim().replace(/[?!.]+$/, '').trim().replace(/\s+/g, ' ')
  // A repeated request to open the same destination is still one navigation
  // action. Keep the suffix narrowly scoped so additional tasks go to chat.
  const match = /^(?:(?:kan du|can you) )?(hvor (?:er|finner jeg)|where (?:is|can i find)|finn(?:e)?|find|åpne|gaa til|gå til|vis(?:e)? meg|show me|open|go to|take me to)\s+(?:the\s+)?([a-zæøå]+(?: [a-zæøå]+)?)(?: (?:for meg|for me))?(?: (og åpne (?:den|det)(?: for meg)?|and open it(?: for me)?))?$/.exec(text)
  if (!match) return null
  const target = findDestination(match[2]!)
  if (!target) return null
  const action = !match[3] && /^(hvor|where|finn|find|vis|show)/.test(match[1]!) ? 'point' : 'navigate'
  return { ...target, action }
}

type Position = { x: number; y: number }
type NavigationOptions = {
  /** User and organization identity; changes invalidate an in-flight action. */
  scope?: () => string | null
  navigate: (href: string) => void
  onComplete: (suggestion: string) => void
  /** Reports a failed route action without treating dispatch as completion. */
  onFailure?: (message: string) => void
  onOpenChat: () => void
  tr?: (norwegian: string, english: string) => string
}

const englishLabels: Readonly<Record<string, string>> = {
  '/dashboard': 'Home', '/chat': 'Chat', '/spaces': 'Spaces', '/support': 'Support',
  '/engagement': 'Engagement', '/agents': 'Agents', '/knowledge': 'Knowledge', '/insights': 'Insights',
}

/** Returns the real, currently visible rail link, not a guessed page coordinate. */
function findRailTarget(href: string): HTMLAnchorElement | null {
  const links = document.querySelectorAll<HTMLAnchorElement>('nav.core-sidebar__mini-nav a[href]')
  for (const link of links) {
    const url = new URL(link.href, window.location.href)
    const rect = link.getBoundingClientRect()
    if (url.origin === window.location.origin && url.pathname === href && rect.width > 0 && rect.height > 0 &&
      rect.right > 0 && rect.bottom > 0 && rect.left < window.innerWidth && rect.top < window.innerHeight) return link
  }
  return null
}

export function createAssistantNavigationGuide(options: NavigationOptions) {
  const tr = options.tr ?? ((norwegian: string) => norwegian)
  const [destination, setDestination] = createSignal<AssistantDestination | null>(null)
  const [position, setPosition] = createSignal<Position>({ x: 0, y: 0 })
  const [menuOpen, setMenuOpen] = createSignal(false)
  const [moving, setMoving] = createSignal(false)
  let timer: ReturnType<typeof setTimeout> | undefined
  let settle: ReturnType<typeof setTimeout> | undefined
  let generation = 0
  let pointer: Position | null = null
  const trackPointer = (event: PointerEvent) => { if (event.pointerType !== 'touch') pointer = { x: event.clientX, y: event.clientY } }

  // Teardown can run inside a Solid-owned scope, where writing signals is not
  // allowed. Dispose resources without mutating UI state during unmount.
  const dispose = () => {
    generation += 1
    if (timer !== undefined) clearTimeout(timer)
    if (settle !== undefined) clearTimeout(settle)
    timer = undefined
    settle = undefined
    if (typeof window !== 'undefined') window.removeEventListener('pointermove', trackPointer)
  }
  const stop = () => {
    dispose()
    setDestination(null)
    setMenuOpen(false)
    setMoving(false)
  }

  const canStart = (prompt: string): boolean => {
    const resolved = resolveAssistantNavigation(prompt)
    return Boolean(resolved && typeof window !== 'undefined' && findRailTarget(resolved.href))
  }

  const startResolved = (resolved: AssistantDestination, origin?: HTMLElement | null): boolean => {
    if (typeof window === 'undefined') return false
    const action = assistantNavigationActionSchema.safeParse({
      type: 'assistant.navigation.guide', href: resolved.href, mode: resolved.action,
    })
    if (!action.success) return false
    const next = { ...resolved, href: action.data.href, action: action.data.mode, label: tr(resolved.label, englishLabels[resolved.href] ?? resolved.label) }
    const target = findRailTarget(next.href)
    if (!target) return false
    stop()
    const run = generation
    const startingLocation = window.location.href
    const startingScope = options.scope?.()
    if (options.scope && !startingScope) return false
    const from = origin?.getBoundingClientRect()
    const to = target.getBoundingClientRect()
    pointer = null
    window.addEventListener('pointermove', trackPointer, { passive: true })
    setPosition({ x: Math.max(0, Math.min(window.innerWidth - 105, from ? from.left + from.width / 2 : window.innerWidth - 70)), y: from ? from.top + from.height / 2 : 30 })
    setDestination(next)
    setMoving(false)
    requestAnimationFrame(() => {
      if (run !== generation) return
      setMoving(true)
      setPosition({ x: to.left + to.width / 2, y: to.top + to.height / 2 })
    })
    timer = setTimeout(() => {
      if (run !== generation) return
      // A user's intervening navigation or a replaced target cancels the guide.
      if ((options.scope && options.scope() !== startingScope) || window.location.href !== startingLocation || !target.isConnected || findRailTarget(next.href) !== target) {
        stop()
        return
      }
      if (next.action === 'navigate') {
        try { options.navigate(next.href) } catch {
          stop()
          options.onFailure?.(tr(`Jeg kunne ikke åpne ${next.label}.`, `I could not open ${next.label}.`))
          return
        }
      }
      // This cursor represents the app-owned route action; it does not move or
      // hide the person's system pointer and is not Quarry browser telemetry.
      if (pointer) setPosition(pointer)
      settle = setTimeout(() => {
        if (run !== generation) return
        if (options.scope && options.scope() !== startingScope) { stop(); return }
        // A router dispatch is not proof that the destination opened: guards
        // can redirect, the route can fail, or the person can navigate away.
        const arrived = next.action !== 'navigate' || window.location.pathname.replace(/\/+$/, '') === next.href
        stop()
        if (!arrived) {
          options.onFailure?.(tr(`Jeg kunne ikke åpne ${next.label}.`, `I could not open ${next.label}.`))
          return
        }
        options.onComplete(tr(`Hva kan jeg gjøre i ${next.label}?`, `What can I do in ${next.label}?`))
      }, pointer ? 650 : next.action === 'navigate' ? 100 : 0)
    }, 1800)
    return true
  }

  // The model may propose only an action from the same fixed app-route contract.
  // Labels are resolved locally; model output cannot supply a URL or UI text.
  const startAction = (input: unknown, origin?: HTMLElement | null): boolean => {
    const parsed = assistantNavigationActionSchema.safeParse(input)
    if (!parsed.success) return false
    const target = Object.values(destinations).find((item) => item.href === parsed.data.href)
    if (!target) return false
    return startResolved({ ...target, action: parsed.data.mode }, origin)
  }

  const start = (prompt: string, origin?: HTMLElement | null): boolean => {
    const resolved = resolveAssistantNavigation(prompt)
    return resolved ? startResolved(resolved, origin) : false
  }

  const openChat = () => { stop(); options.onOpenChat() }
  const toggleMenu = () => {
    if (menuOpen()) { openChat(); return }
    if (timer !== undefined) clearTimeout(timer)
    timer = undefined
    setMenuOpen(true)
  }
  const openInNewTab = () => {
    const current = destination()
    if (!current) return
    window.open(current.href, '_blank', 'noopener,noreferrer')
    stop()
    options.onComplete(tr(`Hva kan jeg gjøre i ${current.label}?`, `What can I do in ${current.label}?`))
  }

  return { destination, position, menuOpen, moving, toggleMenu, canStart, start, startAction, stop, dispose, openChat, openInNewTab, tr }
}

export type AssistantNavigationGuide = ReturnType<typeof createAssistantNavigationGuide>

export function AssistantNavigationCursor(props: { guide: AssistantNavigationGuide }) {
  onCleanup(props.guide.dispose)
  return (
    <Show when={props.guide.destination()}>
      {(destination) => <Portal>
        <div class="assistant-guide-layer">
          <div class={props.guide.moving() ? 'assistant-guide-cursor assistant-guide-cursor--moving' : 'assistant-guide-cursor'}
            style={{ left: `${props.guide.position().x}px`, top: `${props.guide.position().y}px` }}>
            <button type="button" class="assistant-guide-pointer" aria-label={`Verevon: ${destination().label}`}
              aria-expanded={props.guide.menuOpen() ? 'true' : 'false'} onClick={props.guide.toggleMenu}>
              <svg viewBox="0 0 24 26" aria-hidden="true"><path d="M2 2l18 12-8 1-4 8z" /></svg>
              <span>Verevon</span>
            </button>
            <Show when={props.guide.menuOpen()}>
              <div class="assistant-guide-menu" role="group" aria-label="Verevon">
                <button type="button" onClick={props.guide.openChat}>{props.guide.tr('Stopp', 'Stop')}</button>
                <button type="button" onClick={props.guide.openChat}>{props.guide.tr('Åpne samtale', 'Open chat')}</button>
                <button type="button" onClick={props.guide.openInNewTab}>{props.guide.tr('Åpne siden i ny fane', 'Open page in new tab')}</button>
              </div>
            </Show>
          </div>
        </div>
      </Portal>}
    </Show>
  )
}
