import React from 'react';
import { cn } from '../utils/cn';

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'default' | 'outline' | 'ghost' | 'secondary';
  size?: 'sm' | 'md' | 'lg';
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = 'default', size = 'md', ...props }, ref) => {
    return (
      <button
        className={cn(
          // Base styles - Following Fitts's Law for adequate touch targets
          'inline-flex items-center justify-center rounded-xl font-semibold transition-all duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white/30 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent disabled:pointer-events-none disabled:opacity-50 active:scale-95',
          {
            // Primary variant - using the specified primary color
            'bg-primary text-white hover:bg-primary/90 shadow-lg hover:shadow-xl': variant === 'default',
            // Outline variant - clean white border
            'border-2 border-white/20 bg-transparent text-white hover:bg-white/10 hover:border-white/30': variant === 'outline',
            // Ghost variant - subtle hover states
            'hover:bg-white/10 text-white': variant === 'ghost',
            // Secondary variant - white background
            'bg-white text-primary hover:bg-white/90 shadow-md hover:shadow-lg': variant === 'secondary',
          },
          {
            // Size variants - Following Miller's Law with clear size differences
            'h-8 px-3 text-sm': size === 'sm',
            'h-9 px-4 text-sm': size === 'md',
            'h-10 px-6 text-base': size === 'lg',
          },
          className
        )}
        ref={ref}
        {...props}
      />
    );
  }
);

Button.displayName = 'Button';