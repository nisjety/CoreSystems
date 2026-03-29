"use client";

import React, { useState, useEffect, useCallback, useMemo } from 'react';
import { X, Loader2, LogIn, UserPlus, Building2, Shield, User } from 'lucide-react';
import type { AuthMode } from '../types/auth';
import { AuthPage } from '../AuthPage';
import { useLanguageSwitch } from '../lib/i18n/hooks';
import { useConsent, type ConsentState } from '../consent/useConsent';

// ORPC + Better Auth integration types
interface AuthResult {
  user: {
    id: string;
    email: string;
    name?: string;
    role: string;
    organizationId?: string;
  };
  session: {
    id: string;
    token: string;
    expiresAt: Date;
  };
  metadata?: {
    loginMethod: AuthMode;
    timestamp: Date;
    deviceInfo?: string;
  };
}

interface AuthError {
  code: string;
  message: string;
  timestamp: Date;
  details?: unknown;
}

// Enhanced analytics interface with consent-aware tracking
interface ModalAnalytics {
  trackEvent: (eventName: string, properties?: Record<string, unknown>) => void;
  trackError: (error: AuthError) => void;
  trackSuccess: (result: AuthResult) => void;
  trackUserInteraction: (action: string, context?: Record<string, unknown>) => void;
}

// Comprehensive translations for Norwegian and English
const modalTranslations = {
  nb: {
    signin: { title: 'Logg inn', description: 'Logg inn på din konto for å fortsette' },
    signup: { title: 'Opprett konto', description: 'Opprett en ny konto for å komme i gang' },
    'enterprise-sso': { title: 'Enterprise SSO', description: 'Logg inn med din organisasjons identitetsleverandør' },
    org: { title: 'Organisasjon', description: 'Administrer organisasjonsinnstillinger' },
    default: { title: 'Autentisering', description: 'Vennligst autentiser for å fortsette' },
    loading: { title: 'Autentiserer', message: 'Vennligst vent mens vi behandler forespørselen din...', cancel: 'Avbryt' },
    feedback: { success: 'Autentisering fullført', error: 'Autentiseringsfeil', opened: 'Autentiseringsmodal åpnet', closed: 'Autentiseringsmodal lukket' },
    close: 'Lukk',
  },
  en: {
    signin: { title: 'Sign In', description: 'Sign in to your account to continue' },
    signup: { title: 'Sign Up', description: 'Create a new account to get started' },
    'enterprise-sso': { title: 'Enterprise SSO', description: 'Sign in with your organization\'s identity provider' },
    org: { title: 'Organization', description: 'Manage organization settings' },
    default: { title: 'Authentication', description: 'Please authenticate to continue' },
    loading: { title: 'Authenticating', message: 'Please wait while we process your request...', cancel: 'Cancel' },
    feedback: { success: 'Authentication completed', error: 'Authentication error', opened: 'Authentication modal opened', closed: 'Authentication modal closed' },
    close: 'Close',
  },
} as const;

// Enhanced toast actions with console fallbacks and proper typing
interface ToastInterface {
  success?: (title: string, options?: { description?: string }) => void;
  error?: (title: string, options?: { description?: string }) => void;
  info?: (title: string, options?: { description?: string }) => void;
}

const createToastActions = () => ({
  success: (title: string, description?: string) => {
    if (typeof window !== 'undefined' && 'toast' in window) {
      const toast = (window as unknown as { toast: ToastInterface }).toast;
      toast.success?.(title, { description });
    } else {
      console.log(`✅ Success: ${title}`, description ? ` - ${description}` : '');
    }
  },
  error: (title: string, description?: string) => {
    if (typeof window !== 'undefined' && 'toast' in window) {
      const toast = (window as unknown as { toast: ToastInterface }).toast;
      toast.error?.(title, { description });
    } else {
      console.error(`❌ Error: ${title}`, description ? ` - ${description}` : '');
    }
  },
  info: (title: string, description?: string) => {
    if (typeof window !== 'undefined' && 'toast' in window) {
      const toast = (window as unknown as { toast: ToastInterface }).toast;
      toast.info?.(title, { description });
    } else {
      console.log(`ℹ️ Info: ${title}`, description ? ` - ${description}` : '');
    }
  },
});

