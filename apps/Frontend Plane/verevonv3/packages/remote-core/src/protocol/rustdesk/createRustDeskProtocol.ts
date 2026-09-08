import type { RemoteProtocol } from '../RemoteProtocol.js';
import { RemoteConnectionError } from '../../errors/RemoteError.js';
import { RustDeskProtocol, type RustDeskProtocolOptions } from './RustDeskProtocol.js';

/**
 * Bygger en RustDesk-basert RemoteProtocol. Dette er den ENESTE eksporten
 * utenfor protocol/rustdesk/* som nevner RustDesk ved navn — resten av
 * biblioteket ser bare `RemoteProtocol`.
 */
export function createRustDeskProtocol(options: RustDeskProtocolOptions): RemoteProtocol {
  if (!options.rendezvousUrl) {
    throw new RemoteConnectionError('createRustDeskProtocol requires "rendezvousUrl"');
  }
  return new RustDeskProtocol(options);
}
