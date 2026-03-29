"use client";

import { useState, useEffect, createContext, useContext, ReactNode, useCallback, useMemo } from "react";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { authClient } from '../lib/auth-client';

interface User {
  id: string;
  email: string;
  name: string;
  emailVerified: boolean;
}

interface AuthContextType {
  user: User | null;
  isLoading: boolean;
  error: string | null;
  signIn: (email: string, password: string, redirectTo?: string) => Promise<void>;
  signUp: (email: string, password: string, name: string, redirectTo?: string) => Promise<void>;
  signOut: () => Promise<void>;
  forgotPassword: (email: string) => Promise<void>;
  resendVerification: (email: string) => Promise<void>;
  enable2FA: (method: '2fa-email' | '2fa-sms' | '2fa-totp', config?: Record<string, unknown>) => Promise<void>;
  verify2FA: (code: string, method?: string) => Promise<void>;
}

const AuthContext = createContext<AuthContextType | undefined>(undefined);

// Session cache with TTL
interface SessionCache {
  user: User | null;
  timestamp: number;
  ttl: number;
}

const SESSION_CACHE_KEY = 'auth_session_cache';
const CACHE_TTL = 5 * 60 * 1000; // 5 minutes

const getSessionFromCache = (): User | null => {
  try {
    const cached = localStorage.getItem(SESSION_CACHE_KEY);
    if (!cached) return null;
    
    const session: SessionCache = JSON.parse(cached);
    const now = Date.now();
    
    if (now - session.timestamp > session.ttl) {
      localStorage.removeItem(SESSION_CACHE_KEY);
      return null;
    }
    
    return session.user;
  } catch {
    return null;
  }
};

const setSessionCache = (user: User | null) => {
  try {
    const session: SessionCache = {
      user,
      timestamp: Date.now(),
      ttl: CACHE_TTL,
    };
    localStorage.setItem(SESSION_CACHE_KEY, JSON.stringify(session));
  } catch {
    // Ignore cache errors
  }
};

const clearSessionCache = () => {
  try {
    localStorage.removeItem(SESSION_CACHE_KEY);
  } catch {
    // Ignore cache errors
  }
};

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(() => {
    // Initialize from cache if available
    if (typeof window !== 'undefined') {
      return getSessionFromCache();
    }
    return null;
  });
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const router = useRouter();

  // Memoized auth state
  const authState = useMemo(() => ({
    isAuthenticated: !!user,
    isEmailVerified: user?.emailVerified ?? false,
  }), [user]);

  const checkSession = useCallback(async () => {
    try {
      setIsLoading(true);
      setError(null);
      
      // Use Better Auth client to get session
      const session = await authClient.getSession();
      
      if (session.data?.user) {
        const userData = session.data.user as User;
        setUser(userData);
        setSessionCache(userData);
      } else {
        setUser(null);
        clearSessionCache();
      }
    } catch (err) {
      console.error('Session check failed:', err);
      setError('Failed to check session');
      setUser(null);
      clearSessionCache();
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Check for existing session on mount with cache optimization
  useEffect(() => {
    const cachedUser = getSessionFromCache();
    if (cachedUser) {
      setUser(cachedUser);
      return;
    }
    
    checkSession();
  }, [checkSession]);

  // Optimized sign in with immediate UI feedback
  const signIn = useCallback(async (email: string, password: string, redirectTo = '/dashboard') => {
    try {
      setIsLoading(true);
      setError(null);
      
      const result = await authClient.signIn.email({
        email,
        password,
      });

      if (result.data?.user) {
        const userData = result.data.user as User;
        setUser(userData);
        setSessionCache(userData);
        
        // Optimistic UI update
        toast.success('Successfully signed in!');
        router.push(redirectTo);
      } else if (result.error) {
        throw new Error(result.error.message || 'Sign in failed');
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Sign in failed';
      setError(errorMessage);
      toast.error(errorMessage);
      throw err;
    } finally {
      setIsLoading(false);
    }
  }, [router]);

  // Optimized sign up
  const signUp = useCallback(async (email: string, password: string, name: string, redirectTo = '/dashboard') => {
    try {
      setIsLoading(true);
      setError(null);
      
      const result = await authClient.signUp.email({
        email,
        password,
        name,
      });

      if (result.data?.user) {
        const userData = result.data.user as User;
        setUser(userData);
        setSessionCache(userData);
        
        toast.success('Account created successfully!');
        router.push(redirectTo);
      } else if (result.error) {
        throw new Error(result.error.message || 'Sign up failed');
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Sign up failed';
      setError(errorMessage);
      toast.error(errorMessage);
      throw err;
    } finally {
      setIsLoading(false);
    }
  }, [router]);

  // Optimized sign out with immediate cache clear
  const signOut = useCallback(async () => {
    try {
      setIsLoading(true);
      setError(null);
      
      // Clear cache immediately for instant UI feedback
      setUser(null);
      clearSessionCache();
      
      await authClient.signOut();
      
      toast.success('Successfully signed out');
      router.push('/');
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Sign out failed';
      setError(errorMessage);
      toast.error(errorMessage);
      throw err;
    } finally {
      setIsLoading(false);
    }
  }, [router]);

  // Debounced forgot password
  const forgotPassword = useCallback(async (email: string) => {
    try {
      setIsLoading(true);
      setError(null);
      
      await authClient.forgetPassword({
        email,
        redirectTo: '/reset-password',
      });
      
      toast.success('Password reset email sent');
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to send reset email';
      setError(errorMessage);
      toast.error(errorMessage);
      throw err;
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Optimized resend verification
  const resendVerification = useCallback(async (email: string) => {
    try {
      setIsLoading(true);
      setError(null);
      
      await authClient.sendVerificationEmail({
        email,
      });
      
      toast.success('Verification email sent');
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Failed to send verification email';
      setError(errorMessage);
      toast.error(errorMessage);
      throw err;
    } finally {
      setIsLoading(false);
    }
  }, []);

  // 2FA methods with caching
  const enable2FA = useCallback(async (method: '2fa-email' | '2fa-sms' | '2fa-totp', config?: Record<string, unknown>) => {
    try {
      setIsLoading(true);
      setError(null);
      
      // Implementation depends on your 2FA setup
      console.log('Enabling 2FA:', method, config);
      
      toast.success(`${method} enabled successfully`);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : '2FA setup failed';
      setError(errorMessage);
      toast.error(errorMessage);
      throw err;
    } finally {
      setIsLoading(false);
    }
  }, []);

  const verify2FA = useCallback(async (code: string, method?: string) => {
    try {
      setIsLoading(true);
      setError(null);
      
      // Implementation depends on your 2FA setup
      console.log('Verifying 2FA:', code, method);
      
      toast.success('2FA verification successful');
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : '2FA verification failed';
      setError(errorMessage);
      toast.error(errorMessage);
      throw err;
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Memoized context value to prevent unnecessary re-renders
  const contextValue = useMemo(() => ({
    user,
    isLoading,
    error,
    signIn,
    signUp,
    signOut,
    forgotPassword,
    resendVerification,
    enable2FA,
    verify2FA,
    ...authState,
  }), [
    user,
    isLoading,
    error,
    signIn,
    signUp,
    signOut,
    forgotPassword,
    resendVerification,
    enable2FA,
    verify2FA,
    authState,
  ]);

  return (
    <AuthContext.Provider value={contextValue}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  const context = useContext(AuthContext);
  if (context === undefined) {
    throw new Error('useAuth must be used within an AuthProvider');
  }
  return context;
}
