'use client';

import React, { useEffect, useReducer, useCallback } from 'react';
import { useRouter } from 'next/navigation';
import { useAuth } from '../hooks/use-auth';
import { useI18n } from '../hooks/use-i18n';
import { useAuthTranslation } from '../lib/i18n/hooks';
// NOTE: Legacy consent hook deprecated. Using enterprise consent system export.
import { useConsent } from '../consent/useConsent';
import { LoaderOne } from '../ui/loader';
import { Shield, AlertTriangle, CheckCircle2, User, Lock } from 'lucide-react';

// Enhanced types for enterprise authentication with ORPC integration
interface AuthUser {
  id: string;
  email: string;
  emailVerified?: boolean;
  twoFactorEnabled?: boolean;
  role?: string;
  permissions?: string[];
  organizations?: Array<{ 
    id: string; 
    slug: string; 
    role: string;
    permissions?: string[];
  }>;
  createdAt?: Date;
  lastActiveAt?: Date;
  [key: string]: unknown;
}

// Enhanced analytics tracking for comprehensive user behavior monitoring
interface AuthGuardAnalytics {
  event: 'guard_check' | 'access_granted' | 'access_denied' | 'auth_required' | 'validation_failed';
  guardType: 'auth' | 'role' | 'permission' | 'organization';
  userId?: string;
  organizationId?: string;
  timestamp: number;
  metadata: {
    path: string;
    userAgent: string;
    requirements: Record<string, unknown>;
    result: 'success' | 'failure' | 'pending';
    reason?: string;
  };
}

// Auth state type
type AuthState = 'loading' | 'authenticated' | 'unauthenticated' | 'checking' | 'error';

interface GuardReason { key: string; params?: Record<string, string | number>; }

interface AuthGuardProps {
  children: React.ReactNode;
  /** Redirect path for unauthenticated users */
  loginPath?: string;
  /** Redirect path after successful authentication */
  redirectTo?: string;
  /** Allow access for specific authentication states */
  allowedStates?: Array<'authenticated' | 'unauthenticated' | 'loading'>;
  /** Require email verification */
  requireEmailVerification?: boolean;
  /** Require 2FA setup */
  requireTwoFactor?: boolean;
  /** Custom loading component */
  loadingComponent?: React.ReactNode;
  /** Custom unauthorized component */
  unauthorizedComponent?: React.ReactNode;
  /** Show loading indicator while checking auth */
  showLoadingIndicator?: boolean;
  /** Minimum loading time in ms (to prevent flash) */
  minLoadingTime?: number;
  /** Custom error boundary */
  fallback?: React.ComponentType<{ error: Error; reset: () => void }>;
  /** Additional validation function */
  customValidation?: (user: AuthUser) => boolean | Promise<boolean>;
  /** Role-based access (basic implementation) */
  requiredRole?: string;
  /** Permission-based access */
  requiredPermissions?: string[];
  /** Organization access */
  requiredOrganization?: string;
  /** Callback when access is denied */
  onAccessDenied?: (reason: string) => void;
  /** Callback when authentication is required */
  onAuthRequired?: () => void;
}

interface AuthGuardState {
  authState: AuthState;
  validationError: GuardReason | null;
  hasMinLoadingPassed: boolean;
  customValidationResult: boolean | null;
}

type AuthGuardAction =
  | { type: 'SET_AUTH_STATE'; payload: AuthState }
  | { type: 'SET_VALIDATION_ERROR'; payload: GuardReason | null }
  | { type: 'MIN_LOADING_PASSED' }
  | { type: 'SET_CUSTOM_VALIDATION_RESULT'; payload: boolean | null }
  | { type: 'RESET' };

function authGuardReducer(state: AuthGuardState, action: AuthGuardAction): AuthGuardState {
  switch (action.type) {
    case 'SET_AUTH_STATE':
      return { ...state, authState: action.payload };
    case 'SET_VALIDATION_ERROR':
      return { ...state, validationError: action.payload };
    case 'MIN_LOADING_PASSED':
      return { ...state, hasMinLoadingPassed: true };
    case 'SET_CUSTOM_VALIDATION_RESULT':
      return { ...state, customValidationResult: action.payload };
    case 'RESET':
      return { ...state, authState: 'loading', validationError: null };
  }
}

