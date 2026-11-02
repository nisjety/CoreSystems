/**
 * Privacy guard of the interactive demo. main.tsx imports this module first, so it
 * runs before every other module: it installs the fixed clock, in-memory storage and
 * replacements for each browser API that could reach out, persist or move the page.
 * Decisions come from guard-policy.ts. Every blocked attempt is logged with console.error
 * (prefix GUARD_LOG_PREFIX) and recorded in window.__demoBlocked for the probe.
 */
import { installFixedClock } from '@/demo/clock'
import { DEMO_NOW_ISO } from '@/demo/live/shared-types'
import { DEMO_UNAVAILABLE_BODY, GUARD_LOG_PREFIX, classifyRequest, classifyTransport, type TransportKind } from './guard-policy'

declare global {
  interface Window {
    __demoBlocked?: string[]
  }
}

const documentBaseUrl = document.baseURI
const demoBase: string = import.meta.env.BASE_URL

function report(message: string, level: 'error' | 'warn'): void {
  const log = (window.__demoBlocked ??= [])
  log.push(message)
  const line = `${GUARD_LOG_PREFIX} ${message}`
  if (level === 'error') console.error(line)
  else console.warn(line)
}

function originOf(value: string | URL | undefined): string {
  try {
    return new URL(String(value ?? ''), documentBaseUrl).origin
  } catch {
    return 'invalid URL'
  }
}

/** Map-backed Storage with the surface the product uses. Nothing is written to the device. */
class MemoryStorage {
  private readonly items = new Map<string, string>()

  get length(): number {
    return this.items.size
  }

  clear(): void {
    this.items.clear()
  }

  getItem(key: string): string | null {
    return this.items.get(String(key)) ?? null
  }

  key(index: number): string | null {
    return Array.from(this.items.keys())[index] ?? null
  }

  removeItem(key: string): void {
    this.items.delete(String(key))
  }

  setItem(key: string, value: string): void {
    this.items.set(String(key), String(value))
  }
}

function installStorage(): void {
  const local = new MemoryStorage()
  const session = new MemoryStorage()
  Object.defineProperty(window, 'localStorage', { configurable: true, get: () => local })
  Object.defineProperty(window, 'sessionStorage', { configurable: true, get: () => session })
  // Cookies and IndexedDB are inert: reads come back empty and writes go nowhere.
  Object.defineProperty(Document.prototype, 'cookie', { configurable: true, get: () => '', set: () => undefined })
  Object.defineProperty(window, 'indexedDB', { configurable: true, get: () => undefined })
}

function installFetchGuard(): void {
  const nativeFetch = window.fetch.bind(window)
  window.fetch = (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = input instanceof Request ? input.url : String(input)
    const method = init?.method ?? (input instanceof Request ? input.method : 'GET')
    const verdict = classifyRequest({ url, method }, documentBaseUrl, demoBase)
    if (verdict.action === 'allow') return nativeFetch(input, init)
    if (verdict.action === 'unhandled-api') {
      report(`no responder for ${verdict.path}`, 'warn')
      return Promise.resolve(
        new Response(JSON.stringify(DEMO_UNAVAILABLE_BODY), {
          status: 404,
          headers: { 'content-type': 'application/json' },
        }),
      )
    }
    report(`request blocked: ${verdict.reason}`, 'error')
    return Promise.reject(new TypeError(`${GUARD_LOG_PREFIX} request blocked`))
  }
}

function installXhrGuard(): void {
  const proto = XMLHttpRequest.prototype
  const nativeOpen = proto.open
  const nativeSend = proto.send
  const refused = new WeakSet<XMLHttpRequest>()
  proto.open = function open(this: XMLHttpRequest, method: string, url: string | URL, ...rest: unknown[]): void {
    const verdict = classifyRequest({ url: String(url), method }, documentBaseUrl, demoBase)
    if (verdict.action === 'allow') {
      Reflect.apply(nativeOpen, this, [method, url, ...rest])
      return
    }
    const reason = verdict.action === 'block' ? verdict.reason : `no responder for ${verdict.path}`
    report(`XHR blocked: ${reason}`, 'error')
    refused.add(this)
    // Open against the demo's own document so later setRequestHeader calls stay valid. Nothing is sent.
    Reflect.apply(nativeOpen, this, ['GET', documentBaseUrl])
  }
  proto.send = function send(this: XMLHttpRequest, body?: Document | XMLHttpRequestBodyInit | null): void {
    if (!refused.has(this)) {
      Reflect.apply(nativeSend, this, [body])
      return
    }
    setTimeout(() => {
      this.dispatchEvent(new Event('error'))
      this.dispatchEvent(new Event('loadend'))
    }, 0)
  }
}

