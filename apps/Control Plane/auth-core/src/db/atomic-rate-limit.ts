interface RedisEvalClient {
  eval(
    script: string,
    options: { keys: string[]; arguments: string[] },
  ): Promise<unknown>;
}

const ATOMIC_INCREMENT_WITH_EXPIRY = `
  local existing = redis.call('GET', KEYS[1])
  if existing and tonumber(existing) == nil then
    redis.call('DEL', KEYS[1])
  end
  local current = redis.call('INCR', KEYS[1])
  if current == 1 then
    redis.call('EXPIRE', KEYS[1], ARGV[1])
  end
  return current
`;

export async function atomicIncrementWithExpiry(
  client: RedisEvalClient,
  rawKey: string,
  ttlSeconds: number,
): Promise<number> {
  const key = rawKey.trim();
  if (!key) throw new Error('rate-limit key is required');
  if (!Number.isSafeInteger(ttlSeconds) || ttlSeconds <= 0) {
    throw new Error('rate-limit ttl must be positive');
  }

  const result = await client.eval(ATOMIC_INCREMENT_WITH_EXPIRY, {
    keys: [key],
    arguments: [String(ttlSeconds)],
  });
  if (typeof result !== 'number' || !Number.isSafeInteger(result)) {
    throw new Error('Dragonfly returned an invalid rate-limit counter');
  }
  return result;
}
