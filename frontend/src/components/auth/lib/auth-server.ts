import { cookies } from 'next/headers';
import { fetchFromAuthService } from '@/lib/auth/auth-service-url';

export interface Session {
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
export async function getServerSession(): Promise<Session | null> {
  const shouldLogAuthErrors = process.env.NODE_ENV === 'production';

  try {
    const cookieStore = await cookies();
    
    // Get all cookies and format them for the request
    const cookieHeader = cookieStore.toString();
    
    const { response } = await fetchFromAuthService('/api/v2/auth/getSession', {
      method: 'POST',
      headers: {
        'Cookie': cookieHeader,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({}),
      // Don't cache the session response
      cache: 'no-store',
      retryOn5xx: process.env.NODE_ENV !== 'production',
    });

    if (!response.ok) {
      if (shouldLogAuthErrors) {
        console.warn('Session validation failed:', response.status, response.statusText);
      }
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
      if (shouldLogAuthErrors) {
        console.warn('Invalid session structure received:', session);
      }
      return null;
    }

    return session as Session;
  } catch (error) {
    if (shouldLogAuthErrors) {
      console.error('Error validating session on server:', error);
    }
    return null;
  }
}

/**
 * Server action to validate session and redirect if invalid
 * This can be used in Server Components to enforce authentication
 */
export async function requireAuth(): Promise<Session> {
  const session = await getServerSession();
  
  if (!session) {
    // Import redirect dynamically to avoid issues
    const { redirect } = await import('next/navigation');
    redirect('/sign-in');
    // Ensure TypeScript knows execution doesn't continue past redirect
    throw new Error('redirect');
  }
  
  return session!; // We know it's not null because we redirected above
}

/**
 * Check if user is authenticated (without redirecting)
 * Useful for conditional rendering or middleware
 */
export async function isAuthenticated(): Promise<boolean> {
  const session = await getServerSession();
  return !!session;
}

/**
 * Check if the current user has admin privileges
 * Returns true if user has admin or superadmin role
 */
export async function isAdmin(): Promise<boolean> {
  const session = await getServerSession();
  
  if (!session?.user) {
    return false;
  }
  
  const adminRoles = ['admin', 'superadmin'];
  return adminRoles.includes(session.user.role?.toLowerCase() || '');
}

/**
 * Require admin privileges - redirects if not admin
 * This enforces admin role authentication for admin endpoints
 */
export async function requireAdmin(): Promise<Session> {
  const session = await getServerSession();
  
  if (!session) {
    // Import redirect dynamically to avoid issues
    const { redirect } = await import('next/navigation');
    redirect('/sign-in');
    // Ensure TypeScript knows execution doesn't continue past redirect
    throw new Error('redirect');
  }
  
  const adminRoles = ['admin', 'superadmin'];
  const userRole = session.user.role?.toLowerCase() || '';
  
  if (!adminRoles.includes(userRole)) {
    // Import redirect dynamically to avoid issues
    const { redirect } = await import('next/navigation');
    redirect('/unauthorized');
  }
  
  return session!; // We know it's not null because we redirected above
}

/**
 * Check admin privileges without redirecting
 * Useful for API routes that need to return JSON errors
 */
export async function checkAdminRole(): Promise<{ isAdmin: boolean; session: Session | null }> {
  const session = await getServerSession();
  
  if (!session) {
    return { isAdmin: false, session: null };
  }
  
  const adminRoles = ['admin', 'superadmin'];
  const userRole = session.user.role?.toLowerCase() || '';
  const isAdmin = adminRoles.includes(userRole);
  
  return { isAdmin, session };
}
