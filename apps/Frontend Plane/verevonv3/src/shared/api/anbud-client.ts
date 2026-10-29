import { anbudCatalogueContract, anbudNoticeDetailContract, anbudSearchContract } from '@/shared/actions/anbud'
import { loadTenderMatchingContext, type TenderMatchingContext } from './anbud-matching-context'
import { requestJson } from '@/shared/api/http'

/** How far a match reaches beyond the profile's own codes: strict, balanced (the default) or wide. */
export type TenderStrictness = 'strict' | 'balanced' | 'wide'

/** The company profile a search is made for. The server checks the codes; the feature checks their shape first. */
export type TenderProfile = {
  orgNumber?: string
  cpv: string[]
  nuts: string[]
  keywords: string[]
  valueRange?: { currency: string; min?: number; max?: number }
}

export type TenderMatchRequest = {
  paginate?: boolean
  cursor?: string
  sort?: 'relevance' | 'deadline' | 'title'
  filters?: { query?: string; buyer?: string; region?: string; cpv?: string; status?: string; deadline?: string; verdict?: string }
  trackedOnly?: boolean
  context?: TenderMatchingContext
  profile: TenderProfile
  strictness: TenderStrictness
  show?: 'relevant' | 'all'
}

export type RetenderRequest = {
  profile: TenderProfile
  strictness: TenderStrictness
  horizonMonths?: number
}

export type TenderCheck = {
  rule: string
  outcome: 'pass' | 'conditional' | 'fail' | 'unknown'
  detail: string
  cite: { field: string; lot?: string; index?: number; document?: string; snippet?: string }
  daysLeft?: number
}

export type TenderMoney = { currency: string; amount: string }

export type TenderMatch = {
  retained?: boolean
  retentionReason?: string
  noticeType?: string
  procedureType?: string
  status?: string
  arrangementTypes?: string[]
  estimatedValue?: TenderMoney
  awardedValue?: TenderMoney
  issuedAt?: string
  publishedDate?: string
  winners?: TenderWinner[]
  winnersStatus?: string
  contracts?: Array<{ lot: string; title?: string; startDate?: string; endDate?: string; endBasis: string; duration?: { value: number; unit: string }; unlimited: boolean }>
  noticeId: string
  metadataAvailable?: boolean
  title: string
  buyer?: string
  cpv: string[]
  nuts: string[]
  deadline?: { at: string; osloDay: string; daysLeft: number; kind: string; lot?: string }
  /** YYYY-MM-DD: the day a prior information notice plans its call for tenders for. */
  plannedNotice?: string
  verdict: 'pass' | 'conditional' | 'fail' | 'unknown'
  band: 'strong' | 'good' | 'stretch' | 'none'
  gate: TenderCheck[]
  fit: TenderCheck[]
  gateCounts: { pass: number; conditional: number; fail: number; unknown: number }
}

export type CompanyStatus = {
  orgNumber: string
  checked: boolean
  reason?: string
  name?: string
  canBid?: boolean
}

export type TenderFunnel = {
  scanned: number
  unreadable: number
  unsupported: number
  awards: number
  opportunities: number
  superseded: number
  gate: { pass: number; conditional: number; fail: number }
  matched: number
}

export type TenderMatches = {
  nextCursor?: string
  previousCursor?: string
  page?: number
  pageSize?: number
  totalMatched?: number
  inboxTotal?: number
  activeAssessed?: number
  trackedResults?: TenderMatch[]
  coverage?: string
  asOf: string
  strictness: TenderStrictness
  company?: CompanyStatus
  funnel: TenderFunnel
  /** How many more tenders each wider step would show, keyed by the step's name. */
  gainedByWidening: Record<string, number>
  results: TenderMatch[]
}

export type TenderAudit = {
  result: TenderMatch
  company?: CompanyStatus
  evidence: { source: string; noticeId: string; sha256: string; fetchedAt: string; bytes: number }
  parse: {
    root: string
    sdkVersion: string
    flavour: string
    contactPoints: number
    issuesTotal: number
    unread: string[]
    unreadTotal: number
  }
  incumbency?: { related: number; withheldWinners: number; medianTendersReceived?: number }
  incumbencyStatus?: { checked: boolean; reason?: string; scanned: number; awards: number; winners: string }
}

export type TenderWinner = { name?: string; orgNumber?: string; status: string }

