export interface Size {
  readonly width: number;
  readonly height: number;
}

export interface Point {
  readonly x: number;
  readonly y: number;
}

/**
 * Regner om mellom lokale kanvas-koordinater og eksterne skjerm-koordinater.
 * ALL koordinatkonvertering i biblioteket skal gå gjennom denne klassen —
 * "Never spread coordinate calculations throughout UI components."
 */
export class CoordinateMapper {
  private scale = 1;
  private offsetX = 0;
  private offsetY = 0;

  constructor(
    private remoteSize: Size,
    private viewportSize: Size,
  ) {
    this.recompute();
  }

  update(remoteSize: Size, viewportSize: Size): void {
    this.remoteSize = remoteSize;
    this.viewportSize = viewportSize;
    this.recompute();
  }

  private recompute(): void {
    const { width: rw, height: rh } = this.remoteSize;
    const { width: vw, height: vh } = this.viewportSize;
    if (rw <= 0 || rh <= 0 || vw <= 0 || vh <= 0) {
      this.scale = 1;
      this.offsetX = 0;
      this.offsetY = 0;
      return;
    }
    // "contain": skaler slik at hele fjernskjermen er synlig, med
    // letterboxing på den korteste aksen fremfor beskjæring eller strekking.
    this.scale = Math.min(vw / rw, vh / rh);
    this.offsetX = (vw - rw * this.scale) / 2;
    this.offsetY = (vh - rh * this.scale) / 2;
  }

  remoteToLocal(point: Point): Point {
    return {
      x: point.x * this.scale + this.offsetX,
      y: point.y * this.scale + this.offsetY,
    };
  }

  /** Klemmer resultatet til fjernskjermens grenser — punkter i letterbox-feltet finnes ikke eksternt. */
  localToRemote(point: Point): Point {
    const rawX = (point.x - this.offsetX) / this.scale;
    const rawY = (point.y - this.offsetY) / this.scale;
    return {
      x: clamp(rawX, 0, this.remoteSize.width),
      y: clamp(rawY, 0, this.remoteSize.height),
    };
  }

  getScale(): number {
    return this.scale;
  }
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}
