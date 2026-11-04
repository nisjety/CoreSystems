import { For, Show, createEffect, createSignal } from 'solid-js'
import {
  getApprovalPreview,
  type Approval,
  type ApprovalEdit,
  type ApprovalPreview,
} from '@/shared/api/orchestration-client'
import {
  MAX_BODY_TEXT_BYTES,
  MAX_RECIPIENTS,
  MAX_RECIPIENT_BYTES,
  MAX_SUBJECT_BYTES,
  checkMailEditLimits,
  type ApprovalEditCode,
  type MailSendPatch,
} from '@/shared/approvals/approval-edit-limits'
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

/** The mail fields the person may change before approving (EP-10-S6, decision D2). */
export type MailEditFields = {
  to: string[]
  subject: string
  bodyText: string
}

/**
 * The person's pending changes to a draft: the edit to send and the first limit it breaks, or null
 * when it may be sent. The server (session-core, mp-contracts `approval_edit.rs`) is the authority;
 * `problem` only keeps the form from sending what it will refuse.
 */
export type MailEditDraft = {
  edit: ApprovalEdit
  problem: ApprovalEditCode | null
}

const utf8Bytes = (text: string): number => new TextEncoder().encode(text).length

// One address: local part, `@`, a dotted domain, with none of the characters that would let one
// entry name two mailboxes or break a header. Bare (`a@example.com`) or named (`Ola <a@example.com>`).
const ADDRESS = String.raw`[^\s@<>(),;:"\[\]\\]+@[^\s@<>(),;:"\[\]\\]+\.[^\s@<>(),;:"\[\]\\]+`
const BARE_RECIPIENT = new RegExp(`^${ADDRESS}$`)
const NAMED_RECIPIENT = new RegExp(`^[^<>\\r\\n,;]*<${ADDRESS}>$`)

/**
 * Whether a recipient looks like one RFC 5322 mailbox. A first check for the form only: the
 * server parses every address again and refuses `invalid_recipient` on its own.
 */
export function recipientLooksValid(value: string): boolean {
  const trimmed = value.trim()
  if (trimmed.length === 0 || utf8Bytes(trimmed) > MAX_RECIPIENT_BYTES) return false
  return BARE_RECIPIENT.test(trimmed) || NAMED_RECIPIENT.test(trimmed)
}

/**
 * Whether the person may edit this draft: the gateway says so (`can_edit`, server-controlled), it
 * is a normalised mail send (a Gmail `raw` draft is preview-only) and it names editable fields.
 */
export function previewIsEditable(preview: ApprovalPreview): boolean {
  return (
    preview.canEdit === true &&
    preview.kind === 'mail_send' &&
    !preview.rawOnly &&
    preview.editableFields.length > 0
  )
}

/** The form's starting values: exactly the draft. */
export function mailEditFieldsOf(preview: ApprovalPreview): MailEditFields {
  return { to: [...preview.to], subject: preview.subject, bodyText: preview.bodyText }
}

const sameRecipients = (left: readonly string[], right: readonly string[]): boolean =>
  left.length === right.length && left.every((recipient, index) => recipient === right[index])

/**
 * The edit the form holds against its draft, or null when nothing changed. Only a changed, editable
 * field goes into the patch, so an unchanged form is a plain approval and editing only the subject
 * keeps a formatted body. Problems are checked in the server's order: recipients, subject, body.
 */
export function mailEditDraft(preview: ApprovalPreview, fields: MailEditFields): MailEditDraft | null {
  const editable = new Set(preview.editableFields)
  const patch: MailSendPatch = {}
  const to = fields.to.map((recipient) => recipient.trim())
  if (editable.has('to') && !sameRecipients(to, preview.to)) patch.to = to
  if (editable.has('subject') && fields.subject !== preview.subject) patch.subject = fields.subject
  if (editable.has('body_text') && fields.bodyText !== preview.bodyText) patch.bodyText = fields.bodyText
  if (patch.to === undefined && patch.subject === undefined && patch.bodyText === undefined) return null
  const recipientProblem =
    patch.to === undefined
      ? null
      : (checkMailEditLimits({ to: patch.to }) ??
        (patch.to.every(recipientLooksValid) ? null : 'invalid_recipient'))
  return {
    edit: { patch, baseFingerprint: preview.actionFingerprint },
    problem: recipientProblem ?? checkMailEditLimits({ subject: patch.subject, bodyText: patch.bodyText }),
  }
}

