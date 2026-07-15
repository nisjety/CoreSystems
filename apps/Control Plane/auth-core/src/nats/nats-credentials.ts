export type NatsCredentials =
  | { user: string; pass: string }
  | { token: string }
  | Record<string, never>;

export function selectNatsCredentials(
  env: Readonly<Record<string, string | undefined>>,
): NatsCredentials {
  const user = env.NATS_USER?.trim() ?? '';
  const pass = (env.NATS_PASSWORD ?? env.NATS_PASS)?.trim() ?? '';
  if (Boolean(user) !== Boolean(pass)) {
    throw new Error('NATS_USER and NATS_PASSWORD must be configured together');
  }
  if (user) {
    if (pass.length < 32) {
      throw new Error('NATS_PASSWORD must contain at least 32 characters');
    }
    return { user, pass };
  }
  const token = (env.NATS_TOKEN ?? env.NATS_AUTH_TOKEN)?.trim() ?? '';
  if (token) {
    if (env.NATS_ALLOW_TOKEN_FALLBACK !== '1') {
      throw new Error(
        'NATS token fallback requires NATS_ALLOW_TOKEN_FALLBACK=1',
      );
    }
    return { token };
  }
  return {};
}
