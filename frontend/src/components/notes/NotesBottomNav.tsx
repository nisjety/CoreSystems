'use client';

import { Button } from '@/components/ui/button';
import type { LucideIcon } from 'lucide-react';
import {
  CalendarDays,
  Mail,
  NotebookPen,
  Settings,
  StickyNote,
} from 'lucide-react';

interface NotesBottomNavItem {
  id: string;
  label: string;
  icon: LucideIcon;
}

interface NotesBottomNavProps {
  items?: ReadonlyArray<NotesBottomNavItem>;
  activeId?: string;
}

export const DEFAULT_NOTES_BOTTOM_NAV_ITEMS: ReadonlyArray<NotesBottomNavItem> = [
  { id: 'worklog', label: 'Worklog', icon: NotebookPen },
  { id: 'email', label: 'Email', icon: Mail },
  { id: 'meetings', label: 'Meetings', icon: CalendarDays },
  { id: 'notes', label: 'Notes', icon: StickyNote },
  { id: 'setup', label: 'Setup', icon: Settings },
] as const;

export function NotesBottomNav({
  items = DEFAULT_NOTES_BOTTOM_NAV_ITEMS,
  activeId = 'notes',
}: NotesBottomNavProps) {
  return (
    <div className="flex w-full justify-center">
      <div className="flex gap-1.5 rounded-full border border-slate-200 bg-white p-1 shadow-sm">
        {items.map(({ id, label, icon: Icon }) => {
          const isActive = id === activeId;

          return (
            <Button
              key={id}
              variant="ghost"
              className={`flex items-center gap-2 rounded-full px-4 py-2 text-sm font-medium ${
                isActive
                  ? 'bg-slate-900 text-white hover:bg-slate-900'
                  : 'text-slate-600 hover:bg-slate-100'
              }`}
            >
              <Icon className="h-4 w-4" />
              {label}
            </Button>
          );
        })}
      </div>
    </div>
  );
}
