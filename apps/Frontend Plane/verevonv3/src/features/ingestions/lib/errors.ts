import { ApiError } from '@/shared/api/http'
import { translateApiError } from '@/shared/i18n'
import { isAiFormat, type ScrapeFormat } from '@/features/ingestions/lib/playground'

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

function reasonOf(error: unknown): string | null {
  if (!(error instanceof ApiError)) return null
  const reason = error.details?.reason
  return typeof reason === 'string' ? reason : null
}

/**
 * The Quarry switch behind a scrape refusal, or `null`. `actions` names the browser
 * steps switch. A `formats[i]` refusal (or `formats`) is either the AI-format switch
 * or the capture switch; the requested formats decide which one, and the edge's own
 * message breaks a tie when both kinds were asked for.
 */
export function scrapeSwitchFor(error: unknown, requested: readonly ScrapeFormat[] = []): string | null {
  for (const field of fieldsOf(error)) {
    if (field.code !== 'feature_disabled') continue
    const path = typeof field.path === 'string' ? field.path : ''
    if (path === 'actions') return 'QUARRY_EDGE__SCRAPE_ACTIONS'
    if (path === 'max_age') return 'QUARRY_EDGE__SCRAPE_MAX_AGE'
    if (!/^formats(\[\d+\])?$/.test(path)) continue
    const wantsAi = requested.some(isAiFormat)
    const wantsCapture = requested.some((format) => format === 'screenshot' || format === 'pdf')
    if (wantsAi && !wantsCapture) return 'QUARRY_EDGE__AI_FORMATS'
    if (wantsCapture && !wantsAi) return 'QUARRY_EDGE__SCRAPE_CAPTURE'
    const message = typeof field.message === 'string' ? field.message : ''
    if (/screenshot|pdf|capture/i.test(message)) return 'QUARRY_EDGE__SCRAPE_CAPTURE'
    if (/json|summary|question|highlight/i.test(message)) return 'QUARRY_EDGE__AI_FORMATS'
    return wantsAi ? 'QUARRY_EDGE__AI_FORMATS' : 'QUARRY_EDGE__SCRAPE_CAPTURE'
  }
  return null
}

/** The crawl switch behind a refusal of `sitemap` or `scrape_options`, or `null`. */
export function crawlSwitchFor(error: unknown): string | null {
  for (const field of fieldsOf(error)) {
    if (field.code !== 'unsupported_option' && field.code !== 'feature_disabled') continue
    const path = typeof field.path === 'string' ? field.path : ''
    if (path === 'sitemap') return 'QUARRY_EDGE__CRAWL_SITEMAP'
    if (path === 'scrape_options' || path.startsWith('scrape_options.')) return 'QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS'
  }
  return null
}

function isUnsupported(error: unknown): boolean {
  return error instanceof ApiError && (error.status === 501 || error.code?.toLowerCase() === 'unsupported')
}

/** Copy for the schema keyword and shape errors the edge gives an AI `json` schema. */
const SCHEMA_CODES: Record<string, { no: string; en: string }> = {
  schema_keyword_not_supported: {
    no: 'Skjemaet bruker et nøkkelord som ikke støttes, for eksempel $ref, anyOf, oneOf, pattern eller format.',
    en: 'The schema uses a keyword that is not supported, for example $ref, anyOf, oneOf, pattern or format.',
  },
  schema_root_not_object: {
    no: 'Rotnivået i skjemaet må være et objekt.',
    en: 'The root of the schema must be an object.',
  },
  schema_too_large: {
    no: 'Skjemaet er for stort. Det kan være høyst 64 KB.',
    en: 'The schema is too large. It may be at most 64 KB.',
  },
  schema_too_complex: {
    no: 'Skjemaet er for komplekst: høyst 10 nivåer og 200 egenskaper.',
    en: 'The schema is too complex: at most 10 levels and 200 properties.',
  },
}

/** The copy for an AI-format refusal, or `null` when the error is not one. */
function aiRefusalMessage(error: unknown, tr: Tr): string | null {
  if (error instanceof ApiError && error.status === 403 && reasonOf(error) === 'extract_ai_scope_required') {
    return tr(
      'Kontoen din mangler tilgang til AI-formater (extract:ai). Be en administrator om å gi tilgangen.',
      'Your account lacks access to AI formats (extract:ai). Ask an administrator to grant it.',
    )
  }
  if (isUnsupported(error)) {
    const reason = reasonOf(error)
    if (reason === 'model_plane_unavailable') {
      return tr(
        'Modellplanen er ikke koblet til Quarry-edge. Sett QUARRY_EDGE__MODEL_PLANE_URL der, og kjør på nytt.',
        'The model plane is not connected to Quarry-edge. Set QUARRY_EDGE__MODEL_PLANE_URL there and run again.',
      )
    }
    if (reason === 'model_lane_not_granted') {
      return tr(
        'Denne installasjonen har ingen modellnøkkel for AI-formater.',
        'This installation has no model credential for AI formats.',
      )
    }
  }
  for (const field of fieldsOf(error)) {
    const code = typeof field.code === 'string' ? field.code : ''
    const copy = SCHEMA_CODES[code]
    if (copy) return tr(copy.no, copy.en)
  }
  return null
}

