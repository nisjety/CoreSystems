'use client';

import React, { useState, useEffect } from 'react';
import { m, AnimatePresence } from 'framer-motion';
import { Copy, ThumbsUp, ThumbsDown, Share, User, Bot, Check, RotateCcw } from 'lucide-react';
import Image from 'next/image';
import { useI18n } from '@/components/chat/hooks/i18n';

// Types
interface Message {
  id: string;
  content: string;
  sender: 'user' | 'assistant' | 'system';
  timestamp: Date;
  role?: 'user' | 'assistant' | 'system';
}

interface MessageBubbleProps {
  message: Message;
  avatarSrc?: string;
  avatarAlt?: string;
}

interface TextGenerateEffectProps {
  words: string;
  className?: string;
}

// Text Generate Effect Component
const TextGenerateEffect: React.FC<TextGenerateEffectProps> = ({ words, className = "" }) => {
  const [displayedText, setDisplayedText] = useState<string>('');
  const [currentIndex, setCurrentIndex] = useState<number>(0);

  useEffect(() => {
    if (currentIndex < words.length) {
      const timeout = setTimeout(() => {
        setDisplayedText(words.slice(0, currentIndex + 1));
        setCurrentIndex(currentIndex + 1);
      }, 20);
      return () => clearTimeout(timeout);
    }
  }, [currentIndex, words]);

  return (
    <span className={className}>
      {displayedText}
      {currentIndex < words.length && (
        <m.span
          animate={{ opacity: [1, 0] }}
          transition={{ duration: 0.8, repeat: Infinity, repeatType: "reverse" }}
          className="inline-block w-0.5 h-5 bg-gray-600 ml-1"
        />
      )}
    </span>
  );
};

// Helper function for time formatting
const formatTimeAgo = (date: Date, t: (key: string, vars?: Record<string, string | number>) => string): string => {
  const now = new Date();
  const diffInSeconds = Math.floor((now.getTime() - date.getTime()) / 1000);
  
  if (diffInSeconds < 60) return t('chat.justNow');
  if (diffInSeconds < 3600) return t('chat.timeAgo.minutes', { count: Math.floor(diffInSeconds / 60) });
  if (diffInSeconds < 86400) return t('chat.timeAgo.hours', { count: Math.floor(diffInSeconds / 3600) });
  return t('chat.timeAgo.days', { count: Math.floor(diffInSeconds / 86400) });
};

