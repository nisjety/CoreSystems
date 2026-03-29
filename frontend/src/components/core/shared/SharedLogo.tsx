'use client';

import React from 'react';
import { ChevronRight, ChevronLeft, CircleDashed, Menu } from 'lucide-react';
import { m } from 'framer-motion';
import { cn } from '../sidebar/utils';
import { useSidebar } from './SidebarContext';
import { SimpleTooltip } from '../sidebar/ui/simple-tooltip';

interface SharedLogoProps {
  title?: string;
  isScrolled?: boolean;
  showTitle?: boolean;
  showToggle?: boolean;
  variant?: 'navbar' | 'sidebar';
  className?: string;
}

const LogoContent: React.FC<{
  variant: 'navbar' | 'sidebar';
  isMinimized: boolean;
  isScrolled: boolean;
  showTitle: boolean;
  title: string;
}> = ({ variant, isMinimized, isScrolled, showTitle, title }) => (
    <div className="flex items-center gap-3">
      {/* Logo - always show */}
      <div className="flex-1">
        {variant === 'navbar' ? (
          <>
            {isMinimized ? (
              <div className={cn(
                'w-10 h-10 rounded-xl flex items-center justify-center shadow-sm shrink-0',
                isScrolled ? 'bg-white' : 'bg-primary'
              )}>
                <span className={`font-bold text-lg ${isScrolled ? 'text-primary' : 'text-white'}`}>A</span>
              </div>
            ) : showTitle && (
              <span className={`font-semibold text-lg tracking-tight ${isScrolled ? 'text-white' : 'text-black'}`}>
                {title}
              </span>
            )}
          </>
        ) : (
          <>
            {isMinimized ? (
              <div className="flex h-10 w-10 items-center justify-center rounded-2xl border border-black/8 bg-white shadow-[0_1px_0_rgba(255,255,255,0.86)]">
                <CircleDashed className="h-4.5 w-4.5 text-[#DD7A1F]" strokeWidth={1.7} />
              </div>
            ) : showTitle && (
              <div className="flex items-center gap-3">
                <div className="flex h-10 w-10 items-center justify-center rounded-[14px] border border-black/8 bg-white text-[#DD7A1F] shadow-[0_1px_0_rgba(255,255,255,0.86)]">
                  <CircleDashed className="h-4.5 w-4.5" strokeWidth={1.7} />
                </div>
                <div>
                  <div className="text-[10px] font-semibold uppercase tracking-[0.28em] text-[#8D8F98]">Workspace</div>
                  <span className="text-[15px] font-semibold tracking-[-0.02em] text-[#2D2F38]">Overview</span>
                </div>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );

export const SharedLogo: React.FC<SharedLogoProps> = ({
  title = "TRIODELAB",
  isScrolled = false,
  showTitle = true,
  showToggle = true,
  variant = 'navbar',
  className
}) => {
  const { isMinimized, isMobile, toggleMinimize, toggleMobileMenu } = useSidebar();

  const handleToggleClick = () => {
    if (variant === 'navbar') {
      if (isMobile) {
        toggleMobileMenu();
      } else {
        toggleMinimize();
      }
    } else {
      toggleMinimize();
    }
  };

  const getToggleIcon = () => {
    if (variant === 'navbar' && isMobile) {
      return <Menu className={`h-5 w-5 ${isScrolled ? 'text-white' : 'text-black'}`} />;
    }
    
    if (variant === 'sidebar') {
      return isMinimized ? (
        <ChevronRight className="w-3 h-3 text-black/45 group-hover:text-black group-hover:animate-[moveRight_1s_ease-in-out_infinite]" />
      ) : (
        <ChevronLeft className="w-3 h-3 text-black/45 group-hover:text-black group-hover:animate-[moveLeft_1s_ease-in-out_infinite]" />
      );
    }

    return isMinimized ? (
      <ChevronRight className="w-3 h-3 text-gray-400 group-hover:text-blue-500" />
    ) : (
      <ChevronLeft className="w-3 h-3 text-gray-400 group-hover:text-blue-500" />
    );
  };

  const getTooltipContent = () => {
    if (variant === 'navbar' && isMobile) {
      return 'Menu';
    }
    return isMinimized ? 'Expand sidebar' : 'Collapse sidebar';
  };

  if (!showToggle) {
    return (
      <div className={cn("flex items-center", className)}>
        <LogoContent 
          variant={variant}
          isMinimized={isMinimized}
          isScrolled={isScrolled}
          showTitle={showTitle}
          title={title}
        />
      </div>
    );
  }

  if (variant === 'sidebar') {
    return (
      <SimpleTooltip 
        content={getTooltipContent()}
        placement="right" 
        delay={200}
      >
        <button
          onClick={handleToggleClick}
          className={cn(
            'group flex items-center gap-3 rounded-[18px] p-1.5 transition-all duration-200',
            isMinimized 
              ? 'flex-col hover:scale-[1.02]' 
              : 'flex-row',
            className
          )}
        >
          <LogoContent 
            variant={variant}
            isMinimized={isMinimized}
            isScrolled={isScrolled}
            showTitle={showTitle}
            title={title}
          />
          
          {/* Animated Chevron Indicator - only show when expanded */}
          {!isMinimized && (
            <div className="ml-1 rounded-full border border-black/6 bg-[#F5F6F9] p-1.5 transition-all duration-300 ease-in-out">
              {getToggleIcon()}
            </div>
          )}
        </button>
      </SimpleTooltip>
    );
  }

  // Navbar variant
  return (
    <div className={cn("flex items-center", className)}>
      {/* Only show the toggle button on mobile for navbar */}
      {(isMobile) && (
        <m.button
          onClick={handleToggleClick}
          whileHover={{ scale: 1.05 }}
          whileTap={{ scale: 0.98 }}
          className={cn(
            'p-3 rounded-xl transition-all duration-200 mr-2 focus:outline-none focus:ring-2 shadow-sm hover:shadow-md',
            isScrolled 
              ? 'hover:bg-white/10 active:bg-white/20 focus:ring-white/30 shadow-md hover:shadow-lg' 
              : 'hover:bg-primary/10 active:bg-primary/20 focus:ring-primary/30'
          )}
          aria-label="Menu"
        >
          {getToggleIcon()}
        </m.button>
      )}
      <LogoContent 
        variant={variant}
        isMinimized={isMinimized}
        isScrolled={isScrolled}
        showTitle={showTitle}
        title={title}
      />
    </div>
  );
};
