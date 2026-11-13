import {
  createPublicKey,
  createSign,
  generateKeyPairSync,
  type KeyObject,
} from 'node:crypto';
import {
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative, resolve, sep } from 'node:path';

import {
  FileKeyRetentionDecisionSigner,
  RETENTION_DECISION_AUDIENCE,
  RETENTION_DECISION_TTL_SECONDS,
  RetentionDecisionInvalidError,
  RetentionDecisionUnavailableError,
  UnavailableRetentionDecisionSigner,
  VerifiedRetentionDecision,
  createRetentionDecisionSigner,
  type RetentionDecisionClaims,
} from './retention-decision.signer';
import { isVerifiedRetentionDecision } from './retention-decision.verified';

// ZDR-Z-14s: the dedicated decision signer. Two test keypairs are generated
// here; no key material is checked in.

/** The factory's name, which the guard below scans for. */
const FACTORY_NAME = 'createVerifiedRetentionDecision';

/** Every TypeScript source file under a directory, recursively. */
function sourceFilesUnder(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sourceFilesUnder(path);
    return entry.name.endsWith('.ts') && !entry.name.endsWith('.d.ts')
      ? [path]
      : [];
  });
}

type Pair = { privateKey: KeyObject; publicKey: KeyObject };

function rsaPair(bits = 2048): Pair {
  const pair = generateKeyPairSync('rsa', { modulusLength: bits });
  return {
    privateKey: pair.privateKey,
    publicKey: createPublicKey(pair.privateKey),
  };
}

const CURRENT = rsaPair();
const PREVIOUS = rsaPair();
const TOKEN_KEY = rsaPair();

const CLAIMS: RetentionDecisionClaims = {
  userId: 'user-a',
  orgId: 'org-a',
  sessionId: 'session-a',
  connection: 'connection-1',
  provider: 'openai-codex-subscription',
  ackId: 'ack-1',
  digest: 'sha256:abc',
};

function signer(previous: Map<string, KeyObject> = new Map()) {
  return new FileKeyRetentionDecisionSigner({
    keyId: 'decision-key-1',
    privateKey: CURRENT.privateKey,
    publicKey: CURRENT.publicKey,
    previous,
  });
}

function segments(token: string) {
  const [h, p, s] = token.split('.');
  return {
    header: JSON.parse(Buffer.from(h, 'base64url').toString('utf8')) as Record<
      string,
      unknown
    >,
    payload: JSON.parse(Buffer.from(p, 'base64url').toString('utf8')) as Record<
      string,
      unknown
    >,
    signature: s,
  };
}

/** A token signed with `key` over the given header and payload. */
function forge(
  header: Record<string, unknown>,
  payload: Record<string, unknown>,
  key: KeyObject = CURRENT.privateKey,
): string {
  const input = `${Buffer.from(JSON.stringify(header)).toString('base64url')}.${Buffer.from(
    JSON.stringify(payload),
  ).toString('base64url')}`;
  const sign = createSign('RSA-SHA256');
  sign.update(input);
  sign.end();
  return `${input}.${sign.sign(key).toString('base64url')}`;
}

function resigned(
  token: string,
  change: (parts: {
    header: Record<string, unknown>;
    payload: Record<string, unknown>;
  }) => void,
  key: KeyObject = CURRENT.privateKey,
): string {
  const parts = segments(token);
  change(parts);
  return forge(parts.header, parts.payload, key);
}

