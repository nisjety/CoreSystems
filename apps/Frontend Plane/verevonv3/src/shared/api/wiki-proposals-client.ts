import { z } from 'zod'
import {
  parseWikiProposalList,
  parseWikiProposalReview,
  wikiProposalListOutput,
  wikiProposalReviewOutput,
  type WikiProposalStatus,
  type WikiProposalWire,
} from '@/shared/actions/knowledge-wiki-proposals'
import { ApiError, requestJson } from '@/shared/api/http'

// The wiki proposal review in Knowledge (W5-094-b, F-t3-094). The gateway's /api/v1/knowledge/wiki/proposals routes
// proxy the Data Plane's wiki-store, which answers 404 to the list while WIKI_CREATE_PAGE_PROPOSALS_ENABLED is off.
// The gateway takes the org and the reviewer from the session; nothing here names either.

/** Whether this build offers the wiki proposal review (`VITE_WIKI_PROPOSALS_REVIEW`, read with the strict literal `true`). */
export const wikiProposalsReviewEnabled = () => import.meta.env.VITE_WIKI_PROPOSALS_REVIEW === 'true'

/** One proposal as the person reviews it. An edit names the page it changes; a new page names its path and title. */
export type WikiProposal = {
  id: string
  kind: 'edit' | 'create_page'
  status: WikiProposalStatus
  /** The page an edit changes. Empty for a new page. */
  pageId: string
  /** The path and title a new page would take. Null for an edit. */
  proposedPath: string | null
  proposedTitle: string | null
  proposedContent: string
  editReason: string | null
  proposedByAgent: string | null
  sourceRefs: string[]
  reviewedBy: string | null
  createdAt: string
}

/** A live page's id, title and path, as the page list names it. */
export type WikiPageSummary = {
  id: string
  title: string
  path: string
}

/** A live page and the text of its current version: the "before" side of an edit's diff. */
export type WikiPageContent = WikiPageSummary & {
  content: string
}

const pageSummaryWire = z.object({
  page_id: z.string().min(1),
  title: z.string().default(''),
  path: z.string().default(''),
})

// The Data Plane answers `null` for an empty page list, as it does for a nil slice.
const pageListWire = z.object({
  pages: z.array(pageSummaryWire).nullable().optional(),
})

const pageReadWire = z.object({
  page: pageSummaryWire,
  version: z
    .object({
      content: z.string().nullable().optional(),
    })
    .nullable()
    .optional(),
})

function malformed(): ApiError {
  return new ApiError('The wiki proposal answer could not be read.', 502, 'wiki_proposal_failed')
}

function sourceRefsFrom(value: unknown[] | null | undefined): string[] {
  return (value ?? []).filter((ref): ref is string => typeof ref === 'string' && ref.trim().length > 0)
}

function proposalFrom(wire: WikiProposalWire): WikiProposal {
  return {
    id: wire.proposal_id,
    kind: wire.proposal_kind,
    status: wire.proposal_status,
    pageId: wire.page_id,
    proposedPath: wire.proposed_path ?? null,
    proposedTitle: wire.proposed_title ?? null,
    proposedContent: wire.proposed_content,
    editReason: wire.edit_reason ?? null,
    proposedByAgent: wire.proposed_by_agent ?? null,
    sourceRefs: sourceRefsFrom(wire.source_refs),
    reviewedBy: wire.reviewed_by ?? null,
    createdAt: wire.created_at,
  }
}

/**
 * The org's proposals with one status, newest first. The status defaults to pending. Rejects with the gateway's
 * typed error: a 404 means the Data Plane has not switched proposals on for this deployment.
 */
export async function listWikiProposals(
  input: { status?: WikiProposalStatus; limit?: number; offset?: number } = {},
  signal?: AbortSignal,
): Promise<{ proposals: WikiProposal[]; total: number }> {
  const request = parseWikiProposalList(input)
  const query = new URLSearchParams({
    status: request.status,
    limit: String(request.limit),
    offset: String(request.offset),
  })
  const raw = await requestJson<unknown>(`/api/v1/knowledge/wiki/proposals?${query.toString()}`, { signal })
  const parsed = wikiProposalListOutput.safeParse(raw)
  if (!parsed.success) throw malformed()
  return { proposals: (parsed.data.proposals ?? []).map(proposalFrom), total: parsed.data.total }
}

/**
 * Accept or reject one proposal. Resolves to the proposal as the Data Plane now holds it. A conflict rejects with
 * the typed `wiki_proposal_conflict` (nothing was written); any other 409 means the proposal was already decided.
 */
export async function reviewWikiProposal(
  proposalId: string,
  decision: 'accept' | 'reject',
  signal?: AbortSignal,
): Promise<WikiProposal> {
  const request = parseWikiProposalReview({ proposalId, decision })
  const raw = await requestJson<unknown>(
    `/api/v1/knowledge/wiki/proposals/${encodeURIComponent(request.proposalId)}/review`,
    {
      method: 'POST',
      body: JSON.stringify({ decision: request.decision }),
      signal,
    },
  )
  const parsed = wikiProposalReviewOutput.safeParse(raw)
  if (!parsed.success) throw malformed()
  return proposalFrom(parsed.data.proposal)
}

/** The org's live pages by id, newest first, 200 at most: enough to name the page each edit changes. */
export async function listWikiPageSummaries(signal?: AbortSignal): Promise<Map<string, WikiPageSummary>> {
  const raw = await requestJson<unknown>('/api/v1/knowledge/wiki/pages?limit=200', { signal })
  const parsed = pageListWire.safeParse(raw)
  if (!parsed.success) throw malformed()
  return new Map(
    (parsed.data.pages ?? []).map((page): [string, WikiPageSummary] => [
      page.page_id,
      { id: page.page_id, title: page.title, path: page.path },
    ]),
  )
}

/** One live page and the text of its current version, for the diff a person reads before an edit is accepted. */
export async function loadWikiPageContent(pageId: string, signal?: AbortSignal): Promise<WikiPageContent> {
  const raw = await requestJson<unknown>(`/api/v1/knowledge/wiki/pages/${encodeURIComponent(pageId)}`, { signal })
  const parsed = pageReadWire.safeParse(raw)
  if (!parsed.success) throw malformed()
  return {
    id: pageId,
    title: parsed.data.page.title,
    path: parsed.data.page.path,
    content: parsed.data.version?.content ?? '',
  }
}
