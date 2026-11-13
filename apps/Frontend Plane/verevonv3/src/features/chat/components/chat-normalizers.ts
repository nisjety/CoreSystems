import { ATTACHMENT_MAX_TOTAL_BYTES } from '@/features/chat/lib/attachment-policy'
import { sideChatEnabled } from '@/features/chat/lib/side-chat-switch'
import {
  chatLimitNoticesEnabled,
  isLimitCode,
  type LimitAction,
  type LimitCode,
  limitNotice,
} from '@/features/chat/lib/limit-notices'
import {
  type ChatThreadTranscriptStep,
  type ChatThreadTranscriptTurn,
} from '@/features/chat/lib/chat-thread-history'
import {
  type DashboardComposerSubmitPayload,
} from '@/features/dashboard/home/DashboardComposer'
import {
  type ChatAction,
  type ChatMessage,
  type CitationLocator,
  getAttachmentPolicy,
  normalizeStoredResearchPlan,
  type RecalledMemory,
  type ResearchProvenance,
} from '@/shared/api/chat-client'
import { normalizeContextRefs } from '@/shared/actions/chat-context-refs'
import { OPENAI_CODEX_SUBSCRIPTION_PROVIDER } from '@/shared/api/chatgpt-subscription-client'
import { ApiError } from '@/shared/api/api-error'
import {
  isModelEntitlementCode,
  type ModelEntitlementCode,
  modelEntitlementPresentation,
} from '@/shared/api/model-entitlement-refusals'
import {
  isZdrCode,
  type Translate,
  type ZdrRefusalAction,
  zdrRefusalPresentation,
} from '@/shared/api/zdr-refusals'
import {
  blobToDataUrl,
  parseDataUrl,
} from '@/shared/lib/blob-data'
import {
  readClientValue,
  writeClientValue,
} from '@/shared/session/client-storage'
import { getSession } from '@/shared/session/session-store'
import { supportQuestionFromPrompt } from '@/shared/chat/support-context-envelope'
import { en as enStrings } from '@/shared/i18n/en'
import { INTERNAL_FAILURE_COPY, LocalizedError, MODE_LOCKDOWN_COPY, ORG_POLICY_UNAVAILABLE_COPY } from '@/shared/i18n/errors'
import { localeIntl, type Locale } from '@/shared/i18n/locales'
import { no as noStrings } from '@/shared/i18n/no'
import type { RunDetail } from '@/shared/api/runs-client'
import {
  isChatArtifactVersion,
  mergeArtifactVersion,
} from './chat-artifacts'
import {
  artifactTitle,
  capitalize,
  createPreview,
  formatLatency,
  hostname,
  inferArtifactKind,
  numberValue,
  isValidUrl,
  normalizeTaskStatus,
  objectValue,
  stringValue,
  truncateText,
} from './chat-media-markdown'
import {
  type AgentTaskStep,
  type AgentTaskStepEvidence,
  CHAT_BROWSE_WEB_KEY,
  type ChatArtifact,
  type ChatConnectorSuggestion,
  CONNECTOR_SUGGESTION_PROVIDERS,
  type ChatKnowledgeGrounding,
  type ChatToolCall,
  type ChatTurn,
  type Citation,
  type ComposerAttachment,
  type ComposerToolId,
  type GeneratedFile,
  type GeneratedFilePreview,
  type GeneratedFileCheck,
  type GeneratedFileCheckLevel,
  type QueuedInput,
  type StreamAttachment,
  type StreamFailureAction,
  TOOL_LABELS,
  type TaskStepStatus,
} from './chat-types'

export type { StreamFailureAction } from './chat-types'

/**
 * Whether a turn keeps the customer-gateway connection it was written under (W4-BYO-6). The customer-gateway switch keeps it
 * for the regenerate, edit, continue and retry that replay it; the side chat keeps it for the side question that names its
 * model. Either switch alone is enough, and with both off nothing is kept: the stored transcript and the reopened turns are
 * what they were. Fixed per build, like every `VITE_` switch; only the literal `true` turns each one on.
 */
const connectionKeptEnabled = sideChatEnabled || import.meta.env.VITE_BYO_GATEWAY === 'true'

export function messageToTurn(msg: ChatMessage): ChatTurn {
  const source = msg.role === 'user' ? splitInlinedAttachments(msg.content) : { text: msg.content, attachments: [] }
  return {
    id: msg.id,
    role: msg.role,
    sourceScope: msg.sourceScope,
    retrievalScope: msg.retrievalScope,
    contextRefs: msg.contextRefs,
    researchPlan: msg.researchPlan,
    // The route the stored answer was served on (W5-073-b), when the server kept one; no key for a turn that kept none.
    ...(msg.routingDecision ? { routingDecision: msg.routingDecision } : {}),
    content: visibleTurnContent(msg.role, msg.content),
    // Keep an absent server timestamp EMPTY instead of fabricating "now":
    // session-core messages carry no timestamps, so stamping the fetch time
    // here made every thread selection look like fresh activity (the history
    // list re-ordered and re-dated on a mere click). The cached-metadata merge
    // backfills the real time; loadThread fills any remainder from the stored
    // history entry before the turns reach the UI.
    createdAt: msg.createdAt || '',
    streaming: false,
    model: msg.model,
    ...serverConnectionFields(msg),
    // The message-tree position, on a versioned thread only (WS17 S9); the server sends none otherwise.
    ...(msg.version ? { version: msg.version } : {}),
    // Persisted answer quality, so a thread opened on a device that never saw
    // the stream shows the same score the stream reported. Absent for turns
    // recorded before the score was persisted — which stays distinguishable
    // from "scored zero", because the field is simply missing.
    confidence: msg.confidence,
    verification: msg.verification,
    tools: [],
    attachments: source.attachments.map((file, index) => {
      // The durable message contains extracted text, not the original binary.
      // Give PDF/DOCX/XLSX/PPTX text copies an honest extension for preview/download.
      const name = /\.(pdf|docx|xlsx|pptx)$/i.test(file.name) ? `${file.name}.txt` : file.name
      const url = `data:text/plain;charset=utf-8,${encodeURIComponent(file.content)}`
      return { id: `restored-${msg.id}-${index}`, name, type: 'text/plain', size: new TextEncoder().encode(file.content).length, url, previewUrl: url, extractedText: file.content }
    }),
  }
}

/**
 * The connections a server message was written under (W4-BYO-6, DEC-V-R8), so a reopened thread keeps the connection its
 * regenerate, edit, continue and retry replay on without a local copy of the turn. A customer-gateway connection is kept with
 * `connectionKeptEnabled`. A subscription connection is kept only with the side chat on, as it was before. Session Core stores
 * a subscription id only on a subscription send, which names that provider, so a turn that carries one is on the ChatGPT
 * subscription route.
 */
function serverConnectionFields(msg: ChatMessage): Pick<ChatTurn, 'modelConnectionId' | 'subscriptionConnectionId' | 'provider'> {
  return {
    ...(connectionKeptEnabled && msg.modelConnectionId ? { modelConnectionId: msg.modelConnectionId } : {}),
    ...(sideChatEnabled && msg.subscriptionConnectionId
      ? { subscriptionConnectionId: msg.subscriptionConnectionId, provider: OPENAI_CODEX_SUBSCRIPTION_PROVIDER }
      : {}),
  }
}

/** Restore a status-only tail from Model Plane's durable cancellation record.
 * No cached answer, draft, tool result or authority is reconstructed. Require
 * the exact full goal and an unambiguously newer run, so an older cancelled
 * invocation cannot attach to a repeated or newly submitted question.
 */
export function cancelledRunTurn(threadId: string, latestRunId: string, message: ChatMessage, run: RunDetail): ChatTurn | null {
  if (run.status !== 'cancelled' || !runAnswersTrailingQuestion(threadId, latestRunId, message, run)) return null
  return {
    id: `cancelled-run-${run.runId}`, role: 'assistant', content: '',
    createdAt: run.updatedAt || run.createdAt || '', streaming: false,
    status: 'stopped', stopReason: 'client_cancelled', runId: run.runId,
    tools: [], attachments: [],
  }
}

/** Run states Session Core still owns: the run has not terminalised yet. */
export const ACTIVE_RUN_STATUSES: ReadonlySet<string> = new Set(['queued', 'running', 'awaiting_approval', 'awaiting_input'])

/**
 * What a failed run's recorded `error` means for the reader.
 *
 * Session Core's durable contract stores only a fixed, content-free failure
 * vocabulary on the run (model-gateway `session_flow.rs` maps every direct
 * inference failure onto it), so the code is translated rather than shown.
 */
export function failedRunMessage(code: string | undefined): string {
  switch (code?.trim()) {
    case 'provider_unavailable':
      return 'Modelltjenesten var utilgjengelig, så svaret ble ikke fullført.'
    case 'provider_timeout':
      return 'Modelltjenesten svarte ikke i tide, så svaret ble ikke fullført.'
    case 'execution_failed':
      return 'Svaret ble laget, men kunne ikke lagres.'
    default:
      return 'Kjøringen feilet før svaret ble lagret.'
  }
}

/** How a server `error` frame is shown on its turn. */
export type StreamFailurePresentation = {
  /** What the turn says. */
  message: string
  /**
   * The server saved nothing, so any answer text that already streamed must
   * not stay on the turn as though it were the answer.
   */
  unsaved: boolean
  /**
   * The model service (a provider, or the user's ChatGPT subscription) did
   * not answer. An outage, not a verdict: no check failed, so the Work feed
   * must not name one as failed.
   */
  unavailable: boolean
  /**
   * What the reader can do about it. Omitted means `retry`, which is what
   * every failure offered before the reason was known.
   */
  action?: StreamFailureAction
  /** With `action: 'wait'`: the ISO instant a retry can succeed again. */
  retryAt?: string
  /**
   * The Work-feed title for the service step of an `unavailable` failure,
   * when the route has a more precise one than "the model service".
   */
  serviceTitle?: string
}

/** The content-free timing an `error` frame can carry, and the clock to read it by. */
export type StreamFailureDetails = {
  /** Milliseconds until the refusing service accepts a call again. */
  retryAfterMs?: number
  /** When the ChatGPT plan's usage limit resets (an ISO instant). */
  resetsAt?: string
  /** The reader's language for the reset time. Defaults to the one `tr` picks. */
  locale?: Locale
  /** Now, in epoch milliseconds. Defaults to `Date.now()`. */
  now?: number
  /** The zone to read the reset time in. Defaults to the viewer's own. */
  timeZone?: string
  /** For a zero-retention refusal that names a feature: the feature's key. */
  feature?: string
}

/**
 * The instant `ms` from `now`, or nothing for a delay that says nothing.
 * Relative to when the frame is presented, which is as close as the browser
 * can get to when the server measured it.
 */
function instantAfter(ms: number | undefined, now: number): string | undefined {
  if (ms === undefined || !Number.isFinite(ms) || ms <= 0) return undefined
  return new Date(now + ms).toISOString()
}

/** The calendar day (`YYYY-MM-DD`) an instant falls on in `timeZone`. */
function calendarDay(ms: number, timeZone: string | undefined): string {
  const parts = new Intl.DateTimeFormat('en-US', { timeZone, year: 'numeric', month: '2-digit', day: '2-digit' }).formatToParts(ms)
  const part = (type: Intl.DateTimeFormatPartTypes) => parts.find((candidate) => candidate.type === type)?.value ?? ''
  return `${part('year')}-${part('month')}-${part('day')}`
}

/** The calendar day after `day` (`YYYY-MM-DD`). */
function nextCalendarDay(day: string): string {
  const [year = 0, month = 1, date = 1] = day.split('-').map(Number)
  return new Date(Date.UTC(year, month - 1, date + 1)).toISOString().slice(0, 10)
}

const MINUTE_MS = 60_000

/** Every way a reset instant can be worded, and the calendar day it falls on. */
type ResetWording = { day: string; today: string; tomorrow: string; later: string }

function resetWording(iso: string, locale: Locale, timeZone: string | undefined): ResetWording | undefined {
  const parsed = Date.parse(iso)
  if (!Number.isFinite(parsed)) return undefined
  // Shown to the minute, and rounded UP to the next one. A reset at 01:21:40
  // reads 01:22, never 01:21, when a retry would still be refused. The day is
  // read from the rounded instant too, so 23:59:30 reads "i morgen kl. 00:00".
  const at = Math.ceil(parsed / MINUTE_MS) * MINUTE_MS
  const intl = localeIntl(locale)
  const norwegian = locale === 'no'
  const time = new Intl.DateTimeFormat(intl, {
    timeZone,
    hour: norwegian ? '2-digit' : 'numeric',
    minute: '2-digit',
  }).format(at)
  const date = new Intl.DateTimeFormat(intl, { timeZone, weekday: 'long', day: 'numeric', month: 'long' }).format(at)
  const day = calendarDay(at, timeZone)
  return norwegian
    ? { day, today: `i dag kl. ${time}`, tomorrow: `i morgen kl. ${time}`, later: `${date} kl. ${time}` }
    : { day, today: `today at ${time}`, tomorrow: `tomorrow at ${time}`, later: `${date} at ${time}` }
}

