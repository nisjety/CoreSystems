import { requestJson } from '@/shared/api/http'

/**
 * Region and selection attach for the live browser frame (E3.3-d).
 *
 * The person drags a rectangle over the streamed frame; this module turns the
 * drag into a region in viewport CSS pixels, crops that region out of the frame
 * bitmap the client already shows, and posts it to the gateway. The gateway is
 * the authority: it re-resolves the tab against Quarry's live tab list (the
 * frame only shows the active tab), snaps and clamps the region to the session
 * viewport, checks the image against the resolved region, refuses a Zero Data
 * Retention session, and returns an ephemeral attachment classified
 * `browser-scraped` that grants nothing. Nothing here is a grant or an action.
 *
 * The gateway route is mounted only with `BROWSER_REGION_ATTACH` (default off);
 * with the flag off the call answers 404. The BrowserChrome drag overlay and the
 * composer chip that carries the attachment are not wired here.
 *
 * The pure helpers mirror the gateway rules (browser_evidence.rs) so the UI can
 * refuse early, but the gateway answer is the one that counts.
 */

export const REGION_MIN_EDGE_PX = 8
/** Decoded image bytes the gateway accepts (1 MiB). */
export const REGION_IMAGE_MAX_BYTES = 1024 * 1024
/** Longest edge of the cropped image the client sends. */
export const REGION_IMAGE_MAX_EDGE_PX = 1600
export const SELECTION_MAX_CHARS = 4000

const TAB_ID_RE = /^[A-Za-z0-9_-]{1,64}$/
const JPEG_QUALITIES = [0.85, 0.7, 0.55] as const

export type ViewportSize = { width: number; height: number }

/** A point in client (page) coordinates, as a pointer event reports it. */
export type ClientPoint = { x: number; y: number }

/**
 * The box the frame image content is drawn in, in client coordinates (for an
 * `<img>` with object-fit, the content box, not the element box).
 */
export type DisplayBox = { left: number; top: number; width: number; height: number }

/** A region in viewport CSS pixels. */
export type RegionRect = { x: number; y: number; width: number; height: number }

export type RegionImageMimeType = 'image/png' | 'image/jpeg'

export type RegionImagePayload = { mimeType: RegionImageMimeType; dataBase64: string }

export type RegionAttachRequest = {
  tabId: string
  region: RegionRect
  image: RegionImagePayload
  selectionText?: string
}

/** The gateway's answer. Ephemeral: nothing is stored server-side. */
export type BrowserRegionAttachment = {
  kind: 'browser-region'
  trustClass: 'browser-scraped'
  grant: 'none'
  retention: 'ephemeral'
  sessionId: string
  tabId: string
  label: string
  title: string | null
  target: string | null
  region: RegionRect
  viewport: ViewportSize
  image: RegionImagePayload & { width: number; height: number; byteLength: number }
  selection: { text: string; length: number } | null
}

export type RegionAttachRefusal =
  | 'zero-retention'
  | 'invalid-tab'
  | 'region-too-small'
  | 'image-too-large'
  | 'selection-too-long'

export type RegionAttachBuild =
  | { ok: true; request: RegionAttachRequest }
  | { ok: false; reason: RegionAttachRefusal }

/**
 * Snap a region outward to whole pixels and clamp it to the viewport, as the
 * gateway does. Null when a value is not finite, an edge is not positive, or
 * the clamped region is smaller than the minimum edge.
 */
export function clampRegion(region: RegionRect, viewport: ViewportSize): RegionRect | null {
  const { x, y, width, height } = region
  if (![x, y, width, height, viewport.width, viewport.height].every(Number.isFinite)) return null
  if (width <= 0 || height <= 0 || viewport.width <= 0 || viewport.height <= 0) return null
  const left = clamp(Math.floor(x), 0, viewport.width)
  const top = clamp(Math.floor(y), 0, viewport.height)
  const right = clamp(Math.ceil(x + width), 0, viewport.width)
  const bottom = clamp(Math.ceil(y + height), 0, viewport.height)
  if (right - left < REGION_MIN_EDGE_PX || bottom - top < REGION_MIN_EDGE_PX) return null
  return { x: left, y: top, width: right - left, height: bottom - top }
}

/**
 * The viewport region a drag covers, from two client points over the frame's
 * display box. The drag may go in any direction. Null when the drag is too
 * small or falls outside the frame.
 */
export function regionFromDrag(
  start: ClientPoint,
  end: ClientPoint,
  display: DisplayBox,
  viewport: ViewportSize,
): RegionRect | null {
  if (!(display.width > 0) || !(display.height > 0)) return null
  const scaleX = viewport.width / display.width
  const scaleY = viewport.height / display.height
  const x0 = (Math.min(start.x, end.x) - display.left) * scaleX
  const y0 = (Math.min(start.y, end.y) - display.top) * scaleY
  const x1 = (Math.max(start.x, end.x) - display.left) * scaleX
  const y1 = (Math.max(start.y, end.y) - display.top) * scaleY
  return clampRegion({ x: x0, y: y0, width: x1 - x0, height: y1 - y0 }, viewport)
}

/** The rectangle of the frame bitmap that shows a viewport region. */
export function sourceRectFor(
  region: RegionRect,
  viewport: ViewportSize,
  frame: { width: number; height: number },
): { sx: number; sy: number; sw: number; sh: number } {
  const scaleX = frame.width / viewport.width
  const scaleY = frame.height / viewport.height
  return {
    sx: region.x * scaleX,
    sy: region.y * scaleY,
    sw: region.width * scaleX,
    sh: region.height * scaleY,
  }
}

