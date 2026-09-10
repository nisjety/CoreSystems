import type { Logger } from '../../logging/Logger.js';
import { EncryptionError } from '../../errors/RemoteError.js';
import {
  SessionChannel,
  generateBoxKeyPair,
  generateSessionKey,
  openSignedMessage,
  sealSessionKey,
} from '../../crypto/SessionCrypto.js';
import { decodeIdPk } from './messages/sessionCodec.js';
import type { RdPublicKey, RdSignedId } from './messages/types.js';

export interface PeerIdentity {
  /** Motpartens peer-ID slik serveren vouchet for den. */
  readonly id: string;
  /** Motpartens langlevde Ed25519-identitetsnøkkel. */
  readonly signPublicKey: Uint8Array;
}

/**
 * Verifiserer serverens vouch for en motpart. RustDesk lar ID-serveren
 * signere `IdPk{id, pk}` med sin egen langlevde Ed25519-nøkkel og sender det
 * som `PunchHoleResponse.pk` / `RelayResponse.pk` — en sertifikat-lignende
 * bekreftelse på at "peer-ID X eier denne identitetsnøkkelen". Uten
 * serverens offentlige nøkkel kan vi ikke vite hvem vi snakker med.
 */
export async function verifyServerVouch(
  vouch: Uint8Array,
  serverPublicKey: Uint8Array,
): Promise<PeerIdentity> {
  const payload = await openSignedMessage(vouch, serverPublicKey);
  const idPk = decodeIdPk(payload);
  if (idPk.pk.length === 0) {
    throw new EncryptionError('Server vouch contained no peer identity key');
  }
  return { id: idPk.id, signPublicKey: idPk.pk };
}

export interface SecureChannelOptions {
  /**
   * Motpartens identitet, hentet fra serverens vouch. Når denne mangler kan
   * vi kryptere, men ikke AUTENTISERE motparten — se `allowUnverifiedPeer`.
   */
  readonly peerIdentity?: PeerIdentity;
  /**
   * Tillater håndtrykk uten å verifisere motpartens identitet. Standard er
   * false, og det skal den være i produksjon: RustDesk sin egen klient faller
   * stille tilbake til uautentisert/klartekst P2P når serveren ikke har
   * signert noen nøkkel, og docs/security.md krever at Verevon i stedet
   * feiler lukket. Kun for lokal utvikling mot en nøkkelløs testserver.
   */
  readonly allowUnverifiedPeer?: boolean;
  readonly logger: Logger;
}

/**
 * Ende-til-ende-kryptering av selve øktinnholdet, uavhengig av transport.
 * Vi er ALLTID den tilkoblende siden, som i RustDesk sitt håndtrykk betyr at
 * motparten sender `SignedId` først og vi svarer med `PublicKey` — vi trenger
 * derfor ingen egen langlevd identitetsnøkkel, bare et efemert X25519-par
 * for å forsegle øktnøkkelen med.
 *
 * Meldinger FØR håndtrykket (SignedId/PublicKey selv) går i klartekst;
 * alt etter går gjennom secretbox. `isEncrypted` sier hvilken fase vi er i.
 */
export class SecureChannel {
  private channel: SessionChannel | undefined;
  private readonly options: SecureChannelOptions;

  constructor(options: SecureChannelOptions) {
    this.options = options;
  }

  get isEncrypted(): boolean {
    return this.channel !== undefined;
  }

  /**
   * Tar imot motpartens `SignedId`, verifiserer den mot identiteten serveren
   * vouchet for, og returnerer `PublicKey`-meldingen som skal sendes tilbake.
   * Etter dette kallet er kanalen kryptert.
   */
  async acceptSignedId(signedId: RdSignedId): Promise<RdPublicKey> {
    if (this.channel) {
      throw new EncryptionError('Secure channel handshake already completed');
    }
    if (signedId.id.length === 0) {
      // En tom SignedId er RustDesk sin måte å si "jeg har ingen signert
      // nøkkel" — som betyr uautentisert økt. Vi nekter i stedet for å
      // fortsette i klartekst.
      throw new EncryptionError(
        'Peer sent an empty SignedId (no signed key available) — refusing to continue unauthenticated',
      );
    }

    const peerEphemeralPublicKey = await this.recoverPeerEphemeralKey(signedId);

    const sessionKey = await generateSessionKey();
    const ephemeral = await generateBoxKeyPair();
    const sealed = await sealSessionKey(sessionKey, peerEphemeralPublicKey, ephemeral.secretKey);

    this.channel = new SessionChannel(sessionKey);

    return { asymmetricValue: ephemeral.publicKey, symmetricValue: sealed };
  }

  private async recoverPeerEphemeralKey(signedId: RdSignedId): Promise<Uint8Array> {
    const identity = this.options.peerIdentity;

    if (!identity) {
      if (this.options.allowUnverifiedPeer !== true) {
        throw new EncryptionError(
          'No verified peer identity available — refusing an unauthenticated handshake. ' +
            'Configure the rendezvous server public key, or set allowUnverifiedPeer for local development only.',
        );
      }
      // Uten identitetsnøkkel kan vi ikke åpne den påhengte signaturen, så
      // det eneste vi kan gjøre er å lese nyttelasten bak de 64
      // signaturbytene uten å verifisere den. Dette gir kryptering uten
      // autentisering — altså ingen MITM-beskyttelse.
      this.options.logger.warn(
        'Accepting an UNVERIFIED peer identity — the session is encrypted but not authenticated',
      );
      const unverifiedPayload = signedId.id.subarray(SIGNATURE_BYTES);
      const idPk = decodeIdPk(unverifiedPayload);
      if (idPk.pk.length === 0) {
        throw new EncryptionError('Peer SignedId contained no ephemeral key');
      }
      return idPk.pk;
    }

    const payload = await openSignedMessage(signedId.id, identity.signPublicKey);
    const idPk = decodeIdPk(payload);
    if (idPk.pk.length === 0) {
      throw new EncryptionError('Peer SignedId contained no ephemeral key');
    }
    if (idPk.id !== identity.id) {
      throw new EncryptionError('Peer SignedId does not match the identity the server vouched for', {
        expected: identity.id,
        received: idPk.id,
      });
    }
    return idPk.pk;
  }

  // `async` er bevisst: en funksjon som returnerer Promise men kaster
  // SYNKRONT er en felle for kallere som bruker .catch() i stedet for await.
  // Feil skal alltid komme som en avvist promise.
  async encrypt(plaintext: Uint8Array): Promise<Uint8Array> {
    if (!this.channel) {
      throw new EncryptionError('Cannot encrypt before the secure channel handshake has completed');
    }
    return this.channel.encrypt(plaintext);
  }

  async decrypt(ciphertext: Uint8Array): Promise<Uint8Array> {
    if (!this.channel) {
      throw new EncryptionError('Cannot decrypt before the secure channel handshake has completed');
    }
    return this.channel.decrypt(ciphertext);
  }
}

/** Ed25519 påhengt signatur: 64 byte foran nyttelasten. */
const SIGNATURE_BYTES = 64;
