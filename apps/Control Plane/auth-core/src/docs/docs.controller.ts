import { Controller, Get, Header, Res } from '@nestjs/common';
import type { Response } from 'express';

// Basic OpenAPI v3.1 skeleton for Better Auth endpoints. This can be extended over time.
const betterAuthOpenApi = {
  openapi: '3.1.0',
  info: {
    title: 'Better Auth Endpoints',
    version: '1.0.0',
    description:
      'Manual OpenAPI for Better Auth routes exposed under /api/auth/* (proxied by @thallesp/nestjs-better-auth). This is a curated, evolving spec.',
  },
  servers: [{ url: '/' }],
  paths: {
    '/api/auth/sign-in/email': {
      post: {
        summary: 'Sign in with email/password',
        requestBody: {
          required: true,
          content: {
            'application/json': {
              schema: {
                type: 'object',
                properties: {
                  email: { type: 'string', format: 'email' },
                  password: { type: 'string' },
                },
                required: ['email', 'password'],
              },
            },
          },
        },
        responses: {
          '200': { description: 'Signed in' },
          '401': { description: 'Invalid credentials' },
        },
      },
    },
    '/api/auth/sign-up/email': {
      post: {
        summary: 'Sign up with email/password',
        requestBody: {
          required: true,
          content: {
            'application/json': {
              schema: {
                type: 'object',
                properties: {
                  email: { type: 'string', format: 'email' },
                  password: { type: 'string' },
                  name: { type: 'string' },
                },
                required: ['email', 'password'],
              },
            },
          },
        },
        responses: {
          '200': { description: 'Account created' },
          '409': { description: 'User already exists' },
        },
      },
    },
    '/api/auth/get-session': {
      get: {
        summary: 'Get current session',
        responses: {
          '200': { description: 'Returns session if authenticated' },
        },
      },
    },
    '/api/auth/sign-out': {
      post: {
        summary: 'Sign out current session',
        responses: { '200': { description: 'Signed out' } },
      },
    },
  },
  components: {
    securitySchemes: {
      bearer: { type: 'http', scheme: 'bearer', bearerFormat: 'JWT' },
      auth_session: { type: 'apiKey', in: 'cookie', name: 'auth_session' },
    },
  },
};

const hubHtml = () => `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>ID-Knuten Docs Hub</title>
    <style>
      body { font-family: Inter, system-ui, -apple-system, Segoe UI, Roboto, sans-serif; padding: 24px; max-width: 880px; margin: auto; }
      h1 { margin-bottom: 8px; }
      .card { border: 1px solid #e5e7eb; border-radius: 12px; padding: 16px 18px; margin: 12px 0; }
      a { color: #2563eb; text-decoration: none; }
      a:hover { text-decoration: underline; }
      .muted { color: #6b7280; }
      .title { font-weight: 600; font-size: 18px; }
    </style>
  </head>
  <body>
    <h1>ID-Knuten API Documentation</h1>
    <p class="muted">Central hub for all server-side API docs.</p>

    <div class="card">
      <div class="title">Nest Controllers (Swagger)</div>
      <div>Interactive Swagger UI for Nest endpoints (e.g. /users)</div>
      <div><a href="/docs">Open /docs</a></div>
    </div>

    <div class="card">
      <div class="title">oRPC Router (OpenAPI + Swagger)</div>
      <div>OpenAPI schema generated from oRPC router.</div>
      <div>
        <a href="/orpc/docs">Open /orpc/docs</a>
        &nbsp;•&nbsp;
        <a href="/orpc/openapi.json" target="_blank">/orpc/openapi.json</a>
      </div>
    </div>

    <div class="card">
      <div class="title">Better Auth Endpoints (Manual Spec)</div>
      <div>Curated OpenAPI document for Better Auth routes under /api/auth/*</div>
      <div>
        <a href="/docs/better-auth">Open Swagger UI</a>
        &nbsp;•&nbsp;
        <a href="/docs/better-auth/openapi.json" target="_blank">/docs/better-auth/openapi.json</a>
      </div>
    </div>
  </body>
 </html>`;

// Swagger UI via CDN for the manual Better Auth spec
const swaggerHtml = (specUrl: string) => `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Better Auth Swagger</title>
    <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
  </head>
  <body>
    <div id="swagger-ui"></div>
    <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
    <script>
      window.onload = () => {
        window.ui = SwaggerUIBundle({
          url: '${specUrl}',
          dom_id: '#swagger-ui',
          presets: [SwaggerUIBundle.presets.apis],
          layout: 'BaseLayout',
        });
      };
    </script>
  </body>
</html>`;

@Controller('docs')
export class DocsController {
  @Get('hub')
  @Header('Content-Type', 'text/html')
  hub(@Res() res: Response) {
    res.send(hubHtml());
  }

  @Get('better-auth/openapi.json')
  @Header('Content-Type', 'application/json')
  betterAuthSpec() {
    return betterAuthOpenApi;
  }

  @Get('better-auth')
  @Header('Content-Type', 'text/html')
  betterAuthDocs(@Res() res: Response) {
    res.send(swaggerHtml('/docs/better-auth/openapi.json'));
  }
}
