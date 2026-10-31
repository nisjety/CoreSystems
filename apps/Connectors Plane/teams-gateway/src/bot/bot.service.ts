import { Injectable, Logger } from '@nestjs/common';
import { Request, Response } from 'express';
import { ModernTeamsAdapter } from '../adapters/modern-teams.adapter';

@Injectable()
export class BotService {
  private readonly logger = new Logger(BotService.name);

  constructor(private readonly modernTeamsAdapter: ModernTeamsAdapter) {}

  async processActivity(req: Request, res: Response): Promise<void> {
    this.logger.log('🤖 Processing Bot Framework activity through Teams AI library');
    
    // Process the incoming activity through the modern Teams AI library
    await this.modernTeamsAdapter.processActivity(req, res);
  }
}
