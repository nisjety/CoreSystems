import type {
  RemoteCursorShape,
  RemoteEventHandler,
  RemoteSession,
  Unsubscribe,
} from '../types/public.js';
import { asRenderStatsSink, type RenderStatsSink } from '../types/internal.js';
import { CoordinateMapper, type Point } from './CoordinateMapper.js';

type DrawableSource = CanvasImageSource;

interface Surface {
  readonly canvas: DrawableSource;
  readonly context: CanvasRenderingContext2D;
}

interface PreparedCursor {
  readonly shape: RemoteCursorShape;
  readonly surface: Surface;
}

/**
 * Kanvas-visning av en økt. Forbruker frames — den eier ingen protokoll- eller
 * dekodertilstand ("The renderer is a consumer of frames"). All
 * koordinatkonvertering delegeres til CoordinateMapper.
 *
 * Markøren tegnes som et overlegg over en INTERN buffer, ikke over den
 * innkommende frame-kilden direkte. Grunnen: musen kan bevege seg (eller
 * kanvaset endre størrelse) uten at det kommer noen ny frame — et
 * stillestående skrivebord sender ingen video før noe faktisk endres — så
 * markøren må kunne males på nytt fra noe rendereren selv eier. Å beholde en
 * referanse til selve `RemoteVideoFrame`-kilden på tvers av hendelser var det
 * første forsøket, men er IKKE trygt for en bitmap-basert frame: eierskaps-
 * reglene i media/VideoFrame.ts sier produsenten lukker originalen rett etter
 * utsending, og `cloneVideoFrame()` gir bevisst ingen uavhengig kopi av
 * `bitmap` (kun `videoFrame` klones reelt — se den filens doc-kommentar).
 * Bufferen her er derfor rendererens EGEN ressurs: tegnet én gang per
 * innkommende frame, og trygg å male fra så mange ganger som helst etterpå.
 */