export type TenderAward = {
  noticeId: string
  title: string
  lotTitle?: string
  buyer?: string
  relation?: string
  awardDate?: string
  signed?: string
  contractEnd?: string
  endBasis: string
  renewals?: number
  tendersReceived?: number
  winners: TenderWinner[]
  /** exact: the strictest step already reaches the contract's codes. near: only a wider step does. */
  reach?: 'exact' | 'near'
}

/** What a wider step widens, by the core's name for it. The radar words each one. */
export type TenderDial = 'cpv_group' | 'cpv_division' | 'nuts2_region' | 'nuts1_region'

export type TenderWidening = { to: TenderStrictness; adds: number; dials: TenderDial[] }

export type Retenders = {
  asOf: string
  window: { from: string; to: string }
  strictness: TenderStrictness
  /** Every contract the scope matches in the window, before the list is cut at its limit. */
  matching: number
  scanned: number
  awards: number
  winners: string
  widening: TenderWidening[]
  results: TenderAward[]
}

/** The core's field names are snake_case; the browser's are camelCase. Keys are converted here; values are kept. */
function camelize(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(camelize)
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, inner]) => [
        key.replace(/_([a-z0-9])/g, (_match, char: string) => char.toUpperCase()),
        camelize(inner),
      ]),
    )
  }
  return value
}

/** The profile as the core reads it: its own field names, and no empty optional fields. */
function wireProfile(profile: TenderProfile) {
  return {
    org_number: profile.orgNumber || undefined,
    cpv: profile.cpv,
    nuts: profile.nuts,
    keywords: profile.keywords,
    value_range: profile.valueRange
      ? { currency: profile.valueRange.currency, min: profile.valueRange.min, max: profile.valueRange.max }
      : undefined,
  }
}

function post<T>(path: string, body: unknown, signal?: AbortSignal): Promise<T> {
  return requestJson<unknown>(path, { method: 'POST', body: JSON.stringify(body), signal }).then(
    (data) => camelize(data) as T,
  )
}

/** The matches for a profile, with the gate and the fit of each tender. The organisation comes from the session. */
export async function searchTenderMatches(request: TenderMatchRequest, signal?: AbortSignal): Promise<TenderMatches> {
  const context = request.trackedOnly || request.cursor ? { companyKeywords: [], userKeywords: [] } : request.context ?? await loadTenderMatchingContext(signal)
  return post<TenderMatches>(
    anbudSearchContract.path,
    {
      profile: wireProfile(request.profile),
      strictness: request.strictness,
      show: request.show ?? 'relevant',
      ...(request.paginate ? { paginate: true, ...(request.cursor ? { cursor: request.cursor } : {}), sort: request.sort ?? 'relevance', filters: request.filters ?? {} } : {}),
      ...(request.trackedOnly ? { tracked_only: true } : {}),
      ...(context.companyKeywords.length || context.userKeywords.length ? { context: { company_keywords: context.companyKeywords, user_keywords: context.userKeywords } } : {}),
    },
    signal,
  )
}

/** One tender's audit sheet: its checks, where its text came from, how it was read, and its buyer's history. */
export async function fetchTenderAudit(
  noticeId: string,
  request: TenderMatchRequest,
  signal?: AbortSignal,
): Promise<TenderAudit> {
  const context = request.context ?? await loadTenderMatchingContext(signal)
  return post<TenderAudit>(
    `/api/v1/anbud/matches/${encodeURIComponent(noticeId)}`,
    { profile: wireProfile(request.profile), strictness: request.strictness, ...(context.companyKeywords.length || context.userKeywords.length ? { context: { company_keywords: context.companyKeywords, user_keywords: context.userKeywords } } : {}) },
    signal,
  )
}

/** The re-tender radar: contracts whose base period ends within the horizon, for the profile's codes. */
export function fetchRetenders(request: RetenderRequest, signal?: AbortSignal): Promise<Retenders> {
  return post<Retenders>(
    '/api/v1/anbud/retenders',
    {
      profile: wireProfile(request.profile),
      strictness: request.strictness,
      horizon_months: request.horizonMonths ?? 12,
    },
    signal,
  )
}

export type TenderCatalogue = { totalMatched?: number; facets?: Record<string, Record<string, number>>; totalStored?: number; results: TenderMatch[]; nextCursor?: string; scanned: number }
export type TenderCatalogueRequest = { cursor?: string; sort?: string; direction?: string; noticeType?: string; query?: string; buyer?: string; region?: string; cpv?: string; status?: string; deadline?: string }
export function searchTenderCatalogue(request: TenderCatalogueRequest, signal?: AbortSignal): Promise<TenderCatalogue> {
  const { noticeType, ...rest } = request
  return post<TenderCatalogue>(anbudCatalogueContract.path, { ...rest, notice_type: noticeType }, signal)
}

