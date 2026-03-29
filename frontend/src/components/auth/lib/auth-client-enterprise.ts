import { createAuthClient } from 'better-auth/client';
import { organizationClient, oidcClient, passkeyClient } from 'better-auth/client/plugins';
import { ssoClient } from '@better-auth/sso/client';

const AUTH_DEBUG =
  process.env.NEXT_PUBLIC_DEBUG_AUTH === '1' || process.env.NEXT_PUBLIC_DEBUG === '1';

const authDebug = (...args: unknown[]) => {
  if (AUTH_DEBUG) {
    console.log(...args);
  }
};

const baseAuthClient = createAuthClient({
  // Prefer same-origin in the browser to avoid hitting the auth service directly.
  baseURL: typeof window !== 'undefined'
    ? window.location.origin
    : process.env.NEXT_PUBLIC_APP_URL || process.env.NEXT_PUBLIC_API_URL || "http://localhost:3000",
  basePath: "/api/auth", // Next.js API route that proxies to auth service
  plugins: [
    // Organization plugin for multi-tenant organization management
    organizationClient({
      // Optional: configure additional features
    }),
    
    // OIDC client for OAuth applications
    oidcClient(),

    // SSO client for enterprise authentication
    ssoClient(),

    // Passkey client for passwordless authentication
    passkeyClient(),
  ],
  
  // Enhanced session configuration for persistence
  /* session: {
    // Enable automatic session refresh
    refresh: true,
    // Check session on window focus
    refreshOnWindowFocus: true,
    // Retry failed requests
    retry: 3,
    // Session storage for persistence across tabs/windows
    storage: typeof window !== 'undefined' ? {
      get: (key: string) => {
        try {
          return localStorage.getItem(key);
        } catch {
          return null;
        }
      },
      set: (key: string, value: string) => {
        try {
          localStorage.setItem(key, value);
        } catch {
          // Fallback to sessionStorage or ignore
        }
      },
      remove: (key: string) => {
        try {
          localStorage.removeItem(key);
        } catch {
          // Fallback to sessionStorage or ignore
        }
      }
    } : undefined,
  }, */
  
  // Enhanced error handling
  onError: (error: Error) => {
    console.warn('Auth error:', error);
    
    // Handle specific state mismatch errors
    if (error?.message?.includes('State Mismatch') || 
        error?.message?.includes('Verification not found')) {
      authDebug('🔄 State mismatch detected - clearing auth state');
      
      // Clear potentially stale auth state
      if (typeof window !== 'undefined') {
        try {
          // Clear auth-related localStorage items
          Object.keys(localStorage).forEach(key => {
            if (key.includes('auth') || key.includes('better-auth') || key.includes('session')) {
              localStorage.removeItem(key);
            }
          });
          
          // Force page refresh to restart auth flow
          window.location.reload();
        } catch (e) {
          console.warn('Failed to clear auth state:', e);
        }
      }
    }
  },
});

// Debug: Log the structure of the base client
authDebug('🔍 Base Auth Client Structure:', {
  hasSignIn: !!baseAuthClient.signIn,
  signInKeys: baseAuthClient.signIn ? Object.keys(baseAuthClient.signIn) : 'none',
  hasSocial: !!baseAuthClient.signIn?.social,
  hasGetSession: typeof baseAuthClient.getSession,
});

// Add enhanced methods to the base client
(baseAuthClient as any).refreshSession = async function() {
  try {
    const session = await baseAuthClient.getSession();
    return session;
  } catch (error) {
    console.warn('Session refresh failed:', error);
    return null;
  }
};

(baseAuthClient as any).isAuthenticated = async function(retries = 3): Promise<boolean> {
  authDebug('🔍 Custom isAuthenticated called with retries:', retries);
  for (let i = 0; i < retries; i++) {
    try {
      authDebug('🔍 Attempting auth check via getSession, attempt:', i + 1);
      const session = await baseAuthClient.getSession();
      // Handle both success and error cases properly
      if (session && 'data' in session && session.data?.user) {
        return true;
      }
      return false;
    } catch (error) {
      console.warn(`Auth check attempt ${i + 1} failed:`, error);
      if (i < retries - 1) {
        await new Promise(resolve => setTimeout(resolve, 1000 * (i + 1)));
      }
    }
  }
  return false;
};

(baseAuthClient as any).signOutCompletely = async function() {
  try {
    await baseAuthClient.signOut();
    
    // Clear all auth-related storage
    if (typeof window !== 'undefined') {
      Object.keys(localStorage).forEach(key => {
        if (key.includes('auth') || key.includes('better-auth') || key.includes('session')) {
          localStorage.removeItem(key);
        }
      });
      
      Object.keys(sessionStorage).forEach(key => {
        if (key.includes('auth') || key.includes('better-auth') || key.includes('session')) {
          sessionStorage.removeItem(key);
        }
      });
    }
  } catch (error) {
    console.warn('Sign out error:', error);
  }
};

// Export the enhanced base client directly
export const authClient = baseAuthClient;

// Debug: Log the structure of the enhanced client
authDebug('🔍 Final Auth Client Structure:', {
  hasSignIn: !!authClient.signIn,
  signInKeys: authClient.signIn ? Object.keys(authClient.signIn) : 'none',
  hasSocial: !!authClient.signIn?.social,
  hasGetSession: typeof authClient.getSession,
  getSessionExists: !!authClient.getSession,
  hasRefreshSession: typeof (authClient as any).refreshSession,
  hasIsAuthenticated: typeof (authClient as any).isAuthenticated,
});

// Type-safe auth client with all enterprise features
export type AuthClient = typeof authClient;

// Enterprise authentication methods available:
// 
// MICROSOFT AUTHENTICATION:
// - authClient.signIn.social('microsoft')
// 
// ORGANIZATION MANAGEMENT:
// - authClient.organization.create({ name, slug })
// - authClient.organization.list()
// - authClient.organization.setActive({ organizationId })
// - authClient.organization.inviteMember({ email, role })
// - authClient.organization.removeMember({ memberIdOrEmail })
// - authClient.organization.updateMemberRole({ memberId, role })
// - authClient.organization.leave({ organizationId })
// 
// SSO AUTHENTICATION:
// - authClient.signIn.sso({ email, callbackURL })
// - authClient.signIn.sso({ domain, callbackURL })
// - authClient.signIn.sso({ organizationSlug, callbackURL })
// 
// OAUTH APPLICATIONS (OIDC Provider):
// - authClient.oauth2.register({ client_name, redirect_uris })
// - authClient.oauth2.consent({ accept, consent_code })
//
// HOOKS FOR REAL-TIME UPDATES:
// - authClient.useActiveOrganization() // React hook
// - authClient.useListOrganizations() // React hook
//
// ENHANCED PERSISTENCE METHODS:
// - authClient.refreshSession() // Force session refresh
// - authClient.isAuthenticated(retries?) // Check auth with retry
// - authClient.signOutCompletely() // Enhanced sign out with cleanup
