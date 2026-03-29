'use client';

import { useState, useCallback, useEffect, useRef } from 'react';
import { 
  Building2, 
  ArrowLeft, 
  Loader2, 
  CheckCircle, 
  AlertTriangle, 
  Shield,
  ExternalLink,
  Info,
  Mail
} from 'lucide-react';
import Image from 'next/image';
import { useAuthForm } from '../hooks/use-auth-form';
import { useAuthTranslation } from '../lib/i18n/hooks';
import { 
  useDiscoverSSO, 
  useAuthenticateSSO, 
  useEmailAvailability,
  useAuthProviderUtils 
} from '../lib/api/auth-provider-hooks';

interface EnterpriseSSOProps {
  onSuccess?: (result: { provider: string; email: string }) => void;
  onError?: (error: string) => void;
  redirectTo?: string;
  className?: string;
}

interface SSOProvider {
  id: string;
  name: string;
  domain: string;
  logoUrl?: string;
  isEnabled: boolean;
  loginUrl: string;
  description?: string;
  supportedFeatures: string[];
}

interface SSOAnalytics {
  step: 'input' | 'providers' | 'authenticating';
  provider?: string;
  domain?: string;
  success?: boolean;
  errorType?: string;
  timeToComplete?: number;
}

export function EnterpriseSSO({
  onSuccess: _onSuccess,  
  onError,
  redirectTo,
  className = '',
}: EnterpriseSSOProps) {
  const { t, authT } = useAuthTranslation();
  const [step, setStep] = useState<'input' | 'providers' | 'authenticating'>('input');
  const [providers, setProviders] = useState<SSOProvider[]>([]);
  const [selectedProvider, setSelectedProvider] = useState<SSOProvider | null>(null);
  const [validationError, setValidationError] = useState<string | null>(null);
  const [startTime] = useState<number>(Date.now());
  
  // Refs for accessibility
  const emailInputRef = useRef<HTMLInputElement>(null);
  const providersListRef = useRef<HTMLDivElement>(null);
  const statusRef = useRef<HTMLDivElement>(null);

  const { formData, setField } = useAuthForm({
    initialData: {
      ssoEmail: '',
      ssoDomain: '',
    },
  });

  // Enhanced hooks for SSO
  const discoverSSOMutation = useDiscoverSSO();
  const authenticateSSOMutation = useAuthenticateSSO();
  const { isBusinessEmail, getDomainFromEmail } = useAuthProviderUtils();
  
  // Real-time email availability check (debounced)
  const emailAvailability = useEmailAvailability(
    formData.ssoEmail,
    formData.ssoEmail.includes('@') && formData.ssoEmail.length > 5
  );

  // Real-time email validation with enhanced business email check
  const validateEmail = useCallback((email: string): string | null => {
    if (!email) return t('auth.validation.emailRequired');
    
    const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
    if (!emailRegex.test(email)) return t('auth.validation.emailInvalid');
    
    // Use the utility function for business email check
    if (!isBusinessEmail(email)) {
      return authT.sso('businessEmailRequired');
    }
    
    return null;
  }, [t, authT, isBusinessEmail]);

  // Real-time validation effect
  useEffect(() => {
    if (formData.ssoEmail) {
      const error = validateEmail(formData.ssoEmail);
      setValidationError(error);
    } else {
      setValidationError(null);
    }
  }, [formData.ssoEmail, validateEmail]);

  // Analytics tracking (simplified implementation)
  const trackSSOEvent = useCallback((eventData: Partial<SSOAnalytics>) => {
    console.log('SSO Analytics:', {
      ...eventData,
      timestamp: new Date().toISOString(),
      sessionTime: Date.now() - startTime,
    });
  }, [startTime]);

  const handleEmailSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    
    const emailError = validateEmail(formData.ssoEmail);
    if (emailError) {
      setValidationError(emailError);
      onError?.(emailError);
      emailInputRef.current?.focus();
      return;
    }

    const domain = getDomainFromEmail(formData.ssoEmail);
    setField('ssoDomain', domain);
    trackSSOEvent({ step: 'input', domain });
    
    try {
      const result = await discoverSSOMutation.mutateAsync({ 
        email: formData.ssoEmail, 
        domain 
      });
      
      if (result.success && result.providers && result.providers.length > 0) {
        setProviders(result.providers);
        setStep('providers');
        trackSSOEvent({ step: 'providers', domain });
        
        setTimeout(() => {
          providersListRef.current?.focus();
        }, 100);
      } else {
        const noProvidersError = authT.sso('noProvidersFound');
        onError?.(noProvidersError);
        trackSSOEvent({ step: 'input', errorType: 'no_providers', domain });
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : authT.sso('discoveryError');
      onError?.(message);
      trackSSOEvent({ step: 'input', errorType: 'discovery_failed', domain });
    }
  };

  const handleProviderSelect = async (provider: SSOProvider) => {
    setSelectedProvider(provider);
    setStep('authenticating');
    trackSSOEvent({ step: 'authenticating', provider: provider.name });

    try {
      const result = await authenticateSSOMutation.mutateAsync({
        email: formData.ssoEmail,
        providerId: provider.id,
        redirectTo,
      });

      if (result.success && result.redirectUrl) {
        trackSSOEvent({ 
          step: 'authenticating', 
          provider: provider.name, 
          success: true,
          timeToComplete: Date.now() - startTime
        });

        if (statusRef.current) {
          statusRef.current.textContent = authT.sso('redirecting').replace('{{provider}}', provider.name);
        }

        window.location.href = result.redirectUrl;
      } else {
        throw new Error(result.message || authT.sso('authError'));
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : authT.sso('authError');
      onError?.(message);
      trackSSOEvent({ 
        step: 'authenticating', 
        provider: provider.name, 
        errorType: 'auth_failed',
        success: false 
      });
      setStep('providers');
    }
  };

  const handleBack = () => {
    if (step === 'providers') {
      setStep('input');
      setProviders([]);
      trackSSOEvent({ step: 'input' });
      setTimeout(() => {
        emailInputRef.current?.focus();
      }, 100);
    } else if (step === 'authenticating') {
      setStep('providers');
      setSelectedProvider(null);
      trackSSOEvent({ step: 'providers' });
    }
  };

  const handleKeyDown = useCallback((e: React.KeyboardEvent, action: () => void) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      action();
    }
  }, []);

  // Computed loading states from mutations
  const isLoading = discoverSSOMutation.isPending || authenticateSSOMutation.isPending;

  if (step === 'input') {
    return (
      <div className={`space-y-6 ${className}`} role="region" aria-labelledby="sso-heading">
        <div className="text-center space-y-3">
          <div className="w-16 h-16 bg-primary/10 rounded-full flex items-center justify-center mx-auto">
            <Building2 className="w-8 h-8 text-primary" />
          </div>
          <div>
            <h2 id="sso-heading" className="text-xl font-semibold text-foreground">
              {authT.sso('title')}
            </h2>
            <p className="text-sm text-muted-foreground mt-2 max-w-md mx-auto leading-relaxed">
              {authT.sso('description')}
            </p>
          </div>
        </div>

        <form onSubmit={handleEmailSubmit} className="space-y-4" noValidate>
          <div className="space-y-2">
            <label 
              htmlFor="sso-email" 
              className="text-sm font-medium text-foreground flex items-center gap-2"
            >
              <Mail className="w-4 h-4" />
              {authT.sso('businessEmail')} *
            </label>
            <div className="relative">
              <input
                ref={emailInputRef}
                id="sso-email"
                type="email"
                placeholder={authT.placeholder('ssoEmail')}
                value={formData.ssoEmail}
                onChange={(e) => setField('ssoEmail', e.target.value)}
                disabled={isLoading}
                aria-invalid={!!validationError}
                aria-describedby={validationError ? 'email-error' : 'email-help'}
                className={`w-full px-4 py-3 border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground transition-colors ${
                  validationError 
                    ? 'border-red-500 focus:ring-red-500' 
                    : 'border-border focus:border-primary'
                }`}
                required
                autoComplete="email"
              />
              {validationError && (
                <AlertTriangle className="absolute right-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-red-500" />
              )}
              {!validationError && emailAvailability.data && formData.ssoEmail.includes('@') && (
                <CheckCircle className="absolute right-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-green-600" />
              )}
            </div>
            
            {validationError ? (
              <p id="email-error" role="alert" className="text-sm text-red-600 flex items-center gap-2">
                <AlertTriangle className="w-3 h-3" />
                {validationError}
              </p>
            ) : (
              <p id="email-help" className="text-xs text-muted-foreground">
                {authT.sso('emailHelp')}
              </p>
            )}
          </div>

          <button
            type="submit"
            disabled={isLoading || !!validationError || !formData.ssoEmail}
            className="w-full bg-primary hover:bg-primary/90 text-primary-foreground font-medium py-3 px-4 rounded-lg transition-all duration-200 focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
            aria-describedby="continue-help"
          >
            {isLoading ? (
              <>
                <Loader2 className="w-4 h-4 animate-spin" />
                {authT.sso('connecting')}
              </>
            ) : (
              <>
                <Shield className="w-4 h-4" />
                {authT.sso('continue')}
              </>
            )}
          </button>
          <p id="continue-help" className="text-xs text-muted-foreground text-center">
            {authT.sso('continueHelp')}
          </p>
        </form>

        <div className="mt-6 p-4 bg-blue-50 border border-blue-200 rounded-lg">
          <div className="flex items-start gap-3">
            <Info className="w-5 h-5 text-blue-600 flex-shrink-0 mt-0.5" />
            <div className="space-y-2">
              <h3 className="text-sm font-medium text-blue-900">
                {authT.sso('aboutSso')}
              </h3>
              <p className="text-sm text-blue-700 leading-relaxed">
                {authT.sso('aboutDescription')}
              </p>
            </div>
          </div>
        </div>

        <div ref={statusRef} className="sr-only" aria-live="polite" aria-atomic="true"></div>
      </div>
    );
  }

  if (step === 'providers') {
    return (
      <div className={`space-y-6 ${className}`} role="region" aria-labelledby="providers-heading">
        <div className="flex items-center gap-3">
          <button
            onClick={handleBack}
            onKeyDown={(e) => handleKeyDown(e, handleBack)}
            className="p-2 rounded-lg hover:bg-muted transition-colors focus:outline-none focus:ring-2 focus:ring-ring"
            aria-label={t('common.back')}
          >
            <ArrowLeft className="w-4 h-4" />
          </button>
          <div>
            <h2 id="providers-heading" className="text-lg font-semibold text-foreground">
              {authT.sso('selectProvider')}
            </h2>
            <p className="text-sm text-muted-foreground">
              {authT.sso('selectDescription').replace('{{domain}}', formData.ssoDomain)}
            </p>
          </div>
        </div>

        <div 
          ref={providersListRef}
          className="space-y-3"
          role="list"
          tabIndex={-1}
          aria-label={authT.sso('providersListLabel')}
        >
          {providers.map((provider) => (
            <button
              key={provider.id}
              onClick={() => handleProviderSelect(provider)}
              onKeyDown={(e) => handleKeyDown(e, () => handleProviderSelect(provider))}
              disabled={!provider.isEnabled || isLoading}
              role="listitem"
              className="w-full p-4 border border-border rounded-lg hover:bg-muted transition-all duration-200 focus:outline-none focus:ring-2 focus:ring-ring text-left disabled:opacity-50 disabled:cursor-not-allowed group"
              aria-describedby={`provider-${provider.id}-desc`}
            >
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-4">
                  <div className="w-12 h-12 bg-primary/10 rounded-lg flex items-center justify-center">
                    {provider.logoUrl ? (
                      <Image
                        src={provider.logoUrl}
                        alt={`${provider.name} logo`}
                        width={32}
                        height={32}
                        className="rounded"
                      />
                    ) : (
                      <Shield className="w-6 h-6 text-primary" />
                    )}
                  </div>
                  <div>
                    <h3 className="font-medium text-foreground group-hover:text-primary transition-colors">
                      {provider.name}
                    </h3>
                    <p id={`provider-${provider.id}-desc`} className="text-sm text-muted-foreground">
                      {provider.description || authT.sso('defaultProviderDescription')}
                    </p>
                  </div>
                </div>
                {provider.isEnabled ? (
                  <CheckCircle className="w-5 h-5 text-green-600" />
                ) : (
                  <AlertTriangle className="w-5 h-5 text-yellow-600" />
                )}
              </div>
            </button>
          ))}
        </div>

        {providers.some(p => !p.isEnabled) && (
          <div className="p-3 bg-yellow-50 border border-yellow-200 rounded-lg">
            <div className="flex items-center gap-2">
              <AlertTriangle className="w-4 h-4 text-yellow-600" />
              <p className="text-sm text-yellow-700">
                {authT.sso('disabledProvidersNotice')}
              </p>
            </div>
          </div>
        )}
      </div>
    );
  }

  if (step === 'authenticating') {
    return (
      <div className={`space-y-6 text-center ${className}`} role="region" aria-labelledby="auth-heading">
        <div className="space-y-4">
          <div className="w-16 h-16 bg-primary/10 rounded-full flex items-center justify-center mx-auto">
            <Loader2 className="w-8 h-8 text-primary animate-spin" />
          </div>
          <div>
            <h2 id="auth-heading" className="text-lg font-semibold text-foreground">
              {authT.sso('authenticating')}
            </h2>
            <p className="text-sm text-muted-foreground mt-2">
              {authT.sso('authenticatingDescription').replace('{{provider}}', selectedProvider?.name || '')}
            </p>
          </div>
        </div>

        <div className="w-full bg-gray-200 rounded-full h-2">
          <div className="bg-primary h-2 rounded-full animate-pulse" style={{ width: '75%' }}></div>
        </div>

        <button
          onClick={handleBack}
          className="text-sm text-muted-foreground hover:text-foreground underline focus:outline-none focus:ring-2 focus:ring-ring rounded"
        >
          {t('common.cancel')}
        </button>
      </div>
    );
  }

  return null;
}