describe('RetentionDecisionSigner (ZDR-Z-14s)', () => {
  afterEach(() => {
    jest.useRealTimers();
  });

  it('a signed decision verifies and carries the claimed facts', () => {
    const s = signer();
    const signed = s.sign(CLAIMS);
    const { header, payload } = segments(signed.token);
    expect(header).toEqual({
      alg: 'RS256',
      typ: 'retention-decision+jwt',
      kid: 'decision-key-1',
    });
    expect(payload).toMatchObject({
      iss: 'auth-core',
      aud: RETENTION_DECISION_AUDIENCE,
      sub: 'user-a',
      org_id: 'org-a',
      sid: 'session-a',
      connection: 'connection-1',
      provider: 'openai-codex-subscription',
      ack_id: 'ack-1',
      digest: 'sha256:abc',
      wv: 1,
      decision: 'waived',
    });
    expect(Number(payload.exp) - Number(payload.iat)).toBe(60);
    expect(Number(payload.iat) - Number(payload.nbf)).toBe(5);
    expect(signed.expiresInSeconds).toBe(RETENTION_DECISION_TTL_SECONDS);

    const verified = s.verify(signed.token);
    expect(verified).toBeInstanceOf(VerifiedRetentionDecision);
    expect(Object.isFrozen(verified)).toBe(true);
    expect(verified).toMatchObject({
      decision: 'waived',
      userId: 'user-a',
      orgId: 'org-a',
      sessionId: 'session-a',
      connection: 'connection-1',
      provider: 'openai-codex-subscription',
      ackId: 'ack-1',
      digest: 'sha256:abc',
      jti: signed.jti,
      keyId: 'decision-key-1',
    });
  });

  it('two decisions get distinct jti', () => {
    const s = signer();
    expect(s.sign(CLAIMS).jti).not.toBe(s.sign(CLAIMS).jti);
  });

  describe('verify rejects', () => {
    const s = signer();

    it('a tampered payload (same signature)', () => {
      const token = s.sign(CLAIMS).token;
      const [h, , sig] = token.split('.');
      const { payload } = segments(token);
      payload.org_id = 'org-b';
      const tampered = `${h}.${Buffer.from(JSON.stringify(payload)).toString('base64url')}.${sig}`;
      expect(() => s.verify(tampered)).toThrow(RetentionDecisionInvalidError);
    });

    it('a wrong kid', () => {
      const token = resigned(s.sign(CLAIMS).token, ({ header }) => {
        header.kid = 'some-other-key';
      });
      expect(() => s.verify(token)).toThrow(RetentionDecisionInvalidError);
    });

    it('a token signed by another key under the right kid (e.g. the token key)', () => {
      const token = resigned(
        s.sign(CLAIMS).token,
        () => undefined,
        TOKEN_KEY.privateKey,
      );
      expect(() => s.verify(token)).toThrow(RetentionDecisionInvalidError);
    });

    it('an expired token', () => {
      jest.useFakeTimers({ now: new Date('2026-11-12T12:00:00Z') });
      const token = s.sign(CLAIMS).token;
      jest.setSystemTime(new Date('2026-11-12T12:01:01Z'));
      expect(() => s.verify(token)).toThrow(RetentionDecisionInvalidError);
    });

    it('a wrong aud', () => {
      const token = resigned(s.sign(CLAIMS).token, ({ payload }) => {
        payload.aud = 'model-gateway';
      });
      expect(() => s.verify(token)).toThrow(RetentionDecisionInvalidError);
    });

    it('a wrong iss', () => {
      const token = resigned(s.sign(CLAIMS).token, ({ payload }) => {
        payload.iss = 'someone-else';
      });
      expect(() => s.verify(token)).toThrow(RetentionDecisionInvalidError);
    });

    it('wv 2', () => {
      const token = resigned(s.sign(CLAIMS).token, ({ payload }) => {
        payload.wv = 2;
      });
      expect(() => s.verify(token)).toThrow(RetentionDecisionInvalidError);
    });

    it('a wrong typ (an ordinary JWT)', () => {
      const token = resigned(s.sign(CLAIMS).token, ({ header }) => {
        header.typ = 'JWT';
      });
      expect(() => s.verify(token)).toThrow(RetentionDecisionInvalidError);
    });

    it('a lifetime longer than 60 s', () => {
      const token = resigned(s.sign(CLAIMS).token, ({ payload }) => {
        payload.exp = Number(payload.iat) + 3600;
      });
      expect(() => s.verify(token)).toThrow(RetentionDecisionInvalidError);
    });

    it('a not-yet-valid token (nbf beyond the 5 s skew)', () => {
      const token = resigned(s.sign(CLAIMS).token, ({ payload }) => {
        payload.nbf = Number(payload.iat) + 30;
        payload.exp = Number(payload.iat) + 60;
      });
      expect(() => s.verify(token)).toThrow(RetentionDecisionInvalidError);
    });

    it.each([
      ['surrounding whitespace', ' org-a'],
      ['a control character', 'org\u0000a'],
      ['an empty value', ''],
      ['129 characters', 'x'.repeat(129)],
      ['a number', 7],
    ])('a field with %s', (_case, value) => {
      const token = resigned(s.sign(CLAIMS).token, ({ payload }) => {
        payload.org_id = value;
      });
      expect(() => s.verify(token)).toThrow(RetentionDecisionInvalidError);
    });

    it('a garbage token never leaks a parse error', () => {
      for (const token of ['', 'a.b', 'a.b.c', '!!.!!.!!', 'e30.e30.e30']) {
        expect(() => s.verify(token)).toThrow(RetentionDecisionInvalidError);
      }
    });
  });

  it('sign refuses unclean claims', () => {
    expect(() =>
      signer().sign({ ...CLAIMS, connection: ' connection-1' }),
    ).toThrow(RetentionDecisionInvalidError);
    expect(() => signer().sign({ ...CLAIMS, digest: '' })).toThrow(
      RetentionDecisionInvalidError,
    );
  });

  it('a previous-key token verifies only while the previous key is configured', () => {
    const old = new FileKeyRetentionDecisionSigner({
      keyId: 'decision-key-0',
      privateKey: PREVIOUS.privateKey,
      publicKey: PREVIOUS.publicKey,
      previous: new Map(),
    });
    const token = old.sign(CLAIMS).token;
    expect(
      signer(new Map([['decision-key-0', PREVIOUS.publicKey]])).verify(token)
        .keyId,
    ).toBe('decision-key-0');
    expect(() => signer().verify(token)).toThrow(RetentionDecisionInvalidError);
  });

  it('the unavailable signer fails closed on both operations', () => {
    const s = new UnavailableRetentionDecisionSigner();
    expect(s.available()).toBe(false);
    expect(() => s.sign(CLAIMS)).toThrow(RetentionDecisionUnavailableError);
    expect(() => s.verify('a.b.c')).toThrow(RetentionDecisionUnavailableError);
  });

  it('nothing outside the signer can construct a VerifiedRetentionDecision', () => {
    const verified = signer().verify(signer().sign(CLAIMS).token);
    const literal = { ...verified };
    expect(literal).not.toBeInstanceOf(VerifiedRetentionDecision);
    const Ctor = VerifiedRetentionDecision as unknown as new (
      guard: symbol,
      fields: Record<string, unknown>,
    ) => VerifiedRetentionDecision;
    expect(
      () => new Ctor(Symbol('VerifiedRetentionDecisionGuard'), { ...literal }),
    ).toThrow(RetentionDecisionInvalidError);
  });

  it('the brand accepts only what the factory built, never a copy or a prototype-linked object', () => {
    const s = signer();
    const verified = s.verify(s.sign(CLAIMS).token);
    expect(isVerifiedRetentionDecision(verified)).toBe(true);
    const copy = { ...verified };
    const prototypeLinked = Object.assign(
      Object.create(VerifiedRetentionDecision.prototype) as object,
      { ...verified },
    );
    // Both pass an instanceof check. Only the brand tells them apart.
    expect(copy).not.toBeInstanceOf(VerifiedRetentionDecision);
    expect(prototypeLinked).toBeInstanceOf(VerifiedRetentionDecision);
    expect(isVerifiedRetentionDecision(copy)).toBe(false);
    expect(isVerifiedRetentionDecision(prototypeLinked)).toBe(false);
    expect(isVerifiedRetentionDecision(null)).toBe(false);
    expect(isVerifiedRetentionDecision(undefined)).toBe(false);
    expect(isVerifiedRetentionDecision('waived')).toBe(false);
  });

  it('the typed refusal module imports neither the signer, the decision service nor the issuer', () => {
    const source = readFileSync(
      join(__dirname, 'retention-decision.refusal.ts'),
      'utf8',
    );
    expect(source).not.toContain("from './retention-decision.signer'");
    expect(source).not.toContain("from './retention-decision.service'");
    expect(source).not.toContain("from './convex-token.service'");
    expect(source).not.toContain("from './retention-waiver-mint'");
  });

  it('keeps the decision type dependency-free, the issuer off the signer, and the factory with the signer alone under src', () => {
    const dir = __dirname;
    const verifiedSource = readFileSync(
      join(dir, 'retention-decision.verified.ts'),
      'utf8',
    );
    expect(verifiedSource).not.toMatch(/^import\b/m);
    const issuerSource = readFileSync(
      join(dir, 'convex-token.service.ts'),
      'utf8',
    );
    expect(issuerSource).toContain("from './retention-decision.verified'");
    expect(issuerSource).not.toContain("from './retention-decision.signer'");
    // The factory is reachable from any module under src/, so every TypeScript
    // file there is scanned. Only the module that defines it and the signer may
    // name it. This file is skipped: it quotes the name to scan for it.
    const srcRoot = join(dir, '..');
    const builders = sourceFilesUnder(srcRoot)
      .filter((file) => resolve(file) !== resolve(__filename))
      .filter((file) => readFileSync(file, 'utf8').includes(FACTORY_NAME))
      .map((file) => relative(srcRoot, file).split(sep).join('/'))
      .sort();
    expect(builders).toEqual([
      'auth/retention-decision.signer.ts',
      'auth/retention-decision.verified.ts',
    ]);
  });
});

