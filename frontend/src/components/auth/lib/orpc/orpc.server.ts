import 'server-only'

import { createRouterClient } from '@orpc/server'
import { headers } from 'next/headers'
import type { RouterClient } from '@orpc/server'
import { z } from 'zod'
import { os } from '@orpc/server'

/**
 * September 2025 Best Practices Implementation
 * 
 * This server-side oRPC client follows the latest optimization patterns:
 * 1. ✅ Eliminates HTTP roundtrips during SSR
 * 2. ✅ Function-based context for per-request isolation  
 * 3. ✅ Headers integration for auth and tracking
 * 4. ✅ Development debugging support
 * 5. ✅ Type-safe globalThis pattern
 */

// Enhanced router matching our backend structure
const mockRouter = {
  auth: {
    getSession: os
      .input(z.object({}))
      .output(z.object({
        user: z.object({
          id: z.string(),
          name: z.string(),
          email: z.string().email(),
          emailVerified: z.boolean(),
          createdAt: z.date(),
          updatedAt: z.date(),
        }).nullable(),
        session: z.object({
          id: z.string(),
          userId: z.string(),
          expiresAt: z.date(),
          ipAddress: z.string().optional(),
          userAgent: z.string().optional(),
        }).nullable(),
      }))
      .handler(async () => {
        if (process.env.NODE_ENV === 'development') {
          console.log('🔐 Server getSession called')
        }
        
        return {
          user: null,
          session: null,
        }
      }),

    signIn: os
      .input(z.object({
        email: z.string().email(),
        password: z.string().min(1),
      }))
      .output(z.object({
        success: z.boolean(),
        user: z.object({
          id: z.string(),
          email: z.string(),
          name: z.string(),
        }).optional(),
      }))
      .handler(async ({ input }) => {
        if (process.env.NODE_ENV === 'development') {
          console.log('🔐 Server signIn called for:', input.email)
        }
        
        // Mock implementation - will connect to Better Auth later
        return {
          success: false,
        }
      }),

    signUp: os
      .input(z.object({
        email: z.string().email(),
        password: z.string().min(8),
        name: z.string().min(1),
      }))
      .output(z.object({
        success: z.boolean(),
        user: z.object({
          id: z.string(),
          email: z.string(),
          name: z.string(),
        }).optional(),
      }))
      .handler(async ({ input }) => {
        if (process.env.NODE_ENV === 'development') {
          console.log('🔐 Server signUp called for:', input.email)
        }
        
        // Mock implementation - will connect to Better Auth later
        return {
          success: false,
        }
      }),
  },
  
  consent: {
    get: os
      .input(z.object({}))
      .output(z.object({
        purposes: z.record(z.string(), z.boolean()),
        timestamp: z.string(),
        version: z.string(),
        method: z.string(),
      }).nullable())
      .handler(async () => {
        if (process.env.NODE_ENV === 'development') {
          console.log('📋 Server consent.get called')
        }
        return null
      }),
    
    update: os
      .input(z.object({
        purposes: z.record(z.string(), z.boolean()),
        method: z.enum(['banner', 'settings', 'api']).default('banner'),
      }))
      .output(z.object({
        purposes: z.record(z.string(), z.boolean()),
        timestamp: z.string(),
        version: z.string(),
        method: z.string(),
      }))
      .handler(async ({ input }) => {
        if (process.env.NODE_ENV === 'development') {
          console.log('📋 Server consent.update called:', input.method)
        }
        
        return {
          purposes: input.purposes,
          timestamp: new Date().toISOString(),
          version: '1.0',
          method: input.method,
        }
      }),
  },

  profile: {
    get: os
      .input(z.object({}))
      .output(z.object({
        id: z.string(),
        name: z.string(),
        email: z.string().email(),
        avatar: z.string().optional(),
        preferences: z.object({
          theme: z.enum(['light', 'dark', 'system']),
          language: z.string(),
          notifications: z.boolean(),
        }),
      }).nullable())
      .handler(async () => {
        if (process.env.NODE_ENV === 'development') {
          console.log('👤 Server profile.get called')
        }
        return null
      }),

    update: os
      .input(z.object({
        name: z.string().optional(),
        preferences: z.object({
          theme: z.enum(['light', 'dark', 'system']).optional(),
          language: z.string().optional(),
          notifications: z.boolean().optional(),
        }).optional(),
      }))
      .output(z.object({
        success: z.boolean(),
        profile: z.object({
          id: z.string(),
          name: z.string(),
          email: z.string().email(),
          preferences: z.object({
            theme: z.enum(['light', 'dark', 'system']),
            language: z.string(),
            notifications: z.boolean(),
          }),
        }).optional(),
      }))
      .handler(async ({ input }) => {
        if (process.env.NODE_ENV === 'development') {
          console.log('👤 Server profile.update called:', input)
        }
        
        return {
          success: false,
        }
      }),
  },
}

// Type-safe global declaration
declare global {
  var $client: RouterClient<typeof mockRouter> | undefined
}

/**
 * 🚀 2025 Best Practice: Function-based context
 * 
 * This approach ensures:
 * - Per-request context isolation (security)
 * - Headers are accessible from Next.js
 * - Auth context can be added easily
 * - Performance optimization through direct calls
 */
globalThis.$client = createRouterClient(mockRouter, {
  context: async () => {
    const requestHeaders = await headers()
    
    return {
      headers: requestHeaders,
      // Future: Add auth context
      // session: await getServerSession(),
      // user: await getCurrentUser(),
      
      // Request metadata for debugging/analytics
      timestamp: new Date().toISOString(),
      source: 'ssr',
    }
  },
})

if (process.env.NODE_ENV === 'development') {
  console.log('✅ Server-side oRPC client initialized with enhanced context support')
  console.log('📊 Router procedures available:', Object.keys(mockRouter))
}
