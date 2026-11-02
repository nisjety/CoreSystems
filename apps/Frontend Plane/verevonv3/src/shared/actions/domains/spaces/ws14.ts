import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS14-S7: room files. `space.files.attach` is human-only multipart (the SPA posts the bytes to
// POST /api/v1/spaces/{spaceRef}/files; a JSON dispatch answers 422 `upload_requires_multipart`),
// ownerPlane `ingestion` because imports-core accepts the upload. `list`, `read_text` and `delete`
// are Data Plane reads/writes under the caller's own Control decisions (apps/gateway/src/domains/
// spaces/files.rs, dispatchers_spaces/ws14.rs). `read_text` returns extracted text only (there are
// no original bytes) and `delete` is uploader-only. None is model-executable (model-access.ts).

const spaceFilesAttachInput = z.object({
  spaceRef: z.string().trim().min(1).max(200),
  idempotencyKey: z.string().trim().min(8).max(128),
})

const spaceFilesAttachOutput = z.object({
  upload_id: z.string(),
  deduplicated: z.boolean(),
  files: z.array(z.object({ file_name: z.string(), status: z.string() })),
})

const spaceFilesListInput = z.object({
  spaceRef: z.string().trim().min(1).max(200),
  limit: z.number().int().min(1).max(200).optional(),
  cursor: z.string().max(256).optional(),
})

const spaceFilesListOutput = z.object({
  files: z.array(
    z.object({
      document_id: z.string(),
      file_name: z.string().nullable(),
      mime_type: z.string().nullable(),
      size_bytes: z.number().nullable(),
      sha256: z.string().nullable(),
      extension: z.string().nullable(),
      status: z.string().nullable(),
      text_chars: z.number().nullable(),
      uploaded_by: z.string().nullable(),
      uploaded_at: z.string().nullable(),
    }),
  ),
  next_cursor: z.string().nullable(),
  unavailable: z.array(z.object({ section: z.string(), code: z.string(), reason: z.string() }).passthrough()),
})

const spaceFilesReadTextInput = z.object({
  spaceRef: z.string().trim().min(1).max(200),
  documentId: z.string().trim().min(1).max(200),
  maxChars: z.number().int().min(1).max(1_000_000).optional(),
})

const spaceFilesReadTextOutput = z.object({
  document_id: z.string(),
  file_name: z.string().nullable(),
  text: z.string(),
  text_chars: z.number().nullable(),
  truncated: z.boolean(),
})

const spaceFilesDeleteInput = z.object({
  spaceRef: z.string().trim().min(1).max(200),
  documentId: z.string().trim().min(1).max(200),
})

const spaceFilesDeleteOutput = z.object({
  deleted: z.boolean(),
  document_id: z.string(),
})

export const ws14SpaceActions = [
  {
    id: 'space.files.attach',
    label: 'Add files to room',
    description:
      'Add up to ten files to a room so every member can read their text. Only the extracted text is kept, never the original file. The upload is sent as multipart form data by the app, not through this action.',
    ownerPlane: 'ingestion',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceFilesAttachInput,
    outputSchema: spaceFilesAttachOutput,
  },
  {
    id: 'space.files.list',
    label: 'List room files',
    description: 'List the files kept in a room, newest first, with who added each one.',
    ownerPlane: 'data',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceFilesListInput,
    outputSchema: spaceFilesListOutput,
  },
  {
    id: 'space.files.read_text',
    label: 'Read room file text',
    description: 'Read the extracted text of one room file. Text only; the original file cannot be downloaded.',
    ownerPlane: 'data',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceFilesReadTextInput,
    outputSchema: spaceFilesReadTextOutput,
  },
  {
    id: 'space.files.delete',
    label: 'Remove room file',
    description:
      'Remove a file you added to a room. Only the person who added a file can remove it, and it cannot be undone.',
    ownerPlane: 'data',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceFilesDeleteInput,
    outputSchema: spaceFilesDeleteOutput,
  },
] as const satisfies readonly ActionDescriptor[]
