import * as grpc from '@grpc/grpc-js';
import { X509Certificate } from 'node:crypto';
import { readFileSync } from 'node:fs';

const MAXIMUM_CA_FILE_BYTES = 64 * 1024;

export type UserCoreGrpcTransportEnvironment = Readonly<{
  NODE_ENV?: string;
  USER_CORE_GRPC_TLS_CA_FILE?: string;
}>;

export type UserCoreGrpcTransportDependencies = {
  readFile: (path: string) => Buffer;
  createSsl: (rootCertificates: Buffer) => grpc.ChannelCredentials;
  createInsecure: () => grpc.ChannelCredentials;
};

const defaultDependencies: UserCoreGrpcTransportDependencies = {
  readFile: readFileSync,
  createSsl: grpc.credentials.createSsl,
  createInsecure: grpc.credentials.createInsecure,
};

function configuredTransportEnvironment(): UserCoreGrpcTransportEnvironment {
  return {
    NODE_ENV: process.env.NODE_ENV,
    USER_CORE_GRPC_TLS_CA_FILE: process.env.USER_CORE_GRPC_TLS_CA_FILE,
  };
}

function readCertificateAuthority(
  path: string,
  dependencies: UserCoreGrpcTransportDependencies,
): Buffer {
  let contents: Buffer;
  try {
    contents = dependencies.readFile(path);
  } catch {
    throw new Error('USER_CORE_GRPC_TLS_CA_FILE could not be read');
  }
  if (contents.length === 0 || contents.length > MAXIMUM_CA_FILE_BYTES) {
    throw new Error('USER_CORE_GRPC_TLS_CA_FILE has an invalid size');
  }
  const pem = contents.toString('ascii');
  const certificates = pem.match(
    /-----BEGIN CERTIFICATE-----[\s\S]+?-----END CERTIFICATE-----/g,
  );
  if (!certificates || certificates.length === 0) {
    throw new Error('USER_CORE_GRPC_TLS_CA_FILE is not a PEM certificate');
  }
  try {
    for (const certificate of certificates) {
      new X509Certificate(certificate);
    }
  } catch {
    throw new Error('USER_CORE_GRPC_TLS_CA_FILE contains invalid X.509 data');
  }
  return contents;
}

export function loadUserCoreGrpcChannelCredentials(
  environment: UserCoreGrpcTransportEnvironment = configuredTransportEnvironment(),
  dependencies: UserCoreGrpcTransportDependencies = defaultDependencies,
): grpc.ChannelCredentials {
  const certificateAuthorityFile =
    environment.USER_CORE_GRPC_TLS_CA_FILE?.trim();
  if (certificateAuthorityFile) {
    const certificateAuthority = readCertificateAuthority(
      certificateAuthorityFile,
      dependencies,
    );
    try {
      return dependencies.createSsl(certificateAuthority);
    } catch {
      throw new Error('USER_CORE_GRPC_TLS_CA_FILE is not a valid CA bundle');
    }
  }

  if (environment.NODE_ENV?.trim().toLowerCase() === 'production') {
    throw new Error('USER_CORE_GRPC_TLS_CA_FILE is required in production');
  }
  return dependencies.createInsecure();
}
