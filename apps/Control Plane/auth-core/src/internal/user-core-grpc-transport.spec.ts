import type * as grpc from '@grpc/grpc-js';
import { rootCertificates } from 'node:tls';
import {
  loadUserCoreGrpcChannelCredentials,
  type UserCoreGrpcTransportDependencies,
} from './user-core-grpc-transport';

const CA_CERTIFICATE = `${rootCertificates[0]}\n`;

function dependencies(): Readonly<{
  value: UserCoreGrpcTransportDependencies;
  secure: grpc.ChannelCredentials;
  insecure: grpc.ChannelCredentials;
  createSsl: jest.Mock;
  createInsecure: jest.Mock;
}> {
  const secure = {} as grpc.ChannelCredentials;
  const insecure = {} as grpc.ChannelCredentials;
  const createSsl = jest.fn(() => secure);
  const createInsecure = jest.fn(() => insecure);
  return {
    secure,
    insecure,
    createSsl,
    createInsecure,
    value: {
      readFile: jest.fn(() => Buffer.from(CA_CERTIFICATE)),
      createSsl,
      createInsecure,
    },
  };
}

describe('Auth to User Core gRPC transport', () => {
  it('requires a deployment-owned CA file in production', () => {
    const fixture = dependencies();

    expect(() =>
      loadUserCoreGrpcChannelCredentials(
        { NODE_ENV: 'production' },
        fixture.value,
      ),
    ).toThrow(/USER_CORE_GRPC_TLS_CA_FILE is required in production/);
    expect(fixture.createInsecure).not.toHaveBeenCalled();
  });

  it('creates a CA-pinned TLS channel in production', () => {
    const fixture = dependencies();

    const result = loadUserCoreGrpcChannelCredentials(
      {
        NODE_ENV: 'production',
        USER_CORE_GRPC_TLS_CA_FILE: '/run/secrets/user_core_grpc_tls_ca',
      },
      fixture.value,
    );

    expect(result).toBe(fixture.secure);
    expect(fixture.value.readFile).toHaveBeenCalledWith(
      '/run/secrets/user_core_grpc_tls_ca',
    );
    expect(fixture.createSsl).toHaveBeenCalledWith(Buffer.from(CA_CERTIFICATE));
    expect(fixture.createInsecure).not.toHaveBeenCalled();
  });

  it.each([
    Buffer.alloc(0),
    Buffer.from('not a certificate'),
    Buffer.from(
      '-----BEGIN CERTIFICATE-----\ninvalid\n-----END CERTIFICATE-----\n',
    ),
    Buffer.alloc(64 * 1024 + 1, 'x'),
  ])('rejects an invalid production CA file', (contents) => {
    const fixture = dependencies();
    fixture.value.readFile = jest.fn(() => contents);

    expect(() =>
      loadUserCoreGrpcChannelCredentials(
        {
          NODE_ENV: 'production',
          USER_CORE_GRPC_TLS_CA_FILE: '/run/secrets/user_core_grpc_tls_ca',
        },
        fixture.value,
      ),
    ).toThrow(/USER_CORE_GRPC_TLS_CA_FILE/);
    expect(fixture.createSsl).not.toHaveBeenCalled();
    expect(fixture.createInsecure).not.toHaveBeenCalled();
  });

  it('fails closed when the configured CA cannot be read or loaded', () => {
    const unreadable = dependencies();
    unreadable.value.readFile = jest.fn(() => {
      throw new Error('fixture read failure');
    });
    expect(() =>
      loadUserCoreGrpcChannelCredentials(
        {
          NODE_ENV: 'production',
          USER_CORE_GRPC_TLS_CA_FILE: '/run/secrets/user_core_grpc_tls_ca',
        },
        unreadable.value,
      ),
    ).toThrow(/could not be read/);

    const rejected = dependencies();
    rejected.createSsl.mockImplementation(() => {
      throw new Error('fixture TLS failure');
    });
    expect(() =>
      loadUserCoreGrpcChannelCredentials(
        {
          NODE_ENV: 'production',
          USER_CORE_GRPC_TLS_CA_FILE: '/run/secrets/user_core_grpc_tls_ca',
        },
        rejected.value,
      ),
    ).toThrow(/not a valid CA bundle/);
  });

  it('keeps an explicit insecure transport only in development', () => {
    const fixture = dependencies();

    expect(
      loadUserCoreGrpcChannelCredentials(
        { NODE_ENV: 'development' },
        fixture.value,
      ),
    ).toBe(fixture.insecure);
    expect(fixture.createSsl).not.toHaveBeenCalled();
    expect(fixture.createInsecure).toHaveBeenCalledTimes(1);
  });

  it('reads the default development environment without production fallback', () => {
    const originalNodeEnvironment = process.env.NODE_ENV;
    const originalCAFile = process.env.USER_CORE_GRPC_TLS_CA_FILE;
    process.env.NODE_ENV = 'development';
    delete process.env.USER_CORE_GRPC_TLS_CA_FILE;
    try {
      expect(loadUserCoreGrpcChannelCredentials()).toBeDefined();
    } finally {
      if (originalNodeEnvironment === undefined) delete process.env.NODE_ENV;
      else process.env.NODE_ENV = originalNodeEnvironment;
      if (originalCAFile === undefined)
        delete process.env.USER_CORE_GRPC_TLS_CA_FILE;
      else process.env.USER_CORE_GRPC_TLS_CA_FILE = originalCAFile;
    }
  });
});
