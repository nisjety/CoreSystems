import { createEffect, createSignal, onCleanup, Show } from 'solid-js'

type TurnstileChallengeProps = {
  siteKey: string
  locale: 'nb' | 'en'
  onToken: (token: string) => void
}

type TurnstileWidget = {
  render: (
    container: HTMLElement,
    options: {
      sitekey: string
      action?: string
      theme?: 'light' | 'dark' | 'auto'
      size?: 'normal' | 'compact' | 'flexible'
      callback?: (token: string) => void
      'expired-callback'?: () => void
      'error-callback'?: () => void
    },
  ) => string
  remove?: (widgetId: string) => void
  reset?: (widgetId: string) => void
}

declare global {
  interface Window {
    turnstile?: TurnstileWidget
  }
}

const TURNSTILE_SCRIPT_SRC = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit'
let turnstileScriptPromise: Promise<void> | null = null

function loadTurnstileScript(): Promise<void> {
  if (window.turnstile) return Promise.resolve()
  if (turnstileScriptPromise) return turnstileScriptPromise

  turnstileScriptPromise = new Promise((resolve, reject) => {
    const existing = document.querySelector<HTMLScriptElement>(`script[src="${TURNSTILE_SCRIPT_SRC}"]`)
    if (existing) {
      existing.addEventListener('load', () => resolve(), { once: true })
      existing.addEventListener('error', () => reject(new Error('Turnstile failed to load')), { once: true })
      return
    }

    const script = document.createElement('script')
    script.src = TURNSTILE_SCRIPT_SRC
    script.async = true
    script.defer = true
    script.addEventListener('load', () => resolve(), { once: true })
    script.addEventListener('error', () => reject(new Error('Turnstile failed to load')), { once: true })
    document.head.append(script)
  })

  return turnstileScriptPromise
}

export function TurnstileChallenge(props: TurnstileChallengeProps) {
  const [loadFailed, setLoadFailed] = createSignal(false)
  let containerRef: HTMLDivElement | undefined
  let widgetId: string | null = null

  createEffect(() => {
    const siteKey = props.siteKey
    const onToken = props.onToken
    let disposed = false

    if (!siteKey || !containerRef || widgetId) return

    void loadTurnstileScript()
      .then(() => {
        if (disposed || !containerRef || !window.turnstile) return
        widgetId = window.turnstile.render(containerRef, {
          sitekey: siteKey,
          action: 'signup',
          theme: 'light',
          size: 'flexible',
          callback: (token) => onToken(token),
          'expired-callback': () => onToken(''),
          'error-callback': () => onToken(''),
        })
      })
      .catch(() => {
        if (disposed) return
        onToken('')
        setLoadFailed(true)
      })

    onCleanup(() => {
      disposed = true
      onToken('')
      if (widgetId && window.turnstile?.remove) {
        window.turnstile.remove(widgetId)
        widgetId = null
        return
      }
      if (widgetId && window.turnstile?.reset) {
        window.turnstile.reset(widgetId)
      }
      widgetId = null
    })
  })

  return (
    <div class="auth-captcha" aria-label={props.locale === 'nb' ? 'Sikkerhetssjekk' : 'Security check'}>
      <div ref={containerRef} class="auth-captcha__widget" />
      <Show when={loadFailed()}>
        <p class="auth-captcha__error">
          {props.locale === 'nb' ? 'Sikkerhetssjekken kunne ikke lastes.' : 'Security check could not load.'}
        </p>
      </Show>
    </div>
  )
}
