'use client';

import React from 'react';
import { cn } from '@/lib/utils';
import { SimpleTooltip } from '@/components/core/sidebar/ui/simple-tooltip';

interface NavbarActionButtonProps {
  label: string;
  tooltip?: string;
  onClick?: () => void;
  children: React.ReactNode;
  className?: string;
  disabled?: boolean;
  active?: boolean;
}

export const NavbarActionButton = React.forwardRef<
  HTMLButtonElement,
  NavbarActionButtonProps
>(({
  label,
  tooltip,
  onClick,
  children,
  className,
  disabled = false,
  active = false,
}, ref) => {
  const button = (
    <button
      ref={ref}
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={label}
      title={tooltip || label}
      className={cn(
        'flex h-9 w-9 items-center justify-center rounded-[10px] bg-transparent text-[#6F737C] shadow-none transition-[background-color,color,box-shadow] duration-200 motion-safe:transition-transform motion-safe:hover:scale-[1.04] hover:bg-white hover:text-[#383B43] hover:shadow-[0_10px_24px_rgba(17,17,17,0.12)] active:bg-[#F5F5F5] active:shadow-[0_4px_12px_rgba(17,17,17,0.08)] motion-safe:active:scale-[0.97] focus:outline-none motion-safe:focus-visible:scale-[1.04] focus-visible:bg-white focus-visible:text-[#383B43] focus-visible:ring-2 focus-visible:ring-[#111111]/30 focus-visible:shadow-[0_10px_24px_rgba(17,17,17,0.12)] disabled:opacity-50 disabled:cursor-not-allowed',
        active && 'bg-white text-[#383B43] shadow-[0_10px_24px_rgba(17,17,17,0.12)]',
        className,
      )}
    >
      {children}
    </button>
  );

  if (tooltip) {
    return (
      <SimpleTooltip content={tooltip} placement="bottom">
        {button}
      </SimpleTooltip>
    );
  }

  return button;
});

NavbarActionButton.displayName = 'NavbarActionButton';
