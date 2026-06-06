type CacheEntry<T> = {
  expiresAt: number;
  value: T;
};

export function readCachedValue<T>(
  cache: Map<string, CacheEntry<T>>,
  key: string,
  now = Date.now(),
): T | null {
  const entry = cache.get(key);
  if (!entry) {
    return null;
  }

  if (entry.expiresAt <= now) {
    cache.delete(key);
    return null;
  }

  return entry.value;
}

export function writeCachedValue<T>(
  cache: Map<string, CacheEntry<T>>,
  key: string,
  value: T,
  ttlMs: number,
  now = Date.now(),
) {
  cache.set(key, {
    expiresAt: now + ttlMs,
    value,
  });
}
