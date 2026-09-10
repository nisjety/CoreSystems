import sodium from 'libsodium-wrappers';
import { SessionChannel, openSealedSessionKey } from '../../src/crypto/SessionCrypto.js';
import type { MockTransport } from './MockTransport.js';
import {
  encodeHashMessage,
  encodeIdPk,
  encodeLoginResponseWithError,
  encodeLoginResponseWithPeerInfo,
  encodeSignedIdMessage,
  encodeSwitchDisplayMessage,
  encodeTestDelayMessage,
  type FakeDisplay,
} from './peerEncoders.js';
import { decodePeerMessage, decodePeerRendezvousMessage, type PeerMessage } from './peerDecoders.js';

function toBase64(bytes: Uint8Array): string {
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

async function sha256(bytes: Uint8Array<ArrayBuffer>): Promise<Uint8Array<ArrayBuffer>> {
  return new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
}

function concat(a: Uint8Array<ArrayBuffer>, b: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(a.length + b.length);
  out.set(a, 0);
  out.set(b, a.length);
  return out;
}

function bytesEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  return a.every((value, index) => value === b[index]);
}

export interface FakeRustDeskPeerOptions {
  readonly deviceId: string;
  readonly secret: string;
  readonly salt?: string;
  readonly challenge?: string;
  readonly displays?: readonly FakeDisplay[];
  /**
   * When set, a correct password is answered with "2FA Required" and the
   * host then expects exactly this TOTP code in an Auth2FA message — the
   * sequence RustDesk's server/connection.rs drives.
   */
  readonly require2FACode?: string;
}

/**
 * A stand-in for the controlled host (the native Verevon Agent) that plays
 * the *other* side of the RustDesk handshake for real: it signs its own
 * identity, is vouched for by a fake rendezvous-server key, completes the
 * X25519 key exchange, and then speaks secretbox-encrypted protobuf.
 *
 * This is what makes the protocol implementation testable without live
 * infrastructure. It verifies the handshake sequence, the two-level signature
 * chain, nonce/counter synchronisation in both directions, and the login
 * challenge-response — everything except whether our field numbers match the
 * real RustDesk wire format, which only a live server can confirm.
 */
export class FakeRustDeskPeer {
  private channel: SessionChannel | undefined;
  private paired = false;
  private relay: MockTransport | undefined;
  private chain: Promise<void> = Promise.resolve();

  readonly received: PeerMessage[] = [];
  loginPasswordMatched: boolean | undefined;
  /** Set this to reject the next login regardless of the password. */
  rejectLoginWith: string | undefined;
  /** Index of the display the host is currently streaming (after a confirmed switch). */
  currentDisplay = 0;
  /**
   * Answer an Auth2FA with another "2FA Required" instead of accepting or
   * rejecting it — what a host does when the TOTP window rolled over.
   */
  reissueChallengeOnAuth2FA = false;
  private awaiting2FA = false;

  private constructor(
    readonly deviceId: string,
    private readonly secret: string,
    readonly salt: string,
    readonly challenge: string,
    private readonly serverKeys: { publicKey: Uint8Array; privateKey: Uint8Array },
    private readonly hostEphemeral: { publicKey: Uint8Array; privateKey: Uint8Array },
    readonly vouch: Uint8Array,
    readonly signedId: Uint8Array,
    private readonly displays: readonly FakeDisplay[],
    private readonly require2FACode: string | undefined,
  ) {}

  static async create(options: FakeRustDeskPeerOptions): Promise<FakeRustDeskPeer> {
    await sodium.ready;

    const serverKeys = sodium.crypto_sign_keypair();
    const hostIdentity = sodium.crypto_sign_keypair();
    const hostEphemeral = sodium.crypto_box_keypair();

    // Level 1: the rendezvous server vouches for the host's identity key.
    const vouch = sodium.crypto_sign(
      encodeIdPk(options.deviceId, hostIdentity.publicKey),
      serverKeys.privateKey,
    );
    // Level 2: the host's identity key signs its ephemeral exchange key.
    const signedId = sodium.crypto_sign(
      encodeIdPk(options.deviceId, hostEphemeral.publicKey),
      hostIdentity.privateKey,
    );

    return new FakeRustDeskPeer(
      options.deviceId,
      options.secret,
      options.salt ?? 'host-salt',
      options.challenge ?? 'one-time-challenge',
      serverKeys,
      hostEphemeral,
      vouch,
      signedId,
      options.displays ?? [{ name: 'Built-in Display', width: 1920, height: 1080 }],
      options.require2FACode,
    );
  }

  get serverPublicKeyBase64(): string {
    return toBase64(this.serverKeys.publicKey);
  }

  /** A vouch signed by the WRONG key, for negative tests. */
  static async forgedVouch(deviceId: string): Promise<Uint8Array> {
    await sodium.ready;
    const impostor = sodium.crypto_sign_keypair();
    const identity = sodium.crypto_sign_keypair();
    return sodium.crypto_sign(encodeIdPk(deviceId, identity.publicKey), impostor.privateKey);
  }