export class CanvasRenderer {
  private readonly context: CanvasRenderingContext2D;
  private mapper: CoordinateMapper;
  private readonly unsubscribers: Unsubscribe[] = [];
  private lastFrameSize = { width: 0, height: 0 };
  private statsSink: RenderStatsSink | undefined;
  /** Antall frames denne rendereren faktisk har malt. */
  private painted = 0;
  private buffer: Surface | undefined;
  private cursor: PreparedCursor | undefined;
  private cursorPosition: Point | undefined;
  /** Sant når verten allerede har malt markøren inn i videoen — da tegner vi ingen egen. */
  private cursorEmbedded = false;

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
    this.unsubscribers.push(
      session.on('frame', this.handleFrame),
      session.on('display-change', this.handleDisplayChange),
      session.on('cursor-shape', this.handleCursorShape),
      session.on('cursor-position', this.handleCursorPosition),
    );
    this.cursorEmbedded = session.displays.find((display) => display.isPrimary)?.cursorEmbedded ?? false;
    // `renderedFrames` i SessionStats kan bare fylles av den som faktisk maler.
    this.statsSink = asRenderStatsSink(session);
  }

  detach(): void {
    for (const unsubscribe of this.unsubscribers) unsubscribe();
    this.unsubscribers.length = 0;
    this.statsSink = undefined;
    this.buffer = undefined;
    this.cursor = undefined;
    this.cursorPosition = undefined;
  }

  /** Frames denne rendereren har malt. Speiles i `session.stats.renderedFrames`. */
  get paintedFrames(): number {
    return this.painted;
  }

  /** Må kalles når kanvasets CSS-størrelse endres (resize, fullskjerm-bytte). */
  resize(): void {
    const dpr = globalThis.devicePixelRatio || 1;
    const cssWidth = this.canvas.clientWidth || this.canvas.width;
    const cssHeight = this.canvas.clientHeight || this.canvas.height;
    this.canvas.width = Math.round(cssWidth * dpr);
    this.canvas.height = Math.round(cssHeight * dpr);
    this.mapper.update(this.lastFrameSize, { width: this.canvas.width, height: this.canvas.height });
    this.repaint();
  }

  localToRemote(point: Point): Point {
    return this.mapper.localToRemote(point);
  }

  remoteToLocal(point: Point): Point {
    return this.mapper.remoteToLocal(point);
  }

  /**
   * Oversetter en pekerposisjon fra en DOM-hendelse (`clientX`/`clientY`) til
   * fjernskjerm-koordinater. Tar høyde for både kanvasets plassering på siden
   * og forholdet mellom CSS-piksler og backing-store-piksler (HiDPI) — det er
   * nettopp dette regnestykket som ikke skal spres ut i UI-komponenter.
   */
  clientToRemote(clientX: number, clientY: number): Point {
    const rect = this.canvas.getBoundingClientRect();
    const scaleX = rect.width > 0 ? this.canvas.width / rect.width : 1;
    const scaleY = rect.height > 0 ? this.canvas.height / rect.height : 1;
    return this.mapper.localToRemote({
      x: (clientX - rect.left) * scaleX,
      y: (clientY - rect.top) * scaleY,
    });
  }

  private readonly handleFrame: RemoteEventHandler<'frame'> = (frame) => {
    const size = { width: frame.width, height: frame.height };
    if (size.width !== this.lastFrameSize.width || size.height !== this.lastFrameSize.height) {
      this.lastFrameSize = size;
      this.mapper.update(size, { width: this.canvas.width, height: this.canvas.height });
    }

    const source = frame.videoFrame ?? frame.bitmap;
    if (!source) return;

    // Kopier ÉN gang inn i vår egen buffer mens kilden garantert er gyldig
    // (vi tegner synkront, før RemoteSession rekker å frigjøre den). Bufferen
    // er deretter trygg å male fra så mange ganger vi vil, uavhengig av hva
    // som skjer med den opprinnelige frame-kilden etterpå.
    this.buffer = this.ensureBuffer(size);
    this.buffer.context.clearRect(0, 0, size.width, size.height);
    this.buffer.context.drawImage(source, 0, 0, size.width, size.height);

    this.paint();
    this.drawCursor();
    this.painted += 1;
    this.statsSink?.recordRenderedFrame();
  };

  private readonly handleDisplayChange: RemoteEventHandler<'display-change'> = ({ displays }) => {
    this.cursorEmbedded = displays.find((display) => display.isPrimary)?.cursorEmbedded ?? false;
    // Posisjonen tilhørte forrige skjerm; verten sender en ny først når
    // pekeren faktisk beveger seg (ingen øyeblikksbilde ved bytte).
    this.cursorPosition = undefined;
    // Selve oppløsningen kommer med neste 'frame'-hendelse; her er det
    // foreløpig ikke noe å tegne, kun en anledning til å tømme kanvaset.
    this.buffer = undefined;
    this.context.clearRect(0, 0, this.canvas.width, this.canvas.height);
  };

  private readonly handleCursorShape: RemoteEventHandler<'cursor-shape'> = (shape) => {
    const surface = prepareCursorSurface(shape);
    if (!surface) return;
    this.cursor = { shape, surface };
    if (this.cursorEmbedded) return; // ingenting synlig endrer seg — ikke mal på nytt for ingenting
    this.repaint();
  };

  private readonly handleCursorPosition: RemoteEventHandler<'cursor-position'> = (position) => {
    this.cursorPosition = { x: position.x, y: position.y };
    if (this.cursorEmbedded) return;
    this.repaint();
  };

  /** Maler bufferen (siste frame) på nytt (etter markørbevegelse eller resize). */
  private repaint(): void {
    if (!this.buffer) return;
    this.paint();
    this.drawCursor();
  }

  private paint(): void {
    const buffer = this.buffer;
    if (!buffer) return;
    this.context.clearRect(0, 0, this.canvas.width, this.canvas.height);
    const topLeft = this.mapper.remoteToLocal({ x: 0, y: 0 });
    const bottomRight = this.mapper.remoteToLocal({ x: this.lastFrameSize.width, y: this.lastFrameSize.height });
    this.context.drawImage(
      buffer.canvas,
      topLeft.x,
      topLeft.y,
      bottomRight.x - topLeft.x,
      bottomRight.y - topLeft.y,
    );
  }

  /**
   * Markøren skjules til første posisjon er mottatt — RustDesk sender ingen
   * startposisjon, bare endringer — og aldri når verten allerede har bakt den
   * inn i videoen (to markører).
   */
  private drawCursor(): void {
    const cursor = this.cursor;
    const position = this.cursorPosition;
    if (!cursor || !position || this.cursorEmbedded) return;

    const scale = this.mapper.getScale();
    const local = this.mapper.remoteToLocal(position);
    const { shape } = cursor;
    this.context.drawImage(
      cursor.surface.canvas,
      local.x - shape.hotspotX * scale,
      local.y - shape.hotspotY * scale,
      shape.width * scale,
      shape.height * scale,
    );
  }

  /** Gjenbruker bufferen når størrelsen ikke har endret seg; oppretter en ny ellers. */
  private ensureBuffer(size: { width: number; height: number }): Surface {
    if (this.buffer && bufferMatchesSize(this.buffer.canvas, size)) return this.buffer;
    const surface = createSurface(size.width, size.height);
    if (!surface) {
      throw new Error('CanvasRenderer could not create an internal drawing surface');
    }
    return surface;
  }

  requestFullscreen(): Promise<void> {
    return this.canvas.requestFullscreen();
  }
}

function bufferMatchesSize(canvas: DrawableSource, size: { width: number; height: number }): boolean {
  const candidate = canvas as { width?: number; height?: number };
  return candidate.width === size.width && candidate.height === size.height;
}

/**
 * En liten, frittstående tegneoverflate — brukt både for frame-bufferen og
 * for hver markørform. `OffscreenCanvas` er førstevalget (ingen DOM-kobling
 * nødvendig); en vanlig `<canvas>`-node er fallback for miljøer uten den.
 */
function createSurface(width: number, height: number): Surface | undefined {
  if (typeof OffscreenCanvas !== 'undefined') {
    const canvas = new OffscreenCanvas(width, height);
    const context = canvas.getContext('2d');
    if (!context) return undefined;
    return { canvas, context: context as unknown as CanvasRenderingContext2D };
  }
  if (typeof document === 'undefined') return undefined;
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d');
  if (!context) return undefined;
  return { canvas, context };
}

/**
 * Gjør RGBA-bytene om til noe `drawImage` kan tegne. Skjer én gang per
 * markørform (formene er cachet på protokollnivå og gjenbrukes via id).
 * `ImageData` forventer ikke-premultiplisert RGBA — nøyaktig det RustDesk
 * leverer — så ingen kanalbytte er nødvendig.
 */
function prepareCursorSurface(shape: RemoteCursorShape): Surface | undefined {
  if (typeof ImageData === 'undefined') return undefined;
  const surface = createSurface(shape.width, shape.height);
  if (!surface) return undefined;
  const pixels = new Uint8ClampedArray(shape.rgba.length);
  pixels.set(shape.rgba);
  surface.context.putImageData(new ImageData(pixels, shape.width, shape.height), 0, 0);
  return surface;
}
