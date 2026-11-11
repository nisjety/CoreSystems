// Norwegian and English text for every typed zero-data-retention refusal.
//
// The Model Plane refuses a request under Zero Data Retention with one stable
// code, an action and, for a feature refusal, the feature's key. Those are the
// golden `./contracts/zdr-refusals.json`: a byte-identical copy of
// apps/Model Plane/api/contracts/zdr-refusals.json, pinned by sha256 beside it.
// The English text below is that golden's text, verbatim; the Norwegian is this
// SPA's own. The test beside this file fails if either drifts from the golden.
//
// A code this SPA does not know (a newer server) gets the generic text, never the
// server's own English, and never a claim about where the request went.

export type ZdrRefusalCode =
  | 'zdr_unavailable'
  | 'zdr_feature_not_supported'
  | 'zdr_connection_not_supported'
  | 'zdr_connection_not_acknowledged'
  | 'zdr_explicit_request_not_supported_by_connection'
  | 'zdr_decision_unavailable'
  | 'zdr_waiver_route_mismatch'
  | 'zdr_posture_mismatch'
  | 'zdr_durable_mutation_forbidden'
  | 'zdr_inference_failed'

/** What the reader can do, as the golden names it. */
export type ZdrRefusalAction = 'none' | 'retry' | 'choose_another_model' | 'open_integrations' | 'turn_off_feature'

export type ZdrFeatureKey =
  | 'attachments'
  | 'image_generation'
  | 'deep_research'
  | 'agentic'
  | 'plan_mode'
  | 'web_search'
  | 'client_tools'
  | 'structured_output'
  | 'skills'
  | 'pinned_messages'
  | 'spend_cap'
  | 'deployed_agent'
  | 'tool_execution'
  | 'agent_run'
  | 'run_control'
  | 'scheduled_step'
  | 'approval_continuation'
  | 'send_message'
  | 'lsp_query'
  | 'mcp_execution'
  | 'command_execution'
  | 'speech_synthesis'
  | 'speech_transcription'
  | 'browser_run'
  | 'browser_run_control'
  | 'browser_tool'
  | 'browser_human_assist'
  | 'translation'
  | 'document_analysis'
  | 'language_analysis'
  | 'realtime_voice'
  | 'video_generation'
  | 'connector_read'
  | 'message_versions'
  | 'sandbox_workspace'
  | 'user_profile'
  | 'agent_api'
  | 'conversation_export'
  | 'conversation_share'
  | 'conversation_import'
  | 'run_inspection'
  | 'context_references'
  | 'thread_goal'
  | 'durable_crawl'
  | 'browser_profile'
  | 'session_resume'
  | 'page_screenshot'

/** The reader's-language chooser, the same shape the rest of the SPA passes around. */
export type Translate = (noText: string, enText: string) => string

type Copy = { readonly no: string; readonly en: string }

/** The sha256 of `./contracts/zdr-refusals.json`. Changing the golden changes this too. */
export const ZDR_REFUSALS_CONTRACT_SHA256 = 'ae4473ce0a3ddd3d6d104aad3f90509014da63b60e2db27766983956708437d7'

export type ZdrRefusalText = {
  /** Norwegian. A feature refusal names the feature with `{feature}`. */
  readonly no: string
  /** English: the golden's `message_en`, verbatim (`{feature}` as in the golden). */
  readonly en: string
  readonly action: ZdrRefusalAction
  readonly retryable: boolean
  readonly takesFeature: boolean
}

