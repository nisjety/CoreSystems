import { ApiError } from '@/shared/api/api-error'
import { extractChatDocument, getAttachmentPolicy } from '@/shared/api/chat-client'
import { LocalizedError } from '@/shared/i18n/errors'
import { objectUrlToDataUrl, parseDataUrl } from '@/shared/lib/blob-data'
import { ATTACHMENT_MAX_FILE_BYTES } from './attachment-policy'
import type { PendingChatAttachment } from './pending-chat-launch'

/** Read every attachment before clearing the draft or starting inference. */
export async function prepareChatAttachments(attachments: PendingChatAttachment[]): Promise<PendingChatAttachment[]> {
  // What the server declares it will read; today's constant until it declares one.
  const maxFileBytes = getAttachmentPolicy()?.maxFileBytes ?? ATTACHMENT_MAX_FILE_BYTES
  const prepared: PendingChatAttachment[] = []
  let total = 0
  for (const file of attachments) {
    let url: string
    // A connector-attach picker result already extracted its text at pick
    // time (the network fetch already happened there; see connector-picker
    // spec §2.4) — carry that forward instead of re-sending it through
    // extractChatDocument a second time. Only string | undefined here: a
    // pre-populated value skips the pdf/docx branch below outright.
    let extractedText: string | undefined = file.extractedText
    try {
      if (file.size > maxFileBytes) throw new Error('too large')
      url = file.url.startsWith('blob:') ? await objectUrlToDataUrl(file.url) : file.url
      const response = await fetch(url)
      if (!response.ok) throw new Error('unreadable')
      if (file.type.startsWith('image/') && !parseDataUrl(url)?.base64) throw new Error('invalid image encoding')
      if (!file.type.startsWith('image/')) {
        if (/\.(pdf|docx|xlsx|pptx)$/i.test(file.name)) {
          if (extractedText === undefined) {
            const parsed = parseDataUrl(url)
            if (!parsed) throw new Error('unreadable')
            extractedText = (await extractChatDocument(file.name, parsed.base64, file.type)).content
          }
        } else if (file.type.startsWith('text/') || /\.(txt|md|markdown|csv|json|html|htm)$/i.test(file.name)) {
          extractedText = (await response.text()).replace(/\r\n/g, '\n').trim()
        } else throw new Error('unsupported')
        if (!extractedText || extractedText.length > 60_000) throw new Error('unreadable or too long')
        total += extractedText.length
        if (total > 120_000) throw new Error('too long')
      }
    } catch (error) {
      // A typed refusal from the extraction route (a zero-retention or entitlement code, 413, 429) keeps its own words:
      // the send site answers it by its code. Only a failure nobody typed becomes the generic read refusal.
      if (error instanceof ApiError || error instanceof LocalizedError) throw error
      throw new LocalizedError({
        no: `Kunne ikke lese hele «${file.name}». Utkastet er beholdt. Prøv igjen; hvis det fortsatt feiler, velg en lesbar fil under ${formatMegabytes(maxFileBytes, 'nb-NO')} og 60 000 tegn (maks. 120 000 tegn samlet).`,
        en: `Could not read all of “${file.name}”. Your draft has been kept. Try again; if it still fails, choose a readable file under ${formatMegabytes(maxFileBytes, 'en-US')} and 60,000 characters (at most 120,000 characters in total).`,
      })
    }
    prepared.push({ ...file, url, extractedText })
  }
  return prepared
}

function formatMegabytes(bytes: number, locale: 'nb-NO' | 'en-US'): string {
  return `${(bytes / 1_000_000).toLocaleString(locale, { maximumFractionDigits: 1 })} MB`
}
