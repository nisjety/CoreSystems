import { AlertCircle, Loader2, Pause } from '@/shared/icons'
import { createMemo, createSignal, Show } from 'solid-js'
import { decideApproval } from '@/shared/api/orchestration-client'
import { useI18n } from '@/shared/i18n'
import { matchApprovalShortcut } from '@/features/chat/lib/use-chat-shortcuts'
import {
  APPROVAL_REDIRECT_MAX,
  approvalRiskCopy,
  browserGateOnOtpCard,
  buildDecisionRequest,
  type ApprovalCardAction,
  type ApprovalCardModel,
  type ApprovalDecisionRequest,
} from './approval-model'
import { OtpApprovalNotice } from './OtpApprovalNotice'
import { otpApprovalToCardModel, type OtpApprovalCardModel } from './otp-approval'
import './browser-approval-card.css'

type BrowserApprovalDecisionCardProps = {
  approval: ApprovalCardModel
  /** Offer the redirect box. Off by default; turn on when the planner path accepts it. */
  allowRedirect?: boolean
  /** Sends the decision. Defaults to the orchestration client's decideApproval. */
  decide?: (request: ApprovalDecisionRequest) => Promise<unknown>
  /** Called after the owner accepted the decision. The caller removes the card. */
  onDecided?: (request: ApprovalDecisionRequest) => void
  /**
   * "I will type it myself" on a one-time-code fill: the chrome's takeover. It sends no
   * decision. Only the OTP card shows it, and only when this is passed.
   */
  onTakeOver?: () => void
}

/**
 * The one keyboard-first approval card (E3.2-b). With VITE_BROWSER_APPROVAL_CARD
 * set to `1` (default off) it replaces the browser-action gate
 * (BrowserApprovalCard in BrowserChrome.tsx, in the AI bubble and the evidence
 * drawer) and, for approvals it can express, the chat approval deck
 * (ApprovalRequests in ChatMessages.tsx). Both callers pass their own `decide`.
 *
 * Keys (only inside the card, see matchApprovalShortcut):
 *   Cmd/Ctrl+Enter approve, Cmd/Ctrl+Shift+Enter reject,
 *   Enter in the redirect box sends it, Escape clears it.
 *
 * Redirect text is optional and ships dark: it is offered only when the caller
 * passes `allowRedirect`. Until the planner path takes redirect text (E3.2 (B),
 * execution-core browser_agent.rs), a reject with text still aborts the run.
 *
 * A browser one-time-code fill (E3.1-i) gets OtpApprovalNotice instead of this card
 * while VITE_BROWSER_OTP_APPROVAL_CARD is on. The notice replaces the whole card, not
 * only its body: it carries its own Approve, which waits for the origins, so the
 * generic Approve must not show beside it.
 */
export function BrowserApprovalDecisionCard(props: BrowserApprovalDecisionCardProps) {
  // A browser one-time-code fill is shown on the OTP card while its flag is on, never on the generic card.
  const onOtpCard = () => props.approval.source === 'browser' && browserGateOnOtpCard(props.approval.actionType)
  // The OTP card needs the approval id. A fill without one shows nothing, so no generic Approve appears.
  const otpModel = createMemo<OtpApprovalCardModel | null>(() => {
    if (!onOtpCard()) return null
    const approval = props.approval
    return otpApprovalToCardModel({
      approvalId: approval.approvalId,
      topOrigin: approval.topOrigin ?? '',
      frameOrigin: approval.frameOrigin ?? '',
      mailProviderKey: approval.mailProviderKey ?? '',
      mailAccountLabel: approval.mailAccountLabel ?? '',
    })
  })
  return (
    <Show
      when={!onOtpCard()}
      fallback={
        <Show when={otpModel()}>
          {(model) => (
            <OtpApprovalNotice
              approval={model()}
              decide={props.decide}
              onDecided={props.onDecided}
              onTakeOver={props.onTakeOver}
            />
          )}
        </Show>
      }
    >
      <GenericBrowserApprovalDecisionCard {...props} />
    </Show>
  )
}

