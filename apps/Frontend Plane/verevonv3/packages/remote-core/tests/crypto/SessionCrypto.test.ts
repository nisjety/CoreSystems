import { describe, expect, it } from 'vitest';
import {
  DirectionalCipher,
  generateBoxKeyPair,
  generateSessionKey,
  generateSignKeyPair,
  openSealedSessionKey,
  sealSessionKey,
  SessionChannel,
  signDetached,
  verifyDetached,
} from '../../src/crypto/SessionCrypto.js';
import { EncryptionError } from '../../src/errors/RemoteError.js';

describe('SessionCrypto: box key exchange', () => {
  it('a session key sealed to the recipient can only be opened with the matching keypair', async () => {
    const sender = await generateBoxKeyPair();
    const recipient = await generateBoxKeyPair();
    const sessionKey = await generateSessionKey();

    const sealed = await sealSessionKey(sessionKey, recipient.publicKey, sender.secretKey);
    const opened = await openSealedSessionKey(sealed, sender.publicKey, recipient.secretKey);

    expect([...opened]).toEqual([...sessionKey]);
  });

  it('fails to open with the wrong recipient secret key', async () => {
    const sender = await generateBoxKeyPair();
    const recipient = await generateBoxKeyPair();
    const wrongRecipient = await generateBoxKeyPair();
    const sessionKey = await generateSessionKey();

    const sealed = await sealSessionKey(sessionKey, recipient.publicKey, sender.secretKey);

    await expect(openSealedSessionKey(sealed, sender.publicKey, wrongRecipient.secretKey)).rejects.toThrow(
      EncryptionError,
    );
  });

  it('fails to open if the sealed bytes were tampered with', async () => {
    const sender = await generateBoxKeyPair();
    const recipient = await generateBoxKeyPair();
    const sessionKey = await generateSessionKey();

    const sealed = await sealSessionKey(sessionKey, recipient.publicKey, sender.secretKey);
    const tampered = new Uint8Array(sealed);
    tampered[0] = (tampered[0] ?? 0) ^ 0xff;

    await expect(openSealedSessionKey(tampered, sender.publicKey, recipient.secretKey)).rejects.toThrow(
      EncryptionError,
    );
  });
});

describe('SessionCrypto: signing (peer identity)', () => {
  it('verifies a signature made with the matching key pair', async () => {
    const identity = await generateSignKeyPair();
    const message = new TextEncoder().encode('peer-id-and-ephemeral-key');

    const signature = await signDetached(message, identity.secretKey);

    expect(await verifyDetached(signature, message, identity.publicKey)).toBe(true);
  });

  it('rejects a signature from a different key pair (spoofed peer identity)', async () => {
    const identity = await generateSignKeyPair();
    const impostor = await generateSignKeyPair();
    const message = new TextEncoder().encode('peer-id-and-ephemeral-key');

    const signature = await signDetached(message, impostor.secretKey);

    expect(await verifyDetached(signature, message, identity.publicKey)).toBe(false);
  });

  it('rejects a valid signature over a different message (tamper detection)', async () => {
    const identity = await generateSignKeyPair();
    const signature = await signDetached(new TextEncoder().encode('original'), identity.secretKey);

    expect(await verifyDetached(signature, new TextEncoder().encode('tampered'), identity.publicKey)).toBe(false);
  });
});

describe('SessionCrypto: DirectionalCipher / SessionChannel (mechanical round-trip only — see class-level warning)', () => {
  it('a single direction can seal then open its own sequence of messages in order', async () => {
    const key = await generateSessionKey();
    const sealer = new DirectionalCipher(key);
    const opener = new DirectionalCipher(key);

    const messages = ['first', 'second', 'third'].map((text) => new TextEncoder().encode(text));
    const decoded: string[] = [];
    for (const message of messages) {
      const sealed = await sealer.seal(message);
      decoded.push(new TextDecoder().decode(await opener.open(sealed)));
    }

    expect(decoded).toEqual(['first', 'second', 'third']);
  });

  it('opening out of order fails (nonce/counter must track delivery order)', async () => {
    const key = await generateSessionKey();
    const sealer = new DirectionalCipher(key);
    const opener = new DirectionalCipher(key);

    await sealer.seal(new TextEncoder().encode('first')); // sealer's counter -> 1
    const second = await sealer.seal(new TextEncoder().encode('second')); // sealer's counter -> 2

    // opener's very first call expects counter 1, but `second` was sealed at counter 2.
    await expect(opener.open(second)).rejects.toThrow(EncryptionError);
  });

  it('SessionChannel round-trips a message sent from one side and received by the other', async () => {
    const key = await generateSessionKey();
    const alice = new SessionChannel(key);
    const bob = new SessionChannel(key);

    const ciphertext = await alice.encrypt(new TextEncoder().encode('hello from alice'));
    const plaintext = await bob.decrypt(ciphertext);

    expect(new TextDecoder().decode(plaintext)).toBe('hello from alice');
  });
});