/**
 * A reset instant in the reader's own words and zone: "i dag kl. 14:05",
 * "i morgen kl. 01:21", or "mandag 28. september kl. 09:00" (English: "today
 * at 2:05 PM", "tomorrow at …", "Monday, September 28 at …"). The server's
 * UTC wording is never shown; the instant is re-read in the viewer's zone.
 * Seconds are rounded up to the next minute. An instant that does not parse
 * gives an empty string.
 */
export function formatResetTime(iso: string, locale: Locale, now: number, timeZone?: string): string {
  const wording = resetWording(iso, locale, timeZone)
  if (!wording) return ''
  const today = calendarDay(now, timeZone)
  if (wording.day === today) return wording.today
  if (wording.day === nextCalendarDay(today)) return wording.tomorrow
  return wording.later
}

/**
 * `message` with the reset phrase `formatResetTime` wrote into it re-read
 * against `now`. The phrase is relative to the day it was written, so a
 * notice still open after midnight must not keep saying "i morgen" about
 * what is now today. Only that phrase is replaced. A message without one (a
 * server message, or text in another language) is returned unchanged.
 */
export function refreshResetTimeWording(message: string, iso: string, locale: Locale, now: number, timeZone?: string): string {
  const wording = resetWording(iso, locale, timeZone)
  if (!wording) return message
  const current = formatResetTime(iso, locale, now, timeZone)
  const written = [wording.today, wording.tomorrow, wording.later]
    .find((phrase) => phrase !== current && message.includes(phrase))
  return written ? message.replace(written, () => current) : message
}

/** Title of the Work-feed service step for a subscription failure. */
function subscriptionServiceTitle(tr: (noText: string, enText: string) => string): string {
  return tr('ChatGPT svarte ikke', 'ChatGPT did not answer')
}

/**
 * The connected ChatGPT plan's Codex usage limit. The reset is re-rendered
 * locally from the server's instant, so the reader sees their own time; the
 * retry waits for it, and an instant already behind us says so instead of
 * naming a time in the past.
 */
function usageLimitPresentation(
  tr: (noText: string, enText: string) => string,
  details: StreamFailureDetails,
  now: number,
): StreamFailurePresentation {
  const base = {
    unsaved: true,
    unavailable: true,
    serviceTitle: tr('ChatGPT-planen har nådd bruksgrensen', 'The ChatGPT plan reached its usage limit'),
  }
  const resetMs = details.resetsAt ? Date.parse(details.resetsAt) : Number.NaN
  if (Number.isFinite(resetMs) && resetMs <= now) {
    return {
      ...base,
      action: 'retry',
      message: tr(
        'Den tilkoblede ChatGPT-planen nådde bruksgrensen for Codex, men grensen skal nå være tilbakestilt. Prøv igjen. Ingen annen modell ble brukt.',
        'The connected ChatGPT plan reached its Codex usage limit, but the limit should have reset by now. Try again. No other model was used.',
      ),
    }
  }
  const retryAt = Number.isFinite(resetMs) ? details.resetsAt : instantAfter(details.retryAfterMs, now)
  if (retryAt) {
    const locale = details.locale ?? (tr('no', 'en') === 'en' ? 'en' : 'no')
    const time = formatResetTime(retryAt, locale, now, details.timeZone)
    return {
      ...base,
      action: 'wait',
      retryAt,
      message: tr(
        `Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Den tilbakestilles ${time}. Ingen annen modell ble brukt.`,
        `The connected ChatGPT plan has reached its Codex usage limit. It resets ${time}. No other model was used.`,
      ),
    }
  }
  return {
    ...base,
    action: 'retry',
    message: tr(
      'Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Tidspunktet for tilbakestilling er ukjent; sjekk bruken i ChatGPT og prøv igjen senere. Ingen annen modell ble brukt.',
      'The connected ChatGPT plan has reached its Codex usage limit. The reset time is unknown; check your usage in ChatGPT and try again later. No other model was used.',
    ),
  }
}

/** ChatGPT throttling the subscription for a short while. */
function rateLimitedPresentation(
  tr: (noText: string, enText: string) => string,
  details: StreamFailureDetails,
  now: number,
): StreamFailurePresentation {
  const base = { unsaved: true, unavailable: true, serviceTitle: subscriptionServiceTitle(tr) }
  const delay = details.retryAfterMs
  const retryAt = instantAfter(delay, now)
  if (!retryAt || delay === undefined) {
    return {
      ...base,
      action: 'retry',
      message: tr(
        'ChatGPT begrenser forespørslene akkurat nå. Prøv igjen om litt. Ingen annen modell ble brukt.',
        'ChatGPT is limiting requests right now. Try again shortly. No other model was used.',
      ),
    }
  }
  const seconds = Math.max(1, Math.ceil(delay / 1000))
  return {
    ...base,
    action: 'wait',
    retryAt,
    message: seconds === 1
      ? tr(
          'ChatGPT begrenser forespørslene akkurat nå. Prøv igjen om 1 sekund. Ingen annen modell ble brukt.',
          'ChatGPT is limiting requests right now. Try again in 1 second. No other model was used.',
        )
      : tr(
          `ChatGPT begrenser forespørslene akkurat nå. Prøv igjen om ${seconds} sekunder. Ingen annen modell ble brukt.`,
          `ChatGPT is limiting requests right now. Try again in ${seconds} seconds. No other model was used.`,
        ),
  }
}

/**
 * The control a zero-retention refusal's action gives the reader. Retry is the
 * button; open the connection gives a link and then the button. Choosing another
 * model or turning a feature off changes the turn, so sending it again as it
 * was cannot help: no button, and the words say what to change.
 */
function zdrStreamAction(action: ZdrRefusalAction): StreamFailureAction {
  if (action === 'retry') return 'retry'
  if (action === 'open_integrations') return 'open_integrations'
  return 'none'
}

/**
 * A zero-retention refusal in a turn. Nothing was saved (the refusal is made
 * before a model runs, or the request stored nothing), so what streamed is not
 * the answer, and it is a refusal rather than an outage. The words are the
 * catalogue's, in the reader's language; the server's own text is never shown.
 */
function zdrStreamPresentation(code: string, tr: Translate, feature: string | undefined): StreamFailurePresentation {
  const shown = zdrRefusalPresentation(code, feature, tr)
  return {
    message: shown.next === null ? shown.message : `${shown.message} ${shown.next}`,
    unsaved: true,
    unavailable: false,
    action: zdrStreamAction(shown.action),
  }
}

/**
 * A model the organisation does not allow this reader, or an answer about that
 * which could not be read. Both are refused before a model runs, so nothing was
 * saved and what streamed is not the answer. A refusal, not an outage. The words
 * are the catalogue's, in the reader's language; the server's own text is never
 * shown. Retry is the button for the unavailable answer; for a model that is not
 * available the same turn would be refused again, so the words say what to change.
 */
function modelEntitlementStreamPresentation(
  code: ModelEntitlementCode,
  tr: Translate,
): StreamFailurePresentation {
  const shown = modelEntitlementPresentation(code, tr)
  return {
    message: shown.next === null ? shown.message : `${shown.message} ${shown.next}`,
    unsaved: true,
    unavailable: false,
    action: shown.action === 'retry' ? 'retry' : 'none',
  }
}

/**
 * An organisation that has paused agent runs (F-t3-083) refused the Do turn before any model ran, so nothing was run or
 * saved and what streamed is not the answer. A refusal, not an outage. The same turn would be refused the same way until
 * an administrator resumes agent runs, so no retry is offered and the words say that plain chat still works. The words
 * are the SPA's own, in the reader's language; the server's text is English and is never shown.
 */
function agentsPausedStreamPresentation(tr: Translate): StreamFailurePresentation {
  return {
    message: tr(
      'Organisasjonen har satt agentkjøringer på pause. Vanlig chat fungerer som før. Ingenting ble kjørt.',
      'Your organisation has paused agent runs. Plain chat works as before. Nothing was run.',
    ),
    unsaved: true,
    unavailable: false,
    action: 'none',
  }
}

/**
 * The organisation's agent policy cannot be read inside its bound (C-14-mg-rel1): a Do turn refused before any model
 * ran, so nothing was run or saved. Unlike the pause, this clears when the policy can be read again, and the server
 * says the same request may succeed then, so the reader is offered a retry. It is a refusal, not an outage of the
 * model service, so it is not `unavailable`. The words are the copy of errors.ts, shared with a continuation refused
 * over HTTP, which offers no retry; the server's English text is never shown.
 */
function orgPolicyUnavailableStreamPresentation(tr: Translate): StreamFailurePresentation {
  return {
    message: tr(ORG_POLICY_UNAVAILABLE_COPY.no, ORG_POLICY_UNAVAILABLE_COPY.en),
    unsaved: true,
    unavailable: false,
    action: 'retry',
  }
}

/**
 * The words for a refusal by the server's attachment contract. Nothing was sent
 * to a model, so each says what to change, not to retry.
 */
function attachmentRefusalMessage(code: string, tr: Translate): string {
  switch (code) {
    case 'attachment_media_unsupported':
      return tr(
        'Vedlegget ble ikke sendt: filtypen støttes ikke. Bruk PNG, JPEG, WebP eller GIF for bilder. Ingenting ble sendt.',
        'The attachment was not sent: its type is not supported. Use PNG, JPEG, WebP or GIF for images. Nothing was sent.',
      )
    case 'attachment_too_large':
      return tr(
        'Vedlegget ble ikke sendt: det er for stort. Bruk en mindre fil eller færre vedlegg. Ingenting ble sendt.',
        'The attachment was not sent: it is too large. Use a smaller file or fewer attachments. Nothing was sent.',
      )
    case 'attachment_count_exceeded':
      return tr(
        'Vedleggene ble ikke sendt: det er for mange i én melding. Fjern noen og prøv igjen. Ingenting ble sendt.',
        'The attachments were not sent: there are too many in one message. Remove some and try again. Nothing was sent.',
      )
    case 'attachment_route_unsupported':
      return tr(
        'Vedleggene ble ikke sendt: ChatGPT-abonnementet leser ikke vedlegg. Fjern dem eller velg en annen modell. Ingenting ble sendt.',
        'The attachments were not sent: the ChatGPT subscription does not read attachments. Remove them or pick another model. Nothing was sent.',
      )
    case 'attachment_do_unsupported':
      return tr(
        'Bildet ble ikke sendt: Utfør kan ikke bruke bilder ennå. Send meldingen i Spør-modus. Ingenting ble sendt.',
        'The image was not sent: Do cannot use images yet. Send the message in Ask mode. Nothing was sent.',
      )
    default:
      return tr(
        'Vedlegget ble ikke sendt: filen kunne ikke leses. Legg det ved på nytt. Ingenting ble sendt.',
        'The attachment was not sent: the file could not be read. Attach it again. Nothing was sent.',
      )
  }
}

/**
 * The reader's control for a limit notice. A wait or a retry is a control. `contact_admin` and `open_billing` name a
 * person or a page in the words, and a stream has no billing control to offer, so neither is a button.
 */
function limitStreamAction(action: LimitAction): StreamFailureAction {
  if (action === 'wait' || action === 'retry') return action
  return 'none'
}

/**
 * A limit refusal under VITE_CHAT_LIMIT_NOTICES (F-t3-078), worded by `limitNotice` for the reader's role. The server's
 * own text is never shown: a `budget_exceeded` message repeats the organisation's spend and cap. Nothing was saved, so
 * what streamed is not the answer. Only a rate limit offers a retry; when the server gives a Retry-After, that is a wait.
 */
function limitStreamPresentation(
  code: LimitCode,
  details: StreamFailureDetails,
  now: number,
  tr: (noText: string, enText: string) => string,
): StreamFailurePresentation {
  const delay = details.retryAfterMs
  const retryAfterSec = delay !== undefined && Number.isFinite(delay) && delay > 0
    ? Math.max(1, Math.ceil(delay / 1000))
    : undefined
  const notice = limitNotice(code, getSession().activeOrg?.role, { retryAfterSec }, { tr })
  const action = limitStreamAction(notice.action)
  if (code !== 'rate_limited') {
    return { message: notice.message, unsaved: true, unavailable: false, action }
  }
  // The stream reports `rate_limited` only from the ChatGPT subscription route, which never answers with another
  // model, so the reader is told that as well.
  const retryAt = notice.retryAfterSec === undefined ? undefined : instantAfter(notice.retryAfterSec * 1000, now)
  return {
    message: `${notice.message} ${tr('Ingen annen modell ble brukt.', 'No other model was used.')}`,
    unsaved: true,
    unavailable: true,
    serviceTitle: subscriptionServiceTitle(tr),
    action,
    ...(retryAt !== undefined ? { retryAt } : {}),
  }
}

