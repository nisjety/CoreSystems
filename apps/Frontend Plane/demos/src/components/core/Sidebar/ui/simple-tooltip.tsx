'use client';

import React, { ReactNode, useRef, useState } from 'react';
import { cn } from '../utils';

interface SimpleTooltipProps {
  children: ReactNode;
  content: string;
  placement?: 'top' | 'right' | 'bottom' | 'left';
  delay?: number;
}

export function SimpleTooltip({ 
  children, 
  content, 
  placement = 'top',
  delay = 200 
}: SimpleTooltipProps) {
  const [isVisible, setIsVisible] = useState(false);
  const [timeoutId, setTimeoutId] = useState<NodeJS.Timeout | null>(null);
  const [position, setPosition] = useState({ x: 0, y: 0 });
  const containerRef = useRef<HTMLDivElement>(null);

  const handleMouseEnter = () => {
    if (timeoutId) {
      clearTimeout(timeoutId);
    }
    const id = setTimeout(() => {
      if (containerRef.current) {
        const rect = containerRef.current.getBoundingClientRect();
        const offset = 8; // Distance from the trigger element
        
        let x = 0;
        let y = 0;
        
        switch (placement) {
          case 'top':
            x = rect.left + rect.width / 2;
            y = rect.top - offset;
            break;
          case 'right':
            x = rect.right + offset;
            y = rect.top + rect.height / 2;
            break;
          case 'bottom':
            x = rect.left + rect.width / 2;
            y = rect.bottom + offset;
            break;
          case 'left':
            x = rect.left - offset;
            y = rect.top + rect.height / 2;
            break;
        }
        
        setPosition({ x, y });
      }
      setIsVisible(true);
    }, delay);
    setTimeoutId(id);
  };

  const handleMouseLeave = () => {
    if (timeoutId) {
      clearTimeout(timeoutId);
      setTimeoutId(null);
    }
    setIsVisible(false);
  };

  const getTooltipClasses = () => {
    const baseClasses = "fixed z-[9999] px-2 py-1 text-xs font-medium text-white bg-gray-900 rounded-md shadow-lg pointer-events-none transition-opacity duration-200 whitespace-nowrap";
    
    switch (placement) {
      case 'top':
        return cn(baseClasses, "transform -translate-x-1/2 -translate-y-full");
      case 'right':
        return cn(baseClasses, "transform -translate-y-1/2");
      case 'bottom':
        return cn(baseClasses, "transform -translate-x-1/2");
      case 'left':
        return cn(baseClasses, "transform -translate-y-1/2 -translate-x-full");
      default:
        return baseClasses;
    }
  };

  const getArrowClasses = () => {
    // Simplified for fixed positioning - no arrow for now
    return "hidden";
  };

  return (
    <div 
      ref={containerRef}
      className="relative inline-block"
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
    >
      {children}
      {isVisible && (
        <div 
          className={getTooltipClasses()}
          style={{
            left: position.x,
            top: position.y
          }}
        >
          {content}
          <div className={getArrowClasses()} />
        </div>
      )}
    </div>
  );
}
