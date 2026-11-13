import { createEffect, createSignal, onCleanup, Show } from 'solid-js'
import {
  disconnectChatGptSubscription,
  getChatGptSubscriptionStatus,
  listChatGptSubscriptions,
  startChatGptSubscription,
  type ChatGptDeviceLogin,
  type ChatGptSubscriptionConnection,
} from '@/shared/api/chatgpt-subscription-client'
import {
  getConnectionZdr,
  isZdrWaiverUiEnabled,
  revokeConnectionZdr,
  type ConnectionZdrAssessment,
} from '@/shared/api/connection-zdr-client'
import type { ConnectionZdrAcknowledgedVia, ConnectionZdrVariant } from '@/shared/actions/connection-zdr'
import { isLocalRetentionAllowed } from '@/features/chat/lib/chat-retention'
import { useI18n } from '@/shared/i18n'
import { en } from '@/shared/i18n/en'
import { no } from '@/shared/i18n/no'
import { ApiError } from '@/shared/api/http'
import { Button } from '@/shared/ui/Button'
import { ConnectionZdrAcknowledgement } from './ConnectionZdrAcknowledgement'

type ChatGptSubscriptionPanelProps = {
  orgId: string
  /** Used by Settings to refresh its live integration metrics. */
  onConnectionChange?: () => void
  variant?: 'settings' | 'onboarding'
  /**
   * Whether the organisation has zero data retention on, from the settings state the parent already holds (the
   * workspace settings page reads it). Absent, the panel uses the SPA's server-stated retention posture. It is read
   * here, never fetched from Org Core by this panel.
   */
  orgZdr?: boolean
}

function isConnected(status: string): boolean {
  return ['active', 'connected'].includes(status.trim().toLowerCase())
}

/** Integration Core marks a connection whose ChatGPT sign-in must be renewed. */
function needsReconnect(status: string): boolean {
  return status.trim().toLowerCase() === 'needs_refresh'
}

/**
 * The panel's anchor. The chat links here when ChatGPT refuses a turn because
 * the sign-in must be renewed (`subscription_reauth_required`).
 */