type Translate = ReturnType<typeof useI18n>['tr']

/** What the person reads for a limit their edit breaks, or a refusal the server sent back. */
export function mailEditProblemText(code: string, tr: Translate): string {
  switch (code) {
    case 'invalid_recipient':
      return tr(
        'Minst én mottaker er ikke en gyldig e-postadresse.',
        'At least one recipient is not a valid email address.',
      )
    case 'too_many_recipients':
      return tr(`Høyst ${MAX_RECIPIENTS} mottakere.`, `At most ${MAX_RECIPIENTS} recipients.`)
    case 'invalid_subject':
      return tr(
        `Emnet kan ikke ha linjeskift og høyst ${MAX_SUBJECT_BYTES} byte.`,
        `The subject cannot have line breaks and is at most ${MAX_SUBJECT_BYTES} bytes.`,
      )
    case 'invalid_body':
      return tr('Meldingen kan ikke være tom.', 'The message cannot be empty.')
    case 'edit_too_large':
      return tr('Meldingen er for lang.', 'The message is too long.')
    case 'no_change':
      return tr('Ingenting er endret.', 'Nothing was changed.')
    case 'not_editable':
      return tr(
        'Denne handlingen kan ikke endres. Godkjenn eller avvis den slik den er.',
        'This action cannot be edited. Approve or reject it as drafted.',
      )
    case 'revision_limit':
      return tr('Handlingen er endret for mange ganger.', 'This action was edited too many times.')
    case 'approval_changed':
      return tr(
        'Utkastet er endret siden du åpnet det. Se over det nye utkastet; din versjon står nedenfor.',
        'The draft changed since you opened it. Review the new draft; your version is kept below.',
      )
    default:
      return tr('Endringene ble ikke tatt imot.', 'The edits were not accepted.')
  }
}

/**
 * The editable draft (EP-10-S6): recipient chips, subject and body, with live limit counters. It
 * holds no state of its own beyond the address being typed; every change goes to `onChange`.
 */
