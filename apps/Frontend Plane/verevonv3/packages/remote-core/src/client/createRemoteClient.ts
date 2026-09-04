import type { RemoteClient } from '../types/public.js';
import type { RemoteProtocol } from '../protocol/RemoteProtocol.js';
import type { Logger } from '../logging/Logger.js';
import { RemoteConnectionError } from '../errors/RemoteError.js';
import { RemoteClientImpl } from './RemoteClient.js';

export interface CreateRemoteClientOptions {
  /**
   * En eksplisitt protokollimplementasjon. Dette er i dag den ENESTE måten å
   * få en fungerende klient på — se docs/architecture.md, "Status": den
   * fullstendige RustDesk-orkestreringen (rendezvous- og relé-klientene som
   * knytter kryptolaget og meldingskodekene sammen til en reell
   * connect()-sekvens) er ikke ferdigstilt ennå. Bygg-blokkene den vil bruke
   * (protocol/rustdesk/wire, protocol/rustdesk/messages,
   * RustDeskPasswordAuthenticator, crypto/SessionCrypto) er reelle og testet
   * hver for seg.
   */
  readonly protocol: RemoteProtocol;
  readonly logger?: Logger;
}

/**
 * Fabrikk for RemoteClient. Holder RustDesk-spesifikk kobling (auth.token ->
 * RustDeskPasswordAuthenticator) UTENFOR RemoteClientImpl selv — se
 * "RustDesk protocol isolation" i docs/architecture.md.
 */
export function createRemoteClient(options: CreateRemoteClientOptions): RemoteClient {
  if (!options.protocol) {
    throw new RemoteConnectionError('createRemoteClient requires an explicit "protocol"');
  }
  return new RemoteClientImpl({ protocol: options.protocol, logger: options.logger });
}