export const CHATGPT_SUBSCRIPTION_ANCHOR = 'chatgpt-subscription'

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
  // The zero data retention acknowledgement of the connection (ZDR Z-20), behind VITE_ZDR_WAIVER_UI. The assessment is
  // read from Integration Core and shown as it is; nothing about it is stored in the browser.
  const [zdrAssessment, setZdrAssessment] = createSignal<ConnectionZdrAssessment | null>(null)
  const [zdrDialog, setZdrDialog] = createSignal<{
    connectionId: string
    variant: ConnectionZdrVariant
    via: ConnectionZdrAcknowledgedVia
  } | null>(null)
  const [confirmingRevoke, setConfirmingRevoke] = createSignal(false)
  const [zdrBusy, setZdrBusy] = createSignal(false)
  let zdrReadAttempt = 0
  let pollTimer: number | undefined
  let disposed = false
  let loginAttempt = 0
  let pollingLoginId: string | undefined

  const stopPolling = () => {
    if (pollTimer !== undefined) window.clearTimeout(pollTimer)
    pollTimer = undefined
  }

  const notifyConnectionChange = () => props.onConnectionChange?.()

  const zdrCopy = () => (i18n.locale() === 'no' ? no : en).zdr.waiver
  const orgIsZdr = () => props.orgZdr ?? !isLocalRetentionAllowed()

  /** Reads the connection's assessment, to know whether it needs an acknowledgement and whether one is in force. */
  const refreshZdr = async (connectionId: string) => {
    const attempt = ++zdrReadAttempt
    if (!isZdrWaiverUiEnabled()) return
    try {
      const assessment = await getConnectionZdr(connectionId, i18n.locale(), orgIsZdr() ? 'org_zdr_on' : 'org_zdr_off')
      if (disposed || attempt !== zdrReadAttempt) return
      setZdrAssessment(assessment)
    } catch {
      if (disposed || attempt !== zdrReadAttempt) return
      // Not knowing is not a reason to offer anything: the review entry stays out of sight.
      setZdrAssessment(null)
    }
  }

  const revokeZdr = async () => {
    const current = connection()
    if (!current || zdrBusy()) return
    setZdrBusy(true)
    try {
      await revokeConnectionZdr(current.id)
      setConfirmingRevoke(false)
      setNotice(zdrCopy().revoked)
      await refreshZdr(current.id)
      notifyConnectionChange()
    } catch {
      setNotice(zdrCopy().revoke_failed)
    } finally {
      setZdrBusy(false)
    }
  }

  const refreshConnection = async (orgId: string) => {
    if (!orgId.trim()) return
    try {
      const subscriptions = await listChatGptSubscriptions(orgId)
      if (disposed || orgId !== props.orgId) return
      // A connection whose sign-in must be renewed is shown as such, so a
      // reader sent here from the chat sees why, not just "not connected".
      const active = subscriptions.find((candidate) => isConnected(candidate.status))
        ?? subscriptions.find((candidate) => needsReconnect(candidate.status))
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
      // The connection cannot give zero data retention. When the organisation has it on, the member confirms, once,
      // the exact text Integration Core serves before the connection can be used (ZDR Z-20).
      if (isZdrWaiverUiEnabled() && orgIsZdr() && !disposed) {
        setZdrDialog({ connectionId: nextConnection.id, variant: 'org_zdr_on', via: 'connect' })
      }
      return
    }
    setNotice(status === 'expired'
      ? i18n.tr('Påloggingskoden utløp. Start på nytt for å få en ny kode.', 'The sign-in code expired. Start again for a new code.')
      : i18n.tr('ChatGPT-påloggingen ble ikke fullført. Prøv igjen.', 'ChatGPT sign-in was not completed. Please try again.'))
  }

  const pollLogin = async () => {
    stopPolling()
    const currentLogin = login()
    const targetOrgId = props.orgId
    if (disposed || !currentLogin || !targetOrgId.trim() || pollingLoginId === currentLogin.loginId) return
    if (currentLogin.expiresAt && Date.parse(currentLogin.expiresAt) <= Date.now()) {
      setLogin(undefined)
      setNotice(i18n.tr('Påloggingskoden utløp. Start på nytt for å få en ny kode.', 'The sign-in code expired. Start again for a new code.'))
      return
    }
    pollingLoginId = currentLogin.loginId
    try {
      const result = await getChatGptSubscriptionStatus(
        targetOrgId,
        currentLogin.connectionId,
        currentLogin.loginId,
      )
      if (disposed || targetOrgId !== props.orgId || login()?.loginId !== currentLogin.loginId) return
      setConnection(result.connection)
      // Polling returns status only. Keep the one-time code, URL, and expiry
      // from the start response for the lifetime of this login attempt.
      setLogin({ ...currentLogin, status: result.login.status })

      const loginStatus = result.login.status
      if (loginStatus === 'connected' || loginStatus === 'failed' || loginStatus === 'expired') {
        await finishLogin(loginStatus, result.connection)
        return
      }
      pollTimer = window.setTimeout(() => void pollLogin(), 2_500)
    } catch (error) {
      if (disposed || targetOrgId !== props.orgId || login()?.loginId !== currentLogin.loginId) return
      if (error instanceof ApiError && [400, 401, 403, 404, 409, 410].includes(error.status)) {
        setLogin(undefined)
        setNotice(error.status === 401 || error.status === 403
          ? i18n.tr('Verevon-økten eller tilgangen er ikke lenger gyldig. Logg inn i Verevon igjen.', 'Your Verevon session or access is no longer valid. Sign in to Verevon again.')
          : i18n.tr('Påloggingskoden er utløpt eller ikke lenger gyldig. Start på nytt for å få en ny kode.', 'The sign-in code has expired or is no longer valid. Start again for a new code.'))
        return
      }
      // Retain the displayed code and retry once the temporary gateway/network
      // failure has cleared. Never surface a provider's raw auth response.
      setNotice(i18n.tr('Venter fortsatt på at ChatGPT-påloggingen skal fullføres.', 'Still waiting for ChatGPT sign-in to finish.'))
      pollTimer = window.setTimeout(() => void pollLogin(), 4_000)
    } finally {
      if (pollingLoginId === currentLogin.loginId) pollingLoginId = undefined
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
    const attempt = ++loginAttempt
    setLogin(undefined)
    setBusy('starting')
    setNotice(null)
    try {
      const result = await startChatGptSubscription(targetOrgId)
      if (disposed || attempt !== loginAttempt || targetOrgId !== props.orgId) {
        if (loginWindow && !loginWindow.closed) loginWindow.close()
        return
      }
      setConnection(result.connection)
      setLogin(result.login)
      if (loginWindow && !loginWindow.closed) loginWindow.location.replace(result.login.verificationUrl)
      pollTimer = window.setTimeout(() => void pollLogin(), 1_000)
    } catch {
      if (loginWindow && !loginWindow.closed) loginWindow.close()
      if (disposed || attempt !== loginAttempt || targetOrgId !== props.orgId) return
      setNotice(i18n.tr('Kunne ikke starte ChatGPT-påloggingen. Prøv igjen.', 'We could not start ChatGPT sign-in. Please try again.'))
    } finally {
      if (!disposed && attempt === loginAttempt) setBusy(null)
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
      setZdrAssessment(null)
      setConfirmingRevoke(false)
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
      loginAttempt++
      stopPolling()
      setBusy(null)
      setConnection(undefined)
      setLogin(undefined)
      setNotice(null)
      setZdrAssessment(null)
      setZdrDialog(null)
      setConfirmingRevoke(false)
      void refreshConnection(orgId)
    },
  )

  onCleanup(() => {
    disposed = true
    loginAttempt++
    stopPolling()
  })

  // Arriving from the chat's "Koble til ChatGPT på nytt" link: the router
  // renders this page after the browser's own hash scroll has passed, so bring
  // the panel into view once it is mounted. Solid v2 has no onMount; a
  // two-phase effect with a constant compute runs once after mount.
  let section: HTMLElement | undefined
  createEffect(
    () => undefined,
    () => {
      if (typeof window === 'undefined' || window.location.hash !== `#${CHATGPT_SUBSCRIPTION_ANCHOR}`) return
      const reduce = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false
      section?.scrollIntoView?.({ behavior: reduce ? 'auto' : 'smooth', block: 'start' })
    },
  )

  const connected = () => Boolean(connection() && isConnected(connection()!.status))
  // Read the assessment of a connected connection, so the review and withdraw entries show only when they mean something.
  createEffect(
    () => (connected() ? connection()!.id : undefined),
    (connectionId) => {
      setZdrAssessment(null)
      if (!connectionId || !isZdrWaiverUiEnabled()) return
      void refreshZdr(connectionId)
    },
  )
  const zdrNeedsAcknowledgement = () =>
    isZdrWaiverUiEnabled() && connected() && zdrAssessment()?.requiresAcknowledgement === true
  const zdrAcknowledged = () => zdrAssessment()?.acknowledgement?.state === 'active'
  const awaitingSignIn = () => Boolean(login() && !isTerminalLogin(login()!.status))
  const statusLabel = () => {
    if (connected()) return i18n.tr('Tilkoblet', 'Connected')
    if (awaitingSignIn()) return i18n.tr('Venter på pålogging', 'Waiting for sign-in')
    if (connection() && needsReconnect(connection()!.status)) return i18n.tr('Må kobles til på nytt', 'Needs reconnecting')
    return i18n.tr('Ikke tilkoblet', 'Not connected')
  }

  return (
    <section
      ref={section}
      id={CHATGPT_SUBSCRIPTION_ANCHOR}
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

      <Show when={!connected()}>
        <p class="verevon-chatgpt-subscription__privacy">
          {i18n.tr(
            'Første gang: Aktiver autorisasjon med enhetskode for Codex i ChatGPTs sikkerhetsinnstillinger. Kom deretter tilbake hit og hent en ny kode. For en administrert ChatGPT-konto kan administratoren måtte aktivere dette.',
            'First time: Enable device code authorization for Codex in ChatGPT security settings. Then return here and get a new code. For a managed ChatGPT account, your administrator may need to enable this.',
          )}{' '}
          <a href="https://chatgpt.com/#settings/Security" target="_blank" rel="noreferrer">
            {i18n.tr('Åpne ChatGPTs sikkerhetsinnstillinger', 'Open ChatGPT security settings')}
          </a>
        </p>
      </Show>

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
              {/* Zero data retention (ZDR Z-20): review the text served for this connection, or withdraw the
                  confirmation. Both are the member's own acts and both read what Integration Core serves. */}
              <Show when={zdrNeedsAcknowledgement()}>
                <Button
                  size="sm"
                  onClick={() =>
                    setZdrDialog({
                      connectionId: connection()!.id,
                      variant: orgIsZdr() ? 'org_zdr_on' : 'org_zdr_off',
                      via: 'settings',
                    })
                  }
                >
                  {zdrCopy().review}
                </Button>
                <Show when={zdrAcknowledged() && !confirmingRevoke()}>
                  <Button size="sm" disabled={zdrBusy()} onClick={() => setConfirmingRevoke(true)}>
                    {zdrCopy().revoke}
                  </Button>
                </Show>
              </Show>
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
              <Button size="sm" disabled={busy() !== null} onClick={() => void start()}>
                {i18n.tr('Hent ny kode', 'Get a new code')}
              </Button>
            </div>
          </div>
        )}
      </Show>

      <Show when={confirmingRevoke() && zdrAcknowledged()}>
        <div class="verevon-chatgpt-subscription__actions" role="group" aria-label={zdrCopy().revoke}>
          <p class="verevon-chatgpt-subscription__privacy">{zdrCopy().revoke_confirm}</p>
          <Button size="sm" variant="primary" disabled={zdrBusy()} onClick={() => void revokeZdr()}>
            {zdrCopy().revoke_yes}
          </Button>
          <Button size="sm" disabled={zdrBusy()} onClick={() => setConfirmingRevoke(false)}>
            {zdrCopy().revoke_no}
          </Button>
        </div>
      </Show>

      <p class="verevon-chatgpt-subscription__privacy">
        {i18n.tr(
          'Verevon mottar aldri passordet ditt eller en API-nøkkel. Tilkoblingslegitimasjon lagres kun i Integration Core.',
          'Verevon never receives your password or an API key. The connection credential stays in Integration Core.',
        )}
      </p>
      <Show when={notice()}>{(message) => <p class="verevon-chatgpt-subscription__notice" role="status">{message()}</p>}</Show>
      <Show when={zdrDialog()}>
        {(dialog) => (
          <ConnectionZdrAcknowledgement
            connectionId={dialog().connectionId}
            variant={dialog().variant}
            acknowledgedVia={dialog().via}
            onAcknowledged={() => {
              setNotice(zdrCopy().confirmed)
              void refreshZdr(dialog().connectionId)
              notifyConnectionChange()
            }}
            onClose={() => setZdrDialog(null)}
          />
        )}
      </Show>
    </section>
  )
}
