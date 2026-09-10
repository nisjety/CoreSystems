import type { ActionResult, KeyboardController, SessionActor } from '../types/public.js';
import type { ActionExecutor } from '../actions/ActionExecutor.js';

export class KeyboardControllerImpl implements KeyboardController {
  constructor(
    private readonly executor: ActionExecutor,
    private readonly actor: SessionActor = 'human',
  ) {}

  keyDown(key: string): Promise<ActionResult> {
    return this.executor.execute({ type: 'keyboard.keyDown', actor: this.actor, key });
  }

  keyUp(key: string): Promise<ActionResult> {
    return this.executor.execute({ type: 'keyboard.keyUp', actor: this.actor, key });
  }

  type(text: string): Promise<ActionResult> {
    return this.executor.execute({ type: 'keyboard.type', actor: this.actor, text });
  }
}
