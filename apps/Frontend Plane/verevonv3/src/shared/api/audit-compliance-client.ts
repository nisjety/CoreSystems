import { apiErrorFromResponse } from './http'
import { gatewayBaseUrl } from './config'

/**
 * Audit compliance client (WS06-S6).
 *
 * Owner/admin surface over audit-core's compliance API and org-core's SIEM
 * destinations, through the gateway. The gateway takes the org from the session
 * and refuses anyone but an owner or admin, so nothing here names an org and a
 * member sees a typed 403 `ApiError`.
 *
 * Human sessions only: no action here is model-eligible, an agent never reads or
 * administers the audit log. Hashes are lowercase hex as audit-core serves them.
 */

export interface ComplianceEvent {
  org_id: string
  chain_seq: number
  occurred_at: string
  ingested_at: string
  user_id: string
  actor_role: string
  plane: string
  event: string
  subject: string
  resource_id: string
  outcome: string
  event_id: string
  request_id: string
  ip_address: string
  user_agent: string
  source_producer: string
  source_bus: string
  payload_hash: string
  details: Record<string, unknown> | null
  chain_version: number
  prev_hash: string
  row_hash: string
}

export interface ComplianceEventsQuery {
  event?: string
  userId?: string
  plane?: string
  outcome?: string
  since?: string
  until?: string
  limit?: number
  cursor?: string
}

export interface ComplianceEventsPage {
  events: ComplianceEvent[]
  nextCursor: string | null
  limit: number
  headSeq: number
}

export interface ComplianceCheckpoint {
  through_seq: number
  through_hash: string
  pruned_rows_total: number
  key_id: string
  signature: string
  created_at: string
}

export interface ComplianceAttestation {
  chain_seq: number
  row_hash: string
  key_id: string
  signature: string
  signed_at: string
}

export interface ComplianceManifest {
  org_id: string
  generated_at: string
  first_seq: number
  last_seq: number
  count: number
  first_prev_hash: string
  last_row_hash: string
  head_seq: number
  head_hash: string
  legacy_unchained_rows: number
  checkpoint: ComplianceCheckpoint | null
  attestation: ComplianceAttestation | null
  key_id?: string
  signature?: string
}

export interface ComplianceKey {
  key_id: string
  algorithm: string
  /** base64url without padding; absent when the server does not hold the key. */
  public_key?: string
  status: string
}

export type ChainVerifyStatus =
  | 'verified'
  | 'verified_with_legacy'
  | 'broken'
  | 'partial'
  | 'key_unavailable'

export interface ChainVerifyReport {
  org_id: string
  status: ChainVerifyStatus
  reason?: string
  first_broken_seq?: number
  from_seq: number
  through_seq: number
  through_hash?: string
  rows_verified: number
  /** False when the run hit its row cap: resume from through_seq/through_hash. */
  complete: boolean
  legacy_unchained_rows: number
  checkpoint?: { through_seq: number; pruned_rows_total: number; key_id: string; verified: boolean }
  head?: { seq: number; hash: string }
  attested?: { seq: number; key_id: string; signed_at: string; verified: boolean }
}

export interface ChainVerifyRequest {
  fromSeq?: number
  /** Hex hash of the row at `fromSeq`, from a previous capped run's `through_hash`. */
  fromHash?: string
  maxRows?: number
}

export interface ExportStatus {
  exporter_enabled: boolean
  destinations: unknown[]
}

export type SiemDestinationKind = 'ndjson_webhook' | 'splunk_hec'

export interface SiemEventFilter {
  events?: string[]
  planes?: string[]
}

/** A destination as org-core returns it: the secret is write-only, `secret_set` only says one exists. */
export interface SiemDestination {
  id: string
  org_id: string
  kind: SiemDestinationKind
  url: string
  secret_set: boolean
  enabled: boolean
  event_filter: SiemEventFilter
  revision: number
  created_by: string
  created_at: string
  updated_at: string
}

export interface CreateSiemDestination {
  kind: SiemDestinationKind
  url: string
  secret: string
  enabled?: boolean
  eventFilter?: SiemEventFilter
}

export interface UpdateSiemDestination {
  expectedRevision: number
  url?: string
  secret?: string
  enabled?: boolean
  eventFilter?: SiemEventFilter
}

const COMPLIANCE = '/api/v1/audit/compliance'
const DESTINATIONS = '/api/v1/audit/siem-destinations'

type Envelope<T> = { data?: T; meta?: Record<string, unknown> | null; error?: unknown }

/**
 * `requestJson` unwraps `{ data }` and drops `meta`, but the event page needs
 * `meta.next_cursor` and `meta.head_seq`, so the envelope is read whole here.
 * Failures are the same typed `ApiError` every other client throws.
 */
async function requestEnvelope<T>(path: string, init?: RequestInit): Promise<Envelope<T>> {
  const headers = new Headers(init?.headers)
  headers.set('Accept', 'application/json')
  if (init?.body && !headers.has('Content-Type')) headers.set('Content-Type', 'application/json')

  const response = await fetch(`${gatewayBaseUrl()}${path}`, {
    ...init,
    credentials: 'include',
    headers,
  })
  const body = (await response.json().catch(() => null)) as Envelope<T> | null
  if (!response.ok) {
    throw apiErrorFromResponse(body, response)
  }
  return body ?? {}
}