/** A socket that never connects: it reports the attempt and then fails, as an unreachable server would. */
class InertSocket extends EventTarget {
  onopen: ((event: Event) => void) | null = null
  onmessage: ((event: Event) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  onclose: ((event: Event) => void) | null = null
  readonly url: string
  readyState: number

  constructor(kind: TransportKind, url: string | URL) {
    super()
    this.url = String(url)
    this.readyState = kind === 'websocket' ? 3 : 2
    const verdict = classifyTransport(kind, this.url, documentBaseUrl)
    if (verdict.action === 'block') report(verdict.reason, 'error')
    setTimeout(() => this.fail(), 0)
  }

  close(): void {
    // Nothing is open, so there is nothing to close.
  }

  private fail(): void {
    const error = new Event('error')
    this.dispatchEvent(error)
    this.onerror?.(error)
    const closed = new Event('close')
    this.dispatchEvent(closed)
    this.onclose?.(closed)
  }
}

function installTransportGuards(): void {
  window.WebSocket = class extends InertSocket {
    constructor(url: string | URL) {
      super('websocket', url)
    }
  } as unknown as typeof WebSocket
  window.EventSource = class extends InertSocket {
    constructor(url: string | URL) {
      super('eventsource', url)
    }
  } as unknown as typeof EventSource
  Object.defineProperty(navigator, 'sendBeacon', {
    configurable: true,
    value: (url: string | URL): boolean => {
      const verdict = classifyTransport('beacon', String(url), documentBaseUrl)
      if (verdict.action === 'block') report(verdict.reason, 'error')
      return false
    },
  })
  window.open = (url?: string | URL): null => {
    report(`window.open blocked: ${originOf(url)}`, 'error')
    return null
  }
}

function installDeviceGuards(): void {
  // Location: the same denial a visitor gets when they refuse, delivered at once. The
  // weather fixtures already carry a fixed place, so no coordinates are needed.
  const unavailable = (): GeolocationPositionError =>
    ({
      code: 1,
      message: 'Posisjon er ikke tilgjengelig i demoen.',
      PERMISSION_DENIED: 1,
      POSITION_UNAVAILABLE: 2,
      TIMEOUT: 3,
    }) as unknown as GeolocationPositionError
  try {
    Object.defineProperty(navigator, 'geolocation', {
      configurable: true,
      value: {
        getCurrentPosition(_success: PositionCallback, error?: PositionErrorCallback | null): void {
          setTimeout(() => error?.(unavailable()), 0)
        },
        watchPosition(_success: PositionCallback, error?: PositionErrorCallback | null): number {
          setTimeout(() => error?.(unavailable()), 0)
          return 0
        },
        clearWatch(): void {
          // No watch was started.
        },
      },
    })
  } catch (error) {
    // A browser whose `geolocation` is non-configurable keeps its own; the demo must still boot.
    if (Object.getOwnPropertyDescriptor(navigator, 'geolocation')?.configurable !== false) throw error
  }

  // Clipboard: in memory, so copy buttons work and nothing leaves the page.
  let clipboardText = ''
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: {
      writeText: async (text: string): Promise<void> => {
        clipboardText = String(text)
      },
      readText: async (): Promise<string> => clipboardText,
      write: async (): Promise<void> => undefined,
      read: async (): Promise<ClipboardItem[]> => [],
    },
  })

  // Camera and microphone: refused, as when a visitor says no.
  const refuseDevice = (): Promise<never> => {
    report('camera or microphone blocked', 'error')
    return Promise.reject(new DOMException('Mikrofon og kamera er ikke tilgjengelig i demoen.', 'NotAllowedError'))
  }
  Object.defineProperty(navigator, 'mediaDevices', {
    configurable: true,
    value: {
      getUserMedia: refuseDevice,
      getDisplayMedia: refuseDevice,
      enumerateDevices: async (): Promise<MediaDeviceInfo[]> => [],
      addEventListener: (): void => undefined,
      removeEventListener: (): void => undefined,
    },
  })

  // Service workers: the demo never installs one.
  Object.defineProperty(navigator, 'serviceWorker', {
    configurable: true,
    value: {
      controller: null,
      ready: new Promise<never>(() => undefined),
      register: (): Promise<never> => {
        report('service worker blocked', 'error')
        return Promise.reject(new DOMException('Ikke tilgjengelig i demoen.', 'SecurityError'))
      },
      getRegistrations: async (): Promise<[]> => [],
      addEventListener: (): void => undefined,
      removeEventListener: (): void => undefined,
    },
  })
}