/**
 * Why `POST /v1/extract` refused a schema or a prompt with 501. Quarry gives these no
 * `details.reason`, so the cause is read from the `error` text the edge sets. The next step
 * is the one Quarry's `next_actions` gives: the same request without the schema and prompt.
 */
function extractRefusalMessage(error: unknown, tr: Tr): string {
  const text = error instanceof ApiError ? error.message : ''
  let reason: string
  if (text.includes('QUARRY_EDGE__AI_FORMATS is off')) {
    reason = tr(
      'Strukturert uttrekk er slått av på Quarry-edge (QUARRY_EDGE__AI_FORMATS).',
      'Structured extraction is switched off in Quarry-edge (QUARRY_EDGE__AI_FORMATS).',
    )
  } else if (text.includes('QUARRY_EDGE__MODEL_PLANE_URL is not set')) {
    reason = tr(
      'Modellplanen er ikke koblet til Quarry-edge (QUARRY_EDGE__MODEL_PLANE_URL mangler).',
      'The model plane is not connected to Quarry-edge (QUARRY_EDGE__MODEL_PLANE_URL is not set).',
    )
  } else if (text.includes('holds no model credential')) {
    reason = tr(
      'Denne installasjonen har ingen modellnøkkel for strukturert uttrekk.',
      'This installation has no model credential for structured extraction.',
    )
  } else if (text.includes('refused this service')) {
    reason = tr(
      'Modellplanen avviste Quarry-edge ved første modellkall. Det ble ikke belastet noe.',
      'The model plane refused Quarry-edge on the first model call. Nothing was metered.',
    )
  } else {
    reason =
      text ||
      tr('Strukturert uttrekk er ikke tilgjengelig på denne installasjonen.', 'Structured extraction is not available on this installation.')
  }
  const next = tr(
    'Fjern JSON-skjemaet og prompten, og kjør på nytt, for å få Markdown.',
    'Remove the JSON schema and the prompt, and run again, to get Markdown.',
  )
  return `${reason} ${next}`
}

