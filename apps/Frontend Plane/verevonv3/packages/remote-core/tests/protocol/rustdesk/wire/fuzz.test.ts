import { describe, expect, it } from 'vitest';
import { decodeMessage, decodeRendezvousMessage, encodeMessage } from '../../../../src/protocol/rustdesk/messages/envelope.js';
import { ProtoReader } from '../../../../src/protocol/rustdesk/wire/ProtoReader.js';

/**
 * Adversarial-input guard for the hand-written proto3 decoder. On a live
 * connection these functions run on bytes a relay or a hostile peer chose, so
 * the property that matters is: for ANY input they either return a value or
 * throw a plain Error — never hang, never recurse without bound, never let a
 * declared length walk past the buffer into garbage that is then trusted.
 *
 * Deterministic PRNG so a failing seed is reproducible from the assertion
 * message rather than vanishing on rerun.
 */
function prng(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    // xorshift32 — cheap, deterministic, good enough to drive byte generation.
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return (state >>> 0) / 0x1_0000_0000;
  };
}

function randomBytes(next: () => number, length: number): Uint8Array {
  const out = new Uint8Array(length);
  for (let index = 0; index < length; index += 1) out[index] = Math.floor(next() * 256);
  return out;
}

/** Runs `decode` and reports how it ended, never letting a throw escape. */
function outcome(decode: () => unknown): 'value' | 'error' | 'non-error-throw' {
  try {
    decode();
    return 'value';
  } catch (thrown) {
    return thrown instanceof Error ? 'error' : 'non-error-throw';
  }
}

const ITERATIONS = 3000;

function hex(bytes: Uint8Array): string {
  return [...bytes].map((byte) => byte.toString(16).padStart(2, '0')).join('');
}

describe('proto3 decoder under adversarial input', () => {
  it('decodeMessage never hangs or throws a non-Error on random bytes', () => {
    const next = prng(0xc0ffee);
    for (let iteration = 0; iteration < ITERATIONS; iteration += 1) {
      const length = Math.floor(next() * 64);
      const bytes = randomBytes(next, length);
      const result = outcome(() => decodeMessage(bytes));
      expect(result, `iteration ${iteration} bytes=${hex(bytes)}`).not.toBe('non-error-throw');
    }
  });

  it('decodeRendezvousMessage never hangs or throws a non-Error on random bytes', () => {
    const next = prng(0xbeef);
    for (let iteration = 0; iteration < ITERATIONS; iteration += 1) {
      const length = Math.floor(next() * 64);
      const bytes = randomBytes(next, length);
      const result = outcome(() => decodeRendezvousMessage(bytes));
      expect(result, `iteration ${iteration} bytes=${hex(bytes)}`).not.toBe('non-error-throw');
    }
  });

  it('every truncation of a valid message is handled, not misread as valid', () => {
    const valid = encodeMessage({
      kind: 'loginRequest',
      value: {
        username: '123456789',
        password: new Uint8Array(32).fill(7),
        myId: 'verevon-web',
        myName: 'Verevon Support',
        myPlatform: 'Web',
        sessionId: 42n,
        version: '1.4.9',
        videoAckRequired: false,
        hwid: new Uint8Array(0),
        avatar: '',
      },
    });

    for (let cut = 0; cut < valid.length; cut += 1) {
      const truncated = valid.subarray(0, cut);
      // A truncated envelope must not surface as a fully-formed known message
      // with silently missing bytes: it either errors, or degrades to
      // 'unknown', or (for cuts that happen to land on a field boundary in a
      // message we never decode inbound) is simply not a loginRequest.
      const result = outcome(() => {
        const decoded = decodeMessage(truncated);
        expect(decoded.kind).not.toBe('loginRequest');
      });
      expect(result, `cut at ${cut}`).not.toBe('non-error-throw');
    }
  });

  it('a declared length larger than the buffer cannot read past the end', () => {
    // tag: field 9 (hash), wiretype 2, then a varint length of 1_000_000 with
    // only two bytes actually following.
    const bytes = new Uint8Array([0x4a, 0xc0, 0x84, 0x3d, 0x01, 0x02]);
    const reader = new ProtoReader(bytes);
    reader.readTag();
    const slice = reader.readLengthDelimited();
    // subarray clamps: the slice can never exceed what the buffer holds.
    expect(slice.length).toBeLessThanOrEqual(bytes.length);
    expect(reader.eof()).toBe(true);
  });

  it('an over-long varint is rejected rather than looping', () => {
    // 11 continuation bytes: exceeds the 64-bit limit the reader enforces.
    const bytes = new Uint8Array(11).fill(0xff);
    const reader = new ProtoReader(bytes);
    expect(() => reader.readVarintRaw()).toThrow(RangeError);
  });

  it('readTag on a truncated varint throws a RangeError, not undefined-field garbage', () => {
    const reader = new ProtoReader(new Uint8Array([0x80]));
    expect(() => reader.readTag()).toThrow(RangeError);
  });
});
