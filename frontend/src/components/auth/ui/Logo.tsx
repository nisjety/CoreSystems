'use client';

import React from 'react';
import Image from 'next/image';

interface LogoProps {
  /**
   * Logo variant - determines which logo file to load from public folder
   */
  variant?: 'default' | 'aquatiq' | 'favicon' | 'wordmark' | 'icon' | 'dark' | 'light';
  
  /**
   * Size preset for the logo
   */
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl' | '2xl';
  
  /**
   * Custom width (overrides size preset)
   */
  width?: number;
  
  /**
   * Custom height (overrides size preset)
   */
  height?: number;
  
  /**
   * Alt text for accessibility
   */
  alt?: string;
  
  /**
   * Additional CSS classes
   */
  className?: string;
  
  /**
   * Whether to show company name alongside logo
   */
  showCompanyName?: boolean;
  
  /**
   * Company name to display
   */
  companyName?: string;
  
  /**
   * Custom logo URL (overrides variant)
   */
  customSrc?: string;
  
  /**
   * Priority loading for above-the-fold logos
   */
  priority?: boolean;
  
  /**
   * Click handler for interactive logos
   */
  onClick?: () => void;

  /**
   * Optional custom sizes attribute for responsive images
   */
  sizes?: string;

  /**
   * Optional style overrides
   */
  style?: React.CSSProperties;

  /**
   * Force adding auto counterpart dimension when only one dimension is controlled via classes
   * (prevents Next.js aspect ratio warning)
   */
  autoAspectFix?: boolean;
}

/**
 * Logo Component
 * Flexible logo component that loads logos from the public folder
 * with responsive sizing and multiple variants
 */
export function Logo({
  variant = 'default',
  size = 'md',
  width,
  height,
  alt = 'Company Logo',
  className = '',
  showCompanyName = false,
  companyName = 'ID-Knuten',
  customSrc,
  priority = false,
  onClick
}: LogoProps) {
  
  // Size presets
  const sizePresets = {
    xs: { width: 60, height: 60 },
    sm: { width: 70, height: 70 },
    md: { width: 100, height: 100 },
    lg: { width: 110, height: 110 },
    xl: { width: 120, height: 120 },
    '2xl': { width: 130, height: 130 }
  };

  // Logo file mapping based on variant
  const logoFiles = {
    default: '/logo.png',
    aquatiq: '/aquatiq-logo.svg',
    favicon: '/favicon.svg',
    wordmark: '/logo-wordmark.svg', // Add this file to public folder
    icon: '/logo.png',
    dark: '/logo.svg', // Add this file to public folder
    light: '/logo-white.svg', // Add this file to public folder
  };

  // Determine final dimensions
  const finalWidth = width || sizePresets[size].width;
  const finalHeight = height || sizePresets[size].height;
  
  // Determine logo source
  const logoSrc = customSrc || logoFiles[variant];
  
  // Base logo element
  const logoElement = (
    (() => {
      // Detect if only width OR only height utility classes are applied
      const hasWidthClass = /(^|\s)w-\S+/.test(className);
      const hasHeightClass = /(^|\s)h-\S+/.test(className);
      const needsAutoHeight = (hasWidthClass && !hasHeightClass);
      const needsAutoWidth = (hasHeightClass && !hasWidthClass);

  // Always enable auto aspect fix to avoid Next.js warnings in dev
  const enableAutoAspect = true;

      const style: React.CSSProperties = {
        ...(needsAutoHeight && enableAutoAspect ? { height: 'auto' } : {}),
        ...(needsAutoWidth && enableAutoAspect ? { width: 'auto' } : {}),
      };

      return (
        <Image
          src={logoSrc}
          alt={alt}
            // Supply intrinsic dimensions for aspect ratio; CSS utilities can still scale
          width={finalWidth}
          height={finalHeight}
          priority={priority}
          sizes={undefined}
          className={`select-none ${onClick ? 'cursor-pointer' : ''}`}
          onClick={onClick}
          style={style}
        />
      );
    })()
  );

  // If showing company name, wrap in container
  if (showCompanyName) {
    return (
      <div 
        className={`flex items-center gap-2 ${onClick ? 'cursor-pointer' : ''} ${className}`}
        onClick={onClick}
      >
        {logoElement}
        <span className="font-semibold text-foreground select-none">
          {companyName}
        </span>
      </div>
    );
  }

  // Return just the logo with optional wrapper class
  if (onClick) {
    return (
      <button
        type="button"
        className={className}
        onClick={onClick}
        aria-label="Logo"
      >
        {logoElement}
      </button>
    );
  }
  return (
    <div className={className}>
      {logoElement}
    </div>
  );
}

/**
 * Logo with company name - convenience component
 */
export function LogoWithName(props: Omit<LogoProps, 'showCompanyName'>) {
  return <Logo {...props} showCompanyName />;
}

/**
 * Clickable logo - convenience component for navigation
 */
export function ClickableLogo(props: Omit<LogoProps, 'onClick'> & { onLogoClick: () => void }) {
  const { onLogoClick, ...logoProps } = props;
  return <Logo {...logoProps} onClick={onLogoClick} />;
}

/**
 * Responsive logo that adapts to screen size
 */
export function ResponsiveLogo(props: Omit<LogoProps, 'size' | 'className'> & { className?: string }) {
  return (
    <Logo 
      {...props} 
      className={`w-6 h-6 sm:w-8 sm:h-8 md:w-10 md:h-10 ${props.className || ''}`}
      size="md" // Base size, overridden by responsive classes
    />
  );
}

/**
 * Header logo - optimized for navigation bars
 */
export function HeaderLogo(props: Omit<LogoProps, 'size' | 'priority'>) {
  return (
    <Logo 
      {...props}
      size="lg"
      priority
      showCompanyName
    />
  );
}

/**
 * Auth page logo - optimized for authentication pages
 */
export function AuthLogo(props: Omit<LogoProps, 'size' | 'className'>) {
  return (
    <Logo 
      {...props}
      size="xl"
      className="mx-auto"
      showCompanyName
    />
  );
}

export default Logo;
