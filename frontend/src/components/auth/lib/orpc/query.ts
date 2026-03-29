import { createTanstackQueryUtils } from '@orpc/tanstack-query'
import { client } from './orpc'

/**
 * September 2025 Best Practices: oRPC + TanStack Query Integration
 * 
 * This creates optimized query utilities that work with both SSR and CSR:
 * 1. ✅ Direct procedure calls during SSR (no HTTP)
 * 2. ✅ HTTP requests in browser for reactivity
 * 3. ✅ Full TypeScript inference
 * 4. ✅ Suspense and streaming support
 * 5. ✅ Optimistic updates and caching
 */

// Create TanStack Query utilities for oRPC
export const orpc = createTanstackQueryUtils(client)

/**
 * Enhanced query configurations for specific use cases
 * 
 * These can be used to override defaults for particular queries:
 */

// Auth queries - sensitive data with shorter cache times
export const authQueryOptions = {
  staleTime: 30 * 1000, // 30 seconds
  gcTime: 60 * 1000, // 1 minute
  refetchOnWindowFocus: true,
  retry: false, // Don't retry auth failures
}

// Profile queries - user data with moderate caching
export const profileQueryOptions = {
  staleTime: 2 * 60 * 1000, // 2 minutes
  gcTime: 10 * 60 * 1000, // 10 minutes
  refetchOnWindowFocus: false,
  retry: 1,
}

// Consent queries - stable data with longer caching
export const consentQueryOptions = {
  staleTime: 10 * 60 * 1000, // 10 minutes
  gcTime: 30 * 60 * 1000, // 30 minutes
  refetchOnWindowFocus: false,
  retry: 2,
}

/**
 * Example usage patterns for the enhanced orpc utilities:
 * 
 * // Server Component (SSR) - Direct calls, no HTTP
 * const session = await orpc.auth.getSession.query()
 * 
 * // Client Component - HTTP with caching and reactivity
 * const { data: session, isLoading } = orpc.auth.getSession.useQuery({
 *   ...authQueryOptions
 * })
 * 
 * // Suspense pattern - Streams data as it resolves
 * const { data: session } = orpc.auth.getSession.useSuspenseQuery({
 *   ...authQueryOptions
 * })
 * 
 * // Mutations with optimistic updates
 * const signInMutation = orpc.auth.signIn.useMutation({
 *   onSuccess: (data) => {
 *     // Invalidate related queries
 *     orpc.auth.getSession.invalidate()
 *   },
 * })
 * 
 * // Prefetching in Server Components
 * await orpc.auth.getSession.prefetch(queryClient)
 */

// Development helper for debugging
if (process.env.NODE_ENV === 'development') {
  console.log('🔧 oRPC TanStack Query utilities initialized')
  console.log('📊 Available procedures:', {
    auth: ['getSession', 'signIn', 'signUp'],
    consent: ['get', 'update'],
    profile: ['get', 'update'],
  })
}
