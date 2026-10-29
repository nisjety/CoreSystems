'use client';

import { HelpCircle, LogOut } from 'lucide-react';
import { SimpleTooltip } from '../ui/simple-tooltip';
import { cn } from '../utils';

interface MinimizedFooterProps {
  onProfileClick?: () => void;
  onHelpClick?: () => void;
  onSettingsClick?: () => void;
  onLogoutClick?: () => void;
  isSettingsActive?: boolean;
}

export function MinimizedFooter({ 
  onProfileClick, 
  onHelpClick, 
  onSettingsClick,
  onLogoutClick,
  isSettingsActive 
}: MinimizedFooterProps) {
  return (
    <div className="flex flex-col items-center gap-4">
      <SimpleTooltip content="Help" placement="right" delay={200}>
        <button 
          onClick={onHelpClick}
          className="w-12 h-12 flex items-center justify-center rounded-lg text-gray-400 hover:bg-gray-100 hover:text-blue-600 transition-all" 
          aria-label="Help"
        >
          <HelpCircle className="w-6 h-6" />
        </button>
      </SimpleTooltip>
      
      <SimpleTooltip content="Logout" placement="right" delay={200}>
        <button 
          onClick={onLogoutClick}
          className="w-12 h-12 flex items-center justify-center rounded-lg text-gray-400 hover:bg-gray-100 hover:text-red-600 transition-all" 
          aria-label="Logout"
        >
          <LogOut className="w-6 h-6" />
        </button>
      </SimpleTooltip>
      
      <SimpleTooltip content="Profile" placement="right" delay={200}>
        <button 
          onClick={onProfileClick}
          className="w-12 h-12 flex items-center justify-center rounded-full bg-blue-500 hover:bg-blue-600 transition-all text-white font-semibold" 
          aria-label="Profile"
        >
          U
        </button>
      </SimpleTooltip>
    </div>
  );
}
