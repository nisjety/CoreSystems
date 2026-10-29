import { Show, createEffect, createSignal, onCleanup } from 'solid-js'
import type { CheckoutSession } from '@/features/billing/lib/api'
import { Button } from '@/shared/ui/Button'
import { translateApiError, useI18n } from '@/shared/i18n'

type ConfirmedPayment = {
  paymentId?: string
  clientSecret?: string
  status: string
}

type HyperswitchCheckoutProps = {
  confirming?: boolean
  returnUrl: string
  session: CheckoutSession
  onConfirmed: (payment: ConfirmedPayment) => void | Promise<void>
}

type HyperWidget = {
  destroy?: () => void
  mount: (selector: string) => void
  unmount?: () => void
}

type HyperWidgets = {
  create: (type: 'payment', options: Record<string, unknown>) => HyperWidget
  getElements?: () => unknown
}

type HyperInstance = {
  confirmPayment: (options: Record<string, unknown>) => Promise<{
    error?: { message?: string; type?: string }
    status?: string
  }>
  widgets: (options: { appearance: Record<string, unknown>; clientSecret: string }) => HyperWidgets
}

declare global {
  interface Window {
    Hyper?: (publishableKey: string, options?: { customBackendUrl?: string }) => HyperInstance
  }
}

const scriptLoads = new Map<string, Promise<void>>()

function loadHyperScript(src: string): Promise<void> {
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
    return `hyperswitch-${crypto.randomUUID()}`
  }
  return `hyperswitch-${Date.now()}`
}

function amountLabel(session: CheckoutSession) {
  if (!session.amount_cents) return session.currency || 'NOK'
  return `${session.amount_cents / 100} ${session.currency || 'NOK'}`
}

export function HyperswitchCheckout(props: HyperswitchCheckoutProps) {
  const i18n = useI18n()
  const containerId = checkoutContainerId()
  const [loading, setLoading] = createSignal(true)
  const [submitting, setSubmitting] = createSignal(false)
  const [message, setMessage] = createSignal<string>()

  let hyper: HyperInstance | undefined
  let widgets: HyperWidgets | undefined
  let checkout: HyperWidget | undefined
  let mountedSecret: string | undefined

  createEffect(() => {
    const clientSecret = props.session.client_secret
    const publishableKey = props.session.publishable_key
    const clientURL = props.session.client_url
    const backendURL = props.session.backend_url
    const returnURL = props.returnUrl
    if (!clientSecret || !publishableKey || !clientURL || mountedSecret === clientSecret) return

    mountedSecret = clientSecret
    setLoading(true)
    setMessage(undefined)

    void loadHyperScript(clientURL)
      .then(() => {
        if (!window.Hyper) throw new Error('Payment checkout is unavailable.')

        checkout?.destroy?.()
        hyper = window.Hyper(publishableKey, {
          customBackendUrl: backendURL,
        })
        widgets = hyper.widgets({
          appearance: {
            theme: 'midnight',
            variables: {
              colorPrimary: '#ff2e63',
              borderRadius: '8px',
            },
          },
          clientSecret,
        })
        checkout = widgets.create('payment', {
          layout: 'tabs',
          wallets: {
            walletReturnUrl: returnURL,
          },
        })
        checkout.mount(`#${containerId}`)
        setLoading(false)
      })
      .catch((reason: unknown) => {
        setLoading(false)
        setMessage(
          translateApiError(reason, i18n.tr, {
            no: 'Kunne ikke laste betalingsløsningen.',
            en: 'Could not load payment checkout.',
          }),
        )
      })
  })

  onCleanup(() => {
    checkout?.destroy?.()
    checkout?.unmount?.()
  })

  async function confirmPayment() {
    if (!hyper || !widgets || submitting() || props.confirming) return

    setSubmitting(true)
    setMessage(undefined)
    try {
      const elements = widgets.getElements?.()
      const result = await hyper.confirmPayment({
        ...(elements ? { elements } : { widgets }),
        confirmParams: {
          return_url: props.returnUrl,
        },
        redirect: 'if_required',
      })

      if (result.error) {
        setMessage(
          translateApiError(result.error, i18n.tr, {
            no: 'Betalingen feilet.',
            en: 'Payment failed.',
          }),
        )
        return
      }

      await props.onConfirmed({
        paymentId: props.session.payment_id || props.session.id,
        clientSecret: props.session.client_secret,
        status: result.status || 'processing',
      })
    } catch (reason) {
      setMessage(
        translateApiError(reason, i18n.tr, {
          no: 'Betalingen feilet.',
          en: 'Payment failed.',
        }),
      )
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <section class="verevon-billing-checkout" aria-label="Payment checkout">
      <div class="verevon-billing-checkout__header">
        <div>
          <strong>Sikker betaling</strong>
          <span>{amountLabel(props.session)}</span>
        </div>
        <Show when={props.session.status}>
          {(status) => <small>{status()}</small>}
        </Show>
      </div>
      <div id={containerId} class="verevon-billing-checkout__mount" aria-busy={loading()} />
      <Show when={loading()}>
        <p class="verevon-billing-checkout__status">Laster betaling...</p>
      </Show>
      <Show when={message()}>
        {(text) => <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">{text()}</p>}
      </Show>
      <div class="verevon-billing-checkout__action">
        <Button
          variant="primary"
          size="sm"
          disabled={loading() || submitting() || props.confirming}
          onClick={() => void confirmPayment()}
        >
          {submitting() || props.confirming ? 'Bekrefter...' : 'Betal og aktiver'}
        </Button>
      </div>
    </section>
  )
}
