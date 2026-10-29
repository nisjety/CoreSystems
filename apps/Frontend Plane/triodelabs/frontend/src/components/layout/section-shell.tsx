"use client";

import { cn } from '@/lib/utils';

interface SectionShellProps {
  children: React.ReactNode;
  className?: string;
  as?: keyof JSX.IntrinsicElements;
}

export function SectionShell({ 
  children, 
  className, 
  as: Component = 'div' 
}: SectionShellProps) {
  return (
    <Component className={cn('section-shell', className)}>
      {children}
    </Component>
  );
}