import type {
  ConnectOptions,
  DisconnectReason,
  RemoteClient as IRemoteClient,
  RemoteSession,
  SessionAuthenticator,
} from '../types/public.js';
import type { RemoteProtocol } from '../protocol/RemoteProtocol.js';
import type { Logger } from '../logging/Logger.js';
import { noopLogger } from '../logging/Logger.js';
import { AuthenticationError, RemoteConnectionError, RemoteError } from '../errors/RemoteError.js';
import { SessionStateMachine } from './SessionStateMachine.js';
import { RemoteSessionImpl } from './RemoteSession.js';

export interface RemoteClientOptions {
  readonly protocol: RemoteProtocol;
  readonly logger?: Logger;
  /**
   * Oversetter `auth.token`-snarveien til en konkret autentikator. Bevisst
   * injisert i stedet for importert direkte — RemoteClientImpl skal ikke vite
   * at RustDesk finnes ("RustDesk protocol isolation"). `createRemoteClient`
   * sin standardgren kobler denne til RustDeskPasswordAuthenticator; et
   * kall med en egendefinert `protocol` og ingen fabrikk må sende
   * `authenticator` eksplisitt i stedet for `auth.token`.
   */
  readonly createAuthenticatorFromToken?: (token: string) => SessionAuthenticator;
}

export class RemoteClientImpl implements IRemoteClient {
  private readonly protocol: RemoteProtocol;
  private readonly logger: Logger;
  private readonly createAuthenticatorFromToken: ((token: string) => SessionAuthenticator) | undefined;

  constructor(options: RemoteClientOptions) {
    this.protocol = options.protocol;
    this.logger = options.logger ?? noopLogger;
    this.createAuthenticatorFromToken = options.createAuthenticatorFromToken;
  }

  async connect(options: ConnectOptions): Promise<RemoteSession> {
    const authenticator = this.resolveAuthenticator(options);
    const stateMachine = new SessionStateMachine();

    // Protokollen rapporterer sine egne fremgangsfaser (rendezvous,
    // authenticating, ...) via 'state'-hendelser mens connect() fortsatt
    // kjører. Vi speiler dem inn i en lokal tilstandsmaskin slik at et
    // ulovlig hopp fra selve protokollimplementasjonen blir oppdaget her.
    const unsubscribeState = this.protocol.on('state', (change) => {
      if (stateMachine.canTransition(change.to)) {
        stateMachine.transition(change.to);
      }
    });

    try {
      stateMachine.transition('connecting');
      await this.protocol.connect({ deviceId: options.deviceId, authenticator, signal: options.signal });
    } catch (error) {
      unsubscribeState();
      if (stateMachine.state !== 'failed' && stateMachine.state !== 'disconnected') {
        stateMachine.transition('failed', reasonForError(error));
      }
      throw normalizeConnectError(error);
    }

    unsubscribeState();

    if (stateMachine.state !== 'connected') {
      const error = new RemoteConnectionError(
        `Protocol resolved connect() without reaching the "connected" state (ended in "${stateMachine.state}")`,
      );
      this.logger.error(error.message);
      if (stateMachine.canTransition('failed')) stateMachine.transition('failed', 'protocol-error');
      throw error;
    }

    return new RemoteSessionImpl({
      id: crypto.randomUUID(),
      protocol: this.protocol,
      stateMachine,
      logger: this.logger,
    });
  }

  private resolveAuthenticator(options: ConnectOptions): SessionAuthenticator {
    if (options.authenticator) return options.authenticator;
    if (options.auth?.token) {
      if (!this.createAuthenticatorFromToken) {
        throw new AuthenticationError(
          'This protocol does not support the "auth.token" shorthand — pass "authenticator" explicitly.',
        );
      }
      return this.createAuthenticatorFromToken(options.auth.token);
    }
    throw new AuthenticationError('connect() requires either "authenticator" or "auth.token"');
  }
}

function normalizeConnectError(error: unknown): RemoteError {
  if (error instanceof RemoteError) return error;
  const message = error instanceof Error ? error.message : String(error);
  return new RemoteConnectionError(message);
}

function reasonForError(error: unknown): DisconnectReason {
  if (error instanceof AuthenticationError) return 'authentication-failed';
  return 'protocol-error';
}
