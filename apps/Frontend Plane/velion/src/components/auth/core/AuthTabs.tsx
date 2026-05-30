import React, { useState, useEffect, useCallback, useMemo } from 'react';
// import { useMutation } from '@tanstack/react-query';
import { toast } from 'sonner';
import type { AuthMode } from '../types/auth';
import { useCommonTranslation, useLanguageSwitch, useAuthTranslation } from '../lib/i18n/hooks';
import { useIsHydrated } from '../lib/hydration/HydrationGuard';
import { Loader2, UserPlus, LogIn, Building, Users } from 'lucide-react';

// Enhanced loading state management with type safety
interface TabLoadingState {
  changingMode: boolean;
  activeTab: AuthMode | null;
  lastChangeTime: number;
  error: string | null;
}

const defaultTabLoadingState: TabLoadingState = {
  changingMode: false,
  activeTab: null,
  lastChangeTime: 0,
  error: null,
};

interface AuthTabsProps {
  currentMode: AuthMode;
  onModeChange: (mode: AuthMode) => void;
  disabled?: boolean;
  isLoading?: boolean;
  features?: {
    enterpriseSSO?: boolean;
    organizationManagement?: boolean;
  };
  className?: string;
}

export function AuthTabs({ 
  currentMode, 
  onModeChange, 
  disabled = false,
  isLoading = false,
  features = {
    enterpriseSSO: true,
    organizationManagement: true
  },
  className = '',
}: AuthTabsProps) {
  const [tabLoadingState, setTabLoadingState] = useState<TabLoadingState>(defaultTabLoadingState);
  const isHydrated = useIsHydrated();
  const { isEnglish } = useLanguageSwitch();
  const { t } = useAuthTranslation();

  // Helper function to get the appropriate label based on locale
  const getLabel = (mode: AuthMode, isShort = false): string => (isShort ? labelMap[mode].short : labelMap[mode].full);

  const getDescription = (mode: AuthMode): string => {
    return descriptionMap[mode];
  };
  
  // Use the comprehensive i18n system
  const { commonT } = useCommonTranslation();

  // Type-safe tab configuration with custom Norwegian/English labels
  const labelMap = useMemo(() => ({
    signin: {
      full: t('auth.tabs.signin.label'),
      short: t('auth.tabs.signin.shortLabel')
    },
    signup: {
      full: t('auth.tabs.signup.label'),
      short: t('auth.tabs.signup.shortLabel')
    },
    'enterprise-sso': {
      full: t('auth.tabs.enterpriseSso.shortLabel'),
      short: t('auth.tabs.enterpriseSso.shortLabel')
    },
    org: {
      full: t('auth.tabs.org.label'),
      short: t('auth.tabs.org.shortLabel')
    }
  }), [t]);

  const descriptionMap = useMemo(() => ({
    signin: t('auth.tabs.signin.description'),
    signup: t('auth.tabs.signup.description'),
    'enterprise-sso': t('auth.tabs.enterpriseSso.description'),
    org: t('auth.tabs.org.description')
  }), [t]);

  const authModes = useMemo(() => [
    { key: 'signin' as const, icon: LogIn },
    { key: 'signup' as const, icon: UserPlus },
    { key: 'enterprise-sso' as const, icon: Building },
    { key: 'org' as const, icon: Users },
  ], []);

  // Simple mode change handler without TanStack Query for testing
  const handleModeChange = useCallback(async (newMode: AuthMode) => {
    setTabLoadingState({
      changingMode: true,
      activeTab: newMode,
      lastChangeTime: Date.now(),
      error: null,
    });

    const modeInfo = labelMap[newMode];
    toast.loading(
      isHydrated ? commonT.loading() : 'Laster...',
      {
        description: modeInfo?.full,
        id: `mode-change-${newMode}`,
      }
    );

    try {
      // Simulate API call delay for demonstration
      await new Promise(resolve => setTimeout(resolve, 300));
      
      // Here you would make the actual ORPC call
      // const result = await orpcClient.auth.changeMode({ mode: newMode });
      
      setTabLoadingState(prev => ({
        ...prev,
        changingMode: false,
        activeTab: null,
      }));
      
      toast.success(
        isHydrated ? commonT.success() : 'Vellykket',
        {
          description: `${isHydrated ? (isEnglish ? 'Switched to' : 'Byttet til') : (isEnglish ? 'Switched to' : 'Byttet til')} ${modeInfo?.full}`,
          id: `mode-change-${newMode}`,
        }
      );

      onModeChange(newMode);
    } catch (error) {
      setTabLoadingState(prev => ({
        ...prev,
        changingMode: false,
        activeTab: null,
        error: error instanceof Error ? error.message : 'Unknown error',
      }));

      toast.error(
        isHydrated ? commonT.error() : 'Feil oppstod',
        {
          description: isHydrated ? 'Kunne ikke bytte fane' : 'Kunne ikke bytte fane',
          id: `mode-change-${newMode}`,
        }
      );
    }
  }, [onModeChange, isHydrated, commonT, isEnglish, labelMap]);

  // Enhanced tab change handler with proper error handling
  const handleModeChangeClick = useCallback((mode: AuthMode) => {
    if (disabled || isLoading || tabLoadingState.changingMode || mode === currentMode) {
      return;
    }
    
    try {
      handleModeChange(mode);
    } catch (error) {
      console.error('Error initiating mode change:', error);
      toast.error(
        isHydrated ? commonT.error() : 'Feil oppstod',
        {
          description: isHydrated ? 'Kunne ikke starte modebytte' : 'Kunne ikke starte modebytte',
        }
      );
    }
  }, [disabled, isLoading, tabLoadingState.changingMode, currentMode, handleModeChange, isHydrated, commonT]);

  // Auto-clear loading state if it gets stuck
  useEffect(() => {
    if (tabLoadingState.changingMode) {
      const timeout = setTimeout(() => {
        setTabLoadingState(prev => ({
          ...prev,
          changingMode: false,
          activeTab: null,
          error: 'Timeout - mode change took too long',
        }));
        
        toast.error(
          isHydrated ? commonT.error() : 'Feil oppstod',
          {
            description: isHydrated ? 'Modebytte tok for lang tid' : 'Modebytte tok for lang tid',
          }
        );
      }, 10000); // 10 second timeout

      return () => clearTimeout(timeout);
    }
  }, [tabLoadingState.changingMode, isHydrated, commonT]);

  // Filter tabs based on enabled features and current context
  const availableTabs = useMemo(() => {
    return authModes.filter(tab => {
      // Always show basic auth modes
      if (['signin', 'signup'].includes(tab.key)) return true;
      
      // Conditional features
      if (tab.key === 'enterprise-sso' && !features.enterpriseSSO) return false;
      if (tab.key === 'org' && !features.organizationManagement) return false;
      
      return true;
    });
  }, [authModes, features.enterpriseSSO, features.organizationManagement]);

  // Keyboard navigation support
  const handleKeyDown = useCallback((event: React.KeyboardEvent, mode: AuthMode) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      handleModeChange(mode);
    }
  }, [handleModeChange]);

  if (!isHydrated) {
    // SSR-safe loading state
    return (
  <nav className={`mb-6 sm:mb-8 ${className}`} role="tablist" aria-label={t('auth.navigation.authTabsLabel' as string)}>
        <div className="inline-flex bg-muted rounded-full p-1 shadow-inner animate-pulse">
          <div className="h-8 w-20 bg-muted-foreground/20 rounded-full"></div>
          <div className="h-8 w-24 bg-muted-foreground/20 rounded-full ml-1"></div>
        </div>
      </nav>
    );
  }

  return (
  <nav className={`mb-6 sm:mb-8 mt-0.5 ${className}`} role="tablist" aria-label={t('auth.navigation.authTabsLabel' as string)}>
      <div className="inline-flex bg-muted rounded-full p-1 shadow-inner">
        {availableTabs.map(({ key, icon: Icon }) => {
          const isActive = currentMode === key;
          const isTabLoading = tabLoadingState.changingMode && tabLoadingState.activeTab === key;
          const showLoading = (isLoading && isActive) || isTabLoading;

          return (
            <button
              key={key}
              role="tab"
              aria-selected={isActive}
              aria-describedby={`tab-desc-${key}`}
              onClick={() => handleModeChange(key)}
              onKeyDown={(e) => handleKeyDown(e, key)}
              disabled={disabled || isLoading || tabLoadingState.changingMode}
              title={getDescription(key as AuthMode)}
              className={`
                group relative px-2 sm:px-2 py-1.5 rounded-full text-xs sm:text-sm font-small transition-all duration-200 
                focus:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2
                disabled:opacity-50 disabled:cursor-not-allowed flex-shrink-0
                ${isActive 
                  ? 'bg-background shadow text-foreground ring-1 ring-border' 
                  : 'text-muted-foreground hover:text-foreground hover:bg-background/50 hover:shadow-sm'
                }
                ${isTabLoading ? 'animate-pulse' : ''}
                ${tabLoadingState.error && tabLoadingState.activeTab === key ? 'ring-2 ring-destructive' : ''}
              `}
            >
              <div className="flex items-center gap-1 justify-center">
                {/* Icon with loading state */}
                {showLoading ? (
                  <Loader2 className="w-3 h-3 animate-spin flex-shrink-0" />
                ) : (
                  <Icon className={`w-3 h-3 transition-transform flex-shrink-0 ${isActive ? 'scale-110' : 'group-hover:scale-105'}`} />
                )}
                
                {/* Label with responsive text - always show short labels to prevent cutoff */}
                <span className="whitespace-nowrap">
                  {showLoading 
                    ? '...'
                    : getLabel(key as AuthMode, true)
                  }
                </span>
              </div>
              
              {/* Hidden description for screen readers */}
              <span id={`tab-desc-${key}`} className="sr-only">
                {getDescription(key as AuthMode)}
                {isActive && `, ${t('common.selected' as string, 'selected')}`}
                {showLoading && `, ${commonT.loading()}`}
              </span>

              {/* Loading indicator */}
              {showLoading && (
                <div className="absolute inset-0 bg-background/50 rounded-full flex items-center justify-center">
                  <Loader2 className="w-3 h-3 animate-spin" />
                </div>
              )}
            </button>
          );
        })}
      </div>

      {/* Error state display */}
      {tabLoadingState.error && (
        <div className="mt-2 text-sm text-destructive text-center" role="alert">
          {tabLoadingState.error}
        </div>
      )}
    </nav>
  );
}