export const REFUSAL_TEXT: Record<ZdrRefusalCode, ZdrRefusalText> = {
  zdr_unavailable: {
    no: 'Zero Data Retention gjelder for denne forespørselen, men ingen modellinstans er godkjent til å levere det. Forespørselen ble ikke sendt til en modell.',
    en: 'Zero data retention applies to this request, but no model deployment is attested to provide it. The request was not sent to a model.',
    action: 'choose_another_model',
    retryable: false,
    takesFeature: false,
  },
  zdr_feature_not_supported: {
    no: 'Dette støttes ikke med Zero Data Retention ennå: {feature}. Forespørselen ble avvist før noe kall til en modell.',
    en: 'This is not supported with zero data retention yet: {feature}. The request was refused before any model call.',
    action: 'turn_off_feature',
    retryable: false,
    takesFeature: true,
  },
  zdr_connection_not_supported: {
    no: 'Zero Data Retention er slått på for denne organisasjonen, og ChatGPT-tilkoblingen som er valgt for denne samtalen, kan ikke levere det. Forespørselen ble ikke sendt til en modell, og ingen annen modell ble brukt.',
    en: 'Zero data retention is on for this organisation, and the ChatGPT connection selected for this chat cannot provide it. The request was not sent to any model. No other model was used.',
    action: 'choose_another_model',
    retryable: false,
    takesFeature: false,
  },
  zdr_connection_not_acknowledged: {
    no: 'Zero Data Retention er slått på for denne organisasjonen, og tilkoblingen som er valgt for denne samtalen er ikke bekreftet for Zero Data Retention. Forespørselen ble ikke sendt til en modell.',
    en: 'Zero data retention is on for this organisation, and the connection selected for this chat has not been confirmed for it. The request was not sent to any model.',
    action: 'open_integrations',
    retryable: false,
    takesFeature: false,
  },
  zdr_explicit_request_not_supported_by_connection: {
    no: 'Midlertidig chat krever Zero Data Retention, og tilkoblingen som er valgt for denne samtalen kan ikke levere det. Forespørselen ble ikke sendt til en modell.',
    en: 'Temporary chat requires zero data retention, and the connection selected for this chat cannot provide it. The request was not sent to any model.',
    action: 'choose_another_model',
    retryable: false,
    takesFeature: false,
  },
  zdr_decision_unavailable: {
    no: 'Innstillingene for Zero Data Retention kan ikke bekreftes akkurat nå. Forespørselen ble ikke sendt til en modell. Prøv igjen om litt.',
    en: 'The zero data retention settings cannot be confirmed right now. The request was not sent to any model. Try again shortly.',
    action: 'retry',
    retryable: true,
    takesFeature: false,
  },
  zdr_waiver_route_mismatch: {
    no: 'Forespørselen peker på en annen rute enn den avgjørelsen om Zero Data Retention gjelder for. Forespørselen ble ikke sendt til en modell.',
    en: 'This request names a route other than the one its zero data retention decision was made for. The request was not sent to any model.',
    action: 'none',
    retryable: false,
    takesFeature: false,
  },
  zdr_posture_mismatch: {
    no: 'Innstillingen for Zero Data Retention på legitimasjonen til denne forespørselen stemmer ikke. Forespørselen ble ikke sendt til en modell.',
    en: 'The zero data retention setting on the credentials for this request does not match. The request was not sent to any model.',
    action: 'none',
    retryable: false,
    takesFeature: false,
  },
  zdr_durable_mutation_forbidden: {
    no: 'Legitimasjon med Zero Data Retention kan ikke opprette, endre eller slette varig data.',
    en: 'Zero Data Retention credentials cannot create, change, or delete durable state.',
    action: 'none',
    retryable: false,
    takesFeature: false,
  },
  zdr_inference_failed: {
    no: 'Forespørselen med Zero Data Retention kunne ikke fullføres. Ingenting ble lagret. Prøv igjen.',
    en: 'The zero data retention request could not be completed. Nothing was stored.',
    action: 'retry',
    retryable: true,
    takesFeature: false,
  },
}

/** The feature vocabulary: the key the server sends, and the words that fill `{feature}`. */
export const FEATURE_LABELS: Record<ZdrFeatureKey, Copy> = {
  attachments: { no: 'fil- og bildevedlegg', en: 'file and image attachments' },
  image_generation: { no: 'bildegenerering', en: 'image generation' },
  deep_research: { no: 'dyp research', en: 'deep research' },
  agentic: { no: 'Utfør-modus', en: 'Do mode' },
  plan_mode: { no: 'planmodus', en: 'plan mode' },
  web_search: { no: 'websøk og nettlesing', en: 'web search and browsing' },
  client_tools: { no: 'verktøy som klienten angir', en: 'tools the client declares' },
  structured_output: { no: 'strukturert utdata', en: 'structured output' },
  skills: { no: 'valgte ferdigheter', en: 'picked skills' },
  pinned_messages: { no: 'festede meldinger', en: 'pinned messages' },
  spend_cap: { no: 'en kostnadsgrense per svar', en: 'a per-turn spend limit' },
  deployed_agent: { no: 'utrullede agenter', en: 'deployed agents' },
  tool_execution: { no: 'kjøring av verktøy', en: 'tool execution' },
  agent_run: { no: 'agentkjøringer', en: 'agent runs' },
  run_control: { no: 'kontroll av kjøringer', en: 'run controls' },
  scheduled_step: { no: 'planlagte kjøringer', en: 'scheduled runs' },
  approval_continuation: { no: 'fortsettelse etter godkjenning', en: 'approval continuations' },
  send_message: { no: 'sending av meldinger', en: 'sending messages' },
  lsp_query: { no: 'kodeanalyse', en: 'code intelligence queries' },
  mcp_execution: { no: 'kjøring av MCP-verktøy', en: 'MCP tool execution' },
  command_execution: { no: 'kjøring av kommandoer', en: 'command execution' },
  speech_synthesis: { no: 'talesyntese', en: 'speech synthesis' },
  speech_transcription: { no: 'transkribering av tale', en: 'speech transcription' },
  browser_run: { no: 'nettleserkjøringer', en: 'browser runs' },
  browser_run_control: { no: 'kontroll av nettleserkjøringer', en: 'browser run controls' },
  browser_tool: { no: 'nettleserverktøy', en: 'browser tools' },
  browser_human_assist: { no: 'hjelpeforespørsler i nettleseren', en: 'browser help requests' },
  translation: { no: 'oversettelse', en: 'translation' },
  document_analysis: { no: 'dokumentanalyse', en: 'document analysis' },
  language_analysis: { no: 'språkanalyse', en: 'language analysis' },
  realtime_voice: { no: 'sanntidsstemmeøkter', en: 'realtime voice sessions' },
  video_generation: { no: 'videogenerering', en: 'video generation' },
  connector_read: { no: 'lesing fra tilkoblede apper', en: 'connected-app reads' },
  message_versions: { no: 'meldingsversjoner', en: 'message versions' },
  sandbox_workspace: { no: 'filer som beholdes mellom kall i et trådarbeidsområde', en: 'files kept between calls in a thread workspace' },
  user_profile: { no: 'den faste profilen', en: 'the standing profile' },
  agent_api: { no: 'agent-API', en: 'the Agent API' },
  conversation_export: { no: 'eksport av samtaler', en: 'conversation export and replay' },
  conversation_share: { no: 'deling av samtaler', en: 'conversation sharing' },
  conversation_import: { no: 'import av samtaler', en: 'conversation import' },
  run_inspection: { no: 'tidslinje og innsyn i kjøringer', en: 'run timing and request inspection' },
  context_references: { no: 'referanser til dokumenter, samtaler eller utvalg', en: 'references to documents, threads or selections' },
  thread_goal: { no: 'varige mål', en: 'persistent goals' },
  durable_crawl: { no: 'varige gjennomsøk og batcher', en: 'durable crawls and batches' },
  browser_profile: { no: 'lagrede nettleserprofiler', en: 'saved browser profiles' },
  session_resume: { no: 'gjenopptak av en lagret nettleserøkt', en: 'resuming a saved browser session' },
  page_screenshot: { no: 'skjermbilder av sider', en: 'page screenshots' },
}

