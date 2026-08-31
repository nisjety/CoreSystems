import {
  Controller,
  Post,
  Get,
  Body,
  Req,
  Res,
  HttpCode,
  HttpStatus,
  Logger,
  BadRequestException,
} from '@nestjs/common';
import {
  ApiTags,
  ApiOperation,
  ApiResponse,
  ApiSecurity,
} from '@nestjs/swagger';
import type { Request, Response } from 'express';
import { orpcRouter } from '../auth/orpc-router';
import { AuthIntegrationService } from '../internal/auth-integration.service';
import {
  assertNotDisposableEmail,
  DisposableEmailError,
} from '../security/disposable-email';

interface ErrorWithStatus extends Error {
  status?: number;
  code?: string;
}

/**
 * `orpcRouter` is a plain object of oRPC procedure builders whose exact
 * shape (including the internal `~orpc` key) is an implementation detail of
 * `@orpc/server`, not part of its public contract types. `getProcedure`
 * below resolves a dotted procedure path (e.g. `auth.signIn`) by walking
 * this structure at runtime, so it is typed narrowly against only the
 * shape it actually touches rather than against `any`.
 */
type OrpcHandler = (args: {
  input: unknown;
  context: unknown;
}) => Promise<unknown>;

interface OrpcProcedureLike {
  handler?: OrpcHandler;
  ['~orpc']?: unknown;
  [key: string]: unknown;
}

/** Shape of the Better Auth user object read off oRPC signIn/signUp/session results. */
interface BetterAuthUser {
  id: string;
  email: string;
  name?: string;
  emailVerified?: boolean;
}

/**
 * Enhanced Authentication Controller
 *
 * This provides enhanced oRPC-powered authentication endpoints that will eventually
 * replace the native Better Auth endpoints. Currently using /api/v2/auth prefix
 * to avoid conflicts while we test and verify functionality.
 *
 * Features:
 * - Full type safety from oRPC integration
 * - Better Auth security features
 * - Enhanced security headers
 * - Comprehensive API documentation
 * - Proper error handling and request tracing
 * - Profile and consent management
 */
@ApiTags('Enhanced Authentication API (v2)')
@Controller('api/v2/auth')
export class ConsolidatedAuthController {
  private readonly logger = new Logger(ConsolidatedAuthController.name);

  constructor(private authIntegrationService: AuthIntegrationService) {
    // Debug: Log router structure - look for call methods
    console.log('🔍 Router debug:', {
      keys: Object.keys(orpcRouter),
      routerType: typeof orpcRouter,
      // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
      routerPrototype: Object.getPrototypeOf(orpcRouter),
      routerMethods: Object.getOwnPropertyNames(orpcRouter),
      auth: Object.keys(orpcRouter.auth || {}),
      authSignIn: typeof orpcRouter.auth?.signIn,
      authSignInProps: orpcRouter.auth?.signIn
        ? Object.getOwnPropertyNames(
            orpcRouter.auth.signIn as unknown as Record<string, unknown>,
          )
        : 'N/A',
    });
  }

