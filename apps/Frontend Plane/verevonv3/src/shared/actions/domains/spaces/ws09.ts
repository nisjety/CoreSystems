import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS09-S8: a room manager or owner stops another member's live agent run. The gateway is
// apps/gateway/src/domains/spaces/run_live.rs (`stop_run_core`), the dispatcher
// dispatchers_spaces/ws09.rs. Authority is Control's `model.run.control` decision (role manager or
// owner, the organisation's `run_control_entitled`, current audience), verified again by Session
// Core. Deliberately NOT model-executable (model-access.ts closes it): a model stopping a human's
// run is an authority inversion. Refused typed for a Zero Data Retention organisation
// (`zdr_durable_mutation_forbidden`), 403 `space_run_control_not_authorized` for anyone else.

const spaceStopRunInput = z.object({
  spaceRef: z.string().trim().min(1).max(200),
  runId: z.string().trim().min(1).max(64),
  idempotencyKey: z.string().trim().min(1).max(128).optional(),
})

const spaceStopRunOutput = z.object({
  run_id: z.string(),
  status: z.enum(['cancelled', 'already_terminal']),
  cancelled: z.boolean(),
  receipt_id: z.string(),
})

export const ws09SpaceActions = [
  {
    id: 'spaces.stop_run',
    label: 'Stop a member\'s agent run',
    description:
      'Stop an agent run that another member started in this room. Only a room manager or owner whose organisation allows it can do this. The run ends where it is and the room sees that it was stopped.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceStopRunInput,
    outputSchema: spaceStopRunOutput,
  },
] as const satisfies readonly ActionDescriptor[]