function MailEditor(props: {
  preview: ApprovalPreview
  fields: MailEditFields
  problem: ApprovalEditCode | null
  onChange: (fields: MailEditFields) => void
  onDiscard: () => void
}) {
  const i18n = useI18n()
  const [typed, setTyped] = createSignal('')
  const editable = (field: string) => props.preview.editableFields.includes(field)

  const addTyped = () => {
    const entries = typed()
      .split(/[,;\n]/)
      .map((entry) => entry.trim())
      .filter((entry) => entry.length > 0)
    if (entries.length === 0) return
    setTyped('')
    props.onChange({ ...props.fields, to: [...props.fields.to, ...entries] })
  }

  const remove = (index: number) =>
    props.onChange({ ...props.fields, to: props.fields.to.filter((_, at) => at !== index) })

  const bodyChanged = () => props.fields.bodyText !== props.preview.bodyText

  return (
    <div class="verevon-chat-approval__edit">
      <Show when={editable('to')}>
        <fieldset class="verevon-chat-approval__edit-recipients">
          <legend>
            {i18n.tr('Til', 'To')}{' '}
            <span class="verevon-chat-approval__edit-count">
              {props.fields.to.length} / {MAX_RECIPIENTS}
            </span>
          </legend>
          <ul class="verevon-chat-approval__edit-chips">
            <For each={props.fields.to}>
              {(recipient, index) => (
                <li
                  class="verevon-chat-approval__edit-chip"
                  data-invalid={recipientLooksValid(recipient) ? undefined : ''}
                >
                  <span>{recipient}</span>
                  <button
                    type="button"
                    aria-label={i18n.tr(`Fjern ${recipient}`, `Remove ${recipient}`)}
                    onClick={() => remove(index())}
                  >
                    ×
                  </button>
                </li>
              )}
            </For>
          </ul>
          <label class="verevon-chat-approval__edit-label">
            {i18n.tr('Legg til mottaker', 'Add a recipient')}
            <input
              type="email"
              multiple
              autocomplete="off"
              value={typed()}
              onInput={(event) => setTyped(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter' || event.key === ',') {
                  event.preventDefault()
                  addTyped()
                }
              }}
            />
          </label>
          <button type="button" class="verevon-chat-approval__edit-add" onClick={addTyped}>
            {i18n.tr('Legg til', 'Add')}
          </button>
        </fieldset>
      </Show>
      <Show when={editable('subject')}>
        <label class="verevon-chat-approval__edit-label">
          {i18n.tr('Emne', 'Subject')}{' '}
          <span class="verevon-chat-approval__edit-count">
            {utf8Bytes(props.fields.subject.trim())} / {MAX_SUBJECT_BYTES}
          </span>
          <input
            type="text"
            value={props.fields.subject}
            onInput={(event) => props.onChange({ ...props.fields, subject: event.currentTarget.value })}
          />
        </label>
      </Show>
      <Show when={editable('body_text')}>
        <label class="verevon-chat-approval__edit-label">
          {i18n.tr('Melding', 'Message')}{' '}
          <span class="verevon-chat-approval__edit-count">
            {utf8Bytes(props.fields.bodyText)} / {MAX_BODY_TEXT_BYTES}
          </span>
          <textarea
            rows={8}
            value={props.fields.bodyText}
            onInput={(event) => props.onChange({ ...props.fields, bodyText: event.currentTarget.value })}
          />
        </label>
        <Show when={props.preview.hasHtml && bodyChanged()}>
          <p class="verevon-chat-approval__preview-note">
            {i18n.tr(
              'Formateringen fjernes: e-posten sendes som ren tekst.',
              'Formatting will be removed: the email is sent as plain text.',
            )}
          </p>
        </Show>
      </Show>
      <Show when={props.problem}>
        {(code) => (
          <p class="verevon-chat-approval__error" role="alert">
            {mailEditProblemText(code(), i18n.tr)}
          </p>
        )}
      </Show>
      <button type="button" class="verevon-chat-approval__edit-discard" onClick={() => props.onDiscard()}>
        {i18n.tr('Forkast endringene', 'Discard edits')}
      </button>
    </div>
  )
}

/** The person's own text, kept read-only after the draft changed under them (409 approval_changed). */
function YourVersion(props: { fields: MailEditFields }) {
  const i18n = useI18n()
  return (
    <div class="verevon-chat-approval__your-version" role="note">
      <p class="verevon-chat-approval__preview-note">{i18n.tr('Din versjon', 'Your version')}</p>
      <dl class="verevon-chat-approval__preview-fields">
        <dt>{i18n.tr('Til', 'To')}</dt>
        <dd>{props.fields.to.join(', ')}</dd>
        <dt>{i18n.tr('Emne', 'Subject')}</dt>
        <dd>{props.fields.subject}</dd>
        <dt>{i18n.tr('Melding', 'Message')}</dt>
        <dd>
          <pre class="verevon-chat-approval__preview-body" style={{ 'white-space': 'pre-wrap' }}>
            {props.fields.bodyText}
          </pre>
        </dd>
      </dl>
    </div>
  )
}

/**
 * "Show the full email" on a mail send's approval card. The draft is fetched once, on the first
 * open, and kept for the life of the card; closing only hides it.
 *
 * Edit mode (EP-10-S6) is offered only when the card can send an edited approval (`editable`) and
 * the gateway marked the draft `can_edit`. Every change is reported through `onDraftChange` (null
 * when the form equals the draft). Each increase of `conflict` (the card's 409 approval_changed)
 * refetches the draft, starts the form again from it and keeps the person's text as "your version".
 */
