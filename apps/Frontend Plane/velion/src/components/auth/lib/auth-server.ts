import { cache } from 'react';
import { cookies } from 'next/headers';

// Backend URL configuration
const getBackendUrl = () => {
  // Use the same env var as other API routes for consistency
  return process.env.AUTH_SERVICE_URL || process.env.BACKEND_URL || 'http://auth-service:3011';
};

interface Session {
  user: {
    id: string;
    email: string;
    name: string;
    image?: string;
    emailVerified: boolean;
    role?: string;
    banned?: boolean;
    banReason?: string;
    banExpires?: string;
  };
  session: {
    id: string;
    userId: string;
    expiresAt: string;
    impersonatedBy?: string;
  };
}

/**
 * Server-side session validation
 * This function should be called from Server Components or Server Actions
 * to properly validate the session on the server side
 */
export const getServerSession = cache(async (): Promise<Session | null> => {
  try {
    const cookieStore = await cookies();
    
    // Get all cookies and format them for the request
    const cookieHeader = cookieStore.toString();
    
    const BACKEND_URL = getBackendUrl();
    // G30 v3: use Better Auth's native endpoint (GET) instead of the custom
    // oRPC wrapper (POST). The custom one has shown intermittent
    // "not authenticated" responses for cookies that Better Auth accepts.
    const response = await fetch(`${BACKEND_URL}/api/auth/get-session`, {
      method: 'GET',
      headers: {
        'Cookie': cookieHeader,
        'Content-Type': 'application/json',
      },
      // Don't cache the session response
      cache: 'no-store',
    });

    if (!response.ok) {
      console.log('Session validation failed:', response.status, response.statusText);
      return null;
    }

    const sessionData = await response.json();
    
    // Check if we got null response (no session)
    if (!sessionData) {
      return null;
    }
    
    if (sessionData.authenticated === false) {
      return null;
    }

    // Handle Better Auth response structure - can be either:
    // 1. { data: { user, session } } format from Better Auth
    // 2. Direct { user, session } format
    // 3. { authenticated: true, user, session } from proxy endpoints
    let session = sessionData;
    if (sessionData.data) {
      session = sessionData.data;
    }
    
    // Validate the session structure
    if (!session?.user?.id) {
      console.log('Invalid session structure received:', session);
      return null;
    }

    return session as Session;
  } catch (error) {
    if (error instanceof Error && error.message.includes('Dynamic server usage')) {
      return null;
    }
    console.error('Error validating session on server:', error);
    return null;
  }
});

/**
 * Server action to validate session and redirect if invalid
 * This can be used in Server Components to enforce authentication
 */
export async function requireAuth(): Promise<Session> {
  const session = await getServerSession();
  
  if (!session) {
    // Import redirect dynamically to avoid issues
    const { redirect } = await import('next/navigation');
    redirect('/login');
    // Ensure TypeScript knows execution doesn't continue past redirect
    throw new Error('redirect');
  }
  
  return session!; // We know it's not null because we redirected above
}
