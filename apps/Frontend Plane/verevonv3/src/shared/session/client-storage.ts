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

// Tab-lifetime storage only. The helpers above write to localStorage as well;
// content that must not outlive the tab (an unsent draft) goes through these,
// which never touch localStorage.
function sessionOnly(): Storage | null {
  if (typeof window === 'undefined') return null
  try {
    return window.sessionStorage
  } catch {
    // Session storage can be disabled by privacy settings.
    return null
  }
}

export function readSessionValue(key: string): string | null {
  try {
    return sessionOnly()?.getItem(key) ?? null
  } catch {
    return null
  }
}

/** Returns whether the value was stored; a full or blocked storage reports false. */
export function writeSessionValue(key: string, value: string): boolean {
  const storage = sessionOnly()
  if (!storage) return false
  try {
    storage.setItem(key, value)
    return true
  } catch {
    return false
  }
}

export function removeSessionValue(key: string): void {
  try {
    sessionOnly()?.removeItem(key)
  } catch {
    // Best-effort cleanup.
  }
}

/** Removes every entry of `storage` whose key starts with `prefix`. */
function removeKeysWithPrefix(storage: Storage, prefix: string): void {
  try {
    const keys: string[] = []
    for (let index = 0; index < storage.length; index += 1) {
      const key = storage.key(index)
      if (key !== null && key.startsWith(prefix)) keys.push(key)
    }
    for (const key of keys) storage.removeItem(key)
  } catch {
    // Best-effort cleanup.
  }
}

/** Removes every session-storage entry whose key starts with `prefix`. */
export function removeSessionValuesWithPrefix(prefix: string): void {
  const storage = sessionOnly()
  if (storage) removeKeysWithPrefix(storage, prefix)
}

/** Removes every entry whose key starts with `prefix` from both local and session storage. */
export function removeClientValuesWithPrefix(prefix: string): void {
  for (const storage of storageCandidates()) removeKeysWithPrefix(storage, prefix)
}