export function WritePreviewDisclosure(props: {
  approvalId: string
  load?: (approvalId: string) => Promise<ApprovalPreview | null>
  editable?: boolean
  onDraftChange?: (draft: MailEditDraft | null) => void
  conflict?: number
}) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [state, setState] = createSignal<WritePreviewState | undefined>(undefined)
  const [fields, setFields] = createSignal<MailEditFields | null>(null)
  const [problem, setProblem] = createSignal<ApprovalEditCode | null>(null)
  const [yourVersion, setYourVersion] = createSignal<MailEditFields | null>(null)

  // Answers the draft it read (or null), so a caller acts on it without re-reading the signal.
  const load = async (): Promise<ApprovalPreview | null> => {
    setState({ status: 'loading' })
    try {
      const preview = await (props.load ?? getApprovalPreview)(props.approvalId)
      setState(preview ? { status: 'ready', preview } : { status: 'unavailable' })
      return preview
    } catch (failure) {
      setState({
        status: 'error',
        message: translateApiError(failure, i18n.tr, {
          no: 'Kunne ikke hente e-posten.',
          en: 'The email could not be loaded.',
        }),
      })
      return null
    }
  }

  const fetchOnce = async () => {
    if (state() !== undefined) return
    await load()
  }

  const toggle = () => {
    const next = !open()
    setOpen(next)
    if (next) void fetchOnce()
  }

  const ready = () => {
    const current = state()
    return current?.status === 'ready' ? current.preview : undefined
  }

  const editablePreview = () => {
    const preview = ready()
    return props.editable && preview && previewIsEditable(preview) ? preview : undefined
  }

  // The draft and the form while the person is editing it; undefined shows the read-only preview.
  const editing = () => {
    const preview = editablePreview()
    const form = fields()
    return preview && form ? { preview, form } : undefined
  }

  // Every change is reported from the values just computed: a Solid 2 signal read right after
  // its write may still answer the old value until the next flush.
  const change = (preview: ApprovalPreview, next: MailEditFields) => {
    setFields(next)
    const draft = mailEditDraft(preview, next)
    setProblem(draft?.problem ?? null)
    props.onDraftChange?.(draft)
  }

  const startEditing = (preview: ApprovalPreview) => {
    setYourVersion(null)
    change(preview, mailEditFieldsOf(preview))
  }

  const discard = () => {
    setFields(null)
    setProblem(null)
    setYourVersion(null)
    props.onDraftChange?.(null)
  }

  createEffect(
    () => props.conflict ?? 0,
    (conflict) => {
      if (conflict === 0) return
      const kept = fields()
      setYourVersion(kept)
      setFields(null)
      setProblem(null)
      props.onDraftChange?.(null)
      setOpen(true)
      void load().then((preview) => {
        if (kept && preview && props.editable && previewIsEditable(preview)) {
          change(preview, mailEditFieldsOf(preview))
        }
      })
    },
  )

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
      <Show when={open() ? state() : undefined}>
        {(current) => (
          <Show
            when={editing()}
            fallback={
              <>
                <WritePreview state={current()} />
                <Show when={editablePreview()}>
                  {(preview) => (
                    <button
                      type="button"
                      class="verevon-chat-approval__edit-start"
                      onClick={() => startEditing(preview())}
                    >
                      {i18n.tr('Endre før sending', 'Edit before sending')}
                    </button>
                  )}
                </Show>
              </>
            }
          >
            {(session) => (
              <MailEditor
                preview={session().preview}
                fields={session().form}
                problem={problem()}
                onChange={(next) => change(session().preview, next)}
                onDiscard={discard}
              />
            )}
          </Show>
        )}
      </Show>
      <Show when={yourVersion()}>{(kept) => <YourVersion fields={kept()} />}</Show>
    </div>
  )
}