// Simplified SSO button for integration in other forms
interface SSOButtonProps {
  email?: string;
  onInitiate?: () => void;
  disabled?: boolean;
  className?: string;
}

export function SSOButton({
  email,
  onInitiate,
  disabled = false,
  className = '',
}: SSOButtonProps) {
  const { authT } = useAuthTranslation();
  const discoverSSOMutation = useDiscoverSSO();
  const { getDomainFromEmail } = useAuthProviderUtils();

  const handleClick = async () => {
    if (!email) return;
    
    onInitiate?.();
    
    try {
      const domain = getDomainFromEmail(email);
      const result = await discoverSSOMutation.mutateAsync({ 
        email, 
        domain 
      });

      if (result.success && result.providers && result.providers.length > 0) {
        // Handle SSO flow - you might want to emit an event or navigate
        console.log('SSO providers found:', result.providers);
      }
    } catch (error) {
      console.error('SSO discovery failed:', error);
    }
  };

  if (!email || !email.includes('@')) {
    return null;
  }

  return (
    <button
      type="button"
      onClick={handleClick}
      disabled={disabled || discoverSSOMutation.isPending}
      className={`w-full flex items-center justify-center gap-2 px-4 py-2.5 border border-border rounded-lg hover:bg-muted transition-colors focus:outline-none focus:ring-2 focus:ring-ring disabled:opacity-50 ${className}`}
      aria-describedby="sso-button-help"
    >
      {discoverSSOMutation.isPending ? (
        <>
          <Loader2 className="w-4 h-4 animate-spin" />
          {authT.sso('checking')}
        </>
      ) : (
        <>
          <Building2 className="w-4 h-4" />
          {authT.sso('continue')}
        </>
      )}
    </button>
  );
}

