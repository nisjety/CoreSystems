import React from 'react';
import { Globe, Check } from 'lucide-react';
import { useLanguageSwitch, useAvailableLocales } from '../hooks';

interface LanguageSelectorProps {
  variant?: 'dropdown' | 'toggle' | 'inline';
  showIcon?: boolean;
  showLabels?: boolean;
  className?: string;
  buttonClassName?: string;
  dropdownClassName?: string;
}

/**
 * Language selector component with multiple variants
 */
export function LanguageSelector({
  variant = 'dropdown',
  showIcon = true,
  showLabels = true,
  className = '',
  buttonClassName = '',
  dropdownClassName = '',
}: LanguageSelectorProps) {
  const {
    currentLocale,
    currentLanguageName,
    isNorwegian,
    isEnglish,
    switchToNorwegian,
    switchToEnglish,
    toggleLanguage,
  } = useLanguageSwitch();
  
  const availableLocales = useAvailableLocales();
  const [isOpen, setIsOpen] = React.useState(false);

  // Toggle variant - simple button that switches between languages
  if (variant === 'toggle') {
    return (
      <button
        onClick={toggleLanguage}
        className={`inline-flex items-center gap-2 px-3 py-2 text-sm font-medium text-muted-foreground hover:text-foreground transition-colors focus:outline-none focus:ring-2 focus:ring-ring rounded-lg ${buttonClassName}`}
        aria-label={`Switch to ${isNorwegian ? 'English' : 'Norwegian'}`}
      >
        {showIcon && <Globe className="w-4 h-4" />}
        {showLabels && (
          <span>{isNorwegian ? 'EN' : 'NO'}</span>
        )}
      </button>
    );
  }

  // Inline variant - radio button style
  if (variant === 'inline') {
    return (
      <div className={`inline-flex items-center gap-1 p-1 bg-muted rounded-lg ${className}`}>
        <button
          onClick={switchToNorwegian}
          className={`px-3 py-1.5 text-sm font-medium rounded-md transition-colors focus:outline-none focus:ring-2 focus:ring-ring ${
            isNorwegian
              ? 'bg-background text-foreground shadow-sm'
              : 'text-muted-foreground hover:text-foreground'
          }`}
          aria-pressed={isNorwegian}
        >
          NO
        </button>
        <button
          onClick={switchToEnglish}
          className={`px-3 py-1.5 text-sm font-medium rounded-md transition-colors focus:outline-none focus:ring-2 focus:ring-ring ${
            isEnglish
              ? 'bg-background text-foreground shadow-sm'
              : 'text-muted-foreground hover:text-foreground'
          }`}
          aria-pressed={isEnglish}
        >
          EN
        </button>
      </div>
    );
  }

  // Dropdown variant - full dropdown menu
  return (
    <div className={`relative ${className}`}>
      <button
        onClick={() => setIsOpen(!isOpen)}
        className={`inline-flex items-center gap-2 px-3 py-2 text-sm font-medium text-muted-foreground hover:text-foreground transition-colors focus:outline-none focus:ring-2 focus:ring-ring rounded-lg ${buttonClassName}`}
        aria-expanded={isOpen}
        aria-haspopup="menu"
      >
        {showIcon && <Globe className="w-4 h-4" />}
        {showLabels && <span>{currentLanguageName}</span>}
        <svg
          className={`w-4 h-4 transition-transform ${isOpen ? 'rotate-180' : ''}`}
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
        </svg>
      </button>

      {isOpen && (
        <>
          {/* Backdrop */}
          <div 
            className="fixed inset-0 z-10" 
            onClick={() => setIsOpen(false)} 
          />
          
          {/* Dropdown menu */}
          <div className={`absolute top-full mt-1 left-0 z-20 min-w-[140px] bg-background border border-border rounded-lg shadow-lg py-1 ${dropdownClassName}`}>
            {availableLocales.map((locale) => (
              <button
                key={locale.code}
                onClick={() => {
                  if (locale.code === 'nb') switchToNorwegian();
                  else switchToEnglish();
                  setIsOpen(false);
                }}
                className="w-full flex items-center justify-between px-3 py-2 text-sm text-foreground hover:bg-muted transition-colors focus:outline-none focus:bg-muted"
                role="menuitem"
              >
                <span>{locale.nativeName}</span>
                {currentLocale === locale.code && (
                  <Check className="w-4 h-4 text-primary" />
                )}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

/**
 * Simple language toggle for headers/navigation
 */
export function LanguageToggle({ className = '' }: { className?: string }) {
  return (
    <LanguageSelector
      variant="toggle"
      showIcon={true}
      showLabels={true}
      className={className}
    />
  );
}

/**
 * Inline language switcher for forms
 */
export function InlineLanguageSwitch({ className = '' }: { className?: string }) {
  return (
    <LanguageSelector
      variant="inline"
      showIcon={false}
      showLabels={true}
      className={className}
    />
  );
}

/**
 * Full dropdown language selector
 */
export function LanguageDropdown({ className = '' }: { className?: string }) {
  return (
    <LanguageSelector
      variant="dropdown"
      showIcon={true}
      showLabels={true}
      className={className}
    />
  );
}