const EMPTY_PERMISSIONS: string[] = [];

const LoadingSpinner = ({ size = 'md' }: { size?: 'sm' | 'md' | 'lg' }) => {
  const scaleClass = {
    sm: 'scale-50',
    md: 'scale-75', 
    lg: 'scale-100'
  }[size];
  
  return <div className={scaleClass}><LoaderOne /></div>;
};

function AuthLoadingView({ locale, authT }: { locale: string; authT: ReturnType<typeof useAuthTranslation>['authT'] }) {
  return (
    <div 
      className="flex items-center justify-center min-h-screen bg-background"
      role="status"
      aria-live="polite"
      aria-label={locale === 'no' ? 'Sjekker autentisering...' : 'Checking authentication...'}
    >
      <div className="text-center space-y-4">
        <LoadingSpinner size="lg" />
        <div className="space-y-2">
          <h3 className="text-lg font-medium text-foreground">
            {locale === 'no' ? authT.guard('checkingAuthTitle') : authT.guard('checkingAuthTitle')}
          </h3>
          <p className="text-sm text-muted-foreground">
            {authT.guard('checkingAuthMessage')}
          </p>
        </div>
      </div>
    </div>
  );
}

function AuthErrorView({ authT, validationError, onReset }: { authT: ReturnType<typeof useAuthTranslation>['authT']; validationError: GuardReason | null; onReset: () => void }) {
  // Icon mapping based on reason key (semantic, not localized substrings)
  const reasonKey = validationError?.key;
  const icon = reasonKey === 'emailMustBeVerified' ? User :
         reasonKey === 'twoFactorRequired' ? Lock :
         AlertTriangle;

  return (
    <div 
      className="flex items-center justify-center min-h-screen bg-background"
      role="alert"
      aria-live="assertive"
    >
      <div className="text-center space-y-4 max-w-md p-6">
        <div className="flex justify-center">
          {React.createElement(icon, { 
            className: "w-12 h-12 text-destructive",
            'aria-hidden': true 
          })}
        </div>
        <div className="space-y-2">
          <h3 className="text-lg font-medium text-foreground">
            {authT.guard('authErrorTitle')}
          </h3>
          <p className="text-sm text-muted-foreground">
            {validationError ? authT.guardFmt(validationError.key, validationError.params) : authT.guard('authErrorMessage')}
          </p>
        </div>
        <button
          onClick={onReset}
          className="px-4 py-2 bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors focus:outline-none focus:ring-2 focus:ring-ring"
        >
          {authT.guard('retry')}
        </button>
      </div>
    </div>
  );
}

function AuthUnauthorizedView({ authT, validationError, onGoToLogin, onGoBack }: { authT: ReturnType<typeof useAuthTranslation>['authT']; validationError: GuardReason | null; onGoToLogin: () => void; onGoBack: () => void }) {
  return (
    <div className="flex items-center justify-center min-h-screen bg-background">
      <div className="text-center space-y-4 max-w-md p-6">
        <div className="flex justify-center">
          <Lock className="w-12 h-12 text-muted-foreground" />
        </div>
        <div className="space-y-2">
          <h3 className="text-lg font-medium text-foreground">{authT.guard('accessDeniedTitle')}</h3>
          <p className="text-sm text-muted-foreground">
            {validationError ? authT.guardFmt(validationError.key, validationError.params) : authT.guard('accessDeniedMessage')}
          </p>
        </div>
        <div className="flex flex-col sm:flex-row gap-2 justify-center">
          <button
            onClick={onGoToLogin}
            className="px-4 py-2 bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors focus:outline-none focus:ring-2 focus:ring-ring"
          >
            {authT.guard('goToLogin')}
          </button>
          <button
            onClick={onGoBack}
            className="px-4 py-2 border border-border rounded-lg hover:bg-muted transition-colors focus:outline-none focus:ring-2 focus:ring-ring"
          >
            {authT.guard('goBack')}
          </button>
        </div>
      </div>
    </div>
  );
}

