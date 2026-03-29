'use client';

import { LoaderCircle, LogOut, Search, Settings } from 'lucide-react';
import { cn } from '../utils';

interface ExpandedFooterProps {
  userName?: string;
  onGetStartedClick?: () => void;
  onSearchClick?: () => void;
  onSettingsClick?: () => void;
  onProfileClick?: () => void;
  onLogoutClick?: () => void;
  isSettingsActive?: boolean;
}

export function ExpandedFooter({
  userName,
  onGetStartedClick,
  onSearchClick,
  onSettingsClick,
  onProfileClick,
  onLogoutClick,
  isSettingsActive = false,
}: ExpandedFooterProps) {
  const profileInitial = (userName?.trim()?.charAt(0) || 'U').toUpperCase();

  return (
    <div className="space-y-3">
      <div className="space-y-1 rounded-[22px] border border-white/10 bg-[#17181D] p-2 shadow-[inset_0_1px_0_rgba(255,255,255,0.03)]">
        <button
          onClick={onGetStartedClick}
          className="flex h-11 w-full items-center gap-3 rounded-2xl px-3 text-white transition-colors hover:bg-white/6"
          aria-label="Get set up"
        >
          <span className="flex h-8 w-8 items-center justify-center rounded-xl border border-white/10 bg-[#2A241E] text-[#DD7A1F]">
            <LoaderCircle className="h-4 w-4" />
          </span>
          <div className="text-left">
            <div className="text-[10px] uppercase tracking-[0.24em] text-white/34">Workspace</div>
            <div className="text-[14px] font-semibold text-white">Get set up</div>
          </div>
        </button>

        <button
          onClick={onSearchClick}
          className="flex h-11 w-full items-center gap-3 rounded-2xl px-3 text-white transition-colors hover:bg-white/6"
          aria-label="Search"
        >
          <span className="flex h-8 w-8 items-center justify-center rounded-xl border border-white/10 bg-white/6 text-white/55">
            <Search className="h-4 w-4" />
          </span>
          <span className="text-[14px] font-semibold text-white">Search</span>
          <div className="ml-auto flex items-center gap-1.5">
            <span className="flex h-7 min-w-7 items-center justify-center rounded-lg border border-white/10 bg-white/6 px-2 text-xs font-medium text-white/45">
              ⌘
            </span>
            <span className="flex h-7 min-w-7 items-center justify-center rounded-lg border border-white/10 bg-white/6 px-2 text-xs font-medium text-white/45">
              K
            </span>
          </div>
        </button>

        <button
          onClick={onSettingsClick}
          className={cn(
            'flex h-11 w-full items-center gap-3 rounded-2xl px-3 transition-colors',
            isSettingsActive ? 'bg-white/10 text-white' : 'text-white hover:bg-white/6'
          )}
          aria-label="Settings"
        >
          <span className={cn(
            'flex h-8 w-8 items-center justify-center rounded-xl border',
            isSettingsActive ? 'border-white/10 bg-white/10 text-white' : 'border-white/10 bg-white/6 text-white/60'
          )}>
            <Settings className="h-4 w-4" />
          </span>
          <span className="text-[14px] font-semibold">Settings</span>
        </button>

        <button
          onClick={onProfileClick}
          className="flex h-11 w-full items-center gap-3 rounded-2xl px-3 text-white transition-colors hover:bg-white/6"
          aria-label="Profile"
        >
          <span className="relative flex h-8 w-8 items-center justify-center rounded-full border border-white/10 bg-white/6 text-[11px] font-semibold text-white/65">
            {profileInitial}
            <span className="absolute -bottom-0.5 -right-0.5 w-2.5 h-2.5 rounded-full bg-[#16A34A] border border-white" />
          </span>
          <span className="text-[14px] font-semibold text-white">Profile</span>
        </button>

        <button
          onClick={onLogoutClick}
          className="flex h-11 w-full items-center gap-3 rounded-2xl px-3 text-[#B42318] transition-colors hover:bg-[#FEE4E2] hover:text-[#912018]"
          aria-label="Log out"
        >
          <span className="flex h-8 w-8 items-center justify-center rounded-xl border border-[#FECACA] bg-[#FEE4E2]">
            <LogOut className="h-4 w-4" />
          </span>
          <span className="text-[14px] font-semibold">Log out</span>
        </button>
      </div>
    </div>
  );
}
