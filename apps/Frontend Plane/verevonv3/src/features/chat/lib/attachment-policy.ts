import { getAttachmentPolicy, type AttachmentPolicy } from '@/shared/api/chat-client'
import { en } from '@/shared/i18n/en'
import { no } from '@/shared/i18n/no'

/**
 * What the send path can really deliver, decided before anything is sent.
 *
 * A file the model will not receive used to look sent: the composer showed a
 * chip, the transcript said "1 attachment added", and the image was dropped on
 * the subscription route, ignored after the first one, or refused by the server
 * only once the turn had started. This module says no first, per file, with a
 * reason the composer can word. Documents are different: their text is read in
 * the browser and travels inside the message, so only the limits below apply to
 * them. Images are the one kind that rides the wire as an attachment, so the
 * route, mode, count and zero-retention gates are theirs.
 *
 * The numbers mirror what the stack does today, in this one place. S2 makes the
 * server declare them; until then they are the constants every caller shares.
 */

/** Images the model can read. Anything else under `image/` fails at the provider. */
export const ATTACHMENT_IMAGE_MIME_TYPES: readonly string[] = ['image/png', 'image/jpeg', 'image/webp', 'image/gif']

/** Documents whose text is read in the browser (`prepare-chat-attachments.ts`). */
export const ATTACHMENT_DOCUMENT_EXTENSIONS: readonly string[] = [
  '.pdf', '.docx', '.xlsx', '.pptx', '.txt', '.md', '.markdown', '.csv', '.json', '.html', '.htm',
]

/** `prepare-chat-attachments.ts` refuses a file over this before reading it. */
export const ATTACHMENT_MAX_FILE_BYTES = 1_000_000
/** Today's model-gateway analyses the first image only; a second was silently ignored. */
export const ATTACHMENT_MAX_IMAGES = 1
/** The server default S2 declares; there is no per-turn byte total on the client today. */
export const ATTACHMENT_MAX_TOTAL_BYTES = 3_145_728

/**
 * Whether images are read by the picked model instead of the vision-only diversion
 * (the server's `CHAT_VISION_AS_PARTS_ENABLED`). The server's declared
 * `modes.visionAsParts` decides once a policy is loaded, so the client follows the
 * server it talks to; with no policy loaded, the build flag
 * `VITE_CHAT_VISION_AS_PARTS_ENABLED` decides. Off, the diversion serves any model and
 * none of the model-vision gates below apply.
 */
export function visionAsPartsEnabled(
  policy: Pick<AttachmentPolicy, 'modes'> | undefined = getAttachmentPolicy(),
): boolean {
  if (policy) return policy.modes.visionAsParts === true
  return import.meta.env.VITE_CHAT_VISION_AS_PARTS_ENABLED === 'true'
}

/**
 * Whether the server carries a turn's attachments through a Do run (its
 * `CHAT_DO_ATTACHMENTS_ENABLED`). The declared `modes.doAttachments` decides once a
 * policy is loaded, and the build flag `VITE_CHAT_DO_ATTACHMENTS_ENABLED` otherwise.
 * Off, a Do turn refuses images as it always did. A Do run reads an image with the
 * picked model, so the same model-vision gates apply.
 */
export function doAttachmentsEnabled(
  policy: Pick<AttachmentPolicy, 'modes'> | undefined = getAttachmentPolicy(),
): boolean {
  if (policy) return policy.modes.doAttachments === true
  return import.meta.env.VITE_CHAT_DO_ATTACHMENTS_ENABLED === 'true'
}

/** The `accept` value of the file input, so the picker and the policy cannot drift. */
export const ATTACHMENT_ACCEPT = [...ATTACHMENT_IMAGE_MIME_TYPES, ...ATTACHMENT_DOCUMENT_EXTENSIONS].join(',')

export type AttachmentRefusalReason =
  | 'unsupported_type'
  | 'too_large'
  | 'route_unsupported'
  | 'do_images_unsupported'
  | 'too_many_images'
  | 'zdr_refused'
  | 'model_no_vision'

/**
 * How images are read on this turn, present only while images ride as parts of the
 * turn: the picked model is then the reader, and the server's declared limits apply.
 */
export type ImagePartsContext = {
  /** `modelSupportsVision` for the picked model; `false` while the catalogue has not loaded. */
  modelReadsImages: boolean
  /** One catalogue model that reads images, named in the refusal. */
  visionModelName?: string
  /** `attachment_policy.max_images`. */
  maxImages: number
}

export type AttachmentPolicyContext = {
  /** The turn goes through a connected ChatGPT subscription, which carries no attachments. */
  subscriptionBacked: boolean
  /** The turn is a Do turn; its agent loop sees an image only when `doAttachments` is set. */
  planMode: boolean
  /** The server carries attachments through a Do run (`doAttachmentsEnabled`). Absent: it does not. */
  doAttachments?: boolean
  /** Temporary chat: the server refuses attachments under zero data retention. */
  zdr: boolean
  currentImageCount: number
  currentTotalBytes: number
  /** Absent: images go through today's vision diversion, which any model serves. */
  imageParts?: ImagePartsContext
}

