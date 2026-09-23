import { extractChatDocument } from '@/shared/api/chat-client'
import { objectUrlToDataUrl, parseDataUrl } from '@/shared/lib/blob-data'
import type { PendingChatAttachment } from './pending-chat-launch'

/** Read every attachment before clearing the draft or starting inference. */
export async function prepareChatAttachments(attachments: PendingChatAttachment[]): Promise<PendingChatAttachment[]> {
  const prepared: PendingChatAttachment[] = []
  let total = 0
  for (const file of attachments) {
    let url: string
    let extractedText: string | undefined
    try {
      if (file.size > 1_000_000) throw new Error('too large')
      url = file.url.startsWith('blob:') ? await objectUrlToDataUrl(file.url) : file.url
      const response = await fetch(url)
      if (!response.ok) throw new Error('unreadable')
      if (file.type.startsWith('image/') && !parseDataUrl(url)?.base64) throw new Error('invalid image encoding')
      if (!file.type.startsWith('image/')) {
        if (/\.(pdf|docx)$/i.test(file.name)) {
          const parsed = parseDataUrl(url)
          if (!parsed) throw new Error('unreadable')
          extractedText = (await extractChatDocument(file.name, parsed.base64, file.type)).content
        } else if (file.type.startsWith('text/') || /\.(txt|md|markdown|csv|json|html|htm)$/i.test(file.name)) {
          extractedText = (await response.text()).replace(/\r\n/g, '\n').trim()
        } else throw new Error('unsupported')
        if (!extractedText || extractedText.length > 60_000) throw new Error('unreadable or too long')
        total += extractedText.length
        if (total > 120_000) throw new Error('too long')
      }
    } catch {
      throw new Error(`Kunne ikke lese hele «${file.name}». Utkastet er beholdt. Prøv igjen; hvis det fortsatt feiler, velg en lesbar fil under 1 MB og 60 000 tegn (maks. 120 000 tegn samlet).`)
    }
    prepared.push({ ...file, url, extractedText })
  }
  return prepared
}
