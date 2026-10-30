import { Controller, Get } from '@nestjs/common';

@Controller('health')
export class HealthController {
  @Get()
  getHealth() {
    return {
      status: 'ok',
      service: 'teams-gateway',
      timestamp: new Date().toISOString(),
      version: process.env.npm_package_version || '1.0.0',
    };
  }
}
