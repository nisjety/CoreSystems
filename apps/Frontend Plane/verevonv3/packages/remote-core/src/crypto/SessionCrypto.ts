import sodium from 'libsodium-wrappers';
import { EncryptionError } from '../errors/RemoteError.js';

/**
 * Tynn wrapper rundt libsodium (WASM) — SAMME kryptobibliotek RustDesk selv
 * bruker via sodiumoxide (Rust-bindingene), ikke en håndrullet
 * kryptoprimitiv. Se docs/rustdesk-protocol.md, "Encryption / handshake".
 */

export interface KeyPair {
  readonly publicKey: Uint8Array;
  readonly secretKey: Uint8Array;
}

let readyPromise: Promise<typeof sodium> | undefined;

async function ready(): Promise<typeof sodium> {
  readyPromise ??= sodium.ready.then(() => sodium);
  return readyPromise;
}

export async function generateBoxKeyPair(): Promise<KeyPair> {
  const s = await ready();
  const pair = s.crypto_box_keypair();
  return { publicKey: pair.publicKey, secretKey: pair.privateKey };
}

export async function generateSignKeyPair(): Promise<KeyPair> {
  const s = await ready();
  const pair = s.crypto_sign_keypair();
  return { publicKey: pair.publicKey, secretKey: pair.privateKey };
}

export async function generateSessionKey(): Promise<Uint8Array> {
  const s = await ready();
  return s.crypto_secretbox_keygen();
}

/**
 * Forsegler en øktnøkkel til mottakerens X25519-nøkkel med en helt-null
 * nonce. Trygt her fordi avsenderens nøkkelpar er engangsbruk — generert
 * friskt for akkurat dette håndtrykket og aldri gjenbrukt — speiler RustDesk
 * sin faktiske bruk av crypto_box (se docs/rustdesk-protocol.md).
 */
export async function sealSessionKey(
  sessionKey: Uint8Array,
  recipientPublicKey: Uint8Array,
  senderSecretKey: Uint8Array,
): Promise<Uint8Array> {
  const s = await ready();
  const zeroNonce = new Uint8Array(s.crypto_box_NONCEBYTES);
  return s.crypto_box_easy(sessionKey, zeroNonce, recipientPublicKey, senderSecretKey);
}

export async function openSealedSessionKey(
  sealed: Uint8Array,
  senderPublicKey: Uint8Array,
  recipientSecretKey: Uint8Array,
): Promise<Uint8Array> {
  const s = await ready();
  const zeroNonce = new Uint8Array(s.crypto_box_NONCEBYTES);
  try {
    // libsodium-wrappers throws on a bad key/tampered ciphertext rather than
    // returning a falsy value — normalize that into our own typed error so
    // callers never see a raw, unredacted libsodium exception.
    return s.crypto_box_open_easy(sealed, zeroNonce, senderPublicKey, recipientSecretKey);
  } catch {
    throw new EncryptionError('Failed to open sealed session key — wrong keypair or tampered data');
  }
}

export async function signDetached(message: Uint8Array, secretKey: Uint8Array): Promise<Uint8Array> {
  const s = await ready();
  return s.crypto_sign_detached(message, secretKey);
}

export async function verifyDetached(
  signature: Uint8Array,
  message: Uint8Array,
  publicKey: Uint8Array,
): Promise<boolean> {
  const s = await ready();
  return s.crypto_sign_verify_detached(signature, message, publicKey);
}

/**
 * ⚠️ REPLIKERER EN BEKREFTET SVAKHET I RUSTDESK SELV — se docs/security.md,
 * "Nonce reuse in RustDesk's own peer-to-peer encryption" for full forklaring
 * og kildehenvisning. Kort versjon: hbb_common/src/tcp.rs sin Encrypt-struct
 * bruker ÉN delt 32-byte nøkkel for BEGGE retninger, med to UAVHENGIGE
 * tellere som begge starter på 0 og øker med 1 per kall — ingen rollebit,
 * ingen forskjell i nonce utover tellerverdien. Det betyr at klientens N-te
 * sendte melding og vertens N-te sendte melding bruker (nøkkel, nonce) =
 * (K, LE64(N)) — samme par, forskjellig klartekst — for HVER N begge sider
 * når, ikke bare N=1. Dette ER reelt nonce-gjenbruk for XSalsa20-Poly1305,
 * bekreftet direkte i kilden, ikke en hypotese.
 *
 * Denne klassen implementerer likevel EKSAKT samme oppførsel, fordi
 * trådkompatibilitet med en ekte RustDesk-basert Verevon Agent krever det —
 * å avlede separate nøkler per retning her ville bryte interoperabilitet med
 * en umodifisert motpart. Se docs/security.md for hvorfor WSS til
 * Support Plane IKKE avhjelper dette (det beskytter kun nettleser<->server-
 * hoppet, ikke selve ende-til-ende-øktinnholdet mot en kompromittert relé
 * eller en avlytter som ser begge retningers chiffertekst).
 */
export class DirectionalCipher {
  private counter = 0n;

  constructor(private readonly key: Uint8Array) {}

  private async nextNonce(): Promise<Uint8Array> {
    const s = await ready();
    this.counter += 1n;
    const nonce = new Uint8Array(s.crypto_secretbox_NONCEBYTES);
    new DataView(nonce.buffer).setBigUint64(0, this.counter, true);
    return nonce;
  }

  async seal(plaintext: Uint8Array): Promise<Uint8Array> {
    const s = await ready();
    const nonce = await this.nextNonce();
    return s.crypto_secretbox_easy(plaintext, nonce, this.key);
  }

  async open(ciphertext: Uint8Array): Promise<Uint8Array> {
    const s = await ready();
    const nonce = await this.nextNonce();
    try {
      return s.crypto_secretbox_open_easy(ciphertext, nonce, this.key);
    } catch {
      throw new EncryptionError('Failed to decrypt session message — bad key or nonce desync');
    }
  }
}

/** Kombinerer en send- og en mottaks-DirectionalCipher over samme nøkkel. Se advarselen på DirectionalCipher. */
export class SessionChannel {
  private readonly sendCipher: DirectionalCipher;
  private readonly recvCipher: DirectionalCipher;

  constructor(sessionKey: Uint8Array) {
    this.sendCipher = new DirectionalCipher(sessionKey);
    this.recvCipher = new DirectionalCipher(sessionKey);
  }

  encrypt(plaintext: Uint8Array): Promise<Uint8Array> {
    return this.sendCipher.seal(plaintext);
  }

  decrypt(ciphertext: Uint8Array): Promise<Uint8Array> {
    return this.recvCipher.open(ciphertext);
  }
}
