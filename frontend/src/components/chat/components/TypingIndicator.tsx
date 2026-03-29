'use client';

import { m } from 'framer-motion';
import { Bot } from 'lucide-react';
import Image from 'next/image';
import { useI18n } from '@/components/chat/hooks/i18n';

// Types
interface LoaderProps {
  className?: string;
}

interface TypingIndicatorProps {
  botAvatarSrc?: string;
  botName?: string;
}

// LoaderOne Component with proper types
const LoaderOne: React.FC<LoaderProps> = ({ className = "" }) => {
  const dotVariants = {
    scale: [1, 1.5, 1],
    opacity: [1, 0.8, 1]
  };

  const dotTransition = {
    duration: 0.8,
    repeat: Infinity,
    ease: [0.4, 0, 0.6, 1] as const
  };

  return (
    <div className={`flex space-x-2 ${className}`}>
      <div className="flex space-x-1">
        <m.div
          className="w-2 h-2 bg-blue-500 rounded-full"
          animate={dotVariants}
          transition={{ ...dotTransition, delay: 0 }}
        />
        <m.div
          className="w-2 h-2 bg-blue-500 rounded-full"
          animate={dotVariants}
          transition={{ ...dotTransition, delay: 0.2 }}
        />
        <m.div
          className="w-2 h-2 bg-blue-500 rounded-full"
          animate={dotVariants}
          transition={{ ...dotTransition, delay: 0.4 }}
        />
      </div>
    </div>
  );
};

export const TypingIndicator: React.FC<TypingIndicatorProps> = ({ 
  botAvatarSrc, 
  botName = "AI" 
}) => {
  const { t } = useI18n();
  const containerVariants = {
    initial: { opacity: 0, y: 20 },
    animate: { opacity: 1, y: 0 },
    exit: { opacity: 0, y: -20 }
  };

  return (
    <m.div
      variants={containerVariants}
      initial="initial"
      animate="animate"
      exit="exit"
      transition={{ duration: 0.3 }}
      className="flex gap-4 mb-8"
    >
      {/* AI Avatar */}
      <div className="w-10 h-10 rounded-full overflow-hidden bg-gray-100 border-2 border-gray-200 flex-shrink-0">
        {botAvatarSrc ? (
          <Image
            src={botAvatarSrc}
            alt={`${botName} avatar`}
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

      {/* Typing Bubble with Loader */}
      <div className="bg-white border border-gray-200 rounded-2xl rounded-bl-sm px-6 py-4 shadow-sm">
        <div className="flex items-center gap-3">
          <span className="text-sm text-gray-600">{botName} {t('chat.thinking')}</span>
          <LoaderOne />
        </div>
      </div>
    </m.div>
  );
};