/** The generic card: the URL, the selector and the risk shown as text, with Approve and Reject. */
function GenericBrowserApprovalDecisionCard(props: BrowserApprovalDecisionCardProps) {
  const i18n = useI18n()
  const [redirectText, setRedirectText] = createSignal('')
  const [deciding, setDeciding] = createSignal(false)
  const [failed, setFailed] = createSignal(false)
  const [redirectTooLong, setRedirectTooLong] = createSignal(false)
  let inFlight = false

  const headingId = () => `verevon-browser-approval-${props.approval.approvalId}`
  const risk = createMemo(() => approvalRiskCopy(props.approval.riskCategory))
  const redirectOn = () => props.allowRedirect === true
  const modifier = isApplePlatform() ? '⌘' : 'Ctrl'

  const send = async (action: ApprovalCardAction) => {
    if (inFlight) return
    const built = buildDecisionRequest(props.approval, action, redirectOn() ? redirectText() : '')
    if (!built.ok) {
      setRedirectTooLong(built.reason === 'too_long')
      return
    }
    setRedirectTooLong(false)
    setFailed(false)
    inFlight = true
    setDeciding(true)
    try {
      const decide = props.decide ?? defaultDecide
      await decide(built.request)
      props.onDecided?.(built.request)
    } catch {
      // The decision did not land: keep the card pending so the person can retry.
      setFailed(true)
    } finally {
      inFlight = false
      setDeciding(false)
    }
  }

  const onKeyDown = (event: KeyboardEvent) => {
    const action = matchApprovalShortcut(
      event,
      event.target as HTMLElement | null,
      { redirectText: redirectOn() ? redirectText() : '' },
    )
    if (!action) return
    event.preventDefault()
    if (action.type === 'approve') void send('approve')
    else if (action.type === 'reject') void send('reject')
    else if (action.type === 'submit-redirect') void send('redirect')
    else setRedirectText('')
  }

  return (
    <section
      class="verevon-browser-approval"
      role="group"
      aria-labelledby={headingId()}
      tabindex="-1"
      data-source={props.approval.source}
      onKeyDown={onKeyDown}
      ref={(element: HTMLElement) => {
        // Keyboard-first: take focus when nothing else has it, so the shortcuts work
        // at once. A focus already held by the composer or a field is left alone.
        queueMicrotask(() => {
          const active = document.activeElement
          if (!active || active === document.body) element.focus({ preventScroll: true })
        })
      }}
    >
      <header class="verevon-browser-approval__head">
        <span class="verevon-browser-approval__badge">
          <Pause class="size-3" aria-hidden="true" />
          {i18n.tr('Godkjenning', 'Approval')}
        </span>
        <span class="verevon-browser-approval__source">
          {props.approval.source === 'browser'
            ? i18n.tr('Nettleserhandling', 'Browser action')
            : i18n.tr('Agentkjøring', 'Agent run')}
        </span>
      </header>

      <p id={headingId()} class="verevon-browser-approval__reason">
        {props.approval.reason || i18n.tr(
          'Handlingen krever godkjenning før den utføres.',
          'This action needs approval before it runs.',
        )}
      </p>

      <Show when={props.approval.actionType || props.approval.target || risk()}>
        <p class="verevon-browser-approval__meta">
          <Show when={props.approval.actionType}>
            <span>{props.approval.actionType}</span>
          </Show>
          <Show when={props.approval.target}>
            <span> {props.approval.target}</span>
          </Show>
          <Show when={risk()}>
            {(copy) => <span class="verevon-browser-approval__risk"> {i18n.tr(copy().no, copy().en)}</span>}
          </Show>
        </p>
      </Show>

      <Show when={redirectOn()}>
        <div class="verevon-browser-approval__redirect-block">
          <label class="verevon-browser-approval__redirect-label" for={`${headingId()}-redirect`}>
            {i18n.tr('Si hva agenten skal gjøre i stedet (valgfritt)', 'Tell the agent what to do instead (optional)')}
          </label>
          <textarea
            id={`${headingId()}-redirect`}
            class="verevon-browser-approval__redirect"
            rows={2}
            maxlength={APPROVAL_REDIRECT_MAX}
            value={redirectText()}
            disabled={deciding()}
            onInput={(event) => {
              setRedirectText(event.currentTarget.value)
            }}
          />
          <p class="verevon-browser-approval__hint">
            {i18n.tr(
              'Enter sender. Shift+Enter gir linjeskift. Esc tømmer.',
              'Enter sends. Shift+Enter adds a line. Esc clears.',
            )}
          </p>
        </div>
      </Show>

      <div class="verevon-browser-approval__actions">
        <button
          type="button"
          class="verevon-browser-approval__approve"
          disabled={deciding()}
          onClick={() => void send('approve')}
        >
          <Show when={deciding()}>
            <Loader2 class="size-3.5 verevon-browser-approval__spin" aria-hidden="true" />
          </Show>
          {i18n.tr('Godkjenn', 'Approve')}
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
        <Show when={redirectOn()}>
          <button
            type="button"
            class="verevon-browser-approval__redirect-send"
            disabled={deciding() || redirectText().trim().length === 0}
            onClick={() => void send('redirect')}
          >
            {i18n.tr('Send beskjed', 'Send instruction')}
          </button>
        </Show>
      </div>

      <p class="verevon-browser-approval__status" role="status" aria-live="polite">
        <Show when={deciding()}>{i18n.tr('Sender avgjørelsen…', 'Sending the decision…')}</Show>
      </p>
      <Show when={redirectTooLong()}>
        <p class="verevon-browser-approval__error" role="alert">
          {i18n.tr(
            `Beskjeden kan være høyst ${APPROVAL_REDIRECT_MAX} tegn.`,
            `The instruction can be at most ${APPROVAL_REDIRECT_MAX} characters.`,
          )}
        </p>
      </Show>
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
