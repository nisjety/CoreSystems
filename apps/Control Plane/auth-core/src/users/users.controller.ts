import { Controller, Get, UseGuards, Post, Body, Req } from '@nestjs/common';
import {
  ApiBearerAuth,
  ApiCookieAuth,
  ApiOperation,
  ApiTags,
} from '@nestjs/swagger';
import {
  AuthGuard,
  Session,
  Public,
  Optional,
} from '@thallesp/nestjs-better-auth';
import type { UserSession } from '@thallesp/nestjs-better-auth';
import type { Request } from 'express';
import { auth } from '../auth/auth';

const typedPasswordAuth = auth as unknown as {
  api: {
    setPassword(input: {
      body: { newPassword: string };
      headers: { cookie: string };
    }): Promise<unknown>;
  };
};

@ApiTags('users')
@Controller('users')
export class UsersController {
  @Get('public')
  @Public()
  @ApiOperation({ summary: 'Public info (no auth required)' })
  getPublicInfo() {
    return {
      message: 'This is a public route - no authentication required',
      timestamp: new Date().toISOString(),
    };
  }

  @Get('optional')
  @Optional()
  @ApiOperation({ summary: 'Optional auth; returns session if present' })
  getOptionalAuth(@Session() session: UserSession) {
    return {
      message: 'This route has optional authentication',
      authenticated: !!session,
      user: session?.user || null,
      timestamp: new Date().toISOString(),
    };
  }

  @Get('me')
  @UseGuards(AuthGuard)
  @ApiOperation({ summary: 'Get authenticated user profile' })
  @ApiBearerAuth('bearer')
  @ApiCookieAuth('auth_session')
  getProfile(@Session() session: UserSession) {
    return {
      message: 'User profile - authentication required',
      user: session.user,
      session: {
        id: session.session.id,
        expiresAt: session.session.expiresAt,
        createdAt: session.session.createdAt,
      },
    };
  }

  @Get('protected')
  @UseGuards(AuthGuard)
  @ApiOperation({ summary: 'Protected route example' })
  @ApiBearerAuth('bearer')
  @ApiCookieAuth('auth_session')
  getProtectedData(@Session() session: UserSession) {
    return {
      message: 'This is protected data',
      userId: session.user.id,
      userEmail: session.user.email,
      timestamp: new Date().toISOString(),
    };
  }

  // Server-only: set a password for users without a credential account
  @Post('set-password')
  @UseGuards(AuthGuard)
  @ApiOperation({
    summary: 'Set password for credential-less user (server-only)',
  })
  @ApiBearerAuth('bearer')
  @ApiCookieAuth('auth_session')
  async setPassword(
    @Body() body: { newPassword: string },
    @Req() req: Request,
  ) {
    const { newPassword } = body || {};
    if (!newPassword || typeof newPassword !== 'string') {
      return { ok: false, error: 'NEW_PASSWORD_REQUIRED' };
    }
    await typedPasswordAuth.api.setPassword({
      body: { newPassword },
      headers: { cookie: req.headers.cookie ?? '' },
    });
    return { ok: true };
  }
}
