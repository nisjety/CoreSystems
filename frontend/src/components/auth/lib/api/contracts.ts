import { z } from 'zod';
import { oc } from '@orpc/contract';

// User schema
const User = z.object({
  id: z.string(),
  email: z.string().email(),
  name: z.string(),
  emailVerified: z.boolean(),
  createdAt: z.string(),
  updatedAt: z.string(),
});

// Profile update schema
const UpdateProfileInput = z.object({
  name: z.string().min(1).optional(),
  email: z.string().email().optional(),
});

// API contracts for type-safe communication
export const userContract = oc
  .route({
    method: 'GET',
    path: '/profile',
    summary: 'Get user profile',
  })
  .output(User);

export const updateProfileContract = oc
  .route({
    method: 'PUT', 
    path: '/profile',
    summary: 'Update user profile',
  })
  .input(UpdateProfileInput)
  .output(User);

// Account management contracts
export const deleteAccountContract = oc
  .route({
    method: 'DELETE',
    path: '/account',
    summary: 'Delete user account',
  })
  .input(z.object({ 
    password: z.string().min(1) 
  }))
  .output(z.object({ 
    success: z.boolean(),
    message: z.string() 
  }));

// 2FA management contracts
export const enable2FAContract = oc
  .route({
    method: 'POST',
    path: '/2fa/enable',
    summary: 'Enable 2FA for user',
  })
  .input(z.object({
    method: z.enum(['email', 'sms', 'totp']),
    phoneNumber: z.string().optional(),
  }))
  .output(z.object({
    success: z.boolean(),
    qrCode: z.string().optional(), // For TOTP
    secret: z.string().optional(), // For TOTP
  }));

export const disable2FAContract = oc
  .route({
    method: 'POST',
    path: '/2fa/disable',
    summary: 'Disable 2FA for user',
  })
  .input(z.object({
    password: z.string().min(1),
  }))
  .output(z.object({
    success: z.boolean(),
    message: z.string(),
  }));

// Combine all contracts
export const apiContract = oc.router({
  user: userContract,
  updateProfile: updateProfileContract,
  deleteAccount: deleteAccountContract,
  enable2FA: enable2FAContract,
  disable2FA: disable2FAContract,
});

export type ApiContract = typeof apiContract;
