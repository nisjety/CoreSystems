export interface Message {
  id: string;
  content: string;
  sender: 'user' | 'ai';
  timestamp: Date;
  isTyping?: boolean;
  role: 'user' | 'assistant'; // make required to align with server ChatMessage
}

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  timestamp: Date;
  attachments?: FileAttachment[];
}

export interface FileAttachment {
  id: string;
  name: string;
  mimeType: string;
  url?: string;
  extractedText?: string;
}

export interface Chat {
  id: string;
  title: string;
  lastMessage?: string; // Made optional for compatibility
  timestamp: Date;
  messages: ChatMessage[]; // unified format
  category?: 'today' | 'yesterday' | 'older';
  userId?: string;
}

export interface WeatherData {
  location: string;
  temperature: number;
  condition: string;
  high: number;
  low: number;
  emoji: string;
}

export interface User {
  name: string;
  avatar?: string;
  plan: 'free' | 'pro' | 'premium';
}

export type ViewState = 'dashboard' | 'chat';

export interface ChatContextValue {
  chats: Chat[];
  currentChat: Chat | null;
  isTyping: boolean;
  user: User;
  setCurrentChat: (chat: Chat | null) => void;
  createNewChat: (message?: string) => void;
  sendMessage: (content: string) => Promise<void>;
  deleteChat: (chatId: string) => void;
}

export interface QuickAction {
  id: string;
  title: string;
  description: string;
  icon: string;
  action: () => void;
}