  @Post('signIn')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Sign in with email and password',
    description:
      'Authenticate user with email and password using Better Auth + oRPC',
  })
  @ApiResponse({
    status: 200,
    description: 'Successfully signed in',
    schema: {
      type: 'object',
      properties: {
        user: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            email: { type: 'string', format: 'email' },
            name: { type: 'string' },
            emailVerified: { type: 'boolean' },
            image: { type: 'string' },
            createdAt: { type: 'string' },
            updatedAt: { type: 'string' },
          },
        },
        session: { type: 'object' },
      },
    },
  })
  @ApiResponse({
    status: 401,
    description: 'Invalid credentials',
    schema: {
      type: 'object',
      properties: {
        error: { type: 'string' },
        message: { type: 'string' },
      },
    },
  })
  @ApiSecurity('cookieAuth')
  async signIn(
    @Body() body: { email: string; password: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('auth.signIn', body, request, response);
  }

  @Post('signUp')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Create new user account',
    description: 'Register a new user with Better Auth + oRPC',
  })
  @ApiResponse({
    status: 200,
    description: 'Successfully created account',
  })
  async signUp(
    @Body() body: { name: string; email: string; password: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    try {
      try {
        assertNotDisposableEmail(body.email);
      } catch (error) {
        if (error instanceof DisposableEmailError) {
          throw new BadRequestException({
            code: 'DISPOSABLE_EMAIL_BLOCKED',
            message: 'Use a permanent email address to create an account.',
          });
        }
        throw error;
      }

      // Call the original Better Auth signup
      const result = await this.handleProcedure(
        'auth.signUp',
        body,
        request,
        response,
      );

      // If signup was successful and we have user data, sync to user-service
      const signUpResult = result as
        | { user?: BetterAuthUser }
        | null
        | undefined;
      if (signUpResult?.user?.id) {
        this.logger.log(
          '✅ Successful signup detected, syncing user to user-service',
        );

        try {
          const user = signUpResult.user;
          await this.authIntegrationService.handleUserRegistration({
            id: user.id,
            email: user.email,
            name: user.name || undefined,
            emailVerified: user.emailVerified || false,
            provider: 'email',
          });

          this.logger.log('✅ User successfully synced to user-service');
        } catch (syncError) {
          this.logger.error(
            '❌ Failed to sync user to user-service:',
            syncError,
          );
          // Don't throw the error to avoid disrupting the signup flow
        }
      }

      return result;
    } catch (error) {
      this.logger.error('❌ Signup failed:', error);
      throw error;
    }
  }

  @Post('signOut')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Sign out current session',
    description: 'Sign out the current user session',
  })
  @ApiResponse({
    status: 200,
    description: 'Successfully signed out',
  })
  async signOut(
    @Body() body: Record<string, unknown>,
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('auth.signOut', body, request, response);
  }

  @Post('getSession')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Get current session',
    description: 'Retrieve current user session information',
  })
  @ApiResponse({
    status: 200,
    description: 'Session information',
  })
  async getSession(
    @Body() body: Record<string, unknown>,
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    this.logger.debug(
      `getSession hit: cookies=${request.get('cookie') ?? 'none'}`,
    );
    const result = await this.handleProcedure(
      'auth.getSession',
      body,
      request,
      response,
    );
    // Don't stringify result as it may contain circular references
    const sessionResult = result as
      | { authenticated?: boolean }
      | null
      | undefined;
    this.logger.debug(
      `getSession result: ${sessionResult?.authenticated ? 'authenticated' : 'not authenticated'}`,
    );
    return result;
  }

  @Post('profile/getProfile')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Get user profile',
    description: 'Retrieve current user profile information',
  })
  async getProfile(
    @Body() body: Record<string, unknown>,
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('profile.getProfile', body, request, response);
  }

  @Post('profile/updateProfile')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Update user profile',
    description: 'Update current user profile information',
  })
  async updateProfile(
    @Body() body: { name?: string; image?: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure(
      'profile.updateProfile',
      body,
      request,
      response,
    );
  }

  @Post('consent/get')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Get consent settings',
    description: 'Retrieve current user consent preferences',
  })
  async getConsent(
    @Body() body: Record<string, unknown>,
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('consent.get', body, request, response);
  }

  @Post('consent/update')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Update consent settings',
    description: 'Update user consent preferences',
  })
  async updateConsent(
    @Body()
    body: { analytics: boolean; marketing: boolean; necessary: boolean },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('consent.update', body, request, response);
  }

  @Post('consent/withdraw')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Withdraw consent',
    description: 'Withdraw all user consent',
  })
  async withdrawConsent(
    @Body() body: Record<string, unknown>,
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('consent.withdraw', body, request, response);
  }

  // Enhanced Email Verification Endpoints
  @Post('sendEmailVerification')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Send email verification',
    description: "Send verification email to user's email address",
  })
  @ApiResponse({
    status: 200,
    description: 'Email verification sent successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        message: { type: 'string' },
      },
    },
  })
  @ApiResponse({
    status: 400,
    description: 'Invalid email address or verification failed',
  })
  async sendEmailVerification(
    @Body() body: { email: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure(
      'auth.sendEmailVerification',
      body,
      request,
      response,
    );
  }

  @Post('verifyEmail')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Verify email address',
    description: 'Verify user email address using verification token',
  })
  @ApiResponse({
    status: 200,
    description: 'Email verified successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        user: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            email: { type: 'string', format: 'email' },
            emailVerified: { type: 'boolean' },
          },
        },
      },
    },
  })
  async verifyEmail(
    @Body() body: { token: string; callbackURL?: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('auth.verifyEmail', body, request, response);
  }

  // Enhanced Password Reset Endpoints
  @Post('sendPasswordReset')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Send password reset email',
    description: "Send password reset email to user's registered email address",
  })
  @ApiResponse({
    status: 200,
    description: 'Password reset email sent successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        message: { type: 'string' },
      },
    },
  })
  async sendPasswordReset(
    @Body() body: { email: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure(
      'auth.sendPasswordReset',
      body,
      request,
      response,
    );
  }

  @Post('resetPassword')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Reset password',
    description: 'Reset user password using reset token',
  })
  @ApiResponse({
    status: 200,
    description: 'Password reset successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        user: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            email: { type: 'string', format: 'email' },
          },
        },
      },
    },
  })
  async resetPassword(
    @Body() body: { token: string; password: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('auth.resetPassword', body, request, response);
  }

  // Sprint 2: Enhanced Security & External IDP Endpoints

  @Post('oauth/initiate')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Initiate OAuth authentication',
    description: 'Start OAuth flow for external providers (Vipps, Okta, etc.)',
  })
  @ApiResponse({
    status: 200,
    description: 'OAuth flow initiated successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        url: { type: 'string', format: 'uri' },
      },
    },
  })
  async initiateOAuth(
    @Body() body: { provider: string; redirectTo?: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('auth.initiateOAuth', body, request, response);
  }

  @Post('password/check-strength')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Check password strength',
    description:
      'Validate password strength and check against breached passwords',
  })
  @ApiResponse({
    status: 200,
    description: 'Password strength analysis',
    schema: {
      type: 'object',
      properties: {
        isStrong: { type: 'boolean' },
        isCompromised: { type: 'boolean' },
        score: { type: 'number', minimum: 0, maximum: 4 },
        feedback: { type: 'array', items: { type: 'string' } },
      },
    },
  })
  async checkPasswordStrength(
    @Body() body: { password: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure(
      'auth.checkPasswordStrength',
      body,
      request,
      response,
    );
  }

  // Two-Factor Authentication Endpoints
  @Post('2fa/enable')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Enable two-factor authentication',
    description: 'Enable 2FA for the current user account',
  })
  @ApiResponse({
    status: 200,
    description: '2FA enabled successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        secret: { type: 'string' },
        qrCode: { type: 'string' },
        backupCodes: { type: 'array', items: { type: 'string' } },
      },
    },
  })
  async enableTwoFactor(
    @Body() body: { password: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('twoFactor.enable', body, request, response);
  }

  @Post('2fa/disable')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Disable two-factor authentication',
    description: 'Disable 2FA for the current user account',
  })
  async disableTwoFactor(
    @Body() body: { password: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('twoFactor.disable', body, request, response);
  }

  @Post('2fa/verify')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Verify two-factor authentication code',
    description: 'Verify TOTP or backup code for 2FA',
  })
  async verifyTwoFactor(
    @Body() body: { code: string; type?: 'totp' | 'backup-code' },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('twoFactor.verify', body, request, response);
  }

  // Email OTP Endpoints
  @Post('otp/email/send')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Send email OTP',
    description: 'Send one-time password via email',
  })
  async sendEmailOtp(
    @Body()
    body: {
      email: string;
      type?: 'sign-in' | 'email-verification' | 'forget-password';
    },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('emailOtp.send', body, request, response);
  }

  @Post('otp/email/verify')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Verify email OTP',
    description: 'Verify one-time password sent via email',
  })
  async verifyEmailOtp(
    @Body() body: { email: string; otp: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('emailOtp.verify', body, request, response);
  }

  // Phone/SMS OTP Endpoints
  @Post('otp/sms/send')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Send SMS OTP',
    description: 'Send one-time password via SMS',
  })
  async sendPhoneOtp(
    @Body() body: { phoneNumber: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('phoneOtp.send', body, request, response);
  }

  @Post('otp/sms/verify')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Verify SMS OTP',
    description: 'Verify one-time password sent via SMS',
  })
  async verifyPhoneOtp(
    @Body() body: { phoneNumber: string; otp: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('phoneOtp.verify', body, request, response);
  }

  // Passkey Endpoints
  @Post('passkey/create')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Create passkey',
    description: 'Generate passkey creation options for WebAuthn',
  })
  async createPasskey(
    @Body() body: { email?: string; name?: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('passkey.create', body, request, response);
  }

  // Sprint 3: Organization Management Endpoints
  @Post('organization/create')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Create organization',
    description: 'Create a new organization',
  })
  @ApiResponse({
    status: 200,
    description: 'Organization created successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        organization: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            name: { type: 'string' },
            slug: { type: 'string' },
            logo: { type: 'string', nullable: true },
            createdAt: { type: 'string' },
          },
        },
      },
    },
  })
  async createOrganization(
    @Body()
    body: {
      name: string;
      slug?: string;
      logo?: string;
      metadata?: Record<string, unknown>;
    },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('organization.create', body, request, response);
  }

  @Post('organization/list')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'List organizations',
    description: 'Get list of organizations for the current user',
  })
  @ApiResponse({
    status: 200,
    description: 'Organizations retrieved successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        organizations: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              id: { type: 'string' },
              name: { type: 'string' },
              slug: { type: 'string' },
              logo: { type: 'string', nullable: true },
              role: { type: 'string', enum: ['owner', 'admin', 'member'] },
              memberCount: { type: 'number' },
              createdAt: { type: 'string' },
            },
          },
        },
      },
    },
  })
  async listOrganizations(
    @Body() body: Record<string, unknown>,
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('organization.list', body, request, response);
  }

  @Post('organization/invite-member')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Invite member to organization',
    description: 'Send invitation to join organization',
  })
  @ApiResponse({
    status: 200,
    description: 'Invitation sent successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        invitation: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            email: { type: 'string' },
            role: { type: 'string', enum: ['owner', 'admin', 'member'] },
            expiresAt: { type: 'string' },
          },
        },
      },
    },
  })
  async inviteMember(
    @Body()
    body: {
      email: string;
      role: 'owner' | 'admin' | 'member';
      organizationId?: string;
    },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure(
      'organization.inviteMember',
      body,
      request,
      response,
    );
  }

  @Post('organization/switch-active')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Switch active organization',
    description: 'Set a different organization as the active one',
  })
  @ApiResponse({
    status: 200,
    description: 'Organization switched successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        activeOrganization: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            name: { type: 'string' },
            slug: { type: 'string' },
            role: { type: 'string', enum: ['owner', 'admin', 'member'] },
          },
        },
      },
    },
  })
  async switchActiveOrganization(
    @Body() body: { organizationId: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure(
      'organization.switchActive',
      body,
      request,
      response,
    );
  }

  // ============================================================================
  // API KEYS ENDPOINTS
  // ============================================================================

  @Post('api-keys/create')
  @HttpCode(HttpStatus.CREATED)
  @ApiOperation({
    summary: 'Create API key',
    description: 'Generate a new API key for authentication',
  })
  @ApiResponse({
    status: 201,
    description: 'API key created successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        apiKey: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            key: { type: 'string' },
            name: { type: 'string' },
            expiresAt: { type: 'string', format: 'date-time' },
          },
        },
      },
    },
  })
  async createApiKey(
    @Body() body: { name: string; expiresIn?: number },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('apiKeys.create', body, request, response);
  }

  @Post('api-keys/list')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'List API keys',
    description: 'Get all API keys for the authenticated user',
  })
  @ApiResponse({
    status: 200,
    description: 'API keys retrieved successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        apiKeys: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              id: { type: 'string' },
              name: { type: 'string' },
              maskedKey: { type: 'string' },
              createdAt: { type: 'string', format: 'date-time' },
              expiresAt: { type: 'string', format: 'date-time' },
              lastUsed: { type: 'string', format: 'date-time' },
            },
          },
        },
      },
    },
  })
  async listApiKeys(
    @Body() body: Record<string, never>,
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('apiKeys.list', body, request, response);
  }

  @Post('api-keys/delete')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Delete API key',
    description: 'Remove an API key by ID',
  })
  @ApiResponse({
    status: 200,
    description: 'API key deleted successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        message: { type: 'string' },
      },
    },
  })
  async deleteApiKey(
    @Body() body: { id: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('apiKeys.delete', body, request, response);
  }

  @Post('api-keys/rotate')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Rotate API key',
    description: 'Generate a new key for an existing API key',
  })
  @ApiResponse({
    status: 200,
    description: 'API key rotated successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        apiKey: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            key: { type: 'string' },
            name: { type: 'string' },
            expiresAt: { type: 'string', format: 'date-time' },
          },
        },
      },
    },
  })
  async rotateApiKey(
    @Body() body: { id: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('apiKeys.rotate', body, request, response);
  }

  @Post('api-keys/validate')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Validate API key',
    description: 'Check if an API key is valid and active',
  })
  @ApiResponse({
    status: 200,
    description: 'API key validation result',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        valid: { type: 'boolean' },
        keyInfo: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            name: { type: 'string' },
            userId: { type: 'string' },
            expiresAt: { type: 'string', format: 'date-time' },
          },
        },
      },
    },
  })
  async validateApiKey(
    @Body() body: { key: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('apiKeys.validate', body, request, response);
  }

  // ============================================================================
  // BEARER TOKEN ENDPOINTS
  // ============================================================================

  @Post('bearer/validate')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Validate bearer token',
    description: 'Check if a bearer token is valid and active',
  })
  @ApiResponse({
    status: 200,
    description: 'Bearer token validation result',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        valid: { type: 'boolean' },
        tokenInfo: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            userId: { type: 'string' },
            expiresAt: { type: 'string', format: 'date-time' },
            scopes: { type: 'array', items: { type: 'string' } },
          },
        },
      },
    },
  })
  async validateBearerToken(
    @Body() body: { token: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('bearer.validate', body, request, response);
  }

  @Post('bearer/create')
  @HttpCode(HttpStatus.CREATED)
  @ApiOperation({
    summary: 'Create bearer token',
    description: 'Generate a new bearer token',
  })
  @ApiResponse({
    status: 201,
    description: 'Bearer token created successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        token: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            token: { type: 'string' },
            expiresAt: { type: 'string', format: 'date-time' },
            scopes: { type: 'array', items: { type: 'string' } },
          },
        },
      },
    },
  })
  async createBearerToken(
    @Body() body: { expiresIn?: number; scopes?: string[] },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('bearer.create', body, request, response);
  }

  @Post('bearer/revoke')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Revoke bearer token',
    description: 'Invalidate a bearer token',
  })
  @ApiResponse({
    status: 200,
    description: 'Bearer token revoked successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        message: { type: 'string' },
      },
    },
  })
  async revokeBearerToken(
    @Body() body: { tokenId: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('bearer.revoke', body, request, response);
  }

  @Post('bearer/list')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'List bearer tokens',
    description: 'Get all bearer tokens for the authenticated user',
  })
  @ApiResponse({
    status: 200,
    description: 'Bearer tokens retrieved successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        tokens: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              id: { type: 'string' },
              maskedToken: { type: 'string' },
              createdAt: { type: 'string', format: 'date-time' },
              expiresAt: { type: 'string', format: 'date-time' },
              lastUsed: { type: 'string', format: 'date-time' },
              scopes: { type: 'array', items: { type: 'string' } },
            },
          },
        },
      },
    },
  })
  async listBearerTokens(
    @Body() body: Record<string, never>,
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('bearer.list', body, request, response);
  }

  // ============================================================================
  // ADMIN ENDPOINTS
  // ============================================================================

  @Post('admin/users/list')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'List all users (Admin)',
    description: 'Get paginated list of all users - admin only',
  })
  @ApiResponse({
    status: 200,
    description: 'Users retrieved successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        users: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              id: { type: 'string' },
              email: { type: 'string' },
              name: { type: 'string' },
              role: { type: 'string' },
              emailVerified: { type: 'boolean' },
              banned: { type: 'boolean' },
              suspended: { type: 'boolean' },
              createdAt: { type: 'string', format: 'date-time' },
              lastSignIn: { type: 'string', format: 'date-time' },
            },
          },
        },
        pagination: {
          type: 'object',
          properties: {
            total: { type: 'number' },
            page: { type: 'number' },
            limit: { type: 'number' },
            hasMore: { type: 'boolean' },
          },
        },
      },
    },
  })
  async adminListUsers(
    @Body() body: { page?: number; limit?: number; search?: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('admin.listUsers', body, request, response);
  }

  @Post('admin/users/get')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Get user by ID (Admin)',
    description: 'Retrieve detailed user information - admin only',
  })
  @ApiResponse({
    status: 200,
    description: 'User details retrieved successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        user: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            email: { type: 'string' },
            name: { type: 'string' },
            role: { type: 'string' },
            emailVerified: { type: 'boolean' },
            banned: { type: 'boolean' },
            suspended: { type: 'boolean' },
            createdAt: { type: 'string', format: 'date-time' },
            lastSignIn: { type: 'string', format: 'date-time' },
            metadata: { type: 'object' },
          },
        },
      },
    },
  })
  async adminGetUser(
    @Body() body: { userId: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('admin.getUser', body, request, response);
  }

  @Post('admin/users/suspend')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Suspend user (Admin)',
    description: 'Temporarily suspend a user account - admin only',
  })
  @ApiResponse({
    status: 200,
    description: 'User suspended successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        message: { type: 'string' },
        user: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            suspended: { type: 'boolean' },
          },
        },
      },
    },
  })
  async adminSuspendUser(
    @Body() body: { userId: string; reason?: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('admin.suspendUser', body, request, response);
  }

  @Post('admin/users/create')
  @HttpCode(HttpStatus.CREATED)
  @ApiOperation({
    summary: 'Create user (Admin)',
    description: 'Create a new user account - admin only',
  })
  @ApiResponse({
    status: 201,
    description: 'User created successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        user: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            email: { type: 'string' },
            name: { type: 'string' },
            role: { type: 'string' },
            emailVerified: { type: 'boolean' },
            createdAt: { type: 'string', format: 'date-time' },
          },
        },
      },
    },
  })
  async adminCreateUser(
    @Body()
    body: { email: string; password: string; name: string; role?: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('admin.createUser', body, request, response);
  }

  @Post('admin/users/set-role')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Set user role (Admin)',
    description: "Update a user's role - admin only",
  })
  @ApiResponse({
    status: 200,
    description: 'User role updated successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        user: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            role: { type: 'string' },
          },
        },
      },
    },
  })
  async adminSetUserRole(
    @Body() body: { userId: string; role: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('admin.setRole', body, request, response);
  }

  @Post('admin/users/ban')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Ban user (Admin)',
    description: 'Permanently ban a user account - admin only',
  })
  @ApiResponse({
    status: 200,
    description: 'User banned successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        user: {
          type: 'object',
          properties: {
            id: { type: 'string' },
            banned: { type: 'boolean' },
          },
        },
      },
    },
  })
  async adminBanUser(
    @Body() body: { userId: string; reason?: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('admin.banUser', body, request, response);
  }

  @Post('admin/users/sessions')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'List user sessions (Admin)',
    description: 'Get all active sessions for a user - admin only',
  })
  @ApiResponse({
    status: 200,
    description: 'User sessions retrieved successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        sessions: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              id: { type: 'string' },
              userId: { type: 'string' },
              createdAt: { type: 'string', format: 'date-time' },
              lastAccessed: { type: 'string', format: 'date-time' },
              ipAddress: { type: 'string' },
              userAgent: { type: 'string' },
              active: { type: 'boolean' },
            },
          },
        },
      },
    },
  })
  async adminListUserSessions(
    @Body() body: { userId: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure(
      'admin.listUserSessions',
      body,
      request,
      response,
    );
  }

  @Post('admin/users/remove')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Remove user (Admin)',
    description: 'Permanently delete a user account and all data - admin only',
  })
  @ApiResponse({
    status: 200,
    description: 'User removed successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        message: { type: 'string' },
      },
    },
  })
  async adminRemoveUser(
    @Body() body: { userId: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure('admin.removeUser', body, request, response);
  }

  @Post('admin/organizations/list')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'List all organizations (Admin)',
    description: 'Get paginated list of all organizations - admin only',
  })
  @ApiResponse({
    status: 200,
    description: 'Organizations retrieved successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        organizations: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              id: { type: 'string' },
              name: { type: 'string' },
              slug: { type: 'string' },
              createdAt: { type: 'string', format: 'date-time' },
              memberCount: { type: 'number' },
              metadata: { type: 'object' },
            },
          },
        },
        pagination: {
          type: 'object',
          properties: {
            total: { type: 'number' },
            page: { type: 'number' },
            limit: { type: 'number' },
            hasMore: { type: 'boolean' },
          },
        },
      },
    },
  })
  async adminListOrganizations(
    @Body() body: { page?: number; limit?: number; search?: string },
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure(
      'admin.listOrganizations',
      body,
      request,
      response,
    );
  }

  @Post('admin/system/stats')
  @HttpCode(HttpStatus.OK)
  @ApiOperation({
    summary: 'Get system statistics (Admin)',
    description: 'Retrieve system-wide statistics and metrics - admin only',
  })
  @ApiResponse({
    status: 200,
    description: 'System statistics retrieved successfully',
    schema: {
      type: 'object',
      properties: {
        success: { type: 'boolean' },
        stats: {
          type: 'object',
          properties: {
            totalUsers: { type: 'number' },
            activeUsers: { type: 'number' },
            totalOrganizations: { type: 'number' },
            totalApiKeys: { type: 'number' },
            activeSessions: { type: 'number' },
            recentSignUps: { type: 'number' },
            recentLogins: { type: 'number' },
          },
        },
      },
    },
  })
  async adminGetSystemStats(
    @Body() body: Record<string, never>,
    @Req() request: Request,
    @Res() response: Response,
  ): Promise<unknown> {
    return this.handleProcedure(
      'admin.getSystemStats',
      body,
      request,
      response,
    );
  }

  @Get('debug-router')
  @ApiOperation({
    summary: 'Debug router structure',
    description: 'Inspect the oRPC router structure for debugging',
  })
  debugRouter(@Res() response: Response): Response {
    const routerInfo = {
      keys: Object.keys(orpcRouter),
      auth: orpcRouter.auth ? Object.keys(orpcRouter.auth) : 'missing',
      authSignIn: orpcRouter.auth?.signIn
        ? Object.keys(orpcRouter.auth.signIn)
        : 'missing',
      authSignInType: typeof orpcRouter.auth?.signIn,
      hasOrpcProperty: orpcRouter.auth?.signIn
        ? '~orpc' in orpcRouter.auth.signIn
        : false,
      orpcContent: orpcRouter.auth?.signIn?.['~orpc'] ? 'exists' : 'missing',
    };

    return response.json({
      success: true,
      routerInfo,
      timestamp: new Date().toISOString(),
    });
  }

  /**
   * Handle oRPC procedure calls by calling the actual procedures
   * This replaces test responses with real oRPC procedure execution
   */
  private async handleProcedure(
    procedurePath: string,
    input: unknown,
    request: Request,
    response: Response,
  ): Promise<unknown> {
    const requestId = Math.random().toString(36).substring(7);
    const startTime = Date.now();
    const clientIP = this.getClientIP(request);

    // Enhanced security headers
    this.setSecurityHeaders(response);

    // Log request
    console.log(
      `🔄 Enhanced oRPC ${request.method} /api/auth/${procedurePath}`,
      {
        requestId,
        clientIP,
        timestamp: new Date().toISOString(),
      },
    );

    try {
      // Create context for oRPC
      const context = this.createContext(request, response);

      // Get the actual oRPC procedure
      const procedure = this.getProcedure(orpcRouter, procedurePath);

      if (!procedure) {
        const duration = Date.now() - startTime;
        console.error(
          `❌ [${requestId}] Procedure not found: ${procedurePath} (${duration}ms)`,
        );
        return response.status(404).json({
          error: 'Procedure not found',
          message: `Procedure ${procedurePath} not found`,
          code: 'PROCEDURE_NOT_FOUND',
          timestamp: new Date().toISOString(),
          requestId,
        });
      }

      // Call the actual oRPC procedure. `getProcedure` may resolve to either
      // a callable handler directly or an object exposing a `.handler`
      // method (see its comments), so normalize to a callable here.
      console.log(`🔧 [${requestId}] Calling procedure: ${procedurePath}`);
      const handlerFn =
        typeof procedure === 'function' ? procedure : procedure.handler;
      if (typeof handlerFn !== 'function') {
        throw new Error(`Procedure ${procedurePath} has no callable handler`);
      }
      const result: unknown = await handlerFn({ input, context });

      // Log successful completion
      const duration = Date.now() - startTime;
      console.log(`✅ [${requestId}] Completed in ${duration}ms`);
      console.log(
        `📦 [${requestId}] Result data:`,
        JSON.stringify(result, null, 2),
      );

      // Map auth error responses to proper HTTP status codes.
      // oRPC procedures return { error: '...' } on failure instead of throwing, so
      // we must detect these and emit an appropriate 4xx status rather than 200.
      if (
        result !== null &&
        result !== undefined &&
        typeof result === 'object' &&
        'error' in result &&
        typeof (result as Record<string, unknown>).error === 'string' &&
        (result as Record<string, unknown>).error &&
        !(result as Record<string, unknown>).user &&
        (result as Record<string, unknown>).success !== true &&
        (result as Record<string, unknown>).authenticated !== true
      ) {
        const errorStatusMap: Record<string, number> = {
          'auth.signIn': 401,
          'auth.signUp': 409,
          'twoFactor.verify': 401,
          'twoFactor.enable': 400,
          'twoFactor.disable': 400,
          'auth.verifyEmail': 400,
          'auth.resetPassword': 400,
          'emailOtp.verify': 401,
          'phoneOtp.verify': 401,
        };
        const errorStatus = errorStatusMap[procedurePath] ?? 400;
        console.log(
          `⚠️ [${requestId}] Auth error response → HTTP ${errorStatus} (${duration}ms): ${String((result as Record<string, unknown>).error)}`,
        );
        return response.status(errorStatus).json(result);
      }

      return response.json(result);
    } catch (error: unknown) {
      const duration = Date.now() - startTime;
      console.error(`❌ [${requestId}] Failed after ${duration}ms:`, error);
      return this.handleError(error as ErrorWithStatus, response, requestId);
    }
  }

  private createContext(request: Request, response: Response) {
    const headers = new Headers();

    // Copy relevant headers
    const { header: sanitizedCookieHeader, meta: cookieMeta } =
      this.sanitizeSessionCookies(request.get('cookie') ?? undefined);

    if (sanitizedCookieHeader) {
      headers.set('cookie', sanitizedCookieHeader);
      if (cookieMeta.sessionCount > 1) {
        this.logger.log(
          `🔧 Sanitized ${cookieMeta.sessionCount} multi-session cookies: kept=${cookieMeta.keptSessionName ?? 'none'}; mainToken=${cookieMeta.mainSessionToken ? cookieMeta.mainSessionToken.substring(0, 8) + '...' : 'none'}`,
        );
      }
    }
    if (request.get('authorization')) {
      headers.set('authorization', request.get('authorization')!);
    }
    if (request.get('user-agent')) {
      headers.set('user-agent', request.get('user-agent')!);
    }
    if (request.get('x-forwarded-for')) {
      headers.set('x-forwarded-for', request.get('x-forwarded-for')!);
    }
    // Internal service authentication header — required for admin procedures
    if (request.get('x-internal-service-secret')) {
      headers.set(
        'x-internal-service-secret',
        request.get('x-internal-service-secret')!,
      );
    }
    if (request.get('x-internal-api-key')) {
      headers.set('x-internal-api-key', request.get('x-internal-api-key')!);
    }

    // Track if we had multiple sessions (to add deletion cookies later)
    const hadMultipleSessions = cookieMeta.sessionCount > 1;

    return {
      headers,
      setHeader: (name: string, value: string | string[]) => {
        // When Better Auth sets cookies after OAuth callback, also clear old multi-session cookies
        if (name.toLowerCase() === 'set-cookie' && hadMultipleSessions) {
          const cookieValues = Array.isArray(value) ? value : [value];

          // Check if this is setting a new sid_multi- cookie (indicates new session from OAuth)
          const hasNewMultiSession = cookieValues.some(
            (c) => c.includes('sid_multi-') && !c.includes('Max-Age=0'),
          );

          if (hasNewMultiSession) {
            this.logger.log(
              `🧹 OAuth callback - clearing ${cookieMeta.sessionCount - 1} old multi-session cookies + cached sdata`,
            );

            // Parse the incoming cookies to find old multi-session tokens
            const incomingCookies = (request.get('cookie') ?? '')
              .split(';')
              .map((c) => c.trim());
            const oldMultiSessionCookies = incomingCookies
              .filter((c) => /^(?:__Secure-)?sid_multi-/.test(c))
              .map((c) => c.split('=')[0]); // Get cookie names

            // Remove the last one (the one we kept during sanitization)
            if (oldMultiSessionCookies.length > 1) {
              oldMultiSessionCookies.pop();
            }

            // Add deletion cookies for all old sessions
            oldMultiSessionCookies.forEach((cookieName) => {
              const deletionCookie = `${cookieName}=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax; Domain=${process.env.COOKIE_DOMAIN || '.coresystem.com'}`;
              response.append('set-cookie', deletionCookie);
              this.logger.log(`🗑️  Deleting old cookie: ${cookieName}`);
            });

            // CRITICAL: Also delete the cached session data cookie (__Secure-sdata)
            // This cookie caches session data and can be out of sync with the actual session
            const sdataDeletion = `__Secure-sdata=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax; Domain=${process.env.COOKIE_DOMAIN || '.coresystem.com'}`;
            response.append('set-cookie', sdataDeletion);
            this.logger.log(
              `🗑️  Deleting cached sdata cookie (will be regenerated)`,
            );
          }
        }

        // Forward the headers normally
        if (Array.isArray(value)) {
          value.forEach((v) => response.append(name, v));
        } else {
          response.set(name, value);
        }
      },
    };
  }

  private getProcedure(
    router: Record<string, unknown>,
    path: string,
  ): OrpcProcedureLike | OrpcHandler | null {
    console.log(`🔍 Looking for procedure: ${path}`);

    // Try to get the procedure using the full path directly
    const parts = path.split('.');
    if (parts.length === 2) {
      const [namespace, procedureName] = parts;
      console.log(
        `🔍 Trying direct access: router.${namespace}.${procedureName}`,
      );

      const namespaceObj = router[namespace] as
        | Record<string, unknown>
        | undefined;
      if (namespaceObj) {
        console.log(`🔍 Found namespace ${namespace}`);
        console.log(
          `🔍 Namespace descriptors:`,
          Object.getOwnPropertyNames(namespaceObj),
        );

        const procedure = namespaceObj[procedureName];
        if (procedure) {
          console.log(
            `🔍 Found procedure ${procedureName}, type:`,
            typeof procedure,
          );
          console.log(
            `🔍 Procedure props:`,
            Object.getOwnPropertyNames(procedure),
          );

          // Check if it's a function
          if (typeof procedure === 'function') {
            console.log(`🔍 Using function directly`);
            return procedure as OrpcHandler;
          }

          const procedureLike = procedure as OrpcProcedureLike;

          // Check if it has ~orpc property
          if (procedureLike['~orpc']) {
            console.log(`🔍 Using ~orpc property`);
            return procedureLike['~orpc'] as OrpcProcedureLike;
          }

          // Check if it has handler
          if (typeof procedureLike.handler === 'function') {
            console.log(`🔍 Using handler property`);
            return procedureLike.handler;
          }

          // Try to call it as an oRPC procedure
          console.log(`🔍 Trying to use as oRPC procedure object`);
          return procedureLike;
        }
      }
    }

    console.log(`🔍 No valid procedure found for path: ${path}`);
    return null;
  }

  private getClientIP(request: Request): string {
    return (
      (request.headers['x-forwarded-for'] as string)?.split(',')[0]?.trim() ||
      (request.headers['x-real-ip'] as string) ||
      request.connection?.remoteAddress ||
      request.socket?.remoteAddress ||
      '::1'
    );
  }

  private setSecurityHeaders(response: Response): void {
    response.setHeader('X-Content-Type-Options', 'nosniff');
    response.setHeader('X-Frame-Options', 'DENY');
    response.setHeader('X-XSS-Protection', '1; mode=block');
    response.setHeader('Referrer-Policy', 'strict-origin-when-cross-origin');
    response.setHeader('X-Permitted-Cross-Domain-Policies', 'none');
    response.setHeader('X-Download-Options', 'noopen');
    response.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');
    response.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
    response.setHeader('Cross-Origin-Resource-Policy', 'same-origin');
  }

  private handleError(
    error: ErrorWithStatus,
    response: Response,
    requestId: string,
  ): Response {
    const status = error.status || 500;
    const message = error.message || 'Internal Server Error';
    const code = error.code || 'INTERNAL_ERROR';

    return response.status(status).json({
      error: status >= 500 ? 'Internal Server Error' : message,
      message,
      code,
      timestamp: new Date().toISOString(),
      requestId,
      ...(process.env.NODE_ENV === 'development' && { stack: error.stack }),
    });
  }

  private sanitizeSessionCookies(rawCookieHeader?: string) {
    if (!rawCookieHeader) {
      return {
        header: undefined,
        meta: {
          total: 0,
          sessionCount: 0,
          keptSessionName: undefined as string | undefined,
          mainSessionToken: undefined as string | undefined,
        },
      };
    }

    const cookies = rawCookieHeader
      .split(';')
      .map((c) => c.trim())
      .filter(Boolean);

    // Match multi-session cookies (sid_multi-{token})
    const multiSessionRegex = /^(?:__Secure-)?sid_multi-/;
    const multiSessionCookies = cookies.filter((c) =>
      multiSessionRegex.test(c),
    );

    // Match the main session cookie (sid or session_token)
    const mainSessionRegex =
      /^(?:__Secure-)?(?:sid|session_token)(?!_multi-|_data)/;
    const mainSessionCookie = cookies.find((c) => mainSessionRegex.test(c));

    // Keep only the newest multi-session cookie (last one)
    const keptMultiSession = multiSessionCookies.at(-1);

    // Extract the session token from the kept multi-session cookie
    // Format: __Secure-sid_multi-{token}={token}.{signature}
    let mainSessionToken: string | undefined;
    let mainSessionValue: string | undefined;

    if (keptMultiSession) {
      // Get the full value (including signature) from the kept cookie
      const parts = keptMultiSession.split('=');
      if (parts.length >= 2) {
        mainSessionValue = parts.slice(1).join('=');
        // Token is usually the part before the first dot, useful for logging
        mainSessionToken = mainSessionValue.split('.')[0];
      }
    }

    // Build sanitized cookie header
    const otherCookies = cookies.filter(
      (c) => !multiSessionRegex.test(c) && !mainSessionRegex.test(c),
    );

    const sanitized: string[] = [...otherCookies];

    // If we have a kept multi-session, add it and update the main session cookie to match
    if (keptMultiSession && mainSessionValue) {
      sanitized.push(keptMultiSession);

      // Update or create the main session cookie with the matching token + signature
      if (mainSessionCookie) {
        // Replace the existing main session cookie value with the full value to match multi-session
        const cookieName = mainSessionCookie.split('=')[0];
        sanitized.push(`${cookieName}=${mainSessionValue}`);
      } else {
        // No main session cookie exists, create one (defaulting to __Secure-sid)
        sanitized.push(`__Secure-sid=${mainSessionValue}`);
      }
    } else if (mainSessionCookie) {
      // No multi-session cookies, but main session exists - keep it
      sanitized.push(mainSessionCookie);
    }

    return {
      header: sanitized.length ? sanitized.join('; ') : undefined,
      meta: {
        total: cookies.length,
        sessionCount: multiSessionCookies.length,
        keptSessionName: keptMultiSession
          ? keptMultiSession.split('=')[0]
          : undefined,
        mainSessionToken,
      },
    };
  }
}
