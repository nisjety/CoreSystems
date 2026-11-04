import { AlertCircle, Hand, KeyRound, Loader2, Mail } from '@/shared/icons'
import { createSignal, Show } from 'solid-js'
import { decideApproval } from '@/shared/api/orchestration-client'
import { useI18n } from '@/shared/i18n'
import { matchApprovalShortcut } from '@/features/chat/lib/use-chat-shortcuts'
import type { ApprovalDecisionRequest } from './approval-model'
import {
  buildOtpDecisionRequest,
  OTP_BLOCK_COPY,
  OTP_MAIL_PROVIDER_NAME,
  type OtpApprovalCardModel,
} from './otp-approval'
import './browser-approval-card.css'

/**
 * The OTP variant of the approval card (E3.1-i). It shows one `fill_otp` approval
 * with the origin of its page, the origin of the frame that holds the field and the
 * mail connection the code is read from. It says that the approval covers one fill,
 * and it never shows the code.
 *
 * Mount it only when `browserOtpApprovalEnabled()` is true; off, the caller keeps the
 * generic card. Nothing mounts it in this unit: the mount is the approval-card swap
 * (W3-E3.2-b-swap). Styles are the generic card's (browser-approval-card.css).
 *
 * Keys (only inside the card, as on the generic card):
 *   Cmd/Ctrl+Enter approve (refused while Approve is withheld), Cmd/Ctrl+Shift+Enter reject.
 */
