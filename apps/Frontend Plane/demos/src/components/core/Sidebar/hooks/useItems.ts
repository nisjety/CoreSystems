// Re-export hooks from useRealData for backwards compatibility
export {
  useUser as useItems,
  useUser as useSidebarUser,
  useMessages as useSidebarMessages,
  useCalendarEvents as useSidebarCalendar,
  useGraphProfile,
  useGraphMessages,
  useGraphCalendar,
  useGraphChats,
  useAppearanceSettings,
  useUpdateAppearanceSettings,
  useLanguageSettings,
  useUpdateLanguageSettings,
  usePrivacySettings,
  useUpdatePrivacySettings,
  useNotificationSettings,
  useUpdateNotificationSettings,
} from './useRealData';

// Notification hooks (can be enhanced later)
export function useSidebarNotifications() {
  // For now, notifications are the same as messages
  // This can be expanded to a separate endpoint later
  return useMessages();
}

// Import the real data hooks
import { useMessages } from './useRealData';
