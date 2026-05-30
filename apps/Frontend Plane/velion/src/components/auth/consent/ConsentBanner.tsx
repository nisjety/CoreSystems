import React from 'react';
import { ListCheck } from 'lucide-react';
import { useConsentTranslation } from '../lib/i18n/hooks';
import { useIsHydrated } from '../lib/hydration/HydrationGuard';

// Enhanced features - fallback implementations when providers not available
const defaultToastActions = {
  success: (message: string) => console.log('✅ Consent Success:', message),
  info: (message: string) => console.log('ℹ️ Consent Info:', message),
  warning: (message: string) => console.warn('⚠️ Consent Warning:', message),
};

interface ConsentBannerProps {
  onAcceptAll: () => void;
  onRejectAll: () => void;
  onShowPreferences: () => void;
  className?: string;
  isLoading?: boolean;
  embedded?: boolean;
}

export function ConsentBanner({ 
  onAcceptAll, 
  onRejectAll, 
  onShowPreferences, 
  className = '',
  isLoading = false,
  embedded = false,
}: ConsentBannerProps) {
  // Enhanced features - using i18n system with SSR safety
  const { success, info } = defaultToastActions;
  const { consentT } = useConsentTranslation();
  const isHydrated = useIsHydrated();

  // Enhanced action handlers with feedback and analytics
  const handleAcceptAll = async () => {
    try {
      await onAcceptAll();
      success(isHydrated ? consentT.action('accepted') : 'All cookies accepted');
    } catch (error) {
      console.error('Error accepting all consent:', error);
    }
  };

  const handleRejectAll = async () => {
    try {
      await onRejectAll();
      info(isHydrated ? consentT.action('rejected') : 'Non-essential cookies rejected');
    } catch (error) {
      console.error('Error rejecting consent:', error);
    }
  };

  const handleShowPreferences = () => {
    onShowPreferences();
    info(isHydrated ? consentT.action('settingsOpened') : 'Preferences opened');
  };
  return (
    <>
      {/* Desktop Banner - Exact pill design from reference */}
      <div className={`hidden lg:block absolute bottom-6 left-1/2 -translate-x-1/2 w-[92%] max-w-none z-20 ${className}`}>
        <div className="rounded-full bg-background shadow-lg px-3 py-2">
          <div className="flex items-center gap-3">
            {/* Cookie Icon Circle */}
            <div className="w-9 h-9 rounded-full bg-muted flex items-center justify-center text-foreground/70 shrink-0">
              <svg viewBox="0 0 24 24" className="w-[18px] h-[18px]" fill="currentColor" aria-hidden="true">
                <path d="M12 2a10 10 0 1010 10c0-.34-.02-.67-.06-.99a3 3 0 01-3.44-3.44C18.67 7.02 18.34 7 18 7a6 6 0 01-6-6zM8 12a1 1 0 110-2 1 1 0 010 2zm2 4a1 1 0 110-2 1 1 0 010 2zm5-3a1 1 0 110-2 1 1 0 010 2z"/>
              </svg>
            </div>
            
            {/* Norwegian Text */}
            <p className="flex-1 text-sm leading-tight text-foreground/90 min-w-0">
              {isHydrated ? consentT.banner('message') : 'We use cookies to enhance your experience. Please accept our cookie policy.'}
            </p>
            
            {/* Right side: Settings + Buttons */}
            <div className="flex items-center gap-2 shrink-0">
              {/* Settings button */}
              <button
                onClick={handleShowPreferences}
                disabled={isLoading}
                className="w-9 h-9 rounded-full bg-background ring-1 ring-border hover:bg-muted transition focus:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
                aria-label={isHydrated ? consentT.banner('settingsLabel') : 'Cookie settings'}
                title={isHydrated ? consentT.banner('settings') : 'Settings'}
              >
                <ListCheck className="mx-auto w-[18px] h-[18px] text-foreground/70" />
              </button>
              
              {/* Button group with exact oval styling */}
              <div className="inline-flex gap-2">
                <button
                  onClick={handleRejectAll}
                  disabled={isLoading}
                  className="h-9 px-4 bg-background text-foreground hover:bg-muted transition focus:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded-tl-[30px] rounded-tr-[10px] rounded-br-[10px] rounded-bl-[30px] ring-1 ring-border disabled:opacity-50"
                >
                  {isLoading ? '...' : (isHydrated ? consentT.banner('reject') : 'Reject')}
                </button>
                <button
                  onClick={handleAcceptAll}
                  disabled={isLoading}
                  className="h-9 px-5 bg-primary text-primary-foreground font-medium hover:bg-primary/90 transition focus:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded-tl-[10px] rounded-tr-[30px] rounded-br-[30px] rounded-bl-[10px] disabled:opacity-50"
                >
                  {isLoading ? '...' : (isHydrated ? consentT.banner('accept') : 'Accept All')}
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Mobile Banner - Simplified design */}
      <div className={`lg:hidden inset-x-4 ${embedded ? 'absolute bottom-4 z-20' : 'fixed bottom-4 z-50'}`}>
        <div className="rounded-full bg-card shadow-xl px-4 py-2 flex items-center gap-3">
          <div className="w-7 h-7 rounded-full bg-muted flex items-center justify-center text-foreground/70">
            <svg viewBox="0 0 24 24" className="w-4 h-4" fill="currentColor" aria-hidden="true">
              <path d="M12 2a10 10 0 1010 10c0-.34-.02-.67-.06-.99a3 3 0 01-3.44-3.44C18.67 7.02 18.34 7 18 7a6 6 0 01-6-6zM8 12a1 1 0 110-2 1 1 0 010 2zm2 4a1 1 0 110-2 1 1 0 010 2zm5-3a1 1 0 110-2 1 1 0 010 2z"/>
            </svg>
          </div>
          <p className="text-sm text-foreground/80 flex-1">{isHydrated ? consentT.banner('messageMobile') : 'We use cookies. Please accept.'}</p>
          <button 
            onClick={handleShowPreferences} 
            disabled={isLoading}
            className="inline-flex items-center text-sm bg-muted px-3 py-1.5 rounded-full text-foreground hover:opacity-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
          >
            {isHydrated ? consentT.banner('settings') : 'Settings'}
          </button>
          <button 
            onClick={handleRejectAll} 
            disabled={isLoading}
            className="bg-muted text-foreground px-3 py-1.5 rounded-full hover:opacity-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
          >
            {isLoading ? '...' : (isHydrated ? consentT.banner('reject') : 'Reject')}
          </button>
          <button 
            onClick={handleAcceptAll} 
            disabled={isLoading}
            className="bg-primary text-primary-foreground px-3 py-1.5 rounded-full hover:bg-primary/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
          >
            {isLoading ? '...' : (isHydrated ? consentT.banner('accept') : 'Accept All')}
          </button>
        </div>
      </div>
    </>
  );
}