  attachRelay(transport: MockTransport): void {
    this.relay = transport;
    transport.onSend = (data) => {
      // Serialised the same way the real peer must: secretbox counters make
      // both directions strictly order-dependent.
      this.chain = this.chain.then(() => this.process(data));
    };
  }

  /** Resolves once every queued exchange has been processed. */
  async settle(): Promise<void> {
    await this.chain;
  }

  async sendEncrypted(payload: Uint8Array): Promise<void> {
    const channel = this.channel;
    const relay = this.relay;
    if (!channel || !relay) throw new Error('FakeRustDeskPeer: no encrypted channel yet');
    relay.deliver(await channel.encrypt(payload));
  }

  /**
   * Forget the current relay pairing and encrypted channel, as a real host
   * does when the controller's connection drops: the next RequestRelay starts
   * a brand-new handshake with the same identity keys and vouch.
   */
  resetPairing(): void {
    this.paired = false;
    this.channel = undefined;
    this.awaiting2FA = false;
  }

  /** The host's periodic RTT probe; a compliant controller must echo it back verbatim. */
  sendTestDelay(lastDelay: number, time = BigInt(Date.now())): Promise<void> {
    return this.sendEncrypted(encodeTestDelayMessage({ time, fromClient: false, lastDelay }));
  }

  private async process(data: Uint8Array): Promise<void> {
    const relay = this.relay;
    if (!relay) return;

    // hbbr only ever parses the FIRST message (the RequestRelay pairing);
    // everything after it is peer-to-peer `Message` traffic.
    if (!this.paired) {
      const rendezvous = decodePeerRendezvousMessage(data);
      if (rendezvous.kind === 'requestRelay') {
        this.paired = true;
        relay.deliver(encodeSignedIdMessage(this.signedId));
      }
      return;
    }

    if (!this.channel) {
      const message = decodePeerMessage(data);
      this.received.push(message);
      if (message.kind === 'publicKey') {
        const sessionKey = await openSealedSessionKey(
          message.symmetricValue,
          message.asymmetricValue,
          this.hostEphemeral.privateKey,
        );
        this.channel = new SessionChannel(sessionKey);
        await this.sendEncrypted(encodeHashMessage(this.salt, this.challenge));
      }
      return;
    }

    const plaintext = await this.channel.decrypt(data);
    const message = decodePeerMessage(plaintext);
    this.received.push(message);

    switch (message.kind) {
      case 'loginRequest': {
        const expected = await this.expectedPasswordHash();
        this.loginPasswordMatched = bytesEqual(message.password, expected);

        if (this.rejectLoginWith) {
          await this.sendEncrypted(encodeLoginResponseWithError(this.rejectLoginWith));
          return;
        }
        if (!this.loginPasswordMatched) {
          await this.sendEncrypted(encodeLoginResponseWithError('Wrong Password'));
          return;
        }
        if (this.require2FACode !== undefined) {
          // Password accepted, second factor still outstanding.
          this.awaiting2FA = true;
          await this.sendEncrypted(encodeLoginResponseWithError('2FA Required'));
          return;
        }
        await this.sendLogonResponse();
        return;
      }
      case 'auth2fa': {
        if (!this.awaiting2FA) return; // unsolicited — a real host ignores it too
        if (this.reissueChallengeOnAuth2FA) {
          await this.sendEncrypted(encodeLoginResponseWithError('2FA Required'));
          return;
        }
        if (message.code === this.require2FACode) {
          this.awaiting2FA = false;
          await this.sendLogonResponse();
        } else {
          await this.sendEncrypted(encodeLoginResponseWithError('Wrong 2FA Code'));
        }
        return;
      }
      case 'testDelay': {
        // The host echoes client-originated probes unchanged; the controller's
        // echoes of OUR probes (from_client=false) just get recorded above.
        if (message.fromClient) {
          await this.sendEncrypted(
            encodeTestDelayMessage({ time: message.time, fromClient: true, lastDelay: message.lastDelay }),
          );
        }
        return;
      }
      case 'switchDisplay': {
        const target = this.displays[message.display];
        if (!target) return; // a real host silently ignores an out-of-range index
        this.currentDisplay = message.display;
        await this.sendEncrypted(
          encodeSwitchDisplayMessage({ display: message.display, width: target.width, height: target.height }),
        );
        return;
      }
      default:
        return;
    }
  }

  private sendLogonResponse(): Promise<void> {
    return this.sendEncrypted(
      encodeLoginResponseWithPeerInfo({
        hostname: 'DESKTOP-KARI',
        platform: 'Windows',
        displays: this.displays,
        currentDisplay: this.currentDisplay,
      }),
    );
  }

  /**
   * Computed independently of RustDeskPasswordAuthenticator, so the test
   * checks the algorithm rather than agreeing with itself.
   */
  private async expectedPasswordHash(): Promise<Uint8Array> {
    const utf8 = (value: string) => new TextEncoder().encode(value);
    const h1 = await sha256(concat(utf8(this.secret), utf8(this.salt)));
    return sha256(concat(h1, utf8(this.challenge)));
  }
}