/**
 * The sandbox has no allow-forms, so the browser drops a form submission before it fires
 * `submit`. The product's composers send on submit, so without this they would do nothing.
 * This shim delivers the `submit` event itself. The app's handler runs and cancels it, as it
 * does in the product. Nothing can navigate: the browser never submits a form in this frame,
 * and the CSP says so too (form-action 'none').
 */
function installFormSubmitShim(): void {
  document.addEventListener('click', (event) => {
    if (event.button !== 0 || event.defaultPrevented || !(event.target instanceof Element)) return
    const control = event.target.closest('button, input[type="submit"], input[type="image"]')
    if (!(control instanceof HTMLButtonElement || control instanceof HTMLInputElement)) return
    if (control.disabled || !control.form) return
    if (control instanceof HTMLInputElement && control.type !== 'submit' && control.type !== 'image') return
    if (control instanceof HTMLButtonElement && control.type !== 'submit') return
    event.preventDefault()
    control.form.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true, submitter: control }))
  })
  HTMLFormElement.prototype.requestSubmit = function requestSubmit(this: HTMLFormElement, submitter?: HTMLElement | null): void {
    this.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true, submitter: submitter ?? null }))
  }
}

function isScrollable(element: Element): boolean {
  return /(auto|scroll)/.test(getComputedStyle(element).overflowY) && element.scrollHeight > element.clientHeight
}

function verticalOffset(target: { top: number; bottom: number }, box: { top: number; bottom: number }, block: ScrollLogicalPosition): number {
  if (block === 'start') return target.top - box.top
  if (block === 'end') return target.bottom - box.bottom
  if (block === 'center') return (target.top + target.bottom) / 2 - (box.top + box.bottom) / 2
  if (target.top < box.top) return target.top - box.top
  if (target.bottom > box.bottom) return target.bottom - box.bottom
  return 0
}

function installScrollSafety(): void {
  // A focus inside the frame must not scroll the embedding page.
  const nativeFocus = HTMLElement.prototype.focus
  HTMLElement.prototype.focus = function focus(this: HTMLElement, options?: FocusOptions): void {
    nativeFocus.call(this, options ?? { preventScroll: true })
  }
  // Native scrollIntoView also scrolls the embedding page. Scroll only the containers
  // inside this document, then this document's own viewport.
  Element.prototype.scrollIntoView = function scrollIntoView(this: Element, options?: boolean | ScrollIntoViewOptions): void {
    const block: ScrollLogicalPosition = typeof options === 'object' && options.block ? options.block : 'start'
    const behavior: ScrollBehavior = typeof options === 'object' && options.behavior ? options.behavior : 'auto'
    let container = this.parentElement
    while (container && container !== document.documentElement) {
      if (isScrollable(container)) {
        const top = verticalOffset(this.getBoundingClientRect(), container.getBoundingClientRect(), block)
        container.scrollBy({ top, behavior })
      }
      container = container.parentElement
    }
    const top = verticalOffset(this.getBoundingClientRect(), { top: 0, bottom: window.innerHeight }, block)
    window.scrollBy({ top, behavior })
  }
}

installFixedClock(DEMO_NOW_ISO)
installStorage()
installFetchGuard()
installXhrGuard()
installTransportGuards()
installDeviceGuards()
installFormSubmitShim()
installScrollSafety()