export function AuthGuard({
  children,
  loginPath = '/login',
  redirectTo,
  allowedStates = ['authenticated'],
  requireEmailVerification = false,
  requireTwoFactor = false,
  loadingComponent,
  unauthorizedComponent,
  showLoadingIndicator = true,
  minLoadingTime = 500,
  fallback: ErrorFallback,
  customValidation,
  requiredRole,
  requiredPermissions = EMPTY_PERMISSIONS,
  requiredOrganization,
  onAccessDenied,
  onAuthRequired,
}: AuthGuardProps) {
  const router = useRouter();
  const { user, isLoading, error } = useAuth();
  
  // Enhanced features - using i18n and consent systems
  const { locale } = useI18n();
  const { authT } = useAuthTranslation();
  const consentHook = useConsent();
  
  // Helper function to check consent
  const hasAnalyticsConsent = consentHook.consent?.performance || false;

  // State management
  const [{ authState, validationError, hasMinLoadingPassed, customValidationResult }, dispatch] = useReducer(authGuardReducer, {
    authState: 'loading',
    validationError: null,
    hasMinLoadingPassed: false,
    customValidationResult: null,
  });

  // Check if user is authenticated
  const isAuthenticated = Boolean(user && !error);

  // Enhanced analytics tracking function
  const trackAuthEvent = useCallback((event: AuthGuardAnalytics) => {
    if (!hasAnalyticsConsent) return;
    
    try {
      // Default analytics logging (can be replaced with your analytics service)
      console.log('🔒 Auth Guard Event:', event);
      
      // Here you would integrate with your analytics service (e.g., PostHog, Mixpanel)
      // analytics.track('auth_guard_event', event);
    } catch (error) {
      console.error('Failed to track auth event:', error);
    }
  }, [hasAnalyticsConsent]);

    // Enhanced validation checks with useCallback to fix dependency issues
  const performValidationChecks = useCallback(async (currentUser: AuthUser): Promise<{ isValid: boolean; reason?: GuardReason }> => {
    // Email verification check - Norwegian/English message
    if (requireEmailVerification && !currentUser.emailVerified) {
      return { isValid: false, reason: { key: 'emailMustBeVerified' } };
    }

    // 2FA check - Norwegian/English message  
    if (requireTwoFactor && !currentUser.twoFactorEnabled) {
  return { isValid: false, reason: { key: 'twoFactorRequired' } };
    }

    // Role check - Norwegian/English message
    if (requiredRole && currentUser.role !== requiredRole) {
  return { isValid: false, reason: { key: 'roleRequired', params: { role: requiredRole } } };
    }

    // Permissions check - Norwegian/English message
    if (requiredPermissions.length > 0) {
      const userPermissions = currentUser.permissions || [];
      const hasAllPermissions = requiredPermissions.every(permission =>
        userPermissions.includes(permission)
      );
      if (!hasAllPermissions) {
  return { isValid: false, reason: { key: 'permissionsMissing', params: { perms: requiredPermissions.join(', ') } } };
      }
    }

    // Organization access check - Norwegian/English message
    if (requiredOrganization) {
      const userOrganizations = currentUser.organizations || [];
      const hasOrgAccess = userOrganizations.some((org: { id: string; slug: string }) => 
        org.id === requiredOrganization || org.slug === requiredOrganization
      );
      if (!hasOrgAccess) {
    return { isValid: false, reason: { key: 'organizationAccessRequired', params: { org: requiredOrganization } } };
      }
    }

  return { isValid: true };
  }, [requireEmailVerification, requireTwoFactor, requiredRole, requiredPermissions, requiredOrganization]);

  // Minimum loading time timer
  useEffect(() => {
    const timer = setTimeout(() => {
      dispatch({ type: 'MIN_LOADING_PASSED' });
    }, minLoadingTime);

    return () => clearTimeout(timer);
  }, [minLoadingTime]);

  // Main authentication check effect
  useEffect(() => {
    const checkAuth = async () => {
      try {
        dispatch({ type: 'SET_AUTH_STATE', payload: 'checking' });
  dispatch({ type: 'SET_VALIDATION_ERROR', payload: null });
        
        // Track guard check event
        trackAuthEvent({
          event: 'guard_check',
          guardType: 'auth',
          userId: user?.id,
          timestamp: Date.now(),
          metadata: {
            path: typeof window !== 'undefined' ? window.location.pathname : '',
            userAgent: typeof navigator !== 'undefined' ? navigator.userAgent : '',
            requirements: {
              allowedStates,
              requireEmailVerification,
              requireTwoFactor,
              requiredRole,
              requiredPermissions,
              requiredOrganization,
            },
            result: 'pending',
          },
        });

        // Wait for auth to finish loading
        if (isLoading) {
          dispatch({ type: 'SET_AUTH_STATE', payload: 'loading' });
          return;
        }

        // Check basic authentication state
        if (error) {
          dispatch({ type: 'SET_AUTH_STATE', payload: 'error' });
          return;
        }

        if (!isAuthenticated || !user) {
          dispatch({ type: 'SET_AUTH_STATE', payload: 'unauthenticated' });
          if (onAuthRequired) {
            onAuthRequired();
          }
          return;
        }

        // Advanced validation checks
        const validationChecks = await performValidationChecks(user as unknown as AuthUser);
        if (!validationChecks.isValid && validationChecks.reason) {
          dispatch({ type: 'SET_VALIDATION_ERROR', payload: validationChecks.reason });
          dispatch({ type: 'SET_AUTH_STATE', payload: 'unauthenticated' });
          if (onAccessDenied) {
            onAccessDenied(validationChecks.reason.key);
          }
          return;
        }

        dispatch({ type: 'SET_AUTH_STATE', payload: 'authenticated' });
      } catch (err) {
        console.error('AuthGuard validation error:', err);
  dispatch({ type: 'SET_AUTH_STATE', payload: 'error' });
  dispatch({ type: 'SET_VALIDATION_ERROR', payload: { key: err instanceof Error ? err.message : 'authErrorMessage' } });
      }
    };

    checkAuth();
  }, [
    isLoading, 
    isAuthenticated, 
    user, 
    error, 
    requireEmailVerification, 
    requireTwoFactor,
    requiredRole,
    requiredPermissions,
    requiredOrganization,
    allowedStates,
    onAccessDenied,
    onAuthRequired,
    performValidationChecks,
    trackAuthEvent
  ]);

  // Custom validation effect
  useEffect(() => {
    if (!customValidation || !user || authState !== 'authenticated') {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      dispatch({ type: 'SET_CUSTOM_VALIDATION_RESULT', payload: null });
      return;
    }

    const runCustomValidation = async () => {
      try {
        const result = await customValidation(user as unknown as AuthUser);
        dispatch({ type: 'SET_CUSTOM_VALIDATION_RESULT', payload: result });
        if (!result && onAccessDenied) {
          onAccessDenied('customValidationFailed');
        }
      } catch (err) {
        console.error('Custom validation error:', err);
        dispatch({ type: 'SET_CUSTOM_VALIDATION_RESULT', payload: false });
        if (onAccessDenied) {
          onAccessDenied('customValidationError');
        }
      }
    };

    runCustomValidation();
  }, [user, authState, customValidation, onAccessDenied]);

  // Handle redirects
  useEffect(() => {
    if (!hasMinLoadingPassed) return;

    // Only redirect for unauthorized states, not error or checking states
    if (authState === 'unauthenticated' && !allowedStates.includes('unauthenticated')) {
      const currentPath = window.location.pathname;
      const loginUrl = redirectTo 
        ? `${loginPath}?redirectTo=${encodeURIComponent(redirectTo)}`
        : `${loginPath}?redirectTo=${encodeURIComponent(currentPath)}`;
      // eslint-disable-next-line react-doctor/nextjs-no-client-side-redirect
      router.push(loginUrl);
    }
  }, [authState, hasMinLoadingPassed, allowedStates, loginPath, redirectTo, router]);

  // Enhanced loading state with accessibility and Norwegian/English support
  if (!hasMinLoadingPassed || authState === 'loading' || authState === 'checking') {
    if (loadingComponent) {
      return <>{loadingComponent}</>;
    }

    if (!showLoadingIndicator) {
      return null;
    }

    return <AuthLoadingView locale={locale} authT={authT} />;
  }

  // Enhanced error state with accessibility and Norwegian/English support
  if (authState === 'error') {
    if (ErrorFallback) {
      return (
        <ErrorFallback 
          error={new Error(validationError?.key || (locale === 'no' ? 'Autentiseringsfeil' : 'Authentication error'))} 
          reset={() => {
            dispatch({ type: 'RESET' });
            window.location.reload();
          }} 
        />
      );
    }

    return (
      <AuthErrorView
        authT={authT}
        validationError={validationError}
        onReset={() => {
          dispatch({ type: 'RESET' });
          window.location.reload();
        }}
      />
    );
  }

  // Show unauthorized state - Norwegian text
  if (!allowedStates.includes(authState) || validationError || customValidationResult === false) {
    if (unauthorizedComponent) {
      return <>{unauthorizedComponent}</>;
    }

    return (
      <AuthUnauthorizedView
        authT={authT}
        validationError={validationError}
        onGoToLogin={() => router.push(loginPath)}
        onGoBack={() => router.back()}
      />
    );
  }

  // Show success state (authenticated and authorized)
  if (authState === 'authenticated' && allowedStates.includes('authenticated') && !validationError && (customValidationResult === null || customValidationResult === true)) {
    return <>{children}</>;
  }

  // Fallback - should not reach here
  return null;
}

