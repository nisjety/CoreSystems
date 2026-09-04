import type { ActionResult, ClipboardController, SessionActor } from '../types/public.js';
import type { ActionExecutor } from '../actions/ActionExecutor.js';

export class ClipboardControllerImpl implements ClipboardController {
  private lastText: string | undefined;

  constructor(
    private readonly executor: ActionExecutor,
    private readonly actor: SessionActor = 'human',
  ) {}

  get lastKnownText(): string | undefined {
    return this.lastText;
  }

  async write(text: string): Promise<ActionResult> {
    const result = await this.executor.execute({ type: 'clipboard.write', actor: this.actor, text });
    if (result.ok) this.lastText = text;
    return result;
  }

  /** Kalles av økten når protokollen rapporterer at den eksterne utklippstavlen endret seg. */
  handleRemoteUpdate(text: string | undefined): void {
    this.lastText = text;
  }
}
