import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// EP-8 S7: the Space memory tab. `spaces.memory.list` reads what a shared room has learned, each row
// marked `withheld` when the room's current audience does not recall it. `spaces.memory.delete` removes
// one row for every member. Control decides who may do either: it issues a `model.space.memory.manage`
// decision to the room's owner or a manager only, and refuses anyone else. The gateway is
// apps/gateway/src/domains/spaces/memory.rs, the dispatcher dispatchers_spaces/ep08.rs. Neither id is
// model-executable (model-access.ts closes both): an agent must never read or erase a room's memory.

const spaceMemoryListInput = z.object({
  spaceRef: z.string().trim().min(1).max(200),
})

const spaceMemoryRow = z.object({
  memory_id: z.string(),
  content: z.string(),
  updated_at: z.string(),
  provenance: z.string(),
  source_label: z.string(),
  thread_id: z.string(),
  withheld: z.boolean(),
})

const spaceMemoryListOutput = z.object({
  memories: z.array(spaceMemoryRow),
  withheld: z.number().int().min(0),
})

const spaceMemoryDeleteInput = z.object({
  spaceRef: z.string().trim().min(1).max(200),
  memoryId: z
    .string()
    .trim()
    .min(1)
    .max(64)
    .regex(/^[A-Za-z0-9_-]+$/),
})

const spaceMemoryDeleteOutput = z.object({
  memory_id: z.string(),
  deleted: z.boolean(),
})

export const ep08SpaceActions = [
  {
    id: 'spaces.memory.list',
    label: 'List Space memory',
    description:
      'List what a shared room has learned, for the room\'s owner or a manager only. A note the room\'s current members do not recall is marked withheld. Read only: the notes are quoted context and grant nothing.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceMemoryListInput,
    outputSchema: spaceMemoryListOutput,
  },
  {
    id: 'spaces.memory.delete',
    label: 'Delete Space memory',
    description:
      'Delete one note a shared room has learned. Only the room\'s owner or a manager may delete it. The room stops remembering it for every member, and it cannot be undone.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceMemoryDeleteInput,
    outputSchema: spaceMemoryDeleteOutput,
  },
] as const satisfies readonly ActionDescriptor[]
