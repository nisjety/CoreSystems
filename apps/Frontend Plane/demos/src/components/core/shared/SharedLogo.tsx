'use client';

import React from 'react';
import Image from 'next/image';
import { ChevronRight, ChevronLeft, Menu } from 'lucide-react';
import { motion } from 'framer-motion';
import { cn } from '../Sidebar/utils';
import { useSidebar } from './SidebarContext';
import { SimpleTooltip } from '../Sidebar/ui/simple-tooltip';

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
              <div className="w-14 h-14 flex items-center justify-center p-1">
                <Image 
                  src="/logo.png" 
                  alt="Aquatiq Logo" 
                  width={56}
                  height={56}
                  className="object-contain"
                />
              </div>
            ) : showTitle && (
              <div className="flex items-center gap-3">
                <Image 
                  src="/logo-white.png" 
                  alt="Aquatiq Logo" 
                  width={120}
                  height={32}
                  className="object-contain"
                />
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );

export const SharedLogo: React.FC<SharedLogoProps> = ({
  title = "Aquatiq",
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
        <ChevronRight className="w-3 h-3 text-white/60 group-hover:text-white group-hover:animate-[moveRight_1s_ease-in-out_infinite]" />
      ) : (
        <ChevronLeft className="w-3 h-3 text-white/60 group-hover:text-white group-hover:animate-[moveLeft_1s_ease-in-out_infinite]" />
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
            'flex items-center gap-2 p-1 rounded-xl transition-all duration-200 group',
            isMinimized 
              ? 'flex-col hover:scale-105' 
              : 'flex-row hover:bg-white/10',
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
            <div className="transition-all duration-300 ease-in-out ml-1">
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
        <motion.button
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
        </motion.button>
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