/**
 * Presentation for a server-reported stream failure, by its code.
 *
 * Only the codes that change what the reader must be told are named; every
 * other code keeps the server's own message and today's rendering.
 *  - `response_validation_unavailable`: the provider or subscription did not
 *    answer while a draft was checked or repaired. The gateway's message is
 *    already Norwegian and names the route, so it is kept.
 *  - `inference_stream_ended_without_terminal`: the provider stream closed
 *    before its final chunk; nothing was saved.
 *  - `session_liveness_failed`: the run was no longer active when the answer
 *    was about to be saved, so it was NOT saved, even though its text may
 *    already have streamed in full.
 *  - `assistant_persist_failed`: the answer streamed in full, but saving it
 *    failed, so it was NOT saved.
 *  - `artifact_revision_conflict`: another run changed a document this answer
 *    edits before the answer was saved, so nothing was saved. The gateway's
 *    message is already Norwegian, so it is kept.
 * `inference_stream_ended_without_terminal`, `session_liveness_failed` and
 * `assistant_persist_failed` arrive with English operator text, so they are
 * translated.
 *
 * The ChatGPT-subscription refusals (`subscription_usage_limit`,
 * `subscription_reauth_required`, `subscription_upstream_unavailable`,
 * `subscription_bad_request`, `rate_limited`, `subscription_route_unavailable`)
 * carry a fixed, content-free reason, so each gets its own words in both
 * languages and an `action`: wait for a known reset, reconnect, retry, or
 * nothing. None of them ever retries by itself, and each says no other model
 * was used, because the subscription route never swaps.
 * `attachments_not_sent` is the client's own refusal to send an image on the
 * subscription route: nothing left the browser, and no retry is offered.
 * `agentic_subscription_disabled` is the server saying Do is off for the
 * subscription route: the reader is told to choose Ask or another model, and
 * no retry is offered. `plan_grant_unavailable` and `plan_execute_disabled` are
 * the refusals of the turn that runs an approved plan (WS14 S2a): the server holds
 * no live grant for the thread, or the capability is switched off on this
 * deployment. Nothing ran, a retry cannot help, so each says so in words and
 * offers none. `org_agents_paused` is the organisation's agent-run pause
 * (F-t3-083): a Do turn refused before any model ran, so nothing was run, the
 * words say plain chat still works, and no retry is offered. `org_policy_unavailable`
 * (C-14-mg-rel1) is a Do turn refused before any model ran while the organisation's agent
 * policy cannot be read: nothing ran, and a retry is offered, since the policy may be readable
 * again. `mode_lockdown` (the agent mode allows no agent run) and `internal` (the service's own failure) are worded
 * from errors.ts: the first ran nothing and offers no retry, the second does not say whether it was saved and offers a retry.
 * `prompt_too_long`, `inference_stream_error` and `model_plane_unavailable` are translated too;
 * the last one's server text is raw transport detail and is never shown.
 * With VITE_CHAT_LIMIT_NOTICES on, `budget_exceeded`, `quota_exceeded`, `run_ceiling_reached` and `rate_limited` are
 * worded for the reader's role by `limitNotice` instead, and the server's text is not shown for them.
 */
export function streamFailurePresentation(
  code: string | undefined,
  serverMessage: string,
  tr: (noText: string, enText: string) => string,
  details: StreamFailureDetails = {},
): StreamFailurePresentation {
  // Every zero-retention code, the ones this SPA does not know included: the
  // server's text for them is English operator wording, never shown.
  if (code !== undefined && isZdrCode(code)) return zdrStreamPresentation(code, tr, details.feature)
  if (isModelEntitlementCode(code)) return modelEntitlementStreamPresentation(code, tr)
  // The organisation's agent-run pause (F-t3-083): a refusal before any model ran, typed on the stream's error frame.
  if (code === 'org_agents_paused') return agentsPausedStreamPresentation(tr)
  // The organisation's agent policy cannot be read inside its bound (C-14-mg-rel1): a Do turn refused before any model ran.
  if (code === 'org_policy_unavailable') return orgPolicyUnavailableStreamPresentation(tr)
  const server = serverMessage.trim()
  const now = details.now ?? Date.now()
  if (isLimitCode(code) && chatLimitNoticesEnabled()) return limitStreamPresentation(code, details, now, tr)
  switch (code) {
    case 'subscription_usage_limit':
      return usageLimitPresentation(tr, details, now)
    case 'rate_limited':
      return rateLimitedPresentation(tr, details, now)
    case 'subscription_reauth_required':
      return {
        message: tr(
          'ChatGPT-tilkoblingen må fornyes. Koble til ChatGPT på nytt under Profil → ChatGPT-abonnement, og prøv igjen. Ingen annen modell ble brukt.',
          'Your ChatGPT connection needs to be renewed. Reconnect ChatGPT under Profile → ChatGPT subscription, then try again. No other model was used.',
        ),
        unsaved: true,
        unavailable: true,
        action: 'reconnect',
        serviceTitle: tr('ChatGPT-tilkoblingen må fornyes', 'The ChatGPT connection needs renewing'),
      }
    case 'subscription_upstream_unavailable':
      return {
        message: tr(
          'ChatGPT svarte ikke akkurat nå. Prøv igjen om litt. Ingen annen modell ble brukt.',
          'ChatGPT did not answer just now. Try again shortly. No other model was used.',
        ),
        unsaved: true,
        unavailable: true,
        action: 'retry',
        serviceTitle: subscriptionServiceTitle(tr),
      }
    case 'subscription_route_unavailable':
      return {
        message: tr(
          'Den valgte ChatGPT-abonnementstilkoblingen kunne ikke fullføre svaret. Prøv igjen om litt. Ingen annen modell ble brukt.',
          'The selected ChatGPT subscription connection could not complete the answer. Try again shortly. No other model was used.',
        ),
        unsaved: true,
        unavailable: true,
        action: 'retry',
        serviceTitle: subscriptionServiceTitle(tr),
      }
    case 'subscription_bad_request':
      // ChatGPT answered, with a refusal of this request: not an outage, and
      // sending the same turn again would be refused the same way.
      return {
        message: tr(
          'ChatGPT avviste forespørselen. Endre eller kort ned meldingen, og prøv igjen. Ingen annen modell ble brukt.',
          'ChatGPT rejected the request. Change or shorten the message, then try again. No other model was used.',
        ),
        unsaved: true,
        unavailable: false,
        action: 'none',
      }
    case 'mode_lockdown':
      // The agent mode that applies here allows no agent run, so a Do turn was refused before any run existed: nothing was
      // run or saved. The same turn would be refused the same way, so no retry is offered. The server's text is English
      // and is never shown.
      return {
        message: tr(MODE_LOCKDOWN_COPY.no, MODE_LOCKDOWN_COPY.en),
        unsaved: true,
        unavailable: false,
        action: 'none',
      }
    case 'internal':
      // The service's own unexpected failure, which a retry can clear, so a retry is offered: the copy says to try again.
      // Whether the answer was saved is not known, so nothing is claimed about it. The server's text is never shown.
      return {
        message: tr(INTERNAL_FAILURE_COPY.no, INTERNAL_FAILURE_COPY.en),
        unsaved: false,
        unavailable: false,
        action: 'retry',
      }
    case 'attachments_not_sent':
      // Raised by the client itself before anything was sent: the ChatGPT
      // subscription route carries no images. Sending the same turn again would
      // be refused the same way, so there is no retry.
      return {
        message: tr(noStrings.attachments.notSent, enStrings.attachments.notSent),
        unsaved: true,
        unavailable: false,
        action: 'none',
      }
    case 'attachment_media_unsupported':
    case 'attachment_too_large':
    case 'attachment_count_exceeded':
    case 'attachment_route_unsupported':
    case 'attachment_encoding_invalid':
    case 'attachment_do_unsupported':
      // The server's attachment contract (WS08 S2, S4) refused the turn before any
      // model ran. The same turn would be refused the same way, so there is no
      // retry; each code says what to change. The server's text names a file and
      // a limit in English and is never shown.
      return {
        message: attachmentRefusalMessage(code, tr),
        unsaved: true,
        unavailable: false,
        action: 'none',
      }
    case 'agentic_subscription_disabled':
      // The server refused Do on the ChatGPT subscription because its switch is
      // off, before any model ran. Sending the same turn again would be refused
      // the same way, so there is no retry; the words say what to do instead.
      // The server's own text is English operator wording and is never shown.
      return {
        message: tr(
          'Utfør er ikke slått på for ChatGPT-abonnementet. Velg Spør, eller velg en annen modell for å bruke Utfør. Ingen modell ble brukt.',
          'Do is not switched on for the ChatGPT subscription. Choose Ask, or pick another model to use Do. No model was used.',
        ),
        unsaved: true,
        unavailable: false,
        action: 'none',
      }
    case 'plan_grant_unavailable':
      // The turn that runs the approved plan was refused before anything ran:
      // the gateway holds no live grant for the thread (it restarted, or the
      // 24 h grant lapsed). Sending it again would be refused the same way, so
      // there is no retry; the words say what does help. The server's text is
      // English operator wording and is never shown.
      return {
        message: tr(
          'Fullmakten er ikke lenger aktiv (utløpt eller tjenesten ble startet på nytt). Velg Utfør, be agenten lage planen på nytt, og godkjenn den. Ingenting ble kjørt.',
          'The authority is no longer active (it expired or the service restarted). Choose Do, ask the agent to make the plan again, and approve it. Nothing was run.',
        ),
        unsaved: true,
        unavailable: false,
        action: 'none',
      }
    case 'plan_execute_disabled':
      // Running an approved plan is switched off on this deployment. Nothing
      // the reader does here changes that, so there is no retry either.
      return {
        message: tr(
          'Kjøring av godkjent plan er slått av i denne installasjonen. Ingenting ble kjørt.',
          'Running an approved plan is switched off on this deployment. Nothing was run.',
        ),
        unsaved: true,
        unavailable: false,
        action: 'none',
      }
    case 'prompt_too_long':
      return {
        message: tr(
          'Samtalen er for lang for modellen, selv etter komprimering. Start en ny tråd.',
          'This conversation is too long for the model, even after compacting it. Start a new thread.',
        ),
        unsaved: true,
        unavailable: false,
        action: 'none',
      }
    case 'inference_stream_error':
      return {
        message: tr(
          'Modellen svarte ikke denne gangen. Prøv igjen.',
          'The model did not return an answer this time. Try again.',
        ),
        unsaved: true,
        unavailable: false,
      }
    case 'model_plane_unavailable':
      // The server's text here is raw transport detail, never shown.
      return {
        message: tr(
          'Modelltjenesten svarte ikke. Prøv igjen om litt.',
          'The model service did not answer. Try again shortly.',
        ),
        unsaved: true,
        unavailable: true,
      }
    case 'response_validation_unavailable':
      return {
        message: server || tr(
          'Modelltjenesten svarte ikke mens utkastet ble kontrollert. Utkastet er ikke publisert. Prøv igjen om litt.',
          'The model service did not answer while the draft was being checked. The draft was not published. Try again shortly.',
        ),
        unsaved: true,
        unavailable: true,
      }
    case 'inference_stream_ended_without_terminal':
      return {
        message: tr(
          'Svaret ble avbrutt før det var ferdig, og ingenting ble lagret. Prøv igjen.',
          'The answer stopped before it was complete, and nothing was saved. Try again.',
        ),
        unsaved: true,
        unavailable: false,
      }
    case 'session_liveness_failed':
      return {
        message: tr(
          'Svaret ble ikke lagret, fordi kjøringen ikke lenger var aktiv. Prøv igjen.',
          'The answer was not saved, because the run was no longer active. Try again.',
        ),
        unsaved: true,
        unavailable: false,
      }
    case 'assistant_persist_failed':
      return {
        message: tr(
          'Svaret ble laget, men kunne ikke lagres. Prøv igjen.',
          'The answer was produced but could not be saved. Try again.',
        ),
        unsaved: true,
        unavailable: false,
      }
    case 'artifact_revision_conflict':
      return {
        message: server || tr(
          'Dokumentet ble oppdatert av en annen kjøring. Last inn den nyeste versjonen og prøv igjen.',
          'The document was updated by another run. Load the latest version and try again.',
        ),
        unsaved: true,
        unavailable: false,
      }
    default:
      return { message: server || 'Stream error', unsaved: false, unavailable: false }
  }
}

/**
 * The reload tail for a durable run the thread has no assistant message for.
 *
 * `cancelledRunTurn` covers the cancelled case; this adds the other two a
 * reload can find behind a trailing question. A FAILED run becomes an error
 * turn carrying only the run's own recorded error (or a fixed notice), so the
 * failure is shown instead of a question that looks unanswered (the run's code
 * is translated by `failedRunMessage`, never shown raw). A run that is
 * still ACTIVE becomes a pending turn with its run id and no request id: the
 * caller keeps it pending, bounded, and adopts the saved answer when it lands.
 * Same correlation rule as the cancelled case — exact goal, newer run — and,
 * as there, no cached answer, draft, tool result or authority is rebuilt.
 */
