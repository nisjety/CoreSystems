'use client';

import React from 'react';
import { useRouter } from 'next/navigation';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  User,
  Users,
  CreditCard,
  Settings,
  HelpCircle,
  LogOut,
  Zap,
} from 'lucide-react';
import { cn } from '@/lib/utils';

interface ProfileDropdownProps {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  trigger: React.ReactNode;
  displayName?: string;
  email?: string;
  onSignOut?: () => void;
}

interface MenuItem {
  id: string;
  label: string;
  icon: React.ReactNode;
  href?: string;
  onClick?: () => void;
  badge?: React.ReactNode;
  danger?: boolean;
}

export function ProfileDropdown({
  isOpen,
  onOpenChange,
  trigger,
  displayName,
  email,
  onSignOut,
}: ProfileDropdownProps) {
  const router = useRouter();
  const [hoveredParent, setHoveredParent] = React.useState(false);

  React.useEffect(() => {
    if (hoveredParent && !isOpen) {
      onOpenChange(true);
    } else if (!hoveredParent && isOpen) {
      onOpenChange(false);
    }
  }, [hoveredParent, isOpen, onOpenChange]);

  const topItems: MenuItem[] = [
    {
      id: 'profile',
      label: 'Profile',
      icon: <User className="h-[17px] w-[17px]" strokeWidth={1.7} />,
      href: '/profile',
    },
    {
      id: 'community',
      label: 'Community',
      icon: <Users className="h-[17px] w-[17px]" strokeWidth={1.7} />,
      href: '/community',
    },
    {
      id: 'subscription',
      label: 'Subscription',
      icon: <CreditCard className="h-[17px] w-[17px]" strokeWidth={1.7} />,
      href: '/settings/subscription',
      badge: (
        <span className="inline-flex items-center gap-0.5 rounded-md bg-[#E9D5FF] px-2 py-0.5 text-[10px] font-bold text-[#7C3AED]">
          <Zap className="h-2.5 w-2.5 fill-[#7C3AED] stroke-none" />
          PRO
        </span>
      ),
    },
    {
      id: 'settings',
      label: 'Settings',
      icon: <Settings className="h-[17px] w-[17px]" strokeWidth={1.7} />,
      href: '/settings',
    },
  ];

  const bottomItems: MenuItem[] = [
    {
      id: 'help',
      label: 'Help center',
      icon: <HelpCircle className="h-[17px] w-[17px]" strokeWidth={1.7} />,
      href: '/help',
    },
    {
      id: 'signout',
      label: 'Sign out',
      icon: <LogOut className="h-[17px] w-[17px]" strokeWidth={1.7} />,
      onClick: onSignOut,
      danger: false,
    },
  ];

  const handleItemClick = (item: MenuItem, e: React.MouseEvent) => {
    e.stopPropagation();
    onOpenChange(false);
    if (item.onClick) {
      item.onClick();
    } else if (item.href) {
      router.push(item.href);
    }
  };

  return (
    <div
      onMouseEnter={() => setHoveredParent(true)}
      onMouseLeave={() => setHoveredParent(false)}
    >
      <DropdownMenu open={isOpen} onOpenChange={onOpenChange}>
        <DropdownMenuTrigger asChild id="dashboard-profile-trigger">{trigger}</DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          side="bottom"
          sideOffset={10}
          onClick={(e) => e.stopPropagation()}
          className={cn(
            'w-[280px] overflow-hidden rounded-2xl',
            'border border-[#E9EBF2] bg-white shadow-[0_20px_80px_rgba(17,17,17,0.15)]',
            'p-2'
          )}
        >
          {(displayName || email) ? (
            <div className="mb-2 rounded-xl bg-[#F7F7F8] px-3 py-2.5">
              {displayName ? (
                <div className="truncate text-[13px] font-semibold text-[#111111]">{displayName}</div>
              ) : null}
              {email ? (
                <div className="mt-0.5 truncate text-[12px] text-[#777777]">{email}</div>
              ) : null}
            </div>
          ) : null}

          {/* Top group */}
          <div className="mb-1">
            {topItems.map((item) => (
              <button
                key={item.id}
                onClick={(e) => handleItemClick(item, e)}
                className={cn(
                  'flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors',
                  'text-[13px] font-medium text-[#111111]',
                  'hover:bg-[#F5F5F5] focus:outline-none'
                )}
              >
                <span className="text-[#555555]">{item.icon}</span>
                <span className="flex-1">{item.label}</span>
                {item.badge}
              </button>
            ))}
          </div>

          {/* Divider */}
          <div className="my-1 h-px bg-[#EBEBEB]" />

          {/* Bottom group */}
          <div className="mt-1">
            {bottomItems.map((item) => (
              <button
                key={item.id}
                onClick={(e) => handleItemClick(item, e)}
                className={cn(
                  'flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors',
                  'text-[13px] font-medium text-[#111111]',
                  'hover:bg-[#F5F5F5] focus:outline-none'
                )}
              >
                <span className="text-[#555555]">{item.icon}</span>
                <span className="flex-1">{item.label}</span>
              </button>
            ))}
          </div>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