export type TenderNoticeDetail = {
  result: TenderMatch
  descriptionTruncated: boolean
  notice: {
    description: string; procurementType?: string
    documents: Array<{ lot?: string; url: string }>; documentsTotal: number
    selectionCriteria: Array<{ lot?: string; type: string; name: string; description: string }>; selectionCriteriaTotal: number
    lots: Array<{ id: string; title?: string; cpv: string[]; value?: TenderMoney; tenderDeadline?: string; participationDeadline?: string; renewals?: number; awardCriteria: Array<{ type: string; name?: string; description?: string; weightPercent?: number }> }>; lotsTotal: number
    exclusionGrounds: number; criteriaInDocuments: boolean
    exclusionCriteria?: Array<{ lot?: string; type: string; description?: string }>
    awards?: Array<{ lot: string; status: string; tendersReceived?: number; value?: TenderMoney; contractSigned?: string }>
    awardDate?: string
    change?: { replaces: string; reasonCode: string; reason: string; sections: string[] }
  }
  evidence: TenderAudit['evidence']
}
export function fetchTenderNotice(noticeId: string, signal?: AbortSignal): Promise<TenderNoticeDetail> {
  return post<TenderNoticeDetail>(`${anbudNoticeDetailContract.path}/${encodeURIComponent(noticeId)}`, {}, signal)
}

/** The company profile an organisation saved: the profile as the form reads it, and the search step last used. */
export type StoredCompanyProfile = TenderProfile & {
  strictness: TenderStrictness
  /** Where each field came from: 'organisation', 'wins' or 'user'. A profile saved before sources were kept has none. */
  sources?: Record<string, string>
  /** False until the organisation has checked a suggested profile and saved it. Absent on profiles saved by hand. */
  confirmed?: boolean
  /** How many wins a suggested profile was drawn from. */
  suggestedFrom?: { wins: number }
}

/** The organisation's saved company profile, or null when it has never saved one. */
export function readWorkspaceProfile(signal?: AbortSignal): Promise<{ profile: StoredCompanyProfile; updatedAt: string } | null> {
  return post('/api/v1/anbud/workspace/profile/read', {}, signal)
}

export function writeWorkspaceProfile(profile: StoredCompanyProfile, signal?: AbortSignal): Promise<void> {
  return post<unknown>('/api/v1/anbud/workspace/profile/write', { profile }, signal).then(() => undefined)
}

export type PipelineStage = { noticeId: string; stage: string; updatedAt: string }

/** Where each tender stands in the organisation's pipeline. */
export function readWorkspacePipeline(signal?: AbortSignal): Promise<PipelineStage[]> {
  return post<{ items: PipelineStage[] }>('/api/v1/anbud/workspace/pipeline/read', {}, signal).then((data) => data.items)
}

/** Puts a tender in a pipeline stage for the organisation. */
export function writeWorkspaceStage(noticeId: string, stage: string, signal?: AbortSignal): Promise<void> {
  return post<unknown>('/api/v1/anbud/workspace/pipeline/write', { notice_id: noticeId, stage }, signal).then(() => undefined)
}

/** What the organisation's own wins suggest. The draft is saved only where the organisation has no profile yet. */
export type WorkspaceSuggestion = {
  applied: boolean
  reason?: string
  profile?: { profile: StoredCompanyProfile; updatedAt: string }
  suggestion: { orgNumber: string; wins: number; cpv: string[]; nuts: string[] }
}

export function suggestWorkspaceProfile(signal?: AbortSignal): Promise<WorkspaceSuggestion> {
  return post<WorkspaceSuggestion>('/api/v1/anbud/workspace/profile/suggest', {}, signal)
}

/** When the organisation last marked its tenders as seen, or null when it never has. */
export function readWorkspaceSeen(signal?: AbortSignal): Promise<string | null> {
  return post<{ seenAt: string | null }>('/api/v1/anbud/workspace/seen/read', {}, signal).then((data) => data.seenAt)
}

/** Marks the organisation's tenders as seen now; answers the instant it recorded. */
export function markWorkspaceSeen(signal?: AbortSignal): Promise<string> {
  return post<{ seenAt: string }>('/api/v1/anbud/workspace/seen/write', {}, signal).then((data) => data.seenAt)
}
