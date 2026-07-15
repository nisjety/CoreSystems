import { ConfigService } from '@nestjs/config';
import * as protoLoader from '@grpc/proto-loader';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { ScopedUserServiceGrpcClient } from './scoped-user-service-grpc.client';

const CREDENTIAL = JSON.stringify({
  credentialId: 'auth-core-2026-07',
  principal: 'auth-core',
  audience: 'user-core-grpc',
  token: 'auth-user-grpc-0123456789abcdef0123456789abcdef',
});

describe('ScopedUserServiceGrpcClient startup', () => {
  const originalEnvironment = process.env;

  beforeEach(() => {
    process.env = {
      ...originalEnvironment,
      NODE_ENV: 'development',
      USER_CORE_GRPC_CLIENT_CREDENTIAL: CREDENTIAL,
    };
  });

  afterEach(() => {
    jest.restoreAllMocks();
    process.env = originalEnvironment;
  });

  it('propagates initialization errors so Auth startup fails closed', () => {
    jest.spyOn(protoLoader, 'loadSync').mockImplementation(() => {
      throw new Error('fixture proto failure');
    });
    const client = new ScopedUserServiceGrpcClient(new ConfigService());

    expect(() => client.onModuleInit()).toThrow('fixture proto failure');
  });

  it('loads the production User proto from the image working directory', () => {
    const fixtureDirectory = mkdtempSync(join(tmpdir(), 'auth-user-proto-'));
    const credentialFile = join(fixtureDirectory, 'credential.json');
    writeFileSync(credentialFile, CREDENTIAL);
    process.env = {
      ...originalEnvironment,
      NODE_ENV: 'production',
      USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE: credentialFile,
    };
    const stopAfterPathCapture = new Error('fixture path captured');
    const load = jest
      .spyOn(protoLoader, 'loadSync')
      .mockImplementation(() => {
        throw stopAfterPathCapture;
      });
    const client = new ScopedUserServiceGrpcClient(new ConfigService());

    try {
      expect(() => client.onModuleInit()).toThrow(stopAfterPathCapture);
      expect(load).toHaveBeenCalledWith(
        join(process.cwd(), 'proto/user/v1/user.proto'),
        expect.any(Object),
      );
    } finally {
      rmSync(fixtureDirectory, { force: true, recursive: true });
    }
  });

  it('does not silently report an unavailable client as a normal miss', async () => {
    const client = new ScopedUserServiceGrpcClient(new ConfigService());

    await expect(
      client.validateUserExists('person@example.com'),
    ).rejects.toThrow(/User Core gRPC client is unavailable/);
  });
});
