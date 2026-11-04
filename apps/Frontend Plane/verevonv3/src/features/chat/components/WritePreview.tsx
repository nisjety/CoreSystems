import { For, Show, createSignal } from 'solid-js'
import { getApprovalPreview, type Approval, type ApprovalPreview } from '@/shared/api/orchestration-client'
import { translateApiError, useI18n } from '@/shared/i18n'

/**
 * The full draft of a paused mail send, shown on its approval card before the person decides it
 * (EP-10-S1, action `approvals.preview`). Read-only: it changes nothing and grants nothing.
 *
 * The body is plain text in a `pre-wrap` block and is never rendered as markup, so a drafted
 * `<script>` or link is shown as the characters it is. A formatted (HTML) copy is only noted.
 */

/** Off unless `VITE_APPROVAL_WRITE_PREVIEW` is the literal `true`; the gateway has its own switch. */
export const approvalWritePreviewEnabled = () => import.meta.env.VITE_APPROVAL_WRITE_PREVIEW === 'true'

/**
 * The opening every mail send's approval reason has (`provider_effect_summary` in mp-contracts,
 * the only reason that names a send). The browser projection of an approval deliberately carries
 * no operation (`action_key` never leaves the gateway), so this is the one signal the card has;
 * the preview itself says what the call is.
 */
const MAIL_SEND_SUMMARY_OPENING = 'send an email to '

/** Whether an approval is a paused mail send, the one write the full-draft preview is for. */
export function isMailSendApproval(approval: Approval): boolean {
  return (
    approval.cause?.tool === 'execute_provider_action' &&
    (approval.detail ?? '').startsWith(MAIL_SEND_SUMMARY_OPENING)
  )
}

export type WritePreviewState =
  | { status: 'loading' }
  | { status: 'unavailable' }
  | { status: 'error'; message: string }
  | { status: 'ready'; preview: ApprovalPreview }

/** One state of the preview: loading, not available, failed, or the draft itself. */
export function WritePreview(props: { state: WritePreviewState }) {
  const i18n = useI18n()
  const ready = () => {
    const state = props.state
    return state.status === 'ready' ? state.preview : undefined
  }
  const mail = () => {
    const preview = ready()
    return preview?.kind === 'mail_send' && !preview.rawOnly ? preview : undefined
  }
  const failure = () => {
    const state = props.state
    return state.status === 'error' ? state.message : undefined
  }
  return (
    <div class="verevon-chat-approval__preview" aria-live="polite">
      <Show when={props.state.status === 'loading'}>
        <p class="verevon-chat-approval__preview-note">{i18n.tr('Henter e-posten …', 'Loading the email …')}</p>
      </Show>
      <Show when={failure()}>
        {(message) => (
          <p class="verevon-chat-approval__preview-note" role="alert">
            {message()}
          </p>
        )}
      </Show>
      <Show
        when={
          props.state.status === 'unavailable' ||
          (ready() !== undefined && mail() === undefined)
        }
      >
        <p class="verevon-chat-approval__preview-note">
          {ready()?.rawOnly
            ? i18n.tr(
                'Denne e-posten er ferdig kodet og kan ikke vises her.',
                'This email is already encoded and cannot be shown here.',
              )
            : i18n.tr('Forhåndsvisning er ikke tilgjengelig.', 'Preview not available.')}
        </p>
      </Show>
      <Show when={mail()}>
        {(preview) => (
          <dl class="verevon-chat-approval__preview-fields">
            <dt>{i18n.tr('Til', 'To')}</dt>
            <dd>
              <ul class="verevon-chat-approval__preview-recipients">
                <For each={preview().to}>{(recipient) => <li>{recipient}</li>}</For>
              </ul>
            </dd>
            <dt>{i18n.tr('Emne', 'Subject')}</dt>
            <dd>{preview().subject || i18n.tr('(uten emne)', '(no subject)')}</dd>
            <dt>{i18n.tr('Melding', 'Message')}</dt>
            <dd>
              <pre class="verevon-chat-approval__preview-body" style={{ 'white-space': 'pre-wrap' }}>
                {preview().bodyText}
              </pre>
            </dd>
            <Show when={preview().isReply}>
              <p class="verevon-chat-approval__preview-note">
                {i18n.tr('Svar i en eksisterende tråd.', 'Reply in an existing thread.')}
              </p>
            </Show>
            <Show when={preview().hasHtml}>
              <p class="verevon-chat-approval__preview-note">
                {i18n.tr(
                  'Formatert e-post: formateringen vises ikke her.',
                  'Formatted mail: the formatting is not shown here.',
                )}
              </p>
            </Show>
          </dl>
        )}
      </Show>
    </div>
  )
}

/**
 * "Show the full email" on a mail send's approval card. The draft is fetched once, on the first
 * open, and kept for the life of the card; closing only hides it.
 */
export function WritePreviewDisclosure(props: {
  approvalId: string
  load?: (approvalId: string) => Promise<ApprovalPreview | null>
}) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [state, setState] = createSignal<WritePreviewState | undefined>(undefined)

  const fetchOnce = async () => {
    if (state() !== undefined) return
    setState({ status: 'loading' })
    try {
      const preview = await (props.load ?? getApprovalPreview)(props.approvalId)
      setState(preview ? { status: 'ready', preview } : { status: 'unavailable' })
    } catch (failure) {
      setState({
        status: 'error',
        message: translateApiError(failure, i18n.tr, {
          no: 'Kunne ikke hente e-posten.',
          en: 'The email could not be loaded.',
        }),
      })
    }
  }

  const toggle = () => {
    const next = !open()
    setOpen(next)
    if (next) void fetchOnce()
  }

  return (
    <div class="verevon-chat-approval__disclosure">
      <button
        type="button"
        class="verevon-chat-approval__disclosure-toggle"
        aria-expanded={open() ? 'true' : 'false'}
        onClick={toggle}
      >
        {open()
          ? i18n.tr('Skjul e-posten', 'Hide the email')
          : i18n.tr('Vis hele e-posten', 'Show the full email')}
      </button>
      <Show when={open() ? state() : undefined}>{(current) => <WritePreview state={current()} />}</Show>
    </div>
  )
}