export function unsettledRunTurn(threadId: string, latestRunId: string, message: ChatMessage, run: RunDetail): ChatTurn | null {
  const cancelled = cancelledRunTurn(threadId, latestRunId, message, run)
  if (cancelled) return cancelled
  if (!runAnswersTrailingQuestion(threadId, latestRunId, message, run)) return null
  const base = {
    role: 'assistant' as const,
    createdAt: run.updatedAt || run.createdAt || '',
    runId: run.runId,
    tools: [],
    attachments: [],
  }
  if (run.status === 'failed') {
    return {
      ...base,
      id: `failed-run-${run.runId}`,
      content: failedRunMessage(run.error),
      streaming: false,
      status: 'error',
    }
  }
  if (ACTIVE_RUN_STATUSES.has(run.status)) {
    return { ...base, id: `active-run-${run.runId}`, content: '', streaming: true, status: 'waiting' }
  }
  return null
}

/**
 * The server's saved answer for the browser's trailing assistant turn, if the
 * durable transcript holds one.
 *
 * Used when the browser could not settle a turn itself — a resume that failed,
 * or a Stop the server may not have honoured. Server messages carry no request
 * id, so the answer is matched by position, and deliberately conservatively:
 * the turn must be the last local turn, both sides must hold the same number
 * of questions, and the server's last question must be the one this turn
 * answered. Any doubt returns `null`, which leaves the turn as the browser
 * settled it; adopting the wrong answer would be worse than not adopting one.
 *
 * `partial` is for a window of the newest messages (WS09 S9). Its question count
 * cannot match the whole local thread, so only the trailing question is compared.
 */
export function savedAnswerForTrailingTurn(
  localTurns: ChatTurn[],
  turnId: string,
  serverTurns: ChatTurn[],
  options: { partial?: boolean } = {},
): ChatTurn | null {
  const index = localTurns.findIndex((turn) => turn.id === turnId)
  if (index < 0 || index !== localTurns.length - 1 || localTurns[index]?.role !== 'assistant') return null
  const localQuestions = localTurns.slice(0, index).filter((turn) => turn.role === 'user')
  const serverQuestionIndexes = serverTurns.flatMap((turn, at) => (turn.role === 'user' ? [at] : []))
  if (localQuestions.length === 0) return null
  if (options.partial ? serverQuestionIndexes.length === 0 : serverQuestionIndexes.length !== localQuestions.length) return null
  const lastQuestionAt = serverQuestionIndexes.at(-1) as number
  const normalize = (value: string) => value.trim().replace(/\s+/g, ' ')
  const asked = normalize(localQuestions.at(-1)?.content ?? '')
  const recorded = normalize(serverTurns[lastQuestionAt]?.content ?? '')
  if (!asked || !(recorded === asked || recorded.startsWith(asked))) return null
  const answer = serverTurns.slice(lastQuestionAt + 1).filter((turn) => turn.role === 'assistant').at(-1)
  if (!answer || (!answer.content.trim() && (answer.artifacts?.length ?? 0) === 0)) return null
  return answer
}

function runAnswersTrailingQuestion(threadId: string, latestRunId: string, message: ChatMessage, run: RunDetail): boolean {
  if (message.role !== 'user' || run.threadId !== threadId || run.runId !== latestRunId || run.parentRunId) return false
  const ulid = /^[0-7][0-9A-HJKMNP-TV-Z]{25}$/
  if (!ulid.test(message.id) || !ulid.test(run.runId) || run.runId.slice(0, 10) <= message.id.slice(0, 10)) return false
  const normalize = (value: string) => value.trim().replace(/\s+/g, ' ')
  const goal = normalize(run.goal).replace(/ \[full-goal-blake3:[a-f0-9]{64}\]$/, '')
  return Boolean(goal) && goal === normalize(message.content)
}

export function turnsToTranscript(turns: ChatTurn[]): ChatThreadTranscriptTurn[] {
  return turns
    .filter((turn) => turn.role === 'user' || turn.role === 'assistant')
    .map((turn) => ({
      id: turn.id,
      role: turn.role,
      content: turn.content,
      createdAt: turn.createdAt,
      model: turn.model,
      modelUsed: turn.modelUsed,
      provider: turn.provider,
      subscriptionConnectionId: turn.subscriptionConnectionId,
      // Written only while a connection is kept (connectionKeptEnabled): with both switches off the stored transcript is unchanged.
      ...(connectionKeptEnabled ? { modelConnectionId: turn.modelConnectionId } : {}),
      requestId: turn.requestId,
      runId: turn.runId,
      planMode: turn.planMode,
      grantedRung: turn.grantedRung,
      status: turn.status,
      inputTokens: turn.inputTokens,
      outputTokens: turn.outputTokens,
      latencyMs: turn.latencyMs,
      costUsd: turn.costUsd,
      costBasis: turn.costBasis,
      // The three totals only: the span list is a live diagnostic, and the
      // Details rows ("Modelltid", "Systemtid") read nothing else.
      timing: turn.timing
        ? {
            serverMs: turn.timing.serverMs,
            providerMs: turn.timing.providerMs,
            orchestrationMs: turn.timing.orchestrationMs,
          }
        : undefined,
      effectClass: turn.effectClass,
      confidence: turn.confidence,
      verification: turn.verification,
      reasoning: turn.reasoning,
      citations: turn.citations,
      toolCalls: turn.toolCalls,
      artifacts: turn.artifacts,
      files: turn.files,
      grounding: turn.grounding,
      memoryRecallCount: turn.memoryRecallCount,
      recalledMemories: turn.recalledMemories?.map(recalledMemoryWithoutText),
      stopReason: turn.stopReason,
      tools: turn.tools,
      // Kept so a reload shows the suggestion as a note: a provider key and the gateway's scrubbed line.
      connectorSuggestion: turn.connectorSuggestion,
      // `unsavedContent` is deliberately absent: it is text the server did
      // not save, and a reload must not bring it back.
      // `previewUrl` is a live data URL used by the contextual canvas. It can
      // be large and may contain private file bytes, so it never enters the
      // durable transcript/cache. The attachment metadata remains available
      // for the message chip after a reload.
      attachments: turn.attachments.map(({ previewUrl: _previewUrl, ...attachment }) => attachment),
    }))
}

export function taskStepsToTranscript(steps: AgentTaskStep[]): ChatThreadTranscriptStep[] {
  return steps.map((step) => ({
    id: step.id,
    title: step.title,
    detail: step.detail,
    status: step.status,
    createdAt: step.createdAt,
    expandedDetail: step.expandedDetail,
    evidence: step.evidence,
    turnId: step.turnId,
    turnTitle: step.turnTitle,
  }))
}

export function transcriptTurnToChatTurn(turn: ChatThreadTranscriptTurn): ChatTurn {
  const attachments = (turn.attachments ?? []).filter(isComposerAttachment)
  return {
    id: turn.id,
    role: turn.role,
    sourceScope: turn.sourceScope === 'conversation' ? 'conversation' : 'workspace',
    retrievalScope: turn.retrievalScope === 'knowledge_only' || turn.retrievalScope === 'web_only' ? turn.retrievalScope : undefined,
    ...restoredContextRefsField(turn.contextRefs),
    ...restoredResearchPlanField(turn.researchPlan),
    content: visibleTurnContent(turn.role, turn.content),
    createdAt: turn.createdAt,
    streaming: false,
    model: turn.model,
    modelUsed: turn.modelUsed,
    provider: turn.provider,
    subscriptionConnectionId: turn.subscriptionConnectionId,
    // Restored only while a connection is kept (connectionKeptEnabled), so a reopened thread's regenerate, edit and continue
    // requests stay on the connection the turn was written under. With both switches off the turn is what it was.
    ...(connectionKeptEnabled ? { modelConnectionId: turn.modelConnectionId } : {}),
    requestId: turn.requestId,
    runId: turn.runId,
    planMode: turn.planMode,
    grantedRung: turn.grantedRung,
    status: turn.status,
    inputTokens: turn.inputTokens,
    outputTokens: turn.outputTokens,
    latencyMs: turn.latencyMs,
    costUsd: turn.costUsd,
    costBasis: turn.costBasis === 'metered' || turn.costBasis === 'subscription' ? turn.costBasis : undefined,
    timing: transcriptTimingToTurnTiming(turn.timing),
    effectClass: turn.effectClass,
    confidence: turn.confidence,
      verification: turn.verification,
    reasoning: turn.reasoning,
    citations: (turn.citations ?? []).filter(isCitation),
    toolCalls: (turn.toolCalls ?? []).filter(isChatToolCall),
    artifacts: (turn.artifacts ?? []).filter(isChatArtifact).map(deckArtifactForDisplay),
    files: (turn.files ?? []).filter(isGeneratedFile),
    grounding: isChatKnowledgeGrounding(turn.grounding) ? turn.grounding : undefined,
    memoryRecallCount: turn.memoryRecallCount,
    // Also stripped on the way back in, for a snapshot written before
    // `turnsToTranscript` stopped storing the text.
    recalledMemories: (turn.recalledMemories ?? []).filter(isRecalledMemory).map(recalledMemoryWithoutText),
    stopReason: turn.stopReason,
    tools: (turn.tools ?? []).filter(isComposerToolId),
    connectorSuggestion: restoredConnectorSuggestion(turn.connectorSuggestion),
    attachments: attachments.length ? attachments : messageToTurn({ id: turn.id, role: turn.role, content: turn.content, createdAt: turn.createdAt }).attachments,
  }
}

/** The `contextRefs` key for a transcript turn that carries chips, and no key at all for one that does not. */
function restoredContextRefsField(value: unknown): Pick<ChatTurn, 'contextRefs'> {
  const refs = normalizeContextRefs(value)
  return refs.length > 0 ? { contextRefs: refs } : {}
}

/**
 * The `researchPlan` key for a transcript turn that carries a persisted plan (EP-2 S5), and no key for
 * one that does not. The BFF relays sub-queries and questions only, so the card gets the default cap.
 */
function restoredResearchPlanField(value: unknown): Pick<ChatTurn, 'researchPlan'> {
  const plan = normalizeStoredResearchPlan(value)
  return plan ? { researchPlan: plan } : {}
}

/**
 * A stored timing summary back to the turn's shape. All three totals or none:
 * a row missing one would show a number the server never measured.
 */
function transcriptTimingToTurnTiming(value: ChatThreadTranscriptTurn['timing']): ChatTurn['timing'] {
  if (!value) return undefined
  const { serverMs, providerMs, orchestrationMs } = value
  const valid = [serverMs, providerMs, orchestrationMs].every((ms) => typeof ms === 'number' && Number.isFinite(ms) && ms >= 0)
  return valid ? { serverMs, providerMs, orchestrationMs } : undefined
}

/** Longest reason a suggestion card shows: the gateway refuses a longer one (mp-contracts `connector_suggest`). */
export const CONNECTOR_SUGGESTION_MAX_REASON_CHARS = 200

/**
 * A `connector_suggestion` frame as the card reads it (EP-10 S8), or `null` for a frame it cannot trust: no
 * id, a provider outside the closed list, or no reason. The frame's keys are read in either spelling, the
 * wire's or the stream client's. The reason is shown as one line of text and cut to the gateway's bound, so
 * a frame from a drifted build still renders as the card the gateway means.
 */
export function normalizeConnectorSuggestion(event: unknown): ChatConnectorSuggestion | null {
  const record = objectValue(event)
  if (!record) return null
  const suggestionId = stringValue(record.suggestionId ?? record.suggestion_id)?.trim()
  const provider = CONNECTOR_SUGGESTION_PROVIDERS.find((known) => known === record.provider)
  const reason = stringValue(record.reason)
    ?.replace(/\s+/g, ' ')
    .trim()
    .slice(0, CONNECTOR_SUGGESTION_MAX_REASON_CHARS)
  if (!suggestionId || !provider || !reason) return null
  const requestId = stringValue(record.requestId ?? record.request_id)
  return { suggestionId, provider, reason, ...(requestId ? { requestId } : {}) }
}

/** A stored suggestion read back after a reload: checked again, and marked so the card offers nothing. */
export function restoredConnectorSuggestion(value: unknown): ChatConnectorSuggestion | undefined {
  const suggestion = normalizeConnectorSuggestion(value)
  return suggestion ? { ...suggestion, restored: true } : undefined
}

function visibleTurnContent(role: ChatTurn['role'], content: string): string {
  if (role !== 'user') return content
  const visible = splitInlinedAttachments(content).text
  return supportQuestionFromPrompt(visible) ?? visible
}

/** Decode only complete attachment suffixes emitted by appendInlinedAttachments. */
export function splitInlinedAttachments(content: string): { text: string; attachments: InlinedAttachment[] } {
  const start = content.indexOf('\n\n--- VEDLEGG: ')
  const unchanged = { text: content, attachments: [] as InlinedAttachment[] }
  if (start < 0) return unchanged
  let rest = content.slice(start).trimStart()
  const attachments: InlinedAttachment[] = []
  while (rest) {
    const opening = /^--- VEDLEGG: ([^\r\n]+) ---\r?\n/.exec(rest)
    if (!opening) return unchanged
    const name = opening[1]!
    const closing = `\n--- SLUTT PÅ VEDLEGG: ${name} ---`
    const end = rest.indexOf(closing, opening[0].length)
    if (end < 0) return unchanged
    attachments.push({ name, content: rest.slice(opening[0].length, end) })
    rest = rest.slice(end + closing.length).trim()
  }
  return { text: content.slice(0, start).trimEnd(), attachments }
}

