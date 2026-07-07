import { Show, createEffect, createSignal, onCleanup } from 'solid-js'
import type { CheckoutSession } from '@/features/billing/lib/api'

type ConfirmedPayment = {
  paymentId?: string
  clientSecret?: string
  status: string
}

type NexiCheckoutProps = {
  confirming?: boolean
  returnUrl: string
  session: CheckoutSession
  onConfirmed: (payment: ConfirmedPayment) => void | Promise<void>
}

type NexiCheckoutInstance = {
  on: (event: string, handler: (payload?: unknown) => void) => void
  send?: (event: string, payload?: unknown) => void
  cleanup?: () => void
}

declare global {
  interface Window {
    Dibs?: {
      Checkout: (options: {
        checkoutKey: string
        paymentId: string
        containerId: string
        language?: string
        theme?: { textColor?: string; primaryColor?: string; linkColor?: string }
      }) => NexiCheckoutInstance
    }
  }
}

const scriptLoads = new Map<string, Promise<void>>()

function loadNexiScript(src: string): Promise<void> {
  const existing = scriptLoads.get(src)
  if (existing) return existing
  const load = new Promise<void>((resolve, reject) => {
    const script = document.createElement('script')
    script.type = 'text/javascript'
    script.src = src
    script.async = true
    script.onload = () => resolve()
    script.onerror = () => reject(new Error('Could not load payment checkout.'))
    document.body.appendChild(script)
  })
  scriptLoads.set(src, load)
  return load
}

function checkoutContainerId() {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return `nexi-${crypto.randomUUID()}`
  }
  return `nexi-${Date.now()}`
}

function amountLabel(session: CheckoutSession) {
  if (!session.amount_cents) return session.currency || 'NOK'
  return `${session.amount_cents / 100} ${session.currency || 'NOK'}`
}

/**
 * Embedded Nexi Checkout. Loads the Nexi Checkout JS SDK (session.client_url)
 * and initializes it with the public checkout key (session.publishable_key) +
 * the paymentId (session.payment_id). The SDK renders the full payment UI —
 * including its own pay button — inside the container iframe, so unlike the
 * Hyperswitch widget there is no separate confirm button here; we react to the
 * SDK's `payment-completed` event. The authoritative activation still happens
 * server-side via the Nexi webhook; onConfirmed advances the UI optimistically.
 */
export function NexiCheckout(props: NexiCheckoutProps) {
  const containerId = checkoutContainerId()
  const [loading, setLoading] = createSignal(true)
  const [message, setMessage] = createSignal<string>()

  let instance: NexiCheckoutInstance | undefined
  let mountedPayment: string | undefined

  createEffect(() => {
    const paymentId = props.session.payment_id || props.session.id
    const checkoutKey = props.session.publishable_key
    const clientURL = props.session.client_url
    // Capture the confirm callback in this tracked scope so the SDK event
    // handler (a non-tracked callback) closes over a stable reference.
    const onConfirmed = props.onConfirmed
    if (!paymentId || !checkoutKey || !clientURL || mountedPayment === paymentId) return

    mountedPayment = paymentId
    setLoading(true)
    setMessage(undefined)

    void loadNexiScript(clientURL)
      .then(() => {
        if (!window.Dibs) throw new Error('Payment checkout is unavailable.')

        instance = window.Dibs.Checkout({
          checkoutKey,
          paymentId,
          containerId,
          language: 'nb-NO',
          theme: { primaryColor: '#ff2e63' },
        })
        instance.on('pay-initialized', () => setLoading(false))
        instance.on('payment-completed', (payload) => {
          const completedId =
            (payload && typeof payload === 'object' && 'paymentId' in payload
              ? String((payload as { paymentId?: unknown }).paymentId ?? '')
              : '') || paymentId
          void onConfirmed({ paymentId: completedId, status: 'completed' })
        })
        setLoading(false)
      })
      .catch((reason: unknown) => {
        setLoading(false)
        setMessage(reason instanceof Error ? reason.message : 'Could not load payment checkout.')
      })
  })

  onCleanup(() => {
    instance?.cleanup?.()
  })

  return (
    <section class="velion-billing-checkout" aria-label="Payment checkout">
      <div class="velion-billing-checkout__header">
        <div>
          <strong>Sikker betaling</strong>
          <span>{amountLabel(props.session)}</span>
        </div>
        <Show when={props.confirming}>
          <small>Bekrefter...</small>
        </Show>
      </div>
      <div id={containerId} class="velion-billing-checkout__mount" aria-busy={loading()} />
      <Show when={loading()}>
        <p class="velion-billing-checkout__status">Laster betaling...</p>
      </Show>
      <Show when={message()}>
        {(text) => (
          <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
            {text()}
          </p>
        )}
      </Show>
    </section>
  )
}