export const MessageBubble: React.FC<MessageBubbleProps> = ({ 
  message, 
  avatarSrc,
  avatarAlt = "Avatar"
}) => {
  const [copied, setCopied] = useState<boolean>(false);
  const [showActions, setShowActions] = useState<boolean>(false);
  const { t } = useI18n();
  const isUser = message.sender === 'user';

  const handleCopy = async (): Promise<void> => {
    try {
      await navigator.clipboard.writeText(message.content);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch (error) {
      console.error(t('error.copy'), error);
    }
  };

  const handleLike = (): void => {
    // TODO: Implement like functionality
    console.log(t('chat.action.like'), message.id);
  };

  const handleDislike = (): void => {
    // TODO: Implement dislike functionality
    console.log(t('chat.action.dislike'), message.id);
  };

  const handleRegenerate = (): void => {
    // TODO: Implement regenerate functionality
    console.log(t('chat.action.regenerate'), message.id);
  };

  const handleShare = (): void => {
    // TODO: Implement share functionality
    console.log(t('chat.action.share'), message.id);
  };

  return (
    <m.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
      className={`flex gap-4 mb-8 group ${isUser ? 'justify-end' : 'justify-start'}`}
      onMouseEnter={() => setShowActions(true)}
      onMouseLeave={() => setShowActions(false)}
    >
      {/* AI Avatar - Left side */}
      {!isUser && (
        <div className="flex-shrink-0 w-10 h-10 rounded-full overflow-hidden bg-gray-100 border-2 border-gray-200">
          {avatarSrc ? (
            <Image
              src={avatarSrc}
              alt={avatarAlt}
              width={40}
              height={40}
              className="w-full h-full object-cover"
            />
          ) : (
            <div className="w-full h-full bg-gradient-to-br from-purple-500 to-blue-600 flex items-center justify-center">
              <Bot className="w-5 h-5 text-white" />
            </div>
          )}
        </div>
      )}

      <div className={`flex flex-col ${isUser ? 'items-end' : 'items-start'} max-w-2xl lg:max-w-3xl`}>
        {/* Message Bubble */}
        <div
          className={`relative px-6 py-4 rounded-2xl ${
            isUser
              ? 'bg-blue-500 text-white rounded-br-sm shadow-md'
              : 'bg-white text-black border border-gray-200 rounded-bl-sm shadow-sm'
          }`}
        >
          {/* Message Content */}
          {isUser ? (
            <p className="text-base leading-relaxed whitespace-pre-wrap">
              {message.content}
            </p>
          ) : (
            <div className="text-base leading-relaxed">
              <TextGenerateEffect 
                words={message.content}
                className="text-black"
              />
            </div>
          )}

          {/* Action Buttons for AI Messages */}
          {!isUser && (
            <AnimatePresence>
              {showActions && (
                <m.div
                  initial={{ opacity: 0, y: 8 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: 8 }}
                  transition={{ duration: 0.2 }}
                  className="absolute -bottom-12 left-0 flex items-center gap-1 bg-white rounded-xl px-3 py-2 shadow-lg border border-gray-200"
                >
                  <button 
                    onClick={handleCopy}
                    className="p-2 hover:bg-gray-100 rounded-lg transition-colors" 
                    title={copied ? t('chat.action.copied') : t('chat.action.copy')}
                    type="button"
                  >
                    {copied ? (
                      <Check className="w-4 h-4 text-green-600" />
                    ) : (
                      <Copy className="w-4 h-4 text-gray-600" />
                    )}
                  </button>
                  <button 
                    onClick={handleLike}
                    className="p-2 hover:bg-gray-100 rounded-lg transition-colors" 
                    title={t('chat.action.like')}
                    type="button"
                  >
                    <ThumbsUp className="w-4 h-4 text-gray-600" />
                  </button>
                  <button 
                    onClick={handleDislike}
                    className="p-2 hover:bg-gray-100 rounded-lg transition-colors" 
                    title={t('chat.action.dislike')}
                    type="button"
                  >
                    <ThumbsDown className="w-4 h-4 text-gray-600" />
                  </button>
                  <button 
                    onClick={handleRegenerate}
                    className="p-2 hover:bg-gray-100 rounded-lg transition-colors" 
                    title={t('chat.action.regenerate')}
                    type="button"
                  >
                    <RotateCcw className="w-4 h-4 text-gray-600" />
                  </button>
                  <button 
                    onClick={handleShare}
                    className="p-2 hover:bg-gray-100 rounded-lg transition-colors" 
                    title={t('chat.action.share')}
                    type="button"
                  >
                    <Share className="w-4 h-4 text-gray-600" />
                  </button>
                </m.div>
              )}
            </AnimatePresence>
          )}
        </div>

        {/* Timestamp */}
        <div className={`text-xs mt-2 px-1 ${isUser ? 'text-gray-500' : 'text-gray-400'}`}>
          {formatTimeAgo(message.timestamp, t)}
        </div>
      </div>

      {/* User Avatar - Right side */}
      {isUser && (
        <div className="flex-shrink-0 w-10 h-10 rounded-full overflow-hidden bg-gray-100 border-2 border-blue-200">
          {avatarSrc ? (
            <Image
              src={avatarSrc}
              alt={avatarAlt}
              width={40}
              height={40}
              className="w-full h-full object-cover"
            />
          ) : (
            <div className="w-full h-full bg-gradient-to-br from-blue-500 to-indigo-600 flex items-center justify-center">
              <User className="w-5 h-5 text-white" />
            </div>
          )}
        </div>
      )}
    </m.div>
  );
};