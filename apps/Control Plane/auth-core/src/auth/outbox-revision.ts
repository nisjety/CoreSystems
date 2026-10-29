export type OutboxRevision = number | string;

export function normalizeOutboxRevision(value: unknown): number {
  const revision = Number(value);
  if (!Number.isSafeInteger(revision) || revision < 1) {
    throw new Error('outbox revision must be a positive safe integer');
  }
  return revision;
}
