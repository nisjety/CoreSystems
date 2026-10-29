'use client';

import React, { createContext, useContext, useState, useEffect } from 'react';

interface SidebarContextType {
  isMinimized: boolean;
  isMobile: boolean;
  showMobileMenu: boolean;
  setIsMinimized: (minimized: boolean) => void;
  setShowMobileMenu: (show: boolean) => void;
  toggleMinimize: () => void;
  toggleMobileMenu: () => void;
}

const SidebarContext = createContext<SidebarContextType | undefined>(undefined);

export const useSidebar = () => {
  const context = useContext(SidebarContext);
  if (context === undefined) {
    throw new Error('useSidebar must be used within a SidebarProvider');
  }
  return context;
};

interface SidebarProviderProps {
  children: React.ReactNode;
}

export const SidebarProvider: React.FC<SidebarProviderProps> = ({ children }) => {
  const [isMinimized, setIsMinimized] = useState(true); // Default to minimized
  const [isMobile, setIsMobile] = useState(false);
  const [showMobileMenu, setShowMobileMenu] = useState(false);

  // Check mobile screen size
  useEffect(() => {
    const checkMobile = () => {
      const isMobileView = window.innerWidth < 768;
      setIsMobile(isMobileView);
      
      // Auto-minimize on mobile
      if (isMobileView && !isMinimized) {
        setIsMinimized(true);
      }
    };
    
    checkMobile();
    window.addEventListener('resize', checkMobile);
    return () => window.removeEventListener('resize', checkMobile);
  }, [isMinimized]);

  const toggleMinimize = () => {
    setIsMinimized((prev) => {
      const next = !prev;
      if (next && isMobile) {
        setShowMobileMenu(false);
      }
      return next;
    });
  };

  const toggleMobileMenu = () => {
    setShowMobileMenu(!showMobileMenu);
  };

  const value = {
    isMinimized,
    isMobile,
    showMobileMenu,
    setIsMinimized,
    setShowMobileMenu,
    toggleMinimize,
    toggleMobileMenu,
  };

  return (
    <SidebarContext.Provider value={value}>
      {children}
    </SidebarContext.Provider>
  );
};