// Domain-based SSO component for when you know the domain
interface DomainSSOProps {
  domain: string;
  onSuccess?: (result: { provider: string }) => void;
  onError?: (error: string) => void;
  className?: string;
}

export function DomainSSO({
  domain,
  onSuccess,
  onError,
  className = '',
}: DomainSSOProps) {
  const { authT } = useAuthTranslation();
  const discoverSSOMutation = useDiscoverSSO();

  const handleDomainSSO = async () => {
    try {
      const result = await discoverSSOMutation.mutateAsync({ 
        email: `admin@${domain}`, // Use a dummy email for domain discovery
        domain 
      });
      
      if (result.success && result.providers && result.providers.length > 0) {
        onSuccess?.({ provider: result.providers[0].name });
      } else {
        onError?.(authT.sso('noProvidersFound'));
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : authT.sso('discoveryError');
      onError?.(message);
    }
  };

  return (
    <div className={`text-center space-y-4 ${className}`}>
      <div className="w-16 h-16 bg-primary/10 rounded-full flex items-center justify-center mx-auto">
        <Building2 className="w-8 h-8 text-primary" />
      </div>
      <div>
        <h3 className="text-lg font-semibold text-foreground">
          {authT.sso('domainTitle').replace('{{domain}}', domain)}
        </h3>
        <p className="text-sm text-muted-foreground mt-1">
          {authT.sso('domainDescription')}
        </p>
      </div>
      <button
        onClick={handleDomainSSO}
        disabled={discoverSSOMutation.isPending}
        className="px-6 py-2.5 bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors focus:outline-none focus:ring-2 focus:ring-ring disabled:opacity-50 flex items-center gap-2 mx-auto"
      >
        {discoverSSOMutation.isPending ? (
          <>
            <Loader2 className="w-4 h-4 animate-spin" />
            {authT.sso('connecting')}
          </>
        ) : (
          <>
            <ExternalLink className="w-4 h-4" />
            {authT.sso('continue')}
          </>
        )}
      </button>
    </div>
  );
}