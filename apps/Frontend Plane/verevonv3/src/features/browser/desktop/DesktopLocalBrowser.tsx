import { createEffect, createSignal, Show, untrack } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { webAddress } from '@/features/dashboard/home/web-address'
import type { DesktopBrowserBounds, DesktopBrowserBridge, DesktopBrowserCommand, DesktopBrowserState } from './desktop-browser'
import './desktop-local-browser.css'

function nativeBounds(slot: HTMLElement, assistant: Element | null): DesktopBrowserBounds | null {
  const rect = slot.getBoundingClientRect()
  const stageRect = slot.closest('.verevon-home-stage')?.getBoundingClientRect()
  const assistantRect = assistant?.getBoundingClientRect()
  const tabsRect = slot.closest('.dashboard-home')?.querySelector('.verevon-home-tabs')?.getBoundingClientRect()
  const left = Math.max(0, Math.ceil(rect.left), Math.ceil(stageRect?.left ?? 0))
  const top = Math.max(0, Math.ceil(rect.top), Math.ceil(stageRect?.top ?? 0), Math.ceil(tabsRect?.bottom ?? 0))
  const right = Math.floor(Math.min(window.innerWidth, rect.right, stageRect?.right ?? window.innerWidth))
  // A native browser view is above the renderer, including fixed HTML panels.
  // Keep the whole assistant panel reachable while the same guest stays mounted.
  const assistantTop = assistantRect && assistantRect.width > 0 && assistantRect.height > 0
    && assistantRect.left < right && assistantRect.right > left
    ? Math.floor(assistantRect.top) - 8
    : window.innerHeight
  const bottom = Math.floor(Math.min(window.innerHeight, assistantTop, rect.bottom, stageRect?.bottom ?? window.innerHeight))
  return right > left && bottom > top ? { x: left, y: top, width: right - left, height: bottom - top } : null
}

