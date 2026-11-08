/**
 * The connected-app suggestion card (EP-10 S8). When a question needs the person's own mail, calendar
 * or files and no connected account was selected, the model may suggest one provider; the gateway sends
 * that as one `connector_suggestion` frame per turn. The card offers the person's own next step:
 *  - no active connection of the provider: "Connect", the existing connect session and sign-in window
 *    (`integrations.start_connect_session`);
 *  - an active connection: "Use it for this question", which the caller turns into a connected-app
 *    selection for the question and sends it again (`chat.accept_connector_suggestion`, path `use`);
 *  - "Not now" closes the card.
 * Nothing is chosen for the person and nothing is remembered past the turn. A Space thread and a
 * temporary chat never show the card (the gateway never offers the tool there either), and a card read
 * back after a reload is a note: the moment it answered has passed.
 *
 * EP-10 S9: when the caller hands the card the question it answers (`connectTurn`), a connect resends that
 * question on the new connection once (`connect-intent.ts`). A reload during the sign-in shows "Finish
 * connecting", and nothing is resent until the person clicks Continue.
 */

import { createSignal, For, Show } from 'solid-js'
import { listConnections, listConnectorPickerTabs, startConnectSession } from '@/shared/api/integrations-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { connectBundlesForSources } from '@/shared/integrations/connect-bundles'
import {
  continueSavedConnectIntent,
  runConnectIntent,
  type ConnectIntent,
  type ConnectionRef,
} from '@/shared/integrations/connect-intent'
import { reserveDirectOauthWindow, runDirectOauthWindow } from '@/shared/integrations/provider-auth-window'
import { createResource } from '@/shared/lib/create-resource-compat'
import type { ChatConnectorSuggestion, ConnectorSuggestionProvider } from './chat-types'

/** The name the card gives each provider of the closed list. */
export const CONNECTOR_SUGGESTION_PROVIDER_LABELS: Record<ConnectorSuggestionProvider, string> = {
  google: 'Google',
  microsoft: 'Microsoft',
}

/** Whether a thread may show the card at all: never in a Space and never in a temporary chat. */
export function connectorSuggestionVisible(input: { spaceScoped?: boolean; temporary?: boolean }): boolean {
  return input.spaceScoped !== true && input.temporary !== true
}

