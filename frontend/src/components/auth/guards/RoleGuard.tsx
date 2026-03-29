'use client';

import React, { useEffect, useState, useCallback } from 'react';
import { useAuth } from '../hooks/use-auth';
import { useI18n } from '../hooks/use-i18n';
import { useAuthTranslation } from '../lib/i18n/hooks';
// NOTE: Legacy consent hook deprecated. Using enterprise consent system export.
import { useConsent } from '../consent';
import { Shield, AlertTriangle, Users, Crown, UserX } from 'lucide-react';

// Enhanced AuthUser interface for enterprise features
interface AuthUser {
  id: string;
  email: string;
  role?: string;
  permissions?: string[];
  organizations?: Array<{
    id: string;
    slug: string;
    role: string;
    permissions?: string[];
  }>;
  status?: 'active' | 'inactive' | 'suspended';
  lastLoginAt?: string;
  emailVerified?: boolean;
  twoFactorEnabled?: boolean;
  [key: string]: unknown;
}

// Enhanced analytics tracking for role access
interface RoleAccessEvent {
  userId: string;
  action: 'role_check' | 'access_granted' | 'access_denied' | 'role_validation_error';
  userRole?: string;
  requiredRoles: string[];
  requiredPermissions: string[];
  organizationContext?: string;
  timestamp: number;
  metadata: {
    path: string;
    userAgent: string;
    validationMethod: string;
    customValidation?: boolean;
    timeRestricted?: boolean;
    result: 'success' | 'failure' | 'pending';
    reason?: string;
  };
}

// Role validation state type
type RoleValidationState = 'loading' | 'authorized' | 'unauthorized' | 'checking' | 'error';

interface RoleGuardProps {
  children: React.ReactNode;
  /** Required user roles */
  allowedRoles?: string[];
  /** Required permissions */
  allowedPermissions?: string[];
  /** Organization context */
  organizationId?: string;
  /** Organization role requirements */
  organizationRoles?: string[];
  /** Organization permission requirements */
  organizationPermissions?: string[];
  /** Role matching strategy */
  roleStrategy?: 'any' | 'all';
  /** Permission matching strategy */
  permissionStrategy?: 'any' | 'all';
  /** Custom role validation function */
  customRoleValidator?: (user: AuthUser) => boolean | Promise<boolean>;
  /** Fallback component for unauthorized access */
  fallback?: React.ReactNode;
  /** Loading component */
  loadingComponent?: React.ReactNode;
  /** Show detailed error messages */
  showErrorDetails?: boolean;
  /** Callback when access is denied */
  onAccessDenied?: (reason: string, user?: AuthUser) => void;
  /** Enable role inheritance (admin > manager > user) */
  enableRoleHierarchy?: boolean;
  /** Custom role hierarchy definition */
  roleHierarchy?: Record<string, string[]>;
  /** Require account to be active */
  requireActiveAccount?: boolean;
  /** Time-based access control */
  timeRestrictions?: {
    allowedHours?: [number, number]; // [start, end] in 24h format
    allowedDays?: number[]; // 0-6, Sunday = 0
    timezone?: string;
  };
  /** Enable analytics tracking */
  enableAnalytics?: boolean;
  /** Accessibility options */
  accessibilityOptions?: {
    announceStateChanges?: boolean;
    skipLinkTarget?: string;
    customAriaLabels?: Record<string, string>;
  };
}

// Default role hierarchy (higher roles inherit lower role permissions) - Norwegian roles
const DEFAULT_ROLE_HIERARCHY: Record<string, string[]> = {
  'super-admin': ['admin', 'leder', 'redaktør', 'medlem', 'bruker'],
  'admin': ['leder', 'redaktør', 'medlem', 'bruker'],
  'leder': ['redaktør', 'medlem', 'bruker'],
  'redaktør': ['medlem', 'bruker'],
  'medlem': ['bruker'],
  'bruker': []
};

