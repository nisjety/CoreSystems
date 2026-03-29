'use client';

import React from 'react';
import { SidebarProvider } from './SidebarContext';

interface AppLayoutProps {
  children: React.ReactNode;
}

export const AppLayout: React.FC<AppLayoutProps> = ({ children }) => {
  return (
    <SidebarProvider>
      {children}
    </SidebarProvider>
  );
};
