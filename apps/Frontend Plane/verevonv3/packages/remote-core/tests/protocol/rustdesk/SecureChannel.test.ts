import { describe, expect, it } from 'vitest';
import sodium from 'libsodium-wrappers';
import { SecureChannel, verifyServerVouch } from '../../../src/protocol/rustdesk/SecureChannel.js';
import { EncryptionError } from '../../../src/errors/RemoteError.js';
import { noopLogger } from '../../../src/logging/Logger.js';
import { openSealedSessionKey, SessionChannel } from '../../../src/crypto/SessionCrypto.js';
import { encodeIdPk } from '../../doubles/peerEncoders.js';

async function keys() {
  await sodium.ready;
  return sodium;
}

describe('verifyServerVouch', () => {
  it('recovers the peer identity from a correctly signed vouch', async () => {
    const s = await keys();
    const server = s.crypto_sign_keypair();
    const identity = s.crypto_sign_keypair();
    const vouch = s.crypto_sign(encodeIdPk('123456789', identity.publicKey), server.privateKey);

    const peer = await verifyServerVouch(vouch, server.publicKey);

    expect(peer.id).toBe('123456789');
    expect([...peer.signPublicKey]).toEqual([...identity.publicKey]);
  });

  it('rejects a vouch signed by a different key', async () => {
    const s = await keys();
    const server = s.crypto_sign_keypair();
    const impostor = s.crypto_sign_keypair();
    const identity = s.crypto_sign_keypair();
    const vouch = s.crypto_sign(encodeIdPk('123456789', identity.publicKey), impostor.privateKey);

    await expect(verifyServerVouch(vouch, server.publicKey)).rejects.toThrow(EncryptionError);
  });

  it('rejects a tampered vouch', async () => {
    const s = await keys();
    const server = s.crypto_sign_keypair();
    const identity = s.crypto_sign_keypair();
    const vouch = s.crypto_sign(encodeIdPk('123456789', identity.publicKey), server.privateKey);
    vouch[vouch.length - 1] = (vouch[vouch.length - 1] ?? 0) ^ 0xff;

    await expect(verifyServerVouch(vouch, server.publicKey)).rejects.toThrow(EncryptionError);
  });
});

describe('SecureChannel handshake', () => {
  async function fixture() {
    const s = await keys();
    const identity = s.crypto_sign_keypair();
    const ephemeral = s.crypto_box_keypair();
    const signedId = {
      id: s.crypto_sign(encodeIdPk('123456789', ephemeral.publicKey), identity.privateKey),
    };
    return { s, identity, ephemeral, signedId };
  }

  it('completes the exchange and produces a key the peer can unseal', async () => {
    const { identity, ephemeral, signedId } = await fixture();
    const channel = new SecureChannel({
      peerIdentity: { id: '123456789', signPublicKey: identity.publicKey },
      logger: noopLogger,
    });

    const publicKey = await channel.acceptSignedId(signedId);

    expect(channel.isEncrypted).toBe(true);

    // The peer must be able to recover the same session key and talk to us.
    const sessionKey = await openSealedSessionKey(
      publicKey.symmetricValue,
      publicKey.asymmetricValue,
      ephemeral.privateKey,
    );
    const peerChannel = new SessionChannel(sessionKey);
    const ciphertext = await channel.encrypt(new TextEncoder().encode('hallo'));
    expect(new TextDecoder().decode(await peerChannel.decrypt(ciphertext))).toBe('hallo');
  });

  it('refuses a SignedId signed by an identity the server did not vouch for', async () => {
    const { s, ephemeral } = await fixture();
    const impostor = s.crypto_sign_keypair();
    const realIdentity = s.crypto_sign_keypair();
    const forged = { id: s.crypto_sign(encodeIdPk('123456789', ephemeral.publicKey), impostor.privateKey) };

    const channel = new SecureChannel({
      peerIdentity: { id: '123456789', signPublicKey: realIdentity.publicKey },
      logger: noopLogger,
    });

    await expect(channel.acceptSignedId(forged)).rejects.toThrow(EncryptionError);
    expect(channel.isEncrypted).toBe(false);
  });

  it('refuses when the SignedId names a different peer than the vouch', async () => {
    const { s, ephemeral } = await fixture();
    const identity = s.crypto_sign_keypair();
    const mismatched = {
      id: s.crypto_sign(encodeIdPk('999999999', ephemeral.publicKey), identity.privateKey),
    };

    const channel = new SecureChannel({
      peerIdentity: { id: '123456789', signPublicKey: identity.publicKey },
      logger: noopLogger,
    });

    await expect(channel.acceptSignedId(mismatched)).rejects.toThrow(/does not match/i);
  });

  it('refuses an empty SignedId instead of continuing unauthenticated', async () => {
    const { identity } = await fixture();
    const channel = new SecureChannel({
      peerIdentity: { id: '123456789', signPublicKey: identity.publicKey },
      logger: noopLogger,
    });

    await expect(channel.acceptSignedId({ id: new Uint8Array(0) })).rejects.toThrow(/unauthenticated/i);
  });

  it('fails closed with no peer identity unless the unsafe flag is set', async () => {
    const { signedId } = await fixture();
    const strict = new SecureChannel({ logger: noopLogger });

    await expect(strict.acceptSignedId(signedId)).rejects.toThrow(/unauthenticated handshake/i);

    const permissive = new SecureChannel({ allowUnverifiedPeer: true, logger: noopLogger });
    await expect(permissive.acceptSignedId(signedId)).resolves.toBeDefined();
  });

  it('refuses to encrypt before the handshake', async () => {
    const channel = new SecureChannel({ logger: noopLogger });
    await expect(channel.encrypt(new Uint8Array([1]))).rejects.toThrow(/before the secure channel/i);
  });

  it('refuses a second handshake on the same channel', async () => {
    const { identity, signedId } = await fixture();
    const channel = new SecureChannel({
      peerIdentity: { id: '123456789', signPublicKey: identity.publicKey },
      logger: noopLogger,
    });

    await channel.acceptSignedId(signedId);
    await expect(channel.acceptSignedId(signedId)).rejects.toThrow(/already completed/i);
  });
});