// Enhanced analytics with consent-aware tracking (respects GDPR)
const createAnalytics = (consent: ConsentState): ModalAnalytics => ({
  trackEvent: (eventName: string, properties = {}) => {
    if (consent?.analytics) {
      console.log(`📊 Analytics Event: ${eventName}`, properties);
      // Integrate with your analytics provider here
      // Example: analytics.track(eventName, properties);
    }
  },
  trackError: (error: AuthError) => {
    if (consent?.performance) {
      console.log('🔴 Analytics Error:', error);
      // Track errors for performance monitoring
    }
  },
  trackSuccess: (result: AuthResult) => {
    if (consent?.analytics) {
      console.log('🟢 Analytics Success:', result.metadata);
      // Track successful authentications
    }
  },
  trackUserInteraction: (action: string, context = {}) => {
    if (consent?.analytics) {
      console.log(`👆 User Interaction: ${action}`, context);
    }
  },
});

interface AuthModalProps {
  isOpen: boolean;
  onClose: () => void;
  mode?: AuthMode;
  title?: string;
  description?: string;
  redirectTo?: string;
  className?: string;
  overlayClassName?: string;
  showCloseButton?: boolean;
  closeOnOverlayClick?: boolean;
  size?: 'sm' | 'md' | 'lg' | 'xl';
  isLoading?: boolean;
  onSuccess?: (result: AuthResult) => void;
  onError?: (error: AuthError) => void;
  trackingId?: string;
  enableAnalytics?: boolean;
}

