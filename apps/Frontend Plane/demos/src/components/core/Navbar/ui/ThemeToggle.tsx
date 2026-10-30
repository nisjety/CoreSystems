'use client';

import React from 'react';
import { motion } from 'motion/react';
import { useTheme } from '../contexts/ThemeContext';

interface ThemeToggleProps {
  className?: string;
  size?: 'sm' | 'md' | 'lg';
  showLabel?: boolean;
  isScrolled?: boolean;
}

export function ThemeToggle({ className = '', size = 'md', showLabel = false, isScrolled = false }: ThemeToggleProps) {
  const { theme, toggleTheme } = useTheme();
  const isDark = theme === 'dark';

  return (
    <div className={`flex items-center gap-2 ${className}`}>
      {showLabel && (
        <span className={`text-sm font-medium ${isScrolled ? 'text-white/90' : 'text-primary/90'}`}>
          {isDark ? 'Dark' : 'Light'}
        </span>
      )}
      
      <label className="flex cursor-pointer gap-2 items-center">
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          className={`transition-colors ${
            isScrolled 
              ? (isDark ? 'text-white/50' : 'text-white') 
              : (isDark ? 'text-primary/50' : 'text-primary')
          }`}
        >
          <circle cx="12" cy="12" r="5" />
          <path d="M12 1v2M12 21v2M4.2 4.2l1.4 1.4M18.4 18.4l1.4 1.4M1 12h2M21 12h2M4.2 19.8l1.4-1.4M18.4 5.6l1.4-1.4" />
        </svg>
        <input 
          type="checkbox" 
          checked={isDark}
          onChange={toggleTheme}
          className={`toggle theme-controller transition-all duration-200 ${
            isScrolled 
              ? 'bg-gray-300 border-gray-300 [--tglbg:theme(colors.gray.400)] checked:bg-gray-400 checked:border-gray-400' 
              : 'bg-blue-200 border-blue-200 [--tglbg:theme(colors.blue.400)] checked:bg-blue-400 checked:border-blue-400'
          }`}
        />
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          className={`transition-colors ${
            isScrolled 
              ? (isDark ? 'text-white' : 'text-white/50') 
              : (isDark ? 'text-primary' : 'text-primary/50')
          }`}
        >
          <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"></path>
        </svg>
      </label>
    </div>
  );
}

// Alternative simpler version - Following Occam's Razor principle
export function SimpleThemeToggle({ className = '', isScrolled = false }: { className?: string; isScrolled?: boolean }) {
  const { theme, toggleTheme } = useTheme();
  const isDark = theme === 'dark';

  return (
    <label className={`flex cursor-pointer gap-3 items-center ${className}`}>
      {/* Sun Icon */}
      <svg
        xmlns="http://www.w3.org/2000/svg"
        width="20"
        height="20"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        className={`transition-colors duration-200 ${
          isScrolled 
            ? (isDark ? 'text-white/50' : 'text-white') 
            : (isDark ? 'text-primary/50' : 'text-primary')
        }`}
      >
        <circle cx="12" cy="12" r="5" />
        <path d="M12 1v2M12 21v2M4.2 4.2l1.4 1.4M18.4 18.4l1.4 1.4M1 12h2M21 12h2M4.2 19.8l1.4-1.4M18.4 5.6l1.4-1.4" />
      </svg>

      {/* Toggle Switch */}
      <input
        type="checkbox"
        checked={isDark}
        onChange={toggleTheme}
        className={`toggle w-12 h-6 rounded-full transition-all duration-200 focus:ring-2 ${
          isScrolled 
            ? 'bg-gray-300 border-gray-300 checked:bg-gray-400 checked:border-gray-400 focus:ring-white/30' 
            : 'bg-blue-200 border-blue-200 checked:bg-blue-400 checked:border-blue-400 focus:ring-primary/30'
        }`}
      />

      {/* Moon Icon */}
      <svg
        xmlns="http://www.w3.org/2000/svg"
        width="20"
        height="20"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        className={`transition-colors duration-200 ${
          isScrolled 
            ? (isDark ? 'text-white' : 'text-white/50') 
            : (isDark ? 'text-primary' : 'text-primary/50')
        }`}
      >
        <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
      </svg>
    </label>
  );
}