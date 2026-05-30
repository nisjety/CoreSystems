'use client';

import React, { useState, useRef, useEffect, useCallback } from 'react';
import { Globe, ChevronDown, Check } from 'lucide-react';
import { useLanguageSwitch } from '../lib/i18n/hooks';

interface LanguageSwitcherProps {
  variant?: 'minimal' | 'pill' | 'tabs' | 'dropdown';
  className?: string;
  showIcon?: boolean;
  size?: 'sm' | 'md' | 'lg';
}

/**
 * Language Switcher Component with Dropdown
 * Clean, shadcn-style toggle between Norwegian and English
 */
export function LanguageSwitcher({
  variant = 'dropdown',
  className = '',
  showIcon = true,
  size = 'md'
}: LanguageSwitcherProps) {
  const { isNorwegian, toggleLanguage } = useLanguageSwitch();
  const [isOpen, setIsOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  // Remove debugging logs to prevent render spam
  const handleToggle = useCallback(() => {
    toggleLanguage();
  }, [toggleLanguage]);

  // Close dropdown when clicking outside
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, []);

  // Close dropdown on escape key
  useEffect(() => {
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setIsOpen(false);
      }
    };

    if (isOpen) {
      document.addEventListener('keydown', handleEscape);
      return () => {
        document.removeEventListener('keydown', handleEscape);
      };
    }
  }, [isOpen]);

  const languages = [
    { code: 'nb', name: 'Norsk', flag: '🇳🇴' },
    { code: 'en', name: 'English', flag: '🇺🇸' }
  ];

  // Size variants
  const sizeClasses = {
    sm: 'text-sm px-2 py-2 gap-2',
    md: 'text-base px-3 py-3 gap-3',
    lg: 'text-lg px-4 py-4 gap-4'
  };

  const iconSizes = {
    sm: 'w-4 h-4',
    md: 'w-5 h-5',
    lg: 'w-6 h-6'
  };

  // Dropdown variant - globe icon with dropdown
  if (variant === 'dropdown') {
    return (
      <div className={`relative ${className}`} ref={dropdownRef}>
        <button
          onClick={() => setIsOpen(!isOpen)}
          className={`inline-flex items-center ${sizeClasses[size]} font-medium text-muted-foreground hover:text-foreground transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 rounded-lg bg-background/80 backdrop-blur-sm shadow-sm hover:shadow-md`}
          aria-label="Select language"
          aria-expanded={isOpen}
          aria-haspopup="true"
        >
          <Globe className={iconSizes[size]} />
          <ChevronDown className={`${iconSizes[size]} transition-transform ${isOpen ? 'rotate-180' : 'rotate-0'}`} />
        </button>

        {isOpen && (
          <div className="absolute top-full mt-2 right-0 min-w-[140px] bg-background border border-border rounded-lg shadow-lg z-[300] py-2">
            {languages.map((language) => {
              const isSelected = (language.code === 'nb' && isNorwegian) || (language.code === 'en' && !isNorwegian);
              
              return (
                <button
                  key={language.code}
                  onClick={() => {
                    if (!isSelected) {
                      handleToggle();
                    }
                    setIsOpen(false);
                  }}
                  className={`w-full flex items-center gap-3 px-4 py-3 text-base hover:bg-muted transition-colors ${
                    isSelected ? 'text-foreground' : 'text-muted-foreground'
                  }`}
                >
                  <span className="text-lg">{language.flag}</span>
                  <span className="flex-1 text-left">{language.name}</span>
                  {isSelected && <Check className="w-4 h-4" />}
                </button>
              );
            })}
          </div>
        )}
      </div>
    );
  }

  // Pill variant - rounded pill with current language
  if (variant === 'pill') {
    return (
      <button
        onClick={handleToggle}
        className={`inline-flex items-center ${sizeClasses[size]} font-medium text-foreground bg-muted hover:bg-muted/80 transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 rounded-full ${className}`}
        aria-label={`Switch to ${isNorwegian ? 'English' : 'Norwegian'}`}
      >
        {showIcon && <Globe className={iconSizes[size]} />}
        <span>{isNorwegian ? 'Norsk' : 'English'}</span>
      </button>
    );
  }

  // Tabs variant - toggle between two options
  if (variant === 'tabs') {
    return (
      <div className={`inline-flex items-center p-1 bg-muted rounded-lg ${className}`}>
        <button
          onClick={() => !isNorwegian && handleToggle()}
          className={`px-2 py-1 text-sm font-medium rounded-md transition-colors focus:outline-none focus:ring-1 focus:ring-ring ${
            isNorwegian
              ? 'bg-background text-foreground shadow-sm'
              : 'text-muted-foreground hover:text-foreground'
          }`}
          aria-pressed={isNorwegian}
        >
          NO
        </button>
        <button
          onClick={() => isNorwegian && handleToggle()}
          className={`px-2 py-1 text-sm font-medium rounded-md transition-colors focus:outline-none focus:ring-1 focus:ring-ring ${
            !isNorwegian
              ? 'bg-background text-foreground shadow-sm'
              : 'text-muted-foreground hover:text-foreground'
          }`}
          aria-pressed={!isNorwegian}
        >
          EN
        </button>
      </div>
    );
  }

  return null;
}