export function DesktopLocalBrowser(props: {
  bridge: DesktopBrowserBridge
  agentBusy?: boolean
  stopping?: boolean
  navigationError?: string | null
  onManualCommand?: () => void
  onStop: () => void
}) {
  const i18n = useI18n()
  const bridge = untrack(() => props.bridge)
  const [state, setState] = createSignal<DesktopBrowserState | null>(null)
  const [address, setAddress] = createSignal('')
  const [action, setAction] = createSignal<string | null>(null)
  const [error, setError] = createSignal<string | null>(null)
  let slot!: HTMLDivElement
  let addressInput!: HTMLInputElement
  let disposed = false

  const displayAddress = (url: string) => url === 'about:blank' ? '' : url
  const acceptState = (next: DesktopBrowserState) => untrack(() => {
    if (disposed) return
    const previous = state()
    setState(next)
    if (document.activeElement !== addressInput || address() === displayAddress(previous?.url ?? '')) {
      setAddress(displayAddress(next.url))
    }
  })

  createEffect(() => undefined, () => {
    if (typeof window === 'undefined') return
    let stateReceived = false
    const unsubscribe = bridge.subscribe(event => {
      if (event.type === 'state') {
        stateReceived = true
        acceptState(event)
      } else if (event.type === 'cursor' && !disposed) {
        setAction(event.action)
      }
    })
    void bridge.getState().then(next => {
      if (!stateReceived) acceptState(next)
    }).catch(() => { if (!disposed) setError(i18n.tr('Nettleseren er ikke tilgjengelig.', 'The browser is unavailable.')) })

    let frame = 0
    let lastBounds = ''
    let assistant: Element | null = null
    const schedule = () => {
      if (disposed || frame) return
      frame = window.requestAnimationFrame(() => {
        frame = 0
        const bounds = nativeBounds(slot, assistant)
        const key = JSON.stringify(bounds)
        if (key === lastBounds) return
        lastBounds = key
        void bridge.setViewport(bounds).catch(() => {
          if (!disposed) setError(i18n.tr('Kunne ikke vise nettleseren.', 'Could not display the browser.'))
        })
      })
    }
    const resize = new ResizeObserver(schedule)
    resize.observe(slot)
    const observeAssistant = () => {
      const next = document.querySelector('.global-assistant')
      if (next === assistant) return
      if (assistant) resize.unobserve(assistant)
      assistant = next
      if (assistant) resize.observe(assistant)
      schedule()
    }
    const dock = new MutationObserver(observeAssistant)
    dock.observe(document.body, { childList: true, subtree: true })
    observeAssistant()
    window.addEventListener('resize', schedule)
    window.addEventListener('scroll', schedule, true)
    schedule()

    return () => {
      disposed = true
      unsubscribe()
      dock.disconnect()
      resize.disconnect()
      window.removeEventListener('resize', schedule)
      window.removeEventListener('scroll', schedule, true)
      window.cancelAnimationFrame(frame)
      void bridge.setViewport(null).catch(() => undefined)
    }
  })

  createEffect(() => props.agentBusy, () => { setAction(null) })

  const run = async (command: () => Promise<unknown>) => {
    if (props.agentBusy) return
    props.onManualCommand?.()
    setError(null)
    try {
      await command()
    } catch {
      if (!disposed) setError(i18n.tr('Nettleseren kunne ikke utføre handlingen.', 'The browser could not perform that action.'))
    }
  }
  const navigate = () => {
    const initialUrl = webAddress(address())
    if (!initialUrl) {
      setError(i18n.tr('Skriv inn en gyldig nettadresse.', 'Enter a valid web address.'))
      return
    }
    addressInput.blur()
    void run(() => bridge.navigate(initialUrl))
  }
  const command = (name: DesktopBrowserCommand) => void run(() => bridge.command(name))
  const activity = () => {
    if (props.stopping) return i18n.tr('Stopper Verevon …', 'Stopping Verevon …')
    if (!props.agentBusy) return i18n.tr('Du styrer nettleseren', 'You control the browser')
    if (state()?.isLoading || action() === 'navigate') return i18n.tr('Verevon åpner siden', 'Verevon is opening the page')
    if (action() === 'click') return i18n.tr('Verevon klikker på siden', 'Verevon is clicking on the page')
    if (action() === 'fill') return i18n.tr('Verevon fyller ut et felt', 'Verevon is filling in a field')
    if (action() === 'scroll') return i18n.tr('Verevon ruller på siden', 'Verevon is scrolling the page')
    return i18n.tr('Verevon undersøker siden', 'Verevon is inspecting the page')
  }

  return (
    <section class="desktop-local-browser" aria-label={i18n.tr('Lokal nettleser', 'Local browser')}>
      <form class="desktop-local-browser__toolbar" onSubmit={event => { event.preventDefault(); navigate() }}>
        <button type="button" aria-label={i18n.tr('Tilbake', 'Back')} disabled={props.agentBusy || !state()?.canGoBack} onClick={() => command('back')}>←</button>
        <button type="button" aria-label={i18n.tr('Frem', 'Forward')} disabled={props.agentBusy || !state()?.canGoForward} onClick={() => command('forward')}>→</button>
        <button type="button" aria-label={state()?.isLoading ? i18n.tr('Stopp innlasting', 'Stop loading') : i18n.tr('Last på nytt', 'Reload')} disabled={props.agentBusy || !displayAddress(state()?.url ?? '')} onClick={() => command(state()?.isLoading ? 'stop' : 'reload')}>
          {state()?.isLoading ? '×' : '↻'}
        </button>
        <input ref={addressInput} value={address()} disabled={props.agentBusy} onInput={event => setAddress(event.currentTarget.value)}
          aria-label={i18n.tr('Nettadresse', 'Web address')} placeholder={i18n.tr('Skriv en nettadresse', 'Enter a web address')}
          spellcheck={false} autocapitalize="off" autocomplete="off" />
        <button type="submit" disabled={props.agentBusy || !address().trim()}>{i18n.tr('Åpne', 'Open')}</button>
      </form>
      <div class="desktop-local-browser__status">
        <span role="status">{activity()}</span>
        <Show when={props.agentBusy}>
          <button type="button" disabled={props.stopping} onClick={() => props.onStop()}>{i18n.tr('Stopp og ta over fanen', 'Stop and take over tab')}</button>
        </Show>
      </div>
      <Show when={error() || props.navigationError || state()?.loadError}>{message => <p class="desktop-local-browser__error" role="alert">{message()}</p>}</Show>
      <div ref={slot} class="desktop-local-browser__viewport" aria-hidden="true" />
    </section>
  )
}
