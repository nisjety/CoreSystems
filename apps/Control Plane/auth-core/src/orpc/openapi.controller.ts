import { Controller, Get, Header, Res } from '@nestjs/common';
import type { Response } from 'express';
import { OpenAPIGenerator } from '@orpc/openapi';
import { ZodToJsonSchemaConverter } from '@orpc/zod/zod4';
import type { AnyContractRouter } from '@orpc/contract';
import { orpcRouter } from '../auth/orpc-router';

// Simple Swagger UI HTML using CDN
const swaggerHtml = (specUrl: string) => `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>oRPC OpenAPI Docs</title>
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

@Controller('orpc')
export class OpenApiController {
  private generator = new OpenAPIGenerator({
    schemaConverters: [new ZodToJsonSchemaConverter()],
  });

  @Get('openapi.json')
  @Header('Content-Type', 'application/json')
  async getSpec(): Promise<any> {
    const spec = await this.generator.generate(
      orpcRouter as unknown as AnyContractRouter,
      {
        info: {
          title: 'ID-Knuten Auth oRPC API',
          version: '1.0.0',
          description: 'OpenAPI schema generated from oRPC router',
        },
        servers: [{ url: '/orpc/rpc' }],
      },
    );
    return spec;
  }

  @Get('docs')
  getDocs(@Res() res: Response) {
    // Resolve correct URL relative to current host
    res.set('Content-Type', 'text/html');
    res.send(swaggerHtml('/orpc/openapi.json'));
  }
}
