'use client';

import React, { ReactNode, useCallback, useEffect, useId, useRef, useState } from 'react';
import { cn } from '../utils';

type TooltipPlacement = 'top' | 'right' | 'bottom' | 'left';

interface SimpleTooltipProps {
  children: ReactNode;
  content: string;
  placement?: TooltipPlacement;
  delay?: number;
  containerClassName?: string;
}

const OFFSET = 10;

function TooltipCaret({ placement }: { placement: TooltipPlacement }) {
  if (placement === 'top') {
    return (
      <div className="absolute top-full left-1/2 h-2 w-4 -translate-x-1/2 overflow-hidden">
        <div className="absolute -top-1 left-1/2 h-2.5 w-2.5 -translate-x-1/2 rotate-45 bg-[#1a1a1a]" />
      </div>
    );
  }

  if (placement === 'bottom') {
    return (
      <div className="absolute bottom-full left-1/2 h-2 w-4 -translate-x-1/2 overflow-hidden">
        <div className="absolute -bottom-1 left-1/2 h-2.5 w-2.5 -translate-x-1/2 rotate-45 bg-[#1a1a1a]" />
      </div>
    );
  }

  if (placement === 'right') {
    return (
      <div className="absolute right-full top-1/2 h-4 w-2 -translate-y-1/2 overflow-hidden">
        <div className="absolute -right-1 top-1/2 h-2.5 w-2.5 -translate-y-1/2 rotate-45 bg-[#1a1a1a]" />
      </div>
    );
  }

  return (
    <div className="absolute left-full top-1/2 h-4 w-2 -translate-y-1/2 overflow-hidden">
      <div className="absolute -left-1 top-1/2 h-2.5 w-2.5 -translate-y-1/2 rotate-45 bg-[#1a1a1a]" />
    </div>
  );
}

export function SimpleTooltip({
  children,
  content,
  placement = 'top',
  delay = 200,
  containerClassName,
}: SimpleTooltipProps) {
  const [isVisible, setIsVisible] = useState(false);
  const [position, setPosition] = useState({ x: 0, y: 0 });
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const tooltipId = useId();

  const updatePosition = useCallback(() => {
    if (!containerRef.current) {
      return;
    }

    const rect = containerRef.current.getBoundingClientRect();

    switch (placement) {
      case 'top':
        setPosition({ x: rect.left + rect.width / 2, y: rect.top - OFFSET });
        break;
      case 'right':
        setPosition({ x: rect.right + OFFSET, y: rect.top + rect.height / 2 });
        break;
      case 'bottom':
        setPosition({ x: rect.left + rect.width / 2, y: rect.bottom + OFFSET });
        break;
      case 'left':
        setPosition({ x: rect.left - OFFSET, y: rect.top + rect.height / 2 });
        break;
    }
  }, [placement]);

  const handleMouseEnter = () => {
    timerRef.current = setTimeout(() => {
      updatePosition();
      setIsVisible(true);
    }, delay);
  };

  const handleMouseLeave = () => {
    if (timerRef.current) { clearTimeout(timerRef.current); timerRef.current = null; }
    setIsVisible(false);
  };

  // Transform to centre the bubble on the anchor point
  const transformClass: Record<string, string> = {
    top:    '-translate-x-1/2 -translate-y-full',
    right:  '-translate-y-1/2',
    bottom: '-translate-x-1/2',
    left:   '-translate-x-full -translate-y-1/2',
  };

  useEffect(() => {
    if (!isVisible) {
      return;
    }

    window.addEventListener('scroll', updatePosition, true);
    window.addEventListener('resize', updatePosition);

    return () => {
      window.removeEventListener('scroll', updatePosition, true);
      window.removeEventListener('resize', updatePosition);
    };
  }, [isVisible, updatePosition]);

  return (
    <div
      ref={containerRef}
      className={cn('relative inline-block', containerClassName)}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      aria-describedby={isVisible ? tooltipId : undefined}
    >
      {children}
      {isVisible && (
        <div
          id={tooltipId}
          role="tooltip"
          className={cn(
            'fixed z-[9999] pointer-events-none',
            transformClass[placement],
          )}
          style={{ left: position.x, top: position.y }}
        >
          <div className="relative px-3.5 py-2 rounded-[12px] bg-[#1a1a1a] text-white text-[13px] font-semibold whitespace-nowrap shadow-xl">
            {content}
            <TooltipCaret placement={placement} />
          </div>
        </div>
      )}
    </div>
  );
}
