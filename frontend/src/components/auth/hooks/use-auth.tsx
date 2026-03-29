"use client";

import { useState, useEffect, createContext, useContext, ReactNode } from "react";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { authClient } from '../lib/auth-client-enterprise';

export interface User {
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
}

export const AuthContext = createContext<AuthContextType | undefined>(undefined);

interface AuthProviderProps {
  children: ReactNode;
  initialUser?: User | null;
}

export function AuthProvider({ children, initialUser }: AuthProviderProps) {
  const hasInitialUser = initialUser !== undefined;
  const [user, setUser] = useState<User | null>(initialUser ?? null);
  const [isLoading, setIsLoading] = useState(!hasInitialUser);
  const [error, setError] = useState<string | null>(null);
  const router = useRouter();

  const checkSession = async (showLoading = true) => {
    try {
      if (showLoading) {
        setIsLoading(true);
      }

      // Use getSession directly instead of isAuthenticated to avoid 404 error
      const session = await authClient.getSession();

      if (session && 'data' in session && session.data?.user) {
        // User is authenticated - mark so we can detect stale cookies later
        setUser(session.data.user as User);
        if (typeof window !== 'undefined') {
          localStorage.setItem('ba_was_authenticated', '1');
        }
      } else {
        setUser(null);
        // If we previously had an authenticated session but now have none,
        // there may be a stale browser cookie that the middleware would still
        // accept. Trigger a silent sign-out to clear it so /sign-in is reachable.
        if (typeof window !== 'undefined' && localStorage.getItem('ba_was_authenticated')) {
          localStorage.removeItem('ba_was_authenticated');
          authClient.signOut().catch(() => {});
        }
      }
    } catch (err) {
      console.error("Session check failed:", err);
      setUser(null);
    } finally {
      if (showLoading) {
        setIsLoading(false);
      }
    }
  };

  // Check for existing session on mount and handle navigation
  useEffect(() => {
    if (!hasInitialUser) {
      checkSession(true);
    }
    
    // Handle browser navigation events (back/forward)
    const handleNavigationChange = () => {
      // Small delay to allow for any state changes
      setTimeout(() => {
        checkSession(false);
      }, 100);
    };
    
    // Listen for navigation events
    window.addEventListener('popstate', handleNavigationChange);
    window.addEventListener('pageshow', handleNavigationChange);
    
    // Also check session when window gains focus (user comes back to tab)
    const handleVisibilityChange = () => {
      if (!document.hidden) {
        checkSession(false);
      }
    };
    
    document.addEventListener('visibilitychange', handleVisibilityChange);
    
    return () => {
      window.removeEventListener('popstate', handleNavigationChange);
      window.removeEventListener('pageshow', handleNavigationChange);
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    };
  }, [hasInitialUser]);

  const signIn = async (email: string, password: string, redirectTo: string = '/dashboard') => {
    try {
      setIsLoading(true);
      setError(null);
      
      // Use Better Auth client for sign in
      const result = await authClient.signIn.email({
        email,
        password,
      });

      if (result.data?.user) {
        setUser(result.data.user as User);
        if (typeof window !== 'undefined') {
          localStorage.setItem('ba_was_authenticated', '1');
        }
        toast.success("Welcome back! You have been signed in successfully.");
        
        // Wait for session to be fully established, then redirect using Next.js router
        setTimeout(() => {
          router.push(redirectTo);
          // Fallback to window.location if router fails
          setTimeout(() => {
            if (window.location.pathname !== redirectTo) {
              window.location.href = redirectTo;
            }
          }, 500);
        }, 1500);
      } else if (result.error) {
        // Handle specific error cases
        const errorCode = result.error.code;
        const errorMessage = result.error.message;
        
        console.error('Sign in error:', { code: errorCode, message: errorMessage, fullError: result.error });
        
        if (errorCode === 'EMAIL_NOT_VERIFIED') {
          const message = "Please verify your email address before signing in. Check your inbox for a verification link.";
          toast.error(message);
          throw new Error("Email not verified. Please check your inbox for a verification link.");
        } else if (errorCode === 'INVALID_CREDENTIALS' || errorMessage?.includes('Invalid email or password')) {
          const message = "Invalid email or password. Please check your credentials and try again.";
          toast.error(message);
          throw new Error(message);
        } else if (errorCode === 'USER_NOT_FOUND' || errorMessage?.includes('User not found')) {
          const message = "No account found with this email address. Please check your email or create a new account.";
          toast.error(message);
          throw new Error(message);
        } else if (errorCode === 'ACCOUNT_LOCKED' || errorMessage?.includes('Account locked')) {
          const message = "Your account has been temporarily locked due to too many failed attempts. Please try again later.";
          toast.error(message);
          throw new Error(message);
        } else {
          const message = errorMessage || "Sign in failed. Please try again.";
          toast.error(message);
          throw new Error(message);
        }
      }
    } catch (err: unknown) {
      let errorMessage = "An error occurred during sign in";
      
      // Try to extract error from fetch response if it's a network error
      if (err instanceof Error) {
        errorMessage = err.message;
      } else if (typeof err === 'object' && err !== null && 'message' in err) {
        errorMessage = String(err.message);
      }
      
      setError(errorMessage);
      
      // Don't show toast if we already showed one above
      if (!errorMessage.includes('verify') && !errorMessage.includes('Invalid') && !errorMessage.includes('No account') && !errorMessage.includes('locked')) {
        toast.error(errorMessage);
      }
      
      throw err;
    } finally {
      setIsLoading(false);
    }
  };

  const signUp = async (email: string, password: string, name: string, redirectTo: string = '/dashboard') => {
    try {
      setIsLoading(true);
      setError(null);
      
      // Use Better Auth client for sign up
      const result = await authClient.signUp.email({
        email,
        password,
        name,
      });

      if (result.data?.user) {
        setUser(result.data.user as User);
        toast.success("Account created successfully! Please check your email to verify your account.");
        
        // Don't redirect immediately if email verification is required
        if (result.data.user.emailVerified) {
          // Wait for session to be fully established, then redirect using Next.js router
          setTimeout(() => {
            router.push(redirectTo);
            // Fallback to window.location if router fails
            setTimeout(() => {
              if (window.location.pathname !== redirectTo) {
                window.location.href = redirectTo;
              }
            }, 500);
          }, 1500);
        } else {
          // Show verification message instead of redirecting
          toast.info("Please check your email and click the verification link to complete your registration.");
        }
      } else if (result.error) {
        const errorCode = result.error.code;
        const errorMessage = result.error.message;
        
        console.error('Sign up error:', { code: errorCode, message: errorMessage, fullError: result.error });
        
        if (errorCode === 'USER_ALREADY_EXISTS' || errorMessage?.includes('User already exists') || errorMessage?.includes('already exists')) {
          const message = "An account with this email address already exists. Please sign in instead or use a different email.";
          toast.error(message);
          throw new Error(message);
        } else if (errorCode === 'WEAK_PASSWORD' || errorMessage?.includes('Password')) {
          const message = "Password is too weak. Please use a stronger password with at least 8 characters.";
          toast.error(message);
          throw new Error(message);
        } else if (errorCode === 'INVALID_EMAIL' || errorMessage?.includes('Invalid email')) {
          const message = "Please enter a valid email address.";
          toast.error(message);
          throw new Error(message);
        } else {
          const message = errorMessage || "Failed to create account. Please try again.";
          toast.error(message);
          throw new Error(message);
        }
      }
    } catch (err: unknown) {
      let errorMessage = "An error occurred during sign up";
      
      if (err instanceof Error) {
        errorMessage = err.message;
      } else if (typeof err === 'object' && err !== null && 'message' in err) {
        errorMessage = String(err.message);
      }
      
      setError(errorMessage);
      
      // Don't show toast if we already showed one above
      if (!errorMessage.includes('already exists') && !errorMessage.includes('Password') && !errorMessage.includes('valid email')) {
        toast.error(errorMessage);
      }
      
      throw err;
    } finally {
      setIsLoading(false);
    }
  };

  const signOut = async () => {
    const redirectAfterSignOut = () => {
      // Hard redirect ensures a fresh HTTP request so the middleware
      // sees the cleared cookie state rather than using cached client routing.
      if (typeof window !== 'undefined') {
        window.location.href = '/sign-in';
      }
    };

    try {
      setIsLoading(true);
      
      // Use standard Better Auth sign out (POST /api/auth/sign-out)
      const result = await authClient.signOut();
      const maybeError = (result as any)?.error;
      if (maybeError) {
        const status = maybeError.status ?? maybeError.statusCode;
        const message = String(maybeError.message || '').toLowerCase();
        const isAlreadySignedOut =
          status === 400 ||
          status === 401 ||
          message.includes('bad request') ||
          message.includes('unauthorized') ||
          message.includes('session');

        if (!isAlreadySignedOut) {
          throw new Error(maybeError.message || 'Sign out failed');
        }
      }

      setUser(null);
      if (typeof window !== 'undefined') {
        localStorage.removeItem('ba_was_authenticated');
      }
      toast.success("You have been signed out successfully.");

      // Clear any remaining state
      setError(null);
      
      // Redirect away from protected dashboard immediately
      redirectAfterSignOut();
    } catch (err: any) {
      const message = String(err?.message || '').toLowerCase();
      const isAlreadySignedOut =
        message.includes('400') ||
        message.includes('bad request') ||
        message.includes('401') ||
        message.includes('unauthorized') ||
        message.includes('session');

      if (isAlreadySignedOut) {
        setUser(null);
        setError(null);
        redirectAfterSignOut();
        return;
      }

      console.error("Sign out error:", err);
      toast.error("An error occurred during sign out.");
    } finally {
      setIsLoading(false);
    }
  };

  const forgotPassword = async (email: string) => {
    try {
      setIsLoading(true);
      setError(null);
      
      // Use Better Auth client for password reset
      const result = await authClient.forgetPassword({
        email,
        redirectTo: '/reset-password'
      });

      if (result.error) {
        const errorCode = result.error.code;
        const errorMessage = result.error.message;
        
        console.error('Password reset error:', { code: errorCode, message: errorMessage, fullError: result.error });
        
        if (errorCode === 'USER_NOT_FOUND' || errorMessage?.includes('User not found')) {
          const message = "No account found with this email address. Please check your email or create a new account.";
          toast.error(message);
          throw new Error(message);
        } else if (errorCode === 'TOO_MANY_REQUESTS' || errorMessage?.includes('Too many requests')) {
          const message = "Too many password reset attempts. Please wait before trying again.";
          toast.error(message);
          throw new Error(message);
        } else if (errorCode === 'INVALID_EMAIL' || errorMessage?.includes('Invalid email')) {
          const message = "Please enter a valid email address.";
          toast.error(message);
          throw new Error(message);
        } else {
          const message = errorMessage || "Failed to send reset email. Please try again.";
          toast.error(message);
          throw new Error(message);
        }
      }

      toast.success("Password reset email sent! Please check your inbox and follow the instructions.");
    } catch (err: unknown) {
      let errorMessage = "An error occurred while sending the password reset email";
      
      if (err instanceof Error) {
        errorMessage = err.message;
      } else if (typeof err === 'object' && err !== null && 'message' in err) {
        errorMessage = String(err.message);
      }
      
      setError(errorMessage);
      
      // Don't show toast if we already showed one above
      if (!errorMessage.includes('No account') && !errorMessage.includes('Too many') && !errorMessage.includes('valid email')) {
        toast.error(errorMessage);
      }
      
      throw err;
    } finally {
      setIsLoading(false);
    }
  };

  const resendVerification = async (email: string) => {
    try {
      setIsLoading(true);
      setError(null);
      
      // Use Better Auth client to resend verification email
      const result = await authClient.sendVerificationEmail({
        email,
      });

      if (result.error) {
        const errorCode = result.error.code;
        const errorMessage = result.error.message;
        
        console.error('Resend verification error:', { code: errorCode, message: errorMessage, fullError: result.error });
        
        if (errorCode === 'USER_NOT_FOUND' || errorMessage?.includes('User not found')) {
          const message = "No account found with this email address. Please create an account first.";
          toast.error(message);
          throw new Error(message);
        } else if (errorCode === 'EMAIL_ALREADY_VERIFIED' || errorMessage?.includes('already verified')) {
          const message = "Your email is already verified. You can sign in now.";
          toast.info(message);
          throw new Error(message);
        } else if (errorCode === 'TOO_MANY_REQUESTS' || errorMessage?.includes('Too many requests')) {
          const message = "Too many verification email requests. Please wait before trying again.";
          toast.error(message);
          throw new Error(message);
        } else {
          const message = errorMessage || "Failed to send verification email. Please try again.";
          toast.error(message);
          throw new Error(message);
        }
      }

      toast.success("Verification email sent! Please check your inbox and click the verification link.");
    } catch (err: unknown) {
      let errorMessage = "An error occurred while sending the verification email";
      
      if (err instanceof Error) {
        errorMessage = err.message;
      } else if (typeof err === 'object' && err !== null && 'message' in err) {
        errorMessage = String(err.message);
      }
      
      setError(errorMessage);
      
      // Don't show toast if we already showed one above
      if (!errorMessage.includes('No account') && !errorMessage.includes('already verified') && !errorMessage.includes('Too many')) {
        toast.error(errorMessage);
      }
      
      throw err;
    } finally {
      setIsLoading(false);
    }
  };

  const value: AuthContextType = {
    user,
    isLoading,
    error,
    signIn,
    signUp,
    signOut,
    forgotPassword,
    resendVerification,
  };

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth() {
  const context = useContext(AuthContext);
  if (context === undefined) {
    throw new Error("useAuth must be used within an AuthProvider");
  }
  return context;
}
