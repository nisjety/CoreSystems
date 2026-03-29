import { type ClassValue, clsx } from 'clsx';
import { twMerge } from 'tailwind-merge';
import { formatDistanceToNow, isToday, isYesterday } from 'date-fns';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatChatTime(date: Date): string {
  if (isToday(date)) {
    return formatDistanceToNow(date, { addSuffix: true });
  }
  
  if (isYesterday(date)) {
    return 'Yesterday';
  }
  
  return formatDistanceToNow(date, { addSuffix: true });
}

export function getChatCategory(date: Date): 'today' | 'yesterday' | 'older' {
  if (isToday(date)) return 'today';
  if (isYesterday(date)) return 'yesterday';
  return 'older';
}

export function generateChatTitle(message: string): string {
  const words = message.trim().split(' ');
  if (words.length <= 6) return message;
  return words.slice(0, 6).join(' ') + '...';
}

// Time-ago formatting that accepts a translation function
export function formatTimeAgoWith(t: (key: string, vars?: Record<string, string | number>) => string, dateString: string | Date): string {
  const date = typeof dateString === 'string' ? new Date(dateString) : dateString;
  const now = new Date();
  const diffInHours = Math.floor((now.getTime() - date.getTime()) / (1000 * 60 * 60));
  if (diffInHours < 1) return t('news.justNow');
  if (diffInHours < 24) return t('news.hoursAgo', { count: diffInHours });
  const days = Math.floor(diffInHours / 24);
  return t('news.daysAgo', { count: days });
}

// Legacy function - kept for backward compatibility but should be avoided in React components
export function formatTimeAgoLocalized(dateString: string | Date): string {
  // This function should not be used in React components that need reactive language switching
  // Use formatTimeAgoWith with useT() hook instead
  console.warn('formatTimeAgoLocalized is deprecated for React components. Use formatTimeAgoWith with useT() hook.');
  
  const date = typeof dateString === 'string' ? new Date(dateString) : dateString;
  const now = new Date();
  const diffInHours = Math.floor((now.getTime() - date.getTime()) / (1000 * 60 * 60));
  if (diffInHours < 1) return 'Just now';
  if (diffInHours < 24) return `${diffInHours} hours ago`;
  const days = Math.floor(diffInHours / 24);
  return `${days} days ago`;
}

export function simulateTypingDelay(): number {
  return 1000 + Math.random() * 2000; // 1-3 seconds
}

export const AI_RESPONSES = [
  "I'd be happy to help you with that! Could you provide more details about what you're looking for?",
  "That's a great question! Let me think about the best way to approach this.",
  "I can definitely assist with that. Here are a few suggestions to get started:",
  "Interesting! I have some ideas that might work well for your situation.",
  "Let me help you break this down into manageable steps.",
  "Thanks for sharing that with me. I think I can provide some helpful insights.",
  "That's an excellent point. Here's what I would recommend:",
  "I understand what you're asking. Let me walk you through this step by step.",
] as const;