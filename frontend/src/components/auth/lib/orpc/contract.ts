import { z } from 'zod';

// Auth schemas
export const SignInSchema = z.object({
  email: z.string().email(),
  password: z.string().min(1),
});

export const SignUpSchema = z.object({
  name: z.string().min(1),
  email: z.string().email(),
  password: z.string().min(8),
});

export const UserSchema = z.object({
  id: z.string(),
  name: z.string(),
  email: z.string().email(),
  emailVerified: z.boolean(),
  createdAt: z.date(),
  updatedAt: z.date(),
});

export const SessionSchema = z.object({
  id: z.string(),
  userId: z.string(),
  expiresAt: z.date(),
  ipAddress: z.string().optional(),
  userAgent: z.string().optional(),
});

// Consent schemas  
export const ConsentPurposeSchema = z.enum([
  'necessary',
  'analytics', 
  'marketing',
  'personalization',
  'social_media',
]);

export const ConsentDataSchema = z.object({
  purposes: z.record(ConsentPurposeSchema, z.boolean()),
  timestamp: z.string(),
  version: z.string().default('1.0'),
  method: z.enum(['banner', 'settings', 'api']).default('banner'),
});

export const ConsentUpdateSchema = z.object({
  purposes: z.record(ConsentPurposeSchema, z.boolean()),
  method: z.enum(['banner', 'settings', 'api']).default('banner'),
});

// Router types - these match the backend router structure
export type AuthRouter = {
  auth: {
    signIn: any;
    signUp: any;
    signOut: any;
    getSession: any;
  };
  profile: {
    getProfile: any;
    updateProfile: any;
  };
  consent: {
    get: any;
    update: any;
    withdraw: any;
  };
};