// Higher-order component version for easier usage
function withAuthGuard<P extends object>(
  Component: React.ComponentType<P>,
  guardProps?: Omit<AuthGuardProps, 'children'>
) {
  const AuthGuardedComponent = (props: P) => {
    return (
      <AuthGuard {...guardProps}>
        <Component {...props} />
      </AuthGuard>
    );
  };

  AuthGuardedComponent.displayName = `withAuthGuard(${Component.displayName || Component.name})`;
  
  return AuthGuardedComponent;
}

// Quick preset guards with Norwegian names
const KreverAuth = ({ children, ...props }: Omit<AuthGuardProps, 'allowedStates'>) => (
  <AuthGuard allowedStates={['authenticated']} {...props}>
    {children}
  </AuthGuard>
);

const KreverGjest = ({ children, ...props }: Omit<AuthGuardProps, 'allowedStates'>) => (
  <AuthGuard allowedStates={['unauthenticated']} {...props}>
    {children}
  </AuthGuard>
);

const KreverEpostBekreftelse = ({ children, ...props }: Omit<AuthGuardProps, 'requireEmailVerification'>) => (
  <AuthGuard requireEmailVerification={true} {...props}>
    {children}
  </AuthGuard>
);

const KreverTofaktor = ({ children, ...props }: Omit<AuthGuardProps, 'requireTwoFactor'>) => (
  <AuthGuard requireTwoFactor={true} {...props}>
    {children}
  </AuthGuard>
);

