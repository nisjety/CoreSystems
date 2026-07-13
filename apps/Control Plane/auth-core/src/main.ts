import { NestFactory } from '@nestjs/core';
import { AppModule } from './app.module';
import { SwaggerModule, DocumentBuilder } from '@nestjs/swagger';
import {
  MicroserviceOptions,
  NatsOptions,
  Transport,
} from '@nestjs/microservices';
import * as dotenv from 'dotenv';
import { SecurityHeadersMiddleware } from './common/middleware/security-headers.middleware';
import { join } from 'path';
import { ReflectionService } from '@grpc/reflection';
import { toNodeHandler } from 'better-auth/node';
import { auth } from './auth/auth';
import { OrganizationEventMiddleware } from './middleware/organization-event.middleware';
import { SharedPublisher } from './nats/shared-publisher';
import type { Server } from '@grpc/grpc-js';
import type { PackageDefinition } from '@grpc/proto-loader';
import type { Express, NextFunction, Request, Response } from 'express';

// Load environment variables
dotenv.config();

function splitOrigins(value?: string): string[] {
  return (value ?? '')
    .split(',')
    .map((origin) => origin.trim().replace(/\/+$/, ''))
    .filter(
      (origin) => origin.startsWith('http://') || origin.startsWith('https://'),
    );
}

function corsOrigins() {
  return Array.from(
    new Set([
      'http://localhost:3000',
      'http://127.0.0.1:3000',
      'http://[::1]:3000',
      'http://localhost:3107',
      'http://127.0.0.1:3107',
      'http://localhost:3012',
      process.env.BETTER_AUTH_URL || 'http://localhost:3011',
      process.env.FRONTEND_URL || 'http://localhost:3000',
      ...splitOrigins(process.env.BETTER_AUTH_TRUSTED_ORIGINS),
      ...splitOrigins(process.env.AUTH_ALLOWED_ORIGINS),
    ]),
  ).filter((origin): origin is string => Boolean(origin));
}

