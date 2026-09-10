import { describe, expect, it } from 'vitest';
import { RustDeskPasswordAuthenticator } from '../../../src/protocol/rustdesk/RustDeskPasswordAuthenticator.js';

const utf8 = (value: string) => new TextEncoder().encode(value);

function concat(a: Uint8Array<ArrayBuffer>, b: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer> {
  const result = new Uint8Array(a.length + b.length);
  result.set(a, 0);
  result.set(b, a.length);
  return result;
}

async function sha256(bytes: Uint8Array<ArrayBuffer>): Promise<Uint8Array<ArrayBuffer>> {
  return new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
}

describe('RustDeskPasswordAuthenticator', () => {
  it('computes SHA256(SHA256(secret + salt) + challenge), matching RustDesk src/client.rs', async () => {
    const secret = 'correct horse battery staple';
    const salt = 'abc123salt';
    const challenge = 'one-time-challenge';

    const authenticator = new RustDeskPasswordAuthenticator(secret);
    const response = await authenticator.authenticate({ deviceId: 'device-1', salt, challenge });

    const h1 = await sha256(concat(utf8(secret), utf8(salt)));
    const expected = await sha256(concat(h1, utf8(challenge)));

    expect([...response.passwordHash]).toEqual([...expected]);
  });

  it('produces different hashes for different challenges (replay resistance)', async () => {
    const authenticator = new RustDeskPasswordAuthenticator('secret');
    const a = await authenticator.authenticate({ deviceId: 'd', salt: 's', challenge: 'one' });
    const b = await authenticator.authenticate({ deviceId: 'd', salt: 's', challenge: 'two' });

    expect([...a.passwordHash]).not.toEqual([...b.passwordHash]);
  });
});
