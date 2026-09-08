import type { Transport } from '../../transport/Transport.js';
import type { Logger } from '../../logging/Logger.js';
import { RemoteConnectionError, TransportError } from '../../errors/RemoteError.js';
import { MessageInbox } from './MessageInbox.js';
import {
  decodeRendezvousMessage,
  encodeRendezvousMessage,
  type IncomingRendezvousMessage,
} from './messages/envelope.js';
import { ConnType, NatType, PunchHoleFailure } from './messages/types.js';

export interface RendezvousRequest {
  readonly deviceId: string;
  readonly licenceKey: string;
  readonly token: string;
  readonly clientVersion: string;
  readonly timeoutMs: number;
}

export interface RelayNegotiation {
  /** Adressen hbbr skal nås på, slik serveren oppgav den. Tom = bruk konfigurert standard. */
  readonly relayServer: string;
  /** Øktnøkkelen (session token) begge sider oppgir til hbbr for å bli paret. */
  readonly uuid: string;
  /** Serverens signerte vouch for motparten, hvis serveren sendte en. Tom = ingen. */
  readonly peerVouch: Uint8Array;
}

function describePunchHoleFailure(failure: number, otherFailure: string): string {
  if (otherFailure) return otherFailure;
  switch (failure) {
    case PunchHoleFailure.IdNotExist:
      return 'The rendezvous server does not know that device ID';
    case PunchHoleFailure.Offline:
      return 'The remote device is offline';
    case PunchHoleFailure.LicenseMismatch:
      return 'Rendezvous server license key mismatch';
    case PunchHoleFailure.LicenseOveruse:
      return 'Rendezvous server license is over its device limit';
    default:
      return `Rendezvous server refused the connection (failure code ${failure})`;
  }
}

/**
 * Fører samtalen med hbbs (rendezvous-serveren) over én transport, og ender
 * opp med det vi trenger for å koble til hbbr.
 *
 * Vi ber ALLTID om relé (`forceRelay: true`). En nettleser kan ikke gjøre
 * UDP-hullpunsjing i det hele tatt, og hbbs har et førsteklasses flagg for å
 * hoppe rett til relé — se docs/rustdesk-protocol.md.
 *
 * MERK om de to svarveiene: forskningen fikk ikke entydig fastslått om en
 * force_relay-forespørsel besvares med `PunchHoleResponse` (og at klienten
 * så selv genererer en uuid og sender `RequestRelay`) eller direkte med
 * `RelayResponse` (som selv bærer uuid + relay-adresse + vouch). Begge
 * feltsettene finnes i skjemaet og begge er plausible, så vi håndterer
 * BEGGE i stedet for å gamble på én lesning. Dette er nøyaktig punktet som
 * bør bekreftes mot en kjørende hbbs først.
 */
export class RendezvousClient {
  private readonly inbox = new MessageInbox<IncomingRendezvousMessage>();
  private readonly unsubscribers: Array<() => void> = [];

  constructor(
    private readonly transport: Transport,
    private readonly logger: Logger,
  ) {}

  async negotiateRelay(request: RendezvousRequest): Promise<RelayNegotiation> {
    await this.transport.connect();

    this.unsubscribers.push(
      this.transport.onMessage((data) => {
        this.inbox.push(decodeRendezvousMessage(data));
      }),
      this.transport.onClose((info) => {
        this.inbox.close(
          new TransportError('Rendezvous connection closed before relay negotiation completed', {
            code: info.code,
            wasClean: info.wasClean,
          }),
        );
      }),
      this.transport.onError(() => {
        this.inbox.close(new TransportError('Rendezvous transport error during relay negotiation'));
      }),
    );

    this.transport.send(
      encodeRendezvousMessage({
        kind: 'punchHoleRequest',
        value: {
          id: request.deviceId,
          natType: NatType.Unknown,
          licenceKey: request.licenceKey,
          connType: ConnType.DefaultConn,
          token: request.token,
          version: request.clientVersion,
          forceRelay: true,
        },
      }),
    );

    const response = await this.inbox.next(
      (message) => message.kind === 'punchHoleResponse' || message.kind === 'relayResponse',
      request.timeoutMs,
      () =>
        new RemoteConnectionError('Rendezvous server did not answer the connection request in time', {
          timeoutMs: request.timeoutMs,
        }),
    );

    if (response.kind === 'relayResponse') {
      const { value } = response;
      if (value.refuseReason) {
        throw new RemoteConnectionError(`Rendezvous server refused the relay: ${value.refuseReason}`);
      }
      if (!value.uuid) {
        throw new RemoteConnectionError('Relay response carried no session token (uuid)');
      }
      return { relayServer: value.relayServer, uuid: value.uuid, peerVouch: value.pk };
    }

    if (response.kind !== 'punchHoleResponse') {
      // Kan ikke skje med predikatet over, men et uventet meldingsformat skal
      // feile tydelig i stedet for å bli tolket som noe det ikke er.
      throw new RemoteConnectionError('Rendezvous server sent an unexpected reply to the connection request', {
        kind: response.kind,
      });
    }

    const { value } = response;
    // `failure` alene er ikke en pålitelig indikator (IdNotExist=0 er også
    // proto3 sin fraværsverdi) — fravær av BÅDE relay-adresse og vouch er
    // det som faktisk skiller en avvisning fra et brukbart svar.
    if (!value.relayServer && value.pk.length === 0) {
      throw new RemoteConnectionError(describePunchHoleFailure(value.failure, value.otherFailure), {
        failure: value.failure,
      });
    }

    // PunchHoleResponse-veien: vi velger selv økt-tokenet og ber hbbs
    // videreformidle det til motparten, som så møter oss på hbbr.
    const uuid = crypto.randomUUID();
    this.logger.debug('Rendezvous answered with punch-hole response; requesting relay', { uuid });

    this.transport.send(
      encodeRendezvousMessage({
        kind: 'requestRelay',
        value: {
          id: request.deviceId,
          uuid,
          relayServer: value.relayServer,
          secure: true,
          licenceKey: request.licenceKey,
          connType: ConnType.DefaultConn,
          token: request.token,
        },
      }),
    );

    return { relayServer: value.relayServer, uuid, peerVouch: value.pk };
  }

  async close(): Promise<void> {
    for (const unsubscribe of this.unsubscribers) unsubscribe();
    this.unsubscribers.length = 0;
    this.inbox.close(new TransportError('Rendezvous client closed'));
    await this.transport.close();
  }
}
