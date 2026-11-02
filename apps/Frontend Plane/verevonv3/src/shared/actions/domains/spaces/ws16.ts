import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS16-S8: the personal Spaces sidebar layout. Application (convex-core `spaceSidebar`) stores
// one small document of references per member, compare-and-set on a revision; the gateway
// (apps/gateway/src/domains/spaces/sidebar.rs, dispatchers_spaces/ws16.rs) validates the
// envelope and the body cap and holds no state. It is human-only (model-access.ts closes it):
// it is one person's own layout. A Zero Data Retention organization is refused with the typed
// `zdr_durable_mutation_forbidden`; the client then keeps the layout in this browser only.
//
// The document's own caps (starred 100, hidden 500, sections 30, 200 refs each, names 40
// characters, 32 KiB) are enforced in Application; the full zod document schema and the
// hash-pinned limits golden arrive with WS16-S9. Until then the gateway passes the object through
// and a bad one answers 400 `invalid_sidebar_doc` with the reason.

const spaceUpdateSidebarInput = z.object({
  // The revision the client last saw; 0 creates the first one. A stale one answers 409
  // `space_sidebar_conflict` carrying the stored revision and document to rebase on.
  baseRevision: z.number().int().min(0),
  doc: z.record(z.string(), z.unknown()),
})

const spaceUpdateSidebarOutput = z.object({
  revision: z.number().int().min(1),
  doc: z.record(z.string(), z.unknown()),
})

export const ws16SpaceActions = [
  {
    id: 'spaces.update_sidebar',
    label: 'Save Spaces sidebar layout',
    description:
      'Save your own Spaces sidebar layout: starred rooms, hidden rooms and named sections. It holds room references only, changes no access, and a stale base revision is refused with the stored layout so you can merge.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceUpdateSidebarInput,
    outputSchema: spaceUpdateSidebarOutput,
  },
] as const satisfies readonly ActionDescriptor[]