describe('createRetentionDecisionSigner (factory)', () => {
  let dir: string;
  const errors: string[] = [];
  const logger = {
    error: (message: string) => {
      errors.push(message);
    },
    log: () => undefined,
  };

  function pem(key: KeyObject, type: 'private' | 'public'): string {
    return type === 'private'
      ? (key.export({ type: 'pkcs8', format: 'pem' }) as string)
      : (key.export({ type: 'spki', format: 'pem' }) as string);
  }

  function write(name: string, content: string): string {
    const path = join(dir, name);
    writeFileSync(path, content);
    return path;
  }

  function tokenSpki(): Buffer {
    return TOKEN_KEY.publicKey.export({
      type: 'spki',
      format: 'der',
    }) as Buffer;
  }

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'zdr-decision-signer-'));
    errors.length = 0;
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  function goodEnv() {
    return {
      ZDR_DECISION_KEY_ID: 'decision-key-1',
      ZDR_DECISION_PRIVATE_KEY_FILE: write(
        'zdr-decision.key',
        pem(CURRENT.privateKey, 'private'),
      ),
      ZDR_DECISION_PUBLIC_KEY_FILE: write(
        'zdr-decision.pub',
        pem(CURRENT.publicKey, 'public'),
      ),
    };
  }

  it('loads a dedicated keypair and signs', () => {
    const s = createRetentionDecisionSigner(tokenSpki(), goodEnv(), logger);
    expect(s.available()).toBe(true);
    expect(s.verify(s.sign(CLAIMS).token).userId).toBe('user-a');
  });

  it.each([
    'ZDR_DECISION_KEY_ID',
    'ZDR_DECISION_PRIVATE_KEY_FILE',
    'ZDR_DECISION_PUBLIC_KEY_FILE',
  ] as const)(
    '%s unset or empty: unavailable, the log names the variable',
    (variable) => {
      for (const value of [undefined, '', '   ']) {
        errors.length = 0;
        const env = { ...goodEnv(), [variable]: value };
        const s = createRetentionDecisionSigner(tokenSpki(), env, logger);
        expect(s.available()).toBe(false);
        expect(errors.join('\n')).toContain(variable);
      }
    },
  );

  it('a missing key file: unavailable', () => {
    const env = {
      ...goodEnv(),
      ZDR_DECISION_PRIVATE_KEY_FILE: join(dir, 'missing.key'),
    };
    expect(
      createRetentionDecisionSigner(tokenSpki(), env, logger).available(),
    ).toBe(false);
    expect(errors.join('\n')).toContain('ZDR_DECISION_PRIVATE_KEY_FILE');
  });

  it('the decision key equal to the token signing key: unavailable', () => {
    const env = goodEnv();
    const spki = CURRENT.publicKey.export({
      type: 'spki',
      format: 'der',
    }) as Buffer;
    expect(createRetentionDecisionSigner(spki, env, logger).available()).toBe(
      false,
    );
  });

  it('a mismatched public key: unavailable', () => {
    const env = {
      ...goodEnv(),
      ZDR_DECISION_PUBLIC_KEY_FILE: write(
        'other.pub',
        pem(PREVIOUS.publicKey, 'public'),
      ),
    };
    expect(
      createRetentionDecisionSigner(tokenSpki(), env, logger).available(),
    ).toBe(false);
  });

  it('a key weaker than RSA 2048: unavailable', () => {
    const weak = rsaPair(1024);
    const env = {
      ZDR_DECISION_KEY_ID: 'weak',
      ZDR_DECISION_PRIVATE_KEY_FILE: write(
        'weak.key',
        pem(weak.privateKey, 'private'),
      ),
      ZDR_DECISION_PUBLIC_KEY_FILE: write(
        'weak.pub',
        pem(weak.publicKey, 'public'),
      ),
    };
    expect(
      createRetentionDecisionSigner(tokenSpki(), env, logger).available(),
    ).toBe(false);
  });

  it('never logs key material', () => {
    const env = goodEnv();
    createRetentionDecisionSigner(
      CURRENT.publicKey.export({ type: 'spki', format: 'der' }) as Buffer,
      env,
      logger,
    );
    const everything = errors.join('\n');
    expect(everything).not.toContain('BEGIN');
    expect(everything).not.toContain(
      pem(CURRENT.publicKey, 'public').split('\n')[1],
    );
  });

  it('a configured previous key verifies a previous-key token; a bad one is ignored, current stays usable', () => {
    const old = new FileKeyRetentionDecisionSigner({
      keyId: 'decision-key-0',
      privateKey: PREVIOUS.privateKey,
      publicKey: PREVIOUS.publicKey,
      previous: new Map(),
    });
    const oldToken = old.sign(CLAIMS).token;
    const withPrevious = createRetentionDecisionSigner(
      tokenSpki(),
      {
        ...goodEnv(),
        ZDR_DECISION_PREVIOUS_KEY_ID: 'decision-key-0',
        ZDR_DECISION_PREVIOUS_PUBLIC_KEY_FILE: write(
          'prev.pub',
          pem(PREVIOUS.publicKey, 'public'),
        ),
      },
      logger,
    );
    expect(withPrevious.verify(oldToken).keyId).toBe('decision-key-0');

    const badPrevious = createRetentionDecisionSigner(
      tokenSpki(),
      {
        ...goodEnv(),
        ZDR_DECISION_PREVIOUS_KEY_ID: 'decision-key-0',
        ZDR_DECISION_PREVIOUS_PUBLIC_KEY_FILE: join(dir, 'missing.pub'),
      },
      logger,
    );
    expect(badPrevious.available()).toBe(true);
    expect(() => badPrevious.verify(oldToken)).toThrow(
      RetentionDecisionInvalidError,
    );
  });
});