// Enhanced status component for debugging with Norwegian/English support
function AuthGuardStatus() {
  const { user, isLoading, error } = useAuth();
  const { authT } = useAuthTranslation();
  const isAuthenticated = Boolean(user && !error);
  
  return (
    <div className="fixed bottom-4 right-4 bg-background border border-border rounded-lg p-3 shadow-lg text-xs max-w-xs z-50">
      <div className="flex items-center gap-2 mb-2">
        <Shield className="w-4 h-4" />
        <span className="font-medium">{authT.guard('authStatus')}</span>
      </div>
      <div className="space-y-1">
        <div className="flex items-center gap-2">
          <span>{authT.guard('loading')}</span>
          {isLoading ? (
            <LoadingSpinner size="sm" />
          ) : (
            <CheckCircle2 className="w-3 h-3 text-green-500" />
          )}
        </div>
        <div>{authT.guard('authenticated')} {isAuthenticated ? '✅' : '❌'}</div>
        <div>{authT.guard('user')} {user?.email || '—'}</div>
        {user && (
          <>
            <div>{authT.guard('emailVerified')} {(user as unknown as AuthUser)?.emailVerified ? '✅' : '❌'}</div>
            <div>{authT.guard('twoFactor')} {(user as unknown as AuthUser)?.twoFactorEnabled ? '✅' : '❌'}</div>
            {(user as unknown as AuthUser)?.role && <div>{authT.guard('role')} {(user as unknown as AuthUser).role}</div>}
          </>
        )}
        {error && <div className="text-destructive">{authT.guard('errorDetailsLabel')} {String(error)}</div>}
      </div>
    </div>
  );
}
