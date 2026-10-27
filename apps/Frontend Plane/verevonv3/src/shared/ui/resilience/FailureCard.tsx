import { createSignal, Show } from 'solid-js'

import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { Button } from '@/shared/ui/Button'
import {
  buildFailureDetails,
  classifyClientError,
  type ClientRegion,
} from '@/shared/ui/resilience/classify-client-error'
import { FAILURE_ACTIONS, FAILURE_COPY } from '@/shared/ui/resilience/failure-copy'
import '@/shared/ui/resilience/resilience.css'

export interface FailureCardProps {
  /** Whatever the boundary caught. Classified here; the message is never shown or copied. */
  readonly error: unknown
  readonly region: ClientRegion
  /** The boundary's `reset()`. Hidden for `stale_deploy`, where a retry cannot help. */
  readonly onRetry?: () => void
  readonly compact?: boolean
}

/**
 * The card a failed region shows instead of a blank screen (WS07-S1).
 * `role="alert"`, one sentence per failure class, and only the buttons that can
 * help: Reload always, Retry unless the build is stale, Sign in on an expired
 * session. "Copy details" carries class, build id, region, time and the first
 * path segment — never the error message, query string, ids or content.
 */
export function FailureCard(props: FailureCardProps) {
  const i18n = useI18n()
  const [copied, setCopied] = createSignal(false)
  const errorClass = () => classifyClientError(props.error)
  const copy = () => FAILURE_COPY[errorClass()]

  const copyDetails = async () => {
    const details = buildFailureDetails({ errorClass: errorClass(), region: props.region })
    try {
      await navigator.clipboard.writeText(JSON.stringify(details))
      setCopied(true)
    } catch {
      // Clipboard can be blocked (insecure context, permissions). Nothing else to do.
    }
  }

  return (
    <div
      class={cn('failure-card', props.compact === true && 'failure-card--compact', props.region === 'root' && 'failure-card--root')}
      role="alert"
      data-failure-class={errorClass()}
      data-failure-region={props.region}
    >
      <h2 class="failure-card__title">{i18n.tr(copy().titleNo, copy().titleEn)}</h2>
      <p class="failure-card__body">{i18n.tr(copy().bodyNo, copy().bodyEn)}</p>
      <div class="failure-card__actions">
        <Button variant="primary" size="sm" onClick={() => window.location.reload()}>
          {i18n.tr(FAILURE_ACTIONS.reload.no, FAILURE_ACTIONS.reload.en)}
        </Button>
        <Show when={errorClass() !== 'stale_deploy' && props.onRetry}>
          <Button variant="secondary" size="sm" onClick={() => props.onRetry?.()}>
            {i18n.tr(FAILURE_ACTIONS.retry.no, FAILURE_ACTIONS.retry.en)}
          </Button>
        </Show>
        <Show when={errorClass() === 'session_expired'}>
          <Button variant="secondary" size="sm" onClick={() => window.location.assign('/login')}>
            {i18n.tr(FAILURE_ACTIONS.signIn.no, FAILURE_ACTIONS.signIn.en)}
          </Button>
        </Show>
        <Button variant="ghost" size="sm" onClick={() => void copyDetails()}>
          {copied()
            ? i18n.tr(FAILURE_ACTIONS.copied.no, FAILURE_ACTIONS.copied.en)
            : i18n.tr(FAILURE_ACTIONS.copyDetails.no, FAILURE_ACTIONS.copyDetails.en)}
        </Button>
      </div>
    </div>
  )
}