export function OtpApprovalNotice(props: {
  approval: OtpApprovalCardModel
  /** Sends the decision. Defaults to the orchestration client's decideApproval. */
  decide?: (request: ApprovalDecisionRequest) => Promise<unknown>
  /** Called after the owner accepted the decision. The caller removes the notice. */
  onDecided?: (request: ApprovalDecisionRequest) => void
  /**
   * "I will type it myself": hands the browser to the person through the chrome's
   * take-over. It sends no decision. The button is shown only when this is passed.
   */
  onTakeOver?: () => void
}) {
  const i18n = useI18n()
  const [deciding, setDeciding] = createSignal(false)
  const [failed, setFailed] = createSignal(false)
  let inFlight = false

  const headingId = () => `verevon-browser-approval-${props.approval.approvalId}`
  const scopeId = () => `${headingId()}-scope`
  const modifier = isApplePlatform() ? '⌘' : 'Ctrl'
  const approveWithheld = () => props.approval.approveBlock !== null
  const hasTakeOver = () => props.onTakeOver !== undefined

  const heading = () => {
    const site = props.approval.topOrigin
    return site
      ? i18n.tr(
          `Skal agenten fylle inn engangskoden på ${site}?`,
          `Should the agent fill the one-time code on ${site}?`,
        )
      : i18n.tr(
          'Skal agenten fylle inn engangskoden på dette nettstedet?',
          'Should the agent fill the one-time code on this site?',
        )
  }

  const frameLine = () => {
    const { topOrigin, frameOrigin } = props.approval
    if (!frameOrigin) {
      return i18n.tr('Nettstedet for feltet kan ikke fastslås.', 'The site of the field cannot be identified.')
    }
    if (frameOrigin === topOrigin) {
      return i18n.tr(
        `Feltet ligger på ${frameOrigin}, samme nettsted som siden.`,
        `The field is on ${frameOrigin}, the same site as the page.`,
      )
    }
    return i18n.tr(
      `Feltet ligger i en ramme på ${frameOrigin}, ikke på siden.`,
      `The field is in a frame on ${frameOrigin}, not on the page.`,
    )
  }

  const mailLine = () => {
    const { mailProvider, mailAccountLabel } = props.approval
    if (!mailProvider) {
      return i18n.tr('Koden hentes fra e-postkoblingen.', 'The code is read from the mail connection.')
    }
    const provider = OTP_MAIL_PROVIDER_NAME[mailProvider]
    const connection = mailAccountLabel ? `${provider} (${mailAccountLabel})` : provider
    return i18n.tr(`Koden hentes fra ${connection}.`, `The code is read from ${connection}.`)
  }

  const send = async (action: 'approve' | 'reject') => {
    if (inFlight) return
    const built = buildOtpDecisionRequest(props.approval, action)
    // Approve is withheld: the reason is shown above the actions.
    if (!built.ok) return
    setFailed(false)
    inFlight = true
    setDeciding(true)
    try {
      const decide = props.decide ?? defaultDecide
      await decide(built.request)
      props.onDecided?.(built.request)
    } catch {
      // The decision did not land: keep the notice so the person can try again.
      setFailed(true)
    } finally {
      inFlight = false
      setDeciding(false)
    }
  }

  const onKeyDown = (event: KeyboardEvent) => {
    const action = matchApprovalShortcut(event, event.target as HTMLElement | null, { redirectText: '' })
    if (action?.type === 'approve') {
      event.preventDefault()
      void send('approve')
    } else if (action?.type === 'reject') {
      event.preventDefault()
      void send('reject')
    }
  }

  return (
    <section
      class="verevon-browser-approval verevon-browser-approval--otp"
      role="group"
      aria-labelledby={headingId()}
      aria-describedby={scopeId()}
      tabindex="-1"
      data-source="browser"
      data-variant="otp"
      onKeyDown={onKeyDown}
      ref={(element: HTMLElement) => {
        // Take focus when nothing else has it, so the shortcuts work at once (as on the generic card).
        queueMicrotask(() => {
          const active = document.activeElement
          if (!active || active === document.body) element.focus({ preventScroll: true })
        })
      }}
    >
      <header class="verevon-browser-approval__head">
        <span class="verevon-browser-approval__badge">
          <KeyRound class="size-3" aria-hidden="true" />
          {i18n.tr('Godkjenning', 'Approval')}
        </span>
        <span class="verevon-browser-approval__source">
          {i18n.tr('Innfylling av engangskode', 'One-time code fill')}
        </span>
      </header>

      <p id={headingId()} class="verevon-browser-approval__reason">
        {heading()}
      </p>

      <p class="verevon-browser-approval__meta">{frameLine()}</p>
      <p class="verevon-browser-approval__meta">
        <Mail class="size-3" aria-hidden="true" /> {mailLine()}
      </p>

      <p id={scopeId()} class="verevon-browser-approval__hint">
        {i18n.tr(
          'Godkjenningen gjelder én innfylling og kan ikke brukes igjen. Den gjelder bare for dette nettstedet og denne e-postkoblingen.',
          'This approval covers one fill and cannot be used again. It applies only to this site and this mail connection.',
        )}
      </p>
      <p class="verevon-browser-approval__hint">
        {i18n.tr('Koden vises ikke her.', 'The code is not shown here.')}
      </p>

      <Show when={props.approval.approveBlock}>
        {(block) => (
          <p class="verevon-browser-approval__error" role="alert">
            <AlertCircle class="size-3" aria-hidden="true" />
            {i18n.tr(OTP_BLOCK_COPY[block()].no, OTP_BLOCK_COPY[block()].en)}
          </p>
        )}
      </Show>

      <div class="verevon-browser-approval__actions">
        <button
          type="button"
          class="verevon-browser-approval__approve"
          disabled={deciding() || approveWithheld()}
          onClick={() => void send('approve')}
        >
          <Show when={deciding()}>
            <Loader2 class="size-3.5 verevon-browser-approval__spin" aria-hidden="true" />
          </Show>
          {i18n.tr('Godkjenn én gang', 'Approve once')}
          <kbd class="verevon-browser-approval__kbd">{modifier}+Enter</kbd>
        </button>
        <button
          type="button"
          class="verevon-browser-approval__reject"
          disabled={deciding()}
          onClick={() => void send('reject')}
        >
          {i18n.tr('Avslå', 'Reject')}
          <kbd class="verevon-browser-approval__kbd">{modifier}+Shift+Enter</kbd>
        </button>
        <Show when={hasTakeOver()}>
          <button
            type="button"
            class="verevon-browser-approval__redirect-send"
            disabled={deciding()}
            onClick={() => props.onTakeOver?.()}
          >
            <Hand class="size-3.5" aria-hidden="true" />
            {i18n.tr('Jeg skriver den selv', 'I will type it myself')}
          </button>
        </Show>
      </div>

      <p class="verevon-browser-approval__status" role="status" aria-live="polite">
        <Show when={deciding()}>{i18n.tr('Sender avgjørelsen…', 'Sending the decision…')}</Show>
      </p>
      <Show when={failed()}>
        <p class="verevon-browser-approval__error" role="alert">
          <AlertCircle class="size-3" aria-hidden="true" />
          {i18n.tr('Avgjørelsen ble ikke sendt. Prøv igjen.', 'The decision was not sent. Try again.')}
        </p>
      </Show>
    </section>
  )
}

function defaultDecide(request: ApprovalDecisionRequest): Promise<unknown> {
  return decideApproval(request.approvalId, request.decision, request.reason)
}

function isApplePlatform(): boolean {
  try {
    return /Mac|iPhone|iPad/.test(navigator.platform)
  } catch {
    return false
  }
}
