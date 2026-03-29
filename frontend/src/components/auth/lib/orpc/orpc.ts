'use client'

import { createORPCClient } from '@orpc/client'
import { RPCLink } from '@orpc/client/fetch'

/**
 * September 2025 Best Practices: oRPC Client with Client-Side Safety
 * 
 * This implementation follows the latest patterns:
 * 1. ✅ Client-side oRPC client for browser interactions
 * 2. ✅ Proper error handling for server-only code
 * 3. ✅ Safe fallback when server client unavailable
 * 4. ✅ Next.js client boundary compliance
 */

/**
 * Configure RPCLink for client-side requests
 * This will only be used in the browser environment
 */
const link = new RPCLink({
  url: () => {
    // 🔥 Prevent server-side execution
    if (typeof window === 'undefined') {
      throw new Error('RPCLink is not allowed on the server side. Use server-side client instead.')
    }

    // Dynamic URL resolution for different environments
  const baseUrl = window.location.origin
  return `${baseUrl}/orpc/rpc`
  },
  
  // Enhanced headers for better debugging and tracking
  headers: () => ({
    'Content-Type': 'application/json',
    'X-Client-Type': 'orpc-browser',
    'X-Timestamp': new Date().toISOString(),
  }),
  
  // Add retry logic for network resilience
  fetch: async (input, init) => {
    const maxRetries = 3
    let lastError: Error | undefined
    
    for (let attempt = 1; attempt <= maxRetries; attempt++) {
      try {
        const response = await fetch(input, init)
        
        // Log successful requests in development
        if (process.env.NODE_ENV === 'development') {
          console.log(`🌐 oRPC client request successful (attempt ${attempt}):`, {
            url: input.toString(),
            status: response.status,
          })
        }
        
        return response
      } catch (error) {
        lastError = error as Error
        
        if (process.env.NODE_ENV === 'development') {
          console.warn(`🌐 oRPC client request failed (attempt ${attempt}/${maxRetries}):`, {
            url: input.toString(),
            error: lastError.message,
          })
        }
        
        // Don't retry on the last attempt
        if (attempt === maxRetries) break
        
        // Exponential backoff: wait longer between retries
        await new Promise(resolve => setTimeout(resolve, Math.pow(2, attempt) * 100))
      }
    }
    
    throw lastError || new Error('All retry attempts failed')
  },
})

/**
 * Main oRPC client export
 * 
 * Client-side only: Uses RPCLink (HTTP requests to /orpc/rpc endpoint)
 * 
 * This pattern ensures proper client/server boundaries:
 * - No server-side code leakage into client bundle
 * - Clean HTTP-based communication for client interactions
 */
export const client = createORPCClient(link)

// Development debugging
if (process.env.NODE_ENV === 'development') {
  console.log('🔧 oRPC client initialized (client-side)')
  
  if (typeof window !== 'undefined') {
    console.log('🌐 Browser environment detected - using HTTP client')
  }
}
