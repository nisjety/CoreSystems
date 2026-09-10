import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CanvasRenderer } from '../../src/renderer/CanvasRenderer.js';
import { EventBus } from '../../src/events/EventBus.js';
import type { RemoteEventMap, RemoteSession } from '../../src/types/public.js';

/**
 * Node has no canvas/ImageData/OffscreenCanvas — this is exactly why
 * CanvasRenderer has never had a direct test (see docs/architecture.md,
 * "Implemented but NOT verified anywhere yet"). These are the smallest
 * fakes that let the renderer's OWN logic (event wiring, the retained-frame
 * lifecycle, the cursor draw rules) run for real, without pretending to
 * verify actual pixel output — that still needs a real browser.
 */

interface DrawImageCall {
  readonly source: unknown;
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

class FakeContext {
  readonly clearRectCalls: number[][] = [];
  readonly drawImageCalls: DrawImageCall[] = [];

  clearRect(x: number, y: number, w: number, h: number): void {
    this.clearRectCalls.push([x, y, w, h]);
  }

  drawImage(source: unknown, x: number, y: number, width: number, height: number): void {
    this.drawImageCalls.push({ source, x, y, width, height });
  }

  putImageData(): void {
    // The cursor-surface canvases below use this; content is never inspected.
  }
}

class FakeOffscreenCanvas {
  readonly width: number;
  readonly height: number;
  constructor(width: number, height: number) {
    this.width = width;
    this.height = height;
  }
  getContext(): FakeContext {
    return new FakeContext();
  }
}

class FakeImageData {
  constructor(
    public readonly data: Uint8ClampedArray,
    public readonly width: number,
    public readonly height: number,
  ) {}
}

class FakeCanvas {
  readonly context = new FakeContext();
  clientWidth = 800;
  clientHeight = 600;
  width = 800;
  height = 600;
  requestFullscreen = vi.fn(async () => undefined);

  getContext(): FakeContext {
    return this.context;
  }