/** Output size for a crop: the source size, scaled down so the longest edge fits. */
export function outputSizeFor(
  sourceWidth: number,
  sourceHeight: number,
  maxEdge: number = REGION_IMAGE_MAX_EDGE_PX,
): { width: number; height: number } {
  const longest = Math.max(sourceWidth, sourceHeight)
  const scale = longest > maxEdge ? maxEdge / longest : 1
  return {
    width: Math.max(1, Math.round(sourceWidth * scale)),
    height: Math.max(1, Math.round(sourceHeight * scale)),
  }
}

/** Decoded byte length of a base64 string (no data: prefix). */
export function base64ByteLength(dataBase64: string): number {
  const value = dataBase64.trim()
  if (!value) return 0
  const padding = value.endsWith('==') ? 2 : value.endsWith('=') ? 1 : 0
  return Math.floor((value.length * 3) / 4) - padding
}

/**
 * Selected text as the gateway keeps it: control characters other than newline
 * and tab become spaces, the text is trimmed, and blank text is no selection.
 */
export function normalizeSelectionText(raw: string | null | undefined): string | null {
  if (raw === null || raw === undefined) return null
  let cleaned = ''
  for (const ch of raw) {
    const code = ch.codePointAt(0) ?? 0
    const control = code < 0x20 || (code >= 0x7f && code <= 0x9f)
    cleaned += control && ch !== '\n' && ch !== '\t' ? ' ' : ch
  }
  const trimmed = cleaned.trim()
  return trimmed ? trimmed : null
}

export type BuildRegionAttachInput = {
  tabId: string
  region: RegionRect
  viewport: ViewportSize
  image: RegionImagePayload
  selectionText?: string | null
  /** True for a Zero Data Retention session: page regions are never attached. */
  zeroRetention?: boolean
}

/** Validate and shape the request body, refusing early what the gateway refuses. */
export function buildRegionAttachRequest(input: BuildRegionAttachInput): RegionAttachBuild {
  if (input.zeroRetention) return { ok: false, reason: 'zero-retention' }
  const tabId = input.tabId.trim()
  if (!TAB_ID_RE.test(tabId)) return { ok: false, reason: 'invalid-tab' }
  const region = clampRegion(input.region, input.viewport)
  if (!region) return { ok: false, reason: 'region-too-small' }
  if (base64ByteLength(input.image.dataBase64) > REGION_IMAGE_MAX_BYTES) {
    return { ok: false, reason: 'image-too-large' }
  }
  const selection = normalizeSelectionText(input.selectionText)
  if (selection !== null && [...selection].length > SELECTION_MAX_CHARS) {
    return { ok: false, reason: 'selection-too-long' }
  }
  const request: RegionAttachRequest = { tabId, region, image: input.image }
  if (selection !== null) request.selectionText = selection
  return { ok: true, request }
}

export type CanvasLike = {
  width: number
  height: number
  getContext(kind: '2d'): Pick<CanvasRenderingContext2D, 'drawImage'> | null
  toDataURL(type?: string, quality?: number): string
}

export type CropFrameOptions = {
  maxEdge?: number
  /** Injected for tests and workers; defaults to a detached `<canvas>`. */
  createCanvas?: (width: number, height: number) => CanvasLike
}

/**
 * Crop a viewport region out of the frame bitmap the client already shows (the
 * `<img>` of the streamed frame, or an ImageBitmap of it) and encode it as JPEG,
 * lowering the quality until it fits the gateway's byte limit. The frame comes
 * from the same-origin gateway, so the canvas is not tainted. Returns null when
 * the region is unusable or no encoding fits.
 */
export function cropFrameRegion(
  frame: { image: CanvasImageSource; width: number; height: number },
  region: RegionRect,
  viewport: ViewportSize,
  options: CropFrameOptions = {},
): RegionImagePayload | null {
  const resolved = clampRegion(region, viewport)
  if (!resolved || !(frame.width > 0) || !(frame.height > 0)) return null
  const { sx, sy, sw, sh } = sourceRectFor(resolved, viewport, frame)
  const size = outputSizeFor(sw, sh, options.maxEdge)
  const canvas = (options.createCanvas ?? defaultCanvas)(size.width, size.height)
  canvas.width = size.width
  canvas.height = size.height
  const context = canvas.getContext('2d')
  if (!context) return null
  context.drawImage(frame.image, sx, sy, sw, sh, 0, 0, size.width, size.height)
  for (const quality of JPEG_QUALITIES) {
    const dataBase64 = stripDataUrl(canvas.toDataURL('image/jpeg', quality), 'image/jpeg')
    if (dataBase64 && base64ByteLength(dataBase64) <= REGION_IMAGE_MAX_BYTES) {
      return { mimeType: 'image/jpeg', dataBase64 }
    }
  }
  return null
}

/** POST the region to the gateway and return the ephemeral attachment. */
export async function attachBrowserRegion(
  sessionId: string,
  request: RegionAttachRequest,
  signal?: AbortSignal,
): Promise<BrowserRegionAttachment> {
  const response = await requestJson<{ attachment: BrowserRegionAttachment }>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/attachments/region`,
    { method: 'POST', body: JSON.stringify(request), signal },
  )
  return response.attachment
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}

function stripDataUrl(dataUrl: string, mimeType: RegionImageMimeType): string | null {
  const prefix = `data:${mimeType};base64,`
  return dataUrl.startsWith(prefix) ? dataUrl.slice(prefix.length) : null
}

function defaultCanvas(width: number, height: number): CanvasLike {
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  return canvas as unknown as CanvasLike
}
