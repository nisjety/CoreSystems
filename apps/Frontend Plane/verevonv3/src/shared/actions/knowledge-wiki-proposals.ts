import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// The client action contract for the wiki proposals a person reviews in Knowledge (W5-094-b, F-t3-094). An agent
// files a proposal to edit a wiki page, or to create one that does not exist yet (the Data Plane's wiki-store,
// behind WIKI_CREATE_PAGE_PROPOSALS_ENABLED). A person accepts or rejects it. Accepting an edit saves its new
// version, accepting a new page creates it, and rejecting writes nothing. When a live page already holds the path
// of a new page at accept time, the proposal becomes a `conflict` and nothing is written.
//
// `wiki.proposal.review` is a human click, so it needs no approval of its own: the click is the approval. It is not
// in the generic action registry (action-registry.ts), so no model can execute it. The gateway takes the reviewer
// from the session and never from the body, and the Data Plane checks `wiki.approve` and the zero-retention posture
// before it writes a page. The list is read-only.

/** The statuses a proposal can hold: waiting, decided either way, or a create whose path was taken meanwhile. */
export const wikiProposalStatuses = ['pending', 'accepted', 'rejected', 'conflict'] as const
export type WikiProposalStatus = (typeof wikiProposalStatuses)[number]

/** The kinds: an edit to a page that exists, or a new page at a path that does not. */
export const wikiProposalKinds = ['edit', 'create_page'] as const

const proposalId = z.string().trim().min(1).max(200)

// The Data Plane serves one to 200 rows a page and defaults to 50, so the contract takes the same range.
export const wikiProposalListInput = z
  .object({
    status: z.enum(wikiProposalStatuses).default('pending'),
    limit: z.number().int().min(1).max(200).default(50),
    offset: z.number().int().min(0).default(0),
  })
  .strict()

// The person's decision. There is no default: the person picks accept or reject.
export const wikiProposalReviewInput = z
  .object({
    proposalId,
    decision: z.enum(['accept', 'reject']),
  })
  .strict()

// One proposal as the Data Plane returns it. Fields the view does not read are left out, and zod strips them.
const wikiProposalWire = z.object({
  proposal_id: z.string().min(1),
  page_id: z.string().default(''),
  proposal_kind: z.enum(wikiProposalKinds).default('edit'),
  proposed_path: z.string().nullable().optional(),
  proposed_title: z.string().nullable().optional(),
  proposed_content: z.string().default(''),
  edit_reason: z.string().nullable().optional(),
  proposed_by_agent: z.string().nullable().optional(),
  source_refs: z.array(z.unknown()).nullable().optional(),
  proposal_status: z.enum(wikiProposalStatuses),
  reviewed_by: z.string().nullable().optional(),
  created_at: z.string().default(''),
})

export type WikiProposalWire = z.infer<typeof wikiProposalWire>

// A Go nil slice answers `null`, so an empty list may arrive as null as well as [].
export const wikiProposalListOutput = z.object({
  proposals: z.array(wikiProposalWire).nullable().optional(),
  total: z.number().int().min(0),
})

export const wikiProposalReviewOutput = z.object({
  proposal: wikiProposalWire,
  new_version: z.record(z.string(), z.unknown()).nullable().optional(),
})

export const knowledgeWikiProposalActions = [
  {
    id: 'wiki.proposal.review',
    label: 'Accept or reject a wiki proposal',
    description:
      'Accept or reject a wiki proposal an agent filed. Accepting an edit saves its new version. Accepting a new page creates it, unless a live page already holds its path: then the proposal is marked as a conflict and nothing is written. Rejecting writes nothing.',
    ownerPlane: 'data',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: wikiProposalReviewInput,
    outputSchema: wikiProposalReviewOutput,
  },
] as const satisfies readonly ActionDescriptor[]

/**
 * Raised when a wiki proposal request is refused by this contract. Nothing is sent when it is thrown. `action`
 * names the operation and never carries the input, so a refusal can be logged without the person's data.
 */
export class WikiProposalContractError extends Error {
  readonly code = 'wiki_proposal_contract_refused'
  readonly action: string

  constructor(action: string) {
    super(`The wiki proposal ${action} request is refused by its action contract.`)
    this.name = 'WikiProposalContractError'
    this.action = action
  }
}

/** Parses a list request through the contract, or throws a typed refusal. Missing fields take their defaults. */
export function parseWikiProposalList(input: unknown): z.infer<typeof wikiProposalListInput> {
  const parsed = wikiProposalListInput.safeParse(input ?? {})
  if (!parsed.success) throw new WikiProposalContractError('list')
  return parsed.data
}

/** Parses a review request through the contract, or throws a typed refusal. */
export function parseWikiProposalReview(input: unknown): z.infer<typeof wikiProposalReviewInput> {
  const parsed = wikiProposalReviewInput.safeParse(input)
  if (!parsed.success) throw new WikiProposalContractError('review')
  return parsed.data
}