  getBoundingClientRect() {
    return { left: 0, top: 0, width: this.clientWidth, height: this.clientHeight } as DOMRect;
  }
}

function fakeSession(): { session: RemoteSession; bus: EventBus<RemoteEventMap> } {
  const bus = new EventBus<RemoteEventMap>();
  const session = {
    displays: [],
    on: <K extends keyof RemoteEventMap>(event: K, handler: (payload: RemoteEventMap[K]) => void) =>
      bus.on(event, handler),
  } as unknown as RemoteSession;
  return { session, bus };
}

const originalImageData = globalThis.ImageData;
const originalOffscreenCanvas = globalThis.OffscreenCanvas;

beforeEach(() => {
  globalThis.ImageData = FakeImageData as unknown as typeof ImageData;
  globalThis.OffscreenCanvas = FakeOffscreenCanvas as unknown as typeof OffscreenCanvas;
});

afterEach(() => {
  globalThis.ImageData = originalImageData;
  globalThis.OffscreenCanvas = originalOffscreenCanvas;
});

describe('CanvasRenderer', () => {
  it('paints an attached session\'s frames and clears on display-change', () => {
    const canvas = new FakeCanvas();
    const renderer = new CanvasRenderer(canvas as unknown as HTMLCanvasElement);
    const { session, bus } = fakeSession();
    renderer.attach(session);

    const bitmap = {} as ImageBitmap;
    bus.emit('frame', { width: 100, height: 50, timestamp: 0, bitmap });

    expect(canvas.context.drawImageCalls).toHaveLength(1);
    // Paints from its OWN buffer, never the original frame source directly —
    // that source may be closed by the producer the instant this handler
    // returns (see media/VideoFrame.ts's ownership rule).
    expect(canvas.context.drawImageCalls[0]?.source).not.toBe(bitmap);
    expect(renderer.paintedFrames).toBe(1);

    bus.emit('display-change', { displays: [] });
    // display-change clears once more on top of the frame's own clear.
    expect(canvas.context.clearRectCalls.length).toBeGreaterThanOrEqual(2);
  });

  it('reports every painted frame through the session\'s RenderStatsSink', () => {
    const canvas = new FakeCanvas();
    const renderer = new CanvasRenderer(canvas as unknown as HTMLCanvasElement);
    const recordRenderedFrame = vi.fn();
    const { bus } = fakeSession();
    const session = {
      displays: [],
      on: (event: string, handler: (payload: unknown) => void) => bus.on(event as never, handler as never),
      recordRenderedFrame,
    } as unknown as RemoteSession;
    renderer.attach(session);

    bus.emit('frame', { width: 10, height: 10, timestamp: 0, bitmap: {} as ImageBitmap });
    bus.emit('frame', { width: 10, height: 10, timestamp: 1, bitmap: {} as ImageBitmap });

    expect(recordRenderedFrame).toHaveBeenCalledTimes(2);
    expect(renderer.paintedFrames).toBe(2);
  });

  it('does not draw a cursor until BOTH a shape and a position have arrived', () => {
    const canvas = new FakeCanvas();
    const renderer = new CanvasRenderer(canvas as unknown as HTMLCanvasElement);
    const { session, bus } = fakeSession();
    renderer.attach(session);

    bus.emit('frame', { width: 100, height: 100, timestamp: 0, bitmap: {} as ImageBitmap });
    expect(canvas.context.drawImageCalls).toHaveLength(1); // just the frame

    bus.emit('cursor-shape', { id: '1', width: 8, height: 8, hotspotX: 0, hotspotY: 0, rgba: new Uint8Array(8 * 8 * 4) });
    // Shape without a position: still nothing drawn beyond the frame itself
    // (repaint() ran, but drawCursor's own guard rejects a missing position).
    expect(canvas.context.drawImageCalls).toHaveLength(2); // frame is repainted

    bus.emit('cursor-position', { x: 50, y: 50, displayId: '0' });
    // Now both are present: the cursor is actually drawn, at the mapper's scale
    // (100 remote px onto an 800x600 viewport => scale 6, "contain"-fitted).
    const calls = canvas.context.drawImageCalls;
    expect(calls.length).toBeGreaterThan(2);
    expect(calls.at(-1)?.width).toBe(8 * 6);
    expect(calls.at(-1)?.height).toBe(8 * 6);
  });

  it('never draws a cursor overlay when the streamed display reports cursorEmbedded', () => {
    const canvas = new FakeCanvas();
    const renderer = new CanvasRenderer(canvas as unknown as HTMLCanvasElement);
    const bus = new EventBus<RemoteEventMap>();
    const session = {
      displays: [
        { id: '0', label: 'x', width: 100, height: 100, isPrimary: true, scaleFactor: 1, cursorEmbedded: true },
      ],
      on: (event: string, handler: (payload: unknown) => void) => bus.on(event as never, handler as never),
    } as unknown as RemoteSession;
    renderer.attach(session);

    bus.emit('frame', { width: 100, height: 100, timestamp: 0, bitmap: {} as ImageBitmap });
    bus.emit('cursor-shape', { id: '1', width: 8, height: 8, hotspotX: 0, hotspotY: 0, rgba: new Uint8Array(8 * 8 * 4) });
    bus.emit('cursor-position', { x: 50, y: 50, displayId: '0' });

    // Only ever the frame itself — the host already painted the pointer in.
    expect(canvas.context.drawImageCalls).toHaveLength(1);
  });

  it('subtracts the hotspot when positioning the cursor', () => {
    const canvas = new FakeCanvas();
    const renderer = new CanvasRenderer(canvas as unknown as HTMLCanvasElement);
    const { session, bus } = fakeSession();
    renderer.attach(session);

    // 1:1 scale: a 100x100 frame filling an (effectively) 100x100 viewport.
    canvas.clientWidth = 100;
    canvas.clientHeight = 100;
    canvas.width = 100;
    canvas.height = 100;
    bus.emit('frame', { width: 100, height: 100, timestamp: 0, bitmap: {} as ImageBitmap });
    bus.emit('cursor-shape', { id: '1', width: 10, height: 20, hotspotX: 2, hotspotY: 4, rgba: new Uint8Array(10 * 20 * 4) });
    bus.emit('cursor-position', { x: 50, y: 50, displayId: '0' });

    const cursorDraw = canvas.context.drawImageCalls.at(-1);
    expect(cursorDraw).toMatchObject({ x: 48, y: 46, width: 10, height: 20 });
  });

  it('keeps drawing the current cursor when the host re-selects it by id', () => {
    const canvas = new FakeCanvas();
    const renderer = new CanvasRenderer(canvas as unknown as HTMLCanvasElement);
    const { session, bus } = fakeSession();
    renderer.attach(session);

    bus.emit('frame', { width: 100, height: 100, timestamp: 0, bitmap: {} as ImageBitmap });
    bus.emit('cursor-shape', { id: 'shape-a', width: 4, height: 4, hotspotX: 0, hotspotY: 0, rgba: new Uint8Array(4 * 4 * 4) });
    bus.emit('cursor-position', { x: 10, y: 10, displayId: '0' });
    const beforeReselect = canvas.context.drawImageCalls.length;

    // A protocol-level cursor_id re-selection re-emits the SAME shape object
    // (RemoteSession forwards protocol events verbatim) — repaint should just work.
    bus.emit('cursor-shape', { id: 'shape-a', width: 4, height: 4, hotspotX: 0, hotspotY: 0, rgba: new Uint8Array(4 * 4 * 4) });

    expect(canvas.context.drawImageCalls.length).toBeGreaterThan(beforeReselect);
  });

  it('detach() stops all further painting and clears cursor state', () => {
    const canvas = new FakeCanvas();
    const renderer = new CanvasRenderer(canvas as unknown as HTMLCanvasElement);
    const { session, bus } = fakeSession();
    renderer.attach(session);
    bus.emit('frame', { width: 10, height: 10, timestamp: 0, bitmap: {} as ImageBitmap });
    expect(renderer.paintedFrames).toBe(1);

    renderer.detach();
    bus.emit('frame', { width: 10, height: 10, timestamp: 1, bitmap: {} as ImageBitmap });

    // Detached: the old subscription is gone, so the second frame never arrives.
    expect(renderer.paintedFrames).toBe(1);
  });

  it('resize() repaints the retained frame and cursor at the new scale, without a new session frame', () => {
    const canvas = new FakeCanvas();
    const renderer = new CanvasRenderer(canvas as unknown as HTMLCanvasElement);
    const { session, bus } = fakeSession();
    renderer.attach(session);

    bus.emit('frame', { width: 100, height: 100, timestamp: 0, bitmap: {} as ImageBitmap });
    bus.emit('cursor-shape', { id: '1', width: 4, height: 4, hotspotX: 0, hotspotY: 0, rgba: new Uint8Array(4 * 4 * 4) });
    bus.emit('cursor-position', { x: 50, y: 50, displayId: '0' });
    const before = canvas.context.drawImageCalls.length;

    canvas.clientWidth = 400;
    canvas.clientHeight = 400;
    renderer.resize();

    // Both the frame and the cursor are repainted from the RETAINED copy —
    // paintedFrames (which only counts real 'frame' events) must not move.
    expect(canvas.context.drawImageCalls.length).toBeGreaterThan(before);
    expect(renderer.paintedFrames).toBe(1);
  });

  it('does not throw when re-attaching over a live cursor overlay (attach() implies detach())', () => {
    const canvas = new FakeCanvas();
    const renderer = new CanvasRenderer(canvas as unknown as HTMLCanvasElement);
    const first = fakeSession();
    renderer.attach(first.session);
    first.bus.emit('frame', { width: 10, height: 10, timestamp: 0, bitmap: {} as ImageBitmap });
    first.bus.emit('cursor-shape', { id: '1', width: 4, height: 4, hotspotX: 0, hotspotY: 0, rgba: new Uint8Array(4 * 4 * 4) });
    first.bus.emit('cursor-position', { x: 5, y: 5, displayId: '0' });

    const second = fakeSession();
    expect(() => renderer.attach(second.session)).not.toThrow();

    // The old session's events must no longer reach the renderer.
    const before = renderer.paintedFrames;
    first.bus.emit('frame', { width: 10, height: 10, timestamp: 1, bitmap: {} as ImageBitmap });
    expect(renderer.paintedFrames).toBe(before);
  });
});
