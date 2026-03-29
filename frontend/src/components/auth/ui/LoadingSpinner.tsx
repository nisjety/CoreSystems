'use client';

import React from 'react';
import { Loader2, Shield, Smartphone, Mail, Key, Settings } from 'lucide-react';
import { useLanguageSwitch } from '../lib/i18n/hooks';

interface LoadingSpinnerProps {
  /**
   * Additional CSS classes
   */
  className?: string;
  
  /**
   * Size variant for the spinner
   */
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl';
  
  /**
   * Loading text to display
   */
  text?: string;
  
  /**
   * Type of operation being performed (affects icon and messaging)
   */
  operation?: 'auth' | 'verification' | 'email' | 'sms' | 'security' | 'settings' | 'general';
  
  /**
   * Show centered layout
   */
  centered?: boolean;
  
  /**
   * Show as overlay
   */
  overlay?: boolean;
  
  /**
   * Norwegian/English text support
   */
  language?: 'no' | 'en';
}

/**
 * LoadingSpinner Component
 * 
 * Accessible loading spinner with:
 * - ✅ WCAG 2.1 AA compliance (proper ARIA labels, screen reader support)
 * - ✅ Design law compliance (clear feedback, reduced cognitive load)
 * - ✅ Norwegian/English i18n support
 * - ✅ Contextual icons and messaging
 * - ✅ Multiple size variants
 * - ✅ Overlay and centered layouts
 * - ✅ Operation-specific styling
 * 
 * @example
 * ```tsx
 * <LoadingSpinner 
 *   operation="verification" 
 *   text="Verifying code..." 
 *   size="md" 
 * />
 * ```
 */
export function LoadingSpinner({
  className = '',
  size = 'md',
  text,
  operation = 'general',
  centered = false,
  overlay = false,
  language, // Remove default, use hook instead
}: LoadingSpinnerProps) {
  // Use i18n hook for language detection
  const { isNorwegian } = useLanguageSwitch();
  const currentLanguage = language || (isNorwegian ? 'no' : 'en');
  // Size configurations
  const sizeConfig = {
    xs: {
      spinner: 'w-3 h-3',
      icon: 'w-3 h-3',
      text: 'text-xs',
      gap: 'gap-1',
      padding: 'p-1',
    },
    sm: {
      spinner: 'w-4 h-4',
      icon: 'w-4 h-4',
      text: 'text-sm',
      gap: 'gap-2',
      padding: 'p-2',
    },
    md: {
      spinner: 'w-5 h-5',
      icon: 'w-5 h-5',
      text: 'text-sm',
      gap: 'gap-2',
      padding: 'p-3',
    },
    lg: {
      spinner: 'w-6 h-6',
      icon: 'w-6 h-6',
      text: 'text-base',
      gap: 'gap-3',
      padding: 'p-4',
    },
    xl: {
      spinner: 'w-8 h-8',
      icon: 'w-8 h-8',
      text: 'text-lg',
      gap: 'gap-3',
      padding: 'p-6',
    },
  };

  // Operation-specific configurations
  const operationConfig = {
    auth: {
      icon: Shield,
      color: 'text-primary',
      bgColor: 'bg-primary/5',
      borderColor: 'border-primary/20',
      defaultText: {
        no: 'Autentiserer...',
        en: 'Authenticating...',
      },
    },
    verification: {
      icon: Shield,
      color: 'text-blue-600',
      bgColor: 'bg-blue-50',
      borderColor: 'border-blue-200',
      defaultText: {
        no: 'Verifiserer...',
        en: 'Verifying...',
      },
    },
    email: {
      icon: Mail,
      color: 'text-green-600',
      bgColor: 'bg-green-50',
      borderColor: 'border-green-200',
      defaultText: {
        no: 'Sender e-post...',
        en: 'Sending email...',
      },
    },
    sms: {
      icon: Smartphone,
      color: 'text-orange-600',
      bgColor: 'bg-orange-50',
      borderColor: 'border-orange-200',
      defaultText: {
        no: 'Sender SMS...',
        en: 'Sending SMS...',
      },
    },
    security: {
      icon: Key,
      color: 'text-red-600',
      bgColor: 'bg-red-50',
      borderColor: 'border-red-200',
      defaultText: {
        no: 'Sikkerhetskontroll...',
        en: 'Security check...',
      },
    },
    settings: {
      icon: Settings,
      color: 'text-purple-600',
      bgColor: 'bg-purple-50',
      borderColor: 'border-purple-200',
      defaultText: {
        no: 'Lagrer innstillinger...',
        en: 'Saving settings...',
      },
    },
    general: {
      icon: Loader2,
      color: 'text-muted-foreground',
      bgColor: 'bg-muted/20',
      borderColor: 'border-border',
      defaultText: {
        no: 'Laster...',
        en: 'Loading...',
      },
    },
  };

  const config = operationConfig[operation];
  const sizes = sizeConfig[size];
  const displayText = text || config.defaultText[currentLanguage];
  const OperationIcon = config.icon;

  // Base component structure
  const spinnerContent = (
    <div 
      className={`
        inline-flex items-center ${sizes.gap} ${sizes.padding}
        ${config.bgColor} ${config.borderColor} border rounded-lg
        ${className}
      `}
      role="status"
      aria-live="polite"
      aria-label={displayText}
    >
      {/* Operation-specific icon (static) */}
      <OperationIcon 
        className={`${sizes.icon} ${config.color} flex-shrink-0`}
        aria-hidden="true"
      />
      
      {/* Spinning loader */}
      <Loader2 
        className={`${sizes.spinner} ${config.color} animate-spin flex-shrink-0`} 
        aria-hidden="true"
      />
      
      {/* Loading text */}
      {displayText && (
        <span className={`${sizes.text} ${config.color} font-medium`}>
          {displayText}
        </span>
      )}
      
      {/* Screen reader text */}
      <span className="sr-only">
        {language === 'no' ? 'Laster inn innhold' : 'Loading content'}
      </span>
    </div>
  );

  // Overlay version
  if (overlay) {
    return (
      <div 
        className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 backdrop-blur-sm"
        role="dialog"
        aria-modal="true"
        aria-label={language === 'no' ? 'Laster' : 'Loading'}
      >
        <div className="bg-card border border-border rounded-lg shadow-lg">
          {spinnerContent}
        </div>
      </div>
    );
  }

  // Centered version
  if (centered) {
    return (
      <div className="flex items-center justify-center w-full py-8">
        {spinnerContent}
      </div>
    );
  }

  // Default inline version
  return spinnerContent;
}

