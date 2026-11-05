import { z } from 'zod'

// The owner's conversation export as the gateway writes it (`verevon.thread-export.v1`, EP-12 S10). The replay
// shows what these fields carry and nothing else. A document with another format name or version is refused
// before its contents are read: this build never shows a later version as if it were v1 (EP-12 D9).

export const THREAD_EXPORT_FORMAT = 'verevon.thread-export.v1'
export const THREAD_EXPORT_VERSION = 1

const threadSchema = z.object({
  id: z.string(),
  title: z.string(),
  created_at: z.string(),
})

const messageSchema = z.object({
  id: z.string(),
  role: z.string(),
  content: z.string(),
  created_at: z.string(),
  // What the stored turn carried. The replay reads an artifact's kind and title and nothing more, so the
  // entries are not checked further here.
  artifacts: z.array(z.unknown()),
  citations: z.array(z.unknown()),
  metadata: z.record(z.string(), z.unknown()),
})

const runSchema = z.object({
  id: z.string(),
  status: z.string(),
  origin: z.string(),
  model: z.string(),
  input_tokens: z.number(),
  output_tokens: z.number(),
  created_at: z.string(),
})

const threadExportSchema = z.object({
  format: z.literal(THREAD_EXPORT_FORMAT),
  version: z.literal(THREAD_EXPORT_VERSION),
  exported_at: z.string(),
  profile: z.enum(['full', 'redacted']),
  thread: threadSchema,
  messages: z.array(messageSchema),
  runs: z.array(runSchema),
  truncated: z.boolean(),
})

export type ThreadExport = z.infer<typeof threadExportSchema>
export type ThreadExportMessage = ThreadExport['messages'][number]
export type ThreadExportRun = ThreadExport['runs'][number]

/** How one export file reads: the document, or why it is not shown. Neither failure shows any of its contents. */
export type ThreadExportReading =
  | { status: 'ok'; document: ThreadExport }
  | { status: 'unsupported' }
  | { status: 'invalid' }

export function readThreadExport(text: string): ThreadExportReading {
  let raw: unknown
  try {
    raw = JSON.parse(text)
  } catch {
    return { status: 'invalid' }
  }
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return { status: 'invalid' }
  const { format, version } = raw as { format?: unknown; version?: unknown }
  if (format !== THREAD_EXPORT_FORMAT || version !== THREAD_EXPORT_VERSION) return { status: 'unsupported' }
  const parsed = threadExportSchema.safeParse(raw)
  return parsed.success ? { status: 'ok', document: parsed.data } : { status: 'invalid' }
}