export function transcriptStepToTaskStep(step: ChatThreadTranscriptStep): AgentTaskStep {
  return {
    id: step.id,
    title: step.title,
    detail: step.detail,
    status: step.status,
    createdAt: step.createdAt,
    expandedDetail: step.expandedDetail,
    evidence: step.evidence,
    turnId: step.turnId,
    turnTitle: step.turnTitle,
  }
}

export function dedupeChatTurns(turns: ChatTurn[]): ChatTurn[] {
  return turns.reduce<ChatTurn[]>((next, turn) => {
    const previous = next.at(-1)
    if (
      previous &&
      previous.role === turn.role &&
      previous.content === turn.content &&
      // Two timestamp-less turns (session-core sends none) are the case the
      // dedupe existed for back when both were stamped "now" — keep treating
      // them as close.
      (timestampsAreClose(previous.createdAt, turn.createdAt) ||
        (!previous.createdAt && !turn.createdAt))
    ) {
      return next
    }
    return [...next, turn]
  }, [])
}

export function timestampsAreClose(left: string, right: string): boolean {
  const leftTime = Date.parse(left)
  const rightTime = Date.parse(right)
  if (Number.isNaN(leftTime) || Number.isNaN(rightTime)) return false
  return Math.abs(leftTime - rightTime) < 60_000
}

/**
 * `partial` marks `serverTurns` as a window of the thread (WS09 S9). A position
 * in a window says nothing about the cached turn at the same place in the whole
 * thread, so a turn in a window is matched by its id or its words only.
 */
export function mergeServerTurnsWithCachedMetadata(
  serverTurns: ChatTurn[],
  cachedTurns: ChatTurn[],
  options: { partial?: boolean } = {},
): ChatTurn[] {
  if (cachedTurns.length === 0) return serverTurns
  const usedCachedIds = new Set<string>()
  const roleOffsets: Record<ChatTurn['role'], number> = { assistant: 0, user: 0 }

  const merged = serverTurns.map((serverTurn) => {
    const roleIndex = roleOffsets[serverTurn.role]
    roleOffsets[serverTurn.role] += 1
    const cachedTurn = options.partial
      ? selectCachedTurnByIdOrWords(serverTurn, cachedTurns, usedCachedIds)
      : selectCachedTurnForServerTurn(serverTurn, cachedTurns, usedCachedIds, roleIndex)
    if (!cachedTurn) return serverTurn
    usedCachedIds.add(cachedTurn.id)
    return {
      ...serverTurn,
      // The cached transcript recorded the REAL send time during streaming;
      // a server turn without a timestamp takes it instead of staying empty.
      createdAt: serverTurn.createdAt || cachedTurn.createdAt,
      model: serverTurn.model ?? cachedTurn.model,
      modelUsed: serverTurn.modelUsed ?? cachedTurn.modelUsed,
      provider: serverTurn.provider ?? cachedTurn.provider,
      subscriptionConnectionId: serverTurn.subscriptionConnectionId ?? cachedTurn.subscriptionConnectionId,
      ...(connectionKeptEnabled ? { modelConnectionId: serverTurn.modelConnectionId ?? cachedTurn.modelConnectionId } : {}),
      requestId: serverTurn.requestId ?? cachedTurn.requestId,
      runId: serverTurn.runId ?? cachedTurn.runId,
      planMode: serverTurn.planMode ?? cachedTurn.planMode,
      grantedRung: serverTurn.grantedRung ?? cachedTurn.grantedRung,
      effectClass: serverTurn.effectClass ?? cachedTurn.effectClass,
      status: serverTurn.status ?? mergedCachedStatus(serverTurn, cachedTurn),
      // This client's own terminal-frame stamp (EP-14 S1). The server's copy never has one, so the cached turn's
      // is kept for the session; the stored transcript never carries it. Keyed only when set, as the other shapes are.
      ...(cachedTurn.settledAt ? { settledAt: cachedTurn.settledAt } : {}),
      inputTokens: serverTurn.inputTokens ?? cachedTurn.inputTokens,
      outputTokens: serverTurn.outputTokens ?? cachedTurn.outputTokens,
      latencyMs: serverTurn.latencyMs ?? cachedTurn.latencyMs,
      costUsd: serverTurn.costUsd ?? cachedTurn.costUsd,
      // Stream-only, like the usage fields above: the server's message
      // carries neither, so the cached copy is what survives a reload.
      costBasis: serverTurn.costBasis ?? cachedTurn.costBasis,
      timing: serverTurn.timing ?? cachedTurn.timing,
      verification: serverTurn.verification ?? cachedTurn.verification,
      confidence: serverTurn.confidence ?? cachedTurn.confidence,
      reasoning: serverTurn.reasoning ?? cachedTurn.reasoning,
      citations: metadataArray(serverTurn.citations, cachedTurn.citations),
      toolCalls: metadataArray(serverTurn.toolCalls, cachedTurn.toolCalls),
      artifacts: metadataArray(serverTurn.artifacts, cachedTurn.artifacts),
      files: metadataArray(serverTurn.files, cachedTurn.files),
      grounding: serverTurn.grounding ?? cachedTurn.grounding,
      // Both are session-derived: the server's persisted message carries
      // neither, so without falling back to the cache a thread refresh would
      // drop the recall chip and — worse — the truncation warning, turning a
      // "may be cut off" answer into one that looks complete.
      memoryRecallCount: serverTurn.memoryRecallCount ?? cachedTurn.memoryRecallCount,
      recalledMemories: metadataArray(serverTurn.recalledMemories, cachedTurn.recalledMemories),
      stopReason: serverTurn.stopReason ?? cachedTurn.stopReason,
      // The chips and the plan come from the BFF transcript (the cached side here), which flattens them off
      // the message metadata; the messages read may carry the chips too, and wins when it does.
      contextRefs: serverTurn.contextRefs ?? cachedTurn.contextRefs,
      researchPlan: serverTurn.researchPlan ?? cachedTurn.researchPlan,
      // The route is stated by the usage frame while the turn streams and by the stored metadata after a reload; either keeps it.
      routingDecision: serverTurn.routingDecision ?? cachedTurn.routingDecision,
      // Stream-only too: the server's message never carries the suggestion card.
      connectorSuggestion: serverTurn.connectorSuggestion ?? restoredConnectorSuggestion(cachedTurn.connectorSuggestion),
      tools: serverTurn.tools.length > 0 ? serverTurn.tools : cachedTurn.tools,
      attachments: cachedTurn.attachments.length > 0 && serverTurn.attachments.every(a => a.id.startsWith('restored-'))
        ? cachedTurn.attachments : serverTurn.attachments.length > 0 ? serverTurn.attachments : cachedTurn.attachments,
    }
  })

  // Resumable tail (chat-parity §3b): a reload mid-answer leaves the waiting
  // assistant turn ONLY in the cache — the server persists the assistant
  // message at stream end, so mapping over serverTurns alone silently drops
  // it, and the resume path (`maybeResumeStream`) never sees a waiting turn to
  // reattach to. Carry that one turn over: strictly the LAST cached turn, an
  // assistant still 'waiting', unconsumed by the merge above, and holding the
  // requestId the resume endpoint needs (no requestId → nothing to resume →
  // appending would render an unsettleable spinner). Its user question is
  // already in serverTurns: the gateway persists the user message during
  // prepare, before any requestId exists.
  const tail = cachedTurns.at(-1)
  if (
    tail &&
    tail.role === 'assistant' &&
    tail.status === 'waiting' &&
    tail.requestId &&
    !usedCachedIds.has(tail.id)
  ) {
    return [...merged, tail]
  }
  return merged
}

/**
 * The cached status a server-matched turn keeps.
 *
 * A server-persisted assistant message is by definition COMPLETE (session-core
 * stores it once, at stream end), so a cached in-flight 'waiting' never leaks
 * onto it: it would render a finished answer as an eternal spinner. Nor does a
 * cached 'stopped': the gateway saves nothing on a cancel it honoured, so a
 * saved answer means the Stop lost the race with publication, and "Stoppet"
 * over it would be false (a reload before the Stop reconciliation finished
 * used to keep exactly that). A cached 'error' still carries: a failed check
 * saves its fixed explanation as the assistant message, and that is still a
 * failure.
 */
function mergedCachedStatus(serverTurn: ChatTurn, cachedTurn: ChatTurn): ChatTurn['status'] {
  if (cachedTurn.status === 'waiting') return undefined
  if (cachedTurn.status === 'stopped' && serverTurn.role === 'assistant') return undefined
  return cachedTurn.status
}

export function metadataArray<T>(serverItems: T[] | undefined, cachedItems: T[] | undefined): T[] | undefined {
  return serverItems && serverItems.length > 0 ? serverItems : cachedItems
}

export function selectCachedTurnForServerTurn(
  serverTurn: ChatTurn,
  cachedTurns: ChatTurn[],
  usedCachedIds: Set<string>,
  roleIndex: number,
): ChatTurn | undefined {
  const unusedSameRole = cachedTurns.filter((turn) => turn.role === serverTurn.role && !usedCachedIds.has(turn.id))
  return unusedSameRole.find((turn) => turn.id === serverTurn.id)
    ?? unusedSameRole.find((turn) => turnsProbablyMatch(serverTurn, turn))
    ?? unusedSameRole[roleIndex]
    ?? (serverTurn.role === 'assistant' ? [...unusedSameRole].reverse().find(hasCachedTurnMetadata) : undefined)
}

function selectCachedTurnByIdOrWords(
  serverTurn: ChatTurn,
  cachedTurns: ChatTurn[],
  usedCachedIds: Set<string>,
): ChatTurn | undefined {
  const unusedSameRole = cachedTurns.filter((turn) => turn.role === serverTurn.role && !usedCachedIds.has(turn.id))
  return unusedSameRole.find((turn) => turn.id === serverTurn.id)
    ?? unusedSameRole.find((turn) => turnsProbablyMatch(serverTurn, turn))
}

/** Whether two turns are one message: the same id, or the same role and words. */
export function isSameTurn(left: ChatTurn, right: ChatTurn): boolean {
  return left.role === right.role && (left.id === right.id || turnsProbablyMatch(left, right))
}

/**
 * The cached turns that lie before a window of the thread (WS09 S9). A window
 * read leaves them out, but the local snapshot still holds them, so they stay
 * until "load earlier" reaches the same messages. A snapshot that does not reach
 * the window at all is older than it: keeping it would put a gap in the middle
 * of the transcript and pass that gap off as continuous, so nothing is kept.
 */
export function cachedTurnsBeforeWindow(serverTurns: ChatTurn[], cachedTurns: ChatTurn[]): ChatTurn[] {
  const firstShared = cachedTurns.findIndex((cached) => serverTurns.some((server) => isSameTurn(server, cached)))
  return firstShared > 0 ? cachedTurns.slice(0, firstShared) : []
}

export function turnsProbablyMatch(left: ChatTurn, right: ChatTurn): boolean {
  const leftContent = normalizeTurnContentForMatch(left.content)
  const rightContent = normalizeTurnContentForMatch(right.content)
  if (!leftContent || !rightContent) return false
  if (leftContent === rightContent) return true
  const leftSnippet = leftContent.slice(0, 120)
  const rightSnippet = rightContent.slice(0, 120)
  return leftSnippet.length > 40 && rightContent.includes(leftSnippet)
    ? true
    : rightSnippet.length > 40 && leftContent.includes(rightSnippet)
}

export function normalizeTurnContentForMatch(content: string): string {
  return content.replace(/\s+/g, ' ').trim()
}

export function hasCachedTurnMetadata(turn: ChatTurn): boolean {
  return (
    (turn.citations?.length ?? 0) > 0 ||
    (turn.toolCalls?.length ?? 0) > 0 ||
    (turn.artifacts?.length ?? 0) > 0 ||
    (turn.files?.length ?? 0) > 0 ||
    Boolean(turn.grounding) ||
    Boolean(turn.reasoning) ||
    Boolean(turn.effectClass)
  )
}

export function isComposerToolId(value: string): value is ComposerToolId {
  return value === 'image' || value === 'reason' || value === 'research' || value === 'search'
}

/**
 * Does this task step represent work, as opposed to narrating the turn?
 *
 * VEREVON_CHAT_DESIGN.md section 3.1: the right panel is opened by the work,
 * never by the product. Every lifecycle event (connect, compose, model
 * selected, grounding summary, reasoning, usage, memory recall, verification)
 * also becomes a task step, so "any step exists" was true for a bare answer
 * and the Work panel opened on plain Ask turns -- the live audit of
 * 2026-09-02 caught exactly that. Work is: an orchestration step
 * (`:event-`, plan steps and deep-research sub-queries), a provider action
 * (`:action-`), or a tool the model actually called (`:tool-<call id>`). The
 * composer's requested-capability placeholders share the `:tool-` prefix but
 * carry a composer tool id, so they stay bookkeeping. Unknown shapes are
 * bookkeeping too: failing calm is the rule's whole point.
 */
