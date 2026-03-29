'use client';

import React, { useState, useEffect, useRef } from 'react';
import { Volume2, ChevronDown, ChevronUp, ChevronRight } from 'lucide-react';
import { SecurityInfo } from '../verification/SecurityInfo';
import { SecurityStats } from '../verification/SecurityStats';
import { ConsentBanner } from '../consent/ConsentBanner';
import { useLanguageSwitch } from '../lib/i18n/hooks';

interface AuthIllustrationProps {
  className?: string;
  companyName?: string;
  showConsentBanner?: boolean;
  onAcceptAll?: () => void;
  onRejectAll?: () => void;
  onShowPreferences?: () => void;
  language?: 'no' | 'en';
}

export function AuthIllustration({
  className = '',
  companyName = 'ID-Knuten',
  showConsentBanner = false,
  onAcceptAll,
  onRejectAll,
  onShowPreferences,
  language
}: AuthIllustrationProps) {
  // Use i18n hook for language detection
  const { isNorwegian } = useLanguageSwitch();
  const currentLanguage = language || (isNorwegian ? 'no' : 'en');

  const [showSecurityInfo, setShowSecurityInfo] = useState(false);
  const [showSecurityStats, setShowSecurityStats] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  // Localized texts
  const texts = {
    no: {
      securityInfo: 'Sikkerhetsinformasjon',
      soundToggle: 'Lyd av/på',
      secureLogin: 'Sikker innlogging',
      protectedWithEncryption: 'Beskyttet med moderne kryptering',
      showSecurityInfo: 'Vis sikkerhetsinformasjon',
      showSecurityStats: 'Vis sikkerhetsstatistikk',
      securityStats: 'Sikkerhetsstatistikk',
    },
    en: {
      securityInfo: 'Security Information',
      soundToggle: 'Sound on/off',
      secureLogin: 'Secure Login',
      protectedWithEncryption: 'Protected with modern encryption',
      showSecurityInfo: 'Show security information',
      showSecurityStats: 'Show security statistics',
      securityStats: 'Security Statistics',
    },
  };

  const t = texts[currentLanguage];

  // Lukker panelene ved klikk utenfor komponenten
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setShowSecurityInfo(false);
        setShowSecurityStats(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  // Lukker panelene ved Escape
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setShowSecurityInfo(false);
        setShowSecurityStats(false);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  return (
    <div
      ref={containerRef}
      className={`
        aspect-square 
        w-full h-full max-w-full max-h-full
        md:w-[88%] md:h-[102%] md:left-12
        lg:w-[90%] lg:h-[100%] lg:left-8
        xl:w-[88%] xl:h-[102%] xl:left-12
        bg-card border border-border 
        rounded-lg md:rounded-xl lg:rounded-2xl 
        shadow-md md:shadow-lg lg:shadow-xl 
        overflow-hidden relative 
        ${className}
      `}
    >
      {/* Panel: Sikkerhetsinformasjon (øverst til venstre – full bredde) */}
      {showSecurityInfo && (
        <div
          className="
            absolute 
            top-12 sm:top-16 md:top-20 
            left-2 sm:left-3 md:left-4 
            right-2 sm:right-3 md:right-3 
            z-40
            bg-background/95 backdrop-blur-sm 
            rounded-md md:rounded-lg 
            shadow-lg 
            p-2 sm:p-3 md:p-4 
            max-h-64 sm:max-h-80 md:max-h-96 
            overflow-y-auto
          "
          role="dialog"
          aria-label={t.securityInfo}
          onClick={(e) => e.stopPropagation()}
        >
          <SecurityInfo />
        </div>
      )}

      {/* Lydknapp */}
      <button
        className="absolute top-1 sm:top-2 md:top-3 right-1 sm:right-2 md:right-3 z-30 w-6 h-6 sm:w-7 sm:h-7 md:w-8 md:h-8 bg-muted/80 backdrop-blur-sm rounded-full flex items-center justify-center text-muted-foreground hover:bg-muted transition-colors focus:outline-none focus:ring-2 focus:ring-ring"
        aria-label={t.soundToggle}
      >
        <Volume2 className="w-3 h-3 sm:w-3.5 sm:h-3.5 md:w-4 md:h-4" />
      </button>

      {/* Illustrasjonsflate */}
      <div 
        className="w-full h-full relative bg-gradient-to-br from-muted/30 to-muted/50"
        onClick={() => {
          setShowSecurityInfo(false);
          setShowSecurityStats(false);
        }}
      >
        {/* Sentralt ikon og tekst */}
        <div className="absolute inset-0 flex items-center justify-center">
          <div className="text-center text-muted-foreground">
            <div
              className="
                w-12 h-12 sm:w-16 sm:h-16 md:w-20 md:h-20 
                bg-muted rounded-full 
                flex items-center justify-center 
                mb-2 sm:mb-2.5 md:mb-3 
                mx-auto
              "
            >
              <svg
                className="w-6 h-6 sm:w-8 sm:h-8 md:w-10 md:h-10 text-primary"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={1.5}
                  d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z"
                />
              </svg>
            </div>
            <p className="text-xs sm:text-sm font-medium">{t.secureLogin}</p>
            <p className="text-xs opacity-70 mt-0.5 sm:mt-1 hidden sm:block">{t.protectedWithEncryption}</p>
          </div>
        </div>

        {/* Knapp: Sikkerhetsinformasjon (øverst til venstre) */}
        <button
          onClick={(e) => {
            e.stopPropagation();
            setShowSecurityInfo((v) => !v);
            setShowSecurityStats(false);
          }}
          className="
            absolute 
            top-2 sm:top-3 md:top-4 
            left-2 sm:left-3 md:left-4 
            z-30
            w-8 h-8 sm:w-10 sm:h-10 md:w-12 md:h-12 
            bg-primary/10 
            rounded-md sm:rounded-lg 
            flex items-center justify-center 
            hover:bg-primary/20 transition-colors 
            focus:outline-none focus:ring-2 focus:ring-ring 
            group
          "
          aria-label={t.showSecurityInfo}
          aria-expanded={showSecurityInfo}
          title={`${companyName} Info`}
        >
          <svg
            className="w-4 h-4 sm:w-5 sm:h-5 md:w-6 md:h-6 text-primary group-hover:scale-110 transition-transform"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
          </svg>
          {/* Indikatorprikk */}
          <div
            className={`
              absolute -bottom-0.5 -right-0.5 sm:-bottom-1 sm:-right-1 
              w-2 h-2 sm:w-2.5 sm:h-2.5 md:w-3 md:h-3 
              rounded-full bg-primary/80 
              flex items-center justify-center 
              transition-transform
            `}
          >
            {showSecurityInfo ? (
              <ChevronUp className="w-1 h-1 sm:w-1.5 sm:h-1.5 md:w-2 md:h-2 text-white" />
            ) : (
              <ChevronDown className="w-1 h-1 sm:w-1.5 sm:h-1.5 md:w-2 md:h-2 text-white" />
            )}
          </div>
        </button>

        {/* Liten godkjent-markør (øverst til høyre) */}
        <div
          className="
            absolute 
            top-10 sm:top-12 md:top-16 
            right-4 sm:right-6 md:right-8 
            w-6 h-6 sm:w-8 sm:h-8 md:w-10 md:h-10 
            bg-green-500/10 rounded-full 
            flex items-center justify-center
          "
          aria-hidden
        >
          <svg className="w-3 h-3 sm:w-4 sm:h-4 md:w-5 md:h-5 text-green-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
          </svg>
        </div>

        {/* Stats-knapp (samme posisjon som før) */}
        <button
          onClick={(e) => {
            e.stopPropagation();
            setShowSecurityStats((v) => !v);
            setShowSecurityInfo(false);
          }}
          className="
            absolute 
            bottom-12 sm:bottom-16 md:bottom-20 
            left-2 sm:left-4 md:left-8 
            z-30
            w-10 h-10 sm:w-12 sm:h-12 md:w-14 md:h-14 
            bg-blue-500/10 
            rounded-lg sm:rounded-xl 
            flex items-center justify-center 
            hover:bg-blue-500/20 transition-colors 
            focus:outline-none focus:ring-2 focus:ring-ring 
            group
          "
          aria-label={t.showSecurityStats}
          aria-expanded={showSecurityStats}
          aria-controls="security-stats-panel"
          title={`${companyName} Statistikk`}
        >
          <svg className="w-5 h-5 sm:w-6 sm:h-6 md:w-7 md:h-7 text-blue-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z" />
          </svg>
          {/* Indikatorprikk */}
          <div
            className={`
              absolute -top-0.5 -right-0.5 sm:-top-1 sm:-right-1 
              w-2 h-2 sm:w-2.5 sm:h-2.5 md:w-3 md:h-3 
              rounded-full bg-blue-500/80 
              flex items-center justify-center 
              transition-transform
            `}
          >
            {showSecurityStats ? (
              <ChevronUp className="w-1 h-1 sm:w-1.5 sm:h-1.5 md:w-2 md:h-2 text-white" />
            ) : (
              <ChevronRight className="w-1 h-1 sm:w-1.5 sm:h-1.5 md:w-2 md:h-2 text-white" />
            )}
          </div>
        </button>

        {/* Stats-panel: åpner horisontalt til høyre for knappen (beholder bunnposisjon) */}
        {showSecurityStats && (
          <div
            id="security-stats-panel"
            role="dialog"
            aria-label={t.securityStats}
            className="
              absolute
              bottom-12 sm:bottom-16 md:bottom-20
              left-14 sm:left-[4.5rem] md:left-[6.5rem]
              z-40
              bg-background/95 backdrop-blur-sm 
              rounded-md md:rounded-lg 
              shadow-lg 
              p-2 sm:p-3 md:p-4 
              max-w-xs sm:max-w-sm md:max-w-md
            "
            onClick={(e) => e.stopPropagation()}
          >
            <SecurityStats companyName={companyName} />
          </div>
        )}

        {/* Animerte prikker (høyre side) */}
        <div
          className="
            absolute 
            top-1/3 
            right-3 sm:right-4 md:right-6 
            flex flex-col 
            space-y-1 sm:space-y-1.5 md:space-y-2
            z-10
          "
          aria-hidden
        >
          <div className="w-1.5 h-1.5 sm:w-2 sm:h-2 bg-green-500 rounded-full animate-pulse"></div>
          <div className="w-1.5 h-1.5 sm:w-2 sm:h-2 bg-blue-500 rounded-full animate-pulse" style={{ animationDelay: '0.5s' }}></div>
          <div className="w-1.5 h-1.5 sm:w-2 sm:h-2 bg-purple-500 rounded-full animate-pulse" style={{ animationDelay: '1s' }}></div>
        </div>

        {/* Diskré rutenett – må ligge bakerst og ikke fange klikk */}
        <div className="absolute inset-0 opacity-5 pointer-events-none z-0" aria-hidden>
          <svg width="100%" height="100%" xmlns="http://www.w3.org/2000/svg">
            <defs>
              <pattern id="grid" width="20" height="20" patternUnits="userSpaceOnUse">
                <path d="M 20 0 L 0 0 0 20" fill="none" stroke="currentColor" strokeWidth="0.5" />
              </pattern>
            </defs>
            <rect width="100%" height="100%" fill="url(#grid)" />
          </svg>
        </div>

        {/* Små teknologimerker (nederst til høyre) */}
        <div
          className="
            absolute 
            bottom-3 sm:bottom-4 md:bottom-6 
            right-4 sm:right-8 md:right-12 
            text-xs 
            text-muted-foreground/60 
            space-y-0.5 sm:space-y-1 
            hidden sm:block
            z-10
          "
          aria-hidden
        >
          <div className="flex items-center space-x-1 sm:space-x-2">
            <div className="w-1 h-1 bg-green-400 rounded-full"></div>
            <span className="text-xs">WebAuthn</span>
          </div>
          <div className="flex items-center space-x-1 sm:space-x-2">
            <div className="w-1 h-1 bg-blue-400 rounded-full"></div>
            <span className="text-xs">OAuth 2.0</span>
          </div>
          <div className="hidden md:flex items-center space-x-1 sm:space-x-2">
            <div className="w-1 h-1 bg-purple-400 rounded-full"></div>
            <span className="text-xs">GDPR</span>
          </div>
        </div>

        {/* Samtykkebanner (vises ved behov) - Prioritet over andre elementer */}
        {showConsentBanner && onAcceptAll && onRejectAll && onShowPreferences && (
          <div className="absolute inset-0 z-50">
            <ConsentBanner 
              onAcceptAll={onAcceptAll} 
              onRejectAll={onRejectAll} 
              onShowPreferences={onShowPreferences} 
            />
          </div>
        )}
      </div>
    </div>
  );
}