export function RoleGuard({
  children,
  allowedRoles = [],
  allowedPermissions = [],
  organizationId,
  organizationRoles = [],
  organizationPermissions = [],
  roleStrategy = 'any',
  permissionStrategy = 'any',
  customRoleValidator,
  fallback,
  loadingComponent,
  showErrorDetails = true,
  onAccessDenied,
  enableRoleHierarchy = true,
  roleHierarchy = DEFAULT_ROLE_HIERARCHY,
  requireActiveAccount = true,
  timeRestrictions,
  enableAnalytics = true,
}: RoleGuardProps) {
  // Enhanced features - using i18n and consent systems
  const { locale } = useI18n();
  const consentHook = useConsent();
  const { authT } = useAuthTranslation();
  
  // Helper function to check consent
  const hasAnalyticsConsent = consentHook.consent?.performance || false;

  const { user, isLoading, error } = useAuth();
  const [validationState, setValidationState] = useState<RoleValidationState>('loading');
  const [errorMessage, setErrorMessage] = useState<string>('');

  // Check if user is authenticated
  const isAuthenticated = Boolean(user && !error);

  // Analytics tracking function with consent check
  const trackRoleAccessEvent = useCallback((event: Omit<RoleAccessEvent, 'timestamp' | 'metadata'> & { metadata?: Partial<RoleAccessEvent['metadata']> }) => {
    if (!enableAnalytics || !hasAnalyticsConsent || typeof window === 'undefined') return;

    try {
      const fullEvent: RoleAccessEvent = {
        ...event,
        timestamp: Date.now(),
        metadata: {
          path: window.location.pathname,
          userAgent: navigator.userAgent,
          validationMethod: 'RoleGuard',
          result: 'pending',
          ...event.metadata,
        }
      };

      // Send to analytics service (placeholder - implement your analytics integration)
      console.log('[RoleGuard Analytics]', fullEvent);
      
      // You can integrate with your analytics service here
      // Example: analytics.track('role_access_event', fullEvent);
    } catch (err) {
      console.warn('Failed to track role access event:', err);
    }
  }, [enableAnalytics, hasAnalyticsConsent]);

  // Role hierarchy helper - memoized for performance
  const hasRoleWithHierarchy = useCallback((userRole: string, requiredRoles: string[]): boolean => {
    if (!enableRoleHierarchy) {
      return requiredRoles.includes(userRole);
    }

    // Check if user role directly matches
    if (requiredRoles.includes(userRole)) {
      return true;
    }

    // Check if user role has hierarchy access to any required role
    const inheritedRoles = roleHierarchy[userRole] || [];
    return requiredRoles.some(role => inheritedRoles.includes(role));
  }, [enableRoleHierarchy, roleHierarchy]);

  // Permission matching helpers - memoized for performance
  const checkPermissions = useCallback((userPermissions: string[], requiredPermissions: string[], strategy: 'any' | 'all'): boolean => {
    if (requiredPermissions.length === 0) return true;
    
    if (strategy === 'all') {
      return requiredPermissions.every(permission => userPermissions.includes(permission));
    } else {
      return requiredPermissions.some(permission => userPermissions.includes(permission));
    }
  }, []);

  // Time-based access control - memoized for performance
  const checkTimeRestrictions = useCallback((): boolean => {
    if (!timeRestrictions) return true;

    const now = timeRestrictions.timezone 
      ? new Date(new Date().toLocaleString("en-US", { timeZone: timeRestrictions.timezone }))
      : new Date();

    // Check allowed hours
    if (timeRestrictions.allowedHours) {
      const [startHour, endHour] = timeRestrictions.allowedHours;
      const currentHour = now.getHours();
      
      if (startHour <= endHour) {
        // Same day range (e.g., 9-17)
        if (currentHour < startHour || currentHour > endHour) {
          return false;
        }
      } else {
        // Overnight range (e.g., 22-6)
        if (currentHour < startHour && currentHour > endHour) {
          return false;
        }
      }
    }

    // Check allowed days
    if (timeRestrictions.allowedDays) {
      const currentDay = now.getDay();
      if (!timeRestrictions.allowedDays.includes(currentDay)) {
        return false;
      }
    }

    return true;
  }, [timeRestrictions]);

  // Main validation logic
  useEffect(() => {
    const validateAccess = async () => {
      try {
        setValidationState('loading');
        setErrorMessage('');

        // Wait for auth to load
        if (isLoading) {
          return;
        }

        // Track role check event
        trackRoleAccessEvent({
          userId: user?.id || 'anonymous',
          action: 'role_check',
          userRole: (user as unknown as AuthUser)?.role,
          requiredRoles: allowedRoles,
          requiredPermissions: allowedPermissions,
          organizationContext: organizationId,
          metadata: {
            customValidation: !!customRoleValidator,
            timeRestricted: !!timeRestrictions,
          }
        });

        // Check authentication - Norwegian messages
        if (!isAuthenticated || !user) {
          setErrorMessage('authRequired');
          setValidationState('unauthorized');
          if (onAccessDenied) {
            onAccessDenied('Autentisering kreves');
          }
          return;
        }

        // Check account status - Norwegian message
        if (requireActiveAccount && (user as unknown as { status?: string }).status === 'inactive') {
          setErrorMessage('inactiveAccount');
          setValidationState('unauthorized');
          if (onAccessDenied) {
            onAccessDenied('Kontoen er inaktiv', user as unknown as AuthUser);
          }
          return;
        }

        // Check time restrictions - Norwegian message
        if (!checkTimeRestrictions()) {
          setErrorMessage('timeRestricted');
          setValidationState('unauthorized');
          if (onAccessDenied) {
            onAccessDenied('Tilgang ikke tillatt på dette tidspunktet', user as unknown as AuthUser);
          }
          return;
        }

        // Organization-specific validation - Norwegian messages
        if (organizationId) {
          const userOrg = (user as unknown as AuthUser).organizations?.find(
            (org: { id: string; slug: string; role: string; permissions?: string[] }) => org.id === organizationId || org.slug === organizationId
          );

          if (!userOrg) {
            setErrorMessage('organizationAccessRequired');
            setValidationState('unauthorized');
            if (onAccessDenied) {
              onAccessDenied(`Tilgang til organisasjon "${organizationId}" kreves`, user as unknown as AuthUser);
            }
            return;
          }

          // Check organization roles - Norwegian messages
          if (organizationRoles.length > 0) {
            const hasOrgRole = roleStrategy === 'all'
              ? organizationRoles.every(role => hasRoleWithHierarchy(userOrg.role, [role]))
              : organizationRoles.some(role => hasRoleWithHierarchy(userOrg.role, [role]));

            if (!hasOrgRole) {
              setErrorMessage('organizationRoleRequired');
              setValidationState('unauthorized');
              if (onAccessDenied) {
                onAccessDenied(`Nødvendig organisasjonsrolle: ${organizationRoles.join(roleStrategy === 'all' ? ', ' : ' eller ')}`, user as unknown as AuthUser);
              }
              return;
            }
          }

          // Check organization permissions - Norwegian messages
          if (organizationPermissions.length > 0) {
            const userOrgPermissions = userOrg.permissions || [];
            const hasOrgPermissions = checkPermissions(userOrgPermissions, organizationPermissions, permissionStrategy);

            if (!hasOrgPermissions) {
              setErrorMessage('organizationPermissionsRequired');
              setValidationState('unauthorized');
              if (onAccessDenied) {
                onAccessDenied(`Nødvendige organisasjonstillatelser: ${organizationPermissions.join(permissionStrategy === 'all' ? ', ' : ' eller ')}`, user as unknown as AuthUser);
              }
              return;
            }
          }
        }

        // Global role validation - Norwegian messages
        if (allowedRoles.length > 0) {
          const userRole = (user as unknown as AuthUser).role || 'bruker';
          const hasRole = roleStrategy === 'all'
            ? allowedRoles.every(role => hasRoleWithHierarchy(userRole, [role]))
            : allowedRoles.some(role => hasRoleWithHierarchy(userRole, [role]));

          if (!hasRole) {
            setErrorMessage('roleRequiredGlobal');
            setValidationState('unauthorized');
            if (onAccessDenied) {
              onAccessDenied(`Nødvendig rolle: ${allowedRoles.join(roleStrategy === 'all' ? ', ' : ' eller ')}`, user as unknown as AuthUser);
            }
            return;
          }
        }

        // Global permission validation - Norwegian messages
        if (allowedPermissions.length > 0) {
          const userPermissions = (user as unknown as AuthUser).permissions || [];
          const hasPermissions = checkPermissions(userPermissions, allowedPermissions, permissionStrategy);

          if (!hasPermissions) {
            setErrorMessage('permissionsRequiredGlobal');
            setValidationState('unauthorized');
            if (onAccessDenied) {
              onAccessDenied(`Nødvendige tillatelser: ${allowedPermissions.join(permissionStrategy === 'all' ? ', ' : ' eller ')}`, user as unknown as AuthUser);
            }
            return;
          }
        }

        // Custom validation - Norwegian message
        if (customRoleValidator) {
          try {
            const customResult = await customRoleValidator(user as unknown as AuthUser);
            if (!customResult) {
              setErrorMessage('customValidationFailed');
              setValidationState('unauthorized');
              if (onAccessDenied) {
                onAccessDenied('Tilpasset rollevalidering feilet', user as unknown as AuthUser);
              }
              return;
            }
          } catch (err) {
            console.error('Custom role validation error:', err);
            setErrorMessage('customValidationError');
            setValidationState('unauthorized');
            if (onAccessDenied) {
              onAccessDenied('Feil i tilpasset rollevalidering', user as unknown as AuthUser);
            }
            return;
          }
        }

        // All checks passed
        trackRoleAccessEvent({
          userId: user?.id || 'unknown',
          action: 'access_granted',
          userRole: (user as unknown as AuthUser)?.role,
          requiredRoles: allowedRoles,
          requiredPermissions: allowedPermissions,
          organizationContext: organizationId,
          metadata: {
            result: 'success',
            customValidation: !!customRoleValidator,
            timeRestricted: !!timeRestrictions,
          }
        });
        setValidationState('authorized');
      } catch (err) {
        console.error('RoleGuard validation error:', err);
        trackRoleAccessEvent({
          userId: user?.id || 'unknown',
          action: 'role_validation_error',
          userRole: (user as unknown as AuthUser)?.role,
          requiredRoles: allowedRoles,
          requiredPermissions: allowedPermissions,
          organizationContext: organizationId,
          metadata: {
            result: 'failure',
            reason: 'validation_error',
            customValidation: !!customRoleValidator,
            timeRestricted: !!timeRestrictions,
          }
        });
  setErrorMessage('roleValidationError');
        setValidationState('error');
        if (onAccessDenied) {
          onAccessDenied('Feil i rollevalidering', user ? user as unknown as AuthUser : undefined);
        }
      }
    };

    validateAccess();
  }, [
    user,
    isLoading,
    isAuthenticated,
    allowedRoles,
    allowedPermissions,
    organizationId,
    organizationRoles,
    organizationPermissions,
    roleStrategy,
    permissionStrategy,
    customRoleValidator,
    enableRoleHierarchy,
    requireActiveAccount,
    timeRestrictions,
    onAccessDenied,
    hasRoleWithHierarchy,
    checkPermissions,
    checkTimeRestrictions,
    trackRoleAccessEvent
  ]);

  // Show loading state with enhanced Norwegian/English i18n
  if (validationState === 'loading' || validationState === 'checking') {
    if (loadingComponent) {
      return <>{loadingComponent}</>;
    }

    // Enhanced loading with minimum time following Doherty Threshold
    // Note: minLoadingTime is set at component mount for performance tracking
    
    // Determine loading message with locale support
    const loadingMessage = locale === 'en' 
      ? 'Checking permissions...' 
      : 'Sjekker tillatelser...';

    return (
      <div 
        className="flex items-center justify-center p-8"
        role="status"
        aria-live="polite"
        aria-label={loadingMessage}
      >
        <div className="text-center space-y-3">
          <div className="relative">
            <Shield className="w-8 h-8 animate-pulse mx-auto text-primary/70" />
            <div className="absolute inset-0 rounded-full border-2 border-primary/20 animate-ping" />
          </div>
          <div className="space-y-1">
            <p className="text-sm font-medium text-foreground">
              {loadingMessage}
            </p>
            {allowedRoles.length > 0 && (
              <p className="text-xs text-muted-foreground">
                {locale === 'en' 
                  ? `Required roles: ${allowedRoles.join(', ')}`
                  : `Nødvendige roller: ${allowedRoles.join(', ')}`
                }
              </p>
            )}
          </div>
        </div>
      </div>
    );
  }

  // Show unauthorized state with enhanced Norwegian/English i18n
  if (validationState === 'unauthorized' || validationState === 'error') {
    if (fallback) {
      return <>{fallback}</>;
    }

    // Localized content
  const title = validationState === 'error' ? authT.guard('validationErrorTitle') : authT.guard('accessDeniedTitle');
  const description = validationState === 'error' ? authT.guard('errorCheckingPermissions') : authT.guard('accessDeniedGeneric');

    return (
      <div 
        className="flex items-center justify-center p-8"
        role="alert"
        aria-live="assertive"
      >
        <div className="text-center space-y-4 max-w-md">
          <div className="flex justify-center">
            {validationState === 'error' ? (
              <AlertTriangle 
                className="w-12 h-12 text-destructive" 
                aria-hidden="true"
              />
            ) : (
              <div className="relative">
                <Users 
                  className="w-12 h-12 text-muted-foreground" 
                  aria-hidden="true"
                />
                <UserX className="w-6 h-6 text-destructive absolute -bottom-1 -right-1" />
              </div>
            )}
          </div>
          <div className="space-y-2">
            <h3 className="text-lg font-medium text-foreground">
              {title}
            </h3>
            <p className="text-sm text-muted-foreground">
              {description}
            </p>
      {showErrorDetails && errorMessage && (
              <div 
                className="text-xs text-destructive bg-destructive/10 rounded-lg p-2 border border-destructive/20"
        role="alert"
        aria-label={authT.guard('errorDetailsLabel')}
              >
        {authT.guard(errorMessage)}
              </div>
            )}
          </div>
      {(user as unknown as AuthUser)?.role && (
            <div className="text-xs text-muted-foreground bg-muted rounded-lg p-2">
        <span>{authT.guard('currentRole')}: </span>
              <span className="font-medium">{(user as unknown as AuthUser).role}</span>
              {organizationId && (
                <div className="mt-1">
          <span>{authT.guard('organization')}: </span>
                  <span className="font-medium">{organizationId}</span>
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    );
  }

  // Show authorized content
  if (validationState === 'authorized') {
    return <>{children}</>;
  }

  // Fallback
  return null;
}

// Higher-order component version
export function withRoleGuard<P extends object>(
  Component: React.ComponentType<P>,
  guardProps: Omit<RoleGuardProps, 'children'>
) {
  const RoleGuardedComponent = (props: P) => {
    return (
      <RoleGuard {...guardProps}>
        <Component {...props} />
      </RoleGuard>
    );
  };

  RoleGuardedComponent.displayName = `withRoleGuard(${Component.displayName || Component.name})`;
  
  return RoleGuardedComponent;
}

// Quick preset guards with Norwegian names
export const KreverRolle = ({ role, children, ...props }: { role: string; children: React.ReactNode } & Omit<RoleGuardProps, 'allowedRoles' | 'children'>) => (
  <RoleGuard allowedRoles={[role]} {...props}>
    {children}
  </RoleGuard>
);

export const KreverTillatelse = ({ permission, children, ...props }: { permission: string; children: React.ReactNode } & Omit<RoleGuardProps, 'allowedPermissions' | 'children'>) => (
  <RoleGuard allowedPermissions={[permission]} {...props}>
    {children}
  </RoleGuard>
);

export const KreverAdmin = ({ children, ...props }: Omit<RoleGuardProps, 'allowedRoles'>) => (
  <RoleGuard allowedRoles={['admin']} {...props}>
    {children}
  </RoleGuard>
);

export const KreverLeder = ({ children, ...props }: Omit<RoleGuardProps, 'allowedRoles'>) => (
  <RoleGuard allowedRoles={['admin', 'leder']} {...props}>
    {children}
  </RoleGuard>
);

export const KreverOrganisasjonsrolle = ({ 
  organizationId, 
  role, 
  children, 
  ...props 
}: { 
  organizationId: string; 
  role: string; 
  children: React.ReactNode; 
} & Omit<RoleGuardProps, 'organizationId' | 'organizationRoles' | 'children'>) => (
  <RoleGuard 
    organizationId={organizationId} 
    organizationRoles={[role]} 
    {...props}
  >
    {children}
  </RoleGuard>
);

// Role status component for debugging - Norwegian
export function RoleGuardStatus() {
  const { user } = useAuth();
  const { authT } = useAuthTranslation();
  
  if (!user) return null;

  return (
  <div className="fixed bottom-4 left-4 bg-background border border-border rounded-lg p-3 shadow-lg text-xs max-w-xs">
      <div className="flex items-center gap-2 mb-2">
        <Crown className="w-4 h-4" />
    <span className="font-medium">{authT.guard('roleStatus')}</span>
      </div>
      <div className="space-y-1">
    <div>{authT.guard('role')} <span className="font-medium">{(user as unknown as AuthUser).role || '—'}</span></div>
    <div>{authT.guard('permissionsLabel')}: {((user as unknown as AuthUser).permissions || []).length}</div>
    <div>{authT.guard('organizationsLabel')}: {((user as unknown as AuthUser).organizations || []).length}</div>
        {(user as unknown as AuthUser).organizations && (user as unknown as AuthUser).organizations!.length > 0 && (
          <div className="mt-2 pt-2 border-t border-border">
      <div className="text-xs font-medium mb-1">{authT.guard('organizationsHeader')}</div>
            {(user as unknown as AuthUser).organizations!.map((org, index) => (
              <div key={index} className="text-xs">
                {org.slug}: <span className="font-medium">{org.role}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}