export function isWorkStep(step: Pick<AgentTaskStep, 'id'>): boolean {
  // Normalized event ids are `<turn>:event-<opaque upstream id>`. Upstream
  // ids can themselves contain colons (for example `<call>:source-check`);
  // inspecting only the final suffix would hide that work as bookkeeping.
  const eventMarker = step.id.indexOf(':event-')
  if (eventMarker >= 0 && step.id.length > eventMarker + ':event-'.length) return true
  const marker = step.id.lastIndexOf(':')
  if (marker < 0) return false
  const kind = step.id.slice(marker + 1)
  if (kind.startsWith('event-') || kind.startsWith('action-')) return true
  if (kind.startsWith('tool-')) return !isComposerToolId(kind.slice('tool-'.length))
  return false
}

/**
 * A persisted recalled-memory row, validated on the way back in.
 *
 * `origin` is re-narrowed here rather than trusted: a stored transcript may have
 * been written by a build whose vocabulary was wider, and an unrecognised value
 * must read as `unrecorded` rather than as `stated` — the same rule the wire
 * normalizer applies, for the same reason.
 */
/**
 * A recalled memory as it may be stored: which memory, never its text.
 *
 * Rediger/Glem change the memory itself, not the turns that recalled it, so a
 * stored preview would bring a forgotten memory's text, or the text a
 * correction replaced, back on the next reload. The recall chip reads the
 * reader's current memories when it is opened instead.
 */
export function recalledMemoryWithoutText(memory: RecalledMemory): RecalledMemory {
  return { memoryId: memory.memoryId, role: memory.role, origin: memory.origin, label: memory.label, preview: '' }
}

export function isRecalledMemory(value: unknown): value is RecalledMemory {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.memoryId === 'string' &&
    record.memoryId.length > 0 &&
    typeof record.label === 'string' &&
    typeof record.preview === 'string' &&
    (record.role === 'recall' || record.role === 'inject') &&
    (record.origin === 'stated' || record.origin === 'inferred' || record.origin === 'unrecorded')
  )
}

export function isCitation(value: unknown): value is Citation {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    typeof record.title === 'string' &&
    typeof record.url === 'string' &&
    typeof record.snippet === 'string'
  )
}

export function isChatToolCall(value: unknown): value is ChatToolCall {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    typeof record.name === 'string' &&
    (record.args === undefined || typeof record.args === 'object') &&
    (record.status === undefined || typeof record.status === 'string') &&
    (record.output === undefined || typeof record.output === 'string') &&
    (record.error === undefined || typeof record.error === 'string')
  )
}

export function isChatArtifact(value: unknown): value is ChatArtifact {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    typeof record.kind === 'string' &&
    typeof record.content === 'string' &&
    typeof record.title === 'string' &&
    typeof record.version === 'number' &&
    // `history` rides along in the thread snapshot as untyped JSON; a snapshot
    // written by an older build simply has none.
    (record.history === undefined ||
      (Array.isArray(record.history) && record.history.every(isChatArtifactVersion))) &&
    // A deck's content is its manifest. One that is not a JSON object is
    // refused rather than shown as a deck.
    (record.kind !== 'deck' ||
      (typeof record.content === 'string' && deckManifestVersion(record.content) !== null))
  )
}

/** The deck manifest version this client reads (EP-5 S2/S3). */
export const DECK_MANIFEST_VERSION = 1

/**
 * The `version` a deck manifest states: `null` when the content is not a JSON
 * object, `undefined` when the object states none. Never throws.
 */
export function deckManifestVersion(content: string): number | undefined | null {
  let parsed: unknown
  try {
    parsed = JSON.parse(content)
  } catch {
    return null
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return null
  const version = (parsed as Record<string, unknown>).version
  return typeof version === 'number' ? version : undefined
}

/**
 * A deck this client cannot read (a manifest version other than
 * {@link DECK_MANIFEST_VERSION}) is shown as its JSON in a plain code block
 * rather than handed to a deck view that would misread it. Any other artifact
 * is returned as it is.
 */
export function deckArtifactForDisplay(artifact: ChatArtifact): ChatArtifact {
  if (artifact.kind !== 'deck' || deckManifestVersion(artifact.content) === DECK_MANIFEST_VERSION) {
    return artifact
  }
  return { ...artifact, kind: 'code' }
}

export function isGeneratedFile(value: unknown): value is GeneratedFile {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    typeof record.name === 'string' &&
    typeof record.mime === 'string' &&
    typeof record.size === 'number' &&
    typeof record.url === 'string'
  )
}

export function isChatKnowledgeGrounding(value: unknown): value is ChatKnowledgeGrounding {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    (record.mode === 'retrieve' || record.mode === 'hybrid') &&
    typeof record.query === 'string' &&
    typeof record.lowConfidence === 'boolean' &&
    typeof record.factCount === 'number' &&
    typeof record.sourceCount === 'number' &&
    Array.isArray(record.facts) &&
    Array.isArray(record.sources)
  )
}

export function isComposerAttachment(value: unknown): value is ComposerAttachment {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return (
    typeof record.id === 'string' &&
    typeof record.name === 'string' &&
    typeof record.size === 'number' &&
    typeof record.type === 'string' &&
    typeof record.url === 'string'
  )
}

/** Text files plus PDF/DOCX/XLSX/PPTX prepared by the ephemeral reader. */
const TEXT_INGEST_EXTENSIONS = ['.txt', '.md', '.markdown', '.csv', '.json', '.html', '.htm']

export function textIngestibleAttachments(
  attachments: DashboardComposerSubmitPayload['attachments'],
): DashboardComposerSubmitPayload['attachments'] {
  return attachments.filter((attachment) => {
    if (!attachment.url || attachment.type.startsWith('image/')) return false
    const name = (attachment.name ?? '').toLowerCase()
    // Trust the extension over the MIME type: browsers report `.md` and `.csv`
    // inconsistently (often `application/octet-stream`), and the accept list is
    // extension-based for the same reason.
    return typeof attachment.extractedText === 'string' || attachment.type.startsWith('text/')
      || TEXT_INGEST_EXTENSIONS.some((extension) => name.endsWith(extension))
  })
}

/** Text attachment read in the browser, ready to travel inside the message. */
export type InlinedAttachment = { name: string; content: string }

/**
 * Per-file and per-turn ceilings for inlining. Generous for the documents a
 * chat turn is about (a brief, a CSV, a meeting note); a file over the ceiling
 * is rejected before submission so the user can choose a smaller excerpt.
 */
export const INLINE_ATTACHMENT_MAX_CHARS = 60_000
export const INLINE_ATTACHMENTS_TOTAL_MAX_CHARS = 120_000

/**
 * Reads the text attachments small enough to ride inside the user turn.
 *
 * Why inline at all: the retrieval route (`POST /api/v1/chat/documents`) is
 * asynchronous — Data Plane accepts the row as `pending` and chunks it after
 * the answer has started — so the turn that carried the file saw none of it,
 * or one chunk. Measured live on 2026-09-14 (a 2 KB markdown attachment
 * reached the model as its first line). Putting the text in the message is
 * what "attach and ask" means to the person doing it.
 *
 * Reject unreadable, empty, or oversized files before the draft is cleared.
 * PDF/DOCX/XLSX/PPTX arrive with text from the ephemeral extraction endpoint.
 */
export async function inlineTextAttachments(
  attachments: DashboardComposerSubmitPayload['attachments'],
): Promise<InlinedAttachment[]> {
  const out: InlinedAttachment[] = []
  let total = 0
  for (const attachment of attachments.filter((file) => !file.type.startsWith('image/'))) {
    try {
      if (!attachment.url || !textIngestibleAttachments([attachment]).length) throw new Error('unprepared document')
      let content = attachment.extractedText
      if (content === undefined) {
        const response = await fetch(attachment.url)
        if (!response.ok) throw new Error('unreadable')
        content = await response.text()
      }
      content = content.replace(/\r\n/g, '\n').trim()
      if (!content || content.length > INLINE_ATTACHMENT_MAX_CHARS || total + content.length > INLINE_ATTACHMENTS_TOTAL_MAX_CHARS) {
        throw new Error('empty or too large')
      }
      total += content.length
      out.push({ name: attachment.name?.trim() || 'vedlegg.txt', content })
    } catch (error) {
      // A typed refusal from the read keeps its own words; only a failure nobody typed gets the read refusal.
      if (error instanceof ApiError || error instanceof LocalizedError) throw error
      throw new LocalizedError({
        no: `Kunne ikke lese hele «${attachment.name}». Utkastet er beholdt; prøv igjen med et lesbart, mindre vedlegg.`,
        en: `Could not read all of “${attachment.name}”. Your draft has been kept; try again with a readable, smaller attachment.`,
      })
    }
  }
  return out
}

/**
 * The user's text followed by each inlined document under a labelled fence.
 * The label names the file so the model can cite it ("ifølge kildepakke.md")
 * and so a reader of the durable thread can see where the text came from.
 */
export function appendInlinedAttachments(text: string, inlined: InlinedAttachment[]): string {
  const blocks = inlined.map((entry) => `--- VEDLEGG: ${entry.name} ---\n${entry.content}\n--- SLUTT PÅ VEDLEGG: ${entry.name} ---`)
  return `${text.trim()}\n\n${blocks.join('\n\n')}`
}

/** Retry/edit/continue must keep sources even after the visible bubble is restored. */
export function contentWithPreparedSources(text: string, attachments: ComposerAttachment[] = []): string {
  if (splitInlinedAttachments(text).attachments.length > 0) return text
  const sources = attachments.flatMap(file => typeof file.extractedText === 'string' && file.extractedText.trim()
    ? [{ name: file.name, content: file.extractedText }] : [])
  return sources.length ? appendInlinedAttachments(text, sources) : text
}

/** The document kinds a program can usefully read as bytes (the spreadsheet or PDF behind the inlined text). */
const SANDBOX_FILE_EXTENSIONS = /\.(xlsx|csv|pdf|json|docx|pptx)$/i

/** The decoded size of base64 text, without decoding it. */
function base64ByteLength(base64: string): number {
  const padding = base64.endsWith('==') ? 2 : base64.endsWith('=') ? 1 : 0
  return Math.floor((base64.length * 3) / 4) - padding
}

export async function toStreamAttachments(
  attachments: DashboardComposerSubmitPayload['attachments'],
): Promise<StreamAttachment[]> {
  const out: StreamAttachment[] = []
  // Every image rides the wire, so the turn's bytes are bounded here as the server bounds
  // them: the declared total, or today's constant before the catalogue has answered.
  const budget = getAttachmentPolicy()?.maxTotalBytes ?? ATTACHMENT_MAX_TOTAL_BYTES
  let used = 0
  const spend = (base64: string, name: string) => {
    used += base64ByteLength(base64)
    if (used > budget) {
      throw new LocalizedError({
        no: `Vedleggene er for store til å sendes i én melding («${name}» passerte grensen). Fjern noen og prøv igjen.`,
        en: `The attachments are too large to send in one message (“${name}” went over the limit). Remove some and try again.`,
      })
    }
  }
  for (const attachment of attachments) {
    if (!attachment.type.startsWith('image/')) {
      // Documents are included as text by inlineTextAttachments before inference.
      // Their bytes ride along only where the server says it will stage them for
      // the code sandbox, so a server without that switch gets what it always got.
      if (SANDBOX_FILE_EXTENSIONS.test(attachment.name) && getAttachmentPolicy()?.modes.sandboxFiles) {
        out.push(await toStreamFile(attachment))
      }
      continue
    }
    let parsed: ReturnType<typeof parseDataUrl>
    try {
      if (!attachment.url) throw new Error('missing image')
      let dataUrl = attachment.url
      if (!dataUrl.startsWith('data:')) {
        const response = await fetch(dataUrl)
        if (!response.ok) throw new Error('unreadable image')
        dataUrl = await blobToDataUrl(await response.blob())
      }
      parsed = parseDataUrl(dataUrl)
      if (!parsed || !parsed.base64) throw new Error('invalid image encoding')
    } catch (error) {
      // A typed refusal from the read keeps its own words; only a failure nobody typed gets the read refusal.
      if (error instanceof ApiError || error instanceof LocalizedError) throw error
      throw new LocalizedError({
        no: `Kunne ikke lese bildet «${attachment.name}». Prøv igjen eller fjern bildet.`,
        en: `Could not read the image “${attachment.name}”. Try again or remove the image.`,
      })
    }
    spend(parsed.base64, attachment.name)
    out.push({ kind: 'image', data_base64: parsed.base64, mime_type: parsed.mime || attachment.type })
  }
  return out
}

async function toStreamFile(
  attachment: DashboardComposerSubmitPayload['attachments'][number],
): Promise<StreamAttachment> {
  try {
    if (!attachment.url) throw new Error('missing file')
    let dataUrl = attachment.url
    if (!dataUrl.startsWith('data:')) {
      const response = await fetch(dataUrl)
      if (!response.ok) throw new Error('unreadable file')
      dataUrl = await blobToDataUrl(await response.blob())
    }
    const parsed = parseDataUrl(dataUrl)
    if (!parsed || !parsed.base64) throw new Error('invalid file encoding')
    return {
      kind: 'file',
      name: attachment.name,
      data_base64: parsed.base64,
      mime_type: attachment.type || parsed.mime || 'application/octet-stream',
    }
  } catch (error) {
    // A typed refusal from the read keeps its own words; only a failure nobody typed gets the read refusal.
    if (error instanceof ApiError || error instanceof LocalizedError) throw error
    throw new LocalizedError({
      no: `Kunne ikke lese «${attachment.name}». Prøv igjen eller fjern filen.`,
      en: `Could not read “${attachment.name}”. Try again or remove the file.`,
    })
  }
}

