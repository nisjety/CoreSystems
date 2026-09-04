import type { ActionResult, PointerButton, PointerController, SessionActor } from '../types/public.js';
import type { ActionExecutor } from '../actions/ActionExecutor.js';

/**
 * Bekvemmelighetslag for menneskestyrt pekerinput. Husker siste kjente
 * posisjon slik at `click({ button: "left" })` uten koordinater treffer der
 * pekeren faktisk står — akkurat som en ordentlig mus.
 */
export class PointerControllerImpl implements PointerController {
  private lastX = 0;
  private lastY = 0;
  private lastDisplayId: string | undefined;

  constructor(
    private readonly executor: ActionExecutor,
    private readonly actor: SessionActor = 'human',
  ) {}

  move(point: { x: number; y: number; displayId?: string }): Promise<ActionResult> {
    this.lastX = point.x;
    this.lastY = point.y;
    this.lastDisplayId = point.displayId;
    return this.executor.execute({
      type: 'pointer.move',
      actor: this.actor,
      x: point.x,
      y: point.y,
      displayId: point.displayId,
    });
  }

  click(options: { button: PointerButton; x?: number; y?: number; displayId?: string }): Promise<ActionResult> {
    const { x, y, displayId } = this.resolvePosition(options);
    return this.executor.execute({ type: 'pointer.click', actor: this.actor, x, y, button: options.button, displayId });
  }

  doubleClick(options: {
    button: PointerButton;
    x?: number;
    y?: number;
    displayId?: string;
  }): Promise<ActionResult> {
    const { x, y, displayId } = this.resolvePosition(options);
    return this.executor.execute({
      type: 'pointer.doubleClick',
      actor: this.actor,
      x,
      y,
      button: options.button,
      displayId,
    });
  }

  scroll(delta: { deltaX: number; deltaY: number; x?: number; y?: number; displayId?: string }): Promise<ActionResult> {
    const { x, y, displayId } = this.resolvePosition(delta);
    return this.executor.execute({
      type: 'pointer.scroll',
      actor: this.actor,
      x,
      y,
      deltaX: delta.deltaX,
      deltaY: delta.deltaY,
      displayId,
    });
  }

  private resolvePosition(options: { x?: number; y?: number; displayId?: string }): {
    x: number;
    y: number;
    displayId: string | undefined;
  } {
    return {
      x: options.x ?? this.lastX,
      y: options.y ?? this.lastY,
      displayId: options.displayId ?? this.lastDisplayId,
    };
  }
}
