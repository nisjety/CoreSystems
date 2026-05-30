// Type definitions for Sidebar component

export interface User {
  id: string;
  name: string;
  email: string;
  position?: string;
  department?: string;
  avatar?: string;
  status: 'online' | 'away' | 'busy' | 'offline';
}

export interface Message {
  id: string;
  from: {
    name: string;
    email: string;
  };
  content: string;
  timestamp: Date;
  read: boolean;
  priority: 'low' | 'normal' | 'high' | 'medium';
  type: 'teams' | 'email' | 'chat';
  sourceHref?: string;
  sourceLabel?: string;
  relatedChatId?: string;
  relatedRoute?: string;
  relatedRouteLabel?: string;
}

export interface CalendarEvent {
  id: string;
  title: string;
  start: Date;
  end: Date;
  location?: string;
  description?: string;
  attendees: string[];
  status: 'upcoming' | 'ongoing' | 'past';
  type?: 'meeting' | 'event' | 'reminder';
  meetingUrl?: string;
  sourceHref?: string;
  sourceLabel?: string;
  relatedChatId?: string;
  relatedRoute?: string;
  relatedRouteLabel?: string;
}

export interface Notification {
  id: string;
  title: string;
  description: string;
  timestamp: Date;
  read: boolean;
  priority: 'low' | 'normal' | 'high' | 'medium';
  type: 'email' | 'teams' | 'calendar' | 'system';
  sourceHref?: string;
  sourceLabel?: string;
  relatedChatId?: string;
  relatedRoute?: string;
  relatedRouteLabel?: string;
}

export interface NavigationItem {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  href: string;
  badge?: number;
  description?: string;
  status?: 'live' | 'coming-soon';
  aliases?: string[];
}

export interface SidebarProps {
  user?: User;
  onNavigate?: (item: NavigationItem) => void;
  className?: string;
}
