/**
 * Keeps only the answer to the newest call. A call that a newer one has overtaken may still answer late, and
 * its answer must not replace the newer one on screen: the caller asks `isCurrent` before it applies anything.
 */
export function createLatestRequest() {
  let current = 0
  return {
    next(): number {
      current += 1
      return current
    },
    isCurrent(token: number): boolean {
      return token === current
    },
  }
}