export function createId(prefix: string) {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return `${prefix}-${crypto.randomUUID()}`
  }
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`
}

/**
 * The id an ordinary send is named with, or `undefined` for none.
 *
 * The gateway stores the user's message under it, so a repeat of the same send (a retry after
 * a lost reply, a queued message the run never took and `sendContent` then sends as a new turn)
 * lands as ONE transcript row. Sent regardless of the gateway's `MODEL_GATEWAY_SEND_RECEIPTS`
 * switch (which only decides whether it is honoured). Never for a zero-retention turn (nothing
 * of it is stored, and an id would be a stored fact), a regenerate or an edited resubmit (both
 * state the previous question again on purpose, as a new message), or a send that appends no
 * user turn (continue generation). A send that repeats an earlier one passes its id in.
 */
export function sendClientMessageId(
  options: {
    zdr?: boolean
    regenerated?: boolean
    editResubmit?: boolean
    appendUser?: boolean
    clientMessageId?: string
  },
  mint: () => string,
): string | undefined {
  if (options.zdr || options.regenerated || options.editResubmit || options.appendUser === false) {
    return undefined
  }
  return options.clientMessageId ?? mint()
}

/**
 * Promote the mid-run messages a `queued_input` delivery names to delivered.
 *
 * By id when the delivery carries ids and the message has one: the id is ours, so this stays
 * right when two messages say the same words. By text otherwise (a gateway that sends no ids,
 * or a message queued before ids existed), which is also what keeps a message queued by
 * another tab on the same run correct.
 */
export function markQueuedInputsDelivered(
  entries: readonly QueuedInput[],
  messages: readonly string[],
  ids: readonly string[] = [],
): QueuedInput[] {
  const deliveredIds = new Set(ids.filter((id) => id.length > 0))
  const deliveredTexts = new Set(messages.map((message) => message.trim()))
  return entries.map((entry) => {
    if (entry.state !== 'pending') return entry
    const named = deliveredIds.size > 0 && Boolean(entry.clientMessageId)
    const delivered = named
      ? deliveredIds.has(entry.clientMessageId as string)
      : deliveredTexts.has(entry.content.trim())
    return delivered ? { ...entry, state: 'delivered', note: undefined } : entry
  })
}

/**
 * The run ended on its own. A message the gateway accepted that the run never took (it ended
 * between two tool-round boundaries) is told apart from every other entry: it is returned in
 * `resend`, to go out again as an ordinary turn under the same id, and marked `resent` in the
 * strip. A refused message, one already delivered, and one that never reached the gateway
 * (already waiting to go out) are left as they are.
 */
export function settleQueuedInputsAtRunEnd(entries: readonly QueuedInput[]): {
  entries: QueuedInput[]
  resend: QueuedInput[]
} {
  const resend: QueuedInput[] = []
  const settled = entries.map((entry): QueuedInput => {
    if (entry.state !== 'pending' || !entry.accepted) return entry
    resend.push(entry)
    return { ...entry, state: 'resent', accepted: false, note: undefined }
  })
  return { entries: settled, resend }
}

/**
 * The mid-run strip a new turn starts with. A new turn owns the strip, so the previous run's
 * entries go (they landed in the transcript or were reported refused); a turn that is itself a
 * queued message going out again keeps the entries that say so, or "resent" would be gone the
 * moment it appeared.
 */
export function queuedInputsForNewTurn(
  entries: readonly QueuedInput[],
  resendOfQueued: boolean,
): QueuedInput[] {
  return resendOfQueued ? entries.filter((entry) => entry.state === 'resent') : []
}

export function buildTaskSteps(
  content: string,
  tools: ComposerToolId[],
  mode: 'submit' | 'regenerate',
  turnId: string,
  actions: ChatAction[] = [],
): AgentTaskStep[] {
  const now = new Date().toISOString()
  const turnTitle = createPreview(content, 58)
  const intro = mode === 'regenerate' ? 'Regenerating latest response.' : `Preparing "${createPreview(content)}".`
  return [
    { id: `${turnId}:connect`, title: 'Connect stream', detail: intro, status: 'active', createdAt: now, turnId, turnTitle },
    ...tools.map((tool) => ({
      id: `${turnId}:tool-${tool}`,
      title: TOOL_LABELS[tool],
      detail: tool === 'search'
        ? 'Web search is available; it runs only if the answer needs fresh data.'
        : `${TOOL_LABELS[tool]} is enabled for this answer.`,
      status: 'waiting' as const,
      createdAt: now,
      turnId,
      turnTitle,
    })),
    ...actions.map((action) => ({
      id: `${turnId}:action-${action.id}`,
      title: `${capitalize(action.kind)}: ${action.name || action.id}`,
      detail: `Selected action ${action.id}.`,
      status: 'waiting' as const,
      createdAt: now,
      turnId,
      turnTitle,
    })),
    { id: `${turnId}:answer`, title: 'Compose response', detail: 'Waiting for model output.', status: 'waiting', createdAt: now, turnId, turnTitle },
  ]
}

export function createTurnStep(
  turnId: string,
  turnTitle: string,
  id: string,
  title: string,
  detail: string,
  status: TaskStepStatus,
): AgentTaskStep {
  return {
    id: `${turnId}:${id}`,
    title,
    detail,
    status,
    createdAt: new Date().toISOString(),
    turnId,
    turnTitle,
  }
}

export function normalizeArtifact(event: { id?: string; kind?: string; title?: string; content?: string; version?: number; provenance?: ResearchProvenance }): ChatArtifact | null {
  const content = event.content ?? ''
  // An artifact with no content AND no id carries nothing to show or update —
  // drop it. One WITH an id was genuinely announced by the backend, so it is
  // kept: the panel then renders an explicit "kunne ikke lastes" state instead
  // of silently losing an artifact the user was told about.
  if (!content && !event.id) return null
  const kind = event.kind ?? inferArtifactKind(content)
  return {
    id: event.id ?? createId('artifact'),
    kind,
    title: event.title ?? artifactTitle(kind),
    content,
    version: event.version ?? 0,
    ...(event.provenance ? { provenance: event.provenance } : {}),
  }
}

function isGeneratedFileCheckLevel(value: unknown): value is GeneratedFileCheckLevel {
  return value === 'passed' || value === 'warnings' || value === 'failed' || value === 'skipped'
}

/**
 * The structural check the gateway attached to a generated file, read as the
 * wire sends it. A malformed check is no check at all: the file still arrives,
 * and no chip is shown for a verdict the gateway did not give.
 */
export function normalizeGeneratedFileCheck(value: unknown): GeneratedFileCheck | undefined {
  if (!value || typeof value !== 'object') return undefined
  const record = value as Record<string, unknown>
  if (!isGeneratedFileCheckLevel(record.level) || !Array.isArray(record.checks)) return undefined
  const checks: GeneratedFileCheck['checks'] = []
  for (const item of record.checks as unknown[]) {
    if (!item || typeof item !== 'object') continue
    const finding = item as Record<string, unknown>
    if (typeof finding.id === 'string' && typeof finding.message === 'string' && isGeneratedFileCheckLevel(finding.level)) {
      checks.push({ id: finding.id, level: finding.level, message: finding.message })
    }
  }
  return { level: record.level, checks }
}

/**
 * The rendered PDF copy the gateway attached to a generated Office file, read as
 * the wire sends it. Anything but a PDF `data:` URI with its page facts is no
 * preview, and only PNG `data:` thumbnails are kept, so the frame never loads a URL.
 */
export function normalizeGeneratedFilePreview(value: unknown): GeneratedFilePreview | undefined {
  if (!value || typeof value !== 'object') return undefined
  const record = value as Record<string, unknown>
  if (record.mime !== 'application/pdf') return undefined
  if (typeof record.url !== 'string' || !record.url.startsWith('data:application/pdf;base64,')) return undefined
  if (typeof record.pages !== 'number' || typeof record.fonts_substituted !== 'boolean') return undefined
  if (!Array.isArray(record.thumbs)) return undefined
  const thumbs = (record.thumbs as unknown[]).filter(
    (thumb): thumb is string => typeof thumb === 'string' && thumb.startsWith('data:image/png;base64,'),
  )
  return { mime: 'application/pdf', url: record.url, pages: record.pages, fontsSubstituted: record.fonts_substituted, thumbs }
}

export function normalizeGeneratedFile(event: {
  id?: string
  name?: string
  mime?: string
  type?: string
  url?: string
  size?: number
  check?: unknown
  preview?: unknown
}): GeneratedFile | null {
  if (!event.url || !event.name) return null
  const check = normalizeGeneratedFileCheck(event.check)
  const preview = normalizeGeneratedFilePreview(event.preview)
  return {
    id: event.id ?? event.url,
    name: event.name,
    mime: event.mime ?? event.type ?? 'application/octet-stream',
    size: event.size ?? 0,
    url: event.url,
    ...(check ? { check } : {}),
    ...(preview ? { preview } : {}),
  }
}

/** The citation origins this SPA renders. Any other value from a gateway is dropped, never trusted verbatim. */
const CITATION_ORIGINS: readonly NonNullable<Citation['origin']>[] = ['web', 'knowledge', 'connector']

function citationOrigin(value: unknown): Citation['origin'] {
  return CITATION_ORIGINS.find((origin) => origin === value)
}

export function normalizeCitation(event: { id?: string; title?: string; url?: string; snippet?: string; number?: number; claimId?: string; sourceGroupId?: string; sourceGroupLabel?: string; start?: number; end?: number; origin?: string; quote?: string; locator?: CitationLocator }): Citation | null {
  if (!event.url) return null
  const origin = citationOrigin(event.origin)
  return {
    id: event.id ?? event.url,
    title: event.title ?? hostname(event.url),
    url: event.url,
    snippet: event.snippet ?? '',
    ...(event.number !== undefined ? { number: event.number } : {}),
    ...(event.claimId ? { claimId: event.claimId } : {}),
    ...(event.sourceGroupId ? { sourceGroupId: event.sourceGroupId } : {}),
    ...(event.sourceGroupLabel ? { sourceGroupLabel: event.sourceGroupLabel } : {}),
    ...(event.start !== undefined ? { start: event.start } : {}),
    ...(event.end !== undefined ? { end: event.end } : {}),
    ...(origin ? { origin } : {}),
    ...(event.quote ? { quote: event.quote } : {}),
    ...(event.locator ? { locator: event.locator } : {}),
  }
}

export function normalizeStep(
  event: { id?: string; title?: string; detail?: string; status?: string; frameId?: string },
  turnId?: string,
  turnTitle?: string,
): AgentTaskStep | null {
  if (!event.id && !event.title) return null
  // A step without an id of its own is keyed on the SSE frame that carried
  // it: a resume replays that frame under the same id, so it updates the
  // step it created before instead of adding a second one. A random id is
  // left only for a frame the server sent without one (live-only).
  const rawId = event.id ?? (event.frameId ? `frame-${event.frameId}` : createId('step'))
  return {
    id: turnId ? `${turnId}:event-${rawId}` : rawId,
    title: event.title ?? 'Agent step',
    detail: event.detail ?? event.status ?? 'Updated.',
    status: normalizeTaskStatus(event.status),
    createdAt: new Date().toISOString(),
    turnId,
    turnTitle,
  }
}

export function normalizeToolCall(event: { id?: string; name?: string; args?: unknown }): ChatToolCall | null {
  if (!event.id && !event.name) return null
  return {
    id: event.id ?? event.name ?? createId('tool'),
    name: event.name ?? 'Tool call',
    args: event.args,
    status: 'running',
  }
}

export function applyToolResult(calls: ChatToolCall[], event: { id?: string; output?: string; error?: string; status?: string }): ChatToolCall[] {
  const index = calls.findIndex((call) => call.id === event.id)
  if (index < 0) {
    return [
      ...calls,
      {
        id: event.id ?? createId('tool'),
        name: 'Tool result',
        output: event.output,
        error: event.error,
        status: event.status ?? (event.error ? 'error' : 'done'),
      },
    ]
  }
  return calls.map((call, itemIndex) => itemIndex === index
    ? { ...call, output: event.output, error: event.error, status: event.status ?? (event.error ? 'error' : 'done') }
    : call)
}

export function toolNameForResult(calls: ChatToolCall[], id: string): string | undefined {
  return calls.find((call) => call.id === id)?.name ?? (id.includes('web-search') ? 'web_search' : undefined)
}

export function composerToolIdForToolName(name?: string): ComposerToolId | null {
  if (name === 'web_search') return 'search'
  if (name === 'image') return 'image'
  // No 'reason' chip any more: the composer never sends such a tool, and a
  // model's reasoning has its own surface (`turn.reasoning` -> ReasoningTrace)
  // rather than a tool chip on the user's message.
  if (name === 'research' || name === 'deep_research') return 'research'
  return null
}

export function humanizeToolName(name: string): string {
  if (name === 'web_search') return 'Web search'
  if (name === 'fetch_url') return 'Fetch URL'
  if (name === 'knowledge_search') return 'Knowledge search'
  return name
    .replace(/^mcp__/, 'MCP ')
    .replace(/[_.:-]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/^\w/, (match) => match.toUpperCase()) || 'Tool'
}

export function workStepLabel(label: string, locale: string): string {
  const tool = label.replace(/^Tool:\s*/i, '').replace(/_/g, ' ').toLowerCase()
  const labels: Record<string, [string, string]> = {
    'count words': ['Tell ord', 'Count words'],
    'create artifact': ['Opprett utkast', 'Create draft'],
    'read artifact': ['Les utkast', 'Read draft'],
    'update artifact': ['Oppdater utkast', 'Update draft'],
  }
  const translated = labels[tool]
  if (translated) return translated[locale === 'no' ? 0 : 1]
  return locale === 'no' ? label.replace(/^Tool:\s*/, 'Verktøy: ').replace(/^Answer:\s*/, 'Svar: ') : label
}

/**
 * Compact, single-line summary of tool-call arguments for dense run timelines.
 *
 * Agent tool args are heterogeneous (integration tools carry `provider` /
 * `operation`; web tools carry `query` / `provider`; others are free-form), so
 * this surfaces the salient routing keys first, then a few remaining primitive
 * params — e.g. "provider: slack · operation: send_message · channel: #ops".
 * Returns '' when there is nothing meaningful to show, so callers can fall back
 * to a neutral running label instead of a fabricated one.
 */
export function summarizeToolArgs(args: unknown, maxLength = 180): string {
  if (args == null) return ''
  if (typeof args === 'string') return truncateText(args.trim(), maxLength)
  if (typeof args !== 'object') return truncateText(String(args), maxLength)
  if (Array.isArray(args)) {
    try {
      return truncateText(JSON.stringify(args), maxLength)
    } catch {
      return ''
    }
  }

  const record = args as Record<string, unknown>
  const priorityKeys = ['provider', 'operation', 'action', 'capability', 'query', 'url', 'path', 'id']
  const parts: string[] = []
  const seen = new Set<string>()

  const pushPart = (key: string, value: unknown): void => {
    const rendered = renderArgValue(value)
    if (rendered) parts.push(`${key}: ${rendered}`)
  }

  for (const key of priorityKeys) {
    if (key in record) {
      seen.add(key)
      pushPart(key, record[key])
    }
  }
  for (const [key, value] of Object.entries(record)) {
    if (parts.length >= 6) break
    if (seen.has(key)) continue
    pushPart(key, value)
  }

  return truncateText(parts.join(' · '), maxLength)
}

function renderArgValue(value: unknown): string {
  if (value == null) return ''
  if (typeof value === 'string') return truncateText(value.trim(), 64)
  if (typeof value === 'number' || typeof value === 'boolean') return String(value)
  try {
    return truncateText(JSON.stringify(value), 64)
  } catch {
    return ''
  }
}

export function summarizeToolResult(event: { output?: string; error?: string; status?: string }): string {
  if (event.error) return truncateText(event.error, 260)
  if (event.output) return truncateText(event.output, 260)
  if (event.status) return `Tool completed with status ${event.status}.`
  return 'Tool call completed.'
}

export function summarizeGrounding(grounding: ChatKnowledgeGrounding): string {
  const parts = [
    `${grounding.sourceCount} source${grounding.sourceCount === 1 ? '' : 's'}`,
    `${grounding.factCount} fact${grounding.factCount === 1 ? '' : 's'}`,
  ]
  if (grounding.traceId) parts.push(`trace ${grounding.traceId}`)
  return parts.join(' · ')
}

export function formatUsageSummary(usage: {
  inputTokens?: number
  outputTokens?: number
  costUsd?: number
  costBasis?: 'metered' | 'subscription'
  latencyMs?: number
  confidence?: number
}): string {
  const parts: string[] = []
  if (usage.inputTokens != null || usage.outputTokens != null) {
    parts.push(`${usage.inputTokens ?? 0} in / ${usage.outputTokens ?? 0} out tokens`)
  }
  if (usage.latencyMs != null) parts.push(formatLatency(usage.latencyMs))
  // A subscription-carried turn has no list-price cost; its wire 0.0 is a
  // compatibility placeholder, and printing it would read as "free".
  if (usage.costBasis === 'subscription') parts.push('included in subscription')
  else if (usage.costUsd != null) parts.push(`$${usage.costUsd.toFixed(4)}`)
  if (usage.confidence != null) parts.push(`${Math.round(usage.confidence * 100)}% confidence`)
  return parts.join(' · ') || 'Usage metadata received.'
}

export function extractCitationsFromToolOutput(output: string): Citation[] {
  const trimmed = output.trim()
  if (!trimmed) return []

  const jsonCitations = extractJsonCitations(trimmed)
  if (jsonCitations.length > 0) return jsonCitations

  const lines = trimmed.split(/\r?\n/).map((line) => line.trim()).filter(Boolean)
  const citations: Citation[] = []
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    if (!line) continue
    const match = line.match(/https?:\/\/[^\s)"'<>,]+/i)
    if (!match) continue
    const url = match[0].replace(/[)\].,;]+$/, '')
    if (!isValidUrl(url) || citations.some((citation) => citation.url === url)) continue
    const previous = lines[index - 1]?.replace(/^\d+[.)]\s*/, '').trim()
    const nextLine = lines[index + 1]
    const next = nextLine && !/^https?:\/\//i.test(nextLine)
      ? nextLine
      : ''
    citations.push({
      id: `web-${citations.length + 1}-${url}`,
      title: previous || hostname(url),
      url,
      snippet: next,
    })
    if (citations.length >= 8) break
  }
  return citations
}

export function extractJsonCitations(output: string): Citation[] {
  try {
    const parsed = JSON.parse(output) as unknown
    const root = objectValue(parsed)
    const list = Array.isArray(parsed)
      ? parsed
      : Array.isArray(root?.results)
        ? root.results
        : Array.isArray(root?.citations)
          ? root.citations
          : []
    const direct = citationFromUnknown(parsed, 0)
    if (direct) return [direct]
    return list
      .map(citationFromUnknown)
      .filter((citation): citation is Citation => Boolean(citation))
      .slice(0, 8)
  } catch {
    return []
  }
}

export function citationFromUnknown(value: unknown, index: number): Citation | null {
  const item = objectValue(value)
  if (!item) return null
  const url = stringValue(item.url) ?? stringValue(item.href) ?? stringValue(item.final_url) ?? stringValue(item.finalUrl)
  if (!url || !isValidUrl(url)) return null
  const title = stringValue(item.title) ?? hostname(url)
  const snippet = stringValue(item.snippet) ?? stringValue(item.description) ?? ''
  if (isFailedFetchCitation(item, title, snippet)) return null
  const claimId = stringValue(item.claim_id) ?? stringValue(item.claimId)
  const sourceGroupId = stringValue(item.source_group_id) ?? stringValue(item.sourceGroupId)
  const start = numberValue(item.start) ?? numberValue(item.start_offset) ?? numberValue(item.startOffset)
  const end = numberValue(item.end) ?? numberValue(item.end_offset) ?? numberValue(item.endOffset)
  const origin = citationOrigin(item.origin)
  return {
    id: stringValue(item.id) ?? `web-${index + 1}-${url}`,
    title,
    url,
    snippet,
    ...(claimId ? { claimId } : {}),
    ...(sourceGroupId ? { sourceGroupId } : {}),
    ...(start !== undefined ? { start } : {}),
    ...(end !== undefined ? { end } : {}),
    ...(origin ? { origin } : {}),
  }
}

export function isFailedFetchCitation(item: Record<string, unknown>, title: string, snippet: string): boolean {
  const status = Number(item.status ?? item.status_code ?? item.statusCode)
  if (status === 404) return true
  const content = stringValue(item.content) ?? stringValue(item.markdown) ?? stringValue(item.text) ?? ''
  const label = `${title} ${snippet}`.toLowerCase()
  return !content.trim() && (label.includes('404') || label.includes('content not found') || label.includes('not found'))
}

export function upsertToolCall(calls: ChatToolCall[], call: ChatToolCall): ChatToolCall[] {
  const index = calls.findIndex((item) => item.id === call.id)
  if (index < 0) return [...calls, call]
  return calls.map((item, itemIndex) => itemIndex === index ? { ...item, ...call } : item)
}

/**
 * Adds or UPDATES an artifact in a turn's list.
 *
 * A repeated `id` never duplicates the entry: the incoming revision is folded
 * into the existing one by `mergeArtifactVersion`, which keeps the full version
 * history and leaves the top-level fields on the newest revision. `carried` is
 * an artifact with the same id found on an EARLIER turn — the model can rewrite
 * a document several turns later, and its earlier revisions must not be lost.
 */
export function upsertArtifact(
  artifacts: ChatArtifact[],
  artifact: ChatArtifact,
  carried?: ChatArtifact,
): ChatArtifact[] {
  const index = artifacts.findIndex((item) => item.id === artifact.id)
  if (index < 0) return [...artifacts, mergeArtifactVersion(carried, artifact)]
  return artifacts.map((item, itemIndex) => (
    itemIndex === index ? mergeArtifactVersion(item, artifact) : item
  ))
}

export function upsertGeneratedFile(files: GeneratedFile[], file: GeneratedFile): GeneratedFile[] {
  const index = files.findIndex((item) => item.id === file.id || item.url === file.url)
  if (index < 0) return [...files, file]
  return files.map((item, itemIndex) => itemIndex === index ? { ...item, ...file } : item)
}

/**
 * Dedup-and-MERGE, not dedup-and-drop (chat-wave2 citations spec §2.5): the
 * server's post-stream anchor pass re-emits a citation with the SAME `id`
 * once the answer is complete, now carrying `claimId`/`sourceGroupId`/
 * `start`/`end` — a plain no-op-on-duplicate would silently discard that
 * update. A second sighting of the same `id` (or, absent that, the same
 * `url`) merges its fields onto the already-recorded citation in place
 * instead of adding a second row or being dropped.
 */
export function upsertCitation(citations: Citation[], citation: Citation): Citation[] {
  const index = citations.findIndex((item) => item.id === citation.id || item.url === citation.url)
  if (index < 0) return [...citations, citation]
  return citations.map((item, i) => (i === index ? { ...item, ...citation } : item))
}

export function upsertStepEvidence(evidence: AgentTaskStepEvidence[], item: AgentTaskStepEvidence): AgentTaskStepEvidence[] {
  if (evidence.some((existing) => existing.id === item.id || existing.href === item.href)) return evidence
  return [...evidence, item]
}

export function citationEvidence(citation: Citation): AgentTaskStepEvidence {
  return {
    id: citation.id || citation.url,
    label: citation.title || hostname(citation.url),
    value: hostname(citation.url),
    href: citation.url,
  }
}

export function missingSearchResultStep(step: AgentTaskStep, status: TaskStepStatus): AgentTaskStep | null {
  if (status !== 'done' || !step.id.endsWith(':tool-search') || step.status !== 'waiting') return null
  // With web search available by default and used per-query (staleness
  // heuristic + model judgment), a turn that completes without searching is
  // the normal outcome for timeless questions — not a failure. Stay honest
  // (the answer is not web-verified) without alarming the user.
  return {
    ...step,
    detail: 'No web search needed — answered from existing knowledge or other tools.',
    expandedDetail: 'Web search was available for this turn, but the query did not require fresh web data, so no search ran. Time-sensitive claims in this answer are not web-verified.',
    status: 'done',
  }
}

export function readBrowseWebPreference(): boolean {
  // Default ON: web search is meant to be available on every turn, with the
  // backend deciding per-query whether it is actually used (staleness
  // heuristic + model judgment). Only an explicit opt-out ('0') disables it.
  return readClientValue(CHAT_BROWSE_WEB_KEY) !== '0'
}

export function writeBrowseWebPreference(enabled: boolean): void {
  writeClientValue(CHAT_BROWSE_WEB_KEY, enabled ? '1' : '0')
}

export function searchCompletionDetail(toolName: string, failed: boolean, sourceCount: number, output?: string): string {
  if (failed) return `${humanizeToolName(toolName)} failed.`
  if (sourceCount > 0) return `Web search returned ${sourceCount} source${sourceCount === 1 ? '' : 's'}.`
  if (output?.trim()) return 'Web search returned raw output.'
  return 'Web search completed, but no source payload was received.'
}

export function searchQueryFromArgs(args: unknown): string | undefined {
  const direct = objectValue(args)
  if (direct) return stringValue(direct.query)
  if (typeof args !== 'string') return undefined
  try {
    return stringValue(objectValue(JSON.parse(args))?.query)
  } catch {
    return undefined
  }
}
