import { createSignal, Show } from 'solid-js'

import { bindSpaceAgent } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { translateApiError } from '@/shared/i18n/errors'

/**
 * Offer to add, or request to add (WS08-S6): the member mentioned an agent that is not in this
 * room. A room owner or manager can bind it and send in one step ("Add and send"), or send
 * without it (the words stay, no agent is woken). Anyone else can ask the room's managers, which
 * is an ordinary post that mentions them: nothing is bound and nothing is granted by asking.
 * Binding goes through the room's own governed bind route; this dialog decides nothing.
 */

export type AgentAddTarget =
  | { readonly kind: 'add'; readonly agentRef: string; readonly name: string }
  | { readonly kind: 'request'; readonly name: string }

export interface AgentAddDialogProps {
  readonly spaceRef: string
  readonly target: () => AgentAddTarget | undefined
  readonly onClose: () => void
  /** The agent is bound; the composer picks it and sends. */
  readonly onBound: (bound: { readonly subjectId: string; readonly name: string }) => void
  /** Send the words as they are, without the agent. */
  readonly onSendWithout: () => void
  /** Post a normal message that mentions the room's managers. */
  readonly onRequest: (name: string) => void
}

export function AgentAddDialog(props: AgentAddDialogProps) {
  const i18n = useI18n()
  const [busy, setBusy] = createSignal(false)
  const [failure, setFailure] = createSignal<string | undefined>(undefined)

  function close(): void {
    setFailure(undefined)
    props.onClose()
  }

  async function addAndSend(target: Extract<AgentAddTarget, { kind: 'add' }>): Promise<void> {
    if (busy()) return
    setBusy(true)
    setFailure(undefined)
    try {
      const bound = await bindSpaceAgent(props.spaceRef, target.agentRef)
      props.onBound({ subjectId: bound.subject_id, name: target.name })
    } catch (error) {
      setFailure(translateApiError(error, i18n.tr, {
        no: 'Agenten kunne ikke legges til. Ingenting er sendt.',
        en: 'The agent could not be added. Nothing was sent.',
      }))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Show when={props.target()}>
      {(target) => (
        <div
          class="verevon-space-create-agent"
          role="presentation"
          onClick={(event) => { if (event.target === event.currentTarget) close() }}
        >
          <section
            class="verevon-space-create-agent__dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="space-agent-add-title"
          >
            <h3 id="space-agent-add-title">
              {target().kind === 'add'
                ? i18n.tr(`${target().name} er ikke i rommet`, `${target().name} is not in this room`)
                : i18n.tr('Be en leder legge til en agent', 'Ask a manager to add an agent')}
            </h3>
            <p class="verevon-space-create-agent__note">
              {target().kind === 'add'
                ? i18n.tr(
                    'Du kan legge agenten til i rommet og sende meldingen, eller sende uten at agenten svarer.',
                    'You can add the agent to this room and send the message, or send it without the agent answering.',
                  )
                : i18n.tr(
                    `Bare en eier eller leder kan legge til agenter. Vi kan sende meldingen din og nevne lederne, så de kan legge til ${target().name}.`,
                    `Only an owner or manager can add agents. We can send your message and mention the managers so they can add ${target().name}.`,
                  )}
            </p>
            <Show when={failure()}>
              {(message) => <p class="verevon-space-projection-error" role="alert">{message()}</p>}
            </Show>
            <div class="verevon-space-create-agent__actions">
              <Show
                when={target().kind === 'add' ? (target() as Extract<AgentAddTarget, { kind: 'add' }>) : undefined}
                fallback={
                  <button
                    type="button"
                    class="verevon-space-secondary-action"
                    onClick={() => { props.onRequest(target().name); close() }}
                  >
                    {i18n.tr('Send og be om det', 'Send and request it')}
                  </button>
                }
              >
                {(add) => (
                  <>
                    <button
                      type="button"
                      class="verevon-space-secondary-action"
                      disabled={busy()}
                      onClick={() => void addAndSend(add())}
                    >
                      {busy() ? i18n.tr('Legger til …', 'Adding…') : i18n.tr('Legg til og send', 'Add and send')}
                    </button>
                    <button
                      type="button"
                      class="verevon-space-secondary-action"
                      disabled={busy()}
                      onClick={() => { props.onSendWithout(); close() }}
                    >
                      {i18n.tr('Send uten å legge til', 'Send without adding')}
                    </button>
                  </>
                )}
              </Show>
              <button type="button" disabled={busy()} onClick={close}>
                {i18n.tr('Avbryt', 'Cancel')}
              </button>
            </div>
          </section>
        </div>
      )}
    </Show>
  )
}
