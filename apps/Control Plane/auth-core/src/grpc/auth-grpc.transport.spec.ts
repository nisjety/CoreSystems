import {
  status,
  credentials,
  loadPackageDefinition,
  Metadata,
} from '@grpc/grpc-js';
import type { ServiceError } from '@grpc/grpc-js';
import { loadSync } from '@grpc/proto-loader';
import type { INestMicroservice } from '@nestjs/common';
import { Transport } from '@nestjs/microservices';
import { Test } from '@nestjs/testing';
import { createServer } from 'node:net';
import { resolve } from 'node:path';

import { auth } from '../auth/auth';
import { AuthGrpcController } from './auth-grpc.controller';

jest.mock('../auth/auth', () => ({
  auth: {
    api: {
      getSession: jest.fn(),
      signInEmail: jest.fn(),
      signOut: jest.fn(),
      signUpEmail: jest.fn(),
    },
  },
}));

type UnaryCallback = (error: ServiceError | null, response?: unknown) => void;
type UnaryMethod = (
  request: Readonly<Record<string, unknown>>,
  metadata: Metadata,
  callback: UnaryCallback,
) => void;
type GrpcClient = Readonly<Record<string, UnaryMethod>> & {
  close(): void;
};
type GrpcConstructor = new (
  address: string,
  channelCredentials: ReturnType<typeof credentials.createInsecure>,
) => GrpcClient;

const oldToken = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const newToken = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
const readonlyToken = 'cccccccccccccccccccccccccccccccc';

function registry(): string {
  return JSON.stringify([
    {
      credentialId: 'gateway-old',
      principal: 'verevon-gateway',
      audience: 'auth-core',
      token: oldToken,
      scopes: ['auth:token:validate'],
    },
    {
      credentialId: 'gateway-new',
      principal: 'verevon-gateway',
      audience: 'auth-core',
      token: newToken,
      scopes: ['auth:token:validate'],
    },
    {
      credentialId: 'gateway-readonly',
      principal: 'verevon-gateway',
      audience: 'auth-core',
      token: readonlyToken,
      scopes: ['auth:user:read'],
    },
  ]);
}

function serviceMetadata(
  credentialId: string,
  token: string,
  principal = 'verevon-gateway',
): Metadata {
  const metadata = new Metadata();
  metadata.set('x-service-credential-id', credentialId);
  metadata.set('x-service-principal', principal);
  metadata.set('x-service-auth', token);
  return metadata;
}

function unary(
  client: GrpcClient,
  method: string,
  request: Readonly<Record<string, unknown>>,
  metadata = new Metadata(),
): Promise<unknown> {
  return new Promise((resolveCall, rejectCall) => {
    client[method](request, metadata, (error, response) => {
      if (error) {
        rejectCall(error);
        return;
      }
      resolveCall(response);
    });
  });
}

async function unusedLocalPort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolveListen, rejectListen) => {
    server.once('error', rejectListen);
    server.listen(0, '127.0.0.1', resolveListen);
  });
  const address = server.address();
  if (!address || typeof address === 'string') {
    server.close();
    throw new Error('could not allocate a local gRPC port');
  }
  await new Promise<void>((resolveClose, rejectClose) => {
    server.close((error) => (error ? rejectClose(error) : resolveClose()));
  });
  return address.port;
}

function grpcConstructor(
  definition: unknown,
  path: readonly string[],
): GrpcConstructor {
  let value = definition as Record<string, unknown>;
  for (const segment of path) {
    value = value[segment] as Record<string, unknown>;
  }
  return value as unknown as GrpcConstructor;
}

describe('Auth gRPC scoped credentials over the real transport', () => {
  const originalEnv = { ...process.env };
  let application: INestMicroservice;
  let authClient: GrpcClient;
  let tokenClient: GrpcClient;

  beforeAll(async () => {
    process.env = {
      ...originalEnv,
      NODE_ENV: 'test',
      AUTH_GRPC_SERVICE_CREDENTIALS: registry(),
    };
    delete process.env.AUTH_GRPC_SERVICE_CREDENTIALS_FILE;
    delete process.env.INTERNAL_API_KEY;
    delete process.env.INTERNAL_SERVICE_SECRET;
    delete process.env.JWT_SECRET;

    const port = await unusedLocalPort();
    const address = `127.0.0.1:${port}`;
    const authProto = resolve(process.cwd(), 'proto/auth/v1/auth.proto');
    const tokenProto = resolve(
      process.cwd(),
      'proto/dataplane/auth/v1/token_validation.proto',
    );
    const moduleRef = await Test.createTestingModule({
      controllers: [AuthGrpcController],
    }).compile();
    application = moduleRef.createNestMicroservice({
      transport: Transport.GRPC,
      options: {
        package: ['auth.v1', 'dataplane.auth.v1'],
        protoPath: [authProto, tokenProto],
        url: address,
        loader: { keepCase: true, defaults: true, oneofs: true },
      },
    });
    await application.listen();

    const packageDefinition = loadSync([authProto, tokenProto], {
      keepCase: true,
      defaults: true,
      oneofs: true,
    });
    const grpcDefinition = loadPackageDefinition(packageDefinition);
    const AuthService = grpcConstructor(grpcDefinition, [
      'auth',
      'v1',
      'AuthService',
    ]);
    const TokenValidationService = grpcConstructor(grpcDefinition, [
      'dataplane',
      'auth',
      'v1',
      'TokenValidationService',
    ]);
    const insecure = credentials.createInsecure();
    authClient = new AuthService(address, insecure);
    tokenClient = new TokenValidationService(address, insecure);
  });

  afterAll(async () => {
    authClient.close();
    tokenClient.close();
    await application.close();
    process.env = originalEnv;
  });

  it('keeps health unauthenticated', async () => {
    await expect(unary(authClient, 'healthCheck', {})).resolves.toMatchObject({
      status: 1,
    });
  });

  it.each([
    ['missing metadata', new Metadata()],
    ['wrong credential id', serviceMetadata('retired-id', oldToken)],
    ['retired token', serviceMetadata('gateway-old', 'retired-token-value')],
    [
      'wrong principal',
      serviceMetadata('gateway-old', oldToken, 'retrieval-engine'),
    ],
  ])('reports UNAUTHENTICATED for %s', async (_name, metadata) => {
    await expect(
      unary(tokenClient, 'validateToken', { token: '' }, metadata),
    ).rejects.toMatchObject({ code: status.UNAUTHENTICATED });
  });

  it('rejects ambiguous repeated credential metadata as UNAUTHENTICATED', async () => {
    const metadata = serviceMetadata('gateway-old', oldToken);
    metadata.add('x-service-auth', newToken);

    await expect(
      unary(tokenClient, 'validateToken', { token: '' }, metadata),
    ).rejects.toMatchObject({ code: status.UNAUTHENTICATED });
  });

  it('reports PERMISSION_DENIED when the verified principal lacks the method scope', async () => {
    await expect(
      unary(
        authClient,
        'signOut',
        { token: 'session-token' },
        serviceMetadata('gateway-readonly', readonlyToken),
      ),
    ).rejects.toMatchObject({ code: status.PERMISSION_DENIED });
    expect(auth.api.signOut).not.toHaveBeenCalled();
  });

  it.each([
    ['old rotation credential', 'gateway-old', oldToken],
    ['new rotation credential', 'gateway-new', newToken],
  ])('accepts the %s only for its bounded method', async (_name, id, token) => {
    await expect(
      unary(
        tokenClient,
        'validateToken',
        { token: '' },
        serviceMetadata(id, token),
      ),
    ).resolves.toMatchObject({ valid: false });
  });
});
