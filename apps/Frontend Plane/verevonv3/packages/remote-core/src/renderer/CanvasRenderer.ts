import type { RemoteEventHandler, RemoteSession, Unsubscribe } from '../types/public.js';
import { CoordinateMapper, type Point } from './CoordinateMapper.js';
import { releaseVideoFrame } from '../media/VideoFrame.js';

type DrawableSource = CanvasImageSource;

/**
 * Kanvas-visning av en økt. Forbruker frames — den eier ingen protokoll- eller
 * dekodertilstand ("The renderer is a consumer of frames"). All
 * koordinatkonvertering delegeres til CoordinateMapper.
 */
export class CanvasRenderer {
  private readonly context: CanvasRenderingContext2D;
  private mapper: CoordinateMapper;
  private unsubscribeFrame: Unsubscribe | undefined;
  private unsubscribeDisplayChange: Unsubscribe | undefined;
  private lastFrameSize = { width: 0, height: 0 };

  constructor(private readonly canvas: HTMLCanvasElement) {
    const context = canvas.getContext('2d');
    if (!context) {
      throw new Error('CanvasRenderer requires a 2D rendering context');
    }
    this.context = context;
    this.mapper = new CoordinateMapper(
      { width: 1, height: 1 },
      { width: canvas.clientWidth || 1, height: canvas.clientHeight || 1 },
    );
  }

  attach(session: RemoteSession): void {
    this.detach();
    this.unsubscribeFrame = session.on('frame', this.handleFrame);
    this.unsubscribeDisplayChange = session.on('display-change', this.handleDisplayChange);
  }

  detach(): void {
    this.unsubscribeFrame?.();
    this.unsubscribeDisplayChange?.();
    this.unsubscribeFrame = undefined;
    this.unsubscribeDisplayChange = undefined;
  }

  /** Må kalles når kanvasets CSS-størrelse endres (resize, fullskjerm-bytte). */
  resize(): void {
    const dpr = globalThis.devicePixelRatio || 1;
    const cssWidth = this.canvas.clientWidth || this.canvas.width;
    const cssHeight = this.canvas.clientHeight || this.canvas.height;
    this.canvas.width = Math.round(cssWidth * dpr);
    this.canvas.height = Math.round(cssHeight * dpr);
    this.mapper.update(this.lastFrameSize, { width: this.canvas.width, height: this.canvas.height });
  }

  localToRemote(point: Point): Point {
    return this.mapper.localToRemote(point);
  }

  remoteToLocal(point: Point): Point {
    return this.mapper.remoteToLocal(point);
  }

  private readonly handleFrame: RemoteEventHandler<'frame'> = (frame) => {
    const size = { width: frame.width, height: frame.height };
    if (size.width !== this.lastFrameSize.width || size.height !== this.lastFrameSize.height) {
      this.lastFrameSize = size;
      this.mapper.update(size, { width: this.canvas.width, height: this.canvas.height });
    }

    const source = frame.videoFrame ?? frame.bitmap;
    if (source) {
      this.paint(source, size);
    }
    releaseVideoFrame(frame);
  };

  private readonly handleDisplayChange: RemoteEventHandler<'display-change'> = () => {
    // Selve oppløsningen kommer med neste 'frame'-hendelse; her er det
    // foreløpig ikke noe å tegne, kun en anledning til å tømme kanvaset.
    this.context.clearRect(0, 0, this.canvas.width, this.canvas.height);
  };

  private paint(source: DrawableSource, size: { width: number; height: number }): void {
    this.context.clearRect(0, 0, this.canvas.width, this.canvas.height);
    const topLeft = this.mapper.remoteToLocal({ x: 0, y: 0 });
    const bottomRight = this.mapper.remoteToLocal({ x: size.width, y: size.height });
    this.context.drawImage(
      source,
      topLeft.x,
      topLeft.y,
      bottomRight.x - topLeft.x,
      bottomRight.y - topLeft.y,
    );
  }

  requestFullscreen(): Promise<void> {
    return this.canvas.requestFullscreen();
  }
}
