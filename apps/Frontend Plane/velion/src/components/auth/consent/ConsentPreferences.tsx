import React, { useState } from 'react';
import { X, Plus, Minus, Loader2 } from 'lucide-react';
import { ConsentState } from './useConsent';
import { useConsentTranslation } from '../lib/i18n/hooks';
import { useIsHydrated } from '../lib/hydration/HydrationGuard';

// Enhanced features - fallback implementations when providers not available
const defaultToastActions = {
  success: (title: string, description?: string) => console.log('✅ Consent Success:', title, description),
  info: (title: string, description?: string) => console.log('ℹ️ Consent Info:', title, description),
  error: (title: string, description?: string) => console.error('❌ Consent Error:', title, description),
};

interface ConsentPreferencesProps {
  isOpen: boolean;
  onClose: () => void;
  consent: ConsentState;
  onConsentChange: (key: keyof ConsentState, value: boolean) => void;
  onSave: () => void;
  onAcceptAll: () => void;
  onRejectAll: () => void;
  isLoading?: boolean;
}

export function ConsentPreferences({
  isOpen,
  onClose,
  consent,
  onConsentChange,
  onSave,
  onAcceptAll,
  onRejectAll,
  isLoading = false,
}: ConsentPreferencesProps) {
  const [openKey, setOpenKey] = useState<keyof ConsentState | null>('performance');
  
  // Enhanced features - using i18n system with SSR safety
  const { consentT } = useConsentTranslation();
  const isHydrated = useIsHydrated();

  // Enhanced action handlers with toast feedback and analytics
  const handleSave = async () => {
    try {
      await onSave();
      defaultToastActions.success(
        isHydrated ? consentT.action('choicesSaved') : 'Choices saved'
      );
    } catch (error) {
      console.error('Error saving consent choices:', error);
      defaultToastActions.error('Error saving choices');
    }
  };

  const handleAcceptAll = async () => {
    try {
      await onAcceptAll();
      defaultToastActions.success(
        isHydrated ? consentT.action('allAccepted') : 'All cookies accepted'
      );
    } catch (error) {
      console.error('Error accepting all consents:', error);
      defaultToastActions.error('Error accepting all');
    }
  };

  const handleRejectAll = async () => {
    try {
      await onRejectAll();
      defaultToastActions.info(
        isHydrated ? consentT.action('allRejected') : 'Non-essential cookies rejected'
      );
    } catch (error) {
      console.error('Error rejecting all consents:', error);
      defaultToastActions.error('Error rejecting all');
    }
  };

  if (!isOpen) return null;

  // Enhanced consent categories with enterprise types
  const consentCategories = [
    {
      key: 'necessary' as keyof ConsentState,
      title: isHydrated ? consentT.category('necessary', 'title') : 'Necessary Cookies',
      description: isHydrated ? consentT.category('necessary', 'description') : 'Required for basic website functionality',
      disabled: true
    },
    {
      key: 'performance' as keyof ConsentState,
      title: isHydrated ? consentT.category('performance', 'title') : 'Performance Cookies',
      description: isHydrated ? consentT.category('performance', 'description') : 'Help us improve website performance',
      disabled: false
    },
    {
      key: 'functional' as keyof ConsentState,
      title: isHydrated ? consentT.category('functional', 'title') : 'Functional Cookies',
      description: isHydrated ? consentT.category('functional', 'description') : 'Enable enhanced functionality and personalization',
      disabled: false
    },
    {
      key: 'marketing' as keyof ConsentState,
      title: isHydrated ? consentT.category('marketing', 'title') : 'Marketing Cookies',
      description: isHydrated ? consentT.category('marketing', 'description') : 'Used for targeted advertising and marketing',
      disabled: false
    },
    {
      key: 'analytics' as keyof ConsentState,
      title: 'Analytics Cookies',
      description: 'Help us understand how you use our website',
      disabled: false
    },
    {
      key: 'social' as keyof ConsentState,
      title: 'Social Media Cookies', 
      description: 'Enable social media sharing and integration',
      disabled: false
    }
  ];

  return (
    <div 
      role="dialog" 
      aria-modal="true" 
      aria-labelledby="prefs-title" 
      className="fixed inset-0 z-[300] flex items-center justify-center"
    >
      <div 
        className="absolute inset-0 bg-background/60 backdrop-blur-sm" 
        onClick={onClose}
        role="button"
        tabIndex={0}
        aria-label="Close"
        onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') onClose(); }}
      />
      <div className="relative bg-card text-card-foreground w-[92vw] max-w-xl rounded-2xl  shadow-2xl max-h-[90svh] overflow-y-auto">
        
        {/* Header: X left, title centered */}
        <div className="grid grid-cols-3 items-center p-4  sticky top-0 bg-card">
          <button
            onClick={onClose}
            disabled={isLoading}
            className="justify-self-start w-8 h-8 rounded-full bg-muted text-foreground/80 flex items-center justify-center hover:opacity-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
            aria-label={isHydrated ? consentT.preferences('close') : 'Close'}
          >
            <X className="w-4 h-4" />
          </button>
          <h2 id="prefs-title" className="justify-self-center text-sm font-medium">
            {isHydrated ? consentT.preferences('title') : 'Cookie Preferences'}
          </h2>
          <span className="justify-self-end w-8 h-8" aria-hidden="true" />
        </div>

        {/* Intro + large "Allow All" button */}
        <div className="p-4 border-b border-border">
          <p className="text-sm text-muted-foreground">
            {isHydrated ? consentT.preferences('intro') : 'Manage your cookie preferences for this website.'}
          </p>
          <button
            onClick={handleAcceptAll}
            disabled={isLoading}
            className="mt-4 w-full h-10 rounded-full bg-primary text-primary-foreground font-medium hover:bg-primary/90 transition focus:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {isLoading ? (
              <div className="flex items-center justify-center gap-2">
                <Loader2 size={16} className="animate-spin" />
                <span>Processing...</span>
              </div>
            ) : (
              isHydrated ? consentT.preferences('allowAll') : 'Allow All'
            )}
          </button>
        </div>

        {/* Expandable sections */}
        <div className="p-4 space-y-3">
          {consentCategories.map(({ key, title, description, disabled }) => {
            const isOpen = openKey === key;
            return (
              <div key={key} className="rounded-xl border border-border bg-background">
                <div 
                  className="flex items-center justify-between p-3 cursor-pointer hover:bg-muted/30 transition-colors"
                  onClick={() => setOpenKey(isOpen ? null : key)}
                  role="button"
                  tabIndex={0}
                  onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') setOpenKey(isOpen ? null : key); }}
                >
                  <div className="flex items-center gap-3">
                    <button
                      type="button"
                      disabled={isLoading}
                      className="w-7 h-7 rounded-full bg-muted text-foreground/80 flex items-center justify-center hover:opacity-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring pointer-events-none disabled:opacity-50"
                      aria-label={isOpen ? 
                        (isHydrated ? consentT.preferences('hideDetails') : 'Hide details') : 
                        (isHydrated ? consentT.preferences('showDetails') : 'Show details')
                      }
                      aria-expanded={isOpen}
                      tabIndex={-1}
                    >
                      {isOpen ? <Minus className="w-4 h-4" /> : <Plus className="w-4 h-4" />}
                    </button>
                    <p className="text-sm font-medium">{title}</p>
                  </div>

                  {/* Enhanced toggle switch with accessibility */}
                  <label 
                    aria-label={title}
                    className={`inline-flex items-center ${disabled || isLoading ? 'opacity-60 cursor-not-allowed' : 'cursor-pointer'}`}
                    onClick={(e) => e.stopPropagation()}
                  >
                    <input
                      type="checkbox"
                      className="sr-only peer"
                      checked={consent[key]}
                      disabled={disabled || isLoading}
                      onChange={(e) => onConsentChange(key, e.target.checked)}
                      aria-checked={consent[key]}
                      aria-describedby={`${key}-description`}
                    />
                    <span
                      className={`
                        w-11 h-6 rounded-full ring-1 ring-border relative transition-colors
                        after:content-[''] after:absolute after:top-0.5 after:left-0.5 after:w-5 after:h-5 after:rounded-full after:bg-background after:transition-transform
                        ${consent[key] 
                          ? 'bg-primary after:translate-x-5' 
                          : 'bg-muted after:translate-x-0'
                        }
                      `}
                      aria-hidden="true"
                    />
                  </label>
                </div>

                {isOpen && (
                  <div className="px-3 pb-3 pt-0">
                    <p id={`${key}-description`} className="text-xs text-muted-foreground leading-relaxed">
                      {description}
                    </p>
                  </div>
                )}
              </div>
            );
          })}
        </div>

        {/* Enhanced footer with styled buttons */}
        <div className="p-4 border-t border-border bg-card sticky bottom-0">
          <div className="flex gap-2 justify-end">
            <button
              onClick={handleRejectAll}
              disabled={isLoading}
              className="px-4 py-2 rounded-tl-[30px] rounded-tr-[10px] rounded-br-[10px] rounded-bl-[30px] bg-muted text-foreground hover:opacity-90 transition focus:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {isLoading ? (
                <div className="flex items-center gap-2">
                  <Loader2 size={16} className="animate-spin" />
                  <span>Processing...</span>
                </div>
              ) : (
                isHydrated ? consentT.preferences('rejectAll') : 'Reject All'
              )}
            </button>
            <button
              onClick={handleAcceptAll}
              disabled={isLoading}
              className="px-4 py-2 rounded-tl-[5px] rounded-tr-[5px] rounded-br-[5px] rounded-bl-[5px] bg-primary text-primary-foreground hover:bg-primary/90 transition focus:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {isLoading ? (
                <div className="flex items-center gap-2">
                  <Loader2 size={16} className="animate-spin" />
                  <span>Processing...</span>
                </div>
              ) : (
                isHydrated ? consentT.preferences('acceptAll') : 'Accept All'
              )}
            </button>
            <button
              onClick={handleSave}
              disabled={isLoading}
              className="px-4 py-2 rounded-tl-[10px] rounded-tr-[30px] rounded-br-[30px] rounded-bl-[10px] bg-primary text-primary-foreground hover:bg-primary/90 transition focus:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {isLoading ? (
                <div className="flex items-center gap-2">
                  <Loader2 size={16} className="animate-spin" />
                  <span>Saving...</span>
                </div>
              ) : (
                isHydrated ? consentT.preferences('saveChoices') : 'Save Choices'
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
