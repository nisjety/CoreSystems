import React, { useState, useCallback, useMemo } from 'react';
import { 
  Eye, 
  EyeOff, 
  Loader2, 
  CheckCircle, 
  AlertCircle, 
  Building,
  Users,
  Mail,
  Lock,
  User
} from 'lucide-react';

import type { AuthMode, AuthFormData, AuthFormErrors } from '../types/auth';
import { useAuthTranslation, useCommonTranslation } from '../lib/i18n/hooks';
import { useIsHydrated } from '../lib/hydration/HydrationGuard';

// Enhanced toast notification system with better UX
interface ToastNotification {
  id: string;
  type: 'success' | 'error' | 'info' | 'warning';
  title: string;
  description?: string;
  duration?: number;
}

class ToastManager {
  private static instance: ToastManager;
  private listeners: Set<(toast: ToastNotification) => void> = new Set();

  static getInstance(): ToastManager {
    if (!ToastManager.instance) {
      ToastManager.instance = new ToastManager();
    }
    return ToastManager.instance;
  }

  show(toast: Omit<ToastNotification, 'id'>): void {
    const notification: ToastNotification = {
      ...toast,
      id: `toast-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
      duration: toast.duration || 5000,
    };

    this.listeners.forEach(listener => listener(notification));
    
    // Fallback console logging with better formatting
    const message = `${toast.title}${toast.description ? `: ${toast.description}` : ''}`;
    switch (toast.type) {
      case 'success':
        console.log(`✅ Auth Success: ${message}`);
        break;
      case 'error':
        console.error(`❌ Auth Error: ${message}`);
        break;
      case 'warning':
        console.warn(`⚠️ Auth Warning: ${message}`);
        break;
      default:
        console.info(`ℹ️ Auth Info: ${message}`);
    }
  }

  subscribe(listener: (toast: ToastNotification) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }
}

const toastManager = ToastManager.getInstance();

// Enhanced loading state management
interface LoadingState {
  submitting: boolean;
  validating: boolean;
  fieldStates: Record<string, 'idle' | 'validating' | 'valid' | 'invalid'>;
}

const defaultLoadingState: LoadingState = {
  submitting: false,
  validating: false,
  fieldStates: {},
};

// Password feedback validation keys (must match i18n validation union)
type PasswordFeedbackKey = 'passwordMinLength' | 'passwordUpper' | 'passwordLower' | 'passwordNumber' | 'passwordSpecial';

// Enhanced password strength calculation with typed feedback keys
const calculatePasswordStrength = (password: string): { score: number; feedback: PasswordFeedbackKey[] } => {
  let score = 0;
  const feedback: PasswordFeedbackKey[] = [];

  if (password.length >= 8) score += 1;
  else feedback.push('passwordMinLength');

  if (/[A-Z]/.test(password)) score += 1;
  else feedback.push('passwordUpper');

  if (/[a-z]/.test(password)) score += 1;
  else feedback.push('passwordLower');

  if (/\d/.test(password)) score += 1;
  else feedback.push('passwordNumber');

  if (/[!@#$%^&*(),.?":{}|<>]/.test(password)) score += 1;
  else feedback.push('passwordSpecial');

  if (password.length >= 12) score += 1;

  return { score, feedback };
};

// Enhanced features - now with proper toast manager integration
const defaultToastActions = {
  success: (title: string, description?: string) => toastManager.show({ type: 'success', title, description }),
  error: (title: string, description?: string) => toastManager.show({ type: 'error', title, description }),
  info: (title: string, description?: string) => toastManager.show({ type: 'info', title, description }),
  warning: (title: string, description?: string) => toastManager.show({ type: 'warning', title, description }),
};

interface PasswordStrengthIndicatorProps {
  password: string;
  authT: ReturnType<typeof useAuthTranslation>['authT'];
}

function PasswordStrengthIndicator({ password, authT }: PasswordStrengthIndicatorProps) {
  const strength = calculatePasswordStrength(password);
  const strengthColors = ['bg-red-500', 'bg-orange-500', 'bg-yellow-500', 'bg-blue-500', 'bg-green-500'];
  const strengthLabels = [
    authT.passwordStrength('veryWeak'),
    authT.passwordStrength('weak'),
    authT.passwordStrength('ok'),
    authT.passwordStrength('strong'),
    authT.passwordStrength('veryStrong'),
  ];

  if (!password) return null;

  return (
    <div className="mt-2 space-y-2">
      <div className="flex space-x-1">
        {[...Array(5)].map((_, i) => (
          <div
            key={i}
            className={`h-1 flex-1 rounded ${
              i < strength.score ? strengthColors[Math.min(strength.score - 1, 4)] : 'bg-muted'
            }`}
          />
        ))}
      </div>
      <div className="flex justify-between text-xs">
        <span className="text-muted-foreground">
          {strengthLabels[Math.min(strength.score - 1, 4)] || authT.passwordStrength('enterPassword')}
        </span>
        {strength.feedback.length > 0 && (
          <span className="text-muted-foreground">
            {authT.passwordStrength('missing')}: {strength.feedback
              .map(k => authT.validation(k))
              .join(', ')}
          </span>
        )}
      </div>
    </div>
  );
}

interface FieldValidationIndicatorProps {
  validation?: { valid: boolean; warnings: string[] };
  fieldState?: 'idle' | 'validating' | 'valid' | 'invalid';
}

function FieldValidationIndicator({ validation, fieldState }: FieldValidationIndicatorProps) {
  if (!validation || fieldState === 'idle') return null;

  return (
    <div className="absolute right-3 top-1/2 transform -translate-y-1/2">
      {fieldState === 'validating' && (
        <Loader2 className="w-4 h-4 animate-spin text-muted-foreground" />
      )}
      {fieldState === 'valid' && (
        <CheckCircle className="w-4 h-4 text-green-500" />
      )}
      {fieldState === 'invalid' && (
        <AlertCircle className="w-4 h-4 text-destructive" />
      )}
    </div>
  );
}

interface AuthFormsProps {
  mode: AuthMode;
  formData: AuthFormData;
  errors: Partial<AuthFormErrors>;
  isLoading?: boolean;
  onFieldChange: (field: keyof AuthFormData, value: string) => void;
  onSubmit: (e: React.FormEvent) => void;
  className?: string;
}

export function AuthForms({
  mode,
  formData,
  errors,
  isLoading = false,
  onFieldChange,
  onSubmit,
  className = '',
}: AuthFormsProps) {
  const [showPassword, setShowPassword] = useState(false);
  const [showConfirmPassword, setShowConfirmPassword] = useState(false);
  const [loadingState, setLoadingState] = useState<LoadingState>(defaultLoadingState);
  const [fieldValidations, setFieldValidations] = useState<Record<string, { valid: boolean; warnings: string[] }>>({});
  const isHydrated = useIsHydrated();
  
  // Use the comprehensive i18n system
  const { authT, t } = useAuthTranslation();
  const { commonT } = useCommonTranslation();

  // Create validation schema with translations
  const validationSchema = useMemo(() => ({
    email: [
      {
        test: (value: string) => !!value.trim(),
        message: isHydrated ? authT.validation('emailRequired') : 'E-postadresse er påkrevd',
      },
      {
        test: (value: string) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value),
        message: isHydrated ? authT.validation('emailInvalid') : 'Ugyldig e-postadresse format',
      },
      {
        test: (value: string) => value.length <= 254,
        message: 'E-postadresse er for lang (maks 254 tegn)',
      },
    ],
    password: [
      {
        test: (value: string) => !!value,
        message: isHydrated ? authT.validation('passwordRequired') : 'Passord er påkrevd',
      },
      {
        test: (value: string) => value.length >= 8,
        message: isHydrated ? authT.validation('passwordMinLength') : 'Passord må være minst 8 tegn',
      },
      {
        test: (value: string) => /[A-Z]/.test(value),
        message: 'Passord må inneholde store bokstaver',
      },
      {
        test: (value: string) => /[a-z]/.test(value),
        message: 'Passord må inneholde små bokstaver',
      },
      {
        test: (value: string) => /\d/.test(value),
        message: 'Passord må inneholde tall',
      },
      {
        test: (value: string) => /[!@#$%^&*(),.?":{}|<>]/.test(value),
        message: 'Passord må inneholde spesialtegn',
      },
    ],
    confirmPassword: [
      {
        test: (value: string) => !!value,
        message: 'Bekreft passord er påkrevd',
      },
      {
        test: (value: string) => value === formData?.password,
        message: isHydrated ? authT.validation('passwordMismatch') : 'Passordene stemmer ikke overens',
      },
    ],
    name: [
      {
        test: (value: string) => !!value.trim(),
        message: isHydrated ? authT.validation('nameRequired') : 'Navn er påkrevd',
      },
      {
        test: (value: string) => value.trim().length >= 2,
        message: isHydrated ? authT.validation('nameMinLength') : 'Navn må være minst 2 tegn',
      },
      {
        test: (value: string) => /^[a-zA-ZæøåÆØÅ\s\-'\.]+$/.test(value),
        message: 'Navn inneholder ugyldige tegn',
      },
    ],
  }), [authT, formData?.password, isHydrated]);

  // Enhanced validation function
  const validateField = useCallback((fieldName: string, value: string) => {
    const rules = validationSchema[fieldName as keyof typeof validationSchema];
    if (!rules) return { valid: true, warnings: [] };

    const failedRules = rules.filter(rule => !rule.test(value));
    const warnings = failedRules.map(rule => rule.message);

    return {
      valid: failedRules.length === 0,
      warnings,
    };
  }, [validationSchema]);

  // Enhanced field change handler with validation
  const handleFieldChange = useCallback((field: keyof AuthFormData, value: string) => {
    // Update form data
    onFieldChange(field, value);

    // Validate field
    const validation = validateField(field, value);
    setFieldValidations(prev => ({
      ...prev,
      [field]: validation,
    }));

    // Update loading state for this field
    setLoadingState(prev => ({
      ...prev,
      fieldStates: {
        ...prev.fieldStates,
        [field]: validation.valid ? 'valid' : 'invalid',
      },
    }));
  }, [onFieldChange, validateField]);

  // Only show forms for basic auth modes - now including enterprise SSO and organization
  if (!['signin', 'signup', 'enterprise-sso', 'org'].includes(mode as string)) {
    return null;
  }

  // Enterprise SSO Form with enhanced features
  if (mode === 'enterprise-sso') {
    const handleSSOSubmit = async (e: React.FormEvent) => {
      try {
        await onSubmit(e);
        defaultToastActions.success(authT.action('signin'));
      } catch (error) {
        console.error('SSO error:', error);
        defaultToastActions.error(commonT.error());
      }
    };

    return (
      <form onSubmit={handleSSOSubmit} className={`space-y-4 ${className}`}>
        {/* SSO Email field */}
        <div>
          <label htmlFor="ssoEmail" className="block text-sm font-medium text-foreground mb-1">
            {authT.sso('businessEmail')} *
          </label>
          <p className="mt-1 text-xs text-muted-foreground">
            {authT.sso('description')}
          </p>
          <div className="relative">
            <Mail className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
            <input
              id="ssoEmail"
              name="ssoEmail"
              type="email"
              autoComplete="email"
              required
              value={formData.ssoEmail || ''}
              onChange={(e) => onFieldChange('ssoEmail', e.target.value)}
              disabled={isLoading}
              aria-invalid={!!errors.email}
              className={`w-full pl-10 pr-3 py-2 border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground ${
                errors.email ? 'border-destructive focus:border-destructive' : 'border-border focus:border-ring'
              }`}
              placeholder={authT.placeholder('ssoEmail')}
            />
          </div>
          {errors.email && (
            <p className="mt-1 text-sm text-destructive" role="alert">
              {errors.email}
            </p>
          )}
        </div>

        {/* SSO Domain field */}
        <div>
          <label htmlFor="ssoDomain" className="block text-sm font-medium text-foreground mb-1">
            {authT.sso('organizationDomain')} ({commonT.optional()})
          </label>
          <p className="mt-1 text-xs text-muted-foreground">
            {authT.sso('domainDescription')}
          </p>
          <div className="relative">
            <Building className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
            <input
              id="ssoDomain"
              name="ssoDomain"
              type="text"
              value={formData.ssoDomain || ''}
              onChange={(e) => onFieldChange('ssoDomain', e.target.value)}
              disabled={isLoading}
              className="w-full pl-10 pr-3 py-2 border border-border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground"
              placeholder={authT.placeholder('domain')}
            />
          </div>
        </div>

        {/* Enhanced Submit button with loading state */}
        <button
          type="submit"
          disabled={isLoading}
          className="w-full bg-primary hover:bg-primary/90 text-primary-foreground font-medium py-2.5 px-4 rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2"
        >
          {isLoading ? (
            <div className="flex items-center justify-center gap-2">
              <Loader2 size={16} className="animate-spin" />
              <span>{authT.sso('connecting')}</span>
            </div>
          ) : (
            authT.sso('continue')
          )}
        </button>
      </form>
    );
  }

  // Organization Management Form with enhanced features
  if (mode === 'org') {
    const handleOrgSubmit = async (e: React.FormEvent) => {
      try {
        await onSubmit(e);
        defaultToastActions.success(authT.org('create'));
      } catch (error) {
        console.error('Organization error:', error);
        defaultToastActions.error(commonT.error());
      }
    };

    return (
      <form onSubmit={handleOrgSubmit} className={`space-y-4 ${className}`}>
        {/* Organization Name field */}
        <div>
          <label htmlFor="orgName" className="block text-sm font-medium text-foreground mb-1">
            {authT.org('name')} *
          </label>
          <p className="mt-1 text-xs text-muted-foreground">
            {authT.org('nameDescription')}
          </p>
          <div className="relative">
            <Building className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
            <input
              id="orgName"
              name="orgName"
              type="text"
              required
              value={formData.orgName || ''}
              onChange={(e) => onFieldChange('orgName', e.target.value)}
              disabled={isLoading}
              className="w-full pl-10 pr-3 py-2 border border-border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground"
              placeholder={authT.placeholder('orgName')}
            />
          </div>
        </div>

        {/* Organization Slug field */}
        <div>
          <label htmlFor="orgSlug" className="block text-sm font-medium text-foreground mb-1">
            {authT.org('slug')} *
          </label>
          <p className="mt-1 text-xs text-muted-foreground">
            {authT.org('slugDescription')}
          </p>
          <div className="relative">
            <Building className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
            <input
              id="orgSlug"
              name="orgSlug"
              type="text"
              required
              value={formData.orgSlug || ''}
              onChange={(e) => onFieldChange('orgSlug', e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ''))}
              disabled={isLoading}
              className="w-full pl-10 pr-3 py-2 border border-border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground"
              placeholder={authT.placeholder('orgSlug')}
            />
          </div>
        </div>

        {/* Invite Email field */}
        <div>
          <label htmlFor="inviteEmail" className="block text-sm font-medium text-foreground mb-1">
            {authT.org('inviteEmail')} ({commonT.optional()})
          </label>
          <p className="mt-1 text-xs text-muted-foreground">
            {authT.org('inviteDescription')}
          </p>
          <div className="relative">
            <Users className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
            <input
              id="inviteEmail"
              name="inviteEmail"
              type="email"
              value={formData.inviteEmail || ''}
              onChange={(e) => onFieldChange('inviteEmail', e.target.value)}
              disabled={isLoading}
              className="w-full pl-10 pr-3 py-2 border border-border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground"
              placeholder={authT.placeholder('inviteEmail')}
            />
          </div>
        </div>

        {/* Invite Role field */}
        {formData.inviteEmail && (
          <div>
            <label htmlFor="inviteRole" className="block text-sm font-medium text-foreground mb-1">
              {authT.org('inviteRole')}
            </label>
            <select
              id="inviteRole"
              name="inviteRole"
              value={formData.inviteRole || 'member'}
              onChange={(e) => onFieldChange('inviteRole', e.target.value)}
              disabled={isLoading}
              className="w-full px-3 py-2 border border-border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground"
            >
              <option value="member">{authT.role('member')}</option>
              <option value="admin">{authT.role('admin')}</option>
            </select>
          </div>
        )}

        {/* Enhanced Submit button with loading state */}
        <button
          type="submit"
          disabled={isLoading}
          className="w-full bg-primary hover:bg-primary/90 text-primary-foreground font-medium py-2.5 px-4 rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2"
        >
          {isLoading ? (
            <div className="flex items-center justify-center gap-2">
              <Loader2 size={16} className="animate-spin" />
              <span>{authT.org('creating')}</span>
            </div>
          ) : (
            authT.org('create')
          )}
        </button>
      </form>
    );
  }

  // Enhanced main form submission handler
  const handleMainSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    
    try {
      setLoadingState(prev => ({ ...prev, submitting: true }));
      
      await onSubmit(e);
      
      const successMessage = mode === 'signup' ? authT.message('accountCreated') : 
                           mode === 'signin' ? authT.message('loginSuccess') : 
                           authT.message('emailSent');
      defaultToastActions.success(successMessage);
      
      // Check if authentication was successful and redirect or update UI
      const { authClient } = await import('../lib/auth-client-enterprise');
      const session = await authClient.getSession();
      
      if (session.data?.user) {
        // User is now authenticated, we could redirect or emit an event
        console.log('User authenticated successfully:', session.data.user);
        
        // Dispatch a custom event to notify the application
        window.dispatchEvent(new CustomEvent('auth-success', {
          detail: { user: session.data.user, mode }
        }));
      }
      
    } catch (error) {
      console.error('Auth error:', error);
      defaultToastActions.error(
        mode === 'signin' ? 'Sign in failed' : 
        mode === 'signup' ? 'Account creation failed' : 
        'Authentication failed',
        error instanceof Error ? error.message : 'Please try again'
      );
    } finally {
      setLoadingState(prev => ({ ...prev, submitting: false }));
    }
  };

  return (
    <form onSubmit={handleMainSubmit} className={`space-y-4 ${className}`}>
      {/* Name field for signup - Norwegian labels */}
      {mode === 'signup' && (
        <div>
          <label htmlFor="name" className="block text-sm font-medium text-foreground mb-1">
            {authT.field('name')} *
          </label>
          <div className="relative">
            <User className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
            <input
              id="name"
              name="name"
              type="text"
              autoComplete="name"
              required
              value={formData.name || ''}
              onChange={(e) => onFieldChange('name', e.target.value)}
              disabled={isLoading}
              aria-invalid={!!errors.name}
              className={`w-full pl-10 pr-3 py-2 border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground ${
                errors.name ? 'border-destructive focus:border-destructive' : 'border-border focus:border-ring'
              }`}
              placeholder={authT.placeholder('name')}
            />
          </div>
          {errors.name && (
            <p className="mt-1 text-sm text-destructive" role="alert">
              {errors.name}
            </p>
          )}
        </div>
      )}

      {/* Email field - Enhanced with validation and SSR safety */}
      <div>
        <label htmlFor="email" className="block text-sm font-medium text-foreground mb-1">
          {isHydrated ? authT.field('email') : 'E-postadresse'} *
        </label>
        <div className="relative">
          <Mail className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
          <input
            id="email"
            name="email"
            type="email"
            autoComplete={mode === 'signin' ? "email webauthn" : "email"}
            required
            value={formData.email || ''}
            onChange={(e) => handleFieldChange('email', e.target.value)}
            disabled={isLoading}
            aria-invalid={!!errors.email}
            className={`w-full pl-10 pr-10 py-2 border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground ${
              errors.email || (fieldValidations.email && !fieldValidations.email.valid) 
                ? 'border-destructive focus:border-destructive' 
                : fieldValidations.email?.valid 
                ? 'border-green-500 focus:border-green-500' 
                : 'border-border focus:border-ring'
            }`}
            placeholder={isHydrated ? authT.placeholder('email') : 'din@epost.no'}
          />
          <FieldValidationIndicator validation={fieldValidations.email} fieldState={loadingState.fieldStates['email']} />
        </div>
        {/* Enhanced error display with field validation */}
        {(errors.email || (fieldValidations.email && !fieldValidations.email.valid)) && (
          <div className="mt-1 space-y-1">
            {errors.email && (
              <p className="text-sm text-destructive" role="alert">
                {errors.email}
              </p>
            )}
            {fieldValidations.email && !fieldValidations.email.valid && (
              <div className="space-y-1">
                {fieldValidations.email.warnings.map((warning, index) => (
                  <p key={index} className="text-xs text-orange-600" role="alert">
                    • {warning}
                  </p>
                ))}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Password field - Enhanced with strength indicator */}
  {/* Password field (forgot mode not part of current AuthMode union) */}
  {(
        <div>
          <label htmlFor="password" className="block text-sm font-medium text-foreground mb-1">
            {isHydrated ? authT.field('password') : 'Passord'} *
          </label>
          <div className="relative">
            <Lock className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
            <input
              id="password"
              name="password"
              type={showPassword ? 'text' : 'password'}
              autoComplete={mode === 'signin' ? 'current-password' : 'new-password'}
              required
              value={formData.password || ''}
              onChange={(e) => handleFieldChange('password', e.target.value)}
              disabled={isLoading}
              aria-invalid={!!errors.password}
              className={`w-full pl-10 pr-16 py-2 border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground ${
                errors.password || (fieldValidations.password && !fieldValidations.password.valid) 
                  ? 'border-destructive focus:border-destructive' 
                  : fieldValidations.password?.valid 
                  ? 'border-green-500 focus:border-green-500' 
                  : 'border-border focus:border-ring'
              }`}
              placeholder={isHydrated ? authT.placeholder('password') : 'Skriv inn ditt passord'}
            />
            <div className="absolute right-3 top-1/2 transform -translate-y-1/2 flex items-center space-x-1">
              <FieldValidationIndicator validation={fieldValidations.password} fieldState={loadingState.fieldStates['password']} />
              <button
                type="button"
                onClick={() => setShowPassword(!showPassword)}
                disabled={isLoading}
                className="text-muted-foreground hover:text-foreground focus:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 rounded disabled:opacity-50"
                aria-label={showPassword ? 'Skjul passord' : 'Vis passord'}
              >
                {showPassword ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
              </button>
            </div>
          </div>
          
          {/* Password strength indicator for signup */}
          {mode === 'signup' && formData.password && (
            <PasswordStrengthIndicator password={formData.password} authT={authT} />
          )}
          
          {/* Enhanced error display */}
          {(errors.password || (fieldValidations.password && !fieldValidations.password.valid)) && (
            <div className="mt-1 space-y-1">
              {errors.password && (
                <p className="text-sm text-destructive" role="alert">
                  {errors.password}
                </p>
              )}
              {fieldValidations.password && !fieldValidations.password.valid && mode === 'signup' && (
                <div className="space-y-1">
                  {fieldValidations.password.warnings.slice(0, 3).map((warning, index) => (
                    <p key={index} className="text-xs text-orange-600" role="alert">
                      • {warning}
                    </p>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      )}

      {/* Confirm Password field - only for signup */}
      {mode === 'signup' && (
        <div>
          <label htmlFor="confirmPassword" className="block text-sm font-medium text-foreground mb-1">
            {authT.field('confirmPassword')} *
          </label>
          <div className="relative">
            <Lock className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
            <input
              id="confirmPassword"
              name="confirmPassword"
              type={showConfirmPassword ? 'text' : 'password'}
              required
              value={formData.confirmPassword || ''}
              onChange={(e) => onFieldChange('confirmPassword', e.target.value)}
              disabled={isLoading}
              aria-invalid={!!errors.confirmPassword}
              className={`w-full pl-10 pr-12 py-2 border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground ${
                errors.confirmPassword ? 'border-destructive focus:border-destructive' : 'border-border focus:border-ring'
              }`}
              placeholder={authT.placeholder('password')}
            />
            <button
              type="button"
              onClick={() => setShowConfirmPassword(!showConfirmPassword)}
              disabled={isLoading}
              className="absolute right-3 top-1/2 transform -translate-y-1/2 text-muted-foreground hover:text-foreground focus:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 rounded disabled:opacity-50"
              aria-label={showConfirmPassword ? 'Skjul bekreft passord' : 'Vis bekreft passord'}
            >
              {showConfirmPassword ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
            </button>
          </div>
          {errors.confirmPassword && (
            <p className="mt-1 text-sm text-destructive" role="alert">
              {errors.confirmPassword}
            </p>
          )}
        </div>
      )}

      {/* Enhanced Submit button with loading state - Norwegian text */}
      <button
        type="submit"
        disabled={isLoading}
        className="w-full bg-primary hover:bg-primary/90 text-primary-foreground font-medium py-2.5 px-4 rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2"
      >
        {isLoading ? (
          <div className="flex items-center justify-center gap-2">
            <Loader2 size={16} className="animate-spin" />
            <span>{commonT.loading()}</span>
          </div>
        ) : (
          mode === 'signup' ? authT.action('signup') : 
          mode === 'signin' ? authT.action('signin') : 
          authT.action('forgotPassword')
        )}
      </button>
    </form>
  );
}

export default AuthForms;