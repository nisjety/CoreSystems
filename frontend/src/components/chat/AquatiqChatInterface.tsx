'use client';

import { useState } from 'react';
import { useConvexChat } from '@/components/chat/hooks/useConvexChat';
import type { Chat as UIChat } from './types';
import type { Chat as ApiChat } from '@/components/chat/lib/chat-server-api';
import { ChatView } from './components/ChatView';
import { RecentChatsStrip } from './components/RecentChatsStrip';
import { m, AnimatePresence } from 'framer-motion';
import { Sparkles, MessageSquare, Plus } from 'lucide-react';
import { getDefaultModel } from './components/ModelSelector';
import { useI18n } from '@/components/chat/hooks/i18n';
import { useQuery } from 'convex/react';

interface AquatiqChatInterfaceProps {
  userId?: string;
  userName?: string;
  userEmail?: string;
  className?: string;
}

export function AquatiqChatInterface({
  userId = 'anonymous',
  userName,
  userEmail,
  className = '',
}: AquatiqChatInterfaceProps) {
  const {
    chats,
    currentChat,
    isLoading,
    isTyping,
    error,
    sendMessage,
    selectChat,
    startNewChat,
    setError,
  } = useConvexChat({ userId, userName, userEmail });

  const { t } = useI18n();
  const [message, setMessage] = useState('');
  const [selectedModel] = useState(getDefaultModel(t));


  const handleMessageSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!message.trim() || isTyping) return;

    const messageToSend = message;
    setMessage(''); // Clear input immediately

    try {
      await sendMessage(messageToSend, selectedModel);
    } catch (err) {
      // Error handling is done in useChat hook
      console.error('Failed to send message:', err);
    }
  };

  type ChatLike = UIChat & ApiChat;
  const handleChatClick = (chat: ChatLike) => {
    selectChat(chat);
  };

  const handleNewChat = () => {
    startNewChat();
  };

  const handleBack = () => {
    startNewChat();
  };

  const showWelcomeScreen = !currentChat || currentChat.messages.length === 0;

  return (
    <div className={`h-full ${className}`}>
      {/* Mobile/Desktop Layout */}
      <div className="h-full flex">
        {/* Sidebar - Hidden on mobile when chat is active, visible on desktop */}
        <AnimatePresence mode="wait">
          {(!currentChat || window.innerWidth >= 1024) && (
            <m.div
              initial={{ width: 0, opacity: 0 }}
              animate={{ width: 320, opacity: 1 }}
              exit={{ width: 0, opacity: 0 }}
              transition={{ duration: 0.3 }}
              className="hidden lg:flex flex-col border-r border-gray-200 overflow-hidden"
            >
              {/* Sidebar Header */}
              <div className="p-6 border-b border-gray-100">
                <div className="flex items-center gap-3 mb-4">
                  <div className="w-8 h-8 bg-gradient-to-br from-blue-500 to-purple-600 rounded-lg flex items-center justify-center">
                    <Sparkles className="w-4 h-4 text-white" />
                  </div>
                  <h1 className="text-lg font-semibold text-gray-900">Aquatiq Chat</h1>
                </div>

                <button
                  onClick={handleNewChat}
                  className="w-full flex items-center gap-3 px-4 py-3 bg-gray-900 text-white rounded-l hover:bg-gray-800 transition-colors"
                >
                  <Plus className="w-4 h-4" />
                  <span>New Chat</span>
                </button>
              </div>

              {/* Recent Chats */}
              <div className="flex-1 overflow-y-auto p-6">
                {isLoading && chats.length === 0 ? (
                  <div className="flex items-center justify-center py-8 text-gray-500">
                    Loading chats...
                  </div>
                ) : chats.length > 0 ? (
                  <div className="space-y-2">
                    <h2 className="text-sm font-medium text-gray-700 mb-3">Recent Chats</h2>
                    {chats.map((chat) => (
                      <button
                        key={chat.id}
                        onClick={() => handleChatClick(chat)}
                        className={`w-full text-left p-3 rounded-xl transition-colors ${currentChat?.id === chat.id
                          ? 'bg-blue-50 text-blue-700 border border-blue-200'
                          : 'hover:bg-gray-50 text-gray-700'
                          }`}
                      >
                        <div className="flex items-start gap-3">
                          <MessageSquare className="w-4 h-4 mt-0.5 flex-shrink-0" />
                          <div className="min-w-0">
                            <p className="text-sm font-medium truncate">{chat.title}</p>
                            <p className="text-xs text-gray-500 mt-1">
                              {chat.messages.length} messages
                            </p>
                          </div>
                        </div>
                      </button>
                    ))}
                  </div>
                ) : (
                  <div className="text-center py-8 text-gray-500">
                    <MessageSquare className="w-8 h-8 mx-auto mb-3 opacity-50" />
                    <p className="text-sm">No chats yet</p>
                    <p className="text-xs mt-1">Start a new conversation!</p>
                  </div>
                )}
              </div>
            </m.div>
          )}
        </AnimatePresence>

        {/* Main Chat Area */}
        <div className="flex-1 flex flex-col min-w-0">
          {showWelcomeScreen && (
            <div className="flex-1 flex items-center justify-center p-8">
              <div className="text-center max-w-md">
                <div className="w-16 h-16 bg-gradient-to-br from-blue-500 to-purple-600 rounded-2xl flex items-center justify-center mx-auto mb-6">
                  <Sparkles className="w-8 h-8 text-white" />
                </div>
                <h2 className="text-2xl font-bold text-gray-900 mb-3">Welcome to Aquatiq Chat</h2>
                <p className="text-gray-600 mb-8">
                  Your intelligent AI assistant powered by Azure OpenAI.
                  Ask questions, generate images, or start a conversation!
                </p>

                {/* Quick Actions */}
                <div className="grid grid-cols-1 gap-3 mb-8">
                  <button
                    onClick={() => setMessage('Hello! What can you help me with today?')}
                    className="p-4 bg-white rounded-xl border border-gray-200 hover:border-blue-300 hover:bg-blue-50 transition-colors text-left group"
                  >
                    <div className="flex items-center gap-3">
                      <div className="w-8 h-8 bg-blue-100 rounded-lg flex items-center justify-center group-hover:bg-blue-200">
                        💬
                      </div>
                      <span className="text-sm font-medium text-gray-700 group-hover:text-blue-700">
                        Start a conversation
                      </span>
                    </div>
                  </button>

                  <button
                    onClick={() => setMessage('Generate an image of a futuristic city at sunset')}
                    className="p-4 bg-white rounded-xl border border-gray-200 hover:border-purple-300 hover:bg-purple-50 transition-colors text-left group"
                  >
                    <div className="flex items-center gap-3">
                      <div className="w-8 h-8 bg-purple-100 rounded-lg flex items-center justify-center group-hover:bg-purple-200">
                        🎨
                      </div>
                      <span className="text-sm font-medium text-gray-700 group-hover:text-purple-700">
                        Generate an image
                      </span>
                    </div>
                  </button>
                </div>

                {/* Recent Chats Strip on Welcome Screen */}
                {chats.length > 0 && (
                  <RecentChatsStrip
                    chats={chats.slice(0, 4)}
                    onOpen={handleChatClick}
                  />
                )}
              </div>
            </div>
          )}

          {/* Chat View */}
          <AnimatePresence mode="wait">
            {(currentChat && currentChat.messages.length > 0) && (
              <m.div
                key={currentChat.id}
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="flex-1"
              >
                <ChatView
                  currentChat={currentChat}
                  message={message}
                  setMessage={setMessage}
                  onMessageSubmit={handleMessageSubmit}
                  isTyping={isTyping}
                  onBack={handleBack}
                />
              </m.div>
            )}
          </AnimatePresence>

          {/* Input Area for Welcome Screen */}
          {showWelcomeScreen && (
            <div className="border-t border-gray-200">
              <div className="max-w-4xl mx-auto px-6 py-6">
                <form onSubmit={handleMessageSubmit} className="flex gap-3">
                  <div className="flex-1">
                    <input
                      type="text"
                      value={message}
                      onChange={(e) => setMessage(e.target.value)}
                      placeholder="Ask me anything or request an image..."
                      className="w-full px-4 py-3 border border-gray-300 rounded-xl focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                      disabled={isTyping}
                    />
                  </div>
                  <button
                    type="submit"
                    disabled={!message.trim() || isTyping}
                    className="px-6 py-3 bg-blue-600 text-white rounded-xl hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  >
                    {isTyping ? 'Sending...' : 'Send'}
                  </button>
                </form>
              </div>
            </div>
          )}

          {/* Error Display */}
          {error && (
            <m.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              className="fixed bottom-4 right-4 bg-red-100 border border-red-200 text-red-700 px-4 py-3 rounded-xl shadow-lg"
            >
              <div className="flex items-center gap-2">
                <span className="text-sm">{error}</span>
                <button
                  onClick={() => setError(null)}
                  className="text-red-500 hover:text-red-700"
                >
                  ×
                </button>
              </div>
            </m.div>
          )}
        </div>
      </div>
    </div>
  );
}