function queryString(entries: Array<[string, string | number | undefined]>): string {
  const params = new URLSearchParams()
  for (const [name, value] of entries) {
    if (value !== undefined && value !== '') params.set(name, String(value))
  }
  const text = params.toString()
  return text ? `?${text}` : ''
}

function numberMeta(meta: Record<string, unknown> | null | undefined, key: string): number {
  const value = meta?.[key]
  return typeof value === 'number' && Number.isFinite(value) ? value : 0
}

export async function listComplianceEvents(
  query: ComplianceEventsQuery = {},
  signal?: AbortSignal,
): Promise<ComplianceEventsPage> {
  const envelope = await requestEnvelope<ComplianceEvent[]>(
    `${COMPLIANCE}/events${queryString([
      ['event', query.event],
      ['user_id', query.userId],
      ['plane', query.plane],
      ['outcome', query.outcome],
      ['since', query.since],
      ['until', query.until],
      ['limit', query.limit],
      ['cursor', query.cursor],
    ])}`,
    { signal },
  )
  const next = envelope.meta?.next_cursor
  return {
    events: Array.isArray(envelope.data) ? envelope.data : [],
    nextCursor: typeof next === 'string' && next ? next : null,
    limit: numberMeta(envelope.meta, 'limit'),
    headSeq: numberMeta(envelope.meta, 'head_seq'),
  }
}

/**
 * Follow `next_cursor` to the end of the range. A cursor is a keyset on the
 * chain position, so rows appended while this runs do not shift the pages.
 */
export async function* exportComplianceEvents(
  query: Omit<ComplianceEventsQuery, 'cursor'> = {},
  signal?: AbortSignal,
): AsyncGenerator<ComplianceEventsPage> {
  let cursor: string | undefined
  do {
    const page = await listComplianceEvents({ ...query, cursor }, signal)
    yield page
    cursor = page.nextCursor ?? undefined
  } while (cursor)
}

export async function getComplianceManifest(
  range: { since?: string; until?: string } = {},
  signal?: AbortSignal,
): Promise<ComplianceManifest> {
  const envelope = await requestEnvelope<ComplianceManifest>(
    `${COMPLIANCE}/manifest${queryString([['since', range.since], ['until', range.until]])}`,
    { signal },
  )
  return envelope.data as ComplianceManifest
}

export async function listComplianceKeys(signal?: AbortSignal): Promise<ComplianceKey[]> {
  const envelope = await requestEnvelope<ComplianceKey[]>(`${COMPLIANCE}/keys`, { signal })
  return Array.isArray(envelope.data) ? envelope.data : []
}

export async function getComplianceExportStatus(signal?: AbortSignal): Promise<ExportStatus> {
  const envelope = await requestEnvelope<ExportStatus>(`${COMPLIANCE}/export-status`, { signal })
  return envelope.data as ExportStatus
}

export async function verifyComplianceChain(
  request: ChainVerifyRequest = {},
  signal?: AbortSignal,
): Promise<ChainVerifyReport> {
  const body: Record<string, unknown> = {}
  if (request.fromSeq !== undefined) body.from_seq = request.fromSeq
  if (request.fromHash !== undefined) body.from_hash = request.fromHash
  if (request.maxRows !== undefined) body.max_rows = request.maxRows
  const envelope = await requestEnvelope<ChainVerifyReport>(`${COMPLIANCE}/verify`, {
    method: 'POST',
    body: JSON.stringify(body),
    signal,
  })
  return envelope.data as ChainVerifyReport
}

export async function listSiemDestinations(signal?: AbortSignal): Promise<SiemDestination[]> {
  const envelope = await requestEnvelope<SiemDestination[]>(DESTINATIONS, { signal })
  return Array.isArray(envelope.data) ? envelope.data : []
}

function filterBody(filter: SiemEventFilter | undefined): { event_filter?: SiemEventFilter } {
  return filter ? { event_filter: filter } : {}
}

export async function createSiemDestination(input: CreateSiemDestination): Promise<SiemDestination> {
  const envelope = await requestEnvelope<SiemDestination>(DESTINATIONS, {
    method: 'POST',
    body: JSON.stringify({
      kind: input.kind,
      url: input.url,
      secret: input.secret,
      ...(input.enabled !== undefined ? { enabled: input.enabled } : {}),
      ...filterBody(input.eventFilter),
    }),
  })
  return envelope.data as SiemDestination
}

export async function updateSiemDestination(
  destinationId: string,
  input: UpdateSiemDestination,
): Promise<SiemDestination> {
  const envelope = await requestEnvelope<SiemDestination>(
    `${DESTINATIONS}/${encodeURIComponent(destinationId)}`,
    {
      method: 'PATCH',
      body: JSON.stringify({
        expected_revision: input.expectedRevision,
        ...(input.url !== undefined ? { url: input.url } : {}),
        ...(input.secret !== undefined ? { secret: input.secret } : {}),
        ...(input.enabled !== undefined ? { enabled: input.enabled } : {}),
        ...filterBody(input.eventFilter),
      }),
    },
  )
  return envelope.data as SiemDestination
}

export async function deleteSiemDestination(destinationId: string): Promise<void> {
  await requestEnvelope<unknown>(`${DESTINATIONS}/${encodeURIComponent(destinationId)}`, {
    method: 'DELETE',
  })
}
