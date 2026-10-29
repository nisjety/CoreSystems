import { Controller, Post, Req, Res, Logger } from '@nestjs/common';
import { Request, Response } from 'express';
import { BotService } from './bot.service';

@Controller('api')
export class BotController {
  private readonly logger = new Logger(BotController.name);

  constructor(private readonly botService: BotService) {}

  @Post('messages')
  async processActivity(@Req() req: Request, @Res() res: Response): Promise<void> {
    this.logger.log('Received activity from Microsoft Teams');
    
    try {
      await this.botService.processActivity(req, res);
      
      // Only send response if not already sent
      if (!res.headersSent) {
        res.status(200).end();
      }
    } catch (error) {
      this.logger.error('Error processing activity:', error);
      
      // Only send error response if headers not already sent
      if (!res.headersSent) {
        res.status(500).json({ error: 'Internal server error' });
      }
    }
  }
}
