// What the person may change on a paused mail send before approving it, and
// the limits the server holds the edit to (EP-10-S3, decision D2). The server
// (session-core, through mp-contracts `approval_edit.rs`) is the authority;
// these constants only let the form refuse an edit before it is sent. Both
// sides read the golden `./contracts/approval-edit-cases.json`: a byte-identical
// copy of apps/Model Plane/rust/crates/mp-contracts/tests/fixtures/approval-edit-cases.json,
// pinned by sha256 beside it.

/** Fewest recipients a mail send may have. */
export const MIN_RECIPIENTS = 1
/** Most recipients a mail send may have. */
export const MAX_RECIPIENTS = 50
/** Longest subject in UTF-8 bytes, after trimming. */
export const MAX_SUBJECT_BYTES = 998
/** Longest edited plain-text body in UTF-8 bytes (48 KiB). */
export const MAX_BODY_TEXT_BYTES = 48 * 1024
/** Longest single recipient in UTF-8 bytes. */
export const MAX_RECIPIENT_BYTES = 320

/** The closed refusal codes of an edit, in the server's order. */
export const APPROVAL_EDIT_CODES = [
  'not_editable',
  'invalid_recipient',
  'too_many_recipients',
  'invalid_subject',
  'invalid_body',
  'edit_too_large',
  'no_change',
] as const

export type ApprovalEditCode = (typeof APPROVAL_EDIT_CODES)[number]

/** The field names the server reports as edited, in its fixed order. */
export const APPROVAL_EDITED_FIELDS = ['to', 'subject', 'body_text'] as const

export type ApprovalEditedField = (typeof APPROVAL_EDITED_FIELDS)[number]

/** A person's changes to a drafted mail send; the keys are the mail body's own. */
export type MailSendPatch = {
  to?: string[]
  subject?: string
  bodyText?: string
}

/** The sha256 of `./contracts/approval-edit-cases.json`. Changing the golden changes this too. */
export const APPROVAL_EDIT_CONTRACT_SHA256 = 'e54250ebe399854c1464390f0ebfc49d2dde578f401edda61beadf116690c792'

const utf8Bytes = (text: string): number => new TextEncoder().encode(text).length

/**
 * The first limit the patch breaks, checked in the server's order
 * (recipients, subject, body), or null when only the server can tell: whether
 * the call is editable at all, whether an address parses, and whether anything
 * changed.
 */
export function checkMailEditLimits(patch: MailSendPatch): ApprovalEditCode | null {
  if (patch.to !== undefined) {
    if (patch.to.length > MAX_RECIPIENTS) return 'too_many_recipients'
    if (patch.to.length < MIN_RECIPIENTS) return 'invalid_recipient'
  }
  if (patch.subject !== undefined) {
    if (/[\r\n]/.test(patch.subject)) return 'invalid_subject'
    if (utf8Bytes(patch.subject.trim()) > MAX_SUBJECT_BYTES) return 'invalid_subject'
  }
  if (patch.bodyText !== undefined) {
    if (utf8Bytes(patch.bodyText) > MAX_BODY_TEXT_BYTES) return 'edit_too_large'
    if (patch.bodyText.trim().length === 0) return 'invalid_body'
  }
  return null
}
