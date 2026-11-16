import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS20-S5: the room browser run start, a human-only contract. The REST route is R1,
// `POST /api/v1/spaces/{ref}/browser-runs` (apps/gateway/src/domains/spaces/browser_runs.rs); the dispatcher is
// dispatchers_spaces/ws20.rs. Model-access.ts closes this id: an agent starts no room browser run in v1. The output is
// the receipt the start answers, ids and the view scope only; page content, steps and screenshots are never in it.

const clientSendId = z
  .string()
  .regex(/^[A-Za-z0-9_-]{16,64}$/, 'clientSendId must be 16 to 64 letters, digits, "-" or "_"')

const spacesBrowserRunStartInput = z.object({
  spaceRef: z.string().trim().min(1),
  // A new thread cannot be the start's thread in v1: the goal's append decision binds an existing thread.
  threadId: z.string().trim().min(1).max(128),
  goal: z.string().trim().min(1).max(2000),
  allowedDomains: z.array(z.string().trim().min(1).max(253)).min(1).max(8),
  startUrl: z.string().trim().max(2048).optional(),
  maxSteps: z.number().int().min(5).max(40).optional(),
  maxRuntimeSeconds: z.number().int().min(30).max(600).optional(),
  // The live view is the starter's alone in v1 (SD-23); the room never receives a share.
  shareView: z.literal(false).optional(),
  clientSendId,
})

const browserRunStartOutput = z.object({
  run_id: z.string(),
  thread_id: z.string(),
  plan_id: z.string(),
  view: z.object({ scope: z.literal('owner') }),
})

export const ws20SpaceActions = [
  {
    id: 'spaces.browser_run.start',
    label: 'Start room browser run',
    description:
      'Start a browser run on one thread of this room, limited to the domains you name. The room sees the steps and the hosts; only the person who started the run sees the live view and the screenshots.',
    ownerPlane: 'model',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: spacesBrowserRunStartInput,
    outputSchema: browserRunStartOutput,
  },
] as const satisfies readonly ActionDescriptor[]