export function playgroundErrorMessage(
  error: unknown,
  tr: Tr,
  fallback: { no: string; en: string },
  context: { endpoint?: string; requested?: readonly ScrapeFormat[] } = {},
): string {
  if (context.endpoint === 'extract' && isUnsupported(error)) {
    return extractRefusalMessage(error, tr)
  }
  const ai = aiRefusalMessage(error, tr)
  if (ai) return ai
  const switchName = scrapeSwitchFor(error, context.requested ?? []) ?? crawlSwitchFor(error)
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

/** Copy for an AI format's status code in a scrape answer (INNH-1). Unknown codes say so rather than guess. */
export function aiCodeMessage(code: string | null, tr: Tr, retryAfterSeconds?: number | null): string {
  switch (code) {
    case 'ZDR_UNAVAILABLE':
      return tr('Ikke tilgjengelig under dataskjerming (ZDR). Modellen ble ikke kalt.', 'Not available under zero data retention (ZDR). No model was called.')
    case 'BUDGET_EXHAUSTED':
      return tr('Budsjettet for AI-kall i denne forespørselen er brukt opp.', 'The AI call budget for this request is used up.')
    case 'MODEL_RATE_LIMITED':
      return retryAfterSeconds
        ? tr(`Modellen er travel. Prøv igjen om ${retryAfterSeconds} sekunder.`, `The model is busy. Try again in ${retryAfterSeconds} seconds.`)
        : tr('Modellen er travel. Prøv igjen om litt.', 'The model is busy. Try again shortly.')
    case 'MODEL_UNAVAILABLE':
      return tr('Modellen svarte ikke. Prøv igjen senere.', 'The model did not answer. Try again later.')
    case 'SCHEMA_INVALID':
      return tr('Modellen ga ikke svar som følger skjemaet. Se hvilke felt som bryter det.', 'The model did not return an answer that follows the schema. See which fields break it.')
    case 'EMPTY_SOURCE':
      return tr('Siden har ingen tekst å analysere.', 'The page has no text to analyse.')
    case 'PRIVACY_POLICY':
      return tr('Personverninnstillingene for denne forespørselen gjør at AI-formatet ikke kjøres.', 'The privacy settings for this request mean the AI format is not run.')
    default:
      return code
        ? tr(`Ukjent status fra modellen (${code}).`, `Unknown status from the model (${code}).`)
        : tr('Modellen ga ingen forklaring.', 'The model gave no reason.')
  }
}

/** Copy for a refused read of a run's pages (`/results`): the switch, ZDR, or the generic failure. */
export function jobResultsMessage(error: unknown, tr: Tr, fallback: { no: string; en: string }): string {
  if (isUnsupported(error)) {
    return tr(
      'Sidelisten for kjøringer er ikke slått på på Quarry-edge. Slå på QUARRY_EDGE__JOB_RESULTS der, og kjør på nytt.',
      'The per-run page list is not switched on in Quarry-edge. Turn on QUARRY_EDGE__JOB_RESULTS there and run again.',
    )
  }
  if (error instanceof ApiError && error.status === 403 && reasonOf(error) === 'job_result_urls') {
    return tr(
      'Sidelisten er ikke tilgjengelig under dataskjerming (ZDR), fordi adressene til sidene ikke lagres.',
      'The page list is not available under zero data retention (ZDR), because page addresses are not stored.',
    )
  }
  return translateApiError(error, tr, fallback)
}

/** Copy for a refused change diff (`/v1/change/diff`): the switch, ZDR, or the generic failure. */
export function changeDiffMessage(error: unknown, tr: Tr, fallback: { no: string; en: string }): string {
  if (isUnsupported(error)) {
    return tr(
      'Endringsdiff er ikke slått på på Quarry-edge. Slå på QUARRY_EDGE__CHANGE_TEXT_DIFF der, og kjør på nytt.',
      'Change diffs are not switched on in Quarry-edge. Turn on QUARRY_EDGE__CHANGE_TEXT_DIFF there and run again.',
    )
  }
  if (error instanceof ApiError && error.status === 403) {
    return tr(
      'Endringsdiff er ikke tilgjengelig under dataskjerming (ZDR).',
      'Change diffs are not available under zero data retention (ZDR).',
    )
  }
  return translateApiError(error, tr, fallback)
}

/** Why Quarry could not compare two versions of a page, in product copy. `null` when the diff is available. */
export function diffReasonMessage(reason: string | null | undefined, tr: Tr): string | null {
  switch (reason) {
    case 'no_previous_version':
      return tr('Det finnes bare én lagret versjon, så det er ingenting å sammenligne med ennå.', 'Only one stored version exists, so there is nothing to compare with yet.')
    case 'no_previous_text':
      return tr('Forrige versjon ble lagret uten tekst, så endringen kan ikke vises.', 'The previous version was stored without text, so the change cannot be shown.')
    case 'no_text':
      return tr('Siden har ingen tekst å sammenligne.', 'The page has no text to compare.')
    case 'artifact_gone':
      return tr('Teksten til en av versjonene er utgått og er ikke lenger lagret.', 'The text of one of the versions has expired and is no longer stored.')
    default:
      return null
  }
}

/** Why a search has no answer, in product copy. `null` when the answer is ok or was not asked for. */
export function answerStatusMessage(status: string | null | undefined, reason: string | null | undefined, tr: Tr): string | null {
  switch (status) {
    case 'no_sources':
      return tr('Fant ingen kilder å svare fra.', 'No sources to answer from.')
    case 'skipped_zdr':
      return tr('Svar er av under dataskjerming (ZDR).', 'Answers are off under zero data retention (ZDR).')
    case 'unavailable':
      return answerReasonMessage(reason, tr)
    default:
      return null
  }
}

/** Copy for each `answer_reason` Quarry gives with an `unavailable` answer; a reason this build does not know gets the generic line. */
function answerReasonMessage(reason: string | null | undefined, tr: Tr): string {
  switch (reason) {
    case 'synthesis_disabled':
      return tr(
        'Svar er ikke slått på på Quarry-edge. Slå på QUARRY_EDGE__ANSWER_SYNTHESIS der.',
        'Answers are not switched on in Quarry-edge. Turn on QUARRY_EDGE__ANSWER_SYNTHESIS there.',
      )
    case 'zdr_unavailable':
      return tr('Svar er ikke tilgjengelig under dataskjerming (ZDR).', 'Answers are not available under zero data retention (ZDR).')
    case 'model_path_refused':
      return tr(
        'Quarry-edge nektet modellstien for svar ved oppstart. Sjekk konfigurasjonen der.',
        'Quarry-edge refused the model path for answers at startup. Check its configuration.',
      )
    case 'no_search_provider':
      return tr('Ingen søkeleverandør er koblet til svar på Quarry-edge.', 'No search provider is connected to answers on Quarry-edge.')
    case 'no_model_plane_url':
      return tr(
        'QUARRY_EDGE__MODEL_PLANE_URL er ikke satt, så svar kan ikke lages. Sett den der og kjør på nytt.',
        'QUARRY_EDGE__MODEL_PLANE_URL is not set, so answers cannot be made. Set it there and run again.',
      )
    case 'model_client_init_failed':
      return tr('Modellklienten for svar kunne ikke startes på Quarry-edge.', 'The model client for answers could not start on Quarry-edge.')
    case 'model_plane_auth_refused':
      return tr(
        'Modellplanen avviste nøkkelen for svar. Sjekk tilgangen til svar.',
        'The model plane refused the key for answers. Check the grant for answers.',
      )
    case 'model_plane_rate_limited':
      return tr('Modellen er travel. Prøv igjen om litt.', 'The model is busy. Try again shortly.')
    case 'model_plane_error':
      return tr('Modellen svarte ikke. Prøv igjen senere.', 'The model did not answer. Try again later.')
    case 'answer_search_failed':
      return tr('Søket svaret bygger på feilet. Prøv igjen.', 'The search the answer is built on failed. Try again.')
    default:
      return tr('Svar er ikke tilgjengelig akkurat nå.', 'An answer is not available right now.')
  }
}

/** Copy for each field-level reason a webhook create, patch or retry is refused for (`docs/ERROR_REASONS.md`). */
const WEBHOOK_REASONS: Record<string, { no: string; en: string }> = {
  url_invalid: { no: 'Adressen er ikke en gyldig absolutt URL.', en: 'The address is not a valid absolute URL.' },
  url_too_long: { no: 'Adressen er for lang. Høyst 2048 tegn.', en: 'The address is too long. At most 2048 characters.' },
  url_not_https: { no: 'Adressen må bruke https.', en: 'The address must use https.' },
  url_userinfo: {
    no: 'Adressen kan ikke ha brukernavn og passord. Bruk et endepunkt uten dem; signaturen er autentiseringen.',
    en: 'The address cannot carry a username and password. Use an endpoint without them; the signature is the authentication.',
  },
  url_fragment: { no: 'Fjern #-delen fra adressen.', en: 'Remove the #-part from the address.' },
  url_private_address: {
    no: 'Adressen peker til en privat eller intern adresse. Bruk en offentlig adresse.',
    en: 'The address points to a private or internal address. Use a public one.',
  },
  url_unresolvable: { no: 'Vertsnavnet kunne ikke slås opp. Sjekk DNS-navnet.', en: 'The host name could not be resolved. Check the DNS name.' },
  events_empty: { no: 'Velg hendelsen change_detected.', en: 'Choose the change_detected event.' },
  events_unsupported: { no: 'Hendelsen støttes ikke. Bruk change_detected.', en: 'That event is not supported. Use change_detected.' },
  description_too_long: { no: 'Beskrivelsen er for lang. Høyst 200 tegn.', en: 'The description is too long. At most 200 characters.' },
  body_invalid: { no: 'Forespørselen har felt som ikke støttes.', en: 'The request has fields that are not supported.' },
  empty_patch: { no: 'Velg minst ett felt å endre.', en: 'Choose at least one field to change.' },
}

/** Why a webhook call was refused: the switch, ZDR, a limit or duplicate, a bad field, or the generic failure. */
export function webhookErrorMessage(error: unknown, tr: Tr, fallback: { no: string; en: string }): string {
  if (isUnsupported(error)) {
    return tr(
      'Webhooks er ikke slått på på Quarry. Slå på QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS der (og QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS hvis den er av), og prøv igjen.',
      'Webhooks are not switched on in Quarry. Turn on QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS there (and QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS if it is off), and try again.',
    )
  }
  if (error instanceof ApiError) {
    const reason = reasonOf(error)
    if (error.status === 403 && reason === 'webhook_off_process') {
      return tr(
        'Webhooks er ikke tilgjengelig under dataskjerming (ZDR). Eksisterende kan du fortsatt se, slette og rotere.',
        'Webhooks are not available under zero data retention (ZDR). You can still view, delete and rotate existing ones.',
      )
    }
    if (error.status === 409 && reason === 'subscription_exists') {
      return tr(
        'Det finnes allerede en webhook for denne adressen med denne hendelsen.',
        'A webhook for this address with this event already exists.',
      )
    }
    if (error.status === 409 && reason === 'delivery_not_retryable') {
      return tr(
        'Leveringen kan ikke sendes på nytt nå. Den er allerede levert eller er i gang.',
        'The delivery cannot be sent again now. It has already been delivered or is in progress.',
      )
    }
    if (error.status === 429 && reason === 'subscription_limit') {
      return tr(
        'Du har nådd grensen på 10 webhooks. Slett en før du oppretter en ny.',
        'You have reached the limit of 10 webhooks. Delete one before creating another.',
      )
    }
    const copy = error.status === 400 && reason ? WEBHOOK_REASONS[reason] : undefined
    if (copy) return tr(copy.no, copy.en)
  }
  return translateApiError(error, tr, fallback)
}
