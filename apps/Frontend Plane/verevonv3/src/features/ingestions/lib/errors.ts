import { ApiError } from '@/shared/api/http'
import { translateApiError } from '@/shared/i18n'

/**
 * Turns a refusal from Quarry into the sentence a person needs. A switched-off
 * capability is named by the Quarry flag that turns it on, taken from the
 * edge's own `feature_disabled` field, so the page never guesses at it.
 */

type Tr = (no: string, en: string) => string

function fieldsOf(error: unknown): Array<Record<string, unknown>> {
  if (!(error instanceof ApiError)) return []
  const fields = error.details?.fields
  if (!Array.isArray(fields)) return []
  return fields.filter((field): field is Record<string, unknown> => typeof field === 'object' && field !== null)
}

/** The Quarry switch behind a scrape refusal (`actions` or screenshot/PDF `formats`), or `null`. */
export function scrapeSwitchFor(error: unknown): string | null {
  for (const field of fieldsOf(error)) {
    if (field.code !== 'feature_disabled') continue
    if (field.path === 'actions') return 'QUARRY_EDGE__SCRAPE_ACTIONS'
    if (field.path === 'formats') return 'QUARRY_EDGE__SCRAPE_CAPTURE'
  }
  return null
}

function isUnsupported(error: unknown): boolean {
  return error instanceof ApiError && (error.status === 501 || error.code?.toLowerCase() === 'unsupported')
}

export function playgroundErrorMessage(
  error: unknown,
  tr: Tr,
  fallback: { no: string; en: string },
  endpoint?: string,
): string {
  if (endpoint === 'extract' && isUnsupported(error)) {
    return tr(
      'Strukturert uttrekk er ikke slått på på Quarry-edge. Det krever QUARRY_EDGE__AI_FORMATS=1 og en modellplan. Fjern JSON-skjemaet for å få Markdown.',
      'Structured extraction is not switched on in Quarry-edge. It needs QUARRY_EDGE__AI_FORMATS=1 and a model plane. Remove the JSON schema to get Markdown.',
    )
  }
  const switchName = scrapeSwitchFor(error)
  if (switchName) {
    return tr(
      `Denne funksjonen er slått av på Quarry-edge. Slå på ${switchName} der, og kjør på nytt.`,
      `This feature is switched off on Quarry-edge. Turn on ${switchName} there and run again.`,
    )
  }
  if (fieldsOf(error).some((field) => field.code === 'reserved_format')) {
    return tr('Quarry tilbyr ikke dette formatet ennå.', 'Quarry does not offer this format yet.')
  }
  return translateApiError(error, tr, fallback)
}

/** Why a pause, resume or cancel was refused: the job-control switch, a job that cannot be controlled, or the generic failure. */
export function jobControlMessage(error: unknown, tr: Tr): string {
  if (isUnsupported(error)) {
    return tr(
      'Jobbkontroll er slått av på Quarry-edge. Slå på QUARRY_EDGE__JOB_CONTROL der, og prøv igjen.',
      'Job control is switched off on Quarry-edge. Turn on QUARRY_EDGE__JOB_CONTROL there and try again.',
    )
  }
  if (error instanceof ApiError && error.code === 'not_controllable') {
    return tr('Denne jobben kan ikke settes på pause, gjenopptas eller avbrytes.', 'This job cannot be paused, resumed or cancelled.')
  }
  return translateApiError(error, tr, {
    no: 'Jobben kunne ikke styres.',
    en: 'The job could not be controlled.',
  })
}