/**
 * Minimal loading spinner for inline use
 */
interface InlineSpinnerProps {
  className?: string;
  size?: 'xs' | 'sm' | 'md';
  color?: string;
}

export function InlineSpinner({ 
  className = '', 
  size = 'sm',
  color = 'text-muted-foreground'
}: InlineSpinnerProps) {
  const sizeClasses = {
    xs: 'w-3 h-3',
    sm: 'w-4 h-4', 
    md: 'w-5 h-5',
  };

  return (
    <Loader2 
      className={`${sizeClasses[size]} ${color} animate-spin ${className}`}
      aria-hidden="true"
    />
  );
}

/**
 * Loading skeleton for content placeholders
 */
interface LoadingSkeletonProps {
  className?: string;
  lines?: number;
  width?: 'full' | 'auto';
  height?: 'sm' | 'md' | 'lg';
}

export function LoadingSkeleton({ 
  className = '',
  lines = 3,
  width = 'full',
  height = 'md'
}: LoadingSkeletonProps) {
  const heightClasses = {
    sm: 'h-3',
    md: 'h-4',
    lg: 'h-5',
  };

  const widthClasses = {
    full: 'w-full',
    auto: 'w-auto',
  };

  return (
    <div className={`space-y-2 ${className}`} role="status" aria-label="Loading content">
      {Array.from({ length: lines }).map((_, index) => (
        <div
          key={index}
          className={`
            bg-muted rounded animate-pulse
            ${heightClasses[height]}
            ${widthClasses[width]}
            ${index === lines - 1 ? 'w-3/4' : ''}
          `}
        />
      ))}
      <span className="sr-only">Loading content...</span>
    </div>
  );
}

/**
 * Loading dots animation
 */
interface LoadingDotsProps {
  className?: string;
  size?: 'sm' | 'md' | 'lg';
  color?: string;
}

export function LoadingDots({
  className = '',
  size = 'md',
  color = 'text-muted-foreground'
}: LoadingDotsProps) {
  const sizeClasses = {
    sm: 'w-1 h-1',
    md: 'w-1.5 h-1.5',
    lg: 'w-2 h-2',
  };

  return (
    <div 
      className={`flex items-center gap-1 ${className}`}
      role="status"
      aria-label="Loading"
    >
      {[0, 1, 2].map((index) => (
        <div
          key={index}
          className={`
            ${sizeClasses[size]} ${color} bg-current rounded-full
            animate-pulse
          `}
          style={{
            animationDelay: `${index * 0.2}s`,
          }}
        />
      ))}
      <span className="sr-only">Loading</span>
    </div>
  );
}

export default LoadingSpinner;
