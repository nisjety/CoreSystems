import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS19-S4: the room canvas, two human-only contracts. The gateway routes are
// apps/gateway/src/domains/spaces/canvas.rs (dispatchers in dispatchers_spaces/ws19.rs). Control decides
// each write and Data Plane (wiki-store) stores it, so the inputs below are the REST bodies with action
// keys. Neither action is model-executable (model-access.ts closes both): an agent changes the canvas only
// through its own run-action tool under its own decision (WS19-S9). The output carries version metadata
// and ids, never the canvas text.

const canvasVersionId = z
  .string()
  .min(1)
  .max(128)
  .regex(/^[A-Za-z0-9_-]+$/, 'versionId must be a version id')

const clientSaveId = z
  .string()
  .regex(/^[A-Za-z0-9_-]{16,64}$/, 'clientSaveId must be 16 to 64 letters, digits, "-" or "_"')

const canvasText = z
  .string()
  .min(1)
  .refine(
    (text) => new TextEncoder().encode(text).length <= 65536,
    'content is larger than the 64 KiB canvas limit',
  )
  .refine((text) => !text.includes('\u0000'), 'content must not contain a NUL byte')

const spacesSaveCanvasInput = z.object({
  spaceRef: z.string().trim().min(1),
  expectedVersionId: canvasVersionId.nullable(),
  content: canvasText,
  summary: z.string().trim().max(200).optional(),
  clientSaveId,
})

const spacesRestoreCanvasVersionInput = z.object({
  spaceRef: z.string().trim().min(1),
  versionId: canvasVersionId,
  expectedVersionId: canvasVersionId.nullable(),
  clientSaveId,
})

const canvasVersionOutput = z.object({
  version_id: z.string(),
  version_no: z.number().int(),
  parent_version_id: z.string().nullable(),
  author_kind: z.enum(['user', 'agent']),
  author_id: z.string(),
  on_behalf_of: z.string().nullable(),
  run_id: z.string().nullable(),
  change_kind: z.enum(['create', 'edit', 'restore']),
  restored_from_version_id: z.string().nullable(),
  summary: z.string().nullable(),
  content_bytes: z.number().int(),
  content_sha256: z.string(),
  created_at: z.string(),
})

const canvasWriteOutput = z.union([
  z.object({ canvas_id: z.string(), version: canvasVersionOutput, deduplicated: z.boolean() }),
  z.object({ canvas_id: z.string(), head_version_id: z.string(), unchanged: z.literal(true) }),
])

export const ws19SpaceActions = [
  {
    id: 'spaces.save_canvas',
    label: 'Save room canvas',
    description:
      'Save a new version of the room canvas, the shared document of this room. A save based on an older version than the latest is refused, so reload the latest version before you save again.',
    ownerPlane: 'data',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spacesSaveCanvasInput,
    outputSchema: canvasWriteOutput,
  },
  {
    id: 'spaces.restore_canvas_version',
    label: 'Restore room canvas version',
    description:
      'Make an earlier version of the room canvas the current one. The restore is a new version, so nothing in the history is removed.',
    ownerPlane: 'data',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spacesRestoreCanvasVersionInput,
    outputSchema: canvasWriteOutput,
  },
] as const satisfies readonly ActionDescriptor[]
