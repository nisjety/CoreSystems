'use client';

import React from 'react';
import { SimpleTooltip } from '../ui/simple-tooltip';

interface MinimizedFooterProps {
  userName?: string;
  onProfileClick?: () => void;
  avatarButtonRef?: React.RefObject<HTMLButtonElement | null>;
}

export function MinimizedFooter({ 
  userName,
  onProfileClick,
  avatarButtonRef,
}: MinimizedFooterProps) {
  const profileInitial = (userName?.trim()?.charAt(0) || 'U').toUpperCase();

  return (
    <div className="flex flex-col items-center">
      <SimpleTooltip content="Account & workspace" placement="right" delay={200}>
        <button
          ref={avatarButtonRef}
          onClick={onProfileClick}
          className="relative flex h-9 w-9 items-center justify-center rounded-full bg-[#F5DFA0] text-[11px] font-semibold text-[#4A3A12] transition-all hover:scale-105 focus:outline-none"
          aria-label="Account menu"
          aria-haspopup="menu"
        >
          {profileInitial}
          <span className="absolute -bottom-0.5 -right-0.5 h-2.5 w-2.5 rounded-full border-2 border-[#F6F5F3] bg-[#34C77B]" />
        </button>
      </SimpleTooltip>
    </div>
  );
}