/** A refusal whose feature the SPA does not know names it this way. */
const UNNAMED_FEATURE: Copy = { no: 'denne funksjonen', en: 'this feature' }

/** A code the SPA does not know. It says nothing about whether a model was called. */
const GENERIC_REFUSAL: Copy = {
  no: 'Forespørselen med Zero Data Retention kunne ikke fullføres. Kontakt support hvis problemet vedvarer.',
  en: 'The request could not be completed with zero data retention. Contact support if the problem persists.',
}

/** The next step for an action that needs words. Retry and none need none: the button or the text says it. */
const NEXT_STEP: Partial<Record<ZdrRefusalAction, Copy>> = {
  choose_another_model: { no: 'Velg en annen modell og send på nytt.', en: 'Choose another model and send again.' },
  turn_off_feature: { no: 'Slå av funksjonen og send på nytt.', en: 'Turn the feature off and send again.' },
  open_integrations: {
    no: 'Gå til ChatGPT-tilkoblingen, se over den, og send på nytt.',
    en: 'Go to the ChatGPT connection, review it, and send again.',
  },
}

/**
 * Any code the Model Plane sends under the zero-retention prefix. A plain boolean,
 * not a type guard: a guard would narrow a caller's other strings away too.
 */
export function isZdrCode(code: unknown): boolean {
  return typeof code === 'string' && code.startsWith('zdr_')
}

/** A code of the vendored catalogue, which this SPA renders by its own text. */
export function isZdrRefusalCode(code: unknown): code is ZdrRefusalCode {
  return typeof code === 'string' && Object.hasOwn(REFUSAL_TEXT, code)
}

function featureCopy(feature: string | null | undefined): Copy | undefined {
  if (typeof feature !== 'string' || !Object.hasOwn(FEATURE_LABELS, feature)) return undefined
  return FEATURE_LABELS[feature as ZdrFeatureKey]
}

export type ZdrRefusalPresentation = {
  /** The code as received. */
  readonly code: string
  /** False when this SPA does not know the code, and the text is the generic one. */
  readonly known: boolean
  /** What happened, in the reader's language, with the feature named where the code names one. */
  readonly message: string
  /** The next step, in the reader's language, or null when the action needs no words. */
  readonly next: string | null
  readonly action: ZdrRefusalAction
  readonly retryable: boolean
}

/**
 * The reader's words for a zero-retention refusal. The feature is named by its
 * key (never by server text); a feature the SPA does not know reads as "this
 * feature". An unknown code gets the generic text with no action and no claim.
 */
export function zdrRefusalPresentation(
  code: string,
  feature: string | null | undefined,
  tr: Translate,
): ZdrRefusalPresentation {
  if (!isZdrRefusalCode(code)) {
    return {
      code,
      known: false,
      message: tr(GENERIC_REFUSAL.no, GENERIC_REFUSAL.en),
      next: null,
      action: 'none',
      retryable: false,
    }
  }
  const entry = REFUSAL_TEXT[code]
  const label = entry.takesFeature ? (featureCopy(feature) ?? UNNAMED_FEATURE) : undefined
  const no = label ? entry.no.replace('{feature}', () => label.no) : entry.no
  const en = label ? entry.en.replace('{feature}', () => label.en) : entry.en
  const step = NEXT_STEP[entry.action]
  return {
    code,
    known: true,
    message: tr(no, en),
    next: step ? tr(step.no, step.en) : null,
    action: entry.action,
    retryable: entry.retryable,
  }
}
