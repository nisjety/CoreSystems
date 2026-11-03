import { createSignal, For, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { Send, ShieldCheck, X } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import {
  STEER_HELD_MAX,
  STEER_MAX_CHARS,
  isChatBrowserSteerEnabled,
  type BrowserSteer,
  type SteerEntry,
  type SteerRefusal,
} from './steer-input'
import './steer-input.css'

export type SteerInputProps = {
  /** This run's controller, made by the page with `createBrowserSteer`. */
  steer: BrowserSteer
  /** The run is working right now. The composer is offered only then; the strip stays after the run ends. */
  live: boolean
  /** Zero Data Retention on: steering is off, and a line says so. */
  zdr?: boolean
  /** Shows the composer. Defaults to the build flag `VITE_CHAT_BROWSER_STEER`, which is off. */
  enabled?: boolean
  /** The person sends a message the run never took as a new message. The page decides where it goes. */
  onResend?: (content: string) => void
}

/**
 * Steer and queue composer for a live browser-agent run in the dock (E3.3-g). Presentation only: the
 * controller owns the messages and the page owns the run. Enter steers; Shift+Enter starts a new line.
 */
export function SteerInput(props: SteerInputProps): JSX.Element {
  const i18n = useI18n()
  const tr = i18n.tr
  const [draft, setDraft] = createSignal('')
  const enabled = () => props.enabled ?? isChatBrowserSteerEnabled()
  const zdr = () => props.zdr === true
  const visible = () => enabled() && (props.live || props.steer.entries().length > 0)
  const canSend = () => draft().trim().length > 0 && draft().length <= STEER_MAX_CHARS

  const submit = async (mode: 'steer' | 'hold') => {
    const text = draft()
    if (!canSend()) return
    const result = mode === 'steer' ? await props.steer.steer(text) : props.steer.hold(text)
    const accepted = result.outcome === 'queued' || result.outcome === 'held'
    // Clear only the text that was sent: anything typed while the owner answered stays in the box.
    if (accepted && draft() === text) setDraft('')
  }

  const resend = (entry: SteerEntry) => {
    props.onResend?.(entry.content)
    props.steer.dismiss(entry.id)
  }

  const refusalText = (refusal: SteerRefusal | undefined): string => {
    switch (refusal) {
      case 'zdr':
        return tr('Ikke tilgjengelig med Zero Data Retention', 'Not available with Zero Data Retention')
      case 'not_live':
        return tr('Kjøringen er ikke i gang lenger', 'The run is no longer live')
      case 'signed_out':
        return tr('Logg inn på nytt for å styre kjøringen', 'Sign in again to steer the run')
      case 'queue_full':
        return tr(`Køen er full (${STEER_HELD_MAX} meldinger)`, `The queue is full (${STEER_HELD_MAX} messages)`)
      case 'too_long':
        return tr(`For lang melding (maks ${STEER_MAX_CHARS} tegn)`, `Too long (at most ${STEER_MAX_CHARS} characters)`)
      case 'empty':
        return tr('Skriv en melding først', 'Write a message first')
      default:
        return tr('Meldingen ble ikke sendt. Teksten er i behold.', 'The message was not sent. The text is kept.')
    }
  }

  const stateText = (entry: SteerEntry): string => {
    switch (entry.state) {
      case 'sending':
        return tr('Sender …', 'Sending …')
      case 'queued':
        return tr('Venter på agentens neste steg', 'Waiting for the next step of the agent')
      case 'delivered':
        return tr('Tatt imot av agenten', 'Taken by the agent')
      case 'not_taken':
        return tr('Kjøringen ble ferdig før agenten tok den', 'The run ended before the agent took it')
      case 'held':
        return tr('Venter til kjøringen er ferdig', 'Waiting until the run has ended')
      default:
        return refusalText(entry.refusal)
    }
  }

  return (
    <Show when={visible()}>
      <section class="browser-steer" data-live={props.live ? 'true' : 'false'} aria-label={tr('Styr kjøringen', 'Steer the run')}>
        <Show when={props.steer.entries().length > 0}>
          <ul class="browser-steer__strip" aria-live="polite">
            <For each={props.steer.entries()}>
              {(entry) => (
                <li class="browser-steer__item" data-state={entry.state}>
                  <span class="browser-steer__text">{entry.content}</span>
                  <span class="browser-steer__state">{stateText(entry)}</span>
                  <span class="browser-steer__item-actions">
                    <Show when={entry.state === 'not_taken' && props.onResend !== undefined}>
                      <button type="button" class="browser-steer__button" onClick={() => resend(entry)}>
                        {tr('Send som ny melding', 'Send as a new message')}
                      </button>
                    </Show>
                    <Show when={entry.state === 'held' || entry.state === 'not_taken' || entry.state === 'refused'}>
                      <button
                        type="button"
                        class="browser-steer__button"
                        aria-label={tr('Fjern meldingen', 'Remove the message')}
                        onClick={() => props.steer.dismiss(entry.id)}
                      >
                        <X class="size-3" aria-hidden="true" />
                      </button>
                    </Show>
                  </span>
                </li>
              )}
            </For>
          </ul>
        </Show>
        <Show when={props.live && zdr()}>
          <p class="browser-steer__zdr">
            <ShieldCheck class="size-3" aria-hidden="true" />
            <span>
              {tr(
                'Zero Data Retention: du kan ikke styre kjøringen i denne økten.',
                'Zero Data Retention: you cannot steer the run in this session.',
              )}
            </span>
          </p>
        </Show>
        <Show when={props.live && !zdr()}>
          <form
            class="browser-steer__form"
            onSubmit={(event) => {
              event.preventDefault()
              void submit('steer')
            }}
          >
            <textarea
              class="browser-steer__input"
              rows={2}
              maxLength={STEER_MAX_CHARS}
              value={draft()}
              placeholder={tr('Skriv til agenten …', 'Write to the agent …')}
              aria-label={tr('Melding til agenten', 'Message to the agent')}
              onInput={(event) => setDraft(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key !== 'Enter' || event.shiftKey || event.isComposing) return
                event.preventDefault()
                void submit('steer')
              }}
            />
            <div class="browser-steer__actions">
              <span class="browser-steer__count">
                {draft().length}/{STEER_MAX_CHARS}
              </span>
              <button type="button" class="browser-steer__button" disabled={!canSend()} onClick={() => void submit('hold')}>
                {tr('Legg i kø', 'Queue')}
              </button>
              <button type="submit" class="browser-steer__button browser-steer__button--primary" disabled={!canSend()}>
                <Send class="size-3" aria-hidden="true" />
                <span>{tr('Styr agenten', 'Steer the agent')}</span>
              </button>
            </div>
            <p class="browser-steer__hint">
              {tr(
                'Holdes bare i minnet, og lagres ikke i samtalen. «Legg i kø» sender når kjøringen er ferdig.',
                'Held in memory only, and not saved to the conversation. Queue sends once the run has ended.',
              )}
            </p>
          </form>
        </Show>
      </section>
    </Show>
  )
}
