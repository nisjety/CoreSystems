import type { RemoteClient } from '../types/public.js';
import type { RemoteProtocol } from '../protocol/RemoteProtocol.js';
import type { Logger } from '../logging/Logger.js';
import { RemoteConnectionError } from '../errors/RemoteError.js';
import { RemoteClientImpl } from './RemoteClient.js';
import type { ReconnectPolicy } from './Reconnector.js';
import { createRustDeskProtocol } from '../protocol/rustdesk/createRustDeskProtocol.js';
import type { RustDeskProtocolOptions } from '../protocol/rustdesk/RustDeskProtocol.js';
import { RustDeskPasswordAuthenticator } from '../protocol/rustdesk/RustDeskPasswordAuthenticator.js';

/**
 * To bruksmønstre, begge støttet:
 *
 *  1. Kort vei — oppgi `rendezvousUrl` (+ eventuelt `relayUrl`), og du får en
 *     RustDesk-basert klient der `connect({ auth: { token } })` fungerer
 *     direkte.
 *  2. Eksplisitt — oppgi din egen `protocol`. Da er `auth.token`-snarveien
 *     ikke tilgjengelig (vi vet ikke hvordan en vilkårlig protokoll vil ha
 *     legitimasjonen sin), så send `authenticator` i stedet.
 */
export interface CreateRemoteClientOptions extends Partial<RustDeskProtocolOptions> {
  readonly protocol?: RemoteProtocol;
  readonly logger?: Logger;
  readonly reconnectPolicy?: ReconnectPolicy;
}

export function createRemoteClient(options: CreateRemoteClientOptions): RemoteClient {
  const { logger } = options;

  if (options.protocol) {
    return new RemoteClientImpl({ protocol: options.protocol, logger, reconnectPolicy: options.reconnectPolicy });
  }

  if (!options.rendezvousUrl) {
    throw new RemoteConnectionError(
      'createRemoteClient requires either an explicit "protocol" or a "rendezvousUrl"',
    );
  }

  const protocol = createRustDeskProtocol({ ...options, rendezvousUrl: options.rendezvousUrl, logger });

  return new RemoteClientImpl({
    protocol,
    logger,
    // Kun standardgrenen kobler token-snarveien til RustDesk sitt
    // passordoppsett — RemoteClientImpl selv vet ikke at RustDesk finnes.
    createAuthenticatorFromToken: (token, secondFactor) =>
      new RustDeskPasswordAuthenticator(token, { secondFactor }),
    reconnectPolicy: options.reconnectPolicy,
  });
}
