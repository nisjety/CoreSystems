import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  loadUserCoreGrpcClientCredential,
  parseUserCoreGrpcClientCredential,
} from './user-core-grpc-credential';

const TOKEN = 'auth-user-grpc-0123456789abcdef0123456789abcdef';

function credential(): string {
  return JSON.stringify({
    credentialId: 'auth-core-2026-07',
    principal: 'auth-core',
    audience: 'user-core-grpc',
    token: TOKEN,
  });
}

describe('Auth to User Core gRPC credential', () => {
  let fixtureDirectory = '';

  beforeEach(() => {
    fixtureDirectory = mkdtempSync(join(tmpdir(), 'auth-user-grpc-'));
  });

  afterEach(() => {
    rmSync(fixtureDirectory, { force: true, recursive: true });
  });

  it('parses an exact audience-bound tuple', () => {
    expect(parseUserCoreGrpcClientCredential(credential())).toEqual({
      credentialId: 'auth-core-2026-07',
      principal: 'auth-core',
      audience: 'user-core-grpc',
      token: TOKEN,
    });
  });

  it.each([
    '',
    '[',
    JSON.stringify({
      credentialId: 'auth-core-2026-07',
      principal: 'auth-core',
      audience: 'wrong',
      token: TOKEN,
    }),
    JSON.stringify({
      credentialId: 'auth-core-2026-07',
      principal: 'attacker',
      audience: 'user-core-grpc',
      token: TOKEN,
    }),
    JSON.stringify({
      credentialId: 'auth-core-2026-07',
      principal: 'auth-core',
      audience: 'user-core-grpc',
      token: 'short',
    }),
  ])('rejects invalid client policy %s', (raw) => {
    expect(() => parseUserCoreGrpcClientCredential(raw)).toThrow();
  });

  it('requires a deployment-owned file in production', () => {
    expect(() =>
      loadUserCoreGrpcClientCredential({
        NODE_ENV: 'production',
        USER_CORE_GRPC_CLIENT_CREDENTIAL: credential(),
      }),
    ).toThrow(/FILE is required outside development/);
  });

  it('loads the exact tuple from a deployment-owned file', () => {
    const credentialFile = join(fixtureDirectory, 'credential.json');
    writeFileSync(credentialFile, credential());

    expect(
      loadUserCoreGrpcClientCredential({
        NODE_ENV: 'production',
        USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE: credentialFile,
      }),
    ).toEqual({
      credentialId: 'auth-core-2026-07',
      principal: 'auth-core',
      audience: 'user-core-grpc',
      token: TOKEN,
    });
  });

  it('fails closed when the deployment-owned file is missing or oversized', () => {
    expect(() =>
      loadUserCoreGrpcClientCredential({
        NODE_ENV: 'production',
        USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE: join(
          fixtureDirectory,
          'missing.json',
        ),
      }),
    ).toThrow(/could not be read/);

    const oversizedFile = join(fixtureDirectory, 'oversized.json');
    writeFileSync(oversizedFile, 'x'.repeat(64 * 1024 + 1));
    expect(() =>
      loadUserCoreGrpcClientCredential({
        NODE_ENV: 'production',
        USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE: oversizedFile,
      }),
    ).toThrow(/too large/);
  });
});