async function bootstrap() {
  // Creating NestJS application
  // Note: Keep bodyParser enabled for NestJS. AuthModule will handle body parsing for its routes.
  const app = await NestFactory.create(AppModule);

  // Get SharedPublisher (will delegate to SharedNatsService which initializes via OnModuleInit)
  app.get(SharedPublisher);
  console.log(
    '✅ SharedPublisher initialized (delegates to SharedNatsService)',
  );

  // Connect NATS microservice for inter-service communication
  const natsToken = process.env.NATS_TOKEN || process.env.NATS_AUTH_TOKEN;
  const natsUser = process.env.NATS_USER;
  const natsPass = process.env.NATS_PASS;

  console.log('🔍 NATS Debug:', {
    NATS_URL: process.env.NATS_URL,
    hasToken: !!natsToken,
    hasUser: !!natsUser,
    hasPass: !!natsPass,
  });

  const natsOptions: NonNullable<NatsOptions['options']> = {
    servers: [process.env.NATS_URL || 'nats://nats:4222'],
    maxReconnectAttempts: -1,
    reconnectTimeWait: 2000,
  };

  // Token auth takes precedence (for production with aquatiq root container)
  if (natsToken) {
    natsOptions.token = natsToken;
    console.log('🔐 NATS using token authentication');
  } else if (natsUser && natsPass) {
    natsOptions.user = natsUser;
    natsOptions.pass = natsPass;
    console.log('🔐 NATS using user/password authentication');
  }

  app.connectMicroservice<MicroserviceOptions>({
    transport: Transport.NATS,
    options: natsOptions,
  });

  // Connect gRPC microservice.
  // Register both auth.v1 and dataplane.auth.v1 packages on the same server/port
  // to avoid duplicate bind attempts on GRPC_PORT.
  // In production: proto files are at /app/proto (copied to Docker root)
  // In development: proto files are at workspace_root/proto
  const protoPath = join(process.cwd(), 'proto/auth/v1/auth.proto');
  const tokenValidationProtoPath = join(
    process.cwd(),
    'proto/dataplane/auth/v1/token_validation.proto',
  );

  app.connectMicroservice<MicroserviceOptions>({
    transport: Transport.GRPC,
    options: {
      package: ['auth.v1', 'dataplane.auth.v1'],
      protoPath: [protoPath, tokenValidationProtoPath],
      url: `0.0.0.0:${process.env.GRPC_PORT || 50011}`,
      loader: {
        keepCase: true,
        longs: String,
        enums: String,
        defaults: true,
        oneofs: true,
      },
      onLoadPackageDefinition: (
        pkg: PackageDefinition,
        server: Pick<Server, 'addService'>,
      ) => {
        new ReflectionService(pkg).addToServer(server);
      },
    },
  });

  // Apply security headers middleware globally
  app.use(
    new SecurityHeadersMiddleware().use.bind(new SecurityHeadersMiddleware()),
  );

  // Enable CORS for client applications
  app.enableCors({
    origin: corsOrigins(),
    methods: ['GET', 'POST', 'PUT', 'DELETE', 'OPTIONS', 'PATCH'],
    allowedHeaders: [
      'Origin',
      'X-Requested-With',
      'Content-Type',
      'Accept',
      'Authorization',
      'Cache-Control',
      'Cookie',
      'X-Forwarded-For',
      'X-Forwarded-Host',
      'X-Forwarded-Proto',
    ],
    credentials: true,
  });

  // Swagger setup for Nest controllers (e.g., /users, future modules)
  const config = new DocumentBuilder()
    .setTitle('ID-Knuten Auth Service')
    .setDescription(
      'REST API documentation for NestJS endpoints in the Auth service. For oRPC endpoints, see /orpc/docs.',
    )
    .setVersion('1.0.0')
    .addBearerAuth(
      { type: 'http', scheme: 'bearer', bearerFormat: 'JWT', in: 'header' },
      'bearer',
    )
    .addCookieAuth('auth_session', {
      type: 'apiKey',
      in: 'cookie',
      name: 'auth_session',
    })
    .build();
  const document = SwaggerModule.createDocument(app, config, {
    // Optionally include all endpoints; decorators can refine later
    deepScanRoutes: true,
  });
  SwaggerModule.setup('docs', app, document, {
    customSiteTitle: 'ID-Knuten Auth Service Docs',
    swaggerOptions: {
      docExpansion: 'list',
    },
    customCss: '.swagger-ui .topbar { display: none }',
  });

  // Manually register Better Auth handler for OAuth callbacks
  // This ensures Better Auth routes are accessible even if AuthModule.configure() doesn't run
  const typedAuth = auth as unknown as Parameters<typeof toNodeHandler>[0];
  const betterAuthHandler = toNodeHandler(typedAuth);
  const expressApp = app.getHttpAdapter().getInstance() as unknown as Express;

  // Register organization event middleware BEFORE Better Auth handler
  // This allows us to intercept responses from Better Auth organization endpoints
  try {
    const orgMiddleware = app.get(OrganizationEventMiddleware);
    console.log('🔍 Got middleware instance:', !!orgMiddleware);

    // Register middleware for organization endpoints (must come before Better Auth catch-all)
    expressApp.use((req: Request, res: Response, next: NextFunction) => {
      if (req.path.startsWith('/api/auth/organization')) {
        console.log(
          `🔍 Organization middleware intercepting: ${req.method} ${req.path}`,
        );
        orgMiddleware.use(req, res, next);
        return;
      }
      return next();
    });
    console.log('✅ Organization event middleware registered');
  } catch (error) {
    console.warn(
      '⚠️  Failed to register organization middleware:',
      error instanceof Error ? error.message : String(error),
    );
    console.warn('⚠️  Error details:', error);
  }

  // Reserve /api/auth/convex/* for Nest controllers that mint and expose
  // Convex JWT/JWKS material. Better Auth should continue owning the rest
  // of /api/auth/*.
  expressApp.all(
    /^\/api\/auth\/(?!convex(?:\/|$)).*/,
    (req: Request, res: Response) => {
      console.log(`🔵 Better Auth route hit: ${req.method} ${req.path}`);
      return betterAuthHandler(req, res);
    },
  );
  console.log('✅ Better Auth handler manually registered at /api/auth/*');

  // Start all microservices
  await app.startAllMicroservices();
  console.log('🔌 NATS microservice connected');
  console.log(
    `🔌 gRPC microservice listening on: 0.0.0.0:${process.env.GRPC_PORT || 50011}`,
  );

  await app.listen(process.env.PORT ?? 3011);
  console.log(`🚀 Auth service is running on: ${await app.getUrl()}`);
  console.log(
    `📚 Better Auth endpoints available at: ${await app.getUrl()}/api/auth/*`,
  );
  console.log(`📘 Swagger (Nest controllers): ${await app.getUrl()}/docs`);
  console.log(`📙 Docs Hub: ${await app.getUrl()}/docs/hub`);
  console.log(`📗 oRPC OpenAPI: ${await app.getUrl()}/orpc/openapi.json`);
  console.log(`📗 oRPC Swagger UI: ${await app.getUrl()}/orpc/docs`);
}
void bootstrap();
