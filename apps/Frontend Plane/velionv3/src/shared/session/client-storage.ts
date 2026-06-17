type JsonGuard<T> = (value: unknown) => value is T

function storageCandidates(): Storage[] {
  if (typeof window === 'undefined') return []
  const candidates: Storage[] = []
  try {
    candidates.push(window.localStorage)
  } catch {
    // Storage can be disabled by privacy settings.
  }
  try {
    candidates.push(window.sessionStorage)
  } catch {
    // Session storage has the same failure modes as local storage.
  }
  return candidates
}

export function readClientValue(key: string): string | null {
  for (const storage of storageCandidates()) {
    try {
      const value = storage.getItem(key)
      if (value !== null) return value
    } catch {
      // Try the next storage backend.
    }
  }
  return null
}

export function writeClientValue(key: string, value: string): void {
  for (const storage of storageCandidates()) {
    try {
      storage.setItem(key, value)
    } catch {
      // Keep writing to any remaining backend.
    }
  }
}

export function removeClientValue(key: string): void {
  for (const storage of storageCandidates()) {
    try {
      storage.removeItem(key)
    } catch {
      // Best-effort cleanup.
    }
  }
}

export function readClientJson<T>(key: string, guard: JsonGuard<T>): T | null {
  const raw = readClientValue(key)
  if (!raw) return null

  try {
    const parsed = JSON.parse(raw) as unknown
    return guard(parsed) ? parsed : null
  } catch {
    return null
  }
}

export function writeClientJson(key: string, value: unknown): void {
  try {
    writeClientValue(key, JSON.stringify(value))
  } catch {
    // Circular or unserializable values are ignored at the boundary.
  }
}
