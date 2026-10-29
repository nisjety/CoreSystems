'use client';

import React, { useState } from 'react';
import Image from 'next/image';
import { Search, User } from 'lucide-react';
import { motion } from 'framer-motion';
import {
  Navbar,
  NavBody,
  MobileNav,
  NavbarButton,
  MobileNavHeader,
  MobileNavToggle,
  MobileNavMenu,
} from './ui/resizable-navbar';
import { ThemeToggle } from './ui/ThemeToggle';
import { useAuth } from '@/components/auth/hooks/use-auth';
import { useCurrentUser, useGraphProfile } from './hooks';
import { useSidebar } from '../shared/SidebarContext';

export interface ResizableNavbarProps {
  className?: string;
  showSearch?: boolean;
  showProfile?: boolean;
  onProfileClick?: () => void;
}

export function ResizableNavbar({
  className,
  showSearch = true,
  showProfile = true,
  onProfileClick,
}: ResizableNavbarProps) {
  const [isMobileMenuOpen, setIsMobileMenuOpen] = useState(false);
  const [isScrolled, setIsScrolled] = useState(false);
  // Auth state
  const { user: authUser } = useAuth();
  
  // Get sidebar state for dynamic positioning
  const { isMinimized } = useSidebar();

  // Fetch data from corebar backend only if user is authenticated
  const { data: currentUser } = useCurrentUser({ enabled: !!authUser });
  const { data: graphProfile } = useGraphProfile({ enabled: !!authUser });

  // Use either the graph profile or current user data, with fallbacks
  // Graph profile now includes Microsoft 365 data when available
  const userDisplayName = graphProfile?.name || graphProfile?.profile?.displayName || currentUser?.name || 'User';
  const userAvatar = graphProfile?.avatar || currentUser?.avatar;

  // Listen for scroll changes to update colors
  React.useEffect(() => {
    const handleScroll = () => {
      setIsScrolled(window.scrollY > 50);
    };

    window.addEventListener('scroll', handleScroll);
    return () => window.removeEventListener('scroll', handleScroll);
  }, []);

  // User profile section component - Following Law of Proximity for related elements
  const UserProfile = () => (
    <div className="flex items-center space-x-4">
      {/* Theme Toggle - Fitts's Law: adequate touch target size */}
      <div className="p-1">
        <ThemeToggle size="sm" isScrolled={isScrolled} />
      </div>

      {showProfile && (
        <motion.button
          onClick={onProfileClick}
          whileHover={{ scale: 1.02 }}
          whileTap={{ scale: 0.98 }}
          className={`flex items-center space-x-2 p-1.5 bg-white rounded-lg transition-all duration-200 focus:outline-none focus:ring-2 shadow-sm hover:shadow-md ${
            isScrolled 
              ? 'hover:bg-white/90 active:bg-white/80 focus:ring-white/30 shadow-md hover:shadow-lg' 
              : 'hover:bg-white/90 active:bg-white/80 focus:ring-primary/30'
          }`}
          aria-label="Profile menu"
        >
          {userAvatar ? (
            <Image
              src={userAvatar}
              alt={userDisplayName}
              width={28}
              height={28}
              className={`h-7 w-7 rounded-full object-cover  ${
                isScrolled ? 'border-white/20' : 'border-primary/20'
              }`}
            />
          ) : (
            <div className={`h-7 w-7 rounded-full flex items-center justify-center ${
              isScrolled ? 'bg-white' : 'bg-primary'
            }`}>
              <User className={`h-4 w-4 ${isScrolled ? 'text-primary' : 'text-white'}`} />
            </div>
          )}
          <span className={`text-sm font-medium hidden lg:block max-w-28 truncate ${
            isScrolled ? 'text-white' : 'text-black'
          }`}>
            {userDisplayName}
          </span>
        </motion.button>
      )}
    </div>
  );

  // Centered search component - Following Jakob's Law for familiar search patterns
  const SearchBar = () => {
    const [isFocused, setIsFocused] = useState(false);
    
    return (
      showSearch ? (
        <motion.div 
          className="relative w-[40rem] z-10" // Added z-10 to ensure it's above other elements
          animate={{
            scale: isFocused ? 1.02 : 1,
          }}
          transition={{
            type: "spring",
            stiffness: 400,
            damping: 30,
          }}
        >
          <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 h-5 w-5 text-gray-400 pointer-events-none" />
          <input
            type="text"
            placeholder="Search..."
            onFocus={() => setIsFocused(true)}
            onBlur={() => setIsFocused(false)}
            className="w-full pl-10 pr-4 py-3 bg-white text-black placeholder-gray-400 rounded-lg focus:ring-2 focus:ring-primary/30 focus:border-primary/40 outline-none transition-all duration-200 text-sm shadow-sm focus:shadow-md relative z-10"
          />
        </motion.div>
      ) : null
    );
  };

  return (
    <div className="relative w-full">
      <Navbar className={className}>
        {/* Desktop Navigation - Law of Common Region: Clear sections */}
        <NavBody>
          {/* Left Section: Dynamic spacer based on sidebar state */}
          <div className={`transition-all duration-300 ${
            isMinimized ? 'flex-[0.7]' : 'flex-[0.9]'
          }`} />
          
          {/* Center Section: Search Bar - Positioned relative to sidebar */}
          <div className="flex-shrink-0">
            <SearchBar />
          </div>
          
          {/* Right Section: User Controls */}
          <div className="flex-[0.7] flex justify-end">
            <UserProfile />
          </div>
        </NavBody>

        {/* Mobile Navigation */}
        <MobileNav>
          <MobileNavHeader>
            <div className="flex items-center">
              {/* Mobile menu handled by sidebar context */}
            </div>
            <MobileNavToggle
              isOpen={isMobileMenuOpen}
              onClick={() => setIsMobileMenuOpen(!isMobileMenuOpen)}
              isScrolled={isScrolled}
            />
          </MobileNavHeader>

          <MobileNavMenu
            isOpen={isMobileMenuOpen}
            onClose={() => setIsMobileMenuOpen(false)}
          >
            {/* Mobile search - Miller's Law: Keep it simple */}
            {showSearch && (
              <div className="w-full mb-4">
                <motion.div 
                  className="relative"
                  whileFocus={{ scale: 1.02 }}
                  transition={{
                    type: "spring",
                    stiffness: 400,
                    damping: 30,
                  }}
                >
                  <Search className="absolute left-4 top-1/2 transform -translate-y-1/2 h-4 w-4 text-gray-400" />
                  <input
                    type="text"
                    placeholder="Search..."
                    className="w-full pl-11 pr-4 py-2 bg-white text-black placeholder-gray-400 rounded-full focus:ring-2 focus:ring-primary/30 outline-none transition-all duration-200 shadow-sm focus:shadow-md"
                  />
                </motion.div>
              </div>
            )}

            {/* Mobile user section - Law of Proximity for user-related elements */}
            <div className="flex w-full flex-col gap-4 pt-6 border-t border-white/20">
              <div className="flex items-center justify-between">
                <div className="flex items-center space-x-4">
                  {userAvatar ? (
                    <Image
                      src={userAvatar}
                      alt={userDisplayName}
                      width={40}
                      height={40}
                      className="h-10 w-10 rounded-full object-cover border-2 border-white/20"
                    />
                  ) : (
                    <div className="h-10 w-10 bg-white rounded-full flex items-center justify-center">
                      <User className="h-5 w-5 text-primary" />
                    </div>
                  )}
                  <div>
                    <p className="text-base font-semibold text-white">
                      {userDisplayName}
                    </p>
                    <p className="text-sm text-white/70">
                      {currentUser?.email || 'user@example.com'}
                    </p>
                  </div>
                </div>
                
                <div className="flex items-center space-x-2">
                  {/* Theme Toggle in Mobile */}
                  <div className="p-2">
                    <ThemeToggle size="sm" isScrolled={isScrolled} />
                  </div>
                </div>
              </div>
              
              <NavbarButton
                onClick={() => {
                  onProfileClick?.();
                  setIsMobileMenuOpen(false);
                }}
                variant="primary"
                className="w-full justify-center mt-2"
              >
                View Profile
              </NavbarButton>
            </div>
          </MobileNavMenu>
        </MobileNav>
      </Navbar>
    </div>
  );
}
