/**
 * Paste-to-attachment helpers shared by the Home composer and the room composer (WS15-S3).
 * Extracted verbatim from `DashboardComposer.tsx` so a surface that gains an upload path (WS14 in
 * the room) imports them instead of copying them. Pure: they read a clipboard event and build a
 * `File`; nothing here stores or sends anything.
 */

/**
 * ChatGPT and Claude both convert a sufficiently long paste into a "pasted
 * text" attachment instead of dumping it inline into the composer
 * (CHAT_PARITY_AUDIT_2026-09-15.md §3.6 / F-... "long paste"). This is the
 * length, in `string.length` code units, above which Verevon does the same.
 * It sits in the "a few thousand characters" range those products use: short
 * enough that a genuine document-scale paste (a log dump, an article, a long
 * spec) becomes a tidy attachment chip, long enough that an ordinary
 * paragraph-or-two paste still lands inline exactly as before.
 */
export const PASTE_TEXT_ATTACHMENT_THRESHOLD = 4000

/** The image files on a paste event, if any. */
export function pastedImageFiles(event: ClipboardEvent): File[] {
  return Array.from(event.clipboardData?.files ?? []).filter((file) => file.type.startsWith('image/'))
}

/** The plain text on a paste event, or an empty string. */
export function pastedPlainText(event: ClipboardEvent): string {
  return event.clipboardData?.getData('text/plain') ?? ''
}

/** A short, readable name for a paste-generated text attachment, e.g. "Limt inn tekst: some preview text….txt". */
export function pastedTextAttachmentName(text: string, label: string): string {
  const trimmed = text.trim()
  const preview = trimmed.replace(/\s+/g, ' ').slice(0, 60)
  if (preview.length === 0) {
    return `${label} ${new Date().toLocaleTimeString()}.txt`
  }
  const truncated = trimmed.length > preview.length ? '…' : ''
  return `${label}: ${preview}${truncated}.txt`
}

/** Wraps pasted text as a `.txt`-equivalent File so it flows through the existing attachment pipeline (`addFiles`). */
export function pastedTextAttachmentFile(text: string, label: string): File {
  return new File([text], pastedTextAttachmentName(text, label), {
    type: 'text/plain',
  })
}
