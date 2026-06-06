export type NavbarProfile = {
  id: string;
  name: string;
  email?: string;
  avatar?: string;
  status: string;
};

export type NavbarNotification = {
  id: string;
  title: string;
  body: string;
  href?: string;
  createdAt?: string;
  read: boolean;
  seen: boolean;
  archived: boolean;
  feed?: string;
  source: "notification" | "message";
};

export type NotificationPayload = {
  configured: boolean;
  unreadCount: number;
  notifications: NavbarNotification[];
  messages: NavbarNotification[];
};

export type CalendarEvent = {
  id: string;
  title: string;
  start: string;
  end: string;
  type: string;
  status: string;
  createdAt: string;
};

export type CalendarNote = {
  id: string;
  text: string;
  date: string;
  createdAt: string;
};

export type CalendarState = {
  events: CalendarEvent[];
  notes: CalendarNote[];
};

export type ThemePayload = {
  colorScheme?: string | null;
  configured?: boolean;
  theme: "light" | "dark" | "system";
};

export type NavbarPayload = {
  calendar: CalendarState;
  notifications: NotificationPayload;
  profile: NavbarProfile | null;
  theme: ThemePayload | null;
};

export const emptyNotifications: NotificationPayload = {
  configured: false,
  unreadCount: 0,
  notifications: [],
  messages: [],
};

export const emptyCalendar: CalendarState = {
  events: [],
  notes: [],
};

export const emptyNavbarPayload: NavbarPayload = {
  calendar: emptyCalendar,
  notifications: emptyNotifications,
  profile: null,
  theme: null,
};
