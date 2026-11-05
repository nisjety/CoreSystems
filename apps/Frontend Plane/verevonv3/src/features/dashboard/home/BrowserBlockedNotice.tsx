import { AlertCircle, ArrowRight, Hand } from '@/shared/icons'
import { createMemo, Show } from 'solid-js'
import type { BrowserChallenge } from '@/shared/api/browser-client'
import { useI18n } from '@/shared/i18n'
import { blockedSentence, blockedStateFor } from './browser-blocked'
import './browser-blocked-notice.css'

/**
 * The notice that sits above the page when Quarry reports a bot wall (a strong
 * challenge) on the current observation. Renders nothing for any other signal.
 *
 * "Take over" is the chrome's own take-over toggle (`toggleBrowserControlMode`),
 * so it goes through the same gates as the chip in the toolbar, and it is not
 * offered once the person already holds the browser. "Try another source" leaves
 * the site: the owner of the session decides what that closes (see the action
 * contract `browser.blocked.try_other_source`).
 */
export function BrowserBlockedNotice(props: {
  challenge: BrowserChallenge | null | undefined
  /** The person already holds the browser. */
  humanTakeoverActive: boolean
  openOutsideUrl?: string
  takeoverDisabled: boolean
  tryAnotherSourceDisabled: boolean
  onTakeOver: () => void
  onTryAnotherSource: () => void
}) {
  const i18n = useI18n()
  const state = createMemo(() => blockedStateFor(props.challenge))

  return (
    <Show when={state()} keyed>
      {(blocked) => (
        <div class="knowledge-browser-blocked" role="status" data-reason={blocked.reason ?? 'unspecified'}>
          <AlertCircle class="size-3.5" aria-hidden="true" />
          <p class="knowledge-browser-blocked__text">{blockedSentence(blocked, i18n.tr)}</p>
          <div class="knowledge-browser-blocked__actions">
            <Show when={!props.humanTakeoverActive}>
              <button
                type="button"
                class="knowledge-browser-blocked__button"
                disabled={props.takeoverDisabled}
                onClick={() => props.onTakeOver()}
              >
                <Hand class="size-3" aria-hidden="true" />
                {i18n.tr('Ta over', 'Take over')}
              </button>
            </Show>
            <Show when={props.openOutsideUrl}>
              {(url) => <a class="knowledge-browser-blocked__button" href={url()} target="_blank" rel="noopener noreferrer">
                {i18n.tr('Åpne utenfor Verevon', 'Open outside Verevon')}
              </a>}
            </Show>
            <button
              type="button"
              class="knowledge-browser-blocked__button"
              disabled={props.tryAnotherSourceDisabled}
              onClick={() => props.onTryAnotherSource()}
            >
              <ArrowRight class="size-3" aria-hidden="true" />
              {i18n.tr('Prøv en annen kilde', 'Try another source')}
            </button>
          </div>
        </div>
      )}
    </Show>
  )
}
