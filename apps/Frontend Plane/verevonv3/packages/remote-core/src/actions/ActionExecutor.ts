import type { ActionMiddleware, ActionResult, RemoteAction } from '../types/public.js';
import type { RemoteProtocol } from '../protocol/RemoteProtocol.js';
import type { PermissionManager } from '../permissions/PermissionManager.js';
import type { Logger } from '../logging/Logger.js';
import { requiredPermissionFor } from '../types/internal.js';
import { noopLogger } from '../logging/Logger.js';

/**
 * Sentralt sted ALLE handlinger (menneske eller AI) går gjennom. Rekkefølgen
 * er bevisst: obligatorisk tillatelseskontroll først (kan ikke omgås av
 * middleware — "Never assume control permissions from screen-view
 * permission"), deretter den valgfrie middleware-kjeden (AI-godkjenning,
 * revisjonslogging, hastighetsbegrensning), og til slutt selve
 * protokollkallet.
 */
export class ActionExecutor {
  private readonly middlewares: ActionMiddleware[] = [];

  constructor(
    private readonly protocol: RemoteProtocol,
    private readonly permissions: PermissionManager,
    private readonly logger: Logger = noopLogger,
  ) {}

  use(middleware: ActionMiddleware): void {
    this.middlewares.push(middleware);
  }

  async execute(action: RemoteAction): Promise<ActionResult> {
    this.permissions.require(requiredPermissionFor(action));

    const terminal = (): Promise<ActionResult> => this.protocol.sendAction(action);
    const chain = this.middlewares.reduceRight<() => Promise<ActionResult>>(
      (next, middleware) => () => middleware(action, next),
      terminal,
    );

    try {
      return await chain();
    } catch (error) {
      this.logger.warn('Action execution failed', { type: action.type, actor: action.actor });
      throw error;
    }
  }
}
