import { normalizeOutboxRevision } from './outbox-revision';

describe('normalizeOutboxRevision', () => {
  it.each([
    [1, 1],
    ['2', 2],
    [3n, 3],
  ])(
    'serializes Postgres BIGINT revision %p as JSON number %p',
    (input, expected) => {
      expect(normalizeOutboxRevision(input)).toBe(expected);
    },
  );

  it.each([0, -1, 'not-a-number', Number.MAX_SAFE_INTEGER + 1])(
    'fails closed for invalid revision %p',
    (input) => {
      expect(() => normalizeOutboxRevision(input)).toThrow(
        'outbox revision must be a positive safe integer',
      );
    },
  );
});
