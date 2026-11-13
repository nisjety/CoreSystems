import { ApiError } from '@/shared/api/http'
import { translateApiError } from '@/shared/i18n'

/**
 * Copy for a document read: what a warning Quarry attaches to the text means, why a read
 * was refused, and how big a file is. Quarry sends the codes; the page words them.
 */

type Tr = (no: string, en: string) => string

/** What a warning means for the text in front of the person. An unknown code shows Quarry's own message. */
export function documentWarningMessage(warning: { code: string; message: string }, tr: Tr): string {
  switch (warning.code) {
    case 'document_has_no_text_layer':
      return tr(
        'Dokumentet har ingen tekstlag. Det ser ut som en skanning, og Quarry gjør ingen OCR, så teksten er tom.',
        'The document has no text layer. It looks like a scan, and Quarry does no OCR, so the text is empty.',
      )
    case 'some_pages_have_no_text_layer':
      return tr(
        'Noen sider har ingen tekstlag, og teksten fra dem mangler.',
        'Some pages have no text layer, so their text is missing.',
      )
    case 'document_has_no_text':
      return tr('DOCX-filen inneholder ingen tekst.', 'The DOCX file contains no text.')
    case 'truncated_at_8mib':
      return tr('Teksten er kappet ved 8 MiB.', 'The text was cut at 8 MiB.')
    default:
      return warning.message
  }
}

/** Why a read was refused: the switch, the file, the size, the time, or the wait. */
export function documentErrorMessage(error: unknown, tr: Tr, fallback: { no: string; en: string }): string {
  if (!(error instanceof ApiError)) return translateApiError(error, tr, fallback)
  const code = error.code?.toLowerCase() ?? ''
  const details = Array.isArray(error.details) ? null : error.details
  if (error.status === 501) {
    if (code === 'isolation_required' || details?.reason === 'isolation_required') {
      return tr(
        'Quarry leser ikke dokumenter på denne installasjonen ennå, fordi lesingen ikke er isolert.',
        'Quarry does not read documents on this installation yet, because reading is not isolated.',
      )
    }
    if (details?.reason === 'parse_route_disabled') {
      return tr(
        'Dokumentlesing er ikke slått på på Quarry. Slå på QUARRY_EDGE__PARSE_ROUTE der, og prøv igjen.',
        'Document reading is not switched on in Quarry. Turn on QUARRY_EDGE__PARSE_ROUTE there, and try again.',
      )
    }
    return tr('Dokumentlesing er ikke tilgjengelig på denne installasjonen.', 'Document reading is not available on this installation.')
  }
  if (error.status === 429) {
    const seconds = error.retryAfterSeconds
    return seconds
      ? tr(
          `Det leses for mange dokumenter akkurat nå. Prøv igjen om ${seconds} sekunder.`,
          `Too many documents are being read right now. Try again in ${seconds} seconds.`,
        )
      : tr('Det leses for mange dokumenter akkurat nå. Prøv igjen om litt.', 'Too many documents are being read right now. Try again shortly.')
  }
  if (error.status === 504 || code === 'timeout' || code === 'upstream_timeout') {
    return tr('Lesingen tok for lang tid. Prøv med et kortere dokument.', 'Reading took too long. Try a shorter document.')
  }
  switch (code) {
    case 'too_large':
      return tr('Filen er større enn 25 MiB. Velg en mindre fil.', 'The file is larger than 25 MiB. Choose a smaller file.')
    case 'empty_body':
      return tr('Filen er tom.', 'The file is empty.')
    case 'body_unreadable':
      return tr('Nettleseren kunne ikke lese filen. Prøv igjen.', 'The browser could not read the file. Try again.')
    case 'unsupported_media':
      return tr('Filen er verken PDF eller DOCX. Velg en PDF- eller DOCX-fil.', 'The file is neither a PDF nor a DOCX. Choose a PDF or DOCX file.')
    case 'content_type_mismatch':
      return tr(
        'Innholdet stemmer ikke med filtypen. Sjekk at filen er en ekte PDF eller DOCX, og ikke en omdøpt fil.',
        'The content does not match the file type. Check that the file is a real PDF or DOCX, not a renamed file.',
      )
    case 'pdf_parse_failed':
      return tr('PDF-filen kunne ikke leses. Den kan være skadet eller passordbeskyttet.', 'The PDF could not be read. It may be damaged or password-protected.')
    case 'docx_parse_failed':
      return tr('DOCX-filen kunne ikke leses. Den kan være skadet.', 'The DOCX could not be read. It may be damaged.')
    default:
      return translateApiError(error, tr, fallback)
  }
}

/** A file size for a person: bytes, kilobytes or megabytes. */
export function documentSizeLabel(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}