export function ConnectorSuggestionCard(props: {
  suggestion: ChatConnectorSuggestion
  orgId: string
  spaceScoped?: boolean
  temporary?: boolean
  /** Select this connection for the question and send it again. */
  onUse: (connectionId: string) => void
  /** The provider was connected from the card. */
  onConnected?: (provider: ConnectorSuggestionProvider) => void
  onDismiss?: () => void
  /** EP-10 S9: the thread and question a connect resumes; null where none may (temporary, Space, foreign thread). */
  connectTurn?: () => { threadId: string; prompt: string } | null
  /** EP-10 S9: the saved connect of the thread on screen, while it is unexpired. */
  pendingConnectIntent?: () => ConnectIntent | null
  /** EP-10 S9: resends the saved question on the connection; false when it cannot go out now. */
  resumeConnectIntent?: (intent: ConnectIntent, connectionId: string, providerLabel: string) => boolean
  /** EP-10 S9: "Not now" on a saved connect. */
  dismissConnectIntent?: () => void
}) {
  const i18n = useI18n()
  const [phase, setPhase] = createSignal<'open' | 'connecting' | 'used' | 'dismissed'>('open')
  const [failure, setFailure] = createSignal<string | null>(null)
  const [connected, setConnected] = createSignal(false)
  // EP-10 S9: connected but the question not resent yet, a Continue in flight, and a finish state set aside.
  const [pending, setPending] = createSignal(false)
  const [continuing, setContinuing] = createSignal(false)
  const [finishHidden, setFinishHidden] = createSignal(false)

  const label = () => CONNECTOR_SUGGESTION_PROVIDER_LABELS[props.suggestion.provider]
  const live = () => props.suggestion.restored !== true
  // The person's active accounts of the suggested provider. Read only for a live card that may show: a
  // restored note or a hidden card offers nothing, so it asks nothing. A failed read shows the Connect
  // path, which is always safe.
  const [accounts, { refetch }] = createResource(
    () => (live() && connectorSuggestionVisible(props) && props.orgId ? props.orgId : false),
    async (orgId: string) => {
      try {
        const tabs = await listConnectorPickerTabs(orgId)
        return tabs.filter((tab) => tab.providerKey === props.suggestion.provider)
      } catch {
        return []
      }
    },
  )

  const connectionRefs = async (): Promise<ConnectionRef[]> => (await listConnections(props.orgId))
    .map((connection) => ({ id: connection.id, providerKey: connection.providerKey, status: connection.status }))

  const connectFailure = (reason: unknown) => translateApiError(reason, i18n.tr, {
    no: 'Tilkoblingen kunne ikke fullføres.',
    en: 'The connection could not be completed.',
  })

  // A connect that resumes its question (EP-10 S9). The sign-in window is reserved before the first await, in the
  // click, so a browser that blocks late popups still opens it.
  const connectAndResume = async (turn: { threadId: string; prompt: string }) => {
    setPhase('connecting')
    setFailure(null)
    setPending(false)
    const provider = props.suggestion.provider
    const reserved = reserveDirectOauthWindow()
    try {
      const outcome = await runConnectIntent({
        ...turn,
        suggestionId: props.suggestion.suggestionId,
        provider,
        knownIds: new Set((accounts() ?? []).map((account) => account.connectionId)),
        openConnect: async () => {
          const session = await startConnectSession(props.orgId, provider, { bundles: connectBundlesForSources(provider) })
          const connectUrl = session.connectUrl || session.redirectUrl
          const sessionToken = session.sessionToken || session.id
          if (!connectUrl || !sessionToken) {
            reserved?.close()
            throw new Error(i18n.tr('Tilkoblingen kunne ikke startes.', 'The connection could not be started.'))
          }
          await runDirectOauthWindow({ connectUrl, sessionToken }, reserved)
        },
        listConnections: connectionRefs,
        resume: (intent, connectionId) => props.resumeConnectIntent?.(intent, connectionId, label()) ?? false,
      })
      if (outcome.status === 'failed') {
        setFailure(connectFailure(outcome.error))
        setPhase('open')
        return
      }
      setConnected(true)
      props.onConnected?.(provider)
      if (outcome.status === 'resumed') {
        setPhase('used')
        return
      }
      setPending(true)
      await refetch()
      setPhase('open')
    } catch (reason) {
      // A resend that threw: the intent was saved again, so "Finish connecting" can retry it.
      setFailure(connectFailure(reason))
      setPhase('open')
    }
  }

  const connect = async () => {
    if (phase() !== 'open') return
    const turn = props.connectTurn?.() ?? null
    if (turn) return connectAndResume(turn)
    setPhase('connecting')
    setFailure(null)
    const provider = props.suggestion.provider
    try {
      // The same bundle policy as every other connect surface, so a connection made here also serves
      // Knowledge and Settings instead of being a second, narrower one.
      const session = await startConnectSession(props.orgId, provider, { bundles: connectBundlesForSources(provider) })
      const connectUrl = session.connectUrl || session.redirectUrl
      const sessionToken = session.sessionToken || session.id
      if (!connectUrl || !sessionToken) {
        throw new Error(i18n.tr('Tilkoblingen kunne ikke startes.', 'The connection could not be started.'))
      }
      await runDirectOauthWindow({ connectUrl, sessionToken })
      setConnected(true)
      await refetch()
      props.onConnected?.(provider)
    } catch (reason) {
      setFailure(connectFailure(reason))
    } finally {
      setPhase('open')
    }
  }

  // "Finish connecting" (EP-10 S9): a saved connect of this suggestion, shown only where a connect may resume and
  // while this card is not running its own.
  const finishIntent = (): ConnectIntent | null => {
    if (finishHidden() || phase() === 'connecting' || phase() === 'used' || !props.connectTurn?.()) return null
    const intent = props.pendingConnectIntent?.() ?? null
    return intent?.suggestionId === props.suggestion.suggestionId ? intent : null
  }

  const continueSaved = async () => {
    const intent = finishIntent()
    if (!intent || continuing()) return
    setContinuing(true)
    setFailure(null)
    try {
      const outcome = await continueSavedConnectIntent({
        threadId: intent.threadId,
        listConnections: connectionRefs,
        resume: (saved, connectionId) => props.resumeConnectIntent?.(saved, connectionId, label()) ?? false,
      })
      if (outcome === 'resumed') {
        setPhase('used')
      } else if (outcome === 'pending') {
        setPending(true)
      } else {
        // 'not_connected' leaves the Connect button; 'none' has nothing left to finish.
        setFinishHidden(true)
      }
    } catch (reason) {
      // A failed list read changes nothing: the saved connect stays for another try.
      setFailure(connectFailure(reason))
    } finally {
      setContinuing(false)
    }
  }

  const dismissFinish = () => {
    setFinishHidden(true)
    props.dismissConnectIntent?.()
  }

  const use = (connectionId: string) => {
    if (phase() !== 'open') return
    setPhase('used')
    props.onUse(connectionId)
  }

  const dismiss = () => {
    setPhase('dismissed')
    props.onDismiss?.()
  }

  const finishState = () => (
    <Show when={finishIntent()}>
      <div class="verevon-connector-suggestion__finish" role="group">
        <p class="verevon-connector-suggestion__status">
          {i18n.tr('Fullfør tilkoblingen for å fortsette spørsmålet.', 'Finish connecting to continue the question.')}
        </p>
        <button
          type="button"
          class="verevon-connector-suggestion__continue"
          disabled={continuing()}
          onClick={() => void continueSaved()}
        >
          {i18n.tr('Fortsett', 'Continue')}
        </button>
        <button type="button" class="verevon-connector-suggestion__dismiss" onClick={dismissFinish}>
          {i18n.tr('Ikke nå', 'Not now')}
        </button>
      </div>
    </Show>
  )

  return (
    <Show when={connectorSuggestionVisible(props) && phase() !== 'dismissed'}>
      {finishState()}
      <Show
        when={live()}
        fallback={
          <p class="verevon-connector-suggestion__note">
            {i18n.tr(`Foreslo å bruke ${label()}: `, `Suggested using ${label()}: `)}
            {props.suggestion.reason}
          </p>
        }
      >
        <section
          class="verevon-connector-suggestion"
          aria-label={i18n.tr('Forslag om tilkoblet app', 'Connected app suggestion')}
          data-provider={props.suggestion.provider}
        >
          <h3 class="verevon-connector-suggestion__title">
            {i18n.tr(`${label()} kan hjelpe med dette`, `${label()} could help with this`)}
          </h3>
          <p class="verevon-connector-suggestion__reason">{props.suggestion.reason}</p>
          <Show when={connected()}>
            <p class="verevon-connector-suggestion__status" role="status">
              {pending()
                ? i18n.tr(
                  'Koblet til. Fullfør tilkoblingen når chatten er klar.',
                  'Connected. Finish connecting when the chat is ready.',
                )
                : i18n.tr(`${label()} er koblet til.`, `${label()} is connected.`)}
            </p>
          </Show>
          <Show when={phase() === 'used'}>
            <p class="verevon-connector-suggestion__status" role="status">
              {i18n.tr('Spør på nytt med kontoen valgt.', 'Asking again with the account selected.')}
            </p>
          </Show>
          <Show when={failure()}>
            {(message) => (
              <p class="verevon-connector-suggestion__error" role="alert">
                {message()}
              </p>
            )}
          </Show>
          <Show when={phase() !== 'used'}>
            <div class="verevon-connector-suggestion__actions">
              <Show
                when={(accounts() ?? []).length > 0}
                fallback={
                  <button
                    type="button"
                    class="verevon-connector-suggestion__connect"
                    disabled={phase() !== 'open' || accounts.loading}
                    onClick={() => void connect()}
                  >
                    {phase() === 'connecting'
                      ? i18n.tr(`Kobler til ${label()} …`, `Connecting ${label()} …`)
                      : i18n.tr(`Koble til ${label()}`, `Connect ${label()}`)}
                  </button>
                }
              >
                <For each={accounts() ?? []}>
                  {(account) => (
                    <button
                      type="button"
                      class="verevon-connector-suggestion__use"
                      disabled={phase() !== 'open'}
                      onClick={() => use(account.connectionId)}
                    >
                      {i18n.tr(`Bruk ${account.label} for dette spørsmålet`, `Use ${account.label} for this question`)}
                    </button>
                  )}
                </For>
              </Show>
              <button type="button" class="verevon-connector-suggestion__dismiss" onClick={dismiss}>
                {i18n.tr('Ikke nå', 'Not now')}
              </button>
            </div>
          </Show>
        </section>
      </Show>
    </Show>
  )
}
