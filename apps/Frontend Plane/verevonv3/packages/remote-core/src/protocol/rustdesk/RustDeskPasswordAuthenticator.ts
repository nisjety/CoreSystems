import type { AuthenticationChallenge, AuthenticationResponse, SessionAuthenticator } from '../../types/public.js';

/**
 * RustDesk sin faktiske passordverifisering: to runder SHA-256.
 *   h1   = SHA256(hemmelighet ‖ salt)
 *   wire = SHA256(h1 ‖ challenge)
 * Verifisert mot rustdesk/rustdesk sin src/client.rs (handle_hash og
 * handle_login_from_ui bruker samme opplegg) — se docs/rustdesk-protocol.md.
 * Fungerer uendret for et klassisk RustDesk-passord ELLER et Verevon-utstedt
 * kortlevd øktoken, siden serveren bare ser "en hemmelighet som hasher
 * riktig" — den skiller ikke mellom kildene.
 */
export class RustDeskPasswordAuthenticator implements SessionAuthenticator {
  readonly provideSecondFactor: (() => Promise<string>) | undefined;

  constructor(
    private readonly secret: string,
    options: { readonly secondFactor?: () => Promise<string> } = {},
  ) {
    this.provideSecondFactor = options.secondFactor;
  }

  async authenticate(challenge: AuthenticationChallenge): Promise<AuthenticationResponse> {
    const h1 = await sha256(concat(utf8(this.secret), utf8(challenge.salt)));
    const wire = await sha256(concat(h1, utf8(challenge.challenge)));
    return { passwordHash: wire };
  }
}

function utf8(value: string): Uint8Array<ArrayBuffer> {
  return new TextEncoder().encode(value);
}

function concat(a: Uint8Array<ArrayBuffer>, b: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer> {
  const result = new Uint8Array(a.length + b.length);
  result.set(a, 0);
  result.set(b, a.length);
  return result;
}

async function sha256(bytes: Uint8Array<ArrayBuffer>): Promise<Uint8Array<ArrayBuffer>> {
  const digest = await crypto.subtle.digest('SHA-256', bytes);
  return new Uint8Array(digest);
}