export type AttachmentFileLike = { name: string; size: number; type: string }

export type AttachmentVerdict = { ok: true } | { ok: false; reason: AttachmentRefusalReason }

export function isImageAttachment(file: AttachmentFileLike): boolean {
  return ATTACHMENT_IMAGE_MIME_TYPES.includes(file.type.toLowerCase())
}

function isDocument(file: AttachmentFileLike): boolean {
  const name = file.name.toLowerCase()
  // The extension is trusted over the MIME type, as the picker's own accept list
  // is: browsers report `.md` and `.csv` as `application/octet-stream` or not at all.
  return ATTACHMENT_DOCUMENT_EXTENSIONS.some((extension) => name.endsWith(extension))
}

export function evaluateAttachment(file: AttachmentFileLike, ctx: AttachmentPolicyContext): AttachmentVerdict {
  const image = isImageAttachment(file)
  if (!image && !isDocument(file)) return { ok: false, reason: 'unsupported_type' }
  if (file.size > ATTACHMENT_MAX_FILE_BYTES || ctx.currentTotalBytes + file.size > ATTACHMENT_MAX_TOTAL_BYTES) {
    return { ok: false, reason: 'too_large' }
  }
  if (!image) return { ok: true }
  // Zero retention first: it is the refusal that holds whatever else is chosen.
  if (ctx.zdr) return { ok: false, reason: 'zdr_refused' }
  if (ctx.subscriptionBacked) return { ok: false, reason: 'route_unsupported' }
  // A Do run reads the image with the picked model, so it needs the model-vision context
  // too: without it the answer is the old refusal, never an image the run cannot read.
  if (ctx.planMode && (!ctx.doAttachments || !ctx.imageParts)) return { ok: false, reason: 'do_images_unsupported' }
  if (ctx.imageParts && !ctx.imageParts.modelReadsImages) return { ok: false, reason: 'model_no_vision' }
  if (ctx.currentImageCount >= (ctx.imageParts?.maxImages ?? ATTACHMENT_MAX_IMAGES)) {
    return { ok: false, reason: 'too_many_images' }
  }
  return { ok: true }
}

export type AttachmentRefusal = {
  file: AttachmentFileLike
  reason: AttachmentRefusalReason
  /** For `model_no_vision`: one model that would read the image. */
  visionModelName?: string
  /** For `too_many_images`: the limit that applied to the turn. */
  maxImages?: number
}

/**
 * Evaluates files one after the other, each counted against the ones accepted
 * before it, so "the second image" is refused by name and not silently ignored.
 */
export function evaluateAttachments<T extends AttachmentFileLike>(
  files: readonly T[],
  ctx: AttachmentPolicyContext,
): { accepted: T[]; refused: AttachmentRefusal[] } {
  const accepted: T[] = []
  const refused: AttachmentRefusal[] = []
  let imageCount = ctx.currentImageCount
  let totalBytes = ctx.currentTotalBytes
  for (const file of files) {
    const verdict = evaluateAttachment(file, { ...ctx, currentImageCount: imageCount, currentTotalBytes: totalBytes })
    if (!verdict.ok) {
      refused.push({
        file,
        reason: verdict.reason,
        ...(verdict.reason === 'model_no_vision' && ctx.imageParts?.visionModelName
          ? { visionModelName: ctx.imageParts.visionModelName }
          : {}),
        ...(verdict.reason === 'too_many_images' && ctx.imageParts ? { maxImages: ctx.imageParts.maxImages } : {}),
      })
      continue
    }
    accepted.push(file)
    totalBytes += file.size
    if (isImageAttachment(file)) imageCount += 1
  }
  return { accepted, refused }
}

/** The localized sentence for one refused file; it names the file and the reason. */
export function attachmentRefusalMessage(
  refusal: AttachmentRefusal,
  tr: (noText: string, enText: string) => string,
): string {
  const name = refusal.file.name.trim() || tr('uten navn', 'unnamed')
  if (refusal.reason === 'model_no_vision') {
    // Worded here rather than in the i18n catalogues: it is the one reason that names a model.
    const alternative = refusal.visionModelName
    const why = tr(
      `«${name}» ble ikke lagt ved: den valgte modellen kan ikke lese bilder.`,
      `"${name}" was not attached: the selected model cannot read images.`,
    )
    const example = alternative ?? ''
    const noExample = alternative ? `, for eksempel ${example}` : ''
    const enExample = alternative ? `, for example ${example}` : ''
    return `${why} ${tr(`Velg en modell som kan lese bilder${noExample}.`, `Pick a model that supports images${enExample}.`)}`
  }
  const limits = {
    maxFileBytes: ATTACHMENT_MAX_FILE_BYTES,
    maxTotalBytes: ATTACHMENT_MAX_TOTAL_BYTES,
    maxImages: refusal.maxImages ?? ATTACHMENT_MAX_IMAGES,
  }
  const reason = refusal.reason
  return tr(no.attachments.refusal[reason](name, limits), en.attachments.refusal[reason](name, limits))
}