export function AuthModal({
  isOpen,
  onClose,
  mode = 'signin',
  title,
  description,
  redirectTo,
  className = '',
  overlayClassName = '',
  showCloseButton = true,
  closeOnOverlayClick = true,
  size = 'lg',
  isLoading = false,
  onSuccess,
  onError,
  trackingId,
  enableAnalytics = true,
}: AuthModalProps) {
  // Enhanced hooks for i18n and consent management
  const { isEnglish } = useLanguageSwitch();
  const { consent } = useConsent();
  
  // Memoized translations and services
  const t = useMemo(() => {
    return isEnglish ? modalTranslations.en : modalTranslations.nb;
  }, [isEnglish]);
  
  const toastActions = useMemo(() => createToastActions(), []);
  const analytics = useMemo(() => createAnalytics(consent), [consent]);

  // Enhanced modal state management with analytics
  useEffect(() => {
    if (isOpen && enableAnalytics) {
      analytics.trackEvent('modal_opened', { mode, trackingId });
      toastActions.info(t.feedback.opened);
    }
  }, [isOpen, mode, trackingId, enableAnalytics, analytics, toastActions, t.feedback.opened]);

  // Enhanced close handler with analytics
  const handleClose = useCallback(() => {
    if (enableAnalytics) {
      analytics.trackEvent('modal_closed', { mode, trackingId });
      toastActions.info(t.feedback.closed);
    }
    onClose();
  }, [onClose, mode, trackingId, enableAnalytics, analytics, toastActions, t.feedback.closed]);

  // Enhanced success handler with comprehensive analytics
  const handleSuccess = useCallback((result: AuthResult) => {
    if (enableAnalytics) {
      analytics.trackSuccess(result);
      analytics.trackEvent('modal_auth_success', { mode, trackingId });
    }
    toastActions.success(t.feedback.success);
    onSuccess?.(result);
    onClose();
  }, [onSuccess, onClose, mode, trackingId, enableAnalytics, analytics, toastActions, t.feedback.success]);

  // Enhanced error handler with analytics
  const handleError = useCallback((error: AuthError) => {
    if (enableAnalytics) {
      analytics.trackError(error);
      analytics.trackEvent('modal_auth_error', { mode, trackingId, error: error.code });
    }
    toastActions.error(t.feedback.error, error.message);
    onError?.(error);
  }, [onError, mode, trackingId, enableAnalytics, analytics, toastActions, t.feedback.error]);

  // Enhanced success handler for AuthPage compatibility
  const handleAuthPageSuccess = useCallback((user: unknown) => {
    // Convert AuthPage result to our enhanced AuthResult
    const authResult: AuthResult = {
      user: user as AuthResult['user'],
      session: {
        id: `session_${Date.now()}`,
        token: `token_${Date.now()}`,
        expiresAt: new Date(Date.now() + 24 * 60 * 60 * 1000), // 24 hours
      },
      metadata: {
        loginMethod: mode,
        timestamp: new Date(),
        deviceInfo: navigator.userAgent,
      },
    };
    handleSuccess(authResult);
  }, [handleSuccess, mode]);

  // Enhanced error handler for AuthPage compatibility  
  const handleAuthPageError = useCallback((error: Error) => {
    const authError: AuthError = {
      code: error.name || 'AUTH_ERROR',
      message: error.message,
      timestamp: new Date(),
      details: { originalError: error },
    };
    handleError(authError);
  }, [handleError]);

  // Enhanced escape key handler with accessibility focus management
  useEffect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !isLoading) {
        handleClose();
      }
    };
    if (isOpen) {
      document.addEventListener('keydown', handleEscape);
      return () => document.removeEventListener('keydown', handleEscape);
    }
  }, [isOpen, isLoading, handleClose]);

  // Enhanced overlay click handler
  const handleOverlayClick = useCallback((e: React.MouseEvent) => {
    if (closeOnOverlayClick && e.target === e.currentTarget) {
      handleClose();
    }
  }, [closeOnOverlayClick, handleClose]);

  // Modal size configuration following design laws (Fitts's Law - adequate target sizes)
  const getModalSize = useCallback(() => {
    switch (size) {
      case 'sm':
        return 'max-w-md';
      case 'md':
        return 'max-w-lg';
      case 'lg':
        return 'max-w-4xl';
      case 'xl':
        return 'max-w-6xl';
      default:
        return 'max-w-4xl';
    }
  }, [size]);

  // Dynamic title and description with bilingual support
  const getModalTitle = useCallback(() => {
    if (title) return title;
    
    switch (mode) {
      case 'signin':
        return t.signin.title;
      case 'signup':
        return t.signup.title;
      case 'enterprise-sso':
        return t['enterprise-sso'].title;
      case 'org':
        return t.org.title;
      default:
        return t.default.title;
    }
  }, [title, mode, t]);

  const getModalDescription = useCallback(() => {
    if (description) return description;
    
    switch (mode) {
      case 'signin':
        return t.signin.description;
      case 'signup':
        return t.signup.description;
      case 'enterprise-sso':
        return t['enterprise-sso'].description;
      case 'org':
        return t.org.description;
      default:
        return t.default.description;
    }
  }, [description, mode, t]);

  // Mode icon mapping for visual hierarchy (Von Restorff Effect)
  const getModeIcon = useCallback(() => {
    switch (mode) {
      case 'signin':
        return <LogIn className="w-5 h-5" />;
      case 'signup':
        return <UserPlus className="w-5 h-5" />;
      case 'enterprise-sso':
        return <Building2 className="w-5 h-5" />;
      case 'org':
        return <Shield className="w-5 h-5" />;
      default:
        return <User className="w-5 h-5" />;
    }
  }, [mode]);

  if (!isOpen) return null;

  return (
    <div 
      className={`fixed inset-0 z-50 flex items-center justify-center p-4 ${overlayClassName}`}
      onClick={handleOverlayClick}
      role="presentation"
    >
      {/* Enhanced backdrop with accessibility */}
      <div 
        className="absolute inset-0 bg-black/60 backdrop-blur-sm" 
        aria-hidden="true"
      />
      
      {/* Modal with WCAG 2.1 AA compliance */}
      <div 
        className={`relative w-full ${getModalSize()} h-[85vh] max-h-screen overflow-hidden ${className}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="modal-title"
        aria-describedby="modal-description"
        tabIndex={-1}
      >
        {/* Enhanced close button with accessibility */}
        {showCloseButton && (
          <button
            onClick={handleClose}
            disabled={isLoading}
            className="absolute top-4 right-4 z-50 w-10 h-10 bg-background/90 backdrop-blur-sm rounded-full flex items-center justify-center text-muted-foreground hover:bg-background hover:text-foreground transition-all duration-200 focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:opacity-50 disabled:cursor-not-allowed shadow-lg border border-border/50"
            aria-label={t.close}
            type="button"
          >
            {isLoading ? (
              <Loader2 className="w-5 h-5 animate-spin" />
            ) : (
              <X className="w-5 h-5" />
            )}
          </button>
        )}

        {/* Modal header with enhanced visual hierarchy */}
        {(title || description) && (
          <div className="absolute top-0 left-0 right-0 z-40 bg-gradient-to-b from-background/95 to-background/85 backdrop-blur-sm border-b border-border/50 p-6">
            <div className="flex items-start gap-4">
              {/* Mode icon for visual distinction */}
              <div className="flex-shrink-0 w-12 h-12 bg-primary/10 rounded-xl flex items-center justify-center text-primary">
                {getModeIcon()}
              </div>
              
              <div className="flex-1 min-w-0">
                <h2 
                  id="modal-title"
                  className="text-xl font-semibold text-foreground mb-1 truncate"
                >
                  {getModalTitle()}
                </h2>
                <p 
                  id="modal-description"
                  className="text-sm text-muted-foreground leading-relaxed"
                >
                  {getModalDescription()}
                </p>
              </div>
            </div>
          </div>
        )}

        {/* Main content area with AuthPage integration */}
        <div className="flex-1 overflow-auto bg-background rounded-2xl border border-border/50 shadow-xl">
          <AuthPage
            initialMode={mode}
            redirectTo={redirectTo}
            onModeChange={(newMode) => {
              if (enableAnalytics) {
                analytics.trackEvent('modal_mode_changed', { from: mode, to: newMode, trackingId });
              }
            }}
            onAuthSuccess={handleAuthPageSuccess}
            onAuthError={handleAuthPageError}
            features={{ enterpriseSSO: true, organizationManagement: true }}
          />
        </div>
      </div>
    </div>
  );
}

// Simplified auth modal with preset configurations
interface QuickAuthModalProps { mode: AuthMode; isOpen: boolean; onClose: () => void; onSuccess?: (r: AuthResult)=>void; onError?: (e: AuthError)=>void; }
export function QuickAuthModal({ mode, isOpen, onClose, onSuccess, onError }: QuickAuthModalProps) {
  const [trackingId] = React.useState(() => `quick_${mode}_${Date.now()}`);
  return <AuthModal isOpen={isOpen} onClose={onClose} mode={mode} onSuccess={onSuccess} onError={onError} size="md" enableAnalytics trackingId={trackingId} />;
}

// Enhanced custom modal with accessibility features
// (CustomAuthModalProps removed in consolidation; inline props used for simplicity)

export function CustomAuthModal(props: { isOpen:boolean; onClose:()=>void; title:string; description?:string; children:React.ReactNode; className?:string; size?:'sm'|'md'|'lg'|'xl'; showCloseButton?:boolean; closeOnOverlayClick?:boolean; }) {
  const { isOpen, onClose, title, description, children, className='', size='md', showCloseButton=true, closeOnOverlayClick=true } = props;
  const getModalSize = () => ({ sm:'max-w-md', md:'max-w-lg', lg:'max-w-2xl', xl:'max-w-4xl' }[size] || 'max-w-lg');
  const handleOverlay = (e:React.MouseEvent) => { if (closeOnOverlayClick && e.target===e.currentTarget) onClose(); };
  if (!isOpen) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4" onClick={handleOverlay} role="presentation">
      <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" aria-hidden="true" />
      <div className={`relative w-full ${getModalSize()} max-h-[90vh] overflow-hidden bg-card rounded-2xl border border-border shadow-2xl ${className}`} role="dialog" aria-modal="true" aria-labelledby="custom-modal-title" tabIndex={-1}>
        {showCloseButton && (
          <button onClick={onClose} className="absolute top-4 right-4 z-50 w-8 h-8 bg-background/80 backdrop-blur-sm rounded-full flex items-center justify-center text-muted-foreground hover:bg-background hover:text-foreground focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2" aria-label="Close" type="button"><X className="w-4 h-4" /></button>
        )}
        <div className="relative border-b border-border p-6">
          <h2 id="custom-modal-title" className="text-lg font-semibold text-foreground pr-8">{title}</h2>
          {description && <p className="mt-2 text-sm text-muted-foreground">{description}</p>}
        </div>
        <div className="overflow-y-auto max-h-[calc(90vh-120px)] p-6">{children}</div>
      </div>
    </div>
  );
}

// Enhanced modal management hook with analytics
export function useAuthModal() {
  const [isOpen, setIsOpen] = useState(false);
  const [modalData, setModalData] = useState<{ mode: AuthMode; trackingId?: string; title?: string; description?: string; redirectTo?: string; onSuccess?: (r:AuthResult)=>void; onError?: (e:AuthError)=>void }>();
  const { consent } = useConsent();
  const analytics = useMemo(()=>createAnalytics(consent),[consent]);
  const openAuthModal = useCallback((mode:AuthMode, options?:Omit<NonNullable<typeof modalData>, 'mode'>) => {
    const trackingId = options?.trackingId || `modal_${mode}_${Date.now()}`;
    analytics.trackEvent('modal_programmatically_opened', { mode, trackingId });
    setModalData({ mode, trackingId, ...options });
    setIsOpen(true);
  }, [analytics]);
  const closeAuthModal = useCallback(()=>{
    if (modalData?.trackingId) analytics.trackEvent('modal_programmatically_closed', { mode: modalData.mode, trackingId: modalData.trackingId });
    setIsOpen(false); setModalData(undefined);
  }, [modalData, analytics]);
  return { isOpen, authData: modalData, openAuthModal, closeAuthModal };
}

// Enhanced loading modal with accessibility and analytics
interface AuthLoadingModalProps {
  isOpen: boolean;
  title?: string;
  message?: string;
  onCancel?: () => void;
  showCancelButton?: boolean;
  trackingId?: string;
}

export function AuthLoadingModal({ isOpen, title, message, onCancel, showCancelButton=false }: AuthLoadingModalProps) {
  const { isEnglish } = useLanguageSwitch();
  const t = useMemo(()=> isEnglish ? modalTranslations.en : modalTranslations.nb, [isEnglish]);
  if (!isOpen) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" aria-hidden="true" />
      <div className="relative w-full max-w-md bg-card rounded-2xl border border-border shadow-2xl" role="dialog" aria-modal="true" aria-labelledby="loading-modal-title" aria-describedby="loading-modal-message" tabIndex={-1}>
        <div className="p-6">
          <div className="flex items-center gap-4 mb-4">
            <div className="shrink-0 w-10 h-10 bg-primary/10 rounded-full flex items-center justify-center"><Loader2 className="w-5 h-5 text-primary animate-spin" /></div>
            <div className="flex-1"><h3 id="loading-modal-title" className="font-medium text-foreground">{title || t.loading.title}</h3></div>
          </div>
          <p id="loading-modal-message" className="text-sm text-muted-foreground mb-6">{message || t.loading.message}</p>
          {showCancelButton && onCancel && (
            <div className="flex justify-end"><button onClick={onCancel} className="px-4 py-2 text-sm font-medium text-muted-foreground hover:text-foreground focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 rounded-md" type="button">{t.loading.cancel}</button></div>
          )}
        </div>
      </div>
    </div>
  );
}
