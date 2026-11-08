import { describe, expect, it, vi } from 'vitest'
import {
  REGION_IMAGE_MAX_BYTES,
  SELECTION_MAX_CHARS,
  base64ByteLength,
  buildRegionAttachRequest,
  clampRegion,
  cropFrameRegion,
  normalizeSelectionText,
  outputSizeFor,
  regionFromDrag,
  sourceRectFor,
  type CanvasLike,
} from './region-attach'

const viewport = { width: 1280, height: 720 }
const image = { mimeType: 'image/jpeg' as const, dataBase64: 'AAAA' }

describe('clampRegion', () => {
  it('snaps outward and clamps to the viewport like the gateway', () => {
    expect(clampRegion({ x: 10.4, y: 20, width: 200, height: 100 }, viewport)).toEqual({
      x: 10,
      y: 20,
      width: 201,
      height: 100,
    })
    expect(clampRegion({ x: -50, y: 700, width: 100, height: 100 }, viewport)).toEqual({
      x: 0,
      y: 700,
      width: 50,
      height: 20,
    })
  })

  it('refuses degenerate or off-viewport regions', () => {
    expect(clampRegion({ x: Number.NaN, y: 0, width: 10, height: 10 }, viewport)).toBeNull()
    expect(clampRegion({ x: 0, y: 0, width: 0, height: 10 }, viewport)).toBeNull()
    expect(clampRegion({ x: 1300, y: 10, width: 50, height: 50 }, viewport)).toBeNull()
    expect(clampRegion({ x: 0, y: 0, width: 4, height: 40 }, viewport)).toBeNull()
  })
})

describe('regionFromDrag', () => {
  it('maps a reversed drag over a half-size display to viewport pixels', () => {
    const display = { left: 100, top: 50, width: 640, height: 360 }
    const region = regionFromDrag({ x: 300, y: 150 }, { x: 200, y: 100 }, display, viewport)
    expect(region).toEqual({ x: 200, y: 100, width: 200, height: 100 })
  })

  it('returns null for a click without a drag', () => {
    const display = { left: 0, top: 0, width: 1280, height: 720 }
    expect(regionFromDrag({ x: 10, y: 10 }, { x: 12, y: 11 }, display, viewport)).toBeNull()
  })
})

describe('crop sizing', () => {
  it('scales the source rect to the frame bitmap', () => {
    expect(
      sourceRectFor({ x: 100, y: 50, width: 200, height: 100 }, viewport, { width: 2560, height: 1440 }),
    ).toEqual({ sx: 200, sy: 100, sw: 400, sh: 200 })
  })

  it('caps the longest edge and keeps the shape', () => {
    expect(outputSizeFor(3200, 1600)).toEqual({ width: 1600, height: 800 })
    expect(outputSizeFor(400, 200)).toEqual({ width: 400, height: 200 })
  })

  it('counts decoded base64 bytes', () => {
    expect(base64ByteLength('')).toBe(0)
    expect(base64ByteLength('AAAA')).toBe(3)
    expect(base64ByteLength('AAA=')).toBe(2)
    expect(base64ByteLength('AA==')).toBe(1)
  })
})

describe('normalizeSelectionText', () => {
  it('keeps newlines and tabs, replaces other control characters and trims', () => {
    expect(normalizeSelectionText(' a\u0007b\nc\td ')).toBe('a b\nc\td')
    expect(normalizeSelectionText('  \u0000 ')).toBeNull()
    expect(normalizeSelectionText(undefined)).toBeNull()
  })
})

describe('buildRegionAttachRequest', () => {
  const base = { tabId: 'tab_a', region: { x: 10.4, y: 20, width: 200, height: 100 }, viewport, image }

  it('builds the gateway body with the clamped region', () => {
    const built = buildRegionAttachRequest({ ...base, selectionText: ' Total ' })
    expect(built).toEqual({
      ok: true,
      request: {
        tabId: 'tab_a',
        region: { x: 10, y: 20, width: 201, height: 100 },
        image,
        selectionText: 'Total',
      },
    })
  })

  it('omits a blank selection', () => {
    const built = buildRegionAttachRequest({ ...base, selectionText: '   ' })
    expect(built.ok && 'selectionText' in built.request).toBe(false)
  })

  it('refuses early what the gateway refuses', () => {
    expect(buildRegionAttachRequest({ ...base, zeroRetention: true })).toEqual({
      ok: false,
      reason: 'zero-retention',
    })
    expect(buildRegionAttachRequest({ ...base, tabId: 'tab/../a' })).toEqual({
      ok: false,
      reason: 'invalid-tab',
    })
    expect(
      buildRegionAttachRequest({ ...base, region: { x: 0, y: 0, width: 2, height: 2 } }),
    ).toEqual({ ok: false, reason: 'region-too-small' })
    const big = 'A'.repeat(Math.ceil(((REGION_IMAGE_MAX_BYTES + 3) * 4) / 3))
    expect(buildRegionAttachRequest({ ...base, image: { ...image, dataBase64: big } })).toEqual({
      ok: false,
      reason: 'image-too-large',
    })
    expect(
      buildRegionAttachRequest({ ...base, selectionText: 'x'.repeat(SELECTION_MAX_CHARS + 1) }),
    ).toEqual({ ok: false, reason: 'selection-too-long' })
  })
})

describe('cropFrameRegion', () => {
  function fakeCanvas(dataUrl: string) {
    const drawImage = vi.fn()
    const canvas: CanvasLike = {
      width: 0,
      height: 0,
      getContext: () => ({ drawImage }),
      toDataURL: () => dataUrl,
    }
    return { canvas, drawImage }
  }

  it('draws the source rect of the frame and returns base64 JPEG', () => {
    const { canvas, drawImage } = fakeCanvas('data:image/jpeg;base64,QUJD')
    const frame = { image: {} as CanvasImageSource, width: 2560, height: 1440 }
    const result = cropFrameRegion(frame, { x: 100, y: 50, width: 200, height: 100 }, viewport, {
      createCanvas: () => canvas,
    })
    expect(result).toEqual({ mimeType: 'image/jpeg', dataBase64: 'QUJD' })
    expect(drawImage).toHaveBeenCalledWith(frame.image, 200, 100, 400, 200, 0, 0, 400, 200)
  })

  it('returns null when the encoder does not produce JPEG', () => {
    const { canvas } = fakeCanvas('data:,')
    const frame = { image: {} as CanvasImageSource, width: 1280, height: 720 }
    expect(
      cropFrameRegion(frame, { x: 0, y: 0, width: 100, height: 100 }, viewport, {
        createCanvas: () => canvas,
      }),
    ).toBeNull()
  })
})
