import { createEffect, createSignal, onCleanup, Show } from 'solid-js'
import {
  disconnectChatGptSubscription,
  getChatGptSubscriptionStatus,
  listChatGptSubscriptions,
  startChatGptSubscription,
  type ChatGptDeviceLogin,
  type ChatGptSubscriptionConnection,
} from '@/shared/api/chatgpt-subscription-client'
import { useI18n } from '@/shared/i18n'
import { Button } from '@/shared/ui/Button'

type ChatGptSubscriptionPanelProps = {
  orgId: string
  /** Used by Settings to refresh its live integration metrics. */
  onConnectionChange?: () => void
  variant?: 'settings' | 'onboarding'
}

function isConnected(status: string): boolean {
  return ['active', 'connected'].includes(status.trim().toLowerCase())
}

function isTerminalLogin(status: ChatGptDeviceLogin['status']): boolean {
  return status === 'connected' || status === 'failed' || status === 'expired'
}

/**
 * The only browser-side subscription surface. It intentionally never accepts
 * an API key or password: Integration Core owns the device-code exchange and
 * stores the provider credential behind an opaque connection reference.
 */
export function ChatGptSubscriptionPanel(props: ChatGptSubscriptionPanelProps) {
  const i18n = useI18n()
  const [connection, setConnection] = createSignal<ChatGptSubscriptionConnection>()
  const [login, setLogin] = createSignal<ChatGptDeviceLogin>()
  const [busy, setBusy] = createSignal<'starting' | 'disconnecting' | null>(null)
  const [notice, setNotice] = createSignal<string | null>(null)
  let pollTimer: number | undefined

  const stopPolling = () => {
    if (pollTimer !== undefined) window.clearTimeout(pollTimer)
    pollTimer = undefined
  }

  const notifyConnectionChange = () => props.onConnectionChange?.()

  const refreshConnection = async (orgId: string) => {
    if (!orgId.trim()) return
    try {
      const subscriptions = await listChatGptSubscriptions(orgId)
      if (orgId !== props.orgId) return
      const active = subscriptions.find((candidate) => isConnected(candidate.status))
      if (active) setConnection(active)
    } catch {
      // This panel remains usable if the generic list projection is temporarily
      // unavailable; starting a fresh device flow still has its own endpoint.
    }
  }

  const finishLogin = async (status: 'connected' | 'failed' | 'expired', nextConnection: ChatGptSubscriptionConnection) => {
    stopPolling()
    setConnection(nextConnection)
    setLogin(undefined)
    if (status === 'connected') {
      setNotice(i18n.tr('ChatGPT-abonnementet er koblet til og klart for AI-kjøringer.', 'Your ChatGPT subscription is connected and ready for AI runs.'))
      notifyConnectionChange()
      await refreshConnection(props.orgId)
      return
    }
    setNotice(status === 'expired'
      ? i18n.tr('Påloggingskoden utløp. Start på nytt for å få en ny kode.', 'The sign-in code expired. Start again for a new code.')
      : i18n.tr('ChatGPT-påloggingen ble ikke fullført. Prøv igjen.', 'ChatGPT sign-in was not completed. Please try again.'))
  }

  const pollLogin = async () => {
    const currentLogin = login()
    const targetOrgId = props.orgId
    if (!currentLogin || !targetOrgId.trim()) return
    try {
      const result = await getChatGptSubscriptionStatus(
        targetOrgId,
        currentLogin.connectionId,
        currentLogin.loginId,
      )
      if (targetOrgId !== props.orgId || login()?.loginId !== currentLogin.loginId) return
      setConnection(result.connection)
      setLogin(result.login)

      const loginStatus = result.login.status
      if (loginStatus === 'connected' || loginStatus === 'failed' || loginStatus === 'expired') {
        await finishLogin(loginStatus, result.connection)
        return
      }
      pollTimer = window.setTimeout(() => void pollLogin(), 2_500)
    } catch {
      // Retain the displayed code and retry once the temporary gateway/network
      // failure has cleared. Never surface a provider's raw auth response.
      setNotice(i18n.tr('Venter fortsatt på at ChatGPT-påloggingen skal fullføres.', 'Still waiting for ChatGPT sign-in to finish.'))
      pollTimer = window.setTimeout(() => void pollLogin(), 4_000)
    }
  }

  const start = async () => {
    const targetOrgId = props.orgId
    if (!targetOrgId.trim() || busy()) return
    // Reserve the external window in the user gesture. Otherwise browsers may
    // block it after the asynchronous start request returns.
    const loginWindow = typeof window === 'undefined'
      ? null
      : window.open('about:blank', 'verevon-chatgpt-subscription', 'popup,width=560,height=760')
    stopPolling()
    setBusy('starting')
    setNotice(null)
    try {
      const result = await startChatGptSubscription(targetOrgId)
      if (targetOrgId !== props.orgId) {
        if (loginWindow && !loginWindow.closed) loginWindow.close()
        return
      }
      setConnection(result.connection)
      setLogin(result.login)
      if (loginWindow && !loginWindow.closed) loginWindow.location.replace(result.login.verificationUrl)
      pollTimer = window.setTimeout(() => void pollLogin(), 1_000)
    } catch {
      if (loginWindow && !loginWindow.closed) loginWindow.close()
      setNotice(i18n.tr('Kunne ikke starte ChatGPT-påloggingen. Prøv igjen.', 'We could not start ChatGPT sign-in. Please try again.'))
    } finally {
      setBusy(null)
    }
  }

  const disconnect = async () => {
    const currentConnection = connection()
    if (!currentConnection || !props.orgId.trim() || busy()) return
    stopPolling()
    setBusy('disconnecting')
    setNotice(null)
    try {
      await disconnectChatGptSubscription(props.orgId, currentConnection.id)
      setConnection(undefined)
      setLogin(undefined)
      setNotice(i18n.tr('ChatGPT-abonnementet er koblet fra.', 'Your ChatGPT subscription has been disconnected.'))
      notifyConnectionChange()
    } catch {
      setNotice(i18n.tr('Kunne ikke koble fra ChatGPT-abonnementet. Prøv igjen.', 'We could not disconnect the ChatGPT subscription. Please try again.'))
    } finally {
      setBusy(null)
    }
  }

  createEffect(
    () => props.orgId,
    (orgId) => {
      stopPolling()
      setConnection(undefined)
      setLogin(undefined)
      setNotice(null)
      void refreshConnection(orgId)
    },
  )

  onCleanup(stopPolling)

  const connected = () => Boolean(connection() && isConnected(connection()!.status))
  const awaitingSignIn = () => Boolean(login() && !isTerminalLogin(login()!.status))
  const statusLabel = () => {
    if (connected()) return i18n.tr('Tilkoblet', 'Connected')
    if (awaitingSignIn()) return i18n.tr('Venter på pålogging', 'Waiting for sign-in')
    return i18n.tr('Ikke tilkoblet', 'Not connected')
  }

  return (
    <section
      class={['verevon-chatgpt-subscription', `verevon-chatgpt-subscription--${props.variant ?? 'settings'}`]}
      aria-labelledby="chatgpt-subscription-title"
    >
      <div class="verevon-chatgpt-subscription__header">
        <div>
          <span class="verevon-chatgpt-subscription__product">OpenAI</span>
          <h3 id="chatgpt-subscription-title">{i18n.tr('Bruk ChatGPT-abonnementet ditt', 'Use your ChatGPT subscription')}</h3>
          <p>
            {i18n.tr(
              'Koble den eksisterende ChatGPT-planen din til Verevon uten å opprette eller dele en API-nøkkel.',
              'Connect an existing ChatGPT plan to Verevon without creating or sharing an API key.',
            )}
          </p>
        </div>
        <span class={['verevon-chatgpt-subscription__status', { 'verevon-chatgpt-subscription__status--connected': connected(), 'verevon-chatgpt-subscription__status--pending': awaitingSignIn() }]}>
          {statusLabel()}
        </span>
      </div>

      <Show
        when={login()}
        fallback={
          <div class="verevon-chatgpt-subscription__actions">
            <Show
              when={connected()}
              fallback={
                <Button variant="primary" size="sm" disabled={!props.orgId.trim() || busy() === 'starting'} onClick={() => void start()}>
                  {busy() === 'starting' ? i18n.tr('Starter …', 'Starting …') : i18n.tr('Koble til ChatGPT', 'Connect ChatGPT')}
                </Button>
              }
            >
              <Button size="sm" disabled={busy() === 'disconnecting'} onClick={() => void disconnect()}>
                {busy() === 'disconnecting' ? i18n.tr('Kobler fra …', 'Disconnecting …') : i18n.tr('Koble fra', 'Disconnect')}
              </Button>
            </Show>
          </div>
        }
      >
        {(currentLogin) => (
          <div class="verevon-chatgpt-subscription__device" role="status" aria-live="polite">
            <div>
              <strong>{i18n.tr('Fullfør pålogging i ChatGPT', 'Finish signing in to ChatGPT')}</strong>
              <p>{i18n.tr('Åpne innloggingssiden og skriv inn denne engangskoden.', 'Open the sign-in page and enter this one-time code.')}</p>
            </div>
            <code>{currentLogin().userCode}</code>
            <div class="verevon-chatgpt-subscription__actions">
              <a href={currentLogin().verificationUrl} target="_blank" rel="noreferrer">
                {i18n.tr('Åpne ChatGPT-pålogging', 'Open ChatGPT sign-in')}
              </a>
              <Button size="sm" disabled={busy() !== null} onClick={() => void pollLogin()}>
                {i18n.tr('Jeg har logget inn', 'I have signed in')}
              </Button>
            </div>
          </div>
        )}
      </Show>

      <p class="verevon-chatgpt-subscription__privacy">
        {i18n.tr(
          'Verevon mottar aldri passordet ditt eller en API-nøkkel. Tilkoblingslegitimasjon lagres kun i Integration Core.',
          'Verevon never receives your password or an API key. The connection credential stays in Integration Core.',
        )}
      </p>
      <Show when={notice()}>{(message) => <p class="verevon-chatgpt-subscription__notice" role="status">{message()}</p>}</Show>
    </section>
  )
}
